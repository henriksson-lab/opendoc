//! Run and paragraph property parsing, and what the core model cannot hold.

use crate::docx::warnings::{
    APPROXIMATED_ALIGNMENT, DROPPED_ALIGNMENT, DROPPED_INDENT, DROPPED_PARAGRAPH_BORDER,
    DROPPED_PARAGRAPH_SHADING, DROPPED_SPACING, DROPPED_TABS,
};
use crate::mark;
use crate::xml::XmlElement;
use opendoc_core::{
    Alignment, BlockProperties, BlockPropertyKey, Length, LineSpacing, Mark, MarkKind,
    TextDirection,
};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct RunProps {
    bold: Option<bool>,
    italic: Option<bool>,
    underline: Option<bool>,
    strike: Option<bool>,
    vert_align: Option<String>,
    color: Option<String>,
    background: Option<String>,
    size: Option<u32>,
    font: Option<String>,
}

impl RunProps {
    pub(super) fn overlay(&mut self, over: &RunProps) {
        if over.bold.is_some() {
            self.bold = over.bold;
        }
        if over.italic.is_some() {
            self.italic = over.italic;
        }
        if over.underline.is_some() {
            self.underline = over.underline;
        }
        if over.strike.is_some() {
            self.strike = over.strike;
        }
        if over.vert_align.is_some() {
            self.vert_align.clone_from(&over.vert_align);
        }
        if over.color.is_some() {
            self.color.clone_from(&over.color);
        }
        if over.background.is_some() {
            self.background.clone_from(&over.background);
        }
        if over.size.is_some() {
            self.size = over.size;
        }
        if over.font.is_some() {
            self.font.clone_from(&over.font);
        }
    }

    pub(super) fn marks(&self) -> Vec<Mark> {
        let mut marks = Vec::new();
        if self.bold == Some(true) {
            marks.push(mark(MarkKind::Bold, None));
        }
        if self.italic == Some(true) {
            marks.push(mark(MarkKind::Italic, None));
        }
        if self.underline == Some(true) {
            marks.push(mark(MarkKind::Underline, None));
        }
        if self.strike == Some(true) {
            marks.push(mark(MarkKind::Strike, None));
        }
        match self.vert_align.as_deref() {
            Some("superscript") => marks.push(mark(MarkKind::Superscript, None)),
            Some("subscript") => marks.push(mark(MarkKind::Subscript, None)),
            _ => {}
        }
        if let Some(color) = &self.color {
            marks.push(mark(MarkKind::Color, Some(color.clone())));
        }
        if let Some(background) = &self.background {
            marks.push(mark(MarkKind::Background, Some(background.clone())));
        }
        if let Some(font) = &self.font {
            marks.push(mark(MarkKind::Font, Some(font.clone())));
        }
        if let Some(size) = self.size {
            marks.push(mark(MarkKind::Size, Some(size.to_string())));
        }
        marks
    }
}

#[derive(Clone, Debug, Default)]
pub(super) struct ParsedRunProps {
    pub(super) props: RunProps,
    pub(super) style: Option<String>,
    /// Properties before a tracked formatting change (`w:rPrChange`).
    pub(super) previous: Option<RunProps>,
    pub(super) dropped: Vec<&'static str>,
}

pub(super) fn toggle_value(element: &XmlElement) -> bool {
    match element.attr("val") {
        None => true,
        Some(value) => !matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "off"
        ),
    }
}

pub(super) fn hex_color(value: &str) -> Option<String> {
    let value = value.trim();
    if value.len() == 6 && value.chars().all(|ch| ch.is_ascii_hexdigit()) {
        Some(format!("#{}", value.to_ascii_lowercase()))
    } else {
        None
    }
}

pub(super) fn highlight_color(value: &str) -> Option<String> {
    let hex = match value.trim() {
        "black" => "000000",
        "blue" => "0000ff",
        "cyan" => "00ffff",
        "green" => "00ff00",
        "magenta" => "ff00ff",
        "red" => "ff0000",
        "yellow" => "ffff00",
        "white" => "ffffff",
        "darkBlue" => "000080",
        "darkCyan" => "008080",
        "darkGreen" => "008000",
        "darkMagenta" => "800080",
        "darkRed" => "800000",
        "darkYellow" => "808000",
        "darkGray" => "808080",
        "lightGray" => "c0c0c0",
        _ => return None,
    };
    Some(format!("#{hex}"))
}

pub(super) fn parse_run_props(rpr: &XmlElement) -> ParsedRunProps {
    let mut parsed = ParsedRunProps::default();
    for element in rpr.elements() {
        match element.local.as_str() {
            "b" => parsed.props.bold = Some(toggle_value(element)),
            "i" => parsed.props.italic = Some(toggle_value(element)),
            "u" => {
                parsed.props.underline = Some(
                    !element
                        .attr("val")
                        .is_some_and(|value| value.trim().eq_ignore_ascii_case("none")),
                )
            }
            "strike" | "dstrike" => parsed.props.strike = Some(toggle_value(element)),
            "vertAlign" => {
                parsed.props.vert_align = element.attr("val").map(|value| value.trim().to_string())
            }
            "color" => {
                if let Some(color) = element.attr("val").and_then(hex_color) {
                    parsed.props.color = Some(color);
                }
            }
            "highlight" => {
                if let Some(color) = element.attr("val").and_then(highlight_color) {
                    parsed.props.background = Some(color);
                }
            }
            "shd" => {
                if parsed.props.background.is_none() {
                    if let Some(color) = element.attr("fill").and_then(hex_color) {
                        parsed.props.background = Some(color);
                    }
                }
            }
            "sz" => {
                if let Some(size) = element
                    .attr("val")
                    .and_then(|value| value.trim().parse::<u32>().ok())
                    .filter(|size| *size > 0)
                {
                    parsed.props.size = Some(size / 2);
                }
            }
            "rFonts" => {
                if let Some(font) = ["ascii", "hAnsi", "cs", "eastAsia"]
                    .iter()
                    .find_map(|attr| element.attr(attr))
                    .map(str::trim)
                    .filter(|font| !font.is_empty())
                {
                    parsed.props.font = Some(font.to_string());
                }
            }
            "rStyle" => parsed.style = element.attr("val").map(|value| value.to_string()),
            "rPrChange" => {
                parsed.previous = Some(
                    element
                        .child("rPr")
                        .map(|old| parse_run_props(old).props)
                        .unwrap_or_default(),
                );
            }
            "caps" | "smallCaps" | "vanish" | "emboss" | "imprint" | "outline" | "shadow"
            | "webHidden" => {
                if toggle_value(element) {
                    parsed.dropped.push(leak_name(&element.local));
                }
            }
            "spacing" | "w" | "kern" | "position" | "effect" | "em" | "fitText" => {
                parsed.dropped.push(leak_name(&element.local));
            }
            _ => {}
        }
    }
    parsed
}

/// Maps a known run-property name to a `'static` label for warning messages.
pub(super) fn leak_name(name: &str) -> &'static str {
    match name {
        "caps" => "caps",
        "smallCaps" => "smallCaps",
        "vanish" => "vanish",
        "emboss" => "emboss",
        "imprint" => "imprint",
        "outline" => "outline",
        "shadow" => "shadow",
        "webHidden" => "webHidden",
        "spacing" => "spacing",
        "w" => "w",
        "kern" => "kern",
        "position" => "position",
        "effect" => "effect",
        "em" => "em",
        "fitText" => "fitText",
        _ => "other",
    }
}

// ---------------------------------------------------------------------------
// Paragraph properties
// ---------------------------------------------------------------------------

/// `w:pPr` parsed into typed block properties, plus the warning codes for the
/// parts of it OpenDoc cannot represent.
///
/// DOCX measures paragraph geometry in twips and so does [`Length`], so every
/// mapped value here is exact: no unit conversion, no rounding drift.
#[derive(Clone, Debug, Default)]
pub(super) struct ParsedParaProps {
    pub(super) props: BlockProperties,
    pub(super) dropped: Vec<&'static str>,
}

/// Overlays every property `over` sets onto `base`, leaving the rest to
/// inherit. The block-level mirror of [`RunProps::overlay`].
pub(super) fn overlay_para_props(base: &mut BlockProperties, over: &BlockProperties) {
    for key in BlockPropertyKey::ALL {
        if let Some(property) = over.get(key) {
            base.set(property);
        }
    }
}

/// Reads a twips-valued attribute. A present-but-unusable value is reported
/// through `dropped` rather than silently becoming `None`.
pub(super) fn twips_attr(
    element: &XmlElement,
    name: &str,
    code: &'static str,
    dropped: &mut Vec<&'static str>,
) -> Option<Length> {
    let raw = element.attr(name)?;
    match raw
        .trim()
        .parse::<i32>()
        .map_err(|_| ())
        .and_then(|twips| Length::from_twips(twips).map_err(|_| ()))
    {
        Ok(length) => Some(length),
        Err(()) => {
            dropped.push(code);
            None
        }
    }
}

pub(super) fn docx_flag_attr(element: &XmlElement, name: &str) -> bool {
    element.attr(name).is_some_and(|value| {
        let value = value.trim();
        value == "1" || value.eq_ignore_ascii_case("true") || value.eq_ignore_ascii_case("on")
    })
}

pub(super) fn parse_para_props(ppr: &XmlElement) -> ParsedParaProps {
    let mut parsed = ParsedParaProps::default();
    for element in ppr.elements() {
        match element.local.as_str() {
            "jc" => parse_paragraph_alignment(element, &mut parsed),
            "ind" => parse_paragraph_indent(element, &mut parsed),
            "spacing" => parse_paragraph_spacing(element, &mut parsed),
            "bidi" => {
                parsed.props.direction = Some(if toggle_value(element) {
                    TextDirection::RightToLeft
                } else {
                    TextDirection::LeftToRight
                });
            }
            "pBdr" => parsed.dropped.push(DROPPED_PARAGRAPH_BORDER),
            "shd" => parsed.dropped.push(DROPPED_PARAGRAPH_SHADING),
            "tabs" => parsed.dropped.push(DROPPED_TABS),
            _ => {}
        }
    }
    parsed
}

pub(super) fn parse_paragraph_alignment(element: &XmlElement, parsed: &mut ParsedParaProps) {
    let value = element
        .attr("val")
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    parsed.props.alignment = match value.as_str() {
        "start" | "left" => Some(Alignment::Start),
        "center" => Some(Alignment::Center),
        "end" | "right" => Some(Alignment::End),
        "both" => Some(Alignment::Justify),
        // Distributed justification also stretches the last line. OpenDoc has
        // no such rule, so it lands on `Justify` and says it approximated.
        "distribute" | "thaidistribute" => {
            parsed.dropped.push(APPROXIMATED_ALIGNMENT);
            Some(Alignment::Justify)
        }
        _ => {
            parsed.dropped.push(DROPPED_ALIGNMENT);
            None
        }
    };
}

pub(super) fn parse_paragraph_indent(element: &XmlElement, parsed: &mut ParsedParaProps) {
    // `w:start`/`w:end` are the ISO names; `w:left`/`w:right` the transitional
    // ones Word still writes.
    if let Some(name) = ["start", "left"]
        .into_iter()
        .find(|name| element.attr(name).is_some())
    {
        parsed.props.indent_start = twips_attr(element, name, DROPPED_INDENT, &mut parsed.dropped);
    }
    if let Some(name) = ["end", "right"]
        .into_iter()
        .find(|name| element.attr(name).is_some())
    {
        parsed.props.indent_end = twips_attr(element, name, DROPPED_INDENT, &mut parsed.dropped);
    }
    // Both DOCX offsets are relative to the start indent, as OpenDoc's
    // first-line indent is; `w:hanging` is the negative direction and wins
    // over `w:firstLine` when both are present.
    if element.attr("hanging").is_some() {
        if let Some(length) = twips_attr(element, "hanging", DROPPED_INDENT, &mut parsed.dropped) {
            match Length::from_twips(-length.twips()) {
                Ok(hanging) => parsed.props.indent_first_line = Some(hanging),
                Err(_) => parsed.dropped.push(DROPPED_INDENT),
            }
        }
    } else if element.attr("firstLine").is_some() {
        parsed.props.indent_first_line =
            twips_attr(element, "firstLine", DROPPED_INDENT, &mut parsed.dropped);
    }
    // Character-relative indents need the paragraph's font metrics to resolve.
    if [
        "startChars",
        "leftChars",
        "endChars",
        "rightChars",
        "firstLineChars",
        "hangingChars",
    ]
    .into_iter()
    .any(|name| element.attr(name).is_some_and(|value| value.trim() != "0"))
    {
        parsed.dropped.push(DROPPED_INDENT);
    }
}

pub(super) fn parse_paragraph_spacing(element: &XmlElement, parsed: &mut ParsedParaProps) {
    for (name, before) in [("before", true), ("after", false)] {
        if element.attr(name).is_none() {
            continue;
        }
        let Some(length) = twips_attr(element, name, DROPPED_SPACING, &mut parsed.dropped) else {
            continue;
        };
        if length.is_negative() {
            parsed.dropped.push(DROPPED_SPACING);
            continue;
        }
        if before {
            parsed.props.space_before = Some(length);
        } else {
            parsed.props.space_after = Some(length);
        }
    }
    if let Some(raw) = element.attr("line") {
        // `w:lineRule` defaults to `auto`, where `w:line` counts 240ths of a
        // line; the other two rules measure twips directly.
        let rule = element
            .attr("lineRule")
            .unwrap_or("auto")
            .trim()
            .to_ascii_lowercase();
        let spacing =
            raw.trim()
                .parse::<i32>()
                .map_err(|_| ())
                .and_then(|line| match rule.as_str() {
                    "auto" => LineSpacing::multiple(f64::from(line) / 240.0).map_err(|_| ()),
                    "exact" => Length::from_twips(line)
                        .and_then(LineSpacing::exactly)
                        .map_err(|_| ()),
                    "atleast" => Length::from_twips(line)
                        .and_then(LineSpacing::at_least)
                        .map_err(|_| ()),
                    _ => Err(()),
                });
        match spacing {
            Ok(spacing) => parsed.props.line_spacing = Some(spacing),
            Err(()) => parsed.dropped.push(DROPPED_SPACING),
        }
    }
    // Line-relative spacing and "auto" spacing both need line metrics.
    if ["beforeLines", "afterLines"]
        .into_iter()
        .any(|name| element.attr(name).is_some_and(|value| value.trim() != "0"))
        || ["beforeAutospacing", "afterAutospacing"]
            .into_iter()
            .any(|name| docx_flag_attr(element, name))
    {
        parsed.dropped.push(DROPPED_SPACING);
    }
}

// ---------------------------------------------------------------------------
// Styles
// ---------------------------------------------------------------------------
