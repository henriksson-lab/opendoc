//! Tests for the inverse-operation vocabulary and the character re-resolution
//! behind collaborative undo. See
//! `docs/adr/0017-collaborative-undo-as-inverse-operations.md`.

use crate::causal::{ActorId, CausalContext, OperationId};
use crate::inverse::{invert_operation, invert_text_operations, Inversion};
use crate::merge::{batch_inverse_capture_order, merge_operations};
use crate::operation::{BlockTextStyle, Operation, OperationKind};
use opendoc_core::{
    Alignment, Block, BlockKind, BlockProperties, BlockProperty, BlockPropertyKey, Document,
    Inline, InsertPosition, ListKind, Mark, MarkExpand, MarkKind, StableId, TableCell, TableRow,
    TextRange,
};

fn run_id() -> StableId {
    StableId::parse("inl-run").unwrap()
}

fn block_id() -> StableId {
    StableId::parse("blk-one").unwrap()
}

/// One paragraph holding one run of `text`, with stable ids so a document can
/// be compared as a whole.
fn document_with_run(text: &str) -> Document {
    let mut document = Document::new("Doc");
    document.blocks.push(Block {
        id: block_id(),
        kind: BlockKind::Paragraph,
        properties: BlockProperties::default(),
        content: vec![Inline::Text {
            id: run_id(),
            text: text.to_string(),
            marks: Vec::new(),
        }],
    });
    document.validate().expect("the fixture is valid");
    document
}

fn run_text(document: &Document) -> String {
    for block in &document.blocks {
        for inline in &block.content {
            if let Inline::Text { id, text, .. } = inline {
                if *id == run_id() {
                    return text.clone();
                }
            }
        }
    }
    String::new()
}

#[test]
fn inverse_capture_models_the_merges_deferred_passes_instead_of_gesture_order() {
    let range = TextRange {
        start: StableId::new("range-start"),
        end: StableId::new("range-end"),
    };
    let batch = vec![
        // These three occur before the ordinary write in gesture order.  The
        // merge itself deliberately runs them afterwards, so an inverse fold
        // has to do the same or it records states the batch never reaches.
        OperationKind::AddMarkRange {
            range,
            mark: Mark {
                kind: MarkKind::Bold,
                value: None,
                expand: MarkExpand::Both,
            },
        },
        OperationKind::RestoreCommentThread {
            thread_id: StableId::new("thread"),
        },
        OperationKind::AcceptSuggestion {
            suggestion_id: StableId::new("suggestion"),
            accepted_by: "Reviewer".to_string(),
        },
        OperationKind::SetDocumentTitle {
            title: "ordinary pass".to_string(),
        },
        OperationKind::InsertText {
            inline_id: StableId::new("text"),
            offset: 0,
            text: "deferred text".to_string(),
        },
    ];

    assert_eq!(
        batch_inverse_capture_order(&batch),
        vec![3, 2, 1, 0, 4],
        "ordinary operations, suggestion resolutions, comment restores, mark ranges, then character edits"
    );
}

fn op(actor: &str, seq: u64, kind: OperationKind, observing: &[Operation]) -> Operation {
    Operation::in_context(
        OperationId {
            actor: ActorId(actor.to_string()),
            seq,
        },
        kind,
        CausalContext::observing(observing.iter()),
    )
}

fn merged(base: &Document, operations: &[Operation]) -> Document {
    merge_operations(base, &[operations.to_vec()])
        .expect("the set merges")
        .document
}

fn kinds(inversion: Inversion) -> Vec<OperationKind> {
    match inversion {
        Inversion::Operations(kinds) => kinds,
        other => panic!("expected inverse operations, got {other:?}"),
    }
}

/// Applies an undo the way the app does: the inverse operations, in the order
/// they were produced, each observing the ones before it.
fn undo(base: &Document, set: &[Operation], inverse: Vec<OperationKind>) -> Document {
    let mut set = set.to_vec();
    let mut seq = set
        .iter()
        .filter(|operation| operation.id.actor.0 == "a")
        .map(|operation| operation.id.seq)
        .max()
        .unwrap_or(0);
    for kind in inverse {
        seq += 1;
        let observing = set.clone();
        set.push(op("a", seq, kind, &observing));
    }
    merged(base, &set)
}

// ---------------------------------------------------------------------------
// The classic case: undoing an insert a colleague has since edited.
// ---------------------------------------------------------------------------

/// A inserts "hello", B types inside it, A undoes the insert.
///
/// The inverse removes only the characters A inserted. B's characters were
/// never A's to remove, so they stay — and they stay *in place*, because the
/// inverse is two deletes around them rather than one delete across them.
#[test]
fn undoing_an_insert_a_colleague_edited_inside_keeps_the_colleagues_text() {
    let base = document_with_run("[]");
    let a1 = op(
        "a",
        1,
        OperationKind::InsertText {
            inline_id: run_id(),
            offset: 1,
            text: "hello".to_string(),
        },
        &[],
    );
    // B saw A's insert and typed into the middle of it.
    let b1 = op(
        "b",
        1,
        OperationKind::InsertText {
            inline_id: run_id(),
            offset: 3,
            text: "XX".to_string(),
        },
        std::slice::from_ref(&a1),
    );
    let set = vec![a1.clone(), b1];
    assert_eq!(run_text(&merged(&base, &set)), "[heXXllo]");

    let inverse = kinds(invert_text_operations(
        &base,
        &set,
        std::slice::from_ref(&a1.id),
    ));
    assert_eq!(
        inverse,
        vec![
            OperationKind::DeleteText {
                inline_id: run_id(),
                start: 5,
                end: 8,
            },
            OperationKind::DeleteText {
                inline_id: run_id(),
                start: 1,
                end: 3,
            },
        ],
        "A's characters are in two visible ranges, and the undo must name both"
    );
    assert_eq!(run_text(&undo(&base, &set, inverse)), "[XX]");
}

/// The offsets an undo emits are the run's offsets *now*, not the ones the
/// operation was written with.
///
/// B typed ahead of A's insert, so A's characters have moved. Naively
/// re-using A's own offset would delete B's text instead — the bug this
/// resolution exists to prevent.
#[test]
fn an_undo_resolves_offsets_against_the_run_as_it_stands_now() {
    let base = document_with_run("abcdefgh");
    let a1 = op(
        "a",
        1,
        OperationKind::InsertText {
            inline_id: run_id(),
            offset: 2,
            text: "A".to_string(),
        },
        &[],
    );
    // Concurrent: B did not observe A.
    let b1 = op(
        "b",
        1,
        OperationKind::InsertText {
            inline_id: run_id(),
            offset: 0,
            text: "BBB".to_string(),
        },
        &[],
    );
    let set = vec![a1.clone(), b1];
    let text = run_text(&merged(&base, &set));
    assert_eq!(text, "BBBabAcdefgh");

    let inverse = kinds(invert_text_operations(
        &base,
        &set,
        std::slice::from_ref(&a1.id),
    ));
    assert_eq!(
        inverse,
        vec![OperationKind::DeleteText {
            inline_id: run_id(),
            start: 5,
            end: 6,
        }],
        "A's character is at visible offset 5 now, not at the 2 it was written with"
    );
    assert_eq!(run_text(&undo(&base, &set, inverse)), "BBBabcdefgh");
}

/// Undoing a delete restores exactly what that delete removed.
#[test]
fn undoing_a_delete_restores_the_characters_it_removed() {
    let base = document_with_run("abcdefgh");
    let a1 = op(
        "a",
        1,
        OperationKind::DeleteText {
            inline_id: run_id(),
            start: 2,
            end: 5,
        },
        &[],
    );
    let set = vec![a1.clone()];
    assert_eq!(run_text(&merged(&base, &set)), "abfgh");

    let inverse = kinds(invert_text_operations(
        &base,
        &set,
        std::slice::from_ref(&a1.id),
    ));
    assert_eq!(
        inverse,
        vec![OperationKind::InsertText {
            inline_id: run_id(),
            offset: 2,
            text: "cde".to_string(),
        }]
    );
    assert_eq!(run_text(&undo(&base, &set, inverse)), "abcdefgh");
}

/// A character two actors both deleted stays deleted when one of them undoes.
///
/// The other actor still wants it gone, and an undo may only withdraw its own
/// author's contribution.
#[test]
fn undoing_a_delete_does_not_resurrect_what_another_actor_also_deleted() {
    let base = document_with_run("abcdefgh");
    let a1 = op(
        "a",
        1,
        OperationKind::DeleteText {
            inline_id: run_id(),
            start: 1,
            end: 5,
        },
        &[],
    );
    let b1 = op(
        "b",
        1,
        OperationKind::DeleteText {
            inline_id: run_id(),
            start: 3,
            end: 7,
        },
        &[],
    );
    let set = vec![a1.clone(), b1];
    assert_eq!(run_text(&merged(&base, &set)), "ah");

    let inverse = kinds(invert_text_operations(
        &base,
        &set,
        std::slice::from_ref(&a1.id),
    ));
    assert_eq!(
        run_text(&undo(&base, &set, inverse)),
        "abch",
        "only the characters b did not also delete come back"
    );
}

/// Several edits to one run in one undo step share a coordinate space, so
/// their inverses are computed together. A replace-all is the case that
/// forces it: a delete and an insert per occurrence.
#[test]
fn a_step_of_several_edits_to_one_run_inverts_as_a_whole() {
    let base = document_with_run("one one one");
    let mut set: Vec<Operation> = Vec::new();
    // Planned last-first against the pre-edit snapshot, exactly as the
    // replace-all path plans them.
    for (index, (start, end)) in [(8usize, 11usize), (4, 7), (0, 3)].into_iter().enumerate() {
        let observing = set.clone();
        set.push(op(
            "a",
            (index * 2 + 1) as u64,
            OperationKind::DeleteText {
                inline_id: run_id(),
                start,
                end,
            },
            &observing,
        ));
        let observing = set.clone();
        set.push(op(
            "a",
            (index * 2 + 2) as u64,
            OperationKind::InsertText {
                inline_id: run_id(),
                offset: start,
                text: "1".to_string(),
            },
            &observing,
        ));
    }
    assert_eq!(run_text(&merged(&base, &set)), "1 1 1");

    let targets: Vec<OperationId> = set.iter().map(|operation| operation.id.clone()).collect();
    let inverse = kinds(invert_text_operations(&base, &set, &targets));
    assert_eq!(
        run_text(&undo(&base, &set, inverse)),
        "one one one",
        "one undo has to restore every occurrence"
    );
}

/// An insert and a delete of the same characters inside one step cancel: the
/// undo neither resurrects them nor tries to delete them twice.
#[test]
fn text_typed_and_deleted_within_one_step_stays_gone_after_the_undo() {
    let base = document_with_run("ab");
    let a1 = op(
        "a",
        1,
        OperationKind::InsertText {
            inline_id: run_id(),
            offset: 1,
            text: "XYZ".to_string(),
        },
        &[],
    );
    let a2 = op(
        "a",
        2,
        OperationKind::DeleteText {
            inline_id: run_id(),
            start: 1,
            end: 3,
        },
        std::slice::from_ref(&a1),
    );
    let set = vec![a1.clone(), a2.clone()];
    assert_eq!(run_text(&merged(&base, &set)), "aZb");

    let inverse = kinds(invert_text_operations(
        &base,
        &set,
        &[a1.id.clone(), a2.id.clone()],
    ));
    assert_eq!(run_text(&undo(&base, &set, inverse)), "ab");
}

/// A whole-run rewrite ordered after a character edit discarded it (ADR 0007),
/// so there is nothing of it left to undo and the inverse says so rather than
/// inventing an edit.
#[test]
fn a_character_edit_a_whole_run_write_discarded_inverts_to_nothing() {
    let base = document_with_run("abcdefgh");
    let a1 = op(
        "a",
        1,
        OperationKind::DeleteText {
            inline_id: run_id(),
            start: 0,
            end: 3,
        },
        &[],
    );
    let a2 = op(
        "a",
        2,
        OperationKind::UpdateInlineText {
            inline_id: run_id(),
            text: "replaced".to_string(),
        },
        std::slice::from_ref(&a1),
    );
    let set = vec![a1.clone(), a2];
    assert_eq!(run_text(&merged(&base, &set)), "replaced");
    assert_eq!(
        invert_text_operations(&base, &set, std::slice::from_ref(&a1.id)),
        Inversion::Nothing
    );
}

// ---------------------------------------------------------------------------
// Restoring a deleted block, and the operations that have to carry state.
// ---------------------------------------------------------------------------

/// A delete's inverse has to carry what it removed, and it has to carry it as
/// it was *at that moment* — which is why the inverse is captured against the
/// document before the operation rather than worked out afterwards.
#[test]
fn deleting_a_block_inverts_to_reinserting_it_with_its_content_and_anchor() {
    let mut base = document_with_run("body");
    let first = Block::paragraph("first");
    let first_id = first.id.clone();
    base.blocks.insert(0, first);
    base.validate().unwrap();

    let inverse = kinds(invert_operation(
        &base,
        &OperationKind::DeleteBlock {
            block_id: block_id(),
        },
    ));
    match &inverse[0] {
        OperationKind::InsertBlock { position, block } => {
            assert_eq!(
                *position,
                InsertPosition::After(first_id.clone()),
                "the anchor is by identity"
            );
            assert_eq!(block.id, block_id());
            assert_eq!(
                block.content.len(),
                1,
                "the removed block's content has to come back with it"
            );
        }
        other => panic!("expected an InsertBlock, got {other:?}"),
    }
}

/// A restored block brings its runs' text with it, so the character edits that
/// shaped that text before the delete must not be replayed onto the copy. That
/// is the whole-run-write reset ADR 0007 describes, extended to the operation
/// that also writes whole runs.
#[test]
fn a_delete_and_a_reinsert_of_its_block_do_not_replay_the_text_edits_twice() {
    let base = document_with_run("abcdef");
    let a1 = op(
        "a",
        1,
        OperationKind::DeleteText {
            inline_id: run_id(),
            start: 0,
            end: 3,
        },
        &[],
    );
    let after_delete = merged(&base, std::slice::from_ref(&a1));
    assert_eq!(run_text(&after_delete), "def");
    // The block is deleted next, so its inverse captures it as it is *now*.
    let restore = kinds(invert_operation(
        &after_delete,
        &OperationKind::DeleteBlock {
            block_id: block_id(),
        },
    ));
    let a2 = op(
        "a",
        2,
        OperationKind::DeleteBlock {
            block_id: block_id(),
        },
        std::slice::from_ref(&a1),
    );
    let set = vec![a1.clone(), a2];
    assert!(merged(&base, &set).blocks.is_empty());

    // Undo: the block comes back, then the characters do.
    let mut inverse = restore;
    inverse.extend(kinds(invert_text_operations(
        &base,
        &set,
        std::slice::from_ref(&a1.id),
    )));
    assert_eq!(
        run_text(&undo(&base, &set, inverse)),
        "abcdef",
        "the restored snapshot must not be re-deleted by the operation being undone"
    );
}

/// The case the `after: Option<StableId>` spelling could not express:
/// `None` already meant *append*, so there was no way to say "before the first
/// block" and the inverse used to refuse. `InsertPosition::First` says it, and
/// says it without naming an anchor, so nothing can degrade it to an append.
#[test]
fn deleting_the_first_of_several_blocks_inverts_to_reinserting_it_first() {
    let mut base = document_with_run("body");
    base.blocks.insert(0, Block::paragraph("first"));
    let first_id = base.blocks[0].id.clone();
    base.validate().unwrap();

    let inverse = kinds(invert_operation(
        &base,
        &OperationKind::DeleteBlock {
            block_id: first_id.clone(),
        },
    ));
    match &inverse[..] {
        [OperationKind::InsertBlock { position, block }] => {
            assert_eq!(*position, InsertPosition::First);
            assert_eq!(block.id, first_id);
        }
        other => panic!("expected one InsertBlock, got {other:?}"),
    }
}

/// A cell is a sibling container too. Its first block cannot use `First`,
/// because that would address the document body, so the inverse must retain
/// the next cell-local sibling and restore immediately before it.
#[test]
fn deleting_first_table_cell_block_inverts_before_its_cell_local_sibling() {
    let first = Block::paragraph("first");
    let first_id = first.id.clone();
    let second = Block::paragraph("second");
    let second_id = second.id.clone();
    let mut base = Document::new("Doc");
    base.blocks.push(Block {
        id: StableId::new("table"),
        kind: BlockKind::table(vec![TableRow {
            id: StableId::new("row"),
            height: None,
            header: false,
            cells: vec![TableCell::new(vec![first, second])],
        }]),
        content: Vec::new(),
        properties: BlockProperties::default(),
    });
    base.validate().expect("valid table fixture");

    let inverse = kinds(invert_operation(
        &base,
        &OperationKind::DeleteBlock {
            block_id: first_id.clone(),
        },
    ));
    assert!(matches!(
        inverse.as_slice(),
        [OperationKind::InsertBlock {
            position: InsertPosition::Before(anchor),
            block,
        }] if anchor == &second_id && block.id == first_id
    ));

    let delete = op(
        "a",
        1,
        OperationKind::DeleteBlock {
            block_id: first_id.clone(),
        },
        &[],
    );
    let restored = undo(&base, &[delete], inverse);
    let BlockKind::Table { rows, .. } = &restored.blocks[0].kind else {
        panic!("table survives");
    };
    assert_eq!(
        rows[0].cells[0]
            .blocks
            .iter()
            .map(|block| block.id.clone())
            .collect::<Vec<_>>(),
        [first_id, second_id],
        "the undo remains in the cell and retains sibling order"
    );
}

/// The same gap, for an inline: the first inline of a block with several.
#[test]
fn deleting_the_first_of_several_inlines_inverts_to_reinserting_it_first() {
    let mut base = document_with_run("body");
    let second = Inline::Text {
        id: StableId::parse("inline-second").unwrap(),
        text: "second".to_string(),
        marks: Vec::new(),
    };
    base.blocks[0].content.push(second);
    let first_inline_id = crate::inline_ops::inline_id(&base.blocks[0].content[0]).clone();
    base.validate().unwrap();

    let inverse = kinds(invert_operation(
        &base,
        &OperationKind::DeleteInline {
            inline_id: first_inline_id.clone(),
        },
    ));
    match &inverse[..] {
        [OperationKind::InsertInline {
            block_id,
            position,
            inline,
        }] => {
            assert_eq!(*block_id, base.blocks[0].id);
            assert_eq!(*position, InsertPosition::First);
            assert_eq!(*crate::inline_ops::inline_id(inline), first_inline_id);
        }
        other => panic!("expected one InsertInline, got {other:?}"),
    }
}

/// And for a move: undoing "move the first inline away" has to put it back at
/// the front of the block it came from.
#[test]
fn moving_the_first_of_several_inlines_inverts_to_moving_it_back_first() {
    let mut base = document_with_run("body");
    let second = Inline::Text {
        id: StableId::parse("inline-second").unwrap(),
        text: "second".to_string(),
        marks: Vec::new(),
    };
    base.blocks[0].content.push(second);
    base.blocks.push(Block::paragraph("target"));
    let source_block_id = base.blocks[0].id.clone();
    let target_block_id = base.blocks[1].id.clone();
    let first_inline_id = crate::inline_ops::inline_id(&base.blocks[0].content[0]).clone();
    base.validate().unwrap();

    let inverse = kinds(invert_operation(
        &base,
        &OperationKind::MoveInlineToBlock {
            inline_id: first_inline_id.clone(),
            target_block_id,
            position: InsertPosition::Last,
        },
    ));
    match &inverse[..] {
        [OperationKind::MoveInlineToBlock {
            inline_id,
            target_block_id,
            position,
        }] => {
            assert_eq!(*inline_id, first_inline_id);
            assert_eq!(*target_block_id, source_block_id);
            assert_eq!(*position, InsertPosition::First);
        }
        other => panic!("expected one MoveInlineToBlock, got {other:?}"),
    }
}

/// The last of the four: a table cell. `InsertTableCell`'s anchor is a
/// *cell*, so it had the same gap, and it closes the same way.
#[test]
fn deleting_the_first_of_several_table_cells_inverts_to_reinserting_it_first() {
    let mut base = Document::new("Doc");
    let table_block_id = StableId::parse("table-block").unwrap();
    let row_id = StableId::parse("row-1").unwrap();
    let first_cell_id = StableId::parse("cell-1").unwrap();
    let second_cell_id = StableId::parse("cell-2").unwrap();
    base.blocks.push(Block {
        id: table_block_id.clone(),
        kind: BlockKind::table(vec![opendoc_core::TableRow {
            id: row_id.clone(),
            height: None,
            header: false,
            cells: vec![cell(&first_cell_id, "left"), cell(&second_cell_id, "right")],
        }]),
        content: Vec::new(),
        properties: BlockProperties::default(),
    });
    base.validate().expect("the fixture is valid");

    let inverse = kinds(invert_operation(
        &base,
        &OperationKind::DeleteTableCell {
            table_block_id: table_block_id.clone(),
            row_id: row_id.clone(),
            cell_id: first_cell_id.clone(),
        },
    ));
    match &inverse[..] {
        [OperationKind::InsertTableCell {
            table_block_id: table,
            row_id: row,
            position,
            cell,
        }] => {
            assert_eq!(*table, table_block_id);
            assert_eq!(*row, row_id);
            assert_eq!(*position, InsertPosition::First);
            assert_eq!(cell.id, first_cell_id);
        }
        other => panic!("expected one InsertTableCell, got {other:?}"),
    }
}

fn cell(id: &StableId, text: &str) -> opendoc_core::TableCell {
    opendoc_core::TableCell {
        id: id.clone(),
        span: opendoc_core::CellSpan::SINGLE,
        properties: Default::default(),
        blocks: vec![Block::paragraph(text)],
    }
}

/// The only block in the document is also the last one, so appending puts it
/// back exactly where it was.
#[test]
fn deleting_the_only_block_is_invertible() {
    let base = document_with_run("body");
    assert!(invert_operation(
        &base,
        &OperationKind::DeleteBlock {
            block_id: block_id()
        }
    )
    .is_expressible());
}

// ---------------------------------------------------------------------------
// The rest of the vocabulary.
// ---------------------------------------------------------------------------

#[test]
fn setting_a_property_inverts_to_the_value_it_overwrote() {
    let mut base = document_with_run("body");
    base.blocks[0].properties = BlockProperties::default();
    base.blocks[0]
        .properties
        .set(BlockProperty::Alignment(Alignment::Center));

    assert_eq!(
        kinds(invert_operation(
            &base,
            &OperationKind::SetBlockProperty {
                block_id: block_id(),
                property: BlockProperty::Alignment(Alignment::End),
            }
        )),
        vec![OperationKind::SetBlockProperty {
            block_id: block_id(),
            property: BlockProperty::Alignment(Alignment::Center),
        }]
    );
}

/// A property that was inheriting inverts to clearing it, not to writing the
/// default — those are different states, and only one of them keeps following
/// the document's style.
#[test]
fn setting_a_property_that_was_inheriting_inverts_to_clearing_it() {
    let base = document_with_run("body");
    assert_eq!(
        kinds(invert_operation(
            &base,
            &OperationKind::SetBlockProperty {
                block_id: block_id(),
                property: BlockProperty::Alignment(Alignment::End),
            }
        )),
        vec![OperationKind::ClearBlockProperty {
            block_id: block_id(),
            key: BlockPropertyKey::Alignment,
        }]
    );
}

#[test]
fn a_style_change_inverts_to_the_previous_style() {
    let mut base = document_with_run("body");
    base.blocks[0].kind = BlockKind::Heading { level: 2 };
    assert_eq!(
        kinds(invert_operation(
            &base,
            &OperationKind::SetBlockTextStyle {
                block_id: block_id(),
                style: BlockTextStyle::ListItem {
                    list_id: StableId::parse("list-1").unwrap(),
                    level: 0,
                    kind: ListKind::Bullet,
                },
            }
        )),
        vec![OperationKind::SetBlockTextStyle {
            block_id: block_id(),
            style: BlockTextStyle::Heading { level: 2 },
        }]
    );
}

#[test]
fn adding_a_mark_inverts_to_removing_it_and_removing_one_inverts_to_adding_it() {
    let mut base = document_with_run("body");
    let bold = Mark {
        kind: MarkKind::Bold,
        value: None,
        expand: MarkExpand::None,
    };
    assert_eq!(
        kinds(invert_operation(
            &base,
            &OperationKind::AddMark {
                text_id: run_id(),
                mark: bold.clone(),
            }
        )),
        vec![OperationKind::RemoveMark {
            text_id: run_id(),
            kind: MarkKind::Bold,
            value: None,
        }]
    );

    if let Inline::Text { marks, .. } = &mut base.blocks[0].content[0] {
        marks.push(bold.clone());
    }
    assert_eq!(
        kinds(invert_operation(
            &base,
            &OperationKind::RemoveMark {
                text_id: run_id(),
                kind: MarkKind::Bold,
                value: None,
            }
        )),
        vec![OperationKind::AddMark {
            text_id: run_id(),
            mark: bold,
        }]
    );
}

/// A mark that was already there was not added, so undoing the add must not
/// remove it. `Nothing` is the answer, and it is not the same as an inverse
/// that happens to do nothing.
#[test]
fn adding_a_mark_that_was_already_there_inverts_to_nothing() {
    let mut base = document_with_run("body");
    if let Inline::Text { marks, .. } = &mut base.blocks[0].content[0] {
        marks.push(Mark {
            kind: MarkKind::Bold,
            value: None,
            expand: MarkExpand::None,
        });
    }
    assert_eq!(
        invert_operation(
            &base,
            &OperationKind::AddMark {
                text_id: run_id(),
                mark: Mark {
                    kind: MarkKind::Bold,
                    value: None,
                    expand: MarkExpand::None,
                },
            }
        ),
        Inversion::Nothing
    );
}

/// A suggestion cannot be withdrawn and a resolution cannot be reopened. Both
/// are named refusals rather than silent no-ops: an undo that quietly did
/// nothing would be worse than one that says it cannot.
#[test]
fn suggestions_name_what_they_cannot_undo() {
    let base = document_with_run("body");
    let suggestion = opendoc_core::Suggestion {
        id: StableId::parse("sug-1").unwrap(),
        author: "a".to_string(),
        kind: opendoc_core::SuggestionKind::Insert {
            anchor: opendoc_core::Anchor::Document,
            content: vec![Inline::text("x")],
        },
        state: opendoc_core::SuggestionState::Proposed,
        provenance: Vec::new(),
    };
    assert_eq!(
        invert_operation(&base, &OperationKind::AddSuggestion { suggestion }),
        Inversion::Irreversible("no-operation-withdraws-a-suggestion")
    );
    assert_eq!(
        invert_operation(
            &base,
            &OperationKind::AcceptSuggestion {
                suggestion_id: StableId::parse("sug-1").unwrap(),
                accepted_by: "a".to_string(),
            }
        ),
        Inversion::Irreversible("suggestion-resolution-is-terminal")
    );
}

/// Every arm round-trips: applying an operation and then its inverse gives
/// back the document byte for byte.
#[test]
fn applying_an_operation_and_then_its_inverse_restores_the_document() {
    let base = document_with_run("abcdef");
    let cases: Vec<OperationKind> = vec![
        OperationKind::SetDocumentTitle {
            title: "Other".to_string(),
        },
        OperationKind::SetDocumentLocale {
            locale: "sv-SE".to_string(),
        },
        OperationKind::SetDocumentDoi {
            doi: Some("10.1/x".to_string()),
        },
        OperationKind::UpdateInlineText {
            inline_id: run_id(),
            text: "rewritten".to_string(),
        },
        OperationKind::SetBlockTextStyle {
            block_id: block_id(),
            style: BlockTextStyle::Heading { level: 3 },
        },
        OperationKind::SetBlockProperty {
            block_id: block_id(),
            property: BlockProperty::Alignment(Alignment::Center),
        },
        OperationKind::AddMark {
            text_id: run_id(),
            mark: Mark {
                kind: MarkKind::Italic,
                value: None,
                expand: MarkExpand::None,
            },
        },
        OperationKind::InsertBlock {
            position: InsertPosition::After(block_id()),
            block: Block::paragraph("added"),
        },
        OperationKind::DeleteBlock {
            block_id: block_id(),
        },
        OperationKind::SetPageSetup {
            page_setup: opendoc_core::PageSetup::from_size_name("a4").unwrap(),
        },
    ];
    let expected = opendoc_format::encode_canonical_cbor(&base).unwrap();
    for kind in cases {
        // `Nothing` is a legitimate answer — the operation changed nothing, so
        // the round trip is the empty one — but `Irreversible` is not, for any
        // of these.
        let inverse = match invert_operation(&base, &kind) {
            Inversion::Operations(kinds) => kinds,
            Inversion::Nothing => Vec::new(),
            other => panic!("{kind:?} must have an inverse, got {other:?}"),
        };
        let mut set = vec![op("a", 1, kind.clone(), &[])];
        for (index, inverse_kind) in inverse.into_iter().enumerate() {
            let observing = set.clone();
            set.push(op("a", 2 + index as u64, inverse_kind, &observing));
        }
        let mut result = merged(&base, &set);
        // The merge records its degradations on the document; they are not
        // part of the state an undo is answerable for.
        result.warnings = base.warnings.clone();
        assert_eq!(
            opendoc_format::encode_canonical_cbor(&result).unwrap(),
            expected,
            "applying {kind:?} and its inverse did not restore the document"
        );
    }
}

/// [`discarded_by_a_later_whole_run_write`] answers the one question an
/// incremental fold over a batch cannot ask for itself: which of these
/// operations will the merge of the whole batch throw away.
///
/// Stated directly, with a flag per position, so the rule is pinned by
/// something other than a document coming back after an undo. The batch mixes
/// both whole-run writes the vocabulary has — an `UpdateInlineText` and an
/// `InsertBlock` carrying a run — a run nothing rewrites, and an edit either
/// side of a rewrite.
#[test]
fn the_operations_a_batch_discards_are_the_character_edits_a_later_whole_run_write_covers() {
    let other = StableId::parse("inl-other").unwrap();
    let reinserted = StableId::parse("inl-reinserted").unwrap();
    let batch = [
        // 0: lost to the rewrite at 3.
        OperationKind::InsertText {
            inline_id: run_id(),
            offset: 0,
            text: "a".to_string(),
        },
        // 1: a run nothing in this batch writes wholesale.
        OperationKind::DeleteText {
            inline_id: other.clone(),
            start: 0,
            end: 1,
        },
        // 2: not offset-addressed, so never discarded.
        OperationKind::SetDocumentTitle {
            title: "T".to_string(),
        },
        // 3: the rewrite.
        OperationKind::UpdateInlineText {
            inline_id: run_id(),
            text: "rewritten".to_string(),
        },
        // 4: ordered after the rewrite, so it survives it — and is then lost
        // to the block inserted at 6, which carries the same run.
        OperationKind::InsertText {
            inline_id: run_id(),
            offset: 0,
            text: "b".to_string(),
        },
        // 5: the reinserted block's run, edited before the block arrives.
        OperationKind::DeleteText {
            inline_id: reinserted.clone(),
            start: 0,
            end: 2,
        },
        // 6: a whole-run write for both `run_id()` and `reinserted`.
        OperationKind::InsertBlock {
            position: InsertPosition::Last,
            block: Block {
                id: StableId::parse("blk-two").unwrap(),
                kind: BlockKind::Paragraph,
                properties: BlockProperties::default(),
                content: vec![
                    Inline::Text {
                        id: run_id(),
                        text: "carried".to_string(),
                        marks: Vec::new(),
                    },
                    Inline::Text {
                        id: reinserted,
                        text: "also carried".to_string(),
                        marks: Vec::new(),
                    },
                ],
            },
        },
        // 7: after every write in the batch.
        OperationKind::InsertText {
            inline_id: other,
            offset: 0,
            text: "c".to_string(),
        },
    ];

    assert_eq!(
        crate::discarded_by_a_later_whole_run_write(batch.iter()),
        vec![true, false, false, false, true, true, false, false],
    );
}
