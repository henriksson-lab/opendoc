//! Spreadsheet model validation and constructors shared by import, edit,
//! recalculation, and export paths.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::SpreadsheetError;

use super::address::{
    column_to_number, normalize_cell_address, normalize_cell_range, normalize_column_label,
    normalize_merge_range, normalize_named_range_name, normalize_row_label, parse_cell_range,
    range_contains_column_number, split_cell_address, CellRange,
};
use super::google::{
    validate_google_sheets_export_axis_labels, validate_google_sheets_export_cells,
    validate_google_sheets_export_ranges,
};

pub fn normalize_sheet_id(value: &str) -> Result<String, SpreadsheetError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(SpreadsheetError::Format(
            "spreadsheet sheet id is empty".to_string(),
        ));
    }
    Ok(value.to_string())
}

pub fn validate_canonical_sheet_id(value: &str) -> Result<(), SpreadsheetError> {
    let normalized = normalize_sheet_id(value)?;
    if value != normalized {
        return Err(SpreadsheetError::Format(format!(
            "spreadsheet sheet id {value} is not canonical"
        )));
    }
    Ok(())
}

pub fn validate_canonical_cell_address(label: &str, value: &str) -> Result<(), SpreadsheetError> {
    let normalized = normalize_cell_address(value)?;
    if value != normalized {
        return Err(SpreadsheetError::Format(format!(
            "{label} {value} is not canonical; expected {normalized}"
        )));
    }
    Ok(())
}

pub fn validate_canonical_cell_range(label: &str, value: &str) -> Result<(), SpreadsheetError> {
    let normalized = normalize_cell_range(value)?;
    if value != normalized {
        return Err(SpreadsheetError::Format(format!(
            "{label} {value} is not canonical; expected {normalized}"
        )));
    }
    Ok(())
}

pub fn validate_filter_condition(value: &str) -> Result<(), SpreadsheetError> {
    match value {
        "text_contains" | "text_equals" | "number_greater" | "number_less" | "number_equal" => {
            Ok(())
        }
        other => Err(SpreadsheetError::Format(format!(
            "unsupported filter condition {other}"
        ))),
    }
}

pub fn classify_cell_value(value: &str) -> &'static str {
    if value.starts_with('=') {
        "formula"
    } else if !value.is_empty()
        && value.trim() == value
        && value.parse::<f64>().is_ok_and(f64::is_finite)
    {
        "number"
    } else if matches!(value, "TRUE" | "FALSE" | "true" | "false") {
        "bool"
    } else if value.is_empty() {
        "empty"
    } else {
        "string"
    }
}

pub fn normalize_protected_range_description(value: String) -> String {
    value.trim().to_string()
}

pub fn validate_protected_range_description(value: &str) -> Result<(), SpreadsheetError> {
    if value.trim().is_empty() {
        return Err(SpreadsheetError::Format(
            "protected range description is empty".to_string(),
        ));
    }
    if value.trim() != value {
        return Err(SpreadsheetError::Format(
            "protected range description has surrounding whitespace".to_string(),
        ));
    }
    Ok(())
}

pub fn validate_spreadsheet_cell_source(cell: &Cell) -> Result<(), SpreadsheetError> {
    match cell.user_kind.as_str() {
        "formula" => {
            if !cell.user_value.starts_with('=') {
                return Err(SpreadsheetError::Format(format!(
                    "formula cell {} source does not start with =",
                    cell.address
                )));
            }
        }
        "number" => {
            let number = cell.user_value.parse::<f64>().map_err(|_| {
                SpreadsheetError::Format(format!("number cell {} source is invalid", cell.address))
            })?;
            if !number.is_finite() {
                return Err(SpreadsheetError::Format(format!(
                    "number cell {} source is not finite",
                    cell.address
                )));
            }
        }
        "bool" => match cell.user_value.as_str() {
            "true" | "false" => {}
            _ => {
                return Err(SpreadsheetError::Format(format!(
                    "bool cell {} source is invalid",
                    cell.address
                )));
            }
        },
        "empty" | "string" => {}
        other => {
            return Err(SpreadsheetError::Format(format!(
                "cell {} has unsupported source kind {}",
                cell.address, other
            )));
        }
    }
    validate_spreadsheet_cell_projection(cell)?;
    cell.format.validate_source()?;
    if let Some(validation) = &cell.validation {
        validation.validate_source()?;
    }
    validate_dependency_labels("cell dependency", &cell.dependencies)?;
    let mut comment_ids = BTreeSet::new();
    for comment in &cell.comments {
        comment.validate_source()?;
        if !comment_ids.insert(comment.id.clone()) {
            return Err(SpreadsheetError::Format(format!(
                "duplicate cell comment {}",
                comment.id
            )));
        }
    }
    Ok(())
}

pub fn validate_spreadsheet_cell_projection(cell: &Cell) -> Result<(), SpreadsheetError> {
    match cell.computed_kind.as_str() {
        "" => {
            if !cell.computed_value.is_empty() {
                return Err(SpreadsheetError::Format(format!(
                    "cell {} cleared projection has computed value",
                    cell.address
                )));
            }
        }
        "formula" => {
            return Err(SpreadsheetError::Format(format!(
                "cell {} computed kind formula is not a projection kind",
                cell.address
            )));
        }
        "number" => {
            let number = cell.computed_value.parse::<f64>().map_err(|_| {
                SpreadsheetError::Format(format!(
                    "number cell {} computed value is invalid",
                    cell.address
                ))
            })?;
            if !number.is_finite() {
                return Err(SpreadsheetError::Format(format!(
                    "number cell {} computed value is not finite",
                    cell.address
                )));
            }
        }
        "bool" => match cell.computed_value.as_str() {
            "true" | "false" => {}
            _ => {
                return Err(SpreadsheetError::Format(format!(
                    "bool cell {} computed value is invalid",
                    cell.address
                )));
            }
        },
        "empty" => {
            if !cell.computed_value.is_empty() {
                return Err(SpreadsheetError::Format(format!(
                    "empty cell {} computed value is not empty",
                    cell.address
                )));
            }
        }
        "string" | "error" => {}
        other => {
            return Err(SpreadsheetError::Format(format!(
                "cell {} has unsupported computed kind {}",
                cell.address, other
            )));
        }
    }
    Ok(())
}

pub fn validate_dependency_labels(label: &str, values: &[String]) -> Result<(), SpreadsheetError> {
    let mut seen = BTreeSet::new();
    for value in values {
        validate_dependency_label(label, value)?;
        if !seen.insert(value.clone()) {
            return Err(SpreadsheetError::Format(format!(
                "duplicate {label} graph label {value}"
            )));
        }
    }
    Ok(())
}

pub fn validate_dependency_label(label: &str, value: &str) -> Result<(), SpreadsheetError> {
    if value.trim().is_empty() {
        return Err(SpreadsheetError::Format(format!(
            "{label} graph label is empty"
        )));
    }
    if value.trim() != value {
        return Err(SpreadsheetError::Format(format!(
            "{label} graph label {value} has surrounding whitespace"
        )));
    }
    if let Some((prefix, address)) = value.split_once('!') {
        if prefix.trim().is_empty() || prefix.trim() != prefix {
            return Err(SpreadsheetError::Format(format!(
                "{label} graph label {value} has invalid sheet prefix"
            )));
        }
        let normalized = normalize_cell_address(address)?;
        if address != normalized {
            return Err(SpreadsheetError::Format(format!(
                "{label} graph label {value} address is not canonical"
            )));
        }
    } else {
        let normalized = normalize_cell_address(value)?;
        if value != normalized {
            return Err(SpreadsheetError::Format(format!(
                "{label} graph label {value} is not canonical"
            )));
        }
    }
    Ok(())
}

pub fn graph_dependency_target(
    sheets: &[Sheet],
    current_sheet_id: &str,
    dependency: &str,
) -> (String, String) {
    if let Some((prefix, address)) = dependency.split_once('!') {
        if let Some(sheet) = sheets
            .iter()
            .find(|sheet| sheet.id == prefix || sheet.title == prefix)
        {
            return (sheet.id.clone(), address.to_string());
        }
    }
    (current_sheet_id.to_string(), dependency.to_string())
}

pub fn graph_dependent_label(
    sheets: &[Sheet],
    dependency_sheet_id: &str,
    dependent_sheet_id: &str,
    address: &str,
) -> String {
    if dependency_sheet_id == dependent_sheet_id {
        return address.to_string();
    }
    sheets
        .iter()
        .find(|sheet| sheet.id == dependent_sheet_id)
        .map(|sheet| {
            if sheet.title.trim().is_empty() {
                format!("{dependent_sheet_id}!{address}")
            } else {
                format!("{}!{address}", sheet.title)
            }
        })
        .unwrap_or_else(|| format!("{dependent_sheet_id}!{address}"))
}

pub fn row_axis(label: String) -> SheetAxis {
    SheetAxis {
        id: format!("row-{label}"),
        label,
    }
}

pub fn column_axis(label: String) -> SheetAxis {
    SheetAxis {
        id: format!("col-{label}"),
        label,
    }
}

pub fn validate_sheet_axis_metadata(sheet: &Sheet) -> Result<(), SpreadsheetError> {
    let row_labels = sheet.rows.iter().cloned().collect::<BTreeSet<_>>();
    let column_labels = sheet.columns.iter().cloned().collect::<BTreeSet<_>>();
    let mut row_axis_ids = BTreeSet::new();
    let mut row_axis_labels = BTreeSet::new();
    for axis in &sheet.row_axes {
        if axis.id.trim().is_empty() {
            return Err(SpreadsheetError::Format("row axis id is empty".to_string()));
        }
        let label = normalize_row_label(&axis.label)?;
        if label != axis.label {
            return Err(SpreadsheetError::Format(format!(
                "row axis label {} is not canonical",
                axis.label
            )));
        }
        let expected_id = format!("row-{label}");
        if axis.id != expected_id {
            return Err(SpreadsheetError::Format(format!(
                "row axis id {} does not match canonical id {}",
                axis.id, expected_id
            )));
        }
        if !row_labels.contains(&axis.label) {
            return Err(SpreadsheetError::Format(format!(
                "row axis label {} is outside visible sheet grid",
                axis.label
            )));
        }
        if !row_axis_ids.insert(axis.id.clone()) {
            return Err(SpreadsheetError::Format(format!(
                "duplicate row axis id {}",
                axis.id
            )));
        }
        if !row_axis_labels.insert(axis.label.clone()) {
            return Err(SpreadsheetError::Format(format!(
                "duplicate row axis label {}",
                axis.label
            )));
        }
    }
    let mut column_axis_ids = BTreeSet::new();
    let mut column_axis_labels = BTreeSet::new();
    for axis in &sheet.column_axes {
        if axis.id.trim().is_empty() {
            return Err(SpreadsheetError::Format(
                "column axis id is empty".to_string(),
            ));
        }
        let label = normalize_column_label(&axis.label)?;
        if label != axis.label {
            return Err(SpreadsheetError::Format(format!(
                "column axis label {} is not canonical",
                axis.label
            )));
        }
        let expected_id = format!("col-{label}");
        if axis.id != expected_id {
            return Err(SpreadsheetError::Format(format!(
                "column axis id {} does not match canonical id {}",
                axis.id, expected_id
            )));
        }
        if !column_labels.contains(&axis.label) {
            return Err(SpreadsheetError::Format(format!(
                "column axis label {} is outside visible sheet grid",
                axis.label
            )));
        }
        if !column_axis_ids.insert(axis.id.clone()) {
            return Err(SpreadsheetError::Format(format!(
                "duplicate column axis id {}",
                axis.id
            )));
        }
        if !column_axis_labels.insert(axis.label.clone()) {
            return Err(SpreadsheetError::Format(format!(
                "duplicate column axis label {}",
                axis.label
            )));
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SheetAxis {
    pub id: String,
    pub label: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Sheet {
    pub id: String,
    pub title: String,
    pub frozen_rows: u32,
    pub frozen_columns: u32,
    pub merges: Vec<SheetMerge>,
    pub filters: Vec<SheetFilter>,
    pub protected_ranges: Vec<SheetProtectedRange>,
    pub row_axes: Vec<SheetAxis>,
    pub column_axes: Vec<SheetAxis>,
    pub rows: Vec<String>,
    pub columns: Vec<String>,
    pub cells: Vec<Cell>,
    /// Explicit row heights in pixels by row label.
    #[serde(default)]
    pub row_heights: BTreeMap<String, u32>,
    /// Explicit column widths in pixels by column label.
    #[serde(default)]
    pub column_widths: BTreeMap<String, u32>,
    #[serde(default)]
    pub hidden_rows: Vec<String>,
    #[serde(default)]
    pub hidden_columns: Vec<String>,
    /// Hidden sheet tab.
    #[serde(default)]
    pub hidden: bool,
    /// Tab colour as `#rrggbb`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_color: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SheetMerge {
    pub id: String,
    pub range: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SheetFilter {
    pub id: String,
    pub range: String,
    #[serde(default)]
    pub criteria: Vec<SheetFilterCriterion>,
    #[serde(default)]
    pub sort_specs: Vec<SheetFilterSortSpec>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SheetFilterCriterion {
    pub column: String,
    pub condition: String,
    pub value: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SheetFilterSortSpec {
    pub column: String,
    pub descending: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SheetProtectedRange {
    pub id: String,
    pub range: String,
    pub description: String,
    pub warning_only: bool,
}

impl SheetMerge {
    pub fn validate_source(&self) -> Result<(), SpreadsheetError> {
        if self.id.trim().is_empty() {
            return Err(SpreadsheetError::Format("merge id is empty".to_string()));
        }
        if self.id.trim() != self.id {
            return Err(SpreadsheetError::Format(
                "merge id has surrounding whitespace".to_string(),
            ));
        }
        let normalized = normalize_merge_range(&self.range)?;
        if self.range != normalized {
            return Err(SpreadsheetError::Format(format!(
                "merge {} uses non-canonical range {} instead of {}",
                self.id, self.range, normalized
            )));
        }
        Ok(())
    }
}

impl SheetFilter {
    pub fn validate_source(&self) -> Result<(), SpreadsheetError> {
        if self.id.trim().is_empty() {
            return Err(SpreadsheetError::Format("filter id is empty".to_string()));
        }
        if self.id.trim() != self.id {
            return Err(SpreadsheetError::Format(
                "filter id has surrounding whitespace".to_string(),
            ));
        }
        let range = normalize_cell_range(&self.range)?;
        if self.range != range {
            return Err(SpreadsheetError::Format(format!(
                "filter {} uses non-canonical range {} instead of {}",
                self.id, self.range, range
            )));
        }
        let parsed = parse_cell_range(&range)?;
        let mut criterion_columns = BTreeSet::new();
        for criterion in &self.criteria {
            criterion.validate_source(&parsed)?;
            let column = normalize_column_label(&criterion.column)?;
            if !criterion_columns.insert(column.clone()) {
                return Err(SpreadsheetError::Format(format!(
                    "duplicate filter criterion column {column}"
                )));
            }
        }
        let mut sort_columns = BTreeSet::new();
        for sort_spec in &self.sort_specs {
            sort_spec.validate_source(&parsed)?;
            let column = normalize_column_label(&sort_spec.column)?;
            if !sort_columns.insert(column.clone()) {
                return Err(SpreadsheetError::Format(format!(
                    "duplicate filter sort column {column}"
                )));
            }
        }
        Ok(())
    }
}

impl SheetFilterCriterion {
    pub fn validate_source(&self, range: &CellRange) -> Result<(), SpreadsheetError> {
        let column = normalize_column_label(&self.column)?;
        if self.column != column {
            return Err(SpreadsheetError::Format(format!(
                "filter criterion column {} is not canonical",
                self.column
            )));
        }
        if !range_contains_column_number(range, column_to_number(&column).unwrap_or(0)) {
            return Err(SpreadsheetError::Format(format!(
                "filter criterion column {column} is outside range"
            )));
        }
        if self.condition.trim() != self.condition {
            return Err(SpreadsheetError::Format(
                "filter criterion condition has surrounding whitespace".to_string(),
            ));
        }
        validate_filter_condition(&self.condition)?;
        if self.value.trim().is_empty() {
            return Err(SpreadsheetError::Format(
                "filter criterion value is empty".to_string(),
            ));
        }
        Ok(())
    }
}

impl SheetFilterSortSpec {
    pub fn validate_source(&self, range: &CellRange) -> Result<(), SpreadsheetError> {
        let column = normalize_column_label(&self.column)?;
        if self.column != column {
            return Err(SpreadsheetError::Format(format!(
                "filter sort column {} is not canonical",
                self.column
            )));
        }
        if !range_contains_column_number(range, column_to_number(&column).unwrap_or(0)) {
            return Err(SpreadsheetError::Format(format!(
                "filter sort column {column} is outside range"
            )));
        }
        Ok(())
    }
}

impl SheetProtectedRange {
    pub fn validate_source(&self) -> Result<(), SpreadsheetError> {
        if self.id.trim().is_empty() {
            return Err(SpreadsheetError::Format(
                "protected range id is empty".to_string(),
            ));
        }
        if self.id.trim() != self.id {
            return Err(SpreadsheetError::Format(
                "protected range id has surrounding whitespace".to_string(),
            ));
        }
        let normalized = normalize_cell_range(&self.range)?;
        if self.range != normalized {
            return Err(SpreadsheetError::Format(format!(
                "protected range {} uses non-canonical range {} instead of {}",
                self.id, self.range, normalized
            )));
        }
        validate_protected_range_description(&self.description)?;
        if !self.warning_only {
            return Err(SpreadsheetError::Format(format!(
                "protected range {} is not warning-only",
                self.id
            )));
        }
        Ok(())
    }
}

impl Sheet {
    pub fn validate_source(&self) -> Result<(), SpreadsheetError> {
        if self.id.trim().is_empty() {
            return Err(SpreadsheetError::Format(
                "spreadsheet sheet id is empty".to_string(),
            ));
        }
        if self.title.trim().is_empty() {
            return Err(SpreadsheetError::Format(format!(
                "spreadsheet sheet {} title is empty",
                self.id
            )));
        }
        if self.rows.is_empty() {
            return Err(SpreadsheetError::Format(format!(
                "sheet {} has no visible rows",
                self.id
            )));
        }
        if self.columns.is_empty() {
            return Err(SpreadsheetError::Format(format!(
                "sheet {} has no visible columns",
                self.id
            )));
        }
        if self.frozen_rows as usize > self.rows.len() {
            return Err(SpreadsheetError::Format(format!(
                "sheet {} frozen rows exceed visible rows",
                self.id
            )));
        }
        if self.frozen_columns as usize > self.columns.len() {
            return Err(SpreadsheetError::Format(format!(
                "sheet {} frozen columns exceed visible columns",
                self.id
            )));
        }
        validate_google_sheets_export_axis_labels(self)?;
        validate_sheet_axis_metadata(self)?;
        validate_google_sheets_export_ranges(self)?;
        validate_google_sheets_export_cells(self)
    }

    pub fn ensure_address(&mut self, address: &str) {
        let (column, row) = split_cell_address(address);
        if !self.columns.iter().any(|item| item == &column) {
            self.columns.push(column);
            self.columns.sort();
        }
        if !self.rows.iter().any(|item| item == &row) {
            self.rows.push(row);
            self.rows
                .sort_by_key(|value| value.parse::<u32>().unwrap_or(0));
        }
        self.ensure_axis_metadata();
    }

    pub fn ensure_axis_metadata(&mut self) {
        let rows = self.rows.clone();
        let columns = self.columns.clone();
        for row in rows {
            if !self.row_axes.iter().any(|axis| axis.label == row) {
                self.row_axes.push(row_axis(row));
            }
        }
        for column in columns {
            if !self.column_axes.iter().any(|axis| axis.label == column) {
                self.column_axes.push(column_axis(column));
            }
        }
        self.row_axes
            .sort_by_key(|axis| axis.label.parse::<u32>().unwrap_or(0));
        self.column_axes
            .sort_by_key(|axis| column_to_number(&axis.label).unwrap_or(0));
    }

    pub fn cell_mut_or_insert(&mut self, address: &str) -> &mut Cell {
        if let Some(index) = self.cells.iter().position(|cell| cell.address == address) {
            return &mut self.cells[index];
        }
        self.cells.push(Cell::new(address, "empty", ""));
        self.cells
            .sort_by(|left, right| left.address.cmp(&right.address));
        self.cells
            .iter_mut()
            .find(|cell| cell.address == address)
            .expect("newly inserted spreadsheet cell exists")
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Cell {
    pub address: String,
    pub user_kind: String,
    pub user_value: String,
    pub format: CellFormat,
    pub validation: Option<CellValidation>,
    pub computed_kind: String,
    pub computed_value: String,
    /// Formatted text for display, derived from `computed_value` and
    /// `format.number_format`.
    #[serde(default)]
    pub display_value: String,
    pub dependencies: Vec<String>,
    pub comments: Vec<CellComment>,
    /// Address of the array formula that spilled into this cell, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spill_source: Option<String>,
}

impl Cell {
    pub fn new(address: &str, user_kind: &str, user_value: &str) -> Self {
        Self {
            address: address.to_string(),
            user_kind: user_kind.to_string(),
            user_value: user_value.to_string(),
            format: CellFormat::default(),
            validation: None,
            computed_kind: user_kind.to_string(),
            computed_value: user_value.to_string(),
            display_value: String::new(),
            dependencies: Vec::new(),
            comments: Vec::new(),
            spill_source: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CellValidation {
    pub kind: String,
    pub values: Vec<String>,
    pub strict: bool,
    pub show_dropdown: bool,
}

impl CellValidation {
    pub fn new(kind: &str, values: Vec<String>, strict: bool) -> Result<Self, SpreadsheetError> {
        let kind = match kind.trim().to_ascii_lowercase().as_str() {
            "list" | "one_of_list" | "one_of_range" => "list".to_string(),
            "number_greater" | "number_less" | "number_between" | "text_contains"
            | "custom_formula" => kind.trim().to_ascii_lowercase(),
            "" => {
                return Err(SpreadsheetError::Model(
                    "spreadsheet validation kind cannot be empty".to_string(),
                ))
            }
            other => other.to_string(),
        };
        let values = values
            .into_iter()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .take(64)
            .collect::<Vec<_>>();
        if kind == "list" && values.is_empty() {
            return Err(SpreadsheetError::Model(
                "list validation requires at least one value".to_string(),
            ));
        }
        Ok(Self {
            kind,
            values,
            strict,
            show_dropdown: true,
        })
    }

    pub fn validate_source(&self) -> Result<(), SpreadsheetError> {
        let kind = self.kind.trim();
        if kind.is_empty() {
            return Err(SpreadsheetError::Format(
                "spreadsheet validation kind is empty".to_string(),
            ));
        }
        let normalized = kind.to_ascii_lowercase();
        if self.kind != normalized {
            return Err(SpreadsheetError::Format(format!(
                "spreadsheet validation kind {} is not canonical",
                self.kind
            )));
        }
        if !matches!(
            normalized.as_str(),
            "list"
                | "one_of_list"
                | "one_of_range"
                | "number_greater"
                | "number_less"
                | "number_between"
                | "text_contains"
                | "custom_formula"
        ) {
            return Err(SpreadsheetError::Format(format!(
                "unsupported spreadsheet validation kind {kind}"
            )));
        }
        if self.values.len() > 64 {
            return Err(SpreadsheetError::Format(
                "spreadsheet validation values exceed supported limit 64".to_string(),
            ));
        }
        for (index, value) in self.values.iter().enumerate() {
            if value.trim().is_empty() {
                return Err(SpreadsheetError::Format(format!(
                    "spreadsheet validation value {index} is empty"
                )));
            }
            if value.trim() != value {
                return Err(SpreadsheetError::Format(format!(
                    "spreadsheet validation value {index} has surrounding whitespace"
                )));
            }
        }
        if normalized == "list" && self.values.is_empty() {
            return Err(SpreadsheetError::Format(
                "list validation requires at least one value".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CellComment {
    pub id: String,
    pub author: String,
    pub body: String,
    pub deleted: bool,
}

impl CellComment {
    pub fn validate_source(&self) -> Result<(), SpreadsheetError> {
        if self.id.trim().is_empty() {
            return Err(SpreadsheetError::Format(
                "cell comment id is empty".to_string(),
            ));
        }
        if self.id.trim() != self.id {
            return Err(SpreadsheetError::Format(
                "cell comment id has surrounding whitespace".to_string(),
            ));
        }
        if self.author.trim().is_empty() {
            return Err(SpreadsheetError::Format(
                "cell comment author is empty".to_string(),
            ));
        }
        if self.author.trim() != self.author {
            return Err(SpreadsheetError::Format(
                "cell comment author has surrounding whitespace".to_string(),
            ));
        }
        if self.body.trim().is_empty() {
            return Err(SpreadsheetError::Format(
                "cell comment body is empty".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DeletedCellComment {
    pub sheet_id: String,
    pub address: String,
    pub comment: CellComment,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct CellFormat {
    pub bold: bool,
    pub italic: bool,
    pub text_color: Option<String>,
    pub background_color: Option<String>,
    pub horizontal_align: Option<String>,
    pub number_format: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CellDependency {
    pub sheet_id: String,
    pub address: String,
    pub dependencies: Vec<String>,
    pub dependents: Vec<String>,
    pub invalidation_order: Vec<String>,
}

impl CellDependency {
    pub fn validate_source(&self, sheet_ids: &BTreeSet<String>) -> Result<(), SpreadsheetError> {
        if self.sheet_id.trim().is_empty() {
            return Err(SpreadsheetError::Format(
                "dependency graph sheet_id is empty".to_string(),
            ));
        }
        if !sheet_ids.contains(&self.sheet_id) {
            return Err(SpreadsheetError::Format(format!(
                "dependency graph references missing sheet {}",
                self.sheet_id
            )));
        }
        validate_dependency_label("dependency graph address", &self.address)?;
        validate_dependency_labels("dependency", &self.dependencies)?;
        validate_dependency_labels("dependent", &self.dependents)?;
        validate_dependency_labels("invalidation", &self.invalidation_order)?;
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NamedRange {
    pub id: String,
    pub name: String,
    pub sheet_id: String,
    pub range: String,
}

impl NamedRange {
    pub fn validate_source(&self) -> Result<(), SpreadsheetError> {
        if self.id.trim().is_empty() {
            return Err(SpreadsheetError::Format(
                "named range id is empty".to_string(),
            ));
        }
        if self.id.trim() != self.id {
            return Err(SpreadsheetError::Format(
                "named range id has surrounding whitespace".to_string(),
            ));
        }
        let normalized_name = normalize_named_range_name(&self.name)?;
        if self.name != normalized_name {
            return Err(SpreadsheetError::Format(format!(
                "named range name {} is not canonical",
                self.name
            )));
        }
        if self.sheet_id.trim().is_empty() {
            return Err(SpreadsheetError::Format(
                "named range sheet_id is empty".to_string(),
            ));
        }
        let normalized = normalize_cell_range(&self.range)?;
        if self.range != normalized {
            return Err(SpreadsheetError::Format(format!(
                "named range {} uses non-canonical range {} instead of {}",
                self.name, self.range, normalized
            )));
        }
        Ok(())
    }
}
