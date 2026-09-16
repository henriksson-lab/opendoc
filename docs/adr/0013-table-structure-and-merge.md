# ADR 0013: Table Structure, Cell Spans, And How Two Editors Share A Grid

Status: accepted for v0. Covers PLAN77 E2 (parity OB-21).
**Decision 1's "a cell carries no `column_id`" is amended by
`docs/adr/0019-table-cells-name-their-column.md`:** it holds for a cell in the
document, and did not hold for a cell in an `InsertTableRow` payload, which is
generated against one replica's column list and applied against another's.

## Context

Tables were row-only. `BlockKind::Table { rows: Vec<TableRow> }`, `TableRow {
id, cells }`, `TableCell { id, blocks }` — and nothing else. There were no
columns in the model, so:

- there was no column operation anywhere in `opendoc-merge`, only
  `InsertTableCell` / `DeleteTableCell`, which lengthen or shorten **one row**
  and so produce a grid whose rows have different lengths;
- there was nowhere to put a column width, so every column was whatever the
  browser decided;
- there was no way to express a merged cell at all;
- `TableCell.properties` had been deleted along with the untyped `Property`
  bag (ADR 0006) with no typed replacement, so cell background, borders,
  vertical alignment and padding were unrepresentable.

Three questions had to be answered together, because each constrains the next:

1. What is a column, and what identifies one?
2. How is a merged cell represented, and what happens to what it covers?
3. What does merge do when two replicas edit one table at the same time?

## Decision 1 — a table is a rectangle, and a column is an identity

```rust
BlockKind::Table { columns: Vec<TableColumn>, rows: Vec<TableRow> }

pub struct TableColumn { pub id: StableId, pub width: Option<Length> }
pub struct TableRow    { pub id: StableId, pub cells: Vec<TableCell> }
pub struct TableCell {
    pub id: StableId,
    pub span: CellSpan,                  // default 1x1, skipped on the wire
    pub properties: TableCellProperties, // default empty, skipped on the wire
    pub blocks: Vec<Block>,
}
```

Every row carries **exactly one cell per column, in column order**.
`Document::validate()` rejects anything else, so a ragged table cannot be
decoded in, cannot be restored from a manifest, and cannot come out of a
merge.

Consequences, each chosen deliberately:

- **A cell carries no `column_id`.** On a rectangle the cell's index *is* its
  column, and a stored column id would be a second representation of the same
  fact — the thing `PageSetup::orientation` and `BlockProperties::hanging_indent`
  are both shaped to avoid. The invariant is what makes the redundancy
  unnecessary; validation is what makes the invariant safe to rely on.
- **Column widths are `Length` in twips**, like every other measurement in the
  model (ADR 0006 §2). `None` is *auto* — the model never invents a width, the
  view shares out what the sized columns leave. The spreadsheet's column
  widths are a different model for a different surface and are not reused.
- **`InsertTableCell` / `DeleteTableCell` now act on the whole column.** They
  are the old row-local operations and their payloads are unchanged, but a
  table that must stay rectangular has no meaningful "add a cell to one row":
  adding a cell to a row *is* adding a column. The column they create is
  identified by a value derived from the payload cell, so replaying one twice
  on two replicas builds the same column.

## Decision 2 — a merged cell is a span, and merging destroys nothing

```rust
pub struct CellSpan { rows: u32, columns: u32 }   // private, >= 1, smart ctor
```

The cell a merge starts at gets a span; the cells it covers **stay in the
grid**, keep their identity, keep their content, and are simply not drawn.

- Which positions are covered is **derived** (`table_covered_positions`), not
  stored. Only a cell whose span is not `SINGLE` can cover anything, and
  validation rejects overlapping rectangles, so the derivation is unambiguous.
- Splitting is therefore the exact inverse of merging and hands the covered
  content straight back. This is also what OOXML does with `vMerge`
  continuation cells, so DOCX round-trips without inventing anything.
- Covered content is *not* visible text: `Document::visible_text` skips it,
  and so does the renderer. It is retained, not shown.
- Merging over an already-merged region absorbs it: the inner spans are split
  back to single cells. That keeps "merge this rectangle" total — there is no
  region a user can select that the operation has to refuse.

The cost, stated plainly: an exporter that flattens the grid drops the covered
cells' content, because the target format has nowhere to put it. That is the
same trade ADR 0006 made — a loss that is *visible*, rather than a hidden
second representation.

## Decision 3 — structure converges by identity; styling by property

This is the part that had to be decided against ADR 0006 (last-writer-wins
**per property**) and ADR 0009 (whole-value LWW for `PageSetup`, because
merging width and height independently produces a page neither actor chose).
A table needs both answers, for different parts of itself:

| What | Rule | Why |
| --- | --- | --- |
| Rows and columns | Insert/delete anchored on a **neighbour's identity**, never an index | An index means different things on two replicas; an identity does not. This is what `InsertTableRow` already did, extended to columns. |
| Column width | Whole value, LWW, **per column** | Two columns' widths are independent facts — unlike a page's width and height, which are one shape. Two actors resizing different columns both keep their change. |
| Cell span | Whole value, LWW, **per cell**, then a geometry repair | A span is one rectangle, not two independent numbers; merging one actor's row count with another's column count would produce a merge neither asked for — the ADR 0009 argument, applied to a rectangle. |
| Cell styling | LWW **per property** | ADR 0006 unchanged: background, each border edge, vertical alignment and each padding edge are independent, so two actors styling one cell differently both keep their edit. |

### The repair pass

Some concurrent pairs cannot both be applied as written, however the
operations are ordered:

- a row inserted next to a column has **no cell where they cross**;
- two merges of overlapping rectangles both claim a position;
- a merge whose rectangle no longer fits the grid another actor shrank.

Rather than forbid those pairs, merge applies both and then repairs, in
`repair_table_geometry`, deterministically:

1. the grid grows to the widest row, so a cell is never dropped to make the
   rectangle fit — **content loss is never the repair**;
2. missing cells are filled with `TableCell::filling(row_id, column_id)`;
3. spans are clamped to the grid, and where two collide the one earlier in
   row-major order keeps its rectangle while the other splits back to a single
   cell;
4. anything the repair had to change is reported as a `table-geometry-repaired`
   warning — a collision between two people's edits is visible, not silent.

### Derived identity

Steps 1 and 2 mean merge sometimes has to invent a node no operation created.
Every replica must invent the *same* node, or two replicas that agree on every
character disagree on the bytes and so on the document hash. So
`opendoc_core::derived_stable_id(prefix, parts)` mints ids that are a **pure
function of the identities they sit between** — `TableCell::filling` from the
row and column, `TableColumn::filling` and `TableRow::filling` from the table
block. No counter, no clock, no coordinator.

`TableColumn::filling` originally also took the *index* of the hole it was
filling, which is not an identity and collided with itself once a delete
shifted the grid. ADR 0019 replaced it with `TableColumn::for_cell`, derived
from the cell that has no column; `TableColumn::filling` survives only for the
one column a table with no columns at all is given, and takes no index.

This also fixed a live bug: the pre-existing placeholders that merge pushed
when the last row or the last cell of a table was deleted used
`StableId::new`, so two replicas deleting the same last row converged to
documents that differed in that placeholder's ids.

## Alternatives rejected

- **Omit covered cells from their rows.** It is what HTML does, and it makes
  merging a *deletion* — the hardest thing in this codebase to converge — where
  the span model makes it a per-cell value write. It also loses the covered
  content, so splitting cannot be the inverse of merging.
- **Store a `covered: bool` on the cell.** Derivable from the spans, so it is a
  second representation that can disagree with the first.
- **Store `column_id` on every cell.** Same objection, given the rectangular
  invariant.
- **Reject, rather than repair, a concurrent pair that breaks the geometry.**
  `merge_operations` must return a valid document for *any* operation set; an
  operation that cannot be refused after the fact has to be repairable.

## Consequences for import and export

`opendoc-import` is untouched by this change beyond keeping it compiling, and
DOCX/Google table mapping is deliberately left to the crate's owner. The model
it should map onto:

- `w:tblGrid`/`w:gridCol` ↔ `columns`, widths already in twips — exact, no
  rounding;
- `w:gridSpan` ↔ `CellSpan::columns`, `w:vMerge` restart/continue ↔
  `CellSpan::rows` on the restart cell (the continuation cells are the covered
  cells, and they already exist in the model);
- `w:tcPr` shading, `w:tcBorders`, `w:vAlign`, `w:tcMar` ↔ `TableCellProperties`;
- Google Docs `tableColumnProperties`, `rowSpan`/`columnSpan`,
  `tableCellStyle` map one for one onto the same fields.

Anything still unrepresentable must emit a `ModelWarning`, per ADR 0006 — there
is no bag to put it in, by design.
