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

use super::{AppApiError, AppDocument, OpenDocApp};
use crate::editor::{DocumentIndex, EditPlan, Resolved};
use opendoc_api::{AppFindMatch, AppFindMatches, FindOptions};
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

/// Every match, in document order, as character ranges inside a block.
fn block_matches(
    app: &OpenDocApp,
    index: &DocumentIndex,
    options: &FindOptions,
) -> Result<Vec<BlockMatch>, AppApiError> {
    let Some(regex) = compile(options)? else {
        return Ok(Vec::new());
    };
    let mut matches = Vec::new();
    for block in 0..index.blocks.len() {
        if !index.blocks[block].text_block {
            continue;
        }
        // The whole block, runs concatenated: this is what makes a match
        // across a formatting boundary findable at all.
        let haystack = index.text_of(&app.document.blocks, block);
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
    let matches = block_matches(app, &index, options)?
        .into_iter()
        .map(|found| AppFindMatch {
            start: index.position(Resolved {
                block: found.block,
                abs: found.start,
            }),
            end: index.position(Resolved {
                block: found.block,
                abs: found.end,
            }),
            text: found.text,
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
        let matches = block_matches(self.app, &index, options)?;
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
        let matches = block_matches(self.app, &index, options)?;
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
        matches: &[BlockMatch],
        replacement: &str,
    ) -> Result<AppDocument, AppApiError> {
        if matches.is_empty() {
            return Ok(self.app.document());
        }
        let mut plan = EditPlan::new(self.app, index);
        for found in matches.iter().rev() {
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
        let ops = plan.ops;
        if ops.is_empty() {
            return Ok(self.app.document());
        }
        Ok(self.app.apply_batch(ops))
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
}
