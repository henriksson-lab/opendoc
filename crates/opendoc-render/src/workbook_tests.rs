use crate::workbook::{column_label, parse_address, render_workbook_html};
use crate::*;

#[test]
fn renders_spreadsheet_grid_with_addresses_and_merges() {
    let workbook = SpreadsheetWorkbook::sample();
    let sheet_id = workbook.sheets[0].id.clone();
    let html = render_workbook_html(&workbook, &sheet_id).unwrap();
    assert!(html.starts_with("<table class=\"sheet-grid\""));
    assert!(html.contains("data-address=\"A1\""));
    assert!(render_workbook_html(&workbook, "missing").is_err());
    assert_eq!(column_label(28), "AB");
    assert_eq!(parse_address("AB12"), Some((28, 12)));
}

#[test]
fn renders_formatted_number_and_boolean_display_text() {
    let mut workbook = SpreadsheetWorkbook::sample();
    let sheet_id = workbook.sheets[0].id.clone();
    workbook
        .set_cell_in_sheet(&sheet_id, "C2", "1234.5".to_string())
        .unwrap();
    workbook
        .set_cell_format(&sheet_id, "C2", "number_format", "currency".to_string())
        .unwrap()
        .unwrap();
    workbook
        .set_cell_in_sheet(&sheet_id, "C3", "TRUE".to_string())
        .unwrap();
    let workbook = workbook.evaluated();

    let html = render_workbook_html(&workbook, &sheet_id).unwrap();

    // The stored number format reaches the grid instead of the raw value.
    assert!(html.contains(">$1,234.50<"), "{html}");
    assert!(!html.contains(">1234.5<"), "{html}");
    // Booleans project as TRUE/FALSE, not as their canonical storage text.
    assert!(html.contains(">TRUE<"), "{html}");
    assert!(!html.contains(">true<"), "{html}");
}

#[test]
fn renders_stored_row_heights_and_column_widths_sparsely() {
    let mut workbook = SpreadsheetWorkbook::sample();
    let sheet_id = workbook.sheets[0].id.clone();

    // Nothing stored: no colgroup, no inline heights at all.
    let bare = render_workbook_html(&workbook, &sheet_id).unwrap();
    assert!(
        !bare.contains("<colgroup"),
        "unset columns emit no colgroup"
    );
    assert!(!bare.contains("height:"), "unset rows emit no height");
    assert!(!bare.contains("width:"), "unset columns emit no width");

    let row = workbook.sheets[0].rows[1].clone();
    let column = workbook.sheets[0].columns[1].clone();
    let other_column = workbook.sheets[0].columns[0].clone();
    workbook
        .set_row_height(&sheet_id, &row, 48)
        .unwrap()
        .unwrap();
    workbook
        .set_column_width(&sheet_id, &column, 220)
        .unwrap()
        .unwrap();

    let html = render_workbook_html(&workbook, &sheet_id).unwrap();
    assert!(html.contains("<colgroup><col class=\"row-header-col\">"));
    assert!(html.contains(&format!(
        "<col data-column=\"{column}\" style=\"width:220px\">"
    )));
    // The untouched column still gets a <col> placeholder, but no width.
    assert!(html.contains(&format!("<col data-column=\"{other_column}\">")));
    assert!(html.contains(&format!("<tr data-row=\"{row}\" style=\"height:48px\">")));
    assert_eq!(html.matches("style=\"height:").count(), 1);
    assert_eq!(html.matches("style=\"width:").count(), 1);

    // Rendering is a pure projection: it must not touch the workbook.
    let before = workbook.clone();
    let _ = render_workbook_html(&workbook, &sheet_id).unwrap();
    assert_eq!(workbook.sheets[0].row_heights, before.sheets[0].row_heights);
    assert_eq!(
        workbook.sheets[0].column_widths,
        before.sheets[0].column_widths
    );

    // Clearing restores the sparse form.
    workbook
        .set_row_height(&sheet_id, &row, 0)
        .unwrap()
        .unwrap();
    workbook
        .set_column_width(&sheet_id, &column, 0)
        .unwrap()
        .unwrap();
    let cleared = render_workbook_html(&workbook, &sheet_id).unwrap();
    assert!(!cleared.contains("<colgroup"));
    assert!(!cleared.contains("height:"));
}
