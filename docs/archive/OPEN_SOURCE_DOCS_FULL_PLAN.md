# Open Source Docs Full Plan

Status: canonical planning file for the full Google Docs/Sheets-style product.

Goal: build an open source document suite in Rust with a Tauri/browser frontend, local-first storage, optional collaboration, canonical binary formats, automatic rich-document merging, spreadsheets, citations, shallow-clonable blobs, and optional cryptographic signing.

This plan is complete when every workstream below has an implementation, tests or fixtures, documented degradation behavior, and a clear decision for anything deferred.

## Product Scope

- Rich documents: paragraphs, headings, lists, marks, links, comments, suggestions, citations, equations, tables, images, page breaks, footnotes, and arbitrary attachments.
- Spreadsheets: multi-sheet sparse workbooks, stable sheet/row/column/cell identities, formulas, formatting, named ranges, comments, filters, frozen panes, validations, protected-range warnings, and merged ranges.
- Collaboration: target 1-3 active editors and about 5 viewers, with viewers treated as possible editors.
- Modes: Tauri desktop, browser app, local disk repository, raw S3/OpenDAL repository, HPC single-user web mode, and multi-user service mode.
- Compatibility proof: Google Docs/Sheets API-shaped import/export fixtures, plus `.doc`/`.docx` import where practical.
- Signing: optional, visual/audit-oriented, normal unsigned documents open without restriction.

## Fixed Decisions

- Rust owns source schemas, operations, merge, storage, signing, citations, spreadsheets, import/export, and verification.
- TypeScript owns rich editor behavior, DOM integration, selection, IME, keyboard handling, and Tauri/browser GUI.
- The first app shell is Tauri v2; frontend is hybrid TypeScript.
- Local on-disk objects are first-class before S3 becomes default.
- OpenDAL is preferred if it stays simple; otherwise keep an internal object-store trait compatible with OpenDAL later.
- Durable records are canonical binary, likely deterministic CBOR during research. JSON is allowed only for debug, APIs, tests, and import/export projections.
- Backward compatibility is not required until the project leaves research mode.
- Rendering never waits for commit batching.
- Keypress-level operations must exist; batching is only persistence/network packing.
- Merge is operation/state based, not DOM based.
- All merges must be automatic and produce a valid openable document.
- Bad cases degrade deterministically with warnings, audit records, or recovery state.
- Permissions are not document-format state; they belong to repository opening or service mode.
- Equations store TeX/LaTeX source. Rendered MathML/PDF/output is projection only.
- Citations use structured labels backed by a document-local bibliography database. Start with `citum`; CSL import/export is a later adapter.
- Paperpile's lesson is useful, but OpenDoc should not encode citations as links. Paperpile-like self-contained citation data is better represented as document-local bibliography records plus citation occurrences.
- Signatures cover source state, selected metadata, comments, suggestions, citation source records, equation source, formula source, and referenced history as defined by the manifest.
- Signatures exclude rendered output, computed formula values, citation label render caches, equation render caches, volatile UI state, import provenance, and implementation-only invisible IDs.
- Images and arbitrary binary blobs are content-addressed and independently signable.
- Typed semantic signatures for images and FASTQ are design constraints from the start, even if implementation arrives later.
- Deletion removes data from current visible state while retained history keeps it for audit/recovery. Hard deletion is deferred.

## Done Definition

The product reaches credible v0 only when:

- Every v0 document and spreadsheet feature can be created, edited, saved, closed, reopened, imported, exported, verified, and audited from the Tauri app.
- Every source node round-trips through canonical binary encoding with deterministic bytes for equal source state.
- Every user-visible edit has an operation representation, including keypress-level text edits.
- Local rendering is immediate while persistence can batch independently.
- Synthetic and fuzz tests for 1-3 editors converge automatically after rich edits with formatting, comments, suggestions, citations, equations, tables, images, and spreadsheets.
- Local disk and S3/OpenDAL-shaped storage pass the same repository conformance tests.
- Shallow-cloned documents open with deterministic warnings for missing blobs.
- Version, manifest, blob, and typed semantic signatures verify independently and never block unsigned documents.
- Google Docs/Sheets-shaped fixtures prove the supported subset, and unsupported high-risk structures abort rather than silently changing meaning.
- Normal users never need to understand packs, tombstones, candidate heads, or signature sidecars.

## Workstream 1: Canonical Source Schema

Deliver:

- Stable UUID-backed IDs for documents, blocks, inlines, comments, suggestions, citations, bibliography records, sheets, rows, columns, cells, blobs, and lookup records.
- Optional DOI metadata and UUID/DOI aliases.
- Rich document schema for blocks, inline runs, marks, comments, suggestions, citations, equations, tables, images, attachments, footnotes, warnings, provenance, and audit metadata.
- Spreadsheet schema for sparse cells, sheets, axes, formulas, values, formatting, comments, named ranges, filters, frozen panes, validations, protected-range warnings, and merge ranges.
- Binary records for snapshots, operation segments, manifests, lookup records, tombstones, packs, blob metadata, signatures, and sidecars.

Done when schema fixtures round-trip through binary encoding, invalid states fail or repair with warnings, and source-signature projections exclude caches and invisible implementation-only IDs.

## Workstream 2: Operation Model And Merge

This is the highest-risk part.

Research and test:

- Existing CRDT literature and Rust libraries.
- Custom state-machine/event model.
- Block/paragraph UUID graph.
- Hybrid sequence plus object-identity model.

Required semantics:

- Operations are the only merge unit.
- DOM is a projection, not source truth.
- Keypress insert/delete, range formatting, paragraph split/join, block movement, table edits, equation edits, citation edits, comment edits, suggestion edits, and spreadsheet edits are operations.
- Formatting marks must merge robustly with text insert/delete.
- Comments/suggestions/citations anchor to stable ranges that can survive nearby edits.
- Deleted anchors degrade deterministically: nearest viable anchor, hidden/restorable audit state, or warning depending on scenario.
- Equations merge as source objects; formula results and equation renders are caches only.
- Merge has no manual conflict UI in v0.

Done when realistic synthetic scenarios and operation-level fuzz tests converge byte-for-byte at canonical projection level across replica orderings and validate after every operation.

## Workstream 3: Version Control And Storage

Model:

- Immutable content-addressed objects.
- Manifests link document UUID, branch, parent manifests, snapshots, operation segments, blobs, lookup records, tombstones, warnings, provenance, and signatures.
- Operation segments may batch many keypresses, but this must not affect rendering or merge semantics.
- Heads use compare-and-swap where available.
- Serverless stores use deterministic candidate heads and reconcile by fast-forward or operation-level merge.

Storage modes:

- Local disk object repository first.
- Raw S3/OpenDAL-compatible repository next.
- Browser storage adapter later through the same app API.
- HPC single-user web mode can access disk and S3 after external authentication and assumes one authorized user.
- Multi-user service mode adds auth, permissions, presence, lookup, and sync relay.

Small-file mitigation:

- Start with loose objects.
- Add append-only packs and indexes when object counts become painful.
- Compaction is user-invisible and crash-safe.
- Pack rewrites may happen during local maintenance.

Done when create/open/save/reopen, crash recovery, parallel candidate-head reconciliation, pack compaction, UUID/DOI lookup, missing blob warnings, and local/S3-shaped conformance tests pass.

## Workstream 4: Blobs, Shallow Clone, And Tape

Deliver:

- Content-addressed image and arbitrary binary blobs with algorithm agility.
- Reusable metadata for media type, display name, dimensions, and source hints.
- Exact-byte detached sidecar signatures keyed by blob hash.
- Optional embedded signatures only for formats where that is natural.
- Storage-independent typed semantic signature profiles, including image semantic profiles and FASTQ sequence-only/full-content profiles.
- Shallow clone support where documents can open with placeholders for unavailable blobs.
- Tape/archive tombstones that say where missing data can be recovered.
- Optional central lookup acceleration, without requiring a server for basic operation.

Done when shared blobs are not duplicated, shallow clones open, signed blobs verify independently, semantic signatures can be computed independently of storage layout, and archive tombstones are visible in audit/recovery views.

## Workstream 5: Signing And Audit

Deliver:

- Rust-native OpenSSH-compatible signing by default.
- Optional `ssh-keygen` integration only as a convenience.
- Multiple signatures per manifest/version.
- Algorithm agility for keys, hashes, and signature envelopes.
- Signature states: unsigned, signed, trusted, untrusted, broken.
- Separate source-state signatures, exact-byte blob signatures, and typed semantic signatures.
- Audit/recovery views for signatures, warnings, deleted state, tombstones, provenance, and retained history.
- Browser signing postponed, but browser verification can exist earlier.

Done when valid signatures verify after save/open, tampering is detected, multiple signatures verify independently, unsigned documents open normally, and signature indicators are visible but non-blocking.

## Workstream 6: Citations

Deliver:

- Document-local bibliography database.
- Citation occurrences as structured labels, not links.
- Citation groups with multiple items, locators, prefixes, suffixes, labels, and suppress-author flags.
- `citum` integration for initial citation data/rendering.
- Future CSL import/export adapter without changing the source model.
- Merge tests for bibliography edits and citation movements.

Done when editing one bibliography record updates all dependent labels, citation groups survive save/open, citation moves merge automatically, citation comments/signatures are represented, and rendered labels are excluded from source signatures.

## Workstream 7: Spreadsheets

Deliver:

- Sparse multi-sheet workbook with stable sheet, row, column, and cell identities.
- Formula source preservation and deterministic evaluation.
- Dependency invalidation and RAM-only computed caches.
- Named ranges, comments, filters, frozen panes, validations, protected-range warnings, and merged cells.
- Google Sheets API-shaped import/export.
- Graceful warnings for unsupported formulas or degraded import structures.

Done when formulas evaluate deterministically, cached values are excluded from signatures, dependencies update after edits, import/export fixtures pass, and merge tests cover concurrent sheet, axis, cell, formula, and metadata edits.

## Workstream 8: Rich Editor, Tauri, And Browser

Deliver:

- Tauri desktop app for Linux, macOS, and Windows with minimal user-installed dependencies.
- Browser app sharing the same app API.
- TypeScript rich editor with IME-safe input, selection mapping, paste handling, undo/redo, keyboard shortcuts, decorations, comments, suggestions, citations, equations, tables, and spreadsheet editing.
- GUI controls for every v0 schema feature.
- Save, autosave, open, close, import, export, verify, warning, audit, and recovery views.
- Native build verification, including required icons/assets.

Done when packaged desktop builds pass native checks and the same command/API contract is exercised by browser mock tests.

## Workstream 9: Import And Export

Deliver:

- Google Docs API-shaped document fixtures for the supported subset.
- Google Sheets API-shaped spreadsheet fixtures for the supported subset.
- `.doc`/`.docx` import using tooling that is easy to install on Linux/macOS/Windows, or Linux without root as a fallback.
- Deterministic warnings for unsupported but recoverable structures.
- Abort semantics for unsupported structures that would change meaning.
- Provenance metadata stored outside source signatures.

Done when fixtures prove every v0 feature either imports/exports correctly, degrades with asserted warnings, or aborts with a deterministic error.

## Workstream 10: Server Modes

HPC single-user web mode:

- Runs on an HPC node behind external authentication.
- Can access local disk and S3-like storage.
- Assumes one authorized user with full access to reachable objects.
- Does not enforce document-level permissions.
- Does not need to handle multiple concurrent users.

Multi-user service mode:

- Continuously running service outside the HPC trust boundary.
- Owns authentication, authorization, sharing, permissions, presence, lookup, and sync relay.
- May serialize commits server-side, but document semantics remain compatible with local/serverless mode.

Done when both modes use the same source schemas, binary records, app API, operation semantics, storage contracts, and verification fixtures.

## Prototype Order

1. Finish canonical schema/API coverage for every v0 docs and sheets feature.
2. Finish binary persistence for source records, operations, manifests, signatures, lookups, tombstones, blobs, and packs.
3. Build Google Docs/Sheets-shaped import/export fixtures.
4. Resolve merge model through synthetic scenarios and fuzz tests.
5. Expand the Tauri GUI until every schema feature is editable.
6. Harden local disk storage, candidate heads, packs, tombstones, lookup, shallow clone, and crash recovery.
7. Add S3/OpenDAL backend once local semantics are proven.
8. Add signature verification indicators and audit/recovery views.
9. Add HPC single-user web wrapper.
10. Add multi-user service with authentication, permissions, presence, and sync relay.

## Immediate Implementation Milestones

1. Schema completeness: all v0 nodes represented in `opendoc-core`, binary encoded in `opendoc-format`, and exposed through `opendoc-app-api`.
2. Operation completeness: every editor action represented as an operation and replayed from operation segments.
3. Merge proof: fuzz and synthetic scenarios for rich text, formatting, comments, suggestions, citations, equations, tables, blobs, and spreadsheets.
4. Storage proof: local object repository with manifests, heads, snapshots, operation segments, blobs, lookup, tombstones, packs, and crash tests.
5. Import proof: Google Docs/Sheets-shaped fixtures plus `.doc/.docx` import path.
6. GUI proof: Tauri app can edit every v0 document and spreadsheet feature.
7. Signature proof: OpenSSH-compatible manifest signing, blob sidecars, typed semantic profile APIs, and visible verification state.
8. Service proof: HPC single-user wrapper and multi-user service wrapper share the same core semantics.

## Open Research Questions

- Which merge core wins under realistic rich-document fuzzing: existing CRDT, custom state machine, block UUID graph, or hybrid?
- What is the canonical binary format after research: deterministic CBOR long-term, or a tighter custom binary format?
- Should packs be signed as whole physical packs, logical contained objects, or both?
- Which Rust-native OpenSSH-compatible signing crate is best for long-term maintenance?
- Which `.doc`/`.docx` import pipeline is easiest to install cross-platform while preserving enough structure?
- What exact Google Docs/Sheets unsupported structures should abort import versus degrade with warnings?
- How should browser-side signing handle keys safely when implementation is no longer postponed?
