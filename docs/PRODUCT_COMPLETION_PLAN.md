# OpenDoc Product Completion Plan

Status: canonical completion plan.

Purpose: track the work needed to finish OpenDoc as an open source Google Docs/Sheets-style product written primarily in Rust, with a hybrid TypeScript editor and Tauri/browser shells.

Done means every item below has an implementation, fixture or test, and documented graceful-degradation behavior where relevant. Research-only conclusions are not done until they are encoded as schema, operations, storage format, tests, or an explicit deferral.

This file is intentionally broader than the current prototype. It includes storage, merge, signing, citation, spreadsheet, Tauri, browser, HPC, service, import/export, and verification requirements from the design discussion so progress can be audited without rereading the thread.

## Product Scope

OpenDoc must support:

- rich documents with paragraphs, headings, lists, marks, links, comments, suggestions, citations, equations, tables, images, and arbitrary binary attachments
- spreadsheets with multiple sheets, stable row/column/cell identities, formatting, named ranges, dependency tracking, and deterministic formula evaluation
- collaborative editing for 1-3 active editors and about 5 passive viewers, treating passive viewers as potential editors
- single-user offline mode on local disk
- raw S3/OpenDAL-style object storage without a coordination server
- shallow cloning where missing blobs show placeholders and warnings
- binary canonical storage; JSON only for debug/API/import/export projections
- version signing, blob signing, and typed semantic signatures
- Google Docs/Sheets API-shaped import/export as the compatibility proof
- `.doc/.docx` import where available tooling allows
- Tauri desktop app for Linux, macOS, and Windows
- browser app support through the same app API
- HPC single-user web mode and multi-user service mode

## Fixed Decisions

- Rust owns document state, operations, merge, storage, signing, import/export, spreadsheet logic, and citation state.
- TypeScript owns the rich editor surface, DOM integration, selection, IME behavior, keyboard handling, and UI.
- Tauri v2 is the desktop shell.
- Local on-disk object storage is first-class before S3 becomes default.
- Storage should stay compatible with OpenDAL, but the internal `ObjectStore` trait is acceptable if simpler.
- Internal durable format is deterministic binary CBOR or another canonical binary encoding.
- Rendering never waits for commit batching.
- Keypress-level edits must be representable as operations.
- Operation segments are storage batches, not semantic merge units.
- Merge is operation/state based, not DOM based.
- Merges must always produce a valid openable document.
- Manual merge conflict resolution is out of scope for v0; bad cases degrade deterministically with warnings.
- Invisible IDs may be used for merge anchors, but are excluded from normal source-content signatures.
- Unsigned documents open normally.
- Signatures are visible trust/compliance indicators, not default access control.
- Signing uses Rust-native OpenSSH-compatible keys first; browser signing is postponed.
- Citations are first-class labels backed by a document-local bibliography database, using `citum` first.
- CSL import/export can be added later as an adapter.
- Equations store TeX/LaTeX source; rendered MathML/PDF output is projection only.
- Spreadsheet signatures cover formula source, not computed values.
- Rendered outputs, citation labels, formula caches, volatile UI state, and import provenance are not signed source state.
- Deletion removes data from current visible state while retained history keeps it for audit/recovery.
- Permissions are a service concern, not a document-format concern.

## Decisions To Preserve In APIs

- The app must be usable as an API/test prototype before the editor is polished.
- The document format must not assume there is a server that serializes commits.
- Local disk, flat S3/OpenDAL-shaped storage, real S3/OpenDAL, Tauri, browser, HPC single-user web, and multi-user service modes must share source schemas and operation semantics.
- The frontend may use any TypeScript editor architecture that emits OpenDoc operations and can run in both browser and Tauri.
- Browser signing is explicitly postponed; browser verification can arrive earlier.
- Native builds must include required assets, such as icons, and document Linux/macOS/Windows prerequisites.
- Users should not need to understand compaction, candidate heads, packs, tombstones, or shallow-clone internals in normal workflows.

## Runtime Modes

### Local Tauri Mode

Required:

- open/create/save repositories on local disk
- use the same operation and storage APIs as browser/service modes
- support local offline editing without a server
- verify signatures and indicate trust state
- eventually support S3/OpenDAL repositories

Done when a user can create, edit, save, close, reopen, import, export, and verify a document from the packaged desktop app.

### Browser Mode

Required:

- run the same app API contract as Tauri
- use browser-appropriate storage where needed
- open signed and unsigned documents
- defer browser-side signing until key handling is decided

Done when browser tests can exercise the same document operations and projections as Tauri without format forks.

### HPC Single-User Web Mode

Required:

- run on an HPC node behind external authentication
- access local disk and S3-like storage
- assume one authenticated user has full access to reachable data
- avoid document-level permissions in this mode

Done when the web wrapper can open and save the same repositories as local mode using server-side disk/S3 capabilities.

### Multi-User Service Mode

Required:

- own authentication, authorization, sharing, presence, object lookup, and sync relay
- optionally serialize commits server-side
- enforce permissions outside the document format

Done when service tests show users can collaborate within the target scale while unauthorized users cannot access heads, objects, blobs, comments, suggestions, or lookup records.

## Workstream 1: Schema And Binary Format

Deliverables:

- canonical source schema for documents, spreadsheets, comments, suggestions, citations, equations, tables, attachments, warnings, and provenance
- operation schema for every user-visible edit
- binary records for snapshots, operation segments, manifests, signatures, lookup records, tombstones, blob metadata, and packs
- debug JSON projections for inspection only

Done when:

- every source node round-trips through the canonical binary format
- semantically identical source states encode identically
- corrupt records fail cleanly
- signed bytes never rely on JSON
- schema validity is checked after every operation in merge tests

## Workstream 2: Rich Editor

Deliverables:

- TypeScript docs editor that renders OpenDoc state as a projection
- selection and anchor mapping between DOM and OpenDoc operations
- IME-safe input
- paste handling
- operation-backed paragraph insertion after stable top-level block IDs
- operation-backed block deletion
- operation-backed heading level updates by stable block ID
- operation-backed list item nesting/order updates by stable block ID
- operation-backed inline insertion by stable block and inline ID
- operation-backed inline deletion by stable inline ID
- operation-backed link target updates by stable link inline ID
- operation-backed citation occurrence insertion for existing document-local bibliography references
- operation-backed image block insertion by existing content-addressed blob hash plus alt-text editing and blob replacement by stable image block ID
- operation-backed attachment display name and media type updates by blob hash
- operation-backed spreadsheet sheet title renames and current-state sheet deletion by stable sheet ID
- operation-backed spreadsheet row and column axis add/delete by stable sheet ID and visible label
- operation-backed spreadsheet cell comments as signed source state with deleted-state retention
- operation-backed spreadsheet frozen row/column viewport metadata by stable sheet ID
- operation-backed mark addition and removal
- operation-backed comment body updates as signed source state
- operation-backed individual comment and comment-thread deletion with audit retention
- operation-backed delete suggestion creation over stable inline ranges
- operation-backed format suggestion creation over stable inline ranges
- operation-backed insert/delete/format suggestion acceptance that applies source changes
- operation-backed proposed insert suggestion text updates
- undo/redo backed by OpenDoc operations
- controls for all v0 nodes
- warning, audit, and recovery views

Done when:

- every v0 document node can be created and edited in the GUI
- local edits render immediately before persistence batching
- paste/import cannot create invalid state
- undo/redo produces operations and survives save/open
- GUI smoke tests cover comments, suggestions, citations, equations, tables, image blocks, attachments, and formatting

## Workstream 3: Automatic Merge

Deliverables:

- final choice between an existing CRDT, custom state-machine model, block-UUID graph, or hybrid
- operation-level merge engine independent of DOM structure
- stable anchors for blocks, text elements, comments, suggestions, citations, equations, tables, rows, columns, and cells
- deterministic warning semantics for degraded anchors

Required scenarios:

- concurrent typing in the same paragraph
- concurrent delete and formatting over overlapping ranges
- paragraph split/merge with comments and marks
- cross-block comments
- suggestion insert/delete/format/accept/reject
- citation movement and bibliography edits
- equation insert/delete as atomic source objects
- table row/cell insert/delete/format
- shuffled storage batches versus live operation streams

Done when:

- 1-3 active editor fuzz tests converge byte-for-byte at canonical projection level
- passive-viewer-equivalent replicas do not fork state
- every v0 operation class has synthetic merge scenarios
- every degraded merge remains openable and emits deterministic warnings
- no v0 merge requires manual conflict resolution

## Workstream 4: Version Control And Storage

Deliverables:

- content-addressed immutable objects
- manifests with parent links, branch heads, snapshots, operation segments, blobs, signatures, tombstones, UUID lookup, DOI aliases, and provenance
- compare-and-swap head updates for disk and S3-like stores
- serverless candidate heads when CAS is unavailable or races occur
- deterministic candidate resolution and fast-forward/merge handling
- filesystem-backed S3/OpenDAL-shaped conformance adapter before real network storage
- shallow clone support
- pack files for local small-file mitigation
- crash-safe compaction
- tape/archive tombstones and central lookup acceleration when a server exists

Done when:

- local repositories can create/open/update multiple documents
- raw object storage can operate without a commit server
- candidate heads from parallel users are deterministically resolved or merged
- interrupted writes cannot corrupt committed heads
- missing shallow-clone blobs show placeholders and warnings
- pack compaction is invisible to users
- UUID lookup, optional DOI lookup, and repository scanning can locate documents without a server
- tape tombstones describe where recoverable data exists

## Workstream 5: Signing And Audit

Deliverables:

- OpenSSH-compatible manifest/version signing
- multiple signatures per version
- exact-byte blob sidecar signatures keyed by content hash
- typed semantic signature profiles with algorithm agility
- trust states: unsigned, signed, trusted, untrusted, broken
- audit/recovery views for retained history and deleted comments

Signature boundaries:

- sign source state and retained history reachable from the manifest
- sign comments, suggestions, bibliography records, equations, formula source, and source attachment references
- do not sign rendered exports, computed formula values, rendered citation labels, rendered equations, volatile UI state, or import provenance
- do not include invisible editing IDs in normal content signatures
- allow specialized full-structure or forensic profiles later

Done when:

- signed manifests verify after save/open
- tampering with manifests, snapshots, operation segments, signatures, or blobs is detected
- multiple signatures verify independently
- unsigned documents open normally
- typed profiles can sign exact bytes, image semantics, FASTQ sequence-only content, and FASTQ full content at the API/design level

## Workstream 6: Citations

Deliverables:

- document-local bibliography database
- structured citation occurrence labels
- citation groups with locators, prefixes, suffixes, and suppress-author flags
- `citum` rendering adapter
- style switching through projection/cache updates
- future CSL-JSON import/export adapter
- Paperpile-style Google Docs link metadata import/export research kept as adapter logic only

Done when:

- updating one bibliography record updates all dependent labels
- deleting one bibliography record hides it from normal bibliography views while retaining it in audit/recovery state
- deleting one citation group hides it from normal citation views while retaining it in audit/recovery state
- citation labels survive save/open
- merge tests cover citation anchor movement and bibliography edits
- rendered labels are projection/cache, not signed source
- import/export can represent the chosen v0 citation subset

## Workstream 7: Spreadsheets

Deliverables:

- multi-sheet sparse workbook model
- stable sheet/row/column/cell IDs
- sheet metadata edits such as title rename by stable sheet ID
- formatting metadata
- formula parser/evaluator
- deterministic dependency graph and invalidation
- named range create, update, delete, and formula invalidation semantics
- Google Sheets-shaped import/export

Done when:

- formula fixtures pass deterministically on Linux, macOS, and Windows
- supported formulas include arithmetic, references, ranges, `SUM`, `AVERAGE`, `MIN`, `MAX`, and `COUNT`
- cycles and errors produce deterministic warnings/errors
- computed values can be regenerated from signed formula source
- save/open preserves formulas, formatting, axes, and named ranges
- import/export covers the selected v0 subset

## Workstream 8: Import And Export

Deliverables:

- Google Docs API-shaped import/export
- Google Sheets API-shaped import/export
- `.doc/.docx` import through practical converters or Rust/WASM-capable paths
- unsupported high-risk structure detection
- import provenance metadata outside signed authored content

Done when:

- realistic fixtures import to valid OpenDoc state
- unsupported high-risk structures abort rather than being misrepresented
- lower-risk unsupported styles warn and degrade gracefully
- imported documents round-trip through save/open
- exports represent the v0 subset in Google-shaped data

## Workstream 9: Product Shells

Deliverables:

- Tauri command wrappers over `opendoc-app-api`
- browser bindings over the same contract
- native file/repository picker
- recent documents
- autosave
- app-level undo/redo for local edit commands
- explicit close-document state with recent-document preservation and guardrails against editing, saving, signing, verifying, or exporting while no document is open
- plain-text paste handling for editable prototype fields
- import/export controls
- signature indicator
- warnings and audit/recovery views
- package/build checks for Linux, macOS, and Windows

Done when:

- desktop and browser shells exercise the same app API
- Tauri can be packaged for all target OSes
- users do not need to understand repository internals for normal create/open/save/close/reopen flows
- app failures surface actionable warnings or errors
- audit/recovery views expose retained deleted document comments, deleted spreadsheet cell comments, resolved suggestions, warning state, deleted citation records, and operation history
- undo/redo restores source state, records audit operations, persists after save/open, and leaves collaborative per-actor undo explicitly assigned to merge/editor work
- close-document clears current repository/signature/undo state without losing the recent-document list and refuses unsafe source-state commands until create/import/open
- paste handling strips rich clipboard markup unless a structured importer can preserve semantics safely

## Workstream 10: Verification

Required suites:

- binary golden fixtures
- schema validity tests
- merge scenario tests
- merge fuzz tests
- repository crash/recovery tests
- storage conformance tests for local disk, flat S3/OpenDAL-shaped filesystems, and real S3/OpenDAL-like backends
- pack-file tests
- shallow-clone and missing-blob tests
- signing and tamper tests
- typed semantic signature profile tests
- spreadsheet formula fixtures
- citation rendering fixtures
- import/export fixtures
- GUI smoke tests
- Tauri native checks
- browser/webview checks where practical

Done when CI or named local commands prove:

- canonical encoding
- convergence
- save/open
- crash safety
- signing and verification
- graceful degradation warnings
- import/export validity
- GUI coverage of every v0 schema feature

## Done Audit Checklist

A workstream is not complete unless all applicable answers are yes:

- Is the behavior implemented in Rust core, app API, and any required shell?
- Is the durable representation binary and deterministic?
- Are JSON forms limited to debug, API, test, import, or export boundaries?
- Can the feature save, close, reopen, and survive repository lookup?
- Does the feature have merge tests if it affects source state?
- Does the feature have degraded-state warnings for missing, deleted, corrupt, unsupported, or stale references?
- Is the signing boundary explicit, including whether the feature is source state, projection, cache, provenance, or volatile UI?
- Does import either preserve the supported semantics or abort/warn deterministically?
- Does the Tauri/browser contract expose the behavior without format forks?
- Is the verification command or fixture name recorded near the implementation?

## Immediate Next Milestones

1. Finish the Tauri/browser app API surface for every settled v0 schema feature.
2. Choose and prove the rich-text merge core against realistic scenarios and fuzz tests.
3. Replace the demo editor with a real TypeScript editor that emits OpenDoc operations.
4. Broaden candidate merge execution across local disk, flat S3/OpenDAL-shaped storage, service simulations, and spreadsheet operation classes.
5. Expand citation support around `citum` and document-local bibliography updates.
6. Expand Google Docs/Sheets and `.doc/.docx` import fixtures.
7. Extend the flat S3/OpenDAL-shaped conformance adapter into real OpenDAL/S3 adapters and test common stores.
8. Add package/build verification for Linux, macOS, and Windows.

## Final Completion Definition

The product is complete when a user can install or open OpenDoc, create or import a rich document or spreadsheet, edit every v0 feature, collaborate within the target scale, save to local disk or S3-like storage, reopen from UUID or DOI lookup, tolerate missing shallow-clone blobs, verify signatures when present, and export through the selected Google-shaped compatibility formats, with all critical behavior covered by tests.
