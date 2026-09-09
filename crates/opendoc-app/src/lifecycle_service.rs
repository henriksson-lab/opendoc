use crate::{
    AppArchiveTombstone, AppBlobRef, AppOperationEnvelope, AppOperationRecord, OpenDocApp,
};
use opendoc_import::ImportedBlob;
use std::collections::BTreeMap;
use std::path::PathBuf;

pub(crate) struct AppLifecycleService<'a> {
    blobs: &'a mut Vec<AppBlobRef>,
    blob_bytes: &'a mut BTreeMap<String, Vec<u8>>,
    blob_signatures: &'a mut BTreeMap<String, Vec<opendoc_format::SignatureRecord>>,
    blob_tombstones: &'a mut BTreeMap<String, AppArchiveTombstone>,
    blob_tombstone_records: &'a mut BTreeMap<String, opendoc_format::TombstoneRecord>,
    operation_journal: &'a mut Vec<AppOperationRecord>,
    operation_envelopes: &'a mut Vec<AppOperationEnvelope>,
    undo_stack: &'a mut Vec<crate::state::AppUndoCheckpoint>,
    redo_stack: &'a mut Vec<crate::state::AppUndoCheckpoint>,
    repository_root: &'a mut Option<PathBuf>,
    repository_backend: &'a mut Option<String>,
    repository_namespace: &'a mut Option<String>,
    last_manifest: &'a mut Option<String>,
    saved_operation_count: &'a mut usize,
    saved_signature_count: &'a mut usize,
    next_seq: &'a mut u64,
}

impl<'a> AppLifecycleService<'a> {
    pub(crate) fn new(app: &'a mut OpenDocApp) -> Self {
        Self {
            blobs: &mut app.blobs,
            blob_bytes: &mut app.blob_bytes,
            blob_signatures: &mut app.blob_signatures,
            blob_tombstones: &mut app.blob_tombstones,
            blob_tombstone_records: &mut app.blob_tombstone_records,
            operation_journal: &mut app.operation_journal,
            operation_envelopes: &mut app.operation_envelopes,
            undo_stack: &mut app.undo_stack,
            redo_stack: &mut app.redo_stack,
            repository_root: &mut app.repository_root,
            repository_backend: &mut app.repository_backend,
            repository_namespace: &mut app.repository_namespace,
            last_manifest: &mut app.last_manifest,
            saved_operation_count: &mut app.saved_operation_count,
            saved_signature_count: &mut app.saved_signature_count,
            next_seq: &mut app.next_seq,
        }
    }

    pub(crate) fn clear_blob_state(&mut self) {
        self.blobs.clear();
        self.blob_bytes.clear();
        self.blob_signatures.clear();
        self.blob_tombstones.clear();
        self.blob_tombstone_records.clear();
    }

    pub(crate) fn clear_edit_history(&mut self) {
        self.operation_journal.clear();
        self.operation_envelopes.clear();
        self.undo_stack.clear();
        self.redo_stack.clear();
        *self.next_seq = 1;
    }

    pub(crate) fn clear_repository_binding(&mut self) {
        *self.repository_root = None;
        *self.repository_backend = None;
        *self.repository_namespace = None;
        *self.last_manifest = None;
        *self.saved_operation_count = 0;
        *self.saved_signature_count = 0;
    }

    pub(crate) fn restore_imported_blobs(&mut self, blobs: Vec<ImportedBlob>) {
        let mut present = self
            .blobs
            .iter()
            .map(|blob| blob.hash.clone())
            .collect::<std::collections::BTreeSet<_>>();
        for blob in blobs {
            let hash = match opendoc_core::HashRef::parse(&blob.hash) {
                Ok(hash) => hash.to_string(),
                Err(_) => continue,
            };
            self.blob_bytes.insert(hash.clone(), blob.bytes);
            if present.contains(&hash) {
                continue;
            }
            self.blobs.push(AppBlobRef {
                id: opendoc_core::StableId::new("blob").to_string(),
                name: if blob.name.trim().is_empty() {
                    "imported blob".to_string()
                } else {
                    blob.name
                },
                media_type: if blob.media_type.trim().is_empty() {
                    "application/octet-stream".to_string()
                } else {
                    blob.media_type
                },
                hash: hash.clone(),
                size: self
                    .blob_bytes
                    .get(&hash)
                    .map(|bytes| bytes.len() as u64)
                    .unwrap_or(0),
                available: true,
                signature_state: "unsigned".to_string(),
                signatures: Vec::new(),
                typed_signatures: Vec::new(),
                archive_tombstone: None,
            });
            present.insert(hash);
        }
    }
}

impl OpenDocApp {
    pub(crate) fn clear_blob_state(&mut self) {
        self.lifecycle_service().clear_blob_state();
    }

    pub(crate) fn clear_edit_history(&mut self) {
        self.lifecycle_service().clear_edit_history();
    }

    pub(crate) fn clear_repository_binding(&mut self) {
        self.lifecycle_service().clear_repository_binding();
    }

    pub(crate) fn restore_imported_blobs(&mut self, blobs: Vec<ImportedBlob>) {
        self.lifecycle_service().restore_imported_blobs(blobs);
    }

    fn lifecycle_service(&mut self) -> AppLifecycleService<'_> {
        AppLifecycleService::new(self)
    }
}
