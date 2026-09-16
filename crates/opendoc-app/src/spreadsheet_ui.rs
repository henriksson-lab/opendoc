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

    /// Hides or reveals every row the selection covers.
    ///
    /// Selection-scoped rather than per-row because that is how a person
    /// unhides: a hidden row is still inside a range that spans it, so
    /// selecting across the gap and revealing is the only gesture that can
    /// reach a row nothing draws.
    pub fn set_spreadsheet_selection_rows_hidden(
        &mut self,
        sheet_id: impl AsRef<str>,
        anchor: impl AsRef<str>,
        focus: impl AsRef<str>,
        hidden: bool,
    ) -> Result<AppDocument, AppApiError> {
        let sheet_id = normalize_sheet_id(sheet_id.as_ref())?;
        let selection = self.workbook.describe_selection(&sheet_id, anchor, focus)?;
        let rows = selection.row_labels();
        // Forced: `SUBTOTAL(101..)` skips hidden rows, so hiding one changes
        // what a formula elsewhere on the sheet computes.
        //
        // A row the sheet does not have — a selection running past the last
        // one — fails the whole mutation: `mutate_spreadsheet` stages it, so
        // the rows before the missing one are not left hidden by a call that
        // reported a failure.
        self.mutate_spreadsheet(SpreadsheetEvaluationPolicy::Force, |workbook| {
            for row in &rows {
                workbook
                    .set_row_hidden(&sheet_id, row, hidden)
                    .ok_or_else(|| {
                        AppApiError::NotFound(format!(
                            "sheet {sheet_id} or row {row} was not found"
                        ))
                    })?;
            }
            Ok(())
        })?;
        for row in rows {
            self.journal_spreadsheet_operation(
                &format!(
                    "{} row {sheet_id}!{row}",
                    if hidden { "hide" } else { "show" }
                ),
                AppSpreadsheetOperation::SetRowHidden {
                    sheet_id: sheet_id.clone(),
                    row,
                    hidden,
                },
            );
        }
        Ok(self.document())
    }

    /// Hides or reveals every column the selection covers, on the same terms
    /// as [`OpenDocApp::set_spreadsheet_selection_rows_hidden`].
    pub fn set_spreadsheet_selection_columns_hidden(
        &mut self,
        sheet_id: impl AsRef<str>,
        anchor: impl AsRef<str>,
        focus: impl AsRef<str>,
        hidden: bool,
    ) -> Result<AppDocument, AppApiError> {
        let sheet_id = normalize_sheet_id(sheet_id.as_ref())?;
        let selection = self.workbook.describe_selection(&sheet_id, anchor, focus)?;
        let columns = selection.column_labels()?;
        self.mutate_spreadsheet(SpreadsheetEvaluationPolicy::Force, |workbook| {
            for column in &columns {
                workbook
                    .set_column_hidden(&sheet_id, column, hidden)
                    .ok_or_else(|| {
                        AppApiError::NotFound(format!(
                            "sheet {sheet_id} or column {column} was not found"
                        ))
                    })?;
            }
            Ok(())
        })?;
        for column in columns {
            self.journal_spreadsheet_operation(
                &format!(
                    "{} column {sheet_id}!{column}",
                    if hidden { "hide" } else { "show" }
                ),
                AppSpreadsheetOperation::SetColumnHidden {
                    sheet_id: sheet_id.clone(),
                    column,
                    hidden,
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

#[cfg(test)]
mod tests {
    use crate::OpenDocApp;
    use serde_json::json;

    /// Every `data-address` the rendered grid carries, in document order.
    fn drawn_addresses(html: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut rest = html;
        while let Some(index) = rest.find("data-address=\"") {
            rest = &rest[index + "data-address=\"".len()..];
            let Some(end) = rest.find('"') else { break };
            out.push(rest[..end].to_string());
            rest = &rest[end..];
        }
        out
    }

    /// The cross-check that matters for the merged-cell case: whatever address
    /// `reduce_selection` answers with has to be an address the renderer
    /// actually emitted, or the focus ring and the cell editor have nothing to
    /// attach to.
    ///
    /// This is the test that corrected the design. `opendoc-render` draws a
    /// merged block at the first corner of its rectangle that survives hiding,
    /// but labels that `<td>` with the merge's *anchor* — so with column A
    /// hidden the grid contains `data-address="A1"` and does **not** contain
    /// `B1`, which the merge covers. Snapping the click forward to `B1`, which
    /// is what "a hidden cell is never selectable" implied, produced an
    /// address in no element at all. The anchor of a partly drawn merge is
    /// therefore the one hidden address the reducer keeps, and this asserts the
    /// agreement against the real markup rather than assuming it.
    #[test]
    fn a_click_on_a_merged_block_selects_an_address_the_renderer_drew() {
        let mut app = OpenDocApp::new_empty_document();
        app.dispatch_command("create_document", json!({ "title": "Sheet" }))
            .expect("create");
        app.dispatch_command(
            "set_spreadsheet_cell",
            json!({ "sheetId": "sheet-1", "address": "A1", "value": "merged" }),
        )
        .expect("a value on the merge anchor");
        app.dispatch_command(
            "merge_spreadsheet_selection",
            json!({ "sheetId": "sheet-1", "anchor": "A1", "focus": "B2" }),
        )
        .expect("merge A1:B2");
        app.dispatch_command(
            "set_spreadsheet_selection_columns_hidden",
            json!({ "sheetId": "sheet-1", "anchor": "A1", "focus": "A1", "hidden": true }),
        )
        .expect("hide column A, the merge's anchor column");

        let html = app
            .render_workbook_html("sheet-1")
            .expect("the grid projects");
        let drawn = drawn_addresses(&html);
        assert!(
            drawn.contains(&"A1".to_string()),
            "the merged block is still drawn and still labelled with its anchor"
        );
        assert!(
            !drawn.contains(&"A2".to_string()),
            "column A is otherwise omitted from the grid: {drawn:?}"
        );

        assert!(
            !drawn.contains(&"B1".to_string()),
            "B1 is covered by the merge, so the grid emits no element for it"
        );

        // The click the browser can produce: the merged block's own label.
        let selection = app
            .reduce_spreadsheet_selection("sheet-1", "C3", "C3", "set-focus", "A1", false)
            .expect("a click on the merged block");
        assert_eq!(
            selection.focus, "A1",
            "the anchor of a drawn merge stays selectable"
        );
        assert!(
            drawn.contains(&selection.focus),
            "the selected focus {} is not an address the renderer drew",
            selection.focus
        );

        // And the cell the formula bar would edit is the one holding the
        // content, which is the whole reason for keeping the anchor.
        assert_eq!(
            app.describe_spreadsheet_selection("sheet-1", &selection.anchor, &selection.focus)
                .expect("the selection projects")
                .selected_tsv,
            "merged"
        );
    }

    /// Arrow-keying over a hidden row lands on a drawn one, end to end through
    /// the facade and against the grid the renderer produced.
    #[test]
    fn arrow_keying_over_a_hidden_row_lands_on_a_drawn_row() {
        let mut app = OpenDocApp::new_empty_document();
        app.dispatch_command("create_document", json!({ "title": "Sheet" }))
            .expect("create");
        app.dispatch_command(
            "set_spreadsheet_selection_rows_hidden",
            json!({ "sheetId": "sheet-1", "anchor": "A3", "focus": "A4", "hidden": true }),
        )
        .expect("hide rows 3 and 4");

        let selection = app
            .reduce_spreadsheet_selection("sheet-1", "A2", "A2", "move", "down", false)
            .expect("move down");
        assert_eq!(selection.focus, "A5");

        let drawn = drawn_addresses(
            &app.render_workbook_html("sheet-1")
                .expect("the grid projects"),
        );
        assert!(drawn.contains(&"A5".to_string()));
        assert!(!drawn.contains(&"A3".to_string()));
    }
}
