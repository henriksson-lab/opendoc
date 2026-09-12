use crate::css::{thousandths_to_css_number, twips_to_css_pt};
use crate::test_support::*;
use crate::*;
use opendoc_core::{Alignment, BlockProperty, TextDirection};

/// B4: the twips -> CSS conversion is exact on the 0.05pt grid, including
/// negatives (a hanging indent) and values that are not whole points.
#[test]
fn twips_become_exact_css_points() {
    assert_eq!(twips_to_css_pt(0), "0pt");
    assert_eq!(twips_to_css_pt(20), "1pt");
    assert_eq!(twips_to_css_pt(720), "36pt", "0.5in is 36pt");
    assert_eq!(twips_to_css_pt(1440), "72pt", "1in is 72pt");
    assert_eq!(twips_to_css_pt(-360), "-18pt");
    assert_eq!(
        twips_to_css_pt(1),
        "0.05pt",
        "one twip is a twentieth of a point"
    );
    assert_eq!(twips_to_css_pt(-1), "-0.05pt");
    assert_eq!(twips_to_css_pt(130), "6.50pt");
    assert_eq!(thousandths_to_css_number(1_000), "1");
    assert_eq!(thousandths_to_css_number(1_150), "1.15");
    assert_eq!(thousandths_to_css_number(1_500), "1.5");
    assert_eq!(thousandths_to_css_number(2_000), "2");
}

#[test]
fn typed_block_properties_project_onto_the_block_element() {
    let mut document = document_with(&["formatted"]);
    let properties = &mut document.blocks[0].properties;
    properties.set(BlockProperty::Alignment(Alignment::Center));
    properties.set(BlockProperty::IndentStart(
        opendoc_core::Length::from_twips(720).unwrap(),
    ));
    properties.set(BlockProperty::IndentEnd(
        opendoc_core::Length::from_twips(360).unwrap(),
    ));
    properties.set(BlockProperty::IndentFirstLine(
        opendoc_core::Length::from_twips(-360).unwrap(),
    ));
    properties.set(BlockProperty::LineSpacing(
        LineSpacing::multiple(1.5).unwrap(),
    ));
    properties.set(BlockProperty::SpaceBefore(
        opendoc_core::Length::from_twips(120).unwrap(),
    ));
    properties.set(BlockProperty::SpaceAfter(
        opendoc_core::Length::from_twips(240).unwrap(),
    ));
    properties.set(BlockProperty::Direction(TextDirection::RightToLeft));

    let html = render_document_html(&document, []);
    assert!(html.contains("text-align:center;"), "{html}");
    assert!(html.contains("margin-inline-start:36pt;"), "{html}");
    assert!(html.contains("margin-inline-end:18pt;"), "{html}");
    assert!(html.contains("text-indent:-18pt;"), "{html}");
    assert!(html.contains("line-height:1.5;"), "{html}");
    assert!(html.contains("margin-top:6pt;"), "{html}");
    assert!(html.contains("margin-bottom:12pt;"), "{html}");
    assert!(html.contains("direction:rtl;"), "{html}");
}

#[test]
fn an_unset_property_emits_no_declaration_at_all() {
    let document = document_with(&["plain"]);
    let html = render_document_html(&document, []);
    assert!(
        !html.contains("style=\""),
        "inheriting blocks must not carry a style attribute: {html}"
    );
}

#[test]
fn exact_and_at_least_line_spacing_both_project_to_line_height() {
    for spacing in [
        LineSpacing::exactly(opendoc_core::Length::from_twips(480).unwrap()).unwrap(),
        LineSpacing::at_least(opendoc_core::Length::from_twips(480).unwrap()).unwrap(),
    ] {
        let mut document = document_with(&["spaced"]);
        document.blocks[0]
            .properties
            .set(BlockProperty::LineSpacing(spacing));
        let html = render_document_html(&document, []);
        assert!(html.contains("line-height:24pt;"), "{html}");
    }
}

/// B4/B6: a checklist item renders a real checkbox whose state comes from
/// the model, and checklists are their own list rather than being folded
/// into an adjacent bulleted one.
#[test]
fn checklist_items_render_a_checkbox_and_their_own_list() {
    let mut document = document_with(&["open", "done", "bulleted"]);
    let list_id = opendoc_core::new_list_id();
    for (index, kind) in [
        ListKind::Checklist { checked: false },
        ListKind::Checklist { checked: true },
        ListKind::Bullet,
    ]
    .into_iter()
    .enumerate()
    {
        document.blocks[index].kind = BlockKind::ListItem {
            list_id: list_id.clone(),
            level: 0,
            kind,
        };
    }
    let html = render_document_html(&document, []);
    assert_eq!(
        html.matches("<input type=\"checkbox\"").count(),
        2,
        "{html}"
    );
    assert_eq!(html.matches(" checked>").count(), 1, "{html}");
    assert!(
        html.contains(&format!(
            "data-checklist-block-id=\"{}\"",
            document.blocks[1].id
        )),
        "the checkbox names the block the toggle command must target: {html}"
    );
    assert_eq!(
        html.matches("<ul").count(),
        2,
        "a checklist and a bulleted run are two lists: {html}"
    );
    assert_eq!(html.matches("doc-checklist").count(), 1, "{html}");
    assert_eq!(html.matches("<ul").count(), html.matches("</ul>").count());
}

// ---- Page geometry and page furniture (PLAN77 B7) -------------------
