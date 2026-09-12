//! Operation tests.

use crate::causal::{ActorId, OperationId};
use crate::inline_ops::inline_id;
use crate::merge::merge_operations;
use crate::operation::{Operation, OperationKind};
use opendoc_core::{
    Block, BlockKind, BlockProperties, Document, Inline, Mark, MarkExpand, MarkKind, StableId,
    TextRange,
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
            after: Some(text_id.clone()),
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
    assert!(merged_ab.document.validate().is_ok());
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
    assert!(merged_ab.document.validate().is_ok());
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
    assert!(merged_ab.document.validate().is_ok());
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
    assert!(merged_ab.document.validate().is_ok());
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
            after: None,
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
            after: None,
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
    assert!(result.document.validate().is_ok());
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
            after: None,
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
    delete_first.document.validate().unwrap();
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
            after: Some(deleted_inline_id),
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
    actor_streams.document.validate().unwrap();
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
            after: Some(deleted_block_id),
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
    delete_first.document.validate().unwrap();
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
            after: None,
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
            after: None,
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
    alpha_first.document.validate().unwrap();
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
                after: None,
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
                after: None,
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
                after: None,
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
    result.document.validate().unwrap();
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
    result.document.validate().unwrap();
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
    result.document.validate().unwrap();
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
    result.document.validate().unwrap();
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
    result.document.validate().unwrap();
}
