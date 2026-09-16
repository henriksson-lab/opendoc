//! Comment thread, comment and suggestion projection DTOs.

use super::*;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppCommentThread {
    pub id: String,
    pub anchor: String,
    #[serde(default)]
    pub anchor_label: String,
    #[serde(default)]
    pub orphaned_quote: Option<String>,
    #[serde(default)]
    pub orphaned_context: Option<String>,
    /// Why the original target could no longer be resolved.  This is retained
    /// alongside the evidence so an app projection/save cycle does not
    /// replace an importer or merge warning with a generic local message.
    #[serde(default)]
    pub orphaned_warning: Option<String>,
    pub comments: Vec<AppComment>,
    pub state: String,
    pub resolved_by: Option<String>,
    pub resolved_at_ms: Option<u64>,
    pub action_assignee: Option<String>,
    pub action_due_at_ms: Option<u64>,
    pub action_completed_by: Option<String>,
    pub action_completed_at_ms: Option<u64>,
    #[serde(default)]
    pub reactions: Vec<AppCommentThreadReaction>,
    pub deleted: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppCommentThreadReaction {
    pub emoji: String,
    pub actors: Vec<String>,
}

impl AppCommentThread {
    pub(crate) fn from_core(thread: &CommentThread, blocks: &[Block]) -> Self {
        Self {
            id: thread.id.to_string(),
            anchor: anchor_label(&thread.anchor),
            anchor_label: anchor_display_label(&thread.anchor, blocks),
            orphaned_quote: match &thread.anchor {
                Anchor::Orphaned { quote, .. } => Some(quote.clone()),
                _ => None,
            },
            orphaned_context: match &thread.anchor {
                Anchor::Orphaned { context, .. } => Some(context.clone()),
                _ => None,
            },
            orphaned_warning: match &thread.anchor {
                Anchor::Orphaned { warning, .. } => Some(warning.clone()),
                _ => None,
            },
            comments: thread.comments.iter().map(AppComment::from_core).collect(),
            state: match thread.state {
                opendoc_core::CommentThreadState::Open => "open",
                opendoc_core::CommentThreadState::Resolved => "resolved",
                opendoc_core::CommentThreadState::Reopened => "reopened",
            }
            .to_string(),
            resolved_by: thread.resolved_by.clone(),
            resolved_at_ms: thread.resolved_at_ms,
            action_assignee: thread.action_assignee.clone(),
            action_due_at_ms: thread.action_due_at_ms,
            action_completed_by: thread.action_completed_by.clone(),
            action_completed_at_ms: thread.action_completed_at_ms,
            reactions: thread
                .reactions
                .iter()
                .map(|reaction| AppCommentThreadReaction {
                    emoji: reaction.emoji.clone(),
                    actors: reaction.actors.clone(),
                })
                .collect(),
            deleted: thread.deleted,
        }
    }

    pub(crate) fn to_core(&self) -> Result<CommentThread, AppApiError> {
        Ok(CommentThread {
            id: parse_id(&self.id)?,
            anchor: if self.anchor == "orphaned" {
                Anchor::Orphaned {
                    quote: required_orphaned_evidence(&self.orphaned_quote, "quote")?,
                    context: required_orphaned_evidence(&self.orphaned_context, "context")?,
                    // Older app DTOs did not carry the warning.  Keep those
                    // readable, but never overwrite a warning that was
                    // actually supplied by a newer importer or projection.
                    warning: optional_orphaned_warning(&self.orphaned_warning)?
                        .unwrap_or_else(|| "imported orphaned comment anchor".to_string()),
                }
            } else {
                parse_anchor_label(&self.anchor, "comment anchor")?
            },
            comments: self
                .comments
                .iter()
                .map(AppComment::to_core)
                .collect::<Result<Vec<_>, _>>()?,
            state: match self.state.as_str() {
                "open" => opendoc_core::CommentThreadState::Open,
                "resolved" => opendoc_core::CommentThreadState::Resolved,
                "reopened" => opendoc_core::CommentThreadState::Reopened,
                _ => {
                    return Err(AppApiError::Format(
                        "invalid comment thread state".to_string(),
                    ))
                }
            },
            resolved_by: self.resolved_by.clone(),
            resolved_at_ms: self.resolved_at_ms,
            action_assignee: self.action_assignee.clone(),
            action_due_at_ms: self.action_due_at_ms,
            action_completed_by: self.action_completed_by.clone(),
            action_completed_at_ms: self.action_completed_at_ms,
            reactions: self
                .reactions
                .iter()
                .map(|reaction| opendoc_core::CommentThreadReaction {
                    emoji: reaction.emoji.clone(),
                    actors: reaction.actors.clone(),
                })
                .collect(),
            deleted: self.deleted,
        })
    }

    pub(crate) fn validate_source(&self) -> Result<(), AppApiError> {
        parse_id(&self.id)?;
        if self.anchor == "orphaned" {
            required_orphaned_evidence(&self.orphaned_quote, "quote")?;
            required_orphaned_evidence(&self.orphaned_context, "context")?;
            optional_orphaned_warning(&self.orphaned_warning)?;
        } else {
            parse_anchor_label(&self.anchor, "comment anchor")?;
        }
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

fn required_orphaned_evidence(value: &Option<String>, field: &str) -> Result<String, AppApiError> {
    let value = value
        .as_deref()
        .ok_or_else(|| AppApiError::Format(format!("orphaned comment anchor has no {field}")))?;
    if value.trim().is_empty() {
        return Err(AppApiError::Format(format!(
            "orphaned comment anchor {field} is invalid"
        )));
    }
    Ok(value.to_string())
}

fn optional_orphaned_warning(value: &Option<String>) -> Result<Option<String>, AppApiError> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.trim().is_empty() || value.trim() != value {
        return Err(AppApiError::Format(
            "orphaned comment anchor warning is invalid".to_string(),
        ));
    }
    Ok(Some(value.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn orphaned_thread(warning: &str) -> CommentThread {
        CommentThread {
            id: StableId::parse("orphaned-projection-thread").unwrap(),
            anchor: Anchor::Orphaned {
                quote: "removed source".to_string(),
                context: "the paragraph before removal".to_string(),
                warning: warning.to_string(),
            },
            comments: vec![Comment {
                id: StableId::parse("orphaned-projection-comment").unwrap(),
                author: "reviewer".to_string(),
                body: vec![Inline::text("Please revise")],
                created_at_ms: 7,
                deleted: false,
            }],
            state: opendoc_core::CommentThreadState::Open,
            resolved_by: None,
            resolved_at_ms: None,
            action_assignee: None,
            action_due_at_ms: None,
            action_completed_by: None,
            action_completed_at_ms: None,
            reactions: Vec::new(),
            deleted: false,
        }
    }

    #[test]
    fn orphaned_anchor_warning_survives_app_projection_round_trip() {
        let thread = orphaned_thread("import retained deleted-anchor evidence");
        let dto = AppCommentThread::from_core(&thread, &[]);

        assert_eq!(
            dto.orphaned_warning.as_deref(),
            Some("import retained deleted-anchor evidence")
        );
        assert_eq!(dto.to_core().unwrap().anchor, thread.anchor);
    }

    #[test]
    fn legacy_orphaned_projection_without_warning_remains_readable() {
        let thread = orphaned_thread("ignored");
        let mut dto = AppCommentThread::from_core(&thread, &[]);
        dto.orphaned_warning = None;

        assert!(matches!(
            dto.to_core().unwrap().anchor,
            Anchor::Orphaned { warning, .. } if warning == "imported orphaned comment anchor"
        ));
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppComment {
    pub id: String,
    pub author: String,
    pub body: String,
    /// Source creation time from native/imported comment metadata.  This is
    /// data, not a display timestamp: preserving it keeps a projection/save
    /// cycle from rewriting the order and provenance of an imported thread.
    pub created_at_ms: u64,
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
            created_at_ms: comment.created_at_ms,
            deleted: comment.deleted,
        }
    }

    fn to_core(&self) -> Result<Comment, AppApiError> {
        Ok(Comment {
            id: parse_id(&self.id)?,
            author: self.author.clone(),
            body: vec![Inline::text(self.body.clone())],
            created_at_ms: self.created_at_ms,
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
    #[serde(default)]
    pub block_id: Option<String>,
    /// The stable identity of the proposed paragraph for a structural insert
    /// or replacement. Structural tracked changes are intentionally limited
    /// to plain paragraphs, so `text` is the complete payload.
    #[serde(default)]
    pub structural_block_id: Option<String>,
    /// Complete source paragraph snapshot for a block-replacement proposal.
    /// It is a compare-and-set precondition, not a second visible replacement
    /// text field: accepting must not overwrite a collaborator's newer edit.
    #[serde(default)]
    pub structural_expected_block: Option<AppBlock>,
    /// `first`, `last`, `before:<id>` or `after:<id>` for a block insert.
    #[serde(default)]
    pub block_position: Option<String>,
    /// Atomic target for a proposed link add, removal, or replacement.
    #[serde(default)]
    pub link_inline_id: Option<String>,
    #[serde(default)]
    pub link_expected_href: Option<String>,
    #[serde(default)]
    pub link_href: Option<String>,
    /// Source and proposed values for a preconditioned whole-paragraph style
    /// suggestion.  These are model vocabulary (`paragraph`, `title`,
    /// `subtitle`, `heading:<1..=6>`), never a presentation label.
    #[serde(default)]
    pub paragraph_style_expected: Option<String>,
    #[serde(default)]
    pub paragraph_style_proposed: Option<String>,
    /// The source value reviewed by a compare-and-set format replacement.
    #[serde(default)]
    pub format_expected_value: Option<String>,
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
        let (
            anchor,
            range_start,
            range_end,
            block_id,
            structural_block_id,
            block_position,
            marks,
            content,
        ) = match &suggestion.kind {
            SuggestionKind::Insert { anchor, content } => (
                Some(anchor_label(anchor)),
                None,
                None,
                None,
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
                None,
                None,
                None,
                Vec::new(),
                Vec::new(),
            ),
            SuggestionKind::Format { range, marks } => (
                None,
                Some(range.start.to_string()),
                Some(range.end.to_string()),
                None,
                None,
                None,
                marks.iter().map(mark_label).collect(),
                Vec::new(),
            ),
            SuggestionKind::FormatRemove { range, kind, value } => (
                None,
                Some(range.start.to_string()),
                Some(range.end.to_string()),
                None,
                None,
                None,
                vec![mark_label(&opendoc_core::Mark {
                    kind: kind.clone(),
                    value: value.clone(),
                    expand: opendoc_core::MarkExpand::Both,
                })],
                Vec::new(),
            ),
            SuggestionKind::FormatReplace {
                range, kind, value, ..
            } => (
                None,
                Some(range.start.to_string()),
                Some(range.end.to_string()),
                None,
                None,
                None,
                vec![mark_label(&opendoc_core::Mark {
                    kind: kind.clone(),
                    value: Some(value.clone()),
                    expand: opendoc_core::MarkExpand::Both,
                })],
                Vec::new(),
            ),
            SuggestionKind::LinkChange { .. } => {
                (None, None, None, None, None, None, Vec::new(), Vec::new())
            }
            SuggestionKind::BlockDelete { block_id } => (
                None,
                None,
                None,
                Some(block_id.to_string()),
                None,
                None,
                Vec::new(),
                Vec::new(),
            ),
            SuggestionKind::BlockInsert { position, block } => (
                None,
                None,
                None,
                None,
                Some(block.id.to_string()),
                Some(structural_position_label(position)),
                Vec::new(),
                Vec::new(),
            ),
            SuggestionKind::BlockReplace {
                block_id,
                replacement,
                ..
            } => (
                None,
                None,
                None,
                Some(block_id.to_string()),
                Some(replacement.id.to_string()),
                None,
                Vec::new(),
                Vec::new(),
            ),
            SuggestionKind::ParagraphStyleChange { block_id, .. } => (
                None,
                None,
                None,
                Some(block_id.to_string()),
                None,
                None,
                Vec::new(),
                Vec::new(),
            ),
        };
        let anchor_label = suggestion_anchor_display_label(&suggestion.kind, blocks);
        let (link_inline_id, link_expected_href, link_href) = match &suggestion.kind {
            SuggestionKind::LinkChange {
                inline_id,
                expected_href,
                href,
            } => (
                Some(inline_id.to_string()),
                expected_href.clone(),
                href.clone(),
            ),
            _ => (None, None, None),
        };
        let (paragraph_style_expected, paragraph_style_proposed) = match &suggestion.kind {
            SuggestionKind::ParagraphStyleChange {
                expected, proposed, ..
            } => (
                Some(paragraph_style_label(expected)),
                Some(paragraph_style_label(proposed)),
            ),
            _ => (None, None),
        };
        let format_expected_value = match &suggestion.kind {
            SuggestionKind::FormatReplace { expected_value, .. } => Some(expected_value.clone()),
            _ => None,
        };
        let structural_expected_block = match &suggestion.kind {
            SuggestionKind::BlockReplace { expected, .. } => {
                Some(AppBlock::from_core(expected, citations))
            }
            _ => None,
        };
        Self {
            id: suggestion.id.to_string(),
            author: suggestion.author.clone(),
            kind: match &suggestion.kind {
                SuggestionKind::Insert { .. } => "insert".to_string(),
                SuggestionKind::Delete { .. } => "delete".to_string(),
                SuggestionKind::Format { .. } => "format".to_string(),
                SuggestionKind::FormatRemove { .. } => "format_remove".to_string(),
                SuggestionKind::FormatReplace { .. } => "format_replace".to_string(),
                SuggestionKind::LinkChange { .. } => "link_change".to_string(),
                SuggestionKind::BlockDelete { .. } => "block_delete".to_string(),
                SuggestionKind::BlockInsert { .. } => "block_insert".to_string(),
                SuggestionKind::BlockReplace { .. } => "block_replace".to_string(),
                SuggestionKind::ParagraphStyleChange { .. } => "paragraph_style_change".to_string(),
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
            block_id,
            structural_block_id,
            structural_expected_block,
            block_position,
            link_inline_id,
            link_expected_href,
            link_href,
            paragraph_style_expected,
            paragraph_style_proposed,
            format_expected_value,
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
                "format_remove" => {
                    let marks = parse_marks(&self.marks)?;
                    if marks.len() != 1 {
                        return Err(AppApiError::Format(format!(
                            "format removal suggestion {} must name exactly one mark",
                            self.id
                        )));
                    }
                    let mark = marks.into_iter().next().expect("length checked");
                    validate_mark_removal_payload(&mark.kind, mark.value.as_deref())?;
                    SuggestionKind::FormatRemove {
                        range: self.to_range()?,
                        kind: mark.kind,
                        value: mark.value,
                    }
                }
                "format_replace" => {
                    let marks = parse_marks(&self.marks)?;
                    let [mark] = marks.as_slice() else {
                        return Err(AppApiError::Format(format!(
                            "format replacement suggestion {} must name exactly one mark",
                            self.id
                        )));
                    };
                    let expected_value = self.format_expected_value.clone().ok_or_else(|| {
                        AppApiError::Format(format!(
                            "format replacement suggestion {} has no expected value",
                            self.id
                        ))
                    })?;
                    let value = mark.value.clone().ok_or_else(|| {
                        AppApiError::Format(format!(
                            "format replacement suggestion {} has no replacement value",
                            self.id
                        ))
                    })?;
                    validate_format_replacement_payload(&mark.kind, &expected_value, &value)?;
                    SuggestionKind::FormatReplace {
                        range: self.to_range()?,
                        kind: mark.kind.clone(),
                        expected_value,
                        value,
                    }
                }
                "link_change" => SuggestionKind::LinkChange {
                    inline_id: parse_id(self.link_inline_id.as_deref().ok_or_else(|| {
                        AppApiError::Format(format!(
                            "link suggestion {} has no target inline id",
                            self.id
                        ))
                    })?)?,
                    expected_href: self.link_expected_href.clone(),
                    href: self.link_href.clone(),
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
                "block_delete" => SuggestionKind::BlockDelete {
                    block_id: parse_id(self.block_id.as_deref().ok_or_else(|| {
                        AppApiError::Format(format!(
                            "block deletion suggestion {} has no block id",
                            self.id
                        ))
                    })?)?,
                },
                "block_insert" => SuggestionKind::BlockInsert {
                    position: parse_structural_position(self.block_position.as_deref())?,
                    block: structural_suggestion_block(
                        self.structural_block_id.as_deref(),
                        &self.text,
                    )?,
                },
                "block_replace" => {
                    SuggestionKind::BlockReplace {
                        block_id: parse_id(self.block_id.as_deref().ok_or_else(|| {
                            AppApiError::Format(format!(
                                "block replacement suggestion {} has no target block id",
                                self.id
                            ))
                        })?)?,
                        expected: Box::new(
                            self.structural_expected_block
                                .as_ref()
                                .ok_or_else(|| {
                                    AppApiError::Format(format!(
                                    "block replacement suggestion {} has no source precondition",
                                    self.id
                                ))
                                })?
                                .to_core()?,
                        ),
                        replacement: Box::new({
                            let target = parse_id(self.block_id.as_deref().ok_or_else(|| {
                                AppApiError::Format(
                                    "block replacement suggestion missing target block id"
                                        .to_string(),
                                )
                            })?)?;
                            let replacement = structural_suggestion_block(
                                self.structural_block_id.as_deref(),
                                &self.text,
                            )?;
                            if replacement.id != target {
                                return Err(AppApiError::Format("structural suggestion replacement must preserve target block id".to_string()));
                            }
                            replacement
                        }),
                    }
                }
                "paragraph_style_change" => SuggestionKind::ParagraphStyleChange {
                    block_id: parse_id(self.block_id.as_deref().ok_or_else(|| {
                        AppApiError::Format(format!(
                            "paragraph style suggestion {} has no target block id",
                            self.id
                        ))
                    })?)?,
                    expected: parse_paragraph_style(
                        self.paragraph_style_expected.as_deref(),
                        "expected",
                    )?,
                    proposed: parse_paragraph_style(
                        self.paragraph_style_proposed.as_deref(),
                        "proposed",
                    )?,
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
            "format_remove" => {
                self.to_range()?;
                let marks = parse_marks(&self.marks)?;
                if marks.len() != 1 {
                    return Err(AppApiError::Format(
                        "format removal suggestion must name exactly one mark".to_string(),
                    ));
                }
                let mark = &marks[0];
                validate_mark_removal_payload(&mark.kind, mark.value.as_deref())?;
                if !self.content.is_empty() {
                    return Err(AppApiError::Format(format!(
                        "format removal suggestion {} has insert content",
                        self.id
                    )));
                }
            }
            "block_delete" => {
                parse_id(self.block_id.as_deref().ok_or_else(|| {
                    AppApiError::Format("block deletion suggestion missing block id".to_string())
                })?)?;
            }
            "block_insert" => {
                parse_structural_position(self.block_position.as_deref())?;
                structural_suggestion_block(self.structural_block_id.as_deref(), &self.text)?;
            }
            "block_replace" => {
                parse_id(self.block_id.as_deref().ok_or_else(|| {
                    AppApiError::Format(
                        "block replacement suggestion missing target block id".to_string(),
                    )
                })?)?;
                structural_suggestion_block(self.structural_block_id.as_deref(), &self.text)?;
                let expected = self
                    .structural_expected_block
                    .as_ref()
                    .ok_or_else(|| {
                        AppApiError::Format(
                            "block replacement suggestion missing source precondition".to_string(),
                        )
                    })?
                    .to_core()?;
                if expected.id
                    != parse_id(self.block_id.as_deref().ok_or_else(|| {
                        AppApiError::Format(
                            "block replacement suggestion missing target block id".to_string(),
                        )
                    })?)?
                {
                    return Err(AppApiError::Format(
                        "block replacement source precondition must preserve target block id"
                            .to_string(),
                    ));
                }
            }
            "paragraph_style_change" => {
                parse_id(self.block_id.as_deref().ok_or_else(|| {
                    AppApiError::Format(
                        "paragraph style suggestion missing target block id".to_string(),
                    )
                })?)?;
                let expected =
                    parse_paragraph_style(self.paragraph_style_expected.as_deref(), "expected")?;
                let proposed =
                    parse_paragraph_style(self.paragraph_style_proposed.as_deref(), "proposed")?;
                if expected == proposed {
                    return Err(AppApiError::Format(
                        "paragraph style suggestion does not change the source style".to_string(),
                    ));
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

pub(crate) fn paragraph_style_label(style: &ParagraphStyle) -> String {
    match style {
        ParagraphStyle::Paragraph => "paragraph".to_string(),
        ParagraphStyle::Title => "title".to_string(),
        ParagraphStyle::Subtitle => "subtitle".to_string(),
        ParagraphStyle::Heading { level } => format!("heading:{level}"),
    }
}

pub(crate) fn parse_paragraph_style(
    value: Option<&str>,
    role: &str,
) -> Result<ParagraphStyle, AppApiError> {
    let value = value.ok_or_else(|| {
        AppApiError::Format(format!("paragraph style suggestion missing {role} style"))
    })?;
    let style = match value {
        "paragraph" => ParagraphStyle::Paragraph,
        "title" => ParagraphStyle::Title,
        "subtitle" => ParagraphStyle::Subtitle,
        _ => {
            let Some(level) = value.strip_prefix("heading:") else {
                return Err(AppApiError::Format(format!(
                    "invalid {role} paragraph style {value:?}"
                )));
            };
            let level = level.parse::<u8>().map_err(|_| {
                AppApiError::Format(format!("invalid {role} paragraph heading level"))
            })?;
            ParagraphStyle::Heading { level }
        }
    };
    style
        .validate()
        .map_err(|error| AppApiError::Format(error.to_string()))?;
    Ok(style)
}

fn structural_suggestion_block(id: Option<&str>, text: &str) -> Result<Block, AppApiError> {
    let text = normalize_source_text(text.to_string(), "structural suggestion paragraph")?;
    let mut block = Block::paragraph(text);
    block.id = parse_id(id.ok_or_else(|| {
        AppApiError::Format("structural suggestion missing proposed block id".to_string())
    })?)?;
    Ok(block)
}

fn structural_position_label(position: &opendoc_core::InsertPosition) -> String {
    match position {
        opendoc_core::InsertPosition::First => "first".to_string(),
        opendoc_core::InsertPosition::Last => "last".to_string(),
        opendoc_core::InsertPosition::Before(id) => format!("before:{id}"),
        opendoc_core::InsertPosition::After(id) => format!("after:{id}"),
    }
}

fn parse_structural_position(
    value: Option<&str>,
) -> Result<opendoc_core::InsertPosition, AppApiError> {
    let value = value.ok_or_else(|| {
        AppApiError::Format("block insertion suggestion missing position".to_string())
    })?;
    if value == "first" {
        return Ok(opendoc_core::InsertPosition::First);
    }
    if value == "last" {
        return Ok(opendoc_core::InsertPosition::Last);
    }
    for (prefix, before) in [("before:", true), ("after:", false)] {
        if let Some(id) = value.strip_prefix(prefix) {
            let id = parse_id(id)?;
            return Ok(if before {
                opendoc_core::InsertPosition::Before(id)
            } else {
                opendoc_core::InsertPosition::After(id)
            });
        }
    }
    Err(AppApiError::Format(
        "invalid structural suggestion position".to_string(),
    ))
}
