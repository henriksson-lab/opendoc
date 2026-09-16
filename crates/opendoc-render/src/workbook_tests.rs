use crate::workbook::{column_label, parse_address, render_workbook_html};
use crate::*;
use opendoc_spreadsheet::SheetImage;

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
fn emits_sheet_image_anchor_without_claiming_blob_bytes() {
    let mut workbook = SpreadsheetWorkbook::sample();
    let sheet_id = workbook.sheets[0].id.clone();
    workbook.sheets[0].images.push(SheetImage {
        id: "picture-1".to_string(),
        blob_hash: "sha256:abc_DEF-123.png".to_string(),
        start_column: "A".to_string(),
        start_row: "1".to_string(),
        start_offset_x_px: 3,
        start_offset_y_px: 4,
        end_column: "B".to_string(),
        end_row: "3".to_string(),
        end_offset_x_px: 5,
        end_offset_y_px: 6,
    });
    let html = render_workbook_html(&workbook, &sheet_id).unwrap();
    assert!(html.contains("data-sheet-image-id=\"picture-1\""), "{html}");
    assert!(
        html.contains("data-blob-hash=\"sha256:abc_DEF-123.png\""),
        "{html}"
    );
    assert!(!html.contains("src=\"data:"), "pure renderer leaked a blob");
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
fn renders_durable_cell_wrap_and_vertical_alignment() {
    let mut workbook = SpreadsheetWorkbook::sample();
    let sheet_id = workbook.sheets[0].id.clone();
    workbook
        .set_cell_format(&sheet_id, "A1", "wrap_strategy", "wrap".to_string())
        .unwrap()
        .unwrap();
    workbook
        .set_cell_format(&sheet_id, "A1", "vertical_align", "middle".to_string())
        .unwrap()
        .unwrap();

    let html = render_workbook_html(&workbook, &sheet_id).unwrap();
    assert!(html.contains("class=\"text-wrap\""), "{html}");
    assert!(html.contains("vertical-align:middle;"), "{html}");
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

/// The suppression of hidden axes used to live in `spreadsheet.ts`, which had
/// to rebuild a hidden column's addresses from the sheet's own labels and
/// delete its `<col>` after every morph. It is a fact about the sheet, so the
/// projection answers it: the markup is simply not written.
#[test]
fn hidden_rows_and_columns_are_not_drawn_at_all() {
    let mut workbook = SpreadsheetWorkbook::sample();
    let sheet_id = workbook.sheets[0].id.clone();
    workbook.set_row_hidden(&sheet_id, "2", true).unwrap();
    workbook.set_column_hidden(&sheet_id, "B", true).unwrap();

    let html = render_workbook_html(&workbook, &sheet_id).unwrap();

    assert!(!html.contains("data-row=\"2\""), "the row is drawn: {html}");
    assert!(
        !html.contains("data-column=\"B\""),
        "the column header is drawn: {html}"
    );
    assert!(
        !html.contains("data-address=\"B"),
        "a cell of the hidden column is drawn: {html}"
    );
    assert!(
        !html.contains("data-address=\"A2\""),
        "a cell of the hidden row is drawn: {html}"
    );
    // Neighbours are untouched, and so is the model: hiding is visibility.
    assert!(html.contains("data-row=\"3\""), "{html}");
    assert!(html.contains("data-address=\"A3\""), "{html}");
    assert!(html.contains("data-column=\"C\""), "{html}");
    assert!(workbook.sheets[0]
        .cells
        .iter()
        .any(|cell| cell.address == "B2"));

    // Revealing puts them back, byte for byte.
    workbook.set_row_hidden(&sheet_id, "2", false).unwrap();
    workbook.set_column_hidden(&sheet_id, "B", false).unwrap();
    assert_eq!(
        render_workbook_html(&workbook, &sheet_id).unwrap(),
        render_workbook_html(&SpreadsheetWorkbook::sample(), &sheet_id).unwrap()
    );
}

/// The `<col>` elements map to columns by position, so a hidden column must
/// lose its entry or every width after it lands one column early.
#[test]
fn the_colgroup_lines_up_with_the_columns_actually_drawn() {
    let mut workbook = SpreadsheetWorkbook::sample();
    let sheet_id = workbook.sheets[0].id.clone();
    workbook
        .set_column_width(&sheet_id, "C", 220)
        .unwrap()
        .unwrap();
    workbook.set_column_hidden(&sheet_id, "B", true).unwrap();

    let html = render_workbook_html(&workbook, &sheet_id).unwrap();
    let colgroup = html
        .split_once("<colgroup>")
        .and_then(|(_, rest)| rest.split_once("</colgroup>"))
        .map(|(group, _)| group.to_string())
        .expect("a stored width emits a colgroup");
    assert!(!colgroup.contains("data-column=\"B\""), "{colgroup}");
    let drawn = workbook.sheets[0].columns.len() - 1;
    assert_eq!(
        colgroup.matches("<col").count(),
        drawn + 1,
        "one <col> per drawn column plus the row header: {colgroup}"
    );
    // C is the second drawn column, so its width must be on the third <col>:
    // the row header, A, then C.
    let widths: Vec<&str> = colgroup.split("<col").skip(1).collect();
    assert_eq!(
        widths
            .iter()
            .position(|element| element.contains("width:220px")),
        Some(2),
        "the width landed on the wrong column: {colgroup}"
    );

    // A width that belongs only to a hidden column is no width at all.
    let mut only_hidden = SpreadsheetWorkbook::sample();
    only_hidden
        .set_column_width(&sheet_id, "B", 220)
        .unwrap()
        .unwrap();
    only_hidden.set_column_hidden(&sheet_id, "B", true).unwrap();
    let html = render_workbook_html(&only_hidden, &sheet_id).unwrap();
    assert!(!html.contains("<colgroup"), "{html}");
}

/// Hiding part of a merge narrows the block rather than punching a hole in the
/// row: the row must keep exactly one drawn cell per drawn column.
#[test]
fn a_merge_spans_only_the_rows_and_columns_still_drawn() {
    let mut workbook = SpreadsheetWorkbook::sample();
    let sheet_id = workbook.sheets[0].id.clone();
    workbook
        .set_cell_in_sheet(&sheet_id, "A1", "Merged".to_string())
        .unwrap();
    workbook.merge_cells(&sheet_id, "A1:C1").unwrap().unwrap();
    let workbook_all_visible = workbook.evaluated();

    let cells_in_first_row = |html: &str| -> usize {
        html.split("<tr data-row=\"1\"")
            .nth(1)
            .and_then(|rest| rest.split_once("</tr>"))
            .map(|(row, _)| row.matches("<td ").count())
            .unwrap()
    };
    let drawn_columns = |html: &str| html.matches("<th class=\"column-header").count();

    let html = render_workbook_html(&workbook_all_visible, &sheet_id).unwrap();
    assert!(html.contains("colspan=\"3\""), "{html}");
    assert_eq!(
        cells_in_first_row(&html) + 2,
        drawn_columns(&html),
        "three columns collapse into one cell: {html}"
    );

    // Hiding a covered column narrows the span.
    let mut narrowed = workbook_all_visible.clone();
    narrowed.set_column_hidden(&sheet_id, "B", true).unwrap();
    let html = render_workbook_html(&narrowed, &sheet_id).unwrap();
    assert!(html.contains("colspan=\"2\""), "{html}");
    assert!(html.contains(">Merged<"), "{html}");
    assert_eq!(
        cells_in_first_row(&html) + 1,
        drawn_columns(&html),
        "{html}"
    );

    // Hiding the merge's own anchor column draws the block where it starts
    // now, still showing the anchor's content and still naming it.
    let mut reanchored = workbook_all_visible.clone();
    reanchored.set_column_hidden(&sheet_id, "A", true).unwrap();
    let html = render_workbook_html(&reanchored, &sheet_id).unwrap();
    assert!(html.contains("colspan=\"2\""), "{html}");
    assert!(
        html.contains("data-address=\"A1\""),
        "the block still names the anchor the model stores: {html}"
    );
    assert!(html.contains(">Merged<"), "{html}");
    assert_eq!(
        cells_in_first_row(&html) + 1,
        drawn_columns(&html),
        "{html}"
    );

    // Hiding every column of the merge leaves a whole row of ordinary cells.
    let mut gone = workbook_all_visible.clone();
    for column in ["A", "B", "C"] {
        gone.set_column_hidden(&sheet_id, column, true).unwrap();
    }
    let html = render_workbook_html(&gone, &sheet_id).unwrap();
    assert!(!html.contains("colspan="), "{html}");
    assert!(!html.contains(">Merged<"), "{html}");
    assert_eq!(cells_in_first_row(&html), drawn_columns(&html), "{html}");
}
