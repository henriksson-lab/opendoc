use crate::*;

#[test]
fn citation_labels_render_from_document_local_database() {
    let mut doc = Document::new("Example");
    let reference_id = StableId::parse("ref-doe-2020").unwrap();
    let citation_id = StableId::parse("cite-intro").unwrap();
    doc.citation_database
        .upsert_reference(BibliographyReference {
            id: reference_id.clone(),
            revision: 1,
            source: CitationSource {
                format: CitationSourceFormat::CitumNative,
                bytes: b"id: doe-2020\ntitle: Example".to_vec(),
            },
            summary: CitationSummary {
                title: "Example".to_string(),
                authors: vec!["Doe".to_string()],
                issued: Some("2020".to_string()),
                doi: None,
                url: None,
            },
            deleted: false,
        });
    doc.citation_database.upsert_citation(CitationGroup {
        id: citation_id.clone(),
        revision: 1,
        items: vec![CitationItem {
            reference_id,
            locator: Some("42".to_string()),
            label: Some("page".to_string()),
            prefix: Some("see".to_string()),
            suffix: None,
            suppress_author: false,
        }],
        placement: CitationPlacement::Inline,
        rendered_cache: Some("(see Doe 2020, 42)".to_string()),
        deleted: false,
    });
    doc.blocks.push(Block {
        id: StableId::new("block"),
        kind: BlockKind::Paragraph,
        content: vec![Inline::Citation {
            id: StableId::new("citation-label"),
            citation_id,
            rendered_cache: None,
        }],
        properties: BlockProperties::default(),
    });

    assert_eq!(doc.visible_text(), "(see Doe 2020, 42)\n");
}

#[test]
fn citation_groups_require_items_but_not_live_bibliography_targets() {
    let mut doc = Document::new("Citations");
    let reference_id = StableId::parse("deleted-ref").unwrap();
    let citation_id = StableId::parse("cite-deleted-ref").unwrap();
    doc.citation_database
        .upsert_reference(BibliographyReference {
            id: reference_id.clone(),
            revision: 1,
            source: CitationSource {
                format: CitationSourceFormat::CitumNative,
                bytes: b"title: Retired".to_vec(),
            },
            summary: CitationSummary {
                title: "Retired".to_string(),
                authors: Vec::new(),
                issued: None,
                doi: None,
                url: None,
            },
            deleted: true,
        });
    doc.citation_database.upsert_citation(CitationGroup {
        id: citation_id.clone(),
        revision: 1,
        items: vec![CitationItem {
            reference_id,
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
    doc.blocks.push(Block {
        id: StableId::new("block"),
        kind: BlockKind::Paragraph,
        content: vec![Inline::Citation {
            id: StableId::new("citation-label"),
            citation_id,
            rendered_cache: None,
        }],
        properties: BlockProperties::default(),
    });
    doc.validate().unwrap();
    assert_eq!(doc.visible_text(), "[cite-deleted-ref]\n");

    doc.citation_database.citations[0].items.clear();
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument("citation group has no items"))
    ));
}

#[test]
fn citation_payloads_reject_empty_source_fields() {
    let reference_id = StableId::parse("ref-bad").unwrap();
    let mut doc = Document::new("Citation payloads");
    doc.citation_database
        .upsert_reference(BibliographyReference {
            id: reference_id.clone(),
            revision: 1,
            source: CitationSource {
                format: CitationSourceFormat::CitumNative,
                bytes: b"title: Bad source".to_vec(),
            },
            summary: CitationSummary {
                title: "Bad source".to_string(),
                authors: vec![" ".to_string()],
                issued: None,
                doi: None,
                url: None,
            },
            deleted: false,
        });
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "bibliography summary field is empty"
        ))
    ));

    doc.citation_database.references[0].summary.authors = Vec::new();
    doc.citation_database.references[0].summary.doi = Some(" ".to_string());
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "bibliography summary field is empty"
        ))
    ));

    doc.citation_database.references[0].summary.doi = Some(" 10.123/example ".to_string());
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "bibliography summary field has surrounding whitespace"
        ))
    ));

    doc.citation_database.references[0].summary.doi = None;
    doc.citation_database.references[0].summary.title = " Bad source ".to_string();
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "bibliography summary field has surrounding whitespace"
        ))
    ));

    doc.citation_database.references[0].summary.title = "Bad source".to_string();
    doc.citation_database.references[0].source.format =
        CitationSourceFormat::Unknown(String::new());
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "bibliography source format is empty"
        ))
    ));

    doc.citation_database.references[0].source.format =
        CitationSourceFormat::Unknown(" custom-format ".to_string());
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "bibliography source format has surrounding whitespace"
        ))
    ));

    doc.citation_database.references[0].source.format = CitationSourceFormat::CitumNative;
    doc.citation_database.references[0].source.bytes = b" \n\t ".to_vec();
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument("bibliography source is empty"))
    ));

    doc.citation_database.references[0].source.bytes = b"title: Bad source".to_vec();
    doc.citation_database.upsert_citation(CitationGroup {
        id: StableId::parse("cite-bad").unwrap(),
        revision: 1,
        items: vec![CitationItem {
            reference_id,
            locator: None,
            label: None,
            prefix: Some(" ".to_string()),
            suffix: None,
            suppress_author: false,
        }],
        placement: CitationPlacement::Inline,
        rendered_cache: None,
        deleted: false,
    });
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument("citation item field is empty"))
    ));

    doc.citation_database.references[0].summary.title = "Bad source".to_string();
    doc.citation_database.references[0].summary.authors = Vec::new();
    doc.citation_database.references[0].summary.issued = None;
    doc.citation_database.references[0].summary.doi = None;
    doc.citation_database.references[0].summary.url = None;
    doc.citation_database.citations[0].items[0].prefix = Some(" see ".to_string());
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "citation item field has surrounding whitespace"
        ))
    ));
}

#[test]
fn live_footnote_citations_require_live_footnote_targets() {
    let footnote_id = StableId::parse("fn-citation").unwrap();
    let reference_id = StableId::parse("ref-doe-2020").unwrap();
    let citation_id = StableId::parse("cite-footnote").unwrap();
    let mut doc = Document::new("Footnote citation");
    doc.citation_database
        .upsert_reference(BibliographyReference {
            id: reference_id.clone(),
            revision: 1,
            source: CitationSource {
                format: CitationSourceFormat::CitumNative,
                bytes: b"title: Footnote source".to_vec(),
            },
            summary: CitationSummary {
                title: "Footnote source".to_string(),
                authors: Vec::new(),
                issued: None,
                doi: None,
                url: None,
            },
            deleted: false,
        });
    doc.citation_database.upsert_citation(CitationGroup {
        id: citation_id,
        revision: 1,
        items: vec![CitationItem {
            reference_id,
            locator: None,
            label: None,
            prefix: None,
            suffix: None,
            suppress_author: false,
        }],
        placement: CitationPlacement::Footnote {
            footnote_id: footnote_id.clone(),
        },
        rendered_cache: None,
        deleted: false,
    });
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "footnote citation target is missing"
        ))
    ));

    doc.footnotes.push(Footnote {
        id: footnote_id,
        revision: 1,
        body: vec![Inline::text("citation footnote")],
        deleted: false,
    });
    doc.validate().unwrap();
}

// ---- Page setup (PLAN77 B7) ----------------------------------------
