//! Writing the canonical document model back out as Google Docs JSON.

use crate::error::ImportError;
use crate::google_color::export_google_color;
use crate::google_style;
use crate::google_style::{export_bullet, export_paragraph_style, warning};
use crate::opendoc_json::export_equation_source_format;
use opendoc_core::{
    Anchor, Block, BlockKind, Comment, CommentThread, Footnote, Mark, MarkKind, ModelWarning,
    Suggestion, SuggestionKind, SuggestionState, TableRow, TextRange,
};
use serde_json::{json, Map, Value};

pub(crate) fn export_google_block(
    block: &Block,
    allow_tables: bool,
    warnings: &mut Vec<ModelWarning>,
) -> Result<Value, ImportError> {
    match &block.kind {
        BlockKind::Paragraph => Ok(export_google_paragraph(
            block,
            None,
            None,
            export_google_inlines(&block.content)?,
            warnings,
        )),
        BlockKind::Heading { level } => Ok(export_google_paragraph(
            block,
            Some(format!("HEADING_{level}")),
            None,
            export_google_inlines(&block.content)?,
            warnings,
        )),
        BlockKind::ListItem {
            list_id,
            level,
            kind,
        } => Ok(export_google_paragraph(
            block,
            None,
            Some(export_bullet(list_id, level, kind)),
            export_google_inlines(&block.content)?,
            warnings,
        )),
        BlockKind::Table { rows, .. } => {
            if !allow_tables {
                return Err(ImportError::UnsupportedStructure(
                    "nested OpenDoc tables cannot be exported to Google Docs-shaped JSON v0"
                        .to_string(),
                ));
            }
            if rows.is_empty() {
                return Err(ImportError::UnsupportedStructure(
                    "OpenDoc table has no rows".to_string(),
                ));
            }
            Ok(json!({
                "table": {
                    "tableRows": rows
                        .iter()
                        .map(|row| export_google_table_row(row, warnings))
                        .collect::<Result<Vec<_>, _>>()?
                }
            }))
        }
        BlockKind::PageBreak => Ok(export_google_paragraph(
            block,
            None,
            None,
            vec![json!({ "pageBreak": {} })],
            warnings,
        )),
        BlockKind::EquationBlock { equation } => {
            report_unexportable_block_properties(block, warnings);
            if equation.source.trim().is_empty() {
                return Err(ImportError::UnsupportedStructure(
                    "OpenDoc block equation source is empty".to_string(),
                ));
            }
            Ok(json!({
                "opendocEquationBlock": {
                    "blockId": block.id.to_string(),
                    "equationId": equation.id.to_string(),
                    "sourceFormat": export_equation_source_format(&equation.source_format),
                    "source": equation.source
                }
            }))
        }
        BlockKind::Image {
            blob_hash,
            alt_text,
            ..
        } => {
            report_unexportable_block_properties(block, warnings);
            opendoc_core::HashRef::parse(blob_hash).map_err(|err| {
                ImportError::UnsupportedStructure(format!(
                    "OpenDoc image blobHash is invalid: {err}"
                ))
            })?;
            Ok(json!({
                "opendocImage": {
                    "blockId": block.id.to_string(),
                    "blobHash": blob_hash,
                    "altText": alt_text
                }
            }))
        }
    }
}

/// Assembles one Google `paragraph`, merging the block's typed properties into
/// `paragraphStyle` alongside whatever named style the block kind implies.
pub(crate) fn export_google_paragraph(
    block: &Block,
    named_style: Option<String>,
    bullet: Option<Value>,
    elements: Vec<Value>,
    warnings: &mut Vec<ModelWarning>,
) -> Value {
    let mut paragraph = Map::new();
    let mut style = export_paragraph_style(&block.properties, warnings);
    if let Some(named_style) = named_style {
        style.insert("namedStyleType".to_string(), Value::String(named_style));
    }
    if !style.is_empty() {
        paragraph.insert("paragraphStyle".to_string(), Value::Object(style));
    }
    if let Some(bullet) = bullet {
        paragraph.insert("bullet".to_string(), bullet);
    }
    paragraph.insert("elements".to_string(), Value::Array(elements));
    json!({ "paragraph": Value::Object(paragraph) })
}

/// Equation and image blocks travel as OpenDoc extensions, which carry no
/// paragraph style, so any block formatting on them is lost on export.
pub(crate) fn report_unexportable_block_properties(
    block: &Block,
    warnings: &mut Vec<ModelWarning>,
) {
    if block.properties.is_empty() {
        return;
    }
    warnings.push(warning(
        google_style::DROPPED_BLOCK_PROPERTIES,
        "block formatting on an equation or image block has no Google Docs representation and was dropped",
    ));
}

pub(crate) fn export_google_table_row(
    row: &TableRow,
    warnings: &mut Vec<ModelWarning>,
) -> Result<Value, ImportError> {
    if row.cells.is_empty() {
        return Err(ImportError::UnsupportedStructure(
            "OpenDoc table row has no cells".to_string(),
        ));
    }
    let cells = row
        .cells
        .iter()
        .map(|cell| {
            if cell.blocks.is_empty() {
                return Err(ImportError::UnsupportedStructure(
                    "OpenDoc table cell has no blocks".to_string(),
                ));
            }
            Ok(json!({
                "content": cell
                    .blocks
                    .iter()
                    .map(|block| export_google_block(block, false, warnings))
                    .collect::<Result<Vec<_>, _>>()?
            }))
        })
        .collect::<Result<Vec<_>, ImportError>>()?;
    Ok(json!({ "tableCells": cells }))
}

pub(crate) fn export_google_inlines(
    inlines: &[opendoc_core::Inline],
) -> Result<Vec<Value>, ImportError> {
    inlines
        .iter()
        .map(|inline| {
            Ok(match inline {
                opendoc_core::Inline::Text { text, marks, .. } => json!({
                    "textRun": {
                        "content": text,
                        "textStyle": export_google_text_style(marks)?
                    }
                }),
                opendoc_core::Inline::Link {
                    text, href, marks, ..
                } => {
                    if href.trim().is_empty() {
                        return Err(ImportError::UnsupportedStructure(
                            "OpenDoc link href is empty".to_string(),
                        ));
                    }
                    let mut style = export_google_text_style(marks)?;
                    style["link"] = json!({ "url": href });
                    json!({ "textRun": { "content": text, "textStyle": style } })
                }
                opendoc_core::Inline::FootnoteRef { footnote_id, .. } => json!({
                    "footnoteReference": {
                        "footnoteId": footnote_id.to_string()
                    }
                }),
                opendoc_core::Inline::Equation { id, equation } => {
                    if equation.source.trim().is_empty() {
                        return Err(ImportError::UnsupportedStructure(
                            "OpenDoc inline equation source is empty".to_string(),
                        ));
                    }
                    json!({
                        "opendocEquation": {
                            "inlineId": id.to_string(),
                            "equationId": equation.id.to_string(),
                            "sourceFormat": export_equation_source_format(&equation.source_format),
                            "source": equation.source
                        }
                    })
                }
                opendoc_core::Inline::Citation {
                    citation_id,
                    rendered_cache,
                    ..
                } => json!({
                    "opendocCitation": {
                        "citationId": citation_id.to_string(),
                        "renderedCache": rendered_cache
                    }
                }),
                opendoc_core::Inline::Mention { id, label } => {
                    if label.trim().is_empty() {
                        return Err(ImportError::UnsupportedStructure(
                            "OpenDoc mention label is empty".to_string(),
                        ));
                    }
                    json!({
                        "opendocMention": {
                            "inlineId": id.to_string(),
                            "label": label
                        }
                    })
                }
                // Google models page-number fields as `autoText`, which is
                // exactly what `Inline::PageNumber` is, so this round-trips
                // without an OpenDoc extension key.
                opendoc_core::Inline::PageNumber { id, field } => json!({
                    "autoText": {
                        "opendocInlineId": id.to_string(),
                        "type": match field {
                            opendoc_core::PageNumberField::CurrentPage => "PAGE_NUMBER",
                            opendoc_core::PageNumberField::PageCount => "PAGE_COUNT",
                        }
                    }
                }),
            })
        })
        .collect()
}

pub(crate) fn export_google_footnotes(
    footnotes: &[Footnote],
) -> Result<serde_json::Map<String, Value>, ImportError> {
    let mut map = serde_json::Map::new();
    for footnote in footnotes.iter().filter(|footnote| !footnote.deleted) {
        map.insert(
            footnote.id.to_string(),
            json!({
                "footnoteId": footnote.id.to_string(),
                "content": [{
                    "paragraph": {
                        "elements": export_google_inlines(&footnote.body)?
                    }
                }]
            }),
        );
    }
    Ok(map)
}

pub(crate) fn export_google_comments(comments: &[CommentThread]) -> Result<Value, ImportError> {
    Ok(Value::Array(
        comments
            .iter()
            .map(|thread| {
                Ok(json!({
                    "id": thread.id.to_string(),
                    "anchor": export_google_anchor(&thread.anchor),
                    "comments": thread.comments.iter().map(export_google_comment).collect::<Result<Vec<_>, _>>()?,
                    "deleted": thread.deleted
                }))
            })
            .collect::<Result<Vec<_>, ImportError>>()?,
    ))
}

pub(crate) fn export_google_comment(comment: &Comment) -> Result<Value, ImportError> {
    Ok(json!({
        "id": comment.id.to_string(),
        "author": comment.author,
        "body": export_google_inlines(&comment.body)?,
        "createdAtMs": comment.created_at_ms,
        "deleted": comment.deleted
    }))
}

pub(crate) fn export_google_suggestions(suggestions: &[Suggestion]) -> Result<Value, ImportError> {
    Ok(Value::Array(
        suggestions
            .iter()
            .map(export_google_suggestion)
            .collect::<Result<Vec<_>, _>>()?,
    ))
}

pub(crate) fn export_google_suggestion(suggestion: &Suggestion) -> Result<Value, ImportError> {
    Ok(json!({
        "id": suggestion.id.to_string(),
        "author": suggestion.author,
        "kind": export_google_suggestion_kind(&suggestion.kind)?,
        "state": export_google_suggestion_state(&suggestion.state),
        "provenance": suggestion.provenance
    }))
}

pub(crate) fn export_google_suggestion_kind(kind: &SuggestionKind) -> Result<Value, ImportError> {
    Ok(match kind {
        SuggestionKind::Insert { anchor, content } => json!({
            "type": "insert",
            "anchor": export_google_anchor(anchor),
            "content": export_google_inlines(content)?
        }),
        SuggestionKind::Delete { range } => json!({
            "type": "delete",
            "range": export_google_range(range)
        }),
        SuggestionKind::Format { range, marks } => json!({
            "type": "format",
            "range": export_google_range(range),
            "textStyle": export_google_text_style(marks)?
        }),
    })
}

pub(crate) fn export_google_suggestion_state(state: &SuggestionState) -> &'static str {
    match state {
        SuggestionState::Proposed => "proposed",
        SuggestionState::Accepted => "accepted",
        SuggestionState::Rejected => "rejected",
    }
}

pub(crate) fn export_google_anchor(anchor: &Anchor) -> Value {
    match anchor {
        Anchor::TextRange(range) => {
            let mut value = export_google_range(range);
            value["type"] = json!("textRange");
            value
        }
        Anchor::NearestBlock { block_id, warning } => json!({
            "type": "nearestBlock",
            "blockId": block_id.to_string(),
            "warning": warning
        }),
        Anchor::Document => json!({ "type": "document" }),
    }
}

pub(crate) fn export_google_range(range: &TextRange) -> Value {
    json!({
        "start": range.start.to_string(),
        "end": range.end.to_string()
    })
}

pub(crate) fn export_google_text_style(marks: &[Mark]) -> Result<Value, ImportError> {
    let mut style = json!({});
    for mark in marks {
        match mark.kind {
            MarkKind::Bold => {
                reject_boolean_mark_value(mark)?;
                style["bold"] = json!(true);
            }
            MarkKind::Italic => {
                reject_boolean_mark_value(mark)?;
                style["italic"] = json!(true);
            }
            MarkKind::Underline => {
                reject_boolean_mark_value(mark)?;
                style["underline"] = json!(true);
            }
            MarkKind::Strike => {
                reject_boolean_mark_value(mark)?;
                style["strikethrough"] = json!(true);
            }
            MarkKind::Superscript => {
                reject_boolean_mark_value(mark)?;
                style["baselineOffset"] = json!("SUPERSCRIPT");
            }
            MarkKind::Subscript => {
                reject_boolean_mark_value(mark)?;
                style["baselineOffset"] = json!("SUBSCRIPT");
            }
            MarkKind::Color => {
                let value = required_mark_value(mark)?;
                style["foregroundColor"] = export_google_color(value);
            }
            MarkKind::Background => {
                let value = required_mark_value(mark)?;
                style["backgroundColor"] = export_google_color(value);
            }
            MarkKind::Font => {
                let value = required_mark_value(mark)?;
                style["weightedFontFamily"] = json!({ "fontFamily": value });
            }
            MarkKind::Size => {
                let value = required_mark_value(mark)?;
                let magnitude = value.parse::<f64>().map_err(|_| {
                    ImportError::UnsupportedStructure(
                        "OpenDoc size mark value is invalid".to_string(),
                    )
                })?;
                style["fontSize"] = json!({ "magnitude": magnitude, "unit": "PT" });
            }
            MarkKind::Code | MarkKind::Link | MarkKind::Citation => {
                reject_boolean_mark_value(mark)?;
            }
        }
    }
    Ok(style)
}

pub(crate) fn required_mark_value(mark: &Mark) -> Result<&str, ImportError> {
    mark.value
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            ImportError::UnsupportedStructure("OpenDoc mark value is missing".to_string())
        })
}

pub(crate) fn reject_boolean_mark_value(mark: &Mark) -> Result<(), ImportError> {
    if mark.value.is_some() {
        return Err(ImportError::UnsupportedStructure(
            "OpenDoc boolean mark has value".to_string(),
        ));
    }
    Ok(())
}

pub(crate) fn heading_level(named_style: &str) -> Option<u8> {
    named_style
        .strip_prefix("HEADING_")
        .and_then(|level| level.parse::<u8>().ok())
        .filter(|level| (1..=6).contains(level))
}
