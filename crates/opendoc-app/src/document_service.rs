use crate::{
    document_tree::find_inline_in_blocks, now_ms, rich_document_operation_kind, AppApiError,
    AppOperationEnvelope, AppOperationRecord,
};
use opendoc_core::Document;
use opendoc_merge::{
    batch_inverse_capture_order, discarded_by_a_later_whole_run_write, invert_operation,
    merge_operations, merge_operations_into, ActorId, CausalContext, Inversion, Operation,
    OperationId, OperationKind,
};
use std::collections::BTreeMap;

/// Turns a local gesture into a typed operation, a journal record and an
/// envelope that carries both.
///
/// It advances **two** counters, and the difference between them is the point.
/// `next_envelope_seq` numbers the journal entry; `next_operation_seq` numbers
/// the `OperationId` inside it. Only this service advances the second one,
/// because only this service mints operations — which is exactly why an undo
/// marker or a blob upload no longer leaves a hole in an actor's operation
/// sequence for a collaboration service to refuse.
/// Why an inverse captured during a batch cannot be trusted: the fold that
/// produces the intermediate states stopped at an earlier operation, so this
/// one was never taken against a state the batch reached.
const BATCH_INVERSE_CAPTURE_INCOMPLETE: &str = "batch-inverse-capture-incomplete";

pub(crate) struct DocumentOperationService<'a> {
    actor_id: &'a str,
    document: &'a mut Document,
    operation_journal: &'a mut Vec<AppOperationRecord>,
    operation_envelopes: &'a mut Vec<AppOperationEnvelope>,
    next_envelope_seq: &'a mut u64,
    next_operation_seq: &'a mut u64,
    inverses: &'a mut BTreeMap<OperationId, Inversion>,
}

impl<'a> DocumentOperationService<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        actor_id: &'a str,
        document: &'a mut Document,
        operation_journal: &'a mut Vec<AppOperationRecord>,
        operation_envelopes: &'a mut Vec<AppOperationEnvelope>,
        next_envelope_seq: &'a mut u64,
        next_operation_seq: &'a mut u64,
        inverses: &'a mut BTreeMap<OperationId, Inversion>,
    ) -> Self {
        Self {
            actor_id,
            document,
            operation_journal,
            operation_envelopes,
            next_envelope_seq,
            next_operation_seq,
            inverses,
        }
    }

    /// Applies one operation, or **fails loudly**.
    ///
    /// `_operation_kind` is accepted for call-site symmetry but deliberately
    /// ignored: the journal/envelope kind is derived from the payload by
    /// [`rich_document_operation_kind`], so a record can never disagree with
    /// the typed operation it carries. The literals still passed by call
    /// sites are redundant and can be deleted.
    ///
    /// A refused merge used to be swallowed here: the `if let Ok(..)` dropped
    /// the error, both counters stayed put, nothing was journalled, no inverse
    /// was captured, and the caller — all the way out to `dispatch_command` —
    /// was told the command succeeded. The user's gesture silently did
    /// nothing, which is the worst of the three possible outcomes: worse than
    /// refusing it and worse than applying it. The error now leaves the
    /// document, the journal and both counters exactly as they were and
    /// travels back to the caller as an `AppApiError`.
    pub(crate) fn apply(
        &mut self,
        _operation_kind: &str,
        summary: &str,
        kind: OperationKind,
    ) -> Result<(), AppApiError> {
        // Inverted *before* the merge, against the document the operation was
        // written against. That is the only moment the state a delete removed
        // or a set overwrote still exists, which is why undo captures here and
        // not when the user asks for it. ADR 0017.
        let inversion = invert_operation(self.document, &kind);
        let op = Operation::in_context(
            self.operation_id(*self.next_operation_seq),
            kind,
            self.causal_context(),
        );
        // Both counters advance only if the merge accepted the operation, so a
        // refused gesture consumes neither identity.
        let result = merge_operations(self.document, &[vec![op.clone()]]).map_err(|error| {
            AppApiError::Model(format!(
                "the edit was refused because it would not merge into the document: {error}"
            ))
        })?;
        *self.document = result.document;
        self.discard_stale_inverses(op.id.seq);
        *self.next_operation_seq += 1;
        self.inverses.insert(op.id.clone(), inversion);
        self.push_document_operation(summary, op);
        Ok(())
    }

    /// Drop every captured inverse that cannot belong to the journal this
    /// service is writing into: one for another actor, or one at or above the
    /// sequence number now being minted.
    ///
    /// The map's invariant is "one entry per operation *this* replica minted
    /// into *this* journal", and this is where it is kept, because this is the
    /// only place that writes it. Closing a document resets the operation
    /// counter to 1 and empties the journal, and joining a collaboration
    /// session also adopts a new actor id — so without this the entries from
    /// the previous document would sit in the map for the life of the process,
    /// holding on to the content of every block that had been deleted in it.
    ///
    /// In ordinary editing it is a no-op: the minted sequence number is always
    /// above every entry already there. It only ever removes anything after a
    /// reset, which is exactly when it should.
    ///
    /// It is a *bound*, not a substitute for clearing the map where the rest of
    /// the document state is reset (`clear_edit_history`). What it guarantees
    /// is that the residue cannot accumulate across documents: the first edit
    /// in the new document discards all of it.
    fn discard_stale_inverses(&mut self, minting_seq: u64) {
        let actor = ActorId(self.actor_id.to_string());
        let stale = self
            .inverses
            .keys()
            .filter(|id| id.actor != actor || id.seq >= minting_seq)
            .cloned()
            .collect::<Vec<_>>();
        for id in stale {
            self.inverses.remove(&id);
        }
    }

    /// The `&str` kind in each tuple is ignored; see [`Self::apply`].
    ///
    /// All-or-nothing, and loud on failure for the same reason [`Self::apply`]
    /// is: the batch is one gesture, so half of it landing would be a document
    /// state the user never asked for and no undo step describes. The scratch
    /// merge that captures each operation's inverse is held to the same bar —
    /// swallowing *it* left the batch with an inverse captured against a
    /// document the operation had not been applied to, so an undo would have
    /// restored the wrong state.
    pub(crate) fn apply_batch(
        &mut self,
        operations: Vec<(&str, &str, OperationKind)>,
    ) -> Result<(), AppApiError> {
        let batch_size = operations.len();
        // Which of these the merge below will throw away.
        //
        // ADR 0007's whole-run reset is a property of the operation *set*: a
        // character operation loses to a later operation that writes the whole
        // of its run, so the merge never applies it. The fold further down
        // merges one operation at a time, where each is alone in its own merge
        // and there is no later write for it to lose to — so without this it
        // would apply an operation the batch discards, and the next
        // operation's inverse would capture a previous value describing a
        // state the document was never in. Undoing such a step put the
        // discarded characters *back*: an undo that adds text. ADR 0017,
        // amendment "the fold applies the batch's own discards".
        let discarded =
            discarded_by_a_later_whole_run_write(operations.iter().map(|(_, _, kind)| kind));
        let capture_order = batch_inverse_capture_order(operations.iter().map(|(_, _, kind)| kind));
        let mut merge_ops = Vec::new();
        let mut records = Vec::new();
        // Each operation in a batch observes the ones before it, because it
        // does: they are applied in order and the later one was generated
        // knowing the earlier. Stating that in the clock rather than only in
        // the Lamport timestamp costs nothing — `Operation::observes` already
        // implies an actor's own lower sequence numbers, so the merge is
        // unchanged — and it is what a collaboration service checks. A server
        // that sees a timestamp more than one above everything the clock names
        // has to refuse it, or a client can claim `u64::MAX` and win every
        // last-writer-wins contest for ever.
        //
        // The clock is **carried**, not rebuilt. It used to be recomputed by
        // `CausalContext::observing` over a freshly cloned copy of every
        // operation in the journal, once per operation in the batch — an
        // O(journal x batch) walk, and O(journal) `Operation` clones, to reach
        // a map with one entry per actor. Extending it by the operation just
        // minted is exact rather than approximate: `VectorClock::observe` and
        // the Lamport rule are both maxima, and the operation being minted
        // carries `lamport = highest seen + 1`, so the next context's
        // timestamp is this one's plus one and its clock is this one's plus
        // this id.
        let mut context = self.causal_context();
        // Each operation of a batch is inverted against the document as it
        // stood immediately before *it*, not before the batch: the second
        // operation of "delete this text, then delete the block it was in"
        // must capture the block without the text, or undoing the batch would
        // restore the text twice. That needs the intermediate states, so the
        // capture folds the batch into a scratch document one operation at a
        // time. The batch itself is still merged in one pass.
        //
        // The fold is written *into* the scratch document rather than through
        // a fresh copy of it per step (`merge_operations_into` rather than
        // `merge_operations`). The states are the same — it is the same
        // merge — but copying the document, and freeing the copy it replaced,
        // once per operation was together the larger half of what marking text
        // across a long document cost: a 1,500-operation batch copied and
        // freed a 1,500-block document 1,500 times.
        //
        // `None` means "still the document itself". The scratch is only a
        // separate value once something has been folded into it, and for a
        // batch of one operation nothing ever is — so a keystroke no longer
        // copies the document to invert one operation against a copy that is
        // equal to it.
        let mut scratch: Option<Document> = None;
        let mut inversions = (0..batch_size).map(|_| None).collect::<Vec<_>>();
        // The last offset whose inverse was captured against the state the
        // batch really reached. `None` means every one of them was.
        let mut inverse_capture_ends_after: Option<usize> = None;
        for (offset, (_operation_kind, summary, kind)) in operations.into_iter().enumerate() {
            let seq = *self.next_operation_seq + offset as u64;
            // Once the fold has stopped tracking the batch there is nothing
            // true it can say about a later operation, so the refusal is
            // minted here rather than inferred afterwards. That is what lets
            // the fold stop rather than keep folding a scratch nobody may
            // read: an inverse is never taken from a scratch document that
            // stopped advancing, because after that point none is taken.
            let op = Operation::in_context(self.operation_id(seq), kind, context.clone());
            context.observed.observe(&op.id);
            context.lamport += 1;
            // The scratch fold is *inverse capture*, not application, and a
            // step of it failing does not mean the gesture is wrong: a batch
            // is atomic, so a state halfway through it can be one
            // `Document::validate()` rejects — bolding across a page break
            // momentarily empties a run — while the batch as a whole is
            // valid. Refusing here would throw away a gesture the merge below
            // is about to accept.
            //
            // It is not nothing, either, and this is the part that used to be
            // dropped. `invert_operation` above already captured *this*
            // operation's inverse against the state before it, so that one is
            // sound; every inverse captured *after* this point would be taken
            // against a scratch that stopped advancing, and undoing the step
            // with those would restore a state the document was never in. So
            // the batch stops being invertible from the next operation on, and
            // the step falls back to the mechanism that is still correct — the
            // whole-state snapshot, or, in a session where a snapshot is not
            // available, the skip that `step_edit_history` reports.
            //
            // The **last** operation's fold is skipped outright. Its own
            // inverse was captured above, before it, and there is no later
            // offset for a failure to disqualify, so the state it would
            // produce is one nothing reads. That is what makes a
            // single-operation batch — a keystroke, an inserted block, most of
            // what `apply_batch` is actually called with — cost one merge
            // instead of two.
            //
            // A discarded operation is skipped rather than folded, and that is
            // not a failure of the fold: the batch merge discards it too, so
            // skipping it is what makes the fold's states states the batch
            // really passes through.
            merge_ops.push(op.clone());
            records.push((summary.to_string(), op));
        }
        for (capture_rank, offset) in capture_order.into_iter().enumerate() {
            let op = &records[offset].1;
            let capturing = inverse_capture_ends_after.is_none();
            inversions[offset] = Some(if capturing {
                let before = scratch.as_ref().unwrap_or(self.document);
                match &op.kind {
                    OperationKind::InsertText { inline_id, .. }
                    | OperationKind::DeleteText { inline_id, .. }
                        if find_inline_in_blocks(&before.blocks, inline_id).is_none() =>
                    {
                        // Text edits are the merge's final pass. If an earlier
                        // structural phase has already removed their run, the
                        // edit never landed. `invert_text_operations` still
                        // has to support a causally earlier edit whose later
                        // delete captured that edited content, but this local
                        // batch cannot safely distinguish that history with a
                        // per-operation inverse. Use the established snapshot
                        // fallback instead of synthesising an inverse that
                        // changes the restored source run.
                        Inversion::Irreversible("batch-text-target-removed-before-deferred-pass")
                    }
                    _ => invert_operation(before, &op.kind),
                }
            } else {
                Inversion::Irreversible(BATCH_INVERSE_CAPTURE_INCOMPLETE)
            });
            // The final state is not read by any later inverse capture.
            // All earlier states must follow the merge's deferred-pass order.
            if capturing && capture_rank + 1 < batch_size && !discarded[offset] {
                let scratch = match &mut scratch {
                    Some(scratch) => scratch,
                    none => none.insert({
                        opendoc_merge::instrument::count_document_copy();
                        self.document.clone()
                    }),
                };
                if merge_operations_into(scratch, &[vec![op.clone()]]).is_err() {
                    inverse_capture_ends_after = Some(offset);
                }
            }
        }
        let result = merge_operations(self.document, &[merge_ops]).map_err(|error| {
            AppApiError::Model(format!(
                "the edit was refused because it would not merge into the document: {error}"
            ))
        })?;
        *self.document = result.document;
        if let Some((_, op)) = records.first() {
            self.discard_stale_inverses(op.id.seq);
        }
        *self.next_operation_seq += records.len() as u64;
        // Every inverse is committed as the fold produced it. The fold is the
        // only thing that decides which of them are sound: once it has
        // stopped, it mints `BATCH_INVERSE_CAPTURE_INCOMPLETE` for the rest of
        // the batch itself. Re-deciding that here from
        // `inverse_capture_ends_after` would be a second copy of one rule —
        // one that agrees with the first by construction, so nothing could
        // ever catch the two disagreeing.
        for ((summary, op), inversion) in records.into_iter().zip(inversions) {
            // `capture_order` is a stable partition of every input index, so
            // every slot was filled above. Keeping it as an Option while the
            // fold runs prevents a second, gesture-order implementation of
            // the deferred-pass rule.
            let inversion = inversion.expect("every batch operation is captured once");
            self.inverses.insert(op.id.clone(), inversion);
            self.push_document_operation(&summary, op);
        }
        Ok(())
    }

    /// The causal context a locally generated operation is written in: every
    /// operation this replica has already applied, local or merged in from
    /// another actor. Without it the merge would read this replica's edits as
    /// concurrent with work it demonstrably already had, and text offsets
    /// would be re-anchored against a document it never saw. See
    /// `docs/adr/0007-causal-ordering-and-text-convergence.md`.
    ///
    fn causal_context(&self) -> CausalContext {
        CausalContext::observing(
            self.operation_envelopes
                .iter()
                .filter_map(|envelope| envelope.operation.as_ref()),
        )
    }

    /// The envelope's own sequence number, taken from the envelope counter —
    /// *not* from `op.id.seq`. Those two agreed when there was one counter;
    /// keeping them equal was what forced every non-operation envelope to
    /// consume an operation id.
    fn push_document_operation(&mut self, summary: &str, op: Operation) {
        let seq = *self.next_envelope_seq;
        *self.next_envelope_seq += 1;
        let record = AppOperationRecord {
            actor: self.actor_id.to_string(),
            seq,
            kind: rich_document_operation_kind(&op.kind).to_string(),
            summary: summary.to_string(),
            created_at_ms: now_ms(),
        };
        self.operation_journal.push(record.clone());
        self.operation_envelopes.push(AppOperationEnvelope {
            record,
            operation: Some(op),
            spreadsheet: None,
            blob: None,
        });
    }

    fn operation_id(&self, seq: u64) -> OperationId {
        OperationId {
            actor: ActorId(self.actor_id.to_string()),
            seq,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{OpenDocApp, OperationKind};
    use serde_json::json;

    /// Every captured inverse holds the state its operation destroyed — for a
    /// delete, the whole block — so a map that grows for the life of the
    /// process is a leak with the document's content in it.
    ///
    /// `close_document` empties the journal and resets the operation counter to
    /// 1, so the ids of the next document collide with the old entries and
    /// overwrite them one at a time. That makes the leak invisible rather than
    /// absent. `discard_stale_inverses` drops the lot at the first edit of the
    /// new document instead.
    #[test]
    fn captured_inverses_do_not_accumulate_across_documents() {
        let mut app = OpenDocApp::new_empty_document();
        app.dispatch_command("create_document", json!({ "title": "First" }))
            .expect("create");
        for index in 0..6 {
            app.dispatch_command("add_paragraph", json!({ "text": format!("p{index}") }))
                .expect("paragraph");
        }
        let ids: Vec<String> = app
            .source_document()
            .blocks
            .iter()
            .map(|block| block.id.to_string())
            .collect();
        for id in ids.iter().take(3) {
            app.dispatch_command("delete_block", json!({ "blockId": id }))
                .expect("delete");
        }
        let captured = app.operation_inverses.len();
        assert!(
            captured >= 9,
            "the fixture has to capture several inverses, got {captured}"
        );

        app.dispatch_command("close_document", json!({ "discardUnsavedChanges": true }))
            .expect("close");
        app.dispatch_command("create_document", json!({ "title": "Second" }))
            .expect("create");
        app.dispatch_command("add_paragraph", json!({ "text": "fresh" }))
            .expect("paragraph");

        let operations = app.local_operations_after(0).len();
        assert_eq!(
            app.operation_inverses.len(),
            operations,
            "the new document's inverse map holds one entry per operation *it* \
             minted and nothing from the document that was closed"
        );
    }

    /// A batch is atomic, so a state halfway through one can be a document
    /// `Document::validate()` rejects while the batch as a whole is fine —
    /// here, a block that briefly shares an inline id with the block the next
    /// operation removes. Bolding across a page break is the same shape in
    /// the editor, and is how this was found: making the scratch fold fatal
    /// broke that gesture in Chrome while every Rust test stayed green.
    ///
    /// So the scratch fold cannot refuse the gesture. What it can no longer
    /// do is claim the inverses captured *after* the rejected step are sound:
    /// they were taken against a scratch that stopped advancing, so undoing
    /// with them would restore a state the document was never in. Those are
    /// recorded as `Irreversible`, which routes the step to the whole-state
    /// snapshot — the mechanism that is still correct — instead of to a wrong
    /// undo. That is the difference between handling the failure and dropping
    /// it, which is what the old `if let Ok(..)` did.
    #[test]
    fn a_batch_whose_middle_state_is_invalid_still_lands_and_stops_trusting_its_inverses() {
        use opendoc_core::{Block, BlockKind, Inline, InsertPosition, StableId};

        let mut app = OpenDocApp::new_empty_document();
        app.dispatch_command("create_document", json!({ "title": "Doc" }))
            .expect("create");
        app.dispatch_command("add_paragraph", json!({ "text": "alpha" }))
            .expect("paragraph");
        let old_block =
            app.source_document()
                .blocks
                .iter()
                .find(|block| {
                    block.content.iter().any(
                        |inline| matches!(inline, Inline::Text { text, .. } if text == "alpha"),
                    )
                })
                .expect("the paragraph holding alpha")
                .clone();
        let shared_inline_id = match &old_block.content[0] {
            Inline::Text { id, .. } => id.clone(),
            other => panic!("expected a text run, got {other:?}"),
        };
        // The replacement reuses the run's identity, which is legal only once
        // the block holding it is gone — so the state between the two
        // operations is one no document may be in, and the state after them
        // is fine.
        let replacement = Block {
            id: StableId::new("block"),
            kind: BlockKind::Paragraph,
            content: vec![Inline::Text {
                id: shared_inline_id,
                text: "beta".to_string(),
                marks: Vec::new(),
            }],
            properties: Default::default(),
        };

        app.apply_batch(vec![
            (
                "insert-block",
                "replace the paragraph",
                OperationKind::InsertBlock {
                    position: InsertPosition::Last,
                    block: replacement,
                },
            ),
            (
                "delete-block",
                "replace the paragraph",
                OperationKind::DeleteBlock {
                    block_id: old_block.id.clone(),
                },
            ),
        ])
        .expect("the batch as a whole is valid, so it lands");

        let text = app.source_document().visible_text();
        assert!(text.contains("beta"), "the gesture was applied: {text:?}");
        assert!(!text.contains("alpha"), "and completely: {text:?}");

        let unsound = app
            .operation_inverses
            .values()
            .filter(|inversion| !inversion.is_expressible())
            .count();
        assert_eq!(
            unsound,
            1,
            "the one inverse captured after the rejected middle state is named, \
             not trusted: {:?}",
            app.operation_inverses.values().collect::<Vec<_>>()
        );
    }

    /// A merge the document refuses must be **impossible to lose**.
    ///
    /// It used to be dropped on the floor: `if let Ok(result) = ...` with no
    /// `else`, both `apply` and `apply_batch` returning `()`. The gesture was
    /// not journalled, neither counter moved, no inverse was captured, no
    /// warning was raised — and `dispatch_command` answered `Ok`. The user
    /// pressed a key and nothing happened, with nothing anywhere to say why.
    ///
    /// The failure is induced here rather than found: every operation the
    /// vocabulary has guards its own payload, which is why there is no
    /// "poison" operation to reach for. What is under test is the plumbing —
    /// that a refusal travels, and that a refused gesture leaves the document,
    /// the journal and both counters exactly as they were.
    #[test]
    fn a_merge_the_document_refuses_reaches_the_caller() {
        let mut app = OpenDocApp::new_empty_document();
        app.dispatch_command("create_document", json!({ "title": "Doc" }))
            .expect("create");
        app.dispatch_command("add_paragraph", json!({ "text": "kept" }))
            .expect("paragraph");

        // A document `Document::validate()` rejects, so the merge that folds
        // the next operation in cannot produce a valid result.
        app.document.title = " leading space".to_string();
        let journal = app.operation_journal.len();
        let envelopes = app.operation_envelopes.len();
        let operation_seq = app.next_operation_seq;
        let envelope_seq = app.next_envelope_seq;
        let inverses = app.operation_inverses.len();

        let error = app
            .dispatch_command("add_paragraph", json!({ "text": "lost" }))
            .expect_err("a refused merge is an error, not a silent no-op");
        assert!(matches!(error, crate::AppApiError::Model(_)), "{error:?}");
        assert!(
            !app.source_document().visible_text().contains("lost"),
            "the document is untouched"
        );
        assert_eq!(app.operation_journal.len(), journal, "nothing journalled");
        assert_eq!(app.operation_envelopes.len(), envelopes);
        assert_eq!(app.next_operation_seq, operation_seq, "no id was consumed");
        assert_eq!(app.next_envelope_seq, envelope_seq);
        assert_eq!(app.operation_inverses.len(), inverses);
    }

    /// The same for a batch, which is the path a paste, a replace-all or a
    /// multi-block style change takes. A batch is one gesture, so it is
    /// all-or-nothing: half of it landing would be a state the user never
    /// asked for and no undo step describes.
    #[test]
    fn a_refused_batch_lands_none_of_itself_and_says_so() {
        let mut app = OpenDocApp::new_empty_document();
        app.dispatch_command("create_document", json!({ "title": "Doc" }))
            .expect("create");
        app.dispatch_command("add_paragraph", json!({ "text": "alpha beta" }))
            .expect("paragraph");
        let before = app.source_document().visible_text();

        app.document.title = " leading space".to_string();
        let journal = app.operation_journal.len();

        let error = app
            .dispatch_command(
                "replace_all_in_document",
                json!({ "query": "alpha", "replacement": "gamma", "matchCase": false, "wholeWord": false, "regex": false }),
            )
            .expect_err("a refused batch is an error");
        assert!(matches!(error, crate::AppApiError::Model(_)), "{error:?}");
        app.document.title = "Doc".to_string();
        assert_eq!(
            app.source_document().visible_text(),
            before,
            "no part of the batch landed"
        );
        assert_eq!(app.operation_journal.len(), journal, "nothing journalled");
    }

    /// The same guard when the actor id changes rather than the counter: a
    /// replica that joins a collaboration session adopts the service's actor,
    /// so entries keyed on the old one can never be looked up again.
    #[test]
    fn captured_inverses_from_a_previous_actor_are_discarded() {
        let mut app = OpenDocApp::new_empty_document();
        app.dispatch_command("create_document", json!({ "title": "Local" }))
            .expect("create");
        app.dispatch_command("add_paragraph", json!({ "text": "local work" }))
            .expect("paragraph");
        assert!(!app.operation_inverses.is_empty());

        let base = crate::Document::new("Shared");
        app.join_collaboration_session(
            crate::OpenDocServiceSession::new(
                "subject",
                "actor-from-the-service",
                "doc-uuid",
                crate::OpenDocServiceRole::Editor,
            ),
            base,
            Vec::new(),
        )
        .expect("join");
        app.dispatch_command("add_paragraph", json!({ "text": "session work" }))
            .expect("paragraph");

        assert!(
            app.operation_inverses
                .keys()
                .all(|id| id.actor.0 == "actor-from-the-service"),
            "an inverse keyed on the pre-session actor is unreachable: {:?}",
            app.operation_inverses.keys().collect::<Vec<_>>()
        );
    }
}
