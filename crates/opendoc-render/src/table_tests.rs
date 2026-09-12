use crate::test_support::*;
use crate::*;
use opendoc_core::Length;

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
