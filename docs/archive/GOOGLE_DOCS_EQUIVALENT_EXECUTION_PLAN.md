# OpenDoc Google Docs Equivalent Execution Plan

Status: canonical tracked execution plan.

Purpose: collect the product, architecture, storage, signing, citation, spreadsheet, frontend, server, and validation decisions into one plan that is concrete enough to know when the Google Docs equivalent product is done.

Primary detailed references:

- Product/design contract: `docs/GOOGLE_DOCS_EQUIVALENT_PLAN.md`
- Master checklist: `docs/GOOGLE_DOCS_EQUIVALENT_MASTER_PLAN.md`
- Initial implementation plan: `docs/IMPLEMENTATION_PLAN.md`
- Research summary: `docs/research/summary.md`
- Schemas: `docs/schema/document-v0.md`, `docs/schema/spreadsheet-v0.md`, `docs/schema/storage-manifest-v0.md`, `docs/schema/citation-v0.md`, `docs/schema/signed-manifest-v0.md`

## Done Definition

This plan is complete only when all of these are true:

- A user can create, edit, save, close, reopen, import, export, verify, and audit a rich document in the Tauri app.
- The same source schema and app API can run in browser mode without semantic forks.
- Local on-disk object storage is first-class and tested.
- S3/OpenDAL-style object storage is supported or has a compatible tested adapter boundary.
- 1-3 concurrent active editors and about 5 viewer/editor-capable clients converge automatically in tests.
- No v0 merge requires manual conflict resolution; degraded cases produce deterministic warnings and openable documents.
- Every v0 document node is editable: paragraphs, headings, lists, tables, links, marks, comments, suggestions, citations, equations, images, attachments, and page breaks.
- Spreadsheet v0 supports multi-sheet sparse workbooks, formulas, formatting, named ranges, and deterministic formula evaluation.
- Canonical storage is binary, not JSON. JSON is only for debug, command contracts, tests, and import/export adapters.
- Unsigned documents open normally, while signed documents show a clear trust state.
- Manifest, version, blob, and typed semantic signing APIs are algorithm-agile.
- Google Docs/Sheets API-shaped import/export fixtures prove the selected compatibility subset.
- Unsupported import/storage/merge cases fail gracefully with warnings or explicit aborts according to fixture expectations.

## Tracking Checklist

Each item below is complete only when code, tests/fixtures, docs, degradation behavior, storage impact, signing impact, and import/export impact are accounted for where relevant.

- [ ] Canonical binary source schemas for documents, spreadsheets, citations, blobs, manifests, operation segments, lookup records, signatures, packs, and tombstones.
- [ ] Operation-backed rich document editing for every v0 node, with immediate rendering independent of persistence batching.
- [ ] Fully automatic operation-level merge for rich documents with formatting, comments, suggestions, citations, equations, tables, images, and attachments.
- [ ] Deterministic merge scenarios and fuzz tests for 1-3 active editors plus viewer/editor-capable replicas.
- [ ] Local on-disk content-addressed repository with save/open/reopen, UUID/DOI lookup, candidate heads, reconciliation, snapshots, operation segments, and crash-safe writes.
- [ ] Pack files and compaction to avoid pathological small-file behavior on local disks without changing operation semantics.
- [ ] Flat S3/OpenDAL-compatible object-store mode, including serverless lookup by scanning when no server is available.
- [ ] Content-addressed binary blobs, shallow clone placeholders, sidecar exact-byte signatures, reusable blob signatures, and archive/tape tombstones.
- [ ] Storage-independent typed semantic signing APIs for image profiles and FASTQ profiles.
- [ ] Rust-native OpenSSH-compatible document/version signing with multiple signatures, algorithm agility, trust states, and audit/recovery views.
- [ ] Document-local citation database with structured citation labels, `citum` rendering, and future CSL-JSON adapter boundary.
- [ ] Spreadsheet v0 with sparse sheets, stable cell identities, formatting, formulas, named ranges, deterministic evaluation, and source-only signatures.
- [ ] Google Docs/Sheets API-shaped import/export fixtures plus practical `.doc/.docx` import.
- [ ] Tauri v2 desktop app for Linux, macOS, and Windows with GUI controls for the full docs-like schema.
- [ ] Browser app contract sharing the same source schema and app API, with browser signing explicitly postponed.
- [ ] HPC single-user web mode behind external authentication with disk and S3 access.
- [ ] Multi-user service mode with authentication, permissions, sharing, presence, sync relay, lookup acceleration, and optional commit serialization.
- [ ] Release verification suite covering binary golden fixtures, merge convergence, repository recovery, signing/tamper checks, spreadsheet formulas, citation rendering, import/export, GUI smoke, and native checks.

## Fixed Decisions

- Core implementation language: Rust.
- Frontend: hybrid TypeScript editor surface.
- Desktop shell: Tauri v2 for Linux, macOS, and Windows.
- Rendering is immediate and must not wait for commit batching.
- Keypress-level operations must be representable.
- Batching, packs, and compaction are storage/network optimizations, not editing semantics.
- Merge operates on operations/state, not on the DOM as source of truth.
- Formatting marks are first-class source ranges, not fragile rendered spans.
- Comments, suggestions, citations, equations, tables, and formatting are included in merge tests from the start.
- Invisible stable IDs may be used for anchoring, but normal document-content signatures exclude implementation-only IDs.
- Deletion removes content from current state while retained history keeps recovery/audit data.
- Backward compatibility is not required until the project leaves research mode.
- Permissions are not document-format semantics; they live in repository opening or a multi-user service.
- Users should never need to understand commit packs, compaction, shallow clone internals, candidate heads, or tombstones.

## Source Schema

Implement one canonical source model shared by desktop, browser, HPC, and service modes:

- document UUID, optional DOI, title, metadata, and provenance
- stable block, inline, row, column, cell, comment, suggestion, citation, equation, and attachment IDs
- rich document blocks: paragraphs, headings, lists, page breaks, tables, images, equations, and attachments
- inline content: text, links, citation labels, inline equations, comments/suggestion anchors, and typed labels
- text marks: bold, italic, underline, strike, code, superscript, subscript, color, background, font, and size
- comments and suggestions as signed source state
- deleted comments and resolved suggestions hidden by default but visible in audit/recovery views
- citations as structured labels backed by a document-local bibliography database
- equations as TeX/LaTeX source, with rendered MathML/PDF as projection only
- spreadsheets as sparse multi-sheet workbooks with formulas, dependencies, formatting, and named ranges
- attachment references by content hash, with availability state for shallow clones

Done when every source node round-trips through canonical binary encoding and invalid states are rejected or repaired with deterministic warnings.

## Operation And Merge Model

This is the highest-risk workstream.

Required:

- operations are the only merge unit
- local edits render immediately after operation application
- concurrent typing in the same paragraph converges
- formatting survives insertions, deletions, splits, joins, and overlapping mark ranges where possible
- comments and suggestions anchor to stable UUID-backed ranges, including cross-block ranges
- deleted-anchor cases deterministically move to the nearest surviving block/range or become audit-only with warnings
- suggestions support insert, delete, format, accept, reject, and provenance
- citations move as structured occurrences, not links
- equations merge as atomic source objects
- tables use stable row, column, and cell identities
- passive viewers use the same update path as editors where practical

Validation:

- realistic synthetic scenarios
- deterministic operation-level fuzzing for 1-3 active replicas
- passive-viewer-equivalent empty streams
- shuffled/rebatched replay
- schema validation after every operation
- byte-for-byte canonical convergence checks
- warning assertions for degraded merges

Done when all v0 operation classes are covered by merge scenarios and fuzz tests converge without manual conflict resolution.

## Version Control And Storage

Use a content-addressed repository that works locally first and maps to S3/OpenDAL later.

Required:

- immutable content-addressed objects
- canonical binary snapshots
- canonical binary operation segments
- commit manifests with parent links, branch, document UUID, snapshots, operation segments, blob references, lookup records, tombstones, and signatures
- keypress-level operations stored directly or packed into operation segments
- compare-and-swap head updates where available
- deterministic candidate heads when CAS is unavailable or races occur
- automatic reconciliation by fast-forward or operation-level merge
- UUID lookup and optional DOI alias lookup
- serverless lookup by repository/bucket scanning
- optional central lookup acceleration when a server exists
- content-addressed blobs for images and arbitrary binary objects
- shallow clone support with placeholders and warnings for missing blobs
- local pack files to mitigate many small files
- crash-safe compaction by write-new-pack, verify, then atomically swap index
- tape/archive tombstones recording where recoverable data exists

Done when tests prove create/open/save/reopen, concurrent candidate save reconciliation, missing blob handling, UUID/DOI lookup, pack compaction, and tombstone metadata.

## Signing And Audit

Unsigned documents open normally. Signatures are optional trust/compliance indicators for audit, scientific fraud review, patent precedence, and 21 CFR-style workflows.

Required:

- Rust-native OpenSSH-compatible signing by default
- optional `ssh-keygen` helper if useful
- multiple signatures per version
- algorithm agility in hash and signature envelopes
- manifest/version signatures over source state and retained reachable history
- exact-byte detached blob signatures keyed by content hash
- sidecar signatures for images and arbitrary binary objects
- typed semantic signatures independent of storage layout, including image profiles and FASTQ sequence-only/full-content profiles
- trust states: unsigned, signed, trusted, untrusted, broken
- audit/recovery view for warnings, deleted comments, resolved suggestions, deleted citations, operation history, signatures, and tombstones

Do not sign rendered output, computed formula values, rendered citation labels, rendered equation output, volatile UI state, browser caches, or import provenance. Formula signatures cover formula source.

Done when valid signatures verify after save/open, tampering is detected, multiple signatures verify independently, unsigned documents remain openable, and typed signature APIs can sign storage-independent byte or semantic profiles.

## Citations

Use a special citation label type, not normal links.

Required:

- document-local bibliography database
- citation occurrence labels that reference bibliography record IDs
- citation groups with locator/page metadata, prefixes, suffixes, suppress-author flags, and ordering
- `citum` as the first rendering/integration target
- rendered citation labels and bibliography output as projection/cache state outside signatures
- future CSL-JSON import/export adapter
- Paperpile-style Google Docs link metadata only as an import/export compatibility concern

Done when updating one bibliography record updates all dependent labels, citation labels survive save/open, merge tests cover citation movement and bibliography edits, and rendered labels are excluded from source signatures.

## Spreadsheets

Spreadsheet v0 includes formula evaluation.

Required:

- sparse multi-sheet workbook model
- stable sheet, row, column, and cell IDs
- typed values and formatting
- deterministic formula parser/evaluator
- dependencies and invalidation derived from formula source
- named ranges
- Google Sheets API-shaped import/export
- graceful warnings for unsupported formulas or import structures

Done when formula fixtures evaluate deterministically on supported platforms, cached computed values are excluded from signatures, dependencies update after edits, and import/export covers the selected v0 subset.

## Frontend And App Modes

First serious product target: Tauri app with hybrid TypeScript editor over the Rust app API.

Required:

- GUI controls for every v0 document and spreadsheet feature
- operation-backed editing, undo, redo, autosave, save, close, reopen, import, export, verify, and audit/recovery
- IME-safe editing and selection mapping
- paste normalization plus later structured paste/import
- keyboard shortcuts and menus
- warnings and signature indicators
- native file/repository picker
- recent documents
- Linux/macOS/Windows build prerequisites and native checks, including required assets

Browser mode must share the same API contract. Browser signing is postponed, but signed documents must still open and expose verification state where supported.

Done when the packaged app can handle a document containing every v0 schema feature and browser tests prove the same command contract.

## Server And Deployment Modes

Supported modes:

- local single-user disk mode: first-class default until S3 is common
- raw S3/OpenDAL mode: no coordination server, with CAS/listing semantics and simulated multi-user behavior
- HPC single-user web mode: runs behind external authentication, can access disk and S3, assumes one authenticated user with full access, and skips document-level permissions
- multi-user service mode: owns authentication, permissions, sharing, presence, sync relay, lookup acceleration, and optional server-managed commit serialization

Done when all modes share operation/storage semantics and integration tests prove local, HPC-style, and service-style app API behavior.

## Import And Export

Required:

- Google Docs API-shaped import/export for the selected document subset
- Google Sheets API-shaped import/export for the selected spreadsheet subset
- `.doc`/`.docx` import where practical using easy external tooling first
- comments, suggestions, equations, citations, and sheets fixtures
- high-risk unsupported imports abort instead of silently misrepresenting content
- lower-risk unsupported structures degrade with explicit warnings
- import provenance stored outside signed authored content

Done when realistic fixtures import to valid OpenDoc states, save/open after import works, exports preserve the v0 subset, and unsupported cases are explicit.

## Test Infrastructure

Required suites:

- binary schema golden fixtures
- document operation and merge scenario tests
- deterministic merge fuzz tests
- repository crash/recovery tests
- local disk and S3/OpenDAL conformance tests
- spreadsheet formula fixtures
- citation rendering fixtures
- import/export fixtures
- signing/tamper fixtures
- GUI smoke tests
- Tauri native checks
- browser/webview checks where practical

Done when CI proves convergence, schema validity, canonical encoding, save/open, signing, import/export, and graceful degradation invariants.

## Milestones

1. Schema and binary records: docs, sheets, citations, blobs, signatures, manifests, operation segments, lookup records, packs, and tombstones.
2. Operation-level merge: rich document scenarios and fuzzing for collaborative editing.
3. Local repository: save/open/reopen, candidate reconciliation, shallow clone, signing, audit/recovery, packs, and tombstones.
4. Tauri app GUI: all v0 schema features through the shared app API.
5. Spreadsheet v0: formulas, named ranges, formatting, save/open, merge, import/export.
6. Citations: document-local database, citation labels, `citum` rendering, merge behavior, import/export adapters.
7. Import/export proof: Google Docs/Sheets API-shaped fixtures and `.doc/.docx` import path.
8. Browser and server modes: browser app, HPC single-user wrapper, and multi-user service wrapper over the same semantics.
9. Release hardening: installers, CI, tamper tests, crash tests, performance tests, and warnings/audit polish.

## Open Research Questions

- Whether the final merge architecture should be existing CRDT, custom state-machine model, block-UUID graph, or hybrid.
- Whether deterministic CBOR remains sufficient for canonical binary storage.
- How deep OpenDAL integration should be versus maintaining a small internal object-store trait.
- Which browser key-handling/signing model is acceptable.
- Which HPC tape/archive recall metadata convention is most common for the target environment.
- How Paperpile encodes citation payloads in Google Docs for import compatibility.
- Which TypeScript editor substrate best supports operation-first rich editing.

## Immediate Next Implementation Work

1. Finish operation-backed editing coverage for all v0 document node changes in `opendoc-app-api`, Tauri commands, browser mock, GUI smoke tests, and merge tests.
2. Broaden merge fuzzing to include comments, suggestions, citations, equations, tables, mark ranges, block deletion, and spreadsheet edits in the same streams.
3. Add binary golden fixtures for every source, manifest, operation segment, lookup, tombstone, pack, and signature record.
4. Harden local repository behavior: crash tests, pack compaction tests, candidate reconciliation tests, shallow clone tests, and DOI/UUID lookup tests.
5. Integrate `citum` behind a stable citation rendering boundary.
6. Expand spreadsheet formula and Google Sheets-shaped import/export fixtures.
7. Choose and prototype the real TypeScript editor substrate for Tauri/browser.
8. Add HPC single-user and multi-user service API wrappers without changing document semantics.
