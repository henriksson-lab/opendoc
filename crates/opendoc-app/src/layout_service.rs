//! The document layout, projected for the shell.
//!
//! Pagination is computed in `opendoc-layout` from the document and its
//! `PageSetup` alone — see `docs/adr/0014-pagination-in-rust.md`. This module
//! only carries the answer across the command boundary: it adds no policy, no
//! measurement and no defaults.
//!
//! Two shapes here are deliberate:
//!
//! * **Positions are twips, margins are CSS.** `top_twips` and `height_twips`
//!   are the model's own unit, so a test (or a PDF writer) can assert on them
//!   without a conversion. The one value the frontend applies verbatim —
//!   the margin that opens a page — is projected as an exact CSS length in
//!   points, so the frontend does no arithmetic at all.
//! * **The page gutter is not in here.** The gap between sheets is a property
//!   of the screen, not of the document, so the layout describes pages that
//!   touch and the viewer adds `var(--page-gap)` where it wants one.

use serde::{Deserialize, Serialize};

use super::OpenDocApp;

/// Where one block landed.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppBlockPlacement {
    pub block_id: String,
    /// Zero-based page index.
    pub page: u32,
    /// Top of the border box in twips, measured down the stack of pages with
    /// no gutter between them.
    pub top_twips: i32,
    pub height_twips: i32,
    /// Line boxes the block's own text occupies; zero when its height is not
    /// a line count.
    pub lines: u32,
    /// Present on the block that opens a page: the `margin-top` to apply so
    /// the block starts exactly where the layout says it does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_break_margin: Option<String>,
    /// False when this placement rests on an estimated height.
    pub exact: bool,
}

/// The layout of the open document's body.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppDocumentLayout {
    pub page_count: u32,
    /// True when no block's height was estimated.
    pub exact: bool,
    /// The type scale as CSS custom properties. The stylesheet reads these
    /// instead of repeating the sizes the layout measured with, so the two
    /// cannot disagree.
    pub style: String,
    pub blocks: Vec<AppBlockPlacement>,
}

impl OpenDocApp {
    /// Lays the open document out into pages.
    ///
    /// Read-only and deterministic: it neither touches the document nor
    /// depends on anything outside it, so the browser, the native shell and a
    /// headless export all get the same answer.
    pub fn layout_document(&self) -> AppDocumentLayout {
        let layout = opendoc_layout::layout_document(&self.document);
        AppDocumentLayout {
            page_count: layout.page_count,
            exact: layout.exact,
            style: opendoc_layout::type_scale_css_variables(),
            blocks: layout
                .blocks
                .iter()
                .map(|placement| AppBlockPlacement {
                    block_id: placement.block_id.clone(),
                    page: placement.page,
                    top_twips: placement.top_twips,
                    height_twips: placement.height_twips,
                    lines: placement.lines,
                    page_break_margin: placement.page_break_css(),
                    exact: placement.exact,
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> OpenDocApp {
        OpenDocApp::new_empty_document()
    }

    #[test]
    fn a_new_document_lays_out_as_one_page() {
        let app = app();
        let layout = app.layout_document();
        assert_eq!(layout.page_count, 1);
        // A new document is one empty paragraph, which still occupies a line.
        assert_eq!(layout.blocks.len(), app.document().blocks.len());
        assert!(layout.blocks.iter().all(|block| block.page == 0));
        assert!(layout.style.contains("--doc-font-size"));
    }

    #[test]
    fn every_paragraph_gets_a_placement_in_document_order() {
        let mut app = app();
        for index in 0..5 {
            app.add_paragraph(format!("paragraph {index}"));
        }
        let document = app.document();
        let layout = app.layout_document();
        let placed: Vec<&str> = layout
            .blocks
            .iter()
            .map(|block| block.block_id.as_str())
            .collect();
        let expected: Vec<&str> = document
            .blocks
            .iter()
            .map(|block| block.id.as_str())
            .collect();
        assert_eq!(placed, expected);
    }

    #[test]
    fn a_long_document_breaks_onto_later_pages() {
        let mut app = app();
        for index in 0..80 {
            app.add_paragraph(format!(
                "Paragraph {index}: the quick brown fox jumps over the lazy dog."
            ));
        }
        let layout = app.layout_document();
        assert!(layout.page_count > 1, "80 paragraphs fit on one page?");
        let openers = layout
            .blocks
            .iter()
            .filter(|block| block.page_break_margin.is_some())
            .count();
        assert_eq!(
            openers as u32,
            layout.page_count - 1,
            "every page after the first is opened by exactly one block"
        );
    }

    #[test]
    fn the_layout_does_not_change_the_document() {
        let mut app = app();
        app.add_paragraph("a paragraph");
        let before = app.document();
        let _ = app.layout_document();
        assert_eq!(app.document().body_html, before.body_html);
        assert_eq!(
            app.document().has_unsaved_changes,
            before.has_unsaved_changes
        );
    }
}
