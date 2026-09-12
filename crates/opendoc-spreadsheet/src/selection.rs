//! Spreadsheet selection and clipboard projections.

use serde::{Deserialize, Serialize};

use crate::{FormulaValue, SpreadsheetError};

use super::address::{
    cell_address, cell_axis_labels, normalize_cell_address, number_to_column, parse_cell_position,
    trim_number,
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
        for cell in &sheet.cells {
            let Ok((col, row)) = parse_cell_position(&cell.address) else {
                continue;
            };
            if col < from_col || col > to_col || row < from_row || row > to_row {
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
        let anchor = normalize_cell_address(anchor.as_ref()).unwrap_or_else(|_| "A1".to_string());
        let focus = normalize_cell_address(focus.as_ref()).unwrap_or_else(|_| anchor.clone());
        let (col, row) = parse_cell_position(&focus)?;
        let max_col = sheet.columns.len().max(1) as u32;
        let max_row = sheet.rows.len().max(1) as u32;
        let action = action.as_ref();
        let value = value.as_ref();
        let (next_anchor, next_focus) = match action {
            "move" => {
                let (dc, dr) = direction_delta(value)?;
                let next = bounded_address(col as i64 + dc, row as i64 + dr, max_col, max_row)?;
                (if extend { anchor } else { next.clone() }, next)
            }
            "move-edge" => {
                let next_col = match value {
                    "left" => 1,
                    "right" => max_col,
                    _ => col,
                };
                let next_row = match value {
                    "up" => 1,
                    "down" => max_row,
                    _ => row,
                };
                let next = cell_address(next_col, next_row)?;
                (if extend { anchor } else { next.clone() }, next)
            }
            "home" => {
                let next = cell_address(1, if value == "sheet" { 1 } else { row })?;
                (if extend { anchor } else { next.clone() }, next)
            }
            "select-all" => ("A1".to_string(), cell_address(max_col, max_row)?),
            "set-focus" => {
                let next = normalize_cell_address(value)?;
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
                let hover = normalize_cell_address(value)?;
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
            "set-range" => {
                let (start, end) = value.split_once(':').unwrap_or((value, value));
                let start = normalize_cell_address(start)?;
                let end = normalize_cell_address(end)?;
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

fn bounded_address(
    col: i64,
    row: i64,
    max_col: u32,
    max_row: u32,
) -> Result<String, SpreadsheetError> {
    cell_address(
        col.clamp(1, max_col as i64) as u32,
        row.clamp(1, max_row as i64) as u32,
    )
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
