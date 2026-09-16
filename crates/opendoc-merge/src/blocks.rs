//! Block lookup, insertion and deletion inside a (possibly nested) block tree.

use opendoc_core::{Block, BlockKind, Document, InsertPosition, StableId};

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

/// Adds a block at `position`, answering whether the anchor had gone missing.
///
/// Only an anchored insert can degrade, and only when the anchor is gone:
/// `First` and `Last` name no sibling that could have been deleted, which is
/// why `First` is safe to mint for an undo. `Before` and `After` retain a
/// sibling identity and therefore share the same missing-anchor fallback.
pub(crate) fn insert_block(
    document: &mut Document,
    position: InsertPosition,
    block: Block,
) -> bool {
    // An anchor inside a table cell has to be found there. `delete_block_in_blocks`
    // already descends; this did not, so `InsertBlock { position: After(cell block) }`
    // silently degraded to appending at the end of the *body* — which is why
    // pressing Enter in a table cell produced a soft break instead of a paragraph.
    if let Some(target) = position.anchor() {
        if !document.blocks.iter().any(|item| &item.id == target) {
            if let Some(inserted) =
                insert_block_beside_nested_anchor(&mut document.blocks, &position, block)
            {
                // `inserted` is the block back again when no nested anchor matched
                // either, in which case fall through to the body rules below.
                return insert_block_into(&mut document.blocks, position, inserted);
            }
            return false;
        }
    }
    insert_block_into(&mut document.blocks, position, block)
}

/// Places `block` relative to an anchor that lives inside a table cell.
///
/// Returns `None` once it has been placed, or gives the block back when no cell
/// holds the anchor.
fn insert_block_beside_nested_anchor(
    blocks: &mut [Block],
    position: &InsertPosition,
    block: Block,
) -> Option<Block> {
    let mut carried = block;
    for host in blocks {
        if let BlockKind::Table { rows, .. } = &mut host.kind {
            for row in rows {
                for cell in &mut row.cells {
                    let anchor = position.anchor();
                    if anchor.is_some_and(|target| cell.blocks.iter().any(|b| &b.id == target)) {
                        insert_block_into(&mut cell.blocks, position.clone(), carried);
                        return None;
                    }
                    carried =
                        insert_block_beside_nested_anchor(&mut cell.blocks, position, carried)?;
                }
            }
        }
    }
    Some(carried)
}

/// The body rules: `First`/`Last` place absolutely, `After` degrades to an
/// append when its anchor is gone, and an id already present is never doubled.
fn insert_block_into(blocks: &mut Vec<Block>, position: InsertPosition, block: Block) -> bool {
    let anchor_index = position
        .anchor()
        .map(|target| blocks.iter().position(|item| &item.id == target));
    let anchor_degraded = matches!(anchor_index, Some(None));
    let insert_at = position.index(blocks.len(), anchor_index.flatten());
    if !blocks.iter().any(|item| item.id == block.id) {
        blocks.insert(insert_at, block);
    }
    anchor_degraded
}

pub(crate) fn delete_block(document: &mut Document, block_id_to_delete: &StableId) -> bool {
    delete_block_in_blocks(&mut document.blocks, block_id_to_delete)
}

/// Whether deleting this exact block would leave its table cell with no local
/// block at all.  Body blocks are unconstrained; a cell is not.  Keep this
/// query beside the delete primitive so ordinary commands and review
/// acceptance cannot drift apart on the same model invariant.
pub(crate) fn delete_would_empty_table_cell(document: &Document, block_id: &StableId) -> bool {
    delete_would_empty_table_cell_in_blocks(&document.blocks, block_id)
}

pub(crate) fn delete_would_empty_table_cell_in_blocks(
    blocks: &[Block],
    block_id: &StableId,
) -> bool {
    for block in blocks {
        if let BlockKind::Table { rows, .. } = &block.kind {
            for row in rows {
                for cell in &row.cells {
                    if cell.blocks.len() == 1 && &cell.blocks[0].id == block_id {
                        return true;
                    }
                    if delete_would_empty_table_cell_in_blocks(&cell.blocks, block_id) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// Replaces one existing block in its current sibling container.  This is not
/// expressed as delete-plus-insert: doing so would lose the nested-container
/// location when the deleted identity was the only way to name it.
pub(crate) fn replace_block(
    document: &mut Document,
    block_id: &StableId,
    replacement: Block,
) -> bool {
    replace_block_in_blocks(&mut document.blocks, block_id, replacement)
}

fn replace_block_in_blocks(blocks: &mut [Block], block_id: &StableId, replacement: Block) -> bool {
    for block in blocks {
        if &block.id == block_id {
            *block = replacement;
            return true;
        }
        if let BlockKind::Table { rows, .. } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    if replace_block_in_blocks(&mut cell.blocks, block_id, replacement.clone()) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// The result of moving one block without changing its stable identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MoveBlockResult {
    Moved,
    MissingSource,
    MissingAnchor,
    SameBlockAnchor,
    DescendantAnchor,
    WouldEmptyCell,
}

/// Reparents or reorders a block by stable sibling identity.
///
/// A table cell has a non-empty-block invariant.  Rejecting an attempt to
/// move its sole child is preferable to quietly producing a model which no
/// longer validates.  Likewise, a source can never be placed below itself:
/// that would turn the block tree into a cycle in a path-shaped disguise.
pub(crate) fn move_block(
    document: &mut Document,
    block_id: &StableId,
    position: InsertPosition,
) -> MoveBlockResult {
    let Some(anchor) = position.anchor() else {
        // `First`/`Last` intentionally name the document body.
        let Some(block) = take_block(&mut document.blocks, block_id, false) else {
            return MoveBlockResult::MissingSource;
        };
        insert_block_into(&mut document.blocks, position, block);
        return MoveBlockResult::Moved;
    };
    if anchor == block_id {
        return MoveBlockResult::SameBlockAnchor;
    }
    let Some(source) = block_ref(&document.blocks, block_id) else {
        return MoveBlockResult::MissingSource;
    };
    if block_exists(std::slice::from_ref(source), anchor) {
        return MoveBlockResult::DescendantAnchor;
    }
    if !block_exists(&document.blocks, anchor) {
        return MoveBlockResult::MissingAnchor;
    }
    let Some(block) = take_block(&mut document.blocks, block_id, false) else {
        return MoveBlockResult::WouldEmptyCell;
    };
    debug_assert!(!block_exists(&document.blocks, block_id));
    let degraded = insert_block(document, position, block);
    debug_assert!(!degraded, "a verified move anchor must remain present");
    MoveBlockResult::Moved
}

fn block_ref<'a>(blocks: &'a [Block], block_id: &StableId) -> Option<&'a Block> {
    for block in blocks {
        if &block.id == block_id {
            return Some(block);
        }
        if let BlockKind::Table { rows, .. } = &block.kind {
            for row in rows {
                for cell in &row.cells {
                    if let Some(found) = block_ref(&cell.blocks, block_id) {
                        return Some(found);
                    }
                }
            }
        }
    }
    None
}

/// Removes a block and returns it. `None` also means that removing the block
/// would leave a table cell empty.
fn take_block(blocks: &mut Vec<Block>, block_id: &StableId, nested: bool) -> Option<Block> {
    if let Some(index) = blocks.iter().position(|block| &block.id == block_id) {
        if nested && blocks.len() == 1 {
            return None;
        }
        return Some(blocks.remove(index));
    }
    for block in blocks {
        if let BlockKind::Table { rows, .. } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    if let Some(found) = take_block(&mut cell.blocks, block_id, true) {
                        return Some(found);
                    }
                }
            }
        }
    }
    None
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
