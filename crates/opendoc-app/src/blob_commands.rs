use super::*;

impl OpenDocApp {
    pub fn add_binary_blob(
        &mut self,
        name: impl Into<String>,
        media_type: impl Into<String>,
        bytes: Vec<u8>,
    ) -> Result<AppDocument, AppApiError> {
        self.invalidate_source_state();
        match self.blob_lifecycle_service().add_binary_blob(
            name.into(),
            media_type.into(),
            bytes,
        )? {
            BlobLifecycleJournal::Blob(blob) => {
                self.push_blob_operation("add-binary-blob", "add binary blob", blob)
            }
            BlobLifecycleJournal::AppOnly => {
                self.push_app_operation("add-binary-blob", "add binary blob");
            }
        }
        Ok(self.document())
    }

    pub fn simulate_shallow_clone(&mut self) -> AppDocument {
        self.blob_lifecycle_service().simulate_shallow_clone();
        self.invalidate_projection();
        self.document()
    }

    pub fn update_binary_blob_metadata(
        &mut self,
        blob_hash: impl AsRef<str>,
        name: impl Into<String>,
        media_type: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        self.invalidate_source_state();
        let blob = self.blob_lifecycle_service().update_binary_blob_metadata(
            blob_hash.as_ref(),
            name.into(),
            media_type.into(),
        )?;
        self.push_blob_operation(
            "update-binary-blob-metadata",
            "update attachment metadata",
            blob,
        );
        Ok(self.document())
    }

    pub fn delete_binary_blob(
        &mut self,
        blob_hash: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        self.invalidate_source_state();
        let blob = self
            .blob_lifecycle_service()
            .delete_binary_blob(blob_hash.as_ref())?;
        self.push_blob_operation(
            "delete-binary-blob",
            "delete binary blob from current state",
            blob,
        );
        Ok(self.document())
    }

    pub fn restore_binary_blob(
        &mut self,
        blob_hash: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        self.invalidate_source_state();
        let hash = opendoc_core::HashRef::parse(blob_hash.as_ref().trim())
            .map_err(|err| AppApiError::Model(err.to_string()))?
            .to_string();
        let deleted_blob = self.retained_deleted_blob_refs().remove(&hash);
        let blob = self
            .blob_lifecycle_service()
            .restore_binary_blob(&hash, deleted_blob)?;
        self.push_blob_operation(
            "restore-binary-blob",
            "restore binary blob to current state",
            blob,
        );
        Ok(self.document())
    }

    pub fn record_blob_archive_tombstone(
        &mut self,
        blob_hash: impl AsRef<str>,
        archive_locator: impl Into<String>,
        restore_hint: impl Into<String>,
        signer: impl Into<String>,
        signature: Vec<u8>,
    ) -> Result<AppDocument, AppApiError> {
        let hash = opendoc_core::HashRef::parse(blob_hash.as_ref().trim())
            .map_err(|err| AppApiError::Model(err.to_string()))?;
        let hash_text = hash.to_string();
        if !self.blobs.iter().any(|blob| blob.hash == hash_text)
            && !self.retained_deleted_blob_refs().contains_key(&hash_text)
        {
            return Err(AppApiError::NotFound(
                "blob was not found in current state or audit history".to_string(),
            ));
        }
        let archive_locator = archive_locator.into().trim().to_string();
        let restore_hint = restore_hint.into().trim().to_string();
        let signer = signer.into().trim().to_string();
        let record = opendoc_format::TombstoneRecord {
            object: hash,
            archive_locator,
            restore_hint,
            created_at_ms: now_ms(),
            signer,
            signature,
        };
        record
            .validate()
            .map_err(|err| AppApiError::Format(err.to_string()))?;
        let root = self.repository_root.clone().ok_or_else(|| {
            AppApiError::Conflict(
                "recording a blob archive tombstone needs an opened or saved repository"
                    .to_string(),
            )
        })?;
        let target = AppRepositoryTarget::from_backend(
            self.repository_backend.as_deref(),
            self.repository_namespace.clone(),
            "archive tombstone recording",
        )?;
        let repo = target.repository(&root)?;
        repo.write_tombstone(&record)
            .map_err(|err| AppApiError::Store(err.to_string()))?;
        let archive_tombstone = AppArchiveTombstone::from_record(&record);
        self.blob_tombstones
            .insert(hash_text.clone(), archive_tombstone.clone());
        self.blob_tombstone_records
            .insert(hash_text.clone(), record);
        self.push_blob_operation(
            "record-blob-archive-tombstone",
            "record blob archive tombstone",
            AppBlobOperation::ArchiveTombstone {
                hash: hash_text,
                archive_tombstone,
            },
        );
        Ok(self.document())
    }

    pub fn add_image_block(
        &mut self,
        blob_hash: impl AsRef<str>,
        alt_text: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let block = self
            .image_block_service()
            .image_block_for_existing_blob(blob_hash, alt_text)?;
        Ok(self.apply(
            "insert-block",
            "image block",
            OperationKind::InsertBlock {
                after: self.document.blocks.last().map(|block| block.id.clone()),
                block,
            },
        ))
    }

    pub fn insert_image_block_after(
        &mut self,
        after_block_id: impl AsRef<str>,
        blob_hash: impl AsRef<str>,
        alt_text: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let after = parse_id(after_block_id.as_ref())?;
        if !self.document.blocks.iter().any(|block| block.id == after) {
            return Err(AppApiError::NotFound(format!(
                "top-level block {after} was not found"
            )));
        }
        let block = self
            .image_block_service()
            .image_block_for_existing_blob(blob_hash, alt_text)?;
        Ok(self.apply(
            "insert-block",
            "image block after block",
            OperationKind::InsertBlock {
                after: Some(after),
                block,
            },
        ))
    }

    fn image_block_service(&self) -> ImageBlockService<'_> {
        ImageBlockService::new(&self.blobs)
    }

    fn blob_lifecycle_service(&mut self) -> BlobLifecycleService<'_> {
        BlobLifecycleService::new(&mut self.document, &mut self.blobs, &mut self.blob_bytes)
    }
}
