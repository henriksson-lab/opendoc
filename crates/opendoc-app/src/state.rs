use super::*;

/// Number of most recent operation records included in a document projection.
const PROJECTED_OPERATION_LIMIT: usize = 200;
#[derive(Clone, Debug)]
pub struct OpenDocApp {
    pub(crate) actor_id: String,
    pub(crate) document: Document,
    pub(crate) workbook: AppSpreadsheetWorkbook,
    pub(crate) blobs: Vec<AppBlobRef>,
    pub(crate) blob_bytes: BTreeMap<String, Vec<u8>>,
    pub(crate) blob_signatures: BTreeMap<String, Vec<opendoc_format::SignatureRecord>>,
    pub(crate) blob_tombstones: BTreeMap<String, AppArchiveTombstone>,
    pub(crate) blob_tombstone_records: BTreeMap<String, opendoc_format::TombstoneRecord>,
    pub(crate) signatures: Vec<opendoc_format::SignatureRecord>,
    pub(crate) operation_journal: Vec<AppOperationRecord>,
    pub(crate) operation_envelopes: Vec<AppOperationEnvelope>,
    pub(crate) undo_stack: Vec<AppUndoCheckpoint>,
    pub(crate) redo_stack: Vec<AppUndoCheckpoint>,
    pub(crate) is_open: bool,
    pub(crate) saved_projection: Option<AppDocument>,
    pub(crate) repository_root: Option<PathBuf>,
    pub(crate) repository_backend: Option<String>,
    pub(crate) repository_namespace: Option<String>,
    pub(crate) recent_documents: Vec<AppRecentDocument>,
    pub(crate) last_manifest: Option<String>,
    pub(crate) saved_operation_count: usize,
    pub(crate) saved_signature_count: usize,
    pub(crate) defer_spreadsheet_evaluation: bool,
    pub(crate) next_seq: u64,
    /// Key and timestamp of the last coalescable editor gesture so that
    /// continuous typing forms one undo step.
    pub(crate) undo_coalesce: Option<(String, u64)>,
    /// Durable shadow of the operation journal, plus whatever an unclean
    /// prior session left behind. Empty until a runtime installs a store.
    pub(crate) recovery: RecoveryJournal,
}

/// Maximum number of undo checkpoints retained.
pub(crate) const UNDO_STACK_LIMIT: usize = 200;
/// Typing gestures closer together than this (ms) share one undo step.
pub(crate) const UNDO_COALESCE_WINDOW_MS: u64 = 1_000;

#[derive(Clone, Debug)]
pub(crate) struct AppUndoCheckpoint {
    pub(crate) document: Document,
    pub(crate) workbook: AppSpreadsheetWorkbook,
    pub(crate) blobs: Vec<AppBlobRef>,
    pub(crate) blob_bytes: BTreeMap<String, Vec<u8>>,
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
    pub(crate) next_seq: u64,
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
            blob_signatures: BTreeMap::new(),
            blob_tombstones: BTreeMap::new(),
            blob_tombstone_records: BTreeMap::new(),
            signatures: Vec::new(),
            operation_journal: Vec::new(),
            operation_envelopes: Vec::new(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            is_open: true,
            saved_projection: None,
            repository_root: None,
            repository_backend: None,
            repository_namespace: None,
            recent_documents: Vec::new(),
            last_manifest: None,
            saved_operation_count: 0,
            saved_signature_count: 0,
            defer_spreadsheet_evaluation: false,
            next_seq: 1,
            undo_coalesce: None,
            recovery: RecoveryJournal::default(),
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
            blob_signatures: BTreeMap::new(),
            blob_tombstones: BTreeMap::new(),
            blob_tombstone_records: BTreeMap::new(),
            signatures: Vec::new(),
            operation_journal: Vec::new(),
            operation_envelopes: Vec::new(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            is_open: true,
            saved_projection: None,
            repository_root: None,
            repository_backend: None,
            repository_namespace: None,
            recent_documents: Vec::new(),
            last_manifest: None,
            saved_operation_count: 0,
            saved_signature_count: 0,
            defer_spreadsheet_evaluation: false,
            next_seq: 1,
            undo_coalesce: None,
            recovery: RecoveryJournal::default(),
        };
        app.document
            .blocks
            .push(Block::paragraph("OpenDoc editing surface"));
        app.add_heading("Schema coverage", 2)
            .expect("sample heading level is valid");
        app.add_paragraph(
            "This prototype renders the current docs-like model through a TypeScript UI.",
        );
        app.add_link("Project note", "https://example.invalid/opendoc")
            .expect("sample link is valid");
        app.add_equation_inline("E=mc^2")
            .expect("sample inline equation is valid");
        app.add_table();
        app.add_sample_citation();
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
        let Some(previous) = self.undo_stack.pop() else {
            return Err(AppApiError::Conflict("nothing to undo".to_string()));
        };
        let current = self.checkpoint();
        self.restore_checkpoint(previous);
        self.invalidate_source_state();
        self.redo_stack.push(current);
        self.push_app_operation("undo", "undo current edit");
        Ok(self.document())
    }

    pub fn redo_current_edit(&mut self) -> Result<AppDocument, AppApiError> {
        let Some(next) = self.redo_stack.pop() else {
            return Err(AppApiError::Conflict("nothing to redo".to_string()));
        };
        let current = self.checkpoint();
        self.restore_checkpoint(next);
        self.invalidate_source_state();
        self.undo_stack.push(current);
        self.push_app_operation("redo", "redo current edit");
        Ok(self.document())
    }

    pub(crate) fn checkpoint(&self) -> AppUndoCheckpoint {
        AppUndoCheckpoint {
            document: self.document.clone(),
            workbook: self.workbook.clone(),
            blobs: self.blobs.clone(),
            blob_bytes: self.blob_bytes.clone(),
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
            next_seq: self.next_seq,
        }
    }

    fn restore_checkpoint(&mut self, checkpoint: AppUndoCheckpoint) {
        self.document = checkpoint.document;
        self.workbook = checkpoint.workbook;
        self.blobs = checkpoint.blobs;
        self.blob_bytes = checkpoint.blob_bytes;
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
        self.next_seq = checkpoint.next_seq;
    }

    pub(crate) fn apply(
        &mut self,
        operation_kind: &str,
        summary: &str,
        kind: OperationKind,
    ) -> AppDocument {
        self.invalidate_source_state();
        let adjacent_runs_before = adjacent_list_run_pairs(&self.document.blocks);
        self.document_operation_service()
            .apply(operation_kind, summary, kind);
        self.merge_newly_adjacent_list_runs(&adjacent_runs_before);
        self.document()
    }

    pub(crate) fn apply_batch(
        &mut self,
        operations: Vec<(&str, &str, OperationKind)>,
    ) -> AppDocument {
        self.invalidate_source_state();
        let adjacent_runs_before = adjacent_list_run_pairs(&self.document.blocks);
        self.document_operation_service().apply_batch(operations);
        self.merge_newly_adjacent_list_runs(&adjacent_runs_before);
        self.document()
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
    ) {
        let newly_adjacent = adjacent_list_run_pairs(&self.document.blocks)
            .difference(adjacent_runs_before)
            .cloned()
            .collect::<BTreeSet<_>>();
        if newly_adjacent.is_empty() {
            return;
        }
        let operations = list_run_merge_operations(&self.document.blocks, &newly_adjacent);
        if operations.is_empty() {
            return;
        }
        self.document_operation_service().apply_batch(operations);
    }

    fn document_operation_service(&mut self) -> DocumentOperationService<'_> {
        DocumentOperationService::new(
            &self.actor_id,
            &mut self.document,
            &mut self.operation_journal,
            &mut self.operation_envelopes,
            &mut self.next_seq,
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
            self.saved_operation_count,
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
