# OpenDoc Restructure Plan

Status: active cleanup plan, generated 2026-09-06. Last audited 2026-09-09.

**Phase:** Phases 0-4 are complete; Phase 5 (frontend) is the current work.
Phases 6-8 are open, except Phase 8 archiving, which is done.

**Done.** The app monolith is extracted: `opendoc-app-api` is gone from the
tree, `opendoc-app/src/lib.rs` is a 184-line facade, and app orchestration
lives in typed services and command method groups. Spreadsheet, render, and
command-contract ownership moved into `opendoc-spreadsheet`, `opendoc-render`,
and `opendoc-api`. The command contract is fully generated from Rust metadata
and drift-checked in `npm run verify`. The per-extraction history is in
[Completed Work](#completed-work); do not re-summarize it here.

**Next.** Split `apps/desktop/src/main.ts` by surface (it is still one
1,871-line file), move the last TypeScript document semantics behind Rust
commands, and bring the four oversized Rust files listed in
[Open Problems](#open-problems) under the 2,000-line target. See
[Next Architecture Slice](#next-architecture-slice) for ordering and
[Outstanding Work](#outstanding-work) for the tracked items.

**Verification baseline.** `npm run verify` from `apps/desktop` last passed on
2026-09-06 (Rust format, workspace clippy, workspace tests, OpenDAL app tests,
generated-contract staleness check, TypeScript typecheck, WASM build, desktop
build, jsdom smoke). The 2026-09-09 audit was a read-only re-verification of
this document against the tree; it did not rerun the gate.

Purpose: replace the unclear prototype state with a buildable, well-structured
Rust-first document system with a Rust/WASM frontend core, a thin TypeScript UI
shell, and no compatibility obligation to code or tests that pin the wrong
architecture.

This plan supersedes overlapping implementation and parity plans for
restructure work. Product requirements can still live elsewhere, but this file
is the tracking plan for getting the codebase into a shape that can support
them. `docs/GOOGLE_DOCS_PARITY_TODO.md` is historical audit input, not the live
list.

## Open Problems

Only problems that are still true today. Everything already fixed is recorded
once, in [Completed Work](#completed-work).

### The 2,000-line target is not met workspace-wide

Phase 2's exit criterion says no Rust source file exceeds 2,000 lines. That
holds for `opendoc-app` (largest `editor.rs`, 1,942), `opendoc-api` (largest
`commands.rs`, 1,971), `opendoc-spreadsheet` (largest `functions_legacy.rs`,
1,922), and `opendoc-render` (1,169). It does **not** hold for the older
domain crates, which were never part of the app-monolith extraction:

| File | Total | Non-test (before the trailing `#[cfg(test)]`) |
| --- | --- | --- |
| `crates/opendoc-merge/src/lib.rs` | 12,227 | ~3,424 |
| `crates/opendoc-import/src/lib.rs` | 5,053 | ~2,090 |
| `crates/opendoc-store/src/lib.rs` | 4,744 | ~2,537 |
| `crates/opendoc-import/src/docx.rs` | 2,332 | ~2,260 |
| `crates/opendoc-core/src/lib.rs` | 2,690 | ~1,316 |

Four of the five are over target on implementation lines alone;
`opendoc-core/src/lib.rs` is over only because of its inline test tail. Reading
`wc -l crates/*/src/*.rs` and treating the whole workspace as compliant is the
error this table exists to prevent.

### `apps/desktop/src/main.ts` is not split by surface

It is 1,871 lines — under the 2,000-line ceiling, but still a single module
holding home screen, menus, toolbar, status bar, find/replace, side panels,
spreadsheet grid, and all action handlers. The Phase 5 task is a split by
surface, not a line count, and it is not done.

### Remaining TypeScript document semantics

`findMatches()` in `main.ts` walks the document tree and does case-folded text
matching with code-point offset arithmetic. That is document semantics in
TypeScript and belongs behind a Rust command, like the selection, mark, style,
and footnote workflows that already moved.

### `new_sample()` is still the default constructor

`OpenDocApp::new_sample()` is the boot path in both runtimes
(`apps/desktop/src-tauri/src/main.rs:183`, `crates/opendoc-wasm/src/lib.rs:23`
and `:47`) and in app tests. Only `new_sample` and `new_document` exist on
`state.rs`; the Phase 2 rule requiring distinct `new_empty_document`,
`new_empty_workbook`, `new_sample_document`, and `open_repository`
constructors is unimplemented, so sample workbook/document assumptions still
leak into runtime boot.

### Support files are still untracked

`apps/desktop/package-lock.json`, `apps/desktop/scripts/build-wasm.mjs`,
`apps/desktop/scripts/smoke.mjs`, `apps/desktop/src/generated/`,
`docs/GOOGLE_DOCS_PARITY_TODO.md`, `docs/archive/`, and
`docs/adr/0004-service-backend-boundary.md` are all untracked in git. The
Phase 0 *policy* is settled (see Phase 0), and `npm run verify` depends on
several of them, but the commit has not happened.

### Dead-code quarantine in `opendoc-spreadsheet`

`crates/opendoc-spreadsheet/src/lib.rs` carries five `#[allow(dead_code)]`
module attributes over now-private import/format/structure internals. Each
must be wired through a real Rust/API entry point or deleted.

### There is no real service backend

Tauri owns a local `Mutex<OpenDocApp>`, WASM owns a thread-local
`OpenDocApp`, and `relay_runtime_sync` classifies operation envelopes rather
than applying a durable multi-user protocol. The runtime/session/share/presence
DTOs are contract design, not a working backend. The creation gate is
`docs/adr/0004-service-backend-boundary.md`.

### The app facade still coordinates broad services

`OpenDocApp` is a facade rather than a god object, but it still coordinates
cross-domain services directly. The remaining direction is to keep shrinking
command orchestration into typed service objects rather than
`impl OpenDocApp` method groups.

## Non-Negotiable Direction

- Rust owns source schemas, validation, operations, merge, storage, signing,
  import/export, spreadsheet evaluation, and API contract generation.
- TypeScript owns DOM integration, event mapping, selection mapping, and visual
  UI state only. It must not contain document semantics, spreadsheet semantics,
  storage semantics, mock persistence, or duplicate command schemas.
- WASM uses the exact same Rust application/service API as Tauri and future
  server runtimes.
- Backwards compatibility is explicitly not required during this restructure.
  Delete tests, docs, commands, fields, generated artifacts, and adapters that
  preserve bad boundaries.
- Tests should verify invariants and public contracts, not internal historical
  structure.
- API contracts should be generated from Rust metadata or a single schema file,
  not hand-maintained in Rust, TypeScript, JSON, and prose.
- Avoid "god objects". Long-term state should be modeled as typed domain
  services composed behind a small application facade.

## Target Architecture

### Rust Crates

Keep or reshape the workspace around these responsibilities:

- `opendoc-model`: canonical document, spreadsheet, citation, blob, warning,
  operation, and identity types. Move current core model here or rename
  `opendoc-core` if cheaper.
- `opendoc-ops`: typed operations and operation application. No UI command
  parsing here.
- `opendoc-merge`: merge/rebase over typed operations and canonical state.
  Keep it DOM-independent.
- `opendoc-format`: canonical binary records and debug JSON projections.
- `opendoc-store`: object-store abstractions, local repository, flat namespace,
  head/candidate handling.
- `opendoc-sign`: source-state and blob signing.
- `opendoc-import`: Google/Word/Sheets import-export adapters only.
- `opendoc-citations`: citation parsing/rendering/adapters only.
- `opendoc-spreadsheet`: workbook model, parser, evaluator, dependency graph,
  import/export helpers.
- `opendoc-render`: HTML/debug render projections used by UI and tests. It
  should not mutate source state.
- `opendoc-app`: application facade that composes document service,
  spreadsheet service, storage service, signing service, and render service.
- `opendoc-api`: command schema, command dispatch, JSON/WASM/Tauri transport
  DTOs, and generated TypeScript bindings.
- `opendoc-wasm`: thin wasm-bindgen adapter around `opendoc-api`.
- `opendoc-service`: future HTTP/WebSocket multi-user service. Create only when
  the local operation/storage model is sane enough to expose.

`opendoc-app-api` no longer exists. Its implementation moved to `opendoc-app`,
and the leftover empty `crates/opendoc-app-api/` directory was removed from
disk on 2026-09-09. Nothing in the workspace references it; only historical
docs such as `docs/GOOGLE_DOCS_PARITY_TODO.md` still name it.

### Runtime Boundaries

- Tauri backend: native file dialogs, native filesystem access requested by UI,
  window lifecycle, and one call into `opendoc-api`.
- Browser/WASM: local in-memory app instance initially, later IndexedDB or
  browser storage adapter through a Rust-defined storage trait.
- Service backend: authenticated API, permissions, durable sync relay,
  presence, lookup, and commit serialization when implemented.
- Runtime policy objects belong outside document source state. They should not
  live beside core document structs.

### Frontend Boundaries

Audited 2026-09-09. Tracked TypeScript excluding generated WASM output is 4,446
lines, of which 1,588 are generated under `src/generated/`.

- `apps/desktop/src/invoke.ts` (243 lines): transport only. **Holds.** It picks
  Tauri or WASM, exposes `dispatch`/`invoke`, native file dialogs, window
  lifecycle, and frontend runtime config. No document logic.
- `apps/desktop/src/editor.ts` (531 lines): DOM selection/input adapter only.
  **Holds.** `DocumentEditor` plus DOM morphing and code-point/UTF-16 offset
  mapping; no command or document semantics.
- `apps/desktop/src/main.ts` (1,871 lines): screen composition and event
  wiring. **Does not hold yet** — one file per [Open Problems](#open-problems).
  Split it into modules by surface before adding features. Spreadsheet address
  parsing, range normalization, clipboard TSV semantics, formula action
  derivation, and selection reducers have already moved to Rust; find/replace
  matching has not.
- Generated TS bindings live under `apps/desktop/src/generated/` and are never
  hand-edited. `src/types.ts` (98 lines) and `src/commands.ts` (17 lines) are
  thin re-export barrels over them.
- `apps/desktop/src/wasm/` remains ignored generated output. Dev/build scripts
  create it deterministically.

## Phase 0: Stop The Bleeding — done

Goal: make the current tree understandable and reproducibly buildable before
large moves.

Policy for previously untracked files (decided; the commit itself is still
outstanding — see [Open Problems](#open-problems)):

- Track `apps/desktop/package-lock.json`; `npm ci` is part of desktop
  verification.
- Track `apps/desktop/scripts/build-wasm.mjs`; browser/WASM builds are
  supported and required before static desktop builds.
- Track `apps/desktop/scripts/smoke.mjs`; `npm run verify` depends on it.
- Track `apps/desktop/src/generated/`; it is generated but committed.
- Track `docs/GOOGLE_DOCS_PARITY_TODO.md` only as historical audit input. Live
  restructure work is tracked in this file.

Completed tasks:

- `cargo fmt --all` is clean and enforced by `npm run verify`.
- The stale `AppSheet::evaluate` path that called a deleted `evaluate_cell`
  free function was deleted; one workbook-wide recalculator remains.
- `npm run verify` is the top-level verification target: fmt, clippy with
  warnings denied, workspace tests, feature-gated OpenDAL app tests, generated
  contract check, typecheck, `build:wasm`, `build`, and smoke.
- `apps/desktop/README.md` no longer claims a TypeScript mock backend; it
  documents the WASM runtime and the `build:wasm` → `build` → `smoke` sequence.
- Tauri packaging is fixed: `tauri.conf.json` `beforeBuildCommand` is
  `npm run build:app`, which runs `build:wasm` first, and
  `apps/desktop/scripts/build.mjs` now *throws* when `src/wasm/` is missing
  instead of warning.

Exit criteria — met:

- Clean checkout documents exactly how to build WASM and desktop assets.
- `cargo check --workspace` is green.
- `npm run typecheck` is green.
- Stale docs no longer contradict the actual runtime.

## Phase 1: Delete Bad Pins — done

Goal: remove tests and docs that force the current bad architecture.

Deleted or quarantined: monolithic command-by-command app tests, tests that
duplicated command lists by hand, browser-mock tests and docs, and stale
overlapping product plans (archived under `docs/archive/`).

Kept: canonical encoding tests in `opendoc-format`, model validation tests in
`opendoc-core`, convergence tests in `opendoc-merge`, formula parser/evaluator
tests, and import/export fixture tests.

Caveat still open: tests that construct `OpenDocApp::new_sample()` remain
(`files.rs`, `annotation_commands.rs`, `editor.rs`), because `new_sample` is
still the default constructor. See [Open Problems](#open-problems).

Exit criteria — met:

- Remaining tests describe desired invariants.
- No test requires a specific bad file layout.
- The suite can be temporarily smaller, but it must be honest.

## Phase 2: Extract The App Monolith — done for `opendoc-app`

Goal: break `opendoc-app/src/lib.rs` into cohesive modules without keeping
compatibility shims unless needed for one short migration step.

Extraction order (all six steps complete):

1. Spreadsheet model/evaluation/import-export → `opendoc-spreadsheet`, with
   one workbook-wide evaluator and no duplicated cell evaluation paths.
2. Render projection code → `opendoc-render`.
3. Application state and undo/session bookkeeping → `opendoc-app`.
4. Command parsing/dispatch/result DTOs → `opendoc-api` (+ dispatch in
   `opendoc-app/src/dispatch.rs`).
5. Runtime profile/session/authorization/share/relay DTOs → `opendoc-api`.
6. `opendoc-app-api` deleted.

Rules:

- Prefer typed command enums over stringly typed command dispatch internally.
  Done — the legacy string match is gone.
- JSON command dispatch is an edge adapter, not the core API. Done.
- App construction must distinguish `new_empty_document`,
  `new_empty_workbook`, `new_sample_document`, and `open_repository`.
  **Not done** — see [Open Problems](#open-problems).
- Rendering must be pure projection. It must never force recalculation or
  persistence side effects.
- Spreadsheet recalculation must be explicit or occur inside spreadsheet
  service mutation boundaries, not during arbitrary document projection.

Exit criteria:

- **Partly met:** no Rust source file over 2,000 lines without an explicit
  exception. True inside `opendoc-app`, `opendoc-api`, `opendoc-spreadsheet`,
  and `opendoc-render`; false for the five older-crate files tabulated in
  [Open Problems](#open-problems). Those are the declared outstanding
  exceptions, not accepted permanent ones.
- **Met:** `OpenDocApp` is a facade, not the owner of every domain algorithm.
- **Met:** there is one source of truth for spreadsheet evaluation.
- **Met:** Tauri and WASM call the same Rust facade.

Exit evidence, re-verified 2026-09-09:

- `crates/opendoc-app/src/lib.rs` is 184 lines and exports a facade over domain
  modules and application services.
- Runtime DTOs, command policy, typed command arguments, JSON command parsing,
  and generated frontend bindings are owned by `opendoc-api`.
- Command behavior dispatch lives in `opendoc-app/src/dispatch.rs` (849 lines)
  and routes parsed typed commands without a legacy stringly command path.
- App orchestration is split behind focused services for spreadsheet mutation,
  source invalidation, document operation application, operation journaling,
  lifecycle reset, blob lifecycle mutation, projection, rendering, signing,
  audit projection, editor commands, editor selection, image block
  construction, import/export, and repository persistence.
- `opendoc-spreadsheet` is the single spreadsheet model/evaluator crate and no
  longer exports temporary app-era `App*` compatibility aliases.
- Every `crates/opendoc-app/src/*.rs` file is under 2,000 lines (largest:
  `editor.rs` at 1,942), as is every primary desktop TypeScript source file
  (largest: `main.ts` at 1,871).

## Phase 3: Make Contracts Generated — done

Goal: eliminate command/API duplication.

Verified 2026-09-09.
`cargo run -p opendoc-api --bin generate_command_contract` writes, from Rust
command metadata in `opendoc-api`:

- `apps/desktop/commands.v0.json`
- `apps/desktop/src/generated/commands.ts` (command names, arg/result types,
  and the tagged `AppCommandResult` union), plus `audit.ts`, `blob.ts`,
  `citation.ts`, `document.ts`, `editor.ts`, `runtime.ts`, `spreadsheet.ts`
- the fenced generated command reference in `docs/APP_API_CONTRACT_V0.md`
  between the `BEGIN`/`END GENERATED COMMAND REFERENCE` markers

`npm run verify` runs `npm run generate:commands -- --check` and fails on
drift. Command policy (undoable, allowed-without-open-document, authorization
action) lives in the same Rust metadata and is read by typed dispatch. Tests
guard drift in both directions: every typed variant must have Rust metadata,
and every Rust metadata command must have a typed parser. The three former
TS-only commands (`import_bibliography_text`, `insert_spreadsheet_row_at`,
`insert_spreadsheet_column_at`) are absent from the tree.

Exit criteria — met:

- Adding a command requires one source change.
- TypeScript and Rust cannot disagree on command names or argument names.
- Prose docs no longer serve as executable truth.

## Phase 4: Rebuild Verification — done

Goal: replace broad brittle tests with layered tests that make bad states hard
to express.

Test layers in place: model invariants, operation pre/postconditions, merge
convergence, storage save/open/candidate-head/corruption, spreadsheet
parser/evaluator/dependency-graph/import-export, JSON command round-trip
through `opendoc-api`, jsdom WASM smoke against the real dispatcher, and the
Tauri native check. Tests are inline `#[cfg(test)]` modules; there are no
`tests/` integration directories.

Exit criteria — met:

- `cargo test --workspace` is meaningful and green.
- `npm run verify` generates WASM before depending on it.
- CI (`.github/workflows/desktop.yml`) mirrors the documented clean-checkout
  build and also runs the native check.

## Phase 5: Rework Frontend Around The Contract — in progress

Goal: keep TypeScript small and focused.

Done:

- All document/spreadsheet changes go through generated command bindings.
- Spreadsheet address/range parsing, selection movement, range membership,
  numeric range summary, TSV copy/paste expansion, range clearing, range
  formatting, row/column derivation, freeze derivation, and filter range
  derivation are Rust commands, not `main.ts` code.
- Document tree traversal for selection, mark projection, block-style
  application, list indent/outdent clamping, word/character counts, and the
  footnote-citation workflow are Rust-owned.
- `DocumentEditor` is the only contenteditable integration point.
- `npm run build` fails with the exact `build:wasm` command when `src/wasm/`
  is absent.
- Generated-artifact policy settled: commit generated TS bindings under
  `src/generated/`, do not commit `.wasm`.

Remaining:

- Split `apps/desktop/src/main.ts` into:
  - app bootstrap
  - document screen
  - spreadsheet screen
  - toolbar/menu bindings
  - panels
  - dialogs
  - runtime/status
- Move find/replace matching (`findMatches`) out of TypeScript and behind a
  Rust command.
- Remove any remaining TypeScript document mutation helpers uncovered by the
  split.

Exit criteria:

- TypeScript has no app-domain duplicate logic. **Not yet** — find/replace.
- Browser and Tauri use the same command bindings. **Met.**
- Clean checkout browser build works after documented commands. **Met.**

## Phase 6: Correct-By-Design Domain Model — not started

Goal: make invalid source states unrepresentable where practical.

Tasks:

- Replace ad hoc string IDs with typed IDs per domain:
  `DocumentId`, `BlockId`, `InlineId`, `SheetId`, `RowId`, `ColumnId`,
  `CellId`, `BlobHash`, `OperationId`, `ActorId`.
- Use constructors and smart types for ranges, marks, list numbering, table
  shapes, formulas, named ranges, and repository locators.
- Separate source state from projections and caches:
  - source document/workbook
  - render HTML
  - formula computed values
  - citation labels
  - runtime/session state
  - repository context
- Make sample fixtures explicit test/demo data, never default runtime state.
  This subsumes the `new_sample()` problem in [Open Problems](#open-problems).
- Define operation application so every mutation validates before commit and
  validates after commit.

Exit criteria:

- Source state can be validated without UI/runtime context.
- Caches can be dropped and rebuilt deterministically.
- Operation application cannot silently produce invalid documents.

## Phase 7: Real Backend Roadmap — designed, not implemented

Goal: define what "backend" means beyond in-process Tauri/WASM.

Local-first backend:

- `opendoc-store` remains the durable local repository backend.
- Tauri only grants native filesystem access and passes paths/bytes to Rust.
- Browser storage gets a Rust-defined adapter, likely IndexedDB through WASM
  bindings, after the storage trait is clean.

Service backend:

- Add `opendoc-service` only after typed operations and storage are stable.
- Implement HTTP/WebSocket transport, auth boundary, permissions, presence,
  document lookup, and sync relay.
- Relay must integrate operations or commit candidates; it must not merely
  classify placeholder envelopes.
- ADR 0004 records the service backend boundary and creation gate:
  `docs/adr/0004-service-backend-boundary.md`. The ADR exists; no
  `opendoc-service` crate does.

Exit criteria:

- The word backend maps to explicit crates and deployment modes.
- Multi-user service behavior is not faked in the local app facade.

## Phase 8: Documentation Cleanup — archiving done, upkeep ongoing

Goal: make docs useful again.

Done:

- One canonical restructure plan (this file), with
  `docs/GOOGLE_DOCS_PARITY_TODO.md` explicitly marked historical audit input in
  its own header.
- `GOOGLE_DOCS_EQUIVALENT_*`, `OPENDOC_*_PLAN`, `OPEN_SOURCE_DOCS_*`,
  `IMPLEMENTATION_PLAN.md`, `PRODUCT_COMPLETION_PLAN.md`, and `forme.md` are
  archived under `docs/archive/`.
- `docs/APP_API_CONTRACT_V0.md` has a generated command reference section.
- `apps/desktop/README.md` matches WASM/Tauri reality.

Remaining:

- Keep regenerating the API contract doc as the command surface changes; the
  hand-written prose outside the generated markers still needs a pass once
  Phase 6 typed IDs land.

Exit criteria:

- New contributors can identify the source of truth in under one minute.
- No doc claims a TS mock backend unless one exists.
- No doc requires preserving the old monolithic app API.

## Suggested Immediate PR Stack

1. `restructure-plan`: add this plan, decide untracked file fate. Done.
2. `build-green`: fix stale spreadsheet evaluation call, run format, get
   `cargo check --workspace` and `npm run typecheck` green. Done.
3. `docs-truth`: update README/API docs to match current WASM/Tauri reality.
   Done; keep regenerating API docs as the contract changes.
4. `test-triage`: delete/quarantine monolith tests that pin bad structure.
   Done; verification is layered through workspace tests, generated contract
   checks, WASM build, desktop build, and desktop smoke.
5. `spreadsheet-crate`: extract spreadsheet model/evaluator/import-export.
   Done, including public-API narrowing and removal of app-era aliases.
6. `render-crate`: extract pure HTML/debug projections. Done.
7. `api-contract-gen`: move command metadata to Rust, generate TypeScript
   bindings and command registry from it. Done.
8. `app-facade`: split app state/services and delete `opendoc-app-api`. Done.
9. `frontend-split`: split TypeScript UI modules around generated bindings.
   **Next.**
10. `core-crate-split`: bring `opendoc-merge`, `opendoc-import`, and
    `opendoc-store` under the 2,000-line target.
11. `explicit-constructors`: replace `new_sample()` as the default app
    constructor.
12. `dependency-prune`: remove stale app-api dependencies after ownership
    moves. Done.
13. `verify-ci`: make clean-checkout verification deterministic, starting by
    committing the untracked support files.
14. `service-design`: add an ADR for the real service backend before coding it.
    Done in `docs/adr/0004-service-backend-boundary.md`.

## Next Architecture Slice

The restructure pass is complete enough to stop preserving compatibility with
the discarded shape. The next work is ordinary architecture evolution, not
cleanup of the previous mock/app-api boundary:

1. Split `apps/desktop/src/main.ts` by screen/runtime responsibility while
   keeping generated command bindings as the only Rust command entry point.
2. Move find/replace matching, and any other document-tree UI derivation the
   split exposes, behind Rust projections or generated helpers.
3. Apply the same extraction discipline used on `opendoc-app` to
   `opendoc-merge`, `opendoc-import`, and `opendoc-store`.
4. Replace `new_sample()` as the default constructor with explicit
   empty/sample/open-repository construction.
5. Continue narrowing crate public surfaces and retiring the
   `#[allow(dead_code)]` quarantine in `opendoc-spreadsheet` as new call sites
   prove stable.
6. Add service-backend code behind the boundary in
   `docs/adr/0004-service-backend-boundary.md`, with persistence and sync
   behavior validated through app/service tests rather than frontend fixtures.

## Outstanding Work

Unchecked items are the live list. Each was confirmed against the tree on
2026-09-09.

### Frontend

- [ ] Split `apps/desktop/src/main.ts` (1,871 lines) into bootstrap, document
  screen, spreadsheet screen, toolbar/menu bindings, panels, dialogs, and
  runtime/status modules.
- [ ] Move find/replace text matching (`findMatches`, `main.ts`) behind a Rust
  command and generated binding.
- [ ] Remove any remaining TypeScript document mutation helpers exposed by the
  `main.ts` split.

### Rust file size

- [ ] `crates/opendoc-merge/src/lib.rs` — 12,227 lines (~3,424 non-test).
- [ ] `crates/opendoc-import/src/lib.rs` — 5,053 lines (~2,090 non-test).
- [ ] `crates/opendoc-store/src/lib.rs` — 4,744 lines (~2,537 non-test).
- [ ] `crates/opendoc-import/src/docx.rs` — 2,332 lines (~2,260 non-test).
- [ ] `crates/opendoc-core/src/lib.rs` — 2,690 lines (~1,316 non-test); over
  target only because of its inline test tail. Split the tests into a
  `*_tests.rs` sibling or record an explicit exception.

### Domain model and construction

- [ ] Replace `OpenDocApp::new_sample()` as the default constructor with
  `new_empty_document`, `new_empty_workbook`, `new_sample_document`, and
  `open_repository`; update the Tauri and WASM boot paths and the tests that
  depend on sample state.
- [ ] Introduce typed per-domain IDs (Phase 6).
- [ ] Introduce smart constructors for ranges, marks, list numbering, table
  shapes, formulas, named ranges, and repository locators (Phase 6).
- [ ] Enforce validate-before-commit and validate-after-commit in operation
  application (Phase 6).

### Crate surface and hygiene

- [ ] Retire the five `#[allow(dead_code)]` module quarantines in
  `crates/opendoc-spreadsheet/src/lib.rs` by wiring them to real entry points
  or deleting them.
- [ ] Continue moving command orchestration out of `impl OpenDocApp` method
  groups into typed service objects.

### Repository hygiene

- [ ] Commit the untracked files the build already depends on:
  `apps/desktop/package-lock.json`, `apps/desktop/scripts/build-wasm.mjs`,
  `apps/desktop/scripts/smoke.mjs`, `apps/desktop/src/generated/`,
  `docs/GOOGLE_DOCS_PARITY_TODO.md`, `docs/archive/`,
  `docs/adr/0004-service-backend-boundary.md`, and this plan.

### Service backend

- [ ] Create `opendoc-service` behind the ADR 0004 gate with real HTTP/WebSocket
  transport, auth boundary, permissions, presence, lookup, and a sync relay
  that integrates operations instead of classifying envelopes.
- [ ] Add a Rust-defined browser storage adapter (likely IndexedDB via WASM)
  once the storage trait is clean.

## Completed Work

The authoritative record of what has been done. Facts here are not repeated in
the status block, the open-problem list, or the phase bodies.

### Build and repository hygiene

- [x] Build green from current tree.
- [x] Formatting green.
- [x] Untracked file policy resolved (decision recorded in Phase 0; the commit
  itself is still in [Outstanding Work](#outstanding-work)).
- [x] Stale README/mock-backend docs fixed.
- [x] Monolith tests triaged.
- [x] Tauri packaging runs `build:app` so WASM is generated before packaging,
  and `build.mjs` fails instead of warning when `src/wasm/` is missing.
- [x] WASM clean-checkout build documented and verified.
- [x] Tauri native check preserved.
- [x] Old duplicate plans archived or deleted.
- [x] Stale app-api dependencies pruned after extraction.
- [x] Spreadsheet-era direct dependencies (`csv`, `base64`, `calamine`,
  `rust_xlsxwriter`) and unused `regex` pruned from the app boundary;
  spreadsheet dependencies now sit behind `opendoc-spreadsheet`.
- [x] Service backend boundary ADR added before introducing `opendoc-service`.

### Spreadsheet extraction

- [x] Spreadsheet extracted.
- [x] Spreadsheet address/range helpers extracted from app root.
- [x] Spreadsheet model validation/axis/graph helpers extracted from app root.
- [x] Spreadsheet sheet mutation helpers extracted from app root.
- [x] Spreadsheet cell format behavior extracted from app root.
- [x] Google Sheets spreadsheet adapter extracted from app root.
- [x] Spreadsheet named range/dependency DTOs extracted from app root.
- [x] Spreadsheet workbook/sheet/cell DTOs extracted from app root.
- [x] Spreadsheet workbook methods split from model definitions.
- [x] Spreadsheet lookup/rank functions split from function dispatcher.
- [x] Standalone `opendoc-spreadsheet` crate created and checked.
- [x] App facade switched to `opendoc-spreadsheet` with duplicate local modules
  deleted.
- [x] `opendoc-spreadsheet` public surface narrowed after app migration.
- [x] Spreadsheet internals renamed from app-era `App*` naming to domain names.
- [x] Temporary `App*` spreadsheet aliases removed from `opendoc-spreadsheet`;
  app-facing aliases now live only in `opendoc-app`.
- [x] Spreadsheet operation replay/merge helper extraction.

### Render extraction

- [x] Rendering extracted.
- [x] Base64 helpers removed from render module.
- [x] Document and footnote HTML rendering extracted to `opendoc-render`.
- [x] Spreadsheet grid HTML rendering extracted to `opendoc-render`.
- [x] `opendoc-app` keeps only render wrapper methods that adapt app state and
  map render errors into API errors.

### App monolith extraction

- [x] Runtime-facing `opendoc-app` facade crate introduced.
- [x] App implementation moved from `opendoc-app-api` into `opendoc-app`.
- [x] Temporary `opendoc-app-api` compatibility wrapper deleted; the empty
  `crates/opendoc-app-api/` directory was removed from disk on 2026-09-09.
- [x] Warning projection DTO and helper extraction (`warning.rs`).
- [x] Repository target routing extraction (`repository.rs`).
- [x] Repository persistence/save/open/scan/merge IO extraction
  (`repository_io.rs`).
- [x] Operation record/envelope/payload validation extraction (`operation.rs`).
- [x] Blob sidecar import/export/merge helper extraction (`blob_io.rs`).
- [x] Blob projection DTOs split into `blob.rs`.
- [x] Blob lifecycle/image-block app method extraction (`blob_commands.rs`).
- [x] Document/spreadsheet import/export app method extraction
  (`import_export.rs`).
- [x] Document/blob signing app method and typed signing helper extraction
  (`signing.rs`).
- [x] Recent-document projection DTO extraction (`recent.rs`).
- [x] Document projection DTO and app/core conversion extraction
  (`document.rs`).
- [x] Document structure/editing app method extraction
  (`document_commands.rs`).
- [x] Annotation/citation/bibliography app method extraction
  (`annotation_commands.rs`).
- [x] Spreadsheet app method extraction (`spreadsheet_commands.rs`).
- [x] Audit/signature projection DTOs split into `audit.rs`.
- [x] Audit view projection behavior and deleted-object restore lookup helpers
  split into `audit_view.rs`.
- [x] App state/checkpoint/apply orchestration extracted from the app root.
- [x] Command result DTO extracted from the app root.
- [x] Annotation support helpers extracted from the app root.
- [x] Projection support helpers extracted from the app root.
- [x] Clock helper extracted from the app root.

### Typed services behind the facade

- [x] Spreadsheet mutation transaction service added for cache invalidation and
  evaluation policy (`spreadsheet_service.rs`).
- [x] Spreadsheet command call sites moved off manual begin/finish mutation
  pairs onto one transaction API.
- [x] Source mutation/projection invalidation helper extracted
  (`mutation_service.rs`).
- [x] Document operation application service object extracted
  (`document_service.rs`).
- [x] App/blob/spreadsheet operation journal service object extracted
  (`journal_service.rs`).
- [x] Document lifecycle reset service object extracted, including
  imported-blob restoration for Word import adoption (`lifecycle_service.rs`).
- [x] Blob lifecycle state mutation service object extracted
  (`blob_service.rs`).
- [x] UI-facing document/blob projection service object extracted
  (`projection_service.rs`).
- [x] App render service object extracted (`render_service.rs`).
- [x] Document/blob signature service object extracted (`signing_service.rs`).
- [x] Audit projection service object extracted (`audit_view.rs`).
- [x] Editor command service object extracted (`editor.rs`).
- [x] Editor selection projection service object extracted
  (`editor_selection_service.rs`).
- [x] Image block construction service object extracted
  (`image_block_service.rs`).
- [x] Import/export orchestration service object extracted
  (`import_export_service.rs`).
- [x] Repository persistence orchestration service object extracted
  (`repository_io.rs`).
- [x] App implementation services extracted behind `opendoc-app`.

### Generated contract

- [x] API contracts generated.
- [x] TypeScript command bindings generated.
- [x] Typed Rust command argument DTOs owned by `opendoc-api`.
- [x] Typed Rust command enum owned by `opendoc-api`.
- [x] Command name/spec mapping owned by `opendoc-api`.
- [x] Command metadata type definitions split from the large command registry
  (`opendoc-api/src/command_types.rs`).
- [x] JSON command parser owned by `opendoc-api`
  (`opendoc-api/src/command_parse.rs`).
- [x] API doc command reference generated from Rust command metadata.
- [x] Command metadata source moved from desktop JSON to Rust.
- [x] Command policy string lists replaced by Rust command metadata.
- [x] Runtime profile/session/share/lookup DTOs extracted to `opendoc-api`.
- [x] JSON command argument parsing extracted from app root to `opendoc-api`.
- [x] Frontend DTO types generated or Rust-owned for the current Rust-facing
  API surface.
- [x] Runtime frontend DTO types generated (`generated/runtime.ts`).
- [x] Editor input/selection DTOs moved to `opendoc-api` and generated
  (`generated/editor.ts`).
- [x] Citation command item DTO moved to `opendoc-api` and generated
  (`generated/citation.ts`).
- [x] Spreadsheet frontend DTO types generated (`generated/spreadsheet.ts`).
- [x] Audit, blob/signature, warning, operation-record, and recent-document
  frontend DTO types generated (`generated/audit.ts`, `generated/blob.ts`).
- [x] Document, editor-result, citation projection, comment, suggestion, block,
  inline, and footnote frontend DTO types generated (`generated/document.ts`).
- [x] Tagged command result frontend union generated (`generated/commands.ts`).
- [x] Contract staleness check wired into `npm run verify`
  (`generate:commands -- --check`).
- [x] TS-only command drift removed: `import_bibliography_text`,
  `insert_spreadsheet_row_at`, `insert_spreadsheet_column_at`.

### Typed dispatch migration

- [x] First typed dispatch slice added for low-risk commands, with migrated
  legacy string-match arms removed and metadata drift guarded by tests.
- [x] Runtime command group moved onto typed dispatch.
- [x] Import/export/render command group moved onto typed dispatch.
- [x] Blob/image/signature command group moved onto typed dispatch.
- [x] Repository command group moved onto typed dispatch.
- [x] Undo/redo and document-structure command batch moved onto typed dispatch.
- [x] Table row/cell commands moved onto typed dispatch.
- [x] Citation/comment/suggestion command batch moved onto typed dispatch.
- [x] Browser editor input/mark commands moved onto typed dispatch.
- [x] Bibliography/reference, inline/block update, and text-mark commands moved
  onto typed dispatch.
- [x] Spreadsheet command batch moved onto typed dispatch.
- [x] Stringly app-api dispatch replaced by typed command dispatch; the legacy
  string match is gone.
- [x] App command behavior dispatch extracted to `opendoc-app/src/dispatch.rs`.
- [x] Reverse command metadata drift guard added so every Rust metadata command
  has a typed parser path.
- [x] Typed dispatch policy reads from the parsed command metadata spec instead
  of a second string lookup.
- [x] App-api typed command enum/argument structs generated from or owned by
  `opendoc-api`.

### Frontend semantics moved into Rust

- [x] Spreadsheet selection range/summary semantics moved behind
  `describe_spreadsheet_selection` and a generated DTO.
- [x] Spreadsheet TSV copy/paste expansion, selected-range clearing, and
  selected-range formatting moved behind Rust commands
  (`copy_spreadsheet_selection_tsv`, `paste_spreadsheet_tsv`,
  `clear_spreadsheet_selection`, `set_spreadsheet_selection_format`).
- [x] Spreadsheet selection movement and row/column/freeze/filter derivation
  moved behind Rust commands (`reduce_spreadsheet_selection` owns arrow, edge,
  Home, select-all, mouse focus, and name-box reduction).
- [x] Document word/character count moved into Rust projection on
  `AppDocument`.
- [x] Document toolbar block style and focused-inline mark projection moved
  into Rust DTOs (`style_value`, `mark_kinds`, `mark_values`).
- [x] Editor selected block IDs and inline range normalization moved behind
  `describe_editor_selection`.
- [x] Editor-selection block style application and list indent/outdent clamping
  moved behind Rust commands.
- [x] Footnote-citation insertion workflow moved behind a Rust command.
- [x] Accept/reject all suggestions workflow moved behind Rust commands.
- [x] Select-all document selection derivation moved behind a Rust command.
- [x] Comment and suggestion anchor display labels moved into Rust projection
  DTOs.
- [x] Frontend shell split enough to bring `apps/desktop/src/main.ts` under the
  2,000-line target. (The by-surface split is a separate, still-open item.)
