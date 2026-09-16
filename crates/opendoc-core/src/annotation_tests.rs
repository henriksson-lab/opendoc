use crate::*;

#[test]
fn footnote_references_require_live_targets() {
    let footnote_id = StableId::parse("footnote-1").unwrap();
    let mut doc = Document::new("Footnotes");
    doc.footnotes.push(Footnote {
        id: footnote_id.clone(),
        revision: 1,
        body: vec![Inline::text("footnote body")],
        deleted: false,
    });
    doc.blocks.push(Block {
        id: StableId::new("table"),
        kind: BlockKind::Table {
            columns: vec![TableColumn::auto()],
            properties: Default::default(),
            rows: vec![TableRow {
                id: StableId::new("row"),
                height: None,
                header: false,
                cells: vec![TableCell {
                    id: StableId::new("cell"),
                    span: CellSpan::SINGLE,
                    properties: TableCellProperties::default(),
                    blocks: vec![Block {
                        id: StableId::new("cell-block"),
                        kind: BlockKind::Paragraph,
                        content: vec![Inline::FootnoteRef {
                            id: StableId::new("footnote-ref"),
                            footnote_id: footnote_id.clone(),
                        }],
                        properties: BlockProperties::default(),
                    }],
                }],
            }],
        },
        content: Vec::new(),
        properties: BlockProperties::default(),
    });
    doc.validate().unwrap();

    doc.footnotes[0].deleted = true;
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "footnote reference target is missing"
        ))
    ));
}

#[test]
fn footnotes_require_non_empty_source_body() {
    let mut doc = Document::new("Footnotes");
    doc.footnotes.push(Footnote {
        id: StableId::parse("footnote-empty").unwrap(),
        revision: 1,
        body: vec![Inline::text(" ")],
        deleted: false,
    });

    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument("footnote body is empty"))
    ));
}

#[test]
fn missing_footnote_reference_target_is_invalid() {
    let mut doc = Document::new("Footnotes");
    doc.blocks.push(Block {
        id: StableId::new("block"),
        kind: BlockKind::Paragraph,
        content: vec![Inline::FootnoteRef {
            id: StableId::new("footnote-ref"),
            footnote_id: StableId::parse("missing-footnote").unwrap(),
        }],
        properties: BlockProperties::default(),
    });

    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "footnote reference target is missing"
        ))
    ));
}

#[test]
fn comment_threads_require_auditable_comments() {
    let mut doc = Document::new("Comments");
    doc.blocks.push(Block::paragraph("body"));
    doc.comments.push(CommentThread {
        id: StableId::parse("comment-thread").unwrap(),
        anchor: Anchor::Document,
        comments: vec![Comment {
            id: StableId::parse("comment-1").unwrap(),
            author: "Reviewer".to_string(),
            body: vec![Inline::text("Review note")],
            created_at_ms: 1,
            deleted: false,
        }],
        state: CommentThreadState::Open,
        resolved_by: None,
        resolved_at_ms: None,
        action_assignee: None,
        action_due_at_ms: None,
        action_completed_by: None,
        action_completed_at_ms: None,
        reactions: Vec::new(),
        deleted: false,
    });
    doc.validate().unwrap();

    doc.comments[0].comments[0].author = " ".to_string();
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument("comment author is empty"))
    ));
    doc.comments[0].comments[0].author = "Reviewer".to_string();
    doc.comments[0].comments[0].body.clear();
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument("comment body is empty"))
    ));
    doc.comments[0].comments[0].body = vec![Inline::text(" ")];
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument("comment body is empty"))
    ));
}

#[test]
fn comment_action_due_date_requires_assignment() {
    let mut thread = CommentThread {
        id: StableId::parse("comment-thread-due").unwrap(),
        anchor: Anchor::Document,
        comments: vec![Comment {
            id: StableId::parse("comment-due").unwrap(),
            author: "Reviewer".to_string(),
            body: vec![Inline::text("Do this")],
            created_at_ms: 1,
            deleted: false,
        }],
        state: CommentThreadState::Open,
        resolved_by: None,
        resolved_at_ms: None,
        action_assignee: None,
        action_due_at_ms: Some(1_700_000_000_000),
        action_completed_by: None,
        action_completed_at_ms: None,
        reactions: Vec::new(),
        deleted: false,
    };
    assert!(matches!(
        thread.validate(),
        Err(ModelError::InvalidDocument(
            "comment action due date has no assignee"
        ))
    ));
    thread.action_assignee = Some("Ada".to_string());
    thread.validate().unwrap();

    thread.action_due_at_ms = None;
    thread.action_assignee = None;
    thread.action_completed_by = Some("Reviewer".to_string());
    thread.action_completed_at_ms = Some(1);
    assert!(matches!(
        thread.validate(),
        Err(ModelError::InvalidDocument(
            "comment action completion has no assignee"
        ))
    ));
}

#[test]
fn comment_threads_reject_duplicate_comment_ids() {
    let mut doc = Document::new("Comments");
    let comment_id = StableId::parse("comment-1").unwrap();
    doc.comments.push(CommentThread {
        id: StableId::parse("comment-thread").unwrap(),
        anchor: Anchor::Document,
        comments: vec![
            Comment {
                id: comment_id.clone(),
                author: "Reviewer".to_string(),
                body: vec![Inline::text("First")],
                created_at_ms: 1,
                deleted: false,
            },
            Comment {
                id: comment_id,
                author: "Reviewer".to_string(),
                body: vec![Inline::text("Second")],
                created_at_ms: 2,
                deleted: true,
            },
        ],
        state: CommentThreadState::Open,
        resolved_by: None,
        resolved_at_ms: None,
        action_assignee: None,
        action_due_at_ms: None,
        action_completed_by: None,
        action_completed_at_ms: None,
        reactions: Vec::new(),
        deleted: false,
    });

    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument("duplicate comment id"))
    ));
}

#[test]
fn comment_and_suggestion_anchors_require_auditable_payloads() {
    let mut doc = Document::new("Anchors");
    doc.blocks.push(Block::paragraph("body"));
    doc.comments.push(CommentThread {
        id: StableId::parse("comment-thread").unwrap(),
        anchor: Anchor::NearestBlock {
            block_id: StableId::parse("block-retained").unwrap(),
            warning: "anchor moved after deletion".to_string(),
        },
        comments: vec![Comment {
            id: StableId::parse("comment-1").unwrap(),
            author: "Reviewer".to_string(),
            body: vec![Inline::text("Review note")],
            created_at_ms: 1,
            deleted: false,
        }],
        state: CommentThreadState::Open,
        resolved_by: None,
        resolved_at_ms: None,
        action_assignee: None,
        action_due_at_ms: None,
        action_completed_by: None,
        action_completed_at_ms: None,
        reactions: Vec::new(),
        deleted: false,
    });
    doc.validate().unwrap();

    doc.comments[0].comments[0].author = " Reviewer ".to_string();
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "comment author has surrounding whitespace"
        ))
    ));
    doc.comments[0].comments[0].author = "Reviewer".to_string();

    if let Anchor::NearestBlock { warning, .. } = &mut doc.comments[0].anchor {
        warning.clear();
    }
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "nearest block anchor warning is empty"
        ))
    ));

    if let Anchor::NearestBlock { warning, .. } = &mut doc.comments[0].anchor {
        *warning = " moved after delete ".to_string();
    }
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "nearest block anchor warning has surrounding whitespace"
        ))
    ));

    doc.comments.clear();
    doc.suggestions.push(Suggestion {
        id: StableId::parse("suggestion-1").unwrap(),
        author: "Reviewer".to_string(),
        kind: SuggestionKind::Delete {
            range: TextRange {
                start: StableId(String::new()),
                end: StableId::parse("text-end").unwrap(),
            },
        },
        state: SuggestionState::Proposed,
        provenance: Vec::new(),
    });
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument("text range start is empty"))
    ));

    doc.suggestions[0].kind = SuggestionKind::Insert {
        anchor: Anchor::NearestBlock {
            block_id: StableId(String::new()),
            warning: "anchor moved after deletion".to_string(),
        },
        content: vec![Inline::text("inserted")],
    };
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "nearest block anchor block id is empty"
        ))
    ));
}

#[test]
fn suggestions_require_auditable_payloads() {
    let mut doc = Document::new("Suggestions");
    doc.suggestions.push(Suggestion {
        id: StableId::parse("suggestion-1").unwrap(),
        author: "Reviewer".to_string(),
        kind: SuggestionKind::Insert {
            anchor: Anchor::Document,
            content: Vec::new(),
        },
        state: SuggestionState::Proposed,
        provenance: Vec::new(),
    });
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "insert suggestion content is empty"
        ))
    ));
    doc.suggestions[0].kind = SuggestionKind::Insert {
        anchor: Anchor::Document,
        content: vec![Inline::text(" ")],
    };
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "insert suggestion content is empty"
        ))
    ));

    doc.suggestions[0].kind = SuggestionKind::Format {
        range: TextRange {
            start: StableId::parse("text-1").unwrap(),
            end: StableId::parse("text-1").unwrap(),
        },
        marks: Vec::new(),
    };
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "format suggestion marks are empty"
        ))
    ));

    doc.suggestions[0].kind = SuggestionKind::Insert {
        anchor: Anchor::Document,
        content: vec![Inline::text("suggested text")],
    };
    doc.suggestions[0].provenance = vec![" ".to_string()];
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "suggestion provenance entry is empty"
        ))
    ));

    doc.suggestions[0].provenance = vec![" imported ".to_string()];
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "suggestion provenance entry has surrounding whitespace"
        ))
    ));

    doc.suggestions[0].provenance = vec!["imported".to_string(), "imported".to_string()];
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "duplicate suggestion provenance entry"
        ))
    ));

    doc.suggestions[0].provenance = Vec::new();
    doc.suggestions[0].author = " Reviewer ".to_string();
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "suggestion author has surrounding whitespace"
        ))
    ));
}

#[test]
fn comment_history_rejects_duplicate_operations_and_restore_bodies() {
    let thread_id = StableId::parse("history-thread").unwrap();
    let comment_id = StableId::parse("history-comment").unwrap();
    let mut doc = Document::new("Comment history");
    doc.comments.push(CommentThread {
        id: thread_id.clone(),
        anchor: Anchor::Document,
        comments: vec![Comment {
            id: comment_id.clone(),
            author: "Reviewer".to_string(),
            body: vec![Inline::text("live body")],
            created_at_ms: 1,
            deleted: false,
        }],
        state: CommentThreadState::Open,
        resolved_by: None,
        resolved_at_ms: None,
        action_assignee: None,
        action_due_at_ms: None,
        action_completed_by: None,
        action_completed_at_ms: None,
        reactions: Vec::new(),
        deleted: false,
    });
    let edited = CommentHistoryEntry {
        thread_id: thread_id.clone(),
        comment_id: comment_id.clone(),
        kind: "edited".to_string(),
        actor: "editor".to_string(),
        at_ms: 7,
        previous_body: Some(vec![Inline::text("old body")]),
    };
    doc.comment_history.push(edited.clone());
    doc.validate().unwrap();

    doc.comment_history.push(edited);
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "duplicate comment history operation id"
        ))
    ));
    doc.comment_history.truncate(1);
    doc.comment_history.push(CommentHistoryEntry {
        thread_id,
        comment_id,
        kind: "restored".to_string(),
        actor: "restorer".to_string(),
        at_ms: 8,
        previous_body: Some(vec![Inline::text("fabricated revision")]),
    });
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "restored comment history event has prior body"
        ))
    ));
}

#[test]
fn comment_activity_is_bounded_canonical_and_tombstone_aware() {
    let thread_id = StableId::parse("activity-thread").unwrap();
    let comment_id = StableId::parse("activity-comment").unwrap();
    let mut doc = Document::new("Comment activity");
    doc.comments.push(CommentThread {
        id: thread_id.clone(),
        anchor: Anchor::Document,
        comments: vec![Comment {
            id: comment_id.clone(),
            author: "Reviewer".to_string(),
            body: vec![Inline::text("body")],
            created_at_ms: 1,
            deleted: true,
        }],
        state: CommentThreadState::Open,
        resolved_by: None,
        resolved_at_ms: None,
        action_assignee: None,
        action_due_at_ms: None,
        action_completed_by: None,
        action_completed_at_ms: None,
        reactions: Vec::new(),
        deleted: true,
    });
    doc.comment_activity.push(CommentActivityEntry {
        operation_actor: "reviewer".to_string(),
        operation_seq: 3,
        actor: "reviewer".to_string(),
        at_ms: 3,
        thread_id: thread_id.clone(),
        comment_id: Some(comment_id.clone()),
        kind: CommentActivityKind::CommentDeleted,
    });
    doc.validate().unwrap();

    doc.comment_activity.push(doc.comment_activity[0].clone());
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "duplicate comment activity operation id"
        ))
    ));
    doc.comment_activity.truncate(1);
    doc.comment_activity[0].operation_seq = 0;
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "comment activity operation sequence is zero"
        ))
    ));
    doc.comment_activity[0].operation_seq = 3;
    doc.comment_activity[0].thread_id = StableId::parse("missing-thread").unwrap();
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "comment activity references missing thread"
        ))
    ));
}
