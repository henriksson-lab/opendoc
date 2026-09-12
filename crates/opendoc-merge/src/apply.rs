//! The apply pass: one operation kind at a time, against a mutable document.

use crate::anchors::{anchor_resolves, repair_comment_anchor};
use crate::block_edit::{
    clear_block_property, set_block_property, set_block_text_style, update_block_equation_source,
    update_heading_level, update_image_alt_text, update_image_blob_hash, update_image_layout,
    update_list_item, validate_block_text_style,
};
use crate::blocks::{block_exists, delete_block, find_block_mut, insert_block};
use crate::citations::{
    invalidate_all_citation_caches, invalidate_citation_caches_for_reference,
    invalidate_inline_citation_caches,
};
use crate::inline_edit::{
    delete_inline, update_inline_equation_source, update_inline_text, update_link_href,
    update_mention_label,
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
    insert_table_cell, insert_table_column, insert_table_row, repair_table, set_table_cell_span,
    set_table_column_width, table_block_id_for_cell, TableEditResult,
};
use crate::validate::{
    comment_thread_has_empty_body, inline_exists, inline_payload_valid_for_merge,
    inline_sequence_is_empty_source_text, inline_sequence_payload_valid_for_merge,
    insert_block_payload_valid_for_merge, mark_removal_valid_for_merge, marks_valid_for_merge,
    suggestion_insert_content_is_empty, table_cell_payload_valid_for_merge,
    table_row_payload_valid_for_merge,
};
use opendoc_core::{Document, Mark, ModelWarning, SuggestionKind, SuggestionState};

pub(crate) fn apply(
    document: &mut Document,
    warnings: &mut Vec<ModelWarning>,
    kind: OperationKind,
) {
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
            after_column,
            column,
        } => {
            if let Err(err) = column.validate() {
                warnings.push(ModelWarning {
                    code: "invalid-table-column".to_string(),
                    message: format!("table {table_block_id} ignored an invalid column: {err}"),
                });
                return;
            }
            match insert_table_column(document, &table_block_id, after_column, column) {
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
