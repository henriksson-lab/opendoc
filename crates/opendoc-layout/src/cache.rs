//! Laying a document out again after one block changed.
//!
//! [`layout_document`](crate::layout_document) measures every block on every
//! call, which is the honest linear pass ADR 0014 argued for: at the time it
//! was written the browser's DOM work dominated a keystroke by an order of
//! magnitude, so the measuring was not worth caching. That is no longer true
//! — the DOM leg of a keystroke is 0.9 ms and this crate's pass is the larger
//! of the two — so the question is open again.
//!
//! ADR 0014's objection was specifically about the *key*:
//!
//! > A cache keyed on anything less than everything that decides a fragment —
//! > the block, the frame width, the list run state, the type scale — would
//! > trade this crate's one guarantee for a few milliseconds it no longer
//! > costs.
//!
//! The objection is right, and this cache answers it by **not having a key**.
//! A stored entry keeps the inputs themselves — the whole [`Block`], by
//! value, and the [`Frame`] it was laid out in — and is reused only when
//! those compare equal to the inputs of the pass asking. There is no digest
//! to collide, no field a future `Block` variant could add without this
//! noticing, and no run state to get wrong: the memoized unit is exactly
//! `text_fragment`/`block_fragment`, which [`crate::FragmentSource`] defines
//! as a function of the block, the frame, the type scale, the fonts, whether
//! the document carries suggestions and whether the engine is painting.
//! Everything a list *run* decides — the marker glyph, the ordinal, the
//! bottom margin a finished run inherits — is applied by the caller to the
//! value the source returns, so it is recomputed on every pass whether the
//! fragment was reused or not.
//!
//! The remaining inputs are global rather than per block, so they are stored
//! once and the whole table is dropped when any of them differs: the type
//! scale, whether the document carries suggestions, and the footnote
//! numbering (a reference draws its number, so the number is measured text). Painting is not among
//! them because the cache never paints — [`LayoutCache::layout`] builds a
//! non-painting engine, and `opendoc-pdf` goes through
//! [`layout_painted_document`](crate::layout_painted_document), which does not
//! consult a cache at all.
//!
//! What this buys, measured on a 1,500-block document (release, native):
//! a full pass is 1.2 ms and comparing every block of that document for
//! equality is 0.04 ms — 30x cheaper than measuring it. Typing changes one
//! block, so a cached pass measures one block and compares 1,500.
//!
//! **Determinism is unaffected, and that is checked rather than asserted.**
//! `cached_layout_is_identical_to_an_uncached_one` drives a family of
//! documents through a long sequence of edits and compares the cached result
//! against a freshly computed one field for field after every single edit.

use std::collections::{BTreeMap, HashMap};

use opendoc_core::{Block, Document};

use crate::font::Fonts;
use crate::style::TypeScale;
use crate::{engine, DocumentLayout, Fragment, FragmentSource, Frame};

/// How much of the last pass was answered from the cache.
///
/// Exposed so a test can prove the cache is actually being used — a cache
/// that silently stopped hitting would still be correct, and would therefore
/// never fail a correctness test.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CacheStats {
    /// Top-level blocks measured on this pass.
    pub measured: usize,
    /// Top-level blocks answered from the previous pass.
    pub reused: usize,
}

/// One block's inputs and the fragment they produced.
struct Entry {
    block: Block,
    frame: Frame,
    fragment: Fragment,
}

/// A reusable layout pass.
///
/// Holds the previous pass's per-block inputs and outputs, plus the parsed
/// faces — parsing is a table-directory walk and is cheap, but it is a fixed
/// cost per pass that a cached pass has no reason to pay again.
pub struct LayoutCache {
    fonts: Fonts,
    /// The scale the stored fragments were measured against. Compared, not
    /// assumed: today it is always `TypeScale::default()`, and the day it
    /// stops being so the cache must notice rather than serve heights from
    /// the old one.
    scale: TypeScale,
    suggestions_pending: bool,
    /// Footnote numbers by id. Document-wide like the scale: inserting a
    /// footnote renumbers every later reference, and a reference's number is
    /// text that is measured, so a stale table would serve widths for the
    /// numbers the document used to have.
    footnote_numbers: BTreeMap<String, u32>,
    toc_entries: Vec<(u8, String)>,
    bibliography_entries: Vec<String>,
    entries: Vec<Entry>,
    stats: CacheStats,
}

impl Default for LayoutCache {
    fn default() -> Self {
        Self::new()
    }
}

impl LayoutCache {
    pub fn new() -> Self {
        Self {
            fonts: Fonts::load(),
            scale: TypeScale::default(),
            suggestions_pending: false,
            footnote_numbers: BTreeMap::new(),
            toc_entries: Vec::new(),
            bibliography_entries: Vec::new(),
            entries: Vec::new(),
            stats: CacheStats::default(),
        }
    }

    /// What the last [`LayoutCache::layout`] call reused.
    pub fn stats(&self) -> CacheStats {
        self.stats
    }

    /// Forgets everything, so the next pass measures from scratch.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.stats = CacheStats::default();
    }

    /// Lays `document` out, reusing every block whose inputs are unchanged.
    ///
    /// Byte-identical to [`crate::layout_document`] for the same document,
    /// whatever the cache happens to be holding.
    pub fn layout(&mut self, document: &Document) -> DocumentLayout {
        let engine = engine(&self.fonts, document, false);
        // The two global inputs. A change in either invalidates every stored
        // height at once, so there is nothing to reconcile per block.
        if self.scale != engine.scale
            || self.suggestions_pending != engine.suggestions_pending
            || self.footnote_numbers != engine.footnote_numbers
            || self.toc_entries != engine.toc_entries
            || self.bibliography_entries != engine.bibliography_entries
        {
            self.scale = engine.scale.clone();
            self.suggestions_pending = engine.suggestions_pending;
            self.footnote_numbers = engine.footnote_numbers.clone();
            self.toc_entries = engine.toc_entries.clone();
            self.bibliography_entries = engine.bibliography_entries.clone();
            self.entries.clear();
        }
        let mut pass = Pass::new(std::mem::take(&mut self.entries));
        let layout = engine
            .layout(&document.blocks, &document.page_setup, &mut pass)
            .0;
        self.stats = pass.stats;
        self.entries = pass.next;
        layout
    }
}

/// One pass's consumption of the previous pass's entries.
struct Pass {
    /// The previous entries. An entry is taken out when it is reused, so a
    /// document that repeats a block id cannot reuse one entry twice.
    previous: Vec<Option<Entry>>,
    /// Built only when a lookup is not where the previous pass left it, which
    /// is what an inserted or deleted block causes. Typing into a block does
    /// not move anything, so the common case never pays for this.
    by_id: Option<HashMap<String, usize>>,
    /// Where to look first: the entry after the one last reused.
    cursor: usize,
    next: Vec<Entry>,
    stats: CacheStats,
}

impl Pass {
    fn new(previous: Vec<Entry>) -> Self {
        let capacity = previous.len();
        Self {
            previous: previous.into_iter().map(Some).collect(),
            by_id: None,
            cursor: 0,
            next: Vec::with_capacity(capacity),
            stats: CacheStats::default(),
        }
    }

    /// The stored entry for this block id, if one is left.
    fn locate(&mut self, id: &str) -> Option<usize> {
        if let Some(Some(entry)) = self.previous.get(self.cursor) {
            if entry.block.id.as_str() == id {
                return Some(self.cursor);
            }
        }
        let previous = &self.previous;
        let by_id = self.by_id.get_or_insert_with(|| {
            previous
                .iter()
                .enumerate()
                .filter_map(|(index, entry)| {
                    entry
                        .as_ref()
                        .map(|entry| (entry.block.id.to_string(), index))
                })
                .collect()
        });
        by_id.get(id).copied()
    }
}

impl FragmentSource for Pass {
    fn fragment(
        &mut self,
        block: &Block,
        frame: Frame,
        measure: &mut dyn FnMut() -> Fragment,
    ) -> Fragment {
        if let Some(index) = self.locate(block.id.as_str()) {
            // The inputs themselves, compared by value. `Block` is `Eq`, so
            // this is the whole block — kind, content, marks, properties — and
            // not a summary of it.
            let usable = self.previous[index]
                .as_ref()
                .is_some_and(|entry| entry.frame == frame && entry.block == *block);
            // The cursor moves past the entry whether or not it was usable.
            // A block the user is typing into is found where it was and
            // *fails* the comparison, and leaving the cursor behind on it
            // would send the next block — and so every block after it — down
            // the by-id path, which builds the map. That turned one keystroke
            // into a full table rebuild: 0.95 ms instead of 0.50 on a
            // 1,500-block document, most of it allocating 1,500 keys.
            self.cursor = index + 1;
            if usable {
                let entry = self.previous[index].take().expect("checked just above");
                let fragment = entry.fragment.clone();
                self.next.push(entry);
                self.stats.reused += 1;
                return fragment;
            }
        }
        let fragment = measure();
        self.next.push(Entry {
            block: block.clone(),
            frame,
            fragment: fragment.clone(),
        });
        self.stats.measured += 1;
        fragment
    }
}
