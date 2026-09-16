//! Comments, suggestions and the anchors that bind them to content.

use crate::block::Block;
use crate::document::{
    inline_sequence_is_empty_source_text, validate_inline_sequence, validate_marks,
};
use crate::ids::validate_stable_id;
use crate::ids::{InsertPosition, StableId};
use crate::inline::{Inline, Mark, TextRange};
use crate::warning::ModelError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// The maximum number of durable, document-local comment activity records.
/// This bound is source-model policy rather than a UI convenience: every
/// replica must discard the same oldest records on replay.
pub const MAX_COMMENT_ACTIVITY_ENTRIES: usize = 1_024;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CommentThread {
    pub id: StableId,
    pub anchor: Anchor,
    pub comments: Vec<Comment>,
    /// Resolution is separate from deletion: resolved conversations remain part
    /// of the document's review history and can be reopened.
    #[serde(default)]
    pub state: CommentThreadState,
    #[serde(default)]
    pub resolved_by: Option<String>,
    #[serde(default)]
    pub resolved_at_ms: Option<u64>,
    /// An offline display-name assignment.  It intentionally is not an
    /// account id: identity resolution belongs to the service boundary.
    #[serde(default)]
    pub action_assignee: Option<String>,
    /// A document-local UTC due instant for the assigned action.  It is
    /// deliberately an instant rather than a locale-formatted date so replicas
    /// and interchange formats cannot disagree about the day it denotes.
    #[serde(default)]
    pub action_due_at_ms: Option<u64>,
    /// Completion is separate from resolving the discussion: a reviewer may
    /// resolve a thread before, or after, its requested action is done.
    #[serde(default)]
    pub action_completed_by: Option<String>,
    #[serde(default)]
    pub action_completed_at_ms: Option<u64>,
    /// Document-local reactions on this discussion.  Each actor may react to
    /// a particular emoji at most once.  These are review metadata only:
    /// they deliberately do not imply a notification or an external account.
    #[serde(default)]
    pub reactions: Vec<CommentThreadReaction>,
    pub deleted: bool,
}

/// One emoji reaction and the document actors that currently hold it.
///
/// Both collections are canonicalized by the operation applier so replay and
/// independently merged replicas have stable source bytes.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CommentThreadReaction {
    pub emoji: String,
    #[serde(default)]
    pub actors: Vec<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommentThreadState {
    #[default]
    Open,
    Resolved,
    Reopened,
}

impl CommentThread {
    pub fn validate(&self) -> Result<(), ModelError> {
        validate_stable_id("comment thread id", &self.id)?;
        validate_anchor(&self.anchor)?;
        if self.comments.is_empty() {
            return Err(ModelError::InvalidDocument(
                "comment thread has no comments",
            ));
        }
        let mut comment_ids = BTreeSet::new();
        for comment in &self.comments {
            comment.validate()?;
            if !comment_ids.insert(comment.id.clone()) {
                return Err(ModelError::InvalidDocument("duplicate comment id"));
            }
        }
        match self.state {
            CommentThreadState::Resolved => {
                let Some(resolver) = &self.resolved_by else {
                    return Err(ModelError::InvalidDocument(
                        "resolved comment thread has no resolver",
                    ));
                };
                if resolver.trim().is_empty() || resolver.trim() != resolver {
                    return Err(ModelError::InvalidDocument("comment resolver is invalid"));
                }
                if self.resolved_at_ms.is_none() {
                    return Err(ModelError::InvalidDocument(
                        "resolved comment thread has no timestamp",
                    ));
                }
            }
            CommentThreadState::Open | CommentThreadState::Reopened => {
                if self.resolved_by.is_some() || self.resolved_at_ms.is_some() {
                    return Err(ModelError::InvalidDocument(
                        "open comment thread has resolution metadata",
                    ));
                }
            }
        }
        for (field, value) in [
            ("comment action assignee", self.action_assignee.as_ref()),
            (
                "comment action completer",
                self.action_completed_by.as_ref(),
            ),
        ] {
            if let Some(value) = value {
                if value.trim().is_empty() || value.trim() != value {
                    return Err(ModelError::InvalidDocument(match field {
                        "comment action assignee" => "comment action assignee is invalid",
                        _ => "comment action completer is invalid",
                    }));
                }
            }
        }
        if self.action_completed_by.is_some() != self.action_completed_at_ms.is_some() {
            return Err(ModelError::InvalidDocument(
                "comment action completion metadata is incomplete",
            ));
        }
        if self.action_due_at_ms.is_some() && self.action_assignee.is_none() {
            return Err(ModelError::InvalidDocument(
                "comment action due date has no assignee",
            ));
        }
        if self.action_completed_by.is_some() && self.action_assignee.is_none() {
            return Err(ModelError::InvalidDocument(
                "comment action completion has no assignee",
            ));
        }
        let mut emoji_values = BTreeSet::new();
        for reaction in &self.reactions {
            validate_comment_reaction_emoji(&reaction.emoji)?;
            if !emoji_values.insert(reaction.emoji.clone()) {
                return Err(ModelError::InvalidDocument(
                    "duplicate comment reaction emoji",
                ));
            }
            if reaction.actors.is_empty() {
                return Err(ModelError::InvalidDocument(
                    "comment reaction has no actors",
                ));
            }
            let mut actors = BTreeSet::new();
            for actor in &reaction.actors {
                if actor.trim().is_empty() || actor.trim() != actor {
                    return Err(ModelError::InvalidDocument(
                        "comment reaction actor is invalid",
                    ));
                }
                if !actors.insert(actor) {
                    return Err(ModelError::InvalidDocument(
                        "duplicate comment reaction actor",
                    ));
                }
            }
        }
        Ok(())
    }
}

/// Keep reactions visibly emoji-like without trying to implement Unicode's
/// evolving full emoji grammar in the document model.  Variation selectors,
/// joiners, skin-tone modifiers and regional indicators are permitted as part
/// of an otherwise emoji-bearing sequence.
pub fn validate_comment_reaction_emoji(value: &str) -> Result<(), ModelError> {
    if value.is_empty() || value.chars().count() > 32 || value.trim() != value {
        return Err(ModelError::InvalidDocument(
            "comment reaction emoji is invalid",
        ));
    }
    let emoji_bearing = value.chars().any(|ch| {
        matches!(ch as u32,
            0x00A9 | 0x00AE | 0x203C | 0x2049 | 0x2122 | 0x2139 | 0x2328 | 0x24C2
            | 0x25AA..=0x27BF | 0x2934..=0x2935 | 0x2B05..=0x2B55
            | 0x3030 | 0x303D | 0x3297 | 0x3299 | 0x1F000..=0x1FAFF)
    });
    if !emoji_bearing
        || value
            .chars()
            .any(|ch| ch.is_control() || ch.is_whitespace())
    {
        return Err(ModelError::InvalidDocument(
            "comment reaction emoji is invalid",
        ));
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Comment {
    pub id: StableId,
    pub author: String,
    pub body: Vec<Inline>,
    pub created_at_ms: u64,
    pub deleted: bool,
}

/// An immutable review event for one comment.  The live comment deliberately
/// remains small and easy to render; this journal is the durable evidence of
/// what an edit or deletion replaced.  `at_ms` is the document operation's
/// monotonic logical timestamp, not wall-clock time (offline replicas do not
/// have a trustworthy shared clock).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CommentHistoryEntry {
    pub thread_id: StableId,
    pub comment_id: StableId,
    /// `edited`, `deleted`, or `restored`.
    pub kind: String,
    /// The operation actor, which is stable across replay and merge.
    pub actor: String,
    pub at_ms: u64,
    /// The body immediately before the event.  This is present for edits and
    /// deletion so neither action destroys the review record.
    #[serde(default)]
    pub previous_body: Option<Vec<Inline>>,
}

/// A replayable event in the document-local review activity stream.  It is
/// deliberately not a notification: it identifies no recipient and has no
/// delivery/read state.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommentActivityKind {
    ThreadCreated,
    ReplyAdded,
    ThreadResolved,
    ThreadReopened,
    ThreadDeleted,
    ThreadRestored,
    CommentEdited,
    CommentDeleted,
    CommentRestored,
    ActionSet,
    ReactionAdded,
    ReactionRemoved,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CommentActivityEntry {
    pub operation_actor: String,
    pub operation_seq: u64,
    pub actor: String,
    /// The operation's deterministic logical timestamp, not wall-clock time.
    pub at_ms: u64,
    pub thread_id: StableId,
    #[serde(default)]
    pub comment_id: Option<StableId>,
    pub kind: CommentActivityKind,
}

impl CommentActivityEntry {
    pub fn validate(&self) -> Result<(), ModelError> {
        for (name, value) in [
            ("comment activity operation actor", &self.operation_actor),
            ("comment activity actor", &self.actor),
        ] {
            if value.trim().is_empty() || value.trim() != value {
                return Err(ModelError::InvalidDocument(match name {
                    "comment activity operation actor" => {
                        "comment activity operation actor is invalid"
                    }
                    _ => "comment activity actor is invalid",
                }));
            }
        }
        if self.operation_seq == 0 {
            return Err(ModelError::InvalidDocument(
                "comment activity operation sequence is zero",
            ));
        }
        validate_stable_id("comment activity thread id", &self.thread_id)?;
        if let Some(comment_id) = &self.comment_id {
            validate_stable_id("comment activity comment id", comment_id)?;
        }
        Ok(())
    }
}

impl CommentHistoryEntry {
    pub fn validate(&self) -> Result<(), ModelError> {
        validate_stable_id("comment history thread id", &self.thread_id)?;
        validate_stable_id("comment history comment id", &self.comment_id)?;
        if !matches!(self.kind.as_str(), "edited" | "deleted" | "restored") {
            return Err(ModelError::InvalidDocument("invalid comment history event"));
        }
        if self.actor.trim().is_empty() || self.actor.trim() != self.actor {
            return Err(ModelError::InvalidDocument(
                "comment history actor is invalid",
            ));
        }
        if matches!(self.kind.as_str(), "edited" | "deleted") {
            let Some(body) = &self.previous_body else {
                return Err(ModelError::InvalidDocument(
                    "comment history event has no prior body",
                ));
            };
            if body.is_empty() || inline_sequence_is_empty_source_text(body) {
                return Err(ModelError::InvalidDocument(
                    "comment history prior body is empty",
                ));
            }
            validate_inline_sequence(body)?;
        } else if self.previous_body.is_some() {
            // A restore does not replace source text.  Accepting a body here
            // would let an imported journal smuggle an uncorrelated revision
            // into the otherwise immutable review projection.
            return Err(ModelError::InvalidDocument(
                "restored comment history event has prior body",
            ));
        }
        Ok(())
    }
}

impl Comment {
    pub fn validate(&self) -> Result<(), ModelError> {
        validate_stable_id("comment id", &self.id)?;
        if self.author.trim().is_empty() {
            return Err(ModelError::InvalidDocument("comment author is empty"));
        }
        if self.author.trim() != self.author {
            return Err(ModelError::InvalidDocument(
                "comment author has surrounding whitespace",
            ));
        }
        if self.body.is_empty() || inline_sequence_is_empty_source_text(&self.body) {
            return Err(ModelError::InvalidDocument("comment body is empty"));
        }
        validate_inline_sequence(&self.body)?;
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Anchor {
    TextRange(TextRange),
    NearestBlock {
        block_id: StableId,
        warning: String,
    },
    /// The original target was deleted.  Unlike `NearestBlock`, this is not a
    /// live navigation target: moving a review thread to unrelated surviving
    /// text loses the evidence the reviewer was responding to.  The captured
    /// quote and its containing-block context are source text observed just
    /// before the destructive operation.
    Orphaned {
        quote: String,
        context: String,
        warning: String,
    },
    Document,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Suggestion {
    pub id: StableId,
    pub author: String,
    pub kind: SuggestionKind,
    pub state: SuggestionState,
    pub provenance: Vec<String>,
}

impl Suggestion {
    pub fn validate(&self) -> Result<(), ModelError> {
        validate_stable_id("suggestion id", &self.id)?;
        if self.author.trim().is_empty() {
            return Err(ModelError::InvalidDocument("suggestion author is empty"));
        }
        if self.author.trim() != self.author {
            return Err(ModelError::InvalidDocument(
                "suggestion author has surrounding whitespace",
            ));
        }
        let mut provenance_entries = BTreeSet::new();
        for item in &self.provenance {
            if item.trim().is_empty() {
                return Err(ModelError::InvalidDocument(
                    "suggestion provenance entry is empty",
                ));
            }
            if item.trim() != item {
                return Err(ModelError::InvalidDocument(
                    "suggestion provenance entry has surrounding whitespace",
                ));
            }
            if !provenance_entries.insert(item) {
                return Err(ModelError::InvalidDocument(
                    "duplicate suggestion provenance entry",
                ));
            }
        }
        match &self.kind {
            SuggestionKind::Insert { anchor, content } => {
                validate_anchor(anchor)?;
                if content.is_empty() || inline_sequence_is_empty_source_text(content) {
                    return Err(ModelError::InvalidDocument(
                        "insert suggestion content is empty",
                    ));
                }
                validate_inline_sequence(content)?;
            }
            SuggestionKind::Delete { range } => validate_text_range(range)?,
            SuggestionKind::Format { range, marks } => {
                validate_text_range(range)?;
                if marks.is_empty() {
                    return Err(ModelError::InvalidDocument(
                        "format suggestion marks are empty",
                    ));
                }
                validate_marks(marks)?;
            }
            SuggestionKind::FormatRemove { range, kind, value } => {
                validate_text_range(range)?;
                // A removal has the same value rules as the ordinary
                // `RemoveMark` operation.  In particular, a value is only
                // meaningful for value-bearing mark kinds.
                crate::document::validate_mark_removal(kind, value.as_deref())?;
            }
            SuggestionKind::FormatReplace {
                range,
                kind,
                expected_value,
                value,
            } => {
                validate_text_range(range)?;
                crate::document::validate_mark_replacement(kind, expected_value, value)?;
            }
            SuggestionKind::LinkChange {
                inline_id,
                expected_href,
                href,
            } => {
                validate_stable_id("link suggestion target", inline_id)?;
                for (label, value) in [("expected link href", expected_href), ("link href", href)] {
                    if let Some(value) = value {
                        if value.trim().is_empty() || value.trim() != value {
                            return Err(ModelError::InvalidDocument(match label {
                                "expected link href" => {
                                    "expected link href is empty or has surrounding whitespace"
                                }
                                _ => "link href is empty or has surrounding whitespace",
                            }));
                        }
                    }
                }
                if expected_href == href {
                    return Err(ModelError::InvalidDocument(
                        "link suggestion does not change the target href",
                    ));
                }
            }
            SuggestionKind::BlockDelete { block_id } => {
                validate_stable_id("block deletion suggestion target", block_id)?;
            }
            SuggestionKind::BlockInsert { position, block } => {
                validate_structural_suggestion_position(position)?;
                validate_text_block_suggestion(block)?;
            }
            SuggestionKind::BlockReplace {
                block_id,
                expected,
                replacement,
            } => {
                validate_stable_id("block replacement suggestion target", block_id)?;
                validate_text_block_suggestion(expected)?;
                validate_text_block_suggestion(replacement)?;
                if &expected.id != block_id || &replacement.id != block_id {
                    return Err(ModelError::InvalidDocument(
                        "structural suggestion replacement blocks must preserve target block id",
                    ));
                }
            }
            SuggestionKind::ParagraphStyleChange {
                block_id,
                expected,
                proposed,
            } => {
                validate_stable_id("paragraph style suggestion target", block_id)?;
                expected.validate()?;
                proposed.validate()?;
                if expected == proposed {
                    return Err(ModelError::InvalidDocument(
                        "paragraph style suggestion does not change the source style",
                    ));
                }
            }
        }
        Ok(())
    }
}

/// Structural tracked changes deliberately start with one narrow, exact
/// payload: a non-table text block.  Tables and images carry independent
/// identity/resource semantics and must receive their own proposal kinds,
/// rather than being smuggled through a paragraph replacement.
fn validate_text_block_suggestion(block: &Block) -> Result<(), ModelError> {
    block.validate_isolated()?;
    if !matches!(block.kind, crate::block::BlockKind::Paragraph) {
        return Err(ModelError::InvalidDocument(
            "structural suggestion block must be a paragraph",
        ));
    }
    if block
        .content
        .iter()
        .any(|inline| !matches!(inline, Inline::Text { .. }))
    {
        return Err(ModelError::InvalidDocument(
            "structural suggestion block must contain plain text only",
        ));
    }
    Ok(())
}

fn validate_structural_suggestion_position(position: &InsertPosition) -> Result<(), ModelError> {
    match position {
        // Absolute body positions are exact and do not have a deleted anchor.
        InsertPosition::First | InsertPosition::Last => Ok(()),
        InsertPosition::Before(id) | InsertPosition::After(id) => {
            validate_stable_id("structural suggestion insertion anchor", id)
        }
    }
}

pub(crate) fn validate_anchor(anchor: &Anchor) -> Result<(), ModelError> {
    match anchor {
        Anchor::TextRange(range) => validate_text_range(range),
        Anchor::NearestBlock { block_id, warning } => {
            validate_stable_id("nearest block anchor block id", block_id)?;
            if warning.trim().is_empty() {
                return Err(ModelError::InvalidDocument(
                    "nearest block anchor warning is empty",
                ));
            }
            if warning.trim() != warning {
                return Err(ModelError::InvalidDocument(
                    "nearest block anchor warning has surrounding whitespace",
                ));
            }
            Ok(())
        }
        Anchor::Orphaned {
            quote,
            context,
            warning,
        } => {
            for (field, value) in [
                ("orphaned comment quote", quote),
                ("orphaned comment context", context),
            ] {
                if value.trim().is_empty() {
                    return Err(ModelError::InvalidDocument(match field {
                        "orphaned comment quote" => "orphaned comment quote is empty",
                        _ => "orphaned comment context is empty",
                    }));
                }
            }
            if warning.trim().is_empty() {
                return Err(ModelError::InvalidDocument(
                    "orphaned comment anchor warning is empty",
                ));
            }
            if warning.trim() != warning {
                return Err(ModelError::InvalidDocument(
                    "orphaned comment anchor warning has surrounding whitespace",
                ));
            }
            Ok(())
        }
        Anchor::Document => Ok(()),
    }
}

pub(crate) fn validate_text_range(range: &TextRange) -> Result<(), ModelError> {
    validate_stable_id("text range start", &range.start)?;
    validate_stable_id("text range end", &range.end)
}

// Block proposals intentionally retain the complete reviewed block by value so
// accepting/rejecting and signed replay do not depend on a separately owned
// payload. Inline smart-chip metadata increased `Block` beyond Clippy's size
// heuristic; boxing it would change this established serialized operation
// shape without reducing any persisted payload.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum SuggestionKind {
    Insert {
        anchor: Anchor,
        content: Vec<Inline>,
    },
    Delete {
        range: TextRange,
    },
    Format {
        range: TextRange,
        marks: Vec<Mark>,
    },
    /// A tracked proposal to remove a mark from every surviving inline in a
    /// range.  It is deliberately separate from `Format`: accepting an add
    /// proposal must never be interpreted as a removal just because a mark
    /// happened to be present when the proposal was made.
    FormatRemove {
        range: TextRange,
        kind: crate::inline::MarkKind,
        value: Option<String>,
    },
    /// A tracked replacement of one value-bearing mark on every whole inline
    /// in a range.  The old value is a compare-and-set precondition: review
    /// must reject rather than overwrite a concurrent formatting edit.
    FormatReplace {
        range: TextRange,
        kind: crate::inline::MarkKind,
        expected_value: String,
        value: String,
    },
    /// A tracked whole-inline link edit.  `expected_href` records the exact
    /// source shape seen by the proposer (`None` means an unlinked text run),
    /// so acceptance never retargets a concurrent link edit.  `href: None`
    /// removes a link; `Some` adds or replaces it.
    LinkChange {
        inline_id: StableId,
        expected_href: Option<String>,
        href: Option<String>,
    },
    /// A tracked proposal to delete one whole block.  The target is an
    /// identity rather than a position, so concurrent insertions cannot make
    /// a reviewer delete a different block when this proposal is accepted.
    BlockDelete {
        block_id: StableId,
    },
    /// A proposed paragraph insertion.  A sibling identity is retained when
    /// available; if that identity disappears before review, the proposal is
    /// rejected instead of silently appending somewhere else.
    BlockInsert {
        position: InsertPosition,
        block: Block,
    },
    /// A proposed replacement of exactly one surviving block. `expected` is
    /// the complete plain-paragraph source the author reviewed, so accepting
    /// cannot overwrite a concurrent edit to the same stable block.
    /// `replacement` keeps the target's id, so block-anchored review metadata
    /// remains attached; it is in-place, never delete-plus-append.
    BlockReplace {
        block_id: StableId,
        expected: Box<Block>,
        replacement: Box<Block>,
    },
    /// A reviewable change of one existing non-list text block's style. Both
    /// values are retained so acceptance can reject a concurrent style edit
    /// without replacing the block's current content or properties.
    ParagraphStyleChange {
        block_id: StableId,
        expected: ParagraphStyle,
        proposed: ParagraphStyle,
    },
}

/// The deliberately non-list subset of paragraph styles that can be proposed
/// without manufacturing list-run state.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ParagraphStyle {
    Paragraph,
    Title,
    Subtitle,
    Heading { level: u8 },
}

impl ParagraphStyle {
    pub fn validate(&self) -> Result<(), ModelError> {
        if matches!(self, Self::Heading { level: 0 | 7.. }) {
            return Err(ModelError::InvalidDocument(
                "paragraph style heading level must be between 1 and 6",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum SuggestionState {
    Proposed,
    Accepted,
    Rejected,
}
