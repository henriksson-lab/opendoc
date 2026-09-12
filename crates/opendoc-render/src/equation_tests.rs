use crate::footnotes::render_footnotes;
use crate::test_support::*;
use crate::*;

#[test]
fn block_equation_renders_display_mathml_not_latex_source() {
    let document = document_with_equation_block(r"\frac{a}{b}");
    let rendering = render_document(&document, []);
    assert!(rendering.warnings.is_empty(), "{:?}", rendering.warnings);
    assert!(
        rendering.html.contains("<math display=\"block\""),
        "{}",
        rendering.html
    );
    assert!(
        rendering
            .html
            .contains("<mfrac><mi>a</mi><mi>b</mi></mfrac>"),
        "{}",
        rendering.html
    );
    assert!(
        rendering.html.contains("equation-rendered"),
        "{}",
        rendering.html
    );
    // The canonical source stays available as an attribute, and is not the
    // visible content any more.
    assert!(
        rendering
            .html
            .contains("data-equation-source=\"\\frac{a}{b}\""),
        "{}",
        rendering.html
    );
}

#[test]
fn inline_equation_renders_inline_mathml_inside_the_paragraph() {
    let document = document_with_inline_equation("E = mc^2");
    let rendering = render_document(&document, []);
    assert!(rendering.warnings.is_empty(), "{:?}", rendering.warnings);
    assert!(
        rendering
            .html
            .starts_with("<p class=\"doc-block doc-paragraph\""),
        "{}",
        rendering.html
    );
    assert!(
        rendering.html.contains("equation-inline"),
        "{}",
        rendering.html
    );
    assert!(
        rendering.html.contains("equation-rendered"),
        "{}",
        rendering.html
    );
    // Inline math must not be promoted to display style.
    let math_open = rendering
        .html
        .find("<math")
        .expect("inline equation renders a math element");
    let math_tag =
        &rendering.html[math_open..rendering.html[math_open..].find('>').unwrap() + math_open];
    assert!(!math_tag.contains("display=\"block\""), "{math_tag}");
    assert!(rendering.html.contains("<msup>"), "{}", rendering.html);
}

#[test]
fn malformed_equation_degrades_to_flagged_source_with_a_warning() {
    let document = document_with_equation_block(r"\frac{a}{");
    let rendering = render_document(&document, []);
    assert!(!rendering.html.contains("<math"), "{}", rendering.html);
    assert!(
        rendering.html.contains("equation-error"),
        "{}",
        rendering.html
    );
    assert!(
        rendering.html.contains("data-equation-error="),
        "{}",
        rendering.html
    );
    // The source is still shown, escaped, so nothing is silently blanked.
    assert!(
        rendering.html.contains(r"\frac{a}{</span>"),
        "{}",
        rendering.html
    );
    assert_eq!(rendering.warnings.len(), 1, "{:?}", rendering.warnings);
    assert_eq!(rendering.warnings[0].code, WARNING_EQUATION_RENDER_FAILED);
    assert!(rendering.warnings[0].validate().is_ok());
    // The rest of the document still renders.
    assert!(rendering.html.contains("before"), "{}", rendering.html);
}

#[test]
fn equation_rendering_is_idempotent_and_does_not_mutate_the_document() {
    let document = document_with_equation_block(r"\sum_{i=0}^{N} x_i");
    let before = document.clone();
    let first = render_document(&document, []);
    let second = render_document(&document, []);
    assert_eq!(first.html, second.html);
    assert_eq!(first.warnings, second.warnings);
    assert_eq!(document, before, "rendering must not mutate the document");
}

#[test]
fn footnote_equations_report_their_warnings() {
    let mut document = document_with(&["body"]);
    let footnote_id = StableId::parse("footnote-1").expect("valid id");
    document.blocks[0].content.push(Inline::FootnoteRef {
        id: StableId::parse("inline-footnote-ref").expect("valid id"),
        footnote_id: footnote_id.clone(),
    });
    document.footnotes.push(opendoc_core::Footnote {
        id: footnote_id,
        revision: 0,
        body: vec![Inline::Equation {
            id: StableId::parse("inline-equation-2").expect("valid id"),
            equation: latex_equation("equation-inline-2", r"\frac{a}{"),
        }],
        deleted: false,
    });
    let rendering = render_footnotes(&document, []);
    assert!(
        rendering.html.contains("equation-error"),
        "{}",
        rendering.html
    );
    assert_eq!(rendering.warnings.len(), 1, "{:?}", rendering.warnings);
    assert_eq!(rendering.warnings[0].code, WARNING_EQUATION_RENDER_FAILED);
}
