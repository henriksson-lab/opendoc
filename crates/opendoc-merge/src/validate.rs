//! Payload validation: what a merge refuses to write into a document.

use crate::inline_ops::inline_id;
use opendoc_core::{
    Block, BlockKind, CommentThread, HashRef, Inline, Mark, MarkKind, ModelWarning, StableId,
    Suggestion, SuggestionKind, TableCell, TableRow,
};

pub(crate) fn insert_block_payload_valid_for_merge(
    block: &Block,
    warnings: &mut Vec<ModelWarning>,
) -> bool {
    match &block.kind {
        BlockKind::Heading { level } if !(1..=6).contains(level) => {
            warnings.push(ModelWarning {
                code: "invalid-heading-level".to_string(),
                message: format!(
                    "inserted heading block {} ignored invalid level {}",
                    block.id, level
                ),
            });
            return false;
        }
        BlockKind::ListItem { level, .. } if *level > 8 => {
            warnings.push(ModelWarning {
                code: "invalid-list-level".to_string(),
                message: format!(
                    "inserted list item block {} ignored invalid level {}",
                    block.id, level
                ),
            });
            return false;
        }
        BlockKind::EquationBlock { equation } if equation.source.trim().is_empty() => {
            warnings.push(ModelWarning {
                code: "invalid-block-equation-source".to_string(),
                message: format!("inserted equation block {} ignored empty source", block.id),
            });
            return false;
        }
        BlockKind::Image { blob_hash, .. } if HashRef::parse(blob_hash).is_err() => {
            warnings.push(ModelWarning {
                code: "invalid-image-blob-hash".to_string(),
                message: format!(
                    "inserted image block {} ignored invalid blob hash",
                    block.id
                ),
            });
            return false;
        }
        BlockKind::Table { rows, .. } => {
            if rows.is_empty() {
                warnings.push(ModelWarning {
                    code: "invalid-table".to_string(),
                    message: format!("inserted table block {} ignored empty rows", block.id),
                });
                return false;
            }
            for row in rows {
                if !table_row_payload_valid_for_merge(row, warnings) {
                    return false;
                }
            }
        }
        _ => {}
    }

    for inline in &block.content {
        if !inline_payload_valid_for_merge(
            inline,
            warnings,
            &format!("inserted block {}", block.id),
        ) {
            return false;
        }
    }

    true
}

pub(crate) fn inline_payload_valid_for_merge(
    inline: &Inline,
    warnings: &mut Vec<ModelWarning>,
    owner: &str,
) -> bool {
    match inline {
        Inline::Text { id, marks, .. } => marks_valid_for_merge(marks, warnings, owner, id),
        Inline::Link {
            id, href, marks, ..
        } => {
            if href.trim().is_empty() {
                warnings.push(ModelWarning {
                    code: "invalid-link-href".to_string(),
                    message: format!("{owner} ignored empty link href on inline {id}"),
                });
                return false;
            }
            marks_valid_for_merge(marks, warnings, owner, id)
        }
        Inline::Mention { id, label } if label.trim().is_empty() => {
            warnings.push(ModelWarning {
                code: "invalid-mention-label".to_string(),
                message: format!("{owner} ignored empty mention label on inline {id}"),
            });
            false
        }
        Inline::GooglePersonChip {
            id, label, email, ..
        } if label.trim().is_empty() || email.trim().is_empty() => {
            warnings.push(ModelWarning {
                code: "invalid-google-person-chip".to_string(),
                message: format!(
                    "{owner} ignored Google person chip with empty label or email on inline {id}"
                ),
            });
            false
        }
        Inline::GoogleRichLinkChip {
            id, label, href, ..
        } if label.trim().is_empty() || href.trim().is_empty() => {
            warnings.push(ModelWarning {
                code: "invalid-google-rich-link-chip".to_string(),
                message: format!(
                    "{owner} ignored Google rich link chip with empty label or href on inline {id}"
                ),
            });
            false
        }
        Inline::Equation { id, equation } if equation.source.trim().is_empty() => {
            warnings.push(ModelWarning {
                code: "invalid-inline-equation-source".to_string(),
                message: format!("{owner} ignored empty equation source on inline {id}"),
            });
            false
        }
        _ => true,
    }
}

pub(crate) fn inline_sequence_payload_valid_for_merge(
    inlines: &[Inline],
    warnings: &mut Vec<ModelWarning>,
    owner: &str,
) -> bool {
    for inline in inlines {
        if !inline_payload_valid_for_merge(inline, warnings, owner) {
            return false;
        }
    }
    true
}

pub(crate) fn inline_sequence_is_empty_source_text(inlines: &[Inline]) -> bool {
    inlines.iter().all(|inline| match inline {
        Inline::Text { text, .. } | Inline::Link { text, .. } => text.trim().is_empty(),
        Inline::Mention { .. }
        | Inline::GooglePersonChip { .. }
        | Inline::GoogleRichLinkChip { .. }
        | Inline::Dropdown { .. }
        | Inline::DateChip { .. }
        | Inline::Equation { .. }
        | Inline::Citation { .. }
        | Inline::FootnoteRef { .. }
        | Inline::PageNumber { .. } => false,
    })
}

pub(crate) fn suggestion_insert_content_is_empty(suggestion: &Suggestion) -> bool {
    match &suggestion.kind {
        SuggestionKind::Insert { content, .. } => inline_sequence_is_empty_source_text(content),
        SuggestionKind::Delete { .. }
        | SuggestionKind::Format { .. }
        | SuggestionKind::FormatRemove { .. }
        | SuggestionKind::FormatReplace { .. }
        | SuggestionKind::LinkChange { .. }
        | SuggestionKind::BlockDelete { .. }
        | SuggestionKind::BlockInsert { .. }
        | SuggestionKind::BlockReplace { .. }
        | SuggestionKind::ParagraphStyleChange { .. } => false,
    }
}

pub(crate) fn comment_thread_has_empty_body(thread: &CommentThread) -> bool {
    thread
        .comments
        .iter()
        .any(|comment| !comment.deleted && inline_sequence_is_empty_source_text(&comment.body))
}

pub(crate) fn table_row_payload_valid_for_merge(
    row: &TableRow,
    warnings: &mut Vec<ModelWarning>,
) -> bool {
    if row.cells.is_empty() {
        warnings.push(ModelWarning {
            code: "invalid-table-row".to_string(),
            message: format!("inserted table row {} ignored empty cells", row.id),
        });
        return false;
    }
    row.cells
        .iter()
        .all(|cell| table_cell_payload_valid_for_merge(cell, warnings))
}

pub(crate) fn table_cell_payload_valid_for_merge(
    cell: &TableCell,
    warnings: &mut Vec<ModelWarning>,
) -> bool {
    if cell.blocks.is_empty() {
        warnings.push(ModelWarning {
            code: "invalid-table-cell".to_string(),
            message: format!("inserted table cell {} ignored empty blocks", cell.id),
        });
        return false;
    }
    cell.blocks
        .iter()
        .all(|block| insert_block_payload_valid_for_merge(block, warnings))
}

pub(crate) fn marks_valid_for_merge(
    marks: &[Mark],
    warnings: &mut Vec<ModelWarning>,
    owner: &str,
    inline_id: &StableId,
) -> bool {
    for mark in marks {
        let needs_value = matches!(
            mark.kind,
            MarkKind::Color | MarkKind::Background | MarkKind::Font | MarkKind::Size
        );
        match (&mark.value, needs_value) {
            (Some(value), true) if value.trim().is_empty() => {
                warnings.push(ModelWarning {
                    code: "invalid-mark-value".to_string(),
                    message: format!("{owner} ignored empty mark value on inline {inline_id}"),
                });
                return false;
            }
            (None, true) => {
                warnings.push(ModelWarning {
                    code: "invalid-mark-value".to_string(),
                    message: format!("{owner} ignored missing mark value on inline {inline_id}"),
                });
                return false;
            }
            (Some(_), false) => {
                warnings.push(ModelWarning {
                    code: "invalid-mark-value".to_string(),
                    message: format!("{owner} ignored boolean mark value on inline {inline_id}"),
                });
                return false;
            }
            _ => {}
        }
    }
    true
}

pub(crate) fn mark_removal_valid_for_merge(
    kind: &MarkKind,
    value: Option<&str>,
    warnings: &mut Vec<ModelWarning>,
    owner: &str,
    inline_id: &StableId,
) -> bool {
    let supports_value = matches!(
        kind,
        MarkKind::Color | MarkKind::Background | MarkKind::Font | MarkKind::Size
    );
    match (value, supports_value) {
        (Some(value), true) if value.trim().is_empty() => {
            warnings.push(ModelWarning {
                code: "invalid-mark-value".to_string(),
                message: format!("{owner} ignored empty mark value on inline {inline_id}"),
            });
            false
        }
        (Some(_), false) => {
            warnings.push(ModelWarning {
                code: "invalid-mark-value".to_string(),
                message: format!("{owner} ignored boolean mark value on inline {inline_id}"),
            });
            false
        }
        _ => true,
    }
}

pub(crate) fn inline_exists(blocks: &[Block], inline_id_to_find: &StableId) -> bool {
    blocks.iter().any(|block| {
        block
            .content
            .iter()
            .any(|inline| inline_id(inline) == inline_id_to_find)
            || matches!(&block.kind, BlockKind::Table { rows, .. } if rows.iter().any(|row| {
                row.cells
                    .iter()
                    .any(|cell| inline_exists(&cell.blocks, inline_id_to_find))
            }))
    })
}
