use crate::*;
use opendoc_core::{
    Block, BlockKind, BlockProperties, Document, Equation, EquationSourceFormat, StableId,
    TableCell, TableRow,
};
use serde_json::{json, Value};

fn cell_text(cell: &TableCell) -> String {
    cell.blocks
        .iter()
        .flat_map(|block| block.content.iter())
        .filter_map(|inline| match inline {
            opendoc_core::Inline::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

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
fn discloses_native_google_table_review_maps_without_accepting_them() {
    let input = json!({
        "body": { "content": [{
            "table": {
                "suggestedInsertionIds": ["table-insert"],
                "suggestedDeletionIds": ["table-delete"],
                "suggestedTableStyleChanges": { "table-style": { "tableStyle": {} } },
                "tableRows": [{
                    "suggestedInsertionIds": ["row-insert"],
                    "suggestedDeletionIds": ["row-delete"],
                    "suggestedTableRowStyleChanges": { "row-style": { "tableRowStyle": { "tableHeader": true } } },
                    "tableCells": [{
                        "suggestedInsertionIds": ["cell-insert"],
                        "suggestedDeletionIds": ["cell-delete"],
                        "suggestedTableCellStyleChanges": { "cell-style": { "tableCellStyle": { "contentAlignment": "MIDDLE" } } },
                        "content": [{ "paragraph": { "elements": [{ "textRun": { "content": "accepted", "textStyle": {} } }] } }]
                    }]
                }]
            }
        }] }
    });

    let report = import_google_docs_json("reviewed table", input.to_string().as_bytes()).unwrap();
    let warnings = report
        .warnings
        .iter()
        .filter(|warning| warning.code == "google-dropped-table-suggestion")
        .map(|warning| warning.message.as_str())
        .collect::<Vec<_>>();
    assert_eq!(9, warnings.len(), "{warnings:?}");
    assert!(warnings.iter().any(|message| message.contains("at table")));
    assert!(warnings.iter().any(|message| message.contains("at row 1")));
    assert!(warnings
        .iter()
        .any(|message| message.contains("at row 1, cell 1")));
    let BlockKind::Table { rows, .. } = &report.document.blocks[0].kind else {
        panic!("expected table");
    };
    assert!(!rows[0].header, "a suggested header is not accepted");
    assert_eq!(
        None, rows[0].cells[0].properties.vertical_alignment,
        "a suggested cell style is not accepted"
    );
    assert_eq!("accepted", cell_text(&rows[0].cells[0]));
}

#[test]
fn ignores_empty_google_table_review_maps() {
    let input = json!({
        "body": { "content": [{
            "table": {
                "suggestedInsertionIds": [],
                "suggestedTableStyleChanges": {},
                "tableRows": [{
                    "suggestedDeletionIds": [],
                    "suggestedTableRowStyleChanges": {},
                    "tableCells": [{
                        "suggestedInsertionIds": [],
                        "suggestedTableCellStyleChanges": {},
                        "content": [{ "paragraph": { "elements": [] } }]
                    }]
                }]
            }
        }] }
    });
    let report =
        import_google_docs_json("empty reviewed table", input.to_string().as_bytes()).unwrap();
    assert!(report
        .warnings
        .iter()
        .all(|warning| warning.code != "google-dropped-table-suggestion"));
}

#[test]
fn imports_native_google_table_styles_without_an_opendoc_extension() {
    let input = json!({
        "body": { "content": [{
            "table": {
                "tableStyle": { "tableColumnProperties": [
                    { "widthType": "FIXED_WIDTH", "width": { "magnitude": 72, "unit": "PT" } }
                ] },
                "tableRows": [{
                    "tableRowStyle": { "minRowHeight": { "magnitude": 24, "unit": "PT" }, "tableHeader": true },
                    "tableCells": [{
                        "tableCellStyle": {
                            "backgroundColor": { "color": { "rgbColor": { "red": 1, "green": 0.5, "blue": 0 } } },
                            "borderTop": {
                                "width": { "magnitude": 1, "unit": "PT" },
                                "color": { "color": { "rgbColor": { "blue": 1 } } },
                                "dashStyle": "DOT"
                            },
                            "paddingLeft": { "magnitude": 6, "unit": "PT" },
                            "paddingBottom": { "magnitude": 3, "unit": "PT" },
                            "contentAlignment": "MIDDLE"
                        },
                        "content": [{ "paragraph": { "elements": [{ "textRun": { "content": "styled", "textStyle": {} } }] } }]
                    }]
                }]
            }
        }] }
    });
    let report = import_google_docs_json("Google styles", input.to_string().as_bytes()).unwrap();
    let BlockKind::Table { columns, rows, .. } = &report.document.blocks[0].kind else {
        panic!("expected table");
    };
    assert_eq!(Some(1_440), columns[0].width.map(|width| width.twips()));
    assert_eq!(Some(480), rows[0].height.map(|height| height.twips()));
    assert!(rows[0].header);
    let style = &rows[0].cells[0].properties;
    assert_eq!(
        Some("#ff8000".to_string()),
        style.background.map(|color| color.as_hex())
    );
    assert_eq!(
        Some(opendoc_core::VerticalAlignment::Middle),
        style.vertical_alignment
    );
    assert_eq!(
        Some(120),
        style.padding_start.map(|padding| padding.twips())
    );
    assert_eq!(
        Some(60),
        style.padding_bottom.map(|padding| padding.twips())
    );
    let border = style.border_top.expect("top border");
    assert_eq!(opendoc_core::BorderStyle::Dotted, border.style());
    assert_eq!(20, border.width().twips());
    assert_eq!("#0000ff", border.color().as_hex());
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    report
        .document
        .validate()
        .expect("native styles form a valid table");
    let bytes = export_google_docs_json(&report.document).unwrap();
    let exported: Value = serde_json::from_slice(&bytes).unwrap();
    let table = &exported["body"]["content"][0]["table"];
    assert_eq!(
        Some(72.0),
        table["tableStyle"]["tableColumnProperties"][0]["width"]["magnitude"].as_f64()
    );
    assert_eq!(
        Some(24.0),
        table["tableRows"][0]["tableRowStyle"]["minRowHeight"]["magnitude"].as_f64()
    );
    assert_eq!(
        Value::Bool(true),
        table["tableRows"][0]["tableRowStyle"]["tableHeader"]
    );
    assert_eq!(
        "MIDDLE",
        table["tableRows"][0]["tableCells"][0]["tableCellStyle"]["contentAlignment"]
    );
    assert_eq!(
        "DOT",
        table["tableRows"][0]["tableCells"][0]["tableCellStyle"]["borderTop"]["dashStyle"]
    );
}

#[test]
fn discloses_explicit_transparent_google_table_cell_backgrounds() {
    let input = json!({
        "body": { "content": [{ "table": {
            "tableRows": [{ "tableCells": [{
                // A present OptionalColor with no `color` is Google's
                // explicit transparent fill, distinct from an absent,
                // inherited TableCellStyle.backgroundColor.
                "tableCellStyle": { "backgroundColor": {} },
                "content": [{ "paragraph": { "elements": [{
                    "textRun": { "content": "clear fill", "textStyle": {} }
                }] } }]
            }] }]
        } }] }
    });

    let report = import_google_docs_json("transparent cell fill", input.to_string().as_bytes())
        .expect("transparent source is structurally valid");
    let BlockKind::Table { rows, .. } = &report.document.blocks[0].kind else {
        panic!("expected table");
    };
    assert_eq!(None, rows[0].cells[0].properties.background);
    assert!(report.warnings.iter().any(|warning| {
        warning.code == "google-unrepresentable-transparent-table-background"
            && warning.message.contains("explicit transparent background")
    }));

    let bytes = export_google_docs_json(&report.document).expect("export remains valid");
    let exported: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(
        exported["body"]["content"][0]["table"]["tableRows"][0]["tableCells"][0]
            .get("tableCellStyle")
            .is_none()
    );
}

#[test]
fn names_google_table_column_algorithms_that_cannot_survive_export() {
    let input = json!({
        "body": { "content": [{ "table": {
            "tableStyle": { "tableColumnProperties": [
                { "widthType": "EVENLY_DISTRIBUTED" },
                { "widthType": "FIT_TO_CONTENT" }
            ] },
            "tableRows": [{ "tableCells": [
                { "content": [{ "paragraph": { "elements": [{ "textRun": { "content": "A" } }] } }] },
                { "content": [{ "paragraph": { "elements": [{ "textRun": { "content": "B" } }] } }] }
            ] }]
        } }] }
    });

    let report = import_google_docs_json("Google widths", input.to_string().as_bytes()).unwrap();
    let BlockKind::Table { columns, .. } = &report.document.blocks[0].kind else {
        panic!("expected table");
    };
    assert!(columns.iter().all(|column| column.width.is_none()));
    assert_eq!(
        report
            .warnings
            .iter()
            .filter(|warning| warning.code == "google-dropped-table-style")
            .count(),
        2,
        "each unrepresentable native layout algorithm is disclosed"
    );
}

#[test]
fn imports_native_google_cell_spans_into_a_rectangular_grid() {
    let input = json!({
        "body": { "content": [{ "table": { "tableRows": [
            { "tableCells": [
                { "tableCellStyle": { "rowSpan": 2, "columnSpan": 2 }, "content": [{ "paragraph": { "elements": [{ "textRun": { "content": "wide and tall", "textStyle": {} } }] } }] },
                { "content": [{ "paragraph": { "elements": [{ "textRun": { "content": "C1", "textStyle": {} } }] } }] }
            ] },
            { "tableCells": [
                { "content": [{ "paragraph": { "elements": [{ "textRun": { "content": "C2", "textStyle": {} } }] } }] }
            ] }
        ] } }] }
    });
    let report = import_google_docs_json("Google spans", input.to_string().as_bytes()).unwrap();
    let BlockKind::Table { columns, rows, .. } = &report.document.blocks[0].kind else {
        panic!("expected table");
    };
    assert_eq!(3, columns.len());
    assert_eq!(
        (2, 2),
        (
            rows[0].cells[0].span.rows(),
            rows[0].cells[0].span.columns()
        )
    );
    assert_eq!("C1", cell_text(&rows[0].cells[2]));
    assert_eq!("C2", cell_text(&rows[1].cells[2]));
    assert!(rows[1].cells[0].span.is_single());
    report.document.validate().expect("a valid merged grid");
    let bytes = export_google_docs_json(&report.document).unwrap();
    let exported: Value = serde_json::from_slice(&bytes).unwrap();
    let style =
        &exported["body"]["content"][0]["table"]["tableRows"][0]["tableCells"][0]["tableCellStyle"];
    assert_eq!(2, style["rowSpan"]);
    assert_eq!(2, style["columnSpan"]);
    // The covered cells exist in OpenDoc but do not exist in Google's row
    // array. Exporting them would make a re-import see extra cells after the
    // merge and corrupt the grid.
    assert_eq!(
        2,
        exported["body"]["content"][0]["table"]["tableRows"][0]["tableCells"]
            .as_array()
            .unwrap()
            .len()
    );
    assert_eq!(
        1,
        exported["body"]["content"][0]["table"]["tableRows"][1]["tableCells"]
            .as_array()
            .unwrap()
            .len()
    );
    let round_trip = import_google_docs_json("Google spans", &bytes).unwrap();
    let BlockKind::Table { columns, rows, .. } = &round_trip.document.blocks[0].kind else {
        panic!("expected table");
    };
    assert_eq!(3, columns.len());
    assert_eq!(
        (2, 2),
        (
            rows[0].cells[0].span.rows(),
            rows[0].cells[0].span.columns()
        )
    );
    assert_eq!("C2", cell_text(&rows[1].cells[2]));
    round_trip
        .document
        .validate()
        .expect("a stable Google table round trip");
}

#[test]
fn header_rows_round_trip_through_native_google_row_style() {
    let mut document = Document::new("Header row");
    document.blocks.push(Block {
        id: StableId::new("block"),
        kind: BlockKind::table(vec![TableRow {
            id: StableId::parse("header-row").unwrap(),
            height: None,
            header: true,
            cells: vec![
                TableCell::new(vec![Block::paragraph("Heading")]),
                TableCell::new(vec![Block::paragraph("Not a row header")]),
            ],
        }]),
        content: Vec::new(),
        properties: BlockProperties::default(),
    });
    let BlockKind::Table { properties, .. } = &mut document.blocks[0].kind else {
        panic!("expected table");
    };
    properties.alignment = Some(opendoc_core::TableAlignment::Center);
    let BlockKind::Table { rows, .. } = &mut document.blocks[0].kind else {
        panic!("expected table");
    };
    rows[0].cells[0].properties.row_header = Some(true);
    rows[0].cells[1].properties.row_header = Some(false);

    let bytes = export_google_docs_json(&document).unwrap();
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        value["body"]["content"][0]["table"]["tableRows"][0]["tableRowStyle"]["tableHeader"],
        Value::Bool(true)
    );
    assert_eq!(
        value["body"]["content"][0]["table"]["tableRows"][0]["tableCells"][0]["opendocRowHeader"],
        Value::Bool(true)
    );
    assert_eq!(
        value["body"]["content"][0]["table"]["tableRows"][0]["tableCells"][1]["opendocRowHeader"],
        Value::Bool(false)
    );
    assert_eq!(
        value["body"]["content"][0]["table"]["opendocTableAlignment"],
        Value::String("center".to_string())
    );
    let imported = import_google_docs_json("Header row", &bytes).unwrap();
    let BlockKind::Table {
        rows, properties, ..
    } = &imported.document.blocks[0].kind
    else {
        panic!("expected table");
    };
    assert!(rows[0].header);
    assert_eq!(rows[0].cells[0].properties.row_header, Some(true));
    assert_eq!(rows[0].cells[1].properties.row_header, Some(false));
    assert_eq!(
        properties.alignment,
        Some(opendoc_core::TableAlignment::Center)
    );
}

#[test]
fn native_google_table_headers_win_over_legacy_extensions_and_disclose_unsplittable_rows() {
    let input = json!({
        "body": { "content": [{ "table": { "tableRows": [{
            "opendocHeader": false,
            "tableRowStyle": { "tableHeader": true, "preventOverflow": true },
            "tableCells": [{ "content": [{ "paragraph": { "elements": [
                { "textRun": { "content": "Heading", "textStyle": {} } }
            ] } }] }]
        }] } }] }
    });

    let report = import_google_docs_json("native header", input.to_string().as_bytes()).unwrap();
    let BlockKind::Table { rows, .. } = &report.document.blocks[0].kind else {
        panic!("expected table");
    };
    assert!(rows[0].header);
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.code == "google-conflicting-table-header"));
    assert!(report.warnings.iter().any(|warning| {
        warning.code == "google-dropped-table-style" && warning.message.contains("preventOverflow")
    }));

    let exported: Value =
        serde_json::from_slice(&export_google_docs_json(&report.document).unwrap()).unwrap();
    let row = &exported["body"]["content"][0]["table"]["tableRows"][0];
    assert_eq!(Value::Bool(true), row["tableRowStyle"]["tableHeader"]);
    assert!(row.get("opendocHeader").is_none());
}

#[test]
fn absent_google_row_header_extension_is_not_inferred_from_cell_position() {
    let input = json!({
        "body": { "content": [{ "table": { "tableRows": [{ "tableCells": [
            { "content": [{ "paragraph": { "elements": [{ "textRun": { "content": "Label" } }] } }] }
        ] }] } }] }
    });
    let bytes = serde_json::to_vec(&input).unwrap();
    let imported = import_google_docs_json("No inference", &bytes).unwrap();
    let BlockKind::Table { rows, .. } = &imported.document.blocks[0].kind else {
        panic!("expected table");
    };
    assert_eq!(rows[0].cells[0].properties.row_header, None);
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
fn imports_a_nested_google_docs_table_inside_a_table_cell() {
    let input = json!({
        "body": { "content": [{
            "table": {
                "tableRows": [{
                    "tableCells": [{
                        "content": [{ "paragraph": { "elements": [{ "textRun": { "content": "Before\n" } }] } }, {
                            "table": { "tableRows": [{ "tableCells": [{ "content": [{
                                "paragraph": { "elements": [{ "textRun": { "content": "Nested\n" } }] }
                            }] }] }] }
                        }, { "paragraph": { "elements": [{ "textRun": { "content": "After\n" } }] } }]
                    }]
                }]
            }
        }] }
    });
    let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();
    let BlockKind::Table { rows, .. } = &report.document.blocks[0].kind else {
        panic!("expected outer table");
    };
    let cell = &rows[0].cells[0];
    assert_eq!(cell_text(cell), "BeforeAfter");
    let BlockKind::Table {
        rows: nested_rows, ..
    } = &cell.blocks[1].kind
    else {
        panic!("expected nested table between the surrounding cell paragraphs")
    };
    assert_eq!(cell_text(&nested_rows[0].cells[0]), "Nested");
    report
        .document
        .validate()
        .expect("nested source tree validates");
}

#[test]
fn google_table_import_rejects_unbounded_recursive_nesting() {
    let mut table = json!({
        "table": { "tableRows": [{ "tableCells": [{
            "content": [{ "paragraph": { "elements": [{ "textRun": { "content": "leaf\n" } }] } }]
        }] }] }
    });
    // Wrap the valid table until the source exceeds the importer/model budget.
    for _ in 0..16 {
        table = json!({
            "table": { "tableRows": [{ "tableCells": [{ "content": [table] }] }] }
        });
    }
    let input = json!({ "body": { "content": [table] } });
    let error = import_google_docs_json("Google", input.to_string().as_bytes())
        .expect_err("a hostile recursive table source must be bounded");
    assert!(
        matches!(error, ImportError::InvalidInput(message) if message.contains("16-level limit"))
    );
}

#[test]
fn exports_google_docs_table_cells_with_opendoc_block_extensions() {
    let mut document = Document::new("Nested Export");
    document.blocks.push(Block {
        id: StableId::new("block"),
        kind: BlockKind::table(vec![TableRow {
            id: StableId::parse("row-export").unwrap(),
            height: None,
            header: false,
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
fn exports_a_nested_table_cell_without_dropping_or_misrepresenting_it() {
    let mut document = Document::new("Nested Table Export");
    document.blocks.push(Block {
        id: StableId::new("block"),
        kind: BlockKind::table(vec![TableRow {
            id: StableId::parse("outer-row").unwrap(),
            height: None,
            header: false,
            cells: vec![TableCell {
                id: StableId::parse("outer-cell").unwrap(),
                span: Default::default(),
                properties: Default::default(),
                blocks: vec![Block {
                    id: StableId::parse("nested-table").unwrap(),
                    kind: BlockKind::table(vec![TableRow {
                        id: StableId::parse("inner-row").unwrap(),
                        height: None,
                        header: false,
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

    let bytes = export_google_docs_json(&document).unwrap();
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(
        value["body"]["content"][0]["table"]["tableRows"][0]["tableCells"][0]["content"][0]
            ["table"]
            .is_object()
    );
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
        height: None,
        header: false,
        cells: Vec::new(),
    }]);
    assert!(matches!(
        export_google_docs_json(&document),
        Err(ImportError::UnsupportedStructure(message))
            if message == "OpenDoc table row has no cells"
    ));

    document.blocks[0].kind = BlockKind::table(vec![TableRow {
        id: StableId::new("row"),
        height: None,
        header: false,
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
