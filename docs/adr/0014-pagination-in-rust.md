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

Sizes: **149.1 kB of TrueType** compiled into the WebAssembly binary, **72.0 kB
of WOFF2** served to the browser (14.0–14.9 kB per face). The subset is Basic
Latin, Latin-1, Latin Extended-A, the modifier letters, General Punctuation,
currency, and the handful of symbols a word processor emits — 453 glyphs.

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

`generate.py` drops `kern` outright and empties `GSUB`, `GPOS` and `GDEF` — the
table shells survive, but with **zero lookups and zero features**, so nothing in
them is applicable. There is therefore no kerning and
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
attribute carrying Rust's page assignment — no structure changes, and nothing
the selection mapping or the block ids can see.

### Amendment, 2026-09-12: placement is a function, not a repair

As first written, this placement depended on the render having just happened:
`morphChildren` stripped the margin and the attribute on every render, and
`applyLayout` put them back. That made a block's DOM a function of the render
*and* of pagination having re-run since, which is why the frontend could not
skip re-morphing a block whose markup had not changed — and re-morphing every
block was the single largest cost on the typing path.

Two changes make the placement independent of the render, so the two can be
applied separately:

- **The owners no longer share a CSS property.** `opendoc-render` projects the
  model's space-before and space-after as the *logical* `margin-block-start`
  and `margin-block-end` — which is what the model means anyway, in flow order
  — and the physical `margin-top` is pagination's alone. A later declaration in
  the same inline style wins, so the placement overrides while it is set and
  the document's own spacing stands when it is cleared. While both used
  `margin-top`, applying a layout *deleted* every space-before in the document:
  a real, visible bug, measured in Chrome before it was fixed, and now guarded
  by an e2e check that reads the computed margin after pagination has run.
- **Pagination writes the placement of every block**, the ones the layout names
  and the ones it does not, rather than only the ones it was given. A block's
  placement after `applyPlacement` is exactly what the layout says, including
  the absence of one, whatever it was before — so a break that has moved
  cannot be left behind on a block nobody re-rendered.

`editor.ts` then updates only the blocks whose markup changed, comparing the
new markup against the markup it last applied rather than against the live
DOM. On a 1 500-block document that took the DOM leg of one keystroke from
30-43 ms to 9-12 ms, and an update that changes nothing from 31-42 ms to
0.1 ms. `opendoc-render` also gained a per-fragment projection of the body
(`render_document_body`), whose fragments compose to exactly the whole-body
string, and the projection now **carries those instead of the 389 KB string**
(`AppDocument::body_fragments`; `body_html()` reassembles it for the callers
that want it whole). `editor.ts` compares markup *strings* per block id, so
the fragment that changed is the only one parsed: on the same 1,500-block
document the DOM leg of a keystroke went from 9-12 ms to **0.9 ms**, and the
`innerHTML` parse of the whole body — 5-6 ms of it — is gone rather than
reduced. The detached copy of the body the previous scheme kept in memory to
compare against is gone too: a string per block id needs no tree.

Node identity comes from the live DOM, keyed on `data-block-id`, which is
sound because a fragment's key is also the first `data-block-id` written
inside it — on the element itself for every block kind, and on the first
`<li>` for a list run, whose wrapper carries none. `opendoc-render` pins that
(`every_fragments_first_block_id_is_its_key`), and an e2e check catches the
frontend end of it: mis-keying a list run rebuilds it on every keystroke
while leaving its content correct, so only node identity shows it.

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
- **Cost, measured — and then measured again.** The first pass through this
  crate laid a 1 500-paragraph, 150-page document out in 20 ms natively and
  37 ms in the browser, and the whole document is laid out again on every
  keystroke. That number was not a property of the algorithm: **two thirds of
  it was spent asking the `cmap` a question the ASCII cache already knew.**
  `Fonts::covers` — which the line breaker calls for *every* character, to
  decide whether the height is exact — went to the face's `cmap` even for
  printable ASCII, so the cache saved one binary search per character and the
  coverage test paid it straight back. Coverage is cached beside the advance
  now. The other third was a `String` allocated per character to describe a
  piece that the pagination path, which does not capture pieces, dropped
  unread; the piece is built lazily. Same arithmetic, same breaks — 180
  documents of the two paths' output compared byte for byte — at **2.9 ms
  natively and 4.7 ms in the browser**, 6.8x and 7.8x faster, with a 500-page
  document at 10.6 ms.
- **Incremental layout is therefore not worth doing yet**, and that is a
  measurement rather than a deferral. A cache keyed on anything less than
  everything that decides a fragment — the block, the frame width, the list
  run state, the type scale — would trade this crate's one guarantee for a
  few milliseconds it no longer costs. Pagination itself, the part that cannot
  be cached per block, is 0.3 ms of those 2.9 ms for 1 500 blocks; the rest is
  measuring, and measuring is now cheap enough that the honest linear pass
  wins. Revisit at documents an order of magnitude larger than 500 pages.
- The document is now drawn in a bundled font rather than in whatever the
  platform calls Arial. That is a visible change, and it is the point: the
  document looks the same on every machine because it is laid out the same on
  every machine.

### Amendment, 2026-09-13: incremental layout, by comparison rather than by key

The consequence above — "incremental layout is therefore not worth doing yet"
— is now **superseded**. Not because its arithmetic was wrong, but because the
premise it rested on has gone: it was written when the browser's DOM work
dominated a keystroke, and the per-fragment DOM work since (see the 2026-09-12
amendment) took the DOM leg to 0.9 ms. The layout pass then became the larger
of the two, and "a few milliseconds it no longer costs" stopped being true.

The objection itself was about the **key**, and it was right:

> A cache keyed on anything less than everything that decides a fragment — the
> block, the frame width, the list run state, the type scale — would trade
> this crate's one guarantee for a few milliseconds.

`opendoc_layout::cache::LayoutCache` answers it by **not having a key**. A
stored entry keeps the inputs themselves — the whole `Block`, by value, and
the `Frame` it was measured in — and is reused only when those compare *equal*
to the inputs of the pass asking. `Block` is `Eq`, so the comparison is the
whole block: kind, content, marks, properties. There is no digest to collide
and no field a future `Block` variant could add without this noticing. The
list run state, which the ADR named as the hard part, is not in the key
because it is not in the memoized unit: `FragmentSource` memoizes exactly
`text_fragment`/`block_fragment`, and everything a *run* decides — the marker
glyph, the ordinal, the bottom margin a finished run inherits — is applied by
the caller to the value that comes back, so it is recomputed on every pass
whether the fragment was reused or not. The two remaining inputs are
document-wide (the type scale and whether the document carries suggestions),
so they are stored once and the whole table is dropped when either differs.

**What made this cheap enough to be worth having**, measured on a 1,500-block
document in release native code: a full pass is 1.23 ms and comparing every
block of that document for equality is 0.037 ms — 33x cheaper than measuring
it. So a keystroke measures one block and compares 1,500.

| one keystroke, native | 750 blocks | 1,500 | 3,000 |
| --- | --- | --- | --- |
| uncached | 0.54 ms | 1.05 ms | 2.17 ms |
| cached | **0.13 ms** | **0.25 ms** | **0.52 ms** |

In the browser, `layout_document`'s WASM dispatch went from 2.35 ms to 1.4 ms
on 1,501 blocks (medians of four runs of twelve). The gap between 4.2x
natively and 1.7x in the browser is the wire: the command serialises 178 KB of
placements per keystroke, which the cache does not touch and which is now the
larger half of that dispatch. Trimming it is a contract change — `top_twips`,
`height_twips`, `lines` and the per-block `exact` have no consumer outside the
DTO, since `opendoc-pdf` reads `opendoc_layout::BlockPlacement` directly — and
it was left undone deliberately, being 2.5% of a keystroke.

`layout_painted_document` does **not** consult a cache; the PDF path runs the
same uncached pass it always did, so ADR 0014's "the PDF and the screen agree
by construction" is untouched.

The evidence is the only kind that counts for this:
`a_cached_layout_is_identical_to_an_uncached_one` generates 180 documents (60
seeds x 3 page setups) carrying every block kind, every inline kind, marks,
properties, nesting and text outside the bundled subset, drives each through
20 edits — typing, insertion, deletion, reordering, property changes and page
resizes — and compares the cached `DocumentLayout` against a freshly computed
one after **every single edit**: 3,600 whole-layout comparisons, field for
field. Six more tests pin the cases a wrong cache would get wrong (a changed
frame, a moved block, two documents sharing block ids, a document that gains
suggestions) and count the work rather than the answer, because a cache that
silently stopped hitting would still be correct.

### Amendment, 2026-09-13: a line box is a union, not a leading

The "exact" list above said a block's height is "a line count times a
leading". That was wrong, and PLAN88 P1-1 is the measurement that shows it: an
18pt-marked run in an 11pt paragraph is **36px per line in Chrome and was
22px here**, reported as `exact: true`, which put 8 of 41 blocks outside the
page this crate assigned them. A monospace run was worth another pixel per
line. Both are the same mistake — taking the leading from the *block* when CSS
takes it from each *inline box*.

A line box is the **union of the strut and every inline box on the line**, and
Chrome computes that union on a grid this crate now computes on too. Three
quantisations are load-bearing and each is worth a pixel or more:

- a face's ascent and descent are rounded to **whole pixels** (Blink calls
  `lroundf`), which is what makes `OpenDoc Mono` one pixel taller than
  `OpenDoc Sans` at the same size;
- the used `line-height` is quantised to `LayoutUnit`, **1/64 px**;
- the half-leading is added to the ascent and the sum **floored to a whole
  pixel**, with the descent taking whatever is left of the line height
  (`FontHeight::AddLeading`).

`Fonts::line_extent` reproduces all three in integer arithmetic, and
`text::Breaker` carries an extent beside every width it accumulates — the same
mirroring discipline the capture already used, so the extent of a word carried
onto the next line is carried with it. Vertical lengths stay milli-twips; the
line-box arithmetic is done in layout units and converted once per fragment.

The expected numbers are not this crate's own: they were **measured in Chrome
147** against the bundled WOFF2 faces, by reading a paragraph's height and the
position of a zero-height inline-block sitting on its baseline, over sizes from
8pt to 48pt, both families, and unitless and stated line heights. The tests
name them.

Two consequences:

- **Superscripts and subscripts are no longer estimated.** Blink derives their
  baseline shift from the *parent's* font size alone — not from the raised
  run's size and not from any font metric — as `LayoutUnit(size)/3 + 1px` and
  `LayoutUnit(size)/5 + 1px`. That integer form reproduces twenty measurements
  exactly, so the shift and the taller line box it causes are modelled and
  `EstimateReason::RaisedText` is gone. A footnote reference is measured the
  same way, and it now carries its **number** rather than a placeholder `0`.
- **Inline padding is not modelled; it is removed.** `.mark-code`,
  `.citation-label` and `.mention` carried 2px, 2px and 6px of horizontal
  padding that made this ADR's central guarantee — a run's width *is* the sum
  of its advances — false, and changed a real paragraph's line count. The
  padding is gone from `styles.css` rather than projected through `TypeScale`,
  because it is a **skin**: nothing in the document model says a code run is
  four twips wider, and a theme must not decide where the pages break. The
  HTML export never had it, so removing it makes the three surfaces agree
  rather than adding a fourth copy of a number. An e2e check reads the
  computed padding, border and margin of every such run back out of Chrome.

The `.doc-list` wrapper margin is fixed in the same pass: `ListWriter` closes
the outermost wrapper and opens another whenever a list run *changes marker*,
and the closed wrapper's `margin-bottom` — 13.33px, measured — was not in the
flow. `open_list_levels` now reports that case and the run walk pays it.

Finally, **the agreement fixture is widened**, which is the part that matters.
PLAN88 §7 is right that widening it would have caught these on the day they
landed, and that is now demonstrated rather than asserted: with the old
one-leading-per-block arithmetic restored, the original plain-paragraph check
still **passes** and the widened one **fails**. The widened fixture carries
every inline kind (code, size, superscript, underline, strike, colour, link,
mention, citation, footnote reference) and every block kind (heading, bulleted,
ordered and checklist runs with a marker change inside one run, an explicit
page break, a table), and a separate check asserts the fixture actually
contains all of them before any geometry is measured — a fixture that silently
failed to build would be the same failure over again.
