use crate::*;

#[test]
fn minimal_document_is_valid() {
    let mut doc = Document::new("Example");
    doc.blocks.push(Block::paragraph("hello"));
    doc.validate().unwrap();
    assert_eq!(doc.visible_text(), "hello\n");
    doc.title = " Example ".to_string();
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "title has surrounding whitespace"
        ))
    ));
    doc.title = "Example".to_string();
    doc.locale = " ".to_string();
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument("document locale is empty"))
    ));
    doc.locale = " en-US ".to_string();
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "document locale has surrounding whitespace"
        ))
    ));
    doc.locale = "en-US".to_string();
    doc.doi = Some(" ".to_string());
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument("document DOI is empty"))
    ));
    doc.doi = Some(" 10.123/example ".to_string());
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "document DOI has surrounding whitespace"
        ))
    ));
}

#[test]
fn warning_records_require_auditable_payloads() {
    let mut doc = Document::new("Warnings");
    doc.blocks.push(Block::paragraph("body"));
    doc.warnings.push(ModelWarning {
        code: "degraded-import".to_string(),
        message: "unsupported imported field was ignored".to_string(),
    });
    doc.validate().unwrap();

    doc.warnings[0].code = " ".to_string();
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument("warning code is empty"))
    ));

    doc.warnings[0].code = " degraded-import ".to_string();
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "warning code has surrounding whitespace"
        ))
    ));

    doc.warnings[0].code = "degraded-import".to_string();
    doc.warnings[0].message.clear();
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument("warning message is empty"))
    ));

    doc.warnings[0].message = " unsupported imported field was ignored ".to_string();
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "warning message has surrounding whitespace"
        ))
    ));
}

#[test]
fn document_structure_rejects_duplicate_operation_ids() {
    let duplicate_block_id = StableId::parse("block-duplicate").unwrap();
    let duplicate_inline_id = StableId::parse("text-duplicate").unwrap();
    let mut doc = Document::new("Duplicate IDs");
    doc.blocks.push(Block {
        id: duplicate_block_id.clone(),
        kind: BlockKind::Paragraph,
        content: vec![Inline::Text {
            id: StableId::parse("text-1").unwrap(),
            text: "first".to_string(),
            marks: Vec::new(),
        }],
        properties: BlockProperties::default(),
    });
    doc.blocks.push(Block {
        id: duplicate_block_id,
        kind: BlockKind::Paragraph,
        content: vec![Inline::Text {
            id: StableId::parse("text-2").unwrap(),
            text: "second".to_string(),
            marks: Vec::new(),
        }],
        properties: BlockProperties::default(),
    });
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument("duplicate block id"))
    ));

    doc.blocks[1].id = StableId::parse("block-2").unwrap();
    doc.blocks[0].content = vec![Inline::Text {
        id: duplicate_inline_id.clone(),
        text: "first".to_string(),
        marks: Vec::new(),
    }];
    doc.blocks[1].content = vec![Inline::Text {
        id: duplicate_inline_id,
        text: "second".to_string(),
        marks: Vec::new(),
    }];
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument("duplicate inline id"))
    ));
}

#[test]
fn source_model_rejects_empty_stable_ids_after_decode() {
    let mut doc = Document::new("Stable IDs");
    doc.uuid = DocumentUuid(String::new());
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument("document uuid is empty"))
    ));

    doc.uuid = DocumentUuid(" doc-stable ".to_string());
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "document uuid has surrounding whitespace"
        ))
    ));

    doc.uuid = DocumentUuid::parse("doc-stable").unwrap();
    doc.blocks.push(Block {
        id: StableId(String::new()),
        kind: BlockKind::Paragraph,
        content: vec![Inline::text("body")],
        properties: BlockProperties::default(),
    });
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument("block id is empty"))
    ));

    doc.blocks[0].id = StableId::parse("block-1").unwrap();
    doc.blocks[0].content = vec![Inline::Text {
        id: StableId(String::new()),
        text: "body".to_string(),
        marks: Vec::new(),
    }];
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument("inline id is empty"))
    ));

    doc.blocks[0].content = vec![Inline::Text {
        id: StableId(" text-1 ".to_string()),
        text: "body".to_string(),
        marks: Vec::new(),
    }];
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "stable id has surrounding whitespace"
        ))
    ));

    doc.blocks[0].content = vec![Inline::Citation {
        id: StableId::parse("citation-label").unwrap(),
        citation_id: StableId(String::new()),
        rendered_cache: None,
    }];
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument("citation id is empty"))
    ));

    doc.blocks[0].content = vec![Inline::Equation {
        id: StableId::parse("inline-equation").unwrap(),
        equation: Equation {
            id: StableId(String::new()),
            source_format: EquationSourceFormat::LatexLike,
            source: "x^2".to_string(),
        },
    }];
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument("equation id is empty"))
    ));

    doc.blocks[0] = Block {
        id: StableId::parse("table-1").unwrap(),
        kind: BlockKind::Table {
            columns: vec![TableColumn::auto()],
            rows: vec![TableRow {
                id: StableId(String::new()),
                cells: vec![TableCell {
                    id: StableId::parse("cell-1").unwrap(),
                    span: CellSpan::SINGLE,
                    properties: TableCellProperties::default(),
                    blocks: vec![Block::paragraph("cell")],
                }],
            }],
        },
        content: Vec::new(),
        properties: BlockProperties::default(),
    };
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument("table row id is empty"))
    ));

    doc.blocks.clear();
    doc.blocks.push(Block::paragraph("body"));
    doc.citation_database.style = " apa-7th ".to_string();
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "citation style has surrounding whitespace"
        ))
    ));

    doc.citation_database.style = "apa-7th".to_string();
    doc.citation_database.locale = " en-US ".to_string();
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "citation locale has surrounding whitespace"
        ))
    ));

    doc.citation_database.locale = "en-US".to_string();
    doc.citation_database
        .upsert_reference(BibliographyReference {
            id: StableId(String::new()),
            revision: 1,
            source: CitationSource {
                format: CitationSourceFormat::CitumNative,
                bytes: b"title: Source".to_vec(),
            },
            summary: CitationSummary {
                title: "Source".to_string(),
                authors: Vec::new(),
                issued: None,
                doi: None,
                url: None,
            },
            deleted: false,
        });
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "bibliography reference id is empty"
        ))
    ));

    doc.citation_database.references.clear();
    doc.citation_database.upsert_citation(CitationGroup {
        id: StableId::parse("citation-group").unwrap(),
        revision: 1,
        items: vec![CitationItem {
            reference_id: StableId(String::new()),
            locator: None,
            label: None,
            prefix: None,
            suffix: None,
            suppress_author: false,
        }],
        placement: CitationPlacement::Inline,
        rendered_cache: None,
        deleted: false,
    });
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "citation item reference id is empty"
        ))
    ));

    doc.citation_database.citations[0].items[0].reference_id =
        StableId::parse("ref-missing").unwrap();
    doc.citation_database.citations[0].placement = CitationPlacement::Footnote {
        footnote_id: StableId(String::new()),
    };
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument("footnote citation id is empty"))
    ));
}
