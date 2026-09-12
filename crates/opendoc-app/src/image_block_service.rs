use crate::AppApiError;
use opendoc_core::{
    Block, BlockKind, BlockProperties, HashRef, ImageLayout, ImagePlacement, Length, StableId,
};

pub(crate) struct ImageBlockService<'a> {
    blobs: &'a [crate::AppBlobRef],
}

impl<'a> ImageBlockService<'a> {
    pub(crate) fn new(blobs: &'a [crate::AppBlobRef]) -> Self {
        Self { blobs }
    }

    pub(crate) fn image_block_for_existing_blob(
        &self,
        blob_hash: impl AsRef<str>,
        alt_text: impl Into<String>,
    ) -> Result<Block, AppApiError> {
        let hash = HashRef::parse(blob_hash.as_ref().trim())
            .map_err(|err| AppApiError::Model(err.to_string()))?
            .to_string();
        if !self.blobs.iter().any(|blob| blob.hash == hash) {
            return Err(AppApiError::NotFound("blob was not found".to_string()));
        }
        Ok(Block {
            id: StableId::new("block"),
            kind: BlockKind::Image {
                blob_hash: hash,
                alt_text: alt_text.into(),
                // A freshly inserted image states no geometry, so it is drawn
                // at whatever the bytes decode to. Writing the decoded size in
                // here would freeze a fact about the blob into the document.
                layout: ImageLayout::default(),
            },
            content: Vec::new(),
            properties: BlockProperties::default(),
        })
    }
}

/// The layout an image block currently states.
///
/// Image blocks nest: a picture inside a table cell is still an image block,
/// so the search walks cells the same way the merge-side helpers do.
pub(crate) fn image_block_layout(
    blocks: &[Block],
    block_id: &StableId,
) -> Result<ImageLayout, AppApiError> {
    match find_image_block(blocks, block_id) {
        Some(Some(layout)) => Ok(layout.clone()),
        Some(None) => Err(AppApiError::Conflict(format!(
            "block {block_id} is not an image"
        ))),
        None => Err(AppApiError::NotFound(format!(
            "block {block_id} was not found"
        ))),
    }
}

/// `None` when no such block exists, `Some(None)` when it exists but is not an
/// image, `Some(Some(layout))` when it is.
fn find_image_block<'a>(
    blocks: &'a [Block],
    block_id: &StableId,
) -> Option<Option<&'a ImageLayout>> {
    for block in blocks {
        if &block.id == block_id {
            return Some(match &block.kind {
                BlockKind::Image { layout, .. } => Some(layout),
                _ => None,
            });
        }
        if let BlockKind::Table { rows, .. } = &block.kind {
            for row in rows {
                for cell in &row.cells {
                    if let Some(found) = find_image_block(&cell.blocks, block_id) {
                        return Some(found);
                    }
                }
            }
        }
    }
    None
}

/// Parses a drawn length from the command surface.
///
/// Rejected rather than clamped: a zero or negative display size is not a
/// smaller picture, and silently turning it into a legal one would hide the
/// caller's arithmetic bug behind an image that looks nearly right.
pub(crate) fn image_display_length(twips: i32) -> Result<Length, AppApiError> {
    let length = Length::from_twips(twips).map_err(|err| AppApiError::Model(err.to_string()))?;
    if length.twips() <= 0 {
        return Err(AppApiError::Model("image size is not positive".to_string()));
    }
    Ok(length)
}

pub(crate) fn parse_image_placement(value: &str) -> Result<ImagePlacement, AppApiError> {
    ImagePlacement::parse(value.trim()).map_err(|err| AppApiError::Model(err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use opendoc_core::TableCell;

    fn image_block(id: &str, layout: ImageLayout) -> Block {
        Block {
            id: StableId::parse(id).unwrap(),
            kind: BlockKind::Image {
                blob_hash: "sha256:abc".to_string(),
                alt_text: String::new(),
                layout,
            },
            content: Vec::new(),
            properties: BlockProperties::default(),
        }
    }

    #[test]
    fn an_image_block_needs_a_blob_that_exists() {
        let service = ImageBlockService::new(&[]);
        let error = service
            .image_block_for_existing_blob("sha256:abc", "")
            .unwrap_err();
        assert!(matches!(error, AppApiError::NotFound(_)), "{error:?}");
    }

    #[test]
    fn layout_is_read_from_a_nested_image_block() {
        let inner = image_block(
            "block-inner",
            ImageLayout {
                width: Some(Length::from_twips(1440).unwrap()),
                height: None,
                placement: Some(ImagePlacement::WrapEnd),
            },
        );
        let table = Block {
            id: StableId::parse("block-table").unwrap(),
            kind: BlockKind::Table {
                columns: Vec::new(),
                rows: vec![opendoc_core::TableRow {
                    id: StableId::parse("row-1").unwrap(),
                    cells: vec![TableCell {
                        id: StableId::parse("cell-1").unwrap(),
                        blocks: vec![inner],
                        ..TableCell::default()
                    }],
                }],
            },
            content: Vec::new(),
            properties: BlockProperties::default(),
        };
        let blocks = vec![table];
        let found = image_block_layout(&blocks, &StableId::parse("block-inner").unwrap()).unwrap();
        assert_eq!(found.width, Some(Length::from_twips(1440).unwrap()));
        assert_eq!(found.effective_placement(), ImagePlacement::WrapEnd);
    }

    #[test]
    fn reading_the_layout_of_a_non_image_block_is_a_conflict_not_a_default() {
        let blocks = vec![Block::paragraph("text")];
        let id = blocks[0].id.clone();
        let error = image_block_layout(&blocks, &id).unwrap_err();
        assert!(
            matches!(error, AppApiError::Conflict(ref message) if message.contains("not an image")),
            "{error:?}"
        );
    }

    #[test]
    fn a_missing_block_is_not_found() {
        let blocks = vec![Block::paragraph("text")];
        let error =
            image_block_layout(&blocks, &StableId::parse("block-gone").unwrap()).unwrap_err();
        assert!(matches!(error, AppApiError::NotFound(_)), "{error:?}");
    }

    #[test]
    fn a_display_size_has_to_be_positive() {
        assert!(image_display_length(1440).is_ok());
        assert!(image_display_length(0).is_err());
        assert!(image_display_length(-20).is_err());
    }

    #[test]
    fn placement_names_round_trip_through_the_command_surface() {
        for placement in ImagePlacement::ALL {
            assert_eq!(
                parse_image_placement(placement.as_str()).unwrap(),
                placement
            );
        }
        assert!(parse_image_placement("behind-text").is_err());
    }
}
