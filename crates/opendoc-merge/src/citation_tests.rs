//! Citation tests.

use crate::causal::{ActorId, OperationId};
use crate::merge::merge_operations;
use crate::operation::{Operation, OperationKind};
use opendoc_core::{
    BibliographyReference, Block, BlockKind, BlockProperties, CellSpan, CitationGroup,
    CitationItem, CitationPlacement, CitationSource, CitationSourceFormat, CitationSummary,
    Document, Footnote, Inline, StableId, TableCell, TableRow,
};

#[test]
fn citation_labels_reference_document_local_database() {
    let mut base = Document::new("Doc");
    let block = Block::paragraph("cited ");
    let block_id = block.id.clone();
    let after = match &block.content[0] {
        Inline::Text { id, .. } => id.clone(),
        _ => unreachable!(),
    };
    base.blocks.push(block);
    let reference_id = StableId::parse("ref-doe-2020").unwrap();
    let citation_id = StableId::parse("cite-intro").unwrap();

    let reference = BibliographyReference {
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
    };
    let citation = CitationGroup {
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
    };
    let label = Inline::Citation {
        id: StableId::new("citation-label"),
        citation_id,
        rendered_cache: None,
    };
    let ops = vec![
        Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::UpsertBibliographyReference { reference },
            context: None,
        },
        Operation {
            id: OperationId {
                actor: ActorId("b".to_string()),
                seq: 1,
            },
            kind: OperationKind::UpsertCitationGroup { citation },
            context: None,
        },
        Operation {
            id: OperationId {
                actor: ActorId("c".to_string()),
                seq: 1,
            },
            kind: OperationKind::InsertInline {
                block_id,
                after: Some(after),
                inline: label,
            },
            context: None,
        },
    ];
    let result = merge_operations(&base, &[ops]).unwrap();
    assert_eq!(
        result.document.visible_text(),
        "cited (see Doe 2020, page 42)\n"
    );
    assert_eq!(result.document.citation_database.references.len(), 1);
    assert_eq!(result.document.citation_database.citations.len(), 1);
}

#[test]
fn citation_style_updates_replay_as_source_state() {
    let base = Document::new("Doc");
    let result = merge_operations(
        &base,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::UpdateCitationStyle {
                style: " ieee ".to_string(),
                locale: " en-GB ".to_string(),
            },
            context: None,
        }]],
    )
    .unwrap();
    assert_eq!(result.document.citation_database.style, "ieee");
    assert_eq!(result.document.citation_database.locale, "en-GB");

    let mut styled_base = Document::new("Doc");
    styled_base.citation_database.style = "ieee".to_string();
    styled_base.citation_database.locale = "en-GB".to_string();

    let degraded = merge_operations(
        &styled_base,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 2,
            },
            kind: OperationKind::UpdateCitationStyle {
                style: " ".to_string(),
                locale: " ".to_string(),
            },
            context: None,
        }]],
    )
    .unwrap();
    assert_eq!(degraded.document.citation_database.style, "ieee");
    assert_eq!(degraded.document.citation_database.locale, "en-GB");
    assert!(degraded
        .warnings
        .iter()
        .any(|warning| warning.code == "invalid-citation-style"));
}

#[test]
fn footnote_body_updates_are_revision_ordered_source_state() {
    let mut base = Document::new("Doc");
    let footnote_id = StableId::parse("footnote-1").unwrap();
    base.blocks.push(Block {
        id: StableId::new("block"),
        kind: BlockKind::Paragraph,
        content: vec![Inline::FootnoteRef {
            id: StableId::parse("footnote-ref-1").unwrap(),
            footnote_id: footnote_id.clone(),
        }],
        properties: BlockProperties::default(),
    });
    let old = Footnote {
        id: footnote_id.clone(),
        revision: 1,
        body: vec![Inline::text("old footnote")],
        deleted: false,
    };
    let new = Footnote {
        id: footnote_id,
        revision: 2,
        body: vec![Inline::text("new footnote")],
        deleted: false,
    };
    let result = merge_operations(
        &base,
        &[
            vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 2,
                },
                kind: OperationKind::UpsertFootnote { footnote: new },
                context: None,
            }],
            vec![Operation {
                id: OperationId {
                    actor: ActorId("b".to_string()),
                    seq: 1,
                },
                kind: OperationKind::UpsertFootnote { footnote: old },
                context: None,
            }],
        ],
    )
    .unwrap();
    assert_eq!(result.document.footnotes.len(), 1);
    match &result.document.footnotes[0].body[0] {
        Inline::Text { text, .. } => assert_eq!(text, "new footnote"),
        _ => panic!("expected text footnote body"),
    }
}

#[test]
fn whitespace_footnote_body_upsert_degrades_to_warning() {
    let mut base = Document::new("Doc");
    let footnote_id = StableId::parse("footnote-whitespace").unwrap();
    base.footnotes.push(Footnote {
        id: footnote_id.clone(),
        revision: 1,
        body: vec![Inline::text("old footnote")],
        deleted: false,
    });
    let result = merge_operations(
        &base,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::UpsertFootnote {
                footnote: Footnote {
                    id: footnote_id,
                    revision: 2,
                    body: vec![Inline::text(" ")],
                    deleted: false,
                },
            },
            context: None,
        }]],
    )
    .unwrap();

    assert_eq!(result.warnings[0].code, "invalid-footnote");
    match &result.document.footnotes[0].body[0] {
        Inline::Text { text, .. } => assert_eq!(text, "old footnote"),
        _ => panic!("expected text footnote body"),
    }
    result.document.validate().unwrap();
}

#[test]
fn missing_footnote_reference_targets_are_removed_with_warning() {
    let mut base = Document::new("Doc");
    let missing_footnote_id = StableId::parse("missing-footnote").unwrap();
    base.blocks.push(Block {
        id: StableId::parse("block-1").unwrap(),
        kind: BlockKind::Paragraph,
        content: vec![Inline::text("body")],
        properties: BlockProperties::default(),
    });
    base.blocks.push(Block {
        id: StableId::parse("table-block").unwrap(),
        kind: BlockKind::table(vec![TableRow {
            id: StableId::parse("row-1").unwrap(),
            cells: vec![TableCell {
                id: StableId::parse("cell-1").unwrap(),
                span: CellSpan::SINGLE,
                properties: Default::default(),
                blocks: vec![Block {
                    id: StableId::parse("nested-block").unwrap(),
                    kind: BlockKind::Paragraph,
                    content: vec![
                        Inline::text("nested"),
                        Inline::FootnoteRef {
                            id: StableId::parse("nested-footnote-ref").unwrap(),
                            footnote_id: missing_footnote_id.clone(),
                        },
                    ],
                    properties: BlockProperties::default(),
                }],
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
                block_id: StableId::parse("block-1").unwrap(),
                after: None,
                inline: Inline::FootnoteRef {
                    id: StableId::parse("footnote-ref-1").unwrap(),
                    footnote_id: missing_footnote_id,
                },
            },
            context: None,
        }]],
    )
    .unwrap();

    assert!(result.document.validate().is_ok());
    assert!(!result
        .document
        .blocks
        .iter()
        .flat_map(|block| block.content.iter())
        .any(|inline| matches!(inline, Inline::FootnoteRef { .. })));
    match &result.document.blocks[1].kind {
        BlockKind::Table { rows, .. } => {
            assert!(!rows[0].cells[0].blocks[0]
                .content
                .iter()
                .any(|inline| matches!(inline, Inline::FootnoteRef { .. })));
        }
        _ => panic!("expected table"),
    }
    assert!(result
        .warnings
        .iter()
        .any(|warning| warning.code == "footnote-reference-target-missing"));
}

#[test]
fn footnote_body_update_and_reference_delete_converge_to_deleted_footnote() {
    let mut base = Document::new("Doc");
    let footnote_id = StableId::parse("footnote-1").unwrap();
    let footnote_ref_id = StableId::parse("footnote-ref-1").unwrap();
    base.footnotes.push(Footnote {
        id: footnote_id.clone(),
        revision: 1,
        body: vec![Inline::text("old footnote")],
        deleted: false,
    });
    base.blocks.push(Block {
        id: StableId::new("block"),
        kind: BlockKind::Paragraph,
        content: vec![
            Inline::text("body"),
            Inline::FootnoteRef {
                id: footnote_ref_id.clone(),
                footnote_id: footnote_id.clone(),
            },
        ],
        properties: BlockProperties::default(),
    });
    let update = Operation {
        id: OperationId {
            actor: ActorId("a".to_string()),
            seq: 1,
        },
        kind: OperationKind::UpsertFootnote {
            footnote: Footnote {
                id: footnote_id.clone(),
                revision: 2,
                body: vec![Inline::text("new footnote")],
                deleted: false,
            },
        },
        context: None,
    };
    let delete_ref = Operation {
        id: OperationId {
            actor: ActorId("b".to_string()),
            seq: 1,
        },
        kind: OperationKind::DeleteInline {
            inline_id: footnote_ref_id,
        },
        context: None,
    };

    let update_first =
        merge_operations(&base, &[vec![update.clone()], vec![delete_ref.clone()]]).unwrap();
    let delete_first = merge_operations(&base, &[vec![delete_ref], vec![update]]).unwrap();

    assert_eq!(update_first.document, delete_first.document);
    assert!(update_first.document.footnotes[0].deleted);
    match &update_first.document.footnotes[0].body[0] {
        Inline::Text { text, .. } => assert_eq!(text, "new footnote"),
        _ => panic!("expected text footnote body"),
    }
    assert_eq!(update_first.warnings[0].code, "footnote-reference-missing");
    assert!(update_first.document.validate().is_ok());
}

#[test]
fn citation_reference_updates_are_revision_ordered() {
    let base = Document::new("Doc");
    let reference_id = StableId::parse("ref-doe-2020").unwrap();
    let old = BibliographyReference {
        id: reference_id.clone(),
        revision: 1,
        source: CitationSource {
            format: CitationSourceFormat::CitumNative,
            bytes: b"title: Old".to_vec(),
        },
        summary: CitationSummary {
            title: "Old".to_string(),
            authors: vec!["Doe".to_string()],
            issued: Some("2020".to_string()),
            doi: None,
            url: None,
        },
        deleted: false,
    };
    let new = BibliographyReference {
        id: reference_id,
        revision: 2,
        source: CitationSource {
            format: CitationSourceFormat::CitumNative,
            bytes: b"title: New".to_vec(),
        },
        summary: CitationSummary {
            title: "New".to_string(),
            authors: vec!["Doe".to_string()],
            issued: Some("2021".to_string()),
            doi: None,
            url: None,
        },
        deleted: false,
    };
    let result = merge_operations(
        &base,
        &[
            vec![Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 2,
                },
                kind: OperationKind::UpsertBibliographyReference { reference: new },
                context: None,
            }],
            vec![Operation {
                id: OperationId {
                    actor: ActorId("b".to_string()),
                    seq: 1,
                },
                kind: OperationKind::UpsertBibliographyReference { reference: old },
                context: None,
            }],
        ],
    )
    .unwrap();
    assert_eq!(
        result.document.citation_database.references[0]
            .summary
            .title,
        "New"
    );
}

#[test]
fn citation_reference_update_commutes_with_anchor_delete() {
    let mut base = Document::new("Doc");
    let reference_id = StableId::parse("ref-doe-2020").unwrap();
    let citation_id = StableId::parse("cite-intro").unwrap();
    base.citation_database
        .upsert_reference(BibliographyReference {
            id: reference_id.clone(),
            revision: 1,
            source: CitationSource {
                format: CitationSourceFormat::CitumNative,
                bytes: b"title: Old".to_vec(),
            },
            summary: CitationSummary {
                title: "Old".to_string(),
                authors: vec!["Doe".to_string()],
                issued: Some("2020".to_string()),
                doi: None,
                url: None,
            },
            deleted: false,
        });
    base.citation_database.upsert_citation(CitationGroup {
        id: citation_id.clone(),
        revision: 1,
        items: vec![CitationItem {
            reference_id: reference_id.clone(),
            locator: None,
            label: None,
            prefix: None,
            suffix: None,
            suppress_author: false,
        }],
        placement: CitationPlacement::Inline,
        rendered_cache: Some("(Doe 2020)".to_string()),
        deleted: false,
    });
    base.blocks.push(Block {
        id: StableId::new("block"),
        kind: BlockKind::Paragraph,
        content: vec![
            Inline::text("cited "),
            Inline::Citation {
                id: StableId::parse("citation-label").unwrap(),
                citation_id,
                rendered_cache: Some("(Doe 2020)".to_string()),
            },
        ],
        properties: BlockProperties::default(),
    });

    let update = Operation {
        id: OperationId {
            actor: ActorId("a".to_string()),
            seq: 1,
        },
        kind: OperationKind::UpsertBibliographyReference {
            reference: BibliographyReference {
                id: reference_id,
                revision: 2,
                source: CitationSource {
                    format: CitationSourceFormat::CitumNative,
                    bytes: b"title: New".to_vec(),
                },
                summary: CitationSummary {
                    title: "New".to_string(),
                    authors: vec!["Doe".to_string()],
                    issued: Some("2021".to_string()),
                    doi: None,
                    url: None,
                },
                deleted: false,
            },
        },
        context: None,
    };
    let delete_anchor = Operation {
        id: OperationId {
            actor: ActorId("b".to_string()),
            seq: 1,
        },
        kind: OperationKind::DeleteInline {
            inline_id: StableId::parse("citation-label").unwrap(),
        },
        context: None,
    };

    let update_first =
        merge_operations(&base, &[vec![update.clone()], vec![delete_anchor.clone()]]).unwrap();
    let delete_first = merge_operations(&base, &[vec![delete_anchor], vec![update]]).unwrap();

    assert_eq!(update_first.document, delete_first.document);
    assert_eq!(update_first.document.visible_text(), "cited \n");
    assert_eq!(
        update_first.document.citation_database.references[0]
            .summary
            .title,
        "New"
    );
    assert!(update_first.document.validate().is_ok());
    assert!(update_first.warnings.is_empty());
}

#[test]
fn citation_group_item_update_commutes_with_reference_update() {
    let mut base = Document::new("Doc");
    let reference_id = StableId::parse("ref-doe-2020").unwrap();
    let citation_id = StableId::parse("cite-intro").unwrap();
    base.citation_database
        .upsert_reference(BibliographyReference {
            id: reference_id.clone(),
            revision: 1,
            source: CitationSource {
                format: CitationSourceFormat::CitumNative,
                bytes: b"title: Old".to_vec(),
            },
            summary: CitationSummary {
                title: "Old".to_string(),
                authors: vec!["Doe".to_string()],
                issued: Some("2020".to_string()),
                doi: None,
                url: None,
            },
            deleted: false,
        });
    base.citation_database.upsert_citation(CitationGroup {
        id: citation_id.clone(),
        revision: 1,
        items: vec![CitationItem {
            reference_id: reference_id.clone(),
            locator: Some("17".to_string()),
            label: Some("page".to_string()),
            prefix: Some("see".to_string()),
            suffix: None,
            suppress_author: false,
        }],
        placement: CitationPlacement::Inline,
        rendered_cache: Some("(see Doe 2020, 17)".to_string()),
        deleted: false,
    });
    base.blocks.push(Block {
        id: StableId::new("block"),
        kind: BlockKind::Paragraph,
        content: vec![
            Inline::text("cited "),
            Inline::Citation {
                id: StableId::parse("citation-label").unwrap(),
                citation_id: citation_id.clone(),
                rendered_cache: Some("(see Doe 2020, 17)".to_string()),
            },
        ],
        properties: BlockProperties::default(),
    });

    let reference_update = Operation {
        id: OperationId {
            actor: ActorId("a".to_string()),
            seq: 1,
        },
        kind: OperationKind::UpsertBibliographyReference {
            reference: BibliographyReference {
                id: reference_id.clone(),
                revision: 2,
                source: CitationSource {
                    format: CitationSourceFormat::CitumNative,
                    bytes: b"title: New\nauthor: Smith; Jones\nyear: 2024\ndoi: 10.7777/merge\nurl: https://example.invalid/merge".to_vec(),
                },
                summary: CitationSummary {
                    title: "New".to_string(),
                    authors: vec!["Smith".to_string(), "Jones".to_string()],
                    issued: Some("2024".to_string()),
                    doi: Some("10.7777/merge".to_string()),
                    url: Some("https://example.invalid/merge".to_string()),
                },
                deleted: false,
            },
        },
context: None,
    };
    let item_update = Operation {
        id: OperationId {
            actor: ActorId("b".to_string()),
            seq: 1,
        },
        kind: OperationKind::UpsertCitationGroup {
            citation: CitationGroup {
                id: citation_id,
                revision: 2,
                items: vec![CitationItem {
                    reference_id,
                    locator: Some("19".to_string()),
                    label: Some("page".to_string()),
                    prefix: Some("compare".to_string()),
                    suffix: Some("for context".to_string()),
                    suppress_author: true,
                }],
                placement: CitationPlacement::Inline,
                rendered_cache: Some("(stale editor cache)".to_string()),
                deleted: false,
            },
        },
        context: None,
    };

    let reference_first = merge_operations(
        &base,
        &[vec![reference_update.clone()], vec![item_update.clone()]],
    )
    .unwrap();
    let item_first = merge_operations(&base, &[vec![item_update], vec![reference_update]]).unwrap();

    assert_eq!(reference_first.document, item_first.document);
    assert_eq!(
        reference_first.document.visible_text(),
        "cited (compare 2024, page 19 for context)\n"
    );
    let reference = &reference_first.document.citation_database.references[0];
    assert_eq!(reference.summary.title, "New");
    assert_eq!(
        reference.summary.authors,
        vec!["Smith".to_string(), "Jones".to_string()]
    );
    assert_eq!(reference.summary.doi.as_deref(), Some("10.7777/merge"));
    assert_eq!(
        reference.summary.url.as_deref(),
        Some("https://example.invalid/merge")
    );
    let source = String::from_utf8_lossy(&reference.source.bytes);
    assert!(source.contains("author: Smith; Jones"));
    assert!(source.contains("doi: 10.7777/merge"));
    assert!(source.contains("url: https://example.invalid/merge"));
    assert_eq!(
        reference_first.document.citation_database.citations[0].rendered_cache,
        Some("(compare 2024, page 19 for context)".to_string())
    );
    match &reference_first.document.blocks[0].content[1] {
        Inline::Citation { rendered_cache, .. } => assert_eq!(rendered_cache, &None),
        _ => panic!("expected citation label"),
    }
    assert!(reference_first.document.validate().is_ok());
    assert!(reference_first.warnings.is_empty());
}

#[test]
fn deleting_bibliography_reference_invalidates_dependent_citation_caches() {
    let mut base = Document::new("Doc");
    let reference_id = StableId::parse("ref-doe-2020").unwrap();
    let citation_id = StableId::parse("cite-intro").unwrap();
    base.citation_database
        .upsert_reference(BibliographyReference {
            id: reference_id.clone(),
            revision: 1,
            source: CitationSource {
                format: CitationSourceFormat::CitumNative,
                bytes: b"title: Old".to_vec(),
            },
            summary: CitationSummary {
                title: "Old".to_string(),
                authors: vec!["Doe".to_string()],
                issued: Some("2020".to_string()),
                doi: None,
                url: None,
            },
            deleted: false,
        });
    base.citation_database.upsert_citation(CitationGroup {
        id: citation_id.clone(),
        revision: 1,
        items: vec![CitationItem {
            reference_id: reference_id.clone(),
            locator: None,
            label: None,
            prefix: None,
            suffix: None,
            suppress_author: false,
        }],
        placement: CitationPlacement::Inline,
        rendered_cache: Some("(Doe 2020)".to_string()),
        deleted: false,
    });
    base.blocks.push(Block {
        id: StableId::new("block"),
        kind: BlockKind::Paragraph,
        content: vec![Inline::Citation {
            id: StableId::parse("citation-label").unwrap(),
            citation_id: citation_id.clone(),
            rendered_cache: Some("(Doe 2020)".to_string()),
        }],
        properties: BlockProperties::default(),
    });

    let result = merge_operations(
        &base,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::DeleteBibliographyReference {
                reference_id,
                revision: 2,
            },
            context: None,
        }]],
    )
    .unwrap();

    assert_eq!(result.document.visible_text(), "[cite-intro]\n");
    assert!(result.document.citation_database.references[0].deleted);
    assert_eq!(
        result.document.citation_database.citations[0].rendered_cache,
        None
    );
    match &result.document.blocks[0].content[0] {
        Inline::Citation { rendered_cache, .. } => assert_eq!(rendered_cache, &None),
        _ => panic!("expected citation label"),
    }
    assert!(result
        .warnings
        .iter()
        .any(|warning| warning.code == "citation-reference-missing"));
    assert!(result.document.validate().is_ok());
}

#[test]
fn bibliography_reference_delete_wins_over_older_stale_upsert_by_revision() {
    let mut base = Document::new("Doc");
    let reference_id = StableId::parse("ref-delete-stale-upsert").unwrap();
    let citation_id = StableId::parse("cite-delete-stale-upsert").unwrap();
    base.citation_database
        .upsert_reference(BibliographyReference {
            id: reference_id.clone(),
            revision: 1,
            source: CitationSource {
                format: CitationSourceFormat::CitumNative,
                bytes: b"title: Original".to_vec(),
            },
            summary: CitationSummary {
                title: "Original".to_string(),
                authors: vec!["Doe".to_string()],
                issued: Some("2020".to_string()),
                doi: None,
                url: None,
            },
            deleted: false,
        });
    base.citation_database.upsert_citation(CitationGroup {
        id: citation_id.clone(),
        revision: 1,
        items: vec![CitationItem {
            reference_id: reference_id.clone(),
            locator: None,
            label: None,
            prefix: None,
            suffix: None,
            suppress_author: false,
        }],
        placement: CitationPlacement::Inline,
        rendered_cache: Some("(Doe 2020)".to_string()),
        deleted: false,
    });
    base.blocks.push(Block {
        id: StableId::new("block"),
        kind: BlockKind::Paragraph,
        content: vec![Inline::Citation {
            id: StableId::parse("citation-label-ref-delete-stale").unwrap(),
            citation_id: citation_id.clone(),
            rendered_cache: Some("(Doe 2020)".to_string()),
        }],
        properties: BlockProperties::default(),
    });

    let stale_update = Operation {
        id: OperationId {
            actor: ActorId("actor-a".to_string()),
            seq: 1,
        },
        kind: OperationKind::UpsertBibliographyReference {
            reference: BibliographyReference {
                id: reference_id.clone(),
                revision: 2,
                source: CitationSource {
                    format: CitationSourceFormat::CitumNative,
                    bytes: b"title: Stale\nyear: 2021".to_vec(),
                },
                summary: CitationSummary {
                    title: "Stale".to_string(),
                    authors: vec!["Doe".to_string()],
                    issued: Some("2021".to_string()),
                    doi: None,
                    url: None,
                },
                deleted: false,
            },
        },
        context: None,
    };
    let delete = Operation {
        id: OperationId {
            actor: ActorId("actor-z".to_string()),
            seq: 1,
        },
        kind: OperationKind::DeleteBibliographyReference {
            reference_id,
            revision: 3,
        },
        context: None,
    };

    let update_first =
        merge_operations(&base, &[vec![stale_update.clone()], vec![delete.clone()]]).unwrap();
    let delete_first = merge_operations(&base, &[vec![delete], vec![stale_update]]).unwrap();

    assert_eq!(update_first.document, delete_first.document);
    assert_eq!(update_first.warnings, delete_first.warnings);
    let reference = &update_first.document.citation_database.references[0];
    assert!(reference.deleted);
    assert_eq!(reference.revision, 3);
    assert_eq!(reference.summary.title, "Stale");
    assert_eq!(
        update_first.document.visible_text(),
        "[cite-delete-stale-upsert]\n"
    );
    assert_eq!(
        update_first.document.citation_database.citations[0].rendered_cache,
        None
    );
    match &update_first.document.blocks[0].content[0] {
        Inline::Citation { rendered_cache, .. } => assert_eq!(rendered_cache, &None),
        _ => panic!("expected citation label"),
    }
    assert!(update_first
        .warnings
        .iter()
        .any(|warning| warning.code == "citation-reference-missing"));
    update_first.document.validate().unwrap();
}

#[test]
fn concurrent_style_change_and_reference_delete_converge_for_table_citations() {
    let mut base = Document::new("Doc");
    let reference_id = StableId::parse("ref-doe-2020").unwrap();
    let citation_id = StableId::parse("cite-table").unwrap();
    base.citation_database
        .upsert_reference(BibliographyReference {
            id: reference_id.clone(),
            revision: 1,
            source: CitationSource {
                format: CitationSourceFormat::CitumNative,
                bytes: b"title: Table Source".to_vec(),
            },
            summary: CitationSummary {
                title: "Table Source".to_string(),
                authors: vec!["Doe".to_string()],
                issued: Some("2020".to_string()),
                doi: None,
                url: None,
            },
            deleted: false,
        });
    base.citation_database.upsert_citation(CitationGroup {
        id: citation_id.clone(),
        revision: 1,
        items: vec![CitationItem {
            reference_id: reference_id.clone(),
            locator: None,
            label: None,
            prefix: None,
            suffix: None,
            suppress_author: false,
        }],
        placement: CitationPlacement::Inline,
        rendered_cache: Some("(Doe 2020)".to_string()),
        deleted: false,
    });
    base.blocks.push(Block {
        id: StableId::new("table"),
        kind: BlockKind::table(vec![TableRow {
            id: StableId::new("row"),
            cells: vec![TableCell {
                id: StableId::new("cell"),
                span: CellSpan::SINGLE,
                properties: Default::default(),
                blocks: vec![Block {
                    id: StableId::new("cell-block"),
                    kind: BlockKind::Paragraph,
                    content: vec![Inline::Citation {
                        id: StableId::parse("citation-label-table").unwrap(),
                        citation_id: citation_id.clone(),
                        rendered_cache: Some("(Doe 2020)".to_string()),
                    }],
                    properties: BlockProperties::default(),
                }],
            }],
        }]),
        content: Vec::new(),
        properties: BlockProperties::default(),
    });

    let delete_reference = Operation {
        id: OperationId {
            actor: ActorId("a".to_string()),
            seq: 1,
        },
        kind: OperationKind::DeleteBibliographyReference {
            reference_id,
            revision: 2,
        },
        context: None,
    };
    let style_change = Operation {
        id: OperationId {
            actor: ActorId("b".to_string()),
            seq: 1,
        },
        kind: OperationKind::UpdateCitationStyle {
            style: "ieee".to_string(),
            locale: "en-US".to_string(),
        },
        context: None,
    };

    let delete_first = merge_operations(
        &base,
        &[vec![delete_reference.clone()], vec![style_change.clone()]],
    )
    .unwrap();
    let style_first =
        merge_operations(&base, &[vec![style_change], vec![delete_reference]]).unwrap();

    assert_eq!(delete_first.document, style_first.document);
    assert_eq!(delete_first.warnings, style_first.warnings);
    assert_eq!(delete_first.document.visible_text(), "[cite-table]\n");
    assert!(delete_first.document.citation_database.references[0].deleted);
    assert_eq!(
        delete_first.document.citation_database.citations[0].rendered_cache,
        None
    );
    match &delete_first.document.blocks[0].kind {
        BlockKind::Table { rows, .. } => match &rows[0].cells[0].blocks[0].content[0] {
            Inline::Citation { rendered_cache, .. } => assert_eq!(rendered_cache, &None),
            _ => panic!("expected citation label"),
        },
        _ => panic!("expected table"),
    }
    assert!(delete_first
        .warnings
        .iter()
        .any(|warning| warning.code == "citation-reference-missing"));
    assert!(delete_first.document.validate().is_ok());
}

#[test]
fn bibliography_reference_restore_wins_over_older_delete_by_revision() {
    let mut base = Document::new("Doc");
    let reference_id = StableId::parse("ref-doe-2020").unwrap();
    let citation_id = StableId::parse("cite-intro").unwrap();
    base.citation_database
        .upsert_reference(BibliographyReference {
            id: reference_id.clone(),
            revision: 1,
            source: CitationSource {
                format: CitationSourceFormat::CitumNative,
                bytes: b"title: Old".to_vec(),
            },
            summary: CitationSummary {
                title: "Old".to_string(),
                authors: vec!["Doe".to_string()],
                issued: Some("2020".to_string()),
                doi: None,
                url: None,
            },
            deleted: false,
        });
    base.citation_database.upsert_citation(CitationGroup {
        id: citation_id.clone(),
        revision: 1,
        items: vec![CitationItem {
            reference_id: reference_id.clone(),
            locator: None,
            label: None,
            prefix: None,
            suffix: None,
            suppress_author: false,
        }],
        placement: CitationPlacement::Inline,
        rendered_cache: Some("(Doe 2020)".to_string()),
        deleted: false,
    });
    base.blocks.push(Block {
        id: StableId::new("block"),
        kind: BlockKind::Paragraph,
        content: vec![Inline::Citation {
            id: StableId::parse("citation-label").unwrap(),
            citation_id,
            rendered_cache: Some("(Doe 2020)".to_string()),
        }],
        properties: BlockProperties::default(),
    });

    let delete = Operation {
        id: OperationId {
            actor: ActorId("z".to_string()),
            seq: 1,
        },
        kind: OperationKind::DeleteBibliographyReference {
            reference_id: reference_id.clone(),
            revision: 2,
        },
        context: None,
    };
    let restore = Operation {
        id: OperationId {
            actor: ActorId("a".to_string()),
            seq: 1,
        },
        kind: OperationKind::UpsertBibliographyReference {
            reference: BibliographyReference {
                id: reference_id,
                revision: 3,
                source: CitationSource {
                    format: CitationSourceFormat::CitumNative,
                    bytes: b"title: Restored".to_vec(),
                },
                summary: CitationSummary {
                    title: "Restored".to_string(),
                    authors: vec!["Doe".to_string()],
                    issued: Some("2020".to_string()),
                    doi: None,
                    url: None,
                },
                deleted: false,
            },
        },
        context: None,
    };

    let restore_first =
        merge_operations(&base, &[vec![restore.clone()], vec![delete.clone()]]).unwrap();
    let delete_first = merge_operations(&base, &[vec![delete], vec![restore]]).unwrap();

    assert_eq!(restore_first.document, delete_first.document);
    assert!(!restore_first.document.citation_database.references[0].deleted);
    assert_eq!(restore_first.document.visible_text(), "(Doe 2020)\n");
    assert!(restore_first.warnings.is_empty());
    assert!(restore_first.document.validate().is_ok());
}
