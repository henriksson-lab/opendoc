//! Tests for positional row/column insert and delete, and for explicit
//! row height / column width sizing.

use super::*;

fn workbook() -> SpreadsheetWorkbook {
    let mut workbook = SpreadsheetWorkbook::sample();
    // sample() has A1..B3 with `=SUM(B2:B2)` in B3.
    workbook
        .set_cell_in_sheet("sheet-1", "A5", "Tail".to_string())
        .unwrap();
    workbook
}

fn user_value(workbook: &SpreadsheetWorkbook, address: &str) -> String {
    workbook.sheets[0]
        .cells
        .iter()
        .find(|cell| cell.address == address)
        .map(|cell| cell.user_value.clone())
        .unwrap_or_default()
}

#[test]
fn add_row_inserts_positionally_and_shifts_following_rows() {
    let mut workbook = workbook();
    let rows_before = workbook.sheets[0].rows.len();

    workbook.add_row("sheet-1", "2").unwrap();

    // A new blank row 2 exists; the old rows 2..n moved down by one.
    assert_eq!(workbook.sheets[0].rows.len(), rows_before + 1);
    assert_eq!(user_value(&workbook, "A1"), "Item");
    assert_eq!(user_value(&workbook, "A2"), "");
    assert_eq!(user_value(&workbook, "A3"), "Apples");
    assert_eq!(user_value(&workbook, "A4"), "Total");
    assert_eq!(user_value(&workbook, "A6"), "Tail");
    // The formula moved and its references followed the shifted cells.
    assert_eq!(user_value(&workbook, "B4"), "=SUM(B3:B3)");
    // Axis labels stay a dense 1..n run.
    assert_eq!(
        workbook.sheets[0].rows[..4],
        [
            "1".to_string(),
            "2".to_string(),
            "3".to_string(),
            "4".to_string()
        ]
    );
    workbook.validate_source().unwrap();
}

#[test]
fn add_row_past_the_end_appends() {
    let mut workbook = workbook();
    let rows_before = workbook.sheets[0].rows.len();
    let next = (rows_before + 1).to_string();

    workbook.add_row("sheet-1", &next).unwrap();

    assert_eq!(workbook.sheets[0].rows.len(), rows_before + 1);
    assert!(workbook.sheets[0].rows.contains(&next));
    assert_eq!(user_value(&workbook, "A5"), "Tail");
}

#[test]
fn add_column_inserts_positionally_and_shifts_following_columns() {
    let mut workbook = workbook();
    let columns_before = workbook.sheets[0].columns.len();

    workbook.add_column("sheet-1", "B").unwrap();

    assert_eq!(workbook.sheets[0].columns.len(), columns_before + 1);
    assert_eq!(user_value(&workbook, "A1"), "Item");
    assert_eq!(user_value(&workbook, "B1"), "");
    assert_eq!(user_value(&workbook, "C1"), "Count");
    assert_eq!(user_value(&workbook, "C3"), "=SUM(C2:C2)");
    workbook.validate_source().unwrap();
}

#[test]
fn delete_row_shifts_following_rows_up_without_leaving_a_gap() {
    let mut workbook = workbook();
    let rows_before = workbook.sheets[0].rows.len();

    assert_eq!(workbook.delete_row("sheet-1", "2"), Some(true));

    assert_eq!(workbook.sheets[0].rows.len(), rows_before - 1);
    // No gap: labels remain a dense 1..n run.
    assert_eq!(
        workbook.sheets[0].rows[..3],
        ["1".to_string(), "2".to_string(), "3".to_string()]
    );
    assert_eq!(user_value(&workbook, "A1"), "Item");
    assert_eq!(user_value(&workbook, "A2"), "Total");
    assert_eq!(user_value(&workbook, "A4"), "Tail");
    // The formula referenced the deleted row and becomes #REF!.
    assert_eq!(user_value(&workbook, "B2"), "=SUM(#REF!)");
    workbook.validate_source().unwrap();
}

#[test]
fn delete_column_shifts_following_columns_left() {
    let mut workbook = workbook();
    workbook
        .set_cell_in_sheet("sheet-1", "C1", "Note".to_string())
        .unwrap();
    let columns_before = workbook.sheets[0].columns.len();

    assert_eq!(workbook.delete_column("sheet-1", "B"), Some(true));

    assert_eq!(workbook.sheets[0].columns.len(), columns_before - 1);
    assert_eq!(
        workbook.sheets[0].columns[..3],
        ["A".to_string(), "B".to_string(), "C".to_string()]
    );
    assert_eq!(user_value(&workbook, "B1"), "Note");
    workbook.validate_source().unwrap();
}

#[test]
fn positional_insert_moves_merges_and_axis_metadata() {
    let mut workbook = workbook();
    workbook.merge_cells("sheet-1", "A5:B5").unwrap().unwrap();

    workbook.add_row("sheet-1", "3").unwrap();

    assert_eq!(workbook.sheets[0].merges[0].range, "A6:B6");
    assert_eq!(workbook.sheets[0].merges[0].id, "merge-a6-b6");
    // Axis ids follow their labels.
    assert!(workbook.sheets[0]
        .row_axes
        .iter()
        .all(|axis| axis.id == format!("row-{}", axis.label)));
    assert_eq!(
        workbook.sheets[0].row_axes.len(),
        workbook.sheets[0].rows.len()
    );
    workbook.validate_source().unwrap();
}

#[test]
fn positional_delete_moves_named_ranges_and_keeps_the_last_axis() {
    let mut workbook = workbook();
    workbook
        .add_named_range("sheet-1", "TAIL", "A5:A5")
        .unwrap()
        .unwrap();
    assert_eq!(workbook.named_ranges[0].range, "A5");

    assert_eq!(workbook.delete_row("sheet-1", "1"), Some(true));

    assert_eq!(workbook.named_ranges[0].range, "A4");
    workbook.validate_source().unwrap();
}

#[test]
fn explicit_sizes_default_when_unset() {
    let workbook = workbook();
    assert_eq!(
        workbook.row_height("sheet-1", "1"),
        Some(DEFAULT_ROW_HEIGHT_PX)
    );
    assert_eq!(
        workbook.column_width("sheet-1", "A"),
        Some(DEFAULT_COLUMN_WIDTH_PX)
    );
    assert!(workbook.sheets[0].row_heights.is_empty());
    assert!(workbook.sheets[0].column_widths.is_empty());
}

#[test]
fn set_row_height_and_column_width_store_explicit_sizes() {
    let mut workbook = workbook();

    workbook
        .set_row_height("sheet-1", "2", 48)
        .unwrap()
        .unwrap();
    workbook
        .set_column_width("sheet-1", "B", 220)
        .unwrap()
        .unwrap();

    assert_eq!(workbook.row_height("sheet-1", "2"), Some(48));
    assert_eq!(workbook.column_width("sheet-1", "B"), Some(220));
    assert_eq!(workbook.sheets[0].row_heights.get("2"), Some(&48));
    assert_eq!(workbook.sheets[0].column_widths.get("B"), Some(&220));
    workbook.validate_source().unwrap();

    // Zero clears the explicit size and falls back to the default.
    workbook.set_row_height("sheet-1", "2", 0).unwrap().unwrap();
    assert!(workbook.sheets[0].row_heights.is_empty());
    assert_eq!(
        workbook.row_height("sheet-1", "2"),
        Some(DEFAULT_ROW_HEIGHT_PX)
    );
}

#[test]
fn out_of_range_sizes_are_rejected() {
    let mut workbook = workbook();
    assert!(workbook
        .set_row_height("sheet-1", "2", MAX_AXIS_SIZE_PX + 1)
        .unwrap()
        .is_err());
    assert!(workbook.set_row_height("sheet-1", "999999", 40).is_none());
    assert!(workbook
        .set_column_width("sheet-1", "A", 0)
        .unwrap()
        .is_ok());
}

#[test]
fn explicit_sizes_follow_positional_insert_and_delete() {
    let mut workbook = workbook();
    workbook
        .set_row_height("sheet-1", "3", 60)
        .unwrap()
        .unwrap();
    workbook
        .set_column_width("sheet-1", "C", 180)
        .unwrap()
        .unwrap();

    workbook.add_row("sheet-1", "1").unwrap();
    workbook.add_column("sheet-1", "A").unwrap();

    assert_eq!(workbook.row_height("sheet-1", "4"), Some(60));
    assert_eq!(workbook.column_width("sheet-1", "D"), Some(180));

    workbook.delete_row("sheet-1", "1").unwrap();
    workbook.delete_column("sheet-1", "A").unwrap();

    assert_eq!(workbook.row_height("sheet-1", "3"), Some(60));
    assert_eq!(workbook.column_width("sheet-1", "C"), Some(180));
    workbook.validate_source().unwrap();
}

#[test]
fn deleting_a_sized_row_carries_the_size_in_the_restore_payload() {
    let mut workbook = workbook();
    workbook
        .set_row_height("sheet-1", "2", 55)
        .unwrap()
        .unwrap();

    let payload = workbook.row_restore_payload("sheet-1", "2").unwrap();
    assert_eq!(payload.row_height, Some(55));
    workbook.delete_row("sheet-1", "2").unwrap();
    assert!(workbook.sheets[0].row_heights.is_empty());

    workbook.restore_row("sheet-1", "2", payload).unwrap();
    assert_eq!(workbook.row_height("sheet-1", "2"), Some(55));
    assert_eq!(user_value(&workbook, "A2"), "Apples");
    assert_eq!(user_value(&workbook, "A3"), "Total");
    workbook.validate_source().unwrap();
}

#[test]
fn restoring_a_column_reinserts_it_positionally() {
    let mut workbook = workbook();
    let payload = workbook.column_restore_payload("sheet-1", "A").unwrap();
    workbook.delete_column("sheet-1", "A").unwrap();
    assert_eq!(user_value(&workbook, "A1"), "Count");

    workbook.restore_column("sheet-1", "A", payload).unwrap();
    assert_eq!(user_value(&workbook, "A1"), "Item");
    assert_eq!(user_value(&workbook, "B1"), "Count");
    workbook.validate_source().unwrap();
}

// ---- Range sort (SH-7) ------------------------------------------------------

fn sortable() -> SpreadsheetWorkbook {
    let mut workbook = SpreadsheetWorkbook::empty("Sort");
    workbook.add_sheet_with_id("sheet-1", "Sheet1");
    for (address, value) in [
        ("A1", "Name"),
        ("B1", "Score"),
        ("A2", "Cleo"),
        ("B2", "3"),
        ("A3", "Ada"),
        ("B3", "10"),
        ("A4", "Bea"),
        ("B4", "7"),
    ] {
        workbook
            .set_cell_in_sheet("sheet-1", address, value.to_string())
            .unwrap();
    }
    workbook
}

#[test]
fn sort_range_orders_rows_by_a_column_and_keeps_the_header() {
    let mut workbook = sortable();

    workbook
        .sort_range("sheet-1", "A1:B4", "B", false, true)
        .unwrap()
        .unwrap();

    assert_eq!(user_value(&workbook, "A1"), "Name");
    assert_eq!(user_value(&workbook, "A2"), "Cleo");
    assert_eq!(user_value(&workbook, "A3"), "Bea");
    assert_eq!(user_value(&workbook, "A4"), "Ada");
    assert_eq!(user_value(&workbook, "B4"), "10");
    workbook.validate_source().unwrap();
}

#[test]
fn sort_range_descending_and_without_a_header_moves_every_row() {
    let mut workbook = sortable();

    workbook
        .sort_range("sheet-1", "A2:B4", "A", true, false)
        .unwrap()
        .unwrap();

    assert_eq!(user_value(&workbook, "A2"), "Cleo");
    assert_eq!(user_value(&workbook, "A3"), "Bea");
    assert_eq!(user_value(&workbook, "A4"), "Ada");
}

#[test]
fn sort_range_moves_formulas_with_their_row() {
    let mut workbook = sortable();
    workbook
        .set_cell_in_sheet("sheet-1", "C2", "=B2*2".to_string())
        .unwrap();
    workbook
        .set_cell_in_sheet("sheet-1", "C3", "=B3*2".to_string())
        .unwrap();
    workbook
        .set_cell_in_sheet("sheet-1", "C4", "=B4*2".to_string())
        .unwrap();

    workbook
        .sort_range("sheet-1", "A1:C4", "B", false, true)
        .unwrap()
        .unwrap();

    // Ada (10) lands in row 4, and her formula follows and still reads B4.
    assert_eq!(user_value(&workbook, "A4"), "Ada");
    assert_eq!(user_value(&workbook, "C4"), "=B4*2");
    assert_eq!(user_value(&workbook, "C2"), "=B2*2");
}

#[test]
fn sort_range_rejects_a_column_outside_the_range() {
    let mut workbook = sortable();

    assert!(workbook
        .sort_range("sheet-1", "A1:B4", "D", false, true)
        .unwrap()
        .is_err());
    assert!(workbook
        .sort_range("missing-sheet", "A1:B4", "A", false, true)
        .is_none());
}
