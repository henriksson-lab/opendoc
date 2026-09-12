//! Comments, suggestions and the anchors that bind them to content.

use crate::document::{
    inline_sequence_is_empty_source_text, validate_inline_sequence, validate_marks,
};
use crate::ids::validate_stable_id;
use crate::ids::StableId;
use crate::inline::{Inline, Mark, TextRange};
use crate::warning::ModelError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CommentThread {
    pub id: StableId,
    pub anchor: Anchor,
    pub comments: Vec<Comment>,
    pub deleted: bool,
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
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Comment {
    pub id: StableId,
    pub author: String,
    pub body: Vec<Inline>,
    pub created_at_ms: u64,
    pub deleted: bool,
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
    NearestBlock { block_id: StableId, warning: String },
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
        }
        Ok(())
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
        Anchor::Document => Ok(()),
    }
}

pub(crate) fn validate_text_range(range: &TextRange) -> Result<(), ModelError> {
    validate_stable_id("text range start", &range.start)?;
    validate_stable_id("text range end", &range.end)
}

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
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum SuggestionState {
    Proposed,
    Accepted,
    Rejected,
}
