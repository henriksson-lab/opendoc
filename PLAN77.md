# PLAN77: From Research Prototype To Usable Docs/Sheets Replacement

Status: proposed 2026-09-11. Supersedes nothing — this is the sequencing layer
over `docs/GOOGLE_DOCS_PARITY_TODO.md` (the gap inventory, IDs stable) and
`docs/RESTRUCTURE_PLAN.md` (the architecture cleanup). Those two stay the
source of truth for *what* is missing; this file decides *in what order* and
*why*.

Baseline at time of writing: 391 tests passing / 0 failing, clippy `-D warnings`
clean, no contract drift, 260 open parity items (27 marked P0).

## Sequencing principles

1. **Harvest before building.** Several features are fully implemented in Rust
   and simply unreachable. Wiring them is hours of work for P0 payoff, and it
   was proved twice already (`insert_axis`/`delete_axis` shipped 2026-09-11 by
   wiring, not writing).
2. **One model change unblocks a cluster.** The paragraph/page work is the
   largest blocked cluster in the tree and has no service dependency. It is the
   spine of this plan.
3. **Correctness before distribution.** Merge ordering must be right before a
   service crate exists, or the server faithfully replicates wrong documents.
4. **Rust owns semantics.** Every phase below adds commands to
   `opendoc-api`, implements them behind the `OpenDocApp` facade, regenerates
   the contract, and only then touches TypeScript. No phase is complete with
   document semantics living in `main.ts`.
5. **Verification is browser-level.** jsdom passes markup that Chrome renders
   wrong — this already happened once (`table-layout: fixed` silently scaling
   every column width). UI phases are not done until driven in a real browser.

## Phase map

| Phase | Theme | Depends on | Parallel with |
| --- | --- | --- | --- |
| A | Harvest unwired code + stop data loss | — | B, C, G |
| B | Paragraph and page model | — | A, C, E, G |
| C | Version history | — | A, B, E, G |
| D | Export/import fidelity (PDF, DOCX, Docs JSON) | B | E, G |
| E | Equations and table structure | — | B, C, D, G |
| F | Collaboration correctness, then service | F1 before F2 | G |
| G | Coverage, e2e, restructure debt | — | everything |

A, B, C, E and G can start simultaneously. D waits on B (pagination feeds PDF).
F2 waits on F1 and on the ADR 0004 gate.

---

## Phase A — Harvest and stop the bleeding

Highest value per hour in the repo. Nothing here requires new algorithms.

### A1. Display computed number formats (SH-11, P0)
`crates/opendoc-spreadsheet/src/format.rs:1197` `display_value` is fully
implemented and **never called** — its only external references are the
generated DTO field name. Stored formats compute correctly and then render as
raw values. Call it in the projection path so `Cell.display_value` carries the
formatted string; keep the raw value alongside it (source vs projection).

### A2. CSV and XLSX import/export (SH-47, P0)
`crates/opendoc-spreadsheet/src/io.rs` has `parse_csv`, `export_csv`,
`import_xlsx` (calamine) and `export_xlsx` (rust_xlsxwriter) — all with zero
external references. Add four commands to `opendoc-api`, implement through the
spreadsheet service, regenerate the contract, wire File ▸ Import/Export. Use
the native file dialog path that already exists in `src-tauri`.

### A3. Range sort (SH-7, P0)
`structure.rs:` `sort_range` — zero external references. One command, one menu
entry, one dialog for column + direction.

### A4. Formula-shifting paste (P0)
`structure.rs:350` `transform_formula_for_paste` — zero external references.
The clipboard path currently pastes text without rewriting relative references.

### A5. Fill handle / series fill (SH-28, P0)
The only item in Phase A needing new logic. Drag-from-corner, series detection
(constant, linear, date, copy-formula-with-shift reusing A4).

### A6. Unsaved-change guards and crash journal (FS-6, FS-7, P0)
`open-repository`, `open-recent`, `import-word` and `import-json` discard
unsaved work silently; only quitting is guarded. Route all of them through the
existing guard. Then add a crash journal: append each committed operation
envelope to a recovery segment and offer replay on next open — the operation
journal already exists, this is persistence plus a prompt.

**Exit:** no spreadsheet function in `opendoc-spreadsheet` is reachable only
from its own crate; no user action can silently discard unsaved work; a `kill -9`
mid-edit loses at most the in-flight gesture.

---

## Phase B — Paragraph and page model

The spine. `Block.properties` is `Vec<Property>` where `Property` is
`{ key: String, value: String }` — stringly typed and **never populated**.
Everything in §3 of the parity list waits on this.

### B1. Typed block properties in `opendoc-core`
Replace the untyped bag with typed optional fields (alignment, indent
start/end/first-line, line spacing, spacing before/after, direction). Keep
`Vec<Property>` only if an escape hatch for unknown imported keys is wanted —
if so, document it as lossy passthrough, not as the model.

Follow `docs/RESTRUCTURE_PLAN.md` Phase 6: smart constructors, validated
ranges, no raw strings for enumerable values.

### B2. Operations, merge, and journal
New typed operation variants; merge must converge on concurrent property edits
(last-writer-wins per property is acceptable for v0 and must be stated in the
ADR). **Derive the envelope kind from the payload** — the `move-inline` drift
bug of 2026-09-11 is fixed for rich-document ops; do not reintroduce a
hand-written kind string.

### B3. Commands and contract
Set/clear per property, plus selection-scoped variants mirroring
`set_editor_selection_block_style`. Regenerate: `npm run generate:commands`.

### B4. Render projection
`opendoc-render` maps typed properties to CSS on the block element. Pure
projection — no mutation, no recalculation.

### B5. Fix list identity (FM-5, P0)
Every list currently shares the static id `list-main`, so two adjacent lists
are one list. Allocate a real `list_id` per list run; needed before checklists
and list-style variants mean anything.

### B6. Checklists (FM-33, P0)
`BlockKind::ListItem { list_id, level, ordered }` gains a checkbox state.
Prefer a third variant over `ordered: bool` so bullet/ordered/checklist are
exhaustive rather than encoded in flags.

### B7. Page setup, headers/footers, page numbers (FM-37, FM-40, FM-41, P0)
Document-level page geometry (size, orientation, margins) replacing the fixed
`--page-width: 816px` CSS vars. Headers/footers need a document-level block
container and a page-number inline variant.

### B8. Pagination and print layout (FM-47, P0)
The renderer currently emits one `<article class="page">` with a `min-height`.
Real pagination means measuring content into page boxes. This is the hardest
item in Phase B and the precondition for credible PDF export — schedule it
last and treat `window.print()` + `@media print` as the standing stopgap until
it lands.

### B9. Toolbar and menu UI
Alignment, indent/outdent, line spacing, spacing, page setup dialog. Watch for
listener stacking — bind once via delegation, or assign rather than add.

**Exit:** a document with alignment, indents, spacing, headers, footers, page
numbers and checklists round-trips through save → close → open → sign → verify
unchanged, and renders the same in app and print.

---

## Phase C — Version history (CO-16, P0)

Cheap relative to value: content-addressed objects, manifests and
`ManifestRecord.parent` already exist and are tested — today only merge
internals walk the parent chain (`crates/opendoc-store/src/lib.rs:794,817`).

- C1. `list_document_versions` — walk parents from head, project id, timestamp,
  signer, label.
- C2. `open_document_at_version` — load a manifest-addressed snapshot read-only.
- C3. `restore_document_version` — commit the old snapshot as a new head, never
  rewriting history.
- C4. `name_document_version` — a label record alongside the manifest.
- C5. `diff_document_versions` — block-level add/remove/change projection, in
  Rust, reusing merge's comparison machinery.
- C6. UI: a version panel; restore behind a confirm dialog.

**Exit:** any prior version can be listed, previewed, named, diffed and
restored, and restoring is itself an auditable, signable commit.

---

## Phase D — Export and import fidelity

Depends on B (pagination and the property model).

- D1. **PDF export (FS-21, P0).** Once B8 produces page boxes, render them to
  PDF. Decide explicitly: a Rust PDF writer crate, or driving the platform
  print pipeline. ADR 0003 says rendered-output signatures are out of scope —
  do not sign PDFs.
- D2. **DOCX export (FS-22, P0).** `opendoc-import` already does DOCX *reading*
  with `zip` + `quick-xml`; the writer is the mirror. Map the Phase B
  properties, not just runs and headings.
- D3. **Google Docs JSON import robustness (FM-9/10/11, FS-24, P0).** Import
  currently drops all paragraph formatting, treats every numbered list as
  bulleted, and ignores `documentStyle`/`headers`/`footers`/`namedStyles`
  without warning. With B in place these become mappings. Anything still
  unsupported must emit a `ModelWarning`, never silence.
- D4. **Markdown/ODT export** (P1) if cheap after D2.

**Exit:** a Google Docs export imported into OpenDoc keeps its formatting or
says exactly what it dropped; a document exported to DOCX reopens in Word with
its structure intact.

---

## Phase E — Equations and tables

- E1. **Equation rendering (OB-23, P0).** No math engine exists; `opendoc-render`
  emits escaped LaTeX. Bundle KaTeX (desktop app, no CDN) and render to MathML
  or HTML. Keep LaTeX as the canonical source per ADR 0003 — rendering is
  projection.
- E2. **Table structure (OB-21, P0).** Tables are row-only: no column
  operations anywhere in `opendoc-merge`, no merge/split cells, no column
  widths, no cell styling, and no table UI in the document surface. Needs core
  model, ops, merge convergence tests, commands and UI — treat as its own
  slice, comparable in size to a Phase B sub-item.
- E3. **Image sizing/positioning (OB-17), image from URL/clipboard/drag
  (OB-18).** The `drop` handler in `editor.ts` is a bare stub.

---

## Phase F — Collaboration

### F1. Merge correctness (CO-3, CO-4, P0) — do this regardless of F2
Character-level operations exist, but `merge_operations`
(`crates/opendoc-merge/src/lib.rs:227`) orders by a `BTreeMap` keyed on
`(actor, seq)` and never transforms offsets. Two machines editing concurrently
converge to **wrong text**, not merely conflicting text. ADR 0001 promised a
CRDT; the current design does not deliver one.

Decide and record in a new ADR: CRDT sequence identities versus operational
transformation over the existing op set. Then implement causal ordering
(vector clocks or Lamport timestamps) plus offset transformation, and extend
the existing convergence suite — `opendoc-merge` already has 137 tests and is
the right place to prove this with fuzzing, per ADR 0003.

This is local correctness work. It needs no server and should not wait for one.

### F2. Service crate (CO-15, CO-18, CO-8/9/10/19/44) — gated by ADR 0004
Create `opendoc-service` only once F1 lands and the local boundaries are
stable, per ADR 0004. It owns authenticated transport, identity, server-enforced
permissions, presence fanout, a durable operation log and commit serialization.
Today authorization is a pure function of client-supplied grants — that must
stop being true before anything is exposed on a network.

### F3. Browser storage adapter
IndexedDB behind a Rust-defined storage trait, so the WASM shell persists
locally instead of holding documents in memory.

---

## Phase G — Quality and debt (continuous)

- G1. **Restore coverage (QA-6, P0).** 391 tests today against 653 before the
  monolith split; `opendoc-app` and `opendoc-spreadsheet` are the thinnest and
  carry the most recently moved code. Every phase above adds tests to its own
  crate — treat "no new tests" as a failed review.
- G2. **Real-browser e2e (QA-3, P0).** Chrome-via-CDP was used ad hoc on
  2026-09-11 and caught a layout bug jsdom structurally cannot see. Make it a
  committed harness (Playwright or tauri-driver) in `npm run verify`.
- G3. **Split `main.ts`** (1,871 lines) by surface, per RESTRUCTURE_PLAN Phase 5.
- G4. **Move `findMatches` to Rust** — it walks the document tree doing
  case-folded matching with code-point arithmetic, violating the TS boundary.
  Take ED-24 (match case, whole word, regex) and ED-25 (matches spanning two
  runs) with it.
- G5. **Oversized Rust files:** `opendoc-merge` 12,227 lines, `opendoc-store`
  4,744, `opendoc-import` 5,053, `docx.rs` 2,332. Split as each is touched
  rather than in one pass — F1 will rewrite much of `merge` anyway.
- G6. **Replace `new_sample()` as the boot path** for Tauri and WASM with
  explicit `new_empty_document` / `new_empty_workbook` constructors.
- G7. **`verify.mjs` runs debug cargo** (`cargo test`/`clippy` without
  `--release`), contradicting the repo's release-only rule. It is shared with
  CI, so change it deliberately.
- G8. **Dead CI path filter (PL-13):** `.github/workflows/desktop.yml` still
  filters on `docs/GOOGLE_DOCS_EQUIVALENT_PLAN.md`, which moved to
  `docs/archive/`.
- G9. **Derive blob and spreadsheet envelope kinds from their payloads**, as
  rich-document ops now do — 42 hand-written kind strings still carry the drift
  risk that caused the `move-inline` bug.

---

## Deferred, with reasons

- **Signing rendered output** — ADR 0003: signatures cover source state and
  typed content, never rendered exports.
- **Garbage collection of deleted content** — ADR 0003 retains it for now.
- **S3-compatible storage as default** — ADR 0003 keeps on-disk first; the
  OpenDAL adapter already exists behind the `opendal-store` feature.
- **Backwards compatibility** — explicitly not required while OpenDoc is a
  research prototype. Prefer deleting a bad boundary to wrapping it.

## Working agreement

Every phase item lands as: Rust model/ops → command in `opendoc-api` →
implementation behind `OpenDocApp` → `npm run generate:commands` → render
projection → UI → tests at both levels. `npm run verify` green before it counts
as done, and UI work verified in a real browser, not only jsdom.

Tick the corresponding IDs in `docs/GOOGLE_DOCS_PARITY_TODO.md` as each lands;
that file stays the inventory and its IDs are never renumbered.

---

## Progress log

### 2026-09-11
- **B1 done.** `Block.properties` is `opendoc_core::BlockProperties`: typed
  `Option` fields for alignment, indent start/end/first-line, line spacing,
  space before/after and direction. `Property` and the untyped bag are
  deleted, not wrapped — there is no lossy string passthrough; an importer
  that meets an unrepresentable property emits a `ModelWarning`, the mechanism
  `opendoc-import` already uses. Lengths are `Length`, stored in twips so the
  model keeps `Eq` and two replicas serialize identical bytes; a hanging
  indent is a negative `indent_first_line` with no second representation to
  keep in sync. Alignment, direction, line-spacing rule and property key are
  enums with `as_str`/`parse`, and `BlockProperties::validate()` runs inside
  `Document::validate()` so a decoded out-of-range value cannot slip past the
  smart constructors.
- **B2 done.** `OperationKind::{SetBlockProperty, ClearBlockProperty}`.
  `BlockProperty` carries its own key, so a payload cannot pair an indent key
  with a line-spacing value. Envelope kinds come from
  `rich_document_operation_kind`; no kind string is hand-written. Merge is
  last-writer-wins **per property**, so two actors editing different
  properties of one block both keep their edit — recorded, with its limits, in
  `docs/adr/0006-block-property-merge.md`. The ADR states plainly that
  `merge_operations` orders by `(actor, seq)` with no causal ordering, so
  "last writer" means "sorts last", not "wrote last", until F1 lands.
  `opendoc-merge` gained 10 convergence tests including a deterministic
  pseudo-fuzz across four replicas.
- **B5 done (FM-5).** The static `list-main` id is gone. List-run identity
  lives in `app/document_tree.rs`: a run is a maximal sequence of adjacent
  sibling list items sharing one id; a new item joins the run it lands next
  to; a selection converted in one gesture becomes one list, not several; and
  an item leaving a run mid-list moves the tail onto a fresh run so numbering
  stops counting across the paragraph that now separates the halves — in the
  style commands and in the editor's leave-list and backspace-at-start paths.
- **B6 model done (FM-33).** `BlockKind::ListItem { list_id, level, kind:
  ListKind }` with `ListKind::{Bullet, Ordered, Checklist { checked }}` — a
  third variant, not a second bool, so every match site must decide what a
  checklist does and a bulleted item cannot be "checked". Indenting a list
  item now carries the marker through unchanged instead of retyping it.
  Creating and toggling a checklist still needs a command (B3) and a checkbox
  in the renderer (B4).
- **Left for the next wave.** B3/B4/B7/B8/B9 build on this without reshaping
  it: `OpenDocApp::{set,clear}_block_property` and their selection-scoped
  variants exist behind the facade, so B3 adds command specs and dispatch
  arms rather than model code. The command args still say `ordered: bool`;
  a checklist command is the one addition the contract needs.
- **G7 done.** `apps/desktop/scripts/verify.mjs`, `package.json`
  (`generate:commands`) and `scripts/native-check.mjs` now pass `--release` to
  every cargo invocation. One profile across the gate also means the steps
  share a target directory instead of building the tree twice.
- **G8 done.** `.github/workflows/desktop.yml` no longer filters on
  `docs/GOOGLE_DOCS_EQUIVALENT_PLAN.md` (archived, so the trigger was dead); it
  now watches `PLAN77.md`, `docs/RESTRUCTURE_PLAN.md` and
  `docs/GOOGLE_DOCS_PARITY_TODO.md`.
- **Doc correction.** `apps/desktop/README.md` claimed a "workflow check"
  verifies the CI file's contents. No such check exists in `scripts/` or any
  crate — the section now describes the gate as it really is.
- **E1 done.** Equations render as MathML generated in Rust
  (`crates/opendoc-render/src/equation.rs`), via `math-core` 0.8.2 (MIT). No
  JavaScript math library, no font bundling, no new frontend runtime
  dependency. `latex2mathml` was rejected as six years unmaintained.
  Malformed input degrades in three tiers — unknown command inside a valid
  formula still renders the rest and flags it, unparseable source falls back
  to escaped LaTeX with a hover message, and `EquationSourceFormat` is matched
  exhaustively so a future non-LaTeX format fails to compile rather than being
  silently treated as LaTeX. Renderer warnings are returned rather than
  written back, so projection purity holds.
- **G2 done.** `npm run e2e` drives a real browser over the Chrome DevTools
  Protocol (`scripts/cdp.mjs`, `scripts/e2e.mjs`) with **zero new npm
  dependencies** — Node's own WebSocket behind `--experimental-websocket`.
  It runs its own vite server on its own port so it cannot disturb a dev
  server in use. Every assertion is one jsdom cannot make: computed geometry,
  computed styles, or text produced by Chrome's own key handling through
  `contenteditable`.

### Findings that constrain future work

- **Browser choice is load-bearing.** `/usr/bin/google-chrome` on this machine
  is Chrome 104, which predates MathML Core (Chrome 109) and renders equations
  as flat text. `findChrome` now picks the newest installed browser by probed
  version and refuses anything below 109 rather than producing untrustworthy
  results quietly.
- **Snap-confined Chromium cannot read a profile under `/tmp`** and hangs
  rather than reporting why. The harness puts throwaway profiles under
  `$HOME/.cache/opendoc-e2e`.
- **Coordinate clicks need hit-testing.** Toolbar and menu share
  `[data-action="mark:bold"]`; the menu item comes first in the DOM, so a
  naive `querySelector` click landed on whatever was topmost and made a
  working feature look broken. `click()` now verifies `elementFromPoint`
  resolves to the intended element and throws a clear error otherwise.
- **Still to do for E1:** equation `ModelWarning`s are produced by the
  renderer but not surfaced in the UI — needs ~3 lines in
  `crates/opendoc-app/src/{render_service,projection_service}.rs` mapping them
  to `AppWarning`, alongside `append_spreadsheet_formula_warnings`.
- **Stretchy delimiters do not stretch** without a math font installed
  (`Latin Modern Math` / `STIX Two Math`). Fixing it means bundling a
  subsetted woff2 (~100-400 KB) into a deliberately bundler-less frontend —
  an asset-pipeline decision left open.

### 2026-09-11 (wave 1 complete)

Six parallel agents landed. Tree green: **493 tests passing / 0 failing**
(baseline was 391), clippy `-D warnings` clean, `cargo fmt --check` clean, no
contract drift, typecheck clean, jsdom smoke passing, real-browser e2e passing.

- **A1-A5 done.** Number formats now render (`display_value` was in fact being
  computed — the break was one hop later, in `render_workbook_html`, which
  emitted `computed_value`). CSV/XLSX import/export, range sort, and
  formula-shifting paste are wired to the previously dead
  `opendoc-spreadsheet` code. Fill handle implemented with series inference in
  Rust. **G9 done in passing**: all 43 spreadsheet journal call sites now
  derive their envelope kind from the payload; zero hand-written kind strings
  remain.
- **A6 done.** The unsaved-changes guard is structural, not per-call-site:
  `OpenDocCommand::replaces_open_document()` is an exhaustive match with no
  catch-all, so a new command fails to compile until classified, and the
  dispatcher refuses the command rather than relying on the UI to ask. Crash
  recovery journals a base snapshot plus subsequent envelopes and replays them
  through the existing merge machinery; startup never replays without consent.
  ADR 0005.
- **B1/B2/B5/B6 done.** `Property` and the untyped bag are deleted, not
  wrapped. `Length` is stored in twips (`i32`) so `Document` keeps `Eq` and two
  replicas serialize byte-identical CBOR — an `f64` would have cost both.
  Hanging indent is a negative first-line indent, so there is one
  representation. `ListKind::{Bullet, Ordered, Checklist}` is a third variant
  rather than a second bool. `list-main` is gone; list runs get real
  identities that split when a paragraph cuts them. Merge is last-writer-wins
  per `(block_id, key)`. ADR 0006.
- **C done.** Version history lists, opens read-only, names, diffs and
  restores; restore commits a new head so history is never rewritten. Found
  and fixed three real bugs, including a diff that reported untouched blocks as
  changed because it compared inline `StableId`s.

### 2026-09-11 (wave 2)

- **B3 done.** 21 commands: `set_block_alignment` / `set_block_direction`,
  `set_block_{indent_start,indent_end,indent_first_line,space_before,space_after}`
  (twips), `set_block_line_spacing` (rule + value), each with a
  selection-scoped twin, plus `clear_block_property` /
  `clear_editor_selection_block_property`, `set_list_item_checked` and
  `adjust_editor_selection_indent`. One command per property rather than a
  generic `(key, value)` pair: the value's type is the command's identity, so
  a payload cannot pair an indent key with a line-spacing value, which is the
  same invariant `core::BlockProperty` enforces inside the model. Clearing
  carries no value, so one keyed command covers every property there.
  `ordered: bool` is gone from the contract — every list command now names its
  marker (`"bullet"`, `"ordered"`, `"checklist"`), `AppBlock.style_value`
  reports `list:<marker>`, and the checkbox moves only through
  `set_list_item_checked`. A checklist created or converted through a style
  command starts unchecked: there is deliberately no "remember the tick"
  heuristic, because one would make a checklist impossible to convert away
  from cleanly.
- **B4 done.** `opendoc-render` projects the typed properties as CSS on the
  block element and renders checklist items with a real checkbox. Twips ->
  CSS uses `pt`, not `px`: 20 twips *is* 1pt, so every value lands exactly on
  a 0.05pt grid and the browser does the pt -> device-pixel step itself; a px
  projection would have to assume a DPI and drift. Indents are logical
  (`margin-inline-start/end`) because the model's start/end indents flip with
  the block direction. `Exact` line spacing is an acknowledged approximation:
  CSS has one `line-height` rule and it behaves as `AtLeast`. The checkbox is
  an `<input type="checkbox">` whose click is taken by its wrapper — the input
  keeps `pointer-events: none`, so it never goes "dirty" in the HTML sense and
  its checkedness keeps following the attribute a re-render writes; toggling
  goes through `set_list_item_checked`, never a DOM mutation.
- **B9 done.** Alignment buttons (SVG icons, Ctrl+Shift+L/E/R/J), line-spacing
  and space-before/after selects whose empty entry *clears* the property back
  to inheriting, a checklist toggle, and indent/outdent now routed through
  `adjust_editor_selection_indent` so Rust decides whether a gesture moves a
  list level or a block indent. Every control is rendered into the existing
  `morphChildren` toolbar and bound by assignment or by the single delegated
  `[data-action]` listener — no `addEventListener` on a re-runnable path.
- **Fixed in passing.** `.find-bar` and `.error-banner` declared
  `display: flex` on the class, which beats the `hidden` attribute's UA rule,
  so both rendered as empty strips at all times. A screenshot of the new
  toolbar is what exposed it.

### Known gaps carried forward

- **B7/B8 remain**: page setup, headers/footers, page numbers and real
  pagination. B3/B4/B9 landed in wave 2.
- **Adjacent list runs do not auto-merge** after deleting the paragraph
  between them; cosmetic, never corrupting.
- **Google JSON export loses checklist state** (exports as bullet) — belongs
  to D3.
- **Autosave still requires a repository root**, so it does not run before the
  first manual save. The data-loss consequence is covered by the journal, but
  where to autosave an unnamed document is a product decision (FS-41).
- ~~**Browser/WASM has no crash protection**~~ — done 2026-09-11 with F3: the
  browser installs `VolumeRecoveryJournalStore` over IndexedDB (ADR 0008).
- **XLSX cell styles are written but not read back** (calamine reads values
  only), so `$1,234.50` re-imports as `1234.5`.
- **e2e cannot yet verify column sizing end to end.** The synthetic assertion
  was removed after it proved unfalsifiable — re-introducing the original CSS
  bug left it green, so it guarded nothing. Doing it properly needs a page
  seam to drive `set_spreadsheet_column_width` and measure the result.

### 2026-09-11 (wave 2 complete)

- **F1 done — the deepest correctness bug in the project.** ADR 0007.
  Causality is now explicit metadata (Lamport + vector clock) with a
  deterministic topological sort replacing `(actor, seq)` iteration;
  `context: None` provably degrades to the old order, which is why all 147
  pre-existing merge tests pass unchanged. OT was **rejected**: peer-to-peer OT
  under arbitrary reordering needs TP2, and the classic transforms satisfy TP1
  but not TP2 — convergent-looking corruption is the exact failure to avoid.
  A merge-time sequence CRDT is used instead, with **derived** (not stored)
  character identities, so the signed model, `Eq` and canonical CBOR are
  untouched. Accepted cost, documented in the ADR: once a merge result is
  written back as flat text, a later-arriving operation predating it degrades
  to positional application.
  Cases that previously produced text *neither actor wrote* — overlapping
  deletes keeping a character both deleted, an insert landing inside another
  actor's concurrent insert and tearing it in half — now converge correctly.
- **B3/B4/B9 done.** 21 commands, one per property rather than a generic
  `(key, value)` pair, so the value's type is the command's identity and an
  indent key cannot carry a line-spacing value. `ordered: bool` is gone from
  the contract in favour of `listKind`. Twips project to **pt, not px** (20
  twips is exactly 1pt, so no rounding and no assumed DPI).
- **D3 done.** Paragraph formatting round-trips through Google JSON and DOCX.
  Four hard aborts that killed real-world Google imports now degrade with
  warnings; the line drawn is *unsupported feature* degrades, *corrupt input*
  aborts. Google's absolute `indentFirstLine` is converted to OpenDoc's
  relative form on the way in and back on the way out.
- **G6, equation warnings, list-run merge done.** `new_sample()` is now
  `#[cfg(test)]`, so booting a runtime into demo content is a compile error
  rather than a convention.
- **FS-19 fixed (by me).** "Blank spreadsheet" shipped seeded with
  Item/Apples/Total: `new_document` installed a sample workbook, and
  `reset_after_document_import` re-installed it on every import. Both now start
  blank; the spreadsheet test fixture states its own demo cells instead of
  reading them back out of the constructor. Guarded by an e2e check.

### Open decision: export warnings have nowhere correct to go

`export_google_docs_json_with_warnings` now produces warnings (checklist
exported as bullet, dropped line-spacing rule, block formatting on image and
equation blocks) and nothing consumes them.

The obvious fix — calling `push_model_warning` from the export path — is
**wrong**: that mutates `document.warnings`, which is signed source state, so a
read-only export would dirty the document and change its signature.

The correct shapes are (a) carry warnings in the command result, which changes
`export_google_docs_json`'s return from `Text` to a structured result and
ripples through the generated contract and `main.ts`, or (b) surface them as
projection-only warnings like the equation renderer's, which needs a place to
hang warnings that belong to a one-shot command rather than to the document.
(a) is probably right. Deliberately not decided in an unattended run.

### 2026-09-11 (wave 3)

- **G4 done (ED-24, ED-25).** Find and replace moved out of `main.ts` into
  `crates/opendoc-app/src/find.rs`, behind three commands:
  `find_in_document` (returns `AppFindMatches`), `replace_match_in_document`
  and `replace_all_in_document`. All three take the same
  `{query, matchCase, wholeWord, regex}` payload, parsed once by
  `find_options_arg`, so a replace can never search differently from the find
  that listed its targets.
  - **Every search is one compiled `Regex`.** A literal query is
    `regex::escape`d rather than matched by a second code path, so literal and
    regex modes cannot drift apart on case folding or word boundaries, and an
    escaped literal can never fail to compile. Whole word wraps the pattern in
    `\b(?:…)\b`; match case selects `case_insensitive`, which gives Unicode
    folding instead of the old ASCII-ish `toLowerCase()`.
  - **ED-25:** the haystack is the block's whole visible text
    (`DocumentIndex::text_of`), not one inline run, so a query straddling a
    formatting boundary is found. Each hit is mapped back through
    `DocumentIndex::position` into the `{block_id, inline_id, offset}` pair the
    editor already uses, so a match is returned as a selection and the frontend
    does no arithmetic.
  - **Bytes vs characters** is the failure mode this kind of port usually
    ships. `regex` reports byte offsets; the selection model counts scalar
    values. The conversion happens in exactly one place
    (`BlockMatch::from_byte_range`) and is pinned by a test over
    `"héllo wörld ☃ héllo"` whose expected offsets differ from the byte
    offsets.
  - **Replace-all is one dispatch**, so it is one undo checkpoint however many
    occurrences it rewrites — the dispatcher checkpoints per command, and the
    replacements are planned against a single pre-edit snapshot and applied as
    one `apply_batch`, last match first.
  - A user-supplied pattern cannot hang the app (the `regex` crate does not
    backtrack) and cannot panic: compilation failure, including the size and
    DFA-size caps, becomes `AppApiError::Format`. Because a half-typed regex is
    invalid on the way to a valid one, the find bar reports it in its own
    counter instead of raising the app error banner.
  - `main.ts` keeps the bar's DOM only: query box, three option toggles,
    counter, highlight, next/previous, and two buttons that dispatch a replace.
    `findMatches` is deleted.

### 2026-09-11 (wave 3) — D2 done: DOCX export (FS-22)

`crates/opendoc-import/src/docx_write.rs` is the mirror of the reader in
`docx.rs`: it writes the whole package — `[Content_Types].xml`, package and
part relationships, `word/document.xml`, `styles.xml`, `numbering.xml`,
`footnotes.xml`, `header1.xml`/`footer1.xml`, `docProps/core.xml` and the
image parts — reachable through `export_docx`, implemented behind the facade
as `OpenDocApp::export_docx_base64`, and wired into File ▸ Download as Word.

- **Every construct written is one the reader can read.** That is the point of
  the correspondence: the strongest available check is exporting and
  re-importing through a parser with 113 tests already behind it, so the
  writer never invents a shape the reader would have to learn. 30 new tests
  (113 → 143 in `opendoc-import`) plus 3 in `opendoc-app`.
- **Twips map 1:1.** DOCX and `Length` share the unit, so every indent and
  spacing value is the integer the model holds — asserted over values like 19,
  241 and 31679 that no conversion through points or pixels could reproduce.
  Line spacing is the one unavoidable conversion (thousandths → 240ths); it is
  checked for exactness and warns when it has to round.
- **Nothing is dropped silently**, matching the rule the importer already
  follows: 24 warning codes, one per thing WordprocessingML cannot carry
  exactly. Checklists become a bulleted list with ballot-box glyphs (and a
  ticked run and an unticked run become two numbering definitions, because a
  bullet glyph is a property of a definition, not of an item); code spans
  become a monospace character style; equations become their source inside an
  Office Math zone; comments, suggestions and the DOI have no home at all.
- **Headings keep the one asymmetry that cannot be designed away.**
  WordprocessingML says "this is a heading" only through a style, and a style
  with no character formatting produces a document that does not look like it
  has headings — so the heading styles carry bold and a size, and the reader
  faithfully turns that formatting back into marks. The round-trip test states
  the expected result rather than relaxing the comparison. The reverse choice
  was made for hyperlinks: a hyperlink works without the `Hyperlink` style, so
  no colour or underline the model never asked for is invented.
- **Page geometry rides B7's model.** `w:sectPr` carries `PageSetup` (twips to
  twips, `w:orient` derived from the dimensions), and `Document::header` /
  `Document::footer` become real header and footer parts with `PAGE` /
  `NUMPAGES` fields. The reader does not yet read any of that back — it still
  counts `docx-dropped-section-properties` and `docx-dropped-header-footer` —
  so this is the one direction that is currently one-way.
- **Verification beyond the round trip.** Every WordprocessingML part
  validates against the ECMA-376 / ISO-IEC 29500-4 transitional `wml.xsd`
  under `xmllint`; pandoc and LibreOffice — two independent OOXML readers —
  both read the package completely, and LibreOffice's PDF render shows the
  headings, hanging indent, justification, RTL run, every mark, the nested and
  numbered lists, the ballot boxes, the table, the typeset equation, the
  footnote, the repeated header and a footer whose page numbers it recomputed.
  Word itself could not be run here.
- **The export is deterministic**: `zip` is built without its `time` feature,
  so `SimpleFileOptions::default()` stamps 1980-01-01 rather than reading a
  clock — the same document exports to the same bytes, and nothing traps on
  wasm32 the way `rust_xlsxwriter`'s clock call did.

**Left undone, deliberately.** Export warnings still have nowhere to go: the
library returns them from `export_docx_with_warnings`, and the command drops
them exactly as `export_google_docs_json` does, because fixing that is the
open decision recorded above (change the command result shape) and not one to
take unattended. Reading `w:sectPr`, headers and footers back is the natural
follow-up now that the model exists. A blank paragraph exports correctly as
`<w:p/>` and is then dropped by the reader, which discards a paragraph with no
inline content — a reader gap, not an export one.

### 2026-09-11 (wave 3)

- **F3 done — the browser persists.** ADR 0008. IndexedDB is asynchronous and
  `dispatch_command` is synchronous, and neither could move: an async
  `ObjectStore` would colour every command in the app to serve one runtime, and
  a browser's main thread cannot block on a transaction at all. So the
  synchronous side stopped being on the I/O path instead.
  `opendoc-store::MirroredVolume` is a flat in-memory `key -> bytes` volume that
  answers every read and write immediately and records each mutation, in
  sequence, for a driver to drain; `crates/opendoc-wasm/src/storage.rs` drains
  it into IndexedDB one batch per `readwrite` transaction, never two at once,
  and reports back a `durable_seq` watermark. Because a batch is always a
  *prefix* of the mutation sequence and the commit path writes objects before
  it swaps the head, a durable head can never name objects that are not durable
  — the one thing content addressing does not make safe by itself.
  `MirroredObjectStore` passes `verify_object_store_contract` unchanged, and on
  `wasm32` the *local* repository backend resolves to the volume, so
  `save_local_repository`, `open_local_repository`, `scan_local_repository` and
  autosave work in the browser with **no new commands and no change to
  `main.ts`**. `VolumeRecoveryJournalStore` closes ADR 0005 §6: the browser now
  has the same crash protection as the Tauri shell, with one IndexedDB key per
  journal frame so a per-keystroke append does not rewrite the segment's base
  snapshot. The IndexedDB binding is Rust (`web-sys`); the entire TypeScript
  side is one `await module.storage_ready()` in `invoke.ts`. Verified in real
  Chrome: a saved document survives a page reload, and so does unsaved work —
  the crash-recovery offer comes back after a reload and replays.
- **`cdp.mjs` now accepts native dialogs.** `Page.enable` makes Chrome hand
  every JavaScript dialog to its debugging client instead of auto-dismissing
  it, so the unsaved-changes `beforeunload` prompt stalled `Page.navigate`
  forever with no error — any e2e check that navigates away from a dirty
  document would have hung. A process that actually crashed never runs
  `beforeunload` anyway.

### 2026-09-11 (wave 3 complete)

Full gate green: **662 tests passing / 0 failing** (391 → 493 → 567 → 662),
clippy `-D warnings` clean, fmt clean, no contract drift, typecheck clean,
smoke passing, **e2e passing 20/20 in real Chrome**.

- **B7 done.** `PageSetup` on `Document` (twips), header/footer block
  containers, `Inline::PageNumber` as a derived field. Orientation is
  **derived, never stored**; a named size is not stored either — the
  dimensions are, and `size_name()` recovers "a4" by measuring. Body, header
  and footer share one id space, enforced by `validate()`.
- **B8 done as measured pagination** — ADR 0009. Page *geometry* is a document
  fact and lives in Rust; page *breaking* is a function of shaped text, so it
  is computed in the browser and applied as **decoration over one continuous
  contenteditable flow**, never by restructuring the document. A Rust layout
  pass was rejected: it means either a second text-shaping engine that must
  agree with the browser's, or a measurement oracle that would make
  `render_document` non-deterministic. `opendoc-render` stays pure — it emits
  furniture once and page-number fields empty.
  **Not achieved, not faked:** breaking inside a block, widows/orphans,
  per-page footnotes, sections/columns. Repeated headers in *printed* output
  is unverified — the harness drives the screen, not `Page.printToPDF`.
- **D2 done.** DOCX writer (1,892 lines) round-tripping through the existing
  reader, schema-validated against ECMA-376 with `xmllint`, and read back by
  two independent implementations (pandoc; LibreOffice rendered it to PDF and
  recomputed the footer's PAGE/NUMPAGES fields). **Microsoft Word itself could
  not be tested here** — that residual risk is real.
- **F3 done.** IndexedDB persistence and browser crash recovery — ADR 0008.
  `MirroredVolume` answers reads and writes synchronously from memory while an
  async Rust/`web-sys` driver drains an ordered pending list; durability is an
  explicit watermark. Head safety is the crux: pending is always a strict
  prefix, objects are written before the head swaps, so a durable head cannot
  name non-durable objects. Zero new commands, zero `main.ts` changes.
- **G4 done (ED-24, ED-25).** Find & replace moved to Rust; `findMatches` and
  its code-point arithmetic deleted. A match spanning two inline runs is now
  found — searching "lo wo" in a half-bolded "hello world" previously returned
  nothing, silently. Byte→character conversion happens in exactly one place.

### Carried forward

- **Export warnings still have nowhere correct to go.** Now three producers
  (Google JSON, DOCX, equations) and one open decision, unchanged: the obvious
  `push_model_warning` fix is wrong because it mutates signed source state.
- **Header/footer blocks are not reachable by block-addressed operations** —
  needs every `find_block_mut` caller taught about the two extra roots.
  Contained, not done. Page-setup merge is LWW on the *whole* `PageSetup`,
  deliberately coarser than ADR 0006, so one actor's A4 width cannot combine
  with another's Legal height.
- **The DOCX reader does not read `w:sectPr`/headers/footers back** — that
  direction is one-way now that the model exists.
- **Two browser tabs on one origin clobber each other** (each has its own
  volume, last flush wins). Needs Web Locks; belongs with F2.
- **Concurrency cost, measured.** Three agents sharing one tree cost two
  agents 20-35 minute e2e hangs from `dist/` rebuilds mid-run, and a killed
  e2e run orphans its Chrome, which the next run silently attaches to.
  Future waves: serialise e2e, or give each agent its own port and profile.

- **E2 done (table structure, OB-21).** A table is a **rectangular grid**:
  `BlockKind::Table { columns: Vec<TableColumn>, rows }`, every row one cell
  per column, enforced by `Document::validate()` so a ragged or overlapping
  grid cannot be decoded in. Columns carry an identity (what column
  operations anchor on, exactly as row operations anchor on a row id) and an
  optional `Length` width in twips. A merged cell is a `CellSpan` on the cell
  it starts at; the cells it covers stay in the grid and keep their content,
  so splitting is the exact inverse of merging and coverage is *derived*, not
  stored. Cell styling is `TableCellProperties` — background, four border
  edges, vertical alignment, four padding edges — typed with smart
  constructors, the shape ADR 0006 set for block properties, not a string bag.
  Six operations (`InsertTableColumn`, `DeleteTableColumn`,
  `SetTableColumnWidth`, `SetTableCellSpan`, `Set`/`ClearTableCellProperty`)
  and eleven commands. Merge converges by identity for structure, whole-value
  LWW per column width and per cell span, and LWW per property for styling,
  with a deterministic `repair_table_geometry` pass for the pairs that cannot
  both apply as written — recorded in
  `docs/adr/0013-table-structure-and-merge.md`. Merge also gained
  `opendoc_core::derived_stable_id`, which fixed a live convergence bug: the
  placeholders merge pushed when the last row or cell of a table was deleted
  used `StableId::new`, so two replicas deleting the same row converged to
  documents that differed in those ids.
- **Still to do for E2:** row heights, header-row repeat, whole-table
  alignment and table sort (all OB-21 umbrella items); multi-cell selection,
  so "merge cells" still asks for a span in a dialog rather than reading a
  selected rectangle; and inserting a row or column *before the first* one —
  `after: None` means "append" throughout this codebase and forking that
  convention for tables alone was not worth it under one slice.

### Correction 2026-09-11: pagination belongs in Rust (supersedes ADR 0009)

The user overrode ADR 0009. Pagination must live in Rust so **one**
implementation serves the browser, the desktop shell and headless/client-side
use. ADR 0009 weighed "a Rust layout pass means a second engine that must
agree with the browser's" but did not weigh the requirement that pagination
run outside a browser at all. With the engine in Rust there is only one
engine, and the browser places what it decided.

Consequences:
- New crate `crates/opendoc-layout` owns page breaking; `paginate()` in
  `main.ts` is replaced by consumption of the Rust result, not kept as a
  fallback.
- **The font loop must be closed.** Deterministic Rust layout needs real font
  metrics; if the browser then renders with a different font, the screen
  disagrees with the pagination and the mismatch returns by another route.
  That means bundling a subsetted font and rendering through `@font-face` —
  the same asset-pipeline question deferred earlier today for math fonts, now
  forced.
- **D1 (PDF) is re-scoped.** The platform print pipeline is no longer the
  answer; a Rust PDF writer built on the layout engine will agree with the
  screen. The D1 agent was redirected mid-flight to drop PDF and finish export
  warnings and the DOCX page-setup read-back instead.

### 2026-09-11 (wave 4): export warnings and the DOCX page read-back

- **Export warnings now have somewhere to go — ADR 0010.** Shape (a) of the
  two candidates: the warnings ride the *command result*. `export_*` returns a
  new `AppExport { content, encoding, media_type, file_extension, warnings }`
  instead of `string`; `AppCommandResult` gains an `Export` variant,
  `CommandReturn` gains `AppExport`, and the generator projects it to
  `src/generated/export.ts` like every other DTO.
  Shape (b) — projecting them like the equation renderer's — was rejected with
  a reason: a projection warning is recomputed on every `get_document` because
  it is still true next time, whereas an export warning describes *one*
  command's output, and hanging it on the document projection would mean
  running the DOCX writer on every keystroke or keeping "the last export said"
  in app state, which is the same mutable side effect in a different field.
  The whole export path is now `&self`, so the borrow checker enforces what
  the ADR argues. `main.ts` lost its table of "which export is base64 and what
  media type it means"; the format states its own extension and media type.
  Warnings surface in the warnings panel under their own heading, as view
  state, never merged into `doc.warnings`.
  Pinned by `exporting_a_signed_document_changes_neither_its_bytes_nor_its_signature`:
  a signed document with a checklist is exported three ways, the exports are
  asserted to *have* warnings (so the test cannot pass by exporting something
  with nothing to report), and the canonical snapshot payload, the document's
  own warnings, the dirty flag and the signature are all unchanged afterwards.
  `export_spreadsheet_csv`/`export_spreadsheet_xlsx` still return `string` —
  another agent's files this wave; converting them is mechanical.
- **The DOCX round trip is no longer one-way for the page.** `docx.rs` reads
  `w:sectPr` into `PageSetup` (twips to twips, identity mapping) and the
  `w:headerReference`/`w:footerReference` parts into `Document::header` /
  `footer`. `w:orient` is deliberately *ignored*: orientation is derived from
  the dimensions (ADR 0009 §1, which the pagination correction above does not
  touch) and WordprocessingML already writes `w:w`/`w:h` swapped, so honouring
  both would turn the page twice.
  `PAGE`/`NUMPAGES` fields come back as `Inline::PageNumber` in both the
  three-run `w:fldChar` form and the one-element `w:fldSimple` form, and the
  cached result Word writes between `separate` and `end` is swallowed —
  importing it would freeze a computed field into a number that is wrong on
  every page but the first.
  Degradations are named, not silent: a first-page or even/odd variant warns
  (`docx-dropped-header-footer`), geometry outside what the model can hold
  keeps the default and warns (`docx-invalid-page-setup`), multiple columns
  and extra sections warn (`docx-dropped-section-properties`), and a page
  break or footnote reference inside furniture is stripped with a warning
  (`docx-dropped-header-footer-content`) rather than failing the whole import
  on `Document::validate()`.
- **A real layout bug found on the way.** `.doc-body` carried a flat
  `min-height: 800px`, which is taller than the content box of any page under
  about nine inches, so on a short page the editable host hung below the page
  it belongs to. It is now `var(--page-content-height)`. Guarded by an e2e
  check that measures the computed minimum against the page's own content box.
- **PDF (D1) discarded, deliberately.** Work on the platform print pipeline was
  stopped mid-flight by the correction above and removed entirely: no
  `print_to_pdf` Tauri command, no `gtk`/`webkit2gtk` dependency, no menu
  entry, no ADR defending it, and no half-wired command in the contract. Two
  findings from that work are worth keeping for whoever builds the Rust
  engine, because they were measured rather than assumed:
  Chrome's `Page.printToPDF` **does** dispatch `beforeprint`/`afterprint`, and
  Chrome **does** repeat absolutely positioned page decoration on the printed
  page its offset lands on — a three-page document printed `ChapterMark 1/2/3`
  on pages 1/2/3, which answers ADR 0009's open question in the affirmative
  for Chrome even though the mechanism is being replaced.

### 2026-09-11 (wave 4) — B8 redone: pagination in Rust

**ADR 0014 supersedes ADR 0009's Decision 3.** Page breaking no longer happens
in the browser. `crates/opendoc-layout` computes it from the `Document` and its
`PageSetup` with no host measurement and no floating point; `layout_document`
carries the answer out; `main.ts` places what it is told and `paginate()`'s
measurement code is deleted rather than kept as a fallback.

ADR 0009 was not wrong about the danger — two layout engines that disagree — it
was weighed without one requirement: pagination has to run browser-side *and*
client/headless-side from one implementation, which a browser-only paginator
cannot do. The answer is that there is one engine, in Rust, and the browser
renders what it decided.

- **The font loop is the whole problem, and it is closed three ways.** One
  subset of Liberation (four sans faces plus a mono, renamed "OpenDoc Sans" /
  "OpenDoc Mono" because the OFL reserves the upstream name) is written twice
  out of a single generator run: 148 KB of TrueType embedded in the crate, 71.7
  KB of WOFF2 loaded by the stylesheet, with the same `hmtx` advances by
  construction. The subsets carry **no GSUB, GPOS or kern**, so a run's width
  *is* the sum of its advances and Chrome cannot apply shaping the crate did
  not account for. And the type scale is projected as `--doc-*` custom
  properties by the crate that measures with it, so an `h1` cannot be 20pt in
  Rust and 24pt on screen.
- **`ttf-parser` 0.25, not `cosmic-text`/`rustybuzz`/`swash`.** All build for
  `wasm32-unknown-unknown` (checked first, before any code — the
  `rust_xlsxwriter` clock trap was recent enough). Full shaping is only worth
  having if the browser is shaping too, and here it deliberately is not.
- **No floating point anywhere.** Line widths accumulate as
  `advance_units * font_size_twips` against `available_twips * units_per_em`;
  nothing divides, so nothing rounds. Vertical lengths are milli-twips with one
  rounding step at the output boundary.
- **The gutter stayed a view concern.** Rust describes pages that touch; the
  frontend adds `var(--page-gap)` inside a `calc()`. Printing therefore needs no
  code at all — the print stylesheet zeroes that property — and the
  `beforeprint`/`afterprint` re-pagination listeners are gone.
- **Exact:** paragraphs, headings, indents (including hanging), line spacing,
  space before/after with CSS margin collapsing, list runs with their nesting
  and checkboxes, bold/italic/code runs, explicit page breaks.
  **Estimated and reported as such** (`exact: false`, which propagates to every
  block after it): tables, images with no stated height, equations, text outside
  the bundled subset, superscripts (their width is measured; the taller line box
  is not), and documents carrying suggestions.
  **Not attempted:** justification, hyphenation, widows/orphans, floats,
  columns. A block taller than the content box still takes a page and overflows,
  as before.
- **The check that matters is in Chrome**, not in Rust: every block's rendered
  box must lie inside the content box of the page Rust assigned it, *and* the
  block opening a page must not have fitted at the bottom of the previous one.
  Mutating the crate's line budget by ±15% breaks both directions of that
  assertion.


### 2026-09-11 (wave 4 complete)

Full gate green: **761 tests passing / 0 failing** (391 → 493 → 567 → 662 →
761), clippy `-D warnings` clean, fmt clean, no contract drift, typecheck
clean, smoke passing, **e2e 32/32 in real Chrome**.

- **Pagination now runs in Rust** — ADR 0014 supersedes ADR 0009 Decision 3.
  `crates/opendoc-layout` does line breaking, block flow and page breaking
  against a bundled font subset; `paginate()`'s measurement in `main.ts` is
  **deleted**, not kept as a fallback. The frontend places what Rust decided.
  The font loop is closed structurally rather than by hope: one subset shipped
  twice from the same in-memory build (160 KB TTF the crate `include_bytes!`s,
  80 KB WOFF2 the stylesheet loads, identical `hmtx` by construction), the
  subsets carry **no GSUB/GPOS/kern** so a run's width *is* the sum of its
  advances and Chrome cannot apply a kern the crate did not account for, and
  the type scale is projected as CSS custom properties so an `h1` cannot be
  20pt in Rust and 24pt on screen. No floating point: widths accumulate in
  font units, vertical lengths in milli-twips with one rounding at the edge.
  Layout honestly reports `exact: false` for tables, unsized images,
  equations and out-of-subset text, propagating to every block after.
  Left out deliberately: justification, hyphenation, widows/orphans, floats,
  columns, full UAX #14. **Documents now draw in the bundled font rather than
  the platform's Arial — a visible change, and the point.**
- **E2 tables done** — ADR 0013. Merged cells are spans on the origin cell
  with coverage *derived*, never a stored flag, so splitting is the exact
  inverse of merging and DOCX `vMerge` maps 1:1. Convergence is by identity
  for structure, whole-value LWW per column width and per cell span, per
  property for cell styling. Fixed a **live convergence bug**: merge's
  placeholders used `StableId::new`, so two replicas deleting the same row
  converged to documents differing in those ids.
- **E3 images done** — ADR 0012. Typed sizing in twips, axes independent,
  `None` means intrinsic and is never materialised. Insert-from-URL
  deliberately **not** built: fetching is network I/O with policy attached and
  belongs in the Tauri shell, not the local-first WASM core.
- **Export warnings solved** — ADR 0010. `export_*` returns `AppExport
  { content, encoding, media_type, file_extension, warnings }`. Shape (b) was
  rejected with a reason: a projection warning is recomputed because it is
  still true next time, but an export warning describes one command's output.
  The export path is now `&self`, so the borrow checker enforces what the ADR
  argues. Proven by a test that signs, exports three ways, asserts the exports
  actually carry warnings, then asserts byte-identical payload and a still
  valid signature.
- **DOCX page setup round-trips**; `w:orient` deliberately ignored because
  orientation is derived and WordprocessingML already writes `w:w`/`w:h`
  swapped.
- **PDF (D1) reverted cleanly** after the pagination correction — no command
  ever reached the contract, no GTK/WebKit dependency left behind. Two
  measured findings kept for the Rust PDF work: Chrome's `printToPDF` does
  dispatch `beforeprint`/`afterprint`, and it does repeat absolutely
  positioned page decoration onto the printed page.

### The next priority is G3, and it is now structural

`apps/desktop/src/main.ts` is **3,943 lines** (1,871 this morning). It is the
file every agent must touch, and this wave it was corrupted by a botched
concurrent edit (`function exportStyles(): string {function exportStyles…`),
while `scripts/e2e.mjs` was replaced wholesale and lost two agents' checks.
Splitting it by surface is no longer tidy-up; it is the ceiling on doing any
more work in parallel.

Also carried forward: PDF via a Rust writer on top of `opendoc-layout`;
layout is not incremental (25 ms for 100 pages, re-run per keystroke);
insert-from-URL in the Tauri shell; table row/column insert-before-first;
multi-cell selection for merge.

### 2026-09-11 (wave 5: structural debt cleared)

Gate green and unchanged through two large pure refactors: **761 tests / 0
failing**, clippy `-D warnings` clean, fmt clean, no contract drift,
typecheck clean, smoke passing, **e2e 32/32**.

- **G3 done.** `apps/desktop/src/main.ts` **3,936 → 177 lines**, split into 17
  flat modules by surface. Each module owns its state privately (find query,
  spreadsheet cursor and clipboard, table cell context, version view, export
  report); only genuinely shared view state lives in `state.ts`. The
  dispatcher routes surface-owned action names through per-surface handlers
  that return `false` for names they do not own, so the unknown-action branch
  stays honest. Modules are deliberately **flat**: `build.mjs`'s import-rewrite
  regex only matches `./…`, so a nested module's `../invoke` import would have
  silently shipped an extension-less specifier.
  Proof it changed nothing: e2e 32/32 before and after with **byte-identical
  check lists**, all 105 action labels and 11 prefixes confirmed present with
  none added or duplicated, and `addEventListener` sites counted 30 before and
  30 after — which directly guards the listener-stacking bug class.
- **G5 done.** **No file under `crates/**` exceeds 2,000 lines.**
  `opendoc-merge/src/lib.rs` went 14,364 → 61, continuing the existing
  `causal.rs`/`text_sequence.rs` direction into per-domain rule modules; the
  other eleven likewise. Per-crate test counts are identical to baseline
  (merge 185, core 52, store 53, import 148, render 33, api 8, app 141), and
  the generated contract is byte-identical after splitting `commands.rs`.
  **Roughly 60% of the twelve files' bulk was inline test code** appended to
  crate roots — `opendoc-merge/src/lib.rs` was 4,279 lines of implementation
  under 10,085 lines of tests.
  Near-miss worth recording: `cargo fix` silently deleted three renderer
  re-exports as "unused imports"; a reachability check over every `pub` item
  caught it.

### Document semantics still in TypeScript — the next port list

Found by reading every line of `main.ts` during the split, now concrete:

1. `export-html` / `export-text` build their payload in TS with a hand-written
   stylesheet while every other export goes through a Rust command — no
   warnings, no Rust-owned media type, **contradicting ADR 0010**. Best
   candidate.
2. Image size limits hard-coded (`22 * 1440` twips) duplicating Rust
   validation, when the axis sizes beside them already import from the
   generated bindings.
3. `TWIPS_PER_INCH = 1440` and `twips / 20` restated across four modules.
4. The `"multiple:1500"` line-spacing wire format parsed and rebuilt by hand.
5. `contextForCell` derives table geometry from DOM position, not the model.
6. `previewBlockText` implements a projection rule.
