//! What `apply_batch`'s scratch fold must keep producing.
//!
//! The fold exists for one reason: an inverse has to be taken against the
//! state its own operation applied to, which for the second operation of a
//! batch is a state that exists only inside the batch (ADR 0017). Making that
//! fold cheaper is only worth anything if it produces the *same* states, and a
//! change that quietly shifted one inverse would break undo in a way nobody
//! notices until they undo.
//!
//! So the oracle here is the fold the optimisation replaced, written out in
//! full in [`copying_fold`]: a fresh copy of the document per operation via
//! [`merge_operations`], and the causal context rebuilt from the whole journal
//! per operation. It shares `invert_operation` and the merge itself with the
//! code under test — those did not change and are not what is being checked —
//! but not one line of the fold, which is what did. The comparison is over the
//! three things the fold decides: the operations it mints (identity, payload
//! **and causal context**), the inverse captured for each, and the document
//! the batch lands.

use crate::document_service::DocumentOperationService;
use crate::{AppOperationEnvelope, OpenDocApp};
use opendoc_core::{
    Alignment, Block, BlockKind, BlockProperty, BlockPropertyKey, Document, Inline, InsertPosition,
    Length, Mark, MarkExpand, MarkKind, StableId,
};
use opendoc_merge::{
    invert_operation, merge_operations, ActorId, CausalContext, Inversion, Operation, OperationId,
    OperationKind,
};
use serde_json::json;
use std::collections::BTreeMap;

/// A batch entry as the generator produces it: the summary and the operation.
type PlannedOperation = (&'static str, OperationKind);

/// The fold exactly as it was before it was made cheaper.
///
/// Per operation: invert against the scratch, mint the operation in a context
/// rebuilt by scanning everything applied so far, fold it into a **copy** of
/// the scratch, and — when that copy refuses to validate — stop trusting every
/// inverse from the next operation on.
///
/// It skips the operations the batch discards, for the reason the fold under
/// test does, and it decides which those are with [`discarded_by_the_batch`] —
/// this file's own reading of ADR 0007's reset rule, not the crate's.
fn copying_fold(
    document: &Document,
    envelopes: &[AppOperationEnvelope],
    actor: &str,
    next_operation_seq: u64,
    operations: &[PlannedOperation],
) -> (Vec<Operation>, Vec<Inversion>) {
    let mut applied = envelopes
        .iter()
        .filter_map(|envelope| envelope.operation.clone())
        .collect::<Vec<_>>();
    let mut minted = Vec::new();
    for (offset, (_summary, kind)) in operations.iter().enumerate() {
        let op = Operation::in_context(
            OperationId {
                actor: ActorId(actor.to_string()),
                seq: next_operation_seq + offset as u64,
            },
            kind.clone(),
            CausalContext::observing(applied.iter()),
        );
        applied.push(op.clone());
        minted.push(op);
    }
    let mut scratch = document.clone();
    let mut inversions = (0..operations.len()).map(|_| None).collect::<Vec<_>>();
    let mut capture_complete = true;
    let discarded = discarded_by_the_batch(operations);
    for offset in capture_order_from_adr(operations) {
        let (_summary, kind) = &operations[offset];
        inversions[offset] = Some(if capture_complete {
            match kind {
                OperationKind::InsertText { inline_id, .. }
                | OperationKind::DeleteText { inline_id, .. }
                    if crate::document_tree::find_inline_in_blocks(&scratch.blocks, inline_id)
                        .is_none() =>
                {
                    Inversion::Irreversible("batch-text-target-removed-before-deferred-pass")
                }
                _ => invert_operation(&scratch, kind),
            }
        } else {
            Inversion::Irreversible("batch-inverse-capture-incomplete")
        });
        if capture_complete && !discarded[offset] {
            match merge_operations(&scratch, &[vec![minted[offset].clone()]]) {
                Ok(result) => scratch = result.document,
                Err(_) => {
                    capture_complete = false;
                }
            }
        }
    }
    let inversions = inversions
        .into_iter()
        .map(|inversion| inversion.expect("every batch operation is captured once"))
        .collect();
    (minted, inversions)
}

/// The deferred-pass order as ADR 0007 describes it.  This stays independent
/// from `opendoc_merge::batch_inverse_capture_order`: the randomized oracle is
/// meant to catch a phase silently moving in the production fold.
fn capture_order_from_adr(operations: &[PlannedOperation]) -> Vec<usize> {
    let mut ordinary = Vec::new();
    let mut suggestion_resolutions = Vec::new();
    let mut comment_restores = Vec::new();
    let mut mark_ranges = Vec::new();
    let mut text_edits = Vec::new();
    for (index, (_summary, kind)) in operations.iter().enumerate() {
        match kind {
            OperationKind::AcceptSuggestion { .. } | OperationKind::RejectSuggestion { .. } => {
                suggestion_resolutions.push(index)
            }
            OperationKind::RestoreCommentThread { .. } | OperationKind::RestoreComment { .. } => {
                comment_restores.push(index)
            }
            OperationKind::AddMarkRange { .. } => mark_ranges.push(index),
            OperationKind::InsertText { .. } | OperationKind::DeleteText { .. } => {
                text_edits.push(index)
            }
            _ => ordinary.push(index),
        }
    }
    ordinary.extend(suggestion_resolutions);
    ordinary.extend(comment_restores);
    ordinary.extend(mark_ranges);
    ordinary.extend(text_edits);
    ordinary
}

/// Which operations of a batch the merge throws away, read straight off
/// ADR 0007 rather than out of `opendoc-merge`.
///
/// "A whole-run write resets the run": a character operation loses to any
/// *later* operation in the same batch whose payload writes the whole of the
/// run it addresses. This is deliberately a second, independent statement of
/// that rule — if `discarded_by_a_later_whole_run_write` and this one ever
/// disagree about a generated batch, the comparison above says so.
fn discarded_by_the_batch(operations: &[PlannedOperation]) -> Vec<bool> {
    let mut written_wholesale: BTreeMap<StableId, usize> = BTreeMap::new();
    for (rank, (_summary, kind)) in operations.iter().enumerate() {
        let runs = match kind {
            OperationKind::UpdateInlineText { inline_id, .. } => vec![inline_id.clone()],
            OperationKind::InsertInline {
                inline: Inline::Text { id, .. } | Inline::Link { id, .. },
                ..
            } => vec![id.clone()],
            OperationKind::InsertBlock { block, .. } => runs_of(std::slice::from_ref(block)),
            OperationKind::InsertTableRow { row, .. } => row
                .cells
                .iter()
                .flat_map(|cell| runs_of(&cell.blocks))
                .collect(),
            OperationKind::InsertTableCell { cell, .. } => runs_of(&cell.blocks),
            _ => Vec::new(),
        };
        for run in runs {
            written_wholesale.insert(run, rank);
        }
    }
    operations
        .iter()
        .enumerate()
        .map(|(rank, (_summary, kind))| match kind {
            OperationKind::InsertText { inline_id, .. }
            | OperationKind::DeleteText { inline_id, .. } => written_wholesale
                .get(inline_id)
                .is_some_and(|write| rank < *write),
            _ => false,
        })
        .collect()
}

/// Every text run `blocks` carries, table cells included.
fn runs_of(blocks: &[Block]) -> Vec<StableId> {
    let mut found = Vec::new();
    for block in blocks {
        for inline in &block.content {
            if let Inline::Text { id, .. } | Inline::Link { id, .. } = inline {
                found.push(id.clone());
            }
        }
        if let BlockKind::Table { rows, .. } = &block.kind {
            for row in rows {
                for cell in &row.cells {
                    found.extend(runs_of(&cell.blocks));
                }
            }
        }
    }
    found
}

/// A tiny deterministic generator. Not `rand`: the seeds have to reproduce a
/// failure exactly on another machine and in another year.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }
}

/// A document with enough shape for the operations below to bite: several
/// paragraphs, a heading, runs that already carry marks, and blocks that
/// already carry properties, so `AddMark`/`RemoveMark`/`SetBlockProperty`
/// inverses have a previous value to capture rather than always inverting to
/// nothing.
fn fixture(seed: u64) -> OpenDocApp {
    let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
    let mut app = OpenDocApp::new_empty_document();
    app.dispatch_command("create_document", json!({ "title": "Fold fixture" }))
        .expect("create");
    for index in 0..4 + rng.below(4) {
        app.dispatch_command(
            "add_paragraph",
            json!({ "text": format!("block {index} with words") }),
        )
        .expect("paragraph");
    }
    let inline_ids = text_run_ids(&app.document);
    let block_ids = block_ids(&app.document);
    for _ in 0..2 {
        let id = inline_ids[rng.below(inline_ids.len())].clone();
        let _ = app.apply_batch(vec![(
            "add-mark",
            "fixture mark",
            OperationKind::AddMark {
                text_id: id,
                mark: Mark {
                    kind: MarkKind::Italic,
                    value: None,
                    expand: MarkExpand::None,
                },
            },
        )]);
        let id = block_ids[rng.below(block_ids.len())].clone();
        let _ = app.apply_batch(vec![(
            "set-block-property",
            "fixture property",
            OperationKind::SetBlockProperty {
                block_id: id,
                property: BlockProperty::Alignment(Alignment::Center),
            },
        )]);
    }
    app
}

fn block_ids(document: &Document) -> Vec<StableId> {
    document
        .blocks
        .iter()
        .map(|block| block.id.clone())
        .collect()
}

fn text_run_ids(document: &Document) -> Vec<StableId> {
    document
        .blocks
        .iter()
        .flat_map(|block| block.content.iter())
        .filter_map(|inline| match inline {
            Inline::Text { id, .. } => Some(id.clone()),
            _ => None,
        })
        .collect()
}

fn text_run(document: &Document, id: &StableId) -> String {
    document
        .blocks
        .iter()
        .flat_map(|block| block.content.iter())
        .find_map(|inline| match inline {
            Inline::Text {
                id: other, text, ..
            } if other == id => Some(text.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

/// One operation drawn against the document as it stands. The draw is shallow
/// on purpose — it does not simulate the batch — so later operations in a
/// batch routinely address state an earlier one changed or removed, which is
/// the case the fold exists for.
fn draw_operation(rng: &mut Rng, document: &Document) -> Option<PlannedOperation> {
    let blocks = block_ids(document);
    let runs = text_run_ids(document);
    if blocks.is_empty() || runs.is_empty() {
        return None;
    }
    let block = blocks[rng.below(blocks.len())].clone();
    let run = runs[rng.below(runs.len())].clone();
    let text = text_run(document, &run);
    let marks = [
        MarkKind::Bold,
        MarkKind::Italic,
        MarkKind::Underline,
        MarkKind::Code,
    ];
    Some(match rng.below(13) {
        0 => (
            "add-mark",
            OperationKind::AddMark {
                text_id: run,
                mark: Mark {
                    kind: marks[rng.below(marks.len())].clone(),
                    value: None,
                    expand: MarkExpand::None,
                },
            },
        ),
        1 => (
            "remove-mark",
            OperationKind::RemoveMark {
                text_id: run,
                kind: marks[rng.below(marks.len())].clone(),
                value: None,
            },
        ),
        2 => (
            "set-block-property",
            OperationKind::SetBlockProperty {
                block_id: block,
                property: match rng.below(2) {
                    0 => BlockProperty::Alignment(Alignment::ALL[rng.below(Alignment::ALL.len())]),
                    _ => BlockProperty::IndentStart(
                        Length::from_twips(20 * rng.below(40) as i32).expect("in range"),
                    ),
                },
            },
        ),
        3 => (
            "clear-block-property",
            OperationKind::ClearBlockProperty {
                block_id: block,
                key: BlockPropertyKey::ALL[rng.below(BlockPropertyKey::ALL.len())],
            },
        ),
        4 => (
            "update-inline-text",
            OperationKind::UpdateInlineText {
                inline_id: run,
                text: format!("rewritten {}", rng.below(1000)),
            },
        ),
        5 => (
            "insert-text",
            OperationKind::InsertText {
                inline_id: run,
                offset: rng.below(text.chars().count() + 1),
                text: format!("<{}>", rng.below(100)),
            },
        ),
        6 => {
            let len = text.chars().count();
            let start = rng.below(len + 1);
            let end = start + rng.below(len + 1 - start);
            (
                "delete-text",
                OperationKind::DeleteText {
                    inline_id: run,
                    start,
                    end,
                },
            )
        }
        7 => (
            "delete-block",
            OperationKind::DeleteBlock { block_id: block },
        ),
        8 => (
            "insert-block",
            OperationKind::InsertBlock {
                position: if rng.below(2) == 0 {
                    InsertPosition::First
                } else {
                    InsertPosition::After(block)
                },
                block: paragraph(&format!("inserted {}", rng.below(1000))),
            },
        ),
        9 => (
            "insert-inline",
            OperationKind::InsertInline {
                block_id: block,
                position: InsertPosition::Last,
                inline: Inline::Text {
                    id: StableId::new("text"),
                    text: format!("appended {}", rng.below(1000)),
                    marks: Vec::new(),
                },
            },
        ),
        10 => (
            "delete-inline",
            OperationKind::DeleteInline { inline_id: run },
        ),
        11 => (
            "set-document-title",
            OperationKind::SetDocumentTitle {
                title: format!("Retitled {}", rng.below(1000)),
            },
        ),
        _ => (
            "update-heading-level",
            OperationKind::UpdateHeadingLevel {
                block_id: block,
                level: 1 + rng.below(6) as u8,
            },
        ),
    })
}

fn paragraph(text: &str) -> Block {
    Block {
        id: StableId::new("block"),
        kind: BlockKind::Paragraph,
        content: vec![Inline::Text {
            id: StableId::new("text"),
            text: text.to_string(),
            marks: Vec::new(),
        }],
        properties: Default::default(),
    }
}

/// The pair that makes a batch pass through a state `Document::validate()`
/// rejects: a new block that reuses a live inline id, and then the removal of
/// the block that id belonged to. Legal as a gesture, illegal in between —
/// which is what puts an `Irreversible` in the middle of a batch's inverses.
fn shared_id_replacement(document: &Document) -> Option<Vec<PlannedOperation>> {
    let victim = document.blocks.iter().find(|block| {
        matches!(block.kind, BlockKind::Paragraph)
            && matches!(block.content.first(), Some(Inline::Text { .. }))
    })?;
    let Some(Inline::Text { id, .. }) = victim.content.first() else {
        return None;
    };
    Some(vec![
        (
            "insert-block",
            OperationKind::InsertBlock {
                position: InsertPosition::Last,
                block: Block {
                    id: StableId::new("block"),
                    kind: BlockKind::Paragraph,
                    content: vec![Inline::Text {
                        id: id.clone(),
                        text: "replacement".to_string(),
                        marks: Vec::new(),
                    }],
                    properties: Default::default(),
                },
            },
        ),
        (
            "delete-block",
            OperationKind::DeleteBlock {
                block_id: victim.id.clone(),
            },
        ),
    ])
}

/// A text operation is deferred until after ordinary structural operations.
/// This gesture removes its target in that ordinary pass, so the character
/// operation never lands and its local inverse must force the snapshot path.
fn deferred_text_target_removal(document: &Document) -> Option<Vec<PlannedOperation>> {
    let victim = document.blocks.iter().find_map(|block| {
        block.content.iter().find_map(|inline| match inline {
            Inline::Text { id, text, .. } if !text.is_empty() => Some(id.clone()),
            _ => None,
        })
    })?;
    Some(vec![
        (
            "insert-text-before-delete-inline",
            OperationKind::InsertText {
                inline_id: victim.clone(),
                offset: 0,
                text: "deferred".to_string(),
            },
        ),
        (
            "delete-inline-before-text-pass",
            OperationKind::DeleteInline { inline_id: victim },
        ),
    ])
}

fn has_deferred_text_target_removal(operations: &[(&str, &str, OperationKind)]) -> bool {
    operations.iter().any(|(_, _, kind)| {
        let (OperationKind::InsertText { inline_id, .. }
        | OperationKind::DeleteText { inline_id, .. }) = kind else {
            return false;
        };
        operations.iter().any(|(_, _, candidate)| {
            matches!(candidate, OperationKind::DeleteInline { inline_id: deleted } if deleted == inline_id)
        })
    })
}

/// A character edit to one run and then a write of that whole run — the shape
/// ADR 0007's reset rule throws the character edit away in, and the shape the
/// scratch fold used to mis-model.
///
/// Drawn deliberately rather than left to [`draw_operation`], which has to
/// land two of its thirteen draws on the same one of eight runs to produce it:
/// over 600 seeds that happened three times, which is too thin to prove
/// anything about a rule. Half the time a second character edit follows the
/// rewrite, which the reset does **not** discard, so the generator covers both
/// sides of "ordered before".
fn whole_run_rewrite_after_a_character_edit(
    rng: &mut Rng,
    document: &Document,
) -> Option<Vec<PlannedOperation>> {
    let runs = text_run_ids(document);
    if runs.is_empty() {
        return None;
    }
    let run = runs[rng.below(runs.len())].clone();
    let text = text_run(document, &run);
    let length = text.chars().count();
    let first = if rng.below(2) == 0 {
        (
            "insert-text",
            OperationKind::InsertText {
                inline_id: run.clone(),
                offset: rng.below(length + 1),
                text: format!("<{}>", rng.below(100)),
            },
        )
    } else {
        let start = rng.below(length + 1);
        (
            "delete-text",
            OperationKind::DeleteText {
                inline_id: run.clone(),
                start,
                end: start + rng.below(length + 1 - start),
            },
        )
    };
    let rewritten = format!("rewritten {}", rng.below(1000));
    let rewritten_length = rewritten.chars().count();
    let mut batch = vec![
        first,
        (
            "update-inline-text",
            OperationKind::UpdateInlineText {
                inline_id: run.clone(),
                text: rewritten,
            },
        ),
    ];
    if rng.below(2) == 0 {
        batch.push((
            "insert-text",
            OperationKind::InsertText {
                inline_id: run,
                offset: rng.below(rewritten_length + 1),
                text: format!("<{}>", rng.below(100)),
            },
        ));
    }
    Some(batch)
}

fn generate_batch(rng: &mut Rng, document: &Document) -> Vec<PlannedOperation> {
    // One seed in six deliberately removes a text operation's target before
    // the merge's final character pass. This is a different set-wide
    // deferral from the whole-run reset below; keeping it generated and
    // counted makes the snapshot-fallback proof non-vacuous.
    if rng.below(6) == 0 {
        if let Some(batch) = deferred_text_target_removal(document) {
            return batch;
        }
    }
    // One seed in five is the shape that makes an intermediate state invalid,
    // with ordinary work either side of it, so the `Irreversible` tail is
    // generated rather than only asserted about once by hand.
    if rng.below(5) == 0 {
        if let Some(mut batch) = shared_id_replacement(document) {
            let mut head = (0..rng.below(2))
                .filter_map(|_| draw_operation(rng, document))
                .collect::<Vec<_>>();
            head.append(&mut batch);
            head.extend((0..1 + rng.below(3)).filter_map(|_| draw_operation(rng, document)));
            return head;
        }
    }
    // One seed in four is the whole-run reset, again with ordinary work either
    // side of it, so the rule the fold has to know about is generated rather
    // than waited for.
    if rng.below(4) == 0 {
        if let Some(mut pair) = whole_run_rewrite_after_a_character_edit(rng, document) {
            let mut head = (0..rng.below(3))
                .filter_map(|_| draw_operation(rng, document))
                .collect::<Vec<_>>();
            head.append(&mut pair);
            head.extend((0..rng.below(3)).filter_map(|_| draw_operation(rng, document)));
            return head;
        }
    }
    (0..1 + rng.below(6))
        .filter_map(|_| draw_operation(rng, document))
        .collect()
}

fn apply_through_the_service(
    app: &mut OpenDocApp,
    batch: &[PlannedOperation],
) -> Result<(), crate::AppApiError> {
    // The service, not `OpenDocApp::apply_batch`: the wrapper also repairs
    // adjacent list runs, which mints operations of its own and would put work
    // in the journal the oracle knows nothing about.
    DocumentOperationService::new(
        &app.actor_id,
        &mut app.document,
        &mut app.operation_journal,
        &mut app.operation_envelopes,
        &mut app.next_envelope_seq,
        &mut app.next_operation_seq,
        &mut app.operation_inverses,
    )
    .apply_batch(
        batch
            .iter()
            .map(|(summary, kind)| ("", *summary, kind.clone()))
            .collect(),
    )
}

const SEEDS: u64 = 600;

#[test]
fn the_fold_mints_the_same_operations_and_captures_the_same_inverses_as_the_copying_fold() {
    let mut irreversible_tails = 0usize;
    let mut expressible_inverses = 0usize;
    let mut deferred = 0usize;
    let mut refusals = 0usize;
    let mut compared = 0usize;
    let mut discards = 0usize;

    for seed in 0..SEEDS {
        let mut app = fixture(seed);
        let mut rng = Rng(seed.wrapping_mul(0xd1b5_4a32_d192_ed03) | 1);
        let batch = generate_batch(&mut rng, &app.document);
        if batch.is_empty() {
            continue;
        }

        // Every operation in the vocabulary guards its own payload, so a batch
        // the merge refuses outright cannot be generated — it has to be
        // induced. One seed in sixty-one edits the document into a state
        // `Document::validate()` rejects before the batch runs, which is the
        // shape of the refusal the fold has to pass through unchanged: nothing
        // journalled, nothing minted, the document as it was.
        if seed % 61 == 7 {
            app.document.locale = " en ".to_string();
        }
        discards += discarded_by_the_batch(&batch)
            .into_iter()
            .filter(|discarded| *discarded)
            .count();
        let before_document = app.document.clone();
        let before_envelopes = app.operation_envelopes.clone();
        let before_seq = app.next_operation_seq;
        let actor = app.actor_id.clone();
        let (expected_operations, expected_inversions) = copying_fold(
            &before_document,
            &before_envelopes,
            &actor,
            before_seq,
            &batch,
        );
        let expected_document =
            merge_operations(&before_document, std::slice::from_ref(&expected_operations))
                .map(|it| it.document);

        let applied = apply_through_the_service(&mut app, &batch);

        match (&expected_document, &applied) {
            (Err(_), Err(_)) => {
                refusals += 1;
                assert_eq!(
                    app.document, before_document,
                    "seed {seed}: a refused batch leaves the document alone"
                );
                continue;
            }
            (Ok(_), Err(error)) => panic!(
                "seed {seed}: the batch was refused but the copying fold landed it: {error:?}"
            ),
            (Err(error), Ok(())) => {
                panic!("seed {seed}: the batch landed but the copying fold refused it: {error:?}")
            }
            (Ok(_), Ok(())) => {}
        }
        let expected_document = expected_document.expect("checked above");

        let minted = app.operation_envelopes[before_envelopes.len()..]
            .iter()
            .filter_map(|envelope| envelope.operation.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            minted.len(),
            expected_operations.len(),
            "seed {seed}: the batch journalled a different number of operations"
        );
        for (minted, expected) in minted.iter().zip(&expected_operations) {
            assert_eq!(
                minted.id, expected.id,
                "seed {seed}: operation identity moved"
            );
            assert_eq!(
                minted.kind, expected.kind,
                "seed {seed}: operation payload moved for {:?}",
                minted.id
            );
            // The causal context is the half of this that a carried clock
            // could get wrong while the document still looked right.
            assert_eq!(
                minted.context, expected.context,
                "seed {seed}: causal context moved for {:?}",
                minted.id
            );
        }

        for (expected_op, expected_inversion) in
            expected_operations.iter().zip(&expected_inversions)
        {
            let captured = app.operation_inverses.get(&expected_op.id);
            assert_eq!(
                captured,
                Some(expected_inversion),
                "seed {seed}: the inverse captured for {:?} ({:?}) is not the one the copying fold captured",
                expected_op.id,
                expected_op.kind
            );
            match expected_inversion {
                Inversion::Irreversible(_) => irreversible_tails += 1,
                Inversion::Operations(_) => expressible_inverses += 1,
                Inversion::Deferred => deferred += 1,
                Inversion::Nothing => {}
            }
        }

        assert_eq!(
            app.document, expected_document,
            "seed {seed}: the batch landed a different document"
        );
        compared += 1;
    }

    // A generator that stopped producing anything interesting would make every
    // assertion above vacuous, so each thing the fold has to get right has to
    // have actually happened.
    assert!(
        compared >= SEEDS as usize / 2,
        "most seeds must produce a batch that lands: {compared}"
    );
    assert!(
        expressible_inverses >= 500,
        "inverses that carry captured state have to be exercised: {expressible_inverses}"
    );
    assert!(
        deferred >= 50,
        "offset-addressed operations have to be exercised: {deferred}"
    );
    assert!(
        irreversible_tails >= 50,
        "the invalid-intermediate path has to be exercised: {irreversible_tails}"
    );
    assert!(
        refusals >= 5,
        "a batch the document refuses outright has to be exercised: {refusals}"
    );
    // The fold and the copying fold agree about an operation nobody discards
    // whatever either of them believes about discarding, so without this the
    // comparison says nothing about the rule.
    assert!(
        discards >= 100,
        "batches in which the merge discards an operation have to be \
         exercised: {discards}"
    );
}

/// The fold is an implementation detail; undo is what it is *for*. So this
/// does not compare structures at all: it takes a document, runs a batch over
/// it, undoes the batch, and requires the document back.
///
/// The count that matters is `by_inverse`. ADR 0017 keeps the whole-state
/// snapshot as a fallback for a step the vocabulary cannot invert, and a
/// restored snapshot gives the document back *whatever* the captured inverses
/// say — so a test that only asserted the document came back would pass just
/// as well with every inverse wrong. An undo that went through the inverses
/// leaves new operations in the journal behind it; a restored snapshot puts
/// the journal back. That is what tells the two apart here.
#[test]
fn undoing_a_generated_batch_gives_the_document_back() {
    let mut by_inverse = 0usize;
    let mut by_snapshot = 0usize;
    // The shape this test used to *exclude*, because undoing it added text:
    // a whole-run write ordered after a character edit to the same run. It is
    // counted rather than skipped now, and the count is asserted, because a
    // generator that stopped producing it would make the fix look proved
    // while proving nothing.
    let mut discards = 0usize;
    let mut removed_text_targets = 0usize;
    for seed in 0..300u64 {
        let mut app = fixture(seed);
        let mut rng = Rng(seed.wrapping_mul(0x94d0_49bb_1331_11eb) | 1);
        let batch = generate_batch(&mut rng, &app.document)
            .into_iter()
            .map(|(summary, kind)| ("", summary, kind))
            .collect::<Vec<_>>();
        if batch.is_empty() {
            continue;
        }
        discards += discarded_by_the_batch(
            &batch
                .iter()
                .map(|(_, summary, kind)| (*summary, kind.clone()))
                .collect::<Vec<_>>(),
        )
        .into_iter()
        .filter(|discarded| *discarded)
        .count();
        removed_text_targets += usize::from(has_deferred_text_target_removal(&batch));
        let before = app.document.clone();
        // What `dispatch_command` does around an undoable command, and the
        // only part of it this test needs.
        let checkpoint = app.checkpoint();
        if app.apply_batch(batch).is_err() {
            continue;
        }
        if app.document == before {
            continue;
        }
        app.undo_stack.push(checkpoint);
        app.redo_stack.clear();
        let journalled_by_the_batch = app.operation_journal.len();

        app.undo_current_edit()
            .unwrap_or_else(|error| panic!("seed {seed}: the undo was refused: {error:?}"));

        assert_eq!(
            app.document.blocks, before.blocks,
            "seed {seed}: undoing the batch did not give the blocks back"
        );
        assert_eq!(
            app.document.title, before.title,
            "seed {seed}: undoing the batch did not give the title back"
        );
        if app.operation_journal.len() > journalled_by_the_batch {
            by_inverse += 1;
        } else {
            by_snapshot += 1;
        }
    }
    assert!(
        by_inverse >= 150,
        "most undos have to go through the captured inverses rather than the \
         snapshot fallback, or this test cannot see a wrong inverse: \
         {by_inverse} by inverse, {by_snapshot} by snapshot"
    );
    assert!(
        by_snapshot >= 1,
        "the snapshot fallback has to be exercised too: {by_snapshot}"
    );
    assert!(
        discards >= 40,
        "a batch whose merge discards a character operation has to be \
         exercised, or the shape this test was written for is gone: {discards}"
    );
    assert!(
        removed_text_targets >= 30,
        "a deferred text edit whose target disappears before its pass must be generated: {removed_text_targets}"
    );
}

/// The shape the fold used to get wrong, named and pinned.
///
/// ADR 0007: a whole-run write resets the run, so the batch merge throws the
/// `InsertText` away and the document never holds `<51>` at all. The fold that
/// captures inverses merges one operation at a time, where the `InsertText` is
/// alone in its own merge and has nothing to lose to — so it used to apply it,
/// and the `UpdateInlineText` that follows captured `block 3 with w<51>ords`
/// as the value it overwrote. That is a state the document was never in. At
/// undo the character operation correctly inverts to nothing ("a whole-run
/// rewrite ordered after the operation discarded it"), so nothing took those
/// characters back out and **the undo added text**.
///
/// Every value asserted here is a literal, including the captured inverse:
/// reading the previous value back out of the document under test would pass
/// just as well with the inverse wrong.
#[test]
fn a_character_edit_the_batch_discards_is_not_in_the_state_the_next_inverse_captures() {
    let mut app = OpenDocApp::new_empty_document();
    app.dispatch_command("create_document", json!({ "title": "Discards" }))
        .expect("create");
    app.dispatch_command("add_paragraph", json!({ "text": "block 3 with words" }))
        .expect("paragraph");
    let run = text_run_ids(&app.document)
        .into_iter()
        .find(|id| text_run(&app.document, id) == "block 3 with words")
        .expect("the paragraph's run");
    let before = app.document.clone();
    let checkpoint = app.checkpoint();

    app.apply_batch(vec![
        (
            "insert-text",
            "type",
            OperationKind::InsertText {
                inline_id: run.clone(),
                offset: 14,
                text: "<51>".to_string(),
            },
        ),
        (
            "update-inline-text",
            "rewrite the run",
            OperationKind::UpdateInlineText {
                inline_id: run.clone(),
                text: "rewritten 374".to_string(),
            },
        ),
    ])
    .expect("the batch lands");

    assert_eq!(
        text_run(&app.document, &run),
        "rewritten 374",
        "the whole-run write won, so the inserted characters are not in the document"
    );

    let rewrite = app
        .operation_envelopes
        .iter()
        .rev()
        .find_map(|envelope| envelope.operation.as_ref())
        .expect("the rewrite is the last operation journalled")
        .id
        .clone();
    assert_eq!(
        app.operation_inverses.get(&rewrite),
        Some(&Inversion::Operations(vec![
            OperationKind::UpdateInlineText {
                inline_id: run.clone(),
                text: "block 3 with words".to_string(),
            }
        ])),
        "the rewrite's inverse restores the run the document really had, not the \
         one only the fold ever saw"
    );

    app.undo_stack.push(checkpoint);
    app.redo_stack.clear();
    let journalled = app.operation_journal.len();
    app.undo_current_edit().expect("the undo is expressible");

    assert!(
        app.operation_journal.len() > journalled,
        "the undo went through the captured inverses, not the snapshot fallback"
    );
    assert_eq!(
        text_run(&app.document, &run),
        "block 3 with words",
        "the undo gave the run back instead of adding characters to it"
    );
    assert_eq!(app.document.blocks, before.blocks);
}

/// The other direction of the same rule, which is what stops "discard every
/// character operation on a run something rewrites" from passing for it: a
/// character edit ordered **after** the whole-run write is not discarded, and
/// both it and the write have to be undone.
#[test]
fn a_character_edit_after_the_whole_run_write_survives_the_batch_and_is_undone() {
    let mut app = OpenDocApp::new_empty_document();
    app.dispatch_command("create_document", json!({ "title": "Survives" }))
        .expect("create");
    app.dispatch_command("add_paragraph", json!({ "text": "block 3 with words" }))
        .expect("paragraph");
    let run = text_run_ids(&app.document)
        .into_iter()
        .find(|id| text_run(&app.document, id) == "block 3 with words")
        .expect("the paragraph's run");
    let before = app.document.clone();
    let checkpoint = app.checkpoint();

    app.apply_batch(vec![
        (
            "update-inline-text",
            "rewrite the run",
            OperationKind::UpdateInlineText {
                inline_id: run.clone(),
                text: "rewritten 374".to_string(),
            },
        ),
        (
            "insert-text",
            "type",
            OperationKind::InsertText {
                inline_id: run.clone(),
                offset: 10,
                text: "<51>".to_string(),
            },
        ),
    ])
    .expect("the batch lands");
    assert_eq!(
        text_run(&app.document, &run),
        "rewritten <51>374",
        "the character edit came after the rewrite, so it survives it"
    );

    app.undo_stack.push(checkpoint);
    app.redo_stack.clear();
    let journalled = app.operation_journal.len();
    app.undo_current_edit().expect("the undo is expressible");

    assert!(
        app.operation_journal.len() > journalled,
        "the undo went through the captured inverses, not the snapshot fallback"
    );
    assert_eq!(text_run(&app.document, &run), "block 3 with words");
    assert_eq!(app.document.blocks, before.blocks);
}

/// Whole-document work must not grow with the length of one gesture.
///
/// This is deliberately **not** a wall-clock assertion: those are flaky on a
/// shared machine and say nothing about why they failed. It counts the two
/// things the fold used to do once per operation and now does once per batch —
/// copy the whole document, and rebuild the causal clock by scanning the whole
/// journal — and compares one batch against another eight times as long. The
/// numbers the implementation happens to produce are not written down
/// anywhere here; what is asserted is that eight times the work does not cost
/// eight times the copies.
///
/// Both counters have a floor, because a counter that stopped counting would
/// otherwise sail through a ratio of 0 to 0.
#[test]
fn one_gesture_copies_the_document_a_fixed_number_of_times_however_long_it_is() {
    const SHORT: usize = 50;
    const LONG: usize = 400;

    let one = whole_document_work_for_a_batch_of(1);
    let short = whole_document_work_for_a_batch_of(SHORT);
    let long = whole_document_work_for_a_batch_of(LONG);

    // The gesture the editor actually sends most of the time. One copy is the
    // merge that lands it; a second would be a scratch document taken to fold
    // a batch that has nothing to fold — there is no operation after the only
    // operation for an intermediate state to matter to.
    assert_eq!(
        one.document_copies, 1,
        "a one-operation gesture copies the whole document more than once: {one:?}"
    );

    assert!(
        short.document_copies >= 1,
        "the copy counter is dead, so the ratio below proves nothing: {short:?}"
    );
    assert!(
        short.causal_contexts_built >= 1,
        "the causal-context counter is dead: {short:?}"
    );
    assert!(
        long.document_copies <= short.document_copies * 2,
        "a batch {}x as long copied the document {}x as often — the fold is \
         copying per operation again: {short:?} then {long:?}",
        LONG / SHORT,
        long.document_copies as f64 / short.document_copies as f64
    );
    assert!(
        long.causal_contexts_built <= short.causal_contexts_built * 2,
        "a batch {}x as long rebuilt the causal clock {}x as often — the clock \
         is being recomputed per operation again: {short:?} then {long:?}",
        LONG / SHORT,
        long.causal_contexts_built as f64 / short.causal_contexts_built as f64
    );
}

/// Runs one batch of `operations` mark operations over a document big enough
/// that copying it is not free, and answers what the merge did to the whole
/// document while it ran.
fn whole_document_work_for_a_batch_of(operations: usize) -> opendoc_merge::instrument::MergeCounts {
    let mut app = OpenDocApp::new_empty_document();
    app.dispatch_command("create_document", json!({ "title": "Counted" }))
        .expect("create");
    for index in 0..operations {
        app.dispatch_command("add_paragraph", json!({ "text": format!("line {index}") }))
            .expect("paragraph");
    }
    let runs = text_run_ids(&app.document);
    assert!(
        runs.len() >= operations,
        "the fixture has a run per operation"
    );
    let batch = runs
        .into_iter()
        .take(operations)
        .map(|id| {
            (
                "",
                "add-mark",
                OperationKind::AddMark {
                    text_id: id,
                    mark: Mark {
                        kind: MarkKind::Bold,
                        value: None,
                        expand: MarkExpand::None,
                    },
                },
            )
        })
        .collect::<Vec<_>>();

    // Everything the fixture did is off the books; only the gesture counts.
    let _ = opendoc_merge::instrument::take_merge_counts();
    app.apply_batch(batch).expect("the batch lands");
    opendoc_merge::instrument::take_merge_counts()
}
