//! Operation tests.

use crate::causal::{ActorId, OperationId};
use crate::inline_ops::inline_id;
use crate::inverse::{invert_operation, Inversion};
use crate::merge::merge_operations;
use crate::operation::{Operation, OperationKind};
use crate::test_support::assert_mark_kinds;
use opendoc_core::{
    Block, BlockKind, BlockProperties, Bookmark, Document, Footnote, ImageLayout, Inline,
    InsertPosition, Mark, MarkExpand, MarkKind, PositionedImage, PositionedImageAnchor,
    PositionedImageLayer, Section, StableId, TextRange,
};

#[test]
fn concurrent_operations_converge_independent_of_stream_order() {
    let mut base = Document::new("Doc");
    let block = Block::paragraph("hello");
    let block_id = block.id.clone();
    let text_id = match &block.content[0] {
        Inline::Text { id, .. } => id.clone(),
        _ => unreachable!(),
    };
    base.blocks.push(block);

    let op_a = Operation {
        id: OperationId {
            actor: ActorId("a".to_string()),
            seq: 1,
        },
        kind: OperationKind::InsertInline {
            block_id: block_id.clone(),
            position: InsertPosition::After(text_id.clone()),
            inline: Inline::text(" world"),
        },
        context: None,
    };
    let op_b = Operation {
        id: OperationId {
            actor: ActorId("b".to_string()),
            seq: 1,
        },
        kind: OperationKind::AddMark {
            text_id,
            mark: Mark {
                kind: MarkKind::Bold,
                value: None,
                expand: MarkExpand::Both,
            },
        },
        context: None,
    };
    let merged_ab = merge_operations(&base, &[vec![op_a.clone()], vec![op_b.clone()]]).unwrap();
    let merged_ba = merge_operations(&base, &[vec![op_b], vec![op_a]]).unwrap();
    assert_eq!(
        merged_ab.document.visible_text(),
        merged_ba.document.visible_text()
    );
    // The oracle. Agreement between two groupings of the same operation set is
    // true by construction — the merge de-duplicates into one `BTreeMap`
    // before any semantics run — so this test passed for a `merge_operations`
    // that dropped every operation and returned the base. Name the answer
    // instead: both operations landed, and they landed on the right inline.
    // PLAN88 §7.
    assert_eq!(merged_ab.document.visible_text(), "hello world\n");
    assert_eq!(merged_ab.warnings, Vec::new());
    let block = &merged_ab.document.blocks[0];
    assert_eq!(block.content.len(), 2, "{:?}", block.content);
    assert_mark_kinds(&block.content[0], &[MarkKind::Bold]);
    assert_mark_kinds(&block.content[1], &[]);
}

#[test]
fn generic_block_operations_cannot_separate_a_section_from_its_record() {
    let mut base = Document::new("Sections");
    let before = Block::paragraph("before");
    let after = Block::paragraph("after");
    let boundary_id = StableId::parse("section-break").unwrap();
    let section_id = StableId::parse("section-two").unwrap();
    base.blocks = vec![
        before,
        Block {
            id: boundary_id.clone(),
            kind: BlockKind::SectionBreak {
                section_id: section_id.clone(),
            },
            content: Vec::new(),
            properties: BlockProperties::default(),
        },
        after,
    ];
    // Materialize before adding the later boundary, exactly as section
    // authoring will do. This makes the base a valid signed source document.
    base.blocks.remove(1);
    base.materialize_legacy_sections().unwrap();
    base.sections.insert(
        section_id.clone(),
        Section {
            id: section_id.clone(),
            page_setup: Default::default(),
            header: Vec::new(),
            footer: Vec::new(),
            first_page_header: None,
            first_page_footer: None,
            even_page_header: None,
            even_page_footer: None,
        },
    );
    base.blocks.insert(
        1,
        Block {
            id: boundary_id.clone(),
            kind: BlockKind::SectionBreak { section_id },
            content: Vec::new(),
            properties: BlockProperties::default(),
        },
    );
    base.validate().unwrap();

    let delete = Operation {
        id: OperationId {
            actor: ActorId("a".to_string()),
            seq: 1,
        },
        kind: OperationKind::DeleteBlock {
            block_id: boundary_id.clone(),
        },
        context: None,
    };
    let move_boundary = Operation {
        id: OperationId {
            actor: ActorId("b".to_string()),
            seq: 1,
        },
        kind: OperationKind::MoveBlock {
            block_id: boundary_id.clone(),
            position: InsertPosition::First,
        },
        context: None,
    };
    let merged = merge_operations(&base, &[vec![delete], vec![move_boundary]]).unwrap();
    assert_eq!(merged.document.blocks, base.blocks);
    assert_eq!(merged.document.sections, base.sections);
    assert_eq!(
        merged
            .warnings
            .iter()
            .filter(|warning| warning.code == "section-break-requires-section-operation")
            .count(),
        2
    );
}

#[test]
fn section_insert_delete_and_inverse_keep_the_boundary_and_record_atomic() {
    let mut base = Document::new("Section operation");
    let first = Block::paragraph("first");
    let second = Block::paragraph("second");
    let second_id = second.id.clone();
    base.blocks = vec![first, second];
    let section_id = StableId::parse("section-two").unwrap();
    let section = Section {
        id: section_id.clone(),
        page_setup: Default::default(),
        header: Vec::new(),
        footer: Vec::new(),
        first_page_header: None,
        first_page_footer: None,
        even_page_header: None,
        even_page_footer: None,
    };
    let insert_kind = OperationKind::InsertSection {
        before_block_id: second_id,
        boundary_id: StableId::parse("section-break").unwrap(),
        section: section.clone(),
    };
    let inserted = merge_operations(
        &base,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("author".to_string()),
                seq: 1,
            },
            kind: insert_kind.clone(),
            context: None,
        }]],
    )
    .unwrap();
    inserted.document.validate().unwrap();
    assert_eq!(inserted.document.sections.get(&section_id), Some(&section));
    assert!(matches!(
        inserted.document.blocks[1].kind,
        BlockKind::SectionBreak { section_id: ref id } if id == &section_id
    ));
    assert_eq!(
        invert_operation(&base, &insert_kind),
        Inversion::Operations(vec![OperationKind::DeleteSection {
            section_id: section_id.clone(),
        }])
    );

    let delete_kind = OperationKind::DeleteSection {
        section_id: section_id.clone(),
    };
    let inverse = invert_operation(&inserted.document, &delete_kind);
    assert_eq!(
        inverse,
        Inversion::Operations(vec![insert_kind.clone()]),
        "delete inverse restores the exact section and boundary identity"
    );
    let deleted = merge_operations(
        &inserted.document,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("author".to_string()),
                seq: 2,
            },
            kind: delete_kind,
            context: None,
        }]],
    )
    .unwrap();
    assert!(deleted.document.sections.len() == 1);
    assert_eq!(deleted.document.blocks, base.blocks);
    deleted.document.validate().unwrap();
}

#[test]
fn section_configuration_writes_are_scoped_and_keep_override_semantics() {
    let base = Document::new("Section configuration");
    let root_id = base.root_section_id();
    let mut a4 = opendoc_core::PageSetup::default();
    a4.width = opendoc_core::Length::from_twips(11906).unwrap();
    a4.height = opendoc_core::Length::from_twips(16838).unwrap();
    let header = Block::paragraph("first-page header");
    let setup = Operation {
        id: OperationId {
            actor: ActorId("author".to_string()),
            seq: 1,
        },
        kind: OperationKind::SetSectionPageSetup {
            section_id: root_id.clone(),
            page_setup: a4,
        },
        context: None,
    };
    let furniture = Operation {
        id: OperationId {
            actor: ActorId("author".to_string()),
            seq: 2,
        },
        kind: OperationKind::SetSectionFurniture {
            section_id: root_id.clone(),
            slot: opendoc_core::HeaderFooterSlot::FirstPageHeader,
            blocks: vec![header.clone()],
        },
        context: None,
    };
    let configured = merge_operations(&base, &[vec![setup], vec![furniture]]).unwrap();
    let root = &configured.document.sections[&root_id];
    assert_eq!(root.page_setup, a4);
    assert_eq!(
        root.first_page_header.as_deref(),
        Some(std::slice::from_ref(&header))
    );
    assert_eq!(
        invert_operation(
            &configured.document,
            &OperationKind::ClearSectionFurnitureOverride {
                section_id: root_id.clone(),
                slot: opendoc_core::HeaderFooterSlot::FirstPageHeader,
            },
        ),
        Inversion::Operations(vec![OperationKind::SetSectionFurniture {
            section_id: root_id.clone(),
            slot: opendoc_core::HeaderFooterSlot::FirstPageHeader,
            blocks: vec![header],
        }])
    );
    let cleared = merge_operations(
        &configured.document,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("author".to_string()),
                seq: 3,
            },
            kind: OperationKind::ClearSectionFurnitureOverride {
                section_id: root_id.clone(),
                slot: opendoc_core::HeaderFooterSlot::FirstPageHeader,
            },
            context: None,
        }]],
    )
    .unwrap();
    assert!(cleared.document.sections[&root_id]
        .first_page_header
        .is_none());
    cleared.document.validate().unwrap();
}

#[test]
fn tombstoning_an_endnote_also_removes_its_invalid_placement() {
    let mut base = Document::new("Notes");
    let note_id = StableId::parse("note-one").unwrap();
    base.footnotes.push(Footnote {
        id: note_id.clone(),
        revision: 1,
        body: vec![Inline::text("A note")],
        deleted: false,
    });
    base.endnote_ids.insert(note_id.clone());
    base.validate().expect("valid live endnote");

    let tombstone = Operation {
        id: OperationId {
            actor: ActorId("reviewer".to_string()),
            seq: 1,
        },
        kind: OperationKind::UpsertFootnote {
            footnote: Footnote {
                id: note_id.clone(),
                revision: 2,
                body: vec![Inline::text("A note")],
                deleted: true,
            },
        },
        context: None,
    };

    let merged = merge_operations(&base, &[vec![tombstone]]).expect("merge tombstone");
    assert!(!merged.document.endnote_ids.contains(&note_id));
    merged
        .document
        .validate()
        .expect("tombstone cannot leave an invalid endnote placement");
}

#[test]
fn concurrent_same_name_bookmarks_choose_the_later_operation_and_keep_a_tombstone() {
    let mut base = Document::new("Bookmarks");
    let target = Block::paragraph("target");
    let target_id = target.id.clone();
    base.blocks.push(target);
    let bookmark = |actor: &str, id: &str| Operation {
        id: OperationId {
            actor: ActorId(actor.to_string()),
            seq: 1,
        },
        kind: OperationKind::UpsertBookmark {
            bookmark: Bookmark {
                id: StableId::parse(id).unwrap(),
                name: "introduction".to_string(),
                block_id: target_id.clone(),
                revision: 1,
                deleted: false,
            },
        },
        context: None,
    };
    let alpha = bookmark("alpha", "bookmark-alpha");
    let zeta = bookmark("zeta", "bookmark-zeta");
    let ab = merge_operations(&base, &[vec![alpha.clone()], vec![zeta.clone()]]).unwrap();
    let ba = merge_operations(&base, &[vec![zeta], vec![alpha]]).unwrap();
    assert_eq!(ab.document.bookmarks, ba.document.bookmarks);
    let live = ab
        .document
        .bookmarks
        .iter()
        .filter(|item| !item.deleted)
        .collect::<Vec<_>>();
    assert_eq!(live.len(), 1);
    assert_eq!(live[0].id, StableId::parse("bookmark-zeta").unwrap());
    ab.document.validate().unwrap();
}

#[test]
fn concurrent_positioned_image_moves_choose_one_whole_anchor_geometry_and_layer() {
    let mut base = Document::new("Doc");
    let anchor = Block::paragraph("anchor");
    let anchor_id = anchor.id.clone();
    let image_id = StableId::parse("positioned-image").unwrap();
    base.blocks.push(anchor);
    base.blocks.push(Block {
        id: image_id.clone(),
        kind: BlockKind::Image {
            blob_hash: format!("sha256:{}", "a".repeat(64)),
            alt_text: "diagram".to_string(),
            layout: ImageLayout::default(),
        },
        content: Vec::new(),
        properties: BlockProperties::default(),
    });
    let moved = |actor: &str, x, y, layer| Operation {
        id: OperationId {
            actor: ActorId(actor.to_string()),
            seq: 1,
        },
        kind: OperationKind::UpdateImageLayout {
            block_id: image_id.clone(),
            layout: ImageLayout {
                positioned: Some(PositionedImage {
                    anchor: PositionedImageAnchor::Block(anchor_id.clone()),
                    horizontal_offset: opendoc_core::Length::from_twips(x).unwrap(),
                    vertical_offset: opendoc_core::Length::from_twips(y).unwrap(),
                    layer,
                }),
                ..ImageLayout::default()
            },
        },
        context: None,
    };
    let a = moved("alice", -240, 480, PositionedImageLayer::BehindText);
    let b = moved("zoe", 960, -720, PositionedImageLayer::InFrontOfText);
    let result = merge_operations(&base, &[vec![a], vec![b]]).unwrap();
    let layout = match &result.document.blocks[1].kind {
        BlockKind::Image { layout, .. } => layout,
        _ => unreachable!(),
    };
    assert_eq!(
        layout.positioned,
        Some(PositionedImage {
            anchor: PositionedImageAnchor::Block(anchor_id),
            horizontal_offset: opendoc_core::Length::from_twips(960).unwrap(),
            vertical_offset: opendoc_core::Length::from_twips(-720).unwrap(),
            layer: PositionedImageLayer::InFrontOfText,
        }),
        "the winner is one author-selected position, never a field-wise hybrid"
    );
}

#[test]
fn document_title_updates_converge_and_reject_empty_titles() {
    let base = Document::new("Base Title");
    let op_a = Operation {
        id: OperationId {
            actor: ActorId("a".to_string()),
            seq: 1,
        },
        kind: OperationKind::SetDocumentTitle {
            title: " Alpha Title ".to_string(),
        },
        context: None,
    };
    let op_b = Operation {
        id: OperationId {
            actor: ActorId("b".to_string()),
            seq: 1,
        },
        kind: OperationKind::SetDocumentTitle {
            title: "Beta Title".to_string(),
        },
        context: None,
    };
    let op_empty = Operation {
        id: OperationId {
            actor: ActorId("c".to_string()),
            seq: 1,
        },
        kind: OperationKind::SetDocumentTitle {
            title: " ".to_string(),
        },
        context: None,
    };

    let merged_ab = merge_operations(
        &base,
        &[
            vec![op_a.clone()],
            vec![op_empty.clone()],
            vec![op_b.clone()],
        ],
    )
    .unwrap();
    let merged_ba = merge_operations(
        &base,
        &[vec![op_b], vec![op_empty.clone()], vec![op_a.clone()]],
    )
    .unwrap();
    assert_eq!(merged_ab.document.title, merged_ba.document.title);
    assert_eq!(merged_ab.document.title, "Beta Title");
    let title_from_padded_operation =
        merge_operations(&base, &[vec![op_a], vec![op_empty]]).unwrap();
    assert_eq!(title_from_padded_operation.document.title, "Alpha Title");
    assert!(merged_ab
        .warnings
        .iter()
        .any(|warning| warning.code == "invalid-document-title"));
}

#[test]
fn document_doi_updates_converge_allow_clear_and_reject_empty_values() {
    let mut base = Document::new("Base Title");
    base.doi = Some("10.0000/base".to_string());
    let op_a = Operation {
        id: OperationId {
            actor: ActorId("a".to_string()),
            seq: 1,
        },
        kind: OperationKind::SetDocumentDoi {
            doi: Some("10.1234/Alpha".to_string()),
        },
        context: None,
    };
    let op_b = Operation {
        id: OperationId {
            actor: ActorId("b".to_string()),
            seq: 1,
        },
        kind: OperationKind::SetDocumentDoi { doi: None },
        context: None,
    };
    let op_empty = Operation {
        id: OperationId {
            actor: ActorId("c".to_string()),
            seq: 1,
        },
        kind: OperationKind::SetDocumentDoi {
            doi: Some(" ".to_string()),
        },
        context: None,
    };

    let merged_ab = merge_operations(
        &base,
        &[
            vec![op_a.clone()],
            vec![op_empty.clone()],
            vec![op_b.clone()],
        ],
    )
    .unwrap();
    let merged_ba = merge_operations(&base, &[vec![op_b], vec![op_empty], vec![op_a]]).unwrap();
    assert_eq!(merged_ab.document.doi, merged_ba.document.doi);
    assert_eq!(merged_ab.document.doi, None);
    assert!(merged_ab
        .warnings
        .iter()
        .any(|warning| warning.code == "invalid-document-doi"));
}

#[test]
fn document_locale_updates_converge_and_reject_empty_values() {
    let base = Document::new("Base Title");
    let op_a = Operation {
        id: OperationId {
            actor: ActorId("a".to_string()),
            seq: 1,
        },
        kind: OperationKind::SetDocumentLocale {
            locale: " sv-SE ".to_string(),
        },
        context: None,
    };
    let op_b = Operation {
        id: OperationId {
            actor: ActorId("b".to_string()),
            seq: 1,
        },
        kind: OperationKind::SetDocumentLocale {
            locale: "en-GB".to_string(),
        },
        context: None,
    };
    let op_empty = Operation {
        id: OperationId {
            actor: ActorId("c".to_string()),
            seq: 1,
        },
        kind: OperationKind::SetDocumentLocale {
            locale: " ".to_string(),
        },
        context: None,
    };

    let merged_ab = merge_operations(
        &base,
        &[
            vec![op_a.clone()],
            vec![op_empty.clone()],
            vec![op_b.clone()],
        ],
    )
    .unwrap();
    let merged_ba = merge_operations(&base, &[vec![op_b], vec![op_empty], vec![op_a]]).unwrap();
    assert_eq!(merged_ab.document.locale, merged_ba.document.locale);
    assert_eq!(merged_ab.document.locale, "en-GB");
    assert!(merged_ab
        .warnings
        .iter()
        .any(|warning| warning.code == "invalid-document-locale"));
}

#[test]
fn duplicate_insert_ids_degrade_to_warnings() {
    let mut base = Document::new("Doc");
    let existing_block = Block::paragraph("existing");
    let existing_block_id = existing_block.id.clone();
    let existing_inline_id = inline_id(&existing_block.content[0]).clone();
    base.blocks.push(existing_block);

    let duplicate_block = Operation {
        id: OperationId {
            actor: ActorId("a".to_string()),
            seq: 1,
        },
        kind: OperationKind::InsertBlock {
            position: InsertPosition::Last,
            block: Block {
                id: existing_block_id,
                kind: BlockKind::Paragraph,
                content: vec![Inline::text("duplicate block")],
                properties: BlockProperties::default(),
            },
        },
        context: None,
    };
    let duplicate_inline = Operation {
        id: OperationId {
            actor: ActorId("b".to_string()),
            seq: 1,
        },
        kind: OperationKind::InsertInline {
            block_id: base.blocks[0].id.clone(),
            position: InsertPosition::Last,
            inline: Inline::Text {
                id: existing_inline_id,
                text: "duplicate inline".to_string(),
                marks: Vec::new(),
            },
        },
        context: None,
    };

    let result = merge_operations(&base, &[vec![duplicate_block], vec![duplicate_inline]]).unwrap();

    assert_eq!(result.document.visible_text(), "existing\n");
    assert_eq!(
        result
            .warnings
            .iter()
            .map(|warning| warning.code.as_str())
            .collect::<Vec<_>>(),
        vec!["duplicate-block", "duplicate-inline"]
    );
}

#[test]
fn inline_insert_into_deleted_block_appends_to_surviving_block() {
    let mut base = Document::new("Doc");
    let deleted_block = Block::paragraph("deleted");
    let deleted_block_id = deleted_block.id.clone();
    let surviving_block = Block::paragraph("surviving");
    base.blocks.push(deleted_block);
    base.blocks.push(surviving_block);

    let delete = Operation {
        id: OperationId {
            actor: ActorId("a".to_string()),
            seq: 1,
        },
        kind: OperationKind::DeleteBlock {
            block_id: deleted_block_id.clone(),
        },
        context: None,
    };
    let insert = Operation {
        id: OperationId {
            actor: ActorId("b".to_string()),
            seq: 1,
        },
        kind: OperationKind::InsertInline {
            block_id: deleted_block_id,
            position: InsertPosition::Last,
            inline: Inline::text(" preserved"),
        },
        context: None,
    };

    let delete_first =
        merge_operations(&base, &[vec![delete.clone()], vec![insert.clone()]]).unwrap();
    let insert_first = merge_operations(&base, &[vec![insert], vec![delete]]).unwrap();

    assert_eq!(delete_first.document, insert_first.document);
    assert_eq!(
        delete_first.document.visible_text(),
        "surviving preserved\n"
    );
    assert!(delete_first
        .warnings
        .iter()
        .any(|warning| warning.code == "inline-anchor-degraded"));
}

#[test]
fn inline_insert_after_deleted_inline_anchor_appends_with_warning() {
    let mut base = Document::new("Doc");
    let mut block = Block::paragraph("anchor");
    let block_id = block.id.clone();
    let deleted_inline_id = inline_id(&block.content[0]).clone();
    block.content.push(Inline::text(" tail"));
    base.blocks.push(block);

    let delete = Operation {
        id: OperationId {
            actor: ActorId("a".to_string()),
            seq: 1,
        },
        kind: OperationKind::DeleteInline {
            inline_id: deleted_inline_id.clone(),
        },
        context: None,
    };
    let insert = Operation {
        id: OperationId {
            actor: ActorId("b".to_string()),
            seq: 1,
        },
        kind: OperationKind::InsertInline {
            block_id,
            position: InsertPosition::After(deleted_inline_id),
            inline: Inline::text(" inserted"),
        },
        context: None,
    };

    let actor_streams =
        merge_operations(&base, &[vec![delete.clone()], vec![insert.clone()]]).unwrap();
    let storage_batch =
        merge_operations(&base, &[vec![delete.clone(), insert.clone()], vec![]]).unwrap();
    let reversed_batches = merge_operations(&base, &[vec![insert], vec![delete]]).unwrap();

    assert_eq!(actor_streams.document, storage_batch.document);
    assert_eq!(actor_streams.document, reversed_batches.document);
    assert_eq!(actor_streams.document.visible_text(), " tail inserted\n");
    assert!(actor_streams
        .warnings
        .iter()
        .any(|warning| warning.code == "inline-anchor-degraded"));
}

#[test]
fn block_insert_after_deleted_anchor_appends_with_warning() {
    let mut base = Document::new("Doc");
    let deleted_block = Block::paragraph("deleted");
    let deleted_block_id = deleted_block.id.clone();
    let surviving_block = Block::paragraph("surviving");
    base.blocks.push(deleted_block);
    base.blocks.push(surviving_block);

    let delete = Operation {
        id: OperationId {
            actor: ActorId("a".to_string()),
            seq: 1,
        },
        kind: OperationKind::DeleteBlock {
            block_id: deleted_block_id.clone(),
        },
        context: None,
    };
    let insert = Operation {
        id: OperationId {
            actor: ActorId("b".to_string()),
            seq: 1,
        },
        kind: OperationKind::InsertBlock {
            position: InsertPosition::After(deleted_block_id),
            block: Block::paragraph("inserted"),
        },
        context: None,
    };

    let delete_first =
        merge_operations(&base, &[vec![delete.clone()], vec![insert.clone()]]).unwrap();
    let insert_first = merge_operations(&base, &[vec![insert], vec![delete]]).unwrap();

    assert_eq!(delete_first.document, insert_first.document);
    assert_eq!(
        delete_first.document.visible_text(),
        "surviving\ninserted\n"
    );
    assert!(delete_first
        .warnings
        .iter()
        .any(|warning| warning.code == "block-anchor-degraded"));
}

#[test]
fn duplicate_operation_ids_select_deterministic_payload_with_warning() {
    let mut base = Document::new("Doc");
    let block = Block::paragraph("base");
    let block_id = block.id.clone();
    base.blocks.push(block);
    let duplicate_id = OperationId {
        actor: ActorId("actor-a".to_string()),
        seq: 7,
    };
    let insert_alpha = Operation {
        id: duplicate_id.clone(),
        kind: OperationKind::InsertInline {
            block_id: block_id.clone(),
            position: InsertPosition::Last,
            inline: Inline::Text {
                id: StableId::parse("text-alpha").unwrap(),
                text: "alpha ".to_string(),
                marks: Vec::new(),
            },
        },
        context: None,
    };
    let insert_zeta = Operation {
        id: duplicate_id,
        kind: OperationKind::InsertInline {
            block_id,
            position: InsertPosition::Last,
            inline: Inline::Text {
                id: StableId::parse("text-zeta").unwrap(),
                text: "zeta ".to_string(),
                marks: Vec::new(),
            },
        },
        context: None,
    };

    let alpha_first = merge_operations(
        &base,
        &[vec![insert_alpha.clone()], vec![insert_zeta.clone()]],
    )
    .unwrap();
    let zeta_first = merge_operations(&base, &[vec![insert_zeta], vec![insert_alpha]]).unwrap();

    assert_eq!(alpha_first.document, zeta_first.document);
    assert_eq!(alpha_first.warnings, zeta_first.warnings);
    assert_eq!(alpha_first.document.visible_text(), "basealpha \n");
    assert_eq!(alpha_first.warnings[0].code, "duplicate-operation-id");
}

#[test]
fn malformed_operation_ids_are_ignored_with_deterministic_warnings() {
    let mut base = Document::new("Doc");
    let block = Block::paragraph("base");
    let block_id = block.id.clone();
    base.blocks.push(block);
    let malformed = [
        Operation {
            id: OperationId {
                actor: ActorId(String::new()),
                seq: 1,
            },
            kind: OperationKind::InsertInline {
                block_id: block_id.clone(),
                position: InsertPosition::Last,
                inline: Inline::text("empty actor"),
            },
            context: None,
        },
        Operation {
            id: OperationId {
                actor: ActorId(" actor-a ".to_string()),
                seq: 2,
            },
            kind: OperationKind::InsertInline {
                block_id: block_id.clone(),
                position: InsertPosition::Last,
                inline: Inline::text("padded actor"),
            },
            context: None,
        },
        Operation {
            id: OperationId {
                actor: ActorId("actor-b".to_string()),
                seq: 0,
            },
            kind: OperationKind::InsertInline {
                block_id,
                position: InsertPosition::Last,
                inline: Inline::text("zero seq"),
            },
            context: None,
        },
    ];

    let result = merge_operations(
        &base,
        &[
            vec![malformed[1].clone(), malformed[0].clone()],
            vec![malformed[2].clone()],
        ],
    )
    .unwrap();
    let reversed = merge_operations(
        &base,
        &[
            vec![malformed[2].clone()],
            vec![malformed[0].clone(), malformed[1].clone()],
        ],
    )
    .unwrap();

    assert_eq!(result.document, reversed.document);
    assert_eq!(result.warnings, reversed.warnings);
    assert_eq!(result.document.visible_text(), "base\n");
    assert_eq!(
        result
            .warnings
            .iter()
            .filter(|warning| warning.code == "invalid-operation-id")
            .count(),
        3
    );
    assert!(result
        .warnings
        .iter()
        .any(|warning| warning.message == "operation with empty actor was ignored"));
    assert!(result
        .warnings
        .iter()
        .any(|warning| warning.message == "operation with whitespace-padded actor was ignored"));
    assert!(result
        .warnings
        .iter()
        .any(|warning| warning.message == "operation with zero sequence was ignored"));
}

#[test]
fn remove_mark_updates_formatted_inline_by_stable_id() {
    let mut base = Document::new("Doc");
    let mut block = Block::paragraph("hello");
    let text_id = match &mut block.content[0] {
        Inline::Text { id, marks, .. } => {
            marks.push(Mark {
                kind: MarkKind::Bold,
                value: None,
                expand: MarkExpand::Both,
            });
            marks.push(Mark {
                kind: MarkKind::Color,
                value: Some("#2255aa".to_string()),
                expand: MarkExpand::Both,
            });
            id.clone()
        }
        _ => unreachable!(),
    };
    base.blocks.push(block);

    let result = merge_operations(
        &base,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::RemoveMark {
                text_id,
                kind: MarkKind::Bold,
                value: None,
            },
            context: None,
        }]],
    )
    .unwrap();

    let marks = match &result.document.blocks[0].content[0] {
        Inline::Text { marks, .. } => marks,
        _ => unreachable!(),
    };
    assert!(!marks.iter().any(|mark| mark.kind == MarkKind::Bold));
    assert!(marks.iter().any(|mark| mark.kind == MarkKind::Color));
}

#[test]
fn invalid_mark_operation_degrades_to_warning() {
    let mut base = Document::new("Doc");
    let block = Block::paragraph("hello");
    let text_id = inline_id(&block.content[0]).clone();
    base.blocks.push(block);

    let result = merge_operations(
        &base,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::AddMark {
                text_id,
                mark: Mark {
                    kind: MarkKind::Color,
                    value: None,
                    expand: MarkExpand::Both,
                },
            },
            context: None,
        }]],
    )
    .unwrap();

    let marks = match &result.document.blocks[0].content[0] {
        Inline::Text { marks, .. } => marks,
        _ => unreachable!(),
    };
    assert!(marks.is_empty());
    assert_eq!(result.warnings[0].code, "invalid-mark-value");
}

#[test]
fn invalid_mark_removal_operation_degrades_to_warning() {
    let mut base = Document::new("Doc");
    let mut block = Block::paragraph("hello");
    let text_id = match &mut block.content[0] {
        Inline::Text { id, marks, .. } => {
            marks.push(Mark {
                kind: MarkKind::Bold,
                value: None,
                expand: MarkExpand::Both,
            });
            id.clone()
        }
        _ => unreachable!(),
    };
    base.blocks.push(block);

    let result = merge_operations(
        &base,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::RemoveMark {
                text_id,
                kind: MarkKind::Bold,
                value: Some("true".to_string()),
            },
            context: None,
        }]],
    )
    .unwrap();

    let marks = match &result.document.blocks[0].content[0] {
        Inline::Text { marks, .. } => marks,
        _ => unreachable!(),
    };
    assert!(marks.iter().any(|mark| mark.kind == MarkKind::Bold));
    assert_eq!(result.warnings[0].code, "invalid-mark-value");
}

#[test]
fn valued_mark_removal_without_value_removes_all_matching_marks() {
    let mut base = Document::new("Doc");
    let mut block = Block::paragraph("hello");
    let text_id = match &mut block.content[0] {
        Inline::Text { id, marks, .. } => {
            marks.push(Mark {
                kind: MarkKind::Color,
                value: Some("#2255aa".to_string()),
                expand: MarkExpand::Both,
            });
            marks.push(Mark {
                kind: MarkKind::Bold,
                value: None,
                expand: MarkExpand::Both,
            });
            id.clone()
        }
        _ => unreachable!(),
    };
    base.blocks.push(block);

    let result = merge_operations(
        &base,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::RemoveMark {
                text_id,
                kind: MarkKind::Color,
                value: None,
            },
            context: None,
        }]],
    )
    .unwrap();

    let marks = match &result.document.blocks[0].content[0] {
        Inline::Text { marks, .. } => marks,
        _ => unreachable!(),
    };
    assert!(!marks.iter().any(|mark| mark.kind == MarkKind::Color));
    assert!(marks.iter().any(|mark| mark.kind == MarkKind::Bold));
    assert!(result.warnings.is_empty());
}

#[test]
fn invalid_mark_range_operation_degrades_to_warning() {
    let mut base = Document::new("Doc");
    let mut block = Block::paragraph("");
    block.content.clear();
    let first = Inline::text("alpha ");
    let first_id = inline_id(&first).clone();
    let second = Inline::text("beta");
    let second_id = inline_id(&second).clone();
    block.content.extend([first, second]);
    base.blocks.push(block);

    let result = merge_operations(
        &base,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::AddMarkRange {
                range: TextRange {
                    start: first_id,
                    end: second_id,
                },
                mark: Mark {
                    kind: MarkKind::Bold,
                    value: Some("true".to_string()),
                    expand: MarkExpand::Both,
                },
            },
            context: None,
        }]],
    )
    .unwrap();

    assert!(result
        .document
        .blocks
        .iter()
        .flat_map(|block| &block.content)
        .all(|inline| match inline {
            Inline::Text { marks, .. } => marks.is_empty(),
            _ => true,
        }));
    assert_eq!(result.warnings[0].code, "invalid-mark-value");
}

/// `merge_operations` ends with `document.validate()?`, so it can only ever
/// return a valid document.
///
/// This is worth one test because it makes roughly 113 assertions elsewhere in
/// this crate provably dead: every
/// `result.document.validate().unwrap()` — and its `assert!(…is_ok())` twin —
/// on a value obtained from `merge_operations(…).unwrap()` re-checks something
/// the call already checked and already unwrapped. They read as verification
/// and are not. PLAN88 §7.
///
/// Pinning it here is what lets them be deleted rather than trusted: if the
/// final `validate()?` were ever removed, this fails, and *nothing else would
/// have*.
#[test]
fn merge_operations_refuses_to_return_a_document_that_does_not_validate() {
    // A base whose two blocks share an id — the model forbids it, and no
    // operation here repairs it, so the merge's own validation is the only
    // thing that can notice.
    let mut base = Document::new("Doc");
    let mut first = Block::paragraph("one");
    let mut second = Block::paragraph("two");
    second.id = first.id.clone();
    match (&mut first.content[0], &mut second.content[0]) {
        (Inline::Text { id, .. }, Inline::Text { id: other, .. }) => *other = id.clone(),
        _ => unreachable!(),
    }
    base.blocks.push(first);
    base.blocks.push(second);
    assert!(
        base.validate().is_err(),
        "the fixture is meant to be an invalid document"
    );

    let error = merge_operations(
        &base,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::SetDocumentTitle {
                title: "Retitled".to_string(),
            },
            context: None,
        }]],
    )
    .expect_err("the merge returned a document that does not validate");
    assert!(
        format!("{error}").contains("duplicate"),
        "unexpected error: {error}"
    );
}
