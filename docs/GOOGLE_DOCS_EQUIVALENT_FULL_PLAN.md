# OpenDoc Google Docs Equivalent Full Plan

Status: canonical trackable plan.

Purpose: define the complete implementation target for an open source Google Docs/Sheets-style product written primarily in Rust, with local-first storage, automatic collaboration merges, optional signing, citations, spreadsheets, Tauri desktop, browser, HPC, and service modes.

This plan is done only when every workstream below has implementation, fixtures or tests, and documented graceful-degradation behavior where relevant. A research conclusion is not done until it is encoded as schema, operation semantics, binary format, tests, or an explicit deferral.

## 1. Product Goal

Build OpenDoc as a Google Docs/Sheets-style editor with:

- rich documents: paragraphs, headings, lists, tables, links, marks, comments, suggestions, citations, equations, page breaks, images, and arbitrary binary attachments
- spreadsheets: multi-sheet sparse workbooks, formulas, formatting, named ranges, stable row/column/cell identities, and deterministic formula evaluation
- local single-user offline mode on disk
- raw object-store mode on S3/OpenDAL-style stores without a commit server
- collaborative editing for 1-3 active editors and about 5 viewer/editor-capable clients
- fully automatic merges that always produce an openable document
- optional cryptographic signing for versions, blobs, and semantic profiles
- Google Docs/Sheets API-shaped import/export as the main compatibility proof
- Tauri desktop app for Linux, macOS, and Windows
- browser app using the same app API and source schema
- HPC single-user web mode and multi-user service mode

## 2. Fixed Decisions

- Rust owns source schemas, operations, merge, storage, signing, spreadsheet logic, citations, import/export, and verification.
- TypeScript owns the rich editor surface, DOM integration, selection, IME, keyboard behavior, and browser/Tauri GUI.
- Tauri v2 is the desktop shell.
- The DOM is a projection, not the merge source of truth.
- Rendering must be immediate and cannot wait for persistence batching.
- Keypress-level operations must be representable.
- Batching, operation segments, packs, and compaction are storage optimizations, not editing semantics.
- The durable format is canonical binary, currently deterministic CBOR unless replaced during research.
- JSON is allowed only for debug views, command contracts, tests, and external import/export adapters.
- Backward compatibility is not required until the project leaves research mode.
- Permissions are not document-format semantics; they belong to repository opening or the multi-user service.
- Users should not need to understand packs, candidate heads, tombstones, shallow clones, or compaction.

## 3. Canonical Source Schema

Implement one source model shared by desktop, browser, HPC, and service modes:

- document UUID, optional DOI, title, metadata, provenance, warnings, and audit/recovery records
- stable block, inline, row, column, cell, comment, suggestion, citation, equation, and attachment IDs
- blocks for paragraphs, headings, lists, page breaks, tables, images, equations, and attachments
- inline text, links, citation labels, inline equations, marks, comments, suggestion anchors, and typed labels
- formatting marks as first-class ranges, not fragile rendered spans
- comments and suggestions as signed source state
- deleted comments, resolved suggestions, and deleted citations hidden by default but visible in audit/recovery views
- citations as structured labels backed by a document-local bibliography database
- equations as TeX/LaTeX source; rendered MathML/PDF is projection only
- spreadsheets as sparse multi-sheet workbooks with formulas, dependency invalidation, formatting, and named ranges
- attachment references by content hash with availability state for shallow clones

Done when every source node round-trips through canonical binary encoding and invalid source states are rejected or repaired with deterministic warnings.

## 4. Operation And Merge Model

This is the highest-risk workstream.

Required:

- operations are the only merge unit
- research and test existing CRDT libraries, custom state-machine/event models, block-UUID graphs, and hybrid designs
- use realistic synthetic scenarios and deterministic fuzz tests before settling the merge architecture
- local edits apply immediately for rendering, independently of commit batching
- concurrent typing in the same paragraph converges
- formatting survives insertions, deletions, paragraph splits, paragraph joins, and overlapping ranges where possible
- comments and suggestions anchor to stable UUID-backed ranges, including cross-block ranges
- deleted-anchor cases move to the nearest surviving location or become audit-only with warnings
- suggestions support insert, delete, format, accept, reject, provenance, and signing
- citations move as structured occurrence labels, not links
- equations merge as atomic source objects
- tables use stable row, column, and cell identities
- passive viewers use the same update path as editors where practical

Done when 1-3 active-editor fuzz tests and realistic scenarios converge byte-for-byte at canonical projection level, validate schema after every operation, and never require manual conflict resolution.

## 5. Version Control And Storage

Use a content-addressed repository that works on local disk first and maps cleanly to S3/OpenDAL.

Required:

- immutable content-addressed objects
- canonical binary snapshots and operation segments
- manifests with parent links, branch, document UUID, snapshot references, operation segment references, blob references, lookup records, tombstones, provenance, and signatures
- keypress-level operations stored directly or packed into operation segments
- compare-and-swap head updates where available
- deterministic candidate heads when CAS is unavailable or concurrent saves race
- automatic candidate reconciliation by fast-forward or operation-level merge
- UUID lookup and optional DOI alias lookup
- repository/bucket scanning when no lookup server exists
- optional central lookup acceleration when a server exists
- content-addressed blobs for images and arbitrary binary objects
- shallow clone support where missing blobs produce placeholders and warnings
- pack files for local small-file mitigation
- crash-safe compaction by writing new packs, verifying them, then atomically swapping indexes
- tape/archive tombstones describing where recoverable data exists

Done when tests prove create/open/save/reopen, parallel candidate save reconciliation, missing blob handling, lookup by UUID/DOI, pack compaction, and tombstone metadata.

## 6. Signing And Audit

Unsigned documents open normally. Signatures are optional trust/compliance indicators for audit, scientific fraud review, patent precedence, and 21 CFR-style workflows.

Required:

- Rust-native OpenSSH-compatible signing by default
- optional `ssh-keygen` helper if useful
- multiple signatures per version
- algorithm agility in hash and signature envelopes
- manifest/version signatures over source state and retained reachable history
- exact-byte detached blob signatures keyed by content hash
- sidecar signatures for images and arbitrary binary objects
- typed semantic signatures independent of storage layout, including image semantic profiles and FASTQ sequence-only/full-content profiles
- trust states: unsigned, signed, trusted, untrusted, broken
- audit/recovery view for warnings, deleted comments, resolved suggestions, deleted citations, operation history, signatures, and tombstones

Do not sign rendered output, computed spreadsheet values, rendered citation labels, rendered equation output, volatile UI state, browser caches, import provenance, or implementation-only invisible IDs. Formula signatures cover formula source.

Done when valid signatures verify after save/open, tampering is detected, multiple signatures verify independently, unsigned documents remain openable, and typed semantic signature APIs can sign storage-independent byte or semantic profiles.

## 7. Citations

Use a special citation label type, not normal links.

Required:

- document-local bibliography database
- citation occurrence labels that reference bibliography record IDs
- citation groups with locator/page metadata, prefixes, suffixes, suppress-author flags, and ordering
- `citum` as the first rendering/integration target
- rendered citation labels and bibliography output as projection/cache state outside signatures
- future CSL-JSON import/export as an adapter
- Paperpile-style Google Docs link metadata only as import/export compatibility research

Done when updating one bibliography record updates all dependent labels, citation labels survive save/open, merge tests cover citation movement and bibliography edits, and rendered labels are excluded from source signatures.

## 8. Spreadsheets

Spreadsheet v0 includes formula evaluation.

Required:

- sparse multi-sheet workbook model
- stable sheet, row, column, and cell IDs
- typed cell values and formatting
- deterministic formula parser/evaluator
- dependency graph and invalidation derived from formula source
- named ranges
- Google Sheets API-shaped import/export
- graceful warnings for unsupported formulas or import structures

Done when formulas evaluate deterministically, cached computed values are excluded from signatures, dependencies update after edits, imports fail cleanly or degrade with warnings, and merge tests cover concurrent sheet/cell edits.

## 9. Frontend And Tauri App

The first serious product target is a Tauri app with a hybrid TypeScript editor over the Rust app API.

Required:

- GUI controls for every v0 document and spreadsheet feature
- operation-backed editing, undo, redo, autosave, save, close, reopen, import, export, verify, and audit/recovery
- IME-safe editing and selection mapping
- paste normalization and structured paste/import later
- keyboard shortcuts and menus
- warnings and signature indicators
- native file/repository picker
- recent documents
- Linux/macOS/Windows prerequisites and native checks, including icons/assets
- browser tests using the same command contract

Done when the packaged app can create, edit, save, close, reopen, import, export, verify, and audit a document containing every v0 schema feature.

## 10. Server And Deployment Modes

Supported modes:

- local single-user disk mode: first-class default until S3 is common
- raw S3/OpenDAL mode: no coordination server, with CAS/listing semantics and simulated multi-user behavior
- HPC single-user web mode: runs behind external authentication, can access disk and S3, assumes one authenticated user with full access, and skips document-level permissions
- multi-user service mode: owns authentication, permissions, sharing, presence, sync relay, lookup acceleration, and optional server-managed commit serialization

Done when all modes share operation/storage semantics and integration tests prove local, HPC-style, and service-style app API behavior.

## 11. Import And Export

Required:

- Google Docs API-shaped import/export for the selected document subset
- Google Sheets API-shaped import/export for the selected spreadsheet subset
- `.doc`/`.docx` import where practical using easy external tooling first
- fixtures for comments, suggestions, equations, citations, tables, images, attachments, and sheets
- unsupported high-risk imports abort instead of silently misrepresenting content
- lower-risk unsupported structures degrade with explicit warnings
- import provenance stored outside signed authored content
- debug JSON export for tests and inspection only

Done when realistic fixtures import to valid OpenDoc states, save/open after import works, exports preserve the v0 subset, and unsupported cases are explicit.

## 12. Research Tasks Still Open

- final merge architecture choice based on CRDT/custom/hybrid benchmarks and fuzz behavior
- exact binary encoding choice if deterministic CBOR becomes insufficient
- OpenDAL integration depth versus internal object-store trait
- browser key handling and browser signing model
- common HPC tape/archive recall interfaces and tombstone schema details
- Paperpile Google Docs metadata extraction behavior for import compatibility
- best TypeScript editor substrate for operation-first rich editing
- `.doc`/`.docx` import tooling that is easy to install on Linux/macOS/Windows or rootless Linux

## 13. Milestones

1. Schema and binary records: documents, sheets, citations, blobs, signatures, manifests, operation segments, lookup records, packs, and tombstones round-trip deterministically.
2. Merge proof: rich-document operation merge converges under realistic scenarios and fuzzing.
3. Local repository proof: disk object store supports save/open/reopen, candidate reconciliation, shallow clone warnings, signing, and audit/recovery.
4. Tauri app proof: GUI edits every v0 feature through the shared app API and passes native checks.
5. Spreadsheet proof: formulas, dependencies, named ranges, import/export, and signatures work under deterministic tests.
6. Import/export proof: Google Docs/Sheets API-shaped fixtures preserve the selected subset.
7. Storage proof: S3/OpenDAL-compatible tests pass, with no commit server required.
8. Deployment proof: browser, HPC single-user web, and multi-user service modes share the same semantics.

## 14. Tracking Rules

- Mark items done only after tests or fixtures prove them.
- Do not count GUI-only behavior as done unless it goes through the operation/app API.
- Do not count storage as done unless save/open/reopen works after process restart.
- Do not count merge as done unless replay order and batching are varied in tests.
- Do not count signing as done unless tamper tests fail verification.
- Do not count import/export as done unless unsupported structures have explicit fixture expectations.
