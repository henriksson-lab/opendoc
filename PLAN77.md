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
2. ~~Image size limits hard-coded (`22 * 1440` twips)~~ — closed 2026-09-12.
   `ImageLayout::MIN_TWIPS`/`MAX_TWIPS` are now model validation, exported as
   `APP_MIN_IMAGE_TWIPS`/`APP_MAX_IMAGE_TWIPS`.
3. ~~`TWIPS_PER_INCH = 1440` and `twips / 20` restated across four modules~~ —
   closed 2026-09-12. `Length::TWIPS_PER_POINT`/`TWIPS_PER_INCH` are public and
   exported as `APP_TWIPS_PER_POINT`/`APP_TWIPS_PER_INCH`.
4. ~~The `"multiple:1500"` line-spacing wire format parsed and rebuilt by
   hand~~ — closed 2026-09-12. `LineSpacing::{PRESETS, mode, value, parse,
   label}` own the encoding and the wording; the contract exports
   `APP_LINE_SPACING_PRESETS` and the block DTO carries `line_spacing_label`,
   so the `<select>` option values are indexes and nothing splits a string.
5. ~~`contextForCell` derives table geometry from DOM position~~ — closed
   2026-09-12. Indices, counts and previous-sibling ids come from the block
   projection; the DOM supplies only *which* cell the caret is in.
6. ~~`previewBlockText` implements a projection rule~~ — closed 2026-09-12.
   `AppVersionPreview.blocks` carries kind and text, built with the same
   `version_diff::block_text` the diff uses.

Items 2-6 are done; item 1 (HTML/text export) was taken by the export wave.

### Queued: hidden spreadsheet axes are suppressed in TypeScript

The SH-31 agent implemented row/column hiding but could not put the
suppression where it belongs: `opendoc-render` was another agent's this wave,
so `render_workbook_html` still emits hidden rows/columns and `spreadsheet.ts`
removes them after each morph. Two consequences it flagged: TS composes
`column + row` into an address to find a hidden column's cells, and it removes
the hidden column's `<col>` to keep widths aligned.

This is a **new** TS-semantics violation, introduced by ownership constraints
in the same wave that closed five others. `render_workbook_html` should skip
hidden axes; move it there as soon as `opendoc-render` is free.

Also flagged and left alone: `mutate_spreadsheet` is not transactional, so a
range running past the last row would half-apply. The hide commands pre-check
every axis first; `set_spreadsheet_selection_format` has the same latent
behaviour and was not touched.

### F2 landed — and the collaborative loop is open at the client end

`crates/opendoc-service` exists (ADR 0015 amends, does not supersede, ADR 0004
— everything 0004 says about what the crate may *own* still binds). 55 tests,
clippy and fmt clean, **not in the WASM dependency graph**, and deliberately
**no `opendoc-app` dependency**: it composes core/format/merge/store directly,
so a network daemon cannot pull in layout, render, import or spreadsheet. The
dependency list is the tripwire if semantics ever creep in.

Why the gate is met now: before ADR 0007, concurrent character edits converged
to *wrong text* — every replica agreed on a corruption, so the suite was
green. A server on that would have replicated the corruption and made it
durable, which is worse than no server.

**Enforced from server state:** identity, authorship (an operation's actor must
be the one bound to the subject; no two subjects share an `ActorId`),
permissions re-read from durable storage on every submit (a revoked editor's
next keystroke is refused and live sockets close), sequence density, causal
honesty (an operation may not claim to observe what the log lacks, nor a
Lamport timestamp above one past what it observed — without that a client sets
`lamport: u64::MAX` and wins every LWW contest for ever), history
immutability, durability before acknowledgement, and non-leakage of existence.

**Still trusted:** operation payloads are not semantically authorized, comment
author fields, `context: None`, cursor anchors, and the token in the WebSocket
URL (browsers cannot set headers on a WS handshake).

Design calls worth keeping: one OS thread per document rather than a mutex, so
"two clients cannot interleave into an invalid head" is structural; and
re-merge from genesis on every commit, required by ADR 0007's stated limit —
materialising against the previous commit's output would re-anchor concurrent
operations positionally and reintroduce the corruption 0007 removed. Cost is
O(commits²) over a document's life, stated rather than hidden.

#### Blockers before this carries real users

1. ~~**`OpenDocApp` cannot ingest an operation it did not author.**~~ **Closed.**
   `crates/opendoc-app/src/collaboration.rs` adds `join_collaboration_session`
   and `apply_remote_operations`. Intake **re-merges from the session's merge
   base**, never folds a commit into the previous result — ADR 0007's stated
   limit is the reason the server does the same, and a mutation that folds
   incrementally fails the convergence test at round 0. Remote operations become
   ordinary journal envelopes (so the next local operation's causal context
   observes them, so a save writes a replayable history, so the ADR 0005
   recovery segment still replays to the in-memory state); they **drop the undo
   and redo stacks** (restoring a whole-state checkpoint taken before a remote
   edit would delete a collaborator's work while the service still holds it —
   collaborative undo needs inverse operations, which is a feature, not a fix);
   and they **do not make the document locally dirty** (durable on the service
   before acknowledgement), which is why `saved_operation_count` — what a save
   writes from — and `settled_operation_count` — what the dirty flag measures —
   are now two numbers.
2. ~~**`AppOperationEnvelope` is `pub(crate)`**~~ **Closed, from the other side.**
   The type, `AppDocument::from_core`/`to_core` and the envelope payload enums
   are public. But the service still writes `opendoc.service-document.v0`
   carrying `opendoc_core::Document`, because making it write the app's DTO
   needs `opendoc-app` in its `[dependencies]` — ADR 0004's prohibition, and it
   would drag layout, render, import, pdf and spreadsheet into a network daemon.
   Instead **`opendoc-app` reads the service's format**: the record envelope,
   manifest chain, segment chaining and head compare-and-swap are already
   identical, so only the two payload types differ, and the translation lives on
   the app side where it costs the service nothing.
   `a_repository_the_service_wrote_opens_in_the_app` opens one, checks it
   byte-identical to the server's document, then edits and saves back into the
   same chain.
3. **Contract changes `opendoc-api` needs** (reported, not made):
   `authorize_runtime_command` takes the caller's own `grants` — that argument
   *is* the problem, since it makes the client assert its own permissions;
   `deferred_operations` has no meaning in the service protocol;
   `OpenDocPresencePeer` lacks `actor`/`connections`; and
   `OpenDocRuntimeMode::MultiUserService` should mean "permissions are answers
   from the service", not "evaluated locally".

#### What still stands between this and a user-visible collaborative session

- **No transport in a shell.** `opendoc-service` ships a client, but nothing in
  `main.ts` or `src-tauri` opens a socket. The app side is a Rust API; a shell
  has to drive it.
- **`next_seq` is one counter for two things.** Rich-document operations and
  the envelopes that carry no operation (undo markers, blob and spreadsheet
  work) share it, so any of the latter leaves a hole in the document
  operations' numbering and the service refuses the gap.
  `local_operations_are_dense_after` reports it rather than letting a
  transport discover it as a rejection; fixing it means separating envelope
  identity from operation identity, which reaches the journal, the recovery
  segment and the repository together.
- **Undo is not collaborative.** Intake drops the stacks. Undo that survives
  someone else typing has to be new inverse operations submitted like any
  other edit.
- **The caret rebase is best-effort inside one run.** `{block_id, inline_id,
  offset}` survives a remote edit in another block or another run untouched;
  inside the caret's own run the offset is moved by a prefix/suffix diff of the
  materialised text. Exact placement would need the caret anchored to a
  character identity, and ADR 0007 does not persist those.
- **No spreadsheet or blob operations on the wire** (ADR 0015), so a
  collaborative session covers rich text only.
- **Presence is not wired to the app.** `PeerView` reaches a client; nothing
  renders it.

### 2026-09-12 (wave 6) — D1 done: PDF export, and the last two exports move to Rust

- **D1 done (FS-21), and this time on the right foundation — ADR 0016.** The
  earlier attempt drove the platform print pipeline and was reverted when
  pagination moved into Rust. `crates/opendoc-pdf` now writes the PDF from the
  **layout result**, not from the document: `opendoc-layout` gained a painting
  mode that runs the *same* pass and records what it already decided, so the
  PDF and the screen break in the same places by construction. Nothing in the
  PDF crate measures text, breaks a line or picks a page.
  - **The recording lives inside the line breaker, not beside it.**
    `text::Breaker` gained an `Option<Capture>` whose every method mirrors one
    width operation — a piece recorded when its advance is committed, a line
    emitted when the accumulator wraps, hanging spaces dropped from the record
    at the moment they are dropped from the width. `measure` and `break_lines`
    are one function with the capture off and on, and a test asserts they
    break identically across five page widths. Re-breaking the text in the PDF
    writer would have been the second engine ADR 0009 was right to fear, one
    level further down.
  - **`pdf-writer` 0.15**, chosen after building a probe crate for
    `wasm32-unknown-unknown` *before* writing any code — four pure-Rust
    dependencies, no filesystem, no clock, no C. `lopdf` was rejected for its
    `time`/`chrono` dependency (the `rust_xlsxwriter` trap in a different
    shape); `printpdf`/`krilla` for pulling in image and font machinery this
    crate does not want, because the fonts are already chosen and subset.
    Accepted cost: no deflate, so streams are uncompressed and a typical
    export is a few hundred KB.
  - **The embedded faces are literally the ones the pagination measured with**
    — `FaceId::bytes()`, the same TrueType the crate `include_bytes!`s and the
    same subset the stylesheet serves as WOFF2. Type0 / Identity-H /
    CIDFontType2, because the subset is 452 glyphs and a simple font addresses
    256. Identity-H costs extractable text unless a `ToUnicode` CMap ships
    with it, so every face carries one and a test asserts one per font; only
    the faces a document actually uses are embedded.
  - **Estimates are now reasons, not a boolean.** `BlockPlacement::exact` said
    *that* a block was guessed; `EstimateReason` says what was guessed, and it
    is attached to the block whose own geometry was estimated rather than to
    every block after it. The PDF turns each into an ADR 0010 warning naming
    the block and the cause. A document of measured blocks exports with no
    warnings at all, which is what makes the warnings worth reading.
  - **Not drawn, and said so:** images (a frame of the right size with the alt
    text — there is no image decoder here), block equations (their LaTeX
    source, which ADR 0003 makes canonical anyway), merged cells (as the cells
    they cover, which is also what the layout measures), comments and
    suggestions, and bidi reordering. A nested bullet's hollow circle is not
    in the bundled subset, so that depth falls back to the disc rather than
    printing `.notdef` — decided in the layout where it can be tested.
  - **Verified against something other than the writer:** `pdfinfo` (4 pages,
    612x792 Letter, the document's title), `pdffonts` (four used faces,
    embedded CID TrueType, Identity-H, Unicode maps, and only the four),
    `pdftotext -f N -l N` (right text, right page, right order, including
    `café`/`Łódź`/`“quotes”`/`€100`/`≤`, list numbering, table cells in column
    order, the header on all four pages and `Page N of 4` on each), and
    `pdftoppm` for a visual pass. In real Chrome the e2e suite captures the
    PDF the File menu actually saves and asserts its `/MediaBox` equals the
    sheet Chrome laid out (a CSS pixel is exactly 0.75pt) and its page count
    equals the sheets Chrome drew. **Not verified:** Acrobat, Preview, Windows
    print drivers, and any real printer.
- **`export-html` and `export-text` are Rust commands now.** They were the last
  two exports built in TypeScript — a hand-written stylesheet in `files.ts`,
  the body pasted out of `doc.body_html`, no warnings and a media type the
  frontend decided, all of which contradicted ADR 0010 hours after it landed.
  `opendoc-render::render_standalone_html` wraps the renderer's own markup, so
  the export cannot drift from the screen, and returns the projection warnings
  the TypeScript version could not report at all — a broken equation used to
  export in total silence. `render_plain_text` is `Document::visible_text` plus
  a count of the structure plain text loses (tables, images, equations,
  comments, furniture).
  **The exported stylesheet states no length of its own**: every size is a
  `var(--doc-*)` or `var(--page-*)` filled in by `type_scale_css_variables()`
  and `page_setup_css_variables()`, which is the point ADR 0014 §3 made — a
  size cannot be written down twice. A test scans the rules for a digit
  followed by a CSS unit and fails on one.
- **UI-16 partly done.** File ▸ Download as PDF and File ▸ Details landed.
  **Deliberately not added:** ODT (needs a writer nobody has built — the DOCX
  writer is 1,900 lines and ODF is a comparable slice), "Make a copy" (needs a
  command in `opendoc-app`'s document surface, owned by another agent this
  wave) and "Move to trash" (needs FS-27's deletion model). None is stubbed in
  the menu: an entry that does nothing is worse than an absent one.

#### Carried forward from this wave

- **Details is shown in a textarea**, because `ui.ts`'s dialog helper has no
  read-only field. Editing it changes nothing, which is mildly dishonest; a
  `readonly` flag on `DialogField` is the tidy fix and belongs to whoever owns
  that file.
- **Images in the PDF are frames, not pictures.** PNG's IDAT is zlib-compressed
  filtered scanlines and PDF's `FlateDecode` + PNG predictor can consume them
  directly for the non-palette, non-alpha case — a real path to embedding
  without an image decoder, but palette and alpha need one, and half an image
  format is worse than a frame that says so.
- **Layout is still not incremental**, and the painting mode allocates per run,
  so `layout_painted_document` is heavier than `layout_document`. Only the
  export calls it, so it runs once per download rather than per keystroke.
- **Found and fixed while writing this up:** headers and footers were being
  drawn at the *body* size in the PDF, because `TypeScale` did not own the
  furniture scale and `styles.css` stated 10pt/1.3 as literals — a real
  PDF-versus-screen drift in the one export whose selling point is matching
  the screen. `TypeScale` now carries `furniture_size` and
  `furniture_line_height_thousandths`, projects them as `--doc-furniture-*`,
  and the stylesheet reads them. `.doc-table`'s padding and borders remain the
  last literals in `styles.css` that the scale also states.

### 2026-09-12: PDF and the export surface (FS-21, UI-16)

- **PDF export landed — ADR 0016.** `crates/opendoc-pdf` using `pdf-writer`
  0.15, chosen after probing `wasm32-unknown-unknown` **before** writing code
  (`lopdf` rejected for a `time`/`chrono` dependency — the `rust_xlsxwriter`
  clock trap in another shape). Accepted cost: no deflate, so streams are
  uncompressed and therefore readable, which is what lets tests assert on the
  operators actually emitted.
  **The PDF is drawn from the layout result, not the document.**
  `opendoc-layout` gained a painting mode that runs the *same* pass and records
  what it already decided, with the recording inside `text::Breaker` so every
  capture method mirrors one width operation — re-breaking paragraphs in the
  writer would have been exactly the second engine ADR 0009 feared, one level
  down. A test asserts measure and capture break identically across five
  widths. The embedded faces are literally the TrueType files the layout
  measured with, so screen, layout and PDF share one set of metrics.
  Independently verified by me with a throwaway crate outside the repo:
  `pdfinfo` reports Producer `OpenDoc`, `pdffonts` shows an embedded
  `OpenDocSans` CID TrueType with a Unicode map, and `pdftotext -f 3 -l 3`
  extracts the right paragraphs on the right page with `café naïve Łódź
  größer` intact — proving the Identity-H encoding and `ToUnicode` CMap work.
  Estimated content (images as a framed alt-text box, equations as LaTeX
  source, merged cells) each raise an ADR 0010 warning naming the block.
  Could not verify: Acrobat, Preview, Windows print drivers, real printers.
- **HTML/text export ported to Rust**, closing PLAN77 port-list item 1.
  `render_standalone_html` wraps the renderer's *own* markup so it cannot
  drift from the screen, and returns projection warnings the TypeScript
  version could not report at all — a broken equation used to export in total
  silence. The exported stylesheet states **no length of its own**; a test
  scans the rules for a digit followed by a CSS unit and fails on one.
- **Found while writing the ADR:** headers and footers were drawn at *body*
  size in the PDF, because `TypeScale` did not own the furniture scale and
  `styles.css` stated 10pt/1.3 as literals — a real PDF-vs-screen drift in the
  one export whose purpose is matching the screen.
- **UI-16 partly closed:** Download as PDF and Details landed. ODT, "Make a
  copy" and "Move to trash" deliberately omitted rather than added as dead
  entries.
- Minor, queued: Details renders in a `textarea` because `ui.ts`'s dialog
  helper has no read-only field — editing it does nothing, which is mildly
  dishonest. A `readonly` flag on `DialogField` is the tidy fix.

### Watch item: the WASM core, measured — 19.18 MB down to 15.27 MB

`apps/desktop/src/wasm/opendoc_wasm_bg.wasm` had grown to 19.18 MB as
`opendoc-layout`, `opendoc-pdf` and the wider command surface landed. It is
now **15.27 MB** (4.60 MB gzipped, 3.74 MB brotli), from three changes, each
measured:

- **`[profile.release-wasm]`** in the workspace `Cargo.toml` — fat LTO, one
  codegen unit — selected by `build-wasm.mjs`. Only the browser build pays the
  link time; `release` itself is untouched so test cycles are not taxed.
  −18.0% before wasm-bindgen.
- **The `name` section is dropped after wasm-bindgen** (1.69 MB of Rust symbol
  names nothing in the browser reads), in `build-wasm.mjs` rather than with the
  profile's `strip`, which would also drop `target_features` and leave a module
  every wasm post-processor rejects. `WASM_KEEP_NAMES=1` keeps them.
- **The bundled faces were embedded twice** — `const` rather than `static` in
  `opendoc-layout`'s `font.rs`, so each of the two use sites got its own copy.
  148 KB.

What was checked and deliberately *not* done, with the numbers:

- **`wasm-opt` is not run.** On top of fat LTO it saves 4.1% (`-O2`) to 6.4%
  (`-Oz`) and costs 20–45% more time in `layout_document`, measured through
  this module's own command surface. It is also not installed here, and a
  build that silently used a tool when it happened to be present would make
  the artifact machine-dependent.
- **`opt-level` stays at 3.** `"s"` is 1.6 MB smaller and `"z"` 3.1 MB smaller,
  at 71% and 148% more `layout_document` time and 33% and 102% more per
  editing command.
- **`panic = "abort"` buys nothing:** `wasm32-unknown-unknown` is already an
  abort target, so it would only change the native release binaries.
- **The fonts stay in the module.** They are one copy now, ~150 KB — 1% of the
  module, and the loop ADR 0014 closes depends on Rust measuring the bytes it
  carries. Taking metrics from a host-supplied font would need a WOFF2 decoder
  (more code than the fonts cost) and would leave the PDF writer and every
  headless runtime without a face.

**What dominates now, for whoever picks this up next.** The module is 67% code
(10.79 MB) and 32% data (5.20 MB), and **2.97 MB of that data is hayagriva's
bundled CSL archive** — 150 style and locale files reached through
`opendoc-citations`' `archive` feature, 19% of the whole module, before a
citation is formatted. Next after it, in code: `opendoc-app` 1.14 MB,
`opendoc-spreadsheet` 0.68 MB, `cbor2` 0.58 MB, `serde` 0.50 MB,
`rust_xlsxwriter` 0.41 MB, `hayagriva` 0.40 MB. Trimming any of them means
deciding a feature is not reachable in the browser, which is a product
decision, not a build one.

### 2026-09-12: browser runtime performance

- **WASM 19.18 MB → 15.27 MB (−20.4%)**, measured per change: fonts
  `const` → `static` (−148 KB; `const` is inlined per use site, so the faces
  were embedded *twice inside the module* — 10 `OS/2` tags before, 5 after),
  a new `[profile.release-wasm]` with fat LTO and `codegen-units = 1`
  (−2.06 MB), and dropping the `name`/`producers` sections after wasm-bindgen
  (−1.69 MB). `release` itself is deliberately untouched, so
  `cargo test --release --workspace` is not taxed for a binary nobody
  downloads.
- **`strip = "symbols"` was rejected for a concrete reason**: it also removes
  `target_features`, and a module without it is refused by every wasm
  post-processor. `build-wasm.mjs` therefore walks the sections itself and
  validates with `WebAssembly.validate` before writing.
- **`wasm-opt` measured and deliberately not adopted.** On the pre-LTO module
  it gave −21%; on the LTO module only −4% to −6%, and it made
  `layout_document` **20-45% slower**. A build that used a tool only when it
  happened to be installed would also make the artifact machine-dependent,
  which sits badly with this project's determinism thesis.
- **Layout: 36.8 ms → 4.7 ms in the browser (7.8×), and no cache was added.**
  Profiling found 54% of the time in `ttf_parser`'s `cmap` binary search:
  `Fonts::covers` was consulted per character and bypassed the advance cache,
  and `break_content` allocated a `String` per character even when not
  capturing. Both are constant-factor fixes and determinism-neutral — verified
  byte-identical output across 180 documents against the pre-change crate.
  **Incremental layout was then judged not worth it**: pagination, the part
  that genuinely cannot be cached, is 0.29 ms of 2.9 ms; a per-block cache
  would have to key on block, frame width, list-run state and type scale to be
  sound, trading the crate's one guarantee for milliseconds it no longer costs.

#### The next real browser cost is not layout

`add_paragraph` costs **25-29 ms in the browser on a 1,500-block document** —
six times a full relayout, and unlike layout it is **on the typing path**. It
looks like the cost of every command returning the whole `AppDocument` as
JSON. That is an `opendoc-api`/`opendoc-app` concern and is now the browser
runtime's dominant cost.

Also newly measured: **2.97 MB of the 5.20 MB data section is hayagriva's
bundled CSL archive** (150 style/locale files, 19% of the module) — 20× the
fonts, and the largest single item. Trimming it is a product call about which
citation styles the browser can format. `rust_xlsxwriter` + `zip` + `zopfli` +
`calamine` account for a further ~0.67 MB of code reachable in the browser.

### 2026-09-12: render-side and spreadsheet cleanups

- **Hidden axes moved into `render_workbook_html`**; the TypeScript workaround
  is deleted, not kept as a fallback. Three things had to move together: the
  `<colgroup>` iterates *drawn* columns so the positional mapping holds, frozen
  counts still use position in the sheet (hiding a frozen column must not
  promote a later one), and **merges re-anchor to the first drawn corner**
  while keeping content and `data-address` from the stored anchor — without
  which hiding a merge's own anchor column left the rectangle covered by a
  cell that was never written, and the row came out one `<td>` short: a
  sheared grid.
- **`mutate_spreadsheet` is transactional** — stage on a copy, commit on `Ok`,
  with invalidations moved after the commit so a failed mutation invalidates
  nothing. **Cost is zero on the evaluating path**: `evaluated()` was already
  `clone` + recalc, so the staged copy *is* the clone that was being made
  anyway. The cheaper "clone only where it can fail" scheme was rejected
  because it needs to know in advance which mutations can fail — precisely the
  assumption that produced the half-applied ranges. Both pre-checks removed;
  `set_spreadsheet_selection_format` is fixed for free (it half-applied
  because `set_cell_format` creates the cell before validating).
- **Table CSS literals projected**, and a new test `include_str!`s
  `styles.css` and asserts every `var(--doc-…, fallback)` states exactly what
  `TypeScale` projects. **It found a literal the brief had not listed**:
  `.footnote-area { font-size: 9pt }` is `TypeScale::caption_size` — the same
  shape as the PDF furniture drift found yesterday.
- `DialogField.readonly` added and used by File ▸ Details — `readonly` rather
  than `disabled`, since a disabled control is dropped from the form and
  cannot be selected, both wrong for a value you are there to copy.

#### Newly found, queued

- **Spreadsheet selection does not skip hidden axes.** Arrow-keying over a
  hidden row lands on it, and clicking a merged block whose anchor is hidden
  selects the hidden anchor. That is `reduce_selection` in
  `opendoc-spreadsheet`; Sheets skips. A behaviour decision, deliberately not
  invented during a rendering move.
- **`render_service.rs:91` calls `workbook.evaluated()` on every
  `render_workbook_html`** — a full clone plus recalc per grid render.
  `evaluate(&mut self)` now exists if that path wants it.

### 2026-09-12 (wave 7 complete)

Full gate green: **932 tests passing / 0 failing**, clippy `-D warnings`
clean, fmt clean, no contract drift, typecheck clean, smoke passing, **e2e
36/36**, WASM **15.28 MB** (was 19.18).

- **The client can no longer assert its own permissions.**
  `authorize_runtime_command` takes no `permissions`, no `subject`, no
  `documentUuid`; the answers live in an `OpenDocServiceSession` on
  `OpenDocApp` whose only writers are the welcome frame and the service's own
  presence/role/acknowledgement updates. `dispatch.rs` reads it from `self`,
  never from `args`, so **there is no argument left that could widen it**.
  `OpenDocPermissionGrant` is deleted; a share invite is explicitly a request
  (`requested_role`). `decided_by` distinguishes a local runtime deciding from
  its own capabilities, a service answer, and *no answer* — which refuses,
  with no local fallback. The frontend lost `permissions`/`presence` from its
  runtime config, so a page can no longer declare its own access.
- **Envelope identity is separated from operation identity.**
  `next_envelope_seq` numbers journal entries, `next_operation_seq` numbers
  `OperationId` only; envelope validation no longer requires the two to match,
  which is what forced undo markers and blob/spreadsheet work to burn
  operation ids and leave the gaps the service refused. Journal, recovery
  segment (now `v1`, carrying both watermarks because they cannot be
  reconstructed from frames that start at the base snapshot) and repository
  changed together.
  **A pre-change repository opens and is read exactly as written** — no
  renumbering, since that would rewrite causal contexts naming those ids
  inside content-addressed objects a manifest chain commits to — and it is not
  read silently: `legacy-operation-sequence-gap` names the actor and the count,
  firing only on interior gaps so a compacted history is not mislabelled.
  Proven against the real service: a stream of type → blob → spreadsheet →
  type → undo → type over a real socket is now accepted, and reverting the
  split makes it fail with an out-of-sequence refusal.

#### Known, and a genuine product decision

**Undo inside a live session can still roll `next_operation_seq` back below
`acknowledged_seq`.** If the undone operation was already submitted,
re-minting its id is history rewriting and the service refuses it. The two
candidate fixes — gating undo on the acknowledgement watermark, or expressing
undo as inverse operations (which ADR 0015 already names as the real answer) —
are product decisions, deliberately not made unattended.

### 2026-09-12: the Tauri desktop app works

`npm run tauri dev` and `npm run tauri build` both work; **`tauri build` was
broken before** (aborted in the AppImage bundler with "couldn't find a square
icon"). Now produces deb, rpm and AppImage.

**A live ACL bug, invisible to every test we have:** `fetch_url_base64` was in
`generate_handler!` but in neither `build.rs`'s `COMMANDS` manifest nor the
capability file, so **Insert ▸ Image by URL was dead in the native shell** —
silently, with no image and no error. The four `collab_*` commands added
concurrently had the same omission. `scripts/native-check.mjs` now
cross-checks all three places (handler, manifest, capability) and caught the
`collab_*` gap on its first real run. **No Rust test and no browser test can
see this class of bug**, which is why the check is the fix.

Every Tauri-only path was then exercised through the real UI of the **bundled
AppImage** under Xvfb: native open/save dialogs with extension filters, file
read/write, repository save and reopen, `.docx` import, `fetch_url_base64`,
window title, the close guard (`prevent_close` → in-app prompt → clean exit),
and — most valuable — **`FileRecoveryJournalStore` under a real `kill -9`**,
which relaunched offering "9 unsaved changes" and replayed them correctly.
ADR 0005's desktop path had only ever been type-checked before.

Also fixed: `preflight.mjs` never checked `webkit2gtk-4.1` (what `wry`
actually links), and the README's claim that this environment lacks the
GTK/WebKit pkg-config files was false — all four are present.

#### Frontend bugs it found but could not fix (not its files)

1. **`shell.ts`: the OS window title never shows the unsaved marker while
   typing** — `editorHooks.onResult` calls `renderStatus`/`renderToolbar` but
   not `setWindowTitle`, which only `renderAll` does. Cosmetic; the close
   guard reads `has_unsaved_changes` from Rust.
2. **`actions.ts`: `insert-image-url` swallows a native rejection** —
   `fetchUrlFile` only returns `null` outside Tauri, so the `showError` branch
   never runs when the fetch itself fails. This is *why* the ACL bug was
   silent.
3. **`confirmDialog` renders a stray empty text input** — "Discard unsaved
   changes?" shows an unused one-line field.
4. **Recent documents do not survive a restart** in the native shell.

### 2026-09-12: native-shell bug fixes

- **A rejected native command now surfaces.** The swallow was not a bad
  `catch` — nothing caught it at all: a rejected `invoke` propagated out of
  `performAction`, out of `runAction`, into `void runAction(...)` as an
  unhandled console rejection. No image, no message. A `native()` helper now
  turns a rejection into a named error and stops the action the way a declined
  prompt does, and `runAction` no longer rethrows, so an unclaimed failure
  still reaches the banner. The two cases stay distinct: a refused fetch names
  the shell's own reason; a browser still says the runtime has no network.
- **`confirmDialog` has no fields.** `promptDialog` gained a `body` for prose;
  the question is the body and the answer is which control closed it. Focus
  now starts on the accepting button, so Enter answers the question instead of
  meaning "no". Word count, Keyboard shortcuts and About had the same stray
  input and are body-only too.

#### Recents now persist — wired in both runtimes

The diagnosis was not what the symptom suggested. Recents were **never written
anywhere, in any runtime**: `recent_documents` lived only in `state.rs`, and
the Tauri shell starts from `new_empty_document()`, so the list died with the
process. Reopening a folder repopulated entries because
`open_local_repository` scans the lookup index; what was lost was the memory of
*which* repositories were used.

`crates/opendoc-app/src/recent.rs` holds the mechanism —
`RecentDocumentStore`, a file store (temp-file + rename, so a crash mid-write
cannot leave an undecodable list), a volume store for the browser, and a
`RecentDocuments` type where every mutation dedupes, caps and writes back, so
there is no separate "save the recents" step a call site can forget.
`is_durable()` reports a runtime with no store rather than looking like one
that works.

The four call sites that make it real are now in place: `state.rs` holds a
`RecentDocuments` (it derefs to `[AppRecentDocument]`, so the projection is
unchanged); `repository_io.rs` calls `record`/`merge` instead of open-coding
the dedupe, the insert and the truncate, and reports a list it could not store
as `recent-documents-unwritable` rather than failing the save that already
reached the repository; `src-tauri/src/main.rs` installs
`FileRecentDocumentStore` over `<app data dir>/recent-documents`, beside the
recovery segments; and `opendoc-wasm`'s `storage_ready` installs
`VolumeRecentDocumentStore` over the volume key `recent/documents` — but only
once the volume is actually mirrored, because installing over a memory-only
volume would make `is_durable()` promise a reload will remember when it will
not. `OpenDocApp::install_recent_documents` returns **no error at all**: an
unreadable stored list is a model warning, so no shell can turn it into a
failure to start.

15 tests in `recent.rs`, four of them end to end through `dispatch_command`
across a simulated process restart; `npm run e2e` reloads real Chrome and
clicks the remembered entry; and the AppImage-less release binary was driven
under Xvfb through save → `kill -9` → relaunch, where the home screen now
lists the repository it used to forget.

The `native()` helper has also moved from `actions.ts` to `shared.ts` (it
cannot live in `ui.ts` without an import cycle) and is used at every remaining
shell-capability call site in `files.ts` and `spreadsheet.ts`. A refused
`write_file_base64` in an export used to produce neither the "Exported as …"
toast nor a message that said which export failed; the jsdom smoke test now
asserts both the named banner and the absence of a success toast, and asserts
that closing the save dialog is still neither.

### 2026-09-12 — collaborative undo (ADR 0017)

Undo is no longer a whole-state rewind. It is the **inverse of this actor's own
operations, submitted like any other edit**, which makes it new history rather
than a rewrite of old history — so the service accepts it, it converges through
the same merge, and it cannot reach a collaborator's work because every
operation in it names only its author's own contribution.

- **The two failures it fixes.** Restoring a snapshot taken before a remote edit
  deleted that edit locally while the service still held it — which is why
  `apply_remote_operations` used to drop the undo and redo stacks, costing a user
  the right to undo their own typing because somebody else typed. And undo
  re-minted operation ids, rolling `next_operation_seq` back below the service's
  `acknowledged_seq`, which the service refuses as history rewriting. **Both
  guards are gone**; `AppRemoteIntake::dropped_undo_history` is deleted.
- **Inverses are captured when the operation is written**, against the document
  it was written against — the only moment the state a delete removed or a set
  overwrote still exists. The match in `opendoc-merge/src/inverse.rs` is
  exhaustive with no catch-all, so a new operation does not compile until
  somebody decides whether it can be undone. Inside a batch each operation is
  inverted against the state immediately before *it*, so "delete this text, then
  delete the block it was in" does not restore the text twice.
- **Character operations are the exception and are resolved at undo time.**
  `InsertText`/`DeleteText` are the only offset-addressed operations, and an
  offset means something else once a collaborator has edited the run.
  `invert_text_operations` re-derives the run's character identities exactly as
  the merge does — `collect_text_run_edits` is now one shared function — and asks
  where *this* operation's characters are now. Undoing an insert a colleague
  typed inside therefore emits two deletes around their text, and undoing a
  delete restores only what nobody else also deleted.
- **Nine cases genuinely cannot be inverted** and say so by name rather than
  no-opping; four of them are one gap: `InsertBlock`/`InsertInline`/
  `MoveInlineToBlock`/`InsertTableCell` spell their anchor `after: Option<_>`
  where `None` means *append*, so they cannot say "first". The fix is the
  `InsertPosition` the table row/column operations already have (ADR 0013), and
  it touches 28 call sites in command modules — **not done**.
- **The snapshot stack survives as a fallback**, for steps that also move
  spreadsheet or blob state, and is **refused inside a session** rather than
  restored. Its memory cliff is gone: `checkpoint()` shared one copy of
  `blob_bytes` instead of deep-copying every embedded image up to 200 times,
  which is sound because the map is content-addressed. Removing the remaining
  per-command snapshot needs a `dispatch.rs` change and was left.
- **Proved over the real service.** `an_apps_undo_reverses_only_its_own_work_and_the_service_accepts_it`:
  A types and is acknowledged, B edits the same run, A undoes — B's word survives
  in place, every id the undo mints is above the watermark, the `Accepted` frame
  is asserted rather than inferred, and the app, both transport sessions and the
  server's materialised document encode to identical canonical CBOR. Then a redo,
  and the same checks again.

### 2026-09-12: collaborative undo — ADR 0017

**An undo is now an ordinary edit that says the opposite of an earlier one.**
It inverts each operation of the step newest-first and applies them through the
same `apply_batch` a keystroke uses — fresh ids, ordinary envelopes, same
merge, ordinary submit. So it is new history rather than a rewrite, which is
what the service's history-immutability check requires. Redo is the inverse of
the inverse: undo and redo are one function with the stacks swapped.
**Per-actor falls out structurally** — the stack holds this replica's own
steps and an inverse names only its author's contribution.

Inverses are captured **when the operation is written**, which is forced: a
delete's inverse must carry what was removed. Within a batch each operation is
inverted against the state immediately before *it*, so "delete this text, then
delete the block it was in" does not restore the text twice.

Character operations are the one exception and the interesting part.
`InsertText`/`DeleteText` are the only offset-addressed operations, and an
offset means something else once a collaborator has edited the run — so they
defer, and resolve at undo time by re-deriving ADR 0007's character
identities. `collect_text_run_edits` is now **one function shared by the merge
and the inverse**, so the two cannot disagree.

Dependent cases: a colleague typing *inside* your inserted text leaves the
inverse naming only your characters, so the undo goes out as **two** deletes
and their text survives in place. Both actors deleting the same characters →
undo restores only what nobody else deleted. **The one honest loss**, stated
in the ADR: a colleague's edit to a run inside a block you deleted, arriving
after the delete, is not replayed onto the restored snapshot.

- **The stack-clearing guard in `apply_remote_operations` is gone**, along with
  `dropped_undo_history`.
- The snapshot stack survives only as a fallback for steps that also moved
  spreadsheet/blob state or contain one of **nine named irreversible
  operations** — and inside a session it is *refused* rather than restored,
  with a conflict that does not consume the step.
- **The `blob_bytes` undo cliff is fixed**: checkpoints key the map by SHA-256
  of its values, so they share one copy while no blob changes — O(blobs) key
  compare instead of O(bytes) deep copy.

Two of its eight mutations survived the tests as first written, and it
**changed the tests rather than the claim**: a step-window test that only
checked text (an unbounded window lands on the right text anyway) now asserts
each undo authors exactly one operation, and a refusal test that used a
spreadsheet step — rejected before any inversion is read — was replaced.

#### Follow-ups it named

1. **`InsertPosition` for `InsertBlock`/`InsertInline`/`MoveInlineToBlock`/
   `InsertTableCell`** — `after: None` meaning *append* cannot say "first",
   which is four of the nine refusals. The table operations already have
   `InsertPosition`; this is 28 call sites in command modules.
2. Making the dispatcher's checkpoint conditional, which is what would remove
   the snapshot stack's remaining per-command cost.
3. `operation_inverses` is not cleared by `close_document` — a per-process
   memory leak across documents, not a correctness problem.
4. Spreadsheet and blob steps have no inverse at all, so they cannot be undone
   inside a session; ADR 0015 notes the service has no path for either.

### 2026-09-12: per-block morph — and a live bug it uncovered

**`opendoc-render` was projecting `BlockProperties::space_before` as the
physical `margin-top` — the same CSSOM property `pagination.ts` writes the
page-break margin into and clears on every block that does not open a page.**
Measured in Chrome before any change: a paragraph with 36pt of space-before
rendered at 48px and pagination took it to **0px, permanently**. Applying a
layout was deleting the document's own spacing.

Fixed as an ownership split rather than an ordering repair: the renderer
projects the **logical** `margin-block-start`/`-end` (which is what the model
means — space before *in flow order* — and matches why indents already use
`margin-inline-start`), leaving physical `margin-top` to pagination. And
`applyPlacement` is now **total**: it writes every block's placement including
clearing blocks the layout does not name, and only where the value differs. A
block's DOM is therefore `f(last-applied markup, placement)` with the two
written to disjoint properties.

**Per-block application needed no wire change.** `editor.ts` keeps the source
tree it last applied and compares new markup against *that*, never against the
live DOM; where a paired subtree `isEqualNode`s, it is skipped whole. Skipping
is only ever an optimisation — no pairing falls back to the full morph — and
`compositionstart` drops the cache, because the browser owns the DOM until
`compositionend`.

Measured, 1,500 blocks, real Chrome, like-for-like load:

| | before | after |
|---|---|---|
| keystroke | 55.9 ms | **23.9 ms** |
| DOM leg (`setHtml` with a change) | 30.1 ms | **9.2 ms** (morph ~25 → ~4) |
| re-render that changes nothing | 31.5 ms | **0.1 ms** |

Its mutation of the dropped-update case failed **20 non-collab e2e checks**,
led by `one keystroke types one character` — and the placement mutation failed
exactly one, the new `a page-break margin is cleared when the break moves off
a block`, which is the guard for the subtle hazard: changing the page size
changes no block's markup, so only a *total* placement pass can remove a stale
break.

#### The fragments API has a consumer, and the projection lost two fields

`render_document_body`'s fragments now **are** the body on the wire.
`AppDocument::body_fragments: Vec<AppBodyFragment { block_id, blocks, html }>`
**replaces** `body_html` rather than joining it — carrying both would have
doubled the largest item in the payload — and `AppDocument::body_html()`
reassembles the string for the Rust callers that still want it whole, which is
exact because `opendoc-render` pins the composition byte for byte.
`projection_service` fills it from **one** walk of the body, the same walk the
warnings come out of.

`editor.ts` lost `setHtml` for `setFragments`, which compares markup *strings*
per block id and parses only the fragment that changed. Live node identity
comes from the DOM, keyed on `data-block-id` — sound because a fragment's key
is also the first `data-block-id` inside it (on the element for every block
kind, on the first `<li>` for a list run, whose wrapper has none), which
`every_fragments_first_block_id_is_its_key` pins in the renderer. The detached
copy of the parsed body the old scheme held for `isEqualNode` comparison is
gone; a string per key needs no tree. `compositionstart` still drops the cache,
so IME is still repaired by a full re-morph.

**`visible_text` is gone**, and so is the field `word_count` and
`character_count` were counted from — `AppDocument::visible_text()` derives the
text through the model's own `Document::visible_text`, one implementation, so
the counts on the wire and the text cannot drift. `signing_document_from_snapshot`
used to *erase* the field before hashing, which was the standing evidence that
it never belonged in source state; that line is now unnecessary. The payload
changed shape, so the snapshot format says so: a local save writes
**`opendoc.app-document.v1`** and a `v0` repository is refused by name rather
than read as this shape and reported as holding broken signatures for no
stated reason.

Measured, 1,501 blocks, real Chrome, like-for-like (medians of three runs of
twelve, `domperf.mjs`, serving a copy of `dist/`):

| | before | after |
|---|---|---|
| keystroke, end to end | 35.2 ms | **26.4 ms** |
| DOM leg (a body that really differs) | 9.0 ms | **0.9 ms** |
| `onResult` (DOM + status + toolbar + geometry + the sync part of paginate/find) | 19.8 ms | **10.8 ms** |
| parse of the markup that changed | 5.4 ms (whole body) | **0.0 ms** (one paragraph) |
| re-render that changes nothing | 0.1 ms | 0.2 ms |
| document payload | 847.6 KB | 864.8 KB |
| — the body | 397.6 KB | 495.8 KB |
| — `visible_text` | 81.0 KB | **0 KB** |

The payload is **17 KB bigger**, and that is the honest trade: 81 KB of text
left and 98 KB of per-fragment framing arrived (a key and a length per
top-level element, ~65 bytes × 1,501). `dispatch` and `JSON.parse` did not
notice; the DOM leg dropped by 8 ms. The no-change case cost 0.9 ms until a
cheap pre-pass was added — n string compares, no DOM read — which puts it back
at 0.1-0.2 ms; the `setHtml` it replaced compared one string and returned, and
losing that was a real regression rather than a rounding difference.

**The DOM is no longer the browser's dominant per-keystroke cost.** What is
left in `onResult` is Rust: `layout_document` runs synchronously inside the
WASM `invoke`, so pagination's 4.3 ms and `refreshFind`'s query are inside that
10.8 ms, with `renderToolbar` at 1.4 ms. The next thing to attack is that
pagination and find both re-ask the whole document per keystroke.

Also fixed: **`editorHooks.onResult` never wrote the OS window title**, so
typing left the titlebar claiming a document with unsaved work was clean while
the status line said otherwise. One `applyWindowTitle()` now owns it and both
callers use it, guarded by `typing marks the window title unsaved`.

**`build-wasm.mjs` was not retaining symbol names** — that observation was a
misreading of the intermediate line in its own log (`wasm-bindgen produced
16.2 MB`, *then* `without symbol names 14.3 MB (1.90 MB of names dropped)`).
The strip runs, `WebAssembly.validate` still gates it, `target_features`
survives, and `WASM_KEEP_NAMES=1` still keeps the 1.89 MB `name` section. It
can no longer be wrong quietly: dropping **nothing** now fails the build
instead of logging `0.00 MB of names dropped` and shipping the names.

### 2026-09-12 (wave 8 complete): collaboration is user-visible

Full gate green: **1,016 tests passing / 0 failing**, clippy `-D warnings`
clean, fmt clean, `src-tauri` checks, no contract drift, typecheck clean,
smoke passing, native-check passing, **e2e 42/42 in real Chrome**.

**Transport — ADR 0018. One protocol, two transports, one UI.** The split is
forced: `apply_remote_operations` and friends are Rust *methods*, not
commands, so "feed frames through the existing command surface" was not
available.

- **Browser**: TypeScript owns the socket, Rust owns every byte inside it.
  `collab.ts` opens the `WebSocket` and hands each frame to
  `opendoc-wasm/src/collab.rs` as an opaque string; it never parses or
  composes a frame. `web_sys::WebSocket` was rejected for a concrete reason —
  frames arrive in closures that can fire while `dispatch` holds the
  `RefCell`, and the re-entrancy guard would **silently drop the frame**. JS
  cannot interrupt a synchronous call into WASM, so a TS socket structurally
  cannot do that.
- **Tauri**: Rust owns the socket through `opendoc-service`'s own client, one
  thread per session, taking the same mutex as `dispatch` and never across an
  `await`. **The session token never enters the webview.**
- The duplicated frame envelope is checked in bytes against a **generated**
  fixture asserted from both sides, so either side drifting fails a test.
- **A dropped socket** rejoins from the welcome, reads the acked watermark back
  out of the log, and replays the tail the service never got **as operations**.
  Retries are bounded, then the session is declared over — because a browser
  is never told why a handshake failed, so retrying forever would show
  "Reconnecting…" at a service that will never answer.

**The service needed a browser policy at all**: a page could not reach it.
An exact-match origin allowlist now serves both CORS and the WebSocket
`Origin` check — the latter is genuine cross-site-WebSocket-hijacking
protection, since the same-origin policy does not cover a handshake. Default
is deny; a request with no `Origin` is untouched, so native clients are
unaffected.

**Two real bugs found:** the service *binary* served nothing (`main` called
`shutdown()`, which sends the signal — it printed "listening" and tore the
listener down; never noticed because the only client was a test suite that
binds its own server), and **a document the service created had no blocks**,
so a freshly shared document was one nobody could type into.

Evidence was two separate Chrome instances against a real service process —
two profiles, so ADR 0008's two-tab storage clobbering could not be mistaken
for a collaboration bug.

It also **cross-validated the undo work** rather than working around it: its
test asserts from outside the app that an undo in a live session submits new
operations above everything the service holds.

### 2026-09-12: recents persist, and native failures surface everywhere

- **Recents now persist in both runtimes.** The four wiring edits landed:
  `state.rs` field type, `repository_io.rs` record + scan-merge (deleting the
  duplicated retain/insert/truncate at both sites), `src-tauri` installing the
  file store beside `recovery/`, and `opendoc-wasm` installing the volume
  store from `storage::ready()` and `reset()`. A store that refuses a write
  becomes the warning `recent-documents-unwritable` — **the save still
  succeeds**, because the document reached the repository and only the memory
  of it did not. `install_recent_documents` returns **no `Result` at all**, so
  a corrupt stored list cannot be turned into a failure to start even by
  accident.
  One deliberate difference from the recovery journal: the browser store is
  installed only when the volume is persistent, because installing over a
  memory-only volume would make `is_durable()` promise a reload will remember
  when it will not.
  Evidence is a **real process restart**: release binary under Xvfb with a
  private `XDG_DATA_HOME`, typed, saved through the native GTK chooser,
  `kill -9`, relaunched — the repository is listed and reopening it brings the
  text back in a brand-new process. Plus an e2e check that the list survives a
  page reload in the browser.
- **`native()` moved to `shared.ts` and applied at all ten physical call
  sites**, each with its own words, and every `null` keeping its own separate
  meaning (dialog closed vs. capability absent vs. genuine failure). A refused
  `write_file_base64` now says "Could not write the … file" instead of
  producing neither a toast nor an error. Its mutation proving this failed with
  `'write_file_base64 not allowed by the capability file'` — the exact silent
  failure the ACL bug produced.

#### Two things it flagged

- **`crates/opendoc-app/src/import_export.rs:279` has a bare CR inside a byte
  string** (`b"PNG\r"` written literally), which intermittently breaks
  `cargo clippy -p opendoc-app`. Whoever owns `import_export*` should escape
  it.
- **A concurrency risk worth verifying at the next gate**: its mutation runs
  snapshotted and restored `repository_io.rs` and `storage.rs` by file copy
  while other agents were editing the same tree. It reports the other agent's
  edits present in the final diff, but this is the mechanism by which a
  concurrent change could be silently reverted.

### 2026-09-12: typing path finished — and a correction

**Correction to an earlier entry in this file:** `build-wasm.mjs` was **not**
retaining 1.89 MB of symbol names. That claim came from misreading its own log
— `wasm-bindgen produced 16.2 MB` is the line *before* `without symbol names
14.32 MB (1.90 MB of names dropped)`. Confirmed by section dump:
`custom:name` absent, `custom:target_features` present, and
`WASM_KEEP_NAMES=1` still keeps the section. I repeated the claim in a brief
without checking it; it was wrong.

The strip is now unable to fail quietly: dropping **nothing** throws instead
of logging `0.00 MB of names dropped` and shipping the regression.

**`body_fragments` replaced `body_html` on the wire** (rather than being added
beside it — carrying both would have cost 400 KB per keystroke).
`AppDocument::body_html()` survives as a derived accessor for Rust callers,
exact because `opendoc-render` pins the composition byte for byte.
`editor.ts` now compares one string per block id and **parses only the changed
fragment**; the detached DOM copy the old `isEqualNode` scheme kept is gone,
since a string per key needs no tree.

Measured, 1,501 blocks, real Chrome:

| | before | after |
|---|---|---|
| keystroke end to end | 35.2 ms | **26.4 ms** |
| DOM leg (body really differs) | 9.0 ms | **0.9 ms** |
| parse of the changed markup | 5.4 ms (whole body) | **0.0 ms** (one paragraph) |
| `visible_text` on the wire | 81 KB | **0** |

**The payload got 17 KB bigger and that is the honest trade**: 81 KB of text
left, 98 KB of per-fragment framing arrived. It also **fixed a flaw in the
performance harness** — it had been handing `onResult` the *same* result twelve
times, so eleven of twelve calls measured the no-change path.

`visible_text` is gone from source state, and the format string moved to
`opendoc.app-document.v1` so a `v0` repository is refused **by name** rather
than read as this shape and then reporting every signature it holds as broken
with no reason.

**The DOM is no longer the dominant browser cost.** What remains inside
`onResult` is Rust: `layout_document` runs synchronously inside the WASM
`invoke`, so pagination (4.3 ms) and `refreshFind` sit inside it, both
re-asking the whole document per keystroke. That is the next target, and it
revives the question ADR 0014 settled when the DOM dominated.

Its list-run mutation is worth recording: keying a run on its *last* `<li>`
rebuilds it every keystroke while leaving the **content** correct, so all 44
existing checks passed. Only node identity showed it.

### 2026-09-12 (wave 9 complete): PLAN77 is finished

Full gate green: **1,098 tests passing / 0 failing**, clippy `-D warnings`
clean, fmt clean, `src-tauri` checks, no contract drift, typecheck clean,
smoke passing, native-check passing, **e2e 45/45 in real Chrome**, WASM
**14.38 MB** (19.18 at its peak).

- **ODT export built and wired.** `xmllint --relaxng` validates every part
  against the official OpenDocument 1.3 schema, and LibreOffice — the
  reference ODF implementation — rendered it to PDF with the footer
  **recomputed as "Page 1 of 2"**, proving the page-number fields are fields
  rather than frozen text. The nearest thing to a round trip goes
  ODT → LibreOffice → `.docx` → **this repo's own DOCX reader**, and the twips
  survive twips → pt → LibreOffice → twips exactly.
  It went beyond the DOCX writer in one respect: merged cells are written as a
  span **plus real `covered-table-cell` elements**, so the grid stays
  rectangular as the model's is and merging remains invertible.
  **Two LibreOffice behaviours found by measurement, not assumption** — a list
  level's label alignment is ignored without
  `text:list-level-position-and-space-mode="label-alignment"`, and ODF
  resolves margins logically but `text-align="start"` always draws left, so an
  RTL block needs an explicit alignment. Both were bugs in its first version.
  I applied the five contract edits and the menu entry it specified.
- **The CSL archive is trimmed: data section 5.22 MB → 3.00 MB (−42.6%).**
  The archive held 91 styles and 64 locales; the code reached **eight**, and
  `available_styles()`/`all_style_names()` had no callers outside the crate's
  own tests. The eight styles and eight locales are now vendored
  **byte-for-byte from hayagriva's own archive**, so no citation renders
  differently, and hayagriva is built with `default-features = false` so the
  archive is not compiled at all. Attribution was verified by a controlled A/B
  on today's code rather than inferred from the file size, matching the
  observed delta to within 5 KB.
  An unbundled style keeps the built-in renderer — a real answer, never wrong
  output, never a panic — and reports `citation-style-not-bundled` naming the
  bundled set.
- **`InsertPosition` closed four of ADR 0017's nine undo refusals** (now
  five); `operation_inverses` is bounded where it is written and cleared on
  `close_document` (I applied that patch).
- **Hidden-axis selection decided**: any anchor or focus returned is an
  address the grid draws, while the range between them still spans hidden
  axes, because hiding is not deletion. The merged-anchor case was **got wrong
  first** and caught by a cross-check against the renderer's real markup.

#### Still open, all specific

- `citation_support_warnings` is called from the export paths but not folded
  into the `AppDocument` projection, so the live warnings panel is silent
  about an unbundled style. One call beside the equation warnings.
- **Our DOCX *reader* drops `gridSpan`/`vMerge` and `gridCol` widths** — found
  when LibreOffice's DOCX carried both and `docx.rs` reported
  `docx-dropped-cell-span`. The ODT path is now the one that renders RTL
  indents correctly; the DOCX export writes `w:ind w:left` for an RTL block.
- ODT equations are LaTeX source (needs a `math:math` sub-document and a
  LaTeX→MathML step; `opendoc-render` has one but is not a dependency of
  `opendoc-import`); comments have a real ODF home (`office:annotation`) and
  are warned instead.
- `layout_document` and `refreshFind` still re-ask the whole document per
  keystroke inside the synchronous WASM call — now that the DOM leg is 0.9 ms,
  this is the dominant browser cost and revives ADR 0014's incremental-layout
  question, which was settled when the DOM dominated.
- `RECOVERY_SEGMENT_FORMAT` should arguably be `v2` (naming accuracy; a legacy
  segment replays correctly).
