# ADR 0017: Undo Is an Inverse Operation, Not a Rewind

Status: accepted. Completes the item ADR 0015 named as future work ("undo that
survives collaboration has to be expressed as new inverse operations submitted
like any other edit") and removes the guard `apply_remote_operations` carried in
the meantime. Depends on ADR 0007 for what a text offset means.

## Context

Undo was a **whole-state snapshot stack**. `dispatch.rs` pushed a checkpoint —
document, workbook, blobs, blob bytes, signatures, the whole operation journal —
before every undoable command, coalescing gestures inside a one-second window,
keeping up to `UNDO_STACK_LIMIT = 200`; undo popped one and restored it.

That is correct for one actor and wrong the moment there are two, in two
different ways.

1. **Restoring a snapshot deletes a collaborator's work.** A checkpoint taken
   before a remote edit arrived does not contain that edit. Restoring it removes
   the edit locally while the service still holds it, so the replica stops
   agreeing with the server and the collaborator's keystroke is gone from one
   screen and present on another. Removing the *local* operation from the set
   instead has the same problem from the other end: the server's log still
   contains it.

   `apply_remote_operations` therefore **dropped the undo and redo stacks**
   whenever it ingested anything new. Honest, and a second bug: a user lost the
   right to undo their own typing because somebody else typed.

2. **Undo re-minted operation ids.** Restoring a checkpoint restored
   `next_operation_seq` with it, so inside a live session the counter could roll
   back below the sequence the service had already acknowledged. Re-submitting an
   acknowledged `OperationId` is history rewriting and the service refuses it
   (ADR 0015, "History immutability"). `local_operations_are_dense_after` could
   report the hazard; nothing could avoid it.

Both failures have the same root: an undo was being expressed as a *statement
about the past* in a system where the past is a shared, append-only,
server-attested log.

## Decision

**An undo is an ordinary edit that says the opposite of an earlier one.**

For a step made of this actor's own document operations, `undo_current_edit`
computes the inverse of each operation in the step, newest first, and applies
them through the same `apply_batch` path a keystroke uses. They get fresh
`OperationId`s from `next_operation_seq`, land in the journal as ordinary
envelopes, ride the same causal-context machinery, converge through the same
`merge_operations`, and go to the service as an ordinary submit.

Three properties follow, and each of them is the answer to something the
snapshot stack could not express:

- **It cannot delete a collaborator's work,** because every inverse operation
  names only its author's own contribution — the characters this actor inserted,
  the block this actor removed, the value this actor overwrote. There is no
  operation in an undo that mentions anything else.
- **The service accepts it,** because it is new history. Nothing rewinds, so
  nothing re-uses an acknowledged id.
- **It is per-actor.** The undo stack holds this replica's own steps, and Ctrl+Z
  reverses this actor's last edit rather than whatever happened most recently.
  That is the property snapshot undo structurally could not have: a snapshot is
  a statement about the whole document, so restoring one is always global.

`redo` is the same function with the stacks swapped: the inverse of an inverse,
which is ordinary history again. There is no stored future to replay.

### Inverses are captured when the operation is written

`invert_operation(before, kind)` in `crates/opendoc-merge/src/inverse.rs` is
called from `DocumentOperationService::apply` *before* the merge, against the
document the operation was written against, and the answer is kept in
`OpenDocApp::operation_inverses` keyed by `OperationId`.

That timing is forced, not chosen. A delete's inverse has to carry the content
that was removed and a set's inverse has to carry the value that was
overwritten, and neither exists afterwards. Reconstructing it at undo time would
mean re-merging a prefix of the log per undo.

Inside a batch each operation is inverted against the document as it stood
immediately before *it*, not before the batch, which needs the intermediate
states; the capture folds the batch into a scratch document one operation at a
time while the batch itself is still merged in one pass. "Delete this text, then
delete the block it was in" is the case that forces it: the second operation
must capture the block *without* the text, or undoing the step would restore the
text twice. Those two halves are in tension, and the amendment of 2026-09-14
below says how it is resolved: the fold is a *model* of the batch merge, so
where the merge's answer depends on the whole set the fold is told the whole
set's answer.

The match in `invert_operation` is **exhaustive with no catch-all**, so an
operation added to the vocabulary later does not compile until somebody decides
whether it can be undone.

### Character operations are the exception, and they are inverted at undo time

`InsertText` and `DeleteText` are the only operations in the vocabulary that
address their target by an offset rather than by identity — ADR 0007 §Constraints
is explicit that "the offset problem is confined to the inside of a single text
run". An offset means something different once a concurrent edit has landed in
the same run, so a captured inverse would be wrong exactly when it matters.

`invert_operation` therefore answers `Inversion::Deferred` for those two, and
`invert_text_operations(base, operations, targets)` computes the real inverse at
undo time. It re-derives the run's per-character identities from (base, operation
set) *exactly as the merge does* — `collect_text_run_edits` is now one function
shared by the merge and by this, so the two cannot disagree about which
operations are offset-addressed or about which whole-run write resets a run — and
then asks the atom vector two questions:

- **Undoing an insert:** which characters did this operation insert that are
  still visible, and where are they now? Grouped into visible ranges, so a
  collaborator who typed *inside* the inserted text keeps it, in place, and the
  undo goes out as two deletes around it rather than one delete across it.
- **Undoing a delete:** which characters did this operation tombstone that
  nobody else also tombstoned, and what is the anchor they go back at? A
  character two actors both deleted stays deleted: the other actor still wants
  it gone.

`targets` is a set rather than one operation because a single step routinely
holds several edits to one run — a replace-all is a delete and an insert per
occurrence — and their inverses share one coordinate space. Taken one at a time
each would be correct against the run before the undo and wrong against the run
the previous inverse had just changed. The emitted order is load-bearing: every
delete first in descending position, then every insert in descending position,
with the insert offsets counted in the sequence the deletes leave behind.

A run's character inverses are emitted as **one block, at the point the run's
latest character operation appears in the newest-first walk** over the step. That
is what makes a mixed step come out in the right order: "character-edit a run,
then delete the block around it" inverts to *put the block back, then put the
characters back*, while "create a run, then type into it" inverts to *unpick the
typing, then remove the run*.

### One merge change: an insert that carries a run resets that run

ADR 0007 already says a whole-run write resets the run's base, and named
`UpdateInlineText` and the `InsertInline` that created the run. `InsertBlock`,
`InsertTableRow` and `InsertTableCell` also write whole runs — their payload
carries the blocks — and they now reset those runs too.

Undo is what makes this load-bearing. Restoring a deleted block re-inserts a
snapshot of its runs' text; without the reset, the character operations that
shaped that text *before* the delete would be replayed against the restored copy
and delete it a second time. All 186 pre-existing `opendoc-merge` tests pass
unchanged, because for an insert that creates fresh runs there are no character
operations ordered before it.

### The snapshot stack stays, as a fallback, and is refused inside a session

A step that also moved state no typed document operation describes — a
spreadsheet edit, a blob upload, a journal marker — or that contains an
operation the vocabulary cannot invert, is still undone by restoring the
whole-state checkpoint. `inverse_of_step` returns `None` for it, and every reason
is a *structural* property of the step, checked rather than assumed.

Outside a session that keeps single-user undo covering exactly what it covered
before. **Inside a session it is refused** with a conflict that says why, and
refusing does not consume the step. There is no version of "restore a snapshot
while a service holds work you have not got in it" that is not data loss, so the
answer is an error the user can be told about rather than a silent rollback.

## Which operations need to capture state, and which cannot be inverted

Needing captured state is not a defect; it is what "inverse" means for an
operation that destroys information. These read `before`:

| Needs to capture | What it captures |
| --- | --- |
| `DeleteBlock` | the whole block, and the identity of its previous sibling |
| `DeleteInline`, `MoveInlineToBlock` | the inline, its owning block, its previous sibling |
| `DeleteTableRow`, `DeleteTableCell`, `DeleteTableColumn` | the row/cell/column and its `InsertPosition` |
| `DeleteBibliographyReference`, `DeleteCitationGroup` | the record, and a revision above the delete's |
| `SetDocumentTitle` / `Doi` / `Locale`, `SetPageSetup`, `SetPageFurniture`, `UpdateCitationStyle` | the previous value |
| `SetBlockTextStyle`, `UpdateHeadingLevel`, `UpdateListItem` | the previous style |
| `SetBlockProperty`, `SetTableCellProperty` | the previous value, **or a clear** when the property was inheriting — those are different states and only one keeps following the document's style |
| `ClearBlockProperty`, `ClearTableCellProperty` | the value that was cleared |
| `UpdateInlineText`, `UpdateLinkHref`, `UpdateMentionLabel`, `Update*EquationSource`, `UpdateImage*`, `UpdateCommentBody`, `UpdateSuggestionInsertContent` | the previous value |
| `AddMark`, `RemoveMark`, `AddMarkRange` | the marks that were actually there, so an add that was a no-op inverts to nothing rather than removing a mark somebody else's edit had already put on |
| `SetTableColumnWidth`, `SetTableCellSpan` | the previous width/span |
| `UpsertFootnote`, `UpsertBibliographyReference`, `UpsertCitationGroup` | the previous record, or a tombstone when the upsert created it |

The rest invert from their own payload: `InsertBlock` → `DeleteBlock`,
`InsertInline` → `DeleteInline`, `AddCommentThread` → `DeleteCommentThread`,
`AddCommentReply` → `DeleteComment`, `Delete`/`Restore` of a comment or thread
into each other, the table inserts into their deletes.

**Five cases genuinely cannot be inverted.** Each is a named refusal, not a
silent no-op, and each falls back to the snapshot rather than doing something
approximately right:

| Reason | Why |
| --- | --- |
| `nested-block-delete` | `insert_block` only ever inserts into `document.blocks`, so a block deleted from inside a table cell cannot be put back where it was. |
| `table-column-cells-are-derived` | `InsertTableColumn` deliberately carries no cells — they are derived from the row and column ids so a concurrently inserted row gets one too (ADR 0013). Derived cells are empty, so a column that held content cannot be restored by re-inserting it. A column whose cells were all empty *is* invertible. |
| `no-operation-withdraws-a-suggestion` | the vocabulary has accept and reject, not withdraw, and both *resolve* a suggestion rather than un-proposing it. |
| `suggestion-resolution-is-terminal` | the merge records which actors resolved a suggestion so that concurrent resolutions converge; there is no operation that returns a resolved suggestion to review. |
| `mark-differs-only-in-expansion` | `RemoveMark` matches on kind and value, so it would take a same-kind, same-value mark with a different `MarkExpand` with it. |

### Amendment 2026-09-12: the four "cannot say first" refusals are gone

This ADR originally listed nine refusals. Four of them —
`insert-block-cannot-say-first`, `insert-inline-cannot-say-first`,
`move-inline-cannot-say-first` and `insert-table-cell-cannot-say-first` — were
one gap with one fix, and it has been made: `InsertBlock`, `InsertInline`,
`MoveInlineToBlock` and `InsertTableCell` now carry the
`opendoc_core::InsertPosition` the table row and column operations already had
(ADR 0013 introduced it for exactly this reason). `after: Option<StableId>` is
gone from all four payloads, the four `Inversion::Irreversible` arms are deleted
rather than merely unreachable, and deleting the first block, inline, moved
inline or table cell now inverts to putting it back **first**.

`InsertPosition::First` is safe to mint for an undo precisely because it names
no anchor: the `After` arm degrades to an append when its anchor has been
deleted, and that degradation is what `after: None` could not be distinguished
from. `None`-means-append survives unchanged wherever it is still used, bridged
in one place by `InsertPosition::after_or_last`, so no call site silently
acquired a different meaning by being ported.

The command surface was **not** widened: `insert_table_cell`'s `afterCell` and
the block/inline anchors still read an id or nothing, because their meaning is
documented in `opendoc-api`'s command metadata. `InsertPosition::FIRST_KEYWORD`
already reaches the table row and column commands, and extending it to the other
four is an `opendoc-api` change.

Convergence on the new positions is guarded by
`crates/opendoc-merge/src/insert_position_fuzz_tests.rs`: 2,000 seeds per
structural insert, each seed checked for byte-identical convergence across two
independent stream permutations **and** against three positional oracles —
`First` precedes every surviving base sibling, `Last` follows every surviving
base sibling, and `After(base anchor)` follows that anchor. Convergence alone
would not have caught it; an implementation that treated `First` as `Last` would
converge on every seed.

### Amendment 2026-09-14: the fold applies the batch's own discards

ADR 0007's whole-run reset is a property of the operation **set**: a character
operation loses to a later operation that writes the whole of its run, so the
merge that lands the batch never applies it and the document never holds the
characters it names. A fold that merges one operation at a time cannot see
that. Each operation is alone in its own merge, where there is no later write
for it to lose to — so the fold applied an operation the batch discards, and
the *next* operation's inverse was captured against a document holding
characters the real document never had.

```
InsertText       { inline_id: X, offset: 14, text: "<51>" }
UpdateInlineText { inline_id: X, text: "rewritten 374" }
```

The run was `block 3 with words`. The batch lands `rewritten 374`. The fold
landed `block 3 with w<51>ords`, so the rewrite's captured inverse was
`UpdateInlineText { X, "block 3 with w<51>ords" }`. At undo the character
operation inverted to `Nothing` — correctly, by "a whole-run rewrite ordered
after the operation discarded it" above — so nothing took those four characters
back out, and `Ctrl+Z` produced text nobody had typed at a position nobody had
typed it. **An undo that adds text.**

**The rule: the fold's states must be states the batch really passes through.**
The fold is a model of one merge, and a model that ends somewhere the thing it
models does not is wrong from the first divergence on. The last state a fold
reaches has to be the document the batch lands. So the fold now **skips** every
operation the batch merge discards. Skipping is not a capture failure —
`Irreversible` stays reserved for a fold that stopped tracking the batch —
because the merge discards the operation too: the state after skipping it *is*
the state the merge produces.

The alternatives were considered and are wrong:

- *Merge the prefix `0..k` as a batch rather than folding incrementally.* The
  prefix `[InsertText]` does not contain the rewrite, so it lands the same
  wrong state. Every definition of "the state before operation k" that reads
  only operations `0..k` has this hole, because the operation that decides the
  answer comes after k.
- *Un-discard the character operation at undo time.* The document genuinely
  does not hold those characters. An inverse that deleted them would be an edit
  against text that is not there. `invert_text_operations` was already right;
  it is the capture that lied.
- *Defer a whole-run write's inverse the way character operations are
  deferred.* `UpdateInlineText` is not offset-addressed, and its previous value
  is exact the moment the state it is taken against is. Deferring it would
  trade a wrong capture for a re-derivation answering the same question.

`discarded_by_a_later_whole_run_write` (`crates/opendoc-merge/src/inverse.rs`)
answers the question, out of `runs_written_wholesale` — the same per-operation
answer `collect_text_run_edits` already gives the merge and gives
`invert_text_operations`. Three callers, one statement of "which writes reset a
run", so none of the three can drift from the other two.

The defect predates the cheap fold. The copying fold that this ADR's own
validation uses as its oracle merges one operation at a time as well, and
reproduces the failure exactly; the oracle now skips the discards too, deciding
which those are from its own reading of ADR 0007 rather than from the crate
under test. `crates/opendoc-app/src/batch_fold_tests.rs` named and **excluded**
this shape when it was found. The exclusion is gone, and the generator now
produces the shape deliberately — one batch in four — instead of waiting for
two of thirteen draws to land on the same one of eight runs, which happened
three times in six hundred seeds. Both generated tests assert that it is still
being produced, because a generator that quietly stopped would make the fix
look proved while proving nothing.

Falsification for this amendment. Every mutation was applied to a private copy
of the tree, `cargo test --release -p opendoc-app -p opendoc-merge -p
opendoc-core` was run, and the mutation was reverted byte-identically:

| Mutation | What failed |
| --- | --- |
| the fold ignores the discards again (`!discarded[offset]` deleted) | `a_character_edit_the_batch_discards_is_not_in_the_state_the_next_inverse_captures` ("the rewrite's inverse restores the run the document really had, not the one only the fold ever saw"), `undoing_a_generated_batch_gives_the_document_back`, and the copying-fold comparison |
| `discarded_by_a_later_whole_run_write` compares `rank > reset` instead of `rank < reset` | the two above plus `the_operations_a_batch_discards_are_the_character_edits_a_later_whole_run_write_covers` |
| every character operation is discarded, whatever writes the run | the same three |
| `runs_written_wholesale` stops treating `InsertBlock` as a whole-run write | `a_delete_and_a_reinsert_of_its_block_do_not_replay_the_text_edits_twice` and `undoing_a_generated_batch_gives_the_document_back` |
| `runs_written_wholesale` stops treating `UpdateInlineText` as a whole-run write | `a_whole_run_rewrite_beats_the_character_edits_ordered_before_it`, `a_character_edit_a_whole_run_write_discarded_inverts_to_nothing`, and all four batch-fold tests |
| the copying-fold oracle stops skipping the discards | the fold comparison, naming the operation whose inverse moved |
| the generator stops producing the shape | both generated tests, on the coverage floor rather than on a wrong document |


## What happens when an inverted operation has dependents

This is the case the merge machinery has to resolve, and it does.

**A colleague typed inside text this actor inserted.** A inserts `hello`, B types
`XX` into the middle of it, A undoes. A's inverse is computed from the atom
vector, so it names only A's characters — which are now in two visible ranges —
and B's `XX` survives, in place, in the hole A's undo leaves. The undo goes out
as two `DeleteText`s, not one. Proved in process
(`undoing_an_insert_a_colleague_edited_inside_keeps_the_colleagues_text`) and
over a real socket
(`undoing_an_insert_a_collaborator_typed_inside_keeps_the_collaborators_text`).

**A colleague deleted characters this actor also deleted.** Undoing the local
delete restores only the characters the collaborator did not also remove. An undo
withdraws its own author's contribution; it does not overrule anyone.

**A colleague's delete removed text this actor inserted.** The atoms are already
invisible, so the inverse names nothing for them: there is nothing left to
remove. The undo is smaller than the operation it inverts, which is correct.

**A whole-run rewrite ordered after the operation discarded it.** ADR 0007 says
character operations ordered before a whole-run write lose to it. Such an
operation inverts to `Nothing` rather than to an edit against text it never saw.

**A colleague edited a run inside a block this actor deleted, after the delete.**
Here the honest answer is a loss, and it is stated rather than hidden: the
restore re-inserts the snapshot of the block that this replica held, and because
`InsertBlock` resets its runs, character operations ordered before the restore —
including the collaborator's — are not replayed onto it. Restoring deleted
content by carrying it as text cannot preserve edits made to it while it was
deleted. A persisted CRDT could; ADR 0007 already declined to persist one, for
reasons that have not changed.

## What this costs

**The snapshot stack does not go away, and neither does its cost.** It is now a
fallback rather than the mechanism, but `dispatch.rs` still takes a full
checkpoint before every undoable command, because the decision about which path a
step needs can only be made after the step has run. Removing the remaining cost
means making the checkpoint conditional in the dispatcher, which is the one
change this work did not make.

What *was* removed is the memory cliff. `checkpoint()` used to deep-copy
`blob_bytes` — every embedded image's bytes — per non-coalesced undoable command,
keeping up to 200 copies. `blob_bytes` is keyed by the SHA-256 of its values, so
two maps with the same key set hold the same bytes; a checkpoint now *shares* the
previous one's copy whenever no blob was added or removed, and the comparison is
O(blobs) rather than O(bytes). A document with pictures in it no longer pays for
copying them on every keystroke. `checkpoints_share_one_copy_of_the_blob_bytes`
asserts pointer identity, not equality — equality would hold just as well if
every checkpoint took its own copy, which is the thing being fixed.

**The inverse map grows with the journal.** One `Inversion` per document
operation this replica minted, which for a delete holds the deleted content. It
is bounded by the operation log, which is already unbounded for the same
document, and it is far smaller than 200 whole-document snapshots.

It used not to be cleared by `close_document` at all — the operation ids of a new
document restart at 1 and overwrite their entries one at a time, which made the
residue invisible rather than absent. `DocumentOperationService` now keeps the
map's invariant where the map is written: an entry for another actor, or at or
above the sequence number being minted, cannot belong to the journal this
service is writing into, and is discarded. In ordinary editing that is a no-op;
after `close_document` or `join_collaboration_session` the first edit in the new
document discards all of it, so the residue cannot accumulate across documents.
Clearing the map outright in `clear_edit_history`, alongside the journal and the
undo stacks, is still the right complement and is not done.

**An undo makes the document longer, not shorter.** The journal grows by the
inverse; a save writes both. That is the price of undo being history, and it is
the same price the service already pays for re-merging from genesis.

**A signature does not survive an undo.** An edit clears `signatures`
(`invalidate_source_state`) and the old snapshot undo put them back. Inverse undo
does not: the operation log has grown, so the signed source state genuinely is
not the state that was signed. This is a deliberate behaviour change and the one
place single-user behaviour differs.

**One merge pass per operation in a batch**, for the intermediate states the
capture needs. Batches are gestures — a keystroke, a burst delete, a replace-all
— so this is bounded by the gesture, not by the document's history.

## Consequences

- `apply_remote_operations` **no longer touches the undo or redo stacks**, and
  `AppRemoteIntake::dropped_undo_history` is deleted rather than left reporting
  something that cannot happen. It does still close the coalescing window: the
  next keystroke was typed after seeing somebody else's edit, so it is a new
  gesture.
- `AppUndoCheckpoint` gains `step_end`, recorded by the *next* `checkpoint()` —
  the moment another step begins is the moment this one is over. Without an
  explicit boundary an undo of an older step reaches over the newer ones and over
  the operations a previous undo appended, which is a real bug and is what
  `steps_unwind_and_rewind_one_at_a_time` guards.
- `undo_current_edit` and `redo_current_edit` are one function with the stacks
  swapped, because they are one mechanism.
- `Atom` and `resolve_run_atoms` are now visible inside `opendoc-merge`:
  ADR 0007's character identities are derived and discarded per merge, and an
  undo needs to ask them a question before they are discarded.
- **The WASM transport's undo test changed meaning, and has been rewritten.**
  `crates/opendoc-wasm/src/collab_tests.rs` used to assert that after an undo
  *nothing* went on the wire and the user was shown a `history-rewritten`
  notice, because an undo rewound the operation counter below the acknowledged
  watermark. That is no longer true, and the test is now
  `an_undo_inside_a_live_session_is_submitted_as_new_work`: it asserts the undo
  goes out as one more submit whose every operation id is above everything the
  service holds. The notice itself is still raised by `collab.rs` and is still
  correct for a history loaded from a repository written before the sequence
  counters were split (ADR 0015, `legacy-operation-sequence-gap`).

## Validation

`crates/opendoc-merge/src/inverse_tests.rs` (18 tests) covers the vocabulary and
the character re-resolution; `crates/opendoc-app/src/state.rs`
(`edit_history_tests`) and `crates/opendoc-app/src/collaboration.rs` cover the
app-level behaviour; `crates/opendoc-service/src/app_client_tests.rs` covers it
over a real socket against a real service and a second real client.

The two that matter:

- `an_apps_undo_reverses_only_its_own_work_and_the_service_accepts_it` — A (a
  real `OpenDocApp`) types and waits for the service to acknowledge it, B edits
  the same run, A undoes. B's word survives exactly where it was, every id the
  undo minted is above the acknowledged watermark, the `Accepted` frame is
  asserted rather than inferred, the sync preflight agrees with the service, and
  the app, both transport sessions and the server's own materialised document
  encode to identical canonical CBOR. Then a redo, and the same checks again.
- `undoing_an_insert_a_collaborator_typed_inside_keeps_the_collaborators_text` —
  the dependent-operation case, over the socket, asserting that the undo is *two*
  deletes and that the collaborator's character is still in the middle.

Falsification, per ADR 0003. Every mutation below was applied, the suites were
run, and the mutation was reverted:

| Mutation | What failed |
| --- | --- |
| `invert_text_operations` emits the operation's own recorded offsets instead of resolving against the atoms | 6 of `inverse_tests`, `replace_all_is_a_single_undo_step`, and **both** socket undo tests |
| restoring a delete requires only that *some* deleter is being undone, rather than all of them | `undoing_a_delete_does_not_resurrect_what_another_actor_also_deleted` |
| a run's character inverses are computed one operation at a time instead of as a block | `replace_all_is_a_single_undo_step` and `undoing_an_insert_a_collaborator_typed_inside_keeps_the_collaborators_text` |
| `collect_text_run_edits` stops treating `InsertBlock` as a whole-run write | `a_delete_and_a_reinsert_of_its_block_do_not_replay_the_text_edits_twice` |
| `checkpoint()` stops recording `step_end` | `steps_unwind_and_rewind_one_at_a_time` |
| `inverse_of_step` treats `Irreversible` as `Nothing` | `an_operation_the_vocabulary_cannot_invert_is_refused_inside_a_session` |
| `checkpoint()` always takes a fresh copy of the blob bytes | `checkpoints_share_one_copy_of_the_blob_bytes` |
| `apply_remote_operations` clears the undo stack again | `a_local_step_stays_undoable_across_remote_work_and_the_remote_work_survives` and `undo_is_per_actor_not_global` |

Two of those mutations survived the tests as first written, and the tests were
changed rather than the finding waved away:

- **The step boundary.** `steps_unwind_and_rewind_one_at_a_time` originally
  asserted only the text after each undo and redo, and an unbounded step window
  lands on the *right* text: undoing an older step re-applies and re-reverses
  every newer step on the way past, which cancels out. The test now asserts that
  each undo authors exactly one operation, which is the only thing that tells
  "reversed one step" from "reversed three and re-applied two".
- **The irreversible arm.** The only refusal test used a spreadsheet step, which
  `inverse_of_step` rejects on the envelope classification before it ever reads
  an `Inversion`. `an_operation_the_vocabulary_cannot_invert_is_refused_inside_a_session`
  was added to cover the arm that actually reads it: deleting the first of
  several blocks.
