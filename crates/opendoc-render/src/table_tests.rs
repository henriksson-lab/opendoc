use crate::test_support::*;
use crate::*;
use opendoc_core::{Bookmark, Length, StableId};

#[test]
fn a_table_projects_its_columns_and_their_widths() {
    let mut document = table_document();
    let (columns, _) = table_parts(&mut document);
    columns[1].width = Length::from_points(90.0).ok();
    let column_id = columns[1].id.to_string();
    let html = render_document_html(&document, []);

    assert!(html.contains("table-layout:fixed;width:100%;"), "{html}");
    assert!(
        html.contains(&format!(
            "<col data-column-id=\"{column_id}\" style=\"width:90pt\">"
        )),
        "{html}"
    );
    // The other two are auto, so they carry no width at all — the model
    // never invents one.
    assert_eq!(html.matches("style=\"width:").count(), 1, "{html}");
    // 90pt + two auto columns at 72pt each: the table may not be squeezed
    // narrower than the widths it was told.
    assert!(html.contains("min-width:234pt"), "{html}");
    assert!(
        html.contains(&format!("data-column-id=\"{column_id}\"><p")),
        "cells name their column: {html}"
    );
}

#[test]
fn a_fixed_width_table_projects_its_own_alignment_without_aligning_cell_text() {
    let mut document = table_document();
    {
        let (columns, _) = table_parts(&mut document);
        for column in columns {
            column.width = Length::from_points(72.0).ok();
        }
    }
    let BlockKind::Table { properties, .. } = &mut document.blocks[0].kind else {
        panic!("expected table");
    };
    properties.alignment = Some(opendoc_core::TableAlignment::Center);
    let html = render_document_html(&document, []);

    assert!(html.contains("table-layout:fixed;width:216pt;"), "{html}");
    assert!(html.contains("margin-inline:auto;"), "{html}");
    assert!(!html.contains("text-align:center"), "{html}");
}

#[test]
fn a_header_row_projects_semantic_cells_without_implicit_formatting() {
    let mut document = table_document();
    let (_, rows) = table_parts(&mut document);
    rows[0].header = true;
    let html = render_document_html(&document, []);

    assert_eq!(html.matches("<th ").count(), 3, "{html}");
    assert_eq!(html.matches("<td ").count(), 3, "{html}");
    assert!(html.contains("</colgroup><thead>"), "{html}");
    assert!(html.contains("</thead><tbody>"), "{html}");
}

#[test]
fn leading_headers_name_the_data_cells_they_cover() {
    let mut document = table_document();
    let (_, rows) = table_parts(&mut document);
    rows[0].header = true;
    let header_ids: Vec<String> = rows[0]
        .cells
        .iter()
        .map(|cell| cell.id.to_string())
        .collect();
    let table_id = document.blocks[0].id.to_string();
    let html = render_document_html(&document, []);

    for header_id in &header_ids {
        let dom_id = format!("opendoc-table-header:{table_id}:{header_id}");
        assert!(
            html.contains(&format!("id=\"{dom_id}\" scope=\"col\"")),
            "leading header must declare its column scope: {html}"
        );
        assert!(
            html.contains(&format!("headers=\"{dom_id}\"")),
            "data cells must name their column header: {html}"
        );
    }
}

#[test]
fn table_header_ids_cannot_collide_with_portable_bookmark_names() {
    let mut document = table_document();
    let table_id = document.blocks[0].id.clone();
    let cell_id = {
        let (_, rows) = table_parts(&mut document);
        rows[0].header = true;
        rows[0].cells[0].id = StableId::parse("header-cell").expect("stable cell id");
        rows[0].cells[0].id.clone()
    };
    // This was exactly the pre-namespace header id, and is valid portable
    // bookmark syntax. It renders into the same block as the table.
    let legacy_id = format!("table-{table_id}-header-{cell_id}");
    document.bookmarks.push(Bookmark {
        id: StableId::parse("bookmark-table-header").expect("bookmark id"),
        name: legacy_id.clone(),
        block_id: table_id.clone(),
        revision: 1,
        deleted: false,
    });
    document
        .validate()
        .expect("bookmark and table are valid source state");

    let header_id = format!("opendoc-table-header:{table_id}:{cell_id}");
    let html = render_document_html(&document, []);
    assert_eq!(
        html.matches(&format!("id=\"{legacy_id}\"")).count(),
        1,
        "the bookmark keeps its own target without duplicating a table header id: {html}"
    );
    assert!(
        html.contains(&format!("id=\"{header_id}\" scope=\"col\"")),
        "the table uses its reserved header namespace: {html}"
    );
    assert!(
        html.contains(&format!("headers=\"{header_id}\"")),
        "data-cell associations name the reserved table header id: {html}"
    );
}

#[test]
fn merged_leading_headers_use_colgroup_scope_and_cover_every_spanned_column() {
    let mut document = table_document();
    let header_id = {
        let (_, rows) = table_parts(&mut document);
        rows[0].header = true;
        rows[0].cells[0].span = opendoc_core::CellSpan::new(1, 2).expect("legal span");
        rows[0].cells[0].id.to_string()
    };
    document.validate().expect("valid grid");
    let table_id = document.blocks[0].id.to_string();
    let dom_id = format!("opendoc-table-header:{table_id}:{header_id}");
    let html = render_document_html(&document, []);

    assert!(
        html.contains(&format!("id=\"{dom_id}\" scope=\"colgroup\"")),
        "{html}"
    );
    assert_eq!(
        html.matches(&format!("headers=\"{dom_id}\"")).count(),
        2,
        "{html}"
    );
}

#[test]
fn nested_header_tiers_name_their_parent_headers_and_data_names_every_tier() {
    let mut document = table_document();
    let table_id = document.blocks[0].id.to_string();
    let (group_id, first_leaf_id, third_leaf_id, upper_third_id) = {
        let (columns, rows) = table_parts(&mut document);
        let column_count = columns.len();
        rows[0].header = true;
        rows[0].cells[0].span = opendoc_core::CellSpan::new(1, 2).expect("legal span");
        rows[1].header = true;
        rows.push(opendoc_core::TableRow::empty(column_count));
        (
            rows[0].cells[0].id.to_string(),
            rows[1].cells[0].id.to_string(),
            rows[1].cells[2].id.to_string(),
            rows[0].cells[2].id.to_string(),
        )
    };
    document.validate().expect("valid grid");
    let html = render_document_html(&document, []);

    let group_dom_id = format!("opendoc-table-header:{table_id}:{group_id}");
    let first_leaf_dom_id = format!("opendoc-table-header:{table_id}:{first_leaf_id}");
    let third_leaf_dom_id = format!("opendoc-table-header:{table_id}:{third_leaf_id}");
    assert!(
        html.contains(&format!(
            "id=\"{first_leaf_dom_id}\" scope=\"col\" headers=\"{group_dom_id}\""
        )),
        "a leaf header must name its group header: {html}"
    );
    assert!(
        html.contains(&format!(
            "id=\"{third_leaf_dom_id}\" scope=\"col\" headers=\"opendoc-table-header:{table_id}:{}\"",
            upper_third_id
        )),
        "an unmerged leaf must name the upper header in its own column: {html}"
    );
    assert!(
        html.contains(&format!("headers=\"{group_dom_id} {first_leaf_dom_id}\"")),
        "a data cell must name both its group and leaf headers: {html}"
    );
}

#[test]
fn nonleading_header_rows_remain_headers_without_an_invented_association() {
    let mut document = table_document();
    let (_, rows) = table_parts(&mut document);
    rows[1].header = true;
    let html = render_document_html(&document, []);

    assert_eq!(html.matches("<th ").count(), 3, "{html}");
    assert!(
        !html.contains(" scope=") && !html.contains(" headers="),
        "a header after data has no proven row/column meaning: {html}"
    );
}

#[test]
fn explicit_row_headers_name_data_cells_without_a_positional_convention() {
    let mut document = table_document();
    let table_id = document.blocks[0].id.to_string();
    let row_header_id = {
        let (_, rows) = table_parts(&mut document);
        rows[1].cells[0].properties.row_header = Some(true);
        rows[1].cells[0].id.to_string()
    };
    let html = render_document_html(&document, []);
    let dom_id = format!("opendoc-table-header:{table_id}:{row_header_id}");

    assert!(
        html.contains(&format!("id=\"{dom_id}\" scope=\"row\"")),
        "the authored row role must project to a row-scoped header: {html}"
    );
    assert_eq!(
        html.matches(&format!("headers=\"{dom_id}\"")).count(),
        2,
        "each other cell in the row must name the explicit header: {html}"
    );
}

#[test]
fn explicit_row_headers_also_name_their_proven_leading_column_header() {
    let mut document = table_document();
    let table_id = document.blocks[0].id.to_string();
    let (column_header_id, row_header_id) = {
        let (_, rows) = table_parts(&mut document);
        rows[0].header = true;
        rows[1].cells[0].properties.row_header = Some(true);
        (
            rows[0].cells[0].id.to_string(),
            rows[1].cells[0].id.to_string(),
        )
    };
    let html = render_document_html(&document, []);
    let column_dom_id = format!("opendoc-table-header:{table_id}:{column_header_id}");
    let row_dom_id = format!("opendoc-table-header:{table_id}:{row_header_id}");

    assert!(
        html.contains(&format!(
            "id=\"{row_dom_id}\" scope=\"row\" headers=\"{column_dom_id}\""
        )),
        "the explicit row header has a proven column context: {html}"
    );
}

#[test]
fn row_headers_name_a_data_cell_that_spans_into_their_row() {
    let mut document = table_document();
    let table_id = document.blocks[0].id.to_string();
    let (spanning_data_id, row_header_id) = {
        let (_, rows) = table_parts(&mut document);
        // The left data cell is physically present only on row zero, but it
        // occupies rows zero and one. The right cell on row one is an
        // explicitly authored row header, so it labels the merged left cell
        // too. Reading only the data cell's start row used to lose that
        // relationship.
        rows[0].cells[0].span = opendoc_core::CellSpan::new(2, 1).expect("legal span");
        rows[1].cells[1].properties.row_header = Some(true);
        (
            rows[0].cells[0].id.to_string(),
            rows[1].cells[1].id.to_string(),
        )
    };
    document.validate().expect("valid grid");
    let html = render_document_html(&document, []);
    let row_header_dom_id = format!("opendoc-table-header:{table_id}:{row_header_id}");

    let opening_cell = html
        .split(&format!("data-cell-id=\"{spanning_data_id}\""))
        .nth(1)
        .and_then(|tail| tail.split('>').next())
        .expect("the spanning source cell is rendered");
    assert!(
        opening_cell.contains(&format!("headers=\"{row_header_dom_id}\""))
            && opening_cell.contains("rowspan=\"2\""),
        "a cell spanning an explicitly headed row retains that association: {html}"
    );
}

#[test]
fn explicit_row_header_in_a_leading_row_is_not_invented_as_a_column_header() {
    let mut document = table_document();
    let table_id = document.blocks[0].id.to_string();
    let row_header_id = {
        let (_, rows) = table_parts(&mut document);
        rows[0].header = true;
        rows[0].cells[0].properties.row_header = Some(true);
        rows[0].cells[0].id.to_string()
    };
    let html = render_document_html(&document, []);
    let dom_id = format!("opendoc-table-header:{table_id}:{row_header_id}");

    assert!(
        html.contains(&format!("id=\"{dom_id}\" scope=\"row\"")),
        "the explicit cell role wins over its containing header row: {html}"
    );
    assert!(
        !html.contains(&format!("headers=\"{dom_id}"))
            && !html.contains(&format!(" {dom_id}\"")),
        "later data cells must not be associated with a row-scoped header as though it were a column tier: {html}"
    );
}

#[test]
fn a_merged_cell_spans_and_its_covered_neighbours_are_not_drawn() {
    let mut document = table_document();
    let (_, rows) = table_parts(&mut document);
    rows[0].cells[0].span = opendoc_core::CellSpan::new(2, 2).expect("legal span");
    document.validate().expect("valid grid");
    let html = render_document_html(&document, []);

    assert!(html.contains("rowspan=\"2\""), "{html}");
    assert!(html.contains("colspan=\"2\""), "{html}");
    assert!(html.contains("r0c0"), "{html}");
    for hidden in ["r0c1", "r1c0", "r1c1"] {
        assert!(!html.contains(hidden), "{hidden} is covered: {html}");
    }
    // The two uncovered cells of column 3 survive.
    assert!(html.contains("r0c2") && html.contains("r1c2"), "{html}");
    assert_eq!(html.matches("<td ").count(), 3, "{html}");
}

#[test]
fn cell_styling_becomes_direction_relative_css() {
    let mut document = table_document();
    let (_, rows) = table_parts(&mut document);
    let properties = &mut rows[0].cells[0].properties;
    properties.background = Some(opendoc_core::Color::parse("#FFEE00").expect("legal colour"));
    properties.border_start = Some(
        opendoc_core::CellBorder::new(
            opendoc_core::BorderStyle::Dashed,
            Length::from_points(1.5).expect("legal width"),
            opendoc_core::Color::parse("#336699").expect("legal colour"),
        )
        .expect("legal border"),
    );
    properties.border_top = Some(opendoc_core::CellBorder::none());
    properties.vertical_alignment = Some(opendoc_core::VerticalAlignment::Bottom);
    properties.padding_end = Length::from_points(6.0).ok();
    document.validate().expect("valid grid");
    let html = render_document_html(&document, []);

    assert!(html.contains("background-color:#ffee00;"), "{html}");
    assert!(
        html.contains("border-inline-start:1.5pt dashed #336699;"),
        "{html}"
    );
    assert!(html.contains("border-block-start:none;"), "{html}");
    assert!(html.contains("vertical-align:bottom;"), "{html}");
    assert!(html.contains("padding-inline-end:6pt;"), "{html}");
}
