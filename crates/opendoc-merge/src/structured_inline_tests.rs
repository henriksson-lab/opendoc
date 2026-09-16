//! Structured inline tests.

use crate::causal::{ActorId, OperationId};
use crate::inline_ops::inline_id;
use crate::merge::merge_operations;
use crate::operation::{Operation, OperationKind};
use opendoc_core::{
    BibliographyReference, Block, BlockKind, BlockProperties, CitationGroup, CitationItem,
    CitationPlacement, CitationSource, CitationSourceFormat, CitationSummary, Document, Equation,
    EquationSourceFormat, HashRef, Inline, StableId,
};

#[test]
fn citation_label_text_is_not_directly_editable() {
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
    let citation_inline = Inline::Citation {
        id: StableId::new("citation-label"),
        citation_id,
        rendered_cache: Some("(Doe 2020)".to_string()),
    };
    let citation_inline_id = inline_id(&citation_inline).clone();
    base.blocks.push(Block {
        id: StableId::new("block"),
        kind: BlockKind::Paragraph,
        content: vec![citation_inline],
        properties: BlockProperties::default(),
    });

    let result = merge_operations(
        &base,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::UpdateInlineText {
                inline_id: citation_inline_id,
                text: "manual edit".to_string(),
            },
            context: None,
        }]],
    )
    .unwrap();

    assert_eq!(result.warnings[0].code, "non-editable-inline");
    assert_eq!(result.document.visible_text(), "(Doe 2020)\n");
}

#[test]
fn mention_labels_update_as_structured_inline_state() {
    let mut base = Document::new("Doc");
    let mention = Inline::Mention {
        id: StableId::parse("mention-alice").unwrap(),
        label: "@alice".to_string(),
    };
    let mention_id = inline_id(&mention).clone();
    base.blocks.push(Block {
        id: StableId::new("block"),
        kind: BlockKind::Paragraph,
        content: vec![mention],
        properties: BlockProperties::default(),
    });

    let generic = merge_operations(
        &base,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::UpdateInlineText {
                inline_id: mention_id.clone(),
                text: "@generic".to_string(),
            },
            context: None,
        }]],
    )
    .unwrap();
    assert_eq!(generic.warnings[0].code, "non-editable-inline");
    assert_eq!(generic.document.visible_text(), "@alice\n");

    let structured = merge_operations(
        &base,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("b".to_string()),
                seq: 1,
            },
            kind: OperationKind::UpdateMentionLabel {
                inline_id: mention_id.clone(),
                label: "@bob".to_string(),
            },
            context: None,
        }]],
    )
    .unwrap();
    assert!(structured.warnings.is_empty());
    assert_eq!(structured.document.visible_text(), "@bob\n");

    let invalid = merge_operations(
        &base,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("c".to_string()),
                seq: 1,
            },
            kind: OperationKind::UpdateMentionLabel {
                inline_id: mention_id,
                label: " ".to_string(),
            },
            context: None,
        }]],
    )
    .unwrap();
    assert_eq!(invalid.warnings[0].code, "invalid-mention-label");
    assert_eq!(invalid.document.visible_text(), "@alice\n");
}

#[test]
fn inline_equation_source_updates_are_atomic_structured_operations() {
    let mut base = Document::new("Doc");
    let equation = Inline::Equation {
        id: StableId::parse("inline-equation").unwrap(),
        equation: Equation {
            id: StableId::parse("equation-source").unwrap(),
            source_format: EquationSourceFormat::LatexLike,
            source: "a=b".to_string(),
        },
    };
    let equation_id = inline_id(&equation).clone();
    base.blocks.push(Block {
        id: StableId::new("block"),
        kind: BlockKind::Paragraph,
        content: vec![equation],
        properties: BlockProperties::default(),
    });

    let result = merge_operations(
        &base,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::UpdateInlineEquationSource {
                inline_id: equation_id,
                source: "a=c".to_string(),
            },
            context: None,
        }]],
    )
    .unwrap();

    match &result.document.blocks[0].content[0] {
        Inline::Equation { equation, .. } => assert_eq!(equation.source, "a=c"),
        other => panic!("expected inline equation, got {other:?}"),
    }
    assert!(result.warnings.is_empty());
}

#[test]
fn empty_inline_equation_source_update_degrades_to_warning() {
    let mut base = Document::new("Doc");
    let equation = Inline::Equation {
        id: StableId::parse("inline-equation").unwrap(),
        equation: Equation {
            id: StableId::parse("equation-source").unwrap(),
            source_format: EquationSourceFormat::LatexLike,
            source: "a=b".to_string(),
        },
    };
    let equation_id = inline_id(&equation).clone();
    base.blocks.push(Block {
        id: StableId::new("block"),
        kind: BlockKind::Paragraph,
        content: vec![equation],
        properties: BlockProperties::default(),
    });

    let invalid = Operation {
        id: OperationId {
            actor: ActorId("a".to_string()),
            seq: 1,
        },
        kind: OperationKind::UpdateInlineEquationSource {
            inline_id: equation_id.clone(),
            source: " ".to_string(),
        },
        context: None,
    };
    let valid = Operation {
        id: OperationId {
            actor: ActorId("b".to_string()),
            seq: 1,
        },
        kind: OperationKind::UpdateInlineEquationSource {
            inline_id: equation_id,
            source: "a=c".to_string(),
        },
        context: None,
    };

    let actor_streams =
        merge_operations(&base, &[vec![invalid.clone()], vec![valid.clone()]]).unwrap();
    let reversed_batches = merge_operations(&base, &[vec![valid], vec![invalid]]).unwrap();

    assert_eq!(actor_streams.document, reversed_batches.document);
    assert_eq!(actor_streams.warnings, reversed_batches.warnings);
    match &actor_streams.document.blocks[0].content[0] {
        Inline::Equation { equation, .. } => assert_eq!(equation.source, "a=c"),
        other => panic!("expected inline equation, got {other:?}"),
    }
    assert_eq!(
        actor_streams.warnings[0].code,
        "invalid-inline-equation-source"
    );
}

#[test]
fn generic_inline_text_update_does_not_mutate_inline_equation_source() {
    let mut base = Document::new("Doc");
    let equation = Inline::Equation {
        id: StableId::parse("inline-equation").unwrap(),
        equation: Equation {
            id: StableId::parse("equation-source").unwrap(),
            source_format: EquationSourceFormat::LatexLike,
            source: "a=b".to_string(),
        },
    };
    let equation_id = inline_id(&equation).clone();
    base.blocks.push(Block {
        id: StableId::new("block"),
        kind: BlockKind::Paragraph,
        content: vec![equation],
        properties: BlockProperties::default(),
    });

    let result = merge_operations(
        &base,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::UpdateInlineText {
                inline_id: equation_id,
                text: "a=c".to_string(),
            },
            context: None,
        }]],
    )
    .unwrap();

    match &result.document.blocks[0].content[0] {
        Inline::Equation { equation, .. } => assert_eq!(equation.source, "a=b"),
        other => panic!("expected inline equation, got {other:?}"),
    }
    assert_eq!(result.warnings[0].code, "non-editable-inline");
}

#[test]
fn block_equation_source_updates_as_atomic_block_state() {
    let mut base = Document::new("Doc");
    let block_id = StableId::new("block");
    base.blocks.push(Block {
        id: block_id.clone(),
        kind: BlockKind::EquationBlock {
            equation: Equation {
                id: StableId::new("eq"),
                source_format: EquationSourceFormat::LatexLike,
                source: "x=1".to_string(),
            },
        },
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
            kind: OperationKind::UpdateBlockEquationSource {
                block_id,
                source: "x=2".to_string(),
            },
            context: None,
        }]],
    )
    .unwrap();

    assert_eq!(result.document.visible_text(), "x=2\n");
}

#[test]
fn structured_source_updates_are_canonicalized_during_merge() {
    let mut base = Document::new("Doc");
    let link_id = StableId::parse("link-canonical").unwrap();
    let mention_id = StableId::parse("mention-canonical").unwrap();
    let inline_equation_id = StableId::parse("inline-equation-canonical").unwrap();
    let block_equation_id = StableId::parse("block-equation-canonical").unwrap();
    base.blocks.push(Block {
        id: StableId::parse("paragraph-canonical").unwrap(),
        kind: BlockKind::Paragraph,
        content: vec![
            Inline::Link {
                id: link_id.clone(),
                text: "paper".to_string(),
                href: "https://example.invalid/old".to_string(),
                marks: Vec::new(),
            },
            Inline::Mention {
                id: mention_id.clone(),
                label: "@old".to_string(),
            },
            Inline::Equation {
                id: inline_equation_id.clone(),
                equation: Equation {
                    id: StableId::parse("equation-inline-canonical").unwrap(),
                    source_format: EquationSourceFormat::LatexLike,
                    source: "a=b".to_string(),
                },
            },
        ],
        properties: BlockProperties::default(),
    });
    base.blocks.push(Block {
        id: block_equation_id.clone(),
        kind: BlockKind::EquationBlock {
            equation: Equation {
                id: StableId::parse("equation-block-canonical").unwrap(),
                source_format: EquationSourceFormat::LatexLike,
                source: "x=1".to_string(),
            },
        },
        content: Vec::new(),
        properties: BlockProperties::default(),
    });

    let result = merge_operations(
        &base,
        &[vec![
            Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::UpdateLinkHref {
                    inline_id: link_id,
                    href: " https://example.invalid/new ".to_string(),
                },
                context: None,
            },
            Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 2,
                },
                kind: OperationKind::UpdateMentionLabel {
                    inline_id: mention_id,
                    label: " @new ".to_string(),
                },
                context: None,
            },
            Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 3,
                },
                kind: OperationKind::UpdateInlineEquationSource {
                    inline_id: inline_equation_id,
                    source: " c=d ".to_string(),
                },
                context: None,
            },
            Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 4,
                },
                kind: OperationKind::UpdateBlockEquationSource {
                    block_id: block_equation_id,
                    source: "\tx=2\n".to_string(),
                },
                context: None,
            },
        ]],
    )
    .unwrap();

    assert!(result.warnings.is_empty());
    match &result.document.blocks[0].content[0] {
        Inline::Link { href, .. } => assert_eq!(href, "https://example.invalid/new"),
        other => panic!("expected link inline, got {other:?}"),
    }
    match &result.document.blocks[0].content[1] {
        Inline::Mention { label, .. } => assert_eq!(label, "@new"),
        other => panic!("expected mention inline, got {other:?}"),
    }
    match &result.document.blocks[0].content[2] {
        Inline::Equation { equation, .. } => assert_eq!(equation.source, "c=d"),
        other => panic!("expected inline equation, got {other:?}"),
    }
    match &result.document.blocks[1].kind {
        BlockKind::EquationBlock { equation } => assert_eq!(equation.source, "x=2"),
        other => panic!("expected block equation, got {other:?}"),
    }
}

#[test]
fn empty_block_equation_source_update_degrades_to_warning() {
    let mut base = Document::new("Doc");
    let block_id = StableId::new("block");
    base.blocks.push(Block {
        id: block_id.clone(),
        kind: BlockKind::EquationBlock {
            equation: Equation {
                id: StableId::new("eq"),
                source_format: EquationSourceFormat::LatexLike,
                source: "x=1".to_string(),
            },
        },
        content: Vec::new(),
        properties: BlockProperties::default(),
    });

    let invalid = Operation {
        id: OperationId {
            actor: ActorId("a".to_string()),
            seq: 1,
        },
        kind: OperationKind::UpdateBlockEquationSource {
            block_id: block_id.clone(),
            source: "\t".to_string(),
        },
        context: None,
    };
    let valid = Operation {
        id: OperationId {
            actor: ActorId("b".to_string()),
            seq: 1,
        },
        kind: OperationKind::UpdateBlockEquationSource {
            block_id,
            source: "x=2".to_string(),
        },
        context: None,
    };

    let actor_streams =
        merge_operations(&base, &[vec![invalid.clone()], vec![valid.clone()]]).unwrap();
    let reversed_batches = merge_operations(&base, &[vec![valid], vec![invalid]]).unwrap();

    assert_eq!(actor_streams.document, reversed_batches.document);
    assert_eq!(actor_streams.warnings, reversed_batches.warnings);
    assert_eq!(actor_streams.document.visible_text(), "x=2\n");
    assert_eq!(
        actor_streams.warnings[0].code,
        "invalid-block-equation-source"
    );
}

#[test]
fn image_alt_text_updates_by_stable_block_id() {
    let mut base = Document::new("Doc");
    let block_id = StableId::new("block");
    base.blocks.push(Block {
        id: block_id.clone(),
        kind: BlockKind::Image {
            blob_hash: "sha256:abc".to_string(),
            alt_text: "Initial image".to_string(),
            layout: Default::default(),
        },
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
            kind: OperationKind::UpdateImageAltText {
                block_id,
                alt_text: "Updated image".to_string(),
            },
            context: None,
        }]],
    )
    .unwrap();

    assert_eq!(result.document.visible_text(), "Updated image\n");
    match &result.document.blocks[0].kind {
        BlockKind::Image {
            blob_hash,
            alt_text,
            ..
        } => {
            assert_eq!(blob_hash, "sha256:abc");
            assert_eq!(alt_text, "Updated image");
        }
        other => panic!("expected image block, got {other:?}"),
    }
}

#[test]
fn image_blob_hash_updates_keep_block_and_alt_text_stable() {
    let mut base = Document::new("Doc");
    let block_id = StableId::new("block");
    base.blocks.push(Block {
        id: block_id.clone(),
        kind: BlockKind::Image {
            blob_hash: "sha256:old".to_string(),
            alt_text: "Stable caption".to_string(),
            layout: Default::default(),
        },
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
            kind: OperationKind::UpdateImageBlobHash {
                block_id: block_id.clone(),
                blob_hash: "sha256:new".to_string(),
            },
            context: None,
        }]],
    )
    .unwrap();

    assert_eq!(result.document.blocks[0].id, block_id);
    assert_eq!(result.document.visible_text(), "Stable caption\n");
    match &result.document.blocks[0].kind {
        BlockKind::Image {
            blob_hash,
            alt_text,
            ..
        } => {
            assert_eq!(blob_hash, "sha256:new");
            assert_eq!(alt_text, "Stable caption");
        }
        other => panic!("expected image block, got {other:?}"),
    }
}

#[test]
fn invalid_image_blob_hash_update_degrades_to_warning() {
    let mut base = Document::new("Doc");
    let block_id = StableId::new("block");
    base.blocks.push(Block {
        id: block_id.clone(),
        kind: BlockKind::Image {
            blob_hash: "sha256:old".to_string(),
            alt_text: "Stable caption".to_string(),
            layout: Default::default(),
        },
        content: Vec::new(),
        properties: BlockProperties::default(),
    });

    let invalid = Operation {
        id: OperationId {
            actor: ActorId("a".to_string()),
            seq: 1,
        },
        kind: OperationKind::UpdateImageBlobHash {
            block_id: block_id.clone(),
            blob_hash: "not-a-hash".to_string(),
        },
        context: None,
    };
    let valid = Operation {
        id: OperationId {
            actor: ActorId("b".to_string()),
            seq: 1,
        },
        kind: OperationKind::UpdateImageBlobHash {
            block_id: block_id.clone(),
            blob_hash: "sha256:new".to_string(),
        },
        context: None,
    };

    let actor_streams =
        merge_operations(&base, &[vec![invalid.clone()], vec![valid.clone()]]).unwrap();
    let reversed_batches = merge_operations(&base, &[vec![valid], vec![invalid]]).unwrap();

    assert_eq!(actor_streams.document, reversed_batches.document);
    assert_eq!(actor_streams.warnings, reversed_batches.warnings);
    match &actor_streams.document.blocks[0].kind {
        BlockKind::Image { blob_hash, .. } => {
            assert_eq!(blob_hash, "sha256:new");
        }
        other => panic!("expected image block, got {other:?}"),
    }
    assert_eq!(actor_streams.warnings[0].code, "invalid-image-blob-hash");
}

#[test]
fn padded_image_blob_hash_update_degrades_without_changing_source() {
    let mut base = Document::new("Doc");
    let block_id = StableId::new("block");
    base.blocks.push(Block {
        id: block_id.clone(),
        kind: BlockKind::Image {
            blob_hash: "sha256:old".to_string(),
            alt_text: "Stable caption".to_string(),
            layout: Default::default(),
        },
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
            kind: OperationKind::UpdateImageBlobHash {
                block_id,
                blob_hash: " sha256:new ".to_string(),
            },
            context: None,
        }]],
    )
    .unwrap();

    match &result.document.blocks[0].kind {
        BlockKind::Image { blob_hash, .. } => {
            assert_eq!(blob_hash, "sha256:old");
        }
        other => panic!("expected image block, got {other:?}"),
    }
    assert_eq!(result.warnings[0].code, "invalid-image-blob-hash");
}

#[test]
fn image_blob_hash_update_stores_canonical_hash_reference() {
    let mut base = Document::new("Doc");
    let block_id = StableId::new("block");
    base.blocks.push(Block {
        id: block_id.clone(),
        kind: BlockKind::Image {
            blob_hash: "sha256:old".to_string(),
            alt_text: "Stable caption".to_string(),
            layout: Default::default(),
        },
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
            kind: OperationKind::UpdateImageBlobHash {
                block_id,
                blob_hash: "sha256:new".to_string(),
            },
            context: None,
        }]],
    )
    .unwrap();

    match &result.document.blocks[0].kind {
        BlockKind::Image { blob_hash, .. } => {
            assert_eq!(
                blob_hash,
                &HashRef::parse("sha256:new").unwrap().to_string()
            );
        }
        other => panic!("expected image block, got {other:?}"),
    }
    assert!(result.warnings.is_empty());
}

#[test]
fn image_delete_beats_stale_metadata_updates_without_resurrection() {
    let mut base = Document::new("Doc");
    let block_id = StableId::new("image-block");
    base.blocks.push(Block {
        id: block_id.clone(),
        kind: BlockKind::Image {
            blob_hash: "sha256:old".to_string(),
            alt_text: "Original caption".to_string(),
            layout: Default::default(),
        },
        content: Vec::new(),
        properties: BlockProperties::default(),
    });

    let delete = Operation {
        id: OperationId {
            actor: ActorId("a".to_string()),
            seq: 1,
        },
        kind: OperationKind::DeleteBlock {
            block_id: block_id.clone(),
        },
        context: None,
    };
    let stale_alt = Operation {
        id: OperationId {
            actor: ActorId("b".to_string()),
            seq: 1,
        },
        kind: OperationKind::UpdateImageAltText {
            block_id: block_id.clone(),
            alt_text: "Concurrent caption".to_string(),
        },
        context: None,
    };
    let stale_blob = Operation {
        id: OperationId {
            actor: ActorId("c".to_string()),
            seq: 1,
        },
        kind: OperationKind::UpdateImageBlobHash {
            block_id: block_id.clone(),
            blob_hash: "sha256:new".to_string(),
        },
        context: None,
    };

    let actor_streams = merge_operations(
        &base,
        &[
            vec![delete.clone()],
            vec![stale_alt.clone()],
            vec![stale_blob.clone()],
        ],
    )
    .unwrap();
    let reversed_batches =
        merge_operations(&base, &[vec![stale_blob], vec![stale_alt], vec![delete]]).unwrap();

    assert_eq!(actor_streams.document, reversed_batches.document);
    assert_eq!(actor_streams.warnings, reversed_batches.warnings);
    assert!(actor_streams.document.blocks.is_empty());
    assert_eq!(
        actor_streams
            .warnings
            .iter()
            .filter(|warning| warning.code == "missing-block")
            .count(),
        2
    );
    assert!(actor_streams
        .warnings
        .iter()
        .all(|warning| warning.message.contains(&block_id.to_string())));
}

#[test]
fn structured_inline_delete_beats_stale_source_updates_without_resurrection() {
    let mut base = Document::new("Doc");
    let link_id = StableId::parse("link-inline").unwrap();
    let mention_id = StableId::parse("mention-inline").unwrap();
    let equation_id = StableId::parse("equation-inline").unwrap();
    base.blocks.push(Block {
        id: StableId::parse("structured-inline-block").unwrap(),
        kind: BlockKind::Paragraph,
        content: vec![
            Inline::Link {
                id: link_id.clone(),
                text: "paper".to_string(),
                href: "https://example.invalid/old".to_string(),
                marks: Vec::new(),
            },
            Inline::Mention {
                id: mention_id.clone(),
                label: "@old".to_string(),
            },
            Inline::Equation {
                id: equation_id.clone(),
                equation: Equation {
                    id: StableId::parse("equation-source").unwrap(),
                    source_format: EquationSourceFormat::LatexLike,
                    source: "a=b".to_string(),
                },
            },
        ],
        properties: BlockProperties::default(),
    });

    let deletes = vec![
        Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::DeleteInline {
                inline_id: link_id.clone(),
            },
            context: None,
        },
        Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 2,
            },
            kind: OperationKind::DeleteInline {
                inline_id: mention_id.clone(),
            },
            context: None,
        },
        Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 3,
            },
            kind: OperationKind::DeleteInline {
                inline_id: equation_id.clone(),
            },
            context: None,
        },
    ];
    let stale_updates = vec![
        Operation {
            id: OperationId {
                actor: ActorId("b".to_string()),
                seq: 1,
            },
            kind: OperationKind::UpdateLinkHref {
                inline_id: link_id.clone(),
                href: "https://example.invalid/new".to_string(),
            },
            context: None,
        },
        Operation {
            id: OperationId {
                actor: ActorId("c".to_string()),
                seq: 1,
            },
            kind: OperationKind::UpdateMentionLabel {
                inline_id: mention_id.clone(),
                label: "@new".to_string(),
            },
            context: None,
        },
        Operation {
            id: OperationId {
                actor: ActorId("d".to_string()),
                seq: 1,
            },
            kind: OperationKind::UpdateInlineEquationSource {
                inline_id: equation_id.clone(),
                source: "a=c".to_string(),
            },
            context: None,
        },
    ];

    let actor_streams = merge_operations(&base, &[deletes.clone(), stale_updates.clone()]).unwrap();
    let reversed_batches = merge_operations(&base, &[stale_updates, deletes]).unwrap();

    assert_eq!(actor_streams.document, reversed_batches.document);
    assert_eq!(actor_streams.warnings, reversed_batches.warnings);
    assert!(actor_streams.document.blocks[0].content.is_empty());
    assert_eq!(
        actor_streams
            .warnings
            .iter()
            .filter(|warning| warning.code == "missing-inline")
            .count(),
        3
    );
    let warning_text = actor_streams
        .warnings
        .iter()
        .map(|warning| warning.message.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(warning_text.contains(link_id.as_str()));
    assert!(warning_text.contains(mention_id.as_str()));
    assert!(warning_text.contains(equation_id.as_str()));
}
