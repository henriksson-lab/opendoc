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
    let AppCommandResult::Text(csv) = result else {
        panic!("export_spreadsheet_csv returns text");
    };
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
    let AppCommandResult::Text(base64) = result else {
        panic!("export_spreadsheet_xlsx returns text");
    };

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
