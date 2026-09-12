//! Block lookup, insertion and deletion inside a (possibly nested) block tree.

use opendoc_core::{Block, BlockKind, Document, StableId};

pub(crate) fn find_block_mut<'a>(
    blocks: &'a mut [Block],
    block_id: &StableId,
) -> Option<&'a mut Block> {
    for block in blocks {
        if &block.id == block_id {
            return Some(block);
        }
        if let BlockKind::Table { rows, .. } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    if let Some(found) = find_block_mut(&mut cell.blocks, block_id) {
                        return Some(found);
                    }
                }
            }
        }
    }
    None
}

pub(crate) fn block_exists(blocks: &[Block], block_id: &StableId) -> bool {
    blocks.iter().any(|block| {
        &block.id == block_id
            || matches!(&block.kind, BlockKind::Table { rows, .. } if rows.iter().any(|row| {
                row.cells
                    .iter()
                    .any(|cell| block_exists(&cell.blocks, block_id))
            }))
    })
}

pub(crate) fn insert_block(document: &mut Document, after: Option<StableId>, block: Block) -> bool {
    let mut anchor_degraded = false;
    let insert_at = match after {
        Some(target) => match document.blocks.iter().position(|item| item.id == target) {
            Some(index) => index + 1,
            None => {
                anchor_degraded = true;
                document.blocks.len()
            }
        },
        None => document.blocks.len(),
    };
    if !document.blocks.iter().any(|item| item.id == block.id) {
        document.blocks.insert(insert_at, block);
    }
    anchor_degraded
}

pub(crate) fn delete_block(document: &mut Document, block_id_to_delete: &StableId) -> bool {
    delete_block_in_blocks(&mut document.blocks, block_id_to_delete)
}

pub(crate) fn delete_block_in_blocks(
    blocks: &mut Vec<Block>,
    block_id_to_delete: &StableId,
) -> bool {
    let before = blocks.len();
    blocks.retain(|block| &block.id != block_id_to_delete);
    if blocks.len() != before {
        return true;
    }
    for block in blocks {
        if let BlockKind::Table { rows, .. } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    if delete_block_in_blocks(&mut cell.blocks, block_id_to_delete) {
                        return true;
                    }
                }
            }
        }
    }
    false
}
