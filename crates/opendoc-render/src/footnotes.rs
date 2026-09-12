//! Footnote numbering and the footnote list projection.

use crate::context::RenderContext;
use crate::html::attr;
use crate::*;

/// Render footnote bodies, numbered in order of first reference, discarding
/// projection warnings.
pub fn render_footnotes_html<'a>(
    document: &'a Document,
    images: impl IntoIterator<Item = RenderImage<'a>>,
) -> String {
    render_footnotes(document, images).html
}

/// Render footnote bodies, numbered in order of first reference, reporting
/// projection warnings. Footnote bodies may contain inline equations.
pub fn render_footnotes<'a>(
    document: &'a Document,
    images: impl IntoIterator<Item = RenderImage<'a>>,
) -> Rendering {
    let context = RenderContext::new(document, images);
    let mut numbered: Vec<(usize, &opendoc_core::Footnote)> = document
        .footnotes
        .iter()
        .filter(|footnote| !footnote.deleted)
        .filter_map(|footnote| {
            context
                .footnote_numbers
                .get(footnote.id.as_str())
                .map(|number| (*number, footnote))
        })
        .collect();
    numbered.sort_by_key(|(number, _)| *number);
    let mut out = String::new();
    if numbered.is_empty() {
        return Rendering::default();
    }
    out.push_str("<ol class=\"doc-footnotes\">");
    for (number, footnote) in numbered {
        let _ = write!(
            out,
            "<li class=\"doc-footnote\" value=\"{number}\" data-footnote-id=\"{}\">",
            attr(footnote.id.as_str())
        );
        for inline in &footnote.body {
            context.render_inline(inline, &mut out);
        }
        out.push_str("</li>");
    }
    out.push_str("</ol>");
    let warnings = context.into_warnings();
    Rendering {
        html: out,
        warnings,
    }
}

pub(crate) fn collect_footnote_numbers(block: &Block, numbers: &mut BTreeMap<String, usize>) {
    for inline in &block.content {
        if let Inline::FootnoteRef { footnote_id, .. } = inline {
            let next = numbers.len() + 1;
            numbers.entry(footnote_id.to_string()).or_insert(next);
        }
    }
    if let BlockKind::Table { rows, .. } = &block.kind {
        for row in rows {
            for cell in &row.cells {
                for nested in &cell.blocks {
                    collect_footnote_numbers(nested, numbers);
                }
            }
        }
    }
}
