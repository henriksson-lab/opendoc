# ADR 0009: Page Geometry In Rust, Pagination In The View

Status: **partly superseded by [ADR 0014](0014-pagination-in-rust.md)**.

- Decision 1 (page geometry is a document fact and lives in `opendoc-core`)
  and Decision 2 (page setup and each furniture slot merge as whole values)
  **stand**, and 0014 depends on them.
- **Decision 3 — "pagination is a view concern, computed by measurement" — is
  superseded.** Page breaking is now computed in Rust, in `opendoc-layout`,
  against a bundled font that the browser also renders with; `paginate()`'s
  measurement code in `main.ts` is deleted. What changed is not that 0009's
  argument was wrong but that it was weighed without one requirement:
  pagination has to run browser-side *and* client/headless-side from one
  implementation, which a browser-only paginator cannot do. 0014 records how
  the font loop is closed so there is still only one layout engine.
- The "Not achieved" list under Decision 3 is therefore out of date; 0014 has
  the current one.

Covers PLAN77 B7 and B8 (parity FM-37, FM-40, FM-41, FM-47).

## Context

Before this change the page was a constant in a stylesheet:

```css
:root { --page-width: 816px; --page-padding: 96px 72px; }
.page { width: var(--page-width); min-height: 1056px; }
```

US Letter with 1in/0.75in margins, hard-coded in `apps/desktop/src/styles.css`,
not expressible in the model, not saved, not signed, not merged, and not
exportable. `opendoc-render` emitted one `<article class="page">` with a
`min-height`, so a fifty-page document was one very tall page. There were no
headers, no footers and no page numbers, and `opendoc-import` emitted
`google-dropped-document-part` and `docx-dropped-section-properties` warnings
for every real-world import because there was nowhere to put the data.

Two separate questions had to be answered:

1. **Where does page geometry live?** (B7)
2. **Where does page *breaking* live?** (B8)

They have different answers, and conflating them is the trap.

## Decision 1 — page geometry is a document fact and lives in `opendoc-core`

`Document` gains `page_setup: PageSetup`, `header: Vec<Block>` and
`footer: Vec<Block>`.

```rust
pub struct PageSetup {
    pub width: Length,
    pub height: Length,
    pub margin_top: Length,
    pub margin_bottom: Length,
    pub margin_start: Length,   // leading edge; left in a LTR document
    pub margin_end: Length,     // trailing edge
    pub margin_header: Length,  // sheet top    -> top of the header
    pub margin_footer: Length,  // sheet bottom -> bottom of the footer
}
```

Consequences of that shape, each chosen deliberately:

- **Twips, via `Length`.** Same unit as every other length in the model
  (ADR 0006). `Document` keeps `Eq`, two replicas serialize byte-identical
  CBOR, and DOCX section properties are already in this unit. An `f64` in
  points would cost both.
- **`start`/`end`, not `left`/`right`.** Margins follow the writing direction
  for the same reason block indents do; the renderer projects them onto
  logical CSS.
- **Orientation is derived, never stored.** `PageSetup::orientation()` reports
  `Landscape` when `width > height`. A stored orientation is a second
  representation of the same fact and can disagree with the dimensions it
  claims to describe — the same argument that made a hanging indent a negative
  first-line indent rather than a flag. `with_orientation` is idempotent, so
  asking twice for landscape does not turn the page 180°.
- **A named size is a UI affordance, not a document fact.** `PAGE_SIZE_PRESETS`
  is a table of `(name, label, portrait twips)`; picking "A4" writes 11906 by
  16838 and *nothing records that a preset was involved*.
  `PageSetup::size_name()` recovers a name by measuring, so a document imported
  from Word with A4 dimensions shows "A4" in the dialog without ever having
  been told. The presets reach the frontend through the projection
  (`AppPageLayout::size_presets`) so no paper dimension is hard-coded in
  TypeScript.
- **Validation is in the model.** Non-positive sheet, negative margin, or
  margins that leave no content box are `ModelError::InvalidDocument`, checked
  inside `Document::validate()` and therefore on every decode, every merge
  result and every snapshot.

### Headers and footers

They are `Vec<Block>`, so they validate, render and carry block properties like
any other content. Two extra rules:

- **They share the document's block/inline id space.** A header block that
  reused a body block's id would make every id-addressed operation ambiguous,
  so `Document::validate()` collects ids across body, header and footer.
- **They may not hold content whose meaning depends on the body flow**: a page
  break inside a header has nothing to break, and a footnote reference in a
  footer has no numbering context. Both are refused.

**Known limitation:** one header and one footer for the whole document. Word
and Google both allow different first-page and even/odd variants, and OpenDoc
has no section model to hang them on. An importer meeting one should emit a
`ModelWarning` rather than silently applying the default variant everywhere.

### Page numbers are a field, not text

```rust
Inline::PageNumber { id: StableId, field: PageNumberField }
// PageNumberField = CurrentPage | PageCount
```

The variant carries *which* number to print and never the number itself. The
value depends on where the pages were broken, which depends on the page size,
the font and the text — so it is not a property of the document. Consequences:

- `Document::visible_text()` contributes nothing for a field, so the word count
  does not change when the page size does.
- `opendoc-render` projects it to an **empty** `<span data-field="page-number">`.
  Filling in a number there would put a value in the markup that the document
  never stated. The stylesheet renders an unresolved field as `#`, the way a
  word processor shows one.
- DOCX writes `w:fldSimple w:instr=" PAGE "`, Google JSON writes `autoText`.
  Both formats model it as a field too, so it round-trips without degrading.

### Commands

`set_page_setup` writes the **whole** geometry rather than one dimension at a
time (see the merge section), `set_page_orientation` rotates it,
`set_page_furniture` replaces a slot with a paragraph of text plus an optional
field, `clear_page_furniture` empties one. All four are classified in
`OpenDocCommand::replaces_open_document()` as leaving the open document in
place.

## Decision 2 — page setup and each furniture slot merge as whole values

`OperationKind::SetPageSetup { page_setup }` and
`OperationKind::SetPageFurniture { slot, blocks }`. Concurrent writes converge
last-writer-wins — "last" meaning "sorts last in the causal order of ADR 0007".

The unit of last-writer-wins is deliberately **coarser** here than ADR 0006's
per-property rule for blocks. Per-dimension merge would let one actor's "switch
to A4" combine with another's "switch to Legal" into a page with A4's width and
Legal's height — a page *neither actor asked for*, which is the specific
failure mode ADR 0007 exists to prevent. "Switch to A4" is one intent and
merges as one value. Page setup and the two furniture slots are three
independent keys, so an actor editing the header and an actor editing the
margins both keep their edit.

`SetPageFurniture` is validated against the whole document before it commits,
because the only validator that can see an id collision between a header block
and a body block is `Document::validate()`. A rejected slot edit warns
(`invalid-page-furniture`) and leaves the document as it was.

**Known limitation:** header and footer blocks are *not* reachable by the
block-addressed operations (`InsertText`, `SetBlockProperty`, …) — merge's
block lookup walks `document.blocks`. Two actors typing in the same header
therefore converge on one of the two headers rather than on a character-level
merge of both. Making furniture blocks first-class would mean teaching every
`find_block_mut` caller in `opendoc-merge` about the two extra roots; it is a
contained change and it is not done here.

## Decision 3 — pagination is a view concern, computed by measurement

This is the B8 question, and it is the one with a real trade-off.

### What was considered

**(a) A Rust layout pass with a measurement oracle.** `opendoc-render`, or a
new layout crate, breaks the flow into pages by asking the host for text
metrics. Rejected. Page breaking is a function of *shaped* text — font
fallback, kerning, ligatures, line-breaking rules, the resolved line box — and
the host that knows those things is the browser's layout engine. A Rust pass
would either reimplement text shaping (a second layout engine that must agree
with the first, or the caret lands in the wrong place) or call back into the
browser per line, which would make `render_document` non-deterministic,
stateful and asynchronous. The crate's contract is that it is a pure
projection: same document in, same bytes out.

**(b) Full CSS fragmentation.** Let the browser do it with `@page`,
`break-inside: avoid` and friends. This is exactly right for *print*, and it is
what the print path does. It does not work for the on-screen editor: CSS has no
way to report where it broke, so the app cannot draw a page gutter, cannot
number pages, and cannot repeat a header — Chrome supports neither
`position: running()` nor `@page` margin boxes.

**(c) Measure in the browser, decorate the flow.** Chosen.

### What was built

The editable body stays **one continuous `contenteditable` flow**. It is not
split into one DOM subtree per page. That is the load-bearing decision:
`editor.ts` maps the DOM selection to `(block_id, inline_id, offset)` through
that single host, and every document operation is addressed by block id. A
per-page DOM would have to be rebuilt, and the selection re-mapped, on every
keystroke.

`paginate()` in `main.ts`:

1. Reads the page box out of the DOM. `.page-metrics` holds three invisible
   probes sized by `--page-height`, `--page-content-height` and `--page-gap`,
   the same custom properties the sheets use. Reading `offsetHeight` off them
   means the numbers are what the browser actually laid out, and they stay
   correct under the page-stack `zoom`, which scales measured offsets.
   No twips-to-pixels conversion happens in JavaScript.
2. Clears the previous pass's spacing and measures each top-level block's
   natural `offsetTop`/`offsetHeight`. Measuring from a cleared state is what
   makes a second run over unchanged content produce the same answer as the
   first.
3. Walks the blocks, opening a new page when a block would cross the bottom of
   the content box, or immediately after an explicit `BlockKind::PageBreak`.
4. Applies the result as a single `margin-top` on the block that opens each
   page — no nodes inserted, no structure changed, nothing the editor's
   selection mapping or the block ids can see.
5. Draws one absolutely positioned `.page-sheet` per page *outside* the
   editable host, each carrying a copy of the header and footer markup Rust
   rendered once, with every `[data-field]` filled in for that page.

Nothing is written back to the document. The pagination result is derived view
state; `opendoc-render` never learns how many pages there are.

### Print

`window.print()` plus `@media print` keeps working and is still how a PDF is
produced today. It is improved rather than replaced:

- `opendoc-render::page_setup_print_css` projects the same geometry as
  `@page { size: 595.30pt 841.90pt; margin: 0; }`. Custom properties do not
  apply inside `@page` in any shipping engine, so the print box needs a
  concrete rule; generating it in Rust keeps one projection of the page's shape
  rather than two that can drift.
- `beforeprint` re-runs the paginator with a zero gutter and `afterprint` puts
  it back, so the printed sheet boundaries sit on whole multiples of the page
  height, which is where the printer's own boundaries are. The gutter is the
  page viewer's, not the document's. (The *alignment* is by construction; that
  Chrome then paints each sheet's decoration on its own printed page is not
  verified — see the limitations below.)
- `.doc-page-break` uses `break-after: page` rather than the deprecated
  `page-break-after`.

## What this does and does not achieve

Achieved:

- Page size, orientation and margins are model state: set, validated, merged,
  saved, signed, and projected to CSS that Chrome computes correctly (verified
  on computed geometry, not on declarations).
- Headers and footers exist, render, and repeat on every page on screen.
- Page-number and page-count fields resolve per page, from a model that stores
  the field and not the number.
- The document is laid out into real, measured page boxes: content that does
  not fit page 1 starts at the top of page 2, and the page count follows the
  content.
- The importer has a model to map `documentStyle`, `headers`, `footers` and
  DOCX `sectPr` onto.

Not achieved, and deliberately not faked:

- **Repeated headers and footers in printed output.** They are drawn as
  absolutely positioned decoration, which the browser's print fragmentation is
  not obliged to repeat per page. On screen they are verified on every page;
  the printed result is **not verified** — the e2e harness drives the screen,
  not `Page.printToPDF` — so do not rely on them in print until something
  checks the PDF. That check is the natural first step of PLAN77 D1.
- **Breaking inside a block.** A paragraph taller than the content box is not
  split across two pages; it takes a page and overflows. Splitting mid-block
  needs line boxes, which is measurement the model has no representation for.
- **Widows, orphans, keep-with-next, break-inside: avoid.** Not modelled and
  not honoured.
- **Multiple columns, sections, and per-section page setup.** Not modelled.
- **First-page and even/odd header variants.** See the limitation above.
- **Vertical position of footnotes relative to the page.** The footnote area is
  still one block after the flow, not per page.

## Consequences

- `--page-width`/`--page-padding` are gone from `:root` as constants; the
  remaining `--page-*` values there are the model's own defaults used before a
  document is open, not a second source of truth.
- Adding a page-level property means adding it to `PageSetup` and its
  validation, and it reaches CSS, DOCX and Google JSON through the existing
  projections.
- PDF export (PLAN77 D1) can now ask a real question: it either drives this
  print path or rasterizes the measured page boxes. It is no longer blocked on
  "there is only one page".
