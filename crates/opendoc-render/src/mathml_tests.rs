//! Unit tests for the MathML allowlist in [`crate::mathml`].
//!
//! The tests in `equation_tests.rs` prove the hole is closed end to end; these
//! prove the allowlist itself, including the cases `math_core` does not
//! currently produce but a future version could.

use crate::mathml::sanitize_mathml;

fn sanitized(markup: &str) -> String {
    sanitize_mathml(markup).unwrap_or_else(|err| panic!("{markup} was rejected: {}", err.0))
}

fn rejection(markup: &str) -> String {
    match sanitize_mathml(markup) {
        Ok(html) => panic!("{markup} was accepted as {html}"),
        Err(err) => err.0,
    }
}

#[test]
fn well_formed_mathml_survives_unchanged() {
    for markup in [
        "<math><semantics><mfrac><mi>a</mi><mi>b</mi></mfrac><annotation encoding=\"application/x-tex\">\\frac{a}{b}</annotation></semantics></math>",
        "<math display=\"block\"><mrow><mo lspace=\"0\" rspace=\"0\">=</mo></mrow></math>",
        "<math><mtable displaystyle=\"false\" scriptlevel=\"0\"><mtr><mtd><mi>a</mi></mtd></mtr></mtable></math>",
        "<math><merror class=\"math-core-unknown-cmd\"><mtext>\\nbsp</mtext></merror></math>",
        "<math><mtext><span class=\"math-core-serif-font\">x</span></mtext></math>",
    ] {
        assert_eq!(sanitized(markup), markup);
    }
}

#[test]
fn text_and_attribute_values_are_escaped_not_copied() {
    // `&amp;` decodes to `&` and must be written back as `&amp;`, never as a
    // raw ampersand that a later concatenation could turn into an entity.
    assert_eq!(
        sanitized("<math><mo>&amp;</mo></math>"),
        "<math><mo>&amp;</mo></math>"
    );
    assert_eq!(
        sanitized("<math><mo>&lt;</mo></math>"),
        "<math><mo>&lt;</mo></math>"
    );
    assert_eq!(
        sanitized("<math><mtext><![CDATA[<script>]]></mtext></math>"),
        "<math><mtext>&lt;script&gt;</mtext></math>"
    );
    assert_eq!(
        sanitized("<math><mo form=\"a&quot;b\">x</mo></math>"),
        "<math><mo form=\"a&quot;b\">x</mo></math>"
    );
}

#[test]
fn an_empty_element_becomes_a_pair_so_html_and_xhtml_agree() {
    assert_eq!(
        sanitized("<math><mspace width=\"1em\"/></math>"),
        "<math><mspace width=\"1em\"></mspace></math>"
    );
}

#[test]
fn the_operatorname_payload_is_rejected_as_malformed() {
    // What `math_core` 0.8.2 actually returns for
    // `\operatorname{</math><img/src=x/onerror=alert(1)>}`.
    let reason = rejection(
        "<math><semantics><mo lspace=\"0\" rspace=\"0\"></math><img/src=x/onerror=alert(1)></mo><annotation encoding=\"application/x-tex\">x</annotation></semantics></math>",
    );
    assert!(reason.contains("well-formed"), "{reason}");
}

#[test]
fn markup_that_is_well_formed_but_foreign_is_still_rejected() {
    // Well-formed XML is not enough: the allowlist is what refuses an element
    // that a browser would run.
    for (markup, needle) in [
        (
            "<math><mtext><img src=\"x\" onerror=\"alert(1)\"></img></mtext></math>",
            "<img>",
        ),
        (
            "<math><mtext><script>alert(1)</script></mtext></math>",
            "<script>",
        ),
        (
            "<math><mtext><iframe srcdoc=\"x\"></iframe></mtext></math>",
            "<iframe>",
        ),
        ("<math><foreignObject/></math>", "<foreignObject>"),
    ] {
        let reason = rejection(markup);
        assert!(reason.contains(needle), "{markup} -> {reason}");
    }
}

#[test]
fn event_handlers_and_urls_on_allowlisted_elements_are_rejected() {
    for (markup, needle) in [
        ("<math><mi onclick=\"alert(1)\">x</mi></math>", "onclick"),
        ("<math><mi onload=\"alert(1)\">x</mi></math>", "onload"),
        (
            "<math><mo definitionURL=\"javascript:alert(1)\">x</mo></math>",
            "definitionURL",
        ),
        ("<math><mtext src=\"x\">y</mtext></math>", "src"),
    ] {
        let reason = rejection(markup);
        assert!(reason.contains(needle), "{markup} -> {reason}");
    }
}

#[test]
fn only_fragment_links_survive_and_only_on_an_anchor() {
    assert_eq!(
        sanitized("<math><mtext><a href=\"#eq:1\">(1)</a></mtext></math>"),
        "<math><mtext><a href=\"#opendoc-math-eq:1\">(1)</a></mtext></math>"
    );
    for (markup, needle) in [
        (
            "<math><mtext><a href=\"https://evil.example/\">x</a></mtext></math>",
            "fragment reference",
        ),
        (
            "<math><mtext><a href=\"javascript:alert(1)\">x</a></mtext></math>",
            "fragment reference",
        ),
        ("<math><mi href=\"#a\">x</mi></math>", "href on <mi>"),
    ] {
        let reason = rejection(markup);
        assert!(reason.contains(needle), "{markup} -> {reason}");
    }
}

#[test]
fn ids_are_namespaced_so_an_equation_cannot_name_an_application_element() {
    // `\label{app}` makes `math_core` emit a bare `id="app"`, which is the id
    // of the frontend's own root element.
    assert_eq!(
        sanitized("<math><mtd id=\"app\"><mn>1</mn></mtd></math>"),
        "<math><mtd id=\"opendoc-math-app\"><mn>1</mn></mtd></math>"
    );
    assert!(rejection("<math><mtd id=\"a b\"><mn>1</mn></mtd></math>").contains("element id"));
    assert!(rejection("<math><mtd id=\"\"><mn>1</mn></mtd></math>").contains("element id"));
}

#[test]
fn style_is_rebuilt_from_allowlisted_declarations() {
    assert_eq!(
        sanitized("<math><mrow style=\"color:#FF0000;\"><mi>x</mi></mrow></math>"),
        "<math><mrow style=\"color:#FF0000;\"><mi>x</mi></mrow></math>"
    );
    // Whitespace is normalised away because the value is rebuilt, not copied.
    assert_eq!(
        sanitized(
            "<math><mtd style=\"text-align: right;justify-items: end;\"><mn>1</mn></mtd></math>"
        ),
        "<math><mtd style=\"text-align:right;justify-items:end;\"><mn>1</mn></mtd></math>"
    );
    assert_eq!(
        sanitized(
            "<math><mtd style=\"border-top: 0.05em solid currentcolor;\"><mn>1</mn></mtd></math>"
        ),
        "<math><mtd style=\"border-top:0.05em solid currentcolor;\"><mn>1</mn></mtd></math>"
    );
}

#[test]
fn style_values_that_could_fetch_or_escape_are_rejected() {
    for (markup, needle) in [
        (
            "<math><mrow style=\"background-color:url(https://evil.example/)\"><mi>x</mi></mrow></math>",
            "background-color",
        ),
        (
            "<math><mrow style=\"behavior:url(#x)\"><mi>x</mi></mrow></math>",
            "behavior",
        ),
        (
            "<math><mrow style=\"color:expression(alert(1))\"><mi>x</mi></mrow></math>",
            "color",
        ),
        (
            "<math><mrow style=\"color:red&quot;;x\"><mi>x</mi></mrow></math>",
            "color",
        ),
    ] {
        let reason = rejection(markup);
        assert!(reason.contains(needle), "{markup} -> {reason}");
    }
}

#[test]
fn classes_are_restricted_to_a_plain_class_list() {
    assert_eq!(
        sanitized("<math><mi class=\"a b-c_d\">x</mi></math>"),
        "<math><mi class=\"a b-c_d\">x</mi></math>"
    );
    assert!(rejection("<math><mi class=\"a&quot;b\">x</mi></math>").contains("class"));
}

#[test]
fn structure_outside_a_single_math_root_is_rejected() {
    for (markup, needle) in [
        ("<mi>x</mi>", "rooted at <mi>"),
        ("<math><mi>x</mi></math><math><mi>y</mi></math>", "root"),
        ("<math><mi>x</mi>", "unclosed"),
        ("", "no <math> element"),
        ("<math><!-- c --><mi>x</mi></math>", "comment"),
        ("<math><?pi?><mi>x</mi></math>", "processing instruction"),
        (
            "<?xml version=\"1.0\"?><math><mi>x</mi></math>",
            "XML declaration",
        ),
        (
            "<!DOCTYPE math><math><mi>x</mi></math>",
            "doctype declaration",
        ),
        ("<math><mtext>&nbsp;</mtext></math>", "unknown entity"),
    ] {
        let reason = rejection(markup);
        assert!(reason.contains(needle), "{markup:?} -> {reason}");
    }
}

#[test]
fn nesting_and_attribute_size_are_capped() {
    let deep = format!(
        "<math>{}{}</math>",
        "<mrow>".repeat(200),
        "</mrow>".repeat(200)
    );
    assert!(
        rejection(&deep).contains("nests deeper"),
        "{}",
        rejection(&deep)
    );

    let long = format!("<math><mi class=\"{}\">x</mi></math>", "a".repeat(300));
    assert!(
        rejection(&long).contains("longer than"),
        "{}",
        rejection(&long)
    );
}

#[test]
fn sanitising_is_idempotent() {
    let markup = "<math display=\"block\"><semantics><mtd id=\"eq\" style=\"width: 50%\"><mtext><a href=\"#eq\">(1)</a></mtext></mtd><annotation encoding=\"application/x-tex\">x</annotation></semantics></math>";
    let once = sanitized(markup);
    // The namespacing is applied again, which is correct — it is a rewrite, not
    // a marker — so idempotence is asserted on the *shape*, not on equality.
    let twice = sanitized(&once);
    assert!(
        twice.contains("id=\"opendoc-math-opendoc-math-eq\""),
        "{twice}"
    );
    assert!(sanitize_mathml(&twice).is_ok());
}
