use crate::*;
use opendoc_core::{
    Block, BlockKind, BlockProperties, Document, Equation, EquationSourceFormat, StableId,
    TableCell, TableRow,
};
use serde_json::{json, Value};

#[test]
fn imports_google_docs_table_cells_as_nested_blocks() {
    let input = json!({
        "body": { "content": [{
            "table": {
                "tableRows": [{
                    "tableCells": [
                        { "content": [{ "paragraph": { "elements": [{ "textRun": { "content": "A1", "textStyle": {} } }] } }] },
                        { "content": [{ "paragraph": { "elements": [{ "textRun": { "content": "B1", "textStyle": {} } }] } }] }
                    ]
                }]
            }
        }] }
    });
    let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();
    match &report.document.blocks[0].kind {
        BlockKind::Table { rows, .. } => {
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].cells.len(), 2);
            assert_eq!(report.document.visible_text(), "A1\tB1\n");
        }
        other => panic!("expected table, got {other:?}"),
    }
}

#[test]
fn imports_empty_google_docs_table_shapes_as_editable_placeholders() {
    let input = json!({
        "body": { "content": [
            { "table": { "tableRows": [] } },
            { "table": { "tableRows": [{ "tableCells": [] }] } },
            { "table": { "tableRows": [{ "tableCells": [{ "content": [] }] }] } }
        ] }
    });
    let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();

    assert_eq!(report.document.blocks.len(), 3);
    for block in &report.document.blocks {
        let BlockKind::Table { rows, .. } = &block.kind else {
            panic!("expected table block");
        };
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].cells.len(), 1);
        assert_eq!(rows[0].cells[0].blocks.len(), 1);
    }
    assert!(report.document.validate().is_ok());
    let warning_codes = report
        .warnings
        .iter()
        .map(|warning| warning.code.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        warning_codes,
        vec![
            "google-empty-table-normalized",
            "google-empty-table-row-normalized",
            "google-empty-table-cell-normalized"
        ]
    );
}

#[test]
fn imports_google_docs_table_cells_with_opendoc_block_extensions() {
    let input = json!({
        "body": { "content": [{
            "table": {
                "tableRows": [{
                    "tableCells": [{
                        "content": [
                            { "paragraph": { "elements": [
                                { "textRun": { "content": "Cell text", "textStyle": {} } }
                            ] } },
                            { "opendocEquationBlock": {
                                "blockId": "cell-equation-block",
                                "equationId": "cell-equation",
                                "sourceFormat": "latex-like",
                                "source": "x+y"
                            } },
                            { "opendocImage": {
                                "blockId": "cell-image",
                                "blobHash": "sha256:cellimage",
                                "altText": "Cell image"
                            } },
                            { "paragraph": { "elements": [
                                { "pageBreak": {} },
                                { "textRun": { "content": "\n" } }
                            ] } }
                        ]
                    }]
                }]
            }
        }] }
    });
    let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();
    match &report.document.blocks[0].kind {
        BlockKind::Table { rows, .. } => {
            let blocks = &rows[0].cells[0].blocks;
            assert!(matches!(blocks[0].kind, BlockKind::Paragraph));
            assert!(matches!(blocks[1].kind, BlockKind::EquationBlock { .. }));
            assert!(matches!(blocks[2].kind, BlockKind::Image { .. }));
            assert!(matches!(blocks[3].kind, BlockKind::PageBreak));
        }
        other => panic!("expected table, got {other:?}"),
    }
}

#[test]
fn nested_google_docs_table_in_table_cell_aborts() {
    let input = json!({
        "body": { "content": [{
            "table": {
                "tableRows": [{
                    "tableCells": [{
                        "content": [{
                            "table": { "tableRows": [] }
                        }]
                    }]
                }]
            }
        }] }
    });
    assert!(matches!(
        import_google_docs_json("Google", input.to_string().as_bytes()),
        Err(ImportError::UnsupportedStructure(_))
    ));
}

#[test]
fn exports_google_docs_table_cells_with_opendoc_block_extensions() {
    let mut document = Document::new("Nested Export");
    document.blocks.push(Block {
        id: StableId::new("block"),
        kind: BlockKind::table(vec![TableRow {
            id: StableId::parse("row-export").unwrap(),
            cells: vec![TableCell {
                id: StableId::parse("cell-export").unwrap(),
                span: Default::default(),
                properties: Default::default(),
                blocks: vec![
                    Block::paragraph("Cell text"),
                    Block {
                        id: StableId::parse("cell-equation-block").unwrap(),
                        kind: BlockKind::EquationBlock {
                            equation: Equation {
                                id: StableId::parse("cell-equation").unwrap(),
                                source_format: EquationSourceFormat::LatexLike,
                                source: "x+y".to_string(),
                            },
                        },
                        content: Vec::new(),
                        properties: BlockProperties::default(),
                    },
                    Block {
                        id: StableId::parse("cell-image").unwrap(),
                        kind: BlockKind::Image {
                            blob_hash: "sha256:cellimage".to_string(),
                            alt_text: "Cell image".to_string(),
                            layout: Default::default(),
                        },
                        content: Vec::new(),
                        properties: BlockProperties::default(),
                    },
                ],
            }],
        }]),
        content: Vec::new(),
        properties: BlockProperties::default(),
    });
    let bytes = export_google_docs_json(&document).unwrap();
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    let content = &value["body"]["content"][0]["table"]["tableRows"][0]["tableCells"][0]["content"];
    assert_eq!(
        content[0]["paragraph"]["elements"][0]["textRun"]["content"],
        "Cell text"
    );
    assert_eq!(
        content[1]["opendocEquationBlock"]["blockId"],
        "cell-equation-block"
    );
    assert_eq!(content[2]["opendocImage"]["blockId"], "cell-image");
}

#[test]
fn nested_table_cell_export_aborts_instead_of_dropping_or_misrepresenting() {
    let mut document = Document::new("Nested Table Export");
    document.blocks.push(Block {
        id: StableId::new("block"),
        kind: BlockKind::table(vec![TableRow {
            id: StableId::parse("outer-row").unwrap(),
            cells: vec![TableCell {
                id: StableId::parse("outer-cell").unwrap(),
                span: Default::default(),
                properties: Default::default(),
                blocks: vec![Block {
                    id: StableId::parse("nested-table").unwrap(),
                    kind: BlockKind::table(vec![TableRow {
                        id: StableId::parse("inner-row").unwrap(),
                        cells: vec![TableCell {
                            id: StableId::parse("inner-cell").unwrap(),
                            span: Default::default(),
                            properties: Default::default(),
                            blocks: vec![Block::paragraph("Nested")],
                        }],
                    }]),
                    content: Vec::new(),
                    properties: BlockProperties::default(),
                }],
            }],
        }]),
        content: Vec::new(),
        properties: BlockProperties::default(),
    });

    assert!(matches!(
        export_google_docs_json(&document),
        Err(ImportError::UnsupportedStructure(_))
    ));
}

#[test]
fn malformed_empty_table_export_aborts_instead_of_emitting_invalid_google_shape() {
    let mut document = Document::new("Empty Table Export");
    document.blocks.push(Block {
        id: StableId::new("table"),
        kind: BlockKind::table(Vec::new()),
        content: Vec::new(),
        properties: BlockProperties::default(),
    });
    assert!(matches!(
        export_google_docs_json(&document),
        Err(ImportError::UnsupportedStructure(message))
            if message == "OpenDoc table has no rows"
    ));

    document.blocks[0].kind = BlockKind::table(vec![TableRow {
        id: StableId::new("row"),
        cells: Vec::new(),
    }]);
    assert!(matches!(
        export_google_docs_json(&document),
        Err(ImportError::UnsupportedStructure(message))
            if message == "OpenDoc table row has no cells"
    ));

    document.blocks[0].kind = BlockKind::table(vec![TableRow {
        id: StableId::new("row"),
        cells: vec![TableCell {
            id: StableId::new("cell"),
            span: Default::default(),
            properties: Default::default(),
            blocks: Vec::new(),
        }],
    }]);
    assert!(matches!(
        export_google_docs_json(&document),
        Err(ImportError::UnsupportedStructure(message))
            if message == "OpenDoc table cell has no blocks"
    ));
}
