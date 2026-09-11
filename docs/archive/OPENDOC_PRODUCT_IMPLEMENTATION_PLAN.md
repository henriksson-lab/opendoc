# OpenDoc Product Implementation Plan

Status: canonical tracker for the open source Google Docs/Sheets equivalent.

This file is done only when every item below is implemented with tests,
explicitly deferred with a reason, or split into a smaller tracked plan with an
exit test. A feature is not done because it exists in one layer; it is done when
source schema, operation/API support, canonical binary persistence, save/open
coverage, merge coverage where relevant, and graceful degradation behavior all
exist.

## Product Target

Build a Rust-first open source document suite with:

- Google Docs-style rich documents.
- Google Sheets-style spreadsheets with v0 formula evaluation.
- Collaborative editing for 1-3 active editors and about 5 viewers, with
  viewers treated as potential editors.
- Local offline mode on ordinary disk.
- Raw object-store mode without a commit server.
- Future S3/OpenDAL storage with the same semantics as local disk.
- Tauri v2 desktop app for Linux, macOS, and Windows.
- Browser app using the same app API.
- HPC single-user web mode behind external authentication, with disk and S3-like
  access but no document-level permissions.
- Future continuously running multi-user service with authentication,
  authorization, sharing, presence, and sync relay.
- Google Docs/Sheets API-shaped import/export as the compatibility proof.
- Practical `.doc`/`.docx` import where tooling allows.
- Optional cryptographic signatures that never stop unsigned documents from
  opening normally.

## Non-Negotiable Decisions

- Source data is canonical binary, currently deterministic CBOR unless research
  shows a better binary format. JSON is only for debug, tests, API projections,
  and import/export adapters.
- Rendering never waits for batching. Every keypress can be represented as an
  operation, while persistence may batch operations into segments later.
- Merge is fully automatic. It must always produce an openable document with
  deterministic warnings or degraded states instead of manual conflicts.
- Merge works on operations and source state, not on DOM structure.
- Stable invisible IDs may be used as merge anchors, but implementation-only IDs
  are excluded from normal source-content signatures.
- Backward compatibility is not required until the project leaves research mode.
- Users should not need to understand packs, compaction, shallow clones,
  candidate heads, tombstones, signatures, or lookup internals.
- Permissions are service/repository concerns, not document-format state.

## Source Schema

Implement one canonical source model covering:

- Document UUIDs, optional DOI metadata, title, locale, provenance, warnings,
  and audit records.
- Stable block, inline, range, table, blob, comment, suggestion, citation, sheet,
  row, column, and cell IDs where they improve merge or recovery.
- Paragraphs, headings, lists, page breaks, tables, images, attachments, links,
  footnotes, inline marks, comments, suggestions, equations, and citations.
- Citations as structured citation labels backed by a document-local
  bibliography database, not ordinary links.
- Equations as TeX/LaTeX source. Rendered MathML/PDF is projection only.
- Spreadsheets with sparse multi-sheet workbooks, formula source, deterministic
  evaluation, dependency invalidation, named ranges, comments, filters,
  protected-range warnings, validations, frozen panes, and merged ranges.

Done when every v0 node round-trips through canonical binary encoding, invalid
states are rejected or repaired with deterministic warnings, and Google-shaped
fixtures prove the supported subset.

## Operation And Merge

This is the highest-risk workstream. Research and prototype CRDT libraries,
custom state-machine/event models, and hybrid stable-ID/block-sequence models
against realistic tests before freezing the design.

Required semantics:

- Operations are the only merge unit.
- Concurrent typing in the same paragraph converges.
- Formatting ranges survive insertions, deletions, paragraph splits, and joins
  where possible.
- Comments and suggestions anchor to stable UUID-backed text ranges, including
  cross-block ranges.
- Deleted anchors degrade to the nearest useful context with warnings.
- Citations move as structured occurrences and merge with bibliography edits.
- Equations merge as atomic source objects.
- Tables use stable row/cell identities.
- Spreadsheet edits merge for cells, formulas, sheets, named ranges, filters,
  validations, protected ranges, and merged ranges.
- Malformed or duplicate remote operations never corrupt the source state.

Done when synthetic scenarios and deterministic fuzz tests for 1-3 replicas
converge byte-for-byte at canonical projection level, validate schema after
every operation, cover formatting/comments/suggestions/citations/equations/
tables/images/spreadsheets, and never require a user decision.

## Version Control And Storage

Use immutable content-addressed records with manifest commits. Local disk is
first-class first; S3/OpenDAL comes after the format and conformance tests are
stable.

Required records:

- Immutable content-addressed objects.
- Operation segments containing keypress-level or batched operations.
- Snapshots for faster open.
- Manifests with parent links, document UUID, branch/head metadata, snapshots,
  operation segments, blob references, lookup records, tombstones, provenance,
  warnings, and signatures.
- Branch heads with compare-and-swap where available.
- Deterministic candidate heads when compare-and-swap is unavailable or racing.
- UUID lookup records and optional DOI aliases.
- Pack files and pack indexes for local small-file mitigation.
- Tombstones describing tape/archive recall locations for recoverable data.

Required behavior:

- Local create, open, save, close, reopen, and autosave.
- Crash-safe object writes, head updates, and compaction.
- Candidate heads reconcile by fast-forward or operation-level merge.
- Raw bucket/repository scans rebuild lookup when no server exists.
- A server may accelerate lookup but must not be required for local mode.
- Missing shallow-clone blobs show placeholders and warnings.
- No hard deletion for now; deletion removes from current state while retained
  history keeps data for audit/recovery.

Done when local and S3/OpenDAL-shaped conformance tests cover save/open,
parallel saves, candidate reconciliation, lookup, shallow clone warnings, pack
compaction, missing blobs, tombstones, and crash recovery.

## Blobs, Shallow Clone, And Tape

Images and arbitrary binary objects are first-class repository objects.

Required:

- Content-addressed blobs by hash with algorithm agility.
- Blob metadata for media type, display name, dimensions where applicable, and
  document references.
- Exact-byte detached signatures keyed by blob hash.
- Sidecar signatures by default; embedded signatures only where the format makes
  that safe and useful.
- Shared blob reuse across documents and cheap shallow clones.
- Tape/archive tombstones surfaced in audit/recovery views.

Done when signed blobs verify independently, blob reuse avoids duplication,
documents with missing blobs open with warnings, and tombstone recall metadata is
available without changing document semantics.

## Signing And Audit

Unsigned documents open normally. Signatures are visual trust/audit indicators
for scientific fraud review, patent precedence, 21 CFR-style workflows, and
similar contexts.

Required:

- Rust-native OpenSSH-compatible signing by default.
- Optional `ssh-keygen` helper only if useful.
- Multiple signatures per version or object.
- Algorithm agility for hashes and signatures from the start.
- Manifest/version signatures over source state and retained reachable history.
- Exact-byte detached blob signatures.
- Typed semantic signatures independent of storage encoding, including image
  semantic profiles and FASTQ sequence-only/full-content profiles as design
  constraints.
- Trust states: unsigned, signed, trusted, untrusted, and broken.
- Audit/recovery views for signatures, warnings, deleted comments, resolved
  suggestions, deleted citations, operation history, and tombstones.

Do not source-sign rendered output, computed spreadsheet values, formula caches,
rendered citation labels, MathML generated from TeX, volatile UI state, import
provenance, or implementation-only invisible IDs. Formula signatures cover
formula source, not computed values.

Done when valid signatures verify after save/open, tampering is detected,
multiple signatures verify independently, unsigned documents remain openable,
and typed semantic signature APIs can sign storage-independent byte or semantic
profiles.

## Citations

Use OpenDoc citation nodes, not Paperpile-style link embedding.

Required:

- Document-local bibliography records so repeated citations update together.
- Structured citation occurrence labels and citation groups with locators,
  prefixes, suffixes, labels, and suppress-author flags.
- `citum` as the first citation rendering/modeling integration.
- Future CSL/CSL-JSON import/export as an adapter if useful.
- Paperpile Google Docs research only for import compatibility lessons.
- Rendered citation labels are projection/cache state and excluded from source
  signatures.

Done when references and occurrences save/open, merge safely, update together,
render through `citum`, and keep rendered labels outside source signatures.

## Spreadsheets

Spreadsheet v0 includes formula evaluation.

Required:

- Sparse multi-sheet workbook source model.
- Stable sheet, row, column, and cell identities.
- Typed values, formulas, formatting, comments, validations, named ranges,
  filters, frozen panes, protected-range warnings, and merged cells.
- Deterministic formula parser/evaluator.
- Dependency graph and invalidation, with dependency caches rebuilt lazily or in
  memory.
- Google Sheets API-shaped import/export.
- Graceful warnings for unsupported formulas or recoverable imports; abort
  imports that would silently misrepresent source content.

Done when formulas evaluate deterministically, cached computed values are
excluded from signatures, dependencies update after edits, imports follow
fixture expectations, and merge tests cover concurrent sheet/cell edits.

## Frontend And Runtime Modes

Use a shared app API across all shells.

Tauri local:

- First serious app shell.
- Hybrid TypeScript editor with Rust core.
- Local disk object repositories first, then S3/OpenDAL.
- Can use local OpenSSH-compatible keys for signing.

Browser local:

- Same source model and app API.
- Browser-suitable storage or remote service-backed storage.
- Browser signing postponed until key handling is clear.

HPC single-user web:

- Runs behind external authentication, such as an Open OnDemand-style setup.
- Server process can access disk and S3-like storage.
- Assumes the authenticated user has full access.
- Does not implement document-level permissions.

Multi-user service:

- Owns authentication, authorization, sharing, presence, lookup acceleration, and
  sync relay.
- May serialize commits server-side.
- Does not change document, operation, storage, or signing semantics.

Done when Tauri, browser tests, HPC wrapper, and service wrapper use the same
app API contract and repository fixtures.

## GUI Scope

The Tauri GUI must support every v0 schema feature through operations.

Required:

- Rich text editing with IME-safe input, selection mapping, paste handling,
  undo/redo, keyboard shortcuts, and immediate rendering.
- Controls for headings, lists, marks, links, comments, suggestions, citations,
  equations, tables, images, attachments, footnotes, warnings, signatures, and
  audit/recovery views.
- Spreadsheet editing for sheets, cells, formulas, formatting, comments, named
  ranges, validations, filters, frozen panes, protected warnings, and merged
  cells.
- Save, autosave, close, reopen, import, export, verify, and degraded-state
  placeholders.
- Native build checks including required Tauri assets such as icons.

Done when packaged Linux, macOS, and Windows builds can create, edit, save,
close, reopen, import, export, verify, and audit documents containing every v0
feature.

## Import And Export

Compatibility is proven through Google-shaped adapters, not full Word or
OpenOffice compatibility.

Required:

- Google Docs API-shaped import/export for the supported document subset.
- Google Sheets API-shaped import/export for the supported spreadsheet subset.
- `.doc`/`.docx` import where practical.
- Unsupported high-risk structures abort import.
- Recoverable unsupported structures produce deterministic warnings.
- Debug JSON export remains only for tests and inspection.

Done when fixtures produce deterministic source states, unsupported structures
have explicit expected behavior, and round-trip exports preserve the v0 subset.

## Verification Matrix

Required test families:

- Canonical binary round trips and deterministic encoding.
- Schema validation after every operation and import.
- Merge scenarios and fuzzing for formatted documents and spreadsheets.
- Save/open/reopen and crash recovery.
- Candidate-head reconciliation without a commit server.
- Pack compaction and local small-file mitigation.
- Shallow clone, missing blob placeholders, UUID/DOI lookup, and tombstones.
- Manifest/version signing, blob sidecars, typed semantic signatures, and
  tamper detection.
- Citation save/open/merge/rendering with `citum`.
- Spreadsheet formula evaluation and dependency invalidation.
- Google Docs/Sheets-shaped import/export fixtures.
- Tauri command contract, browser mock contract, GUI smoke, and native package
  checks.

Done when CI or documented local commands run all required families and failures
point to a specific product invariant.

## Milestones

1. Complete source schema and canonical binary records for documents,
   spreadsheets, citations, blobs, signatures, manifests, operation segments,
   lookup records, packs, and tombstones.
2. Complete operation-level merge proof with realistic scenarios and fuzzing.
3. Complete local repository with candidate-head reconciliation, shallow clone,
   pack compaction, signing, audit, and recovery metadata.
4. Complete the shared app API used by Rust, Tauri, browser mocks, HPC mode, and
   future service mode.
5. Complete the Tauri GUI for every v0 document and spreadsheet feature.
6. Complete Google Docs/Sheets-shaped import/export and practical `.doc/.docx`
   import.
7. Complete real or realistic S3/OpenDAL conformance.
8. Complete HPC single-user web wrapper.
9. Complete multi-user service with authentication, permissions, presence,
   sharing, and sync relay.
10. Complete Linux, macOS, and Windows package verification.

## Open Research Tasks

- Final merge architecture choice: existing CRDT, custom state machine, or
  hybrid stable-ID operation model.
- Exact binary encoding choice if deterministic CBOR becomes limiting.
- OpenDAL integration depth versus a small internal object-store trait.
- S3-compatible store behavior for compare-and-swap heads, listing latency, and
  candidate-head reconciliation.
- Best pack-signing boundary: logical objects, physical packs, or both.
- Browser key handling and browser-side signing.
- Common HPC tape/archive recall interfaces and tombstone schema details.
- Paperpile Google Docs metadata extraction behavior for import compatibility.
- Best TypeScript editor substrate for operation-first rich editing.
- Performance thresholds for loose local objects, packed local objects, S3-like
  stores, and common HPC filesystems.

## Immediate Next Work

1. Finish operation/API coverage for every v0 source feature.
2. Expand merge hardening for comments, suggestions, citations, equations,
   tables, formatting ranges, deleted anchors, images, and spreadsheets.
3. Add deterministic realistic merge fuzzing for 1-3 editor scenarios.
4. Harden local object storage, candidate-head reconciliation, and pack-file
   mitigation.
5. Complete manifest/version signing, blob sidecar signing, and typed semantic
   signing APIs.
6. Integrate `citum` through the document-local citation database.
7. Expand spreadsheet formula evaluation and dependency tests.
8. Add Google Docs/Sheets and `.doc/.docx` import fixtures.
9. Replace prototype Tauri controls with a schema-complete operation-backed
   editor.
10. Add S3/OpenDAL conformance only after local object semantics stabilize.
