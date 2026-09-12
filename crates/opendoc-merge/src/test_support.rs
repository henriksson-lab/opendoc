//! Fixtures and assertions shared by the merge test modules.

use crate::causal::{ActorId, OperationId};
use crate::merge::{merge_operations, MergeResult};
use crate::operation::{Operation, OperationKind};
use opendoc_core::{
    Block, BlockKind, BlockProperties, CellSpan, Document, Inline, MarkKind, StableId, TableCell,
    TableColumn, TableRow,
};

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
        kind: BlockKind::Table { columns, rows },
        content: Vec::new(),
        properties: BlockProperties::default(),
    });
    base.validate().expect("the fixture is a valid grid");
    (base, table_block_id)
}

pub(crate) fn table_of(document: &Document) -> (&[TableColumn], &[TableRow]) {
    match &document.blocks[0].kind {
        BlockKind::Table { columns, rows } => (columns, rows),
        other => panic!("expected a table, got {other:?}"),
    }
}

/// Merges the same operation set in both stream orders and asserts the
/// two results are byte-identical and valid — the property every case
/// below is really testing.
pub(crate) fn converges(base: &Document, a: Operation, b: Operation) -> MergeResult {
    let ab = merge_operations(base, &[vec![a.clone()], vec![b.clone()]]).unwrap();
    let ba = merge_operations(base, &[vec![b], vec![a]]).unwrap();
    assert_eq!(ab.document, ba.document, "merge is order dependent");
    ab.document
        .validate()
        .expect("merged table is a valid grid");
    ab
}

pub(crate) fn table_row(id: &StableId, text: &str) -> TableRow {
    TableRow {
        id: id.clone(),
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
