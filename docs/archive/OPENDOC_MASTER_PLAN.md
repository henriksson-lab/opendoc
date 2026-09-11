# OpenDoc Master Plan

Status: canonical tracking plan.

Purpose: capture the full product design discussed so far for an open source Google Docs/Sheets alternative in Rust. This file is done when every requirement below is implemented with tests, explicitly deferred with a reason, or moved into a smaller tracked plan with exit criteria.

Tracking rule: a feature is complete only when it has schema support, operation/API support where relevant, canonical binary persistence, save/open coverage, merge coverage where relevant, and documented graceful degradation behavior.

## Product Target

OpenDoc is a local-first collaborative document suite with:

- rich documents: paragraphs, headings, lists, marks, links, comments, suggestions, citations, equations, tables, images, page breaks, footnotes, and arbitrary binary attachments
- spreadsheets: multi-sheet workbooks, stable sheet/row/column/cell identities, formatting, named ranges, merged cells, filters, protected ranges, frozen panes, comments, and deterministic formula evaluation
- collaboration for 1-3 active editors and about 5 viewers, with viewers treated as potential editors
- single-user offline local disk mode first
- raw S3/OpenDAL-compatible storage later, including serverless single-user mode without a commit server
- Tauri desktop app for Linux, macOS, and Windows
- browser app support through the same app API
- HPC single-user web mode behind external authentication, with disk and S3-like access
- continuously running multi-user service mode with authentication, permissions, sharing, presence, lookup, and sync relay
- Google Docs/Sheets API-shaped import/export as the compatibility proof
- `.doc`/`.docx` import where practical
- optional signing for versions, blobs, and typed semantic profiles

## Fixed Decisions

- Rust owns source schemas, operations, merge, storage, signing, citations, spreadsheets, import/export, and verification.
- TypeScript owns the rich editor surface, DOM integration, selection, IME behavior, keyboard handling, and browser/Tauri GUI.
- Tauri v2 is the first serious app shell.
- The frontend approach is hybrid TypeScript; Leptos remains optional, not a required decision.
- Local on-disk object storage is first-class before S3 becomes default.
- Prefer OpenDAL if it stays simple; otherwise keep a small internal object-store trait compatible with OpenDAL later.
- Durable storage is canonical binary, likely deterministic CBOR during research. JSON is only for debug/API/import/export projections.
- Backward compatibility is not required until the project leaves research mode.
- Rendering must never wait for persistence batching.
- Keypress-level operations must be representable.
- Operation segments are storage batches, not merge semantics.
- Merge is operation/state based, not DOM based.
- Merge must always produce a valid openable document; v0 has no manual conflict UI.
- Bad cases degrade deterministically with warnings.
- Invisible IDs may anchor merge behavior but are excluded from normal source-content signatures.
- Permissions are not document-format state; they belong to repository opening or service mode.
- Unsigned documents open normally.
- Signatures are visual/audit indicators for scientific fraud review, patent precedence, and 21 CFR-style workflows.
- Rust-native OpenSSH-compatible signing is the default; `ssh-keygen` may be optional.
- Browser signing is postponed.
- Citations are structured labels backed by a document-local bibliography database.
- Use `citum` first; CSL import/export can be added later as an adapter.
- OpenDoc should not copy Paperpile's link-embedded citation model.
- Equations store TeX/LaTeX source; rendered MathML/PDF output is projection only.
- Spreadsheet signatures cover formula source, not computed formula values.
- Rendered output, citation label caches, formula caches, volatile UI state, import provenance, and implementation-only invisible IDs are excluded from source signatures.
- Deletion removes data from current visible state while retained history keeps it for audit/recovery. Hard deletion is deferred.

## Workstreams

### 1. Source Schema And Binary Format

Deliver:

- canonical document, spreadsheet, citation, equation, comment, suggestion, warning, provenance, blob, and audit schemas
- stable UUID-backed IDs for documents, blocks, inlines, comments, suggestions, citations, sheets, rows, columns, and cells
- optional DOI aliases and UUID lookup records
- binary records for snapshots, operation segments, manifests, lookup records, tombstones, blob metadata, packs, and signatures
- debug JSON projections only for tests, inspection, and import/export adapters

Done when every source node round-trips through canonical binary encoding, semantically identical source states encode identically, corrupt records fail cleanly, and invalid states are rejected or repaired with deterministic warnings.

### 2. Operation Model And Automatic Merge

This is the highest-risk workstream.

Deliver:

- final tested choice between an existing CRDT, custom state-machine model, block UUID graph, or hybrid
- operation-level merge engine independent of DOM structure
- keypress-level text insert/delete operations
- first-class formatting range operations
- stable anchors for comments, suggestions, citations, equations, tables, rows, columns, and cells
- deterministic repair semantics for deleted anchors, split/join paragraphs, delete-versus-format, delete-versus-comment, and concurrent table edits

Done when realistic synthetic scenarios and deterministic fuzz tests for 1-3 active editors converge byte-for-byte at canonical projection level, validate after every operation, and never require manual merge resolution.

### 3. Version Control And Storage

Deliver:

- content-addressed immutable objects
- snapshots plus operation segments
- manifests with parent links, branch, document UUID, snapshots, operation segments, blobs, lookup records, tombstones, provenance, and signatures
- branch/head records with compare-and-swap where available
- deterministic candidate heads where compare-and-swap is unavailable
- candidate-head reconciliation by fast-forward or operation-level merge
- local disk repository first
- S3/OpenDAL-compatible repository later
- repository scan fallback for serverless UUID/DOI lookup
- optional central lookup acceleration in service mode
- pack files for local small-file mitigation
- user-invisible compaction
- tape/archive tombstones describing how missing data can be recovered

Done when tests prove local create/open/save/reopen, parallel candidate save reconciliation, missing blob handling, UUID/DOI lookup, pack compaction, and tombstone recovery metadata.

### 4. Blobs, Shallow Clone, And Tape

Deliver:

- content-addressed images and arbitrary binary blobs with hash algorithm agility
- reusable blob metadata with media type, display name, and optional dimensions
- exact-byte detached sidecar signatures keyed by blob hash
- optional embedded signatures only where a data type naturally supports them
- shared blob reuse across documents
- shallow clone mode with placeholders and warnings for missing blobs
- tape/archive tombstones surfaced in audit/recovery views

Done when documents open with missing blobs, shared blobs are not duplicated, signed blobs verify independently, and archive/tape metadata can guide recovery.

### 5. Signing And Audit

Deliver:

- version/manifest signatures over source state and retained reachable history
- multiple signatures per version
- signature states: unsigned, signed, trusted, untrusted, broken
- exact-byte blob signatures
- storage-independent typed semantic signature APIs
- image semantic signature profile design
- FASTQ sequence-only and full-content signature profile design
- audit/recovery views for signatures, warnings, deleted state, tombstones, and provenance

Done when valid signatures verify after save/open, tampering is detected, multiple signatures verify independently, unsigned documents remain openable, and typed signature APIs can sign storage-independent byte or semantic profiles.

### 6. Citations

Deliver:

- document-local bibliography database
- citation occurrence nodes as structured labels, not links
- citation groups with locators, labels, prefixes, suffixes, and suppress-author flags
- `citum` integration for initial rendering/data model
- later CSL import/export adapter if useful
- merge tests for moving citations and editing bibliography records

Done when updating one reference updates every dependent label, citation groups survive save/open, citation movement merges automatically, and rendered labels are excluded from source signatures.

### 7. Spreadsheets

Deliver:

- sparse multi-sheet workbook model
- stable sheet, row, column, and cell identities
- typed values, formulas, formatting, comments, named ranges, filters, frozen panes, validations, protected ranges, and merged cells
- deterministic formula parser/evaluator and dependency invalidation
- Google Sheets API-shaped import/export
- warning semantics for unsupported formulas or degraded imports

Done when formulas evaluate deterministically, cached values are excluded from signatures, dependencies update after edits, import/export fixtures pass, and merge tests cover concurrent sheet/cell edits.

### 8. Rich Editor And App API

Deliver:

- TypeScript editor that projects OpenDoc state and emits OpenDoc operations
- IME-safe input, selection mapping, paste handling, undo/redo, keyboard shortcuts, and decorations
- operation-backed GUI controls for every v0 document node and spreadsheet feature
- immediate local rendering before save batching
- save, autosave, open, close, import, export, verify, warning, audit, and recovery views
- browser mock/API tests sharing the same app contract as Tauri
- native build verification including required assets such as icons

Done when the packaged Tauri app can create, edit, save, close, reopen, import, export, verify, and audit documents containing every v0 schema feature.

### 9. Import And Export

Deliver:

- Google Docs API-shaped import/export fixtures for the supported document subset
- Google Sheets API-shaped import/export fixtures for the supported spreadsheet subset
- `.doc`/`.docx` import where tooling allows without root-only dependencies
- deterministic warnings for unsupported Google structures
- abort semantics for imports that cannot be represented safely
- provenance metadata that is not part of signed source state

Done when compatibility fixtures prove every v0 schema feature either imports/exports correctly, degrades with an asserted warning, or aborts with a clear deterministic error.

### 10. Server Modes

Deliver:

- HPC single-user web wrapper that runs behind external authentication, accesses disk/S3 from the server process, assumes one authorized user, and skips document-level permissions
- multi-user service with authentication, authorization, sharing, presence, object lookup, and sync relay
- optional server-side commit serialization in service mode
- same source schemas, binary records, and operation semantics across local, browser, HPC, S3, and service modes

Done when service tests prove collaboration within target scale and unauthorized users cannot access heads, objects, blobs, comments, suggestions, or lookup records.

## Immediate Prototype Order

1. Finish the canonical schema/API surface for every v0 docs and sheets feature.
2. Finish canonical binary persistence for every source and operation record.
3. Build richer import/export fixtures for Google Docs/Sheets-shaped data.
4. Stress automatic merge with synthetic scenarios and fuzzing.
5. Expand the Tauri GUI until every schema feature is editable.
6. Harden local disk storage, candidate heads, packs, tombstones, and shallow clones.
7. Add OpenDAL/S3 backend once local disk semantics are proven.
8. Add signing verification indicators and audit/recovery views.
9. Add HPC single-user wrapper.
10. Add multi-user service with auth, permissions, presence, and sync relay.

## Phase Plan

### Phase 0: Research Lock-In

Done when the repository records decisions for:

- Google Docs and Sheets public subset mappings
- operation/state merge architecture candidates and rejection criteria
- binary canonical format and debug projection boundaries
- local disk object layout, S3/OpenDAL mapping, candidate heads, packs, shallow clones, lookup records, and tape tombstones
- signing boundaries for versions, blobs, and storage-independent semantic profiles
- citation model using document-local references and `citum`
- Tauri/browser/HPC/service runtime boundaries

### Phase 1: Format And API Prototype

Done when tests prove:

- every v0 source node can be constructed through Rust APIs
- every v0 operation validates before and after application
- canonical binary records round-trip for snapshots, operation segments, manifests, blobs, signatures, citations, spreadsheets, lookup records, packs, and tombstones
- JSON remains only a debug/import/export/API-contract projection
- invalid or unsupported states fail with deterministic warnings or explicit aborts

### Phase 2: Local Editing Product

Done when the Tauri app can:

- create, edit, save, close, and reopen rich documents and spreadsheets on local disk
- expose GUI controls for every v0 schema feature
- render edits immediately while persistence batches independently
- show warnings, signature state, audit/recovery data, deleted retained state, missing blobs, and tombstones
- pass Linux, macOS, and Windows native preflight/build checks

### Phase 3: Collaboration Proof

Done when simulations and fuzz tests prove:

- 1-3 active editors and viewer-equivalent replicas converge automatically
- concurrent text, formatting, comments, suggestions, citations, equations, tables, spreadsheet cells, rows, columns, and named ranges remain valid
- operation batches can be reordered, compacted, or packed without changing canonical projection
- all degraded merges remain openable and warn deterministically
- no v0 merge path requires manual conflict resolution

### Phase 4: Storage, Signing, And Import/Export Proof

Done when tests prove:

- local disk and S3/OpenDAL-style stores share the same repository semantics
- candidate heads reconcile without a server by fast-forward or operation-level merge
- shallow clones reuse content-addressed blobs and open with placeholders for missing data
- version signatures, sidecar blob signatures, and semantic profiles verify independently
- Google Docs/Sheets-shaped fixtures import/export every supported v0 feature or fail/degrade explicitly

### Phase 5: Runtime Modes

Done when:

- local Tauri is the default single-user offline mode
- browser mode uses the same app contract and source schema
- HPC single-user web mode can access disk and S3-like storage behind external authentication without document permissions
- multi-user service mode owns authentication, authorization, sharing, presence, lookup acceleration, sync relay, and optional server-side commit serialization

## Current Implementation Audit

The project is not done until every item in the project-level done definition passes. Current prototype work should be tracked as implementation progress only, not product completion.

Known prototype coverage already started:

- Rust source schemas for documents, spreadsheets, citations, storage manifests, and signed manifests
- app API and Tauri/browser command contract
- local object-store and signing foundations
- citation schema and document-local citation approach
- deterministic spreadsheet formula evaluator with an expanding compatibility corpus
- desktop GUI and native verification scaffolding

Known gaps that still block the Google Docs-equivalent product:

- final merge architecture is not proven by realistic fuzz tests
- canonical binary persistence is incomplete across all source and operation records
- GUI does not yet edit every v0 schema feature
- Google Docs/Sheets import/export fixtures are incomplete
- local disk storage semantics are not yet proven against serverless concurrent writer simulations
- OpenDAL/S3 backend, HPC wrapper, and multi-user service are not finished
- audit/recovery views, signing UX, pack compaction, shallow clone recovery, and tape tombstone workflows need end-to-end tests

## Project-Level Done Definition

OpenDoc is a credible Google Docs-equivalent v0 when:

- every supported document and spreadsheet feature is editable in the Tauri GUI
- local rendering is immediate while persistence can batch independently
- 1-3 concurrent editors converge automatically in merge tests
- local disk save/open and S3/OpenDAL-style save/open use the same semantics
- shallow-cloned documents open with warnings for missing blobs
- version/blob signatures verify independently and never block unsigned documents
- citations, comments, suggestions, equations, tables, images, and spreadsheet formulas all survive save/open and merge scenarios
- Google Docs/Sheets-shaped import/export fixtures prove the supported subset
- packaged Linux/macOS/Windows desktop builds pass native verification
- browser, HPC single-user, and multi-user service modes share the same app API and storage semantics
