use crate::{
    append_spreadsheet_formula_warnings, verify_app_typed_signature, AppArchiveTombstone,
    AppBlobRef, AppDocument, AppOperationRecord, AppRecentDocument, AppRenderService, AppSignature,
    AppWarning,
};
use opendoc_core::Document;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Builds UI-facing projections from source state and runtime sidecars.
pub(crate) struct AppProjectionService<'a> {
    document: &'a Document,
    workbook: &'a crate::AppSpreadsheetWorkbook,
    blobs: &'a [AppBlobRef],
    blob_bytes: &'a BTreeMap<String, Vec<u8>>,
    blob_signatures: &'a BTreeMap<String, Vec<opendoc_format::SignatureRecord>>,
    blob_tombstones: &'a BTreeMap<String, AppArchiveTombstone>,
    signatures: &'a [opendoc_format::SignatureRecord],
    operation_journal: &'a [AppOperationRecord],
    is_open: bool,
    saved_projection: &'a Option<AppDocument>,
    repository_root: &'a Option<PathBuf>,
    repository_backend: &'a Option<String>,
    repository_namespace: &'a Option<String>,
    recent_documents: &'a [AppRecentDocument],
    last_manifest: &'a Option<String>,
    saved_operation_count: usize,
    saved_signature_count: usize,
    defer_spreadsheet_evaluation: bool,
}

impl<'a> AppProjectionService<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        document: &'a Document,
        workbook: &'a crate::AppSpreadsheetWorkbook,
        blobs: &'a [AppBlobRef],
        blob_bytes: &'a BTreeMap<String, Vec<u8>>,
        blob_signatures: &'a BTreeMap<String, Vec<opendoc_format::SignatureRecord>>,
        blob_tombstones: &'a BTreeMap<String, AppArchiveTombstone>,
        signatures: &'a [opendoc_format::SignatureRecord],
        operation_journal: &'a [AppOperationRecord],
        is_open: bool,
        saved_projection: &'a Option<AppDocument>,
        repository_root: &'a Option<PathBuf>,
        repository_backend: &'a Option<String>,
        repository_namespace: &'a Option<String>,
        recent_documents: &'a [AppRecentDocument],
        last_manifest: &'a Option<String>,
        saved_operation_count: usize,
        saved_signature_count: usize,
        defer_spreadsheet_evaluation: bool,
    ) -> Self {
        Self {
            document,
            workbook,
            blobs,
            blob_bytes,
            blob_signatures,
            blob_tombstones,
            signatures,
            operation_journal,
            is_open,
            saved_projection,
            repository_root,
            repository_backend,
            repository_namespace,
            recent_documents,
            last_manifest,
            saved_operation_count,
            saved_signature_count,
            defer_spreadsheet_evaluation,
        }
    }

    pub(crate) fn document(&self, operation_limit: usize) -> AppDocument {
        let mut document = self
            .saved_projection
            .clone()
            .unwrap_or_else(|| AppDocument::from_core(self.document));
        document.repository_root = self
            .repository_root
            .as_ref()
            .map(|path| path.to_string_lossy().to_string());
        document.repository_backend = self.repository_backend.clone();
        document.repository_namespace = self.repository_namespace.clone();
        document.recent_documents = self.recent_documents.to_vec();
        document.last_manifest = self.last_manifest.clone();
        document.has_unsaved_changes = self.is_open && self.has_pending_save_changes();
        if document.operations.len() > operation_limit {
            let skip = document.operations.len() - operation_limit;
            document.operations.drain(..skip);
        }
        document.is_open = self.is_open;
        if self.is_open {
            let renderer =
                AppRenderService::new(self.document, self.workbook, self.blobs, self.blob_bytes);
            document.body_html = renderer.render_document_html();
            document.footnotes_html = renderer.render_footnotes_html();
        }
        document.signature_state = self.signature_state_label().to_string();
        document.signatures = self
            .signatures
            .iter()
            .map(AppSignature::from_record)
            .collect();
        document.signature = document.signatures.first().cloned();
        document.workbook = if self.defer_spreadsheet_evaluation {
            self.workbook.clone()
        } else {
            self.workbook.evaluated()
        };
        append_spreadsheet_formula_warnings(&mut document);
        document.blobs = self.projected_blobs();
        for blob in &document.blobs {
            for typed in &blob.typed_signatures {
                if typed.signature_state == "broken" {
                    let message = format!(
                        "blob {} has a broken {} typed signature",
                        blob.name, typed.profile
                    );
                    if !document.warnings.iter().any(|warning| {
                        warning.code == "broken-typed-blob-signature" && warning.message == message
                    }) {
                        document.warnings.push(AppWarning {
                            code: "broken-typed-blob-signature".to_string(),
                            message,
                        });
                    }
                }
            }
        }
        document.operation_count = self.operation_journal.len();
        document.operations = self.operation_journal.to_vec();
        document
    }

    pub(crate) fn projected_blobs(&self) -> Vec<AppBlobRef> {
        self.blobs
            .iter()
            .map(|blob| {
                let mut blob = blob.clone();
                blob.signatures = self
                    .blob_signatures
                    .get(&blob.hash)
                    .into_iter()
                    .flatten()
                    .map(AppSignature::from_record)
                    .collect();
                blob.signature_state = if blob.signatures.is_empty() {
                    "unsigned".to_string()
                } else {
                    "signed".to_string()
                };
                blob.archive_tombstone = self.blob_tombstones.get(&blob.hash).cloned();
                for typed in &mut blob.typed_signatures {
                    typed.signature_state = if let Some(bytes) = self.blob_bytes.get(&blob.hash) {
                        verify_app_typed_signature(typed, &blob.hash, bytes)
                    } else {
                        "untrusted".to_string()
                    };
                }
                blob
            })
            .collect()
    }

    fn signature_count(&self) -> usize {
        self.signatures.len()
            + self
                .blob_signatures
                .values()
                .map(|signatures| signatures.len())
                .sum::<usize>()
    }

    fn has_pending_save_changes(&self) -> bool {
        self.saved_operation_count != self.operation_journal.len()
            || self.saved_signature_count != self.signature_count()
    }

    fn signature_state_label(&self) -> &'static str {
        if !self.signatures.is_empty() {
            "signed"
        } else {
            "unsigned"
        }
    }
}
