//! Reading Google Docs JSON into the canonical document model.

use crate::error::ImportError;
use crate::google_color::{import_google_rgb, trim_float};
use crate::google_export::named_block_style;
use crate::google_style;
use crate::google_style::{
    import_dimension, import_paragraph_style, report_unmapped_named_style, warning, GoogleLists,
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
    Anchor, Block, BlockKind, BlockProperties, Bookmark, BorderStyle, CellBorder, CellSpan, Color,
    Comment, CommentHistoryEntry, CommentThread, DropdownOption, Footnote, Inline, InsertPosition,
    Mark, MarkKind, ModelWarning, PageNumberField, ParagraphStyle, StableId, Suggestion,
    SuggestionKind, SuggestionState, TableCell, TableColumn, TableRow, TextRange,
    VerticalAlignment,
};
use serde_json::{json, Value};
use std::collections::BTreeSet;

// Google Docs' native `equation` element deliberately carries no equation
// source or rendered accessibility text in the Documents API. Do not infer
// either from adjacent runs or undocumented fields: OpenDoc's equation source
// is editable canonical content, so an invented value would be a false
// round-trip claim.
const GOOGLE_EQUATION_SOURCE_UNAVAILABLE: &str = "google-equation-source-unavailable";
const GOOGLE_EQUATION_SOURCE_UNAVAILABLE_MESSAGE: &str =
    "dropped Google Docs equation: the native API element exposes neither equation source nor accessible rendered text";

/// A Docs font-size `Dimension` is measured in points.  The text mark stores
/// a bare point value, so accepting a foreign unit would make a later export
/// claim the same rendered size when it is not.
const GOOGLE_DROPPED_TEXT_FONT_SIZE: &str = "google-dropped-text-font-size";
const GOOGLE_UNREPRESENTABLE_SMALL_CAPS: &str = "google-unrepresentable-small-caps";

/// Google table cells are structural containers and can contain tables. Keep
/// that useful native shape, but bound untrusted recursive JSON before it can
/// turn validation/layout into an unbounded tree walk. This matches the DOCX
/// reader's sixteen-level representation budget.
const MAX_GOOGLE_TABLE_DEPTH: usize = 16;

// `TableCellStyle.backgroundColor` is an OptionalColor.  Its absence inherits
// table styling, while a present OptionalColor without `color` explicitly
// clears the fill to transparent.  `TableCellProperties.background` only has
// `None` for the former, so collapsing these states would make a later export
// silently restore inheritance instead of the user's explicit clear.
const GOOGLE_TRANSPARENT_TABLE_BACKGROUND_UNREPRESENTABLE: &str =
    "google-unrepresentable-transparent-table-background";

/// The body source interval that produced an imported top-level block. Google
/// bookmarks use UTF-16 document indices; OpenDoc deliberately projects their
/// position to a stable block target, not a character offset that would become
/// stale under concurrent edits.
#[derive(Clone, Debug)]
pub(crate) struct GoogleBlockRange {
    pub(crate) start: u64,
    pub(crate) end: u64,
    pub(crate) block_id: StableId,
}

/// One Google paragraph becomes one OpenDoc block, except when a `pageBreak` or
/// `horizontalRule` element sits inside it: OpenDoc models these as own blocks, so the
/// paragraph splits around it, exactly as the DOCX reader does.
pub(crate) fn import_google_paragraph(
    paragraph: &Value,
    lists: &GoogleLists,
    warnings: &mut Vec<ModelWarning>,
) -> Result<Vec<Block>, ImportError> {
    let style = optional_object(paragraph, "paragraphStyle")?.unwrap_or(&Value::Null);
    let named_style = optional_str(style, "namedStyleType")?.unwrap_or_default();
    let bullet = optional_object(paragraph, "bullet")?;
    // `pageBreakBefore` is not paragraph decoration in OpenDoc: it is the
    // existing standalone physical boundary immediately before this block.
    // Keeping it structural means layout, PDF and every other exporter see
    // the same break rather than one Google-only formatting bit.
    let page_break_before = match optional_bool(style, "pageBreakBefore")? {
        Some(true) => true,
        // `false` is an explicit override in a Docs style chain. OpenDoc's
        // structural PageBreak can express only a positive boundary; silently
        // treating false as an absent inherited value would lose that fact.
        Some(false) => {
            warnings.push(warning(
                google_style::DROPPED_PARAGRAPH_STYLE,
                "Google Docs explicit pageBreakBefore=false cannot be represented by OpenDoc's positive structural PageBreak and was dropped",
            ));
            false
        }
        None => false,
    };
    let properties = import_paragraph_style(style, warnings)?;
    let kind = if let Some(kind) = named_block_style(named_style) {
        kind
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
    let has_source_segments = !segments.is_empty();
    let split = segments
        .iter()
        .any(|segment| !matches!(segment, GoogleSegment::Inline(_)));
    let mut blocks = Vec::new();
    if page_break_before {
        blocks.push(Block {
            id: StableId::new("block"),
            kind: BlockKind::PageBreak,
            content: Vec::new(),
            properties: BlockProperties::default(),
        });
    }
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
            GoogleSegment::HorizontalRule => {
                flush_google_fragment(&kind, &properties, &mut current, &mut blocks);
                blocks.push(Block {
                    id: StableId::new("block"),
                    kind: BlockKind::HorizontalRule,
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
    // A source paragraph with only its terminator still becomes an empty
    // paragraph. A `pageBreak`/`horizontalRule` element already is the
    // paragraph's standalone structural content, however, so retain the
    // existing no-extra-empty-block behavior for those shapes.
    if blocks.is_empty() || (page_break_before && !has_source_segments) {
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
    HorizontalRule,
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
            GoogleSegment::HorizontalRule => warnings.push(warning(
                google_style::DROPPED_PARAGRAPH_ELEMENT,
                &format!("a Google Docs horizontal rule inside a {context} was dropped"),
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
    // In the native Docs representation the paragraph terminator is the
    // final newline of its final text run.  Earlier text runs can legitimately
    // end in a hard line break, so trimming every run (or every trailing
    // newline) loses source text.  Remember the one terminal run and remove
    // at most its one structural terminator below.
    let terminal_text_run = elements
        .last()
        .is_some_and(|element| element.get("textRun").is_some())
        .then(|| elements.len() - 1);
    let mut inlines = Vec::new();
    for (element_index, element) in elements.iter().enumerate() {
        if element.get("pageBreak").is_some() {
            inlines.push(GoogleSegment::PageBreak);
        } else if element.get("horizontalRule").is_some() {
            inlines.push(GoogleSegment::HorizontalRule);
        } else if let Some(run) = element.get("textRun") {
            let content = run
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let text = if terminal_text_run == Some(element_index) {
                content.strip_suffix('\n').unwrap_or(content)
            } else {
                content
            }
            .to_string();
            if text.is_empty() {
                continue;
            }
            let style = optional_object(run, "textStyle")?.unwrap_or(&Value::Null);
            let marks = import_google_text_marks(style, warnings);
            if let Some(href) = import_google_text_link_href(style, warnings)? {
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
        } else if let Some(auto_text) = element.get("autoText") {
            // Google names the two pagination-derived fields directly.  They
            // have the same meaning as OpenDoc's `PageNumberField`: retain
            // the field kind, never a source-rendered number.  The optional
            // id is our own export aid; native Docs objects have no durable
            // inline identity, so a missing one gets a fresh local id.
            let Some(auto_text) = auto_text.as_object() else {
                warnings.push(warning(
                    google_style::DROPPED_PARAGRAPH_ELEMENT,
                    "a malformed Google Docs auto text field was dropped",
                ));
                continue;
            };
            let field = match auto_text.get("type").and_then(Value::as_str) {
                Some("PAGE_NUMBER") => Some(PageNumberField::CurrentPage),
                Some("PAGE_COUNT") => Some(PageNumberField::PageCount),
                Some(other) => {
                    warnings.push(warning(
                        google_style::DROPPED_PARAGRAPH_ELEMENT,
                        &format!(
                            "a Google Docs auto text field of unsupported type {other:?} was dropped"
                        ),
                    ));
                    None
                }
                None => {
                    warnings.push(warning(
                        google_style::DROPPED_PARAGRAPH_ELEMENT,
                        "a Google Docs auto text field missing its type was dropped",
                    ));
                    None
                }
            };
            if let Some(field) = field {
                let id = auto_text
                    .get("opendocInlineId")
                    .and_then(Value::as_str)
                    .map(parse_imported_stable_id)
                    .transpose()?
                    .unwrap_or_else(|| StableId::new("page-number"));
                inlines.push(GoogleSegment::Inline(Inline::PageNumber { id, field }));
            }
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
                code: GOOGLE_EQUATION_SOURCE_UNAVAILABLE.to_string(),
                message: GOOGLE_EQUATION_SOURCE_UNAVAILABLE_MESSAGE.to_string(),
            });
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
        } else if let Some(dropdown) = element.get("opendocDropdown") {
            let options = dropdown
                .get("options")
                .and_then(Value::as_array)
                .ok_or_else(|| {
                    ImportError::UnsupportedStructure(
                        "OpenDoc dropdown element missing options".to_string(),
                    )
                })?
                .iter()
                .map(|option| {
                    Ok(DropdownOption {
                        id: option
                            .get("id")
                            .and_then(Value::as_str)
                            .ok_or_else(|| {
                                ImportError::UnsupportedStructure(
                                    "OpenDoc dropdown option missing id".to_string(),
                                )
                            })?
                            .to_string(),
                        label: option
                            .get("label")
                            .and_then(Value::as_str)
                            .ok_or_else(|| {
                                ImportError::UnsupportedStructure(
                                    "OpenDoc dropdown option missing label".to_string(),
                                )
                            })?
                            .to_string(),
                    })
                })
                .collect::<Result<Vec<_>, ImportError>>()?;
            let selected_option_id = dropdown
                .get("selectedOptionId")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    ImportError::UnsupportedStructure(
                        "OpenDoc dropdown element missing selectedOptionId".to_string(),
                    )
                })?
                .to_string();
            let id = dropdown
                .get("inlineId")
                .and_then(Value::as_str)
                .map(parse_imported_stable_id)
                .transpose()?
                .unwrap_or_else(|| StableId::new("dropdown"));
            let inline = Inline::Dropdown {
                id,
                options,
                selected_option_id,
            };
            inline
                .validate()
                .map_err(|error| ImportError::UnsupportedStructure(error.to_string()))?;
            warnings.push(ModelWarning {
                code: "opendoc-google-dropdown-extension".to_string(),
                message: "imported OpenDoc dropdown extension from Google Docs-shaped JSON"
                    .to_string(),
            });
            inlines.push(GoogleSegment::Inline(inline));
        } else if let Some(date_chip) = element.get("opendocDateChip") {
            let date = date_chip
                .get("date")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    ImportError::UnsupportedStructure(
                        "OpenDoc date chip element missing date".to_string(),
                    )
                })?;
            let id = date_chip
                .get("inlineId")
                .and_then(Value::as_str)
                .map(parse_imported_stable_id)
                .transpose()?
                .unwrap_or_else(|| StableId::new("date-chip"));
            let inline = Inline::DateChip {
                id,
                date: date.to_string(),
            };
            inline
                .validate()
                .map_err(|error| ImportError::UnsupportedStructure(error.to_string()))?;
            warnings.push(ModelWarning {
                code: "opendoc-google-date-chip-extension".to_string(),
                message: "imported OpenDoc date chip extension from Google Docs-shaped JSON"
                    .to_string(),
            });
            inlines.push(GoogleSegment::Inline(inline));
        } else if let Some(person) = element.get("person") {
            // Preserve the identity supplied by the source, but never resolve
            // it: opening a document must not fetch a profile or disclose it.
            let label = person
                .pointer("/personProperties/name")
                .or_else(|| person.pointer("/personProperties/email"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|label| !label.is_empty());
            let email = person
                .pointer("/personProperties/email")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|email| !email.is_empty());
            let person_id = person
                .pointer("/personProperties/personId")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(str::to_string);
            match (label, email) {
                (Some(label), Some(email)) => {
                    inlines.push(GoogleSegment::Inline(Inline::GooglePersonChip {
                        id: StableId::new("google-person-chip"),
                        label: label.to_string(),
                        email: email.to_string(),
                        person_id,
                    }));
                }
                _ => warnings.push(warning(
                    google_style::DROPPED_PARAGRAPH_ELEMENT,
                    "a Google Docs person chip carried no usable label and email and was dropped",
                )),
            }
        } else if let Some(rich_link) = element.get("richLink") {
            // Keep provider metadata opaque and offline. The URI is still the
            // readable/exportable fallback if a consumer cannot draw a chip.
            // It is nevertheless navigation data in the desktop renderer, so
            // it must cross the exact same scheme/control-character boundary
            // as a textStyle Link.  A rich-link chip is not a safe place to
            // preserve an executable URI merely because it has a title.
            let uri = rich_link
                .pointer("/richLinkProperties/uri")
                .and_then(Value::as_str)
                .filter(|uri| !uri.is_empty());
            let title = rich_link
                .pointer("/richLinkProperties/title")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|title| !title.is_empty());
            let rich_link_id = rich_link
                .get("richLinkId")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(str::to_string);
            let mime_type = rich_link
                .pointer("/richLinkProperties/mimeType")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|mime| !mime.is_empty())
                .map(str::to_string);
            match (title, uri) {
                (Some(title), Some(uri)) => {
                    if uri.trim() != uri || !google_external_link_href_is_safe(uri) {
                        warnings.push(warning(
                            google_style::DROPPED_UNSAFE_EXTERNAL_LINK,
                            "a Google Docs rich-link chip was imported as plain text because its URI uses an unsafe navigation scheme",
                        ));
                        inlines.push(GoogleSegment::Inline(Inline::Text {
                            id: StableId::new("text"),
                            text: title.to_string(),
                            marks: Vec::new(),
                        }));
                    } else {
                        inlines.push(GoogleSegment::Inline(Inline::GoogleRichLinkChip {
                            id: StableId::new("google-rich-link-chip"),
                            label: title.to_string(),
                            href: uri.to_string(),
                            rich_link_id,
                            mime_type,
                        }));
                    }
                }
                _ => warnings.push(warning(
                    google_style::DROPPED_PARAGRAPH_ELEMENT,
                    "a Google Docs rich link chip carried no usable title and uri and was dropped",
                )),
            }
        } else if let Some(date_element) = element.get("dateElement") {
            if let Some(inline) = import_google_date_element(date_element, warnings)? {
                inlines.push(GoogleSegment::Inline(inline));
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

/// Imports the deliberately small native Google date-element subset that has
/// the same durable meaning as OpenDoc's calendar-only [`Inline::DateChip`].
///
/// A DateElement can also carry a time zone, locale, date/time display format,
/// text marks, and suggested changes.  `DateChip` intentionally has none of
/// those knobs, so accepting them would make an ISO calendar date look like a
/// lossless source record.  When Google supplies a displayed fallback, retain
/// that visible text (and its ordinary text marks) rather than dropping it.
fn import_google_date_element(
    date_element: &Value,
    warnings: &mut Vec<ModelWarning>,
) -> Result<Option<Inline>, ImportError> {
    let Some(properties) = date_element
        .get("dateElementProperties")
        .and_then(Value::as_object)
    else {
        warnings.push(warning(
            google_style::DROPPED_PARAGRAPH_ELEMENT,
            "a Google Docs date element without properties was dropped",
        ));
        return Ok(None);
    };
    let properties = Value::Object(properties.clone());
    let display_text = properties
        .get("displayText")
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty());
    let timestamp = properties.get("timestamp").and_then(Value::as_str);
    let iso_date = timestamp.and_then(|timestamp| timestamp.strip_suffix("T00:00:00Z"));
    let has_supported_zone = matches!(
        properties.get("timeZoneId").and_then(Value::as_str),
        None | Some("Etc/UTC") | Some("etc/UTC") | Some("UTC")
    );
    let has_supported_date_format = matches!(
        properties.get("dateFormat").and_then(Value::as_str),
        Some("DATE_FORMAT_ISO8601")
    );
    let has_supported_time_format = matches!(
        properties.get("timeFormat").and_then(Value::as_str),
        None | Some("TIME_FORMAT_DISABLED") | Some("TIME_FORMAT_UNSPECIFIED")
    );
    let has_text_style = date_element
        .get("textStyle")
        .and_then(Value::as_object)
        .is_some_and(|style| style.values().any(|value| !value.is_null()));
    let has_suggestions = [
        "suggestedInsertionIds",
        "suggestedDeletionIds",
        "suggestedTextStyleChanges",
        "suggestedDateElementPropertiesChanges",
    ]
    .into_iter()
    .any(|key| google_part_is_present(date_element.get(key)));
    let exact = iso_date.is_some_and(|date| {
        Inline::DateChip {
            id: StableId::new("date-chip-validation"),
            date: date.to_string(),
        }
        .validate()
        .is_ok()
            && display_text.is_none_or(|display| display == date)
    }) && has_supported_zone
        && has_supported_date_format
        && has_supported_time_format
        && !has_text_style
        && !has_suggestions;
    if exact {
        let id = date_element
            .get("dateId")
            .and_then(Value::as_str)
            .map(parse_imported_stable_id)
            .transpose()?
            .unwrap_or_else(|| StableId::new("date-chip"));
        return Ok(Some(Inline::DateChip {
            id,
            date: iso_date.expect("exact requires an ISO date").to_string(),
        }));
    }

    let reason = if has_suggestions {
        "suggested date changes"
    } else if has_text_style {
        "text styling"
    } else {
        "a timestamp, locale, time zone, or display format outside OpenDoc's ISO UTC date-only subset"
    };
    warnings.push(warning(
        "google-date-element-degraded",
        &format!(
            "a Google Docs date element with {reason} could not be retained as an atomic OpenDoc date chip"
        ),
    ));
    let Some(display_text) = display_text else {
        warnings.push(warning(
            google_style::DROPPED_PARAGRAPH_ELEMENT,
            "a nonrepresentable Google Docs date element had no displayText fallback and was dropped",
        ));
        return Ok(None);
    };
    let style = date_element.get("textStyle").unwrap_or(&Value::Null);
    let marks = import_google_text_marks(style, warnings);
    Ok(Some(Inline::Text {
        id: StableId::new("text"),
        text: display_text.to_string(),
        marks,
    }))
}

/// Google paragraph elements that are understood but have no OpenDoc model.
/// Each is dropped by name rather than aborting the import.
pub(crate) const DROPPED_GOOGLE_PARAGRAPH_ELEMENTS: [(&str, &str); 2] = [
    (
        "inlineObjectElement",
        "a Google Docs inline object (image or drawing) was dropped: the JSON carries only an object id, not the image bytes",
    ),
    (
        "columnBreak",
        "a Google Docs column break was dropped: OpenDoc has no column model",
    ),
];

pub(crate) fn import_google_text_link_href(
    style: &Value,
    warnings: &mut Vec<ModelWarning>,
) -> Result<Option<String>, ImportError> {
    let Some(link) = optional_object(style, "link")? else {
        return Ok(None);
    };
    // Validate the source one-of before accepting a URL. In particular, a
    // malformed source must not smuggle an internal destination past this
    // reader merely because `url` happens to be present too.
    let internal_destination = google_internal_link_destination(link)?;
    if let Some(href) = optional_str(link, "url")? {
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
        if !google_external_link_href_is_safe(href) {
            warnings.push(warning(
                google_style::DROPPED_UNSAFE_EXTERNAL_LINK,
                "Google Docs external link was imported as plain text because its URL uses an unsafe navigation scheme",
            ));
            return Ok(None);
        }
        return Ok(Some(href.to_string()));
    }

    // `Link.destination` is a native one-of. The legacy bookmarkId and
    // headingId forms occur in single-tab responses; current tab-aware
    // responses use the nested bookmark/heading forms. None map safely to an
    // OpenDoc href because their identifiers only make sense within the
    // source document. Preserve visible text and other marks while naming the
    // lost navigation target.
    if let Some(destination) = internal_destination {
        warnings.push(warning(
            google_style::DROPPED_INTERNAL_LINK,
            &format!(
                "Google Docs internal {destination} link was imported as plain text: OpenDoc has no durable internal-link target"
            ),
        ));
        return Ok(None);
    }

    Err(ImportError::UnsupportedStructure(
        "Google Docs link missing url or a recognized internal destination".to_string(),
    ))
}

/// Whether an external URL can be represented by OpenDoc's navigation link.
///
/// This is deliberately a navigation policy rather than a URL parser. Google
/// Docs can store opaque URL strings, but OpenDoc's desktop editor opens an
/// href on a modifier click. Keep ordinary absolute and relative web links
/// while refusing executable/unknown schemes. Rejecting controls also avoids
/// browser URL normalisation changing what the user inspected in the source.
pub(crate) fn google_external_link_href_is_safe(href: &str) -> bool {
    if href.is_empty() || href.chars().any(char::is_control) {
        return false;
    }
    let lowered = href.to_ascii_lowercase();
    if lowered.starts_with('#') || lowered.starts_with('/') || lowered.starts_with('.') {
        return true;
    }
    match lowered.split_once(':') {
        Some((scheme, _)) => matches!(scheme, "http" | "https" | "mailto" | "tel" | "ftp"),
        // A URL with no scheme is a relative reference.
        None => true,
    }
}

/// Returns the source-native destination name after validating the exact
/// active destination shape. This is deliberately narrow: an arbitrary link
/// object without `url` remains an import error instead of being silently
/// downgraded as though it were a Google internal link.
fn google_internal_link_destination(link: &Value) -> Result<Option<&'static str>, ImportError> {
    let direct_destinations = [
        ("tabId", "tab"),
        ("bookmarkId", "bookmark"),
        ("headingId", "heading"),
    ];
    let nested_destinations = [("bookmark", "bookmark"), ("heading", "heading")];
    let destinations = usize::from(link.get("url").is_some())
        + direct_destinations
            .iter()
            .filter(|(key, _)| link.get(*key).is_some())
            .count()
        + nested_destinations
            .iter()
            .filter(|(key, _)| link.get(*key).is_some())
            .count();
    if destinations > 1 {
        return Err(ImportError::UnsupportedStructure(
            "Google Docs link has multiple destinations".to_string(),
        ));
    }
    if let Some((key, destination)) = direct_destinations
        .into_iter()
        .find(|(key, _)| link.get(*key).is_some())
    {
        validate_google_internal_link_id(link, key)?;
        return Ok(Some(destination));
    }
    if let Some((key, destination)) = nested_destinations
        .into_iter()
        .find(|(key, _)| link.get(*key).is_some())
    {
        let target = link
            .get(key)
            .expect("the preceding filter establishes the target exists");
        if !target.is_object() {
            return Err(ImportError::InvalidInput(format!(
                "{key} must be an object"
            )));
        }
        validate_google_internal_link_id(target, "id")?;
        if target.get("tabId").is_some() {
            validate_google_internal_link_id(target, "tabId")?;
        }
        return Ok(Some(destination));
    }
    Ok(None)
}

fn validate_google_internal_link_id(value: &Value, key: &str) -> Result<(), ImportError> {
    let id = value.get(key).and_then(Value::as_str).ok_or_else(|| {
        ImportError::InvalidInput(format!("Google Docs internal link {key} must be a string"))
    })?;
    if id.trim().is_empty() {
        return Err(ImportError::UnsupportedStructure(format!(
            "Google Docs internal link {key} is empty"
        )));
    }
    if id.trim() != id {
        return Err(ImportError::UnsupportedStructure(format!(
            "Google Docs internal link {key} has surrounding whitespace"
        )));
    }
    Ok(())
}

pub(crate) fn import_google_footnotes(
    value: &Value,
    lists: &GoogleLists,
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
                // The Docs API uses the same StructuralElement union here as
                // in the document body.  A footnote cannot own OpenDoc
                // blocks, so retain the structural element's readable
                // content in its inline-only body instead of rejecting an
                // otherwise valid document.  Import it through the ordinary
                // structural reader first: malformed table JSON and invalid
                // extension payloads remain hard errors rather than being
                // disguised as a lossy fallback.
                let blocks = import_google_structural_element(element, lists, warnings, 0)?;
                for block in &blocks {
                    append_google_footnote_block(block, &mut body, warnings);
                }
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

/// Append an imported block to an inline-only Google footnote body.
///
/// Docs footnotes use `StructuralElement`, while OpenDoc intentionally keeps
/// footnote source inline-only.  Paragraph text is retained verbatim by the
/// caller; tables are projected to their readable tab/newline grid, equations
/// remain equations, and block-only objects get a visible bounded marker.
/// This keeps a source document importable without pretending its footnote
/// retained an editable block tree.
fn append_google_footnote_block(
    block: &Block,
    body: &mut Vec<Inline>,
    warnings: &mut Vec<ModelWarning>,
) {
    match &block.kind {
        BlockKind::Table { rows, .. } => {
            warnings.push(warning(
                "google-footnote-table-flattened",
                "a Google Docs table in a footnote was flattened into tab- and newline-separated text because OpenDoc footnote bodies are inline-only",
            ));
            append_google_footnote_separator(body);
            for (row_index, row) in rows.iter().enumerate() {
                if row_index > 0 {
                    append_google_footnote_separator(body);
                }
                for (cell_index, cell) in row.cells.iter().enumerate() {
                    if cell_index > 0 {
                        body.push(Inline::text("\t"));
                    }
                    // Build the cell locally so its first paragraph does not
                    // mistake the preceding cell's tab for paragraph source
                    // that needs a newline separator.
                    let mut cell_body = Vec::new();
                    for child in &cell.blocks {
                        append_google_footnote_block(child, &mut cell_body, warnings);
                    }
                    body.extend(cell_body);
                }
            }
        }
        BlockKind::EquationBlock { equation } => {
            warnings.push(warning(
                "google-footnote-block-flattened",
                "a block equation in a Google Docs footnote was converted to an inline equation because OpenDoc footnote bodies are inline-only",
            ));
            append_google_footnote_separator(body);
            body.push(Inline::Equation {
                id: StableId::new("equation"),
                equation: equation.clone(),
            });
        }
        BlockKind::Image { alt_text, .. } => {
            warnings.push(warning(
                "google-footnote-block-fallback",
                "an image in a Google Docs footnote was replaced with an explicit text marker because OpenDoc footnote bodies cannot embed images",
            ));
            append_google_footnote_separator(body);
            body.push(Inline::text(format!("[Image: {alt_text}]")));
        }
        BlockKind::HorizontalRule => {
            append_google_footnote_marker(
                "Horizontal rule",
                "a horizontal rule in a Google Docs footnote was replaced with an explicit text marker because OpenDoc footnote bodies are inline-only",
                body,
                warnings,
            );
        }
        BlockKind::PageBreak => {
            append_google_footnote_marker(
                "Page break",
                "a page break in a Google Docs footnote was replaced with an explicit text marker because OpenDoc footnote bodies are inline-only",
                body,
                warnings,
            );
        }
        BlockKind::TableOfContents { .. } => {
            append_google_footnote_marker(
                "Table of contents",
                "a generated table of contents in a Google Docs footnote was replaced with an explicit text marker because OpenDoc footnote bodies are inline-only",
                body,
                warnings,
            );
        }
        BlockKind::Bibliography => {
            append_google_footnote_marker(
                "Bibliography",
                "a generated bibliography in a Google Docs footnote was replaced with an explicit text marker because OpenDoc footnote bodies are inline-only",
                body,
                warnings,
            );
        }
        BlockKind::Paragraph
        | BlockKind::Title
        | BlockKind::Subtitle
        | BlockKind::Heading { .. }
        | BlockKind::ListItem { .. } => {
            append_google_footnote_separator(body);
            body.extend(block.content.iter().cloned());
        }
    }
}

fn append_google_footnote_marker(
    marker: &str,
    message: &str,
    body: &mut Vec<Inline>,
    warnings: &mut Vec<ModelWarning>,
) {
    warnings.push(warning("google-footnote-block-fallback", message));
    append_google_footnote_separator(body);
    body.push(Inline::text(format!("[{marker}]")));
}

fn append_google_footnote_separator(body: &mut Vec<Inline>) {
    if !body.is_empty() && !body_ends_with_newline(body) {
        body.push(Inline::text("\n"));
    }
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

/// Reads OpenDoc's append-only per-comment review evidence. Google Docs has
/// no portable history API for this information, so this is intentionally an
/// extension rather than a claim about native Google comment metadata.
pub(crate) fn import_google_comment_history(
    value: &Value,
    warnings: &mut Vec<ModelWarning>,
) -> Result<Vec<CommentHistoryEntry>, ImportError> {
    let Some(entries) = optional_array(value, "opendocCommentHistory")? else {
        return Ok(Vec::new());
    };
    warnings.push(ModelWarning {
        code: "opendoc-google-comment-history-extension".to_string(),
        message: "imported OpenDoc comment history extension from Google Docs-shaped JSON"
            .to_string(),
    });
    entries
        .iter()
        .map(|entry| {
            expect_object(entry, "comment history entry")?;
            let previous_body = entry
                .get("previousBody")
                .map(|body| import_google_inline_body(body, "comment history", warnings))
                .transpose()?;
            Ok(CommentHistoryEntry {
                thread_id: parse_imported_stable_id(required_str(
                    entry,
                    "threadId",
                    "comment history entry",
                )?)?,
                comment_id: parse_imported_stable_id(required_str(
                    entry,
                    "commentId",
                    "comment history entry",
                )?)?,
                kind: required_source_string(
                    entry,
                    "kind",
                    "comment history entry",
                    "comment history kind",
                )?,
                actor: required_source_string(
                    entry,
                    "actor",
                    "comment history entry",
                    "comment history actor",
                )?,
                at_ms: optional_u64(entry, "atMs")?.ok_or_else(|| {
                    ImportError::InvalidInput("comment history entry missing atMs".to_string())
                })?,
                previous_body,
            })
        })
        .collect()
}

/// Reads OpenDoc's lossless JSON extension. Google Docs' public resource does
/// not expose a portable bookmark object with a stable block target, so native
/// Google bookmark data is never guessed at here.
pub(crate) fn import_google_bookmarks(
    value: &Value,
    warnings: &mut Vec<ModelWarning>,
) -> Result<Vec<Bookmark>, ImportError> {
    let Some(bookmarks) = optional_array(value, "opendocBookmarks")? else {
        return Ok(Vec::new());
    };
    warnings.push(ModelWarning {
        code: "opendoc-google-bookmarks-extension".to_string(),
        message: "imported OpenDoc bookmarks extension from Google Docs-shaped JSON".to_string(),
    });
    let mut imported = Vec::with_capacity(bookmarks.len());
    for value in bookmarks {
        expect_object(value, "bookmark")?;
        let bookmark = Bookmark {
            id: parse_imported_stable_id(required_str(value, "id", "bookmark")?)?,
            name: required_source_string(value, "name", "bookmark", "bookmark name")?,
            block_id: parse_imported_stable_id(required_str(value, "blockId", "bookmark")?)?,
            revision: optional_u64(value, "revision")?.unwrap_or(1),
            deleted: optional_bool(value, "deleted")?.unwrap_or(false),
        };
        bookmark
            .validate()
            .map_err(|error| ImportError::InvalidInput(format!("invalid bookmark: {error}")))?;
        imported.push(bookmark);
    }
    imported.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(imported)
}

/// Projects the one native Google Docs bookmark shape which has an exact
/// stable-block analogue: a zero-width body position at the beginning of one
/// imported structural element. OpenDoc intentionally names a block, rather
/// than carrying a mutable UTF-16 offset. Interior offsets and supplied ranges
/// are consequently left unimported with a loss warning, rather than moving a
/// character-range bookmark to an entire block. Header/footer and footnote
/// segment positions are likewise left named rather than guessed against the
/// body.
pub(crate) fn import_native_google_bookmarks(
    value: &Value,
    block_ranges: &[GoogleBlockRange],
    inline_image_split_ranges: &[(u64, u64)],
    occupied_names: &BTreeSet<String>,
    occupied_ids: &BTreeSet<StableId>,
    warnings: &mut Vec<ModelWarning>,
) -> Result<Vec<Bookmark>, ImportError> {
    let Some(raw_bookmarks) = value.get("bookmarks") else {
        return Ok(Vec::new());
    };
    let raw_bookmarks = raw_bookmarks
        .as_object()
        .ok_or_else(|| ImportError::InvalidInput("bookmarks must be an object".to_string()))?;
    let mut entries = raw_bookmarks.iter().collect::<Vec<_>>();
    entries.sort_by_key(|(id, _)| *id);
    let mut names = occupied_names.clone();
    let mut ids = occupied_ids.clone();
    let mut imported = Vec::new();
    for (ordinal, (map_id, bookmark)) in entries.into_iter().enumerate() {
        expect_object(bookmark, "Google bookmark")?;
        let id = optional_str(bookmark, "bookmarkId")?.unwrap_or(map_id);
        let Some(position) = optional_object(bookmark, "position")? else {
            warnings.push(warning(
                "google-bookmark-unplaced",
                "a Google bookmark without a body position was not imported",
            ));
            continue;
        };
        if optional_str(position, "segmentId")?.is_some() {
            warnings.push(warning(
                "google-bookmark-nonbody-segment",
                "a Google bookmark in a non-body segment was not imported",
            ));
            continue;
        }
        let Some(index) = optional_u64(position, "index")? else {
            warnings.push(warning(
                "google-bookmark-unplaced",
                "a Google bookmark without an index was not imported",
            ));
            continue;
        };
        // The public Bookmark resource is zero-width. Accept a producer's
        // optional endIndex only when it confirms that shape; a non-empty
        // range cannot honestly become one durable block target.
        if let Some(end_index) = optional_u64(bookmark, "endIndex")? {
            if end_index != index {
                warnings.push(warning(
                    "google-bookmark-character-range-unrepresentable",
                    "a Google bookmark with a character range was not imported because OpenDoc bookmarks name whole blocks",
                ));
                continue;
            }
        }
        if inline_image_split_ranges
            .iter()
            .any(|(start, end)| *start <= index && index < *end)
        {
            warnings.push(warning(
                "google-bookmark-inline-image-split-unrepresentable",
                "a Google bookmark in a paragraph split around an inline image was not imported because its source position cannot be mapped safely to one block",
            ));
            continue;
        }
        let Some(target) = block_ranges.iter().find(|range| range.start == index) else {
            let code = if block_ranges
                .iter()
                .any(|range| range.start < index && index < range.end)
            {
                "google-bookmark-offset-unrepresentable"
            } else {
                "google-bookmark-unplaced"
            };
            let message = if code == "google-bookmark-offset-unrepresentable" {
                "a Google bookmark inside a block was not imported because OpenDoc bookmarks cannot retain character offsets"
            } else {
                "a Google bookmark outside imported body content was not imported"
            };
            warnings.push(warning(code, message));
            continue;
        };
        let mut name = format!("google-bookmark-{}", ordinal + 1);
        let mut suffix = 2usize;
        while names.contains(&name) {
            name = format!("google-bookmark-{}-{suffix}", ordinal + 1);
            suffix += 1;
        }
        names.insert(name.clone());
        let bookmark = Bookmark {
            id: parse_imported_stable_id(id).unwrap_or_else(|_| StableId::new("google-bookmark")),
            name,
            block_id: target.block_id.clone(),
            revision: 1,
            deleted: false,
        };
        if bookmark.validate().is_err() {
            warnings.push(warning(
                "google-bookmark-invalid",
                "a Google bookmark could not be represented and was not imported",
            ));
            continue;
        }
        if !ids.insert(bookmark.id.clone()) {
            warnings.push(warning(
                "google-bookmark-duplicate-id",
                "a Google bookmark duplicate of an existing OpenDoc bookmark was not imported",
            ));
            continue;
        }
        imported.push(bookmark);
    }
    if !imported.is_empty() {
        warnings.push(warning(
            "google-bookmark-projected-to-block",
            "native Google bookmark block-start positions were projected to stable OpenDoc block targets; character ranges are not represented",
        ));
    }
    Ok(imported)
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
    let state = match value.get("state").and_then(Value::as_str).unwrap_or("open") {
        "open" => opendoc_core::CommentThreadState::Open,
        "resolved" => opendoc_core::CommentThreadState::Resolved,
        "reopened" => opendoc_core::CommentThreadState::Reopened,
        other => {
            return Err(ImportError::InvalidInput(format!(
                "unsupported comment thread state {other}"
            )))
        }
    };
    Ok(CommentThread {
        id: parse_imported_stable_id(id)?,
        anchor: import_google_anchor(value.get("anchor").unwrap_or(&Value::Null))?,
        comments,
        state,
        resolved_by: value
            .get("resolvedBy")
            .and_then(Value::as_str)
            .map(str::to_string),
        resolved_at_ms: optional_u64(value, "resolvedAtMs")?,
        action_assignee: value
            .get("actionAssignee")
            .and_then(Value::as_str)
            .map(str::to_string),
        action_due_at_ms: optional_u64(value, "actionDueAtMs")?,
        action_completed_by: value
            .get("actionCompletedBy")
            .and_then(Value::as_str)
            .map(str::to_string),
        action_completed_at_ms: optional_u64(value, "actionCompletedAtMs")?,
        reactions: import_google_comment_reactions(value.get("reactions"))?,
        deleted: optional_bool(value, "deleted")?.unwrap_or(false),
    })
}

fn import_google_comment_reactions(
    value: Option<&Value>,
) -> Result<Vec<opendoc_core::CommentThreadReaction>, ImportError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let reactions = value.as_array().ok_or_else(|| {
        ImportError::InvalidInput("comment reactions must be an array".to_string())
    })?;
    reactions
        .iter()
        .map(|reaction| {
            expect_object(reaction, "comment reaction")?;
            let emoji = required_str(reaction, "emoji", "comment reaction")?.to_string();
            opendoc_core::validate_comment_reaction_emoji(&emoji)
                .map_err(|err| ImportError::InvalidInput(err.to_string()))?;
            let actors = reaction
                .get("actors")
                .and_then(Value::as_array)
                .ok_or_else(|| {
                    ImportError::InvalidInput("comment reaction actors missing".to_string())
                })?
                .iter()
                .map(|actor| {
                    actor.as_str().map(str::to_string).ok_or_else(|| {
                        ImportError::InvalidInput(
                            "comment reaction actor must be a string".to_string(),
                        )
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(opendoc_core::CommentThreadReaction { emoji, actors })
        })
        .collect()
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
        "format_remove" => {
            let marks = import_google_text_marks(
                value.get("textStyle").unwrap_or(&Value::Null),
                &mut Vec::new(),
            );
            let [mark] = marks.as_slice() else {
                return Err(ImportError::InvalidInput(
                    "format removal suggestion must contain exactly one text style".to_string(),
                ));
            };
            Ok(SuggestionKind::FormatRemove {
                range: import_google_range(value.get("range").unwrap_or(&Value::Null))?,
                kind: mark.kind.clone(),
                value: mark.value.clone(),
            })
        }
        "format_replace" => {
            let marks = import_google_text_marks(
                value.get("textStyle").unwrap_or(&Value::Null),
                &mut Vec::new(),
            );
            let [mark] = marks.as_slice() else {
                return Err(ImportError::InvalidInput(
                    "format replacement suggestion must contain exactly one text style".to_string(),
                ));
            };
            Ok(SuggestionKind::FormatReplace {
                range: import_google_range(value.get("range").unwrap_or(&Value::Null))?,
                kind: mark.kind.clone(),
                expected_value: required_str(value, "expectedValue", "format replacement")?
                    .to_string(),
                value: mark.value.clone().ok_or_else(|| {
                    ImportError::InvalidInput(
                        "format replacement suggestion must have a value-bearing text style"
                            .to_string(),
                    )
                })?,
            })
        }
        "link_change" => Ok(SuggestionKind::LinkChange {
            inline_id: parse_imported_stable_id(required_str(
                value,
                "inlineId",
                "link suggestion",
            )?)?,
            expected_href: value
                .get("expectedHref")
                .and_then(Value::as_str)
                .map(str::to_owned),
            href: value.get("href").and_then(Value::as_str).map(str::to_owned),
        }),
        "block_delete" => Ok(SuggestionKind::BlockDelete {
            block_id: parse_imported_stable_id(required_str(
                value,
                "blockId",
                "block deletion suggestion",
            )?)?,
        }),
        "block_insert" => Ok(SuggestionKind::BlockInsert {
            position: serde_json::from_value::<InsertPosition>(
                value.get("position").cloned().unwrap_or(Value::Null),
            )
            .map_err(|error| {
                ImportError::InvalidInput(format!("invalid block insertion position: {error}"))
            })?,
            block: serde_json::from_value::<Block>(
                value.get("block").cloned().unwrap_or(Value::Null),
            )
            .map_err(|error| {
                ImportError::InvalidInput(format!("invalid block insertion payload: {error}"))
            })?,
        }),
        "block_replace" => Ok(SuggestionKind::BlockReplace {
            block_id: parse_imported_stable_id(required_str(
                value,
                "blockId",
                "block replacement suggestion",
            )?)?,
            expected: Box::new(
                serde_json::from_value::<Block>(
                    value.get("expected").cloned().unwrap_or(Value::Null),
                )
                .map_err(|error| {
                    ImportError::InvalidInput(format!(
                        "invalid block replacement source precondition: {error}"
                    ))
                })?,
            ),
            replacement: Box::new(
                serde_json::from_value::<Block>(
                    value.get("replacement").cloned().unwrap_or(Value::Null),
                )
                .map_err(|error| {
                    ImportError::InvalidInput(format!("invalid block replacement payload: {error}"))
                })?,
            ),
        }),
        "block_style_change" => Ok(SuggestionKind::ParagraphStyleChange {
            block_id: parse_imported_stable_id(required_str(
                value,
                "blockId",
                "paragraph style suggestion",
            )?)?,
            expected: serde_json::from_value::<ParagraphStyle>(
                value.get("expectedStyle").cloned().unwrap_or(Value::Null),
            )
            .map_err(|error| {
                ImportError::InvalidInput(format!("invalid paragraph style expectation: {error}"))
            })?,
            proposed: serde_json::from_value::<ParagraphStyle>(
                value.get("proposedStyle").cloned().unwrap_or(Value::Null),
            )
            .map_err(|error| {
                ImportError::InvalidInput(format!("invalid paragraph style proposal: {error}"))
            })?,
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
        "orphaned" => Ok(Anchor::Orphaned {
            quote: required_source_string(value, "quote", "orphaned anchor", "orphan quote")?,
            context: required_source_string(value, "context", "orphaned anchor", "orphan context")?,
            warning: optional_source_string(value, "warning", "orphaned anchor warning")?
                .unwrap_or_else(|| "imported orphaned anchor".to_string()),
        }),
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
    // Google exposes the rendered font weight separately from its `bold`
    // switch.  A weight of 700 is the one non-default explicit weight whose
    // rendering is exactly representable by OpenDoc's existing Bold mark:
    // Google applies a default weight of 400 before that mark on export.
    // Do not collapse the other CSS weights into Bold: 100..600 and 800..900
    // convey a distinct rendered weight and the core mark vocabulary has no
    // durable value-bearing weight mark.
    match style.pointer("/weightedFontFamily/weight") {
        Some(weight) if weight.as_i64() == Some(700) => {
            if !marks.iter().any(|mark| mark.kind == MarkKind::Bold) {
                marks.push(mark(MarkKind::Bold, None));
            }
        }
        Some(weight) if weight.as_i64() == Some(400) => {}
        Some(_) => warnings.push(ModelWarning {
            code: "google-text-style-font-weight-degraded".to_string(),
            message: "Google Docs font weight is not representable; retained the font family and other text styling"
                .to_string(),
        }),
        None => {}
    }
    if let Some(size) = import_google_font_size(style, warnings) {
        marks.push(mark(MarkKind::Size, Some(size)));
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
    // `false` has the same rendered state as an absent small-caps setting.
    // Only a true value loses source-visible formatting.  In particular, do
    // not uppercase text: that would alter its contents rather than preserve
    // a typographic property the model cannot carry.
    if style.get("smallCaps").and_then(Value::as_bool) == Some(true) {
        warnings.push(ModelWarning {
            code: GOOGLE_UNREPRESENTABLE_SMALL_CAPS.to_string(),
            message: "Google Docs smallCaps is not representable and was dropped".to_string(),
        });
    }
    marks
}

/// Reads the one text-style dimension directly supported by the mark model.
/// This deliberately does not reuse paragraph-dimension diagnostics: a bad
/// run font size is neither a paragraph-style loss nor a reason to drop the
/// surrounding run's other formatting.
fn import_google_font_size(style: &Value, warnings: &mut Vec<ModelWarning>) -> Option<String> {
    let value = style.get("fontSize")?;
    let Some(dimension) = value.as_object() else {
        warnings.push(ModelWarning {
            code: GOOGLE_DROPPED_TEXT_FONT_SIZE.to_string(),
            message: "Google Docs fontSize must be a dimension object and was dropped".to_string(),
        });
        return None;
    };

    match dimension.get("unit") {
        None | Some(Value::Null) => {}
        Some(Value::String(unit)) if unit == "PT" || unit == "UNIT_UNSPECIFIED" => {}
        Some(Value::String(unit)) => {
            warnings.push(ModelWarning {
                code: GOOGLE_DROPPED_TEXT_FONT_SIZE.to_string(),
                message: format!(
                    "Google Docs fontSize used unsupported unit {unit} and was dropped"
                ),
            });
            return None;
        }
        Some(_) => {
            warnings.push(ModelWarning {
                code: GOOGLE_DROPPED_TEXT_FONT_SIZE.to_string(),
                message: "Google Docs fontSize unit must be a string and was dropped".to_string(),
            });
            return None;
        }
    }

    let Some(magnitude) = dimension.get("magnitude").and_then(Value::as_f64) else {
        warnings.push(ModelWarning {
            code: GOOGLE_DROPPED_TEXT_FONT_SIZE.to_string(),
            message: "Google Docs fontSize magnitude must be a number and was dropped".to_string(),
        });
        return None;
    };
    // The browser and PDF layout both deliberately bound point sizes at this
    // limit. Keeping a larger native value in the mark model would therefore
    // be a false round-trip: it serializes, but OpenDoc cannot render it.
    if !(0.0 < magnitude && magnitude <= 1_600.0) {
        warnings.push(ModelWarning {
            code: GOOGLE_DROPPED_TEXT_FONT_SIZE.to_string(),
            message: format!(
                "Google Docs fontSize of {magnitude}pt is outside OpenDoc's renderable range and was dropped"
            ),
        });
        return None;
    }
    Some(trim_float(magnitude))
}

/// Reads the native `TableCellStyle` values Google returns on every cell.
///
/// These are deliberately parsed here rather than treated as OpenDoc-only
/// extensions: a document fetched from the Docs API contains the native
/// spelling. Fields the model cannot represent are named in a warning instead
/// of being silently discarded.
fn import_google_table_cell_style(
    style: Option<&Value>,
    warnings: &mut Vec<ModelWarning>,
) -> Result<opendoc_core::TableCellProperties, ImportError> {
    let Some(style) = style else {
        return Ok(Default::default());
    };
    let style = style.as_object().ok_or_else(|| {
        ImportError::UnsupportedStructure("malformed Google Docs table cell style".to_string())
    })?;
    let value = Value::Object(style.clone());
    let mut properties = opendoc_core::TableCellProperties::default();
    match value.get("backgroundColor") {
        None => {}
        Some(background) => {
            let background = background.as_object().ok_or_else(|| {
                ImportError::UnsupportedStructure(
                    "malformed Google Docs table cell background color".to_string(),
                )
            })?;
            match background
                .get("color")
                .and_then(Value::as_object)
                .and_then(|color| color.get("rgbColor"))
                .and_then(import_google_rgb)
            {
                Some(rgb) => {
                    properties.background = Some(
                        Color::parse(&rgb)
                            .map_err(|err| ImportError::InvalidInput(err.to_string()))?,
                    );
                }
                None if !background.contains_key("color") => warnings.push(warning(
                    GOOGLE_TRANSPARENT_TABLE_BACKGROUND_UNREPRESENTABLE,
                    "Google Docs table cell has an explicit transparent background; OpenDoc distinguishes only an inherited background, so the explicit clear was dropped",
                )),
                None => warnings.push(warning(
                    "google-dropped-table-style",
                    "Google Docs table cell background color is not an RGB color and was dropped",
                )),
            }
        }
    }
    properties.border_top = import_google_table_border(value.get("borderTop"), warnings)?;
    properties.border_bottom = import_google_table_border(value.get("borderBottom"), warnings)?;
    properties.border_start = import_google_table_border(value.get("borderLeft"), warnings)?;
    properties.border_end = import_google_table_border(value.get("borderRight"), warnings)?;
    properties.padding_top = import_dimension(&value, "paddingTop", warnings)?;
    properties.padding_bottom = import_dimension(&value, "paddingBottom", warnings)?;
    properties.padding_start = import_dimension(&value, "paddingLeft", warnings)?;
    properties.padding_end = import_dimension(&value, "paddingRight", warnings)?;
    properties.vertical_alignment = match value.get("contentAlignment").and_then(Value::as_str) {
        None | Some("CONTENT_ALIGNMENT_UNSPECIFIED") => None,
        Some("TOP") => Some(VerticalAlignment::Top),
        Some("MIDDLE") => Some(VerticalAlignment::Middle),
        Some("BOTTOM") => Some(VerticalAlignment::Bottom),
        Some(other) => {
            warnings.push(warning(
                "google-dropped-table-style",
                &format!("Google Docs table cell content alignment {other} is unsupported and was dropped"),
            ));
            None
        }
    };
    Ok(properties)
}

fn import_google_table_cell_span(style: Option<&Value>) -> Result<CellSpan, ImportError> {
    let Some(style) = style else {
        return Ok(CellSpan::SINGLE);
    };
    let style = style.as_object().ok_or_else(|| {
        ImportError::UnsupportedStructure("malformed Google Docs table cell style".to_string())
    })?;
    let read = |key: &str| -> Result<u32, ImportError> {
        match style.get(key) {
            None => Ok(1),
            Some(value) => value
                .as_u64()
                .and_then(|value| u32::try_from(value).ok())
                .filter(|value| *value > 0)
                .ok_or_else(|| {
                    ImportError::UnsupportedStructure(format!(
                        "malformed Google Docs table cell {key}"
                    ))
                }),
        }
    };
    let rows = read("rowSpan")?;
    let columns = read("columnSpan")?;
    CellSpan::new(rows, columns).map_err(|err| ImportError::UnsupportedStructure(err.to_string()))
}

/// Google represents a merged range once, at its leading cell; OpenDoc keeps
/// the covered positions in the rectangular grid. Materialise those covered
/// cells before `BlockKind::table` validates the result.
fn normalize_google_table_spans(rows: &mut [TableRow], warnings: &mut Vec<ModelWarning>) {
    let columns = rows
        .iter()
        .map(|row| {
            row.cells
                .iter()
                .map(|cell| cell.span.columns() as usize)
                .sum::<usize>()
        })
        .max()
        .unwrap_or(1);
    // A later Google row omits cells hidden by a vertical merge. `occupied`
    // makes the next source cell land after those hidden coordinates instead
    // of overwriting them (the crucial difference from merely padding every
    // row to its widest source-cell count).
    let mut occupied = vec![vec![false; columns]; rows.len()];
    for row_index in 0..rows.len() {
        let mut expanded = (0..columns).map(|_| TableCell::empty()).collect::<Vec<_>>();
        let mut column_index = 0usize;
        for mut cell in std::mem::take(&mut rows[row_index].cells) {
            while column_index < columns && occupied[row_index][column_index] {
                column_index += 1;
            }
            if column_index == columns {
                warnings.push(warning(
                    "google-table-span-clamped",
                    "Google Docs table row had cells beyond its declared merged geometry and they were dropped",
                ));
                break;
            }
            let requested = cell.span.rows() as usize;
            let available = rows.len() - row_index;
            if requested > available {
                cell.span = CellSpan::new(available as u32, cell.span.columns())
                    .expect("remaining table rows form a valid span");
                warnings.push(warning(
                    "google-table-span-clamped",
                    "Google Docs table row span extended beyond the table and was clamped",
                ));
            }
            let span = cell.span;
            if column_index + span.columns() as usize > columns {
                cell.span = CellSpan::new(span.rows(), (columns - column_index) as u32)
                    .expect("remaining table columns form a valid span");
                warnings.push(warning(
                    "google-table-span-clamped",
                    "Google Docs table column span extended beyond the table and was clamped",
                ));
            }
            let span = cell.span;
            expanded[column_index] = cell;
            for covered_row in occupied
                .iter_mut()
                .take((row_index + span.rows() as usize).min(rows.len()))
                .skip(row_index + 1)
            {
                for covered_column in covered_row
                    .iter_mut()
                    .take((column_index + span.columns() as usize).min(columns))
                    .skip(column_index)
                {
                    *covered_column = true;
                }
            }
            column_index += span.columns() as usize;
        }
        rows[row_index].cells = expanded;
    }
}

fn import_google_table_border(
    border: Option<&Value>,
    warnings: &mut Vec<ModelWarning>,
) -> Result<Option<CellBorder>, ImportError> {
    let Some(border) = border else {
        return Ok(None);
    };
    let object = border.as_object().ok_or_else(|| {
        ImportError::UnsupportedStructure("malformed Google Docs table cell border".to_string())
    })?;
    let value = Value::Object(object.clone());
    let width = import_dimension(&value, "width", warnings)?.unwrap_or(opendoc_core::Length::ZERO);
    if width.twips() == 0 {
        return Ok(Some(CellBorder::none()));
    }
    let style = match value.get("dashStyle").and_then(Value::as_str) {
        None | Some("SOLID") | Some("DASH_STYLE_UNSPECIFIED") => BorderStyle::Solid,
        Some("DASH") => BorderStyle::Dashed,
        Some("DOT") => BorderStyle::Dotted,
        Some("DASH_DOT") | Some("DASH_DOT_DOT") => {
            warnings.push(warning(
                "google-approximated-table-border",
                "Google Docs dash-dot table border was imported as dashed",
            ));
            BorderStyle::Dashed
        }
        Some(other) => {
            warnings.push(warning(
                "google-dropped-table-style",
                &format!(
                    "Google Docs table border dash style {other} is unsupported and was dropped"
                ),
            ));
            return Ok(None);
        }
    };
    let color = value
        .pointer("/color/color/rgbColor")
        .and_then(import_google_rgb)
        .map(|rgb| Color::parse(&rgb).map_err(|err| ImportError::InvalidInput(err.to_string())))
        .transpose()?
        .unwrap_or(Color::BLACK);
    match CellBorder::new(style, width, color) {
        Ok(border) => Ok(Some(border)),
        Err(_) => {
            warnings.push(warning(
                "google-dropped-table-style",
                "Google Docs table border width is outside OpenDoc's supported range and was dropped",
            ));
            Ok(None)
        }
    }
}

fn import_google_table_column_widths(
    table: &Value,
    columns: &mut [TableColumn],
    warnings: &mut Vec<ModelWarning>,
) -> Result<(), ImportError> {
    let Some(properties) = table
        .pointer("/tableStyle/tableColumnProperties")
        .and_then(Value::as_array)
    else {
        return Ok(());
    };
    for (index, property) in properties.iter().enumerate() {
        let Some(column) = columns.get_mut(index) else {
            warnings.push(warning(
                "google-dropped-table-style",
                "Google Docs supplied more table column styles than the table has columns",
            ));
            break;
        };
        let property = property.as_object().ok_or_else(|| {
            ImportError::UnsupportedStructure(
                "malformed Google Docs table column style".to_string(),
            )
        })?;
        let value = Value::Object(property.clone());
        match value.get("widthType").and_then(Value::as_str) {
            None | Some("WIDTH_TYPE_UNSPECIFIED") => {}
            Some("FIXED_WIDTH") => match import_dimension(&value, "width", warnings)? {
                Some(width) if width.twips() >= TableColumn::MIN_WIDTH_TWIPS => column.width = Some(width),
                Some(_) => warnings.push(warning(
                    "google-dropped-table-style",
                    "Google Docs table column width is below OpenDoc's 0.1in minimum and was dropped",
                )),
                None => warnings.push(warning(
                    "google-dropped-table-style",
                    "Google Docs fixed-width table column had no width and was dropped",
                )),
            },
            // `None` in the model means no authored fixed width.  It does
            // not preserve either of Google's layout algorithms, so treating
            // them as unspecified would silently change a table on export.
            Some(algorithm @ ("EVENLY_DISTRIBUTED" | "FIT_TO_CONTENT")) => warnings.push(
                warning(
                    "google-dropped-table-style",
                    &format!(
                        "Google Docs table column width algorithm {algorithm} is not represented by OpenDoc and was dropped"
                    ),
                ),
            ),
            Some(other) => warnings.push(warning(
                "google-dropped-table-style",
                &format!("Google Docs table column width type {other} is unsupported and was dropped"),
            )),
        }
    }
    Ok(())
}

fn import_google_table(
    table: &Value,
    lists: &GoogleLists,
    warnings: &mut Vec<ModelWarning>,
    table_depth: usize,
) -> Result<Block, ImportError> {
    warn_google_table_suggestions(table, "table", warnings);
    let border = table
        .get("opendocTableBorder")
        .map(|value| {
            let object = value.as_object().ok_or_else(|| {
                ImportError::UnsupportedStructure("malformed OpenDoc table border".to_string())
            })?;
            let style = object
                .get("style")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    ImportError::UnsupportedStructure(
                        "malformed OpenDoc table border style".to_string(),
                    )
                })
                .and_then(|value| {
                    opendoc_core::BorderStyle::parse(value)
                        .map_err(|err| ImportError::UnsupportedStructure(err.to_string()))
                })?;
            let twips = object
                .get("twips")
                .and_then(Value::as_i64)
                .and_then(|value| i32::try_from(value).ok())
                .ok_or_else(|| {
                    ImportError::UnsupportedStructure(
                        "malformed OpenDoc table border width".to_string(),
                    )
                })?;
            let color = object
                .get("color")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    ImportError::UnsupportedStructure(
                        "malformed OpenDoc table border colour".to_string(),
                    )
                })
                .and_then(|value| {
                    opendoc_core::Color::parse(value)
                        .map_err(|err| ImportError::UnsupportedStructure(err.to_string()))
                })?;
            opendoc_core::CellBorder::new(
                style,
                opendoc_core::Length::from_twips(twips)
                    .map_err(|err| ImportError::UnsupportedStructure(err.to_string()))?,
                color,
            )
            .map_err(|err| ImportError::UnsupportedStructure(err.to_string()))
        })
        .transpose()?;
    let alignment = table
        .get("opendocTableAlignment")
        .map(|value| {
            value
                .as_str()
                .ok_or_else(|| {
                    ImportError::UnsupportedStructure(
                        "malformed OpenDoc table alignment".to_string(),
                    )
                })
                .and_then(|value| {
                    opendoc_core::TableAlignment::parse(value)
                        .map_err(|err| ImportError::UnsupportedStructure(err.to_string()))
                })
        })
        .transpose()?;
    let rows = table
        .get("tableRows")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            ImportError::UnsupportedStructure("malformed Google Docs table".to_string())
        })?
        .iter()
        .enumerate()
        .map(|(row_index, row)| {
            warn_google_table_suggestions(row, &format!("row {}", row_index + 1), warnings);
            // `tableHeader` is the native Docs API representation.  Older
            // OpenDoc-shaped exports used `opendocHeader`; keep accepting
            // that extension, but never let it override a native value.
            let native_header = match row.pointer("/tableRowStyle/tableHeader") {
                Some(Value::Bool(value)) => Some(*value),
                Some(_) => {
                    return Err(ImportError::UnsupportedStructure(
                        "malformed Google Docs table row header".to_string(),
                    ))
                }
                None => None,
            };
            let extension_header = match row.get("opendocHeader") {
                Some(Value::Bool(value)) => Some(*value),
                Some(_) => {
                    return Err(ImportError::UnsupportedStructure(
                        "malformed OpenDoc table row header".to_string(),
                    ))
                }
                None => None,
            };
            if native_header.is_some()
                && extension_header.is_some()
                && native_header != extension_header
            {
                warnings.push(warning(
                    "google-conflicting-table-header",
                    "native Google Docs table row header overrides a conflicting OpenDoc extension",
                ));
            }
            let header = native_header.or(extension_header).unwrap_or(false);
            match row.pointer("/tableRowStyle/preventOverflow") {
                Some(Value::Bool(true)) => warnings.push(warning(
                    "google-dropped-table-style",
                    "Google Docs table row preventOverflow is not representable and was dropped",
                )),
                Some(Value::Bool(false)) | None => {}
                Some(_) => {
                    return Err(ImportError::UnsupportedStructure(
                        "malformed Google Docs table row preventOverflow".to_string(),
                    ))
                }
            }
            let cells = row
                .get("tableCells")
                .and_then(Value::as_array)
                .ok_or_else(|| {
                    ImportError::UnsupportedStructure("malformed Google Docs table row".to_string())
                })?
                .iter()
                .enumerate()
                .map(|(cell_index, cell)| {
                    warn_google_table_suggestions(
                        cell,
                        &format!("row {}, cell {}", row_index + 1, cell_index + 1),
                        warnings,
                    );
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
                            import_google_structural_element(
                                element,
                                lists,
                                warnings,
                                table_depth + 1,
                            )
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
                    let mut imported = TableCell::new(content);
                    imported.properties =
                        import_google_table_cell_style(cell.get("tableCellStyle"), warnings)?;
                    imported.properties.row_header = match cell.get("opendocRowHeader") {
                        Some(Value::Bool(value)) => Some(*value),
                        Some(_) => {
                            return Err(ImportError::UnsupportedStructure(
                                "malformed OpenDoc table cell row header".to_string(),
                            ))
                        }
                        None => None,
                    };
                    imported.span = import_google_table_cell_span(cell.get("tableCellStyle"))?;
                    Ok(imported)
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
                height: import_dimension(
                    row.get("tableRowStyle").unwrap_or(&Value::Null),
                    "minRowHeight",
                    warnings,
                )?,
                header,
                cells,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut rows = if rows.is_empty() {
        warnings.push(ModelWarning {
            code: "google-empty-table-normalized".to_string(),
            message: "empty Google Docs table imported with a blank row and cell".to_string(),
        });
        vec![TableRow {
            id: StableId::new("row"),
            height: None,
            header: false,
            cells: vec![TableCell::empty()],
        }]
    } else {
        rows
    };
    normalize_google_table_spans(&mut rows, warnings);
    Ok(Block {
        id: StableId::new("block"),
        kind: {
            let mut imported_table = BlockKind::table(rows);
            if let BlockKind::Table {
                columns,
                properties,
                ..
            } = &mut imported_table
            {
                properties.border = border;
                properties.alignment = alignment;
                import_google_table_column_widths(table, columns, warnings)?;
            }
            imported_table
        },
        content: Vec::new(),
        properties: BlockProperties::default(),
    })
}

/// Native Docs table review maps describe proposed grid structure and
/// formatting, not the accepted table state.  OpenDoc has no stable row/cell
/// proposal target yet, so consuming any of them as base formatting would
/// silently accept a review change.  Empty maps are normal API noise and have
/// no proposal to disclose.
fn warn_google_table_suggestions(value: &Value, location: &str, warnings: &mut Vec<ModelWarning>) {
    const STRUCTURE_FIELDS: [&str; 2] = ["suggestedInsertionIds", "suggestedDeletionIds"];
    const STYLE_FIELDS: [&str; 3] = [
        "suggestedTableStyleChanges",
        "suggestedTableRowStyleChanges",
        "suggestedTableCellStyleChanges",
    ];
    let Some(object) = value.as_object() else {
        return;
    };
    for (field, proposed) in object {
        if (STRUCTURE_FIELDS.contains(&field.as_str()) || STYLE_FIELDS.contains(&field.as_str()))
            && !matches!(proposed, Value::Array(values) if values.is_empty())
            && !matches!(proposed, Value::Object(values) if values.is_empty())
        {
            warnings.push(warning(
                "google-dropped-table-suggestion",
                &format!(
                    "Google Docs {field} at {location} is a proposed table change; OpenDoc retained the accepted table state because stable grid suggestion targets are not implemented"
                ),
            ));
        }
    }
}

pub(crate) fn import_google_structural_element(
    element: &Value,
    lists: &GoogleLists,
    warnings: &mut Vec<ModelWarning>,
    table_depth: usize,
) -> Result<Vec<Block>, ImportError> {
    if let Some(paragraph) = element.get("paragraph") {
        import_google_paragraph(paragraph, lists, warnings)
    } else if let Some(table) = element.get("table") {
        if table_depth >= MAX_GOOGLE_TABLE_DEPTH {
            return Err(ImportError::InvalidInput(format!(
                "Google Docs table nesting exceeds the supported {MAX_GOOGLE_TABLE_DEPTH}-level limit"
            )));
        }
        Ok(vec![import_google_table(
            table,
            lists,
            warnings,
            table_depth,
        )?])
    } else if let Some(equation) = element.get("opendocEquationBlock") {
        Ok(vec![import_opendoc_equation_block(equation)?])
    } else if let Some(image) = element.get("opendocImage") {
        Ok(vec![import_opendoc_image(image)?])
    } else if let Some(toc) = element.get("opendocTableOfContents") {
        let block_id = optional_str(toc, "blockId")?.unwrap_or("block");
        let max_level = optional_u8(toc, "maxLevel")?.unwrap_or(3);
        if !(1..=6).contains(&max_level) {
            return Err(ImportError::InvalidInput(
                "OpenDoc table of contents maxLevel is outside 1..=6".to_string(),
            ));
        }
        warnings.push(warning(
            "opendoc-google-toc-extension",
            "an OpenDoc table of contents extension was imported; it is generated from current headings",
        ));
        Ok(vec![Block {
            id: parse_imported_stable_id(block_id)?,
            kind: BlockKind::TableOfContents { max_level },
            content: Vec::new(),
            properties: BlockProperties::default(),
        }])
    } else if let Some(bibliography) = element.get("opendocBibliography") {
        let block_id = optional_str(bibliography, "blockId")?.unwrap_or("block");
        warnings.push(warning(
            "opendoc-google-bibliography-extension",
            "an OpenDoc bibliography extension was imported; its entries are generated from live citation groups",
        ));
        Ok(vec![Block {
            id: parse_imported_stable_id(block_id)?,
            kind: BlockKind::Bibliography,
            content: Vec::new(),
            properties: BlockProperties::default(),
        }])
    } else if let Some(section_break) = element.get("sectionBreak") {
        if google_part_is_present(section_break.get("sectionStyle")) {
            warnings.push(warning(
                google_style::DROPPED_DOCUMENT_PART,
                "Google Docs section styling (columns, margins, headers per section) is not representable",
            ));
        }
        Ok(vec![Block {
            id: StableId::new("block"),
            kind: BlockKind::PageBreak,
            content: Vec::new(),
            properties: BlockProperties::default(),
        }])
    } else if let Some(toc) = element.get("tableOfContents") {
        // Google gives us the generated entry fragments, not a heading-scope
        // setting.  Do not turn that cache into ordinary author text: it would
        // become stale as soon as a heading changed.  Google Docs' ordinary
        // table-of-contents presentation is projected to OpenDoc's default
        // H1--H3 generated scope, whose entries are derived from the imported
        // headings on every replica.
        //
        // Validate `content` when present so malformed source JSON does not
        // become invisible merely because its generated display is ignored.
        let _ = optional_array(toc, "content")?;
        warnings.push(warning(
            "google-toc-scope-projected",
            "a native Google Docs table of contents was imported as a generated OpenDoc table of contents with the default Heading 1--3 scope; Google does not expose its scope in document JSON",
        ));
        Ok(vec![Block {
            id: StableId::new("block"),
            kind: BlockKind::TableOfContents { max_level: 3 },
            content: Vec::new(),
            properties: BlockProperties::default(),
        }])
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
