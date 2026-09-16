//! Fixtures and assertions shared by the merge test modules.

use crate::causal::{ActorId, OperationId};
use crate::merge::{merge_operations, MergeResult};
use crate::operation::{Operation, OperationKind};
use opendoc_core::{
    Block, BlockKind, BlockProperties, CellSpan, Document, Inline, InsertPosition, MarkKind,
    StableId, TableCell, TableColumn, TableRow,
};
use std::collections::BTreeMap;

pub(crate) fn assert_mark_kinds(inline: &Inline, expected: &[MarkKind]) {
    let marks = match inline {
        Inline::Text { marks, .. } | Inline::Link { marks, .. } => marks,
        other => panic!("expected editable inline, got {other:?}"),
    };
    let actual = marks
        .iter()
        .map(|mark| mark.kind.clone())
        .collect::<Vec<_>>();
    assert_eq!(actual, expected);
}

// ---- Table structure (PLAN77 E2, ADR 0013) --------------------------

/// A document holding one table whose ids are stable across runs, so a
/// convergence assertion can compare whole documents.
pub(crate) fn grid_document(row_count: usize, column_count: usize) -> (Document, StableId) {
    let mut base = Document::new("Doc");
    let table_block_id = StableId::parse("table-block").unwrap();
    let columns: Vec<TableColumn> = (0..column_count)
        .map(|index| TableColumn {
            id: StableId::parse(format!("column-{index}")).unwrap(),
            width: None,
        })
        .collect();
    let rows: Vec<TableRow> = (0..row_count)
        .map(|row| TableRow {
            id: StableId::parse(format!("row-{row}")).unwrap(),
            height: None,
            header: false,
            cells: (0..column_count)
                .map(|column| TableCell {
                    id: StableId::parse(format!("cell-{row}-{column}")).unwrap(),
                    span: CellSpan::SINGLE,
                    properties: Default::default(),
                    blocks: vec![Block {
                        id: StableId::parse(format!("block-{row}-{column}")).unwrap(),
                        kind: BlockKind::Paragraph,
                        content: vec![Inline::Text {
                            id: StableId::parse(format!("text-{row}-{column}")).unwrap(),
                            text: format!("r{row}c{column}"),
                            marks: Vec::new(),
                        }],
                        properties: BlockProperties::default(),
                    }],
                })
                .collect(),
        })
        .collect();
    base.blocks.push(Block {
        id: table_block_id.clone(),
        kind: BlockKind::Table {
            columns,
            properties: Default::default(),
            rows,
        },
        content: Vec::new(),
        properties: BlockProperties::default(),
    });
    base.validate().expect("the fixture is a valid grid");
    (base, table_block_id)
}

pub(crate) fn table_of(document: &Document) -> (&[TableColumn], &[TableRow]) {
    match &document.blocks[0].kind {
        BlockKind::Table { columns, rows, .. } => (columns, rows),
        other => panic!("expected a table, got {other:?}"),
    }
}

/// Merge `a` and `b` as two concurrent streams and hand back the result for
/// the caller to make its real assertions about.
///
/// The two-stream-orders comparison this used to be named for is **not** the
/// check. `merge_operations` folds every stream into one
/// `BTreeMap<OperationId, Operation>` before a line of semantics runs, so its
/// answer is a function of the operation *set*: two groupings of one set agree
/// by construction, for any implementation, including one that drops every
/// operation and returns the base. PLAN88 §7. It is kept below because it
/// costs nothing and says what the merge is for, and the positive controls
/// beside it are what can actually fail: both operations have to land.
pub(crate) fn converges(base: &Document, a: Operation, b: Operation) -> MergeResult {
    let ab = merge_operations(base, &[vec![a.clone()], vec![b.clone()]]).unwrap();
    let ba = merge_operations(base, &[vec![b.clone()], vec![a.clone()]]).unwrap();
    assert_eq!(ab.document, ba.document, "merge is order dependent");

    // The positive controls. Each operation on its own must change the
    // document, or "they converge" is a statement about a merge that did
    // nothing; and the pair together must change it too.
    for (label, operation) in [("the first", a), ("the second", b)] {
        let alone = merge_operations(base, &[vec![operation]]).unwrap();
        assert_ne!(
            &alone.document, base,
            "{label} operation left the document untouched, so this case cannot tell \
             a merge from a no-op"
        );
    }
    assert_ne!(
        &ab.document, base,
        "neither operation reached the merged document"
    );

    ab.document
        .validate()
        .expect("merged table is a valid grid");
    ab
}

/// The cell -> column binding a replica writes when it generates a row
/// insert: cell *i* of the payload was typed into column *i* of the grid its
/// author could see. Cells past the end of that grid are left unbound, which
/// is the payload a malformed operation carries.
pub(crate) fn cell_columns(
    document: &Document,
    table_block_id: &StableId,
    row: &TableRow,
) -> BTreeMap<StableId, StableId> {
    let (columns, _) = table_in(document, table_block_id);
    row.cells
        .iter()
        .zip(columns.iter())
        .map(|(cell, column)| (cell.id.clone(), column.id.clone()))
        .collect()
}

fn table_in<'a>(
    document: &'a Document,
    table_block_id: &StableId,
) -> (&'a [TableColumn], &'a [TableRow]) {
    for block in &document.blocks {
        if let BlockKind::Table { columns, rows, .. } = &block.kind {
            if &block.id == table_block_id {
                return (columns, rows);
            }
        }
    }
    panic!("no table {table_block_id} in the fixture");
}

/// A row insert bound to the columns of `document` as it stands — what
/// `OpenDocApp::add_table_row` produces on a real replica.
pub(crate) fn insert_row_op(
    document: &Document,
    table_block_id: &StableId,
    actor: &str,
    seq: u64,
    position: InsertPosition,
    row: TableRow,
) -> Operation {
    Operation {
        id: OperationId {
            actor: ActorId(actor.to_string()),
            seq,
        },
        kind: OperationKind::InsertTableRow {
            table_block_id: table_block_id.clone(),
            position,
            cell_columns: cell_columns(document, table_block_id, &row),
            row,
        },
        context: None,
    }
}

pub(crate) fn table_row(id: &StableId, text: &str) -> TableRow {
    TableRow {
        id: id.clone(),
        height: None,
        header: false,
        cells: vec![table_cell(
            &StableId::parse(format!("cell-{id}")).unwrap(),
            text,
        )],
    }
}

pub(crate) fn table_cell(id: &StableId, text: &str) -> TableCell {
    TableCell {
        id: id.clone(),
        span: CellSpan::SINGLE,
        properties: Default::default(),
        blocks: vec![Block::paragraph(text)],
    }
}

pub(crate) fn property_op(actor: &str, seq: u64, kind: OperationKind) -> Operation {
    Operation {
        id: OperationId {
            actor: ActorId(actor.to_string()),
            seq,
        },
        kind,
        context: None,
    }
}

pub(crate) fn document_with_two_paragraphs() -> (Document, StableId, StableId) {
    let mut base = Document::new("Doc");
    let first = Block::paragraph("first");
    let second = Block::paragraph("second");
    let first_id = first.id.clone();
    let second_id = second.id.clone();
    base.blocks.push(first);
    base.blocks.push(second);
    (base, first_id, second_id)
}
