//! CSS projection of typed lengths, block properties and cell styles.

use crate::html::css_value;
use crate::*;

/// Twips (twentieths of a point) as a CSS `pt` length.
///
/// `pt` rather than `px` because the conversion is then exact and total:
/// 20 twips *is* 1pt, so every twip value lands on a 0.05pt grid with no
/// rounding, and the browser does the pt -> device-pixel step itself. A px
/// projection would have to assume a DPI (96 here) and would drift on any
/// value that is not a whole multiple of 0.75pt.
/// The `style` attribute an image's display size projects to, or `""` when
/// the document states no size.
///
/// A stated axis becomes an exact `pt` length; an unstated one becomes `auto`
/// only when the *other* axis is stated, so a one-axis resize keeps the
/// picture's aspect ratio instead of stretching it. With neither axis stated
/// no attribute is written at all, which is what leaves the image at its
/// intrinsic size.
pub(crate) fn image_size_css(layout: &ImageLayout) -> String {
    if layout.width.is_none() && layout.height.is_none() {
        return String::new();
    }
    let axis = |length: Option<opendoc_core::Length>| match length {
        Some(length) => twips_to_css_pt(length.twips()),
        None => "auto".to_string(),
    };
    format!(
        " style=\"width: {}; height: {};\"",
        axis(layout.width),
        axis(layout.height)
    )
}

pub(crate) fn twips_to_css_pt(twips: i32) -> String {
    let sign = if twips < 0 { "-" } else { "" };
    let magnitude = twips.unsigned_abs();
    let points = magnitude / 20;
    // One twip is 0.05pt, so the remainder is always a whole number of
    // hundredths and two decimals are exact.
    let hundredths = (magnitude % 20) * 5;
    if hundredths == 0 {
        format!("{sign}{points}pt")
    } else {
        format!("{sign}{points}.{hundredths:02}pt")
    }
}

/// Thousandths of a line as a unitless CSS `line-height` number.
pub(crate) fn thousandths_to_css_number(thousandths: u32) -> String {
    let whole = thousandths / 1_000;
    let frac = thousandths % 1_000;
    if frac == 0 {
        format!("{whole}")
    } else {
        format!("{whole}.{frac:03}")
            .trim_end_matches('0')
            .to_string()
    }
}

/// Projects typed block properties onto CSS declarations for the block
/// element. Pure: it reads the properties and returns a string.
///
/// `None` means inherit, so an unset property emits nothing at all rather than
/// a default that would override a stylesheet.
/// What an auto-width column is worth when working out how narrow the table
/// may get: one inch, the width Word gives a column it has no opinion about.
pub(crate) const DEFAULT_TABLE_COLUMN_POINTS: f64 = 72.0;

/// A CSS length in points, printed without trailing zeroes so two replicas
/// that agree on the twips agree on the markup.
pub(crate) fn points(value: f64) -> String {
    let rounded = (value * 100.0).round() / 100.0;
    if rounded == rounded.trunc() {
        format!("{}pt", rounded.trunc() as i64)
    } else {
        format!("{rounded}pt")
    }
}

/// Cell-level formatting as CSS declarations.
///
/// `start`/`end` borders and padding are direction-relative in the model, and
/// the CSS logical properties of the same name are too, so a right-to-left
/// table needs no special case here.
pub(crate) fn table_cell_css(properties: &opendoc_core::TableCellProperties) -> String {
    let mut css = String::new();
    if let Some(background) = properties.background {
        let _ = write!(css, "background-color:{};", background.as_hex());
    }
    let border = |edge: &str, value: Option<opendoc_core::CellBorder>, css: &mut String| {
        let Some(border) = value else {
            return;
        };
        if border.style() == opendoc_core::BorderStyle::None {
            let _ = write!(css, "border-{edge}:none;");
            return;
        }
        let _ = write!(
            css,
            "border-{edge}:{} {} {};",
            points(border.width().points()),
            border.style().as_str(),
            border.color().as_hex()
        );
    };
    border("block-start", properties.border_top, &mut css);
    border("block-end", properties.border_bottom, &mut css);
    border("inline-start", properties.border_start, &mut css);
    border("inline-end", properties.border_end, &mut css);
    if let Some(alignment) = properties.vertical_alignment {
        let _ = write!(css, "vertical-align:{};", alignment.as_str());
    }
    let padding = |edge: &str, value: Option<opendoc_core::Length>, css: &mut String| {
        if let Some(length) = value {
            let _ = write!(css, "padding-{edge}:{};", points(length.points()));
        }
    };
    padding("block-start", properties.padding_top, &mut css);
    padding("block-end", properties.padding_bottom, &mut css);
    padding("inline-start", properties.padding_start, &mut css);
    padding("inline-end", properties.padding_end, &mut css);
    css
}

pub(crate) fn block_property_css(properties: &BlockProperties) -> String {
    let mut css = String::new();
    if let Some(alignment) = properties.alignment {
        // `Alignment::{Start, End}` are direction-relative, and so are the CSS
        // keywords of the same name, so the projection needs no knowledge of
        // the block's direction to get right-to-left text right.
        let _ = write!(css, "text-align:{};", alignment.as_str());
    }
    if let Some(length) = properties.indent_start {
        // Logical, not `margin-left`: the model's start/end indents flip with
        // the block direction and `margin-inline-*` is the same rule. In a
        // left-to-right block the used value is `margin-left`.
        let _ = write!(
            css,
            "margin-inline-start:{};",
            twips_to_css_pt(length.twips())
        );
    }
    if let Some(length) = properties.indent_end {
        let _ = write!(
            css,
            "margin-inline-end:{};",
            twips_to_css_pt(length.twips())
        );
    }
    if let Some(length) = properties.indent_first_line {
        // CSS `text-indent` is measured from the content edge, i.e. after the
        // start indent — exactly what the model means by "relative to
        // IndentStart". A negative value hangs the first line, which is the
        // model's only representation of a hanging indent.
        let _ = write!(css, "text-indent:{};", twips_to_css_pt(length.twips()));
    }
    if let Some(spacing) = properties.line_spacing {
        match spacing {
            LineSpacing::Multiple(multiple) => {
                // Unitless: it multiplies the element's own font size and is
                // inherited as a ratio, which is what "a multiple of the
                // natural line height" means.
                let _ = write!(
                    css,
                    "line-height:{};",
                    thousandths_to_css_number(multiple.thousandths())
                );
            }
            // CSS has one line-height rule and it behaves as `AtLeast`: the
            // declared height sets the strut, and a taller inline box on the
            // line still grows the line box. `Exact` is therefore projected to
            // the same declaration and is an approximation — HTML has no way
            // to clip a line to a fixed height. The source keeps the
            // distinction; only this projection loses it.
            LineSpacing::Exact(height) | LineSpacing::AtLeast(height) => {
                let _ = write!(css, "line-height:{};", twips_to_css_pt(height.twips()));
            }
        }
    }
    if let Some(length) = properties.space_before {
        let _ = write!(css, "margin-top:{};", twips_to_css_pt(length.twips()));
    }
    if let Some(length) = properties.space_after {
        let _ = write!(css, "margin-bottom:{};", twips_to_css_pt(length.twips()));
    }
    if let Some(direction) = properties.direction {
        let _ = write!(css, "direction:{};", direction.as_str());
    }
    css
}

pub(crate) fn block_kind_label(kind: &BlockKind) -> &'static str {
    match kind {
        BlockKind::Paragraph => "paragraph",
        BlockKind::Heading { .. } => "heading",
        BlockKind::ListItem { .. } => "list-item",
        BlockKind::Table { .. } => "table",
        BlockKind::EquationBlock { .. } => "equation-block",
        BlockKind::Image { .. } => "image",
        BlockKind::PageBreak => "page-break",
    }
}

pub(crate) fn mark_classes(marks: &[Mark]) -> Vec<&'static str> {
    let mut classes = Vec::new();
    for mark in marks {
        let class = match mark.kind {
            MarkKind::Bold => "mark-bold",
            MarkKind::Italic => "mark-italic",
            MarkKind::Underline => "mark-underline",
            MarkKind::Strike => "mark-strike",
            MarkKind::Code => "mark-code",
            MarkKind::Superscript => "mark-superscript",
            MarkKind::Subscript => "mark-subscript",
            MarkKind::Color => "mark-color",
            MarkKind::Background => "mark-background",
            MarkKind::Font => "mark-font",
            MarkKind::Size => "mark-size",
            MarkKind::Link | MarkKind::Citation => continue,
        };
        if !classes.contains(&class) {
            classes.push(class);
        }
    }
    classes
}

pub(crate) fn write_mark_style(marks: &[Mark], out: &mut String) {
    let mut style = String::new();
    for mark in marks {
        let Some(value) = mark.value.as_deref().and_then(css_value) else {
            continue;
        };
        match mark.kind {
            MarkKind::Color => {
                let _ = write!(style, "color:{value};");
            }
            MarkKind::Background => {
                let _ = write!(style, "background-color:{value};");
            }
            MarkKind::Font => {
                let _ = write!(style, "font-family:{value};");
            }
            MarkKind::Size => {
                let size = if value.chars().all(|ch| ch.is_ascii_digit() || ch == '.') {
                    format!("{value}pt")
                } else {
                    value
                };
                let _ = write!(style, "font-size:{size};");
            }
            _ => {}
        }
    }
    if !style.is_empty() {
        let _ = write!(out, " style=\"{style}\"");
    }
}
