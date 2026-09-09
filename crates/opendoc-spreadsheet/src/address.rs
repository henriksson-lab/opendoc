//! Spreadsheet address, range, and reference rewriting helpers.

use crate::SpreadsheetError;

use super::formula::is_unquoted_sheet_prefix_char;

pub fn normalize_cell_address(value: &str) -> Result<String, SpreadsheetError> {
    let value = value.trim().replace('$', "").to_ascii_uppercase();
    let (column, row) = split_cell_address(&value);
    if column.is_empty() || row.is_empty() || row.parse::<u32>().unwrap_or(0) == 0 {
        return Err(SpreadsheetError::Format(format!(
            "invalid cell address {value}"
        )));
    }
    Ok(format!("{column}{row}"))
}

pub fn normalize_cell_range(value: &str) -> Result<String, SpreadsheetError> {
    let (start, end) = value.split_once(':').unwrap_or((value, value));
    let start = normalize_cell_address(start)?;
    let end = normalize_cell_address(end)?;
    if start == end {
        Ok(start)
    } else {
        Ok(format!("{start}:{end}"))
    }
}

pub fn normalize_merge_range(value: &str) -> Result<String, SpreadsheetError> {
    let range = normalize_cell_range(value)?;
    let parsed = parse_cell_range(&range)?;
    if parsed.width == 1 && parsed.height == 1 {
        return Err(SpreadsheetError::Format(format!(
            "merge range {range} must contain at least two cells"
        )));
    }
    Ok(range)
}

pub fn validate_canonical_merge_range(label: &str, value: &str) -> Result<(), SpreadsheetError> {
    let normalized = normalize_merge_range(value)?;
    if value != normalized {
        return Err(SpreadsheetError::Format(format!(
            "{label} {value} is not canonical; expected {normalized}"
        )));
    }
    Ok(())
}

pub fn normalize_row_label(value: &str) -> Result<String, SpreadsheetError> {
    let value = value.trim();
    let row = value
        .parse::<u32>()
        .map_err(|_| SpreadsheetError::Format(format!("invalid row label {value}")))?;
    if row == 0 {
        return Err(SpreadsheetError::Format(format!(
            "invalid row label {value}"
        )));
    }
    Ok(row.to_string())
}

pub fn validate_canonical_row_label(value: &str) -> Result<(), SpreadsheetError> {
    let normalized = normalize_row_label(value)?;
    if value != normalized {
        return Err(SpreadsheetError::Format(format!(
            "spreadsheet row label {value} is not canonical; expected {normalized}"
        )));
    }
    Ok(())
}

pub fn normalize_column_label(value: &str) -> Result<String, SpreadsheetError> {
    let value = value.trim().to_ascii_uppercase();
    if value.is_empty() || column_to_number(&value).is_none() {
        return Err(SpreadsheetError::Format(format!(
            "invalid column label {value}"
        )));
    }
    Ok(value)
}

pub fn validate_canonical_column_label(value: &str) -> Result<(), SpreadsheetError> {
    let normalized = normalize_column_label(value)?;
    if value != normalized {
        return Err(SpreadsheetError::Format(format!(
            "spreadsheet column label {value} is not canonical; expected {normalized}"
        )));
    }
    Ok(())
}

pub fn normalize_named_range_name(value: &str) -> Result<String, SpreadsheetError> {
    let value = value.trim();
    if value.is_empty()
        || !value.chars().all(|ch| ch.is_ascii_alphabetic())
        || normalize_cell_address(value).is_ok()
    {
        return Err(SpreadsheetError::Format(format!(
            "invalid named range {value}"
        )));
    }
    Ok(value.to_ascii_uppercase())
}

pub fn normalize_sheet_title(value: impl Into<String>) -> String {
    let value = value.into();
    let title = value.trim();
    if title.is_empty() {
        "Sheet".to_string()
    } else {
        title.to_string()
    }
}

pub fn split_cell_address(value: &str) -> (String, String) {
    let column: String = value
        .chars()
        .take_while(|ch| ch.is_ascii_alphabetic())
        .collect();
    let row: String = value
        .chars()
        .skip_while(|ch| ch.is_ascii_alphabetic())
        .collect();
    (column, row)
}

pub fn cell_axis_labels(value: &str) -> Result<(String, String), SpreadsheetError> {
    let address = normalize_cell_address(value)?;
    Ok(split_cell_address(&address))
}

pub fn normalize_formula_sheet_prefix(prefix: &str) -> Result<String, String> {
    let prefix = prefix.trim();
    if prefix.starts_with('\'') {
        let Some(unquoted) = prefix
            .strip_prefix('\'')
            .and_then(|value| value.strip_suffix('\''))
        else {
            return Err("#REF!".to_string());
        };
        let unquoted = unquoted.replace("''", "'");
        if unquoted.trim().is_empty() {
            Err("#REF!".to_string())
        } else {
            Ok(unquoted)
        }
    } else {
        Ok(prefix.to_string())
    }
}

pub fn column_to_number(column: &str) -> Option<u32> {
    let mut value = 0u32;
    for ch in column.chars() {
        if !ch.is_ascii_alphabetic() {
            return None;
        }
        value = value
            .checked_mul(26)?
            .checked_add((ch.to_ascii_uppercase() as u8 - b'A' + 1) as u32)?;
    }
    Some(value)
}

pub fn number_to_column(mut value: u32) -> Option<String> {
    if value == 0 {
        return None;
    }
    let mut chars = Vec::new();
    while value > 0 {
        value -= 1;
        chars.push((b'A' + (value % 26) as u8) as char);
        value /= 26;
    }
    chars.reverse();
    Some(chars.into_iter().collect())
}

pub fn trim_number(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{}", value as i64)
    } else {
        value.to_string()
    }
}

#[derive(Clone, Copy)]
pub struct CellRange {
    pub start_column: u32,
    pub start_row: u32,
    pub width: u32,
    pub height: u32,
}

pub fn parse_cell_range(value: &str) -> Result<CellRange, SpreadsheetError> {
    let (start, end) = value.split_once(':').unwrap_or((value, value));
    let start = normalize_cell_address(start)?;
    let end = normalize_cell_address(end)?;
    let (start_column, start_row) = parse_cell_position(&start)?;
    let (end_column, end_row) = parse_cell_position(&end)?;
    let first_column = start_column.min(end_column);
    let last_column = start_column.max(end_column);
    let first_row = start_row.min(end_row);
    let last_row = start_row.max(end_row);
    Ok(CellRange {
        start_column: first_column,
        start_row: first_row,
        width: last_column - first_column + 1,
        height: last_row - first_row + 1,
    })
}

pub fn parse_cell_position(value: &str) -> Result<(u32, u32), SpreadsheetError> {
    let address = normalize_cell_address(value)?;
    let (column, row) = split_cell_address(&address);
    let column = column_to_number(&column)
        .ok_or_else(|| SpreadsheetError::Format(format!("invalid cell address {value}")))?;
    let row = row
        .parse::<u32>()
        .map_err(|_| SpreadsheetError::Format(format!("invalid cell address {value}")))?;
    Ok((column, row))
}

pub fn cell_address(column: u32, row: u32) -> Result<String, SpreadsheetError> {
    let column = number_to_column(column)
        .ok_or_else(|| SpreadsheetError::Format(format!("invalid cell column {column}")))?;
    Ok(format!("{column}{row}"))
}

pub fn range_contains_row(range: &str, row: &str) -> bool {
    let Ok(parsed) = parse_cell_range(range) else {
        return false;
    };
    let Ok(row) = row.parse::<u32>() else {
        return false;
    };
    row >= parsed.start_row && row < parsed.start_row + parsed.height
}

pub fn range_contains_column(range: &str, column: &str) -> bool {
    let Ok(parsed) = parse_cell_range(range) else {
        return false;
    };
    let Some(column) = column_to_number(column) else {
        return false;
    };
    range_contains_column_number(&parsed, column)
}

pub fn range_contains_column_number(range: &CellRange, column: u32) -> bool {
    column >= range.start_column && column < range.start_column + range.width
}

pub fn ranges_overlap(left: CellRange, right: CellRange) -> bool {
    let left_end_column = left.start_column + left.width - 1;
    let right_end_column = right.start_column + right.width - 1;
    let left_end_row = left.start_row + left.height - 1;
    let right_end_row = right.start_row + right.height - 1;
    left.start_column <= right_end_column
        && right.start_column <= left_end_column
        && left.start_row <= right_end_row
        && right.start_row <= left_end_row
}

pub fn range_contains_range(outer: CellRange, inner: CellRange) -> bool {
    let outer_end_column = outer.start_column + outer.width - 1;
    let outer_end_row = outer.start_row + outer.height - 1;
    let inner_end_column = inner.start_column + inner.width - 1;
    let inner_end_row = inner.start_row + inner.height - 1;
    inner.start_column >= outer.start_column
        && inner.start_row >= outer.start_row
        && inner_end_column <= outer_end_column
        && inner_end_row <= outer_end_row
}

pub fn shift_cell_range(
    range: CellRange,
    column_delta: i32,
    row_delta: i32,
) -> Result<String, SpreadsheetError> {
    let start_column = range.start_column as i32 + column_delta;
    let start_row = range.start_row as i32 + row_delta;
    let end_column = range.start_column as i32 + range.width as i32 - 1 + column_delta;
    let end_row = range.start_row as i32 + range.height as i32 - 1 + row_delta;
    if start_column < 1 || start_row < 1 || end_column < 1 || end_row < 1 {
        return Err(SpreadsheetError::Format(
            "shifted spreadsheet range moved outside the sheet".to_string(),
        ));
    }
    let start = cell_address(start_column as u32, start_row as u32)?;
    let end = cell_address(end_column as u32, end_row as u32)?;
    normalize_cell_range(&format!("{start}:{end}"))
}

pub fn shift_formula_references(formula: &str, column_delta: i32, row_delta: i32) -> String {
    let mut out = String::new();
    let mut index = 0;
    let chars = formula.chars().collect::<Vec<_>>();
    while index < chars.len() {
        if chars[index] == '"' {
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
            out.extend(chars[start..index].iter());
            continue;
        }
        if chars[index] == '\'' {
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
            if chars.get(index) == Some(&'!') {
                index += 1;
            }
            out.extend(chars[start..index].iter());
            continue;
        }
        if chars[index].is_ascii_alphabetic() || chars[index] == '$' {
            let start = index;
            if chars[index] == '$' {
                index += 1;
            }
            while index < chars.len() && chars[index].is_ascii_alphabetic() {
                index += 1;
            }
            if chars.get(index) == Some(&'$') {
                index += 1;
            }
            let row_start = index;
            while index < chars.len() && chars[index].is_ascii_digit() {
                index += 1;
            }
            if row_start != index {
                let token = chars[start..index].iter().collect::<String>();
                if chars.get(index) == Some(&'!') {
                    out.push_str(&token);
                } else if let Some(shifted) = shift_cell_reference(&token, column_delta, row_delta)
                {
                    out.push_str(&shifted);
                } else {
                    out.push_str(&token);
                }
                continue;
            }
            out.extend(chars[start..index].iter());
            continue;
        }
        out.push(chars[index]);
        index += 1;
    }
    out
}

pub fn rewrite_formula_sheet_title_references(
    formula: &str,
    old_title: &str,
    new_title: &str,
) -> String {
    let old_title = old_title.trim();
    if old_title.is_empty() || old_title == new_title.trim() {
        return formula.to_string();
    }
    let mut out = String::new();
    let mut index = 0;
    let chars = formula.chars().collect::<Vec<_>>();
    while index < chars.len() {
        if chars[index] == '"' {
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
            out.extend(chars[start..index].iter());
            continue;
        }
        if chars[index] == '\'' {
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
            let prefix = chars[start..index].iter().collect::<String>();
            if chars.get(index) == Some(&'!') {
                index += 1;
                if normalize_formula_sheet_prefix(&prefix).as_deref() == Ok(old_title) {
                    out.push_str(&formula_sheet_title_prefix(new_title));
                    out.push('!');
                } else {
                    out.push_str(&prefix);
                    out.push('!');
                }
            } else {
                out.push_str(&prefix);
            }
            continue;
        }
        if chars[index].is_ascii_alphabetic() {
            let start = index;
            while index < chars.len() && is_unquoted_sheet_prefix_char(chars[index]) {
                index += 1;
            }
            if chars.get(index) == Some(&'!') {
                let prefix = chars[start..index].iter().collect::<String>();
                index += 1;
                if prefix == old_title {
                    out.push_str(&formula_sheet_title_prefix(new_title));
                } else {
                    out.push_str(&prefix);
                }
                out.push('!');
                continue;
            }
            out.extend(chars[start..index].iter());
            continue;
        }
        out.push(chars[index]);
        index += 1;
    }
    out
}

pub fn formula_sheet_title_prefix(title: &str) -> String {
    let title = title.trim();
    if title
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_alphabetic())
        && title.chars().all(is_unquoted_sheet_prefix_char)
    {
        title.to_string()
    } else {
        format!("'{}'", title.replace('\'', "''"))
    }
}

fn shift_cell_reference(reference: &str, column_delta: i32, row_delta: i32) -> Option<String> {
    let mut chars = reference.chars().peekable();
    let column_absolute = chars.peek() == Some(&'$');
    if column_absolute {
        chars.next();
    }
    let mut column = String::new();
    while chars.peek().is_some_and(|ch| ch.is_ascii_alphabetic()) {
        column.push(chars.next()?);
    }
    let row_absolute = chars.peek() == Some(&'$');
    if row_absolute {
        chars.next();
    }
    let row = chars.collect::<String>();
    if column.is_empty() || row.is_empty() {
        return None;
    }
    let column = column_to_number(&column)? as i32 + if column_absolute { 0 } else { column_delta };
    let row = row.parse::<i32>().ok()? + if row_absolute { 0 } else { row_delta };
    if column < 1 || row < 1 {
        return None;
    }
    let column = number_to_column(column as u32)?;
    let column_prefix = if column_absolute { "$" } else { "" };
    let row_prefix = if row_absolute { "$" } else { "" };
    Some(format!("{column_prefix}{column}{row_prefix}{row}"))
}
