//! Accepting and rejecting tracked-change suggestions.

use crate::blocks::{
    block_exists, delete_block, delete_would_empty_table_cell, find_block_mut, insert_block,
    replace_block,
};
use crate::inline_ops::{
    delete_inline_range, insert_inlines_after_anchor, AnchorInsertResult, RangeEditResult,
};
use crate::marks::{
    add_mark_range, remove_mark_range, replace_mark_range_if_expected, MarkRangeResult,
    MarkReplaceRangeResult,
};
use crate::validate::mark_removal_valid_for_merge;
use crate::validate::{inline_sequence_payload_valid_for_merge, marks_valid_for_merge};
use opendoc_core::{
    Anchor, BlockKind, Document, Inline, Mark, ModelWarning, ParagraphStyle, StableId, Suggestion,
    SuggestionKind, SuggestionState,
};

/// The two hypothetical review decisions a caller may render without changing
/// the document it owns.
///
/// This is deliberately not an operation.  A preview must not acquire an
/// operation id, provenance, or a place in the journal merely because somebody
/// opened it in a review pane.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SuggestionPreviewResolution {
    Accept,
    Reject,
}

/// Project one pending suggestion as though it were resolved, without mutating
/// `document`.
///
/// The accept path calls the same implementation as `AcceptSuggestion`; this
/// is important for structural proposals, whose validity depends on the live
/// sibling/target identities.  The returned warnings consequently describe
/// the exact current-state degradation the real operation would take.  The
/// synthetic reviewer is confined to the discarded clone and is never exposed
/// by the application preview API.
pub fn preview_suggestion_resolution(
    document: &Document,
    suggestion_id: &StableId,
    resolution: SuggestionPreviewResolution,
) -> (Document, Vec<ModelWarning>) {
    let mut document = document.clone();
    let mut warnings = Vec::new();
    match resolution {
        SuggestionPreviewResolution::Accept => {
            accept_suggestion(&mut document, &mut warnings, suggestion_id, "preview");
        }
        SuggestionPreviewResolution::Reject => {
            let Some(suggestion) = document
                .suggestions
                .iter_mut()
                .find(|item| &item.id == suggestion_id)
            else {
                warnings.push(ModelWarning {
                    code: "missing-suggestion".to_string(),
                    message: format!("suggestion {suggestion_id} was missing"),
                });
                return (document, warnings);
            };
            if suggestion.state != SuggestionState::Proposed {
                warnings.push(ModelWarning {
                    code: "resolved-suggestion".to_string(),
                    message: format!("suggestion {suggestion_id} was already resolved"),
                });
            } else {
                suggestion.state = SuggestionState::Rejected;
            }
        }
    }
    (document, warnings)
}

pub(crate) fn push_provenance_once(suggestion: &mut Suggestion, value: &str) {
    if !suggestion.provenance.iter().any(|item| item == value) {
        suggestion.provenance.push(value.to_string());
    }
}

pub(crate) fn reviewer_provenance_value(
    warnings: &mut Vec<ModelWarning>,
    suggestion_id: &StableId,
    action: &str,
    reviewer: &str,
) -> String {
    let trimmed = reviewer.trim();
    if trimmed.is_empty() || trimmed != reviewer {
        warnings.push(ModelWarning {
            code: "invalid-suggestion-reviewer".to_string(),
            message: format!(
                "suggestion {suggestion_id} {action} reviewer was invalid; recorded unknown reviewer"
            ),
        });
        "unknown".to_string()
    } else {
        reviewer.to_string()
    }
}

pub(crate) fn canonical_diagnostic_value(value: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        "<empty>".to_string()
    } else {
        value.to_string()
    }
}

pub(crate) fn accept_suggestion(
    document: &mut Document,
    warnings: &mut Vec<ModelWarning>,
    suggestion_id: &StableId,
    accepted_by: &str,
) {
    let accepted_by = reviewer_provenance_value(warnings, suggestion_id, "accept", accepted_by);
    let Some(index) = document
        .suggestions
        .iter()
        .position(|item| &item.id == suggestion_id)
    else {
        warnings.push(ModelWarning {
            code: "missing-suggestion".to_string(),
            message: format!("suggestion {suggestion_id} was missing"),
        });
        return;
    };

    if document.suggestions[index].state != SuggestionState::Proposed {
        warnings.push(ModelWarning {
            code: "resolved-suggestion".to_string(),
            message: format!("suggestion {suggestion_id} was already resolved"),
        });
        return;
    }

    let kind = document.suggestions[index].kind.clone();
    let accepted = match kind {
        SuggestionKind::Insert { anchor, content } => {
            accept_insert_suggestion(document, warnings, suggestion_id, &anchor, content)
        }
        SuggestionKind::Delete { range } => {
            accept_delete_suggestion(document, warnings, suggestion_id, &range);
            true
        }
        SuggestionKind::Format { range, marks } => {
            accept_format_suggestion(document, warnings, suggestion_id, &range, marks)
        }
        SuggestionKind::FormatRemove { range, kind, value } => accept_format_removal_suggestion(
            document,
            warnings,
            suggestion_id,
            &range,
            &kind,
            value.as_deref(),
        ),
        SuggestionKind::FormatReplace {
            range,
            kind,
            expected_value,
            value,
        } => accept_format_replacement_suggestion(
            document,
            warnings,
            suggestion_id,
            &range,
            &kind,
            &expected_value,
            &value,
        ),
        SuggestionKind::LinkChange {
            inline_id,
            expected_href,
            href,
        } => accept_link_change_suggestion(
            document,
            warnings,
            suggestion_id,
            &inline_id,
            expected_href.as_deref(),
            href,
        ),
        SuggestionKind::BlockDelete { block_id } => {
            if delete_would_empty_table_cell(document, &block_id) {
                warnings.push(ModelWarning {
                    code: "table-cell-requires-block".to_string(),
                    message: format!(
                        "suggestion {suggestion_id} could not delete block {block_id} because its table cell requires one block"
                    ),
                });
                false
            } else if !delete_block(document, &block_id) {
                warnings.push(ModelWarning {
                    code: "suggestion-block-missing".to_string(),
                    message: format!(
                        "suggestion {suggestion_id} had no surviving block {block_id} to delete"
                    ),
                });
                false
            } else {
                true
            }
        }
        SuggestionKind::BlockInsert { position, block } => {
            accept_block_insert_suggestion(document, warnings, suggestion_id, position, block)
        }
        SuggestionKind::BlockReplace {
            block_id,
            expected,
            replacement,
        } => accept_block_replace_suggestion(
            document,
            warnings,
            suggestion_id,
            &block_id,
            &expected,
            *replacement,
        ),
        SuggestionKind::ParagraphStyleChange {
            block_id,
            expected,
            proposed,
        } => accept_paragraph_style_suggestion(
            document,
            warnings,
            suggestion_id,
            &block_id,
            expected,
            proposed,
        ),
    };

    let suggestion = &mut document.suggestions[index];
    if accepted {
        suggestion.state = SuggestionState::Accepted;
        push_provenance_once(suggestion, &format!("accepted-by:{accepted_by}"));
    } else {
        suggestion.state = SuggestionState::Rejected;
        suggestion.kind = SuggestionKind::Insert {
            anchor: Anchor::Document,
            content: vec![Inline::text("[invalid suggestion payload]")],
        };
        push_provenance_once(suggestion, "auto-rejected:invalid-accept-payload");
    }
}

fn accept_paragraph_style_suggestion(
    document: &mut Document,
    warnings: &mut Vec<ModelWarning>,
    suggestion_id: &StableId,
    block_id: &StableId,
    expected: ParagraphStyle,
    proposed: ParagraphStyle,
) -> bool {
    let Some(block) = find_block_mut(&mut document.blocks, block_id) else {
        warnings.push(ModelWarning {
            code: "suggestion-paragraph-style-missing".to_string(),
            message: format!("suggestion {suggestion_id} target block {block_id} no longer exists"),
        });
        return false;
    };
    let current = match &block.kind {
        BlockKind::Paragraph => ParagraphStyle::Paragraph,
        BlockKind::Title => ParagraphStyle::Title,
        BlockKind::Subtitle => ParagraphStyle::Subtitle,
        BlockKind::Heading { level } => ParagraphStyle::Heading { level: *level },
        _ => {
            warnings.push(ModelWarning { code: "suggestion-paragraph-style-ineligible".to_string(), message: format!("suggestion {suggestion_id} target block {block_id} is not a non-list text block") });
            return false;
        }
    };
    if current != expected {
        warnings.push(ModelWarning {
            code: "suggestion-paragraph-style-mismatch".to_string(),
            message: format!(
                "suggestion {suggestion_id} expected a different source style for block {block_id}"
            ),
        });
        return false;
    }
    block.kind = match proposed {
        ParagraphStyle::Paragraph => BlockKind::Paragraph,
        ParagraphStyle::Title => BlockKind::Title,
        ParagraphStyle::Subtitle => BlockKind::Subtitle,
        ParagraphStyle::Heading { level } => BlockKind::Heading { level },
    };
    true
}

/// Apply a link proposal only to the exact source inline and only if its link
/// state still matches the value the author reviewed.  Unlike range formatting
/// this intentionally has no degraded endpoint behaviour: a link is an atomic
/// inline property, not a caret/span mark.
fn accept_link_change_suggestion(
    document: &mut Document,
    warnings: &mut Vec<ModelWarning>,
    suggestion_id: &StableId,
    inline_id: &StableId,
    expected_href: Option<&str>,
    href: Option<String>,
) -> bool {
    let Some(result) = update_link_in_blocks(&mut document.blocks, inline_id, expected_href, href)
    else {
        warnings.push(ModelWarning {
            code: "suggestion-link-target-missing".to_string(),
            message: format!(
                "suggestion {suggestion_id} target inline {inline_id} no longer exists"
            ),
        });
        return false;
    };
    if !result {
        warnings.push(ModelWarning {
            code: "suggestion-link-target-changed".to_string(),
            message: format!("suggestion {suggestion_id} target inline {inline_id} no longer has the proposed source link state"),
        });
        return false;
    }
    true
}

/// `None` is no matching inline; `Some(false)` is a surviving but changed or
/// non-text target; `Some(true)` applied the exact proposal.
fn update_link_in_blocks(
    blocks: &mut [opendoc_core::Block],
    target: &StableId,
    expected_href: Option<&str>,
    href: Option<String>,
) -> Option<bool> {
    for block in blocks {
        for inline in &mut block.content {
            let replacement = match inline {
                Inline::Text { id, text, marks } if id == target => {
                    if expected_href.is_some() {
                        return Some(false);
                    }
                    href.as_ref().map(|href| Inline::Link {
                        id: id.clone(),
                        text: text.clone(),
                        href: href.clone(),
                        marks: marks.clone(),
                    })
                }
                Inline::Link {
                    id,
                    text,
                    href: current,
                    marks,
                } if id == target => {
                    if expected_href != Some(current.as_str()) {
                        return Some(false);
                    }
                    Some(match href {
                        Some(href) => Inline::Link {
                            id: id.clone(),
                            text: text.clone(),
                            href,
                            marks: marks.clone(),
                        },
                        None => Inline::Text {
                            id: id.clone(),
                            text: text.clone(),
                            marks: marks.clone(),
                        },
                    })
                }
                _ => continue,
            };
            let Some(replacement) = replacement else {
                return Some(false);
            };
            *inline = replacement;
            return Some(true);
        }
        if let opendoc_core::BlockKind::Table { rows, .. } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    if let Some(result) =
                        update_link_in_blocks(&mut cell.blocks, target, expected_href, href.clone())
                    {
                        return Some(result);
                    }
                }
            }
        }
    }
    None
}

fn accept_block_insert_suggestion(
    document: &mut Document,
    warnings: &mut Vec<ModelWarning>,
    suggestion_id: &StableId,
    position: opendoc_core::InsertPosition,
    block: opendoc_core::Block,
) -> bool {
    if block_exists(&document.blocks, &block.id) {
        warnings.push(ModelWarning {
            code: "suggestion-block-duplicate".to_string(),
            message: format!(
                "suggestion {suggestion_id} could not insert duplicate block {}",
                block.id
            ),
        });
        return false;
    }
    // Ordinary InsertBlock intentionally degrades a vanished anchor to the
    // end of the body. A review proposal cannot do that: it would make a
    // reviewer accept content at a location they never saw.
    if position
        .anchor()
        .is_some_and(|anchor| !block_exists(&document.blocks, anchor))
    {
        warnings.push(ModelWarning {
            code: "suggestion-block-anchor-missing".to_string(),
            message: format!(
                "suggestion {suggestion_id} was rejected because its insertion anchor was deleted"
            ),
        });
        return false;
    }
    let degraded = insert_block(document, position, block);
    debug_assert!(
        !degraded,
        "a checked review insertion anchor must remain present"
    );
    true
}

fn accept_block_replace_suggestion(
    document: &mut Document,
    warnings: &mut Vec<ModelWarning>,
    suggestion_id: &StableId,
    block_id: &StableId,
    expected: &opendoc_core::Block,
    replacement: opendoc_core::Block,
) -> bool {
    let Some(current) = find_block_in_blocks(&document.blocks, block_id) else {
        warnings.push(ModelWarning {
            code: "suggestion-block-missing".to_string(),
            message: format!(
                "suggestion {suggestion_id} had no surviving block {block_id} to replace"
            ),
        });
        return false;
    };
    if current != expected {
        warnings.push(ModelWarning {
            code: "suggestion-block-source-changed".to_string(),
            message: format!(
                "suggestion {suggestion_id} source block {block_id} changed before review"
            ),
        });
        return false;
    }
    if &replacement.id != block_id && block_exists(&document.blocks, &replacement.id) {
        warnings.push(ModelWarning {
            code: "suggestion-block-duplicate".to_string(),
            message: format!(
                "suggestion {suggestion_id} could not replace with duplicate block {}",
                replacement.id
            ),
        });
        return false;
    }
    replace_block(document, block_id, replacement)
}

fn find_block_in_blocks<'a>(
    blocks: &'a [opendoc_core::Block],
    target: &StableId,
) -> Option<&'a opendoc_core::Block> {
    blocks.iter().find_map(|block| {
        if &block.id == target {
            Some(block)
        } else if let BlockKind::Table { rows, .. } = &block.kind {
            rows.iter().find_map(|row| {
                row.cells
                    .iter()
                    .find_map(|cell| find_block_in_blocks(&cell.blocks, target))
            })
        } else {
            None
        }
    })
}

pub(crate) fn accept_format_removal_suggestion(
    document: &mut Document,
    warnings: &mut Vec<ModelWarning>,
    suggestion_id: &StableId,
    range: &opendoc_core::TextRange,
    kind: &opendoc_core::MarkKind,
    value: Option<&str>,
) -> bool {
    if !mark_removal_valid_for_merge(
        kind,
        value,
        warnings,
        &format!("suggestion {suggestion_id} acceptance"),
        &range.start,
    ) {
        return false;
    }
    match remove_mark_range(document, range, kind, value) {
        MarkRangeResult::Applied => {}
        MarkRangeResult::Degraded => warnings.push(ModelWarning {
            code: "suggestion-range-degraded".to_string(),
            message: format!(
                "suggestion {suggestion_id} removed formatting from surviving range endpoint"
            ),
        }),
        MarkRangeResult::Missing => warnings.push(ModelWarning {
            code: "suggestion-range-missing".to_string(),
            message: format!(
                "suggestion {suggestion_id} had no surviving range to remove formatting"
            ),
        }),
    }
    true
}

fn accept_format_replacement_suggestion(
    document: &mut Document,
    warnings: &mut Vec<ModelWarning>,
    suggestion_id: &StableId,
    range: &opendoc_core::TextRange,
    kind: &opendoc_core::MarkKind,
    expected_value: &str,
    value: &str,
) -> bool {
    if (opendoc_core::Suggestion {
        id: suggestion_id.clone(),
        author: "validation".to_string(),
        kind: SuggestionKind::FormatReplace {
            range: range.clone(),
            kind: kind.clone(),
            expected_value: expected_value.to_string(),
            value: value.to_string(),
        },
        state: SuggestionState::Proposed,
        provenance: Vec::new(),
    })
    .validate()
    .is_err()
    {
        warnings.push(ModelWarning {
            code: "invalid-mark-value".to_string(),
            message: format!(
                "suggestion {suggestion_id} has an invalid format replacement payload"
            ),
        });
        return false;
    }
    match replace_mark_range_if_expected(document, range, kind, expected_value, value) {
        MarkReplaceRangeResult::Applied => true,
        MarkReplaceRangeResult::Missing => {
            warnings.push(ModelWarning {
                code: "suggestion-range-missing".to_string(),
                message: format!(
                    "suggestion {suggestion_id} had no surviving range to replace formatting"
                ),
            });
            false
        }
        MarkReplaceRangeResult::Changed => {
            warnings.push(ModelWarning {
                code: "suggestion-format-precondition-failed".to_string(),
                message: format!(
                    "suggestion {suggestion_id} source formatting changed before review"
                ),
            });
            false
        }
    }
}

pub(crate) fn accept_insert_suggestion(
    document: &mut Document,
    warnings: &mut Vec<ModelWarning>,
    suggestion_id: &StableId,
    anchor: &Anchor,
    content: Vec<Inline>,
) -> bool {
    if content.is_empty() {
        warnings.push(ModelWarning {
            code: "invalid-suggestion".to_string(),
            message: format!("suggestion {suggestion_id} ignored empty insert content"),
        });
        return false;
    }
    if !inline_sequence_payload_valid_for_merge(
        &content,
        warnings,
        &format!("suggestion {suggestion_id} acceptance"),
    ) {
        return false;
    }
    match insert_inlines_after_anchor(document, anchor, content) {
        AnchorInsertResult::Applied => {}
        AnchorInsertResult::Degraded => warnings.push(ModelWarning {
            code: "suggestion-anchor-degraded".to_string(),
            message: format!("suggestion {suggestion_id} inserted at degraded anchor"),
        }),
        AnchorInsertResult::Missing => warnings.push(ModelWarning {
            code: "suggestion-anchor-degraded".to_string(),
            message: format!("suggestion {suggestion_id} could not find an insertion anchor"),
        }),
    }
    true
}

pub(crate) fn accept_delete_suggestion(
    document: &mut Document,
    warnings: &mut Vec<ModelWarning>,
    suggestion_id: &StableId,
    range: &opendoc_core::TextRange,
) {
    match delete_inline_range(document, range) {
        RangeEditResult::Applied => {}
        RangeEditResult::Degraded => warnings.push(ModelWarning {
            code: "suggestion-range-degraded".to_string(),
            message: format!("suggestion {suggestion_id} deleted surviving range endpoint"),
        }),
        RangeEditResult::Missing => warnings.push(ModelWarning {
            code: "suggestion-range-missing".to_string(),
            message: format!("suggestion {suggestion_id} had no surviving range to delete"),
        }),
    }
}

pub(crate) fn accept_format_suggestion(
    document: &mut Document,
    warnings: &mut Vec<ModelWarning>,
    suggestion_id: &StableId,
    range: &opendoc_core::TextRange,
    marks: Vec<Mark>,
) -> bool {
    if marks.is_empty() {
        warnings.push(ModelWarning {
            code: "invalid-suggestion".to_string(),
            message: format!("suggestion {suggestion_id} ignored empty format marks"),
        });
        return false;
    }
    for mark in marks {
        if !marks_valid_for_merge(
            std::slice::from_ref(&mark),
            warnings,
            &format!("suggestion {suggestion_id} acceptance"),
            &range.start,
        ) {
            return false;
        }
        match add_mark_range(document, range, mark) {
            MarkRangeResult::Applied => {}
            MarkRangeResult::Degraded => warnings.push(ModelWarning {
                code: "suggestion-range-degraded".to_string(),
                message: format!("suggestion {suggestion_id} formatted surviving range endpoint"),
            }),
            MarkRangeResult::Missing => warnings.push(ModelWarning {
                code: "suggestion-range-missing".to_string(),
                message: format!("suggestion {suggestion_id} had no surviving range to format"),
            }),
        }
    }
    true
}
