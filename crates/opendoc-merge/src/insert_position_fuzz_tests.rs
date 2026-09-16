//! Property and fuzz tests for [`InsertPosition`] on the structural inserts.
//!
//! `InsertBlock`, `InsertInline`, `MoveInlineToBlock` and `InsertTableCell`
//! used to anchor with `after: Option<StableId>`, where `None` meant *append*
//! and nothing could say "before the first sibling". They now carry an
//! [`InsertPosition`], which is what makes undoing the deletion of a first
//! block, inline or cell expressible (ADR 0017).
//!
//! Plain convergence is not enough to guard that. Two replicas can agree
//! perfectly on the *wrong* answer — an implementation that quietly treated
//! `First` as `Last` would converge on every seed. So each fuzz below pairs
//! convergence with an **oracle**: a property of the merged document that can
//! be computed without running the merge, and that a mis-placed insert breaks.
//!
//! The oracles, and why they hold under any merge order:
//!
//! * a sibling inserted at `First` precedes every surviving **base** sibling —
//!   `First` lands at index 0 whenever it applies, and nothing that applies
//!   later can move a base sibling in front of it;
//! * a sibling inserted at `Last` follows every surviving base sibling — `Last`
//!   lands past the end, and no later operation moves a base sibling past it;
//! * a sibling inserted at `After(anchor)`, for a base anchor that survives,
//!   follows that anchor — the insert lands at the anchor's index plus one, and
//!   later inserts can only push it further away.
//!
//! Base siblings specifically: an anchor that is itself an inserted sibling may
//! be ordered *after* the insert that names it, in which case the anchored
//! insert legitimately degrades to an append and the relation is not defined.

use std::collections::BTreeMap;

use opendoc_core::{
    Block, BlockKind, BlockProperties, CellSpan, Document, Inline, InsertPosition, StableId,
    TableCell, TableRow,
};

use crate::causal::{ActorId, CausalContext, OperationId, VectorClock};
use crate::inline_ops::inline_id;
use crate::merge::merge_operations;
use crate::operation::{Operation, OperationKind};

const FUZZ_SEEDS: u64 = 2_000;
const BASE_BLOCKS: usize = 4;
const BASE_INLINES: usize = 3;
const BASE_CELLS: usize = 3;

/// Reproducible PRNG; a failure replays from the seed the panic prints.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(6364136223846793005).wrapping_add(1))
    }

    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 11
    }

    fn below(&mut self, bound: usize) -> usize {
        if bound == 0 {
            0
        } else {
            (self.next() % bound as u64) as usize
        }
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

/// One replica's causal state, so generated operations carry a consistent
/// context and some of them are genuinely concurrent.
struct Replica {
    actor: ActorId,
    next_seq: u64,
    lamport: u64,
    observed: VectorClock,
}

struct Generator {
    replicas: Vec<Replica>,
}

impl Generator {
    fn new(actors: usize) -> Self {
        Self {
            replicas: (0..actors)
                .map(|index| Replica {
                    actor: ActorId(format!("actor-{index}")),
                    next_seq: 1,
                    lamport: 0,
                    observed: VectorClock::new(),
                })
                .collect(),
        }
    }

    /// Mint an id and context for one operation, sometimes syncing the author
    /// with another replica first so the set mixes causal and concurrent work.
    fn mint(&mut self, rng: &mut Rng) -> (OperationId, CausalContext) {
        let actors = self.replicas.len();
        let author = rng.below(actors);
        if rng.below(3) == 0 {
            let source = rng.below(actors);
            if source != author {
                let (clock, lamport) = (
                    self.replicas[source].observed.clone(),
                    self.replicas[source].lamport,
                );
                self.replicas[author].observed.join(&clock);
                self.replicas[author].lamport = self.replicas[author].lamport.max(lamport);
            }
        }
        let replica = &mut self.replicas[author];
        let id = OperationId {
            actor: replica.actor.clone(),
            seq: replica.next_seq,
        };
        replica.next_seq += 1;
        replica.lamport += 1;
        let context = CausalContext {
            lamport: replica.lamport,
            observed: replica.observed.clone(),
        };
        replica.observed.observe(&id);
        (id, context)
    }
}

fn shuffle_into_streams(rng: &mut Rng, operations: &[Operation]) -> Vec<Vec<Operation>> {
    let stream_count = 1 + rng.below(4);
    let mut streams: Vec<Vec<Operation>> = vec![Vec::new(); stream_count];
    let mut shuffled = operations.to_vec();
    for index in (1..shuffled.len()).rev() {
        shuffled.swap(index, rng.below(index + 1));
    }
    for operation in shuffled {
        streams[rng.below(stream_count)].push(operation);
    }
    streams
}

fn id(prefix: &str, index: usize) -> StableId {
    StableId::parse(format!("{prefix}-{index:04}")).unwrap()
}

fn text(id: StableId, body: &str) -> Inline {
    Inline::Text {
        id,
        text: body.to_string(),
        marks: Vec::new(),
    }
}

/// `BASE_BLOCKS` paragraphs of `BASE_INLINES` runs each, all with stable ids.
fn base_document() -> Document {
    let mut document = Document::new("Doc");
    for block in 0..BASE_BLOCKS {
        document.blocks.push(Block {
            id: id("block", block),
            kind: BlockKind::Paragraph,
            properties: BlockProperties::default(),
            content: (0..BASE_INLINES)
                .map(|inline| {
                    text(
                        id("run", block * 100 + inline),
                        &format!("b{block}i{inline} "),
                    )
                })
                .collect(),
        });
    }
    document.validate().expect("the fixture is valid");
    document
}

/// A random position among the four kinds, anchored on one of `siblings`.
fn position(rng: &mut Rng, siblings: &[StableId]) -> InsertPosition {
    match rng.below(4) {
        0 => InsertPosition::First,
        1 => InsertPosition::Before(rng.pick(siblings).clone()),
        2 => InsertPosition::After(rng.pick(siblings).clone()),
        _ => InsertPosition::Last,
    }
}

/// What an insert asked for, so the oracle can check where it ended up.
struct Expectation {
    inserted: StableId,
    position: InsertPosition,
}

fn block_index(document: &Document, target: &StableId) -> Option<usize> {
    document.blocks.iter().position(|block| &block.id == target)
}

fn inline_index(document: &Document, block: &StableId, target: &StableId) -> Option<usize> {
    document
        .blocks
        .iter()
        .find(|item| &item.id == block)?
        .content
        .iter()
        .position(|inline| inline_id(inline) == target)
}

/// Assert the four position oracles over one ordered sibling list, and
/// return how many expectations were actually *checked*.
///
/// `index_of` answers where a sibling is in the merged document, or `None`
/// when it is not there any more; `base` names the siblings that were present
/// before any operation applied.
///
/// The return value is not bookkeeping. An expectation whose insert is not in
/// the merged document is skipped, so a `merge_operations` that dropped every
/// operation and returned the base skipped *all* of them and this function
/// asserted nothing at all. The callers turn the count into a coverage floor.
/// PLAN88 §7.
#[must_use]
fn assert_positions(
    seed: u64,
    label: &str,
    expectations: &[Expectation],
    base: &[StableId],
    index_of: impl Fn(&StableId) -> Option<usize>,
) -> usize {
    let surviving_base: Vec<(StableId, usize)> = base
        .iter()
        .filter_map(|item| index_of(item).map(|index| (item.clone(), index)))
        .collect();
    let mut checked = 0usize;
    for expectation in expectations {
        let Some(index) = index_of(&expectation.inserted) else {
            // Deleted again by another operation in the same set; the
            // position it asked for is no longer observable.
            continue;
        };
        checked += 1;
        match &expectation.position {
            InsertPosition::First => {
                for (base_id, base_index) in &surviving_base {
                    assert!(
                        index < *base_index,
                        "seed {seed}: {label} {} was inserted First but sits at {index}, \
                         after base sibling {base_id} at {base_index}",
                        expectation.inserted
                    );
                }
            }
            InsertPosition::Last => {
                for (base_id, base_index) in &surviving_base {
                    assert!(
                        index > *base_index,
                        "seed {seed}: {label} {} was inserted Last but sits at {index}, \
                         before base sibling {base_id} at {base_index}",
                        expectation.inserted
                    );
                }
            }
            InsertPosition::Before(anchor) => {
                if !base.contains(anchor) {
                    continue;
                }
                if let Some(anchor_index) = index_of(anchor) {
                    assert!(
                        index < anchor_index,
                        "seed {seed}: {label} {} was inserted Before base sibling {anchor} \
                         at {anchor_index} but sits at {index}",
                        expectation.inserted
                    );
                }
            }
            InsertPosition::After(anchor) => {
                if !base.contains(anchor) {
                    continue;
                }
                if let Some(anchor_index) = index_of(anchor) {
                    assert!(
                        index > anchor_index,
                        "seed {seed}: {label} {} was inserted After base sibling {anchor} \
                         at {anchor_index} but sits at {index}",
                        expectation.inserted
                    );
                }
            }
        }
    }
    checked
}

/// Blocks and inlines: convergence across permutations, plus the three
/// position oracles at both levels.
#[test]
fn randomised_block_and_inline_inserts_converge_and_land_where_they_asked() {
    let base = base_document();
    let base_block_ids: Vec<StableId> = base.blocks.iter().map(|block| block.id.clone()).collect();
    let base_inline_ids: BTreeMap<StableId, Vec<StableId>> = base
        .blocks
        .iter()
        .map(|block| {
            (
                block.id.clone(),
                block
                    .content
                    .iter()
                    .map(|inline| inline_id(inline).clone())
                    .collect(),
            )
        })
        .collect();

    let mut first_inserts = 0usize;
    let mut checked_positions = 0usize;
    for seed in 0..FUZZ_SEEDS {
        let mut rng = Rng::new(seed);
        let mut generator = Generator::new(3);
        let mut operations = Vec::new();
        let mut block_expectations = Vec::new();
        let mut inline_expectations: BTreeMap<StableId, Vec<Expectation>> = BTreeMap::new();
        // Live sibling lists, so an anchor can name something an earlier
        // operation of the same set introduced.
        let mut block_ids = base_block_ids.clone();
        let mut inline_ids = base_inline_ids.clone();

        let steps = 4 + rng.below(8);
        for step in 0..steps {
            let (operation_id, context) = generator.mint(&mut rng);
            let kind = match rng.below(5) {
                // Insert a block.
                0 | 1 => {
                    let new_block = id("added-block", seed as usize * 100 + step);
                    let new_run = id("added-block-run", seed as usize * 100 + step);
                    let position = position(&mut rng, &block_ids);
                    if position == InsertPosition::First {
                        first_inserts += 1;
                    }
                    block_expectations.push(Expectation {
                        inserted: new_block.clone(),
                        position: position.clone(),
                    });
                    block_ids.push(new_block.clone());
                    inline_ids.insert(new_block.clone(), vec![new_run.clone()]);
                    OperationKind::InsertBlock {
                        position,
                        block: Block {
                            id: new_block,
                            kind: BlockKind::Paragraph,
                            properties: BlockProperties::default(),
                            content: vec![text(new_run, "added ")],
                        },
                    }
                }
                // Insert an inline into an existing block.
                2 | 3 => {
                    let block = rng.pick(&block_ids).clone();
                    let siblings = inline_ids.get(&block).cloned().unwrap_or_default();
                    if siblings.is_empty() {
                        continue;
                    }
                    let new_inline = id("added-run", seed as usize * 100 + step);
                    let position = position(&mut rng, &siblings);
                    if position == InsertPosition::First {
                        first_inserts += 1;
                    }
                    inline_expectations
                        .entry(block.clone())
                        .or_default()
                        .push(Expectation {
                            inserted: new_inline.clone(),
                            position: position.clone(),
                        });
                    inline_ids
                        .entry(block.clone())
                        .or_default()
                        .push(new_inline.clone());
                    OperationKind::InsertInline {
                        block_id: block,
                        position,
                        inline: text(new_inline, "in "),
                    }
                }
                // Delete something, so the set is not inserts alone and an
                // anchor can genuinely go missing.
                _ => {
                    if rng.below(2) == 0 && block_ids.len() > 1 {
                        OperationKind::DeleteBlock {
                            block_id: rng.pick(&block_ids).clone(),
                        }
                    } else {
                        let block = rng.pick(&block_ids).clone();
                        let siblings = inline_ids.get(&block).cloned().unwrap_or_default();
                        if siblings.len() < 2 {
                            continue;
                        }
                        OperationKind::DeleteInline {
                            inline_id: rng.pick(&siblings).clone(),
                        }
                    }
                }
            };
            operations.push(Operation::in_context(operation_id, kind, context));
        }

        // Convergence: two independent shufflings of the same set must
        // produce the same document, byte for byte.
        let first = merge_operations(&base, &shuffle_into_streams(&mut rng, &operations))
            .unwrap_or_else(|err| panic!("seed {seed}: {err}"))
            .document;
        let second = merge_operations(&base, &shuffle_into_streams(&mut rng, &operations))
            .unwrap_or_else(|err| panic!("seed {seed}: {err}"))
            .document;
        assert_eq!(first, second, "seed {seed}: permutations diverged");
        first
            .validate()
            .unwrap_or_else(|err| panic!("seed {seed}: merged document is invalid: {err}"));

        checked_positions += assert_positions(
            seed,
            "block",
            &block_expectations,
            &base_block_ids,
            |target| block_index(&first, target),
        );
        for (block, expectations) in &inline_expectations {
            let base_siblings = base_inline_ids.get(block).cloned().unwrap_or_default();
            checked_positions +=
                assert_positions(seed, "inline", expectations, &base_siblings, |target| {
                    inline_index(&first, block, target)
                });
        }
    }

    // The suite is worthless if it never exercised the position the whole
    // change exists for.
    assert!(
        first_inserts > FUZZ_SEEDS as usize,
        "only {first_inserts} inserts across {FUZZ_SEEDS} seeds asked for First"
    );
    // …and the position oracle only says anything about inserts that are
    // actually in the merged document, so a merge that inserted nothing would
    // have skipped every one of them.
    assert!(
        checked_positions > FUZZ_SEEDS as usize * 4,
        "only {checked_positions} inserted siblings were found and checked across \
         {FUZZ_SEEDS} seeds"
    );
}

/// `MoveInlineToBlock` carries a position too. A move is a delete and an
/// insert at once, so the "base sibling" oracle above is not defined for the
/// blocks it touches — what is asserted here is convergence, that the moved
/// inline is in the block it was sent to, and that a move to `First` puts it
/// at the front of that block.
#[test]
fn randomised_inline_moves_converge_and_a_move_to_first_lands_at_the_front() {
    let base = base_document();
    let base_block_ids: Vec<StableId> = base.blocks.iter().map(|block| block.id.clone()).collect();
    let base_inline_ids: Vec<StableId> = base
        .blocks
        .iter()
        .flat_map(|block| block.content.iter().map(|inline| inline_id(inline).clone()))
        .collect();

    let mut checked_first_moves = 0usize;
    for seed in 0..FUZZ_SEEDS {
        let mut rng = Rng::new(seed ^ 0x5151_5151);
        let mut generator = Generator::new(3);
        let mut operations = Vec::new();
        // The last move of each inline is the one that decides where it ends
        // up, so the oracle is keyed by inline.
        let mut last_move: BTreeMap<StableId, (StableId, InsertPosition)> = BTreeMap::new();

        // Some seeds deliberately generate exactly one move: that is the case
        // where where the inline ends up is decided by one position rather
        // than by the merge order between several, and so the only case the
        // positional oracle below is defined for.
        let steps = 1 + rng.below(5);
        for _ in 0..steps {
            let (operation_id, context) = generator.mint(&mut rng);
            let moved = rng.pick(&base_inline_ids).clone();
            let target = rng.pick(&base_block_ids).clone();
            let siblings: Vec<StableId> = base
                .blocks
                .iter()
                .find(|block| block.id == target)
                .map(|block| {
                    block
                        .content
                        .iter()
                        .map(|inline| inline_id(inline).clone())
                        .collect()
                })
                .unwrap_or_default();
            let position = position(&mut rng, &siblings);
            last_move.insert(moved.clone(), (target.clone(), position.clone()));
            operations.push(Operation::in_context(
                operation_id,
                OperationKind::MoveInlineToBlock {
                    inline_id: moved,
                    target_block_id: target,
                    position,
                },
                context,
            ));
        }

        let first = merge_operations(&base, &shuffle_into_streams(&mut rng, &operations))
            .unwrap_or_else(|err| panic!("seed {seed}: {err}"))
            .document;
        let second = merge_operations(&base, &shuffle_into_streams(&mut rng, &operations))
            .unwrap_or_else(|err| panic!("seed {seed}: {err}"))
            .document;
        assert_eq!(first, second, "seed {seed}: permutations diverged");
        first
            .validate()
            .unwrap_or_else(|err| panic!("seed {seed}: merged document is invalid: {err}"));

        // Nothing is lost or duplicated: every base inline is somewhere, once.
        for inline in &base_inline_ids {
            let occurrences: usize = first
                .blocks
                .iter()
                .map(|block| {
                    block
                        .content
                        .iter()
                        .filter(|item| inline_id(item) == inline)
                        .count()
                })
                .sum();
            assert_eq!(
                occurrences, 1,
                "seed {seed}: inline {inline} appears {occurrences} times after moves"
            );
        }

        // Only a single move is unambiguous about where it ends up: with two
        // moves of the same inline the merge order decides which wins, which
        // is a convergence question rather than a positional one.
        if last_move.len() == 1 && operations.len() == 1 {
            let (inline, (target, position)) = last_move.into_iter().next().unwrap();
            let index = inline_index(&first, &target, &inline)
                .unwrap_or_else(|| panic!("seed {seed}: the moved inline left its target block"));
            if position == InsertPosition::First {
                assert_eq!(
                    index, 0,
                    "seed {seed}: a move to First did not land at the front of {target}"
                );
                checked_first_moves += 1;
            }
        }
    }

    assert!(
        checked_first_moves > 0,
        "no seed produced a single move to First"
    );
}

/// Table cells: the same three oracles over one row's cells.
///
/// `InsertTableCell` inserts a *column* and seeds one row's cell in it, so the
/// position it carries is a position in the grid's column order.
#[test]
fn randomised_table_cell_inserts_converge_and_land_where_they_asked() {
    let base = base_table_document();
    let table_block_id = id("table-block", 0);
    let row_id = id("table-row", 0);
    let base_cell_ids: Vec<StableId> = (0..BASE_CELLS).map(|index| id("cell", index)).collect();

    let mut first_inserts = 0usize;
    let mut checked_positions = 0usize;
    for seed in 0..FUZZ_SEEDS {
        let mut rng = Rng::new(seed ^ 0x2727_2727);
        let mut generator = Generator::new(3);
        let mut operations = Vec::new();
        let mut expectations = Vec::new();
        let mut cell_ids = base_cell_ids.clone();

        let steps = 2 + rng.below(5);
        for step in 0..steps {
            let (operation_id, context) = generator.mint(&mut rng);
            let new_cell = id("added-cell", seed as usize * 100 + step);
            let position = position(&mut rng, &cell_ids);
            if position == InsertPosition::First {
                first_inserts += 1;
            }
            expectations.push(Expectation {
                inserted: new_cell.clone(),
                position: position.clone(),
            });
            cell_ids.push(new_cell.clone());
            operations.push(Operation::in_context(
                operation_id,
                OperationKind::InsertTableCell {
                    table_block_id: table_block_id.clone(),
                    row_id: row_id.clone(),
                    position,
                    cell: cell(&new_cell, "added"),
                },
                context,
            ));
        }

        let first = merge_operations(&base, &shuffle_into_streams(&mut rng, &operations))
            .unwrap_or_else(|err| panic!("seed {seed}: {err}"))
            .document;
        let second = merge_operations(&base, &shuffle_into_streams(&mut rng, &operations))
            .unwrap_or_else(|err| panic!("seed {seed}: {err}"))
            .document;
        assert_eq!(first, second, "seed {seed}: permutations diverged");
        first
            .validate()
            .unwrap_or_else(|err| panic!("seed {seed}: merged document is invalid: {err}"));

        checked_positions +=
            assert_positions(seed, "cell", &expectations, &base_cell_ids, |target| {
                row_cell_index(&first, &table_block_id, &row_id, target)
            });
    }

    assert!(
        first_inserts > FUZZ_SEEDS as usize / 2,
        "only {first_inserts} inserts across {FUZZ_SEEDS} seeds asked for First"
    );
    assert!(
        checked_positions > FUZZ_SEEDS as usize,
        "only {checked_positions} inserted cells were found and checked across {FUZZ_SEEDS} seeds"
    );
}

fn cell(id_: &StableId, body: &str) -> TableCell {
    TableCell {
        id: id_.clone(),
        span: CellSpan::SINGLE,
        properties: Default::default(),
        blocks: vec![Block::paragraph(body)],
    }
}

/// One table block holding one row of `BASE_CELLS` cells.
fn base_table_document() -> Document {
    let mut document = Document::new("Doc");
    document.blocks.push(Block {
        id: id("table-block", 0),
        kind: BlockKind::table(vec![TableRow {
            id: id("table-row", 0),
            height: None,
            header: false,
            cells: (0..BASE_CELLS)
                .map(|index| cell(&id("cell", index), &format!("c{index}")))
                .collect(),
        }]),
        properties: BlockProperties::default(),
        content: Vec::new(),
    });
    document.validate().expect("the fixture is valid");
    document
}

fn row_cell_index(
    document: &Document,
    table_block_id: &StableId,
    row_id: &StableId,
    target: &StableId,
) -> Option<usize> {
    let block = document
        .blocks
        .iter()
        .find(|block| &block.id == table_block_id)?;
    let BlockKind::Table { rows, .. } = &block.kind else {
        return None;
    };
    rows.iter()
        .find(|row| &row.id == row_id)?
        .cells
        .iter()
        .position(|cell| &cell.id == target)
}
