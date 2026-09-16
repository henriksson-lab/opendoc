//! Tests for causal ordering and character-level convergence.
//!
//! See `docs/adr/0007-causal-ordering-and-text-convergence.md`. Every test in
//! the "transformation cases" section below was run against the pre-ADR
//! behaviour (positional application, `(actor, seq)` ordering) and fails
//! there; the assertions name the exact text, not a `contains`, so that a
//! merge that corrupts concurrent edits cannot pass them.

use opendoc_core::{
    Alignment, Block, BlockKind, BlockProperties, BlockProperty, BlockPropertyKey, Document,
    Inline, Length, StableId,
};
use std::collections::{BTreeMap, BTreeSet};

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
/// Grouping agreement is **not** the check and never was: `merge_operations`
/// folds every stream into one `BTreeMap<OperationId, Operation>` before a
/// line of semantics runs, so the result is a function of the operation set
/// and the five groupings below agree for any implementation at all. PLAN88
/// §7. What can fail is the exact string every caller then asserts, and the
/// positive control here: each side, merged on its own, has to change the
/// document. Without that, a merge that discarded one actor's stream — or
/// every operation — satisfied the grouping half unchanged.
fn converged_text(base: &Document, left: Vec<Operation>, right: Vec<Operation>) -> String {
    for (label, side) in [("left", &left), ("right", &right)] {
        let alone = merge_operations(base, &[side.to_vec()]).unwrap();
        assert_ne!(
            &alone.document, base,
            "the {label} operations left the document untouched, so this case \
             cannot tell a merge from a no-op"
        );
    }
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
fn the_vector_clock_orders_operations_even_when_the_lamport_stamp_does_not() {
    // `causal_order` has two sources of truth about happened-before: the
    // explicit predecessor edges it builds from `context.observed`, and the
    // `(lamport, actor, seq)` priority it drains the ready set with. A correct
    // Lamport stamp already puts an observer after what it observed, so the
    // edges are *redundant on well-formed input* — and every other test here
    // supplies well-formed input, so deleting the `observed` loop from
    // `causal_order` entirely used to leave the whole suite green.
    //
    // A replica is not the only thing that writes a context. Anything that
    // arrives over a wire can carry a stamp that does not increase — a peer
    // that resets its clock, a replayed frame, a forgery — and then the edges
    // are the only thing holding the order up. So: one flat Lamport stamp, an
    // honest vector clock, and an actor ordering that points the wrong way.
    let (base, _) = document_with_run("text");
    let zoe = Operation::in_context(
        OperationId {
            actor: ActorId("zoe".to_string()),
            seq: 1,
        },
        OperationKind::SetBlockProperty {
            block_id: run_id("block-1"),
            property: BlockProperty::Alignment(Alignment::Center),
        },
        CausalContext {
            lamport: 7,
            observed: VectorClock::new(),
        },
    );
    // alice observed zoe's write, and says so — but stamps the same 7, and
    // sorts before "zoe" on actor id.
    let mut observed = VectorClock::new();
    observed.observe(&zoe.id);
    let alice = Operation::in_context(
        OperationId {
            actor: ActorId("alice".to_string()),
            seq: 1,
        },
        OperationKind::SetBlockProperty {
            block_id: run_id("block-1"),
            property: BlockProperty::Alignment(Alignment::End),
        },
        CausalContext {
            lamport: 7,
            observed,
        },
    );

    let order = causal::causal_order(&[alice.clone(), zoe.clone()]);
    assert_eq!(
        order,
        vec![1, 0],
        "the observed clock did not put zoe before alice"
    );
    for grouping in [
        vec![vec![zoe.clone()], vec![alice.clone()]],
        vec![vec![alice.clone()], vec![zoe.clone()]],
        vec![vec![alice.clone(), zoe.clone()]],
        vec![vec![zoe, alice]],
    ] {
        let result = merge_operations(&base, &grouping).unwrap();
        assert_eq!(
            result.document.blocks[0].properties.alignment,
            Some(Alignment::End),
            "the write that observed the other one did not win"
        );
    }
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
            // One distinct uppercase letter per step, repeated. `FUZZ_BASE`
            // holds only lowercase and spaces, so the merged text says
            // unambiguously which operation wrote each character — which is
            // what lets the oracle below count them. A bracketed number would
            // not: a delete that ate the `1` of `<10>` would leave a second
            // `<0>` and read as a duplicated insert.
            0 => OperationKind::InsertText {
                inline_id,
                offset: rng.below(FUZZ_BASE.chars().count() + 6),
                text: std::iter::repeat_n((b'A' + step as u8) as char, 1 + step % 3).collect(),
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

/// Every operation in `operations` that wrote `key`, de-duplicated by
/// identity. Computed from the operation set alone — this is oracle material,
/// so it must never call the merge.
fn property_writers(operations: &[Operation], key: BlockPropertyKey) -> Vec<&Operation> {
    let mut seen: BTreeSet<OperationId> = BTreeSet::new();
    let mut writers = Vec::new();
    for operation in operations {
        if let OperationKind::SetBlockProperty { property, .. } = &operation.kind {
            if property.key() == key && seen.insert(operation.id.clone()) {
                writers.push(operation);
            }
        }
    }
    writers
}

/// The maximal elements of happened-before among `writers`.
///
/// Last-writer-wins over a partial order has an answer that does not need the
/// merge to find: an operation some other writer already had applied cannot be
/// the last word, so the winner is maximal. And when the maximal element is
/// *unique* it is the winner outright — every other writer is below it, so
/// every linear extension puts it last. (If some writer `x` were not below the
/// unique maximal `w`, take `m` maximal among the writers not below `w`;
/// anything above `m` would also not be below `w`, so `m` is maximal in the
/// whole set, so `m = w` — and `w` is below `w`.)
fn causally_last<'a>(writers: &[&'a Operation]) -> Vec<&'a Operation> {
    writers
        .iter()
        .filter(|candidate| {
            !writers
                .iter()
                .any(|other| other.id != candidate.id && other.observes(&candidate.id))
        })
        .copied()
        .collect()
}

fn written_property(operation: &Operation) -> BlockProperty {
    match &operation.kind {
        OperationKind::SetBlockProperty { property, .. } => *property,
        other => panic!("not a property write: {other:?}"),
    }
}

/// Is `text` a subsequence of `whole`?
fn is_subsequence(text: &str, whole: &str) -> bool {
    let mut remaining = whole.chars();
    text.chars()
        .all(|wanted| remaining.any(|candidate| candidate == wanted))
}

#[test]
fn randomised_concurrent_operation_sets_converge_byte_identically() {
    // Convergence is the *weak* half of this test and always was: the merge
    // folds every stream into one `BTreeMap<OperationId, Operation>` before a
    // line of semantics runs, so "grouping A agrees with grouping B" is true
    // by construction and holds for any implementation — including one that
    // abandons causal ordering entirely. PLAN88 §7.
    //
    // So each seed also carries oracles, computed from the operation set
    // without the merge:
    //
    // * the winning value of each block property is the one written by the
    //   causally last writer of that key, which `causally_last` finds from the
    //   vector clocks alone;
    // * no base character is duplicated, invented or reordered — what is left
    //   of the base is a subsequence of it;
    // * no insert is duplicated.
    let mut divergent_from_base = 0usize;
    let mut decided_by_causality = 0usize;
    let mut pinned_by_a_unique_winner = 0usize;
    let mut surviving_inserts = 0usize;
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
                    // ---- the oracles -------------------------------------
                    let properties = &result.document.blocks[0].properties;
                    for key in [BlockPropertyKey::Alignment, BlockPropertyKey::IndentStart] {
                        let writers = property_writers(&operations, key);
                        let winners = causally_last(&writers);
                        let actual = properties.get(key);
                        if writers.is_empty() {
                            assert_eq!(
                                actual, None,
                                "seed {seed}: no operation wrote {key:?}, yet the merge did"
                            );
                            continue;
                        }
                        if let [only] = winners.as_slice() {
                            pinned_by_a_unique_winner += 1;
                            assert_eq!(
                                actual,
                                Some(written_property(only)),
                                "seed {seed}: {key:?} did not come from the causally last \
                                 writer {}#{}",
                                only.id.actor.0,
                                only.id.seq
                            );
                            // Did causality decide it, or would the old
                            // `(actor, seq)` order have picked the same one?
                            // This counter is what makes the oracle bite: a
                            // merge that ignores the vector clocks answers
                            // these seeds differently.
                            let naive = writers
                                .iter()
                                .max_by_key(|operation| {
                                    (operation.id.actor.0.clone(), operation.id.seq)
                                })
                                .expect("writers is not empty");
                            if naive.id != only.id {
                                decided_by_causality += 1;
                            }
                        } else {
                            // Genuinely concurrent writers: the merge may pick
                            // any of them, but not one they all superseded and
                            // not a value nobody wrote.
                            assert!(
                                winners
                                    .iter()
                                    .any(|winner| actual == Some(written_property(winner))),
                                "seed {seed}: {key:?} is {actual:?}, which no causally last \
                                 writer wrote"
                            );
                        }
                    }

                    let text = result.document.visible_text();
                    let text = text.trim_end_matches('\n');
                    // What is left of the base must still be the base, in
                    // order: the merge may delete characters, never duplicate,
                    // reorder or invent them. Every inserted character is an
                    // uppercase letter; `FUZZ_BASE` holds none.
                    let base_only: String =
                        text.chars().filter(|ch| !ch.is_ascii_uppercase()).collect();
                    assert!(
                        is_subsequence(&base_only, &FUZZ_BASE.repeat(runs.len())),
                        "seed {seed}: the surviving base text is not a subsequence of the \
                         base: {base_only:?}"
                    );
                    // …and no insert is delivered twice. Each operation writes
                    // its own letter and re-deliveries share an `OperationId`,
                    // so a delete can remove those characters but nothing can
                    // ever produce more of them than were written.
                    let mut written: BTreeMap<char, usize> = BTreeMap::new();
                    for operation in &operations {
                        if let OperationKind::InsertText { text: inserted, .. } = &operation.kind {
                            let letter = inserted.chars().next().expect("markers are non-empty");
                            let slot = written.entry(letter).or_default();
                            *slot = (*slot).max(inserted.chars().count());
                        }
                    }
                    for (letter, count) in &written {
                        let seen = text.chars().filter(|ch| ch == letter).count();
                        assert!(
                            seen <= *count,
                            "seed {seed}: {seen} copies of {letter:?} survived, but only \
                             {count} were ever inserted"
                        );
                        surviving_inserts += seen;
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
    // …and against the oracles above never reaching a case worth checking.
    assert!(
        pinned_by_a_unique_winner > FUZZ_SEEDS as usize / 2,
        "only {pinned_by_a_unique_winner} property values were pinned to a unique causal winner"
    );
    assert!(
        decided_by_causality > FUZZ_SEEDS as usize / 20,
        "causality separated the winner from the (actor, seq) maximum in only \
         {decided_by_causality} cases, so the oracle would not notice a merge that \
         ignored the vector clocks"
    );
    assert!(
        surviving_inserts > FUZZ_SEEDS as usize,
        "only {surviving_inserts} inserts survived in one piece across {FUZZ_SEEDS} seeds"
    );
}

#[test]
fn randomised_causally_linear_edit_scripts_match_a_plain_string_simulation() {
    // The strongest oracle available for mixed inserts and deletes: make every
    // operation observe all of its predecessors, and there is no concurrency
    // left to resolve — the answer is the one a plain `Vec<char>` gives when
    // each edit is applied, in causal order, at the offset its author wrote.
    //
    // The merge has to reach that answer from a *shuffled* delivery in which
    // the causal order is not the arrival order and is not the `(actor, seq)`
    // order either. A merge that ordered operations by anything but
    // happened-before gets a different string here, and no amount of
    // permutation agreement would have said so.
    let mut deleting_seeds = 0usize;
    let mut reordered_seeds = 0usize;
    for seed in 0..FUZZ_SEEDS {
        let mut rng = Rng::new(seed ^ 0xc0ff_ee11);
        let (base, id) = document_with_run(FUZZ_BASE);
        let mut expected: Vec<char> = FUZZ_BASE.chars().collect();

        const ACTORS: usize = 3;
        let mut next_seq = [1u64; ACTORS];
        // One clock shared by every replica: each operation is generated on a
        // replica that has already applied everything before it.
        let mut observed = VectorClock::new();
        let mut operations: Vec<Operation> = Vec::new();
        let mut deleted_anything = false;
        for step in 0..10 {
            let actor = rng.below(ACTORS);
            let operation_id = OperationId {
                actor: ActorId(format!("actor-{actor}")),
                seq: next_seq[actor],
            };
            next_seq[actor] += 1;
            // One Lamport tick per operation. The clock is shared, so it is
            // strictly increasing along the generation order, which is exactly
            // the causal order this script builds.
            let context = CausalContext {
                lamport: step as u64 + 1,
                observed: observed.clone(),
            };
            observed.observe(&operation_id);

            let kind = if expected.is_empty() || rng.below(3) != 0 {
                let offset = rng.below(expected.len() + 1);
                let inserted = format!("<{step}>");
                for (index, ch) in inserted.chars().enumerate() {
                    expected.insert(offset + index, ch);
                }
                insert(&id, offset, &inserted)
            } else {
                deleted_anything = true;
                let start = rng.below(expected.len());
                let end = (start + 1 + rng.below(5)).min(expected.len());
                expected.drain(start..end);
                delete(&id, start, end)
            };
            operations.push(Operation::in_context(operation_id, kind, context));
        }
        if deleted_anything {
            deleting_seeds += 1;
        }
        // Did the causal order differ from the order the old `(actor, seq)`
        // rule would have produced? If it never did, the oracle could not tell
        // the two apart.
        let mut naive: Vec<&Operation> = operations.iter().collect();
        naive.sort_by_key(|operation| (operation.id.actor.0.clone(), operation.id.seq));
        if naive
            .iter()
            .zip(operations.iter())
            .any(|(left, right)| left.id != right.id)
        {
            reordered_seeds += 1;
        }

        let streams = shuffle_into_streams(&mut rng, &operations);
        let text = visible_run_text(&merge_operations(&base, &streams).unwrap().document);
        assert_eq!(
            text,
            expected.iter().collect::<String>(),
            "seed {seed}: the merge of a causally linear script disagreed with applying it"
        );
    }
    assert!(
        deleting_seeds > FUZZ_SEEDS as usize / 4,
        "only {deleting_seeds} of {FUZZ_SEEDS} seeds deleted anything"
    );
    assert!(
        reordered_seeds > FUZZ_SEEDS as usize / 2,
        "the causal order matched the (actor, seq) order in all but {reordered_seeds} seeds, \
         so this oracle would not notice a merge that ignored causality"
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

// ---------------------------------------------------------------------------
// what stream-grouping agreement cannot see
// ---------------------------------------------------------------------------
//
// `merge_operations` de-duplicates every stream into one
// `BTreeMap<OperationId, Operation>` and hands `causal_order` the values of
// that map — always in `(actor, seq)` order, whatever the caller passed. The
// merge's answer is therefore a function of the operation *set* and the base,
// and "grouping A agrees with grouping B" is true by construction: it holds
// for a merge that ignores causality, and for one that drops every operation.
// PLAN88 §7.
//
// The three tests below are the ones grouping agreement was standing in for.
// Two attack `causal_order` on the axis where order genuinely is an input —
// the slice it is handed — and the third makes the tautology itself
// enforceable, by keeping a deliberately causality-blind merge in the file and
// requiring that the real one disagrees with it.

/// The operation ids `causal_order` emits, in the order it emits them.
fn ordered_ids(operations: &[Operation]) -> Vec<OperationId> {
    causal::causal_order(operations)
        .into_iter()
        .map(|index| operations[index].id.clone())
        .collect()
}

/// The same total order computed a different way.
///
/// `causal_order` builds a predecessor graph — one edge per actor named in a
/// vector clock, to that actor's newest operation at or below the sequence the
/// clock records — and drains it with a priority queue. This does none of
/// that: it asks `Operation::observes` about every pair directly, and at each
/// step emits the smallest `(lamport, actor, seq)` operation whose whole
/// causal past has already been emitted. Kahn's algorithm with a total
/// priority over the ready set has exactly one answer, so the two must agree
/// operation for operation — and they disagree the moment the graph is built
/// from the wrong edges.
fn reference_causal_order(operations: &[Operation]) -> Vec<OperationId> {
    let mut emitted: BTreeSet<OperationId> = BTreeSet::new();
    let mut order = Vec::with_capacity(operations.len());
    while order.len() < operations.len() {
        let next = operations
            .iter()
            .filter(|candidate| !emitted.contains(&candidate.id))
            .filter(|candidate| {
                operations.iter().all(|other| {
                    other.id == candidate.id
                        || emitted.contains(&other.id)
                        || !candidate.observes(&other.id)
                })
            })
            .min_by_key(|candidate| {
                (
                    candidate.lamport(),
                    candidate.id.actor.0.clone(),
                    candidate.id.seq,
                )
            });
        let Some(next) = next else {
            // Nothing is ready and work remains: the input has a cycle, which
            // this reference does not model. The generator never makes one.
            panic!("the generated operation set has a causal cycle");
        };
        emitted.insert(next.id.clone());
        order.push(next.id.clone());
    }
    order
}

fn shuffled(rng: &mut Rng, operations: &[Operation]) -> Vec<Operation> {
    let mut out = operations.to_vec();
    for index in (1..out.len()).rev() {
        out.swap(index, rng.below(index + 1));
    }
    out
}

/// The same operations with every Lamport stamp flattened to 1.
///
/// A well-formed Lamport stamp already puts an observer after what it
/// observed, so on honest input the vector-clock edges are redundant and a
/// `causal_order` that never read them would still pass. Flattening the stamps
/// — a peer that reset its clock, a replayed frame, a forgery — leaves the
/// edges as the only thing holding the order up.
fn with_flat_lamports(operations: &[Operation]) -> Vec<Operation> {
    operations
        .iter()
        .map(|operation| {
            let mut flattened = operation.clone();
            if let Some(context) = &mut flattened.context {
                context.lamport = 1;
            }
            flattened
        })
        .collect()
}

fn generated_set(seed: u64) -> Vec<Operation> {
    let mut rng = Rng::new(seed);
    let runs = vec![run_id("text-1"), run_id("text-2")];
    generate_operations(&mut rng, &runs, 4, 12, &[0, 1, 2, 3])
}

#[test]
fn causal_order_is_the_same_total_order_however_its_input_is_permuted() {
    // `merge_operations` always hands `causal_order` a `(actor, seq)`-sorted
    // vector, so no test that goes through the merge can see this. The
    // contract is nonetheless that the order is a function of the operation
    // set: the ready-set priority is `(lamport, actor, seq)`, which separates
    // every pair of distinct operations. Weaken that key and the binary heap
    // falls back to the position the operation happened to arrive at, and the
    // merge's answer starts depending on its caller.
    let mut permuted_inputs = 0usize;
    let mut non_identity_orders = 0usize;
    for seed in 0..400u64 {
        let operations = generated_set(seed ^ 0x50e1_4b1e);
        let expected = ordered_ids(&operations);
        let identity: Vec<OperationId> = operations.iter().map(|one| one.id.clone()).collect();
        if expected != identity {
            non_identity_orders += 1;
        }
        let mut rng = Rng::new(seed ^ 0xfeed_5eed);
        for permutation in 0..6 {
            let input = shuffled(&mut rng, &operations);
            let input_ids: Vec<OperationId> = input.iter().map(|one| one.id.clone()).collect();
            if input_ids != identity {
                permuted_inputs += 1;
            }
            assert_eq!(
                ordered_ids(&input),
                expected,
                "seed {seed} permutation {permutation}: causal_order answered \
                 differently for a re-ordering of the same operation set"
            );
        }
    }
    // The guards: the permutations have to really permute, and the answer has
    // to be something other than "whatever order you gave me", or an identity
    // implementation would satisfy the loop above.
    assert!(
        permuted_inputs > 2_000,
        "only {permuted_inputs} of 2,400 permutations changed the input order"
    );
    assert!(
        non_identity_orders > 200,
        "causal_order returned its input order for all but {non_identity_orders} \
         of 400 seeds, so permuting the input could not tell it apart from the identity"
    );
}

#[test]
fn causal_order_agrees_with_an_independent_topological_sort() {
    // The oracle for the order itself, rather than for the document it
    // produces: a second implementation over the same definition of
    // happened-before, sharing no code with the first.
    let mut reordered = 0usize;
    let mut flat_reordered = 0usize;
    for seed in 0..400u64 {
        let operations = generated_set(seed ^ 0x7013_9a17);
        let actual = ordered_ids(&operations);
        assert_eq!(
            actual,
            reference_causal_order(&operations),
            "seed {seed}: causal_order disagreed with an independent topological sort"
        );
        let naive: Vec<OperationId> = {
            let mut ids: Vec<OperationId> = operations.iter().map(|one| one.id.clone()).collect();
            ids.sort();
            ids
        };
        if actual != naive {
            reordered += 1;
        }

        // …and again with the Lamport stamps flattened, where the vector-clock
        // edges are the only thing left to order by.
        let flattened = with_flat_lamports(&operations);
        let flat_actual = ordered_ids(&flattened);
        assert_eq!(
            flat_actual,
            reference_causal_order(&flattened),
            "seed {seed}: causal_order disagreed with the reference once the \
             Lamport stamps stopped separating the operations"
        );
        if flat_actual != naive {
            flat_reordered += 1;
        }
    }
    assert!(
        reordered > 100,
        "the causal order matched plain (actor, seq) order in all but {reordered} \
         of 400 seeds, so this oracle would not notice a merge that ignored causality"
    );
    assert!(
        flat_reordered > 40,
        "with flat Lamport stamps the order was plain (actor, seq) in all but \
         {flat_reordered} of 400 seeds, so the vector-clock edges are not being \
         exercised"
    );
}

/// A merge that is wrong in exactly the way no grouping comparison can see: it
/// throws every causal context away and folds the operation set in
/// `(actor, seq)` order, which is what this crate did before ADR 0007. It
/// shares all of its other semantics with the real merge, so the only thing
/// that can separate the two is causality.
fn causality_blind_merge(base: &Document, streams: &[Vec<Operation>]) -> Document {
    let stripped: Vec<Vec<Operation>> = streams
        .iter()
        .map(|stream| {
            stream
                .iter()
                .map(|operation| {
                    let mut blind = operation.clone();
                    blind.context = None;
                    blind
                })
                .collect()
        })
        .collect();
    merge_operations(base, &stripped).unwrap().document
}

#[test]
fn a_merge_that_ignored_causality_would_pass_every_grouping_check_in_this_file() {
    // The negative control for the whole convergence suite. Two whole-run
    // rewrites of one inline: "zoe" wrote first, "alice" wrote afterwards
    // having seen zoe's text. The last writer is alice, and only the vector
    // clock says so — on actor id alone, "alice" sorts first and zoe wins.
    let (base, id) = document_with_run("shared");
    let zoe = Operation::new(
        OperationId {
            actor: ActorId("zoe".to_string()),
            seq: 1,
        },
        OperationKind::UpdateInlineText {
            inline_id: id.clone(),
            text: "zoe wrote this".to_string(),
        },
    );
    let alice = Operation::in_context(
        OperationId {
            actor: ActorId("alice".to_string()),
            seq: 1,
        },
        OperationKind::UpdateInlineText {
            inline_id: id.clone(),
            text: "alice wrote this".to_string(),
        },
        CausalContext::observing([&zoe]),
    );
    let groupings: Vec<Vec<Vec<Operation>>> = vec![
        vec![vec![zoe.clone()], vec![alice.clone()]],
        vec![vec![alice.clone()], vec![zoe.clone()]],
        vec![vec![zoe.clone(), alice.clone()]],
        vec![vec![alice.clone(), zoe.clone()]],
        vec![vec![alice.clone()], vec![zoe.clone()], Vec::new()],
    ];

    // The wrong merge is *perfectly* convergent. Every grouping assertion in
    // this file — 5 groupings here, 12 permutations in the randomised test —
    // is satisfied by it.
    let mut blind: Option<Vec<u8>> = None;
    for grouping in &groupings {
        let encoded =
            opendoc_format::encode_canonical_cbor(&causality_blind_merge(&base, grouping)).unwrap();
        match &blind {
            None => blind = Some(encoded),
            Some(first) => assert_eq!(
                *first, encoded,
                "even the causality-blind merge is grouping-invariant, which is \
                 the point: grouping agreement cannot fail"
            ),
        }
    }

    // So the assertion that can fail is that the real merge is not that merge.
    let blind_text = visible_run_text(&causality_blind_merge(&base, &groupings[0]));
    assert_eq!(blind_text, "zoe wrote this");
    for (index, grouping) in groupings.iter().enumerate() {
        let real = merge_operations(&base, grouping).unwrap();
        let text = visible_run_text(&real.document);
        assert_eq!(
            text, "alice wrote this",
            "grouping {index}: the causally later rewrite did not win"
        );
        assert_ne!(
            text, blind_text,
            "grouping {index}: the merge agrees with a merge that throws every \
             causal context away, so every stream-grouping assertion in this \
             file is satisfied by an implementation that ignores concurrency"
        );
    }
}
