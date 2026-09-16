//! Table grid tests.

use crate::causal::{ActorId, OperationId};
use crate::merge::merge_operations;
use crate::operation::{Operation, OperationKind};
use crate::test_support::{converges, grid_document, insert_row_op, table_of, table_row};
use opendoc_core::{
    BlockKind, CellSpan, InsertPosition, Length, StableId, TableCell, TableCellProperty,
    TableCellPropertyKey, TableColumn,
};

#[test]
fn concurrent_table_column_inserts_converge_by_operation_id() {
    let (base, table_block_id) = grid_document(2, 2);
    let after = StableId::parse("column-0").unwrap();
    let insert = |actor: &str, id: &str| Operation {
        id: OperationId {
            actor: ActorId(actor.to_string()),
            seq: 1,
        },
        kind: OperationKind::InsertTableColumn {
            table_block_id: table_block_id.clone(),
            position: InsertPosition::After(after.clone()),
            column: TableColumn {
                id: StableId::parse(id).unwrap(),
                width: None,
            },
        },
        context: None,
    };

    let merged = converges(&base, insert("a", "column-a"), insert("b", "column-b"));
    let (columns, rows) = table_of(&merged.document);
    assert_eq!(
        columns.iter().map(|c| c.id.to_string()).collect::<Vec<_>>(),
        vec!["column-0", "column-b", "column-a", "column-1"]
    );
    for row in rows {
        assert_eq!(row.cells.len(), 4, "every row gained both columns");
    }
    assert!(merged.warnings.is_empty(), "{:?}", merged.warnings);
}

#[test]
fn a_row_and_a_column_inserted_concurrently_still_meet_in_a_cell() {
    // The case that has no right answer without derived identity: the new
    // row's payload knows nothing about the new column, and the new
    // column's payload knows nothing about the new row, yet the position
    // where they cross has to exist and has to be the *same* cell on
    // every replica.
    let (base, table_block_id) = grid_document(2, 2);
    let insert_row = insert_row_op(
        &base,
        &table_block_id,
        "a",
        1,
        InsertPosition::After(StableId::parse("row-0").unwrap()),
        table_row(&StableId::parse("row-new").unwrap(), "new row"),
    );
    let insert_column = Operation {
        id: OperationId {
            actor: ActorId("b".to_string()),
            seq: 1,
        },
        kind: OperationKind::InsertTableColumn {
            table_block_id,
            position: InsertPosition::After(StableId::parse("column-0").unwrap()),
            column: TableColumn {
                id: StableId::parse("column-new").unwrap(),
                width: None,
            },
        },
        context: None,
    };

    let merged = converges(&base, insert_row, insert_column);
    let (columns, rows) = table_of(&merged.document);
    assert_eq!(columns.len(), 3);
    assert_eq!(rows.len(), 3);
    for row in rows {
        assert_eq!(row.cells.len(), 3, "row {} is ragged", row.id);
    }
    assert!(
        merged.document.visible_text().contains("new row"),
        "the inserted row kept its content"
    );
    // The filled-in cell is the derived one, not a freshly minted id.
    let new_row = &rows[1];
    assert_eq!(
        new_row.cells[1].id,
        TableCell::filling(&new_row.id, &columns[1].id).id
    );
}

#[test]
fn a_column_deleted_under_a_concurrent_insert_still_leaves_a_rectangle() {
    let (base, table_block_id) = grid_document(2, 3);
    let delete = Operation {
        id: OperationId {
            actor: ActorId("a".to_string()),
            seq: 1,
        },
        kind: OperationKind::DeleteTableColumn {
            table_block_id: table_block_id.clone(),
            column_id: StableId::parse("column-1").unwrap(),
        },
        context: None,
    };
    let insert = Operation {
        id: OperationId {
            actor: ActorId("b".to_string()),
            seq: 1,
        },
        kind: OperationKind::InsertTableColumn {
            table_block_id,
            position: InsertPosition::After(StableId::parse("column-1").unwrap()),
            column: TableColumn {
                id: StableId::parse("column-new").unwrap(),
                width: None,
            },
        },
        context: None,
    };

    let merged = converges(&base, delete, insert);
    let (columns, rows) = table_of(&merged.document);
    assert_eq!(
        columns.iter().map(|c| c.id.to_string()).collect::<Vec<_>>(),
        // The anchor was deleted first, so — exactly as for a row whose
        // anchor is gone — the column lands at the end rather than
        // guessing a position.
        vec!["column-0", "column-2", "column-new"]
    );
    for row in rows {
        assert_eq!(row.cells.len(), 3);
    }
    assert!(merged
        .warnings
        .iter()
        .any(|warning| warning.code == "table-column-anchor-degraded"));
    let text = merged.document.visible_text();
    assert!(!text.contains("r0c1"), "the deleted column is gone: {text}");
    assert!(text.contains("r0c2"), "the surviving column is intact");
}

#[test]
fn deleting_the_last_column_leaves_a_placeholder_every_replica_agrees_on() {
    let (base, table_block_id) = grid_document(2, 1);
    let delete = Operation {
        id: OperationId {
            actor: ActorId("a".to_string()),
            seq: 1,
        },
        kind: OperationKind::DeleteTableColumn {
            table_block_id: table_block_id.clone(),
            column_id: StableId::parse("column-0").unwrap(),
        },
        context: None,
    };
    let first = merge_operations(&base, &[vec![delete.clone()]]).unwrap();
    let second = merge_operations(&base, &[vec![delete]]).unwrap();
    assert_eq!(first.document, second.document);
    let (columns, rows) = table_of(&first.document);
    assert_eq!(columns.len(), 1);
    assert_eq!(columns[0].id, TableColumn::filling(&table_block_id).id);
    assert_eq!(rows.len(), 2);
    assert!(first
        .warnings
        .iter()
        .any(|warning| warning.code == "table-column-delete-degraded"));
}

#[test]
fn merging_cells_hides_the_covered_content_and_splitting_hands_it_back() {
    let (base, _) = grid_document(2, 2);
    let merge_op = |span: CellSpan, seq: u64| Operation {
        id: OperationId {
            actor: ActorId("a".to_string()),
            seq,
        },
        kind: OperationKind::SetTableCellSpan {
            cell_id: StableId::parse("cell-0-0").unwrap(),
            span,
        },
        context: None,
    };

    let merged =
        merge_operations(&base, &[vec![merge_op(CellSpan::new(2, 2).unwrap(), 1)]]).unwrap();
    let text = merged.document.visible_text();
    assert!(text.contains("r0c0"));
    for hidden in ["r0c1", "r1c0", "r1c1"] {
        assert!(!text.contains(hidden), "{hidden} should be covered: {text}");
    }

    let split = merge_operations(&merged.document, &[vec![merge_op(CellSpan::SINGLE, 2)]]).unwrap();
    let text = split.document.visible_text();
    for restored in ["r0c0", "r0c1", "r1c0", "r1c1"] {
        assert!(text.contains(restored), "{restored} should be back: {text}");
    }
}

#[test]
fn concurrent_overlapping_merges_are_split_back_deterministically() {
    let (base, _) = grid_document(3, 3);
    let merge_at = |actor: &str, cell: &str| Operation {
        id: OperationId {
            actor: ActorId(actor.to_string()),
            seq: 1,
        },
        kind: OperationKind::SetTableCellSpan {
            cell_id: StableId::parse(cell).unwrap(),
            span: CellSpan::new(2, 2).unwrap(),
        },
        context: None,
    };

    // (0,0)-(1,1) and (1,1)-(2,2) overlap at (1,1): they cannot both win.
    let merged = converges(&base, merge_at("a", "cell-0-0"), merge_at("b", "cell-1-1"));
    let (_, rows) = table_of(&merged.document);
    let spans: Vec<(u32, u32)> = rows
        .iter()
        .flat_map(|row| {
            row.cells
                .iter()
                .map(|cell| (cell.span.rows(), cell.span.columns()))
        })
        .filter(|span| *span != (1, 1))
        .collect();
    assert_eq!(spans, vec![(2, 2)], "exactly one merge survived");
    assert_eq!(rows[0].cells[0].span, CellSpan::new(2, 2).unwrap());
    assert!(merged
        .warnings
        .iter()
        .any(|warning| warning.code == "table-geometry-repaired"));
}

#[test]
fn a_column_inserted_through_a_merge_widens_it() {
    let (mut base, table_block_id) = grid_document(2, 3);
    if let BlockKind::Table { rows, .. } = &mut base.blocks[0].kind {
        rows[0].cells[0].span = CellSpan::new(1, 2).unwrap();
    }
    base.validate().unwrap();

    let merged = merge_operations(
        &base,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::InsertTableColumn {
                table_block_id,
                position: InsertPosition::After(StableId::parse("column-0").unwrap()),
                column: TableColumn {
                    id: StableId::parse("column-new").unwrap(),
                    width: None,
                },
            },
            context: None,
        }]],
    )
    .unwrap();
    let (_, rows) = table_of(&merged.document);
    assert_eq!(
        rows[0].cells[0].span,
        CellSpan::new(1, 3).unwrap(),
        "the merge still covers the columns it covered, plus the new one"
    );
}

#[test]
fn a_row_deleted_through_a_merge_narrows_it() {
    // The deleted row is *interior* to the span and the span does not
    // reach the bottom of the grid, so clamping the span to the shrunken
    // table would not be enough: without narrowing it deliberately, the
    // merge would swallow the row below it.
    let (mut base, table_block_id) = grid_document(4, 2);
    if let BlockKind::Table { rows, .. } = &mut base.blocks[0].kind {
        rows[0].cells[0].span = CellSpan::new(3, 1).unwrap();
    }
    base.validate().unwrap();
    assert!(
        !base.visible_text().contains("r1c0"),
        "row 1 starts covered"
    );
    assert!(base.visible_text().contains("r3c0"), "row 3 starts visible");

    let merged = merge_operations(
        &base,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::DeleteTableRow {
                table_block_id,
                row_id: StableId::parse("row-1").unwrap(),
            },
            context: None,
        }]],
    )
    .unwrap();
    let (_, rows) = table_of(&merged.document);
    assert_eq!(rows.len(), 3);
    assert_eq!(
        rows[0].cells[0].span,
        CellSpan::new(2, 1).unwrap(),
        "the merge lost the row that was deleted out from under it"
    );
    assert!(
        merged.document.visible_text().contains("r3c0"),
        "the row below the merge was swallowed by it"
    );
}

#[test]
fn a_column_deleted_through_a_merge_narrows_it() {
    let (mut base, table_block_id) = grid_document(2, 4);
    if let BlockKind::Table { rows, .. } = &mut base.blocks[0].kind {
        rows[0].cells[0].span = CellSpan::new(1, 3).unwrap();
    }
    base.validate().unwrap();
    assert!(
        base.visible_text().contains("r0c3"),
        "column 3 starts visible"
    );

    let merged = merge_operations(
        &base,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::DeleteTableColumn {
                table_block_id,
                column_id: StableId::parse("column-1").unwrap(),
            },
            context: None,
        }]],
    )
    .unwrap();
    let (columns, rows) = table_of(&merged.document);
    assert_eq!(columns.len(), 3);
    assert_eq!(
        rows[0].cells[0].span,
        CellSpan::new(1, 2).unwrap(),
        "the merge kept the column that was deleted out from under it"
    );
    assert!(
        merged.document.visible_text().contains("r0c3"),
        "the column beside the merge was swallowed by it"
    );
}

#[test]
fn concurrent_cell_style_edits_keep_both_properties() {
    // ADR 0006 applies unchanged to cell styling: two actors setting
    // *different* properties of one cell both keep their edit.
    let (base, _) = grid_document(1, 1);
    let cell_id = StableId::parse("cell-0-0").unwrap();
    let background = Operation {
        id: OperationId {
            actor: ActorId("a".to_string()),
            seq: 1,
        },
        kind: OperationKind::SetTableCellProperty {
            cell_id: cell_id.clone(),
            property: TableCellProperty::Background(opendoc_core::Color::parse("#ffee00").unwrap()),
        },
        context: None,
    };
    let alignment = Operation {
        id: OperationId {
            actor: ActorId("b".to_string()),
            seq: 1,
        },
        kind: OperationKind::SetTableCellProperty {
            cell_id: cell_id.clone(),
            property: TableCellProperty::VerticalAlignment(opendoc_core::VerticalAlignment::Bottom),
        },
        context: None,
    };
    let row_header = Operation {
        id: OperationId {
            actor: ActorId("c".to_string()),
            seq: 1,
        },
        kind: OperationKind::SetTableCellProperty {
            cell_id: cell_id.clone(),
            property: TableCellProperty::RowHeader(true),
        },
        context: None,
    };

    let merged = merge_operations(
        &base,
        &[vec![background], vec![alignment], vec![row_header]],
    )
    .expect("independent cell properties converge");
    let (_, rows) = table_of(&merged.document);
    let properties = &rows[0].cells[0].properties;
    assert_eq!(
        properties.background,
        Some(opendoc_core::Color::parse("#ffee00").unwrap())
    );
    assert_eq!(
        properties.vertical_alignment,
        Some(opendoc_core::VerticalAlignment::Bottom)
    );
    assert_eq!(properties.row_header, Some(true));

    let cleared = merge_operations(
        &merged.document,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 2,
            },
            kind: OperationKind::ClearTableCellProperty {
                cell_id,
                key: TableCellPropertyKey::Background,
            },
            context: None,
        }]],
    )
    .unwrap();
    let (_, rows) = table_of(&cleared.document);
    assert_eq!(rows[0].cells[0].properties.background, None);
    assert_eq!(
        rows[0].cells[0].properties.vertical_alignment,
        Some(opendoc_core::VerticalAlignment::Bottom)
    );
    assert_eq!(rows[0].cells[0].properties.row_header, Some(true));
}

#[test]
fn concurrent_width_edits_on_different_columns_both_survive() {
    let (base, table_block_id) = grid_document(1, 2);
    let resize = |actor: &str, column: &str, points: f64| Operation {
        id: OperationId {
            actor: ActorId(actor.to_string()),
            seq: 1,
        },
        kind: OperationKind::SetTableColumnWidth {
            table_block_id: table_block_id.clone(),
            column_id: StableId::parse(column).unwrap(),
            width: Some(Length::from_points(points).unwrap()),
        },
        context: None,
    };

    let merged = converges(
        &base,
        resize("a", "column-0", 90.0),
        resize("b", "column-1", 120.0),
    );
    let (columns, _) = table_of(&merged.document);
    assert_eq!(columns[0].width, Length::from_points(90.0).ok());
    assert_eq!(columns[1].width, Length::from_points(120.0).ok());

    // An out-of-range width is refused rather than written.
    let refused = merge_operations(
        &merged.document,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 2,
            },
            kind: OperationKind::SetTableColumnWidth {
                table_block_id,
                column_id: StableId::parse("column-0").unwrap(),
                width: Some(Length::from_twips(TableColumn::MIN_WIDTH_TWIPS - 1).unwrap()),
            },
            context: None,
        }]],
    )
    .unwrap();
    let (columns, _) = table_of(&refused.document);
    assert_eq!(columns[0].width, Length::from_points(90.0).ok());
    assert!(refused
        .warnings
        .iter()
        .any(|warning| warning.code == "invalid-table-column-width"));
}

#[test]
fn inserting_first_lands_before_every_sibling() {
    // The position `after: Option<StableId>` could not express: `None` there
    // already means *append*, so "before the first one" needed a value of its
    // own rather than a second optional field beside the anchor.
    let (base, table_block_id) = grid_document(2, 2);
    let insert_row = insert_row_op(
        &base,
        &table_block_id,
        "a",
        1,
        InsertPosition::First,
        table_row(&StableId::parse("row-new").unwrap(), "new row"),
    );
    let insert_column = Operation {
        id: OperationId {
            actor: ActorId("a".to_string()),
            seq: 2,
        },
        kind: OperationKind::InsertTableColumn {
            table_block_id,
            position: InsertPosition::First,
            column: TableColumn {
                id: StableId::parse("column-new").unwrap(),
                width: None,
            },
        },
        context: None,
    };
    let merged = merge_operations(&base, &[vec![insert_row, insert_column]]).unwrap();
    let (columns, rows) = table_of(&merged.document);
    assert_eq!(
        columns.iter().map(|c| c.id.to_string()).collect::<Vec<_>>(),
        vec!["column-new", "column-0", "column-1"]
    );
    assert_eq!(
        rows.iter().map(|r| r.id.to_string()).collect::<Vec<_>>(),
        vec!["row-new", "row-0", "row-1"]
    );
    for row in rows {
        assert_eq!(row.cells.len(), 3, "the new column reached every row");
    }
    // `First` names no sibling, so there is no anchor to have gone missing
    // and nothing to degrade. (The fixture row is short by a column, so the
    // geometry repair does speak up — that is the fixture, not the position.)
    assert!(
        !merged
            .warnings
            .iter()
            .any(|warning| warning.code.contains("anchor-degraded")),
        "{:?}",
        merged.warnings
    );
}
