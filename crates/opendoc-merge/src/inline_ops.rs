//! Structural inline moves: insertion after an anchor, range deletion, takes.

use crate::blocks::{block_exists, find_block_mut};
use crate::inline_edit::delete_inline;
use crate::marks::editable_inline_ids;
use opendoc_core::{Anchor, Block, BlockKind, Document, Inline, InsertPosition, StableId};

pub(crate) fn insert_inlines_after_anchor(
    document: &mut Document,
    anchor: &Anchor,
    content: Vec<Inline>,
) -> AnchorInsertResult {
    match anchor {
        Anchor::TextRange(range) => {
            if insert_inlines_after_inline_id(document, &range.end, content.clone()) {
                AnchorInsertResult::Applied
            } else if insert_inlines_after_inline_id(document, &range.start, content) {
                AnchorInsertResult::Degraded
            } else {
                AnchorInsertResult::Missing
            }
        }
        // Inline insertion has no character atom representation yet. Refuse
        // rather than rounding a durable gap to the end of its whole run.
        Anchor::TokenRange(_) => AnchorInsertResult::Missing,
        Anchor::NearestBlock { block_id, .. } => {
            let Some(block) = find_block_mut(&mut document.blocks, block_id) else {
                return if insert_inlines_at_document_end(document, content) {
                    AnchorInsertResult::Degraded
                } else {
                    AnchorInsertResult::Missing
                };
            };
            append_inlines(&mut block.content, content);
            AnchorInsertResult::Applied
        }
        Anchor::Orphaned { .. } => AnchorInsertResult::Missing,
        Anchor::Document => {
            if insert_inlines_at_document_end(document, content) {
                AnchorInsertResult::Applied
            } else {
                AnchorInsertResult::Missing
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AnchorInsertResult {
    Applied,
    Degraded,
    Missing,
}

pub(crate) fn insert_inlines_after_inline_id(
    document: &mut Document,
    target_inline_id: &StableId,
    content: Vec<Inline>,
) -> bool {
    let Some(target_content) =
        find_content_mut_containing_inline(&mut document.blocks, target_inline_id)
    else {
        return false;
    };
    let mut after = Some(target_inline_id.clone());
    for inline in content {
        let inserted_id = inline_id(&inline).clone();
        insert_inline(target_content, InsertPosition::after_or_last(after), inline);
        after = Some(inserted_id);
    }
    true
}

pub(crate) fn insert_inlines_at_document_end(
    document: &mut Document,
    content: Vec<Inline>,
) -> bool {
    let Some(block) = document.blocks.last_mut() else {
        return false;
    };
    append_inlines(&mut block.content, content);
    true
}

pub(crate) fn append_inlines(target: &mut Vec<Inline>, content: Vec<Inline>) {
    let mut after = target.last().map(|inline| inline_id(inline).clone());
    for inline in content {
        let inserted_id = inline_id(&inline).clone();
        insert_inline(target, InsertPosition::after_or_last(after), inline);
        after = Some(inserted_id);
    }
}

pub(crate) fn find_content_mut_containing_inline<'a>(
    blocks: &'a mut [Block],
    target_id: &StableId,
) -> Option<&'a mut Vec<Inline>> {
    for block in blocks {
        if block
            .content
            .iter()
            .any(|inline| inline_id(inline) == target_id)
        {
            return Some(&mut block.content);
        }
        if let BlockKind::Table { rows, .. } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    if let Some(found) =
                        find_content_mut_containing_inline(&mut cell.blocks, target_id)
                    {
                        return Some(found);
                    }
                }
            }
        }
    }
    None
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RangeEditResult {
    Applied,
    Degraded,
    Missing,
}

pub(crate) fn delete_inline_range(
    document: &mut Document,
    range: &opendoc_core::TextRange,
) -> RangeEditResult {
    let ids = editable_inline_ids(&document.blocks);
    let start = ids.iter().position(|id| id == &range.start);
    let end = ids.iter().position(|id| id == &range.end);
    let targets = match (start, end) {
        (Some(start), Some(end)) => {
            let first = start.min(end);
            let last = start.max(end);
            ids[first..=last].to_vec()
        }
        (Some(start), None) => vec![ids[start].clone()],
        (None, Some(end)) => vec![ids[end].clone()],
        (None, None) => return RangeEditResult::Missing,
    };
    for id in &targets {
        delete_inline(document, id);
    }
    if start.is_some() && end.is_some() {
        RangeEditResult::Applied
    } else {
        RangeEditResult::Degraded
    }
}

/// Adds an inline at `position`, answering whether the anchor had gone
/// missing. Only an anchored insert can degrade; `First` and `Last` name no
/// sibling that could have been deleted.
pub(crate) fn insert_inline(
    content: &mut Vec<Inline>,
    position: InsertPosition,
    inline: Inline,
) -> bool {
    let new_inline_id = inline_id(&inline).clone();
    if content.iter().any(|item| inline_id(item) == &new_inline_id) {
        return false;
    }
    let anchor_index = position
        .anchor()
        .map(|target| content.iter().position(|item| inline_id(item) == target));
    let anchor_degraded = matches!(anchor_index, Some(None));
    let insert_at = position.index(content.len(), anchor_index.flatten());
    content.insert(insert_at, inline);
    anchor_degraded
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum InlineMoveResult {
    Applied,
    AnchorDegraded,
    MissingInline,
    MissingBlock,
}

pub(crate) fn move_inline_to_block(
    document: &mut Document,
    inline_id_to_move: &StableId,
    target_block_id: &StableId,
    position: InsertPosition,
) -> InlineMoveResult {
    if !block_exists(&document.blocks, target_block_id) {
        return InlineMoveResult::MissingBlock;
    }
    let Some(inline) = take_inline(&mut document.blocks, inline_id_to_move) else {
        return InlineMoveResult::MissingInline;
    };
    let Some(target) = find_block_mut(&mut document.blocks, target_block_id) else {
        return InlineMoveResult::MissingBlock;
    };
    if insert_inline(&mut target.content, position, inline) {
        InlineMoveResult::AnchorDegraded
    } else {
        InlineMoveResult::Applied
    }
}

pub(crate) fn take_inline(blocks: &mut [Block], inline_id_to_take: &StableId) -> Option<Inline> {
    for block in blocks {
        if let Some(index) = block
            .content
            .iter()
            .position(|inline| inline_id(inline) == inline_id_to_take)
        {
            return Some(block.content.remove(index));
        }
        if let BlockKind::Table { rows, .. } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    if let Some(inline) = take_inline(&mut cell.blocks, inline_id_to_take) {
                        return Some(inline);
                    }
                }
            }
        }
    }
    None
}

pub(crate) fn inline_id(inline: &Inline) -> &StableId {
    match inline {
        Inline::Text { id, .. }
        | Inline::Link { id, .. }
        | Inline::Citation { id, .. }
        | Inline::FootnoteRef { id, .. }
        | Inline::Mention { id, .. }
        | Inline::GooglePersonChip { id, .. }
        | Inline::GoogleRichLinkChip { id, .. }
        | Inline::Dropdown { id, .. }
        | Inline::DateChip { id, .. }
        | Inline::Equation { id, .. }
        | Inline::PageNumber { id, .. } => id,
    }
}
