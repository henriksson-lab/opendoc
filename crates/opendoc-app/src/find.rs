//! Find and replace over the rich document.
//!
//! Matching is a document question, so it lives here and not in the shell:
//! the frontend sends a query plus its option toggles and gets back
//! selections it can highlight, exactly the `{block_id, inline_id, offset}`
//! positions [`crate::editor`] already uses for the caret.
//!
//! Two properties are deliberate:
//!
//! * **A match may span inline runs.** A block stores "hello world" as two
//!   runs the moment half of it is bolded, so a per-run search silently
//!   misses "lo wo". The haystack is therefore the block's whole visible
//!   text ([`DocumentIndex::text_of`]), which is the same linear character
//!   space `Resolved::abs` is expressed in, and each match is mapped back to
//!   run-relative positions afterwards.
//! * **Offsets are characters, never bytes, at every boundary.** The `regex`
//!   crate reports byte offsets into a UTF-8 haystack; the editor's selection
//!   model counts Unicode scalar values. The conversion happens once, here,
//!   in [`BlockMatch::from_byte_range`] — everything downstream is in
//!   characters.
//!
//! Every search is one compiled [`Regex`]: a literal query is the escaped
//! pattern, so literal and regex searches cannot drift apart in whole-word or
//! case handling. The `regex` crate has no backtracking, so a user-supplied
//! pattern cannot make the app hang; it can only fail to compile, and that
//! surfaces as [`AppApiError::Format`].

use std::collections::HashMap;

use super::{AppApiError, AppDocument, OpenDocApp};
use crate::editor::{inline_stable_id, DocumentIndex, EditPlan, Resolved};
use opendoc_api::{AppFindMatch, AppFindMatches, AppFindRegion, EditorPosition, FindOptions};
use opendoc_core::{Block, BlockKind, Footnote, HeaderFooterSlot, Inline, StableId};
use opendoc_merge::OperationKind;
use regex::{Regex, RegexBuilder};

/// Caps on the compiled program so a pathological pattern costs memory that
/// is bounded rather than unbounded. Exceeding them is a compile error, which
/// reaches the user as an invalid-pattern message like any other.
const REGEX_SIZE_LIMIT: usize = 1 << 20;
const REGEX_DFA_SIZE_LIMIT: usize = 1 << 20;

/// One match in the linear character space of a single block.
#[derive(Clone, Debug, Eq, PartialEq)]
struct BlockMatch {
    block: usize,
    /// Character offset of the first matched character within the block.
    start: usize,
    /// Character offset one past the last matched character.
    end: usize,
    text: String,
}

impl BlockMatch {
    /// Convert one regex hit, reported in **byte** offsets into `haystack`,
    /// into the **character** offsets the editor's selection model uses.
    fn from_byte_range(block: usize, haystack: &str, byte_start: usize, byte_end: usize) -> Self {
        let start = haystack[..byte_start].chars().count();
        let matched = &haystack[byte_start..byte_end];
        Self {
            block,
            start,
            end: start + matched.chars().count(),
            text: matched.to_string(),
        }
    }
}

/// Compile `options` into the one regex that drives both find and replace.
///
/// Returns `None` for an empty query: "no query" is not "no matches for a
/// pattern", and an empty pattern would otherwise match everywhere.
fn compile(options: &FindOptions) -> Result<Option<Regex>, AppApiError> {
    if options.query.is_empty() {
        return Ok(None);
    }
    // A literal query is escaped rather than matched by a separate code
    // path, so `whole_word` and case folding behave identically in both
    // modes and an escaped literal can never fail to compile.
    let pattern = if options.regex {
        options.query.clone()
    } else {
        regex::escape(&options.query)
    };
    let pattern = if options.whole_word {
        format!(r"\b(?:{pattern})\b")
    } else {
        pattern
    };
    RegexBuilder::new(&pattern)
        .case_insensitive(!options.match_case)
        .size_limit(REGEX_SIZE_LIMIT)
        .dfa_size_limit(REGEX_DFA_SIZE_LIMIT)
        .build()
        .map(Some)
        .map_err(|error| {
            AppApiError::Format(format!(
                "find pattern {:?} is not a valid regular expression: {error}",
                options.query
            ))
        })
}

/// Every block in the document, nested table-cell blocks included, by id.
///
/// Searching needs each block's text, and the text lives on the block rather
/// than in the index, so the index's `text_of` re-finds the block by walking
/// the document from the top — a linear scan inside a loop over every block.
/// That made a search quadratic in the document: 1.4 ms for 750 blocks,
/// 5.3 ms for 1,500 and 19.1 ms for 3,000 natively, and 20.7 ms for 1,500 in
/// the browser, where the find bar re-asks on every keystroke. Building the
/// map once makes the same search linear.
///
/// Keyed by id rather than by position on purpose: `DocumentIndex` flattens
/// nested blocks in its own order, and a second copy of that order here would
/// be a thing to keep in step. An id lookup does not care what the order is.
fn blocks_by_id(blocks: &[Block]) -> HashMap<&str, &Block> {
    let mut out = HashMap::new();
    collect_blocks(blocks, &mut out);
    out
}

fn collect_blocks<'a>(blocks: &'a [Block], out: &mut HashMap<&'a str, &'a Block>) {
    for block in blocks {
        out.insert(block.id.as_str(), block);
        if let BlockKind::Table { rows, .. } = &block.kind {
            for row in rows {
                for cell in &row.cells {
                    collect_blocks(&cell.blocks, out);
                }
            }
        }
    }
}

/// One block's visible text: every run concatenated, with an atomic inline
/// (an equation, a footnote reference) standing in as one object-replacement
/// character so it occupies exactly the one position the selection model
/// gives it.
///
/// This is `DocumentIndex::text_of`'s rule, stated here because the map above
/// replaces the lookup it did. The two are pinned together by
/// `block_text_is_what_the_document_index_would_have_produced`, so the
/// duplication cannot drift silently.
fn block_text(block: &Block) -> String {
    let mut out = String::new();
    for inline in &block.content {
        match inline {
            Inline::Text { text, .. } | Inline::Link { text, .. } => out.push_str(text),
            _ => out.push('\u{FFFC}'),
        }
    }
    out
}

/// A part of the document that is searchable but not part of `blocks`.
///
/// `DocumentIndex` walks `document.blocks`, and page furniture plus footnote
/// bodies are all kept outside it — so a search saw none of them, and a
/// replace-all left a running head still saying the old thing while reporting
/// that it had replaced everything.
///
/// They are not simply more blocks, either. `opendoc-merge` resolves a
/// block- or inline-addressed operation only inside `document.blocks`
/// (ADR 0009 says so outright: "header blocks are not reachable by the
/// block-addressed operations"), so an edit here is a **whole-slot
/// replacement** — `SetPageFurniture` for every explicit furniture slot,
/// `UpsertFootnote` for a note. That is why this is a second path rather
/// than a wider `DocumentIndex`.
#[derive(Clone, Debug, Eq, PartialEq)]
enum Region {
    Furniture(HeaderFooterSlot),
    /// One note, by id. Its body is a run list, not a block list.
    Footnote(StableId),
}

/// One match in a region, as a character range in one of its run lists.
#[derive(Clone, Debug, Eq, PartialEq)]
struct RegionMatch {
    region: Region,
    /// Index of the run list within the region.
    part: usize,
    /// The id that addresses that run list: a block id for the furniture, the
    /// note's own id for a footnote body.
    id: StableId,
    start: usize,
    end: usize,
    text: String,
}

/// Either kind of match, in the one order both `find` and `replace` use.
#[derive(Clone, Debug, Eq, PartialEq)]
enum Found {
    Body(BlockMatch),
    Region(RegionMatch),
}

/// The run lists of one region, in order.
fn region_parts(app: &OpenDocApp, region: &Region) -> Vec<(StableId, Vec<Inline>)> {
    match region {
        Region::Furniture(slot) => app
            .document
            .furniture(*slot)
            .iter()
            .map(|block| (block.id.clone(), block.content.clone()))
            .collect(),
        Region::Footnote(id) => app
            .document
            .footnotes
            .iter()
            .filter(|footnote| !footnote.deleted && footnote.id == *id)
            .map(|footnote| (footnote.id.clone(), footnote.body.clone()))
            .collect(),
    }
}

/// Every searchable region outside the body, in the order a search reports
/// them: ordinary furniture, explicit first/even overrides, then notes by id.
/// An absent override inherits ordinary furniture, so it must not be searched
/// a second time under a fictional variant region.
///
/// The body comes first in the combined list, ahead of all of these, because
/// that is where the caret is and where a match can be highlighted; putting
/// the header first would renumber every match a user already knows.
fn regions(app: &OpenDocApp) -> Vec<Region> {
    let mut regions = vec![
        Region::Furniture(HeaderFooterSlot::Header),
        Region::Furniture(HeaderFooterSlot::Footer),
    ];
    regions.extend(
        [
            HeaderFooterSlot::FirstPageHeader,
            HeaderFooterSlot::FirstPageFooter,
            HeaderFooterSlot::EvenPageHeader,
            HeaderFooterSlot::EvenPageFooter,
        ]
        .into_iter()
        .filter(|slot| app.document.has_furniture_override(*slot))
        .map(Region::Furniture),
    );
    regions.extend(
        app.document
            .footnotes
            .iter()
            .filter(|footnote| !footnote.deleted)
            .map(|footnote| Region::Footnote(footnote.id.clone())),
    );
    regions
}

/// One run list's visible text, on `block_text`'s rule.
fn runs_text(runs: &[Inline]) -> String {
    let mut out = String::new();
    for inline in runs {
        match inline {
            Inline::Text { text, .. } | Inline::Link { text, .. } => out.push_str(text),
            _ => out.push('\u{FFFC}'),
        }
    }
    out
}

/// A character offset in a run list, as the position that addresses it.
///
/// `DocumentIndex::position`'s rule, for a run list that has no index: the
/// first editable run whose range contains `abs`, so a boundary names the end
/// of the preceding run.
fn position_in(runs: &[Inline], id: &StableId, abs: usize) -> EditorPosition {
    let mut cursor = 0usize;
    let mut fallback: Option<(&Inline, usize)> = None;
    for inline in runs {
        let (len, editable) = match inline {
            Inline::Text { text, .. } | Inline::Link { text, .. } => (text.chars().count(), true),
            _ => (1, false),
        };
        if abs >= cursor && abs <= cursor + len {
            if editable {
                return EditorPosition {
                    block_id: id.to_string(),
                    inline_id: Some(inline_stable_id(inline).to_string()),
                    offset: abs - cursor,
                };
            }
            fallback.get_or_insert((inline, abs - cursor));
        }
        cursor += len;
    }
    match fallback {
        Some((inline, offset)) => EditorPosition {
            block_id: id.to_string(),
            inline_id: Some(inline_stable_id(inline).to_string()),
            offset,
        },
        None => EditorPosition {
            block_id: id.to_string(),
            inline_id: None,
            offset: abs,
        },
    }
}

/// Replaces the characters in `[start, end)` of `runs` with `replacement`.
///
/// The replacement lands in the first editable run the range touches, so it
/// keeps that run's marks — the same answer typing over a selection gives.
/// A run the range covers entirely is removed, atomic inlines included.
fn replace_runs_range(runs: &mut Vec<Inline>, start: usize, end: usize, replacement: &str) {
    let mut cursor = 0usize;
    let mut placed = false;
    let mut kept: Vec<Inline> = Vec::with_capacity(runs.len());
    for inline in runs.drain(..) {
        let len = match &inline {
            Inline::Text { text, .. } | Inline::Link { text, .. } => text.chars().count(),
            _ => 1,
        };
        let run_start = cursor;
        let run_end = cursor + len;
        cursor = run_end;
        if run_end <= start || run_start >= end {
            kept.push(inline);
            continue;
        }
        let local_from = start.saturating_sub(run_start);
        let local_to = end.min(run_end) - run_start;
        match inline {
            Inline::Text {
                id,
                ref text,
                ref marks,
            } => {
                let mut next: String = text.chars().take(local_from).collect();
                if !placed {
                    next.push_str(replacement);
                    placed = true;
                }
                next.extend(text.chars().skip(local_to));
                if next.is_empty() && runs_would_be_empty(&kept) {
                    // Never leave a run list with nothing in it: an empty
                    // header block and an empty footnote body are both
                    // refused by `validate`.
                    kept.push(Inline::Text {
                        id,
                        text: next,
                        marks: marks.clone(),
                    });
                } else if !next.is_empty() {
                    kept.push(Inline::Text {
                        id,
                        text: next,
                        marks: marks.clone(),
                    });
                }
            }
            Inline::Link {
                id,
                ref text,
                ref href,
                ref marks,
            } => {
                let mut next: String = text.chars().take(local_from).collect();
                if !placed {
                    next.push_str(replacement);
                    placed = true;
                }
                next.extend(text.chars().skip(local_to));
                if !next.is_empty() {
                    kept.push(Inline::Link {
                        id,
                        text: next,
                        href: href.clone(),
                        marks: marks.clone(),
                    });
                }
            }
            // An atomic inside the match is replaced along with it.
            _ => {}
        }
    }
    if !placed && !replacement.is_empty() {
        kept.push(Inline::text(replacement));
    }
    if kept.is_empty() {
        kept.push(Inline::text(""));
    }
    *runs = kept;
}

fn runs_would_be_empty(kept: &[Inline]) -> bool {
    kept.is_empty()
}

/// Every match outside the body, in region order.
fn region_matches(
    app: &OpenDocApp,
    options: &FindOptions,
) -> Result<Vec<RegionMatch>, AppApiError> {
    let Some(regex) = compile(options)? else {
        return Ok(Vec::new());
    };
    let mut matches = Vec::new();
    for region in regions(app) {
        for (part, (id, runs)) in region_parts(app, &region).into_iter().enumerate() {
            let haystack = runs_text(&runs);
            for hit in regex.find_iter(&haystack) {
                if hit.start() == hit.end() {
                    continue;
                }
                let start = haystack[..hit.start()].chars().count();
                let text = haystack[hit.start()..hit.end()].to_string();
                matches.push(RegionMatch {
                    region: region.clone(),
                    part,
                    id: id.clone(),
                    start,
                    end: start + text.chars().count(),
                    text,
                });
            }
        }
    }
    Ok(matches)
}

/// Body matches and region matches, in the one order every caller uses.
fn all_matches(
    app: &OpenDocApp,
    index: &DocumentIndex,
    options: &FindOptions,
) -> Result<Vec<Found>, AppApiError> {
    let mut found: Vec<Found> = block_matches(app, index, options)?
        .into_iter()
        .map(Found::Body)
        .collect();
    found.extend(region_matches(app, options)?.into_iter().map(Found::Region));
    Ok(found)
}

/// Every match, in document order, as character ranges inside a block.
fn block_matches(
    app: &OpenDocApp,
    index: &DocumentIndex,
    options: &FindOptions,
) -> Result<Vec<BlockMatch>, AppApiError> {
    let Some(regex) = compile(options)? else {
        return Ok(Vec::new());
    };
    let by_id = blocks_by_id(&app.document.blocks);
    let mut matches = Vec::new();
    for block in 0..index.blocks.len() {
        if !index.blocks[block].text_block {
            continue;
        }
        // The whole block, runs concatenated: this is what makes a match
        // across a formatting boundary findable at all.
        let haystack = by_id
            .get(index.blocks[block].id.as_str())
            .map(|block| block_text(block))
            .unwrap_or_default();
        for hit in regex.find_iter(&haystack) {
            // A zero-length hit (`a*`, `^`) selects nothing and would replace
            // nothing, so it is not a match a user can act on.
            if hit.start() == hit.end() {
                continue;
            }
            matches.push(BlockMatch::from_byte_range(
                block,
                &haystack,
                hit.start(),
                hit.end(),
            ));
        }
    }
    Ok(matches)
}

/// Every match as a selection the frontend can highlight.
fn find(app: &OpenDocApp, options: &FindOptions) -> Result<AppFindMatches, AppApiError> {
    let index = DocumentIndex::build(&app.document.blocks);
    let matches = all_matches(app, &index, options)?
        .into_iter()
        .map(|found| match found {
            Found::Body(found) => AppFindMatch {
                start: index.position(Resolved {
                    block: found.block,
                    abs: found.start,
                }),
                end: index.position(Resolved {
                    block: found.block,
                    abs: found.end,
                }),
                text: found.text,
                region: AppFindRegion::Body,
            },
            Found::Region(found) => {
                let runs = region_parts(app, &found.region)
                    .into_iter()
                    .nth(found.part)
                    .map(|(_, runs)| runs)
                    .unwrap_or_default();
                AppFindMatch {
                    start: position_in(&runs, &found.id, found.start),
                    end: position_in(&runs, &found.id, found.end),
                    text: found.text,
                    region: match found.region {
                        Region::Furniture(HeaderFooterSlot::Header) => AppFindRegion::Header,
                        Region::Furniture(HeaderFooterSlot::Footer) => AppFindRegion::Footer,
                        Region::Furniture(HeaderFooterSlot::FirstPageHeader) => {
                            AppFindRegion::FirstPageHeader
                        }
                        Region::Furniture(HeaderFooterSlot::FirstPageFooter) => {
                            AppFindRegion::FirstPageFooter
                        }
                        Region::Furniture(HeaderFooterSlot::EvenPageHeader) => {
                            AppFindRegion::EvenPageHeader
                        }
                        Region::Furniture(HeaderFooterSlot::EvenPageFooter) => {
                            AppFindRegion::EvenPageFooter
                        }
                        Region::Footnote(_) => AppFindRegion::Footnote,
                    },
                }
            }
        })
        .collect();
    Ok(AppFindMatches { matches })
}

pub(crate) struct FindService<'a> {
    app: &'a mut OpenDocApp,
}

impl<'a> FindService<'a> {
    pub(crate) fn new(app: &'a mut OpenDocApp) -> Self {
        Self { app }
    }

    pub(crate) fn replace_match(
        &mut self,
        options: &FindOptions,
        replacement: &str,
        match_index: usize,
    ) -> Result<AppDocument, AppApiError> {
        let index = DocumentIndex::build(&self.app.document.blocks);
        let matches = all_matches(self.app, &index, options)?;
        let Some(target) = matches.get(match_index).cloned() else {
            return Err(AppApiError::NotFound(format!(
                "match {match_index} was not found; the search has {} match(es)",
                matches.len()
            )));
        };
        self.apply_replacements(index, &[target], replacement)
    }

    pub(crate) fn replace_all(
        &mut self,
        options: &FindOptions,
        replacement: &str,
    ) -> Result<AppDocument, AppApiError> {
        let index = DocumentIndex::build(&self.app.document.blocks);
        let matches = all_matches(self.app, &index, options)?;
        self.apply_replacements(index, &matches, replacement)
    }

    /// Turn matches into one batch of journalled operations.
    ///
    /// One batch, not one per match: the whole replace-all is a single
    /// dispatch, so it is a single undo checkpoint, and every offset in the
    /// batch is expressed against the same pre-edit snapshot the way
    /// [`EditPlan`] requires. Matches are replaced last-first so that a
    /// replacement of a different length can never shift the coordinates of
    /// one still to be planned.
    fn apply_replacements(
        &mut self,
        index: DocumentIndex,
        matches: &[Found],
        replacement: &str,
    ) -> Result<AppDocument, AppApiError> {
        if matches.is_empty() {
            return Ok(self.app.document());
        }
        let mut plan = EditPlan::new(self.app, index);
        for found in matches.iter().rev() {
            let Found::Body(found) = found else {
                continue;
            };
            let from = Resolved {
                block: found.block,
                abs: found.start,
            };
            let to = Resolved {
                block: found.block,
                abs: found.end,
            };
            plan.delete_range(from, to);
            plan.insert_text(from, replacement);
        }
        let mut ops = plan.ops;
        ops.extend(self.region_operations(matches, replacement));
        if ops.is_empty() {
            return Ok(self.app.document());
        }
        self.app.apply_batch(ops)
    }

    /// The whole-slot operations that carry the replacements outside the
    /// body.
    ///
    /// One operation per region touched, not one per match: a header slot and
    /// a footnote body are each replaced whole (ADR 0009), so every match in
    /// one of them has to be folded into a single new value. They join the
    /// body's operations in the same batch, so a replace-all over a document
    /// and its header is still exactly one undo step.
    fn region_operations(
        &self,
        matches: &[Found],
        replacement: &str,
    ) -> Vec<(&'static str, &'static str, OperationKind)> {
        let mut ops = Vec::new();
        for region in regions(self.app) {
            let mut touched = matches
                .iter()
                .filter_map(|found| match found {
                    Found::Region(found) if found.region == region => Some(found),
                    _ => None,
                })
                .collect::<Vec<_>>();
            if touched.is_empty() {
                continue;
            }
            // Last first, so a replacement of a different length cannot shift
            // the coordinates of one still to be applied.
            touched.sort_by_key(|found| (found.part, found.start));
            let mut parts = region_parts(self.app, &region)
                .into_iter()
                .map(|(_, runs)| runs)
                .collect::<Vec<_>>();
            for found in touched.into_iter().rev() {
                let Some(runs) = parts.get_mut(found.part) else {
                    continue;
                };
                replace_runs_range(runs, found.start, found.end, replacement);
            }
            match &region {
                Region::Furniture(slot) => {
                    let source = self.app.document.furniture(*slot);
                    let blocks = source
                        .iter()
                        .zip(parts)
                        .map(|(block, runs)| {
                            let mut block = block.clone();
                            block.content = runs;
                            block
                        })
                        .collect::<Vec<_>>();
                    ops.push((
                        "set-page-furniture",
                        "replace in page furniture",
                        OperationKind::SetPageFurniture {
                            slot: *slot,
                            blocks,
                        },
                    ));
                }
                Region::Footnote(id) => {
                    let Some(body) = parts.into_iter().next() else {
                        continue;
                    };
                    ops.push((
                        "upsert-footnote",
                        "replace in footnote body",
                        OperationKind::UpsertFootnote {
                            footnote: Footnote {
                                id: id.clone(),
                                // A whole-slot write is last-writer-wins by
                                // revision, so it has to claim a revision the
                                // stored one cannot already be at.
                                revision: self.app.next_envelope_seq,
                                body,
                                deleted: false,
                            },
                        },
                    ));
                }
            }
        }
        ops
    }
}

impl OpenDocApp {
    pub fn find_in_document(&self, options: &FindOptions) -> Result<AppFindMatches, AppApiError> {
        find(self, options)
    }

    pub fn replace_match_in_document(
        &mut self,
        options: &FindOptions,
        replacement: impl AsRef<str>,
        match_index: usize,
    ) -> Result<AppDocument, AppApiError> {
        FindService::new(self).replace_match(options, replacement.as_ref(), match_index)
    }

    pub fn replace_all_in_document(
        &mut self,
        options: &FindOptions,
        replacement: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        FindService::new(self).replace_all(options, replacement.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EditorMarkInput, EditorPosition, EditorSelection};
    use opendoc_core::Block;
    use serde_json::json;

    fn app_with(paragraphs: &[&str]) -> OpenDocApp {
        let mut app = OpenDocApp::new_sample();
        app.new_document("Find");
        app.document.blocks.clear();
        for text in paragraphs {
            app.document.blocks.push(Block::paragraph(*text));
        }
        app
    }

    fn options(query: &str) -> FindOptions {
        FindOptions {
            query: query.to_string(),
            ..FindOptions::default()
        }
    }

    fn texts(app: &OpenDocApp) -> Vec<String> {
        app.document
            .blocks
            .iter()
            .map(|block| {
                block
                    .content
                    .iter()
                    .map(|inline| match inline {
                        opendoc_core::Inline::Text { text, .. }
                        | opendoc_core::Inline::Link { text, .. } => text.as_str(),
                        _ => "\u{FFFC}",
                    })
                    .collect::<String>()
            })
            .collect()
    }

    fn run_count(app: &OpenDocApp, block: usize) -> usize {
        app.document.blocks[block].content.len()
    }

    /// Bold half of the first paragraph, which is what makes the document
    /// store it as two inline runs.
    fn bold(app: &mut OpenDocApp, from: usize, to: usize) {
        let block = &app.document.blocks[0];
        let inline_id = match &block.content[0] {
            opendoc_core::Inline::Text { id, .. } => id.to_string(),
            other => panic!("expected a text run, got {other:?}"),
        };
        let position = |offset: usize| EditorPosition {
            block_id: app.document.blocks[0].id.to_string(),
            inline_id: Some(inline_id.clone()),
            offset,
        };
        let selection = EditorSelection {
            anchor: position(from),
            focus: position(to),
        };
        app.apply_editor_mark(EditorMarkInput {
            selection,
            mark_kind: "bold".to_string(),
            value: None,
            action: None,
        })
        .expect("bold applies");
    }

    #[test]
    fn an_empty_query_matches_nothing() {
        let app = app_with(&["hello world"]);
        let found = app.find_in_document(&options("")).expect("find");
        assert!(found.matches.is_empty());
    }

    #[test]
    fn matching_is_case_insensitive_unless_match_case_is_set() {
        let app = app_with(&["Cat cat CAT"]);
        let insensitive = app.find_in_document(&options("cat")).expect("find");
        assert_eq!(insensitive.matches.len(), 3);
        assert_eq!(
            insensitive
                .matches
                .iter()
                .map(|found| found.text.as_str())
                .collect::<Vec<_>>(),
            vec!["Cat", "cat", "CAT"]
        );

        let sensitive = app
            .find_in_document(&FindOptions {
                match_case: true,
                ..options("cat")
            })
            .expect("find");
        assert_eq!(sensitive.matches.len(), 1);
        assert_eq!(sensitive.matches[0].text, "cat");
        assert_eq!(sensitive.matches[0].start.offset, 4);
    }

    #[test]
    fn whole_word_rejects_matches_inside_a_longer_word() {
        let app = app_with(&["cat cats scatter cat."]);
        assert_eq!(
            app.find_in_document(&options("cat")).unwrap().matches.len(),
            4
        );
        let whole = app
            .find_in_document(&FindOptions {
                whole_word: true,
                ..options("cat")
            })
            .expect("find");
        assert_eq!(whole.matches.len(), 2);
        assert_eq!(whole.matches[0].start.offset, 0);
        assert_eq!(whole.matches[1].start.offset, 17);
    }

    #[test]
    fn a_literal_query_is_escaped_rather_than_compiled_as_a_pattern() {
        let app = app_with(&["a.b axb"]);
        let literal = app.find_in_document(&options("a.b")).expect("find");
        assert_eq!(literal.matches.len(), 1);
        assert_eq!(literal.matches[0].start.offset, 0);

        let pattern = app
            .find_in_document(&FindOptions {
                regex: true,
                ..options("a.b")
            })
            .expect("find");
        assert_eq!(pattern.matches.len(), 2);
    }

    #[test]
    fn regex_search_matches_patterns_and_respects_whole_word() {
        let app = app_with(&["a1 b22 xc3x"]);
        let all = app
            .find_in_document(&FindOptions {
                regex: true,
                ..options(r"[a-z]\d+")
            })
            .expect("find");
        assert_eq!(
            all.matches
                .iter()
                .map(|found| found.text.as_str())
                .collect::<Vec<_>>(),
            vec!["a1", "b22", "c3"]
        );

        let whole = app
            .find_in_document(&FindOptions {
                regex: true,
                whole_word: true,
                ..options(r"[a-z]\d+")
            })
            .expect("find");
        assert_eq!(
            whole
                .matches
                .iter()
                .map(|found| found.text.as_str())
                .collect::<Vec<_>>(),
            vec!["a1", "b22"]
        );
    }

    /// An invalid pattern is a clean error, never a panic.
    #[test]
    fn an_invalid_regex_is_reported_rather_than_panicking() {
        let mut app = app_with(&["hello"]);
        let broken = FindOptions {
            regex: true,
            ..options("(unclosed")
        };
        let error = app.find_in_document(&broken).expect_err("invalid pattern");
        assert!(
            matches!(&error, AppApiError::Format(message)
                if message.contains("is not a valid regular expression")),
            "unexpected error: {error:?}"
        );
        // The replace paths compile the same pattern, so they refuse it too
        // instead of editing the document against a half-parsed query.
        assert!(app.replace_all_in_document(&broken, "x").is_err());
        assert!(app.replace_match_in_document(&broken, "x", 0).is_err());
        assert_eq!(texts(&app), vec!["hello".to_string()]);
    }

    /// ED-25: a match that starts in one inline run and ends in another.
    #[test]
    fn a_match_spanning_two_inline_runs_is_found_and_replaced() {
        let mut app = app_with(&["hello world"]);
        bold(&mut app, 6, 11);
        assert_eq!(
            run_count(&app, 0),
            2,
            "bolding half the paragraph must split it into two runs"
        );

        let found = app.find_in_document(&options("lo wo")).expect("find");
        assert_eq!(found.matches.len(), 1, "the match crosses the run boundary");
        let matched = &found.matches[0];
        assert_eq!(matched.text, "lo wo");
        // The selection the frontend highlights starts in the first run and
        // ends in the second one.
        assert_ne!(matched.start.inline_id, matched.end.inline_id);
        assert_eq!(matched.start.offset, 3);
        assert_eq!(matched.end.offset, 2);

        app.replace_all_in_document(&options("lo wo"), "LO-WO")
            .expect("replace");
        assert_eq!(texts(&app), vec!["helLO-WOrld".to_string()]);
    }

    /// Character offsets, not byte offsets: every one of these positions is
    /// larger when counted in UTF-8 bytes.
    #[test]
    fn offsets_are_characters_not_bytes() {
        let mut app = app_with(&["héllo wörld ☃ héllo"]);
        let found = app.find_in_document(&options("héllo")).expect("find");
        assert_eq!(found.matches.len(), 2);
        assert_eq!(found.matches[0].start.offset, 0);
        assert_eq!(found.matches[0].end.offset, 5);
        // Byte offset of the second "héllo" is 17; its character offset is 14.
        assert_eq!(found.matches[1].start.offset, 14);
        assert_eq!(found.matches[1].end.offset, 19);

        let snowman = app.find_in_document(&options("☃")).expect("find");
        assert_eq!(snowman.matches.len(), 1);
        assert_eq!(snowman.matches[0].start.offset, 12);
        assert_eq!(snowman.matches[0].end.offset, 13);

        app.replace_all_in_document(&options("wörld"), "wörlden")
            .expect("replace");
        assert_eq!(texts(&app), vec!["héllo wörlden ☃ héllo".to_string()]);
    }

    #[test]
    fn replace_match_rewrites_only_the_indexed_occurrence() {
        let mut app = app_with(&["one two one two", "one"]);
        app.replace_match_in_document(&options("one"), "1", 1)
            .expect("replace");
        assert_eq!(
            texts(&app),
            vec!["one two 1 two".to_string(), "one".to_string()]
        );

        app.replace_match_in_document(&options("one"), "1", 1)
            .expect("replace");
        assert_eq!(
            texts(&app),
            vec!["one two 1 two".to_string(), "1".to_string()]
        );

        let error = app
            .replace_match_in_document(&options("one"), "1", 5)
            .expect_err("out of range");
        assert!(matches!(error, AppApiError::NotFound(_)), "{error:?}");
    }

    /// Replace-all is one dispatch, so it is one undo checkpoint however
    /// many occurrences it rewrites.
    ///
    /// The stack length is asserted, not only the undo result: the dispatcher
    /// pushes its own checkpoint last, so "one undo restores everything"
    /// would still hold if the command quietly pushed five more underneath.
    #[test]
    fn replace_all_is_a_single_undo_step() {
        let mut app = app_with(&["one one one", "one two one"]);
        let checkpoints_before = app.undo_stack.len();
        app.dispatch_command(
            "replace_all_in_document",
            json!({
                "query": "one",
                "matchCase": false,
                "wholeWord": false,
                "regex": false,
                "replacement": "1",
            }),
        )
        .expect("replace all");
        assert_eq!(
            texts(&app),
            vec!["1 1 1".to_string(), "1 two 1".to_string()]
        );
        assert_eq!(
            app.find_in_document(&options("one")).unwrap().matches.len(),
            0
        );
        assert_eq!(
            app.undo_stack.len(),
            checkpoints_before + 1,
            "five replacements must add exactly one undo checkpoint"
        );

        app.undo_current_edit().expect("undo");
        assert_eq!(
            texts(&app),
            vec!["one one one".to_string(), "one two one".to_string()],
            "one undo restores every occurrence"
        );
    }

    /// Replacements are journalled operations, not a quiet rewrite of the
    /// document, so a replica replaying the journal reaches the same text.
    #[test]
    fn replacements_are_journalled_operations() {
        let mut app = app_with(&["one one"]);
        let before = app.document().operations.len();
        app.replace_all_in_document(&options("one"), "two")
            .expect("replace");
        let after = app.document();
        assert!(
            after.operations.len() > before,
            "replace-all appended operations to the journal"
        );
        assert!(after.has_unsaved_changes);
    }

    #[test]
    fn a_zero_length_regex_match_is_not_an_actionable_match() {
        let app = app_with(&["aaa b"]);
        let found = app
            .find_in_document(&FindOptions {
                regex: true,
                ..options("a*")
            })
            .expect("find");
        assert_eq!(
            found.matches.len(),
            1,
            "only the non-empty run of a's is a match"
        );
        assert_eq!(found.matches[0].text, "aaa");
    }

    /// The haystack `block_text` builds is exactly the one
    /// `DocumentIndex::text_of` used to build, for every block of a document
    /// carrying every inline kind and a table.
    ///
    /// This is the guard on the one duplication the linear search introduced:
    /// the map replaces `text_of`'s lookup, so the rule it applied has to be
    /// stated here too, and the two are compared rather than trusted.
    #[test]
    fn block_text_is_what_the_document_index_would_have_produced() {
        use opendoc_core::{
            Equation, EquationSourceFormat, PageNumberField, StableId, TableCell, TableRow,
        };

        let mut app = app_with(&["a plain paragraph"]);
        let mut rich = Block::paragraph("");
        rich.id = StableId::parse("rich").expect("valid id");
        rich.content = vec![
            Inline::text("before "),
            Inline::Link {
                id: StableId::parse("link").expect("valid id"),
                text: "a link".to_string(),
                href: "https://example.invalid/".to_string(),
                marks: Vec::new(),
            },
            Inline::Citation {
                id: StableId::parse("cite").expect("valid id"),
                citation_id: StableId::parse("source").expect("valid id"),
                rendered_cache: Some("(Author 2020)".to_string()),
            },
            Inline::FootnoteRef {
                id: StableId::parse("ref").expect("valid id"),
                footnote_id: StableId::parse("note").expect("valid id"),
            },
            Inline::Mention {
                id: StableId::parse("mention").expect("valid id"),
                label: "@someone".to_string(),
            },
            Inline::Equation {
                id: StableId::parse("eq").expect("valid id"),
                equation: Equation {
                    id: StableId::parse("eq-body").expect("valid id"),
                    source_format: EquationSourceFormat::LatexLike,
                    source: "x^2".to_string(),
                },
            },
            Inline::PageNumber {
                id: StableId::parse("page").expect("valid id"),
                field: PageNumberField::CurrentPage,
            },
            Inline::text(" after"),
        ];
        app.document.blocks.push(rich);

        let mut table = Block::paragraph("");
        table.id = StableId::parse("table").expect("valid id");
        let mut nested = Block::paragraph("text inside a cell");
        nested.id = StableId::parse("cell-block").expect("valid id");
        table.kind = BlockKind::table(vec![TableRow {
            id: StableId::parse("row").expect("valid id"),
            height: None,
            header: false,
            cells: vec![TableCell {
                id: StableId::parse("cell").expect("valid id"),
                span: opendoc_core::CellSpan::SINGLE,
                properties: Default::default(),
                blocks: vec![nested],
            }],
        }]);
        app.document.blocks.push(table);

        let index = DocumentIndex::build(&app.document.blocks);
        let by_id = blocks_by_id(&app.document.blocks);
        assert!(index.blocks.len() >= 4);
        for position in 0..index.blocks.len() {
            let id = index.blocks[position].id.as_str();
            let mapped = by_id
                .get(id)
                .map(|block| block_text(block))
                .unwrap_or_default();
            assert_eq!(
                mapped,
                index.text_of(&app.document.blocks, position),
                "block {id} reads differently through the map than through the index"
            );
        }
    }

    /// A search reads every block exactly once, whatever the document's
    /// shape. Stated as a match count on a document where a linear scan per
    /// block would still have produced the right answer — the point is that
    /// the nested block is reached at all.
    #[test]
    fn a_search_finds_text_inside_a_table_cell() {
        use opendoc_core::{StableId, TableCell, TableRow};

        let mut app = app_with(&["nothing here"]);
        let mut table = Block::paragraph("");
        table.id = StableId::parse("table").expect("valid id");
        let mut nested = Block::paragraph("needle in a cell");
        nested.id = StableId::parse("cell-block").expect("valid id");
        table.kind = BlockKind::table(vec![TableRow {
            id: StableId::parse("row").expect("valid id"),
            height: None,
            header: false,
            cells: vec![TableCell {
                id: StableId::parse("cell").expect("valid id"),
                span: opendoc_core::CellSpan::SINGLE,
                properties: Default::default(),
                blocks: vec![nested],
            }],
        }]);
        app.document.blocks.push(table);
        let found = app.find_in_document(&options("needle")).expect("find");
        assert_eq!(found.matches.len(), 1);
        assert_eq!(found.matches[0].start.block_id, "cell-block");
    }

    // ---- Headers, footers and footnote bodies ----------------------------
    //
    // `DocumentIndex` walks `document.blocks`, and all three of these are
    // kept outside it, so a search saw none of them and a replace-all left
    // them still saying the old thing while reporting it had replaced
    // everything.

    fn set_furniture(app: &mut OpenDocApp, slot: HeaderFooterSlot, text: &str) {
        let mut block = Block::paragraph(text);
        block.id = StableId::new("furniture");
        *app.document.furniture_mut(slot) = vec![block];
    }

    fn furniture_text(app: &OpenDocApp, slot: HeaderFooterSlot) -> String {
        app.document
            .furniture(slot)
            .iter()
            .flat_map(|block| block.content.iter())
            .map(|inline| match inline {
                Inline::Text { text, .. } | Inline::Link { text, .. } => text.as_str(),
                _ => "\u{FFFC}",
            })
            .collect()
    }

    #[test]
    fn a_search_covers_the_header_the_footer_and_a_footnote_body() {
        let mut app = app_with(&["needle in the body"]);
        set_furniture(&mut app, HeaderFooterSlot::Header, "needle in the header");
        set_furniture(&mut app, HeaderFooterSlot::Footer, "needle in the footer");
        app.document.footnotes.push(Footnote {
            id: StableId::parse("note").expect("valid id"),
            revision: 1,
            body: vec![Inline::text("needle in a note")],
            deleted: false,
        });

        let found = app.find_in_document(&options("needle")).expect("find");
        assert_eq!(
            found.matches.len(),
            4,
            "expected the body, the header, the footer and the note"
        );
        // The body comes first, so a match a user is already looking at keeps
        // its number when a header gains one.
        assert_eq!(
            found.matches[0].start.block_id,
            app.document.blocks[0].id.to_string()
        );
        assert_eq!(found.matches[3].start.block_id, "note");
    }

    /// Every match says which part of the document it is in, because its
    /// positions cannot.
    ///
    /// A header block, a footer block and a footnote body are outside
    /// `document.blocks`, so their ids are outside the editor's DOM. The
    /// frontend used to hand every match's positions to `setSelection`, and
    /// for three matches out of four that call did nothing at all — the
    /// counter advanced, the caret stayed where it was, and nothing said why.
    /// Region is the field that lets the caller take the user somewhere the
    /// match can actually be edited.
    #[test]
    fn every_match_names_the_region_it_was_found_in() {
        let mut app = app_with(&["needle in the body"]);
        set_furniture(&mut app, HeaderFooterSlot::Header, "needle in the header");
        set_furniture(&mut app, HeaderFooterSlot::Footer, "needle in the footer");
        app.document.footnotes.push(Footnote {
            id: StableId::parse("note").expect("valid id"),
            revision: 1,
            body: vec![Inline::text("needle in a note")],
            deleted: false,
        });

        let found = app.find_in_document(&options("needle")).expect("find");
        let regions = found
            .matches
            .iter()
            .map(|found| found.region)
            .collect::<Vec<_>>();
        assert_eq!(
            regions,
            vec![
                AppFindRegion::Body,
                AppFindRegion::Header,
                AppFindRegion::Footer,
                AppFindRegion::Footnote,
            ],
            "each match must name its own region, in the order the search reports them"
        );
        // The footnote match addresses the *note*, which is what the footnote
        // editor takes — so "open the region this match is in" needs nothing
        // the match does not already carry.
        assert_eq!(found.matches[3].start.block_id, "note");

        // Only the body match addresses a block the editor surface holds, and
        // that is the property the frontend branches on.
        let index = DocumentIndex::build(&app.document.blocks);
        for found in &found.matches {
            let in_the_editor = index.block_index(&found.start.block_id).is_some();
            assert_eq!(
                in_the_editor,
                found.region == AppFindRegion::Body,
                "{:?} at block {} disagrees with whether the editor can address it",
                found.region,
                found.start.block_id
            );
        }
    }

    #[test]
    fn find_and_replace_reaches_explicit_page_furniture_variants_without_searching_inheritance_twice(
    ) {
        let mut app = app_with(&["needle in the body"]);
        set_furniture(
            &mut app,
            HeaderFooterSlot::Header,
            "needle in ordinary header",
        );
        set_furniture(
            &mut app,
            HeaderFooterSlot::FirstPageHeader,
            "needle in first header",
        );
        set_furniture(
            &mut app,
            HeaderFooterSlot::EvenPageFooter,
            "needle in even footer",
        );

        let regions = app
            .find_in_document(&options("needle"))
            .expect("find")
            .matches
            .into_iter()
            .map(|found| found.region)
            .collect::<Vec<_>>();
        assert_eq!(
            regions,
            vec![
                AppFindRegion::Body,
                AppFindRegion::Header,
                AppFindRegion::FirstPageHeader,
                AppFindRegion::EvenPageFooter,
            ],
            "an absent override inherits ordinary furniture and must not make a duplicate match"
        );

        app.replace_all_in_document(&options("needle"), "pin")
            .expect("replace all");
        assert_eq!(
            furniture_text(&app, HeaderFooterSlot::Header),
            "pin in ordinary header"
        );
        assert_eq!(
            furniture_text(&app, HeaderFooterSlot::FirstPageHeader),
            "pin in first header"
        );
        assert_eq!(
            furniture_text(&app, HeaderFooterSlot::EvenPageFooter),
            "pin in even footer"
        );
        assert!(app
            .document
            .has_furniture_override(HeaderFooterSlot::FirstPageHeader));
        assert!(app
            .document
            .has_furniture_override(HeaderFooterSlot::EvenPageFooter));
        app.document
            .validate()
            .expect("variant replacement stays valid");
    }

    #[test]
    fn replace_all_reaches_the_header_the_footer_and_the_footnote() {
        let mut app = app_with(&["needle in the body"]);
        set_furniture(&mut app, HeaderFooterSlot::Header, "needle in the header");
        set_furniture(&mut app, HeaderFooterSlot::Footer, "needle in the footer");
        app.document.footnotes.push(Footnote {
            id: StableId::parse("note").expect("valid id"),
            revision: 1,
            body: vec![Inline::text("needle in a note")],
            deleted: false,
        });

        app.replace_all_in_document(&options("needle"), "pin")
            .expect("replace all");

        assert_eq!(texts(&app), vec!["pin in the body".to_string()]);
        assert_eq!(
            furniture_text(&app, HeaderFooterSlot::Header),
            "pin in the header"
        );
        assert_eq!(
            furniture_text(&app, HeaderFooterSlot::Footer),
            "pin in the footer"
        );
        assert_eq!(
            app.document.footnotes[0]
                .body
                .iter()
                .map(|inline| match inline {
                    Inline::Text { text, .. } => text.as_str(),
                    _ => "",
                })
                .collect::<String>(),
            "pin in a note"
        );
        assert!(
            app.find_in_document(&options("needle"))
                .expect("find")
                .matches
                .is_empty(),
            "a replace-all reported success with matches still standing"
        );
        app.document.validate().expect("the document stays valid");
    }

    /// A replace-all over several regions is still one undo step: the
    /// whole-slot operations join the body's in the same batch.
    #[test]
    fn replacing_across_regions_is_one_undo_step() {
        let mut app = app_with(&["needle in the body"]);
        set_furniture(&mut app, HeaderFooterSlot::Header, "needle in the header");
        let checkpoints_before = app.undo_stack.len();
        app.dispatch_command(
            "replace_all_in_document",
            json!({
                "query": "needle",
                "matchCase": false,
                "wholeWord": false,
                "regex": false,
                "replacement": "pin",
            }),
        )
        .expect("replace all");
        assert_eq!(
            app.undo_stack.len(),
            checkpoints_before + 1,
            "a replace across regions must add exactly one undo checkpoint"
        );

        app.undo_current_edit().expect("undo");
        assert_eq!(texts(&app), vec!["needle in the body".to_string()]);
        assert_eq!(
            furniture_text(&app, HeaderFooterSlot::Header),
            "needle in the header"
        );
    }

    /// Several matches in one header slot fold into one replacement, applied
    /// last-first so a longer replacement cannot shift the next one.
    #[test]
    fn several_matches_in_one_slot_are_all_replaced() {
        let mut app = app_with(&["body"]);
        set_furniture(&mut app, HeaderFooterSlot::Header, "a a a");
        app.replace_all_in_document(&options("a"), "bbb")
            .expect("replace all");
        assert_eq!(
            furniture_text(&app, HeaderFooterSlot::Header),
            "bbb bbb bbb"
        );
    }

    /// Replacing one named match still picks the right one when the match is
    /// outside the body.
    #[test]
    fn replacing_one_match_can_name_a_header_match() {
        let mut app = app_with(&["needle in the body"]);
        set_furniture(&mut app, HeaderFooterSlot::Header, "needle in the header");
        app.replace_match_in_document(&options("needle"), "pin", 1)
            .expect("replace the second match");
        assert_eq!(texts(&app), vec!["needle in the body".to_string()]);
        assert_eq!(
            furniture_text(&app, HeaderFooterSlot::Header),
            "pin in the header"
        );
    }
}
