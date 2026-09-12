//! Table tests.

use crate::causal::{ActorId, OperationId};
use crate::inline_ops::inline_id;
use crate::merge::merge_operations;
use crate::operation::{Operation, OperationKind};
use crate::test_support::{table_cell, table_row};
use opendoc_core::{
    Anchor, Block, BlockKind, BlockProperties, CellSpan, Comment, CommentThread, Document, Inline,
    Mark, MarkExpand, MarkKind, StableId, Suggestion, SuggestionKind, SuggestionState, TableCell,
    TableRow, TextRange,
};

#[test]
fn insert_inline_targets_nested_table_cell_blocks() {
    let mut base = Document::new("Doc");
    let nested = Block::paragraph("cell");
    let nested_block_id = nested.id.clone();
    let after = inline_id(&nested.content[0]).clone();
    base.blocks.push(Block {
        id: StableId::parse("table-block").unwrap(),
        kind: BlockKind::table(vec![TableRow {
            id: StableId::parse("row-1").unwrap(),
            cells: vec![opendoc_core::TableCell {
                id: StableId::parse("cell-1").unwrap(),
                span: CellSpan::SINGLE,
                properties: Default::default(),
                blocks: vec![nested],
            }],
        }]),
        content: Vec::new(),
        properties: BlockProperties::default(),
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
            context: None,
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
        kind: BlockKind::table(vec![TableRow {
            id: StableId::parse("row-1").unwrap(),
            cells: vec![opendoc_core::TableCell {
                id: StableId::parse("cell-1").unwrap(),
                span: CellSpan::SINGLE,
                properties: Default::default(),
                blocks: vec![nested],
            }],
        }]),
        content: Vec::new(),
        properties: BlockProperties::default(),
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
            context: None,
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
        kind: BlockKind::table(vec![TableRow {
            id: StableId::parse("row-1").unwrap(),
            cells: vec![opendoc_core::TableCell {
                id: StableId::parse("cell-1").unwrap(),
                span: CellSpan::SINGLE,
                properties: Default::default(),
                blocks: vec![nested],
            }],
        }]),
        content: Vec::new(),
        properties: BlockProperties::default(),
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
            context: None,
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
        kind: BlockKind::table(vec![table_row(&first_row_id, "one")]),
        content: Vec::new(),
        properties: BlockProperties::default(),
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
        context: None,
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
        context: None,
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
        kind: BlockKind::table(vec![table_row(&first_row_id, "one")]),
        content: Vec::new(),
        properties: BlockProperties::default(),
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
            context: None,
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
        kind: BlockKind::table(vec![table_row(&first_row_id, "one")]),
        content: Vec::new(),
        properties: BlockProperties::default(),
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
            context: None,
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
        kind: BlockKind::table(vec![table_row(&first_row_id, "one")]),
        content: Vec::new(),
        properties: BlockProperties::default(),
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
                        span: CellSpan::SINGLE,
                        properties: Default::default(),
                        blocks: vec![Block {
                            id: StableId::parse("heading-bad-nested").unwrap(),
                            kind: BlockKind::Heading { level: 0 },
                            content: vec![Inline::text("bad")],
                            properties: BlockProperties::default(),
                        }],
                    }],
                },
            },
            context: None,
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
        kind: BlockKind::table(vec![
            table_row(&first_row_id, "one"),
            table_row(&second_row_id, "two"),
        ]),
        content: Vec::new(),
        properties: BlockProperties::default(),
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
            context: None,
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
        kind: BlockKind::table(vec![table_row(&row_id, "only")]),
        content: Vec::new(),
        properties: BlockProperties::default(),
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
            context: None,
        }]],
    )
    .unwrap();

    assert_eq!(result.document.visible_text(), "\n");
    assert_eq!(result.warnings[0].code, "table-row-delete-degraded");
    let BlockKind::Table { rows, .. } = &result.document.blocks[0].kind else {
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
        kind: BlockKind::table(vec![table_row(&first_row_id, "one")]),
        content: Vec::new(),
        properties: BlockProperties::default(),
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
            context: None,
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
        kind: BlockKind::table(vec![TableRow {
            id: row_id.clone(),
            cells: vec![table_cell(&first_cell_id, "one")],
        }]),
        content: Vec::new(),
        properties: BlockProperties::default(),
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
        context: None,
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
        context: None,
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
        kind: BlockKind::table(vec![TableRow {
            id: row_id.clone(),
            cells: vec![table_cell(&first_cell_id, "one")],
        }]),
        content: Vec::new(),
        properties: BlockProperties::default(),
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
            context: None,
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
        kind: BlockKind::table(vec![TableRow {
            id: row_id.clone(),
            cells: vec![table_cell(&first_cell_id, "one")],
        }]),
        content: Vec::new(),
        properties: BlockProperties::default(),
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
                    span: CellSpan::SINGLE,
                    properties: Default::default(),
                    blocks: Vec::new(),
                },
            },
            context: None,
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
        kind: BlockKind::table(vec![TableRow {
            id: row_id.clone(),
            cells: vec![
                table_cell(&first_cell_id, "one"),
                table_cell(&second_cell_id, "two"),
            ],
        }]),
        content: Vec::new(),
        properties: BlockProperties::default(),
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
            context: None,
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
        kind: BlockKind::table(vec![TableRow {
            id: row_id.clone(),
            cells: vec![table_cell(&cell_id, "only")],
        }]),
        content: Vec::new(),
        properties: BlockProperties::default(),
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
            context: None,
        }]],
    )
    .unwrap();

    assert_eq!(result.document.visible_text(), "\n");
    assert_eq!(result.warnings[0].code, "table-cell-delete-degraded");
    let BlockKind::Table { rows, .. } = &result.document.blocks[0].kind else {
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
        kind: BlockKind::table(vec![TableRow {
            id: row_id.clone(),
            cells: vec![table_cell(&first_cell_id, "one")],
        }]),
        content: Vec::new(),
        properties: BlockProperties::default(),
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
            context: None,
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
        kind: BlockKind::table(vec![TableRow {
            id: row_id.clone(),
            cells: vec![table_cell(&cell_id, "one")],
        }]),
        content: Vec::new(),
        properties: BlockProperties::default(),
    });

    let delete_table = Operation {
        id: OperationId {
            actor: ActorId("a".to_string()),
            seq: 1,
        },
        kind: OperationKind::DeleteBlock {
            block_id: table_block_id.clone(),
        },
        context: None,
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
        context: None,
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
        context: None,
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
        kind: BlockKind::table(vec![
            TableRow {
                id: deleted_row_id.clone(),
                cells: vec![TableCell {
                    id: deleted_cell_id,
                    span: CellSpan::SINGLE,
                    properties: Default::default(),
                    blocks: vec![Block {
                        id: deleted_block_id,
                        kind: BlockKind::Paragraph,
                        content: vec![Inline::Text {
                            id: deleted_text_id.clone(),
                            text: "stale".to_string(),
                            marks: Vec::new(),
                        }],
                        properties: BlockProperties::default(),
                    }],
                }],
            },
            table_row(&survivor_row_id, "survivor"),
        ]),
        content: Vec::new(),
        properties: BlockProperties::default(),
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
        context: None,
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
        context: None,
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
        context: None,
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
    let BlockKind::Table { rows, .. } = &actor_streams.document.blocks[0].kind else {
        panic!("expected table after deleting only one row");
    };
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, survivor_row_id);
    assert_eq!(rows[0].cells[0].id, survivor_cell_id);
}
