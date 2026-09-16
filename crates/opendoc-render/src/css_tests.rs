use crate::css::{thousandths_to_css_number, twips_to_css_pt};
use crate::test_support::*;
use crate::*;
use opendoc_core::{Alignment, BlockProperty, BorderStyle, CellBorder, TextDirection};

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
    properties.set(BlockProperty::KeepWithNext(true));
    properties.set(BlockProperty::Background(
        opendoc_core::Color::parse("#336699").unwrap(),
    ));
    properties.set(BlockProperty::Border(
        CellBorder::new(
            BorderStyle::Dashed,
            opendoc_core::Length::from_twips(20).unwrap(),
            opendoc_core::Color::parse("#336699").unwrap(),
        )
        .unwrap(),
    ));

    let html = render_document_html(&document, []);
    assert!(html.contains("text-align:center;"), "{html}");
    assert!(html.contains("margin-inline-start:36pt;"), "{html}");
    assert!(html.contains("margin-inline-end:18pt;"), "{html}");
    assert!(html.contains("text-indent:-18pt;"), "{html}");
    assert!(html.contains("line-height:1.5;"), "{html}");
    assert!(html.contains("margin-block-start:6pt;"), "{html}");
    assert!(html.contains("margin-block-end:12pt;"), "{html}");
    assert!(html.contains("direction:rtl;"), "{html}");
    assert!(html.contains("break-after:avoid-page;"), "{html}");
    assert!(html.contains("background-color:#336699;"), "{html}");
    assert!(html.contains("border:1pt dashed #336699;"), "{html}");
}

/// The block element's vertical spacing is projected *logically*, and that is
/// load-bearing rather than stylistic: the frontend's pagination owns the
/// physical `margin-top` of a page-opening block (ADR 0014 — Rust decides,
/// the frontend places). Two owners writing one CSSOM property meant applying
/// a layout deleted the document's own space-before; two different properties
/// cannot collide, and clearing the placement restores the model's value.
#[test]
fn vertical_spacing_never_claims_the_physical_margin_pagination_owns() {
    let mut document = document_with(&["spaced"]);
    let properties = &mut document.blocks[0].properties;
    properties.set(BlockProperty::SpaceBefore(
        opendoc_core::Length::from_twips(300).unwrap(),
    ));
    properties.set(BlockProperty::SpaceAfter(
        opendoc_core::Length::from_twips(300).unwrap(),
    ));
    let html = render_document_html(&document, []);
    assert!(html.contains("margin-block-start:15pt;"), "{html}");
    assert!(html.contains("margin-block-end:15pt;"), "{html}");
    assert!(
        !html.contains("margin-top") && !html.contains("margin-bottom"),
        "the physical vertical margins belong to the frontend's placement: {html}"
    );
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
    assert!(
        html.contains(
            "tabindex=\"0\" role=\"checkbox\" aria-checked=\"false\" aria-label=\"Done\""
        ) && html.contains("aria-checked=\"true\""),
        "the durable wrapper is the one keyboard-reachable checkbox: {html}"
    );
    assert_eq!(html.matches("aria-hidden=\"true\"").count(), 2, "{html}");
    assert_eq!(
        html.matches("<ul").count(),
        2,
        "a checklist and a bulleted run are two lists: {html}"
    );
    assert_eq!(html.matches("doc-checklist").count(), 1, "{html}");
    assert_eq!(html.matches("<ul").count(), html.matches("</ul>").count());
}

// ---- Page geometry and page furniture (PLAN77 B7) -------------------

// ---- The app stylesheet's fallbacks (ADR 0014 §3) --------------------------

/// The desktop stylesheet, included at compile time so this test cannot go
/// stale against a file that moved.
///
/// The export's stylesheet may state no length at all
/// (`the_stylesheet_states_no_length_of_its_own`), because it is generated
/// after the scale is known. The app's stylesheet is served before the first
/// layout arrives, so by design it repeats each projected value as a `var()`
/// fallback — and a repeated value is exactly what ADR 0014 §3 forbids being
/// *different*. A literal moved into a fallback would otherwise be a literal
/// the export's scanner cannot see.
const APP_STYLESHEET: &str = include_str!("../../../apps/desktop/src/styles.css");

/// Every `var(--doc-…, fallback)` in the app stylesheet must state exactly the
/// value `TypeScale` projects for that property.
///
/// This is the drift the PDF's furniture size had: a size stated twice, the
/// two statements disagreeing, and nothing to notice. `.doc-table`'s padding
/// and border were the last literals here that the scale also owns.
#[test]
fn the_app_stylesheets_fallbacks_agree_with_the_projected_scale() {
    let projected = opendoc_layout::type_scale_css_variables();
    let mut scale: std::collections::BTreeMap<&str, &str> = std::collections::BTreeMap::new();
    for declaration in projected.split(';') {
        if let Some((name, value)) = declaration.split_once(':') {
            scale.insert(name.trim(), value.trim());
        }
    }
    assert!(
        scale.contains_key("--doc-cell-padding-block"),
        "the scale no longer projects the table cell padding: {projected}"
    );

    let mut checked: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    let mut rest = APP_STYLESHEET;
    while let Some(index) = rest.find("var(--doc-") {
        rest = &rest[index + 4..];
        let end = rest.find(')').expect("an unclosed var()");
        let (name, fallback) = match rest[..end].split_once(',') {
            Some((name, fallback)) => (name.trim(), fallback.trim()),
            // No fallback is always honest: nothing is stated twice.
            None => continue,
        };
        let Some(value) = scale.get(name) else {
            continue;
        };
        assert_eq!(
            &fallback, value,
            "styles.css states {name} as {fallback}, the type scale projects {value}"
        );
        checked.insert(name);
    }
    // Named rather than counted: a fallback whose property name is misspelt
    // finds nothing in the scale and would otherwise be skipped in silence,
    // which is the failure this test exists to catch.
    for name in [
        "--doc-cell-padding-block",
        "--doc-cell-padding-inline",
        "--doc-cell-border",
        "--doc-block-space-after",
        "--doc-furniture-size",
        "--doc-font-size",
    ] {
        assert!(
            checked.contains(name),
            "the stylesheet no longer reads {name}; it states the size itself"
        );
    }
}
