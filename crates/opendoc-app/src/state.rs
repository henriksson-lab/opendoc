use super::*;
use std::sync::Arc;

/// Number of most recent operation records included in a document projection.
const PROJECTED_OPERATION_LIMIT: usize = 200;
#[derive(Clone, Debug)]
pub struct OpenDocApp {
    pub(crate) actor_id: String,
    pub(crate) document: Document,
    pub(crate) workbook: AppSpreadsheetWorkbook,
    pub(crate) blobs: Vec<AppBlobRef>,
    pub(crate) blob_bytes: BTreeMap<String, Vec<u8>>,
    /// The blob bytes the last checkpoint captured, shared with it.
    ///
    /// `blob_bytes` is keyed by the SHA-256 of its values, so two maps with
    /// the same key set hold the same bytes — which is what lets a checkpoint
    /// *share* the previous one's copy instead of taking its own. Without it
    /// every undoable command deep-copied every embedded image, up to 200
    /// times over, which is a memory cliff on a document with pictures in it.
    /// See `docs/adr/0017-collaborative-undo-as-inverse-operations.md`,
    /// "What this costs".
    pub(crate) blob_bytes_snapshot: Option<Arc<BTreeMap<String, Vec<u8>>>>,
    pub(crate) blob_signatures: BTreeMap<String, Vec<opendoc_format::SignatureRecord>>,
    pub(crate) blob_tombstones: BTreeMap<String, AppArchiveTombstone>,
    pub(crate) blob_tombstone_records: BTreeMap<String, opendoc_format::TombstoneRecord>,
    pub(crate) signatures: Vec<opendoc_format::SignatureRecord>,
    pub(crate) operation_journal: Vec<AppOperationRecord>,
    pub(crate) operation_envelopes: Vec<AppOperationEnvelope>,
    pub(crate) undo_stack: Vec<AppUndoCheckpoint>,
    pub(crate) redo_stack: Vec<AppUndoCheckpoint>,
    /// The inverse of every document operation this replica has minted, keyed
    /// by the operation it undoes.
    ///
    /// Captured when the operation is written, because that is the only moment
    /// the state its inverse has to carry — the block a delete removed, the
    /// value a set overwrote — still exists. This is what makes undo an
    /// ordinary edit rather than a rewind: see
    /// `docs/adr/0017-collaborative-undo-as-inverse-operations.md`.
    pub(crate) operation_inverses: BTreeMap<OperationId, Inversion>,
    pub(crate) is_open: bool,
    pub(crate) saved_projection: Option<AppDocument>,
    pub(crate) repository_root: Option<PathBuf>,
    pub(crate) repository_backend: Option<String>,
    pub(crate) repository_namespace: Option<String>,
    /// The recents list *and* the storage it survives a restart in.
    ///
    /// Not a bare `Vec`: every mutation goes through [`RecentDocuments`], which
    /// dedupes, caps and writes the list back, so the moments worth
    /// remembering (a save, an open, a scan) cannot record one without storing
    /// it. A runtime that installed no store still gets a working list for the
    /// life of the process — see `recent.rs`.
    pub(crate) recent_documents: RecentDocuments,
    pub(crate) last_manifest: Option<String>,
    pub(crate) saved_operation_count: usize,
    pub(crate) saved_signature_count: usize,
    pub(crate) defer_spreadsheet_evaluation: bool,
    /// Numbers **envelopes**: one per journal entry, whatever it carries.
    ///
    /// This is the identity a candidate merge deduplicates on and the recovery
    /// segment tracks, so every envelope needs one — including the ones that
    /// carry no typed operation at all (undo and redo markers, blob work,
    /// spreadsheet work).
    pub(crate) next_envelope_seq: u64,
    /// Numbers this actor's **document operations**: the `seq` half of
    /// `OperationId`, and nothing else.
    ///
    /// It is a second counter because it answers a different question.
    /// `VectorClock::observed` reads `seq >= n` as "every one of that actor's
    /// operations up to n", which is only true if an actor's operation
    /// sequence is dense — and a collaboration service refuses a gap rather
    /// than storing one (ADR 0015, "Sequence density"). One shared counter
    /// made every undo marker, blob upload and spreadsheet edit punch a hole
    /// in the document operations' numbering, so a client's next keystroke was
    /// refused by the server through no fault of its own.
    pub(crate) next_operation_seq: u64,
    /// The merge base of an open collaboration session: the document the
    /// whole shared operation log is anchored to, exactly as the service
    /// holds it. `None` when this replica is not a client of a service.
    ///
    /// While it is `Some`, the open document is re-materialised as
    /// `merge_operations(base, every logged operation)` rather than by
    /// folding each new operation into the previous result. ADR 0007 is the
    /// reason: a merge result written back as plain text is a base against
    /// which a late-arriving concurrent operation can only be placed
    /// positionally, which is the corruption that ADR removed. The service
    /// re-merges from genesis for the same reason.
    pub(crate) merge_base: Option<Document>,
    /// How many of the journal's unsaved envelopes arrived from the service
    /// rather than from this user. They are durable on the service before it
    /// acknowledged them, so they are not *locally* unsaved work — but the
    /// local repository does not have them, so a local save must still write
    /// them. Those are two different questions, and this is the difference
    /// between them.
    pub(crate) remote_operation_count: usize,
    /// Key and timestamp of the last coalescable editor gesture so that
    /// continuous typing forms one undo step.
    pub(crate) undo_coalesce: Option<(String, u64)>,
    /// Durable shadow of the operation journal, plus whatever an unclean
    /// prior session left behind. Empty until a runtime installs a store.
    pub(crate) recovery: RecoveryJournal,
    /// The collaboration service's answers about this session, when a
    /// transport has joined one.
    ///
    /// The *only* writer is `join_collaboration_session` and the presence and
    /// acknowledgement helpers next to it, all of which are called with what
    /// the socket delivered. No command argument reaches this field, which is
    /// what makes `authorize_runtime_command` an answer rather than an
    /// assertion.
    pub(crate) service_session: Option<OpenDocServiceSession>,
}

/// Maximum number of undo checkpoints retained.
pub(crate) const UNDO_STACK_LIMIT: usize = 200;
/// Typing gestures closer together than this (ms) share one undo step.
pub(crate) const UNDO_COALESCE_WINDOW_MS: u64 = 1_000;

/// Which direction along the edit history one gesture moves.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HistoryStep {
    Undo,
    Redo,
}

impl HistoryStep {
    fn verb(self) -> &'static str {
        match self {
            Self::Undo => "undo",
            Self::Redo => "redo",
        }
    }

    fn past(self) -> &'static str {
        match self {
            Self::Undo => "undone",
            Self::Redo => "redone",
        }
    }

    fn summary(self) -> &'static str {
        match self {
            Self::Undo => "undo current edit",
            Self::Redo => "redo current edit",
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct AppUndoCheckpoint {
    pub(crate) document: Document,
    pub(crate) workbook: AppSpreadsheetWorkbook,
    pub(crate) blobs: Vec<AppBlobRef>,
    pub(crate) blob_bytes: Arc<BTreeMap<String, Vec<u8>>>,
    pub(crate) blob_signatures: BTreeMap<String, Vec<opendoc_format::SignatureRecord>>,
    pub(crate) blob_tombstones: BTreeMap<String, AppArchiveTombstone>,
    pub(crate) blob_tombstone_records: BTreeMap<String, opendoc_format::TombstoneRecord>,
    pub(crate) signatures: Vec<opendoc_format::SignatureRecord>,
    pub(crate) operation_journal: Vec<AppOperationRecord>,
    pub(crate) operation_envelopes: Vec<AppOperationEnvelope>,
    pub(crate) is_open: bool,
    pub(crate) saved_projection: Option<AppDocument>,
    pub(crate) repository_root: Option<PathBuf>,
    pub(crate) repository_backend: Option<String>,
    pub(crate) repository_namespace: Option<String>,
    pub(crate) last_manifest: Option<String>,
    pub(crate) saved_operation_count: usize,
    pub(crate) saved_signature_count: usize,
    pub(crate) remote_operation_count: usize,
    pub(crate) next_envelope_seq: u64,
    pub(crate) next_operation_seq: u64,
    /// Where the step this checkpoint opened *ends* in the journal, once it is
    /// known. `None` means the step is still the newest thing that happened,
    /// so it ends at whatever the journal has reached.
    ///
    /// A checkpoint is pushed before the command runs, so its own end cannot
    /// be recorded then. It is recorded by the next [`OpenDocApp::checkpoint`]
    /// — the moment another step begins is the moment this one is over — which
    /// is what stops an undo of an older step from reaching over a newer one,
    /// or over the operations a previous undo appended.
    pub(crate) step_end: Option<usize>,
}

/// The title a document carries until the user names it.
pub(crate) const UNTITLED_DOCUMENT: &str = "Untitled document";
/// Identity of the sheet a blank workbook starts with.
const FIRST_SHEET_ID: &str = "sheet-1";
const FIRST_SHEET_TITLE: &str = "Sheet1";

impl OpenDocApp {
    /// The state a runtime boots into, and the state `create_document`
    /// resets to: one empty paragraph and one empty sheet.
    ///
    /// Demo content is a test fixture (`new_sample`), never the state an
    /// editor starts in — `docs/RESTRUCTURE_PLAN.md` Phase 6, "make sample
    /// fixtures explicit test/demo data, never default runtime state".
    pub fn new_empty_document() -> Self {
        Self::empty_titled(UNTITLED_DOCUMENT)
    }

    /// `new_empty_document` under a caller-supplied title.
    pub(crate) fn empty_titled(title: &str) -> Self {
        let mut app = Self::contentless(title);
        // One paragraph so there is somewhere to put the caret; the editor
        // cannot resolve a selection against a document with no blocks.
        app.document.blocks.push(Block::paragraph(""));
        app
    }

    /// A workbook with one empty sheet.
    ///
    /// Not `SpreadsheetWorkbook::sample()`, which is the "Prototype Sheet"
    /// demo data — a blank spreadsheet must be blank.
    pub(crate) fn blank_workbook(title: &str) -> AppSpreadsheetWorkbook {
        let mut workbook = AppSpreadsheetWorkbook::empty(title);
        workbook.add_sheet_with_id(FIRST_SHEET_ID, FIRST_SHEET_TITLE);
        workbook
    }

    /// Every field at its zero value and no content on either surface.
    /// Callers add whatever content the state they are building requires.
    fn contentless(title: &str) -> Self {
        Self {
            actor_id: StableId::new("actor").to_string(),
            document: Document::new(title),
            workbook: Self::blank_workbook(title),
            blobs: Vec::new(),
            blob_bytes: BTreeMap::new(),
            blob_bytes_snapshot: None,
            blob_signatures: BTreeMap::new(),
            blob_tombstones: BTreeMap::new(),
            blob_tombstone_records: BTreeMap::new(),
            signatures: Vec::new(),
            operation_journal: Vec::new(),
            operation_envelopes: Vec::new(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            operation_inverses: BTreeMap::new(),
            is_open: true,
            saved_projection: None,
            repository_root: None,
            repository_backend: None,
            repository_namespace: None,
            recent_documents: RecentDocuments::default(),
            last_manifest: None,
            saved_operation_count: 0,
            saved_signature_count: 0,
            defer_spreadsheet_evaluation: false,
            next_envelope_seq: 1,
            next_operation_seq: 1,
            merge_base: None,
            remote_operation_count: 0,
            undo_coalesce: None,
            recovery: RecoveryJournal::default(),
            service_session: None,
        }
    }

    /// Demo content exercising most of the model: headings, links, an inline
    /// equation, a table, a citation, a comment and a suggestion.
    ///
    /// A test fixture, not a constructor a runtime may boot into — hence
    /// `cfg(test)`, which makes booting into it a compile error rather than a
    /// convention.
    #[cfg(test)]
    pub(crate) fn new_sample() -> Self {
        let mut app = Self {
            actor_id: StableId::new("actor").to_string(),
            document: Document::new(UNTITLED_DOCUMENT),
            workbook: AppSpreadsheetWorkbook::sample(),
            blobs: Vec::new(),
            blob_bytes: BTreeMap::new(),
            blob_bytes_snapshot: None,
            blob_signatures: BTreeMap::new(),
            blob_tombstones: BTreeMap::new(),
            blob_tombstone_records: BTreeMap::new(),
            signatures: Vec::new(),
            operation_journal: Vec::new(),
            operation_envelopes: Vec::new(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            operation_inverses: BTreeMap::new(),
            is_open: true,
            saved_projection: None,
            repository_root: None,
            repository_backend: None,
            repository_namespace: None,
            recent_documents: RecentDocuments::default(),
            last_manifest: None,
            saved_operation_count: 0,
            saved_signature_count: 0,
            defer_spreadsheet_evaluation: false,
            next_envelope_seq: 1,
            next_operation_seq: 1,
            merge_base: None,
            remote_operation_count: 0,
            undo_coalesce: None,
            recovery: RecoveryJournal::default(),
            service_session: None,
        };
        app.document
            .blocks
            .push(Block::paragraph("OpenDoc editing surface"));
        app.add_heading("Schema coverage", 2)
            .expect("sample heading level is valid");
        app.add_paragraph(
            "This prototype renders the current docs-like model through a TypeScript UI.",
        )
        .expect("paragraph");
        app.add_link("Project note", "https://example.invalid/opendoc")
            .expect("sample link is valid");
        app.add_equation_inline("E=mc^2")
            .expect("sample inline equation is valid");
        app.add_table().expect("a table");
        app.add_sample_citation().expect("sample citation");
        app.add_comment(
            "Alice",
            "Comment threads are part of signed document state.",
        )
        .expect("sample comment is valid");
        app.add_suggestion("Bob", "Suggested replacement text")
            .expect("sample suggestion is valid");
        // The sample document is a starting point, not unsaved work.
        app.saved_operation_count = app.operation_journal.len();
        app.saved_signature_count = app.signature_count();
        app
    }

    pub fn new_document(&mut self, title: impl Into<String>) -> AppDocument {
        let title = title.into();
        let title = if title.trim().is_empty() {
            UNTITLED_DOCUMENT.to_string()
        } else {
            title
        };
        let empty = Self::empty_titled(&title);
        self.document = empty.document;
        // FS-19: a new document starts blank. Tests that want demo content
        // seed it themselves rather than relying on the constructor.
        self.workbook = empty.workbook;
        self.clear_blob_state();
        self.invalidate_source_state();
        self.clear_edit_history();
        self.is_open = true;
        self.clear_repository_binding();
        self.document()
    }

    pub fn document(&self) -> AppDocument {
        let mut document = self
            .projection_service()
            .document(PROJECTED_OPERATION_LIMIT);
        document.recovery_sessions = self.recovery_sessions();
        document
    }

    pub(crate) fn document_with_doi_lookup_warning(
        &mut self,
        mut document: AppDocument,
        doi: &str,
        used_scan: bool,
        scan_warnings: Vec<String>,
    ) -> AppDocument {
        if !used_scan {
            return document;
        }
        let warning = ModelWarning {
            code: "doi-lookup-scan-fallback".to_string(),
            message: format!(
                "DOI lookup index for {doi} was missing or stale; repository snapshots were scanned"
            ),
        };
        if !self
            .document
            .warnings
            .iter()
            .any(|existing| existing == &warning)
        {
            self.document.warnings.push(warning.clone());
        }
        let app_warning = AppWarning::from_core(&warning);
        if !document
            .warnings
            .iter()
            .any(|existing| existing == &app_warning)
        {
            document.warnings.push(app_warning);
        }
        for message in scan_warnings {
            let warning = ModelWarning {
                code: "doi-lookup-scan-problem".to_string(),
                message,
            };
            if !self
                .document
                .warnings
                .iter()
                .any(|existing| existing == &warning)
            {
                self.document.warnings.push(warning.clone());
            }
            let app_warning = AppWarning::from_core(&warning);
            if !document
                .warnings
                .iter()
                .any(|existing| existing == &app_warning)
            {
                document.warnings.push(app_warning);
            }
        }
        document
    }

    pub(crate) fn push_model_warning(&mut self, code: &str, message: impl Into<String>) {
        let warning = ModelWarning {
            code: code.to_string(),
            message: message.into(),
        };
        if !self
            .document
            .warnings
            .iter()
            .any(|existing| existing == &warning)
        {
            self.document.warnings.push(warning);
        }
    }

    pub fn close_document(&mut self) -> AppDocument {
        self.document = Document::new("No document open");
        self.workbook = Self::blank_workbook("No document open");
        self.clear_blob_state();
        self.invalidate_source_state();
        self.clear_edit_history();
        self.is_open = false;
        self.clear_repository_binding();
        self.defer_spreadsheet_evaluation = false;
        self.document()
    }

    pub fn undo_current_edit(&mut self) -> Result<AppDocument, AppApiError> {
        self.step_edit_history(HistoryStep::Undo)
    }

    pub fn redo_current_edit(&mut self) -> Result<AppDocument, AppApiError> {
        self.step_edit_history(HistoryStep::Redo)
    }

    /// Undo and redo are one function because they are one mechanism: take the
    /// step off one stack, say the opposite of it, and put what was just
    /// reversed onto the other stack. A redo is the inverse of an inverse, and
    /// there is nothing else to it.
    ///
    /// **Inverse operations, not a rewind.** A step made only of this actor's
    /// own document operations is undone by submitting the inverse of each,
    /// newest first, as ordinary new operations. They get fresh
    /// `OperationId`s, so nothing rolls the sequence back below what a service
    /// already made durable, and they name only this actor's own contribution,
    /// so a collaborator's concurrent edit to the same run or the same block
    /// survives. ADR 0017.
    ///
    /// **The snapshot is the fallback, not the mechanism.** A step that also
    /// moved state no typed document operation describes — a spreadsheet edit,
    /// a blob upload — or that contains an operation the vocabulary cannot
    /// invert, is still undone by restoring the whole-state checkpoint. That
    /// keeps single-user undo exactly as it was, and it is refused outright
    /// while a collaboration session is open, because restoring a snapshot
    /// taken before a remote edit would delete that edit locally while the
    /// service still holds it.
    /// **Skip and continue.** A step that needs the snapshot inside a session
    /// cannot be reversed at all — restoring a whole-state snapshot would
    /// delete a collaborator's edit that the service already holds — but
    /// refusing it used to *put it back on the stack*, so the very next
    /// `Ctrl+Z` met the same step and refused again. One spreadsheet edit, one
    /// image upload, or one delete of a table column that held content
    /// therefore wedged undo permanently, taking every ordinary keystroke
    /// underneath it with it.
    ///
    /// The step is now dropped rather than replayed: the edit it describes
    /// becomes permanent, which is the honest outcome (nothing can reverse
    /// it), and the search continues to the step below, which usually can be
    /// reversed. The user is told, once, which is what makes "your edit is now
    /// permanent" a report rather than a silence. Only when *no* step in the
    /// stack can be reversed is the stack put back exactly as it was and the
    /// refusal returned — so a refusal still never costs history it could
    /// have kept.
    fn step_edit_history(&mut self, step: HistoryStep) -> Result<AppDocument, AppApiError> {
        // Popped and found un-reversible, newest first. Restored verbatim if
        // the whole stack turns out to be un-reversible.
        let mut skipped: Vec<AppUndoCheckpoint> = Vec::new();
        loop {
            let popped = match step {
                HistoryStep::Undo => self.undo_stack.pop(),
                HistoryStep::Redo => self.redo_stack.pop(),
            };
            let Some(target) = popped else {
                let exhausted = !skipped.is_empty();
                for checkpoint in skipped.into_iter().rev() {
                    self.push_same(step, checkpoint);
                }
                return Err(AppApiError::Conflict(if exhausted {
                    format!(
                        "nothing can be {} during a collaboration session: every step left moved state no document operation describes, and restoring a snapshot of one would discard work the service has already accepted from other actors",
                        step.past()
                    )
                } else {
                    format!("nothing to {}", step.verb())
                }));
            };
            // The state this step ended in, which is the checkpoint of
            // whatever step came next — the one just above it on the stack, if
            // any was skipped — and otherwise the state as it stands now.
            let inverse = self.inverse_of_step(&target, skipped.last());
            match inverse {
                Some(operations) => {
                    let dropped = skipped.len();
                    let reversed = self.checkpoint();
                    if !operations.is_empty() {
                        self.apply_batch(
                            operations
                                .iter()
                                .map(|kind| (step.verb(), step.summary(), kind.clone()))
                                .collect(),
                        )?;
                    }
                    self.push_opposite(step, reversed);
                    if dropped > 0 {
                        self.push_model_warning(
                            "undo-step-skipped",
                            format!(
                                "{dropped} edit(s) could not be {} during a collaboration session — they moved state no document operation describes — so they are now permanent and the {} reached the edit before them",
                                step.past(),
                                step.verb()
                            ),
                        );
                    }
                    return Ok(self.document());
                }
                None if self.merge_base.is_some() => {
                    // Un-reversible *and* un-restorable. Set it aside and look
                    // further down rather than refusing for ever.
                    skipped.push(target);
                }
                None => {
                    // Outside a session the snapshot is a legitimate
                    // mechanism, so this step is reversible after all and
                    // nothing above it was ever skipped.
                    let current = self.checkpoint();
                    self.restore_checkpoint(target);
                    self.invalidate_source_state();
                    self.push_opposite(step, current);
                    self.push_app_operation(step.verb(), step.summary());
                    return Ok(self.document());
                }
            }
        }
    }

    fn push_opposite(&mut self, step: HistoryStep, checkpoint: AppUndoCheckpoint) {
        match step {
            HistoryStep::Undo => self.redo_stack.push(checkpoint),
            HistoryStep::Redo => self.undo_stack.push(checkpoint),
        }
    }

    fn push_same(&mut self, step: HistoryStep, checkpoint: AppUndoCheckpoint) {
        match step {
            HistoryStep::Undo => self.undo_stack.push(checkpoint),
            HistoryStep::Redo => self.redo_stack.push(checkpoint),
        }
    }

    /// The operations that reverse everything journalled since `checkpoint`,
    /// or `None` when the step has to be undone by restoring the snapshot.
    ///
    /// `None` is returned rather than an error because it is not a failure: it
    /// is the honest answer for a step that moved something the operation
    /// vocabulary does not describe. Every reason for it is a *structural*
    /// property of the step, checked here rather than assumed:
    ///
    /// * an envelope carrying a spreadsheet or blob payload, or no payload at
    ///   all — the service has no path for either (ADR 0015) and neither has
    ///   an inverse operation;
    /// * an operation another actor authored, which this actor may not undo;
    /// * an operation whose inverse the vocabulary cannot express, which
    ///   [`Inversion::Irreversible`] names;
    /// * state outside the operation log moving under the step.
    fn inverse_of_step(
        &self,
        checkpoint: &AppUndoCheckpoint,
        ended_in: Option<&AppUndoCheckpoint>,
    ) -> Option<Vec<OperationKind>> {
        let start = checkpoint.operation_envelopes.len();
        let end = checkpoint
            .step_end
            .unwrap_or(self.operation_envelopes.len())
            .min(self.operation_envelopes.len());
        if start > end {
            return None;
        }
        // "Did state outside the operation log move **under this step**" — so
        // the comparison is against the state the step *ended* in, not against
        // the state as it stands now. Those are the same thing for the newest
        // step, and only for it: comparing a deeper step against the present
        // asked whether anything had changed the workbook since, which made
        // one spreadsheet edit poison every ordinary edit beneath it.
        let (workbook, blobs, blob_tombstones) = match ended_in {
            Some(next) => (&next.workbook, &next.blobs, &next.blob_tombstones),
            None => (&self.workbook, &self.blobs, &self.blob_tombstones),
        };
        if &checkpoint.workbook != workbook
            || &checkpoint.blobs != blobs
            || &checkpoint.blob_tombstones != blob_tombstones
        {
            return None;
        }
        let mut appended: Vec<Operation> = Vec::new();
        for envelope in &self.operation_envelopes[start..end] {
            match &envelope.operation {
                // Another actor's operation that landed inside this step's
                // window. Not part of the step, and not this actor's to undo.
                Some(operation) if operation.id.actor.0 != self.actor_id => {}
                Some(operation) if envelope.spreadsheet.is_none() && envelope.blob.is_none() => {
                    appended.push(operation.clone())
                }
                // A spreadsheet envelope, a blob envelope, or a marker
                // carrying no typed operation at all. None of the three has an
                // inverse operation, so the step needs the snapshot.
                _ => return None,
            }
        }

        // The (base, set) pair the current document is a merge of. In a
        // session that is the service's merge base and the whole log; outside
        // one it is the state this step started from and just this step's
        // operations. Either describes the same runs, because the merge is a
        // pure function of the pair.
        let (base, set) = match self.merge_base.as_ref() {
            Some(base) => (base, self.collaboration_operations()),
            None => (&checkpoint.document, appended.clone()),
        };

        // Walked newest first, so an inverse that removes something comes
        // after the inverse that put it back.
        //
        // A run's character inverses are emitted as one block, at the point
        // the run's *latest* character operation appears in this walk. They
        // have to be one block because they share a coordinate space, and that
        // is the right place for it: a step that character-edits a run and
        // then deletes the block around it inverts to "put the block back,
        // then put the characters back", while a step that creates a run and
        // then types into it inverts to "unpick the typing, then remove the
        // run".
        let mut inverse = Vec::new();
        let mut runs_done: Vec<StableId> = Vec::new();
        for operation in appended.iter().rev() {
            match self.operation_inverses.get(&operation.id)? {
                Inversion::Irreversible(_) => return None,
                Inversion::Nothing => {}
                Inversion::Operations(kinds) => inverse.extend(kinds.iter().cloned()),
                // An offset means something different once a concurrent edit
                // has landed in the same run, so these are resolved now,
                // against the run's character identities as the merge derives
                // them. ADR 0017.
                Inversion::Deferred => {
                    let run = text_run_of(&operation.kind)?;
                    if runs_done.contains(run) {
                        continue;
                    }
                    runs_done.push(run.clone());
                    let targets: Vec<OperationId> = appended
                        .iter()
                        .filter(|candidate| text_run_of(&candidate.kind) == Some(run))
                        .map(|candidate| candidate.id.clone())
                        .collect();
                    match invert_text_operations(base, &set, &targets) {
                        Inversion::Operations(kinds) => inverse.extend(kinds),
                        Inversion::Nothing => {}
                        _ => return None,
                    }
                }
            }
        }
        Some(inverse)
    }

    /// A whole-state snapshot, and the close of whatever step was open.
    ///
    /// Taking a checkpoint is how a step begins, so it is also how the
    /// previous one ends: the journal has reached exactly the operations that
    /// step produced, and nothing it produces later belongs to it. Recording
    /// that here rather than in the dispatcher means no caller has to remember
    /// to, including the undo path itself.
    pub(crate) fn checkpoint(&mut self) -> AppUndoCheckpoint {
        let frontier = self.operation_envelopes.len();
        for stack in [&mut self.undo_stack, &mut self.redo_stack] {
            if let Some(open) = stack.last_mut() {
                if open.step_end.is_none() {
                    open.step_end = Some(frontier);
                }
            }
        }
        AppUndoCheckpoint {
            document: self.document.clone(),
            workbook: self.workbook.clone(),
            blobs: self.blobs.clone(),
            blob_bytes: self.shared_blob_bytes(),
            blob_signatures: self.blob_signatures.clone(),
            blob_tombstones: self.blob_tombstones.clone(),
            blob_tombstone_records: self.blob_tombstone_records.clone(),
            signatures: self.signatures.clone(),
            operation_journal: self.operation_journal.clone(),
            operation_envelopes: self.operation_envelopes.clone(),
            is_open: self.is_open,
            saved_projection: self.saved_projection.clone(),
            repository_root: self.repository_root.clone(),
            repository_backend: self.repository_backend.clone(),
            repository_namespace: self.repository_namespace.clone(),
            last_manifest: self.last_manifest.clone(),
            saved_operation_count: self.saved_operation_count,
            saved_signature_count: self.saved_signature_count,
            remote_operation_count: self.remote_operation_count,
            next_envelope_seq: self.next_envelope_seq,
            next_operation_seq: self.next_operation_seq,
            step_end: None,
        }
    }

    /// The blob bytes a checkpoint captures, reusing the previous
    /// checkpoint's copy when nothing has been added or removed since.
    ///
    /// Sound because the map is content-addressed: the keys are SHA-256
    /// digests of the values, so an identical key set *is* identical bytes.
    /// Comparing keys is O(blobs); copying the values is O(bytes).
    fn shared_blob_bytes(&mut self) -> Arc<BTreeMap<String, Vec<u8>>> {
        if let Some(cached) = self.blob_bytes_snapshot.as_ref() {
            if cached.len() == self.blob_bytes.len() && cached.keys().eq(self.blob_bytes.keys()) {
                return Arc::clone(cached);
            }
        }
        let fresh = Arc::new(self.blob_bytes.clone());
        self.blob_bytes_snapshot = Some(Arc::clone(&fresh));
        fresh
    }

    fn restore_checkpoint(&mut self, checkpoint: AppUndoCheckpoint) {
        self.document = checkpoint.document;
        self.workbook = checkpoint.workbook;
        self.blobs = checkpoint.blobs;
        self.blob_bytes = (*checkpoint.blob_bytes).clone();
        self.blob_bytes_snapshot = Some(checkpoint.blob_bytes);
        self.blob_signatures = checkpoint.blob_signatures;
        self.blob_tombstones = checkpoint.blob_tombstones;
        self.blob_tombstone_records = checkpoint.blob_tombstone_records;
        self.signatures = checkpoint.signatures;
        self.operation_journal = checkpoint.operation_journal;
        self.operation_envelopes = checkpoint.operation_envelopes;
        self.is_open = checkpoint.is_open;
        self.saved_projection = checkpoint.saved_projection;
        self.repository_root = checkpoint.repository_root;
        self.repository_backend = checkpoint.repository_backend;
        self.repository_namespace = checkpoint.repository_namespace;
        self.last_manifest = checkpoint.last_manifest;
        self.saved_signature_count = checkpoint.saved_signature_count;
        self.saved_operation_count = checkpoint
            .saved_operation_count
            .min(self.operation_envelopes.len());
        self.remote_operation_count = checkpoint
            .remote_operation_count
            .min(self.operation_envelopes.len() - self.saved_operation_count);
        self.next_envelope_seq = checkpoint.next_envelope_seq;
        self.next_operation_seq = checkpoint.next_operation_seq;
    }

    /// Journal entries that are not unsaved *local* work.
    ///
    /// The repository already holds the first `saved_operation_count`, and the
    /// service already holds the remote envelopes counted after them, so
    /// neither is work that exists only here. This is what the dirty flag is
    /// measured against; `saved_operation_count` on its own is what a local
    /// save writes from, and once remote operations exist those are two
    /// different numbers.
    pub(crate) fn settled_operation_count(&self) -> usize {
        self.saved_operation_count
            .saturating_add(self.remote_operation_count)
            .min(self.operation_journal.len())
    }

    /// Every document mutation in the app goes through here, and a merge that
    /// refuses one now **reaches the caller**.
    ///
    /// It used to be dropped: `DocumentOperationService::apply` returned `()`
    /// and swallowed the error, so a refused gesture left the document
    /// untouched, journalled nothing, and reported success all the way out
    /// through `dispatch_command`. Failing here leaves the document exactly as
    /// it was — the operation was never applied — so the error is also safe to
    /// propagate: there is no half-applied state behind it.
    pub(crate) fn apply(
        &mut self,
        operation_kind: &str,
        summary: &str,
        kind: OperationKind,
    ) -> Result<AppDocument, AppApiError> {
        self.invalidate_source_state();
        let adjacent_runs_before = adjacent_list_run_pairs(&self.document.blocks);
        self.document_operation_service()
            .apply(operation_kind, summary, kind)?;
        self.merge_newly_adjacent_list_runs(&adjacent_runs_before)?;
        self.remateralise_in_session();
        Ok(self.document())
    }

    pub(crate) fn apply_batch(
        &mut self,
        operations: Vec<(&str, &str, OperationKind)>,
    ) -> Result<AppDocument, AppApiError> {
        self.invalidate_source_state();
        let adjacent_runs_before = adjacent_list_run_pairs(&self.document.blocks);
        self.document_operation_service().apply_batch(operations)?;
        self.merge_newly_adjacent_list_runs(&adjacent_runs_before)?;
        self.remateralise_in_session();
        Ok(self.document())
    }

    /// Restores list-run identity after an edit joined two lists.
    ///
    /// Deleting the paragraph between two lists leaves two runs where the
    /// user now sees one list, so numbering restarts in the middle of it.
    /// Every document mutation reaches the journal through `apply` and
    /// `apply_batch`, so this is the one place the repair belongs: no
    /// deletion path has to remember to call it, and a deletion path added
    /// later gets it for free.
    ///
    /// The repair is expressed as ordinary journalled operations, not as a
    /// quiet rewrite of `self.document`: a replica replaying the journal has
    /// to reach the same document, and a signature covers source state.
    fn merge_newly_adjacent_list_runs(
        &mut self,
        adjacent_runs_before: &BTreeSet<(StableId, StableId)>,
    ) -> Result<(), AppApiError> {
        let newly_adjacent = adjacent_list_run_pairs(&self.document.blocks)
            .difference(adjacent_runs_before)
            .cloned()
            .collect::<BTreeSet<_>>();
        if newly_adjacent.is_empty() {
            return Ok(());
        }
        let operations = list_run_merge_operations(&self.document.blocks, &newly_adjacent);
        if operations.is_empty() {
            return Ok(());
        }
        self.document_operation_service().apply_batch(operations)
    }

    fn document_operation_service(&mut self) -> DocumentOperationService<'_> {
        DocumentOperationService::new(
            &self.actor_id,
            &mut self.document,
            &mut self.operation_journal,
            &mut self.operation_envelopes,
            &mut self.next_envelope_seq,
            &mut self.next_operation_seq,
            &mut self.operation_inverses,
        )
    }

    pub(crate) fn projection_service(&self) -> AppProjectionService<'_> {
        AppProjectionService::new(
            &self.document,
            &self.workbook,
            &self.blobs,
            &self.blob_bytes,
            &self.blob_signatures,
            &self.blob_tombstones,
            &self.signatures,
            &self.operation_journal,
            self.is_open,
            &self.saved_projection,
            &self.repository_root,
            &self.repository_backend,
            &self.repository_namespace,
            &self.recent_documents,
            &self.last_manifest,
            self.settled_operation_count(),
            self.saved_signature_count,
            self.defer_spreadsheet_evaluation,
        )
    }
}

#[cfg(test)]
mod list_run_merge_tests {
    use crate::OpenDocApp;
    use opendoc_core::{new_list_id, Block, BlockKind, BlockProperties, ListKind, StableId};

    fn list_item(list_id: &StableId, kind: ListKind, text: &str) -> Block {
        let mut block = Block::paragraph(text);
        block.kind = BlockKind::ListItem {
            list_id: list_id.clone(),
            level: 0,
            kind,
        };
        block
    }

    fn list_ids(app: &OpenDocApp) -> Vec<StableId> {
        app.document
            .blocks
            .iter()
            .filter_map(|block| match &block.kind {
                BlockKind::ListItem { list_id, .. } => Some(list_id.clone()),
                _ => None,
            })
            .collect()
    }

    /// Two lists separated by one paragraph are one list once that paragraph
    /// is gone — numbering included, which is what the shared run id buys.
    #[test]
    fn deleting_the_paragraph_between_two_lists_merges_their_runs() {
        let mut app = OpenDocApp::new_empty_document();
        app.document.blocks.clear();
        let first = new_list_id();
        let second = new_list_id();
        app.document
            .blocks
            .push(list_item(&first, ListKind::Ordered, "one"));
        app.document.blocks.push(Block::paragraph("between"));
        app.document
            .blocks
            .push(list_item(&second, ListKind::Ordered, "two"));
        app.document
            .blocks
            .push(list_item(&second, ListKind::Ordered, "three"));
        let between = app.document.blocks[1].id.clone();

        app.delete_block(between.to_string())
            .expect("the paragraph exists");

        let ids = list_ids(&app);
        assert_eq!(ids.len(), 3, "{ids:?}");
        assert!(
            ids.iter().all(|id| id == &ids[0]),
            "the three items should share one run: {ids:?}"
        );
        // The repair must be journalled, not a quiet rewrite of source state,
        // or a replica replaying this journal ends up with two runs.
        assert!(
            app.operation_journal
                .iter()
                .any(|record| record.summary == "merge list runs"),
            "{:?}",
            app.operation_journal
        );
    }

    /// A bulleted list next to a numbered one stays two lists: they are
    /// visibly different lists, and merging them would retype the markers.
    #[test]
    fn lists_with_different_markers_are_not_merged() {
        let mut app = OpenDocApp::new_empty_document();
        app.document.blocks.clear();
        let bullets = new_list_id();
        let numbers = new_list_id();
        app.document
            .blocks
            .push(list_item(&bullets, ListKind::Bullet, "one"));
        app.document.blocks.push(Block::paragraph("between"));
        app.document
            .blocks
            .push(list_item(&numbers, ListKind::Ordered, "1"));
        let between = app.document.blocks[1].id.clone();

        app.delete_block(between.to_string())
            .expect("the paragraph exists");

        let ids = list_ids(&app);
        assert_eq!(ids, vec![bullets, numbers], "markers differ, runs must not");
    }

    /// Only runs *this* edit brought together are merged. Two lists that were
    /// already adjacent — an import that means them to be separate, say —
    /// must survive an unrelated edit elsewhere in the document untouched.
    #[test]
    fn already_adjacent_runs_are_left_alone_by_an_unrelated_edit() {
        let mut app = OpenDocApp::new_empty_document();
        app.document.blocks.clear();
        let first = new_list_id();
        let second = new_list_id();
        app.document
            .blocks
            .push(list_item(&first, ListKind::Ordered, "one"));
        app.document
            .blocks
            .push(list_item(&second, ListKind::Ordered, "1"));
        app.document.blocks.push(Block {
            id: StableId::new("block"),
            kind: BlockKind::Paragraph,
            content: Vec::new(),
            properties: BlockProperties::default(),
        });
        let elsewhere = app.document.blocks[2].id.clone();

        app.delete_block(elsewhere.to_string())
            .expect("the paragraph exists");

        assert_eq!(
            list_ids(&app),
            vec![first, second],
            "an unrelated deletion must not re-identify lists the user did not touch"
        );
    }
}

#[cfg(test)]
mod edit_history_tests {
    use crate::{AppApiError, OpenDocApp};
    use serde_json::json;

    fn app() -> OpenDocApp {
        let mut app = OpenDocApp::new_empty_document();
        app.dispatch_command("create_document", json!({ "title": "History" }))
            .expect("create");
        app
    }

    fn text(app: &OpenDocApp) -> String {
        app.document().visible_text()
    }

    /// The paragraphs of the document, in order, ignoring the empty one every
    /// document starts with.
    fn words(app: &OpenDocApp) -> Vec<&'static str> {
        let visible = app.document().visible_text();
        ["one", "two", "three"]
            .into_iter()
            .filter(|word| visible.contains(word))
            .collect()
    }

    fn opendoc_merge_inline_id(block: &opendoc_core::Block) -> String {
        match block.content.first().expect("a run") {
            opendoc_core::Inline::Text { id, .. } => id.to_string(),
            other => panic!("expected a text run, got {other:?}"),
        }
    }

    /// Redo is the inverse of the inverse, so it is ordinary history too: it
    /// appends rather than replaying a stored future.
    #[test]
    fn redo_is_the_inverse_of_the_inverse() {
        let mut app = app();
        app.dispatch_command("add_paragraph", json!({ "text": "alpha" }))
            .expect("paragraph");
        let after_edit = text(&app);
        let operations_after_edit = app.operation_envelopes.len();

        app.dispatch_command("undo_current_edit", json!({}))
            .expect("undo");
        assert!(!text(&app).contains("alpha"));
        assert!(
            app.operation_envelopes.len() > operations_after_edit,
            "the undo appends its inverse rather than rewinding the journal"
        );
        let after_undo = app.operation_envelopes.len();

        app.dispatch_command("redo_current_edit", json!({}))
            .expect("redo");
        assert_eq!(text(&app), after_edit, "redo has to restore the edit");
        assert!(
            app.operation_envelopes.len() > after_undo,
            "the redo appends too: it is the inverse of the inverse"
        );
    }

    /// Several steps, undone and redone in order.
    ///
    /// The point is that an undo of an older step reaches **exactly** its own
    /// step: not over the newer ones, and not over the operations a previous
    /// undo appended. The operation count per step is asserted alongside the
    /// text, and that is the assertion that bites — a step whose window has no
    /// upper bound re-applies and re-reverses the newer steps on the way past,
    /// which lands on the right document by three times the operations. Only
    /// the count can tell those apart.
    #[test]
    fn steps_unwind_and_rewind_one_at_a_time() {
        let mut app = app();
        for word in ["one", "two", "three"] {
            app.dispatch_command("add_paragraph", json!({ "text": word }))
                .expect("paragraph");
        }
        assert_eq!(words(&app), vec!["one", "two", "three"]);

        for expected in [
            vec!["one", "two"],
            vec!["one"],
            Vec::<&str>::new(),
            vec!["one"],
            vec!["one", "two"],
            vec!["one", "two", "three"],
        ]
        .into_iter()
        .enumerate()
        {
            let (index, expected) = expected;
            let command = if index < 3 {
                "undo_current_edit"
            } else {
                "redo_current_edit"
            };
            let before = app.operation_envelopes.len();
            app.dispatch_command(command, json!({}))
                .unwrap_or_else(|error| panic!("{command} {index}: {error:?}"));
            assert_eq!(
                app.operation_envelopes.len() - before,
                1,
                "{command} {index} must author exactly the one operation that \
                 reverses one `add_paragraph`, not the newer steps as well"
            );
            assert_eq!(words(&app), expected, "{command} {index}");
        }
    }

    /// Outside a session a step the vocabulary cannot invert still falls back
    /// to the whole-state snapshot, so single-user undo covers everything it
    /// covered before inverse operations existed.
    #[test]
    fn a_spreadsheet_step_still_undoes_by_snapshot_outside_a_session() {
        let mut app = app();
        app.dispatch_command(
            "set_spreadsheet_cell",
            json!({ "address": "A1", "value": "7" }),
        )
        .expect("spreadsheet edit");
        let journal_before = app.operation_envelopes.len();
        app.dispatch_command("undo_current_edit", json!({}))
            .expect("a snapshot restore is still available locally");
        assert!(
            app.operation_envelopes.len() <= journal_before,
            "a snapshot restore rewinds the journal; only inverse undo appends"
        );
    }

    /// Checkpoints share one copy of the blob bytes while no blob is added or
    /// removed, so a document with pictures in it does not pay for a deep copy
    /// of every image on every undoable keystroke.
    ///
    /// The assertion is pointer identity, not equality: equality would hold
    /// just as well if every checkpoint took its own copy, which is the thing
    /// being fixed.
    #[test]
    fn checkpoints_share_one_copy_of_the_blob_bytes() {
        let mut app = app();
        app.dispatch_command(
            "add_binary_blob",
            json!({
                "name": "picture.bin",
                "mediaType": "application/octet-stream",
                "bytes": [1, 2, 3, 4],
            }),
        )
        .expect("a blob");
        for word in ["one", "two", "three"] {
            app.dispatch_command("add_paragraph", json!({ "text": word }))
                .expect("paragraph");
        }
        let captured: Vec<_> = app
            .undo_stack
            .iter()
            .filter(|checkpoint| !checkpoint.blob_bytes.is_empty())
            .map(|checkpoint| std::sync::Arc::as_ptr(&checkpoint.blob_bytes))
            .collect();
        assert!(
            captured.len() > 1,
            "the fixture has to produce several checkpoints holding blob bytes"
        );
        assert!(
            captured.windows(2).all(|pair| pair[0] == pair[1]),
            "checkpoints taken while the blob set is unchanged must share one copy"
        );

        // A second blob changes the set, so the next checkpoint takes its own.
        app.dispatch_command(
            "add_binary_blob",
            json!({
                "name": "other.bin",
                "mediaType": "application/octet-stream",
                "bytes": [9, 9],
            }),
        )
        .expect("a second blob");
        app.dispatch_command("add_paragraph", json!({ "text": "after" }))
            .expect("paragraph");
        let newest =
            std::sync::Arc::as_ptr(&app.undo_stack.last().expect("a checkpoint").blob_bytes);
        assert_ne!(
            newest, captured[0],
            "a checkpoint taken after the blob set changed must not share the stale copy"
        );
    }

    #[test]
    fn nothing_to_undo_and_nothing_to_redo_are_conflicts() {
        let mut app = app();
        assert!(matches!(
            app.undo_current_edit().expect_err("empty stack"),
            AppApiError::Conflict(_)
        ));
        assert!(matches!(
            app.redo_current_edit().expect_err("empty stack"),
            AppApiError::Conflict(_)
        ));
    }

    /// Typing inside the coalescing window is one step, and one undo has to
    /// reverse all of it — the property the dispatcher's coalescing exists
    /// for, now carried by a multi-operation inverse instead of by one
    /// snapshot.
    #[test]
    fn one_coalesced_typing_gesture_is_one_undo_step() {
        let mut app = app();
        app.dispatch_command("add_paragraph", json!({ "text": "" }))
            .expect("a paragraph to type into");
        let last = app
            .source_document()
            .blocks
            .last()
            .expect("a block")
            .clone();
        let block = last.id.to_string();
        let inline = opendoc_merge_inline_id(&last);
        let steps_before = app.undo_stack.len();
        for (offset, letter) in ["x", "y", "z"].into_iter().enumerate() {
            let position = json!({
                "block_id": block,
                "inline_id": inline,
                "offset": offset,
            });
            app.dispatch_command(
                "apply_editor_input",
                json!({
                    "selection": { "anchor": position, "focus": position },
                    "input_type": "insertText",
                    "data": letter,
                }),
            )
            .expect("typing");
        }
        assert!(text(&app).contains("xyz"), "{:?}", text(&app));
        assert_eq!(
            app.undo_stack.len(),
            steps_before + 1,
            "continuous typing is one undo step"
        );
        app.dispatch_command("undo_current_edit", json!({}))
            .expect("undo");
        assert!(
            !text(&app).contains('x') && !text(&app).contains('z'),
            "one undo has to reverse the whole gesture: {:?}",
            text(&app)
        );
    }
}
