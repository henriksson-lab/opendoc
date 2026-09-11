# OpenDoc Google Docs Equivalent Plan

Status: canonical all-up tracker.

Tracking file: `docs/OPENDOC_GOOGLE_DOCS_EQUIVALENT_PLAN.md`.

This file is the plan to track. It is done only when every requirement below has implementation evidence, tests or fixtures, and documented failure/degradation behavior where relevant.

## Product Goal

Build an open source Google Docs/Sheets-style suite with a Rust core, a hybrid TypeScript UI, and Tauri desktop support. The same source model must support single-user local work, local offline mode, raw object-store repositories, S3/OpenDAL repositories, collaborative editing, browser use, HPC single-user web serving, and a later multi-user service.

The first serious prototype should optimize for the hard parts: source schemas, object/disk formats, operation model, deterministic automatic merge, local repository behavior, signing semantics, Google-shaped import/export, and a Tauri-oriented app API. UI polish comes after those semantics are proven.

## Done Definition

The plan is complete only when these are true:

| Area | Required evidence |
| --- | --- |
| Rich documents | Binary round-trip fixtures cover paragraphs, headings, lists, tables, page breaks, links, marks, images, attachments, equations, footnotes, mentions, comments, suggestions, citations, warnings, provenance, UUIDs, and optional DOI metadata. |
| Spreadsheets | Multi-sheet sparse workbook tests cover stable sheet/row/column/cell identities, formulas, deterministic recalculation, dependency invalidation, named ranges, formatting, comments, validations, frozen panes, merged ranges, filters, protected-range warnings, and Google Sheets-shaped import/export. |
| Operations | Every user-visible edit is represented as an operation, including keypress-level text edits; batching affects persistence only, not rendering or merge semantics. |
| Merge | Synthetic scenarios and fuzz tests for 1-3 active editors plus passive-viewer-equivalent replicas converge automatically for text, formatting, comments, suggestions, citations, equations, tables, images, and spreadsheets. |
| Storage | Local disk and S3/OpenDAL-shaped stores pass the same object-store suite for manifests, snapshots, operation segments, blobs, packs, shallow clone, UUID/DOI lookup, tombstones, crash recovery, and candidate-head reconciliation. |
| Binary format | Source state, operation segments, snapshots, manifests, lookup records, tombstones, packs, signatures, and sidecars have canonical binary records. JSON is limited to debug/API/import-export projections. |
| Signing | Unsigned documents open normally. OpenSSH-compatible manifest/version signatures, exact-byte blob sidecars, image semantic signatures, FASTQ semantic signatures, multiple signatures, algorithm agility, and tamper detection are tested. |
| Citations | Citations are first-class labels backed by a document-local bibliography, render through `citum`, merge safely, update all occurrences from one reference record, and can later import/export CSL without changing source semantics. |
| Import/export | Google Docs/Sheets-shaped fixtures prove the v0 subset. `.doc/.docx` import works where practical. Unsupported high-risk structures abort or warn by documented rules. |
| Apps | Tauri desktop, browser local mode, HPC single-user web mode, and multi-user service mode share the same app API, source schema, storage records, and operation semantics. |
| UX | Normal users do not need to understand packs, tombstones, candidate heads, shallow clone internals, or signature sidecars; degraded states appear as warnings and audit/recovery records. |

## Non-Negotiable Constraints

- Canonical storage is binary, not JSON. Deterministic CBOR is acceptable during research.
- Backward compatibility is not required until the project leaves research.
- Rendering must never wait for batching, storage commits, signing, or compaction.
- Keypress-level operations must be possible; operation segments are storage batches, not semantic commits.
- Merge must be fully automatic. Bad cases create deterministic warnings or recovery metadata, not manual conflicts.
- The DOM is a projection, not the merge/source model.
- Formatting marks are first-class ranges anchored to stable identities, not fragile rendered spans.
- Comments, suggestions, citations, equations, tables, and spreadsheet structures participate in merge tests from the start.
- Invisible block/inline IDs may help merging, but normal content signatures must exclude implementation-only IDs.
- Rendered PDFs, rendered equations, rendered citation labels, cached formula values, volatile UI state, and import provenance are excluded from source signatures.
- Deletion removes objects from current state while retained history/audit data remains. Hard deletion is out of scope for now.
- Permissions are not part of the document format; they belong to repository-opening capabilities or the multi-user service layer.

## Current Design Decisions

- Core language: Rust.
- UI direction: hybrid TypeScript frontend with Rust document logic behind a stable app API.
- Desktop shell: Tauri v2 for Linux, macOS, and Windows.
- Frontend framework: TypeScript first; Leptos remains optional for experiments.
- Storage default: local on-disk object repository first.
- Storage abstraction: prefer OpenDAL if simple; otherwise maintain a small internal object-store trait compatible with OpenDAL/S3 later.
- Browser signing: postponed until key handling is clearer.
- Tauri signing: Rust-native OpenSSH-compatible keys by default; optional `ssh-keygen` helper can be added if useful.
- Spreadsheet v0: include formula evaluation, but keep formulas as source and computed values as regenerated projections/RAM cache.
- Equations: store TeX/LaTeX source; render MathML/browser output as projection.
- Citations: use structured citation labels plus a document-local bibliography; start with `citum`; keep CSL import/export as an adapter.
- Import priority: Google Docs-shaped documents and `.doc/.docx` before export polish.
- Compatibility proof: Google Docs/Sheets API-shaped import/export fixtures.
- Archive/tape support: tombstones plus optional central lookup acceleration; serverless mode must still work by repository/bucket scan.

## Runtime Modes

Tauri local mode:

- Uses local filesystem repositories first.
- Later supports S3/OpenDAL repositories.
- Can load local OpenSSH-compatible signing keys.
- Must use the same operations, manifests, and source schemas as every other mode.

Browser local mode:

- Uses the same app contract.
- Stores locally through browser-appropriate backends or talks to a service.
- Opens unsigned and signed documents normally.
- Browser-side signing is deferred.

HPC single-user web mode:

- Runs on an HPC node behind external authentication such as Open OnDemand.
- Can access disk and S3-like storage.
- Assumes one authenticated user with full access to reachable objects.
- Does not enforce document-level permissions.

Multi-user service mode:

- Runs continuously outside the HPC single-user trust boundary.
- Owns authentication, permission checks, sharing, presence, sync relay, and optional commit serialization.
- Must not change document source semantics.

## Architecture

Core crates:

- `opendoc-core`: document, spreadsheet, IDs, comments, suggestions, citations, equations, and attachments.
- `opendoc-merge`: operations, CRDT/state-machine experiments, automatic merge, deterministic warnings.
- `opendoc-format`: canonical binary records for source state, operations, manifests, signatures, lookup records, tombstones, and packs.
- `opendoc-store`: local disk and S3/OpenDAL-shaped object stores, heads, packs, lookup, shallow clone, archive tombstones.
- `opendoc-sign`: OpenSSH-compatible signatures, exact-byte blob sidecars, typed semantic signatures.
- `opendoc-import`: Google Docs/Sheets-shaped adapters and `.doc/.docx` import adapters.
- `opendoc-app-api`: UI-facing API used by Tauri, browser tests, CLI tests, HPC mode, and later service wrappers.

Applications:

- `apps/desktop`: Tauri app.
- Later `apps/web`: browser app.
- Later HPC single-user server wrapper.
- Later multi-user collaboration service.

## Version Control And Merge Plan

Commit granularity:

- Semantic unit: individual operations.
- UI unit: operations are applied immediately so local rendering stays responsive.
- Persistence unit: operation segments may batch many operations.
- Repository unit: manifests point to snapshots, operation segments, blobs, signatures, lookup records, and tombstones.

Merge model:

- Evaluate an Automerge-style CRDT/state-machine model first, while keeping room to compare Yjs-style or custom sequence/block identity approaches.
- Use stable document, block, inline, range, row, column, cell, citation, comment, suggestion, blob, and lookup IDs where they improve merge or recovery.
- Rich-text marks are source ranges over stable identities.
- Comments and suggestions anchor to stable ranges and may span blocks.
- Citation occurrences are typed labels referencing document-local citation groups.
- Equations merge as atomic source objects.
- Tables use stable row/cell identities.
- Spreadsheet rows/columns/cells use stable identities; UI coordinates are projections.

Graceful degradation:

- If referenced text is deleted, anchors move to the nearest valid surviving place or become audit-only where that benefits the user.
- If a citation reference is deleted, visible occurrences degrade to deterministic missing-reference labels.
- If blobs are missing in a shallow clone, documents open with placeholders and restore hints.
- If imported structures are high-risk and unsupported, import aborts rather than silently changing meaning.
- All degraded cases emit deterministic warnings.

Validation:

- Realistic synthetic scenarios.
- Operation-level fuzzing.
- Deterministic replay.
- Reordered actor streams.
- Byte-for-byte canonical convergence checks.
- Validity checks after every operation and merge.

## Storage Plan

Object layout:

- Content-addressed immutable objects.
- Manifests for versions.
- Snapshots for fast open.
- Operation segments for replay.
- Branch heads with CAS where possible.
- Candidate branch heads when CAS is unavailable or races occur.
- Lookup records for UUIDs and optional DOI aliases.
- Tombstone records for tape/archive recall.
- Sidecar signature records for blobs and semantic profiles.
- Pack files to mitigate local small-file overhead.

Local disk:

- First-class v0 backend.
- Crash-safe writes use write-verify-atomic-swap semantics where possible.
- Pack compaction is maintenance work and invisible to users.

S3/OpenDAL:

- Same logical object semantics as local disk.
- Raw S3/serverless mode cannot rely on a commit server.
- Candidate-head reconciliation must handle parallel writers and later merge divergent candidates automatically.
- One store is preferred to avoid disagreement between stores.

Shallow clone:

- Documents may open with missing large blobs.
- Blobs are referenced by hash and can be shared across documents or clones.
- Tombstones can describe where archived/missing blobs may be restored from tape or other storage.

## Signing Plan

Unsigned documents:

- Open normally.
- Show no special blocking behavior.
- Signature state is a visual/audit signal.

Source signatures:

- Sign authored source state plus reachable retained history selected by the manifest.
- Exclude rendered output, computed formula values, volatile UI state, implementation-only merge IDs, and import provenance.
- Sign comments and suggestions because they are source/audit state.
- Sign formula source, not computed values.
- Include visible signature metadata such as title, signer display name, signer key identity, and signing timestamp.

Blob signatures:

- Images and arbitrary binary blobs are content-addressed and independently signable.
- Exact-byte signatures are best stored as sidecars keyed by blob hash, unless embedding is natural for a specific format.
- Sidecars keep signing independent of S3/local/tape layout.

Typed semantic signatures:

- Design for semantic profile signatures from the start.
- Image semantic signatures should sign an image profile independent of compression/container details.
- FASTQ semantic signatures should allow signing data profiles that can survive dropping PHRED scores where that is the intended scientific meaning.
- Algorithm agility is required immediately.

## Citation Plan

- Citations are special inline labels, not links.
- A label references a document-local citation group.
- Citation groups reference document-local bibliography records.
- One reference record may be used by many occurrences.
- Updating a reference updates all rendered occurrences through projection/cache regeneration.
- `citum` is the first renderer/integration target.
- CSL import/export is a future adapter, not the canonical source store.
- Paperpile’s Google Docs link-based embedding is useful research input but should not define OpenDoc internals.
- Citation labels, groups, bibliography records, style, and locale are source state.
- Rendered labels are cache/projection and are not signed as content.

## Spreadsheet Plan

V0 includes:

- Sparse multi-sheet workbooks.
- Stable sheet, row, column, and cell identities.
- UI coordinates as projections.
- Sheet rename/delete by stable sheet ID.
- Formula source preservation.
- Deterministic formula evaluation for the selected subset.
- Dependency graph and invalidation.
- Named ranges.
- Cell formatting.
- Cell comments.
- Frozen panes.
- List validations.
- Merged ranges.
- Basic filters.
- Protected ranges as warning-only metadata.
- Google Sheets-shaped import/export fixtures.

Formula rules:

- Store source formulas.
- Regenerate computed values.
- Avoid volatile functions until deterministic semantics are defined.
- Date/time values are numbers with typed formatting.
- Formula source is signed; computed values are not.

## Tauri App Plan

The Tauri app must eventually support the full v0 docs-like schema through a GUI:

- Create/open/close/save/reopen documents.
- Local repository selection.
- Rich text editing.
- Paragraphs, headings, lists, tables, page breaks.
- Inline marks, links, mentions, citations, footnotes, equations.
- Image and attachment insertion with missing-blob placeholders.
- Comments and suggestions with audit/recovery views.
- Spreadsheet editing for all v0 structures.
- Import/export commands.
- Signature verify/sign UI for desktop mode.
- Warnings and degraded-state display.
- Recent documents.
- Undo/redo.

The Tauri shell must remain thin. Document logic stays in Rust crates and `opendoc-app-api`.

## Browser And Service Plan

Browser app:

- Shares the TypeScript UI and app API contract.
- Uses WASM/service/Tauri command adapters depending on runtime.
- Does not own canonical merge or storage semantics.

HPC single-user web:

- Provides browser access to a user-owned local/S3 repository.
- Assumes external authentication and full reachable-object access.
- Avoids multi-user permission complexity.

Multi-user service:

- Adds authentication, permissions, sharing, presence, and relay.
- May serialize commits for convenience, but source semantics must remain valid without it.
- Permissions are service metadata, not document source.

## Import And Export Plan

Google Docs:

- Use Google Docs API-shaped fixtures to prove the document subset.
- Preserve OpenDoc-only features through explicit extension records where public Google API objects lack native support.
- Abort import for unsupported high-risk structures that would change meaning.

Google Sheets:

- Use Google Sheets API-shaped fixtures to prove spreadsheet v0.
- Map supported Sheets concepts directly where possible.
- Preserve OpenDoc extensions for full round-trip behavior.

`.doc/.docx`:

- Prioritize document import where tooling is easy to install on Linux/macOS/Windows or can run without root on Linux.
- Import adapters produce OpenDoc operations or source state.
- Preserve import provenance outside signed content.

## Archive And Lookup Plan

- Documents have UUIDs.
- DOI metadata is optional.
- Cross-document references use UUIDs for flexibility.
- DOI aliases point to UUID lookup records when present.
- Serverless repositories can scan lookup records when no central lookup exists.
- A central lookup service may accelerate discovery but is not required.
- Tape/archive tombstones record enough locator/restore metadata to recover missing objects where possible.
- Missing archived data remains hidden by default, visible in audit/recovery, and restorable if the data can be found.

## Phases

1. Schema and binary records: finish canonical source records and round-trip fixtures for docs, sheets, citations, manifests, signatures, blobs, lookup records, and tombstones.
2. Operation and merge proof: complete rich-document and spreadsheet operation coverage, synthetic scenarios, fuzz tests, convergence checks, and deterministic warnings.
3. Local object repository: finish local create/open/save/reopen, crash recovery, candidate-head reconciliation, shallow clone, pack compaction, UUID/DOI lookup, tombstone restore metadata, and missing-blob behavior.
4. Signing: finish manifest/version signing, exact-byte blob signatures, semantic signature profile design, trust states, and tamper tests.
5. Spreadsheet v0: finish deterministic formula subset, import/export fixtures, dependency invalidation, and all source metadata commands.
6. Citations: finish `citum` integration, bibliography operations, citation group merge behavior, style switching, and future CSL adapter boundary.
7. Tauri app: build GUI support for every v0 schema feature through the app API, with native build checks and smoke tests.
8. Browser/HPC wrappers: add browser adapter and HPC single-user server mode without changing source/storage semantics.
9. Multi-user service: add authentication, permissions, presence, sync relay, and service-managed collaboration paths.
10. Product hardening: improve performance, packaging, import/export coverage, diagnostics, recovery tools, and documentation.

## Immediate Work Queue

1. Finish the docs-like editor surface for every v0 document node.
2. Finish the merge model validation for formatting, comments, suggestions, citations, equations, tables, images, and spreadsheets.
3. Finish local disk object-store semantics and keep the API compatible with OpenDAL/S3.
4. Expand formula coverage and Google Sheets-shaped fixtures until spreadsheet v0 is demonstrably useful.
5. Integrate `citum` behind a document-local citation database.
6. Complete signing verification and source-vs-cache signature boundaries.
7. Keep the Tauri app buildable on Linux/macOS/Windows and add GUI smoke tests for all major schema features.

## Current Product Gaps

These gaps must be closed before OpenDoc can reasonably be called a Google Docs-equivalent product:

1. Real operation-first rich editor: the Tauri/browser GUI must create and edit every v0 document and spreadsheet feature without making DOM state authoritative.
2. Proven automatic merge: realistic scenarios and fuzz tests must show deterministic convergence for concurrent text, formatting, comments, suggestions, citations, equations, tables, images, blobs, and spreadsheet edits.
3. Repository completeness: local disk must pass the full object-store suite, then the same suite must run against flat/S3/OpenDAL-shaped storage.
4. Candidate-head recovery: parallel writers in serverless storage must always reconcile through fast-forward or operation-level merge, with invalid candidate records visible in audit/recovery.
5. Binary format coverage: all source, operation, manifest, lookup, tombstone, pack, signature, and sidecar records need deterministic binary round-trip tests.
6. Signing and audit: manifest/version signatures, blob sidecar signatures, typed semantic signatures, trust states, and tamper detection need end-to-end save/open verification.
7. Citation completeness: `citum` rendering, document-local bibliography updates, citation merge behavior, cache invalidation, and future CSL import/export boundaries need tests.
8. Spreadsheet completeness: formula evaluation, dependency invalidation, named ranges, filters, validations, protected-range warnings, merged cells, comments, and Google Sheets-shaped fixtures must be finished.
9. Import/export proof: Google Docs/Sheets-shaped fixtures and practical `.doc/.docx` imports must either preserve supported content, warn deterministically, or abort high-risk unsupported content.
10. Runtime wrappers: browser local mode, HPC single-user web mode, and multi-user service mode must use the same app API and storage/source semantics as Tauri.
11. Packaging and UX: Linux/macOS/Windows Tauri builds must be reproducible, include required assets, and show degraded states as normal warnings instead of exposing storage internals.

## Research Still Open

These items are intentionally unresolved until implementation evidence or tests settle them:

- Best merge architecture: existing CRDT library, custom state-machine model, stable block/paragraph identity graph, or a hybrid.
- Best TypeScript editor substrate for IME-safe operation-first editing.
- Whether deterministic CBOR remains sufficient or a more specialized binary format is needed.
- OpenDAL integration depth versus a smaller internal object-store trait with OpenDAL adapters.
- Browser-side key handling and signing.
- Common HPC/tape recall metadata needed for useful tombstones.
- Paperpile-compatible import behavior, if we later need to ingest link-embedded citation metadata from Google Docs.
