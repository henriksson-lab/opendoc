//! Positional row/column operations, formula reference rewriting, and
//! range sorting.

use std::collections::BTreeMap;

use super::address::{
    cell_address, column_to_number, formula_sheet_title_prefix, normalize_cell_range,
    normalize_column_label, normalize_formula_sheet_prefix, normalize_merge_range,
    number_to_column, parse_cell_position, parse_cell_range, range_contains_range, ranges_overlap,
    shift_cell_range, shift_formula_references, split_cell_address,
};
use super::format::{self, Locale};
use super::formula::{is_unquoted_sheet_prefix_char, parse_reference, CellCoord, RefExpr};
use super::model::{
    column_axis, normalize_protected_range_description, row_axis,
    validate_protected_range_description,
};
use super::value::{compare_values, FormulaValue};
use crate::{
    Cell, Sheet, SheetFilter, SheetFilterCriterion, SheetFilterSortSpec, SheetMerge,
    SheetProtectedRange, SpreadsheetError, SpreadsheetWorkbook,
};

/// A lexical segment of a formula: plain text or a reference.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Segment {
    Text(String),
    Reference {
        /// Sheet prefix exactly as written (without `!`), if any.
        prefix_raw: Option<String>,
        /// Normalized (unquoted) sheet prefix, if any.
        prefix: Option<String>,
        /// Local reference text as written (`$A$1:B2`, `A:A`, `1:1`).
        local_raw: String,
        local: RefExpr,
    },
}

fn is_name_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.')
}

/// Scans a local reference (`$A$1`, `A1:B2`, `A:A`, `1:1`) at `start`.
fn scan_local_reference(chars: &[char], start: usize) -> Option<(usize, RefExpr)> {
    let mut index = start;
    let scan_coord = |index: &mut usize| -> Option<(bool, bool)> {
        let begin = *index;
        if chars.get(*index) == Some(&'$') {
            *index += 1;
        }
        let letters_start = *index;
        while chars.get(*index).is_some_and(|ch| ch.is_ascii_alphabetic()) {
            *index += 1;
        }
        let letters = *index - letters_start;
        if chars.get(*index) == Some(&'$') {
            *index += 1;
        }
        let digits_start = *index;
        while chars.get(*index).is_some_and(|ch| ch.is_ascii_digit()) {
            *index += 1;
        }
        let digits = *index - digits_start;
        if (letters == 0 && digits == 0) || letters > 3 {
            *index = begin;
            return None;
        }
        Some((letters > 0, digits > 0))
    };
    let (has_letters, has_digits) = scan_coord(&mut index)?;
    // A bare column or row needs a `:` continuation.
    if !(has_letters && has_digits) {
        if chars.get(index) != Some(&':') {
            return None;
        }
        let mut end = index + 1;
        let (end_letters, end_digits) = scan_coord(&mut end)?;
        if end_letters != has_letters || end_digits != has_digits {
            return None;
        }
        if chars
            .get(end)
            .is_some_and(|ch| is_name_char(*ch) || *ch == '(')
        {
            return None;
        }
        let text: String = chars[start..end].iter().collect();
        let reference = parse_reference(&text)?;
        return Some((end, reference));
    }
    if chars
        .get(index)
        .is_some_and(|ch| is_name_char(*ch) || *ch == '(')
    {
        return None;
    }
    let mut end = index;
    if chars.get(index) == Some(&':') {
        let mut range_end = index + 1;
        if let Some((true, true)) = scan_coord(&mut range_end) {
            if !chars
                .get(range_end)
                .is_some_and(|ch| is_name_char(*ch) || *ch == '(')
            {
                end = range_end;
            }
        }
    }
    let text: String = chars[start..end].iter().collect();
    let reference = parse_reference(&text)?;
    Some((end, reference))
}

/// Splits a formula into text and reference segments. Strings and
/// function names are never treated as references.
pub fn tokenize_formula(formula: &str) -> Vec<Segment> {
    let chars: Vec<char> = formula.chars().collect();
    let mut segments: Vec<Segment> = Vec::new();
    let mut text = String::new();
    let mut index = 0;
    let push_text = |segments: &mut Vec<Segment>, text: &mut String| {
        if !text.is_empty() {
            segments.push(Segment::Text(std::mem::take(text)));
        }
    };
    while index < chars.len() {
        let ch = chars[index];
        if ch == '"' {
            let start = index;
            index += 1;
            while index < chars.len() {
                if chars[index] == '"' {
                    index += 1;
                    if chars.get(index) == Some(&'"') {
                        index += 1;
                        continue;
                    }
                    break;
                }
                index += 1;
            }
            text.extend(chars[start..index].iter());
            continue;
        }
        if ch == '\'' {
            let start = index;
            index += 1;
            while index < chars.len() {
                if chars[index] == '\'' {
                    index += 1;
                    if chars.get(index) == Some(&'\'') {
                        index += 1;
                        continue;
                    }
                    break;
                }
                index += 1;
            }
            let prefix_raw: String = chars[start..index].iter().collect();
            if chars.get(index) == Some(&'!') {
                if let Some((end, local)) = scan_local_reference(&chars, index + 1) {
                    push_text(&mut segments, &mut text);
                    segments.push(Segment::Reference {
                        prefix: normalize_formula_sheet_prefix(&prefix_raw).ok(),
                        prefix_raw: Some(prefix_raw),
                        local_raw: chars[index + 1..end].iter().collect(),
                        local,
                    });
                    index = end;
                    continue;
                }
            }
            text.push_str(&prefix_raw);
            continue;
        }
        if ch.is_ascii_alphabetic() || ch == '$' || ch.is_ascii_digit() {
            // Unquoted sheet prefix.
            if ch.is_ascii_alphabetic() {
                let mut prefix_end = index;
                while chars
                    .get(prefix_end)
                    .is_some_and(|ch| is_unquoted_sheet_prefix_char(*ch))
                {
                    prefix_end += 1;
                }
                if chars.get(prefix_end) == Some(&'!') {
                    if let Some((end, local)) = scan_local_reference(&chars, prefix_end + 1) {
                        let prefix_raw: String = chars[index..prefix_end].iter().collect();
                        push_text(&mut segments, &mut text);
                        segments.push(Segment::Reference {
                            prefix: Some(prefix_raw.clone()),
                            prefix_raw: Some(prefix_raw),
                            local_raw: chars[prefix_end + 1..end].iter().collect(),
                            local,
                        });
                        index = end;
                        continue;
                    }
                }
            }
            // Numbers that are not whole-row ranges are plain text.
            let previous_is_name = index > 0 && is_name_char(chars[index - 1]);
            if !previous_is_name {
                if let Some((end, local)) = scan_local_reference(&chars, index) {
                    push_text(&mut segments, &mut text);
                    segments.push(Segment::Reference {
                        prefix_raw: None,
                        prefix: None,
                        local_raw: chars[index..end].iter().collect(),
                        local,
                    });
                    index = end;
                    continue;
                }
            }
            // Consume the identifier/number as text.
            let start = index;
            index += 1;
            while chars
                .get(index)
                .is_some_and(|ch| is_name_char(*ch) || *ch == '$')
            {
                index += 1;
            }
            text.extend(chars[start..index].iter());
            continue;
        }
        text.push(ch);
        index += 1;
    }
    push_text(&mut segments, &mut text);
    segments
}

/// Rewrites every reference in a formula. The callback receives the
/// normalized sheet prefix (if any) and the local reference and returns
/// replacement text for the local part (`None` keeps it unchanged).
pub fn rewrite_formula_references(
    formula: &str,
    mut rewrite: impl FnMut(Option<&str>, &RefExpr) -> Option<String>,
) -> String {
    let mut out = String::new();
    for segment in tokenize_formula(formula) {
        match segment {
            Segment::Text(text) => out.push_str(&text),
            Segment::Reference {
                prefix_raw,
                prefix,
                local_raw,
                local,
            } => {
                let replacement = rewrite(prefix.as_deref(), &local);
                if let Some(prefix_raw) = &prefix_raw {
                    if replacement.as_deref() != Some("#REF!") {
                        out.push_str(prefix_raw);
                        out.push('!');
                    }
                }
                out.push_str(&replacement.unwrap_or(local_raw));
            }
        }
    }
    out
}

/// Rewrites sheet prefixes that match `old_title` to `new_title`.
pub fn rewrite_sheet_prefixes(formula: &str, old_title: &str, new_title: &str) -> String {
    let mut out = String::new();
    for segment in tokenize_formula(formula) {
        match segment {
            Segment::Text(text) => out.push_str(&text),
            Segment::Reference {
                prefix_raw,
                prefix,
                local_raw,
                ..
            } => {
                if let Some(prefix_raw) = prefix_raw {
                    if prefix.as_deref() == Some(old_title) {
                        out.push_str(&formula_sheet_title_prefix(new_title));
                    } else {
                        out.push_str(&prefix_raw);
                    }
                    out.push('!');
                }
                out.push_str(&local_raw);
            }
        }
    }
    out
}

/// Shifts relative references by a copy/paste offset. References that
/// would move off the sheet become `#REF!`.
pub fn shift_relative_references(formula: &str, column_delta: i32, row_delta: i32) -> String {
    rewrite_formula_references(formula, |_, local| {
        let shift = |coord: &CellCoord| -> Option<CellCoord> {
            let col = match coord.col {
                Some(col) if !coord.col_abs => {
                    let shifted = col as i64 + column_delta as i64;
                    if shifted < 1 {
                        return None;
                    }
                    Some(shifted as u32)
                }
                other => other,
            };
            let row = match coord.row {
                Some(row) if !coord.row_abs => {
                    let shifted = row as i64 + row_delta as i64;
                    if shifted < 1 {
                        return None;
                    }
                    Some(shifted as u32)
                }
                other => other,
            };
            Some(CellCoord {
                col,
                row,
                col_abs: coord.col_abs,
                row_abs: coord.row_abs,
            })
        };
        let start = shift(&local.start);
        let end = local.end.as_ref().map(shift);
        match (start, end) {
            (Some(start), None) => Some(
                RefExpr {
                    sheet: None,
                    start,
                    end: None,
                }
                .to_text(),
            ),
            (Some(start), Some(Some(end))) => Some(
                RefExpr {
                    sheet: None,
                    start,
                    end: Some(end),
                }
                .to_text(),
            ),
            _ => Some("#REF!".to_string()),
        }
    })
}

/// Shifts a formula so that a formula copied from `from` behaves the same
/// way at `to` (SH-7).
pub fn transform_formula_for_paste(
    formula: &str,
    from: &str,
    to: &str,
) -> Result<String, SpreadsheetError> {
    let (from_column, from_row) = parse_cell_position(from)?;
    let (to_column, to_row) = parse_cell_position(to)?;
    if !formula.trim_start().starts_with('=') {
        return Ok(formula.to_string());
    }
    Ok(shift_relative_references(
        formula,
        to_column as i32 - from_column as i32,
        to_row as i32 - from_row as i32,
    ))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    Row,
    Column,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AxisEdit {
    Insert { at: u32, count: u32 },
    Delete { start: u32, count: u32 },
}

impl AxisEdit {
    /// Maps a 1-based index through the edit; `None` when deleted.
    fn shift(&self, index: u32) -> Option<u32> {
        match *self {
            AxisEdit::Insert { at, count } => Some(if index >= at { index + count } else { index }),
            AxisEdit::Delete { start, count } => {
                if index < start {
                    Some(index)
                } else if index < start + count {
                    None
                } else {
                    Some(index - count)
                }
            }
        }
    }

    /// Maps an inclusive span; `None` when entirely deleted.
    fn shift_span(&self, first: u32, last: u32) -> Option<(u32, u32)> {
        match *self {
            AxisEdit::Insert { .. } => Some((self.shift(first)?, self.shift(last)?)),
            AxisEdit::Delete { start, count } => {
                let end = start + count;
                if first >= start && last < end {
                    return None;
                }
                let new_first = if first < start {
                    first
                } else if first < end {
                    start
                } else {
                    first - count
                };
                let new_last = if last < start {
                    last
                } else if last < end {
                    start - 1
                } else {
                    last - count
                };
                if new_last < new_first || new_first < 1 {
                    None
                } else {
                    Some((new_first, new_last))
                }
            }
        }
    }
}

/// Rewrites a local reference for an axis edit; `None` means `#REF!`.
fn shift_reference(local: &RefExpr, axis: Axis, edit: AxisEdit) -> Option<RefExpr> {
    let axis_of = |coord: &CellCoord| match axis {
        Axis::Row => coord.row,
        Axis::Column => coord.col,
    };
    let with_axis = |coord: &CellCoord, value: u32| match axis {
        Axis::Row => CellCoord {
            row: Some(value),
            ..coord.clone()
        },
        Axis::Column => CellCoord {
            col: Some(value),
            ..coord.clone()
        },
    };
    match &local.end {
        None => {
            let Some(value) = axis_of(&local.start) else {
                return Some(local.clone());
            };
            let shifted = edit.shift(value)?;
            Some(RefExpr {
                sheet: None,
                start: with_axis(&local.start, shifted),
                end: None,
            })
        }
        Some(end) => {
            let (Some(first), Some(last)) = (axis_of(&local.start), axis_of(end)) else {
                return Some(local.clone());
            };
            let (low, high) = (first.min(last), first.max(last));
            let (new_low, new_high) = edit.shift_span(low, high)?;
            let (new_first, new_last) = if first <= last {
                (new_low, new_high)
            } else {
                (new_high, new_low)
            };
            Some(RefExpr {
                sheet: None,
                start: with_axis(&local.start, new_first),
                end: Some(with_axis(end, new_last)),
            })
        }
    }
}

fn shift_range_text(range: &str, axis: Axis, edit: AxisEdit) -> Option<String> {
    let parsed = parse_cell_range(range).ok()?;
    let (first, last) = match axis {
        Axis::Row => (parsed.start_row, parsed.start_row + parsed.height - 1),
        Axis::Column => (parsed.start_column, parsed.start_column + parsed.width - 1),
    };
    let (new_first, new_last) = edit.shift_span(first, last)?;
    let (c1, c2, r1, r2) = match axis {
        Axis::Row => (
            parsed.start_column,
            parsed.start_column + parsed.width - 1,
            new_first,
            new_last,
        ),
        Axis::Column => (
            new_first,
            new_last,
            parsed.start_row,
            parsed.start_row + parsed.height - 1,
        ),
    };
    let start = format!("{}{r1}", number_to_column(c1)?);
    let end = format!("{}{r2}", number_to_column(c2)?);
    normalize_cell_range(&format!("{start}:{end}")).ok()
}

fn shift_address(address: &str, axis: Axis, edit: AxisEdit) -> Option<String> {
    let (column, row) = split_cell_address(address);
    let col = column_to_number(&column)?;
    let row = row.parse::<u32>().ok()?;
    let (col, row) = match axis {
        Axis::Row => (col, edit.shift(row)?),
        Axis::Column => (edit.shift(col)?, row),
    };
    Some(format!("{}{row}", number_to_column(col)?))
}

fn shift_label(label: &str, axis: Axis, edit: AxisEdit) -> Option<String> {
    match axis {
        Axis::Row => label
            .parse::<u32>()
            .ok()
            .and_then(|row| edit.shift(row))
            .map(|row| row.to_string()),
        Axis::Column => column_to_number(label)
            .and_then(|col| edit.shift(col))
            .and_then(number_to_column),
    }
}

/// Rewrites all formulas in the workbook for an axis edit on `sheet_id`.
fn rewrite_workbook_formulas(
    workbook: &mut SpreadsheetWorkbook,
    sheet_id: &str,
    axis: Axis,
    edit: AxisEdit,
) {
    let target_title = workbook
        .sheets
        .iter()
        .find(|sheet| sheet.id == sheet_id)
        .map(|sheet| sheet.title.clone())
        .unwrap_or_default();
    for sheet in &mut workbook.sheets {
        let same_sheet = sheet.id == sheet_id;
        for cell in &mut sheet.cells {
            if cell.user_kind != "formula" {
                continue;
            }
            cell.user_value = rewrite_formula_references(&cell.user_value, |prefix, local| {
                let targets_sheet = match prefix {
                    None => same_sheet,
                    Some(prefix) => prefix == sheet_id || prefix == target_title,
                };
                if !targets_sheet {
                    return None;
                }
                Some(
                    shift_reference(local, axis, edit)
                        .map(|reference| reference.to_text())
                        .unwrap_or_else(|| "#REF!".to_string()),
                )
            });
        }
    }
}

/// Inserts `count` rows or columns so that the first new one sits at
/// 1-based position `at`.
pub fn insert_axis(
    workbook: &mut SpreadsheetWorkbook,
    sheet_id: &str,
    axis: Axis,
    at: u32,
    count: u32,
) -> Result<(), SpreadsheetError> {
    if at < 1 || !(1..=10_000).contains(&count) {
        return Err(SpreadsheetError::Format(
            "spreadsheet insert position and count must be positive".to_string(),
        ));
    }
    let edit = AxisEdit::Insert { at, count };
    apply_axis_edit(workbook, sheet_id, axis, edit)?;
    let sheet = workbook
        .sheets
        .iter_mut()
        .find(|sheet| sheet.id == sheet_id)
        .ok_or_else(|| SpreadsheetError::NotFound(format!("sheet {sheet_id} was not found")))?;
    for offset in 0..count {
        let label = match axis {
            Axis::Row => (at + offset).to_string(),
            Axis::Column => number_to_column(at + offset).unwrap_or_default(),
        };
        match axis {
            Axis::Row => {
                if !sheet.rows.contains(&label) {
                    sheet.rows.push(label);
                }
            }
            Axis::Column => {
                if !sheet.columns.contains(&label) {
                    sheet.columns.push(label);
                }
            }
        }
    }
    sort_axis_labels(sheet);
    Ok(())
}

/// Deletes `count` rows or columns starting at 1-based `start`, returning
/// the removed cells.
pub fn delete_axis(
    workbook: &mut SpreadsheetWorkbook,
    sheet_id: &str,
    axis: Axis,
    start: u32,
    count: u32,
) -> Result<Vec<Cell>, SpreadsheetError> {
    if start < 1 || count < 1 {
        return Err(SpreadsheetError::Format(
            "spreadsheet delete position and count must be positive".to_string(),
        ));
    }
    let sheet = workbook
        .sheets
        .iter()
        .find(|sheet| sheet.id == sheet_id)
        .ok_or_else(|| SpreadsheetError::NotFound(format!("sheet {sheet_id} was not found")))?;
    let labels = match axis {
        Axis::Row => &sheet.rows,
        Axis::Column => &sheet.columns,
    };
    let edit = AxisEdit::Delete { start, count };
    let surviving = labels
        .iter()
        .filter(|label| shift_label(label, axis, edit).is_some())
        .count();
    if surviving == 0 {
        return Err(SpreadsheetError::Conflict(format!(
            "cannot delete every spreadsheet {}",
            match axis {
                Axis::Row => "row",
                Axis::Column => "column",
            }
        )));
    }
    let removed = sheet
        .cells
        .iter()
        .filter(|cell| shift_address(&cell.address, axis, edit).is_none())
        .cloned()
        .collect();
    apply_axis_edit(workbook, sheet_id, axis, edit)?;
    Ok(removed)
}

fn sort_axis_labels(sheet: &mut Sheet) {
    sheet
        .rows
        .sort_by_key(|value| value.parse::<u32>().unwrap_or(0));
    sheet.rows.dedup();
    sheet
        .columns
        .sort_by_key(|value| column_to_number(value).unwrap_or(0));
    sheet.columns.dedup();
    sheet.row_axes = sheet.rows.iter().cloned().map(row_axis).collect();
    sheet.column_axes = sheet.columns.iter().cloned().map(column_axis).collect();
}

/// Writes one cell from text the user typed.
///
/// # What is stored, and what is derived
///
/// The text is *input*, not state. `50%`, `$5`, `1,000` and `2024-01-05`
/// each name a number and a way of showing it, and the cell stores both
/// separately: `user_value` holds the canonical number (`0.5`, `5`, `1000`,
/// the date serial `45296`) and `format.number_format` holds the pattern the
/// input implied (`0%`, `$#,##0`, `#,##0`, `YYYY-MM-DD`). Storing the raw
/// text instead is what made `=B1+1` on a typed date `#VALUE!` and a column
/// of typed percentages sum to zero — every consumer would have had to
/// re-parse the same text, in the same locale, and agree.
///
/// The typed text is not lost: the operation journal records what the user
/// typed, and replaying it re-parses to the same value. `display_value` is
/// then derived from the stored number and the stored format, so the cell
/// shows `50%` again.
///
/// An inferred format never overwrites a format the cell already has. A
/// column deliberately formatted as currency stays currency when someone
/// types `1,000` into it; only a cell with no format of its own takes the
/// one its input implied.
pub fn set_sheet_cell(sheet: &mut Sheet, address: &str, value: String, locale: &Locale) {
    sheet.ensure_address(address);
    let parsed = format::parse_input(&value, locale);
    let cell = if let Some(index) = sheet.cells.iter().position(|cell| cell.address == address) {
        &mut sheet.cells[index]
    } else {
        sheet
            .cells
            .push(Cell::new(address, parsed.kind, &parsed.value));
        sheet
            .cells
            .sort_by(|left, right| left.address.cmp(&right.address));
        let index = sheet
            .cells
            .iter()
            .position(|cell| cell.address == address)
            .expect("newly inserted spreadsheet cell exists");
        &mut sheet.cells[index]
    };
    cell.user_kind = parsed.kind.to_string();
    cell.user_value = parsed.value;
    if let Some(number_format) = parsed.number_format {
        if cell.format.number_format.is_none() {
            cell.format.number_format = Some(number_format);
        }
    }
}

pub fn set_sheet_cell_format(
    sheet: &mut Sheet,
    address: &str,
    property: &str,
    value: String,
) -> Result<(), SpreadsheetError> {
    sheet.ensure_address(address);
    let cell = if let Some(index) = sheet.cells.iter().position(|cell| cell.address == address) {
        &mut sheet.cells[index]
    } else {
        sheet.cells.push(Cell::new(address, "empty", ""));
        sheet
            .cells
            .sort_by(|left, right| left.address.cmp(&right.address));
        sheet
            .cells
            .iter_mut()
            .find(|cell| cell.address == address)
            .expect("newly inserted cell exists")
    };
    cell.format.set_property(property, value)
}

pub fn copy_sheet_range(
    sheet: &mut Sheet,
    source_range: &str,
    target_address: &str,
) -> Result<(), SpreadsheetError> {
    let source = parse_cell_range(source_range)?;
    let (target_column, target_row) = parse_cell_position(target_address)?;
    let source_cells = sheet
        .cells
        .iter()
        .map(|cell| (cell.address.clone(), cell.clone()))
        .collect::<BTreeMap<_, _>>();
    let source_merges = sheet.merges.clone();

    for row_offset in 0..source.height {
        for column_offset in 0..source.width {
            let source_column = source.start_column + column_offset;
            let source_row = source.start_row + row_offset;
            let target_column = target_column + column_offset;
            let target_row = target_row + row_offset;
            let source_address = cell_address(source_column, source_row)?;
            let target_address = cell_address(target_column, target_row)?;
            let mut copied = source_cells
                .get(&source_address)
                .cloned()
                .unwrap_or_else(|| Cell::new(&source_address, "empty", ""));
            copied.address = target_address.clone();
            if copied.user_kind == "formula" {
                let column_delta = target_column as i32 - source_column as i32;
                let row_delta = target_row as i32 - source_row as i32;
                copied.user_value =
                    shift_formula_references(&copied.user_value, column_delta, row_delta);
            }
            upsert_sheet_cell(sheet, copied);
        }
    }
    let column_delta = target_column as i32 - source.start_column as i32;
    let row_delta = target_row as i32 - source.start_row as i32;
    for merge in source_merges {
        let merge_range = parse_cell_range(&merge.range)?;
        if range_contains_range(source, merge_range) {
            let shifted = shift_cell_range(merge_range, column_delta, row_delta)?;
            merge_sheet_cells(sheet, &shifted)?;
        }
    }
    Ok(())
}

/// The anchor of the merge covering `address`, when `address` is covered by a
/// merge but is not itself that merge's anchor.
///
/// A merged block keeps its content, its formatting and its `<td>` on the
/// top-left anchor, and `opendoc-render` emits nothing at all for the cells
/// the block covers. A value written into one of those is stored, signed and
/// exported — and never drawn anywhere. Every write path therefore asks this
/// first and refuses, naming the anchor the caller meant; and selection asks
/// it so the focus ring never lands somewhere with no pixel to draw it in.
pub fn merge_cover_anchor(sheet: &Sheet, address: &str) -> Option<String> {
    let (column, row) = parse_cell_position(address).ok()?;
    for merge in &sheet.merges {
        let Ok(range) = parse_cell_range(&merge.range) else {
            continue;
        };
        let covers = (range.start_column..range.start_column + range.width).contains(&column)
            && (range.start_row..range.start_row + range.height).contains(&row);
        if !covers || (column, row) == (range.start_column, range.start_row) {
            continue;
        }
        return cell_address(range.start_column, range.start_row).ok();
    }
    None
}

pub fn merge_sheet_cells(sheet: &mut Sheet, range: &str) -> Result<(), SpreadsheetError> {
    let normalized = normalize_merge_range(range)?;
    let parsed = parse_cell_range(&normalized)?;
    for existing in &sheet.merges {
        if existing.range == normalized {
            return Ok(());
        }
        if ranges_overlap(parsed, parse_cell_range(&existing.range)?) {
            return Err(SpreadsheetError::Conflict(format!(
                "spreadsheet merge range {normalized} overlaps {}",
                existing.range
            )));
        }
    }
    for row_offset in 0..parsed.height {
        for column_offset in 0..parsed.width {
            let address = cell_address(
                parsed.start_column + column_offset,
                parsed.start_row + row_offset,
            )?;
            sheet.ensure_address(&address);
        }
    }
    // Only the anchor of a merged block is ever drawn again, so content left
    // on the cells it covers would be invisible from here on — stored,
    // signed and exported, but unreachable and uneditable. Sheets discards
    // it for the same reason; keeping it is what makes a merge a data
    // hazard rather than a formatting choice. Formatting, validation and the
    // cell's conversation stay, because unmerging brings the cells back.
    for row_offset in 0..parsed.height {
        for column_offset in 0..parsed.width {
            if (row_offset, column_offset) == (0, 0) {
                continue;
            }
            let address = cell_address(
                parsed.start_column + column_offset,
                parsed.start_row + row_offset,
            )?;
            if let Some(cell) = sheet.cells.iter_mut().find(|cell| cell.address == address) {
                cell.user_kind = "empty".to_string();
                cell.user_value = String::new();
                cell.computed_kind = "empty".to_string();
                cell.computed_value = String::new();
                cell.display_value = String::new();
                cell.dependencies = Vec::new();
                cell.spill_source = None;
            }
        }
    }
    sheet.merges.push(SheetMerge {
        id: format!(
            "merge-{}",
            normalized.replace(':', "-").to_ascii_lowercase()
        ),
        range: normalized,
    });
    sheet
        .merges
        .sort_by(|left, right| left.range.cmp(&right.range));
    Ok(())
}

pub fn set_sheet_basic_filter(sheet: &mut Sheet, range: &str) -> Result<(), SpreadsheetError> {
    let normalized = normalize_cell_range(range)?;
    let parsed = parse_cell_range(&normalized)?;
    for row_offset in 0..parsed.height {
        for column_offset in 0..parsed.width {
            let address = cell_address(
                parsed.start_column + column_offset,
                parsed.start_row + row_offset,
            )?;
            sheet.ensure_address(&address);
        }
    }
    sheet.filters.clear();
    sheet.filters.push(SheetFilter {
        id: "basic-filter".to_string(),
        range: normalized,
        criteria: Vec::new(),
        sort_specs: Vec::new(),
    });
    Ok(())
}

pub fn set_sheet_basic_filter_options(
    sheet: &mut Sheet,
    criteria: Vec<SheetFilterCriterion>,
    sort_specs: Vec<SheetFilterSortSpec>,
) -> Result<(), SpreadsheetError> {
    let filter = sheet
        .filters
        .first_mut()
        .ok_or_else(|| SpreadsheetError::Format("sheet has no basic filter".to_string()))?;
    let range = parse_cell_range(&filter.range)?;
    for criterion in &criteria {
        criterion.validate_source(&range)?;
    }
    for sort_spec in &sort_specs {
        sort_spec.validate_source(&range)?;
    }
    filter.criteria = criteria
        .into_iter()
        .map(|mut criterion| {
            criterion.column = normalize_column_label(&criterion.column)?;
            criterion.condition = criterion.condition.trim().to_string();
            criterion.value = criterion.value.trim().to_string();
            Ok(criterion)
        })
        .collect::<Result<Vec<_>, SpreadsheetError>>()?;
    filter
        .criteria
        .sort_by(|left, right| left.column.cmp(&right.column));
    filter.sort_specs = sort_specs
        .into_iter()
        .map(|mut sort_spec| {
            sort_spec.column = normalize_column_label(&sort_spec.column)?;
            Ok(sort_spec)
        })
        .collect::<Result<Vec<_>, SpreadsheetError>>()?;
    filter
        .sort_specs
        .sort_by(|left, right| left.column.cmp(&right.column));
    Ok(())
}

pub fn add_sheet_protected_range(
    sheet: &mut Sheet,
    range: &str,
    description: &str,
    _warning_only: bool,
) -> Result<(), SpreadsheetError> {
    let normalized = normalize_cell_range(range)?;
    let parsed = parse_cell_range(&normalized)?;
    for row_offset in 0..parsed.height {
        for column_offset in 0..parsed.width {
            let address = cell_address(
                parsed.start_column + column_offset,
                parsed.start_row + row_offset,
            )?;
            sheet.ensure_address(&address);
        }
    }
    let description = normalize_protected_range_description(description.to_string());
    validate_protected_range_description(&description)?;
    if let Some(existing) = sheet
        .protected_ranges
        .iter_mut()
        .find(|protected_range| protected_range.range == normalized)
    {
        existing.description = description;
        existing.warning_only = true;
        return Ok(());
    }
    sheet.protected_ranges.push(SheetProtectedRange {
        id: format!(
            "protected-{}",
            normalized.replace(':', "-").to_ascii_lowercase()
        ),
        range: normalized,
        description,
        warning_only: true,
    });
    sheet
        .protected_ranges
        .sort_by(|left, right| left.range.cmp(&right.range));
    Ok(())
}

pub fn upsert_sheet_cell(sheet: &mut Sheet, cell: Cell) {
    sheet.ensure_address(&cell.address);
    if let Some(existing) = sheet
        .cells
        .iter_mut()
        .find(|existing| existing.address == cell.address)
    {
        *existing = cell;
    } else {
        sheet.cells.push(cell);
        sheet
            .cells
            .sort_by(|left, right| left.address.cmp(&right.address));
    }
}

fn apply_axis_edit(
    workbook: &mut SpreadsheetWorkbook,
    sheet_id: &str,
    axis: Axis,
    edit: AxisEdit,
) -> Result<(), SpreadsheetError> {
    rewrite_workbook_formulas(workbook, sheet_id, axis, edit);
    let sheet = workbook
        .sheets
        .iter_mut()
        .find(|sheet| sheet.id == sheet_id)
        .ok_or_else(|| SpreadsheetError::NotFound(format!("sheet {sheet_id} was not found")))?;
    // Cells.
    let mut cells = Vec::with_capacity(sheet.cells.len());
    for mut cell in std::mem::take(&mut sheet.cells) {
        if cell.spill_source.is_some() {
            cell.spill_source = None;
            if cell.user_kind == "empty" {
                cell.computed_kind = "empty".to_string();
                cell.computed_value = String::new();
                if cell.format == Default::default()
                    && cell.comments.is_empty()
                    && cell.validation.is_none()
                {
                    continue;
                }
            }
        }
        let Some(address) = shift_address(&cell.address, axis, edit) else {
            continue;
        };
        cell.address = address;
        cells.push(cell);
    }
    cells.sort_by(|left, right| left.address.cmp(&right.address));
    sheet.cells = cells;
    // Axis labels and metadata.
    let labels = match axis {
        Axis::Row => &mut sheet.rows,
        Axis::Column => &mut sheet.columns,
    };
    *labels = labels
        .iter()
        .filter_map(|label| shift_label(label, axis, edit))
        .collect();
    let sizes = match axis {
        Axis::Row => &mut sheet.row_heights,
        Axis::Column => &mut sheet.column_widths,
    };
    *sizes = sizes
        .iter()
        .filter_map(|(label, size)| shift_label(label, axis, edit).map(|label| (label, *size)))
        .collect::<BTreeMap<_, _>>();
    let hidden = match axis {
        Axis::Row => &mut sheet.hidden_rows,
        Axis::Column => &mut sheet.hidden_columns,
    };
    *hidden = hidden
        .iter()
        .filter_map(|label| shift_label(label, axis, edit))
        .collect();
    sort_axis_labels(sheet);
    // Frozen counts.
    let frozen = match axis {
        Axis::Row => &mut sheet.frozen_rows,
        Axis::Column => &mut sheet.frozen_columns,
    };
    match edit {
        AxisEdit::Insert { at, count } => {
            if *frozen > 0 && at <= *frozen {
                *frozen += count;
            }
        }
        AxisEdit::Delete { start, count } => {
            if *frozen >= start {
                let overlap = (*frozen).min(start + count - 1) - start + 1;
                *frozen -= overlap;
            }
        }
    }
    // Ranges.
    sheet.merges = std::mem::take(&mut sheet.merges)
        .into_iter()
        .filter_map(|mut merge| {
            let range = shift_range_text(&merge.range, axis, edit)?;
            if !range.contains(':') {
                return None;
            }
            merge.range = range;
            merge.id = format!(
                "merge-{}",
                merge.range.replace(':', "-").to_ascii_lowercase()
            );
            Some(merge)
        })
        .collect();
    sheet.filters = std::mem::take(&mut sheet.filters)
        .into_iter()
        .filter_map(|mut filter| {
            filter.range = shift_range_text(&filter.range, axis, edit)?;
            if axis == Axis::Column {
                filter.criteria = filter
                    .criteria
                    .into_iter()
                    .filter_map(|mut criterion| {
                        criterion.column = shift_label(&criterion.column, axis, edit)?;
                        Some(criterion)
                    })
                    .collect();
                filter.sort_specs = filter
                    .sort_specs
                    .into_iter()
                    .filter_map(|mut spec| {
                        spec.column = shift_label(&spec.column, axis, edit)?;
                        Some(spec)
                    })
                    .collect();
            }
            Some(filter)
        })
        .collect();
    sheet.protected_ranges = std::mem::take(&mut sheet.protected_ranges)
        .into_iter()
        .filter_map(|mut protected| {
            protected.range = shift_range_text(&protected.range, axis, edit)?;
            protected.id = format!(
                "protected-{}",
                protected.range.replace(':', "-").to_ascii_lowercase()
            );
            Some(protected)
        })
        .collect();
    workbook.named_ranges = std::mem::take(&mut workbook.named_ranges)
        .into_iter()
        .filter_map(|mut named| {
            if named.sheet_id != sheet_id {
                return Some(named);
            }
            named.range = shift_range_text(&named.range, axis, edit)?;
            Some(named)
        })
        .collect();
    Ok(())
}

/// Sorts the rows of a range by one column. Formulas keep working because
/// relative references move with their row.
pub fn sort_range(
    sheet: &mut Sheet,
    range: &str,
    column: u32,
    descending: bool,
    has_header: bool,
) -> Result<(), SpreadsheetError> {
    let parsed = parse_cell_range(range)?;
    if column < parsed.start_column || column >= parsed.start_column + parsed.width {
        return Err(SpreadsheetError::Format(format!(
            "sort column {} is outside range {range}",
            number_to_column(column).unwrap_or_default()
        )));
    }
    let first_row = if has_header {
        parsed.start_row + 1
    } else {
        parsed.start_row
    };
    let last_row = parsed.start_row + parsed.height - 1;
    if first_row >= last_row {
        return Ok(());
    }
    let mut rows: Vec<(u32, Vec<Cell>)> = (first_row..=last_row)
        .map(|row| {
            let cells = sheet
                .cells
                .iter()
                .filter(|cell| {
                    let (col, cell_row) = split_cell_address(&cell.address);
                    cell_row.parse::<u32>().ok() == Some(row)
                        && column_to_number(&col).is_some_and(|col| {
                            col >= parsed.start_column && col < parsed.start_column + parsed.width
                        })
                })
                .cloned()
                .collect();
            (row, cells)
        })
        .collect();
    let key_address = |row: u32| format!("{}{row}", number_to_column(column).unwrap_or_default());
    let key_of = |cells: &[Cell], row: u32| -> FormulaValue {
        cells
            .iter()
            .find(|cell| cell.address == key_address(row))
            .map(|cell| FormulaValue::from_projection(&cell.computed_kind, &cell.computed_value))
            .unwrap_or(FormulaValue::Blank)
    };
    rows.sort_by(|(left_row, left), (right_row, right)| {
        let left_key = key_of(left, *left_row);
        let right_key = key_of(right, *right_row);
        // Blanks always sort last.
        match (left_key.is_blank(), right_key.is_blank()) {
            (true, true) => return std::cmp::Ordering::Equal,
            (true, false) => return std::cmp::Ordering::Greater,
            (false, true) => return std::cmp::Ordering::Less,
            _ => {}
        }
        let ordering = compare_values(&left_key, &right_key).unwrap_or(std::cmp::Ordering::Equal);
        if descending {
            ordering.reverse()
        } else {
            ordering
        }
    });
    // Remove old cells in the sorted block, then reinsert at new rows.
    sheet.cells.retain(|cell| {
        let (col, cell_row) = split_cell_address(&cell.address);
        let in_rows = cell_row
            .parse::<u32>()
            .is_ok_and(|row| row >= first_row && row <= last_row);
        let in_cols = column_to_number(&col).is_some_and(|col| {
            col >= parsed.start_column && col < parsed.start_column + parsed.width
        });
        !(in_rows && in_cols)
    });
    for (target_index, (source_row, cells)) in rows.into_iter().enumerate() {
        let target_row = first_row + target_index as u32;
        let delta = target_row as i32 - source_row as i32;
        for mut cell in cells {
            let (col, _) = split_cell_address(&cell.address);
            cell.address = format!("{col}{target_row}");
            if cell.user_kind == "formula" && delta != 0 {
                cell.user_value = shift_formula_references(&cell.user_value, 0, delta);
            }
            cell.spill_source = None;
            sheet.cells.push(cell);
        }
    }
    sheet
        .cells
        .sort_by(|left, right| left.address.cmp(&right.address));
    Ok(())
}
