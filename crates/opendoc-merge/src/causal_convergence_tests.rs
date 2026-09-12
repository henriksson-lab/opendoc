//! Tests for causal ordering and character-level convergence.
//!
//! See `docs/adr/0007-causal-ordering-and-text-convergence.md`. Every test in
//! the "transformation cases" section below was run against the pre-ADR
//! behaviour (positional application, `(actor, seq)` ordering) and fails
//! there; the assertions name the exact text, not a `contains`, so that a
//! merge that corrupts concurrent edits cannot pass them.

use opendoc_core::{
    Alignment, Block, BlockKind, BlockProperties, BlockProperty, Document, Inline, Length, StableId,
};

use crate::causal::{ActorId, CausalContext, OperationId, VectorClock};
use crate::merge::merge_operations;
use crate::operation::{Operation, OperationKind};
use crate::{causal, text_sequence};

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn run_id(label: &str) -> StableId {
    StableId::parse(label).unwrap()
}

/// A document whose single paragraph holds one text run with `text`.
fn document_with_run(text: &str) -> (Document, StableId) {
    let mut base = Document::new("Doc");
    let id = run_id("text-1");
    base.blocks.push(Block {
        id: run_id("block-1"),
        kind: BlockKind::Paragraph,
        content: vec![Inline::Text {
            id: id.clone(),
            text: text.to_string(),
            marks: Vec::new(),
        }],
        properties: BlockProperties::default(),
    });
    (base, id)
}

fn op(actor: &str, seq: u64, kind: OperationKind) -> Operation {
    Operation::new(
        OperationId {
            actor: ActorId(actor.to_string()),
            seq,
        },
        kind,
    )
}

fn insert(inline_id: &StableId, offset: usize, text: &str) -> OperationKind {
    OperationKind::InsertText {
        inline_id: inline_id.clone(),
        offset,
        text: text.to_string(),
    }
}

fn delete(inline_id: &StableId, start: usize, end: usize) -> OperationKind {
    OperationKind::DeleteText {
        inline_id: inline_id.clone(),
        start,
        end,
    }
}

/// Merge `streams` and return the document's visible text.
///
/// `visible_text` terminates each block with a newline; the assertions below
/// are about the run contents, so the block terminator is trimmed off.
fn merged_text(base: &Document, streams: &[Vec<Operation>]) -> String {
    visible_run_text(&merge_operations(base, streams).unwrap().document)
}

fn visible_run_text(document: &Document) -> String {
    document.visible_text().trim_end_matches('\n').to_string()
}

/// Merge the same operation set under several stream groupings and assert all
/// of them produce byte-identical canonical CBOR, then return the text.
///
/// Convergence is the weaker half of the claim — the pre-ADR merge converged
/// too, it just converged on the wrong text — so callers also assert the exact
/// string.
fn converged_text(base: &Document, left: Vec<Operation>, right: Vec<Operation>) -> String {
    let both: Vec<Operation> = left.iter().chain(right.iter()).cloned().collect();
    let mut reversed = both.clone();
    reversed.reverse();
    let groupings: Vec<Vec<Vec<Operation>>> = vec![
        vec![left.clone(), right.clone()],
        vec![right, left],
        vec![both.clone()],
        vec![reversed],
        both.iter().map(|one| vec![one.clone()]).collect(),
    ];
    let mut bytes: Option<Vec<u8>> = None;
    let mut text = String::new();
    for grouping in groupings {
        let result = merge_operations(base, &grouping).unwrap();
        let encoded = opendoc_format::encode_canonical_cbor(&result.document).unwrap();
        match &bytes {
            None => {
                bytes = Some(encoded);
                text = visible_run_text(&result.document);
            }
            Some(expected) => assert_eq!(
                *expected, encoded,
                "stream grouping changed the merged document"
            ),
        }
    }
    text
}

// ---------------------------------------------------------------------------
// transformation cases
// ---------------------------------------------------------------------------

#[test]
fn concurrent_inserts_at_different_offsets_do_not_shift_each_other() {
    let (base, id) = document_with_run("Hello world");
    // alice prepends, bob appends. Positional application applied bob's
    // offset 11 to a document alice had already lengthened, so bob's text
    // landed one character early, inside "world".
    let text = converged_text(
        &base,
        vec![op("alice", 1, insert(&id, 0, "<"))],
        vec![op("bob", 1, insert(&id, 11, ">"))],
    );
    assert_eq!(text, "<Hello world>");
}

#[test]
fn a_concurrent_insert_is_never_split_by_another_insert() {
    let (base, id) = document_with_run("abcdefg");
    // bob's offset 5 means "after e" in the document bob saw. Applied
    // positionally after alice's three-character insert it lands *inside*
    // "AAA", tearing one actor's typing in half.
    let text = converged_text(
        &base,
        vec![op("alice", 1, insert(&id, 3, "AAA"))],
        vec![op("bob", 1, insert(&id, 5, "B"))],
    );
    assert_eq!(text, "abcAAAdeBfg");
}

#[test]
fn concurrent_inserts_at_one_anchor_both_survive_in_a_stable_order() {
    let (base, id) = document_with_run("abcdefg");
    let text = converged_text(
        &base,
        vec![op("alice", 1, insert(&id, 3, "A"))],
        vec![op("bob", 1, insert(&id, 3, "B"))],
    );
    assert!(text.contains('A') && text.contains('B'), "{text}");
    assert_eq!(text, "abcBAdefg");
}

#[test]
fn an_insert_after_a_concurrent_delete_keeps_its_own_anchor() {
    let (base, id) = document_with_run("0123456789");
    // alice removes three characters before bob's insertion point. Applied
    // positionally, bob's "*" slides three characters to the right.
    let text = converged_text(
        &base,
        vec![op("alice", 1, delete(&id, 0, 3))],
        vec![op("bob", 1, insert(&id, 5, "*"))],
    );
    assert_eq!(text, "34*56789");
}

#[test]
fn an_insert_before_a_concurrent_delete_is_unaffected() {
    let (base, id) = document_with_run("0123456789");
    let text = converged_text(
        &base,
        vec![op("alice", 1, delete(&id, 6, 9))],
        vec![op("bob", 1, insert(&id, 2, "*"))],
    );
    assert_eq!(text, "01*23459");
}

#[test]
fn an_insert_inside_a_concurrently_deleted_range_survives_at_its_anchor() {
    let (base, id) = document_with_run("abcdef");
    // The character bob anchored on is tombstoned rather than removed, so
    // bob's text still has somewhere to go and does not drift to the end.
    let text = converged_text(
        &base,
        vec![op("alice", 1, delete(&id, 1, 4))],
        vec![op("bob", 1, insert(&id, 3, "X"))],
    );
    assert_eq!(text, "aXef");
}

#[test]
fn disjoint_concurrent_deletes_remove_exactly_their_own_ranges() {
    let (base, id) = document_with_run("abcdefgh");
    let text = converged_text(
        &base,
        vec![op("alice", 1, delete(&id, 1, 3))],
        vec![op("bob", 1, delete(&id, 5, 7))],
    );
    assert_eq!(text, "adeh");
}

#[test]
fn overlapping_concurrent_deletes_remove_the_union_exactly_once() {
    let (base, id) = document_with_run("abcdefgh");
    // alice removes "bcde", bob removes "defg". The union is "bcdefg".
    // Positional application ran bob's [3, 7) against a string alice had
    // already shortened and ate a character that neither actor deleted.
    let text = converged_text(
        &base,
        vec![op("alice", 1, delete(&id, 1, 5))],
        vec![op("bob", 1, delete(&id, 3, 7))],
    );
    assert_eq!(text, "ah");
}

#[test]
fn a_concurrent_delete_nested_inside_another_changes_nothing() {
    let (base, id) = document_with_run("abcdefg");
    // alice removes "cd", bob removes the wider "bcdef" that contains it.
    let text = converged_text(
        &base,
        vec![op("alice", 1, delete(&id, 2, 4))],
        vec![op("bob", 1, delete(&id, 1, 6))],
    );
    assert_eq!(text, "ag");
}

#[test]
fn the_same_range_deleted_by_two_actors_is_removed_once() {
    let (base, id) = document_with_run("abcdefg");
    let text = converged_text(
        &base,
        vec![op("alice", 1, delete(&id, 2, 5))],
        vec![op("bob", 1, delete(&id, 2, 5))],
    );
    assert_eq!(text, "abfg");
}

#[test]
fn concurrent_character_edits_count_scalar_values_not_bytes() {
    let (base, id) = document_with_run("héllo wörld");
    let text = converged_text(
        &base,
        vec![op("alice", 1, delete(&id, 0, 2))],
        vec![op("bob", 1, insert(&id, 6, "Ünicode "))],
    );
    assert_eq!(text, "llo Ünicode wörld");
}

#[test]
fn three_concurrent_actors_editing_one_run_converge_on_every_grouping() {
    let (base, id) = document_with_run("the quick brown fox");
    let streams = vec![
        vec![op("alice", 1, insert(&id, 4, "very "))],
        vec![op("bob", 1, delete(&id, 10, 16))],
        vec![op("carol", 1, insert(&id, 19, " jumps"))],
    ];
    let expected = merged_text(&base, &streams);
    assert_eq!(expected, "the very quick fox jumps");

    // Every grouping and ordering of the same three operations.
    let flat: Vec<Operation> = streams.iter().flatten().cloned().collect();
    let mut seen = Vec::new();
    for a in 0..3 {
        for b in 0..3 {
            for c in 0..3 {
                if a == b || b == c || a == c {
                    continue;
                }
                let permuted = vec![
                    vec![flat[a].clone()],
                    vec![flat[b].clone(), flat[c].clone()],
                ];
                let result = merge_operations(&base, &permuted).unwrap();
                seen.push(opendoc_format::encode_canonical_cbor(&result.document).unwrap());
            }
        }
    }
    assert_eq!(seen.len(), 6);
    assert!(seen.windows(2).all(|pair| pair[0] == pair[1]));
}

#[test]
fn sequential_edits_from_one_actor_still_read_as_sequential() {
    let (base, id) = document_with_run("Hello");
    // Guard against the fix over-reaching: an actor always observes its own
    // earlier operations, so these must compose exactly as typing does.
    let text = merged_text(
        &base,
        &[vec![
            op("alice", 1, insert(&id, 5, " world")),
            op("alice", 2, insert(&id, 11, "!")),
            op("alice", 3, delete(&id, 0, 1)),
        ]],
    );
    assert_eq!(text, "ello world!");
}

#[test]
fn a_causally_later_edit_is_placed_where_its_author_saw_it() {
    let (base, id) = document_with_run("abc");
    let alice = Operation::new(
        OperationId {
            actor: ActorId("alice".to_string()),
            seq: 1,
        },
        insert(&id, 3, "XY"),
    );
    // bob saw alice's insert and typed at offset 4, i.e. between X and Y.
    let bob = Operation::in_context(
        OperationId {
            actor: ActorId("bob".to_string()),
            seq: 1,
        },
        insert(&id, 4, "-"),
        CausalContext::observing([&alice]),
    );
    // carol did not see either, and appends at the end of "abc".
    let carol = Operation::new(
        OperationId {
            actor: ActorId("carol".to_string()),
            seq: 1,
        },
        insert(&id, 3, "!"),
    );
    let text = converged_text(&base, vec![alice, bob], vec![carol]);
    assert_eq!(text, "abc!X-Y");
}

#[test]
fn a_whole_run_rewrite_beats_the_character_edits_ordered_before_it() {
    let (base, id) = document_with_run("Hello");
    let text = merged_text(
        &base,
        &[vec![
            op("alice", 1, insert(&id, 5, " world")),
            op(
                "alice",
                2,
                OperationKind::UpdateInlineText {
                    inline_id: id.clone(),
                    text: "replaced".to_string(),
                },
            ),
            op("alice", 3, insert(&id, 8, "!")),
        ]],
    );
    assert_eq!(text, "replaced!");
}

#[test]
fn character_edits_on_a_missing_or_derived_run_still_warn_once_each() {
    let mut base = Document::new("Doc");
    base.blocks.push(Block {
        id: run_id("block-1"),
        kind: BlockKind::Paragraph,
        content: vec![Inline::Mention {
            id: run_id("mention-1"),
            label: "@someone".to_string(),
        }],
        properties: BlockProperties::default(),
    });
    let mention = run_id("mention-1");
    let missing = run_id("nope");
    let result = merge_operations(
        &base,
        &[vec![
            op("alice", 1, insert(&mention, 0, "x")),
            op("alice", 2, insert(&mention, 0, "y")),
            op("alice", 3, delete(&missing, 0, 1)),
        ]],
    )
    .unwrap();
    let codes: Vec<&str> = result
        .warnings
        .iter()
        .map(|warning| warning.code.as_str())
        .collect();
    assert_eq!(
        codes,
        vec![
            "non-editable-inline",
            "non-editable-inline",
            "missing-inline"
        ]
    );
}

// ---------------------------------------------------------------------------
// causal ordering
// ---------------------------------------------------------------------------

fn alignment_op(actor: &str, seq: u64, alignment: Alignment) -> Operation {
    Operation::new(
        OperationId {
            actor: ActorId(actor.to_string()),
            seq,
        },
        OperationKind::SetBlockProperty {
            block_id: run_id("block-1"),
            property: BlockProperty::Alignment(alignment),
        },
    )
}

#[test]
fn a_causally_later_property_write_wins_regardless_of_actor_id() {
    let (base, _) = document_with_run("text");
    // "zoe" writes first. "alice" then writes *with knowledge of* zoe's write.
    // Under the old `(actor, seq)` order alice sorted first and zoe won, which
    // is the limitation ADR 0006 recorded and pointed at F1.
    let zoe = alignment_op("zoe", 1, Alignment::Center);
    let mut alice = alignment_op("alice", 1, Alignment::End);
    alice.context = Some(CausalContext::observing([&zoe]));

    let result = merge_operations(&base, &[vec![zoe.clone()], vec![alice.clone()]]).unwrap();
    assert_eq!(
        result.document.blocks[0].properties.alignment,
        Some(Alignment::End)
    );
    // …and it does not depend on which stream arrived first.
    let flipped = merge_operations(&base, &[vec![alice], vec![zoe]]).unwrap();
    assert_eq!(
        opendoc_format::encode_canonical_cbor(&result.document).unwrap(),
        opendoc_format::encode_canonical_cbor(&flipped.document).unwrap()
    );
}

#[test]
fn genuinely_concurrent_property_writes_still_break_the_tie_on_actor_id() {
    let (base, _) = document_with_run("text");
    // No causal context anywhere: the two writes are concurrent, and the
    // deterministic tie-break is unchanged.
    let result = merge_operations(
        &base,
        &[
            vec![alignment_op("alice", 1, Alignment::End)],
            vec![alignment_op("zoe", 1, Alignment::Center)],
        ],
    )
    .unwrap();
    assert_eq!(
        result.document.blocks[0].properties.alignment,
        Some(Alignment::Center)
    );
}

#[test]
fn causal_order_reproduces_actor_seq_order_when_no_context_is_supplied() {
    let operations: Vec<Operation> = [("bob", 2u64), ("alice", 2), ("bob", 1), ("alice", 1)]
        .into_iter()
        .map(|(actor, seq)| {
            op(
                actor,
                seq,
                OperationKind::SetDocumentTitle {
                    title: format!("{actor}-{seq}"),
                },
            )
        })
        .collect();
    let order: Vec<String> = causal::causal_order(&operations)
        .into_iter()
        .map(|index| {
            format!(
                "{}#{}",
                operations[index].id.actor.0, operations[index].id.seq
            )
        })
        .collect();
    assert_eq!(order, vec!["alice#1", "alice#2", "bob#1", "bob#2"]);
}

#[test]
fn causal_order_puts_an_observed_operation_before_its_observer() {
    let zoe = op(
        "zoe",
        1,
        OperationKind::SetDocumentTitle {
            title: "zoe".to_string(),
        },
    );
    let mut alice = op(
        "alice",
        1,
        OperationKind::SetDocumentTitle {
            title: "alice".to_string(),
        },
    );
    alice.context = Some(CausalContext::observing([&zoe]));
    assert!(alice.observes(&zoe.id));
    assert!(!zoe.observes(&alice.id));
    assert!(!alice.concurrent_with(&zoe));

    let operations = vec![alice, zoe];
    let order = causal::causal_order(&operations);
    assert_eq!(operations[order[0]].id.actor.0, "zoe");
    assert_eq!(operations[order[1]].id.actor.0, "alice");
}

#[test]
fn a_forged_cyclic_vector_clock_still_yields_every_operation_once() {
    // Two operations that each claim to have observed the other. This cannot
    // be produced by a replica; it can arrive over a wire. The merge must
    // still be total and deterministic rather than silently dropping work.
    let mut left = op(
        "alice",
        1,
        OperationKind::SetDocumentTitle {
            title: "left".to_string(),
        },
    );
    let mut right = op(
        "bob",
        1,
        OperationKind::SetDocumentTitle {
            title: "right".to_string(),
        },
    );
    left.context = Some(CausalContext::observing([&right]));
    right.context = Some(CausalContext::observing([&left]));
    let operations = vec![left, right];
    let order = causal::causal_order(&operations);
    assert_eq!(order.len(), 2);
    assert_ne!(order[0], order[1]);

    let (base, _) = document_with_run("text");
    let forward = merge_operations(
        &base,
        &[vec![operations[0].clone()], vec![operations[1].clone()]],
    )
    .unwrap();
    let backward = merge_operations(
        &base,
        &[vec![operations[1].clone()], vec![operations[0].clone()]],
    )
    .unwrap();
    assert_eq!(forward.document, backward.document);
}

#[test]
fn resolve_run_is_independent_of_the_order_concurrent_edits_arrive() {
    // Exercises the RGA placement rule directly, which the merge's own
    // causal-order pass cannot: here the higher-ranked concurrent edit is
    // integrated first.
    let alice = text_sequence::RunEdit {
        rank: 0,
        id: OperationId {
            actor: ActorId("alice".to_string()),
            seq: 1,
        },
        context: None,
        kind: text_sequence::RunEditKind::Insert {
            offset: 3,
            text: "AA".to_string(),
        },
    };
    let bob = text_sequence::RunEdit {
        rank: 1,
        id: OperationId {
            actor: ActorId("bob".to_string()),
            seq: 1,
        },
        context: None,
        kind: text_sequence::RunEditKind::Insert {
            offset: 3,
            text: "BB".to_string(),
        },
    };
    let carol = text_sequence::RunEdit {
        rank: 2,
        id: OperationId {
            actor: ActorId("carol".to_string()),
            seq: 1,
        },
        context: None,
        kind: text_sequence::RunEditKind::Delete { start: 4, end: 6 },
    };
    let forward =
        text_sequence::resolve_run("abcdefg", &[alice.clone(), bob.clone(), carol.clone()]);
    let backward = text_sequence::resolve_run("abcdefg", &[carol, bob, alice]);
    assert_eq!(forward, backward);
    // "ef" is tombstoned; the two concurrent inserts share an anchor and land
    // in rank order, higher rank first, whichever was integrated first.
    assert_eq!(forward, "abcBBAAdg");
}

// ---------------------------------------------------------------------------
// property / fuzz
// ---------------------------------------------------------------------------

/// Reproducible PRNG. A failure is replayable from the seed the panic prints.
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

struct Replica {
    actor: ActorId,
    next_seq: u64,
    lamport: u64,
    observed: VectorClock,
}

const FUZZ_SEEDS: u64 = 2_000;
const FUZZ_BASE: &str = "the quick brown fox jumps over the lazy dog";

/// Generate a causally consistent operation set from several replicas that
/// sync with each other at random.
fn generate_operations(
    rng: &mut Rng,
    runs: &[StableId],
    actors: usize,
    steps: usize,
    kinds: &[u8],
) -> Vec<Operation> {
    let mut replicas: Vec<Replica> = (0..actors)
        .map(|index| Replica {
            actor: ActorId(format!("actor-{index}")),
            next_seq: 1,
            lamport: 0,
            observed: VectorClock::new(),
        })
        .collect();
    let mut generated = Vec::new();
    for step in 0..steps {
        let author = rng.below(actors);
        // A sync: the author learns everything another replica knew. This is
        // what makes some operations causally ordered and others concurrent,
        // and it is the only source of cross-actor causal edges.
        if rng.below(3) == 0 {
            let source = rng.below(actors);
            if source != author {
                let (clock, lamport) =
                    (replicas[source].observed.clone(), replicas[source].lamport);
                replicas[author].observed.join(&clock);
                replicas[author].lamport = replicas[author].lamport.max(lamport);
            }
        }
        let replica = &mut replicas[author];
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

        let inline_id = runs[rng.below(runs.len())].clone();
        let kind = match kinds[rng.below(kinds.len())] {
            0 => OperationKind::InsertText {
                inline_id,
                offset: rng.below(FUZZ_BASE.chars().count() + 6),
                text: format!("<{step}>"),
            },
            1 => {
                let start = rng.below(FUZZ_BASE.chars().count() + 6);
                OperationKind::DeleteText {
                    inline_id,
                    start,
                    end: start + 1 + rng.below(7),
                }
            }
            2 => OperationKind::SetBlockProperty {
                block_id: run_id("block-1"),
                property: BlockProperty::IndentStart(
                    Length::from_twips((rng.below(400) as i32) * 3).unwrap(),
                ),
            },
            _ => OperationKind::SetBlockProperty {
                block_id: run_id("block-1"),
                property: BlockProperty::Alignment(match rng.below(4) {
                    0 => Alignment::Start,
                    1 => Alignment::Center,
                    2 => Alignment::End,
                    _ => Alignment::Justify,
                }),
            },
        };
        generated.push(Operation::in_context(id, kind, context));
    }
    generated
}

/// Split `operations` into a random number of streams, shuffling both the
/// stream contents and the order within each stream.
fn shuffle_into_streams(rng: &mut Rng, operations: &[Operation]) -> Vec<Vec<Operation>> {
    let stream_count = 1 + rng.below(4);
    let mut streams: Vec<Vec<Operation>> = vec![Vec::new(); stream_count];
    let mut shuffled = operations.to_vec();
    for index in (1..shuffled.len()).rev() {
        shuffled.swap(index, rng.below(index + 1));
    }
    for operation in shuffled {
        let target = rng.below(stream_count);
        streams[target].push(operation);
    }
    streams
}

#[test]
fn randomised_concurrent_operation_sets_converge_byte_identically() {
    let mut divergent_from_base = 0usize;
    for seed in 0..FUZZ_SEEDS {
        let mut rng = Rng::new(seed);
        let mut base = Document::new("Doc");
        let runs = vec![run_id("text-1"), run_id("text-2")];
        base.blocks.push(Block {
            id: run_id("block-1"),
            kind: BlockKind::Paragraph,
            content: runs
                .iter()
                .map(|id| Inline::Text {
                    id: id.clone(),
                    text: FUZZ_BASE.to_string(),
                    marks: Vec::new(),
                })
                .collect(),
            properties: BlockProperties::default(),
        });

        let mut operations = generate_operations(&mut rng, &runs, 4, 14, &[0, 0, 1, 2, 3]);
        // Redeliver a deterministic slice of the set, so every permutation
        // carries the same multiset and duplicate handling is exercised.
        let duplicates: Vec<Operation> = operations
            .iter()
            .enumerate()
            .filter(|(index, _)| index % 5 == 0)
            .map(|(_, operation)| operation.clone())
            .collect();
        operations.extend(duplicates);

        let mut expected: Option<Vec<u8>> = None;
        for permutation in 0..12 {
            let streams = shuffle_into_streams(&mut rng, &operations);
            let result = merge_operations(&base, &streams)
                .unwrap_or_else(|err| panic!("seed {seed} permutation {permutation}: {err}"));
            let encoded = opendoc_format::encode_canonical_cbor(&result.document).unwrap();
            match &expected {
                None => {
                    if result.document.visible_text() != base.visible_text() {
                        divergent_from_base += 1;
                    }
                    expected = Some(encoded);
                }
                Some(first) => assert_eq!(
                    *first,
                    encoded,
                    "seed {seed} permutation {permutation} diverged; text was {:?}",
                    result.document.visible_text()
                ),
            }
        }
    }
    // Guard against a vacuous pass: the generated operations must actually be
    // changing the document.
    assert!(
        divergent_from_base > FUZZ_SEEDS as usize * 9 / 10,
        "only {divergent_from_base} of {FUZZ_SEEDS} seeds changed the document"
    );
}

#[test]
fn randomised_concurrent_inserts_survive_whole_and_contiguous() {
    // An oracle the merge cannot fake: with no deletes in play, every
    // character every actor typed must be present, each insert must appear as
    // one unbroken run, and removing the inserts must give back the base.
    // Positional application fails this the moment two inserts interleave.
    for seed in 0..FUZZ_SEEDS {
        let mut rng = Rng::new(seed ^ 0x9e37_79b9);
        let (base, id) = document_with_run(FUZZ_BASE);
        let base_length = FUZZ_BASE.chars().count();

        let mut operations = Vec::new();
        let mut expected_pieces = Vec::new();
        let actor_count = 2 + rng.below(4);
        for actor in 0..actor_count {
            let marker = (b'A' + actor as u8) as char;
            let piece: String = std::iter::repeat_n(marker, 1 + rng.below(4)).collect();
            operations.push(op(
                &format!("actor-{actor}"),
                1,
                insert(&id, rng.below(base_length + 1), &piece),
            ));
            expected_pieces.push(piece);
        }

        let streams = shuffle_into_streams(&mut rng, &operations);
        let text = visible_run_text(&merge_operations(&base, &streams).unwrap().document);

        let inserted_length: usize = expected_pieces
            .iter()
            .map(|piece| piece.chars().count())
            .sum();
        assert_eq!(
            text.chars().count(),
            base_length + inserted_length,
            "seed {seed}: merged text lost or gained characters: {text:?}"
        );
        for piece in &expected_pieces {
            assert_eq!(
                text.matches(piece.as_str()).count(),
                1,
                "seed {seed}: insert {piece:?} was split or duplicated in {text:?}"
            );
        }
        let stripped: String = text.chars().filter(|ch| !ch.is_ascii_uppercase()).collect();
        assert_eq!(stripped, FUZZ_BASE, "seed {seed}: base text was disturbed");
    }
}

#[test]
fn randomised_concurrent_deletes_remove_exactly_the_union_of_their_ranges() {
    // The second oracle: concurrent deletes against a shared base have an
    // answer that can be computed without the merge at all — the base minus
    // the union of the ranges. Positional application over-deletes as soon as
    // two ranges overlap.
    for seed in 0..FUZZ_SEEDS {
        let mut rng = Rng::new(seed ^ 0x1234_5678);
        let (base, id) = document_with_run(FUZZ_BASE);
        let base_length = FUZZ_BASE.chars().count();

        let mut operations = Vec::new();
        let mut removed = vec![false; base_length];
        let actor_count = 2 + rng.below(4);
        for actor in 0..actor_count {
            let start = rng.below(base_length);
            let end = (start + 1 + rng.below(12)).min(base_length);
            operations.push(op(&format!("actor-{actor}"), 1, delete(&id, start, end)));
            for slot in removed.iter_mut().take(end).skip(start) {
                *slot = true;
            }
        }
        let expected: String = FUZZ_BASE
            .chars()
            .enumerate()
            .filter(|(index, _)| !removed[*index])
            .map(|(_, ch)| ch)
            .collect();

        let streams = shuffle_into_streams(&mut rng, &operations);
        let text = visible_run_text(&merge_operations(&base, &streams).unwrap().document);
        assert_eq!(
            text, expected,
            "seed {seed}: deletes did not remove the union"
        );
    }
}

#[test]
fn concurrent_deletes_that_between_them_empty_a_run_still_validate() {
    let (base, id) = document_with_run("abcd");
    let text = converged_text(
        &base,
        vec![op("alice", 1, delete(&id, 0, 2))],
        vec![op("bob", 1, delete(&id, 2, 4))],
    );
    assert_eq!(text, "");
}

#[test]
fn a_deleted_run_reports_one_warning_per_lost_character_operation() {
    let (base, id) = document_with_run("abcd");
    // alice removes the run outright while bob is still typing into it.
    let result = merge_operations(
        &base,
        &[
            vec![op(
                "alice",
                1,
                OperationKind::DeleteInline {
                    inline_id: id.clone(),
                },
            )],
            vec![
                op("bob", 1, insert(&id, 1, "X")),
                op("bob", 2, delete(&id, 0, 1)),
            ],
        ],
    )
    .unwrap();
    let codes: Vec<&str> = result
        .warnings
        .iter()
        .map(|warning| warning.code.as_str())
        .collect();
    assert_eq!(codes, vec!["missing-inline", "missing-inline"]);
}
