//! Table geometry and cell styling, read out of WordprocessingML.
//!
//! This module answers the two questions `convert.rs` cannot answer while it
//! is walking blocks: **which grid position is this `w:tc` at**, and **what
//! does its `w:tcPr` say**.
//!
//! # Why the grid has to be planned before the cells are converted
//!
//! WordprocessingML writes a row as a list of `w:tc` elements, and a cell
//! carrying `w:gridSpan="2"` occupies *two* grid columns while being one
//! element. A reader that pushes one model cell per `w:tc` therefore puts
//! every cell after a span one column too far left — the cell that belongs in
//! column 3 lands in column 2. That is not a merge being dropped, it is data
//! landing in the wrong cell, which is why the covered positions are
//! *materialised* here rather than counted and warned about.
//!
//! ADR 0013 fixes the mapping this module implements:
//!
//! * `w:tblGrid`/`w:gridCol` ↔ [`TableColumn::width`], both in twips, exact;
//! * `w:gridSpan` ↔ [`CellSpan::columns`];
//! * `w:vMerge` restart/continue ↔ [`CellSpan::rows`] on the restart cell —
//!   the continuation cells *are* the model's covered cells, and they already
//!   exist in the file, so nothing is invented;
//! * `w:tcPr` shading, borders, vertical alignment and margins ↔
//!   [`TableCellProperties`];
//! * `w:tblCellMar` ↔ the same padding fields on every cell that does not
//!   state its own — WordprocessingML resolves the table-level default onto
//!   the cell, and the model has only the per-cell field, so the resolution
//!   has to happen here or the default is lost. LibreOffice writes a
//!   `w:tblCellMar` on every table it produces and states `w:tcMar` on only
//!   some cells, so this is the common case rather than an exotic one;
//! * `w:tblBorders` ↔ the same four border fields, by exactly the same rule.
//!   The model has no table-level border, so the table's grid is *resolved*
//!   onto the cells here: a cell in the first row takes the table's `w:top`
//!   and one below it takes `w:insideH`, and a `w:tcBorders` edge overrides
//!   whichever it inherited. Word's own table styles state a table's borders
//!   nowhere else — `TableGrid` is a `w:tblBorders` in `styles.xml` and
//!   nothing more — so a reader that skips this imports every styled Word
//!   table with no borders at all.
//!
//! # Where a table's borders come from
//!
//! Three layers, innermost last, which is the order WordprocessingML
//! resolves them in:
//!
//! 1. the `w:tblStyle` the table names, and the `w:basedOn` chain above it;
//! 2. the table's own `w:tblPr`;
//! 3. the cell's own `w:tcPr`.
//!
//! What is *not* resolved is a table style's conditional formatting
//! (`w:tblStylePr`: banded rows, header row, first column). It is named
//! rather than dropped in silence — `docx-dropped-table-style-banding`.
//!
//! An edge nothing states is left unset, which is the model's *inherit* and
//! WordprocessingML's *no border*. That agreement is what makes the export
//! the reader's inverse: a table nobody stated a border on writes no
//! `w:tblBorders` and no `w:tcBorders`, and reads back as the table it was.

use crate::docx::props::{hex_color, toggle_value};
use crate::docx::util::collect_wrapped;
use crate::docx::warnings::{
    APPROXIMATED_CELL_BORDER, CLAMPED_COLUMN_WIDTH, DROPPED_CELL_PROPERTY, DROPPED_TABLE_BORDER,
    DROPPED_TABLE_STYLE_BANDING, TABLE_MERGE_REPAIRED,
};
use crate::xml::XmlElement;
use opendoc_core::{
    BorderStyle, CellBorder, CellSpan, Color, Length, TableCellProperties, TableCellProperty,
    VerticalAlignment,
};

/// `w:tblBorders`: the four outer edges of a table plus the two interior
/// ones, each `None` when nothing stated it.
///
/// A uniform six-edge grid maps directly to OpenDoc's table-wide border.
/// Non-uniform Word grids still resolve onto cell edges, because that is the
/// only lossless representation for different outside and inside rules.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct TableBorders {
    pub(super) top: Option<CellBorder>,
    pub(super) bottom: Option<CellBorder>,
    pub(super) start: Option<CellBorder>,
    pub(super) end: Option<CellBorder>,
    pub(super) inside_horizontal: Option<CellBorder>,
    pub(super) inside_vertical: Option<CellBorder>,
}

impl TableBorders {
    /// Lays `other` over `self` edge by edge: a `w:tblBorders` closer to the
    /// table wins on the edges it states and leaves the rest standing, which
    /// is how WordprocessingML resolves a style against direct formatting.
    pub(super) fn overlay(&mut self, other: &TableBorders) {
        for (slot, value) in [
            (&mut self.top, other.top),
            (&mut self.bottom, other.bottom),
            (&mut self.start, other.start),
            (&mut self.end, other.end),
            (&mut self.inside_horizontal, other.inside_horizontal),
            (&mut self.inside_vertical, other.inside_vertical),
        ] {
            if value.is_some() {
                *slot = value;
            }
        }
    }

    fn is_empty(&self) -> bool {
        *self == TableBorders::default()
    }

    fn uniform(&self) -> Option<CellBorder> {
        let border = self.top?;
        (self.bottom == Some(border)
            && self.start == Some(border)
            && self.end == Some(border)
            && self.inside_horizontal == Some(border)
            && self.inside_vertical == Some(border))
        .then_some(border)
    }
}

/// Everything a table inherits before its own `w:tblPr` is read: what its
/// `w:tblStyle` says, and whether that style also carries conditional
/// formatting this reader does not apply.
#[derive(Clone, Debug, Default)]
pub(super) struct TableDefaults {
    pub(super) borders: TableBorders,
    pub(super) margins: TableCellProperties,
    /// What reading the *style* cost: an artistic border approximated, a
    /// diagonal dropped. Counted once per table that uses the style rather
    /// than once per style definition, which is the unit the reader reports.
    pub(super) dropped: Vec<&'static str>,
    /// The style chain carries `w:tblStylePr` — banded rows, a header row, a
    /// first column — none of which is resolved here.
    pub(super) conditional_formatting: bool,
}

/// Reads a `w:tblBorders` (a table's own, or a table style's) onto `out`.
pub(super) fn parse_table_borders(
    borders: &XmlElement,
    out: &mut TableBorders,
    dropped: &mut Vec<&'static str>,
) {
    for edge in borders.elements() {
        let slot = match edge.local.as_str() {
            // The 2010 revision names the logical edges `start`/`end` and the
            // original the physical `left`/`right`; both spell the model's
            // start/end for a LTR document, exactly as `w:tcBorders` does.
            "top" => &mut out.top,
            "bottom" => &mut out.bottom,
            "left" | "start" => &mut out.start,
            "right" | "end" => &mut out.end,
            "insideH" => &mut out.inside_horizontal,
            "insideV" => &mut out.inside_vertical,
            // `w:tl2br` and `w:tr2bl` draw a line across a cell, which no
            // per-cell edge can hold.
            _ => {
                dropped.push(DROPPED_TABLE_BORDER);
                continue;
            }
        };
        if let Some(border) = parse_cell_border(edge, dropped) {
            *slot = Some(border);
        }
    }
}

/// What a `w:vMerge` says about the cell carrying it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum VerticalMerge {
    /// No `w:vMerge` at all.
    None,
    /// `w:vMerge w:val="restart"` — this cell begins a vertical merge.
    Restart,
    /// `w:vMerge` with no value, or `w:val="continue"` — this cell is covered
    /// by the merge that began above it. OOXML makes the valueless form mean
    /// *continue*, which is the opposite of the toggle-property default, so it
    /// is spelled out rather than left to [`toggle_value`].
    Continue,
}

/// One `w:tc`, with everything read out of its `w:tcPr` and its position in
/// the grid resolved.
pub(super) struct PlannedCell<'a> {
    pub(super) element: &'a XmlElement,
    /// The grid column this cell starts at, counting the columns earlier
    /// cells in the same row consumed through their `w:gridSpan`.
    pub(super) column: usize,
    /// `w:gridSpan`, at least 1.
    pub(super) grid_span: usize,
    pub(super) vertical_merge: VerticalMerge,
    pub(super) properties: TableCellProperties,
    /// How far down the merge started here reaches, filled in by
    /// [`TablePlan::resolve_vertical_merges`]. 1 means "this row only".
    pub(super) row_span: usize,
    /// True when a `w:vMerge` continuation could not be attached to a restart
    /// above it, so the cell is read as an ordinary cell instead.
    pub(super) orphaned_continuation: bool,
}

impl PlannedCell<'_> {
    pub(super) fn span(&self) -> CellSpan {
        CellSpan::new(self.row_span as u32, self.grid_span as u32).unwrap_or(CellSpan::SINGLE)
    }

    /// Whether this cell is covered by a merge that began in an earlier row,
    /// and therefore holds content WordprocessingML already treats as hidden.
    fn is_covered_continuation(&self) -> bool {
        self.vertical_merge == VerticalMerge::Continue && !self.orphaned_continuation
    }
}

/// A whole `w:tbl` with its grid resolved: column widths, and every row's
/// cells with their starting column and span.
pub(super) struct TablePlan<'a> {
    /// One entry per grid column. `None` is the model's *auto*.
    pub(super) column_widths: Vec<Option<Length>>,
    pub(super) rows: Vec<Vec<PlannedCell<'a>>>,
    /// `w:trHeight` is an authored minimum unless Word marks it exact. OpenDoc
    /// has the same grow-to-content rule, so its value maps directly here.
    pub(super) row_heights: Vec<Option<Length>>,
    /// Word's `w:tblHeader` flag: a semantic header row.
    pub(super) row_headers: Vec<bool>,
    pub(super) border: Option<CellBorder>,
    pub(super) alignment: Option<opendoc_core::TableAlignment>,
    /// Warning codes to count, one entry per occurrence.
    pub(super) dropped: Vec<&'static str>,
}

impl<'a> TablePlan<'a> {
    /// The number of grid columns the table has: the widest row, or the
    /// declared `w:tblGrid`, whichever reaches further. Never zero — a table
    /// block with no columns is not a table the model can hold.
    pub(super) fn column_count(&self) -> usize {
        let widest = self
            .rows
            .iter()
            .map(|row| {
                row.last()
                    .map(|cell| cell.column + cell.grid_span)
                    .unwrap_or(0)
            })
            .max()
            .unwrap_or(0);
        widest.max(self.column_widths.len()).max(1)
    }

    fn resolve_vertical_merges(&mut self) {
        for row_index in 0..self.rows.len() {
            for cell_index in 0..self.rows[row_index].len() {
                match self.rows[row_index][cell_index].vertical_merge {
                    VerticalMerge::Restart => {
                        let column = self.rows[row_index][cell_index].column;
                        let grid_span = self.rows[row_index][cell_index].grid_span;
                        let span = self.count_continuations(row_index, column, grid_span);
                        self.rows[row_index][cell_index].row_span = span;
                    }
                    VerticalMerge::Continue => {}
                    VerticalMerge::None => {}
                }
            }
        }
        self.mark_orphaned_continuations();
    }

    /// How many rows below `row_index` continue the merge that starts there.
    ///
    /// A continuation only counts when it sits at the **same grid column and
    /// has the same `w:gridSpan`**: a rectangle is the only thing
    /// [`CellSpan`] can express, and stopping the merge early is a smaller
    /// lie than claiming a rectangle the file does not describe.
    fn count_continuations(&self, row_index: usize, column: usize, grid_span: usize) -> usize {
        let mut rows = 1;
        for row in self.rows.iter().skip(row_index + 1) {
            let Some(cell) = row
                .iter()
                .find(|cell| cell.column == column && cell.grid_span == grid_span)
            else {
                break;
            };
            if cell.vertical_merge != VerticalMerge::Continue {
                break;
            }
            rows += 1;
        }
        rows
    }

    /// A `w:vMerge="continue"` that no restart above it reaches is not a
    /// covered cell — it is an ordinary cell in a file that lost its restart.
    /// Reading it as covered would hide its content, so it is read as a plain
    /// cell and the repair is reported.
    fn mark_orphaned_continuations(&mut self) {
        let mut covered: Vec<(usize, usize)> = Vec::new();
        for (row_index, row) in self.rows.iter().enumerate() {
            for cell in row {
                if cell.row_span > 1 {
                    for covered_row in row_index + 1..row_index + cell.row_span {
                        covered.push((covered_row, cell.column));
                    }
                }
            }
        }
        for (row_index, row) in self.rows.iter_mut().enumerate() {
            for cell in row.iter_mut() {
                if cell.vertical_merge == VerticalMerge::Continue
                    && !covered.contains(&(row_index, cell.column))
                {
                    cell.orphaned_continuation = true;
                    self.dropped.push(TABLE_MERGE_REPAIRED);
                }
            }
        }
    }

    /// Writes the table's own border grid onto every cell that did not state
    /// that edge itself.
    ///
    /// Which of the six `w:tblBorders` edges a cell inherits is a fact about
    /// where the cell sits: the top of a cell in the first row is the
    /// *table's* top, and the top of any cell below it is the table's
    /// `w:insideH`. A merged cell is a rectangle, so what decides its bottom
    /// and its end is where the rectangle ends, not where it starts — which
    /// is why this runs after [`TablePlan::resolve_vertical_merges`] has
    /// filled in `row_span`.
    fn resolve_table_borders(&mut self, borders: &TableBorders) {
        if borders.is_empty() {
            return;
        }
        let row_count = self.rows.len();
        let column_count = self.column_count();
        for (row_index, row) in self.rows.iter_mut().enumerate() {
            for cell in row.iter_mut() {
                let first_row = row_index == 0;
                let last_row = row_index + cell.row_span >= row_count;
                let first_column = cell.column == 0;
                let last_column = cell.column + cell.grid_span >= column_count;
                let inherited = [
                    (
                        TableCellProperty::BorderTop as fn(CellBorder) -> TableCellProperty,
                        if first_row {
                            borders.top
                        } else {
                            borders.inside_horizontal
                        },
                        cell.properties.border_top.is_none(),
                    ),
                    (
                        TableCellProperty::BorderBottom,
                        if last_row {
                            borders.bottom
                        } else {
                            borders.inside_horizontal
                        },
                        cell.properties.border_bottom.is_none(),
                    ),
                    (
                        TableCellProperty::BorderStart,
                        if first_column {
                            borders.start
                        } else {
                            borders.inside_vertical
                        },
                        cell.properties.border_start.is_none(),
                    ),
                    (
                        TableCellProperty::BorderEnd,
                        if last_column {
                            borders.end
                        } else {
                            borders.inside_vertical
                        },
                        cell.properties.border_end.is_none(),
                    ),
                ];
                for (property, border, unstated) in inherited {
                    if let (true, Some(border)) = (unstated, border) {
                        cell.properties.set(property(border));
                    }
                }
            }
        }
    }

    /// Whether the cell at `(row_index, cell_index)` holds content that the
    /// model retains but no reader draws, because a merge above covers it.
    pub(super) fn is_covered(&self, row_index: usize, cell_index: usize) -> bool {
        self.rows[row_index][cell_index].is_covered_continuation()
    }
}

/// Reads a `w:tbl` element's grid without descending into cell content.
///
/// `inherited` is what the table's `w:tblStyle` said; the table's own
/// `w:tblPr` is laid over it here, and each cell's `w:tcPr` over that.
pub(super) fn plan_table<'a>(table: &'a XmlElement, inherited: &TableDefaults) -> TablePlan<'a> {
    let mut dropped = inherited.dropped.clone();
    if inherited.conditional_formatting {
        dropped.push(DROPPED_TABLE_STYLE_BANDING);
    }
    // `w:tblCellMar` states the padding of every cell that does not state its
    // own, so it is read once and used as each cell's starting point;
    // `w:tblBorders` says the same thing about borders, but which edge a cell
    // inherits depends on where it sits, so it waits until the grid is known.
    let mut default_margins = inherited.margins.clone();
    let mut default_borders = inherited.borders;
    let mut alignment = None;
    if let Some(tbl_pr) = table.child("tblPr") {
        if let Some(margins) = tbl_pr.child("tblCellMar") {
            parse_cell_margins(margins, &mut default_margins, &mut dropped);
        }
        if let Some(borders) = tbl_pr.child("tblBorders") {
            let mut direct = TableBorders::default();
            parse_table_borders(borders, &mut direct, &mut dropped);
            default_borders.overlay(&direct);
        }
        alignment = tbl_pr
            .child("jc")
            .and_then(|value| value.attr("val"))
            .and_then(|value| opendoc_core::TableAlignment::parse(value.trim()).ok());
    }
    let mut rows = Vec::new();
    let mut row_heights = Vec::new();
    let mut row_headers = Vec::new();
    let mut row_elements = Vec::new();
    collect_wrapped(table, "tr", &mut row_elements);
    for row in row_elements {
        let height = row
            .child("trPr")
            .and_then(|properties| properties.child("trHeight"))
            .and_then(|height| height.attr("val"))
            .and_then(|value| value.trim().parse::<i32>().ok())
            .and_then(|twips| (twips > 0).then_some(twips))
            .and_then(|twips| Length::from_twips(twips).ok());
        row_heights.push(height);
        let header = row
            .child("trPr")
            .and_then(|properties| properties.child("tblHeader"))
            .is_some_and(|header| header.attr("val").map(str::trim) != Some("0"));
        row_headers.push(header);
        let mut cell_elements = Vec::new();
        collect_wrapped(row, "tc", &mut cell_elements);
        let mut column = 0usize;
        let mut cells = Vec::new();
        for element in cell_elements {
            let tc_pr = element.child("tcPr");
            let grid_span = tc_pr
                .and_then(|tc_pr| tc_pr.child("gridSpan"))
                .and_then(|span| span.attr("val"))
                .and_then(|value| value.trim().parse::<usize>().ok())
                .unwrap_or(1)
                .clamp(1, CellSpan::MAX as usize);
            let vertical_merge = tc_pr
                .and_then(|tc_pr| tc_pr.child("vMerge"))
                .map(|merge| match merge.attr("val").map(str::trim) {
                    Some("restart") => VerticalMerge::Restart,
                    // `w:val="continue"`, and the valueless form OOXML also
                    // defines as continue.
                    _ => VerticalMerge::Continue,
                })
                .unwrap_or(VerticalMerge::None);
            let properties = match tc_pr {
                Some(tc_pr) => parse_cell_properties(tc_pr, default_margins.clone(), &mut dropped),
                None => default_margins.clone(),
            };
            cells.push(PlannedCell {
                element,
                column,
                grid_span,
                vertical_merge,
                properties,
                row_span: 1,
                orphaned_continuation: false,
            });
            column += grid_span;
        }
        rows.push(cells);
    }
    let column_widths = plan_column_widths(table, &rows, &mut dropped);
    let mut plan = TablePlan {
        column_widths,
        rows,
        row_heights,
        row_headers,
        border: default_borders.uniform(),
        alignment,
        dropped,
    };
    plan.resolve_vertical_merges();
    if plan.border.is_none() {
        plan.resolve_table_borders(&default_borders);
    }
    plan
}

/// `w:tblGrid` is the authored column list, and its widths are already twips.
///
/// The one case where they are not authored is a table that declares itself
/// *autofit*: then `w:gridCol` carries the producer's cached layout rather
/// than a width anybody chose, and the model's *auto* (`None`) says exactly
/// that. Nothing is lost, so nothing warns. A table with no `w:tblGrid` at
/// all falls back to the first row's `w:tcW`, which is where Word puts the
/// same numbers.
fn plan_column_widths(
    table: &XmlElement,
    rows: &[Vec<PlannedCell<'_>>],
    dropped: &mut Vec<&'static str>,
) -> Vec<Option<Length>> {
    if table_is_autofit(table) {
        return Vec::new();
    }
    if let Some(grid) = table.child("tblGrid") {
        let widths: Vec<Option<Length>> = grid
            .children_named("gridCol")
            .map(|column| column_width(column.attr("w"), dropped))
            .collect();
        if !widths.is_empty() {
            return widths;
        }
    }
    // No `w:tblGrid`: the first row's cell widths describe the same grid, one
    // entry per grid column the cell spans.
    let Some(first_row) = rows.first() else {
        return Vec::new();
    };
    let mut widths = Vec::new();
    for cell in first_row {
        let declared = cell
            .element
            .child("tcPr")
            .and_then(|tc_pr| tc_pr.child("tcW"))
            .filter(|width| !matches!(width.attr("type").map(str::trim), Some("pct" | "auto")))
            .and_then(|width| width.attr("w"));
        // A `w:tcW` measures the whole span, so a spanning cell says nothing
        // about any single column underneath it.
        if cell.grid_span > 1 {
            widths.extend((0..cell.grid_span).map(|_| None));
        } else {
            widths.push(column_width(declared, dropped));
        }
    }
    if widths.iter().all(Option::is_none) {
        return Vec::new();
    }
    widths
}

/// Whether the table says its `w:gridCol` values are a cached layout rather
/// than authored widths. Word's own autofit tables leave `w:tblLayout` out
/// entirely and still mean their widths, so only an explicit declaration
/// counts.
fn table_is_autofit(table: &XmlElement) -> bool {
    table
        .child("tblPr")
        .and_then(|tbl_pr| tbl_pr.child("tblLayout"))
        .and_then(|layout| layout.attr("type"))
        .map(|value| value.trim().eq_ignore_ascii_case("autofit"))
        .unwrap_or(false)
}

/// One `w:w` attribute as a column width, clamped into what the model holds.
fn column_width(value: Option<&str>, dropped: &mut Vec<&'static str>) -> Option<Length> {
    let twips = value?.trim().parse::<i32>().ok()?;
    if twips <= 0 {
        // Word writes 0 for "no preference", which is the model's auto.
        return None;
    }
    let clamped = twips.clamp(
        opendoc_core::TableColumn::MIN_WIDTH_TWIPS,
        Length::MAX_TWIPS,
    );
    if clamped != twips {
        dropped.push(CLAMPED_COLUMN_WIDTH);
    }
    Length::from_twips(clamped).ok()
}

// ---------------------------------------------------------------------------
// w:tcPr
// ---------------------------------------------------------------------------

/// Everything a `w:tcPr` says that [`TableCellProperties`] can hold, with
/// everything else counted.
///
/// The four `w:tcPr` children that carry the cell's geometry rather than its
/// style — `w:gridSpan`, `w:vMerge`, `w:hMerge` and `w:tcW` — are read
/// elsewhere (or derivable from the grid) and are not styling, so they are not
/// counted as dropped here.
fn parse_cell_properties(
    tc_pr: &XmlElement,
    inherited: TableCellProperties,
    dropped: &mut Vec<&'static str>,
) -> TableCellProperties {
    // The table's `w:tblCellMar` is already in `inherited`; a `w:tcMar` edge
    // below overrides its own edge and leaves the others standing, which is
    // how WordprocessingML resolves the pair.
    let mut properties = inherited;
    for child in tc_pr.elements() {
        match child.local.as_str() {
            "gridSpan" | "vMerge" | "vMergeOrig" | "tcW" | "cnfStyle" | "hideMark" | "headers" => {}
            "hMerge" => {
                // A `w:hMerge` is the legacy spelling of `w:gridSpan`, and a
                // reader that ignored it would misalign the row exactly the
                // way an ignored `w:gridSpan` does.
                dropped.push(DROPPED_CELL_PROPERTY);
            }
            "shd" => {
                if let Some(background) = shading_color(child, dropped) {
                    properties.set(TableCellProperty::Background(background));
                }
            }
            "tcBorders" => parse_cell_borders(child, &mut properties, dropped),
            "vAlign" => match child.attr("val").map(str::trim) {
                Some("top") => {
                    properties.set(TableCellProperty::VerticalAlignment(VerticalAlignment::Top));
                }
                Some("center") => {
                    properties.set(TableCellProperty::VerticalAlignment(
                        VerticalAlignment::Middle,
                    ));
                }
                Some("bottom") => {
                    properties.set(TableCellProperty::VerticalAlignment(
                        VerticalAlignment::Bottom,
                    ));
                }
                // `w:val="both"` stretches the content, which the model has no
                // word for.
                _ => dropped.push(DROPPED_CELL_PROPERTY),
            },
            "tcMar" => parse_cell_margins(child, &mut properties, dropped),
            "noWrap" => {
                if toggle_value(child) {
                    dropped.push(DROPPED_CELL_PROPERTY);
                }
            }
            _ => dropped.push(DROPPED_CELL_PROPERTY),
        }
    }
    properties
}

/// `w:shd` is a pattern over two colours; the model holds one flat colour, so
/// only the `clear` pattern maps exactly.
fn shading_color(shd: &XmlElement, dropped: &mut Vec<&'static str>) -> Option<Color> {
    let pattern = shd.attr("val").map(str::trim).unwrap_or("clear");
    if !matches!(pattern, "clear" | "nil" | "solid") {
        dropped.push(DROPPED_CELL_PROPERTY);
    }
    // `solid` fills with the *foreground* colour; every other pattern that
    // reaches here is approximated by its fill.
    let source = if pattern == "solid" {
        shd.attr("color").or_else(|| shd.attr("fill"))
    } else {
        shd.attr("fill")
    };
    let value = source?.trim();
    if value.eq_ignore_ascii_case("auto") {
        return None;
    }
    let hex = hex_color(value)?;
    Color::parse(&hex).ok()
}

fn parse_cell_borders(
    borders: &XmlElement,
    properties: &mut TableCellProperties,
    dropped: &mut Vec<&'static str>,
) {
    for edge in borders.elements() {
        let property = match edge.local.as_str() {
            // WordprocessingML names the logical edges `start`/`end` in the
            // 2010 revision and the physical ones `left`/`right` in the
            // original; both spell the model's start/end for a LTR document,
            // which is the same reading `docx_write` writes back.
            "top" => TableCellProperty::BorderTop,
            "bottom" => TableCellProperty::BorderBottom,
            "left" | "start" => TableCellProperty::BorderStart,
            "right" | "end" => TableCellProperty::BorderEnd,
            // Diagonals and the table-level inside edges have no per-cell
            // model equivalent.
            _ => {
                dropped.push(DROPPED_CELL_PROPERTY);
                continue;
            }
        };
        if let Some(border) = parse_cell_border(edge, dropped) {
            properties.set(property(border));
        }
    }
}

fn parse_cell_border(edge: &XmlElement, dropped: &mut Vec<&'static str>) -> Option<CellBorder> {
    let value = edge.attr("val").map(str::trim).unwrap_or("single");
    let style = match value {
        "nil" | "none" => BorderStyle::None,
        "single" | "thick" => BorderStyle::Solid,
        "double" | "triple" | "doubleWave" => BorderStyle::Double,
        "dotted" | "dotDash" | "dotDotDash" => BorderStyle::Dotted,
        "dashed" | "dashSmallGap" | "dashDotStroked" => BorderStyle::Dashed,
        _ => {
            // Every other `w:val` is an artistic border Word draws from a
            // bitmap; it becomes a plain line rather than vanishing.
            dropped.push(APPROXIMATED_CELL_BORDER);
            BorderStyle::Solid
        }
    };
    if style == BorderStyle::None {
        return Some(CellBorder::none());
    }
    // `w:sz` counts eighths of a point and the model counts twips, so one
    // eighth is exactly 2.5 twips. The halving is the only inexact step in
    // the whole table mapping, and it is reported when it bites.
    let eighths = edge
        .attr("sz")
        .and_then(|value| value.trim().parse::<i32>().ok())
        .unwrap_or(4)
        .max(0);
    let twips = (eighths * 20) / 8;
    if (eighths * 20) % 8 != 0 {
        dropped.push(APPROXIMATED_CELL_BORDER);
    }
    let clamped = twips.clamp(0, CellBorder::MAX_WIDTH_TWIPS);
    if clamped != twips {
        dropped.push(APPROXIMATED_CELL_BORDER);
    }
    let color = edge
        .attr("color")
        .map(str::trim)
        .filter(|value| !value.eq_ignore_ascii_case("auto"))
        .and_then(hex_color)
        .and_then(|hex| Color::parse(&hex).ok())
        .unwrap_or(Color::BLACK);
    CellBorder::new(style, Length::from_twips(clamped).ok()?, color).ok()
}

pub(super) fn parse_cell_margins(
    margins: &XmlElement,
    properties: &mut TableCellProperties,
    dropped: &mut Vec<&'static str>,
) {
    for edge in margins.elements() {
        let property = match edge.local.as_str() {
            "top" => TableCellProperty::PaddingTop,
            "bottom" => TableCellProperty::PaddingBottom,
            "left" | "start" => TableCellProperty::PaddingStart,
            "right" | "end" => TableCellProperty::PaddingEnd,
            _ => {
                dropped.push(DROPPED_CELL_PROPERTY);
                continue;
            }
        };
        // `w:type="pct"` measures in fiftieths of a percent of the table
        // width, which is not a length until the table is laid out.
        if matches!(edge.attr("type").map(str::trim), Some("pct")) {
            dropped.push(DROPPED_CELL_PROPERTY);
            continue;
        }
        let Some(twips) = edge
            .attr("w")
            .and_then(|value| value.trim().parse::<i32>().ok())
        else {
            continue;
        };
        if twips < 0 {
            dropped.push(DROPPED_CELL_PROPERTY);
            continue;
        }
        let clamped = twips.min(Length::MAX_TWIPS);
        if clamped != twips {
            dropped.push(DROPPED_CELL_PROPERTY);
        }
        if let Ok(length) = Length::from_twips(clamped) {
            properties.set(property(length));
        }
    }
}
