//! Comment tests.

use crate::causal::{ActorId, OperationId};
use crate::inline_ops::inline_id;
use crate::merge::merge_operations;
use crate::operation::{Operation, OperationKind};
use opendoc_core::{Anchor, Block, Comment, CommentThread, Document, Inline, StableId, TextRange};
use std::collections::BTreeSet;

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
        state: opendoc_core::CommentThreadState::Open,
        resolved_by: None,
        resolved_at_ms: None,
        action_assignee: None,
        action_due_at_ms: None,
        action_completed_by: None,
        action_completed_at_ms: None,
        reactions: Vec::new(),
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
            context: None,
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
fn imported_orphaned_comment_keeps_its_source_evidence_without_a_false_repair_warning() {
    let base = Document::new("Doc");
    let thread = CommentThread {
        id: StableId::parse("imported-orphan-thread").unwrap(),
        anchor: Anchor::Orphaned {
            quote: "removed source".to_string(),
            context: "the removed source paragraph".to_string(),
            warning: "imported orphaned comment anchor".to_string(),
        },
        comments: vec![Comment {
            id: StableId::parse("imported-orphan-comment").unwrap(),
            author: "Alice".to_string(),
            body: vec![Inline::text("keep this review context")],
            created_at_ms: 1,
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
    };
    let result = merge_operations(
        &base,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("importer".to_string()),
                seq: 1,
            },
            kind: OperationKind::AddCommentThread { thread },
            context: None,
        }]],
    )
    .expect("imported orphan remains valid");

    assert!(matches!(
        &result.document.comments[0].anchor,
        Anchor::Orphaned { quote, context, .. }
            if quote == "removed source" && context == "the removed source paragraph"
    ));
    assert!(result
        .warnings
        .iter()
        .all(|warning| warning.code != "comment-anchor-degraded"));
}

#[test]
fn completion_without_assignment_is_rejected_before_it_can_hide_from_action_queues() {
    let base = Document::new("Doc");
    let thread_id = StableId::parse("comment-thread-action").unwrap();
    let thread = CommentThread {
        id: thread_id.clone(),
        anchor: Anchor::Document,
        comments: vec![Comment {
            id: StableId::parse("comment-action").unwrap(),
            author: "Alice".to_string(),
            body: vec![Inline::text("note")],
            created_at_ms: 1,
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
    };
    let result = merge_operations(
        &base,
        &[vec![
            Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::AddCommentThread { thread },
                context: None,
            },
            Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 2,
                },
                kind: OperationKind::SetCommentThreadAction {
                    thread_id: thread_id.clone(),
                    assignee: None,
                    due_at_ms: None,
                    completed_by: Some("Alice".to_string()),
                    completed_at_ms: Some(2),
                },
                context: None,
            },
        ]],
    )
    .unwrap();

    let stored = &result.document.comments[0];
    assert!(stored.action_assignee.is_none());
    assert!(stored.action_completed_by.is_none());
    assert_eq!(result.warnings.len(), 1);
    assert_eq!(result.warnings[0].code, "invalid-comment-action");
    result
        .document
        .validate()
        .expect("invalid action operation leaves a valid document");
}

#[test]
fn incomplete_remote_action_completion_is_warned_without_corrupting_thread_state() {
    let base = Document::new("Doc");
    let thread_id = StableId::parse("comment-thread-incomplete-action").unwrap();
    let thread = CommentThread {
        id: thread_id.clone(),
        anchor: Anchor::Document,
        comments: vec![Comment {
            id: StableId::parse("comment-incomplete-action").unwrap(),
            author: "Alice".to_string(),
            body: vec![Inline::text("note")],
            created_at_ms: 1,
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
    };
    let result = merge_operations(
        &base,
        &[vec![
            Operation {
                id: OperationId {
                    actor: ActorId("importer".to_string()),
                    seq: 1,
                },
                kind: OperationKind::AddCommentThread { thread },
                context: None,
            },
            Operation {
                id: OperationId {
                    actor: ActorId("importer".to_string()),
                    seq: 2,
                },
                kind: OperationKind::SetCommentThreadAction {
                    thread_id,
                    assignee: Some("Alice".to_string()),
                    due_at_ms: None,
                    completed_by: Some("Alice".to_string()),
                    completed_at_ms: None,
                },
                context: None,
            },
        ]],
    )
    .expect("malformed completion is isolated to its operation");

    let stored = &result.document.comments[0];
    assert!(stored.action_assignee.is_none());
    assert!(stored.action_completed_by.is_none());
    assert!(stored.action_completed_at_ms.is_none());
    assert_eq!(result.warnings[0].code, "invalid-comment-action");
    result
        .document
        .validate()
        .expect("bad replayed metadata cannot poison the document");
}

#[test]
fn malformed_remote_action_actor_is_ignored_before_it_can_poison_the_thread() {
    let mut base = Document::new("Doc");
    let thread_id = StableId::parse("comment-thread-invalid-action-actor").unwrap();
    base.comments.push(CommentThread {
        id: thread_id.clone(),
        anchor: Anchor::Document,
        comments: vec![Comment {
            id: StableId::parse("comment-invalid-action-actor").unwrap(),
            author: "Alice".to_string(),
            body: vec![Inline::text("note")],
            created_at_ms: 1,
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
    });
    let result = merge_operations(
        &base,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("importer".to_string()),
                seq: 1,
            },
            kind: OperationKind::SetCommentThreadAction {
                thread_id,
                assignee: Some(" Assignee".to_string()),
                due_at_ms: Some(1_700_000_000_000),
                completed_by: None,
                completed_at_ms: None,
            },
            context: None,
        }]],
    )
    .expect("malformed remote action actor is isolated to its operation");

    let thread = &result.document.comments[0];
    assert!(thread.action_assignee.is_none());
    assert!(thread.action_due_at_ms.is_none());
    assert_eq!(result.warnings[0].code, "invalid-comment-action");
    result
        .document
        .validate()
        .expect("invalid replay actor leaves a valid document");
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
        state: opendoc_core::CommentThreadState::Open,
        resolved_by: None,
        resolved_at_ms: None,
        action_assignee: None,
        action_due_at_ms: None,
        action_completed_by: None,
        action_completed_at_ms: None,
        reactions: Vec::new(),
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
            context: None,
        }]],
    )
    .unwrap();

    assert_eq!(result.warnings[0].code, "comment-anchor-orphaned");
    assert!(result.warnings[0].message.contains(thread_id.as_str()));
    assert!(matches!(
        result.document.comments[0].anchor,
        Anchor::Orphaned { ref quote, ref context, .. }
            if quote == "hello" && context == "hello"
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
        state: opendoc_core::CommentThreadState::Open,
        resolved_by: None,
        resolved_at_ms: None,
        action_assignee: None,
        action_due_at_ms: None,
        action_completed_by: None,
        action_completed_at_ms: None,
        reactions: Vec::new(),
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
        context: None,
    };
    let duplicate_add = Operation {
        id: OperationId {
            actor: ActorId("actor-comment".to_string()),
            seq: 1,
        },
        kind: OperationKind::AddCommentThread { thread },
        context: None,
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
        state: opendoc_core::CommentThreadState::Open,
        resolved_by: None,
        resolved_at_ms: None,
        action_assignee: None,
        action_due_at_ms: None,
        action_completed_by: None,
        action_completed_at_ms: None,
        reactions: Vec::new(),
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
            context: None,
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
        state: opendoc_core::CommentThreadState::Open,
        resolved_by: None,
        resolved_at_ms: None,
        action_assignee: None,
        action_due_at_ms: None,
        action_completed_by: None,
        action_completed_at_ms: None,
        reactions: Vec::new(),
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
        context: None,
    };
    let restore = Operation {
        id: OperationId {
            actor: ActorId("a".to_string()),
            seq: 1,
        },
        kind: OperationKind::RestoreCommentThread { thread_id },
        context: None,
    };

    // The positive control. "Restore wins" is the base state, so without this
    // the test also passed for a `merge_operations` that dropped every
    // operation: the delete has to be able to land before "the restore beat
    // it" means anything. PLAN88 §7.
    let deleted_only = merge_operations(&base, &[vec![delete.clone()]]).unwrap();
    assert!(deleted_only.document.comments[0].deleted);

    let restore_first =
        merge_operations(&base, &[vec![restore.clone()], vec![delete.clone()]]).unwrap();
    let delete_first = merge_operations(&base, &[vec![delete], vec![restore]]).unwrap();

    assert_eq!(restore_first.document, delete_first.document);
    assert!(!restore_first.document.comments[0].deleted);
    assert!(!restore_first.document.comments[0].comments[0].deleted);
    assert!(restore_first.warnings.is_empty());
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
        state: opendoc_core::CommentThreadState::Open,
        resolved_by: None,
        resolved_at_ms: None,
        action_assignee: None,
        action_due_at_ms: None,
        action_completed_by: None,
        action_completed_at_ms: None,
        reactions: Vec::new(),
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
        context: None,
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
        context: None,
    };

    // The positive control, as above: the delete has to land on its own, or
    // "the restore beat it" is satisfied by a merge that does nothing.
    let deleted_only = merge_operations(&base, &[vec![delete.clone()]]).unwrap();
    assert!(deleted_only.document.comments[0].comments[0].deleted);

    let restore_first =
        merge_operations(&base, &[vec![restore.clone()], vec![delete.clone()]]).unwrap();
    let delete_first = merge_operations(&base, &[vec![delete], vec![restore]]).unwrap();

    assert_eq!(restore_first.document, delete_first.document);
    assert!(!restore_first.document.comments[0].deleted);
    assert!(!restore_first.document.comments[0].comments[0].deleted);
    assert!(restore_first.warnings.is_empty());
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
        state: opendoc_core::CommentThreadState::Open,
        resolved_by: None,
        resolved_at_ms: None,
        action_assignee: None,
        action_due_at_ms: None,
        action_completed_by: None,
        action_completed_at_ms: None,
        reactions: Vec::new(),
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
        context: None,
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
        context: None,
    };
    let delete = Operation {
        id: OperationId {
            actor: ActorId("actor-z".to_string()),
            seq: 1,
        },
        kind: OperationKind::DeleteCommentThread {
            thread_id: thread_id.clone(),
        },
        context: None,
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
        state: opendoc_core::CommentThreadState::Open,
        resolved_by: None,
        resolved_at_ms: None,
        action_assignee: None,
        action_due_at_ms: None,
        action_completed_by: None,
        action_completed_at_ms: None,
        reactions: Vec::new(),
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
        context: None,
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
        context: None,
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
        state: opendoc_core::CommentThreadState::Open,
        resolved_by: None,
        resolved_at_ms: None,
        action_assignee: None,
        action_due_at_ms: None,
        action_completed_by: None,
        action_completed_at_ms: None,
        reactions: Vec::new(),
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
        context: None,
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
        context: None,
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
        state: opendoc_core::CommentThreadState::Open,
        resolved_by: None,
        resolved_at_ms: None,
        action_assignee: None,
        action_due_at_ms: None,
        action_completed_by: None,
        action_completed_at_ms: None,
        reactions: Vec::new(),
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
            context: None,
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
        state: opendoc_core::CommentThreadState::Open,
        resolved_by: None,
        resolved_at_ms: None,
        action_assignee: None,
        action_due_at_ms: None,
        action_completed_by: None,
        action_completed_at_ms: None,
        reactions: Vec::new(),
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
        context: None,
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
        context: None,
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
        state: opendoc_core::CommentThreadState::Open,
        resolved_by: None,
        resolved_at_ms: None,
        action_assignee: None,
        action_due_at_ms: None,
        action_completed_by: None,
        action_completed_at_ms: None,
        reactions: Vec::new(),
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
                context: None,
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
                context: None,
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
                context: None,
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
        state: opendoc_core::CommentThreadState::Open,
        resolved_by: None,
        resolved_at_ms: None,
        action_assignee: None,
        action_due_at_ms: None,
        action_completed_by: None,
        action_completed_at_ms: None,
        reactions: Vec::new(),
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
            context: None,
        }]],
    )
    .unwrap();

    assert_eq!(result.document.blocks.len(), 1);
    assert_eq!(result.document.blocks[0].id, survivor_id);
    assert_eq!(result.warnings[0].code, "comment-anchor-orphaned");
    assert!(matches!(
        &result.document.comments[0].anchor,
        Anchor::Orphaned { quote, context, .. }
            if quote == "delete target" && context == "delete target"
    ));
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
        state: opendoc_core::CommentThreadState::Open,
        resolved_by: None,
        resolved_at_ms: None,
        action_assignee: None,
        action_due_at_ms: None,
        action_completed_by: None,
        action_completed_at_ms: None,
        reactions: Vec::new(),
        deleted: false,
    };
    let add = Operation {
        id: OperationId {
            actor: ActorId("a".to_string()),
            seq: 1,
        },
        kind: OperationKind::AddCommentThread { thread },
        context: None,
    };
    let delete = Operation {
        id: OperationId {
            actor: ActorId("z".to_string()),
            seq: 1,
        },
        kind: OperationKind::DeleteInline { inline_id: text_id },
        context: None,
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
        0,
        "the real deletion records orphan evidence; the repair pass must not add a false nearest-anchor warning"
    );
    assert_eq!(
        result_add_first
            .warnings
            .iter()
            .filter(|warning| warning.code == "comment-anchor-orphaned")
            .count(),
        1
    );
    assert!(matches!(
        result_add_first.document.comments[0].anchor,
        Anchor::Orphaned { ref quote, ref context, .. }
            if quote == "hello" && context == "hello"
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
        state: opendoc_core::CommentThreadState::Open,
        resolved_by: None,
        resolved_at_ms: None,
        action_assignee: None,
        action_due_at_ms: None,
        action_completed_by: None,
        action_completed_at_ms: None,
        reactions: Vec::new(),
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
        context: None,
    };
    let second = Operation {
        id: OperationId {
            actor: ActorId("b".to_string()),
            seq: 1,
        },
        kind: OperationKind::AddCommentThread {
            thread: comment_thread("comment-duplicate-merge-b", "second"),
        },
        context: None,
    };

    let result = merge_operations(&base, &[vec![first], vec![second]]).unwrap();
    assert_eq!(result.document.comments.len(), 1);
    assert!(result
        .warnings
        .iter()
        .any(|warning| warning.code == "duplicate-comment-thread"));
}

#[test]
fn invalid_comment_thread_add_degrades_to_warning() {
    let mut base = Document::new("Doc");
    base.blocks.push(Block::paragraph("hello"));
    let invalid_thread = CommentThread {
        id: StableId::parse("comment-thread-invalid-merge").unwrap(),
        anchor: Anchor::Document,
        comments: Vec::new(),
        state: opendoc_core::CommentThreadState::Open,
        resolved_by: None,
        resolved_at_ms: None,
        action_assignee: None,
        action_due_at_ms: None,
        action_completed_by: None,
        action_completed_at_ms: None,
        reactions: Vec::new(),
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
            context: None,
        }]],
    )
    .unwrap();

    assert!(result.document.comments.is_empty());
    assert!(result
        .warnings
        .iter()
        .any(|warning| warning.code == "invalid-comment-thread"));
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
        state: opendoc_core::CommentThreadState::Open,
        resolved_by: None,
        resolved_at_ms: None,
        action_assignee: None,
        action_due_at_ms: None,
        action_completed_by: None,
        action_completed_at_ms: None,
        reactions: Vec::new(),
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
            context: None,
        }]],
    )
    .unwrap();

    assert!(result.document.comments.is_empty());
    assert_eq!(result.warnings[0].code, "invalid-comment-thread");
}

#[test]
fn comment_edit_and_delete_keep_append_only_actor_provenance() {
    let mut base = Document::new("Doc");
    let thread_id = StableId::parse("history-thread").unwrap();
    let comment_id = StableId::parse("history-comment").unwrap();
    base.comments.push(CommentThread {
        id: thread_id.clone(),
        anchor: Anchor::Document,
        comments: vec![Comment {
            id: comment_id.clone(),
            author: "Alice".to_string(),
            body: vec![Inline::text("first version")],
            created_at_ms: 1,
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
    });
    let edited = merge_operations(
        &base,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("editor-a".to_string()),
                seq: 7,
            },
            kind: OperationKind::UpdateCommentBody {
                thread_id: thread_id.clone(),
                comment_id: comment_id.clone(),
                body: vec![Inline::text("second version")],
            },
            context: None,
        }]],
    )
    .unwrap();
    let result = merge_operations(
        &edited.document,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("moderator-b".to_string()),
                seq: 8,
            },
            kind: OperationKind::DeleteComment {
                thread_id,
                comment_id,
            },
            context: None,
        }]],
    )
    .unwrap();
    assert_eq!(result.document.comment_history.len(), 2);
    assert_eq!(result.document.comment_history[0].kind, "edited");
    assert_eq!(result.document.comment_history[0].actor, "editor-a");
    assert!(matches!(
        result.document.comment_history[0].previous_body.as_deref(),
        Some([Inline::Text { text, .. }]) if text == "first version"
    ));
    assert_eq!(result.document.comment_history[1].kind, "deleted");
    assert_eq!(result.document.comment_history[1].actor, "moderator-b");
    assert!(matches!(
        result.document.comment_history[1].previous_body.as_deref(),
        Some([Inline::Text { text, .. }]) if text == "second version"
    ));
    result.document.validate().unwrap();
}

#[test]
fn comment_activity_is_replayable_and_retains_final_comment_tombstones() {
    let thread_id = StableId::parse("activity-thread").unwrap();
    let comment_id = StableId::parse("activity-comment").unwrap();
    let mut base = Document::new("Doc");
    base.comments.push(CommentThread {
        id: thread_id.clone(),
        anchor: Anchor::Document,
        comments: vec![Comment {
            id: comment_id.clone(),
            author: "Ada".to_string(),
            body: vec![Inline::text("Review")],
            created_at_ms: 1,
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
    });
    let delete = Operation {
        id: OperationId {
            actor: ActorId("reviewer".to_string()),
            seq: 9,
        },
        kind: OperationKind::DeleteComment {
            thread_id: thread_id.clone(),
            comment_id: comment_id.clone(),
        },
        context: None,
    };
    let result = merge_operations(&base, &[vec![delete.clone()], vec![delete]]).unwrap();
    assert!(result.document.comments[0].deleted);
    assert_eq!(result.document.comment_activity.len(), 1);
    let activity = &result.document.comment_activity[0];
    assert_eq!(activity.operation_actor, "reviewer");
    assert_eq!(activity.operation_seq, 9);
    assert_eq!(activity.thread_id, thread_id);
    assert_eq!(activity.comment_id.as_ref(), Some(&comment_id));
    assert_eq!(
        activity.kind,
        opendoc_core::CommentActivityKind::CommentDeleted
    );
    result.document.validate().unwrap();
}

#[test]
fn deferred_comment_restore_keeps_its_operation_activity_attribution() {
    let thread_id = StableId::parse("activity-restore-thread").unwrap();
    let comment_id = StableId::parse("activity-restore-comment").unwrap();
    let mut base = Document::new("Doc");
    base.comments.push(CommentThread {
        id: thread_id.clone(),
        anchor: Anchor::Document,
        comments: vec![Comment {
            id: comment_id.clone(),
            author: "Ada".to_string(),
            body: vec![Inline::text("Review")],
            created_at_ms: 1,
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
    });
    let result = merge_operations(
        &base,
        &[vec![
            Operation {
                id: OperationId {
                    actor: ActorId("deleter".to_string()),
                    seq: 1,
                },
                kind: OperationKind::DeleteComment {
                    thread_id: thread_id.clone(),
                    comment_id: comment_id.clone(),
                },
                context: None,
            },
            Operation {
                id: OperationId {
                    actor: ActorId("restorer".to_string()),
                    seq: 2,
                },
                kind: OperationKind::RestoreComment {
                    thread_id,
                    comment_id,
                },
                context: None,
            },
        ]],
    )
    .unwrap();
    assert_eq!(result.document.comment_activity.len(), 2);
    assert_eq!(
        result.document.comment_activity[1].operation_actor,
        "restorer"
    );
    assert_eq!(
        result.document.comment_activity[1].kind,
        opendoc_core::CommentActivityKind::CommentRestored
    );
}

#[test]
fn comment_activity_keeps_the_deterministic_final_capacity_window() {
    let thread_id = StableId::parse("activity-cap-thread").unwrap();
    let mut base = Document::new("Doc");
    base.comments.push(CommentThread {
        id: thread_id.clone(),
        anchor: Anchor::Document,
        comments: vec![Comment {
            id: StableId::parse("activity-cap-comment").unwrap(),
            author: "Ada".to_string(),
            body: vec![Inline::text("Review")],
            created_at_ms: 1,
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
    });
    let operations = (1..=1_025)
        .map(|seq| Operation {
            id: OperationId {
                actor: ActorId("reviewer".to_string()),
                seq,
            },
            kind: OperationKind::SetCommentThreadAction {
                thread_id: thread_id.clone(),
                assignee: Some(format!("Reviewer {seq}")),
                due_at_ms: None,
                completed_by: None,
                completed_at_ms: None,
            },
            context: None,
        })
        .collect::<Vec<_>>();
    let result = merge_operations(&base, &[operations]).unwrap();
    assert_eq!(result.document.comment_activity.len(), 1_024);
    assert_eq!(result.document.comment_activity[0].operation_seq, 2);
    assert_eq!(result.document.comment_activity[1_023].operation_seq, 1_025);
    result.document.validate().unwrap();
}

#[test]
fn comment_reactions_are_actor_scoped_canonical_and_invertible() {
    let thread_id = StableId::parse("reaction-thread").unwrap();
    let mut base = Document::new("Doc");
    base.comments.push(CommentThread {
        id: thread_id.clone(),
        anchor: Anchor::Document,
        comments: vec![Comment {
            id: StableId::parse("reaction-comment").unwrap(),
            author: "Ada".to_string(),
            body: vec![Inline::text("Review")],
            created_at_ms: 1,
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
    });
    let add = OperationKind::SetCommentThreadReaction {
        thread_id: thread_id.clone(),
        emoji: "👍".to_string(),
        actor: "Bea".to_string(),
        present: true,
    };
    let merged = merge_operations(
        &base,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("bea".to_string()),
                seq: 1,
            },
            kind: add.clone(),
            context: None,
        }]],
    )
    .unwrap();
    assert_eq!(merged.document.comments[0].reactions[0].actors, ["Bea"]);
    let inverse = crate::inverse::invert_operation(&base, &add);
    let crate::inverse::Inversion::Operations(operations) = inverse else {
        panic!("reaction must be invertible");
    };
    assert!(matches!(
        operations.as_slice(),
        [OperationKind::SetCommentThreadReaction { present: false, .. }]
    ));
    let invalid = OperationKind::SetCommentThreadReaction {
        thread_id,
        emoji: "not emoji".to_string(),
        actor: "Bea".to_string(),
        present: true,
    };
    let result = merge_operations(
        &base,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("bea".to_string()),
                seq: 2,
            },
            kind: invalid,
            context: None,
        }]],
    )
    .unwrap();
    assert!(result.document.comments[0].reactions.is_empty());
    assert!(result
        .warnings
        .iter()
        .any(|warning| warning.code == "invalid-comment-reaction"));
}

#[test]
fn malformed_remote_comment_resolution_actor_is_ignored_without_poisoning_document() {
    let thread_id = StableId::parse("invalid-resolution-thread").unwrap();
    let mut base = Document::new("Doc");
    base.comments.push(CommentThread {
        id: thread_id.clone(),
        anchor: Anchor::Document,
        comments: vec![Comment {
            id: StableId::parse("invalid-resolution-comment").unwrap(),
            author: "Ada".to_string(),
            body: vec![Inline::text("Review")],
            created_at_ms: 1,
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
    });

    let result = merge_operations(
        &base,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("remote".to_string()),
                seq: 1,
            },
            kind: OperationKind::ResolveCommentThread {
                thread_id,
                resolved_by: " reviewer ".to_string(),
                resolved_at_ms: 4,
            },
            context: None,
        }]],
    )
    .expect("invalid remote resolution is isolated rather than invalidating replay");

    assert_eq!(
        result.document.comments[0].state,
        opendoc_core::CommentThreadState::Open
    );
    assert!(result.document.comments[0].resolved_by.is_none());
    assert!(result
        .warnings
        .iter()
        .any(|warning| warning.code == "invalid-comment-resolution"));
    result.document.validate().unwrap();
}

#[test]
fn no_op_comment_deletes_and_edits_do_not_duplicate_provenance() {
    let thread_id = StableId::parse("no-op-history-thread").unwrap();
    let comment_id = StableId::parse("no-op-history-comment").unwrap();
    let mut base = Document::new("Doc");
    base.comments.push(CommentThread {
        id: thread_id.clone(),
        anchor: Anchor::Document,
        comments: vec![Comment {
            id: comment_id.clone(),
            author: "Ada".to_string(),
            body: vec![Inline::text("unchanged")],
            created_at_ms: 1,
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
    });
    let unchanged = OperationKind::UpdateCommentBody {
        thread_id: thread_id.clone(),
        comment_id: comment_id.clone(),
        // A byte-for-byte replay of the current structured body is the
        // no-op boundary.  Fresh inline ids, even with identical visible
        // text, are a distinct structured revision.
        body: base.comments[0].comments[0].body.clone(),
    };
    let no_op_edit = merge_operations(
        &base,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("editor".to_string()),
                seq: 1,
            },
            kind: unchanged,
            context: None,
        }]],
    )
    .unwrap();
    assert!(no_op_edit.document.comment_history.is_empty());

    let deleted = merge_operations(
        &base,
        &[vec![
            Operation {
                id: OperationId {
                    actor: ActorId("deleter".to_string()),
                    seq: 2,
                },
                kind: OperationKind::DeleteComment {
                    thread_id: thread_id.clone(),
                    comment_id: comment_id.clone(),
                },
                context: None,
            },
            Operation {
                id: OperationId {
                    actor: ActorId("deleter".to_string()),
                    seq: 3,
                },
                kind: OperationKind::DeleteComment {
                    thread_id,
                    comment_id,
                },
                context: None,
            },
        ]],
    )
    .unwrap();
    assert_eq!(deleted.document.comment_history.len(), 1);
    assert_eq!(deleted.document.comment_history[0].kind, "deleted");
    deleted.document.validate().unwrap();
}
