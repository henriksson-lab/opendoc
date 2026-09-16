//! Application-level tests for positional row/column insert and delete
//! (the `＋row` / `＋col` toolbar path) and for explicit row height /
//! column width sizing.

use super::*;
use crate::spreadsheet_replay::apply_spreadsheet_envelopes;
use serde_json::json;

/// A workbook with demo content for these tests to operate on.
///
/// `new_document` deliberately starts blank (FS-19: "Blank spreadsheet" must
/// be blank), so the fixture states the cells it depends on rather than
/// relying on the constructor to seed them.
fn app() -> OpenDocApp {
    let mut app = OpenDocApp::new_empty_document();
    app.new_document("Spreadsheet");
    let sheet = &mut app.workbook.sheets[0];
    sheet.frozen_rows = 1;
    sheet.cells = vec![
        opendoc_spreadsheet::Cell::new("A1", "string", "Item"),
        opendoc_spreadsheet::Cell::new("B1", "string", "Count"),
        opendoc_spreadsheet::Cell::new("A2", "string", "Apples"),
        opendoc_spreadsheet::Cell::new("B2", "number", "5"),
        opendoc_spreadsheet::Cell::new("A3", "string", "Total"),
        opendoc_spreadsheet::Cell::new("B3", "formula", "=SUM(B2:B2)"),
    ];
    app.workbook = app.workbook.clone().evaluated();
    app.invalidate_source_state();
    app
}

fn user_value(app: &OpenDocApp, address: &str) -> String {
    app.workbook.sheets[0]
        .cells
        .iter()
        .find(|cell| cell.address == address)
        .map(|cell| cell.user_value.clone())
        .unwrap_or_default()
}

fn row_count(app: &OpenDocApp) -> usize {
    app.workbook.sheets[0].rows.len()
}

fn column_count(app: &OpenDocApp) -> usize {
    app.workbook.sheets[0].columns.len()
}

#[test]
fn add_row_after_selection_inserts_below_the_focus_row() {
    let mut app = app();
    let rows_before = row_count(&app);

    // The toolbar `＋row` button dispatches exactly this command.
    app.dispatch_command(
        "add_spreadsheet_row_after_selection",
        json!({ "sheetId": "sheet-1", "anchor": "A1", "focus": "A1" }),
    )
    .unwrap();

    assert_eq!(row_count(&app), rows_before + 1);
    // A blank row 2 opened up and the old rows moved down.
    assert_eq!(user_value(&app, "A1"), "Item");
    assert_eq!(user_value(&app, "A2"), "");
    assert_eq!(user_value(&app, "A3"), "Apples");
    assert_eq!(user_value(&app, "A4"), "Total");
    assert_eq!(user_value(&app, "B4"), "=SUM(B3:B3)");
    app.workbook.validate_source().unwrap();
}

#[test]
fn add_column_after_selection_inserts_right_of_the_focus_column() {
    let mut app = app();
    let columns_before = column_count(&app);

    app.dispatch_command(
        "add_spreadsheet_column_after_selection",
        json!({ "sheetId": "sheet-1", "anchor": "A1", "focus": "A1" }),
    )
    .unwrap();

    assert_eq!(column_count(&app), columns_before + 1);
    assert_eq!(user_value(&app, "A1"), "Item");
    assert_eq!(user_value(&app, "B1"), "");
    assert_eq!(user_value(&app, "C1"), "Count");
    app.workbook.validate_source().unwrap();
}

#[test]
fn delete_selection_row_shifts_the_following_rows_up() {
    let mut app = app();
    let rows_before = row_count(&app);

    app.dispatch_command(
        "delete_spreadsheet_selection_row",
        json!({ "sheetId": "sheet-1", "anchor": "A2", "focus": "A2" }),
    )
    .unwrap();

    assert_eq!(row_count(&app), rows_before - 1);
    // No gap is left behind: "Total" moved up from row 3 to row 2.
    assert_eq!(user_value(&app, "A2"), "Total");
    assert_eq!(
        app.workbook.sheets[0].rows[..3],
        ["1".to_string(), "2".to_string(), "3".to_string()]
    );
    app.workbook.validate_source().unwrap();
}

#[test]
fn delete_selection_column_shifts_the_following_columns_left() {
    let mut app = app();
    app.set_spreadsheet_cell_in_sheet("sheet-1", "C1", "Note")
        .unwrap();
    let columns_before = column_count(&app);

    app.dispatch_command(
        "delete_spreadsheet_selection_column",
        json!({ "sheetId": "sheet-1", "anchor": "B1", "focus": "B1" }),
    )
    .unwrap();

    assert_eq!(column_count(&app), columns_before - 1);
    assert_eq!(user_value(&app, "B1"), "Note");
    app.workbook.validate_source().unwrap();
}

#[test]
fn positional_insert_and_delete_are_undoable() {
    let mut app = app();
    let before = app.workbook.clone();

    app.dispatch_command(
        "add_spreadsheet_row_after_selection",
        json!({ "sheetId": "sheet-1", "anchor": "A1", "focus": "A1" }),
    )
    .unwrap();
    assert_ne!(
        app.workbook.sheets[0].rows.len(),
        before.sheets[0].rows.len()
    );

    app.undo_current_edit().unwrap();
    assert_eq!(app.workbook.sheets[0].rows, before.sheets[0].rows);
    assert_eq!(user_value(&app, "A2"), "Apples");
}

#[test]
fn deleted_row_can_be_restored_into_its_original_position() {
    let mut app = app();
    app.set_spreadsheet_row_height("sheet-1", "2", 48).unwrap();

    app.delete_spreadsheet_row("sheet-1", "2").unwrap();
    assert_eq!(user_value(&app, "A2"), "Total");

    app.restore_spreadsheet_row("sheet-1", "2").unwrap();
    assert_eq!(user_value(&app, "A2"), "Apples");
    assert_eq!(user_value(&app, "A3"), "Total");
    assert_eq!(app.workbook.row_height("sheet-1", "2"), Some(48));
    app.workbook.validate_source().unwrap();
}

#[test]
fn set_row_height_and_column_width_dispatch_and_persist() {
    let mut app = app();

    app.dispatch_command(
        "set_spreadsheet_row_height",
        json!({ "sheetId": "sheet-1", "row": "2", "height": 48 }),
    )
    .unwrap();
    app.dispatch_command(
        "set_spreadsheet_column_width",
        json!({ "sheetId": "sheet-1", "column": "B", "width": 220 }),
    )
    .unwrap();

    assert_eq!(app.workbook.sheets[0].row_heights.get("2"), Some(&48));
    assert_eq!(app.workbook.sheets[0].column_widths.get("B"), Some(&220));
    // Unsized axes project the default.
    assert_eq!(
        app.workbook.row_height("sheet-1", "1"),
        Some(opendoc_spreadsheet::DEFAULT_ROW_HEIGHT_PX)
    );
    assert_eq!(
        app.workbook.column_width("sheet-1", "A"),
        Some(opendoc_spreadsheet::DEFAULT_COLUMN_WIDTH_PX)
    );
    // The sizes reach the projected document DTO the UI reads.
    let document = app.document();
    assert_eq!(document.workbook.sheets[0].row_heights.get("2"), Some(&48));
    assert_eq!(
        document.workbook.sheets[0].column_widths.get("B"),
        Some(&220)
    );
    app.workbook.validate_source().unwrap();
}

#[test]
fn sizing_commands_are_undoable_and_redoable() {
    let mut app = app();

    app.dispatch_command(
        "set_spreadsheet_row_height",
        json!({ "sheetId": "sheet-1", "row": "2", "height": 48 }),
    )
    .unwrap();
    assert_eq!(app.workbook.row_height("sheet-1", "2"), Some(48));

    app.undo_current_edit().unwrap();
    assert_eq!(
        app.workbook.row_height("sheet-1", "2"),
        Some(opendoc_spreadsheet::DEFAULT_ROW_HEIGHT_PX)
    );

    app.redo_current_edit().unwrap();
    assert_eq!(app.workbook.row_height("sheet-1", "2"), Some(48));
}

#[test]
fn sizing_is_journalled_and_replays_onto_a_fresh_workbook() {
    let mut app = app();
    app.set_spreadsheet_row_height("sheet-1", "3", 60).unwrap();
    app.set_spreadsheet_column_width("sheet-1", "C", 180)
        .unwrap();
    // A later structural edit must move the sizes with their axes.
    app.add_spreadsheet_row("sheet-1", "1").unwrap();

    assert!(app
        .operation_journal
        .iter()
        .any(|record| record.kind == "set-spreadsheet-row-height"));
    assert!(app
        .operation_journal
        .iter()
        .any(|record| record.kind == "set-spreadsheet-column-width"));

    let mut replayed = AppSpreadsheetWorkbook::sample();
    let warnings = apply_spreadsheet_envelopes(&mut replayed, &app.operation_envelopes).unwrap();

    assert!(
        warnings.is_empty(),
        "unexpected replay warnings: {warnings:?}"
    );
    assert_eq!(replayed.row_height("sheet-1", "4"), Some(60));
    assert_eq!(replayed.column_width("sheet-1", "C"), Some(180));
    assert_eq!(
        replayed.sheets[0].row_heights,
        app.workbook.sheets[0].row_heights
    );
    assert_eq!(
        replayed.sheets[0].column_widths,
        app.workbook.sheets[0].column_widths
    );
}

#[test]
fn clearing_a_size_restores_the_default() {
    let mut app = app();
    app.set_spreadsheet_row_height("sheet-1", "2", 48).unwrap();

    app.dispatch_command(
        "set_spreadsheet_row_height",
        json!({ "sheetId": "sheet-1", "row": "2", "height": 0 }),
    )
    .unwrap();

    assert!(app.workbook.sheets[0].row_heights.is_empty());
    assert_eq!(
        app.workbook.row_height("sheet-1", "2"),
        Some(opendoc_spreadsheet::DEFAULT_ROW_HEIGHT_PX)
    );
}

#[test]
fn out_of_range_and_missing_axis_sizes_are_rejected() {
    let mut app = app();

    assert!(app
        .dispatch_command(
            "set_spreadsheet_row_height",
            json!({ "sheetId": "sheet-1", "row": "2", "height": 5000 }),
        )
        .is_err());
    assert!(app
        .dispatch_command(
            "set_spreadsheet_column_width",
            json!({ "sheetId": "sheet-1", "column": "ZZ", "width": 120 }),
        )
        .is_err());
    assert!(app.workbook.sheets[0].row_heights.is_empty());
    assert!(app.workbook.sheets[0].column_widths.is_empty());
}

// ---- Hidden rows and columns (SH-31) ---------------------------------------

#[test]
fn hiding_and_revealing_a_selection_covers_every_axis_it_spans() {
    let mut app = app();
    app.set_spreadsheet_row_height("sheet-1", "2", 48).unwrap();

    app.dispatch_command(
        "set_spreadsheet_selection_rows_hidden",
        json!({ "sheetId": "sheet-1", "anchor": "A2", "focus": "B4", "hidden": true }),
    )
    .unwrap();
    app.dispatch_command(
        "set_spreadsheet_selection_columns_hidden",
        json!({ "sheetId": "sheet-1", "anchor": "B1", "focus": "C1", "hidden": true }),
    )
    .unwrap();

    assert_eq!(app.workbook.sheets[0].hidden_rows, vec!["2", "3", "4"]);
    assert_eq!(app.workbook.sheets[0].hidden_columns, vec!["B", "C"]);
    // Hiding is visibility, not a size: the row keeps the height it was given.
    assert_eq!(app.workbook.row_height("sheet-1", "2"), Some(48));
    assert_eq!(app.workbook.row_hidden("sheet-1", "2"), Some(true));
    assert_eq!(app.workbook.row_hidden("sheet-1", "1"), Some(false));
    assert_eq!(app.workbook.column_hidden("sheet-1", "B"), Some(true));
    // The projection the grid reads carries it.
    let document = app.document();
    assert_eq!(document.workbook.sheets[0].hidden_rows, vec!["2", "3", "4"]);
    assert_eq!(document.workbook.sheets[0].hidden_columns, vec!["B", "C"]);

    // Unhiding works by selecting across the gap: a hidden row is still inside
    // a range that spans it, which is the only gesture that can reach a row
    // nothing draws.
    app.dispatch_command(
        "set_spreadsheet_selection_rows_hidden",
        json!({ "sheetId": "sheet-1", "anchor": "A1", "focus": "A5", "hidden": false }),
    )
    .unwrap();
    assert!(app.workbook.sheets[0].hidden_rows.is_empty());
    assert_eq!(app.workbook.row_height("sheet-1", "2"), Some(48));
    app.workbook.validate_source().unwrap();
}

#[test]
fn hiding_is_undoable_and_replays_onto_a_fresh_workbook() {
    let mut app = app();
    app.dispatch_command(
        "set_spreadsheet_selection_rows_hidden",
        json!({ "sheetId": "sheet-1", "anchor": "A3", "focus": "A3", "hidden": true }),
    )
    .unwrap();
    app.dispatch_command(
        "set_spreadsheet_selection_columns_hidden",
        json!({ "sheetId": "sheet-1", "anchor": "C1", "focus": "C1", "hidden": true }),
    )
    .unwrap();
    assert_eq!(app.workbook.sheets[0].hidden_rows, vec!["3"]);

    app.undo_current_edit().unwrap();
    assert!(app.workbook.sheets[0].hidden_columns.is_empty());
    app.redo_current_edit().unwrap();
    assert_eq!(app.workbook.sheets[0].hidden_columns, vec!["C"]);

    assert!(app
        .operation_journal
        .iter()
        .any(|record| record.kind == "set-spreadsheet-row-hidden"));
    assert!(app
        .operation_journal
        .iter()
        .any(|record| record.kind == "set-spreadsheet-column-hidden"));

    let mut replayed = AppSpreadsheetWorkbook::sample();
    let warnings = apply_spreadsheet_envelopes(&mut replayed, &app.operation_envelopes).unwrap();
    assert!(
        warnings.is_empty(),
        "unexpected replay warnings: {warnings:?}"
    );
    assert_eq!(replayed.sheets[0].hidden_rows, vec!["3"]);
    assert_eq!(replayed.sheets[0].hidden_columns, vec!["C"]);
}

#[test]
fn hidden_rows_move_with_a_structural_edit_and_reject_missing_axes() {
    let mut app = app();
    app.dispatch_command(
        "set_spreadsheet_selection_rows_hidden",
        json!({ "sheetId": "sheet-1", "anchor": "A3", "focus": "A3", "hidden": true }),
    )
    .unwrap();
    // Inserting above shifts the hidden row down with everything else.
    app.add_spreadsheet_row("sheet-1", "1").unwrap();
    assert_eq!(app.workbook.sheets[0].hidden_rows, vec!["4"]);

    assert!(app
        .dispatch_command(
            "set_spreadsheet_selection_columns_hidden",
            json!({ "sheetId": "sheet-1", "anchor": "A1", "focus": "ZZ1", "hidden": true }),
        )
        .is_err());
    assert!(app.workbook.sheets[0].hidden_columns.is_empty());
}

// ---- Spreadsheet mutations are transactional --------------------------------

/// A spreadsheet mutation that fails partway through leaves the workbook
/// exactly as it found it.
///
/// `set_spreadsheet_selection_format` walks the selection cell by cell, and
/// `set_cell_format` creates the cell it is about to format *before* it
/// validates what it was asked to write. Applied in place, a rejected format
/// therefore used to leave a new empty cell behind for the address it reached
/// first and then report a failure. The hide commands used to defend
/// themselves against the same shape with a pre-check of every axis; that
/// pre-check is gone, because staging the mutation covers every command
/// rather than the two that remembered to ask.
#[test]
fn a_rejected_selection_format_leaves_no_trace_on_the_workbook() {
    let mut app = app();
    let before = app.workbook.clone();
    let signatures = app.signatures.len();

    // D1:E2 holds no cells yet, so the first address the loop touches is one
    // `set_cell_format` has to create before it rejects the value.
    let error = app.dispatch_command(
        "set_spreadsheet_selection_format",
        json!({
            "sheetId": "sheet-1",
            "anchor": "D1",
            "focus": "E2",
            "property": "horizontal_align",
            "value": "sideways",
        }),
    );
    assert!(error.is_err(), "an unsupported alignment must be refused");
    assert_eq!(
        app.workbook, before,
        "the refused format changed the workbook"
    );
    assert!(
        !app.workbook.sheets[0]
            .cells
            .iter()
            .any(|cell| cell.address == "D1"),
        "the refused format left a cell behind"
    );
    assert_eq!(
        app.signatures.len(),
        signatures,
        "a mutation that changed nothing invalidated the signatures"
    );
}

/// The same guarantee for the axis commands, which is what their removed
/// pre-check used to provide on its own.
#[test]
fn a_hide_running_past_the_last_row_hides_nothing_at_all() {
    let mut app = app();
    let rows = app.workbook.sheets[0].rows.len();
    let before = app.workbook.clone();

    // The selection starts inside the sheet and runs off the end of it.
    let error = app.dispatch_command(
        "set_spreadsheet_selection_rows_hidden",
        json!({
            "sheetId": "sheet-1",
            "anchor": format!("A{}", rows - 1),
            "focus": format!("A{}", rows + 5),
            "hidden": true,
        }),
    );
    assert!(
        error.is_err(),
        "a row the sheet does not have is not hidden"
    );
    assert_eq!(app.workbook, before, "the rows it did reach stayed hidden");
    assert!(app.workbook.sheets[0].hidden_rows.is_empty());
    // And nothing was journalled, so no replica replays a half edit.
    assert!(!app
        .operation_journal
        .iter()
        .any(|record| record.kind == "set-spreadsheet-row-hidden"));
}

// ---- Harvested spreadsheet features (PLAN77 A1-A5) --------------------------

fn sheet_value(app: &OpenDocApp, address: &str) -> String {
    user_value(app, address)
}

#[test]
fn sort_range_dispatches_is_journalled_and_replays() {
    let mut app = app();
    // sample() is A1 "Item" / B1 "Count", A2 "Apples" / B2 5, A3 "Total".
    app.set_spreadsheet_cell_in_sheet("sheet-1", "A3", "Zucchini")
        .unwrap();
    app.set_spreadsheet_cell_in_sheet("sheet-1", "B3", "1")
        .unwrap();

    app.dispatch_command(
        "sort_spreadsheet_range",
        json!({
            "sheetId": "sheet-1",
            "range": "A1:B3",
            "column": "B",
            "descending": false,
            "hasHeader": true,
        }),
    )
    .unwrap();

    assert_eq!(sheet_value(&app, "A2"), "Zucchini");
    assert_eq!(sheet_value(&app, "A3"), "Apples");
    assert!(app
        .operation_journal
        .iter()
        .any(|record| record.kind == "sort-spreadsheet-range"));

    let mut replayed = AppSpreadsheetWorkbook::sample();
    let warnings = apply_spreadsheet_envelopes(&mut replayed, &app.operation_envelopes).unwrap();
    assert!(
        warnings.is_empty(),
        "unexpected replay warnings: {warnings:?}"
    );
    assert_eq!(replayed.sheets[0].cells, app.workbook.sheets[0].cells);

    app.undo_current_edit().unwrap();
    assert_eq!(sheet_value(&app, "A2"), "Apples");
    app.workbook.validate_source().unwrap();
}

#[test]
fn fill_range_extends_a_series_and_is_undoable_and_replayable() {
    let mut app = app();
    app.set_spreadsheet_cell_in_sheet("sheet-1", "D1", "2")
        .unwrap();
    app.set_spreadsheet_cell_in_sheet("sheet-1", "D2", "4")
        .unwrap();

    app.dispatch_command(
        "fill_spreadsheet_range",
        json!({ "sheetId": "sheet-1", "sourceRange": "D1:D2", "targetRange": "D1:D4" }),
    )
    .unwrap();

    assert_eq!(sheet_value(&app, "D3"), "6");
    assert_eq!(sheet_value(&app, "D4"), "8");
    assert!(app
        .operation_journal
        .iter()
        .any(|record| record.kind == "fill-spreadsheet-range"));

    let mut replayed = AppSpreadsheetWorkbook::sample();
    let warnings = apply_spreadsheet_envelopes(&mut replayed, &app.operation_envelopes).unwrap();
    assert!(
        warnings.is_empty(),
        "unexpected replay warnings: {warnings:?}"
    );
    assert_eq!(replayed.sheets[0].cells, app.workbook.sheets[0].cells);

    app.undo_current_edit().unwrap();
    assert_eq!(sheet_value(&app, "D3"), "");
    app.redo_current_edit().unwrap();
    assert_eq!(sheet_value(&app, "D3"), "6");
    app.workbook.validate_source().unwrap();
}

#[test]
fn fill_range_rejects_a_target_that_does_not_line_up() {
    let mut app = app();

    assert!(app
        .dispatch_command(
            "fill_spreadsheet_range",
            json!({ "sheetId": "sheet-1", "sourceRange": "D1:D2", "targetRange": "D1:F4" }),
        )
        .is_err());
    // A rejected fill leaves nothing behind.
    assert!(!app
        .operation_journal
        .iter()
        .any(|record| record.kind == "fill-spreadsheet-range"));
}

#[test]
fn pasting_within_the_workbook_shifts_formulas_and_pasting_from_outside_does_not() {
    let mut app = app();
    // B3 holds `=SUM(B2:B2)` in the sample workbook.
    let copied = app
        .copy_spreadsheet_selection_tsv("sheet-1", "B3", "B3")
        .unwrap();
    assert_eq!(copied, "=SUM(B2:B2)");

    app.dispatch_command(
        "paste_spreadsheet_tsv",
        json!({ "sheetId": "sheet-1", "origin": "D5", "text": copied, "sourceOrigin": "B3" }),
    )
    .unwrap();
    assert_eq!(sheet_value(&app, "D5"), "=SUM(D4:D4)");

    app.dispatch_command(
        "paste_spreadsheet_tsv",
        json!({ "sheetId": "sheet-1", "origin": "E5", "text": copied }),
    )
    .unwrap();
    assert_eq!(sheet_value(&app, "E5"), "=SUM(B2:B2)");
    app.workbook.validate_source().unwrap();
}

#[test]
fn csv_imports_at_the_selection_and_exports_display_text() {
    let mut app = app();

    app.dispatch_command(
        "import_spreadsheet_csv",
        json!({
            "sheetId": "sheet-1",
            "origin": "D1",
            "text": "a,b\n1,2\n",
            "delimiter": ",",
        }),
    )
    .unwrap();

    assert_eq!(sheet_value(&app, "D1"), "a");
    assert_eq!(sheet_value(&app, "E2"), "2");
    // Imported cells journal as ordinary cell edits, so they replay and undo.
    let mut replayed = AppSpreadsheetWorkbook::sample();
    let warnings = apply_spreadsheet_envelopes(&mut replayed, &app.operation_envelopes).unwrap();
    assert!(
        warnings.is_empty(),
        "unexpected replay warnings: {warnings:?}"
    );
    assert_eq!(replayed.sheets[0].cells, app.workbook.sheets[0].cells);

    let result = app
        .dispatch_command("export_spreadsheet_csv", json!({ "sheetId": "sheet-1" }))
        .unwrap();
    let AppCommandResult::Export(export) = result else {
        panic!("export_spreadsheet_csv returns an export");
    };
    let csv = export.content;
    // Column C is untouched, so the imported block sits in D and E.
    assert!(csv.starts_with("Item,Count,,a,b\n"), "{csv}");

    app.undo_current_edit().unwrap();
    assert_eq!(sheet_value(&app, "D1"), "");
}

#[test]
fn xlsx_round_trips_through_the_commands_and_is_guarded_and_undoable() {
    let mut app = app();
    app.set_spreadsheet_cell_in_sheet("sheet-1", "C1", "Note")
        .unwrap();
    let result = app
        .dispatch_command("export_spreadsheet_xlsx", json!({}))
        .unwrap();
    let AppCommandResult::Export(export) = result else {
        panic!("export_spreadsheet_xlsx returns an export");
    };
    let base64 = export.content;

    // Replacing the workbook is guarded like every other replacing command.
    let guarded = app.dispatch_command(
        "import_spreadsheet_xlsx",
        json!({ "title": "Imported", "base64": base64 }),
    );
    assert!(
        matches!(guarded, Err(AppApiError::UnsavedChanges(_))),
        "expected an unsaved-changes refusal, got {guarded:?}"
    );

    app.dispatch_command(
        "import_spreadsheet_xlsx",
        json!({
            "title": "Imported",
            "base64": base64,
            "discardUnsavedChanges": true,
        }),
    )
    .unwrap();

    assert_eq!(app.workbook.title, "Imported");
    assert_eq!(sheet_value(&app, "C1"), "Note");
    assert_eq!(sheet_value(&app, "B3"), "=SUM(B2:B2)");
    app.workbook.validate_source().unwrap();

    // And an accidental import is recoverable.
    app.undo_current_edit().unwrap();
    assert_ne!(app.workbook.title, "Imported");
}

#[test]
fn spreadsheet_pdf_export_is_a_typed_pdf_of_evaluated_cells() {
    let mut app = app();
    let AppCommandResult::Export(export) = app
        .dispatch_command("export_spreadsheet_pdf", json!({}))
        .expect("spreadsheet PDF export")
    else {
        panic!("export_spreadsheet_pdf returns an export");
    };
    assert_eq!(export.encoding, AppExportEncoding::Base64);
    assert_eq!(export.media_type, "application/pdf");
    assert_eq!(export.file_extension, "pdf");
    let bytes = base64_decode(&export.content).expect("PDF is base64");
    assert!(bytes.starts_with(b"%PDF-"), "not a PDF");
    let operators = String::from_utf8_lossy(&bytes);
    assert!(
        operators.contains("Apples"),
        "cell not rendered: {operators}"
    );
    assert!(
        operators.contains("Total"),
        "cell not rendered: {operators}"
    );
    assert!(
        operators.contains("5"),
        "formula result not rendered: {operators}"
    );
}

#[test]
fn every_journalled_spreadsheet_envelope_carries_the_kind_of_its_payload() {
    let mut app = app();
    app.set_spreadsheet_cell_in_sheet("sheet-1", "D1", "1")
        .unwrap();
    app.set_spreadsheet_cell_in_sheet("sheet-1", "D2", "2")
        .unwrap();
    app.set_spreadsheet_cell_format("sheet-1", "D1", "bold", "true")
        .unwrap();
    app.fill_spreadsheet_range("sheet-1", "D1:D2", "D1:D4")
        .unwrap();
    app.sort_spreadsheet_range("sheet-1", "D1:D4", "D", true, false)
        .unwrap();
    app.add_spreadsheet_sheet("Second").unwrap();
    app.set_spreadsheet_row_height("sheet-1", "2", 40).unwrap();

    // `validate_operation_envelopes` compares every envelope's record kind
    // against the kind its payload derives, so a hand-written kind that drifts
    // from its payload fails here.
    crate::operation::validate_operation_envelopes(&app.operation_envelopes).unwrap();
    assert!(app
        .operation_envelopes
        .iter()
        .filter_map(|envelope| envelope.spreadsheet.as_ref().map(|op| (envelope, op)))
        .all(|(envelope, operation)| envelope.record.kind == operation.operation_kind()));
}

/// An import replaces the whole workbook, and every replay path has to be
/// able to reproduce that.
///
/// It could not: the import journalled a bare app envelope with no
/// spreadsheet payload, and `merge_spreadsheet_envelope_streams` skips
/// exactly those (`if envelope.spreadsheet.is_none() { continue; }`). Crash
/// recovery replays from the segment's *base* workbook and repository merge
/// from the merge base, so both reproduced the **pre-import** workbook — the
/// edits the user made before importing, silently resurrected over the file
/// they imported.
#[test]
fn an_xlsx_import_is_reproduced_by_replay() {
    let mut app = app();
    app.set_spreadsheet_cell_in_sheet("sheet-1", "C1", "imported")
        .unwrap();
    let AppCommandResult::Export(export) = app
        .dispatch_command("export_spreadsheet_xlsx", json!({}))
        .unwrap()
    else {
        panic!("export_spreadsheet_xlsx returns an export");
    };
    let base64 = export.content;

    // Diverge from the exported file, so "the import happened" and "the
    // import did not happen" are distinguishable states.
    app.set_spreadsheet_cell_in_sheet("sheet-1", "C1", "overwritten")
        .unwrap();
    app.set_spreadsheet_cell_in_sheet("sheet-1", "D1", "only before the import")
        .unwrap();

    app.dispatch_command(
        "import_spreadsheet_xlsx",
        json!({
            "title": "Imported",
            "base64": base64,
            "discardUnsavedChanges": true,
        }),
    )
    .unwrap();
    assert_eq!(sheet_value(&app, "C1"), "imported");
    assert_eq!(sheet_value(&app, "D1"), "");

    // The same journal, replayed from the genesis workbook, must land on the
    // same cells. `sample()` is the genesis the `app()` fixture is built to
    // match.
    let (replayed, warnings) = crate::spreadsheet_replay::merge_spreadsheet_envelope_streams(
        AppSpreadsheetWorkbook::sample(),
        &[app.operation_envelopes.as_slice()],
    )
    .unwrap();
    assert!(
        warnings.is_empty(),
        "unexpected replay warnings: {warnings:?}"
    );
    let replayed_value = |address: &str| {
        replayed.sheets[0]
            .cells
            .iter()
            .find(|cell| cell.address == address)
            .map(|cell| cell.user_value.clone())
            .unwrap_or_default()
    };
    assert_eq!(
        replayed_value("C1"),
        "imported",
        "replay resurrected the pre-import value"
    );
    assert_eq!(
        replayed_value("D1"),
        "",
        "replay resurrected a cell the import replaced away"
    );
    assert_eq!(replayed.title, "Imported");
}

/// Replay order is `(actor, seq)` as a **pair of typed values**.
///
/// It used to be `format!("{actor}:{seq}")`, keyed into a `BTreeMap` — so
/// `"alice:10"` sorted before `"alice:2"`, the tenth edit of a cell replayed
/// before the second, and the cell came back holding whichever value was
/// written *earliest* past the ninth. Ten edits to one cell is all it takes.
#[test]
fn spreadsheet_replay_orders_sequence_numbers_numerically() {
    let mut envelopes = Vec::new();
    for seq in 1..=12u64 {
        envelopes.push(AppOperationEnvelope {
            record: AppOperationRecord {
                actor: "alice".to_string(),
                seq,
                kind: "set-spreadsheet-cell".to_string(),
                summary: format!("set A1 to {seq}"),
                created_at_ms: 1_700_000_000_000 + seq,
            },
            operation: None,
            spreadsheet: Some(AppSpreadsheetOperation::SetCell {
                sheet_id: "sheet-1".to_string(),
                address: "A1".to_string(),
                value: seq.to_string(),
            }),
            blob: None,
        });
    }

    let (replayed, warnings) = crate::spreadsheet_replay::merge_spreadsheet_envelope_streams(
        AppSpreadsheetWorkbook::sample(),
        &[envelopes.as_slice()],
    )
    .unwrap();
    assert!(
        warnings.is_empty(),
        "unexpected replay warnings: {warnings:?}"
    );
    assert_eq!(
        replayed.sheets[0]
            .cells
            .iter()
            .find(|cell| cell.address == "A1")
            .map(|cell| cell.user_value.clone())
            .unwrap_or_default(),
        "12",
        "the last edit did not win; the order was lexicographic"
    );
}

/// The scenario the audit drove: a strict rule, then a write that breaks it.
#[test]
fn a_strict_cell_validation_refuses_the_write_through_the_command_surface() {
    let mut app = app();
    app.dispatch_command(
        "set_spreadsheet_cell_validation",
        json!({
            "sheetId": "sheet-1",
            "address": "D1",
            "kind": "number_greater",
            "values": ["10"],
            "strict": true,
        }),
    )
    .unwrap();

    let refused = app.dispatch_command(
        "set_spreadsheet_cell_in_sheet",
        json!({ "sheetId": "sheet-1", "address": "D1", "value": "1" }),
    );
    assert!(
        refused.is_err(),
        "a strict rule accepted 1 > 10: {refused:?}"
    );
    assert_eq!(sheet_value(&app, "D1"), "");

    app.dispatch_command(
        "set_spreadsheet_cell_in_sheet",
        json!({ "sheetId": "sheet-1", "address": "D1", "value": "11" }),
    )
    .unwrap();
    assert_eq!(sheet_value(&app, "D1"), "11");
}

/// The other scenario: a merge, then a write into a cell it covers. The value
/// was stored and never drawn — invisible, uneditable, and signed into the
/// document all the same.
#[test]
fn a_cell_a_merge_covers_is_refused_through_the_command_surface() {
    let mut app = app();
    app.dispatch_command(
        "merge_spreadsheet_cells",
        json!({ "sheetId": "sheet-1", "range": "B2:C3" }),
    )
    .unwrap();

    let refused = app.dispatch_command(
        "set_spreadsheet_cell_in_sheet",
        json!({ "sheetId": "sheet-1", "address": "C2", "value": "999" }),
    );
    assert!(refused.is_err(), "C2 is covered by B2:C3: {refused:?}");
    assert_eq!(sheet_value(&app, "C2"), "");

    app.dispatch_command(
        "set_spreadsheet_cell_in_sheet",
        json!({ "sheetId": "sheet-1", "address": "B2", "value": "999" }),
    )
    .unwrap();
    assert_eq!(sheet_value(&app, "B2"), "999");
}

/// A workbook holding one of everything a CSV file and an XLSX file can lose.
fn workbook_with_everything_a_flat_file_loses() -> OpenDocApp {
    let mut app = app();
    app.dispatch_command(
        "add_spreadsheet_sheet",
        json!({ "title": "Second", "sheetId": "sheet-2" }),
    )
    .expect("a second sheet");
    let sheet = &mut app.workbook.sheets[0];
    sheet.merges.push(opendoc_spreadsheet::SheetMerge {
        id: "merge-1".to_string(),
        range: "A1:B1".to_string(),
    });
    sheet.filters.push(opendoc_spreadsheet::SheetFilter {
        id: "filter-1".to_string(),
        range: "A1:B3".to_string(),
        criteria: Vec::new(),
        sort_specs: Vec::new(),
    });
    sheet
        .protected_ranges
        .push(opendoc_spreadsheet::SheetProtectedRange {
            id: "protected-1".to_string(),
            range: "A1:A3".to_string(),
            description: "locked".to_string(),
            warning_only: false,
        });
    let cell = sheet
        .cells
        .iter_mut()
        .find(|cell| cell.address == "A2")
        .expect("A2");
    cell.format.bold = true;
    cell.comments.push(opendoc_spreadsheet::CellComment {
        id: "comment-1".to_string(),
        author: "Reviewer".to_string(),
        body: "check this".to_string(),
        deleted: false,
    });
    cell.validation = Some(opendoc_spreadsheet::CellValidation {
        kind: "list".to_string(),
        values: vec!["Apples".to_string(), "Pears".to_string()],
        strict: true,
        show_dropdown: true,
    });
    app.workbook = app.workbook.clone().evaluated();
    app
}

fn export_codes(app: &OpenDocApp, command: &str, args: serde_json::Value) -> Vec<String> {
    let AppCommandResult::Export(export) = app.clone().dispatch_command(command, args).unwrap()
    else {
        panic!("{command} returns an export");
    };
    export
        .warnings
        .iter()
        .map(|warning| warning.code.clone())
        .collect()
}

/// A CSV export says what the grid of text left behind.
///
/// The old signature was `String`, so there was nowhere to say it: a workbook
/// with four sheets, formulas, formatting, merges, comments and validation
/// came out as one flat file with no indication that any of it was gone.
#[test]
fn a_csv_export_names_what_a_grid_of_text_could_not_carry() {
    let app = workbook_with_everything_a_flat_file_loses();
    let codes = export_codes(
        &app,
        "export_spreadsheet_csv",
        json!({ "sheetId": "sheet-1" }),
    );
    for expected in [
        "csv-export-single-sheet",
        "csv-export-dropped-formulas",
        "csv-export-dropped-cell-formatting",
        "csv-export-dropped-merged-cells",
        "csv-export-dropped-cell-comments",
        "csv-export-dropped-data-validation",
    ] {
        assert!(
            codes.iter().any(|code| code == expected),
            "the CSV export did not report {expected}; it reported {codes:?}"
        );
    }
}

/// The negative control: a workbook with nothing to lose loses nothing, and
/// the export says so by staying quiet.
///
/// Without this, an exporter that pushed all six warnings unconditionally
/// would pass the test above and put six lies in front of every user.
#[test]
fn a_csv_export_of_one_sheet_of_literal_text_warns_about_nothing() {
    let mut app = OpenDocApp::new_empty_document();
    app.new_document("Spreadsheet");
    app.workbook.sheets[0].cells = vec![
        opendoc_spreadsheet::Cell::new("A1", "string", "Item"),
        opendoc_spreadsheet::Cell::new("B1", "number", "5"),
    ];
    app.workbook = app.workbook.clone().evaluated();
    let codes = export_codes(
        &app,
        "export_spreadsheet_csv",
        json!({ "sheetId": "sheet-1" }),
    );
    assert!(
        codes.is_empty(),
        "a single sheet of literal text lost nothing, but the export claimed {codes:?}"
    );
}

/// A tab-delimited file is not a CSV file, and the export result is where that
/// is decided — the frontend used to hardcode `.csv` and `text/csv` for both.
#[test]
fn a_tab_delimited_sheet_export_is_a_tsv_file() {
    let mut app = app();
    let AppCommandResult::Export(comma) = app
        .dispatch_command(
            "export_spreadsheet_csv",
            json!({ "sheetId": "sheet-1", "delimiter": "," }),
        )
        .unwrap()
    else {
        panic!("an export");
    };
    assert_eq!(comma.file_extension, "csv");
    assert_eq!(comma.media_type, "text/csv;charset=utf-8");
    assert!(comma.content.contains("Item,Count"), "{}", comma.content);

    let AppCommandResult::Export(tab) = app
        .dispatch_command(
            "export_spreadsheet_csv",
            json!({ "sheetId": "sheet-1", "delimiter": "tab" }),
        )
        .unwrap()
    else {
        panic!("an export");
    };
    assert_eq!(tab.file_extension, "tsv");
    assert_eq!(tab.media_type, "text/tab-separated-values;charset=utf-8");
    assert!(tab.content.contains("Item\tCount"), "{}", tab.content);
}

/// The XLSX export names the model property its writer has no code for — and
/// the round trip proves it really is gone, so the warning is a finding rather
/// than a claim. A bare AutoFilter range is deliberately the control: it must
/// survive, and must not still be named as a dropped property.
///
/// If the writer ever learns to write one of them, the round-trip half of this
/// test fails and the warning it produces becomes a lie that someone has to
/// come and delete. That is the point.
#[test]
fn an_xlsx_export_names_the_properties_it_drops_and_really_drops_them() {
    let app = workbook_with_everything_a_flat_file_loses();
    let codes = export_codes(&app, "export_spreadsheet_xlsx", json!({}));
    let expected = "xlsx-export-dropped-protected-ranges";
    assert!(
        codes.iter().any(|code| code == expected),
        "the XLSX export did not report {expected}; it reported {codes:?}"
    );

    let AppCommandResult::Export(export) = app
        .clone()
        .dispatch_command("export_spreadsheet_xlsx", json!({}))
        .unwrap()
    else {
        panic!("an export");
    };
    let reread = AppSpreadsheetWorkbook::from_xlsx_base64(&export.content, "Reread")
        .expect("the file OpenDoc just wrote reads back");
    assert!(
        reread
            .sheets
            .iter()
            .any(|sheet| sheet.filters.iter().any(|filter| filter.range == "A1:B3")),
        "the bare AutoFilter range did not survive the XLSX round trip"
    );
    assert!(
        reread
            .sheets
            .iter()
            .all(|sheet| sheet.protected_ranges.is_empty()),
        "a protected range survived the file, so the warning is wrong"
    );
    assert!(
        reread
            .sheets
            .iter()
            .flat_map(|sheet| sheet.cells.iter())
            .any(|cell| cell.validation.as_ref().is_some_and(|rule| {
                rule.kind == "list"
                    && rule.values == ["Apples", "Pears"]
                    && rule.strict
                    && rule.show_dropdown
            })),
        "the list validation did not survive the XLSX round trip"
    );
    assert!(
        reread
            .sheets
            .iter()
            .flat_map(|sheet| sheet.cells.iter())
            .any(|cell| cell.comments.iter().any(|comment| {
                comment.author == "Reviewer" && comment.body == "check this" && !comment.deleted
            })),
        "the legacy cell note did not survive the XLSX round trip"
    );
    // And the control: the things the writer *does* write did survive, so the
    // assertions above are about those declared losses and not about a file
    // that failed to carry anything.
    assert!(
        reread
            .sheets
            .iter()
            .flat_map(|sheet| sheet.cells.iter())
            .any(|cell| cell.user_kind == "formula"),
        "the round trip lost the formulas too, so it proves nothing"
    );
    assert!(
        reread.sheets.iter().any(|sheet| !sheet.merges.is_empty()),
        "the round trip lost the merges too, so it proves nothing"
    );
}

#[test]
fn an_xlsx_export_names_filter_options_the_writer_cannot_preserve() {
    let mut app = workbook_with_everything_a_flat_file_loses();
    app.workbook.sheets[0].filters[0].criteria = vec![opendoc_spreadsheet::SheetFilterCriterion {
        column: "A".to_string(),
        condition: "text_equals".to_string(),
        value: "Apples".to_string(),
    }];
    app.workbook.sheets[0].filters[0].sort_specs = vec![opendoc_spreadsheet::SheetFilterSortSpec {
        column: "B".to_string(),
        descending: true,
    }];
    let codes = export_codes(&app, "export_spreadsheet_xlsx", json!({}));
    assert!(
        codes
            .iter()
            .any(|code| code == "xlsx-export-dropped-filter-options"),
        "criteria/sort state vanished without a warning: {codes:?}"
    );

    let AppCommandResult::Export(export) = app
        .dispatch_command("export_spreadsheet_xlsx", json!({}))
        .expect("XLSX export")
    else {
        panic!("an export");
    };
    let reread = AppSpreadsheetWorkbook::from_xlsx_base64(&export.content, "Reread")
        .expect("the file OpenDoc just wrote reads back");
    let filter = &reread.sheets[0].filters[0];
    assert_eq!(
        filter.criteria,
        vec![opendoc_spreadsheet::SheetFilterCriterion {
            column: "A".to_string(),
            condition: "text_equals".to_string(),
            value: "Apples".to_string(),
        }]
    );
    assert!(filter.sort_specs.is_empty());
    assert_eq!("A1:B3", filter.range);
}

/// The negative control for the XLSX list: a workbook with none of those two
/// properties exports with no warnings at all.
#[test]
fn an_xlsx_export_of_a_plain_workbook_warns_about_nothing() {
    let app = app();
    let codes = export_codes(&app, "export_spreadsheet_xlsx", json!({}));
    assert!(
        codes.is_empty(),
        "this workbook has no filters, protected ranges, validation or cell comments, but the export claimed {codes:?}"
    );
}

#[test]
fn print_area_commands_are_undoable_replayable_and_stale_clear_safe() {
    let mut app = app();
    app.dispatch_command(
        "set_spreadsheet_print_area",
        json!({ "sheetId": "sheet-1", "range": "b3:a1" }),
    )
    .unwrap();
    assert_eq!(
        app.workbook.sheets[0].print_settings.print_area.as_deref(),
        Some("A1:B3")
    );
    app.dispatch_command(
        "clear_spreadsheet_print_area",
        json!({ "sheetId": "sheet-1" }),
    )
    .unwrap();
    assert_eq!(app.workbook.sheets[0].print_settings.print_area, None);
    let stale_clear = app.operation_envelopes.last().cloned().unwrap();

    app.undo_current_edit().unwrap();
    assert_eq!(
        app.workbook.sheets[0].print_settings.print_area.as_deref(),
        Some("A1:B3")
    );

    // A replacement made after the clear's author read A1:B3 must survive if
    // a merged/recovered log orders it before that stale clear.
    app.set_spreadsheet_print_area("sheet-1", "C1:C2").unwrap();
    let mut envelopes = app.operation_envelopes.clone();
    envelopes.push(stale_clear);
    let mut replayed = AppSpreadsheetWorkbook::sample();
    let warnings = apply_spreadsheet_envelopes(&mut replayed, &envelopes).unwrap();
    assert_eq!(
        replayed.sheets[0].print_settings.print_area.as_deref(),
        Some("C1:C2")
    );
    assert!(warnings
        .iter()
        .any(|warning| warning.code == "spreadsheet-print-area-conflict"));
}

#[test]
fn print_orientation_command_is_typed_undoable_and_replayable() {
    let mut app = app();
    app.dispatch_command(
        "set_spreadsheet_print_orientation",
        json!({ "sheetId": "sheet-1", "orientation": "portrait" }),
    )
    .unwrap();
    assert_eq!(
        app.workbook.sheets[0].print_settings.orientation,
        AppSheetPrintOrientation::Portrait
    );
    // A normal persisted command replays before undo deliberately removes it
    // from the live journal segment.
    let mut replayed = AppSpreadsheetWorkbook::sample();
    apply_spreadsheet_envelopes(&mut replayed, &app.operation_envelopes).unwrap();
    assert_eq!(
        replayed.sheets[0].print_settings.orientation,
        AppSheetPrintOrientation::Portrait
    );
    app.undo_current_edit().unwrap();
    assert_eq!(
        app.workbook.sheets[0].print_settings.orientation,
        AppSheetPrintOrientation::Landscape
    );
    let invalid = app.dispatch_command(
        "set_spreadsheet_print_orientation",
        json!({ "sheetId": "sheet-1", "orientation": "sideways" }),
    );
    assert!(matches!(invalid, Err(AppApiError::Format(_))));
}
