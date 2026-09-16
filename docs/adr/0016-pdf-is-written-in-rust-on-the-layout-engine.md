# ADR 0016: The PDF Is Written In Rust, On Top Of The Layout Engine

Status: accepted. Depends on [ADR 0014](0014-pagination-in-rust.md) (pagination
lives in Rust, against a bundled font) and [ADR 0010](0010-export-results-carry-their-own-warnings.md)
(an export's warnings ride its result). Closes PLAN77 D1 / parity FS-21.

## Context

PDF export was attempted once before, through the platform print pipeline
(Chrome's `Page.printToPDF` behind a Tauri command). It was **deliberately
reverted**, unbuilt rather than merely disabled, when the user ruled that
pagination must live in Rust so one implementation serves the browser, the
desktop shell and headless use.

That ruling is what makes this ADR possible. ADR 0009 had argued that a Rust
layout pass would mean two engines that must agree; ADR 0014 answered it by
making there be only one. `opendoc-layout` now breaks lines, flows blocks and
breaks pages deterministically, with no host measurement, against a font subset
it ships to the browser out of the same generator run — and it builds for
`wasm32-unknown-unknown`.

So the question this ADR answers is no longer *where* to paginate. It is: given
a layout, who draws it?

## Decision

**A Rust PDF writer, `crates/opendoc-pdf`, that consumes the layout result.**

Not a second traversal of the document. `opendoc-layout` gained a painting mode
— [`layout_painted_document`] — that runs the *same* pass and records what it
already decided: which glyphs sit at which coordinates, on which page. The PDF
writer receives placed runs and turns them into PDF operators. It measures
nothing, breaks nothing and decides nothing.

That is the whole point. ADR 0009 feared a PDF that drifts from the screen; a
PDF drawn from the pagination cannot drift from it, because a drift would
require the pass to disagree with itself.

### The recording happens inside the breaker, not beside it

The obvious shape — let the PDF writer re-break each paragraph — is exactly the
second engine ADR 0009 was right to fear, only hidden one level down. Instead
`text::Breaker` gained an optional capture whose every method mirrors one width
operation: a piece is recorded when its advance is committed, a line is emitted
when the accumulator wraps, and hanging spaces are dropped from the record at
the same moment they are dropped from the width. `measure` and `break_lines`
are one function with the capture off and on.

The capture is `Option<Capture>` because `layout_document` runs on every
keystroke and must not allocate a string per run for nobody.

`capturing_the_lines_does_not_change_where_they_break` asserts the two agree
over five page widths; mutating the space handling under capture breaks it.

## The PDF crate: `pdf-writer` 0.15

Chosen after checking the only property that disqualifies a crate outright:
**it must build for `wasm32-unknown-unknown`**, because the whole core reaches
the browser through `opendoc-wasm`, and `rust_xlsxwriter` trapping on a clock
call is recent enough in this repo to check before writing code rather than
after. A probe crate was built for the target before any of this was written.

`pdf-writer` is a step-by-step object writer — not a document model — with four
dependencies (`bitflags`, `itoa`, `memchr`, `ryu`), all pure Rust. No
filesystem, no clock, no C, no image codec. It builds for wasm32 clean, and
`npm run build:wasm` exercises that on every run.

Rejected: `printpdf` and `krilla` are higher level but pull in image decoding
and font machinery this crate does not want (the fonts are already chosen and
already subset); `lopdf` carries a `time`/`chrono` dependency, which is the
shape of the wasm32 trap already met once.

Consequences of a low-level writer, accepted:

- **No stream compression.** `pdf-writer` bundles no deflate. Content streams
  and the embedded faces are written uncompressed, which makes a typical export
  a few hundred kilobytes rather than a few tens. Adding `miniz_oxide` later is
  a contained change; paying a dependency now to save bytes nobody is short of
  is not.
- **Uncompressed streams are also readable**, which is why the crate's own
  tests can assert on the operators that were actually emitted rather than on
  what the writer intended to emit.

## Fonts: the same bytes, embedded

The faces embedded in the PDF are `FaceId::bytes()` — literally the TrueType
files `opendoc-layout` measured with, and the same subset the stylesheet serves
the browser as WOFF2. There is no second subsetting step and no metric
conversion, so the advances the page breaks were computed from are the advances
the reader will use.

Each used face is embedded as a **Type0 font with Identity-H encoding and a
CIDFontType2 descendant**. A simple font addresses at most 256 glyphs through a
byte encoding and the bundled subset carries 452 — Latin-1, Latin Extended-A,
punctuation, currency, the symbols a word processor emits. A composite font
addresses glyphs directly, so `café`, `€100` and `≤` on one line need no
re-encoding and no second font object.

Direct glyph addressing costs one thing, and it is paid: a PDF that names
glyphs has no extractable text unless it carries a **`ToUnicode` CMap**. Without
one, copying a paragraph out of the file — or checking it with `pdftotext` —
yields nothing. Every embedded face carries one, built from the glyphs actually
drawn, and a test asserts there is one per Type0 font.

Only the faces a document uses are embedded. A plain document ships one face,
not five.

## What is drawn, and what is not

**Drawn, from the layout's own numbers:** paragraphs, headings and list items
with their markers and checkboxes; bold, italic and code runs in the matching
bundled face; alignment; start, end and first-line indents including hanging;
line spacing; space before and after with margin collapsing; explicit page
breaks; tables as their cell text inside a ruled grid, including merged-cell
anchor rectangles and repeated leading headers across page continuations;
headers and footers on every page with their page-number fields resolved; and
the page geometry the document states, as the `/MediaBox`.

**Not drawn, and warned about by name:**

- **Images.** There is no image decoder here and the writer embeds no bitmaps.
  An image block becomes a frame of exactly the size the layout gave it, with
  its alt text below — so the page keeps its shape and the gap is visible
  rather than silent. `pdf-image-not-drawn`.
- **Block equations.** `opendoc-render` projects them to MathML and the browser
  lays them out; there is no math typesetter in this crate, and ADR 0003 makes
  the LaTeX source canonical anyway, so the source is what is set, in the mono
  face. `pdf-equation-drawn-as-source`.
- **Comments and suggestions**, which have no page to live on.
- **Bidi reordering.** A right-to-left block is placed at the right edge but
  its runs keep logical order. The layout has never reordered glyphs; the PDF
  does not pretend to.
- **A hollow bullet.** CSS cycles disc, circle, square by depth. The bundled
  subset has the disc (U+2022) and the square (U+25A0) but no hollow circle, so
  that depth falls back to the disc rather than printing `.notdef`. The
  fallback is decided in the layout, where it can be tested, rather than left
  to a font the reader may not have.

### An estimated layout says so

`opendoc-layout` reports `exact: false` for a block it estimated. That was a
boolean; it is now also a **reason** — `EstimateReason::{Table,
ImageWithoutHeight, Equation, TextOutsideBundledFont, RaisedText,
UnmeasurableMark, Suggestions}` — attached to the block whose own geometry was
guessed, not to every block after it. The PDF turns each into a warning naming
the block and the cause:

> `pdf-estimated-table`: block `tbl` was placed from an estimate, not a
> measurement: a table's row heights are estimated

Per ADR 0010 these ride `AppExport.warnings` and reach the warnings panel. A
document of measured blocks exports with no warnings at all, which is what
makes the warnings worth reading.

Characters with no glyph in the bundled subset are reported separately
(`pdf-glyph-outside-bundled-font`), with a sample, because they print blank.

## Not signed

ADR 0003 keeps signatures on source state and typed blob content, never on
rendered output. A PDF is rendered output. Export is `&self` throughout (ADR
0010), so the borrow checker enforces that exporting cannot touch the bytes a
signature covers.

## Deterministic

No clock, no random file id, no hash-map iteration order: the same document
exports to the same bytes. The DOCX writer already holds that property and it
is the only one that makes an export diffable, cacheable and reproducible.

## Evidence

The failure mode to avoid is a writer that is self-consistent and wrong, so the
decisive checks are made by something other than this writer:

- **poppler.** A fixture with a heading, a wrapped paragraph, bold/italic/mono
  runs, accented and symbol text, a centred line, an ordered list, a checklist,
  a table, an explicit page break, a header and a page-numbered footer, and
  forty filler paragraphs, exported and then read back:
  - `pdfinfo` reports 4 pages at 612 x 792 pt (Letter) with the document's
    title;
  - `pdffonts` reports the four used faces as embedded CID TrueType,
    Identity-H, with Unicode maps — and *only* the four used;
  - `pdftotext -f N -l N` extracts the right text on the right page in the
    right order, including `café`, `naïve`, `Łódź`, `größer`, `“quotes”`,
    `€100` and `≤ 5`, the list numbering, the table cells in column order, the
    header on all four pages and `Page N of 4` on each;
  - `pdftoppm` renders page 1 and shows the headings, the marks, the centred
    line, the ticked and unticked checkboxes, the table rules and the page-break
    rule.
- **In real Chrome** (`npm run e2e`), the PDF the File menu actually produces is
  captured out of the save path and compared against the page Chrome laid out:
  its `/MediaBox` must equal the sheet's computed size (a CSS pixel is exactly
  0.75 pt) and its page count must equal the number of sheets Chrome drew.

**Not verified:** Adobe Acrobat, macOS Preview and Windows print drivers were
not available here. `pdftotext`'s notion of reading order is poppler's, not a
guarantee about every reader. Nothing checks the PDF against a printed page.

### Amendment, 2026-09-13: the silent losses

"A document of measured blocks exports with no warnings at all, which is what
makes the warnings worth reading" was the claim. It was not true, and what
broke it was not blocks but *runs*: `TextStyle` carried only
`{size, bold, italic, mono}`, so **colour, highlight, underline, strikethrough
and links were dropped with no warning of any kind**, every footnote reference
printed a literal `0`, footnote bodies never appeared at all, and superscripts
and subscripts were drawn on the baseline.

Drawn now, from the layout's own numbers rather than from a second reading of
the marks:

- **Colour and highlight.** `RunDecoration` rides beside `TextStyle` — beside,
  not inside, because `TextStyle` is the *measuring* style and a colour has no
  business picking a face. A `MarkKind::Color`/`Background` value is parsed
  once, in the layout, so the screen and the paper cannot disagree about what
  `#e8f0fe` means; a value that is not a hex triple is **not** guessed at, it
  is reported as `pdf-estimated-unmeasurable-mark`.
- **Underline and strikethrough**, as bars at the position and thickness the
  *face* states, which is where the browser draws them.
- **Links**, as real `/Link` annotations with a URI action and no border,
  drawn in the link colour and underlined exactly as `.run-link` draws them.
  The rectangle comes from the run's own measured width, so it cannot be a
  different width from the text under it.
- **Superscripts and subscripts**, raised and lowered by the shift Chrome
  uses (see ADR 0014's amendment), which also makes their line box the right
  height.
- **Footnote references**, carrying the number the screen shows — numbered by
  order of first reference, the renderer's rule, cross-checked against the
  renderer by a test in `opendoc-render` because neither crate can call the
  other.
- **Footnote bodies**, as a numbered trailer after the last block, under a
  rule, at the caption size. This is the one thing the PDF places differently
  from the screen — the editing surface keeps them in an area under the whole
  page stack, which is not a page — so it is drawn *and* warned about by name
  (`pdf-footnotes-after-the-body`) rather than quietly omitted.
- **The explicit page break as a dashed rule**, because `.doc-page-break` is
  `border-top: … dashed` on screen and a solid bar is a different mark.

Two smaller corrections in the same pass. `.notdef` is **no longer recorded in
the `ToUnicode` map**: it is one glyph for every uncovered character, so an
entry for it claimed they were all whichever one reached it first — an Arabic
paragraph extracted as the same letter repeated. Absent text is honest, wrong
text is not, and `pdf-glyph-outside-bundled-font` already names it. And the
bullet/ordered marker cycle now stops at `depth-8`, where `styles.css` and the
standalone export stop stating one, instead of cycling past the last rule the
stylesheet has.

Verified the way the original work was: `pdfinfo`, `pdffonts`, `pdftotext` and
`pdftoppm` over a fixture carrying every one of the above, and in real Chrome
against the page Chrome laid out.
