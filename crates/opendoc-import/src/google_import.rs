//! Reading Google Docs JSON into the canonical document model.

use crate::error::ImportError;
use crate::google_color::{import_google_rgb, trim_float};
use crate::google_export::heading_level;
use crate::google_style;
use crate::google_style::{
    import_paragraph_style, report_unmapped_named_style, warning, GoogleLists,
};
use crate::json::{
    expect_object, optional_array, optional_bool, optional_object, optional_source_string,
    optional_str, optional_u64, optional_u8, parse_imported_stable_id, required_array,
    required_source_string, required_str, source_string,
};
use crate::opendoc_json::{
    import_opendoc_equation_block, import_opendoc_image, import_opendoc_inline_equation,
};
use crate::{google_part_is_present, mark};
use opendoc_core::{
    Anchor, Block, BlockKind, BlockProperties, Comment, CommentThread, Equation,
    EquationSourceFormat, Footnote, Inline, Mark, MarkKind, ModelWarning, StableId, Suggestion,
    SuggestionKind, SuggestionState, TableCell, TableRow, TextRange,
};
use serde_json::{json, Value};

/// One Google paragraph becomes one OpenDoc block, except when a `pageBreak`
/// element sits inside it: OpenDoc models a page break as its own block, so the
/// paragraph splits around it, exactly as the DOCX reader does.
pub(crate) fn import_google_paragraph(
    paragraph: &Value,
    lists: &GoogleLists,
    warnings: &mut Vec<ModelWarning>,
) -> Result<Vec<Block>, ImportError> {
    let style = optional_object(paragraph, "paragraphStyle")?.unwrap_or(&Value::Null);
    let named_style = optional_str(style, "namedStyleType")?.unwrap_or_default();
    let bullet = optional_object(paragraph, "bullet")?;
    let properties = import_paragraph_style(style, warnings)?;
    let kind = if let Some(level) = heading_level(named_style) {
        BlockKind::Heading { level }
    } else {
        report_unmapped_named_style(named_style, warnings);
        match bullet {
            Some(bullet) => {
                let level = optional_u8(bullet, "nestingLevel")?.unwrap_or(0);
                if level > 8 {
                    return Err(ImportError::InvalidInput(
                        "nestingLevel is outside 0..=8".to_string(),
                    ));
                }
                let list_id = optional_str(bullet, "listId")?.unwrap_or("google-list");
                BlockKind::ListItem {
                    list_id: parse_imported_stable_id(list_id)?,
                    level,
                    kind: lists.resolve(bullet, list_id, level, warnings)?,
                }
            }
            None => BlockKind::Paragraph,
        }
    };
    if optional_array(paragraph, "positionedObjectIds")?.is_some_and(|ids| !ids.is_empty()) {
        warnings.push(warning(
            google_style::DROPPED_DOCUMENT_PART,
            "Google Docs positioned objects anchored to a paragraph are not representable and were dropped",
        ));
    }
    let segments = import_google_paragraph_elements(paragraph, warnings)?;
    let split = segments
        .iter()
        .any(|segment| matches!(segment, GoogleSegment::PageBreak));
    let mut blocks = Vec::new();
    let mut current: Vec<Inline> = Vec::new();
    for segment in segments {
        match segment {
            GoogleSegment::Inline(inline) => current.push(inline),
            GoogleSegment::PageBreak => {
                flush_google_fragment(&kind, &properties, &mut current, &mut blocks);
                blocks.push(Block {
                    id: StableId::new("block"),
                    kind: BlockKind::PageBreak,
                    content: Vec::new(),
                    properties: BlockProperties::default(),
                });
            }
        }
    }
    flush_google_fragment(&kind, &properties, &mut current, &mut blocks);
    if split && blocks.len() > 1 {
        warnings.push(warning(
            google_style::SPLIT_PAGE_BREAK,
            "a Google Docs page break inside a paragraph was imported as a standalone page break block, splitting the paragraph",
        ));
    }
    if blocks.is_empty() {
        blocks.push(Block {
            id: StableId::new("block"),
            kind,
            content: Vec::new(),
            properties,
        });
    }
    Ok(blocks)
}

pub(crate) fn flush_google_fragment(
    kind: &BlockKind,
    properties: &BlockProperties,
    current: &mut Vec<Inline>,
    out: &mut Vec<Block>,
) {
    if current.is_empty() {
        return;
    }
    out.push(Block {
        id: StableId::new("block"),
        kind: kind.clone(),
        content: std::mem::take(current),
        properties: properties.clone(),
    });
}

/// A paragraph element, once read: either inline content or the page break
/// that splits the paragraph around it.
pub(crate) enum GoogleSegment {
    Inline(Inline),
    PageBreak,
}

/// Footnote, comment and suggestion bodies are inline-only, so a page break
/// inside one has nowhere to go. It is dropped by name, never silently.
pub(crate) fn google_segments_as_inlines(
    segments: Vec<GoogleSegment>,
    context: &str,
    warnings: &mut Vec<ModelWarning>,
) -> Vec<Inline> {
    let mut inlines = Vec::new();
    for segment in segments {
        match segment {
            GoogleSegment::Inline(inline) => inlines.push(inline),
            GoogleSegment::PageBreak => warnings.push(warning(
                google_style::DROPPED_PARAGRAPH_ELEMENT,
                &format!("a Google Docs page break inside a {context} was dropped"),
            )),
        }
    }
    inlines
}

pub(crate) fn import_google_paragraph_elements(
    paragraph: &Value,
    warnings: &mut Vec<ModelWarning>,
) -> Result<Vec<GoogleSegment>, ImportError> {
    let elements = paragraph
        .get("elements")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            ImportError::InvalidInput("paragraph.elements must be an array".to_string())
        })?;
    let mut inlines = Vec::new();
    for element in elements {
        if element.get("pageBreak").is_some() {
            inlines.push(GoogleSegment::PageBreak);
        } else if let Some(run) = element.get("textRun") {
            let text = run
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim_end_matches('\n')
                .to_string();
            if text.is_empty() {
                continue;
            }
            let style = optional_object(run, "textStyle")?.unwrap_or(&Value::Null);
            let marks = import_google_text_marks(style, warnings);
            if let Some(href) = import_google_text_link_href(style)? {
                inlines.push(GoogleSegment::Inline(opendoc_core::Inline::Link {
                    id: StableId::new("link"),
                    text,
                    href,
                    marks,
                }));
            } else {
                inlines.push(GoogleSegment::Inline(opendoc_core::Inline::Text {
                    id: StableId::new("text"),
                    text,
                    marks,
                }));
            }
        } else if element.get("footnoteReference").is_some() {
            let footnote_id = element
                .pointer("/footnoteReference/footnoteId")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    ImportError::UnsupportedStructure(
                        "Google Docs footnote reference missing footnoteId".to_string(),
                    )
                })?;
            inlines.push(GoogleSegment::Inline(Inline::FootnoteRef {
                id: StableId::new("footnote-ref"),
                footnote_id: parse_imported_stable_id(footnote_id)?,
            }));
        } else if let Some(equation) = element.get("opendocEquation") {
            warnings.push(ModelWarning {
                code: "opendoc-google-equation-extension".to_string(),
                message: "imported OpenDoc inline equation extension from Google Docs-shaped JSON"
                    .to_string(),
            });
            inlines.push(GoogleSegment::Inline(import_opendoc_inline_equation(
                equation,
            )?));
        } else if element.get("equation").is_some() {
            warnings.push(ModelWarning {
                code: "google-equation-source-unavailable".to_string(),
                message: "Google Docs API equation elements do not expose equation source"
                    .to_string(),
            });
            inlines.push(GoogleSegment::Inline(Inline::Equation {
                id: StableId::new("equation"),
                equation: Equation {
                    id: StableId::new("eq"),
                    source_format: EquationSourceFormat::LatexLike,
                    source: "\\placeholder{}".to_string(),
                },
            }));
        } else if let Some(citation) = element.get("opendocCitation") {
            warnings.push(ModelWarning {
                code: "opendoc-google-citation-extension".to_string(),
                message: "imported OpenDoc citation extension from Google Docs-shaped JSON"
                    .to_string(),
            });
            let citation_id = citation
                .get("citationId")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    ImportError::UnsupportedStructure(
                        "OpenDoc citation element missing citationId".to_string(),
                    )
                })?;
            inlines.push(GoogleSegment::Inline(Inline::Citation {
                id: StableId::new("citation-label"),
                citation_id: parse_imported_stable_id(citation_id)?,
                rendered_cache: citation
                    .get("renderedCache")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
            }));
        } else if let Some(mention) = element.get("opendocMention") {
            warnings.push(ModelWarning {
                code: "opendoc-google-mention-extension".to_string(),
                message: "imported OpenDoc mention extension from Google Docs-shaped JSON"
                    .to_string(),
            });
            let label = mention
                .get("label")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| {
                    ImportError::UnsupportedStructure(
                        "OpenDoc mention element missing label".to_string(),
                    )
                })?;
            let id = mention
                .get("inlineId")
                .and_then(Value::as_str)
                .map(parse_imported_stable_id)
                .transpose()?
                .unwrap_or_else(|| StableId::new("mention"));
            inlines.push(GoogleSegment::Inline(Inline::Mention {
                id,
                label: label.to_string(),
            }));
        } else if let Some(person) = element.get("person") {
            // A person chip is a name plus an email; OpenDoc has a mention
            // inline, so the text survives even though the chip does not.
            let label = person
                .pointer("/personProperties/name")
                .or_else(|| person.pointer("/personProperties/email"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|label| !label.is_empty());
            match label {
                Some(label) => {
                    warnings.push(warning(
                        google_style::DEGRADED_PARAGRAPH_ELEMENT,
                        "a Google Docs person chip was imported as a plain mention",
                    ));
                    inlines.push(GoogleSegment::Inline(Inline::Mention {
                        id: StableId::new("mention"),
                        label: label.to_string(),
                    }));
                }
                None => warnings.push(warning(
                    google_style::DROPPED_PARAGRAPH_ELEMENT,
                    "a Google Docs person chip carried neither a name nor an email and was dropped",
                )),
            }
        } else if let Some(rich_link) = element.get("richLink") {
            // A smart chip is a link with a title; keep both and say the chip
            // itself did not survive.
            let uri = rich_link
                .pointer("/richLinkProperties/uri")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|uri| !uri.is_empty());
            let title = rich_link
                .pointer("/richLinkProperties/title")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|title| !title.is_empty());
            match uri {
                Some(uri) => {
                    warnings.push(warning(
                        google_style::DEGRADED_PARAGRAPH_ELEMENT,
                        "a Google Docs rich link chip was imported as a plain link",
                    ));
                    inlines.push(GoogleSegment::Inline(Inline::Link {
                        id: StableId::new("link"),
                        text: title.unwrap_or(uri).to_string(),
                        href: uri.to_string(),
                        marks: Vec::new(),
                    }));
                }
                None => warnings.push(warning(
                    google_style::DROPPED_PARAGRAPH_ELEMENT,
                    "a Google Docs rich link chip carried no uri and was dropped",
                )),
            }
        } else if let Some((_, message)) = DROPPED_GOOGLE_PARAGRAPH_ELEMENTS
            .into_iter()
            .find(|(key, _)| element.get(key).is_some())
        {
            warnings.push(warning(google_style::DROPPED_PARAGRAPH_ELEMENT, message));
        } else {
            // An element this importer has never seen must not be fatal:
            // ADR 0003 asks import to degrade with a warning, and a Google
            // schema addition is not a corrupt document.
            let names = element
                .as_object()
                .map(|object| object.keys().cloned().collect::<Vec<_>>().join(", "))
                .unwrap_or_default();
            warnings.push(warning(
                google_style::DROPPED_PARAGRAPH_ELEMENT,
                &format!("unknown Google Docs paragraph element ({names}) was dropped"),
            ));
        }
    }
    Ok(inlines)
}

/// Google paragraph elements that are understood but have no OpenDoc model.
/// Each is dropped by name rather than aborting the import.
pub(crate) const DROPPED_GOOGLE_PARAGRAPH_ELEMENTS: [(&str, &str); 4] = [
    (
        "inlineObjectElement",
        "a Google Docs inline object (image or drawing) was dropped: the JSON carries only an object id, not the image bytes",
    ),
    (
        "horizontalRule",
        "a Google Docs horizontal rule was dropped: OpenDoc has no horizontal rule block",
    ),
    (
        "columnBreak",
        "a Google Docs column break was dropped: OpenDoc has no column model",
    ),
    (
        "autoText",
        "a Google Docs auto text field (page number or page count) was dropped: OpenDoc has no page field",
    ),
];

pub(crate) fn import_google_text_link_href(style: &Value) -> Result<Option<String>, ImportError> {
    let Some(link) = optional_object(style, "link")? else {
        return Ok(None);
    };
    let href = optional_str(link, "url")?.ok_or_else(|| {
        ImportError::UnsupportedStructure("Google Docs link missing url".to_string())
    })?;
    if href.trim().is_empty() {
        return Err(ImportError::UnsupportedStructure(
            "Google Docs link url is empty".to_string(),
        ));
    }
    if href.trim() != href {
        return Err(ImportError::UnsupportedStructure(
            "Google Docs link url has surrounding whitespace".to_string(),
        ));
    }
    Ok(Some(href.to_string()))
}

pub(crate) fn import_google_footnotes(
    value: &Value,
    warnings: &mut Vec<ModelWarning>,
) -> Result<Vec<Footnote>, ImportError> {
    let Some(footnotes_value) = value.get("footnotes") else {
        return Ok(Vec::new());
    };
    let footnotes = footnotes_value
        .as_object()
        .ok_or_else(|| ImportError::InvalidInput("footnotes must be an object".to_string()))?;
    let mut out = Vec::new();
    for (fallback_id, footnote) in footnotes {
        expect_object(footnote, "footnote")?;
        let id = optional_str(footnote, "footnoteId")?.unwrap_or(fallback_id);
        let content = required_array(footnote, "content", "footnote")?;
        let mut body = Vec::new();
        for element in content {
            expect_object(element, "footnote content element")?;
            if let Some(paragraph) = element.get("paragraph") {
                let mut inlines = google_segments_as_inlines(
                    import_google_paragraph_elements(paragraph, warnings)?,
                    "footnote",
                    warnings,
                );
                if !body.is_empty() && !body_ends_with_newline(&body) {
                    body.push(Inline::text("\n"));
                }
                body.append(&mut inlines);
            } else if element.get("startIndex").is_some() || element.get("endIndex").is_some() {
                warnings.push(ModelWarning {
                    code: "unsupported-google-footnote-metadata".to_string(),
                    message: "ignored Google Docs footnote metadata-only element".to_string(),
                });
            } else {
                return Err(ImportError::UnsupportedStructure(
                    "only paragraph footnote content is supported".to_string(),
                ));
            }
        }
        if body.is_empty() {
            body.push(Inline::text(" "));
        }
        out.push(Footnote {
            id: parse_imported_stable_id(id)?,
            revision: 1,
            body,
            deleted: false,
        });
    }
    out.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(out)
}

pub(crate) fn body_ends_with_newline(inlines: &[Inline]) -> bool {
    matches!(
        inlines.last(),
        Some(Inline::Text { text, .. }) if text.ends_with('\n')
    )
}

pub(crate) fn import_google_comments(
    value: &Value,
    warnings: &mut Vec<ModelWarning>,
) -> Result<Vec<CommentThread>, ImportError> {
    let Some(threads) = optional_array(value, "opendocComments")? else {
        return Ok(Vec::new());
    };
    warnings.push(ModelWarning {
        code: "opendoc-google-comments-extension".to_string(),
        message: "imported OpenDoc comments extension from Google Docs-shaped JSON".to_string(),
    });
    threads
        .iter()
        .map(|thread| {
            expect_object(thread, "comment thread")?;
            import_google_comment_thread(thread, warnings)
        })
        .collect()
}

pub(crate) fn import_google_comment_thread(
    value: &Value,
    warnings: &mut Vec<ModelWarning>,
) -> Result<CommentThread, ImportError> {
    let id = required_str(value, "id", "comment thread")?;
    let comments = value
        .get("comments")
        .and_then(Value::as_array)
        .ok_or_else(|| ImportError::InvalidInput("comment thread comments missing".to_string()))?
        .iter()
        .map(|comment| {
            expect_object(comment, "comment")?;
            import_google_comment(comment, warnings)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(CommentThread {
        id: parse_imported_stable_id(id)?,
        anchor: import_google_anchor(value.get("anchor").unwrap_or(&Value::Null))?,
        comments,
        deleted: optional_bool(value, "deleted")?.unwrap_or(false),
    })
}

pub(crate) fn import_google_comment(
    value: &Value,
    warnings: &mut Vec<ModelWarning>,
) -> Result<Comment, ImportError> {
    let id = required_str(value, "id", "comment")?;
    Ok(Comment {
        id: parse_imported_stable_id(id)?,
        author: required_source_string(value, "author", "comment", "comment author")?,
        body: import_google_inline_body(
            value.get("body").unwrap_or(&Value::Null),
            "comment",
            warnings,
        )?,
        created_at_ms: optional_u64(value, "createdAtMs")?.unwrap_or(0),
        deleted: optional_bool(value, "deleted")?.unwrap_or(false),
    })
}

pub(crate) fn import_google_suggestions(
    value: &Value,
    warnings: &mut Vec<ModelWarning>,
) -> Result<Vec<Suggestion>, ImportError> {
    let Some(suggestions) = optional_array(value, "opendocSuggestions")? else {
        return Ok(Vec::new());
    };
    warnings.push(ModelWarning {
        code: "opendoc-google-suggestions-extension".to_string(),
        message: "imported OpenDoc suggestions extension from Google Docs-shaped JSON".to_string(),
    });
    suggestions
        .iter()
        .map(|suggestion| {
            expect_object(suggestion, "suggestion")?;
            import_google_suggestion(suggestion, warnings)
        })
        .collect()
}

pub(crate) fn import_google_suggestion(
    value: &Value,
    warnings: &mut Vec<ModelWarning>,
) -> Result<Suggestion, ImportError> {
    let id = required_str(value, "id", "suggestion")?;
    let kind = value
        .get("kind")
        .ok_or_else(|| ImportError::InvalidInput("suggestion kind missing".to_string()))?;
    expect_object(kind, "suggestion kind")?;
    Ok(Suggestion {
        id: parse_imported_stable_id(id)?,
        author: required_source_string(value, "author", "suggestion", "suggestion author")?,
        kind: import_google_suggestion_kind(kind, warnings)?,
        state: import_google_suggestion_state(optional_str(value, "state")?.unwrap_or("proposed"))?,
        provenance: optional_array(value, "provenance")?
            .map(|items| {
                items
                    .iter()
                    .map(|item| {
                        let Some(raw) = item.as_str() else {
                            return Err(ImportError::InvalidInput(
                                "suggestion provenance entries must be strings".to_string(),
                            ));
                        };
                        source_string(raw, "suggestion provenance entry")
                    })
                    .collect()
            })
            .transpose()?
            .unwrap_or_default(),
    })
}

pub(crate) fn import_google_suggestion_kind(
    value: &Value,
    warnings: &mut Vec<ModelWarning>,
) -> Result<SuggestionKind, ImportError> {
    match required_str(value, "type", "suggestion kind")? {
        "insert" => Ok(SuggestionKind::Insert {
            anchor: import_google_anchor(value.get("anchor").unwrap_or(&Value::Null))?,
            content: import_google_inline_body(
                value.get("content").unwrap_or(&Value::Null),
                "suggestion",
                warnings,
            )?,
        }),
        "delete" => Ok(SuggestionKind::Delete {
            range: import_google_range(value.get("range").unwrap_or(&Value::Null))?,
        }),
        "format" => Ok(SuggestionKind::Format {
            range: import_google_range(value.get("range").unwrap_or(&Value::Null))?,
            marks: import_google_text_marks(
                value.get("textStyle").unwrap_or(&Value::Null),
                &mut Vec::new(),
            ),
        }),
        other => Err(ImportError::InvalidInput(format!(
            "unsupported suggestion kind {other}"
        ))),
    }
}

pub(crate) fn import_google_suggestion_state(value: &str) -> Result<SuggestionState, ImportError> {
    match value {
        "accepted" => Ok(SuggestionState::Accepted),
        "rejected" => Ok(SuggestionState::Rejected),
        "proposed" => Ok(SuggestionState::Proposed),
        other => Err(ImportError::InvalidInput(format!(
            "unsupported suggestion state {other}"
        ))),
    }
}

pub(crate) fn import_google_anchor(value: &Value) -> Result<Anchor, ImportError> {
    if value.is_null() {
        return Ok(Anchor::Document);
    }
    expect_object(value, "anchor")?;
    match optional_str(value, "type")?.unwrap_or("document") {
        "textRange" => Ok(Anchor::TextRange(import_google_range(value)?)),
        "nearestBlock" => {
            let block_id = required_str(value, "blockId", "nearest block anchor")?;
            Ok(Anchor::NearestBlock {
                block_id: parse_imported_stable_id(block_id)?,
                warning: optional_source_string(value, "warning", "nearest block anchor warning")?
                    .unwrap_or_else(|| "imported degraded anchor".to_string()),
            })
        }
        "document" => Ok(Anchor::Document),
        other => Err(ImportError::InvalidInput(format!(
            "unsupported anchor type {other}"
        ))),
    }
}

pub(crate) fn import_google_range(value: &Value) -> Result<TextRange, ImportError> {
    let start = required_str(value, "start", "text range")?;
    let end = required_str(value, "end", "text range")?;
    Ok(TextRange {
        start: parse_imported_stable_id(start)?,
        end: parse_imported_stable_id(end)?,
    })
}

pub(crate) fn import_google_inline_body(
    value: &Value,
    context: &str,
    warnings: &mut Vec<ModelWarning>,
) -> Result<Vec<Inline>, ImportError> {
    let elements = value.as_array().ok_or_else(|| {
        ImportError::InvalidInput("inline body must be an array of paragraph elements".to_string())
    })?;
    let paragraph = json!({ "elements": elements });
    let mut body = google_segments_as_inlines(
        import_google_paragraph_elements(&paragraph, warnings)?,
        context,
        warnings,
    );
    if body.is_empty() {
        body.push(Inline::text(" "));
    }
    Ok(body)
}

pub(crate) fn import_google_text_marks(
    style: &Value,
    warnings: &mut Vec<ModelWarning>,
) -> Vec<Mark> {
    let mut marks = Vec::new();
    let simple = [
        ("bold", MarkKind::Bold),
        ("italic", MarkKind::Italic),
        ("underline", MarkKind::Underline),
        ("strikethrough", MarkKind::Strike),
        ("baselineOffset", MarkKind::Superscript),
    ];
    for (key, kind) in simple {
        if key == "baselineOffset" {
            if style.get(key).and_then(Value::as_str) == Some("SUPERSCRIPT") {
                marks.push(mark(kind, None));
            } else if style.get(key).and_then(Value::as_str) == Some("SUBSCRIPT") {
                marks.push(mark(MarkKind::Subscript, None));
            }
        } else if style.get(key).and_then(Value::as_bool) == Some(true) {
            marks.push(mark(kind, None));
        }
    }
    if let Some(font) = style
        .pointer("/weightedFontFamily/fontFamily")
        .and_then(Value::as_str)
    {
        marks.push(mark(MarkKind::Font, Some(font.to_string())));
    }
    if let Some(size) = style.pointer("/fontSize/magnitude").and_then(Value::as_f64) {
        marks.push(mark(MarkKind::Size, Some(trim_float(size))));
    }
    if let Some(color) = style
        .pointer("/foregroundColor/color/rgbColor")
        .and_then(import_google_rgb)
    {
        marks.push(mark(MarkKind::Color, Some(color)));
    }
    if let Some(color) = style
        .pointer("/backgroundColor/color/rgbColor")
        .and_then(import_google_rgb)
    {
        marks.push(mark(MarkKind::Background, Some(color)));
    }
    if style
        .as_object()
        .is_some_and(|object| object.contains_key("smallCaps"))
    {
        warnings.push(ModelWarning {
            code: "unsupported-google-text-style".to_string(),
            message: "ignored unsupported Google Docs text style field".to_string(),
        });
    }
    marks
}

pub(crate) fn import_google_table(
    table: &Value,
    lists: &GoogleLists,
    warnings: &mut Vec<ModelWarning>,
) -> Result<Block, ImportError> {
    let rows = table
        .get("tableRows")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            ImportError::UnsupportedStructure("malformed Google Docs table".to_string())
        })?
        .iter()
        .map(|row| {
            let cells = row
                .get("tableCells")
                .and_then(Value::as_array)
                .ok_or_else(|| {
                    ImportError::UnsupportedStructure("malformed Google Docs table row".to_string())
                })?
                .iter()
                .map(|cell| {
                    let mut content = cell
                        .get("content")
                        .and_then(Value::as_array)
                        .ok_or_else(|| {
                            ImportError::UnsupportedStructure(
                                "malformed Google Docs table cell".to_string(),
                            )
                        })?
                        .iter()
                        .map(|element| {
                            import_google_structural_element(element, lists, warnings, false)
                        })
                        .collect::<Result<Vec<_>, _>>()?
                        .concat();
                    if content.is_empty() {
                        warnings.push(ModelWarning {
                            code: "google-empty-table-cell-normalized".to_string(),
                            message: "empty Google Docs table cell imported as a blank paragraph"
                                .to_string(),
                        });
                        content.push(Block::paragraph(""));
                    }
                    Ok(TableCell::new(content))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let cells = if cells.is_empty() {
                warnings.push(ModelWarning {
                    code: "google-empty-table-row-normalized".to_string(),
                    message: "empty Google Docs table row imported with a blank cell".to_string(),
                });
                vec![TableCell::empty()]
            } else {
                cells
            };
            Ok(TableRow {
                id: StableId::new("row"),
                cells,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let rows = if rows.is_empty() {
        warnings.push(ModelWarning {
            code: "google-empty-table-normalized".to_string(),
            message: "empty Google Docs table imported with a blank row and cell".to_string(),
        });
        vec![TableRow {
            id: StableId::new("row"),
            cells: vec![TableCell::empty()],
        }]
    } else {
        rows
    };
    Ok(Block {
        id: StableId::new("block"),
        kind: BlockKind::table(rows),
        content: Vec::new(),
        properties: BlockProperties::default(),
    })
}

pub(crate) fn import_google_structural_element(
    element: &Value,
    lists: &GoogleLists,
    warnings: &mut Vec<ModelWarning>,
    allow_tables: bool,
) -> Result<Vec<Block>, ImportError> {
    if let Some(paragraph) = element.get("paragraph") {
        import_google_paragraph(paragraph, lists, warnings)
    } else if let Some(table) = element.get("table") {
        if allow_tables {
            Ok(vec![import_google_table(table, lists, warnings)?])
        } else {
            Err(ImportError::UnsupportedStructure(
                "nested Google Docs tables are unsupported".to_string(),
            ))
        }
    } else if let Some(equation) = element.get("opendocEquationBlock") {
        Ok(vec![import_opendoc_equation_block(equation)?])
    } else if let Some(image) = element.get("opendocImage") {
        Ok(vec![import_opendoc_image(image)?])
    } else if let Some(section_break) = element.get("sectionBreak") {
        if google_part_is_present(section_break.get("sectionStyle")) {
            warnings.push(warning(
                google_style::DROPPED_DOCUMENT_PART,
                "Google Docs section styling (columns, margins, headers per section) is not representable; only the break itself was imported",
            ));
        }
        Ok(vec![Block {
            id: StableId::new("block"),
            kind: BlockKind::PageBreak,
            content: Vec::new(),
            properties: BlockProperties::default(),
        }])
    } else if let Some(toc) = element.get("tableOfContents") {
        // A table of contents is a generated projection OpenDoc does not model.
        // Its entries are ordinary structural elements, so the text survives as
        // plain blocks and the warning says the index is no longer live.
        warnings.push(warning(
            google_style::DROPPED_STRUCTURAL_ELEMENT,
            "a Google Docs table of contents was flattened into ordinary paragraphs: OpenDoc does not generate one",
        ));
        let mut blocks = Vec::new();
        for entry in optional_array(toc, "content")?.into_iter().flatten() {
            blocks.extend(import_google_structural_element(
                entry,
                lists,
                warnings,
                allow_tables,
            )?);
        }
        Ok(blocks)
    } else if element.get("startIndex").is_some() || element.get("endIndex").is_some() {
        warnings.push(ModelWarning {
            code: "unsupported-google-structural-element".to_string(),
            message: "ignored Google Docs structural metadata-only element".to_string(),
        });
        Ok(Vec::new())
    } else {
        let names = element
            .as_object()
            .map(|object| object.keys().cloned().collect::<Vec<_>>().join(", "))
            .unwrap_or_default();
        warnings.push(warning(
            google_style::DROPPED_STRUCTURAL_ELEMENT,
            &format!("unknown Google Docs structural element ({names}) was dropped"),
        ));
        Ok(Vec::new())
    }
}
