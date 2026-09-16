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
fn sections_materialize_a_deterministic_root_and_require_atomic_body_boundaries() {
    let mut document = Document::new("Section source");
    let root_id = document.root_section_id();
    document.blocks = vec![
        Block::paragraph("before"),
        Block {
            id: StableId::parse("section-break").unwrap(),
            kind: BlockKind::SectionBreak {
                section_id: StableId::parse("section-two").unwrap(),
            },
            content: Vec::new(),
            properties: BlockProperties::default(),
        },
        Block::paragraph("after"),
    ];
    assert!(matches!(
        document.validate(),
        Err(ModelError::InvalidDocument(
            "section break requires materialized section source"
        ))
    ));

    // A materialized root has the same page context as the legacy projection,
    // but the later section must still be an actual source record.
    assert!(matches!(
        document.materialize_legacy_sections(),
        Err(ModelError::InvalidDocument(
            "section break refers to an unknown or root section"
        ))
    ));
    assert!(document.sections.is_empty());
    document.blocks.remove(1);
    assert!(document.materialize_legacy_sections().unwrap());
    assert_eq!(document.sections[&root_id].page_setup, document.page_setup);
    document.sections.insert(
        StableId::parse("section-two").unwrap(),
        Section {
            id: StableId::parse("section-two").unwrap(),
            page_setup: PageSetup::default(),
            header: Vec::new(),
            footer: Vec::new(),
            first_page_header: None,
            first_page_footer: None,
            even_page_header: None,
            even_page_footer: None,
        },
    );
    document.blocks.insert(
        1,
        Block {
            id: StableId::parse("section-break").unwrap(),
            kind: BlockKind::SectionBreak {
                section_id: StableId::parse("section-two").unwrap(),
            },
            content: Vec::new(),
            properties: BlockProperties::default(),
        },
    );
    document.validate().unwrap();
    assert!(!document.materialize_legacy_sections().unwrap());

    document.blocks[0].kind = BlockKind::SectionBreak {
        section_id: StableId::parse("section-two").unwrap(),
    };
    assert!(matches!(
        document.validate(),
        Err(ModelError::InvalidDocument(
            "section break cannot be first or last body block"
        ))
    ));
}

#[test]
fn section_breaks_are_not_generic_content_or_furniture() {
    let section_id = StableId::parse("section-two").unwrap();
    let section_break = Block {
        id: StableId::parse("section-break").unwrap(),
        kind: BlockKind::SectionBreak { section_id },
        content: vec![Inline::text("not allowed")],
        properties: BlockProperties::default(),
    };
    assert!(matches!(
        section_break.validate_isolated(),
        Err(ModelError::InvalidDocument(
            "section break is only allowed in document body flow"
        ))
    ));
}

#[test]
fn section_furniture_participates_in_durable_text_source() {
    let mut document = Document::new("Section furniture tokens");
    document.blocks.push(Block::paragraph("body"));
    document.materialize_legacy_sections().unwrap();
    let root_id = document.root_section_id();
    let header = Block {
        id: StableId::parse("section-header").unwrap(),
        kind: BlockKind::Paragraph,
        content: vec![Inline::Text {
            id: StableId::parse("section-header-text").unwrap(),
            text: "header".to_string(),
            marks: Vec::new(),
        }],
        properties: BlockProperties::default(),
    };
    document.sections.get_mut(&root_id).unwrap().header = vec![header];
    document.materialize_legacy_text_sequences().unwrap();
    assert!(document
        .text_sequences
        .contains_key(&StableId::parse("section-header-text").unwrap()));
    document.validate().unwrap();
}

#[test]
fn legacy_text_sequences_materialize_once_for_every_editable_run() {
    let mut document = Document::new("Token migration");
    document.blocks.push(Block {
        id: StableId::parse("paragraph").unwrap(),
        kind: BlockKind::Paragraph,
        content: vec![
            Inline::Text {
                id: StableId::parse("text-run").unwrap(),
                text: "a😀".to_string(),
                marks: Vec::new(),
            },
            Inline::Link {
                id: StableId::parse("link-run").unwrap(),
                text: "link".to_string(),
                href: "https://example.test".to_string(),
                marks: Vec::new(),
            },
        ],
        properties: BlockProperties::default(),
    });
    document.validate().unwrap();
    assert!(
        document.text_sequences.is_empty(),
        "old source stays legacy"
    );

    assert!(document.materialize_legacy_text_sequences().unwrap());
    assert_eq!(document.text_sequences.len(), 2);
    assert_eq!(
        document
            .text_sequences
            .get(&StableId::parse("text-run").unwrap())
            .unwrap()
            .visible_text(),
        "a😀"
    );
    document.validate().unwrap();
    assert!(!document.materialize_legacy_text_sequences().unwrap());
}

#[test]
fn persisted_text_sequences_must_cover_runs_and_match_their_projection() {
    let mut document = Document::new("Token validation");
    document.blocks.push(Block {
        id: StableId::parse("paragraph").unwrap(),
        kind: BlockKind::Paragraph,
        content: vec![Inline::Text {
            id: StableId::parse("text-run").unwrap(),
            text: "body".to_string(),
            marks: Vec::new(),
        }],
        properties: BlockProperties::default(),
    });
    document.materialize_legacy_text_sequences().unwrap();
    document.text_sequences.insert(
        StableId::parse("unknown-run").unwrap(),
        TextSequence::default(),
    );
    assert!(matches!(
        document.validate(),
        Err(ModelError::InvalidDocument(
            "text sequence map does not cover editable runs exactly"
        ))
    ));

    document
        .text_sequences
        .remove(&StableId::parse("unknown-run").unwrap());
    document
        .text_sequences
        .get_mut(&StableId::parse("text-run").unwrap())
        .unwrap()
        .tokens[0]
        .tombstoned = true;
    assert!(matches!(
        document.validate(),
        Err(ModelError::InvalidDocument(
            "text sequence visible text differs from inline text"
        ))
    ));
}

#[test]
fn baseline_tokens_cannot_be_reused_by_another_editable_run() {
    let mut document = Document::new("Token provenance");
    document.blocks.push(Block {
        id: StableId::parse("paragraph").unwrap(),
        kind: BlockKind::Paragraph,
        content: vec![Inline::Text {
            id: StableId::parse("text-run").unwrap(),
            text: "x".to_string(),
            marks: Vec::new(),
        }],
        properties: BlockProperties::default(),
    });
    document.materialize_legacy_text_sequences().unwrap();
    let token = &mut document
        .text_sequences
        .get_mut(&StableId::parse("text-run").unwrap())
        .unwrap()
        .tokens[0];
    if let TextTokenId::Baseline { inline_id, .. } = &mut token.id {
        *inline_id = StableId::parse("other-run").unwrap();
    } else {
        panic!("legacy migration must materialize a baseline token");
    }
    assert!(matches!(
        document.validate(),
        Err(ModelError::InvalidDocument(
            "baseline text token belongs to another editable run"
        ))
    ));
}

#[test]
fn token_comment_anchor_requires_its_persisted_ordered_sequence() {
    let run_id = StableId::parse("token-comment-run").unwrap();
    let mut document = Document::new("Token comment");
    document.blocks.push(Block {
        id: StableId::parse("token-comment-block").unwrap(),
        kind: BlockKind::Paragraph,
        content: vec![Inline::Text {
            id: run_id.clone(),
            text: "a😀b".to_string(),
            marks: Vec::new(),
        }],
        properties: BlockProperties::default(),
    });
    document.materialize_legacy_text_sequences().unwrap();
    let sequence = document.text_sequences.get(&run_id).unwrap();
    let anchor = Anchor::TokenRange(TextTokenRange {
        inline_id: run_id.clone(),
        start: sequence.gap_at_visible_offset(1, TextGapBias::Before),
        end: sequence.gap_at_visible_offset(2, TextGapBias::After),
    });
    document.comments.push(CommentThread {
        id: StableId::parse("token-comment-thread").unwrap(),
        anchor,
        comments: vec![Comment {
            id: StableId::parse("token-comment").unwrap(),
            author: "Ada".to_string(),
            body: vec![Inline::text("check this")],
            created_at_ms: 1,
            deleted: false,
        }],
        state: CommentThreadState::Open,
        resolved_by: None,
        resolved_at_ms: None,
        action_assignee: None,
        action_due_at_ms: None,
        action_completed_by: None,
        action_completed_at_ms: None,
        reactions: Vec::new(),
        deleted: false,
    });
    document.validate().unwrap();

    document.text_sequences.remove(&run_id);
    assert!(matches!(
        document.validate(),
        Err(ModelError::InvalidDocument(
            "token anchor requires a persisted text sequence"
        ))
    ));
}

#[test]
fn horizontal_rule_is_content_free_structural_block() {
    let mut doc = Document::new("Rules");
    doc.blocks.push(Block {
        id: StableId::new("block"),
        kind: BlockKind::HorizontalRule,
        content: Vec::new(),
        properties: BlockProperties::default(),
    });
    doc.validate().unwrap();
    doc.blocks[0].content.push(Inline::text("not allowed"));
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "horizontal rule cannot contain inline content"
        ))
    ));
}

#[test]
fn table_of_contents_is_content_free_and_has_a_bounded_heading_scope() {
    let mut doc = Document::new("Contents");
    doc.blocks.push(Block {
        id: StableId::new("block"),
        kind: BlockKind::TableOfContents { max_level: 3 },
        content: Vec::new(),
        properties: BlockProperties::default(),
    });
    doc.validate().unwrap();
    doc.blocks[0].content.push(Inline::text("stale entry"));
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "table of contents cannot contain inline content"
        ))
    ));
    doc.blocks[0].content.clear();
    doc.blocks[0].kind = BlockKind::TableOfContents { max_level: 7 };
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "table of contents max level is outside 1..=6"
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
            properties: Default::default(),
            rows: vec![TableRow {
                id: StableId(String::new()),
                height: None,
                header: false,
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
