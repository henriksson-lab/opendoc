//! Google Sheets JSON import/export adapter for the spreadsheet model.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Value};

use super::address::{
    cell_address, column_to_number, normalize_cell_address, normalize_cell_range,
    normalize_column_label, normalize_merge_range, normalize_named_range_name, number_to_column,
    parse_cell_range, ranges_overlap, split_cell_address, CellRange,
};
use super::format::{
    export_sheet_color, import_sheet_rgb, trim_sheet_number, validate_sheet_color,
};
use super::model::{validate_axis_size_px, validate_spreadsheet_cell_source};
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
    // Google returns a single workbook resource even when only a small part of
    // it uses a feature this codec does not model.  Keep the independently
    // representable grid, but make every omitted feature explicit.  This scan
    // deliberately happens before importing cells so an unsupported formula,
    // link, or chip is never accidentally interpreted by a later code path.
    let mut warnings = google_sheets_unsupported_warnings(&value);
    workbook.set_metadata(title, locale, timezone)?;
    let mut sheet_id_map = BTreeMap::new();
    let mut google_sheet_ids = BTreeSet::new();
    let mut sheet_titles = BTreeSet::new();
    // Google assigns protected-range IDs across the workbook, not per sheet.
    // Retain those native identities in the model's existing opaque ID slot so
    // a JSON import/export/re-import does not manufacture different resources.
    let mut google_protected_range_ids = BTreeSet::new();

    // `sheets` is usually returned in tab order, but the native resource has
    // an explicit SheetProperties.index. Use that authoritative order when it
    // is supplied: a partial/reordered API response must not silently change
    // the workbook's ordered sheet list. Older/minimal fixtures that omit it
    // altogether retain their array order for compatibility.
    let sheets = google_sheets_in_tab_order(sheets)?;
    for (index, sheet_value) in sheets.into_iter().enumerate() {
        let properties = google_optional_object(sheet_value, "properties")?.unwrap_or(&Value::Null);
        let app_sheet_id = format!("sheet-{}", index + 1);
        let google_sheet_id = google_optional_non_negative_i64(properties, "sheetId")?;
        if let Some(google_sheet_id) = google_sheet_id {
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
        sheet.hidden = google_optional_bool(properties, "hidden")?.unwrap_or(false);
        sheet.tab_color = import_google_sheet_tab_color(properties, &mut warnings)?;
        import_google_sheets_grid_data(sheet_value, &mut sheet, &mut warnings)?;
        import_google_sheets_axis_metadata(sheet_value, &mut sheet)?;
        import_google_sheets_merges(sheet_value, &mut sheet, google_sheet_id)?;
        import_google_sheets_basic_filter(sheet_value, &mut sheet, &mut warnings)?;
        import_google_sheets_protected_ranges(
            sheet_value,
            &mut sheet,
            &mut warnings,
            &mut google_protected_range_ids,
        )?;
        sheet.ensure_axis_metadata();
        workbook.sheets.push(sheet);
    }

    // Sheets will not allow every tab in a spreadsheet to be hidden.  This is
    // not merely a UI preference: accepting such a native resource would let
    // a later export manufacture a payload the Google service cannot apply.
    // Keep the source boundary honest rather than quietly making one tab
    // visible, which would change the imported workbook.
    ensure_google_sheets_has_visible_sheet(&workbook.sheets, "import")?;

    if let Some(named_ranges) = google_optional_array(&value, "namedRanges")? {
        let mut named_range_ids = BTreeSet::new();
        let mut named_range_names = BTreeSet::new();
        for named_range in named_ranges {
            let name = google_required_string(named_range, "name", "named range missing name")?;
            let name = normalize_named_range_name(name)?;
            if !named_range_names.insert(name.clone()) {
                return Err(SpreadsheetError::Import(format!(
                    "duplicate Google Sheets named range {name}"
                )));
            }
            // `namedRangeId` is server-generated identity, but it is still
            // part of the native resource and our model has an exact home for
            // it.  Older/minimal fixtures may omit it; use the model's normal
            // deterministic ID in that case, while checking it against
            // explicit source IDs so the returned workbook is always valid.
            let named_range_id = google_optional_string(named_range, "namedRangeId")?
                .map(str::to_string)
                .unwrap_or_else(|| format!("named-{}", name.to_ascii_lowercase()));
            if named_range_id.trim().is_empty() {
                return Err(SpreadsheetError::Import(
                    "named range id is empty".to_string(),
                ));
            }
            if !named_range_ids.insert(named_range_id.clone()) {
                return Err(SpreadsheetError::Import(format!(
                    "duplicate Google Sheets namedRangeId {named_range_id}"
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
            // `add_named_range` supplies the local default identity for UI
            // creation.  Native import must instead retain Google's identity
            // so export/re-import does not manufacture a different resource.
            let imported = workbook
                .named_ranges
                .iter_mut()
                .find(|item| item.name == name)
                .expect("named range was just inserted or updated");
            imported.id = named_range_id;
        }
    }

    Ok(ImportedGoogleSheetsWorkbook { workbook, warnings })
}

/// Reads the Sheets API's current `tabColorStyle` and its deprecated
/// `tabColor` fallback.  If both are sent, `tabColorStyle` is authoritative:
/// accepting the deprecated field first would change a document whose current
/// style deliberately replaced it. The model owns literal RGB only, so a
/// themed colour remains intentionally undisclosed rather than being guessed.
fn import_google_sheet_tab_color(
    properties: &Value,
    warnings: &mut Vec<SpreadsheetWarning>,
) -> Result<Option<String>, SpreadsheetError> {
    if let Some(style) = google_optional_object(properties, "tabColorStyle")? {
        if style.get("rgbColor").is_some_and(|value| !value.is_null()) {
            return import_sheet_rgb(style.get("rgbColor"), "tabColorStyle.rgbColor");
        }
        if style
            .get("themeColor")
            .is_some_and(|value| !value.is_null())
        {
            push_unique_warning(
                warnings,
                "google-sheets-unsupported-tab-theme-color",
                "Google Sheets tabColorStyle.themeColor has no durable literal-RGB OpenDoc mapping, so the tab colour was not imported".to_string(),
            );
            return Ok(None);
        }
    }
    if properties
        .get("tabColor")
        .is_some_and(|value| !value.is_null())
    {
        return import_sheet_rgb(properties.get("tabColor"), "tabColor");
    }
    Ok(None)
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

/// Google Sheets requires at least one visible tab in a spreadsheet.  The
/// model intentionally permits an all-hidden workbook for formats with a
/// different contract, so enforce this native invariant only at the Sheets
/// interchange boundary.
fn ensure_google_sheets_has_visible_sheet(
    sheets: &[Sheet],
    direction: &str,
) -> Result<(), SpreadsheetError> {
    if sheets.iter().any(|sheet| !sheet.hidden) {
        return Ok(());
    }
    Err(match direction {
        "import" => SpreadsheetError::Import(
            "Google Sheets workbook has every sheet hidden; Google Sheets requires at least one visible sheet"
                .to_string(),
        ),
        "export" => SpreadsheetError::Format(
            "Google Sheets export requires at least one visible sheet; every OpenDoc sheet is hidden"
                .to_string(),
        ),
        _ => unreachable!("only the native Sheets import/export boundaries use this helper"),
    })
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

/// Returns the native sheet resources in their tab order.
///
/// Google documents `SheetProperties.index` as the zero-based tab position.
/// Either every returned sheet carries that native order, or none does (the
/// latter occurs in intentionally minimal legacy payloads). A mixed or
/// duplicate set has no unambiguous exact projection, so reject it instead of
/// privileging response-array order.
fn google_sheets_in_tab_order(sheets: &[Value]) -> Result<Vec<&Value>, SpreadsheetError> {
    let mut indexed = Vec::with_capacity(sheets.len());
    let mut index_count = 0;
    for (array_index, sheet) in sheets.iter().enumerate() {
        let properties = google_optional_object(sheet, "properties")?.unwrap_or(&Value::Null);
        if let Some(tab_index) = google_optional_non_negative_i64(properties, "index")? {
            index_count += 1;
            indexed.push((tab_index, array_index, sheet));
        }
    }
    if index_count == 0 {
        return Ok(sheets.iter().collect());
    }
    if index_count != sheets.len() {
        return Err(SpreadsheetError::Import(
            "Google Sheets sheet properties.index is missing on part of the workbook".to_string(),
        ));
    }
    indexed.sort_by_key(|(tab_index, _, _)| *tab_index);
    for (expected, (tab_index, _, _)) in indexed.iter().enumerate() {
        if *tab_index != expected as i64 {
            return Err(SpreadsheetError::Import(format!(
                "Google Sheets sheet properties.index must be the unique contiguous tab order; expected {expected}, found {tab_index}"
            )));
        }
    }
    Ok(indexed.into_iter().map(|(_, _, sheet)| sheet).collect())
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

fn google_sheets_unsupported_warnings(value: &Value) -> Vec<SpreadsheetWarning> {
    let mut locations = BTreeMap::<&'static str, BTreeSet<String>>::new();
    let mut record = |feature, location: String| {
        locations.entry(feature).or_default().insert(location);
    };
    for feature in ["charts", "pivotTables", "dataSourceSheetProperties"] {
        if value.get(feature).is_some() {
            record(feature, "workbook".to_string());
        }
    }
    for (sheet_index, sheet) in value
        .get("sheets")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
    {
        let sheet_location = format!("sheet {}", sheet_index + 1);
        for feature in [
            "charts",
            "pivotTables",
            "filterViews",
            "dataSourceSheetProperties",
        ] {
            if sheet.get(feature).is_some()
                || sheet
                    .get("properties")
                    .and_then(Value::as_object)
                    .is_some_and(|properties| properties.contains_key(feature))
            {
                record(feature, sheet_location.clone());
            }
        }
        // The model owns grid dimensions and frozen leading axes exactly, but
        // it deliberately has no sheet-wide gridline visibility or outline
        // grouping/control placement. Do not silently turn a source that
        // hides gridlines or moves outline controls after their group into the
        // Google default. Explicit `false` is the source default and has no
        // observable state to retain, so it needs no loss warning.
        if let Some(grid) = sheet
            .get("properties")
            .and_then(Value::as_object)
            .and_then(|properties| properties.get("gridProperties"))
            .and_then(Value::as_object)
        {
            for key in [
                "hideGridlines",
                "rowGroupControlAfter",
                "columnGroupControlAfter",
            ] {
                if !matches!(
                    grid.get(key),
                    None | Some(Value::Null) | Some(Value::Bool(false))
                ) {
                    record(
                        "gridProperties",
                        format!("{sheet_location} properties.gridProperties.{key}"),
                    );
                }
            }
        }
        // A conditional-format rule applies presentation dynamically over a
        // range. `CellFormat` intentionally contains only durable, authored
        // per-cell facts, so importing a rule as an ordinary format would
        // freeze one contingent display state and be untruthful. Retain the
        // independently usable sheet and name every discarded rule at its
        // native resource location instead.
        match sheet.get("conditionalFormats") {
            Some(Value::Array(rules)) => {
                for (rule_index, _) in rules.iter().enumerate() {
                    record(
                        "conditionalFormats",
                        format!("{sheet_location} conditionalFormats[{rule_index}]"),
                    );
                }
            }
            Some(_) => record("conditionalFormats", sheet_location.clone()),
            None => {}
        }
        for (data_index, grid_data) in sheet
            .get("data")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
        {
            for (row_index, row) in grid_data
                .get("rowData")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .enumerate()
            {
                for (column_index, cell) in row
                    .get("values")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .enumerate()
                {
                    let location = google_sheets_grid_cell_warning_location(
                        sheet_index,
                        data_index,
                        grid_data,
                        row_index,
                        column_index,
                    );
                    for feature in GOOGLE_SHEETS_UNSUPPORTED_CELL_FEATURES {
                        if cell.get(feature).is_some() {
                            record(feature, location.clone());
                        }
                    }
                    // `effectiveValue` is a calculated/read-only projection,
                    // not an authored cell source.  A response which omits
                    // `userEnteredValue` (for example because of a narrow
                    // field mask) must not turn that cache into an editable
                    // literal or formula.  The ordinary import path retains
                    // other authored metadata such as a note or format on
                    // the blank cell, but names the absent source/result.
                    if cell.get("effectiveValue").is_some()
                        && cell.get("userEnteredValue").is_none_or(Value::is_null)
                    {
                        record("effectiveValueWithoutUserEnteredValue", location);
                    }
                }
            }
        }
    }
    locations
        .into_iter()
        .map(|(feature, locations)| SpreadsheetWarning {
            code: format!(
                "google-sheets-unsupported-{}",
                google_sheets_warning_slug(feature)
            ),
            message: format!(
                "Google Sheets {feature} was not imported at {}",
                locations.into_iter().collect::<Vec<_>>().join(", ")
            ),
        })
        .collect()
}

const GOOGLE_SHEETS_UNSUPPORTED_CELL_FEATURES: [&str; 6] = [
    "pivotTable",
    "dataSourceTable",
    "dataSourceFormula",
    "chipRuns",
    "hyperlink",
    "textFormatRuns",
];

fn google_sheets_warning_slug(feature: &str) -> String {
    let mut slug = String::new();
    for (index, character) in feature.chars().enumerate() {
        if character.is_ascii_uppercase() {
            if index > 0 {
                slug.push('-');
            }
            slug.push(character.to_ascii_lowercase());
        } else {
            slug.push(character);
        }
    }
    slug
}

fn import_google_sheets_grid_data(
    sheet_value: &Value,
    sheet: &mut Sheet,
    warnings: &mut Vec<SpreadsheetWarning>,
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
                // A pivot/data-source payload can carry a cached value or a
                // formula.  Importing either would turn an omitted object into
                // an ordinary editable cell, so omit the entire cell.  The
                // preflight warning above records its exact source location.
                if google_sheets_cell_requires_omission(cell_value) {
                    continue;
                }
                let address =
                    google_grid_data_address(start_column, start_row, column_offset, row_offset)?;
                if !imported_cells.insert(address.clone()) {
                    return Err(SpreadsheetError::Import(format!(
                        "duplicate Google Sheets cell {address}"
                    )));
                }
                let mut cell = import_google_sheets_cell(&address, cell_value, warnings)?;
                cell.address = address;
                upsert_sheet_cell(sheet, cell);
            }
        }
    }
    Ok(())
}

/// Imports the dimension properties that belong to `GridData`, rather than to
/// `SheetProperties`.  Google may return more than one grid-data segment, so
/// conflicting repeats fail instead of depending on response order.
fn import_google_sheets_axis_metadata(
    sheet_value: &Value,
    sheet: &mut Sheet,
) -> Result<(), SpreadsheetError> {
    let Some(data_ranges) = google_optional_array(sheet_value, "data")? else {
        return Ok(());
    };
    let mut row_hidden = BTreeMap::new();
    let mut column_hidden = BTreeMap::new();
    for grid_data in data_ranges {
        import_google_sheets_dimension_metadata(
            grid_data,
            "rowMetadata",
            google_grid_data_start(grid_data, "startRow")?,
            &sheet.rows,
            &mut sheet.row_heights,
            &mut sheet.hidden_rows,
            &mut row_hidden,
        )?;
        import_google_sheets_dimension_metadata(
            grid_data,
            "columnMetadata",
            google_grid_data_start(grid_data, "startColumn")?,
            &sheet.columns,
            &mut sheet.column_widths,
            &mut sheet.hidden_columns,
            &mut column_hidden,
        )?;
    }
    sheet
        .hidden_rows
        .sort_by_key(|label| label.parse::<u32>().unwrap_or(0));
    sheet.hidden_rows.dedup();
    sheet
        .hidden_columns
        .sort_by_key(|label| column_to_number(label).unwrap_or(0));
    sheet.hidden_columns.dedup();
    Ok(())
}

fn import_google_sheets_dimension_metadata(
    grid_data: &Value,
    key: &str,
    start: u64,
    labels: &[String],
    sizes: &mut BTreeMap<String, u32>,
    hidden_labels: &mut Vec<String>,
    seen_hidden: &mut BTreeMap<String, bool>,
) -> Result<(), SpreadsheetError> {
    let Some(metadata) = google_optional_array(grid_data, key)? else {
        return Ok(());
    };
    for (offset, dimension) in metadata.iter().enumerate() {
        google_expect_object(dimension, key)?;
        let index = start
            .checked_add(
                u64::try_from(offset)
                    .map_err(|_| SpreadsheetError::Import(format!("{key} offset is too large")))?,
            )
            .and_then(|value| usize::try_from(value).ok())
            .ok_or_else(|| SpreadsheetError::Import(format!("{key} index is too large")))?;
        let label = labels.get(index).ok_or_else(|| {
            SpreadsheetError::Import(format!("{key} index {} is outside the visible grid", index))
        })?;
        if let Some(size) = google_optional_u64(dimension, "pixelSize")? {
            let size = u32::try_from(size).map_err(|_| {
                SpreadsheetError::Import(format!("{key} pixelSize exceeds supported u32 range"))
            })?;
            validate_axis_size_px(&format!("Google Sheets {key} pixelSize"), size)
                .map_err(|error| SpreadsheetError::Import(error.to_string()))?;
            if let Some(previous) = sizes.insert(label.clone(), size) {
                if previous != size {
                    return Err(SpreadsheetError::Import(format!(
                        "conflicting Google Sheets {key} pixelSize for {label}"
                    )));
                }
            }
        }
        if let Some(hidden) = google_optional_bool(dimension, "hiddenByUser")? {
            if let Some(previous) = seen_hidden.insert(label.clone(), hidden) {
                if previous != hidden {
                    return Err(SpreadsheetError::Import(format!(
                        "conflicting Google Sheets {key} hiddenByUser for {label}"
                    )));
                }
            }
            if hidden {
                hidden_labels.push(label.clone());
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

/// Gives an omission disclosure both a human-usable cell address and the
/// native JSON path that identifies the payload. A `GridData` response can
/// start at an arbitrary row/column, so `rowData[0].values[0]` alone is not a
/// cell address. Keep the JSON path when malformed offsets prevent a safe
/// address calculation; the importer will subsequently report that malformed
/// input through its normal validation path.
fn google_sheets_grid_cell_warning_location(
    sheet_index: usize,
    data_index: usize,
    grid_data: &Value,
    row_index: usize,
    column_index: usize,
) -> String {
    let path = format!(
        "sheet {} data[{data_index}] rowData[{row_index}].values[{column_index}]",
        sheet_index + 1
    );
    let Some(start_row) = grid_data
        .get("startRow")
        .map(Value::as_u64)
        .unwrap_or(Some(0))
    else {
        return path;
    };
    let Some(start_column) = grid_data
        .get("startColumn")
        .map(Value::as_u64)
        .unwrap_or(Some(0))
    else {
        return path;
    };
    let Ok(address) = google_grid_data_address(start_column, start_row, column_index, row_index)
    else {
        return path;
    };
    format!("sheet {} {address} ({path})", sheet_index + 1)
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
    expected_sheet_id: Option<i64>,
) -> Result<(), SpreadsheetError> {
    let mut merge_ranges = BTreeSet::new();
    let Some(merges) = google_optional_array(sheet_value, "merges")? else {
        return Ok(());
    };
    for merge in merges {
        google_expect_object(merge, "merge")?;
        if let (Some(expected_sheet_id), Some(merge_sheet_id)) = (
            expected_sheet_id,
            google_optional_non_negative_i64(merge, "sheetId")?,
        ) {
            if merge_sheet_id != expected_sheet_id {
                return Err(SpreadsheetError::Import(format!(
                    "Google Sheets merge belongs to sheetId {merge_sheet_id}, not sheetId {expected_sheet_id}"
                )));
            }
        }
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
    warnings: &mut Vec<SpreadsheetWarning>,
) -> Result<(), SpreadsheetError> {
    if let Some(filter) = sheet_value.get("basicFilter") {
        if filter.is_null() {
            return Ok(());
        }
        google_expect_object(filter, "basicFilter")?;
        let range_value = google_required_object(filter, "range", "basicFilter missing range")?;
        let range = google_grid_range_to_a1(range_value)?;
        set_sheet_basic_filter(sheet, &range)?;
        import_google_sheets_basic_filter_options(filter, sheet, warnings)?;
    }
    Ok(())
}

fn import_google_sheets_basic_filter_options(
    filter: &Value,
    sheet: &mut Sheet,
    warnings: &mut Vec<SpreadsheetWarning>,
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
    let mut dropped_criteria = Vec::new();
    if let Some(criteria_value) = google_optional_object(filter, "criteria")? {
        for (offset, criterion_value) in criteria_value.as_object().ok_or_else(|| {
            SpreadsheetError::Import("basicFilter criteria must be an object".to_string())
        })? {
            let Ok(offset) = offset.parse::<u32>() else {
                dropped_criteria.push(format!("criteria key {offset:?} is not a column offset"));
                continue;
            };
            let Some(column_number) = range.start_column.checked_add(offset) else {
                dropped_criteria.push(format!("criteria column offset {offset} is too large"));
                continue;
            };
            if column_number >= range.start_column + range.width {
                dropped_criteria.push(format!(
                    "criteria column offset {offset} is outside filter range {}",
                    sheet.filters[0].range
                ));
                continue;
            }
            let column = cell_address(column_number, 1)?
                .trim_end_matches('1')
                .to_string();
            let Some(condition_value) = google_optional_object(criterion_value, "condition")?
            else {
                dropped_criteria.push(format!("criterion {column} has no condition"));
                continue;
            };
            let Some(google_condition) = google_optional_string(condition_value, "type")? else {
                dropped_criteria.push(format!("criterion {column} condition has no type"));
                continue;
            };
            let Ok(condition) = import_google_filter_condition(google_condition) else {
                dropped_criteria.push(format!(
                    "criterion {column} uses unsupported condition {google_condition}"
                ));
                continue;
            };
            let Some(values) = google_optional_array(condition_value, "values")? else {
                dropped_criteria.push(format!("criterion {column} condition has no value"));
                continue;
            };
            if values.len() != 1 {
                dropped_criteria.push(format!(
                    "criterion {column} has {} values (the model owns one)",
                    values.len()
                ));
                continue;
            }
            let Some(value) = google_optional_string(&values[0], "userEnteredValue")? else {
                dropped_criteria.push(format!(
                    "criterion {column} value is not a string userEnteredValue"
                ));
                continue;
            };
            if value.trim().is_empty() {
                dropped_criteria.push(format!("criterion {column} value is empty"));
                continue;
            }
            criteria.push(SheetFilterCriterion {
                column,
                condition,
                value: value.to_string(),
            });
            if criterion_value.get("hiddenValues").is_some()
                || criterion_value.get("visibleBackgroundColor").is_some()
                || criterion_value.get("visibleBackgroundColorStyle").is_some()
            {
                dropped_criteria.push(format!(
                    "criterion {} has Google-only hidden-value or colour state",
                    criteria.last().expect("criterion was just pushed").column
                ));
            }
        }
    }
    let mut sort_specs = Vec::new();
    let mut dropped_sorts = Vec::new();
    if let Some(sort_values) = google_optional_array(filter, "sortSpecs")? {
        for (sort_index, sort_value) in sort_values.iter().enumerate() {
            google_expect_object(sort_value, "sortSpec")?;
            let Some(index) = google_range_optional_u64(sort_value, "dimensionIndex")? else {
                dropped_sorts.push(format!("sort spec {sort_index} has no dimensionIndex"));
                continue;
            };
            if index < u64::from(range.start_column - 1)
                || index >= u64::from(range.start_column + range.width - 1)
            {
                dropped_sorts.push(format!(
                    "sort spec {sort_index} dimensionIndex {index} is outside filter range {}",
                    sheet.filters[0].range
                ));
                continue;
            }
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
                    dropped_sorts.push(format!(
                        "sort spec {sort_index} uses unsupported sort order {other}"
                    ));
                    continue;
                }
            };
            sort_specs.push(SheetFilterSortSpec { column, descending });
        }
    }
    set_sheet_basic_filter_options(sheet, criteria, sort_specs)?;
    if !dropped_criteria.is_empty() {
        push_unique_warning(
            warnings,
            "google-sheets-basic-filter-criteria-unimported",
            format!(
                "Google Sheets basic filter {} retained its range, but omitted unsupported criteria: {}",
                sheet.filters[0].range,
                dropped_criteria.join("; ")
            ),
        );
    }
    if !dropped_sorts.is_empty() {
        push_unique_warning(
            warnings,
            "google-sheets-basic-filter-sorts-unimported",
            format!(
                "Google Sheets basic filter {} retained its range, but omitted unsupported sorts: {}",
                sheet.filters[0].range,
                dropped_sorts.join("; ")
            ),
        );
    }
    Ok(())
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
    protected_range_ids: &mut BTreeSet<i64>,
) -> Result<(), SpreadsheetError> {
    let mut protected_ranges = BTreeSet::new();
    let Some(ranges) = google_optional_array(sheet_value, "protectedRanges")? else {
        return Ok(());
    };
    for (index, protected_range) in ranges.iter().enumerate() {
        google_expect_object(protected_range, "protected range")?;
        let protected_range_id =
            google_optional_non_negative_i64(protected_range, "protectedRangeId")?;
        if let Some(protected_range_id) = protected_range_id {
            if !protected_range_ids.insert(protected_range_id) {
                return Err(SpreadsheetError::Import(format!(
                    "duplicate Google Sheets protectedRangeId {protected_range_id}"
                )));
            }
        }
        let location = format!("{} protectedRanges[{index}]", sheet.id);
        // A native protected range may name a NamedRange instead of embedding a
        // rectangle.  The durable advisory model intentionally owns only an
        // explicit rectangle, so leave that independent source feature out
        // rather than failing the whole workbook or guessing its target.
        let Some(range_value) = google_optional_object(protected_range, "range")? else {
            push_unique_warning(
                warnings,
                "google-sheets-protected-range-unimported",
                format!(
                    "Google Sheets protected range at {location} was not imported because it has no explicit rectangular range"
                ),
            );
            continue;
        };
        let range = google_grid_range_to_a1(range_value)?;
        let normalized = normalize_cell_range(&range)?;
        if !protected_ranges.insert(normalized.clone()) {
            return Err(SpreadsheetError::Import(format!(
                "duplicate Google Sheets protected range {normalized}"
            )));
        }
        // Google permits an omitted description.  OpenDoc does not have an
        // empty-description state, and inventing one would make an apparently
        // exact export lie about authored metadata.
        let Some(description) = google_optional_string(protected_range, "description")? else {
            push_unique_warning(
                warnings,
                "google-sheets-protected-range-unimported",
                format!(
                    "Google Sheets protected range at {location} was not imported because its optional description is absent"
                ),
            );
            continue;
        };
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
        if let Some(protected_range_id) = protected_range_id {
            // IDs are opaque outside the Google adapter.  A namespaced spelling
            // cannot collide with OpenDoc's normal `protected-...` IDs, while
            // retaining the exact native integer for later export.
            let imported = sheet
                .protected_ranges
                .iter_mut()
                .find(|item| item.range == normalized)
                .expect("protected range was just inserted or updated");
            imported.id = format!("google-protected-range-{protected_range_id}");
        }
    }
    Ok(())
}

fn import_google_sheets_cell(
    address: &str,
    value: &Value,
    warnings: &mut Vec<SpreadsheetWarning>,
) -> Result<Cell, SpreadsheetError> {
    let user_value = value.get("userEnteredValue").unwrap_or(&Value::Null);
    let mut cell = import_google_sheets_user_entered_value(address, user_value)?;
    if let Some(format) = google_optional_object(value, "userEnteredFormat")? {
        cell.format = import_google_sheets_format(address, format, warnings)?;
    }
    if let Some(validation) = google_optional_object(value, "dataValidation")? {
        // Validation is independent metadata: an unfamiliar Google-only
        // condition must not prevent importing the otherwise safe cell value.
        // Do not coerce it into a nearby OpenDoc rule; name the exact cell and
        // leave the rule absent instead.
        match import_google_sheets_validation(validation) {
            Ok(validation) => cell.validation = Some(validation),
            Err(error) => push_unique_warning(
                warnings,
                "google-sheets-data-validation-unimported",
                format!("data validation at {address} was omitted: {error}"),
            ),
        }
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

fn google_sheets_cell_requires_omission(value: &Value) -> bool {
    // Data-source and pivot cells do not have an independent user-entered
    // value.  A chip or hyperlink may, but a formula carrying either is still
    // an unsafe formula (for example HYPERLINK()), so it must not be parsed.
    ["pivotTable", "dataSourceTable", "dataSourceFormula"]
        .into_iter()
        .any(|feature| value.get(feature).is_some())
        || (value
            .get("userEnteredValue")
            .and_then(Value::as_object)
            .is_some_and(|user_value| user_value.contains_key("formulaValue"))
            && ["chipRuns", "hyperlink", "textFormatRuns"]
                .into_iter()
                .any(|feature| value.get(feature).is_some()))
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
    let google_type = google_optional_string(condition, "type")?.unwrap_or("CUSTOM_FORMULA");
    let kind = match google_type {
        "ONE_OF_LIST" => "list".to_string(),
        "ONE_OF_RANGE" => "one_of_range".to_string(),
        "CUSTOM_FORMULA" => "custom_formula".to_string(),
        "NUMBER_GREATER" => "number_greater".to_string(),
        "NUMBER_LESS" => "number_less".to_string(),
        "NUMBER_BETWEEN" => "number_between".to_string(),
        "TEXT_CONTAINS" => "text_contains".to_string(),
        other => other.to_ascii_lowercase(),
    };
    if !matches!(
        kind.as_str(),
        "list"
            | "one_of_range"
            | "custom_formula"
            | "number_greater"
            | "number_less"
            | "number_between"
            | "text_contains"
    ) {
        return Err(SpreadsheetError::Import(format!(
            "unsupported Google Sheets data-validation condition {google_type}"
        )));
    }
    let values = import_google_sheets_validation_values(condition)?;
    let mut validation = CellValidation::new(
        &kind,
        values,
        google_optional_bool(value, "strict")?.unwrap_or(false),
    )?;
    validation.show_dropdown = google_optional_bool(value, "showCustomUi")?.unwrap_or(true);
    validation.validate_source()?;
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

fn import_google_sheets_format(
    address: &str,
    value: &Value,
    warnings: &mut Vec<SpreadsheetWarning>,
) -> Result<CellFormat, SpreadsheetError> {
    let text_format = google_optional_object(value, "textFormat")?.unwrap_or(&Value::Null);
    let number_format = google_optional_object(value, "numberFormat")?.unwrap_or(&Value::Null);
    let format = CellFormat {
        bold: google_optional_bool(text_format, "bold")?.unwrap_or(false),
        italic: google_optional_bool(text_format, "italic")?.unwrap_or(false),
        text_color: import_sheet_rgb(text_format.get("foregroundColor"), "foregroundColor")?,
        background_color: import_sheet_rgb(value.get("backgroundColor"), "backgroundColor")?,
        horizontal_align: import_google_horizontal_alignment(address, value, warnings)?,
        wrap_strategy: import_google_wrap_strategy(address, value, warnings)?,
        vertical_align: import_google_vertical_alignment(address, value, warnings)?,
        // `type` is only a broad category.  Sheets uses `pattern` for the
        // authored display contract (including accounting, elapsed-time, and
        // locale-specific custom formats), so prefer it when present rather
        // than irreversibly collapsing it to the category.
        number_format: import_google_number_format(number_format)?,
    };
    format.validate_source()?;
    Ok(format)
}

/// Google represents the model default with an optional omitted field, but
/// its public enum also has an explicit `*_ALIGN_UNSPECIFIED` spelling. That
/// spelling is not an authored alignment and must not turn an otherwise
/// importable cell into an invalid OpenDoc `CellFormat`. Conversely, an enum
/// member outside the three model-owned positions cannot be quietly copied
/// into an invalid format or coerced into a nearby position.
fn import_google_horizontal_alignment(
    address: &str,
    value: &Value,
    warnings: &mut Vec<SpreadsheetWarning>,
) -> Result<Option<String>, SpreadsheetError> {
    let Some(alignment) = google_optional_string(value, "horizontalAlignment")? else {
        return Ok(None);
    };
    let normalized = alignment.to_ascii_uppercase();
    match normalized.as_str() {
        "LEFT" => Ok(Some("left".to_string())),
        "CENTER" => Ok(Some("center".to_string())),
        "RIGHT" => Ok(Some("right".to_string())),
        "HORIZONTAL_ALIGN_UNSPECIFIED" => Ok(None),
        _ => {
            push_unique_warning(
                warnings,
                "google-sheets-unsupported-horizontal-alignment",
                format!(
                    "Google Sheets cell {address} uses horizontalAlignment {alignment:?}; only LEFT, CENTER, and RIGHT have a durable OpenDoc mapping, so the alignment was not imported"
                ),
            );
            Ok(None)
        }
    }
}

fn import_google_vertical_alignment(
    address: &str,
    value: &Value,
    warnings: &mut Vec<SpreadsheetWarning>,
) -> Result<Option<String>, SpreadsheetError> {
    let Some(alignment) = google_optional_string(value, "verticalAlignment")? else {
        return Ok(None);
    };
    let normalized = alignment.to_ascii_uppercase();
    match normalized.as_str() {
        "TOP" => Ok(Some("top".to_string())),
        "MIDDLE" | "CENTER" => Ok(Some("middle".to_string())),
        "BOTTOM" => Ok(Some("bottom".to_string())),
        "VERTICAL_ALIGN_UNSPECIFIED" => Ok(None),
        _ => {
            push_unique_warning(
                warnings,
                "google-sheets-unsupported-vertical-alignment",
                format!(
                    "Google Sheets cell {address} uses verticalAlignment {alignment:?}; only TOP, MIDDLE, and BOTTOM have a durable OpenDoc mapping, so the alignment was not imported"
                ),
            );
            Ok(None)
        }
    }
}

fn import_google_number_format(value: &Value) -> Result<Option<String>, SpreadsheetError> {
    if let Some(pattern) = google_optional_string(value, "pattern")? {
        if !pattern.is_empty() {
            return Ok(Some(pattern.to_string()));
        }
    }
    Ok(google_optional_string(value, "type")?.map(ToString::to_string))
}

fn import_google_wrap_strategy(
    address: &str,
    value: &Value,
    warnings: &mut Vec<SpreadsheetWarning>,
) -> Result<Option<String>, SpreadsheetError> {
    let Some(strategy) = google_optional_string(value, "wrapStrategy")? else {
        return Ok(None);
    };
    if strategy.eq_ignore_ascii_case("WRAP") {
        return Ok(Some("wrap".to_string()));
    }
    // `OVERFLOW_CELL` and `CLIP` are meaningful authored choices in Sheets;
    // treating either as our absent default would lie about source intent.
    push_unique_warning(
        warnings,
        "google-sheets-unsupported-wrap-strategy",
        format!(
            "Google Sheets cell {address} uses wrapStrategy {strategy:?}; only WRAP has a durable OpenDoc mapping, so the strategy was not imported"
        ),
    );
    Ok(None)
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
    let protected_range_id_map = assign_google_protected_range_ids(workbook)?;
    let sheets = workbook
        .sheets
        .iter()
        .enumerate()
        .map(|(index, sheet)| {
            export_google_sheets_sheet(index as i64, sheet, &protected_range_id_map)
        })
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

fn assign_google_protected_range_ids(
    workbook: &SpreadsheetWorkbook,
) -> Result<BTreeMap<(String, String), i64>, SpreadsheetError> {
    let mut assigned = BTreeMap::new();
    let mut used = BTreeSet::new();
    for sheet in &workbook.sheets {
        for protected_range in &sheet.protected_ranges {
            if let Some(native_id) = google_protected_range_native_id(&protected_range.id) {
                if !used.insert(native_id) {
                    return Err(SpreadsheetError::Format(format!(
                        "duplicate native Google protected range id {native_id}"
                    )));
                }
                assigned.insert((sheet.id.clone(), protected_range.id.clone()), native_id);
            }
        }
    }
    let mut next_id = 1_i64;
    for sheet in &workbook.sheets {
        for protected_range in &sheet.protected_ranges {
            let key = (sheet.id.clone(), protected_range.id.clone());
            if assigned.contains_key(&key) {
                continue;
            }
            while used.contains(&next_id) {
                next_id += 1;
            }
            used.insert(next_id);
            assigned.insert(key, next_id);
            next_id += 1;
        }
    }
    Ok(assigned)
}

fn google_protected_range_native_id(id: &str) -> Option<i64> {
    let parsed = id.strip_prefix("google-protected-range-")?.parse().ok()?;
    (parsed >= 0).then_some(parsed)
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
    ensure_google_sheets_has_visible_sheet(&workbook.sheets, "export")?;

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

fn export_google_sheets_sheet(
    sheet_id: i64,
    sheet: &Sheet,
    protected_range_id_map: &BTreeMap<(String, String), i64>,
) -> Result<Value, SpreadsheetError> {
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
    let mut properties = serde_json::Map::new();
    properties.insert("sheetId".to_string(), json!(sheet_id));
    // The exporter assigns deterministic IDs in workbook order, so this is
    // also the exact zero-based native tab position.
    properties.insert("index".to_string(), json!(sheet_id));
    properties.insert("title".to_string(), json!(sheet.title));
    properties.insert("hidden".to_string(), json!(sheet.hidden));
    properties.insert(
        "gridProperties".to_string(),
        json!({
            "rowCount": sheet.rows.len(),
            "columnCount": sheet.columns.len(),
            "frozenRowCount": sheet.frozen_rows,
            "frozenColumnCount": sheet.frozen_columns,
        }),
    );
    if let Some(color) = &sheet.tab_color {
        // `tabColor` is accepted by old payloads but deprecated by the
        // Sheets API. New native output uses the current ColorStyle shape.
        properties.insert(
            "tabColorStyle".to_string(),
            json!({ "rgbColor": export_sheet_color(color) }),
        );
    }

    let mut data = serde_json::Map::new();
    data.insert("startRow".to_string(), json!(0));
    data.insert("startColumn".to_string(), json!(0));
    data.insert("rowData".to_string(), Value::Array(rows));
    if let Some(metadata) =
        export_google_sheets_dimension_metadata(&sheet.rows, &sheet.row_heights, &sheet.hidden_rows)
    {
        data.insert("rowMetadata".to_string(), metadata);
    }
    if let Some(metadata) = export_google_sheets_dimension_metadata(
        &sheet.columns,
        &sheet.column_widths,
        &sheet.hidden_columns,
    ) {
        data.insert("columnMetadata".to_string(), metadata);
    }

    Ok(json!({
        "properties": Value::Object(properties),
        "data": [Value::Object(data)],
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
            .map(|protected_range| {
                let protected_range_id = protected_range_id_map
                    .get(&(sheet.id.clone(), protected_range.id.clone()))
                    .copied()
                    .expect("every protected range was assigned a Google ID");
                export_google_sheets_protected_range(
                    sheet_id,
                    protected_range_id,
                    protected_range,
                )
            })
            .collect::<Result<Vec<_>, _>>()?,
    }))
}

fn export_google_sheets_dimension_metadata(
    labels: &[String],
    sizes: &BTreeMap<String, u32>,
    hidden_labels: &[String],
) -> Option<Value> {
    if sizes.is_empty() && hidden_labels.is_empty() {
        return None;
    }
    let hidden = hidden_labels.iter().collect::<BTreeSet<_>>();
    Some(Value::Array(
        labels
            .iter()
            .map(|label| {
                let mut dimension = serde_json::Map::new();
                if let Some(size) = sizes.get(label) {
                    dimension.insert("pixelSize".to_string(), json!(size));
                }
                if hidden.contains(&label) {
                    dimension.insert("hiddenByUser".to_string(), Value::Bool(true));
                }
                Value::Object(dimension)
            })
            .collect(),
    ))
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
    if let Some(color) = &sheet.tab_color {
        validate_sheet_color(color)?;
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
    if format.wrap_strategy.as_deref() == Some("wrap") {
        out.insert("wrapStrategy".to_string(), json!("WRAP"));
    }
    if let Some(align) = &format.vertical_align {
        let align = match align.as_str() {
            "middle" => "MIDDLE",
            "top" => "TOP",
            "bottom" => "BOTTOM",
            _ => unreachable!("CellFormat validation accepts only known vertical alignment"),
        };
        out.insert("verticalAlignment".to_string(), json!(align));
    }
    if let Some(number_format) = &format.number_format {
        out.insert(
            "numberFormat".to_string(),
            export_google_number_format(number_format),
        );
    }
    Ok(Value::Object(out))
}

fn export_google_number_format(number_format: &str) -> Value {
    // Google calls these values `NumberFormatType`; a raw OpenDoc pattern is
    // not itself an enum member.  Custom patterns therefore use NUMBER as
    // their broad category and preserve the actual authored string in
    // `pattern`, which is the field Sheets itself uses for display.
    let format_type = number_format.to_ascii_uppercase();
    match format_type.as_str() {
        "GENERAL" | "AUTOMATIC" => json!({ "type": "NUMBER" }),
        "NUMBER" | "PERCENT" | "CURRENCY" | "DATE" | "TIME" | "DATE_TIME" | "DATETIME"
        | "SCIENTIFIC" | "TEXT" => json!({
            "type": if format_type == "DATETIME" {
                "DATE_TIME"
            } else {
                format_type.as_str()
            }
        }),
        _ => json!({ "type": "NUMBER", "pattern": number_format }),
    }
}

fn export_google_sheets_protected_range(
    sheet_id: i64,
    protected_range_id: i64,
    protected_range: &SheetProtectedRange,
) -> Result<Value, SpreadsheetError> {
    protected_range.validate_source()?;
    Ok(json!({
        "protectedRangeId": protected_range_id,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn google_effective_value_without_authored_source_is_disclosed_not_imported_as_a_literal() {
        let payload = json!({
            "sheets": [{
                "properties": {
                    "sheetId": 1,
                    "title": "Partial response",
                    "gridProperties": { "rowCount": 2, "columnCount": 2 }
                },
                "data": [{
                    "startRow": 1,
                    "startColumn": 1,
                    "rowData": [{ "values": [{
                        "effectiveValue": { "numberValue": 42 },
                        "formattedValue": "42",
                        "note": "the authored note survives"
                    }] }]
                }]
            }]
        });

        let imported = import_google_sheets_workbook(&payload.to_string()).unwrap();
        let cell = imported.workbook.sheets[0]
            .cells
            .iter()
            .find(|cell| cell.address == "B2")
            .expect("metadata-bearing cell remains addressable");
        assert_eq!(cell.user_kind, "empty");
        assert_eq!(cell.user_value, "");
        assert_eq!(cell.comments[0].body, "the authored note survives");
        let warning = imported
            .warnings
            .iter()
            .find(|warning| {
                warning.code
                    == "google-sheets-unsupported-effective-value-without-user-entered-value"
            })
            .expect("result-only value is disclosed");
        assert!(warning.message.contains("B2"));
        assert!(warning.message.contains("data[0] rowData[0].values[0]"));

        let exported: Value =
            serde_json::from_str(&export_google_sheets_workbook(&imported.workbook).unwrap())
                .unwrap();
        let value = &exported["sheets"][0]["data"][0]["rowData"][1]["values"][1];
        assert_eq!(value["userEnteredValue"], json!({}));
        assert!(value.get("effectiveValue").is_none());
        assert_eq!(value["note"], "the authored note survives");
    }

    #[test]
    fn google_sheet_merges_round_trip_with_their_native_grid_ranges() {
        let payload = json!({
            "sheets": [{
                "properties": {
                    "sheetId": 37,
                    "title": "Merged cells",
                    "gridProperties": { "rowCount": 4, "columnCount": 4 }
                },
                "data": [{
                    "rowData": [{
                        "values": [{ "userEnteredValue": { "stringValue": "anchor" } }]
                    }]
                }],
                "merges": [
                    {
                        "sheetId": 37,
                        "endRowIndex": 2,
                        "endColumnIndex": 3
                    },
                    {
                        "sheetId": 37,
                        "startRowIndex": 2,
                        "endRowIndex": 4,
                        "startColumnIndex": 1,
                        "endColumnIndex": 4
                    }
                ]
            }]
        });

        let imported = import_google_sheets_workbook(&payload.to_string()).unwrap();
        let sheet = &imported.workbook.sheets[0];
        assert_eq!(
            sheet
                .merges
                .iter()
                .map(|merge| merge.range.as_str())
                .collect::<Vec<_>>(),
            ["A1:C2", "B3:D4"]
        );
        assert_eq!(sheet.cells[0].address, "A1");
        assert_eq!(sheet.cells[0].user_value, "anchor");

        let exported: Value =
            serde_json::from_str(&export_google_sheets_workbook(&imported.workbook).unwrap())
                .unwrap();
        assert_eq!(
            exported["sheets"][0]["merges"],
            json!([
                {
                    "sheetId": 0,
                    "startRowIndex": 0,
                    "endRowIndex": 2,
                    "startColumnIndex": 0,
                    "endColumnIndex": 3
                },
                {
                    "sheetId": 0,
                    "startRowIndex": 2,
                    "endRowIndex": 4,
                    "startColumnIndex": 1,
                    "endColumnIndex": 4
                }
            ])
        );
        let reread = import_google_sheets_workbook(&exported.to_string()).unwrap();
        assert_eq!(reread.workbook.sheets[0].merges, sheet.merges);
    }

    #[test]
    fn a_google_sheet_merge_cannot_point_to_another_sheet() {
        let payload = json!({
            "sheets": [{
                "properties": {
                    "sheetId": 37,
                    "gridProperties": { "rowCount": 2, "columnCount": 2 }
                },
                "merges": [{
                    "sheetId": 38,
                    "endRowIndex": 2,
                    "endColumnIndex": 2
                }]
            }]
        });

        let error = match import_google_sheets_workbook(&payload.to_string()) {
            Ok(_) => panic!("a merge cannot belong to a different sheet"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("sheetId 38, not sheetId 37"));
    }

    #[test]
    fn google_named_ranges_preserve_native_identity_and_grid_range() {
        let payload = json!({
            "sheets": [{
                "properties": {
                    "sheetId": 17,
                    "title": "Data",
                    "gridProperties": { "rowCount": 4, "columnCount": 3 }
                }
            }],
            "namedRanges": [{
                "namedRangeId": "native-range-9",
                "name": "quarterly_total2",
                "range": {
                    "sheetId": 17,
                    "startRowIndex": 1,
                    "endRowIndex": 4,
                    "startColumnIndex": 0,
                    "endColumnIndex": 2
                }
            }]
        });

        let imported = import_google_sheets_workbook(&payload.to_string()).unwrap();
        assert_eq!(imported.workbook.named_ranges.len(), 1);
        assert_eq!(
            imported.workbook.named_ranges[0],
            NamedRange {
                id: "native-range-9".to_string(),
                name: "QUARTERLY_TOTAL2".to_string(),
                sheet_id: "sheet-1".to_string(),
                range: "A2:B4".to_string(),
            }
        );

        let exported: Value =
            serde_json::from_str(&export_google_sheets_workbook(&imported.workbook).unwrap())
                .unwrap();
        assert_eq!(
            exported["namedRanges"],
            json!([{
                "namedRangeId": "native-range-9",
                "name": "QUARTERLY_TOTAL2",
                "range": {
                    "sheetId": 0,
                    "startRowIndex": 1,
                    "endRowIndex": 4,
                    "startColumnIndex": 0,
                    "endColumnIndex": 2,
                }
            }])
        );
        let reread = import_google_sheets_workbook(&exported.to_string()).unwrap();
        assert_eq!(reread.workbook.named_ranges, imported.workbook.named_ranges);
    }

    #[test]
    fn google_named_range_identity_collisions_fail_before_returning_invalid_workbook() {
        let payload = json!({
            "sheets": [{
                "properties": {
                    "sheetId": 17,
                    "gridProperties": { "rowCount": 1, "columnCount": 1 }
                }
            }],
            "namedRanges": [
                {
                    "name": "first",
                    "range": { "sheetId": 17, "endRowIndex": 1, "endColumnIndex": 1 }
                },
                {
                    "namedRangeId": "named-first",
                    "name": "second",
                    "range": { "sheetId": 17, "endRowIndex": 1, "endColumnIndex": 1 }
                }
            ]
        });

        let error = import_google_sheets_workbook(&payload.to_string())
            .err()
            .unwrap();
        assert!(error
            .to_string()
            .contains("duplicate Google Sheets namedRangeId named-first"));
    }

    #[test]
    fn google_warning_only_protected_ranges_preserve_native_identity_and_rectangle() {
        let payload = json!({
            "sheets": [
                {
                    "properties": {
                        "sheetId": 17,
                        "title": "First",
                        "gridProperties": { "rowCount": 3, "columnCount": 3 }
                    },
                    "protectedRanges": [{
                        "protectedRangeId": 41,
                        "range": {
                            "sheetId": 17,
                            "startRowIndex": 0,
                            "endRowIndex": 2,
                            "startColumnIndex": 1,
                            "endColumnIndex": 3
                        },
                        "description": "Check before editing",
                        "warningOnly": true
                    }]
                },
                {
                    "properties": {
                        "sheetId": 18,
                        "title": "Second",
                        "gridProperties": { "rowCount": 2, "columnCount": 2 }
                    },
                    "protectedRanges": [{
                        "protectedRangeId": 7,
                        "range": { "sheetId": 18, "endRowIndex": 1, "endColumnIndex": 1 },
                        "description": "Advisory only",
                        "warningOnly": true
                    }]
                }
            ]
        });

        let imported = import_google_sheets_workbook(&payload.to_string()).unwrap();
        assert!(imported.warnings.is_empty());
        assert_eq!(
            imported.workbook.sheets[0].protected_ranges[0],
            SheetProtectedRange {
                id: "google-protected-range-41".to_string(),
                range: "B1:C2".to_string(),
                description: "Check before editing".to_string(),
                warning_only: true,
            }
        );

        let exported: Value =
            serde_json::from_str(&export_google_sheets_workbook(&imported.workbook).unwrap())
                .unwrap();
        assert_eq!(
            exported["sheets"][0]["protectedRanges"],
            json!([{
                "protectedRangeId": 41,
                "range": {
                    "sheetId": 0,
                    "startRowIndex": 0,
                    "endRowIndex": 2,
                    "startColumnIndex": 1,
                    "endColumnIndex": 3
                },
                "description": "Check before editing",
                "warningOnly": true
            }])
        );
        assert_eq!(
            exported["sheets"][1]["protectedRanges"][0]["protectedRangeId"],
            7
        );
        let reread = import_google_sheets_workbook(&exported.to_string()).unwrap();
        assert_eq!(
            reread.workbook.sheets[0].protected_ranges,
            imported.workbook.sheets[0].protected_ranges
        );
        assert_eq!(
            reread.workbook.sheets[1].protected_ranges,
            imported.workbook.sheets[1].protected_ranges
        );
    }

    #[test]
    fn google_permission_protection_is_downgraded_and_nonrectangular_forms_are_disclosed() {
        let payload = json!({
            "sheets": [{
                "properties": {
                    "sheetId": 17,
                    "title": "Data",
                    "gridProperties": { "rowCount": 2, "columnCount": 2 }
                },
                "protectedRanges": [
                    {
                        "protectedRangeId": 2,
                        "range": { "sheetId": 17, "endRowIndex": 1, "endColumnIndex": 1 },
                        "description": "Permission boundary",
                        "warningOnly": false,
                        "editors": { "users": ["person@example.invalid"] }
                    },
                    {
                        "protectedRangeId": 3,
                        "namedRangeId": "native-range",
                        "description": "Named range protection",
                        "warningOnly": true
                    },
                    {
                        "protectedRangeId": 4,
                        "range": { "sheetId": 17, "startRowIndex": 1, "endRowIndex": 2, "startColumnIndex": 1, "endColumnIndex": 2 },
                        "warningOnly": true
                    }
                ]
            }]
        });

        let imported = import_google_sheets_workbook(&payload.to_string()).unwrap();
        assert_eq!(imported.workbook.sheets[0].protected_ranges.len(), 1);
        assert!(imported.workbook.sheets[0].protected_ranges[0].warning_only);
        assert!(imported.warnings.iter().any(|warning| {
            warning.code == "protected-range-warning-only" && warning.message.contains("sheet-1!A1")
        }));
        assert!(imported.warnings.iter().any(|warning| {
            warning.code == "google-sheets-protected-range-unimported"
                && warning.message.contains("sheet-1 protectedRanges[1]")
                && warning.message.contains("explicit rectangular range")
        }));
        assert!(imported.warnings.iter().any(|warning| {
            warning.code == "google-sheets-protected-range-unimported"
                && warning.message.contains("sheet-1 protectedRanges[2]")
                && warning.message.contains("description is absent")
        }));
    }

    #[test]
    fn unsupported_google_features_warn_but_do_not_reject_safe_grid() {
        let payload = json!({
            "properties": { "title": "Advanced but usable" },
            "charts": [{ "chartId": 1 }],
            "sheets": [{
                "properties": {
                    "sheetId": 7,
                    "title": "Data",
                    "gridProperties": { "rowCount": 3, "columnCount": 4 }
                },
                "filterViews": [{ "filterViewId": 2 }],
                "conditionalFormats": [
                    {
                        "ranges": [{
                            "sheetId": 7,
                            "startRowIndex": 0,
                            "endRowIndex": 1,
                            "startColumnIndex": 0,
                            "endColumnIndex": 1
                        }],
                        "booleanRule": {
                            "condition": { "type": "TEXT_EQ", "values": [{ "userEnteredValue": "safe" }] },
                            "format": { "backgroundColor": { "red": 1.0 } }
                        }
                    },
                    {
                        "ranges": [{
                            "sheetId": 7,
                            "startRowIndex": 1,
                            "endRowIndex": 2,
                            "startColumnIndex": 0,
                            "endColumnIndex": 1
                        }],
                        "gradientRule": {}
                    }
                ],
                "data": [{ "rowData": [{ "values": [
                    { "userEnteredValue": { "stringValue": "safe" } },
                    {
                        "userEnteredValue": { "stringValue": "visible label" },
                        "hyperlink": "https://example.invalid"
                    },
                    {
                        "userEnteredValue": { "formulaValue": "=HYPERLINK(\"https://example.invalid\", \"unsafe\")" },
                        "hyperlink": "https://example.invalid"
                    },
                    {
                        "userEnteredValue": { "numberValue": 9 },
                        "pivotTable": { "source": {} }
                    }
                ] }] }]
            }]
        });

        let imported = import_google_sheets_workbook(&payload.to_string()).unwrap();
        let sheet = &imported.workbook.sheets[0];
        assert_eq!(sheet.cells.len(), 2);
        assert!(sheet
            .cells
            .iter()
            .any(|cell| cell.address == "A1" && cell.user_value == "safe"));
        // A non-formula label remains useful grid data, but the URL itself is
        // never imported as a link.
        assert!(sheet
            .cells
            .iter()
            .any(|cell| cell.address == "B1" && cell.user_value == "visible label"));
        // Neither an unsupported link formula nor a pivot cache is interpreted
        // as an ordinary formula/value cell.
        assert!(!sheet.cells.iter().any(|cell| cell.address == "C1"));
        assert!(!sheet.cells.iter().any(|cell| cell.address == "D1"));

        let warnings = imported
            .warnings
            .iter()
            .map(|warning| (&warning.code, &warning.message))
            .collect::<Vec<_>>();
        assert!(warnings.iter().any(|(code, message)| {
            *code == "google-sheets-unsupported-charts" && message.contains("workbook")
        }));
        assert!(warnings.iter().any(|(code, message)| {
            *code == "google-sheets-unsupported-filter-views" && message.contains("sheet 1")
        }));
        assert!(warnings.iter().any(|(code, message)| {
            *code == "google-sheets-unsupported-conditional-formats"
                && message.contains("sheet 1 conditionalFormats[0]")
                && message.contains("sheet 1 conditionalFormats[1]")
        }));
        assert!(warnings.iter().any(|(code, message)| {
            *code == "google-sheets-unsupported-hyperlink"
                && message.contains("rowData[0].values[1]")
                && message.contains("rowData[0].values[2]")
        }));
        assert!(warnings.iter().any(|(code, message)| {
            *code == "google-sheets-unsupported-pivot-table"
                && message.contains("D1")
                && message.contains("rowData[0].values[3]")
        }));
    }

    #[test]
    fn google_unsupported_data_source_locations_include_offset_a1_addresses() {
        let payload = json!({
            "sheets": [{
                "properties": {
                    "sheetId": 1,
                    "title": "Sparse",
                    "gridProperties": { "rowCount": 8, "columnCount": 8 }
                },
                "data": [{
                    "startRow": 3,
                    "startColumn": 2,
                    "rowData": [{ "values": [
                        { "userEnteredValue": { "numberValue": 12 }, "dataSourceFormula": {} },
                        { "userEnteredValue": { "stringValue": "safe" } }
                    ] }]
                }]
            }]
        });

        let imported = import_google_sheets_workbook(&payload.to_string()).unwrap();
        let sheet = &imported.workbook.sheets[0];
        assert!(!sheet.cells.iter().any(|cell| cell.address == "C4"));
        assert!(sheet
            .cells
            .iter()
            .any(|cell| cell.address == "D4" && cell.user_value == "safe"));
        let warning = imported
            .warnings
            .iter()
            .find(|warning| warning.code == "google-sheets-unsupported-data-source-formula")
            .expect("data-source formula warning");
        assert!(warning.message.contains("C4"));
        assert!(warning.message.contains("data[0] rowData[0].values[0]"));
    }

    #[test]
    fn google_basic_filter_keeps_representable_options_and_discloses_the_rest() {
        let payload = json!({
            "sheets": [{
                "properties": {
                    "sheetId": 1,
                    "title": "Filtered",
                    "gridProperties": { "rowCount": 3, "columnCount": 2 }
                },
                "basicFilter": {
                    "range": {
                        "sheetId": 1,
                        "startRowIndex": 0,
                        "endRowIndex": 3,
                        "startColumnIndex": 0,
                        "endColumnIndex": 2
                    },
                    "criteria": {
                        "0": {
                            "condition": {
                                "type": "TEXT_CONTAINS",
                                "values": [{ "userEnteredValue": "keep" }]
                            },
                            "hiddenValues": ["not-modelled"]
                        },
                        "1": {
                            "condition": {
                                "type": "DATE_AFTER",
                                "values": [{ "userEnteredValue": "2026-01-01" }]
                            }
                        }
                    },
                    "sortSpecs": [
                        { "dimensionIndex": 1, "sortOrder": "DESCENDING" },
                        { "dimensionIndex": 0, "sortOrder": "SORT_ORDER_UNSPECIFIED" }
                    ]
                }
            }]
        });

        let imported = import_google_sheets_workbook(&payload.to_string()).unwrap();
        let filter = imported.workbook.sheets[0].filters.first().unwrap();
        assert_eq!(filter.range, "A1:B3");
        assert_eq!(filter.criteria.len(), 1);
        assert_eq!(filter.criteria[0].column, "A");
        assert_eq!(filter.criteria[0].condition, "text_contains");
        assert_eq!(filter.criteria[0].value, "keep");
        assert_eq!(filter.sort_specs.len(), 1);
        assert_eq!(filter.sort_specs[0].column, "B");
        assert!(filter.sort_specs[0].descending);
        assert!(imported.warnings.iter().any(|warning| {
            warning.code == "google-sheets-basic-filter-criteria-unimported"
                && warning.message.contains("DATE_AFTER")
                && warning.message.contains("hidden-value")
        }));
        assert!(imported.warnings.iter().any(|warning| {
            warning.code == "google-sheets-basic-filter-sorts-unimported"
                && warning.message.contains("SORT_ORDER_UNSPECIFIED")
        }));

        let exported: Value =
            serde_json::from_str(&export_google_sheets_workbook(&imported.workbook).unwrap())
                .unwrap();
        let filter = &exported["sheets"][0]["basicFilter"];
        assert_eq!(filter["range"]["startColumnIndex"], 0);
        assert_eq!(
            filter["criteria"]["0"]["condition"]["type"],
            "TEXT_CONTAINS"
        );
        assert!(filter["criteria"].get("1").is_none());
        assert_eq!(filter["sortSpecs"][0]["dimensionIndex"], 1);
        assert_eq!(filter["sortSpecs"].as_array().unwrap().len(), 1);

        let reread = import_google_sheets_workbook(&exported.to_string()).unwrap();
        assert_eq!(
            reread.workbook.sheets[0].filters,
            imported.workbook.sheets[0].filters
        );
    }

    #[test]
    fn google_cell_wrap_and_vertical_alignment_round_trip() {
        let payload = json!({
            "sheets": [{
                "properties": { "sheetId": 1, "title": "Styled", "gridProperties": { "rowCount": 1, "columnCount": 1 } },
                "data": [{ "rowData": [{ "values": [{
                    "userEnteredValue": { "stringValue": "two words" },
                    "userEnteredFormat": { "wrapStrategy": "WRAP", "verticalAlignment": "MIDDLE" }
                }] }] }]
            }]
        });
        let imported = import_google_sheets_workbook(&payload.to_string()).unwrap();
        let cell = &imported.workbook.sheets[0].cells[0];
        assert_eq!(cell.format.wrap_strategy.as_deref(), Some("wrap"));
        assert_eq!(cell.format.vertical_align.as_deref(), Some("middle"));

        let exported: Value =
            serde_json::from_str(&export_google_sheets_workbook(&imported.workbook).unwrap())
                .unwrap();
        let format =
            &exported["sheets"][0]["data"][0]["rowData"][0]["values"][0]["userEnteredFormat"];
        assert_eq!(format["wrapStrategy"], "WRAP");
        assert_eq!(format["verticalAlignment"], "MIDDLE");
    }

    #[test]
    fn google_cell_alignment_unspecified_and_unowned_values_do_not_reject_the_grid() {
        let payload = json!({
            "sheets": [{
                "properties": { "sheetId": 1, "title": "Aligned", "gridProperties": { "rowCount": 1, "columnCount": 3 } },
                "data": [{ "rowData": [{ "values": [
                    {
                        "userEnteredValue": { "stringValue": "default" },
                        "userEnteredFormat": {
                            "horizontalAlignment": "HORIZONTAL_ALIGN_UNSPECIFIED",
                            "verticalAlignment": "VERTICAL_ALIGN_UNSPECIFIED"
                        }
                    },
                    {
                        "userEnteredValue": { "stringValue": "kept" },
                        "userEnteredFormat": {
                            "horizontalAlignment": "LEFT",
                            "verticalAlignment": "TOP"
                        }
                    },
                    {
                        "userEnteredValue": { "stringValue": "disclosed" },
                        "userEnteredFormat": {
                            "horizontalAlignment": "JUSTIFY",
                            "verticalAlignment": "DISTRIBUTED"
                        }
                    }
                ] }] }]
            }]
        });

        let imported = import_google_sheets_workbook(&payload.to_string()).unwrap();
        let cells = &imported.workbook.sheets[0].cells;
        assert_eq!(cells.len(), 3);
        assert_eq!(cells[0].format.horizontal_align, None);
        assert_eq!(cells[0].format.vertical_align, None);
        assert_eq!(cells[1].format.horizontal_align.as_deref(), Some("left"));
        assert_eq!(cells[1].format.vertical_align.as_deref(), Some("top"));
        assert_eq!(cells[2].format.horizontal_align, None);
        assert_eq!(cells[2].format.vertical_align, None);
        assert!(imported.warnings.iter().any(|warning| {
            warning.code == "google-sheets-unsupported-horizontal-alignment"
                && warning.message.contains("C1")
                && warning.message.contains("JUSTIFY")
        }));
        assert!(imported.warnings.iter().any(|warning| {
            warning.code == "google-sheets-unsupported-vertical-alignment"
                && warning.message.contains("C1")
                && warning.message.contains("DISTRIBUTED")
        }));
        assert!(!imported.warnings.iter().any(|warning| {
            warning.code.starts_with("google-sheets-unsupported-") && warning.message.contains("A1")
        }));
    }

    #[test]
    fn google_custom_number_format_pattern_round_trips_without_becoming_a_type() {
        let pattern = "$#,##0.00;[Red]($#,##0.00)";
        let payload = json!({
            "sheets": [{
                "properties": {
                    "sheetId": 1,
                    "title": "Accounting",
                    "gridProperties": { "rowCount": 1, "columnCount": 2 }
                },
                "data": [{ "rowData": [{ "values": [
                    {
                        "userEnteredValue": { "numberValue": 12.5 },
                        "userEnteredFormat": {
                            "numberFormat": { "type": "CURRENCY", "pattern": pattern }
                        }
                    },
                    {
                        "userEnteredValue": { "numberValue": 0.25 },
                        "userEnteredFormat": { "numberFormat": { "type": "PERCENT" } }
                    }
                ] }] }]
            }]
        });

        let imported = import_google_sheets_workbook(&payload.to_string()).unwrap();
        let cells = &imported.workbook.sheets[0].cells;
        assert_eq!(cells[0].format.number_format.as_deref(), Some(pattern));
        assert_eq!(cells[1].format.number_format.as_deref(), Some("PERCENT"));

        let exported: Value =
            serde_json::from_str(&export_google_sheets_workbook(&imported.workbook).unwrap())
                .unwrap();
        let values = &exported["sheets"][0]["data"][0]["rowData"][0]["values"];
        assert_eq!(
            values[0]["userEnteredFormat"]["numberFormat"]["type"],
            "NUMBER"
        );
        assert_eq!(
            values[0]["userEnteredFormat"]["numberFormat"]["pattern"],
            pattern
        );
        assert_eq!(
            values[1]["userEnteredFormat"]["numberFormat"]["type"],
            "PERCENT"
        );
        assert!(values[1]["userEnteredFormat"]["numberFormat"]
            .get("pattern")
            .is_none());

        let reread = import_google_sheets_workbook(&exported.to_string()).unwrap();
        assert_eq!(
            reread.workbook.sheets[0].cells[0].format.number_format,
            imported.workbook.sheets[0].cells[0].format.number_format
        );
        assert_eq!(
            reread.workbook.sheets[0].cells[1].format.number_format,
            imported.workbook.sheets[0].cells[1].format.number_format
        );
    }

    #[test]
    fn google_range_validation_and_note_round_trip_without_coercion() {
        let payload = json!({
            "sheets": [
                {
                    "properties": {
                        "sheetId": 1,
                        "title": "Data",
                        "gridProperties": { "rowCount": 2, "columnCount": 2 }
                    },
                    "data": [{ "rowData": [{ "values": [
                        {
                            "userEnteredValue": { "stringValue": "Red" },
                            "note": "Native Google note",
                            "dataValidation": {
                                "condition": {
                                    "type": "ONE_OF_RANGE",
                                    "values": [{ "userEnteredValue": "Choices!$A$1:$A$2" }]
                                },
                                "strict": true,
                                "showCustomUi": false
                            }
                        },
                        {
                            "userEnteredValue": { "stringValue": "ordinary data" },
                            "dataValidation": {
                                "condition": {
                                    "type": "DATE_AFTER",
                                    "values": [{ "userEnteredValue": "2026-01-01" }]
                                },
                                "strict": true
                            }
                        }
                    ] }] }]
                },
                {
                    "properties": {
                        "sheetId": 2,
                        "title": "Choices",
                        "gridProperties": { "rowCount": 2, "columnCount": 1 }
                    },
                    "data": [{ "rowData": [
                        { "values": [{ "userEnteredValue": { "stringValue": "Red" } }] },
                        { "values": [{ "userEnteredValue": { "stringValue": "Green" } }] }
                    ] }]
                }
            ]
        });

        let imported = import_google_sheets_workbook(&payload.to_string()).unwrap();
        let data = &imported.workbook.sheets[0];
        let a1 = data.cells.iter().find(|cell| cell.address == "A1").unwrap();
        assert_eq!(a1.validation.as_ref().unwrap().kind, "one_of_range");
        assert_eq!(
            a1.validation.as_ref().unwrap().values,
            ["Choices!$A$1:$A$2"]
        );
        assert!(a1.validation.as_ref().unwrap().strict);
        assert!(!a1.validation.as_ref().unwrap().show_dropdown);
        assert_eq!(a1.comments[0].author, "Google Sheets note");
        assert_eq!(a1.comments[0].body, "Native Google note");
        let expected_validation = a1.validation.clone();
        // The unsupported rule did not poison the nearby primitive cell.
        assert!(data.cells.iter().any(|cell| cell.address == "B1"));
        assert!(data
            .cells
            .iter()
            .find(|cell| cell.address == "B1")
            .unwrap()
            .validation
            .is_none());
        assert!(imported.warnings.iter().any(|warning| {
            warning.code == "google-sheets-data-validation-unimported"
                && warning.message.contains("B1")
                && warning.message.contains("DATE_AFTER")
        }));

        let exported: Value =
            serde_json::from_str(&export_google_sheets_workbook(&imported.workbook).unwrap())
                .unwrap();
        let a1 = &exported["sheets"][0]["data"][0]["rowData"][0]["values"][0];
        assert_eq!(a1["dataValidation"]["condition"]["type"], "ONE_OF_RANGE");
        assert_eq!(
            a1["dataValidation"]["condition"]["values"][0]["userEnteredValue"],
            "Choices!$A$1:$A$2"
        );
        assert_eq!(a1["dataValidation"]["strict"], true);
        assert_eq!(a1["dataValidation"]["showCustomUi"], false);
        assert_eq!(a1["note"], "Native Google note");

        let reread = import_google_sheets_workbook(&exported.to_string()).unwrap();
        let reread_a1 = reread.workbook.sheets[0]
            .cells
            .iter()
            .find(|cell| cell.address == "A1")
            .unwrap();
        assert_eq!(reread_a1.validation, expected_validation);
        assert_eq!(reread_a1.comments[0].body, "Native Google note");
    }

    #[test]
    fn google_sheet_properties_and_axis_metadata_round_trip() {
        let payload = json!({
            "sheets": [{
                "properties": {
                    "sheetId": 4,
                    "title": "Metadata",
                    "hidden": true,
                    "tabColorStyle": {
                        "rgbColor": { "red": 0.25, "green": 0.5, "blue": 0.75 }
                    },
                    "gridProperties": {
                        "rowCount": 3,
                        "columnCount": 3,
                        "frozenRowCount": 1,
                        "frozenColumnCount": 2
                    }
                },
                "data": [{
                    "startRow": 0,
                    "startColumn": 0,
                    "rowMetadata": [
                        {},
                        { "pixelSize": 37, "hiddenByUser": true },
                        {}
                    ],
                    "columnMetadata": [
                        {},
                        {},
                        { "pixelSize": 143, "hiddenByUser": true }
                    ],
                    "rowData": [{ "values": [{ "userEnteredValue": { "stringValue": "kept" } }] }]
                }]
            }, {
                "properties": {
                    "sheetId": 5,
                    "title": "Visible companion",
                    "hidden": false,
                    "gridProperties": { "rowCount": 1, "columnCount": 1 }
                }
            }]
        });

        let imported = import_google_sheets_workbook(&payload.to_string()).unwrap();
        let sheet = &imported.workbook.sheets[0];
        assert!(sheet.hidden);
        assert_eq!(sheet.tab_color.as_deref(), Some("#4080bf"));
        assert_eq!(sheet.row_heights.get("2"), Some(&37));
        assert_eq!(sheet.column_widths.get("C"), Some(&143));
        assert_eq!(sheet.hidden_rows, ["2"]);
        assert_eq!(sheet.hidden_columns, ["C"]);
        assert_eq!(sheet.frozen_rows, 1);
        assert_eq!(sheet.frozen_columns, 2);

        let exported: Value =
            serde_json::from_str(&export_google_sheets_workbook(&imported.workbook).unwrap())
                .unwrap();
        let properties = &exported["sheets"][0]["properties"];
        assert_eq!(properties["hidden"], true);
        assert_eq!(
            properties["gridProperties"],
            json!({
                "rowCount": 3,
                "columnCount": 3,
                "frozenRowCount": 1,
                "frozenColumnCount": 2,
            })
        );
        assert_eq!(
            properties["tabColorStyle"],
            json!({ "rgbColor": {
                "red": 64.0 / 255.0,
                "green": 128.0 / 255.0,
                "blue": 191.0 / 255.0,
            }})
        );
        assert!(properties.get("tabColor").is_none());
        let data = &exported["sheets"][0]["data"][0];
        assert_eq!(
            data["rowMetadata"][1],
            json!({ "pixelSize": 37, "hiddenByUser": true })
        );
        assert_eq!(
            data["columnMetadata"][2],
            json!({ "pixelSize": 143, "hiddenByUser": true })
        );

        let reread = import_google_sheets_workbook(&exported.to_string()).unwrap();
        let reread = &reread.workbook.sheets[0];
        assert!(reread.hidden);
        assert_eq!(reread.tab_color, sheet.tab_color);
        assert_eq!(reread.row_heights, sheet.row_heights);
        assert_eq!(reread.column_widths, sheet.column_widths);
        assert_eq!(reread.hidden_rows, sheet.hidden_rows);
        assert_eq!(reread.hidden_columns, sheet.hidden_columns);
        assert_eq!(reread.frozen_rows, sheet.frozen_rows);
        assert_eq!(reread.frozen_columns, sheet.frozen_columns);
    }

    #[test]
    fn google_sheet_visibility_requires_a_visible_tab_at_both_native_boundaries() {
        let all_hidden = json!({
            "sheets": [{
                "properties": {
                    "sheetId": 7,
                    "title": "Hidden",
                    "hidden": true,
                    "gridProperties": { "rowCount": 1, "columnCount": 1 }
                }
            }]
        });
        let error = match import_google_sheets_workbook(&all_hidden.to_string()) {
            Ok(_) => panic!("an all-hidden native workbook is invalid"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("every sheet hidden"));
        assert!(error.to_string().contains("at least one visible sheet"));

        let mut workbook = SpreadsheetWorkbook::sample();
        workbook.title = "Hidden export".to_string();
        workbook.sheets[0].hidden = true;
        let error = export_google_sheets_workbook(&workbook).unwrap_err();
        assert!(error.to_string().contains("every OpenDoc sheet is hidden"));
        assert!(error.to_string().contains("at least one visible sheet"));
    }

    #[test]
    fn google_current_tab_color_style_beats_deprecated_tab_color() {
        let payload = json!({
            "sheets": [{
                "properties": {
                    "sheetId": 0,
                    "title": "Current style",
                    "tabColor": { "red": 1.0, "green": 0.0, "blue": 0.0 },
                    "tabColorStyle": {
                        "rgbColor": { "red": 0.0, "green": 0.5, "blue": 1.0 }
                    },
                    "gridProperties": { "rowCount": 1, "columnCount": 1 }
                }
            }]
        });

        let imported = import_google_sheets_workbook(&payload.to_string()).unwrap();
        assert_eq!(
            imported.workbook.sheets[0].tab_color.as_deref(),
            Some("#0080ff")
        );

        let exported: Value =
            serde_json::from_str(&export_google_sheets_workbook(&imported.workbook).unwrap())
                .unwrap();
        let properties = &exported["sheets"][0]["properties"];
        assert_eq!(
            properties["tabColorStyle"]["rgbColor"],
            json!({ "red": 0.0, "green": 128.0 / 255.0, "blue": 1.0 })
        );
        assert!(properties.get("tabColor").is_none());
    }

    #[test]
    fn google_deprecated_tab_color_remains_a_read_only_compatibility_input() {
        let payload = json!({
            "sheets": [{
                "properties": {
                    "sheetId": 0,
                    "title": "Legacy",
                    "tabColor": { "red": 1.0, "green": 0.0, "blue": 0.0 },
                    "gridProperties": { "rowCount": 1, "columnCount": 1 }
                }
            }]
        });

        let imported = import_google_sheets_workbook(&payload.to_string()).unwrap();
        assert_eq!(
            imported.workbook.sheets[0].tab_color.as_deref(),
            Some("#ff0000")
        );
        let exported: Value =
            serde_json::from_str(&export_google_sheets_workbook(&imported.workbook).unwrap())
                .unwrap();
        let properties = &exported["sheets"][0]["properties"];
        assert_eq!(
            properties["tabColorStyle"]["rgbColor"],
            json!({ "red": 1.0, "green": 0.0, "blue": 0.0 })
        );
        assert!(properties.get("tabColor").is_none());
    }

    #[test]
    fn google_themed_tab_color_does_not_fall_back_to_deprecated_rgb() {
        let payload = json!({
            "sheets": [{
                "properties": {
                    "sheetId": 0,
                    "title": "Themed",
                    "tabColor": { "red": 1.0, "green": 0.0, "blue": 0.0 },
                    "tabColorStyle": { "themeColor": "ACCENT1" },
                    "gridProperties": { "rowCount": 1, "columnCount": 1 }
                }
            }]
        });

        let imported = import_google_sheets_workbook(&payload.to_string()).unwrap();
        assert!(imported.workbook.sheets[0].tab_color.is_none());
        assert!(imported
            .warnings
            .iter()
            .any(|warning| { warning.code == "google-sheets-unsupported-tab-theme-color" }));
    }

    #[test]
    fn google_unmodelled_nondefault_grid_properties_are_disclosed() {
        let payload = json!({
            "sheets": [{
                "properties": {
                    "gridProperties": {
                        "rowCount": 2,
                        "columnCount": 2,
                        "hideGridlines": true,
                        "rowGroupControlAfter": true,
                        "columnGroupControlAfter": true
                    }
                }
            }]
        });

        let imported = import_google_sheets_workbook(&payload.to_string()).unwrap();
        assert_eq!(imported.workbook.sheets[0].rows.len(), 2);
        assert_eq!(imported.workbook.sheets[0].columns.len(), 2);
        let warning = imported
            .warnings
            .iter()
            .find(|warning| warning.code == "google-sheets-unsupported-grid-properties")
            .expect("one combined grid-properties loss warning");
        assert!(warning.message.contains("hideGridlines"));
        assert!(warning.message.contains("rowGroupControlAfter"));
        assert!(warning.message.contains("columnGroupControlAfter"));
    }

    #[test]
    fn google_non_wrap_strategy_is_disclosed_not_coerced() {
        let payload = json!({ "sheets": [{
            "properties": { "gridProperties": { "rowCount": 1, "columnCount": 1 } },
            "data": [{ "rowData": [{ "values": [{
                "userEnteredValue": { "stringValue": "plain" },
                "userEnteredFormat": { "wrapStrategy": "OVERFLOW_CELL" }
            }] }] }]
        }] });
        let imported = import_google_sheets_workbook(&payload.to_string()).unwrap();
        assert_eq!(
            imported.workbook.sheets[0].cells[0].format.wrap_strategy,
            None
        );
        assert!(imported.warnings.iter().any(|warning| {
            warning.code == "google-sheets-unsupported-wrap-strategy"
                && warning.message.contains("A1")
        }));
    }

    #[test]
    fn unsupported_cell_feature_has_one_family_warning_with_all_locations() {
        let payload = json!({
            "sheets": [{
                "properties": { "gridProperties": { "rowCount": 1, "columnCount": 2 } },
                "data": [{ "rowData": [{ "values": [
                    { "userEnteredValue": { "stringValue": "one" }, "chipRuns": [] },
                    { "userEnteredValue": { "stringValue": "two" }, "chipRuns": [] }
                ] }] }]
            }]
        });
        let imported = import_google_sheets_workbook(&payload.to_string()).unwrap();
        let chip_warnings = imported
            .warnings
            .iter()
            .filter(|warning| warning.code == "google-sheets-unsupported-chip-runs")
            .collect::<Vec<_>>();
        assert_eq!(chip_warnings.len(), 1);
        assert!(chip_warnings[0].message.contains("rowData[0].values[0]"));
        assert!(chip_warnings[0].message.contains("rowData[0].values[1]"));
        assert_eq!(imported.workbook.sheets[0].cells.len(), 2);
    }

    #[test]
    fn google_sheet_properties_index_controls_tab_order_and_round_trips() {
        // The response array is deliberately not tab order. Named ranges also
        // prove that reordering happens before cross-sheet identities resolve.
        let payload = json!({
            "sheets": [
                { "properties": { "sheetId": 41, "index": 1, "title": "Second", "gridProperties": { "rowCount": 1, "columnCount": 1 } } },
                { "properties": { "sheetId": 12, "index": 0, "title": "First", "gridProperties": { "rowCount": 1, "columnCount": 1 } } }
            ],
            "namedRanges": [{
                "namedRangeId": "first-cell",
                "name": "FirstCell",
                "range": { "sheetId": 12, "endRowIndex": 1, "endColumnIndex": 1 }
            }]
        });

        let imported = import_google_sheets_workbook(&payload.to_string()).unwrap();
        assert_eq!(
            imported
                .workbook
                .sheets
                .iter()
                .map(|sheet| (sheet.id.as_str(), sheet.title.as_str()))
                .collect::<Vec<_>>(),
            [("sheet-1", "First"), ("sheet-2", "Second")]
        );
        assert_eq!(imported.workbook.named_ranges[0].sheet_id, "sheet-1");

        let exported: Value =
            serde_json::from_str(&export_google_sheets_workbook(&imported.workbook).unwrap())
                .unwrap();
        assert_eq!(exported["sheets"][0]["properties"]["index"], json!(0));
        assert_eq!(exported["sheets"][1]["properties"]["index"], json!(1));
        let reread = import_google_sheets_workbook(&exported.to_string()).unwrap();
        assert_eq!(
            reread
                .workbook
                .sheets
                .iter()
                .map(|sheet| sheet.title.as_str())
                .collect::<Vec<_>>(),
            ["First", "Second"]
        );
    }

    #[test]
    fn google_sheet_properties_index_must_be_complete_unique_and_contiguous() {
        let duplicate = json!({ "sheets": [
            { "properties": { "index": 0 } },
            { "properties": { "index": 0 } }
        ] });
        let error = match import_google_sheets_workbook(&duplicate.to_string()) {
            Ok(_) => panic!("duplicate tab indexes must be rejected"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("unique contiguous tab order"));

        let mixed = json!({ "sheets": [
            { "properties": { "index": 0 } },
            { "properties": {} }
        ] });
        let error = match import_google_sheets_workbook(&mixed.to_string()) {
            Ok(_) => panic!("mixed indexed and unindexed sheets must be rejected"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("index is missing on part"));
    }
}
