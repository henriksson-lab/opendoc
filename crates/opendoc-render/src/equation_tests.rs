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
    assert!(
        rendering
            .html
            .contains("role=\"group\" aria-label=\"Block equation\""),
        "{}",
        rendering.html
    );
    assert!(
        rendering.html.contains("tabindex=\"0\""),
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

/// The LaTeX that `math_core` 0.8.2 turns into escaping markup.
///
/// `\operatorname` does not escape its argument, and it strips whitespace —
/// hence the `/` separators, which an HTML parser accepts in place of spaces.
/// `\text{}`, `\mathrm{}` and a bare `<` are all escaped correctly; this one
/// command is the hole.
const OPERATORNAME_XSS: &str = r"\operatorname{</math><img/src=x/onerror=alert(1)>}";

/// Nothing from the equation source may reach the markup as a *tag*, in any of
/// the places an equation is written into it.
///
/// The payload's characters may still appear — escaped, as visible text inside
/// the degraded equation — and that is the point: the check is that `<` and `>`
/// around them were escaped, so no element and no attribute was created.
fn assert_no_injected_markup(html: &str, where_: &str) {
    for needle in [
        "<img",
        "<script",
        "<iframe",
        // The payload's own `</math>`, which is what escapes the element.
        "</math><",
        // An event handler only exists once a raw `=` follows it in attribute
        // position; escaped text carries `onerror=alert(1)&gt;` instead.
        "onerror=alert(1)>",
    ] {
        assert!(
            !html.contains(needle),
            "{where_} carries {needle:?} from the equation source:\n{html}"
        );
    }
}

#[test]
fn operatorname_markup_injection_never_reaches_the_body() {
    let document = document_with_equation_block(OPERATORNAME_XSS);
    // Reachability, not self-XSS: a document carrying this passes validation,
    // so it arrives through import, collaboration or a shared repository.
    assert!(document.validate().is_ok(), "{:?}", document.validate());

    let rendering = render_document(&document, []);
    assert_no_injected_markup(&rendering.html, "the document body");
    // Degraded, loudly: the source is shown escaped and the failure is
    // reported rather than silently dropped.
    assert!(
        rendering.html.contains("equation-error"),
        "{}",
        rendering.html
    );
    assert!(
        rendering
            .html
            .contains("&lt;/math&gt;&lt;img/src=x/onerror=alert(1)&gt;"),
        "{}",
        rendering.html
    );
    assert_eq!(rendering.warnings.len(), 1, "{:?}", rendering.warnings);
    assert_eq!(rendering.warnings[0].code, WARNING_EQUATION_UNSAFE_MARKUP);
    assert!(rendering.warnings[0].validate().is_ok());
    // The rest of the document still renders.
    assert!(rendering.html.contains("before"), "{}", rendering.html);
}

#[test]
fn operatorname_markup_injection_never_reaches_a_fragment_or_an_export() {
    let document = document_with_equation_block(OPERATORNAME_XSS);
    let body = render_document_body(&document, []);
    assert_no_injected_markup(&body.html, "the body rendering");
    for fragment in &body.fragments {
        assert_no_injected_markup(&fragment.html, "a body fragment");
    }
    // `render_document_body` slices the same string, so a fragment that
    // carried the payload would reach `editor.ts`'s `template.innerHTML`.
    assert!(
        body.fragments
            .iter()
            .any(|fragment| fragment.html.contains("equation-error")),
        "{:?}",
        body.fragments.iter().map(|f| &f.html).collect::<Vec<_>>()
    );
    assert_no_injected_markup(
        &render_standalone_html(&document, []).html,
        "the HTML export",
    );
}

#[test]
fn operatorname_markup_injection_never_reaches_an_inline_equation_or_a_footnote() {
    let inline = document_with_inline_equation(OPERATORNAME_XSS);
    assert_no_injected_markup(&render_document(&inline, []).html, "an inline equation");

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
            equation: latex_equation("equation-inline-2", OPERATORNAME_XSS),
        }],
        deleted: false,
    });
    let rendering = render_footnotes(&document, []);
    assert_no_injected_markup(&rendering.html, "a footnote");
    assert_eq!(rendering.warnings.len(), 1, "{:?}", rendering.warnings);
    assert_eq!(rendering.warnings[0].code, WARNING_EQUATION_UNSAFE_MARKUP);
}
