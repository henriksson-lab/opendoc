use crate::{
    normalize_sheet_id, AppApiError, AppDocument, AppSpreadsheetOperation, OpenDocApp,
    SpreadsheetEvaluationPolicy,
};

pub type AppSpreadsheetSelection = opendoc_spreadsheet::SpreadsheetSelectionSummary;

impl OpenDocApp {
    pub fn describe_spreadsheet_selection(
        &self,
        sheet_id: impl AsRef<str>,
        anchor: impl AsRef<str>,
        focus: impl AsRef<str>,
    ) -> Result<AppSpreadsheetSelection, AppApiError> {
        let sheet_id = normalize_sheet_id(sheet_id.as_ref())?;
        Ok(self.workbook.describe_selection(sheet_id, anchor, focus)?)
    }

    pub fn reduce_spreadsheet_selection(
        &self,
        sheet_id: impl AsRef<str>,
        anchor: impl AsRef<str>,
        focus: impl AsRef<str>,
        action: impl AsRef<str>,
        value: impl AsRef<str>,
        extend: bool,
    ) -> Result<AppSpreadsheetSelection, AppApiError> {
        let sheet_id = normalize_sheet_id(sheet_id.as_ref())?;
        Ok(self
            .workbook
            .reduce_selection(sheet_id, anchor, focus, action, value, extend)?)
    }

    pub fn copy_spreadsheet_selection_tsv(
        &self,
        sheet_id: impl AsRef<str>,
        anchor: impl AsRef<str>,
        focus: impl AsRef<str>,
    ) -> Result<String, AppApiError> {
        let sheet_id = normalize_sheet_id(sheet_id.as_ref())?;
        Ok(self.workbook.selection_tsv(sheet_id, anchor, focus)?)
    }

    /// Pastes clipboard TSV at `origin`.
    ///
    /// `source_origin` is the top-left cell the text was copied from, when the
    /// copy came from this workbook. Formulas then move with the paste:
    /// relative references shift by the paste offset, absolute ones do not.
    /// Text pasted from elsewhere passes `None` and is stored verbatim.
    pub fn paste_spreadsheet_tsv(
        &mut self,
        sheet_id: impl AsRef<str>,
        origin: impl AsRef<str>,
        text: impl AsRef<str>,
        source_origin: Option<&str>,
    ) -> Result<AppDocument, AppApiError> {
        let sheet_id = normalize_sheet_id(sheet_id.as_ref())?;
        let source_origin = source_origin
            .map(str::trim)
            .filter(|source_origin| !source_origin.is_empty());
        let cells =
            opendoc_spreadsheet::SpreadsheetWorkbook::tsv_cell_edits(origin, text, source_origin)?;
        if cells.is_empty() {
            return Ok(self.document());
        }
        self.set_spreadsheet_cells_in_sheet(sheet_id, cells)
    }

    pub fn clear_spreadsheet_selection(
        &mut self,
        sheet_id: impl AsRef<str>,
        anchor: impl AsRef<str>,
        focus: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let sheet_id = normalize_sheet_id(sheet_id.as_ref())?;
        let selection = self.workbook.describe_selection(&sheet_id, anchor, focus)?;
        let cells = selection
            .addresses()?
            .into_iter()
            .map(|address| (address, String::new()))
            .collect();
        self.set_spreadsheet_cells_in_sheet(sheet_id, cells)
    }

    pub fn set_spreadsheet_selection_format(
        &mut self,
        sheet_id: impl AsRef<str>,
        anchor: impl AsRef<str>,
        focus: impl AsRef<str>,
        property: impl AsRef<str>,
        value: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let sheet_id = normalize_sheet_id(sheet_id.as_ref())?;
        let selection = self.workbook.describe_selection(&sheet_id, anchor, focus)?;
        let property = property.as_ref().trim().to_string();
        let value = value.into();
        let addresses = selection.addresses()?;
        self.mutate_spreadsheet(SpreadsheetEvaluationPolicy::Force, |workbook| {
            for address in &addresses {
                workbook
                    .set_cell_format(&sheet_id, address, &property, value.clone())
                    .ok_or_else(|| {
                        AppApiError::NotFound(format!("sheet {sheet_id} was not found"))
                    })??;
            }
            Ok(())
        })?;
        for address in addresses {
            self.journal_spreadsheet_operation(
                &format!("format {sheet_id}!{address} {property}"),
                AppSpreadsheetOperation::SetCellFormat {
                    sheet_id: sheet_id.clone(),
                    address,
                    property: property.clone(),
                    value: value.clone(),
                },
            );
        }
        Ok(self.document())
    }

    pub fn add_spreadsheet_row_after_selection(
        &mut self,
        sheet_id: impl AsRef<str>,
        focus: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let row = self.workbook.row_after_focus(focus)?;
        self.add_spreadsheet_row(sheet_id, row)
    }

    pub fn add_spreadsheet_column_after_selection(
        &mut self,
        sheet_id: impl AsRef<str>,
        focus: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let column = self.workbook.column_after_focus(focus)?;
        self.add_spreadsheet_column(sheet_id, column)
    }

    pub fn delete_spreadsheet_selection_row(
        &mut self,
        sheet_id: impl AsRef<str>,
        focus: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let row = self.workbook.focus_row_label(focus)?;
        self.delete_spreadsheet_row(sheet_id, row)
    }

    pub fn delete_spreadsheet_selection_column(
        &mut self,
        sheet_id: impl AsRef<str>,
        focus: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let column = self.workbook.focus_column_label(focus)?;
        self.delete_spreadsheet_column(sheet_id, column)
    }

    pub fn merge_spreadsheet_selection(
        &mut self,
        sheet_id: impl AsRef<str>,
        anchor: impl AsRef<str>,
        focus: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let sheet_id = normalize_sheet_id(sheet_id.as_ref())?;
        let selection = self.workbook.describe_selection(&sheet_id, anchor, focus)?;
        if selection.from_address == selection.to_address {
            return Ok(self.document());
        }
        self.merge_spreadsheet_cells(sheet_id, selection.range)
    }

    pub fn freeze_spreadsheet_selection(
        &mut self,
        sheet_id: impl AsRef<str>,
        focus: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let (frozen_rows, frozen_columns) = self.workbook.frozen_axes_for_focus(focus)?;
        self.set_spreadsheet_frozen_axes(sheet_id, frozen_rows, frozen_columns)
    }

    pub fn set_spreadsheet_selection_filter(
        &mut self,
        sheet_id: impl AsRef<str>,
        anchor: impl AsRef<str>,
        focus: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let sheet_id = normalize_sheet_id(sheet_id.as_ref())?;
        let selection = self.workbook.describe_selection(&sheet_id, anchor, focus)?;
        self.set_spreadsheet_basic_filter(sheet_id, selection.range)
    }
}
