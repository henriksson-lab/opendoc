# OpenDoc Full Product Plan

Status: canonical planning checklist for the Google Docs/Sheets equivalent.

This file is done when every requirement from the design discussion is either
implemented with tests, explicitly deferred with rationale, or listed as an open
risk with a validation plan. A feature is not done because it is described here;
it is done only when source format, operations, storage behavior, UI/API surface,
tests or fixtures, and graceful-degradation behavior all exist where applicable.

## Product Goal

Build an open source Google Docs/Sheets-style product, written primarily in
Rust, with a hybrid TypeScript frontend, local-first storage, automatic
collaboration merges, optional cryptographic signing, citations, spreadsheets,
Tauri desktop support, browser support, HPC single-user web support, and a later
multi-user service.

The first serious prototype should prove the document model, binary object
format, operation model, storage semantics, signing boundaries, spreadsheet
schema, citations, and app API through tests. UI polish comes after those
semantics are hard to break.

## Non-Negotiable Requirements

- Source data is canonical binary, currently deterministic CBOR unless replaced
  during research. JSON is only for debug, tests, API projections, and import or
  export adapters.
- Rendering never waits for batching. Every keypress can become an operation,
  while persistence may batch or pack operations independently.
- Merge is fully automatic. It must always produce an openable document and use
  deterministic warnings or degraded states instead of manual conflict prompts.
- Merge works on source operations/state, not on DOM structure.
- Formatting, comments, suggestions, citations, equations, tables, images,
  attachments, spreadsheet metadata, and deleted anchors are part of merge tests.
- Local disk object storage is first-class first; S3/OpenDAL object storage must
  use the same semantics later.
- Users should not need to understand packs, compaction, shallow clones,
  candidate heads, tombstones, signatures, or lookup internals.
- Unsigned documents open normally. Signatures are trust/audit indicators.
- Backward compatibility is not required until the project leaves research mode.

## Runtime Modes

- Tauri desktop: Linux, macOS, and Windows app with minimal external
  dependencies. Rust owns storage, signing, source formats, merge, import/export,
  citations, and spreadsheet evaluation. TypeScript owns rich editor UX.
- Browser local mode: same app API and source semantics, with browser-suitable
  storage. Browser signing is postponed until key handling is clear.
- HPC single-user web mode: runs on an HPC node behind external authentication,
  can access disk and S3-like storage, assumes one authenticated user with full
  access, and does not enforce document-level permissions.
- Multi-user service mode: continuously running shared service that owns
  authentication, permissions, sharing, presence, sync relay, and optional
  server-side commit serialization. Permissions stay outside the document format.

## Source Schema Work

Implement one canonical source model covering:

- document UUIDs, optional DOI metadata, provenance, warnings, and audit records
- stable internal IDs for blocks, text elements, ranges, sheets, rows, columns,
  cells, citations, comments, suggestions, blobs, and lookup records where they
  improve merge, recovery, shallow clone, or audit behavior
- paragraphs, headings, lists, page breaks, tables, images, attachments, links,
  inline marks, comments, suggestions, equations, citations, and footnotes
- equations as TeX/LaTeX source; rendered MathML/PDF is projection only
- citations as structured labels backed by a document-local bibliography
  database; use `citum` first, with CSL import/export left as an adapter
- spreadsheets with sparse multi-sheet workbooks, formula source, deterministic
  formula evaluation, dependency invalidation, named ranges, comments, filters,
  protected-range warnings, validations, frozen panes, merge ranges, and Google
  Sheets-shaped fixtures

Done when every v0 node round-trips through canonical binary encoding, invalid
states are rejected or repaired with deterministic warnings, and import/export
fixtures prove the Google Docs/Sheets-shaped subset.

## Operation And Merge Work

This is the highest-risk area.

Research and prototype both existing CRDT approaches and custom state-machine
models. Use realistic synthetic scenarios and fuzz testing before freezing the
model. Invisible block or paragraph UUIDs are allowed as merge anchors, but
implementation-only IDs must stay outside normal source-content signatures.

Required merge scenarios:

- concurrent typing in the same paragraph
- concurrent formatting over overlapping ranges
- insert/delete/format interactions
- paragraph split and join with marks
- comment and suggestion anchors moving across edits
- deleted anchors degrading to nearest useful context with warnings
- citation occurrence movement and bibliography edits
- equation source edits as atomic structured changes
- table row/column/cell edits with stable identities
- spreadsheet cell, formula, named-range, and metadata edits
- 1-3 active editors and about 5 replicas/viewers, with no special passive
  viewer semantics

Done when fuzz and scenario tests converge byte-for-byte at canonical projection
level, schema validation passes after every operation, and no merge requires a
user decision.

## Version Control And Storage Work

Use immutable content-addressed objects with manifest commits.

Required records:

- manifests with parent links, document UUID, branch/head metadata, snapshot
  references, operation segment references, blob references, lookup references,
  tombstone references, warnings, and signatures
- operation segments that may contain keypress-level operations
- snapshots for faster open
- pack files for local disk small-file mitigation
- lookup records for document UUID and optional DOI aliases
- candidate heads for stores without reliable compare-and-swap
- tombstones describing where archived or tape-backed data can be recovered
- sidecars or equivalent embedded records for blob signatures

Required behavior:

- local create/open/save/reopen
- crash-safe write and compaction
- deterministic candidate-head reconciliation by fast-forward or operation merge
- serverless lookup by bucket/repository scan
- central lookup acceleration when a server exists
- shallow cloning where missing blobs become placeholders with warnings
- no hard deletion for now; history retains deleted material

Done when local disk and S3/OpenDAL-shaped object-store tests pass the same
conformance suite for save/open, concurrent saves, reconciliation, shallow clone,
pack compaction, lookup, missing blobs, and tombstone metadata.

## Signing And Audit Work

Signing must be independent of storage layout where the data type supports it.

Required:

- Rust-native OpenSSH-compatible identities by default
- optional `ssh-keygen` helper only if useful
- multiple signatures per version or object
- algorithm agility from the start
- source-state manifest/version signatures
- exact-byte detached signatures for arbitrary binary blobs
- reusable hash-based image/blob signatures
- typed semantic signatures, including image semantic profiles, FASTQ
  sequence-only profiles, and FASTQ full-content profiles
- trust states: unsigned, signed, trusted, untrusted, broken
- audit/recovery views for signatures, warnings, deleted comments, resolved
  suggestions, deleted citations, operation history, and tombstones

Do not include rendered output, computed spreadsheet values, citation label
renderings, equation renderings, volatile UI state, browser caches, import
provenance, or implementation-only invisible IDs in normal source signatures.
Formula signatures cover formula source, not computed values.

Done when signatures verify after save/open, tampering is detected, multiple
signatures verify independently, unsigned documents remain normal, and typed
semantic signature APIs can sign storage-independent byte or semantic profiles.

## Citation Work

Use OpenDoc citation nodes, not link hacks.

Required:

- document-local reference database
- structured citation occurrence labels
- citation groups with locators, prefixes, suffixes, and suppress-author flags
- repeated citations update when the local reference changes
- citation rendering through `citum`
- future CSL or CSL-JSON import/export adapter
- Paperpile Google Docs embedding research only for import compatibility lessons

Done when references and occurrence labels save/open, merge safely, update
together, render through `citum`, and keep rendered labels outside signatures.

## Import And Export Work

Compatibility is proven through Google-shaped adapters, not by implementing full
Word/OpenOffice schemas.

Required:

- Google Docs API-shaped import/export for the chosen v0 document subset
- Google Sheets API-shaped import/export for the chosen v0 spreadsheet subset
- `.doc`/`.docx` import where practical, especially before export polish
- abort import for unsupported high-risk structures
- warnings for recoverable unsupported features
- debug JSON export for tests and inspection only

Done when fixtures produce deterministic source states, unsupported structures
have explicit expected behavior, and exports preserve the supported subset.

## Frontend And App Work

The first serious product shell is a Tauri app with a hybrid TypeScript editor.

Required:

- GUI controls for every v0 document and spreadsheet schema feature
- immediate local rendering of operations
- IME-safe text input, selection mapping, paste handling, undo/redo, keyboard
  shortcuts, and decorations
- operation-backed editing for rich text, marks, comments, suggestions,
  citations, equations, tables, images, attachments, footnotes, and spreadsheets
- save, autosave, close, reopen, import, export, verify, audit/recovery, warning
  views, and degraded-state placeholders
- native build verification including required Tauri assets such as icons

Done when the desktop app can create, edit, save, close, reopen, import, export,
verify, and audit documents containing every v0 feature on Linux, macOS, and
Windows, with the same app API usable by browser and server modes.

## Milestones

1. Schema and binary records: complete canonical records for docs, sheets,
   citations, blobs, signatures, manifests, operation segments, lookup records,
   packs, and tombstones.
2. Merge proof: complete operation-level rich document and spreadsheet merge
   tests with deterministic fuzzing and realistic 1-3 editor scenarios.
3. Local repository: complete on-disk object storage with candidate-head
   reconciliation, shallow clone, pack compaction, signatures, audit, and
   recovery metadata.
4. Tauri app: complete GUI/API support for all v0 schema features.
5. S3/OpenDAL: pass the same object-store conformance suite against real or
   realistic S3/OpenDAL backends.
6. Browser and HPC modes: run the same app API through browser local storage and
   single-user HPC web storage.
7. Multi-user service: add authentication, permissions, sharing, presence, sync
   relay, and optional server-side commit coordination without changing source
   semantics.
8. Import/export proof: complete Google Docs/Sheets-shaped fixture coverage and
   practical `.doc/.docx` import.

## Open Research Tasks

- Final merge architecture choice: CRDT library, custom state machine, or hybrid.
- Exact binary encoding choice if deterministic CBOR becomes too limiting.
- OpenDAL integration depth versus a small internal object-store trait.
- Browser signing and key handling.
- HPC tape/archive recall conventions and tombstone details.
- Paperpile Google Docs metadata extraction behavior for import compatibility.
- Best TypeScript editor substrate for operation-first rich editing.
- Performance thresholds for local disk, packed local storage, S3-like object
  stores, and common HPC filesystems.

## Tracking Files

- Product checklist: `docs/OPENDOC_FULL_PRODUCT_PLAN.md`
- Existing detailed implementation plan: `docs/GOOGLE_DOCS_EQUIVALENT_PLAN.md`
- Tauri/browser architecture notes: `forme.md`
- Schema docs: `docs/schema/`
- Research notes: `docs/research/`
- ADRs: `docs/adr/`
