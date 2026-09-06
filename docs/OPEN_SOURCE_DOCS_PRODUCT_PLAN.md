# OpenDoc Product Plan

Status: master tracker for the open source Google Docs/Sheets-equivalent product.

Tracking file: `docs/OPEN_SOURCE_DOCS_PRODUCT_PLAN.md`.

Purpose: capture the design decisions, implementation plan, research questions,
and done criteria discussed so far. This plan is finished only when every
requirement below is implemented, tested, explicitly deferred, or converted into
a bounded research spike with an exit test.

## Product Target

Build a Rust-first, open source document suite with:

- Google Docs-style rich documents.
- Google Sheets-style spreadsheets with v0 formula evaluation.
- Local offline mode.
- Single-user raw object-store mode without a commit server.
- Future S3/OpenDAL storage.
- Tauri desktop app for Linux, macOS, and Windows.
- Browser app using the same app API.
- HPC single-user web mode behind external authentication.
- Multi-user service mode with authentication and permissions.
- Collaborative editing for 1-3 active editors and about 5 viewers/editors.
- Fully automatic merge with graceful degradation, never manual merge conflicts.
- Canonical binary storage, not JSON.
- Optional cryptographic signatures that never prevent unsigned documents from
  opening normally.
- Blob, image, FASTQ, document, comment, citation, and version signing semantics.
- Document-local citation database using `citum` first, with CSL import/export
  left as an adapter.
- Google Docs/Sheets-shaped import/export as the compatibility proof.

## Non-Negotiable Constraints

- Rendering must not wait for batching.
- Every keypress must be representable as an operation.
- Batching and packing are storage/network optimizations only.
- The DOM is a projection, not the source of truth.
- Merge must operate on document operations/state.
- Merge must always produce a valid openable document.
- Formatting marks, comments, suggestions, citations, equations, tables,
  spreadsheets, and deleted anchors must be covered by merge tests.
- Implementation-only invisible IDs are not included in document source hashes.
- Rendered output, cached formula values, citation label renderings, MathML/PDF
  equation output, volatile UI state, and import provenance are not source
  signed.
- Images and arbitrary binary blobs are content-addressed and separately
  signable.
- Typed semantic signatures must be storage independent.
- Deleted objects disappear from current state but remain recoverable from
  history where retained.
- Users should not need to know about packs, tombstones, shallow clones, sidecar
  signatures, or candidate heads.

## Current Architecture Decisions

- Core language: Rust.
- Desktop shell: Tauri v2.
- Frontend: hybrid TypeScript UI with Rust core behind a stable app API.
- Storage v0: local on-disk object repository.
- Storage abstraction: use OpenDAL if it stays simple; otherwise keep a small
  internal object-store trait compatible with OpenDAL later.
- Binary metadata: deterministic CBOR is acceptable during research.
- Backward compatibility: not required until the project leaves research mode.
- Citations: special citation nodes plus document-local bibliography records,
  not ordinary links.
- Citation renderer: start with `citum`.
- Equation source: TeX/LaTeX.
- Signing identity: Rust-native OpenSSH-compatible signing by default.
- Browser signing: postponed.
- Permissions: service-layer concern only; not part of document source format.
- Import priority: Google Docs-shaped import and `.doc/.docx` practical import
  before export polish.

## Runtime Modes

### Tauri Local

- Uses local filesystem object repositories first.
- Later supports S3/OpenDAL.
- Can read local OpenSSH-compatible keys for signing.
- Owns local import/export helpers where browser cannot.

### Browser Local

- Uses the same app API and source model.
- Uses browser-suitable storage or a remote service.
- Supports verification before browser-side signing.
- Does not assume direct filesystem access.

### HPC Single-User Web

- Runs on an HPC node behind external authentication, such as Open OnDemand.
- Can access disk and S3-like storage.
- Assumes the authenticated user has full access to reachable storage.
- Does not handle multiple users or document-level permissions.

### Multi-User Service

- Continuously running service.
- Owns authentication, permissions, sharing, presence, and sync relay.
- May serialize commits, but document semantics must not depend on that.

## Source Model

Required v0 schema coverage:

- Document UUID and optional DOI metadata.
- Stable block IDs for paragraphs, headings, lists, tables, image blocks, and
  equation blocks.
- Stable inline IDs for text runs, links, citations, footnote references,
  equations, comments, and suggestion anchors.
- Text marks for Google Docs-like formatting.
- Tables with stable rows and cells.
- Comments and suggestions as signed source-state records.
- Citations as structured citation labels backed by a document-local
  bibliography database.
- Images and attachments as hash-addressed blob references.
- Equations as TeX/LaTeX source objects.
- Provenance and warnings for import/degradation/audit views.

Done when: canonical binary round-trip tests cover every v0 node type, and a
debug projection exists where useful for tests and UI development.

## Spreadsheet Model

Required v0 schema coverage:

- Multi-sheet sparse workbooks.
- Stable sheet, row, column, and cell identities.
- Cell values, formulas, formatting, comments, validations, named ranges,
  frozen panes, filters, protected-range warnings, and merged ranges.
- Deterministic formula evaluation.
- Formula source is signed; computed values are cached/projection state only.
- Dependency graph may be cached in RAM and rebuilt lazily.

Done when: Google Sheets-shaped fixtures import/export the supported subset, and
formula evaluation passes deterministic Rust and browser mock contract tests.

## Operation And Merge Model

The merge system is the highest-risk area.

Plan:

- Treat documents as operation-driven source state, not as DOM trees.
- Research and test CRDT sequence/block approaches, state-machine/event models,
  and block-UUID graph approaches.
- Use stable local IDs where they simplify automatic merge.
- Keep operation granularity fine enough for every keypress.
- Allow operation segments to pack many operations without changing rendering.
- Make comments, suggestions, citations, equations, tables, formatting, and
  spreadsheet edits first-class operations.
- Resolve all merge cases automatically.
- Emit deterministic warnings or recovery metadata for degraded cases.

Required merge scenarios:

- Concurrent insert/delete in the same paragraph.
- Overlapping formatting marks.
- Paragraph split/merge with comments.
- Comments spanning blocks.
- Deletion of all text referenced by a comment.
- Suggestion insertion, deletion, and formatting changes.
- Citation anchor movement and bibliography edits.
- Table row/cell edits.
- Equation insertion/deletion/source edit.
- Image/blob replacement.
- Spreadsheet cell, row, column, sheet, formula, validation, filter, and merge
  range edits.
- Passive viewer becoming editor.

Done when: realistic synthetic scenarios and fuzz tests for 1-3 replicas
converge byte-for-byte at canonical projection level, and invalid documents are
test failures.

## Storage And Version Control

Repository records:

- Content-addressed objects.
- Snapshots.
- Operation segments.
- Manifests.
- Branch heads and candidate heads.
- Lookup records for UUID and optional DOI.
- Tombstones and archive recall hints.
- Blob objects.
- Blob sidecar signatures.
- Packs and pack indexes.

Commit granularity:

- UI operations may be keypress-level.
- Operation segments are persistence units.
- Manifests commit source snapshots and operation segment heads.
- Batching is independent of viewing and local rendering.

Local disk:

- Start with loose objects.
- Add append-only packs when object count becomes painful.
- Maintenance may rewrite packs atomically.
- Use crash-safe writes and head updates.

S3/OpenDAL:

- Use the same object semantics as local disk.
- Avoid requiring a commit server for single-editor usage.
- Use CAS-like head update patterns where available.
- Reconcile divergent candidate heads automatically.

Shallow clone:

- Allow documents to reference missing blobs.
- Missing blobs show placeholders and warnings.
- Shared content-addressed blobs can be reused across documents and clones.

Tape/archive:

- Use tombstones with recall metadata for recoverable objects.
- Allow central lookup acceleration where a server exists.
- Without a server, support bucket/repository scanning for lookup rebuild.

Done when: local and S3/OpenDAL-shaped conformance tests cover save/open,
manifest integrity, snapshot integrity, operation chain integrity, lookup,
tombstones, shallow clone warnings, pack compaction, and crash recovery.

## Signing Model

Signing modes:

- Unsigned documents open normally.
- Signatures are visual/audit trust indicators.
- Multiple parties may sign.
- Nobody is required to sign.
- Algorithm agility is required immediately.
- OpenSSH-compatible identities are the default v0 identity model.

Signed payloads:

- Manifest/version source state.
- Exact-byte blobs.
- Comments and suggestions as source state.
- Citation database and citation occurrence source.
- Equations by source text.
- Spreadsheet formula source and source metadata.
- Typed semantic profiles for images and FASTQ.

Excluded from source signatures:

- Rendered PDF or visual output.
- Computed formula values.
- Citation label rendering.
- MathML generated from TeX.
- Volatile UI state.
- Import provenance.
- Implementation-only invisible IDs.

Sidecars:

- Blob signatures should be sidecars unless a format-specific embedded
  signature is clearly useful.
- Sidecars must identify target hash, signature algorithm, signer, signature
  payload type, and signed profile.

Done when: tampering with manifests, snapshots, operation segments, lookup
records, tombstones, blobs, and signature targets is detected after save/open,
while unsigned documents still open normally.

## Citations

Plan:

- Use a document-local reference database.
- Citation occurrences reference local bibliography IDs.
- Multiple occurrences can point to one bibliography record.
- Citation labels are special inline nodes, not links.
- Use `citum` first for citation rendering/modeling.
- Add CSL import/export later if useful.
- Paperpile's Google Docs strategy is useful mainly as evidence that full
  citation metadata should travel with the document, not require database
  access.

Done when: citation records and occurrences can be edited, merged, saved,
signed, reopened, and rendered through `citum`, with tests showing that updating
one bibliography record updates all occurrences.

## Import And Export

Priorities:

- Google Docs API-shaped document fixtures.
- Google Sheets API-shaped spreadsheet fixtures.
- Practical `.doc/.docx` import where tooling is easy to install.
- Export after import proves the v0 source model.

Unsupported content:

- Abort import for high-risk unsupported structures.
- Warn and degrade only when semantics remain clear.
- Preserve external IDs only as provenance if useful.

Done when: fixtures either round-trip supported content, warn with deterministic
degradation, or abort deterministically.

## Tauri And GUI Plan

The GUI must eventually expose the full v0 schema:

- Rich text editing.
- Formatting marks.
- Paragraphs, headings, lists, tables, images, attachments, equations, footnotes.
- Comments and suggestions.
- Citations and bibliography editor.
- Spreadsheet editing and formula evaluation.
- Import/export.
- Save/open/reopen.
- Signature verification indicators.
- Audit/recovery warnings.

Implementation rule:

- TypeScript owns DOM editing, selection, IME, paste, keyboard/mouse behavior,
  menus, and view state.
- Rust owns source state, operations, merge, storage, signing, citations,
  formulas, and import/export.
- Tauri commands and browser mocks must share one app API contract.

Done when: Linux/macOS/Windows app builds and a GUI smoke suite exercises every
v0 schema feature without changing source semantics.

## Research Spikes

Each spike must end in a decision plus tests or fixtures:

- Merge model comparison: CRDT library, custom state machine, block graph, or
  hybrid.
- Realistic merge fuzzer and scenario generator.
- OpenDAL versus internal object-store trait.
- S3 candidate-head reconciliation without a commit server.
- Pack layout for many small objects on local disks and object stores.
- Tape/archive recall metadata and lookup rebuild.
- Paperpile citation embedding behavior.
- `citum` source model and CSL adapter boundary.
- `.doc/.docx` import tooling on Linux/macOS/Windows.
- Browser signing options.
- HPC storage/access assumptions for the target facility.

## Implementation Phases

### Phase 1: Source Schema

Implement and test canonical Rust source types for documents, spreadsheets,
citations, comments, suggestions, equations, blobs, warnings, and provenance.

Exit criteria: binary round-trip and debug projection tests for every v0 schema
record.

### Phase 2: Operation Model

Implement operation types for all v0 user-visible edits.

Exit criteria: each operation applies locally, renders immediately, saves to an
operation segment, replays after open, and has at least one merge test.

### Phase 3: Local Repository

Implement the on-disk object store, manifests, heads, snapshots, operation
segments, lookup, tombstones, blobs, signatures, and initial packs.

Exit criteria: local conformance tests cover save/open, integrity failures,
shallow clone warnings, lookup rebuild, tombstone recall, and compaction.

### Phase 4: Merge Proof

Build scenario and fuzz tests for formatted documents and spreadsheets.

Exit criteria: 1-3 replicas converge deterministically across operation
orderings, with automatic warnings for degraded cases.

### Phase 5: App API

Stabilize a UI-facing API for Tauri, browser, HPC, and service wrappers.

Exit criteria: Tauri commands, browser mocks, and Rust tests use the same
contract for every v0 schema feature.

### Phase 6: Tauri GUI

Build the desktop app as the first full interactive surface.

Exit criteria: GUI smoke tests cover create, edit, save, close, reopen, import,
verify, audit, and recovery paths for every v0 feature.

### Phase 7: Compatibility

Implement Google Docs/Sheets-shaped import/export and `.doc/.docx` import.

Exit criteria: supported fixtures round-trip; unsupported high-risk fixtures
abort; degraded fixtures warn deterministically.

### Phase 8: S3/OpenDAL And HPC

Add object-store backends and the HPC single-user server wrapper.

Exit criteria: storage conformance passes on local disk and S3/OpenDAL-shaped
stores; HPC wrapper preserves single-user semantics.

### Phase 9: Multi-User Service

Add service authentication, permissions, sharing, presence, and sync relay.

Exit criteria: service tests prove permissions and collaboration without
changing document, merge, storage, or signing semantics.

### Phase 10: Product Hardening

Package, benchmark, harden, and document the product.

Exit criteria: CI covers Rust tests, frontend build, GUI smoke, mock contract,
native checks, import fixtures, storage conformance, signing verification, and
merge fuzz smoke.

## Completion Checklist

- [ ] Rich document schema complete.
- [ ] Spreadsheet schema and formula v0 complete.
- [ ] Citation model using `citum` complete.
- [ ] Operation model complete for every v0 edit.
- [ ] Automatic merge proof complete.
- [ ] Local repository complete.
- [ ] Pack/small-object mitigation complete.
- [ ] S3/OpenDAL-shaped store complete.
- [ ] Shallow clone behavior complete.
- [ ] Tape/archive tombstone and lookup behavior complete.
- [ ] Manifest/version signing complete.
- [ ] Blob and sidecar signing complete.
- [ ] Image and FASTQ semantic signature profiles complete.
- [ ] Google Docs/Sheets-shaped import/export complete.
- [ ] `.doc/.docx` import complete or explicitly deferred with fixture evidence.
- [ ] Tauri desktop GUI complete.
- [ ] Browser app shell complete.
- [ ] HPC single-user web mode complete.
- [ ] Multi-user service mode complete.
- [ ] Linux/macOS/Windows packaging complete.
- [ ] CI verification complete.

## Immediate Next Work

1. Finish app-level integrity tests for corrupt/missing snapshots and operation
   segments.
2. Complete operation coverage for remaining rich document edits.
3. Build merge scenario tests for formatting, comments, suggestions, citations,
   equations, tables, and spreadsheet edits.
4. Add `citum` integration behind the citation model.
5. Continue Tauri GUI controls until every v0 schema feature is editable.
6. Add storage conformance tests that can later run unchanged against OpenDAL.
7. Add import fixtures for Google Docs-shaped documents and practical `.docx`
   samples.
8. Keep updating this file with concrete test names as each item lands.
