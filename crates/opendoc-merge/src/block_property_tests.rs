//! Block property tests.

use crate::merge::merge_operations;
use crate::operation::{Operation, OperationKind};
use crate::test_support::{document_with_two_paragraphs, property_op};
use opendoc_core::{
    Alignment, Block, BlockKind, BlockProperties, BlockProperty, BlockPropertyKey, CellSpan,
    Document, Inline, Length, LineSpacing, ListKind, StableId, TableCell, TableRow, TextDirection,
};

#[test]
fn block_properties_are_set_and_cleared_through_the_operation_path() {
    let (base, block_id, _) = document_with_two_paragraphs();
    let indent = Length::from_points(36.0).unwrap();

    let set = merge_operations(
        &base,
        &[vec![
            property_op(
                "a",
                1,
                OperationKind::SetBlockProperty {
                    block_id: block_id.clone(),
                    property: BlockProperty::Alignment(Alignment::Center),
                },
            ),
            property_op(
                "a",
                2,
                OperationKind::SetBlockProperty {
                    block_id: block_id.clone(),
                    property: BlockProperty::IndentStart(indent),
                },
            ),
        ]],
    )
    .unwrap();
    assert!(set.warnings.is_empty());
    assert_eq!(
        set.document.blocks[0].properties.alignment,
        Some(Alignment::Center)
    );
    assert_eq!(set.document.blocks[0].properties.indent_start, Some(indent));
    // Untouched blocks stay inheriting.
    assert!(set.document.blocks[1].properties.is_empty());
    set.document.validate().unwrap();

    let cleared = merge_operations(
        &set.document,
        &[vec![property_op(
            "a",
            3,
            OperationKind::ClearBlockProperty {
                block_id,
                key: BlockPropertyKey::Alignment,
            },
        )]],
    )
    .unwrap();
    assert!(cleared.warnings.is_empty());
    assert_eq!(cleared.document.blocks[0].properties.alignment, None);
    assert_eq!(
        cleared.document.blocks[0].properties.indent_start,
        Some(indent)
    );
}

#[test]
fn block_properties_apply_to_blocks_nested_in_table_cells() {
    let mut base = Document::new("Doc");
    let nested = Block::paragraph("cell text");
    let nested_id = nested.id.clone();
    base.blocks.push(Block {
        id: StableId::parse("table-1").unwrap(),
        kind: BlockKind::table(vec![TableRow {
            id: StableId::parse("row-1").unwrap(),
            cells: vec![TableCell {
                id: StableId::parse("cell-1").unwrap(),
                span: CellSpan::SINGLE,
                properties: Default::default(),
                blocks: vec![nested],
            }],
        }]),
        content: Vec::new(),
        properties: BlockProperties::default(),
    });

    let result = merge_operations(
        &base,
        &[vec![property_op(
            "a",
            1,
            OperationKind::SetBlockProperty {
                block_id: nested_id,
                property: BlockProperty::Alignment(Alignment::End),
            },
        )]],
    )
    .unwrap();
    assert!(result.warnings.is_empty());
    let BlockKind::Table { rows, .. } = &result.document.blocks[0].kind else {
        unreachable!()
    };
    assert_eq!(
        rows[0].cells[0].blocks[0].properties.alignment,
        Some(Alignment::End)
    );
}

#[test]
fn concurrent_edits_to_the_same_property_converge_last_writer_wins() {
    let (base, block_id, _) = document_with_two_paragraphs();
    // Same block, same property, two actors, no causal relationship.
    let a = vec![property_op(
        "actor-a",
        1,
        OperationKind::SetBlockProperty {
            block_id: block_id.clone(),
            property: BlockProperty::Alignment(Alignment::Center),
        },
    )];
    let b = vec![property_op(
        "actor-b",
        1,
        OperationKind::SetBlockProperty {
            block_id: block_id.clone(),
            property: BlockProperty::Alignment(Alignment::Justify),
        },
    )];

    let forward = merge_operations(&base, &[a.clone(), b.clone()]).unwrap();
    let reverse = merge_operations(&base, &[b.clone(), a.clone()]).unwrap();
    let interleaved = merge_operations(
        &base,
        &[a.iter().chain(b.iter()).cloned().collect::<Vec<_>>()],
    )
    .unwrap();

    assert_eq!(forward.document, reverse.document);
    assert_eq!(forward.document, interleaved.document);
    assert!(forward.warnings.is_empty());
    // "Last writer" is the highest (actor, seq) — actor-b here. That is a
    // deterministic order, not a causal one; see docs/adr/0006.
    assert_eq!(
        forward.document.blocks[0].properties.alignment,
        Some(Alignment::Justify)
    );
    forward.document.validate().unwrap();
}

#[test]
fn concurrent_edits_to_different_properties_of_one_block_all_survive() {
    let (base, block_id, _) = document_with_two_paragraphs();
    let a = vec![property_op(
        "actor-a",
        1,
        OperationKind::SetBlockProperty {
            block_id: block_id.clone(),
            property: BlockProperty::Alignment(Alignment::Center),
        },
    )];
    let b = vec![property_op(
        "actor-b",
        1,
        OperationKind::SetBlockProperty {
            block_id: block_id.clone(),
            property: BlockProperty::LineSpacing(LineSpacing::multiple(1.5).unwrap()),
        },
    )];
    let c = vec![property_op(
        "actor-c",
        1,
        OperationKind::SetBlockProperty {
            block_id: block_id.clone(),
            property: BlockProperty::IndentFirstLine(Length::from_points(-18.0).unwrap()),
        },
    )];

    let forward = merge_operations(&base, &[a.clone(), b.clone(), c.clone()]).unwrap();
    let reverse = merge_operations(&base, &[c, b, a]).unwrap();
    assert_eq!(forward.document, reverse.document);

    let properties = &forward.document.blocks[0].properties;
    assert_eq!(properties.alignment, Some(Alignment::Center));
    assert_eq!(
        properties.line_spacing,
        Some(LineSpacing::multiple(1.5).unwrap())
    );
    // Per-property granularity is what makes this hold: a whole-bag
    // last-writer-wins would have dropped two of these three edits.
    assert_eq!(properties.hanging_indent(), Length::from_points(18.0).ok());
    assert_eq!(properties.iter().count(), 3);
}

#[test]
fn concurrent_property_edits_on_different_blocks_do_not_interfere() {
    let (base, first_id, second_id) = document_with_two_paragraphs();
    let a = vec![property_op(
        "actor-a",
        1,
        OperationKind::SetBlockProperty {
            block_id: first_id,
            property: BlockProperty::Alignment(Alignment::Center),
        },
    )];
    let b = vec![property_op(
        "actor-b",
        1,
        OperationKind::SetBlockProperty {
            block_id: second_id,
            property: BlockProperty::Alignment(Alignment::End),
        },
    )];

    let forward = merge_operations(&base, &[a.clone(), b.clone()]).unwrap();
    let reverse = merge_operations(&base, &[b, a]).unwrap();
    assert_eq!(forward.document, reverse.document);
    assert_eq!(
        forward.document.blocks[0].properties.alignment,
        Some(Alignment::Center)
    );
    assert_eq!(
        forward.document.blocks[1].properties.alignment,
        Some(Alignment::End)
    );
}

#[test]
fn a_concurrent_clear_and_set_of_one_property_converge() {
    let (mut base, block_id, _) = document_with_two_paragraphs();
    base.blocks[0]
        .properties
        .set(BlockProperty::Alignment(Alignment::Start));

    let clear = vec![property_op(
        "actor-a",
        1,
        OperationKind::ClearBlockProperty {
            block_id: block_id.clone(),
            key: BlockPropertyKey::Alignment,
        },
    )];
    let set = vec![property_op(
        "actor-b",
        1,
        OperationKind::SetBlockProperty {
            block_id: block_id.clone(),
            property: BlockProperty::Alignment(Alignment::Center),
        },
    )];

    let forward = merge_operations(&base, &[clear.clone(), set.clone()]).unwrap();
    let reverse = merge_operations(&base, &[set, clear]).unwrap();
    assert_eq!(forward.document, reverse.document);
    // actor-b sorts last, so the set wins over the clear.
    assert_eq!(
        forward.document.blocks[0].properties.alignment,
        Some(Alignment::Center)
    );
}

#[test]
fn property_edits_to_a_concurrently_deleted_block_degrade_to_warnings() {
    let (base, block_id, _) = document_with_two_paragraphs();
    let delete = vec![property_op(
        "actor-a",
        1,
        OperationKind::DeleteBlock {
            block_id: block_id.clone(),
        },
    )];
    let style = vec![
        property_op(
            "actor-b",
            1,
            OperationKind::SetBlockProperty {
                block_id: block_id.clone(),
                property: BlockProperty::Alignment(Alignment::Center),
            },
        ),
        property_op(
            "actor-b",
            2,
            OperationKind::ClearBlockProperty {
                block_id,
                key: BlockPropertyKey::SpaceAfter,
            },
        ),
    ];

    let forward = merge_operations(&base, &[delete.clone(), style.clone()]).unwrap();
    let reverse = merge_operations(&base, &[style, delete]).unwrap();
    assert_eq!(forward.document, reverse.document);
    assert_eq!(forward.warnings, reverse.warnings);
    assert_eq!(forward.document.blocks.len(), 1);
    assert_eq!(
        forward
            .warnings
            .iter()
            .filter(|warning| warning.code == "missing-block")
            .count(),
        2
    );
    forward.document.validate().unwrap();
}

#[test]
fn out_of_range_property_values_degrade_to_warnings_instead_of_corrupting_the_document() {
    let (base, block_id, _) = document_with_two_paragraphs();
    // Smart constructors cannot produce these; a hostile or stale peer can.
    let invalid = vec![
        property_op(
            "actor-a",
            1,
            OperationKind::SetBlockProperty {
                block_id: block_id.clone(),
                property: BlockProperty::SpaceBefore(Length::from_points(-6.0).unwrap()),
            },
        ),
        property_op(
            "actor-a",
            2,
            OperationKind::SetBlockProperty {
                block_id: block_id.clone(),
                property: BlockProperty::Alignment(Alignment::Center),
            },
        ),
    ];

    let result = merge_operations(&base, &[invalid]).unwrap();
    assert_eq!(result.warnings[0].code, "invalid-block-property");
    assert_eq!(result.document.blocks[0].properties.space_before, None);
    assert_eq!(
        result.document.blocks[0].properties.alignment,
        Some(Alignment::Center)
    );
    result.document.validate().unwrap();
}

#[test]
fn list_kind_updates_carry_checklist_state_through_merge() {
    let mut base = Document::new("Doc");
    let block_id = StableId::parse("list-item-1").unwrap();
    base.blocks.push(Block {
        id: block_id.clone(),
        kind: BlockKind::ListItem {
            list_id: StableId::parse("list-run-1").unwrap(),
            level: 0,
            kind: ListKind::Bullet,
        },
        content: vec![Inline::text("buy milk")],
        properties: BlockProperties::default(),
    });

    let checked = merge_operations(
        &base,
        &[vec![
            property_op(
                "a",
                1,
                OperationKind::UpdateListItem {
                    block_id: block_id.clone(),
                    level: 0,
                    kind: ListKind::unchecked(),
                },
            ),
            property_op(
                "a",
                2,
                OperationKind::UpdateListItem {
                    block_id: block_id.clone(),
                    level: 0,
                    kind: ListKind::Checklist { checked: true },
                },
            ),
        ]],
    )
    .unwrap();
    assert!(checked.warnings.is_empty());
    assert_eq!(
        checked.document.blocks[0].list_kind(),
        Some(ListKind::Checklist { checked: true })
    );
    checked.document.validate().unwrap();

    // Two actors racing on the checkbox still converge.
    let a = vec![property_op(
        "actor-a",
        1,
        OperationKind::UpdateListItem {
            block_id: block_id.clone(),
            level: 0,
            kind: ListKind::Checklist { checked: true },
        },
    )];
    let b = vec![property_op(
        "actor-b",
        1,
        OperationKind::UpdateListItem {
            block_id,
            level: 1,
            kind: ListKind::Ordered,
        },
    )];
    let forward = merge_operations(&base, &[a.clone(), b.clone()]).unwrap();
    let reverse = merge_operations(&base, &[b, a]).unwrap();
    assert_eq!(forward.document, reverse.document);
    assert_eq!(
        forward.document.blocks[0].list_kind(),
        Some(ListKind::Ordered)
    );
}

#[test]
fn deterministic_pseudo_fuzz_of_property_edits_converges() {
    let mut base = Document::new("Doc");
    let mut block_ids = Vec::new();
    for index in 0..4 {
        let block = Block::paragraph(format!("paragraph {index}"));
        block_ids.push(block.id.clone());
        base.blocks.push(block);
    }

    let mut streams = vec![Vec::new(), Vec::new(), Vec::new(), Vec::new()];
    let mut seqs = [1_u64, 1, 1, 1];
    let mut rng = 0x0b10_c4de_u64;
    for step in 0..64_u64 {
        rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
        let actor_index = (rng as usize) % streams.len();
        let seq = seqs[actor_index];
        seqs[actor_index] += 1;
        let block_id = block_ids[(rng as usize >> 8) % block_ids.len()].clone();
        let key = BlockPropertyKey::ALL[(rng as usize >> 16) % BlockPropertyKey::ALL.len()];
        let kind = if step % 5 == 0 {
            OperationKind::ClearBlockProperty { block_id, key }
        } else {
            let twips = ((rng >> 24) % 1_440) as i32;
            let property = match key {
                BlockPropertyKey::Alignment => BlockProperty::Alignment(
                    Alignment::ALL[(rng as usize >> 32) % Alignment::ALL.len()],
                ),
                BlockPropertyKey::IndentStart => {
                    BlockProperty::IndentStart(Length::from_twips(twips).unwrap())
                }
                BlockPropertyKey::IndentEnd => {
                    BlockProperty::IndentEnd(Length::from_twips(twips).unwrap())
                }
                BlockPropertyKey::IndentFirstLine => {
                    BlockProperty::IndentFirstLine(Length::from_twips(-twips).unwrap())
                }
                BlockPropertyKey::LineSpacing => BlockProperty::LineSpacing(
                    LineSpacing::multiple(1.0 + (twips % 200) as f64 / 100.0).unwrap(),
                ),
                BlockPropertyKey::SpaceBefore => {
                    BlockProperty::SpaceBefore(Length::from_twips(twips).unwrap())
                }
                BlockPropertyKey::SpaceAfter => {
                    BlockProperty::SpaceAfter(Length::from_twips(twips).unwrap())
                }
                BlockPropertyKey::Direction => BlockProperty::Direction(
                    TextDirection::ALL[(rng as usize >> 32) % TextDirection::ALL.len()],
                ),
            };
            OperationKind::SetBlockProperty { block_id, property }
        };
        streams[actor_index].push(property_op(&format!("actor-{actor_index}"), seq, kind));
    }

    let reference = merge_operations(&base, &streams).unwrap();
    let single = streams.iter().flatten().cloned().collect::<Vec<_>>();
    let reversed = streams
        .iter()
        .rev()
        .cloned()
        .collect::<Vec<Vec<Operation>>>();
    let split_in_two = {
        let mut halves = vec![Vec::new(), Vec::new()];
        for (index, op) in single.iter().enumerate() {
            halves[index % 2].push(op.clone());
        }
        halves
    };

    assert_eq!(
        reference.document,
        merge_operations(&base, &[single]).unwrap().document
    );
    assert_eq!(
        reference.document,
        merge_operations(&base, &reversed).unwrap().document
    );
    assert_eq!(
        reference.document,
        merge_operations(&base, &split_in_two).unwrap().document
    );
    assert!(reference.warnings.is_empty());
    // The fuzz must actually have written something, or it proves nothing.
    assert!(reference
        .document
        .blocks
        .iter()
        .any(|block| !block.properties.is_empty()));
    reference.document.validate().unwrap();
}
