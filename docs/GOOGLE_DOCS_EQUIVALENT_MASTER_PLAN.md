# OpenDoc Google Docs Equivalent Master Plan

Status: product contract and completion checklist.

Goal: build an open source Google Docs/Sheets-style system written primarily in Rust, with deterministic local-first storage, fully automatic collaboration merges, optional cryptographic signing, and desktop/browser frontends.

This plan is complete only when every workstream below has implementation, tests or fixtures, and documented graceful degradation where applicable. A research conclusion counts as done only after it is encoded as schema, operation semantics, binary format, tests, or an explicit deferral.

## Completion Contract

The product is not considered Google Docs-equivalent until these gates pass:

- create, edit, save, close, reopen, import, export, verify, and audit one document containing every v0 document node
- create, edit, save, close, reopen, import, export, verify, and audit one spreadsheet containing every v0 spreadsheet feature
- replay and merge realistic 1-3 editor scenarios in multiple operation orders with byte-for-byte canonical convergence
- run keypress-level operations immediately in the UI while independently batching persistence records
- store all durable state in canonical binary objects, with JSON limited to debug/API/import/export projections
- open unsigned documents normally while showing signature state when signatures exist
- verify version signatures, detached blob signatures, and storage-independent semantic signatures
- shallow-clone a document with missing blobs and show deterministic placeholders plus warnings
- recover archive/tape tombstone metadata enough to tell a user where missing data can be recalled from
- run the same app API through local tests, the Tauri app, HPC single-user web mode, and multi-user service mode
- pass Linux, macOS, and Windows packaging/build checks without requiring unusual user-installed tools

Each gate needs an automated test, fixture, smoke test, or documented manual packaging check before it can be marked done.

## Traceability

This file is the canonical plan for the design discussion. It covers:

- Rust-first source schemas, operations, merge, storage, signing, citation, spreadsheet, import/export, and verification logic.
- Hybrid TypeScript frontend work for a Tauri desktop app and browser app using the same app API.
- Local on-disk object storage first, with S3/OpenDAL-compatible semantics later.
- Single-user raw object storage, offline use, HPC single-user web serving, and continuously running multi-user service serving.
- Keypress-level operation granularity with persistence batching kept separate from rendering.
- Fully automatic merge semantics for formatted documents, comments, suggestions, citations, equations, tables, images, and spreadsheets.
- Canonical binary records instead of JSON for durable state; JSON remains only for debug/API/import-export projections.
- Content-addressed blobs, shallow clone, sidecar or embedded signatures, storage-independent semantic signatures, tombstones, UUID/DOI lookup, and tape/archive recovery metadata.
- Spreadsheet v0 formula evaluation and Google Docs/Sheets API-shaped import/export as the compatibility proof.
- Document-local citation records with structured citation labels and `citum` as the first rendering target.

## Product Scope

OpenDoc must support:

- rich documents with paragraphs, headings, lists, tables, formatting marks, links, comments, suggestions, citations, equations, images, and arbitrary binary attachments
- spreadsheets with multi-sheet workbooks, formatting, formulas, named ranges, stable row/column/cell identities, and deterministic formula evaluation
- single-user offline mode on local disk
- raw object-store mode using S3/OpenDAL-style semantics without a commit server
- collaborative editing for 1-3 active editors and about 5 viewers, treating every viewer as a potential editor
- fully automatic merges that always produce an openable document
- Tauri desktop app for Linux, macOS, and Windows
- browser app using the same app API and source schema
- HPC single-user web mode behind external authentication with local disk and S3 access
- multi-user service mode with authentication, permissions, sharing, presence, and sync relay
- Google Docs/Sheets API-shaped import/export as the main compatibility proof
- `.doc`/`.docx` import where tooling is practical

## Fixed Design Decisions

- Rust owns source schemas, operations, merge, storage, signing, spreadsheet logic, citations, import/export, and verification.
- TypeScript owns the rich editor surface, DOM integration, selection, IME, keyboard behavior, and browser/Tauri GUI.
- Rendering must not wait for commit batching.
- Keypress-level operations must be representable.
- Storage batches and commit packs are persistence optimizations, not editing semantics.
- The DOM is a projection, not the merge source of truth.
- The durable format is canonical binary, currently deterministic CBOR unless replaced by a better binary format during research.
- JSON is allowed only for debug views, tests, command contracts, and external API/import/export adapters.
- Backward compatibility is not required until the project leaves research mode.
- Permissions are not part of the document format; they belong to repository opening or the multi-user service.
- Users should not need to understand packs, candidate heads, compaction, tombstones, or shallow clone internals.

## Source Schema

Define one canonical source model for all runtimes:

- document UUID, optional DOI, title, metadata, and provenance
- stable block IDs and text element IDs for merge anchoring
- paragraphs, headings, lists, page breaks, tables, images, equations, and attachments
- formatting as first-class marks and ranges, not fragile DOM spans
- comments and suggestions as signed source state
- citations as structured citation labels backed by a document-local bibliography database
- equations as TeX/LaTeX source; rendered MathML/PDF is projection only
- spreadsheet sheets, rows, columns, cells, formulas, dependencies, named ranges, and formatting
- warnings and audit/recovery records for degraded state

Done when every v0 source node round-trips through canonical binary encoding and invalid source states are rejected or repaired with deterministic warnings.

## Operation And Merge Model

This is the highest-risk workstream.

Research and prototype:

- compare existing CRDT libraries, custom state-machine/event models, block UUID graphs, and hybrid designs
- run realistic synthetic scenarios and deterministic fuzz tests before declaring the merge model settled
- use operations as the only merge unit
- allow invisible IDs as merge anchors, but exclude them from normal source-content signatures

Required merge semantics:

- concurrent typing in the same paragraph converges
- formatting ranges survive insertions, deletions, paragraph splits, and paragraph joins where possible
- delete-versus-format and delete-versus-comment degrade deterministically
- comments and suggestions anchor to stable UUID-backed ranges, including cross-block ranges
- deleted comments disappear from normal state but remain available in audit/recovery history when retained
- suggestions support insert, delete, format, accept, reject, and provenance
- citations move as structured occurrences, not links
- equations merge as atomic source objects
- tables use stable row/column/cell identities
- passive-viewer-equivalent replicas do not introduce special semantics

Done when 1-3 active-editor fuzz tests and realistic merge scenarios converge byte-for-byte at canonical projection level, validate schema after every operation, and never require manual conflict resolution.

## Version Control And Storage

Use a content-addressed repository that works on local disk first and maps cleanly to S3/OpenDAL later.

Required:

- immutable content-addressed objects
- manifests with parent links, document UUID, branch, snapshot references, operation segment references, blob references, lookup records, tombstones, and signatures
- operation segments that may contain keypress-level operations but can be packed for storage efficiency
- snapshots for fast open
- compare-and-swap head updates where available
- deterministic candidate heads when CAS is unavailable or concurrent saves race
- automatic candidate reconciliation by fast-forward or operation-level merge
- UUID lookup and optional DOI alias lookup
- repository scanning for serverless lookup
- central lookup acceleration when a server exists
- content-addressed blobs for images and arbitrary binary objects
- shallow clone support where missing blobs produce placeholders and warnings
- pack files for local small-file mitigation
- crash-safe compaction by writing new packs, verifying them, and atomically swapping indexes
- tape/archive tombstones describing where recoverable data exists

Done when local disk and flat object-store tests prove create/open/save/reopen, parallel candidate save reconciliation, missing blob handling, lookup by UUID/DOI, pack compaction, and tombstone recovery metadata.

## Signing And Audit

Unsigned documents open normally. Signatures are trust/compliance indicators, mainly for audit, scientific fraud review, patent precedence, and 21 CFR-style workflows.

Required:

- Rust-native OpenSSH-compatible signing by default
- optional `ssh-keygen` helper only if useful
- multiple signatures per version
- algorithm agility in all hash and signature envelopes
- manifest/version signatures over source state and retained history reachable from the manifest
- exact-byte detached blob signatures keyed by content hash
- sidecar signatures for images and arbitrary binary objects
- typed semantic signature profiles, including image semantic profiles, FASTQ sequence-only profiles, and FASTQ full-content profiles
- trust states: unsigned, signed, trusted, untrusted, broken
- audit/recovery view for warnings, deleted comments, resolved suggestions, deleted citations, operation history, signatures, and recoverable tombstones

Do not sign rendered PDF/output, computed spreadsheet values, citation label renderings, equation renderings, volatile UI state, browser caches, or import provenance. Formula signatures cover formula source, not computed values.

Done when valid signatures verify after save/open, tampering is detected, multiple signatures verify independently, unsigned documents remain openable, and typed signature APIs can sign storage-independent byte or semantic profiles.

## Citations

Use a document-local bibliography database and structured citation labels.

Required:

- citation occurrence nodes, not normal links
- document-local reference records so repeated citations update together
- citation groups with locators, prefixes, suffixes, and suppress-author flags
- `citum` as the first rendering/integration target
- future CSL-JSON import/export as an adapter
- Paperpile-style Google Docs import research only as compatibility adapter logic; OpenDoc does not need to copy Paperpile's link embedding design

Done when updating a reference updates all dependent labels, citation labels survive save/open, merge tests cover citation movement and bibliography edits, and rendered labels remain projection/cache state outside source signatures.

## Spreadsheets

Spreadsheet v0 includes formula evaluation.

Required:

- sparse multi-sheet workbook model
- stable sheet/row/column/cell IDs
- typed cell values and formatting
- formula parser/evaluator
- deterministic dependency graph and invalidation
- named ranges
- Google Sheets API-shaped import/export
- graceful warnings for unsupported formulas or imports

Done when formulas evaluate deterministically, cached computed values are excluded from signatures, dependencies update after edits, imports fail cleanly on unsupported structures, and merge tests cover concurrent sheet/cell edits.

## Frontend And App Modes

The first serious UI target is a Tauri app with a hybrid TypeScript editor. The same app API must also run in browser tests and later browser deployments.

Required:

- GUI controls for every v0 document node and spreadsheet feature
- immediate local rendering of operations
- IME-safe text input and selection mapping
- paste normalization plus later structured paste/import
- undo/redo backed by operations
- save, autosave, close, reopen, import, export, verify, audit/recovery, and warning views
- Linux/macOS/Windows native packaging prerequisites documented
- native build checks include required assets such as icons

Done when the packaged app can create, edit, save, close, reopen, import, export, verify, and audit documents containing every v0 schema feature.

## Server Modes

HPC single-user web mode:

- runs behind external authentication, such as an HPC/Open OnDemand-style environment
- can access disk and S3-like storage
- assumes one authenticated user has full access to reachable repositories
- does not enforce document-level permissions

Multi-user service mode:

- owns authentication, permissions, sharing, presence, sync relay, and optional commit serialization
- enforces permissions outside the document format
- uses the same source schemas, binary records, and operation semantics as local mode

Done when both server modes pass integration tests against the same app API contract and repository fixtures.

## Import And Export

Required:

- Google Docs API-shaped import/export for document subset
- Google Sheets API-shaped import/export for spreadsheet subset
- `.doc`/`.docx` import first where practical
- unsupported import structures abort or degrade with explicit warnings according to fixture expectations
- debug JSON export for tests and inspection only

Done when fixture imports produce deterministic source states, unsupported cases are explicit, and round-trip exports preserve the v0 subset.

## Research Still Needed

- final merge architecture choice based on CRDT/custom/hybrid benchmarks and fuzz behavior
- exact binary encoding choice if deterministic CBOR becomes insufficient
- OpenDAL integration depth versus internal object-store trait
- browser key handling and browser signing model
- common HPC tape/archive recall interfaces and tombstone schema details
- Paperpile Google Docs metadata extraction behavior for import compatibility
- best TypeScript editor substrate for operation-first rich editing

## Milestones

1. Prove schema and binary records for docs, sheets, citations, blobs, signatures, manifests, operation segments, lookup records, packs, and tombstones.
2. Prove operation-level merge with realistic scenarios and fuzzing for rich documents.
3. Prove local disk repository with save/open/reopen, candidate reconciliation, shallow clone, signing, and audit/recovery.
4. Build the Tauri app GUI for all v0 schema features through the shared app API.
5. Add Google Docs/Sheets-shaped import/export and `.doc/.docx` import fixtures.
6. Add flat S3/OpenDAL-shaped storage and then real S3/OpenDAL adapters.
7. Add HPC single-user web wrapper.
8. Add multi-user service with authentication, permissions, presence, and sync relay.
9. Package and verify Linux/macOS/Windows desktop builds.

## Immediate Execution Order

Work in this order until the completion contract passes:

1. Freeze the v0 source schema and binary records enough for tests, without promising backward compatibility.
2. Build operation-level document editing and merge scenarios before polishing UI behavior.
3. Keep comments, suggestions, citations, formatting, equations, tables, images, and blobs in the early merge tests so the model cannot accidentally only work for plain text.
4. Finish local on-disk repository semantics first, including candidate heads, packed small-file mitigation, shallow clone, tombstones, and signing.
5. Implement the shared app API as the stable boundary for Tauri, browser, HPC single-user web, and multi-user service modes.
6. Build the Tauri GUI against that API, with hybrid TypeScript editing and Rust-owned source semantics.
7. Add Google Docs/Sheets-shaped import/export fixtures as the compatibility proof.
8. Add S3/OpenDAL once local disk semantics are proven.
9. Add HPC single-user serving with external authentication and no document-level permissions.
10. Add continuously running multi-user serving with authentication, permissions, sharing, presence, and sync relay.

## Implementation Sequence

Execute the work in this order unless a test result shows the architecture is wrong.

### 1. Baseline Source Formats

Deliver:

- canonical binary document records
- canonical binary spreadsheet records
- operation segment records
- manifest records
- lookup records
- tombstone records
- blob metadata records
- signature envelope records

Done when:

- every v0 node round-trips through binary encoding
- semantically identical states encode identically
- corrupt records fail cleanly
- JSON is limited to command contracts, tests, debug, import, and export

### 2. Operation-First Rich Document Core

Deliver:

- operations for every v0 document feature
- stable anchors for blocks, inline text, ranges, comments, suggestions, citations, equations, tables, rows, columns, cells, images, and attachments
- operation-backed undo/redo semantics
- deterministic validation and warning generation after each operation

Done when:

- every GUI-visible document edit has a Rust operation
- operations can be applied immediately for rendering
- keypress-level edits can be represented without forcing one S3 object per keypress
- storage batching is proven separate from rendering semantics

### 3. Merge Architecture Decision

Deliver:

- realistic synthetic scenarios for 1-3 active editors
- passive-viewer-equivalent replay tests
- deterministic fuzz tests
- comparison notes for existing CRDTs, custom state-machine design, block-UUID graph design, and any hybrid approach

Done when:

- all v0 document operation classes converge byte-for-byte at canonical projection level
- formatting, comments, suggestions, citations, equations, and tables survive concurrent edits or degrade with deterministic warnings
- no v0 merge path requires manual conflict resolution
- the chosen merge architecture is recorded in an ADR with fixture names

### 4. Local Repository And Version Control

Deliver:

- local on-disk object repository
- immutable content-addressed objects
- snapshots and operation segments
- candidate heads for serverless concurrent saves
- fast-forward and divergent-candidate reconciliation
- pack files for local small-file mitigation
- crash-safe compaction

Done when:

- create, save, close, reopen, lookup by UUID, and lookup by DOI work locally
- parallel save simulations reconcile deterministically
- small-operation workloads are packed without changing merge semantics
- interrupted writes cannot corrupt committed heads

### 5. Blobs, Shallow Clone, And Archive Recall

Deliver:

- content-addressed image and arbitrary binary blobs
- reusable exact-byte blob signatures keyed by hash
- sidecar signature storage
- missing-blob placeholders and warnings
- shallow clone behavior
- tape/archive tombstones
- optional central lookup acceleration
- repository/bucket scanning fallback

Done when:

- missing blobs do not prevent documents from opening
- restored blobs reconnect by hash without rewriting document source
- sidecar signatures verify independently of local path, S3 path, compression container, or archive location
- tombstones describe enough information to request recovery from tape/archive storage

### 6. Signing And Audit

Deliver:

- Rust-native OpenSSH-compatible signing
- multiple signatures per version
- algorithm-agile hash and signature envelopes
- source-state manifest signatures
- detached blob signatures
- typed semantic signature API for image profiles and FASTQ profiles
- audit/recovery views

Done when:

- unsigned documents open normally
- valid signatures show signed/trusted state
- tampering shows broken/untrusted state
- comments, suggestions, bibliography source, equations, formula source, and attachment references are signed
- rendered output, formula caches, citation renderings, equation renderings, volatile UI state, import provenance, and normal invisible editor IDs are excluded

### 7. Citations

Deliver:

- document-local bibliography database
- structured citation labels, not links
- citation groups with locator, prefix, suffix, suppress-author, and order metadata
- `citum` rendering adapter
- future CSL-JSON adapter boundary
- Paperpile metadata import research as adapter-only logic

Done when:

- one bibliography update updates all dependent citations
- citation labels survive save/open and merge
- deleted bibliography records and citation groups are hidden normally but retained for audit/recovery when data is present
- rendered citation strings are projection/cache state outside signatures

### 8. Spreadsheets

Deliver:

- sparse multi-sheet workbook
- stable sheet, row, column, and cell IDs
- typed cell values and formatting
- formula parser/evaluator
- dependency graph and invalidation from formula source
- named ranges
- Google Sheets-shaped import/export

Done when:

- formula results are deterministic
- computed values are regenerated from source and excluded from signatures
- concurrent sheet/cell edits merge deterministically
- unsupported formulas or imports abort or warn according to fixture expectations

### 9. Tauri App And Browser Contract

Deliver:

- Tauri v2 app for Linux, macOS, and Windows
- hybrid TypeScript rich editor
- browser contract using the same app API
- native file/repository picker
- recent documents
- autosave
- import/export controls
- signature indicators
- warnings and audit/recovery views
- build prerequisites and asset checks, including icons

Done when:

- the packaged app can create, edit, save, close, reopen, import, export, verify, and audit a document containing every v0 feature
- browser tests exercise the same command contract without forking source semantics
- browser signing remains explicitly postponed while signed-document opening and verification semantics are still represented

### 10. Server Modes

Deliver:

- HPC single-user web wrapper behind external authentication
- disk and S3 access from the HPC wrapper
- raw S3/OpenDAL mode without a coordination server
- multi-user service with authentication, permissions, sharing, presence, sync relay, lookup acceleration, and optional commit serialization

Done when:

- all modes use the same source schema, operation semantics, binary records, and repository API
- HPC mode has no unnecessary document-level permission model
- service permissions live outside the document format
- disk, flat object-store, and S3/OpenDAL conformance tests cover the target scale

### 11. Import And Export

Deliver:

- Google Docs API-shaped import/export
- Google Sheets API-shaped import/export
- `.doc/.docx` import through practical tooling
- fixtures for comments, suggestions, equations, citations, tables, images, attachments, and sheets
- explicit unsupported-feature behavior

Done when:

- imports produce valid OpenDoc source states
- high-risk unsupported imports abort rather than misrepresenting content
- lower-risk unsupported structures degrade with warnings
- exports preserve the v0 subset
- import provenance stays outside signed authored content

### 12. Release Verification

Deliver:

- binary golden fixtures
- schema validity tests
- merge scenario and fuzz tests
- repository crash/recovery tests
- local disk and S3/OpenDAL conformance tests
- pack-file tests
- shallow-clone and missing-blob tests
- signing and tamper tests
- typed semantic signature tests
- spreadsheet formula fixtures
- citation rendering fixtures
- import/export fixtures
- GUI smoke tests
- Tauri native checks
- browser/webview checks where practical

Done when:

- CI or named local commands prove canonical encoding, convergence, save/open, crash safety, signing, graceful degradation, import/export validity, and GUI coverage
- each test fixture names the product requirement it proves
- every known gap is either fixed, deferred, or listed in Research Still Needed

## Tracking Rule

For each work item, track:

- implementation path
- fixture or test name
- degradation behavior
- signature impact
- storage impact
- import/export impact

No item should be marked done without all relevant fields accounted for.

## Top-Level Completion Checklist

This product plan is complete when all checklist items below are checked and backed by the tracking fields above.

- [ ] Canonical binary schemas exist for documents, spreadsheets, citations, blobs, manifests, operations, signatures, lookup records, packs, and tombstones.
- [ ] Rich document operations cover paragraphs, headings, lists, marks, links, comments, suggestions, citations, equations, tables, images, attachments, warnings, and audit/recovery state.
- [ ] Spreadsheet operations cover sheets, rows, columns, cells, formatting, formulas, named ranges, merged cells, filters, validations, protected-range metadata, and deterministic evaluation.
- [ ] The merge engine converges automatically for realistic 1-3 editor scenarios and deterministic fuzz tests, including formatting, comments, suggestions, citations, equations, tables, and spreadsheets.
- [ ] Local disk object storage supports create, save, close, reopen, UUID/DOI lookup, candidate-head reconciliation, shallow clones, pack compaction, and crash recovery.
- [ ] S3/OpenDAL-shaped storage passes the same repository conformance tests as local disk, including serverless concurrent save simulations.
- [ ] Blob handling supports content-addressed images and arbitrary binary objects, shallow-clone warnings, reusable blob signatures, and tape/archive tombstones.
- [ ] Signing supports unsigned-open behavior, Rust-native OpenSSH-compatible signatures, multiple signatures, algorithm agility, detached blob signatures, source-state signatures, and typed semantic signatures.
- [ ] Citations use a document-local bibliography database, structured citation labels, `citum` rendering, merge tests, and a future CSL-JSON adapter boundary.
- [ ] Google Docs and Google Sheets API-shaped import/export preserve the v0 subset and fail or degrade with explicit warnings for unsupported structures.
- [ ] `.doc/.docx` import has practical tooling, fixtures, and explicit unsupported-feature behavior.
- [ ] The Tauri app can create, edit, save, close, reopen, import, export, verify, and audit documents containing every v0 schema feature.
- [ ] Browser mode exercises the same app API and source schema, with browser signing explicitly postponed but signed-document opening/verification represented.
- [ ] HPC single-user web mode works behind external authentication with disk and S3 access and no document-level permission burden.
- [ ] Multi-user service mode handles authentication, permissions, sharing, presence, sync relay, lookup acceleration, and optional commit serialization outside the document format.
- [ ] CI or named local commands cover canonical encoding, schema validation, merge convergence, storage conformance, crash safety, signing/tamper detection, import/export fixtures, GUI smoke tests, and native checks.
