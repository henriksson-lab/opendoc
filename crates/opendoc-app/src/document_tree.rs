use crate::{inline_id, AppBlock};
use opendoc_core::{Block, BlockKind, Inline, StableId};

pub(crate) fn block_exists(blocks: &[Block], block_id: &StableId) -> bool {
    blocks.iter().any(|block| {
        &block.id == block_id
            || match &block.kind {
                BlockKind::Table { rows } => rows
                    .iter()
                    .flat_map(|row| row.cells.iter())
                    .any(|cell| block_exists(&cell.blocks, block_id)),
                _ => false,
            }
    })
}

pub(crate) fn default_table_block() -> Block {
    let cell = |text: &str| opendoc_core::TableCell {
        id: StableId::new("cell"),
        blocks: vec![Block::paragraph(text)],
        properties: Vec::new(),
    };
    Block {
        id: StableId::new("block"),
        kind: BlockKind::Table {
            rows: vec![
                opendoc_core::TableRow {
                    id: StableId::new("row"),
                    cells: vec![cell("A1"), cell("B1")],
                },
                opendoc_core::TableRow {
                    id: StableId::new("row"),
                    cells: vec![cell("A2"), cell("B2")],
                },
            ],
        },
        content: Vec::new(),
        properties: Vec::new(),
    }
}

pub(crate) fn find_block_in_blocks<'a>(
    blocks: &'a [Block],
    block_id_to_find: &StableId,
) -> Option<&'a Block> {
    for block in blocks {
        if &block.id == block_id_to_find {
            return Some(block);
        }
        if let BlockKind::Table { rows } = &block.kind {
            for row in rows {
                for cell in &row.cells {
                    if let Some(found) = find_block_in_blocks(&cell.blocks, block_id_to_find) {
                        return Some(found);
                    }
                }
            }
        }
    }
    None
}

pub(crate) fn app_editable_inline_ids(blocks: &[AppBlock]) -> Vec<String> {
    blocks
        .iter()
        .flat_map(|block| {
            let direct = block
                .content
                .iter()
                .filter(|inline| {
                    matches!(
                        inline.kind.as_str(),
                        "text" | "link" | "equation" | "mention"
                    )
                })
                .map(|inline| inline.id.clone())
                .collect::<Vec<_>>();
            let nested = block
                .rows
                .iter()
                .flat_map(|row| row.iter())
                .flat_map(|cell| app_editable_inline_ids(cell))
                .collect::<Vec<_>>();
            direct.into_iter().chain(nested).collect::<Vec<_>>()
        })
        .collect()
}

pub(crate) fn blocks_reference_blob(blocks: &[Block], blob_hash: &str) -> bool {
    blocks.iter().any(|block| match &block.kind {
        BlockKind::Image {
            blob_hash: hash, ..
        } => hash == blob_hash,
        BlockKind::Table { rows } => rows
            .iter()
            .flat_map(|row| row.cells.iter())
            .any(|cell| blocks_reference_blob(&cell.blocks, blob_hash)),
        _ => false,
    })
}

pub(crate) fn block_contains_inline(
    blocks: &[Block],
    block_id: &StableId,
    inline_id_to_find: &StableId,
) -> bool {
    blocks.iter().any(|block| {
        if &block.id == block_id {
            return block
                .content
                .iter()
                .any(|inline| inline_id(inline) == inline_id_to_find);
        }
        match &block.kind {
            BlockKind::Table { rows } => rows
                .iter()
                .flat_map(|row| row.cells.iter())
                .any(|cell| block_contains_inline(&cell.blocks, block_id, inline_id_to_find)),
            _ => false,
        }
    })
}

pub(crate) fn find_inline_in_blocks<'a>(
    blocks: &'a [Block],
    inline_id_to_find: &StableId,
) -> Option<&'a Inline> {
    for block in blocks {
        if let Some(inline) = block
            .content
            .iter()
            .find(|inline| inline_id(inline) == inline_id_to_find)
        {
            return Some(inline);
        }
        if let BlockKind::Table { rows } = &block.kind {
            for row in rows {
                for cell in &row.cells {
                    if let Some(inline) = find_inline_in_blocks(&cell.blocks, inline_id_to_find) {
                        return Some(inline);
                    }
                }
            }
        }
    }
    None
}

pub(crate) fn find_block_id_containing_inline(
    blocks: &[Block],
    inline_id_to_find: &StableId,
) -> Option<StableId> {
    for block in blocks {
        if block
            .content
            .iter()
            .any(|inline| inline_id(inline) == inline_id_to_find)
        {
            return Some(block.id.clone());
        }
        if let BlockKind::Table { rows } = &block.kind {
            for row in rows {
                for cell in &row.cells {
                    if let Some(found) =
                        find_block_id_containing_inline(&cell.blocks, inline_id_to_find)
                    {
                        return Some(found);
                    }
                }
            }
        }
    }
    None
}

pub(crate) fn block_tree_contains_id(blocks: &[Block], block_id_to_find: &StableId) -> bool {
    for block in blocks {
        if &block.id == block_id_to_find {
            return true;
        }
        if let BlockKind::Table { rows } = &block.kind {
            for row in rows {
                for cell in &row.cells {
                    if block_tree_contains_id(&cell.blocks, block_id_to_find) {
                        return true;
                    }
                }
            }
        }
    }
    false
}
