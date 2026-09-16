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
    render_notes(document, images, false)
}

/// Render endnote bodies in their document-end trailer.
pub fn render_endnotes<'a>(
    document: &'a Document,
    images: impl IntoIterator<Item = RenderImage<'a>>,
) -> Rendering {
    render_notes(document, images, true)
}

fn render_notes<'a>(
    document: &'a Document,
    images: impl IntoIterator<Item = RenderImage<'a>>,
    endnotes: bool,
) -> Rendering {
    let context = RenderContext::new(document, images);
    let mut numbered: Vec<(usize, &opendoc_core::Footnote)> = document
        .footnotes
        .iter()
        .filter(|footnote| !footnote.deleted)
        .filter(|footnote| document.endnote_ids.contains(&footnote.id) == endnotes)
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
    out.push_str(if endnotes {
        "<ol class=\"doc-endnotes\" aria-label=\"Endnotes\">"
    } else {
        "<ol class=\"doc-footnotes\" aria-label=\"Footnotes\">"
    });
    for (number, footnote) in numbered {
        let _ = write!(
            out,
            "<li class=\"{}\" value=\"{number}\" data-footnote-id=\"{}\">",
            if endnotes {
                "doc-endnote"
            } else {
                "doc-footnote"
            },
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
