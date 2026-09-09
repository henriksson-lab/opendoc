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

impl OpenDocApp {
    pub fn new_sample() -> Self {
        let mut app = Self {
            actor_id: StableId::new("actor").to_string(),
            document: Document::new("Untitled document"),
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
            "Untitled document".to_string()
        } else {
            title
        };
        self.document = Document::new(title);
        self.document.blocks.push(Block::paragraph(""));
        self.workbook = AppSpreadsheetWorkbook::sample();
        self.clear_blob_state();
        self.invalidate_source_state();
        self.clear_edit_history();
        self.is_open = true;
        self.clear_repository_binding();
        self.document()
    }

    pub fn document(&self) -> AppDocument {
        self.projection_service()
            .document(PROJECTED_OPERATION_LIMIT)
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
        self.workbook = AppSpreadsheetWorkbook::sample();
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
        self.document_operation_service()
            .apply(operation_kind, summary, kind);
        self.document()
    }

    pub(crate) fn apply_batch(
        &mut self,
        operations: Vec<(&str, &str, OperationKind)>,
    ) -> AppDocument {
        self.invalidate_source_state();
        self.document_operation_service().apply_batch(operations);
        self.document()
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
