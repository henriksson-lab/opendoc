//! Anchor resolution and the repairs that keep anchors pointing at live content.

use crate::blocks::block_exists;
use crate::inline_ops::inline_id;
use crate::marks::editable_inline_ids;
use crate::suggestions::push_provenance_once;
use opendoc_core::{
    Anchor, Block, BlockKind, Document, ModelWarning, StableId, SuggestionKind, SuggestionState,
};

pub(crate) fn repair_comment_anchors(document: &mut Document, warnings: &mut Vec<ModelWarning>) {
    let mut degraded = Vec::new();
    for thread in &mut document.comments {
        if thread.deleted || anchor_resolves_in_blocks(&document.blocks, &thread.anchor) {
            continue;
        }
        repair_comment_anchor(
            &document.blocks,
            &mut thread.anchor,
            "comment anchor text was deleted",
        );
        degraded.push(thread.id.clone());
    }
    for thread_id in degraded {
        warnings.push(ModelWarning {
            code: "comment-anchor-degraded".to_string(),
            message: format!("comment thread {thread_id} moved to nearest surviving anchor"),
        });
    }
}

pub(crate) fn repair_comment_anchor(blocks: &[Block], anchor: &mut Anchor, warning: &str) {
    match anchor {
        Anchor::TextRange(range) => {
            let ids = editable_inline_ids(blocks);
            let start_survives = ids.iter().any(|id| id == &range.start);
            let end_survives = ids.iter().any(|id| id == &range.end);
            match (start_survives, end_survives) {
                (true, false) => range.end = range.start.clone(),
                (false, true) => range.start = range.end.clone(),
                (false, false) => *anchor = nearest_block_anchor_in_blocks(blocks, warning),
                (true, true) => {}
            }
        }
        Anchor::NearestBlock { .. } if !anchor_resolves_in_blocks(blocks, anchor) => {
            *anchor = nearest_block_anchor_in_blocks(blocks, warning);
        }
        _ => {}
    }
}

pub(crate) fn repair_suggestion_anchors(document: &mut Document, warnings: &mut Vec<ModelWarning>) {
    let replacement = nearest_block_anchor(document, "suggestion anchor text was deleted");
    let editable_ids = editable_inline_ids(&document.blocks);
    let mut emitted_warnings = Vec::new();

    for suggestion in &mut document.suggestions {
        if suggestion.state != SuggestionState::Proposed {
            continue;
        }

        match &mut suggestion.kind {
            SuggestionKind::Insert { anchor, .. } => {
                if anchor_resolves_in_blocks(&document.blocks, anchor) {
                    continue;
                }
                *anchor = replacement.clone();
                push_provenance_once(suggestion, "auto-degraded:missing-anchor");
                emitted_warnings.push(ModelWarning {
                    code: "suggestion-anchor-degraded".to_string(),
                    message: format!(
                        "suggestion {} moved to nearest surviving block",
                        suggestion.id
                    ),
                });
            }
            SuggestionKind::Delete { range } | SuggestionKind::Format { range, .. } => {
                match repair_text_range(&editable_ids, range) {
                    TextRangeRepair::Unchanged => {}
                    TextRangeRepair::Collapsed => {
                        push_provenance_once(suggestion, "auto-degraded:partial-range");
                        emitted_warnings.push(ModelWarning {
                            code: "suggestion-range-degraded".to_string(),
                            message: format!(
                                "suggestion {} collapsed to surviving range endpoint",
                                suggestion.id
                            ),
                        });
                    }
                    TextRangeRepair::Missing => {
                        suggestion.state = SuggestionState::Rejected;
                        push_provenance_once(suggestion, "auto-rejected:missing-range");
                        emitted_warnings.push(ModelWarning {
                            code: "suggestion-range-missing".to_string(),
                            message: format!(
                                "suggestion {} was rejected because its range was deleted",
                                suggestion.id
                            ),
                        });
                    }
                }
            }
        }
    }

    warnings.extend(emitted_warnings);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TextRangeRepair {
    Unchanged,
    Collapsed,
    Missing,
}

pub(crate) fn repair_text_range(
    editable_ids: &[StableId],
    range: &mut opendoc_core::TextRange,
) -> TextRangeRepair {
    let start_exists = editable_ids.iter().any(|id| id == &range.start);
    let end_exists = editable_ids.iter().any(|id| id == &range.end);
    match (start_exists, end_exists) {
        (true, true) => TextRangeRepair::Unchanged,
        (true, false) => {
            range.end = range.start.clone();
            TextRangeRepair::Collapsed
        }
        (false, true) => {
            range.start = range.end.clone();
            TextRangeRepair::Collapsed
        }
        (false, false) => TextRangeRepair::Missing,
    }
}

pub(crate) fn anchor_resolves(document: &Document, anchor: &Anchor) -> bool {
    anchor_resolves_in_blocks(&document.blocks, anchor)
}

pub(crate) fn anchor_resolves_in_blocks(blocks: &[Block], anchor: &Anchor) -> bool {
    match anchor {
        Anchor::Document => true,
        Anchor::NearestBlock { block_id, .. } => block_exists(blocks, block_id),
        Anchor::TextRange(range) => {
            let mut found_start = false;
            let mut found_end = false;
            for block in blocks {
                for inline in &block.content {
                    let id = inline_id(inline);
                    found_start |= id == &range.start;
                    found_end |= id == &range.end;
                }
                if let BlockKind::Table { rows, .. } = &block.kind {
                    for row in rows {
                        for cell in &row.cells {
                            found_start |= anchor_endpoint_resolves(&cell.blocks, &range.start);
                            found_end |= anchor_endpoint_resolves(&cell.blocks, &range.end);
                        }
                    }
                }
            }
            found_start && found_end
        }
    }
}

pub(crate) fn anchor_endpoint_resolves(blocks: &[Block], target: &StableId) -> bool {
    blocks.iter().any(|block| {
        block
            .content
            .iter()
            .any(|inline| inline_id(inline) == target)
            || matches!(&block.kind, BlockKind::Table { rows, .. } if rows.iter().any(|row| {
                row.cells
                    .iter()
                    .any(|cell| anchor_endpoint_resolves(&cell.blocks, target))
            }))
    })
}

pub(crate) fn nearest_block_anchor(document: &Document, warning: &str) -> Anchor {
    nearest_block_anchor_in_blocks(&document.blocks, warning)
}

pub(crate) fn nearest_block_anchor_in_blocks(blocks: &[Block], warning: &str) -> Anchor {
    if let Some(block) = blocks.first() {
        Anchor::NearestBlock {
            block_id: block.id.clone(),
            warning: warning.to_string(),
        }
    } else {
        Anchor::Document
    }
}
