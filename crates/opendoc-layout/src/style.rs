//! The document type scale: the one place the sizes, leading and spacing of
//! the document surface are stated.
//!
//! These numbers decide two things that have to agree: how tall this crate
//! computes a block to be, and how tall Chrome actually draws it. So they are
//! not written twice. [`TypeScale::css_variables`] projects the scale as
//! custom properties, the stylesheet consumes those properties instead of
//! literals, and the layout engine measures against the same struct. Changing
//! a heading size changes both, or neither.
//!
//! Everything is in twips (20 to the point, `opendoc_core::Length`'s unit).
//! Values that the stylesheet historically expressed in CSS pixels are
//! recorded here in their exact twip equivalent — 1px is 0.75pt is 15 twips
//! at the 96dpi reference the CSS `px` unit is defined against.

use std::fmt::Write;

use crate::font;

/// Twips in one CSS reference pixel.
pub const TWIPS_PER_PX: i32 = 15;
/// Twips in one point.
pub const TWIPS_PER_PT: i32 = 20;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypeScale {
    /// Body text size.
    pub body_size: i32,
    /// Default leading, as thousandths of the element's own font size, which
    /// is what a unitless CSS `line-height` means.
    pub line_height_thousandths: u32,
    /// The gap a paragraph, heading, list, table, image or equation leaves
    /// below itself.
    pub block_space_after: i32,
    /// Sizes for `h1`..`h6`.
    pub heading_size: [i32; 6],
    /// Extra space above `h1`..`h6`.
    pub heading_space_before: [i32; 6],
    /// The indent a list level adds.
    pub list_indent: i32,
    /// The indent a checklist level adds: the checkbox is the marker, so the
    /// list reclaims the room a bullet would have used.
    pub checklist_indent: i32,
    /// The checkbox's own box.
    pub checkbox_size: i32,
    /// The gap after a checkbox, as thousandths of the item's font size.
    pub checkbox_gap_thousandths: u32,
    /// Image captions and footnote bodies.
    pub caption_size: i32,
    /// Space above and below the rule an explicit page break draws.
    pub page_break_space: i32,
    /// The rule itself.
    pub page_break_rule: i32,
    /// Space above a block image or a block equation.
    pub float_block_space_before: i32,
    /// A table cell's vertical padding.
    pub cell_padding_block: i32,
    /// A table cell's horizontal padding.
    pub cell_padding_inline: i32,
    /// A collapsed table border.
    pub cell_border: i32,
    /// Superscript and subscript size, as thousandths of the surrounding
    /// size.
    pub script_size_thousandths: u32,
}

impl Default for TypeScale {
    fn default() -> Self {
        Self {
            body_size: 11 * TWIPS_PER_PT,
            line_height_thousandths: 1_500,
            block_space_after: 10 * TWIPS_PER_PT,
            heading_size: [
                20 * TWIPS_PER_PT,
                16 * TWIPS_PER_PT,
                14 * TWIPS_PER_PT,
                12 * TWIPS_PER_PT,
                11 * TWIPS_PER_PT,
                11 * TWIPS_PER_PT,
            ],
            heading_space_before: [
                16 * TWIPS_PER_PT,
                14 * TWIPS_PER_PT,
                12 * TWIPS_PER_PT,
                0,
                0,
                0,
            ],
            list_indent: 28 * TWIPS_PER_PX,
            checklist_indent: 4 * TWIPS_PER_PX,
            checkbox_size: 13 * TWIPS_PER_PX,
            checkbox_gap_thousandths: 450,
            caption_size: 9 * TWIPS_PER_PT,
            page_break_space: 24 * TWIPS_PER_PT,
            page_break_rule: TWIPS_PER_PX,
            float_block_space_before: 10 * TWIPS_PER_PT,
            cell_padding_block: 4 * TWIPS_PER_PX,
            cell_padding_inline: 8 * TWIPS_PER_PX,
            cell_border: TWIPS_PER_PX,
            script_size_thousandths: 750,
        }
    }
}

impl TypeScale {
    /// Size for a heading level, clamped the way the renderer clamps it.
    pub fn heading(&self, level: u8) -> i32 {
        let index = usize::from(level.clamp(1, 6)) - 1;
        self.heading_size[index]
    }

    /// Space above a heading level.
    pub fn heading_space(&self, level: u8) -> i32 {
        let index = usize::from(level.clamp(1, 6)) - 1;
        self.heading_space_before[index]
    }

    /// The scale as CSS custom properties, ready for a `style` attribute on
    /// the document element.
    ///
    /// The stylesheet keeps its own copies of these values as `:root`
    /// fallbacks so the surface is not unstyled before the first layout
    /// arrives — the same arrangement `--page-*` already uses — but the
    /// values here are the ones that were measured against.
    pub fn css_variables(&self) -> String {
        let mut out = String::new();
        let _ = write!(
            out,
            "--doc-font-family: \"{}\", Arial, Helvetica, sans-serif; ",
            font::SANS_FAMILY
        );
        let _ = write!(
            out,
            "--doc-mono-family: \"{}\", \"Courier New\", monospace; ",
            font::MONO_FAMILY
        );
        let mut length = |name: &str, twips: i32| {
            let _ = write!(out, "{name}: {}; ", css_pt(twips));
        };
        length("--doc-font-size", self.body_size);
        length("--doc-block-space-after", self.block_space_after);
        for (index, size) in self.heading_size.iter().enumerate() {
            length(&format!("--doc-h{}-size", index + 1), *size);
        }
        for (index, space) in self.heading_space_before.iter().enumerate() {
            length(&format!("--doc-h{}-space-before", index + 1), *space);
        }
        length("--doc-list-indent", self.list_indent);
        length("--doc-checklist-indent", self.checklist_indent);
        length("--doc-checkbox-size", self.checkbox_size);
        length("--doc-caption-size", self.caption_size);
        length("--doc-page-break-space", self.page_break_space);
        length("--doc-page-break-rule", self.page_break_rule);
        length("--doc-float-space", self.float_block_space_before);
        length("--doc-cell-padding-block", self.cell_padding_block);
        length("--doc-cell-padding-inline", self.cell_padding_inline);
        length("--doc-cell-border", self.cell_border);
        let _ = write!(
            out,
            "--doc-line-height: {}; ",
            css_number(self.line_height_thousandths)
        );
        let _ = write!(
            out,
            "--doc-checkbox-gap: {}em; ",
            css_number(self.checkbox_gap_thousandths)
        );
        let _ = write!(
            out,
            "--doc-script-size: {}em;",
            css_number(self.script_size_thousandths)
        );
        out
    }
}

/// Twips as an exact CSS length in points. One twip is 0.05pt, so two
/// decimals never round.
pub fn css_pt(twips: i32) -> String {
    let sign = if twips < 0 { "-" } else { "" };
    let magnitude = twips.unsigned_abs();
    let points = magnitude / 20;
    let hundredths = (magnitude % 20) * 5;
    if hundredths == 0 {
        format!("{sign}{points}pt")
    } else {
        format!("{sign}{points}.{hundredths:02}pt")
    }
}

/// Milli-twips as an exact CSS length in points. One milli-twip is 0.00005pt,
/// so five decimals never round either — which matters for the one value the
/// frontend applies verbatim, the margin that opens a page.
pub fn css_pt_milli(milli_twips: i64) -> String {
    let sign = if milli_twips < 0 { "-" } else { "" };
    let magnitude = milli_twips.unsigned_abs();
    let points = magnitude / 20_000;
    let fraction = (magnitude % 20_000) * 5;
    if fraction == 0 {
        format!("{sign}{points}pt")
    } else {
        let text = format!("{fraction:05}");
        format!("{sign}{points}.{}pt", text.trim_end_matches('0'))
    }
}

fn css_number(thousandths: u32) -> String {
    let whole = thousandths / 1_000;
    let frac = thousandths % 1_000;
    if frac == 0 {
        format!("{whole}")
    } else {
        format!("{whole}.{}", format!("{frac:03}").trim_end_matches('0'))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn twips_project_to_exact_points() {
        assert_eq!(css_pt(220), "11pt");
        assert_eq!(css_pt(15), "0.75pt");
        assert_eq!(css_pt(-241), "-12.05pt");
        assert_eq!(css_pt(0), "0pt");
    }

    #[test]
    fn milli_twips_project_to_exact_points() {
        assert_eq!(css_pt_milli(20_000), "1pt");
        assert_eq!(css_pt_milli(1), "0.00005pt");
        assert_eq!(css_pt_milli(330_000), "16.5pt");
        assert_eq!(css_pt_milli(-10_000), "-0.5pt");
    }

    #[test]
    fn the_scale_projects_every_value_it_measures_with() {
        let css = TypeScale::default().css_variables();
        for name in [
            "--doc-font-family",
            "--doc-font-size",
            "--doc-line-height",
            "--doc-block-space-after",
            "--doc-h1-size",
            "--doc-h6-size",
            "--doc-h1-space-before",
            "--doc-list-indent",
            "--doc-checklist-indent",
            "--doc-page-break-space",
            "--doc-cell-padding-inline",
            "--doc-script-size",
        ] {
            assert!(
                css.contains(name),
                "{name} is measured with but not projected"
            );
        }
        assert!(css.contains("--doc-h1-size: 20pt"));
        assert!(css.contains("--doc-line-height: 1.5"));
        // 28 CSS pixels, stated in the model's unit and projected exactly.
        assert!(css.contains("--doc-list-indent: 21pt"));
    }
}
