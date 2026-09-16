//! HTML rendering of the document body. The frontend inserts this markup
//! into a single `contenteditable` host and maps DOM selections back to
//! block/inline ids using the `data-block-id` / `data-inline-id`
//! attributes, so every shell shares one renderer and the TypeScript layer
//! stays a thin DOM adapter.

mod context;
mod css;
mod equation;
mod footnotes;
mod html;
mod lists;
mod mathml;
mod standalone;
mod text;
mod workbook;

#[cfg(test)]
mod test_support;

#[cfg(test)]
mod css_tests;
#[cfg(test)]
mod document_tests;
#[cfg(test)]
mod endnote_tests;
#[cfg(test)]
mod equation_tests;
#[cfg(test)]
mod fragment_tests;
#[cfg(test)]
mod mathml_tests;
#[cfg(test)]
mod page_tests;
#[cfg(test)]
mod table_tests;
#[cfg(test)]
mod workbook_tests;

pub use footnotes::{render_endnotes, render_footnotes, render_footnotes_html};
pub use standalone::{render_plain_text, render_standalone_html};
pub use workbook::render_workbook_html;

pub use equation::{
    WARNING_EQUATION_RENDER_FAILED, WARNING_EQUATION_UNDEFINED_REFERENCE,
    WARNING_EQUATION_UNKNOWN_COMMAND, WARNING_EQUATION_UNSAFE_MARKUP,
};

use crate::context::RenderContext;
use crate::css::twips_to_css_pt;
use base64::Engine;
use equation::{render_equation, EquationDisplay, RenderedEquation};
use opendoc_core::{
    Anchor, Block, BlockKind, BlockProperties, Document, HeaderFooterSlot, ImageLayout, Inline,
    LineSpacing, ListKind, Mark, MarkKind, ModelWarning, PageSetup, StableId, Suggestion,
    SuggestionKind, SuggestionState,
};
use opendoc_spreadsheet::{Cell, SpreadsheetWorkbook};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

pub fn escape_html(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Image bytes available to the renderer, keyed by content hash.
pub struct RenderImage<'a> {
    pub hash: &'a str,
    pub media_type: &'a str,
    pub bytes: &'a [u8],
}

impl<'a> RenderImage<'a> {
    /// A copy of this reference, for a caller that has to hand the same image
    /// list to two renderers. Every field is already a borrow, so this copies
    /// three pointers and nothing else.
    pub fn borrowed(&self) -> RenderImage<'a> {
        RenderImage {
            hash: self.hash,
            media_type: self.media_type,
            bytes: self.bytes,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum RenderError {
    NotFound(String),
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for RenderError {}

/// Markup plus the warnings the projection produced.
///
/// The renderer is a pure projection: warnings are *returned*, never written
/// back into the document. Callers that surface warnings to the user append
/// them to their own projection state.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Rendering {
    pub html: String,
    pub warnings: Vec<ModelWarning>,
}

/// Render the document body as HTML, reporting projection warnings.
pub fn render_document<'a>(
    document: &'a Document,
    images: impl IntoIterator<Item = RenderImage<'a>>,
) -> Rendering {
    let context = RenderContext::new(document, images);
    let mut html = String::new();
    context.render_blocks(&document.blocks, &mut html);
    let warnings = context.into_warnings();
    Rendering { html, warnings }
}

/// One top-level element of the rendered body.
///
/// `html` is exactly the markup of that element — a `<p>`, a `<table>`, or a
/// whole list run — so a consumer can parse and apply one fragment on its
/// own. Concatenating every fragment's `html`, in order, reproduces the
/// whole body byte for byte; that is the property that makes updating one
/// block's DOM instead of the document's safe, and
/// `fragments_compose_to_the_whole_body` in `fragment_tests.rs` is what
/// pins it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BodyFragment {
    /// The first block this fragment renders — unique across the fragments
    /// of one body, since block ids are unique, so it is usable as the key
    /// a consumer matches its own state on.
    pub block_id: String,
    /// How many blocks this fragment covers. One for everything except a
    /// list run, which is a single element holding one block per item.
    pub blocks: usize,
    pub html: String,
}

/// The document body as one string *and* as the ordered fragments that
/// compose it, with the warnings the projection produced.
///
/// Both forms come out of one walk, so they cannot disagree: the fragments
/// are slices of the string that was rendered.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BodyRendering {
    pub html: String,
    pub fragments: Vec<BodyFragment>,
    pub warnings: Vec<ModelWarning>,
}

/// Render the document body as HTML *and* as its top-level fragments.
///
/// The whole-body [`render_document`] stays the cheaper path and keeps its
/// callers (HTML export, the PDF writer, the tests); this one is for a
/// consumer that wants to apply the body a piece at a time — a DOM that
/// should only touch the block that changed, for instance. It is the same
/// projection: the string it returns is the string `render_document` returns
/// for the same document.
pub fn render_document_body<'a>(
    document: &'a Document,
    images: impl IntoIterator<Item = RenderImage<'a>>,
) -> BodyRendering {
    let context = RenderContext::new(document, images);
    let mut html = String::new();
    let mut spans = Vec::new();
    context.render_blocks_recording(&document.blocks, &mut html, Some(&mut spans));
    let fragments = spans
        .into_iter()
        .map(|span| BodyFragment {
            block_id: span.block_id,
            blocks: span.blocks,
            html: html[span.start..span.end].to_string(),
        })
        .collect();
    let warnings = context.into_warnings();
    BodyRendering {
        html,
        fragments,
        warnings,
    }
}

/// Render the document body as HTML, discarding projection warnings.
pub fn render_document_html<'a>(
    document: &'a Document,
    images: impl IntoIterator<Item = RenderImage<'a>>,
) -> String {
    render_document(document, images).html
}

/// Render one page-furniture slot (header or footer) as HTML, reporting
/// projection warnings.
///
/// The markup is rendered **once**. Repeating it on every page is the job of
/// whatever paginates, because only a layout engine knows how many pages
/// there are; a renderer that emitted one copy per page would have to know
/// that, and would stop being a pure projection of the document. See ADR
/// 0009.
pub fn render_page_furniture<'a>(
    document: &'a Document,
    slot: HeaderFooterSlot,
    images: impl IntoIterator<Item = RenderImage<'a>>,
) -> Rendering {
    let blocks = document.furniture(slot);
    if blocks.is_empty() {
        return Rendering::default();
    }
    let context = RenderContext::new(document, images);
    let mut html = String::new();
    context.render_blocks(blocks, &mut html);
    let warnings = context.into_warnings();
    Rendering { html, warnings }
}

/// Render one page-furniture slot, discarding projection warnings.
pub fn render_page_furniture_html<'a>(
    document: &'a Document,
    slot: HeaderFooterSlot,
    images: impl IntoIterator<Item = RenderImage<'a>>,
) -> String {
    render_page_furniture(document, slot, images).html
}

/// Project page geometry as CSS custom properties, in `pt`.
///
/// `pt` rather than `px` for the same reason block lengths use it: 20 twips
/// *is* 1pt, so the projection is exact and no DPI has to be assumed. The
/// result is a declaration list suitable for a `style` attribute on whatever
/// element owns the page stack; the stylesheet reads the variables, so the
/// page's shape has exactly one source and it is the document.
///
/// `--page-content-width` and `--page-content-height` are derived here rather
/// than in CSS `calc()` so that a consumer measuring the page box gets the
/// same integer arithmetic the model uses.
pub fn page_setup_css_variables(setup: &PageSetup) -> String {
    let mut out = String::new();
    for (name, length) in [
        ("--page-width", setup.width),
        ("--page-height", setup.height),
        ("--page-margin-top", setup.margin_top),
        ("--page-margin-bottom", setup.margin_bottom),
        ("--page-margin-start", setup.margin_start),
        ("--page-margin-end", setup.margin_end),
        ("--page-margin-header", setup.margin_header),
        ("--page-margin-footer", setup.margin_footer),
        ("--page-content-width", setup.content_width()),
        ("--page-content-height", setup.content_height()),
    ] {
        let _ = write!(out, "{name}: {}; ", twips_to_css_pt(length.twips()));
    }
    let _ = write!(out, "--page-orientation: {};", setup.orientation().as_str());
    out
}

/// Project page geometry as a `@page` rule for printing.
///
/// Custom properties do not apply inside `@page` in any shipping engine, so
/// the print box cannot reuse `page_setup_css_variables`; the concrete
/// lengths have to be written out. Doing it here rather than in the frontend
/// keeps one projection of the page's shape instead of two that can disagree.
///
/// The margin is zero because the page boxes on screen already carry the
/// document's margins as padding: putting the margin in both places would
/// apply it twice.
pub fn page_setup_print_css(setup: &PageSetup) -> String {
    format!(
        "@page {{ size: {} {}; margin: 0; }}",
        twips_to_css_pt(setup.width.twips()),
        twips_to_css_pt(setup.height.twips())
    )
}
