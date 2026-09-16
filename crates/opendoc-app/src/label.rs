//! Human-readable labels for anchors, ranges and suggestion payloads.

use super::*;

pub(crate) fn anchor_label(anchor: &Anchor) -> String {
    match anchor {
        Anchor::TextRange(range) => format!("{}..{}", range.start, range.end),
        Anchor::NearestBlock { block_id, .. } => format!("nearest:{block_id}"),
        Anchor::Orphaned { .. } => "orphaned".to_string(),
        Anchor::Document => "document".to_string(),
    }
}

pub(crate) fn anchor_display_label(anchor: &Anchor, blocks: &[Block]) -> String {
    match anchor {
        Anchor::TextRange(range) => range_display_label(range, blocks),
        Anchor::NearestBlock { block_id, .. } => find_block_in_blocks(blocks, block_id)
            .map(|block| format!("On: \"{}\"", truncate_label(&block_display_text(block), 60)))
            .unwrap_or_else(|| "On a removed block".to_string()),
        Anchor::Orphaned { quote, .. } => {
            format!("Deleted text: \"{}\"", truncate_label(quote, 80))
        }
        Anchor::Document => "Document".to_string(),
    }
}

pub(crate) fn suggestion_anchor_display_label(
    kind: &SuggestionKind,
    blocks: &[Block],
) -> Option<String> {
    match kind {
        SuggestionKind::Insert { anchor, .. } => Some(anchor_display_label(anchor, blocks)),
        SuggestionKind::Delete { range }
        | SuggestionKind::Format { range, .. }
        | SuggestionKind::FormatRemove { range, .. }
        | SuggestionKind::FormatReplace { range, .. } => Some(range_display_label(range, blocks)),
        SuggestionKind::BlockDelete { block_id } => find_block_in_blocks(blocks, block_id)
            .map(|block| {
                format!(
                    "Delete: \"{}\"",
                    truncate_label(&block_display_text(block), 60)
                )
            })
            .or_else(|| Some("Delete a removed block".to_string())),
        SuggestionKind::BlockInsert { position, .. } => Some(match position.anchor() {
            Some(anchor) => format!("Insert beside block {anchor}"),
            None => "Insert at document boundary".to_string(),
        }),
        SuggestionKind::BlockReplace { block_id, .. } => find_block_in_blocks(blocks, block_id)
            .map(|block| {
                format!(
                    "Replace: \"{}\"",
                    truncate_label(&block_display_text(block), 60)
                )
            })
            .or_else(|| Some("Replace a removed block".to_string())),
        SuggestionKind::ParagraphStyleChange { block_id, .. } => {
            find_block_in_blocks(blocks, block_id)
                .map(|block| {
                    format!(
                        "Style: \"{}\"",
                        truncate_label(&block_display_text(block), 60)
                    )
                })
                .or_else(|| Some("Change style on a removed block".to_string()))
        }
        SuggestionKind::LinkChange {
            inline_id, href, ..
        } => find_inline_in_blocks(blocks, inline_id)
            .map(|inline| {
                format!(
                    "{} link on: \"{}\"",
                    if href.is_some() { "Change" } else { "Remove" },
                    truncate_label(&inline_display_text(inline), 60)
                )
            })
            .or_else(|| Some("Change link on removed text".to_string())),
    }
}

fn range_display_label(range: &TextRange, blocks: &[Block]) -> String {
    let first = find_inline_in_blocks(blocks, &range.start);
    let last = find_inline_in_blocks(blocks, &range.end);
    let Some(first) = first else {
        return "On removed text".to_string();
    };
    let text = if inline_id(first) == &range.end {
        inline_display_text(first)
    } else {
        format!(
            "{} ... {}",
            inline_display_text(first),
            last.map(inline_display_text).unwrap_or_default()
        )
    };
    format!("\"{}\"", truncate_label(&text, 80))
}

fn block_display_text(block: &Block) -> String {
    block
        .content
        .iter()
        .map(inline_display_text)
        .collect::<Vec<_>>()
        .join("")
}

fn inline_display_text(inline: &Inline) -> String {
    match inline {
        Inline::Text { text, .. } | Inline::Link { text, .. } => text.clone(),
        Inline::Citation {
            rendered_cache: Some(text),
            ..
        } => text.clone(),
        Inline::Citation { citation_id, .. } => format!("[{citation_id}]"),
        Inline::FootnoteRef { footnote_id, .. } => format!("[{footnote_id}]"),
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
            .unwrap_or_default(),
        Inline::DateChip { date, .. } => date.clone(),
        Inline::Equation { equation, .. } => equation.source.clone(),
        // Shown as the field it is, the way a word processor shows an
        // unresolved field, because it has no resolved value outside a page.
        Inline::PageNumber { field, .. } => format!("[{}]", field.as_str()),
    }
}

fn truncate_label(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

pub(crate) fn suggestion_text(kind: &SuggestionKind) -> String {
    match kind {
        SuggestionKind::Insert { content, .. } => content
            .iter()
            .map(inline_display_text)
            .collect::<Vec<_>>()
            .join(""),
        SuggestionKind::Delete { .. }
        | SuggestionKind::Format { .. }
        | SuggestionKind::FormatRemove { .. }
        | SuggestionKind::FormatReplace { .. }
        | SuggestionKind::LinkChange { .. }
        | SuggestionKind::BlockDelete { .. } => String::new(),
        SuggestionKind::BlockInsert { block, .. } => block_display_text(block),
        SuggestionKind::BlockReplace { replacement, .. } => block_display_text(replacement),
        SuggestionKind::ParagraphStyleChange { .. } => String::new(),
    }
}

pub(crate) fn citation_source_format(format: &CitationSourceFormat) -> String {
    match format {
        CitationSourceFormat::CitumNative => "citum-native".to_string(),
        CitationSourceFormat::CslJson => "csl-json".to_string(),
        CitationSourceFormat::Bibtex => "bibtex".to_string(),
        CitationSourceFormat::Ris => "ris".to_string(),
        CitationSourceFormat::Unknown(value) => value.clone(),
    }
}

pub(crate) fn citation_source_format_from_label(label: &str) -> CitationSourceFormat {
    match label {
        "citum-native" => CitationSourceFormat::CitumNative,
        "csl-json" => CitationSourceFormat::CslJson,
        "bibtex" => CitationSourceFormat::Bibtex,
        "ris" => CitationSourceFormat::Ris,
        other => CitationSourceFormat::Unknown(other.to_string()),
    }
}

pub(crate) fn refresh_inline_citation_cache(blocks: &mut [Block], citation_id: &StableId) {
    for block in blocks {
        for inline in &mut block.content {
            if let Inline::Citation {
                citation_id: inline_citation_id,
                rendered_cache,
                ..
            } = inline
            {
                if inline_citation_id == citation_id {
                    *rendered_cache = None;
                }
            }
        }
        if let BlockKind::Table { rows, .. } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    refresh_inline_citation_cache(&mut cell.blocks, citation_id);
                }
            }
        }
    }
}

pub(crate) fn parse_id(value: &str) -> Result<StableId, AppApiError> {
    StableId::parse(value.trim().to_string()).map_err(|err| AppApiError::Model(err.to_string()))
}

pub(crate) fn parse_anchor_label(value: &str, label: &str) -> Result<Anchor, AppApiError> {
    if value.trim().is_empty() {
        return Err(AppApiError::Format(format!("{label} is empty")));
    }
    if value.trim() != value {
        return Err(AppApiError::Format(format!(
            "{label} has surrounding whitespace"
        )));
    }
    if value == "document" {
        return Ok(Anchor::Document);
    }
    if let Some(block_id) = value.strip_prefix("nearest:") {
        return StableId::parse(block_id.to_string())
            .map(|block_id| Anchor::NearestBlock {
                block_id,
                warning: "anchor restored to nearest block".to_string(),
            })
            .map_err(|err| AppApiError::Model(err.to_string()));
    }
    if let Some((start, end)) = value.split_once("..") {
        return match (
            StableId::parse(start.to_string()),
            StableId::parse(end.to_string()),
        ) {
            (Ok(start), Ok(end)) => Ok(Anchor::TextRange(TextRange { start, end })),
            (Err(err), _) | (_, Err(err)) => Err(AppApiError::Model(err.to_string())),
        };
    }
    Err(AppApiError::Format(format!(
        "unsupported app anchor label {value}"
    )))
}
