//! Application-level tests for positional row/column insert and delete
//! (the `＋row` / `＋col` toolbar path) and for explicit row height /
//! column width sizing.

use super::*;
use crate::spreadsheet_replay::apply_spreadsheet_envelopes;
use serde_json::json;

fn app() -> OpenDocApp {
    let mut app = OpenDocApp::new_sample();
    app.new_document("Spreadsheet");
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
