//! Spreadsheet workbook state and high-level workbook mutations.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{DeletedColumnPayload, DeletedRowPayload, SpreadsheetError, SpreadsheetWarning};

use super::address::{
    cell_address, column_to_number, normalize_cell_address, normalize_cell_range,
    normalize_named_range_name, normalize_sheet_title, parse_cell_range, range_contains_column,
    range_contains_row, rewrite_formula_sheet_title_references, split_cell_address,
};
use super::fill::fill_sheet_range;
use super::format::Locale;
use super::io;
use super::model::{
    column_axis, row_axis, validate_axis_size_px, Cell, CellComment, CellDependency,
    CellValidation, DeletedCellComment, NamedRange, Sheet, SheetFilter, SheetFilterCriterion,
    SheetFilterSortSpec, SheetMerge, SheetProtectedRange,
};
use super::recalc::SpreadsheetEvaluationContext;
use super::structure::{
    add_sheet_protected_range, copy_sheet_range, delete_axis, insert_axis, merge_cover_anchor,
    merge_sheet_cells, set_sheet_basic_filter, set_sheet_basic_filter_options, set_sheet_cell,
    set_sheet_cell_format, sort_range, upsert_sheet_cell, Axis,
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
        workbook.evaluate();
        workbook
    }

    /// Evaluates in place, on the same terms as [`SpreadsheetWorkbook::evaluated`].
    ///
    /// The two are one function: `evaluated` is this one applied to a copy.
    /// Callers that already hold a workbook they own — a staged mutation about
    /// to be committed, for instance — would otherwise clone it a second time
    /// only to throw the original away.
    ///
    /// The dependency graph is rebuilt only when evaluation reports that some
    /// formula source or the sheet structure changed. It is derived from
    /// formula source alone, so an evaluation that changed no source — the
    /// second `evaluate()` of a commit, a projection re-evaluating what a
    /// render already evaluated — leaves a graph that is still exact, and
    /// rebuilding it re-parsed every formula in the workbook for nothing.
    pub fn evaluate(&mut self) {
        for sheet in &mut self.sheets {
            sheet.ensure_axis_metadata();
        }
        let outcome = super::recalc::evaluate_workbook(self);
        if outcome.dependency_graph_stale {
            self.dependency_graph =
                super::recalc::build_dependency_graph(&self.sheets, &self.named_ranges);
        }
    }

    /// The raw first-sheet writer: no validation rule and no merge is
    /// consulted.
    ///
    /// It exists for fixtures that stage a workbook directly — a render test
    /// wanting a formula cell that was never evaluated, say. **Every path a
    /// user's keystroke can reach must use
    /// [`SpreadsheetWorkbook::set_cell_in_sheet`] instead**, which is the one
    /// that refuses a write a strict rule rejects or a merge would hide.
    pub fn set_cell(&mut self, address: &str, value: String) {
        let locale = Locale::for_tag(&self.locale);
        let Some(sheet) = self.sheets.first_mut() else {
            return;
        };
        set_sheet_cell(sheet, address, value, &locale);
    }

    /// Writes one cell from text the user typed, refusing the two writes
    /// that would otherwise land somewhere the user cannot see or did not
    /// agree to.
    ///
    /// **A cell a merge covers is not writable.** Only the anchor of a merged
    /// block is drawn, so a value stored on a covered cell is invisible,
    /// uneditable and still signed into the document. The refusal names the
    /// anchor, which is the cell the caller meant.
    ///
    /// **A `strict` validation rule is enforced.** A rule marked strict is the
    /// sheet saying *reject this entry*; storing the value anyway made the
    /// rule worse than no rule at all, because the dropdown, the export and
    /// the audit view all went on advertising it. See [`super::validation`]
    /// for exactly which rules are decidable here and which deliberately are
    /// not.
    pub fn set_cell_in_sheet(
        &mut self,
        sheet_id: &str,
        address: &str,
        value: String,
    ) -> Result<(), SpreadsheetError> {
        let locale = Locale::for_tag(&self.locale);
        self.refuse_hidden_or_invalid_cell_write(sheet_id, address, &value, &locale)?;
        let sheet = self
            .sheets
            .iter_mut()
            .find(|sheet| sheet.id == sheet_id)
            .ok_or_else(|| SpreadsheetError::NotFound(format!("sheet {sheet_id} was not found")))?;
        set_sheet_cell(sheet, address, value, &locale);
        Ok(())
    }

    /// The two refusals [`SpreadsheetWorkbook::set_cell_in_sheet`] documents,
    /// taken before anything is borrowed mutably — a `one_of_range` rule has
    /// to read the rest of the workbook to decide.
    fn refuse_hidden_or_invalid_cell_write(
        &self,
        sheet_id: &str,
        address: &str,
        value: &str,
        locale: &Locale,
    ) -> Result<(), SpreadsheetError> {
        let sheet = self
            .sheets
            .iter()
            .find(|sheet| sheet.id == sheet_id)
            .ok_or_else(|| SpreadsheetError::NotFound(format!("sheet {sheet_id} was not found")))?;
        if let Some(anchor) = merge_cover_anchor(sheet, address) {
            return Err(SpreadsheetError::Conflict(format!(
                "cell {sheet_id}!{address} is covered by a merge and is never drawn; write to its anchor {anchor} instead"
            )));
        }
        let Some(validation) = sheet
            .cells
            .iter()
            .find(|cell| cell.address == address)
            .and_then(|cell| cell.validation.as_ref())
        else {
            return Ok(());
        };
        if !validation.strict {
            return Ok(());
        }
        let range_values = super::validation::range_reference(validation)
            .and_then(|reference| self.range_literal_values(sheet_id, reference));
        match super::validation::refusal(validation, value, locale, range_values.as_deref()) {
            Some(reason) => Err(SpreadsheetError::Conflict(format!(
                "cell {sheet_id}!{address} rejects this entry: {reason}"
            ))),
            None => Ok(()),
        }
    }

    /// The non-empty user values of the cells `reference` names, for a
    /// `one_of_range` validation rule. `None` when the reference does not
    /// resolve, which makes the rule undecidable rather than refusing.
    fn range_literal_values(&self, sheet_id: &str, reference: &str) -> Option<Vec<String>> {
        let reference = reference.trim().trim_start_matches('=').replace('$', "");
        let (sheet, range) = match reference.rsplit_once('!') {
            Some((prefix, range)) => {
                let prefix = prefix.trim().trim_matches('\'');
                let sheet = self
                    .sheets
                    .iter()
                    .find(|sheet| sheet.id == prefix || sheet.title == prefix)?;
                (sheet, range)
            }
            None => (
                self.sheets.iter().find(|sheet| sheet.id == sheet_id)?,
                reference.as_str(),
            ),
        };
        let range = parse_cell_range(&normalize_cell_range(range).ok()?).ok()?;
        let mut values = Vec::new();
        for row in range.start_row..range.start_row + range.height {
            for column in range.start_column..range.start_column + range.width {
                let address = cell_address(column, row).ok()?;
                if let Some(cell) = sheet.cells.iter().find(|cell| cell.address == address) {
                    if !cell.computed_value.is_empty() {
                        values.push(cell.computed_value.clone());
                    }
                }
            }
        }
        Some(values)
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

    /// Sorts the rows of `range` by one of its columns (SH-7). Formulas move
    /// with their row, so relative references keep pointing at the same data.
    pub fn sort_range(
        &mut self,
        sheet_id: &str,
        range: &str,
        column: &str,
        descending: bool,
        has_header: bool,
    ) -> Option<Result<(), SpreadsheetError>> {
        let sheet = self.sheets.iter_mut().find(|sheet| sheet.id == sheet_id)?;
        let Some(column) = column_to_number(column) else {
            return Some(Err(SpreadsheetError::Format(format!(
                "sort column {column} is not a column label"
            ))));
        };
        Some(sort_range(sheet, range, column, descending, has_header))
    }

    /// Expands `source_range` over `target_range` from the fill handle
    /// (SH-28). Series inference lives in `fill`.
    pub fn fill_range(
        &mut self,
        sheet_id: &str,
        source_range: &str,
        target_range: &str,
    ) -> Option<Result<(), SpreadsheetError>> {
        let sheet = self.sheets.iter_mut().find(|sheet| sheet.id == sheet_id)?;
        Some(fill_sheet_range(sheet, source_range, target_range))
    }

    /// Cell edits for a CSV/TSV import landing at `origin` (SH-47).
    pub fn csv_cell_edits(
        origin: &str,
        text: &str,
        delimiter: Option<&str>,
    ) -> Result<Vec<(String, String)>, SpreadsheetError> {
        let delimiter = io::parse_delimiter(delimiter)?;
        let rows = io::parse_csv(text, delimiter)?;
        let (origin_column, origin_row) =
            super::address::parse_cell_position(&normalize_cell_address(origin)?)?;
        let mut edits = Vec::new();
        for (row_offset, row) in rows.iter().enumerate() {
            for (column_offset, value) in row.iter().enumerate() {
                let address = cell_address(
                    origin_column + column_offset as u32,
                    origin_row + row_offset as u32,
                )?;
                edits.push((address, value.clone()));
            }
        }
        Ok(edits)
    }

    /// One sheet's populated grid as CSV/TSV, using display text so stored
    /// number formats survive the round trip to another tool.
    pub fn export_csv(
        &self,
        sheet_id: &str,
        delimiter: Option<&str>,
    ) -> Result<String, SpreadsheetError> {
        let delimiter = io::parse_delimiter(delimiter)?;
        let sheet = self
            .sheets
            .iter()
            .find(|sheet| sheet.id == sheet_id)
            .ok_or_else(|| SpreadsheetError::NotFound(format!("sheet {sheet_id} was not found")))?;
        io::export_csv(sheet, delimiter, &Locale::for_tag(&self.locale))
    }

    /// Reads a base64-encoded XLSX workbook (SH-47).
    pub fn from_xlsx_base64(base64: &str, title: &str) -> Result<Self, SpreadsheetError> {
        let bytes = io::decode_base64(base64)?;
        io::import_xlsx(&bytes, title)
    }

    /// Reads an XLSX workbook and reports package features that could not be
    /// represented by the spreadsheet model. UI import paths use this form so
    /// a successful cell-grid import never implies that floating drawings
    /// arrived too.
    pub fn from_xlsx_base64_with_warnings(
        base64: &str,
        title: &str,
    ) -> Result<io::XlsxImportReport, SpreadsheetError> {
        let bytes = io::decode_base64(base64)?;
        io::import_xlsx_with_warnings(&bytes, title)
    }

    /// Writes the workbook as a base64-encoded XLSX file (SH-47).
    pub fn to_xlsx_base64(&self) -> Result<String, SpreadsheetError> {
        Ok(io::encode_base64(&io::export_xlsx(self)?))
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

    /// Sets or clears a sheet's durable inclusive print area.
    pub fn set_print_area(
        &mut self,
        sheet_id: &str,
        print_area: Option<&str>,
    ) -> Option<Result<(), SpreadsheetError>> {
        let sheet = self.sheets.iter_mut().find(|sheet| sheet.id == sheet_id)?;
        Some(sheet.set_print_area(print_area))
    }

    /// Sets a sheet's durable PDF paper orientation.
    pub fn set_print_orientation(
        &mut self,
        sheet_id: &str,
        orientation: crate::SheetPrintOrientation,
    ) -> Option<()> {
        let sheet = self.sheets.iter_mut().find(|sheet| sheet.id == sheet_id)?;
        sheet.set_print_orientation(orientation);
        Some(())
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

    /// Inserts one row at the 1-based position named by `row`, shifting the
    /// rows at and after it down. Cells, formulas, merges, filters,
    /// protected ranges, named ranges, frozen counts, hidden axes, and
    /// explicit row heights all move with the shift.
    pub fn add_row(&mut self, sheet_id: &str, row: &str) -> Option<()> {
        let at = row.parse::<u32>().ok()?;
        insert_axis(self, sheet_id, Axis::Row, at, 1).ok()?;
        Some(())
    }

    /// Stores an explicit row height in pixels. `height == 0` clears the
    /// explicit height so the row falls back to
    /// [`crate::DEFAULT_ROW_HEIGHT_PX`]. `None` means the sheet or the row
    /// was not found.
    pub fn set_row_height(
        &mut self,
        sheet_id: &str,
        row: &str,
        height: u32,
    ) -> Option<Result<(), SpreadsheetError>> {
        let sheet = self.sheets.iter_mut().find(|sheet| sheet.id == sheet_id)?;
        if !sheet.rows.iter().any(|item| item == row) {
            return None;
        }
        if height == 0 {
            sheet.row_heights.remove(row);
            return Some(Ok(()));
        }
        if let Err(err) = validate_axis_size_px("row height", height) {
            return Some(Err(err));
        }
        sheet.row_heights.insert(row.to_string(), height);
        Some(Ok(()))
    }

    /// Stores an explicit column width in pixels. `width == 0` clears the
    /// explicit width so the column falls back to
    /// [`crate::DEFAULT_COLUMN_WIDTH_PX`].
    pub fn set_column_width(
        &mut self,
        sheet_id: &str,
        column: &str,
        width: u32,
    ) -> Option<Result<(), SpreadsheetError>> {
        let sheet = self.sheets.iter_mut().find(|sheet| sheet.id == sheet_id)?;
        if !sheet.columns.iter().any(|item| item == column) {
            return None;
        }
        if width == 0 {
            sheet.column_widths.remove(column);
            return Some(Ok(()));
        }
        if let Err(err) = validate_axis_size_px("column width", width) {
            return Some(Err(err));
        }
        sheet.column_widths.insert(column.to_string(), width);
        Some(Ok(()))
    }

    /// Hides or reveals one row.
    ///
    /// Hiding is not a size of zero: a hidden row keeps whatever height it was
    /// given, so revealing it restores that height rather than a default, and
    /// a formula that reads the row still reads it — only `SUBTOTAL(101..)`
    /// and the projection care that it is hidden. `None` means the sheet or
    /// the row was not found.
    pub fn set_row_hidden(&mut self, sheet_id: &str, row: &str, hidden: bool) -> Option<()> {
        let sheet = self.sheets.iter_mut().find(|sheet| sheet.id == sheet_id)?;
        if !sheet.rows.iter().any(|item| item == row) {
            return None;
        }
        set_axis_hidden(&mut sheet.hidden_rows, row, hidden, |label| {
            label.parse::<u32>().unwrap_or(0)
        });
        Some(())
    }

    /// Hides or reveals one column, on the same terms as
    /// [`SpreadsheetWorkbook::set_row_hidden`].
    pub fn set_column_hidden(&mut self, sheet_id: &str, column: &str, hidden: bool) -> Option<()> {
        let sheet = self.sheets.iter_mut().find(|sheet| sheet.id == sheet_id)?;
        if !sheet.columns.iter().any(|item| item == column) {
            return None;
        }
        set_axis_hidden(&mut sheet.hidden_columns, column, hidden, |label| {
            column_to_number(label).unwrap_or(0)
        });
        Some(())
    }

    /// Whether a row is hidden. `None` means the sheet or the row was not
    /// found, which is a different answer from "not hidden".
    pub fn row_hidden(&self, sheet_id: &str, row: &str) -> Option<bool> {
        let sheet = self.sheets.iter().find(|sheet| sheet.id == sheet_id)?;
        if !sheet.rows.iter().any(|item| item == row) {
            return None;
        }
        Some(sheet.hidden_rows.iter().any(|item| item == row))
    }

    /// Whether a column is hidden.
    pub fn column_hidden(&self, sheet_id: &str, column: &str) -> Option<bool> {
        let sheet = self.sheets.iter().find(|sheet| sheet.id == sheet_id)?;
        if !sheet.columns.iter().any(|item| item == column) {
            return None;
        }
        Some(sheet.hidden_columns.iter().any(|item| item == column))
    }

    /// The effective row height in pixels: the explicit height when one is
    /// stored, otherwise [`crate::DEFAULT_ROW_HEIGHT_PX`].
    pub fn row_height(&self, sheet_id: &str, row: &str) -> Option<u32> {
        let sheet = self.sheets.iter().find(|sheet| sheet.id == sheet_id)?;
        if !sheet.rows.iter().any(|item| item == row) {
            return None;
        }
        Some(
            sheet
                .row_heights
                .get(row)
                .copied()
                .unwrap_or(crate::DEFAULT_ROW_HEIGHT_PX),
        )
    }

    /// The effective column width in pixels: the explicit width when one is
    /// stored, otherwise [`crate::DEFAULT_COLUMN_WIDTH_PX`].
    pub fn column_width(&self, sheet_id: &str, column: &str) -> Option<u32> {
        let sheet = self.sheets.iter().find(|sheet| sheet.id == sheet_id)?;
        if !sheet.columns.iter().any(|item| item == column) {
            return None;
        }
        Some(
            sheet
                .column_widths
                .get(column)
                .copied()
                .unwrap_or(crate::DEFAULT_COLUMN_WIDTH_PX),
        )
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
            row_height: sheet.row_heights.get(row).copied(),
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

    /// Deletes the row named by `row`, shifting every following row up so
    /// the axis keeps a dense 1..n run. `Some(false)` means the delete was
    /// refused because it would remove the last row.
    pub fn delete_row(&mut self, sheet_id: &str, row: &str) -> Option<bool> {
        let start = row.parse::<u32>().ok()?;
        let sheet = self.sheets.iter().find(|sheet| sheet.id == sheet_id)?;
        if !sheet.rows.iter().any(|item| item == row) {
            return None;
        }
        Some(delete_axis(self, sheet_id, Axis::Row, start, 1).is_ok())
    }

    pub fn restore_row(
        &mut self,
        sheet_id: &str,
        row: &str,
        payload: DeletedRowPayload,
    ) -> Result<(), SpreadsheetError> {
        payload.validate_source(row)?;
        let at = row.parse::<u32>().map_err(|_| {
            SpreadsheetError::Format(format!("spreadsheet row {row} is not a position"))
        })?;
        if !self.sheets.iter().any(|sheet| sheet.id == sheet_id) {
            return Err(SpreadsheetError::NotFound(format!(
                "sheet {sheet_id} was not found"
            )));
        }
        // Re-open the position the delete closed, then refill it.
        insert_axis(self, sheet_id, Axis::Row, at, 1)?;
        let sheet = self
            .sheets
            .iter_mut()
            .find(|sheet| sheet.id == sheet_id)
            .ok_or_else(|| SpreadsheetError::NotFound(format!("sheet {sheet_id} was not found")))?;
        if let Some(height) = payload.row_height {
            sheet.row_heights.insert(row.to_string(), height);
        }
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

    /// Inserts one column at the position named by `column`, shifting the
    /// columns at and after it right. See [`SpreadsheetWorkbook::add_row`].
    pub fn add_column(&mut self, sheet_id: &str, column: &str) -> Option<()> {
        let at = column_to_number(column)?;
        insert_axis(self, sheet_id, Axis::Column, at, 1).ok()?;
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
            column_width: sheet.column_widths.get(column).copied(),
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

    /// Deletes the column named by `column`, shifting every following
    /// column left. See [`SpreadsheetWorkbook::delete_row`].
    pub fn delete_column(&mut self, sheet_id: &str, column: &str) -> Option<bool> {
        let start = column_to_number(column)?;
        let sheet = self.sheets.iter().find(|sheet| sheet.id == sheet_id)?;
        if !sheet.columns.iter().any(|item| item == column) {
            return None;
        }
        Some(delete_axis(self, sheet_id, Axis::Column, start, 1).is_ok())
    }

    pub fn restore_column(
        &mut self,
        sheet_id: &str,
        column: &str,
        payload: DeletedColumnPayload,
    ) -> Result<(), SpreadsheetError> {
        payload.validate_source(column)?;
        let at = column_to_number(column).ok_or_else(|| {
            SpreadsheetError::Format(format!("spreadsheet column {column} is not a position"))
        })?;
        if !self.sheets.iter().any(|sheet| sheet.id == sheet_id) {
            return Err(SpreadsheetError::NotFound(format!(
                "sheet {sheet_id} was not found"
            )));
        }
        insert_axis(self, sheet_id, Axis::Column, at, 1)?;
        let sheet = self
            .sheets
            .iter_mut()
            .find(|sheet| sheet.id == sheet_id)
            .ok_or_else(|| SpreadsheetError::NotFound(format!("sheet {sheet_id} was not found")))?;
        if let Some(width) = payload.column_width {
            sheet.column_widths.insert(column.to_string(), width);
        }
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

/// Adds or removes one label from a hidden-axis list, keeping it in axis order
/// and free of duplicates so two replicas that hid the same row serialize the
/// same bytes.
fn set_axis_hidden(
    hidden: &mut Vec<String>,
    label: &str,
    should_hide: bool,
    order: impl Fn(&str) -> u32,
) {
    let present = hidden.iter().any(|item| item == label);
    match (should_hide, present) {
        (true, false) => {
            hidden.push(label.to_string());
            hidden.sort_by_key(|item| order(item));
            hidden.dedup();
        }
        (false, true) => hidden.retain(|item| item != label),
        _ => {}
    }
}
