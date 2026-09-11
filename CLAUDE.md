# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

OpenDoc is a research prototype of a Rust-first, open-source alternative to Google Docs/Sheets: collaborative rich text plus spreadsheets, local-first storage in content-addressed object stores, and cryptographically signed document versions. It is not yet a product — `RESEARCH_PLAN.md` states the goals, `docs/adr/` records the accepted design decisions, and `docs/RESTRUCTURE_PLAN.md` is the **active** tracking plan for in-flight cleanup (`docs/GOOGLE_DOCS_PARITY_TODO.md` is a historical audit input, not the live list).

Backwards compatibility is explicitly **not** required. Deleting tests, commands, fields, and adapters that preserve bad boundaries is the intended move, not a regression.

## Commands

All npm commands run from `apps/desktop/`.

**Always build in release mode.** Debug builds are slow here and never useful — pass `--release` to every cargo invocation.

```sh
# Full gate (fmt, clippy -D warnings, workspace tests, opendal tests,
# generated-binding check, tsc, wasm build, static build, jsdom smoke)
npm run verify

# Rust only
cargo test --release --workspace
cargo test --release -p opendoc-merge convergence          # single crate / filtered test
cargo test --release -p opendoc-app --features opendal-store   # feature-gated store tests
cargo clippy --release --workspace --all-targets -- -D warnings

# Frontend / WASM
npm run build:wasm      # cargo build opendoc-wasm + wasm-bindgen into src/wasm/
npm run build           # static dist/ via scripts/build.mjs (no bundler needed)
npm run smoke           # jsdom test driving the built UI against the real WASM core
npm run typecheck

# Regenerate the Rust-owned command contract + TS bindings + docs
npm run generate:commands
npm run generate:commands -- --check   # what CI/verify runs

# Run it
npm run dev             # vite on 0.0.0.0:10084 (browser/WASM mode)
npm run tauri dev       # native shell (needs GTK/WebKit, see below)
npm run preflight && npm run native-check   # check native prerequisites first
```

Tests are inline `#[cfg(test)]` modules (sometimes in dedicated `*_tests.rs` files inside `src/`); there are no `tests/` integration directories.

Native Tauri builds need `libgtk-3-dev libwebkit2gtk-4.1-dev pkg-config` (Ubuntu) or equivalents. `npm run build` deliberately works without those and without a bundler, so the WASM/browser path is always testable. `wasm-bindgen-cli` must match the `wasm-bindgen` version pinned in `Cargo.lock`.

## Architecture

### One command surface, three runtimes

Everything the UI can do is a named command in the registry in `opendoc-api/src/commands.rs`, dispatched through `OpenDocApp::dispatch_command` (`opendoc-app/src/dispatch.rs`). There is exactly one transport entry point per runtime:

- **Tauri**: `apps/desktop/src-tauri/src/` exposes a single generic `dispatch` command plus native-only extras (file dialogs, file IO, window lifecycle).
- **Browser/WASM**: `opendoc-wasm` is a thin wasm-bindgen wrapper over the same `OpenDocApp`.
- **Frontend**: `apps/desktop/src/invoke.ts` picks Tauri or WASM at runtime. No document logic lives there.

A future `opendoc-service` (HTTP/WebSocket, auth, server-enforced permissions) is deliberately **not** created yet — see `docs/adr/0004`. The `relay_runtime_sync` / share / presence DTOs exist as contract design but are local simulations; do not describe them as a working backend.

### The contract is generated, never hand-written

`cargo run -p opendoc-api --bin generate_command_contract` writes, from Rust command metadata:

- `apps/desktop/commands.v0.json`
- `apps/desktop/src/generated/*.ts` (command bindings + DTO types)
- the generated command reference section of `docs/APP_API_CONTRACT_V0.md`

Adding or changing a command means editing the Rust metadata and regenerating. Never hand-edit the generated files; `npm run verify` fails on drift. `apps/desktop/src/types.ts` and `src/commands.ts` are thin compatibility barrels over `src/generated/`.

### Rust owns semantics; TypeScript owns the DOM

This is the project's central rule (`docs/RESTRUCTURE_PLAN.md` "Non-Negotiable Direction"):

- Rust owns schemas, validation, operations, merge, storage, signing, import/export, spreadsheet evaluation, and contract generation.
- TypeScript owns DOM integration, event mapping, selection mapping, and visual UI state — **not** document semantics, spreadsheet semantics, storage semantics, or duplicate command schemas. Address parsing, range normalization, clipboard semantics, and selection reducers belong in Rust.
- `invoke.ts` = transport only; `editor.ts` = DOM selection/input adapter only; `main.ts` = screen composition and event wiring.

### Crates

| Crate | Owns |
| --- | --- |
| `opendoc-core` | Canonical document model: `Document`, `Block`/`BlockKind`, `Inline`, `Mark`, comments, suggestions, citations, `StableId`, `HashRef`, warnings |
| `opendoc-spreadsheet` | Workbook model, formula parser/evaluator, dependency graph + recalc, formats, named ranges, Google/CSV/XLSX helpers |
| `opendoc-merge` | Merge/rebase over typed operations; DOM-independent, largest convergence test suite in the repo |
| `opendoc-format` | Deterministic CBOR canonical records and debug JSON projections |
| `opendoc-store` | Object-store abstractions (`FlatObjectStore`, `LocalObjectStore`, optional `OpenDalObjectStore`), repository, manifests, heads/candidate heads, tombstones |
| `opendoc-sign` | OpenSSH-key signing of canonical source state and of typed blob content |
| `opendoc-import` | Google Docs/Sheets JSON, `.docx`, `.doc` adapters |
| `opendoc-citations` | Citation model/parse/render (hayagriva-backed) |
| `opendoc-render` | Pure HTML/debug projections — must never mutate source state or force recalculation |
| `opendoc-api` | Command registry/specs, typed command enum, arg DTOs, JSON parsing, contract generator binary |
| `opendoc-app` | Application facade composing domain services; the runtime-facing entry point |
| `opendoc-wasm` | wasm-bindgen adapter (`cdylib`) |

### Inside `opendoc-app`

`OpenDocApp` is a facade, not a god object. State is in `state.rs`; behaviour is split into typed services (`document_service`, `spreadsheet_service`, `blob_service`, `signing_service`, `projection_service`, `audit_view`, `journal_service`, `lifecycle_service`, …) with command method groups in `*_commands.rs`. Keep every file under the 2,000-line target — when a module grows past it, extract a service rather than appending.

Command specs carry policy flags (`undoable`, `allowed_without_open_document`, required permission action) that `dispatch.rs` enforces, including undo checkpointing and time-window coalescing. Persisted state is source-level: formulas, marks, and formatting are stored and signed; computed values and rendered HTML are projections.

## Working conventions

- Changing the app API means updating the Rust projection tests, the generated TS bindings, and the WASM/Tauri transport checks together — `npm run verify` is the gate that proves it.
- Prefer the Rust/WASM core over adding TypeScript. Do not introduce TS mocks for things the core should compute.
- `apps/desktop/src/wasm/` and `dist/` are generated and gitignored; `apps/desktop/src/generated/` is generated but tracked.
