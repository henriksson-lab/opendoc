//! The merge entry point: ordering operations and folding them into a document.

use crate::anchors::{repair_comment_anchors, repair_suggestion_anchors};
use crate::apply::{apply, apply_mark_range, OperationProvenance};
use crate::causal::{causal_order, ActorId, OperationId};
use crate::citations::{
    refresh_citation_projection_caches, repair_citation_placements, repair_citation_references,
    repair_inline_citation_labels,
};
use crate::footnotes::{repair_missing_footnote_references, repair_unreferenced_footnotes};
use crate::inline_edit::edit_inline_text;
use crate::operation::{Operation, OperationKind};
use crate::text_sequence::{
    collect_text_run_edits, is_offset_addressed, resolve_text_sequence, RunEdit,
};
use crate::validate::marks_valid_for_merge;
use opendoc_core::{Document, ModelError, ModelWarning, StableId};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MergeResult {
    pub document: Document,
    pub warnings: Vec<ModelWarning>,
}

/// The order in which a local batch's inverse-capture fold must model the
/// merge's effects.
///
/// This is deliberately an order of the caller's indexes, rather than an
/// order of operation ids: the caller has already assigned its local causal
/// contexts in gesture order.  The merge applies ordinary operations first,
/// then suggestion resolutions, comment restores, mark ranges, and finally
/// offset-addressed character edits. A fold that applies any of those deferred
/// kinds immediately can capture an inverse from a state the batch merge never
/// has.
///
/// Keep this beside the merge's deferred passes.  `opendoc-app` needs this
/// answer while capturing inverses, and a second spelling there would drift
/// as readily as the whole-run-reset rule did.
pub fn batch_inverse_capture_order<'a>(
    batch: impl IntoIterator<Item = &'a OperationKind>,
) -> Vec<usize> {
    let mut ordinary = Vec::new();
    let mut suggestion_resolutions = Vec::new();
    let mut comment_restores = Vec::new();
    let mut mark_ranges = Vec::new();
    let mut text_run_edits = Vec::new();
    for (index, kind) in batch.into_iter().enumerate() {
        match kind {
            OperationKind::AcceptSuggestion { .. } | OperationKind::RejectSuggestion { .. } => {
                suggestion_resolutions.push(index)
            }
            OperationKind::RestoreCommentThread { .. } | OperationKind::RestoreComment { .. } => {
                comment_restores.push(index)
            }
            OperationKind::AddMarkRange { .. } => mark_ranges.push(index),
            kind if is_offset_addressed(kind) => text_run_edits.push(index),
            _ => ordinary.push(index),
        }
    }
    ordinary.extend(suggestion_resolutions);
    ordinary.extend(comment_restores);
    ordinary.extend(mark_ranges);
    ordinary.extend(text_run_edits);
    ordinary
}

pub fn merge_operations(
    base: &Document,
    streams: &[Vec<Operation>],
) -> Result<MergeResult, ModelError> {
    crate::instrument::count_document_copy();
    let mut document = base.clone();
    let warnings = merge_operations_into(&mut document, streams)?;
    Ok(MergeResult { document, warnings })
}

/// [`merge_operations`] without the copy: the same fold, written straight into
/// `document`.
///
/// `merge_operations` **is** this function applied to a clone, which is the
/// whole difference between them, so neither can drift from the other's
/// semantics. It exists because folding a batch one operation at a time —
/// which is what capturing an inverse per operation needs, since an inverse
/// has to be taken against the state its own operation applied to (ADR 0017) —
/// otherwise copies the entire document, and frees the copy it replaced, once
/// per operation. That is quadratic in the document and in the batch at once,
/// and for marking text across a long document it was the dominant cost of
/// the gesture.
///
/// **On `Err` the document is left holding the merged state that failed to
/// validate**, not the state it started in. That is the one behavioural
/// difference from `merge_operations`, which discards its copy, and it is why
/// this is not a drop-in replacement at a call site that has to leave the
/// caller's document untouched when an edit is refused. A caller that needs
/// that all-or-nothing guarantee merges into a copy — which is
/// `merge_operations`.
pub fn merge_operations_into(
    document: &mut Document,
    streams: &[Vec<Operation>],
) -> Result<Vec<ModelWarning>, ModelError> {
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

    // A character edit is the explicit migration boundary for legacy source:
    // capture the base tokens before any whole-run writer changes its visible
    // string.  Documents that already carry tokens also need the later
    // synchronization pass for structural writers, even without a character
    // edit in this batch.
    let tracks_text_sequences = !document.text_sequences.is_empty()
        || ordered
            .iter()
            .any(|operation| is_offset_addressed(&operation.kind));
    if tracks_text_sequences && document.text_sequences.is_empty() {
        document.materialize_legacy_text_sequences()?;
    }

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
                if document
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
    //
    // One pass collects them and the whole-run writes that reset a run's base;
    // the inverse computation reuses it, so the merge and an undo cannot
    // disagree about which operations are offset-addressed.
    let (text_run_edits, text_run_resets) = collect_text_run_edits(&ordered);
    for operation in ordered.iter() {
        if is_offset_addressed(&operation.kind) {
            continue;
        }
        let op_id = operation.id.clone();
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
                comment_restores.push((
                    kind,
                    OperationProvenance {
                        operation_actor: op_id.actor.0.clone(),
                        operation_seq: op_id.seq,
                    },
                ));
            }
            kind => apply(
                document,
                &mut warnings,
                kind,
                Some(OperationProvenance {
                    operation_actor: op_id.actor.0.clone(),
                    operation_seq: op_id.seq,
                }),
            ),
        }
    }
    for kind in suggestion_resolutions {
        apply(document, &mut warnings, kind, None);
    }
    for (kind, provenance) in comment_restores {
        apply(document, &mut warnings, kind, Some(provenance));
    }
    for (range, mark) in mark_ranges {
        if marks_valid_for_merge(
            std::slice::from_ref(&mark),
            &mut warnings,
            "mark range operation",
            &range.start,
        ) {
            apply_mark_range(document, &mut warnings, range, mark);
        }
    }
    if tracks_text_sequences {
        document.synchronize_text_sequences()?;
    }
    apply_text_run_edits(document, &mut warnings, text_run_edits, &text_run_resets);
    repair_comment_anchors(document, &mut warnings);
    repair_suggestion_anchors(document, &mut warnings);
    repair_citation_placements(document, &mut warnings);
    repair_missing_footnote_references(document, &mut warnings);
    repair_unreferenced_footnotes(document, &mut warnings);
    repair_citation_references(document, &mut warnings);
    repair_inline_citation_labels(document, &mut warnings);
    refresh_citation_projection_caches(document);
    document.warnings.extend(warnings.iter().cloned());
    document.validate()?;
    Ok(warnings)
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
        let Some(sequence) = document.text_sequences.get(&inline_id_to_edit).cloned() else {
            let (code, message) = match edit_inline_text(document, &inline_id_to_edit, |_| {}) {
                Some(false) => (
                    "non-editable-inline",
                    format!("inline {inline_id_to_edit} is derived from structured state"),
                ),
                None => (
                    "missing-inline",
                    format!("inline {inline_id_to_edit} was missing"),
                ),
                Some(true) => (
                    "missing-text-sequence",
                    format!("inline {inline_id_to_edit} was missing durable token source"),
                ),
            };
            for _ in 0..edit_count {
                warnings.push(ModelWarning {
                    code: code.to_string(),
                    message: message.clone(),
                });
            }
            continue;
        };
        let resolved_sequence = resolve_text_sequence(&sequence, &edits);
        let resolved_text = resolved_sequence.visible_text();
        let resolved = edit_inline_text(document, &inline_id_to_edit, |value| {
            *value = resolved_text.clone();
        });
        if resolved == Some(true) {
            document
                .text_sequences
                .insert(inline_id_to_edit.clone(), resolved_sequence);
            continue;
        }
        let (code, message) = match resolved {
            Some(false) => (
                "non-editable-inline",
                format!("inline {inline_id_to_edit} is derived from structured state"),
            ),
            None => (
                "missing-inline",
                format!("inline {inline_id_to_edit} was missing"),
            ),
            Some(true) => unreachable!("successful edit returned above"),
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
