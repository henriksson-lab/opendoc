//! Ingesting operations this replica did not author.
//!
//! `dispatch_command` turns a *local gesture* into operations. This module is
//! the other direction: operations that arrive from a collaboration service's
//! fanout, which no command produced and no local user can be asked about.
//!
//! # Merge, never replay
//!
//! A remote operation is **not** applied to the current document. It is added
//! to the operation set and the document is re-materialised as
//! `merge_operations(merge_base, the whole set)`.
//!
//! ADR 0007 is the reason and it is not a preference. That ADR buys structural
//! convergence by reconstructing per-character identities from (base,
//! operation set) at merge time and *not* persisting them, and pays for it
//! with a stated limit: once a merge result is written back as plain text and
//! becomes the new base, an operation that predates it can no longer be placed
//! by identity and degrades to positional, clamped application. Folding each
//! arriving remote operation into the previous result would make *every*
//! concurrent operation such a late arrival — the corruption ADR 0007 exists
//! to remove, reintroduced on the client. `crates/opendoc-service`'s document
//! thread re-merges from genesis for exactly the same reason, and the two
//! sides agreeing on (base, set) is the whole convergence argument.
//!
//! # Where remote operations land, and why
//!
//! They become ordinary [`AppOperationEnvelope`]s in the journal, next to the
//! ones this actor authored. Three things need that and none of them is
//! optional:
//!
//! * **Causal context.** A locally authored operation's context is built by
//!   scanning `operation_envelopes`. If remote work were held somewhere else,
//!   the next local keystroke would claim not to have observed an edit it
//!   demonstrably saw, and its offsets would be re-anchored against a document
//!   this replica never had.
//! * **The local save.** A repository snapshot is the materialised document,
//!   which includes remote work. If the operation segment did not, replaying
//!   the chain would not reproduce the snapshot.
//! * **The recovery journal.** Its invariant is that the segment replays to
//!   exactly the in-memory state (ADR 0005). Excluding remote work would make
//!   recovery silently roll a collaborator's edits back.
//!
//! # What they deliberately do *not* do
//!
//! * They do not become undoable: the undo stack holds this actor's own steps
//!   and Ctrl+Z reverses this actor's last edit, never whatever happened most
//!   recently. See [`OpenDocApp::apply_remote_operations`].
//! * They do not make the document look locally dirty. They were durable on
//!   the service before it acknowledged them, so they are not work that exists
//!   only here — but the local repository still has to be given them, so
//!   `saved_operation_count` (what a save writes from) and
//!   [`OpenDocApp::settled_operation_count`] (what the dirty flag is measured
//!   against) part company the moment remote work arrives.

use super::*;
use opendoc_api::{EditorPosition, EditorSelection};

/// What one call to [`OpenDocApp::apply_remote_operations`] did.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppRemoteIntake {
    /// The document after the remote work landed.
    pub document: AppDocument,
    /// Operations this call folded in — ones the replica had not seen.
    pub applied: usize,
    /// Operations the replica already had: the service relays a commit to its
    /// submitter too, so a client's own work comes back to it, and a
    /// redelivery is a no-op rather than a special case.
    pub already_known: usize,
    /// The caller's caret, moved to where it now points. `None` when no
    /// selection was supplied, or when the block it named no longer exists.
    pub selection: Option<EditorSelection>,
}

impl OpenDocApp {
    /// Adopt a collaboration session: the actor id the service bound to this
    /// subject, the merge base, and the operation log it sent in its welcome.
    ///
    /// This replaces the open document, because the service's document *is*
    /// the document from here on. The actor id is the service's, not this
    /// process's: the service refuses an operation whose `id.actor` is not the
    /// one it bound to the authenticated subject, and `OperationId` is what
    /// the causal order and last-writer-wins are computed from.
    pub fn join_collaboration_session(
        &mut self,
        session: OpenDocServiceSession,
        base: Document,
        operations: Vec<Operation>,
    ) -> Result<AppDocument, AppApiError> {
        let session = session.normalized();
        if !session.is_valid() {
            return Err(AppApiError::Format(
                "collaboration session subject, actor or document uuid is empty".to_string(),
            ));
        }
        let actor_id = session.actor.clone();
        // The base is about to become source state, so it is held to the same
        // bar a repository snapshot is.
        AppDocument::from_core(&base).validate_source()?;

        self.clear_blob_state();
        self.clear_edit_history();
        self.clear_repository_binding();
        self.actor_id = actor_id;
        // The service's answers, as they arrived. Nothing else writes this,
        // which is what makes `authorize_runtime_command` an answer the client
        // received rather than a claim the client made.
        self.service_session = Some(session);
        self.workbook = Self::blank_workbook(&base.title);
        self.document = base.clone();
        self.merge_base = Some(base);
        self.remote_operation_count = 0;
        self.is_open = true;
        self.defer_spreadsheet_evaluation = false;
        self.invalidate_source_state();

        let intake = self.apply_remote_operations(operations, None)?;
        // Everything the welcome carried is the service's, not this user's
        // unsaved work, and nothing in it can be undone.
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.undo_coalesce = None;
        Ok(intake.document)
    }

    /// Fold operations from the service's fanout into the document.
    ///
    /// `selection` is the caret the caller is holding, if any; the caret that
    /// comes back in [`AppRemoteIntake::selection`] points at the same place in
    /// the document the remote work produced. Passing `None` is fine and means
    /// "I am not showing a caret".
    ///
    /// # The undo stack survives this
    ///
    /// Ingesting work this replica had not seen used to **drop the undo and
    /// redo stacks**, because a checkpoint is a whole-state snapshot and
    /// restoring one taken before a remote edit would have deleted that edit
    /// locally while the service still held it. That guard is gone, and with
    /// it the reason for it: an undo is now the *inverse* of this actor's own
    /// operations, submitted like any other edit (ADR 0017). It addresses only
    /// characters this actor typed and blocks this actor removed, so there is
    /// no snapshot to restore and nothing of anyone else's for it to reach.
    ///
    /// The service relays a commit to its submitter as well, so this replica's
    /// own operations come back here. Those are already in the set, so they are
    /// counted in `already_known` and change nothing.
    pub fn apply_remote_operations(
        &mut self,
        operations: Vec<Operation>,
        selection: Option<EditorSelection>,
    ) -> Result<AppRemoteIntake, AppApiError> {
        if self.merge_base.is_none() {
            return Err(AppApiError::Conflict(
                "no collaboration session is open; join one before ingesting remote operations"
                    .to_string(),
            ));
        }

        let mut applied = 0usize;
        let mut already_known = 0usize;
        let mut accepted = Vec::new();
        for operation in operations {
            // `accepted` carries the envelopes minted earlier in this same
            // call, so a batch that repeats an operation is recognised and a
            // batch of new ones does not hand two envelopes one number.
            match self.classify_incoming(&operation, &accepted)? {
                Incoming::Known => already_known += 1,
                Incoming::New(envelope) => {
                    accepted.push(*envelope);
                    applied += 1;
                }
            }
        }

        if applied == 0 {
            return Ok(AppRemoteIntake {
                document: self.document(),
                applied,
                already_known,
                selection,
            });
        }

        let before = self.document.clone();
        // **Merge first, commit second.** The envelopes used to be pushed here
        // and the merge run afterwards, so a set that did not merge left the
        // journal holding it for ever: every later call — including the
        // `remateralise_in_session()` on each keystroke — re-merged the same
        // poisoned set and failed identically, and the replica was frozen for
        // the rest of the session with no way back. Nothing is committed until
        // the candidate set is known to merge, so a refused batch costs the
        // caller an error and nothing else.
        let candidate: Vec<Operation> = self
            .collaboration_operations()
            .into_iter()
            .chain(
                accepted
                    .iter()
                    .filter_map(|envelope| envelope.operation.clone()),
            )
            .collect();
        let merged = self.merge_from_merge_base(&candidate)?;
        for envelope in accepted {
            self.operation_journal.push(envelope.record.clone());
            self.operation_envelopes.push(envelope);
        }
        // Continue this actor's own numbering past anything the service
        // already holds under it. The welcome carries this replica's earlier
        // work, and the fanout echoes what it just submitted; without this the
        // next local operation would re-mint an id the log already has, which
        // the service refuses as an attempt to rewrite history.
        //
        // Two numberings, two maxima. The envelope counter has to clear every
        // envelope in the journal; the operation counter has to clear every
        // *operation* this actor authored, and only those — taking the
        // envelope maximum for it would skip ids for no reason, and taking a
        // count instead of a maximum would re-mint one.
        let highest_own_envelope = self
            .operation_envelopes
            .iter()
            .filter(|envelope| envelope.record.actor == self.actor_id)
            .map(|envelope| envelope.record.seq)
            .max()
            .unwrap_or(0);
        self.next_envelope_seq = self.next_envelope_seq.max(highest_own_envelope + 1);
        let highest_own_operation = self
            .operation_envelopes
            .iter()
            .filter_map(|envelope| envelope.operation.as_ref())
            .filter(|operation| operation.id.actor.0 == self.actor_id)
            .map(|operation| operation.id.seq)
            .max()
            .unwrap_or(0);
        self.next_operation_seq = self.next_operation_seq.max(highest_own_operation + 1);
        // A remote operation changes source state, so any signature over the
        // previous source state is void — the same reasoning a local edit uses.
        self.invalidate_source_state();
        self.install_merged_document(merged);
        self.remote_operation_count += applied;

        // The undo stack is deliberately **not** touched. It used to be
        // cleared here, because a checkpoint is a whole-state snapshot and
        // restoring one taken before a remote edit would have deleted that
        // edit locally while the service still held it. Undo no longer
        // restores snapshots for steps made of document operations: it submits
        // the inverse of each, which names only this actor's own contribution.
        // Remote work is therefore not something an undo can reach, and
        // clearing the history would cost the user work nobody disturbed.
        // ADR 0017.
        //
        // The coalescing window is closed, though. The next keystroke is a new
        // gesture: it was typed after seeing somebody else's edit, so it does
        // not belong in the same undo step as the one before it.
        self.undo_coalesce = None;

        // ADR 0005's hook is `dispatch_command`, and this is not a command, so
        // the journal would otherwise lag the state it shadows across exactly
        // the window where someone else's work is the only thing in memory.
        self.sync_recovery_journal();

        let selection =
            selection.and_then(|selection| rebase_selection(&before, &self.document, selection));
        Ok(AppRemoteIntake {
            document: self.document(),
            applied,
            already_known,
            selection,
        })
    }

    /// Leave the session and keep the document as ordinary local state.
    ///
    /// The operation log stays, because it is the document's history; only the
    /// anchor to the service goes. Once the base is gone the app materialises
    /// incrementally again, which is correct for a replica that is no longer
    /// receiving anyone else's concurrent work.
    pub fn leave_collaboration_session(&mut self) -> AppDocument {
        self.merge_base = None;
        self.remote_operation_count = 0;
        // The service's answers do not outlive the session they answered
        // about. Keeping a role after the socket closed would be the client
        // deciding its own permissions from a stale fact.
        self.service_session = None;
        self.document()
    }

    /// The service's answers about the open session, if there is one.
    pub fn service_session(&self) -> Option<&OpenDocServiceSession> {
        self.service_session.as_ref()
    }

    /// Replace the peer list with the one a service presence frame carried.
    ///
    /// A no-op without an open session: presence is server state, so there is
    /// nothing for it to be about.
    pub fn apply_service_presence(&mut self, peers: Vec<OpenDocPresencePeer>) {
        if let Some(session) = self.service_session.as_mut() {
            session.set_peers(peers);
        }
    }

    /// Record that the service acknowledged this actor's operations up to
    /// `seq` as durable.
    ///
    /// Monotonic: an out-of-order acknowledgement cannot walk the watermark
    /// backwards and make already-durable work look unsent.
    pub fn acknowledge_service_operations(&mut self, seq: u64) {
        if let Some(session) = self.service_session.as_mut() {
            session.acknowledged_seq = session.acknowledged_seq.max(seq);
        }
    }

    /// Adopt a role the service re-attested — on a presence frame, or after a
    /// grant changed under this session.
    pub fn apply_service_role(&mut self, role: OpenDocServiceRole) {
        if let Some(session) = self.service_session.as_mut() {
            session.role = role;
        }
    }

    /// The open document as the canonical model.
    ///
    /// `document()` returns the projection a UI renders; this is the record
    /// that gets hashed, signed and compared. A collaboration client needs it
    /// because convergence is stated in canonical CBOR bytes, not in a DTO.
    pub fn source_document(&self) -> &Document {
        &self.document
    }

    /// The merge base of the open session, if there is one.
    pub fn collaboration_merge_base(&self) -> Option<&Document> {
        self.merge_base.as_ref()
    }

    /// Every typed operation in this replica's set, in the order it arrived.
    ///
    /// The merged document is a pure function of this and the merge base
    /// (ADR 0007), so a client and the server holding the same two encode to
    /// the same canonical CBOR.
    pub fn collaboration_operations(&self) -> Vec<Operation> {
        self.operation_envelopes
            .iter()
            .filter_map(|envelope| envelope.operation.clone())
            .collect()
    }

    /// The actor id operations authored here are stamped with.
    pub fn actor_id(&self) -> &str {
        &self.actor_id
    }

    /// Operations this replica authored with a sequence number above `seq` —
    /// what a transport submits after the service acknowledged up to `seq`.
    ///
    /// The service requires an actor's sequence numbers to be dense, and
    /// refuses a gap rather than storing one. This returns what the journal
    /// has; it does not invent the missing numbers. See
    /// [`OpenDocApp::local_operations_are_dense_after`].
    pub fn local_operations_after(&self, seq: u64) -> Vec<Operation> {
        self.operation_envelopes
            .iter()
            .filter_map(|envelope| envelope.operation.as_ref())
            .filter(|operation| operation.id.actor.0 == self.actor_id && operation.id.seq > seq)
            .cloned()
            .collect()
    }

    /// Whether [`OpenDocApp::local_operations_after`] would be accepted by a
    /// service, which requires `seq + 1, seq + 2, …` with no gaps.
    ///
    /// For a session this replica authored, it is true by construction:
    /// `next_operation_seq` numbers document operations and nothing else, so
    /// an undo marker, a blob upload or a spreadsheet edit no longer consumes
    /// an operation id. It is kept because "by construction" is a claim about
    /// this crate, and a transport should be able to check rather than trust
    /// it — and because a history loaded from a repository written before the
    /// counters were split really does have gaps, which
    /// `report_legacy_operation_sequence_gaps` names on the way in.
    pub fn local_operations_are_dense_after(&self, seq: u64) -> bool {
        (seq + 1..)
            .zip(self.local_operations_after(seq))
            .all(|(expected, operation)| operation.id.seq == expected)
    }

    /// Re-materialise while a session is open, so the local document stays
    /// `merge_operations(base, set)` after a *local* edit too.
    ///
    /// **No test can tell this line from its absence, and that is expected.** A
    /// locally authored operation's causal context observes every operation in
    /// the set, so it sorts last and its offsets resolve against the whole
    /// visible run — which is exactly what applying it to the already-collapsed
    /// document does. `the_open_document_is_the_merge_of_the_base_and_the_whole_set`
    /// asserts the two agree.
    ///
    /// It is kept because it makes agreement with the server *structural*
    /// rather than a property argued from how contexts happen to be built, at
    /// the O(log) per edit the server already pays for the same reason
    /// (`docs/adr/0015`, "Why the server re-merges from genesis").
    pub(crate) fn remateralise_in_session(&mut self) {
        if self.merge_base.is_none() {
            return;
        }
        if let Err(error) = self.materialise_from_merge_base() {
            self.push_model_warning("collaboration-merge-failed", error.to_string());
        }
    }

    /// Re-merges the session's base with `operations` **without touching any
    /// state**, so the answer can be thrown away if it is an error.
    ///
    /// This is the half of materialisation that can fail. Keeping it separate
    /// from [`OpenDocApp::install_merged_document`] is what lets the remote
    /// ingest be transactional: the candidate set is merged before a single
    /// envelope is committed, so a set that does not merge cannot be left in
    /// the journal to fail again on every later call.
    fn merge_from_merge_base(&self, operations: &[Operation]) -> Result<Document, AppApiError> {
        let Some(base) = self.merge_base.as_ref() else {
            return Ok(self.document.clone());
        };
        let result = merge_operations(base, std::slice::from_ref(&operations.to_vec())).map_err(
            |error| {
                AppApiError::Model(format!("collaboration operations did not merge: {error:?}"))
            },
        )?;
        Ok(result.document)
    }

    /// Adopts a document the merge produced. Infallible by construction: the
    /// only thing that could have failed already did, in
    /// [`OpenDocApp::merge_from_merge_base`].
    fn install_merged_document(&mut self, mut document: Document) {
        // Warnings are the one part of `self.document` that no operation
        // produced — blob restoration, DOI lookup fallbacks and the recovery
        // journal push them directly — so the merge cannot reproduce them and
        // dropping them would lose the only report the user gets.
        for warning in &self.document.warnings {
            if !document.warnings.contains(warning) {
                document.warnings.push(warning.clone());
            }
        }
        self.document = document;
        self.invalidate_projection();
    }

    fn materialise_from_merge_base(&mut self) -> Result<(), AppApiError> {
        if self.merge_base.is_none() {
            return Ok(());
        }
        let merged = self.merge_from_merge_base(&self.collaboration_operations())?;
        self.install_merged_document(merged);
        Ok(())
    }

    fn classify_incoming(
        &self,
        operation: &Operation,
        pending: &[AppOperationEnvelope],
    ) -> Result<Incoming, AppApiError> {
        if operation.id.actor.0.trim().is_empty() {
            return Err(AppApiError::Format(
                "remote operation actor is empty".to_string(),
            ));
        }
        if operation.id.seq == 0 {
            return Err(AppApiError::Format(
                "remote operation sequence is zero".to_string(),
            ));
        }
        // Matched on the *operation's* id, not on the envelope's record. The
        // two were the same number until the counters were split; comparing
        // the record's would now ask an undo marker or a blob envelope whether
        // it is this operation, and get "same number, no payload" — a
        // conflict, for an operation the replica has never seen.
        if let Some(existing) = self
            .operation_envelopes
            .iter()
            .chain(pending)
            .filter_map(|envelope| envelope.operation.as_ref())
            .find(|existing| existing.id == operation.id)
        {
            // Same id, same bytes is a redelivery. Same id, different bytes is
            // an attempt to rewrite history, and this replica refuses it for
            // the same reason the service does.
            if existing == operation {
                return Ok(Incoming::Known);
            }
            return Err(AppApiError::Conflict(format!(
                "operation {}#{} is already held with a different payload",
                operation.id.actor.0, operation.id.seq
            )));
        }
        // An envelope number this replica has not used for this actor. A
        // remote actor's operation sequence is dense (the service keeps it so),
        // so its own sequence is normally free — but this replica's own
        // envelopes are numbered from its own counter, so an echo of its own
        // work can collide with one. Taking the next free number keeps envelope
        // identity unique without renumbering anybody's operation.
        let envelope_seq =
            self.next_free_envelope_seq(&operation.id.actor.0, operation.id.seq, pending);
        let envelope = AppOperationEnvelope::from_operation(operation.clone(), envelope_seq);
        // The same source validation a locally minted envelope passes before a
        // save. A service is not a trusted author of this repository's bytes.
        validate_operation_envelopes(std::slice::from_ref(&envelope))?;
        Ok(Incoming::New(Box::new(envelope)))
    }
}

impl OpenDocApp {
    /// The lowest envelope number at or above `preferred` that `actor` does
    /// not already hold in this journal.
    ///
    /// Envelope identity is `(actor, seq)` and has to stay unique — a
    /// candidate merge and the recovery segment both deduplicate on it — but
    /// nothing outside this process reads it, so this replica is free to pick
    /// the next free number rather than insisting on one that is taken.
    fn next_free_envelope_seq(
        &self,
        actor: &str,
        preferred: u64,
        pending: &[AppOperationEnvelope],
    ) -> u64 {
        let taken = self
            .operation_envelopes
            .iter()
            .chain(pending)
            .filter(|envelope| envelope.record.actor == actor)
            .map(|envelope| envelope.record.seq)
            .collect::<BTreeSet<_>>();
        let mut seq = preferred.max(1);
        while taken.contains(&seq) {
            seq += 1;
        }
        seq
    }
}

enum Incoming {
    Known,
    New(Box<AppOperationEnvelope>),
}

/// Move a selection from the document before an intake to the one after it.
///
/// `{block_id, inline_id, offset}` already survives most of what a remote edit
/// can do: an insert in another block, or in another run of the same block,
/// shifts no identity and no run-relative offset. The one case it does not
/// survive is a change inside the caret's own run, and that is what this
/// repairs — against the *materialised* texts rather than against the remote
/// operations' own offsets, which are stated in their author's causal context
/// and not in this replica's.
fn rebase_selection(
    before: &Document,
    after: &Document,
    selection: EditorSelection,
) -> Option<EditorSelection> {
    let anchor = rebase_position(before, after, selection.anchor)?;
    let focus = rebase_position(before, after, selection.focus)?;
    Some(EditorSelection { anchor, focus })
}

fn rebase_position(
    before: &Document,
    after: &Document,
    position: EditorPosition,
) -> Option<EditorPosition> {
    let block_id = StableId::parse(&position.block_id).ok()?;
    // A block that is gone cannot hold a caret, and guessing where the user
    // would want it instead is the caller's decision, not this function's.
    find_block_by_id(&after.blocks, &block_id)?;
    let Some(inline_id) = position.inline_id.as_ref() else {
        return Some(position);
    };
    let Ok(inline_id) = StableId::parse(inline_id) else {
        return Some(position);
    };
    let after_text = match find_inline_text(&after.blocks, &inline_id) {
        Some(text) => text,
        // The run is gone, or is not a text run. Keep the block and let the
        // caller re-place the caret inside it.
        None => {
            return Some(EditorPosition {
                block_id: position.block_id,
                inline_id: None,
                offset: 0,
            })
        }
    };
    let Some(before_text) = find_inline_text(&before.blocks, &inline_id) else {
        // The run is new to this replica, so there is no old offset to move.
        return Some(EditorPosition {
            block_id: position.block_id,
            inline_id: Some(inline_id.to_string()),
            offset: position.offset.min(after_text.chars().count()),
        });
    };
    let offset = rebase_offset(&before_text, &after_text, position.offset);
    Some(EditorPosition {
        block_id: position.block_id,
        inline_id: Some(inline_id.to_string()),
        offset,
    })
}

/// Where character `offset` of `before` now sits in `after`.
///
/// Everything before the first difference is untouched, everything after the
/// last difference moved by the length change, and a caret inside the changed
/// span has no place of its own left — it goes to the start of that span,
/// which is the only position the user can still recognise.
fn rebase_offset(before: &str, after: &str, offset: usize) -> usize {
    let before: Vec<char> = before.chars().collect();
    let after: Vec<char> = after.chars().collect();
    let offset = offset.min(before.len());
    let mut prefix = 0;
    while prefix < before.len() && prefix < after.len() && before[prefix] == after[prefix] {
        prefix += 1;
    }
    if offset <= prefix {
        return offset;
    }
    let mut suffix = 0;
    while suffix < before.len() - prefix
        && suffix < after.len() - prefix
        && before[before.len() - 1 - suffix] == after[after.len() - 1 - suffix]
    {
        suffix += 1;
    }
    if offset >= before.len() - suffix {
        return after.len() - (before.len() - offset);
    }
    prefix
}

fn find_block_by_id<'a>(blocks: &'a [Block], id: &StableId) -> Option<&'a Block> {
    for block in blocks {
        if &block.id == id {
            return Some(block);
        }
        if let BlockKind::Table { rows, .. } = &block.kind {
            for row in rows {
                for cell in &row.cells {
                    if let Some(found) = find_block_by_id(&cell.blocks, id) {
                        return Some(found);
                    }
                }
            }
        }
    }
    None
}

fn find_inline_text(blocks: &[Block], id: &StableId) -> Option<String> {
    for block in blocks {
        for inline in &block.content {
            match inline {
                Inline::Text {
                    id: inline_id,
                    text,
                    ..
                }
                | Inline::Link {
                    id: inline_id,
                    text,
                    ..
                } if inline_id == id => return Some(text.clone()),
                _ => {}
            }
        }
        if let BlockKind::Table { rows, .. } = &block.kind {
            for row in rows {
                for cell in &row.cells {
                    if let Some(found) = find_inline_text(&cell.blocks, id) {
                        return Some(found);
                    }
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use opendoc_merge::{ActorId, CausalContext, OperationId};

    const BLOCK: &str = "blk-collab-0001";
    const RUN: &str = "inl-collab-0001";

    fn block_id() -> StableId {
        StableId::parse(BLOCK).expect("block id")
    }

    fn run_id() -> StableId {
        StableId::parse(RUN).expect("run id")
    }

    /// One paragraph holding one run, so character operations have a target.
    fn seeded_base(text: &str) -> Document {
        let mut document = Document::new("Shared document");
        document.blocks.push(Block {
            id: block_id(),
            kind: BlockKind::Paragraph,
            properties: Default::default(),
            content: vec![Inline::Text {
                id: run_id(),
                text: text.to_string(),
                marks: Vec::new(),
            }],
        });
        document
    }

    fn remote_insert(actor: &str, seq: u64, offset: usize, text: &str) -> Operation {
        Operation::in_context(
            OperationId {
                actor: ActorId(actor.to_string()),
                seq,
            },
            OperationKind::InsertText {
                inline_id: run_id(),
                offset,
                text: text.to_string(),
            },
            CausalContext::default(),
        )
    }

    fn local_session() -> OpenDocServiceSession {
        OpenDocServiceSession::new(
            "local",
            "actor-local",
            "doc-collab",
            OpenDocServiceRole::Editor,
        )
    }

    fn joined(text: &str) -> OpenDocApp {
        let mut app = OpenDocApp::new_empty_document();
        app.join_collaboration_session(local_session(), seeded_base(text), Vec::new())
            .expect("joining a session");
        app
    }

    fn run_text(app: &OpenDocApp) -> String {
        find_inline_text(&app.document.blocks, &run_id()).expect("the run is still there")
    }

    fn block_text(block: &opendoc_core::Block) -> String {
        block
            .content
            .iter()
            .filter_map(|inline| match inline {
                opendoc_core::Inline::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// A refused ingest must commit **nothing**, because the alternative is a
    /// replica that never recovers.
    ///
    /// The envelopes used to be pushed into the journal, both counters
    /// advanced and the projection invalidated *before* the merge ran. A merge
    /// that then failed left the operations in the set with nothing to take
    /// them out again, so every later call re-merged the same poisoned set and
    /// failed identically — including `remateralise_in_session()`, which runs
    /// on every keystroke. The replica was frozen for the rest of the session.
    ///
    /// The failure is induced by making the merge base a document
    /// `Document::validate()` rejects: every typed operation guards its own
    /// payload, so there is no operation that poisons a merge on its own, and
    /// what is under test is the commit order, not the cause.
    #[test]
    fn an_ingest_that_does_not_merge_commits_nothing_and_does_not_freeze_the_replica() {
        let mut app = joined("abcdefgh");
        let good_base = app.merge_base.clone().expect("a session base");
        let envelopes = app.operation_envelopes.len();
        let journal = app.operation_journal.len();
        let remote = app.remote_operation_count;
        let text = run_text(&app);

        let mut broken = good_base.clone();
        broken.title = " leading space".to_string();
        app.merge_base = Some(broken);

        let incoming = vec![remote_insert("actor-remote", 1, 0, "X")];
        let error = app
            .apply_remote_operations(incoming.clone(), None)
            .expect_err("the ingest cannot merge");
        assert!(matches!(error, AppApiError::Model(_)), "{error:?}");
        assert_eq!(
            app.operation_envelopes.len(),
            envelopes,
            "a refused ingest commits no envelope"
        );
        assert_eq!(app.operation_journal.len(), journal);
        assert_eq!(app.remote_operation_count, remote);
        assert_eq!(run_text(&app), text, "and leaves the document alone");

        // Whatever made the merge fail is gone; the very same operations now
        // land, which they could not if the first attempt had kept them.
        app.merge_base = Some(good_base);
        let intake = app
            .apply_remote_operations(incoming, None)
            .expect("the ingest succeeds once the merge does");
        assert_eq!(
            intake.applied, 1,
            "the operation is new, not `already_known`"
        );
        assert_eq!(run_text(&app), "Xabcdefgh");
    }

    #[test]
    fn ingesting_without_a_session_is_refused_rather_than_applied_positionally() {
        let mut app = OpenDocApp::new_empty_document();
        let error = app
            .apply_remote_operations(vec![remote_insert("actor-remote", 1, 0, "x")], None)
            .expect_err("there is no merge base to place the operation against");
        assert!(matches!(error, AppApiError::Conflict(_)), "{error:?}");
    }

    #[test]
    fn a_remote_insert_reaches_the_document() {
        let mut app = joined("abcdefgh");
        let intake = app
            .apply_remote_operations(vec![remote_insert("actor-remote", 1, 3, "XY")], None)
            .expect("the operation merges");
        assert_eq!(intake.applied, 1);
        assert_eq!(intake.already_known, 0);
        assert_eq!(run_text(&app), "abcXYdefgh");
    }

    /// The service relays every commit to its submitter too. Redelivery must
    /// be a no-op, not a second application.
    #[test]
    fn a_redelivered_operation_changes_nothing() {
        let mut app = joined("abcdefgh");
        let operation = remote_insert("actor-remote", 1, 3, "XY");
        app.apply_remote_operations(vec![operation.clone()], None)
            .expect("first delivery");
        let intake = app
            .apply_remote_operations(vec![operation], None)
            .expect("second delivery");
        assert_eq!(intake.applied, 0);
        assert_eq!(intake.already_known, 1);
        assert_eq!(run_text(&app), "abcXYdefgh");
    }

    /// The same id carrying different bytes is an attempt to rewrite history.
    #[test]
    fn a_logged_id_with_a_different_payload_is_refused() {
        let mut app = joined("abcdefgh");
        app.apply_remote_operations(vec![remote_insert("actor-remote", 1, 3, "XY")], None)
            .expect("first delivery");
        let error = app
            .apply_remote_operations(vec![remote_insert("actor-remote", 1, 3, "ZZ")], None)
            .expect_err("the id is already held with other bytes");
        assert!(matches!(error, AppApiError::Conflict(_)), "{error:?}");
        assert_eq!(run_text(&app), "abcXYdefgh");
    }

    /// Undoing someone else's keystroke is a bug; losing the right to undo
    /// your own because someone else typed is also a bug. Both used to be
    /// avoided by clearing the stack, which is the second bug.
    ///
    /// Now the local step is still undoable, the undo reverses only the local
    /// step, and the remote edit is untouched — the whole point of ADR 0017.
    #[test]
    fn a_local_step_stays_undoable_across_remote_work_and_the_remote_work_survives() {
        let mut app = joined("abcdefgh");
        app.dispatch_command("add_paragraph", serde_json::json!({ "text": "local work" }))
            .expect("a local, undoable command");
        assert!(!app.undo_stack.is_empty(), "the local edit is undoable");

        app.apply_remote_operations(vec![remote_insert("actor-remote", 1, 3, "XY")], None)
            .expect("the operation merges");

        assert!(
            !app.undo_stack.is_empty(),
            "remote work must not cost this user its own undo history"
        );
        app.undo_current_edit()
            .expect("the local step is still undoable");
        assert!(
            !app.document().visible_text().contains("local work"),
            "the undo must reverse the local step: {:?}",
            app.document().visible_text()
        );
        assert_eq!(
            run_text(&app),
            "abcXYdefgh",
            "the remote edit must still be there"
        );
    }

    /// The undo goes on the wire as new history, not as a rewind.
    ///
    /// The operation ids it mints are above everything this actor has already
    /// authored, so a service that refuses to rewrite history has nothing to
    /// refuse — which is exactly what the old snapshot undo could not manage.
    #[test]
    fn an_undo_mints_new_operation_ids_rather_than_reusing_them() {
        let mut app = joined("abcdefgh");
        app.dispatch_command("add_paragraph", serde_json::json!({ "text": "local work" }))
            .expect("a local, undoable command");
        let authored = app.local_operations_after(0);
        let highest = authored
            .iter()
            .map(|operation| operation.id.seq)
            .max()
            .expect("the command authored something");

        app.undo_current_edit().expect("undo");

        let after = app.local_operations_after(0);
        assert!(
            after.len() > authored.len(),
            "an undo appends operations: {} then {}",
            authored.len(),
            after.len()
        );
        assert!(
            after.iter().any(|operation| operation.id.seq > highest),
            "the undo's own operations must be above the acknowledged watermark"
        );
        assert!(
            app.local_operations_are_dense_after(highest),
            "what the undo authored has to be submittable: {:?}",
            after.iter().map(|op| op.id.seq).collect::<Vec<_>>()
        );
    }

    /// Ctrl+Z is *mine*. A remote actor's edit is the most recent thing that
    /// happened, and it is still not what an undo reverses.
    #[test]
    fn undo_is_per_actor_not_global() {
        let mut app = joined("abcdefgh");
        app.dispatch_command("add_paragraph", serde_json::json!({ "text": "local work" }))
            .expect("a local, undoable command");
        app.apply_remote_operations(vec![remote_insert("actor-remote", 1, 3, "XY")], None)
            .expect("the remote edit arrives last");

        app.undo_current_edit().expect("undo");

        assert_eq!(
            run_text(&app),
            "abcXYdefgh",
            "the undo reversed the most recent *local* step, not the most recent step"
        );
        assert!(!app.document().visible_text().contains("local work"));
    }

    /// The same gap for an inline: undoing "delete the first inline of a
    /// block that had several" must put it back at the front of the block's
    /// content, not on the end of it.
    #[test]
    fn an_undo_puts_a_deleted_first_inline_back_at_the_front() {
        let mut app = joined("abcdefgh");
        let block = block_id().to_string();
        app.dispatch_command(
            "insert_inline_text",
            serde_json::json!({
                "blockId": block,
                "afterInlineId": run_id().to_string(),
                "text": "tail",
            }),
        )
        .expect("a second inline");
        let ids_before: Vec<String> = app.source_document().blocks[0]
            .content
            .iter()
            .map(|inline| inline_id(inline).to_string())
            .collect();
        assert_eq!(ids_before.len(), 2);

        app.dispatch_command(
            "delete_inline",
            serde_json::json!({ "inlineId": run_id().to_string() }),
        )
        .expect("deleting the first inline");
        assert_eq!(app.source_document().blocks[0].content.len(), 1);

        app.undo_current_edit().expect("the undo is expressible");

        let ids_after: Vec<String> = app.source_document().blocks[0]
            .content
            .iter()
            .map(|inline| inline_id(inline).to_string())
            .collect();
        assert_eq!(
            ids_after, ids_before,
            "the restored inline is first again, not appended"
        );
        assert_eq!(run_text(&app), "abcdefgh");
    }

    /// And for a *moved* inline. Joining a paragraph onto the previous one
    /// moves every inline of the source, first one included, and then deletes
    /// the source block — so before `InsertPosition` reached
    /// `MoveInlineToBlock` this whole gesture was un-undoable inside a
    /// session.
    ///
    /// The undo is walked newest first, so the source block comes back (empty,
    /// which is what it was by then) and the inlines are moved back into it
    /// last-to-first. The first one says `First`, which is the only reason the
    /// two end up in their original order.
    #[test]
    fn an_undo_moves_a_moved_first_inline_back_to_the_front_of_its_block() {
        let mut app = joined("head");
        let first_block = block_id().to_string();
        app.dispatch_command("add_paragraph", serde_json::json!({ "text": "one" }))
            .expect("a second paragraph");
        let second_block = app.source_document().blocks[1].id.to_string();
        app.dispatch_command(
            "insert_inline_text",
            serde_json::json!({
                "blockId": second_block,
                "afterInlineId": inline_id(
                    &app.source_document().blocks[1].content[0],
                )
                .to_string(),
                "text": "two",
            }),
        )
        .expect("a second inline in the second paragraph");
        let before = block_shape(&app);
        assert_eq!(
            before,
            vec![
                (first_block.clone(), vec!["head".to_string()]),
                (
                    second_block.clone(),
                    vec!["one".to_string(), "two".to_string()]
                ),
            ]
        );

        app.dispatch_command(
            "join_paragraph_with_previous",
            serde_json::json!({ "blockId": second_block }),
        )
        .expect("joining moves both inlines and deletes the source block");
        assert_eq!(app.source_document().blocks.len(), 1);

        app.undo_current_edit().expect("the undo is expressible");

        assert_eq!(
            block_shape(&app),
            before,
            "both inlines went back to the block they came from, in order"
        );
    }

    /// And for a table cell: the first cell of a row that had several.
    #[test]
    fn an_undo_puts_a_deleted_first_table_cell_back_at_the_front() {
        let mut app = joined("head");
        let anchor = block_id().to_string();
        app.dispatch_command(
            "insert_table_after",
            serde_json::json!({ "afterBlockId": anchor }),
        )
        .expect("a two-by-two table");
        let (table, row, cells_before) = table_row_shape(&app);
        assert_eq!(cells_before.len(), 2);

        app.dispatch_command(
            "delete_table_cell",
            serde_json::json!({
                "tableBlockId": table,
                "rowId": row,
                "cellId": cells_before[0].0.clone(),
            }),
        )
        .expect("deleting the first cell of the row");
        let (_, _, after_delete) = table_row_shape(&app);
        assert_ne!(after_delete[0].0, cells_before[0].0);

        app.undo_current_edit().expect("the undo is expressible");

        let (_, _, restored) = table_row_shape(&app);
        assert_eq!(
            restored[0], cells_before[0],
            "the restored cell is the row's first again, with its text"
        );
    }

    /// Every top-level block as `(id, [inline text])`.
    fn block_shape(app: &OpenDocApp) -> Vec<(String, Vec<String>)> {
        app.source_document()
            .blocks
            .iter()
            .map(|block| {
                (
                    block.id.to_string(),
                    block
                        .content
                        .iter()
                        .filter_map(|inline| match inline {
                            opendoc_core::Inline::Text { text, .. } => Some(text.clone()),
                            _ => None,
                        })
                        .collect(),
                )
            })
            .collect()
    }

    /// The table block id, its first row id, and that row's cells as
    /// `(cell id, visible text)`.
    fn table_row_shape(app: &OpenDocApp) -> (String, String, Vec<(String, String)>) {
        let block = app
            .source_document()
            .blocks
            .iter()
            .find(|block| matches!(block.kind, opendoc_core::BlockKind::Table { .. }))
            .expect("the table is there")
            .clone();
        let rows = match &block.kind {
            opendoc_core::BlockKind::Table { rows, .. } => rows.clone(),
            other => panic!("expected a table, got {other:?}"),
        };
        let cells = rows[0]
            .cells
            .iter()
            .map(|cell| {
                let text = cell
                    .blocks
                    .iter()
                    .flat_map(|block| block.content.iter())
                    .filter_map(|inline| match inline {
                        opendoc_core::Inline::Text { text, .. } => Some(text.clone()),
                        _ => None,
                    })
                    .collect::<String>();
                (cell.id.to_string(), text)
            })
            .collect();
        (block.id.to_string(), rows[0].id.to_string(), cells)
    }

    /// An operation the vocabulary cannot invert is **skipped**, and the undo
    /// carries on to the step below it.
    ///
    /// Deleting a table column that held content is the case
    /// (`table-column-cells-are-derived`): `InsertTableColumn` deliberately
    /// carries no cells, because they are derived from the row and column ids
    /// so that a concurrently inserted row gets one too (ADR 0013). Derived
    /// cells are empty, so re-inserting the column cannot bring the text back,
    /// and inside a session the whole-state snapshot is not available either —
    /// restoring one would delete a collaborator's edit the service already
    /// holds.
    ///
    /// The step therefore cannot be reversed by anything. It used to be pushed
    /// straight back on the stack, which meant the *next* `Ctrl+Z` met it
    /// again and refused again, for ever — one column delete wedged undo for
    /// the rest of the session, including for all the ordinary typing beneath
    /// it. It is now dropped, reported, and the undo reaches the step below.
    #[test]
    fn an_operation_the_vocabulary_cannot_invert_is_skipped_inside_a_session() {
        let mut app = joined("abcdefgh");
        let anchor = app.source_document().blocks[0].id.to_string();
        app.dispatch_command(
            "insert_table_after",
            serde_json::json!({ "afterBlockId": anchor }),
        )
        .expect("a table with text in its cells");
        let table = app.source_document().blocks[1].id.to_string();
        let column = match &app.source_document().blocks[1].kind {
            opendoc_core::BlockKind::Table { columns, .. } => columns[0].id.to_string(),
            other => panic!("expected a table, got {other:?}"),
        };
        app.dispatch_command(
            "delete_table_column",
            serde_json::json!({ "tableBlockId": table, "columnId": column }),
        )
        .expect("deleting a column that held content");
        assert_eq!(app.source_document().blocks.len(), 2);

        let document = app
            .undo_current_edit()
            .expect("the column delete is skipped and the table insert is undone");

        assert_eq!(
            app.source_document().blocks.len(),
            1,
            "the step under the un-reversible one was undone"
        );
        assert!(
            document
                .warnings
                .iter()
                .any(|warning| warning.code == "undo-step-skipped"),
            "the skipped step is reported, not silent: {:?}",
            document.warnings
        );
    }

    /// The case that made the wedge expensive: a spreadsheet edit on top of
    /// ordinary typing.
    ///
    /// The spreadsheet step moves state no document operation describes, so it
    /// cannot be reversed inside a session. It used to take *everything under
    /// it* with it, twice over: it was pushed back on the stack so the next
    /// `Ctrl+Z` met it again, and even past it, `inverse_of_step` compared
    /// each deeper step's workbook against the workbook **as it stands now**,
    /// which the spreadsheet edit had changed — so every paragraph typed
    /// before it was refused too. Both are fixed here: the step is skipped,
    /// and a deeper step is compared against the state *it* ended in.
    #[test]
    fn a_spreadsheet_edit_does_not_wedge_the_typing_underneath_it() {
        let mut app = joined("abcdefgh");
        app.dispatch_command("add_paragraph", serde_json::json!({ "text": "typed" }))
            .expect("a paragraph");
        assert_eq!(app.source_document().blocks.len(), 2);
        app.dispatch_command(
            "set_spreadsheet_cell",
            serde_json::json!({ "address": "A1", "value": "42" }),
        )
        .expect("a spreadsheet edit");

        let document = app
            .undo_current_edit()
            .expect("the spreadsheet step is skipped and the typing is undone");

        assert_eq!(
            app.source_document().blocks.len(),
            1,
            "the paragraph typed before the spreadsheet edit was undone"
        );
        assert!(
            document
                .warnings
                .iter()
                .any(|warning| warning.code == "undo-step-skipped"),
            "{:?}",
            document.warnings
        );
    }

    /// The other half of skip-and-continue: when *nothing* on the stack can be
    /// reversed, the stack is put back exactly as it was and the refusal is
    /// returned. Skipping must never be a way to lose history it could have
    /// kept.
    #[test]
    fn a_stack_of_nothing_but_un_reversible_steps_is_refused_and_restored() {
        let mut app = joined("abcdefgh");
        let anchor = app.source_document().blocks[0].id.to_string();
        app.dispatch_command(
            "insert_table_after",
            serde_json::json!({ "afterBlockId": anchor }),
        )
        .expect("a table with text in its cells");
        // Everything before the column delete is the service's, not this
        // actor's, so only the delete is on the stack.
        app.undo_stack.clear();
        app.redo_stack.clear();
        let table = app.source_document().blocks[1].id.to_string();
        let column = match &app.source_document().blocks[1].kind {
            opendoc_core::BlockKind::Table { columns, .. } => columns[0].id.to_string(),
            other => panic!("expected a table, got {other:?}"),
        };
        app.dispatch_command(
            "delete_table_column",
            serde_json::json!({ "tableBlockId": table, "columnId": column }),
        )
        .expect("deleting a column that held content");
        let steps = app.undo_stack.len();
        assert!(steps > 0);
        let blocks = app.source_document().blocks.len();

        let error = app
            .undo_current_edit()
            .expect_err("a deleted column's cell content cannot be re-derived");
        assert!(matches!(error, AppApiError::Conflict(_)), "{error:?}");
        assert_eq!(
            app.undo_stack.len(),
            steps,
            "refusing every step keeps every step"
        );
        assert_eq!(
            app.source_document().blocks.len(),
            blocks,
            "and changes nothing"
        );
    }

    /// The gap ADR 0017 named and this wave closed: undoing "delete the first
    /// block" puts it back *first*, not appended at the end.
    ///
    /// It runs inside a session, so it also proves the undo goes out as new
    /// history rather than falling back to the snapshot — the snapshot path is
    /// refused here.
    #[test]
    fn an_undo_puts_a_deleted_first_block_back_at_the_front() {
        let mut app = joined("abcdefgh");
        app.dispatch_command("add_paragraph", serde_json::json!({ "text": "second" }))
            .expect("a second block");
        let first = app.source_document().blocks[0].id.to_string();
        let first_text = block_text(&app.source_document().blocks[0]);
        app.dispatch_command("delete_block", serde_json::json!({ "blockId": first }))
            .expect("deleting the first block");
        assert_eq!(app.source_document().blocks.len(), 1);

        app.undo_current_edit().expect("the undo is expressible");

        let blocks = &app.source_document().blocks;
        assert_eq!(blocks.len(), 2);
        assert_eq!(
            blocks[0].id.to_string(),
            first,
            "the restored block is the first one again, not appended"
        );
        assert_eq!(block_text(&blocks[0]), first_text);
    }

    /// A step the operation vocabulary cannot express is refused rather than
    /// restored, and refusing does not consume the step.
    ///
    /// Restoring a whole-state snapshot inside a session would roll the
    /// operation log back to before the remote work the service has already
    /// accepted. There is no version of that which is not data loss, so the
    /// answer is a conflict the user can be told about.
    #[test]
    fn a_step_that_needs_a_snapshot_is_refused_inside_a_session() {
        let mut app = joined("abcdefgh");
        app.dispatch_command(
            "set_spreadsheet_cell",
            serde_json::json!({ "address": "A1", "value": "7" }),
        )
        .expect("a spreadsheet command is undoable and has no operation");
        let before = app.undo_stack.len();
        let error = app
            .undo_current_edit()
            .expect_err("a spreadsheet step has no inverse operation");
        assert!(matches!(error, AppApiError::Conflict(_)), "{error:?}");
        assert_eq!(
            app.undo_stack.len(),
            before,
            "refusing must not consume the step"
        );
    }

    /// A replica's own operations come back in the fanout. Those are already
    /// in the set, so they must not cost the user an undo history nobody else
    /// disturbed.
    #[test]
    fn the_echo_of_this_replicas_own_work_keeps_the_undo_stack() {
        let mut app = joined("abcdefgh");
        app.dispatch_command("add_paragraph", serde_json::json!({ "text": "local work" }))
            .expect("a local, undoable command");
        let own = app.local_operations_after(0);
        assert!(!own.is_empty(), "the local command authored operations");

        let intake = app
            .apply_remote_operations(own, None)
            .expect("the echo is accepted");

        assert_eq!(intake.applied, 0);
        assert!(
            !app.undo_stack.is_empty(),
            "the local edit is still undoable"
        );
    }

    /// Dirty state means "work that exists only here". A remote operation was
    /// durable on the service before it was acknowledged, so it is not that —
    /// but it must not clear a genuine local edit either.
    #[test]
    fn remote_work_alone_does_not_make_the_document_locally_dirty() {
        let mut app = joined("abcdefgh");
        assert!(
            !app.has_unsaved_changes(),
            "a freshly joined session holds no local work"
        );

        app.apply_remote_operations(vec![remote_insert("actor-remote", 1, 3, "XY")], None)
            .expect("the operation merges");
        assert!(
            !app.has_unsaved_changes(),
            "someone else's keystroke is not this user's unsaved work"
        );

        app.dispatch_command("add_paragraph", serde_json::json!({ "text": "local work" }))
            .expect("a local command");
        assert!(app.has_unsaved_changes(), "a local edit is unsaved work");

        app.apply_remote_operations(vec![remote_insert("actor-remote", 2, 0, "Z")], None)
            .expect("more remote work");
        assert!(
            app.has_unsaved_changes(),
            "remote work must not clear a genuine local edit"
        );
    }

    /// A local save has to write the remote operations even though they never
    /// made the document dirty, or the chain replays to a different document
    /// than the snapshot it sits next to.
    #[test]
    fn a_local_save_writes_the_remote_operations_it_never_counted_as_dirty() {
        let mut app = joined("abcdefgh");
        app.apply_remote_operations(vec![remote_insert("actor-remote", 1, 3, "XY")], None)
            .expect("the operation merges");
        assert_eq!(app.saved_operation_count, 0);
        assert_eq!(app.remote_operation_count, 1);

        let root = std::env::temp_dir().join(format!(
            "opendoc-collab-save-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        app.save_to_local_repository(&root).expect("a local save");
        assert_eq!(app.remote_operation_count, 0);
        assert_eq!(app.saved_operation_count, app.operation_journal.len());

        let mut reopened = OpenDocApp::new_empty_document();
        reopened
            .open_saved_projection(&root, app.document.uuid.to_string())
            .expect("reopening what was written");
        assert_eq!(run_text(&reopened), "abcXYdefgh");
        assert_eq!(
            reopened.operation_envelopes.len(),
            1,
            "the remote operation is in the saved history"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// `{block_id, inline_id, offset}` already survives a remote insert in a
    /// different run, because the offset is run-relative.
    #[test]
    fn a_caret_in_another_run_is_untouched_by_a_remote_insert() {
        let mut base = seeded_base("abcdefgh");
        let other = StableId::parse("inl-collab-0002").expect("run id");
        base.blocks[0].content.push(Inline::Text {
            id: other.clone(),
            text: "tail".to_string(),
            marks: Vec::new(),
        });
        let mut app = OpenDocApp::new_empty_document();
        app.join_collaboration_session(local_session(), base, Vec::new())
            .expect("joining");

        let caret = EditorSelection::collapsed(EditorPosition {
            block_id: BLOCK.to_string(),
            inline_id: Some(other.to_string()),
            offset: 2,
        });
        let intake = app
            .apply_remote_operations(vec![remote_insert("actor-remote", 1, 0, "XY")], Some(caret))
            .expect("the operation merges");
        let selection = intake.selection.expect("the caret survives");
        assert_eq!(selection.focus.inline_id.as_deref(), Some(other.as_str()));
        assert_eq!(selection.focus.offset, 2);
    }

    /// The case the identity model does *not* cover on its own: an insert
    /// inside the caret's own run, before it.
    #[test]
    fn a_caret_moves_over_a_remote_insert_in_its_own_run() {
        let mut app = joined("abcdefgh");
        let caret = EditorSelection::collapsed(EditorPosition {
            block_id: BLOCK.to_string(),
            inline_id: Some(RUN.to_string()),
            offset: 6,
        });
        let intake = app
            .apply_remote_operations(vec![remote_insert("actor-remote", 1, 2, "XY")], Some(caret))
            .expect("the operation merges");
        assert_eq!(run_text(&app), "abXYcdefgh");
        let selection = intake.selection.expect("the caret survives");
        assert_eq!(selection.focus.inline_id.as_deref(), Some(RUN));
        assert_eq!(
            selection.focus.offset, 8,
            "two characters landed before the caret"
        );
    }

    #[test]
    fn a_caret_after_a_remote_insert_that_follows_it_does_not_move() {
        let mut app = joined("abcdefgh");
        let caret = EditorSelection::collapsed(EditorPosition {
            block_id: BLOCK.to_string(),
            inline_id: Some(RUN.to_string()),
            offset: 2,
        });
        let intake = app
            .apply_remote_operations(vec![remote_insert("actor-remote", 1, 5, "XY")], Some(caret))
            .expect("the operation merges");
        let selection = intake.selection.expect("the caret survives");
        assert_eq!(selection.focus.offset, 2);
    }

    /// A caret in a block that a remote edit deleted has nowhere to be, and
    /// guessing is the caller's job.
    #[test]
    fn a_caret_in_a_deleted_block_is_reported_as_lost() {
        let mut app = joined("abcdefgh");
        let delete = Operation::in_context(
            OperationId {
                actor: ActorId("actor-remote".to_string()),
                seq: 1,
            },
            OperationKind::DeleteBlock {
                block_id: block_id(),
            },
            CausalContext::default(),
        );
        let caret = EditorSelection::collapsed(EditorPosition {
            block_id: BLOCK.to_string(),
            inline_id: Some(RUN.to_string()),
            offset: 3,
        });
        let intake = app
            .apply_remote_operations(vec![delete], Some(caret))
            .expect("the operation merges");
        assert!(intake.selection.is_none());
    }

    /// A locally authored operation must observe the remote work this replica
    /// has already applied, or the merge reads the two as concurrent and
    /// re-anchors text against a document neither side ever had.
    #[test]
    fn a_local_edit_after_remote_work_observes_it() {
        let mut app = joined("abcdefgh");
        app.apply_remote_operations(vec![remote_insert("actor-remote", 1, 3, "XY")], None)
            .expect("the operation merges");
        app.dispatch_command("add_paragraph", serde_json::json!({ "text": "local work" }))
            .expect("a local command");
        let local = app
            .local_operations_after(0)
            .into_iter()
            .next()
            .expect("the command authored an operation");
        let context = local.context.expect("local operations carry a context");
        assert_eq!(
            context.observed.0.get(&ActorId("actor-remote".to_string())),
            Some(&1),
            "the remote operation is in the local operation's vector clock"
        );
    }

    /// The document a session holds is a pure function of (base, set), so it
    /// must equal a merge computed from scratch — that identity is what makes
    /// agreeing with the server bytes rather than luck.
    #[test]
    fn the_open_document_is_the_merge_of_the_base_and_the_whole_set() {
        let mut app = joined("abcdefgh");
        app.apply_remote_operations(
            vec![
                remote_insert("actor-remote", 1, 3, "XY"),
                remote_insert("actor-remote", 2, 0, "Z"),
            ],
            None,
        )
        .expect("the operations merge");
        app.dispatch_command("add_paragraph", serde_json::json!({ "text": "local work" }))
            .expect("a local command");

        // The oracle, and it has to come first: the comparison below recomputes
        // the *same* call `materialise_from_merge_base` already made, with the
        // same base and the same grouping, so it is `f(x) == f(x)` and a
        // `merge_operations` that dropped every operation and returned the base
        // would satisfy it. PLAN88 §7.
        //
        // Say what the answer is instead, without a merge. The two remote
        // operations come from one actor, so the second observes the first:
        // "abcdefgh" with "XY" inserted after "abc" is "abcXYdefgh", and "Z"
        // then goes at the front of *that*. The local command appends its own
        // paragraph.
        assert_eq!(
            app.document.visible_text(),
            "ZabcXYdefgh\nlocal work\n",
            "the session's document is not the result of the operations it holds"
        );

        let base = app.collaboration_merge_base().cloned().expect("a session");
        let operations = app.collaboration_operations();
        let expected = merge_operations(&base, std::slice::from_ref(&operations))
            .expect("the set merges")
            .document;
        assert_eq!(
            encode_canonical_cbor(&app.document).expect("encoding"),
            encode_canonical_cbor(&expected).expect("encoding"),
        );
    }

    /// An envelope that carries no operation numbers itself and leaves the
    /// document operations' numbering alone.
    ///
    /// One counter used to do both jobs, so an undo marker, a blob upload or a
    /// spreadsheet edit punched a hole in this actor's operation sequence —
    /// and a service refuses a gap rather than storing one, so the next
    /// keystroke after any of those was refused. The two identities are now
    /// separate counters, and this is the assertion that says so.
    #[test]
    fn a_non_document_envelope_numbers_itself_and_not_the_operations() {
        let mut app = joined("abcdefgh");
        app.dispatch_command("add_paragraph", serde_json::json!({ "text": "one" }))
            .expect("a local command");
        assert!(app.local_operations_are_dense_after(0));
        app.push_app_operation("marker", "a non-document envelope");
        app.dispatch_command("add_paragraph", serde_json::json!({ "text": "two" }))
            .expect("a local command");
        assert!(
            app.local_operations_are_dense_after(0),
            "the marker must not consume an operation id: {:?}",
            app.local_operations_after(0)
                .iter()
                .map(|operation| operation.id.seq)
                .collect::<Vec<_>>()
        );

        // And the envelope numbering did advance past it, so the marker still
        // has an identity of its own for a candidate merge to deduplicate on.
        let own_envelope_seqs = app
            .operation_envelopes
            .iter()
            .filter(|envelope| envelope.record.actor == app.actor_id)
            .map(|envelope| envelope.record.seq)
            .collect::<Vec<_>>();
        assert_eq!(
            own_envelope_seqs.len(),
            own_envelope_seqs
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            "envelope identities must stay unique: {own_envelope_seqs:?}"
        );
        assert!(
            own_envelope_seqs.len() > app.local_operations_after(0).len(),
            "the marker has an envelope number of its own"
        );
    }

    /// A blob envelope and a spreadsheet envelope are the same case as the
    /// marker above, reached through the command surface rather than through a
    /// journal helper — which is how a real client hits it.
    #[test]
    fn blob_and_spreadsheet_work_leave_the_operation_sequence_dense() {
        let mut app = joined("abcdefgh");
        app.dispatch_command("add_paragraph", serde_json::json!({ "text": "one" }))
            .expect("a local command");
        app.dispatch_command(
            "add_binary_blob",
            serde_json::json!({
                "name": "note.txt",
                "mediaType": "text/plain",
                "bytes": [104, 105],
            }),
        )
        .expect("a blob command");
        app.dispatch_command(
            "set_spreadsheet_cell",
            serde_json::json!({ "address": "A1", "value": "7" }),
        )
        .expect("a spreadsheet command");
        app.dispatch_command("add_paragraph", serde_json::json!({ "text": "two" }))
            .expect("a local command");
        assert!(
            app.local_operations_are_dense_after(0),
            "blob and spreadsheet envelopes must not consume operation ids: {:?}",
            app.local_operations_after(0)
                .iter()
                .map(|operation| operation.id.seq)
                .collect::<Vec<_>>()
        );
    }

    /// Undo rolls both counters back with the state they belong to, so the
    /// operation the next keystroke mints is the one the undone operation gave
    /// up — not a number past a marker the journal happens to have kept.
    #[test]
    fn undo_leaves_the_operation_sequence_dense() {
        let mut app = joined("abcdefgh");
        app.dispatch_command("add_paragraph", serde_json::json!({ "text": "one" }))
            .expect("a local command");
        app.dispatch_command("add_paragraph", serde_json::json!({ "text": "two" }))
            .expect("a local command");
        app.dispatch_command("undo_current_edit", serde_json::json!({}))
            .expect("undo");
        app.dispatch_command("add_paragraph", serde_json::json!({ "text": "three" }))
            .expect("a local command");
        assert!(
            app.local_operations_are_dense_after(0),
            "undo must not leave a hole: {:?}",
            app.local_operations_after(0)
                .iter()
                .map(|operation| operation.id.seq)
                .collect::<Vec<_>>()
        );
    }

    /// A session's answers are the service's, and leaving gives them up.
    #[test]
    fn a_session_holds_the_services_answers_and_drops_them_on_leaving() {
        let mut app = joined("abcdefgh");
        let session = app.service_session().expect("a session").clone();
        assert_eq!(session.subject, "local");
        assert_eq!(session.actor, "actor-local");
        assert_eq!(session.role, OpenDocServiceRole::Editor);

        app.apply_service_role(OpenDocServiceRole::Viewer);
        app.acknowledge_service_operations(4);
        app.acknowledge_service_operations(2);
        let session = app.service_session().expect("a session");
        assert_eq!(session.role, OpenDocServiceRole::Viewer);
        assert_eq!(
            session.acknowledged_seq, 4,
            "an out-of-order acknowledgement must not walk the watermark back"
        );

        app.leave_collaboration_session();
        assert!(app.service_session().is_none());
    }

    /// A welcome carries this replica's own earlier work. Numbering the next
    /// local operation from 1 again would re-mint an id the service already
    /// holds, and the service refuses that rather than overwriting it.
    #[test]
    fn joining_continues_this_actors_sequence_past_what_the_service_holds() {
        let mut app = OpenDocApp::new_empty_document();
        app.join_collaboration_session(
            local_session(),
            seeded_base("abcdefgh"),
            vec![
                remote_insert("actor-local", 1, 0, "a"),
                remote_insert("actor-local", 2, 0, "b"),
            ],
        )
        .expect("joining a session that already holds this actor's work");
        app.dispatch_command("add_paragraph", serde_json::json!({ "text": "next" }))
            .expect("a local command");
        let minted = app.local_operations_after(2);
        assert_eq!(minted.len(), 1);
        assert_eq!(minted[0].id.seq, 3);
    }

    /// A service refuses an operation whose Lamport timestamp is more than one
    /// above everything its vector clock names — without that check a client
    /// sets `u64::MAX` once and wins every last-writer-wins contest for ever.
    /// A batch's later operations did observe its earlier ones, so they have to
    /// say so rather than only carrying a higher timestamp.
    #[test]
    fn operations_in_one_batch_observe_the_ones_before_them() {
        let mut app = joined("abcdefgh");
        app.split_paragraph_at_inline(RUN)
            .expect("splitting the paragraph at the run");
        let batch = app.local_operations_after(0);
        assert!(batch.len() >= 2, "this command authors a batch: {batch:?}");
        for (index, operation) in batch.iter().enumerate().skip(1) {
            let context = operation.context.as_ref().expect("a context");
            let previous = &batch[index - 1];
            assert_eq!(
                context.observed.0.get(&previous.id.actor),
                Some(&previous.id.seq),
                "operation {index} must observe the one before it"
            );
            let highest_observed = batch[..index]
                .iter()
                .filter(|earlier| {
                    context
                        .observed
                        .0
                        .get(&earlier.id.actor)
                        .copied()
                        .unwrap_or(0)
                        >= earlier.id.seq
                })
                .map(|earlier| earlier.lamport())
                .max()
                .unwrap_or(0);
            assert!(
                context.lamport <= highest_observed + 1,
                "operation {index} claims Lamport {} but observed nothing above {highest_observed}",
                context.lamport
            );
        }
    }

    /// A service is not a trusted author of this repository's bytes: a payload
    /// that would be refused from a local command is refused from the wire too.
    #[test]
    fn a_remote_operation_whose_payload_fails_source_validation_is_refused() {
        let mut app = joined("abcdefgh");
        let padded = Operation::in_context(
            OperationId {
                actor: ActorId("actor-remote".to_string()),
                seq: 1,
            },
            OperationKind::SetDocumentTitle {
                title: "  padded  ".to_string(),
            },
            CausalContext::default(),
        );
        let error = app
            .apply_remote_operations(vec![padded], None)
            .expect_err("the payload is not canonical source state");
        assert!(matches!(error, AppApiError::Format(_)), "{error:?}");
        assert!(
            app.operation_envelopes.is_empty(),
            "a refused operation must not reach the journal"
        );
    }

    #[test]
    fn an_offset_inside_a_replaced_span_falls_back_to_its_start() {
        // "abcdef" -> "abXYef": "cd" became "XY", so the changed span is
        // [2, 4). A caret inside it has nowhere of its own left and goes to
        // its start; one at either edge keeps the edge it was on.
        assert_eq!(rebase_offset("abcdef", "abXYef", 3), 2);
        assert_eq!(rebase_offset("abcdef", "abXYef", 2), 2);
        assert_eq!(rebase_offset("abcdef", "abXYef", 4), 4);
        assert_eq!(rebase_offset("abcdef", "abXYZef", 5), 6);
    }
}
