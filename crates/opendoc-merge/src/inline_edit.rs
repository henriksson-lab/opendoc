//! Inline content edits: text, links, equations, mention labels.

use crate::inline_ops::inline_id;
use opendoc_core::{Block, BlockKind, Document, Inline, StableId};

pub(crate) fn delete_inline(document: &mut Document, inline_id_to_delete: &StableId) -> bool {
    delete_inline_in_blocks(&mut document.blocks, inline_id_to_delete)
}

pub(crate) fn delete_inline_in_blocks(
    blocks: &mut [Block],
    inline_id_to_delete: &StableId,
) -> bool {
    for block in blocks {
        let before = block.content.len();
        block
            .content
            .retain(|item| inline_id(item) != inline_id_to_delete);
        if block.content.len() != before {
            return true;
        }
        if let BlockKind::Table { rows, .. } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    if delete_inline_in_blocks(&mut cell.blocks, inline_id_to_delete) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

pub(crate) fn update_inline_text(
    document: &mut Document,
    inline_id_to_update: &StableId,
    text: &str,
) -> Option<bool> {
    update_inline_text_in_blocks(&mut document.blocks, inline_id_to_update, text)
}

pub(crate) fn update_link_href(
    document: &mut Document,
    inline_id_to_update: &StableId,
    href: &str,
) -> Option<bool> {
    update_link_href_in_blocks(&mut document.blocks, inline_id_to_update, href)
}

pub(crate) fn update_inline_equation_source(
    document: &mut Document,
    inline_id_to_update: &StableId,
    source: &str,
) -> Option<bool> {
    update_inline_equation_source_in_blocks(&mut document.blocks, inline_id_to_update, source)
}

pub(crate) fn update_mention_label(
    document: &mut Document,
    inline_id_to_update: &StableId,
    label: &str,
) -> Option<bool> {
    update_mention_label_in_blocks(&mut document.blocks, inline_id_to_update, label)
}

/// Byte index of the `offset`-th Unicode scalar value, clamped to the end.
pub fn byte_index_for_char_offset(value: &str, offset: usize) -> usize {
    value
        .char_indices()
        .nth(offset)
        .map(|(index, _)| index)
        .unwrap_or(value.len())
}

pub(crate) fn edit_inline_text(
    document: &mut Document,
    inline_id_to_update: &StableId,
    edit: impl FnOnce(&mut String),
) -> Option<bool> {
    let mut edit = Some(edit);
    edit_inline_text_in_blocks(&mut document.blocks, inline_id_to_update, &mut edit)
}

pub(crate) fn edit_inline_text_in_blocks(
    blocks: &mut [Block],
    inline_id_to_update: &StableId,
    edit: &mut Option<impl FnOnce(&mut String)>,
) -> Option<bool> {
    for block in blocks {
        for inline in &mut block.content {
            match inline {
                Inline::Text {
                    id, text: value, ..
                }
                | Inline::Link {
                    id, text: value, ..
                } if id == inline_id_to_update => {
                    if let Some(edit) = edit.take() {
                        edit(value);
                    }
                    return Some(true);
                }
                Inline::Citation { id, .. }
                | Inline::FootnoteRef { id, .. }
                | Inline::Mention { id, .. }
                | Inline::Equation { id, .. }
                    if id == inline_id_to_update =>
                {
                    return Some(false);
                }
                _ => {}
            }
        }
        if let BlockKind::Table { rows, .. } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    if let Some(result) =
                        edit_inline_text_in_blocks(&mut cell.blocks, inline_id_to_update, edit)
                    {
                        return Some(result);
                    }
                }
            }
        }
    }
    None
}

pub(crate) fn update_inline_text_in_blocks(
    blocks: &mut [Block],
    inline_id_to_update: &StableId,
    text: &str,
) -> Option<bool> {
    for block in blocks {
        for inline in &mut block.content {
            match inline {
                Inline::Text {
                    id, text: value, ..
                } if id == inline_id_to_update => {
                    *value = text.to_string();
                    return Some(true);
                }
                Inline::Link {
                    id, text: value, ..
                } if id == inline_id_to_update => {
                    *value = text.to_string();
                    return Some(true);
                }
                Inline::Citation { id, .. }
                | Inline::FootnoteRef { id, .. }
                | Inline::Mention { id, .. }
                | Inline::Equation { id, .. }
                    if id == inline_id_to_update =>
                {
                    return Some(false);
                }
                _ => {}
            }
        }
        if let BlockKind::Table { rows, .. } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    if let Some(result) =
                        update_inline_text_in_blocks(&mut cell.blocks, inline_id_to_update, text)
                    {
                        return Some(result);
                    }
                }
            }
        }
    }
    None
}

pub(crate) fn update_mention_label_in_blocks(
    blocks: &mut [Block],
    inline_id_to_update: &StableId,
    label: &str,
) -> Option<bool> {
    for block in blocks {
        for inline in &mut block.content {
            match inline {
                Inline::Mention { id, label: value } if id == inline_id_to_update => {
                    *value = label.to_string();
                    return Some(true);
                }
                Inline::Text { id, .. }
                | Inline::Link { id, .. }
                | Inline::Citation { id, .. }
                | Inline::FootnoteRef { id, .. }
                | Inline::Equation { id, .. }
                    if id == inline_id_to_update =>
                {
                    return Some(false);
                }
                _ => {}
            }
        }
        if let BlockKind::Table { rows, .. } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    if let Some(updated) =
                        update_mention_label_in_blocks(&mut cell.blocks, inline_id_to_update, label)
                    {
                        return Some(updated);
                    }
                }
            }
        }
    }
    None
}

pub(crate) fn update_inline_equation_source_in_blocks(
    blocks: &mut [Block],
    inline_id_to_update: &StableId,
    source: &str,
) -> Option<bool> {
    for block in blocks {
        for inline in &mut block.content {
            match inline {
                Inline::Equation { id, equation } if id == inline_id_to_update => {
                    equation.source = source.to_string();
                    return Some(true);
                }
                Inline::Text { id, .. }
                | Inline::Link { id, .. }
                | Inline::Citation { id, .. }
                | Inline::FootnoteRef { id, .. }
                | Inline::Mention { id, .. }
                    if id == inline_id_to_update =>
                {
                    return Some(false);
                }
                _ => {}
            }
        }
        if let BlockKind::Table { rows, .. } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    if let Some(result) = update_inline_equation_source_in_blocks(
                        &mut cell.blocks,
                        inline_id_to_update,
                        source,
                    ) {
                        return Some(result);
                    }
                }
            }
        }
    }
    None
}

pub(crate) fn update_link_href_in_blocks(
    blocks: &mut [Block],
    inline_id_to_update: &StableId,
    href: &str,
) -> Option<bool> {
    for block in blocks {
        for inline in &mut block.content {
            match inline {
                Inline::Link {
                    id, href: value, ..
                } if id == inline_id_to_update => {
                    *value = href.to_string();
                    return Some(true);
                }
                Inline::Text { id, .. }
                | Inline::Citation { id, .. }
                | Inline::FootnoteRef { id, .. }
                | Inline::Mention { id, .. }
                | Inline::Equation { id, .. }
                    if id == inline_id_to_update =>
                {
                    return Some(false);
                }
                _ => {}
            }
        }
        if let BlockKind::Table { rows, .. } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    if let Some(result) =
                        update_link_href_in_blocks(&mut cell.blocks, inline_id_to_update, href)
                    {
                        return Some(result);
                    }
                }
            }
        }
    }
    None
}
