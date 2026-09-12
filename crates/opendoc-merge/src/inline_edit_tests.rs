//! Inline edit tests.

use crate::causal::{ActorId, OperationId};
use crate::inline_ops::inline_id;
use crate::merge::merge_operations;
use crate::operation::{Operation, OperationKind};
use opendoc_core::{
    Block, BlockKind, BlockProperties, CellSpan, Document, Equation, EquationSourceFormat, Inline,
    ListKind, Mark, MarkExpand, MarkKind, StableId,
};

#[test]
fn inline_text_updates_preserve_marks_and_reach_table_cells() {
    let mut base = Document::new("Doc");
    let paragraph = Block::paragraph("before");
    let paragraph_text_id = match &paragraph.content[0] {
        Inline::Text { id, .. } => id.clone(),
        _ => unreachable!(),
    };
    let table_text = Inline::text("cell");
    let table_text_id = inline_id(&table_text).clone();
    base.blocks.push(paragraph);
    base.blocks.push(Block {
        id: StableId::new("block"),
        kind: BlockKind::table(vec![opendoc_core::TableRow {
            id: StableId::new("row"),
            cells: vec![opendoc_core::TableCell {
                id: StableId::new("cell"),
                span: CellSpan::SINGLE,
                properties: Default::default(),
                blocks: vec![Block {
                    id: StableId::new("block"),
                    kind: BlockKind::Paragraph,
                    content: vec![table_text],
                    properties: BlockProperties::default(),
                }],
            }],
        }]),
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
                kind: OperationKind::AddMark {
                    text_id: paragraph_text_id.clone(),
                    mark: Mark {
                        kind: MarkKind::Bold,
                        value: None,
                        expand: MarkExpand::Both,
                    },
                },
                context: None,
            },
            Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 2,
                },
                kind: OperationKind::UpdateInlineText {
                    inline_id: paragraph_text_id,
                    text: "after".to_string(),
                },
                context: None,
            },
            Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 3,
                },
                kind: OperationKind::UpdateInlineText {
                    inline_id: table_text_id,
                    text: "edited cell".to_string(),
                },
                context: None,
            },
        ]],
    )
    .unwrap();

    assert!(result.document.visible_text().contains("after"));
    assert!(result.document.visible_text().contains("edited cell"));
    match &result.document.blocks[0].content[0] {
        Inline::Text { marks, .. } => assert_eq!(marks.len(), 1),
        _ => unreachable!(),
    }
}

#[test]
fn character_level_text_operations_merge_without_losing_text() {
    let mut base = Document::new("Doc");
    let text_id = StableId::parse("text-1").unwrap();
    base.blocks.push(Block {
        id: StableId::parse("block-1").unwrap(),
        kind: BlockKind::Paragraph,
        content: vec![Inline::Text {
            id: text_id.clone(),
            text: "Hello world".to_string(),
            marks: Vec::new(),
        }],
        properties: BlockProperties::default(),
    });
    let op = |actor: &str, seq: u64, kind: OperationKind| Operation {
        id: OperationId {
            actor: ActorId(actor.to_string()),
            seq,
        },
        kind,
        context: None,
    };
    // Two actors edit the same run concurrently: one inserts, one deletes.
    let result = merge_operations(
        &base,
        &[
            vec![op(
                "alice",
                1,
                OperationKind::InsertText {
                    inline_id: text_id.clone(),
                    offset: 5,
                    text: " brave".to_string(),
                },
            )],
            vec![op(
                "bob",
                1,
                OperationKind::DeleteText {
                    inline_id: text_id.clone(),
                    start: 0,
                    end: 5,
                },
            )],
        ],
    )
    .unwrap();
    let text = result.document.visible_text();
    assert!(text.contains("brave"), "{text}");
    assert!(text.contains("world"), "{text}");
    assert!(result.warnings.is_empty(), "{:?}", result.warnings);

    // Offsets past the end are clamped instead of panicking; unicode
    // offsets count scalar values, not bytes.
    let result = merge_operations(
        &base,
        &[vec![
            op(
                "alice",
                1,
                OperationKind::InsertText {
                    inline_id: text_id.clone(),
                    offset: 5,
                    text: " héllo".to_string(),
                },
            ),
            op(
                "alice",
                2,
                OperationKind::DeleteText {
                    inline_id: text_id.clone(),
                    start: 7,
                    end: 999,
                },
            ),
        ]],
    )
    .unwrap();
    assert_eq!(result.document.visible_text().trim_end(), "Hello h");

    // Editing a derived inline is refused with a warning, missing ones warn.
    let mut derived = Document::new("Doc");
    derived.blocks.push(Block {
        id: StableId::parse("block-2").unwrap(),
        kind: BlockKind::Paragraph,
        content: vec![Inline::Mention {
            id: StableId::parse("mention-1").unwrap(),
            label: "@someone".to_string(),
        }],
        properties: BlockProperties::default(),
    });
    let result = merge_operations(
        &derived,
        &[vec![
            op(
                "alice",
                1,
                OperationKind::InsertText {
                    inline_id: StableId::parse("mention-1").unwrap(),
                    offset: 0,
                    text: "x".to_string(),
                },
            ),
            op(
                "alice",
                2,
                OperationKind::DeleteText {
                    inline_id: StableId::parse("nope").unwrap(),
                    start: 0,
                    end: 1,
                },
            ),
        ]],
    )
    .unwrap();
    assert_eq!(result.warnings.len(), 2);
}

#[test]
fn link_href_updates_are_operation_backed() {
    let mut base = Document::new("Doc");
    let link_id = StableId::parse("link-1").unwrap();
    base.blocks.push(Block {
        id: StableId::parse("block-1").unwrap(),
        kind: BlockKind::Paragraph,
        content: vec![Inline::Link {
            id: link_id.clone(),
            text: "paper".to_string(),
            href: "https://example.invalid/old".to_string(),
            marks: Vec::new(),
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
            kind: OperationKind::UpdateLinkHref {
                inline_id: link_id,
                href: "https://example.invalid/new".to_string(),
            },
            context: None,
        }]],
    )
    .unwrap();

    match &result.document.blocks[0].content[0] {
        Inline::Link { href, .. } => assert_eq!(href, "https://example.invalid/new"),
        _ => unreachable!(),
    }
    assert!(result.warnings.is_empty());
}

#[test]
fn empty_link_href_update_degrades_to_warning() {
    let mut base = Document::new("Doc");
    let link_id = StableId::parse("link-1").unwrap();
    base.blocks.push(Block {
        id: StableId::parse("block-1").unwrap(),
        kind: BlockKind::Paragraph,
        content: vec![Inline::Link {
            id: link_id.clone(),
            text: "paper".to_string(),
            href: "https://example.invalid/old".to_string(),
            marks: Vec::new(),
        }],
        properties: BlockProperties::default(),
    });

    let invalid = Operation {
        id: OperationId {
            actor: ActorId("a".to_string()),
            seq: 1,
        },
        kind: OperationKind::UpdateLinkHref {
            inline_id: link_id.clone(),
            href: " ".to_string(),
        },
        context: None,
    };
    let valid = Operation {
        id: OperationId {
            actor: ActorId("b".to_string()),
            seq: 1,
        },
        kind: OperationKind::UpdateLinkHref {
            inline_id: link_id,
            href: "https://example.invalid/new".to_string(),
        },
        context: None,
    };

    let actor_streams =
        merge_operations(&base, &[vec![invalid.clone()], vec![valid.clone()]]).unwrap();
    let reversed_batches = merge_operations(&base, &[vec![valid], vec![invalid]]).unwrap();

    assert_eq!(actor_streams.document, reversed_batches.document);
    assert_eq!(actor_streams.warnings, reversed_batches.warnings);
    match &actor_streams.document.blocks[0].content[0] {
        Inline::Link { href, .. } => assert_eq!(href, "https://example.invalid/new"),
        _ => unreachable!(),
    }
    assert_eq!(actor_streams.warnings[0].code, "invalid-link-href");
    actor_streams.document.validate().unwrap();
}

#[test]
fn link_href_update_warns_for_non_link_inline() {
    let mut base = Document::new("Doc");
    let text = Inline::text("not a link");
    let text_id = inline_id(&text).clone();
    base.blocks.push(Block {
        id: StableId::parse("block-1").unwrap(),
        kind: BlockKind::Paragraph,
        content: vec![text],
        properties: BlockProperties::default(),
    });

    let result = merge_operations(
        &base,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::UpdateLinkHref {
                inline_id: text_id,
                href: "https://example.invalid/new".to_string(),
            },
            context: None,
        }]],
    )
    .unwrap();

    assert_eq!(result.warnings[0].code, "non-link-inline");
    assert_eq!(result.document.visible_text(), "not a link\n");
}

#[test]
fn list_item_properties_update_by_stable_block_id() {
    let mut base = Document::new("Doc");
    let block_id = StableId::parse("list-1").unwrap();
    base.blocks.push(Block {
        id: block_id.clone(),
        kind: BlockKind::ListItem {
            list_id: StableId::parse("list-1-id").unwrap(),
            level: 0,
            kind: ListKind::Bullet,
        },
        content: vec![Inline::text("item")],
        properties: BlockProperties::default(),
    });

    let result = merge_operations(
        &base,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::UpdateListItem {
                block_id,
                level: 2,
                kind: ListKind::Ordered,
            },
            context: None,
        }]],
    )
    .unwrap();

    match &result.document.blocks[0].kind {
        BlockKind::ListItem { level, kind, .. } => {
            assert_eq!((*level, *kind), (2, ListKind::Ordered));
        }
        _ => unreachable!(),
    }
    assert!(result.warnings.is_empty());
}

#[test]
fn list_item_update_warns_for_non_list_block() {
    let mut base = Document::new("Doc");
    let block = Block::paragraph("not list");
    let block_id = block.id.clone();
    base.blocks.push(block);

    let result = merge_operations(
        &base,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::UpdateListItem {
                block_id,
                level: 1,
                kind: ListKind::Ordered,
            },
            context: None,
        }]],
    )
    .unwrap();

    assert_eq!(result.warnings[0].code, "non-list-item-block");
    assert_eq!(result.document.visible_text(), "not list\n");
}

#[test]
fn invalid_list_item_level_update_degrades_to_warning() {
    let mut base = Document::new("Doc");
    let block_id = StableId::parse("list-1").unwrap();
    base.blocks.push(Block {
        id: block_id.clone(),
        kind: BlockKind::ListItem {
            list_id: StableId::parse("list-1-id").unwrap(),
            level: 0,
            kind: ListKind::Bullet,
        },
        content: vec![Inline::text("item")],
        properties: BlockProperties::default(),
    });

    let invalid = Operation {
        id: OperationId {
            actor: ActorId("a".to_string()),
            seq: 1,
        },
        kind: OperationKind::UpdateListItem {
            block_id: block_id.clone(),
            level: 9,
            kind: ListKind::Ordered,
        },
        context: None,
    };
    let valid = Operation {
        id: OperationId {
            actor: ActorId("b".to_string()),
            seq: 1,
        },
        kind: OperationKind::UpdateListItem {
            block_id,
            level: 2,
            kind: ListKind::Ordered,
        },
        context: None,
    };

    let actor_streams =
        merge_operations(&base, &[vec![invalid.clone()], vec![valid.clone()]]).unwrap();
    let reversed_batches = merge_operations(&base, &[vec![valid], vec![invalid]]).unwrap();

    assert_eq!(actor_streams.document, reversed_batches.document);
    assert_eq!(actor_streams.warnings, reversed_batches.warnings);
    match &actor_streams.document.blocks[0].kind {
        BlockKind::ListItem { level, kind, .. } => {
            assert_eq!((*level, *kind), (2, ListKind::Ordered));
        }
        _ => unreachable!(),
    }
    assert_eq!(actor_streams.warnings[0].code, "invalid-list-level");
    actor_streams.document.validate().unwrap();
}

#[test]
fn invalid_inserted_structured_block_payloads_degrade_to_warnings() {
    let mut base = Document::new("Doc");
    base.blocks.push(Block::paragraph("base"));

    let invalid_blocks = vec![
        Block {
            id: StableId::parse("heading-bad").unwrap(),
            kind: BlockKind::Heading { level: 0 },
            content: vec![Inline::text("bad heading")],
            properties: BlockProperties::default(),
        },
        Block {
            id: StableId::parse("list-bad").unwrap(),
            kind: BlockKind::ListItem {
                list_id: StableId::parse("list-bad-id").unwrap(),
                level: 9,
                kind: ListKind::Bullet,
            },
            content: vec![Inline::text("bad list")],
            properties: BlockProperties::default(),
        },
        Block {
            id: StableId::parse("image-bad").unwrap(),
            kind: BlockKind::Image {
                blob_hash: "not-a-hash".to_string(),
                alt_text: "bad image".to_string(),
                layout: Default::default(),
            },
            content: Vec::new(),
            properties: BlockProperties::default(),
        },
        Block {
            id: StableId::parse("equation-block-bad").unwrap(),
            kind: BlockKind::EquationBlock {
                equation: Equation {
                    id: StableId::parse("equation-bad").unwrap(),
                    source_format: EquationSourceFormat::LatexLike,
                    source: String::new(),
                },
            },
            content: Vec::new(),
            properties: BlockProperties::default(),
        },
        Block {
            id: StableId::parse("link-bad").unwrap(),
            kind: BlockKind::Paragraph,
            content: vec![Inline::Link {
                id: StableId::parse("link-empty").unwrap(),
                text: "bad link".to_string(),
                href: String::new(),
                marks: Vec::new(),
            }],
            properties: BlockProperties::default(),
        },
        Block {
            id: StableId::parse("mention-bad").unwrap(),
            kind: BlockKind::Paragraph,
            content: vec![Inline::Mention {
                id: StableId::parse("mention-empty").unwrap(),
                label: " ".to_string(),
            }],
            properties: BlockProperties::default(),
        },
        Block {
            id: StableId::parse("inline-equation-bad-block").unwrap(),
            kind: BlockKind::Paragraph,
            content: vec![Inline::Equation {
                id: StableId::parse("inline-equation-empty").unwrap(),
                equation: Equation {
                    id: StableId::parse("inline-equation-bad").unwrap(),
                    source_format: EquationSourceFormat::LatexLike,
                    source: String::new(),
                },
            }],
            properties: BlockProperties::default(),
        },
        Block {
            id: StableId::parse("table-empty").unwrap(),
            kind: BlockKind::table(Vec::new()),
            content: Vec::new(),
            properties: BlockProperties::default(),
        },
    ];

    let operations = invalid_blocks
        .into_iter()
        .enumerate()
        .map(|(index, block)| Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: index as u64 + 1,
            },
            kind: OperationKind::InsertBlock { after: None, block },
            context: None,
        })
        .collect::<Vec<_>>();

    let result = merge_operations(&base, &[operations]).unwrap();
    let warning_codes = result
        .warnings
        .iter()
        .map(|warning| warning.code.as_str())
        .collect::<Vec<_>>();

    assert_eq!(result.document.visible_text(), "base\n");
    assert_eq!(
        warning_codes,
        vec![
            "invalid-heading-level",
            "invalid-list-level",
            "invalid-image-blob-hash",
            "invalid-block-equation-source",
            "invalid-link-href",
            "invalid-mention-label",
            "invalid-inline-equation-source",
            "invalid-table",
        ]
    );
    result.document.validate().unwrap();
}

#[test]
fn invalid_inserted_inline_payloads_degrade_to_warnings() {
    let mut base = Document::new("Doc");
    let block = Block::paragraph("base");
    let block_id = block.id.clone();
    base.blocks.push(block);

    let invalid_inlines = vec![
        Inline::Link {
            id: StableId::parse("link-empty").unwrap(),
            text: "bad link".to_string(),
            href: String::new(),
            marks: Vec::new(),
        },
        Inline::Mention {
            id: StableId::parse("mention-empty").unwrap(),
            label: " ".to_string(),
        },
        Inline::Equation {
            id: StableId::parse("inline-equation-empty").unwrap(),
            equation: Equation {
                id: StableId::parse("equation-empty").unwrap(),
                source_format: EquationSourceFormat::LatexLike,
                source: String::new(),
            },
        },
        Inline::Text {
            id: StableId::parse("color-missing").unwrap(),
            text: "bad color".to_string(),
            marks: vec![Mark {
                kind: MarkKind::Color,
                value: None,
                expand: MarkExpand::Both,
            }],
        },
        Inline::Text {
            id: StableId::parse("bold-valued").unwrap(),
            text: "bad bold".to_string(),
            marks: vec![Mark {
                kind: MarkKind::Bold,
                value: Some("true".to_string()),
                expand: MarkExpand::Both,
            }],
        },
    ];

    let operations = invalid_inlines
        .into_iter()
        .enumerate()
        .map(|(index, inline)| Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: index as u64 + 1,
            },
            kind: OperationKind::InsertInline {
                block_id: block_id.clone(),
                after: None,
                inline,
            },
            context: None,
        })
        .collect::<Vec<_>>();

    let result = merge_operations(&base, &[operations]).unwrap();
    let warning_codes = result
        .warnings
        .iter()
        .map(|warning| warning.code.as_str())
        .collect::<Vec<_>>();

    assert_eq!(result.document.visible_text(), "base\n");
    assert_eq!(
        warning_codes,
        vec![
            "invalid-link-href",
            "invalid-mention-label",
            "invalid-inline-equation-source",
            "invalid-mark-value",
            "invalid-mark-value",
        ]
    );
    result.document.validate().unwrap();
}

#[test]
fn heading_level_updates_by_stable_block_id() {
    let mut base = Document::new("Doc");
    let block_id = StableId::parse("heading-1").unwrap();
    base.blocks.push(Block {
        id: block_id.clone(),
        kind: BlockKind::Heading { level: 2 },
        content: vec![Inline::text("Heading")],
        properties: BlockProperties::default(),
    });

    let result = merge_operations(
        &base,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::UpdateHeadingLevel { block_id, level: 4 },
            context: None,
        }]],
    )
    .unwrap();

    match &result.document.blocks[0].kind {
        BlockKind::Heading { level } => assert_eq!(*level, 4),
        _ => unreachable!(),
    }
    assert!(result.warnings.is_empty());
}

#[test]
fn invalid_heading_level_update_degrades_to_warning() {
    let mut base = Document::new("Doc");
    let block_id = StableId::parse("heading-1").unwrap();
    base.blocks.push(Block {
        id: block_id.clone(),
        kind: BlockKind::Heading { level: 2 },
        content: vec![Inline::text("Heading")],
        properties: BlockProperties::default(),
    });

    let invalid = Operation {
        id: OperationId {
            actor: ActorId("a".to_string()),
            seq: 1,
        },
        kind: OperationKind::UpdateHeadingLevel {
            block_id: block_id.clone(),
            level: 0,
        },
        context: None,
    };
    let valid = Operation {
        id: OperationId {
            actor: ActorId("b".to_string()),
            seq: 1,
        },
        kind: OperationKind::UpdateHeadingLevel { block_id, level: 4 },
        context: None,
    };

    let actor_streams =
        merge_operations(&base, &[vec![invalid.clone()], vec![valid.clone()]]).unwrap();
    let reversed_batches = merge_operations(&base, &[vec![valid], vec![invalid]]).unwrap();

    assert_eq!(actor_streams.document, reversed_batches.document);
    assert_eq!(actor_streams.warnings, reversed_batches.warnings);
    match &actor_streams.document.blocks[0].kind {
        BlockKind::Heading { level } => assert_eq!(*level, 4),
        _ => unreachable!(),
    }
    assert_eq!(actor_streams.warnings[0].code, "invalid-heading-level");
    actor_streams.document.validate().unwrap();
}

#[test]
fn heading_level_update_warns_for_non_heading_block() {
    let mut base = Document::new("Doc");
    let block = Block::paragraph("not heading");
    let block_id = block.id.clone();
    base.blocks.push(block);

    let result = merge_operations(
        &base,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::UpdateHeadingLevel { block_id, level: 3 },
            context: None,
        }]],
    )
    .unwrap();

    assert_eq!(result.warnings[0].code, "non-heading-block");
    assert_eq!(result.document.visible_text(), "not heading\n");
}
