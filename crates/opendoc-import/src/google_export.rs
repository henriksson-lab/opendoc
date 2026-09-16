//! Writing the canonical document model back out as Google Docs JSON.

use crate::error::ImportError;
use crate::google_color::export_google_color;
use crate::google_style;
use crate::google_style::{export_bullet, export_paragraph_style, warning};
use crate::opendoc_json::export_equation_source_format;
use opendoc_core::{
    table_covered_positions, Anchor, Block, BlockKind, Comment, CommentHistoryEntry, CommentThread,
    Footnote, Mark, MarkKind, ModelWarning, Suggestion, SuggestionKind, SuggestionState, TableRow,
    TextRange,
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
        BlockKind::Title => Ok(export_google_paragraph(
            block,
            Some("TITLE".to_string()),
            None,
            export_google_inlines(&block.content)?,
            warnings,
        )),
        BlockKind::Subtitle => Ok(export_google_paragraph(
            block,
            Some("SUBTITLE".to_string()),
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
        BlockKind::Table {
            columns,
            properties,
            rows,
        } => {
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
            let mut table = Map::new();
            table.insert(
                "tableRows".to_string(),
                Value::Array({
                    let covered = table_covered_positions(rows);
                    rows.iter()
                        .enumerate()
                        .map(|(row_index, row)| {
                            export_google_table_row(row, row_index, &covered, warnings)
                        })
                        .collect::<Result<Vec<_>, _>>()?
                }),
            );
            if columns.iter().any(|column| column.width.is_some()) {
                table.insert(
                    "tableStyle".to_string(),
                    json!({ "tableColumnProperties": columns.iter().map(export_google_table_column).collect::<Vec<_>>() }),
                );
            }
            if let Some(border) = properties.border {
                table.insert(
                    "opendocTableBorder".to_string(),
                    json!({
                        "style": border.style().as_str(),
                        "twips": border.width().twips(),
                        "color": border.color().as_hex(),
                    }),
                );
            }
            if let Some(alignment) = properties.alignment {
                table.insert(
                    "opendocTableAlignment".to_string(),
                    Value::String(alignment.as_str().to_string()),
                );
            }
            Ok(json!({ "table": table }))
        }
        BlockKind::PageBreak => Ok(export_google_paragraph(
            block,
            None,
            None,
            vec![json!({ "pageBreak": {} })],
            warnings,
        )),
        BlockKind::HorizontalRule => Ok(export_google_paragraph(
            block,
            None,
            None,
            vec![json!({ "horizontalRule": {} })],
            warnings,
        )),
        BlockKind::TableOfContents { max_level } => {
            warnings.push(warning(
                "google-export-toc-as-opendoc-extension",
                "the generated OpenDoc table of contents was retained only in an OpenDoc extension; no native Google Docs TOC request is emitted",
            ));
            Ok(
                json!({ "opendocTableOfContents": { "blockId": block.id.to_string(), "maxLevel": max_level } }),
            )
        }
        BlockKind::Bibliography => {
            warnings.push(warning(
                "google-export-bibliography-as-opendoc-extension",
                "the generated OpenDoc bibliography was retained only in an OpenDoc extension; no native Google Docs bibliography request is emitted",
            ));
            Ok(json!({ "opendocBibliography": { "blockId": block.id.to_string() } }))
        }
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
            layout,
        } => {
            report_unexportable_block_properties(block, warnings);
            // The portable OpenDoc extension preserves this complete layout,
            // but is deliberately not a claim that a Google positioned
            // object was created. Native upload/placement remains a caller's
            // authorised API transport responsibility.
            // The Docs API represents an image by an `inlineObjectId`, but
            // its JSON contains a transient authenticated `contentUri`, not
            // transferable image bytes. This entry deliberately remains an
            // OpenDoc reference until a caller supplies an authorised upload
            // transport; emitting a made-up URI would make an unusable JSON
            // document look portable.
            if layout.positioned.is_some() {
                warnings.push(warning(
                    "google-export-positioned-image-unmapped",
                    "a positioned OpenDoc image was retained only in the OpenDoc layout extension; no native Google positioned object or placement request was emitted",
                ));
            }
            warnings.push(warning(
                "google-image-resource-required",
                "image bytes were not emitted as a Google native inline object: Google Docs JSON has no transferable image payload; upload the blob through an authorised API transport and bind its inlineObjectId",
            ));
            opendoc_core::HashRef::parse(blob_hash).map_err(|err| {
                ImportError::UnsupportedStructure(format!(
                    "OpenDoc image blobHash is invalid: {err}"
                ))
            })?;
            Ok(json!({
                "opendocImage": {
                    "blockId": block.id.to_string(),
                    "blobHash": blob_hash,
                    "altText": alt_text,
                    "layout": layout
                }
            }))
        }
    }
}

/// Whether this block is emitted as a native Google paragraph and can carry
/// `paragraphStyle.pageBreakBefore`. Tables and OpenDoc extension blocks need
/// an explicit standalone page-break paragraph instead.
pub(crate) fn can_export_page_break_before(block: &Block) -> bool {
    matches!(
        &block.kind,
        BlockKind::Paragraph
            | BlockKind::Title
            | BlockKind::Subtitle
            | BlockKind::Heading { .. }
            | BlockKind::ListItem { .. }
            | BlockKind::HorizontalRule
    )
}

/// Marks a just-exported native paragraph with Google's `pageBreakBefore`.
/// This is intentionally a post-processing step because the OpenDoc fact is
/// structural (`PageBreak` followed by this block), while the Google spelling
/// sits inside the following paragraph's style.
pub(crate) fn set_google_page_break_before(value: &mut Value) {
    let paragraph = value
        .get_mut("paragraph")
        .and_then(Value::as_object_mut)
        .expect("page-break-before is only applied to exported Google paragraphs");
    let style = paragraph
        .entry("paragraphStyle".to_string())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .expect("exported Google paragraph style is an object");
    style.insert("pageBreakBefore".to_string(), Value::Bool(true));
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
    row_index: usize,
    covered: &std::collections::BTreeSet<(usize, usize)>,
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
        .enumerate()
        .filter(|(column_index, _)| !covered.contains(&(row_index, *column_index)))
        .map(|(_, cell)| {
            if cell.blocks.is_empty() {
                return Err(ImportError::UnsupportedStructure(
                    "OpenDoc table cell has no blocks".to_string(),
                ));
            }
            let mut exported = Map::new();
            exported.insert(
                "content".to_string(),
                Value::Array(
                    cell.blocks
                        .iter()
                        .map(|block| export_google_block(block, true, warnings))
                        .collect::<Result<Vec<_>, _>>()?,
                ),
            );
            if let Some(style) = export_google_table_cell_style(cell, warnings) {
                exported.insert("tableCellStyle".to_string(), style);
            }
            // Google Docs has no semantic row-header cell field. Preserve
            // OpenDoc's explicit (including explicit false) source state in
            // the same opt-in extension namespace used for row headers.
            if let Some(row_header) = cell.properties.row_header {
                exported.insert("opendocRowHeader".to_string(), Value::Bool(row_header));
            }
            Ok(Value::Object(exported))
        })
        .collect::<Result<Vec<_>, ImportError>>()?;
    let mut exported = Map::new();
    exported.insert("tableCells".to_string(), Value::Array(cells));
    if row.height.is_some() || row.header {
        let mut style = Map::new();
        if let Some(height) = row.height {
            style.insert("minRowHeight".to_string(), export_google_dimension(height));
        }
        if row.header {
            style.insert("tableHeader".to_string(), Value::Bool(true));
        }
        exported.insert("tableRowStyle".to_string(), Value::Object(style));
    }
    Ok(Value::Object(exported))
}

fn export_google_table_column(column: &opendoc_core::TableColumn) -> Value {
    match column.width {
        Some(width) => json!({
            "widthType": "FIXED_WIDTH",
            "width": export_google_dimension(width),
        }),
        None => json!({ "widthType": "WIDTH_TYPE_UNSPECIFIED" }),
    }
}

fn export_google_dimension(length: opendoc_core::Length) -> Value {
    json!({ "magnitude": length.twips() as f64 / 20.0, "unit": "PT" })
}

fn export_google_table_cell_style(
    cell: &opendoc_core::TableCell,
    warnings: &mut Vec<ModelWarning>,
) -> Option<Value> {
    let properties = &cell.properties;
    let mut style = Map::new();
    if cell.span.rows() != 1 {
        style.insert("rowSpan".to_string(), json!(cell.span.rows()));
    }
    if cell.span.columns() != 1 {
        style.insert("columnSpan".to_string(), json!(cell.span.columns()));
    }
    if let Some(background) = properties.background {
        style.insert(
            "backgroundColor".to_string(),
            export_google_color(&background.as_hex()),
        );
    }
    for (key, border) in [
        ("borderTop", properties.border_top),
        ("borderBottom", properties.border_bottom),
        ("borderLeft", properties.border_start),
        ("borderRight", properties.border_end),
    ] {
        if let Some(border) = border {
            style.insert(
                key.to_string(),
                export_google_table_border(border, warnings),
            );
        }
    }
    for (key, padding) in [
        ("paddingTop", properties.padding_top),
        ("paddingBottom", properties.padding_bottom),
        ("paddingLeft", properties.padding_start),
        ("paddingRight", properties.padding_end),
    ] {
        if let Some(padding) = padding {
            style.insert(key.to_string(), export_google_dimension(padding));
        }
    }
    if let Some(alignment) = properties.vertical_alignment {
        let alignment = match alignment {
            opendoc_core::VerticalAlignment::Top => "TOP",
            opendoc_core::VerticalAlignment::Middle => "MIDDLE",
            opendoc_core::VerticalAlignment::Bottom => "BOTTOM",
        };
        style.insert(
            "contentAlignment".to_string(),
            Value::String(alignment.to_string()),
        );
    }
    (!style.is_empty()).then_some(Value::Object(style))
}

fn export_google_table_border(
    border: opendoc_core::CellBorder,
    warnings: &mut Vec<ModelWarning>,
) -> Value {
    if border.style() == opendoc_core::BorderStyle::None || border.width().twips() == 0 {
        return json!({ "width": export_google_dimension(opendoc_core::Length::ZERO) });
    }
    let dash = match border.style() {
        opendoc_core::BorderStyle::Solid => "SOLID",
        opendoc_core::BorderStyle::Dashed => "DASH",
        opendoc_core::BorderStyle::Dotted => "DOT",
        opendoc_core::BorderStyle::Double => {
            warnings.push(warning(
                "google-approximated-table-border",
                "OpenDoc double table border was exported as Google Docs solid: Google has no double dash style",
            ));
            "SOLID"
        }
        opendoc_core::BorderStyle::None => unreachable!("zero-width border returned above"),
    };
    json!({
        "width": export_google_dimension(border.width()),
        "color": export_google_color(&border.color().as_hex()),
        "dashStyle": dash,
    })
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
                    if href.trim() != href {
                        return Err(ImportError::UnsupportedStructure(
                            "OpenDoc link href has surrounding whitespace for Google Docs export"
                                .to_string(),
                        ));
                    }
                    if !crate::google_import::google_external_link_href_is_safe(href) {
                        return Err(ImportError::UnsupportedStructure(
                            "OpenDoc link href uses an unsafe navigation scheme for Google Docs export"
                                .to_string(),
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
                opendoc_core::Inline::GooglePersonChip {
                    label,
                    email,
                    person_id,
                    ..
                } => json!({
                    "person": { "personProperties": {
                        "name": label,
                        "email": email,
                        "personId": person_id,
                    }}
                }),
                opendoc_core::Inline::GoogleRichLinkChip {
                    label,
                    href,
                    rich_link_id,
                    mime_type,
                    ..
                } => {
                    if href.trim() != href {
                        return Err(ImportError::UnsupportedStructure(
                            "OpenDoc Google rich link href has surrounding whitespace for Google Docs export"
                                .to_string(),
                        ));
                    }
                    if !crate::google_import::google_external_link_href_is_safe(href) {
                        return Err(ImportError::UnsupportedStructure(
                            "OpenDoc Google rich link href uses an unsafe navigation scheme for Google Docs export"
                                .to_string(),
                        ));
                    }
                    json!({
                        "richLink": {
                            "richLinkId": rich_link_id,
                            "richLinkProperties": {
                                "title": label,
                                "uri": href,
                                "mimeType": mime_type,
                            }
                        }
                    })
                }
                opendoc_core::Inline::Dropdown {
                    id,
                    options,
                    selected_option_id,
                } => {
                    // The public Google Docs JSON surface has no portable
                    // dropdown paragraph element.  Keep the typed values in
                    // OpenDoc's namespaced extension rather than pretending
                    // a rich-link/person chip is a dropdown.
                    json!({
                        "opendocDropdown": {
                            "inlineId": id.to_string(),
                            "options": options.iter().map(|option| json!({
                                "id": option.id,
                                "label": option.label,
                            })).collect::<Vec<_>>(),
                            "selectedOptionId": selected_option_id,
                        }
                    })
                }
                // A DateChip is deliberately a calendar day, so it has an
                // exact native Google DateElement spelling only as ISO UTC
                // midnight with time display disabled.  Do not use the old
                // OpenDoc-only extension: a Google reader can preserve and
                // render this bounded native subset itself.
                opendoc_core::Inline::DateChip { id, date } => json!({
                    "dateElement": {
                        "dateId": id.to_string(),
                        "dateElementProperties": {
                            "timestamp": format!("{date}T00:00:00Z"),
                            "timeZoneId": "Etc/UTC",
                            "dateFormat": "DATE_FORMAT_ISO8601",
                            "timeFormat": "TIME_FORMAT_DISABLED",
                            "displayText": date,
                        }
                    }
                }),
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
                    "state": match thread.state { opendoc_core::CommentThreadState::Open => "open", opendoc_core::CommentThreadState::Resolved => "resolved", opendoc_core::CommentThreadState::Reopened => "reopened" },
                    "resolvedBy": thread.resolved_by,
                    "resolvedAtMs": thread.resolved_at_ms,
                    "actionAssignee": thread.action_assignee,
                    "actionDueAtMs": thread.action_due_at_ms,
                    "actionCompletedBy": thread.action_completed_by,
                    "actionCompletedAtMs": thread.action_completed_at_ms,
                    "reactions": thread.reactions.iter().map(|reaction| json!({
                        "emoji": reaction.emoji,
                        "actors": reaction.actors,
                    })).collect::<Vec<_>>(),
                    "deleted": thread.deleted
                }))
            })
            .collect::<Result<Vec<_>, ImportError>>()?,
    ))
}

/// OpenDoc's append-only comment evidence has no Google Docs REST equivalent,
/// so retain it in the same explicit Google-shaped extension namespace as the
/// thread/action/reaction data.  `previousBody` stays structured inline data:
/// reducing it to display text here would destroy marks and links that the
/// history record is specifically intended to preserve.
pub(crate) fn export_google_comment_history(
    history: &[CommentHistoryEntry],
) -> Result<Value, ImportError> {
    Ok(Value::Array(
        history
            .iter()
            .map(|entry| {
                let mut value = json!({
                    "threadId": entry.thread_id.to_string(),
                    "commentId": entry.comment_id.to_string(),
                    "kind": entry.kind,
                    "actor": entry.actor,
                    "atMs": entry.at_ms,
                });
                if let Some(body) = &entry.previous_body {
                    value["previousBody"] = Value::Array(export_google_inlines(body)?);
                }
                Ok(value)
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
        SuggestionKind::FormatRemove { range, kind, value } => json!({
            // This is OpenDoc's Google-native interchange contract, not a
            // claim that the Google Docs REST API can create a tracked style
            // removal.  Keeping a distinct tag prevents an import from
            // turning a removal into an added mark.
            "type": "format_remove",
            "range": export_google_range(range),
            "textStyle": export_google_text_style(&[opendoc_core::Mark {
                kind: kind.clone(),
                value: value.clone(),
                expand: opendoc_core::MarkExpand::Both,
            }])?
        }),
        SuggestionKind::FormatReplace {
            range,
            kind,
            expected_value,
            value,
        } => json!({
            // OpenDoc's extension retains the reviewed source value rather
            // than pretending Google REST has a tracked compare-and-set API.
            "type": "format_replace",
            "range": export_google_range(range),
            "expectedValue": expected_value,
            "textStyle": export_google_text_style(&[opendoc_core::Mark {
                kind: kind.clone(),
                value: Some(value.clone()),
                expand: opendoc_core::MarkExpand::Both,
            }])?
        }),
        SuggestionKind::LinkChange {
            inline_id,
            expected_href,
            href,
        } => json!({
            // OpenDoc's Google-shaped extension: a link is an atomic inline
            // property, not a Google REST text-style suggestion.
            "type": "link_change",
            "inlineId": inline_id.to_string(),
            "expectedHref": expected_href,
            "href": href
        }),
        SuggestionKind::BlockDelete { block_id } => json!({
            // OpenDoc's Google-shaped interchange extension. Google Docs'
            // public API cannot author a tracked structural deletion.
            "type": "block_delete",
            "blockId": block_id.to_string()
        }),
        SuggestionKind::BlockInsert { position, block } => json!({
            // OpenDoc extension: native Google APIs cannot create a tracked
            // structural insertion. The complete canonical paragraph and
            // sibling anchor are retained, not reduced to preview text.
            "type": "block_insert",
            "position": position,
            "block": block
        }),
        SuggestionKind::BlockReplace {
            block_id,
            expected,
            replacement,
        } => json!({
            "type": "block_replace",
            "blockId": block_id.to_string(),
            "expected": expected,
            "replacement": replacement
        }),
        SuggestionKind::ParagraphStyleChange {
            block_id,
            expected,
            proposed,
        } => json!({
            "type": "block_style_change",
            "blockId": block_id.to_string(),
            "expectedStyle": expected,
            "proposedStyle": proposed
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
        // This is an OpenDoc-extension value, not a Google position.  In
        // particular, never turn an orphan into a document or nearest-block
        // anchor: that would falsely present the thread as attached to live
        // content and discard the review evidence it was preserving.
        Anchor::Orphaned {
            quote,
            context,
            warning,
        } => json!({
            "type": "orphaned",
            "quote": quote,
            "context": context,
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

/// The Google named paragraph styles that have an exact non-outline OpenDoc
/// counterpart.  Headings stay a separate numeric mapping because their
/// outline level is semantic as well as visual.
pub(crate) fn named_block_style(named_style: &str) -> Option<BlockKind> {
    match named_style {
        "TITLE" => Some(BlockKind::Title),
        "SUBTITLE" => Some(BlockKind::Subtitle),
        _ => heading_level(named_style).map(|level| BlockKind::Heading { level }),
    }
}
