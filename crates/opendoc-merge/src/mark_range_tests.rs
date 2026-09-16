//! Mark range tests.

use crate::causal::{ActorId, OperationId};
use crate::inline_ops::inline_id;
use crate::merge::merge_operations;
use crate::operation::{Operation, OperationKind};
use crate::test_support::assert_mark_kinds;
use opendoc_core::{
    Anchor, BibliographyReference, Block, BlockKind, BlockProperties, CitationGroup, CitationItem,
    CitationPlacement, CitationSource, CitationSourceFormat, CitationSummary, Comment,
    CommentThread, Document, Inline, InsertPosition, Mark, MarkExpand, MarkKind, StableId,
    Suggestion, SuggestionKind, SuggestionState, TextRange,
};

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
        context: None,
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
        context: None,
    };

    let merged_ab = merge_operations(&base, &[vec![bold.clone()], vec![italic.clone()]]).unwrap();
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
            position: InsertPosition::After(first_id.clone()),
            inline: inserted,
        },
        context: None,
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
        context: None,
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
                context: None,
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
                context: None,
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
                position: InsertPosition::After(original_block_id.clone()),
                block: Block {
                    id: split_block_id.clone(),
                    kind: BlockKind::Paragraph,
                    content: Vec::new(),
                    properties: BlockProperties::default(),
                },
            },
            context: None,
        },
        Operation {
            id: OperationId {
                actor: ActorId("actor-split".to_string()),
                seq: 2,
            },
            kind: OperationKind::MoveInlineToBlock {
                inline_id: second_id.clone(),
                target_block_id: split_block_id.clone(),
                position: InsertPosition::Last,
            },
            context: None,
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
            context: None,
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
                    state: opendoc_core::CommentThreadState::Open,
                    resolved_by: None,
                    resolved_at_ms: None,
                    action_assignee: None,
                    action_due_at_ms: None,
                    action_completed_by: None,
                    action_completed_at_ms: None,
                    reactions: Vec::new(),
                    deleted: false,
                },
            },
            context: None,
        },
    ];

    let split_first = merge_operations(&base, &[split_ops.clone(), review_ops.clone()]).unwrap();
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
            position: InsertPosition::After(first_id.clone()),
            inline: inserted,
        },
        context: None,
    }];
    let split_ops = vec![
        Operation {
            id: OperationId {
                actor: ActorId("actor-split".to_string()),
                seq: 1,
            },
            kind: OperationKind::InsertBlock {
                position: InsertPosition::After(original_block_id.clone()),
                block: Block {
                    id: split_block_id.clone(),
                    kind: BlockKind::Paragraph,
                    content: Vec::new(),
                    properties: BlockProperties::default(),
                },
            },
            context: None,
        },
        Operation {
            id: OperationId {
                actor: ActorId("actor-split".to_string()),
                seq: 2,
            },
            kind: OperationKind::MoveInlineToBlock {
                inline_id: second_id.clone(),
                target_block_id: split_block_id.clone(),
                position: InsertPosition::Last,
            },
            context: None,
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
            context: None,
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
                    state: opendoc_core::CommentThreadState::Open,
                    resolved_by: None,
                    resolved_at_ms: None,
                    action_assignee: None,
                    action_due_at_ms: None,
                    action_completed_by: None,
                    action_completed_at_ms: None,
                    reactions: Vec::new(),
                    deleted: false,
                },
            },
            context: None,
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
    let reversed_batches = merge_operations(&base, &[review_ops, split_ops, typing_ops]).unwrap();

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
                position: InsertPosition::After(original_block_id.clone()),
                block: Block {
                    id: split_block_id.clone(),
                    kind: BlockKind::Paragraph,
                    content: Vec::new(),
                    properties: BlockProperties::default(),
                },
            },
            context: None,
        },
        Operation {
            id: OperationId {
                actor: ActorId("actor-split".to_string()),
                seq: 2,
            },
            kind: OperationKind::MoveInlineToBlock {
                inline_id: second_id.clone(),
                target_block_id: split_block_id.clone(),
                position: InsertPosition::Last,
            },
            context: None,
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
            context: None,
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
            context: None,
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
            context: None,
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
            SuggestionKind::Delete { range }
            | SuggestionKind::Format { range, .. }
            | SuggestionKind::FormatRemove { range, .. }
            | SuggestionKind::FormatReplace { range, .. } => {
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
            SuggestionKind::BlockDelete { .. }
            | SuggestionKind::BlockInsert { .. }
            | SuggestionKind::BlockReplace { .. }
            | SuggestionKind::LinkChange { .. }
            | SuggestionKind::ParagraphStyleChange { .. } => {
                panic!("this fixture contains only inline suggestions")
            }
        }
    }
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
                position: InsertPosition::Last,
            },
            context: None,
        }]],
    )
    .unwrap();

    assert_eq!(result.document.visible_text(), "alpha\n");
    assert!(result
        .warnings
        .iter()
        .any(|warning| warning.code == "missing-block"));
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
                position: InsertPosition::Last,
            },
            context: None,
        }]],
    )
    .unwrap();

    assert_eq!(result.document.visible_text(), "target\n");
    assert!(result
        .warnings
        .iter()
        .any(|warning| warning.code == "missing-inline"));
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
                position: InsertPosition::After(StableId::parse("missing-after-inline").unwrap()),
            },
            context: None,
        }]],
    )
    .unwrap();

    assert_eq!(result.document.visible_text(), "\ntarget moved\n");
    assert!(result
        .warnings
        .iter()
        .any(|warning| warning.code == "inline-anchor-degraded"));
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
                position: InsertPosition::After(first_id.clone()),
            },
            context: None,
        },
        Operation {
            id: OperationId {
                actor: ActorId("actor-join".to_string()),
                seq: 2,
            },
            kind: OperationKind::DeleteBlock {
                block_id: second_block_id.clone(),
            },
            context: None,
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
            context: None,
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
                    state: opendoc_core::CommentThreadState::Open,
                    resolved_by: None,
                    resolved_at_ms: None,
                    action_assignee: None,
                    action_due_at_ms: None,
                    action_completed_by: None,
                    action_completed_at_ms: None,
                    reactions: Vec::new(),
                    deleted: false,
                },
            },
            context: None,
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
        context: None,
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
            context: None,
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
            context: None,
        },
        Operation {
            id: OperationId {
                actor: ActorId("b".to_string()),
                seq: 3,
            },
            kind: OperationKind::InsertInline {
                block_id: block_id.clone(),
                position: InsertPosition::After(first_id.clone()),
                inline: Inline::Citation {
                    id: citation_inline_id.clone(),
                    citation_id: citation_id.clone(),
                    rendered_cache: None,
                },
            },
            context: None,
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
            context: None,
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
                    state: opendoc_core::CommentThreadState::Open,
                    resolved_by: None,
                    resolved_at_ms: None,
                    action_assignee: None,
                    action_due_at_ms: None,
                    action_completed_by: None,
                    action_completed_at_ms: None,
                    reactions: Vec::new(),
                    deleted: false,
                },
            },
            context: None,
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
    let merged_cba = merge_operations(&base, &[review_ops, citation_ops, vec![delete]]).unwrap();

    assert_eq!(merged_abc.document, merged_cba.document);
    assert_eq!(merged_abc.warnings, merged_cba.warnings);
    assert_eq!(merged_abc.document.visible_text(), "omega(Doe, 2020)\n");
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
        Some("(Doe, 2020)")
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
}
