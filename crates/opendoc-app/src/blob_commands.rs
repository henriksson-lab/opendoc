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

    /// Draws the image `twips` wide, leaving the height to follow the aspect
    /// ratio unless it was set on its own. One command per property: the
    /// width's type *is* the command, so nothing has to parse a key out of a
    /// string to know what was meant.
    pub fn set_image_block_width(
        &mut self,
        block_id: impl AsRef<str>,
        twips: i32,
    ) -> Result<AppDocument, AppApiError> {
        let width = image_display_length(twips)?;
        self.update_image_layout(block_id.as_ref(), "image width", |layout| {
            layout.width = Some(width);
        })
    }

    /// Draws the image `twips` tall, leaving the width to follow.
    pub fn set_image_block_height(
        &mut self,
        block_id: impl AsRef<str>,
        twips: i32,
    ) -> Result<AppDocument, AppApiError> {
        let height = image_display_length(twips)?;
        self.update_image_layout(block_id.as_ref(), "image height", |layout| {
            layout.height = Some(height);
        })
    }

    /// Sets both axes at once.
    ///
    /// Not the same as calling the two single-axis commands in sequence: a
    /// corner drag is *one* gesture, so it has to be one undoable operation.
    /// Two would mean an undo left the picture at a shape the user never saw.
    pub fn set_image_block_size(
        &mut self,
        block_id: impl AsRef<str>,
        width_twips: i32,
        height_twips: i32,
    ) -> Result<AppDocument, AppApiError> {
        let width = image_display_length(width_twips)?;
        let height = image_display_length(height_twips)?;
        self.update_image_layout(block_id.as_ref(), "image size", |layout| {
            layout.width = Some(width);
            layout.height = Some(height);
        })
    }

    /// Returns the image to the size its bytes decode to.
    pub fn clear_image_block_size(
        &mut self,
        block_id: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        self.update_image_layout(block_id.as_ref(), "image size reset", |layout| {
            layout.width = None;
            layout.height = None;
        })
    }

    /// Places the image in the column: on its own line, or floated so the
    /// following blocks flow beside it.
    ///
    /// `"block"` is stored as *unstated* rather than as an explicit value:
    /// it is what an image does when the document says nothing, so writing it
    /// down would only make two byte-different documents that mean the same
    /// thing.
    pub fn set_image_block_placement(
        &mut self,
        block_id: impl AsRef<str>,
        placement: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let placement = parse_image_placement(placement.as_ref())?;
        let stored = match placement {
            opendoc_core::ImagePlacement::Block => None,
            other => Some(other),
        };
        self.update_image_layout(block_id.as_ref(), "image placement", |layout| {
            layout.placement = stored;
        })
    }

    /// Read-modify-write of one image block's geometry.
    ///
    /// The whole layout travels in the operation, which is what makes
    /// concurrent resizes converge on a shape somebody actually dragged
    /// rather than on a mix of two.
    fn update_image_layout(
        &mut self,
        block_id: &str,
        label: &'static str,
        edit: impl FnOnce(&mut opendoc_core::ImageLayout),
    ) -> Result<AppDocument, AppApiError> {
        let block_id = parse_id(block_id)?;
        let mut layout = image_block_layout(&self.document.blocks, &block_id)?;
        edit(&mut layout);
        Ok(self.apply(
            "update-image-layout",
            label,
            OperationKind::UpdateImageLayout { block_id, layout },
        ))
    }

    fn image_block_service(&self) -> ImageBlockService<'_> {
        ImageBlockService::new(&self.blobs)
    }

    fn blob_lifecycle_service(&mut self) -> BlobLifecycleService<'_> {
        BlobLifecycleService::new(&mut self.document, &mut self.blobs, &mut self.blob_bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opendoc_core::{Block, ImagePlacement, Length};

    /// A document whose second block is an image backed by a real blob.
    fn app_with_image() -> (OpenDocApp, String) {
        let mut app = OpenDocApp::new_sample();
        app.new_document("Images");
        app.document.blocks.clear();
        app.document.blocks.push(Block::paragraph("Body"));
        app.add_binary_blob("picture.png", "image/png", vec![1, 2, 3, 4])
            .unwrap();
        let hash = app.blobs[0].hash.clone();
        let after = app.document.blocks[0].id.to_string();
        app.insert_image_block_after(&after, &hash, "A picture")
            .unwrap();
        let image_id = app.document.blocks[1].id.to_string();
        (app, image_id)
    }

    fn layout(app: &OpenDocApp) -> opendoc_core::ImageLayout {
        match &app.document.blocks[1].kind {
            BlockKind::Image { layout, .. } => layout.clone(),
            other => panic!("expected an image block, got {other:?}"),
        }
    }

    #[test]
    fn an_inserted_image_states_no_size_and_renders_without_one() {
        let (app, _) = app_with_image();
        assert!(layout(&app).is_empty());
        let html = app.document().body_html;
        assert!(html.contains("<figure"), "{html}");
        // No size attribute at all: absent means intrinsic, and materialising
        // a default here would freeze a fact about the blob into the document.
        assert!(!html.contains("width:"), "{html}");
    }

    #[test]
    fn a_width_is_stored_projected_and_rendered_with_a_free_height() {
        let (mut app, image_id) = app_with_image();
        app.set_image_block_width(&image_id, 1440).unwrap();

        assert_eq!(layout(&app).width, Some(Length::from_twips(1440).unwrap()));
        assert_eq!(layout(&app).height, None);
        let projected = app.document();
        assert_eq!(projected.blocks[1].image_width_twips, Some(1440));
        assert_eq!(projected.blocks[1].image_height_twips, None);
        // 1440 twips is 72pt, and the unset axis has to stay free or a
        // one-axis resize would stretch the picture.
        assert!(
            projected.body_html.contains("width: 72pt; height: auto;"),
            "{}",
            projected.body_html
        );
    }

    /// A corner drag sets both axes, and the whole point of doing that with
    /// one command is that one undo puts the picture back where it was.
    /// Dispatched by name rather than called directly, because the undo
    /// checkpoint is pushed by the dispatcher from the command's policy flag.
    #[test]
    fn a_corner_resize_is_a_single_undoable_operation() {
        let (mut app, image_id) = app_with_image();
        let operations_before = app.document().operations.len();
        let checkpoints_before = app.undo_stack.len();
        app.dispatch_command(
            "set_image_block_size",
            serde_json::json!({ "blockId": image_id, "widthTwips": 2880, "heightTwips": 1440 }),
        )
        .expect("resize");

        assert_eq!(
            app.document().operations.len(),
            operations_before + 1,
            "a resize gesture has to journal exactly one operation"
        );
        assert_eq!(
            app.undo_stack.len(),
            checkpoints_before + 1,
            "a resize gesture has to be exactly one undo step"
        );
        assert_eq!(layout(&app).width, Some(Length::from_twips(2880).unwrap()));
        assert_eq!(layout(&app).height, Some(Length::from_twips(1440).unwrap()));
        assert_eq!(
            app.document().operations.last().unwrap().kind,
            "update-image-layout"
        );

        app.undo_current_edit().expect("undo");
        assert!(
            layout(&app).is_empty(),
            "one undo has to put back the size the user saw before the drag"
        );
    }

    #[test]
    fn clearing_the_size_returns_the_image_to_its_intrinsic_one() {
        let (mut app, image_id) = app_with_image();
        app.set_image_block_size(&image_id, 2880, 1440).unwrap();
        app.clear_image_block_size(&image_id).unwrap();

        assert!(layout(&app).is_empty());
        assert!(!app.document().body_html.contains("width:"));
    }

    #[test]
    fn a_size_that_is_not_positive_is_refused_rather_than_clamped() {
        let (mut app, image_id) = app_with_image();
        assert!(app.set_image_block_width(&image_id, 0).is_err());
        assert!(app.set_image_block_height(&image_id, -20).is_err());
        assert!(app.set_image_block_size(&image_id, 1440, 0).is_err());
        assert!(layout(&app).is_empty(), "a refused command must not write");
    }

    #[test]
    fn sizing_something_that_is_not_an_image_is_a_conflict() {
        let (mut app, _) = app_with_image();
        let paragraph = app.document.blocks[0].id.to_string();
        let error = app.set_image_block_width(&paragraph, 1440).unwrap_err();
        assert!(
            matches!(error, AppApiError::Conflict(ref message) if message.contains("not an image")),
            "{error:?}"
        );
    }

    #[test]
    fn placement_is_stored_projected_and_rendered_and_the_default_is_not() {
        let (mut app, image_id) = app_with_image();
        assert!(
            app.document()
                .body_html
                .contains("data-placement=\"block\""),
            "the projection always states the placement it drew"
        );

        app.set_image_block_placement(&image_id, "wrap-end")
            .unwrap();
        assert_eq!(layout(&app).placement, Some(ImagePlacement::WrapEnd));
        assert_eq!(
            app.document().blocks[1].image_placement.as_deref(),
            Some("wrap-end")
        );
        assert!(app
            .document()
            .body_html
            .contains("data-placement=\"wrap-end\""));

        app.set_image_block_placement(&image_id, "block").unwrap();
        assert_eq!(
            layout(&app).placement,
            None,
            "the default placement is stored as unstated, not written down"
        );
        assert!(layout(&app).is_empty());

        assert!(app
            .set_image_block_placement(&image_id, "behind-text")
            .is_err());
    }

    #[test]
    fn image_geometry_survives_the_projection_that_is_persisted() {
        let (mut app, image_id) = app_with_image();
        app.set_image_block_size(&image_id, 2880, 1620).unwrap();
        app.set_image_block_placement(&image_id, "wrap-start")
            .unwrap();

        // `AppDocument` is what a repository stores and reloads, so a field it
        // cannot carry is a field that is silently lost on save.
        let restored = app.document().to_core().expect("projection parses back");
        assert_eq!(restored.blocks[1].kind, app.document.blocks[1].kind);
    }
}
