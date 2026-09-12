# ADR 0014: Pagination Moves Into Rust, Against A Bundled Font

Status: accepted. **Supersedes the pagination half of
[ADR 0009](0009-pagination-and-page-geometry.md)** — its Decision 3 ("pagination
is a view concern, computed by measurement"). ADR 0009's Decisions 1 and 2
(page geometry lives in `opendoc-core`; page setup and furniture merge as whole
values) stand unchanged, and this ADR depends on them.

Covers PLAN77 B8 (parity FM-47) and unblocks D1.

## Context

ADR 0009 put page *breaking* in the browser. Its reasoning was sound as far as
it went:

> Page breaking is a function of *shaped* text — font fallback, kerning,
> ligatures, line-breaking rules, the resolved line box — and the host that
> knows those things is the browser's layout engine. A Rust pass would either
> reimplement text shaping (a second layout engine that must agree with the
> first) or call back into the browser per line.

The fear is real and it is the right fear. But ADR 0009 weighed it against one
requirement it did not have: **pagination has to run browser-side *and*
client/headless-side, from one implementation.** OpenDoc is not only a web
surface. A Tauri shell, a future `opendoc-service`, and above all a Rust PDF
writer all need to know where the pages break, and none of them has a DOM to
measure. `paginate()` in `main.ts` is reachable from exactly one of the three
runtimes the repo is built around.

It also sat badly with the project's central rule — Rust owns semantics,
TypeScript owns the DOM. Where a page ends is not a fact about a DOM.

## Decision

Pagination is computed in Rust, in a new crate **`opendoc-layout`**, from the
`Document` and its `PageSetup` alone. It is reached through one command,
`layout_document`, and the frontend **places what it is told**. `paginate()`'s
measurement code is deleted, not kept as a fallback: a fallback that measures
would be the second engine ADR 0009 was right to fear.

### The answer to ADR 0009's fear

0009 worried about *two* engines disagreeing. The answer is that there is
**one** engine — in Rust — and the browser renders what that engine decided.
That is only true if the browser cannot reach a different answer, which is what
the rest of this ADR is about.

## The central problem: closing the font loop

Deterministic layout needs font metrics. Metrics computed against a font the
browser does not have are metrics for a document nobody sees — the same
mismatch ADR 0009 feared, arriving by a different route. Three things close the
loop, and all three are load-bearing.

### 1. One font, shipped twice from one subset

`crates/opendoc-layout/fonts/generate.py` subsets Liberation Sans (four faces)
and Liberation Mono (one) and writes each face **twice from the same in-memory
subset**: a TrueType file that `opendoc-layout` embeds with `include_bytes!`,
and a WOFF2 file that `apps/desktop/src/fonts/` ships and the stylesheet loads
through `@font-face`. WOFF2's transform is lossless for `hmtx`, so the two
encodings carry the same advance widths *by construction*, not by agreement.

Sizes: **148 KB of TrueType** compiled into the WebAssembly binary, **71.7 KB of
WOFF2** served to the browser (13.9–14.9 KB per face). The subset is Basic
Latin, Latin-1, Latin Extended-A, the modifier letters, General Punctuation,
currency, and the handful of symbols a word processor emits — 452 glyphs.

The frontend is deliberately bundler-less, and this is the first binary asset it
has. It needs no pipeline: the stylesheet's `url("./fonts/…")` resolves next to
`styles.css` in both the vite dev server and the static build, and
`scripts/build.mjs` copies the directory. That answers the asset question left
open when a subsetted math `woff2` was deferred earlier — the same shape works
there.

Upstream is Liberation, SIL OFL 1.1. Subsetting is a modification and the OFL
reserves the name "Liberation" for unmodified versions, so the family is renamed
to **OpenDoc Sans** / **OpenDoc Mono**. The licence travels with the files.

### 2. The subsets carry no shaping tables

`generate.py` drops `GSUB`, `GPOS` and `kern`. There is therefore no kerning and
no ligature for the browser to apply that Rust has not accounted for: a run's
width **is** the sum of its glyphs' advances, which is what `opendoc-layout`
computes. The stylesheet also declares `font-kerning: none`,
`font-variant-ligatures: none` and — the one that matters most —
`font-synthesis: none`, so a face that failed to load renders as the face that
did rather than as a synthesised weight whose advances no bundled face states.

This is why `rustybuzz`, `cosmic-text` and `swash` are not used. All three build
for `wasm32-unknown-unknown` (checked before any code was written; `rustybuzz`
compiles clean, and `cosmic-text` would need `fontdb` without its filesystem
features). But full shaping is only worth having if the browser is shaping too,
and here it deliberately is not. The dependency is **`ttf-parser` 0.25**,
`default-features = false, features = ["std"]`: pure Rust, no filesystem, no C,
and it builds for `wasm32-unknown-unknown` — verified, and `npm run build:wasm`
exercises it every time.

### 3. The type scale is projected, not duplicated

Font metrics are only half of a block's height. The other half is the type scale
— body size, leading, heading sizes, the space above a heading, a list's indent,
the checkbox's box. If Rust measured an `h1` at 20pt while the stylesheet drew it
at 24pt, the loop would be open again at the other end.

So `opendoc_layout::style::TypeScale` owns those numbers, and
`type_scale_css_variables()` projects them as `--doc-*` custom properties that
the layout command returns and `main.ts` installs as a `:root` rule. The
stylesheet reads `var(--doc-h1-size, 20pt)` — the literal is only the fallback
used before the first layout arrives, exactly as the `--page-*` fallbacks are.
The scale is stated once, in the crate that measures with it.

### Arithmetic: no floating point, anywhere

A line's width is accumulated as `advance_units * font_size_twips` and compared
against `available_twips * units_per_em`. Nothing is divided, so nothing rounds,
so a line mixing three sizes and two faces costs no precision at all. All
bundled faces share 2048 units per em, which is what makes one accumulator work
across them; `Fonts::load` asserts it.

Vertical lengths are **milli-twips** (20 000 to the point). Plain twips would
round a 1.15 line height on an 11pt paragraph; milli-twips keep leading exact
and stay integers. There is exactly one rounding step, `to_twips`, at the
output boundary.

## How the answer reaches the screen

`layout_document` returns, per block: its page, the top and height of its border
box in twips, its line count, whether the height was measured or estimated, and
— on the block that opens a page — the exact `margin-top` that puts it there,
already formatted as a CSS length.

`main.ts` does three things with that and computes nothing:

- sets `margin-top: calc(<the margin Rust gave> + var(--page-gap))` on each
  page-opening block;
- sets `--page-flow-height: calc(N * var(--page-height) + (N-1) * var(--page-gap))`;
- draws sheet *k* at `top: calc(k * (var(--page-height) + var(--page-gap)))`.

Three consequences fall out of that shape:

- **The gutter is not a document fact.** Rust describes pages that touch; the
  gap between sheets is the viewer's, and it enters only as `var(--page-gap)`.
- **Printing needs no special case.** The print stylesheet sets that gap to
  zero and every `calc()` follows. ADR 0009 needed `beforeprint`/`afterprint`
  listeners to re-run a measuring paginator with the gutter collapsed; both are
  deleted.
- **Zoom needs no special case either.** Everything is a CSS length under the
  page stack's `zoom`.

The margin is safe against CSS margin collapsing rather than in spite of it: it
is measured from the *previous block's border box*, and it always spans at least
one page's bottom and top margin, so it is always larger than the margin-bottom
it collapses with and the collapse resolves to exactly the stated value. A test
asserts that invariant rather than assuming it.

Two things ADR 0009 got right are kept unchanged. The editable body stays **one
continuous `contenteditable` flow** — it is not split into per-page DOM, because
`editor.ts` maps the DOM selection through that single host. And the only thing
written into the flow is still inert: a `margin-top`, plus a `data-page-index`
attribute carrying Rust's page assignment (no structure changes, nothing the
selection mapping or the block ids can see; `morphChildren` strips both on the
next render and `applyLayout` puts them back).

## What is laid out exactly, and what is estimated

Exact — height is a line count times a leading, both from bundled metrics:

- paragraphs and headings, with alignment, start/end indents, first-line and
  hanging indents, line spacing, and space before/after;
- CSS sibling margin collapsing, including the first block's margin collapsing
  out of the editable host;
- list items: run identity, nesting depth (the same wrapper-stack rule
  `opendoc-render`'s `ListWriter` uses, since that decides the indent),
  bullet/ordered/checklist wrappers, and the checkbox as an inline box;
- bold, italic and code runs, measured with the matching bundled face, and
  explicit size marks;
- explicit page breaks.

Estimated, and reported as such — `BlockPlacement::exact` is false for the block
**and for every block after it**, because an error above propagates down:

- **tables.** Cells are laid out for real against the column widths
  `table-layout: fixed` implies, but the distribution of leftover width and the
  interaction with merged cells are not pinned down by the model.
- **images with no stated height.** A stated height is honoured exactly; without
  one the drawn size depends on the image's own pixels, which this crate does
  not have.
- **block and inline equations.** A MathML box's size is the browser's math
  layout.
- **text outside the bundled subset.** The browser falls back to a font this
  crate cannot measure. Greek, Cyrillic, CJK and the rest are in this bucket.
- **superscripts, subscripts and footnote references.** Their *width* is
  measured; the raised box can make the line box taller than the strut, and that
  is not modelled.
- **documents carrying suggestions**, which render inline content that is not in
  `Block::content`.

Not attempted at all, deliberately: justification, hyphenation, widows and
orphans, `keep-with-next`, floats and text wrapping around them, columns and
sections, and the full UAX #14 break-class table (spaces, hyphens and dashes are
honoured; scripts that break without spaces are outside the bundled coverage
anyway).

**A block taller than the content box still takes a page and overflows.** That
was true before this change and is unchanged; it is visible and honest, and
splitting a block across pages needs line-box positions the frontend has no way
to consume without per-page DOM.

## Evidence

The failure mode to avoid is a paginator that is self-consistent and disagrees
with the screen, so the decisive check is in a real browser, not in Rust:
`npm run e2e` builds a multi-page document with wrapped paragraphs, a heading
and a list on a 3-inch page, then asserts that **every block's rendered box lies
inside the content box of the page Rust assigned it**, to within half a pixel.
Any line-count disagreement anywhere in the flow accumulates and pushes a later
block out of its page, so the check is sensitive to exactly the mismatch this
ADR is about. Two more browser checks guard the halves of the loop: that all
five bundled faces actually load and that the page asks for them, and that the
projected type scale is what Chrome computes with.

## Consequences

- `opendoc-render` stays a pure projection and learns nothing about pages. It
  still emits furniture once and page-number fields empty; the frontend still
  resolves the field values, because they depend on where the pages fell.
- A Rust PDF writer (PLAN77 D1) can now ask `opendoc-layout` where the pages are
  instead of driving a browser, and its output will agree with the screen
  because both came from this crate.
- Changing a document-surface size or spacing means changing `TypeScale`. The
  stylesheet follows; a literal left behind in `styles.css` is a bug, and the
  browser check on the projected scale is what catches it.
- `.doc-table`'s cell padding and borders are still literals in the stylesheet
  rather than projected. They match the scale's values exactly today; projecting
  them belongs with the next table change.
- **Cost, measured:** a 1 500-paragraph, 100-page document lays out in 25 ms in
  a native release build, and the whole document is laid out again on every
  keystroke. That is fine for the documents this prototype handles and it runs
  off the typing path (the command is awaited, the editor has already applied
  the edit), but it is linear in the document and it will need to become
  incremental — relaying only from the first changed block — before very long
  documents are comfortable. Printable ASCII advances are cached per face at
  load, which is what makes the constant small enough to defer that.
- The document is now drawn in a bundled font rather than in whatever the
  platform calls Arial. That is a visible change, and it is the point: the
  document looks the same on every machine because it is laid out the same on
  every machine.
