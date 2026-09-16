//! A table's borders: which line every cell boundary resolves to, and how
//! that line is drawn.
//!
//! ## Why there is a resolution step at all
//!
//! Two cells meet at every interior boundary, and each of them owns an edge
//! there: the left cell's `border_end` and the right cell's `border_start`.
//! The document may state both, and it may state them differently. Something
//! has to decide, or the boundary gets drawn twice — once in each cell's
//! colour, at each cell's width, which is not a thing any renderer draws.
//!
//! The screen already decides, and the rule it uses is not this crate's to
//! invent: `.doc-table` is `border-collapse: collapse`, so Chrome applies CSS
//! 2.1 §17.6.2.1's conflict resolution. Reproducing that rule is therefore the
//! *only* choice that keeps the paper agreeing with the screen, which is the
//! whole reason [ADR 0014](../../../docs/adr/0014-pagination-in-rust.md) put
//! layout in Rust. Inventing a different one — "the first cell wins", "draw
//! the thicker of the two and warn" — would be a second engine again.
//!
//! Three consequences of that rule are worth stating because they surprise:
//!
//! - **An unstated edge is not an absent border.** `.doc-table td` gives every
//!   cell `border: 0.75pt solid #999` on all four edges and the per-edge
//!   properties override it, so a cell that states nothing contributes the
//!   default, not nothing.
//! - **`BorderStyle::None` does not clear a shared boundary.** A `none` border
//!   has a used width of zero and the lowest style priority, so the *other*
//!   cell's border wins and the line is still drawn. Turning a line off on a
//!   shared boundary takes both cells. On the table's outer rim, where there
//!   is only one contributor, `none` does clear it.
//! - **Wider wins before prettier.** Width is compared first and style only
//!   breaks a tie, so a 2.25pt dotted border beats a 0.75pt double one.
//!
//! CSS's `hidden` — which would win over everything — has no counterpart in
//! [`BorderStyle`], so it needs no case here.

use opendoc_core::{BorderStyle, CellBorder};

use crate::paint::{Local, Rgb};
use crate::MILLI;

/// One edge's border, as a set of numbers rather than as a document property.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Border {
    pub style: BorderStyle,
    /// Twips, across the line.
    pub width: i32,
    pub color: Rgb,
}

impl Border {
    /// The border a cell property states.
    pub(crate) fn stated(border: CellBorder) -> Self {
        let (red, green, blue) = border.color().rgb();
        Self {
            style: border.style(),
            width: border.width().twips(),
            color: Rgb { red, green, blue },
        }
    }

    /// The *used* width, which is what the collapsing model compares.
    ///
    /// CSS computes a border's width to zero when its style is `none`, so a
    /// turned-off border loses to every real one on width alone, before style
    /// priority is even reached.
    fn used_width(self) -> i32 {
        if self.style == BorderStyle::None {
            0
        } else {
            self.width.max(0)
        }
    }

    /// Style priority: CSS 2.1 §17.6.2.1's list, restricted to the styles this
    /// model has. Higher wins.
    fn rank(self) -> u8 {
        match self.style {
            BorderStyle::None => 0,
            BorderStyle::Dotted => 1,
            BorderStyle::Dashed => 2,
            BorderStyle::Solid => 3,
            BorderStyle::Double => 4,
        }
    }
}

/// The single border one boundary resolves to, or `None` when nothing is drawn
/// there.
///
/// `earlier` is the contributor from the cell that comes first in document
/// order — the one above for a horizontal boundary, the one to the left for a
/// vertical one in a left-to-right table. It wins ties, which is CSS's
/// tie-break ("the border from the element earlier in the tree order"); at the
/// table's rim one side is simply absent.
pub(crate) fn collapse(earlier: Option<Border>, later: Option<Border>) -> Option<Border> {
    let winner = match (earlier, later) {
        (None, None) => return None,
        (Some(only), None) | (None, Some(only)) => only,
        (Some(earlier), Some(later)) => {
            if (later.used_width(), later.rank()) > (earlier.used_width(), earlier.rank()) {
                later
            } else {
                earlier
            }
        }
    };
    (winner.used_width() > 0).then_some(winner)
}

/// The lines a resolved border draws along a boundary, from `from` to `to` in
/// the fragment's own frame (x in twips, y in milli-twips).
///
/// The endpoints are the boundary itself — the *centre* of the line, which is
/// where `border-collapse` puts it. Everything about how the style reaches the
/// page is decided here rather than by the consumer, for the reason ADR 0016
/// gives for the hollow bullet: a fallback decided in the layout can be
/// tested, one left to whoever draws cannot.
///
/// - `Double` becomes **two** solid lines of a third of the width each, a
///   third apart, which is what CSS's three equal parts come to. Nothing
///   downstream has to divide anything.
/// - `Dashed` and `Dotted` become a dash pattern in twips. Dots are the
///   thickness long with a gap of the same, square rather than round, which at
///   these widths is a difference no print shows. Dashes are four times the
///   thickness on and off — 3pt at the default 0.75pt border, which is the
///   length already measured for the explicit page break's dashed rule.
///
/// `border` is a boundary's *resolved* border — what [`collapse`] returned —
/// so it always draws something. A boundary that draws nothing never reaches
/// here, because `collapse` answered `None` for it.
pub(crate) fn draw(border: Border, from: (i32, i64), to: (i32, i64)) -> Vec<Local> {
    let width = border.used_width();
    let dash = match border.style {
        BorderStyle::Dashed => Some([4 * width, 4 * width]),
        BorderStyle::Dotted => Some([width, width]),
        _ => None,
    };
    // (offset across the boundary, thickness).
    let lines: Vec<(i32, i32)> = if border.style == BorderStyle::Double {
        let thickness = (width / 3).max(1);
        let offset = (width - thickness) / 2;
        vec![(-offset, thickness), (offset, thickness)]
    } else {
        vec![(0, width)]
    };
    let horizontal = from.1 == to.1;
    lines
        .into_iter()
        .map(|(offset, thickness)| {
            let (dx, dy) = if horizontal {
                (0, i64::from(offset) * MILLI)
            } else {
                (offset, 0)
            };
            Local::Edge {
                x1: from.0 + dx,
                y1: from.1 + dy,
                x2: to.0 + dx,
                y2: to.1 + dy,
                thickness,
                color: border.color,
                dash,
            }
        })
        .collect()
}
