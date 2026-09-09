//! Spreadsheet workbook state and high-level workbook mutations.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{DeletedColumnPayload, DeletedRowPayload, SpreadsheetError, SpreadsheetWarning};

use super::address::{
    cell_address, column_to_number, normalize_cell_address, normalize_cell_range,
    normalize_named_range_name, normalize_sheet_title, parse_cell_range, range_contains_column,
    range_contains_row, rewrite_formula_sheet_title_references, split_cell_address,
};
use super::model::{
    column_axis, row_axis, Cell, CellComment, CellDependency, CellValidation, DeletedCellComment,
    NamedRange, Sheet, SheetFilter, SheetFilterCriterion, SheetFilterSortSpec, SheetMerge,
    SheetProtectedRange,
};
use super::recalc::SpreadsheetEvaluationContext;
use super::structure::{
    add_sheet_protected_range, copy_sheet_range, merge_sheet_cells, set_sheet_basic_filter,
    set_sheet_basic_filter_options, set_sheet_cell, set_sheet_cell_format, upsert_sheet_cell,
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SpreadsheetWorkbook {
    pub title: String,
    pub locale: String,
    pub timezone: String,
    pub named_ranges: Vec<NamedRange>,
    pub dependency_graph: Vec<CellDependency>,
    /// Sheets in tab order.
    pub sheets: Vec<Sheet>,
    /// Fixed clock/seed for volatile functions (`NOW`, `TODAY`, `RAND`,
    /// `RANDBETWEEN`); `None` uses the real clock at evaluation time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evaluation_context: Option<SpreadsheetEvaluationContext>,
    /// Warnings from the last evaluation (circular references).
    #[serde(default)]
    pub evaluation_warnings: Vec<SpreadsheetWarning>,
    #[serde(skip)]
    pub(crate) recalc: super::recalc::RecalcState,
}

impl SpreadsheetWorkbook {
    /// An empty workbook with no sheets (importers add their own).
    pub fn empty(title: &str) -> Self {
        Self {
            title: normalize_sheet_title(title),
            locale: "en-US".to_string(),
            timezone: "UTC".to_string(),
            named_ranges: Vec::new(),
            dependency_graph: Vec::new(),
            sheets: Vec::new(),
            evaluation_context: None,
            evaluation_warnings: Vec::new(),
            recalc: Default::default(),
        }
    }

    pub fn sample() -> Self {
        let mut sheet = super::io::blank_sheet(
            "sheet-1",
            "Sheet1",
            super::io::DEFAULT_SHEET_ROWS,
            super::io::DEFAULT_SHEET_COLUMNS,
        );
        sheet.frozen_rows = 1;
        sheet.cells = vec![
            Cell::new("A1", "string", "Item"),
            Cell::new("B1", "string", "Count"),
            Cell::new("A2", "string", "Apples"),
            Cell::new("B2", "number", "5"),
            Cell::new("A3", "string", "Total"),
            Cell::new("B3", "formula", "=SUM(B2:B2)"),
        ];
        let mut workbook = Self::empty("Prototype Sheet");
        workbook.sheets.push(sheet);
        workbook = workbook.evaluated();
        workbook
    }

    pub fn set_metadata(
        &mut self,
        title: impl AsRef<str>,
        locale: impl AsRef<str>,
        timezone: impl AsRef<str>,
    ) -> Result<(), SpreadsheetError> {
        let title = title.as_ref().trim();
        let locale = locale.as_ref().trim();
        let timezone = timezone.as_ref().trim();
        if title.is_empty() {
            return Err(SpreadsheetError::Format(
                "spreadsheet workbook title is empty".to_string(),
            ));
        }
        if locale.is_empty() {
            return Err(SpreadsheetError::Format(
                "spreadsheet workbook locale is empty".to_string(),
            ));
        }
        if timezone.is_empty() {
            return Err(SpreadsheetError::Format(
                "spreadsheet workbook timezone is empty".to_string(),
            ));
        }
        self.title = title.to_string();
        self.locale = locale.to_string();
        self.timezone = timezone.to_string();
        Ok(())
    }

    pub fn validate_source(&self) -> Result<(), SpreadsheetError> {
        if self.title.trim().is_empty() {
            return Err(SpreadsheetError::Format(
                "spreadsheet workbook title is empty".to_string(),
            ));
        }
        if self.title.trim() != self.title {
            return Err(SpreadsheetError::Format(
                "spreadsheet workbook title has surrounding whitespace".to_string(),
            ));
        }
        if self.locale.trim().is_empty() {
            return Err(SpreadsheetError::Format(
                "spreadsheet workbook locale is empty".to_string(),
            ));
        }
        if self.locale.trim() != self.locale {
            return Err(SpreadsheetError::Format(
                "spreadsheet workbook locale has surrounding whitespace".to_string(),
            ));
        }
        if self.timezone.trim().is_empty() {
            return Err(SpreadsheetError::Format(
                "spreadsheet workbook timezone is empty".to_string(),
            ));
        }
        if self.timezone.trim() != self.timezone {
            return Err(SpreadsheetError::Format(
                "spreadsheet workbook timezone has surrounding whitespace".to_string(),
            ));
        }
        if self.sheets.is_empty() {
            return Err(SpreadsheetError::Format(
                "spreadsheet workbook has no sheets".to_string(),
            ));
        }

        let mut sheet_ids = BTreeSet::new();
        let mut sheet_titles = BTreeSet::new();
        for sheet in &self.sheets {
            if sheet.id.trim().is_empty() {
                return Err(SpreadsheetError::Format(
                    "spreadsheet sheet id is empty".to_string(),
                ));
            }
            if sheet.id.trim() != sheet.id {
                return Err(SpreadsheetError::Format(
                    "spreadsheet sheet id has surrounding whitespace".to_string(),
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
            if sheet.title.trim() != sheet.title {
                return Err(SpreadsheetError::Format(format!(
                    "spreadsheet sheet {} title has surrounding whitespace",
                    sheet.id
                )));
            }
            if !sheet_titles.insert(sheet.title.clone()) {
                return Err(SpreadsheetError::Format(format!(
                    "duplicate spreadsheet sheet title {}",
                    sheet.title
                )));
            }
            sheet.validate_source()?;
        }

        let mut named_range_ids = BTreeSet::new();
        let mut named_range_names = BTreeSet::new();
        for named_range in &self.named_ranges {
            named_range.validate_source()?;
            if !sheet_ids.contains(&named_range.sheet_id) {
                return Err(SpreadsheetError::Format(format!(
                    "named range {} references missing sheet {}",
                    named_range.name, named_range.sheet_id
                )));
            }
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

        let mut dependency_keys = BTreeSet::new();
        for dependency in &self.dependency_graph {
            dependency.validate_source(&sheet_ids)?;
            if !dependency_keys.insert((dependency.sheet_id.clone(), dependency.address.clone())) {
                return Err(SpreadsheetError::Format(format!(
                    "duplicate dependency graph entry {}!{}",
                    dependency.sheet_id, dependency.address
                )));
            }
        }

        Ok(())
    }

    /// Returns a copy with every formula evaluated. Only cells whose
    /// inputs changed since the last evaluation are recomputed; the whole
    /// workbook is one dependency graph so cross-sheet chains are never
    /// stale.
    pub fn evaluated(&self) -> Self {
        let mut workbook = self.clone();
        for sheet in &mut workbook.sheets {
            sheet.ensure_axis_metadata();
        }
        super::recalc::evaluate_workbook(&mut workbook);
        workbook.dependency_graph =
            super::recalc::build_dependency_graph(&workbook.sheets, &workbook.named_ranges);
        workbook
    }

    pub fn set_cell(&mut self, address: &str, value: String) {
        let Some(sheet) = self.sheets.first_mut() else {
            return;
        };
        set_sheet_cell(sheet, address, value);
    }

    pub fn set_cell_in_sheet(
        &mut self,
        sheet_id: &str,
        address: &str,
        value: String,
    ) -> Option<()> {
        let sheet = self.sheets.iter_mut().find(|sheet| sheet.id == sheet_id)?;
        set_sheet_cell(sheet, address, value);
        Some(())
    }

    pub fn set_cell_format(
        &mut self,
        sheet_id: &str,
        address: &str,
        property: &str,
        value: String,
    ) -> Option<Result<(), SpreadsheetError>> {
        let sheet = self.sheets.iter_mut().find(|sheet| sheet.id == sheet_id)?;
        Some(set_sheet_cell_format(sheet, address, property, value))
    }

    pub fn copy_range(
        &mut self,
        sheet_id: &str,
        source_range: &str,
        target_address: &str,
    ) -> Option<Result<(), SpreadsheetError>> {
        let sheet = self.sheets.iter_mut().find(|sheet| sheet.id == sheet_id)?;
        Some(copy_sheet_range(sheet, source_range, target_address))
    }

    pub fn add_named_range(
        &mut self,
        sheet_id: &str,
        name: &str,
        range: &str,
    ) -> Option<Result<(), SpreadsheetError>> {
        let sheet = self.sheets.iter_mut().find(|sheet| sheet.id == sheet_id)?;
        let normalized = match normalize_cell_range(range) {
            Ok(normalized) => normalized,
            Err(err) => return Some(Err(err)),
        };
        let parsed = match parse_cell_range(&normalized) {
            Ok(parsed) => parsed,
            Err(err) => return Some(Err(err)),
        };
        for row_offset in 0..parsed.height {
            for column_offset in 0..parsed.width {
                let address = match cell_address(
                    parsed.start_column + column_offset,
                    parsed.start_row + row_offset,
                ) {
                    Ok(address) => address,
                    Err(err) => return Some(Err(err)),
                };
                sheet.ensure_address(&address);
            }
        }
        let id = format!("named-{}", name.to_ascii_lowercase());
        if let Some(existing) = self.named_ranges.iter_mut().find(|item| item.name == name) {
            existing.sheet_id = sheet_id.to_string();
            existing.range = normalized;
        } else {
            self.named_ranges.push(NamedRange {
                id,
                name: name.to_string(),
                sheet_id: sheet_id.to_string(),
                range: normalized,
            });
            self.named_ranges
                .sort_by(|left, right| left.name.cmp(&right.name));
        }
        Some(Ok(()))
    }

    pub fn update_named_range(
        &mut self,
        sheet_id: &str,
        name: &str,
        range: &str,
    ) -> Option<Result<(), SpreadsheetError>> {
        if !self.named_ranges.iter().any(|item| item.name == name) {
            return Some(Err(SpreadsheetError::NotFound(format!(
                "named range {name} was not found"
            ))));
        }
        self.add_named_range(sheet_id, name, range)
    }

    pub fn delete_named_range(&mut self, name: &str) -> Option<()> {
        let index = self
            .named_ranges
            .iter()
            .position(|item| item.name.eq_ignore_ascii_case(name))?;
        self.named_ranges.remove(index);
        Some(())
    }

    pub fn named_range(&self, name: &str) -> Option<&NamedRange> {
        self.named_ranges
            .iter()
            .find(|item| item.name.eq_ignore_ascii_case(name))
    }

    pub fn restore_named_range(&mut self, range: NamedRange) -> Result<(), SpreadsheetError> {
        range.validate_source()?;
        if !self.sheets.iter().any(|sheet| sheet.id == range.sheet_id) {
            return Err(SpreadsheetError::Format(format!(
                "named range {} references missing sheet {}",
                range.name, range.sheet_id
            )));
        }
        if self
            .named_ranges
            .iter()
            .any(|existing| existing.id == range.id || existing.name == range.name)
        {
            return Err(SpreadsheetError::Conflict(format!(
                "named range {} already exists",
                range.name
            )));
        }
        self.named_ranges.push(range);
        self.named_ranges
            .sort_by(|left, right| left.name.cmp(&right.name));
        self.validate_source()
    }

    pub fn add_sheet_with_id(&mut self, sheet_id: &str, title: &str) {
        if self.sheets.iter().any(|sheet| sheet.id == sheet_id) {
            return;
        }
        let title = if normalize_sheet_title(title) == "Sheet" {
            format!("Sheet{}", self.sheets.len() + 1)
        } else {
            normalize_sheet_title(title)
        };
        self.sheets.push(super::io::blank_sheet(
            sheet_id,
            &title,
            super::io::DEFAULT_SHEET_ROWS,
            super::io::DEFAULT_SHEET_COLUMNS,
        ));
    }

    pub fn rename_sheet(&mut self, sheet_id: &str, title: &str) -> Option<()> {
        let sheet = self.sheets.iter_mut().find(|sheet| sheet.id == sheet_id)?;
        sheet.title = normalize_sheet_title(title);
        Some(())
    }

    pub fn rewrite_formula_sheet_title_references(&mut self, old_title: &str, new_title: &str) {
        if old_title == new_title {
            return;
        }
        for sheet in &mut self.sheets {
            for cell in &mut sheet.cells {
                if cell.user_kind == "formula" {
                    cell.user_value = rewrite_formula_sheet_title_references(
                        &cell.user_value,
                        old_title,
                        new_title,
                    );
                }
            }
        }
    }

    pub fn delete_sheet(&mut self, sheet_id: &str) -> Option<bool> {
        let index = self.sheets.iter().position(|sheet| sheet.id == sheet_id)?;
        if self.sheets.len() <= 1 {
            return Some(false);
        }
        self.sheets.remove(index);
        self.named_ranges.retain(|range| range.sheet_id != sheet_id);
        Some(true)
    }

    pub fn sheet_restore_payload(&self, sheet_id: &str) -> Option<(Sheet, Vec<NamedRange>)> {
        let sheet = self
            .sheets
            .iter()
            .find(|sheet| sheet.id == sheet_id)?
            .clone();
        let named_ranges = self
            .named_ranges
            .iter()
            .filter(|range| range.sheet_id == sheet_id)
            .cloned()
            .collect();
        Some((sheet, named_ranges))
    }

    pub fn restore_sheet(
        &mut self,
        sheet: Sheet,
        named_ranges: Vec<NamedRange>,
    ) -> Result<(), SpreadsheetError> {
        if self.sheets.iter().any(|existing| existing.id == sheet.id) {
            return Err(SpreadsheetError::Conflict(format!(
                "sheet {} already exists",
                sheet.id
            )));
        }
        sheet.validate_source()?;
        for range in &named_ranges {
            range.validate_source()?;
            if range.sheet_id != sheet.id {
                return Err(SpreadsheetError::Format(format!(
                    "restored named range {} references sheet {} instead of {}",
                    range.name, range.sheet_id, sheet.id
                )));
            }
        }
        self.sheets.push(sheet);
        for range in named_ranges {
            self.named_ranges
                .retain(|existing| existing.id != range.id && existing.name != range.name);
            self.named_ranges.push(range);
        }
        self.named_ranges
            .sort_by(|left, right| left.name.cmp(&right.name));
        self.validate_source()
    }

    pub fn set_frozen_axes(
        &mut self,
        sheet_id: &str,
        frozen_rows: u32,
        frozen_columns: u32,
    ) -> Option<(u32, u32)> {
        let sheet = self.sheets.iter_mut().find(|sheet| sheet.id == sheet_id)?;
        let frozen_rows = frozen_rows.min(sheet.rows.len() as u32);
        let frozen_columns = frozen_columns.min(sheet.columns.len() as u32);
        sheet.frozen_rows = frozen_rows;
        sheet.frozen_columns = frozen_columns;
        Some((frozen_rows, frozen_columns))
    }

    pub fn set_cell_validation(
        &mut self,
        sheet_id: &str,
        address: &str,
        validation: CellValidation,
    ) -> Option<()> {
        let sheet = self.sheets.iter_mut().find(|sheet| sheet.id == sheet_id)?;
        sheet.ensure_address(address);
        sheet.cell_mut_or_insert(address).validation = Some(validation);
        Some(())
    }

    pub fn clear_cell_validation(&mut self, sheet_id: &str, address: &str) -> Option<()> {
        let sheet = self.sheets.iter_mut().find(|sheet| sheet.id == sheet_id)?;
        sheet.ensure_address(address);
        sheet.cell_mut_or_insert(address).validation = None;
        Some(())
    }

    pub fn cell_validation(&self, sheet_id: &str, address: &str) -> Option<&CellValidation> {
        let normalized = normalize_cell_address(address).ok()?;
        self.sheets
            .iter()
            .find(|sheet| sheet.id == sheet_id)?
            .cells
            .iter()
            .find(|cell| cell.address == normalized)?
            .validation
            .as_ref()
    }

    pub fn restore_cell_validation(
        &mut self,
        sheet_id: &str,
        address: &str,
        validation: CellValidation,
    ) -> Result<(), SpreadsheetError> {
        validation.validate_source()?;
        let address = normalize_cell_address(address)?;
        let sheet = self
            .sheets
            .iter_mut()
            .find(|sheet| sheet.id == sheet_id)
            .ok_or_else(|| SpreadsheetError::NotFound(format!("sheet {sheet_id} was not found")))?;
        sheet.ensure_address(&address);
        let cell = sheet.cell_mut_or_insert(&address);
        if cell.validation.is_some() {
            return Err(SpreadsheetError::Conflict(format!(
                "cell validation {sheet_id}!{address} already exists"
            )));
        }
        cell.validation = Some(validation);
        Ok(())
    }

    pub fn merge_cells(
        &mut self,
        sheet_id: &str,
        range: &str,
    ) -> Option<Result<(), SpreadsheetError>> {
        let sheet = self.sheets.iter_mut().find(|sheet| sheet.id == sheet_id)?;
        Some(merge_sheet_cells(sheet, range))
    }

    pub fn merge_range(&self, sheet_id: &str, range: &str) -> Option<&SheetMerge> {
        let normalized = normalize_cell_range(range).ok()?;
        self.sheets
            .iter()
            .find(|sheet| sheet.id == sheet_id)?
            .merges
            .iter()
            .find(|merge| merge.range == normalized)
    }

    pub fn unmerge_cells(&mut self, sheet_id: &str, range: &str) -> Option<()> {
        let sheet = self.sheets.iter_mut().find(|sheet| sheet.id == sheet_id)?;
        let normalized = normalize_cell_range(range).unwrap_or_else(|_| range.to_string());
        sheet.merges.retain(|merge| merge.range != normalized);
        Some(())
    }

    pub fn restore_merge(
        &mut self,
        sheet_id: &str,
        merge: SheetMerge,
    ) -> Result<(), SpreadsheetError> {
        merge.validate_source()?;
        let sheet = self
            .sheets
            .iter_mut()
            .find(|sheet| sheet.id == sheet_id)
            .ok_or_else(|| SpreadsheetError::NotFound(format!("sheet {sheet_id} was not found")))?;
        if sheet
            .merges
            .iter()
            .any(|existing| existing.id == merge.id || existing.range == merge.range)
        {
            return Err(SpreadsheetError::Conflict(format!(
                "merge {sheet_id}!{} already exists",
                merge.range
            )));
        }
        merge_sheet_cells(sheet, &merge.range)
    }

    pub fn set_basic_filter(
        &mut self,
        sheet_id: &str,
        range: &str,
    ) -> Option<Result<(), SpreadsheetError>> {
        let sheet = self.sheets.iter_mut().find(|sheet| sheet.id == sheet_id)?;
        Some(set_sheet_basic_filter(sheet, range))
    }

    pub fn set_basic_filter_options(
        &mut self,
        sheet_id: &str,
        criteria: Vec<SheetFilterCriterion>,
        sort_specs: Vec<SheetFilterSortSpec>,
    ) -> Option<Result<(), SpreadsheetError>> {
        let sheet = self.sheets.iter_mut().find(|sheet| sheet.id == sheet_id)?;
        Some(set_sheet_basic_filter_options(sheet, criteria, sort_specs))
    }

    pub fn clear_basic_filter(&mut self, sheet_id: &str) -> Option<()> {
        let sheet = self.sheets.iter_mut().find(|sheet| sheet.id == sheet_id)?;
        sheet.filters.clear();
        Some(())
    }

    pub fn basic_filter(&self, sheet_id: &str) -> Option<&SheetFilter> {
        self.sheets
            .iter()
            .find(|sheet| sheet.id == sheet_id)?
            .filters
            .first()
    }

    pub fn restore_basic_filter(
        &mut self,
        sheet_id: &str,
        filter: SheetFilter,
    ) -> Result<(), SpreadsheetError> {
        filter.validate_source()?;
        let sheet = self
            .sheets
            .iter_mut()
            .find(|sheet| sheet.id == sheet_id)
            .ok_or_else(|| SpreadsheetError::NotFound(format!("sheet {sheet_id} was not found")))?;
        if !sheet.filters.is_empty() {
            return Err(SpreadsheetError::Conflict(format!(
                "sheet {sheet_id} already has a filter"
            )));
        }
        let parsed = parse_cell_range(&filter.range)?;
        for row_offset in 0..parsed.height {
            for column_offset in 0..parsed.width {
                let address = cell_address(
                    parsed.start_column + column_offset,
                    parsed.start_row + row_offset,
                )?;
                sheet.ensure_address(&address);
            }
        }
        sheet.filters.push(filter);
        Ok(())
    }

    pub fn add_protected_range(
        &mut self,
        sheet_id: &str,
        range: &str,
        description: &str,
        warning_only: bool,
    ) -> Option<Result<(), SpreadsheetError>> {
        let sheet = self.sheets.iter_mut().find(|sheet| sheet.id == sheet_id)?;
        Some(add_sheet_protected_range(
            sheet,
            range,
            description,
            warning_only,
        ))
    }

    pub fn update_protected_range(
        &mut self,
        sheet_id: &str,
        range: &str,
        description: &str,
        warning_only: bool,
    ) -> Option<Result<(), SpreadsheetError>> {
        let sheet = self.sheets.iter_mut().find(|sheet| sheet.id == sheet_id)?;
        let normalized = match normalize_cell_range(range) {
            Ok(normalized) => normalized,
            Err(err) => return Some(Err(err)),
        };
        if !sheet
            .protected_ranges
            .iter()
            .any(|protected| protected.range == normalized)
        {
            return Some(Err(SpreadsheetError::NotFound(format!(
                "protected range {sheet_id}!{normalized} was not found"
            ))));
        }
        Some(add_sheet_protected_range(
            sheet,
            &normalized,
            description,
            warning_only,
        ))
    }

    pub fn protected_range(&self, sheet_id: &str, range: &str) -> Option<&SheetProtectedRange> {
        let normalized = normalize_cell_range(range).ok()?;
        self.sheets
            .iter()
            .find(|sheet| sheet.id == sheet_id)?
            .protected_ranges
            .iter()
            .find(|protected| protected.range == normalized)
    }

    pub fn delete_protected_range(&mut self, sheet_id: &str, range: &str) -> Option<()> {
        let sheet = self.sheets.iter_mut().find(|sheet| sheet.id == sheet_id)?;
        let normalized = normalize_cell_range(range).unwrap_or_else(|_| range.to_string());
        sheet
            .protected_ranges
            .retain(|protected| protected.range != normalized);
        Some(())
    }

    pub fn restore_protected_range(
        &mut self,
        sheet_id: &str,
        protected_range: SheetProtectedRange,
    ) -> Result<(), SpreadsheetError> {
        protected_range.validate_source()?;
        let sheet = self
            .sheets
            .iter_mut()
            .find(|sheet| sheet.id == sheet_id)
            .ok_or_else(|| SpreadsheetError::NotFound(format!("sheet {sheet_id} was not found")))?;
        if sheet.protected_ranges.iter().any(|existing| {
            existing.id == protected_range.id || existing.range == protected_range.range
        }) {
            return Err(SpreadsheetError::Conflict(format!(
                "protected range {sheet_id}!{} already exists",
                protected_range.range
            )));
        }
        add_sheet_protected_range(
            sheet,
            &protected_range.range,
            &protected_range.description,
            protected_range.warning_only,
        )?;
        Ok(())
    }

    pub fn add_row(&mut self, sheet_id: &str, row: &str) -> Option<()> {
        let sheet = self.sheets.iter_mut().find(|sheet| sheet.id == sheet_id)?;
        if !sheet.rows.iter().any(|item| item == row) {
            sheet.rows.push(row.to_string());
            sheet
                .rows
                .sort_by_key(|value| value.parse::<u32>().unwrap_or(0));
        }
        sheet.ensure_axis_metadata();
        Some(())
    }

    pub fn has_row(&self, sheet_id: &str, row: &str) -> bool {
        self.sheets
            .iter()
            .find(|sheet| sheet.id == sheet_id)
            .is_some_and(|sheet| sheet.rows.iter().any(|item| item == row))
    }

    pub fn row_restore_payload(&self, sheet_id: &str, row: &str) -> Option<DeletedRowPayload> {
        let sheet = self.sheets.iter().find(|sheet| sheet.id == sheet_id)?;
        if !sheet.rows.iter().any(|item| item == row) {
            return None;
        }
        let row_axis = sheet
            .row_axes
            .iter()
            .find(|axis| axis.label == row)
            .cloned()
            .unwrap_or_else(|| row_axis(row.to_string()));
        Some(DeletedRowPayload {
            row_axis,
            cells: sheet
                .cells
                .iter()
                .filter(|cell| split_cell_address(&cell.address).1 == row)
                .cloned()
                .collect(),
            merges: sheet
                .merges
                .iter()
                .filter(|merge| range_contains_row(&merge.range, row))
                .cloned()
                .collect(),
            filters: sheet
                .filters
                .iter()
                .filter(|filter| range_contains_row(&filter.range, row))
                .cloned()
                .collect(),
            protected_ranges: sheet
                .protected_ranges
                .iter()
                .filter(|protected| range_contains_row(&protected.range, row))
                .cloned()
                .collect(),
            named_ranges: self
                .named_ranges
                .iter()
                .filter(|range| range.sheet_id == sheet_id && range_contains_row(&range.range, row))
                .cloned()
                .collect(),
        })
    }

    pub fn delete_row(&mut self, sheet_id: &str, row: &str) -> Option<bool> {
        let sheet = self.sheets.iter_mut().find(|sheet| sheet.id == sheet_id)?;
        let index = sheet.rows.iter().position(|item| item == row)?;
        if sheet.rows.len() <= 1 {
            return Some(false);
        }
        sheet.rows.remove(index);
        sheet.row_axes.retain(|axis| axis.label != row);
        sheet
            .cells
            .retain(|cell| split_cell_address(&cell.address).1 != row);
        sheet
            .merges
            .retain(|merge| !range_contains_row(&merge.range, row));
        sheet
            .filters
            .retain(|filter| !range_contains_row(&filter.range, row));
        sheet
            .protected_ranges
            .retain(|protected| !range_contains_row(&protected.range, row));
        self.named_ranges
            .retain(|range| range.sheet_id != sheet_id || !range_contains_row(&range.range, row));
        Some(true)
    }

    pub fn restore_row(
        &mut self,
        sheet_id: &str,
        row: &str,
        payload: DeletedRowPayload,
    ) -> Result<(), SpreadsheetError> {
        payload.validate_source(row)?;
        let sheet = self
            .sheets
            .iter_mut()
            .find(|sheet| sheet.id == sheet_id)
            .ok_or_else(|| SpreadsheetError::NotFound(format!("sheet {sheet_id} was not found")))?;
        if sheet.rows.iter().any(|item| item == row) {
            return Err(SpreadsheetError::Conflict(format!(
                "row {sheet_id}!{row} already exists"
            )));
        }
        sheet.rows.push(row.to_string());
        sheet
            .rows
            .sort_by_key(|value| value.parse::<u32>().unwrap_or(0));
        sheet.row_axes.retain(|axis| axis.label != row);
        sheet.row_axes.push(payload.row_axis.clone());
        sheet.ensure_axis_metadata();
        for cell in payload.cells {
            upsert_sheet_cell(sheet, cell);
        }
        for merge in payload.merges {
            if !sheet
                .merges
                .iter()
                .any(|existing| existing.id == merge.id || existing.range == merge.range)
            {
                merge_sheet_cells(sheet, &merge.range)?;
            }
        }
        for filter in payload.filters {
            if sheet.filters.is_empty() {
                sheet.filters.push(filter);
            }
        }
        for protected_range in payload.protected_ranges {
            if !sheet.protected_ranges.iter().any(|existing| {
                existing.id == protected_range.id || existing.range == protected_range.range
            }) {
                add_sheet_protected_range(
                    sheet,
                    &protected_range.range,
                    &protected_range.description,
                    protected_range.warning_only,
                )?;
            }
        }
        for named_range in payload.named_ranges {
            if !self
                .named_ranges
                .iter()
                .any(|existing| existing.id == named_range.id || existing.name == named_range.name)
            {
                self.named_ranges.push(named_range);
            }
        }
        self.named_ranges
            .sort_by(|left, right| left.name.cmp(&right.name));
        self.validate_source()
    }

    pub fn add_column(&mut self, sheet_id: &str, column: &str) -> Option<()> {
        let sheet = self.sheets.iter_mut().find(|sheet| sheet.id == sheet_id)?;
        if !sheet.columns.iter().any(|item| item == column) {
            sheet.columns.push(column.to_string());
            sheet
                .columns
                .sort_by_key(|value| column_to_number(value).unwrap_or(0));
        }
        sheet.ensure_axis_metadata();
        Some(())
    }

    pub fn has_column(&self, sheet_id: &str, column: &str) -> bool {
        self.sheets
            .iter()
            .find(|sheet| sheet.id == sheet_id)
            .is_some_and(|sheet| sheet.columns.iter().any(|item| item == column))
    }

    pub fn column_restore_payload(
        &self,
        sheet_id: &str,
        column: &str,
    ) -> Option<DeletedColumnPayload> {
        let sheet = self.sheets.iter().find(|sheet| sheet.id == sheet_id)?;
        if !sheet.columns.iter().any(|item| item == column) {
            return None;
        }
        let column_axis = sheet
            .column_axes
            .iter()
            .find(|axis| axis.label == column)
            .cloned()
            .unwrap_or_else(|| column_axis(column.to_string()));
        Some(DeletedColumnPayload {
            column_axis,
            cells: sheet
                .cells
                .iter()
                .filter(|cell| split_cell_address(&cell.address).0 == column)
                .cloned()
                .collect(),
            merges: sheet
                .merges
                .iter()
                .filter(|merge| range_contains_column(&merge.range, column))
                .cloned()
                .collect(),
            filters: sheet
                .filters
                .iter()
                .filter(|filter| range_contains_column(&filter.range, column))
                .cloned()
                .collect(),
            protected_ranges: sheet
                .protected_ranges
                .iter()
                .filter(|protected| range_contains_column(&protected.range, column))
                .cloned()
                .collect(),
            named_ranges: self
                .named_ranges
                .iter()
                .filter(|range| {
                    range.sheet_id == sheet_id && range_contains_column(&range.range, column)
                })
                .cloned()
                .collect(),
        })
    }

    pub fn delete_column(&mut self, sheet_id: &str, column: &str) -> Option<bool> {
        let sheet = self.sheets.iter_mut().find(|sheet| sheet.id == sheet_id)?;
        let index = sheet.columns.iter().position(|item| item == column)?;
        if sheet.columns.len() <= 1 {
            return Some(false);
        }
        sheet.columns.remove(index);
        sheet.column_axes.retain(|axis| axis.label != column);
        sheet
            .cells
            .retain(|cell| split_cell_address(&cell.address).0 != column);
        sheet
            .merges
            .retain(|merge| !range_contains_column(&merge.range, column));
        sheet
            .filters
            .retain(|filter| !range_contains_column(&filter.range, column));
        sheet
            .protected_ranges
            .retain(|protected| !range_contains_column(&protected.range, column));
        self.named_ranges.retain(|range| {
            range.sheet_id != sheet_id || !range_contains_column(&range.range, column)
        });
        Some(true)
    }

    pub fn restore_column(
        &mut self,
        sheet_id: &str,
        column: &str,
        payload: DeletedColumnPayload,
    ) -> Result<(), SpreadsheetError> {
        payload.validate_source(column)?;
        let sheet = self
            .sheets
            .iter_mut()
            .find(|sheet| sheet.id == sheet_id)
            .ok_or_else(|| SpreadsheetError::NotFound(format!("sheet {sheet_id} was not found")))?;
        if sheet.columns.iter().any(|item| item == column) {
            return Err(SpreadsheetError::Conflict(format!(
                "column {sheet_id}!{column} already exists"
            )));
        }
        sheet.columns.push(column.to_string());
        sheet
            .columns
            .sort_by_key(|value| column_to_number(value).unwrap_or(0));
        sheet.column_axes.retain(|axis| axis.label != column);
        sheet.column_axes.push(payload.column_axis.clone());
        sheet.ensure_axis_metadata();
        for cell in payload.cells {
            upsert_sheet_cell(sheet, cell);
        }
        for merge in payload.merges {
            if !sheet
                .merges
                .iter()
                .any(|existing| existing.id == merge.id || existing.range == merge.range)
            {
                merge_sheet_cells(sheet, &merge.range)?;
            }
        }
        for filter in payload.filters {
            if sheet.filters.is_empty() {
                sheet.filters.push(filter);
            }
        }
        for protected_range in payload.protected_ranges {
            if !sheet.protected_ranges.iter().any(|existing| {
                existing.id == protected_range.id || existing.range == protected_range.range
            }) {
                add_sheet_protected_range(
                    sheet,
                    &protected_range.range,
                    &protected_range.description,
                    protected_range.warning_only,
                )?;
            }
        }
        for named_range in payload.named_ranges {
            if !self
                .named_ranges
                .iter()
                .any(|existing| existing.id == named_range.id || existing.name == named_range.name)
            {
                self.named_ranges.push(named_range);
            }
        }
        self.named_ranges
            .sort_by(|left, right| left.name.cmp(&right.name));
        self.validate_source()
    }

    pub fn add_cell_comment(
        &mut self,
        sheet_id: &str,
        address: &str,
        comment_id: &str,
        author: &str,
        body: &str,
    ) -> Option<()> {
        let sheet = self.sheets.iter_mut().find(|sheet| sheet.id == sheet_id)?;
        sheet.ensure_address(address);
        let cell = sheet.cell_mut_or_insert(address);
        if cell.comments.iter().any(|comment| comment.id == comment_id) {
            return Some(());
        }
        cell.comments.push(CellComment {
            id: comment_id.to_string(),
            author: author.to_string(),
            body: body.to_string(),
            deleted: false,
        });
        cell.comments.sort_by(|left, right| left.id.cmp(&right.id));
        Some(())
    }

    pub fn update_cell_comment(&mut self, comment_id: &str, body: &str) -> Option<()> {
        let comment = self.cell_comment_mut(comment_id)?;
        if comment.deleted {
            return None;
        }
        comment.body = body.to_string();
        Some(())
    }

    pub fn delete_cell_comment(&mut self, comment_id: &str) -> Option<()> {
        let comment = self.cell_comment_mut(comment_id)?;
        comment.deleted = true;
        Some(())
    }

    pub fn restore_cell_comment(&mut self, comment_id: &str) -> Option<()> {
        let comment = self.cell_comment_mut(comment_id)?;
        if !comment.deleted {
            return None;
        }
        comment.deleted = false;
        Some(())
    }

    pub fn cell_comment_mut(&mut self, comment_id: &str) -> Option<&mut CellComment> {
        self.sheets
            .iter_mut()
            .flat_map(|sheet| sheet.cells.iter_mut())
            .flat_map(|cell| cell.comments.iter_mut())
            .find(|comment| comment.id == comment_id)
    }

    pub fn deleted_cell_comments(&self) -> Vec<DeletedCellComment> {
        let mut deleted = self
            .sheets
            .iter()
            .flat_map(|sheet| {
                sheet.cells.iter().flat_map(move |cell| {
                    cell.comments
                        .iter()
                        .filter(|comment| comment.deleted)
                        .map(move |comment| DeletedCellComment {
                            sheet_id: sheet.id.clone(),
                            address: cell.address.clone(),
                            comment: comment.clone(),
                        })
                })
            })
            .collect::<Vec<_>>();
        deleted.sort_by(|left, right| {
            (
                left.sheet_id.as_str(),
                left.address.as_str(),
                left.comment.id.as_str(),
            )
                .cmp(&(
                    right.sheet_id.as_str(),
                    right.address.as_str(),
                    right.comment.id.as_str(),
                ))
        });
        deleted
    }
}
