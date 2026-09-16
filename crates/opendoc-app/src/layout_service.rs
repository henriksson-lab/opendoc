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

use std::cell::RefCell;

use opendoc_layout::LayoutCache;
use serde::{Deserialize, Serialize};

use super::OpenDocApp;

thread_local! {
    /// The previous layout pass, kept so the next one only measures what
    /// changed.
    ///
    /// A thread local rather than a field on [`OpenDocApp`] because it is not
    /// application state: it holds no answer the app does not already have,
    /// and dropping it changes nothing but how long the next pass takes.
    /// `LayoutCache` establishes a stored fragment's validity by comparing
    /// the block and the frame it was measured in *by value*, so sharing one
    /// table between two documents — or between two `OpenDocApp`s cloned from
    /// each other — cannot produce a wrong answer, only a miss. See
    /// `opendoc_layout::cache`.
    ///
    /// It is also why `layout_document` can stay `&self`: the cache is not
    /// part of the document, and a read-only layout has no business needing a
    /// mutable borrow of the app.
    static LAYOUT_CACHE: RefCell<LayoutCache> = RefCell::new(LayoutCache::new());
}

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
    /// The `list-style-type` cycle for nested lists, as complete CSS rules
    /// scoped to `.doc-body`.
    ///
    /// Written by `opendoc_layout::list_style_type_rules` from the same
    /// `list_style_type` the painted page picks its markers with, for the
    /// reason `opendoc-render`'s standalone HTML already takes it from there:
    /// CSS cannot say "cycle for ever", so the cycle has a last rule, and a
    /// hand-written copy of it in the app stylesheet is a second statement of
    /// where that last rule falls. The two disagreeing means the screen
    /// showing a hollow circle where the PDF prints a square.
    pub list_style: String,
    pub blocks: Vec<AppBlockPlacement>,
}

impl OpenDocApp {
    /// Lays the open document out into pages.
    ///
    /// Read-only and deterministic: it neither touches the document nor
    /// depends on anything outside it, so the browser, the native shell and a
    /// headless export all get the same answer.
    pub fn layout_document(&self) -> AppDocumentLayout {
        // The cached pass is byte-identical to `opendoc_layout::layout_document`
        // for the same document — `a_cached_layout_is_identical_to_an_uncached_one`
        // drives 180 generated documents through 20 edits each and compares
        // the two after every one — so this is a speed decision and nothing
        // else. `try_with` rather than `with`: during thread teardown the
        // local is gone, and a layout is never worth a panic.
        let layout = LAYOUT_CACHE
            .try_with(|cache| cache.borrow_mut().layout(&self.document))
            .unwrap_or_else(|_| opendoc_layout::layout_document(&self.document));
        AppDocumentLayout {
            page_count: layout.page_count,
            exact: layout.exact,
            style: opendoc_layout::type_scale_css_variables(),
            list_style: opendoc_layout::list_style_type_rules(".doc-body "),
            // `into_iter`, so each block id is moved across rather than
            // copied: on a 1,500-block document the projection was allocating
            // a second string per block on every keystroke.
            blocks: layout
                .blocks
                .into_iter()
                .map(|placement| AppBlockPlacement {
                    page_break_margin: placement.page_break_css(),
                    block_id: placement.block_id,
                    page: placement.page,
                    top_twips: placement.top_twips,
                    height_twips: placement.height_twips,
                    lines: placement.lines,
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
            let _ = app.add_paragraph(format!("paragraph {index}"));
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
            let _ = app.add_paragraph(format!(
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

    /// The command's answer is the crate's answer, cache or no cache — and
    /// two apps taking turns on one thread share the cache, which is the
    /// arrangement that could serve one document's heights for another's.
    #[test]
    fn the_projected_layout_matches_an_uncached_one_for_every_app_on_the_thread() {
        let mut first = app();
        let mut second = app();
        for index in 0..40 {
            let _ =
                first.add_paragraph(format!("first {index}: the quick brown fox jumps over it"));
            let _ = second.add_paragraph(format!(
                "second {index}: a rather longer paragraph with more words in it than the other, \
                 long enough that it wraps onto a second line and cannot have the first one's height"
            ));
        }
        // The two documents are given the *same* block ids. Nothing produces
        // that on its own — ids are minted per document — and that is the
        // point: a cache that recognised a stored entry by its id alone would
        // hand one document's heights to the other, and there would be
        // nothing in a single-document test to show it.
        let ids: Vec<_> = first
            .document
            .blocks
            .iter()
            .map(|block| block.id.clone())
            .collect();
        for (block, id) in second.document.blocks.iter_mut().zip(ids) {
            block.id = id;
        }
        let expect = |app: &OpenDocApp| {
            let uncached = opendoc_layout::layout_document(&app.document);
            (
                uncached.page_count,
                uncached
                    .blocks
                    .iter()
                    .map(|block| {
                        (
                            block.block_id.clone(),
                            block.page,
                            block.top_twips,
                            block.height_twips,
                            block.lines,
                            block.page_break_css(),
                            block.exact,
                        )
                    })
                    .collect::<Vec<_>>(),
            )
        };
        let projected = |app: &OpenDocApp| {
            let layout = app.layout_document();
            (
                layout.page_count,
                layout
                    .blocks
                    .into_iter()
                    .map(|block| {
                        (
                            block.block_id,
                            block.page,
                            block.top_twips,
                            block.height_twips,
                            block.lines,
                            block.page_break_margin,
                            block.exact,
                        )
                    })
                    .collect::<Vec<_>>(),
            )
        };
        for round in 0..3 {
            assert_eq!(projected(&first), expect(&first), "round {round}, first");
            assert_eq!(projected(&second), expect(&second), "round {round}, second");
            let _ = first.add_paragraph(format!("another {round}"));
        }
    }

    /// The nested-list marker cycle reaches the app as rules, and says the
    /// things the app stylesheet used to say by hand.
    ///
    /// The expected values are the ones the deleted stylesheet block stated —
    /// `circle` at depth 1, `square` at depth 2, `lower-alpha` and
    /// `lower-roman` for the ordered side — written out here rather than read
    /// back from `opendoc_layout`, which is the code producing them.
    #[test]
    fn the_projected_layout_carries_the_nested_list_marker_cycle() {
        let app = app();
        let css = app.layout_document().list_style;
        for (selector, expected) in [
            (".doc-body ul.depth-1", "circle"),
            (".doc-body ul.depth-2", "square"),
            (".doc-body ol.depth-1", "lower-alpha"),
            (".doc-body ol.depth-2", "lower-roman"),
        ] {
            let rule = css
                .lines()
                .find(|line| line.contains(selector))
                .unwrap_or_else(|| panic!("no rule for {selector} in:\n{css}"));
            assert!(
                rule.contains(&format!("list-style-type: {expected};")),
                "{selector} should be {expected}, the rule was {rule}"
            );
        }
        // Depth 0 takes the initial value from the browser, so stating it
        // would be the stylesheet overriding a default with the default.
        assert!(
            !css.contains("ul.depth-0") && !css.contains("ol.depth-0"),
            "depth 0 is the initial marker and must not be restated:\n{css}"
        );
    }

    /// And the app stylesheet must not state the cycle a second time.
    ///
    /// This is the whole point of the field above: two statements of where a
    /// finite CSS cycle stops are two chances for the screen and the PDF to
    /// stop in different places. A copy reappearing in `styles.css` is the
    /// regression, and a test that only checked the projection could not see
    /// it.
    #[test]
    fn the_app_stylesheet_does_not_restate_the_marker_cycle() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|path| path.parent())
            .expect("opendoc-app should live under crates/")
            .join("apps/desktop/src/styles.css");
        let css = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
        // Comments describe the rule; only a real declaration is a second
        // statement of it, and every one of those needs a `depth-N` selector.
        let offenders = css
            .split('}')
            .filter(|block| block.contains("depth-") && block.contains("list-style-type:"))
            .collect::<Vec<_>>();
        assert!(
            offenders.is_empty(),
            "styles.css states the list marker cycle again; it comes from \
             AppDocumentLayout.list_style: {offenders:?}"
        );
    }

    #[test]
    fn the_layout_does_not_change_the_document() {
        let mut app = app();
        let _ = app.add_paragraph("a paragraph");
        let before = app.document();
        let _ = app.layout_document();
        assert_eq!(app.document().body_html(), before.body_html());
        assert_eq!(
            app.document().has_unsaved_changes,
            before.has_unsaved_changes
        );
    }
}
