//! CSV/TSV and XLSX interchange.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read};

use base64::Engine;
use calamine::{Data, Reader};
use quick_xml::{events::Event, Reader as XmlReader};

use super::address::{
    column_to_number, normalize_cell_range, normalize_named_range_name, number_to_column,
    parse_cell_range, split_cell_address, trim_number,
};
use super::format::{self, Locale};
use crate::{
    model::{column_axis, row_axis, MAX_AXIS_SIZE_PX, MIN_AXIS_SIZE_PX},
    push_unique_warning, Cell, CellComment, CellValidation, NamedRange, Sheet, SheetFilter,
    SheetFilterCriterion, SheetMerge, SheetPrintOrientation, SpreadsheetError, SpreadsheetWarning,
    SpreadsheetWorkbook,
};

/// Default grid for newly created sheets.
pub const DEFAULT_SHEET_ROWS: u32 = 100;
pub const DEFAULT_SHEET_COLUMNS: u32 = 26;

/// Largest grid an `.xlsx` import will materialise, matching the limits the
/// Google Sheets importer already enforces
/// (`GOOGLE_SHEETS_IMPORT_MAX_ROWS` / `_COLUMNS`).
///
/// The grid is dense: a sheet whose single used cell sits at row 65,000
/// still materialises 65,000 row labels and 65,000 axis entries, so a 5 KB
/// file could otherwise buy minutes of work. The limit is a clean import
/// error, never a silent truncation — a user told their workbook is too
/// large is better off than one whose rows quietly vanish.
pub const XLSX_IMPORT_MAX_ROWS: u32 = 10_000;
pub const XLSX_IMPORT_MAX_COLUMNS: u32 = 1_000;

/// The value model produced by an XLSX import, together with facts from the
/// package which the spreadsheet model deliberately cannot own.
///
/// Keeping these warnings beside the import (rather than smuggling them into
/// formula-evaluation warnings) lets the application show a source-loss fact
/// once, without claiming that a floating drawing became a cell object.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XlsxImportReport {
    pub workbook: SpreadsheetWorkbook,
    pub warnings: Vec<SpreadsheetWarning>,
}

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
        images: Vec::new(),
        row_heights: Default::default(),
        column_widths: Default::default(),
        hidden_rows: Vec::new(),
        hidden_columns: Vec::new(),
        hidden: false,
        tab_color: None,
        print_settings: Default::default(),
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

/// Largest amount of one worksheet part the pre-flight will inflate looking
/// for a shared-string reference. Generous, and bounded: this runs only for a
/// package that has no shared-string table at all.
const XLSX_PREFLIGHT_SCAN_BYTES: u64 = 4 * 1024 * 1024;

/// Refuses a package whose sheets name a shared string while the package
/// carries no shared-string table.
///
/// `calamine` 0.31 trusts the relationship graph: with no
/// `xl/sharedStrings.xml` it builds an empty table and then indexes it
/// directly for a cell of type `s` (`cells_reader.rs`, "index out of bounds:
/// the len is 0 but the index is 0"). That is a **panic**, not an error, and
/// a panic is not an error path in either runtime — on wasm32 it is an
/// unrecoverable module trap, and in the native shell it unwinds out of a
/// Tauri command. A fuzz target reached it with one flipped byte in a zip
/// entry name (`xl/sharedStrings.xml` → `xl/sharedStr)ngs.xml`), so the
/// reachability is opening a file someone sent you.
///
/// The check is the panic's own precondition rather than a guess at it: a
/// package with a shared-string table is left alone, and one without is only
/// refused if a sheet actually references the table. A workbook of nothing
/// but numbers legitimately has no such part, and still imports.
///
/// `catch_unwind` was the other candidate and is not a fix: it catches
/// nothing on `wasm32-unknown-unknown`, which is the runtime where a panic
/// costs the most.
fn refuse_dangling_shared_strings(bytes: &[u8]) -> Result<(), SpreadsheetError> {
    use std::io::Read;

    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|err| SpreadsheetError::Import(format!("invalid xlsx: {err}")))?;
    let names: Vec<String> = archive.file_names().map(ToString::to_string).collect();
    if names.iter().any(|name| name == "xl/sharedStrings.xml") {
        return Ok(());
    }
    for name in names
        .iter()
        .filter(|name| name.starts_with("xl/worksheets/") && name.ends_with(".xml"))
    {
        let Ok(part) = archive.by_name(name) else {
            continue;
        };
        let mut body = Vec::new();
        if part
            .take(XLSX_PREFLIGHT_SCAN_BYTES)
            .read_to_end(&mut body)
            .is_err()
        {
            continue;
        }
        // A cell of type `s` is a shared-string reference. Both spellings,
        // because the attribute quoting is the writer's choice.
        let references_shared_string = body
            .windows(5)
            .any(|window| window == br#"t="s""# || window == b"t='s'");
        if references_shared_string {
            return Err(SpreadsheetError::Import(format!(
                "xlsx sheet part {name} names a shared string, but the package has no xl/sharedStrings.xml"
            )));
        }
    }
    Ok(())
}

/// Axis facts not exposed by calamine's value-oriented workbook reader.
///
/// XLSX stores these on worksheet XML rather than beside cell values.  They
/// materially affect both the spreadsheet canvas and PDF pagination, so
/// discarding them at import makes an otherwise successful Google Sheets
/// import visibly change shape.
#[derive(Default)]
struct XlsxAxisMetadata {
    column_widths: BTreeMap<u32, u32>,
    row_heights: BTreeMap<u32, u32>,
    hidden_columns: BTreeSet<u32>,
    hidden_rows: BTreeSet<u32>,
}

/// The one sheet-local page-setup fact OpenDoc currently owns.  A print area
/// is a defined name in OOXML rather than worksheet XML, so calamine's normal
/// value reader cannot associate it with the corresponding sheet (it drops
/// `localSheetId`).  Keep that association while preflighting the workbook.
#[derive(Default)]
struct XlsxPrintAreaImport {
    area: Option<String>,
    unsupported: bool,
}

/// The only page-setup value the durable sheet model owns. XLSX defaults to
/// portrait when `pageSetup` is absent, which differs from OpenDoc's legacy
/// landscape compatibility default, so absence must be made explicit on
/// import instead of inheriting the model default.
#[derive(Default)]
struct XlsxPrintOrientationImport {
    orientation: Option<SheetPrintOrientation>,
    unsupported: bool,
}

fn xml_attribute(element: &quick_xml::events::BytesStart<'_>, name: &[u8]) -> Option<String> {
    element.attributes().flatten().find_map(|attribute| {
        (attribute.key.as_ref() == name)
            .then(|| String::from_utf8_lossy(attribute.value.as_ref()).into_owned())
    })
}

fn xml_flag(value: Option<String>) -> bool {
    matches!(value.as_deref(), Some("1") | Some("true") | Some("TRUE"))
}

/// OpenDoc uses CSS pixels; XLSX stores column width in 7px character cells
/// plus Excel's five-pixel cell margin, and row height in points.  The margin
/// matters here: `rust_xlsxwriter` encodes a requested 140px column as a
/// 20.71-character XLSX width, so merely multiplying by seven would make
/// every reimport five pixels wider.
fn xlsx_column_width_px(width: f64) -> Option<u32> {
    let width = (width * 7.0 - 5.0).round();
    (width.is_finite() && width >= MIN_AXIS_SIZE_PX as f64 && width <= MAX_AXIS_SIZE_PX as f64)
        .then_some(width as u32)
}

fn xlsx_row_height_px(height: f64) -> Option<u32> {
    let height = (height / 0.75).round();
    (height.is_finite() && height >= MIN_AXIS_SIZE_PX as f64 && height <= MAX_AXIS_SIZE_PX as f64)
        .then_some(height as u32)
}

fn parse_xlsx_axis_metadata(xml: &[u8]) -> Result<XlsxAxisMetadata, SpreadsheetError> {
    let mut reader = XmlReader::from_reader(Cursor::new(xml));
    reader.config_mut().trim_text(true);
    let mut buffer = Vec::new();
    let mut metadata = XlsxAxisMetadata::default();
    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(element)) | Ok(Event::Empty(element)) => {
                match element.name().as_ref() {
                    b"col" => {
                        let min = xml_attribute(&element, b"min")
                            .and_then(|value| value.parse::<u32>().ok());
                        let max = xml_attribute(&element, b"max")
                            .and_then(|value| value.parse::<u32>().ok());
                        let width = xml_attribute(&element, b"width")
                            .and_then(|value| value.parse::<f64>().ok())
                            .and_then(xlsx_column_width_px);
                        let hidden = xml_flag(xml_attribute(&element, b"hidden"));
                        if let (Some(min), Some(max)) = (min, max) {
                            // The workbook grid is bounded; dimension metadata
                            // outside it has no valid model address to attach to.
                            for column in min..=max.min(XLSX_IMPORT_MAX_COLUMNS) {
                                if let Some(width) = width {
                                    metadata.column_widths.insert(column, width);
                                }
                                if hidden {
                                    metadata.hidden_columns.insert(column);
                                }
                            }
                        }
                    }
                    b"row" => {
                        let row = xml_attribute(&element, b"r")
                            .and_then(|value| value.parse::<u32>().ok());
                        let height = xml_attribute(&element, b"ht")
                            .and_then(|value| value.parse::<f64>().ok())
                            .and_then(xlsx_row_height_px);
                        if let Some(row) = row.filter(|row| *row <= XLSX_IMPORT_MAX_ROWS) {
                            if let Some(height) = height {
                                metadata.row_heights.insert(row, height);
                            }
                            if xml_flag(xml_attribute(&element, b"hidden")) {
                                metadata.hidden_rows.insert(row);
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => return Ok(metadata),
            Err(error) => {
                return Err(SpreadsheetError::Import(format!(
                    "invalid xlsx worksheet metadata: {error}"
                )))
            }
            _ => {}
        }
        buffer.clear();
    }
}

fn xlsx_worksheet_parts(bytes: &[u8]) -> Result<Vec<String>, SpreadsheetError> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| SpreadsheetError::Import(format!("invalid xlsx: {error}")))?;
    let mut workbook = Vec::new();
    archive
        .by_name("xl/workbook.xml")
        .map_err(|error| SpreadsheetError::Import(format!("invalid xlsx workbook: {error}")))?
        .read_to_end(&mut workbook)
        .map_err(|error| SpreadsheetError::Import(format!("invalid xlsx workbook: {error}")))?;
    let mut relationships = Vec::new();
    archive
        .by_name("xl/_rels/workbook.xml.rels")
        .map_err(|error| {
            SpreadsheetError::Import(format!("invalid xlsx workbook relationships: {error}"))
        })?
        .read_to_end(&mut relationships)
        .map_err(|error| {
            SpreadsheetError::Import(format!("invalid xlsx workbook relationships: {error}"))
        })?;

    let mut targets = BTreeMap::new();
    let mut relationship_reader = XmlReader::from_reader(Cursor::new(relationships));
    let mut buffer = Vec::new();
    loop {
        match relationship_reader.read_event_into(&mut buffer) {
            Ok(Event::Start(element)) | Ok(Event::Empty(element))
                if element.name().as_ref() == b"Relationship" =>
            {
                let id = xml_attribute(&element, b"Id");
                let target = xml_attribute(&element, b"Target");
                let kind = xml_attribute(&element, b"Type");
                if kind
                    .as_deref()
                    .is_some_and(|kind| kind.ends_with("/worksheet"))
                {
                    if let (Some(id), Some(target)) = (id, target) {
                        if !target.contains("..") {
                            targets.insert(id, format!("xl/{}", target.trim_start_matches('/')));
                        }
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(error) => {
                return Err(SpreadsheetError::Import(format!(
                    "invalid xlsx workbook relationships: {error}"
                )))
            }
            _ => {}
        }
        buffer.clear();
    }

    let mut parts = Vec::new();
    let mut workbook_reader = XmlReader::from_reader(Cursor::new(workbook));
    let mut buffer = Vec::new();
    loop {
        match workbook_reader.read_event_into(&mut buffer) {
            Ok(Event::Start(element)) | Ok(Event::Empty(element))
                if element.name().as_ref() == b"sheet" =>
            {
                let relationship_id = xml_attribute(&element, b"r:id");
                if let Some(part) = relationship_id.and_then(|id| targets.get(&id).cloned()) {
                    parts.push(part);
                }
            }
            Ok(Event::Eof) => return Ok(parts),
            Err(error) => {
                return Err(SpreadsheetError::Import(format!(
                    "invalid xlsx workbook: {error}"
                )))
            }
            _ => {}
        }
        buffer.clear();
    }
}

/// Reads sheet-local `_xlnm.Print_Area` names from the workbook part.
///
/// OpenDoc has one rectangular, in-grid A1 print area.  OOXML also permits a
/// union of rectangles, entire rows/columns, external references, and formula
/// shapes.  Those are deliberately not approximated: callers receive one
/// warning for the affected sheet and retain no misleading near-match.
fn xlsx_print_areas(bytes: &[u8]) -> Result<Vec<XlsxPrintAreaImport>, SpreadsheetError> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| SpreadsheetError::Import(format!("invalid xlsx: {error}")))?;
    let mut workbook = Vec::new();
    archive
        .by_name("xl/workbook.xml")
        .map_err(|error| SpreadsheetError::Import(format!("invalid xlsx workbook: {error}")))?
        .read_to_end(&mut workbook)
        .map_err(|error| SpreadsheetError::Import(format!("invalid xlsx workbook: {error}")))?;

    let mut output = Vec::<XlsxPrintAreaImport>::new();
    let mut reader = XmlReader::from_reader(Cursor::new(workbook));
    let mut buffer = Vec::new();
    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(element)) if element.local_name().as_ref() == b"definedName" => {
                let name = xml_attribute(&element, b"name");
                let sheet_index = xml_attribute(&element, b"localSheetId")
                    .and_then(|value| value.parse::<usize>().ok());
                let mut formula = String::new();
                let mut inner = Vec::new();
                loop {
                    match reader.read_event_into(&mut inner) {
                        Ok(Event::Text(text)) => {
                            let decoded = text.xml10_content().map_err(|error| {
                                SpreadsheetError::Import(format!(
                                    "invalid xlsx defined name: {error}"
                                ))
                            })?;
                            formula.push_str(&decoded);
                        }
                        Ok(Event::CData(text)) => {
                            formula.push_str(&String::from_utf8_lossy(text.as_ref()));
                        }
                        Ok(Event::End(end)) if end.local_name().as_ref() == b"definedName" => break,
                        Ok(Event::Eof) => {
                            return Err(SpreadsheetError::Import(
                                "invalid xlsx workbook: unterminated defined name".to_string(),
                            ));
                        }
                        Err(error) => {
                            return Err(SpreadsheetError::Import(format!(
                                "invalid xlsx workbook: {error}"
                            )));
                        }
                        _ => {}
                    }
                    inner.clear();
                }
                if name.as_deref() == Some("_xlnm.Print_Area") {
                    let Some(index) = sheet_index else {
                        // A workbook-global print area has no unambiguous
                        // sheet owner. It cannot become any sheet's setting.
                        buffer.clear();
                        continue;
                    };
                    while output.len() <= index {
                        output.push(XlsxPrintAreaImport::default());
                    }
                    let candidate = xlsx_print_area_formula(&formula);
                    let entry = &mut output[index];
                    if entry.area.is_some() || candidate.is_none() {
                        entry.area = None;
                        entry.unsupported = true;
                    } else {
                        entry.area = candidate;
                    }
                }
            }
            Ok(Event::Eof) => return Ok(output),
            Err(error) => {
                return Err(SpreadsheetError::Import(format!(
                    "invalid xlsx workbook: {error}"
                )))
            }
            _ => {}
        }
        buffer.clear();
    }
}

/// Converts the strictly rectangular OOXML spelling (`Sheet!$A$1:$B$2`) to
/// our canonical A1 range. The sheet qualifier is intentionally ignored here:
/// `localSheetId` above is the authoritative owner and handles quoted names.
fn xlsx_print_area_formula(formula: &str) -> Option<String> {
    let formula = formula.trim().trim_start_matches('=');
    let (_, range) = formula.rsplit_once('!')?;
    if formula.matches('!').count() != 1
        || formula.contains('[')
        || formula.contains(']')
        || range.contains(',')
        || range.contains(' ')
    {
        return None;
    }
    let range = range.replace('$', "");
    let canonical = normalize_cell_range(&range).ok()?;
    // `normalize_cell_range` accepts an individual address too, which is a
    // valid one-cell print rectangle. It rejects entire row/column forms.
    Some(canonical)
}

/// Reads the worksheet-local OOXML page orientation. Page size, scaling,
/// margins, print titles, and breaks remain deliberately outside this bridge.
fn xlsx_print_orientations(
    bytes: &[u8],
) -> Result<Vec<XlsxPrintOrientationImport>, SpreadsheetError> {
    let parts = xlsx_worksheet_parts(bytes)?;
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| SpreadsheetError::Import(format!("invalid xlsx: {error}")))?;
    parts
        .iter()
        .map(|part| {
            let mut xml = Vec::new();
            archive
                .by_name(part)
                .map_err(|error| {
                    SpreadsheetError::Import(format!("invalid xlsx worksheet {part}: {error}"))
                })?
                .read_to_end(&mut xml)
                .map_err(|error| {
                    SpreadsheetError::Import(format!("invalid xlsx worksheet {part}: {error}"))
                })?;
            let mut reader = XmlReader::from_reader(Cursor::new(xml));
            let mut buffer = Vec::new();
            let mut result = XlsxPrintOrientationImport::default();
            loop {
                match reader.read_event_into(&mut buffer) {
                    Ok(Event::Start(element)) | Ok(Event::Empty(element))
                        if element.name().as_ref() == b"pageSetup" =>
                    {
                        let orientation = match xml_attribute(&element, b"orientation").as_deref() {
                            None | Some("portrait") => Some(SheetPrintOrientation::Portrait),
                            Some("landscape") => Some(SheetPrintOrientation::Landscape),
                            Some(_) => None,
                        };
                        if result.orientation.is_some() || orientation.is_none() {
                            result.unsupported = true;
                        } else {
                            result.orientation = orientation;
                        }
                    }
                    Ok(Event::Eof) => return Ok(result),
                    Err(error) => {
                        return Err(SpreadsheetError::Import(format!(
                            "invalid xlsx worksheet {part}: {error}"
                        )));
                    }
                    _ => {}
                }
                buffer.clear();
            }
        })
        .collect()
}

/// Resolves an internal OPC relationship target relative to a package part.
///
/// XLSX drawing relationships normally say `../drawings/drawing1.xml`.  Do
/// not pass that string through to a filesystem path: package names are not
/// host paths, and a malformed workbook must not turn this inspection into a
/// traversal rule.
fn resolve_xlsx_relationship_target(part: &str, target: &str) -> Option<String> {
    let mut segments = if target.starts_with('/') {
        Vec::new()
    } else {
        part.split('/').collect::<Vec<_>>()
    };
    if !target.starts_with('/') {
        segments.pop();
    }
    for segment in target.trim_start_matches('/').split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                // Never allow a relationship to escape the `xl` package
                // subtree. All ordinary worksheet drawings stay under it.
                if segments.pop().is_none() || segments.first() != Some(&"xl") {
                    return None;
                }
            }
            segment => segments.push(segment),
        }
    }
    let resolved = segments.join("/");
    resolved.starts_with("xl/").then_some(resolved)
}

fn xlsx_floating_drawing_warnings(
    bytes: &[u8],
    sheet_names: &[String],
) -> Result<Vec<SpreadsheetWarning>, SpreadsheetError> {
    let worksheet_parts = xlsx_worksheet_parts(bytes)?;
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| SpreadsheetError::Import(format!("invalid xlsx: {error}")))?;
    let mut warnings = Vec::new();

    for (index, worksheet_part) in worksheet_parts.iter().enumerate() {
        let Some(file_name) = worksheet_part.rsplit('/').next() else {
            continue;
        };
        let relationships_part = format!(
            "{}/_rels/{}.rels",
            worksheet_part
                .rsplit_once('/')
                .map(|(parent, _)| parent)
                .unwrap_or(""),
            file_name
        );
        let Ok(mut relationships) = archive.by_name(&relationships_part) else {
            continue;
        };
        let mut xml = Vec::new();
        if relationships.read_to_end(&mut xml).is_err() {
            // The value reader can still import a workbook whose optional
            // drawing relationship part is broken. Its drawings are not part
            // of our model either, so avoid making an unrelated import fail.
            continue;
        }
        let mut reader = XmlReader::from_reader(Cursor::new(xml));
        let mut buffer = Vec::new();
        let mut drawing_parts = BTreeSet::new();
        loop {
            match reader.read_event_into(&mut buffer) {
                Ok(Event::Start(element)) | Ok(Event::Empty(element))
                    if element.name().as_ref() == b"Relationship" =>
                {
                    let kind = xml_attribute(&element, b"Type");
                    let target = xml_attribute(&element, b"Target");
                    let external = xml_attribute(&element, b"TargetMode")
                        .is_some_and(|mode| mode.eq_ignore_ascii_case("External"));
                    if !external
                        && kind
                            .as_deref()
                            .is_some_and(|kind| kind.ends_with("/drawing"))
                    {
                        if let Some(target) = target.as_deref().and_then(|target| {
                            resolve_xlsx_relationship_target(worksheet_part, target)
                        }) {
                            drawing_parts.insert(target);
                        }
                    }
                }
                Ok(Event::Eof) => break,
                Err(_) => {
                    // This relationship part is auxiliary to the cell grid;
                    // leave malformed drawing metadata to its producer rather
                    // than rejecting an otherwise readable workbook.
                    drawing_parts.clear();
                    break;
                }
                _ => {}
            }
            buffer.clear();
        }
        if !drawing_parts.is_empty() {
            let sheet_name = sheet_names
                .get(index)
                .map(String::as_str)
                .unwrap_or("unnamed sheet");
            let count = drawing_parts.len();
            push_unique_warning(
                &mut warnings,
                "xlsx-floating-drawing-unrepresentable",
                format!(
                    "XLSX sheet {sheet_name:?} contains {count} floating drawing part{} (images, charts, or shapes); OpenDoc's spreadsheet model has no floating drawing layer, so it was not imported",
                    if count == 1 { "" } else { "s" }
                ),
            );
        }
    }
    Ok(warnings)
}

/// The intentionally small style fragment that can be carried without
/// changing its meaning. Excel's `general`, `justify`, `distributed`, and
/// `centerContinuous` values are layout policies, not aliases for our three
/// durable horizontal positions, so they remain a disclosed loss. Likewise,
/// vertical `justify` and `distributed` depend on Excel row-layout behaviour
/// that OpenDoc does not own.
#[derive(Default)]
struct XlsxCellStyle {
    bold: bool,
    italic: bool,
    text_color: Option<String>,
    horizontal_align: Option<String>,
    wrap_strategy: Option<String>,
    vertical_align: Option<String>,
    number_format: Option<String>,
    background_color: Option<String>,
    /// A non-default OOXML `borderId`.  Borders are deliberately separate
    /// from the other style losses so the import report says what disappeared
    /// instead of hiding a visible grid/frame change in a generic style count.
    has_unimported_border: bool,
    /// Underlining is a visible font fact, but `CellFormat` deliberately has
    /// no decoration field. Keep it separate from the generic style loss so
    /// an import report identifies this common, visible change precisely.
    has_unimported_underline: bool,
    /// Strike-through is another visible font decoration with no durable
    /// spreadsheet counterpart. Keep it separate for the same reason as
    /// underline: users need to know that text was no longer struck out.
    has_unimported_strikethrough: bool,
    has_unimported_parts: bool,
}

#[derive(Default)]
struct XlsxCellStyleImport {
    cells: Vec<BTreeMap<String, XlsxCellStyle>>,
    warnings: Vec<SpreadsheetWarning>,
}

fn xlsx_style_has_unimported_attributes(element: &quick_xml::events::BytesStart<'_>) -> bool {
    [
        b"applyProtection".as_slice(),
        b"quotePrefix",
        b"pivotButton",
    ]
    .into_iter()
    .any(|name| xml_flag(xml_attribute(element, name)))
}

#[derive(Default)]
struct XlsxFontImport {
    bold: bool,
    italic: bool,
    text_color: Option<String>,
    has_unimported_underline: bool,
    has_unimported_strikethrough: bool,
    has_unimported_parts: bool,
}

/// Read the font facts which have exact, durable counterparts in `CellFormat`.
/// A non-default font is not inherently a loss: a font record containing just
/// `<b/>`, `<i/>`, and/or an opaque explicit sRGB `<color>` says exactly what
/// the model can say. Every other font child (family, size, underline,
/// strike-through, script, scheme, and so on) is deliberately disclosed
/// instead of being mistaken for an OpenDoc font fact.
fn xlsx_font_import(bytes: &[u8]) -> Result<Vec<XlsxFontImport>, SpreadsheetError> {
    let mut reader = XmlReader::from_reader(Cursor::new(bytes));
    let mut buffer = Vec::new();
    let mut in_fonts = false;
    let mut current: Option<XlsxFontImport> = None;
    let mut fonts = Vec::new();
    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(element)) if element.name().as_ref() == b"fonts" => in_fonts = true,
            Ok(Event::End(element)) if element.name().as_ref() == b"fonts" => break,
            Ok(Event::Start(element)) if in_fonts && element.name().as_ref() == b"font" => {
                let mut font = XlsxFontImport::default();
                if element.attributes().flatten().next().is_some() {
                    font.has_unimported_parts = true;
                }
                current = Some(font);
            }
            Ok(Event::Empty(element)) if in_fonts && element.name().as_ref() == b"font" => {
                let mut font = XlsxFontImport::default();
                if element.attributes().flatten().next().is_some() {
                    font.has_unimported_parts = true;
                }
                fonts.push(font);
            }
            Ok(Event::End(element)) if in_fonts && element.name().as_ref() == b"font" => {
                if let Some(font) = current.take() {
                    fonts.push(font);
                }
            }
            Ok(Event::Empty(element)) | Ok(Event::Start(element)) if current.is_some() => {
                let font = current.as_mut().expect("guarded above");
                match element.name().as_ref() {
                    b"b" | b"i" => {
                        // `val` is the sole meaningful attribute on these
                        // switches.  Absent means true in OOXML.
                        let other_attribute = element
                            .attributes()
                            .flatten()
                            .any(|attribute| attribute.key.as_ref() != b"val");
                        let enabled = match xml_attribute(&element, b"val").as_deref() {
                            None | Some("1" | "true" | "TRUE") => Some(true),
                            Some("0" | "false" | "FALSE") => Some(false),
                            Some(_) => None,
                        };
                        if let Some(enabled) = enabled.filter(|_| !other_attribute) {
                            if element.name().as_ref() == b"b" {
                                font.bold = enabled;
                            } else {
                                font.italic = enabled;
                            }
                        } else {
                            font.has_unimported_parts = true;
                        }
                    }
                    b"u" => {
                        // `u` without a value means single underline. The
                        // other non-`none` variants are also visible text
                        // decoration, none of which has a durable model
                        // counterpart. An explicit `none`, on the other
                        // hand, is precisely the model default and does not
                        // itself lose an authored fact.
                        let has_only_val = element
                            .attributes()
                            .flatten()
                            .all(|attribute| attribute.key.as_ref() == b"val");
                        match xml_attribute(&element, b"val").as_deref() {
                            Some("none") if has_only_val => {}
                            None
                            | Some("single" | "double" | "singleAccounting" | "doubleAccounting")
                                if has_only_val =>
                            {
                                font.has_unimported_underline = true;
                                font.has_unimported_parts = true;
                            }
                            _ => font.has_unimported_parts = true,
                        }
                    }
                    b"strike" => {
                        // As with bold and italic, absent means true. An
                        // explicit false says precisely the model default;
                        // true is a visible decoration we cannot retain.
                        let other_attribute = element
                            .attributes()
                            .flatten()
                            .any(|attribute| attribute.key.as_ref() != b"val");
                        let enabled = match xml_attribute(&element, b"val").as_deref() {
                            None | Some("1" | "true" | "TRUE") => Some(true),
                            Some("0" | "false" | "FALSE") => Some(false),
                            Some(_) => None,
                        };
                        match enabled.filter(|_| !other_attribute) {
                            Some(true) => {
                                font.has_unimported_strikethrough = true;
                                font.has_unimported_parts = true;
                            }
                            Some(false) => {}
                            None => font.has_unimported_parts = true,
                        }
                    }
                    b"color" => {
                        // An OOXML font colour can be an opaque ARGB literal,
                        // a theme/indexed/automatic token, or a literal with
                        // a tint. Only the first has the same meaning as the
                        // model's one CSS/sRGB text colour. `rgb` combined
                        // with `tint` is *not* that literal: Excel alters it.
                        if font.text_color.is_some() {
                            font.has_unimported_parts = true;
                        } else if let Some(color) = xlsx_opaque_srgb_color(&element) {
                            font.text_color = Some(color);
                        } else {
                            font.has_unimported_parts = true;
                        }
                    }
                    _ => font.has_unimported_parts = true,
                }
            }
            Ok(Event::Eof) => break,
            Err(error) => {
                return Err(SpreadsheetError::Import(format!(
                    "invalid xlsx styles: {error}"
                )));
            }
            _ => {}
        }
        buffer.clear();
    }
    Ok(fonts)
}

fn xlsx_style_font(
    element: &quick_xml::events::BytesStart<'_>,
    fonts: &[XlsxFontImport],
) -> (bool, bool, Option<String>, bool, bool, bool) {
    let Some(font_id) = xml_attribute(element, b"fontId") else {
        return (false, false, None, false, false, false);
    };
    let Ok(font_id) = font_id.parse::<usize>() else {
        return (false, false, None, false, false, true);
    };
    // `fontId=0` is the workbook's base font.  Its family, size and theme
    // colour are defaults rather than facts selected by this cell XF, just as
    // `fillId=0` and `borderId=0` are defaults.  Reading those declarations
    // as a loss would make every otherwise-supported style warn.
    if font_id == 0 {
        return (false, false, None, false, false, false);
    }
    let Some(font) = fonts.get(font_id) else {
        return (false, false, None, false, false, true);
    };
    (
        font.bold,
        font.italic,
        font.text_color.clone(),
        font.has_unimported_underline,
        font.has_unimported_strikethrough,
        font.has_unimported_parts,
    )
}

/// OOXML stores colours in a few incompatible namespaces.  The spreadsheet
/// model owns one opaque CSS/sRGB colour, so retain only an explicit opaque
/// ARGB `rgb` value.  Theme, indexed and automatic colours depend on external
/// palette state; partial alpha would require compositing against a background
/// OpenDoc does not own.
fn xlsx_opaque_srgb_color(element: &quick_xml::events::BytesStart<'_>) -> Option<String> {
    // A tint changes an RGB literal, and any additional colour namespace
    // makes a producer's intent ambiguous. Retaining the bare `rgb` form is
    // the only lossless projection to an uncomposited CSS colour.
    if element
        .attributes()
        .flatten()
        .any(|attribute| attribute.key.as_ref() != b"rgb")
    {
        return None;
    }
    let rgb = xml_attribute(element, b"rgb")?;
    let (alpha, color) = rgb.get(..2).zip(rgb.get(2..))?;
    (rgb.len() == 8
        && alpha.eq_ignore_ascii_case("ff")
        && color.chars().all(|character| character.is_ascii_hexdigit()))
    .then(|| format!("#{}", color.to_ascii_lowercase()))
}

#[derive(Default)]
struct XlsxFillImport {
    background_color: Option<String>,
    has_unimported_parts: bool,
}

/// Reads only the fill representation which has the same durable meaning as
/// `CellFormat.background_color`: a `solid` pattern with an opaque explicit
/// sRGB foreground colour.  In OOXML a solid fill is painted with `fgColor`;
/// its `bgColor` is deliberately irrelevant.  Everything else remains a
/// disclosed loss rather than silently resolving a theme or guessing a
/// patterned/gradient appearance.
fn xlsx_fill_import(bytes: &[u8]) -> Result<Vec<XlsxFillImport>, SpreadsheetError> {
    #[derive(Default)]
    struct PendingFill {
        pattern_type: Option<String>,
        foreground_color: Option<String>,
        foreground_seen: bool,
        unsupported: bool,
    }

    let mut reader = XmlReader::from_reader(Cursor::new(bytes));
    let mut buffer = Vec::new();
    let mut in_fills = false;
    let mut current: Option<PendingFill> = None;
    let mut fills = Vec::new();
    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(element)) if element.name().as_ref() == b"fills" => in_fills = true,
            Ok(Event::End(element)) if element.name().as_ref() == b"fills" => break,
            Ok(Event::Start(element)) if in_fills && element.name().as_ref() == b"fill" => {
                current = Some(PendingFill::default());
            }
            Ok(Event::End(element)) if in_fills && element.name().as_ref() == b"fill" => {
                let pending = current.take().expect("a closing fill has an opening fill");
                let mut fill = XlsxFillImport::default();
                match pending.pattern_type.as_deref() {
                    Some("none") | None if !pending.unsupported && !pending.foreground_seen => {}
                    Some("solid") if !pending.unsupported => {
                        if let Some(color) = pending.foreground_color {
                            fill.background_color = Some(color);
                        } else {
                            fill.has_unimported_parts = true;
                        }
                    }
                    _ => fill.has_unimported_parts = true,
                }
                fills.push(fill);
            }
            Ok(Event::Empty(element)) | Ok(Event::Start(element))
                if current.is_some() && element.name().as_ref() == b"patternFill" =>
            {
                let fill = current.as_mut().expect("guarded above");
                fill.pattern_type = xml_attribute(&element, b"patternType");
            }
            Ok(Event::Empty(element)) | Ok(Event::Start(element))
                if current.is_some() && element.name().as_ref() == b"gradientFill" =>
            {
                current.as_mut().expect("guarded above").unsupported = true;
            }
            Ok(Event::Empty(element)) | Ok(Event::Start(element))
                if current.is_some() && element.name().as_ref() == b"fgColor" =>
            {
                let fill = current.as_mut().expect("guarded above");
                fill.foreground_seen = true;
                fill.foreground_color = xlsx_opaque_srgb_color(&element);
                if fill.foreground_color.is_none() {
                    fill.unsupported = true;
                }
            }
            Ok(Event::Eof) => break,
            Err(error) => {
                return Err(SpreadsheetError::Import(format!(
                    "invalid xlsx styles: {error}"
                )));
            }
            _ => {}
        }
        buffer.clear();
    }
    Ok(fills)
}

fn xlsx_style_fill(
    element: &quick_xml::events::BytesStart<'_>,
    fills: &[XlsxFillImport],
) -> (Option<String>, bool) {
    let Some(fill_id) = xml_attribute(element, b"fillId") else {
        return (None, false);
    };
    let Ok(fill_id) = fill_id.parse::<usize>() else {
        return (None, true);
    };
    let Some(fill) = fills.get(fill_id) else {
        return (None, true);
    };
    (fill.background_color.clone(), fill.has_unimported_parts)
}

/// `borderId=0` is OOXML's required empty/default border.  Any other value is
/// an authored border record, but `CellFormat` has no per-edge border facts:
/// importing it as a fill, colour, or generic gridline would be a lie.
fn xlsx_style_has_border(element: &quick_xml::events::BytesStart<'_>) -> bool {
    xml_attribute(element, b"borderId").is_some_and(|value| value != "0")
}

/// The built-in OOXML formats that have stable, locale-independent format
/// codes.  The remaining built-in ids deliberately stay a disclosed loss:
/// several are locale-resolved by Excel, so inventing an English-looking code
/// would not preserve what the source actually said.
fn xlsx_builtin_number_format(id: u32) -> Option<Option<&'static str>> {
    Some(match id {
        0 => None,
        1 => Some("0"),
        2 => Some("0.00"),
        3 => Some("#,##0"),
        4 => Some("#,##0.00"),
        9 => Some("0%"),
        10 => Some("0.00%"),
        11 => Some("0.00E+00"),
        12 => Some("# ?/?"),
        13 => Some("# ??/??"),
        37 => Some("#,##0 ;(#,##0)"),
        38 => Some("#,##0 ;[Red](#,##0)"),
        39 => Some("#,##0.00;(#,##0.00)"),
        40 => Some("#,##0.00;[Red](#,##0.00)"),
        45 => Some("mm:ss"),
        46 => Some("[h]:mm:ss"),
        47 => Some("mmss.0"),
        48 => Some("##0.0E+0"),
        49 => Some("@"),
        _ => return None,
    })
}

fn xlsx_number_format(
    element: &quick_xml::events::BytesStart<'_>,
    custom_number_formats: &BTreeMap<u32, String>,
) -> Result<Option<String>, ()> {
    let Some(id) = xml_attribute(element, b"numFmtId") else {
        return Ok(None);
    };
    let id = id.parse::<u32>().map_err(|_| ())?;
    if let Some(format) = custom_number_formats.get(&id) {
        return Ok(Some(format.clone()));
    }
    xlsx_builtin_number_format(id)
        .map(|format| format.map(str::to_string))
        .ok_or(())
}

fn xlsx_alignment(style: &mut XlsxCellStyle, element: &quick_xml::events::BytesStart<'_>) {
    match xml_attribute(element, b"horizontal").as_deref() {
        None => {}
        Some("left") | Some("center") | Some("right") => {
            style.horizontal_align = xml_attribute(element, b"horizontal");
        }
        Some(_) => style.has_unimported_parts = true,
    }
    match xml_attribute(element, b"vertical").as_deref() {
        None => {}
        Some("top") => style.vertical_align = Some("top".to_string()),
        // Excel calls its centre position `center`; OpenDoc's durable value
        // is `middle` to match the Google Sheets interchange vocabulary.
        Some("center") => style.vertical_align = Some("middle".to_string()),
        Some("bottom") => style.vertical_align = Some("bottom".to_string()),
        Some(_) => style.has_unimported_parts = true,
    }
    // `wrapText` is the one OOXML text-flow switch with exactly the meaning
    // carried by CellFormat.  A missing or explicitly false attribute is the
    // Excel default, so neither creates an authored OpenDoc fact.  Do not
    // confuse the remaining switches with wrapping: they need layout rules
    // the spreadsheet model deliberately does not own.
    if xml_flag(xml_attribute(element, b"wrapText")) {
        style.wrap_strategy = Some("wrap".to_string());
    }
    if xml_flag(xml_attribute(element, b"shrinkToFit"))
        || xml_attribute(element, b"textRotation").is_some()
        || xml_attribute(element, b"indent").is_some()
        || xml_attribute(element, b"relativeIndent").is_some()
        || xml_flag(xml_attribute(element, b"justifyLastLine"))
        || xml_flag(xml_attribute(element, b"readingOrder"))
    {
        style.has_unimported_parts = true;
    }
}

/// Reads the OOXML alignment fragment owned by `CellFormat` today:
/// left/center/right horizontal, `wrapText`, and top/center/bottom vertical
/// alignment.
/// Calamine's value reader exposes no style IDs, so retain the worksheet's
/// cell-to-style association ourselves. All other style components remain
/// explicitly disclosed rather than being recast as an OpenDoc format.
fn xlsx_cell_style_import(
    bytes: &[u8],
    sheet_names: &[String],
) -> Result<XlsxCellStyleImport, SpreadsheetError> {
    let parts = xlsx_worksheet_parts(bytes)?;
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| SpreadsheetError::Import(format!("invalid xlsx: {error}")))?;
    let has_styles = archive.file_names().any(|name| name == "xl/styles.xml");
    if !has_styles {
        return Ok(XlsxCellStyleImport {
            cells: (0..parts.len()).map(|_| BTreeMap::new()).collect(),
            warnings: Vec::new(),
        });
    }
    let mut styles_xml = Vec::new();
    archive
        .by_name("xl/styles.xml")
        .map_err(|error| SpreadsheetError::Import(format!("invalid xlsx styles: {error}")))?
        .read_to_end(&mut styles_xml)
        .map_err(|error| SpreadsheetError::Import(format!("invalid xlsx styles: {error}")))?;
    let fills = xlsx_fill_import(&styles_xml)?;
    let fonts = xlsx_font_import(&styles_xml)?;
    let mut style_reader = XmlReader::from_reader(Cursor::new(styles_xml));
    let mut style_buffer = Vec::new();
    let mut in_cell_xfs = false;
    let mut in_num_fmts = false;
    let mut current: Option<XlsxCellStyle> = None;
    let mut styles = Vec::new();
    let mut custom_number_formats = BTreeMap::new();
    loop {
        match style_reader.read_event_into(&mut style_buffer) {
            Ok(Event::Start(element)) if element.name().as_ref() == b"numFmts" => {
                in_num_fmts = true;
            }
            Ok(Event::End(element)) if element.name().as_ref() == b"numFmts" => {
                in_num_fmts = false;
            }
            Ok(Event::Empty(element)) if in_num_fmts && element.name().as_ref() == b"numFmt" => {
                if let (Some(id), Some(code)) = (
                    xml_attribute(&element, b"numFmtId").and_then(|id| id.parse::<u32>().ok()),
                    xml_attribute(&element, b"formatCode"),
                ) {
                    custom_number_formats.insert(id, code);
                }
            }
            Ok(Event::Start(element)) if element.name().as_ref() == b"cellXfs" => {
                in_cell_xfs = true;
            }
            Ok(Event::End(element)) if element.name().as_ref() == b"cellXfs" => break,
            Ok(Event::Start(element)) if in_cell_xfs && element.name().as_ref() == b"xf" => {
                let number_format = xlsx_number_format(&element, &custom_number_formats);
                let (
                    bold,
                    italic,
                    text_color,
                    has_unimported_underline,
                    has_unimported_strikethrough,
                    has_unimported_font,
                ) = xlsx_style_font(&element, &fonts);
                let has_unimported_border = xlsx_style_has_border(&element);
                let (background_color, has_unimported_fill) = xlsx_style_fill(&element, &fills);
                current = Some(XlsxCellStyle {
                    bold,
                    italic,
                    text_color,
                    horizontal_align: None,
                    wrap_strategy: None,
                    vertical_align: None,
                    number_format: number_format.clone().ok().flatten(),
                    background_color,
                    has_unimported_border,
                    has_unimported_underline,
                    has_unimported_strikethrough,
                    has_unimported_parts: has_unimported_border
                        || xlsx_style_has_unimported_attributes(&element)
                        || has_unimported_font
                        || has_unimported_fill
                        || number_format.is_err(),
                });
            }
            Ok(Event::Empty(element)) if in_cell_xfs && element.name().as_ref() == b"xf" => {
                let number_format = xlsx_number_format(&element, &custom_number_formats);
                let (
                    bold,
                    italic,
                    text_color,
                    has_unimported_underline,
                    has_unimported_strikethrough,
                    has_unimported_font,
                ) = xlsx_style_font(&element, &fonts);
                let has_unimported_border = xlsx_style_has_border(&element);
                let (background_color, has_unimported_fill) = xlsx_style_fill(&element, &fills);
                styles.push(XlsxCellStyle {
                    bold,
                    italic,
                    text_color,
                    horizontal_align: None,
                    wrap_strategy: None,
                    vertical_align: None,
                    number_format: number_format.clone().ok().flatten(),
                    background_color,
                    has_unimported_border,
                    has_unimported_underline,
                    has_unimported_strikethrough,
                    has_unimported_parts: has_unimported_border
                        || xlsx_style_has_unimported_attributes(&element)
                        || has_unimported_font
                        || has_unimported_fill
                        || number_format.is_err(),
                });
            }
            Ok(Event::Empty(element)) if element.name().as_ref() == b"alignment" => {
                if let Some(style) = &mut current {
                    xlsx_alignment(style, &element);
                }
            }
            Ok(Event::Start(element)) if element.name().as_ref() == b"alignment" => {
                if let Some(style) = &mut current {
                    xlsx_alignment(style, &element);
                }
            }
            Ok(Event::Empty(element)) if element.name().as_ref() == b"protection" => {
                if let Some(style) = &mut current {
                    style.has_unimported_parts = true;
                }
            }
            Ok(Event::End(element)) if in_cell_xfs && element.name().as_ref() == b"xf" => {
                if let Some(style) = current.take() {
                    styles.push(style);
                }
            }
            Ok(Event::Eof) => break,
            Err(error) => {
                return Err(SpreadsheetError::Import(format!(
                    "invalid xlsx styles: {error}"
                )));
            }
            _ => {}
        }
        style_buffer.clear();
    }
    let mut result = XlsxCellStyleImport {
        cells: Vec::with_capacity(parts.len()),
        warnings: Vec::new(),
    };
    for (index, part) in parts.iter().enumerate() {
        let mut xml = Vec::new();
        archive
            .by_name(part)
            .map_err(|error| {
                SpreadsheetError::Import(format!("invalid xlsx worksheet {part}: {error}"))
            })?
            .read_to_end(&mut xml)
            .map_err(|error| {
                SpreadsheetError::Import(format!("invalid xlsx worksheet {part}: {error}"))
            })?;
        let mut reader = XmlReader::from_reader(Cursor::new(xml));
        let mut buffer = Vec::new();
        let mut cells = BTreeMap::new();
        let mut unimported_cells = 0usize;
        let mut border_cells = 0usize;
        let mut underline_cells = 0usize;
        let mut strikethrough_cells = 0usize;
        loop {
            match reader.read_event_into(&mut buffer) {
                Ok(Event::Start(element)) | Ok(Event::Empty(element))
                    if element.name().as_ref() == b"c" =>
                {
                    if let (Some(address), Some(style)) = (
                        xml_attribute(&element, b"r"),
                        xml_attribute(&element, b"s").and_then(|value| value.parse::<usize>().ok()),
                    ) {
                        let style = styles.get(style).ok_or_else(|| {
                            SpreadsheetError::Import(format!(
                                "xlsx worksheet {part} references missing cell style {style}"
                            ))
                        })?;
                        if style.bold
                            || style.italic
                            || style.text_color.is_some()
                            || style.horizontal_align.is_some()
                            || style.wrap_strategy.is_some()
                            || style.vertical_align.is_some()
                            || style.number_format.is_some()
                            || style.background_color.is_some()
                        {
                            cells.insert(
                                address,
                                XlsxCellStyle {
                                    bold: style.bold,
                                    italic: style.italic,
                                    text_color: style.text_color.clone(),
                                    horizontal_align: style.horizontal_align.clone(),
                                    wrap_strategy: style.wrap_strategy.clone(),
                                    vertical_align: style.vertical_align.clone(),
                                    number_format: style.number_format.clone(),
                                    background_color: style.background_color.clone(),
                                    has_unimported_border: style.has_unimported_border,
                                    has_unimported_underline: style.has_unimported_underline,
                                    has_unimported_strikethrough: style
                                        .has_unimported_strikethrough,
                                    has_unimported_parts: style.has_unimported_parts,
                                },
                            );
                        }
                        if style.has_unimported_parts {
                            unimported_cells += 1;
                        }
                        if style.has_unimported_border {
                            border_cells += 1;
                        }
                        if style.has_unimported_underline {
                            underline_cells += 1;
                        }
                        if style.has_unimported_strikethrough {
                            strikethrough_cells += 1;
                        }
                    }
                }
                Ok(Event::Eof) => break,
                Err(error) => {
                    return Err(SpreadsheetError::Import(format!(
                        "invalid xlsx worksheet {part}: {error}"
                    )));
                }
                _ => {}
            }
            buffer.clear();
        }
        if unimported_cells > 0 {
            let name = sheet_names
                .get(index)
                .map(String::as_str)
                .unwrap_or("unnamed sheet");
            push_unique_warning(
                &mut result.warnings,
                "xlsx-cell-styles-unimported",
                format!(
                    "XLSX sheet {name:?} has {unimported_cells} cell{} with style properties OpenDoc cannot represent; only bold/italic switches and opaque sRGB text colours, opaque solid sRGB background fills, stable number formats, left/center/right horizontal, wrap-text, and top/middle/bottom vertical alignment were imported",
                    if unimported_cells == 1 { "" } else { "s" }
                ),
            );
        }
        if border_cells > 0 {
            let name = sheet_names
                .get(index)
                .map(String::as_str)
                .unwrap_or("unnamed sheet");
            push_unique_warning(
                &mut result.warnings,
                "xlsx-cell-borders-unimported",
                format!(
                    "XLSX sheet {name:?} has {border_cells} cell{} with authored borders; OpenDoc's spreadsheet CellFormat has no border representation, so those borders were not imported",
                    if border_cells == 1 { "" } else { "s" }
                ),
            );
        }
        if underline_cells > 0 {
            let name = sheet_names
                .get(index)
                .map(String::as_str)
                .unwrap_or("unnamed sheet");
            push_unique_warning(
                &mut result.warnings,
                "xlsx-cell-font-underlines-unimported",
                format!(
                    "XLSX sheet {name:?} has {underline_cells} cell{} with an underlined font; OpenDoc's spreadsheet CellFormat has no underline representation, so the underlining was not imported",
                    if underline_cells == 1 { "" } else { "s" }
                ),
            );
        }
        if strikethrough_cells > 0 {
            let name = sheet_names
                .get(index)
                .map(String::as_str)
                .unwrap_or("unnamed sheet");
            push_unique_warning(
                &mut result.warnings,
                "xlsx-cell-font-strikethrough-unimported",
                format!(
                    "XLSX sheet {name:?} has {strikethrough_cells} cell{} with a strike-through font; OpenDoc's spreadsheet CellFormat has no strike-through representation, so the strike-through was not imported",
                    if strikethrough_cells == 1 { "" } else { "s" }
                ),
            );
        }
        result.cells.push(cells);
    }
    Ok(result)
}

fn xlsx_axis_metadata(bytes: &[u8]) -> Result<Vec<XlsxAxisMetadata>, SpreadsheetError> {
    let parts = xlsx_worksheet_parts(bytes)?;
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| SpreadsheetError::Import(format!("invalid xlsx: {error}")))?;
    parts
        .iter()
        .map(|part| {
            let mut xml = Vec::new();
            archive
                .by_name(part)
                .map_err(|error| {
                    SpreadsheetError::Import(format!("invalid xlsx worksheet {part}: {error}"))
                })?
                .read_to_end(&mut xml)
                .map_err(|error| {
                    SpreadsheetError::Import(format!("invalid xlsx worksheet {part}: {error}"))
                })?;
            parse_xlsx_axis_metadata(&xml)
        })
        .collect()
}

/// Cell validation is not exposed by calamine's value reader. Read only the
/// rules whose OpenDoc meaning is exactly expressible in OOXML. In particular,
/// an Excel whole-number rule is *not* a decimal-number rule, and a formula
/// operand is not a literal bound.
#[derive(Default)]
struct XlsxValidationImport {
    cells: BTreeMap<String, CellValidation>,
    unsupported_rules: usize,
}

type XlsxValidationRecord = (
    String,
    Option<String>,
    Vec<String>,
    bool,
    bool,
    String,
    String,
);

fn xlsx_validations(bytes: &[u8]) -> Result<Vec<XlsxValidationImport>, SpreadsheetError> {
    let parts = xlsx_worksheet_parts(bytes)?;
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| SpreadsheetError::Import(format!("invalid xlsx: {error}")))?;
    parts
        .iter()
        .map(|part| {
            let mut xml = Vec::new();
            archive
                .by_name(part)
                .map_err(|error| {
                    SpreadsheetError::Import(format!("invalid xlsx worksheet {part}: {error}"))
                })?
                .read_to_end(&mut xml)
                .map_err(|error| {
                    SpreadsheetError::Import(format!("invalid xlsx worksheet {part}: {error}"))
                })?;
            parse_xlsx_validations(&xml)
        })
        .collect()
}

/// Calamine deliberately exposes cells rather than worksheet presentation
/// records, so read the portable AutoFilter facts ourselves. A one-value list
/// filter is the one criteria shape the model and OOXML share exactly: it is a
/// string equality selection, with no blank selection, grouping, colour, or
/// hidden-row state. Everything else remains an explicit loss.
#[derive(Clone, Debug)]
struct XlsxBasicFilter {
    range: String,
    criteria: Vec<SheetFilterCriterion>,
    has_unsupported_criteria: bool,
    has_sort: bool,
}

#[derive(Default)]
struct XlsxFilterColumn {
    offset: Option<u32>,
    list_values: Vec<String>,
    is_plain_list: bool,
    has_unsupported_state: bool,
}

fn xlsx_basic_filters(bytes: &[u8]) -> Result<Vec<Option<XlsxBasicFilter>>, SpreadsheetError> {
    let parts = xlsx_worksheet_parts(bytes)?;
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| SpreadsheetError::Import(format!("invalid xlsx: {error}")))?;
    parts
        .iter()
        .map(|part| {
            let mut xml = Vec::new();
            archive
                .by_name(part)
                .map_err(|error| {
                    SpreadsheetError::Import(format!("invalid xlsx worksheet {part}: {error}"))
                })?
                .read_to_end(&mut xml)
                .map_err(|error| {
                    SpreadsheetError::Import(format!("invalid xlsx worksheet {part}: {error}"))
                })?;
            parse_xlsx_basic_filter(&xml)
        })
        .collect()
}

fn parse_xlsx_basic_filter(xml: &[u8]) -> Result<Option<XlsxBasicFilter>, SpreadsheetError> {
    let mut reader = XmlReader::from_reader(Cursor::new(xml));
    let mut buffer = Vec::new();
    let mut filter = None;
    let mut filter_column: Option<XlsxFilterColumn> = None;
    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(element)) if element.name().as_ref() == b"autoFilter" => {
                // `$` is legal OOXML spelling but is not part of OpenDoc's
                // canonical cell-range grammar.
                let Some(range) = xml_attribute(&element, b"ref")
                    .map(|range| range.replace('$', ""))
                    .and_then(|range| normalize_cell_range(&range).ok())
                else {
                    buffer.clear();
                    continue;
                };
                filter = Some(XlsxBasicFilter {
                    range,
                    criteria: Vec::new(),
                    has_unsupported_criteria: false,
                    has_sort: false,
                });
            }
            Ok(Event::Empty(element)) if element.name().as_ref() == b"autoFilter" => {
                let Some(range) = xml_attribute(&element, b"ref")
                    .map(|range| range.replace('$', ""))
                    .and_then(|range| normalize_cell_range(&range).ok())
                else {
                    buffer.clear();
                    continue;
                };
                return Ok(Some(XlsxBasicFilter {
                    range,
                    criteria: Vec::new(),
                    has_unsupported_criteria: false,
                    has_sort: false,
                }));
            }
            Ok(Event::Start(element)) if filter.is_some() => match element.name().as_ref() {
                b"filterColumn" => {
                    let offset = xml_attribute(&element, b"colId")
                        .and_then(|value| value.parse::<u32>().ok());
                    let invalid_attributes = element
                        .attributes()
                        .flatten()
                        .any(|attribute| attribute.key.as_ref() != b"colId");
                    // An unset colId is malformed for our bounded mapping, but
                    // still must be disclosed instead of losing the whole range.
                    filter_column = Some(XlsxFilterColumn {
                        offset,
                        has_unsupported_state: invalid_attributes || offset.is_none(),
                        ..Default::default()
                    });
                }
                b"filters" => {
                    if let Some(column) = filter_column.as_mut() {
                        column.is_plain_list = xml_attribute(&element, b"blank")
                            .is_none_or(|value| value != "1" && value != "true");
                        if element
                            .attributes()
                            .flatten()
                            .any(|attribute| attribute.key.as_ref() != b"blank")
                        {
                            column.has_unsupported_state = true;
                        }
                    }
                }
                b"filter" => {
                    if let Some(column) = filter_column.as_mut() {
                        match xml_attribute(&element, b"val") {
                            Some(value) if !value.is_empty() => column.list_values.push(value),
                            _ => column.has_unsupported_state = true,
                        }
                        if element
                            .attributes()
                            .flatten()
                            .any(|attribute| attribute.key.as_ref() != b"val")
                        {
                            column.has_unsupported_state = true;
                        }
                    }
                }
                b"sortState" => filter.as_mut().expect("checked above").has_sort = true,
                // These are not a one-string equality selection. Mark their
                // containing column as foreign but keep parsing later columns.
                _ => {
                    if let Some(column) = filter_column.as_mut() {
                        column.has_unsupported_state = true;
                    }
                }
            },
            Ok(Event::Empty(element)) if filter.is_some() => match element.name().as_ref() {
                b"filter" => {
                    if let Some(column) = filter_column.as_mut() {
                        match xml_attribute(&element, b"val") {
                            Some(value) if !value.is_empty() => column.list_values.push(value),
                            _ => column.has_unsupported_state = true,
                        }
                        if element
                            .attributes()
                            .flatten()
                            .any(|attribute| attribute.key.as_ref() != b"val")
                        {
                            column.has_unsupported_state = true;
                        }
                    }
                }
                b"filters" => {
                    if let Some(column) = filter_column.as_mut() {
                        column.is_plain_list = false;
                    }
                }
                b"sortState" => filter.as_mut().expect("checked above").has_sort = true,
                b"filterColumn" => {
                    filter
                        .as_mut()
                        .expect("checked above")
                        .has_unsupported_criteria = true
                }
                _ => {
                    if let Some(column) = filter_column.as_mut() {
                        column.has_unsupported_state = true;
                    }
                }
            },
            Ok(Event::End(element)) if element.name().as_ref() == b"filterColumn" => {
                let Some(column) = filter_column.take() else {
                    buffer.clear();
                    continue;
                };
                let filter = filter.as_mut().expect("checked above");
                let parsed = parse_cell_range(&filter.range)?;
                let Some(offset) = column.offset else {
                    filter.has_unsupported_criteria = true;
                    buffer.clear();
                    continue;
                };
                let Some(column_number) = parsed.start_column.checked_add(offset) else {
                    filter.has_unsupported_criteria = true;
                    buffer.clear();
                    continue;
                };
                if column_number >= parsed.start_column + parsed.width
                    || !column.is_plain_list
                    || column.list_values.len() != 1
                    || column.has_unsupported_state
                {
                    filter.has_unsupported_criteria = true;
                    buffer.clear();
                    continue;
                }
                filter.criteria.push(SheetFilterCriterion {
                    column: number_to_column(column_number)
                        .expect("parsed OpenDoc range has a valid column"),
                    condition: "text_equals".to_string(),
                    value: column
                        .list_values
                        .into_iter()
                        .next()
                        .expect("checked length"),
                });
            }
            Ok(Event::End(element)) if element.name().as_ref() == b"autoFilter" => {
                return Ok(filter);
            }
            Ok(Event::Eof) => return Ok(filter),
            Err(error) => {
                return Err(SpreadsheetError::Import(format!(
                    "invalid xlsx filter metadata: {error}"
                )))
            }
            _ => {}
        }
        buffer.clear();
    }
}

/// Legacy XLSX notes are a single author/body record per cell. They are not
/// threaded comments, so this deliberately imports only that portable shape.
fn xlsx_legacy_comments(
    bytes: &[u8],
) -> Result<Vec<BTreeMap<String, CellComment>>, SpreadsheetError> {
    let parts = xlsx_worksheet_parts(bytes)?;
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| SpreadsheetError::Import(format!("invalid xlsx: {error}")))?;
    parts
        .iter()
        .map(|part| {
            let file_name = part.rsplit('/').next().unwrap_or_default();
            let rels = format!(
                "{}/_rels/{}.rels",
                part.rsplit_once('/')
                    .map(|(parent, _)| parent)
                    .unwrap_or_default(),
                file_name
            );
            let mut rel_xml = Vec::new();
            let Ok(mut relationship_file) = archive.by_name(&rels) else {
                return Ok(BTreeMap::new());
            };
            relationship_file
                .read_to_end(&mut rel_xml)
                .map_err(|error| {
                    SpreadsheetError::Import(format!("invalid xlsx relationships {rels}: {error}"))
                })?;
            drop(relationship_file);
            let target = xlsx_comment_relationship_target(&rel_xml)
                .and_then(|target| resolve_xlsx_relationship_target(part, &target));
            let Some(target) = target else {
                return Ok(BTreeMap::new());
            };
            let mut comment_xml = Vec::new();
            archive
                .by_name(&target)
                .map_err(|error| {
                    SpreadsheetError::Import(format!("invalid xlsx comments {target}: {error}"))
                })?
                .read_to_end(&mut comment_xml)
                .map_err(|error| {
                    SpreadsheetError::Import(format!("invalid xlsx comments {target}: {error}"))
                })?;
            parse_xlsx_legacy_comments(&comment_xml)
        })
        .collect()
}

fn xlsx_comment_relationship_target(xml: &[u8]) -> Option<String> {
    let mut reader = XmlReader::from_reader(Cursor::new(xml));
    let mut buffer = Vec::new();
    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(element)) | Ok(Event::Empty(element))
                if element.name().as_ref() == b"Relationship" =>
            {
                if xml_attribute(&element, b"Type")
                    .as_deref()
                    .is_some_and(|kind| kind.ends_with("/comments"))
                {
                    return xml_attribute(&element, b"Target");
                }
            }
            Ok(Event::Eof) | Err(_) => return None,
            _ => {}
        }
        buffer.clear();
    }
}

fn parse_xlsx_legacy_comments(
    xml: &[u8],
) -> Result<BTreeMap<String, CellComment>, SpreadsheetError> {
    let mut reader = XmlReader::from_reader(Cursor::new(xml));
    let mut buffer = Vec::new();
    let mut authors = Vec::new();
    let mut author_text = None::<String>;
    let mut comment = None::<(String, usize, String)>;
    let mut in_text = false;
    let mut out = BTreeMap::new();
    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(element)) => match element.name().as_ref() {
                b"author" => author_text = Some(String::new()),
                b"comment" => {
                    if let (Some(address), Some(author_id)) = (
                        xml_attribute(&element, b"ref"),
                        xml_attribute(&element, b"authorId").and_then(|value| value.parse().ok()),
                    ) {
                        comment = Some((address, author_id, String::new()));
                    }
                }
                b"t" if comment.is_some() => in_text = true,
                _ => {}
            },
            Ok(Event::Text(text)) => {
                let decoded = text.decode().map_err(|error| {
                    SpreadsheetError::Import(format!("invalid xlsx comment: {error}"))
                })?;
                let decoded = quick_xml::escape::unescape(&decoded).map_err(|error| {
                    SpreadsheetError::Import(format!("invalid xlsx comment: {error}"))
                })?;
                if let Some(author) = author_text.as_mut() {
                    author.push_str(&decoded);
                }
                if in_text {
                    if let Some((_, _, body)) = comment.as_mut() {
                        body.push_str(&decoded);
                    }
                }
            }
            Ok(Event::End(element)) => match element.name().as_ref() {
                b"author" => {
                    if let Some(author) = author_text.take() {
                        authors.push(author);
                    }
                }
                b"t" => in_text = false,
                b"comment" => {
                    if let Some((address, author_id, body)) = comment.take() {
                        if let (Some(author), Ok(address)) = (
                            authors.get(author_id),
                            crate::address::normalize_cell_address(&address),
                        ) {
                            // rust_xlsxwriter follows Excel's legacy note
                            // convention and writes the author prefix into
                            // the rich-text body as well as `authorId`.
                            // The model owns those separately, so remove only
                            // that exact generated prefix.
                            let body = body
                                .strip_prefix(&format!("{author}:\n"))
                                .unwrap_or(&body)
                                .to_string();
                            if !body.trim().is_empty() {
                                out.insert(
                                    address.clone(),
                                    CellComment {
                                        id: format!("xlsx-note-{}", address.to_ascii_lowercase()),
                                        author: author.clone(),
                                        body,
                                        deleted: false,
                                    },
                                );
                            }
                        }
                    }
                }
                _ => {}
            },
            Ok(Event::Eof) => return Ok(out),
            Err(error) => {
                return Err(SpreadsheetError::Import(format!(
                    "invalid xlsx comments: {error}"
                )))
            }
            _ => {}
        }
        buffer.clear();
    }
}

fn parse_xlsx_validations(xml: &[u8]) -> Result<XlsxValidationImport, SpreadsheetError> {
    let mut reader = XmlReader::from_reader(Cursor::new(xml));
    reader.config_mut().trim_text(true);
    let mut buffer = Vec::new();
    let mut out = XlsxValidationImport::default();
    let mut current: Option<XlsxValidationRecord> = None;
    let mut reading_formula: Option<u8> = None;
    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(element)) if element.name().as_ref() == b"dataValidation" => {
                let addresses = xml_attribute(&element, b"sqref")
                    .unwrap_or_default()
                    .split_whitespace()
                    .map(str::to_string)
                    .collect();
                let strict = !matches!(
                    xml_attribute(&element, b"errorStyle").as_deref(),
                    Some("warning") | Some("information")
                );
                // OOXML's showDropDown is inverted: 1 means hide it.
                let show_dropdown = !xml_flag(xml_attribute(&element, b"showDropDown"));
                current = Some((
                    xml_attribute(&element, b"type").unwrap_or_default(),
                    xml_attribute(&element, b"operator"),
                    addresses,
                    strict,
                    show_dropdown,
                    String::new(),
                    String::new(),
                ));
            }
            Ok(Event::Start(element))
                if element.name().as_ref() == b"formula1" && current.is_some() =>
            {
                reading_formula = Some(1);
            }
            Ok(Event::Start(element))
                if element.name().as_ref() == b"formula2" && current.is_some() =>
            {
                reading_formula = Some(2);
            }
            Ok(Event::Text(text)) if reading_formula.is_some() => {
                let decoded = text.decode().map_err(|error| {
                    SpreadsheetError::Import(format!("invalid xlsx validation: {error}"))
                })?;
                let decoded = quick_xml::escape::unescape(&decoded).map_err(|error| {
                    SpreadsheetError::Import(format!("invalid xlsx validation: {error}"))
                })?;
                if let Some(current) = current.as_mut() {
                    match reading_formula {
                        Some(1) => current.5.push_str(&decoded),
                        Some(2) => current.6.push_str(&decoded),
                        _ => {}
                    }
                }
            }
            Ok(Event::GeneralRef(reference)) if reading_formula.is_some() => {
                let reference = reference.decode().map_err(|error| {
                    SpreadsheetError::Import(format!("invalid xlsx validation: {error}"))
                })?;
                let decoded = xlsx_xml_reference(&reference).ok_or_else(|| {
                    SpreadsheetError::Import(format!(
                        "invalid xlsx validation: unknown XML entity {reference:?}"
                    ))
                })?;
                if let Some(current) = current.as_mut() {
                    match reading_formula {
                        Some(1) => current.5.push_str(&decoded),
                        Some(2) => current.6.push_str(&decoded),
                        _ => {}
                    }
                }
            }
            Ok(Event::End(element)) if element.name().as_ref() == b"formula1" => {
                reading_formula = None;
            }
            Ok(Event::End(element)) if element.name().as_ref() == b"formula2" => {
                reading_formula = None;
            }
            Ok(Event::End(element)) if element.name().as_ref() == b"dataValidation" => {
                reading_formula = None;
                if let Some((kind, operator, ranges, strict, show_dropdown, first, second)) =
                    current.take()
                {
                    let validation = xlsx_validation_from_fields(
                        &kind,
                        operator.as_deref(),
                        &first,
                        &second,
                        strict,
                        show_dropdown,
                    );
                    let Some(validation) = validation else {
                        out.unsupported_rules += 1;
                        buffer.clear();
                        continue;
                    };
                    for range in ranges {
                        let Ok(range) = parse_cell_range(&range) else {
                            continue;
                        };
                        let end_column = range
                            .start_column
                            .saturating_add(range.width)
                            .saturating_sub(1);
                        let end_row = range
                            .start_row
                            .saturating_add(range.height)
                            .saturating_sub(1);
                        if end_column > XLSX_IMPORT_MAX_COLUMNS || end_row > XLSX_IMPORT_MAX_ROWS {
                            continue;
                        }
                        for row in range.start_row..=end_row {
                            for column in range.start_column..=end_column {
                                if let Some(column) = number_to_column(column) {
                                    out.cells
                                        .insert(format!("{column}{row}"), validation.clone());
                                }
                            }
                        }
                    }
                }
            }
            Ok(Event::Eof) => return Ok(out),
            Err(error) => {
                return Err(SpreadsheetError::Import(format!(
                    "invalid xlsx validation metadata: {error}"
                )))
            }
            _ => {}
        }
        buffer.clear();
    }
}

fn xlsx_validation_from_fields(
    kind: &str,
    operator: Option<&str>,
    first: &str,
    second: &str,
    strict: bool,
    show_dropdown: bool,
) -> Option<CellValidation> {
    let (kind, values) = match (kind, operator) {
        ("list", _) => {
            // Literal Excel lists are quoted and escape an embedded quote by doubling it.
            let source = first.strip_prefix('"')?.strip_suffix('"')?;
            (
                "list",
                source
                    .split(',')
                    .map(|value| value.replace("\"\"", "\""))
                    .collect(),
            )
        }
        ("decimal", Some("greaterThan")) if xlsx_literal_number(first).is_some() => {
            ("number_greater", vec![first.to_string()])
        }
        ("decimal", Some("lessThan")) if xlsx_literal_number(first).is_some() => {
            ("number_less", vec![first.to_string()])
        }
        // OOXML omits the operator for its default inclusive `between` rule.
        ("decimal", None) if xlsx_ordered_literal_numbers(first, second) => (
            "number_between",
            vec![first.to_string(), second.to_string()],
        ),
        // Excel stores validation formulas without their leading equals sign;
        // OpenDoc keeps formula source in the same canonical spelling as cell
        // formulas, so restore it on import.
        ("custom", _) if !first.trim().is_empty() => (
            "custom_formula",
            vec![format!("={}", first.trim_start_matches('='))],
        ),
        _ => return None,
    };
    let mut validation = CellValidation::new(kind, values, strict).ok()?;
    validation.show_dropdown = show_dropdown;
    Some(validation)
}

fn xlsx_literal_number(value: &str) -> Option<f64> {
    let value = value.trim();
    (!value.starts_with('='))
        .then(|| value.parse::<f64>().ok())
        .flatten()
        .filter(|value| value.is_finite())
}

fn xlsx_xml_reference(reference: &str) -> Option<String> {
    if let Some(value) = quick_xml::escape::resolve_predefined_entity(reference) {
        return Some(value.to_string());
    }
    let digits = reference.strip_prefix('#')?;
    let value = digits.strip_prefix(['x', 'X']).map_or_else(
        || digits.parse::<u32>().ok(),
        |hex| u32::from_str_radix(hex, 16).ok(),
    )?;
    char::from_u32(value).map(|value| value.to_string())
}

fn xlsx_ordered_literal_numbers(first: &str, second: &str) -> bool {
    matches!(
        (xlsx_literal_number(first), xlsx_literal_number(second)),
        (Some(first), Some(second)) if first <= second
    )
}

/// Imports an XLSX workbook (values, formulas, sheet names, merges,
/// date formats) into an OpenDoc workbook, retaining bounded disclosure of
/// package features which have no spreadsheet-model representation.
pub fn import_xlsx_with_warnings(
    bytes: &[u8],
    title: &str,
) -> Result<XlsxImportReport, SpreadsheetError> {
    refuse_dangling_shared_strings(bytes)?;
    let axis_metadata = xlsx_axis_metadata(bytes)?;
    let print_areas = xlsx_print_areas(bytes)?;
    let print_orientations = xlsx_print_orientations(bytes)?;
    let validations = xlsx_validations(bytes)?;
    let legacy_comments = xlsx_legacy_comments(bytes)?;
    let basic_filters = xlsx_basic_filters(bytes)?;
    let cursor = Cursor::new(bytes.to_vec());
    let mut reader: calamine::Xlsx<_> = calamine::open_workbook_from_rs(cursor)
        .map_err(|err| SpreadsheetError::Import(format!("invalid xlsx: {err}")))?;
    // Propagated, not discarded. `calamine` 0.31 keeps the merged-region
    // table as an `Option` and `expect()`s it on the next `worksheet_range`,
    // so a failed load here does not stay here: it becomes a **panic** two
    // lines down. A panic is not an error path in either runtime — on wasm32
    // it is an unrecoverable module trap, and in the native shell it unwinds
    // out of a Tauri command — and the reachability is opening a file someone
    // sent you. A fuzz target found it with one flipped byte in a zip entry
    // name (`xl/sharedStrings.xml` → `xl/sharedStr)ngs.xml`), which is enough
    // to make the relationships name a part that is not there.
    reader
        .load_merged_regions()
        .map_err(|err| SpreadsheetError::Import(format!("invalid xlsx: {err}")))?;
    let sheet_names = reader.sheet_names();
    if sheet_names.is_empty() {
        return Err(SpreadsheetError::Import(
            "xlsx workbook has no sheets".to_string(),
        ));
    }
    let style_import = xlsx_cell_style_import(bytes, &sheet_names)?;
    let mut warnings = xlsx_floating_drawing_warnings(bytes, &sheet_names)?;
    warnings.extend(style_import.warnings.clone());
    for (index, print_area) in print_areas.iter().enumerate() {
        if !print_area.unsupported {
            continue;
        }
        let name = sheet_names
            .get(index)
            .map(String::as_str)
            .unwrap_or("unnamed sheet");
        push_unique_warning(
            &mut warnings,
            "xlsx-print-area-unimported",
            format!(
                "XLSX sheet {name:?} has a print area OpenDoc cannot represent as one rectangular in-grid range"
            ),
        );
    }
    for (index, orientation) in print_orientations.iter().enumerate() {
        if !orientation.unsupported {
            continue;
        }
        let name = sheet_names
            .get(index)
            .map(String::as_str)
            .unwrap_or("unnamed sheet");
        push_unique_warning(
            &mut warnings,
            "xlsx-print-orientation-unimported",
            format!(
                "XLSX sheet {name:?} has a page orientation OpenDoc cannot represent exactly; the XLSX portrait default was retained"
            ),
        );
    }
    for (index, validation) in validations.iter().enumerate() {
        if validation.unsupported_rules == 0 {
            continue;
        }
        let name = sheet_names
            .get(index)
            .map(String::as_str)
            .unwrap_or("unnamed sheet");
        push_unique_warning(
            &mut warnings,
            "xlsx-data-validation-unimported",
            format!(
                "XLSX sheet {name:?} has {} data validation rule(s) OpenDoc cannot represent exactly",
                validation.unsupported_rules
            ),
        );
    }
    for (index, filter) in basic_filters.iter().enumerate() {
        let Some(filter) = filter else {
            continue;
        };
        let name = sheet_names
            .get(index)
            .map(String::as_str)
            .unwrap_or("unnamed sheet");
        if filter.has_unsupported_criteria {
            push_unique_warning(
                &mut warnings,
                "xlsx-filter-criteria-unimported",
                format!(
                    "XLSX sheet {name:?} has AutoFilter criteria OpenDoc cannot represent exactly; compatible one-value text selections were retained"
                ),
            );
        }
        if filter.has_sort {
            push_unique_warning(
                &mut warnings,
                "xlsx-filter-sort-unimported",
                format!(
                    "XLSX sheet {name:?} has AutoFilter sort state; OpenDoc retained its range but did not import the sort"
                ),
            );
        }
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
        if rows > XLSX_IMPORT_MAX_ROWS {
            return Err(SpreadsheetError::Import(format!(
                "xlsx sheet {name} uses {rows} rows, which exceeds the supported limit {XLSX_IMPORT_MAX_ROWS}"
            )));
        }
        if columns > XLSX_IMPORT_MAX_COLUMNS {
            return Err(SpreadsheetError::Import(format!(
                "xlsx sheet {name} uses {columns} columns, which exceeds the supported limit {XLSX_IMPORT_MAX_COLUMNS}"
            )));
        }
        let sheet_id = format!("sheet-{}", index + 1);
        let sheet_title = unique_title(name, &titles);
        titles.insert(sheet_title.clone());
        let mut sheet = blank_sheet(
            &sheet_id,
            &sheet_title,
            rows.max(DEFAULT_SHEET_ROWS),
            columns.max(DEFAULT_SHEET_COLUMNS),
        );
        // OOXML's absent `pageSetup` is portrait. Do not accidentally turn
        // that source default into OpenDoc's unrelated landscape default.
        sheet.set_print_orientation(
            print_orientations
                .get(index)
                .and_then(|item| item.orientation)
                .unwrap_or(SheetPrintOrientation::Portrait),
        );
        if let Some(Some(filter)) = basic_filters.get(index) {
            let range = parse_cell_range(&filter.range)?;
            let last_row = range
                .start_row
                .saturating_add(range.height)
                .saturating_sub(1);
            let last_column = range
                .start_column
                .saturating_add(range.width)
                .saturating_sub(1);
            if last_row <= XLSX_IMPORT_MAX_ROWS && last_column <= XLSX_IMPORT_MAX_COLUMNS {
                super::structure::set_sheet_basic_filter(&mut sheet, &filter.range)?;
                super::structure::set_sheet_basic_filter_options(
                    &mut sheet,
                    filter.criteria.clone(),
                    Vec::new(),
                )?;
            }
        }
        if let Some(metadata) = axis_metadata.get(index) {
            // Dimension-only rows/columns are meaningful in a spreadsheet:
            // Google Sheets commonly sets a print-oriented width far beyond
            // the populated cells. Give those dimensions an address before
            // attaching their model metadata.
            let last_column = metadata
                .column_widths
                .keys()
                .chain(metadata.hidden_columns.iter())
                .copied()
                .max();
            let last_row = metadata
                .row_heights
                .keys()
                .chain(metadata.hidden_rows.iter())
                .copied()
                .max();
            if let (Some(column), Some(row)) = (last_column, last_row) {
                if let Some(column) = number_to_column(column) {
                    sheet.ensure_address(&format!("{column}{row}"));
                }
            } else if let Some(column) = last_column {
                if let Some(column) = number_to_column(column) {
                    sheet.ensure_address(&format!("{column}1"));
                }
            } else if let Some(row) = last_row {
                sheet.ensure_address(&format!("A{row}"));
            }
            sheet
                .column_widths
                .extend(metadata.column_widths.iter().filter_map(|(column, width)| {
                    number_to_column(*column).map(|column| (column, *width))
                }));
            sheet.row_heights.extend(
                metadata
                    .row_heights
                    .iter()
                    .map(|(row, height)| (row.to_string(), *height)),
            );
            sheet.hidden_columns.extend(
                metadata
                    .hidden_columns
                    .iter()
                    .filter_map(|column| number_to_column(*column)),
            );
            sheet
                .hidden_rows
                .extend(metadata.hidden_rows.iter().map(u32::to_string));
        }
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
        if let Some(styles) = style_import.cells.get(index) {
            for (address, style) in styles {
                sheet.ensure_address(address);
                match sheet.cells.iter_mut().find(|cell| cell.address == *address) {
                    Some(cell) => {
                        cell.format.bold = style.bold;
                        cell.format.italic = style.italic;
                        cell.format.text_color = style.text_color.clone();
                        cell.format.horizontal_align = style.horizontal_align.clone();
                        cell.format.wrap_strategy = style.wrap_strategy.clone();
                        cell.format.vertical_align = style.vertical_align.clone();
                        cell.format.number_format = style.number_format.clone();
                        cell.format.background_color = style.background_color.clone();
                    }
                    None => {
                        let mut cell = Cell::new(address, "empty", "");
                        cell.format.bold = style.bold;
                        cell.format.italic = style.italic;
                        cell.format.text_color = style.text_color.clone();
                        cell.format.horizontal_align = style.horizontal_align.clone();
                        cell.format.wrap_strategy = style.wrap_strategy.clone();
                        cell.format.vertical_align = style.vertical_align.clone();
                        cell.format.number_format = style.number_format.clone();
                        cell.format.background_color = style.background_color.clone();
                        sheet.cells.push(cell);
                    }
                }
            }
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
        if let Some(validations) = validations.get(index) {
            for (address, validation) in &validations.cells {
                sheet.ensure_address(address);
                match sheet.cells.iter_mut().find(|cell| cell.address == *address) {
                    Some(cell) => cell.validation = Some(validation.clone()),
                    None => {
                        let mut cell = Cell::new(address, "empty", "");
                        cell.validation = Some(validation.clone());
                        sheet.cells.push(cell);
                    }
                }
            }
        }
        if let Some(comments) = legacy_comments.get(index) {
            for (address, comment) in comments {
                sheet.ensure_address(address);
                match sheet.cells.iter_mut().find(|cell| cell.address == *address) {
                    Some(cell) => cell.comments.push(comment.clone()),
                    None => {
                        let mut cell = Cell::new(address, "empty", "");
                        cell.comments.push(comment.clone());
                        sheet.cells.push(cell);
                    }
                }
            }
        }
        if let Some(Some(print_area)) = print_areas.get(index).map(|item| item.area.as_ref()) {
            let parsed = parse_cell_range(print_area)?;
            // A print box can intentionally extend beyond populated cells.
            // Materialise its final corner before source validation so that
            // the durable sheet grid, not only its values, owns the box.
            let Some(end_column) = number_to_column(parsed.start_column + parsed.width - 1) else {
                return Err(SpreadsheetError::Import(format!(
                    "xlsx sheet {name} print area {print_area} exceeds supported columns"
                )));
            };
            let end_row = parsed.start_row + parsed.height - 1;
            if end_row > XLSX_IMPORT_MAX_ROWS
                || parsed.start_column + parsed.width - 1 > XLSX_IMPORT_MAX_COLUMNS
            {
                return Err(SpreadsheetError::Import(format!(
                    "xlsx sheet {name} print area {print_area} exceeds the supported grid"
                )));
            }
            sheet.ensure_address(&format!("{end_column}{end_row}"));
            sheet.set_print_area(Some(print_area))?;
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
    Ok(XlsxImportReport { workbook, warnings })
}

/// Imports an XLSX workbook where the caller does not need the bounded
/// fidelity disclosures. New UI import paths should use
/// [`import_xlsx_with_warnings`] so those disclosures reach the user.
pub fn import_xlsx(bytes: &[u8], title: &str) -> Result<SpreadsheetWorkbook, SpreadsheetError> {
    Ok(import_xlsx_with_warnings(bytes, title)?.workbook)
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
        && format.wrap_strategy.is_none()
        && format.vertical_align.is_none()
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
    if format.wrap_strategy.as_deref() == Some("wrap") {
        out = out.set_text_wrap();
    }
    match format.vertical_align.as_deref() {
        Some("top") => out = out.set_align(rust_xlsxwriter::FormatAlign::Top),
        Some("middle") => out = out.set_align(rust_xlsxwriter::FormatAlign::VerticalCenter),
        Some("bottom") => out = out.set_align(rust_xlsxwriter::FormatAlign::Bottom),
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

/// Whether an OpenDoc rule has an exact OOXML representation in this writer.
/// Keep this public predicate beside the conversion: the application warning
/// must not guess writer coverage from the rule name alone.
pub fn xlsx_validation_is_representable(validation: &CellValidation) -> bool {
    if validation.validate_source().is_err() {
        return false;
    }
    match validation.kind.as_str() {
        "list" => true,
        "number_greater" | "number_less" => {
            validation.values.len() == 1 && xlsx_literal_number(&validation.values[0]).is_some()
        }
        "number_between" => {
            validation.values.len() == 2
                && xlsx_ordered_literal_numbers(&validation.values[0], &validation.values[1])
        }
        "custom_formula" => validation.values.len() == 1 && !validation.values[0].trim().is_empty(),
        // Excel only has text *length* validation, not an exact contains
        // predicate. Do not translate it into a weaker or different rule.
        "text_contains" => false,
        _ => false,
    }
}

/// Whether an OpenDoc basic filter is the intentionally tiny AutoFilter shape
/// this writer can preserve: one exact text-selection value per column and no
/// sort state. XLSX list filters carry an OR-list rather than a predicate, so
/// this boundary must stay at exactly one selected value.
pub fn xlsx_filter_is_representable(filter: &SheetFilter) -> bool {
    xlsx_filter_criteria_are_representable(filter) && filter.sort_specs.is_empty()
}

fn xlsx_filter_criteria_are_representable(filter: &SheetFilter) -> bool {
    filter.validate_source().is_ok()
        && filter
            .criteria
            .iter()
            .all(|criterion| criterion.condition == "text_equals")
}

fn xlsx_validation(
    cell: &Cell,
) -> Result<Option<rust_xlsxwriter::DataValidation>, SpreadsheetError> {
    let Some(validation) = &cell.validation else {
        return Ok(None);
    };
    if !xlsx_validation_is_representable(validation) {
        return Ok(None);
    }
    let validation = match validation.kind.as_str() {
        "list" => rust_xlsxwriter::DataValidation::new()
            .allow_list_strings(&validation.values)
            .map_err(xlsx_err)?,
        "number_greater" => rust_xlsxwriter::DataValidation::new().allow_decimal_number(
            rust_xlsxwriter::DataValidationRule::GreaterThan(
                xlsx_literal_number(&validation.values[0]).expect("representable number"),
            ),
        ),
        "number_less" => rust_xlsxwriter::DataValidation::new().allow_decimal_number(
            rust_xlsxwriter::DataValidationRule::LessThan(
                xlsx_literal_number(&validation.values[0]).expect("representable number"),
            ),
        ),
        "number_between" => rust_xlsxwriter::DataValidation::new().allow_decimal_number(
            rust_xlsxwriter::DataValidationRule::Between(
                xlsx_literal_number(&validation.values[0]).expect("representable number"),
                xlsx_literal_number(&validation.values[1]).expect("representable number"),
            ),
        ),
        "custom_formula" => rust_xlsxwriter::DataValidation::new()
            .allow_custom(rust_xlsxwriter::Formula::new(&validation.values[0])),
        _ => return Ok(None),
    }
    .show_dropdown(validation.show_dropdown)
    .set_error_style(if validation.strict {
        rust_xlsxwriter::DataValidationErrorStyle::Stop
    } else {
        rust_xlsxwriter::DataValidationErrorStyle::Warning
    });
    Ok(Some(validation))
}

fn xlsx_err(err: rust_xlsxwriter::XlsxError) -> SpreadsheetError {
    SpreadsheetError::Format(format!("xlsx export failed: {err}"))
}

/// Exports the workbook as XLSX bytes (values, formulas, basic formatting,
/// sheet names, column widths, row heights, hidden axes, frozen panes,
/// merges, and named ranges).
pub fn export_xlsx(workbook: &SpreadsheetWorkbook) -> Result<Vec<u8>, SpreadsheetError> {
    // `rust_xlsxwriter` enables ZIP's Zopfli backend. Its writer enum contains
    // the Zopfli encoder inline, which exceeds the relatively small stacks used
    // by browser hosts and some native embedding environments. Constructing the
    // workbook on this worker keeps that implementation detail out of the UI
    // command stack without changing the XLSX package or its compression.
    #[cfg(not(target_arch = "wasm32"))]
    {
        let workbook = workbook.clone();
        std::thread::Builder::new()
            .name("opendoc-xlsx-export".to_string())
            .stack_size(32 * 1024 * 1024)
            .spawn(move || export_xlsx_inner(&workbook))
            .map_err(|error| {
                SpreadsheetError::Format(format!("could not start XLSX export worker: {error}"))
            })?
            .join()
            .map_err(|_| SpreadsheetError::Format("XLSX export worker panicked".to_string()))?
    }

    // Browser builds cannot create OS threads. Their runtime owns the WebAssembly
    // stack, and `rust_xlsxwriter`'s `wasm` feature supplies the matching clock.
    #[cfg(target_arch = "wasm32")]
    export_xlsx_inner(workbook)
}

fn export_xlsx_inner(workbook: &SpreadsheetWorkbook) -> Result<Vec<u8>, SpreadsheetError> {
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
            if let Some(validation) = xlsx_validation(cell)? {
                worksheet
                    .add_data_validation(row, col, row, col, &validation)
                    .map_err(xlsx_err)?;
            }
            if let Some(comment) = cell.comments.iter().find(|comment| !comment.deleted) {
                let note = rust_xlsxwriter::Note::new(&comment.body).set_author(&comment.author);
                worksheet.insert_note(row, col, &note).map_err(xlsx_err)?;
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
        if let Some(filter) = sheet.filters.first() {
            let range = parse_cell_range(&filter.range)?;
            worksheet
                .autofilter(
                    range.start_row - 1,
                    (range.start_column - 1) as u16,
                    range.start_row + range.height - 2,
                    (range.start_column + range.width - 2) as u16,
                )
                .map_err(xlsx_err)?;
            if xlsx_filter_criteria_are_representable(filter) {
                for criterion in &filter.criteria {
                    let column = column_to_number(&criterion.column).ok_or_else(|| {
                        SpreadsheetError::Format(format!(
                            "invalid XLSX filter column {}",
                            criterion.column
                        ))
                    })?;
                    let condition = rust_xlsxwriter::FilterCondition::new()
                        .add_list_filter(criterion.value.as_str());
                    worksheet
                        .filter_column((column - 1) as u16, &condition)
                        .map_err(xlsx_err)?;
                }
            }
        }
        if let Some(print_area) = &sheet.print_settings.print_area {
            let range = parse_cell_range(print_area)?;
            worksheet
                .set_print_area(
                    range.start_row - 1,
                    (range.start_column - 1) as u16,
                    range.start_row + range.height - 2,
                    (range.start_column + range.width - 2) as u16,
                )
                .map_err(xlsx_err)?;
        }
        match sheet.print_settings.orientation {
            SheetPrintOrientation::Portrait => {
                worksheet.set_portrait();
            }
            SheetPrintOrientation::Landscape => {
                worksheet.set_landscape();
            }
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

    /// One cell at row 65,000 in a 5 KB file used to cost 15 seconds and
    /// 65,000 materialised rows, scaling quadratically toward an hour at the
    /// XLSX row maximum. The grid is dense, so the only honest answer is to
    /// refuse the import and say why — never to truncate the sheet and let
    /// the user discover the missing rows later.
    #[test]
    fn xlsx_import_refuses_a_sheet_larger_than_the_supported_grid() {
        let mut book = rust_xlsxwriter::Workbook::new();
        let worksheet = book.add_worksheet();
        worksheet.write_string(0, 0, "top").unwrap();
        worksheet.write_number(65_000, 0, 1.0).unwrap();
        let bytes = book.save_to_buffer().unwrap();
        assert!(bytes.len() < 64 * 1024, "fixture is {} bytes", bytes.len());

        let started = std::time::Instant::now();
        let error = super::import_xlsx(&bytes, "Huge").expect_err("a 65,000-row sheet is refused");
        let elapsed = started.elapsed();

        let message = format!("{error:?}");
        assert!(message.contains("65001"), "{message}");
        assert!(
            message.contains(&super::XLSX_IMPORT_MAX_ROWS.to_string()),
            "{message}"
        );
        // The refusal has to come *before* the grid is built. If this ever
        // takes seconds, the cap is not being applied where it looks like
        // it is.
        assert!(elapsed.as_millis() < 500, "import took {elapsed:?}");

        let mut wide = rust_xlsxwriter::Workbook::new();
        let worksheet = wide.add_worksheet();
        worksheet.write_number(0, 2_000, 1.0).unwrap();
        let error = super::import_xlsx(&wide.save_to_buffer().unwrap(), "Wide")
            .expect_err("a 2,001-column sheet is refused");
        assert!(format!("{error:?}").contains("columns"), "{error:?}");
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
        // The bounded XLSX style reader owns horizontal alignment. Other
        // format facts remain separately loss-bounded below.
        // Defined names survive, which is only true because the importer
        // reads them back.
        assert_eq!(reopened.named_ranges.len(), 1);
        assert_eq!(reopened.named_ranges[0].name, "COUNTS");
        assert_eq!(reopened.named_ranges[0].range, "B1:B3");
    }

    #[test]
    fn xlsx_round_trips_a_rectangular_print_area_including_blank_cells() {
        let mut workbook = SpreadsheetWorkbook::sample();
        workbook.sheets[0].ensure_address("D140");
        workbook
            .set_print_area("sheet-1", Some("B2:D140"))
            .expect("sample sheet exists")
            .expect("area is in the grid");

        let bytes = super::export_xlsx(&workbook).expect("XLSX exports");
        let report = super::import_xlsx_with_warnings(&bytes, "Reread").expect("XLSX imports");
        let sheet = &report.workbook.sheets[0];
        assert_eq!(sheet.print_settings.print_area.as_deref(), Some("B2:D140"));
        assert!(sheet.rows.contains(&"140".to_string()));
        assert!(sheet.columns.contains(&"D".to_string()));
        assert!(report
            .warnings
            .iter()
            .all(|warning| warning.code != "xlsx-print-area-unimported"));
    }

    #[test]
    fn xlsx_round_trips_each_owned_print_orientation() {
        use crate::SheetPrintOrientation;

        let mut workbook = SpreadsheetWorkbook::sample();
        workbook.add_sheet_with_id("sheet-2", "Portrait");
        workbook
            .set_print_orientation("sheet-2", SheetPrintOrientation::Portrait)
            .expect("sheet exists");

        let report = super::import_xlsx_with_warnings(
            &super::export_xlsx(&workbook).expect("XLSX exports"),
            "Reread",
        )
        .expect("XLSX imports");
        assert_eq!(
            report.workbook.sheets[0].print_settings.orientation,
            SheetPrintOrientation::Landscape
        );
        assert_eq!(
            report.workbook.sheets[1].print_settings.orientation,
            SheetPrintOrientation::Portrait
        );
        assert!(report
            .warnings
            .iter()
            .all(|warning| warning.code != "xlsx-print-orientation-unimported"));
    }

    #[test]
    fn xlsx_unknown_print_orientation_is_disclosed_without_rejecting_grid() {
        use std::io::{Cursor, Read, Write};

        let source = super::export_xlsx(&SpreadsheetWorkbook::sample()).expect("XLSX exports");
        let mut archive = zip::ZipArchive::new(Cursor::new(source)).expect("export is zip");
        let names: Vec<String> = archive.file_names().map(ToString::to_string).collect();
        let mut rewritten = Vec::new();
        {
            let mut writer = zip::ZipWriter::new(Cursor::new(&mut rewritten));
            for name in names {
                let mut entry = archive.by_name(&name).expect("listed entry opens");
                let mut body = Vec::new();
                entry.read_to_end(&mut body).expect("entry inflates");
                if name == "xl/worksheets/sheet1.xml" {
                    let sheet = String::from_utf8(body).expect("writer makes UTF-8 XML");
                    assert!(sheet.contains("orientation=\"landscape\""), "{sheet}");
                    body = sheet
                        .replacen("orientation=\"landscape\"", "orientation=\"sideways\"", 1)
                        .into_bytes();
                }
                writer
                    .start_file(name, zip::write::SimpleFileOptions::default())
                    .expect("entry starts");
                writer.write_all(&body).expect("entry writes");
            }
            writer.finish().expect("fixture closes");
        }

        let report = super::import_xlsx_with_warnings(&rewritten, "Read")
            .expect("unknown page setup does not reject the safe grid");
        assert_eq!(user_value(&report.workbook, 0, "A1"), "Item");
        assert_eq!(
            report.workbook.sheets[0].print_settings.orientation,
            crate::SheetPrintOrientation::Portrait,
            "the XLSX default is retained instead of inventing a nearby orientation"
        );
        assert!(report
            .warnings
            .iter()
            .any(|warning| warning.code == "xlsx-print-orientation-unimported"));
    }

    #[test]
    fn xlsx_print_area_parser_rejects_unions_and_non_rectangles() {
        assert_eq!(
            super::xlsx_print_area_formula("'Sheet name'!$B$2:$D$4"),
            Some("B2:D4".to_string())
        );
        assert_eq!(super::xlsx_print_area_formula("Sheet1!$B$2,$D$4"), None);
        assert_eq!(super::xlsx_print_area_formula("Sheet1!$2:$4"), None);
        assert_eq!(
            super::xlsx_print_area_formula("[other.xlsx]Sheet1!$B$2:$D$4"),
            None
        );
    }

    #[test]
    fn xlsx_print_area_preflight_marks_a_foreign_union_unimportable() {
        use std::io::{Cursor, Write};

        let mut bytes = Vec::new();
        {
            let mut writer = zip::ZipWriter::new(Cursor::new(&mut bytes));
            writer
                .start_file("xl/workbook.xml", zip::write::SimpleFileOptions::default())
                .unwrap();
            writer
                .write_all(
                    br#"<?xml version="1.0" encoding="UTF-8"?><workbook><definedNames><definedName name="_xlnm.Print_Area" localSheetId="0">Sheet1!$B$2,$D$4</definedName></definedNames></workbook>"#,
                )
                .unwrap();
            writer.finish().unwrap();
        }

        let areas = super::xlsx_print_areas(&bytes).expect("preflight reads workbook part");
        assert!(areas[0].unsupported);
        assert_eq!(areas[0].area, None);
    }

    #[test]
    fn xlsx_round_trips_exact_numeric_and_custom_validations() {
        let mut workbook = SpreadsheetWorkbook::sample();
        for (address, kind, values, strict, show_dropdown) in [
            ("C1", "number_greater", vec!["1.5"], true, true),
            ("C2", "number_less", vec!["9"], false, false),
            ("C3", "number_between", vec!["-2", "3.25"], true, true),
            ("D1", "custom_formula", vec!["=LEN(D1)>2"], false, true),
        ] {
            let mut rule = super::CellValidation::new(
                kind,
                values.into_iter().map(str::to_string).collect(),
                strict,
            )
            .unwrap();
            rule.show_dropdown = show_dropdown;
            workbook
                .set_cell_validation("sheet-1", address, rule)
                .unwrap();
        }

        let bytes = super::export_xlsx(&workbook).unwrap();
        let report = super::import_xlsx_with_warnings(&bytes, "Reread").unwrap();
        let cell = |address: &str| {
            report.workbook.sheets[0]
                .cells
                .iter()
                .find(|cell| cell.address == address)
                .and_then(|cell| cell.validation.as_ref())
                .cloned()
                .unwrap_or_else(|| panic!("missing validation at {address}"))
        };
        assert_eq!(cell("C1").kind, "number_greater");
        assert_eq!(cell("C1").values, ["1.5"]);
        assert!(cell("C1").strict);
        assert_eq!(cell("C2").kind, "number_less");
        assert_eq!(cell("C2").values, ["9"]);
        assert!(!cell("C2").strict);
        assert!(!cell("C2").show_dropdown);
        assert_eq!(cell("C3").kind, "number_between");
        assert_eq!(cell("C3").values, ["-2", "3.25"]);
        assert_eq!(cell("D1").kind, "custom_formula");
        assert_eq!(cell("D1").values, ["=LEN(D1)>2"]);
        assert!(report
            .warnings
            .iter()
            .all(|warning| warning.code != "xlsx-data-validation-unimported"));
    }

    #[test]
    fn xlsx_unrepresentable_validation_is_disclosed_not_recast() {
        let mut book = rust_xlsxwriter::Workbook::new();
        let worksheet = book.add_worksheet();
        let rule = rust_xlsxwriter::DataValidation::new()
            .allow_whole_number(rust_xlsxwriter::DataValidationRule::GreaterThan(0));
        worksheet.add_data_validation(0, 0, 0, 0, &rule).unwrap();
        let report = super::import_xlsx_with_warnings(&book.save_to_buffer().unwrap(), "Read")
            .expect("the XLSX file imports");
        assert!(report.workbook.sheets[0]
            .cells
            .iter()
            .all(|cell| cell.validation.is_none()));
        assert!(report
            .warnings
            .iter()
            .any(|warning| warning.code == "xlsx-data-validation-unimported"));
    }

    #[test]
    fn xlsx_round_trips_one_value_text_autofilter_selection() {
        let mut book = rust_xlsxwriter::Workbook::new();
        let worksheet = book.add_worksheet();
        worksheet.write_string(0, 0, "Region").unwrap();
        worksheet.write_string(1, 0, "East").unwrap();
        worksheet.write_string(2, 0, "West").unwrap();
        worksheet.autofilter(0, 0, 2, 0).unwrap();
        let criterion = rust_xlsxwriter::FilterCondition::new().add_list_filter("East");
        worksheet.filter_column(0, &criterion).unwrap();

        let report = super::import_xlsx_with_warnings(&book.save_to_buffer().unwrap(), "Read")
            .expect("the XLSX file imports");
        assert_eq!("A1:A3", report.workbook.sheets[0].filters[0].range);
        assert_eq!(
            report.workbook.sheets[0].filters[0].criteria,
            vec![crate::SheetFilterCriterion {
                column: "A".to_string(),
                condition: "text_equals".to_string(),
                value: "East".to_string(),
            }]
        );
        assert!(report
            .warnings
            .iter()
            .all(|warning| warning.code != "xlsx-filter-criteria-unimported"));
        let reread = super::import_xlsx_with_warnings(
            &super::export_xlsx(&report.workbook).expect("the model exports"),
            "Reread",
        )
        .expect("the exported XLSX reimports");
        assert_eq!(
            reread.workbook.sheets[0].filters[0].criteria,
            report.workbook.sheets[0].filters[0].criteria
        );
    }

    #[test]
    fn xlsx_multi_value_autofilter_selection_is_disclosed_not_recast() {
        let mut book = rust_xlsxwriter::Workbook::new();
        let worksheet = book.add_worksheet();
        worksheet.write_string(0, 0, "Region").unwrap();
        worksheet.write_string(1, 0, "East").unwrap();
        worksheet.write_string(2, 0, "West").unwrap();
        worksheet.autofilter(0, 0, 2, 0).unwrap();
        let criterion = rust_xlsxwriter::FilterCondition::new()
            .add_list_filter("East")
            .add_list_filter("West");
        worksheet.filter_column(0, &criterion).unwrap();
        let report = super::import_xlsx_with_warnings(&book.save_to_buffer().unwrap(), "Read")
            .expect("the XLSX file imports");
        assert!(report.workbook.sheets[0].filters[0].criteria.is_empty());
        assert!(report
            .warnings
            .iter()
            .any(|warning| warning.code == "xlsx-filter-criteria-unimported"));
    }

    #[test]
    fn xlsx_imports_owned_alignment_and_wrap_without_style_loss() {
        let mut workbook = SpreadsheetWorkbook::sample();
        workbook
            .set_cell_format("sheet-1", "A1", "horizontal_align", "center".to_string())
            .unwrap()
            .unwrap();
        workbook
            .set_cell_format("sheet-1", "A1", "vertical_align", "middle".to_string())
            .unwrap()
            .unwrap();
        workbook
            .set_cell_format("sheet-1", "A1", "wrap_strategy", "wrap".to_string())
            .unwrap()
            .unwrap();
        let bytes = super::export_xlsx(&workbook).unwrap();
        let report = super::import_xlsx_with_warnings(&bytes, "Reopened").unwrap();
        let cell = report.workbook.sheets[0]
            .cells
            .iter()
            .find(|cell| cell.address == "A1")
            .expect("A1 survives");
        assert_eq!(cell.format.horizontal_align.as_deref(), Some("center"));
        assert_eq!(cell.format.wrap_strategy.as_deref(), Some("wrap"));
        assert_eq!(cell.format.vertical_align.as_deref(), Some("middle"));
        assert!(
            report
                .warnings
                .iter()
                .all(|warning| warning.code != "xlsx-cell-styles-unimported"),
            "{:?}",
            report.warnings
        );
    }

    #[test]
    fn xlsx_round_trips_opaque_srgb_solid_background_fills_on_values_and_blanks() {
        let mut workbook = SpreadsheetWorkbook::sample();
        for address in ["A1", "D5"] {
            workbook
                .set_cell_format(
                    "sheet-1",
                    address,
                    "background_color",
                    "#336699".to_string(),
                )
                .unwrap()
                .unwrap();
        }

        let report = super::import_xlsx_with_warnings(
            &super::export_xlsx(&workbook).expect("XLSX exports"),
            "Reopened",
        )
        .expect("XLSX imports");
        for address in ["A1", "D5"] {
            let cell = report.workbook.sheets[0]
                .cells
                .iter()
                .find(|cell| cell.address == address)
                .unwrap_or_else(|| panic!("{address} survives"));
            assert_eq!(cell.format.background_color.as_deref(), Some("#336699"));
        }
        assert_eq!(
            report.workbook.sheets[0]
                .cells
                .iter()
                .find(|cell| cell.address == "D5")
                .expect("formatted blank survives")
                .user_kind,
            "empty"
        );
        assert!(
            report
                .warnings
                .iter()
                .all(|warning| warning.code != "xlsx-cell-styles-unimported"),
            "{:?}",
            report.warnings
        );
    }

    #[test]
    fn xlsx_round_trips_bold_and_italic_font_switches_on_values_and_blanks() {
        let mut workbook = SpreadsheetWorkbook::sample();
        for address in ["A1", "D5"] {
            workbook
                .set_cell_format("sheet-1", address, "bold", "true".to_string())
                .unwrap()
                .unwrap();
            workbook
                .set_cell_format("sheet-1", address, "italic", "true".to_string())
                .unwrap()
                .unwrap();
        }

        let report = super::import_xlsx_with_warnings(
            &super::export_xlsx(&workbook).expect("XLSX exports"),
            "Reopened",
        )
        .expect("XLSX imports");
        for address in ["A1", "D5"] {
            let cell = report.workbook.sheets[0]
                .cells
                .iter()
                .find(|cell| cell.address == address)
                .unwrap_or_else(|| panic!("{address} survives"));
            assert!(cell.format.bold);
            assert!(cell.format.italic);
        }
        assert_eq!(
            report.workbook.sheets[0]
                .cells
                .iter()
                .find(|cell| cell.address == "D5")
                .expect("formatted blank survives")
                .user_kind,
            "empty"
        );
        assert!(report.warnings.iter().any(|warning| {
            warning.code == "xlsx-cell-styles-unimported"
                && warning.message.contains("bold/italic switches")
        }));
    }

    #[test]
    fn xlsx_font_reader_retains_exact_text_colours_but_discloses_other_font_features() {
        let fonts = super::xlsx_font_import(
            br#"
                <styleSheet><fonts count="7">
                  <font/>
                  <font><b/><i val="true"/></font>
                  <font><b/><u/></font>
                  <font><color rgb="FF336699"/></font>
                  <font><color theme="1"/></font>
                  <font><color rgb="FF336699" tint="0.5"/></font>
                  <font><u val="none"/></font>
                  <font><strike/></font>
                  <font><strike val="false"/></font>
                </fonts></styleSheet>
            "#,
        )
        .expect("font fixture parses");
        assert_eq!(fonts.len(), 9);
        assert!(!fonts[0].bold && !fonts[0].italic && !fonts[0].has_unimported_parts);
        assert!(fonts[1].bold && fonts[1].italic && !fonts[1].has_unimported_parts);
        assert!(fonts[2].bold && !fonts[2].italic && fonts[2].has_unimported_parts);
        assert!(fonts[2].has_unimported_underline);
        assert_eq!(fonts[3].text_color.as_deref(), Some("#336699"));
        assert!(!fonts[3].has_unimported_parts);
        for font in &fonts[4..6] {
            assert_eq!(font.text_color, None);
            assert!(font.has_unimported_parts);
        }
        assert!(!fonts[6].has_unimported_parts);
        assert!(!fonts[6].has_unimported_underline);
        assert!(fonts[7].has_unimported_parts);
        assert!(fonts[7].has_unimported_strikethrough);
        assert!(!fonts[8].has_unimported_parts);
        assert!(!fonts[8].has_unimported_strikethrough);
    }

    #[test]
    fn xlsx_font_decorations_are_disclosed_separately_from_other_style_loss() {
        use std::io::{Cursor, Read, Write};

        let mut workbook = SpreadsheetWorkbook::sample();
        workbook
            .set_cell_format("sheet-1", "A1", "bold", "true".to_string())
            .unwrap()
            .unwrap();
        let source = super::export_xlsx(&workbook).expect("XLSX exports");
        let mut archive = zip::ZipArchive::new(Cursor::new(source)).expect("export is zip");
        let names: Vec<String> = archive.file_names().map(ToString::to_string).collect();
        let mut rewritten = Vec::new();
        {
            let mut writer = zip::ZipWriter::new(Cursor::new(&mut rewritten));
            for name in names {
                let mut entry = archive.by_name(&name).expect("listed entry opens");
                let mut body = Vec::new();
                entry.read_to_end(&mut body).expect("entry inflates");
                if name == "xl/styles.xml" {
                    let styles = String::from_utf8(body).expect("writer makes UTF-8 XML");
                    assert!(styles.contains("<b/>"), "{styles}");
                    body = styles.replacen("<b/>", "<b/><u/><strike/>", 1).into_bytes();
                }
                writer
                    .start_file(name, zip::write::SimpleFileOptions::default())
                    .expect("entry starts");
                writer.write_all(&body).expect("entry writes");
            }
            writer.finish().expect("fixture closes");
        }

        let report = super::import_xlsx_with_warnings(&rewritten, "Reopened")
            .expect("the supported value and bold switch still import");
        let cell = report.workbook.sheets[0]
            .cells
            .iter()
            .find(|cell| cell.address == "A1")
            .expect("A1 survives");
        assert!(cell.format.bold);
        assert!(report.warnings.iter().any(|warning| {
            warning.code == "xlsx-cell-font-underlines-unimported"
                && warning
                    .message
                    .contains("has 1 cell with an underlined font")
        }));
        assert!(report.warnings.iter().any(|warning| {
            warning.code == "xlsx-cell-font-strikethrough-unimported"
                && warning
                    .message
                    .contains("has 1 cell with a strike-through font")
        }));
        assert!(report
            .warnings
            .iter()
            .any(|warning| warning.code == "xlsx-cell-styles-unimported"));
    }

    #[test]
    fn xlsx_round_trips_opaque_srgb_text_colour_on_values_and_blanks() {
        let mut workbook = SpreadsheetWorkbook::sample();
        for address in ["A1", "D5"] {
            workbook
                .set_cell_format("sheet-1", address, "text_color", "#336699".to_string())
                .unwrap()
                .unwrap();
        }

        let report = super::import_xlsx_with_warnings(
            &super::export_xlsx(&workbook).expect("XLSX exports"),
            "Reopened",
        )
        .expect("XLSX imports");
        for address in ["A1", "D5"] {
            let cell = report.workbook.sheets[0]
                .cells
                .iter()
                .find(|cell| cell.address == address)
                .unwrap_or_else(|| panic!("{address} survives"));
            assert_eq!(cell.format.text_color.as_deref(), Some("#336699"));
        }
        assert_eq!(
            report.workbook.sheets[0]
                .cells
                .iter()
                .find(|cell| cell.address == "D5")
                .expect("formatted blank survives")
                .user_kind,
            "empty"
        );
    }

    #[test]
    fn xlsx_theme_font_colour_is_disclosed_without_guessing_a_text_colour() {
        use std::io::{Cursor, Read, Write};

        let mut workbook = SpreadsheetWorkbook::sample();
        workbook
            .set_cell_format("sheet-1", "A1", "text_color", "#336699".to_string())
            .unwrap()
            .unwrap();
        let source = super::export_xlsx(&workbook).expect("XLSX exports");
        let mut archive = zip::ZipArchive::new(Cursor::new(source)).expect("export is zip");
        let names: Vec<String> = archive.file_names().map(ToString::to_string).collect();
        let mut rewritten = Vec::new();
        {
            let mut writer = zip::ZipWriter::new(Cursor::new(&mut rewritten));
            for name in names {
                let mut entry = archive.by_name(&name).expect("listed entry opens");
                let mut body = Vec::new();
                entry.read_to_end(&mut body).expect("entry inflates");
                if name == "xl/styles.xml" {
                    let styles = String::from_utf8(body).expect("writer makes UTF-8 XML");
                    assert!(styles.contains(r#"color rgb="FF336699""#), "{styles}");
                    body = styles
                        .replacen(r#"color rgb="FF336699""#, r#"color theme="1""#, 1)
                        .into_bytes();
                }
                writer
                    .start_file(name, zip::write::SimpleFileOptions::default())
                    .expect("entry starts");
                writer.write_all(&body).expect("entry writes");
            }
            writer.finish().expect("fixture closes");
        }

        let report = super::import_xlsx_with_warnings(&rewritten, "Reopened")
            .expect("the value still imports");
        let cell = report.workbook.sheets[0]
            .cells
            .iter()
            .find(|cell| cell.address == "A1")
            .expect("A1 survives");
        assert_eq!(cell.format.text_color, None);
        assert!(report.warnings.iter().any(|warning| {
            warning.code == "xlsx-cell-styles-unimported"
                && warning.message.contains("opaque sRGB text colours")
        }));
    }

    #[test]
    fn xlsx_fill_reader_discloses_theme_indexed_alpha_and_gradient_encodings() {
        let styles = br#"
            <styleSheet>
              <fills count="6">
                <fill><patternFill patternType="none"/></fill>
                <fill><patternFill patternType="gray125"/></fill>
                <fill><patternFill patternType="solid"><fgColor theme="1"/></patternFill></fill>
                <fill><patternFill patternType="solid"><fgColor indexed="64"/></patternFill></fill>
                <fill><patternFill patternType="solid"><fgColor rgb="80336699"/></patternFill></fill>
                <fill><gradientFill><stop position="0"><color rgb="ff336699"/></stop></gradientFill></fill>
              </fills>
            </styleSheet>
        "#;
        let fills = super::xlsx_fill_import(styles).expect("fixture parses");
        assert_eq!(fills.len(), 6);
        assert!(fills[0].background_color.is_none() && !fills[0].has_unimported_parts);
        for fill in &fills[1..] {
            assert_eq!(fill.background_color, None);
            assert!(fill.has_unimported_parts);
        }
    }

    #[test]
    fn xlsx_theme_fill_is_disclosed_without_recasting_it_as_a_flat_colour() {
        use std::io::{Cursor, Read, Write};

        let mut workbook = SpreadsheetWorkbook::sample();
        workbook
            .set_cell_format("sheet-1", "A1", "background_color", "#336699".to_string())
            .unwrap()
            .unwrap();
        let source = super::export_xlsx(&workbook).expect("XLSX exports");
        let mut archive = zip::ZipArchive::new(Cursor::new(source)).expect("export is zip");
        let names: Vec<String> = archive.file_names().map(ToString::to_string).collect();
        let mut rewritten = Vec::new();
        {
            let mut writer = zip::ZipWriter::new(Cursor::new(&mut rewritten));
            for name in names {
                let mut entry = archive.by_name(&name).expect("listed entry opens");
                let mut body = Vec::new();
                entry.read_to_end(&mut body).expect("entry inflates");
                if name == "xl/styles.xml" {
                    let styles = String::from_utf8(body).expect("writer makes UTF-8 XML");
                    assert!(styles.contains(r#"fgColor rgb="FF336699""#), "{styles}");
                    body = styles
                        .replacen(r#"fgColor rgb="FF336699""#, r#"fgColor theme="1""#, 1)
                        .into_bytes();
                }
                writer
                    .start_file(name, zip::write::SimpleFileOptions::default())
                    .expect("entry starts");
                writer.write_all(&body).expect("entry writes");
            }
            writer.finish().expect("fixture closes");
        }

        let report = super::import_xlsx_with_warnings(&rewritten, "Reopened")
            .expect("the value still imports");
        let cell = report.workbook.sheets[0]
            .cells
            .iter()
            .find(|cell| cell.address == "A1")
            .expect("A1 survives");
        assert_eq!(cell.format.background_color, None);
        assert!(report.warnings.iter().any(|warning| {
            warning.code == "xlsx-cell-styles-unimported"
                && warning
                    .message
                    .contains("opaque solid sRGB background fills")
        }));
    }

    #[test]
    fn xlsx_imports_wrap_on_a_formatted_blank_cell() {
        let mut workbook = SpreadsheetWorkbook::sample();
        // No value is authored in D5.  The OOXML cell exists solely because
        // it has a style, which is the case calamine's value iterator cannot
        // reveal on its own.
        workbook
            .set_cell_format("sheet-1", "D5", "wrap_strategy", "wrap".to_string())
            .unwrap()
            .unwrap();

        let report = super::import_xlsx_with_warnings(
            &super::export_xlsx(&workbook).expect("XLSX exports"),
            "Reopened",
        )
        .expect("XLSX imports");
        let cell = report.workbook.sheets[0]
            .cells
            .iter()
            .find(|cell| cell.address == "D5")
            .expect("formatted blank cell survives");
        assert_eq!(cell.user_kind, "empty");
        assert_eq!(cell.user_value, "");
        assert_eq!(cell.format.wrap_strategy.as_deref(), Some("wrap"));
        assert!(report
            .warnings
            .iter()
            .all(|warning| warning.code != "xlsx-cell-styles-unimported"));
    }

    #[test]
    fn xlsx_imports_exact_custom_number_formats_on_values_and_blanks() {
        let mut workbook = SpreadsheetWorkbook::sample();
        let pattern = "$#,##0.00_);[Red]($#,##0.00)";
        workbook
            .set_cell_format("sheet-1", "A1", "number_format", pattern.to_string())
            .unwrap()
            .unwrap();
        // The cell is deliberately value-free: styles still have to be read
        // from worksheet XML, because calamine's value range omits it.
        workbook
            .set_cell_format("sheet-1", "D5", "number_format", pattern.to_string())
            .unwrap()
            .unwrap();

        let report = super::import_xlsx_with_warnings(
            &super::export_xlsx(&workbook).expect("XLSX exports"),
            "Reopened",
        )
        .expect("XLSX imports");
        for address in ["A1", "D5"] {
            let cell = report.workbook.sheets[0]
                .cells
                .iter()
                .find(|cell| cell.address == address)
                .unwrap_or_else(|| panic!("{address} survives"));
            assert_eq!(cell.format.number_format.as_deref(), Some(pattern));
        }
        assert_eq!(
            report.workbook.sheets[0]
                .cells
                .iter()
                .find(|cell| cell.address == "D5")
                .expect("formatted blank survives")
                .user_kind,
            "empty"
        );
        assert!(report
            .warnings
            .iter()
            .all(|warning| warning.code != "xlsx-cell-styles-unimported"));
    }

    #[test]
    fn xlsx_imports_stable_builtin_number_format_without_style_loss() {
        let mut workbook = SpreadsheetWorkbook::sample();
        workbook
            .set_cell_format("sheet-1", "A1", "number_format", "0.00%".to_string())
            .unwrap()
            .unwrap();

        let report = super::import_xlsx_with_warnings(
            &super::export_xlsx(&workbook).expect("XLSX exports"),
            "Reopened",
        )
        .expect("XLSX imports");
        let cell = report.workbook.sheets[0]
            .cells
            .iter()
            .find(|cell| cell.address == "A1")
            .expect("A1 survives");
        assert_eq!(cell.format.number_format.as_deref(), Some("0.00%"));
        assert!(report
            .warnings
            .iter()
            .all(|warning| warning.code != "xlsx-cell-styles-unimported"));
    }

    #[test]
    fn xlsx_discloses_a_locale_resolved_builtin_number_format() {
        use std::io::{Cursor, Read, Write};

        let mut workbook = SpreadsheetWorkbook::sample();
        workbook
            .set_cell_format("sheet-1", "A1", "number_format", "0.0000".to_string())
            .unwrap()
            .unwrap();
        let source = super::export_xlsx(&workbook).expect("XLSX exports");
        let mut archive = zip::ZipArchive::new(Cursor::new(source)).expect("export is zip");
        let names: Vec<String> = archive.file_names().map(ToString::to_string).collect();
        let mut rewritten = Vec::new();
        {
            let mut writer = zip::ZipWriter::new(Cursor::new(&mut rewritten));
            for name in names {
                let mut entry = archive.by_name(&name).expect("listed entry opens");
                let mut body = Vec::new();
                entry.read_to_end(&mut body).expect("entry inflates");
                if name == "xl/styles.xml" {
                    let mut styles = String::from_utf8(body).expect("writer makes UTF-8 XML");
                    // Keep the custom `<numFmt>` declaration but make the
                    // cell XF refer to Excel's locale-resolved date id 14.
                    // That id has no source-stable textual code, so it must
                    // not be guessed as a format OpenDoc owns.
                    let style_id = styles
                        .rfind("numFmtId=\"164\"")
                        .expect("custom XF uses the first custom id");
                    styles.replace_range(
                        style_id..style_id + "numFmtId=\"164\"".len(),
                        "numFmtId=\"14\"",
                    );
                    body = styles.into_bytes();
                }
                writer
                    .start_file(name, zip::write::SimpleFileOptions::default())
                    .expect("entry starts");
                writer.write_all(&body).expect("entry writes");
            }
            writer.finish().expect("fixture closes");
        }

        let report = super::import_xlsx_with_warnings(&rewritten, "Reopened")
            .expect("the value still imports");
        let cell = report.workbook.sheets[0]
            .cells
            .iter()
            .find(|cell| cell.address == "A1")
            .expect("A1 survives");
        assert_eq!(cell.format.number_format, None);
        assert!(report.warnings.iter().any(|warning| {
            warning.code == "xlsx-cell-styles-unimported"
                && warning.message.contains("stable number formats")
        }));
    }

    #[test]
    fn xlsx_imports_wrap_but_discloses_a_mixed_unowned_alignment_switch() {
        use std::io::{Cursor, Read, Write};

        let mut workbook = SpreadsheetWorkbook::sample();
        workbook
            .set_cell_format("sheet-1", "A1", "wrap_strategy", "wrap".to_string())
            .unwrap()
            .unwrap();
        let source = super::export_xlsx(&workbook).expect("XLSX exports");
        let mut archive = zip::ZipArchive::new(Cursor::new(source)).expect("export is zip");
        let names: Vec<String> = archive.file_names().map(ToString::to_string).collect();
        let mut rewritten = Vec::new();
        {
            let mut writer = zip::ZipWriter::new(Cursor::new(&mut rewritten));
            for name in names {
                let mut entry = archive.by_name(&name).expect("listed entry opens");
                let mut body = Vec::new();
                entry.read_to_end(&mut body).expect("entry inflates");
                if name == "xl/styles.xml" {
                    let styles = String::from_utf8(body).expect("writer makes UTF-8 XML");
                    assert!(styles.contains("wrapText=\"1\""));
                    body = styles
                        .replacen("wrapText=\"1\"", "wrapText=\"1\" shrinkToFit=\"1\"", 1)
                        .into_bytes();
                }
                writer
                    .start_file(name, zip::write::SimpleFileOptions::default())
                    .expect("entry starts");
                writer.write_all(&body).expect("entry writes");
            }
            writer.finish().expect("fixture closes");
        }

        let report = super::import_xlsx_with_warnings(&rewritten, "Reopened")
            .expect("the supported portion imports");
        let cell = report.workbook.sheets[0]
            .cells
            .iter()
            .find(|cell| cell.address == "A1")
            .expect("A1 survives");
        assert_eq!(cell.format.wrap_strategy.as_deref(), Some("wrap"));
        assert!(report.warnings.iter().any(|warning| {
            warning.code == "xlsx-cell-styles-unimported" && warning.message.contains("wrap-text")
        }));
    }

    #[test]
    fn xlsx_discloses_authored_cell_borders_without_recasting_them_as_gridlines() {
        use rust_xlsxwriter::{Format, FormatBorder};

        let mut book = rust_xlsxwriter::Workbook::new();
        let worksheet = book.add_worksheet();
        let border = Format::new().set_border(FormatBorder::Thin);
        worksheet
            .write_string_with_format(0, 0, "Bordered", &border)
            .expect("fixture writes");

        let report = super::import_xlsx_with_warnings(&book.save_to_buffer().unwrap(), "Read")
            .expect("the value still imports");
        let cell = report.workbook.sheets[0]
            .cells
            .iter()
            .find(|cell| cell.address == "A1")
            .expect("A1 survives");
        assert_eq!(cell.user_value, "Bordered");
        assert_eq!(cell.format, crate::CellFormat::default());
        assert!(report.warnings.iter().any(|warning| {
            warning.code == "xlsx-cell-borders-unimported"
                && warning.message.contains("1 cell")
                && warning.message.contains("authored borders")
        }));
    }

    #[test]
    fn xlsx_round_trips_a_supported_horizontal_alignment_without_style_loss() {
        let mut workbook = SpreadsheetWorkbook::sample();
        workbook
            .set_cell_format("sheet-1", "A1", "horizontal_align", "right".to_string())
            .unwrap()
            .unwrap();

        let report = super::import_xlsx_with_warnings(
            &super::export_xlsx(&workbook).expect("XLSX exports"),
            "Reopened",
        )
        .expect("XLSX imports");
        let cell = report.workbook.sheets[0]
            .cells
            .iter()
            .find(|cell| cell.address == "A1")
            .expect("A1 survives");
        assert_eq!(cell.format.horizontal_align.as_deref(), Some("right"));
        assert!(report
            .warnings
            .iter()
            .all(|warning| warning.code != "xlsx-cell-styles-unimported"));
    }

    #[test]
    fn xlsx_round_trips_a_supported_vertical_alignment_without_style_loss() {
        let mut workbook = SpreadsheetWorkbook::sample();
        workbook
            .set_cell_format("sheet-1", "A1", "vertical_align", "top".to_string())
            .unwrap()
            .unwrap();

        let report = super::import_xlsx_with_warnings(
            &super::export_xlsx(&workbook).expect("XLSX exports"),
            "Reopened",
        )
        .expect("XLSX imports");
        let cell = report.workbook.sheets[0]
            .cells
            .iter()
            .find(|cell| cell.address == "A1")
            .expect("A1 survives");
        assert_eq!(cell.format.vertical_align.as_deref(), Some("top"));
        assert!(report
            .warnings
            .iter()
            .all(|warning| warning.code != "xlsx-cell-styles-unimported"));
    }

    #[test]
    fn xlsx_import_retains_axis_dimensions_and_hidden_axes_for_pdf_pagination() {
        let mut workbook = SpreadsheetWorkbook::sample();
        workbook
            .set_column_width("sheet-1", "B", 140)
            .unwrap()
            .unwrap();
        workbook
            .set_row_height("sheet-1", "2", 36)
            .unwrap()
            .unwrap();
        workbook.set_column_hidden("sheet-1", "C", true).unwrap();
        workbook.set_row_hidden("sheet-1", "3", true).unwrap();

        let exported = workbook.to_xlsx_base64().unwrap();
        let imported = SpreadsheetWorkbook::from_xlsx_base64(&exported, "Reread").unwrap();
        let sheet = &imported.sheets[0];

        assert_eq!(sheet.column_widths.get("B"), Some(&140));
        assert_eq!(sheet.row_heights.get("2"), Some(&36));
        assert!(sheet.hidden_columns.contains(&"C".to_string()));
        assert!(sheet.hidden_rows.contains(&"3".to_string()));
    }

    /// Floating spreadsheet drawings use package drawing parts instead of
    /// cells. The value reader intentionally has no place to put them, but a
    /// successful grid import must say that fact rather than looking complete.
    #[test]
    fn xlsx_floating_drawing_is_disclosed_once_without_affecting_cells() {
        use std::io::{Cursor, Read, Write};

        let mut source = SpreadsheetWorkbook::sample();
        source
            .set_cell_in_sheet("sheet-1", "D4", "retained".to_string())
            .unwrap();
        let bytes = super::export_xlsx(&source).expect("source workbook exports");
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).expect("source is zip");
        let names: Vec<String> = archive.file_names().map(ToString::to_string).collect();
        assert!(
            !names
                .iter()
                .any(|name| name == "xl/worksheets/_rels/sheet1.xml.rels"),
            "fixture writer unexpectedly owns drawing relationships"
        );
        let mut rewritten = Vec::new();
        {
            let mut writer = zip::ZipWriter::new(Cursor::new(&mut rewritten));
            for name in names {
                let mut entry = archive.by_name(&name).expect("listed entry opens");
                let mut body = Vec::new();
                entry.read_to_end(&mut body).expect("entry inflates");
                writer
                    .start_file(name, zip::write::SimpleFileOptions::default())
                    .expect("entry starts");
                writer.write_all(&body).expect("entry writes");
            }
            writer
                .start_file(
                    "xl/worksheets/_rels/sheet1.xml.rels",
                    zip::write::SimpleFileOptions::default(),
                )
                .expect("relationship part starts");
            writer
                .write_all(
                    br#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing" Target="../drawings/drawing1.xml"/></Relationships>"#,
                )
                .expect("relationship part writes");
            writer
                .start_file(
                    "xl/drawings/drawing1.xml",
                    zip::write::SimpleFileOptions::default(),
                )
                .expect("drawing part starts");
            writer
                .write_all(b"<xdr:wsDr xmlns:xdr=\"http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing\"><xdr:twoCellAnchor/></xdr:wsDr>")
                .expect("drawing part writes");
            writer.finish().expect("fixture closes");
        }

        let report = super::import_xlsx_with_warnings(&rewritten, "Imported")
            .expect("grid imports despite the unsupported drawing");
        assert_eq!(user_value(&report.workbook, 0, "D4"), "retained");
        assert_eq!(report.warnings.len(), 1, "{:?}", report.warnings);
        assert_eq!(
            report.warnings[0].code,
            "xlsx-floating-drawing-unrepresentable"
        );
        assert!(report.warnings[0].message.contains("Sheet1"));
        assert!(report.warnings[0]
            .message
            .contains("1 floating drawing part"));
    }

    #[test]
    fn invalid_xlsx_bytes_are_rejected() {
        assert!(SpreadsheetWorkbook::from_xlsx_base64("bm90IGEgemlw", "Bad").is_err());
        assert!(SpreadsheetWorkbook::from_xlsx_base64("not base64!!", "Bad").is_err());
    }

    /// A package whose sheets name a shared string it does not contain is
    /// refused, not aborted.
    ///
    /// `calamine` indexes an empty shared-string table directly, so this was
    /// a **panic** — on wasm32 an unrecoverable module trap, and in the
    /// native shell an unwind out of a Tauri command. A fuzz target found it
    /// with one flipped byte in a zip entry name.
    #[test]
    fn a_workbook_naming_a_missing_shared_string_table_is_refused() {
        use base64::Engine as _;
        use std::io::Cursor;

        let seed = SpreadsheetWorkbook::sample()
            .to_xlsx_base64()
            .expect("the sample workbook exports");
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(seed)
            .expect("valid base64");

        // The same package with `xl/sharedStrings.xml` renamed, which is what
        // one flipped byte in a zip entry name produces.
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes.as_slice()))
            .expect("the export is a zip archive");
        let names: Vec<String> = archive.file_names().map(ToString::to_string).collect();
        assert!(
            names.iter().any(|name| name == "xl/sharedStrings.xml"),
            "the fixture must have a shared-string table to remove: {names:?}"
        );
        let mut out = Vec::new();
        {
            use std::io::Read;
            let mut writer = zip::ZipWriter::new(Cursor::new(&mut out));
            for name in &names {
                let mut part = archive.by_name(name).expect("the entry is listed");
                let mut body = Vec::new();
                part.read_to_end(&mut body).expect("the entry inflates");
                let written = if name == "xl/sharedStrings.xml" {
                    "xl/sharedStr)ngs.xml"
                } else {
                    name.as_str()
                };
                writer
                    .start_file(written, zip::write::SimpleFileOptions::default())
                    .expect("the entry starts");
                std::io::Write::write_all(&mut writer, &body).expect("the entry writes");
            }
            writer.finish().expect("the archive closes");
        }

        let err = super::import_xlsx(&out, "crafted").expect_err("a dangling part must be refused");
        let message = format!("{err:?}");
        assert!(
            message.contains("sharedStrings"),
            "the refusal must name what is missing: {message}"
        );
    }
}
