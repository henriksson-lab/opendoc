//! Comment thread, comment and suggestion projection DTOs.

use super::*;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppCommentThread {
    pub id: String,
    pub anchor: String,
    #[serde(default)]
    pub anchor_label: String,
    pub comments: Vec<AppComment>,
    pub deleted: bool,
}

impl AppCommentThread {
    pub(crate) fn from_core(thread: &CommentThread, blocks: &[Block]) -> Self {
        Self {
            id: thread.id.to_string(),
            anchor: anchor_label(&thread.anchor),
            anchor_label: anchor_display_label(&thread.anchor, blocks),
            comments: thread.comments.iter().map(AppComment::from_core).collect(),
            deleted: thread.deleted,
        }
    }

    pub(crate) fn to_core(&self) -> Result<CommentThread, AppApiError> {
        Ok(CommentThread {
            id: parse_id(&self.id)?,
            anchor: parse_anchor_label(&self.anchor, "comment anchor")?,
            comments: self
                .comments
                .iter()
                .map(AppComment::to_core)
                .collect::<Result<Vec<_>, _>>()?,
            deleted: self.deleted,
        })
    }

    pub(crate) fn validate_source(&self) -> Result<(), AppApiError> {
        parse_id(&self.id)?;
        parse_anchor_label(&self.anchor, "comment anchor")?;
        if self.comments.is_empty() {
            return Err(AppApiError::Format(
                "comment thread has no comments".to_string(),
            ));
        }
        let mut comment_ids = BTreeSet::new();
        for comment in &self.comments {
            comment.validate_source()?;
            if !comment_ids.insert(comment.id.clone()) {
                return Err(AppApiError::Format(format!(
                    "duplicate app comment id {}",
                    comment.id
                )));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppComment {
    pub id: String,
    pub author: String,
    pub body: String,
    pub deleted: bool,
}

impl AppComment {
    fn from_core(comment: &Comment) -> Self {
        Self {
            id: comment.id.to_string(),
            author: comment.author.clone(),
            body: comment
                .body
                .iter()
                .map(|inline| match inline {
                    Inline::Text { text, .. } => text.clone(),
                    Inline::Link { text, .. } => text.clone(),
                    _ => String::new(),
                })
                .collect::<Vec<_>>()
                .join(""),
            deleted: comment.deleted,
        }
    }

    fn to_core(&self) -> Result<Comment, AppApiError> {
        Ok(Comment {
            id: parse_id(&self.id)?,
            author: self.author.clone(),
            body: vec![Inline::text(self.body.clone())],
            created_at_ms: 0,
            deleted: self.deleted,
        })
    }

    pub(crate) fn validate_source(&self) -> Result<(), AppApiError> {
        parse_id(&self.id)?;
        if self.author.trim().is_empty() {
            return Err(AppApiError::Format("comment author is empty".to_string()));
        }
        if self.author.trim() != self.author {
            return Err(AppApiError::Format(
                "comment author has surrounding whitespace".to_string(),
            ));
        }
        if self.body.trim().is_empty() {
            return Err(AppApiError::Format("comment body is empty".to_string()));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppSuggestion {
    pub id: String,
    pub author: String,
    pub kind: String,
    pub text: String,
    pub state: String,
    pub anchor: Option<String>,
    #[serde(default)]
    pub anchor_label: Option<String>,
    pub range_start: Option<String>,
    pub range_end: Option<String>,
    pub marks: Vec<String>,
    pub content: Vec<AppInline>,
    pub provenance: Vec<String>,
}

impl AppSuggestion {
    pub(crate) fn from_core(
        suggestion: &Suggestion,
        citations: &opendoc_core::CitationDatabase,
        blocks: &[Block],
    ) -> Self {
        let (anchor, range_start, range_end, marks, content) = match &suggestion.kind {
            SuggestionKind::Insert { anchor, content } => (
                Some(anchor_label(anchor)),
                None,
                None,
                Vec::new(),
                content
                    .iter()
                    .map(|inline| AppInline::from_core(inline, citations))
                    .collect(),
            ),
            SuggestionKind::Delete { range } => (
                None,
                Some(range.start.to_string()),
                Some(range.end.to_string()),
                Vec::new(),
                Vec::new(),
            ),
            SuggestionKind::Format { range, marks } => (
                None,
                Some(range.start.to_string()),
                Some(range.end.to_string()),
                marks.iter().map(mark_label).collect(),
                Vec::new(),
            ),
        };
        let anchor_label = suggestion_anchor_display_label(&suggestion.kind, blocks);
        Self {
            id: suggestion.id.to_string(),
            author: suggestion.author.clone(),
            kind: match &suggestion.kind {
                SuggestionKind::Insert { .. } => "insert".to_string(),
                SuggestionKind::Delete { .. } => "delete".to_string(),
                SuggestionKind::Format { .. } => "format".to_string(),
            },
            text: suggestion_text(&suggestion.kind),
            state: match suggestion.state {
                SuggestionState::Proposed => "proposed".to_string(),
                SuggestionState::Accepted => "accepted".to_string(),
                SuggestionState::Rejected => "rejected".to_string(),
            },
            anchor,
            anchor_label,
            range_start,
            range_end,
            marks,
            content,
            provenance: suggestion.provenance.clone(),
        }
    }

    pub(crate) fn to_core(&self) -> Result<Suggestion, AppApiError> {
        Ok(Suggestion {
            id: parse_id(&self.id)?,
            author: self.author.clone(),
            kind: match self.kind.as_str() {
                "delete" => SuggestionKind::Delete {
                    range: self.to_range()?,
                },
                "format" => SuggestionKind::Format {
                    range: self.to_range()?,
                    marks: parse_marks(&self.marks)?,
                },
                "insert" => SuggestionKind::Insert {
                    anchor: self
                        .anchor
                        .as_deref()
                        .map(|anchor| parse_anchor_label(anchor, "suggestion anchor"))
                        .transpose()?
                        .unwrap_or(Anchor::Document),
                    content: if self.content.is_empty() {
                        vec![Inline::text(self.text.clone())]
                    } else {
                        self.content
                            .iter()
                            .map(AppInline::to_core)
                            .collect::<Result<Vec<_>, _>>()?
                    },
                },
                other => {
                    return Err(AppApiError::Format(format!(
                        "unsupported suggestion kind {other}"
                    )));
                }
            },
            state: match self.state.as_str() {
                "proposed" => SuggestionState::Proposed,
                "accepted" => SuggestionState::Accepted,
                "rejected" => SuggestionState::Rejected,
                other => {
                    return Err(AppApiError::Format(format!(
                        "unsupported suggestion state {other}"
                    )));
                }
            },
            provenance: self.provenance.clone(),
        })
    }

    fn to_range(&self) -> Result<TextRange, AppApiError> {
        Ok(TextRange {
            start: parse_id(self.range_start.as_deref().ok_or_else(|| {
                AppApiError::Format(format!("suggestion {} missing range_start", self.id))
            })?)?,
            end: parse_id(self.range_end.as_deref().ok_or_else(|| {
                AppApiError::Format(format!("suggestion {} missing range_end", self.id))
            })?)?,
        })
    }

    pub(crate) fn validate_source(&self) -> Result<(), AppApiError> {
        parse_id(&self.id)?;
        if self.author.trim().is_empty() {
            return Err(AppApiError::Format(
                "suggestion author is empty".to_string(),
            ));
        }
        if self.author.trim() != self.author {
            return Err(AppApiError::Format(
                "suggestion author has surrounding whitespace".to_string(),
            ));
        }
        for item in &self.provenance {
            if item.trim().is_empty() {
                return Err(AppApiError::Format(
                    "suggestion provenance entry is empty".to_string(),
                ));
            }
            if item.trim() != item {
                return Err(AppApiError::Format(
                    "suggestion provenance entry has surrounding whitespace".to_string(),
                ));
            }
        }
        match self.state.as_str() {
            "proposed" | "accepted" | "rejected" => {}
            other => {
                return Err(AppApiError::Format(format!(
                    "unsupported suggestion state {other}"
                )));
            }
        }
        match self.kind.as_str() {
            "insert" => {
                if self
                    .anchor
                    .as_deref()
                    .is_none_or(|anchor| anchor.trim().is_empty())
                {
                    return Err(AppApiError::Format(format!(
                        "suggestion {} missing anchor",
                        self.id
                    )));
                }
                if let Some(anchor) = &self.anchor {
                    parse_anchor_label(anchor, "suggestion anchor")?;
                }
                if self.content.is_empty() && self.text.trim().is_empty() {
                    return Err(AppApiError::Format(
                        "insert suggestion content is empty".to_string(),
                    ));
                }
                for inline in &self.content {
                    inline.to_core()?;
                }
            }
            "delete" => {
                self.to_range()?;
                if !self.marks.is_empty() || !self.content.is_empty() {
                    return Err(AppApiError::Format(format!(
                        "delete suggestion {} has non-delete payload",
                        self.id
                    )));
                }
            }
            "format" => {
                self.to_range()?;
                if self.marks.is_empty() {
                    return Err(AppApiError::Format(
                        "format suggestion marks are empty".to_string(),
                    ));
                }
                parse_marks(&self.marks)?;
                if !self.content.is_empty() {
                    return Err(AppApiError::Format(format!(
                        "format suggestion {} has insert content",
                        self.id
                    )));
                }
            }
            other => {
                return Err(AppApiError::Format(format!(
                    "unsupported suggestion kind {other}"
                )));
            }
        }
        Ok(())
    }
}
