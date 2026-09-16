use crate::*;

pub(crate) fn document_with(paragraphs: &[&str]) -> Document {
    let mut document = Document::new("render");
    document.blocks.clear();
    for text in paragraphs {
        document.blocks.push(Block::paragraph(*text));
    }
    document
}

/// A 2x3 grid whose cells say where they are.
pub(crate) fn table_document() -> Document {
    let mut document = Document::new("render");
    document.blocks.clear();
    document.blocks.push(Block {
        id: StableId::parse("table-1").expect("valid id"),
        kind: BlockKind::table(
            (0..2)
                .map(|row| opendoc_core::TableRow {
                    id: StableId::parse(format!("row-{row}")).expect("valid id"),
                    height: None,
                    header: false,
                    cells: (0..3)
                        .map(|column| {
                            opendoc_core::TableCell::new(vec![Block::paragraph(format!(
                                "r{row}c{column}"
                            ))])
                        })
                        .collect(),
                })
                .collect(),
        ),
        content: Vec::new(),
        properties: opendoc_core::BlockProperties::default(),
    });
    document
}

pub(crate) fn table_parts(
    document: &mut Document,
) -> (
    &mut Vec<opendoc_core::TableColumn>,
    &mut Vec<opendoc_core::TableRow>,
) {
    match &mut document.blocks[0].kind {
        BlockKind::Table { columns, rows, .. } => (columns, rows),
        other => panic!("expected a table, got {other:?}"),
    }
}

pub(crate) fn latex_equation(id: &str, source: &str) -> opendoc_core::Equation {
    opendoc_core::Equation {
        id: StableId::parse(id).expect("valid id"),
        source_format: opendoc_core::EquationSourceFormat::LatexLike,
        source: source.to_string(),
    }
}

pub(crate) fn document_with_equation_block(source: &str) -> Document {
    let mut document = document_with(&["before"]);
    // Built from the paragraph constructor so this test does not have to
    // track every field other phases add to `Block`.
    let mut block = Block::paragraph("");
    block.id = StableId::parse("block-equation").expect("valid id");
    block.kind = BlockKind::EquationBlock {
        equation: latex_equation("equation-block-1", source),
    };
    block.content = Vec::new();
    document.blocks.push(block);
    document
}

pub(crate) fn document_with_inline_equation(source: &str) -> Document {
    let mut document = document_with(&["before "]);
    document.blocks[0].content.push(Inline::Equation {
        id: StableId::parse("inline-equation-1").expect("valid id"),
        equation: latex_equation("equation-inline-1", source),
    });
    document
}
