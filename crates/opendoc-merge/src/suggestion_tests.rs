//! Suggestion tests.

use crate::causal::{ActorId, OperationId};
use crate::inline_ops::inline_id;
use crate::merge::merge_operations;
use crate::operation::{Operation, OperationKind};
use opendoc_core::{
    Anchor, Block, Document, Equation, EquationSourceFormat, Inline, Mark, MarkExpand, MarkKind,
    StableId, Suggestion, SuggestionKind, SuggestionState, TextRange,
};

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
            context: None,
        },
        Operation {
            id: OperationId {
                actor: ActorId("b".to_string()),
                seq: 1,
            },
            kind: OperationKind::AddSuggestion { suggestion },
            context: None,
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
            context: None,
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
        context: None,
    };
    let second = Operation {
        id: OperationId {
            actor: ActorId("b".to_string()),
            seq: 1,
        },
        kind: OperationKind::AddSuggestion {
            suggestion: suggestion("Bob", "second"),
        },
        context: None,
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
                context: None,
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
                context: None,
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
        context: None,
    };
    let add = Operation {
        id: OperationId {
            actor: ActorId("actor-z".to_string()),
            seq: 1,
        },
        kind: OperationKind::AddSuggestion { suggestion },
        context: None,
    };

    let accept_first = merge_operations(&base, &[vec![accept.clone()], vec![add.clone()]]).unwrap();
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
        context: None,
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
        context: None,
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
            context: None,
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
            context: None,
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
            context: None,
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
            context: None,
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
                context: None,
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
                context: None,
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
            context: None,
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
            context: None,
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
            context: None,
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
        context: None,
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
        context: None,
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
        context: None,
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
        context: None,
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
            context: None,
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
                context: None,
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
                context: None,
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
                context: None,
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
            context: None,
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
            context: None,
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
            context: None,
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
            context: None,
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
                context: None,
            },
            Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 2,
                },
                kind: OperationKind::DeleteInline {
                    inline_id: second_id,
                },
                context: None,
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
