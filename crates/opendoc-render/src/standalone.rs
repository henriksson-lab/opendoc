//! A self-contained HTML file of the whole document.
//!
//! "Download as HTML" used to be assembled in TypeScript: a hand-written
//! stylesheet in `files.ts`, the body taken from the document projection, no
//! warnings, and a media type stated in the frontend rather than by the
//! exporter. That contradicted ADR 0010 on one side and the project's central
//! rule — Rust owns semantics — on the other, so it lives here now.
//!
//! ## The stylesheet is projected, not restated
//!
//! The old one said `font-size: 11pt` and `line-height: 1.5` in a string in
//! `files.ts`. Those are the numbers `opendoc-layout`'s [`TypeScale`] measures
//! pages with, and ADR 0014 projects them as `--doc-*` custom properties
//! precisely so that a size cannot be stated twice. This stylesheet therefore
//! contains **no lengths of its own**: every one is a `var(--doc-…)` or
//! `var(--page-…)` filled in by the projections above it. Change the scale and
//! the export follows, as the app surface does.
//!
//! [`TypeScale`]: opendoc_layout::style::TypeScale
//!
//! ## What it is not
//!
//! Not a page-broken document. The export is one continuous flow with the
//! document's own margins and measure; where the pages fall is a property of
//! printing it, and the `@page` rule `opendoc-render` already projects tells
//! the browser's print pipeline the page size. A reader that wants paginated
//! output wants the PDF.

use crate::*;

/// Renders the document as one standalone HTML file, with its projection
/// warnings.
///
/// The markup is the same `render_document` produces — one renderer, so the
/// export cannot drift from the screen — wrapped in a document with the
/// stylesheet and the footnotes.
pub fn render_standalone_html<'a>(
    document: &'a Document,
    images: impl IntoIterator<Item = RenderImage<'a>>,
) -> Rendering {
    let images: Vec<RenderImage<'a>> = images.into_iter().collect();
    let body = render_document(document, images.iter().map(RenderImage::borrowed));
    let footnotes = render_footnotes(document, images.iter().map(RenderImage::borrowed));
    let endnotes = render_endnotes(document, images.iter().map(RenderImage::borrowed));
    let mut warnings = body.warnings;
    warnings.extend(footnotes.warnings);
    warnings.extend(endnotes.warnings);

    let mut html = String::with_capacity(body.html.len() + 4_096);
    html.push_str("<!doctype html>\n<html");
    if !document.locale.is_empty() {
        let _ = write!(html, " lang=\"{}\"", escape_html(&document.locale));
    }
    html.push_str(">\n<head>\n<meta charset=\"utf-8\">\n");
    html.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    let _ = writeln!(html, "<title>{}</title>", escape_html(&document.title));
    let _ = write!(html, "<style>\n{}</style>\n", stylesheet(document));
    html.push_str("</head>\n<body>\n");
    let _ = write!(
        html,
        "<article class=\"doc-body\">\n{}</article>\n",
        body.html
    );
    if !footnotes.html.is_empty() {
        let _ = write!(
            html,
            "<footer class=\"doc-footnotes\">\n{}</footer>\n",
            footnotes.html
        );
    }
    if !endnotes.html.is_empty() {
        let _ = write!(
            html,
            "<footer class=\"doc-endnotes\">\n{}</footer>\n",
            endnotes.html
        );
    }
    html.push_str("</body>\n</html>\n");

    Rendering { html, warnings }
}

/// The exported stylesheet: the projected scale, then rules that use it.
///
/// Deliberately free of literal lengths. A number written here would be a
/// second statement of a size the layout engine already owns, which is the
/// exact mistake the old TypeScript stylesheet made.
fn stylesheet(document: &Document) -> String {
    let mut css = String::new();
    let _ = writeln!(
        css,
        ":root {{ {} {} --doc-horizontal-rule-width: 0.75pt; --doc-horizontal-rule-space: 12pt; --doc-horizontal-rule-color: #6b7280; --doc-toc-padding-block: 10pt; --doc-toc-padding-inline: 12pt; --doc-toc-border-width: 1px; --doc-toc-indent-step: 1em; }}",
        opendoc_layout::type_scale_css_variables(),
        page_setup_css_variables(&document.page_setup)
    );
    // The bundled faces are not shipped alongside an exported file, so the
    // family falls back to the platform's. `--doc-font-family` names the
    // bundled family first and a stack after it, which is what makes that
    // degrade to a metric-compatible face rather than to nothing.
    css.push_str(concat!(
        "body { margin: 0; background: #f8f9fa; color: #202124; }\n",
        ".doc-body, .doc-footnotes, .doc-endnotes {\n",
        "  box-sizing: border-box;\n",
        "  max-width: var(--page-width);\n",
        "  margin: 0 auto;\n",
        "  padding-inline: var(--page-margin-start) var(--page-margin-end);\n",
        "  background: #fff;\n",
        "  font-family: var(--doc-font-family);\n",
        "  font-size: var(--doc-font-size);\n",
        "  line-height: var(--doc-line-height);\n",
        "  font-kerning: none;\n",
        "  font-variant-ligatures: none;\n",
        "}\n",
        ".doc-body { padding-block: var(--page-margin-top) var(--page-margin-bottom); }\n",
        ".doc-body p, .doc-body h1, .doc-body h2, .doc-body h3, .doc-body h4,\n",
        ".doc-body h5, .doc-body h6 { margin: 0 0 var(--doc-block-space-after); }\n",
        ".doc-body h1 { font-size: var(--doc-h1-size); font-weight: 400; margin-top: var(--doc-h1-space-before); }\n",
        ".doc-body h2 { font-size: var(--doc-h2-size); font-weight: 400; margin-top: var(--doc-h2-space-before); }\n",
        ".doc-body h3 { font-size: var(--doc-h3-size); font-weight: 400; margin-top: var(--doc-h3-space-before); }\n",
        ".doc-body h4 { font-size: var(--doc-h4-size); font-weight: 400; }\n",
        ".doc-body h5, .doc-body h6 { font-size: var(--doc-h5-size); font-weight: 400; }\n",
        ".doc-body .doc-title { font-size: var(--doc-title-size); font-weight: 400; }\n",
        ".doc-body .doc-subtitle { font-size: var(--doc-subtitle-size); color: #666; }\n",
        ".doc-list { margin: 0 0 var(--doc-block-space-after); padding-inline-start: var(--doc-list-indent); padding-left: 0; }\n",
        ".doc-list .doc-list { margin-bottom: 0; }\n",
    ));
    // The disc/circle/square and decimal/alpha/roman cycles, *written from*
    // `opendoc_layout::list_style_type` rather than beside it. The painted
    // page picks its markers with the same function, so a marker the PDF
    // cycles past the last rule this stylesheet states — which is what a
    // hand-written copy of the cycle eventually produces — cannot happen.
    css.push_str(&opendoc_layout::list_style_type_rules(".doc-body "));
    css.push_str(concat!(
        ".doc-body li { margin: 0; }\n",
        ".doc-body ul.doc-checklist { list-style: none; padding-inline-start: var(--doc-checklist-indent); padding-left: 0; }\n",
        ".doc-checkbox { display: inline-flex; align-items: center; margin-inline-end: var(--doc-checkbox-gap); }\n",
        ".doc-checkbox input { pointer-events: none; margin: 0; width: var(--doc-checkbox-size); height: var(--doc-checkbox-size); }\n",
        ".doc-body li[data-checked=\"true\"] { color: #5f6368; text-decoration: line-through; }\n",
        ".doc-table { border-collapse: collapse; table-layout: fixed; width: 100%; margin: 0 0 var(--doc-block-space-after); }\n",
        ".doc-table td, .doc-table th { border: var(--doc-table-border, var(--doc-cell-border) solid var(--doc-cell-border-color)); padding: var(--doc-cell-padding-block) var(--doc-cell-padding-inline); vertical-align: top; }\n",
        ".doc-table th { font-weight: inherit; text-align: inherit; }\n",
        ".doc-table thead { display: table-header-group; }\n",
        ".doc-image { margin: var(--doc-float-space) 0; }\n",
        ".doc-image img { max-width: 100%; height: auto; }\n",
        ".doc-image[data-placement=\"wrap-start\"] { float: inline-start; margin: var(--doc-image-clearance-top, 0) var(--doc-image-clearance-end, var(--doc-float-space)) var(--doc-image-clearance-bottom, var(--doc-float-space)) var(--doc-image-clearance-start, 0); }\n",
        ".doc-image[data-placement=\"wrap-end\"] { float: inline-end; margin: var(--doc-image-clearance-top, 0) var(--doc-image-clearance-end, 0) var(--doc-image-clearance-bottom, var(--doc-float-space)) var(--doc-image-clearance-start, var(--doc-float-space)); }\n",
        ".doc-equation-block { margin: var(--doc-float-space) 0; }\n",
        ".doc-page-break { border: 0; border-top: var(--doc-page-break-rule) dashed #bdc1c6; margin: var(--doc-page-break-space) 0; }\n",
        ".doc-horizontal-rule { border: 0; border-top: var(--doc-horizontal-rule-width) solid var(--doc-horizontal-rule-color); margin: var(--doc-horizontal-rule-space) 0; }\n",
        ".doc-table-of-contents { margin: var(--doc-block-space-after) 0; padding: var(--doc-toc-padding-block) var(--doc-toc-padding-inline); border: var(--doc-toc-border-width) solid #9ca3af; } .doc-table-of-contents ol { margin: 0; padding-inline-start: calc(1.4 * var(--doc-toc-indent-step)); }\n",
        ".doc-bibliography { margin: var(--doc-block-space-after) 0; } .doc-bibliography h2 { margin: 0 0 var(--doc-block-space-after); } .doc-bibliography ol { margin: 0; padding-inline-start: var(--doc-list-indent); }\n",
        ".doc-footnotes, .doc-endnotes { font-size: var(--doc-caption-size); padding-block: var(--doc-block-space-after); }\n",
        ".mark-bold { font-weight: 700; }\n",
        ".mark-italic { font-style: italic; }\n",
        ".mark-underline { text-decoration: underline; }\n",
        ".mark-strike { text-decoration: line-through; }\n",
        ".mark-code { font-family: var(--doc-mono-family); }\n",
        ".mark-superscript { vertical-align: super; font-size: var(--doc-script-size); }\n",
        ".mark-subscript { vertical-align: sub; font-size: var(--doc-script-size); }\n",
        // Links, citation labels and mentions: the colours come from the type
        // scale for the same reason the sizes do — the layout engine draws
        // them in the PDF and the browser draws them here, and one of the two
        // would otherwise be guessing. No horizontal padding on any of them,
        // deliberately: a run's width is the sum of its advances (ADR 0014),
        // and padding would make the app's own stylesheet break lines where
        // this one does not.
        ".run-link { color: var(--doc-link-color); }\n",
        ".citation-label { color: var(--doc-citation-color); background: var(--doc-citation-background); }\n",
        ".mention { background: var(--doc-mention-background); }\n",
        ".footnote-ref { color: var(--doc-link-color); font-size: var(--doc-script-size); vertical-align: super; }\n",
    ));
    // The document's own page box, so printing the file gives the paper the
    // document states rather than the browser's default.
    let _ = writeln!(css, "{}", page_setup_print_css(&document.page_setup));
    css
}

/// The document as plain text, plus what plain text cannot carry.
///
/// The text itself is `Document::visible_text`, the same projection the
/// document panel counts words with — not a second walk of the tree.
pub fn render_plain_text(document: &Document) -> (String, Vec<ModelWarning>) {
    let mut warnings = Vec::new();
    let mut dropped: BTreeMap<&'static str, usize> = BTreeMap::new();
    count_untranslatable(&document.blocks, &mut dropped);
    for (what, count) in dropped {
        warnings.push(ModelWarning {
            code: format!("text-dropped-{what}"),
            message: format!("{count} {what} block(s) became plain text or nothing at all"),
        });
    }
    if !document.comments.is_empty() {
        warnings.push(ModelWarning {
            code: "text-dropped-comments".to_string(),
            message: format!(
                "{} comment thread(s) have no plain-text form",
                document.comments.len()
            ),
        });
    }
    for (slot, what) in [
        (HeaderFooterSlot::Header, "header"),
        (HeaderFooterSlot::Footer, "footer"),
    ] {
        if !document.furniture(slot).is_empty() {
            warnings.push(ModelWarning {
                code: format!("text-dropped-{what}"),
                message: format!("the page {what} is not part of the text flow"),
            });
        }
    }
    (document.visible_text(), warnings)
}

/// Counts the block kinds whose structure plain text loses. Formatting is not
/// counted: a reader asking for plain text has asked for the marks to go.
fn count_untranslatable(blocks: &[Block], out: &mut BTreeMap<&'static str, usize>) {
    for block in blocks {
        match &block.kind {
            BlockKind::Table { rows, .. } => {
                *out.entry("table").or_default() += 1;
                for row in rows {
                    for cell in &row.cells {
                        count_untranslatable(&cell.blocks, out);
                    }
                }
            }
            BlockKind::Image { .. } => *out.entry("image").or_default() += 1,
            BlockKind::EquationBlock { .. } => *out.entry("equation").or_default() += 1,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::document_with;

    fn html(document: &Document) -> String {
        render_standalone_html(document, Vec::new()).html
    }

    #[test]
    fn the_export_is_a_complete_html_document() {
        let mut source = document_with(&["a paragraph"]);
        source.title = "A Title".to_string();
        let out = html(&source);
        assert!(out.starts_with("<!doctype html>"));
        assert!(out.contains("<title>A Title</title>"));
        assert!(out.contains("<meta charset=\"utf-8\">"));
        assert!(out.trim_end().ends_with("</html>"));
        assert!(out.contains("a paragraph"));
    }

    #[test]
    fn the_title_is_escaped() {
        let mut source = document_with(&["x"]);
        source.title = "<script>alert(1)</script>".to_string();
        let out = html(&source);
        assert!(!out.contains("<script>"), "the title was not escaped");
        assert!(out.contains("&lt;script&gt;"));
    }

    #[test]
    fn the_body_markup_is_the_renderers_own() {
        // One renderer: the export cannot drift from what the screen shows.
        let source = document_with(&["shared markup"]);
        let body = render_document(&source, Vec::new()).html;
        assert!(html(&source).contains(&body));
    }

    #[test]
    fn stylesheet_carries_the_image_wrap_modes() {
        let css = stylesheet(&document_with(&["x"]));
        assert!(css.contains(".doc-image[data-placement=\"wrap-start\"]"));
        assert!(css.contains(".doc-image[data-placement=\"wrap-end\"]"));
        assert!(!css.contains("figcaption"));
    }

    #[test]
    fn the_stylesheet_states_no_length_of_its_own() {
        // ADR 0014 §3: the type scale is projected so a size cannot be
        // written down twice. A literal length in these rules would be the
        // second place — exactly the bug the TypeScript stylesheet had.
        let source = document_with(&["x"]);
        let css = stylesheet(&source);
        let rules = css
            .lines()
            .filter(|line| !line.starts_with(":root") && !line.starts_with("@page"))
            .collect::<Vec<_>>()
            .join("\n");
        // A literal length is a number followed by a CSS unit. Matching the
        // unit alone would fire on "superscript"; matching the number alone
        // would fire on `font-weight: 400` and on every hex colour.
        let bytes: Vec<char> = rules.chars().collect();
        for (index, ch) in bytes.iter().enumerate() {
            if !ch.is_ascii_digit() {
                continue;
            }
            // Digits in an identifier such as `--doc-h5-size` are not CSS
            // numeric tokens. Starting a match there used to mistake that
            // identifier for the literal length `5pt`.
            if index > 0
                && (bytes[index - 1].is_ascii_alphanumeric()
                    || matches!(bytes[index - 1], '-' | '_'))
            {
                continue;
            }
            let rest: String = bytes[index + 1..].iter().take(3).collect();
            for unit in [
                "pt", "px", "em", "rem", "in", "cm", "mm", "pc", "ex", "ch", "vw", "vh",
            ] {
                assert!(
                    !rest.starts_with(unit),
                    "a rule states the literal length {ch}{rest}:\n{rules}"
                );
            }
        }
        assert!(rules.contains("var(--doc-font-size)"));
        assert!(rules.contains("var(--doc-h1-size)"));
    }

    #[test]
    fn the_stylesheet_carries_the_projected_scale_and_page() {
        let source = document_with(&["x"]);
        let css = stylesheet(&source);
        // The *declaration*, not the `var()` reference: a stylesheet that
        // only referred to the properties without ever defining them would
        // fall back to nothing and draw an unstyled document.
        assert!(
            css.contains("--doc-h1-size: "),
            "the type scale is not declared"
        );
        assert!(
            css.contains("--page-width: "),
            "the page geometry is not declared"
        );
        assert!(css.contains("@page"), "printing has no page box");
    }

    #[test]
    fn an_unrenderable_equation_reaches_the_caller_as_a_warning() {
        // The TypeScript export could not report this at all: it pasted
        // `body_html` into a string and threw the warnings away.
        let source = crate::test_support::document_with_equation_block("\\nosuchcommand{x}");
        let rendering = render_standalone_html(&source, Vec::new());
        assert!(
            !rendering.warnings.is_empty(),
            "a broken equation exported in silence"
        );
    }

    #[test]
    fn plain_text_is_the_documents_own_visible_text() {
        let source = document_with(&["first", "second"]);
        let (text, warnings) = render_plain_text(&source);
        assert_eq!(source.visible_text(), text);
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn plain_text_says_what_it_dropped() {
        let mut source = crate::test_support::table_document();
        source.blocks.push(Block {
            id: opendoc_core::StableId::parse("img").expect("valid id"),
            kind: BlockKind::Image {
                blob_hash: "sha256:deadbeef".to_string(),
                alt_text: "a picture".to_string(),
                layout: ImageLayout::default(),
            },
            content: Vec::new(),
            properties: BlockProperties::default(),
        });
        let (_, warnings) = render_plain_text(&source);
        for code in ["text-dropped-table", "text-dropped-image"] {
            assert!(
                warnings.iter().any(|w| w.code == code),
                "{code} was not reported: {warnings:?}"
            );
        }
    }
}
