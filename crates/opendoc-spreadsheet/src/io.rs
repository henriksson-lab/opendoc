//! CSV/TSV and XLSX interchange.

use std::collections::BTreeSet;
use std::io::Cursor;

use base64::Engine;
use calamine::{Data, Reader};

use super::address::{
    column_to_number, normalize_cell_range, normalize_named_range_name, number_to_column,
    parse_cell_range, split_cell_address, trim_number,
};
use super::format::{self, Locale};
use crate::{
    model::{column_axis, row_axis},
    Cell, NamedRange, Sheet, SheetMerge, SpreadsheetError, SpreadsheetWorkbook,
};

/// Default grid for newly created sheets.
pub const DEFAULT_SHEET_ROWS: u32 = 100;
pub const DEFAULT_SHEET_COLUMNS: u32 = 26;

pub fn default_row_labels(count: u32) -> Vec<String> {
    (1..=count.max(1)).map(|row| row.to_string()).collect()
}

pub fn default_column_labels(count: u32) -> Vec<String> {
    (1..=count.max(1)).filter_map(number_to_column).collect()
}

/// Builds an empty sheet with the default grid.
pub fn blank_sheet(id: &str, title: &str, rows: u32, columns: u32) -> Sheet {
    let rows = default_row_labels(rows);
    let columns = default_column_labels(columns);
    Sheet {
        id: id.to_string(),
        title: title.to_string(),
        frozen_rows: 0,
        frozen_columns: 0,
        merges: Vec::new(),
        filters: Vec::new(),
        protected_ranges: Vec::new(),
        row_axes: rows.iter().cloned().map(row_axis).collect(),
        column_axes: columns.iter().cloned().map(column_axis).collect(),
        rows,
        columns,
        cells: Vec::new(),
        row_heights: Default::default(),
        column_widths: Default::default(),
        hidden_rows: Vec::new(),
        hidden_columns: Vec::new(),
        hidden: false,
        tab_color: None,
    }
}

/// Parses delimiter text (`","`, `"\t"`, `"tab"`, `";"`).
pub fn parse_delimiter(value: Option<&str>) -> Result<u8, SpreadsheetError> {
    let value = value.unwrap_or(",");
    let delimiter = match value {
        "\\t" | "tab" | "TAB" | "\t" => b'\t',
        other => {
            let mut chars = other.chars();
            let first = chars
                .next()
                .ok_or_else(|| SpreadsheetError::Format("csv delimiter is empty".to_string()))?;
            if chars.next().is_some() || !first.is_ascii() {
                return Err(SpreadsheetError::Format(format!(
                    "csv delimiter {other:?} must be a single ASCII character"
                )));
            }
            first as u8
        }
    };
    Ok(delimiter)
}

/// Parses CSV/TSV text into rows of cell text.
pub fn parse_csv(text: &str, delimiter: u8) -> Result<Vec<Vec<String>>, SpreadsheetError> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .delimiter(delimiter)
        .from_reader(text.as_bytes());
    let mut rows = Vec::new();
    for record in reader.records() {
        let record =
            record.map_err(|err| SpreadsheetError::Import(format!("invalid csv: {err}")))?;
        rows.push(record.iter().map(str::to_string).collect());
    }
    Ok(rows)
}

/// Serializes a sheet's populated grid as CSV/TSV using display values.
pub fn export_csv(
    sheet: &Sheet,
    delimiter: u8,
    locale: &Locale,
) -> Result<String, SpreadsheetError> {
    let mut max_col = 0u32;
    let mut max_row = 0u32;
    let mut cells = std::collections::HashMap::new();
    for cell in &sheet.cells {
        if cell.user_kind == "empty" && cell.spill_source.is_none() {
            continue;
        }
        let (column, row) = split_cell_address(&cell.address);
        let (Some(col), Ok(row)) = (column_to_number(&column), row.parse::<u32>()) else {
            continue;
        };
        max_col = max_col.max(col);
        max_row = max_row.max(row);
        cells.insert((col, row), cell);
    }
    let mut writer = csv::WriterBuilder::new()
        .delimiter(delimiter)
        .from_writer(Vec::new());
    for row in 1..=max_row {
        let mut record = Vec::with_capacity(max_col as usize);
        for col in 1..=max_col {
            let text = match cells.get(&(col, row)) {
                Some(cell) => {
                    if cell.display_value.is_empty() {
                        format::display_value(
                            &cell.computed_kind,
                            &cell.computed_value,
                            cell.format.number_format.as_deref(),
                            locale,
                        )
                    } else {
                        cell.display_value.clone()
                    }
                }
                None => String::new(),
            };
            record.push(text);
        }
        writer
            .write_record(&record)
            .map_err(|err| SpreadsheetError::Format(format!("csv export failed: {err}")))?;
    }
    let bytes = writer
        .into_inner()
        .map_err(|err| SpreadsheetError::Format(format!("csv export failed: {err}")))?;
    String::from_utf8(bytes)
        .map_err(|err| SpreadsheetError::Format(format!("csv export failed: {err}")))
}

pub fn decode_base64(text: &str) -> Result<Vec<u8>, SpreadsheetError> {
    let cleaned: String = text.chars().filter(|ch| !ch.is_whitespace()).collect();
    base64::engine::general_purpose::STANDARD
        .decode(cleaned.as_bytes())
        .map_err(|err| SpreadsheetError::Format(format!("invalid base64: {err}")))
}

pub fn encode_base64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn unique_title(base: &str, taken: &BTreeSet<String>) -> String {
    let base = base.trim();
    let base = if base.is_empty() { "Sheet" } else { base };
    if !taken.contains(base) {
        return base.to_string();
    }
    let mut suffix = 2;
    loop {
        let candidate = format!("{base} ({suffix})");
        if !taken.contains(&candidate) {
            return candidate;
        }
        suffix += 1;
    }
}

fn xlsx_error_code(error: &calamine::CellErrorType) -> &'static str {
    match error {
        calamine::CellErrorType::Div0 => "#DIV/0!",
        calamine::CellErrorType::NA => "#N/A",
        calamine::CellErrorType::Name => "#NAME?",
        calamine::CellErrorType::Null => "#NULL!",
        calamine::CellErrorType::Num => "#NUM!",
        calamine::CellErrorType::Ref => "#REF!",
        calamine::CellErrorType::Value => "#VALUE!",
        calamine::CellErrorType::GettingData => "#N/A",
    }
}

/// Imports an XLSX workbook (values, formulas, sheet names, merges,
/// date formats) into an OpenDoc workbook.
pub fn import_xlsx(bytes: &[u8], title: &str) -> Result<SpreadsheetWorkbook, SpreadsheetError> {
    let cursor = Cursor::new(bytes.to_vec());
    let mut reader: calamine::Xlsx<_> = calamine::open_workbook_from_rs(cursor)
        .map_err(|err| SpreadsheetError::Import(format!("invalid xlsx: {err}")))?;
    let _ = reader.load_merged_regions();
    let sheet_names = reader.sheet_names();
    if sheet_names.is_empty() {
        return Err(SpreadsheetError::Import(
            "xlsx workbook has no sheets".to_string(),
        ));
    }
    let mut workbook = SpreadsheetWorkbook::empty(title);
    let mut titles = BTreeSet::new();
    for (index, name) in sheet_names.iter().enumerate() {
        let range = reader
            .worksheet_range(name)
            .map_err(|err| SpreadsheetError::Import(format!("invalid xlsx sheet {name}: {err}")))?;
        let formulas = reader.worksheet_formula(name).ok();
        let (rows, columns) = range
            .end()
            .map(|(row, col)| (row + 1, col + 1))
            .unwrap_or((0, 0));
        let sheet_id = format!("sheet-{}", index + 1);
        let sheet_title = unique_title(name, &titles);
        titles.insert(sheet_title.clone());
        let mut sheet = blank_sheet(
            &sheet_id,
            &sheet_title,
            rows.max(DEFAULT_SHEET_ROWS),
            columns.max(DEFAULT_SHEET_COLUMNS),
        );
        let start = range.start().unwrap_or((0, 0));
        for (row, col, value) in range.used_cells() {
            let abs_row = start.0 as usize + row;
            let abs_col = start.1 as usize + col;
            let Some(column) = number_to_column(abs_col as u32 + 1) else {
                continue;
            };
            let address = format!("{column}{}", abs_row + 1);
            let mut cell = match value {
                Data::Empty => continue,
                Data::Int(value) => Cell::new(&address, "number", &value.to_string()),
                Data::Float(value) => Cell::new(&address, "number", &trim_number(*value)),
                Data::String(text) => Cell::new(&address, "string", text),
                Data::Bool(flag) => {
                    Cell::new(&address, "bool", if *flag { "true" } else { "false" })
                }
                Data::DateTime(value) => {
                    let serial = value.as_f64();
                    let mut cell = Cell::new(&address, "number", &trim_number(serial));
                    cell.format.number_format = Some(if serial.fract() == 0.0 {
                        "DATE".to_string()
                    } else {
                        "DATE_TIME".to_string()
                    });
                    cell
                }
                Data::DateTimeIso(text) | Data::DurationIso(text) => {
                    Cell::new(&address, "string", text)
                }
                Data::Error(error) => Cell::new(&address, "string", xlsx_error_code(error)),
            };
            if let Some(formulas) = &formulas {
                let formula_start = formulas.start().unwrap_or((0, 0));
                let position = (abs_row as u32, abs_col as u32);
                if position.0 >= formula_start.0 && position.1 >= formula_start.1 {
                    if let Some(formula) = formulas.get_value(position) {
                        if !formula.trim().is_empty() {
                            cell.user_kind = "formula".to_string();
                            cell.user_value = format!("={}", formula.trim_start_matches('='));
                        }
                    }
                }
            }
            sheet.ensure_address(&address);
            sheet.cells.push(cell);
        }
        for (_, _, dimensions) in reader.merged_regions_by_sheet(name) {
            let (Some(start_col), Some(end_col)) = (
                number_to_column(dimensions.start.1 + 1),
                number_to_column(dimensions.end.1 + 1),
            ) else {
                continue;
            };
            let range = format!(
                "{start_col}{}:{end_col}{}",
                dimensions.start.0 + 1,
                dimensions.end.0 + 1
            );
            if let Ok(range) = normalize_cell_range(&range) {
                if range.contains(':') {
                    sheet.merges.push(SheetMerge {
                        id: format!("merge-{}", range.replace(':', "-").to_ascii_lowercase()),
                        range,
                    });
                }
            }
        }
        sheet
            .cells
            .sort_by(|left, right| left.address.cmp(&right.address));
        sheet.ensure_axis_metadata();
        workbook.sheets.push(sheet);
    }
    // Defined names resolve against the sheet titles just imported, so this
    // runs once every sheet exists.
    workbook.named_ranges = import_defined_names(&reader, &workbook);
    Ok(workbook)
}

fn xlsx_color(value: &str) -> Option<u32> {
    u32::from_str_radix(value.trim_start_matches('#'), 16).ok()
}

fn xlsx_format(cell: &Cell) -> Option<rust_xlsxwriter::Format> {
    let format = &cell.format;
    if !format.bold
        && !format.italic
        && format.text_color.is_none()
        && format.background_color.is_none()
        && format.horizontal_align.is_none()
        && format.number_format.is_none()
    {
        return None;
    }
    let mut out = rust_xlsxwriter::Format::new();
    if format.bold {
        out = out.set_bold();
    }
    if format.italic {
        out = out.set_italic();
    }
    if let Some(color) = format.text_color.as_deref().and_then(xlsx_color) {
        out = out.set_font_color(color);
    }
    if let Some(color) = format.background_color.as_deref().and_then(xlsx_color) {
        out = out.set_background_color(color);
    }
    match format.horizontal_align.as_deref() {
        Some("left") => out = out.set_align(rust_xlsxwriter::FormatAlign::Left),
        Some("center") => out = out.set_align(rust_xlsxwriter::FormatAlign::Center),
        Some("right") => out = out.set_align(rust_xlsxwriter::FormatAlign::Right),
        _ => {}
    }
    if let Some(number_format) = &format.number_format {
        let pattern = match number_format.to_ascii_uppercase().as_str() {
            "GENERAL" | "AUTOMATIC" => None,
            "TEXT" => Some("@".to_string()),
            "NUMBER" => Some("#,##0.00".to_string()),
            "PERCENT" => Some("0.00%".to_string()),
            "CURRENCY" => Some("$#,##0.00".to_string()),
            "DATE" => Some("yyyy-mm-dd".to_string()),
            "TIME" => Some("h:mm:ss AM/PM".to_string()),
            "DATE_TIME" | "DATETIME" => Some("yyyy-mm-dd h:mm:ss".to_string()),
            "SCIENTIFIC" => Some("0.00E+00".to_string()),
            _ => Some(number_format.clone()),
        };
        if let Some(pattern) = pattern {
            out = out.set_num_format(pattern);
        }
    }
    Some(out)
}

fn xlsx_err(err: rust_xlsxwriter::XlsxError) -> SpreadsheetError {
    SpreadsheetError::Format(format!("xlsx export failed: {err}"))
}

/// Exports the workbook as XLSX bytes (values, formulas, basic formatting,
/// sheet names, column widths, row heights, hidden axes, frozen panes,
/// merges, and named ranges).
pub fn export_xlsx(workbook: &SpreadsheetWorkbook) -> Result<Vec<u8>, SpreadsheetError> {
    let mut book = rust_xlsxwriter::Workbook::new();
    let mut sheet_titles = Vec::new();
    for sheet in &workbook.sheets {
        let worksheet = book.add_worksheet();
        let title: String = sheet
            .title
            .chars()
            .filter(|ch| !matches!(ch, '[' | ']' | ':' | '*' | '?' | '/' | '\\'))
            .take(31)
            .collect();
        let title = if title.trim().is_empty() {
            format!("Sheet{}", sheet_titles.len() + 1)
        } else {
            title
        };
        worksheet.set_name(&title).map_err(xlsx_err)?;
        sheet_titles.push(title);
        if sheet.hidden {
            worksheet.set_hidden(true);
        }
        if let Some(color) = sheet.tab_color.as_deref().and_then(xlsx_color) {
            worksheet.set_tab_color(color);
        }
        if sheet.frozen_rows > 0 || sheet.frozen_columns > 0 {
            worksheet
                .set_freeze_panes(sheet.frozen_rows, sheet.frozen_columns as u16)
                .map_err(xlsx_err)?;
        }
        for (label, width) in &sheet.column_widths {
            if let Some(col) = column_to_number(label) {
                worksheet
                    .set_column_width((col - 1) as u16, *width as f64 / 7.0)
                    .map_err(xlsx_err)?;
            }
        }
        for (label, height) in &sheet.row_heights {
            if let Ok(row) = label.parse::<u32>() {
                worksheet
                    .set_row_height(row - 1, *height as f64 * 0.75)
                    .map_err(xlsx_err)?;
            }
        }
        for label in &sheet.hidden_columns {
            if let Some(col) = column_to_number(label) {
                worksheet
                    .set_column_hidden((col - 1) as u16)
                    .map_err(xlsx_err)?;
            }
        }
        for label in &sheet.hidden_rows {
            if let Ok(row) = label.parse::<u32>() {
                worksheet.set_row_hidden(row - 1).map_err(xlsx_err)?;
            }
        }
        for cell in &sheet.cells {
            let (column, row) = split_cell_address(&cell.address);
            let (Some(col), Ok(row)) = (column_to_number(&column), row.parse::<u32>()) else {
                continue;
            };
            let (row, col) = (row - 1, (col - 1) as u16);
            let format = xlsx_format(cell);
            match cell.user_kind.as_str() {
                "number" => {
                    let value = cell.user_value.parse::<f64>().unwrap_or(0.0);
                    match &format {
                        Some(format) => worksheet.write_number_with_format(row, col, value, format),
                        None => worksheet.write_number(row, col, value),
                    }
                    .map_err(xlsx_err)?;
                }
                "bool" => {
                    let value = cell.user_value.eq_ignore_ascii_case("true");
                    match &format {
                        Some(format) => {
                            worksheet.write_boolean_with_format(row, col, value, format)
                        }
                        None => worksheet.write_boolean(row, col, value),
                    }
                    .map_err(xlsx_err)?;
                }
                "formula" => {
                    let mut formula = rust_xlsxwriter::Formula::new(&cell.user_value);
                    if cell.computed_kind != "error" && !cell.computed_value.is_empty() {
                        formula = formula.set_result(&cell.computed_value);
                    }
                    match &format {
                        Some(format) => {
                            worksheet.write_formula_with_format(row, col, formula, format)
                        }
                        None => worksheet.write_formula(row, col, formula),
                    }
                    .map_err(xlsx_err)?;
                }
                "string" => {
                    match &format {
                        Some(format) => {
                            worksheet.write_string_with_format(row, col, &cell.user_value, format)
                        }
                        None => worksheet.write_string(row, col, &cell.user_value),
                    }
                    .map_err(xlsx_err)?;
                }
                _ => {
                    if let Some(format) = &format {
                        worksheet.write_blank(row, col, format).map_err(xlsx_err)?;
                    }
                }
            }
        }
        for merge in &sheet.merges {
            let Ok(range) = parse_cell_range(&merge.range) else {
                continue;
            };
            let first_row = range.start_row - 1;
            let first_col = (range.start_column - 1) as u16;
            let last_row = first_row + range.height - 1;
            let last_col = first_col + (range.width - 1) as u16;
            let anchor = sheet
                .cells
                .iter()
                .find(|cell| cell.address == merge.range.split(':').next().unwrap_or_default());
            let text = anchor
                .map(|cell| cell.user_value.clone())
                .unwrap_or_default();
            let format = anchor.and_then(xlsx_format).unwrap_or_default();
            worksheet
                .merge_range(first_row, first_col, last_row, last_col, &text, &format)
                .map_err(xlsx_err)?;
        }
    }
    for named in &workbook.named_ranges {
        let Some(index) = workbook
            .sheets
            .iter()
            .position(|sheet| sheet.id == named.sheet_id)
        else {
            continue;
        };
        let title = &sheet_titles[index];
        let absolute = named
            .range
            .split(':')
            .map(|part| {
                let (column, row) = split_cell_address(part);
                format!("${column}${row}")
            })
            .collect::<Vec<_>>()
            .join(":");
        let _ = book.define_name(
            &named.name,
            &format!("='{}'!{absolute}", title.replace('\'', "''")),
        );
    }
    book.save_to_buffer().map_err(xlsx_err)
}

/// Named-range helper used by the XLSX importer for defined names (kept
/// minimal: calamine exposes defined names via `Xlsx::defined_names`).
pub fn import_defined_names(
    reader: &calamine::Xlsx<Cursor<Vec<u8>>>,
    workbook: &SpreadsheetWorkbook,
) -> Vec<NamedRange> {
    let mut out = Vec::new();
    for (name, formula) in reader.defined_names() {
        let Some((sheet, range)) = formula.trim_start_matches('=').rsplit_once('!') else {
            continue;
        };
        let sheet = sheet.trim_matches('\'');
        let Some(sheet) = workbook.sheets.iter().find(|item| item.title == sheet) else {
            continue;
        };
        let Ok(range) = normalize_cell_range(&range.replace('$', "")) else {
            continue;
        };
        let Ok(name) = normalize_named_range_name(name) else {
            continue;
        };
        out.push(NamedRange {
            id: format!("named-{}", name.to_ascii_lowercase()),
            name,
            sheet_id: sheet.id.clone(),
            range,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use crate::SpreadsheetWorkbook;

    fn user_value(workbook: &SpreadsheetWorkbook, sheet: usize, address: &str) -> String {
        workbook.sheets[sheet]
            .cells
            .iter()
            .find(|cell| cell.address == address)
            .map(|cell| cell.user_value.clone())
            .unwrap_or_default()
    }

    #[test]
    fn csv_cell_edits_land_at_the_origin_and_honour_the_delimiter() {
        let edits = SpreadsheetWorkbook::csv_cell_edits("b2", "a,b\n1,2\n", None).unwrap();

        assert_eq!(
            edits,
            vec![
                ("B2".to_string(), "a".to_string()),
                ("C2".to_string(), "b".to_string()),
                ("B3".to_string(), "1".to_string()),
                ("C3".to_string(), "2".to_string()),
            ]
        );

        let tabbed = SpreadsheetWorkbook::csv_cell_edits("A1", "a\tb", Some("tab")).unwrap();
        assert_eq!(tabbed.len(), 2);
        // Quoted separators stay inside one cell.
        let quoted = SpreadsheetWorkbook::csv_cell_edits("A1", "\"x,y\",z", None).unwrap();
        assert_eq!(quoted[0].1, "x,y");
        assert!(SpreadsheetWorkbook::csv_cell_edits("A1", "a", Some("<>")).is_err());
    }

    #[test]
    fn export_csv_writes_display_text_not_raw_values() {
        let mut workbook = SpreadsheetWorkbook::sample();
        workbook
            .set_cell_in_sheet("sheet-1", "C1", "0.25".to_string())
            .unwrap();
        workbook
            .set_cell_format("sheet-1", "C1", "number_format", "percent".to_string())
            .unwrap()
            .unwrap();
        let workbook = workbook.evaluated();

        let csv = workbook.export_csv("sheet-1", None).unwrap();

        assert!(csv.starts_with("Item,Count,25.00%\n"), "{csv}");
        // The formula in B3 exports as its computed value.
        assert!(csv.contains("\nTotal,5,"), "{csv}");
        assert!(workbook.export_csv("missing", None).is_err());
    }

    #[test]
    fn xlsx_round_trips_values_formulas_formats_and_named_ranges() {
        let mut workbook = SpreadsheetWorkbook::sample();
        workbook
            .set_cell_in_sheet("sheet-1", "C1", "0.25".to_string())
            .unwrap();
        workbook
            .set_cell_format("sheet-1", "C1", "bold", "true".to_string())
            .unwrap()
            .unwrap();
        workbook
            .set_column_width("sheet-1", "B", 200)
            .unwrap()
            .unwrap();
        workbook.set_frozen_axes("sheet-1", 1, 0).unwrap();
        workbook
            .add_named_range("sheet-1", "Counts", "B1:B3")
            .unwrap()
            .unwrap();
        let workbook = workbook.evaluated();

        let base64 = workbook.to_xlsx_base64().unwrap();
        let reopened = SpreadsheetWorkbook::from_xlsx_base64(&base64, "Reopened").unwrap();

        assert_eq!(reopened.title, "Reopened");
        assert_eq!(user_value(&reopened, 0, "A1"), "Item");
        assert_eq!(user_value(&reopened, 0, "C1"), "0.25");
        assert_eq!(user_value(&reopened, 0, "B3"), "=SUM(B2:B2)");
        // Cell styles are deliberately not asserted: the exporter writes
        // them, but calamine's value reader does not read styles back, so
        // formatting is a known one-way trip today.
        // Defined names survive, which is only true because the importer
        // reads them back.
        assert_eq!(reopened.named_ranges.len(), 1);
        assert_eq!(reopened.named_ranges[0].name, "COUNTS");
        assert_eq!(reopened.named_ranges[0].range, "B1:B3");
    }

    #[test]
    fn invalid_xlsx_bytes_are_rejected() {
        assert!(SpreadsheetWorkbook::from_xlsx_base64("bm90IGEgemlw", "Bad").is_err());
        assert!(SpreadsheetWorkbook::from_xlsx_base64("not base64!!", "Bad").is_err());
    }
}
