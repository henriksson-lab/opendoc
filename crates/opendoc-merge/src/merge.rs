//! The merge entry point: ordering operations and folding them into a document.

use crate::anchors::{repair_comment_anchors, repair_suggestion_anchors};
use crate::apply::{apply, apply_mark_range};
use crate::causal::{causal_order, ActorId, OperationId};
use crate::citations::{
    refresh_citation_projection_caches, repair_citation_placements, repair_citation_references,
    repair_inline_citation_labels,
};
use crate::footnotes::{repair_missing_footnote_references, repair_unreferenced_footnotes};
use crate::inline_edit::edit_inline_text;
use crate::inline_ops::inline_id;
use crate::operation::{Operation, OperationKind};
use crate::text_sequence::{resolve_run, RunEdit, RunEditKind};
use crate::validate::marks_valid_for_merge;
use opendoc_core::{Document, ModelError, ModelWarning, StableId};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MergeResult {
    pub document: Document,
    pub warnings: Vec<ModelWarning>,
}

pub fn merge_operations(
    base: &Document,
    streams: &[Vec<Operation>],
) -> Result<MergeResult, ModelError> {
    let mut deduplicated: BTreeMap<OperationId, Operation> = BTreeMap::new();
    let mut duplicate_operation_ids = BTreeSet::new();
    let mut invalid_operation_id_warnings = BTreeSet::new();
    let mut warnings = Vec::new();
    for stream in streams {
        for op in stream {
            if let Some(message) = validate_operation_id_for_merge(&op.id) {
                invalid_operation_id_warnings.insert(message);
                continue;
            }
            match deduplicated.get_mut(&op.id) {
                Some(existing) => {
                    duplicate_operation_ids.insert(op.id.clone());
                    if operation_payload_sort_key(op) < operation_payload_sort_key(existing) {
                        *existing = op.clone();
                    }
                }
                None => {
                    deduplicated.insert(op.id.clone(), op.clone());
                }
            }
        }
    }
    // One deterministic total order over the operation *set*, a linear
    // extension of happened-before. Not a function of stream grouping or
    // arrival order, which is what makes the merge converge byte-identically
    // however the operations were received. See ADR 0007.
    let operations: Vec<Operation> = deduplicated.into_values().collect();
    let ordered: Vec<&Operation> = causal_order(&operations)
        .into_iter()
        .map(|index| &operations[index])
        .collect();

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
    for operation in &ordered {
        let op_id = &operation.id;
        match &operation.kind {
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
    // Character operations are not applied positionally in this pass. They are
    // collected per run and resolved by the sequence CRDT once the structural
    // pass has settled, so an offset is interpreted in the context it was
    // written in rather than against a document later operations have already
    // shifted. ADR 0007.
    let mut text_run_edits: BTreeMap<StableId, Vec<RunEdit>> = BTreeMap::new();
    // The rank of the last operation that wrote a run's text wholesale.
    // Character operations ordered before it lost to it, exactly as they did
    // when the pass was sequential.
    let mut text_run_resets: BTreeMap<StableId, usize> = BTreeMap::new();
    for (rank, operation) in ordered.into_iter().enumerate() {
        let op_id = &operation.id;
        match &operation.kind {
            OperationKind::InsertText {
                inline_id,
                offset,
                text,
            } => {
                if !text.is_empty() {
                    text_run_edits
                        .entry(inline_id.clone())
                        .or_default()
                        .push(RunEdit {
                            rank,
                            id: op_id.clone(),
                            context: operation.context.clone(),
                            kind: RunEditKind::Insert {
                                offset: *offset,
                                text: text.clone(),
                            },
                        });
                }
                continue;
            }
            OperationKind::DeleteText {
                inline_id,
                start,
                end,
            } => {
                if end > start {
                    text_run_edits
                        .entry(inline_id.clone())
                        .or_default()
                        .push(RunEdit {
                            rank,
                            id: op_id.clone(),
                            context: operation.context.clone(),
                            kind: RunEditKind::Delete {
                                start: *start,
                                end: *end,
                            },
                        });
                }
                continue;
            }
            OperationKind::UpdateInlineText { inline_id, .. } => {
                text_run_resets.insert(inline_id.clone(), rank);
            }
            OperationKind::InsertInline { inline, .. } => {
                text_run_resets.insert(inline_id(inline).clone(), rank);
            }
            _ => {}
        }
        let op_id = op_id.clone();
        let kind = operation.kind.clone();
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
    apply_text_run_edits(
        &mut document,
        &mut warnings,
        text_run_edits,
        &text_run_resets,
    );
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

/// Deterministic tie-break between two payloads that claim the same
/// `OperationId`. Only one of them can be the real operation; which one does
/// not matter, only that every replica picks the same one.
pub(crate) fn operation_payload_sort_key(operation: &Operation) -> String {
    format!("{:?}|{:?}", operation.kind, operation.context)
}

/// Resolve every run that character operations touched.
///
/// Runs are handled independently: an offset never means anything outside the
/// run it names, so there is no cross-run ordering to preserve. Warnings stay
/// one per operation, as they were when each operation was applied on its own.
pub(crate) fn apply_text_run_edits(
    document: &mut Document,
    warnings: &mut Vec<ModelWarning>,
    edits_by_run: BTreeMap<StableId, Vec<RunEdit>>,
    resets: &BTreeMap<StableId, usize>,
) {
    for (inline_id_to_edit, mut edits) in edits_by_run {
        if let Some(reset_rank) = resets.get(&inline_id_to_edit) {
            edits.retain(|edit| edit.rank > *reset_rank);
        }
        if edits.is_empty() {
            continue;
        }
        let edit_count = edits.len();
        let resolved = edit_inline_text(document, &inline_id_to_edit, |value| {
            *value = resolve_run(value, &edits);
        });
        let (code, message) = match resolved {
            Some(true) => continue,
            Some(false) => (
                "non-editable-inline",
                format!("inline {inline_id_to_edit} is derived from structured state"),
            ),
            None => (
                "missing-inline",
                format!("inline {inline_id_to_edit} was missing"),
            ),
        };
        for _ in 0..edit_count {
            warnings.push(ModelWarning {
                code: code.to_string(),
                message: message.clone(),
            });
        }
    }
}

pub(crate) fn validate_operation_id_for_merge(id: &OperationId) -> Option<&'static str> {
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
