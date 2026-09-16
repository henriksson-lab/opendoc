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
        self.apply(
            "insert-block",
            "image block",
            OperationKind::InsertBlock {
                position: InsertPosition::after_or_last(
                    self.document.blocks.last().map(|block| block.id.clone()),
                ),
                block,
            },
        )
    }

    pub fn insert_image_block_after(
        &mut self,
        after_block_id: impl AsRef<str>,
        blob_hash: impl AsRef<str>,
        alt_text: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let after = parse_id(after_block_id.as_ref())?;
        if find_block_in_blocks(&self.document.blocks, &after).is_none() {
            return Err(AppApiError::NotFound(format!(
                "block {after} was not found"
            )));
        }
        let block = self
            .image_block_service()
            .image_block_for_existing_blob(blob_hash, alt_text)?;
        self.apply(
            "insert-block",
            "image block after block",
            OperationKind::InsertBlock {
                position: InsertPosition::After(after),
                block,
            },
        )
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
            if stored.is_none() {
                layout.wrap_clearance = None;
            }
        })
    }

    /// Sets the four logical text clearances around a floated image. A spacing
    /// dialog is one authored choice and therefore one undoable operation.
    pub fn set_image_block_wrap_clearance(
        &mut self,
        block_id: impl AsRef<str>,
        top_twips: i32,
        end_twips: i32,
        bottom_twips: i32,
        start_twips: i32,
    ) -> Result<AppDocument, AppApiError> {
        let length = |twips| {
            opendoc_core::Length::from_twips(twips)
                .map_err(|err| AppApiError::Model(err.to_string()))
        };
        let clearance = opendoc_core::ImageWrapClearance {
            top: length(top_twips)?,
            end: length(end_twips)?,
            bottom: length(bottom_twips)?,
            start: length(start_twips)?,
        };
        clearance
            .validate()
            .map_err(|err| AppApiError::Model(err.to_string()))?;
        self.update_image_layout(block_id.as_ref(), "image wrap clearance", |layout| {
            layout.wrap_clearance = (!clearance.is_empty()).then_some(clearance);
        })
    }

    /// Makes an image an out-of-flow object as defined by ADR 0022.
    ///
    /// This command persists and collaborates the intent. `None` anchors at
    /// the page content rectangle; a supplied id anchors at that block's
    /// border box. Both offsets are twips and may be negative.
    pub fn set_image_block_positioned(
        &mut self,
        block_id: impl AsRef<str>,
        anchor_block_id: Option<&str>,
        horizontal_offset_twips: i32,
        vertical_offset_twips: i32,
        layer: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let block_id = parse_id(block_id.as_ref())?;
        let anchor = match anchor_block_id {
            Some(anchor) => {
                let anchor = parse_id(anchor)?;
                if anchor == block_id {
                    return Err(AppApiError::Format(
                        "positioned image cannot anchor to itself".to_string(),
                    ));
                }
                opendoc_core::PositionedImageAnchor::Block(anchor)
            }
            None => opendoc_core::PositionedImageAnchor::PageContent,
        };
        let layer = opendoc_core::PositionedImageLayer::parse(layer.as_ref())
            .map_err(|error| AppApiError::Model(error.to_string()))?;
        let positioned = opendoc_core::PositionedImage {
            anchor,
            horizontal_offset: opendoc_core::Length::from_twips(horizontal_offset_twips)
                .map_err(|error| AppApiError::Model(error.to_string()))?,
            vertical_offset: opendoc_core::Length::from_twips(vertical_offset_twips)
                .map_err(|error| AppApiError::Model(error.to_string()))?,
            layer,
        };
        self.update_image_layout(&block_id.to_string(), "position image", |layout| {
            layout.placement = None;
            layout.positioned = Some(positioned);
        })
    }

    /// Returns an image from the positioned-object model to normal in-flow
    /// layout without materialising an otherwise implicit block placement.
    pub fn clear_image_block_positioned(
        &mut self,
        block_id: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        self.update_image_layout(block_id.as_ref(), "clear image position", |layout| {
            layout.positioned = None;
        })
    }

    pub fn set_image_block_effects(
        &mut self,
        block_id: impl AsRef<str>,
        rotation_degrees: i16,
        opacity_percent: u8,
    ) -> Result<AppDocument, AppApiError> {
        self.update_image_layout(block_id.as_ref(), "image effects", |layout| {
            layout.rotation_degrees = (rotation_degrees != 0).then_some(rotation_degrees);
            layout.opacity_percent = (opacity_percent != 100).then_some(opacity_percent);
        })
    }

    pub fn set_image_block_crop(
        &mut self,
        block_id: impl AsRef<str>,
        top_percent: u8,
        right_percent: u8,
        bottom_percent: u8,
        left_percent: u8,
    ) -> Result<AppDocument, AppApiError> {
        let crop = opendoc_core::ImageCrop {
            top_percent,
            right_percent,
            bottom_percent,
            left_percent,
        };
        crop.validate()
            .map_err(|error| AppApiError::Model(error.to_string()))?;
        self.update_image_layout(block_id.as_ref(), "image crop", |layout| {
            layout.crop = (!crop.is_empty()).then_some(crop);
        })
    }

    pub fn set_image_block_caption(
        &mut self,
        block_id: impl AsRef<str>,
        caption: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let caption = caption.into();
        self.update_image_layout(block_id.as_ref(), "image caption", |layout| {
            layout.caption = (!caption.trim().is_empty()).then_some(caption);
        })
    }

    pub fn set_image_block_border(
        &mut self,
        block_id: impl AsRef<str>,
        style: impl AsRef<str>,
        twips: i32,
        color: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let style = opendoc_core::BorderStyle::parse(style.as_ref().trim())
            .map_err(|error| AppApiError::Model(error.to_string()))?;
        let border = if style == opendoc_core::BorderStyle::None {
            None
        } else {
            let width = opendoc_core::Length::from_twips(twips)
                .map_err(|error| AppApiError::Model(error.to_string()))?;
            let color = opendoc_core::Color::parse(color.as_ref().trim())
                .map_err(|error| AppApiError::Model(error.to_string()))?;
            Some(
                opendoc_core::CellBorder::new(style, width, color)
                    .map_err(|error| AppApiError::Model(error.to_string()))?,
            )
        };
        self.update_image_layout(block_id.as_ref(), "image border", |layout| {
            layout.border = border;
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
        layout
            .validate()
            .map_err(|error| AppApiError::Model(error.to_string()))?;
        self.apply(
            "update-image-layout",
            label,
            OperationKind::UpdateImageLayout { block_id, layout },
        )
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
        let html = app.document().body_html();
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
            projected.body_html().contains("width: 72pt; height: auto;"),
            "{}",
            projected.body_html()
        );
    }

    #[test]
    fn float_clearance_is_durable_and_clears_with_block_placement() {
        let (mut app, image_id) = app_with_image();
        app.set_image_block_placement(&image_id, "wrap-start")
            .unwrap();
        app.set_image_block_wrap_clearance(&image_id, 20, 40, 60, 80)
            .unwrap();
        assert_eq!(
            layout(&app).wrap_clearance.unwrap().start,
            Length::from_twips(80).unwrap()
        );
        assert!(app
            .document()
            .body_html()
            .contains("--doc-image-clearance-start: 4pt"));
        app.set_image_block_placement(&image_id, "block").unwrap();
        assert!(layout(&app).wrap_clearance.is_none());
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
        assert!(!app.document().body_html().contains("width:"));
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
                .body_html()
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
            .body_html()
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
    fn positioned_image_persists_as_one_layout_operation_and_projects_geometry() {
        let (mut app, image_id) = app_with_image();
        let anchor_id = app.document.blocks[0].id.to_string();
        let operations_before = app.document().operations.len();
        app.set_image_block_positioned(&image_id, Some(&anchor_id), -240, 480, "behind-text")
            .unwrap();

        let stored_layout = layout(&app);
        let positioned = stored_layout
            .positioned
            .as_ref()
            .expect("position persisted");
        assert_eq!(positioned.horizontal_offset.twips(), -240);
        assert_eq!(positioned.vertical_offset.twips(), 480);
        assert_eq!(
            positioned.layer,
            opendoc_core::PositionedImageLayer::BehindText
        );
        assert_eq!(app.document().operations.len(), operations_before + 1);
        assert!(app
            .document()
            .body_html()
            .contains("data-positioned=\"true\""));
        assert!(app
            .set_image_block_positioned(&image_id, Some(&image_id), 0, 0, "behind-text")
            .is_err());
    }

    /// The desktop invokes positioning through the named command, where the
    /// dispatcher owns undo checkpoints.  Keep this separate from the direct
    /// service test above so a future UI/API wiring regression cannot make a
    /// persisted position look like an undoable gesture.
    #[test]
    fn positioned_image_command_is_undoable_and_can_return_to_flow() {
        let (mut app, image_id) = app_with_image();
        let anchor_id = app.document.blocks[0].id.to_string();
        let checkpoints_before = app.undo_stack.len();
        app.dispatch_command(
            "set_image_block_positioned",
            serde_json::json!({
                "blockId": image_id,
                "anchorBlockId": anchor_id,
                "horizontalOffsetTwips": -240,
                "verticalOffsetTwips": 480,
                "layer": "in-front-of-text",
            }),
        )
        .expect("position image");
        assert_eq!(app.undo_stack.len(), checkpoints_before + 1);
        assert!(layout(&app).positioned.is_some());

        app.dispatch_command(
            "clear_image_block_positioned",
            serde_json::json!({ "blockId": image_id }),
        )
        .expect("return image to flow");
        assert!(layout(&app).positioned.is_none());
        app.undo_current_edit().expect("undo returning to flow");
        assert!(layout(&app).positioned.is_some());
    }

    #[test]
    fn visual_effects_are_one_validated_undoable_image_layout_edit() {
        let (mut app, image_id) = app_with_image();
        app.dispatch_command(
            "set_image_block_effects",
            serde_json::json!({
                "blockId": image_id,
                "rotationDegrees": 90,
                "opacityPercent": 40,
            }),
        )
        .expect("effects");
        assert_eq!(layout(&app).rotation_degrees, Some(90));
        assert_eq!(layout(&app).opacity_percent, Some(40));
        let html = app.document().body_html();
        assert!(html.contains("transform: rotate(90deg);"), "{html}");
        assert!(html.contains("opacity: 0.4;"), "{html}");
        app.undo_current_edit().expect("undo effects");
        assert!(layout(&app).is_empty());

        let error = app
            .set_image_block_effects(&image_id, 361, 100)
            .expect_err("invalid rotation");
        assert!(matches!(error, AppApiError::Model(_)), "{error:?}");
    }

    #[test]
    fn crop_is_projected_and_refuses_to_erase_the_image() {
        let (mut app, image_id) = app_with_image();
        app.set_image_block_crop(&image_id, 10, 20, 30, 5)
            .expect("crop");
        assert_eq!(
            layout(&app).crop,
            Some(opendoc_core::ImageCrop {
                top_percent: 10,
                right_percent: 20,
                bottom_percent: 30,
                left_percent: 5,
            })
        );
        assert!(app
            .document()
            .body_html()
            .contains("clip-path: inset(10% 20% 30% 5%);"));
        assert!(app.set_image_block_crop(&image_id, 50, 0, 50, 0).is_err());
    }

    #[test]
    fn caption_is_a_separate_undoable_image_layout_edit() {
        let (mut app, image_id) = app_with_image();
        app.dispatch_command(
            "set_image_block_caption",
            serde_json::json!({ "blockId": image_id, "caption": "Figure 1. Results" }),
        )
        .expect("caption");
        assert_eq!(layout(&app).caption.as_deref(), Some("Figure 1. Results"));
        let html = app.document().body_html();
        assert!(html.contains("<figcaption id=\"opendoc-image-caption:"));
        assert!(html.contains(">Figure 1. Results</figcaption>"));
        app.undo_current_edit().expect("undo caption");
        assert!(layout(&app).caption.is_none());
    }

    #[test]
    fn border_is_validated_rendered_and_undoable() {
        let (mut app, image_id) = app_with_image();
        app.dispatch_command(
            "set_image_block_border",
            serde_json::json!({ "blockId": image_id, "style": "dashed", "twips": 20, "color": "#336699" }),
        )
        .expect("border");
        let border = layout(&app).border.expect("stored border");
        assert_eq!(border.style(), opendoc_core::BorderStyle::Dashed);
        assert_eq!(border.width().twips(), 20);
        assert_eq!(border.color().as_hex(), "#336699");
        assert!(app
            .document()
            .body_html()
            .contains("border: 1pt dashed #336699;"));
        app.undo_current_edit().expect("undo border");
        assert!(layout(&app).border.is_none());
        assert!(app
            .set_image_block_border(&image_id, "solid", 121, "#000000")
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
