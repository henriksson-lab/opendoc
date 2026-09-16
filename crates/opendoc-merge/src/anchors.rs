//! Anchor resolution and the repairs that keep anchors pointing at live content.

use crate::blocks::{block_exists, delete_would_empty_table_cell_in_blocks};
use crate::inline_ops::inline_id;
use crate::marks::editable_inline_ids;
use crate::suggestions::push_provenance_once;
use opendoc_core::{
    Anchor, Block, BlockKind, Document, Inline, ModelWarning, ParagraphStyle, StableId,
    SuggestionKind, SuggestionState, TextRange,
};

pub(crate) fn repair_comment_anchors(document: &mut Document, warnings: &mut Vec<ModelWarning>) {
    let mut degraded = Vec::new();
    for thread in &mut document.comments {
        if thread.deleted
            || matches!(&thread.anchor, Anchor::Orphaned { .. })
            || anchor_resolves_in_blocks(&document.blocks, &thread.anchor)
        {
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
    // The loop below skips every suggestion that is not `Proposed`, so with
    // none of them proposed it reads nothing and writes nothing. Saying that
    // *before* `editable_inline_ids` walks the document and clones an id per
    // inline is what keeps a merge into a document that has no live
    // suggestions in it off a whole-document pass whose answer it cannot use.
    // The condition is the loop's own vacuity, not a guess about when the
    // repair matters.
    if !document
        .suggestions
        .iter()
        .any(|suggestion| suggestion.state == SuggestionState::Proposed)
    {
        return;
    }
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
            SuggestionKind::Delete { range }
            | SuggestionKind::Format { range, .. }
            | SuggestionKind::FormatRemove { range, .. } => {
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
            // A format replacement is compare-and-set.  It must not collapse
            // to an endpoint, because that would apply a replacement to a
            // different reviewed set of inlines.
            SuggestionKind::FormatReplace { range, .. } => {
                if !editable_ids.iter().any(|id| id == &range.start)
                    || !editable_ids.iter().any(|id| id == &range.end)
                {
                    suggestion.state = SuggestionState::Rejected;
                    push_provenance_once(suggestion, "auto-rejected:missing-format-target");
                    emitted_warnings.push(ModelWarning {
                        code: "suggestion-range-missing".to_string(),
                        message: format!(
                            "suggestion {} was rejected because its format target was deleted",
                            suggestion.id
                        ),
                    });
                }
            }
            // A block-delete proposal remains meaningful only while that
            // exact identity survives.  Do not retarget it: accepting a
            // review proposal must never delete a neighbouring block.
            SuggestionKind::BlockDelete { block_id } => {
                if delete_would_empty_table_cell_in_blocks(&document.blocks, block_id) {
                    suggestion.state = SuggestionState::Rejected;
                    push_provenance_once(suggestion, "auto-rejected:table-cell-requires-block");
                    emitted_warnings.push(ModelWarning {
                        code: "table-cell-requires-block".to_string(),
                        message: format!(
                            "suggestion {} was rejected because its table cell requires one block",
                            suggestion.id
                        ),
                    });
                } else if !block_exists(&document.blocks, block_id) {
                    suggestion.state = SuggestionState::Rejected;
                    push_provenance_once(suggestion, "auto-rejected:missing-block");
                    emitted_warnings.push(ModelWarning {
                        code: "suggestion-block-missing".to_string(),
                        message: format!(
                            "suggestion {} was rejected because its block was deleted",
                            suggestion.id
                        ),
                    });
                }
            }
            SuggestionKind::BlockInsert { position, .. } => {
                // Review insertions retain their intended sibling identity.
                // Unlike ordinary operations they must not degrade to a
                // body append after concurrent deletion.
                if position
                    .anchor()
                    .is_some_and(|anchor| !block_exists(&document.blocks, anchor))
                {
                    suggestion.state = SuggestionState::Rejected;
                    push_provenance_once(suggestion, "auto-rejected:missing-block-anchor");
                    emitted_warnings.push(ModelWarning {
                        code: "suggestion-block-anchor-missing".to_string(),
                        message: format!(
                            "suggestion {} was rejected because its insertion anchor was deleted",
                            suggestion.id
                        ),
                    });
                }
            }
            SuggestionKind::BlockReplace {
                block_id, expected, ..
            } => {
                if !block_exists(&document.blocks, block_id) {
                    suggestion.state = SuggestionState::Rejected;
                    push_provenance_once(suggestion, "auto-rejected:missing-block");
                    emitted_warnings.push(ModelWarning {
                        code: "suggestion-block-missing".to_string(),
                        message: format!(
                            "suggestion {} was rejected because its block was deleted",
                            suggestion.id
                        ),
                    });
                } else if find_block(&document.blocks, block_id) != Some(expected) {
                    suggestion.state = SuggestionState::Rejected;
                    push_provenance_once(suggestion, "auto-rejected:source-block-mismatch");
                    emitted_warnings.push(ModelWarning {
                        code: "suggestion-block-source-changed".to_string(),
                        message: format!(
                            "suggestion {} was rejected because its source block changed",
                            suggestion.id
                        ),
                    });
                }
            }
            SuggestionKind::ParagraphStyleChange {
                block_id, expected, ..
            } => match find_block(&document.blocks, block_id).and_then(paragraph_style_of) {
                None if !block_exists(&document.blocks, block_id) => {
                    suggestion.state = SuggestionState::Rejected;
                    push_provenance_once(suggestion, "auto-rejected:missing-block");
                    emitted_warnings.push(ModelWarning {
                        code: "suggestion-paragraph-style-missing".to_string(),
                        message: format!(
                            "suggestion {} was rejected because its block was deleted",
                            suggestion.id
                        ),
                    });
                }
                None => {
                    suggestion.state = SuggestionState::Rejected;
                    push_provenance_once(suggestion, "auto-rejected:ineligible-block-style");
                    emitted_warnings.push(ModelWarning {
                        code: "suggestion-paragraph-style-ineligible".to_string(),
                        message: format!(
                            "suggestion {} was rejected because its block is no longer eligible for a paragraph style change",
                            suggestion.id
                        ),
                    });
                }
                Some(current) if current != *expected => {
                    suggestion.state = SuggestionState::Rejected;
                    push_provenance_once(suggestion, "auto-rejected:source-style-mismatch");
                    emitted_warnings.push(ModelWarning {
                        code: "suggestion-paragraph-style-mismatch".to_string(),
                        message: format!(
                            "suggestion {} was rejected because its source paragraph style changed",
                            suggestion.id
                        ),
                    });
                }
                Some(_) => {}
            },
            // Link edits are whole-inline proposals.  They cannot collapse
            // to a neighbouring text run after their target is removed.
            SuggestionKind::LinkChange { inline_id, .. } => {
                if !editable_ids.iter().any(|id| id == inline_id) {
                    suggestion.state = SuggestionState::Rejected;
                    push_provenance_once(suggestion, "auto-rejected:missing-link-target");
                    emitted_warnings.push(ModelWarning {
                        code: "suggestion-link-target-missing".to_string(),
                        message: format!(
                            "suggestion {} was rejected because its link target was deleted",
                            suggestion.id
                        ),
                    });
                }
            }
        }
    }

    warnings.extend(emitted_warnings);
}

fn paragraph_style_of(block: &Block) -> Option<ParagraphStyle> {
    match &block.kind {
        BlockKind::Paragraph => Some(ParagraphStyle::Paragraph),
        BlockKind::Title => Some(ParagraphStyle::Title),
        BlockKind::Subtitle => Some(ParagraphStyle::Subtitle),
        BlockKind::Heading { level } => Some(ParagraphStyle::Heading { level: *level }),
        _ => None,
    }
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
        Anchor::Orphaned { .. } => false,
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

/// Whether an anchor has lost every meaningful target.  A partially deleted
/// text range deliberately remains repairable by collapsing it to its live
/// endpoint; only this fully-missing case becomes durable orphan evidence.
pub(crate) fn anchor_has_no_surviving_target(blocks: &[Block], anchor: &Anchor) -> bool {
    match anchor {
        Anchor::TextRange(range) => {
            !anchor_endpoint_resolves(blocks, &range.start)
                && !anchor_endpoint_resolves(blocks, &range.end)
        }
        Anchor::NearestBlock { block_id, .. } => !block_exists(blocks, block_id),
        Anchor::Orphaned { .. } => true,
        Anchor::Document => false,
    }
}

/// Snapshot a live comment anchor before an operation mutates the document.
/// The evidence is deliberately plain text: it is a review label, not a
/// second rich-text fragment with independent formatting semantics.
pub(crate) fn comment_anchor_evidence(
    blocks: &[Block],
    anchor: &Anchor,
) -> Option<(String, String)> {
    match anchor {
        Anchor::TextRange(range) => text_range_evidence(blocks, range),
        Anchor::NearestBlock { block_id, .. } => find_block(blocks, block_id).and_then(|block| {
            let context = block_text(block);
            (!context.is_empty()).then(|| (context.clone(), context))
        }),
        Anchor::Orphaned { .. } | Anchor::Document => None,
    }
}

fn text_range_evidence(blocks: &[Block], range: &TextRange) -> Option<(String, String)> {
    let (start, start_context) = find_inline_with_context(blocks, &range.start)?;
    let (end, end_context) = find_inline_with_context(blocks, &range.end)?;
    let quote = if range.start == range.end {
        inline_text(start)
    } else {
        format!("{} … {}", inline_text(start), inline_text(end))
    };
    let context = if start_context == end_context {
        start_context
    } else {
        format!("{} … {}", start_context, end_context)
    };
    (!quote.trim().is_empty() && !context.trim().is_empty()).then_some((quote, context))
}

fn find_block<'a>(blocks: &'a [Block], target: &StableId) -> Option<&'a Block> {
    blocks.iter().find_map(|block| {
        if &block.id == target {
            Some(block)
        } else if let BlockKind::Table { rows, .. } = &block.kind {
            rows.iter().find_map(|row| {
                row.cells
                    .iter()
                    .find_map(|cell| find_block(&cell.blocks, target))
            })
        } else {
            None
        }
    })
}

fn find_inline_with_context<'a>(
    blocks: &'a [Block],
    target: &StableId,
) -> Option<(&'a Inline, String)> {
    for block in blocks {
        if let Some(inline) = block
            .content
            .iter()
            .find(|inline| inline_id(inline) == target)
        {
            return Some((inline, block_text(block)));
        }
        if let BlockKind::Table { rows, .. } = &block.kind {
            for row in rows {
                for cell in &row.cells {
                    if let Some(found) = find_inline_with_context(&cell.blocks, target) {
                        return Some(found);
                    }
                }
            }
        }
    }
    None
}

fn block_text(block: &Block) -> String {
    block
        .content
        .iter()
        .map(inline_text)
        .collect::<Vec<_>>()
        .join("")
}

fn inline_text(inline: &Inline) -> String {
    match inline {
        Inline::Text { text, .. } | Inline::Link { text, .. } => text.clone(),
        Inline::Citation {
            rendered_cache,
            citation_id,
            ..
        } => rendered_cache
            .clone()
            .unwrap_or_else(|| format!("[{citation_id}]")),
        Inline::Mention { label, .. }
        | Inline::GooglePersonChip { label, .. }
        | Inline::GoogleRichLinkChip { label, .. } => label.clone(),
        Inline::Dropdown {
            options,
            selected_option_id,
            ..
        } => options
            .iter()
            .find(|option| option.id == *selected_option_id)
            .map(|option| option.label.clone())
            .unwrap_or_else(|| "[dropdown]".to_string()),
        Inline::DateChip { date, .. } => date.clone(),
        Inline::Equation { equation, .. } => equation.source.clone(),
        Inline::FootnoteRef { footnote_id, .. } => format!("[footnote: {footnote_id}]"),
        Inline::PageNumber { .. } => "[page number]".to_string(),
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
