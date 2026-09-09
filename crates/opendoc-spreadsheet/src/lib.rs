//! Spreadsheet model, formula engine, structural edits, formatting, and
//! interchange adapters.

mod address;
#[allow(dead_code)]
mod format;
#[allow(dead_code)]
mod formula;
#[allow(dead_code)]
mod functions;
mod functions_legacy;
mod functions_legacy_tail;
mod google;
#[allow(dead_code)]
mod io;
mod lookup;
mod model;
mod recalc;
mod selection;
#[allow(dead_code)]
mod structure;
mod value;
mod workbook;

pub use address::{
    cell_axis_labels, normalize_cell_address, normalize_cell_range, normalize_column_label,
    normalize_merge_range, normalize_named_range_name, normalize_row_label, normalize_sheet_title,
    validate_canonical_column_label, validate_canonical_merge_range, validate_canonical_row_label,
};
pub use format::{parse_format_bool, validate_sheet_color};
pub use google::{export_google_sheets_workbook, import_google_sheets_workbook};
pub use model::{
    normalize_protected_range_description, normalize_sheet_id, validate_canonical_cell_address,
    validate_canonical_cell_range, validate_canonical_sheet_id, validate_filter_condition,
    validate_protected_range_description, Cell, CellComment, CellDependency, CellFormat,
    CellValidation, DeletedCellComment, NamedRange, Sheet, SheetAxis, SheetFilter,
    SheetFilterCriterion, SheetFilterSortSpec, SheetMerge, SheetProtectedRange,
};
pub use recalc::SpreadsheetEvaluationContext;
pub use selection::SpreadsheetSelectionSummary;
pub use value::{FormulaError, FormulaValue};
pub use workbook::SpreadsheetWorkbook;

use serde::{Deserialize, Serialize};

#[derive(Debug)]
pub enum SpreadsheetError {
    Conflict(String),
    Format(String),
    Import(String),
    Model(String),
    NotFound(String),
}

impl std::fmt::Display for SpreadsheetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for SpreadsheetError {}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SpreadsheetWarning {
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DeletedRowPayload {
    pub row_axis: SheetAxis,
    pub cells: Vec<Cell>,
    pub merges: Vec<SheetMerge>,
    pub filters: Vec<SheetFilter>,
    pub protected_ranges: Vec<SheetProtectedRange>,
    pub named_ranges: Vec<NamedRange>,
}

impl DeletedRowPayload {
    pub fn validate_source(&self, row: &str) -> Result<(), SpreadsheetError> {
        let row = address::normalize_row_label(row)?;
        if self.row_axis.label != row {
            return Err(SpreadsheetError::Format(format!(
                "deleted row payload axis {} does not match row {}",
                self.row_axis.label, row
            )));
        }
        if self.row_axis.id.trim().is_empty() {
            return Err(SpreadsheetError::Format(
                "deleted row payload axis id is empty".to_string(),
            ));
        }
        for cell in &self.cells {
            let (_, cell_row) = address::split_cell_address(&cell.address);
            cell.format.validate_source()?;
            if let Some(validation) = &cell.validation {
                validation.validate_source()?;
            }
            for dependency in &cell.dependencies {
                address::normalize_cell_address(dependency)?;
            }
            for comment in &cell.comments {
                comment.validate_source()?;
            }
            if cell_row != row {
                return Err(SpreadsheetError::Format(format!(
                    "deleted row payload cell {} is outside row {}",
                    cell.address, row
                )));
            }
        }
        for merge in &self.merges {
            merge.validate_source()?;
            if !address::range_contains_row(&merge.range, &row) {
                return Err(SpreadsheetError::Format(format!(
                    "deleted row payload merge {} is outside row {}",
                    merge.range, row
                )));
            }
        }
        for filter in &self.filters {
            filter.validate_source()?;
            if !address::range_contains_row(&filter.range, &row) {
                return Err(SpreadsheetError::Format(format!(
                    "deleted row payload filter {} is outside row {}",
                    filter.range, row
                )));
            }
        }
        for protected_range in &self.protected_ranges {
            protected_range.validate_source()?;
            if !address::range_contains_row(&protected_range.range, &row) {
                return Err(SpreadsheetError::Format(format!(
                    "deleted row payload protected range {} is outside row {}",
                    protected_range.range, row
                )));
            }
        }
        for named_range in &self.named_ranges {
            named_range.validate_source()?;
            if !address::range_contains_row(&named_range.range, &row) {
                return Err(SpreadsheetError::Format(format!(
                    "deleted row payload named range {} is outside row {}",
                    named_range.name, row
                )));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DeletedColumnPayload {
    pub column_axis: SheetAxis,
    pub cells: Vec<Cell>,
    pub merges: Vec<SheetMerge>,
    pub filters: Vec<SheetFilter>,
    pub protected_ranges: Vec<SheetProtectedRange>,
    pub named_ranges: Vec<NamedRange>,
}

impl DeletedColumnPayload {
    pub fn validate_source(&self, column: &str) -> Result<(), SpreadsheetError> {
        let column = address::normalize_column_label(column)?;
        if self.column_axis.label != column {
            return Err(SpreadsheetError::Format(format!(
                "deleted column payload axis {} does not match column {}",
                self.column_axis.label, column
            )));
        }
        if self.column_axis.id.trim().is_empty() {
            return Err(SpreadsheetError::Format(
                "deleted column payload axis id is empty".to_string(),
            ));
        }
        for cell in &self.cells {
            let (cell_column, _) = address::split_cell_address(&cell.address);
            cell.format.validate_source()?;
            if let Some(validation) = &cell.validation {
                validation.validate_source()?;
            }
            for dependency in &cell.dependencies {
                address::normalize_cell_address(dependency)?;
            }
            for comment in &cell.comments {
                comment.validate_source()?;
            }
            if cell_column != column {
                return Err(SpreadsheetError::Format(format!(
                    "deleted column payload cell {} is outside column {}",
                    cell.address, column
                )));
            }
        }
        for merge in &self.merges {
            merge.validate_source()?;
            if !address::range_contains_column(&merge.range, &column) {
                return Err(SpreadsheetError::Format(format!(
                    "deleted column payload merge {} is outside column {}",
                    merge.range, column
                )));
            }
        }
        for filter in &self.filters {
            filter.validate_source()?;
            if !address::range_contains_column(&filter.range, &column) {
                return Err(SpreadsheetError::Format(format!(
                    "deleted column payload filter {} is outside column {}",
                    filter.range, column
                )));
            }
        }
        for protected_range in &self.protected_ranges {
            protected_range.validate_source()?;
            if !address::range_contains_column(&protected_range.range, &column) {
                return Err(SpreadsheetError::Format(format!(
                    "deleted column payload protected range {} is outside column {}",
                    protected_range.range, column
                )));
            }
        }
        for named_range in &self.named_ranges {
            named_range.validate_source()?;
            if !address::range_contains_column(&named_range.range, &column) {
                return Err(SpreadsheetError::Format(format!(
                    "deleted column payload named range {} is outside column {}",
                    named_range.name, column
                )));
            }
        }
        Ok(())
    }
}

pub fn push_unique_warning(warnings: &mut Vec<SpreadsheetWarning>, code: &str, message: String) {
    if !warnings
        .iter()
        .any(|warning| warning.code == code && warning.message == message)
    {
        warnings.push(SpreadsheetWarning {
            code: code.to_string(),
            message,
        });
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(target_arch = "wasm32")]
fn now_ms() -> u64 {
    js_sys::Date::now() as u64
}

fn wildcard_text_matches(value: &[u8], pattern: &[u8]) -> bool {
    let mut value_index = 0usize;
    let mut pattern_index = 0usize;
    let mut star_pattern = None;
    let mut star_value = 0usize;
    while value_index < value.len() {
        if pattern_index < pattern.len() && pattern[pattern_index] == b'~' {
            pattern_index += 1;
            if pattern_index < pattern.len() && pattern[pattern_index] == value[value_index] {
                pattern_index += 1;
                value_index += 1;
                continue;
            }
        } else if pattern_index < pattern.len()
            && (pattern[pattern_index] == b'?' || pattern[pattern_index] == value[value_index])
        {
            pattern_index += 1;
            value_index += 1;
            continue;
        } else if pattern_index < pattern.len() && pattern[pattern_index] == b'*' {
            star_pattern = Some(pattern_index);
            pattern_index += 1;
            star_value = value_index;
            continue;
        }
        let Some(star) = star_pattern else {
            return false;
        };
        pattern_index = star + 1;
        star_value += 1;
        value_index = star_value;
    }
    while pattern_index < pattern.len() && pattern[pattern_index] == b'*' {
        pattern_index += 1;
    }
    pattern_index == pattern.len()
}

fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let year = year - if month <= 2 { 1 } else { 0 };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month = month as i32;
    let day = day as i32;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    (era * 146097 + day_of_era - 719468) as i64
}

fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let days = days + 719468;
    let era = if days >= 0 { days } else { days - 146096 } / 146097;
    let day_of_era = days - era * 146097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36524 - day_of_era / 146096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += if month <= 2 { 1 } else { 0 };
    (year as i32, month as u32, day as u32)
}
