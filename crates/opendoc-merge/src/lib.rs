use opendoc_core::{
    Anchor, BibliographyReference, Block, BlockKind, CitationGroup, CitationPlacement,
    CommentThread, Document, Footnote, HashRef, Inline, Mark, MarkKind, ModelError, ModelWarning,
    StableId, Suggestion, SuggestionKind, SuggestionState, TableCell, TableRow,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct ActorId(pub String);

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct OperationId {
    pub actor: ActorId,
    pub seq: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Operation {
    pub id: OperationId,
    pub kind: OperationKind,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum BlockTextStyle {
    Paragraph,
    Heading {
        level: u8,
    },
    ListItem {
        list_id: StableId,
        level: u8,
        ordered: bool,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum OperationKind {
    SetDocumentTitle {
        title: String,
    },
    SetDocumentDoi {
        doi: Option<String>,
    },
    SetDocumentLocale {
        locale: String,
    },
    InsertBlock {
        after: Option<StableId>,
        block: Block,
    },
    DeleteBlock {
        block_id: StableId,
    },
    SetBlockTextStyle {
        block_id: StableId,
        style: BlockTextStyle,
    },
    InsertInline {
        block_id: StableId,
        after: Option<StableId>,
        inline: Inline,
    },
    MoveInlineToBlock {
        inline_id: StableId,
        target_block_id: StableId,
        after: Option<StableId>,
    },
    AddMark {
        text_id: StableId,
        mark: Mark,
    },
    RemoveMark {
        text_id: StableId,
        kind: MarkKind,
        value: Option<String>,
    },
    AddMarkRange {
        range: opendoc_core::TextRange,
        mark: Mark,
    },
    AddSuggestion {
        suggestion: Suggestion,
    },
    UpdateSuggestionInsertContent {
        suggestion_id: StableId,
        content: Vec<Inline>,
    },
    AddCommentThread {
        thread: CommentThread,
    },
    AddCommentReply {
        thread_id: StableId,
        comment: opendoc_core::Comment,
    },
    UpsertFootnote {
        footnote: Footnote,
    },
    UpsertBibliographyReference {
        reference: BibliographyReference,
    },
    DeleteBibliographyReference {
        reference_id: StableId,
        revision: u64,
    },
    UpsertCitationGroup {
        citation: CitationGroup,
    },
    DeleteCitationGroup {
        citation_id: StableId,
        revision: u64,
    },
    UpdateCitationStyle {
        style: String,
        locale: String,
    },
    DeleteCommentThread {
        thread_id: StableId,
    },
    RestoreCommentThread {
        thread_id: StableId,
    },
    DeleteComment {
        thread_id: StableId,
        comment_id: StableId,
    },
    RestoreComment {
        thread_id: StableId,
        comment_id: StableId,
    },
    UpdateCommentBody {
        thread_id: StableId,
        comment_id: StableId,
        body: Vec<Inline>,
    },
    UpdateInlineText {
        inline_id: StableId,
        text: String,
    },
    /// Character-level insert into a text or link run. `offset` counts
    /// Unicode scalar values from the start of the run and is clamped to
    /// the run length when concurrent edits shortened it.
    InsertText {
        inline_id: StableId,
        offset: usize,
        text: String,
    },
    /// Character-level delete of `[start, end)` from a text or link run.
    /// Offsets count Unicode scalar values and are clamped to the run.
    DeleteText {
        inline_id: StableId,
        start: usize,
        end: usize,
    },
    UpdateInlineEquationSource {
        inline_id: StableId,
        source: String,
    },
    UpdateMentionLabel {
        inline_id: StableId,
        label: String,
    },
    UpdateLinkHref {
        inline_id: StableId,
        href: String,
    },
    UpdateBlockEquationSource {
        block_id: StableId,
        source: String,
    },
    UpdateImageAltText {
        block_id: StableId,
        alt_text: String,
    },
    UpdateImageBlobHash {
        block_id: StableId,
        blob_hash: String,
    },
    UpdateHeadingLevel {
        block_id: StableId,
        level: u8,
    },
    UpdateListItem {
        block_id: StableId,
        level: u8,
        ordered: bool,
    },
    AcceptSuggestion {
        suggestion_id: StableId,
        accepted_by: String,
    },
    RejectSuggestion {
        suggestion_id: StableId,
        rejected_by: String,
    },
    DeleteInline {
        inline_id: StableId,
    },
    InsertTableRow {
        table_block_id: StableId,
        after_row: Option<StableId>,
        row: TableRow,
    },
    DeleteTableRow {
        table_block_id: StableId,
        row_id: StableId,
    },
    InsertTableCell {
        table_block_id: StableId,
        row_id: StableId,
        after_cell: Option<StableId>,
        cell: TableCell,
    },
    DeleteTableCell {
        table_block_id: StableId,
        row_id: StableId,
        cell_id: StableId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MergeResult {
    pub document: Document,
    pub warnings: Vec<ModelWarning>,
}

pub fn merge_operations(
    base: &Document,
    streams: &[Vec<Operation>],
) -> Result<MergeResult, ModelError> {
    let mut ordered = BTreeMap::new();
    let mut duplicate_operation_ids = BTreeSet::new();
    let mut invalid_operation_id_warnings = BTreeSet::new();
    let mut warnings = Vec::new();
    for stream in streams {
        for op in stream {
            if let Some(message) = validate_operation_id_for_merge(&op.id) {
                invalid_operation_id_warnings.insert(message);
                continue;
            }
            match ordered.get_mut(&op.id) {
                Some(existing) => {
                    duplicate_operation_ids.insert(op.id.clone());
                    if operation_kind_sort_key(&op.kind) < operation_kind_sort_key(existing) {
                        *existing = op.kind.clone();
                    }
                }
                None => {
                    ordered.insert(op.id.clone(), op.kind.clone());
                }
            }
        }
    }

    let mut document = base.clone();
    for message in invalid_operation_id_warnings {
        warnings.push(ModelWarning {
            code: "invalid-operation-id".to_string(),
            message: message.to_string(),
        });
    }
    for duplicate_id in duplicate_operation_ids {
        warnings.push(ModelWarning {
            code: "duplicate-operation-id".to_string(),
            message: format!(
                "operation {}#{} was duplicated; deterministic operation payload was selected",
                duplicate_id.actor.0, duplicate_id.seq
            ),
        });
    }
    let mut suggestion_resolution_actors: BTreeMap<StableId, BTreeSet<ActorId>> = BTreeMap::new();
    let mut comment_thread_delete_actors: BTreeMap<StableId, BTreeSet<ActorId>> = BTreeMap::new();
    let mut comment_thread_closing_delete_actors: BTreeMap<StableId, BTreeSet<ActorId>> =
        BTreeMap::new();
    let mut comment_delete_actors: BTreeMap<(StableId, StableId), BTreeSet<ActorId>> =
        BTreeMap::new();
    for (op_id, kind) in &ordered {
        match kind {
            OperationKind::AcceptSuggestion { suggestion_id, .. }
            | OperationKind::RejectSuggestion { suggestion_id, .. } => {
                suggestion_resolution_actors
                    .entry(suggestion_id.clone())
                    .or_default()
                    .insert(op_id.actor.clone());
            }
            OperationKind::DeleteCommentThread { thread_id } => {
                comment_thread_delete_actors
                    .entry(thread_id.clone())
                    .or_default()
                    .insert(op_id.actor.clone());
            }
            OperationKind::DeleteComment {
                thread_id,
                comment_id,
            } => {
                comment_delete_actors
                    .entry((thread_id.clone(), comment_id.clone()))
                    .or_default()
                    .insert(op_id.actor.clone());
                if base
                    .comments
                    .iter()
                    .find(|thread| thread.id == *thread_id && !thread.deleted)
                    .is_some_and(|thread| {
                        let mut live_comments =
                            thread.comments.iter().filter(|comment| !comment.deleted);
                        live_comments
                            .next()
                            .is_some_and(|comment| comment.id == *comment_id)
                            && live_comments.next().is_none()
                    })
                {
                    comment_thread_closing_delete_actors
                        .entry(thread_id.clone())
                        .or_default()
                        .insert(op_id.actor.clone());
                }
            }
            _ => {}
        }
    }
    let mut mark_ranges = Vec::new();
    let mut suggestion_resolutions = Vec::new();
    let mut comment_restores = Vec::new();
    for (op_id, kind) in ordered {
        match kind {
            OperationKind::UpdateSuggestionInsertContent { suggestion_id, .. }
                if suggestion_resolution_actors
                    .get(&suggestion_id)
                    .is_some_and(|actors| !actors.contains(&op_id.actor)) =>
            {
                warnings.push(ModelWarning {
                    code: "stale-suggestion-update".to_string(),
                    message: format!(
                        "suggestion {suggestion_id} content update from {} was ignored because another actor resolved it",
                        op_id.actor.0
                    ),
                });
            }
            OperationKind::AcceptSuggestion { .. } | OperationKind::RejectSuggestion { .. } => {
                suggestion_resolutions.push(kind);
            }
            OperationKind::AddCommentReply { thread_id, .. }
                if comment_thread_delete_actors
                    .get(&thread_id)
                    .is_some_and(|actors| !actors.contains(&op_id.actor)) =>
            {
                warnings.push(ModelWarning {
                    code: "stale-comment-reply".to_string(),
                    message: format!(
                        "comment reply for thread {thread_id} from {} was ignored because another actor deleted the thread",
                        op_id.actor.0
                    ),
                });
            }
            OperationKind::AddCommentReply { thread_id, .. }
                if comment_thread_closing_delete_actors
                    .get(&thread_id)
                    .is_some_and(|actors| !actors.contains(&op_id.actor)) =>
            {
                warnings.push(ModelWarning {
                    code: "stale-comment-reply".to_string(),
                    message: format!(
                        "comment reply for thread {thread_id} from {} was ignored because another actor deleted the thread's last live comment",
                        op_id.actor.0
                    ),
                });
            }
            OperationKind::UpdateCommentBody {
                thread_id,
                comment_id,
                ..
            } if comment_thread_delete_actors
                .get(&thread_id)
                .is_some_and(|actors| !actors.contains(&op_id.actor))
                || comment_delete_actors
                    .get(&(thread_id.clone(), comment_id.clone()))
                    .is_some_and(|actors| !actors.contains(&op_id.actor)) =>
            {
                warnings.push(ModelWarning {
                    code: "stale-comment-update".to_string(),
                    message: format!(
                        "comment {comment_id} update from {} was ignored because another actor deleted the comment or thread",
                        op_id.actor.0
                    ),
                });
            }
            OperationKind::AddMarkRange { range, mark } => mark_ranges.push((range, mark)),
            OperationKind::RestoreCommentThread { .. } | OperationKind::RestoreComment { .. } => {
                comment_restores.push(kind);
            }
            kind => apply(&mut document, &mut warnings, kind),
        }
    }
    for kind in suggestion_resolutions {
        apply(&mut document, &mut warnings, kind);
    }
    for kind in comment_restores {
        apply(&mut document, &mut warnings, kind);
    }
    for (range, mark) in mark_ranges {
        if marks_valid_for_merge(
            std::slice::from_ref(&mark),
            &mut warnings,
            "mark range operation",
            &range.start,
        ) {
            apply_mark_range(&mut document, &mut warnings, range, mark);
        }
    }
    repair_comment_anchors(&mut document, &mut warnings);
    repair_suggestion_anchors(&mut document, &mut warnings);
    repair_citation_placements(&mut document, &mut warnings);
    repair_missing_footnote_references(&mut document, &mut warnings);
    repair_unreferenced_footnotes(&mut document, &mut warnings);
    repair_citation_references(&mut document, &mut warnings);
    repair_inline_citation_labels(&mut document, &mut warnings);
    refresh_citation_projection_caches(&mut document);
    document.warnings.extend(warnings.clone());
    document.validate()?;
    Ok(MergeResult { document, warnings })
}

fn operation_kind_sort_key(kind: &OperationKind) -> String {
    format!("{kind:?}")
}

fn validate_operation_id_for_merge(id: &OperationId) -> Option<&'static str> {
    if id.actor.0.trim().is_empty() {
        return Some("operation with empty actor was ignored");
    }
    if id.actor.0.trim() != id.actor.0 {
        return Some("operation with whitespace-padded actor was ignored");
    }
    if id.seq == 0 {
        return Some("operation with zero sequence was ignored");
    }
    None
}

fn apply(document: &mut Document, warnings: &mut Vec<ModelWarning>, kind: OperationKind) {
    match kind {
        OperationKind::SetDocumentTitle { title } => {
            if title.trim().is_empty() {
                warnings.push(ModelWarning {
                    code: "invalid-document-title".to_string(),
                    message: "empty document title update was ignored".to_string(),
                });
            } else {
                document.title = title.trim().to_string();
            }
        }
        OperationKind::SetDocumentDoi { doi } => match doi {
            Some(doi) if doi.trim().is_empty() => {
                warnings.push(ModelWarning {
                    code: "invalid-document-doi".to_string(),
                    message: "empty document DOI update was ignored".to_string(),
                });
            }
            Some(doi) => {
                document.doi = Some(doi.trim().to_string());
            }
            None => {
                document.doi = None;
            }
        },
        OperationKind::SetDocumentLocale { locale } => {
            if locale.trim().is_empty() {
                warnings.push(ModelWarning {
                    code: "invalid-document-locale".to_string(),
                    message: "empty document locale update was ignored".to_string(),
                });
            } else {
                document.locale = locale.trim().to_string();
            }
        }
        OperationKind::InsertBlock { after, block } => {
            if !insert_block_payload_valid_for_merge(&block, warnings) {
                return;
            }
            if block_exists(&document.blocks, &block.id) {
                warnings.push(ModelWarning {
                    code: "duplicate-block".to_string(),
                    message: format!("block {} was already present", block.id),
                });
            } else {
                let block_id = block.id.clone();
                if insert_block(document, after, block) {
                    warnings.push(ModelWarning {
                        code: "block-anchor-degraded".to_string(),
                        message: format!(
                            "block insert anchor was missing; block {block_id} was appended"
                        ),
                    });
                }
            }
        }
        OperationKind::DeleteBlock { block_id } => {
            if !delete_block(document, &block_id) {
                warnings.push(ModelWarning {
                    code: "missing-block".to_string(),
                    message: format!("block {block_id} was already absent"),
                });
            }
        }
        OperationKind::SetBlockTextStyle { block_id, style } => {
            if let Err(message) = validate_block_text_style(&style) {
                warnings.push(ModelWarning {
                    code: "invalid-block-text-style".to_string(),
                    message: format!("block {block_id} ignored invalid text style: {message}"),
                });
                return;
            }
            match set_block_text_style(&mut document.blocks, &block_id, style) {
                Some(true) => {}
                Some(false) => warnings.push(ModelWarning {
                    code: "non-text-block".to_string(),
                    message: format!("block {block_id} is not a paragraph, heading, or list item"),
                }),
                None => warnings.push(ModelWarning {
                    code: "missing-block".to_string(),
                    message: format!("block text style target {block_id} was missing"),
                }),
            }
        }
        OperationKind::InsertInline {
            block_id,
            after,
            inline,
        } => {
            if !inline_payload_valid_for_merge(&inline, warnings, "inserted inline") {
                return;
            }
            let inline_id = inline_id(&inline).clone();
            if inline_exists(&document.blocks, &inline_id) {
                warnings.push(ModelWarning {
                    code: "duplicate-inline".to_string(),
                    message: format!("inline {inline_id} was already present"),
                });
                return;
            }
            if let Some(block) = find_block_mut(&mut document.blocks, &block_id) {
                if insert_inline(&mut block.content, after, inline) {
                    warnings.push(ModelWarning {
                        code: "inline-anchor-degraded".to_string(),
                        message: format!(
                            "inline insert anchor in block {block_id} was missing; inline was appended"
                        ),
                    });
                }
            } else if insert_inlines_at_document_end(document, vec![inline]) {
                warnings.push(ModelWarning {
                    code: "inline-anchor-degraded".to_string(),
                    message: format!(
                        "inline insert target block {block_id} was missing; inline was appended"
                    ),
                });
            } else {
                warnings.push(ModelWarning {
                    code: "missing-block".to_string(),
                    message: format!("inline insert target block {block_id} was missing"),
                });
            }
        }
        OperationKind::MoveInlineToBlock {
            inline_id,
            target_block_id,
            after,
        } => match move_inline_to_block(document, &inline_id, &target_block_id, after) {
            InlineMoveResult::Applied => {}
            InlineMoveResult::AnchorDegraded => warnings.push(ModelWarning {
                code: "inline-anchor-degraded".to_string(),
                message: format!(
                    "inline {inline_id} moved to block {target_block_id} with a degraded anchor"
                ),
            }),
            InlineMoveResult::MissingInline => warnings.push(ModelWarning {
                code: "missing-inline".to_string(),
                message: format!("inline move source {inline_id} was missing"),
            }),
            InlineMoveResult::MissingBlock => warnings.push(ModelWarning {
                code: "missing-block".to_string(),
                message: format!("inline move target block {target_block_id} was missing"),
            }),
        },
        OperationKind::AddMark { text_id, mark } => {
            if !marks_valid_for_merge(std::slice::from_ref(&mark), warnings, "mark operation", &text_id)
            {
                return;
            }
            if !add_mark(document, &text_id, mark) {
                warnings.push(ModelWarning {
                    code: "missing-text".to_string(),
                    message: format!("mark target {text_id} was missing"),
                });
            }
        }
        OperationKind::RemoveMark {
            text_id,
            kind,
            value,
        } => {
            if !mark_removal_valid_for_merge(
                &kind,
                value.as_deref(),
                warnings,
                "mark removal operation",
                &text_id,
            ) {
                return;
            }
            if !remove_mark(document, &text_id, &kind, value.as_deref()) {
                warnings.push(ModelWarning {
                    code: "missing-text".to_string(),
                    message: format!("mark removal target {text_id} was missing"),
                });
            }
        }
        OperationKind::AddMarkRange { range, mark } => {
            if !marks_valid_for_merge(
                std::slice::from_ref(&mark),
                warnings,
                "mark range operation",
                &range.start,
            ) {
                return;
            }
            apply_mark_range(document, warnings, range, mark);
        }
        OperationKind::AddSuggestion { suggestion } => {
            if let Err(err) = suggestion.validate() {
                warnings.push(ModelWarning {
                    code: "invalid-suggestion".to_string(),
                    message: format!("suggestion {} was ignored: {err}", suggestion.id),
                });
            } else if suggestion_insert_content_is_empty(&suggestion) {
                warnings.push(ModelWarning {
                    code: "invalid-suggestion".to_string(),
                    message: format!("suggestion {} ignored empty insert content", suggestion.id),
                });
            } else if document
                .suggestions
                .iter()
                .any(|existing| existing.id == suggestion.id)
            {
                warnings.push(ModelWarning {
                    code: "duplicate-suggestion".to_string(),
                    message: format!("suggestion {} was already present", suggestion.id),
                });
            } else {
                document.suggestions.push(suggestion);
            }
        }
        OperationKind::UpdateSuggestionInsertContent {
            suggestion_id,
            content,
        } => {
            if content.is_empty()
                || inline_sequence_is_empty_source_text(&content)
                || !inline_sequence_payload_valid_for_merge(
                    &content,
                    warnings,
                    &format!("suggestion {suggestion_id} update"),
                )
            {
                if content.is_empty() || inline_sequence_is_empty_source_text(&content) {
                    warnings.push(ModelWarning {
                        code: "invalid-suggestion".to_string(),
                        message: format!("suggestion {suggestion_id} ignored empty insert content"),
                    });
                }
                return;
            }
            if let Some(suggestion) = document
                .suggestions
                .iter_mut()
                .find(|item| item.id == suggestion_id && item.state == SuggestionState::Proposed)
            {
                match &mut suggestion.kind {
                    SuggestionKind::Insert {
                        content: existing, ..
                    } => {
                        *existing = content;
                    }
                    _ => warnings.push(ModelWarning {
                        code: "non-editable-suggestion".to_string(),
                        message: format!("suggestion {suggestion_id} is not an insert suggestion"),
                    }),
                }
            } else {
                warnings.push(ModelWarning {
                    code: "missing-suggestion".to_string(),
                    message: format!("suggestion {suggestion_id} was missing"),
                });
            }
        }
        OperationKind::AddCommentThread { thread } => {
            if let Err(err) = thread.validate() {
                warnings.push(ModelWarning {
                    code: "invalid-comment-thread".to_string(),
                    message: format!("comment thread {} was ignored: {err}", thread.id),
                });
            } else if comment_thread_has_empty_body(&thread) {
                warnings.push(ModelWarning {
                    code: "invalid-comment-thread".to_string(),
                    message: format!("comment thread {} was ignored: empty comment body", thread.id),
                });
            } else if document
                .comments
                .iter()
                .any(|existing| existing.id == thread.id)
            {
                warnings.push(ModelWarning {
                    code: "duplicate-comment-thread".to_string(),
                    message: format!("comment thread {} was already present", thread.id),
                });
            } else if anchor_resolves(document, &thread.anchor) {
                document.comments.push(thread);
            } else {
                let mut thread = thread;
                repair_comment_anchor(
                    &document.blocks,
                    &mut thread.anchor,
                    "comment anchor could not be resolved",
                );
                document.comments.push(thread);
                warnings.push(ModelWarning {
                    code: "comment-anchor-degraded".to_string(),
                    message: "comment anchor moved to nearest surviving anchor".to_string(),
                });
            }
        }
        OperationKind::AddCommentReply { thread_id, comment } => match document
            .comments
            .iter_mut()
            .find(|thread| thread.id == thread_id && !thread.deleted)
        {
            Some(thread) => {
                if let Err(err) = comment.validate() {
                    warnings.push(ModelWarning {
                        code: "invalid-comment-reply".to_string(),
                        message: format!("comment reply {} was ignored: {err}", comment.id),
                    });
                } else if inline_sequence_is_empty_source_text(&comment.body)
                    || !inline_sequence_payload_valid_for_merge(
                        &comment.body,
                        warnings,
                        &format!("comment reply {}", comment.id),
                    )
                {
                    warnings.push(ModelWarning {
                        code: "invalid-comment-reply".to_string(),
                        message: format!("comment reply {} ignored empty body", comment.id),
                    });
                } else if thread
                    .comments
                    .iter()
                    .any(|existing| existing.id == comment.id)
                {
                    warnings.push(ModelWarning {
                        code: "duplicate-comment-reply".to_string(),
                        message: format!("comment reply {} was already present", comment.id),
                    });
                } else {
                    thread.comments.push(comment);
                    thread.comments.sort_by(|left, right| {
                        left.created_at_ms
                            .cmp(&right.created_at_ms)
                            .then_with(|| left.id.cmp(&right.id))
                    });
                }
            }
            None => warnings.push(ModelWarning {
                code: "missing-comment-thread".to_string(),
                message: format!("comment thread {thread_id} was missing"),
            }),
        },
        OperationKind::UpsertFootnote { footnote } => {
            if let Err(err) = footnote.validate() {
                warnings.push(ModelWarning {
                    code: "invalid-footnote".to_string(),
                    message: format!("footnote {} was ignored: {err}", footnote.id),
                });
                return;
            }
            if let Some(existing) = document
                .footnotes
                .iter_mut()
                .find(|item| item.id == footnote.id)
            {
                if footnote.revision >= existing.revision {
                    *existing = footnote;
                }
            } else {
                document.footnotes.push(footnote);
                document
                    .footnotes
                    .sort_by(|left, right| left.id.cmp(&right.id));
            }
        }
        OperationKind::UpsertBibliographyReference { reference } => {
            if let Err(err) = reference.validate() {
                warnings.push(ModelWarning {
                    code: "invalid-bibliography-reference".to_string(),
                    message: format!("bibliography reference {} was ignored: {err}", reference.id),
                });
                return;
            }
            let reference_id = reference.id.clone();
            document.citation_database.upsert_reference(reference);
            invalidate_citation_caches_for_reference(document, &reference_id);
        }
        OperationKind::DeleteBibliographyReference {
            reference_id,
            revision,
        } => {
            if !document
                .citation_database
                .delete_reference(&reference_id, revision)
            {
                warnings.push(ModelWarning {
                    code: "missing-bibliography-reference".to_string(),
                    message: format!("bibliography reference {reference_id} was missing"),
                });
            } else {
                invalidate_citation_caches_for_reference(document, &reference_id);
            }
        }
        OperationKind::UpsertCitationGroup { citation } => {
            if let Err(err) = citation.validate_payload() {
                warnings.push(ModelWarning {
                    code: "invalid-citation-group".to_string(),
                    message: format!("citation group {} was ignored: {err}", citation.id),
                });
            } else {
                let citation_id = citation.id.clone();
                let mut citation = citation;
                citation.rendered_cache = None;
                document.citation_database.upsert_citation(citation);
                invalidate_inline_citation_caches(&mut document.blocks, &citation_id);
            }
        }
        OperationKind::DeleteCitationGroup {
            citation_id,
            revision,
        } => {
            if !document
                .citation_database
                .delete_citation(&citation_id, revision)
            {
                warnings.push(ModelWarning {
                    code: "missing-citation-group".to_string(),
                    message: format!("citation group {citation_id} was missing"),
                });
            }
        }
        OperationKind::UpdateCitationStyle { style, locale } => {
            let style = style.trim().to_string();
            let locale = locale.trim().to_string();
            if style.is_empty() || locale.is_empty() {
                warnings.push(ModelWarning {
                    code: "invalid-citation-style".to_string(),
                    message: "citation style update was ignored because style or locale is empty"
                        .to_string(),
                });
            } else {
                document.citation_database.style = style;
                document.citation_database.locale = locale;
                invalidate_all_citation_caches(document);
            }
        }
        OperationKind::DeleteCommentThread { thread_id } => {
            if let Some(thread) = document
                .comments
                .iter_mut()
                .find(|thread| thread.id == thread_id)
            {
                thread.deleted = true;
            } else {
                warnings.push(ModelWarning {
                    code: "missing-comment-thread".to_string(),
                    message: format!("comment thread {thread_id} was missing"),
                });
            }
        }
        OperationKind::RestoreCommentThread { thread_id } => {
            if let Some(thread) = document
                .comments
                .iter_mut()
                .find(|thread| thread.id == thread_id && thread.deleted)
            {
                thread.deleted = false;
                for comment in &mut thread.comments {
                    comment.deleted = false;
                }
            } else {
                warnings.push(ModelWarning {
                    code: "missing-deleted-comment-thread".to_string(),
                    message: format!("deleted comment thread {thread_id} was missing"),
                });
            }
        }
        OperationKind::DeleteComment {
            thread_id,
            comment_id,
        } => match document
            .comments
            .iter_mut()
            .find(|thread| thread.id == thread_id)
        {
            Some(thread) => match thread
                .comments
                .iter_mut()
                .find(|comment| comment.id == comment_id)
            {
                Some(comment) => {
                    comment.deleted = true;
                    if thread.comments.iter().all(|comment| comment.deleted) {
                        thread.deleted = true;
                    }
                }
                None => warnings.push(ModelWarning {
                    code: "missing-comment".to_string(),
                    message: format!("comment {comment_id} was missing"),
                }),
            },
            None => warnings.push(ModelWarning {
                code: "missing-comment-thread".to_string(),
                message: format!("comment thread {thread_id} was missing"),
            }),
        },
        OperationKind::RestoreComment {
            thread_id,
            comment_id,
        } => match document
            .comments
            .iter_mut()
            .find(|thread| thread.id == thread_id)
        {
            Some(thread) => match thread
                .comments
                .iter_mut()
                .find(|comment| comment.id == comment_id && comment.deleted)
            {
                Some(comment) => {
                    comment.deleted = false;
                    thread.deleted = false;
                }
                None => warnings.push(ModelWarning {
                    code: "missing-deleted-comment".to_string(),
                    message: format!("deleted comment {comment_id} was missing"),
                }),
            },
            None => warnings.push(ModelWarning {
                code: "missing-comment-thread".to_string(),
                message: format!("comment thread {thread_id} was missing"),
            }),
        },
        OperationKind::UpdateCommentBody {
            thread_id,
            comment_id,
            body,
        } => match document
            .comments
            .iter_mut()
            .find(|thread| thread.id == thread_id && !thread.deleted)
        {
            Some(thread) => match thread
                .comments
                .iter_mut()
                .find(|comment| comment.id == comment_id && !comment.deleted)
            {
                Some(comment) => {
                    if body.is_empty()
                        || inline_sequence_is_empty_source_text(&body)
                        || !inline_sequence_payload_valid_for_merge(
                            &body,
                            warnings,
                            &format!("comment {comment_id} update"),
                        )
                    {
                        if body.is_empty() || inline_sequence_is_empty_source_text(&body) {
                            warnings.push(ModelWarning {
                                code: "invalid-comment".to_string(),
                                message: format!("comment {comment_id} ignored empty body"),
                            });
                        }
                        return;
                    }
                    comment.body = body;
                }
                None => warnings.push(ModelWarning {
                    code: "missing-comment".to_string(),
                    message: format!("comment {comment_id} was missing"),
                }),
            },
            None => warnings.push(ModelWarning {
                code: "missing-comment-thread".to_string(),
                message: format!("comment thread {thread_id} was missing"),
            }),
        },
        OperationKind::UpdateInlineText { inline_id, text } => {
            match update_inline_text(document, &inline_id, &text) {
                Some(true) => {}
                Some(false) => warnings.push(ModelWarning {
                    code: "non-editable-inline".to_string(),
                    message: format!("inline {inline_id} is derived from structured state"),
                }),
                None => warnings.push(ModelWarning {
                    code: "missing-inline".to_string(),
                    message: format!("inline {inline_id} was missing"),
                }),
            }
        }
        OperationKind::InsertText {
            inline_id,
            offset,
            text,
        } => {
            if text.is_empty() {
                return;
            }
            match edit_inline_text(document, &inline_id, |value| {
                let at = byte_index_for_char_offset(value, offset);
                value.insert_str(at, &text);
            }) {
                Some(true) => {}
                Some(false) => warnings.push(ModelWarning {
                    code: "non-editable-inline".to_string(),
                    message: format!("inline {inline_id} is derived from structured state"),
                }),
                None => warnings.push(ModelWarning {
                    code: "missing-inline".to_string(),
                    message: format!("inline {inline_id} was missing"),
                }),
            }
        }
        OperationKind::DeleteText {
            inline_id,
            start,
            end,
        } => {
            if end <= start {
                return;
            }
            match edit_inline_text(document, &inline_id, |value| {
                let from = byte_index_for_char_offset(value, start);
                let to = byte_index_for_char_offset(value, end);
                if to > from {
                    value.replace_range(from..to, "");
                }
            }) {
                Some(true) => {}
                Some(false) => warnings.push(ModelWarning {
                    code: "non-editable-inline".to_string(),
                    message: format!("inline {inline_id} is derived from structured state"),
                }),
                None => warnings.push(ModelWarning {
                    code: "missing-inline".to_string(),
                    message: format!("inline {inline_id} was missing"),
                }),
            }
        }
        OperationKind::UpdateInlineEquationSource { inline_id, source } => {
            let source = source.trim();
            if source.is_empty() {
                warnings.push(ModelWarning {
                    code: "invalid-inline-equation-source".to_string(),
                    message: format!("inline equation {inline_id} ignored empty source"),
                });
                return;
            }
            match update_inline_equation_source(document, &inline_id, source) {
                Some(true) => {}
                Some(false) => warnings.push(ModelWarning {
                    code: "non-equation-inline".to_string(),
                    message: format!("inline {inline_id} is not an equation"),
                }),
                None => warnings.push(ModelWarning {
                    code: "missing-inline".to_string(),
                    message: format!("equation inline {inline_id} was missing"),
                }),
            }
        }
        OperationKind::UpdateMentionLabel { inline_id, label } => {
            let label = label.trim();
            if label.is_empty() {
                warnings.push(ModelWarning {
                    code: "invalid-mention-label".to_string(),
                    message: format!("mention {inline_id} ignored empty label"),
                });
                return;
            }
            match update_mention_label(document, &inline_id, label) {
                Some(true) => {}
                Some(false) => warnings.push(ModelWarning {
                    code: "non-mention-inline".to_string(),
                    message: format!("inline {inline_id} is not a mention"),
                }),
                None => warnings.push(ModelWarning {
                    code: "missing-inline".to_string(),
                    message: format!("inline {inline_id} was missing"),
                }),
            }
        }
        OperationKind::UpdateLinkHref { inline_id, href } => {
            let href = href.trim();
            if href.is_empty() {
                warnings.push(ModelWarning {
                    code: "invalid-link-href".to_string(),
                    message: format!("link inline {inline_id} ignored empty href"),
                });
                return;
            }
            match update_link_href(document, &inline_id, href) {
                Some(true) => {}
                Some(false) => warnings.push(ModelWarning {
                    code: "non-link-inline".to_string(),
                    message: format!("inline {inline_id} is not a link"),
                }),
                None => warnings.push(ModelWarning {
                    code: "missing-inline".to_string(),
                    message: format!("link inline {inline_id} was missing"),
                }),
            }
        }
        OperationKind::UpdateBlockEquationSource { block_id, source } => {
            let source = source.trim();
            if source.is_empty() {
                warnings.push(ModelWarning {
                    code: "invalid-block-equation-source".to_string(),
                    message: format!("block equation {block_id} ignored empty source"),
                });
                return;
            }
            match update_block_equation_source(&mut document.blocks, &block_id, source) {
                Some(true) => {}
                Some(false) => warnings.push(ModelWarning {
                    code: "non-equation-block".to_string(),
                    message: format!("block {block_id} is not a block equation"),
                }),
                None => warnings.push(ModelWarning {
                    code: "missing-block".to_string(),
                    message: format!("block equation target {block_id} was missing"),
                }),
            }
        }
        OperationKind::UpdateImageAltText { block_id, alt_text } => {
            match update_image_alt_text(&mut document.blocks, &block_id, &alt_text) {
                Some(true) => {}
                Some(false) => warnings.push(ModelWarning {
                    code: "non-image-block".to_string(),
                    message: format!("block {block_id} is not an image"),
                }),
                None => warnings.push(ModelWarning {
                    code: "missing-block".to_string(),
                    message: format!("image target {block_id} was missing"),
                }),
            }
        }
        OperationKind::UpdateImageBlobHash {
            block_id,
            blob_hash,
        } => {
            let blob_hash = match opendoc_core::HashRef::parse(&blob_hash) {
                Ok(hash) => hash.to_string(),
                Err(_) => {
                let diagnostic_hash = canonical_diagnostic_value(&blob_hash);
                warnings.push(ModelWarning {
                    code: "invalid-image-blob-hash".to_string(),
                    message: format!(
                        "image target {block_id} ignored invalid blob hash {diagnostic_hash}"
                    ),
                });
                return;
                }
            };
            match update_image_blob_hash(&mut document.blocks, &block_id, &blob_hash) {
                Some(true) => {}
                Some(false) => warnings.push(ModelWarning {
                    code: "non-image-block".to_string(),
                    message: format!("block {block_id} is not an image"),
                }),
                None => warnings.push(ModelWarning {
                    code: "missing-block".to_string(),
                    message: format!("image target {block_id} was missing"),
                }),
            }
        }
        OperationKind::UpdateHeadingLevel { block_id, level } => {
            if !(1..=6).contains(&level) {
                warnings.push(ModelWarning {
                    code: "invalid-heading-level".to_string(),
                    message: format!("heading block {block_id} ignored invalid level {level}"),
                });
                return;
            }
            match update_heading_level(&mut document.blocks, &block_id, level) {
                Some(true) => {}
                Some(false) => warnings.push(ModelWarning {
                    code: "non-heading-block".to_string(),
                    message: format!("block {block_id} is not a heading"),
                }),
                None => warnings.push(ModelWarning {
                    code: "missing-block".to_string(),
                    message: format!("heading target {block_id} was missing"),
                }),
            }
        }
        OperationKind::UpdateListItem {
            block_id,
            level,
            ordered,
        } => {
            if level > 8 {
                warnings.push(ModelWarning {
                    code: "invalid-list-level".to_string(),
                    message: format!("list item block {block_id} ignored invalid level {level}"),
                });
                return;
            }
            match update_list_item(&mut document.blocks, &block_id, level, ordered) {
                Some(true) => {}
                Some(false) => warnings.push(ModelWarning {
                    code: "non-list-item-block".to_string(),
                    message: format!("block {block_id} is not a list item"),
                }),
                None => warnings.push(ModelWarning {
                    code: "missing-block".to_string(),
                    message: format!("list item target {block_id} was missing"),
                }),
            }
        }
        OperationKind::AcceptSuggestion {
            suggestion_id,
            accepted_by,
        } => {
            accept_suggestion(document, warnings, &suggestion_id, &accepted_by);
        }
        OperationKind::RejectSuggestion {
            suggestion_id,
            rejected_by,
        } => {
            let rejected_by =
                reviewer_provenance_value(warnings, &suggestion_id, "reject", &rejected_by);
            if let Some(suggestion) = document
                .suggestions
                .iter_mut()
                .find(|item| item.id == suggestion_id)
            {
                if suggestion.state != SuggestionState::Proposed {
                    warnings.push(ModelWarning {
                        code: "resolved-suggestion".to_string(),
                        message: format!("suggestion {suggestion_id} was already resolved"),
                    });
                    return;
                }
                suggestion.state = SuggestionState::Rejected;
                suggestion
                    .provenance
                    .push(format!("rejected-by:{rejected_by}"));
            } else {
                warnings.push(ModelWarning {
                    code: "missing-suggestion".to_string(),
                    message: format!("suggestion {suggestion_id} was missing"),
                });
            }
        }
        OperationKind::DeleteInline { inline_id } => {
            if !delete_inline(document, &inline_id) {
                warnings.push(ModelWarning {
                    code: "missing-inline".to_string(),
                    message: format!("inline {inline_id} was already absent"),
                });
            }
        }
        OperationKind::InsertTableRow {
            table_block_id,
            after_row,
            row,
        } => {
            if !table_row_payload_valid_for_merge(&row, warnings) {
                return;
            }
            match insert_table_row(document, &table_block_id, after_row, row) {
                TableEditResult::Applied => {}
                TableEditResult::AnchorDegraded => warnings.push(ModelWarning {
                    code: "table-row-anchor-degraded".to_string(),
                    message: format!(
                        "table row insert anchor was missing in block {table_block_id}; row was appended"
                    ),
                }),
                TableEditResult::Duplicate => warnings.push(ModelWarning {
                    code: "duplicate-table-row".to_string(),
                    message: "table row was already present".to_string(),
                }),
                TableEditResult::NonTableBlock => warnings.push(ModelWarning {
                    code: "non-table-block".to_string(),
                    message: format!("block {table_block_id} is not a table"),
                }),
                TableEditResult::MissingTable => warnings.push(ModelWarning {
                    code: "missing-block".to_string(),
                    message: format!("table row insert target block {table_block_id} was missing"),
                }),
                TableEditResult::MissingRow | TableEditResult::MissingCell => {}
            }
        }
        OperationKind::DeleteTableRow {
            table_block_id,
            row_id,
        } => match delete_table_row(document, &table_block_id, &row_id) {
            TableEditResult::Applied => {}
            TableEditResult::AnchorDegraded => warnings.push(ModelWarning {
                code: "table-row-delete-degraded".to_string(),
                message: format!(
                    "table block {table_block_id} kept an empty placeholder row after deleting its last row"
                ),
            }),
            TableEditResult::MissingRow => warnings.push(ModelWarning {
                code: "missing-table-row".to_string(),
                message: format!("table row {row_id} was missing"),
            }),
            TableEditResult::MissingTable => warnings.push(ModelWarning {
                code: "missing-block".to_string(),
                message: format!("table row delete target block {table_block_id} was missing"),
            }),
            TableEditResult::NonTableBlock => warnings.push(ModelWarning {
                code: "non-table-block".to_string(),
                message: format!("block {table_block_id} is not a table"),
            }),
            TableEditResult::Duplicate | TableEditResult::MissingCell => {}
        },
        OperationKind::InsertTableCell {
            table_block_id,
            row_id,
            after_cell,
            cell,
        } => {
            if !table_cell_payload_valid_for_merge(&cell, warnings) {
                return;
            }
            match insert_table_cell(document, &table_block_id, &row_id, after_cell, cell) {
                TableEditResult::Applied => {}
                TableEditResult::AnchorDegraded => warnings.push(ModelWarning {
                    code: "table-cell-anchor-degraded".to_string(),
                    message: format!(
                        "table cell insert anchor was missing in row {row_id}; cell was appended"
                    ),
                }),
                TableEditResult::Duplicate => warnings.push(ModelWarning {
                    code: "duplicate-table-cell".to_string(),
                    message: "table cell was already present".to_string(),
                }),
                TableEditResult::MissingTable => warnings.push(ModelWarning {
                    code: "missing-block".to_string(),
                    message: format!(
                        "table cell insert target block {table_block_id} was missing"
                    ),
                }),
                TableEditResult::NonTableBlock => warnings.push(ModelWarning {
                    code: "non-table-block".to_string(),
                    message: format!("block {table_block_id} is not a table"),
                }),
                TableEditResult::MissingRow => warnings.push(ModelWarning {
                    code: "missing-table-row".to_string(),
                    message: format!("table cell insert target row {row_id} was missing"),
                }),
                TableEditResult::MissingCell => warnings.push(ModelWarning {
                    code: "missing-table-cell".to_string(),
                    message: "table cell insert target was missing".to_string(),
                }),
            }
        }
        OperationKind::DeleteTableCell {
            table_block_id,
            row_id,
            cell_id,
        } => match delete_table_cell(document, &table_block_id, &row_id, &cell_id) {
            TableEditResult::Applied => {}
            TableEditResult::AnchorDegraded => warnings.push(ModelWarning {
                code: "table-cell-delete-degraded".to_string(),
                message: format!(
                    "table row {row_id} kept an empty placeholder cell after deleting its last cell"
                ),
            }),
            TableEditResult::Duplicate => {}
            TableEditResult::MissingTable => warnings.push(ModelWarning {
                code: "missing-block".to_string(),
                message: format!("table cell delete target block {table_block_id} was missing"),
            }),
            TableEditResult::NonTableBlock => warnings.push(ModelWarning {
                code: "non-table-block".to_string(),
                message: format!("block {table_block_id} is not a table"),
            }),
            TableEditResult::MissingRow => warnings.push(ModelWarning {
                code: "missing-table-row".to_string(),
                message: format!("table cell delete target row {row_id} was missing"),
            }),
            TableEditResult::MissingCell => warnings.push(ModelWarning {
                code: "missing-table-cell".to_string(),
                message: format!("table cell {cell_id} was missing"),
            }),
        },
    }
}

fn apply_mark_range(
    document: &mut Document,
    warnings: &mut Vec<ModelWarning>,
    range: opendoc_core::TextRange,
    mark: Mark,
) {
    match add_mark_range(document, &range, mark) {
        MarkRangeResult::Applied => {}
        MarkRangeResult::Degraded => warnings.push(ModelWarning {
            code: "mark-range-degraded".to_string(),
            message: "mark range applied to surviving range endpoints".to_string(),
        }),
        MarkRangeResult::Missing => warnings.push(ModelWarning {
            code: "missing-text-range".to_string(),
            message: format!("mark range {}..{} was missing", range.start, range.end),
        }),
    }
}

fn repair_comment_anchors(document: &mut Document, warnings: &mut Vec<ModelWarning>) {
    let mut degraded = Vec::new();
    for thread in &mut document.comments {
        if thread.deleted || anchor_resolves_in_blocks(&document.blocks, &thread.anchor) {
            continue;
        }
        repair_comment_anchor(
            &document.blocks,
            &mut thread.anchor,
            "comment anchor text was deleted",
        );
        degraded.push(thread.id.clone());
    }
    for thread_id in degraded {
        warnings.push(ModelWarning {
            code: "comment-anchor-degraded".to_string(),
            message: format!("comment thread {thread_id} moved to nearest surviving anchor"),
        });
    }
}

fn repair_comment_anchor(blocks: &[Block], anchor: &mut Anchor, warning: &str) {
    match anchor {
        Anchor::TextRange(range) => {
            let ids = editable_inline_ids(blocks);
            let start_survives = ids.iter().any(|id| id == &range.start);
            let end_survives = ids.iter().any(|id| id == &range.end);
            match (start_survives, end_survives) {
                (true, false) => range.end = range.start.clone(),
                (false, true) => range.start = range.end.clone(),
                (false, false) => *anchor = nearest_block_anchor_in_blocks(blocks, warning),
                (true, true) => {}
            }
        }
        Anchor::NearestBlock { .. } if !anchor_resolves_in_blocks(blocks, anchor) => {
            *anchor = nearest_block_anchor_in_blocks(blocks, warning);
        }
        _ => {}
    }
}

fn repair_suggestion_anchors(document: &mut Document, warnings: &mut Vec<ModelWarning>) {
    let replacement = nearest_block_anchor(document, "suggestion anchor text was deleted");
    let editable_ids = editable_inline_ids(&document.blocks);
    let mut emitted_warnings = Vec::new();

    for suggestion in &mut document.suggestions {
        if suggestion.state != SuggestionState::Proposed {
            continue;
        }

        match &mut suggestion.kind {
            SuggestionKind::Insert { anchor, .. } => {
                if anchor_resolves_in_blocks(&document.blocks, anchor) {
                    continue;
                }
                *anchor = replacement.clone();
                push_provenance_once(suggestion, "auto-degraded:missing-anchor");
                emitted_warnings.push(ModelWarning {
                    code: "suggestion-anchor-degraded".to_string(),
                    message: format!(
                        "suggestion {} moved to nearest surviving block",
                        suggestion.id
                    ),
                });
            }
            SuggestionKind::Delete { range } | SuggestionKind::Format { range, .. } => {
                match repair_text_range(&editable_ids, range) {
                    TextRangeRepair::Unchanged => {}
                    TextRangeRepair::Collapsed => {
                        push_provenance_once(suggestion, "auto-degraded:partial-range");
                        emitted_warnings.push(ModelWarning {
                            code: "suggestion-range-degraded".to_string(),
                            message: format!(
                                "suggestion {} collapsed to surviving range endpoint",
                                suggestion.id
                            ),
                        });
                    }
                    TextRangeRepair::Missing => {
                        suggestion.state = SuggestionState::Rejected;
                        push_provenance_once(suggestion, "auto-rejected:missing-range");
                        emitted_warnings.push(ModelWarning {
                            code: "suggestion-range-missing".to_string(),
                            message: format!(
                                "suggestion {} was rejected because its range was deleted",
                                suggestion.id
                            ),
                        });
                    }
                }
            }
        }
    }

    warnings.extend(emitted_warnings);
}

fn repair_unreferenced_footnotes(document: &mut Document, warnings: &mut Vec<ModelWarning>) {
    let mut referenced = footnote_reference_ids(&document.blocks);
    referenced.extend(citation_footnote_reference_ids(document));
    for footnote in &mut document.footnotes {
        if footnote.deleted || referenced.contains(&footnote.id) {
            continue;
        }
        footnote.deleted = true;
        footnote.revision = footnote.revision.saturating_add(1);
        warnings.push(ModelWarning {
            code: "footnote-reference-missing".to_string(),
            message: format!("footnote {} has no surviving reference", footnote.id),
        });
    }
}

fn repair_missing_footnote_references(document: &mut Document, warnings: &mut Vec<ModelWarning>) {
    let live_footnotes = document
        .footnotes
        .iter()
        .filter(|footnote| !footnote.deleted)
        .map(|footnote| footnote.id.clone())
        .collect::<BTreeSet<_>>();
    let mut removed = BTreeSet::new();
    remove_missing_footnote_references_from_blocks(
        &mut document.blocks,
        &live_footnotes,
        &mut removed,
    );
    for footnote_id in removed {
        warnings.push(ModelWarning {
            code: "footnote-reference-target-missing".to_string(),
            message: format!(
                "footnote reference to {footnote_id} was removed because its target was missing"
            ),
        });
    }
}

fn remove_missing_footnote_references_from_blocks(
    blocks: &mut [Block],
    live_footnotes: &BTreeSet<StableId>,
    removed: &mut BTreeSet<StableId>,
) {
    for block in blocks {
        block.content.retain(|inline| {
            if let Inline::FootnoteRef { footnote_id, .. } = inline {
                if !live_footnotes.contains(footnote_id) {
                    removed.insert(footnote_id.clone());
                    return false;
                }
            }
            true
        });
        if let BlockKind::Table { rows } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    remove_missing_footnote_references_from_blocks(
                        &mut cell.blocks,
                        live_footnotes,
                        removed,
                    );
                }
            }
        }
    }
}

fn citation_footnote_reference_ids(document: &Document) -> BTreeSet<StableId> {
    document
        .citation_database
        .citations
        .iter()
        .filter(|citation| !citation.deleted)
        .filter_map(|citation| match &citation.placement {
            CitationPlacement::Footnote { footnote_id } => Some(footnote_id.clone()),
            CitationPlacement::Inline => None,
        })
        .collect()
}

fn repair_citation_placements(document: &mut Document, warnings: &mut Vec<ModelWarning>) {
    let live_footnotes = document
        .footnotes
        .iter()
        .filter(|footnote| !footnote.deleted)
        .map(|footnote| footnote.id.clone())
        .collect::<BTreeSet<_>>();
    for citation in &mut document.citation_database.citations {
        if citation.deleted {
            continue;
        }
        let CitationPlacement::Footnote { footnote_id } = &citation.placement else {
            continue;
        };
        if live_footnotes.contains(footnote_id) {
            continue;
        }
        let missing_footnote_id = footnote_id.clone();
        citation.placement = CitationPlacement::Inline;
        citation.rendered_cache = None;
        warnings.push(ModelWarning {
            code: "citation-footnote-target-missing".to_string(),
            message: format!(
                "citation group {} moved inline because footnote {missing_footnote_id} was missing",
                citation.id
            ),
        });
    }
}

fn repair_citation_references(document: &mut Document, warnings: &mut Vec<ModelWarning>) {
    let live_references = document
        .citation_database
        .references
        .iter()
        .filter(|reference| !reference.deleted)
        .map(|reference| reference.id.clone())
        .collect::<BTreeSet<_>>();
    let mut affected_citations = Vec::new();
    for citation in &mut document.citation_database.citations {
        if citation.deleted {
            continue;
        }
        let missing = citation
            .items
            .iter()
            .any(|item| !live_references.contains(&item.reference_id));
        if !missing {
            continue;
        }
        citation.rendered_cache = None;
        affected_citations.push(citation.id.clone());
    }
    affected_citations.sort();
    affected_citations.dedup();
    for citation_id in affected_citations {
        invalidate_inline_citation_caches(&mut document.blocks, &citation_id);
        warnings.push(ModelWarning {
            code: "citation-reference-missing".to_string(),
            message: format!(
                "citation group {citation_id} references a missing bibliography record"
            ),
        });
    }
}

fn repair_inline_citation_labels(document: &mut Document, warnings: &mut Vec<ModelWarning>) {
    let live_citations = document
        .citation_database
        .citations
        .iter()
        .filter(|citation| !citation.deleted)
        .map(|citation| citation.id.clone())
        .collect::<BTreeSet<_>>();
    let mut affected = BTreeSet::new();
    clear_missing_inline_citation_caches(&mut document.blocks, &live_citations, &mut affected);
    for citation_id in affected {
        warnings.push(ModelWarning {
            code: "citation-group-missing".to_string(),
            message: format!(
                "inline citation label {citation_id} references a missing citation group"
            ),
        });
    }
}

fn refresh_citation_projection_caches(document: &mut Document) {
    let live_references = document
        .citation_database
        .references
        .iter()
        .filter(|reference| !reference.deleted)
        .map(|reference| reference.id.clone())
        .collect::<BTreeSet<_>>();
    let database = document.citation_database.clone();
    for citation in &mut document.citation_database.citations {
        if citation.deleted
            || citation
                .items
                .iter()
                .any(|item| !live_references.contains(&item.reference_id))
        {
            citation.rendered_cache = None;
            continue;
        }
        citation.rendered_cache = Some(opendoc_citations::render_citation_group(
            &database, citation,
        ));
    }
}

fn clear_missing_inline_citation_caches(
    blocks: &mut [Block],
    live_citations: &BTreeSet<StableId>,
    affected: &mut BTreeSet<StableId>,
) {
    for block in blocks {
        for inline in &mut block.content {
            if let Inline::Citation {
                citation_id,
                rendered_cache,
                ..
            } = inline
            {
                if !live_citations.contains(citation_id) {
                    *rendered_cache = None;
                    affected.insert(citation_id.clone());
                }
            }
        }
        if let BlockKind::Table { rows } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    clear_missing_inline_citation_caches(
                        &mut cell.blocks,
                        live_citations,
                        affected,
                    );
                }
            }
        }
    }
}

fn invalidate_citation_caches_for_reference(document: &mut Document, reference_id: &StableId) {
    let mut affected_citations = BTreeSet::new();
    for citation in &mut document.citation_database.citations {
        if citation.deleted {
            continue;
        }
        if citation
            .items
            .iter()
            .any(|item| &item.reference_id == reference_id)
        {
            citation.rendered_cache = None;
            affected_citations.insert(citation.id.clone());
        }
    }
    for citation_id in affected_citations {
        invalidate_inline_citation_caches(&mut document.blocks, &citation_id);
    }
}

fn invalidate_all_citation_caches(document: &mut Document) {
    for citation in &mut document.citation_database.citations {
        citation.rendered_cache = None;
    }
    invalidate_all_inline_citation_caches(&mut document.blocks);
}

fn invalidate_inline_citation_caches(blocks: &mut [Block], citation_id: &StableId) {
    for block in blocks {
        for inline in &mut block.content {
            if let Inline::Citation {
                citation_id: inline_citation_id,
                rendered_cache,
                ..
            } = inline
            {
                if inline_citation_id == citation_id {
                    *rendered_cache = None;
                }
            }
        }
        if let BlockKind::Table { rows } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    invalidate_inline_citation_caches(&mut cell.blocks, citation_id);
                }
            }
        }
    }
}

fn invalidate_all_inline_citation_caches(blocks: &mut [Block]) {
    for block in blocks {
        for inline in &mut block.content {
            if let Inline::Citation { rendered_cache, .. } = inline {
                *rendered_cache = None;
            }
        }
        if let BlockKind::Table { rows } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    invalidate_all_inline_citation_caches(&mut cell.blocks);
                }
            }
        }
    }
}

fn footnote_reference_ids(blocks: &[Block]) -> BTreeSet<StableId> {
    let mut ids = BTreeSet::new();
    collect_footnote_reference_ids(blocks, &mut ids);
    ids
}

fn collect_footnote_reference_ids(blocks: &[Block], ids: &mut BTreeSet<StableId>) {
    for block in blocks {
        for inline in &block.content {
            if let Inline::FootnoteRef { footnote_id, .. } = inline {
                ids.insert(footnote_id.clone());
            }
        }
        if let BlockKind::Table { rows } = &block.kind {
            for row in rows {
                for cell in &row.cells {
                    collect_footnote_reference_ids(&cell.blocks, ids);
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TextRangeRepair {
    Unchanged,
    Collapsed,
    Missing,
}

fn repair_text_range(
    editable_ids: &[StableId],
    range: &mut opendoc_core::TextRange,
) -> TextRangeRepair {
    let start_exists = editable_ids.iter().any(|id| id == &range.start);
    let end_exists = editable_ids.iter().any(|id| id == &range.end);
    match (start_exists, end_exists) {
        (true, true) => TextRangeRepair::Unchanged,
        (true, false) => {
            range.end = range.start.clone();
            TextRangeRepair::Collapsed
        }
        (false, true) => {
            range.start = range.end.clone();
            TextRangeRepair::Collapsed
        }
        (false, false) => TextRangeRepair::Missing,
    }
}

fn push_provenance_once(suggestion: &mut Suggestion, value: &str) {
    if !suggestion.provenance.iter().any(|item| item == value) {
        suggestion.provenance.push(value.to_string());
    }
}

fn reviewer_provenance_value(
    warnings: &mut Vec<ModelWarning>,
    suggestion_id: &StableId,
    action: &str,
    reviewer: &str,
) -> String {
    let reviewer = reviewer.trim();
    if reviewer.is_empty() {
        warnings.push(ModelWarning {
            code: "invalid-suggestion-reviewer".to_string(),
            message: format!(
                "suggestion {suggestion_id} {action} reviewer was empty; recorded unknown reviewer"
            ),
        });
        "unknown".to_string()
    } else {
        reviewer.to_string()
    }
}

fn canonical_diagnostic_value(value: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        "<empty>".to_string()
    } else {
        value.to_string()
    }
}

fn accept_suggestion(
    document: &mut Document,
    warnings: &mut Vec<ModelWarning>,
    suggestion_id: &StableId,
    accepted_by: &str,
) {
    let accepted_by = reviewer_provenance_value(warnings, suggestion_id, "accept", accepted_by);
    let Some(index) = document
        .suggestions
        .iter()
        .position(|item| &item.id == suggestion_id)
    else {
        warnings.push(ModelWarning {
            code: "missing-suggestion".to_string(),
            message: format!("suggestion {suggestion_id} was missing"),
        });
        return;
    };

    if document.suggestions[index].state != SuggestionState::Proposed {
        warnings.push(ModelWarning {
            code: "resolved-suggestion".to_string(),
            message: format!("suggestion {suggestion_id} was already resolved"),
        });
        return;
    }

    let kind = document.suggestions[index].kind.clone();
    let accepted = match kind {
        SuggestionKind::Insert { anchor, content } => {
            accept_insert_suggestion(document, warnings, suggestion_id, &anchor, content)
        }
        SuggestionKind::Delete { range } => {
            accept_delete_suggestion(document, warnings, suggestion_id, &range);
            true
        }
        SuggestionKind::Format { range, marks } => {
            accept_format_suggestion(document, warnings, suggestion_id, &range, marks)
        }
    };

    let suggestion = &mut document.suggestions[index];
    if accepted {
        suggestion.state = SuggestionState::Accepted;
        push_provenance_once(suggestion, &format!("accepted-by:{accepted_by}"));
    } else {
        suggestion.state = SuggestionState::Rejected;
        suggestion.kind = SuggestionKind::Insert {
            anchor: Anchor::Document,
            content: vec![Inline::text("[invalid suggestion payload]")],
        };
        push_provenance_once(suggestion, "auto-rejected:invalid-accept-payload");
    }
}

fn accept_insert_suggestion(
    document: &mut Document,
    warnings: &mut Vec<ModelWarning>,
    suggestion_id: &StableId,
    anchor: &Anchor,
    content: Vec<Inline>,
) -> bool {
    if content.is_empty() {
        warnings.push(ModelWarning {
            code: "invalid-suggestion".to_string(),
            message: format!("suggestion {suggestion_id} ignored empty insert content"),
        });
        return false;
    }
    if !inline_sequence_payload_valid_for_merge(
        &content,
        warnings,
        &format!("suggestion {suggestion_id} acceptance"),
    ) {
        return false;
    }
    match insert_inlines_after_anchor(document, anchor, content) {
        AnchorInsertResult::Applied => {}
        AnchorInsertResult::Degraded => warnings.push(ModelWarning {
            code: "suggestion-anchor-degraded".to_string(),
            message: format!("suggestion {suggestion_id} inserted at degraded anchor"),
        }),
        AnchorInsertResult::Missing => warnings.push(ModelWarning {
            code: "suggestion-anchor-degraded".to_string(),
            message: format!("suggestion {suggestion_id} could not find an insertion anchor"),
        }),
    }
    true
}

fn accept_delete_suggestion(
    document: &mut Document,
    warnings: &mut Vec<ModelWarning>,
    suggestion_id: &StableId,
    range: &opendoc_core::TextRange,
) {
    match delete_inline_range(document, range) {
        RangeEditResult::Applied => {}
        RangeEditResult::Degraded => warnings.push(ModelWarning {
            code: "suggestion-range-degraded".to_string(),
            message: format!("suggestion {suggestion_id} deleted surviving range endpoint"),
        }),
        RangeEditResult::Missing => warnings.push(ModelWarning {
            code: "suggestion-range-missing".to_string(),
            message: format!("suggestion {suggestion_id} had no surviving range to delete"),
        }),
    }
}

fn accept_format_suggestion(
    document: &mut Document,
    warnings: &mut Vec<ModelWarning>,
    suggestion_id: &StableId,
    range: &opendoc_core::TextRange,
    marks: Vec<Mark>,
) -> bool {
    if marks.is_empty() {
        warnings.push(ModelWarning {
            code: "invalid-suggestion".to_string(),
            message: format!("suggestion {suggestion_id} ignored empty format marks"),
        });
        return false;
    }
    for mark in marks {
        if !marks_valid_for_merge(
            std::slice::from_ref(&mark),
            warnings,
            &format!("suggestion {suggestion_id} acceptance"),
            &range.start,
        ) {
            return false;
        }
        match add_mark_range(document, range, mark) {
            MarkRangeResult::Applied => {}
            MarkRangeResult::Degraded => warnings.push(ModelWarning {
                code: "suggestion-range-degraded".to_string(),
                message: format!("suggestion {suggestion_id} formatted surviving range endpoint"),
            }),
            MarkRangeResult::Missing => warnings.push(ModelWarning {
                code: "suggestion-range-missing".to_string(),
                message: format!("suggestion {suggestion_id} had no surviving range to format"),
            }),
        }
    }
    true
}

fn insert_inlines_after_anchor(
    document: &mut Document,
    anchor: &Anchor,
    content: Vec<Inline>,
) -> AnchorInsertResult {
    match anchor {
        Anchor::TextRange(range) => {
            if insert_inlines_after_inline_id(document, &range.end, content.clone()) {
                AnchorInsertResult::Applied
            } else if insert_inlines_after_inline_id(document, &range.start, content) {
                AnchorInsertResult::Degraded
            } else {
                AnchorInsertResult::Missing
            }
        }
        Anchor::NearestBlock { block_id, .. } => {
            let Some(block) = find_block_mut(&mut document.blocks, block_id) else {
                return if insert_inlines_at_document_end(document, content) {
                    AnchorInsertResult::Degraded
                } else {
                    AnchorInsertResult::Missing
                };
            };
            append_inlines(&mut block.content, content);
            AnchorInsertResult::Applied
        }
        Anchor::Document => {
            if insert_inlines_at_document_end(document, content) {
                AnchorInsertResult::Applied
            } else {
                AnchorInsertResult::Missing
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AnchorInsertResult {
    Applied,
    Degraded,
    Missing,
}

fn insert_inlines_after_inline_id(
    document: &mut Document,
    target_inline_id: &StableId,
    content: Vec<Inline>,
) -> bool {
    let Some(target_content) =
        find_content_mut_containing_inline(&mut document.blocks, target_inline_id)
    else {
        return false;
    };
    let mut after = Some(target_inline_id.clone());
    for inline in content {
        let inserted_id = inline_id(&inline).clone();
        insert_inline(target_content, after, inline);
        after = Some(inserted_id);
    }
    true
}

fn insert_inlines_at_document_end(document: &mut Document, content: Vec<Inline>) -> bool {
    let Some(block) = document.blocks.last_mut() else {
        return false;
    };
    append_inlines(&mut block.content, content);
    true
}

fn append_inlines(target: &mut Vec<Inline>, content: Vec<Inline>) {
    let mut after = target.last().map(|inline| inline_id(inline).clone());
    for inline in content {
        let inserted_id = inline_id(&inline).clone();
        insert_inline(target, after, inline);
        after = Some(inserted_id);
    }
}

fn find_content_mut_containing_inline<'a>(
    blocks: &'a mut [Block],
    target_id: &StableId,
) -> Option<&'a mut Vec<Inline>> {
    for block in blocks {
        if block
            .content
            .iter()
            .any(|inline| inline_id(inline) == target_id)
        {
            return Some(&mut block.content);
        }
        if let BlockKind::Table { rows } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    if let Some(found) =
                        find_content_mut_containing_inline(&mut cell.blocks, target_id)
                    {
                        return Some(found);
                    }
                }
            }
        }
    }
    None
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RangeEditResult {
    Applied,
    Degraded,
    Missing,
}

fn delete_inline_range(
    document: &mut Document,
    range: &opendoc_core::TextRange,
) -> RangeEditResult {
    let ids = editable_inline_ids(&document.blocks);
    let start = ids.iter().position(|id| id == &range.start);
    let end = ids.iter().position(|id| id == &range.end);
    let targets = match (start, end) {
        (Some(start), Some(end)) => {
            let first = start.min(end);
            let last = start.max(end);
            ids[first..=last].to_vec()
        }
        (Some(start), None) => vec![ids[start].clone()],
        (None, Some(end)) => vec![ids[end].clone()],
        (None, None) => return RangeEditResult::Missing,
    };
    for id in &targets {
        delete_inline(document, id);
    }
    if start.is_some() && end.is_some() {
        RangeEditResult::Applied
    } else {
        RangeEditResult::Degraded
    }
}

fn anchor_resolves(document: &Document, anchor: &Anchor) -> bool {
    anchor_resolves_in_blocks(&document.blocks, anchor)
}

fn anchor_resolves_in_blocks(blocks: &[Block], anchor: &Anchor) -> bool {
    match anchor {
        Anchor::Document => true,
        Anchor::NearestBlock { block_id, .. } => block_exists(blocks, block_id),
        Anchor::TextRange(range) => {
            let mut found_start = false;
            let mut found_end = false;
            for block in blocks {
                for inline in &block.content {
                    let id = inline_id(inline);
                    found_start |= id == &range.start;
                    found_end |= id == &range.end;
                }
                if let BlockKind::Table { rows } = &block.kind {
                    for row in rows {
                        for cell in &row.cells {
                            found_start |= anchor_endpoint_resolves(&cell.blocks, &range.start);
                            found_end |= anchor_endpoint_resolves(&cell.blocks, &range.end);
                        }
                    }
                }
            }
            found_start && found_end
        }
    }
}

fn anchor_endpoint_resolves(blocks: &[Block], target: &StableId) -> bool {
    blocks.iter().any(|block| {
        block
            .content
            .iter()
            .any(|inline| inline_id(inline) == target)
            || matches!(&block.kind, BlockKind::Table { rows } if rows.iter().any(|row| {
                row.cells
                    .iter()
                    .any(|cell| anchor_endpoint_resolves(&cell.blocks, target))
            }))
    })
}

fn nearest_block_anchor(document: &Document, warning: &str) -> Anchor {
    nearest_block_anchor_in_blocks(&document.blocks, warning)
}

fn nearest_block_anchor_in_blocks(blocks: &[Block], warning: &str) -> Anchor {
    if let Some(block) = blocks.first() {
        Anchor::NearestBlock {
            block_id: block.id.clone(),
            warning: warning.to_string(),
        }
    } else {
        Anchor::Document
    }
}

fn find_block_mut<'a>(blocks: &'a mut [Block], block_id: &StableId) -> Option<&'a mut Block> {
    for block in blocks {
        if &block.id == block_id {
            return Some(block);
        }
        if let BlockKind::Table { rows } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    if let Some(found) = find_block_mut(&mut cell.blocks, block_id) {
                        return Some(found);
                    }
                }
            }
        }
    }
    None
}

fn block_exists(blocks: &[Block], block_id: &StableId) -> bool {
    blocks.iter().any(|block| {
        &block.id == block_id
            || matches!(&block.kind, BlockKind::Table { rows } if rows.iter().any(|row| {
                row.cells
                    .iter()
                    .any(|cell| block_exists(&cell.blocks, block_id))
            }))
    })
}

fn insert_block_payload_valid_for_merge(block: &Block, warnings: &mut Vec<ModelWarning>) -> bool {
    match &block.kind {
        BlockKind::Heading { level } if !(1..=6).contains(level) => {
            warnings.push(ModelWarning {
                code: "invalid-heading-level".to_string(),
                message: format!(
                    "inserted heading block {} ignored invalid level {}",
                    block.id, level
                ),
            });
            return false;
        }
        BlockKind::ListItem { level, .. } if *level > 8 => {
            warnings.push(ModelWarning {
                code: "invalid-list-level".to_string(),
                message: format!(
                    "inserted list item block {} ignored invalid level {}",
                    block.id, level
                ),
            });
            return false;
        }
        BlockKind::EquationBlock { equation } if equation.source.trim().is_empty() => {
            warnings.push(ModelWarning {
                code: "invalid-block-equation-source".to_string(),
                message: format!("inserted equation block {} ignored empty source", block.id),
            });
            return false;
        }
        BlockKind::Image { blob_hash, .. } if HashRef::parse(blob_hash).is_err() => {
            warnings.push(ModelWarning {
                code: "invalid-image-blob-hash".to_string(),
                message: format!(
                    "inserted image block {} ignored invalid blob hash",
                    block.id
                ),
            });
            return false;
        }
        BlockKind::Table { rows } => {
            if rows.is_empty() {
                warnings.push(ModelWarning {
                    code: "invalid-table".to_string(),
                    message: format!("inserted table block {} ignored empty rows", block.id),
                });
                return false;
            }
            for row in rows {
                if !table_row_payload_valid_for_merge(row, warnings) {
                    return false;
                }
            }
        }
        _ => {}
    }

    for inline in &block.content {
        if !inline_payload_valid_for_merge(
            inline,
            warnings,
            &format!("inserted block {}", block.id),
        ) {
            return false;
        }
    }

    true
}

fn inline_payload_valid_for_merge(
    inline: &Inline,
    warnings: &mut Vec<ModelWarning>,
    owner: &str,
) -> bool {
    match inline {
        Inline::Text { id, marks, .. } => marks_valid_for_merge(marks, warnings, owner, id),
        Inline::Link {
            id, href, marks, ..
        } => {
            if href.trim().is_empty() {
                warnings.push(ModelWarning {
                    code: "invalid-link-href".to_string(),
                    message: format!("{owner} ignored empty link href on inline {id}"),
                });
                return false;
            }
            marks_valid_for_merge(marks, warnings, owner, id)
        }
        Inline::Mention { id, label } if label.trim().is_empty() => {
            warnings.push(ModelWarning {
                code: "invalid-mention-label".to_string(),
                message: format!("{owner} ignored empty mention label on inline {id}"),
            });
            false
        }
        Inline::Equation { id, equation } if equation.source.trim().is_empty() => {
            warnings.push(ModelWarning {
                code: "invalid-inline-equation-source".to_string(),
                message: format!("{owner} ignored empty equation source on inline {id}"),
            });
            false
        }
        _ => true,
    }
}

fn inline_sequence_payload_valid_for_merge(
    inlines: &[Inline],
    warnings: &mut Vec<ModelWarning>,
    owner: &str,
) -> bool {
    for inline in inlines {
        if !inline_payload_valid_for_merge(inline, warnings, owner) {
            return false;
        }
    }
    true
}

fn inline_sequence_is_empty_source_text(inlines: &[Inline]) -> bool {
    inlines.iter().all(|inline| match inline {
        Inline::Text { text, .. } | Inline::Link { text, .. } => text.trim().is_empty(),
        Inline::Mention { .. }
        | Inline::Equation { .. }
        | Inline::Citation { .. }
        | Inline::FootnoteRef { .. } => false,
    })
}

fn suggestion_insert_content_is_empty(suggestion: &Suggestion) -> bool {
    match &suggestion.kind {
        SuggestionKind::Insert { content, .. } => inline_sequence_is_empty_source_text(content),
        SuggestionKind::Delete { .. } | SuggestionKind::Format { .. } => false,
    }
}

fn comment_thread_has_empty_body(thread: &CommentThread) -> bool {
    thread
        .comments
        .iter()
        .any(|comment| !comment.deleted && inline_sequence_is_empty_source_text(&comment.body))
}

fn table_row_payload_valid_for_merge(row: &TableRow, warnings: &mut Vec<ModelWarning>) -> bool {
    if row.cells.is_empty() {
        warnings.push(ModelWarning {
            code: "invalid-table-row".to_string(),
            message: format!("inserted table row {} ignored empty cells", row.id),
        });
        return false;
    }
    row.cells
        .iter()
        .all(|cell| table_cell_payload_valid_for_merge(cell, warnings))
}

fn table_cell_payload_valid_for_merge(cell: &TableCell, warnings: &mut Vec<ModelWarning>) -> bool {
    if cell.blocks.is_empty() {
        warnings.push(ModelWarning {
            code: "invalid-table-cell".to_string(),
            message: format!("inserted table cell {} ignored empty blocks", cell.id),
        });
        return false;
    }
    cell.blocks
        .iter()
        .all(|block| insert_block_payload_valid_for_merge(block, warnings))
}

fn marks_valid_for_merge(
    marks: &[Mark],
    warnings: &mut Vec<ModelWarning>,
    owner: &str,
    inline_id: &StableId,
) -> bool {
    for mark in marks {
        let needs_value = matches!(
            mark.kind,
            MarkKind::Color | MarkKind::Background | MarkKind::Font | MarkKind::Size
        );
        match (&mark.value, needs_value) {
            (Some(value), true) if value.trim().is_empty() => {
                warnings.push(ModelWarning {
                    code: "invalid-mark-value".to_string(),
                    message: format!("{owner} ignored empty mark value on inline {inline_id}"),
                });
                return false;
            }
            (None, true) => {
                warnings.push(ModelWarning {
                    code: "invalid-mark-value".to_string(),
                    message: format!("{owner} ignored missing mark value on inline {inline_id}"),
                });
                return false;
            }
            (Some(_), false) => {
                warnings.push(ModelWarning {
                    code: "invalid-mark-value".to_string(),
                    message: format!("{owner} ignored boolean mark value on inline {inline_id}"),
                });
                return false;
            }
            _ => {}
        }
    }
    true
}

fn mark_removal_valid_for_merge(
    kind: &MarkKind,
    value: Option<&str>,
    warnings: &mut Vec<ModelWarning>,
    owner: &str,
    inline_id: &StableId,
) -> bool {
    let supports_value = matches!(
        kind,
        MarkKind::Color | MarkKind::Background | MarkKind::Font | MarkKind::Size
    );
    match (value, supports_value) {
        (Some(value), true) if value.trim().is_empty() => {
            warnings.push(ModelWarning {
                code: "invalid-mark-value".to_string(),
                message: format!("{owner} ignored empty mark value on inline {inline_id}"),
            });
            false
        }
        (Some(_), false) => {
            warnings.push(ModelWarning {
                code: "invalid-mark-value".to_string(),
                message: format!("{owner} ignored boolean mark value on inline {inline_id}"),
            });
            false
        }
        _ => true,
    }
}

fn inline_exists(blocks: &[Block], inline_id_to_find: &StableId) -> bool {
    blocks.iter().any(|block| {
        block
            .content
            .iter()
            .any(|inline| inline_id(inline) == inline_id_to_find)
            || matches!(&block.kind, BlockKind::Table { rows } if rows.iter().any(|row| {
                row.cells
                    .iter()
                    .any(|cell| inline_exists(&cell.blocks, inline_id_to_find))
            }))
    })
}

fn insert_block(document: &mut Document, after: Option<StableId>, block: Block) -> bool {
    let mut anchor_degraded = false;
    let insert_at = match after {
        Some(target) => match document.blocks.iter().position(|item| item.id == target) {
            Some(index) => index + 1,
            None => {
                anchor_degraded = true;
                document.blocks.len()
            }
        },
        None => document.blocks.len(),
    };
    if !document.blocks.iter().any(|item| item.id == block.id) {
        document.blocks.insert(insert_at, block);
    }
    anchor_degraded
}

fn delete_block(document: &mut Document, block_id_to_delete: &StableId) -> bool {
    delete_block_in_blocks(&mut document.blocks, block_id_to_delete)
}

fn delete_block_in_blocks(blocks: &mut Vec<Block>, block_id_to_delete: &StableId) -> bool {
    let before = blocks.len();
    blocks.retain(|block| &block.id != block_id_to_delete);
    if blocks.len() != before {
        return true;
    }
    for block in blocks {
        if let BlockKind::Table { rows } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    if delete_block_in_blocks(&mut cell.blocks, block_id_to_delete) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

fn insert_inline(content: &mut Vec<Inline>, after: Option<StableId>, inline: Inline) -> bool {
    let new_inline_id = inline_id(&inline).clone();
    if content.iter().any(|item| inline_id(item) == &new_inline_id) {
        return false;
    }
    let mut anchor_degraded = false;
    let insert_at = match after {
        Some(target) => match content.iter().position(|item| inline_id(item) == &target) {
            Some(index) => index + 1,
            None => {
                anchor_degraded = true;
                content.len()
            }
        },
        None => content.len(),
    };
    content.insert(insert_at, inline);
    anchor_degraded
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InlineMoveResult {
    Applied,
    AnchorDegraded,
    MissingInline,
    MissingBlock,
}

fn move_inline_to_block(
    document: &mut Document,
    inline_id_to_move: &StableId,
    target_block_id: &StableId,
    after: Option<StableId>,
) -> InlineMoveResult {
    if !block_exists(&document.blocks, target_block_id) {
        return InlineMoveResult::MissingBlock;
    }
    let Some(inline) = take_inline(&mut document.blocks, inline_id_to_move) else {
        return InlineMoveResult::MissingInline;
    };
    let Some(target) = find_block_mut(&mut document.blocks, target_block_id) else {
        return InlineMoveResult::MissingBlock;
    };
    if insert_inline(&mut target.content, after, inline) {
        InlineMoveResult::AnchorDegraded
    } else {
        InlineMoveResult::Applied
    }
}

fn take_inline(blocks: &mut [Block], inline_id_to_take: &StableId) -> Option<Inline> {
    for block in blocks {
        if let Some(index) = block
            .content
            .iter()
            .position(|inline| inline_id(inline) == inline_id_to_take)
        {
            return Some(block.content.remove(index));
        }
        if let BlockKind::Table { rows } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    if let Some(inline) = take_inline(&mut cell.blocks, inline_id_to_take) {
                        return Some(inline);
                    }
                }
            }
        }
    }
    None
}

fn insert_table_row(
    document: &mut Document,
    table_block_id: &StableId,
    after_row: Option<StableId>,
    row: TableRow,
) -> TableEditResult {
    let Some(table) = find_block_mut(&mut document.blocks, table_block_id) else {
        return TableEditResult::MissingTable;
    };
    let BlockKind::Table { rows } = &mut table.kind else {
        return TableEditResult::NonTableBlock;
    };
    if rows.iter().any(|item| item.id == row.id) {
        return TableEditResult::Duplicate;
    }
    let mut result = TableEditResult::Applied;
    let insert_at = match after_row {
        Some(target) => match rows.iter().position(|item| item.id == target) {
            Some(index) => index + 1,
            None => {
                result = TableEditResult::AnchorDegraded;
                rows.len()
            }
        },
        None => rows.len(),
    };
    rows.insert(insert_at, row);
    result
}

fn delete_table_row(
    document: &mut Document,
    table_block_id: &StableId,
    row_id: &StableId,
) -> TableEditResult {
    let Some(table) = find_block_mut(&mut document.blocks, table_block_id) else {
        return TableEditResult::MissingTable;
    };
    let BlockKind::Table { rows } = &mut table.kind else {
        return TableEditResult::NonTableBlock;
    };
    let before = rows.len();
    rows.retain(|row| &row.id != row_id);
    if rows.len() == before {
        TableEditResult::MissingRow
    } else if rows.is_empty() {
        rows.push(empty_table_row());
        TableEditResult::AnchorDegraded
    } else {
        TableEditResult::Applied
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TableEditResult {
    Applied,
    AnchorDegraded,
    Duplicate,
    MissingTable,
    NonTableBlock,
    MissingRow,
    MissingCell,
}

fn insert_table_cell(
    document: &mut Document,
    table_block_id: &StableId,
    row_id: &StableId,
    after_cell: Option<StableId>,
    cell: TableCell,
) -> TableEditResult {
    let Some(table) = find_block_mut(&mut document.blocks, table_block_id) else {
        return TableEditResult::MissingTable;
    };
    let BlockKind::Table { rows } = &mut table.kind else {
        return TableEditResult::NonTableBlock;
    };
    let Some(row) = rows.iter_mut().find(|row| &row.id == row_id) else {
        return TableEditResult::MissingRow;
    };
    if row.cells.iter().any(|item| item.id == cell.id) {
        return TableEditResult::Duplicate;
    }
    let insert_at = match after_cell {
        Some(target) => {
            let Some(index) = row.cells.iter().position(|item| item.id == target) else {
                row.cells.push(cell);
                return TableEditResult::AnchorDegraded;
            };
            index + 1
        }
        None => row.cells.len(),
    };
    row.cells.insert(insert_at, cell);
    TableEditResult::Applied
}

fn delete_table_cell(
    document: &mut Document,
    table_block_id: &StableId,
    row_id: &StableId,
    cell_id: &StableId,
) -> TableEditResult {
    let Some(table) = find_block_mut(&mut document.blocks, table_block_id) else {
        return TableEditResult::MissingTable;
    };
    let BlockKind::Table { rows } = &mut table.kind else {
        return TableEditResult::NonTableBlock;
    };
    let Some(row) = rows.iter_mut().find(|row| &row.id == row_id) else {
        return TableEditResult::MissingRow;
    };
    let before = row.cells.len();
    row.cells.retain(|cell| &cell.id != cell_id);
    if row.cells.len() == before {
        TableEditResult::MissingCell
    } else if row.cells.is_empty() {
        row.cells.push(empty_table_cell());
        TableEditResult::AnchorDegraded
    } else {
        TableEditResult::Applied
    }
}

fn empty_table_row() -> TableRow {
    TableRow {
        id: StableId::new("row"),
        cells: vec![empty_table_cell()],
    }
}

fn empty_table_cell() -> TableCell {
    TableCell {
        id: StableId::new("cell"),
        blocks: vec![Block::paragraph("")],
        properties: Vec::new(),
    }
}

fn add_mark(document: &mut Document, text_id: &StableId, mark: Mark) -> bool {
    add_mark_in_blocks(&mut document.blocks, text_id, mark)
}

fn remove_mark(
    document: &mut Document,
    text_id: &StableId,
    kind: &MarkKind,
    value: Option<&str>,
) -> bool {
    remove_mark_in_blocks(&mut document.blocks, text_id, kind, value)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MarkRangeResult {
    Applied,
    Degraded,
    Missing,
}

fn add_mark_range(
    document: &mut Document,
    range: &opendoc_core::TextRange,
    mark: Mark,
) -> MarkRangeResult {
    let ids = editable_inline_ids(&document.blocks);
    let Some(start_index) = ids.iter().position(|id| id == &range.start) else {
        return if ids.iter().any(|id| id == &range.end) {
            add_mark(document, &range.end, mark);
            MarkRangeResult::Degraded
        } else {
            MarkRangeResult::Missing
        };
    };
    let Some(end_index) = ids.iter().position(|id| id == &range.end) else {
        add_mark(document, &range.start, mark);
        return MarkRangeResult::Degraded;
    };
    let first = start_index.min(end_index);
    let last = start_index.max(end_index);
    for id in &ids[first..=last] {
        add_mark(document, id, mark.clone());
    }
    MarkRangeResult::Applied
}

fn editable_inline_ids(blocks: &[Block]) -> Vec<StableId> {
    let mut ids = Vec::new();
    for block in blocks {
        for inline in &block.content {
            if matches!(inline, Inline::Text { .. } | Inline::Link { .. }) {
                ids.push(inline_id(inline).clone());
            }
        }
        if let BlockKind::Table { rows } = &block.kind {
            for row in rows {
                for cell in &row.cells {
                    ids.extend(editable_inline_ids(&cell.blocks));
                }
            }
        }
    }
    ids
}

fn add_mark_in_blocks(blocks: &mut [Block], text_id: &StableId, mark: Mark) -> bool {
    for block in blocks {
        for inline in &mut block.content {
            match inline {
                Inline::Text { id, marks, .. } | Inline::Link { id, marks, .. }
                    if id == text_id =>
                {
                    if !marks.contains(&mark) {
                        marks.push(mark);
                    }
                    return true;
                }
                _ => {}
            }
        }
        if let BlockKind::Table { rows } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    if add_mark_in_blocks(&mut cell.blocks, text_id, mark.clone()) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

fn remove_mark_in_blocks(
    blocks: &mut [Block],
    text_id: &StableId,
    kind_to_remove: &MarkKind,
    value_to_remove: Option<&str>,
) -> bool {
    for block in blocks {
        for inline in &mut block.content {
            match inline {
                Inline::Text { id, marks, .. } | Inline::Link { id, marks, .. }
                    if id == text_id =>
                {
                    marks.retain(|mark| {
                        mark.kind != *kind_to_remove
                            || value_to_remove
                                .is_some_and(|value| mark.value.as_deref() != Some(value))
                    });
                    return true;
                }
                _ => {}
            }
        }
        if let BlockKind::Table { rows } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    if remove_mark_in_blocks(
                        cell.blocks.as_mut_slice(),
                        text_id,
                        kind_to_remove,
                        value_to_remove,
                    ) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

fn delete_inline(document: &mut Document, inline_id_to_delete: &StableId) -> bool {
    delete_inline_in_blocks(&mut document.blocks, inline_id_to_delete)
}

fn delete_inline_in_blocks(blocks: &mut [Block], inline_id_to_delete: &StableId) -> bool {
    for block in blocks {
        let before = block.content.len();
        block
            .content
            .retain(|item| inline_id(item) != inline_id_to_delete);
        if block.content.len() != before {
            return true;
        }
        if let BlockKind::Table { rows } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    if delete_inline_in_blocks(&mut cell.blocks, inline_id_to_delete) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

fn update_inline_text(
    document: &mut Document,
    inline_id_to_update: &StableId,
    text: &str,
) -> Option<bool> {
    update_inline_text_in_blocks(&mut document.blocks, inline_id_to_update, text)
}

fn update_link_href(
    document: &mut Document,
    inline_id_to_update: &StableId,
    href: &str,
) -> Option<bool> {
    update_link_href_in_blocks(&mut document.blocks, inline_id_to_update, href)
}

fn update_inline_equation_source(
    document: &mut Document,
    inline_id_to_update: &StableId,
    source: &str,
) -> Option<bool> {
    update_inline_equation_source_in_blocks(&mut document.blocks, inline_id_to_update, source)
}

fn update_mention_label(
    document: &mut Document,
    inline_id_to_update: &StableId,
    label: &str,
) -> Option<bool> {
    update_mention_label_in_blocks(&mut document.blocks, inline_id_to_update, label)
}

/// Byte index of the `offset`-th Unicode scalar value, clamped to the end.
pub fn byte_index_for_char_offset(value: &str, offset: usize) -> usize {
    value
        .char_indices()
        .nth(offset)
        .map(|(index, _)| index)
        .unwrap_or(value.len())
}

fn edit_inline_text(
    document: &mut Document,
    inline_id_to_update: &StableId,
    edit: impl FnOnce(&mut String),
) -> Option<bool> {
    let mut edit = Some(edit);
    edit_inline_text_in_blocks(&mut document.blocks, inline_id_to_update, &mut edit)
}

fn edit_inline_text_in_blocks(
    blocks: &mut [Block],
    inline_id_to_update: &StableId,
    edit: &mut Option<impl FnOnce(&mut String)>,
) -> Option<bool> {
    for block in blocks {
        for inline in &mut block.content {
            match inline {
                Inline::Text {
                    id, text: value, ..
                }
                | Inline::Link {
                    id, text: value, ..
                } if id == inline_id_to_update => {
                    if let Some(edit) = edit.take() {
                        edit(value);
                    }
                    return Some(true);
                }
                Inline::Citation { id, .. }
                | Inline::FootnoteRef { id, .. }
                | Inline::Mention { id, .. }
                | Inline::Equation { id, .. }
                    if id == inline_id_to_update =>
                {
                    return Some(false);
                }
                _ => {}
            }
        }
        if let BlockKind::Table { rows } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    if let Some(result) =
                        edit_inline_text_in_blocks(&mut cell.blocks, inline_id_to_update, edit)
                    {
                        return Some(result);
                    }
                }
            }
        }
    }
    None
}

fn update_inline_text_in_blocks(
    blocks: &mut [Block],
    inline_id_to_update: &StableId,
    text: &str,
) -> Option<bool> {
    for block in blocks {
        for inline in &mut block.content {
            match inline {
                Inline::Text {
                    id, text: value, ..
                } if id == inline_id_to_update => {
                    *value = text.to_string();
                    return Some(true);
                }
                Inline::Link {
                    id, text: value, ..
                } if id == inline_id_to_update => {
                    *value = text.to_string();
                    return Some(true);
                }
                Inline::Citation { id, .. }
                | Inline::FootnoteRef { id, .. }
                | Inline::Mention { id, .. }
                | Inline::Equation { id, .. }
                    if id == inline_id_to_update =>
                {
                    return Some(false);
                }
                _ => {}
            }
        }
        if let BlockKind::Table { rows } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    if let Some(result) =
                        update_inline_text_in_blocks(&mut cell.blocks, inline_id_to_update, text)
                    {
                        return Some(result);
                    }
                }
            }
        }
    }
    None
}

fn update_mention_label_in_blocks(
    blocks: &mut [Block],
    inline_id_to_update: &StableId,
    label: &str,
) -> Option<bool> {
    for block in blocks {
        for inline in &mut block.content {
            match inline {
                Inline::Mention { id, label: value } if id == inline_id_to_update => {
                    *value = label.to_string();
                    return Some(true);
                }
                Inline::Text { id, .. }
                | Inline::Link { id, .. }
                | Inline::Citation { id, .. }
                | Inline::FootnoteRef { id, .. }
                | Inline::Equation { id, .. }
                    if id == inline_id_to_update =>
                {
                    return Some(false);
                }
                _ => {}
            }
        }
        if let BlockKind::Table { rows } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    if let Some(updated) =
                        update_mention_label_in_blocks(&mut cell.blocks, inline_id_to_update, label)
                    {
                        return Some(updated);
                    }
                }
            }
        }
    }
    None
}

fn update_inline_equation_source_in_blocks(
    blocks: &mut [Block],
    inline_id_to_update: &StableId,
    source: &str,
) -> Option<bool> {
    for block in blocks {
        for inline in &mut block.content {
            match inline {
                Inline::Equation { id, equation } if id == inline_id_to_update => {
                    equation.source = source.to_string();
                    return Some(true);
                }
                Inline::Text { id, .. }
                | Inline::Link { id, .. }
                | Inline::Citation { id, .. }
                | Inline::FootnoteRef { id, .. }
                | Inline::Mention { id, .. }
                    if id == inline_id_to_update =>
                {
                    return Some(false);
                }
                _ => {}
            }
        }
        if let BlockKind::Table { rows } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    if let Some(result) = update_inline_equation_source_in_blocks(
                        &mut cell.blocks,
                        inline_id_to_update,
                        source,
                    ) {
                        return Some(result);
                    }
                }
            }
        }
    }
    None
}

fn update_link_href_in_blocks(
    blocks: &mut [Block],
    inline_id_to_update: &StableId,
    href: &str,
) -> Option<bool> {
    for block in blocks {
        for inline in &mut block.content {
            match inline {
                Inline::Link {
                    id, href: value, ..
                } if id == inline_id_to_update => {
                    *value = href.to_string();
                    return Some(true);
                }
                Inline::Text { id, .. }
                | Inline::Citation { id, .. }
                | Inline::FootnoteRef { id, .. }
                | Inline::Mention { id, .. }
                | Inline::Equation { id, .. }
                    if id == inline_id_to_update =>
                {
                    return Some(false);
                }
                _ => {}
            }
        }
        if let BlockKind::Table { rows } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    if let Some(result) =
                        update_link_href_in_blocks(&mut cell.blocks, inline_id_to_update, href)
                    {
                        return Some(result);
                    }
                }
            }
        }
    }
    None
}

fn update_block_equation_source(
    blocks: &mut [Block],
    block_id_to_update: &StableId,
    source: &str,
) -> Option<bool> {
    for block in blocks {
        if &block.id == block_id_to_update {
            return match &mut block.kind {
                BlockKind::EquationBlock { equation } => {
                    equation.source = source.to_string();
                    Some(true)
                }
                _ => Some(false),
            };
        }
        if let BlockKind::Table { rows } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    if let Some(result) =
                        update_block_equation_source(&mut cell.blocks, block_id_to_update, source)
                    {
                        return Some(result);
                    }
                }
            }
        }
    }
    None
}

fn update_image_alt_text(
    blocks: &mut [Block],
    block_id_to_update: &StableId,
    value: &str,
) -> Option<bool> {
    for block in blocks {
        if &block.id == block_id_to_update {
            return match &mut block.kind {
                BlockKind::Image { alt_text, .. } => {
                    *alt_text = value.to_string();
                    Some(true)
                }
                _ => Some(false),
            };
        }
        if let BlockKind::Table { rows } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    if let Some(result) =
                        update_image_alt_text(&mut cell.blocks, block_id_to_update, value)
                    {
                        return Some(result);
                    }
                }
            }
        }
    }
    None
}

fn update_image_blob_hash(
    blocks: &mut [Block],
    block_id_to_update: &StableId,
    value: &str,
) -> Option<bool> {
    for block in blocks {
        if &block.id == block_id_to_update {
            return match &mut block.kind {
                BlockKind::Image { blob_hash, .. } => {
                    *blob_hash = value.to_string();
                    Some(true)
                }
                _ => Some(false),
            };
        }
        if let BlockKind::Table { rows } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    if let Some(result) =
                        update_image_blob_hash(&mut cell.blocks, block_id_to_update, value)
                    {
                        return Some(result);
                    }
                }
            }
        }
    }
    None
}

fn update_heading_level(
    blocks: &mut [Block],
    block_id_to_update: &StableId,
    level: u8,
) -> Option<bool> {
    for block in blocks {
        if &block.id == block_id_to_update {
            return match &mut block.kind {
                BlockKind::Heading {
                    level: heading_level,
                } => {
                    *heading_level = level;
                    Some(true)
                }
                _ => Some(false),
            };
        }
        if let BlockKind::Table { rows } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    if let Some(result) =
                        update_heading_level(&mut cell.blocks, block_id_to_update, level)
                    {
                        return Some(result);
                    }
                }
            }
        }
    }
    None
}

fn set_block_text_style(
    blocks: &mut [Block],
    block_id_to_update: &StableId,
    style: BlockTextStyle,
) -> Option<bool> {
    for block in blocks {
        if &block.id == block_id_to_update {
            return match &block.kind {
                BlockKind::Paragraph | BlockKind::Heading { .. } | BlockKind::ListItem { .. } => {
                    block.kind = match style {
                        BlockTextStyle::Paragraph => BlockKind::Paragraph,
                        BlockTextStyle::Heading { level } => BlockKind::Heading { level },
                        BlockTextStyle::ListItem {
                            list_id,
                            level,
                            ordered,
                        } => BlockKind::ListItem {
                            list_id,
                            level,
                            ordered,
                        },
                    };
                    Some(true)
                }
                _ => Some(false),
            };
        }
        if let BlockKind::Table { rows } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    if let Some(result) =
                        set_block_text_style(&mut cell.blocks, block_id_to_update, style.clone())
                    {
                        return Some(result);
                    }
                }
            }
        }
    }
    None
}

fn validate_block_text_style(style: &BlockTextStyle) -> Result<(), &'static str> {
    match style {
        BlockTextStyle::Paragraph => Ok(()),
        BlockTextStyle::Heading { level } if (1..=6).contains(level) => Ok(()),
        BlockTextStyle::Heading { .. } => Err("heading level is outside 1..=6"),
        BlockTextStyle::ListItem { level, .. } if *level <= 8 => Ok(()),
        BlockTextStyle::ListItem { .. } => Err("list item level is outside 0..=8"),
    }
}

fn update_list_item(
    blocks: &mut [Block],
    block_id_to_update: &StableId,
    level: u8,
    ordered: bool,
) -> Option<bool> {
    for block in blocks {
        if &block.id == block_id_to_update {
            return match &mut block.kind {
                BlockKind::ListItem {
                    level: item_level,
                    ordered: item_ordered,
                    ..
                } => {
                    *item_level = level;
                    *item_ordered = ordered;
                    Some(true)
                }
                _ => Some(false),
            };
        }
        if let BlockKind::Table { rows } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    if let Some(result) =
                        update_list_item(&mut cell.blocks, block_id_to_update, level, ordered)
                    {
                        return Some(result);
                    }
                }
            }
        }
    }
    None
}

fn inline_id(inline: &Inline) -> &StableId {
    match inline {
        Inline::Text { id, .. }
        | Inline::Link { id, .. }
        | Inline::Citation { id, .. }
        | Inline::FootnoteRef { id, .. }
        | Inline::Mention { id, .. }
        | Inline::Equation { id, .. } => id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opendoc_core::{
        BibliographyReference, CitationItem, CitationPlacement, CitationSource,
        CitationSourceFormat, CitationSummary, Comment, Equation, EquationSourceFormat, MarkExpand,
        MarkKind, SuggestionKind, TextRange,
    };

    #[test]
    fn concurrent_operations_converge_independent_of_stream_order() {
        let mut base = Document::new("Doc");
        let block = Block::paragraph("hello");
        let block_id = block.id.clone();
        let text_id = match &block.content[0] {
            Inline::Text { id, .. } => id.clone(),
            _ => unreachable!(),
        };
        base.blocks.push(block);

        let op_a = Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::InsertInline {
                block_id: block_id.clone(),
                after: Some(text_id.clone()),
                inline: Inline::text(" world"),
            },
        };
        let op_b = Operation {
            id: OperationId {
                actor: ActorId("b".to_string()),
                seq: 1,
            },
            kind: OperationKind::AddMark {
                text_id,
                mark: Mark {
                    kind: MarkKind::Bold,
                    value: None,
                    expand: MarkExpand::Both,
                },
            },
        };
        let merged_ab = merge_operations(&base, &[vec![op_a.clone()], vec![op_b.clone()]]).unwrap();
        let merged_ba = merge_operations(&base, &[vec![op_b], vec![op_a]]).unwrap();
        assert_eq!(
            merged_ab.document.visible_text(),
            merged_ba.document.visible_text()
        );
        assert!(merged_ab.document.validate().is_ok());
    }

    #[test]
    fn document_title_updates_converge_and_reject_empty_titles() {
        let base = Document::new("Base Title");
        let op_a = Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::SetDocumentTitle {
                title: " Alpha Title ".to_string(),
            },
        };
        let op_b = Operation {
            id: OperationId {
                actor: ActorId("b".to_string()),
                seq: 1,
            },
            kind: OperationKind::SetDocumentTitle {
                title: "Beta Title".to_string(),
            },
        };
        let op_empty = Operation {
            id: OperationId {
                actor: ActorId("c".to_string()),
                seq: 1,
            },
            kind: OperationKind::SetDocumentTitle {
                title: " ".to_string(),
            },
        };

        let merged_ab = merge_operations(
            &base,
            &[
                vec![op_a.clone()],
                vec![op_empty.clone()],
                vec![op_b.clone()],
            ],
        )
        .unwrap();
        let merged_ba = merge_operations(
            &base,
            &[vec![op_b], vec![op_empty.clone()], vec![op_a.clone()]],
        )
        .unwrap();
        assert_eq!(merged_ab.document.title, merged_ba.document.title);
        assert_eq!(merged_ab.document.title, "Beta Title");
        let title_from_padded_operation =
            merge_operations(&base, &[vec![op_a], vec![op_empty]]).unwrap();
        assert_eq!(title_from_padded_operation.document.title, "Alpha Title");
        assert!(merged_ab
            .warnings
            .iter()
            .any(|warning| warning.code == "invalid-document-title"));
        assert!(merged_ab.document.validate().is_ok());
    }

    #[test]
    fn document_doi_updates_converge_allow_clear_and_reject_empty_values() {
        let mut base = Document::new("Base Title");
        base.doi = Some("10.0000/base".to_string());
        let op_a = Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::SetDocumentDoi {
                doi: Some("10.1234/Alpha".to_string()),
            },
        };
        let op_b = Operation {
            id: OperationId {
                actor: ActorId("b".to_string()),
                seq: 1,
            },
            kind: OperationKind::SetDocumentDoi { doi: None },
        };
        let op_empty = Operation {
            id: OperationId {
                actor: ActorId("c".to_string()),
                seq: 1,
            },
            kind: OperationKind::SetDocumentDoi {
                doi: Some(" ".to_string()),
            },
        };

        let merged_ab = merge_operations(
            &base,
            &[
                vec![op_a.clone()],
                vec![op_empty.clone()],
                vec![op_b.clone()],
            ],
        )
        .unwrap();
        let merged_ba = merge_operations(&base, &[vec![op_b], vec![op_empty], vec![op_a]]).unwrap();
        assert_eq!(merged_ab.document.doi, merged_ba.document.doi);
        assert_eq!(merged_ab.document.doi, None);
        assert!(merged_ab
            .warnings
            .iter()
            .any(|warning| warning.code == "invalid-document-doi"));
        assert!(merged_ab.document.validate().is_ok());
    }

    #[test]
    fn document_locale_updates_converge_and_reject_empty_values() {
        let base = Document::new("Base Title");
        let op_a = Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::SetDocumentLocale {
                locale: " sv-SE ".to_string(),
            },
        };
        let op_b = Operation {
            id: OperationId {
                actor: ActorId("b".to_string()),
                seq: 1,
            },
            kind: OperationKind::SetDocumentLocale {
                locale: "en-GB".to_string(),
            },
        };
        let op_empty = Operation {
            id: OperationId {
                actor: ActorId("c".to_string()),
                seq: 1,
            },
            kind: OperationKind::SetDocumentLocale {
                locale: " ".to_string(),
            },
        };

        let merged_ab = merge_operations(
            &base,
            &[
                vec![op_a.clone()],
                vec![op_empty.clone()],
                vec![op_b.clone()],
            ],
        )
        .unwrap();
        let merged_ba = merge_operations(&base, &[vec![op_b], vec![op_empty], vec![op_a]]).unwrap();
        assert_eq!(merged_ab.document.locale, merged_ba.document.locale);
        assert_eq!(merged_ab.document.locale, "en-GB");
        assert!(merged_ab
            .warnings
            .iter()
            .any(|warning| warning.code == "invalid-document-locale"));
        assert!(merged_ab.document.validate().is_ok());
    }

    #[test]
    fn duplicate_insert_ids_degrade_to_warnings() {
        let mut base = Document::new("Doc");
        let existing_block = Block::paragraph("existing");
        let existing_block_id = existing_block.id.clone();
        let existing_inline_id = inline_id(&existing_block.content[0]).clone();
        base.blocks.push(existing_block);

        let duplicate_block = Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::InsertBlock {
                after: None,
                block: Block {
                    id: existing_block_id,
                    kind: BlockKind::Paragraph,
                    content: vec![Inline::text("duplicate block")],
                    properties: Vec::new(),
                },
            },
        };
        let duplicate_inline = Operation {
            id: OperationId {
                actor: ActorId("b".to_string()),
                seq: 1,
            },
            kind: OperationKind::InsertInline {
                block_id: base.blocks[0].id.clone(),
                after: None,
                inline: Inline::Text {
                    id: existing_inline_id,
                    text: "duplicate inline".to_string(),
                    marks: Vec::new(),
                },
            },
        };

        let result =
            merge_operations(&base, &[vec![duplicate_block], vec![duplicate_inline]]).unwrap();

        assert_eq!(result.document.visible_text(), "existing\n");
        assert_eq!(
            result
                .warnings
                .iter()
                .map(|warning| warning.code.as_str())
                .collect::<Vec<_>>(),
            vec!["duplicate-block", "duplicate-inline"]
        );
        assert!(result.document.validate().is_ok());
    }

    #[test]
    fn inline_insert_into_deleted_block_appends_to_surviving_block() {
        let mut base = Document::new("Doc");
        let deleted_block = Block::paragraph("deleted");
        let deleted_block_id = deleted_block.id.clone();
        let surviving_block = Block::paragraph("surviving");
        base.blocks.push(deleted_block);
        base.blocks.push(surviving_block);

        let delete = Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::DeleteBlock {
                block_id: deleted_block_id.clone(),
            },
        };
        let insert = Operation {
            id: OperationId {
                actor: ActorId("b".to_string()),
                seq: 1,
            },
            kind: OperationKind::InsertInline {
                block_id: deleted_block_id,
                after: None,
                inline: Inline::text(" preserved"),
            },
        };

        let delete_first =
            merge_operations(&base, &[vec![delete.clone()], vec![insert.clone()]]).unwrap();
        let insert_first = merge_operations(&base, &[vec![insert], vec![delete]]).unwrap();

        assert_eq!(delete_first.document, insert_first.document);
        assert_eq!(
            delete_first.document.visible_text(),
            "surviving preserved\n"
        );
        assert!(delete_first
            .warnings
            .iter()
            .any(|warning| warning.code == "inline-anchor-degraded"));
        delete_first.document.validate().unwrap();
    }

    #[test]
    fn inline_insert_after_deleted_inline_anchor_appends_with_warning() {
        let mut base = Document::new("Doc");
        let mut block = Block::paragraph("anchor");
        let block_id = block.id.clone();
        let deleted_inline_id = inline_id(&block.content[0]).clone();
        block.content.push(Inline::text(" tail"));
        base.blocks.push(block);

        let delete = Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::DeleteInline {
                inline_id: deleted_inline_id.clone(),
            },
        };
        let insert = Operation {
            id: OperationId {
                actor: ActorId("b".to_string()),
                seq: 1,
            },
            kind: OperationKind::InsertInline {
                block_id,
                after: Some(deleted_inline_id),
                inline: Inline::text(" inserted"),
            },
        };

        let actor_streams =
            merge_operations(&base, &[vec![delete.clone()], vec![insert.clone()]]).unwrap();
        let storage_batch =
            merge_operations(&base, &[vec![delete.clone(), insert.clone()], vec![]]).unwrap();
        let reversed_batches = merge_operations(&base, &[vec![insert], vec![delete]]).unwrap();

        assert_eq!(actor_streams.document, storage_batch.document);
        assert_eq!(actor_streams.document, reversed_batches.document);
        assert_eq!(actor_streams.document.visible_text(), " tail inserted\n");
        assert!(actor_streams
            .warnings
            .iter()
            .any(|warning| warning.code == "inline-anchor-degraded"));
        actor_streams.document.validate().unwrap();
    }

    #[test]
    fn block_insert_after_deleted_anchor_appends_with_warning() {
        let mut base = Document::new("Doc");
        let deleted_block = Block::paragraph("deleted");
        let deleted_block_id = deleted_block.id.clone();
        let surviving_block = Block::paragraph("surviving");
        base.blocks.push(deleted_block);
        base.blocks.push(surviving_block);

        let delete = Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::DeleteBlock {
                block_id: deleted_block_id.clone(),
            },
        };
        let insert = Operation {
            id: OperationId {
                actor: ActorId("b".to_string()),
                seq: 1,
            },
            kind: OperationKind::InsertBlock {
                after: Some(deleted_block_id),
                block: Block::paragraph("inserted"),
            },
        };

        let delete_first =
            merge_operations(&base, &[vec![delete.clone()], vec![insert.clone()]]).unwrap();
        let insert_first = merge_operations(&base, &[vec![insert], vec![delete]]).unwrap();

        assert_eq!(delete_first.document, insert_first.document);
        assert_eq!(
            delete_first.document.visible_text(),
            "surviving\ninserted\n"
        );
        assert!(delete_first
            .warnings
            .iter()
            .any(|warning| warning.code == "block-anchor-degraded"));
        delete_first.document.validate().unwrap();
    }

    #[test]
    fn duplicate_operation_ids_select_deterministic_payload_with_warning() {
        let mut base = Document::new("Doc");
        let block = Block::paragraph("base");
        let block_id = block.id.clone();
        base.blocks.push(block);
        let duplicate_id = OperationId {
            actor: ActorId("actor-a".to_string()),
            seq: 7,
        };
        let insert_alpha = Operation {
            id: duplicate_id.clone(),
            kind: OperationKind::InsertInline {
                block_id: block_id.clone(),
                after: None,
                inline: Inline::Text {
                    id: StableId::parse("text-alpha").unwrap(),
                    text: "alpha ".to_string(),
                    marks: Vec::new(),
                },
            },
        };
        let insert_zeta = Operation {
            id: duplicate_id,
            kind: OperationKind::InsertInline {
                block_id,
                after: None,
                inline: Inline::Text {
                    id: StableId::parse("text-zeta").unwrap(),
                    text: "zeta ".to_string(),
                    marks: Vec::new(),
                },
            },
        };

        let alpha_first = merge_operations(
            &base,
            &[vec![insert_alpha.clone()], vec![insert_zeta.clone()]],
        )
        .unwrap();
        let zeta_first = merge_operations(&base, &[vec![insert_zeta], vec![insert_alpha]]).unwrap();

        assert_eq!(alpha_first.document, zeta_first.document);
        assert_eq!(alpha_first.warnings, zeta_first.warnings);
        assert_eq!(alpha_first.document.visible_text(), "basealpha \n");
        assert_eq!(alpha_first.warnings[0].code, "duplicate-operation-id");
        alpha_first.document.validate().unwrap();
    }

    #[test]
    fn malformed_operation_ids_are_ignored_with_deterministic_warnings() {
        let mut base = Document::new("Doc");
        let block = Block::paragraph("base");
        let block_id = block.id.clone();
        base.blocks.push(block);
        let malformed = [
            Operation {
                id: OperationId {
                    actor: ActorId(String::new()),
                    seq: 1,
                },
                kind: OperationKind::InsertInline {
                    block_id: block_id.clone(),
                    after: None,
                    inline: Inline::text("empty actor"),
                },
            },
            Operation {
                id: OperationId {
                    actor: ActorId(" actor-a ".to_string()),
                    seq: 2,
                },
                kind: OperationKind::InsertInline {
                    block_id: block_id.clone(),
                    after: None,
                    inline: Inline::text("padded actor"),
                },
            },
            Operation {
                id: OperationId {
                    actor: ActorId("actor-b".to_string()),
                    seq: 0,
                },
                kind: OperationKind::InsertInline {
                    block_id,
                    after: None,
                    inline: Inline::text("zero seq"),
                },
            },
        ];

        let result = merge_operations(
            &base,
            &[
                vec![malformed[1].clone(), malformed[0].clone()],
                vec![malformed[2].clone()],
            ],
        )
        .unwrap();
        let reversed = merge_operations(
            &base,
            &[
                vec![malformed[2].clone()],
                vec![malformed[0].clone(), malformed[1].clone()],
            ],
        )
        .unwrap();

        assert_eq!(result.document, reversed.document);
        assert_eq!(result.warnings, reversed.warnings);
        assert_eq!(result.document.visible_text(), "base\n");
        assert_eq!(
            result
                .warnings
                .iter()
                .filter(|warning| warning.code == "invalid-operation-id")
                .count(),
            3
        );
        assert!(result
            .warnings
            .iter()
            .any(|warning| warning.message == "operation with empty actor was ignored"));
        assert!(
            result
                .warnings
                .iter()
                .any(|warning| warning.message
                    == "operation with whitespace-padded actor was ignored")
        );
        assert!(result
            .warnings
            .iter()
            .any(|warning| warning.message == "operation with zero sequence was ignored"));
        result.document.validate().unwrap();
    }

    #[test]
    fn remove_mark_updates_formatted_inline_by_stable_id() {
        let mut base = Document::new("Doc");
        let mut block = Block::paragraph("hello");
        let text_id = match &mut block.content[0] {
            Inline::Text { id, marks, .. } => {
                marks.push(Mark {
                    kind: MarkKind::Bold,
                    value: None,
                    expand: MarkExpand::Both,
                });
                marks.push(Mark {
                    kind: MarkKind::Color,
                    value: Some("#2255aa".to_string()),
                    expand: MarkExpand::Both,
                });
                id.clone()
            }
            _ => unreachable!(),
        };
        base.blocks.push(block);

        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::RemoveMark {
                    text_id,
                    kind: MarkKind::Bold,
                    value: None,
                },
            }]],
        )
        .unwrap();

        let marks = match &result.document.blocks[0].content[0] {
            Inline::Text { marks, .. } => marks,
            _ => unreachable!(),
        };
        assert!(!marks.iter().any(|mark| mark.kind == MarkKind::Bold));
        assert!(marks.iter().any(|mark| mark.kind == MarkKind::Color));
    }

    #[test]
    fn invalid_mark_operation_degrades_to_warning() {
        let mut base = Document::new("Doc");
        let block = Block::paragraph("hello");
        let text_id = inline_id(&block.content[0]).clone();
        base.blocks.push(block);

        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::AddMark {
                    text_id,
                    mark: Mark {
                        kind: MarkKind::Color,
                        value: None,
                        expand: MarkExpand::Both,
                    },
                },
            }]],
        )
        .unwrap();

        let marks = match &result.document.blocks[0].content[0] {
            Inline::Text { marks, .. } => marks,
            _ => unreachable!(),
        };
        assert!(marks.is_empty());
        assert_eq!(result.warnings[0].code, "invalid-mark-value");
        result.document.validate().unwrap();
    }

    #[test]
    fn invalid_mark_removal_operation_degrades_to_warning() {
        let mut base = Document::new("Doc");
        let mut block = Block::paragraph("hello");
        let text_id = match &mut block.content[0] {
            Inline::Text { id, marks, .. } => {
                marks.push(Mark {
                    kind: MarkKind::Bold,
                    value: None,
                    expand: MarkExpand::Both,
                });
                id.clone()
            }
            _ => unreachable!(),
        };
        base.blocks.push(block);

        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::RemoveMark {
                    text_id,
                    kind: MarkKind::Bold,
                    value: Some("true".to_string()),
                },
            }]],
        )
        .unwrap();

        let marks = match &result.document.blocks[0].content[0] {
            Inline::Text { marks, .. } => marks,
            _ => unreachable!(),
        };
        assert!(marks.iter().any(|mark| mark.kind == MarkKind::Bold));
        assert_eq!(result.warnings[0].code, "invalid-mark-value");
        result.document.validate().unwrap();
    }

    #[test]
    fn valued_mark_removal_without_value_removes_all_matching_marks() {
        let mut base = Document::new("Doc");
        let mut block = Block::paragraph("hello");
        let text_id = match &mut block.content[0] {
            Inline::Text { id, marks, .. } => {
                marks.push(Mark {
                    kind: MarkKind::Color,
                    value: Some("#2255aa".to_string()),
                    expand: MarkExpand::Both,
                });
                marks.push(Mark {
                    kind: MarkKind::Bold,
                    value: None,
                    expand: MarkExpand::Both,
                });
                id.clone()
            }
            _ => unreachable!(),
        };
        base.blocks.push(block);

        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::RemoveMark {
                    text_id,
                    kind: MarkKind::Color,
                    value: None,
                },
            }]],
        )
        .unwrap();

        let marks = match &result.document.blocks[0].content[0] {
            Inline::Text { marks, .. } => marks,
            _ => unreachable!(),
        };
        assert!(!marks.iter().any(|mark| mark.kind == MarkKind::Color));
        assert!(marks.iter().any(|mark| mark.kind == MarkKind::Bold));
        assert!(result.warnings.is_empty());
        result.document.validate().unwrap();
    }

    #[test]
    fn invalid_mark_range_operation_degrades_to_warning() {
        let mut base = Document::new("Doc");
        let mut block = Block::paragraph("");
        block.content.clear();
        let first = Inline::text("alpha ");
        let first_id = inline_id(&first).clone();
        let second = Inline::text("beta");
        let second_id = inline_id(&second).clone();
        block.content.extend([first, second]);
        base.blocks.push(block);

        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::AddMarkRange {
                    range: TextRange {
                        start: first_id,
                        end: second_id,
                    },
                    mark: Mark {
                        kind: MarkKind::Bold,
                        value: Some("true".to_string()),
                        expand: MarkExpand::Both,
                    },
                },
            }]],
        )
        .unwrap();

        assert!(result
            .document
            .blocks
            .iter()
            .flat_map(|block| &block.content)
            .all(|inline| match inline {
                Inline::Text { marks, .. } => marks.is_empty(),
                _ => true,
            }));
        assert_eq!(result.warnings[0].code, "invalid-mark-value");
        result.document.validate().unwrap();
    }

    #[test]
    fn comment_anchor_degrades_to_nearest_block_when_text_is_missing() {
        let mut base = Document::new("Doc");
        base.blocks.push(Block::paragraph("hello"));
        let thread = CommentThread {
            id: StableId::new("comment-thread"),
            anchor: Anchor::TextRange(TextRange {
                start: StableId::parse("missing-start").unwrap(),
                end: StableId::parse("missing-end").unwrap(),
            }),
            comments: vec![Comment {
                id: StableId::new("comment"),
                author: "Alice".to_string(),
                body: vec![Inline::text("note")],
                created_at_ms: 1,
                deleted: false,
            }],
            deleted: false,
        };
        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::AddCommentThread { thread },
            }]],
        )
        .unwrap();
        assert_eq!(result.warnings[0].code, "comment-anchor-degraded");
        assert!(matches!(
            result.document.comments[0].anchor,
            Anchor::NearestBlock { .. }
        ));
    }

    #[test]
    fn existing_comment_anchor_repairs_after_target_text_delete() {
        let mut base = Document::new("Doc");
        let block = Block::paragraph("hello");
        let text_id = match &block.content[0] {
            Inline::Text { id, .. } => id.clone(),
            _ => unreachable!(),
        };
        base.blocks.push(block);
        let thread_id = StableId::new("comment-thread");
        base.comments.push(CommentThread {
            id: thread_id.clone(),
            anchor: Anchor::TextRange(TextRange {
                start: text_id.clone(),
                end: text_id.clone(),
            }),
            comments: vec![Comment {
                id: StableId::new("comment"),
                author: "Alice".to_string(),
                body: vec![Inline::text("note")],
                created_at_ms: 1,
                deleted: false,
            }],
            deleted: false,
        });

        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::DeleteInline { inline_id: text_id },
            }]],
        )
        .unwrap();

        assert_eq!(result.warnings[0].code, "comment-anchor-degraded");
        assert!(result.warnings[0].message.contains(thread_id.as_str()));
        assert!(matches!(
            result.document.comments[0].anchor,
            Anchor::NearestBlock { .. }
        ));
        assert_eq!(result.document.visible_text(), "\n");
    }

    #[test]
    fn comment_range_collapses_to_surviving_endpoint_when_partially_deleted() {
        let mut base = Document::new("Doc");
        let mut block = Block::paragraph("");
        block.content.clear();
        let first = Inline::text("alpha ");
        let first_id = inline_id(&first).clone();
        let second = Inline::text("omega");
        let second_id = inline_id(&second).clone();
        block.content.extend([first, second]);
        base.blocks.push(block);

        let thread_id = StableId::parse("comment-thread-partial-anchor").unwrap();
        let thread = CommentThread {
            id: thread_id.clone(),
            anchor: Anchor::TextRange(TextRange {
                start: first_id.clone(),
                end: second_id.clone(),
            }),
            comments: vec![Comment {
                id: StableId::parse("comment-partial-anchor").unwrap(),
                author: "Alice".to_string(),
                body: vec![Inline::text("note")],
                created_at_ms: 1,
                deleted: false,
            }],
            deleted: false,
        };
        base.comments.push(thread.clone());

        let delete = Operation {
            id: OperationId {
                actor: ActorId("actor-delete".to_string()),
                seq: 1,
            },
            kind: OperationKind::DeleteInline {
                inline_id: first_id,
            },
        };
        let duplicate_add = Operation {
            id: OperationId {
                actor: ActorId("actor-comment".to_string()),
                seq: 1,
            },
            kind: OperationKind::AddCommentThread { thread },
        };

        let delete_first =
            merge_operations(&base, &[vec![delete.clone()], vec![duplicate_add.clone()]]).unwrap();
        let comment_first = merge_operations(&base, &[vec![duplicate_add], vec![delete]]).unwrap();

        assert_eq!(delete_first.document, comment_first.document);
        assert_eq!(delete_first.warnings, comment_first.warnings);
        assert_eq!(delete_first.document.visible_text(), "omega\n");
        assert!(delete_first
            .warnings
            .iter()
            .any(|warning| warning.code == "comment-anchor-degraded"));
        let thread = delete_first
            .document
            .comments
            .iter()
            .find(|thread| thread.id == thread_id)
            .expect("comment thread survives with repaired anchor");
        assert!(matches!(
            &thread.anchor,
            Anchor::TextRange(range) if range.start == second_id && range.end == second_id
        ));
        delete_first.document.validate().unwrap();
    }

    #[test]
    fn comment_delete_marks_comment_and_thread_when_empty() {
        let mut base = Document::new("Doc");
        base.blocks.push(Block::paragraph("hello"));
        let thread_id = StableId::new("comment-thread");
        let comment_id = StableId::new("comment");
        base.comments.push(CommentThread {
            id: thread_id.clone(),
            anchor: Anchor::Document,
            comments: vec![Comment {
                id: comment_id.clone(),
                author: "Alice".to_string(),
                body: vec![Inline::text("note")],
                created_at_ms: 1,
                deleted: false,
            }],
            deleted: false,
        });

        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::DeleteComment {
                    thread_id,
                    comment_id,
                },
            }]],
        )
        .unwrap();

        assert!(result.document.comments[0].deleted);
        assert!(result.document.comments[0].comments[0].deleted);
    }

    #[test]
    fn comment_thread_restore_converges_with_delete() {
        let mut base = Document::new("Doc");
        base.blocks.push(Block::paragraph("hello"));
        let thread_id = StableId::new("comment-thread");
        base.comments.push(CommentThread {
            id: thread_id.clone(),
            anchor: Anchor::Document,
            comments: vec![Comment {
                id: StableId::new("comment"),
                author: "Alice".to_string(),
                body: vec![Inline::text("note")],
                created_at_ms: 1,
                deleted: false,
            }],
            deleted: false,
        });

        let delete = Operation {
            id: OperationId {
                actor: ActorId("z".to_string()),
                seq: 1,
            },
            kind: OperationKind::DeleteCommentThread {
                thread_id: thread_id.clone(),
            },
        };
        let restore = Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::RestoreCommentThread { thread_id },
        };

        let restore_first =
            merge_operations(&base, &[vec![restore.clone()], vec![delete.clone()]]).unwrap();
        let delete_first = merge_operations(&base, &[vec![delete], vec![restore]]).unwrap();

        assert_eq!(restore_first.document, delete_first.document);
        assert!(!restore_first.document.comments[0].deleted);
        assert!(!restore_first.document.comments[0].comments[0].deleted);
        assert!(restore_first.warnings.is_empty());
        assert!(restore_first.document.validate().is_ok());
    }

    #[test]
    fn single_comment_restore_converges_with_delete() {
        let mut base = Document::new("Doc");
        base.blocks.push(Block::paragraph("hello"));
        let thread_id = StableId::new("comment-thread");
        let comment_id = StableId::new("comment");
        base.comments.push(CommentThread {
            id: thread_id.clone(),
            anchor: Anchor::Document,
            comments: vec![Comment {
                id: comment_id.clone(),
                author: "Alice".to_string(),
                body: vec![Inline::text("note")],
                created_at_ms: 1,
                deleted: false,
            }],
            deleted: false,
        });

        let delete = Operation {
            id: OperationId {
                actor: ActorId("z".to_string()),
                seq: 1,
            },
            kind: OperationKind::DeleteComment {
                thread_id: thread_id.clone(),
                comment_id: comment_id.clone(),
            },
        };
        let restore = Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::RestoreComment {
                thread_id,
                comment_id,
            },
        };

        let restore_first =
            merge_operations(&base, &[vec![restore.clone()], vec![delete.clone()]]).unwrap();
        let delete_first = merge_operations(&base, &[vec![delete], vec![restore]]).unwrap();

        assert_eq!(restore_first.document, delete_first.document);
        assert!(!restore_first.document.comments[0].deleted);
        assert!(!restore_first.document.comments[0].comments[0].deleted);
        assert!(restore_first.warnings.is_empty());
        assert!(restore_first.document.validate().is_ok());
    }

    #[test]
    fn comment_thread_delete_beats_stale_reply_and_body_update_without_reopening() {
        let mut base = Document::new("Doc");
        base.blocks.push(Block::paragraph("hello"));
        let thread_id = StableId::parse("comment-thread-delete-stale").unwrap();
        let comment_id = StableId::parse("comment-delete-stale").unwrap();
        base.comments.push(CommentThread {
            id: thread_id.clone(),
            anchor: Anchor::Document,
            comments: vec![Comment {
                id: comment_id.clone(),
                author: "Alice".to_string(),
                body: vec![Inline::text("original note")],
                created_at_ms: 1,
                deleted: false,
            }],
            deleted: false,
        });

        let stale_reply = Operation {
            id: OperationId {
                actor: ActorId("actor-a".to_string()),
                seq: 1,
            },
            kind: OperationKind::AddCommentReply {
                thread_id: thread_id.clone(),
                comment: Comment {
                    id: StableId::parse("comment-stale-reply").unwrap(),
                    author: "Bob".to_string(),
                    body: vec![Inline::text("stale reply")],
                    created_at_ms: 2,
                    deleted: false,
                },
            },
        };
        let stale_update = Operation {
            id: OperationId {
                actor: ActorId("actor-b".to_string()),
                seq: 1,
            },
            kind: OperationKind::UpdateCommentBody {
                thread_id: thread_id.clone(),
                comment_id: comment_id.clone(),
                body: vec![Inline::text("stale update")],
            },
        };
        let delete = Operation {
            id: OperationId {
                actor: ActorId("actor-z".to_string()),
                seq: 1,
            },
            kind: OperationKind::DeleteCommentThread {
                thread_id: thread_id.clone(),
            },
        };

        let actor_streams = merge_operations(
            &base,
            &[
                vec![stale_reply.clone()],
                vec![stale_update.clone()],
                vec![delete.clone()],
            ],
        )
        .unwrap();
        let reversed_batches = merge_operations(
            &base,
            &[vec![delete], vec![stale_update], vec![stale_reply]],
        )
        .unwrap();

        assert_eq!(actor_streams.document, reversed_batches.document);
        assert_eq!(actor_streams.warnings, reversed_batches.warnings);
        let thread = &actor_streams.document.comments[0];
        assert!(thread.deleted);
        assert_eq!(thread.comments.len(), 1);
        assert!(thread.comments[0].body.iter().any(|inline| match inline {
            Inline::Text { text, .. } => text == "original note",
            _ => false,
        }));
        let warning_codes: BTreeSet<_> = actor_streams
            .warnings
            .iter()
            .map(|warning| warning.code.as_str())
            .collect();
        assert!(warning_codes.contains("stale-comment-reply"));
        assert!(warning_codes.contains("stale-comment-update"));
        actor_streams.document.validate().unwrap();
    }

    #[test]
    fn last_comment_delete_beats_concurrent_reply_without_order_divergence() {
        let mut base = Document::new("Doc");
        base.blocks.push(Block::paragraph("hello"));
        let thread_id = StableId::parse("comment-thread-last-delete").unwrap();
        let comment_id = StableId::parse("comment-last-delete").unwrap();
        base.comments.push(CommentThread {
            id: thread_id.clone(),
            anchor: Anchor::Document,
            comments: vec![Comment {
                id: comment_id.clone(),
                author: "Alice".to_string(),
                body: vec![Inline::text("original note")],
                created_at_ms: 1,
                deleted: false,
            }],
            deleted: false,
        });

        let stale_reply = Operation {
            id: OperationId {
                actor: ActorId("actor-a".to_string()),
                seq: 1,
            },
            kind: OperationKind::AddCommentReply {
                thread_id: thread_id.clone(),
                comment: Comment {
                    id: StableId::parse("comment-stale-after-last-delete").unwrap(),
                    author: "Bob".to_string(),
                    body: vec![Inline::text("stale reply")],
                    created_at_ms: 2,
                    deleted: false,
                },
            },
        };
        let delete = Operation {
            id: OperationId {
                actor: ActorId("actor-z".to_string()),
                seq: 1,
            },
            kind: OperationKind::DeleteComment {
                thread_id: thread_id.clone(),
                comment_id: comment_id.clone(),
            },
        };

        let reply_first =
            merge_operations(&base, &[vec![stale_reply.clone()], vec![delete.clone()]]).unwrap();
        let delete_first = merge_operations(&base, &[vec![delete], vec![stale_reply]]).unwrap();

        assert_eq!(reply_first.document, delete_first.document);
        assert_eq!(reply_first.warnings, delete_first.warnings);
        let thread = &reply_first.document.comments[0];
        assert!(thread.deleted);
        assert_eq!(thread.comments.len(), 1);
        assert!(thread.comments[0].deleted);
        assert!(reply_first
            .warnings
            .iter()
            .any(|warning| warning.code == "stale-comment-reply"));
        reply_first.document.validate().unwrap();
    }

    #[test]
    fn single_comment_delete_preserves_concurrent_reply_when_thread_still_has_live_comment() {
        let mut base = Document::new("Doc");
        base.blocks.push(Block::paragraph("hello"));
        let thread_id = StableId::parse("comment-thread-partial-delete").unwrap();
        let deleted_comment_id = StableId::parse("comment-partial-delete").unwrap();
        let survivor_comment_id = StableId::parse("comment-partial-survivor").unwrap();
        let reply_id = StableId::parse("comment-partial-reply").unwrap();
        base.comments.push(CommentThread {
            id: thread_id.clone(),
            anchor: Anchor::Document,
            comments: vec![
                Comment {
                    id: deleted_comment_id.clone(),
                    author: "Alice".to_string(),
                    body: vec![Inline::text("delete this note")],
                    created_at_ms: 1,
                    deleted: false,
                },
                Comment {
                    id: survivor_comment_id.clone(),
                    author: "Carol".to_string(),
                    body: vec![Inline::text("surviving note")],
                    created_at_ms: 2,
                    deleted: false,
                },
            ],
            deleted: false,
        });

        let reply = Operation {
            id: OperationId {
                actor: ActorId("actor-a".to_string()),
                seq: 1,
            },
            kind: OperationKind::AddCommentReply {
                thread_id: thread_id.clone(),
                comment: Comment {
                    id: reply_id.clone(),
                    author: "Bob".to_string(),
                    body: vec![Inline::text("valid reply")],
                    created_at_ms: 3,
                    deleted: false,
                },
            },
        };
        let delete = Operation {
            id: OperationId {
                actor: ActorId("actor-z".to_string()),
                seq: 1,
            },
            kind: OperationKind::DeleteComment {
                thread_id,
                comment_id: deleted_comment_id.clone(),
            },
        };

        let reply_first =
            merge_operations(&base, &[vec![reply.clone()], vec![delete.clone()]]).unwrap();
        let delete_first = merge_operations(&base, &[vec![delete], vec![reply]]).unwrap();

        assert_eq!(reply_first.document, delete_first.document);
        assert_eq!(reply_first.warnings, delete_first.warnings);
        assert!(reply_first.warnings.is_empty());
        let thread = &reply_first.document.comments[0];
        assert!(!thread.deleted);
        assert!(thread
            .comments
            .iter()
            .any(|comment| comment.id == deleted_comment_id && comment.deleted));
        assert!(thread
            .comments
            .iter()
            .any(|comment| comment.id == survivor_comment_id && !comment.deleted));
        assert!(thread
            .comments
            .iter()
            .any(|comment| comment.id == reply_id && !comment.deleted));
        reply_first.document.validate().unwrap();
    }

    #[test]
    fn comment_body_updates_by_stable_thread_and_comment_id() {
        let mut base = Document::new("Doc");
        base.blocks.push(Block::paragraph("hello"));
        let thread_id = StableId::new("comment-thread");
        let comment_id = StableId::new("comment");
        base.comments.push(CommentThread {
            id: thread_id.clone(),
            anchor: Anchor::Document,
            comments: vec![Comment {
                id: comment_id.clone(),
                author: "Alice".to_string(),
                body: vec![Inline::text("old note")],
                created_at_ms: 1,
                deleted: false,
            }],
            deleted: false,
        });

        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::UpdateCommentBody {
                    thread_id,
                    comment_id,
                    body: vec![Inline::text("updated note")],
                },
            }]],
        )
        .unwrap();

        match &result.document.comments[0].comments[0].body[0] {
            Inline::Text { text, .. } => assert_eq!(text, "updated note"),
            _ => unreachable!(),
        }
    }

    #[test]
    fn concurrent_comment_replies_append_deterministically() {
        let mut base = Document::new("Doc");
        base.blocks.push(Block::paragraph("hello"));
        let thread_id = StableId::new("comment-thread");
        base.comments.push(CommentThread {
            id: thread_id.clone(),
            anchor: Anchor::Document,
            comments: vec![Comment {
                id: StableId::parse("comment-root").unwrap(),
                author: "Alice".to_string(),
                body: vec![Inline::text("root note")],
                created_at_ms: 1,
                deleted: false,
            }],
            deleted: false,
        });
        let first_reply = Operation {
            id: OperationId {
                actor: ActorId("b".to_string()),
                seq: 1,
            },
            kind: OperationKind::AddCommentReply {
                thread_id: thread_id.clone(),
                comment: Comment {
                    id: StableId::parse("comment-reply-b").unwrap(),
                    author: "Bob".to_string(),
                    body: vec![Inline::text("second reply")],
                    created_at_ms: 3,
                    deleted: false,
                },
            },
        };
        let second_reply = Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::AddCommentReply {
                thread_id,
                comment: Comment {
                    id: StableId::parse("comment-reply-a").unwrap(),
                    author: "Carol".to_string(),
                    body: vec![Inline::text("first reply")],
                    created_at_ms: 2,
                    deleted: false,
                },
            },
        };

        let left = merge_operations(
            &base,
            &[vec![first_reply.clone()], vec![second_reply.clone()]],
        )
        .unwrap();
        let right = merge_operations(&base, &[vec![second_reply], vec![first_reply]]).unwrap();

        assert_eq!(left.document, right.document);
        assert_eq!(left.document.comments[0].comments.len(), 3);
        assert_eq!(
            left.document.comments[0]
                .comments
                .iter()
                .map(|comment| comment.id.as_str())
                .collect::<Vec<_>>(),
            ["comment-root", "comment-reply-a", "comment-reply-b"]
        );
        assert!(left.warnings.is_empty());
        left.document.validate().unwrap();
    }

    #[test]
    fn invalid_comment_body_update_degrades_to_warning() {
        let mut base = Document::new("Doc");
        base.blocks.push(Block::paragraph("hello"));
        let thread_id = StableId::new("comment-thread");
        let comment_id = StableId::new("comment");
        base.comments.push(CommentThread {
            id: thread_id.clone(),
            anchor: Anchor::Document,
            comments: vec![Comment {
                id: comment_id.clone(),
                author: "Alice".to_string(),
                body: vec![Inline::text("old note")],
                created_at_ms: 1,
                deleted: false,
            }],
            deleted: false,
        });

        let result = merge_operations(
            &base,
            &[vec![
                Operation {
                    id: OperationId {
                        actor: ActorId("a".to_string()),
                        seq: 1,
                    },
                    kind: OperationKind::UpdateCommentBody {
                        thread_id: thread_id.clone(),
                        comment_id: comment_id.clone(),
                        body: Vec::new(),
                    },
                },
                Operation {
                    id: OperationId {
                        actor: ActorId("a".to_string()),
                        seq: 2,
                    },
                    kind: OperationKind::UpdateCommentBody {
                        thread_id: thread_id.clone(),
                        comment_id: comment_id.clone(),
                        body: vec![Inline::text(" ")],
                    },
                },
                Operation {
                    id: OperationId {
                        actor: ActorId("a".to_string()),
                        seq: 3,
                    },
                    kind: OperationKind::UpdateCommentBody {
                        thread_id,
                        comment_id,
                        body: vec![Inline::Link {
                            id: StableId::parse("comment-link-empty").unwrap(),
                            text: "bad link".to_string(),
                            href: String::new(),
                            marks: Vec::new(),
                        }],
                    },
                },
            ]],
        )
        .unwrap();

        match &result.document.comments[0].comments[0].body[0] {
            Inline::Text { text, .. } => assert_eq!(text, "old note"),
            _ => unreachable!(),
        }
        assert_eq!(
            result
                .warnings
                .iter()
                .map(|warning| warning.code.as_str())
                .collect::<Vec<_>>(),
            vec!["invalid-comment", "invalid-comment", "invalid-link-href"]
        );
        result.document.validate().unwrap();
    }

    #[test]
    fn block_delete_repairs_nearest_block_comment_anchor() {
        let mut base = Document::new("Doc");
        let target = Block::paragraph("delete target");
        let target_id = target.id.clone();
        let survivor = Block::paragraph("survivor");
        let survivor_id = survivor.id.clone();
        base.blocks.push(target);
        base.blocks.push(survivor);
        base.comments.push(CommentThread {
            id: StableId::parse("comment-thread-block-delete").unwrap(),
            anchor: Anchor::NearestBlock {
                block_id: target_id.clone(),
                warning: "nearest block anchor".to_string(),
            },
            comments: vec![Comment {
                id: StableId::parse("comment-block-delete").unwrap(),
                author: "Alice".to_string(),
                body: vec![Inline::text("note")],
                created_at_ms: 1,
                deleted: false,
            }],
            deleted: false,
        });

        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::DeleteBlock {
                    block_id: target_id,
                },
            }]],
        )
        .unwrap();

        assert_eq!(result.document.blocks.len(), 1);
        assert_eq!(result.document.blocks[0].id, survivor_id);
        assert_eq!(result.warnings[0].code, "comment-anchor-degraded");
        assert!(matches!(
            &result.document.comments[0].anchor,
            Anchor::NearestBlock { block_id, .. } if block_id == &survivor_id
        ));
        assert!(result.document.validate().is_ok());
    }

    #[test]
    fn concurrent_comment_add_and_anchor_delete_repairs_after_replay() {
        let mut base = Document::new("Doc");
        let block = Block::paragraph("hello");
        let text_id = match &block.content[0] {
            Inline::Text { id, .. } => id.clone(),
            _ => unreachable!(),
        };
        base.blocks.push(block);
        let thread = CommentThread {
            id: StableId::new("comment-thread"),
            anchor: Anchor::TextRange(TextRange {
                start: text_id.clone(),
                end: text_id.clone(),
            }),
            comments: vec![Comment {
                id: StableId::new("comment"),
                author: "Alice".to_string(),
                body: vec![Inline::text("note")],
                created_at_ms: 1,
                deleted: false,
            }],
            deleted: false,
        };
        let add = Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::AddCommentThread { thread },
        };
        let delete = Operation {
            id: OperationId {
                actor: ActorId("z".to_string()),
                seq: 1,
            },
            kind: OperationKind::DeleteInline { inline_id: text_id },
        };

        let result_add_first =
            merge_operations(&base, &[vec![add.clone()], vec![delete.clone()]]).unwrap();
        let result_delete_first = merge_operations(&base, &[vec![delete], vec![add]]).unwrap();
        assert_eq!(result_add_first.document, result_delete_first.document);
        assert_eq!(
            result_add_first
                .warnings
                .iter()
                .filter(|warning| warning.code == "comment-anchor-degraded")
                .count(),
            1
        );
        assert!(matches!(
            result_add_first.document.comments[0].anchor,
            Anchor::NearestBlock { .. }
        ));
    }

    #[test]
    fn duplicate_comment_thread_add_degrades_to_warning() {
        let mut base = Document::new("Doc");
        let block = Block::paragraph("hello");
        let text_id = match &block.content[0] {
            Inline::Text { id, .. } => id.clone(),
            _ => unreachable!(),
        };
        base.blocks.push(block);
        let thread_id = StableId::parse("comment-thread-duplicate-merge").unwrap();
        let comment_thread = |comment_id: &str, body: &str| CommentThread {
            id: thread_id.clone(),
            anchor: Anchor::TextRange(TextRange {
                start: text_id.clone(),
                end: text_id.clone(),
            }),
            comments: vec![Comment {
                id: StableId::parse(comment_id).unwrap(),
                author: "Alice".to_string(),
                body: vec![Inline::text(body)],
                created_at_ms: 1,
                deleted: false,
            }],
            deleted: false,
        };
        let first = Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::AddCommentThread {
                thread: comment_thread("comment-duplicate-merge-a", "first"),
            },
        };
        let second = Operation {
            id: OperationId {
                actor: ActorId("b".to_string()),
                seq: 1,
            },
            kind: OperationKind::AddCommentThread {
                thread: comment_thread("comment-duplicate-merge-b", "second"),
            },
        };

        let result = merge_operations(&base, &[vec![first], vec![second]]).unwrap();
        assert_eq!(result.document.comments.len(), 1);
        assert!(result
            .warnings
            .iter()
            .any(|warning| warning.code == "duplicate-comment-thread"));
        result.document.validate().unwrap();
    }

    #[test]
    fn invalid_comment_thread_add_degrades_to_warning() {
        let mut base = Document::new("Doc");
        base.blocks.push(Block::paragraph("hello"));
        let invalid_thread = CommentThread {
            id: StableId::parse("comment-thread-invalid-merge").unwrap(),
            anchor: Anchor::Document,
            comments: Vec::new(),
            deleted: false,
        };
        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::AddCommentThread {
                    thread: invalid_thread,
                },
            }]],
        )
        .unwrap();

        assert!(result.document.comments.is_empty());
        assert!(result
            .warnings
            .iter()
            .any(|warning| warning.code == "invalid-comment-thread"));
        result.document.validate().unwrap();
    }

    #[test]
    fn whitespace_comment_thread_add_degrades_to_warning() {
        let mut base = Document::new("Doc");
        base.blocks.push(Block::paragraph("hello"));
        let invalid_thread = CommentThread {
            id: StableId::parse("comment-thread-whitespace-merge").unwrap(),
            anchor: Anchor::Document,
            comments: vec![Comment {
                id: StableId::parse("comment-whitespace-merge").unwrap(),
                author: "Alice".to_string(),
                body: vec![Inline::text(" ")],
                created_at_ms: 1,
                deleted: false,
            }],
            deleted: false,
        };
        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::AddCommentThread {
                    thread: invalid_thread,
                },
            }]],
        )
        .unwrap();

        assert!(result.document.comments.is_empty());
        assert_eq!(result.warnings[0].code, "invalid-comment-thread");
        result.document.validate().unwrap();
    }

    #[test]
    fn suggestions_and_atomic_equations_converge() {
        let mut base = Document::new("Doc");
        let block = Block::paragraph("hello");
        let block_id = block.id.clone();
        base.blocks.push(block);
        let equation = Inline::Equation {
            id: StableId::new("eq-inline"),
            equation: Equation {
                id: StableId::new("eq"),
                source_format: EquationSourceFormat::LatexLike,
                source: "E=mc^2".to_string(),
            },
        };
        let suggestion_id = StableId::new("suggestion");
        let suggestion = Suggestion {
            id: suggestion_id.clone(),
            author: "Bob".to_string(),
            kind: SuggestionKind::Format {
                range: TextRange {
                    start: StableId::parse("x").unwrap(),
                    end: StableId::parse("y").unwrap(),
                },
                marks: vec![Mark {
                    kind: MarkKind::Italic,
                    value: None,
                    expand: MarkExpand::Both,
                }],
            },
            state: SuggestionState::Proposed,
            provenance: Vec::new(),
        };
        let ops = vec![
            Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::InsertInline {
                    block_id,
                    after: None,
                    inline: equation,
                },
            },
            Operation {
                id: OperationId {
                    actor: ActorId("b".to_string()),
                    seq: 1,
                },
                kind: OperationKind::AddSuggestion { suggestion },
            },
            Operation {
                id: OperationId {
                    actor: ActorId("c".to_string()),
                    seq: 1,
                },
                kind: OperationKind::AcceptSuggestion {
                    suggestion_id,
                    accepted_by: "Carol".to_string(),
                },
            },
        ];
        let result = merge_operations(&base, &[ops]).unwrap();
        assert!(result.document.visible_text().contains("E=mc^2"));
        assert_eq!(
            result.document.suggestions[0].state,
            SuggestionState::Accepted
        );
        assert_eq!(
            result.document.suggestions[0].provenance,
            vec!["accepted-by:Carol"]
        );
    }

    #[test]
    fn duplicate_suggestion_add_degrades_to_warning() {
        let mut base = Document::new("Doc");
        base.blocks.push(Block::paragraph("hello"));
        let suggestion_id = StableId::parse("suggestion-duplicate-merge").unwrap();
        let suggestion = |author: &str, text: &str| Suggestion {
            id: suggestion_id.clone(),
            author: author.to_string(),
            kind: SuggestionKind::Insert {
                anchor: Anchor::Document,
                content: vec![Inline::text(text)],
            },
            state: SuggestionState::Proposed,
            provenance: Vec::new(),
        };
        let first = Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::AddSuggestion {
                suggestion: suggestion("Alice", "first"),
            },
        };
        let second = Operation {
            id: OperationId {
                actor: ActorId("b".to_string()),
                seq: 1,
            },
            kind: OperationKind::AddSuggestion {
                suggestion: suggestion("Bob", "second"),
            },
        };

        let result = merge_operations(&base, &[vec![first], vec![second]]).unwrap();
        assert_eq!(result.document.suggestions.len(), 1);
        assert!(result
            .warnings
            .iter()
            .any(|warning| warning.code == "duplicate-suggestion"));
        result.document.validate().unwrap();
    }

    #[test]
    fn accepting_insert_suggestion_applies_content_after_anchor() {
        let mut base = Document::new("Doc");
        let block = Block::paragraph("hello");
        let text_id = match &block.content[0] {
            Inline::Text { id, .. } => id.clone(),
            _ => unreachable!(),
        };
        base.blocks.push(block);
        let suggestion_id = StableId::parse("suggestion-insert-accept").unwrap();
        let inserted = Inline::Text {
            id: StableId::parse("text-accepted-insert").unwrap(),
            text: " accepted".to_string(),
            marks: Vec::new(),
        };

        let result = merge_operations(
            &base,
            &[vec![
                Operation {
                    id: OperationId {
                        actor: ActorId("a".to_string()),
                        seq: 1,
                    },
                    kind: OperationKind::AddSuggestion {
                        suggestion: Suggestion {
                            id: suggestion_id.clone(),
                            author: "Bob".to_string(),
                            kind: SuggestionKind::Insert {
                                anchor: Anchor::TextRange(TextRange {
                                    start: text_id.clone(),
                                    end: text_id,
                                }),
                                content: vec![inserted],
                            },
                            state: SuggestionState::Proposed,
                            provenance: Vec::new(),
                        },
                    },
                },
                Operation {
                    id: OperationId {
                        actor: ActorId("a".to_string()),
                        seq: 2,
                    },
                    kind: OperationKind::AcceptSuggestion {
                        suggestion_id: suggestion_id.clone(),
                        accepted_by: "Alice".to_string(),
                    },
                },
            ]],
        )
        .unwrap();

        assert_eq!(result.document.visible_text(), "hello accepted\n");
        assert_eq!(
            result.document.suggestions[0].state,
            SuggestionState::Accepted
        );
        assert_eq!(
            result.document.suggestions[0].provenance,
            vec!["accepted-by:Alice"]
        );
    }

    #[test]
    fn concurrent_suggestion_add_and_accept_converge_when_accept_sorts_first() {
        let mut base = Document::new("Doc");
        base.blocks.push(Block::paragraph("hello"));
        let suggestion_id = StableId::parse("suggestion-add-accept-race").unwrap();
        let suggestion = Suggestion {
            id: suggestion_id.clone(),
            author: "Bob".to_string(),
            kind: SuggestionKind::Insert {
                anchor: Anchor::Document,
                content: vec![Inline::text(" accepted proposal")],
            },
            state: SuggestionState::Proposed,
            provenance: Vec::new(),
        };
        let accept = Operation {
            id: OperationId {
                actor: ActorId("actor-a".to_string()),
                seq: 1,
            },
            kind: OperationKind::AcceptSuggestion {
                suggestion_id: suggestion_id.clone(),
                accepted_by: "Alice".to_string(),
            },
        };
        let add = Operation {
            id: OperationId {
                actor: ActorId("actor-z".to_string()),
                seq: 1,
            },
            kind: OperationKind::AddSuggestion { suggestion },
        };

        let accept_first =
            merge_operations(&base, &[vec![accept.clone()], vec![add.clone()]]).unwrap();
        let add_first = merge_operations(&base, &[vec![add], vec![accept]]).unwrap();

        assert_eq!(accept_first.document, add_first.document);
        assert_eq!(accept_first.warnings, add_first.warnings);
        assert_eq!(
            accept_first.document.suggestions[0].state,
            SuggestionState::Accepted
        );
        assert_eq!(
            accept_first.document.suggestions[0].provenance,
            vec!["accepted-by:Alice"]
        );
        assert!(accept_first
            .document
            .visible_text()
            .contains("accepted proposal"));
        assert!(accept_first.warnings.is_empty());
        accept_first.document.validate().unwrap();
    }

    #[test]
    fn accepting_insert_suggestion_with_deleted_range_end_degrades_to_start() {
        let mut base = Document::new("Doc");
        let mut block = Block::paragraph("");
        block.content.clear();
        let start = Inline::text("start");
        let start_id = inline_id(&start).clone();
        let end = Inline::text(" end");
        let end_id = inline_id(&end).clone();
        block.content.push(start);
        block.content.push(end);
        base.blocks.push(block);

        let suggestion_id = StableId::parse("suggestion-insert-partial-anchor").unwrap();
        base.suggestions.push(Suggestion {
            id: suggestion_id.clone(),
            author: "Reviewer".to_string(),
            kind: SuggestionKind::Insert {
                anchor: Anchor::TextRange(TextRange {
                    start: start_id,
                    end: end_id.clone(),
                }),
                content: vec![Inline::text(" inserted")],
            },
            state: SuggestionState::Proposed,
            provenance: Vec::new(),
        });

        let delete_end = Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::DeleteInline { inline_id: end_id },
        };
        let accept = Operation {
            id: OperationId {
                actor: ActorId("b".to_string()),
                seq: 1,
            },
            kind: OperationKind::AcceptSuggestion {
                suggestion_id,
                accepted_by: "Alice".to_string(),
            },
        };

        let actor_streams =
            merge_operations(&base, &[vec![delete_end.clone()], vec![accept.clone()]]).unwrap();
        let storage_batch =
            merge_operations(&base, &[vec![delete_end.clone(), accept.clone()], vec![]]).unwrap();
        let reversed_batches = merge_operations(&base, &[vec![accept], vec![delete_end]]).unwrap();

        assert_eq!(actor_streams.document, storage_batch.document);
        assert_eq!(actor_streams.document, reversed_batches.document);
        assert_eq!(actor_streams.document.visible_text(), "start inserted\n");
        assert_eq!(
            actor_streams.document.suggestions[0].state,
            SuggestionState::Accepted
        );
        assert!(actor_streams
            .warnings
            .iter()
            .any(|warning| warning.code == "suggestion-anchor-degraded"));
        actor_streams.document.validate().unwrap();
    }

    #[test]
    fn accepting_invalid_insert_suggestion_degrades_without_mutating_source() {
        let mut base = Document::new("Doc");
        base.blocks.push(Block::paragraph("hello"));
        let suggestion_id = StableId::parse("suggestion-insert-invalid-accept").unwrap();
        base.suggestions.push(Suggestion {
            id: suggestion_id.clone(),
            author: "Bob".to_string(),
            kind: SuggestionKind::Insert {
                anchor: Anchor::Document,
                content: vec![Inline::Link {
                    id: StableId::parse("invalid-accepted-link").unwrap(),
                    text: "bad link".to_string(),
                    href: String::new(),
                    marks: Vec::new(),
                }],
            },
            state: SuggestionState::Proposed,
            provenance: Vec::new(),
        });

        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::AcceptSuggestion {
                    suggestion_id: suggestion_id.clone(),
                    accepted_by: "Alice".to_string(),
                },
            }]],
        )
        .unwrap();

        assert_eq!(result.document.visible_text(), "hello\n");
        assert_eq!(result.warnings[0].code, "invalid-link-href");
        assert_eq!(
            result.document.suggestions[0].state,
            SuggestionState::Rejected
        );
        assert_eq!(
            result.document.suggestions[0].provenance,
            vec!["auto-rejected:invalid-accept-payload"]
        );
        result.document.validate().unwrap();
    }

    #[test]
    fn accepting_delete_suggestion_removes_source_range() {
        let mut base = Document::new("Doc");
        base.blocks.push(Block::paragraph("alpha "));
        let second = Inline::Text {
            id: StableId::parse("text-delete-target").unwrap(),
            text: "beta".to_string(),
            marks: Vec::new(),
        };
        let second_id = inline_id(&second).clone();
        base.blocks[0].content.push(second);
        let suggestion_id = StableId::parse("suggestion-delete-accept").unwrap();
        base.suggestions.push(Suggestion {
            id: suggestion_id.clone(),
            author: "Bob".to_string(),
            kind: SuggestionKind::Delete {
                range: TextRange {
                    start: second_id.clone(),
                    end: second_id,
                },
            },
            state: SuggestionState::Proposed,
            provenance: Vec::new(),
        });

        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::AcceptSuggestion {
                    suggestion_id,
                    accepted_by: "Alice".to_string(),
                },
            }]],
        )
        .unwrap();

        assert_eq!(result.document.visible_text(), "alpha \n");
        assert_eq!(
            result.document.suggestions[0].state,
            SuggestionState::Accepted
        );
    }

    #[test]
    fn accepting_format_suggestion_applies_range_marks() {
        let mut base = Document::new("Doc");
        base.blocks.push(Block::paragraph("alpha"));
        let text_id = match &base.blocks[0].content[0] {
            Inline::Text { id, .. } => id.clone(),
            _ => unreachable!(),
        };
        let suggestion_id = StableId::parse("suggestion-format-accept").unwrap();
        base.suggestions.push(Suggestion {
            id: suggestion_id.clone(),
            author: "Bob".to_string(),
            kind: SuggestionKind::Format {
                range: TextRange {
                    start: text_id.clone(),
                    end: text_id,
                },
                marks: vec![Mark {
                    kind: MarkKind::Bold,
                    value: None,
                    expand: MarkExpand::Both,
                }],
            },
            state: SuggestionState::Proposed,
            provenance: Vec::new(),
        });

        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::AcceptSuggestion {
                    suggestion_id,
                    accepted_by: "Alice".to_string(),
                },
            }]],
        )
        .unwrap();

        match &result.document.blocks[0].content[0] {
            Inline::Text { marks, .. } => {
                assert!(marks.iter().any(|mark| mark.kind == MarkKind::Bold));
            }
            _ => unreachable!(),
        }
        assert_eq!(
            result.document.suggestions[0].state,
            SuggestionState::Accepted
        );
    }

    #[test]
    fn accepting_invalid_format_suggestion_degrades_without_mutating_marks() {
        let mut base = Document::new("Doc");
        base.blocks.push(Block::paragraph("alpha"));
        let text_id = inline_id(&base.blocks[0].content[0]).clone();
        let suggestion_id = StableId::parse("suggestion-format-invalid-accept").unwrap();
        base.suggestions.push(Suggestion {
            id: suggestion_id.clone(),
            author: "Bob".to_string(),
            kind: SuggestionKind::Format {
                range: TextRange {
                    start: text_id.clone(),
                    end: text_id,
                },
                marks: vec![Mark {
                    kind: MarkKind::Color,
                    value: None,
                    expand: MarkExpand::Both,
                }],
            },
            state: SuggestionState::Proposed,
            provenance: Vec::new(),
        });

        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::AcceptSuggestion {
                    suggestion_id: suggestion_id.clone(),
                    accepted_by: "Alice".to_string(),
                },
            }]],
        )
        .unwrap();

        match &result.document.blocks[0].content[0] {
            Inline::Text { marks, .. } => assert!(marks.is_empty()),
            _ => unreachable!(),
        }
        assert_eq!(result.warnings[0].code, "invalid-mark-value");
        assert_eq!(
            result.document.suggestions[0].state,
            SuggestionState::Rejected
        );
        assert_eq!(
            result.document.suggestions[0].provenance,
            vec!["auto-rejected:invalid-accept-payload"]
        );
        result.document.validate().unwrap();
    }

    #[test]
    fn suggestion_rejection_is_recorded_as_provenance() {
        let mut base = Document::new("Doc");
        base.blocks.push(Block::paragraph("hello"));
        let suggestion_id = StableId::new("suggestion");
        let suggestion = Suggestion {
            id: suggestion_id.clone(),
            author: "Bob".to_string(),
            kind: SuggestionKind::Insert {
                anchor: Anchor::Document,
                content: vec![Inline::text("nope")],
            },
            state: SuggestionState::Proposed,
            provenance: Vec::new(),
        };

        let result = merge_operations(
            &base,
            &[vec![
                Operation {
                    id: OperationId {
                        actor: ActorId("a".to_string()),
                        seq: 1,
                    },
                    kind: OperationKind::AddSuggestion { suggestion },
                },
                Operation {
                    id: OperationId {
                        actor: ActorId("a".to_string()),
                        seq: 2,
                    },
                    kind: OperationKind::RejectSuggestion {
                        suggestion_id,
                        rejected_by: "Carol".to_string(),
                    },
                },
            ]],
        )
        .unwrap();

        assert_eq!(
            result.document.suggestions[0].state,
            SuggestionState::Rejected
        );
        assert_eq!(
            result.document.suggestions[0].provenance,
            vec!["rejected-by:Carol"]
        );
    }

    #[test]
    fn malformed_suggestion_resolution_reviewer_degrades_to_valid_provenance() {
        let mut accept_base = Document::new("Doc");
        accept_base.blocks.push(Block::paragraph("hello"));
        let accept_id = StableId::parse("suggestion-accept-reviewer").unwrap();
        accept_base.suggestions.push(Suggestion {
            id: accept_id.clone(),
            author: "Bob".to_string(),
            kind: SuggestionKind::Insert {
                anchor: Anchor::Document,
                content: vec![Inline::text(" accepted")],
            },
            state: SuggestionState::Proposed,
            provenance: Vec::new(),
        });
        let accepted = merge_operations(
            &accept_base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::AcceptSuggestion {
                    suggestion_id: accept_id,
                    accepted_by: " Alice ".to_string(),
                },
            }]],
        )
        .unwrap();
        assert_eq!(
            accepted.document.suggestions[0].state,
            SuggestionState::Accepted
        );
        assert_eq!(
            accepted.document.suggestions[0].provenance,
            vec!["accepted-by:Alice"]
        );
        accepted.document.validate().unwrap();

        let mut reject_base = Document::new("Doc");
        reject_base.blocks.push(Block::paragraph("hello"));
        let reject_id = StableId::parse("suggestion-reject-reviewer").unwrap();
        reject_base.suggestions.push(Suggestion {
            id: reject_id.clone(),
            author: "Bob".to_string(),
            kind: SuggestionKind::Insert {
                anchor: Anchor::Document,
                content: vec![Inline::text(" rejected")],
            },
            state: SuggestionState::Proposed,
            provenance: Vec::new(),
        });
        let rejected = merge_operations(
            &reject_base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("b".to_string()),
                    seq: 1,
                },
                kind: OperationKind::RejectSuggestion {
                    suggestion_id: reject_id,
                    rejected_by: " ".to_string(),
                },
            }]],
        )
        .unwrap();
        assert_eq!(
            rejected.document.suggestions[0].state,
            SuggestionState::Rejected
        );
        assert_eq!(
            rejected.document.suggestions[0].provenance,
            vec!["rejected-by:unknown"]
        );
        assert!(rejected
            .warnings
            .iter()
            .any(|warning| warning.code == "invalid-suggestion-reviewer"));
        rejected.document.validate().unwrap();
    }

    #[test]
    fn rejecting_resolved_suggestion_degrades_to_warning_without_state_change() {
        let mut base = Document::new("Doc");
        base.blocks.push(Block::paragraph("hello"));
        let suggestion_id = StableId::parse("suggestion-already-accepted").unwrap();
        base.suggestions.push(Suggestion {
            id: suggestion_id.clone(),
            author: "Bob".to_string(),
            kind: SuggestionKind::Insert {
                anchor: Anchor::Document,
                content: vec![Inline::text("accepted text")],
            },
            state: SuggestionState::Accepted,
            provenance: vec!["accepted-by:Alice".to_string()],
        });

        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::RejectSuggestion {
                    suggestion_id,
                    rejected_by: "Carol".to_string(),
                },
            }]],
        )
        .unwrap();

        assert_eq!(
            result.document.suggestions[0].state,
            SuggestionState::Accepted
        );
        assert_eq!(
            result.document.suggestions[0].provenance,
            vec!["accepted-by:Alice"]
        );
        assert!(result
            .warnings
            .iter()
            .any(|warning| warning.code == "resolved-suggestion"));
        result.document.validate().unwrap();
    }

    #[test]
    fn concurrent_accept_and_reject_suggestion_converges_without_manual_conflict() {
        let mut base = Document::new("Doc");
        base.blocks.push(Block::paragraph("hello"));
        let suggestion_id = StableId::parse("suggestion-review-race").unwrap();
        base.suggestions.push(Suggestion {
            id: suggestion_id.clone(),
            author: "Bob".to_string(),
            kind: SuggestionKind::Insert {
                anchor: Anchor::Document,
                content: vec![Inline::text(" suggestion")],
            },
            state: SuggestionState::Proposed,
            provenance: Vec::new(),
        });
        let reject = Operation {
            id: OperationId {
                actor: ActorId("actor-a".to_string()),
                seq: 1,
            },
            kind: OperationKind::RejectSuggestion {
                suggestion_id: suggestion_id.clone(),
                rejected_by: "Carol".to_string(),
            },
        };
        let accept = Operation {
            id: OperationId {
                actor: ActorId("actor-b".to_string()),
                seq: 1,
            },
            kind: OperationKind::AcceptSuggestion {
                suggestion_id: suggestion_id.clone(),
                accepted_by: "Alice".to_string(),
            },
        };

        let actor_streams =
            merge_operations(&base, &[vec![reject.clone()], vec![accept.clone()]]).unwrap();
        let storage_batch =
            merge_operations(&base, &[vec![reject.clone(), accept.clone()], vec![]]).unwrap();
        let reversed_batches = merge_operations(&base, &[vec![accept], vec![reject]]).unwrap();

        assert_eq!(actor_streams.document, storage_batch.document);
        assert_eq!(actor_streams.document, reversed_batches.document);
        assert_eq!(actor_streams.warnings, storage_batch.warnings);
        assert_eq!(actor_streams.warnings, reversed_batches.warnings);
        assert_eq!(
            actor_streams.document.suggestions[0].state,
            SuggestionState::Rejected
        );
        assert_eq!(
            actor_streams.document.suggestions[0].provenance,
            vec!["rejected-by:Carol"]
        );
        assert!(!actor_streams.document.visible_text().contains("suggestion"));
        assert!(actor_streams
            .warnings
            .iter()
            .any(|warning| warning.code == "resolved-suggestion"));
        actor_streams.document.validate().unwrap();
    }

    #[test]
    fn concurrent_suggestion_resolution_beats_stale_content_update() {
        let mut base = Document::new("Doc");
        base.blocks.push(Block::paragraph("hello"));
        let suggestion_id = StableId::parse("suggestion-stale-update-race").unwrap();
        base.suggestions.push(Suggestion {
            id: suggestion_id.clone(),
            author: "Bob".to_string(),
            kind: SuggestionKind::Insert {
                anchor: Anchor::Document,
                content: vec![Inline::text(" original proposal")],
            },
            state: SuggestionState::Proposed,
            provenance: Vec::new(),
        });
        let stale_update = Operation {
            id: OperationId {
                actor: ActorId("actor-a".to_string()),
                seq: 1,
            },
            kind: OperationKind::UpdateSuggestionInsertContent {
                suggestion_id: suggestion_id.clone(),
                content: vec![Inline::text(" stale rewrite")],
            },
        };
        let accept = Operation {
            id: OperationId {
                actor: ActorId("actor-b".to_string()),
                seq: 1,
            },
            kind: OperationKind::AcceptSuggestion {
                suggestion_id: suggestion_id.clone(),
                accepted_by: "Alice".to_string(),
            },
        };

        let actor_streams =
            merge_operations(&base, &[vec![stale_update.clone()], vec![accept.clone()]]).unwrap();
        let reversed_batches =
            merge_operations(&base, &[vec![accept.clone()], vec![stale_update.clone()]]).unwrap();
        let storage_batch = merge_operations(&base, &[vec![stale_update, accept]]).unwrap();

        assert_eq!(actor_streams.document, reversed_batches.document);
        assert_eq!(actor_streams.document, storage_batch.document);
        assert_eq!(actor_streams.warnings, reversed_batches.warnings);
        assert_eq!(actor_streams.warnings, storage_batch.warnings);
        assert_eq!(
            actor_streams.document.suggestions[0].state,
            SuggestionState::Accepted
        );
        match &actor_streams.document.suggestions[0].kind {
            SuggestionKind::Insert { content, .. } => match &content[0] {
                Inline::Text { text, .. } => assert_eq!(text, " original proposal"),
                _ => unreachable!(),
            },
            _ => unreachable!(),
        }
        assert!(actor_streams
            .document
            .visible_text()
            .contains("original proposal"));
        assert!(!actor_streams
            .document
            .visible_text()
            .contains("stale rewrite"));
        assert!(actor_streams
            .warnings
            .iter()
            .any(|warning| warning.code == "stale-suggestion-update"));
        actor_streams.document.validate().unwrap();
    }

    #[test]
    fn insert_suggestion_content_updates_by_stable_id() {
        let mut base = Document::new("Doc");
        base.blocks.push(Block::paragraph("hello"));
        let suggestion_id = StableId::new("suggestion");
        base.suggestions.push(Suggestion {
            id: suggestion_id.clone(),
            author: "Alice".to_string(),
            kind: SuggestionKind::Insert {
                anchor: Anchor::Document,
                content: vec![Inline::text("old proposal")],
            },
            state: SuggestionState::Proposed,
            provenance: Vec::new(),
        });

        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::UpdateSuggestionInsertContent {
                    suggestion_id,
                    content: vec![Inline::text("new proposal")],
                },
            }]],
        )
        .unwrap();

        match &result.document.suggestions[0].kind {
            SuggestionKind::Insert { content, .. } => match &content[0] {
                Inline::Text { text, .. } => assert_eq!(text, "new proposal"),
                _ => unreachable!(),
            },
            _ => unreachable!(),
        }
    }

    #[test]
    fn invalid_insert_suggestion_content_update_degrades_to_warning() {
        let mut base = Document::new("Doc");
        base.blocks.push(Block::paragraph("hello"));
        let suggestion_id = StableId::new("suggestion");
        base.suggestions.push(Suggestion {
            id: suggestion_id.clone(),
            author: "Alice".to_string(),
            kind: SuggestionKind::Insert {
                anchor: Anchor::Document,
                content: vec![Inline::text("old proposal")],
            },
            state: SuggestionState::Proposed,
            provenance: Vec::new(),
        });

        let result = merge_operations(
            &base,
            &[vec![
                Operation {
                    id: OperationId {
                        actor: ActorId("a".to_string()),
                        seq: 1,
                    },
                    kind: OperationKind::UpdateSuggestionInsertContent {
                        suggestion_id: suggestion_id.clone(),
                        content: Vec::new(),
                    },
                },
                Operation {
                    id: OperationId {
                        actor: ActorId("a".to_string()),
                        seq: 2,
                    },
                    kind: OperationKind::UpdateSuggestionInsertContent {
                        suggestion_id: suggestion_id.clone(),
                        content: vec![Inline::text(" ")],
                    },
                },
                Operation {
                    id: OperationId {
                        actor: ActorId("a".to_string()),
                        seq: 3,
                    },
                    kind: OperationKind::UpdateSuggestionInsertContent {
                        suggestion_id,
                        content: vec![Inline::Equation {
                            id: StableId::parse("suggestion-equation-empty").unwrap(),
                            equation: Equation {
                                id: StableId::parse("suggestion-equation").unwrap(),
                                source_format: EquationSourceFormat::LatexLike,
                                source: String::new(),
                            },
                        }],
                    },
                },
            ]],
        )
        .unwrap();

        match &result.document.suggestions[0].kind {
            SuggestionKind::Insert { content, .. } => match &content[0] {
                Inline::Text { text, .. } => assert_eq!(text, "old proposal"),
                _ => unreachable!(),
            },
            _ => unreachable!(),
        }
        assert_eq!(
            result
                .warnings
                .iter()
                .map(|warning| warning.code.as_str())
                .collect::<Vec<_>>(),
            vec![
                "invalid-suggestion",
                "invalid-suggestion",
                "invalid-inline-equation-source"
            ]
        );
        result.document.validate().unwrap();
    }

    #[test]
    fn invalid_suggestion_operations_degrade_to_warning() {
        let base = Document::new("Doc");
        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::AddSuggestion {
                    suggestion: Suggestion {
                        id: StableId::parse("suggestion-empty").unwrap(),
                        author: "Reviewer".to_string(),
                        kind: SuggestionKind::Insert {
                            anchor: Anchor::Document,
                            content: Vec::new(),
                        },
                        state: SuggestionState::Proposed,
                        provenance: Vec::new(),
                    },
                },
            }]],
        )
        .unwrap();

        assert!(result.document.suggestions.is_empty());
        assert_eq!(result.warnings[0].code, "invalid-suggestion");
        assert!(result.document.validate().is_ok());
    }

    #[test]
    fn whitespace_insert_suggestion_add_degrades_to_warning() {
        let base = Document::new("Doc");
        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::AddSuggestion {
                    suggestion: Suggestion {
                        id: StableId::parse("suggestion-whitespace").unwrap(),
                        author: "Reviewer".to_string(),
                        kind: SuggestionKind::Insert {
                            anchor: Anchor::Document,
                            content: vec![Inline::text(" ")],
                        },
                        state: SuggestionState::Proposed,
                        provenance: Vec::new(),
                    },
                },
            }]],
        )
        .unwrap();

        assert!(result.document.suggestions.is_empty());
        assert_eq!(result.warnings[0].code, "invalid-suggestion");
        assert!(result.document.validate().is_ok());
    }

    #[test]
    fn insert_suggestion_anchor_repairs_after_target_text_delete() {
        let mut base = Document::new("Doc");
        let block = Block::paragraph("hello");
        let text_id = match &block.content[0] {
            Inline::Text { id, .. } => id.clone(),
            _ => unreachable!(),
        };
        base.blocks.push(block);
        let suggestion_id = StableId::new("suggestion");
        base.suggestions.push(Suggestion {
            id: suggestion_id.clone(),
            author: "Bob".to_string(),
            kind: SuggestionKind::Insert {
                anchor: Anchor::TextRange(TextRange {
                    start: text_id.clone(),
                    end: text_id.clone(),
                }),
                content: vec![Inline::text("insert")],
            },
            state: SuggestionState::Proposed,
            provenance: Vec::new(),
        });

        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::DeleteInline { inline_id: text_id },
            }]],
        )
        .unwrap();

        assert_eq!(result.warnings[0].code, "suggestion-anchor-degraded");
        assert!(result.warnings[0].message.contains(suggestion_id.as_str()));
        match &result.document.suggestions[0].kind {
            SuggestionKind::Insert { anchor, .. } => {
                assert!(matches!(anchor, Anchor::NearestBlock { .. }))
            }
            _ => unreachable!(),
        }
        assert_eq!(
            result.document.suggestions[0].provenance,
            vec!["auto-degraded:missing-anchor"]
        );
    }

    #[test]
    fn format_suggestion_range_collapses_to_surviving_endpoint() {
        let mut base = Document::new("Doc");
        let mut block = Block::paragraph("");
        block.content.clear();
        let first = Inline::text("alpha ");
        let first_id = inline_id(&first).clone();
        let second = Inline::text("beta");
        let second_id = inline_id(&second).clone();
        block.content.extend([first, second]);
        base.blocks.push(block);
        let suggestion_id = StableId::new("suggestion");
        base.suggestions.push(Suggestion {
            id: suggestion_id.clone(),
            author: "Bob".to_string(),
            kind: SuggestionKind::Format {
                range: TextRange {
                    start: first_id.clone(),
                    end: second_id.clone(),
                },
                marks: vec![Mark {
                    kind: MarkKind::Bold,
                    value: None,
                    expand: MarkExpand::Both,
                }],
            },
            state: SuggestionState::Proposed,
            provenance: Vec::new(),
        });

        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::DeleteInline {
                    inline_id: first_id,
                },
            }]],
        )
        .unwrap();

        assert_eq!(result.warnings[0].code, "suggestion-range-degraded");
        assert!(result.warnings[0].message.contains(suggestion_id.as_str()));
        match &result.document.suggestions[0].kind {
            SuggestionKind::Format { range, .. } => {
                assert_eq!(range.start, second_id);
                assert_eq!(range.end, second_id);
            }
            _ => unreachable!(),
        }
        assert_eq!(
            result.document.suggestions[0].state,
            SuggestionState::Proposed
        );
        assert_eq!(
            result.document.suggestions[0].provenance,
            vec!["auto-degraded:partial-range"]
        );
    }

    #[test]
    fn delete_suggestion_auto_rejects_when_entire_range_was_deleted() {
        let mut base = Document::new("Doc");
        let mut block = Block::paragraph("");
        block.content.clear();
        let first = Inline::text("alpha ");
        let first_id = inline_id(&first).clone();
        let second = Inline::text("beta");
        let second_id = inline_id(&second).clone();
        block.content.extend([first, second]);
        base.blocks.push(block);
        let suggestion_id = StableId::new("suggestion");
        base.suggestions.push(Suggestion {
            id: suggestion_id.clone(),
            author: "Bob".to_string(),
            kind: SuggestionKind::Delete {
                range: TextRange {
                    start: first_id.clone(),
                    end: second_id.clone(),
                },
            },
            state: SuggestionState::Proposed,
            provenance: Vec::new(),
        });

        let result = merge_operations(
            &base,
            &[vec![
                Operation {
                    id: OperationId {
                        actor: ActorId("a".to_string()),
                        seq: 1,
                    },
                    kind: OperationKind::DeleteInline {
                        inline_id: first_id,
                    },
                },
                Operation {
                    id: OperationId {
                        actor: ActorId("a".to_string()),
                        seq: 2,
                    },
                    kind: OperationKind::DeleteInline {
                        inline_id: second_id,
                    },
                },
            ]],
        )
        .unwrap();

        assert_eq!(result.warnings[0].code, "suggestion-range-missing");
        assert!(result.warnings[0].message.contains(suggestion_id.as_str()));
        assert_eq!(
            result.document.suggestions[0].state,
            SuggestionState::Rejected
        );
        assert_eq!(
            result.document.suggestions[0].provenance,
            vec!["auto-rejected:missing-range"]
        );
    }

    #[test]
    fn citation_labels_reference_document_local_database() {
        let mut base = Document::new("Doc");
        let block = Block::paragraph("cited ");
        let block_id = block.id.clone();
        let after = match &block.content[0] {
            Inline::Text { id, .. } => id.clone(),
            _ => unreachable!(),
        };
        base.blocks.push(block);
        let reference_id = StableId::parse("ref-doe-2020").unwrap();
        let citation_id = StableId::parse("cite-intro").unwrap();

        let reference = BibliographyReference {
            id: reference_id.clone(),
            revision: 1,
            source: CitationSource {
                format: CitationSourceFormat::CitumNative,
                bytes: b"id: doe-2020\ntitle: Example".to_vec(),
            },
            summary: CitationSummary {
                title: "Example".to_string(),
                authors: vec!["Doe".to_string()],
                issued: Some("2020".to_string()),
                doi: None,
                url: None,
            },
            deleted: false,
        };
        let citation = CitationGroup {
            id: citation_id.clone(),
            revision: 1,
            items: vec![CitationItem {
                reference_id,
                locator: Some("42".to_string()),
                label: Some("page".to_string()),
                prefix: Some("see".to_string()),
                suffix: None,
                suppress_author: false,
            }],
            placement: CitationPlacement::Inline,
            rendered_cache: Some("(see Doe 2020, 42)".to_string()),
            deleted: false,
        };
        let label = Inline::Citation {
            id: StableId::new("citation-label"),
            citation_id,
            rendered_cache: None,
        };
        let ops = vec![
            Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::UpsertBibliographyReference { reference },
            },
            Operation {
                id: OperationId {
                    actor: ActorId("b".to_string()),
                    seq: 1,
                },
                kind: OperationKind::UpsertCitationGroup { citation },
            },
            Operation {
                id: OperationId {
                    actor: ActorId("c".to_string()),
                    seq: 1,
                },
                kind: OperationKind::InsertInline {
                    block_id,
                    after: Some(after),
                    inline: label,
                },
            },
        ];
        let result = merge_operations(&base, &[ops]).unwrap();
        assert_eq!(
            result.document.visible_text(),
            "cited (see Doe 2020, page 42)\n"
        );
        assert_eq!(result.document.citation_database.references.len(), 1);
        assert_eq!(result.document.citation_database.citations.len(), 1);
    }

    #[test]
    fn citation_style_updates_replay_as_source_state() {
        let base = Document::new("Doc");
        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::UpdateCitationStyle {
                    style: " ieee ".to_string(),
                    locale: " en-GB ".to_string(),
                },
            }]],
        )
        .unwrap();
        assert_eq!(result.document.citation_database.style, "ieee");
        assert_eq!(result.document.citation_database.locale, "en-GB");

        let mut styled_base = Document::new("Doc");
        styled_base.citation_database.style = "ieee".to_string();
        styled_base.citation_database.locale = "en-GB".to_string();

        let degraded = merge_operations(
            &styled_base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 2,
                },
                kind: OperationKind::UpdateCitationStyle {
                    style: " ".to_string(),
                    locale: " ".to_string(),
                },
            }]],
        )
        .unwrap();
        assert_eq!(degraded.document.citation_database.style, "ieee");
        assert_eq!(degraded.document.citation_database.locale, "en-GB");
        assert!(degraded
            .warnings
            .iter()
            .any(|warning| warning.code == "invalid-citation-style"));
    }

    #[test]
    fn footnote_body_updates_are_revision_ordered_source_state() {
        let mut base = Document::new("Doc");
        let footnote_id = StableId::parse("footnote-1").unwrap();
        base.blocks.push(Block {
            id: StableId::new("block"),
            kind: BlockKind::Paragraph,
            content: vec![Inline::FootnoteRef {
                id: StableId::parse("footnote-ref-1").unwrap(),
                footnote_id: footnote_id.clone(),
            }],
            properties: Vec::new(),
        });
        let old = Footnote {
            id: footnote_id.clone(),
            revision: 1,
            body: vec![Inline::text("old footnote")],
            deleted: false,
        };
        let new = Footnote {
            id: footnote_id,
            revision: 2,
            body: vec![Inline::text("new footnote")],
            deleted: false,
        };
        let result = merge_operations(
            &base,
            &[
                vec![Operation {
                    id: OperationId {
                        actor: ActorId("a".to_string()),
                        seq: 2,
                    },
                    kind: OperationKind::UpsertFootnote { footnote: new },
                }],
                vec![Operation {
                    id: OperationId {
                        actor: ActorId("b".to_string()),
                        seq: 1,
                    },
                    kind: OperationKind::UpsertFootnote { footnote: old },
                }],
            ],
        )
        .unwrap();
        assert_eq!(result.document.footnotes.len(), 1);
        match &result.document.footnotes[0].body[0] {
            Inline::Text { text, .. } => assert_eq!(text, "new footnote"),
            _ => panic!("expected text footnote body"),
        }
    }

    #[test]
    fn whitespace_footnote_body_upsert_degrades_to_warning() {
        let mut base = Document::new("Doc");
        let footnote_id = StableId::parse("footnote-whitespace").unwrap();
        base.footnotes.push(Footnote {
            id: footnote_id.clone(),
            revision: 1,
            body: vec![Inline::text("old footnote")],
            deleted: false,
        });
        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::UpsertFootnote {
                    footnote: Footnote {
                        id: footnote_id,
                        revision: 2,
                        body: vec![Inline::text(" ")],
                        deleted: false,
                    },
                },
            }]],
        )
        .unwrap();

        assert_eq!(result.warnings[0].code, "invalid-footnote");
        match &result.document.footnotes[0].body[0] {
            Inline::Text { text, .. } => assert_eq!(text, "old footnote"),
            _ => panic!("expected text footnote body"),
        }
        result.document.validate().unwrap();
    }

    #[test]
    fn missing_footnote_reference_targets_are_removed_with_warning() {
        let mut base = Document::new("Doc");
        let missing_footnote_id = StableId::parse("missing-footnote").unwrap();
        base.blocks.push(Block {
            id: StableId::parse("block-1").unwrap(),
            kind: BlockKind::Paragraph,
            content: vec![Inline::text("body")],
            properties: Vec::new(),
        });
        base.blocks.push(Block {
            id: StableId::parse("table-block").unwrap(),
            kind: BlockKind::Table {
                rows: vec![TableRow {
                    id: StableId::parse("row-1").unwrap(),
                    cells: vec![TableCell {
                        id: StableId::parse("cell-1").unwrap(),
                        blocks: vec![Block {
                            id: StableId::parse("nested-block").unwrap(),
                            kind: BlockKind::Paragraph,
                            content: vec![
                                Inline::text("nested"),
                                Inline::FootnoteRef {
                                    id: StableId::parse("nested-footnote-ref").unwrap(),
                                    footnote_id: missing_footnote_id.clone(),
                                },
                            ],
                            properties: Vec::new(),
                        }],
                        properties: Vec::new(),
                    }],
                }],
            },
            content: Vec::new(),
            properties: Vec::new(),
        });

        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::InsertInline {
                    block_id: StableId::parse("block-1").unwrap(),
                    after: None,
                    inline: Inline::FootnoteRef {
                        id: StableId::parse("footnote-ref-1").unwrap(),
                        footnote_id: missing_footnote_id,
                    },
                },
            }]],
        )
        .unwrap();

        assert!(result.document.validate().is_ok());
        assert!(!result
            .document
            .blocks
            .iter()
            .flat_map(|block| block.content.iter())
            .any(|inline| matches!(inline, Inline::FootnoteRef { .. })));
        match &result.document.blocks[1].kind {
            BlockKind::Table { rows } => {
                assert!(!rows[0].cells[0].blocks[0]
                    .content
                    .iter()
                    .any(|inline| matches!(inline, Inline::FootnoteRef { .. })));
            }
            _ => panic!("expected table"),
        }
        assert!(result
            .warnings
            .iter()
            .any(|warning| warning.code == "footnote-reference-target-missing"));
    }

    #[test]
    fn footnote_body_update_and_reference_delete_converge_to_deleted_footnote() {
        let mut base = Document::new("Doc");
        let footnote_id = StableId::parse("footnote-1").unwrap();
        let footnote_ref_id = StableId::parse("footnote-ref-1").unwrap();
        base.footnotes.push(Footnote {
            id: footnote_id.clone(),
            revision: 1,
            body: vec![Inline::text("old footnote")],
            deleted: false,
        });
        base.blocks.push(Block {
            id: StableId::new("block"),
            kind: BlockKind::Paragraph,
            content: vec![
                Inline::text("body"),
                Inline::FootnoteRef {
                    id: footnote_ref_id.clone(),
                    footnote_id: footnote_id.clone(),
                },
            ],
            properties: Vec::new(),
        });
        let update = Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::UpsertFootnote {
                footnote: Footnote {
                    id: footnote_id.clone(),
                    revision: 2,
                    body: vec![Inline::text("new footnote")],
                    deleted: false,
                },
            },
        };
        let delete_ref = Operation {
            id: OperationId {
                actor: ActorId("b".to_string()),
                seq: 1,
            },
            kind: OperationKind::DeleteInline {
                inline_id: footnote_ref_id,
            },
        };

        let update_first =
            merge_operations(&base, &[vec![update.clone()], vec![delete_ref.clone()]]).unwrap();
        let delete_first = merge_operations(&base, &[vec![delete_ref], vec![update]]).unwrap();

        assert_eq!(update_first.document, delete_first.document);
        assert!(update_first.document.footnotes[0].deleted);
        match &update_first.document.footnotes[0].body[0] {
            Inline::Text { text, .. } => assert_eq!(text, "new footnote"),
            _ => panic!("expected text footnote body"),
        }
        assert_eq!(update_first.warnings[0].code, "footnote-reference-missing");
        assert!(update_first.document.validate().is_ok());
    }

    #[test]
    fn citation_reference_updates_are_revision_ordered() {
        let base = Document::new("Doc");
        let reference_id = StableId::parse("ref-doe-2020").unwrap();
        let old = BibliographyReference {
            id: reference_id.clone(),
            revision: 1,
            source: CitationSource {
                format: CitationSourceFormat::CitumNative,
                bytes: b"title: Old".to_vec(),
            },
            summary: CitationSummary {
                title: "Old".to_string(),
                authors: vec!["Doe".to_string()],
                issued: Some("2020".to_string()),
                doi: None,
                url: None,
            },
            deleted: false,
        };
        let new = BibliographyReference {
            id: reference_id,
            revision: 2,
            source: CitationSource {
                format: CitationSourceFormat::CitumNative,
                bytes: b"title: New".to_vec(),
            },
            summary: CitationSummary {
                title: "New".to_string(),
                authors: vec!["Doe".to_string()],
                issued: Some("2021".to_string()),
                doi: None,
                url: None,
            },
            deleted: false,
        };
        let result = merge_operations(
            &base,
            &[
                vec![Operation {
                    id: OperationId {
                        actor: ActorId("a".to_string()),
                        seq: 2,
                    },
                    kind: OperationKind::UpsertBibliographyReference { reference: new },
                }],
                vec![Operation {
                    id: OperationId {
                        actor: ActorId("b".to_string()),
                        seq: 1,
                    },
                    kind: OperationKind::UpsertBibliographyReference { reference: old },
                }],
            ],
        )
        .unwrap();
        assert_eq!(
            result.document.citation_database.references[0]
                .summary
                .title,
            "New"
        );
    }

    #[test]
    fn citation_reference_update_commutes_with_anchor_delete() {
        let mut base = Document::new("Doc");
        let reference_id = StableId::parse("ref-doe-2020").unwrap();
        let citation_id = StableId::parse("cite-intro").unwrap();
        base.citation_database
            .upsert_reference(BibliographyReference {
                id: reference_id.clone(),
                revision: 1,
                source: CitationSource {
                    format: CitationSourceFormat::CitumNative,
                    bytes: b"title: Old".to_vec(),
                },
                summary: CitationSummary {
                    title: "Old".to_string(),
                    authors: vec!["Doe".to_string()],
                    issued: Some("2020".to_string()),
                    doi: None,
                    url: None,
                },
                deleted: false,
            });
        base.citation_database.upsert_citation(CitationGroup {
            id: citation_id.clone(),
            revision: 1,
            items: vec![CitationItem {
                reference_id: reference_id.clone(),
                locator: None,
                label: None,
                prefix: None,
                suffix: None,
                suppress_author: false,
            }],
            placement: CitationPlacement::Inline,
            rendered_cache: Some("(Doe 2020)".to_string()),
            deleted: false,
        });
        base.blocks.push(Block {
            id: StableId::new("block"),
            kind: BlockKind::Paragraph,
            content: vec![
                Inline::text("cited "),
                Inline::Citation {
                    id: StableId::parse("citation-label").unwrap(),
                    citation_id,
                    rendered_cache: Some("(Doe 2020)".to_string()),
                },
            ],
            properties: Vec::new(),
        });

        let update = Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::UpsertBibliographyReference {
                reference: BibliographyReference {
                    id: reference_id,
                    revision: 2,
                    source: CitationSource {
                        format: CitationSourceFormat::CitumNative,
                        bytes: b"title: New".to_vec(),
                    },
                    summary: CitationSummary {
                        title: "New".to_string(),
                        authors: vec!["Doe".to_string()],
                        issued: Some("2021".to_string()),
                        doi: None,
                        url: None,
                    },
                    deleted: false,
                },
            },
        };
        let delete_anchor = Operation {
            id: OperationId {
                actor: ActorId("b".to_string()),
                seq: 1,
            },
            kind: OperationKind::DeleteInline {
                inline_id: StableId::parse("citation-label").unwrap(),
            },
        };

        let update_first =
            merge_operations(&base, &[vec![update.clone()], vec![delete_anchor.clone()]]).unwrap();
        let delete_first = merge_operations(&base, &[vec![delete_anchor], vec![update]]).unwrap();

        assert_eq!(update_first.document, delete_first.document);
        assert_eq!(update_first.document.visible_text(), "cited \n");
        assert_eq!(
            update_first.document.citation_database.references[0]
                .summary
                .title,
            "New"
        );
        assert!(update_first.document.validate().is_ok());
        assert!(update_first.warnings.is_empty());
    }

    #[test]
    fn citation_group_item_update_commutes_with_reference_update() {
        let mut base = Document::new("Doc");
        let reference_id = StableId::parse("ref-doe-2020").unwrap();
        let citation_id = StableId::parse("cite-intro").unwrap();
        base.citation_database
            .upsert_reference(BibliographyReference {
                id: reference_id.clone(),
                revision: 1,
                source: CitationSource {
                    format: CitationSourceFormat::CitumNative,
                    bytes: b"title: Old".to_vec(),
                },
                summary: CitationSummary {
                    title: "Old".to_string(),
                    authors: vec!["Doe".to_string()],
                    issued: Some("2020".to_string()),
                    doi: None,
                    url: None,
                },
                deleted: false,
            });
        base.citation_database.upsert_citation(CitationGroup {
            id: citation_id.clone(),
            revision: 1,
            items: vec![CitationItem {
                reference_id: reference_id.clone(),
                locator: Some("17".to_string()),
                label: Some("page".to_string()),
                prefix: Some("see".to_string()),
                suffix: None,
                suppress_author: false,
            }],
            placement: CitationPlacement::Inline,
            rendered_cache: Some("(see Doe 2020, 17)".to_string()),
            deleted: false,
        });
        base.blocks.push(Block {
            id: StableId::new("block"),
            kind: BlockKind::Paragraph,
            content: vec![
                Inline::text("cited "),
                Inline::Citation {
                    id: StableId::parse("citation-label").unwrap(),
                    citation_id: citation_id.clone(),
                    rendered_cache: Some("(see Doe 2020, 17)".to_string()),
                },
            ],
            properties: Vec::new(),
        });

        let reference_update = Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::UpsertBibliographyReference {
                reference: BibliographyReference {
                    id: reference_id.clone(),
                    revision: 2,
                    source: CitationSource {
                        format: CitationSourceFormat::CitumNative,
                        bytes: b"title: New\nauthor: Smith; Jones\nyear: 2024\ndoi: 10.7777/merge\nurl: https://example.invalid/merge".to_vec(),
                    },
                    summary: CitationSummary {
                        title: "New".to_string(),
                        authors: vec!["Smith".to_string(), "Jones".to_string()],
                        issued: Some("2024".to_string()),
                        doi: Some("10.7777/merge".to_string()),
                        url: Some("https://example.invalid/merge".to_string()),
                    },
                    deleted: false,
                },
            },
        };
        let item_update = Operation {
            id: OperationId {
                actor: ActorId("b".to_string()),
                seq: 1,
            },
            kind: OperationKind::UpsertCitationGroup {
                citation: CitationGroup {
                    id: citation_id,
                    revision: 2,
                    items: vec![CitationItem {
                        reference_id,
                        locator: Some("19".to_string()),
                        label: Some("page".to_string()),
                        prefix: Some("compare".to_string()),
                        suffix: Some("for context".to_string()),
                        suppress_author: true,
                    }],
                    placement: CitationPlacement::Inline,
                    rendered_cache: Some("(stale editor cache)".to_string()),
                    deleted: false,
                },
            },
        };

        let reference_first = merge_operations(
            &base,
            &[vec![reference_update.clone()], vec![item_update.clone()]],
        )
        .unwrap();
        let item_first =
            merge_operations(&base, &[vec![item_update], vec![reference_update]]).unwrap();

        assert_eq!(reference_first.document, item_first.document);
        assert_eq!(
            reference_first.document.visible_text(),
            "cited (compare 2024, page 19 for context)\n"
        );
        let reference = &reference_first.document.citation_database.references[0];
        assert_eq!(reference.summary.title, "New");
        assert_eq!(
            reference.summary.authors,
            vec!["Smith".to_string(), "Jones".to_string()]
        );
        assert_eq!(reference.summary.doi.as_deref(), Some("10.7777/merge"));
        assert_eq!(
            reference.summary.url.as_deref(),
            Some("https://example.invalid/merge")
        );
        let source = String::from_utf8_lossy(&reference.source.bytes);
        assert!(source.contains("author: Smith; Jones"));
        assert!(source.contains("doi: 10.7777/merge"));
        assert!(source.contains("url: https://example.invalid/merge"));
        assert_eq!(
            reference_first.document.citation_database.citations[0].rendered_cache,
            Some("(compare 2024, page 19 for context)".to_string())
        );
        match &reference_first.document.blocks[0].content[1] {
            Inline::Citation { rendered_cache, .. } => assert_eq!(rendered_cache, &None),
            _ => panic!("expected citation label"),
        }
        assert!(reference_first.document.validate().is_ok());
        assert!(reference_first.warnings.is_empty());
    }

    #[test]
    fn deleting_bibliography_reference_invalidates_dependent_citation_caches() {
        let mut base = Document::new("Doc");
        let reference_id = StableId::parse("ref-doe-2020").unwrap();
        let citation_id = StableId::parse("cite-intro").unwrap();
        base.citation_database
            .upsert_reference(BibliographyReference {
                id: reference_id.clone(),
                revision: 1,
                source: CitationSource {
                    format: CitationSourceFormat::CitumNative,
                    bytes: b"title: Old".to_vec(),
                },
                summary: CitationSummary {
                    title: "Old".to_string(),
                    authors: vec!["Doe".to_string()],
                    issued: Some("2020".to_string()),
                    doi: None,
                    url: None,
                },
                deleted: false,
            });
        base.citation_database.upsert_citation(CitationGroup {
            id: citation_id.clone(),
            revision: 1,
            items: vec![CitationItem {
                reference_id: reference_id.clone(),
                locator: None,
                label: None,
                prefix: None,
                suffix: None,
                suppress_author: false,
            }],
            placement: CitationPlacement::Inline,
            rendered_cache: Some("(Doe 2020)".to_string()),
            deleted: false,
        });
        base.blocks.push(Block {
            id: StableId::new("block"),
            kind: BlockKind::Paragraph,
            content: vec![Inline::Citation {
                id: StableId::parse("citation-label").unwrap(),
                citation_id: citation_id.clone(),
                rendered_cache: Some("(Doe 2020)".to_string()),
            }],
            properties: Vec::new(),
        });

        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::DeleteBibliographyReference {
                    reference_id,
                    revision: 2,
                },
            }]],
        )
        .unwrap();

        assert_eq!(result.document.visible_text(), "[cite-intro]\n");
        assert!(result.document.citation_database.references[0].deleted);
        assert_eq!(
            result.document.citation_database.citations[0].rendered_cache,
            None
        );
        match &result.document.blocks[0].content[0] {
            Inline::Citation { rendered_cache, .. } => assert_eq!(rendered_cache, &None),
            _ => panic!("expected citation label"),
        }
        assert!(result
            .warnings
            .iter()
            .any(|warning| warning.code == "citation-reference-missing"));
        assert!(result.document.validate().is_ok());
    }

    #[test]
    fn bibliography_reference_delete_wins_over_older_stale_upsert_by_revision() {
        let mut base = Document::new("Doc");
        let reference_id = StableId::parse("ref-delete-stale-upsert").unwrap();
        let citation_id = StableId::parse("cite-delete-stale-upsert").unwrap();
        base.citation_database
            .upsert_reference(BibliographyReference {
                id: reference_id.clone(),
                revision: 1,
                source: CitationSource {
                    format: CitationSourceFormat::CitumNative,
                    bytes: b"title: Original".to_vec(),
                },
                summary: CitationSummary {
                    title: "Original".to_string(),
                    authors: vec!["Doe".to_string()],
                    issued: Some("2020".to_string()),
                    doi: None,
                    url: None,
                },
                deleted: false,
            });
        base.citation_database.upsert_citation(CitationGroup {
            id: citation_id.clone(),
            revision: 1,
            items: vec![CitationItem {
                reference_id: reference_id.clone(),
                locator: None,
                label: None,
                prefix: None,
                suffix: None,
                suppress_author: false,
            }],
            placement: CitationPlacement::Inline,
            rendered_cache: Some("(Doe 2020)".to_string()),
            deleted: false,
        });
        base.blocks.push(Block {
            id: StableId::new("block"),
            kind: BlockKind::Paragraph,
            content: vec![Inline::Citation {
                id: StableId::parse("citation-label-ref-delete-stale").unwrap(),
                citation_id: citation_id.clone(),
                rendered_cache: Some("(Doe 2020)".to_string()),
            }],
            properties: Vec::new(),
        });

        let stale_update = Operation {
            id: OperationId {
                actor: ActorId("actor-a".to_string()),
                seq: 1,
            },
            kind: OperationKind::UpsertBibliographyReference {
                reference: BibliographyReference {
                    id: reference_id.clone(),
                    revision: 2,
                    source: CitationSource {
                        format: CitationSourceFormat::CitumNative,
                        bytes: b"title: Stale\nyear: 2021".to_vec(),
                    },
                    summary: CitationSummary {
                        title: "Stale".to_string(),
                        authors: vec!["Doe".to_string()],
                        issued: Some("2021".to_string()),
                        doi: None,
                        url: None,
                    },
                    deleted: false,
                },
            },
        };
        let delete = Operation {
            id: OperationId {
                actor: ActorId("actor-z".to_string()),
                seq: 1,
            },
            kind: OperationKind::DeleteBibliographyReference {
                reference_id,
                revision: 3,
            },
        };

        let update_first =
            merge_operations(&base, &[vec![stale_update.clone()], vec![delete.clone()]]).unwrap();
        let delete_first = merge_operations(&base, &[vec![delete], vec![stale_update]]).unwrap();

        assert_eq!(update_first.document, delete_first.document);
        assert_eq!(update_first.warnings, delete_first.warnings);
        let reference = &update_first.document.citation_database.references[0];
        assert!(reference.deleted);
        assert_eq!(reference.revision, 3);
        assert_eq!(reference.summary.title, "Stale");
        assert_eq!(
            update_first.document.visible_text(),
            "[cite-delete-stale-upsert]\n"
        );
        assert_eq!(
            update_first.document.citation_database.citations[0].rendered_cache,
            None
        );
        match &update_first.document.blocks[0].content[0] {
            Inline::Citation { rendered_cache, .. } => assert_eq!(rendered_cache, &None),
            _ => panic!("expected citation label"),
        }
        assert!(update_first
            .warnings
            .iter()
            .any(|warning| warning.code == "citation-reference-missing"));
        update_first.document.validate().unwrap();
    }

    #[test]
    fn concurrent_style_change_and_reference_delete_converge_for_table_citations() {
        let mut base = Document::new("Doc");
        let reference_id = StableId::parse("ref-doe-2020").unwrap();
        let citation_id = StableId::parse("cite-table").unwrap();
        base.citation_database
            .upsert_reference(BibliographyReference {
                id: reference_id.clone(),
                revision: 1,
                source: CitationSource {
                    format: CitationSourceFormat::CitumNative,
                    bytes: b"title: Table Source".to_vec(),
                },
                summary: CitationSummary {
                    title: "Table Source".to_string(),
                    authors: vec!["Doe".to_string()],
                    issued: Some("2020".to_string()),
                    doi: None,
                    url: None,
                },
                deleted: false,
            });
        base.citation_database.upsert_citation(CitationGroup {
            id: citation_id.clone(),
            revision: 1,
            items: vec![CitationItem {
                reference_id: reference_id.clone(),
                locator: None,
                label: None,
                prefix: None,
                suffix: None,
                suppress_author: false,
            }],
            placement: CitationPlacement::Inline,
            rendered_cache: Some("(Doe 2020)".to_string()),
            deleted: false,
        });
        base.blocks.push(Block {
            id: StableId::new("table"),
            kind: BlockKind::Table {
                rows: vec![TableRow {
                    id: StableId::new("row"),
                    cells: vec![TableCell {
                        id: StableId::new("cell"),
                        blocks: vec![Block {
                            id: StableId::new("cell-block"),
                            kind: BlockKind::Paragraph,
                            content: vec![Inline::Citation {
                                id: StableId::parse("citation-label-table").unwrap(),
                                citation_id: citation_id.clone(),
                                rendered_cache: Some("(Doe 2020)".to_string()),
                            }],
                            properties: Vec::new(),
                        }],
                        properties: Vec::new(),
                    }],
                }],
            },
            content: Vec::new(),
            properties: Vec::new(),
        });

        let delete_reference = Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::DeleteBibliographyReference {
                reference_id,
                revision: 2,
            },
        };
        let style_change = Operation {
            id: OperationId {
                actor: ActorId("b".to_string()),
                seq: 1,
            },
            kind: OperationKind::UpdateCitationStyle {
                style: "ieee".to_string(),
                locale: "en-US".to_string(),
            },
        };

        let delete_first = merge_operations(
            &base,
            &[vec![delete_reference.clone()], vec![style_change.clone()]],
        )
        .unwrap();
        let style_first =
            merge_operations(&base, &[vec![style_change], vec![delete_reference]]).unwrap();

        assert_eq!(delete_first.document, style_first.document);
        assert_eq!(delete_first.warnings, style_first.warnings);
        assert_eq!(delete_first.document.visible_text(), "[cite-table]\n");
        assert!(delete_first.document.citation_database.references[0].deleted);
        assert_eq!(
            delete_first.document.citation_database.citations[0].rendered_cache,
            None
        );
        match &delete_first.document.blocks[0].kind {
            BlockKind::Table { rows } => match &rows[0].cells[0].blocks[0].content[0] {
                Inline::Citation { rendered_cache, .. } => assert_eq!(rendered_cache, &None),
                _ => panic!("expected citation label"),
            },
            _ => panic!("expected table"),
        }
        assert!(delete_first
            .warnings
            .iter()
            .any(|warning| warning.code == "citation-reference-missing"));
        assert!(delete_first.document.validate().is_ok());
    }

    #[test]
    fn bibliography_reference_restore_wins_over_older_delete_by_revision() {
        let mut base = Document::new("Doc");
        let reference_id = StableId::parse("ref-doe-2020").unwrap();
        let citation_id = StableId::parse("cite-intro").unwrap();
        base.citation_database
            .upsert_reference(BibliographyReference {
                id: reference_id.clone(),
                revision: 1,
                source: CitationSource {
                    format: CitationSourceFormat::CitumNative,
                    bytes: b"title: Old".to_vec(),
                },
                summary: CitationSummary {
                    title: "Old".to_string(),
                    authors: vec!["Doe".to_string()],
                    issued: Some("2020".to_string()),
                    doi: None,
                    url: None,
                },
                deleted: false,
            });
        base.citation_database.upsert_citation(CitationGroup {
            id: citation_id.clone(),
            revision: 1,
            items: vec![CitationItem {
                reference_id: reference_id.clone(),
                locator: None,
                label: None,
                prefix: None,
                suffix: None,
                suppress_author: false,
            }],
            placement: CitationPlacement::Inline,
            rendered_cache: Some("(Doe 2020)".to_string()),
            deleted: false,
        });
        base.blocks.push(Block {
            id: StableId::new("block"),
            kind: BlockKind::Paragraph,
            content: vec![Inline::Citation {
                id: StableId::parse("citation-label").unwrap(),
                citation_id,
                rendered_cache: Some("(Doe 2020)".to_string()),
            }],
            properties: Vec::new(),
        });

        let delete = Operation {
            id: OperationId {
                actor: ActorId("z".to_string()),
                seq: 1,
            },
            kind: OperationKind::DeleteBibliographyReference {
                reference_id: reference_id.clone(),
                revision: 2,
            },
        };
        let restore = Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::UpsertBibliographyReference {
                reference: BibliographyReference {
                    id: reference_id,
                    revision: 3,
                    source: CitationSource {
                        format: CitationSourceFormat::CitumNative,
                        bytes: b"title: Restored".to_vec(),
                    },
                    summary: CitationSummary {
                        title: "Restored".to_string(),
                        authors: vec!["Doe".to_string()],
                        issued: Some("2020".to_string()),
                        doi: None,
                        url: None,
                    },
                    deleted: false,
                },
            },
        };

        let restore_first =
            merge_operations(&base, &[vec![restore.clone()], vec![delete.clone()]]).unwrap();
        let delete_first = merge_operations(&base, &[vec![delete], vec![restore]]).unwrap();

        assert_eq!(restore_first.document, delete_first.document);
        assert!(!restore_first.document.citation_database.references[0].deleted);
        assert_eq!(restore_first.document.visible_text(), "(Doe 2020)\n");
        assert!(restore_first.warnings.is_empty());
        assert!(restore_first.document.validate().is_ok());
    }

    #[test]
    fn citation_missing_reference_clears_stale_rendered_labels() {
        let mut base = Document::new("Doc");
        let citation_id = StableId::parse("cite-missing-reference").unwrap();
        base.blocks.push(Block {
            id: StableId::new("block"),
            kind: BlockKind::Paragraph,
            content: vec![Inline::Citation {
                id: StableId::parse("citation-label-missing-reference").unwrap(),
                citation_id: citation_id.clone(),
                rendered_cache: Some("(Misleading 2020)".to_string()),
            }],
            properties: Vec::new(),
        });

        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::UpsertCitationGroup {
                    citation: CitationGroup {
                        id: citation_id,
                        revision: 1,
                        items: vec![CitationItem {
                            reference_id: StableId::parse("ref-missing").unwrap(),
                            locator: Some("42".to_string()),
                            label: Some("page".to_string()),
                            prefix: None,
                            suffix: None,
                            suppress_author: false,
                        }],
                        placement: CitationPlacement::Inline,
                        rendered_cache: Some("(Misleading 2020)".to_string()),
                        deleted: false,
                    },
                },
            }]],
        )
        .unwrap();

        assert_eq!(result.document.visible_text(), "[cite-missing-reference]\n");
        assert_eq!(
            result.document.citation_database.citations[0].rendered_cache,
            None
        );
        match &result.document.blocks[0].content[0] {
            Inline::Citation { rendered_cache, .. } => assert_eq!(rendered_cache, &None),
            _ => panic!("expected citation label"),
        }
        assert!(result
            .warnings
            .iter()
            .any(|warning| warning.code == "citation-reference-missing"));
        assert!(result.document.validate().is_ok());
    }

    #[test]
    fn missing_citation_group_clears_inline_rendered_label_cache() {
        let mut base = Document::new("Doc");
        let citation_id = StableId::parse("cite-missing-group").unwrap();
        base.blocks.push(Block {
            id: StableId::new("block"),
            kind: BlockKind::Paragraph,
            content: vec![Inline::Citation {
                id: StableId::parse("citation-label-missing-group").unwrap(),
                citation_id: citation_id.clone(),
                rendered_cache: Some("(Stale Citation)".to_string()),
            }],
            properties: Vec::new(),
        });

        let result = merge_operations(&base, &[Vec::new()]).unwrap();

        assert_eq!(result.document.visible_text(), "[cite-missing-group]\n");
        match &result.document.blocks[0].content[0] {
            Inline::Citation { rendered_cache, .. } => assert_eq!(rendered_cache, &None),
            _ => panic!("expected citation label"),
        }
        assert!(result
            .warnings
            .iter()
            .any(|warning| warning.code == "citation-group-missing"));
        assert!(result.document.validate().is_ok());
    }

    #[test]
    fn deleted_citation_group_clears_nested_inline_rendered_label_cache() {
        let mut base = Document::new("Doc");
        let reference_id = StableId::parse("ref-doe-2020").unwrap();
        let citation_id = StableId::parse("cite-deleted-group").unwrap();
        base.citation_database
            .upsert_reference(BibliographyReference {
                id: reference_id.clone(),
                revision: 1,
                source: CitationSource {
                    format: CitationSourceFormat::CitumNative,
                    bytes: b"title: Example".to_vec(),
                },
                summary: CitationSummary {
                    title: "Example".to_string(),
                    authors: vec!["Doe".to_string()],
                    issued: Some("2020".to_string()),
                    doi: None,
                    url: None,
                },
                deleted: false,
            });
        base.citation_database.upsert_citation(CitationGroup {
            id: citation_id.clone(),
            revision: 1,
            items: vec![CitationItem {
                reference_id,
                locator: None,
                label: None,
                prefix: None,
                suffix: None,
                suppress_author: false,
            }],
            placement: CitationPlacement::Inline,
            rendered_cache: Some("(Doe 2020)".to_string()),
            deleted: false,
        });
        base.blocks.push(Block {
            id: StableId::new("table"),
            kind: BlockKind::Table {
                rows: vec![TableRow {
                    id: StableId::new("row"),
                    cells: vec![TableCell {
                        id: StableId::new("cell"),
                        blocks: vec![Block {
                            id: StableId::new("cell-block"),
                            kind: BlockKind::Paragraph,
                            content: vec![Inline::Citation {
                                id: StableId::parse("citation-label-deleted-group").unwrap(),
                                citation_id: citation_id.clone(),
                                rendered_cache: Some("(Doe 2020)".to_string()),
                            }],
                            properties: Vec::new(),
                        }],
                        properties: Vec::new(),
                    }],
                }],
            },
            content: Vec::new(),
            properties: Vec::new(),
        });

        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::DeleteCitationGroup {
                    citation_id,
                    revision: 2,
                },
            }]],
        )
        .unwrap();

        assert_eq!(result.document.visible_text(), "[cite-deleted-group]\n");
        match &result.document.blocks[0].kind {
            BlockKind::Table { rows } => match &rows[0].cells[0].blocks[0].content[0] {
                Inline::Citation { rendered_cache, .. } => assert_eq!(rendered_cache, &None),
                _ => panic!("expected citation label"),
            },
            _ => panic!("expected table"),
        }
        assert!(result.document.citation_database.citations[0].deleted);
        assert!(result
            .warnings
            .iter()
            .any(|warning| warning.code == "citation-group-missing"));
        assert!(result.document.validate().is_ok());
    }

    #[test]
    fn citation_group_restore_wins_over_older_delete_by_revision() {
        let mut base = Document::new("Doc");
        let reference_id = StableId::parse("ref-doe-2020").unwrap();
        let citation_id = StableId::parse("cite-intro").unwrap();
        base.citation_database
            .upsert_reference(BibliographyReference {
                id: reference_id.clone(),
                revision: 1,
                source: CitationSource {
                    format: CitationSourceFormat::CitumNative,
                    bytes: b"title: Example".to_vec(),
                },
                summary: CitationSummary {
                    title: "Example".to_string(),
                    authors: vec!["Doe".to_string()],
                    issued: Some("2020".to_string()),
                    doi: None,
                    url: None,
                },
                deleted: false,
            });
        base.citation_database.upsert_citation(CitationGroup {
            id: citation_id.clone(),
            revision: 1,
            items: vec![CitationItem {
                reference_id: reference_id.clone(),
                locator: None,
                label: None,
                prefix: None,
                suffix: None,
                suppress_author: false,
            }],
            placement: CitationPlacement::Inline,
            rendered_cache: Some("(Doe 2020)".to_string()),
            deleted: false,
        });
        base.blocks.push(Block {
            id: StableId::new("block"),
            kind: BlockKind::Paragraph,
            content: vec![Inline::Citation {
                id: StableId::parse("citation-label").unwrap(),
                citation_id: citation_id.clone(),
                rendered_cache: Some("(Doe 2020)".to_string()),
            }],
            properties: Vec::new(),
        });

        let delete = Operation {
            id: OperationId {
                actor: ActorId("z".to_string()),
                seq: 1,
            },
            kind: OperationKind::DeleteCitationGroup {
                citation_id: citation_id.clone(),
                revision: 2,
            },
        };
        let restore = Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::UpsertCitationGroup {
                citation: CitationGroup {
                    id: citation_id,
                    revision: 3,
                    items: vec![CitationItem {
                        reference_id,
                        locator: Some("12".to_string()),
                        label: Some("page".to_string()),
                        prefix: Some("see".to_string()),
                        suffix: None,
                        suppress_author: false,
                    }],
                    placement: CitationPlacement::Inline,
                    rendered_cache: Some("(stale restore cache)".to_string()),
                    deleted: false,
                },
            },
        };

        let restore_first =
            merge_operations(&base, &[vec![restore.clone()], vec![delete.clone()]]).unwrap();
        let delete_first = merge_operations(&base, &[vec![delete], vec![restore]]).unwrap();

        assert_eq!(restore_first.document, delete_first.document);
        assert!(!restore_first.document.citation_database.citations[0].deleted);
        assert_eq!(
            restore_first.document.visible_text(),
            "(see Doe 2020, page 12)\n"
        );
        assert_eq!(
            restore_first.document.citation_database.citations[0].rendered_cache,
            Some("(see Doe 2020, page 12)".to_string())
        );
        assert!(restore_first.warnings.is_empty());
        assert!(restore_first.document.validate().is_ok());
    }

    #[test]
    fn citation_group_delete_wins_over_older_stale_upsert_by_revision() {
        let mut base = Document::new("Doc");
        let reference_id = StableId::parse("ref-doe-2020").unwrap();
        let citation_id = StableId::parse("cite-intro").unwrap();
        base.citation_database
            .upsert_reference(BibliographyReference {
                id: reference_id.clone(),
                revision: 1,
                source: CitationSource {
                    format: CitationSourceFormat::CitumNative,
                    bytes: b"title: Example".to_vec(),
                },
                summary: CitationSummary {
                    title: "Example".to_string(),
                    authors: vec!["Doe".to_string()],
                    issued: Some("2020".to_string()),
                    doi: None,
                    url: None,
                },
                deleted: false,
            });
        base.citation_database.upsert_citation(CitationGroup {
            id: citation_id.clone(),
            revision: 1,
            items: vec![CitationItem {
                reference_id: reference_id.clone(),
                locator: None,
                label: None,
                prefix: None,
                suffix: None,
                suppress_author: false,
            }],
            placement: CitationPlacement::Inline,
            rendered_cache: Some("(Doe 2020)".to_string()),
            deleted: false,
        });
        base.blocks.push(Block {
            id: StableId::new("block"),
            kind: BlockKind::Paragraph,
            content: vec![Inline::Citation {
                id: StableId::parse("citation-label").unwrap(),
                citation_id: citation_id.clone(),
                rendered_cache: Some("(Doe 2020)".to_string()),
            }],
            properties: Vec::new(),
        });

        let stale_update = Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::UpsertCitationGroup {
                citation: CitationGroup {
                    id: citation_id.clone(),
                    revision: 2,
                    items: vec![CitationItem {
                        reference_id,
                        locator: Some("44".to_string()),
                        label: Some("page".to_string()),
                        prefix: Some("see".to_string()),
                        suffix: None,
                        suppress_author: false,
                    }],
                    placement: CitationPlacement::Inline,
                    rendered_cache: Some("(stale rendered label)".to_string()),
                    deleted: false,
                },
            },
        };
        let delete = Operation {
            id: OperationId {
                actor: ActorId("z".to_string()),
                seq: 1,
            },
            kind: OperationKind::DeleteCitationGroup {
                citation_id: citation_id.clone(),
                revision: 3,
            },
        };

        let update_first =
            merge_operations(&base, &[vec![stale_update.clone()], vec![delete.clone()]]).unwrap();
        let delete_first = merge_operations(&base, &[vec![delete], vec![stale_update]]).unwrap();

        assert_eq!(update_first.document, delete_first.document);
        assert!(update_first.document.citation_database.citations[0].deleted);
        assert_eq!(
            update_first.document.citation_database.citations[0].rendered_cache,
            None
        );
        assert_eq!(update_first.document.visible_text(), "[cite-intro]\n");
        match &update_first.document.blocks[0].content[0] {
            Inline::Citation { rendered_cache, .. } => assert_eq!(rendered_cache, &None),
            _ => panic!("expected citation label"),
        }
        assert!(update_first
            .warnings
            .iter()
            .any(|warning| warning.code == "citation-group-missing"));
        assert!(update_first.document.validate().is_ok());
    }

    #[test]
    fn citation_style_update_invalidates_table_inline_caches() {
        let mut base = Document::new("Doc");
        let reference_id = StableId::parse("ref-doe-2020").unwrap();
        let citation_id = StableId::parse("cite-intro").unwrap();
        base.citation_database
            .upsert_reference(BibliographyReference {
                id: reference_id.clone(),
                revision: 1,
                source: CitationSource {
                    format: CitationSourceFormat::CitumNative,
                    bytes: b"title: Old".to_vec(),
                },
                summary: CitationSummary {
                    title: "Old".to_string(),
                    authors: vec!["Doe".to_string()],
                    issued: Some("2020".to_string()),
                    doi: None,
                    url: None,
                },
                deleted: false,
            });
        base.citation_database.upsert_citation(CitationGroup {
            id: citation_id.clone(),
            revision: 1,
            items: vec![CitationItem {
                reference_id,
                locator: None,
                label: None,
                prefix: None,
                suffix: None,
                suppress_author: false,
            }],
            placement: CitationPlacement::Inline,
            rendered_cache: Some("(Doe 2020)".to_string()),
            deleted: false,
        });
        base.blocks.push(Block {
            id: StableId::new("table"),
            kind: BlockKind::Table {
                rows: vec![opendoc_core::TableRow {
                    id: StableId::new("row"),
                    cells: vec![opendoc_core::TableCell {
                        id: StableId::new("cell"),
                        blocks: vec![Block {
                            id: StableId::new("cell-block"),
                            kind: BlockKind::Paragraph,
                            content: vec![Inline::Citation {
                                id: StableId::parse("citation-label").unwrap(),
                                citation_id,
                                rendered_cache: Some("(Doe 2020)".to_string()),
                            }],
                            properties: Vec::new(),
                        }],
                        properties: Vec::new(),
                    }],
                }],
            },
            content: Vec::new(),
            properties: Vec::new(),
        });

        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::UpdateCitationStyle {
                    style: "ieee".to_string(),
                    locale: "en-US".to_string(),
                },
            }]],
        )
        .unwrap();

        assert_eq!(result.document.visible_text(), "[1]\n");
        assert_eq!(
            result.document.citation_database.citations[0].rendered_cache,
            Some("[1]".to_string())
        );
        match &result.document.blocks[0].kind {
            BlockKind::Table { rows } => match &rows[0].cells[0].blocks[0].content[0] {
                Inline::Citation { rendered_cache, .. } => assert_eq!(rendered_cache, &None),
                _ => panic!("expected citation label"),
            },
            _ => panic!("expected table"),
        }
        assert!(result.document.validate().is_ok());
    }

    #[test]
    fn invalid_retained_record_upserts_degrade_to_warnings() {
        let base = Document::new("Doc");
        let result = merge_operations(
            &base,
            &[vec![
                Operation {
                    id: OperationId {
                        actor: ActorId("a".to_string()),
                        seq: 1,
                    },
                    kind: OperationKind::UpsertFootnote {
                        footnote: Footnote {
                            id: StableId::parse("footnote-empty").unwrap(),
                            revision: 1,
                            body: Vec::new(),
                            deleted: false,
                        },
                    },
                },
                Operation {
                    id: OperationId {
                        actor: ActorId("a".to_string()),
                        seq: 2,
                    },
                    kind: OperationKind::UpsertBibliographyReference {
                        reference: BibliographyReference {
                            id: StableId::parse("ref-empty").unwrap(),
                            revision: 1,
                            source: CitationSource {
                                format: CitationSourceFormat::CitumNative,
                                bytes: Vec::new(),
                            },
                            summary: CitationSummary {
                                title: String::new(),
                                authors: Vec::new(),
                                issued: None,
                                doi: None,
                                url: None,
                            },
                            deleted: false,
                        },
                    },
                },
                Operation {
                    id: OperationId {
                        actor: ActorId("a".to_string()),
                        seq: 3,
                    },
                    kind: OperationKind::UpsertCitationGroup {
                        citation: CitationGroup {
                            id: StableId::parse("cite-empty").unwrap(),
                            revision: 1,
                            items: Vec::new(),
                            placement: CitationPlacement::Inline,
                            rendered_cache: None,
                            deleted: false,
                        },
                    },
                },
            ]],
        )
        .unwrap();

        assert!(result.document.footnotes.is_empty());
        assert!(result.document.citation_database.references.is_empty());
        assert!(result.document.citation_database.citations.is_empty());
        assert_eq!(
            result
                .warnings
                .iter()
                .map(|warning| warning.code.as_str())
                .collect::<Vec<_>>(),
            vec![
                "invalid-footnote",
                "invalid-bibliography-reference",
                "invalid-citation-group"
            ]
        );
        assert!(result.document.validate().is_ok());
    }

    #[test]
    fn missing_footnote_citation_target_degrades_to_inline_placement() {
        let base = Document::new("Doc");
        let reference_id = StableId::parse("ref-doe-2020").unwrap();
        let citation_id = StableId::parse("cite-footnote").unwrap();
        let result = merge_operations(
            &base,
            &[vec![
                Operation {
                    id: OperationId {
                        actor: ActorId("a".to_string()),
                        seq: 1,
                    },
                    kind: OperationKind::UpsertBibliographyReference {
                        reference: BibliographyReference {
                            id: reference_id.clone(),
                            revision: 1,
                            source: CitationSource {
                                format: CitationSourceFormat::CitumNative,
                                bytes: b"title: Example".to_vec(),
                            },
                            summary: CitationSummary {
                                title: "Example".to_string(),
                                authors: vec!["Doe".to_string()],
                                issued: Some("2020".to_string()),
                                doi: None,
                                url: None,
                            },
                            deleted: false,
                        },
                    },
                },
                Operation {
                    id: OperationId {
                        actor: ActorId("a".to_string()),
                        seq: 2,
                    },
                    kind: OperationKind::UpsertCitationGroup {
                        citation: CitationGroup {
                            id: citation_id,
                            revision: 1,
                            items: vec![CitationItem {
                                reference_id,
                                locator: None,
                                label: None,
                                prefix: None,
                                suffix: None,
                                suppress_author: false,
                            }],
                            placement: CitationPlacement::Footnote {
                                footnote_id: StableId::parse("missing-footnote").unwrap(),
                            },
                            rendered_cache: Some("(Doe 2020)".to_string()),
                            deleted: false,
                        },
                    },
                },
            ]],
        )
        .unwrap();

        assert_eq!(result.warnings[0].code, "citation-footnote-target-missing");
        assert!(matches!(
            result.document.citation_database.citations[0].placement,
            CitationPlacement::Inline
        ));
        assert_eq!(
            result.document.citation_database.citations[0].rendered_cache,
            Some("(Doe 2020)".to_string())
        );
        assert!(result.document.validate().is_ok());
    }

    #[test]
    fn footnote_citation_placement_keeps_target_footnote_alive() {
        let mut base = Document::new("Doc");
        let footnote_id = StableId::parse("footnote-citation-target").unwrap();
        let reference_id = StableId::parse("ref-doe-2020").unwrap();
        let citation_id = StableId::parse("cite-footnote").unwrap();
        base.footnotes.push(Footnote {
            id: footnote_id.clone(),
            revision: 1,
            body: vec![Inline::text("citation lives in this footnote")],
            deleted: false,
        });
        base.citation_database
            .upsert_reference(BibliographyReference {
                id: reference_id.clone(),
                revision: 1,
                source: CitationSource {
                    format: CitationSourceFormat::CitumNative,
                    bytes: b"title: Example".to_vec(),
                },
                summary: CitationSummary {
                    title: "Example".to_string(),
                    authors: vec!["Doe".to_string()],
                    issued: Some("2020".to_string()),
                    doi: None,
                    url: None,
                },
                deleted: false,
            });
        base.citation_database.upsert_citation(CitationGroup {
            id: citation_id,
            revision: 1,
            items: vec![CitationItem {
                reference_id,
                locator: None,
                label: None,
                prefix: None,
                suffix: None,
                suppress_author: false,
            }],
            placement: CitationPlacement::Footnote {
                footnote_id: footnote_id.clone(),
            },
            rendered_cache: Some("(Doe 2020)".to_string()),
            deleted: false,
        });

        let result = merge_operations(&base, &[Vec::new()]).unwrap();

        assert!(!result.document.footnotes[0].deleted);
        assert!(matches!(
            &result.document.citation_database.citations[0].placement,
            CitationPlacement::Footnote { footnote_id: id } if id == &footnote_id
        ));
        assert!(result.warnings.is_empty());
        assert!(result.document.validate().is_ok());
    }

    #[test]
    fn footnote_citation_placement_survives_concurrent_inline_reference_delete() {
        let mut base = Document::new("Doc");
        let footnote_id = StableId::parse("footnote-citation-delete-ref").unwrap();
        let footnote_ref_id = StableId::parse("footnote-ref-delete-inline").unwrap();
        let reference_id = StableId::parse("ref-footnote-delete-inline").unwrap();
        let citation_id = StableId::parse("cite-footnote-delete-inline").unwrap();
        base.footnotes.push(Footnote {
            id: footnote_id.clone(),
            revision: 1,
            body: vec![Inline::text("citation footnote")],
            deleted: false,
        });
        base.citation_database
            .upsert_reference(BibliographyReference {
                id: reference_id.clone(),
                revision: 1,
                source: CitationSource {
                    format: CitationSourceFormat::CitumNative,
                    bytes: b"title: Footnote Citation".to_vec(),
                },
                summary: CitationSummary {
                    title: "Footnote Citation".to_string(),
                    authors: vec!["Doe".to_string()],
                    issued: Some("2020".to_string()),
                    doi: None,
                    url: None,
                },
                deleted: false,
            });
        base.citation_database.upsert_citation(CitationGroup {
            id: citation_id.clone(),
            revision: 1,
            items: vec![CitationItem {
                reference_id,
                locator: None,
                label: None,
                prefix: None,
                suffix: None,
                suppress_author: false,
            }],
            placement: CitationPlacement::Footnote {
                footnote_id: footnote_id.clone(),
            },
            rendered_cache: Some("(Doe 2020)".to_string()),
            deleted: false,
        });
        base.blocks.push(Block {
            id: StableId::new("block"),
            kind: BlockKind::Paragraph,
            content: vec![
                Inline::text("body"),
                Inline::FootnoteRef {
                    id: footnote_ref_id.clone(),
                    footnote_id: footnote_id.clone(),
                },
            ],
            properties: Vec::new(),
        });

        let delete_ref = Operation {
            id: OperationId {
                actor: ActorId("actor-a".to_string()),
                seq: 1,
            },
            kind: OperationKind::DeleteInline {
                inline_id: footnote_ref_id,
            },
        };
        let style_update = Operation {
            id: OperationId {
                actor: ActorId("actor-b".to_string()),
                seq: 1,
            },
            kind: OperationKind::UpdateCitationStyle {
                style: "ieee".to_string(),
                locale: "en-GB".to_string(),
            },
        };

        let delete_first = merge_operations(
            &base,
            &[vec![delete_ref.clone()], vec![style_update.clone()]],
        )
        .unwrap();
        let style_first = merge_operations(&base, &[vec![style_update], vec![delete_ref]]).unwrap();

        assert_eq!(delete_first.document, style_first.document);
        assert_eq!(delete_first.warnings, style_first.warnings);
        assert_eq!(delete_first.document.visible_text(), "body\n");
        assert!(!delete_first.document.footnotes[0].deleted);
        assert!(delete_first.warnings.is_empty());
        assert!(matches!(
            &delete_first.document.citation_database.citations[0].placement,
            CitationPlacement::Footnote { footnote_id: id } if id == &footnote_id
        ));
        delete_first.document.validate().unwrap();
    }

    #[test]
    fn inline_text_updates_preserve_marks_and_reach_table_cells() {
        let mut base = Document::new("Doc");
        let paragraph = Block::paragraph("before");
        let paragraph_text_id = match &paragraph.content[0] {
            Inline::Text { id, .. } => id.clone(),
            _ => unreachable!(),
        };
        let table_text = Inline::text("cell");
        let table_text_id = inline_id(&table_text).clone();
        base.blocks.push(paragraph);
        base.blocks.push(Block {
            id: StableId::new("block"),
            kind: BlockKind::Table {
                rows: vec![opendoc_core::TableRow {
                    id: StableId::new("row"),
                    cells: vec![opendoc_core::TableCell {
                        id: StableId::new("cell"),
                        blocks: vec![Block {
                            id: StableId::new("block"),
                            kind: BlockKind::Paragraph,
                            content: vec![table_text],
                            properties: Vec::new(),
                        }],
                        properties: Vec::new(),
                    }],
                }],
            },
            content: Vec::new(),
            properties: Vec::new(),
        });

        let result = merge_operations(
            &base,
            &[vec![
                Operation {
                    id: OperationId {
                        actor: ActorId("a".to_string()),
                        seq: 1,
                    },
                    kind: OperationKind::AddMark {
                        text_id: paragraph_text_id.clone(),
                        mark: Mark {
                            kind: MarkKind::Bold,
                            value: None,
                            expand: MarkExpand::Both,
                        },
                    },
                },
                Operation {
                    id: OperationId {
                        actor: ActorId("a".to_string()),
                        seq: 2,
                    },
                    kind: OperationKind::UpdateInlineText {
                        inline_id: paragraph_text_id,
                        text: "after".to_string(),
                    },
                },
                Operation {
                    id: OperationId {
                        actor: ActorId("a".to_string()),
                        seq: 3,
                    },
                    kind: OperationKind::UpdateInlineText {
                        inline_id: table_text_id,
                        text: "edited cell".to_string(),
                    },
                },
            ]],
        )
        .unwrap();

        assert!(result.document.visible_text().contains("after"));
        assert!(result.document.visible_text().contains("edited cell"));
        match &result.document.blocks[0].content[0] {
            Inline::Text { marks, .. } => assert_eq!(marks.len(), 1),
            _ => unreachable!(),
        }
    }

    #[test]
    fn character_level_text_operations_merge_without_losing_text() {
        let mut base = Document::new("Doc");
        let text_id = StableId::parse("text-1").unwrap();
        base.blocks.push(Block {
            id: StableId::parse("block-1").unwrap(),
            kind: BlockKind::Paragraph,
            content: vec![Inline::Text {
                id: text_id.clone(),
                text: "Hello world".to_string(),
                marks: Vec::new(),
            }],
            properties: Vec::new(),
        });
        let op = |actor: &str, seq: u64, kind: OperationKind| Operation {
            id: OperationId {
                actor: ActorId(actor.to_string()),
                seq,
            },
            kind,
        };
        // Two actors edit the same run concurrently: one inserts, one deletes.
        let result = merge_operations(
            &base,
            &[
                vec![op(
                    "alice",
                    1,
                    OperationKind::InsertText {
                        inline_id: text_id.clone(),
                        offset: 5,
                        text: " brave".to_string(),
                    },
                )],
                vec![op(
                    "bob",
                    1,
                    OperationKind::DeleteText {
                        inline_id: text_id.clone(),
                        start: 0,
                        end: 5,
                    },
                )],
            ],
        )
        .unwrap();
        let text = result.document.visible_text();
        assert!(text.contains("brave"), "{text}");
        assert!(text.contains("world"), "{text}");
        assert!(result.warnings.is_empty(), "{:?}", result.warnings);

        // Offsets past the end are clamped instead of panicking; unicode
        // offsets count scalar values, not bytes.
        let result = merge_operations(
            &base,
            &[vec![
                op(
                    "alice",
                    1,
                    OperationKind::InsertText {
                        inline_id: text_id.clone(),
                        offset: 5,
                        text: " héllo".to_string(),
                    },
                ),
                op(
                    "alice",
                    2,
                    OperationKind::DeleteText {
                        inline_id: text_id.clone(),
                        start: 7,
                        end: 999,
                    },
                ),
            ]],
        )
        .unwrap();
        assert_eq!(result.document.visible_text().trim_end(), "Hello h");

        // Editing a derived inline is refused with a warning, missing ones warn.
        let mut derived = Document::new("Doc");
        derived.blocks.push(Block {
            id: StableId::parse("block-2").unwrap(),
            kind: BlockKind::Paragraph,
            content: vec![Inline::Mention {
                id: StableId::parse("mention-1").unwrap(),
                label: "@someone".to_string(),
            }],
            properties: Vec::new(),
        });
        let result = merge_operations(
            &derived,
            &[vec![
                op(
                    "alice",
                    1,
                    OperationKind::InsertText {
                        inline_id: StableId::parse("mention-1").unwrap(),
                        offset: 0,
                        text: "x".to_string(),
                    },
                ),
                op(
                    "alice",
                    2,
                    OperationKind::DeleteText {
                        inline_id: StableId::parse("nope").unwrap(),
                        start: 0,
                        end: 1,
                    },
                ),
            ]],
        )
        .unwrap();
        assert_eq!(result.warnings.len(), 2);
    }

    #[test]
    fn link_href_updates_are_operation_backed() {
        let mut base = Document::new("Doc");
        let link_id = StableId::parse("link-1").unwrap();
        base.blocks.push(Block {
            id: StableId::parse("block-1").unwrap(),
            kind: BlockKind::Paragraph,
            content: vec![Inline::Link {
                id: link_id.clone(),
                text: "paper".to_string(),
                href: "https://example.invalid/old".to_string(),
                marks: Vec::new(),
            }],
            properties: Vec::new(),
        });

        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::UpdateLinkHref {
                    inline_id: link_id,
                    href: "https://example.invalid/new".to_string(),
                },
            }]],
        )
        .unwrap();

        match &result.document.blocks[0].content[0] {
            Inline::Link { href, .. } => assert_eq!(href, "https://example.invalid/new"),
            _ => unreachable!(),
        }
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn empty_link_href_update_degrades_to_warning() {
        let mut base = Document::new("Doc");
        let link_id = StableId::parse("link-1").unwrap();
        base.blocks.push(Block {
            id: StableId::parse("block-1").unwrap(),
            kind: BlockKind::Paragraph,
            content: vec![Inline::Link {
                id: link_id.clone(),
                text: "paper".to_string(),
                href: "https://example.invalid/old".to_string(),
                marks: Vec::new(),
            }],
            properties: Vec::new(),
        });

        let invalid = Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::UpdateLinkHref {
                inline_id: link_id.clone(),
                href: " ".to_string(),
            },
        };
        let valid = Operation {
            id: OperationId {
                actor: ActorId("b".to_string()),
                seq: 1,
            },
            kind: OperationKind::UpdateLinkHref {
                inline_id: link_id,
                href: "https://example.invalid/new".to_string(),
            },
        };

        let actor_streams =
            merge_operations(&base, &[vec![invalid.clone()], vec![valid.clone()]]).unwrap();
        let reversed_batches = merge_operations(&base, &[vec![valid], vec![invalid]]).unwrap();

        assert_eq!(actor_streams.document, reversed_batches.document);
        assert_eq!(actor_streams.warnings, reversed_batches.warnings);
        match &actor_streams.document.blocks[0].content[0] {
            Inline::Link { href, .. } => assert_eq!(href, "https://example.invalid/new"),
            _ => unreachable!(),
        }
        assert_eq!(actor_streams.warnings[0].code, "invalid-link-href");
        actor_streams.document.validate().unwrap();
    }

    #[test]
    fn link_href_update_warns_for_non_link_inline() {
        let mut base = Document::new("Doc");
        let text = Inline::text("not a link");
        let text_id = inline_id(&text).clone();
        base.blocks.push(Block {
            id: StableId::parse("block-1").unwrap(),
            kind: BlockKind::Paragraph,
            content: vec![text],
            properties: Vec::new(),
        });

        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::UpdateLinkHref {
                    inline_id: text_id,
                    href: "https://example.invalid/new".to_string(),
                },
            }]],
        )
        .unwrap();

        assert_eq!(result.warnings[0].code, "non-link-inline");
        assert_eq!(result.document.visible_text(), "not a link\n");
    }

    #[test]
    fn list_item_properties_update_by_stable_block_id() {
        let mut base = Document::new("Doc");
        let block_id = StableId::parse("list-1").unwrap();
        base.blocks.push(Block {
            id: block_id.clone(),
            kind: BlockKind::ListItem {
                list_id: StableId::parse("list-main").unwrap(),
                level: 0,
                ordered: false,
            },
            content: vec![Inline::text("item")],
            properties: Vec::new(),
        });

        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::UpdateListItem {
                    block_id,
                    level: 2,
                    ordered: true,
                },
            }]],
        )
        .unwrap();

        match &result.document.blocks[0].kind {
            BlockKind::ListItem { level, ordered, .. } => {
                assert_eq!((*level, *ordered), (2, true));
            }
            _ => unreachable!(),
        }
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn list_item_update_warns_for_non_list_block() {
        let mut base = Document::new("Doc");
        let block = Block::paragraph("not list");
        let block_id = block.id.clone();
        base.blocks.push(block);

        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::UpdateListItem {
                    block_id,
                    level: 1,
                    ordered: true,
                },
            }]],
        )
        .unwrap();

        assert_eq!(result.warnings[0].code, "non-list-item-block");
        assert_eq!(result.document.visible_text(), "not list\n");
    }

    #[test]
    fn invalid_list_item_level_update_degrades_to_warning() {
        let mut base = Document::new("Doc");
        let block_id = StableId::parse("list-1").unwrap();
        base.blocks.push(Block {
            id: block_id.clone(),
            kind: BlockKind::ListItem {
                list_id: StableId::parse("list-main").unwrap(),
                level: 0,
                ordered: false,
            },
            content: vec![Inline::text("item")],
            properties: Vec::new(),
        });

        let invalid = Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::UpdateListItem {
                block_id: block_id.clone(),
                level: 9,
                ordered: true,
            },
        };
        let valid = Operation {
            id: OperationId {
                actor: ActorId("b".to_string()),
                seq: 1,
            },
            kind: OperationKind::UpdateListItem {
                block_id,
                level: 2,
                ordered: true,
            },
        };

        let actor_streams =
            merge_operations(&base, &[vec![invalid.clone()], vec![valid.clone()]]).unwrap();
        let reversed_batches = merge_operations(&base, &[vec![valid], vec![invalid]]).unwrap();

        assert_eq!(actor_streams.document, reversed_batches.document);
        assert_eq!(actor_streams.warnings, reversed_batches.warnings);
        match &actor_streams.document.blocks[0].kind {
            BlockKind::ListItem { level, ordered, .. } => {
                assert_eq!((*level, *ordered), (2, true));
            }
            _ => unreachable!(),
        }
        assert_eq!(actor_streams.warnings[0].code, "invalid-list-level");
        actor_streams.document.validate().unwrap();
    }

    #[test]
    fn invalid_inserted_structured_block_payloads_degrade_to_warnings() {
        let mut base = Document::new("Doc");
        base.blocks.push(Block::paragraph("base"));

        let invalid_blocks = vec![
            Block {
                id: StableId::parse("heading-bad").unwrap(),
                kind: BlockKind::Heading { level: 0 },
                content: vec![Inline::text("bad heading")],
                properties: Vec::new(),
            },
            Block {
                id: StableId::parse("list-bad").unwrap(),
                kind: BlockKind::ListItem {
                    list_id: StableId::parse("list-main").unwrap(),
                    level: 9,
                    ordered: false,
                },
                content: vec![Inline::text("bad list")],
                properties: Vec::new(),
            },
            Block {
                id: StableId::parse("image-bad").unwrap(),
                kind: BlockKind::Image {
                    blob_hash: "not-a-hash".to_string(),
                    alt_text: "bad image".to_string(),
                },
                content: Vec::new(),
                properties: Vec::new(),
            },
            Block {
                id: StableId::parse("equation-block-bad").unwrap(),
                kind: BlockKind::EquationBlock {
                    equation: Equation {
                        id: StableId::parse("equation-bad").unwrap(),
                        source_format: EquationSourceFormat::LatexLike,
                        source: String::new(),
                    },
                },
                content: Vec::new(),
                properties: Vec::new(),
            },
            Block {
                id: StableId::parse("link-bad").unwrap(),
                kind: BlockKind::Paragraph,
                content: vec![Inline::Link {
                    id: StableId::parse("link-empty").unwrap(),
                    text: "bad link".to_string(),
                    href: String::new(),
                    marks: Vec::new(),
                }],
                properties: Vec::new(),
            },
            Block {
                id: StableId::parse("mention-bad").unwrap(),
                kind: BlockKind::Paragraph,
                content: vec![Inline::Mention {
                    id: StableId::parse("mention-empty").unwrap(),
                    label: " ".to_string(),
                }],
                properties: Vec::new(),
            },
            Block {
                id: StableId::parse("inline-equation-bad-block").unwrap(),
                kind: BlockKind::Paragraph,
                content: vec![Inline::Equation {
                    id: StableId::parse("inline-equation-empty").unwrap(),
                    equation: Equation {
                        id: StableId::parse("inline-equation-bad").unwrap(),
                        source_format: EquationSourceFormat::LatexLike,
                        source: String::new(),
                    },
                }],
                properties: Vec::new(),
            },
            Block {
                id: StableId::parse("table-empty").unwrap(),
                kind: BlockKind::Table { rows: Vec::new() },
                content: Vec::new(),
                properties: Vec::new(),
            },
        ];

        let operations = invalid_blocks
            .into_iter()
            .enumerate()
            .map(|(index, block)| Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: index as u64 + 1,
                },
                kind: OperationKind::InsertBlock { after: None, block },
            })
            .collect::<Vec<_>>();

        let result = merge_operations(&base, &[operations]).unwrap();
        let warning_codes = result
            .warnings
            .iter()
            .map(|warning| warning.code.as_str())
            .collect::<Vec<_>>();

        assert_eq!(result.document.visible_text(), "base\n");
        assert_eq!(
            warning_codes,
            vec![
                "invalid-heading-level",
                "invalid-list-level",
                "invalid-image-blob-hash",
                "invalid-block-equation-source",
                "invalid-link-href",
                "invalid-mention-label",
                "invalid-inline-equation-source",
                "invalid-table",
            ]
        );
        result.document.validate().unwrap();
    }

    #[test]
    fn invalid_inserted_inline_payloads_degrade_to_warnings() {
        let mut base = Document::new("Doc");
        let block = Block::paragraph("base");
        let block_id = block.id.clone();
        base.blocks.push(block);

        let invalid_inlines = vec![
            Inline::Link {
                id: StableId::parse("link-empty").unwrap(),
                text: "bad link".to_string(),
                href: String::new(),
                marks: Vec::new(),
            },
            Inline::Mention {
                id: StableId::parse("mention-empty").unwrap(),
                label: " ".to_string(),
            },
            Inline::Equation {
                id: StableId::parse("inline-equation-empty").unwrap(),
                equation: Equation {
                    id: StableId::parse("equation-empty").unwrap(),
                    source_format: EquationSourceFormat::LatexLike,
                    source: String::new(),
                },
            },
            Inline::Text {
                id: StableId::parse("color-missing").unwrap(),
                text: "bad color".to_string(),
                marks: vec![Mark {
                    kind: MarkKind::Color,
                    value: None,
                    expand: MarkExpand::Both,
                }],
            },
            Inline::Text {
                id: StableId::parse("bold-valued").unwrap(),
                text: "bad bold".to_string(),
                marks: vec![Mark {
                    kind: MarkKind::Bold,
                    value: Some("true".to_string()),
                    expand: MarkExpand::Both,
                }],
            },
        ];

        let operations = invalid_inlines
            .into_iter()
            .enumerate()
            .map(|(index, inline)| Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: index as u64 + 1,
                },
                kind: OperationKind::InsertInline {
                    block_id: block_id.clone(),
                    after: None,
                    inline,
                },
            })
            .collect::<Vec<_>>();

        let result = merge_operations(&base, &[operations]).unwrap();
        let warning_codes = result
            .warnings
            .iter()
            .map(|warning| warning.code.as_str())
            .collect::<Vec<_>>();

        assert_eq!(result.document.visible_text(), "base\n");
        assert_eq!(
            warning_codes,
            vec![
                "invalid-link-href",
                "invalid-mention-label",
                "invalid-inline-equation-source",
                "invalid-mark-value",
                "invalid-mark-value",
            ]
        );
        result.document.validate().unwrap();
    }

    #[test]
    fn heading_level_updates_by_stable_block_id() {
        let mut base = Document::new("Doc");
        let block_id = StableId::parse("heading-1").unwrap();
        base.blocks.push(Block {
            id: block_id.clone(),
            kind: BlockKind::Heading { level: 2 },
            content: vec![Inline::text("Heading")],
            properties: Vec::new(),
        });

        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::UpdateHeadingLevel { block_id, level: 4 },
            }]],
        )
        .unwrap();

        match &result.document.blocks[0].kind {
            BlockKind::Heading { level } => assert_eq!(*level, 4),
            _ => unreachable!(),
        }
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn invalid_heading_level_update_degrades_to_warning() {
        let mut base = Document::new("Doc");
        let block_id = StableId::parse("heading-1").unwrap();
        base.blocks.push(Block {
            id: block_id.clone(),
            kind: BlockKind::Heading { level: 2 },
            content: vec![Inline::text("Heading")],
            properties: Vec::new(),
        });

        let invalid = Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::UpdateHeadingLevel {
                block_id: block_id.clone(),
                level: 0,
            },
        };
        let valid = Operation {
            id: OperationId {
                actor: ActorId("b".to_string()),
                seq: 1,
            },
            kind: OperationKind::UpdateHeadingLevel { block_id, level: 4 },
        };

        let actor_streams =
            merge_operations(&base, &[vec![invalid.clone()], vec![valid.clone()]]).unwrap();
        let reversed_batches = merge_operations(&base, &[vec![valid], vec![invalid]]).unwrap();

        assert_eq!(actor_streams.document, reversed_batches.document);
        assert_eq!(actor_streams.warnings, reversed_batches.warnings);
        match &actor_streams.document.blocks[0].kind {
            BlockKind::Heading { level } => assert_eq!(*level, 4),
            _ => unreachable!(),
        }
        assert_eq!(actor_streams.warnings[0].code, "invalid-heading-level");
        actor_streams.document.validate().unwrap();
    }

    #[test]
    fn heading_level_update_warns_for_non_heading_block() {
        let mut base = Document::new("Doc");
        let block = Block::paragraph("not heading");
        let block_id = block.id.clone();
        base.blocks.push(block);

        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::UpdateHeadingLevel { block_id, level: 3 },
            }]],
        )
        .unwrap();

        assert_eq!(result.warnings[0].code, "non-heading-block");
        assert_eq!(result.document.visible_text(), "not heading\n");
    }

    #[test]
    fn insert_inline_targets_nested_table_cell_blocks() {
        let mut base = Document::new("Doc");
        let nested = Block::paragraph("cell");
        let nested_block_id = nested.id.clone();
        let after = inline_id(&nested.content[0]).clone();
        base.blocks.push(Block {
            id: StableId::parse("table-block").unwrap(),
            kind: BlockKind::Table {
                rows: vec![TableRow {
                    id: StableId::parse("row-1").unwrap(),
                    cells: vec![opendoc_core::TableCell {
                        id: StableId::parse("cell-1").unwrap(),
                        blocks: vec![nested],
                        properties: Vec::new(),
                    }],
                }],
            },
            content: Vec::new(),
            properties: Vec::new(),
        });

        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::InsertInline {
                    block_id: nested_block_id,
                    after: Some(after),
                    inline: Inline::text(" plus"),
                },
            }]],
        )
        .unwrap();

        assert_eq!(result.document.visible_text(), "cell plus\n");
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn nearest_block_comment_anchor_resolves_inside_table_cell() {
        let mut base = Document::new("Doc");
        let nested = Block::paragraph("cell");
        let nested_block_id = nested.id.clone();
        base.blocks.push(Block {
            id: StableId::parse("table-block").unwrap(),
            kind: BlockKind::Table {
                rows: vec![TableRow {
                    id: StableId::parse("row-1").unwrap(),
                    cells: vec![opendoc_core::TableCell {
                        id: StableId::parse("cell-1").unwrap(),
                        blocks: vec![nested],
                        properties: Vec::new(),
                    }],
                }],
            },
            content: Vec::new(),
            properties: Vec::new(),
        });

        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::AddCommentThread {
                    thread: CommentThread {
                        id: StableId::parse("comment-thread-nested").unwrap(),
                        anchor: Anchor::NearestBlock {
                            block_id: nested_block_id.clone(),
                            warning: "nearest block anchor".to_string(),
                        },
                        comments: vec![Comment {
                            id: StableId::parse("comment-nested").unwrap(),
                            author: "Reviewer".to_string(),
                            body: vec![Inline::text("nested comment")],
                            created_at_ms: 1,
                            deleted: false,
                        }],
                        deleted: false,
                    },
                },
            }]],
        )
        .unwrap();

        assert_eq!(
            result.document.comments[0].anchor,
            Anchor::NearestBlock {
                block_id: nested_block_id,
                warning: "nearest block anchor".to_string()
            }
        );
        assert!(result.warnings.is_empty());
        result.document.validate().unwrap();
    }

    #[test]
    fn nearest_block_suggestion_anchor_resolves_inside_table_cell() {
        let mut base = Document::new("Doc");
        let nested = Block::paragraph("cell");
        let nested_block_id = nested.id.clone();
        base.blocks.push(Block {
            id: StableId::parse("table-block").unwrap(),
            kind: BlockKind::Table {
                rows: vec![TableRow {
                    id: StableId::parse("row-1").unwrap(),
                    cells: vec![opendoc_core::TableCell {
                        id: StableId::parse("cell-1").unwrap(),
                        blocks: vec![nested],
                        properties: Vec::new(),
                    }],
                }],
            },
            content: Vec::new(),
            properties: Vec::new(),
        });

        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::AddSuggestion {
                    suggestion: Suggestion {
                        id: StableId::parse("suggestion-nested").unwrap(),
                        author: "Editor".to_string(),
                        kind: SuggestionKind::Insert {
                            anchor: Anchor::NearestBlock {
                                block_id: nested_block_id.clone(),
                                warning: "nearest block anchor".to_string(),
                            },
                            content: vec![Inline::text(" added")],
                        },
                        state: SuggestionState::Proposed,
                        provenance: Vec::new(),
                    },
                },
            }]],
        )
        .unwrap();

        match &result.document.suggestions[0].kind {
            SuggestionKind::Insert { anchor, .. } => assert_eq!(
                anchor,
                &Anchor::NearestBlock {
                    block_id: nested_block_id,
                    warning: "nearest block anchor".to_string()
                }
            ),
            other => panic!("expected insert suggestion, got {other:?}"),
        }
        assert!(result.warnings.is_empty());
        result.document.validate().unwrap();
    }

    #[test]
    fn concurrent_table_row_inserts_converge_by_operation_id() {
        let mut base = Document::new("Doc");
        let table_block_id = StableId::parse("table-block").unwrap();
        let first_row_id = StableId::parse("row-1").unwrap();
        base.blocks.push(Block {
            id: table_block_id.clone(),
            kind: BlockKind::Table {
                rows: vec![table_row(&first_row_id, "one")],
            },
            content: Vec::new(),
            properties: Vec::new(),
        });
        let row_a_id = StableId::parse("row-a").unwrap();
        let row_b_id = StableId::parse("row-b").unwrap();
        let insert_a = Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::InsertTableRow {
                table_block_id: table_block_id.clone(),
                after_row: Some(first_row_id.clone()),
                row: table_row(&row_a_id, "two"),
            },
        };
        let insert_b = Operation {
            id: OperationId {
                actor: ActorId("b".to_string()),
                seq: 1,
            },
            kind: OperationKind::InsertTableRow {
                table_block_id,
                after_row: Some(first_row_id),
                row: table_row(&row_b_id, "three"),
            },
        };

        let merged_ab =
            merge_operations(&base, &[vec![insert_a.clone()], vec![insert_b.clone()]]).unwrap();
        let merged_ba = merge_operations(&base, &[vec![insert_b], vec![insert_a]]).unwrap();

        assert_eq!(merged_ab.document, merged_ba.document);
        assert_eq!(merged_ab.document.visible_text(), "one\nthree\ntwo\n");
        assert!(merged_ab.warnings.is_empty());
    }

    #[test]
    fn duplicate_table_row_insert_degrades_to_warning() {
        let mut base = Document::new("Doc");
        let table_block_id = StableId::parse("table-block").unwrap();
        let first_row_id = StableId::parse("row-1").unwrap();
        base.blocks.push(Block {
            id: table_block_id.clone(),
            kind: BlockKind::Table {
                rows: vec![table_row(&first_row_id, "one")],
            },
            content: Vec::new(),
            properties: Vec::new(),
        });

        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::InsertTableRow {
                    table_block_id,
                    after_row: None,
                    row: table_row(&first_row_id, "duplicate"),
                },
            }]],
        )
        .unwrap();

        assert_eq!(result.document.visible_text(), "one\n");
        assert_eq!(result.warnings[0].code, "duplicate-table-row");
        result.document.validate().unwrap();
    }

    #[test]
    fn invalid_table_row_insert_degrades_to_warning() {
        let mut base = Document::new("Doc");
        let table_block_id = StableId::parse("table-block").unwrap();
        let first_row_id = StableId::parse("row-1").unwrap();
        base.blocks.push(Block {
            id: table_block_id.clone(),
            kind: BlockKind::Table {
                rows: vec![table_row(&first_row_id, "one")],
            },
            content: Vec::new(),
            properties: Vec::new(),
        });

        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::InsertTableRow {
                    table_block_id,
                    after_row: None,
                    row: TableRow {
                        id: StableId::parse("row-empty").unwrap(),
                        cells: Vec::new(),
                    },
                },
            }]],
        )
        .unwrap();

        assert_eq!(result.document.visible_text(), "one\n");
        assert_eq!(result.warnings[0].code, "invalid-table-row");
        result.document.validate().unwrap();
    }

    #[test]
    fn invalid_nested_table_row_payload_degrades_to_warning() {
        let mut base = Document::new("Doc");
        let table_block_id = StableId::parse("table-block").unwrap();
        let first_row_id = StableId::parse("row-1").unwrap();
        base.blocks.push(Block {
            id: table_block_id.clone(),
            kind: BlockKind::Table {
                rows: vec![table_row(&first_row_id, "one")],
            },
            content: Vec::new(),
            properties: Vec::new(),
        });

        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::InsertTableRow {
                    table_block_id,
                    after_row: None,
                    row: TableRow {
                        id: StableId::parse("row-bad-nested").unwrap(),
                        cells: vec![TableCell {
                            id: StableId::parse("cell-bad-nested").unwrap(),
                            blocks: vec![Block {
                                id: StableId::parse("heading-bad-nested").unwrap(),
                                kind: BlockKind::Heading { level: 0 },
                                content: vec![Inline::text("bad")],
                                properties: Vec::new(),
                            }],
                            properties: Vec::new(),
                        }],
                    },
                },
            }]],
        )
        .unwrap();

        assert_eq!(result.document.visible_text(), "one\n");
        assert_eq!(result.warnings[0].code, "invalid-heading-level");
        result.document.validate().unwrap();
    }

    #[test]
    fn table_row_delete_removes_current_state_but_keeps_document_valid() {
        let mut base = Document::new("Doc");
        let table_block_id = StableId::parse("table-block").unwrap();
        let first_row_id = StableId::parse("row-1").unwrap();
        let second_row_id = StableId::parse("row-2").unwrap();
        base.blocks.push(Block {
            id: table_block_id.clone(),
            kind: BlockKind::Table {
                rows: vec![
                    table_row(&first_row_id, "one"),
                    table_row(&second_row_id, "two"),
                ],
            },
            content: Vec::new(),
            properties: Vec::new(),
        });

        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::DeleteTableRow {
                    table_block_id,
                    row_id: first_row_id,
                },
            }]],
        )
        .unwrap();

        assert_eq!(result.document.visible_text(), "two\n");
        assert!(result.document.validate().is_ok());
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn table_row_delete_keeps_placeholder_when_last_row_is_deleted() {
        let mut base = Document::new("Doc");
        let table_block_id = StableId::parse("table-block").unwrap();
        let row_id = StableId::parse("row-1").unwrap();
        base.blocks.push(Block {
            id: table_block_id.clone(),
            kind: BlockKind::Table {
                rows: vec![table_row(&row_id, "only")],
            },
            content: Vec::new(),
            properties: Vec::new(),
        });

        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::DeleteTableRow {
                    table_block_id,
                    row_id,
                },
            }]],
        )
        .unwrap();

        assert_eq!(result.document.visible_text(), "\n");
        assert_eq!(result.warnings[0].code, "table-row-delete-degraded");
        let BlockKind::Table { rows } = &result.document.blocks[0].kind else {
            unreachable!();
        };
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].cells.len(), 1);
        assert_ne!(rows[0].id.as_str(), "row-1");
        result.document.validate().unwrap();
    }

    #[test]
    fn table_row_insert_appends_when_anchor_row_is_missing() {
        let mut base = Document::new("Doc");
        let table_block_id = StableId::parse("table-block").unwrap();
        let first_row_id = StableId::parse("row-1").unwrap();
        base.blocks.push(Block {
            id: table_block_id.clone(),
            kind: BlockKind::Table {
                rows: vec![table_row(&first_row_id, "one")],
            },
            content: Vec::new(),
            properties: Vec::new(),
        });

        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::InsertTableRow {
                    table_block_id,
                    after_row: Some(StableId::parse("missing-row").unwrap()),
                    row: table_row(&StableId::parse("row-2").unwrap(), "two"),
                },
            }]],
        )
        .unwrap();

        assert_eq!(result.document.visible_text(), "one\ntwo\n");
        assert_eq!(result.warnings[0].code, "table-row-anchor-degraded");
        result.document.validate().unwrap();
    }

    #[test]
    fn concurrent_table_cell_inserts_converge_by_operation_id() {
        let mut base = Document::new("Doc");
        let table_block_id = StableId::parse("table-block").unwrap();
        let row_id = StableId::parse("row-1").unwrap();
        let first_cell_id = StableId::parse("cell-1").unwrap();
        base.blocks.push(Block {
            id: table_block_id.clone(),
            kind: BlockKind::Table {
                rows: vec![TableRow {
                    id: row_id.clone(),
                    cells: vec![table_cell(&first_cell_id, "one")],
                }],
            },
            content: Vec::new(),
            properties: Vec::new(),
        });
        let cell_a_id = StableId::parse("cell-a").unwrap();
        let cell_b_id = StableId::parse("cell-b").unwrap();
        let insert_a = Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::InsertTableCell {
                table_block_id: table_block_id.clone(),
                row_id: row_id.clone(),
                after_cell: Some(first_cell_id.clone()),
                cell: table_cell(&cell_a_id, "two"),
            },
        };
        let insert_b = Operation {
            id: OperationId {
                actor: ActorId("b".to_string()),
                seq: 1,
            },
            kind: OperationKind::InsertTableCell {
                table_block_id,
                row_id,
                after_cell: Some(first_cell_id),
                cell: table_cell(&cell_b_id, "three"),
            },
        };

        let merged_ab =
            merge_operations(&base, &[vec![insert_a.clone()], vec![insert_b.clone()]]).unwrap();
        let merged_ba = merge_operations(&base, &[vec![insert_b], vec![insert_a]]).unwrap();

        assert_eq!(merged_ab.document, merged_ba.document);
        assert_eq!(merged_ab.document.visible_text(), "one\tthree\ttwo\n");
        assert!(merged_ab.warnings.is_empty());
    }

    #[test]
    fn duplicate_table_cell_insert_degrades_to_warning() {
        let mut base = Document::new("Doc");
        let table_block_id = StableId::parse("table-block").unwrap();
        let row_id = StableId::parse("row-1").unwrap();
        let first_cell_id = StableId::parse("cell-1").unwrap();
        base.blocks.push(Block {
            id: table_block_id.clone(),
            kind: BlockKind::Table {
                rows: vec![TableRow {
                    id: row_id.clone(),
                    cells: vec![table_cell(&first_cell_id, "one")],
                }],
            },
            content: Vec::new(),
            properties: Vec::new(),
        });

        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::InsertTableCell {
                    table_block_id,
                    row_id,
                    after_cell: None,
                    cell: table_cell(&first_cell_id, "duplicate"),
                },
            }]],
        )
        .unwrap();

        assert_eq!(result.document.visible_text(), "one\n");
        assert_eq!(result.warnings[0].code, "duplicate-table-cell");
        result.document.validate().unwrap();
    }

    #[test]
    fn invalid_table_cell_insert_degrades_to_warning() {
        let mut base = Document::new("Doc");
        let table_block_id = StableId::parse("table-block").unwrap();
        let row_id = StableId::parse("row-1").unwrap();
        let first_cell_id = StableId::parse("cell-1").unwrap();
        base.blocks.push(Block {
            id: table_block_id.clone(),
            kind: BlockKind::Table {
                rows: vec![TableRow {
                    id: row_id.clone(),
                    cells: vec![table_cell(&first_cell_id, "one")],
                }],
            },
            content: Vec::new(),
            properties: Vec::new(),
        });

        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::InsertTableCell {
                    table_block_id,
                    row_id,
                    after_cell: None,
                    cell: TableCell {
                        id: StableId::parse("cell-empty").unwrap(),
                        blocks: Vec::new(),
                        properties: Vec::new(),
                    },
                },
            }]],
        )
        .unwrap();

        assert_eq!(result.document.visible_text(), "one\n");
        assert_eq!(result.warnings[0].code, "invalid-table-cell");
        result.document.validate().unwrap();
    }

    #[test]
    fn table_cell_delete_removes_current_state_but_keeps_row_valid() {
        let mut base = Document::new("Doc");
        let table_block_id = StableId::parse("table-block").unwrap();
        let row_id = StableId::parse("row-1").unwrap();
        let first_cell_id = StableId::parse("cell-1").unwrap();
        let second_cell_id = StableId::parse("cell-2").unwrap();
        base.blocks.push(Block {
            id: table_block_id.clone(),
            kind: BlockKind::Table {
                rows: vec![TableRow {
                    id: row_id.clone(),
                    cells: vec![
                        table_cell(&first_cell_id, "one"),
                        table_cell(&second_cell_id, "two"),
                    ],
                }],
            },
            content: Vec::new(),
            properties: Vec::new(),
        });

        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::DeleteTableCell {
                    table_block_id,
                    row_id,
                    cell_id: first_cell_id,
                },
            }]],
        )
        .unwrap();

        assert_eq!(result.document.visible_text(), "two\n");
        assert!(result.document.validate().is_ok());
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn table_cell_delete_keeps_placeholder_when_last_cell_is_deleted() {
        let mut base = Document::new("Doc");
        let table_block_id = StableId::parse("table-block").unwrap();
        let row_id = StableId::parse("row-1").unwrap();
        let cell_id = StableId::parse("cell-1").unwrap();
        base.blocks.push(Block {
            id: table_block_id.clone(),
            kind: BlockKind::Table {
                rows: vec![TableRow {
                    id: row_id.clone(),
                    cells: vec![table_cell(&cell_id, "only")],
                }],
            },
            content: Vec::new(),
            properties: Vec::new(),
        });

        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::DeleteTableCell {
                    table_block_id,
                    row_id,
                    cell_id,
                },
            }]],
        )
        .unwrap();

        assert_eq!(result.document.visible_text(), "\n");
        assert_eq!(result.warnings[0].code, "table-cell-delete-degraded");
        let BlockKind::Table { rows } = &result.document.blocks[0].kind else {
            unreachable!();
        };
        assert_eq!(rows[0].cells.len(), 1);
        assert_ne!(rows[0].cells[0].id.as_str(), "cell-1");
        result.document.validate().unwrap();
    }

    #[test]
    fn table_cell_insert_appends_when_anchor_cell_is_missing() {
        let mut base = Document::new("Doc");
        let table_block_id = StableId::parse("table-block").unwrap();
        let row_id = StableId::parse("row-1").unwrap();
        let first_cell_id = StableId::parse("cell-1").unwrap();
        base.blocks.push(Block {
            id: table_block_id.clone(),
            kind: BlockKind::Table {
                rows: vec![TableRow {
                    id: row_id.clone(),
                    cells: vec![table_cell(&first_cell_id, "one")],
                }],
            },
            content: Vec::new(),
            properties: Vec::new(),
        });

        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::InsertTableCell {
                    table_block_id,
                    row_id,
                    after_cell: Some(StableId::parse("missing-cell").unwrap()),
                    cell: table_cell(&StableId::parse("cell-2").unwrap(), "two"),
                },
            }]],
        )
        .unwrap();

        assert_eq!(result.document.visible_text(), "one\ttwo\n");
        assert_eq!(result.warnings[0].code, "table-cell-anchor-degraded");
        result.document.validate().unwrap();
    }

    #[test]
    fn table_block_delete_beats_stale_row_and_cell_edits_without_resurrection() {
        let mut base = Document::new("Doc");
        let table_block_id = StableId::parse("table-block").unwrap();
        let row_id = StableId::parse("row-1").unwrap();
        let cell_id = StableId::parse("cell-1").unwrap();
        base.blocks.push(Block {
            id: table_block_id.clone(),
            kind: BlockKind::Table {
                rows: vec![TableRow {
                    id: row_id.clone(),
                    cells: vec![table_cell(&cell_id, "one")],
                }],
            },
            content: Vec::new(),
            properties: Vec::new(),
        });

        let delete_table = Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::DeleteBlock {
                block_id: table_block_id.clone(),
            },
        };
        let stale_row_insert = Operation {
            id: OperationId {
                actor: ActorId("b".to_string()),
                seq: 1,
            },
            kind: OperationKind::InsertTableRow {
                table_block_id: table_block_id.clone(),
                after_row: Some(row_id.clone()),
                row: table_row(&StableId::parse("row-stale").unwrap(), "two"),
            },
        };
        let stale_cell_delete = Operation {
            id: OperationId {
                actor: ActorId("c".to_string()),
                seq: 1,
            },
            kind: OperationKind::DeleteTableCell {
                table_block_id: table_block_id.clone(),
                row_id,
                cell_id,
            },
        };

        let actor_streams = merge_operations(
            &base,
            &[
                vec![delete_table.clone()],
                vec![stale_row_insert.clone()],
                vec![stale_cell_delete.clone()],
            ],
        )
        .unwrap();
        let reversed_batches = merge_operations(
            &base,
            &[
                vec![stale_cell_delete],
                vec![stale_row_insert],
                vec![delete_table],
            ],
        )
        .unwrap();

        assert_eq!(actor_streams.document, reversed_batches.document);
        assert_eq!(actor_streams.warnings, reversed_batches.warnings);
        assert!(actor_streams.document.blocks.is_empty());
        assert_eq!(
            actor_streams
                .warnings
                .iter()
                .filter(|warning| warning.code == "missing-block")
                .count(),
            2
        );
        assert!(actor_streams
            .warnings
            .iter()
            .all(|warning| warning.message.contains(&table_block_id.to_string())));
        actor_streams.document.validate().unwrap();
    }

    #[test]
    fn table_row_delete_beats_stale_nested_text_and_mark_edits_without_resurrection() {
        let mut base = Document::new("Doc");
        let table_block_id = StableId::parse("table-row-nested-delete-table").unwrap();
        let deleted_row_id = StableId::parse("row-deleted-nested").unwrap();
        let deleted_cell_id = StableId::parse("cell-deleted-nested").unwrap();
        let deleted_block_id = StableId::parse("paragraph-deleted-nested").unwrap();
        let deleted_text_id = StableId::parse("text-deleted-nested").unwrap();
        let survivor_row_id = StableId::parse("row-survives-nested").unwrap();
        let survivor_cell_id = StableId::parse("cell-row-survives-nested").unwrap();
        base.blocks.push(Block {
            id: table_block_id.clone(),
            kind: BlockKind::Table {
                rows: vec![
                    TableRow {
                        id: deleted_row_id.clone(),
                        cells: vec![TableCell {
                            id: deleted_cell_id,
                            blocks: vec![Block {
                                id: deleted_block_id,
                                kind: BlockKind::Paragraph,
                                content: vec![Inline::Text {
                                    id: deleted_text_id.clone(),
                                    text: "stale".to_string(),
                                    marks: Vec::new(),
                                }],
                                properties: Vec::new(),
                            }],
                            properties: Vec::new(),
                        }],
                    },
                    table_row(&survivor_row_id, "survivor"),
                ],
            },
            content: Vec::new(),
            properties: Vec::new(),
        });

        let delete_row = Operation {
            id: OperationId {
                actor: ActorId("actor-a".to_string()),
                seq: 1,
            },
            kind: OperationKind::DeleteTableRow {
                table_block_id: table_block_id.clone(),
                row_id: deleted_row_id,
            },
        };
        let stale_text_update = Operation {
            id: OperationId {
                actor: ActorId("actor-b".to_string()),
                seq: 1,
            },
            kind: OperationKind::UpdateInlineText {
                inline_id: deleted_text_id.clone(),
                text: "resurrected".to_string(),
            },
        };
        let stale_mark = Operation {
            id: OperationId {
                actor: ActorId("actor-c".to_string()),
                seq: 1,
            },
            kind: OperationKind::AddMarkRange {
                range: TextRange {
                    start: deleted_text_id.clone(),
                    end: deleted_text_id,
                },
                mark: Mark {
                    kind: MarkKind::Bold,
                    value: None,
                    expand: MarkExpand::Both,
                },
            },
        };

        let actor_streams = merge_operations(
            &base,
            &[
                vec![delete_row.clone()],
                vec![stale_text_update.clone()],
                vec![stale_mark.clone()],
            ],
        )
        .unwrap();
        let reversed_batches = merge_operations(
            &base,
            &[vec![stale_mark], vec![stale_text_update], vec![delete_row]],
        )
        .unwrap();

        assert_eq!(actor_streams.document, reversed_batches.document);
        assert_eq!(actor_streams.warnings, reversed_batches.warnings);
        assert_eq!(actor_streams.document.visible_text(), "survivor\n");
        assert!(actor_streams
            .warnings
            .iter()
            .any(|warning| warning.code == "missing-inline"));
        assert!(actor_streams
            .warnings
            .iter()
            .any(|warning| warning.code == "missing-text-range"));
        actor_streams.document.validate().unwrap();
        let BlockKind::Table { rows } = &actor_streams.document.blocks[0].kind else {
            panic!("expected table after deleting only one row");
        };
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, survivor_row_id);
        assert_eq!(rows[0].cells[0].id, survivor_cell_id);
    }

    #[test]
    fn concurrent_overlapping_mark_ranges_converge_without_dom_spans() {
        let mut base = Document::new("Doc");
        let mut block = Block::paragraph("");
        block.content.clear();
        let first = Inline::text("alpha ");
        let first_id = inline_id(&first).clone();
        let second = Inline::text("beta ");
        let second_id = inline_id(&second).clone();
        let third = Inline::text("gamma");
        let third_id = inline_id(&third).clone();
        block.content.extend([first, second, third]);
        base.blocks.push(block);

        let bold = Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::AddMarkRange {
                range: TextRange {
                    start: first_id.clone(),
                    end: second_id.clone(),
                },
                mark: Mark {
                    kind: MarkKind::Bold,
                    value: None,
                    expand: MarkExpand::Both,
                },
            },
        };
        let italic = Operation {
            id: OperationId {
                actor: ActorId("b".to_string()),
                seq: 1,
            },
            kind: OperationKind::AddMarkRange {
                range: TextRange {
                    start: second_id.clone(),
                    end: third_id.clone(),
                },
                mark: Mark {
                    kind: MarkKind::Italic,
                    value: None,
                    expand: MarkExpand::Both,
                },
            },
        };

        let merged_ab =
            merge_operations(&base, &[vec![bold.clone()], vec![italic.clone()]]).unwrap();
        let merged_ba = merge_operations(&base, &[vec![italic], vec![bold]]).unwrap();
        assert_eq!(merged_ab.document, merged_ba.document);
        assert!(merged_ab.warnings.is_empty());
        let content = &merged_ab.document.blocks[0].content;
        assert_mark_kinds(&content[0], &[MarkKind::Bold]);
        assert_mark_kinds(&content[1], &[MarkKind::Bold, MarkKind::Italic]);
        assert_mark_kinds(&content[2], &[MarkKind::Italic]);
    }

    #[test]
    fn inserted_inline_inside_mark_range_inherits_range_format_independent_of_order() {
        let mut base = Document::new("Doc");
        let mut block = Block::paragraph("");
        block.content.clear();
        let first = Inline::text("alpha ");
        let first_id = inline_id(&first).clone();
        let second = Inline::text("omega");
        let second_id = inline_id(&second).clone();
        block.content.extend([first, second]);
        let block_id = block.id.clone();
        base.blocks.push(block);

        let inserted = Inline::text("middle ");
        let inserted_id = inline_id(&inserted).clone();
        let insert = Operation {
            id: OperationId {
                actor: ActorId("z".to_string()),
                seq: 1,
            },
            kind: OperationKind::InsertInline {
                block_id,
                after: Some(first_id.clone()),
                inline: inserted,
            },
        };
        let format = Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::AddMarkRange {
                range: TextRange {
                    start: first_id,
                    end: second_id,
                },
                mark: Mark {
                    kind: MarkKind::Bold,
                    value: None,
                    expand: MarkExpand::Both,
                },
            },
        };

        let merged_format_first =
            merge_operations(&base, &[vec![format.clone()], vec![insert.clone()]]).unwrap();
        let merged_insert_first = merge_operations(&base, &[vec![insert], vec![format]]).unwrap();
        assert_eq!(merged_format_first.document, merged_insert_first.document);
        let inserted = merged_format_first.document.blocks[0]
            .content
            .iter()
            .find(|inline| inline_id(inline) == &inserted_id)
            .unwrap();
        assert_mark_kinds(inserted, &[MarkKind::Bold]);
    }

    #[test]
    fn mark_range_degrades_deterministically_when_endpoint_was_deleted() {
        let mut base = Document::new("Doc");
        let mut block = Block::paragraph("");
        block.content.clear();
        let first = Inline::text("alpha ");
        let first_id = inline_id(&first).clone();
        let second = Inline::text("beta");
        let second_id = inline_id(&second).clone();
        block.content.extend([first, second]);
        base.blocks.push(block);

        let result = merge_operations(
            &base,
            &[
                vec![Operation {
                    id: OperationId {
                        actor: ActorId("a".to_string()),
                        seq: 1,
                    },
                    kind: OperationKind::DeleteInline {
                        inline_id: first_id.clone(),
                    },
                }],
                vec![Operation {
                    id: OperationId {
                        actor: ActorId("b".to_string()),
                        seq: 1,
                    },
                    kind: OperationKind::AddMarkRange {
                        range: TextRange {
                            start: first_id,
                            end: second_id,
                        },
                        mark: Mark {
                            kind: MarkKind::Bold,
                            value: None,
                            expand: MarkExpand::Both,
                        },
                    },
                }],
            ],
        )
        .unwrap();

        assert_eq!(result.warnings[0].code, "mark-range-degraded");
        assert_eq!(result.document.visible_text(), "beta\n");
        assert_mark_kinds(&result.document.blocks[0].content[0], &[MarkKind::Bold]);
    }

    #[test]
    fn paragraph_split_move_preserves_format_and_comment_range_anchors() {
        let mut base = Document::new("Doc");
        let mut block = Block::paragraph("");
        block.content.clear();
        let first = Inline::text("alpha ");
        let first_id = inline_id(&first).clone();
        let second = Inline::text("omega");
        let second_id = inline_id(&second).clone();
        block.content.extend([first, second]);
        let original_block_id = block.id.clone();
        let split_block_id = StableId::parse("paragraph-split-target").unwrap();
        base.blocks.push(block);

        let split_ops = vec![
            Operation {
                id: OperationId {
                    actor: ActorId("actor-split".to_string()),
                    seq: 1,
                },
                kind: OperationKind::InsertBlock {
                    after: Some(original_block_id.clone()),
                    block: Block {
                        id: split_block_id.clone(),
                        kind: BlockKind::Paragraph,
                        content: Vec::new(),
                        properties: Vec::new(),
                    },
                },
            },
            Operation {
                id: OperationId {
                    actor: ActorId("actor-split".to_string()),
                    seq: 2,
                },
                kind: OperationKind::MoveInlineToBlock {
                    inline_id: second_id.clone(),
                    target_block_id: split_block_id.clone(),
                    after: None,
                },
            },
        ];
        let review_ops = vec![
            Operation {
                id: OperationId {
                    actor: ActorId("actor-review".to_string()),
                    seq: 1,
                },
                kind: OperationKind::AddMarkRange {
                    range: TextRange {
                        start: first_id.clone(),
                        end: second_id.clone(),
                    },
                    mark: Mark {
                        kind: MarkKind::Bold,
                        value: None,
                        expand: MarkExpand::Both,
                    },
                },
            },
            Operation {
                id: OperationId {
                    actor: ActorId("actor-review".to_string()),
                    seq: 2,
                },
                kind: OperationKind::AddCommentThread {
                    thread: CommentThread {
                        id: StableId::parse("comment-thread-split-range").unwrap(),
                        anchor: Anchor::TextRange(TextRange {
                            start: first_id.clone(),
                            end: second_id.clone(),
                        }),
                        comments: vec![Comment {
                            id: StableId::parse("comment-split-range").unwrap(),
                            author: "Reviewer".to_string(),
                            body: vec![Inline::text("range spans split paragraphs")],
                            created_at_ms: 1,
                            deleted: false,
                        }],
                        deleted: false,
                    },
                },
            },
        ];

        let split_first =
            merge_operations(&base, &[split_ops.clone(), review_ops.clone()]).unwrap();
        let review_first = merge_operations(&base, &[review_ops, split_ops]).unwrap();

        assert_eq!(split_first.document, review_first.document);
        assert_eq!(split_first.warnings, review_first.warnings);
        assert!(split_first.warnings.is_empty());
        assert_eq!(split_first.document.visible_text(), "alpha \nomega\n");
        assert_eq!(split_first.document.blocks[0].id, original_block_id);
        assert_eq!(split_first.document.blocks[1].id, split_block_id);
        assert_mark_kinds(
            &split_first.document.blocks[0].content[0],
            &[MarkKind::Bold],
        );
        assert_mark_kinds(
            &split_first.document.blocks[1].content[0],
            &[MarkKind::Bold],
        );
        assert!(matches!(
            &split_first.document.comments[0].anchor,
            Anchor::TextRange(range) if range.start == first_id && range.end == second_id
        ));
        split_first.document.validate().unwrap();
    }

    #[test]
    fn three_actor_typing_formatting_and_split_converge_without_batching() {
        let mut base = Document::new("Doc");
        let mut block = Block::paragraph("");
        block.content.clear();
        let first = Inline::text("alpha ");
        let first_id = inline_id(&first).clone();
        let second = Inline::text("omega");
        let second_id = inline_id(&second).clone();
        block.content.extend([first, second]);
        let original_block_id = block.id.clone();
        let split_block_id = StableId::parse("paragraph-three-actor-split").unwrap();
        base.blocks.push(block);

        let inserted = Inline::text("middle ");
        let inserted_id = inline_id(&inserted).clone();
        let typing_ops = vec![Operation {
            id: OperationId {
                actor: ActorId("actor-typing".to_string()),
                seq: 1,
            },
            kind: OperationKind::InsertInline {
                block_id: original_block_id.clone(),
                after: Some(first_id.clone()),
                inline: inserted,
            },
        }];
        let split_ops = vec![
            Operation {
                id: OperationId {
                    actor: ActorId("actor-split".to_string()),
                    seq: 1,
                },
                kind: OperationKind::InsertBlock {
                    after: Some(original_block_id.clone()),
                    block: Block {
                        id: split_block_id.clone(),
                        kind: BlockKind::Paragraph,
                        content: Vec::new(),
                        properties: Vec::new(),
                    },
                },
            },
            Operation {
                id: OperationId {
                    actor: ActorId("actor-split".to_string()),
                    seq: 2,
                },
                kind: OperationKind::MoveInlineToBlock {
                    inline_id: second_id.clone(),
                    target_block_id: split_block_id.clone(),
                    after: None,
                },
            },
        ];
        let review_ops = vec![
            Operation {
                id: OperationId {
                    actor: ActorId("actor-review".to_string()),
                    seq: 1,
                },
                kind: OperationKind::AddMarkRange {
                    range: TextRange {
                        start: first_id.clone(),
                        end: second_id.clone(),
                    },
                    mark: Mark {
                        kind: MarkKind::Bold,
                        value: None,
                        expand: MarkExpand::Both,
                    },
                },
            },
            Operation {
                id: OperationId {
                    actor: ActorId("actor-review".to_string()),
                    seq: 2,
                },
                kind: OperationKind::AddCommentThread {
                    thread: CommentThread {
                        id: StableId::parse("comment-thread-three-actor-range").unwrap(),
                        anchor: Anchor::TextRange(TextRange {
                            start: first_id.clone(),
                            end: second_id.clone(),
                        }),
                        comments: vec![Comment {
                            id: StableId::parse("comment-three-actor-range").unwrap(),
                            author: "Reviewer".to_string(),
                            body: vec![Inline::text("range spans typed and split content")],
                            created_at_ms: 1,
                            deleted: false,
                        }],
                        deleted: false,
                    },
                },
            },
        ];
        let all_ops = typing_ops
            .iter()
            .chain(split_ops.iter())
            .chain(review_ops.iter())
            .cloned()
            .collect::<Vec<_>>();

        let actor_batches = merge_operations(
            &base,
            &[typing_ops.clone(), split_ops.clone(), review_ops.clone()],
        )
        .unwrap();
        let single_batch = merge_operations(&base, &[all_ops]).unwrap();
        let reversed_batches =
            merge_operations(&base, &[review_ops, split_ops, typing_ops]).unwrap();

        assert_eq!(actor_batches.document, single_batch.document);
        assert_eq!(actor_batches.document, reversed_batches.document);
        assert_eq!(actor_batches.warnings, single_batch.warnings);
        assert_eq!(actor_batches.warnings, reversed_batches.warnings);
        assert!(actor_batches.warnings.is_empty());
        assert_eq!(
            actor_batches.document.visible_text(),
            "alpha middle \nomega\n"
        );
        assert_eq!(actor_batches.document.blocks[0].id, original_block_id);
        assert_eq!(actor_batches.document.blocks[1].id, split_block_id);
        for id in [&first_id, &inserted_id] {
            let inline = actor_batches.document.blocks[0]
                .content
                .iter()
                .find(|inline| inline_id(inline) == id)
                .unwrap();
            assert_mark_kinds(inline, &[MarkKind::Bold]);
        }
        assert_mark_kinds(
            &actor_batches.document.blocks[1].content[0],
            &[MarkKind::Bold],
        );
        assert!(matches!(
            &actor_batches.document.comments[0].anchor,
            Anchor::TextRange(range) if range.start == first_id && range.end == second_id
        ));
        actor_batches.document.validate().unwrap();
    }

    #[test]
    fn paragraph_split_move_preserves_suggestion_range_anchors() {
        let mut base = Document::new("Doc");
        let mut block = Block::paragraph("");
        block.content.clear();
        let first = Inline::text("alpha ");
        let first_id = inline_id(&first).clone();
        let second = Inline::text("omega");
        let second_id = inline_id(&second).clone();
        block.content.extend([first, second]);
        let original_block_id = block.id.clone();
        let split_block_id = StableId::parse("paragraph-suggestion-split-target").unwrap();
        base.blocks.push(block);

        let split_ops = vec![
            Operation {
                id: OperationId {
                    actor: ActorId("actor-split".to_string()),
                    seq: 1,
                },
                kind: OperationKind::InsertBlock {
                    after: Some(original_block_id.clone()),
                    block: Block {
                        id: split_block_id.clone(),
                        kind: BlockKind::Paragraph,
                        content: Vec::new(),
                        properties: Vec::new(),
                    },
                },
            },
            Operation {
                id: OperationId {
                    actor: ActorId("actor-split".to_string()),
                    seq: 2,
                },
                kind: OperationKind::MoveInlineToBlock {
                    inline_id: second_id.clone(),
                    target_block_id: split_block_id.clone(),
                    after: None,
                },
            },
        ];
        let suggestion_ops = vec![
            Operation {
                id: OperationId {
                    actor: ActorId("actor-suggest".to_string()),
                    seq: 1,
                },
                kind: OperationKind::AddSuggestion {
                    suggestion: Suggestion {
                        id: StableId::parse("suggestion-delete-split-range").unwrap(),
                        author: "Reviewer".to_string(),
                        kind: SuggestionKind::Delete {
                            range: TextRange {
                                start: first_id.clone(),
                                end: second_id.clone(),
                            },
                        },
                        state: SuggestionState::Proposed,
                        provenance: Vec::new(),
                    },
                },
            },
            Operation {
                id: OperationId {
                    actor: ActorId("actor-suggest".to_string()),
                    seq: 2,
                },
                kind: OperationKind::AddSuggestion {
                    suggestion: Suggestion {
                        id: StableId::parse("suggestion-format-split-range").unwrap(),
                        author: "Reviewer".to_string(),
                        kind: SuggestionKind::Format {
                            range: TextRange {
                                start: first_id.clone(),
                                end: second_id.clone(),
                            },
                            marks: vec![Mark {
                                kind: MarkKind::Italic,
                                value: None,
                                expand: MarkExpand::Both,
                            }],
                        },
                        state: SuggestionState::Proposed,
                        provenance: Vec::new(),
                    },
                },
            },
            Operation {
                id: OperationId {
                    actor: ActorId("actor-suggest".to_string()),
                    seq: 3,
                },
                kind: OperationKind::AddSuggestion {
                    suggestion: Suggestion {
                        id: StableId::parse("suggestion-insert-split-anchor").unwrap(),
                        author: "Reviewer".to_string(),
                        kind: SuggestionKind::Insert {
                            anchor: Anchor::TextRange(TextRange {
                                start: first_id.clone(),
                                end: second_id.clone(),
                            }),
                            content: vec![Inline::text(" inserted")],
                        },
                        state: SuggestionState::Proposed,
                        provenance: Vec::new(),
                    },
                },
            },
        ];

        let split_first =
            merge_operations(&base, &[split_ops.clone(), suggestion_ops.clone()]).unwrap();
        let suggestions_first = merge_operations(&base, &[suggestion_ops, split_ops]).unwrap();

        assert_eq!(split_first.document, suggestions_first.document);
        assert_eq!(split_first.warnings, suggestions_first.warnings);
        assert!(split_first.warnings.is_empty());
        assert_eq!(split_first.document.visible_text(), "alpha \nomega\n");
        assert_eq!(split_first.document.suggestions.len(), 3);
        for suggestion in &split_first.document.suggestions {
            assert_eq!(suggestion.state, SuggestionState::Proposed);
            match &suggestion.kind {
                SuggestionKind::Delete { range } | SuggestionKind::Format { range, .. } => {
                    assert_eq!(range.start, first_id);
                    assert_eq!(range.end, second_id);
                }
                SuggestionKind::Insert { anchor, content } => {
                    assert_eq!(content.len(), 1);
                    assert!(matches!(
                        &content[0],
                        Inline::Text { text, .. } if text == " inserted"
                    ));
                    assert!(matches!(
                        anchor,
                        Anchor::TextRange(range) if range.start == first_id && range.end == second_id
                    ));
                }
            }
        }
        split_first.document.validate().unwrap();
    }

    #[test]
    fn move_inline_to_missing_block_keeps_source_and_warns() {
        let mut base = Document::new("Doc");
        let mut block = Block::paragraph("");
        block.content.clear();
        let inline = Inline::text("alpha");
        let inline_id_to_move = inline_id(&inline).clone();
        block.content.push(inline);
        base.blocks.push(block);

        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("actor-move".to_string()),
                    seq: 1,
                },
                kind: OperationKind::MoveInlineToBlock {
                    inline_id: inline_id_to_move,
                    target_block_id: StableId::parse("missing-target-block").unwrap(),
                    after: None,
                },
            }]],
        )
        .unwrap();

        assert_eq!(result.document.visible_text(), "alpha\n");
        assert!(result
            .warnings
            .iter()
            .any(|warning| warning.code == "missing-block"));
        result.document.validate().unwrap();
    }

    #[test]
    fn move_missing_inline_to_block_warns_without_mutating_target() {
        let mut base = Document::new("Doc");
        let mut block = Block::paragraph("");
        block.content.clear();
        block.content.push(Inline::text("target"));
        let target_block_id = block.id.clone();
        base.blocks.push(block);

        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("actor-move".to_string()),
                    seq: 1,
                },
                kind: OperationKind::MoveInlineToBlock {
                    inline_id: StableId::parse("missing-inline").unwrap(),
                    target_block_id,
                    after: None,
                },
            }]],
        )
        .unwrap();

        assert_eq!(result.document.visible_text(), "target\n");
        assert!(result
            .warnings
            .iter()
            .any(|warning| warning.code == "missing-inline"));
        result.document.validate().unwrap();
    }

    #[test]
    fn move_inline_to_block_with_missing_anchor_appends_and_warns() {
        let mut base = Document::new("Doc");
        let mut source = Block::paragraph("");
        source.content.clear();
        let moved = Inline::text("moved");
        let moved_id = inline_id(&moved).clone();
        source.content.push(moved);
        let mut target = Block::paragraph("");
        target.content.clear();
        target.content.push(Inline::text("target "));
        let target_block_id = target.id.clone();
        base.blocks.extend([source, target]);

        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("actor-move".to_string()),
                    seq: 1,
                },
                kind: OperationKind::MoveInlineToBlock {
                    inline_id: moved_id,
                    target_block_id,
                    after: Some(StableId::parse("missing-after-inline").unwrap()),
                },
            }]],
        )
        .unwrap();

        assert_eq!(result.document.visible_text(), "\ntarget moved\n");
        assert!(result
            .warnings
            .iter()
            .any(|warning| warning.code == "inline-anchor-degraded"));
        result.document.validate().unwrap();
    }

    #[test]
    fn paragraph_join_preserves_format_and_comment_range_anchors() {
        let mut base = Document::new("Doc");
        let mut first_block = Block::paragraph("");
        first_block.content.clear();
        let first = Inline::text("alpha ");
        let first_id = inline_id(&first).clone();
        first_block.content.push(first);
        let first_block_id = first_block.id.clone();
        let mut second_block = Block::paragraph("");
        second_block.content.clear();
        let second = Inline::text("omega");
        let second_id = inline_id(&second).clone();
        second_block.content.push(second);
        let second_block_id = second_block.id.clone();
        base.blocks.extend([first_block, second_block]);

        let join_ops = vec![
            Operation {
                id: OperationId {
                    actor: ActorId("actor-join".to_string()),
                    seq: 1,
                },
                kind: OperationKind::MoveInlineToBlock {
                    inline_id: second_id.clone(),
                    target_block_id: first_block_id.clone(),
                    after: Some(first_id.clone()),
                },
            },
            Operation {
                id: OperationId {
                    actor: ActorId("actor-join".to_string()),
                    seq: 2,
                },
                kind: OperationKind::DeleteBlock {
                    block_id: second_block_id.clone(),
                },
            },
        ];
        let review_ops = vec![
            Operation {
                id: OperationId {
                    actor: ActorId("actor-review".to_string()),
                    seq: 1,
                },
                kind: OperationKind::AddMarkRange {
                    range: TextRange {
                        start: first_id.clone(),
                        end: second_id.clone(),
                    },
                    mark: Mark {
                        kind: MarkKind::Bold,
                        value: None,
                        expand: MarkExpand::Both,
                    },
                },
            },
            Operation {
                id: OperationId {
                    actor: ActorId("actor-review".to_string()),
                    seq: 2,
                },
                kind: OperationKind::AddCommentThread {
                    thread: CommentThread {
                        id: StableId::parse("comment-thread-join-range").unwrap(),
                        anchor: Anchor::TextRange(TextRange {
                            start: first_id.clone(),
                            end: second_id.clone(),
                        }),
                        comments: vec![Comment {
                            id: StableId::parse("comment-join-range").unwrap(),
                            author: "Reviewer".to_string(),
                            body: vec![Inline::text("range spans joined paragraphs")],
                            created_at_ms: 1,
                            deleted: false,
                        }],
                        deleted: false,
                    },
                },
            },
        ];

        let join_first = merge_operations(&base, &[join_ops.clone(), review_ops.clone()]).unwrap();
        let review_first = merge_operations(&base, &[review_ops, join_ops]).unwrap();

        assert_eq!(join_first.document, review_first.document);
        assert_eq!(join_first.warnings, review_first.warnings);
        assert!(join_first.warnings.is_empty());
        assert_eq!(join_first.document.visible_text(), "alpha omega\n");
        assert_eq!(join_first.document.blocks.len(), 1);
        assert_eq!(join_first.document.blocks[0].id, first_block_id);
        assert_mark_kinds(&join_first.document.blocks[0].content[0], &[MarkKind::Bold]);
        assert_mark_kinds(&join_first.document.blocks[0].content[1], &[MarkKind::Bold]);
        assert!(matches!(
            &join_first.document.comments[0].anchor,
            Anchor::TextRange(range) if range.start == first_id && range.end == second_id
        ));
        join_first.document.validate().unwrap();
    }

    #[test]
    fn concurrent_format_citation_comment_and_delete_converge_with_degraded_anchors() {
        let mut base = Document::new("Doc");
        let mut block = Block::paragraph("");
        block.content.clear();
        let first = Inline::text("alpha ");
        let first_id = inline_id(&first).clone();
        let second = Inline::text("omega");
        let second_id = inline_id(&second).clone();
        block.content.extend([first, second]);
        let block_id = block.id.clone();
        base.blocks.push(block);

        let reference_id = StableId::parse("ref-merge-doe-2020").unwrap();
        let citation_id = StableId::parse("cite-merge-doe-2020").unwrap();
        let citation_inline_id = StableId::parse("citation-label-merge-doe-2020").unwrap();
        let delete = Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::DeleteInline {
                inline_id: first_id.clone(),
            },
        };
        let citation_ops = vec![
            Operation {
                id: OperationId {
                    actor: ActorId("b".to_string()),
                    seq: 1,
                },
                kind: OperationKind::UpsertBibliographyReference {
                    reference: BibliographyReference {
                        id: reference_id.clone(),
                        revision: 1,
                        source: CitationSource {
                            format: CitationSourceFormat::CitumNative,
                            bytes: b"title: Merge Citation".to_vec(),
                        },
                        summary: CitationSummary {
                            title: "Merge Citation".to_string(),
                            authors: vec!["Doe".to_string()],
                            issued: Some("2020".to_string()),
                            doi: None,
                            url: None,
                        },
                        deleted: false,
                    },
                },
            },
            Operation {
                id: OperationId {
                    actor: ActorId("b".to_string()),
                    seq: 2,
                },
                kind: OperationKind::UpsertCitationGroup {
                    citation: CitationGroup {
                        id: citation_id.clone(),
                        revision: 1,
                        items: vec![CitationItem {
                            reference_id: reference_id.clone(),
                            locator: None,
                            label: None,
                            prefix: None,
                            suffix: None,
                            suppress_author: false,
                        }],
                        placement: CitationPlacement::Inline,
                        rendered_cache: None,
                        deleted: false,
                    },
                },
            },
            Operation {
                id: OperationId {
                    actor: ActorId("b".to_string()),
                    seq: 3,
                },
                kind: OperationKind::InsertInline {
                    block_id: block_id.clone(),
                    after: Some(first_id.clone()),
                    inline: Inline::Citation {
                        id: citation_inline_id.clone(),
                        citation_id: citation_id.clone(),
                        rendered_cache: None,
                    },
                },
            },
        ];
        let review_ops = vec![
            Operation {
                id: OperationId {
                    actor: ActorId("c".to_string()),
                    seq: 1,
                },
                kind: OperationKind::AddMarkRange {
                    range: TextRange {
                        start: first_id.clone(),
                        end: second_id.clone(),
                    },
                    mark: Mark {
                        kind: MarkKind::Bold,
                        value: None,
                        expand: MarkExpand::Both,
                    },
                },
            },
            Operation {
                id: OperationId {
                    actor: ActorId("c".to_string()),
                    seq: 2,
                },
                kind: OperationKind::AddCommentThread {
                    thread: CommentThread {
                        id: StableId::parse("comment-thread-merge-anchor").unwrap(),
                        anchor: Anchor::TextRange(TextRange {
                            start: first_id.clone(),
                            end: second_id.clone(),
                        }),
                        comments: vec![Comment {
                            id: StableId::parse("comment-merge-anchor").unwrap(),
                            author: "Alice".to_string(),
                            body: vec![Inline::text("keep this citation checked")],
                            created_at_ms: 1,
                            deleted: false,
                        }],
                        deleted: false,
                    },
                },
            },
        ];

        let merged_abc = merge_operations(
            &base,
            &[
                vec![delete.clone()],
                citation_ops.clone(),
                review_ops.clone(),
            ],
        )
        .unwrap();
        let merged_cba =
            merge_operations(&base, &[review_ops, citation_ops, vec![delete]]).unwrap();

        assert_eq!(merged_abc.document, merged_cba.document);
        assert_eq!(merged_abc.warnings, merged_cba.warnings);
        assert_eq!(merged_abc.document.visible_text(), "omega(Doe 2020)\n");
        assert!(matches!(
            &merged_abc.document.comments[0].anchor,
            Anchor::TextRange(range) if range.start == second_id && range.end == second_id
        ));
        assert_mark_kinds(&merged_abc.document.blocks[0].content[0], &[MarkKind::Bold]);
        match &merged_abc.document.blocks[0].content[1] {
            Inline::Citation {
                citation_id: id,
                rendered_cache,
                ..
            } => {
                assert_eq!(id, &citation_id);
                assert!(rendered_cache.is_none());
            }
            other => panic!("expected citation label, got {other:?}"),
        }
        assert_eq!(
            merged_abc
                .document
                .citation_database
                .rendered_citation(&citation_id)
                .map(String::as_str),
            Some("(Doe 2020)")
        );
        assert_eq!(
            merged_abc
                .warnings
                .iter()
                .map(|warning| warning.code.as_str())
                .collect::<Vec<_>>(),
            vec![
                "inline-anchor-degraded",
                "comment-anchor-degraded",
                "mark-range-degraded"
            ]
        );
        merged_abc.document.validate().unwrap();
    }

    #[test]
    fn citation_label_text_is_not_directly_editable() {
        let mut base = Document::new("Doc");
        let reference_id = StableId::parse("ref-doe-2020").unwrap();
        let citation_id = StableId::parse("cite-intro").unwrap();
        base.citation_database
            .upsert_reference(BibliographyReference {
                id: reference_id.clone(),
                revision: 1,
                source: CitationSource {
                    format: CitationSourceFormat::CitumNative,
                    bytes: b"title: Example".to_vec(),
                },
                summary: CitationSummary {
                    title: "Example".to_string(),
                    authors: vec!["Doe".to_string()],
                    issued: Some("2020".to_string()),
                    doi: None,
                    url: None,
                },
                deleted: false,
            });
        base.citation_database.upsert_citation(CitationGroup {
            id: citation_id.clone(),
            revision: 1,
            items: vec![CitationItem {
                reference_id,
                locator: None,
                label: None,
                prefix: None,
                suffix: None,
                suppress_author: false,
            }],
            placement: CitationPlacement::Inline,
            rendered_cache: Some("(Doe 2020)".to_string()),
            deleted: false,
        });
        let citation_inline = Inline::Citation {
            id: StableId::new("citation-label"),
            citation_id,
            rendered_cache: Some("(Doe 2020)".to_string()),
        };
        let citation_inline_id = inline_id(&citation_inline).clone();
        base.blocks.push(Block {
            id: StableId::new("block"),
            kind: BlockKind::Paragraph,
            content: vec![citation_inline],
            properties: Vec::new(),
        });

        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::UpdateInlineText {
                    inline_id: citation_inline_id,
                    text: "manual edit".to_string(),
                },
            }]],
        )
        .unwrap();

        assert_eq!(result.warnings[0].code, "non-editable-inline");
        assert_eq!(result.document.visible_text(), "(Doe 2020)\n");
    }

    #[test]
    fn mention_labels_update_as_structured_inline_state() {
        let mut base = Document::new("Doc");
        let mention = Inline::Mention {
            id: StableId::parse("mention-alice").unwrap(),
            label: "@alice".to_string(),
        };
        let mention_id = inline_id(&mention).clone();
        base.blocks.push(Block {
            id: StableId::new("block"),
            kind: BlockKind::Paragraph,
            content: vec![mention],
            properties: Vec::new(),
        });

        let generic = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::UpdateInlineText {
                    inline_id: mention_id.clone(),
                    text: "@generic".to_string(),
                },
            }]],
        )
        .unwrap();
        assert_eq!(generic.warnings[0].code, "non-editable-inline");
        assert_eq!(generic.document.visible_text(), "@alice\n");

        let structured = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("b".to_string()),
                    seq: 1,
                },
                kind: OperationKind::UpdateMentionLabel {
                    inline_id: mention_id.clone(),
                    label: "@bob".to_string(),
                },
            }]],
        )
        .unwrap();
        assert!(structured.warnings.is_empty());
        assert_eq!(structured.document.visible_text(), "@bob\n");

        let invalid = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("c".to_string()),
                    seq: 1,
                },
                kind: OperationKind::UpdateMentionLabel {
                    inline_id: mention_id,
                    label: " ".to_string(),
                },
            }]],
        )
        .unwrap();
        assert_eq!(invalid.warnings[0].code, "invalid-mention-label");
        assert_eq!(invalid.document.visible_text(), "@alice\n");
    }

    #[test]
    fn inline_equation_source_updates_are_atomic_structured_operations() {
        let mut base = Document::new("Doc");
        let equation = Inline::Equation {
            id: StableId::parse("inline-equation").unwrap(),
            equation: Equation {
                id: StableId::parse("equation-source").unwrap(),
                source_format: EquationSourceFormat::LatexLike,
                source: "a=b".to_string(),
            },
        };
        let equation_id = inline_id(&equation).clone();
        base.blocks.push(Block {
            id: StableId::new("block"),
            kind: BlockKind::Paragraph,
            content: vec![equation],
            properties: Vec::new(),
        });

        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::UpdateInlineEquationSource {
                    inline_id: equation_id,
                    source: "a=c".to_string(),
                },
            }]],
        )
        .unwrap();

        match &result.document.blocks[0].content[0] {
            Inline::Equation { equation, .. } => assert_eq!(equation.source, "a=c"),
            other => panic!("expected inline equation, got {other:?}"),
        }
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn empty_inline_equation_source_update_degrades_to_warning() {
        let mut base = Document::new("Doc");
        let equation = Inline::Equation {
            id: StableId::parse("inline-equation").unwrap(),
            equation: Equation {
                id: StableId::parse("equation-source").unwrap(),
                source_format: EquationSourceFormat::LatexLike,
                source: "a=b".to_string(),
            },
        };
        let equation_id = inline_id(&equation).clone();
        base.blocks.push(Block {
            id: StableId::new("block"),
            kind: BlockKind::Paragraph,
            content: vec![equation],
            properties: Vec::new(),
        });

        let invalid = Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::UpdateInlineEquationSource {
                inline_id: equation_id.clone(),
                source: " ".to_string(),
            },
        };
        let valid = Operation {
            id: OperationId {
                actor: ActorId("b".to_string()),
                seq: 1,
            },
            kind: OperationKind::UpdateInlineEquationSource {
                inline_id: equation_id,
                source: "a=c".to_string(),
            },
        };

        let actor_streams =
            merge_operations(&base, &[vec![invalid.clone()], vec![valid.clone()]]).unwrap();
        let reversed_batches = merge_operations(&base, &[vec![valid], vec![invalid]]).unwrap();

        assert_eq!(actor_streams.document, reversed_batches.document);
        assert_eq!(actor_streams.warnings, reversed_batches.warnings);
        match &actor_streams.document.blocks[0].content[0] {
            Inline::Equation { equation, .. } => assert_eq!(equation.source, "a=c"),
            other => panic!("expected inline equation, got {other:?}"),
        }
        assert_eq!(
            actor_streams.warnings[0].code,
            "invalid-inline-equation-source"
        );
        actor_streams.document.validate().unwrap();
    }

    #[test]
    fn generic_inline_text_update_does_not_mutate_inline_equation_source() {
        let mut base = Document::new("Doc");
        let equation = Inline::Equation {
            id: StableId::parse("inline-equation").unwrap(),
            equation: Equation {
                id: StableId::parse("equation-source").unwrap(),
                source_format: EquationSourceFormat::LatexLike,
                source: "a=b".to_string(),
            },
        };
        let equation_id = inline_id(&equation).clone();
        base.blocks.push(Block {
            id: StableId::new("block"),
            kind: BlockKind::Paragraph,
            content: vec![equation],
            properties: Vec::new(),
        });

        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::UpdateInlineText {
                    inline_id: equation_id,
                    text: "a=c".to_string(),
                },
            }]],
        )
        .unwrap();

        match &result.document.blocks[0].content[0] {
            Inline::Equation { equation, .. } => assert_eq!(equation.source, "a=b"),
            other => panic!("expected inline equation, got {other:?}"),
        }
        assert_eq!(result.warnings[0].code, "non-editable-inline");
    }

    #[test]
    fn block_equation_source_updates_as_atomic_block_state() {
        let mut base = Document::new("Doc");
        let block_id = StableId::new("block");
        base.blocks.push(Block {
            id: block_id.clone(),
            kind: BlockKind::EquationBlock {
                equation: Equation {
                    id: StableId::new("eq"),
                    source_format: EquationSourceFormat::LatexLike,
                    source: "x=1".to_string(),
                },
            },
            content: Vec::new(),
            properties: Vec::new(),
        });

        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::UpdateBlockEquationSource {
                    block_id,
                    source: "x=2".to_string(),
                },
            }]],
        )
        .unwrap();

        assert_eq!(result.document.visible_text(), "x=2\n");
    }

    #[test]
    fn structured_source_updates_are_canonicalized_during_merge() {
        let mut base = Document::new("Doc");
        let link_id = StableId::parse("link-canonical").unwrap();
        let mention_id = StableId::parse("mention-canonical").unwrap();
        let inline_equation_id = StableId::parse("inline-equation-canonical").unwrap();
        let block_equation_id = StableId::parse("block-equation-canonical").unwrap();
        base.blocks.push(Block {
            id: StableId::parse("paragraph-canonical").unwrap(),
            kind: BlockKind::Paragraph,
            content: vec![
                Inline::Link {
                    id: link_id.clone(),
                    text: "paper".to_string(),
                    href: "https://example.invalid/old".to_string(),
                    marks: Vec::new(),
                },
                Inline::Mention {
                    id: mention_id.clone(),
                    label: "@old".to_string(),
                },
                Inline::Equation {
                    id: inline_equation_id.clone(),
                    equation: Equation {
                        id: StableId::parse("equation-inline-canonical").unwrap(),
                        source_format: EquationSourceFormat::LatexLike,
                        source: "a=b".to_string(),
                    },
                },
            ],
            properties: Vec::new(),
        });
        base.blocks.push(Block {
            id: block_equation_id.clone(),
            kind: BlockKind::EquationBlock {
                equation: Equation {
                    id: StableId::parse("equation-block-canonical").unwrap(),
                    source_format: EquationSourceFormat::LatexLike,
                    source: "x=1".to_string(),
                },
            },
            content: Vec::new(),
            properties: Vec::new(),
        });

        let result = merge_operations(
            &base,
            &[vec![
                Operation {
                    id: OperationId {
                        actor: ActorId("a".to_string()),
                        seq: 1,
                    },
                    kind: OperationKind::UpdateLinkHref {
                        inline_id: link_id,
                        href: " https://example.invalid/new ".to_string(),
                    },
                },
                Operation {
                    id: OperationId {
                        actor: ActorId("a".to_string()),
                        seq: 2,
                    },
                    kind: OperationKind::UpdateMentionLabel {
                        inline_id: mention_id,
                        label: " @new ".to_string(),
                    },
                },
                Operation {
                    id: OperationId {
                        actor: ActorId("a".to_string()),
                        seq: 3,
                    },
                    kind: OperationKind::UpdateInlineEquationSource {
                        inline_id: inline_equation_id,
                        source: " c=d ".to_string(),
                    },
                },
                Operation {
                    id: OperationId {
                        actor: ActorId("a".to_string()),
                        seq: 4,
                    },
                    kind: OperationKind::UpdateBlockEquationSource {
                        block_id: block_equation_id,
                        source: "\tx=2\n".to_string(),
                    },
                },
            ]],
        )
        .unwrap();

        assert!(result.warnings.is_empty());
        match &result.document.blocks[0].content[0] {
            Inline::Link { href, .. } => assert_eq!(href, "https://example.invalid/new"),
            other => panic!("expected link inline, got {other:?}"),
        }
        match &result.document.blocks[0].content[1] {
            Inline::Mention { label, .. } => assert_eq!(label, "@new"),
            other => panic!("expected mention inline, got {other:?}"),
        }
        match &result.document.blocks[0].content[2] {
            Inline::Equation { equation, .. } => assert_eq!(equation.source, "c=d"),
            other => panic!("expected inline equation, got {other:?}"),
        }
        match &result.document.blocks[1].kind {
            BlockKind::EquationBlock { equation } => assert_eq!(equation.source, "x=2"),
            other => panic!("expected block equation, got {other:?}"),
        }
        result.document.validate().unwrap();
    }

    #[test]
    fn empty_block_equation_source_update_degrades_to_warning() {
        let mut base = Document::new("Doc");
        let block_id = StableId::new("block");
        base.blocks.push(Block {
            id: block_id.clone(),
            kind: BlockKind::EquationBlock {
                equation: Equation {
                    id: StableId::new("eq"),
                    source_format: EquationSourceFormat::LatexLike,
                    source: "x=1".to_string(),
                },
            },
            content: Vec::new(),
            properties: Vec::new(),
        });

        let invalid = Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::UpdateBlockEquationSource {
                block_id: block_id.clone(),
                source: "\t".to_string(),
            },
        };
        let valid = Operation {
            id: OperationId {
                actor: ActorId("b".to_string()),
                seq: 1,
            },
            kind: OperationKind::UpdateBlockEquationSource {
                block_id,
                source: "x=2".to_string(),
            },
        };

        let actor_streams =
            merge_operations(&base, &[vec![invalid.clone()], vec![valid.clone()]]).unwrap();
        let reversed_batches = merge_operations(&base, &[vec![valid], vec![invalid]]).unwrap();

        assert_eq!(actor_streams.document, reversed_batches.document);
        assert_eq!(actor_streams.warnings, reversed_batches.warnings);
        assert_eq!(actor_streams.document.visible_text(), "x=2\n");
        assert_eq!(
            actor_streams.warnings[0].code,
            "invalid-block-equation-source"
        );
        actor_streams.document.validate().unwrap();
    }

    #[test]
    fn image_alt_text_updates_by_stable_block_id() {
        let mut base = Document::new("Doc");
        let block_id = StableId::new("block");
        base.blocks.push(Block {
            id: block_id.clone(),
            kind: BlockKind::Image {
                blob_hash: "sha256:abc".to_string(),
                alt_text: "Initial image".to_string(),
            },
            content: Vec::new(),
            properties: Vec::new(),
        });

        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::UpdateImageAltText {
                    block_id,
                    alt_text: "Updated image".to_string(),
                },
            }]],
        )
        .unwrap();

        assert_eq!(result.document.visible_text(), "Updated image\n");
        match &result.document.blocks[0].kind {
            BlockKind::Image {
                blob_hash,
                alt_text,
            } => {
                assert_eq!(blob_hash, "sha256:abc");
                assert_eq!(alt_text, "Updated image");
            }
            other => panic!("expected image block, got {other:?}"),
        }
    }

    #[test]
    fn image_blob_hash_updates_keep_block_and_alt_text_stable() {
        let mut base = Document::new("Doc");
        let block_id = StableId::new("block");
        base.blocks.push(Block {
            id: block_id.clone(),
            kind: BlockKind::Image {
                blob_hash: "sha256:old".to_string(),
                alt_text: "Stable caption".to_string(),
            },
            content: Vec::new(),
            properties: Vec::new(),
        });

        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::UpdateImageBlobHash {
                    block_id: block_id.clone(),
                    blob_hash: "sha256:new".to_string(),
                },
            }]],
        )
        .unwrap();

        assert_eq!(result.document.blocks[0].id, block_id);
        assert_eq!(result.document.visible_text(), "Stable caption\n");
        match &result.document.blocks[0].kind {
            BlockKind::Image {
                blob_hash,
                alt_text,
            } => {
                assert_eq!(blob_hash, "sha256:new");
                assert_eq!(alt_text, "Stable caption");
            }
            other => panic!("expected image block, got {other:?}"),
        }
    }

    #[test]
    fn invalid_image_blob_hash_update_degrades_to_warning() {
        let mut base = Document::new("Doc");
        let block_id = StableId::new("block");
        base.blocks.push(Block {
            id: block_id.clone(),
            kind: BlockKind::Image {
                blob_hash: "sha256:old".to_string(),
                alt_text: "Stable caption".to_string(),
            },
            content: Vec::new(),
            properties: Vec::new(),
        });

        let invalid = Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::UpdateImageBlobHash {
                block_id: block_id.clone(),
                blob_hash: "not-a-hash".to_string(),
            },
        };
        let valid = Operation {
            id: OperationId {
                actor: ActorId("b".to_string()),
                seq: 1,
            },
            kind: OperationKind::UpdateImageBlobHash {
                block_id: block_id.clone(),
                blob_hash: "sha256:new".to_string(),
            },
        };

        let actor_streams =
            merge_operations(&base, &[vec![invalid.clone()], vec![valid.clone()]]).unwrap();
        let reversed_batches = merge_operations(&base, &[vec![valid], vec![invalid]]).unwrap();

        assert_eq!(actor_streams.document, reversed_batches.document);
        assert_eq!(actor_streams.warnings, reversed_batches.warnings);
        match &actor_streams.document.blocks[0].kind {
            BlockKind::Image { blob_hash, .. } => {
                assert_eq!(blob_hash, "sha256:new");
            }
            other => panic!("expected image block, got {other:?}"),
        }
        assert_eq!(actor_streams.warnings[0].code, "invalid-image-blob-hash");
        actor_streams.document.validate().unwrap();
    }

    #[test]
    fn padded_image_blob_hash_update_degrades_without_changing_source() {
        let mut base = Document::new("Doc");
        let block_id = StableId::new("block");
        base.blocks.push(Block {
            id: block_id.clone(),
            kind: BlockKind::Image {
                blob_hash: "sha256:old".to_string(),
                alt_text: "Stable caption".to_string(),
            },
            content: Vec::new(),
            properties: Vec::new(),
        });

        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::UpdateImageBlobHash {
                    block_id,
                    blob_hash: " sha256:new ".to_string(),
                },
            }]],
        )
        .unwrap();

        match &result.document.blocks[0].kind {
            BlockKind::Image { blob_hash, .. } => {
                assert_eq!(blob_hash, "sha256:old");
            }
            other => panic!("expected image block, got {other:?}"),
        }
        assert_eq!(result.warnings[0].code, "invalid-image-blob-hash");
        result.document.validate().unwrap();
    }

    #[test]
    fn image_blob_hash_update_stores_canonical_hash_reference() {
        let mut base = Document::new("Doc");
        let block_id = StableId::new("block");
        base.blocks.push(Block {
            id: block_id.clone(),
            kind: BlockKind::Image {
                blob_hash: "sha256:old".to_string(),
                alt_text: "Stable caption".to_string(),
            },
            content: Vec::new(),
            properties: Vec::new(),
        });

        let result = merge_operations(
            &base,
            &[vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::UpdateImageBlobHash {
                    block_id,
                    blob_hash: "sha256:new".to_string(),
                },
            }]],
        )
        .unwrap();

        match &result.document.blocks[0].kind {
            BlockKind::Image { blob_hash, .. } => {
                assert_eq!(
                    blob_hash,
                    &HashRef::parse("sha256:new").unwrap().to_string()
                );
            }
            other => panic!("expected image block, got {other:?}"),
        }
        assert!(result.warnings.is_empty());
        result.document.validate().unwrap();
    }

    #[test]
    fn image_delete_beats_stale_metadata_updates_without_resurrection() {
        let mut base = Document::new("Doc");
        let block_id = StableId::new("image-block");
        base.blocks.push(Block {
            id: block_id.clone(),
            kind: BlockKind::Image {
                blob_hash: "sha256:old".to_string(),
                alt_text: "Original caption".to_string(),
            },
            content: Vec::new(),
            properties: Vec::new(),
        });

        let delete = Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::DeleteBlock {
                block_id: block_id.clone(),
            },
        };
        let stale_alt = Operation {
            id: OperationId {
                actor: ActorId("b".to_string()),
                seq: 1,
            },
            kind: OperationKind::UpdateImageAltText {
                block_id: block_id.clone(),
                alt_text: "Concurrent caption".to_string(),
            },
        };
        let stale_blob = Operation {
            id: OperationId {
                actor: ActorId("c".to_string()),
                seq: 1,
            },
            kind: OperationKind::UpdateImageBlobHash {
                block_id: block_id.clone(),
                blob_hash: "sha256:new".to_string(),
            },
        };

        let actor_streams = merge_operations(
            &base,
            &[
                vec![delete.clone()],
                vec![stale_alt.clone()],
                vec![stale_blob.clone()],
            ],
        )
        .unwrap();
        let reversed_batches =
            merge_operations(&base, &[vec![stale_blob], vec![stale_alt], vec![delete]]).unwrap();

        assert_eq!(actor_streams.document, reversed_batches.document);
        assert_eq!(actor_streams.warnings, reversed_batches.warnings);
        assert!(actor_streams.document.blocks.is_empty());
        assert_eq!(
            actor_streams
                .warnings
                .iter()
                .filter(|warning| warning.code == "missing-block")
                .count(),
            2
        );
        assert!(actor_streams
            .warnings
            .iter()
            .all(|warning| warning.message.contains(&block_id.to_string())));
        actor_streams.document.validate().unwrap();
    }

    #[test]
    fn structured_inline_delete_beats_stale_source_updates_without_resurrection() {
        let mut base = Document::new("Doc");
        let link_id = StableId::parse("link-inline").unwrap();
        let mention_id = StableId::parse("mention-inline").unwrap();
        let equation_id = StableId::parse("equation-inline").unwrap();
        base.blocks.push(Block {
            id: StableId::parse("structured-inline-block").unwrap(),
            kind: BlockKind::Paragraph,
            content: vec![
                Inline::Link {
                    id: link_id.clone(),
                    text: "paper".to_string(),
                    href: "https://example.invalid/old".to_string(),
                    marks: Vec::new(),
                },
                Inline::Mention {
                    id: mention_id.clone(),
                    label: "@old".to_string(),
                },
                Inline::Equation {
                    id: equation_id.clone(),
                    equation: Equation {
                        id: StableId::parse("equation-source").unwrap(),
                        source_format: EquationSourceFormat::LatexLike,
                        source: "a=b".to_string(),
                    },
                },
            ],
            properties: Vec::new(),
        });

        let deletes = vec![
            Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::DeleteInline {
                    inline_id: link_id.clone(),
                },
            },
            Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 2,
                },
                kind: OperationKind::DeleteInline {
                    inline_id: mention_id.clone(),
                },
            },
            Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 3,
                },
                kind: OperationKind::DeleteInline {
                    inline_id: equation_id.clone(),
                },
            },
        ];
        let stale_updates = vec![
            Operation {
                id: OperationId {
                    actor: ActorId("b".to_string()),
                    seq: 1,
                },
                kind: OperationKind::UpdateLinkHref {
                    inline_id: link_id.clone(),
                    href: "https://example.invalid/new".to_string(),
                },
            },
            Operation {
                id: OperationId {
                    actor: ActorId("c".to_string()),
                    seq: 1,
                },
                kind: OperationKind::UpdateMentionLabel {
                    inline_id: mention_id.clone(),
                    label: "@new".to_string(),
                },
            },
            Operation {
                id: OperationId {
                    actor: ActorId("d".to_string()),
                    seq: 1,
                },
                kind: OperationKind::UpdateInlineEquationSource {
                    inline_id: equation_id.clone(),
                    source: "a=c".to_string(),
                },
            },
        ];

        let actor_streams =
            merge_operations(&base, &[deletes.clone(), stale_updates.clone()]).unwrap();
        let reversed_batches = merge_operations(&base, &[stale_updates, deletes]).unwrap();

        assert_eq!(actor_streams.document, reversed_batches.document);
        assert_eq!(actor_streams.warnings, reversed_batches.warnings);
        assert!(actor_streams.document.blocks[0].content.is_empty());
        assert_eq!(
            actor_streams
                .warnings
                .iter()
                .filter(|warning| warning.code == "missing-inline")
                .count(),
            3
        );
        let warning_text = actor_streams
            .warnings
            .iter()
            .map(|warning| warning.message.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(warning_text.contains(link_id.as_str()));
        assert!(warning_text.contains(mention_id.as_str()));
        assert!(warning_text.contains(equation_id.as_str()));
        actor_streams.document.validate().unwrap();
    }

    #[test]
    fn deterministic_fuzz_like_replay_keeps_document_valid() {
        let mut base = Document::new("Doc");
        base.blocks.push(Block::paragraph("seed"));
        let block_id = base.blocks[0].id.clone();
        let mut streams = Vec::new();
        for actor in 0..3 {
            let mut stream = Vec::new();
            for seq in 0..8 {
                stream.push(Operation {
                    id: OperationId {
                        actor: ActorId(format!("actor-{actor}")),
                        seq,
                    },
                    kind: OperationKind::InsertInline {
                        block_id: block_id.clone(),
                        after: None,
                        inline: Inline::text(format!("{actor}-{seq};")),
                    },
                });
            }
            streams.push(stream);
        }
        let result = merge_operations(&base, &streams).unwrap();
        result.document.validate().unwrap();
        assert!(result.document.visible_text().contains("2-7;"));
    }

    #[test]
    fn shuffled_mixed_rich_document_streams_converge_with_warnings() {
        let mut base = Document::new("Doc");
        let mut block = Block::paragraph("");
        block.content.clear();
        let first = Inline::text("alpha ");
        let first_id = inline_id(&first).clone();
        let second = Inline::text("beta ");
        let second_id = inline_id(&second).clone();
        let third = Inline::text("gamma");
        let third_id = inline_id(&third).clone();
        block.content.extend([first, second, third]);
        let block_id = block.id.clone();
        base.blocks.push(block);

        let comment = CommentThread {
            id: StableId::parse("comment-thread-mixed").unwrap(),
            anchor: Anchor::TextRange(TextRange {
                start: first_id.clone(),
                end: first_id.clone(),
            }),
            comments: vec![Comment {
                id: StableId::parse("comment-mixed").unwrap(),
                author: "Alice".to_string(),
                body: vec![Inline::text("note")],
                created_at_ms: 1,
                deleted: false,
            }],
            deleted: false,
        };
        let suggestion = Suggestion {
            id: StableId::parse("suggestion-mixed").unwrap(),
            author: "Bob".to_string(),
            kind: SuggestionKind::Format {
                range: TextRange {
                    start: first_id.clone(),
                    end: second_id.clone(),
                },
                marks: vec![Mark {
                    kind: MarkKind::Italic,
                    value: None,
                    expand: MarkExpand::Both,
                }],
            },
            state: SuggestionState::Proposed,
            provenance: Vec::new(),
        };
        let equation = Inline::Equation {
            id: StableId::parse("eq-inline-mixed").unwrap(),
            equation: Equation {
                id: StableId::parse("eq-mixed").unwrap(),
                source_format: EquationSourceFormat::LatexLike,
                source: "x+y".to_string(),
            },
        };
        let inserted = Inline::text("inserted ");

        let ops = vec![
            Operation {
                id: OperationId {
                    actor: ActorId("actor-a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::AddCommentThread { thread: comment },
            },
            Operation {
                id: OperationId {
                    actor: ActorId("actor-b".to_string()),
                    seq: 1,
                },
                kind: OperationKind::AddSuggestion { suggestion },
            },
            Operation {
                id: OperationId {
                    actor: ActorId("actor-c".to_string()),
                    seq: 1,
                },
                kind: OperationKind::DeleteInline {
                    inline_id: first_id.clone(),
                },
            },
            Operation {
                id: OperationId {
                    actor: ActorId("actor-d".to_string()),
                    seq: 1,
                },
                kind: OperationKind::AddMarkRange {
                    range: TextRange {
                        start: first_id,
                        end: third_id,
                    },
                    mark: Mark {
                        kind: MarkKind::Bold,
                        value: None,
                        expand: MarkExpand::Both,
                    },
                },
            },
            Operation {
                id: OperationId {
                    actor: ActorId("actor-e".to_string()),
                    seq: 1,
                },
                kind: OperationKind::InsertInline {
                    block_id: block_id.clone(),
                    after: Some(second_id.clone()),
                    inline: inserted,
                },
            },
            Operation {
                id: OperationId {
                    actor: ActorId("actor-f".to_string()),
                    seq: 1,
                },
                kind: OperationKind::InsertInline {
                    block_id,
                    after: Some(second_id.clone()),
                    inline: equation,
                },
            },
        ];

        let reference = merge_operations(&base, &[ops.clone(), Vec::new()]).unwrap();
        let batched_by_actor = merge_operations(
            &base,
            &[
                vec![ops[0].clone(), ops[3].clone()],
                vec![ops[1].clone(), ops[4].clone()],
                vec![ops[2].clone(), ops[5].clone()],
                Vec::new(),
                Vec::new(),
            ],
        )
        .unwrap();
        let reversed_batches = merge_operations(
            &base,
            &[
                vec![ops[5].clone(), ops[2].clone()],
                vec![ops[4].clone(), ops[1].clone()],
                vec![ops[3].clone(), ops[0].clone()],
            ],
        )
        .unwrap();

        assert_eq!(reference.document, batched_by_actor.document);
        assert_eq!(reference.document, reversed_batches.document);
        assert_eq!(reference.warnings, batched_by_actor.warnings);
        assert_eq!(reference.warnings, reversed_batches.warnings);
        assert!(reference
            .warnings
            .iter()
            .any(|warning| warning.code == "comment-anchor-degraded"));
        assert!(reference
            .warnings
            .iter()
            .any(|warning| warning.code == "suggestion-range-degraded"));
        assert!(reference
            .warnings
            .iter()
            .any(|warning| warning.code == "mark-range-degraded"));
        reference.document.validate().unwrap();
    }

    #[test]
    fn shuffled_structured_document_batches_converge_with_citations_and_tables() {
        let mut base = Document::new("Doc");
        let reference_id = StableId::parse("ref-structured").unwrap();
        let citation_id = StableId::parse("citation-structured").unwrap();
        base.citation_database
            .upsert_reference(BibliographyReference {
                id: reference_id.clone(),
                revision: 1,
                source: CitationSource {
                    format: CitationSourceFormat::CitumNative,
                    bytes: b"title: Structured Merge".to_vec(),
                },
                summary: CitationSummary {
                    title: "Structured Merge".to_string(),
                    authors: vec!["Doe".to_string()],
                    issued: Some("2026".to_string()),
                    doi: None,
                    url: None,
                },
                deleted: false,
            });
        base.citation_database.upsert_citation(CitationGroup {
            id: citation_id.clone(),
            revision: 1,
            items: vec![CitationItem {
                reference_id: reference_id.clone(),
                locator: None,
                label: None,
                prefix: None,
                suffix: None,
                suppress_author: false,
            }],
            placement: CitationPlacement::Inline,
            rendered_cache: Some("(Doe 2026)".to_string()),
            deleted: false,
        });

        let first = Inline::text("alpha ");
        let first_id = inline_id(&first).clone();
        let citation_inline = Inline::Citation {
            id: StableId::parse("citation-inline-structured").unwrap(),
            citation_id: citation_id.clone(),
            rendered_cache: Some("(Doe 2026)".to_string()),
        };
        let second = Inline::text("omega");
        let second_id = inline_id(&second).clone();
        let paragraph_id = StableId::parse("paragraph-structured").unwrap();
        base.blocks.push(Block {
            id: paragraph_id,
            kind: BlockKind::Paragraph,
            content: vec![first, citation_inline, second],
            properties: Vec::new(),
        });

        let equation_inline_id = StableId::parse("equation-inline-structured").unwrap();
        base.blocks.push(Block {
            id: StableId::parse("equation-paragraph-structured").unwrap(),
            kind: BlockKind::Paragraph,
            content: vec![Inline::Equation {
                id: equation_inline_id.clone(),
                equation: Equation {
                    id: StableId::parse("equation-structured").unwrap(),
                    source_format: EquationSourceFormat::LatexLike,
                    source: "x=1".to_string(),
                },
            }],
            properties: Vec::new(),
        });

        let table_block_id = StableId::parse("table-structured").unwrap();
        let row_id = StableId::parse("row-structured").unwrap();
        let cell_id = StableId::parse("cell-structured").unwrap();
        base.blocks.push(Block {
            id: table_block_id.clone(),
            kind: BlockKind::Table {
                rows: vec![TableRow {
                    id: row_id.clone(),
                    cells: vec![table_cell(&cell_id, "base cell")],
                }],
            },
            content: Vec::new(),
            properties: Vec::new(),
        });

        let ops = vec![
            Operation {
                id: OperationId {
                    actor: ActorId("actor-a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::AddCommentThread {
                    thread: CommentThread {
                        id: StableId::parse("comment-thread-structured").unwrap(),
                        anchor: Anchor::TextRange(TextRange {
                            start: first_id.clone(),
                            end: first_id.clone(),
                        }),
                        comments: vec![Comment {
                            id: StableId::parse("comment-structured").unwrap(),
                            author: "Reviewer".to_string(),
                            body: vec![Inline::text("structured note")],
                            created_at_ms: 1,
                            deleted: false,
                        }],
                        deleted: false,
                    },
                },
            },
            Operation {
                id: OperationId {
                    actor: ActorId("actor-b".to_string()),
                    seq: 1,
                },
                kind: OperationKind::AddSuggestion {
                    suggestion: Suggestion {
                        id: StableId::parse("suggestion-structured").unwrap(),
                        author: "Editor".to_string(),
                        kind: SuggestionKind::Format {
                            range: TextRange {
                                start: first_id.clone(),
                                end: second_id.clone(),
                            },
                            marks: vec![Mark {
                                kind: MarkKind::Italic,
                                value: None,
                                expand: MarkExpand::Both,
                            }],
                        },
                        state: SuggestionState::Proposed,
                        provenance: Vec::new(),
                    },
                },
            },
            Operation {
                id: OperationId {
                    actor: ActorId("actor-c".to_string()),
                    seq: 1,
                },
                kind: OperationKind::DeleteInline {
                    inline_id: first_id.clone(),
                },
            },
            Operation {
                id: OperationId {
                    actor: ActorId("actor-d".to_string()),
                    seq: 1,
                },
                kind: OperationKind::AddMarkRange {
                    range: TextRange {
                        start: first_id,
                        end: second_id,
                    },
                    mark: Mark {
                        kind: MarkKind::Bold,
                        value: None,
                        expand: MarkExpand::Both,
                    },
                },
            },
            Operation {
                id: OperationId {
                    actor: ActorId("actor-e".to_string()),
                    seq: 1,
                },
                kind: OperationKind::UpdateInlineEquationSource {
                    inline_id: equation_inline_id,
                    source: "y=2".to_string(),
                },
            },
            Operation {
                id: OperationId {
                    actor: ActorId("actor-f".to_string()),
                    seq: 1,
                },
                kind: OperationKind::InsertTableRow {
                    table_block_id: table_block_id.clone(),
                    after_row: Some(row_id.clone()),
                    row: table_row(
                        &StableId::parse("row-inserted-structured").unwrap(),
                        "row inserted",
                    ),
                },
            },
            Operation {
                id: OperationId {
                    actor: ActorId("actor-g".to_string()),
                    seq: 1,
                },
                kind: OperationKind::InsertTableCell {
                    table_block_id,
                    row_id,
                    after_cell: Some(cell_id),
                    cell: table_cell(
                        &StableId::parse("cell-inserted-structured").unwrap(),
                        "cell inserted",
                    ),
                },
            },
            Operation {
                id: OperationId {
                    actor: ActorId("actor-h".to_string()),
                    seq: 1,
                },
                kind: OperationKind::DeleteBibliographyReference {
                    reference_id,
                    revision: 2,
                },
            },
        ];

        let reference = merge_operations(&base, &[ops.clone(), Vec::new()]).unwrap();
        let interleaved_batches = merge_operations(
            &base,
            &[
                vec![ops[0].clone(), ops[4].clone(), ops[7].clone()],
                vec![ops[1].clone(), ops[5].clone()],
                vec![ops[2].clone(), ops[6].clone()],
                vec![ops[3].clone()],
                Vec::new(),
            ],
        )
        .unwrap();
        let reversed_batches = merge_operations(
            &base,
            &[
                vec![ops[7].clone(), ops[3].clone()],
                vec![ops[6].clone(), ops[2].clone()],
                vec![ops[5].clone(), ops[1].clone()],
                vec![ops[4].clone(), ops[0].clone()],
            ],
        )
        .unwrap();

        assert_eq!(reference.document, interleaved_batches.document);
        assert_eq!(reference.document, reversed_batches.document);
        assert_eq!(reference.warnings, interleaved_batches.warnings);
        assert_eq!(reference.warnings, reversed_batches.warnings);
        for expected in [
            "comment-anchor-degraded",
            "suggestion-range-degraded",
            "mark-range-degraded",
            "citation-reference-missing",
        ] {
            assert!(reference
                .warnings
                .iter()
                .any(|warning| warning.code == expected));
        }
        let visible = reference.document.visible_text();
        assert!(visible.contains("y=2"));
        assert!(visible.contains("row inserted"));
        assert!(visible.contains("cell inserted"));
        reference.document.validate().unwrap();
    }

    #[test]
    fn deterministic_multi_replica_pseudo_fuzz_converges() {
        let mut base = Document::new("Doc");
        let mut block = Block::paragraph("");
        block.content.clear();
        let mut seed_ids = Vec::new();
        for label in ["alpha ", "beta ", "gamma ", "delta ", "epsilon"] {
            let inline = Inline::text(label);
            seed_ids.push(inline_id(&inline).clone());
            block.content.push(inline);
        }
        let block_id = block.id.clone();
        base.blocks.push(block);

        let mut streams = vec![Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new()];
        let mut seqs = [1_u64, 1, 1];
        let mut rng = 0x5eed_cafe_u64;
        for step in 0..36 {
            rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
            let actor_index = (rng as usize) % 3;
            let actor = format!("actor-{actor_index}");
            let seq = seqs[actor_index];
            seqs[actor_index] += 1;
            let first = seed_ids[(rng as usize >> 8) % seed_ids.len()].clone();
            let second = seed_ids[(rng as usize >> 16) % seed_ids.len()].clone();
            let kind = match step % 6 {
                0 => OperationKind::InsertInline {
                    block_id: block_id.clone(),
                    after: Some(first),
                    inline: Inline::text(format!("i{step} ")),
                },
                1 => OperationKind::UpdateInlineText {
                    inline_id: first,
                    text: format!("u{step} "),
                },
                2 => OperationKind::AddMarkRange {
                    range: TextRange {
                        start: first,
                        end: second,
                    },
                    mark: Mark {
                        kind: if step % 12 == 2 {
                            MarkKind::Bold
                        } else {
                            MarkKind::Italic
                        },
                        value: None,
                        expand: MarkExpand::Both,
                    },
                },
                3 => OperationKind::AddCommentThread {
                    thread: CommentThread {
                        id: StableId::parse(format!("comment-thread-{step}")).unwrap(),
                        anchor: Anchor::TextRange(TextRange {
                            start: first,
                            end: second,
                        }),
                        comments: vec![Comment {
                            id: StableId::parse(format!("comment-{step}")).unwrap(),
                            author: format!("author-{actor_index}"),
                            body: vec![Inline::text("note")],
                            created_at_ms: step,
                            deleted: false,
                        }],
                        deleted: false,
                    },
                },
                4 => OperationKind::AddSuggestion {
                    suggestion: Suggestion {
                        id: StableId::parse(format!("suggestion-{step}")).unwrap(),
                        author: format!("author-{actor_index}"),
                        kind: SuggestionKind::Delete {
                            range: TextRange {
                                start: first,
                                end: second,
                            },
                        },
                        state: SuggestionState::Proposed,
                        provenance: Vec::new(),
                    },
                },
                _ => OperationKind::DeleteInline { inline_id: first },
            };
            streams[actor_index].push(Operation {
                id: OperationId {
                    actor: ActorId(actor),
                    seq,
                },
                kind,
            });
        }

        let single_stream = streams
            .iter()
            .flatten()
            .cloned()
            .collect::<Vec<Operation>>();
        let reversed_streams = streams
            .iter()
            .rev()
            .cloned()
            .collect::<Vec<Vec<Operation>>>();

        let reference = merge_operations(&base, &streams).unwrap();
        let single = merge_operations(&base, &[single_stream]).unwrap();
        let reversed = merge_operations(&base, &reversed_streams).unwrap();

        assert_eq!(reference.document, single.document);
        assert_eq!(reference.document, reversed.document);
        assert_eq!(reference.warnings, single.warnings);
        assert_eq!(reference.warnings, reversed.warnings);
        assert!(reference
            .warnings
            .iter()
            .any(|warning| warning.code == "missing-inline"));
        reference.document.validate().unwrap();
    }

    fn assert_mark_kinds(inline: &Inline, expected: &[MarkKind]) {
        let marks = match inline {
            Inline::Text { marks, .. } | Inline::Link { marks, .. } => marks,
            other => panic!("expected editable inline, got {other:?}"),
        };
        let actual = marks
            .iter()
            .map(|mark| mark.kind.clone())
            .collect::<Vec<_>>();
        assert_eq!(actual, expected);
    }

    fn table_row(id: &StableId, text: &str) -> TableRow {
        TableRow {
            id: id.clone(),
            cells: vec![table_cell(
                &StableId::parse(format!("cell-{id}")).unwrap(),
                text,
            )],
        }
    }

    fn table_cell(id: &StableId, text: &str) -> TableCell {
        TableCell {
            id: id.clone(),
            blocks: vec![Block::paragraph(text)],
            properties: Vec::new(),
        }
    }
}
