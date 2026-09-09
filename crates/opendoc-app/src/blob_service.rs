use crate::{document_tree::blocks_reference_blob, AppApiError, AppBlobOperation, AppBlobRef};
use opendoc_core::{digest_bytes, Document, HashRef, ModelWarning, StableId};
use std::collections::BTreeMap;

pub(crate) enum BlobLifecycleJournal {
    AppOnly,
    Blob(AppBlobOperation),
}

pub(crate) struct BlobLifecycleService<'a> {
    document: &'a mut Document,
    blobs: &'a mut Vec<AppBlobRef>,
    blob_bytes: &'a mut BTreeMap<String, Vec<u8>>,
}

impl<'a> BlobLifecycleService<'a> {
    pub(crate) fn new(
        document: &'a mut Document,
        blobs: &'a mut Vec<AppBlobRef>,
        blob_bytes: &'a mut BTreeMap<String, Vec<u8>>,
    ) -> Self {
        Self {
            document,
            blobs,
            blob_bytes,
        }
    }

    pub(crate) fn add_binary_blob(
        &mut self,
        name: String,
        media_type: String,
        bytes: Vec<u8>,
    ) -> Result<BlobLifecycleJournal, AppApiError> {
        let hash =
            digest_bytes("sha256", &bytes).map_err(|err| AppApiError::Model(err.to_string()))?;
        let hash_text = hash.to_string();
        let journal = if self.blobs.iter().any(|blob| blob.hash == hash_text) {
            BlobLifecycleJournal::AppOnly
        } else {
            let blob = AppBlobRef {
                id: StableId::new("blob").to_string(),
                name: clean_blob_name(name),
                media_type: clean_blob_media_type(media_type),
                hash: hash_text.clone(),
                size: bytes.len() as u64,
                available: true,
                signature_state: "unsigned".to_string(),
                signatures: Vec::new(),
                typed_signatures: Vec::new(),
                archive_tombstone: None,
            };
            let journal = BlobLifecycleJournal::Blob(AppBlobOperation::Add {
                id: blob.id.clone(),
                name: blob.name.clone(),
                media_type: blob.media_type.clone(),
                hash: blob.hash.clone(),
                size: blob.size,
            });
            self.blobs.push(blob);
            journal
        };
        self.blob_bytes.insert(hash_text, bytes);
        Ok(journal)
    }

    pub(crate) fn simulate_shallow_clone(&mut self) {
        let mut missing_count = 0usize;
        for blob in &mut *self.blobs {
            if blob.available {
                missing_count += 1;
            }
            blob.available = false;
            if !self.document.warnings.iter().any(|warning| {
                warning.code == "missing-blob" && warning.message.contains(&blob.hash)
            }) {
                self.document.warnings.push(ModelWarning {
                    code: "missing-blob".to_string(),
                    message: format!(
                        "blob {} ({}) is missing and will render as a placeholder",
                        blob.name, blob.hash
                    ),
                });
            }
        }
        self.blob_bytes.clear();
        if missing_count == 0 && self.blobs.is_empty() {
            self.document.warnings.push(ModelWarning {
                code: "shallow-clone-empty".to_string(),
                message: "shallow clone simulation found no current blob references".to_string(),
            });
        }
    }

    pub(crate) fn update_binary_blob_metadata(
        &mut self,
        blob_hash: &str,
        name: String,
        media_type: String,
    ) -> Result<AppBlobOperation, AppApiError> {
        let hash = parse_blob_hash(blob_hash)?;
        let Some(blob) = self.blobs.iter_mut().find(|blob| blob.hash == hash) else {
            return Err(AppApiError::NotFound("blob was not found".to_string()));
        };
        blob.name = clean_blob_name(name);
        blob.media_type = clean_blob_media_type(media_type);
        Ok(AppBlobOperation::UpdateMetadata {
            hash,
            name: blob.name.clone(),
            media_type: blob.media_type.clone(),
        })
    }

    pub(crate) fn delete_binary_blob(
        &mut self,
        blob_hash: &str,
    ) -> Result<AppBlobOperation, AppApiError> {
        let hash = parse_blob_hash(blob_hash)?;
        let Some(index) = self.blobs.iter().position(|blob| blob.hash == hash) else {
            return Err(AppApiError::NotFound("blob was not found".to_string()));
        };
        if blocks_reference_blob(&self.document.blocks, &hash) {
            return Err(AppApiError::Conflict(
                "blob is still referenced by an image block".to_string(),
            ));
        }
        let blob = self.blobs.remove(index);
        Ok(AppBlobOperation::Delete {
            hash,
            typed_signatures: blob.typed_signatures,
        })
    }

    pub(crate) fn restore_binary_blob(
        &mut self,
        blob_hash: &str,
        deleted_blob: Option<AppBlobRef>,
    ) -> Result<AppBlobOperation, AppApiError> {
        let hash = parse_blob_hash(blob_hash)?;
        if self.blobs.iter().any(|blob| blob.hash == hash) {
            return Err(AppApiError::Conflict(
                "blob is already in current state".to_string(),
            ));
        }
        let Some(blob) = deleted_blob else {
            return Err(AppApiError::NotFound(
                "deleted blob was not found in audit history".to_string(),
            ));
        };
        self.blobs.push(blob.clone());
        Ok(AppBlobOperation::Restore {
            id: blob.id,
            name: blob.name,
            media_type: blob.media_type,
            hash,
            size: blob.size,
            typed_signatures: blob.typed_signatures,
        })
    }
}

fn clean_blob_name(name: String) -> String {
    if name.trim().is_empty() {
        "unnamed blob".to_string()
    } else {
        name
    }
}

fn clean_blob_media_type(media_type: String) -> String {
    if media_type.trim().is_empty() {
        "application/octet-stream".to_string()
    } else {
        media_type
    }
}

fn parse_blob_hash(blob_hash: &str) -> Result<String, AppApiError> {
    Ok(HashRef::parse(blob_hash.trim())
        .map_err(|err| AppApiError::Model(err.to_string()))?
        .to_string())
}
