//! Mark add/remove, by inline id and across a text range.

use crate::inline_ops::inline_id;
use opendoc_core::{Block, BlockKind, Document, Inline, Mark, MarkKind, StableId};

pub(crate) fn add_mark(document: &mut Document, text_id: &StableId, mark: Mark) -> bool {
    add_mark_in_blocks(&mut document.blocks, text_id, mark)
}

pub(crate) fn remove_mark(
    document: &mut Document,
    text_id: &StableId,
    kind: &MarkKind,
    value: Option<&str>,
) -> bool {
    remove_mark_in_blocks(&mut document.blocks, text_id, kind, value)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MarkRangeResult {
    Applied,
    Degraded,
    Missing,
}

pub(crate) fn add_mark_range(
    document: &mut Document,
    range: &opendoc_core::TextRange,
    mark: Mark,
) -> MarkRangeResult {
    let ids = editable_inline_ids(&document.blocks);
    let Some(start_index) = ids.iter().position(|id| id == &range.start) else {
        return if ids.iter().any(|id| id == &range.end) {
            add_mark(document, &range.end, mark);
            MarkRangeResult::Degraded
        } else {
            MarkRangeResult::Missing
        };
    };
    let Some(end_index) = ids.iter().position(|id| id == &range.end) else {
        add_mark(document, &range.start, mark);
        return MarkRangeResult::Degraded;
    };
    let first = start_index.min(end_index);
    let last = start_index.max(end_index);
    for id in &ids[first..=last] {
        add_mark(document, id, mark.clone());
    }
    MarkRangeResult::Applied
}

pub(crate) fn editable_inline_ids(blocks: &[Block]) -> Vec<StableId> {
    let mut ids = Vec::new();
    for block in blocks {
        for inline in &block.content {
            if matches!(inline, Inline::Text { .. } | Inline::Link { .. }) {
                ids.push(inline_id(inline).clone());
            }
        }
        if let BlockKind::Table { rows, .. } = &block.kind {
            for row in rows {
                for cell in &row.cells {
                    ids.extend(editable_inline_ids(&cell.blocks));
                }
            }
        }
    }
    ids
}

pub(crate) fn add_mark_in_blocks(blocks: &mut [Block], text_id: &StableId, mark: Mark) -> bool {
    for block in blocks {
        for inline in &mut block.content {
            match inline {
                Inline::Text { id, marks, .. } | Inline::Link { id, marks, .. }
                    if id == text_id =>
                {
                    if !marks.contains(&mark) {
                        marks.push(mark);
                    }
                    return true;
                }
                _ => {}
            }
        }
        if let BlockKind::Table { rows, .. } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    if add_mark_in_blocks(&mut cell.blocks, text_id, mark.clone()) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

pub(crate) fn remove_mark_in_blocks(
    blocks: &mut [Block],
    text_id: &StableId,
    kind_to_remove: &MarkKind,
    value_to_remove: Option<&str>,
) -> bool {
    for block in blocks {
        for inline in &mut block.content {
            match inline {
                Inline::Text { id, marks, .. } | Inline::Link { id, marks, .. }
                    if id == text_id =>
                {
                    marks.retain(|mark| {
                        mark.kind != *kind_to_remove
                            || value_to_remove
                                .is_some_and(|value| mark.value.as_deref() != Some(value))
                    });
                    return true;
                }
                _ => {}
            }
        }
        if let BlockKind::Table { rows, .. } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    if remove_mark_in_blocks(
                        cell.blocks.as_mut_slice(),
                        text_id,
                        kind_to_remove,
                        value_to_remove,
                    ) {
                        return true;
                    }
                }
            }
        }
    }
    false
}
