//! Table tests.

use crate::causal::{ActorId, OperationId};
use crate::inline_ops::inline_id;
use crate::merge::merge_operations;
use crate::operation::{Operation, OperationKind};
use crate::test_support::{cell_columns, table_cell, table_row};
use opendoc_core::{
    Anchor, Block, BlockKind, BlockProperties, CellSpan, Comment, CommentThread, Document, Inline,
    InsertPosition, Mark, MarkExpand, MarkKind, StableId, Suggestion, SuggestionKind,
    SuggestionState, TableCell, TableRow, TextRange,
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
            height: None,
            header: false,
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
                position: InsertPosition::After(after),
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
            height: None,
            header: false,
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
            height: None,
            header: false,
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
        kind: {
            let row = table_row(&row_a_id, "two");
            OperationKind::InsertTableRow {
                cell_columns: cell_columns(&base, &table_block_id, &row),
                table_block_id: table_block_id.clone(),
                position: InsertPosition::After(first_row_id.clone()),
                row,
            }
        },
        context: None,
    };
    let insert_b = Operation {
        id: OperationId {
            actor: ActorId("b".to_string()),
            seq: 1,
        },
        kind: {
            let row = table_row(&row_b_id, "three");
            OperationKind::InsertTableRow {
                cell_columns: cell_columns(&base, &table_block_id, &row),
                table_block_id,
                position: InsertPosition::After(first_row_id),
                row,
            }
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
            kind: {
                let row = table_row(&first_row_id, "duplicate");
                OperationKind::InsertTableRow {
                    cell_columns: cell_columns(&base, &table_block_id, &row),
                    table_block_id,
                    position: InsertPosition::Last,
                    row,
                }
            },
            context: None,
        }]],
    )
    .unwrap();

    assert_eq!(result.document.visible_text(), "one\n");
    assert_eq!(result.warnings[0].code, "duplicate-table-row");
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
            kind: {
                let row = TableRow {
                    id: StableId::parse("row-empty").unwrap(),
                    height: None,
                    header: false,
                    cells: Vec::new(),
                };
                OperationKind::InsertTableRow {
                    cell_columns: cell_columns(&base, &table_block_id, &row),
                    table_block_id,
                    position: InsertPosition::Last,
                    row,
                }
            },
            context: None,
        }]],
    )
    .unwrap();

    assert_eq!(result.document.visible_text(), "one\n");
    assert_eq!(result.warnings[0].code, "invalid-table-row");
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
            kind: {
                let row = TableRow {
                    id: StableId::parse("row-bad-nested").unwrap(),
                    height: None,
                    header: false,
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
                };
                OperationKind::InsertTableRow {
                    cell_columns: cell_columns(&base, &table_block_id, &row),
                    table_block_id,
                    position: InsertPosition::Last,
                    row,
                }
            },
            context: None,
        }]],
    )
    .unwrap();

    assert_eq!(result.document.visible_text(), "one\n");
    assert_eq!(result.warnings[0].code, "invalid-heading-level");
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
            kind: {
                let row = table_row(&StableId::parse("row-2").unwrap(), "two");
                OperationKind::InsertTableRow {
                    cell_columns: cell_columns(&base, &table_block_id, &row),
                    table_block_id,
                    position: InsertPosition::After(StableId::parse("missing-row").unwrap()),
                    row,
                }
            },
            context: None,
        }]],
    )
    .unwrap();

    assert_eq!(result.document.visible_text(), "one\ntwo\n");
    assert_eq!(result.warnings[0].code, "table-row-anchor-degraded");
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
            height: None,
            header: false,
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
            position: InsertPosition::After(first_cell_id.clone()),
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
            position: InsertPosition::After(first_cell_id),
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
            height: None,
            header: false,
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
                position: InsertPosition::Last,
                cell: table_cell(&first_cell_id, "duplicate"),
            },
            context: None,
        }]],
    )
    .unwrap();

    assert_eq!(result.document.visible_text(), "one\n");
    assert_eq!(result.warnings[0].code, "duplicate-table-cell");
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
            height: None,
            header: false,
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
                position: InsertPosition::Last,
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
            height: None,
            header: false,
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
            height: None,
            header: false,
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
            height: None,
            header: false,
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
                position: InsertPosition::After(StableId::parse("missing-cell").unwrap()),
                cell: table_cell(&StableId::parse("cell-2").unwrap(), "two"),
            },
            context: None,
        }]],
    )
    .unwrap();

    assert_eq!(result.document.visible_text(), "one\ttwo\n");
    assert_eq!(result.warnings[0].code, "table-cell-anchor-degraded");
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
            height: None,
            header: false,
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
        kind: {
            let row = table_row(&StableId::parse("row-stale").unwrap(), "two");
            OperationKind::InsertTableRow {
                cell_columns: cell_columns(&base, &table_block_id, &row),
                table_block_id: table_block_id.clone(),
                position: InsertPosition::After(row_id.clone()),
                row,
            }
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
                height: None,
                header: false,
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
    let BlockKind::Table { rows, .. } = &actor_streams.document.blocks[0].kind else {
        panic!("expected table after deleting only one row");
    };
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, survivor_row_id);
    assert_eq!(rows[0].cells[0].id, survivor_cell_id);
}

/// Pressing Enter in a table cell has to put the new paragraph *in that cell*.
///
/// `insert_block` used to resolve its anchor only in `document.blocks`, so an
/// anchor inside a cell looked missing, `After` degraded to append, and the new
/// paragraph landed at the end of the body. `EditPlan::split_block` fell back to
/// a soft break because of it, which is what made Enter in a cell produce
/// `"line\n one"` in a single run.
#[test]
fn a_block_inserted_after_one_inside_a_table_cell_lands_in_that_cell() {
    let (mut base, _table_block_id) = crate::test_support::grid_document(2, 2);
    let anchor = StableId::parse("block-1-0").unwrap();
    let inserted = StableId::parse("block-new").unwrap();
    let body_blocks_before = base.blocks.len();

    let operation = Operation {
        id: OperationId {
            actor: ActorId("alice".to_string()),
            seq: 1,
        },
        context: None,
        kind: OperationKind::InsertBlock {
            position: InsertPosition::After(anchor.clone()),
            block: Block {
                id: inserted.clone(),
                kind: BlockKind::Paragraph,
                content: vec![Inline::Text {
                    id: StableId::parse("text-new").unwrap(),
                    text: "second line".to_string(),
                    marks: Vec::new(),
                }],
                properties: BlockProperties::default(),
            },
        },
    };

    let merged = merge_operations(&base, &[vec![operation]]).unwrap();

    // The body did not grow: the paragraph is not loose at the end of the document.
    assert_eq!(
        merged.document.blocks.len(),
        body_blocks_before,
        "the inserted block escaped the table and landed in the body"
    );
    let BlockKind::Table { rows, .. } = &merged.document.blocks[0].kind else {
        panic!("expected the table");
    };
    let cell = &rows[1].cells[0];
    let ids: Vec<&str> = cell.blocks.iter().map(|block| block.id.as_str()).collect();
    assert_eq!(
        ids,
        vec![anchor.as_str(), inserted.as_str()],
        "the new paragraph must sit immediately after its anchor, inside the cell"
    );
    // And nowhere else.
    assert_eq!(
        merged
            .document
            .visible_text()
            .matches("second line")
            .count(),
        1
    );
    base.blocks.clear();
}

#[test]
fn row_reorder_is_exact_and_a_partial_order_is_refused() {
    let table_id = StableId::parse("reorder-table").unwrap();
    let first = StableId::parse("reorder-first").unwrap();
    let second = StableId::parse("reorder-second").unwrap();
    let mut base = Document::new("Reorder");
    base.blocks.push(Block {
        id: table_id.clone(),
        kind: BlockKind::table(vec![table_row(&first, "a"), table_row(&second, "b")]),
        content: Vec::new(),
        properties: BlockProperties::default(),
    });
    let operation = |row_ids| Operation {
        id: OperationId {
            actor: ActorId("sorter".to_string()),
            seq: 1,
        },
        kind: OperationKind::ReorderTableRows {
            table_block_id: table_id.clone(),
            row_ids,
        },
        context: None,
    };
    let reordered = merge_operations(
        &base,
        &[vec![operation(vec![second.clone(), first.clone()])]],
    )
    .unwrap();
    let BlockKind::Table { rows, .. } = &reordered.document.blocks[0].kind else {
        panic!("expected table");
    };
    assert_eq!(
        rows.iter().map(|row| &row.id).collect::<Vec<_>>(),
        vec![&second, &first]
    );

    let refused = merge_operations(&base, &[vec![operation(vec![first])]]).unwrap();
    assert_eq!(refused.document.blocks, base.blocks);
    assert!(refused
        .warnings
        .iter()
        .any(|warning| warning.code == "invalid-table-row-order"));
}

#[test]
fn move_block_reorders_and_reparents_cell_blocks_without_changing_identity() {
    let first = Block::paragraph("first");
    let first_id = first.id.clone();
    let moved = Block::paragraph("moved");
    let moved_id = moved.id.clone();
    let destination = Block::paragraph("destination");
    let destination_id = destination.id.clone();
    let mut base = Document::new("Move cell block");
    base.blocks.push(Block {
        id: StableId::parse("move-table").unwrap(),
        kind: BlockKind::table(vec![TableRow {
            id: StableId::parse("move-row").unwrap(),
            height: None,
            header: false,
            cells: vec![
                TableCell::new(vec![first, moved]),
                TableCell::new(vec![destination]),
            ],
        }]),
        content: Vec::new(),
        properties: BlockProperties::default(),
    });
    let move_op = Operation {
        id: OperationId {
            actor: ActorId("editor".to_string()),
            seq: 1,
        },
        kind: OperationKind::MoveBlock {
            block_id: moved_id.clone(),
            position: InsertPosition::Before(destination_id.clone()),
        },
        context: None,
    };
    let moved_document = merge_operations(&base, &[vec![move_op.clone()]])
        .unwrap()
        .document;
    let BlockKind::Table { rows, .. } = &moved_document.blocks[0].kind else {
        panic!("expected table");
    };
    assert_eq!(
        rows[0].cells[0]
            .blocks
            .iter()
            .map(|block| &block.id)
            .collect::<Vec<_>>(),
        vec![&first_id]
    );
    assert_eq!(
        rows[0].cells[1]
            .blocks
            .iter()
            .map(|block| &block.id)
            .collect::<Vec<_>>(),
        vec![&moved_id, &destination_id]
    );

    // Its inverse restores the original cell-local sibling placement rather
    // than replacing the block with a fresh identity.
    let crate::Inversion::Operations(inverse) = crate::invert_operation(&base, &move_op.kind)
    else {
        panic!("move must be undoable");
    };
    let restored = merge_operations(
        &base,
        &[vec![
            move_op,
            Operation {
                id: OperationId {
                    actor: ActorId("editor".to_string()),
                    seq: 2,
                },
                kind: inverse.into_iter().next().unwrap(),
                context: None,
            },
        ]],
    )
    .unwrap();
    assert_eq!(restored.document, base);
}

#[test]
fn moving_a_nested_cell_block_and_editing_it_concurrently_keeps_both_effects() {
    let nested_remaining = Block::paragraph("nested remaining");
    let moved = Block::paragraph("moved");
    let moved_id = moved.id.clone();
    let moved_text_id = inline_id(&moved.content[0]).clone();
    let destination = Block::paragraph("outer destination");
    let destination_id = destination.id.clone();
    let nested_table = Block {
        id: StableId::parse("nested-table").unwrap(),
        kind: BlockKind::table(vec![TableRow {
            id: StableId::parse("nested-row").unwrap(),
            height: None,
            header: false,
            cells: vec![TableCell::new(vec![nested_remaining, moved])],
        }]),
        content: Vec::new(),
        properties: BlockProperties::default(),
    };
    let mut base = Document::new("Nested move with edit");
    base.blocks.push(Block {
        id: StableId::parse("outer-table").unwrap(),
        kind: BlockKind::table(vec![TableRow {
            id: StableId::parse("outer-row").unwrap(),
            height: None,
            header: false,
            cells: vec![TableCell::new(vec![nested_table, destination])],
        }]),
        content: Vec::new(),
        properties: BlockProperties::default(),
    });
    base.validate().unwrap();

    let operations = |move_actor: &str, edit_actor: &str| {
        vec![
            Operation {
                id: OperationId {
                    actor: ActorId(move_actor.to_string()),
                    seq: 1,
                },
                kind: OperationKind::MoveBlock {
                    block_id: moved_id.clone(),
                    position: InsertPosition::Before(destination_id.clone()),
                },
                context: None,
            },
            Operation {
                id: OperationId {
                    actor: ActorId(edit_actor.to_string()),
                    seq: 1,
                },
                kind: OperationKind::InsertInline {
                    block_id: moved_id.clone(),
                    position: InsertPosition::After(moved_text_id.clone()),
                    inline: Inline::text(" concurrently edited"),
                },
                context: None,
            },
        ]
    };

    // Exercise both semantic fold orders: one replica sees the edit before
    // the reparenting, the other sees reparenting before the edit. A move is
    // identity-preserving, so neither result may lose the inline anchor.
    for operations in [operations("a", "z"), operations("z", "a")] {
        let result = merge_operations(&base, &[operations]).unwrap();
        assert!(result.warnings.is_empty(), "{:?}", result.warnings);
        result.document.validate().unwrap();
        let BlockKind::Table { rows, .. } = &result.document.blocks[0].kind else {
            panic!("outer table");
        };
        let outer_blocks = &rows[0].cells[0].blocks;
        assert_eq!(outer_blocks.len(), 3);
        assert_eq!(outer_blocks[1].id, moved_id);
        assert_eq!(outer_blocks[2].id, destination_id);
        assert!(matches!(
            outer_blocks[1].content.as_slice(),
            [Inline::Text { text, .. }, Inline::Text { text: inserted, .. }]
                if text == "moved" && inserted == " concurrently edited"
        ));
        let BlockKind::Table {
            rows: nested_rows, ..
        } = &outer_blocks[0].kind
        else {
            panic!("nested table");
        };
        assert_eq!(nested_rows[0].cells[0].blocks.len(), 1);
        assert!(matches!(
            nested_rows[0].cells[0].blocks[0].content.as_slice(),
            [Inline::Text { text, .. }] if text == "nested remaining"
        ));
    }
}

#[test]
fn move_block_refuses_to_empty_a_cell_or_enter_its_own_subtree() {
    let only = Block::paragraph("only");
    let only_id = only.id.clone();
    let target = Block::paragraph("target");
    let target_id = target.id.clone();
    let mut base = Document::new("Move guards");
    base.blocks.push(Block {
        id: StableId::parse("guard-table").unwrap(),
        kind: BlockKind::table(vec![TableRow {
            id: StableId::parse("guard-row").unwrap(),
            height: None,
            header: false,
            cells: vec![TableCell::new(vec![only]), TableCell::new(vec![target])],
        }]),
        content: Vec::new(),
        properties: BlockProperties::default(),
    });
    let result = merge_operations(
        &base,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("editor".to_string()),
                seq: 1,
            },
            kind: OperationKind::MoveBlock {
                block_id: only_id,
                position: InsertPosition::Before(target_id),
            },
            context: None,
        }]],
    )
    .unwrap();
    assert_eq!(result.document.blocks, base.blocks);
    assert!(result
        .warnings
        .iter()
        .any(|warning| warning.code == "table-cell-requires-block"));
}
