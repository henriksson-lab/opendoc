//! Citation cache tests.

use crate::causal::{ActorId, OperationId};
use crate::merge::merge_operations;
use crate::operation::{Operation, OperationKind};
use opendoc_core::{
    BibliographyReference, Block, BlockKind, BlockProperties, CellSpan, CitationGroup,
    CitationItem, CitationPlacement, CitationSource, CitationSourceFormat, CitationSummary,
    Document, Footnote, Inline, StableId, TableCell, TableRow,
};

#[test]
fn citation_missing_reference_clears_stale_rendered_labels() {
    let mut base = Document::new("Doc");
    let citation_id = StableId::parse("cite-missing-reference").unwrap();
    base.blocks.push(Block {
        id: StableId::new("block"),
        kind: BlockKind::Paragraph,
        content: vec![Inline::Citation {
            id: StableId::parse("citation-label-missing-reference").unwrap(),
            citation_id: citation_id.clone(),
            rendered_cache: Some("(Misleading 2020)".to_string()),
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
            kind: OperationKind::UpsertCitationGroup {
                citation: CitationGroup {
                    id: citation_id,
                    revision: 1,
                    items: vec![CitationItem {
                        reference_id: StableId::parse("ref-missing").unwrap(),
                        locator: Some("42".to_string()),
                        label: Some("page".to_string()),
                        prefix: None,
                        suffix: None,
                        suppress_author: false,
                    }],
                    placement: CitationPlacement::Inline,
                    rendered_cache: Some("(Misleading 2020)".to_string()),
                    deleted: false,
                },
            },
            context: None,
        }]],
    )
    .unwrap();

    assert_eq!(result.document.visible_text(), "[cite-missing-reference]\n");
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
}

#[test]
fn missing_citation_group_clears_inline_rendered_label_cache() {
    let mut base = Document::new("Doc");
    let citation_id = StableId::parse("cite-missing-group").unwrap();
    base.blocks.push(Block {
        id: StableId::new("block"),
        kind: BlockKind::Paragraph,
        content: vec![Inline::Citation {
            id: StableId::parse("citation-label-missing-group").unwrap(),
            citation_id: citation_id.clone(),
            rendered_cache: Some("(Stale Citation)".to_string()),
        }],
        properties: BlockProperties::default(),
    });

    let result = merge_operations(&base, &[Vec::new()]).unwrap();

    assert_eq!(result.document.visible_text(), "[cite-missing-group]\n");
    match &result.document.blocks[0].content[0] {
        Inline::Citation { rendered_cache, .. } => assert_eq!(rendered_cache, &None),
        _ => panic!("expected citation label"),
    }
    assert!(result
        .warnings
        .iter()
        .any(|warning| warning.code == "citation-group-missing"));
}

#[test]
fn deleted_citation_group_clears_nested_inline_rendered_label_cache() {
    let mut base = Document::new("Doc");
    let reference_id = StableId::parse("ref-doe-2020").unwrap();
    let citation_id = StableId::parse("cite-deleted-group").unwrap();
    base.citation_database
        .upsert_reference(BibliographyReference {
            id: reference_id.clone(),
            revision: 1,
            source: CitationSource {
                format: CitationSourceFormat::CitumNative,
                bytes: b"title: Example".to_vec(),
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
    base.citation_database.upsert_citation(CitationGroup {
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
        rendered_cache: Some("(Doe 2020)".to_string()),
        deleted: false,
    });
    base.blocks.push(Block {
        id: StableId::new("table"),
        kind: BlockKind::table(vec![TableRow {
            id: StableId::new("row"),
            height: None,
            header: false,
            cells: vec![TableCell {
                id: StableId::new("cell"),
                span: CellSpan::SINGLE,
                properties: Default::default(),
                blocks: vec![Block {
                    id: StableId::new("cell-block"),
                    kind: BlockKind::Paragraph,
                    content: vec![Inline::Citation {
                        id: StableId::parse("citation-label-deleted-group").unwrap(),
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

    let result = merge_operations(
        &base,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::DeleteCitationGroup {
                citation_id,
                revision: 2,
            },
            context: None,
        }]],
    )
    .unwrap();

    assert_eq!(result.document.visible_text(), "[cite-deleted-group]\n");
    match &result.document.blocks[0].kind {
        BlockKind::Table { rows, .. } => match &rows[0].cells[0].blocks[0].content[0] {
            Inline::Citation { rendered_cache, .. } => assert_eq!(rendered_cache, &None),
            _ => panic!("expected citation label"),
        },
        _ => panic!("expected table"),
    }
    assert!(result.document.citation_database.citations[0].deleted);
    assert!(result
        .warnings
        .iter()
        .any(|warning| warning.code == "citation-group-missing"));
}

#[test]
fn citation_group_restore_wins_over_older_delete_by_revision() {
    let mut base = Document::new("Doc");
    let reference_id = StableId::parse("ref-doe-2020").unwrap();
    let citation_id = StableId::parse("cite-intro").unwrap();
    base.citation_database
        .upsert_reference(BibliographyReference {
            id: reference_id.clone(),
            revision: 1,
            source: CitationSource {
                format: CitationSourceFormat::CitumNative,
                bytes: b"title: Example".to_vec(),
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

    let delete = Operation {
        id: OperationId {
            actor: ActorId("z".to_string()),
            seq: 1,
        },
        kind: OperationKind::DeleteCitationGroup {
            citation_id: citation_id.clone(),
            revision: 2,
        },
        context: None,
    };
    let restore = Operation {
        id: OperationId {
            actor: ActorId("a".to_string()),
            seq: 1,
        },
        kind: OperationKind::UpsertCitationGroup {
            citation: CitationGroup {
                id: citation_id,
                revision: 3,
                items: vec![CitationItem {
                    reference_id,
                    locator: Some("12".to_string()),
                    label: Some("page".to_string()),
                    prefix: Some("see".to_string()),
                    suffix: None,
                    suppress_author: false,
                }],
                placement: CitationPlacement::Inline,
                rendered_cache: Some("(stale restore cache)".to_string()),
                deleted: false,
            },
        },
        context: None,
    };

    let restore_first =
        merge_operations(&base, &[vec![restore.clone()], vec![delete.clone()]]).unwrap();
    let delete_first = merge_operations(&base, &[vec![delete], vec![restore]]).unwrap();

    assert_eq!(restore_first.document, delete_first.document);
    assert!(!restore_first.document.citation_database.citations[0].deleted);
    assert_eq!(
        restore_first.document.visible_text(),
        "(see Doe, 2020, p. 12)\n"
    );
    // Merge re-renders the group it restored, so this is the renderer's
    // output rather than the stale cache the operation carried: APA 7 through
    // CSL, which `CitationDatabase::default()`'s `apa` style selects.
    assert_eq!(
        restore_first.document.citation_database.citations[0].rendered_cache,
        Some("(see Doe, 2020, p. 12)".to_string())
    );
    assert!(restore_first.warnings.is_empty());
}

#[test]
fn citation_group_delete_wins_over_older_stale_upsert_by_revision() {
    let mut base = Document::new("Doc");
    let reference_id = StableId::parse("ref-doe-2020").unwrap();
    let citation_id = StableId::parse("cite-intro").unwrap();
    base.citation_database
        .upsert_reference(BibliographyReference {
            id: reference_id.clone(),
            revision: 1,
            source: CitationSource {
                format: CitationSourceFormat::CitumNative,
                bytes: b"title: Example".to_vec(),
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

    let stale_update = Operation {
        id: OperationId {
            actor: ActorId("a".to_string()),
            seq: 1,
        },
        kind: OperationKind::UpsertCitationGroup {
            citation: CitationGroup {
                id: citation_id.clone(),
                revision: 2,
                items: vec![CitationItem {
                    reference_id,
                    locator: Some("44".to_string()),
                    label: Some("page".to_string()),
                    prefix: Some("see".to_string()),
                    suffix: None,
                    suppress_author: false,
                }],
                placement: CitationPlacement::Inline,
                rendered_cache: Some("(stale rendered label)".to_string()),
                deleted: false,
            },
        },
        context: None,
    };
    let delete = Operation {
        id: OperationId {
            actor: ActorId("z".to_string()),
            seq: 1,
        },
        kind: OperationKind::DeleteCitationGroup {
            citation_id: citation_id.clone(),
            revision: 3,
        },
        context: None,
    };

    let update_first =
        merge_operations(&base, &[vec![stale_update.clone()], vec![delete.clone()]]).unwrap();
    let delete_first = merge_operations(&base, &[vec![delete], vec![stale_update]]).unwrap();

    assert_eq!(update_first.document, delete_first.document);
    assert!(update_first.document.citation_database.citations[0].deleted);
    assert_eq!(
        update_first.document.citation_database.citations[0].rendered_cache,
        None
    );
    assert_eq!(update_first.document.visible_text(), "[cite-intro]\n");
    match &update_first.document.blocks[0].content[0] {
        Inline::Citation { rendered_cache, .. } => assert_eq!(rendered_cache, &None),
        _ => panic!("expected citation label"),
    }
    assert!(update_first
        .warnings
        .iter()
        .any(|warning| warning.code == "citation-group-missing"));
}

#[test]
fn citation_style_update_invalidates_table_inline_caches() {
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
            reference_id,
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
        kind: BlockKind::table(vec![opendoc_core::TableRow {
            id: StableId::new("row"),
            height: None,
            header: false,
            cells: vec![opendoc_core::TableCell {
                id: StableId::new("cell"),
                span: CellSpan::SINGLE,
                properties: Default::default(),
                blocks: vec![Block {
                    id: StableId::new("cell-block"),
                    kind: BlockKind::Paragraph,
                    content: vec![Inline::Citation {
                        id: StableId::parse("citation-label").unwrap(),
                        citation_id,
                        rendered_cache: Some("(Doe 2020)".to_string()),
                    }],
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
            kind: OperationKind::UpdateCitationStyle {
                style: "ieee".to_string(),
                locale: "en-US".to_string(),
            },
            context: None,
        }]],
    )
    .unwrap();

    assert_eq!(result.document.visible_text(), "[1]\n");
    assert_eq!(
        result.document.citation_database.citations[0].rendered_cache,
        Some("[1]".to_string())
    );
    match &result.document.blocks[0].kind {
        BlockKind::Table { rows, .. } => match &rows[0].cells[0].blocks[0].content[0] {
            Inline::Citation { rendered_cache, .. } => assert_eq!(rendered_cache, &None),
            _ => panic!("expected citation label"),
        },
        _ => panic!("expected table"),
    }
}

#[test]
fn invalid_retained_record_upserts_degrade_to_warnings() {
    let base = Document::new("Doc");
    let result = merge_operations(
        &base,
        &[vec![
            Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::UpsertFootnote {
                    footnote: Footnote {
                        id: StableId::parse("footnote-empty").unwrap(),
                        revision: 1,
                        body: Vec::new(),
                        deleted: false,
                    },
                },
                context: None,
            },
            Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 2,
                },
                kind: OperationKind::UpsertBibliographyReference {
                    reference: BibliographyReference {
                        id: StableId::parse("ref-empty").unwrap(),
                        revision: 1,
                        source: CitationSource {
                            format: CitationSourceFormat::CitumNative,
                            bytes: Vec::new(),
                        },
                        summary: CitationSummary {
                            title: String::new(),
                            authors: Vec::new(),
                            issued: None,
                            doi: None,
                            url: None,
                        },
                        deleted: false,
                    },
                },
                context: None,
            },
            Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 3,
                },
                kind: OperationKind::UpsertCitationGroup {
                    citation: CitationGroup {
                        id: StableId::parse("cite-empty").unwrap(),
                        revision: 1,
                        items: Vec::new(),
                        placement: CitationPlacement::Inline,
                        rendered_cache: None,
                        deleted: false,
                    },
                },
                context: None,
            },
        ]],
    )
    .unwrap();

    assert!(result.document.footnotes.is_empty());
    assert!(result.document.citation_database.references.is_empty());
    assert!(result.document.citation_database.citations.is_empty());
    assert_eq!(
        result
            .warnings
            .iter()
            .map(|warning| warning.code.as_str())
            .collect::<Vec<_>>(),
        vec![
            "invalid-footnote",
            "invalid-bibliography-reference",
            "invalid-citation-group"
        ]
    );
}

#[test]
fn missing_footnote_citation_target_degrades_to_inline_placement() {
    let base = Document::new("Doc");
    let reference_id = StableId::parse("ref-doe-2020").unwrap();
    let citation_id = StableId::parse("cite-footnote").unwrap();
    let result = merge_operations(
        &base,
        &[vec![
            Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::UpsertBibliographyReference {
                    reference: BibliographyReference {
                        id: reference_id.clone(),
                        revision: 1,
                        source: CitationSource {
                            format: CitationSourceFormat::CitumNative,
                            bytes: b"title: Example".to_vec(),
                        },
                        summary: CitationSummary {
                            title: "Example".to_string(),
                            authors: vec!["Doe".to_string()],
                            issued: Some("2020".to_string()),
                            doi: None,
                            url: None,
                        },
                        deleted: false,
                    },
                },
                context: None,
            },
            Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 2,
                },
                kind: OperationKind::UpsertCitationGroup {
                    citation: CitationGroup {
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
                            footnote_id: StableId::parse("missing-footnote").unwrap(),
                        },
                        rendered_cache: Some("(Doe 2020)".to_string()),
                        deleted: false,
                    },
                },
                context: None,
            },
        ]],
    )
    .unwrap();

    assert_eq!(result.warnings[0].code, "citation-footnote-target-missing");
    assert!(matches!(
        result.document.citation_database.citations[0].placement,
        CitationPlacement::Inline
    ));
    // The degraded placement is re-rendered, so this is the renderer's own
    // output and not the cache the operation carried: APA 7 through CSL,
    // which is what `CitationDatabase::default()`'s `apa` style selects.
    assert_eq!(
        result.document.citation_database.citations[0].rendered_cache,
        Some("(Doe, 2020)".to_string())
    );
}

#[test]
fn footnote_citation_placement_keeps_target_footnote_alive() {
    let mut base = Document::new("Doc");
    let footnote_id = StableId::parse("footnote-citation-target").unwrap();
    let reference_id = StableId::parse("ref-doe-2020").unwrap();
    let citation_id = StableId::parse("cite-footnote").unwrap();
    base.footnotes.push(Footnote {
        id: footnote_id.clone(),
        revision: 1,
        body: vec![Inline::text("citation lives in this footnote")],
        deleted: false,
    });
    base.citation_database
        .upsert_reference(BibliographyReference {
            id: reference_id.clone(),
            revision: 1,
            source: CitationSource {
                format: CitationSourceFormat::CitumNative,
                bytes: b"title: Example".to_vec(),
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
    base.citation_database.upsert_citation(CitationGroup {
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
        rendered_cache: Some("(Doe 2020)".to_string()),
        deleted: false,
    });

    let result = merge_operations(&base, &[Vec::new()]).unwrap();

    assert!(!result.document.footnotes[0].deleted);
    assert!(matches!(
        &result.document.citation_database.citations[0].placement,
        CitationPlacement::Footnote { footnote_id: id } if id == &footnote_id
    ));
    assert!(result.warnings.is_empty());
}

#[test]
fn footnote_citation_placement_survives_concurrent_inline_reference_delete() {
    let mut base = Document::new("Doc");
    let footnote_id = StableId::parse("footnote-citation-delete-ref").unwrap();
    let footnote_ref_id = StableId::parse("footnote-ref-delete-inline").unwrap();
    let reference_id = StableId::parse("ref-footnote-delete-inline").unwrap();
    let citation_id = StableId::parse("cite-footnote-delete-inline").unwrap();
    base.footnotes.push(Footnote {
        id: footnote_id.clone(),
        revision: 1,
        body: vec![Inline::text("citation footnote")],
        deleted: false,
    });
    base.citation_database
        .upsert_reference(BibliographyReference {
            id: reference_id.clone(),
            revision: 1,
            source: CitationSource {
                format: CitationSourceFormat::CitumNative,
                bytes: b"title: Footnote Citation".to_vec(),
            },
            summary: CitationSummary {
                title: "Footnote Citation".to_string(),
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
        rendered_cache: Some("(Doe 2020)".to_string()),
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

    let delete_ref = Operation {
        id: OperationId {
            actor: ActorId("actor-a".to_string()),
            seq: 1,
        },
        kind: OperationKind::DeleteInline {
            inline_id: footnote_ref_id,
        },
        context: None,
    };
    let style_update = Operation {
        id: OperationId {
            actor: ActorId("actor-b".to_string()),
            seq: 1,
        },
        kind: OperationKind::UpdateCitationStyle {
            style: "ieee".to_string(),
            locale: "en-GB".to_string(),
        },
        context: None,
    };

    let delete_first = merge_operations(
        &base,
        &[vec![delete_ref.clone()], vec![style_update.clone()]],
    )
    .unwrap();
    let style_first = merge_operations(&base, &[vec![style_update], vec![delete_ref]]).unwrap();

    assert_eq!(delete_first.document, style_first.document);
    assert_eq!(delete_first.warnings, style_first.warnings);
    assert_eq!(delete_first.document.visible_text(), "body\n");
    assert!(!delete_first.document.footnotes[0].deleted);
    assert!(delete_first.warnings.is_empty());
    assert!(matches!(
        &delete_first.document.citation_database.citations[0].placement,
        CitationPlacement::Footnote { footnote_id: id } if id == &footnote_id
    ));
    // The positive control: every assertion above is satisfied by the base
    // document, so without this the test also passed for a merge that dropped
    // both operations. Both of them have to be able to land. PLAN88 §7.
    assert_eq!(delete_first.document.citation_database.style, "ieee");
    assert_eq!(delete_first.document.citation_database.locale, "en-GB");
}
