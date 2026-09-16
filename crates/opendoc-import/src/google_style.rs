//! Google Docs JSON ⇄ OpenDoc paragraph formatting and list-type mapping.
//!
//! Google measures paragraph geometry in points (`{ "magnitude": …, "unit":
//! "PT" }`), OpenDoc in twips, so every length here goes through
//! [`Length::from_points`] / [`Length::points`] rather than hand-rolled
//! arithmetic.
//!
//! Two conventions hold throughout:
//!
//! * an absent source property stays `None` (inherit) — no default is invented;
//! * a property that is present but not representable produces a
//!   [`ModelWarning`], never silence.

use crate::google_color::{export_google_color, import_google_rgb};
use crate::json::optional_u64;
use crate::{optional_bool, optional_object, optional_str, ImportError};
use opendoc_core::{
    Alignment, Block, BlockKind, BlockProperties, BorderStyle, BulletListMarker, CellBorder, Color,
    Length, LineSpacing, ListKind, ListProperties, ModelWarning, OrderedListFormat, StableId,
    TextDirection,
};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

pub(crate) const DROPPED_PARAGRAPH_STYLE: &str = "google-dropped-paragraph-style";
pub(crate) const DROPPED_DIMENSION_UNIT: &str = "google-dropped-dimension-unit";
pub(crate) const UNRESOLVED_FIRST_LINE_INDENT: &str = "google-unresolved-first-line-indent";
pub(crate) const DROPPED_NAMED_STYLE: &str = "google-dropped-named-style";
pub(crate) const UNKNOWN_LIST_DEFINITION: &str = "google-unknown-list-definition";
pub(crate) const UNREPRESENTABLE_LIST_FORMAT: &str = "google-unrepresentable-list-format";
/// Google `glyphFormat` controls the punctuation/template around a list
/// counter.  OpenDoc's bounded list model owns the counter format but not an
/// arbitrary template, and consistently renders ordered markers with a
/// trailing period (and bullets without a template).
pub(crate) const UNREPRESENTABLE_LIST_GLYPH_FORMAT: &str =
    "google-unrepresentable-list-glyph-format";
/// `startNumber` has meaning only for an ordered Docs nesting level.  Keeping
/// it on a bullet/checklist run would create dormant source state which the
/// model cannot render or write back.
pub(crate) const UNREPRESENTABLE_LIST_START: &str = "google-unrepresentable-list-start";
pub(crate) const CHECKLIST_AS_BULLET: &str = "google-checklist-exported-as-bullet";
pub(crate) const DROPPED_LINE_SPACING_RULE: &str = "google-dropped-line-spacing-rule";
pub(crate) const DROPPED_DOCUMENT_PART: &str = "google-dropped-document-part";
pub(crate) const DROPPED_BLOCK_PROPERTIES: &str = "google-dropped-block-properties";
pub(crate) const DROPPED_PARAGRAPH_ELEMENT: &str = "google-dropped-paragraph-element";
pub(crate) const DROPPED_STRUCTURAL_ELEMENT: &str = "google-dropped-structural-element";
pub(crate) const SPLIT_PAGE_BREAK: &str = "google-split-page-break";
/// A Docs `Link` has a one-of destination: an external URL, tab, bookmark,
/// or heading. OpenDoc's inline link currently has only an external href, so
/// retaining a native internal destination as a URL would invent a target
/// outside its source document.
pub(crate) const DROPPED_INTERNAL_LINK: &str = "google-internal-link-dropped";
/// An external Docs `Link.url` is carried into the desktop renderer.  It must
/// be restricted to the same navigation schemes the renderer is willing to
/// open; otherwise a document import could turn source JSON into executable
/// browser navigation (for example, a `javascript:` URL).
pub(crate) const DROPPED_UNSAFE_EXTERNAL_LINK: &str = "google-unsafe-external-link-dropped";

pub(crate) fn warning(code: &str, message: &str) -> ModelWarning {
    ModelWarning {
        code: code.to_string(),
        message: message.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Paragraph style
// ---------------------------------------------------------------------------

/// `ParagraphStyle` fields this module maps onto [`BlockProperties`], plus
/// `namedStyleType`, which the block kind carries instead.
const MAPPED_PARAGRAPH_STYLE_KEYS: [&str; 16] = [
    "namedStyleType",
    "alignment",
    "direction",
    "indentStart",
    "indentEnd",
    "indentFirstLine",
    "lineSpacing",
    "spaceAbove",
    "spaceBelow",
    "keepWithNext",
    // This is represented structurally as the immediately preceding
    // `BlockKind::PageBreak`, rather than as a dormant formatting bit.
    "pageBreakBefore",
    "shading",
    "borderTop",
    "borderBottom",
    "borderLeft",
    "borderRight",
];

/// Google `ParagraphStyle` fields that carry formatting OpenDoc has no model
/// for. Presence alone is reported; the value is never guessed at.
const UNREPRESENTABLE_PARAGRAPH_STYLE_KEYS: [(&str, &str); 6] = [
    ("borderBetween", "paragraph borders"),
    ("tabStops", "custom tab stops"),
    ("keepLinesTogether", "keep-lines-together"),
    ("avoidWidowAndOrphan", "widow and orphan control"),
    ("spacingMode", "collapsed-spacing mode"),
    ("headingId", "heading bookmark ids"),
];

/// Maps a Google `paragraphStyle` object onto typed block properties.
pub(crate) fn import_paragraph_style(
    style: &Value,
    warnings: &mut Vec<ModelWarning>,
) -> Result<BlockProperties, ImportError> {
    let mut properties = BlockProperties::default();
    if !style.is_object() {
        return Ok(properties);
    }
    properties.alignment = import_alignment(style, warnings)?;
    properties.direction = import_direction(style, warnings)?;
    properties.indent_start = import_dimension(style, "indentStart", warnings)?;
    properties.indent_end = import_dimension(style, "indentEnd", warnings)?;
    properties.indent_first_line =
        import_first_line_indent(style, properties.indent_start, warnings)?;
    properties.line_spacing = import_line_spacing(style, warnings)?;
    properties.space_before = import_spacing(style, "spaceAbove", warnings)?;
    properties.space_after = import_spacing(style, "spaceBelow", warnings)?;
    properties.keep_with_next = optional_bool(style, "keepWithNext")?;
    properties.border = import_uniform_border(style, warnings)?;
    if let Some(shading) = style.get("shading") {
        match shading
            .pointer("/backgroundColor/color/rgbColor")
            .and_then(import_google_rgb)
        {
            Some(rgb) => {
                properties.background = Some(
                    Color::parse(&rgb)
                        .map_err(|error| ImportError::InvalidInput(error.to_string()))?,
                )
            }
            None => warnings.push(warning(
                DROPPED_PARAGRAPH_STYLE,
                "Google Docs paragraph shading was not an opaque RGB background and was dropped",
            )),
        }
    }
    // Every remaining field is either a known-unrepresentable one or a field
    // this importer has never heard of. Both are named in the warning; neither
    // is silently discarded, and neither is fatal.
    for (key, value) in style.as_object().into_iter().flatten() {
        if value.is_null() || MAPPED_PARAGRAPH_STYLE_KEYS.contains(&key.as_str()) {
            continue;
        }
        let message = match UNREPRESENTABLE_PARAGRAPH_STYLE_KEYS
            .into_iter()
            .find(|(name, _)| name == key)
        {
            Some((_, label)) => {
                format!("Google Docs {label} ({key}) are not representable and were dropped")
            }
            None => format!("unknown Google Docs paragraph style field {key} was ignored"),
        };
        warnings.push(warning(DROPPED_PARAGRAPH_STYLE, &message));
    }
    Ok(properties)
}

/// Google has five paragraph-border fields.  The document model deliberately
/// owns only one frame, so it accepts the source only when all four edges say
/// precisely the same supported thing and there is no between-paragraph
/// rule.  Padding is a material part of Google's border geometry; nonzero
/// padding is warned rather than silently folded into paragraph spacing.
fn import_uniform_border(
    style: &Value,
    warnings: &mut Vec<ModelWarning>,
) -> Result<Option<CellBorder>, ImportError> {
    let names = ["borderTop", "borderBottom", "borderLeft", "borderRight"];
    let present: Vec<_> = names
        .into_iter()
        .filter_map(|name| style.get(name))
        .collect();
    if present.is_empty() {
        return Ok(None);
    }
    if present.len() != names.len() || style.get("borderBetween").is_some() {
        warnings.push(warning(
            DROPPED_PARAGRAPH_STYLE,
            "Google Docs per-edge or between-paragraph borders are not representable by OpenDoc's uniform paragraph border and were dropped",
        ));
        return Ok(None);
    }
    let Some(first) = parse_google_border(present[0])? else {
        warnings.push(warning(
            DROPPED_PARAGRAPH_STYLE,
            "Google Docs paragraph border was not a supported opaque uniform line and was dropped",
        ));
        return Ok(None);
    };
    for border in &present[1..] {
        if parse_google_border(border)? != Some(first) {
            warnings.push(warning(DROPPED_PARAGRAPH_STYLE, "Google Docs paragraph borders differ by edge or use unsupported padding/style and were dropped"));
            return Ok(None);
        }
    }
    Ok(Some(first))
}

fn parse_google_border(value: &Value) -> Result<Option<CellBorder>, ImportError> {
    let Some(object) = value.as_object() else {
        return Ok(None);
    };
    let padding = match object.get("padding") {
        None => 0,
        Some(value) => import_border_dimension(value)?
            .map(Length::twips)
            .unwrap_or(0),
    };
    if padding != 0 {
        return Ok(None);
    }
    let Some(width) = object
        .get("width")
        .map(import_border_dimension)
        .transpose()?
        .flatten()
    else {
        return Ok(None);
    };
    let Some(rgb) = object
        .get("color")
        .and_then(|color| color.pointer("/color/rgbColor"))
        .and_then(import_google_rgb)
    else {
        return Ok(None);
    };
    let color = Color::parse(&rgb).map_err(|error| ImportError::InvalidInput(error.to_string()))?;
    let style = match object
        .get("dashStyle")
        .and_then(Value::as_str)
        .unwrap_or("SOLID")
    {
        "SOLID" => BorderStyle::Solid,
        "DASH" => BorderStyle::Dashed,
        "DOT" => BorderStyle::Dotted,
        _ => return Ok(None),
    };
    CellBorder::new(style, width, color)
        .map(Some)
        .map_err(|error| ImportError::InvalidInput(error.to_string()))
}

/// Strict local reader for a border dimension. Unlike paragraph indents this
/// has no warning sink: callers report the entire border as unrepresentable.
fn import_border_dimension(value: &Value) -> Result<Option<Length>, ImportError> {
    let Some(dimension) = value.as_object() else {
        return Ok(None);
    };
    match dimension.get("unit").and_then(Value::as_str) {
        None | Some("PT") | Some("UNIT_UNSPECIFIED") => {}
        Some(_) => return Ok(None),
    }
    let magnitude = match dimension.get("magnitude") {
        None | Some(Value::Null) => 0.0,
        Some(value) => value.as_f64().ok_or_else(|| {
            ImportError::InvalidInput(
                "Google Docs border dimension magnitude must be a number".to_string(),
            )
        })?,
    };
    Ok(Length::from_points(magnitude).ok())
}

fn import_alignment(
    style: &Value,
    warnings: &mut Vec<ModelWarning>,
) -> Result<Option<Alignment>, ImportError> {
    let Some(raw) = optional_str(style, "alignment")? else {
        return Ok(None);
    };
    Ok(match raw {
        "START" => Some(Alignment::Start),
        "CENTER" => Some(Alignment::Center),
        "END" => Some(Alignment::End),
        "JUSTIFIED" => Some(Alignment::Justify),
        // `ALIGNMENT_UNSPECIFIED` means "inherit", which is what `None` is.
        "ALIGNMENT_UNSPECIFIED" => None,
        other => {
            warnings.push(warning(
                DROPPED_PARAGRAPH_STYLE,
                &format!("unknown Google Docs paragraph alignment {other} was dropped"),
            ));
            None
        }
    })
}

fn import_direction(
    style: &Value,
    warnings: &mut Vec<ModelWarning>,
) -> Result<Option<TextDirection>, ImportError> {
    let Some(raw) = optional_str(style, "direction")? else {
        return Ok(None);
    };
    Ok(match raw {
        "LEFT_TO_RIGHT" => Some(TextDirection::LeftToRight),
        "RIGHT_TO_LEFT" => Some(TextDirection::RightToLeft),
        "CONTENT_DIRECTION_UNSPECIFIED" => None,
        other => {
            warnings.push(warning(
                DROPPED_PARAGRAPH_STYLE,
                &format!("unknown Google Docs content direction {other} was dropped"),
            ));
            None
        }
    })
}

/// Reads a Google `Dimension`. `PT` is the only unit the Docs API defines;
/// anything else is reported rather than reinterpreted.
pub(crate) fn import_dimension(
    style: &Value,
    key: &str,
    warnings: &mut Vec<ModelWarning>,
) -> Result<Option<Length>, ImportError> {
    let Some(dimension) = optional_object(style, key)? else {
        return Ok(None);
    };
    match optional_str(dimension, "unit")? {
        None | Some("PT") | Some("UNIT_UNSPECIFIED") => {}
        Some(other) => {
            warnings.push(warning(
                DROPPED_DIMENSION_UNIT,
                &format!("Google Docs {key} used unsupported unit {other} and was dropped"),
            ));
            return Ok(None);
        }
    }
    // `magnitude` is omitted when it is zero.
    let magnitude = match dimension.get("magnitude") {
        None | Some(Value::Null) => 0.0,
        Some(value) => value.as_f64().ok_or_else(|| {
            ImportError::InvalidInput(format!("{key}.magnitude must be a number"))
        })?,
    };
    match Length::from_points(magnitude) {
        Ok(length) => Ok(Some(length)),
        Err(_) => {
            warnings.push(warning(
                DROPPED_PARAGRAPH_STYLE,
                &format!("Google Docs {key} of {magnitude}pt is out of range and was dropped"),
            ));
            Ok(None)
        }
    }
}

/// Google's `indentFirstLine` is absolute — measured from the same edge as
/// `indentStart` — while OpenDoc stores the first line *relative to* the start
/// indent, so that a hanging indent is one signed number. The conversion is
/// therefore a subtraction, not a copy.
fn import_first_line_indent(
    style: &Value,
    indent_start: Option<Length>,
    warnings: &mut Vec<ModelWarning>,
) -> Result<Option<Length>, ImportError> {
    let Some(absolute) = import_dimension(style, "indentFirstLine", warnings)? else {
        return Ok(None);
    };
    if indent_start.is_none() && absolute.twips() != 0 {
        warnings.push(warning(
            UNRESOLVED_FIRST_LINE_INDENT,
            "Google Docs indentFirstLine was resolved against an inherited indentStart of 0",
        ));
    }
    let base = indent_start.map(Length::twips).unwrap_or(0);
    match Length::from_twips(absolute.twips() - base) {
        Ok(relative) => Ok(Some(relative)),
        Err(_) => {
            warnings.push(warning(
                DROPPED_PARAGRAPH_STYLE,
                "Google Docs indentFirstLine is out of range relative to indentStart and was dropped",
            ));
            Ok(None)
        }
    }
}

/// `lineSpacing` is a percentage of the natural line height, where 100 is
/// single spacing. Google has no exact/at-least rule, so it always maps to
/// [`LineSpacing::Multiple`].
fn import_line_spacing(
    style: &Value,
    warnings: &mut Vec<ModelWarning>,
) -> Result<Option<LineSpacing>, ImportError> {
    let percentage = match style.get("lineSpacing") {
        None | Some(Value::Null) => return Ok(None),
        Some(value) => value
            .as_f64()
            .ok_or_else(|| ImportError::InvalidInput("lineSpacing must be a number".to_string()))?,
    };
    match LineSpacing::multiple(percentage / 100.0) {
        Ok(spacing) => Ok(Some(spacing)),
        Err(_) => {
            warnings.push(warning(
                DROPPED_PARAGRAPH_STYLE,
                &format!(
                    "Google Docs lineSpacing of {percentage}% is out of range and was dropped"
                ),
            ));
            Ok(None)
        }
    }
}

fn import_spacing(
    style: &Value,
    key: &str,
    warnings: &mut Vec<ModelWarning>,
) -> Result<Option<Length>, ImportError> {
    let Some(length) = import_dimension(style, key, warnings)? else {
        return Ok(None);
    };
    if length.is_negative() {
        warnings.push(warning(
            DROPPED_PARAGRAPH_STYLE,
            &format!(
                "Google Docs {key} is negative, which OpenDoc cannot represent, and was dropped"
            ),
        ));
        return Ok(None);
    }
    Ok(Some(length))
}

/// Named styles OpenDoc maps structurally. Anything else is a style OpenDoc
/// has no model for, and says so.
pub(crate) fn report_unmapped_named_style(named_style: &str, warnings: &mut Vec<ModelWarning>) {
    match named_style {
        "" | "NORMAL_TEXT" | "TITLE" | "SUBTITLE" | "HEADING_1" | "HEADING_2"
        | "HEADING_3" | "HEADING_4" | "HEADING_5" | "HEADING_6" => {}
        other => warnings.push(warning(
            DROPPED_NAMED_STYLE,
            &format!(
                "Google Docs named style {other} has no OpenDoc equivalent and was imported as body text"
            ),
        )),
    }
}

/// Builds the `paragraphStyle` fields for a block's properties.
pub(crate) fn export_paragraph_style(
    properties: &BlockProperties,
    warnings: &mut Vec<ModelWarning>,
) -> Map<String, Value> {
    let mut style = Map::new();
    if let Some(alignment) = properties.alignment {
        style.insert(
            "alignment".to_string(),
            Value::String(
                match alignment {
                    Alignment::Start => "START",
                    Alignment::Center => "CENTER",
                    Alignment::End => "END",
                    Alignment::Justify => "JUSTIFIED",
                }
                .to_string(),
            ),
        );
    }
    if let Some(direction) = properties.direction {
        style.insert(
            "direction".to_string(),
            Value::String(
                match direction {
                    TextDirection::LeftToRight => "LEFT_TO_RIGHT",
                    TextDirection::RightToLeft => "RIGHT_TO_LEFT",
                }
                .to_string(),
            ),
        );
    }
    if let Some(indent) = properties.indent_start {
        style.insert("indentStart".to_string(), dimension(indent));
    }
    if let Some(indent) = properties.indent_end {
        style.insert("indentEnd".to_string(), dimension(indent));
    }
    if let Some(first_line) = properties.indent_first_line {
        // Back to Google's absolute basis: relative offset plus the start
        // indent, which is 0 when the block inherits it.
        let base = properties.indent_start.map(Length::twips).unwrap_or(0);
        match Length::from_twips(base + first_line.twips()) {
            Ok(absolute) => {
                style.insert("indentFirstLine".to_string(), dimension(absolute));
            }
            Err(_) => warnings.push(warning(
                DROPPED_PARAGRAPH_STYLE,
                "first-line indent plus start indent is out of range for Google Docs and was dropped",
            )),
        }
    }
    match properties.line_spacing {
        Some(LineSpacing::Multiple(multiple)) => {
            style.insert(
                "lineSpacing".to_string(),
                json!(multiple.ratio() * 100.0),
            );
        }
        Some(LineSpacing::Exact(_)) | Some(LineSpacing::AtLeast(_)) => warnings.push(warning(
            DROPPED_LINE_SPACING_RULE,
            "Google Docs expresses line spacing only as a percentage of the natural line height, so an exact or at-least line height was dropped",
        )),
        None => {}
    }
    if let Some(space) = properties.space_before {
        style.insert("spaceAbove".to_string(), dimension(space));
    }
    if let Some(space) = properties.space_after {
        style.insert("spaceBelow".to_string(), dimension(space));
    }
    if let Some(keep_with_next) = properties.keep_with_next {
        style.insert("keepWithNext".to_string(), Value::Bool(keep_with_next));
    }
    if let Some(background) = properties.background {
        style.insert(
            "shading".to_string(),
            json!({ "backgroundColor": export_google_color(&background.as_hex()) }),
        );
    }
    if let Some(border) = properties.border {
        let dash_style = match border.style() {
            BorderStyle::Solid => Some("SOLID"),
            BorderStyle::Dashed => Some("DASH"),
            BorderStyle::Dotted => Some("DOT"),
            // Google Docs' public ParagraphBorder API has no exact spelling
            // for double or an explicit absent frame.
            BorderStyle::None | BorderStyle::Double => None,
        };
        if let Some(dash_style) = dash_style {
            let border = json!({
                "color": export_google_color(&border.color().as_hex()),
                "width": dimension(border.width()),
                "padding": dimension(Length::ZERO),
                "dashStyle": dash_style,
            });
            for edge in ["borderTop", "borderBottom", "borderLeft", "borderRight"] {
                style.insert(edge.to_string(), border.clone());
            }
        } else {
            warnings.push(warning(
                DROPPED_PARAGRAPH_STYLE,
                "OpenDoc's explicit-none or double paragraph border has no exact Google Docs ParagraphBorder spelling and was dropped",
            ));
        }
    }
    style
}

fn dimension(length: Length) -> Value {
    json!({ "magnitude": length.points(), "unit": "PT" })
}

// ---------------------------------------------------------------------------
// Lists
// ---------------------------------------------------------------------------

/// Google glyph types that number their items. Everything else is a bullet.
const ORDERED_GLYPH_TYPES: [&str; 6] = [
    "DECIMAL",
    "ZERO_DECIMAL",
    "UPPER_ALPHA",
    "ALPHA",
    "UPPER_ROMAN",
    "ROMAN",
];

/// The Unicode ballot boxes OpenDoc writes for a checklist. The Docs API has
/// no checklist glyph type, so a checklist survives a Google round trip only
/// through the `opendocListKind`/`opendocChecked` extension; these symbols are
/// the best-effort reading of a checklist authored elsewhere.
const UNCHECKED_GLYPHS: [&str; 2] = ["\u{2610}", "\u{274F}"];
const CHECKED_GLYPHS: [&str; 2] = ["\u{2611}", "\u{2612}"];

pub(crate) const UNCHECKED_GLYPH: &str = "\u{2610}";
pub(crate) const CHECKED_GLYPH: &str = "\u{2611}";

/// `document.lists`, reduced to the one thing OpenDoc models: whether each
/// nesting level numbers, bullets or checks its items.
#[derive(Debug, Default)]
pub(crate) struct GoogleLists {
    by_id: BTreeMap<String, BTreeMap<u8, ListKind>>,
    starts: BTreeMap<String, ListProperties>,
}

impl GoogleLists {
    pub(crate) fn parse(
        document: &Value,
        warnings: &mut Vec<ModelWarning>,
    ) -> Result<Self, ImportError> {
        let Some(lists) = optional_object(document, "lists")? else {
            return Ok(Self::default());
        };
        let mut by_id = BTreeMap::new();
        let mut starts = BTreeMap::new();
        for (list_id, list) in lists.as_object().into_iter().flatten() {
            let Some(properties) = optional_object(list, "listProperties")? else {
                continue;
            };
            let Some(levels) = properties.get("nestingLevels").and_then(Value::as_array) else {
                continue;
            };
            let mut kinds = BTreeMap::new();
            let mut properties_by_level = ListProperties::default();
            for (index, level) in levels.iter().enumerate() {
                let Ok(index) = u8::try_from(index) else {
                    break;
                };
                kinds.insert(index, nesting_level_kind(level, warnings)?);
                report_unrepresentable_glyph_format(
                    list_id,
                    index,
                    kinds.get(&index).copied(),
                    level,
                    warnings,
                )?;
                if matches!(kinds.get(&index), Some(ListKind::Ordered)) {
                    if let Some(glyph_type) = optional_str(level, "glyphType")? {
                        match google_ordered_format(glyph_type) {
                            Some(format) => {
                                // An omitted map entry is the model's depth-cycle
                                // inheritance. Keep an explicit Google format only
                                // when it changes that inherited rendering.
                                if format != OrderedListFormat::inherited_at(index) {
                                    properties_by_level.ordered_formats.insert(index, format);
                                }
                            }
                            // ZERO_DECIMAL carries leading-zero presentation that
                            // the bounded counter model deliberately lacks. It is
                            // still an ordered list, but must not silently claim
                            // that decimal is lossless.
                            None if glyph_type == "ZERO_DECIMAL" => warnings.push(warning(
                                UNREPRESENTABLE_LIST_FORMAT,
                                &format!("Google Docs list {list_id} level {index} uses ZERO_DECIMAL numbering; imported as decimal without leading-zero formatting"),
                            )),
                            None => {}
                        }
                    }
                }
                if matches!(kinds.get(&index), Some(ListKind::Bullet)) {
                    if let Some(symbol) = optional_str(level, "glyphSymbol")? {
                        let symbol = symbol.trim();
                        match google_bullet_marker(symbol) {
                            Some(marker) if marker != BulletListMarker::inherited_at(index) => {
                                properties_by_level.bullet_markers.insert(index, marker);
                            }
                            Some(_) => {}
                            // A literal marker that is unsafe for the CSS
                            // renderer or exceeds the bounded custom-marker
                            // vocabulary has no model home. Falling back to
                            // the inherited glyph is safe, but must not make
                            // a source-specific Google bullet look preserved.
                            None => warnings.push(warning(
                                "google-unrepresentable-bullet-marker",
                                &format!(
                                    "Google Docs list {list_id} level {index} glyphSymbol is not a supported OpenDoc bullet marker and was dropped"
                                ),
                            )),
                        }
                    }
                }
                if let Some(start) = optional_u64(level, "startNumber")? {
                    if !matches!(kinds.get(&index), Some(ListKind::Ordered)) {
                        warnings.push(warning(
                            UNREPRESENTABLE_LIST_START,
                            &format!(
                                "Google Docs list {list_id} level {index} specifies startNumber {start} for a non-ordered marker; it has no OpenDoc representation and was dropped"
                            ),
                        ));
                        continue;
                    }
                    let Ok(start) = u32::try_from(start) else {
                        warnings.push(warning(
                            "google-list-start-out-of-range",
                            &format!("Google Docs list {list_id} level {index} startNumber exceeds OpenDoc's u32 range and was ignored"),
                        ));
                        continue;
                    };
                    if start == 0 {
                        warnings.push(warning(
                            "google-list-start-invalid",
                            &format!("Google Docs list {list_id} level {index} startNumber is zero and was ignored"),
                        ));
                    } else {
                        properties_by_level.ordered_starts.insert(index, start);
                    }
                }
            }
            by_id.insert(list_id.clone(), kinds);
            if !properties_by_level.is_empty() {
                starts.insert(list_id.clone(), properties_by_level);
            }
        }
        Ok(Self { by_id, starts })
    }

    fn kind(&self, list_id: &str, level: u8) -> Option<ListKind> {
        self.by_id.get(list_id)?.get(&level).copied()
    }

    /// Settings exactly stated by Google's list definition.  The importer
    /// only installs entries for list ids actually used by the imported body,
    /// so a dangling Google definition cannot create dormant source state.
    pub(crate) fn properties_for(&self, list_id: &str) -> Option<&ListProperties> {
        self.starts.get(list_id)
    }

    /// Resolves a paragraph's list marker, preferring the explicit OpenDoc
    /// extension (exact, including per-item checked state) over the Google
    /// nesting level (which cannot express a checklist at all).
    pub(crate) fn resolve(
        &self,
        bullet: &Value,
        list_id: &str,
        level: u8,
        warnings: &mut Vec<ModelWarning>,
    ) -> Result<ListKind, ImportError> {
        if let Some(kind) = optional_str(bullet, "opendocListKind")? {
            return match kind {
                "bullet" => Ok(ListKind::Bullet),
                "ordered" => Ok(ListKind::Ordered),
                "checklist" => Ok(ListKind::Checklist {
                    checked: optional_bool(bullet, "opendocChecked")?.unwrap_or(false),
                }),
                other => Err(ImportError::InvalidInput(format!(
                    "unknown opendocListKind {other}"
                ))),
            };
        }
        match self.kind(list_id, level) {
            Some(kind) => Ok(kind),
            None => {
                warnings.push(warning(
                    UNKNOWN_LIST_DEFINITION,
                    &format!(
                        "Google Docs list {list_id} has no nesting level {level}; imported as a bullet item"
                    ),
                ));
                Ok(ListKind::Bullet)
            }
        }
    }
}

/// Google stores the marker template separately from the counter's glyph
/// type: `%0)` and `%0.` have the same `glyphType` but different visible
/// punctuation.  Do not silently turn the former into the latter just because
/// the bounded OpenDoc model deliberately has no arbitrary template field.
fn report_unrepresentable_glyph_format(
    list_id: &str,
    level_index: u8,
    kind: Option<ListKind>,
    level: &Value,
    warnings: &mut Vec<ModelWarning>,
) -> Result<(), ImportError> {
    let Some(format) = optional_str(level, "glyphFormat")? else {
        return Ok(());
    };
    let canonical = match kind {
        Some(ListKind::Ordered) => "%0.",
        Some(ListKind::Bullet | ListKind::Checklist { .. }) | None => "%0",
    };
    if format != canonical {
        warnings.push(warning(
            UNREPRESENTABLE_LIST_GLYPH_FORMAT,
            &format!(
                "Google Docs list {list_id} level {level_index} glyphFormat {format:?} is not representable and was imported using OpenDoc's {canonical:?} marker template"
            ),
        ));
    }
    Ok(())
}

fn nesting_level_kind(
    level: &Value,
    warnings: &mut Vec<ModelWarning>,
) -> Result<ListKind, ImportError> {
    if let Some(glyph_type) = optional_str(level, "glyphType")? {
        if ORDERED_GLYPH_TYPES.contains(&glyph_type) {
            return Ok(ListKind::Ordered);
        }
        if !matches!(glyph_type, "GLYPH_TYPE_UNSPECIFIED" | "NONE") {
            warnings.push(warning(
                UNKNOWN_LIST_DEFINITION,
                &format!("unknown Google Docs glyph type {glyph_type}; imported as a bullet item"),
            ));
        }
    }
    if let Some(symbol) = optional_str(level, "glyphSymbol")? {
        let symbol = symbol.trim();
        if UNCHECKED_GLYPHS.contains(&symbol) {
            return Ok(ListKind::Checklist { checked: false });
        }
        if CHECKED_GLYPHS.contains(&symbol) {
            return Ok(ListKind::Checklist { checked: true });
        }
    }
    Ok(ListKind::Bullet)
}

/// The Google glyph types with an exact one-counter OpenDoc equivalent.
/// `ZERO_DECIMAL` is intentionally absent: leading-zero width is a distinct
/// numbering rule, not a decimal counter value.
fn google_ordered_format(glyph_type: &str) -> Option<OrderedListFormat> {
    match glyph_type {
        "DECIMAL" => Some(OrderedListFormat::Decimal),
        "ALPHA" => Some(OrderedListFormat::LowerAlpha),
        "UPPER_ALPHA" => Some(OrderedListFormat::UpperAlpha),
        "ROMAN" => Some(OrderedListFormat::LowerRoman),
        "UPPER_ROMAN" => Some(OrderedListFormat::UpperRoman),
        _ => None,
    }
}

fn google_ordered_glyph_type(format: OrderedListFormat) -> &'static str {
    match format {
        OrderedListFormat::Decimal => "DECIMAL",
        OrderedListFormat::LowerAlpha => "ALPHA",
        OrderedListFormat::UpperAlpha => "UPPER_ALPHA",
        OrderedListFormat::LowerRoman => "ROMAN",
        OrderedListFormat::UpperRoman => "UPPER_ROMAN",
    }
}

/// Builds `document.lists` from the list items in every exported block root,
/// so an exported
/// document carries real Google list definitions instead of a non-standard
/// `bullet.ordered` flag.
pub(crate) fn export_lists(
    block_roots: &[&[Block]],
    list_properties: &BTreeMap<StableId, ListProperties>,
    warnings: &mut Vec<ModelWarning>,
) -> Option<Value> {
    let mut by_id: BTreeMap<String, BTreeMap<u8, ListKind>> = BTreeMap::new();
    for blocks in block_roots {
        collect_list_levels(blocks, &mut by_id);
    }
    if by_id.is_empty() {
        return None;
    }
    if by_id
        .values()
        .flat_map(BTreeMap::values)
        .any(|kind| matches!(kind, ListKind::Checklist { .. }))
    {
        warnings.push(warning(
            CHECKLIST_AS_BULLET,
            "Google Docs has no checklist list type, so checklists were exported as ballot-box bullets plus an opendocListKind extension carrying the checked state",
        ));
    }
    let mut lists = Map::new();
    for (list_id, levels) in by_id {
        let depth = levels.keys().copied().max().unwrap_or(0);
        let nesting_levels = (0..=depth)
            .map(|level| {
                nesting_level_value(
                    levels.get(&level).copied(),
                    list_properties
                        .iter()
                        .find(|(id, _)| id.as_str() == list_id)
                        .map(|(_, properties)| properties),
                    level,
                )
            })
            .collect::<Vec<_>>();
        lists.insert(
            list_id,
            json!({ "listProperties": { "nestingLevels": nesting_levels } }),
        );
    }
    Some(Value::Object(lists))
}

fn collect_list_levels(blocks: &[Block], out: &mut BTreeMap<String, BTreeMap<u8, ListKind>>) {
    for block in blocks {
        match &block.kind {
            BlockKind::ListItem {
                list_id,
                level,
                kind,
            } => {
                out.entry(list_id.to_string())
                    .or_default()
                    .entry(*level)
                    .or_insert(*kind);
            }
            BlockKind::Table { rows, .. } => {
                for row in rows {
                    for cell in &row.cells {
                        collect_list_levels(&cell.blocks, out);
                    }
                }
            }
            _ => {}
        }
    }
}

fn nesting_level_value(
    kind: Option<ListKind>,
    properties: Option<&ListProperties>,
    level: u8,
) -> Value {
    let start = properties
        .map(|properties| properties.start_for(level))
        .filter(|start| *start != 1);
    match kind {
        Some(ListKind::Ordered) => {
            let format = properties
                .map(|properties| properties.format_for(level))
                .unwrap_or_else(|| OrderedListFormat::inherited_at(level));
            let mut level =
                json!({ "glyphType": google_ordered_glyph_type(format), "glyphFormat": "%0." });
            if let Some(start) = start {
                level["startNumber"] = json!(start);
            }
            level
        }
        Some(ListKind::Checklist { checked: true }) => {
            json!({ "glyphSymbol": CHECKED_GLYPH, "glyphFormat": "%0" })
        }
        Some(ListKind::Checklist { checked: false }) => {
            json!({ "glyphSymbol": UNCHECKED_GLYPH, "glyphFormat": "%0" })
        }
        // A level no item in the document occupies still needs a slot so the
        // array index keeps meaning "nesting level".
        Some(ListKind::Bullet) | None => {
            let marker = properties
                .map(|properties| properties.bullet_marker_for(level))
                .unwrap_or_else(|| BulletListMarker::inherited_at(level));
            json!({ "glyphSymbol": marker.glyph().to_string(), "glyphFormat": "%0" })
        }
    }
}

fn google_bullet_marker(symbol: &str) -> Option<BulletListMarker> {
    match symbol {
        "\u{2022}" | "\u{25cf}" => Some(BulletListMarker::Disc),
        "\u{25e6}" | "\u{25cb}" => Some(BulletListMarker::Circle),
        "\u{25a0}" => Some(BulletListMarker::Square),
        _ => BulletListMarker::parse(symbol),
    }
}

/// The `bullet` object for one exported list item. The `opendoc*` fields are
/// the only lossless carrier for a checklist; `nestingLevel` and the document's
/// `lists` entry are what a Google consumer reads.
pub(crate) fn export_bullet(list_id: &StableId, level: &u8, kind: &ListKind) -> Value {
    let mut bullet = Map::new();
    bullet.insert("listId".to_string(), json!(list_id.to_string()));
    bullet.insert("nestingLevel".to_string(), json!(level));
    bullet.insert(
        "opendocListKind".to_string(),
        Value::String(
            match kind {
                ListKind::Bullet => "bullet",
                ListKind::Ordered => "ordered",
                ListKind::Checklist { .. } => "checklist",
            }
            .to_string(),
        ),
    );
    if let ListKind::Checklist { checked } = kind {
        bullet.insert("opendocChecked".to_string(), Value::Bool(*checked));
    }
    Value::Object(bullet)
}
