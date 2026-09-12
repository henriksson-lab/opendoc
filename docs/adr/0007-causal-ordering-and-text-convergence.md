# ADR 0007: Causal Ordering and Character-Level Text Convergence

Status: accepted for v0. Supersedes the sequencing half of ADR 0001 and lifts
the "Limits" section of ADR 0006.

## Context

`merge_operations` collected every operation into a `BTreeMap` keyed on
`OperationId { actor, seq }` and applied them in that key order. Two things
were wrong with that, and they are different problems:

1. **Ordering was not causal.** `(actor, seq)` is a *deterministic* order, so
   replicas that saw the same operation set agreed — which is all the existing
   convergence suite asserted. But it is not a *causal* order. "Last writer
   wins", the rule ADR 0006 gave block properties, meant "the writer whose
   actor id sorts last". An actor named `alice` could be permanently
   out-voted by `bob` on every property, even when `alice` edited afterwards
   with full knowledge of `bob`'s edit. Concurrency was not a decidable
   property of the data at all: it was an artifact of how the operations
   happened to be grouped into streams by the caller.

2. **Character offsets were never transformed.** `InsertText { inline_id,
   offset, text }` and `DeleteText { inline_id, start, end }` carry integer
   offsets into a text run. The merge applied each one at its *original*
   offset against a document that earlier operations had already shifted.
   Concurrent edits therefore converged to **wrong text**, not merely
   conflicting text. If A inserts at offset 5 and B deletes `[0, 3)`
   concurrently, A's insert lands three characters late in the merged
   document. Every replica produced the *same* wrong text, so the suite was
   green.

ADR 0001 promised an Automerge-style CRDT. Nothing in the repository delivered
one; the document model stores `Inline::Text { id, text: String }` and always
has.

## Constraints that decide this

- `opendoc-core::Document` is the **stored, content-addressed, signed**
  artifact. Changing its shape changes what gets hashed and signed
  (ADR 0002), and every other crate reads it.
- `Document` derives `Eq` and must serialize to byte-identical canonical CBOR
  across replicas. ADR 0006 chose integer twips over `f64` precisely to keep
  that true.
- Operations are already typed and journalled (`AppOperationEnvelope`), and
  the journal is replayed for crash recovery (ADR 0005).
- Only two operation kinds carry offsets at all. Every other sequence in the
  model — blocks, inlines, table rows, table cells — is already ordered by
  `after: Option<StableId>`, i.e. by identity, not by index. Mark ranges use
  `TextRange` of `StableId`s. The offset problem is *confined to the inside of
  a single text run*.

## Decision

Two changes, addressing the two problems separately.

### 1. Causality is explicit metadata; ordering is a topological sort

`Operation` gains an optional causal context:

```rust
pub struct CausalContext {
    pub lamport: u64,
    pub observed: VectorClock,   // actor -> highest seq observed
}

pub struct Operation {
    pub id: OperationId,
    pub kind: OperationKind,
    pub context: Option<CausalContext>,
}
```

`Operation::observes(&OperationId)` is the decidable happened-before test:

- an operation always observes its own actor's lower sequence numbers;
- otherwise it observes exactly what its vector clock says it observed.

`merge_operations` no longer iterates a `BTreeMap`. It runs a **deterministic
topological sort**: edges from every operation to the operations it observes,
and among the ready set it takes the smallest `(lamport, actor, seq)`. The
result is a total order that is a linear extension of happened-before, and it
is a pure function of the operation *set* — not of stream grouping, not of
receive order.

Last-writer-wins now means *higher Lamport timestamp wins*, with `(actor, seq)`
only as the tie-break between genuinely concurrent writes. ADR 0006's
granularity (per property) is unchanged; only the definition of "last" changed,
and no payload changed with it, exactly as that ADR predicted.

**`context: None` degrades to the previous behaviour, deliberately.** With no
context anywhere, the only edges are each actor's own chain, the ready-set
priority is `(0, actor, seq)`, and the topological sort emits precisely the old
`BTreeMap<(actor, seq)>` order. Operations written before this ADR keep their
exact previous merge result. `None` is also the *conservative* reading for
concurrency: an operation with no context is treated as concurrent with every
other actor's work, which is the truth for the two-branch merge in
`repository_io` and is never an over-claim.

### 2. Text runs converge by identity, not by transformation

We considered two candidates.

**Rejected: operational transformation over the typed ops.** Keeping offsets
and transforming concurrent operations against each other is the smaller
diff. It is also the one that is hard to get right and easy to get *silently*
wrong. Peer-to-peer OT with arbitrary reordering needs TP2 (the transformation
property that says transforming along two different paths agrees), and the
classic insert/delete transformation functions with a site-id tie-break
satisfy TP1 but **not** TP2 — that is a published result, not a matter of
care. A merge that looks convergent and corrupts text under a three-way
concurrency pattern is the exact failure mode this work exists to prevent.
Implementing full COT/context-vector OT here would be a larger and riskier
change than the alternative.

**Chosen: a sequence CRDT materialised at merge time, with identities derived
rather than stored.** Inside `merge_operations`, each text run touched by
character operations is expanded into a vector of atoms — one per Unicode
scalar value — where

- base characters carry the identity "present in the merge base";
- a character inserted by an operation carries that operation's identity;
- a delete is a tombstone set on the atoms it covered, never a removal.

An operation's `offset` is resolved against the **subsequence visible in that
operation's own causal context** — the atoms whose inserting operation it
observed, minus the atoms whose deletion it observed. That is what the offset
meant when it was generated, so no transformation is needed: the offset is
converted once into an identity (the atom it is anchored after), and identities
do not shift. New atoms are placed with the RGA rule (skip forward over atoms
inserted by unobserved operations that sort after this one), which makes the
final linearisation a function of the atom set alone.

Then the atom vector is collapsed back to a `String` and written into
`Inline::Text`/`Inline::Link`. **The tombstones and identities do not survive
the merge.** They are reconstructed from (base text, operation set) on every
merge, deterministically.

## The tradeoff we are accepting

**What it buys.** The stored model does not change. `Document` is byte-for-byte
the same shape, still `Eq`, still canonical CBOR, still what gets signed.
`opendoc-render`, `opendoc-import`, `opendoc-store` and the exporters are
untouched. Convergence is structural — the merged text is a function of the
operation set, so *any* receive order, *any* stream grouping and *any*
duplication produce identical bytes — rather than something we hope a
transformation matrix preserves.

**What it costs, and this is real.** Because tombstones are not persisted, the
identities are only as good as the merge base. Two concurrent edits are
resolved correctly only while both are expressed relative to a base that the
merge still has. Once a merge result is written back as plain text and becomes
the new base, a *later*-arriving operation that predates it cannot be placed by
identity any more — its offsets are re-anchored against the collapsed string
and it degrades to positional (clamped) application. In practice that is the
"a branch shows up after its merge base was already compacted away" case. A
persisted CRDT would not have that limit; it would instead put per-character
identities and tombstones into the signed document, growing it without bound
and making the signed bytes a function of editing history rather than of
content. We chose the bounded loss over the unbounded model change.

The second cost is that placement is only as good as the causal context the
caller supplies. `context: None` means "concurrent with everyone else", so a
run of operations made by different actors *in sequence* on one branch is
treated as concurrent. That does not break convergence — it is still a pure
function of the set — but it can place text where a strict reading of intent
would not. `opendoc-app` now supplies a real context for locally generated
operations; anything replayed from an older journal keeps `None`.

Third: merge cost for a text run is O(atoms x edits) in the naive visibility
scan. Runs are short (a run is split by every mark change) and the edit set is
a delta, not a history, so this is not the hot path. If it ever is, the
visibility scan is the thing to index.

## Consequences

- `merge_operations` applies character operations in a second phase, after the
  main operation pass and after suggestion resolution, so "the run was deleted
  by a concurrently accepted suggestion" is decided against the near-final
  document. Its `missing-inline` / `non-editable-inline` warnings are still one
  per operation, but they are now emitted grouped by run rather than
  interleaved with other operations' warnings.
- A whole-run write (`UpdateInlineText`, or the `InsertInline` that created the
  run) resets the run's base: character operations *ordered before it* are
  discarded, which is the same last-write-wins semantics the sequential path
  had. Character operations concurrent with it but ordered after it are
  re-anchored against the new text and clamped.
- `ActorId`, `OperationId`, `VectorClock`, `CausalContext` and the ordering
  live in `crates/opendoc-merge/src/causal.rs`; the sequence CRDT lives in
  `crates/opendoc-merge/src/text_sequence.rs`. This is the start of the split
  PLAN77 G5 asks for; `lib.rs` keeps the operation kinds and the apply pass.
- `opendoc-core` is **not** changed by this ADR.
- Convergence is now asserted at the byte level: the property test encodes
  merged documents with `opendoc_format::encode_canonical_cbor` and compares
  bytes, not just `Eq`.

## Validation

Per ADR 0003, both realistic scenarios and fuzzing. Everything lives in
`crates/opendoc-merge/src/causal_convergence_tests.rs` (27 tests).

- **Convergence fuzz** — `randomised_concurrent_operation_sets_converge_byte_identically`.
  2,000 seeds. Each seed simulates four replicas that sync with each other at
  random, so some operations are causally ordered and some are genuinely
  concurrent; it generates 14 mixed character and property operations, redelivers
  a fifth of them as duplicates, then merges the same multiset under 12 random
  stream partitions and shuffles and asserts byte-identical canonical CBOR.
  24,000 merges per run. It also asserts that at least 90% of seeds actually
  changed the document, so it cannot pass vacuously.
- **Insert oracle fuzz** — `randomised_concurrent_inserts_survive_whole_and_contiguous`.
  2,000 seeds. With only inserts in play the answer is checkable without the
  merge: every character typed must survive, each actor's insert must appear as
  one unbroken run, and stripping the inserts must give back the base.
- **Delete oracle fuzz** — `randomised_concurrent_deletes_remove_exactly_the_union_of_their_ranges`.
  2,000 seeds. Concurrent deletes against a shared base must leave exactly the
  base minus the union of their ranges, computed independently of the merge.
- **Targeted transformation cases**, each asserting an exact string rather than
  a `contains`: insert/insert at different offsets, insert/insert at one anchor,
  an insert never split by a concurrent insert, insert/delete before, insert/delete
  after, an insert inside a concurrently deleted range, disjoint deletes,
  overlapping deletes, nested deletes, the identical range deleted twice,
  deletes that between them empty a run, and the unicode scalar-value case.
- **Ordering cases**: a causally later property write beating an actor id that
  sorts later, genuinely concurrent writes still tie-breaking on actor id,
  `causal_order` reproducing `(actor, seq)` with no contexts supplied, an
  observed operation preceding its observer, and a forged cyclic vector clock
  still yielding every operation exactly once.
- **Regression guards**: an actor's own sequential edits still compose as
  typing, a whole-run rewrite still beats the character edits ordered before it,
  and character edits on a missing or derived run still warn once per operation.

### Falsification

Every test above was run against the pre-ADR behaviour — `(actor, seq)`
ordering restored and character operations applied positionally — with the new
test file unchanged. **13 of the 27 fail there**, including all three fuzz
oracles except the pure convergence one:

```
a_causally_later_property_write_wins_regardless_of_actor_id     FAILED  Center != End
a_concurrent_delete_nested_inside_another_changes_nothing       FAILED  "a" != "ag"
a_concurrent_insert_is_never_split_by_another_insert            FAILED  "abcAABAdefg" != "abcAAAdeBfg"
an_insert_after_a_concurrent_delete_keeps_its_own_anchor        FAILED  "34567*89" != "34*56789"
an_insert_inside_a_concurrently_deleted_range_survives...       FAILED  "aefX" != "aXef"
concurrent_character_edits_count_scalar_values_not_bytes        FAILED
concurrent_inserts_at_different_offsets_do_not_shift_each_other FAILED
disjoint_concurrent_deletes_remove_exactly_their_own_ranges     FAILED
overlapping_concurrent_deletes_remove_the_union_exactly_once    FAILED  "afg" != "ah"
the_same_range_deleted_by_two_actors_is_removed_once            FAILED  "ab" != "abfg"
three_concurrent_actors_editing_one_run_converge_on_every_...   FAILED
randomised_concurrent_inserts_survive_whole_and_contiguous      FAILED
randomised_concurrent_deletes_remove_exactly_the_union...       FAILED
```

`randomised_concurrent_operation_sets_converge_byte_identically` **passes**
against the old behaviour, and that is the point worth recording: the old merge
*did* converge. Convergence alone is not evidence of correctness here, which is
why the two oracle fuzzes exist alongside it. The remaining tests that pass
against the old behaviour are the regression guards and the direct unit tests
of the new `causal_order` / `resolve_run` functions, which have no old
behaviour to differ from.

## What is not done

- `opendoc-app` supplies a causal context for operations it generates locally,
  derived by scanning `operation_envelopes` — O(operations) per operation. It
  is a few microseconds on a realistic journal, but the right home for the
  clock is a field in `AppState` next to `next_seq`, maintained incrementally.
  That file belongs to the app layer and was left alone.
- Operations replayed from a journal written before this ADR carry
  `context: None` and are read as concurrent with other actors. That is the
  conservative reading, not a wrong one, but it is weaker than the causal
  information those operations actually had.
- The spreadsheet merge (`spreadsheet_replay.rs`) still orders by
  `(actor, seq, kind)` on its own envelopes and was not touched. Its operations
  are cell-addressed rather than offset-addressed, so they do not have the
  corruption problem this ADR fixes, but they do have the same weak
  "last writer" definition ADR 0006 described.
