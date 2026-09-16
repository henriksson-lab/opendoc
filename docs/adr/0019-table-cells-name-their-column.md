# ADR 0019: A Table Cell Names The Column It Was Written Into

Status: accepted for v0. Amends Decision 1 of
`docs/adr/0013-table-structure-and-merge.md`; applies the argument of
`docs/adr/0007-causal-ordering-and-text-convergence.md` one level down.

## Context

ADR 0013 made a table a rectangle and gave rows and columns identities,
because "an index means different things on two replicas, an identity does
not". It then declined to apply that argument to cells:

> **A cell carries no `column_id`.** On a rectangle the cell's index *is* its
> column, and a stored column id would be a second representation of the same
> fact.

That is true of a cell *in a document*, and it stayed true. It is not true of
a cell **in an operation**. `InsertTableRow` carries a whole `TableRow`, built
by `OpenDocApp::add_table_row` as one cell per column *as the authoring
replica sees them*. By the time the merge applies it, the column list may be a
different list: another replica may have deleted a column before them, or
inserted one between them. The payload's cells were then placed by position
against a grid that had moved, and the two failures below both followed.

### Consequence A — silent content loss, agreed on by every replica

Base 3x3; Alice appends two rows; Bob concurrently deletes `col-0` and
`col-1`. The causal order can interleave the four operations, so Alice's first
row was placed against a two-column grid and her second against a one-column
grid. Each row lost a *different* cell. The merge converged — every replica
produced the same bytes — and the only report was a generic
`table-geometry-repaired` warning. This is exactly the failure ADR 0007 was
written about: a merge that converges on a document neither author wrote.

### Consequence B — a deterministic merge failure

`repair_table_geometry` grew the column list to the width of the widest row
with `TableColumn::filling(table_block_id, index)`, an identity derived from a
**positional index**. So: insert a row with four cells into a 3x3 table (a
column is synthesised at index 3), delete `col-0` (the grid shifts), insert
another four-cell row — and the repair mints the same derived id again. The
grid then holds two columns with one id, and two cells with one derived id, so
`Document::validate()` rejects it and `merge_operations` returns
`Err(InvalidDocument("duplicate table cell id"))`. One actor, three
operations, no concurrency at all. A randomised 3-actor 12-operation table
script hit it on **145 of 6,000 seeds**.

Because `DocumentOperationService::apply` swallowed merge failures at the time
(fixed in the same change), the user saw neither of these.

## Decision 1 — the binding lives in the operation, not in the document

`OperationKind::InsertTableRow` gains

```rust
cell_columns: BTreeMap<StableId /* cell */, StableId /* column */>
```

— a map, not a parallel list, so it is keyed on the cell's own identity and
cannot drift out of alignment with `row.cells` the way two lists could.
`opendoc-app` validates that it names only cells the row carries, names no
column twice, and either covers every cell or is empty.

The **stored model is unchanged.** `TableCell` gains no field, `Document` is
byte-for-byte the same shape, still `Eq`, still canonical CBOR, still what
gets signed, and `opendoc-render`, `opendoc-import`, `opendoc-layout`,
`opendoc-pdf` and the exporters are untouched. This is the same trade ADR 0007
made for text: the identities a merge needs are **derived at merge time from
the operation set**, not persisted into the signed document.

ADR 0013's objection to a stored `column_id` therefore stands as written, and
this ADR does not overturn it. What it corrects is the unstated step: the
rectangular invariant makes the index sufficient *for a cell that is already
in the grid*, and says nothing about a cell that is on its way in.

`merge` places the payload's cells by identity:

- for each live column, the payload cell bound to it, else
  `TableCell::filling(row_id, column_id)` — the derived empty cell a column
  another actor added concurrently gets;
- a payload cell whose column is **not** in the grid is dropped, and the merge
  says so with a `table-row-cell-column-deleted` warning.

Dropping is what makes the two orders agree. "Insert the row, then delete the
column" removes that cell; so "delete the column, then insert the row" has to
remove it too. The alternative — resurrecting the column the cell names —
would make the result depend on the order, which is the bug.

The result always has exactly one cell per live column, so the row that enters
the grid is rectangular by construction. That is what makes the
index-addressed operations downstream (a column delete removes
`row.cells[index]` from every row) sound rather than lucky.

## Decision 2 — an invented column is named after the cell that needs it

`TableColumn::filling` no longer takes an index. It is now the one column a
table with **no columns at all** is given, derived from the table block alone
and mintable at most once per table, because it is only reachable while the
column list is empty.

Every other column the repair has to invent is
`TableColumn::for_cell(cell_id)`, derived from the cell that has no column. A
cell id is unique in a document, so the derived column id is unique too — the
property an index never had. It is the same rule `InsertTableCell` already
used to name the column that "add a cell to one row" creates, so a column a
repair invents and a column that operation creates cannot disagree about a
cell.

## What an older repository does

`cell_columns` is `#[serde(default)]` and `skip_serializing_if` empty, so an
operation journalled before this ADR decodes rather than failing, and decodes
to an **empty** map — the one value that means "this operation named no
column".

A row insert with no bindings is then read **positionally**: exactly as it was
written, which is exactly what the replica that wrote it meant at the time.
And the merge says so, once per operation, with a `legacy-table-row-binding`
warning. That is the rule `legacy-operation-sequence-gap` and the rename set
that `opendoc.app-document.v1` introduced: an older repository is read as
written, and the weaker reading is reported rather than disguised. The payload
format has since moved to `v2`, and `signature-predates-payload-format` is the
same rule applied to a signature the re-encoding outran.

Backwards compatibility is not required here (CLAUDE.md). Being *silently
misread* is a different thing, and is what this avoids.

## Validation

`crates/opendoc-merge/src/table_fuzz_tests.rs`. The gap that let this ship is
that **no randomised generator in the repository ever emitted a table row or
column operation** — the ADR 0007 fuzz generates character and property
operations only — so the generator is the first half of the fix.

- **Convergence fuzz**, 2,000 seeds: three replicas that sync with each other
  at random, 12 row/column inserts (`First`, `Last`, `After` an anchor the
  author has observed) and deletes, merged under 8 stream partitions and
  compared as canonical CBOR bytes.
- **The oracle**, 2,000 seeds, and it is the test that matters: a row is live
  if it was inserted and not deleted, a column likewise, and the content at
  (row, column) is whatever the row insert bound to that column. That map is
  computed **without the merge** and is a function of the operation set, not
  of any order. Convergence alone would not have caught this: every replica
  agreed on the same wrong table.
- **Consequence A**, stated exactly, over three stream groupings.
- **Consequence B**, the deterministic one-actor crash.
- A row bound to a column another actor deleted loses that cell and says so.
- An operation in the pre-ADR wire shape decodes, is read positionally, and
  reports `legacy-table-row-binding`.
- `crates/opendoc-app/src/table_commands.rs`: `add_table_row` attaches the
  binding at the seam where unbound cells used to enter.
- `crates/opendoc-service/src/app_client_tests.rs`: the same case end to end
  over real sockets, so the binding has to survive the protocol's encoding —
  the app adds two rows while a peer deletes another column, and app, both
  replicas and the server agree on bytes *and* on where the text is.

### Falsification

Both halves were mutated back and the tests were run against them.

| Mutation | Result |
| --- | --- |
| `bind_row_cells` ignores the binding (the pre-ADR positional read) | the oracle fuzz, Consequence A and the dropped-cell test fail; **the convergence fuzz still passes** |
| `TableColumn::for_cell` replaced by the index-derived identity | Consequence B fails with `InvalidDocument("duplicate table cell id")` |

The first row is the point worth recording, and it is the same point ADR 0007
recorded: *the old merge did converge*. A convergence fuzz is not evidence of
correctness for a merge, which is why the oracle exists beside it.
