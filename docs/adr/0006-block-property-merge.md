# ADR 0006: Typed Block Properties and Their Merge Semantics

Status: accepted for v0.

## Context

`Block.properties` was `Vec<Property>` with `Property { key: String, value:
String }`. It was stringly typed, it had no validation, no unit, and no
enumeration of legal keys — and it was **never populated anywhere in the
codebase**. Every paragraph-formatting item in
`docs/GOOGLE_DOCS_PARITY_TODO.md` §3 (alignment, indents, line spacing,
paragraph spacing, direction) was blocked behind it, as were DOCX and Google
Docs import fidelity (`docs/archive` audit items FM-9/10/11) and PDF/DOCX
export.

Three questions had to be answered together, because the answer to each
constrains the others:

1. What shape do the typed properties take?
2. What operations write them?
3. What happens when two replicas write the same property concurrently?

## Decision

### 1. Typed optional fields, no untyped bag

`opendoc_core::BlockProperties` is a struct of typed `Option` fields:
`alignment`, `indent_start`, `indent_end`, `indent_first_line`, `line_spacing`,
`space_before`, `space_after`, `direction`. `None` means *inherit* — the model
never invents a default; the renderer and the exporters decide one.

`Property` and the untyped bag are **deleted**, not wrapped. There is no
lossy passthrough for unrecognised imported keys: an importer that meets a
property OpenDoc cannot represent emits a `ModelWarning`, which is the
mechanism `opendoc-import` already uses (`docx-dropped-section-properties`
and friends) and which is visible to the user. A silent string bag is not.

Consequences of "no bag": a DOCX or Google Docs property with no typed home is
dropped *and reported*, rather than round-tripped invisibly. That is a
deliberate trade of fidelity for honesty; D3 closes the gap by adding typed
homes, not by re-adding the bag.

### 2. One unit, carried by the type

Lengths are `opendoc_core::Length`, stored as **twips** (twentieths of a
point) in an `i32`:

- It is the DOCX unit, so DOCX import/export is exact rather than rounded.
- It is integral, so `Document` keeps `Eq` and two replicas that computed the
  same length serialize byte-identical CBOR. A bare `f64` would have cost both.
- `Length::from_points` / `from_inches` / `from_centimeters` are the smart
  constructors; the range is ±22in, wider than any page OpenDoc supports and
  far from `i32` overflow.

Line spacing is `LineSpacing::{Multiple, Exact, AtLeast}`, mirroring the three
rules DOCX and Google Docs both have. `Multiple` carries a
`LineHeightMultiple` in thousandths of a line (1000 = single), validated to
0.1×–10×.

A hanging indent is **not** a separate field or flag: it is a negative
`indent_first_line`. `BlockProperties::hanging_indent()` reads it back the
other way round, so the two representations cannot drift apart.

Enumerable values are enums with `as_str`/`parse`, never raw strings:
`Alignment::{Start, Center, End, Justify}` (direction-relative, with `"left"`
and `"right"` accepted as import aliases and never emitted) and
`TextDirection::{LeftToRight, RightToLeft}`.

### 3. `BlockProperty` carries its own key

`BlockProperty` is one enum whose variants are the property *and* its value
(`Alignment(Alignment)`, `IndentStart(Length)`, …), and `BlockProperty::key()`
maps it to `BlockPropertyKey`. An operation payload therefore cannot pair
`IndentStart` with a line-spacing value — the pairing is not representable.

Two typed operations exist:

```rust
OperationKind::SetBlockProperty   { block_id, property: BlockProperty }
OperationKind::ClearBlockProperty { block_id, key: BlockPropertyKey }
```

Their journal/envelope kinds (`set-block-property`, `clear-block-property`)
are derived from the payload by `rich_document_operation_kind`, the single
source of truth introduced when the `move-inline` kind-drift bug was fixed on
2026-09-11. No kind string is hand-written at a call site.

### 4. Merge: last-writer-wins, **per property**

Concurrent property edits converge under last-writer-wins at the granularity of
a single `(block_id, BlockPropertyKey)` pair.

Because the granularity is the property and not the whole bag, two actors who
concurrently set *different* properties of the same block both keep their edit.
Only two actors setting the *same* property race, and then exactly one value
survives. `SetBlockProperty` and `ClearBlockProperty` race the same way: a
clear is just another writer of that key.

A property edit against a concurrently deleted block degrades to a
`missing-block` warning, like every other block-targeting operation.

An out-of-range value that a smart constructor could not have produced — a
decoded or hostile payload — is rejected at apply time with an
`invalid-block-property` warning, and `BlockProperties::validate()` runs as
part of `Document::validate()`, so the range checks cannot be bypassed by
deserialization.

## Limits — what "last writer" does *not* mean yet

`merge_operations` orders operations in a `BTreeMap` keyed on
`(actor, seq)` and applies them in that order. That order is **deterministic**,
which is what convergence needs: every replica that has seen the same set of
operations reaches the same document, regardless of the order the streams
arrived in. The convergence tests in `opendoc-merge` assert exactly that, and
nothing stronger.

It is **not causal**. There are no vector clocks and no Lamport timestamps, so
"last writer" currently means "the writer whose actor id sorts last", not "the
writer who wrote most recently" and not "the writer who knew about the other's
edit". Two consequences follow, and neither is hidden:

- An actor whose id sorts early can be permanently out-voted on a property by
  an actor whose id sorts late, even when the early actor edited afterwards
  and with full knowledge of the other edit.
- A read-modify-write against a property a replica had already observed is
  indistinguishable from a blind concurrent write.

PLAN77 Phase F1 fixes the ordering itself (CO-3/CO-4), and that is where the
causal story belongs — it is not a property-specific problem, and no
property-specific workaround should be built for it here. When F1 lands, this
decision's *granularity* (per property) stays; only the definition of "last"
changes, and no operation payload has to change with it.

## Rejected alternatives

- **Whole-bag last-writer-wins.** Simplest to implement, and wrong in the
  common case: two collaborators, one setting alignment and one setting line
  spacing on the same paragraph, would silently lose one of the two edits.
- **A CRDT register per property (LWW-register with a timestamp).** This is
  what F1's ordering work effectively provides for free once causal ordering
  exists. Building a parallel timestamp mechanism for properties only would
  create a second, divergent ordering scheme in the same crate.
- **Merging property values structurally** (e.g. taking the larger indent).
  There is no user-meaningful merge of two alignments, and a rule that works
  for indents and not for alignment is worse than one rule everywhere.
- **Keeping `Vec<Property>` as an escape hatch.** See §1: the escape hatch is
  a warning, not a silent string.

## Consequences

- `Block.properties` is a typed struct, so `opendoc-render` (B4),
  `opendoc-import` (D3) and the DOCX/PDF writers (D1/D2) map typed values
  rather than parsing strings.
- `TableCell.properties` was deleted with `Property`. Cell styling is a
  different model (borders, background, span) and belongs to PLAN77 E2, which
  should add a typed `TableCellProperties` rather than reviving the bag.
- `AppBlock` (the app projection DTO) carries the properties as twips and
  canonical enum names, and parses them back through the same smart
  constructors, so the projection cannot introduce a value the model would
  reject.
- The commands and the contract (B3), the render projection (B4) and the UI
  (B9) build on this without reshaping it: they add command specs and CSS
  mapping over `BlockProperty` / `BlockPropertyKey`, not new model shapes.
