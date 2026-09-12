//! Block-level property edits: headings, styles, images, list items, properties.

use crate::blocks::find_block_mut;
use crate::operation::BlockTextStyle;
use opendoc_core::{
    Block, BlockKind, BlockProperty, BlockPropertyKey, ImageLayout, ListKind, StableId,
};

pub(crate) fn update_block_equation_source(
    blocks: &mut [Block],
    block_id_to_update: &StableId,
    source: &str,
) -> Option<bool> {
    for block in blocks {
        if &block.id == block_id_to_update {
            return match &mut block.kind {
                BlockKind::EquationBlock { equation } => {
                    equation.source = source.to_string();
                    Some(true)
                }
                _ => Some(false),
            };
        }
        if let BlockKind::Table { rows, .. } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    if let Some(result) =
                        update_block_equation_source(&mut cell.blocks, block_id_to_update, source)
                    {
                        return Some(result);
                    }
                }
            }
        }
    }
    None
}

pub(crate) fn update_image_layout(
    blocks: &mut [Block],
    block_id_to_update: &StableId,
    value: &ImageLayout,
) -> Option<bool> {
    for block in blocks.iter_mut() {
        if &block.id == block_id_to_update {
            return match &mut block.kind {
                BlockKind::Image { layout, .. } => {
                    *layout = value.clone();
                    Some(true)
                }
                _ => Some(false),
            };
        }
        if let BlockKind::Table { rows, .. } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    if let Some(result) =
                        update_image_layout(&mut cell.blocks, block_id_to_update, value)
                    {
                        return Some(result);
                    }
                }
            }
        }
    }
    None
}

pub(crate) fn update_image_alt_text(
    blocks: &mut [Block],
    block_id_to_update: &StableId,
    value: &str,
) -> Option<bool> {
    for block in blocks {
        if &block.id == block_id_to_update {
            return match &mut block.kind {
                BlockKind::Image { alt_text, .. } => {
                    *alt_text = value.to_string();
                    Some(true)
                }
                _ => Some(false),
            };
        }
        if let BlockKind::Table { rows, .. } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    if let Some(result) =
                        update_image_alt_text(&mut cell.blocks, block_id_to_update, value)
                    {
                        return Some(result);
                    }
                }
            }
        }
    }
    None
}

pub(crate) fn update_image_blob_hash(
    blocks: &mut [Block],
    block_id_to_update: &StableId,
    value: &str,
) -> Option<bool> {
    for block in blocks {
        if &block.id == block_id_to_update {
            return match &mut block.kind {
                BlockKind::Image { blob_hash, .. } => {
                    *blob_hash = value.to_string();
                    Some(true)
                }
                _ => Some(false),
            };
        }
        if let BlockKind::Table { rows, .. } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    if let Some(result) =
                        update_image_blob_hash(&mut cell.blocks, block_id_to_update, value)
                    {
                        return Some(result);
                    }
                }
            }
        }
    }
    None
}

pub(crate) fn update_heading_level(
    blocks: &mut [Block],
    block_id_to_update: &StableId,
    level: u8,
) -> Option<bool> {
    for block in blocks {
        if &block.id == block_id_to_update {
            return match &mut block.kind {
                BlockKind::Heading {
                    level: heading_level,
                } => {
                    *heading_level = level;
                    Some(true)
                }
                _ => Some(false),
            };
        }
        if let BlockKind::Table { rows, .. } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    if let Some(result) =
                        update_heading_level(&mut cell.blocks, block_id_to_update, level)
                    {
                        return Some(result);
                    }
                }
            }
        }
    }
    None
}

pub(crate) fn set_block_text_style(
    blocks: &mut [Block],
    block_id_to_update: &StableId,
    style: BlockTextStyle,
) -> Option<bool> {
    for block in blocks {
        if &block.id == block_id_to_update {
            return match &block.kind {
                BlockKind::Paragraph | BlockKind::Heading { .. } | BlockKind::ListItem { .. } => {
                    block.kind = match style {
                        BlockTextStyle::Paragraph => BlockKind::Paragraph,
                        BlockTextStyle::Heading { level } => BlockKind::Heading { level },
                        BlockTextStyle::ListItem {
                            list_id,
                            level,
                            kind,
                        } => BlockKind::ListItem {
                            list_id,
                            level,
                            kind,
                        },
                    };
                    Some(true)
                }
                _ => Some(false),
            };
        }
        if let BlockKind::Table { rows, .. } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    if let Some(result) =
                        set_block_text_style(&mut cell.blocks, block_id_to_update, style.clone())
                    {
                        return Some(result);
                    }
                }
            }
        }
    }
    None
}

pub(crate) fn validate_block_text_style(style: &BlockTextStyle) -> Result<(), &'static str> {
    match style {
        BlockTextStyle::Paragraph => Ok(()),
        BlockTextStyle::Heading { level } if (1..=6).contains(level) => Ok(()),
        BlockTextStyle::Heading { .. } => Err("heading level is outside 1..=6"),
        BlockTextStyle::ListItem { level, .. } if *level <= 8 => Ok(()),
        BlockTextStyle::ListItem { .. } => Err("list item level is outside 0..=8"),
    }
}

pub(crate) fn update_list_item(
    blocks: &mut [Block],
    block_id_to_update: &StableId,
    level: u8,
    kind: ListKind,
) -> Option<bool> {
    for block in blocks {
        if &block.id == block_id_to_update {
            return match &mut block.kind {
                BlockKind::ListItem {
                    level: item_level,
                    kind: item_kind,
                    ..
                } => {
                    *item_level = level;
                    *item_kind = kind;
                    Some(true)
                }
                _ => Some(false),
            };
        }
        if let BlockKind::Table { rows, .. } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    if let Some(result) =
                        update_list_item(&mut cell.blocks, block_id_to_update, level, kind)
                    {
                        return Some(result);
                    }
                }
            }
        }
    }
    None
}

/// Writes one typed property on `block_id`, wherever it lives in the tree.
/// Returns `None` only when the block is gone — a concurrently deleted block
/// is the single failure mode, and every block kind accepts properties.
pub(crate) fn set_block_property(
    blocks: &mut [Block],
    block_id: &StableId,
    property: BlockProperty,
) -> Option<()> {
    let block = find_block_mut(blocks, block_id)?;
    block.properties.set(property);
    Some(())
}

pub(crate) fn clear_block_property(
    blocks: &mut [Block],
    block_id: &StableId,
    key: BlockPropertyKey,
) -> Option<()> {
    let block = find_block_mut(blocks, block_id)?;
    block.properties.clear(key);
    Some(())
}
