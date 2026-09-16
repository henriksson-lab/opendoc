//! The apply pass: one operation kind at a time, against a mutable document.

use crate::anchors::{
    anchor_has_no_surviving_target, anchor_resolves, comment_anchor_evidence, repair_comment_anchor,
};
use crate::block_edit::{
    clear_block_property, set_block_property, set_block_text_style, update_block_equation_source,
    update_heading_level, update_image_alt_text, update_image_blob_hash, update_image_layout,
    update_list_item, validate_block_text_style,
};
use crate::blocks::{
    block_exists, delete_block, delete_would_empty_table_cell, find_block_mut, insert_block,
    move_block, MoveBlockResult,
};
use crate::citations::{
    invalidate_all_citation_caches, invalidate_citation_caches_for_reference,
    invalidate_inline_citation_caches,
};
use crate::inline_edit::{
    delete_inline, select_dropdown_option, update_date_chip, update_inline_equation_source,
    update_inline_text, update_link_href, update_mention_label,
};
use crate::inline_ops::{
    inline_id, insert_inline, insert_inlines_at_document_end, move_inline_to_block,
    InlineMoveResult,
};
use crate::marks::{add_mark, add_mark_range, remove_mark, MarkRangeResult};
use crate::operation::OperationKind;
use crate::suggestions::{
    accept_suggestion, canonical_diagnostic_value, reviewer_provenance_value,
};
use crate::tables::{
    delete_table_cell, delete_table_column, delete_table_row, find_table_cell_mut,
    insert_table_cell, insert_table_column, insert_table_row, reorder_table_rows, repair_table,
    set_table_alignment, set_table_border, set_table_cell_span, set_table_column_width,
    set_table_row_header, set_table_row_height, table_block_id_for_cell, TableEditResult,
};
use crate::validate::{
    comment_thread_has_empty_body, inline_exists, inline_payload_valid_for_merge,
    inline_sequence_is_empty_source_text, inline_sequence_payload_valid_for_merge,
    insert_block_payload_valid_for_merge, mark_removal_valid_for_merge, marks_valid_for_merge,
    suggestion_insert_content_is_empty, table_cell_payload_valid_for_merge,
    table_row_payload_valid_for_merge,
};
use opendoc_core::{
    Anchor, CommentActivityEntry, CommentActivityKind, CommentHistoryEntry, Document, Mark,
    ModelWarning, StableId, SuggestionKind, SuggestionState, MAX_COMMENT_ACTIVITY_ENTRIES,
};

#[derive(Clone)]
pub(crate) struct OperationProvenance {
    pub operation_actor: String,
    pub operation_seq: u64,
}

pub(crate) fn apply(
    document: &mut Document,
    warnings: &mut Vec<ModelWarning>,
    kind: OperationKind,
    provenance: Option<OperationProvenance>,
) {
    // Preserve evidence only for anchors that were live immediately before an
    // operation.  This distinguishes a real deletion from a malformed or
    // externally imported missing target, where no trustworthy quote exists.
    let comment_evidence: Vec<(StableId, String, String)> = document
        .comments
        .iter()
        .filter(|thread| !thread.deleted && anchor_resolves(document, &thread.anchor))
        .filter_map(|thread| {
            comment_anchor_evidence(&document.blocks, &thread.anchor)
                .map(|(quote, context)| (thread.id.clone(), quote, context))
        })
        .collect();
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
        OperationKind::UpsertBookmark { bookmark } => {
            if let Err(err) = bookmark.validate() {
                warnings.push(ModelWarning {
                    code: "invalid-bookmark".to_string(),
                    message: format!("bookmark {} was ignored: {err}", bookmark.id),
                });
                return;
            }
            let bookmark_id = bookmark.id.clone();
            let bookmark_name = bookmark.name.clone();
            let bookmark_is_live = !bookmark.deleted;
            let applied = if let Some(existing) = document
                .bookmarks
                .iter_mut()
                .find(|item| item.id == bookmark.id)
            {
                if bookmark.revision >= existing.revision {
                    *existing = bookmark;
                    true
                } else {
                    false
                }
            } else {
                document.bookmarks.push(bookmark);
                true
            };
            // A bookmark name is a document-wide link target. The winner is
            // the operation currently being applied (the merge ordering is
            // deterministic); tombstone the other entry rather than silently
            // retaining two anchors for one name.
            if applied && bookmark_is_live {
                for item in &mut document.bookmarks {
                    if item.id != bookmark_id && !item.deleted && item.name == bookmark_name {
                        item.deleted = true;
                        item.revision = item.revision.saturating_add(1);
                        warnings.push(ModelWarning {
                            code: "bookmark-name-conflict".to_string(),
                            message: format!(
                                "bookmark name {} was superseded by a later bookmark",
                                item.name
                            ),
                        });
                    }
                }
            }
            document
                .bookmarks
                .sort_by(|left, right| left.id.cmp(&right.id));
        }
        OperationKind::InsertBlock { position, block } => {
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
                if insert_block(document, position, block) {
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
            if delete_would_empty_table_cell(document, &block_id) {
                warnings.push(ModelWarning {
                    code: "table-cell-requires-block".to_string(),
                    message: format!("deleting block {block_id} would leave its table cell empty"),
                });
            } else if !delete_block(document, &block_id) {
                warnings.push(ModelWarning {
                    code: "missing-block".to_string(),
                    message: format!("block {block_id} was already absent"),
                });
            }
        }
        OperationKind::MoveBlock { block_id, position } => {
            match move_block(document, &block_id, position) {
                MoveBlockResult::Moved => {}
                MoveBlockResult::MissingSource => warnings.push(ModelWarning {
                    code: "missing-block".to_string(),
                    message: format!("block move source {block_id} was missing"),
                }),
                MoveBlockResult::MissingAnchor => warnings.push(ModelWarning {
                    code: "block-anchor-missing".to_string(),
                    message: format!("block move anchor for {block_id} was missing"),
                }),
                MoveBlockResult::SameBlockAnchor => warnings.push(ModelWarning {
                    code: "block-move-self-anchor".to_string(),
                    message: format!("block {block_id} cannot be moved relative to itself"),
                }),
                MoveBlockResult::DescendantAnchor => warnings.push(ModelWarning {
                    code: "block-move-descendant-anchor".to_string(),
                    message: format!("block {block_id} cannot be moved into its own subtree"),
                }),
                MoveBlockResult::WouldEmptyCell => warnings.push(ModelWarning {
                    code: "table-cell-requires-block".to_string(),
                    message: format!("moving block {block_id} would leave a table cell empty"),
                }),
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
            position,
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
                if insert_inline(&mut block.content, position, inline) {
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
            position,
        } => match move_inline_to_block(document, &inline_id, &target_block_id, position) {
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
            if !marks_valid_for_merge(
                std::slice::from_ref(&mark),
                warnings,
                "mark operation",
                &text_id,
            ) {
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
                    message: format!(
                        "comment thread {} was ignored: empty comment body",
                        thread.id
                    ),
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
            } else if anchor_resolves(document, &thread.anchor)
                // An imported/replayed orphan is already an honest statement
                // that no live target remains, complete with its source
                // evidence. Do not pretend this operation moved it to a
                // nearest block (and do not manufacture that warning).
                || matches!(&thread.anchor, Anchor::Orphaned { .. })
            {
                let thread_id = thread.id.clone();
                document.comments.push(thread);
                append_comment_activity(
                    document,
                    &thread_id,
                    None,
                    CommentActivityKind::ThreadCreated,
                    provenance,
                );
            } else {
                let mut thread = thread;
                repair_comment_anchor(
                    &document.blocks,
                    &mut thread.anchor,
                    "comment anchor could not be resolved",
                );
                let thread_id = thread.id.clone();
                document.comments.push(thread);
                append_comment_activity(
                    document,
                    &thread_id,
                    None,
                    CommentActivityKind::ThreadCreated,
                    provenance,
                );
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
                    let comment_id = comment.id.clone();
                    thread.comments.push(comment);
                    thread.comments.sort_by(|left, right| {
                        left.created_at_ms
                            .cmp(&right.created_at_ms)
                            .then_with(|| left.id.cmp(&right.id))
                    });
                    append_comment_activity(
                        document,
                        &thread_id,
                        Some(&comment_id),
                        CommentActivityKind::ReplyAdded,
                        provenance,
                    );
                }
            }
            None => warnings.push(ModelWarning {
                code: "missing-comment-thread".to_string(),
                message: format!("comment thread {thread_id} was missing"),
            }),
        },
        OperationKind::ResolveCommentThread {
            thread_id,
            resolved_by,
            resolved_at_ms,
        } => {
            // Match the core model and app command boundary.  A malformed
            // remote resolver must be isolated as one ignored operation, not
            // written into a thread that will then make the whole document
            // invalid at the end of replay.
            if resolved_by.trim().is_empty() || resolved_by.trim() != resolved_by {
                warnings.push(ModelWarning {
                    code: "invalid-comment-resolution".to_string(),
                    message: format!("comment thread {thread_id} ignored invalid resolution actor"),
                });
            } else if let Some(thread) = document
                .comments
                .iter_mut()
                .find(|thread| thread.id == thread_id && !thread.deleted)
            {
                if thread.state == opendoc_core::CommentThreadState::Resolved
                    && thread.resolved_by.as_deref() == Some(resolved_by.as_str())
                    && thread.resolved_at_ms == Some(resolved_at_ms)
                {
                    return;
                }
                thread.state = opendoc_core::CommentThreadState::Resolved;
                thread.resolved_by = Some(resolved_by.clone());
                thread.resolved_at_ms = Some(resolved_at_ms);
                append_comment_activity(
                    document,
                    &thread_id,
                    None,
                    CommentActivityKind::ThreadResolved,
                    provenance,
                );
            } else {
                warnings.push(ModelWarning {
                    code: "missing-comment-thread".to_string(),
                    message: format!("comment thread {thread_id} was missing"),
                });
            }
        }
        OperationKind::ReopenCommentThread { thread_id } => {
            if let Some(thread) = document
                .comments
                .iter_mut()
                .find(|thread| thread.id == thread_id && !thread.deleted)
            {
                if thread.state == opendoc_core::CommentThreadState::Reopened {
                    return;
                }
                thread.state = opendoc_core::CommentThreadState::Reopened;
                thread.resolved_by = None;
                thread.resolved_at_ms = None;
                append_comment_activity(
                    document,
                    &thread_id,
                    None,
                    CommentActivityKind::ThreadReopened,
                    provenance,
                );
            } else {
                warnings.push(ModelWarning {
                    code: "missing-comment-thread".to_string(),
                    message: format!("comment thread {thread_id} was missing"),
                });
            }
        }
        OperationKind::SetCommentThreadAction {
            thread_id,
            assignee,
            due_at_ms,
            completed_by,
            completed_at_ms,
        } => {
            if completed_by.is_some() != completed_at_ms.is_some() {
                warnings.push(ModelWarning {
                    code: "invalid-comment-action".to_string(),
                    message: format!(
                        "comment thread {thread_id} ignored incomplete action completion metadata"
                    ),
                });
                return;
            }
            if due_at_ms.is_some() && assignee.is_none() {
                warnings.push(ModelWarning {
                    code: "invalid-comment-action".to_string(),
                    message: format!(
                        "comment thread {thread_id} ignored an action due date without an assignee"
                    ),
                });
                return;
            }
            if completed_by.is_some() && assignee.is_none() {
                warnings.push(ModelWarning {
                    code: "invalid-comment-action".to_string(),
                    message: format!(
                        "comment thread {thread_id} ignored completion for an unassigned action"
                    ),
                });
                return;
            }
            if assignee
                .as_deref()
                .is_some_and(|value| value.trim().is_empty() || value.trim() != value)
                || completed_by
                    .as_deref()
                    .is_some_and(|value| value.trim().is_empty() || value.trim() != value)
            {
                warnings.push(ModelWarning {
                    code: "invalid-comment-action".to_string(),
                    message: format!(
                        "comment thread {thread_id} ignored action metadata with an invalid actor"
                    ),
                });
                return;
            }
            if let Some(thread) = document
                .comments
                .iter_mut()
                .find(|thread| thread.id == thread_id && !thread.deleted)
            {
                if thread.action_assignee == assignee
                    && thread.action_due_at_ms == due_at_ms
                    && thread.action_completed_by == completed_by
                    && thread.action_completed_at_ms == completed_at_ms
                {
                    return;
                }
                thread.action_assignee = assignee.clone();
                thread.action_due_at_ms = due_at_ms;
                thread.action_completed_by = completed_by.clone();
                thread.action_completed_at_ms = completed_at_ms;
                append_comment_activity(
                    document,
                    &thread_id,
                    None,
                    CommentActivityKind::ActionSet,
                    provenance,
                );
            } else {
                warnings.push(ModelWarning {
                    code: "missing-comment-thread".to_string(),
                    message: format!("comment thread {thread_id} was missing"),
                });
            }
        }
        OperationKind::SetCommentThreadReaction {
            thread_id,
            emoji,
            actor,
            present,
        } => {
            if let Err(err) = opendoc_core::validate_comment_reaction_emoji(&emoji) {
                warnings.push(ModelWarning {
                    code: "invalid-comment-reaction".to_string(),
                    message: format!("comment reaction was ignored: {err}"),
                });
            } else if actor.trim().is_empty() || actor.trim() != actor {
                warnings.push(ModelWarning {
                    code: "invalid-comment-reaction".to_string(),
                    message: "comment reaction actor was invalid".to_string(),
                });
            } else if let Some(thread) = document
                .comments
                .iter_mut()
                .find(|thread| thread.id == thread_id && !thread.deleted)
            {
                let had_reaction = thread
                    .reactions
                    .iter()
                    .find(|item| item.emoji == emoji)
                    .is_some_and(|reaction| reaction.actors.iter().any(|item| item == &actor));
                if let Some(reaction) = thread.reactions.iter_mut().find(|item| item.emoji == emoji)
                {
                    if present {
                        if !reaction.actors.iter().any(|item| item == &actor) {
                            reaction.actors.push(actor.clone());
                            reaction.actors.sort();
                        }
                    } else {
                        reaction.actors.retain(|item| item != &actor);
                    }
                } else if present {
                    thread.reactions.push(opendoc_core::CommentThreadReaction {
                        emoji: emoji.clone(),
                        actors: vec![actor.clone()],
                    });
                }
                thread
                    .reactions
                    .retain(|reaction| !reaction.actors.is_empty());
                thread
                    .reactions
                    .sort_by(|left, right| left.emoji.cmp(&right.emoji));
                if had_reaction != present {
                    let kind = if present {
                        CommentActivityKind::ReactionAdded
                    } else {
                        CommentActivityKind::ReactionRemoved
                    };
                    append_comment_activity(document, &thread_id, None, kind, provenance);
                }
            } else {
                warnings.push(ModelWarning {
                    code: "missing-comment-thread".to_string(),
                    message: format!("comment thread {thread_id} was missing"),
                });
            }
        }
        OperationKind::UpsertFootnote { footnote } => {
            if let Err(err) = footnote.validate() {
                warnings.push(ModelWarning {
                    code: "invalid-footnote".to_string(),
                    message: format!("footnote {} was ignored: {err}", footnote.id),
                });
                return;
            }
            let footnote_id = footnote.id.clone();
            let deleted = footnote.deleted;
            let applied = if let Some(existing) = document
                .footnotes
                .iter_mut()
                .find(|item| item.id == footnote.id)
            {
                if footnote.revision >= existing.revision {
                    *existing = footnote;
                    true
                } else {
                    false
                }
            } else {
                document.footnotes.push(footnote);
                document
                    .footnotes
                    .sort_by(|left, right| left.id.cmp(&right.id));
                true
            };
            // A note tombstone makes any placement invalid. This is also what
            // lets an inverse batch safely delete a newly-created endnote
            // before its placement inverse arrives.
            if applied && deleted {
                document.endnote_ids.remove(&footnote_id);
            }
        }
        OperationKind::SetEndnotePlacement {
            footnote_id,
            revision,
            endnote,
        } => {
            let Some(note) = document
                .footnotes
                .iter_mut()
                .find(|note| note.id == footnote_id)
            else {
                warnings.push(ModelWarning {
                    code: "missing-footnote".to_string(),
                    message: format!("note {footnote_id} was missing"),
                });
                return;
            };
            if note.deleted || revision < note.revision {
                return;
            }
            note.revision = revision;
            if endnote {
                document.endnote_ids.insert(footnote_id.clone());
            } else {
                document.endnote_ids.remove(&footnote_id);
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
                if thread.deleted {
                    return;
                }
                thread.deleted = true;
                append_comment_activity(
                    document,
                    &thread_id,
                    None,
                    CommentActivityKind::ThreadDeleted,
                    provenance,
                );
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
                append_comment_activity(
                    document,
                    &thread_id,
                    None,
                    CommentActivityKind::ThreadRestored,
                    provenance,
                );
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
        } => {
            let (comments, history) = (&mut document.comments, &mut document.comment_history);
            match comments.iter_mut().find(|thread| thread.id == thread_id) {
                Some(thread) => match thread
                    .comments
                    .iter_mut()
                    .find(|comment| comment.id == comment_id && !comment.deleted)
                {
                    Some(comment) => {
                        let previous_body = comment.body.clone();
                        comment.deleted = true;
                        append_comment_history(
                            history,
                            &thread_id,
                            &comment_id,
                            "deleted",
                            provenance.clone(),
                            Some(previous_body),
                        );
                        if thread.comments.iter().all(|comment| comment.deleted) {
                            thread.deleted = true;
                        }
                        append_comment_activity(
                            document,
                            &thread_id,
                            Some(&comment_id),
                            CommentActivityKind::CommentDeleted,
                            provenance.clone(),
                        );
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
            }
        }
        OperationKind::RestoreComment {
            thread_id,
            comment_id,
        } => {
            let (comments, history) = (&mut document.comments, &mut document.comment_history);
            match comments.iter_mut().find(|thread| thread.id == thread_id) {
                Some(thread) => match thread
                    .comments
                    .iter_mut()
                    .find(|comment| comment.id == comment_id && comment.deleted)
                {
                    Some(comment) => {
                        comment.deleted = false;
                        thread.deleted = false;
                        append_comment_history(
                            history,
                            &thread_id,
                            &comment_id,
                            "restored",
                            provenance.clone(),
                            None,
                        );
                        append_comment_activity(
                            document,
                            &thread_id,
                            Some(&comment_id),
                            CommentActivityKind::CommentRestored,
                            provenance,
                        );
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
            }
        }
        OperationKind::UpdateCommentBody {
            thread_id,
            comment_id,
            body,
        } => {
            let (comments, history) = (&mut document.comments, &mut document.comment_history);
            match comments
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
                        // An equal body is a real replay no-op, not an edit.
                        // In particular it must not grow the immutable
                        // provenance ledger every time a delayed replica
                        // redelivers it under a new operation id.
                        if comment.body == body {
                            return;
                        }
                        let previous_body = std::mem::replace(&mut comment.body, body);
                        append_comment_history(
                            history,
                            &thread_id,
                            &comment_id,
                            "edited",
                            provenance.clone(),
                            Some(previous_body),
                        );
                        append_comment_activity(
                            document,
                            &thread_id,
                            Some(&comment_id),
                            CommentActivityKind::CommentEdited,
                            provenance,
                        );
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
            }
        }
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
        // Character operations never reach here: `merge_operations` diverts
        // them to `apply_text_run_edits`, which resolves them by character
        // identity instead of by offset. Applying one positionally is the bug
        // ADR 0007 exists to remove, so this arm deliberately does nothing.
        OperationKind::InsertText { .. } | OperationKind::DeleteText { .. } => {
            debug_assert!(
                false,
                "character operations are resolved by the text sequence phase"
            );
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
        OperationKind::SelectDropdownOption {
            inline_id,
            option_id,
        } => match select_dropdown_option(document, &inline_id, &option_id) {
            Some(true) => {}
            Some(false) => warnings.push(ModelWarning {
                code: "invalid-dropdown-option".to_string(),
                message: format!("dropdown {inline_id} has no option {option_id}"),
            }),
            None => warnings.push(ModelWarning {
                code: "missing-inline".to_string(),
                message: format!("dropdown inline {inline_id} was missing"),
            }),
        },
        OperationKind::UpdateDateChip { inline_id, date } => {
            let candidate = opendoc_core::Inline::DateChip {
                id: inline_id.clone(),
                date: date.clone(),
            };
            if let Err(error) = candidate.validate() {
                warnings.push(ModelWarning {
                    code: "invalid-date-chip".to_string(),
                    message: format!("date chip {inline_id} ignored invalid date: {error}"),
                });
            } else {
                match update_date_chip(document, &inline_id, &date) {
                    Some(true) => {}
                    Some(false) => warnings.push(ModelWarning {
                        code: "non-date-chip-inline".to_string(),
                        message: format!("inline {inline_id} is not a date chip"),
                    }),
                    None => warnings.push(ModelWarning {
                        code: "missing-inline".to_string(),
                        message: format!("date chip inline {inline_id} was missing"),
                    }),
                }
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
        OperationKind::UpdateImageLayout { block_id, layout } => {
            if let Err(err) = layout.validate() {
                warnings.push(ModelWarning {
                    code: "invalid-image-layout".to_string(),
                    message: format!("image target {block_id} ignored invalid layout: {err}"),
                });
                return;
            }
            match update_image_layout(&mut document.blocks, &block_id, &layout) {
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
            kind,
        } => {
            if level > 8 {
                warnings.push(ModelWarning {
                    code: "invalid-list-level".to_string(),
                    message: format!("list item block {block_id} ignored invalid level {level}"),
                });
                return;
            }
            match update_list_item(&mut document.blocks, &block_id, level, kind) {
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
        OperationKind::SetListStart {
            list_id,
            level,
            start,
        } => {
            if level > 8 || start == 0 {
                warnings.push(ModelWarning {
                    code: "invalid-list-start".to_string(),
                    message: format!(
                        "list {list_id} ignored invalid ordered-list start {start} at level {level}"
                    ),
                });
                return;
            }
            let exists_as_ordered_level = document.blocks.iter().any(|block| {
                matches!(
                    block.kind,
                    opendoc_core::BlockKind::ListItem {
                        list_id: ref item_list_id,
                        level: item_level,
                        kind,
                    } if item_list_id == &list_id && item_level == level && kind.is_ordered()
                )
            });
            if !exists_as_ordered_level {
                warnings.push(ModelWarning {
                    code: "missing-ordered-list-level".to_string(),
                    message: format!("ordered list {list_id} at level {level} was missing"),
                });
                return;
            }
            let properties = document.list_properties.entry(list_id).or_default();
            if start == 1 {
                properties.ordered_starts.remove(&level);
                if properties.is_empty() {
                    document
                        .list_properties
                        .retain(|_, properties| !properties.is_empty());
                }
            } else {
                properties.ordered_starts.insert(level, start);
            }
        }
        OperationKind::SetListFormat {
            list_id,
            level,
            format,
        } => {
            if level > 8 {
                warnings.push(ModelWarning {
                    code: "invalid-list-format".to_string(),
                    message: format!(
                        "list {list_id} ignored invalid ordered-list format level {level}"
                    ),
                });
                return;
            }
            let exists_as_ordered_level = document.blocks.iter().any(|block| {
                matches!(
                    block.kind,
                    opendoc_core::BlockKind::ListItem {
                        list_id: ref item_list_id,
                        level: item_level,
                        kind,
                    } if item_list_id == &list_id && item_level == level && kind.is_ordered()
                )
            });
            if !exists_as_ordered_level {
                warnings.push(ModelWarning {
                    code: "missing-ordered-list-level".to_string(),
                    message: format!("ordered list {list_id} at level {level} was missing"),
                });
                return;
            }
            let properties = document.list_properties.entry(list_id).or_default();
            if format == opendoc_core::OrderedListFormat::inherited_at(level) {
                properties.ordered_formats.remove(&level);
                if properties.is_empty() {
                    document
                        .list_properties
                        .retain(|_, properties| !properties.is_empty());
                }
            } else {
                properties.ordered_formats.insert(level, format);
            }
        }
        OperationKind::SetListBulletMarker {
            list_id,
            level,
            marker,
        } => {
            if level > 8 {
                warnings.push(ModelWarning {
                    code: "invalid-list-bullet-marker".to_string(),
                    message: format!("list {list_id} ignored invalid bullet marker level {level}"),
                });
                return;
            }
            if matches!(
                &marker,
                opendoc_core::BulletListMarker::Custom(glyph)
                    if !opendoc_core::BulletListMarker::valid_custom(glyph)
            ) {
                warnings.push(ModelWarning {
                    code: "invalid-list-bullet-marker".to_string(),
                    message: format!(
                        "list {list_id} ignored an unsafe custom bullet marker at level {level}"
                    ),
                });
                return;
            }
            let exists_as_bullet_level = document.blocks.iter().any(|block| {
                matches!(
                    block.kind,
                    opendoc_core::BlockKind::ListItem {
                        list_id: ref item_list_id,
                        level: item_level,
                        kind: opendoc_core::ListKind::Bullet,
                    } if item_list_id == &list_id && item_level == level
                )
            });
            if !exists_as_bullet_level {
                warnings.push(ModelWarning {
                    code: "missing-bullet-list-level".to_string(),
                    message: format!("bullet list {list_id} at level {level} was missing"),
                });
                return;
            }
            let properties = document.list_properties.entry(list_id).or_default();
            if marker == opendoc_core::BulletListMarker::inherited_at(level) {
                properties.bullet_markers.remove(&level);
                if properties.is_empty() {
                    document
                        .list_properties
                        .retain(|_, properties| !properties.is_empty());
                }
            } else {
                properties.bullet_markers.insert(level, marker);
            }
        }
        OperationKind::SetBlockProperty { block_id, property } => {
            if let Err(err) = property.validate() {
                warnings.push(ModelWarning {
                    code: "invalid-block-property".to_string(),
                    message: format!("block {block_id} ignored invalid property value: {err}"),
                });
                return;
            }
            match set_block_property(&mut document.blocks, &block_id, property) {
                Some(()) => {}
                None => warnings.push(ModelWarning {
                    code: "missing-block".to_string(),
                    message: format!("block property target {block_id} was missing"),
                }),
            }
        }
        OperationKind::SetPageSetup { page_setup } => match page_setup.validate() {
            Ok(()) => document.page_setup = page_setup,
            Err(err) => warnings.push(ModelWarning {
                code: "invalid-page-setup".to_string(),
                message: format!("page setup update was ignored: {err}"),
            }),
        },
        OperationKind::SetPageFurniture { slot, blocks } => {
            // Page furniture shares the document's block-id space, so the
            // only validator that can see a collision is the document's own.
            // It is consulted before the edit as well, so an operation is
            // blamed for breaking the document only if the document was
            // whole when it arrived.
            let was_valid = document.validate().is_ok();
            let previous = std::mem::replace(document.furniture_mut(slot), blocks);
            if was_valid {
                if let Err(err) = document.validate() {
                    *document.furniture_mut(slot) = previous;
                    warnings.push(ModelWarning {
                        code: "invalid-page-furniture".to_string(),
                        message: format!("{} content was rejected: {err}", slot.as_str()),
                    });
                }
            }
        }
        OperationKind::ClearPageFurnitureOverride { slot } => {
            if !slot.is_override() {
                warnings.push(ModelWarning {
                    code: "invalid-page-furniture-override".to_string(),
                    message: format!(
                        "{} is ordinary furniture and cannot inherit from itself",
                        slot.as_str()
                    ),
                });
            } else {
                document.clear_furniture_override(slot);
            }
        }
        OperationKind::ClearBlockProperty { block_id, key } => {
            match clear_block_property(&mut document.blocks, &block_id, key) {
                Some(()) => {}
                None => warnings.push(ModelWarning {
                    code: "missing-block".to_string(),
                    message: format!("block property target {block_id} was missing"),
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
            position,
            row,
            cell_columns,
        } => {
            if !table_row_payload_valid_for_merge(&row, warnings) {
                return;
            }
            let (outcome, binding) =
                insert_table_row(document, &table_block_id, position, row, &cell_columns);
            if binding.legacy {
                warnings.push(ModelWarning {
                    code: "legacy-table-row-binding".to_string(),
                    message: format!(
                        "table row insert into block {table_block_id} named no column for its cells; \
                         it predates the cell-to-column binding and was read positionally"
                    ),
                });
            }
            if binding.dropped_cells > 0 {
                warnings.push(ModelWarning {
                    code: "table-row-cell-column-deleted".to_string(),
                    message: format!(
                        "{} cell(s) of a row inserted into block {table_block_id} named a column that \
                         no longer exists and were dropped with it",
                        binding.dropped_cells
                    ),
                });
            }
            match outcome {
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
                _ => {}
            }
            repair_table(document, &table_block_id, warnings);
        }
        OperationKind::DeleteTableRow {
            table_block_id,
            row_id,
        } => {
            match delete_table_row(document, &table_block_id, &row_id) {
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
                _ => {}
            }
            repair_table(document, &table_block_id, warnings);
        }
        OperationKind::InsertTableCell {
            table_block_id,
            row_id,
            position,
            cell,
        } => {
            if !table_cell_payload_valid_for_merge(&cell, warnings) {
                return;
            }
            match insert_table_cell(document, &table_block_id, &row_id, position, cell) {
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
                    message: format!("table cell insert target block {table_block_id} was missing"),
                }),
                TableEditResult::NonTableBlock => warnings.push(ModelWarning {
                    code: "non-table-block".to_string(),
                    message: format!("block {table_block_id} is not a table"),
                }),
                TableEditResult::MissingRow => warnings.push(ModelWarning {
                    code: "missing-table-row".to_string(),
                    message: format!("table cell insert target row {row_id} was missing"),
                }),
                _ => {}
            }
            repair_table(document, &table_block_id, warnings);
        }
        OperationKind::DeleteTableCell {
            table_block_id,
            row_id,
            cell_id,
        } => {
            match delete_table_cell(document, &table_block_id, &row_id, &cell_id) {
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
                _ => {}
            }
            repair_table(document, &table_block_id, warnings);
        }
        OperationKind::InsertTableColumn {
            table_block_id,
            position,
            column,
        } => {
            if let Err(err) = column.validate() {
                warnings.push(ModelWarning {
                    code: "invalid-table-column".to_string(),
                    message: format!("table {table_block_id} ignored an invalid column: {err}"),
                });
                return;
            }
            match insert_table_column(document, &table_block_id, position, column) {
                TableEditResult::Applied => {}
                TableEditResult::AnchorDegraded => warnings.push(ModelWarning {
                    code: "table-column-anchor-degraded".to_string(),
                    message: format!(
                        "table column insert anchor was missing in block {table_block_id}; column was appended"
                    ),
                }),
                TableEditResult::Duplicate => warnings.push(ModelWarning {
                    code: "duplicate-table-column".to_string(),
                    message: "table column was already present".to_string(),
                }),
                TableEditResult::NonTableBlock => warnings.push(ModelWarning {
                    code: "non-table-block".to_string(),
                    message: format!("block {table_block_id} is not a table"),
                }),
                TableEditResult::MissingTable => warnings.push(ModelWarning {
                    code: "missing-block".to_string(),
                    message: format!(
                        "table column insert target block {table_block_id} was missing"
                    ),
                }),
                _ => {}
            }
            repair_table(document, &table_block_id, warnings);
        }
        OperationKind::DeleteTableColumn {
            table_block_id,
            column_id,
        } => {
            match delete_table_column(document, &table_block_id, &column_id) {
                TableEditResult::Applied => {}
                TableEditResult::AnchorDegraded => warnings.push(ModelWarning {
                    code: "table-column-delete-degraded".to_string(),
                    message: format!(
                        "table block {table_block_id} kept an empty placeholder column after deleting its last column"
                    ),
                }),
                TableEditResult::MissingColumn => warnings.push(ModelWarning {
                    code: "missing-table-column".to_string(),
                    message: format!("table column {column_id} was missing"),
                }),
                TableEditResult::MissingTable => warnings.push(ModelWarning {
                    code: "missing-block".to_string(),
                    message: format!(
                        "table column delete target block {table_block_id} was missing"
                    ),
                }),
                TableEditResult::NonTableBlock => warnings.push(ModelWarning {
                    code: "non-table-block".to_string(),
                    message: format!("block {table_block_id} is not a table"),
                }),
                _ => {}
            }
            repair_table(document, &table_block_id, warnings);
        }
        OperationKind::SetTableColumnWidth {
            table_block_id,
            column_id,
            width,
        } => match set_table_column_width(document, &table_block_id, &column_id, width) {
            TableEditResult::Applied => {}
            TableEditResult::InvalidPayload => warnings.push(ModelWarning {
                code: "invalid-table-column-width".to_string(),
                message: format!("table column {column_id} ignored an out-of-range width"),
            }),
            TableEditResult::MissingColumn => warnings.push(ModelWarning {
                code: "missing-table-column".to_string(),
                message: format!("table column {column_id} was missing"),
            }),
            TableEditResult::MissingTable => warnings.push(ModelWarning {
                code: "missing-block".to_string(),
                message: format!("table column width target block {table_block_id} was missing"),
            }),
            TableEditResult::NonTableBlock => warnings.push(ModelWarning {
                code: "non-table-block".to_string(),
                message: format!("block {table_block_id} is not a table"),
            }),
            _ => {}
        },
        OperationKind::SetTableRowHeight {
            table_block_id,
            row_id,
            height,
        } => match set_table_row_height(document, &table_block_id, &row_id, height) {
            TableEditResult::Applied => {}
            TableEditResult::InvalidPayload => warnings.push(ModelWarning {
                code: "invalid-table-row-height".to_string(),
                message: format!("table row {row_id} ignored an out-of-range height"),
            }),
            TableEditResult::MissingRow => warnings.push(ModelWarning {
                code: "missing-table-row".to_string(),
                message: format!("table row {row_id} was missing"),
            }),
            TableEditResult::MissingTable => warnings.push(ModelWarning {
                code: "missing-block".to_string(),
                message: format!("table row height target block {table_block_id} was missing"),
            }),
            TableEditResult::NonTableBlock => warnings.push(ModelWarning {
                code: "non-table-block".to_string(),
                message: format!("block {table_block_id} is not a table"),
            }),
            _ => {}
        },
        OperationKind::SetTableRowHeader {
            table_block_id,
            row_id,
            header,
        } => match set_table_row_header(document, &table_block_id, &row_id, header) {
            TableEditResult::Applied => {}
            TableEditResult::MissingRow => warnings.push(ModelWarning {
                code: "missing-table-row".to_string(),
                message: format!("table row {row_id} was missing"),
            }),
            TableEditResult::MissingTable => warnings.push(ModelWarning {
                code: "missing-block".to_string(),
                message: format!("table row header target block {table_block_id} was missing"),
            }),
            TableEditResult::NonTableBlock => warnings.push(ModelWarning {
                code: "non-table-block".to_string(),
                message: format!("block {table_block_id} is not a table"),
            }),
            _ => {}
        },
        OperationKind::ReorderTableRows {
            table_block_id,
            row_ids,
        } => match reorder_table_rows(document, &table_block_id, &row_ids) {
            TableEditResult::Applied => {}
            TableEditResult::InvalidPayload => warnings.push(ModelWarning {
                code: "invalid-table-row-order".to_string(),
                message: format!(
                    "table {table_block_id} ignored an incomplete or duplicate row order"
                ),
            }),
            TableEditResult::MissingTable => warnings.push(ModelWarning {
                code: "missing-block".to_string(),
                message: format!("table row reorder target block {table_block_id} was missing"),
            }),
            TableEditResult::NonTableBlock => warnings.push(ModelWarning {
                code: "non-table-block".to_string(),
                message: format!("block {table_block_id} is not a table"),
            }),
            _ => {}
        },
        OperationKind::SetTableBorder {
            table_block_id,
            border,
        } => match set_table_border(document, &table_block_id, border) {
            TableEditResult::Applied => {}
            TableEditResult::InvalidPayload => warnings.push(ModelWarning {
                code: "invalid-table-border".to_string(),
                message: format!("table {table_block_id} ignored an invalid border"),
            }),
            TableEditResult::MissingTable => warnings.push(ModelWarning {
                code: "missing-block".to_string(),
                message: format!("table border target block {table_block_id} was missing"),
            }),
            TableEditResult::NonTableBlock => warnings.push(ModelWarning {
                code: "non-table-block".to_string(),
                message: format!("block {table_block_id} is not a table"),
            }),
            _ => {}
        },
        OperationKind::SetTableAlignment {
            table_block_id,
            alignment,
        } => match set_table_alignment(document, &table_block_id, alignment) {
            TableEditResult::Applied => {}
            TableEditResult::MissingTable => warnings.push(ModelWarning {
                code: "missing-block".to_string(),
                message: format!("table alignment target block {table_block_id} was missing"),
            }),
            TableEditResult::NonTableBlock => warnings.push(ModelWarning {
                code: "non-table-block".to_string(),
                message: format!("block {table_block_id} is not a table"),
            }),
            _ => {}
        },
        OperationKind::SetTableCellSpan { cell_id, span } => {
            if let Err(err) = span.validate() {
                warnings.push(ModelWarning {
                    code: "invalid-table-cell-span".to_string(),
                    message: format!("table cell {cell_id} ignored an invalid span: {err}"),
                });
                return;
            }
            let table_block_id = table_block_id_for_cell(&document.blocks, &cell_id);
            match set_table_cell_span(document, &cell_id, span) {
                TableEditResult::Applied => {}
                TableEditResult::SpanOutsideGrid => warnings.push(ModelWarning {
                    code: "table-cell-span-outside-grid".to_string(),
                    message: format!(
                        "table cell {cell_id} was not merged: the span reaches outside the table"
                    ),
                }),
                _ => warnings.push(ModelWarning {
                    code: "missing-table-cell".to_string(),
                    message: format!("table cell {cell_id} was missing"),
                }),
            }
            if let Some(table_block_id) = table_block_id {
                repair_table(document, &table_block_id, warnings);
            }
        }
        OperationKind::SetTableCellProperty { cell_id, property } => {
            if let Err(err) = property.validate() {
                warnings.push(ModelWarning {
                    code: "invalid-table-cell-property".to_string(),
                    message: format!("table cell {cell_id} ignored invalid property value: {err}"),
                });
                return;
            }
            match find_table_cell_mut(&mut document.blocks, &cell_id) {
                Some(cell) => {
                    cell.properties.set(property);
                }
                None => warnings.push(ModelWarning {
                    code: "missing-table-cell".to_string(),
                    message: format!("table cell property target {cell_id} was missing"),
                }),
            }
        }
        OperationKind::ClearTableCellProperty { cell_id, key } => {
            match find_table_cell_mut(&mut document.blocks, &cell_id) {
                Some(cell) => {
                    cell.properties.clear(key);
                }
                None => warnings.push(ModelWarning {
                    code: "missing-table-cell".to_string(),
                    message: format!("table cell property target {cell_id} was missing"),
                }),
            }
        }
    }
    for (thread_id, quote, context) in comment_evidence {
        let became_orphaned = document
            .comments
            .iter()
            .find(|thread| thread.id == thread_id)
            .is_some_and(|thread| {
                !thread.deleted && anchor_has_no_surviving_target(&document.blocks, &thread.anchor)
            });
        if !became_orphaned {
            continue;
        }
        let Some(thread) = document
            .comments
            .iter_mut()
            .find(|thread| thread.id == thread_id)
        else {
            continue;
        };
        thread.anchor = Anchor::Orphaned {
            quote,
            context,
            warning: "comment anchor source was deleted".to_string(),
        };
        warnings.push(ModelWarning {
            code: "comment-anchor-orphaned".to_string(),
            message: format!("comment thread {thread_id} retains its deleted anchor context"),
        });
    }
}

/// Record provenance only when this operation came through the merge fold.
/// Internal repair applications intentionally have no actor and must not
/// masquerade as user review events.
fn append_comment_history(
    history: &mut Vec<CommentHistoryEntry>,
    thread_id: &opendoc_core::StableId,
    comment_id: &opendoc_core::StableId,
    kind: &str,
    provenance: Option<OperationProvenance>,
    previous_body: Option<Vec<opendoc_core::Inline>>,
) {
    let Some(provenance) = provenance else {
        return;
    };
    history.push(CommentHistoryEntry {
        thread_id: thread_id.clone(),
        comment_id: comment_id.clone(),
        kind: kind.to_string(),
        actor: provenance.operation_actor,
        at_ms: provenance.operation_seq,
        previous_body,
    });
}

fn append_comment_activity(
    document: &mut Document,
    thread_id: &StableId,
    comment_id: Option<&StableId>,
    kind: CommentActivityKind,
    provenance: Option<OperationProvenance>,
) {
    let Some(provenance) = provenance else {
        return;
    };
    if document.comment_activity.iter().any(|entry| {
        entry.operation_actor == provenance.operation_actor
            && entry.operation_seq == provenance.operation_seq
    }) {
        return;
    }
    document.comment_activity.push(CommentActivityEntry {
        operation_actor: provenance.operation_actor.clone(),
        operation_seq: provenance.operation_seq,
        actor: provenance.operation_actor,
        at_ms: provenance.operation_seq,
        thread_id: thread_id.clone(),
        comment_id: comment_id.cloned(),
        kind,
    });
    document.comment_activity.sort_by(|left, right| {
        (left.at_ms, &left.operation_actor, left.operation_seq).cmp(&(
            right.at_ms,
            &right.operation_actor,
            right.operation_seq,
        ))
    });
    let excess = document
        .comment_activity
        .len()
        .saturating_sub(MAX_COMMENT_ACTIVITY_ENTRIES);
    if excess != 0 {
        document.comment_activity.drain(..excess);
    }
}

pub(crate) fn apply_mark_range(
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
