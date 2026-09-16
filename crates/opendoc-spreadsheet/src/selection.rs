//! Spreadsheet selection and clipboard projections.

use serde::{Deserialize, Serialize};

use crate::{FormulaValue, SpreadsheetError};

use std::collections::BTreeSet;

use super::address::{
    cell_address, cell_axis_labels, column_to_number, normalize_cell_address, number_to_column,
    parse_cell_position, parse_cell_range, trim_number,
};
use super::structure::transform_formula_for_paste;
use super::workbook::SpreadsheetWorkbook;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SpreadsheetSelectionSummary {
    pub anchor: String,
    pub focus: String,
    pub range: String,
    pub from_address: String,
    pub to_address: String,
    pub from_col: u32,
    pub from_row: u32,
    pub to_col: u32,
    pub to_row: u32,
    pub numeric_count: usize,
    pub numeric_sum: String,
    pub numeric_average: Option<String>,
    pub summary_label: String,
    pub selected_tsv: String,
    pub selected_addresses: Vec<String>,
}

impl SpreadsheetSelectionSummary {
    pub fn addresses(&self) -> Result<Vec<String>, SpreadsheetError> {
        selected_addresses(self.from_col, self.from_row, self.to_col, self.to_row)
    }

    /// The labels of the rows the selection covers, top to bottom.
    ///
    /// Whole rows, not cells: a command that acts on a row — hiding it,
    /// sizing it — needs the axis the selection spans, and deriving that from
    /// [`SpreadsheetSelectionSummary::addresses`] would mean splitting every
    /// address back apart.
    pub fn row_labels(&self) -> Vec<String> {
        (self.from_row..=self.to_row)
            .map(|row| row.to_string())
            .collect()
    }

    /// The labels of the columns the selection covers, left to right.
    pub fn column_labels(&self) -> Result<Vec<String>, SpreadsheetError> {
        (self.from_col..=self.to_col)
            .map(|column| {
                number_to_column(column).ok_or_else(|| {
                    SpreadsheetError::Format(format!("column {column} is out of range"))
                })
            })
            .collect()
    }
}

impl SpreadsheetWorkbook {
    pub fn describe_selection(
        &self,
        sheet_id: impl AsRef<str>,
        anchor: impl AsRef<str>,
        focus: impl AsRef<str>,
    ) -> Result<SpreadsheetSelectionSummary, SpreadsheetError> {
        let sheet_id = sheet_id.as_ref();
        let sheet = self
            .sheets
            .iter()
            .find(|sheet| sheet.id == sheet_id)
            .ok_or_else(|| SpreadsheetError::NotFound(format!("sheet {sheet_id} was not found")))?;
        let anchor = normalize_cell_address(anchor.as_ref())?;
        let focus = normalize_cell_address(focus.as_ref())?;
        let (anchor_col, anchor_row) = parse_cell_position(&anchor)?;
        let (focus_col, focus_row) = parse_cell_position(&focus)?;
        let from_col = anchor_col.min(focus_col);
        let from_row = anchor_row.min(focus_row);
        let to_col = anchor_col.max(focus_col);
        let to_row = anchor_row.max(focus_row);
        let from_address = cell_address(from_col, from_row)?;
        let to_address = cell_address(to_col, to_row)?;
        let range = if from_address == to_address {
            from_address.clone()
        } else {
            format!("{from_address}:{to_address}")
        };
        let mut sum = 0.0;
        let mut numeric_count = 0;
        let axes = DrawnGrid::of(sheet);
        for cell in &sheet.cells {
            let Ok((col, row)) = parse_cell_position(&cell.address) else {
                continue;
            };
            if col < from_col || col > to_col || row < from_row || row > to_row {
                continue;
            }
            // A merged block holds its content on its anchor and is drawn
            // once; counting the cells it covers counted the same block
            // several times over.
            if axes.cover_anchor(col, row).is_some() {
                continue;
            }
            if let FormulaValue::Number(value) =
                FormulaValue::from_projection(&cell.computed_kind, &cell.computed_value)
            {
                sum += value;
                numeric_count += 1;
            }
        }
        let numeric_sum = trim_number(sum);
        let numeric_average =
            (numeric_count > 0).then(|| format!("{:.2}", sum / numeric_count as f64));
        let summary_label = if let Some(average) = &numeric_average {
            format!("Sum {numeric_sum} · Count {numeric_count} · Avg {average}")
        } else {
            range.clone()
        };
        let selected_tsv = selection_tsv(sheet, from_col, from_row, to_col, to_row)?;
        let selected_addresses = selected_addresses(from_col, from_row, to_col, to_row)?;
        Ok(SpreadsheetSelectionSummary {
            anchor,
            focus,
            range,
            from_address,
            to_address,
            from_col,
            from_row,
            to_col,
            to_row,
            numeric_count,
            numeric_sum,
            numeric_average,
            summary_label,
            selected_tsv,
            selected_addresses,
        })
    }

    /// Fold one selection gesture into the next selection.
    ///
    /// # Hidden rows and columns are not selectable
    ///
    /// `opendoc-render` omits a hidden axis from the grid entirely — no `<tr>`,
    /// no `<col>`, no `<td>` — so a hidden cell has no pixel on screen to draw
    /// a focus ring in or open an editor over. The rule here follows from that,
    /// and it is one rule rather than a patch per action: **an anchor or a
    /// focus this function returns is always a cell the grid draws.** The
    /// *range* between them still covers hidden axes, because hiding is not
    /// deletion and a copy or a clear over a range includes what is hidden —
    /// which is also what Google Sheets does.
    ///
    /// Consequences, each of them Google Sheets' behaviour too:
    ///
    /// * an arrow key steps *over* a hidden run rather than landing in it, and
    ///   when nothing is drawn further along it stays put, exactly as at the
    ///   sheet edge;
    /// * Ctrl+arrow and Home land on the last and first *drawn* axis;
    /// * select-all still covers the whole sheet, hidden axes included, but
    ///   its endpoints are drawn cells.
    ///
    /// ## The merged-cell anchor, which has no obvious answer
    ///
    /// A merged block stores its content and its formatting on the merge's
    /// top-left *anchor* cell. Hide the anchor's row or column and the block is
    /// still drawn — at the first corner of the rectangle that survives — but
    /// the `<td>` the renderer emits for it is still labelled with the
    /// *anchor's* address, deliberately, "the one a click must select and the
    /// one the formula bar must edit".
    ///
    /// **So the anchor of a partly drawn merge stays selectable, even when its
    /// own row or column is hidden.** It is the one hidden address the grid
    /// really does draw an element for, so the focus ring has somewhere to go
    /// and an edit has somewhere to land; and it is the only cell of the merge
    /// that can be edited at all, because the others hold no content.
    ///
    /// The alternatives were weighed and rejected:
    ///
    /// * snapping forward to the corner the block was drawn at picks a cell
    ///   the renderer *covers* rather than emits, so the selection would point
    ///   at nothing in the markup and the formula bar would edit an empty cell
    ///   next to the merge's content;
    /// * snapping backward leaves the merge altogether, which is not what was
    ///   clicked;
    /// * refusing the click reads as a broken grid, because the block is
    ///   plainly visible.
    ///
    /// A merge with *no* drawn cell is not an exception: nothing is drawn for
    /// it, so its anchor snaps like any other hidden address.
    ///
    /// When a sheet has no drawn axis at all there is nothing to snap to, and
    /// the requested address is returned unchanged rather than invented.
    ///
    /// ## The cells a merge covers
    ///
    /// The same rule, one step further: **a cell a merged block covers is not
    /// a cell the grid draws either**, so it is never an anchor or a focus.
    /// An arrow key crosses the whole block in one press (`A2 → B2 → D2` over
    /// a `B2:C3` merge, as in Sheets), a click reporting a covered address
    /// selects the block's anchor, and the selection summary counts the
    /// block's content once rather than once per covered cell.
    ///
    /// The write side follows from the same fact and lives with the writer:
    /// [`SpreadsheetWorkbook::set_cell_in_sheet`] refuses a covered address
    /// outright, because a value stored there is drawn nowhere.
    pub fn reduce_selection(
        &self,
        sheet_id: impl AsRef<str>,
        anchor: impl AsRef<str>,
        focus: impl AsRef<str>,
        action: impl AsRef<str>,
        value: impl AsRef<str>,
        extend: bool,
    ) -> Result<SpreadsheetSelectionSummary, SpreadsheetError> {
        let sheet_id = sheet_id.as_ref();
        let sheet = self
            .sheets
            .iter()
            .find(|sheet| sheet.id == sheet_id)
            .ok_or_else(|| SpreadsheetError::NotFound(format!("sheet {sheet_id} was not found")))?;
        let axes = DrawnGrid::of(sheet);
        let anchor = normalize_cell_address(anchor.as_ref()).unwrap_or_else(|_| "A1".to_string());
        let focus = normalize_cell_address(focus.as_ref()).unwrap_or_else(|_| anchor.clone());
        // Both ends of the incoming selection are snapped once, here, so every
        // action below does its arithmetic from drawn positions and only has
        // to snap the position it invents.
        let anchor = axes.snap_address(&anchor, Bias::Forward)?;
        let focus = axes.snap_address(&focus, Bias::Forward)?;
        let (col, row) = parse_cell_position(&focus)?;
        let max_col = axes.max_col;
        let max_row = axes.max_row;
        let action = action.as_ref();
        let value = value.as_ref();
        let (next_anchor, next_focus) = match action {
            "move" => {
                let (dc, dr) = direction_delta(value)?;
                let (next_col, next_row) = axes.step(col, row, dc, dr);
                let next = cell_address(next_col, next_row)?;
                (if extend { anchor } else { next.clone() }, next)
            }
            "move-edge" => {
                let next_col = match value {
                    "left" => axes.column_edge(Bias::Forward),
                    "right" => axes.column_edge(Bias::Backward),
                    _ => col,
                };
                let next_row = match value {
                    "up" => axes.row_edge(Bias::Forward),
                    "down" => axes.row_edge(Bias::Backward),
                    _ => row,
                };
                let next = axes.snap_position(next_col, next_row, Bias::Forward)?;
                (if extend { anchor } else { next.clone() }, next)
            }
            "home" => {
                let next_col = axes.column_edge(Bias::Forward);
                let next_row = if value == "sheet" {
                    axes.row_edge(Bias::Forward)
                } else {
                    row
                };
                let next = axes.snap_position(next_col, next_row, Bias::Forward)?;
                (if extend { anchor } else { next.clone() }, next)
            }
            // The whole sheet, hidden axes included — but named by drawn
            // corners, so the focus ring has somewhere to go.
            "select-all" => (
                cell_address(
                    axes.column_edge(Bias::Forward),
                    axes.row_edge(Bias::Forward),
                )?,
                cell_address(
                    axes.column_edge(Bias::Backward),
                    axes.row_edge(Bias::Backward),
                )?,
            ),
            // A pointer gesture: the address comes off a drawn element, so the
            // only way it names a hidden cell is a merged block whose anchor
            // is hidden. See the doc comment.
            "set-focus" => {
                let next = axes.snap_address(&normalize_cell_address(value)?, Bias::Forward)?;
                (if extend { anchor } else { next.clone() }, next)
            }
            // Fill-handle drag: `anchor`/`focus` are the source block and
            // `value` is the cell the pointer is over. The drag snaps to
            // whichever axis it reached furthest along, so a diagonal gesture
            // still produces a target `fill_range` will accept.
            "fill-target" => {
                let (anchor_col, anchor_row) = parse_cell_position(&anchor)?;
                let from_col = anchor_col.min(col);
                let from_row = anchor_row.min(row);
                let to_col = anchor_col.max(col);
                let to_row = anchor_row.max(row);
                // Also a pointer gesture, and snapped for the same reason.
                let hover = axes.snap_address(&normalize_cell_address(value)?, Bias::Forward)?;
                let (hover_col, hover_row) = parse_cell_position(&hover)?;
                let hover_col = hover_col.clamp(1, max_col);
                let hover_row = hover_row.clamp(1, max_row);
                let reach = |first: u32, last: u32, at: u32| {
                    at.saturating_sub(last).max(first.saturating_sub(at))
                };
                let row_reach = reach(from_row, to_row, hover_row);
                let column_reach = reach(from_col, to_col, hover_col);
                if row_reach >= column_reach {
                    (
                        cell_address(from_col, from_row.min(hover_row))?,
                        cell_address(to_col, to_row.max(hover_row))?,
                    )
                } else {
                    (
                        cell_address(from_col.min(hover_col), from_row)?,
                        cell_address(to_col.max(hover_col), to_row)?,
                    )
                }
            }
            // The edge of the current selection a keyboard fill copies from:
            // the top row for "down", the leftmost column for "right".
            "fill-source" => {
                let (anchor_col, anchor_row) = parse_cell_position(&anchor)?;
                let from_col = anchor_col.min(col);
                let from_row = anchor_row.min(row);
                let to_col = anchor_col.max(col);
                let to_row = anchor_row.max(row);
                match value {
                    "down" => (
                        cell_address(from_col, from_row)?,
                        cell_address(to_col, from_row)?,
                    ),
                    "right" => (
                        cell_address(from_col, from_row)?,
                        cell_address(from_col, to_row)?,
                    ),
                    other => {
                        return Err(SpreadsheetError::Format(format!(
                            "unknown spreadsheet fill source {other}"
                        )))
                    }
                }
            }
            // Explicit addressing from the name box. The range is honoured as
            // typed and may span hidden axes; its two *ends* snap inwards, so
            // naming a hidden cell selects the nearest drawn one instead of a
            // cell with nowhere to put the caret.
            "set-range" => {
                let (start, end) = value.split_once(':').unwrap_or((value, value));
                let start = axes.snap_address(&normalize_cell_address(start)?, Bias::Forward)?;
                let end = axes.snap_address(&normalize_cell_address(end)?, Bias::Backward)?;
                (start, end)
            }
            _ => {
                return Err(SpreadsheetError::Format(format!(
                    "unknown spreadsheet selection action {action}"
                )))
            }
        };
        self.describe_selection(sheet_id, next_anchor, next_focus)
    }

    pub fn selection_tsv(
        &self,
        sheet_id: impl AsRef<str>,
        anchor: impl AsRef<str>,
        focus: impl AsRef<str>,
    ) -> Result<String, SpreadsheetError> {
        let sheet_id = sheet_id.as_ref();
        let sheet = self
            .sheets
            .iter()
            .find(|sheet| sheet.id == sheet_id)
            .ok_or_else(|| SpreadsheetError::NotFound(format!("sheet {sheet_id} was not found")))?;
        let selection = self.describe_selection(sheet_id, anchor, focus)?;
        selection_tsv(
            sheet,
            selection.from_col,
            selection.from_row,
            selection.to_col,
            selection.to_row,
        )
    }

    /// Cell edits for a TSV paste landing at `origin`.
    ///
    /// `source_origin` is the top-left cell the text was copied from, and is
    /// `None` for text that came from outside the workbook. When it is known,
    /// pasted formulas move with the paste: relative references shift by the
    /// paste offset and absolute (`$`) ones stay where they are.
    pub fn tsv_cell_edits(
        origin: impl AsRef<str>,
        text: impl AsRef<str>,
        source_origin: Option<&str>,
    ) -> Result<Vec<(String, String)>, SpreadsheetError> {
        let origin = normalize_cell_address(origin.as_ref())?;
        let (origin_col, origin_row) = parse_cell_position(&origin)?;
        let source_origin = source_origin
            .map(normalize_cell_address)
            .transpose()?
            .filter(|source_origin| *source_origin != origin);
        let normalized_text = text.as_ref().replace("\r\n", "\n").replace('\r', "\n");
        let mut rows: Vec<&str> = normalized_text.split('\n').collect();
        if rows.len() > 1 && rows.last().is_some_and(|row| row.is_empty()) {
            rows.pop();
        }
        let mut cells = Vec::new();
        for (row_offset, line) in rows.into_iter().enumerate() {
            for (col_offset, value) in line.split('\t').enumerate() {
                let address = cell_address(
                    origin_col + col_offset as u32,
                    origin_row + row_offset as u32,
                )?;
                let value = match &source_origin {
                    // Every cell in the block moves by the same offset, so the
                    // block origins describe the whole paste.
                    Some(source_origin) => {
                        transform_formula_for_paste(value, source_origin, &origin)?
                    }
                    None => value.to_string(),
                };
                cells.push((address, value));
            }
        }
        Ok(cells)
    }

    pub fn row_after_focus(&self, focus: impl AsRef<str>) -> Result<String, SpreadsheetError> {
        let (_, row) = parse_cell_position(focus.as_ref())?;
        Ok((row + 1).to_string())
    }

    pub fn column_after_focus(&self, focus: impl AsRef<str>) -> Result<String, SpreadsheetError> {
        let (col, _) = parse_cell_position(focus.as_ref())?;
        number_to_column(col + 1)
            .ok_or_else(|| SpreadsheetError::Format(format!("invalid spreadsheet column {col}")))
    }

    pub fn focus_row_label(&self, focus: impl AsRef<str>) -> Result<String, SpreadsheetError> {
        let (_, row) = cell_axis_labels(focus.as_ref())?;
        Ok(row)
    }

    pub fn focus_column_label(&self, focus: impl AsRef<str>) -> Result<String, SpreadsheetError> {
        let (column, _) = cell_axis_labels(focus.as_ref())?;
        Ok(column)
    }

    pub fn frozen_axes_for_focus(
        &self,
        focus: impl AsRef<str>,
    ) -> Result<(u32, u32), SpreadsheetError> {
        let (col, row) = parse_cell_position(focus.as_ref())?;
        Ok((row.saturating_sub(1), col.saturating_sub(1)))
    }
}

fn selected_addresses(
    from_col: u32,
    from_row: u32,
    to_col: u32,
    to_row: u32,
) -> Result<Vec<String>, SpreadsheetError> {
    let mut addresses = Vec::new();
    for row in from_row..=to_row {
        for col in from_col..=to_col {
            addresses.push(cell_address(col, row)?);
        }
    }
    Ok(addresses)
}

fn selection_tsv(
    sheet: &super::model::Sheet,
    from_col: u32,
    from_row: u32,
    to_col: u32,
    to_row: u32,
) -> Result<String, SpreadsheetError> {
    let mut lines = Vec::new();
    for row in from_row..=to_row {
        let mut values = Vec::new();
        for col in from_col..=to_col {
            let address = cell_address(col, row)?;
            values.push(
                sheet
                    .cells
                    .iter()
                    .find(|cell| cell.address == address)
                    .map(|cell| cell.user_value.clone())
                    .unwrap_or_default(),
            );
        }
        lines.push(values.join("\t"));
    }
    Ok(lines.join("\n"))
}

/// Which way a snap looks for a drawn axis first.
///
/// It is a preference, not a restriction: a snap that finds nothing in its
/// preferred direction looks the other way, because returning a hidden address
/// would be worse than returning a drawn one on the wrong side. Only
/// [`VisibleAxes::step`] is one-directional, and deliberately: an arrow key
/// that reversed direction at the end of a hidden run would move the focus
/// *backwards*.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Bias {
    /// Prefer the next axis along: down, or to the right.
    Forward,
    /// Prefer the previous axis: up, or to the left.
    Backward,
}

impl Bias {
    fn step(self) -> i64 {
        match self {
            Bias::Forward => 1,
            Bias::Backward => -1,
        }
    }

    fn flipped(self) -> Self {
        match self {
            Bias::Forward => Bias::Backward,
            Bias::Backward => Bias::Forward,
        }
    }
}

/// What one sheet's grid actually draws.
///
/// Hidden axes are stored by *label* (`"3"`, `"B"`); selection arithmetic is
/// by 1-based number, so the labels are resolved once here rather than
/// re-parsed per step. The merge anchors are resolved here too, because an
/// anchor whose merge still has a drawn cell is drawn even when its own axis
/// is hidden — see [`SpreadsheetWorkbook::reduce_selection`].
struct DrawnGrid {
    hidden_rows: BTreeSet<u32>,
    hidden_columns: BTreeSet<u32>,
    /// Anchors of merges with at least one drawn cell, as `(column, row)`.
    drawn_merge_anchors: BTreeSet<(u32, u32)>,
    /// Every merge as `(first_column, first_row, last_column, last_row)`,
    /// drawn or not. A merged block behaves as one cell for selection: the
    /// grid emits a `<td>` only for its anchor, so the cells it covers have
    /// no pixel to put a focus ring in and an arrow key steps over the whole
    /// block in one press.
    merges: Vec<(u32, u32, u32, u32)>,
    max_col: u32,
    max_row: u32,
}

impl DrawnGrid {
    fn of(sheet: &super::model::Sheet) -> Self {
        let hidden_rows: BTreeSet<u32> = sheet
            .hidden_rows
            .iter()
            .filter_map(|label| label.parse::<u32>().ok())
            .collect();
        let hidden_columns: BTreeSet<u32> = sheet
            .hidden_columns
            .iter()
            .filter_map(|label| column_to_number(label))
            .collect();
        // The same test the renderer applies: a merge is drawn when at least
        // one of its rows and one of its columns survives hiding.
        let drawn_merge_anchors = sheet
            .merges
            .iter()
            .filter_map(|merge| {
                let range = parse_cell_range(&merge.range).ok()?;
                let columns = range.start_column..range.start_column + range.width;
                let rows = range.start_row..range.start_row + range.height;
                let any_column = columns
                    .clone()
                    .any(|column| !hidden_columns.contains(&column));
                let any_row = rows.clone().any(|row| !hidden_rows.contains(&row));
                (any_column && any_row).then_some((range.start_column, range.start_row))
            })
            .collect();
        let merges = sheet
            .merges
            .iter()
            .filter_map(|merge| {
                let range = parse_cell_range(&merge.range).ok()?;
                Some((
                    range.start_column,
                    range.start_row,
                    range.start_column + range.width - 1,
                    range.start_row + range.height - 1,
                ))
            })
            .collect();
        Self {
            hidden_rows,
            hidden_columns,
            drawn_merge_anchors,
            merges,
            max_col: sheet.columns.len().max(1) as u32,
            max_row: sheet.rows.len().max(1) as u32,
        }
    }

    /// The rectangle `(col, row)` belongs to: its merge, or itself.
    fn block(&self, column: u32, row: u32) -> (u32, u32, u32, u32) {
        self.merges
            .iter()
            .copied()
            .find(|(first_column, first_row, last_column, last_row)| {
                (*first_column..=*last_column).contains(&column)
                    && (*first_row..=*last_row).contains(&row)
            })
            .unwrap_or((column, row, column, row))
    }

    /// The anchor of the merge covering `(column, row)`, when that cell is
    /// covered but is not itself the anchor.
    fn cover_anchor(&self, column: u32, row: u32) -> Option<(u32, u32)> {
        let (first_column, first_row, _, _) = self.block(column, row);
        ((first_column, first_row) != (column, row)).then_some((first_column, first_row))
    }

    /// Whether the grid emits an element carrying this address.
    ///
    /// A cell a merge covers is never one: `opendoc-render` emits a single
    /// `<td>` for the whole block, labelled with the anchor's address, and
    /// nothing at all for the rest.
    fn draws(&self, column: u32, row: u32) -> bool {
        if self.cover_anchor(column, row).is_some() {
            return false;
        }
        (self.column_drawn(column) && self.row_drawn(row))
            || self.drawn_merge_anchors.contains(&(column, row))
    }

    fn row_drawn(&self, row: u32) -> bool {
        !self.hidden_rows.contains(&row)
    }

    fn column_drawn(&self, column: u32) -> bool {
        !self.hidden_columns.contains(&column)
    }

    /// The first drawn axis at or beyond `at`, looking `bias` first and then
    /// the other way. `None` only when the whole axis is hidden.
    fn snap(&self, at: u32, bias: Bias, row_axis: bool) -> Option<u32> {
        let len = if row_axis { self.max_row } else { self.max_col };
        let drawn = |index: u32| {
            if row_axis {
                self.row_drawn(index)
            } else {
                self.column_drawn(index)
            }
        };
        let at = at.clamp(1, len);
        for bias in [bias, bias.flipped()] {
            let mut index = at as i64;
            while (1..=len as i64).contains(&index) {
                if drawn(index as u32) {
                    return Some(index as u32);
                }
                index += bias.step();
            }
        }
        None
    }

    fn snap_column(&self, column: u32, bias: Bias) -> u32 {
        self.snap(column, bias, false).unwrap_or(column)
    }

    fn snap_row(&self, row: u32, bias: Bias) -> u32 {
        self.snap(row, bias, true).unwrap_or(row)
    }

    /// `address` moved to the nearest drawn cell, each axis independently —
    /// unless the grid already draws an element carrying it, which for a
    /// hidden address means it is the anchor of a partly drawn merge.
    fn snap_address(&self, address: &str, bias: Bias) -> Result<String, SpreadsheetError> {
        let (column, row) = parse_cell_position(address)?;
        self.snap_position(column, row, bias)
    }

    /// `(column, row)` moved to a cell the grid draws.
    ///
    /// A cell a merge covers resolves to the merge's anchor *first* — the one
    /// cell of the block that is drawn, that holds its content, and that the
    /// formula bar edits — and only then to the nearest drawn axis, so a
    /// wholly hidden merge is not an exception.
    fn snap_position(&self, column: u32, row: u32, bias: Bias) -> Result<String, SpreadsheetError> {
        let (column, row) = self.cover_anchor(column, row).unwrap_or((column, row));
        if self.draws(column, row) {
            return cell_address(column, row);
        }
        cell_address(self.snap_column(column, bias), self.snap_row(row, bias))
    }

    /// The first or last drawn column of the sheet.
    fn column_edge(&self, bias: Bias) -> u32 {
        match bias {
            Bias::Forward => self.snap_column(1, Bias::Forward),
            Bias::Backward => self.snap_column(self.max_col, Bias::Backward),
        }
    }

    /// The first or last drawn row of the sheet.
    fn row_edge(&self, bias: Bias) -> u32 {
        match bias {
            Bias::Forward => self.snap_row(1, Bias::Forward),
            Bias::Backward => self.snap_row(self.max_row, Bias::Backward),
        }
    }

    /// One arrow-key step from `(col, row)`.
    ///
    /// The step is taken and then carried on in the *same* direction until it
    /// reaches a drawn axis, so a hidden run is crossed in one keypress. When
    /// there is nothing drawn beyond it the focus does not move at all —
    /// the same answer the sheet edge already gives, and the reason this is
    /// the one snap that does not look the other way.
    fn step(&self, col: u32, row: u32, dc: i64, dr: i64) -> (u32, u32) {
        // A merged block is one cell, so the step leaves from the block's far
        // edge in the direction of travel and lands past it. Without this an
        // arrow key walked through the cells the block covers — positions the
        // grid draws nothing for, whose content is on the anchor.
        let (first_col, first_row, last_col, last_row) = self.block(col, row);
        let col = match dc {
            delta if delta > 0 => last_col,
            delta if delta < 0 => first_col,
            _ => col,
        };
        let row = match dr {
            delta if delta > 0 => last_row,
            delta if delta < 0 => first_row,
            _ => row,
        };
        let next_axis = |at: u32, delta: i64, len: u32, row_axis: bool| -> u32 {
            if delta == 0 {
                return at;
            }
            let mut index = at as i64 + delta;
            while (1..=len as i64).contains(&index) {
                let drawn = if row_axis {
                    self.row_drawn(index as u32)
                } else {
                    self.column_drawn(index as u32)
                };
                if drawn {
                    return index as u32;
                }
                index += delta;
            }
            at
        };
        let next_col = next_axis(col, dc, self.max_col, false);
        let next_row = next_axis(row, dr, self.max_row, true);
        // Landing inside another block means landing on its anchor.
        self.cover_anchor(next_col, next_row)
            .unwrap_or((next_col, next_row))
    }
}

fn direction_delta(value: &str) -> Result<(i64, i64), SpreadsheetError> {
    match value {
        "up" => Ok((0, -1)),
        "down" => Ok((0, 1)),
        "left" => Ok((-1, 0)),
        "right" => Ok((1, 0)),
        _ => Err(SpreadsheetError::Format(format!(
            "unknown spreadsheet selection direction {value}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describes_reversed_selection_and_numeric_summary() {
        let workbook = SpreadsheetWorkbook::sample();
        let selection = workbook
            .describe_selection("sheet-1", "b3", "B2")
            .expect("sample sheet selection should be valid");

        assert_eq!(selection.anchor, "B3");
        assert_eq!(selection.focus, "B2");
        assert_eq!(selection.range, "B2:B3");
        assert_eq!(selection.from_address, "B2");
        assert_eq!(selection.to_address, "B3");
        assert_eq!(selection.numeric_count, 2);
        assert_eq!(selection.numeric_sum, "10");
        assert_eq!(selection.numeric_average.as_deref(), Some("5.00"));
    }

    #[test]
    fn single_cell_selection_uses_single_address_range() {
        let workbook = SpreadsheetWorkbook::sample();
        let selection = workbook
            .describe_selection("sheet-1", "a1", "A1")
            .expect("sample sheet selection should be valid");

        assert_eq!(selection.range, "A1");
    }

    #[test]
    fn selection_tsv_reads_sparse_user_values() {
        let workbook = SpreadsheetWorkbook::sample();
        let tsv = workbook
            .selection_tsv("sheet-1", "A2", "B3")
            .expect("sample sheet selection should be valid");

        assert_eq!(tsv, "Apples\t5\nTotal\t=SUM(B2:B2)");
    }

    #[test]
    fn tsv_cell_edits_normalizes_origin_and_line_endings() {
        let cells = SpreadsheetWorkbook::tsv_cell_edits("b2", "x\ty\r\nz\t", None)
            .expect("TSV paste should be valid");

        assert_eq!(
            cells,
            vec![
                ("B2".to_string(), "x".to_string()),
                ("C2".to_string(), "y".to_string()),
                ("B3".to_string(), "z".to_string()),
                ("C3".to_string(), String::new()),
            ]
        );
    }

    #[test]
    fn fill_target_snaps_a_drag_to_one_axis() {
        let workbook = SpreadsheetWorkbook::sample();

        // Mostly downwards: the columns stay put.
        let down = workbook
            .reduce_selection("sheet-1", "A1", "B2", "fill-target", "D6", false)
            .unwrap();
        assert_eq!(down.range, "A1:B6");

        // Mostly sideways.
        let right = workbook
            .reduce_selection("sheet-1", "A1", "B2", "fill-target", "F3", false)
            .unwrap();
        assert_eq!(right.range, "A1:F2");

        // Upwards extends backwards from the source.
        let up = workbook
            .reduce_selection("sheet-1", "A4", "B5", "fill-target", "A1", false)
            .unwrap();
        assert_eq!(up.range, "A1:B5");

        // Inside the source: nothing to fill.
        let inside = workbook
            .reduce_selection("sheet-1", "A1", "B3", "fill-target", "A2", false)
            .unwrap();
        assert_eq!(inside.range, "A1:B3");
    }

    #[test]
    fn fill_source_names_the_leading_edge_of_the_selection() {
        let workbook = SpreadsheetWorkbook::sample();

        let down = workbook
            .reduce_selection("sheet-1", "B4", "A1", "fill-source", "down", false)
            .unwrap();
        assert_eq!(down.range, "A1:B1");

        let right = workbook
            .reduce_selection("sheet-1", "A1", "C3", "fill-source", "right", false)
            .unwrap();
        assert_eq!(right.range, "A1:A3");

        assert!(workbook
            .reduce_selection("sheet-1", "A1", "C3", "fill-source", "sideways", false)
            .is_err());
    }

    #[test]
    fn pasting_a_copied_formula_shifts_relative_references_only() {
        let cells = SpreadsheetWorkbook::tsv_cell_edits("C5", "=A1+$B$1\t7", Some("B2"))
            .expect("TSV paste should be valid");

        // B2 -> C5 is one column right and three rows down.
        assert_eq!(
            cells,
            vec![
                ("C5".to_string(), "=B4+$B$1".to_string()),
                ("D5".to_string(), "7".to_string()),
            ]
        );
    }

    #[test]
    fn pasting_text_from_outside_the_workbook_leaves_formulas_alone() {
        let cells = SpreadsheetWorkbook::tsv_cell_edits("C5", "=A1+1", None)
            .expect("TSV paste should be valid");

        assert_eq!(cells, vec![("C5".to_string(), "=A1+1".to_string())]);
    }

    #[test]
    fn pasting_back_onto_the_copy_origin_changes_nothing() {
        let cells = SpreadsheetWorkbook::tsv_cell_edits("b2", "=A1", Some("B2"))
            .expect("TSV paste should be valid");

        assert_eq!(cells, vec![("B2".to_string(), "=A1".to_string())]);
    }

    #[test]
    fn reduce_selection_handles_keyboard_and_name_box_actions() {
        let workbook = SpreadsheetWorkbook::sample();

        let moved = workbook
            .reduce_selection("sheet-1", "A1", "A1", "move", "right", false)
            .expect("move should be valid");
        assert_eq!(moved.anchor, "B1");
        assert_eq!(moved.focus, "B1");

        let extended = workbook
            .reduce_selection("sheet-1", "B1", "B1", "move", "down", true)
            .expect("move should be valid");
        assert_eq!(extended.anchor, "B1");
        assert_eq!(extended.focus, "B2");
        assert_eq!(extended.selected_addresses, vec!["B1", "B2"]);

        let named = workbook
            .reduce_selection("sheet-1", "A1", "A1", "set-range", "b3:A2", false)
            .expect("range should be valid");
        assert_eq!(named.anchor, "B3");
        assert_eq!(named.focus, "A2");
        assert_eq!(named.range, "A2:B3");
    }

    /// A workbook whose sheet has the named rows and columns hidden.
    fn workbook_hiding(rows: &[&str], columns: &[&str]) -> SpreadsheetWorkbook {
        let mut workbook = SpreadsheetWorkbook::sample();
        for row in rows {
            workbook
                .set_row_hidden("sheet-1", row, true)
                .expect("the row exists");
        }
        for column in columns {
            workbook
                .set_column_hidden("sheet-1", column, true)
                .expect("the column exists");
        }
        workbook
    }

    /// Arrow keys walk drawn cells: a hidden run is crossed in one keypress,
    /// in both directions, and at the end of the sheet the focus stays put
    /// rather than landing on something the grid does not draw.
    #[test]
    fn arrow_keys_step_over_hidden_rows_and_columns() {
        let workbook = workbook_hiding(&["3", "4"], &["C"]);

        let down = workbook
            .reduce_selection("sheet-1", "A2", "A2", "move", "down", false)
            .expect("move down");
        assert_eq!(
            down.focus, "A5",
            "rows 3 and 4 are hidden, so they are skipped"
        );

        let up = workbook
            .reduce_selection("sheet-1", "A5", "A5", "move", "up", false)
            .expect("move up");
        assert_eq!(up.focus, "A2");

        let right = workbook
            .reduce_selection("sheet-1", "B1", "B1", "move", "right", false)
            .expect("move right");
        assert_eq!(right.focus, "D1", "column C is hidden");

        let left = workbook
            .reduce_selection("sheet-1", "D1", "D1", "move", "left", false)
            .expect("move left");
        assert_eq!(left.focus, "B1");
    }

    /// Nothing drawn further along is the same answer as the sheet edge: the
    /// focus does not move, and it certainly does not turn round.
    #[test]
    fn an_arrow_key_with_only_hidden_axes_ahead_does_not_move() {
        let mut workbook = SpreadsheetWorkbook::sample();
        let rows: Vec<String> = workbook.sheets[0].rows.clone();
        for row in rows.iter().skip(1) {
            workbook
                .set_row_hidden("sheet-1", row, true)
                .expect("the row exists");
        }

        let down = workbook
            .reduce_selection("sheet-1", "A1", "A1", "move", "down", false)
            .expect("move down");
        assert_eq!(down.focus, "A1");
    }

    /// Shift+arrow extends *over* a hidden row: the focus lands on the next
    /// drawn one, and the range between still covers what is hidden, because
    /// hiding is not deletion.
    #[test]
    fn extending_a_selection_covers_the_hidden_rows_it_crosses() {
        let workbook = workbook_hiding(&["3"], &[]);

        let extended = workbook
            .reduce_selection("sheet-1", "A2", "A2", "move", "down", true)
            .expect("extend down");
        assert_eq!(extended.anchor, "A2");
        assert_eq!(extended.focus, "A4");
        assert_eq!(extended.selected_addresses, vec!["A2", "A3", "A4"]);
    }

    /// Ctrl+arrow, Home and select-all land on drawn extremes.
    #[test]
    fn edge_and_home_actions_land_on_drawn_axes() {
        let mut workbook = workbook_hiding(&["1"], &["A"]);
        let last_row = workbook.sheets[0].rows.last().cloned().expect("rows");
        let last_column = workbook.sheets[0].columns.last().cloned().expect("columns");
        workbook
            .set_row_hidden("sheet-1", &last_row, true)
            .expect("the row exists");
        workbook
            .set_column_hidden("sheet-1", &last_column, true)
            .expect("the column exists");

        let up = workbook
            .reduce_selection("sheet-1", "C5", "C5", "move-edge", "up", false)
            .expect("edge up");
        assert_eq!(up.focus, "C2", "row 1 is hidden");

        let down = workbook
            .reduce_selection("sheet-1", "C5", "C5", "move-edge", "down", false)
            .expect("edge down");
        assert_eq!(down.focus, "C99", "row 100 is hidden");

        let left = workbook
            .reduce_selection("sheet-1", "C5", "C5", "move-edge", "left", false)
            .expect("edge left");
        assert_eq!(left.focus, "B5", "column A is hidden");

        let home = workbook
            .reduce_selection("sheet-1", "C5", "C5", "home", "row", false)
            .expect("home");
        assert_eq!(home.focus, "B5");

        let sheet_home = workbook
            .reduce_selection("sheet-1", "C5", "C5", "home", "sheet", false)
            .expect("home sheet");
        assert_eq!(sheet_home.focus, "B2");

        let all = workbook
            .reduce_selection("sheet-1", "C5", "C5", "select-all", "", false)
            .expect("select all");
        assert_eq!(all.range, "B2:Y99", "the drawn corners of the whole sheet");
    }

    /// The merged-anchor case: the one hidden address that stays selectable.
    ///
    /// A merge `A1:B2` with column A hidden is still drawn, and the `<td>` the
    /// renderer emits for it still carries the merge's anchor `A1`, because
    /// that is where the content and the formula live. So a click reporting
    /// `A1` selects `A1` — snapping it to `B1` would point the selection at a
    /// cell the merge *covers* and the grid never emits.
    #[test]
    fn clicking_a_merged_block_whose_anchor_is_hidden_keeps_the_anchor() {
        let mut workbook = workbook_hiding(&[], &["A"]);
        workbook
            .merge_cells("sheet-1", "A1:B2")
            .expect("the sheet exists")
            .expect("merging A1:B2");

        let clicked = workbook
            .reduce_selection("sheet-1", "C3", "C3", "set-focus", "A1", false)
            .expect("a click on the merged block");
        assert_eq!(
            clicked.focus, "A1",
            "the drawn block is labelled with its anchor, so the anchor is selectable"
        );
        assert_eq!(clicked.anchor, "A1");
    }

    /// Without the merge there is nothing drawn for `A1`, and the same click
    /// snaps — which is what makes the exemption above an exemption rather
    /// than a blanket "hidden cells are fine".
    #[test]
    fn the_merge_exemption_does_not_leak_to_ordinary_hidden_cells() {
        let workbook = workbook_hiding(&[], &["A"]);

        let clicked = workbook
            .reduce_selection("sheet-1", "C3", "C3", "set-focus", "A1", false)
            .expect("a click");
        assert_eq!(clicked.focus, "B1");
    }

    /// A merge every cell of which is hidden is not drawn at all, so its
    /// anchor is not an exception either.
    #[test]
    fn a_wholly_hidden_merge_does_not_keep_its_anchor_selectable() {
        let mut workbook = workbook_hiding(&[], &["A", "B"]);
        workbook
            .merge_cells("sheet-1", "A1:B2")
            .expect("the sheet exists")
            .expect("merging A1:B2");

        let clicked = workbook
            .reduce_selection("sheet-1", "D3", "D3", "set-focus", "A1", false)
            .expect("a stale selection pointing into the hidden merge");
        assert_eq!(clicked.focus, "C1");
    }

    fn workbook_merging(range: &str) -> SpreadsheetWorkbook {
        let mut workbook = SpreadsheetWorkbook::sample();
        workbook
            .merge_cells("sheet-1", range)
            .expect("the sheet exists")
            .expect("the merge is accepted");
        workbook
    }

    /// A merged block is one cell to the keyboard: `A2 → B2 → D2` over a
    /// `B2:C3` merge, never stopping on `C2`, which the grid draws nothing
    /// for. Walking back is the mirror image, and lands on the anchor.
    #[test]
    fn an_arrow_key_steps_over_the_cells_a_merge_covers() {
        let workbook = workbook_merging("B2:C3");

        let into = workbook
            .reduce_selection("sheet-1", "A2", "A2", "move", "right", false)
            .expect("a step into the block");
        assert_eq!(into.focus, "B2");

        let out = workbook
            .reduce_selection("sheet-1", "B2", "B2", "move", "right", false)
            .expect("a step out of the block");
        assert_eq!(
            out.focus, "D2",
            "the step stopped on C2, which the grid never draws"
        );

        let back = workbook
            .reduce_selection("sheet-1", "D2", "D2", "move", "left", false)
            .expect("a step back into the block");
        assert_eq!(back.focus, "B2", "walking back lands on the anchor");

        let below = workbook
            .reduce_selection("sheet-1", "B2", "B2", "move", "down", false)
            .expect("a step below the block");
        assert_eq!(below.focus, "B4", "the block is two rows tall");
    }

    /// A click reported against a covered address selects the block, which
    /// means the anchor: that is the cell that is drawn, that holds the
    /// content, and that the formula bar edits.
    #[test]
    fn a_click_on_a_covered_cell_snaps_to_the_merge_anchor() {
        let workbook = workbook_merging("B2:C3");

        let clicked = workbook
            .reduce_selection("sheet-1", "A1", "A1", "set-focus", "C2", false)
            .expect("a click inside the block");
        assert_eq!(clicked.focus, "B2");
        assert_eq!(clicked.anchor, "B2");
    }

    /// The status bar describes what is drawn. A merged block holds its
    /// content on its anchor, so a selection covering the block counts it
    /// once — not once per covered cell.
    ///
    /// The value under the merge is placed **directly**, not through
    /// `set_cell_in_sheet` and not before `merge_cells`: both of those now
    /// refuse or clear it, so a test that built the state through them would
    /// pass whether or not the summary skips covered cells. This is the state
    /// that really occurs — a workbook that arrived from an import with
    /// content already under a merge, which `merge_cells` never saw.
    #[test]
    fn the_selection_summary_counts_a_merged_block_once() {
        let mut workbook = SpreadsheetWorkbook::sample();
        workbook
            .set_cell_in_sheet("sheet-1", "D1", "10".to_string())
            .expect("the sheet exists");
        workbook
            .set_cell_in_sheet("sheet-1", "E1", "10".to_string())
            .expect("the sheet exists");
        let before = workbook
            .describe_selection("sheet-1", "D1", "E1")
            .expect("a selection over both cells");
        assert_eq!(before.numeric_count, 2);

        workbook.sheets[0].merges.push(crate::SheetMerge {
            id: "merge-d1-e1".to_string(),
            range: "D1:E1".to_string(),
        });
        let after = workbook
            .describe_selection("sheet-1", "D1", "E1")
            .expect("a selection over the block");
        assert_eq!(
            after.numeric_count, 1,
            "the covered cell was counted as a second value"
        );
        assert_eq!(after.numeric_sum, "10");
    }

    /// The same snap for a fill-handle drag, which also reads its target off a
    /// drawn element.
    #[test]
    fn a_fill_drag_over_a_hidden_anchor_snaps_to_the_drawn_cell() {
        let workbook = workbook_hiding(&["2"], &[]);

        let dragged = workbook
            .reduce_selection("sheet-1", "B1", "B1", "fill-target", "A2", false)
            .expect("a fill drag");
        assert_eq!(
            dragged.to_row, 3,
            "row 2 is hidden, so the drag reaches row 3"
        );
    }

    /// Explicit addressing from the name box keeps the range it was given —
    /// hidden axes inside it included — but its two ends snap inwards so the
    /// caret has somewhere to go.
    #[test]
    fn the_name_box_keeps_a_range_that_spans_hidden_rows_but_snaps_its_ends() {
        let workbook = workbook_hiding(&["2", "5"], &[]);

        let spanning = workbook
            .reduce_selection("sheet-1", "A1", "A1", "set-range", "A1:A6", false)
            .expect("a range across a hidden row");
        assert_eq!(
            spanning.range, "A1:A6",
            "the hidden row stays inside the range"
        );

        let hidden_end = workbook
            .reduce_selection("sheet-1", "A1", "A1", "set-range", "A5", false)
            .expect("a hidden single cell");
        assert_eq!(
            hidden_end.range, "A4:A6",
            "a hidden cell named on its own snaps its ends apart, forward and back"
        );
        assert_eq!(hidden_end.anchor, "A6");
        assert_eq!(hidden_end.focus, "A4");
    }

    /// A selection that arrives pointing at a hidden cell — from a state saved
    /// before the row was hidden — is normalised on the way in, so no action
    /// does its arithmetic from a cell the grid does not draw.
    #[test]
    fn an_incoming_selection_on_a_hidden_row_is_snapped_before_anything_else() {
        let workbook = workbook_hiding(&["3"], &[]);

        let moved = workbook
            .reduce_selection("sheet-1", "A3", "A3", "move", "down", false)
            .expect("move down from a hidden row");
        assert_eq!(
            moved.focus, "A5",
            "A3 snaps forward to A4, and the step goes on to A5"
        );
    }

    #[test]
    fn derives_focus_axis_actions() {
        let workbook = SpreadsheetWorkbook::sample();

        assert_eq!(workbook.row_after_focus("b2").unwrap(), "3");
        assert_eq!(workbook.column_after_focus("b2").unwrap(), "C");
        assert_eq!(workbook.focus_row_label("b2").unwrap(), "2");
        assert_eq!(workbook.focus_column_label("b2").unwrap(), "B");
        assert_eq!(workbook.frozen_axes_for_focus("b2").unwrap(), (1, 1));
    }
}
