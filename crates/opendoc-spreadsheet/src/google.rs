//! Google Sheets JSON import/export adapter for the spreadsheet model.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Value};

use super::address::{
    cell_address, column_to_number, normalize_cell_address, normalize_cell_range,
    normalize_column_label, normalize_merge_range, normalize_named_range_name, number_to_column,
    parse_cell_range, ranges_overlap, split_cell_address, CellRange,
};
use super::format::{export_sheet_color, import_sheet_rgb, trim_sheet_number};
use super::model::validate_spreadsheet_cell_source;
use super::structure::{
    add_sheet_protected_range, merge_sheet_cells, set_sheet_basic_filter,
    set_sheet_basic_filter_options, upsert_sheet_cell,
};
use crate::{
    push_unique_warning, Cell, CellComment, CellFormat, CellValidation, NamedRange, Sheet,
    SheetFilter, SheetFilterCriterion, SheetFilterSortSpec, SheetProtectedRange, SpreadsheetError,
    SpreadsheetWarning, SpreadsheetWorkbook,
};

const GOOGLE_SHEETS_IMPORT_DEFAULT_ROWS: u64 = 3;
const GOOGLE_SHEETS_IMPORT_DEFAULT_COLUMNS: u64 = 2;
const GOOGLE_SHEETS_IMPORT_MAX_ROWS: u64 = 10_000;
const GOOGLE_SHEETS_IMPORT_MAX_COLUMNS: u64 = 1_000;

pub struct ImportedGoogleSheetsWorkbook {
    pub workbook: SpreadsheetWorkbook,
    pub warnings: Vec<SpreadsheetWarning>,
}

pub fn import_google_sheets_workbook(
    json_text: &str,
) -> Result<ImportedGoogleSheetsWorkbook, SpreadsheetError> {
    let value: Value =
        serde_json::from_str(json_text).map_err(|err| SpreadsheetError::Import(err.to_string()))?;
    reject_google_sheets_high_risk(&value)?;
    let properties = google_optional_object(&value, "properties")?.unwrap_or(&Value::Null);
    let title = google_optional_string(properties, "title")?
        .unwrap_or("Imported Sheet")
        .to_string();
    let locale = google_optional_string(properties, "locale")?
        .unwrap_or("en-US")
        .to_string();
    let timezone = google_optional_string(properties, "timeZone")?
        .unwrap_or("UTC")
        .to_string();
    let sheets = value
        .get("sheets")
        .and_then(Value::as_array)
        .ok_or_else(|| SpreadsheetError::Import("missing sheets array".to_string()))?;
    if sheets.is_empty() {
        return Err(SpreadsheetError::Import(
            "Google Sheets import had no sheets".to_string(),
        ));
    }

    let mut workbook = SpreadsheetWorkbook::empty("Imported Sheet");
    let mut warnings = Vec::new();
    workbook.set_metadata(title, locale, timezone)?;
    let mut sheet_id_map = BTreeMap::new();
    let mut google_sheet_ids = BTreeSet::new();
    let mut sheet_titles = BTreeSet::new();

    for (index, sheet_value) in sheets.iter().enumerate() {
        let properties = google_optional_object(sheet_value, "properties")?.unwrap_or(&Value::Null);
        let app_sheet_id = format!("sheet-{}", index + 1);
        if let Some(google_sheet_id) = google_optional_non_negative_i64(properties, "sheetId")? {
            if !google_sheet_ids.insert(google_sheet_id) {
                return Err(SpreadsheetError::Import(format!(
                    "duplicate Google Sheets sheetId {google_sheet_id}"
                )));
            }
            sheet_id_map.insert(google_sheet_id, app_sheet_id.clone());
        }
        let title = google_optional_string(properties, "title")?
            .map(import_google_sheet_title)
            .transpose()?
            .unwrap_or_else(|| format!("Sheet{}", index + 1));
        if !sheet_titles.insert(title.clone()) {
            return Err(SpreadsheetError::Import(format!(
                "duplicate Google Sheets title {title}"
            )));
        }
        let grid = google_optional_object(properties, "gridProperties")?.unwrap_or(&Value::Null);
        let row_count = import_google_grid_dimension(
            grid,
            "rowCount",
            GOOGLE_SHEETS_IMPORT_DEFAULT_ROWS,
            GOOGLE_SHEETS_IMPORT_MAX_ROWS,
        )?;
        let column_count = import_google_grid_dimension(
            grid,
            "columnCount",
            GOOGLE_SHEETS_IMPORT_DEFAULT_COLUMNS,
            GOOGLE_SHEETS_IMPORT_MAX_COLUMNS,
        )?;
        let frozen_rows = import_google_frozen_dimension(grid, "frozenRowCount", row_count)?;
        let frozen_columns =
            import_google_frozen_dimension(grid, "frozenColumnCount", column_count)?;
        let mut sheet = super::io::blank_sheet(&app_sheet_id, &title, row_count, column_count);
        sheet.frozen_rows = frozen_rows;
        sheet.frozen_columns = frozen_columns;
        import_google_sheets_grid_data(sheet_value, &mut sheet)?;
        import_google_sheets_merges(sheet_value, &mut sheet)?;
        import_google_sheets_basic_filter(sheet_value, &mut sheet)?;
        import_google_sheets_protected_ranges(sheet_value, &mut sheet, &mut warnings)?;
        sheet.ensure_axis_metadata();
        workbook.sheets.push(sheet);
    }

    if let Some(named_ranges) = google_optional_array(&value, "namedRanges")? {
        let mut named_range_ids = BTreeSet::new();
        let mut named_range_names = BTreeSet::new();
        for named_range in named_ranges {
            if let Some(named_range_id) = google_optional_string(named_range, "namedRangeId")? {
                if named_range_id.trim().is_empty() {
                    return Err(SpreadsheetError::Import(
                        "named range id is empty".to_string(),
                    ));
                }
                if !named_range_ids.insert(named_range_id.to_string()) {
                    return Err(SpreadsheetError::Import(format!(
                        "duplicate Google Sheets namedRangeId {named_range_id}"
                    )));
                }
            }
            let name = google_required_string(named_range, "name", "named range missing name")?;
            let name = normalize_named_range_name(name)?;
            if !named_range_names.insert(name.clone()) {
                return Err(SpreadsheetError::Import(format!(
                    "duplicate Google Sheets named range {name}"
                )));
            }
            let google_range =
                google_required_object(named_range, "range", "named range missing range")?;
            let google_sheet_id = google_required_non_negative_i64(
                google_range,
                "sheetId",
                "named range missing sheetId",
            )?;
            let sheet_id = sheet_id_map.get(&google_sheet_id).cloned().ok_or_else(|| {
                SpreadsheetError::Import(format!(
                    "named range references unknown sheetId {google_sheet_id}"
                ))
            })?;
            let range = google_grid_range_to_a1(google_range)?;
            workbook
                .add_named_range(&sheet_id, &name, &range)
                .ok_or_else(|| {
                    SpreadsheetError::Import(format!("named range sheet {sheet_id} missing"))
                })??;
        }
    }

    Ok(ImportedGoogleSheetsWorkbook { workbook, warnings })
}

fn import_google_sheet_title(value: &str) -> Result<String, SpreadsheetError> {
    let title = value.trim();
    if title.is_empty() {
        return Err(SpreadsheetError::Import(
            "Google Sheets title is empty".to_string(),
        ));
    }
    Ok(title.to_string())
}

fn import_google_grid_dimension(
    grid: &Value,
    key: &str,
    default: u64,
    max: u64,
) -> Result<u32, SpreadsheetError> {
    let value = google_optional_u64(grid, key)?.unwrap_or(default);
    if value == 0 {
        return Err(SpreadsheetError::Import(format!(
            "gridProperties {key} must be at least 1"
        )));
    }
    if value > max {
        return Err(SpreadsheetError::Import(format!(
            "gridProperties {key} exceeds supported limit {max}"
        )));
    }
    u32::try_from(value).map_err(|_| {
        SpreadsheetError::Import(format!("gridProperties {key} exceeds supported u32 range"))
    })
}

fn import_google_frozen_dimension(
    grid: &Value,
    key: &str,
    axis_count: u32,
) -> Result<u32, SpreadsheetError> {
    let value = google_optional_u64(grid, key)?.unwrap_or(0);
    if value > axis_count as u64 {
        return Err(SpreadsheetError::Import(format!(
            "gridProperties {key} exceeds visible grid size"
        )));
    }
    u32::try_from(value).map_err(|_| {
        SpreadsheetError::Import(format!("gridProperties {key} exceeds supported u32 range"))
    })
}

fn google_optional_u64(value: &Value, key: &str) -> Result<Option<u64>, SpreadsheetError> {
    match value.get(key) {
        Some(Value::Null) | None => Ok(None),
        Some(field) => field.as_u64().map(Some).ok_or_else(|| {
            SpreadsheetError::Import(format!(
                "gridProperties {key} must be a non-negative integer"
            ))
        }),
    }
}

fn google_optional_array<'a>(
    value: &'a Value,
    key: &str,
) -> Result<Option<&'a Vec<Value>>, SpreadsheetError> {
    match value.get(key) {
        Some(Value::Null) | None => Ok(None),
        Some(field) => field
            .as_array()
            .map(Some)
            .ok_or_else(|| SpreadsheetError::Import(format!("{key} must be an array"))),
    }
}

fn google_optional_object<'a>(
    value: &'a Value,
    key: &str,
) -> Result<Option<&'a Value>, SpreadsheetError> {
    match value.get(key) {
        Some(Value::Null) | None => Ok(None),
        Some(field) if field.is_object() => Ok(Some(field)),
        Some(_) => Err(SpreadsheetError::Import(format!("{key} must be an object"))),
    }
}

fn google_expect_object(value: &Value, label: &str) -> Result<(), SpreadsheetError> {
    if value.is_object() {
        Ok(())
    } else {
        Err(SpreadsheetError::Import(format!(
            "{label} must be an object"
        )))
    }
}

fn google_required_object<'a>(
    value: &'a Value,
    key: &str,
    missing: &str,
) -> Result<&'a Value, SpreadsheetError> {
    google_optional_object(value, key)?.ok_or_else(|| SpreadsheetError::Import(missing.to_string()))
}

fn google_optional_string<'a>(
    value: &'a Value,
    key: &str,
) -> Result<Option<&'a str>, SpreadsheetError> {
    match value.get(key) {
        Some(Value::Null) | None => Ok(None),
        Some(field) => field
            .as_str()
            .map(Some)
            .ok_or_else(|| SpreadsheetError::Import(format!("{key} must be a string"))),
    }
}

fn google_required_string<'a>(
    value: &'a Value,
    key: &str,
    missing: &str,
) -> Result<&'a str, SpreadsheetError> {
    google_optional_string(value, key)?.ok_or_else(|| SpreadsheetError::Import(missing.to_string()))
}

fn google_optional_non_negative_i64(
    value: &Value,
    key: &str,
) -> Result<Option<i64>, SpreadsheetError> {
    match value.get(key) {
        Some(Value::Null) | None => Ok(None),
        Some(field) => {
            let number = field.as_i64().ok_or_else(|| {
                SpreadsheetError::Import(format!("{key} must be a non-negative integer"))
            })?;
            if number < 0 {
                return Err(SpreadsheetError::Import(format!(
                    "{key} must be a non-negative integer"
                )));
            }
            Ok(Some(number))
        }
    }
}

fn google_required_non_negative_i64(
    value: &Value,
    key: &str,
    missing: &str,
) -> Result<i64, SpreadsheetError> {
    google_optional_non_negative_i64(value, key)?
        .ok_or_else(|| SpreadsheetError::Import(missing.to_string()))
}

fn google_optional_bool(value: &Value, key: &str) -> Result<Option<bool>, SpreadsheetError> {
    match value.get(key) {
        Some(Value::Null) | None => Ok(None),
        Some(field) => field
            .as_bool()
            .map(Some)
            .ok_or_else(|| SpreadsheetError::Import(format!("{key} must be a boolean"))),
    }
}

fn reject_google_sheets_high_risk(value: &Value) -> Result<(), SpreadsheetError> {
    for key in ["charts", "pivotTables", "dataSourceSheetProperties"] {
        if value.pointer(&format!("/{key}")).is_some() {
            return Err(SpreadsheetError::Import(format!(
                "unsupported high-risk Google Sheets field {key}"
            )));
        }
    }
    for sheet in value
        .get("sheets")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        for key in ["charts", "pivotTables", "filterViews"] {
            if sheet.get(key).is_some() {
                return Err(SpreadsheetError::Import(format!(
                    "unsupported high-risk Google Sheets sheet field {key}"
                )));
            }
        }
    }
    Ok(())
}

fn import_google_sheets_grid_data(
    sheet_value: &Value,
    sheet: &mut Sheet,
) -> Result<(), SpreadsheetError> {
    let mut imported_cells = BTreeSet::new();
    let Some(data_ranges) = google_optional_array(sheet_value, "data")? else {
        return Ok(());
    };
    for grid_data in data_ranges {
        let start_row = google_grid_data_start(grid_data, "startRow")?;
        let start_column = google_grid_data_start(grid_data, "startColumn")?;
        let Some(row_data) = google_optional_array(grid_data, "rowData")? else {
            continue;
        };
        for (row_offset, row) in row_data.iter().enumerate() {
            let values = google_optional_array(row, "values")?
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            for (column_offset, cell_value) in values.iter().enumerate() {
                if cell_value
                    .as_object()
                    .is_some_and(|object| object.is_empty())
                {
                    continue;
                }
                let address =
                    google_grid_data_address(start_column, start_row, column_offset, row_offset)?;
                if !imported_cells.insert(address.clone()) {
                    return Err(SpreadsheetError::Import(format!(
                        "duplicate Google Sheets cell {address}"
                    )));
                }
                let mut cell = import_google_sheets_cell(&address, cell_value)?;
                cell.address = address;
                upsert_sheet_cell(sheet, cell);
            }
        }
    }
    Ok(())
}

fn google_grid_data_start(value: &Value, key: &str) -> Result<u64, SpreadsheetError> {
    match value.get(key) {
        None => Ok(0),
        Some(value) => value.as_u64().ok_or_else(|| {
            SpreadsheetError::Import(format!("grid data {key} must be a non-negative integer"))
        }),
    }
}

fn google_grid_data_address(
    start_column: u64,
    start_row: u64,
    column_offset: usize,
    row_offset: usize,
) -> Result<String, SpreadsheetError> {
    let column_offset = u64::try_from(column_offset).map_err(|_| {
        SpreadsheetError::Import("grid data column offset is too large".to_string())
    })?;
    let row_offset = u64::try_from(row_offset)
        .map_err(|_| SpreadsheetError::Import("grid data row offset is too large".to_string()))?;
    let column = start_column
        .checked_add(column_offset)
        .and_then(|value| value.checked_add(1))
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| {
            SpreadsheetError::Import("grid data startColumn is too large".to_string())
        })?;
    let row = start_row
        .checked_add(row_offset)
        .and_then(|value| value.checked_add(1))
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| SpreadsheetError::Import("grid data startRow is too large".to_string()))?;
    cell_address(column, row)
}

fn import_google_sheets_merges(
    sheet_value: &Value,
    sheet: &mut Sheet,
) -> Result<(), SpreadsheetError> {
    let mut merge_ranges = BTreeSet::new();
    let Some(merges) = google_optional_array(sheet_value, "merges")? else {
        return Ok(());
    };
    for merge in merges {
        google_expect_object(merge, "merge")?;
        let range = google_grid_range_to_a1(merge)?;
        let normalized = normalize_merge_range(&range)?;
        if !merge_ranges.insert(normalized.clone()) {
            return Err(SpreadsheetError::Import(format!(
                "duplicate Google Sheets merge range {normalized}"
            )));
        }
        merge_sheet_cells(sheet, &range)?;
    }
    Ok(())
}

fn import_google_sheets_basic_filter(
    sheet_value: &Value,
    sheet: &mut Sheet,
) -> Result<(), SpreadsheetError> {
    if let Some(filter) = sheet_value.get("basicFilter") {
        if filter.is_null() {
            return Ok(());
        }
        google_expect_object(filter, "basicFilter")?;
        let range_value = google_required_object(filter, "range", "basicFilter missing range")?;
        let range = google_grid_range_to_a1(range_value)?;
        set_sheet_basic_filter(sheet, &range)?;
        import_google_sheets_basic_filter_options(filter, sheet)?;
    }
    Ok(())
}

fn import_google_sheets_basic_filter_options(
    filter: &Value,
    sheet: &mut Sheet,
) -> Result<(), SpreadsheetError> {
    let range = sheet
        .filters
        .first()
        .map(|filter| parse_cell_range(&filter.range))
        .transpose()?
        .ok_or_else(|| {
            SpreadsheetError::Import("basicFilter missing normalized range".to_string())
        })?;
    let mut criteria = Vec::new();
    if let Some(criteria_value) = google_optional_object(filter, "criteria")? {
        for (offset, criterion_value) in criteria_value.as_object().ok_or_else(|| {
            SpreadsheetError::Import("basicFilter criteria must be an object".to_string())
        })? {
            let offset = offset.parse::<u32>().map_err(|_| {
                SpreadsheetError::Import(format!(
                    "basicFilter criteria key {offset} is not a column offset"
                ))
            })?;
            let column = cell_address(range.start_column + offset, 1)?
                .trim_end_matches('1')
                .to_string();
            let condition_value = google_required_object(
                criterion_value,
                "condition",
                "basicFilter criterion missing condition",
            )?;
            let google_condition =
                google_required_string(condition_value, "type", "filter condition")?;
            let condition = import_google_filter_condition(google_condition)?;
            let values = google_optional_array(condition_value, "values")?.ok_or_else(|| {
                SpreadsheetError::Import("filter condition missing values".to_string())
            })?;
            let Some(first) = values.first() else {
                return Err(SpreadsheetError::Import(
                    "filter condition has no userEnteredValue".to_string(),
                ));
            };
            let value = google_required_string(first, "userEnteredValue", "filter value")?;
            criteria.push(SheetFilterCriterion {
                column,
                condition,
                value: value.to_string(),
            });
        }
    }
    let mut sort_specs = Vec::new();
    if let Some(sort_values) = google_optional_array(filter, "sortSpecs")? {
        for sort_value in sort_values {
            google_expect_object(sort_value, "sortSpec")?;
            let index = google_range_required_u64(sort_value, "dimensionIndex")?;
            let column = cell_address(
                u32::try_from(index + 1).map_err(|_| {
                    SpreadsheetError::Import("sortSpec dimensionIndex is too large".to_string())
                })?,
                1,
            )?
            .trim_end_matches('1')
            .to_string();
            let order = google_optional_string(sort_value, "sortOrder")?.unwrap_or("ASCENDING");
            let descending = match order {
                "ASCENDING" => false,
                "DESCENDING" => true,
                other => {
                    return Err(SpreadsheetError::Import(format!(
                        "unsupported sort order {other}"
                    )));
                }
            };
            sort_specs.push(SheetFilterSortSpec { column, descending });
        }
    }
    set_sheet_basic_filter_options(sheet, criteria, sort_specs)
}

fn import_google_filter_condition(value: &str) -> Result<String, SpreadsheetError> {
    match value {
        "TEXT_CONTAINS" => Ok("text_contains".to_string()),
        "TEXT_EQ" => Ok("text_equals".to_string()),
        "NUMBER_GREATER" => Ok("number_greater".to_string()),
        "NUMBER_LESS" => Ok("number_less".to_string()),
        "NUMBER_EQ" => Ok("number_equal".to_string()),
        other => Err(SpreadsheetError::Import(format!(
            "unsupported basicFilter condition {other}"
        ))),
    }
}

fn export_google_filter_condition(value: &str) -> Result<&'static str, SpreadsheetError> {
    match value {
        "text_contains" => Ok("TEXT_CONTAINS"),
        "text_equals" => Ok("TEXT_EQ"),
        "number_greater" => Ok("NUMBER_GREATER"),
        "number_less" => Ok("NUMBER_LESS"),
        "number_equal" => Ok("NUMBER_EQ"),
        other => Err(SpreadsheetError::Import(format!(
            "unsupported filter condition {other}"
        ))),
    }
}

fn import_google_sheets_protected_ranges(
    sheet_value: &Value,
    sheet: &mut Sheet,
    warnings: &mut Vec<SpreadsheetWarning>,
) -> Result<(), SpreadsheetError> {
    let mut protected_range_ids = BTreeSet::new();
    let mut protected_ranges = BTreeSet::new();
    let Some(ranges) = google_optional_array(sheet_value, "protectedRanges")? else {
        return Ok(());
    };
    for protected_range in ranges {
        google_expect_object(protected_range, "protected range")?;
        if let Some(protected_range_id) =
            google_optional_non_negative_i64(protected_range, "protectedRangeId")?
        {
            if !protected_range_ids.insert(protected_range_id) {
                return Err(SpreadsheetError::Import(format!(
                    "duplicate Google Sheets protectedRangeId {protected_range_id}"
                )));
            }
        }
        let range_value =
            google_required_object(protected_range, "range", "protected range missing range")?;
        let range = google_grid_range_to_a1(range_value)?;
        let normalized = normalize_cell_range(&range)?;
        if !protected_ranges.insert(normalized.clone()) {
            return Err(SpreadsheetError::Import(format!(
                "duplicate Google Sheets protected range {normalized}"
            )));
        }
        let description = google_required_string(
            protected_range,
            "description",
            "protected range missing description",
        )?;
        let warning_only = google_optional_bool(protected_range, "warningOnly")?.unwrap_or(true);
        if !warning_only {
            push_unique_warning(
                warnings,
                "protected-range-warning-only",
                format!(
                    "protected range {}!{} was downgraded to warning-only because v0 does not enforce spreadsheet permissions",
                    sheet.id, normalized
                ),
            );
        }
        add_sheet_protected_range(sheet, &range, description, warning_only)?;
    }
    Ok(())
}

fn import_google_sheets_cell(address: &str, value: &Value) -> Result<Cell, SpreadsheetError> {
    reject_google_sheets_cell_high_risk(value)?;
    let user_value = value.get("userEnteredValue").unwrap_or(&Value::Null);
    let mut cell = import_google_sheets_user_entered_value(address, user_value)?;
    if let Some(format) = google_optional_object(value, "userEnteredFormat")? {
        cell.format = import_google_sheets_format(format)?;
    }
    if let Some(validation) = google_optional_object(value, "dataValidation")? {
        cell.validation = Some(import_google_sheets_validation(validation)?);
    }
    if let Some(note) = google_optional_string(value, "note")? {
        if !note.trim().is_empty() {
            cell.comments.push(CellComment {
                id: format!("note-{}", address.to_ascii_lowercase()),
                author: "Google Sheets note".to_string(),
                body: note.to_string(),
                deleted: false,
            });
        }
    }
    if let Some(comments) = google_optional_array(value, "opendocCellComments")? {
        let mut comment_ids = cell
            .comments
            .iter()
            .map(|comment| comment.id.clone())
            .collect::<BTreeSet<_>>();
        for comment in comments {
            let comment = import_google_sheets_cell_comment(comment)?;
            if !comment_ids.insert(comment.id.clone()) {
                if cell.comments.iter().any(|existing| {
                    existing.id == comment.id
                        && existing.author == comment.author
                        && existing.body == comment.body
                        && existing.deleted == comment.deleted
                }) {
                    continue;
                }
                return Err(SpreadsheetError::Import(format!(
                    "duplicate Google Sheets cell comment id {}",
                    comment.id
                )));
            }
            cell.comments.push(comment);
        }
        cell.comments.sort_by(|left, right| left.id.cmp(&right.id));
    }
    Ok(cell)
}

fn import_google_sheets_user_entered_value(
    address: &str,
    value: &Value,
) -> Result<Cell, SpreadsheetError> {
    if value.is_null() {
        return Ok(Cell::new(address, "empty", ""));
    }
    if !value.is_object() {
        return Err(SpreadsheetError::Import(
            "userEnteredValue must be an object".to_string(),
        ));
    }
    let mut seen = Vec::new();
    for key in ["formulaValue", "numberValue", "boolValue", "stringValue"] {
        if value.get(key).is_some() {
            seen.push(key);
        }
    }
    if seen.len() > 1 {
        return Err(SpreadsheetError::Import(format!(
            "userEnteredValue has multiple value kinds: {}",
            seen.join(", ")
        )));
    }
    match seen.first().copied() {
        None => Ok(Cell::new(address, "empty", "")),
        Some("formulaValue") => {
            let formula = value
                .get("formulaValue")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    SpreadsheetError::Import("formulaValue must be a string".to_string())
                })?;
            if !formula.starts_with('=') {
                return Err(SpreadsheetError::Import(format!(
                    "formula cell {address} source does not start with ="
                )));
            }
            Ok(Cell::new(address, "formula", formula))
        }
        Some("numberValue") => {
            let number = value
                .get("numberValue")
                .and_then(Value::as_f64)
                .ok_or_else(|| {
                    SpreadsheetError::Import("numberValue must be a number".to_string())
                })?;
            Ok(Cell::new(address, "number", &trim_sheet_number(number)))
        }
        Some("boolValue") => {
            let boolean = value
                .get("boolValue")
                .and_then(Value::as_bool)
                .ok_or_else(|| {
                    SpreadsheetError::Import("boolValue must be a boolean".to_string())
                })?;
            Ok(Cell::new(
                address,
                "bool",
                if boolean { "true" } else { "false" },
            ))
        }
        Some("stringValue") => {
            let string = value
                .get("stringValue")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    SpreadsheetError::Import("stringValue must be a string".to_string())
                })?;
            Ok(Cell::new(address, "string", string))
        }
        Some(_) => unreachable!("unexpected Google Sheets value kind"),
    }
}

fn reject_google_sheets_cell_high_risk(value: &Value) -> Result<(), SpreadsheetError> {
    for key in [
        "pivotTable",
        "dataSourceTable",
        "dataSourceFormula",
        "chipRuns",
        "hyperlink",
        "textFormatRuns",
    ] {
        if value.get(key).is_some() {
            return Err(SpreadsheetError::Import(format!(
                "unsupported high-risk Google Sheets cell field {key}"
            )));
        }
    }
    Ok(())
}

fn import_google_sheets_cell_comment(value: &Value) -> Result<CellComment, SpreadsheetError> {
    let comment = CellComment {
        id: google_required_string(value, "id", "cell comment missing id")?.to_string(),
        author: google_required_string(value, "author", "cell comment missing author")?.to_string(),
        body: google_required_string(value, "body", "cell comment missing body")?.to_string(),
        deleted: google_optional_bool(value, "deleted")?.unwrap_or(false),
    };
    comment.validate_source()?;
    Ok(comment)
}

fn import_google_sheets_validation(value: &Value) -> Result<CellValidation, SpreadsheetError> {
    let condition = google_optional_object(value, "condition")?.unwrap_or(&Value::Null);
    let kind = google_optional_string(condition, "type")?
        .unwrap_or("CUSTOM_FORMULA")
        .to_ascii_lowercase();
    let values = import_google_sheets_validation_values(condition)?;
    let mut validation = CellValidation::new(
        &kind,
        values,
        google_optional_bool(value, "strict")?.unwrap_or(false),
    )?;
    validation.show_dropdown = google_optional_bool(value, "showCustomUi")?.unwrap_or(true);
    Ok(validation)
}

fn import_google_sheets_validation_values(
    condition: &Value,
) -> Result<Vec<String>, SpreadsheetError> {
    let Some(values) = condition.get("values") else {
        return Ok(Vec::new());
    };
    let values = values.as_array().ok_or_else(|| {
        SpreadsheetError::Import("dataValidation values must be an array".to_string())
    })?;
    if values.len() > 64 {
        return Err(SpreadsheetError::Import(
            "dataValidation values exceed supported limit 64".to_string(),
        ));
    }
    values
        .iter()
        .enumerate()
        .map(|(index, item)| {
            google_expect_object(item, &format!("dataValidation value {index}"))?;
            let value = item
                .get("userEnteredValue")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    SpreadsheetError::Import(format!(
                        "dataValidation value {index} missing string userEnteredValue"
                    ))
                })?
                .trim()
                .to_string();
            if value.is_empty() {
                return Err(SpreadsheetError::Import(format!(
                    "dataValidation value {index} is empty"
                )));
            }
            Ok(value)
        })
        .collect()
}

fn import_google_sheets_format(value: &Value) -> Result<CellFormat, SpreadsheetError> {
    let text_format = google_optional_object(value, "textFormat")?.unwrap_or(&Value::Null);
    let number_format = google_optional_object(value, "numberFormat")?.unwrap_or(&Value::Null);
    let format = CellFormat {
        bold: google_optional_bool(text_format, "bold")?.unwrap_or(false),
        italic: google_optional_bool(text_format, "italic")?.unwrap_or(false),
        text_color: import_sheet_rgb(text_format.get("foregroundColor"), "foregroundColor")?,
        background_color: import_sheet_rgb(value.get("backgroundColor"), "backgroundColor")?,
        horizontal_align: google_optional_string(value, "horizontalAlignment")?
            .map(|value| value.to_ascii_lowercase()),
        number_format: google_optional_string(number_format, "type")?.map(ToString::to_string),
    };
    format.validate_source()?;
    Ok(format)
}

fn google_grid_range_to_a1(value: &Value) -> Result<String, SpreadsheetError> {
    let start_row_index = google_range_optional_u64(value, "startRowIndex")?.unwrap_or(0);
    let end_row_index = google_range_required_u64(value, "endRowIndex")?;
    let start_column_index = google_range_optional_u64(value, "startColumnIndex")?.unwrap_or(0);
    let end_column_index = google_range_required_u64(value, "endColumnIndex")?;

    if end_row_index <= start_row_index {
        return Err(SpreadsheetError::Import(
            "range endRowIndex must be greater than startRowIndex".to_string(),
        ));
    }
    if end_column_index <= start_column_index {
        return Err(SpreadsheetError::Import(
            "range endColumnIndex must be greater than startColumnIndex".to_string(),
        ));
    }

    let start_row = start_row_index
        .checked_add(1)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| SpreadsheetError::Import("range startRowIndex is too large".to_string()))?;
    let end_row = u32::try_from(end_row_index)
        .map_err(|_| SpreadsheetError::Import("range endRowIndex is too large".to_string()))?;
    let start_column = start_column_index
        .checked_add(1)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| {
            SpreadsheetError::Import("range startColumnIndex is too large".to_string())
        })?;
    let end_column = u32::try_from(end_column_index)
        .map_err(|_| SpreadsheetError::Import("range endColumnIndex is too large".to_string()))?;
    let start = cell_address(start_column, start_row)?;
    let end = cell_address(end_column, end_row)?;
    Ok(format!("{start}:{end}"))
}

fn google_range_optional_u64(value: &Value, key: &str) -> Result<Option<u64>, SpreadsheetError> {
    match value.get(key) {
        Some(Value::Null) | None => Ok(None),
        Some(field) => field.as_u64().map(Some).ok_or_else(|| {
            SpreadsheetError::Import(format!("range {key} must be a non-negative integer"))
        }),
    }
}

fn google_range_required_u64(value: &Value, key: &str) -> Result<u64, SpreadsheetError> {
    google_range_optional_u64(value, key)?
        .ok_or_else(|| SpreadsheetError::Import(format!("range missing {key}")))
}

pub fn export_google_sheets_workbook(
    workbook: &SpreadsheetWorkbook,
) -> Result<String, SpreadsheetError> {
    validate_google_sheets_export_workbook(workbook)?;
    let sheet_id_map = workbook
        .sheets
        .iter()
        .enumerate()
        .map(|(index, sheet)| (sheet.id.clone(), index as i64))
        .collect::<BTreeMap<_, _>>();
    let sheets = workbook
        .sheets
        .iter()
        .enumerate()
        .map(|(index, sheet)| export_google_sheets_sheet(index as i64, sheet))
        .collect::<Result<Vec<_>, _>>()?;
    let named_ranges = workbook
        .named_ranges
        .iter()
        .map(|range| export_google_sheets_named_range(range, &sheet_id_map))
        .collect::<Result<Vec<_>, _>>()?;
    serde_json::to_string_pretty(&json!({
        "spreadsheetId": "opendoc-export",
        "properties": {
            "title": workbook.title,
            "locale": workbook.locale,
            "timeZone": workbook.timezone,
        },
        "sheets": sheets,
        "namedRanges": named_ranges,
    }))
    .map_err(|err| SpreadsheetError::Import(err.to_string()))
}

fn validate_google_sheets_export_workbook(
    workbook: &SpreadsheetWorkbook,
) -> Result<(), SpreadsheetError> {
    workbook.validate_source()?;
    if workbook.title.trim().is_empty() {
        return Err(SpreadsheetError::Format(
            "spreadsheet workbook title is empty".to_string(),
        ));
    }
    if workbook.locale.trim().is_empty() {
        return Err(SpreadsheetError::Format(
            "spreadsheet workbook locale is empty".to_string(),
        ));
    }
    if workbook.timezone.trim().is_empty() {
        return Err(SpreadsheetError::Format(
            "spreadsheet workbook timezone is empty".to_string(),
        ));
    }
    if workbook.sheets.is_empty() {
        return Err(SpreadsheetError::Format(
            "spreadsheet workbook has no sheets".to_string(),
        ));
    }

    let mut sheet_ids = BTreeSet::new();
    let mut sheet_titles = BTreeSet::new();
    for sheet in &workbook.sheets {
        if sheet.id.trim().is_empty() {
            return Err(SpreadsheetError::Format(
                "spreadsheet sheet id is empty".to_string(),
            ));
        }
        if !sheet_ids.insert(sheet.id.clone()) {
            return Err(SpreadsheetError::Format(format!(
                "duplicate spreadsheet sheet id {}",
                sheet.id
            )));
        }
        if sheet.title.trim().is_empty() {
            return Err(SpreadsheetError::Format(format!(
                "spreadsheet sheet {} title is empty",
                sheet.id
            )));
        }
        if !sheet_titles.insert(sheet.title.clone()) {
            return Err(SpreadsheetError::Format(format!(
                "duplicate spreadsheet sheet title {}",
                sheet.title
            )));
        }
    }
    validate_google_sheets_export_named_ranges(workbook)?;
    Ok(())
}

fn validate_google_sheets_export_named_ranges(
    workbook: &SpreadsheetWorkbook,
) -> Result<(), SpreadsheetError> {
    let mut named_range_ids = BTreeSet::new();
    let mut named_range_names = BTreeSet::new();
    for named_range in &workbook.named_ranges {
        named_range.validate_source()?;
        if !named_range_ids.insert(named_range.id.clone()) {
            return Err(SpreadsheetError::Format(format!(
                "duplicate spreadsheet named range id {}",
                named_range.id
            )));
        }
        let normalized_name = normalize_named_range_name(&named_range.name)?;
        if !named_range_names.insert(normalized_name.clone()) {
            return Err(SpreadsheetError::Format(format!(
                "duplicate spreadsheet named range {normalized_name}"
            )));
        }
    }
    Ok(())
}

fn export_google_sheets_sheet(sheet_id: i64, sheet: &Sheet) -> Result<Value, SpreadsheetError> {
    validate_google_sheets_export_sheet(sheet)?;
    let cell_map = sheet
        .cells
        .iter()
        .map(|cell| (cell.address.as_str(), cell))
        .collect::<BTreeMap<_, _>>();
    let mut rows = Vec::new();
    for row in &sheet.rows {
        let mut values = Vec::new();
        for column in &sheet.columns {
            let address = format!("{column}{row}");
            values.push(
                cell_map
                    .get(address.as_str())
                    .map(|cell| export_google_sheets_cell(cell))
                    .transpose()?
                    .unwrap_or_else(|| json!({})),
            );
        }
        rows.push(json!({ "values": values }));
    }
    Ok(json!({
        "properties": {
            "sheetId": sheet_id,
            "title": sheet.title,
                "gridProperties": {
                    "rowCount": sheet.rows.len(),
                    "columnCount": sheet.columns.len(),
                    "frozenRowCount": sheet.frozen_rows,
                    "frozenColumnCount": sheet.frozen_columns,
                },
            },
        "data": [{
            "startRow": 0,
            "startColumn": 0,
            "rowData": rows,
        }],
        "merges": sheet
            .merges
            .iter()
            .map(|merge| export_google_sheets_grid_range(&merge.range, sheet_id))
            .collect::<Result<Vec<_>, _>>()?,
        "basicFilter": sheet
            .filters
            .first()
            .map(|filter| export_google_sheets_basic_filter(filter, sheet_id))
            .transpose()?,
        "protectedRanges": sheet
            .protected_ranges
            .iter()
            .enumerate()
            .map(|(index, protected_range)| {
                export_google_sheets_protected_range(sheet_id, index, protected_range)
            })
            .collect::<Result<Vec<_>, _>>()?,
    }))
}

fn validate_google_sheets_export_sheet(sheet: &Sheet) -> Result<(), SpreadsheetError> {
    if sheet.rows.is_empty() {
        return Err(SpreadsheetError::Format(format!(
            "sheet {} has no visible rows",
            sheet.id
        )));
    }
    if sheet.columns.is_empty() {
        return Err(SpreadsheetError::Format(format!(
            "sheet {} has no visible columns",
            sheet.id
        )));
    }
    if sheet.frozen_rows as usize > sheet.rows.len() {
        return Err(SpreadsheetError::Format(format!(
            "sheet {} frozen rows exceed visible rows",
            sheet.id
        )));
    }
    if sheet.frozen_columns as usize > sheet.columns.len() {
        return Err(SpreadsheetError::Format(format!(
            "sheet {} frozen columns exceed visible columns",
            sheet.id
        )));
    }
    validate_google_sheets_export_axis_labels(sheet)?;
    validate_google_sheets_export_ranges(sheet)?;
    validate_google_sheets_export_cells(sheet)
}

pub fn validate_google_sheets_export_ranges(sheet: &Sheet) -> Result<(), SpreadsheetError> {
    let mut merge_ids = BTreeSet::new();
    let mut parsed_merges: Vec<(&str, CellRange)> = Vec::new();
    for merge in &sheet.merges {
        merge.validate_source()?;
        if !merge_ids.insert(merge.id.clone()) {
            return Err(SpreadsheetError::Format(format!(
                "duplicate spreadsheet merge id {}",
                merge.id
            )));
        }
        let parsed = parse_cell_range(&merge.range)?;
        for (existing_range, existing) in &parsed_merges {
            if ranges_overlap(parsed, *existing) {
                return Err(SpreadsheetError::Format(format!(
                    "spreadsheet merge range {} overlaps {}",
                    merge.range, existing_range
                )));
            }
        }
        parsed_merges.push((merge.range.as_str(), parsed));
    }

    if sheet.filters.len() > 1 {
        return Err(SpreadsheetError::Format(format!(
            "sheet {} has multiple basic filters",
            sheet.id
        )));
    }
    let mut filter_ids = BTreeSet::new();
    for filter in &sheet.filters {
        filter.validate_source()?;
        if !filter_ids.insert(filter.id.clone()) {
            return Err(SpreadsheetError::Format(format!(
                "duplicate spreadsheet filter id {}",
                filter.id
            )));
        }
    }

    let mut protected_ids = BTreeSet::new();
    let mut protected_ranges = BTreeSet::new();
    for protected_range in &sheet.protected_ranges {
        protected_range.validate_source()?;
        if !protected_ids.insert(protected_range.id.clone()) {
            return Err(SpreadsheetError::Format(format!(
                "duplicate protected range id {}",
                protected_range.id
            )));
        }
        let normalized = normalize_cell_range(&protected_range.range)?;
        if !protected_ranges.insert(normalized.clone()) {
            return Err(SpreadsheetError::Format(format!(
                "duplicate protected range {normalized}"
            )));
        }
    }
    Ok(())
}

pub fn validate_google_sheets_export_axis_labels(sheet: &Sheet) -> Result<(), SpreadsheetError> {
    let mut seen_rows = BTreeSet::new();
    for row in &sheet.rows {
        let parsed = row
            .parse::<u32>()
            .map_err(|_| SpreadsheetError::Format(format!("invalid sheet row label {row}")))?;
        if parsed == 0 || parsed.to_string() != *row {
            return Err(SpreadsheetError::Format(format!(
                "invalid sheet row label {row}"
            )));
        }
        if !seen_rows.insert(row.clone()) {
            return Err(SpreadsheetError::Format(format!(
                "duplicate sheet row label {row}"
            )));
        }
    }

    let mut seen_columns = BTreeSet::new();
    for column in &sheet.columns {
        let Some(parsed) = column_to_number(column) else {
            return Err(SpreadsheetError::Format(format!(
                "invalid sheet column label {column}"
            )));
        };
        if number_to_column(parsed).as_deref() != Some(column.as_str()) {
            return Err(SpreadsheetError::Format(format!(
                "invalid sheet column label {column}"
            )));
        }
        if !seen_columns.insert(column.clone()) {
            return Err(SpreadsheetError::Format(format!(
                "duplicate sheet column label {column}"
            )));
        }
    }
    Ok(())
}

pub fn validate_google_sheets_export_cells(sheet: &Sheet) -> Result<(), SpreadsheetError> {
    let rows = sheet.rows.iter().cloned().collect::<BTreeSet<_>>();
    let columns = sheet.columns.iter().cloned().collect::<BTreeSet<_>>();
    let mut seen = BTreeSet::new();
    for cell in &sheet.cells {
        let normalized = normalize_cell_address(&cell.address)?;
        if normalized != cell.address {
            return Err(SpreadsheetError::Format(format!(
                "cell {} address is not canonical",
                cell.address
            )));
        }
        if !seen.insert(cell.address.clone()) {
            return Err(SpreadsheetError::Format(format!(
                "duplicate spreadsheet cell {}",
                cell.address
            )));
        }
        let (column, row) = split_cell_address(&cell.address);
        if !columns.contains(&column) || !rows.contains(&row) {
            return Err(SpreadsheetError::Format(format!(
                "cell {} is outside visible sheet grid",
                cell.address
            )));
        }
        validate_spreadsheet_cell_source(cell)?;
    }
    Ok(())
}

fn export_google_sheets_cell(cell: &Cell) -> Result<Value, SpreadsheetError> {
    validate_spreadsheet_cell_source(cell)?;
    let user_entered_value = match cell.user_kind.as_str() {
        "formula" => json!({ "formulaValue": cell.user_value }),
        "number" => {
            let number = cell.user_value.parse::<f64>().unwrap_or_default();
            json!({ "numberValue": number })
        }
        "bool" => json!({ "boolValue": cell.user_value == "true" }),
        "empty" => json!({}),
        _ => json!({ "stringValue": cell.user_value }),
    };
    let mut out = json!({
        "userEnteredValue": user_entered_value,
        "userEnteredFormat": export_google_sheets_format(&cell.format)?,
    });
    if let Some(validation) = &cell.validation {
        if let Some(object) = out.as_object_mut() {
            object.insert(
                "dataValidation".to_string(),
                export_google_sheets_validation(validation)?,
            );
        }
    }
    if let Some(first_comment) = cell
        .comments
        .iter()
        .find(|comment| !comment.deleted && comment.id.starts_with("note-"))
        .or_else(|| cell.comments.iter().find(|comment| !comment.deleted))
    {
        if let Some(object) = out.as_object_mut() {
            object.insert("note".to_string(), json!(first_comment.body));
        }
    }
    if !cell.comments.is_empty() {
        let comments = cell
            .comments
            .iter()
            .map(export_google_sheets_cell_comment)
            .collect::<Result<Vec<_>, _>>()?;
        if let Some(object) = out.as_object_mut() {
            object.insert("opendocCellComments".to_string(), Value::Array(comments));
        }
    }
    Ok(out)
}

fn export_google_sheets_cell_comment(comment: &CellComment) -> Result<Value, SpreadsheetError> {
    comment.validate_source()?;
    Ok(json!({
        "id": comment.id,
        "author": comment.author,
        "body": comment.body,
        "deleted": comment.deleted,
    }))
}

fn export_google_sheets_validation(validation: &CellValidation) -> Result<Value, SpreadsheetError> {
    validation.validate_source()?;
    let condition_type = if validation.kind == "list" {
        "ONE_OF_LIST".to_string()
    } else {
        validation.kind.to_ascii_uppercase()
    };
    Ok(json!({
        "condition": {
            "type": condition_type,
            "values": validation
                .values
                .iter()
                .map(|value| json!({ "userEnteredValue": value }))
                .collect::<Vec<_>>(),
        },
        "strict": validation.strict,
        "showCustomUi": validation.show_dropdown,
    }))
}

fn export_google_sheets_format(format: &CellFormat) -> Result<Value, SpreadsheetError> {
    format.validate_source()?;
    let mut text_format = serde_json::Map::new();
    if format.bold {
        text_format.insert("bold".to_string(), json!(true));
    }
    if format.italic {
        text_format.insert("italic".to_string(), json!(true));
    }
    if let Some(color) = &format.text_color {
        text_format.insert("foregroundColor".to_string(), export_sheet_color(color));
    }
    let mut out = serde_json::Map::new();
    if !text_format.is_empty() {
        out.insert("textFormat".to_string(), Value::Object(text_format));
    }
    if let Some(color) = &format.background_color {
        out.insert("backgroundColor".to_string(), export_sheet_color(color));
    }
    if let Some(align) = &format.horizontal_align {
        out.insert(
            "horizontalAlignment".to_string(),
            json!(align.to_ascii_uppercase()),
        );
    }
    if let Some(number_format) = &format.number_format {
        out.insert("numberFormat".to_string(), json!({ "type": number_format }));
    }
    Ok(Value::Object(out))
}

fn export_google_sheets_protected_range(
    sheet_id: i64,
    index: usize,
    protected_range: &SheetProtectedRange,
) -> Result<Value, SpreadsheetError> {
    protected_range.validate_source()?;
    Ok(json!({
        "protectedRangeId": index + 1,
        "range": export_google_sheets_grid_range(&protected_range.range, sheet_id)?,
        "description": protected_range.description,
        "warningOnly": protected_range.warning_only,
    }))
}

fn export_google_sheets_named_range(
    named_range: &NamedRange,
    sheet_id_map: &BTreeMap<String, i64>,
) -> Result<Value, SpreadsheetError> {
    named_range.validate_source()?;
    let google_sheet_id = sheet_id_map
        .get(&named_range.sheet_id)
        .copied()
        .ok_or_else(|| {
            SpreadsheetError::Format(format!(
                "named range {} references missing sheet {}",
                named_range.name, named_range.sheet_id
            ))
        })?;
    let range = parse_cell_range(&named_range.range)?;
    Ok(json!({
        "namedRangeId": named_range.id,
        "name": named_range.name,
        "range": {
            "sheetId": google_sheet_id,
            "startRowIndex": range.start_row - 1,
            "endRowIndex": range.start_row + range.height - 1,
            "startColumnIndex": range.start_column - 1,
            "endColumnIndex": range.start_column + range.width - 1,
        },
    }))
}

fn export_google_sheets_grid_range(range: &str, sheet_id: i64) -> Result<Value, SpreadsheetError> {
    let range = parse_cell_range(range)?;
    Ok(json!({
        "sheetId": sheet_id,
        "startRowIndex": range.start_row - 1,
        "endRowIndex": range.start_row + range.height - 1,
        "startColumnIndex": range.start_column - 1,
        "endColumnIndex": range.start_column + range.width - 1,
    }))
}

fn export_google_sheets_basic_filter(
    filter: &SheetFilter,
    sheet_id: i64,
) -> Result<Value, SpreadsheetError> {
    filter.validate_source()?;
    let parsed = parse_cell_range(&filter.range)?;
    let mut out = serde_json::Map::new();
    out.insert(
        "range".to_string(),
        export_google_sheets_grid_range(&filter.range, sheet_id)?,
    );
    if !filter.criteria.is_empty() {
        let mut criteria = serde_json::Map::new();
        for criterion in &filter.criteria {
            let column = column_to_number(&normalize_column_label(&criterion.column)?)
                .ok_or_else(|| SpreadsheetError::Format("invalid filter column".to_string()))?;
            let offset = column.checked_sub(parsed.start_column).ok_or_else(|| {
                SpreadsheetError::Format(format!(
                    "filter criterion column {} is outside range",
                    criterion.column
                ))
            })?;
            criteria.insert(
                offset.to_string(),
                json!({
                    "condition": {
                        "type": export_google_filter_condition(&criterion.condition)?,
                        "values": [{ "userEnteredValue": criterion.value }]
                    }
                }),
            );
        }
        out.insert("criteria".to_string(), Value::Object(criteria));
    }
    if !filter.sort_specs.is_empty() {
        out.insert(
            "sortSpecs".to_string(),
            Value::Array(
                filter
                    .sort_specs
                    .iter()
                    .map(|sort_spec| {
                        let column =
                            column_to_number(&normalize_column_label(&sort_spec.column)?).ok_or_else(
                                || SpreadsheetError::Format("invalid filter sort column".to_string()),
                            )?;
                        Ok(json!({
                            "dimensionIndex": column - 1,
                            "sortOrder": if sort_spec.descending { "DESCENDING" } else { "ASCENDING" }
                        }))
                    })
                    .collect::<Result<Vec<_>, SpreadsheetError>>()?,
            ),
        );
    }
    Ok(Value::Object(out))
}
