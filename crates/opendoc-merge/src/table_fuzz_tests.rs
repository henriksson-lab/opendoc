//! Randomised table structure tests, with an oracle.
//!
//! The gap that let ADR 0013's cell-binding bug ship is that no randomised
//! generator in this crate ever emitted a table row or column operation: the
//! convergence fuzz in `causal_convergence_tests` generates character and
//! property operations only. Convergence alone would not have caught it
//! either — every replica agreed on the same *wrong* table, which is the
//! ADR 0007 failure mode one level down — so the generator here is paired
//! with an oracle that is computed without the merge:
//!
//! * a row is live if it was inserted (or in the base) and not deleted;
//! * a column is live under the same rule;
//! * the content at (row, column) is whatever the row-insert operation bound
//!   to that column, the base text when both are from the base, and empty for
//!   a position no operation ever wrote.
//!
//! That map is a function of the operation *set*, not of any order, which is
//! exactly the property a merge has to have. Asserting it catches a merge
//! that converges on a table where a cell has silently moved into another
//! column — which is what an index-addressed cell did.

use crate::causal::{ActorId, CausalContext, OperationId, VectorClock};
use crate::merge::merge_operations;
use crate::operation::{Operation, OperationKind};
use crate::test_support::{grid_document, table_of};
use opendoc_core::{
    Block, BlockKind, BlockProperties, Inline, InsertPosition, StableId, TableCell, TableColumn,
    TableRow,
};
use std::collections::{BTreeMap, BTreeSet};

const SEEDS: u64 = 2_000;
const BASE_ROWS: usize = 3;
const BASE_COLUMNS: usize = 3;
const STEPS: usize = 12;
const ACTORS: usize = 3;
const PERMUTATIONS: usize = 8;

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
}

fn id(label: impl AsRef<str>) -> StableId {
    StableId::parse(label.as_ref()).unwrap()
}

/// The visible text of a cell holding one paragraph, which is every cell this
/// generator makes.
fn cell_text(cell: &TableCell) -> String {
    let mut out = String::new();
    for block in &cell.blocks {
        for inline in &block.content {
            if let Inline::Text { text, .. } = inline {
                out.push_str(text);
            }
        }
    }
    out
}

fn text_cell(cell_id: &StableId, text: &str) -> TableCell {
    TableCell {
        id: cell_id.clone(),
        span: Default::default(),
        properties: Default::default(),
        blocks: vec![Block {
            id: id(format!("block-{cell_id}")),
            kind: BlockKind::Paragraph,
            content: vec![Inline::Text {
                id: id(format!("text-{cell_id}")),
                text: text.to_string(),
                marks: Vec::new(),
            }],
            properties: BlockProperties::default(),
        }],
    }
}

/// One actor's replica: its causal state *and* the grid it can see, because
/// an anchor an actor names has to be one it has actually observed.
struct Replica {
    actor: ActorId,
    next_seq: u64,
    lamport: u64,
    observed: VectorClock,
    rows: Vec<StableId>,
    columns: Vec<StableId>,
}

impl Replica {
    fn new(index: usize) -> Self {
        Self {
            actor: ActorId(format!("actor-{index}")),
            next_seq: 1,
            lamport: 0,
            observed: VectorClock::new(),
            rows: (0..BASE_ROWS).map(|row| id(format!("row-{row}"))).collect(),
            columns: (0..BASE_COLUMNS)
                .map(|column| id(format!("column-{column}")))
                .collect(),
        }
    }
}

/// What the operation set says the final grid holds, computed without the
/// merge. Rows and columns are sets because insert/delete by identity is
/// order-independent; content is keyed on the pair, for the same reason.
#[derive(Default)]
struct Oracle {
    inserted_rows: BTreeSet<StableId>,
    deleted_rows: BTreeSet<StableId>,
    inserted_columns: BTreeSet<StableId>,
    deleted_columns: BTreeSet<StableId>,
    content: BTreeMap<(StableId, StableId), String>,
}

impl Oracle {
    fn new() -> Self {
        let mut oracle = Oracle::default();
        for row in 0..BASE_ROWS {
            oracle.inserted_rows.insert(id(format!("row-{row}")));
            for column in 0..BASE_COLUMNS {
                oracle.content.insert(
                    (id(format!("row-{row}")), id(format!("column-{column}"))),
                    format!("r{row}c{column}"),
                );
            }
        }
        for column in 0..BASE_COLUMNS {
            oracle
                .inserted_columns
                .insert(id(format!("column-{column}")));
        }
        oracle
    }

    fn live_rows(&self) -> BTreeSet<StableId> {
        self.inserted_rows
            .difference(&self.deleted_rows)
            .cloned()
            .collect()
    }

    fn live_columns(&self) -> BTreeSet<StableId> {
        self.inserted_columns
            .difference(&self.deleted_columns)
            .cloned()
            .collect()
    }

    fn expected_text(&self, row: &StableId, column: &StableId) -> &str {
        self.content
            .get(&(row.clone(), column.clone()))
            .map(String::as_str)
            .unwrap_or("")
    }
}

/// Generates a realistic table script: row and column inserts anchored
/// `First`, `Last` or `After` an anchor the author has observed, plus row and
/// column deletes, across `ACTORS` replicas that sync with each other at
/// random so some operations are causally ordered and some are concurrent.
fn generate(rng: &mut Rng) -> (Vec<Operation>, Oracle) {
    let table_block_id = id("table-block");
    let mut replicas: Vec<Replica> = (0..ACTORS).map(Replica::new).collect();
    let mut oracle = Oracle::new();
    let mut generated = Vec::new();

    for step in 0..STEPS {
        let author = rng.below(ACTORS);
        if rng.below(3) == 0 {
            let source = rng.below(ACTORS);
            if source != author {
                let (clock, lamport, rows, columns) = (
                    replicas[source].observed.clone(),
                    replicas[source].lamport,
                    replicas[source].rows.clone(),
                    replicas[source].columns.clone(),
                );
                replicas[author].observed.join(&clock);
                replicas[author].lamport = replicas[author].lamport.max(lamport);
                for row in rows {
                    if !replicas[author].rows.contains(&row) {
                        replicas[author].rows.push(row);
                    }
                }
                for column in columns {
                    if !replicas[author].columns.contains(&column) {
                        replicas[author].columns.push(column);
                    }
                }
            }
        }

        // Weighted so the grid grows on balance: a script that empties the
        // table tells us nothing about where its content went.
        let choice = rng.below(10);
        let replica = &mut replicas[author];
        let kind = match choice {
            0..=3 => {
                // Insert a row, one cell per column *this replica* can see,
                // each cell naming the column it was written into.
                let row_id = id(format!("row-s{step}"));
                let mut cells = Vec::new();
                let mut cell_columns = BTreeMap::new();
                for (index, column_id) in replica.columns.iter().enumerate() {
                    let cell_id = id(format!("cell-s{step}-{index}"));
                    let text = format!("s{step}@{column_id}");
                    cells.push(text_cell(&cell_id, &text));
                    cell_columns.insert(cell_id, column_id.clone());
                    oracle
                        .content
                        .insert((row_id.clone(), column_id.clone()), text);
                }
                if cells.is_empty() {
                    continue;
                }
                let position = position_in(rng, &replica.rows);
                replica.rows.push(row_id.clone());
                oracle.inserted_rows.insert(row_id.clone());
                OperationKind::InsertTableRow {
                    table_block_id: table_block_id.clone(),
                    position,
                    row: TableRow {
                        id: row_id,
                        height: None,
                        header: false,
                        cells,
                    },
                    cell_columns,
                }
            }
            4..=6 => {
                let column_id = id(format!("column-s{step}"));
                let position = position_in(rng, &replica.columns);
                replica.columns.push(column_id.clone());
                oracle.inserted_columns.insert(column_id.clone());
                OperationKind::InsertTableColumn {
                    table_block_id: table_block_id.clone(),
                    position,
                    column: TableColumn {
                        id: column_id,
                        width: None,
                    },
                }
            }
            7..=8 => {
                if replica.rows.len() < 2 {
                    continue;
                }
                let index = rng.below(replica.rows.len());
                let row_id = replica.rows.remove(index);
                oracle.deleted_rows.insert(row_id.clone());
                OperationKind::DeleteTableRow {
                    table_block_id: table_block_id.clone(),
                    row_id,
                }
            }
            _ => {
                if replica.columns.len() < 2 {
                    continue;
                }
                let index = rng.below(replica.columns.len());
                let column_id = replica.columns.remove(index);
                oracle.deleted_columns.insert(column_id.clone());
                OperationKind::DeleteTableColumn {
                    table_block_id: table_block_id.clone(),
                    column_id,
                }
            }
        };

        let replica = &mut replicas[author];
        let operation_id = OperationId {
            actor: replica.actor.clone(),
            seq: replica.next_seq,
        };
        replica.next_seq += 1;
        replica.lamport += 1;
        let context = CausalContext {
            lamport: replica.lamport,
            observed: replica.observed.clone(),
        };
        replica.observed.observe(&operation_id);
        generated.push(Operation::in_context(operation_id, kind, context));
    }
    (generated, oracle)
}

/// `First`, `Last`, or `After` one of the siblings the author has observed —
/// the three positions the operation vocabulary has.
fn position_in(rng: &mut Rng, siblings: &[StableId]) -> InsertPosition {
    match rng.below(4) {
        0 => InsertPosition::First,
        1 => InsertPosition::Last,
        _ if siblings.is_empty() => InsertPosition::Last,
        _ => InsertPosition::After(siblings[rng.below(siblings.len())].clone()),
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

#[test]
fn randomised_table_structure_scripts_converge_byte_identically() {
    let (base, _) = grid_document(BASE_ROWS, BASE_COLUMNS);
    let mut changed = 0usize;
    for seed in 0..SEEDS {
        let mut rng = Rng::new(seed);
        let (operations, _) = generate(&mut rng);
        let mut expected: Option<Vec<u8>> = None;
        for permutation in 0..PERMUTATIONS {
            let streams = shuffle_into_streams(&mut rng, &operations);
            let result = merge_operations(&base, &streams)
                .unwrap_or_else(|err| panic!("seed {seed} permutation {permutation}: {err:?}"));
            result
                .document
                .validate()
                .unwrap_or_else(|err| panic!("seed {seed} permutation {permutation}: {err:?}"));
            let encoded = opendoc_format::encode_canonical_cbor(&result.document).unwrap();
            match &expected {
                None => {
                    if result.document != base {
                        changed += 1;
                    }
                    expected = Some(encoded);
                }
                Some(first) => assert_eq!(
                    *first, encoded,
                    "seed {seed} permutation {permutation} diverged"
                ),
            }
        }
    }
    assert!(
        changed > SEEDS as usize * 9 / 10,
        "only {changed} of {SEEDS} seeds changed the table"
    );
}

#[test]
fn randomised_table_structure_scripts_keep_every_cell_in_its_own_column() {
    // The oracle. Convergence is not enough here and never was: the
    // index-addressed cell converged on the same wrong table everywhere.
    let (base, _) = grid_document(BASE_ROWS, BASE_COLUMNS);
    let mut checked = 0usize;
    let mut surviving_content = 0usize;
    for seed in 0..SEEDS {
        let mut rng = Rng::new(seed ^ 0x5bf0_3635);
        let (operations, oracle) = generate(&mut rng);
        let streams = shuffle_into_streams(&mut rng, &operations);
        let result =
            merge_operations(&base, &streams).unwrap_or_else(|err| panic!("seed {seed}: {err:?}"));
        let (columns, rows) = table_of(&result.document);

        let live_rows = oracle.live_rows();
        let live_columns = oracle.live_columns();
        if live_rows.is_empty() || live_columns.is_empty() {
            // Every row or every column was deleted, so merge keeps a derived
            // placeholder and there is no content left to check. Rare by
            // construction; counted by `checked` below so it cannot become
            // the whole run.
            continue;
        }
        checked += 1;

        let merged_rows: BTreeSet<StableId> = rows.iter().map(|row| row.id.clone()).collect();
        let merged_columns: BTreeSet<StableId> =
            columns.iter().map(|column| column.id.clone()).collect();
        // Everything the operations left must be there. Merge is allowed to
        // keep a *derived* placeholder beyond that — deleting the last column
        // of a table leaves one, and a later insert then sits beside it — but
        // a placeholder is empty, which the content check below enforces.
        assert!(
            live_rows.is_subset(&merged_rows),
            "seed {seed}: rows the operations left are missing: {:?}",
            live_rows.difference(&merged_rows).collect::<Vec<_>>()
        );
        assert!(
            live_columns.is_subset(&merged_columns),
            "seed {seed}: columns the operations left are missing: {:?}",
            live_columns.difference(&merged_columns).collect::<Vec<_>>()
        );

        for row in rows {
            assert_eq!(
                row.cells.len(),
                columns.len(),
                "seed {seed}: row {} is ragged",
                row.id
            );
            let row_is_live = live_rows.contains(&row.id);
            for (index, cell) in row.cells.iter().enumerate() {
                let column_id = &columns[index].id;
                // A position merge invented — a placeholder row or column —
                // holds nothing. A position the operations describe holds
                // exactly what they wrote there.
                let expected = if row_is_live && live_columns.contains(column_id) {
                    oracle.expected_text(&row.id, column_id)
                } else {
                    ""
                };
                assert_eq!(
                    cell_text(cell),
                    expected,
                    "seed {seed}: cell at ({}, {column_id}) holds another column's content",
                    row.id
                );
                if !expected.is_empty() {
                    surviving_content += 1;
                }
            }
        }
    }
    assert!(
        checked > SEEDS as usize * 9 / 10,
        "only {checked} of {SEEDS} seeds had a grid left to check"
    );
    assert!(
        surviving_content > SEEDS as usize,
        "the oracle checked almost no written content ({surviving_content} cells)"
    );
}

#[test]
fn a_cells_content_survives_a_concurrent_delete_of_another_column() {
    // Consequence A, stated exactly. Alice appends two rows; Bob deletes two
    // columns. Whatever order the merge puts them in, every cell Alice typed
    // into the column that survived must still be in it.
    //
    // With cells addressed by index this produced a table on which every
    // replica agreed and in which Alice's rows had each lost a *different*
    // cell, reported only as `table-geometry-repaired`.
    let (base, table_block_id) = grid_document(3, 3);
    let columns: Vec<StableId> = (0..3).map(|index| id(format!("column-{index}"))).collect();

    let alice_row = |seq: u64| {
        let row_id = id(format!("row-alice-{seq}"));
        let mut cells = Vec::new();
        let mut cell_columns = BTreeMap::new();
        for (index, column_id) in columns.iter().enumerate() {
            let cell_id = id(format!("cell-alice-{seq}-{index}"));
            cells.push(text_cell(&cell_id, &format!("alice{seq}-in-{column_id}")));
            cell_columns.insert(cell_id, column_id.clone());
        }
        Operation::in_context(
            OperationId {
                actor: ActorId("alice".to_string()),
                seq,
            },
            OperationKind::InsertTableRow {
                table_block_id: table_block_id.clone(),
                position: InsertPosition::Last,
                row: TableRow {
                    id: row_id,
                    height: None,
                    header: false,
                    cells,
                },
                cell_columns,
            },
            CausalContext {
                lamport: seq,
                observed: VectorClock::new(),
            },
        )
    };
    let bob_delete = |seq: u64, column: usize| {
        Operation::in_context(
            OperationId {
                actor: ActorId("bob".to_string()),
                seq,
            },
            OperationKind::DeleteTableColumn {
                table_block_id: table_block_id.clone(),
                column_id: columns[column].clone(),
            },
            CausalContext {
                lamport: seq,
                observed: VectorClock::new(),
            },
        )
    };

    let alice = vec![alice_row(1), alice_row(2)];
    let bob = vec![bob_delete(1, 0), bob_delete(2, 1)];

    let mut encodings = BTreeSet::new();
    for streams in [
        vec![alice.clone(), bob.clone()],
        vec![bob.clone(), alice.clone()],
        vec![alice.iter().chain(bob.iter()).cloned().collect::<Vec<_>>()],
    ] {
        let result = merge_operations(&base, &streams).expect("the merge must not fail");
        result.document.validate().expect("valid grid");
        encodings.insert(opendoc_format::encode_canonical_cbor(&result.document).unwrap());
        let (merged_columns, rows) = table_of(&result.document);
        assert_eq!(
            merged_columns
                .iter()
                .map(|column| column.id.to_string())
                .collect::<Vec<_>>(),
            vec!["column-2"],
            "only the column nobody deleted is left"
        );
        let alice_rows: Vec<&TableRow> = rows
            .iter()
            .filter(|row| row.id.as_str().starts_with("row-alice-"))
            .collect();
        assert_eq!(alice_rows.len(), 2, "both of Alice's rows are there");
        for (index, row) in alice_rows.iter().enumerate() {
            assert_eq!(row.cells.len(), 1);
            assert_eq!(
                cell_text(&row.cells[0]),
                format!("alice{}-in-column-2", index + 1),
                "row {} kept the cell it typed into column-2",
                row.id
            );
        }
        // The base rows keep their own column-2 content too.
        for base_row in 0..3 {
            let row = rows
                .iter()
                .find(|row| row.id.as_str() == format!("row-{base_row}"))
                .expect("base row");
            assert_eq!(cell_text(&row.cells[0]), format!("r{base_row}c2"));
        }
    }
    assert_eq!(
        encodings.len(),
        1,
        "the three groupings must agree byte for byte"
    );
}

#[test]
fn a_synthesised_column_does_not_collide_with_a_later_one() {
    // Consequence B. One actor, three operations, no concurrency at all: a
    // row wider than the grid synthesises a column, a delete shifts the grid,
    // and a second wide row synthesises again. With the synthesised column's
    // identity derived from its *index*, the second synthesis minted the id
    // the first one already held and the whole merge failed with
    // `duplicate table cell id` — a document-losing error, deterministic,
    // on a table nobody was collaborating on.
    let (base, table_block_id) = grid_document(3, 3);
    let wide_row = |seq: u64| {
        let row_id = id(format!("row-wide-{seq}"));
        let cells: Vec<TableCell> = (0..4)
            .map(|index| {
                text_cell(
                    &id(format!("cell-wide-{seq}-{index}")),
                    &format!("wide{seq}-{index}"),
                )
            })
            .collect();
        Operation::new(
            OperationId {
                actor: ActorId("a".to_string()),
                seq,
            },
            OperationKind::InsertTableRow {
                table_block_id: table_block_id.clone(),
                position: InsertPosition::Last,
                row: TableRow {
                    id: row_id,
                    height: None,
                    header: false,
                    cells,
                },
                // Deliberately unbound: this is what a journal written before
                // the binding carries, and the shape the crash needs.
                cell_columns: BTreeMap::new(),
            },
        )
    };
    let delete_first = Operation::new(
        OperationId {
            actor: ActorId("a".to_string()),
            seq: 2,
        },
        OperationKind::DeleteTableColumn {
            table_block_id: table_block_id.clone(),
            column_id: id("column-0"),
        },
    );

    let result = merge_operations(&base, &[vec![wide_row(1), delete_first, wide_row(3)]])
        .expect("a one-actor table script must not fail the merge");
    result.document.validate().expect("valid grid");
    let (columns, _) = table_of(&result.document);
    let unique: BTreeSet<&StableId> = columns.iter().map(|column| &column.id).collect();
    assert_eq!(unique.len(), columns.len(), "column ids are unique");
    // And the operations that carried no binding said so, rather than being
    // read as though they had one.
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.code == "legacy-table-row-binding"),
        "{:?}",
        result.warnings
    );
}

#[test]
fn a_row_bound_to_a_deleted_column_loses_that_cell_and_says_so() {
    // The other half of the binding: a cell whose column is gone goes with
    // it, rather than resurrecting the column or sliding into its neighbour.
    let (base, table_block_id) = grid_document(2, 2);
    let cell_zero = id("cell-new-0");
    let cell_one = id("cell-new-1");
    // The delete sorts first (no causal context, so the order is
    // `(lamport, actor, seq)`), which is the case that has to drop the cell
    // rather than resurrect the column.
    let insert = Operation::new(
        OperationId {
            actor: ActorId("b".to_string()),
            seq: 1,
        },
        OperationKind::InsertTableRow {
            table_block_id: table_block_id.clone(),
            position: InsertPosition::Last,
            row: TableRow {
                id: id("row-new"),
                height: None,
                header: false,
                cells: vec![
                    text_cell(&cell_zero, "for column zero"),
                    text_cell(&cell_one, "for column one"),
                ],
            },
            cell_columns: BTreeMap::from([(cell_zero, id("column-0")), (cell_one, id("column-1"))]),
        },
    );
    let delete = Operation::new(
        OperationId {
            actor: ActorId("a".to_string()),
            seq: 1,
        },
        OperationKind::DeleteTableColumn {
            table_block_id,
            column_id: id("column-0"),
        },
    );

    let result = merge_operations(&base, &[vec![delete], vec![insert]]).expect("merge");
    let (columns, rows) = table_of(&result.document);
    assert_eq!(columns.len(), 1);
    let new_row = rows
        .iter()
        .find(|row| row.id.as_str() == "row-new")
        .unwrap();
    assert_eq!(new_row.cells.len(), 1);
    assert_eq!(
        cell_text(&new_row.cells[0]),
        "for column one",
        "the surviving column kept its own cell, not its neighbour's"
    );
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.code == "table-row-cell-column-deleted"),
        "the dropped cell is reported: {:?}",
        result.warnings
    );
}

/// The `InsertTableRow` payload exactly as journals written before the
/// binding carry it: a row, and nothing that names a column.
#[derive(serde::Serialize)]
struct LegacyRowInsert {
    table_block_id: StableId,
    position: InsertPosition,
    row: TableRow,
}

/// Serialized the same way `OperationKind::InsertTableRow` is — externally
/// tagged, so the bytes are the bytes an older repository holds.
#[derive(serde::Serialize)]
enum LegacyOperationKind {
    InsertTableRow(LegacyRowInsert),
}

#[test]
fn an_operation_written_before_the_binding_still_decodes_and_is_read_as_written() {
    // What an older repository does. `cell_columns` is `#[serde(default)]`, so
    // a journal written before the binding existed decodes rather than
    // failing — and decodes to an *empty* map, which is the one value that
    // means "this operation named no column". The merge then reads it
    // positionally, exactly as it was written, and says so, the way a
    // repository with a pre-split operation sequence reports
    // `legacy-operation-sequence-gap` rather than pretending it has none.
    let bytes = opendoc_format::encode_canonical_cbor(&LegacyOperationKind::InsertTableRow(
        LegacyRowInsert {
            table_block_id: id("table-block"),
            position: InsertPosition::Last,
            row: TableRow {
                id: id("row-legacy"),
                height: None,
                header: false,
                cells: vec![text_cell(&id("cell-legacy"), "old")],
            },
        },
    ))
    .expect("the old shape encodes");
    let kind: OperationKind =
        opendoc_format::decode_cbor(&bytes).expect("an operation from an older journal decodes");
    match &kind {
        OperationKind::InsertTableRow { cell_columns, .. } => assert!(
            cell_columns.is_empty(),
            "an absent binding is an empty one, never a guess"
        ),
        other => panic!("expected InsertTableRow, got {other:?}"),
    }

    let (base, _) = grid_document(1, 2);
    let result = merge_operations(
        &base,
        &[vec![Operation::new(
            OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind,
        )]],
    )
    .expect("merge");
    let (columns, rows) = table_of(&result.document);
    assert_eq!(columns.len(), 2);
    let legacy_row = rows
        .iter()
        .find(|row| row.id.as_str() == "row-legacy")
        .expect("the legacy row landed");
    assert_eq!(
        cell_text(&legacy_row.cells[0]),
        "old",
        "its one cell was read positionally, into the first column"
    );
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.code == "legacy-table-row-binding"),
        "and the weaker reading is reported: {:?}",
        result.warnings
    );

    // The reverse direction: a row insert that *does* carry a binding is not
    // mistaken for a legacy one, and an empty binding is kept off the wire by
    // `skip_serializing_if`, so the two shapes stay interchangeable.
    let bound = OperationKind::InsertTableRow {
        table_block_id: id("table-block"),
        position: InsertPosition::Last,
        row: TableRow {
            id: id("row-bound"),
            height: None,
            header: false,
            cells: vec![text_cell(&id("cell-bound"), "new")],
        },
        cell_columns: BTreeMap::from([(id("cell-bound"), id("column-1"))]),
    };
    let round_tripped: OperationKind =
        opendoc_format::decode_cbor(&opendoc_format::encode_canonical_cbor(&bound).unwrap())
            .expect("a bound operation round-trips");
    assert_eq!(round_tripped, bound);
}
