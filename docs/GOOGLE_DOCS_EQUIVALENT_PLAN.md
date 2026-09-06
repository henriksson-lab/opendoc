# OpenDoc Google Docs Equivalent Plan

Status: canonical tracked plan, last consolidated 2026-08-30 after the
Docs/Sheets, Tauri, storage, signing, citation, HPC, and service-mode planning
discussion. This file is the one plan to track for the Google Docs/Sheets-
equivalent product. It supersedes the older overlapping plan files in `docs/`
and `forme.md` unless a later commit explicitly replaces it.

Filename: `docs/GOOGLE_DOCS_EQUIVALENT_PLAN.md`.

Tracking instruction: use this file, and this file only, as the product plan
until it is explicitly replaced. When implementation changes scope, update the
decision snapshot, the 50-line tracking index, the relevant workstream, and the
evidence log in this file.

Scope: includes the product, research, storage, merge, signing, citation,
spreadsheet, archive, Tauri/browser, HPC single-user, and multi-user service
decisions captured in the planning discussion through 2026-08-30.

## Plan Done Criteria

The planning deliverable is done when a reviewer can answer these questions
from this file without guessing:

1. What product are we building, and which Google Docs/Sheets-like features are
   in v0?
2. Which source schemas, binary records, operations, merge rules, repository
   objects, signatures, citations, spreadsheet features, and runtime modes are
   required?
3. Which choices are fixed for now, which are deferred, and which require
   research spikes or fuzz tests?
4. Which implementation workstreams remain, in what order, and what concrete
   tests, fixtures, contracts, smoke checks, or ADRs prove each one is done?
5. How do local disk, S3/OpenDAL, raw serverless object storage, Tauri,
   browser-local, HPC single-user, and multi-user service modes differ without
   changing document semantics?

If any answer is missing, the plan is not complete. If implementation proves a
decision wrong, update this file before treating the new behavior as accepted.

Planning done definition: the planning task is complete when this file has a
current decision snapshot, a 50-line tracking index, ordered workstreams,
per-workstream done criteria, and a proof requirement for every major system:
source schemas, binary format, operations, merge, storage, signing, citations,
spreadsheets, import/export, Tauri GUI, browser-local mode, HPC single-user
mode, multi-user service mode, audit/recovery, archive/tape recovery, and
performance. Implementation work is done only when the relevant workstream
names the exact tests, fixtures, smoke checks, contract checks, CI jobs, or ADRs
that prove the behavior.

## 50-Line Tracking Index

Use this section as the short progress view. The rest of the file is the
detailed contract and evidence log.

1. Source schema: Docs and Sheets v0 nodes must have stable IDs, validation,
   canonical binary encoding, warnings, and import/export fixtures.
2. Operations: every user-visible edit must be operation-backed; batching is
   storage-only and must not affect rendering or merge semantics.
3. Merge: 1-3 editors must converge automatically for text, formatting,
   comments, suggestions, citations, equations, tables, images, attachments,
   and spreadsheet edits.
4. Storage: local on-disk object repositories are first-class first; S3/OpenDAL
   follows after the same repository tests pass.
5. Lookup/archive: UUID lookup, optional DOI lookup, scan fallback, shallow
   clone, missing-blob warnings, pack compaction, and tape tombstones are all
   required storage semantics.
6. Signing: unsigned documents open normally; version, blob, and typed
   semantic signatures are independent trust indicators with algorithm agility.
7. Citations: use structured citation labels plus a document-local
   bibliography database; start with `citum`, add CSL/CSL-JSON as adapters.
8. Spreadsheets: include formula evaluation, source-only formula signatures,
   named ranges, comments, filters, warning-only protected ranges, validations,
   frozen panes, and merged ranges.
9. Frontend/runtime: build a Tauri v2 app with a minimal first page that lists
   openable documents and create/import actions before any blank document is
   opened; the temporary left command rail has been removed in favor of the
   proper app menu and active Docs/Sheets toolbar; share hybrid TypeScript
   editor logic across browser-local, HPC single-user web, and multi-user
   service modes.
10. Import/export: Google Docs/Sheets API-shaped import/export is the
    compatibility proof; practical `.doc`/`.docx` import is secondary.
11. Audit/recovery: deleted and degraded objects must be hidden in normal view
    but visible/restorable where data is retained.
12. Done means: each item has source schema, operation semantics, persistence,
    merge behavior, graceful degradation rules, and named tests or fixtures.

## Current Decision Snapshot

These are the design decisions this plan must preserve while implementation
continues:

- The first serious prototype optimizes for a robust Google Docs-style editing
  and collaboration model, with on-disk object storage first and S3/OpenDAL
  later.
- The source of truth is a Rust-owned operation/source model, not the DOM.
  TypeScript owns the editor surface, selection, IME, and Tauri/browser UI.
- Durable state uses a binary format. Deterministic CBOR is acceptable during
  research; JSON is limited to debug, fixtures, contracts, and import/export.
- Rendering is immediate. Keypress-level edits become operations immediately;
  batching, snapshots, operation segments, and pack files are storage
  optimizations only.
- Merge must be fully automatic for 1-3 active editors and about 5 viewers,
  including formatting, comments, suggestions, citations, equations, tables,
  images, attachments, and spreadsheet edits.
- Graceful degradation means the document remains openable and warnings or
  audit/recovery records explain degraded anchors, missing blobs, unsupported
  imports, broken signatures, or archived content.
- Citations are structured citation-label nodes backed by a document-local
  bibliography database. Use `citum` first; keep CSL/CSL-JSON as future
  import/export adapters.
- Spreadsheets v0 include formula evaluation, source-only formula signatures,
  named ranges, comments, filters, warning-only protected ranges, validations,
  frozen panes, merged ranges, and Google Sheets-shaped import/export.
- Blobs, images, attachments, and typed semantic profiles can be signed
  independently from document versions and independently from storage layout.
- UUID lookup, optional DOI aliases, shallow clone, reusable blob signatures,
  missing-blob placeholders, central lookup acceleration, scan fallback,
  archive tombstones, and tape recovery are repository semantics.
- Unsigned documents open normally. Signatures are trust/compliance indicators;
  browser signing is postponed until key handling is clearer.
- Tauri desktop, browser-local, HPC single-user web, raw object-store mode, and
  multi-user service mode must share source, operation, merge, and repository
  semantics while differing only in runtime capabilities.

## Product Finish Plan

This is the short plan for finishing the Google Docs-equivalent product:

1. Stabilize the v0 source and operation schema for docs, sheets, citations,
   equations, comments, suggestions, blobs, signatures, lookup, tombstones, and
   warnings.
2. Prove canonical binary persistence for all source objects, operation
   segments, snapshots, manifests, heads, packs, tombstones, and signature
   envelopes. JSON remains test/debug/import/export only.
3. Rework the Tauri/browser GUI into the first serious product surface: a
   minimal first page/home document picker must list openable documents and
   expose create-document, create-spreadsheet, open, and import actions, with
   no implicit blank document required when the GUI opens. After open/create/
   import, show a Google Docs-like document canvas and a Google Sheets-like
   workbook grid with dense toolbars, menus, title/status chrome, panels, and
   workflow affordances that are close enough to test product behavior instead
   of a prototype dashboard.
4. Finish the operation-first editing API so every visible GUI edit is captured
   as an immediate operation; batching and compaction are storage-only.
5. Resolve the merge design with realistic simulations and fuzz tests for 1-3
   editors, including text, formatting marks, comments, suggestions, citations,
   equations, tables, images, attachments, and spreadsheet edits.
6. Make local on-disk repositories production-shaped first: create/open/save,
   CAS heads, candidate reconciliation, UUID/DOI lookup, scan fallback, shallow
   clone, missing-blob warnings, pack compaction, and archive/tape tombstones.
7. Add S3/OpenDAL after the same repository conformance tests pass locally.
8. Complete signing in Rust for unsigned normal open, version signatures,
   exact-byte blob sidecars, typed semantic signatures, multiple signers,
   algorithm agility, trust states, and tamper warnings. Browser signing stays
   postponed.
9. Finish citations with structured citation-label nodes, a document-local
   bibliography database, `citum` rendering, update propagation, delete/restore
   behavior, and future CSL/CSL-JSON adapters.
10. Finish spreadsheets with formula evaluation, dependency invalidation,
   source-only formula signing, named ranges, comments, filters, warning-only
   protected ranges, validations, frozen panes, merged ranges, and
   Google Sheets-shaped import/export.
11. Add runtime wrappers: browser-local storage, HPC single-user disk/S3 access
    with external auth assumed, and multi-user service auth/permissions/sync
    with permissions kept outside signed document source.
12. Declare the product done only when each gate below names the exact test,
    fixture, smoke check, contract check, or ADR proving it.

Goal: build an open source Google Docs/Sheets-style product, written primarily in Rust, with local offline use, collaborative editing, object-store storage, optional cryptographic signing, citations, spreadsheets, and Tauri/browser frontends.

Owner-facing summary: track this file as the product plan. It includes the
research decisions, implementation sequence, runtime modes, schema scope,
storage/version-control strategy, signing model, citation plan, spreadsheet
scope, archive/tape recovery, frontend target, and completion gates discussed
so far.

This plan is done only when every workstream below has implementation, tests or fixtures, named verification commands, and documented graceful-degradation behavior where applicable. Research is not done until the conclusion is encoded as schema, operation semantics, binary format, tests, or an explicit deferral.

Tracking rule: each completed gate must be updated in this file with links to
the implementation evidence, fixtures, fuzz tests, conformance tests, or ADRs
that prove it. A workstream is not complete because a UI exposes it; it is
complete only when the source schema, operations, persistence, merge behavior,
and degradation semantics are covered where they apply.

## Planning Deliverable Checklist

This planning task is complete when this file contains all of the following:

- Product scope for Docs, Sheets, citations, equations, comments, suggestions,
  images, attachments, signing, audit/recovery, import/export, and runtime
  modes.
- Fixed architecture decisions for Rust crates, hybrid TypeScript frontend,
  Tauri desktop, browser reuse, HPC single-user web, and multi-user service.
- A version-control/storage plan covering operation logs, snapshots, manifests,
  branch heads, candidate heads, local disk first, raw S3/OpenDAL later, shallow
  clone, pack compaction, UUID/DOI lookup, scan fallback, central lookup
  acceleration, and tape/archive tombstones.
- A merge plan covering operation-level automatic merge, keypress-level edits,
  immediate rendering separate from batching, stable anchors, formatting marks,
  comments, suggestions, citations, equations, tables, spreadsheets,
  degradation warnings, realistic synthetic scenarios, and fuzz testing.
- A signing plan covering unsigned normal open, OpenSSH-compatible Rust-native
  identities, version signatures, exact-byte blob sidecars, typed semantic
  signatures, algorithm agility, browser signing deferral, and trust/audit
  projections.
- A citation plan covering structured citation labels, a document-local
  bibliography database, `citum` first, CSL/CSL-JSON as future adapters, and
  Paperpile-style Google Docs links as research input only.
- A spreadsheet plan covering formula evaluation, dependency invalidation,
  formula-source signatures, named ranges, comments, filters, warning-only
  protected ranges, validations, frozen panes, merged ranges, and Google
  Sheets-shaped import/export.
- A finishable execution order with explicit completion gates and named proof
  requirements for source, operations, merge, storage, signing, citations,
  spreadsheets, import/export, frontend, and runtime modes.

If a future discussion changes any of these points, update this checklist and
the matching workstream before implementing the change.

## How To Use This Plan

Treat this file as the single implementation tracker for the Google
Docs/Sheets-equivalent product. New design decisions from research or product
discussion should be added to the Decision Ledger, and implementation progress
should be added under the relevant workstream as current evidence.

Do not mark a gate complete until the gate names the exact test, fixture,
contract check, smoke test, ADR, or verification command that proves the
behavior. If a feature is intentionally deferred, record the deferral and the
reason in the relevant workstream so it is clear that the gap is known rather
than accidentally omitted.

## Completion Map

The product is finishable only if these gates are all green:

- Source model: every supported Docs/Sheets feature has stable IDs, operation
  semantics, binary encoding, validation, and Google-shaped import/export proof.
- Merge: realistic synthetic scenarios and fuzz tests for 1-3 editors converge
  automatically, including formatting, comments, suggestions, citations,
  equations, tables, and spreadsheet edits.
- Storage: local on-disk repositories create, save, reopen, reconcile candidate
  heads, compact packs, handle missing blobs, and expose scan-based UUID/DOI
  lookup before S3/OpenDAL becomes default.
- Signing: unsigned documents open normally; signed versions, blobs, and typed
  semantic profiles verify independently; tampering degrades to warnings or
  broken trust states without making recoverable documents unopenable.
- Runtime modes: Tauri local, browser local, HPC single-user web, and multi-user
  service mode all use the same app API while differing only in storage,
  authentication, permissions, signing availability, and sync relay policy.
- GUI: the Tauri app exposes a real Docs/Sheets-like editing surface for the
  full v0 schema, with immediate rendering separate from persistence batching.
- Audit/recovery: warnings, signatures, missing blobs, tombstones, deleted
  comments, suggestions, citation records, and operation history are visible in
  dedicated audit/recovery views.
- Verification: each completed feature has named Rust, TypeScript, contract,
  import/export, fuzz, and packaging checks documented in this file.

## Design Discussion Coverage Checklist

This plan includes the design constraints from the research discussion:

- Google Docs/Sheets equivalent in Rust, with a hybrid TypeScript frontend and
  Tauri as the first serious product shell.
- Local on-disk storage first, then S3/OpenDAL-compatible object stores, raw
  single-user object-store mode, HPC single-user web mode, and multi-user
  service mode.
- Fully automatic collaborative merge for 1-3 active editors and about 5
  viewers, with viewers treated as possible editors.
- Immediate rendering separated from persistence batching, so keypress-level
  operations can render locally while operation segments, snapshots, and packs
  optimize storage later.
- Binary durable format, currently deterministic CBOR during research; JSON is
  limited to debug, tests, contracts, and import/export adapters.
- Formatting, comments, suggestions, citations, equations, tables,
  spreadsheets, images, attachments, deleted anchors, and warning states are
  part of operation and merge semantics.
- Content-addressed manifests, blobs, operation segments, candidate heads,
  shallow clones, UUID/DOI lookup, scan fallback, central lookup acceleration,
  pack compaction, and tape/archive tombstones.
- Optional signing with unsigned documents opening normally, Rust-native
  OpenSSH-compatible identities, version signatures, exact-byte blob
  signatures, typed semantic signatures, sidecar signatures, algorithm agility,
  and trust/audit views.
- Storage-independent signing boundaries for typed profiles such as image
  semantics and FASTQ sequence-only/full-content profiles.
- Document-local bibliography database, structured citation labels, `citum`
  first, and CSL/CSL-JSON left as future adapters.
- Spreadsheet v0 with formula evaluation, dependency invalidation, named
  ranges, comments, filters, warning-only protected ranges, validations, frozen
  panes, merged ranges, and Google Sheets-shaped import/export.
- Google Docs/Sheets API-shaped import/export as the compatibility proof, with
  practical `.doc`/`.docx` import where installable tooling permits it.
- Browser signing postponed; permissions kept outside signed document source
  and enforced only by runtime/service modes that can support them.
- Audit/recovery views for warnings, signatures, missing blobs, tombstones,
  deleted comments, deleted citations, suggestions, operation history, and
  degraded imports.

## Work Needed To Finish The Product

Finish work in this order unless evidence from tests changes the order:

1. Prove the v0 source and binary format: every Docs/Sheets feature listed in
   this plan has canonical binary records, validation, deterministic warnings,
   and save/open round-trip tests.
2. Prove the operation model: every visible edit is represented as an operation,
   applies immediately to the local projection, and can later be batched into
   operation segments without changing merge semantics.
3. Prove automatic merge: realistic synthetic scenarios and fuzz tests for 1-3
   editors converge for text, formatting, comments, suggestions, citations,
   equations, tables, images, attachments, and spreadsheet edits.
4. Prove local storage: on-disk repositories handle manifests, heads, candidate
   heads, snapshots, operation segments, blob reuse, lookup by UUID/DOI, scan
   fallback, shallow clones, packs, compaction, and archive tombstones.
5. Prove signing and audit: unsigned documents open normally, signed versions
   and blobs verify independently, typed semantic signatures work without
   depending on storage layout, and all broken or missing trust data degrades to
   visible audit warnings.
6. Prove citations and spreadsheets: document-local bibliography records,
   `citum` rendering, citation labels, formula evaluation, dependency
   invalidation, named ranges, filters, protections, validations, frozen panes,
   comments, and merged ranges all survive save/open and merge.
7. Prove import/export: Google Docs/Sheets-shaped fixtures and practical
   `.doc`/`.docx` fixtures either round-trip the supported subset, warn
   deterministically, or abort safely before partial state replacement.
8. Prove the app: the Tauri GUI creates, edits, saves, closes, reopens,
   imports, exports, verifies, and audits documents containing every v0 schema
   feature, while the browser command contract exercises the same app API.
9. Prove deployment modes: browser-local, HPC single-user, raw object-store, and
   multi-user service modes share the same source, operation, and storage
   semantics; only authentication, permissions, lookup acceleration, signing
   availability, and sync relay policy differ.

The product is not finished until each item above names the exact test,
fixture, smoke test, CI job, or ADR proving completion.

## Current Finish Plan

This is the implementation sequence to finish the Google Docs/Sheets equivalent
from the current prototype state. A step is complete only when the named proof
exists in this file and can be rerun.

1. Close source/schema gaps: audit every v0 document, citation, spreadsheet,
   blob, signature, tombstone, lookup, pack, manifest, and operation record;
   add canonical binary round-trip, validation, warning, and import/export
   fixtures for any missing state.
2. Close operation gaps: ensure every app-visible edit, including comments,
   suggestions, citation edits, equations, tables, images, attachments,
   spreadsheet structure, formula edits, audit restore actions, signing, and
   archive metadata, appends a durable operation or an explicit warning-only
   no-op.
3. Finish merge proof: extend realistic scenarios and fuzzing until 1-3 editors
   converge for keypress-level text, formatting, paragraph split/join, moved or
   deleted anchors, comments, suggestions, citation occurrences, bibliography
   edits, equations, tables, images, and spreadsheet edits.
4. Harden local storage: make on-disk repositories the default path for create,
   save, autosave, close, reopen, candidate reconciliation, UUID/DOI lookup,
   scan fallback, shallow clone, missing blobs, pack compaction, archive
   tombstones, and no-hard-delete recovery.
5. Prove signing boundaries: keep unsigned documents normal, verify
   OpenSSH-compatible Rust-native identities, support multiple version
   signatures, exact-byte blob sidecars, typed semantic profiles, algorithm
   agility, tamper detection, and trust/audit projections. Keep browser signing
   deferred.
6. Finish citation integration: keep citation labels as structured source nodes
   backed by a document-local bibliography database, render with `citum`, merge
   bibliography and occurrence edits, and leave CSL/CSL-JSON as future adapters.
7. Finish spreadsheet v0: formula evaluation, dependency invalidation, source
   signature boundaries, named ranges, comments, filters, warning-only
   protected ranges, validations, frozen panes, merged ranges, and Google
   Sheets-shaped import/export must all have source, operation, merge, and
   persistence proof.
8. Prove import/export: Google Docs/Sheets-shaped fixtures are the compatibility
   proof; practical `.doc`/`.docx` import is supported where easy tooling or
   Rust-native parsing permits it; recoverable gaps warn and high-risk imports
   abort without partial state replacement.
9. Finish the Tauri product shell: the GUI must expose the full v0 schema with
   immediate rendering, operation-backed undo/redo, save/autosave/open, import,
   export, signing verification, warnings, and audit/recovery. Linux, macOS,
   and Windows native build prerequisites and assets must be checked.
10. Finish runtime modes: browser-local, HPC single-user web, raw object-store,
    and multi-user service modes must share source, operation, merge, and
    repository semantics. Runtime differences are limited to storage access,
    authentication, permissions, signing availability, lookup acceleration,
    sharing, presence, sync relay, and optional service-side commit
    coordination.
11. Add S3/OpenDAL conformance after local semantics are stable: run the same
    repository tests against realistic S3/OpenDAL stores, including candidate
    heads, scan fallback, shallow clone, packs, tombstones, and signature
    sidecars.
12. Keep performance measurable: track operation replay cost, snapshot open
    cost, pack compaction behavior, local small-file pressure, object-store
    round trips, and merge/fuzz throughput. Optimize only after correctness
    gates have named baselines.

## Product Contract

OpenDoc must support:

- Rich documents with paragraphs, headings, lists, marks, links, comments, suggestions, citations, equations, tables, footnotes, images, and arbitrary binary attachments.
- Spreadsheets with sparse multi-sheet workbooks, formatting, formulas, dependency invalidation, named ranges, comments, filters, warning-only protected ranges, validations, frozen panes, and merged ranges.
- Local on-disk repositories as the first-class default.
- S3/OpenDAL-shaped object storage later, including raw single-user object-store mode without a commit server.
- Collaborative editing for 1-3 active editors and about 5 viewers, treating viewers as potential editors.
- Fully automatic merge that always produces an openable document.
- Tauri v2 desktop app for Linux, macOS, and Windows.
- Browser mode using the same app API and source semantics.
- HPC single-user web mode behind external authentication, with access to disk and S3-like storage and no document-level permission checks.
- Multi-user service mode with authentication, permissions, sharing, presence, lookup, and sync relay.
- Google Docs/Sheets API-shaped import/export as the compatibility proof.
- Practical `.doc`/`.docx` import where installable tooling permits it.

## Non-Negotiable Decisions

- Rust owns source schemas, operation semantics, merge, binary format, storage, signing, spreadsheet logic, citation logic, import/export, and verification.
- TypeScript owns the rich editor surface, DOM integration, selection, IME, keyboard behavior, and Tauri/browser GUI.
- The DOM is a projection, not the source of truth.
- Rendering must not wait for commit batching.
- Every keypress-level edit must be representable as an operation.
- Batching, operation segments, snapshots, and packs are persistence optimizations only.
- The durable format is binary, currently deterministic CBOR during research. JSON is allowed only for debug views, tests, command contracts, and import/export adapters.
- Backward compatibility is not required until the project leaves research mode.
- Permissions are not part of document source state; they belong to repository-opening or service-layer capabilities.
- Unsigned documents open normally. Signatures are trust/compliance indicators.
- Browser signing is postponed until key handling is clearer.
- Rendered output, computed spreadsheet values, citation render caches, equation render caches, volatile UI state, import provenance, and implementation-only helper IDs are excluded from source signatures.
- Deletion removes objects from current state while retained history and audit/recovery data may keep them. Do not hard-delete for now.

## Decision Ledger

These decisions come from the product discussion and are binding until this file
is edited again:

- First serious prototype optimizes for a robust Google Docs-style editing system with collaboration semantics and a credible Google Docs/Sheets-like GUI. It does not need pixel-perfect cloning, but it must present the same basic product shape: document canvas, spreadsheet grid, dense toolbar/menu chrome, title/status area, comments/suggestions/citations/equations/images/tables, and save/share/verify/audit workflows.
- First-class storage is local on-disk objects. S3/OpenDAL becomes default later, after the same tests pass against local repositories.
- API/test work remains required, but the first product proof is now GUI-first enough that users can exercise Docs-like and Sheets-like workflows directly. Headless tests prove semantics; the GUI proves that the model can support the product we intend to build.
- The app target is Tauri plus a hybrid TypeScript frontend, while keeping source logic reusable by browser and service runtimes.
- Browser/server variants are separate deployment modes: browser-local, HPC single-user web behind external authentication, and continuously running multi-user service.
- HPC single-user mode can access disk and S3-like storage and assumes the authenticated user has full access. It must not require document-level permissions.
- Multi-user service mode owns authentication, permissions, sharing, presence, lookup acceleration, and sync relay. Permissions are runtime/service state, not signed source state.
- Unsigned documents open normally. Signatures are visual trust/compliance indicators for cases such as 21 CFR, scientific fraud review, and patent precedence.
- Signer identities should reuse OpenSSH-compatible concepts in Rust. Do not rely on OpenSSH as a trusted authority.
- Browser signing is explicitly postponed.
- Every edit must be representable at operation level, including keypress-level text insertion. Persistence batching is allowed, but rendering must be immediate.
- All merges must be automatic and must always produce an openable document. Degraded results carry deterministic warnings.
- Merge research must compare existing CRDT approaches with a custom operation/state-machine model using realistic synthetic scenarios and fuzz testing.
- DOM structure is not assumed to be a good merge source. The source model uses stable block/object identities and operation semantics; the DOM is projection.
- Formatting applies immediately and must merge with inserts, deletes, paragraph splits/joins, comments, suggestions, and citations.
- Comments and suggestions are essential v0 features. Comments are signed source state and deleted comments are visible only in audit/recovery views unless restored.
- Suggestions should behave roughly like Google Docs track changes; no special `@user` semantics are required beyond syntax highlighting if useful.
- Citations use a special citation label node, not ordinary links. Paperpile-style embedded Google Docs links are research input only.
- Citation data belongs in a document-local bibliography database so repeated citations update together and merge with document history.
- Use `citum` first for citation rendering/source integration. CSL/CSL-JSON remains a future import/export adapter.
- Equations are TeX/LaTeX source. Rendering, MathML, PDF output, and computed displays are projections and are excluded from signatures.
- Spreadsheet v0 includes formula evaluation. Formula source is signed; computed values are cache/projection state and may be recomputed lazily.
- Spreadsheet v0 must include comments, named ranges, basic filters with criteria/sort specs, warning-only protected ranges, validations, frozen panes, merged ranges, and Google Sheets-shaped import/export.
- Google Docs/Sheets API-shaped import/export is the compatibility proof. Practical `.doc`/`.docx` import is prioritized when tooling is easy to install.
- Unsupported recoverable imports open with warnings. Unsupported high-risk imports abort deterministically.
- Debug JSON is allowed for tests and inspection. Durable storage is binary, currently deterministic CBOR during research.
- Backward compatibility is not required until the project leaves research mode.
- Repository storage must support content-addressed blobs, shallow cloning, missing-blob placeholders, reusable blob signatures, UUID lookup, optional DOI aliases, scan fallback, and central lookup acceleration when a server exists.
- Binary objects can be signed independently of storage and format. Exact-byte blob signatures are keyed by hash; typed semantic signatures support profiles such as image semantics and FASTQ sequence-only/full-content.
- Blob signatures may be sidecars by default. Embedded signatures are allowed only for specific formats where that reduces object count without weakening format-independent signing.
- Tape/archive support uses tombstone metadata for recoverable data. A central lookup can accelerate discovery, but no-server mode must still work by scanning one bucket/repository.
- Local filesystems dislike many small files. Pack files and crash-safe compaction are required, but users should not need to know about compaction.
- Do not hard-delete data during the research/prototype period. Users delete from current state; retained history and audit/recovery may keep data.

## Architecture

Core crates:

- `opendoc-core`: document, spreadsheet, comments, suggestions, citations, equations, IDs, validation.
- `opendoc-merge`: operation model, automatic merge, deterministic degradation, fuzz scenarios.
- `opendoc-format`: canonical binary encoding for source, snapshots, operation segments, manifests, lookup records, tombstones, packs, and signatures.
- `opendoc-store`: local disk object store first; S3/OpenDAL-compatible semantics later; heads, candidate heads, packs, lookup, tombstones, shallow clone.
- `opendoc-sign`: OpenSSH-compatible identities, manifest/version signatures, blob sidecars, semantic profile signatures.
- `opendoc-citations`: document-local bibliography model, citation labels, `citum` integration, future CSL adapters.
- `opendoc-import`: Google Docs/Sheets-shaped adapters and practical `.doc`/`.docx` adapters.
- `opendoc-app-api`: stable UI-facing API shared by Tauri, browser, tests, HPC wrapper, and service wrapper.

Applications:

- Tauri desktop app.
- Browser local app.
- HPC single-user web wrapper.
- Multi-user collaboration service.

## Source Schema Workstream

Define one canonical source model:

- Document UUID, optional DOI, title, locale, metadata, provenance, warnings, audit/recovery records.
- Stable block IDs and object IDs for merge anchoring.
- Paragraphs, headings, lists, page breaks, tables, images, equations, footnotes, attachments, comments, suggestions, citations, and links.
- Formatting as first-class marks and ranges, not fragile DOM spans.
- Citations as structured citation labels backed by document-local bibliography records.
- Equations as TeX/LaTeX source; MathML/PDF/rendered output is projection only.
- Spreadsheets with stable sheets, axes, cells, formulas, dependencies, ranges, metadata, and formatting.

Done when every v0 source node round-trips through canonical binary encoding and invalid source states are rejected or repaired with deterministic warnings.

Current implementation evidence: `opendoc-app-api` has a full app snapshot
canonical CBOR round-trip test covering representative rich document blocks,
inline marks, footnotes, comments, suggestions, citations, equations, image
blob refs, typed blob signatures, spreadsheet formulas, comments, validations,
filters, frozen panes, merged ranges, protected ranges, named ranges, and
warnings. The test verifies deterministic bytes, decodes back to the same
`SnapshotRecord<AppDocument>`, validates the combined app-level document,
workbook, blob metadata, exact-byte signature, typed semantic signature, and
archive tombstone source, converts back to core source, and validates the
decoded source model. The focused tests `app_projection_rejects_invalid_structured_payloads`
and `spreadsheet_source_validation_rejects_invalid_structural_payloads`
prove app source validation rejects malformed rich-document payloads, duplicate
or mismatched blob/signature metadata, whitespace-padded blob IDs, blob names,
and media types, path-hostile blob hash text, whitespace-padded typed semantic
signature profile and field names, named ranges pointing at missing sheets,
duplicate cells, overlapping merged ranges, and corrupt row/column axis metadata
before export. `save_rejects_structurally_invalid_app_snapshot_source`
and `open_rejects_structurally_invalid_app_snapshot_source` prove combined app
source validation runs before durable repository writes and after binary
snapshot decode. The focused core test
`comment_and_suggestion_anchors_require_auditable_payloads` proves retained
comment and suggestion anchors reject empty text-range IDs and empty or
whitespace-padded degraded nearest-block warning payloads while still allowing
deleted-anchor recovery states to remain in source. Core/app warning records
also reject empty or whitespace-padded warning codes/messages before trusted
source snapshots are accepted, covered by `warning_records_require_auditable_payloads`,
`app_projection_rejects_invalid_structured_payloads`, and the browser mock
contract warning validator. Document title, locale, DOI, spreadsheet workbook
title/locale/timezone, spreadsheet sheet IDs, spreadsheet sheet titles, and
content-addressed hash references now reject whitespace-padded values in
core/app source validation, app operation envelope validation, and the browser
mock projection contract; covered by
`minimal_document_is_valid`,
`app_projection_rejects_invalid_structured_payloads`,
`document_title_updates_converge_and_reject_empty_titles`,
`save_rejects_padded_document_title_operation_before_writing_objects`,
`save_rejects_padded_document_locale_operation_before_writing_objects`,
`save_rejects_padded_document_doi_operation_before_writing_objects`,
`save_rejects_padded_spreadsheet_metadata_operations_before_writing_objects`,
and `mock-contract`. App-visible `nearest:` anchor labels parse back to
auditable nearest-block source anchors with non-empty warnings, covered by
`nearest_anchor_labels_parse_to_auditable_source_anchors` and the broader
`dispatches_desktop_command_contract`. The browser mock Google Docs-shaped
import/export contract mirrors this boundary by exporting `nearestBlock`
anchors with a deterministic non-empty warning payload after wrapper-whitespace
canonicalization. Browser projection validation now also mirrors Rust named
range source validation for canonical names, canonical sheet references,
existing target sheets, and canonical A1 ranges, covered by `mock-contract`.
Browser projection validation now also mirrors Rust numeric cell source and
computed-projection parsing by rejecting empty or whitespace-padded number
strings instead of accepting JavaScript coercions; covered by
`spreadsheet_source_validation_rejects_invalid_structural_payloads` and
`mock-contract`.
The focused core test
`source_model_rejects_empty_stable_ids_after_decode` proves decoded/imported
source cannot retain empty stable IDs for blocks, inlines, table axes,
equations, bibliography references, citation items, or footnote citation
placements. The focused core test
`stable_id_parse_rejects_surrounding_whitespace` and the expanded
`source_model_rejects_empty_stable_ids_after_decode` test prove source-level
stable IDs must also be canonical without wrapper whitespace, while app command
input parsing trims wrapper whitespace before creating source operation IDs so
forgiving UI/import inputs cannot create distinct durable merge anchors. The
browser command contract now sends padded table block, row, and cell IDs through
row/cell insert and delete operations, and browser-local command handling trims
them before lookup to match Rust stable-ID parsing.
The same browser contract now sends padded block and inline IDs through heading
updates, list updates, paragraph insertion, block deletion, inline text edits,
inline deletion, mention/link/equation updates, mark add/remove, and image
metadata/blob replacement; browser-local command handling trims those targets
before lookup and operation projection. Rust and browser-local mention creation
and update commands trim mention labels before storing structured source text,
and trusted app/browser source projections reject whitespace-padded mention
labels; covered by `mention_label_update_is_operation_backed_and_persists`,
`app_projection_rejects_invalid_structured_payloads`,
`save_rejects_padded_structured_inline_operation_fields_before_writing_objects`,
and the browser command
sweep. Link creation and href-update commands trim link targets before storing
source metadata, and trusted app/browser projections reject whitespace-padded
link hrefs; covered by `update_link_href_is_operation_backed_and_persists`,
`add_link_rejects_empty_text_or_href`,
`app_projection_rejects_invalid_structured_payloads`,
`save_rejects_padded_structured_inline_operation_fields_before_writing_objects`,
and the browser command
sweep. Inline and block equation creation/update commands trim wrapper
whitespace around TeX/LaTeX source before storing structured source, while
trusted app/browser source projections reject padded equation source; covered
by `source_model_rejects_empty_stable_ids_after_decode`,
`block_level_schema_nodes_are_projected_and_persisted`,
`inline_equation_source_update_is_operation_backed_and_persists`,
`app_projection_rejects_invalid_structured_payloads`,
`save_rejects_padded_equation_operation_source_before_writing_objects`, and the
browser command sweep.
The browser mock Google Docs extension import now mirrors Rust app validation
for typed semantic signature profile names and included/excluded field names:
whitespace-padded values abort import without replacing the current source
projection. The same contract also rejects path-hostile content hashes in
`opendocBlobs` metadata and image-extension blob references before they enter
browser-local source state while accepting non-`sha256` algorithm-agile,
path-safe content hashes for shallow imported blob refs, covered by
`npm --prefix apps/desktop run mock-contract`.
The
focused core test `document_uuid_parse_rejects_surrounding_whitespace` and the
expanded decoded-source validation test prove document UUIDs obey the same
canonical no-wrapper-whitespace rule and cannot be empty in trusted source
snapshots; repository-facing app paths continue trimming user-supplied UUID
lookups before resolving heads. The format test
`decoded_repository_records_validate_semantic_required_fields` proves the same
canonical document UUID rule is enforced on durable binary manifests,
snapshots, operation segments, branch heads, and lookup records before those
records are trusted. The format test
`snapshot_and_operation_segments_have_native_binary_envelopes` now proves
snapshot and operation-segment envelopes also have deterministic native binary
`ODF0` records carrying raw source/operation bytes, while
`operation_segment_binary_envelope_rejects_oversized_operation_vectors` keeps
operation vector lengths bounded before allocation. The app repository save/open
path now writes those native binary envelopes for snapshots and operation
segments, with a decode fallback for legacy canonical-CBOR fixtures; verified by
`saves_and_opens_projection_through_local_object_repository` and the full
`cargo test -p opendoc-app-api` suite. Snapshot source-format
identifiers must also be
non-empty and free of wrapper whitespace before app-specific decoding trusts
the source payload; `snapshot_and_operation_segment_records_validate_required_envelope_fields`
and `local_repository_rejects_semantically_invalid_snapshot_record` cover this
durable-format and repository-open boundary. The same format test also proves
manifest, operation segment, branch-head, and lookup branch names must be valid
repository key segments before durable records are accepted, keeping branch
identity semantics aligned with local, flat, and S3-shaped head paths. Manifest
operation-segment, signature, and blob reference lists must be unique before a manifest is trusted;
`decoded_repository_records_validate_semantic_required_fields` and
`repository_rejects_semantically_invalid_binary_records` cover write-time and
read-time rejection of duplicate durable manifest references. Repository open
also verifies that a snapshot envelope UUID and its contained app-source UUID
match the manifest document UUID before source state, signatures, or operation
history are trusted; `local_repository_rejects_snapshot_manifest_identity_mismatch`
proves both mismatch cases fail deterministically. The
core test `warning_records_require_auditable_payloads` plus
the app projection test `app_projection_rejects_invalid_structured_payloads`
prove retained warning records must carry non-empty codes and messages before
source snapshots can be treated as valid. The same app projection test now
proves native app-source suggestion provenance entries are non-empty and
canonical before signed/audited source snapshots are accepted, matching the
Google-shaped import and browser mock contracts for suggestion review
metadata. It also proves native app-source comment and suggestion anchor labels
must use an emitted, auditable label shape (`document`, `nearest:<id>`, or
`<start>..<end>`) instead of silently degrading malformed labels to document
anchors during trusted source loading; the browser mock contract mirrors this
native source boundary and proves Google-shaped text-range comment anchors
import into the strict native `<start>..<end>` label before exporting back to
Google-shaped ranges. The format test
`binary_record_decode_rejects_unbounded_vector_lengths_before_allocation`
proves binary manifest, lookup, and pack-index records reject hostile vector
lengths before allocating large buffers.

## Operation And Merge Workstream

This is the highest-risk area.

Research and prototype both existing CRDT approaches and a custom operation/state-machine model. Use realistic synthetic scenarios and fuzz testing to choose the design.

Required semantics:

- Operation-level merge only.
- Keypress-level typing is representable, but storage may pack operations later.
- Concurrent typing in the same paragraph converges.
- Formatting ranges survive insertions, deletions, paragraph splits, and paragraph joins where possible.
- Delete-versus-format, delete-versus-comment, and deleted-anchor cases degrade deterministically.
- Comments and suggestions anchor to UUID-backed ranges and can move to nearest useful anchors or become hidden/restorable with warnings.
- Suggestions support insert, delete, format, accept, reject, and provenance.
- Citations move as structured occurrences, not links.
- Equations merge as atomic source objects.
- Tables use stable row/column/cell identities.
- Spreadsheet edits merge at operation level.
- Passive viewers do not have separate semantics from editors.

Done when 1-3 active-editor fuzz tests and realistic merge scenarios converge byte-for-byte at canonical projection level, validate schema after every operation, and never require manual conflict resolution.

Current implementation evidence: `opendoc-merge` has deterministic merge tests
for mixed rich-document streams and multi-replica pseudo-fuzz replay. The named
tests `shuffled_mixed_rich_document_streams_converge_with_warnings`,
`shuffled_structured_document_batches_converge_with_citations_and_tables`, and
`deterministic_multi_replica_pseudo_fuzz_converges` cover operation-order
convergence across concurrent actors, stable block/inline IDs instead of DOM
spans, delete-versus-format/comment/suggestion degradation, citation cache
repair, tables, equations, and schema validation after merge. The focused test
`image_delete_beats_stale_metadata_updates_without_resurrection` proves an
image block deleted by one actor is not resurrected by concurrent stale alt-text
or blob-hash updates from other actors; the stale updates degrade to
deterministic `missing-block` warnings while the merged document remains valid.
The focused test
`table_block_delete_beats_stale_row_and_cell_edits_without_resurrection` proves
the same delete-wins invariant for table blocks: stale concurrent row insertion
and cell deletion operations do not recreate a deleted table and instead
produce deterministic missing-table-block warnings. The focused test
`table_row_delete_beats_stale_nested_text_and_mark_edits_without_resurrection`
proves row-level deletion also wins over stale concurrent text and formatting
edits inside deleted table cells: the surviving row remains valid, stale nested
text updates degrade to `missing-inline`, stale formatting ranges degrade to
`missing-text-range`, and operation batch order still converges. The focused test
`structured_inline_delete_beats_stale_source_updates_without_resurrection`
extends that invariant to structured inline objects: link, mention, and inline
equation deletions win over stale concurrent source updates, emit deterministic
`missing-inline` warnings, and leave the document valid without resurrecting
deleted inline state. The app-api repository regression
`*_repository_merge_keeps_structured_inline_deletes_over_stale_updates` carries
the same case through local, flat, and OpenDAL-gated candidate merge storage:
delete and stale update operation envelopes are retained for audit, while the
reopened current projection keeps the deleted structured inlines absent with
persisted `missing-inline` warnings. The focused test
`paragraph_split_move_preserves_suggestion_range_anchors` proves concurrent
paragraph splitting/moving preserves insert, delete, and format suggestion
anchors by invisible inline UUID rather than DOM span position, and converges
without warnings across operation order while keeping the source document valid.
The focused test
`three_actor_typing_formatting_and_split_converge_without_batching` proves a
more realistic 1-3 editor workflow: one actor inserts text inside a formatted
range, another actor splits the paragraph by moving the trailing inline to a new
block, and a reviewer applies range formatting plus a comment. Separate actor
batches, a single unbatched operation stream, and reversed batch order converge
to the same valid document, proving rendering/storage batching is not part of
the merge semantics for that formatted editing path.
The focused test
`citation_group_delete_wins_over_older_stale_upsert_by_revision` proves a newer
citation-group delete wins over an older stale group update, clears stale
rendered citation caches, leaves inline citation labels as degraded
placeholders, and emits deterministic `citation-group-missing` warnings without
manual conflict resolution. The focused test
`concurrent_style_change_and_reference_delete_converge_for_table_citations`
proves a bibliography-reference delete and concurrent citation-style change
converge even when the rendered citation label is nested inside a table cell:
the reference remains deleted, document and inline rendered caches are cleared,
the citation degrades to a placeholder, and deterministic
`citation-reference-missing` warnings are emitted. The focused test
`malformed_operation_ids_are_ignored_with_deterministic_warnings` proves merge
streams ignore empty-actor, whitespace-padded-actor, and zero-sequence
operation IDs with sorted, deduplicated `invalid-operation-id` warnings across
stream order instead of letting malformed remote operations affect source
state. The focused test
`malformed_suggestion_resolution_reviewer_degrades_to_valid_provenance` proves
accept/reject suggestion operations canonicalize reviewer provenance and turn
empty reviewer IDs into deterministic warnings plus valid `unknown` provenance,
so malformed review operations cannot make merged source invalid. The focused
test `structured_source_updates_are_canonicalized_during_merge` proves merge
replay trims link hrefs, mention labels, inline equation source, and block
equation source before storing signed source state. The focused test
`padded_image_blob_hash_update_degrades_without_changing_source` proves padded
content-addressed image blob references degrade to deterministic warnings
without mutating valid source, and
`image_blob_hash_update_stores_canonical_hash_reference` proves valid image
blob updates store the canonical content-addressed string used by repository
lookup and blob-signature sidecars. The focused test
`concurrent_suggestion_resolution_beats_stale_content_update` proves a
suggestion accept/reject operation from one actor wins over another actor's
stale insert-content update, keeps the resolved suggestion source stable, emits
a deterministic `stale-suggestion-update` warning, and converges across stream
and storage-batch order. Suggestion resolution is now delayed until after
proposal/update replay at merge time:
`concurrent_suggestion_add_and_accept_converge_when_accept_sorts_first` proves
a concurrent accept still resolves a concurrently added suggestion even when
the accept actor sorts before the add actor in repository replay.
Bibliography reference merge now has the same delete-vs-stale-update proof as
citation groups:
`bibliography_reference_delete_wins_over_older_stale_upsert_by_revision` proves
a higher-revision reference delete keeps the reference hidden, clears dependent
citation render caches, leaves inline citation labels as degraded placeholders,
and converges independent of whether the stale lower-revision reference update
or delete replays first. The focused test
`concurrent_format_citation_comment_and_delete_converge_with_degraded_anchors`
proves three editor streams converge when one actor deletes a text anchor,
another inserts a structured citation plus document-local bibliography record,
and a third applies formatting plus a comment to the original range; the final
document keeps the citation occurrence, renders from the bibliography database,
collapses formatting to surviving editable text, collapses the comment anchor
to the surviving text endpoint before falling back to block-level anchoring,
emits deterministic warnings, and validates without manual conflict
resolution. The focused test
`comment_range_collapses_to_surviving_endpoint_when_partially_deleted` covers
the same partial-delete anchor repair directly, while
`cargo test -p opendoc-merge converge` verifies the broader mixed convergence
suite. Paragraph split/merge research is represented without DOM merging by
`OperationKind::MoveInlineToBlock`: the focused test
`paragraph_split_move_preserves_format_and_comment_range_anchors` proves a
split can move an existing inline UUID into a new paragraph while concurrent
formatting and comment ranges still span the resulting paragraphs and converge
without warnings. The focused tests
`move_inline_to_missing_block_keeps_source_and_warns`,
`move_missing_inline_to_block_warns_without_mutating_target`, and
`move_inline_to_block_with_missing_anchor_appends_and_warns` prove the same
operation degrades automatically when concurrent history removes its source
inline, destination block, or destination anchor. The operation is now exposed
through the app-facing `split_paragraph_at_inline` command, covered by
`split_paragraph_at_inline_moves_stable_inline_id_and_persists`, the desktop
command contract, the browser mock contract, and the GUI smoke workflow.
Paragraph join uses the same operation primitive in reverse through
`join_paragraph_with_previous`, covered by
`paragraph_join_preserves_format_and_comment_range_anchors` and
`join_paragraph_with_previous_moves_inlines_and_persists`; it moves stable
inline UUIDs into the previous paragraph before deleting the source paragraph,
so concurrent formatting and comment ranges survive automatic merge and
repository replay. The desktop GUI smoke now clicks both split and join toolbar
controls and asserts the resulting operation summaries are visible.
Comment-thread deletion now wins over stale concurrent reply and body-update
operations from other actors without reopening hidden review state:
`comment_thread_delete_beats_stale_reply_and_body_update_without_reopening`
proves the deleted thread remains hidden, stale replies are not retained, stale
body updates do not rewrite retained audit history, deterministic
`stale-comment-reply` and `stale-comment-update` warnings are emitted, and
arrival/batch order converges.
Single-comment deletion now has an explicit order-independent rule as well:
`last_comment_delete_beats_concurrent_reply_without_order_divergence` proves
that deleting the only live comment in a thread suppresses stale concurrent
replies and keeps the thread hidden, while
`single_comment_delete_preserves_concurrent_reply_when_thread_still_has_live_comment`
proves replies are still accepted when the base thread has another live
comment. This keeps automatic merge deterministic without over-deleting active
review threads.
Spreadsheet merge proof now starts at the repository candidate boundary as
well as replay tests: `merge_repository_candidates_inner` derives current and
candidate spreadsheet deltas from the same merge base, sorts and deduplicates
spreadsheet envelopes by actor/sequence, replays them onto the base workbook,
and emits deterministic warnings for conflicting duplicate spreadsheet
operation IDs. The focused test
`spreadsheet_merge_replays_current_and_candidate_deltas_from_base` proves two
simulated editors converge independent of stream order while preserving cell
edits, formula recalculation, validations, cell comments, filters, merged
ranges, named ranges, and source validation.

Operation-boundary evidence: the shared browser/Tauri mock command contract now
checks that every non-maintenance command that changes operation-tracked source,
signature, citation, spreadsheet, comment, suggestion, attachment, or archive
state appends a distinguishable audit operation. Import commands are treated as
document-replacement operations and must leave a non-empty import journal;
warning-only no-ops remain valid graceful degradation. Operation segment
envelopes validate non-empty and whitespace-canonical audit
actor/kind/summary records, non-zero sequences, unique actor/sequence IDs
across the manifest history, single payload shape, and rich-document operation
IDs that match the audit record before repository writes and after history
reads.
The focused test `local_repository_rejects_empty_operation_audit_summary`
proves hash-valid operation segments with empty or whitespace-padded audit
records, and whitespace-padded rich payload actors, are rejected before
operation history is trusted. The browser mock contract validates projected
operation records with the same canonical actor/kind/summary rules, requires
the projected `operation_count` to match the operation array length, and
rejects duplicate projected actor/sequence operation identities before audit
history is treated as valid. Blob
operation payloads are also validated at the same boundary:
`save_rejects_invalid_blob_operation_envelope_before_writing_objects`
and `local_repository_rejects_invalid_blob_operation_segment_history_envelope`
prove invalid blob IDs, hashes, metadata, or typed-signature payloads are
rejected before repository writes and after operation segment decode.
Spreadsheet operation payloads are validated at the same envelope boundary:
`save_rejects_invalid_spreadsheet_operation_envelope_before_writing_objects`
and
`local_repository_rejects_invalid_spreadsheet_operation_segment_history_envelope`
prove invalid sheet IDs, cell addresses, ranges, comments, validations,
filters, formatting, protected ranges, and named ranges cannot enter durable
operation history silently. `save_rejects_operation_envelope_payload_kind_mismatch_before_writing_objects`
and `local_repository_rejects_operation_segment_payload_kind_mismatch` prove
blob and spreadsheet payloads cannot be stored under misleading audit operation
kinds. Rich-document operation envelopes now validate audit kind against the
embedded operation variant for every current `OperationKind`, including generic
block/inline insertions, comments, suggestions, citations, equations, tables,
marks, and document metadata; the focused tests
`save_rejects_rich_operation_envelope_payload_kind_mismatch_before_writing_objects`
and `local_repository_rejects_rich_operation_segment_payload_kind_mismatch`
prove misleading rich-document audit records are rejected before repository
writes and after hash-valid history segment reads. Rich-document operation
payloads also validate embedded source objects before persistence and after
history segment decode, using the same core validators as normal source state:
invalid document metadata, blocks, inlines, marks, comments, suggestions,
footnotes, bibliography references, citation groups, equations, table rows,
table cells, image hashes, and update payloads fail before replay can trust
them. The focused tests
`save_rejects_invalid_rich_operation_payload_before_writing_objects`,
`save_rejects_invalid_table_operation_payloads_before_writing_objects`, and
`local_repository_rejects_invalid_rich_operation_segment_history_payload` prove
that hash-valid operation histories cannot silently carry malformed
rich-document payloads. Mark removal now has explicit source semantics:
boolean marks reject impossible values, valued marks reject empty specific
values, and omitted valued-mark removal values mean remove all matching marks
of that kind. `remove_text_mark_rejects_invalid_mark_payload` proves invalid
app command arguments are rejected before journaling, padded inline IDs are
trimmed to canonical operation targets, and value-less color clearing remains
valid for the GUI toolbar;
`save_rejects_invalid_mark_range_operation_payload_before_writing_objects`
proves malformed formatted-range mark payloads are rejected before repository
writes. The app command surface now exposes `add_text_mark_range` over stable
inline endpoint IDs, with `text_mark_ranges_are_operation_backed_and_persisted`
proving range marks project through the merge engine, persist through local
repository save/open, and remain visible through the shared desktop command
contract. `invalid_mark_removal_operation_degrades_to_warning`
and `valued_mark_removal_without_value_removes_all_matching_marks` prove direct
merge replay keeps the document valid and deterministic for both malformed and
remove-all mark-removal operations. Verified by
`cargo test -p opendoc-app-api operation_envelope`,
`cargo test -p opendoc-app-api text_mark_ranges_are_operation_backed_and_persisted`,
`npm --prefix apps/desktop run build` followed by
`npm --prefix apps/desktop run mock-contract`.

## Version Control And Storage Workstream

Use a content-addressed repository that works on local disk first and maps cleanly to S3/OpenDAL later.

Required:

- Immutable content-addressed objects.
- Manifests with parent links, document UUID, branch/head metadata, snapshot references, operation segment references, blob references, lookup records, tombstones, and signatures.
- Operation segments that may contain keypress-level operations.
- Snapshots for fast open.
- Compare-and-swap head updates where available.
- Deterministic candidate heads where CAS is unavailable or concurrent saves race.
- Automatic candidate reconciliation by fast-forward or operation-level merge.
- UUID lookup and optional DOI alias lookup.
- Repository scanning fallback when no server lookup exists.
- Central lookup acceleration when a server exists.
- Content-addressed blobs for images and arbitrary binary objects.
- Shallow clone support, where missing blobs produce placeholders and warnings.
- Pack files for local small-file mitigation.
- Crash-safe compaction by writing new packs, verifying them, and atomically swapping indexes.
- Tape/archive tombstones describing where recoverable data exists.

Done when local disk and flat object-store tests prove create/open/save/reopen, parallel candidate save reconciliation, missing blob handling, lookup by UUID/DOI, pack compaction, and tombstone recovery metadata.

Current implementation evidence: local disk pack compaction is exposed through
`compact_local_repository` in `opendoc-app-api`, Tauri, and the browser command
contract. The command creates/updates local pack files, returns a projection
and the reusable store conformance harness now requires any backend advertising
`local_pack_files` to compact loose content into a readable hash-addressed pack,
so local and flat filesystem stores prove the same small-file mitigation path.
Verified by `cargo test -p opendoc-store` and
`cargo test -p opendoc-store --features opendal`.
warning with pack statistics, and is tested to preserve operation count and
existing signatures without mutating signed document source state. Pack indexes
are durable binary `PackIndexRecord` records in `opendoc-format`, not text
sidecars, and local pack tests assert the shared binary magic before packed
objects are trusted. The app-level
test `compacted_local_repository_opens_after_loose_objects_are_removed` proves
a saved signed document can reopen and verify after loose content-addressed
objects are removed and only pack files remain. The lower-level store tests
`local_store_reads_objects_from_pack_after_loose_files_are_removed`,
`flat_store_reads_namespaced_objects_from_pack_after_loose_files_are_removed`,
`local_pack_recompaction_preserves_existing_packed_objects`,
`local_pack_write_recovers_from_stale_temp_files`, and
`local_store_rejects_corrupt_packed_object_bytes` cover pack reads,
namespaced flat-store pack reads, recompaction, interrupted temp-file cleanup,
and packed-object integrity; the
focused test `local_store_rejects_corrupt_binary_pack_index` proves corrupt
binary index sidecars fail closed before packed objects are trusted, and
`local_store_rejects_pack_index_that_targets_different_pack` proves an index
file cannot redirect reads to a different pack name, carry an empty pack name
or a pack name that violates the local-pack filename contract, point entries into the pack header,
point outside the pack file, overflow entry ranges, or contain duplicate hash
entries that make packed object lookup ambiguous. Pack readers validate ranges
before allocating entry buffers. The format test
`decoded_repository_records_validate_semantic_required_fields` also rejects
empty or invalid binary `PackIndexRecord` pack names, invalid entry offsets, and
duplicate pack-entry hashes before pack metadata is trusted. It also rejects
overlapping pack byte ranges and offset+length overflow at the binary format
layer, so ambiguous packed-object lookup is blocked before local or S3-backed
stores inspect object payloads.
Repository tests `repository_commits_and_plans_candidates_on_flat_store`,
`repository_can_write_candidate_head_when_cas_fails`,
`repository_resolves_candidate_heads_deterministically`,
`repository_reconciles_fast_forward_candidate_chain`,
`flat_repository_reconciles_fast_forward_candidate_chain`, and
`repository_writes_lookup_indexes_and_tombstones` cover raw flat-store
candidate heads, deterministic reconciliation, UUID/DOI lookup records,
multiple DOI aliases, deduplicated scan records, invalid lookup path rejection,
dot-segment traversal rejection, empty DOI lookup rejection, duplicate
lookup-alias rejection including normalized DOI aliases, lookup sidecars whose
decoded UUID/DOI target does not match the index path, and archive tombstones.
Raw durable branch-head files are strict hash references without wrapper
whitespace across local, flat, and OpenDAL-shaped stores;
`stores_reject_corrupt_branch_head_records` proves malformed and
whitespace-padded heads fail before repository state or candidate
reconciliation trusts them.
Candidate-head scan fallback also validates that each candidate file path and
decoded `BranchHeadRecord.manifest` name the same content-addressed manifest;
`repository_resolves_candidate_heads_while_reporting_invalid_records` proves
path/record mismatches are reported as invalid candidates while valid
fast-forward candidates remain usable. Public candidate listing reuses the same
scanner and returns valid candidate heads without letting corrupt candidate
sidecars block no-server reconciliation;
`repository_lists_valid_candidate_heads_while_ignoring_invalid_records` proves
the valid-list path and leaves detailed invalid-candidate reporting to
resolution/audit code.
Durable lookup aliases require repository-key-safe scheme labels and reject
surrounding whitespace in values, while DOI query inputs remain trim/case
normalized at the repository boundary; this is covered by
`decoded_repository_records_validate_semantic_required_fields`,
`repository_rejects_semantically_invalid_binary_records`,
`repository_writes_lookup_indexes_and_tombstones`, and
`local_repository_writes_uuid_and_doi_lookup_records`.
Archive tombstone locator, restore-hint, and signer metadata are also canonical
durable fields: command inputs trim before writing, injected binary or
app-projection tombstones with surrounding whitespace are rejected, and
`repository_rejects_semantically_invalid_binary_records`,
`app_projection_rejects_invalid_structured_payloads`, `tombstone`, and
`archive` focused app tests prove current, deleted, and orphan tombstones remain
audit/recovery-visible only after validation. The browser mock import/runtime
now validates the same app-projection tombstone shape (`archive_locator`,
`restore_hint`, `created_at_ms`, `signer`), and the mock contract rejects a
padded archive tombstone projection before audit/recovery metadata can drift
from the Rust source rules.
The focused store test
`repository_rejects_lookup_records_from_wrong_index_paths` proves hash-valid
binary lookup records cannot silently redirect UUID/DOI lookup paths, while app
DOI open still treats stale DOI indexes as scan-fallback candidates rather than
open failures. Lookup scan fallback exposes invalid index sidecars separately
from valid discovery results;
`repository_scans_valid_lookup_records_while_reporting_invalid_indexes` proves
corrupt lookup bytes and path-mismatched lookup records do not block valid
UUID/DOI scan discovery. App DOI open carries those scan diagnostics into
document warnings during fallback;
`local_repository_writes_uuid_and_doi_lookup_records` proves corrupt scan
sidecars do not block DOI open and are visible as `doi-lookup-scan-problem`
warnings. `repository_rejects_semantically_invalid_binary_records` now also
proves whitespace-wrapped document UUIDs are rejected both before repository
writes and after reading injected binary lookup records, so semantically
distinct head or lookup identities cannot be introduced below the app boundary.
It also proves malformed branch names are rejected before manifest, candidate
head, or lookup writes, so invalid branches cannot create content-addressed
objects that fail only during mutable head updates. Duplicate manifest segment,
signature, or blob references are rejected at the binary manifest validation
layer before storage or history traversal trusts them.
Hash references are also path-safe at the shared model boundary: algorithms and
digests reject wrapper whitespace, slashes, backslashes, extra colons, dot-only
components, and control characters while retaining algorithm-agile
alphanumeric/`-`/`_`/`.` names. The focused core test
`hash_ref_rejects_path_hostile_components` keeps object, candidate-head,
tombstone, signature-sidecar, and pack-index path derivation from accepting
hash strings that only fail later in a specific backend.
Operation segment integrity is verified for
both local disk and flat object-store layouts: missing and tampered segment
objects fail open attempts with deterministic errors before operation history is
trusted, and hash-valid but semantically mismatched operation envelopes are
rejected before replay, including empty operation segment objects on local,
flat-store, and OpenDAL filesystem repositories, duplicate operation identities
across parent/child segment chains, and operation segments whose
`base_manifest` does not match the manifest parent they claim to extend. The
durable operation segment format rejects non-hash `base_manifest` values.
Local, flat-store, and OpenDAL filesystem no-op saves skip empty operation
segment objects while later non-empty segments keep chaining through no-op
manifests to the previous real segment. Verified by
`cargo test -p opendoc-format operation_segment_records_validate`,
`cargo test -p opendoc-app-api operation_segment_history`,
`cargo test -p opendoc-app-api skips_empty_operation_segments_and_preserves_chain_links`, and
`cargo test -p opendoc-app-api --features opendal-store skips_empty_operation_segments`.
The generic binary operation-segment validator also rejects empty operation
lists and oversized operation lists before app-specific envelope replay, covered by
`snapshot_and_operation_segment_records_validate_required_envelope_fields`.
Operation segment `base_manifest` links must be canonical hash-reference
strings without wrapper whitespace before replay trusts manifest ancestry;
`snapshot_and_operation_segment_records_validate_required_envelope_fields` and
`local_repository_rejects_semantically_invalid_operation_segment_history`
cover the durable-format and repository-history boundaries.
The reusable object-store conformance tests also prove lookup records with
multiple DOI aliases, deduplicated lookup scans, and empty DOI rejection across
local, flat, and OpenDAL filesystem stores. Repository tombstone tests now prove
repository-wide archive tombstone scans and path-target mismatch rejection for
no-server tape/archive recovery. The app audit view surfaces scanned repository
tombstones, including orphan tombstones not attached to current or deleted blob
refs; corrupt scanned tombstones are reported as audit problems without hiding
valid scanned tombstones, and backend app tests cover local, flat, and OpenDAL
filesystem repository audit scans. The browser mock contract
asserts the repository tombstone audit arrays, archive locator, and configured
tombstone scan-problem propagation; the runtime contract proves the same
configuration path survives `runtimeConfig()` exact-shape checks. The desktop GUI
exposes local compaction through the repository toolbar and smoke-tests the visible
`local-repository-compacted` warning after saving a local repository, and the
GUI smoke test verifies visible repository archive-tombstone counts and
tombstone scan-problem counts in the audit panel.
Repository UUID and DOI command inputs are now trimmed before head lookup,
candidate reconciliation, direct DOI lookup, and scan fallback in both the Rust
app API and browser mock. Paths and namespaces remain exact. The Rust dispatch
contract and browser mock contract pass padded UUID/DOI values through local,
flat, and OpenDAL filesystem open/merge-by-identifier commands to keep lookup
normalization test-backed.
`simulate_shallow_clone` is exposed through `opendoc-app-api`, the browser mock,
and Tauri to prove missing-blob placeholder behavior without corrupting a
repository fixture. It clears only local blob bytes/availability, preserves
current blob references, exact-byte signatures, typed signatures, archive
tombstones, and version signatures, and emits `missing-blob` warnings. Verified
by `cargo test -p opendoc-app-api shallow_clone_simulation`,
`npm --prefix apps/desktop run mock-contract`, and
`npm --prefix apps/desktop run gui-smoke`.
The backend conformance tests
`local_missing_blob_keeps_exact_byte_signature_sidecar_trust`,
`flat_missing_blob_keeps_exact_byte_signature_sidecar_trust`, and
`opendal_fs_missing_blob_keeps_exact_byte_signature_sidecar_trust` now also
prove that live shallow-cloned FASTQ blobs keep exact-byte blob-signature trust
by hash while typed semantic FASTQ signatures become `untrusted` when the blob
bytes are absent, both in the normal projection and audit view.
The store layer also exposes a read-only manifest dependency audit that reports
snapshot, operation-segment, version-signature, and blob object availability
without mutating repository state. `repository_audits_manifest_dependencies_for_shallow_clone_recovery`
proves this audit preserves reusable blob-signature sidecar trust for present
blobs, distinguishes missing operation segments from missing blob bytes, and
reports which missing blobs are recoverable from archive tombstones. The same
store test suite now pins pack-index overflow rejection at the binary index
validation layer before any backend reads corrupted pack payloads.
`verify_object_store_contract` now exercises the same audit for local, flat,
and OpenDAL filesystem conformance runs, so future object-store backends must
preserve the same shallow-clone and archive-recovery semantics. The
app-level backend matrix for
`local_missing_blob_keeps_exact_byte_signature_sidecar_trust`,
`flat_missing_blob_keeps_exact_byte_signature_sidecar_trust`, and
`opendal_fs_missing_blob_keeps_exact_byte_signature_sidecar_trust` also asserts
that the repository audit and visible app projection agree on missing,
recoverable signed blobs before opening the shallow-cloned document.
Tauri command metadata is contract-checked against the shared command list:
`npm --prefix apps/desktop run command-contract` proves `commands.v0.json`,
TypeScript command types, browser mock command coverage, Rust app dispatch,
Tauri `build.rs` command generation, the default capability allow list, and
generated allow/deny permission files all name the same command set.

## Signing And Audit Workstream

Signing is optional. It must not block normal document use.

Required:

- Rust-native OpenSSH-compatible signing by default.
- Optional `ssh-keygen` helper only if useful.
- Multiple signatures per version.
- Algorithm agility in hash and signature envelopes.
- Manifest/version signatures over source state and retained history reachable from the manifest.
- Exact-byte detached blob signatures keyed by content hash.
- Sidecar signatures for images and arbitrary binary objects, with embedded signatures allowed only where that simplifies a supported format.
- Typed semantic signatures, including image semantic profiles, FASTQ sequence-only profiles, and FASTQ full-content profiles.
- Trust states: unsigned, signed, trusted, untrusted, broken.
- Audit/recovery view for warnings, deleted comments, resolved suggestions, deleted citations, operation history, signatures, missing blobs, invalid candidates, and archive tombstones.

Do not sign rendered PDF/output, computed spreadsheet values, citation label renderings, equation renderings, volatile UI state, browser caches, or import provenance. Formula signatures cover formula source, not computed values.

Done when valid signatures verify after save/open, tampering is detected, multiple signatures verify independently, unsigned documents remain openable, and typed signature APIs can sign storage-independent byte or semantic profiles.

Current implementation evidence: `opendoc-app-api` signs the current canonical
snapshot with Rust-native OpenSSH-compatible keys, clears version signatures on
source edits, persists multiple manifest signature sidecars through local
save/open, verifies distinct OpenSSH signer identities independently after
reopen, exposes reopened version signatures in the audit projection, verifies
`unsigned`, `signed`, `trusted`, `untrusted`, and `broken` states, rejects
missing/corrupt/mismatched manifest signature sidecars on local disk,
flat object-store, and OpenDAL filesystem layouts, and keeps exact-byte blob
signatures reusable by
content hash. Signature envelopes now canonicalize signer, display-name, and
title metadata at signing time and reject whitespace-padded injected durable or
app-projection records before they are trusted; durable binary signature
records also reject empty signature byte payloads before sidecars are trusted,
and `opendoc-sign` now validates trimmed signer/title metadata before invoking
the signing backend, so invalid UI/import metadata cannot trigger private-key
or backend signing work. This is covered by
`sign_target_rejects_invalid_metadata_before_backend_signing`,
`sign_target_canonicalizes_signature_metadata`,
`decoded_repository_records_validate_semantic_required_fields`,
`repository_rejects_semantically_invalid_binary_records`, and
`app_projection_rejects_invalid_structured_payloads`. The browser mock contract
verifies that signing through the
shared command API is visible through `get_audit_view` signer metadata. Typed semantic blob
tests cover FASTQ sequence-only signing and image pixel-profile signing,
including malformed input and tamper detection. Equation TeX/LaTeX source is
inside the signed source payload for both inline and block equations. App
source validation rejects typed semantic signature claim lists with duplicate
included/excluded field names or fields claimed as both included and excluded,
so semantic signatures remain auditable before source snapshots are trusted.
The browser mock contract now requires `typed_signatures` on blob projections
and verifies that image pixel signing records the storage-independent
`opendoc.image.pixels.v0` profile, canonical dimensions, normalized pixel
coverage, compression exclusion, signature bytes, and RGBA profile payload. It
also rejects non-byte RGBA profile payloads before mutating typed signatures,
matching the Rust malformed-pixel rejection boundary.
`opendoc-sign` now exposes target-aware verification helpers for backend and
embedded-public-key verification, so callers can reject copied or mismatched
sidecars even when signature bytes still verify against the supplied payload.
The
focused verification tests
are `openssh_signing_hook_signs_current_snapshot_and_clears_on_edit`,
`document_signature_verification_reports_signed_trusted_untrusted_and_broken`,
`signed_snapshot_persists_signature_sidecar`,
`local_repository_rejects_missing_manifest_signature_object`,
`local_repository_rejects_corrupt_manifest_signature_object`,
`local_repository_rejects_manifest_signature_with_wrong_signing_payload_target`,
`flat_repository_rejects_missing_manifest_signature_object`,
`flat_repository_rejects_corrupt_manifest_signature_object`,
`flat_repository_rejects_manifest_signature_with_wrong_signing_payload_target`,
`opendal_fs_repository_rejects_missing_manifest_signature_object`,
`opendal_fs_repository_rejects_corrupt_manifest_signature_object`,
`opendal_fs_repository_rejects_manifest_signature_with_wrong_signing_payload_target`,
`exact_byte_blob_signature_is_reusable_sidecar`,
`local_corrupt_blob_signature_sidecar_warns_and_opens`,
`flat_corrupt_blob_signature_sidecar_warns_and_opens`,
`opendal_fs_corrupt_blob_signature_sidecar_warns_and_opens`,
`local_wrong_target_blob_signature_sidecar_warns_and_opens`,
`flat_wrong_target_blob_signature_sidecar_warns_and_opens`,
`opendal_fs_wrong_target_blob_signature_sidecar_warns_and_opens`,
`local_deleted_corrupt_blob_signature_sidecar_warns_in_audit`,
`flat_deleted_corrupt_blob_signature_sidecar_warns_in_audit`,
`opendal_fs_deleted_corrupt_blob_signature_sidecar_warns_in_audit`,
`local_deleted_wrong_target_blob_signature_sidecar_warns_in_audit`,
`flat_deleted_wrong_target_blob_signature_sidecar_warns_in_audit`,
`opendal_fs_deleted_wrong_target_blob_signature_sidecar_warns_in_audit`,
`signing_rejects_structurally_invalid_app_snapshot_source`,
`verification_rejects_structurally_invalid_app_snapshot_source`,
`signatures_include_equation_source`,
`fastq_typed_blob_signature_persists_and_verifies_after_open`,
`fastq_typed_blob_signature_rejects_missing_or_malformed_blob`,
`image_pixel_typed_blob_signature_persists_and_verifies_after_open`, and
`image_pixel_typed_blob_signature_rejects_bad_pixels`. The lower-level signing
tests `target_aware_verification_rejects_reused_sidecar_for_wrong_hash` and
`openssh_target_aware_verification_rejects_wrong_manifest_target` prove the
target boundary directly. The browser mock command contract now also requires
command-created FASTQ and image typed semantic signatures to use
content-hash-shaped semantic digests and matching signature targets, keeping the
debug/browser path aligned with the Rust signing boundary. The same browser
contract now validates generic signature metadata targets, signer fields,
titles, and timestamps instead of only checking field presence, so malformed
trust metadata is caught in the browser/debug projection. Exact-byte blob
signatures in the browser contract now also require content-hash-shaped blob
hashes, signature arrays, and signature targets that match the signed blob hash,
matching the import-side Rust/API trust boundary. The same contract includes
negative projection checks proving duplicate exact-byte blob signatures and
duplicate typed semantic signatures are rejected before they can appear in
normal browser/debug state.
Browser projection validation now also rejects unsupported document, exact blob,
and typed semantic signature-state labels with explicit negative mock-contract
coverage, matching the Rust source validator's `unsigned`, `signed`, `trusted`,
`untrusted`, and `broken` trust-state boundary. App source validation now also
validates top-level document signature metadata, the current-signature
projection, and duplicate document signature keys; the Rust
`app_projection_rejects_invalid_structured_payloads` test and browser
`mock-contract` duplicate-document-signature projection check cover that
boundary. The same tests now require the single `signature` projection to be
absent only when `signatures` is empty, and otherwise to match the first
document signature exactly, preventing debug/browser projections from drifting
away from the signed source trust list. They also prove document
`signature_state` cannot claim a signed/trusted/broken state without document
signatures, and cannot claim `unsigned` while signatures are present.
Exact-byte blob `signature_state` now follows the same derived projection rule:
signed/trusted exact-byte states require exact-byte blob signatures, while
`unsigned` cannot be paired with exact-byte signatures. Missing-blob
placeholders may still project `untrusted` without exact signatures; Rust
source validation and the browser `mock-contract` cover those mismatches.
Typed semantic signature entries now also reject the impossible
`signature_state: unsigned` projection because a typed entry always carries
signature bytes and a semantic signature envelope; `untrusted` remains valid
for shallow clones, missing bytes, and unknown profiles. Browser projection
validation now also rejects empty typed semantic signature byte arrays, matching
the Rust source validator's signed-sidecar byte requirement. Typed profile
payload shape is now profile-aware: image pixel signatures require a non-empty
profile payload, while FASTQ semantic signatures must not carry one; Rust
source validation and the browser `mock-contract` cover those cases. Rust
source validation now also parses the canonical image pixel profile frame before
trusting an image semantic signature payload, and rejects declared image
semantic digests that do not match that canonical payload. Browser projection
validation now mirrors that image-profile boundary in both the browser-local
runtime validator and the mock app API by storing canonical image pixel frames
for image semantic signatures, recomputing the profile-payload digest, and
rejecting digest/payload drift. Rust Google Docs-shaped import and the browser
mock import contract now both abort image-pixel typed signature metadata whose
declared semantic digest does not match the canonical payload, before replacing
current document state. Browser validation also mirrors the portable byte-level
and field-list rules by rejecting empty image payloads plus empty, padded,
duplicate, and overlapping included/excluded field claims.

## Citations Workstream

Use a special citation label node, not ordinary links.

Design:

- Store a document-local bibliography database.
- Citation occurrences reference bibliography records by local IDs.
- Citation groups support locators, prefixes, suffixes, and suppress-author flags.
- Updating one bibliography record updates all dependent labels.
- Use `citum` first for rendering/source integration.
- Keep CSL/CSL-JSON as future import/export adapters.
- Paperpile-style Google Docs link embedding is research input only; OpenDoc does not need to copy that storage design.

Done when updating a reference updates all dependent labels, citation labels survive save/open, merge tests cover citation movement and bibliography edits, and rendered labels remain projection/cache state outside source signatures.

Current implementation evidence: `opendoc-core` models citation labels as
first-class inline nodes backed by a document-local `CitationDatabase`, and
`opendoc-citations` provides the v0 `citum-native` source/projection adapter.
The adapter keeps ordinary citation source text human-readable while escaping
newlines, carriage returns, semicolons, and backslashes in line-oriented
fields, so titles, URLs, and author names cannot be reparsed as different
document-local source fields or extra authors. Verified by
`citum_native_source_bytes_escape_line_and_author_separators` and
`citum_native_parser_keeps_unknown_escape_sequences_literal`; the browser mock
contract verifies the same escaped `bytesUtf8` shape through
`add_bibliography_reference` and `export_google_docs_json`. Google Docs-shaped
citation import also unescapes those fields, fills missing summary metadata,
and recomputes live citation projection caches after import repair, so moved
footnote citations warn about missing footnotes while still rendering from
valid document-local bibliography source. Verified by
`citation_import_unescapes_citum_native_source_fields` and the updated
`google_docs_citation_import_moves_missing_footnote_placement_inline`.
`opendoc-app-api` exposes bibliography-reference updates, metadata updates,
grouped citations, footnote citations, style changes, delete/restore, and
save/open persistence through operation-backed commands. The GUI citation panel
now exposes per-reference Cite actions, and generic Docs menu/context Cite
paths fall back to the first live bibliography record instead of hardcoding a
sample reference; locator, label, prefix, and suffix are entered through the
same prompt-backed operation path. The focused tests
`bibliography_reference_update_rerenders_all_dependent_citation_labels`,
`citation_occurrence_inserts_document_local_label_and_persists`,
`citation_group_items_update_rerenders_labels_and_persists`,
`footnote_citation_group_inserts_source_placement_and_persists`,
`signatures_exclude_citation_rendering_projection_cache`, and
`signatures_include_document_local_citation_source` prove that repeated
citations update from one local reference, labels survive persistence, rendered
labels remain projection state, and citation source bytes remain signed source
state. `opendoc-citations`, `opendoc-app-api`, and the browser mock renderer
now render citation locator labels such as `page 17` from structured citation
items in both inline and footnote citation groups; verified by
`renders_author_year_and_numeric_labels_from_document_local_database`,
`citation_group_items_update_rerenders_labels_and_persists`,
`google_docs_citation_import_degrades_missing_footnote_placement_inline`,
`npm --prefix apps/desktop run mock-contract`, and
`npm --prefix apps/desktop run gui-smoke`. Incomplete imported author-year
references render visible stable placeholders such as `[citation-id]` but do
not persist those placeholders as durable `rendered_cache` source projection;
verified by `incomplete_author_year_reference_renders_group_placeholder`,
`google_docs_citation_import_degrades_missing_references_without_stale_labels`,
and `google_docs_citation_import_degrades_missing_footnote_placement_inline`.
Citation style and locale command inputs are normalized before storage,
while trusted core/app source and durable style-change operations reject padded
style/locale metadata; covered by
`source_model_rejects_empty_stable_ids_after_decode`,
`app_projection_rejects_invalid_structured_payloads`, and
`save_rejects_padded_citation_style_operation_fields_before_writing_objects`.
App source validation now also allows missing or deleted document-local
bibliography references to degrade gracefully, but rejects live citation groups
that keep stale rendered label caches for those missing references; covered by
`app_projection_rejects_invalid_structured_payloads`.
Malformed bibliography-reference and citation-group operation payloads are also
rejected before repository writes by
`save_rejects_invalid_citation_operation_payloads_before_writing_objects`, so
hash-valid operation history cannot smuggle invalid document-local citation
source into replay.
Trusted citation source now rejects whitespace-only bibliography source bytes,
whitespace-padded custom source format labels, whitespace-padded bibliography
summary metadata, including titles, authors, DOI, URL, and issued fields, plus
citation item locator/label/prefix/suffix fields before those fields can become
signed source or merge inputs; covered by
`citation_payloads_reject_empty_source_fields`,
`app_citation_projection_rejects_invalid_source_payloads`,
`malformed_citation_extension_import_aborts`,
`malformed_google_docs_citation_metadata_does_not_replace_current_document`,
and the browser mock command contract malformed citation cases.
Google Docs-shaped citation import now degrades citation groups that reference
missing or deleted bibliography records by clearing stale rendered citation
caches, clearing inline label caches back to stable citation IDs, and emitting
`citation-reference-missing` warnings; verified by
`google_docs_citation_import_clears_stale_labels_for_missing_references`,
`google_docs_citation_import_degrades_missing_references_without_stale_labels`,
and the browser mock command contract rich-import fixture.
The same import repair path now moves footnote-placed citation groups inline
when their Google Docs footnote target is missing, clears stale group and inline
render caches, and emits `citation-footnote-target-missing`; verified by
`google_docs_citation_import_moves_missing_footnote_placement_inline`,
`google_docs_citation_import_degrades_missing_footnote_placement_inline`, and
the browser mock command contract rich-import fixture.
Inline citation labels that reference missing or deleted citation groups also
degrade during Google Docs-shaped import, including labels nested in imported
tables; verified by
`google_docs_citation_import_clears_stale_labels_for_missing_groups`,
`google_docs_citation_import_clears_nested_labels_for_deleted_groups`,
`google_docs_citation_import_degrades_missing_group_without_stale_label`, and
the browser mock command contract rich-import fixture.
`opendoc-merge` covers citation movement, bibliography edits,
delete/restore revision ordering, missing-reference degradation, and stale cache
repair in operation-level merge tests. The focused test
`footnote_citation_placement_survives_concurrent_inline_reference_delete`
proves a footnote remains live when a citation group is placed in that footnote
even if another actor deletes the visible inline footnote reference, preserving
citation-owned footnote reachability across replay order.

## Spreadsheet Workstream

Spreadsheet v0 includes formula evaluation.

Required:

- Sparse multi-sheet workbook model.
- Stable sheet, row, column, and cell identities.
- Typed cell values and formatting.
- Formula parser/evaluator.
- Deterministic dependency graph and invalidation.
- Formula source is signed; computed values are cache/projection state.
- Named ranges.
- Comments.
- Filters.
- Warning-only protected ranges.
- Validations.
- Frozen panes.
- Merged ranges.
- Google Sheets API-shaped import/export.
- Graceful warnings for unsupported formulas or imports.

Done when formulas evaluate deterministically, cached computed values are excluded from signatures, dependencies update after edits, imports fail cleanly on unsupported structures, and merge tests cover concurrent sheet/cell edits.

Current implementation evidence: `opendoc-app-api` exposes operation-backed
spreadsheet commands for workbook metadata, cell edits, bulk edits, sheets,
rows, columns, comments, formats, formulas, named ranges, filters, validations,
frozen panes, merged ranges, and warning-only protected ranges. The focused
tests `spreadsheet_formula_engine_handles_arithmetic_ranges_and_errors`,
`spreadsheet_formula_engine_handles_workday_holiday_ranges`,
`spreadsheet_formula_engine_handles_paired_correlation_ranges`,
`spreadsheet_dependency_graph_tracks_deterministic_invalidation`,
`signatures_exclude_spreadsheet_axis_and_formula_projection_cache`,
`assert_candidate_merge_recomputes_formula_dependencies`,
`assert_candidate_merge_resolves_new_named_range_formula`,
`assert_candidate_merge_preserves_spreadsheet_filter_options`, and
`assert_candidate_merge_preserves_spreadsheet_structural_features` prove
formula evaluation, deterministic dependency invalidation, source-signature
boundaries, and candidate merge behavior across local, flat, and OpenDAL
filesystem repositories. The arithmetic/range formula test and browser mock
contract now also cover Google Sheets-style `AVERAGEA`, `MINA`, and `MAXA`
aggregate variants, including source-value text/boolean coercion, empty-range
degradation, and formula dependency reporting; `examples/formulas/v0.tsv` lists
all three functions as supported v0 formulas. The same formula test and browser mock
contract now cover `DAYS360` with deterministic US/NASD and European month-end
semantics plus bad-arity warning degradation, and the public formula fixture
lists it as supported. `WEEKNUM` now shares the same Rust/browser date serial
path, supports default, explicit start-day, and ISO mode `21`, rejects
unsupported modes deterministically, and is listed in the public formula
fixture. Logical formula coverage now includes `XOR` over numeric truthiness
and ranges, with deterministic bad-arity degradation and browser/Rust
dependency reporting. Operator coverage now includes `ISBETWEEN` with default
inclusive bounds, explicit exclusive endpoints, dependency reporting, and
bad-arity degradation. Info formula coverage now includes `TYPE`, returning
displayed-value type codes for number, text, boolean, and error values without
treating formula source cells as a distinct displayed type, plus `ERROR.TYPE`
for deterministic numeric classification of spreadsheet error values and
`#N/A` degradation for non-error inputs. `ISREF` now classifies valid cell or
named-range references without treating quoted reference text as a reference.
`ISEMAIL` now provides deterministic common-format email validation over
literal text and referenced cell text without network existence checks.
`ISURL` now provides deterministic syntactic URL validation for literal and
referenced text, including Google-documented optional protocols and bare
domains without depending on live TLD lookups. `ISDATE` now provides a
deterministic date predicate for ISO date text and date-producing formula
sources, without treating ordinary numeric cells as dates merely because
spreadsheets encode dates as serial numbers. `ISEVEN` and `ISODD` now classify
negative numeric inputs by truncating toward zero and applying signed parity
instead of rejecting negative integers. `LOG` now supports the Google Sheets
optional base argument, defaulting to base 10 when only the value is supplied
while preserving explicit-base dependency reporting and `#NUM`/`#VALUE`
degradation in Rust and the browser mock contract. The Google Sheets rounding-family
coverage now includes `FLOOR.MATH`, `FLOOR.PRECISE`, `CEILING.MATH`,
`CEILING.PRECISE`, and `ISO.CEILING` in the Rust evaluator, browser mock
contract, and public formula fixture, including sign-ignored significance,
negative-number mode behavior, zero-significance errors, and alias semantics.
`BASE` and `DECIMAL` now exercise text-result formula projection and
text-to-number base conversion in Rust and the browser mock, with tests for
radix truncation, base-2-through-36 bounds, zero padding, invalid digits,
negative-input degradation, bad arity, and dependency reporting for referenced
cells. Text formula coverage also includes Google Sheets `PROPER`, including
literal text, referenced text dependency reporting, and bad-arity degradation,
Google Sheets `CLEAN`, including removal of non-printable ASCII control
characters, referenced text dependency reporting, and bad-arity degradation,
plus Google Sheets `T`, which returns only text-valued inputs while leaving
numeric/boolean source values as empty text and preserving dependency reporting
in both Rust and the browser mock contract. `ERF.PRECISE` and
`ERFC.PRECISE` are now supported as Google Sheets
aliases for `ERF` and `ERFC`, including nested-formula coverage and bad-arity
degradation in Rust and the browser mock.
Sheet-scoped spreadsheet commands now trim wrapper
whitespace from `sheetId` before lookup and operation recording in both the
Rust app API and browser mock contract, and candidate replay applies the same
normalization to operation `sheet_id` values while degrading empty sheet IDs to
`invalid-spreadsheet-sheet` warnings. Persisted operation segments remain
stricter than replay repair: injected padded sheet IDs are rejected before
repository writes by
`save_rejects_padded_spreadsheet_sheet_id_operations_before_writing_objects`.
Formula errors now degrade to deterministic projection warnings instead of
silent error cells: formula source remains editable, the computed cell shows the
spreadsheet error value, and stale `spreadsheet-formula-error` warnings are
cleared after formula repair. The warnings are recomputed from current formula
projection and remain outside the signed source payload alongside computed
values; verified by
`spreadsheet_formula_errors_project_deterministic_warnings_and_repair`,
`spreadsheet_formula_engine_handles_arithmetic_ranges_and_errors`,
`signatures_exclude_spreadsheet_axis_and_formula_projection_cache`,
`npm --prefix apps/desktop run mock-contract`, and
`npm --prefix apps/desktop run gui-smoke`. Formula copy/paste shifts relative A1
references and preserves absolute anchors while leaving quoted string literals,
quoted sheet titles, escaped apostrophes in sheet titles, and unquoted sheet
names such as `Sheet1!C$3` intact; verified by
`spreadsheet_formula_copy_does_not_shift_string_literals_or_sheet_names` and
`spreadsheet_copy_paste_preserves_format_and_shifts_formulas`. The browser mock
contract mirrors that copy/paste boundary and asserts the same quoted-token
preservation through `copy_spreadsheet_range`. Formula dependency extraction
also ignores cell-looking tokens inside quoted string literals while retaining
real range dependencies; verified by
`spreadsheet_dependency_graph_ignores_cell_tokens_inside_string_literals` and
the browser mock contract's dependency-graph assertion. Dependency graph
projection entries and per-cell dependency labels are now source-validated for
canonical cell labels, qualified sheet prefixes, duplicate graph keys, and
duplicate dependency labels before trusted snapshots are accepted; the browser
mock graph builder preserves unresolved qualified dependency labels without
truncating them at embedded sheet separators, and the browser mock contract
validates the same projection boundary. The browser mock also
keeps quoted and unquoted sheet-qualified dependencies as qualified dependency
strings, so formulas such as `'Q1 Data'!N2` and `Sheet1!N$1` do not degrade
into local `N2`/`N1` invalidation edges. Cross-sheet dependency graph entries
are keyed on the referenced sheet and expose qualified dependent labels such as
`Summary!B1`/`Sheet1!P2`; verified by
`google_sheets_json_import_evaluates_cross_sheet_formula_ranges`,
`spreadsheet_dependency_graph_tracks_deterministic_invalidation`, and the
browser mock contract. Sheet renames rewrite title-qualified formula sources
to the new title while leaving stable sheet-ID-qualified formulas unchanged, so
rename keeps formula anchors live without hiding the source edit; verified by
`spreadsheet_sheet_rename_rewrites_title_qualified_formula_sources`,
`spreadsheet_replay_sheet_rename_rewrites_title_qualified_formula_sources`, and
the browser mock contract. The replay test proves saved operation histories and
candidate reconciliation use the same rename semantics as interactive editing.
Replayed row/column deletes that would remove the last visible axis now degrade
to deterministic `invalid-spreadsheet-row-delete` and
`invalid-spreadsheet-column-delete` warnings without removing the final axis;
verified by
`final_spreadsheet_axis_delete_replay_warns_without_removing_last_axis`.
Deleted row/column formula references remain openable as `#REF` formulas while
the dependency graph keeps the stale source anchors for audit and later repair;
verified by
`spreadsheet_axis_delete_degrades_formula_refs_but_keeps_graph_anchors`. The
focused replay test
`spreadsheet_sheet_delete_beats_stale_sheet_edits_without_resurrection` proves
that deleting a non-final sheet wins over later stale cell, format, named-range,
and filter operations targeting that sheet; those operations degrade to
deterministic `missing-spreadsheet-sheet` warnings without recreating sheet
state or named-range references. The companion replay test
`spreadsheet_sheet_delete_beats_stale_restore_payloads_without_resurrection`
extends the same invariant to stale row, column, validation, merge, filter,
protected-range, and named-range restore payloads, so retained audit payloads do
not resurrect a sheet that a newer merge deleted. The focused replay test
`spreadsheet_axis_delete_beats_stale_cell_edits_without_resurrection` now
extends delete-wins replay to row and column axes: stale cell value, formatting,
comment, and validation operations targeting axes deleted by another actor
degrade to deterministic warnings instead of implicitly recreating row or
column structure. Spreadsheet candidate replay
now validates after regenerating formula projections, so candidate formula edits
are not rolled back because transient `computed_kind = formula` source state
exists before evaluation. This is covered by the full `opendoc-app-api` library
suite and the focused candidate merge tests
`local_repository_merges_divergent_spreadsheet_candidate_operations`,
`local_candidate_merge_resolves_new_named_range_formula`,
`flat_candidate_merge_resolves_new_named_range_formula`,
`local_repository_merges_same_cell_value_and_format_candidates`,
`flat_repository_merges_same_cell_value_and_format_candidates`,
`local_repository_merges_three_spreadsheet_candidate_editors`, and
`flat_repository_merges_three_spreadsheet_candidate_editors`. The focused
candidate-merge tests
`*_candidate_merge_recomputes_formula_after_named_range_move` now prove the
stale-projection hard case where one editor moves an existing named range,
another editor changes a newly included source cell, and a third stale editor
adds a formula against that named range; merge re-evaluates the formula and
dependency graph automatically after local, flat, and OpenDAL-gated replay.
Source-level spreadsheet range payloads for filters, protected ranges, and named ranges now reject
non-canonical single-cell spellings such as `A1:A1`, while `add_named_range`
canonicalizes command/import input before storing signed source. Trusted source
and durable named-range operations also reject non-canonical names, so lowercase
or otherwise padded names cannot enter signed source or operation history after
the command boundary; covered by
`spreadsheet_source_validation_rejects_invalid_structural_payloads`,
`save_rejects_noncanonical_named_range_operation_name_before_writing_objects`,
`save_rejects_noncanonical_spreadsheet_range_operations_before_writing_objects`,
`save_rejects_noncanonical_spreadsheet_merge_range_operation_before_writing_objects`,
`save_rejects_noncanonical_spreadsheet_unmerge_range_operation_before_writing_objects`,
and `google_sheets_json_import_export_preserves_v0_workbook_source`. Formula
source is now canonical at the command, source-validation, Rust import, and
browser mock boundaries: only values whose first character is `=` are formula
source, so whitespace-padded formula strings remain literal text or abort
Google Sheets import instead of entering signed workbook state with a
non-canonical formula spelling. Normal cell entry now also keeps
whitespace-padded numbers and booleans as literal string source while canonical
finite numbers and exact `TRUE`/`FALSE` or lowercase booleans are stored as
typed source with canonical lowercase boolean spelling, preventing user input
from creating invalid signed workbook state before save. Covered by
`spreadsheet_source_validation_rejects_invalid_structural_payloads`,
`spreadsheet_cell_entry_keeps_noncanonical_typed_values_as_strings`,
`malformed_google_sheets_cell_payloads_do_not_replace_current_workbook`, and
`npm --prefix apps/desktop run mock-contract`. The browser
mock Google Sheets adapter now canonicalizes Google grid ranges on import and
its contract validator rejects non-canonical named-range, merge, filter, and
protected-range source payloads, including whitespace-padded structural IDs and
whitespace-padded descriptions, duplicate sheet titles, duplicate merge and
protected-range source IDs, multiple basic filters per sheet, plus
non-canonical filter criterion/sort columns, keeping browser-local and
Tauri/Rust command semantics aligned. Protected ranges are now strictly
warning-only in v0 source and durable operation records: command and Google
Sheets import inputs with `warningOnly: false` are downgraded to
`warning_only: true` with deterministic `protected-range-warning-only`
warnings, while trusted source projections with `warning_only: false` are
rejected. Verified by
`spreadsheet_protected_ranges_persist_replay_and_are_signed_source_state`,
`invalid_spreadsheet_protected_range_replay_is_transactional_warning`,
`npm --prefix apps/desktop run mock-contract`, and
`npm --prefix apps/desktop run gui-smoke`.
Rust filter-option commands now normalize criterion and sort columns plus
criterion conditions before operation recording, while injected padded durable
filter-option operations are rejected before repository writes; covered by
`spreadsheet_basic_filter_persists_replay_and_is_signed_source_state` and
`save_rejects_padded_spreadsheet_filter_option_operation_before_writing_objects`.
Spreadsheet row and column axis metadata now requires
canonical IDs derived from canonical labels (`row-<n>` and `col-<A1 column>`)
in Rust source validation and in the browser mock contract. The browser
contract also rejects non-canonical visible row/column labels, duplicate visible
axis labels, and axis labels outside the visible sheet grid, closing another
duplicate signed-source representation for the same visible sheet axis. Durable
row/column axis operation labels are also stricter than command input: injected
padded, lowercase, or otherwise non-canonical row or column labels are rejected
before repository writes by
`save_rejects_noncanonical_spreadsheet_axis_label_operations_before_writing_objects`.
Sheet
cell source validation now runs before trusted source snapshots are accepted:
cell addresses must be canonical, source kinds are restricted to the v0 set, and
formula, number, boolean, cell-format, and cell-validation source values must
have their canonical shape, including trimmed number formats; the browser mock
contract validates the same command result boundary, including boolean
`strict` and `show_dropdown` validation flags in projected browser-local
source state, and
`save_rejects_padded_spreadsheet_number_format_operation_before_writing_objects`
proves injected padded durable number-format operations are rejected before
repository writes. Cell-format command properties are normalized before
operation recording, and injected padded durable format properties are rejected
by `save_rejects_padded_spreadsheet_format_property_operation_before_writing_objects`.
Durable cell-address operation fields are also stricter than command input:
injected lowercase, absolute, or padded cell addresses are
rejected before repository writes by
`save_rejects_noncanonical_spreadsheet_address_operations_before_writing_objects`.
Copy/paste operation source ranges are held to the same durable boundary:
forgiving command input normalizes before operation recording, while injected
non-canonical source ranges are rejected before repository writes by
`save_rejects_noncanonical_spreadsheet_copy_range_operation_before_writing_objects`.
Stored computed cell cache
fields are validated as projection metadata, including the cleared projection
shape used for signing payloads, without adding them to signature material.
Google Sheets-shaped import also treats `effectiveValue` and `formattedValue`
as external projection/cache fields: formula source comes only from
`userEnteredValue`, computed values are recomputed after import, and export
does not re-emit stale Google cache fields; covered by
`google_sheets_json_import_export_preserves_v0_workbook_source` and the browser
mock contract.
Spreadsheet
cell comment source now rejects empty or whitespace-padded IDs/authors, empty
bodies, and duplicate IDs before trusted source snapshots are accepted, while
Rust and browser-local command paths trim user-entered authors before storing
signed source. Cell-comment update, delete, and restore command inputs also
trim wrapper whitespace from comment IDs before lookup and operation recording,
so forgiving UI/import inputs cannot create padded durable audit targets;
injected padded add-comment operation authors are rejected before repository
writes by
`save_rejects_padded_spreadsheet_comment_author_operation_before_writing_objects`;
verified by
`spreadsheet_source_validation_rejects_invalid_structural_payloads`,
`spreadsheet_cell_comments_persist_update_and_hide_deleted_state`, and the
browser mock contract. The browser mock contract and Google Sheets-shaped Rust
import/export tests cover formulas, notes/cell comments, filter criteria, sort
specs, named ranges, validations, frozen panes, merged ranges, and protected
ranges.

## Import And Export Workstream

Required:

- Google Docs API-shaped import/export for the supported document subset.
- Google Sheets API-shaped import/export for the supported spreadsheet subset.
- `.doc`/`.docx` import first where easy-to-install Linux/macOS/Windows tooling exists.
- Unsupported recoverable content opens with explicit warnings.
- Unsupported high-risk structures abort import deterministically.
- Debug JSON export for tests and inspection only.

Done when fixture imports produce deterministic source states, unsupported cases are explicit, and round-trip exports preserve the v0 subset.

Current implementation evidence: the browser mock Google Docs-shaped adapter
now imports and re-exports the same OpenDoc extension names as the Rust adapter
for footnotes, document-local citations, comments, suggestions, inline
citations, inline equations, mentions, block equations, image blob references,
tables with stable row/cell IDs, and shallow `opendocBlobs` metadata. The
`mock-contract` fixture parses exported Google Docs JSON and imports a rich
Google Docs-shaped document to prove those fields survive the shared frontend
command surface; its table validator now rejects non-canonical or duplicate
table row/cell IDs so imported table anchors remain usable for merge and
review metadata. The focused Rust regression
`google_docs_import_canonicalizes_wrapper_title` and the browser mock contract
prove Google Docs import wrapper titles are canonicalized before source
replacement and empty titles abort atomically. The app-level regression
`google_docs_table_import_has_stable_row_cell_ids_and_persists` proves
Google Docs-shaped table imports produce stable row/cell IDs that survive
local repository save/open. Google Sheets-shaped browser contract coverage already
includes frozen panes, merges, filters, protected ranges, named ranges,
validations, formulas, Google notes, and `opendocCellComments`; it now also
aborts high-risk pivot-table imports and duplicate Google `sheetId`/title
imports while verifying the source projection is unchanged after each failed
command. The app-level regression
`malformed_google_sheets_sheet_identity_does_not_replace_current_workbook`
proves the Rust app API keeps the current workbook unchanged when imported
Google Sheets sheet identities are ambiguous. The app-level regression
`malformed_google_sheets_named_ranges_do_not_replace_current_workbook` and the
browser mock contract prove unknown named-range target sheets and duplicate
named-range names abort without replacing the current workbook. The app-level
regression `malformed_google_sheets_ranges_do_not_replace_current_workbook` and
the browser mock contract prove duplicate imported merge ranges, duplicate
protected-range IDs, duplicate protected ranges, missing protected-range range
objects, and malformed warning-only flags also abort before replacing the
current workbook. The browser mock contract also proves Google Sheets
numeric metadata such as `sheetId`, grid dimensions, and grid range indices are
validated as integers instead of being silently coerced before source
replacement. The app-level regression
`malformed_google_sheets_properties_do_not_replace_current_workbook` and the
browser mock contract prove malformed workbook properties, sheet properties,
grid properties, and sheet titles abort atomically instead of being coerced
into signed source. The focused regression
`google_sheets_import_canonicalizes_workbook_metadata` and the browser mock
contract prove Google Sheets workbook title, locale, and timezone are
canonicalized before source replacement, while empty workbook metadata aborts
without replacing the current workbook. Google Sheets export now runs a
source-before-serialization guard in Rust and in the browser mock for workbook
metadata, sheet IDs and titles, row/column axes, named ranges, merges, filters,
protected ranges, cell addresses, formulas, boolean and numeric
source/projection values, formats, validations, dependencies, and cell
comments; covered by
`google_sheets_export_validates_workbook_source_before_serializing` and
`mock-contract`, which exercises the normal export path after import. Google
Docs-shaped export now runs the same source-before-serialization
guard in Rust and in the browser mock for document metadata, stable block and
inline IDs, table row/cell IDs, footnotes, citation database records, review
metadata including comment anchors, suggestion variant shape, provenance,
warnings, image hashes, links, mentions, marks, and equation source; covered by
`google_docs_export_validates_source_before_serializing`,
`malformed_structured_payload_export_aborts_instead_of_emitting_invalid_extensions`,
and `mock-contract`. The app-level regression
`high_risk_google_sheets_imports_do_not_replace_current_workbook` and the
browser mock contract prove unsupported high-risk top-level, sheet-level, and
cell-level Google Sheets structures abort without replacing the current
workbook; the covered rejection list includes top-level charts, pivot tables,
and data-source sheet properties, sheet-level charts, pivot tables, and filter
views, and cell-level pivot tables, data-source tables, data-source formulas,
chip runs, hyperlinks, and rich text format runs. The app-level regression
`malformed_google_sheets_cell_payloads_do_not_replace_current_workbook` and the
browser mock contract prove malformed formula sources, malformed cell value
kinds, and duplicate imported cell comments also abort before replacing the
current workbook; the same test and contract cover malformed imported cell
format and validation payloads. The
app-level regression
`malformed_google_sheets_filter_payloads_do_not_replace_current_workbook` and
the browser mock contract prove missing basic-filter range objects and
malformed imported filter criteria and sort specs abort before replacing the
current workbook.
`malformed_google_sheets_collection_shapes_do_not_replace_current_workbook` and
the browser mock contract prove malformed Google Sheets collection fields
(`sheets`, sheet `data`, nested `rowData`/`values`, `merges`,
`protectedRanges`, and top-level `namedRanges`) abort atomically instead of
being treated as absent. The cell-payload regression and browser mock contract
also prove overlapping imported grid ranges with duplicate cell addresses abort
before replacing the current workbook. The range-import regression and browser
mock contract also cover malformed protected-range descriptions and warning
flags.
Practical `.docx` import now has a Rust-native XML projection before external
converter fallback: headings, list paragraphs, text marks, tabs, and line
breaks, standalone page breaks, and simple tables are imported into source
blocks/inlines without requiring `zip`, `unzip`, Pandoc, or LibreOffice for raw
document XML fixtures. Raw DOCX XML hyperlink anchors import as source link
nodes, `w:vertAlign` superscript/subscript imports as baseline marks, simple
inline Office Math `<m:oMath>` runs with `<m:t>` text import as OpenDoc inline
equation source nodes, standalone display `<m:oMathPara>` paragraphs import as
OpenDoc equation blocks, simple `<w:tbl>` rows/cells import as OpenDoc table
rows/cells with nested paragraph content and editable empty-cell placeholders,
standalone `<w:br w:type="page"/>` paragraphs import as OpenDoc page-break
blocks while ordinary line breaks remain paragraph text, and raw DOCX source
detection accepts equation-only and page-break-only documents without requiring
ordinary `<w:t>` text runs. It also accepts table-only documents as editable
table placeholders and raw drawing-only documents as missing-image placeholders
when no package relationship media can be read. Zipped DOCX imports read
`word/_rels/document.xml.rels` so relationship-backed external hyperlinks
retain their target URL and inline marks. Standalone DOCX drawing paragraphs
with bundled image relationships import as OpenDoc image blocks whose blob
hashes are computed from the exact package media bytes, with imported blob
metadata and bytes returned as content-addressed sidecars instead of fake hashes
or missing placeholders. DOCX image source alt text is imported from drawing
metadata such as `wp:docPr descr` before falling back to the media filename, so
accessibility text remains source state while blob identity remains exact-byte
content addressed. Standalone DOCX image paragraphs with missing or unreadable
media relationships now degrade to editable placeholder paragraphs with
`missing-docx-image-blob` warnings instead of silently dropping the source node
or inventing blob hashes. The focused tests
`imports_docx_xml_source_with_structure_and_marks_without_external_converter`,
`imports_docx_office_math_as_equation_source_without_external_converter`, and
`imports_docx_tables_as_source_rows_cells_and_nested_blocks`,
`imports_docx_standalone_page_break_as_page_break_block`,
`imports_raw_docx_xml_equation_only_source_without_text_runs`,
`imports_raw_docx_xml_page_break_only_source_without_text_runs`,
`imports_raw_docx_xml_table_only_source_without_text_runs`,
`imports_raw_docx_xml_drawing_only_as_missing_image_placeholder`,
`imports_docx_standalone_image_as_content_addressed_blob`, plus
`missing_docx_image_media_imports_placeholder_with_warning`, plus
`imports_docx_via_available_converter`, prove the no-external-tool and
zipped-DOCX paths, including hyperlink, table, page-break, image blob,
missing-image degradation, and inline/block equation-source preservation. The app-level regression
`docx_office_math_import_is_operation_backed_app_source` proves DOCX Office
Math equation source also survives app import, local repository save/open, and
the import operation journal; `raw_docx_equation_only_import_is_operation_backed_app_source`
proves the same for equation-only raw DOCX XML without ordinary text runs. The app-level regression
`docx_table_import_is_operation_backed_app_source` proves DOCX table rows,
cells, nested blocks, inline equations, and empty-cell placeholders also survive
app import, local repository save/open, and the import operation journal. The
app-level regression
`docx_page_break_import_is_operation_backed_app_source` proves DOCX standalone
page breaks also survive app import, local repository save/open, and the import
operation journal; `raw_docx_page_break_only_import_is_operation_backed_app_source`
proves the same for page-break-only raw DOCX XML. The app-level regression
`docx_baseline_mark_import_is_operation_backed_app_source` proves DOCX
superscript/subscript baseline marks also survive app import, local repository
save/open, and the import operation journal. The app-level regression
`docx_image_import_keeps_content_addressed_blob_bytes` proves DOCX image media
bytes are retained as available content-addressed app blobs through import,
local repository save/open, and the import operation journal. The app-level
regression `docx_missing_image_media_import_warns_and_persists_placeholder`
proves missing DOCX image media degrades to a warning-backed editable
placeholder that persists through local repository save/open and the import
operation journal. The app-level regressions
`raw_docx_table_only_import_is_operation_backed_app_source` and
`raw_docx_drawing_only_import_warns_and_persists_placeholder` prove raw
table-only and raw drawing-only DOCX XML imports also survive app import, local
repository save/open, warnings, and the import operation journal. Legacy `.doc`
import stays converter-backed for real OLE/RTF inputs, but mislabeled arbitrary
`.doc` payloads now abort before converter fallback and before app source
replacement; `mislabeled_legacy_doc_payload_aborts_before_converter_fallback`
and
`unsupported_legacy_doc_import_aborts_without_replacing_current_document` prove
the import adapter and app API wrapper both fail atomically. The app-level regression
`google_docs_app_export_import_preserves_comments_and_suggestions` proves
Google Docs-shaped export/import preserves comment threads and suggestion
source state through repository save/open. The app-level regression
`google_docs_import_warns_or_aborts_without_partial_state_replacement` proves
Google Docs-shaped recoverable gaps import with deterministic warnings while
high-risk unsupported elements abort without partially replacing the currently
open document. The app-level regression
`malformed_google_docs_citation_metadata_does_not_replace_current_document`
proves malformed document-local citation extension data, including malformed
citation maps, reference/group collections, reference/group/item objects,
revision fields, summaries, authors, booleans, placement fields, empty
source/group payloads, duplicate bibliography reference IDs, and duplicate
citation group IDs, aborts before replacing the current app source state. The
app-level regression
`malformed_google_docs_review_metadata_does_not_replace_current_document`
proves malformed comment/suggestion collections, comment threads, comments,
timestamps, booleans, authors, anchors, suggestions, provenance entries,
suggestion kind/state fields, duplicate comment-thread IDs, and unsupported
suggestion kinds also abort before replacing the current app source state. The
app-level regression
`malformed_google_docs_inline_extension_ids_do_not_replace_current_document`
proves present-but-empty Google Docs-shaped mention, equation, and image
extension IDs, non-string citation/mention extension payloads, non-string
equation source payloads, and unsupported equation source formats abort before
the importer can synthesize replacement source identities. The app-level
regression
`malformed_google_docs_footnote_metadata_does_not_replace_current_document`
proves malformed footnote maps, footnote objects, footnote IDs, footnote
content arrays, footnote content elements, unsupported footnote content, and
duplicate Google Docs footnote IDs abort before imported footnote anchors
become ambiguous. The app-level regression
`malformed_google_docs_paragraph_metadata_does_not_replace_current_document`
proves malformed Google Docs paragraph style, malformed element containers,
malformed text styles and link metadata, unsupported high-risk paragraph
elements, bullet, list, and mixed page-break structures abort before replacing
the current app source state. The lower-level import regression
`rejects_malformed_google_docs_links_without_plain_text_downgrade` proves
malformed link metadata is not silently downgraded to plain text. The app-level
regression
`malformed_google_docs_structural_metadata_does_not_replace_current_document`
proves missing or malformed body content arrays, malformed structural
elements, unknown high-risk structural elements, malformed tables, malformed
table rows/cells, and nested tables abort before replacing the current app
source state. The
browser mock command contract mirrors those boundaries by aborting Google Docs
inline-object, typed-blob mismatch, malformed citation, duplicate reference,
duplicate citation-group, duplicate comment, malformed comment/suggestion
metadata, and malformed suggestion imports plus malformed
mention/equation/image extension IDs and payloads, unsupported equation source
formats, duplicate footnote IDs, malformed text-style/link metadata, malformed citation metadata, malformed
footnote metadata, malformed paragraph metadata, malformed structural/table
metadata, unsupported paragraph elements, and mixed page-break paragraphs while
verifying the source projection is unchanged after each failed command.

## Frontend And App Workstream

The first serious UI target is a Tauri app with a hybrid TypeScript editor.
This workstream is now an early product priority, not a late polish pass. The
prototype must look and behave close enough to Google Docs and Google Sheets
that normal editing workflows can be tested in the GUI.

Required:

- A minimal first page/home view that lists openable/recent documents and offers
  clear actions to create a new document, create a new spreadsheet, open a
  repository/document, and import files. The app must not require a blank
  document to exist just because the GUI launched. Opening the GUI must show
  this first page until the user opens, creates, or imports a document.
- Two primary editor modes after open/create/import: a Docs-like document editor
  and a Sheets-like workbook editor. These editor screens must be usable
  workspaces, not prototype dashboards or marketing pages.
- Shared app chrome with document title, save/autosave state, signature/trust
  state, runtime/storage state, open/recent controls, import/export, verify, and
  audit/recovery entry points.
- Docs-like surface: paged white document canvas on a neutral workspace, compact
  menu/toolbar rows, ruler or equivalent layout controls, selection-aware rich
  text controls, and panels for outline, comments, suggestions, citations,
  warnings, signatures, and recovery.
- Sheets-like surface: workbook tabs, formula bar, name box, row/column headers,
  selected cell/range affordances, formatting toolbar, frozen-pane markers,
  filter controls, named ranges, validation/comment indicators, and warning
  badges. It must not remain a generic table/card prototype.
- GUI controls for every v0 document and spreadsheet feature, including
  comments, suggestions, citations, equations, tables, spreadsheets, images,
  attachments, signing state, warnings, and audit/recovery.
- Every visible control must dispatch or inspect real app API operations/state.
  Demo-only GUI affordances do not count toward completion.
- Immediate local rendering of operations.
- IME-safe text input and stable selection mapping.
- Paste normalization, with structured paste/import later.
- Undo/redo backed by operations.
- Save, autosave, close, reopen, import, export, verify, warning, and audit/recovery views.
- Linux/macOS/Windows native prerequisites documented.
- Native build checks include required assets such as icons.
- GUI smoke checks must assert the launch/home view and first-viewport editor
  layouts: recent/openable document list, create/open/import actions, Docs top
  chrome, toolbar/menu controls, editable document canvas, Sheets grid, visible
  selection state, status indicators, and no overlapping text.
- Paragraph layout is a product gate, not polish: paragraphs, headings, and
  list items must render as full-width writing blocks inside the usable page
  text column, with margins matching the page model, correct wrapping, visible
  caret placement anywhere in the line box, and review/comment gutters outside
  the writing area. A paragraph that behaves like a narrow inline widget is a
  failing Docs-equivalence result even if text operations technically work.

Done when the packaged app can create, edit, save, close, reopen, import,
export, verify, and audit documents containing every v0 schema feature through
Docs-like and Sheets-like GUI workflows, with the same app API reused by
browser-local, HPC single-user, and multi-user service modes.

Current implementation evidence: the Tauri/browser GUI exposes runtime status,
service-layer sharing, service sync relay classification, and runtime
UUID/DOI lookup in the same session panel. The GUI smoke test runs in
multi-user service mode, creates a share invite, invokes `relay_runtime_sync`,
verifies the visible accepted/deferred/rejected operation counts, and resolves
a DOI through `resolve_runtime_document_lookup` with a service-index result.
The same smoke test saves a local repository, edits after the save, triggers
`autosave_current_repository` through the GUI, closes the document, reopens it,
and verifies the autosaved edit is present. It also creates a different
document and verifies the recent-document list exposes the autosaved document
with repository root, backend, DOI, and stored manifest metadata needed for a
deterministic reopen. Verified by
`npm --prefix apps/desktop run gui-smoke`. The same GUI smoke covers the
attachment `Shallow` action, verifying that a current attachment remains listed
with missing-byte placeholder state after shallow-clone simulation. It also
exercises plain-text paste normalization and both keyboard and visible-toolbar
undo/redo paths over operation-backed document edits. Focused app API tests
`block_level_schema_nodes_are_projected_and_persisted` and
`inline_equation_source_update_is_operation_backed_and_persists` prove block
and inline equation source edits are operation-backed, persist through
repository save/open, and clear version signatures.
The shared contenteditable binding now defers document title, rich text,
equation, comment, suggestion, footnote, image alt text, sheet title,
spreadsheet cell, and spreadsheet cell-comment commits while IME composition is
active, then commits the operation after `compositionend`; the GUI smoke test
simulates this on the editable document title and proves blur during
composition does not prematurely mutate app state.
Block-level document keyboard editing now follows the same source-safety rule:
composition input is treated as local browser state until `compositionend`,
then one operation-backed text update is committed through the keyboard block
mapper. The GUI smoke test proves an in-progress composed paragraph does not
mutate rendered app state before composition ends and then commits normally.
Document block `beforeinput` now also handles browser-native
`insertParagraph` and `insertLineBreak` events through the same operation-backed
paragraph split/list continuation/soft-break paths as keydown Enter and
Shift+Enter, so ordinary typing paths from modern browsers and mobile-style
inputs do not bypass source operations; GUI smoke and visual-contract checks
cover both input types.
Document paste in keyboard-editable blocks now respects the active caret or
selected text range instead of appending blindly: single-line paste replaces the
range in the current source inline, multi-line paste updates the current block
and creates following operation-backed paragraphs, and browser-native
`beforeinput insertFromPaste` plus `deleteByCut` route through the same source
update path. GUI smoke covers selected-range paste, multi-line paste, and
`insertFromPaste`.
Browser-native rich editing `beforeinput` format events for bold, italic,
underline, strike, superscript, and subscript now dispatch the same
operation-backed text mark toggles as keyboard shortcuts and toolbar buttons,
after first committing any pending block text draft. GUI smoke covers
`formatBold`, and the visual contract locks the broader input-type mapping.
The GUI also normalizes toolbar selection before every render: selected inline
IDs must still resolve to live editable text/link/mention/equation source
nodes, selected source is rendered with `data-selected-inline`, and stale
selection is cleared after deletion or source replacement. The GUI smoke test
proves mark commands preserve selected-inline projection across re-render and
inline deletion clears the stale selection.
The app now launches into a minimal home/document picker instead of a blank
document. The first page exposes document/spreadsheet template tiles, repository
open, Google Docs/Sheets import, Word import, explicit open-current when a
document is loaded, and a recent/openable document list with repository,
identifier, and manifest metadata. Secondary home picker commands for DOI
open, flat/OpenDAL open, and repository scan now render as compact icon buttons
with accessible labels, keeping the first page closer to product chrome instead
of a form-heavy prototype. The File menu mirrors creation for both text
documents and spreadsheets, so workbook creation is not stranded on the launch
page; it also exposes Google Docs JSON, Word, and Google Sheets JSON import
routes beside export, so import/export workflows are available from normal app
chrome after a document is open. The editor view is separate from launch
state and exposes a Docs-like white page canvas plus a Sheets-like workbook
surface with formula bar, name box, sheet tab, row/column headers, and grid. It also has separate Docs and Sheets
horizontal toolbars with operation-backed controls for undo/redo, formatting,
comments, suggestions, citations, equations, tables, images, sheet edits,
formula/range workflows, freezes, merges, filters, and warning-only protected
ranges. Workbook title, locale, and timezone are shown as header metadata, while
editing that metadata now lives behind the Sheet menu instead of persistent
input fields and a text button inside the grid header. The editor now folds the Document/Spreadsheet mode switch, application
menus, Find, status summary, and Home/Share/Panel/Close controls into one
compact menu chrome row instead of separate title, tab, find, and quick-action
rows; the main surface renders the active Docs or Sheets workspace instead of
showing both as one long prototype page, and command routing keeps document
operations in the Docs view and spreadsheet operations in the Sheets view.
Document insertion/review paths for links, mentions, equations, table
row/cell text, comments, and suggestions now prompt for user content with
deterministic defaults from menu, context, keyboard, or slash workflows instead
of being locked to canned demo strings or persistent toolbox buttons, while
still committing through the same operation-backed app API. Paragraph, heading,
and list structure changes are now selection-oriented: the paragraph-style
dropdown uses `set_block_text_style` to convert compatible selected text blocks
without changing their stable block or inline IDs, and still uses
`update_heading_level`/`update_list_item` for same-kind updates. The temporary left
command rail has been removed: broad file/edit/insert/format/sheet/storage/view/review
commands now live in the application menu, and active mode-specific document
and spreadsheet commands live in the horizontal toolbar. The application menu
now uses named File, Edit, Insert, Format, Sheet, Storage, View, and Review
dropdown groups instead of a long inline command strip. The Storage menu
exposes existing operation-backed repository/object-store controls for local,
flat, and OpenDAL saves, candidate merges, compaction, scan/open by DOI, and
shallow-clone simulation without requiring the advanced panel to stay visible.
The GUI smoke test proves former
left-rail command IDs are still reachable through menu/toolbar markup and that
the old `data-tool-rail-mode` surface is absent; the visual contract requires
the grouped menu shape. Menu groups are controlled so opening one group closes
the others, and choosing a menu command closes the active menu. The Find control
now lives at the right side of the application menu row with explicit Close and
Find reopen actions, removing the separate find row from the quick-status
chrome; `Ctrl`/`Cmd+F` opens the same projection-only Find control in the active
Docs or Sheets mode and is covered by desktop GUI smoke. Spreadsheet Find
`Next` and `Clear` controls now use explicit chevron and
eraser icons through the shared action-icon registry instead of text-derived
fallback badges, keeping the right-side menu chrome compact.
The repeated
quick-status action row and standalone editor topbar have
also been removed: Home, Share, Panels, and Close remain in the compact menu
chrome for navigation, collaboration, and window control, while save, autosave,
verify, audit, storage, and repository workflows live in the application menu
or advanced panel. Home returns to the document picker without closing the
current document; Close dispatches the source-level close command and returns
to the picker with the document closed. The editor and home topbars no longer
repeat the `OpenDoc` product label; the editable title and document list title
now own that space. Save/signature/repository state now appears as a compact
status summary with runtime, locale, operation count, manifest, permission, and
storage-backend details behind an on-demand status dropdown. Repository,
sharing, and signing controls are hidden by default and opened explicitly
through Storage chrome, with a close action inside the panel, so they do not
consume editor-canvas vertical space until needed.
Runtime capability warnings, such as missing local/OpenDAL repositories or
disabled private-key signing, now render only as compact landing-page footer
notes. They remain visible before a document is opened but no longer consume
editor-canvas vertical space.
The home picker now renders a single runtime-warning block, uses the shared
icon registry for document/spreadsheet/import template tiles, and uses the
same icon vocabulary for recent/openable document rows. The desktop GUI smoke
test asserts the single warning block plus home/recent icon markers, DOI-aware
recent open labels, and `data-recent-identifier` markers. The visual contract
pins the home tile, recent-file icon layout, and recent identifier helper.
The first-page manual open row exposes compact icon actions for local UUID
open, local DOI lookup, flat namespace open, OpenDAL filesystem open, and local
repository scanning, so no blank editor needs to be opened before locating an
existing document. These first-page storage actions mirror runtime capability
state with disabled buttons for unavailable backends, so users see missing
local/flat/OpenDAL support before clicking. The larger home repository tile
uses the same disabled state when local object repositories are unavailable,
and the shared action binder suppresses disabled or `aria-disabled` actions so
capability-disabled first-page controls cannot fire stale commands. GUI smoke
now asserts that disabled menu actions cancel the originating click event and
leave the open menu state intact instead of leaking a fallback command.
Right-side panel chrome now follows the same icon-first rule as the toolbar:
the panel Close control and comments/suggestions/citations/attachments/
warnings/signatures/audit tabs render as compact icon buttons with accessible
labels and live badges. The desktop GUI smoke test asserts the side-tab icon
markers and accessible labels, and the visual contract pins the compact close
button and side-tab icon sizing. Document-canvas review markers now follow the
same compact convention: block comment/suggestion markers and inline suggestion
markers render icon-plus-count pills instead of raw count buttons, with smoke
and visual-contract coverage for the icon markers and count spans. Zero-count
review marker pills are not rendered; the gutter only shows active review
targets.
Spreadsheet grid cells now render display-first: inactive cells show literal
values or formula results, and selected cells remain display-only until an
explicit edit action starts. Double-click, F2, or typing a printable key enters
cell edit mode with an explicit editing marker and text cursor; Enter, Tab, and
arrow keys remain navigation actions, while Shift+Arrow extends the
projection-only selected range. Display-mode selected cells also support
Ctrl/Cmd+B, Ctrl/Cmd+I, Delete, and Backspace through operation-backed format
or clear-cell commands without entering edit mode. Cell context menus expose
the same explicit edit and clear-value actions, so mouse and keyboard context
workflows can enter edit mode or clear content without turning the grid into
generic form fields. Bold/italic formatting now respects the selected range:
toolbar, context-menu, and keyboard shortcut paths fan out through the existing
single-cell format operation for each selected cell and toggle off only when all
selected cells already have the mark; the toolbar pressed state follows the
same all-selected-cells semantic instead of tracking only the active cell.
GUI smoke now asserts that both Enter-based and double-click-based edit entry
restore the caret to the end of the source value, matching the F2 formula-source
coverage already required for formula cells.
Spreadsheet toolbar formatting now preserves active editing state: cell-format
operations capture the focused in-cell editor or formula bar draft plus caret
before rerender, and toolbar toggle mouse-down prevents focus theft, so
formatting a cell does not discard uncommitted typed/formula text.
Active in-cell editing also supports Escape cancellation,
returning to display mode without writing the abandoned text; printable-key
entry seeds the active editor locally and commits only on blur/Enter/Tab rather
than writing a partial cell value immediately. Formula source remains editable
in the formula bar and active cell editor, and formula results are exposed
as projection attributes for tests without adding source/result badges inside
the grid cell. The
desktop GUI smoke verifies display-only
selection, explicit edit entry, shortcut formatting, clear-by-key behavior,
keyboard range extension, Escape cancellation, cancelable printable-key seeding,
literal edits, formula edits, navigation, formula result markers, and
selected-cell formula source markers; the visual contract pins the cell
display/editor styling and explicit edit-mode markers. Spreadsheet row and
column headers now expose their own right-click context menus for axis actions
and row/column clipboard workflows, so users can reach operation-backed add,
delete, copy, cut, and paste paths from the relevant header rather than from a
generic toolbox or unrelated cell. Sheet tabs now expose an operation-backed
right-click menu for adding sheets, deleting the selected sheet, and freezing
panes; the persistent Sheets toolbar no longer carries the destructive
delete-sheet button, while the Sheet menu remains available for menu-driven
access. Row headers, column headers, and sheet tabs also open the same context
menus from `Shift+F10` or the `ContextMenu` key, with GUI smoke covering both
mouse and keyboard entry points.
Document text blocks now expose a native contenteditable canvas target with a
visible focus/caret affordance plus textbox and multiline accessibility
semantics, and direct inline editors use the same textbox/multiline projection
for source text that can contain soft line breaks. Browser `beforeinput` events for ordinary text
insertion plus Backspace/Delete now commit keypress-level inline-text operations
and restore the caret after the operation-backed render; selected text inside a
focused block uses the same mapper, so typed replacement and Backspace/Delete
over a selected range rewrite source text through `update_inline_text` while
restoring the projected caret to the inserted-text end or selection start. The
existing `input` path remains as a browser fallback and IME completion path.
Enter routes through paragraph-insert commands, so normal typing, deletion, and
paragraph creation no longer require toolbar-only prompt paths for the first
basic workflow. Newly inserted empty keyboard-editable blocks now expose a
`data-doc-empty-block` marker plus a stable minimum-height caret target, so the
blank paragraph remains visibly editable in the document canvas without adding
block toolbar buttons. Clicking blank space on the document page now focuses the
selected editable block, or the last editable block, at its text end via the
same projected caret path, making the page itself behave like a normal document
editing surface rather than a collection of tiny controls. This is a first
slice: richer selection geometry and full
IME-grade composition-range handling remains part of the keyboard-editing
priority, but block-level composition no longer writes partial source
operations before the user confirms text.
Imported paragraphs with multiple editable text/link/mention/equation runs now
keep their existing inline IDs and formatting lanes when ordinary block-level
typing rewrites the visible text: the keyboard mapper preserves existing run
lengths and places overflow in the final editable run, while structured
non-editable labels still fall back to the conservative single-inline path. The
desktop GUI smoke test imports a two-run Google Docs paragraph and proves
block-level typing updates both runs without collapsing the bold run into a
single stale inline. The desktop GUI smoke test edits document block
text through explicit keypress-level `beforeinput` insertion/deletion and the
native input fallback path, verifies focus and caret restoration after typed and
deleted text, applies Ctrl/Cmd+B/I/U formatting shortcuts through
operation-backed text marks, pastes multiline plain text into the canvas as
current-block text plus new paragraphs, creates a paragraph via Enter, and uses
typed `# `, `- `, and `1. ` prefixes to convert the current paragraph into a
heading, unordered list item, or ordered list item without exposing block
creation buttons.
Direct inline editors also support Escape cancellation without writing
abandoned text, with focus restored to the same inline after rerender. They
also support operation-backed Ctrl/Cmd+B, Ctrl/Cmd+I, Ctrl/Cmd+U,
Ctrl/Cmd+Shift+X or Alt+Shift+5 for strikethrough, Ctrl/Cmd+. for superscript,
and Ctrl/Cmd+, for subscript, with focus restored to the active inline. Direct
text-run editing now has the same basic keyboard formatting path as block-level
editing. Direct inline `beforeinput` now also commits ordinary typed text,
Backspace/Delete, and selected-range replacement/deletion immediately through
the same inline update operations, restoring focus and caret projection after
the operation-backed render instead of waiting for blur. Plain printable
`keydown` events in direct inline editors fall back to that same source-backed
insert path when `beforeinput` is missing or unreliable, and plain Backspace/Delete
`keydown` events use the same source-backed deletion helper, matching the
keyboard-editable block fallback. Plain Backspace at the start of a focused
direct inline paragraph now joins with the previous paragraph, and plain Delete
at the end joins with the next paragraph, after committing any visible inline
draft and restoring the document caret to the join boundary; browser-native
direct-inline `beforeinput deleteContentBackward/Forward` follows the same
single-character source-backed deletion semantics inside a run, then follows
the same paragraph-boundary join path only when character deletion has no local
text to remove. Ctrl/Cmd+Backspace and Ctrl/Cmd+Delete in
direct inline editors now also use the same source-backed word-boundary deletion
model as block text. Focusing a direct inline editor now retargets the selected
inline and containing block to the focused run, so toolbar/context actions do
not apply to stale formatted text after keyboard focus moves. Plain Home/End in
direct inline editors now stays in the editor model too, committing any visible
draft text and restoring the caret to the start or end of the focused formatted
run instead of relying on browser-only caret state. Ctrl/Cmd+ArrowLeft/Right in
direct inline editors uses the same word-boundary navigation model, committing
draft text and restoring focus/caret at the computed word boundary. Ctrl/Cmd+Shift+ArrowLeft/Right
projects an explicit word-range selection on the focused direct inline editor
with tracked anchor/focus offsets; Bold/Italic-style text mark commands consume
that range by splitting the focused text inline and marking only the selected
run; toggling a mark off uses the same selected-range path instead of clearing a
stale whole inline. Plain Shift+ArrowLeft/Right now projects the same
anchor/focus selection metadata one character at a time in focused direct inline
editors, so ordinary keyboard selection feeds the same formatting and clipboard
operations without depending on hidden browser selection state. Ctrl/Cmd+A in a
focused direct inline editor now selects that run's editable source text first,
and typing over that selection replaces the run through `update_inline_text`
instead of projecting a stale whole-document inline range. Collapsed-caret
formatting shortcuts in direct inline editors now reuse the block pending-mark
model: Ctrl/Cmd+B at a caret records the containing block offset, leaves the
existing run unmodified, and the next single-line direct-inline insertion creates
a marked run with focus restored to the inserted text, including browsers that
fall back to printable `keydown` without a usable `beforeinput`. Native paste
events and internal Ctrl/Cmd+V in a direct inline editor now check that pending
mark path before plain paste, so copy-then-Bold-then-paste creates marked pasted
text without mutating the base run. Toggling the same collapsed-caret direct
inline mark off now clears that pending state, so the next typed character uses
the normal plain insertion path and does not split out a marked run. Shift+Home/End in
focused direct inline editors now projects selection to the start or end of the
same source run with tracked active edge, matching the other direct-inline
selection gestures. Plain ArrowLeft/Right over a projected direct-inline text
selection now collapses the selection to the left or right edge and clears the
projection metadata, so a normal caret move cannot leave stale formatting or
clipboard selection state behind. Ctrl/Cmd+ArrowLeft/Right over a projected
direct-inline selection now uses the active selection edge as its starting point,
clears the projection, and restores a plain caret at the computed word boundary
after rerender. Browser-native `beforeinput historyUndo` and `historyRedo` in
direct inline editors now prevent DOM-only undo/redo and route through the app
operation history, committing pending inline draft text before undo and restoring
focus to the edited run. Browser-native
`beforeinput formatRemove` also clears marks
from only the selected direct-inline text run through source operations, without
relying on hidden browser selection state. Escape over a direct-inline projected
text selection now clears only the projected selection and restores the caret at
the active edge, leaving source text intact instead of cancelling the whole
inline edit. Typing over a direct-inline projected selection replaces only the
selected text through the source update path, clears stale projection metadata,
and restores the caret after the replacement. Browser-native copy and cut over
that projected direct-inline text selection now use the same source-backed
clipboard model as keyboard-editable paragraph blocks: copy writes the selected
plain text to the runtime clipboard without changing source, while cut writes
the same text and deletes the selected source range through the inline update
operation with caret restoration at the deletion point; Ctrl/Cmd+C, X, and V
route through that same runtime clipboard path when browser clipboard events are
unavailable or delayed. Direct inline IME
composition is held as browser-local draft text until `compositionend`, then
committed through the inline source operation with focus restored, so formatted
text runs do not write partial composition text into history. Direct inline paste is
also operation-backed immediately and respects the active caret or selected text
range, normalizing pasted line endings and restoring focus/caret on the same
inline after rerender. Browser-native `beforeinput insertFromPaste`,
`deleteByCut`, and `deleteByDrag` on direct inline editors use that same source
update path, so mobile/browser editing events remain immediate operations too. Direct inline
editors now also route browser-native word and line deletion events
(`deleteWordBackward`/`deleteWordForward` and soft/hard line deletes) through
operation-backed inline updates with caret restoration, matching the block-level
editing path for formatted runs. Plain Enter from a focused inline, through
either physical `keydown` or browser-native `beforeinput insertParagraph`,
commits any changed inline text and, when the caret offset is known, dispatches
`split_paragraph_at_text_offset` so leading text remains before the break,
trailing text moves into the new paragraph, formatting follows the moved text,
and focus lands at caret offset 0 in the trailing paragraph. If direct inline
source text is selected, Enter first removes the selected source range and then
splits at the selection start, matching normal editor replacement semantics for
both physical `keydown` and `beforeinput` paths. The older
`split_paragraph_at_inline` path remains the graceful fallback when the browser
does not expose a caret offset. For direct inline
editors inside nested table-cell paragraphs, plain Enter now uses the same
operation-backed soft-line-break behavior as Shift+Enter instead of dispatching
the top-level paragraph split command. Browser-native direct-inline
`beforeinput insertParagraph` and `insertLineBreak` now follow those same
operation-backed Enter/soft-break paths, so WebViews that report editing through
`beforeinput` do not get a native-only DOM paragraph or line break. Shift+Enter
and direct-inline `beforeinput insertLineBreak` also replace selected source text
with exactly one soft line break and restore the caret after that newline, so
soft breaks follow normal editor replacement semantics. Direct inline focus also accepts the same Ctrl/Cmd+Alt+0-6 and
Ctrl/Cmd+Shift+7/8 block-style shortcuts as the canvas block, preserving inline
focus after the paragraph/list/heading conversion. The
visual contract pins the block keyboard markers, caret
styling, inline shortcut helper, and inline cancellation marker. Focused
document blocks and direct inline editors now handle Ctrl/Cmd+Z, Ctrl/Cmd+Y,
and Ctrl/Cmd+Shift+Z before browser-native editing can steal the event,
preserving the active block or inline focus across operation-backed undo/redo.
Focused document blocks and direct inline editors also handle Ctrl/Cmd+A as
projection-only whole-document editable-inline selection, and both focused
document blocks and direct inline editors can extend stable-inline ranges one run at a time with
Shift+ArrowLeft/Right. Toolbar formatting then applies through the existing
`add_text_mark_range` operation instead of requiring manual shift-click range
construction. Delete and Backspace on an active stable-inline range now dispatch
`delete_inline` for each selected run, clear the projection selection, and move
focus toward the nearest surviving inline/block, so deletion is also keyboard
editing rather than a toolbox button. Escape on a focused document block clears
that projection-only inline range and restores canvas focus without writing an
operation. Whole-block typing and paste now deliberately target only text/link
runs, leaving structured inline equation and mention labels to their direct
inline editors so block-level text editing cannot accidentally rewrite
structured source labels. Document text blocks and direct text/link inline
editors expose multiline textbox semantics for keyboard editing, while
structured inline controls such as equations keep their context-target roles.
Ctrl/Cmd+Alt+M now works from both a focused document block and a focused
inline text run, opening the comments panel and dispatching the existing
`add_block_comment` operation against the nearest stable block rather than
adding a toolbar-only review path. The direct-inline path preserves inline focus
after rerender so comments can be added while editing formatted text without
falling back to a block toolbox.
Ctrl/Cmd+Alt+S now mirrors that flow for suggestions: focused blocks and
focused inline text runs prompt for suggested text, open the suggestions panel,
and dispatch `add_block_suggestion` against the nearest stable block while
restoring the active block or inline focus after rerender.
Ctrl/Cmd+K now works from both the
keyboard-editable document block and focused inline/link nodes, using the same
operation-backed link insertion and link target update paths as menu/toolbar
commands. Tab and Shift+Tab on focused list items now update list nesting
through the existing `update_list_item` operation, keeping list indentation in
the keyboard editing flow rather than forcing users back to toolbar buttons.
Backspace at the start of a focused list item now follows the same keyboard
model: nested items outdent through `update_list_item`, and top-level list
items convert to normal paragraphs through `set_block_text_style` while
preserving the stable block/inline IDs and caret focus. Focused direct-inline
text runs inside list items use the same start-Backspace behavior for both
physical Backspace and browser-native `beforeinput deleteContentBackward` after
committing any visible inline draft, so formatted list text can be outdented or
converted without switching to the block editor.
Backspace at the start of a non-empty focused heading now also converts the
heading to normal paragraph text through `set_block_text_style`, keeping the
same stable block/inline IDs and focus instead of requiring the paragraph-style
toolbar; focused direct-inline heading runs now share that start-Backspace
conversion path for both physical Backspace and browser-native `beforeinput
deleteContentBackward`. Enter from a focused direct-inline text run inside a top-level heading
now commits the heading draft and inserts a normal paragraph after the heading
for both physical `keydown Enter` and browser-native `beforeinput
insertParagraph`, matching the block-level heading Enter path instead of moving
the heading inline into a split paragraph.
Enter on a non-empty focused list item now inserts the next list item through
`insert_list_item_after` and moves focus to it; Enter on an empty focused list
item exits list mode through `set_block_text_style` while preserving the stable
block and inline IDs. Focused direct-inline text runs inside top-level list
items now follow the same continuation and empty-list exit behavior for both
physical `keydown Enter` and browser-native `beforeinput insertParagraph`, after
committing any visible inline draft.
Backspace on an empty focused paragraph, heading, or list item now deletes that
block through the existing `delete_block` operation when another block remains,
which makes ordinary keyboard editing cover block removal without a visible
block toolbox. Direct-inline Delete on an empty final paragraph is now a
source-controlled graceful no-op for both physical `Delete` and browser-native
`beforeinput deleteContentForward`: the browser default is prevented, the final
editable paragraph remains rendered, and focus/caret return to offset zero.
Enter-created paragraphs now receive focus immediately, and the GUI smoke test
verifies that typing into the newly-created paragraph commits through
`update_inline_text` without returning the user to the previous block.
The Docs toolbar no longer exposes a dedicated Blocks button group, and
paragraph/list/table/image/page-break/block-equation insertion buttons have been
removed from the always-visible Insert toolbar and generic repository toolbox.
The top menu no longer exposes paragraph, heading, list, table, image,
page-break, equation-block, insert-paragraph-after, or delete-block buttons as
general block tooling. Paragraph/block insertion is now treated as a
keyboard-editing workflow; structural table commands remain operation-backed
behind the API for import, paste, and tests, but they are no longer exposed as
global Insert-menu controls or as block context-menu/toolbox buttons.
GUI smoke and visual-contract checks reject the removed Blocks toolbar plus
block-creation toolbar/menu buttons, including the old generic image-block
button and context-menu paragraph insertion. The document canvas no longer
renders a default block-action mini toolbox; block editing should flow through
keyboard input, and block review/structure actions should be reached from
context menus or menu chrome when needed. Image and equation block context
menus no longer expose block-delete controls either; deleting blocks belongs in
keyboard editing and explicit recovery/audit flows, not a visible block
toolbox. Manual inline text insertion is no
longer a top-level Insert-menu item and remains available only as a targeted
text context action; explicit block toolbox buttons are intentionally out of the
main GUI contract:
paragraphs, list continuation, tables, image blocks, equations, and page breaks
are inserted through keyboard editing, slash commands, paste, or contextual
actions instead of a persistent Blocks toolbox. Table row and cell structural
commands remain absent from the persistent toolbar, global Insert menu, and
generic table-block context menu, but are now exposed in the focused
table-cell right-click/keyboard context menu so low-frequency table structure
editing is available where the user is actually working. Nested table-cell
paragraphs now share the same block-level `beforeinput` text editing path as
top-level paragraphs through recursive stable-block lookup, so ordinary typing
inside a table cell commits `update_inline_text`, restores focus/caret, and
does not need row/cell toolbox buttons for basic cell text edits. Tab and
Shift+Tab on a focused table-cell paragraph now commit dirty cell text and move
keyboard focus across the stable table cell order, keeping basic table
navigation in the editor surface instead of a row/cell toolbox. Plain Enter in
nested table-cell paragraphs now degrades to an operation-backed soft line
break in the same cell instead of dispatching invalid top-level paragraph
insertions; full nested paragraph splitting remains a later source-operation
extension. ArrowLeft/ArrowRight at the start/end of a focused table-cell
paragraph now moves to the previous/next cell, and ArrowUp/ArrowDown at the
top/bottom text boundary moves to the same-column cell in the adjacent table
row, preserving normal caret movement while adding spreadsheet-like keyboard
navigation for document tables. Plain-text paste into a focused table-cell
paragraph now uses the cell selection/caret, normalizes CRLF/CR newlines, keeps
multiline pasted text inside the same cell as source newlines, and restores the
cell caret instead of creating top-level document paragraphs. Block-level
formatting shortcuts such as Ctrl/Cmd+B now preserve the caret offset after an
operation-backed rerender, including nested table-cell paragraphs, so inline
mark toggles do not throw editor focus out of the cell. Docs toolbar mark
toggles now also inspect the active stable-inline range before deciding pressed
state or add/remove behavior, so selected text ranges use the same all-selected
mark semantic already used by Sheets range formatting. Partially formatted
stable-inline selections expose a mixed toolbar state, making the visible Docs
toolbar reflect heterogeneous selected text without writing projection state
into the signed document. The selection context now derives its mark summary
from the active inline or stable-inline range instead of stale single-inline
debug data, including mixed binary and value marks as projection-only editor
feedback.
Inline deletion is likewise kept out of the persistent Docs toolbar and remains
available through menu/context routes, so normal writing is keyboard-first.
Link target editing also moved out of the persistent Docs toolbar: selected
links can still be edited through the text context menu, the Format menu, and
Ctrl/Cmd+K, while visual-contract checks reject the old toolbar `Target`
button. Link insertion remains available through the selected-text context
menu, Ctrl/Cmd+K, and `/link`; these paths now use
the operation-backed `insert_link_after` command when a canvas block/inline
anchor exists, so link insertion stays inside the focused paragraph instead of
creating an unrelated link paragraph. The redundant top-level Insert-menu
`Link` item is no longer rendered. Mention and
footnote-reference insertion are no longer top-level Insert menu items; they
remain available through the text context menu and the keyboard canvas slash
palette. Mention and footnote insertion now mirror links with anchored
operation-backed `insert_mention_after` and `insert_footnote_ref_after`
commands, so `/mention` and `/footnote` insert structured inline labels in the
focused paragraph instead of appending standalone paragraphs. The slash palette
inserts inline citation labels, footnote references, mention labels, and links
through `/cite`, `/footnote`, `/mention`, and `/link`, so these inline objects
no longer require leaving the editing surface. Focused document blocks and
direct inline editors also support Ctrl/Cmd+Alt+F for anchored
footnote-reference insertion through `insert_footnote_ref_after`; the block
path restores block focus and the direct-inline path restores inline focus
without creating a new paragraph or reintroducing top-level Insert-menu
footnote buttons. Focused document blocks and direct inline editors also
support Ctrl/Cmd+Alt+@ for anchored mention insertion through
`insert_mention_after`; the shortcut uses the same mention-label prompt as the
context/slash paths, inserts after the active inline, and restores block or
inline focus without creating a new paragraph. Focused document blocks and direct inline editors also
support Ctrl/Cmd+Alt+C for single-reference citation insertion: the shortcut
uses the same citation locator/label/prefix/suffix prompts as the context and
citation-panel paths, dispatches `insert_citation` after the active inline, opens the
citations panel, and restores block or inline focus without inserting a new
paragraph. Grouped citation insertion
remains operation-backed through the Review menu and citation workflows, while
the persistent Docs toolbar stays formatting-only to avoid duplicating
low-frequency citation controls. The sample `add_citation` command remains available to tests/API, but
its top-level Insert-menu item is no longer rendered; citation insertion in the
GUI goes through `insert-citation` from context, reference row, Review, or
slash workflows.
Inline equation insertion is likewise removed from the top-level Insert menu and
kept on the keyboard/slash editing path via `/math <source>` and
Ctrl/Cmd+Alt+E. The shortcut prompts for TeX source, inserts the inline
equation after the active inline through `insert_equation_after`, and restores
block or inline focus without creating a new paragraph, while equation source editing
remains operation-backed in the canvas. Suggestion creation is also kept
in the Edit/Review menus plus text/block context menus instead of the persistent
toolbar, matching the direction that review decisions live in contextual chrome.
Heading demotion and list indentation are also removed from visible menu/toolbar
buttons: heading structure should be changed by the paragraph-style control or
Markdown-like typing, and list nesting should be changed by Tab/Shift+Tab while
editing. Focused document text blocks also support Docs-style keyboard
structure shortcuts: Ctrl/Cmd+Alt+0-6 converts the current block to normal text
or Heading 1-6, and Ctrl/Cmd+Shift+7/8 converts it to numbered or bulleted list
items through the same `set_block_text_style` operation. Empty focused
paragraphs also convert `# ` through `###### ` into Heading 1-6 through
`update_inline_text` plus `set_block_text_style`, clear the typed marker, and
restore editor focus/caret; GUI smoke now pins the Heading 6 case so the whole
Markdown-like range cannot silently collapse back to H1-only behavior. The generic block toolbox must stay absent; block construction and
structure changes should come from keyboard/editor interactions, with only
context-specific menus for non-text objects where keyboard editing is not enough.
There must be no persistent block-insertion buttons in the toolbar, menu chrome,
left rail, or per-block gutter; the old `insert-paragraph-after-block` button
path is removed, while the underlying operation remains available to keyboard
editing, paste, and import flows.
Slash command suggestions are rendered as inert keyboard hints, not clickable
buttons; accepting a block command must flow through the focused editor block
with Enter so the slash palette cannot become a renamed block toolbox. The
slash command model itself is keyed by command names and icon names only, its
accessible label is `Editor commands` rather than `Insert block`, and the
visual contract rejects both clickable slash rows and any future `action:`
fields in `SLASH_COMMANDS`.
Non-text structural document blocks are now keyboard focus targets with visible
focus chrome; pressing Delete or Backspace on a selected page break, image,
equation block, or table dispatches the existing `delete_block` operation and
returns focus to the nearest text block. This keeps block removal available
without reintroducing a Blocks toolbox or per-block delete buttons.
Pressing Enter on a selected non-text structural block now dispatches
`insert_paragraph_after` and focuses the inserted normal paragraph, so users can
continue writing after page breaks, images, equation blocks, and tables directly
from the canvas without using an add-paragraph button.
Typing a printable character on a selected structural block now follows the
same path but seeds the inserted paragraph with that character, and plain-text
paste/drop creates normal following paragraphs for each pasted or dropped line;
browser-native `beforeinput` text, paragraph, paste/drop, and delete events
follow the same operation-backed paths, so selected objects do not trap
ordinary writing input.
ArrowUp/ArrowLeft and ArrowDown/ArrowRight on a selected non-text structural
block now move focus to the nearest editable text block before or after the
object without writing an operation, giving page breaks, images, equation
blocks, and tables normal keyboard navigation inside the document canvas.
Adjacent structural blocks are visited in document order before navigation
falls through to the nearest text block, so keyboard users can move across
consecutive objects without clicking. The inverse navigation is also covered:
ArrowDown/ArrowRight at the end of a top-level editable text block selects the
immediately following structural block, and ArrowUp/ArrowLeft at the start
selects the immediately preceding structural block. Dirty text is committed
before moving focus, so object navigation does not drop keypress-level edits.
Plain ArrowRight/ArrowDown at the end of a top-level editable text block now
also moves to the next editable text block when no structural block is adjacent;
ArrowLeft at the start moves back to the previous editable block. The handler
accepts restored caret projections after rerender, commits visible draft text
before moving, and restores the target caret at the paragraph boundary.
The first structural keyboard command is now operation-backed: typing
`/pagebreak` in a paragraph clears the command text and dispatches
`insert_page_break_after`, which inserts a page-break block after the current
top-level block instead of appending from a hidden toolbar path.
Focused document blocks also support Ctrl/Cmd+Enter for the same operation:
the editor first commits any changed paragraph text through the run-aware block
mapper, inserts a page-break block after the current stable block, and restores
focus to the originating paragraph. GUI smoke covers the path from a multi-run
paragraph with a formatted tail.
This keeps page breaks available through normal keyboard editing while the
persistent block toolbox remains absent.
Heading blocks now handle plain Enter directly in the canvas: the edited heading
text is committed through `update_inline_text`, a normal paragraph is inserted
after the heading with `insert_paragraph_after`, and focus moves to that new
paragraph. Heading continuation therefore does not require an add-paragraph or
block-toolbox button.
The always-visible Docs toolbar is now
kept to history plus text formatting/state controls; block insertion and
document-object insertion buttons must not appear there, because block
construction belongs to keyboard editing, slash commands, context menus, or
dedicated side panels.
Typing `/equation <source>` follows the same pattern through
`insert_equation_block_after`: it clears the paragraph command text and inserts
an editable equation block after the current top-level block with the TeX source
kept as signed document source.
Typing `/table` now dispatches `insert_table_after` and creates the default v0
table after the current top-level block, so table creation is also available
from keyboard editing without a block toolbox button.
Typing `/image` now dispatches `insert_image_block_after` using the first
attached image blob, or the first attached blob if no image media type is
present. Missing attachments fail visibly without creating source state, while
successful insertion stores only the blob hash and alt text in the signed source
document.
The document canvas also renders a projection-only slash command palette beside
the selected editable paragraph when its source text begins with `/`. The
palette exposes Heading 1-3, bulleted list, numbered list, table, image,
page-break, and equation-block commands, filters by typed command names and
aliases such as `/h2` and `/number`, and dispatches operation-backed helpers
without contributing to document text, hashes, or signatures. Heading and list
commands clear the slash text and call `set_block_text_style`, preserving the
paragraph block ID and inline anchor; insertion commands use the block insertion
operations described above. ArrowUp and ArrowDown move the active palette item,
Enter accepts it, and Escape cancels the slash command text through the normal
inline text operation. After a slash insertion, focus restoration follows the
nearest editable target: text-like inserted blocks receive keyboard focus,
inserted tables focus their first focusable cell, inserted equation/image
blocks focus their source or alt-text editor, and non-editable page breaks
return focus to the originating paragraph.
Plain paragraph Enter handling now has an operation-backed caret split path:
when the browser exposes a caret character offset, the GUI dispatches
`split_paragraph_at_text_offset`, which updates the leading text in the existing
inline, inserts a new paragraph containing the trailing text, focuses that
trailing paragraph, and restores the caret to offset 0. If no caret offset is
available, the editor keeps the existing graceful fallback of inserting an empty
paragraph after the current block. GUI smoke now covers both end-of-paragraph
Enter and mid-paragraph Enter, including the preserved leading/trailing text
and restored caret on the trailing paragraph.
Shift+Enter on a focused document text block now inserts a soft line break as a
newline in the existing inline text source through the same operation-backed
`update_inline_text` path used by ordinary typing. In formatted multi-run
paragraphs, Shift+Enter maps the block caret to the active inline run first, so
the newline inherits that run's formatting instead of redistributing the whole
paragraph. Rendering projects source
newlines with `data-soft-line-break` markers and `white-space: pre-wrap`, so
soft breaks remain visible without adding a block node or reintroducing block
toolbox controls. Direct inline editors follow the same rule: Shift+Enter
commits a newline into the active inline and restores focus to that inline,
while plain Enter remains the paragraph split command.
Backspace at the start of a plain paragraph now dispatches
`join_paragraph_with_previous` when the preceding top-level block is also a
paragraph, then returns keyboard focus to that previous paragraph at the former
end of its text. This keeps paragraph joining in ordinary keyboard editing while
preserving automatic graceful no-op behavior when the previous block is not
joinable.
Delete at the end of a plain paragraph now handles the forward join case with
the same operation model: the editor dispatches `join_paragraph_with_previous`
for the next top-level paragraph, keeps keyboard focus on the original
paragraph, and restores the caret to the former end of the original text. GUI
smoke covers this Delete-join path next to the Backspace-join path.
Specialized review commands such as delete-suggestion and format-suggestion are
also menu/context workflows rather than always-visible toolbar buttons.
Comment rows no longer expose persistent reply, resolve, or delete buttons, and
suggestion rows no longer expose persistent accept/reject button pairs. These
review decisions are kept in the right-click context menu so the side panels
behave more like selectable review lists while preserving operation-backed
reply, resolve, accept, reject, and delete commands.
Citation side-panel references and citation-group rows now use right-click
context menus for destructive deletion actions; reference rows keep the common
`Cite` action visible, but deletion is no longer another persistent row button.
Attachment panel blob rows now expose a right-click attachment context menu for
metadata edit, archive tombstone recording, and deletion, leaving the row itself
as the selectable target instead of another dense edit/archive/delete button
cluster. Signing/profile actions remain visible for the audit workflow in this
slice, while the context-menu contract pins the attachment menu entry points.
The first right-click context menu is implemented as transient editor chrome.
Right-clicking document text opens inline actions for bold/italic/underline,
comments, suggestions, citations, links, inline insertion, and deletion;
structured citation labels and footnote references have separate context menus
for citation-panel navigation, citation deletion, footnote citation insertion,
comments, and reference deletion; inline equations have a math-specific context
menu for equation-adjacent review/deletion actions;
right-clicking document blocks opens review/object actions; generic table-block
context menus avoid row/cell structure buttons, while focused table-cell
context menus expose insert/delete row and insert/delete cell actions against
the selected row/cell; block equations expose comment/suggestion actions, and image
blocks expose blob replacement plus archive-tombstone actions without block
deletion buttons or a persistent Replace button in the document canvas. Block
equation source and image alt text now also handle ordinary `beforeinput`
insertion, deletion, paste/drop, selected-range replacement, and actual browser
text drops through their existing operation-backed update commands, restoring
focus and caret projection after rerender instead of waiting for blur.
Focused keyboard-editable paragraphs now do the same for actual text drops:
the drop point is mapped to the paragraph source offset before insertion, and
multi-line drops reuse the paragraph-splitting paste path so the first dropped
line stays in the current paragraph while the original suffix moves to the
final inserted paragraph.
Footnote bodies, comment text, and suggestion text now also accept actual
browser text drops through the same operation-backed update commands used by
their `beforeinput` editing paths, so side-panel review and citation text no
longer depends on native DOM-only drop mutation.
Right-clicking spreadsheet cells
opens explicit cell edit/clear actions, sheet formatting, text/fill color,
row/column, cell comment, validation, and range actions with target-specific
IDs. Comment and suggestion rows in the
side panel now expose review context menus for reply, resolve, delete, accept,
and reject actions. Comment, footnote, and editable insert-suggestion body edits
now also handle ordinary `beforeinput` replacement/deletion through the existing
`update_comment`, `update_footnote_body`, and `update_suggestion` operations,
retaining side-panel focus and caret projection after rerender while preserving
the blur/IME completion fallback. Context menu items reuse the same
operation-backed `data-action` handlers and icon labels as menu/toolbar
controls, close with outside click, Escape, or action dispatch, and are covered
by desktop GUI smoke plus visual-contract checks. Editable document blocks,
editable inlines, table cells, and spreadsheet cells also open their target-specific context
menus from the keyboard via `Shift+F10` or the `ContextMenu` key, with GUI
smoke coverage for document, text, table, and spreadsheet targets. Comment rows,
suggestion rows, bibliography references, citation groups, inline citation
labels, inline equation nodes, footnote references, attachment rows, named
ranges, protected ranges, merged ranges, and cell-comment rows are also
focusable context targets with the same keyboard context-menu entry points, so
contextual workflows are not mouse-only.
The Docs toolbar now treats binary marks as selected-state toggle buttons:
bold, italic, underline, strike, code, superscript, and subscript all route
through the same operation-backed toggle path instead of one-way mark buttons.
The paragraph-style selector now reports `Mixed` for stable-inline selections
that span differently styled blocks, so cross-block selection feedback is
truthful without adding projection state to signed source.
The Format menu now follows the same consolidation for bold: `Bold` toggles the
mark on or off, and the redundant `Clear bold` menu item is no longer rendered
as a separate remove-style control.
Text color and highlight toolbar controls expose compact swatch palettes, a
custom raw color entry, and clear/uncolor buttons; the palettes now expose the
selected text/cell color as `data-selected-color` and a visible active swatch
when the selected target or range has a uniform color. Swatch clicks route
through the same operation-backed text mark commands as the menu/raw-input path.
The redundant top-level `Clear color` and `Clear highlight` menu items are no
longer rendered; uncoloring remains available directly in the compact palette
and text context menu. Text
and spreadsheet cell context menus now expose palette swatches alongside custom
and clear color actions, so right-click formatting is palette-first without
changing source or signing semantics. GUI smoke verifies toggle-on and
toggle-off behavior plus swatch-driven color/highlight operations, including
context-menu text color and spreadsheet fill swatches, and the visual contract
pins the palette/toggle markers and styling. Docs text color/highlight palettes
also distinguish mixed selected-range values from genuinely uncolored ranges
with projection-only `data-color-state` markers and a compact mixed-color
indicator. Font and size
selects remain in the toolbar and now reflect the selected inline or selected
stable-inline range's `font` and `size` marks instead of always showing
defaults; mixed selected-range values render a disabled `Mixed` option rather
than pretending they are defaults. Selecting a concrete font/size applies that
mark to the full selected range, while selecting the default `Arial` or `14`
option removes the corresponding mark from the full range, so the redundant
top-level `Clear font` and `Clear size` menu items are no longer rendered; GUI
smoke verifies the default-select clear paths and visual-contract checks reject
the removed toolbar/menu buttons. Spreadsheet
cell validation follows the same rule: setting validation remains a selected-cell
toolbar/context action, but clearing validation moved out of the persistent
toolbar and into the cell right-click menu, with GUI smoke proving the
operation-backed clear path and visual-contract checks rejecting the old toolbar
clear button.
Side-panel navigation is now available through a View menu with entries for
comments, suggestions, citations, attachments, warnings, signatures, and audit,
so those workflows are not reachable only through the right-side panel tabs.
The side panel also exposes a compact editable Author identity field; comment,
reply, suggestion, suggestion accept/reject, and spreadsheet cell-note
operations use that value for their author/reviewer metadata instead of a fixed
local-user string.
Comment threads now expose a panel-level Reply action that dispatches
`add_comment_reply`; the Rust merge engine appends stable-ID replies
deterministically and degrades missing/deleted targets to warnings rather than
blocking replay. Live comment threads expose a Google Docs-style Resolve action
that hides the thread from normal views through the existing retained-history
`delete_comment_thread` operation, so audit/recovery can still restore it.
Block-level review is now represented explicitly: gutter comments dispatch
`add_block_comment`, gutter suggestions dispatch `add_block_suggestion`, and
both project as `nearest:<block-id>` anchors instead of relying on stale inline
selection.
Warnings and signatures also have first-class panel pages and collapsed-strip
badges derived from real app/audit state, instead of being visible only inside
repository or audit rollups. The right inspector is
now collapsible: the editor starts with the side panel closed to give the canvas
more width, View menu/topbar panel actions reopen it, panel-specific commands
open it automatically when their results need inspection, and the panel has its
own Close action. When collapsed, a narrow right-side panel strip keeps
comments, suggestions, citations, attachments, warnings, signatures, and audit
discoverable through badge-bearing buttons without restoring the old command
rail. Escape now closes
transient editor chrome: open menus, status details, Find, Storage controls,
and the right inspector collapse through one shared path. The shared Escape
handler treats menu-only and status-only closures as handled events too, so
keyboard users do not leak Escape to browser/default behavior after closing a
single dropdown; GUI smoke asserts `preventDefault()` for menu-only,
status-only, and combined transient-chrome Escape paths.
Document keyboard blocks now expose `data-doc-active-caret` on the selected
canvas block, and CSS renders the same green insertion caret for focused or
selected keyboard-editable blocks. GUI smoke proves operation-backed typing
preserves that active caret marker after rerender, so the Docs canvas has a
visible editing cursor without relying on toolbar paragraph insertion. New
blank documents now seed one empty paragraph as source state and focus it at
caret offset `0`; GUI smoke dispatches text input into that focused empty
paragraph and proves the typed text reaches operation-backed document state,
marks the document unsaved, and restores the caret after rerender. It then
presses Backspace in the only empty paragraph and proves the editor keeps that
editable paragraph alive, focused, and at caret offset `0`, so a blank document
cannot be left without a typing target. Backspace and Delete in this protected
only-empty-paragraph state now prevent native contenteditable deletion and
remain controlled no-op editor events rather than mutating the DOM outside the
operation model. It then uses focused-block Ctrl/Cmd+Z and Ctrl/Cmd+Y after the
first typed text and proves undo/redo restores source text while keeping focus
in the document canvas. It then
selects the word `typed`, toggles Bold from the toolbar, and proves the editor
splits the single text run into operation-backed inline runs, marks only the
selected word, keeps focus in the canvas, and restores the caret at the end of
the selected range. The same operation-backed path now handles selections that
span existing plain-text inline runs: partial boundary runs are split, inserted
pieces inherit their prior marks, and the requested mark is applied to the
selected pieces. Removing a mark from a selected character range uses the same
split-first path, so toggling a mark off does not accidentally clear formatting
outside the selected text. Typing at a caret inside or immediately after an
existing formatted plain-text run now updates that run directly, preserving the
run's marks for the newly inserted characters instead of redistributing the
whole paragraph by stale run lengths. Backspace/Delete caret edits in a
multi-run plain-text paragraph use the same inline-local path, so correcting
text inside formatted runs preserves the formatting lane around the edited
characters. Selected Backspace/Delete/Cut deletion across multiple plain-text
runs now also updates only affected inline texts, keeping unselected surviving
characters in their existing marked or unmarked runs. Typing over a selected
range across multiple plain-text runs uses the same run-aware update path: the
replacement text is inserted into the start-boundary run, so it inherits the
formatting at the replacement point while other affected runs only lose the
selected characters. Single-line paste over a selected range reuses that
replacement path from both native paste and `beforeinput insertFromPaste`, and
single-line paste at a caret inside an existing formatted run updates that run
directly so pasted text inherits the run's marks. Native cut events in
keyboard-editable document blocks now copy the selected plain text to
clipboard state, delete the selected source text through the same
operation-backed `deleteByCut` path, and restore focus/caret at the selection
start instead of relying on browser DOM mutation. Native copy events now use
the same selected-range extraction, writing plain text to clipboard state while
leaving signed source untouched and restoring the canvas caret at the copied
selection boundary. Projection-only stable-inline ranges created with keyboard
selection shortcuts now also feed native copy/cut events: copy extracts the
selected inline texts without writing source operations, while cut writes the
same clipboard text and deletes the selected inlines through deterministic
source operations. Pasting while such a stable-inline range is active now
replaces the range at its first inline with the pasted plain text and removes
the remaining selected inlines, keeping the replacement operation-backed rather
than dependent on contenteditable DOM mutation. Docs blocks now also keep a
runtime-only document clipboard cache and route Ctrl/Cmd+C, Ctrl/Cmd+X, and
Ctrl/Cmd+V through the same copy, cut, and paste helpers, so keyboard shortcuts
remain deterministic even when browser clipboard events or OS clipboard access
are unavailable. Multi-line paste at a caret
inside a formatted multi-run paragraph now writes the first pasted line into
the caret run, clears later source runs from the original paragraph, and moves
the trailing text to the final inserted paragraph; follow-on paragraphs still
use the operation-backed paragraph-creation path. Keyboard caret mapping and
default inline selection now skip empty text runs when a visible text run is
available, so retained empty source runs do not attract later typing or paste
operations before compaction exists. Keyboard and browser-native formatting
shortcuts now resolve the text-mark target from the current block caret before
toggling, so Ctrl/Cmd+B inside a formatted run affects that run instead of a
stale selected inline. Keyboard paragraph-style shortcuts now commit pending
visible text through the run-aware block mapper before applying heading,
paragraph, or list-item style changes, so Alt+number and Shift+7/8 conversions
do not drop uncommitted edits. Focused-block comment and suggestion shortcuts
now prompt first, commit pending canvas text through the same mapper, and then
create the review anchor, so Ctrl/Cmd+Alt+M and Ctrl/Cmd+Alt+S do not lose typed
text when the user starts review directly from the editing canvas. Focused-block
footnote, citation, mention, inline equation, and link shortcuts now follow the
same prompt-then-commit-then-insert flow, so ordinary writing shortcuts can add
inline objects without discarding the paragraph draft that is still only present
in the editable canvas node. These focused-block object shortcuts now resolve
their insertion anchor from the current canvas caret; a caret in the middle of a
plain text run splits that run and inserts the object between the left and right
text fragments rather than after stale selected source, and a caret at the start
of the first run preserves the original text after the inserted object instead
of appending the object at the end. Toolbar and menu/context link, citation,
inline equation, mention, and footnote actions now reuse that focused-canvas
caret preparation path, so mouse-driven insertion does not fall back to stale
selected-inline placement. Keyboard-opened document context menus now use the
same focused-canvas caret path: `ContextMenu`/`Shift+F10` commits the visible
block draft, stores the caret insertion anchor, and inserts mentions or other
inline objects at the writing cursor instead of at stale source selection. The
click path now maps clicks inside keyboard-editable paragraphs to a source text
offset with browser caret APIs when available, so click-to-type can insert in
the middle of a paragraph instead of jumping to the paragraph end.
editor also remembers the last focused canvas
caret while toolbar buttons take focus, so collapsed-caret formatting and
insertion actions can still commit the visible draft and restore the writing
position. Direct inline review and object shortcuts also commit the active
inline edit before dispatching their insert/review operation.
Markdown-style
paragraph shortcuts and slash-object shortcuts now clear every editable text run
in the focused paragraph before changing style or inserting an object, so
shortcut source text cannot survive in stale formatted fragments. Forward
Delete at a formatting boundary now edits the run after the caret, while
Backspace continues to edit the run before the caret, so boundary corrections do
not fall back to whole-paragraph redistribution. Backspace-at-start and
Delete-at-end paragraph joins now commit pending canvas text before dispatching
the join operation, so joining paragraphs cannot discard a draft edit in the
paragraph being merged or the paragraph that remains focused. List-item Tab,
Shift+Tab, and Backspace-at-start structural edits now commit pending canvas or
direct-inline text before changing list depth or converting the item back to a
paragraph, so outline/list editing preserves the text the user just typed.
Heading Backspace-at-start conversion now follows the same commit-before-style
rule before converting the heading to a normal paragraph. The caret-preservation path
accepts the restored caret projection as input for the next operation, and
collapsed-caret toolbar/context mark actions now commit pending canvas text and
retarget from the live caret before adding, removing, or toggling a mark, so
toolbar formatting cannot silently discard text typed into the page. The
paragraph-style toolbar selector now uses the same rule: a focused canvas or
direct-inline draft is committed before paragraph/heading/list conversion, then
focus and caret are restored after the final style operation. Toolbar actions remain stable
after operation-backed rerender. The same
blank-document smoke path presses Enter
after typed text, including after the paragraph has been split into multiple
inline runs by formatting, and proves the editor maps the block caret to the
correct inline-local split point, creates a second keyboard-editable paragraph,
moves focus to it, and restores the caret at the start of the new paragraph
without using prompt-based paragraph insertion. Browser-native `beforeinput
insertParagraph` now uses the same caret-to-inline mapping in formatted
multi-run paragraphs, so mobile/browser paragraph insertion cannot split the
wrong source run. Keydown Enter and browser-native paragraph insertion from
headings now commit edited heading text through the run-aware block mapper
before inserting the following normal paragraph. Keydown Enter and
browser-native paragraph insertion from non-empty list items now commit edited
list text through the same mapper before inserting the continuation item, while
empty-list exit behavior remains unchanged. It also pastes
multi-line plain text into a menu-created blank document and proves the first
line updates the focused empty paragraph while subsequent lines create
additional keyboard-editable paragraphs through the operation-backed paste path.
Docs toolbar mark toggles also preserve the active keyboard block and caret:
toolbar toggle mouse-down prevents focus theft, mark operations capture the
document caret before dispatching source operations, and GUI smoke proves Bold
returns focus to the same canvas block at the same caret offset after rerender.
Collapsed-caret mark toggles now preserve pending typing state: Ctrl/Cmd+B or
toolbar Bold before typing records a caret-local pending mark set, and browser
`beforeinput format*` commands use the same path for collapsed carets.
Consecutive typed characters use operation-backed marked runs, toggling the
mark off keeps the next inserted character plain, and stale canvas focus is
ignored when a mouse-selected inline range is formatted. Plain single-line paste
at the same pending caret now consumes the pending mark set too, so pasted text
follows the same formatting rule as typed text for both native paste events and
the internal Ctrl/Cmd+V fallback, while multi-line paste stays on the existing
paragraph paste path. Valued marks use the same pending-caret model: choosing
text color, highlight, font, or size at a collapsed caret updates the pending
typing style and the toolbar state, then the next inserted run receives the
valued mark without recoloring existing text.
Focused document text blocks now handle Home/End explicitly in the editor model:
plain Home/End commits any visible draft and restores the caret to the start or
end of the current block, while Ctrl/Cmd+Home/End moves to the first or last
keyboard-editable block in document order with focus and caret projected after
rerender. Shift+Home/End also projects a character selection inside the current
block, so toolbar formatting can consume the selected range after rerender and
split/mark source text through normal operations. This keeps document
navigation aligned with operation-backed state rather than depending on
browser-only selection movement. Plain printable `keydown` text now falls back
to the same operation-backed `insertText` path as `beforeinput`, so Tauri
WebViews and test harnesses that do not provide useful `beforeinput` events
still let a user click into the page and type with deterministic caret restore.
Browser-native `beforeinput historyUndo/historyRedo` events now follow the same
model as Ctrl/Cmd+Z and Ctrl/Cmd+Y: the editor prevents native-only DOM undo,
commits visible draft text before undo, and runs redo directly against the
operation redo stack so rendered editable DOM cannot accidentally clear redo
state. Both paths preserve document-canvas focus.
Additional browser/WebView `beforeinput` variants now stay operation-backed:
`insertReplacementText` follows the same path as typed text for autocomplete or
replacement input, and `deleteWordBackward`/`deleteWordForward` use the same
word-boundary deletion semantics as Ctrl/Cmd+Backspace and Ctrl/Cmd+Delete.
`insertTranspose` is also source-backed for keyboard-editable blocks and direct
inline editors, swapping adjacent characters deterministically and restoring the
caret instead of letting the browser mutate contenteditable DOM.
Browser `beforeinput formatRemove` now clears formatting through source
operations as well: selected text is split once into stable inline runs and all
known text marks are removed from the selected runs, while collapsed-caret
pending marks are cleared without mutating existing text. GUI smoke covers
inline, selected-range, and pending-caret clear-format cases.
Browser-native line deletion events are also captured: `deleteSoftLineBackward`,
`deleteSoftLineForward`, `deleteHardLineBackward`, `deleteHardLineForward`, and
`deleteEntireSoftLine` delete source ranges at soft line-break boundaries and
keep focus/caret in the document canvas instead of letting the WebView mutate
contenteditable DOM. The same source-backed line deletion path now runs when the
page canvas holds fallback focus, so clicked full-width paragraphs keep their
soft-line editing semantics even before the inline editable node has native
focus.
Mouse-driven browser editing is covered too: `beforeinput insertFromDrop` now
uses the same plain-text insertion path as paste, and `deleteByDrag` uses the
same selected-range deletion semantics as cut, so dropped or dragged text cannot
escape the operation history. The single-inline paste/drop fallback also uses
the current caret offset instead of appending blindly, so simple paragraphs keep
the same insertion semantics as formatted multi-run paragraphs.
Actual drops on the focused document canvas now project the drop event to a
paragraph source offset before inserting. Multi-line focused-canvas drops reuse
the keyboard paste split path, leaving the first dropped line in the target
paragraph and carrying the original suffix into the final inserted paragraph.
Paragraph-boundary `beforeinput deleteContentBackward/Forward` is now routed
through the same operation-backed behavior as Backspace/Delete keydown: empty
single-block documents stay editable, headings and lists degrade through their
normal start-backspace rules, and adjacent paragraphs join with focus/caret
restored at the merge boundary.
Browser-native list commands are source-backed too: `insertUnorderedList` and
`insertOrderedList` commit visible draft text, convert the active block through
`set_block_text_style`, preserve focus/caret, and toggle matching list items
back to normal paragraph text.
List indentation now behaves like an in-place formatting change during writing:
Tab and Shift+Tab commit visible draft text, update list depth through source
operations, and restore the caret at the prior text offset instead of jumping to
the start of the item. Browser-native `beforeinput formatIndent` and
`formatOutdent` now use the same source-backed list-depth path for both
keyboard-editable blocks and direct inline editors.
The first-page `Blank document` tile is now covered by GUI smoke as a typing
entry point: creation switches to the editor, focuses the empty paragraph,
places the caret at offset zero, and accepts immediate printable-key input
without any toolbar/menu paragraph command. The same smoke path now presses
Enter after that first typed character, verifies that a second editable
paragraph is created and focused at caret offset zero, types again there, and
presses Backspace at the second paragraph start to prove the paragraphs join
back together with focus and caret restored at the former paragraph boundary.
Plain Backspace/Delete `keydown` events inside text now fall back to the same
operation-backed deletion path as `beforeinput`, and the home-created writing
flow smoke proves both directions prevent native-only mutation and restore the
caret after source update. Clicking the row/shell around an editable document
block now focuses the block's text surface at the end of its text, so the page
behaves like a document canvas even when the user misses the exact glyph area.
Plain Shift+ArrowLeft/Right with a live document text caret now projects a
source-aware character range before the older inline-object range shortcut can
run. Repeated Shift+Arrow extends the range from the active caret edge and the
opposite direction shrinks it from that edge; GUI smoke proves toolbar
formatting consumes the resulting single-character selection through the
split-and-mark operation path. The same keyboard-projected character selection
is also covered for direct text replacement: typing a printable key over the
selection prevents native-only mutation, updates source text through operations,
and restores focus/caret after rerender. Backspace and Delete over a
keyboard-projected character selection are covered the same way, deleting the
selected source range through operations and restoring the caret at the deletion
start. Ctrl/Cmd+C and Ctrl/Cmd+X over a keyboard-projected character selection
now have GUI smoke coverage as well: copy populates the document clipboard cache,
cut deletes the selected source range through the same operation path, and
Ctrl/Cmd+V pastes the copied/cut character back into the focused block.
Shift+ArrowUp/Down now extends the stable inline selection in document order,
so keyboard users can select across adjacent editable paragraphs without using
the mouse; GUI smoke proves the cross-paragraph selection renders range markers
and can be copied through the document clipboard path. Toolbar mark toggles now
also work over that keyboard-created cross-paragraph range: mixed/unmarked
ranges are filled, fully marked ranges are cleared, and GUI smoke pins bold
on/off before deletion consumes the selection. Backspace/Delete over
that keyboard-created cross-paragraph range now clears the first selected text
inline as an editable anchor, removes later selected inlines, and restores focus
and caret to the first selected block so writing can continue immediately.
Typing a printable key over the same keyboard-created cross-paragraph selection
now replaces the range through operations, updates the first selected inline
with the typed character, deletes later selected inlines, and restores the caret
after the inserted text. Paste over that stable cross-paragraph range now uses
the same replacement path; multi-line pasted text updates the first selected
inline with the first pasted line, deletes later selected inlines, creates
following paragraphs for later pasted lines, and keeps focus/caret in the first
selected block. Enter over that same stable range now behaves as a paragraph
replacement operation: it clears the first selected editable inline, removes
later selected inlines, inserts a fresh paragraph after the anchor block, and
focuses that paragraph at caret zero. Shift+Enter over the same stable range now
replaces the selected inline text with one soft line break in the anchor inline,
removes later selected inlines, and restores focus after the break without
creating another paragraph. Escape over a keyboard-created cross-paragraph range
now clears
the range without changing source text and restores focus/caret to the
originating editable block, so typing can resume predictably.
Ctrl/Cmd+A from a focused keyboard-editable document block now projects a
full-block text selection instead of falling back to the older document-wide
inline-object range; Escape clears that projected selection back to a caret, and
toolbar mark toggles consume the selected text range through source operations.
Collapsed-caret formatting shortcuts and toolbar toggles now keep an explicit
pending mark state at the caret instead of mutating existing visible text; when
the next character is typed, the editor splits/inserts a marked inline run
through source operations and leaves the earlier text unmodified, so Ctrl/Cmd+B
and toolbar Bold behave like pending bold instead of retroactively bolding the
previous text. Pending caret marks now advance with consecutive typed
characters, so formatting stays active until the user toggles it off or cancels
it by moving/deleting/selecting; toggling the mark off keeps an explicit empty
pending state at the caret, so the next character is inserted as a plain run
instead of being appended to the previous marked run. Docs toolbar toggle state
now reflects pending caret marks before text exists and continues to reflect
that pending mode while consecutive typed characters inherit the mark; GUI smoke
pins this for both browser `beforeinput` and the plain printable-key `keydown`
fallback used by Tauri/WebView runtimes.
Mouse-selected inline
controls now also ignore stale focused
canvas caret state from a prior toolbar interaction, so font/color/range toolbar
operations target the selected inline or selected range rather than an old
writing cursor. Pending caret marks are cleared by navigation, selection,
deletion, Enter, Tab, or other non-text keydown paths, so a user can cancel a
pending Bold/Italic mode by moving the caret before typing and will not get a
stale mark applied later at the old offset.
The same full-block projected selection is now pinned for ordinary writing:
typing a printable key over Ctrl/Cmd+A replaces the block text through
operations, and Backspace over the full-block selection clears the block while
restoring the caret at offset zero. Ctrl/Cmd+A in an already-empty focused
document block now consumes the shortcut locally, keeps focus/caret at offset
zero, and does not fall back to a document-wide inline selection.
Ctrl/Cmd+Backspace and Ctrl/Cmd+Delete in a focused document text block now use
the same word-boundary model as Ctrl/Cmd+Arrow navigation, delete the computed
word range through source operations, normalize the common middle-word
double-space case, and restore focus/caret at the deletion boundary.
Enter over a non-empty projected document text selection now follows normal
editor semantics: the selected source range is deleted first, the paragraph is
split at the selection start through `split_paragraph_at_text_offset`, and focus
moves to the trailing paragraph with the caret restored at offset zero.
Shift+Enter over a non-empty document text selection now follows the matching
soft-break behavior: it replaces the selected source range with a soft line
break, keeps the edit in the same paragraph, and restores the caret immediately
after the inserted break.
Multi-line paste over a selected range in a formatted multi-run paragraph now
uses the same source-first semantics: the selected range is deleted across text
runs, the first pasted line remains in the source paragraph, later lines become
following paragraphs, and focus/caret stay on the first pasted-line block.
Ctrl/Cmd+Z and Ctrl/Cmd+Y from a focused document text block now run through an
async pre-command hook so a visible, uncommitted canvas draft is committed before
undo/redo dispatch; GUI smoke proves the draft can be undone back to the prior
source text and redone without losing editor focus.
Ctrl/Cmd+ArrowLeft/Right now follows the same
rule for word-wise movement inside a focused document text block: visible
drafts are committed first, the target word boundary is computed from the block
text, and focus/caret are restored through the editor projection after rerender.
Ctrl/Cmd+Shift+ArrowLeft/Right uses the same word-boundary logic to project a
source-aware selected range inside the focused block, so toolbar formatting can
consume keyboard-selected words through the normal split-and-mark operation
path.
Menu commands carry `data-command-scope` plus disabled/`aria-disabled` state for
Docs-only and Sheets-only actions, and the shared binder ignores disabled
actions. Toolbar buttons now carry explicit `data-toolbar-action`
markers and stable sizing, so command access can be tested as editor toolbar
chrome rather than incidental panel buttons. The horizontal toolbar now uses
named sections: Docs mode groups History and Text formatting controls only,
while Sheets mode groups Sheet and Range controls. Docs block creation must not
appear as a Blocks or Insert toolbox; paragraphs, headings, lists, page breaks,
tables, images, citations, footnotes, mentions, and equations are created
through normal keyboard editing, slash commands, paste/import flows, or
context-specific non-toolbar surfaces. The horizontal toolbar is
mode-specific too: Docs mode renders only
the Docs formatting toolbar, while Sheets mode renders only the Sheets
formatting toolbar. Spreadsheet selected-sheet lifecycle, paste, copy/range
naming, selected-cell formatting, validation, and comment commands remain in
Sheets menu/toolbar chrome instead of a sheet-body action strip, while
row/column, freeze, merge/unmerge, filter/sort/clear-filter, and
protected-range structural commands are kept out of the persistent toolbar and
reached through the Sheet menu or cell right-click menu. The toolbar targets the selected workbook tab rather than the first
sheet. The home page now also
has a Refresh action that scans the selected local
object repository for validated lookup records, enriches discovered entries
with manifest/snapshot title and DOI metadata when available, and merges them
into `recent_documents` without opening a document implicitly. Browser/mock mode
uses the same `scan_local_repository` command over its mock repository index,
while native/Tauri scans the real local repository through Rust.
The spreadsheet view
tracks selected sheet and selected cell as local UI state, renders that state in
the name box/formula bar, highlights the selected cell, and exposes a bottom
workbook tab bar for sheet switching. Selected-cell validation and note actions
now live in the selected-cell right-click menu rather than the persistent
Sheets toolbar, while grid cells render compact validation and comment
indicators plus a selected-cell detail strip under the formula bar for
editing current cell comments; the grid no longer exposes always-visible
per-cell action buttons. Cell comments remain inline-editable in the detail
strip, while deletion is reached through a cell-comment context menu instead
of a persistent delete button. Spreadsheet row and column headers no longer
render persistent `x` delete buttons; row and column deletion remain available
through the Sheet menu and spreadsheet cell context menu after selecting a cell
on the target axis. It also supports projection-only shift-click range
selection: the name box and selection context display the
normalized range, cells inside the range get a distinct selection affordance,
and merge/filter/protected-range controls consume the selected range while
falling back to their previous defaults when no range is selected. Copy,
named-range, selected-cell formatting, and active-sheet batch cell actions now
also consume the selected sheet, selected cell, or selected range instead of
hardcoded sample cells. The persistent Sheets toolbar Copy/Paste buttons now
use spreadsheet clipboard semantics for the selected range and selected cell;
the explicit duplicate-range command that copies formulas with shifted
references remains in the Sheet menu and cell context menu, targeting the first
cell immediately to the right of the selected range. Named-range rows no longer render persistent
update/delete buttons; selecting a named range stays a direct click target,
while update/delete move to a named-range context menu covered by GUI smoke.
Creating a named range is also no longer a persistent toolbar button; it is
available from the Sheet menu and the selected-cell/range context menu so the
range toolbar stays focused on frequent spreadsheet editing controls. Named
range chips are focusable and open the same context menu from `Shift+F10` or
the platform ContextMenu key.
Protected-range rows follow the same pattern: warning metadata remains visible,
but update/delete actions are reached through a protected-range context menu
instead of persistent row buttons, and the protected-range row also exposes the
same menu from keyboard context-menu events. Merged-range rows now keep only the visible
range label; unmerge is reached through a merged-range context menu and covered
by GUI smoke for imported and newly created merges, including keyboard-opened
context menus.
Selected-cell formatting controls now cover bold,
italic, text color, fill color, horizontal alignment, and number format through
the existing `set_spreadsheet_cell_format` operation, with both toolbar and
right-click context-menu smoke coverage proving the rendered cell classes and
format attributes update immediately. Sheets formatting controls also expose
mixed selected-range state for partially bold/italic ranges and mixed text/fill
colors, while the first toggle click fills the selected range and the second
clears it through normal cell-format operations. Horizontal alignment and
number-format toolbar controls are now range-aware dropdowns with mixed-state
display for heterogeneous selections; menu/context prompt actions remain for
less common custom values. GUI smoke now covers mixed alignment, mixed
number-format state, concrete range updates, and returning number formats to
General/default across the selected range. Named range chips are now selectable projection
targets: clicking one switches to its sheet, selects its range, and updates the
name box without committing source data. The former sample cell-batch command is now a
Paste action that accepts tab-separated values and writes them from the
selected cell through the active-sheet batch operation; direct TSV paste into a
selected grid cell uses the same operation-backed path and is covered by GUI
smoke. Prompt-driven paste and the low-level active-sheet batch setter remain
available through the Sheet menu/API for coverage, but they are no longer
persistent toolbar buttons; normal users reach that behavior through clipboard
buttons, grid paste, and spreadsheet editing.
Cell validation lists and warning-only protected range descriptions are
prompt-backed operation inputs rather than fixed strings; validation cells now render allowed values
and strict/dropdown state as visible grid affordances covered by GUI smoke and
visual-contract checks, with validation create/clear covered through the
selected-cell context menu. Selected-cell comments prompt for the note body from
the selected-cell context menu before dispatch, and existing cell-comment detail
rows are focusable so delete-comment context actions are reachable from the
keyboard as well as right-click. New spreadsheet sheets now prompt for a
sheet title before dispatching the add-sheet operation instead of creating a
fixed `Data` tab; the operation is reached from the Sheet menu or workbook tab
bar plus button rather than a persistent Sheets toolbar button. Row and column headers are selectable too:
clicking a visible header selects the full visible row or column range, updates
the range display, and highlights the selected axis header without committing
source data. Frozen row and column metadata now marks body cells as frozen and
applies sticky grid behavior, with GUI smoke and visual-contract coverage for
the body-cell freeze affordances. Merged cell ranges now render in the grid as
actual `colspan`/`rowspan` cells with covered cells omitted, and GUI smoke
proves imported, unmerged, and newly merged ranges update the grid projection.
Typed ranges in the name box, including
reversed ranges such as B3:A1, normalize through the same
projection-only selection path without committing source data. The Sheets
selection context now computes projection-only count and numeric sum summaries
for the active range, ignoring empty and non-numeric cells, so range selection
has spreadsheet-style feedback without entering the signed source. Spreadsheet
find is projection-only too: queries match cell addresses, user-entered values,
and computed values, report match counts, highlight matching cells, and `Next`
moves the selected cell without appending operations. The formula bar is
editable and uses the shared Sigma icon for its compact formula marker instead
of a text `fx` chip. It commits changes to the selected cell through the same
spreadsheet cell operations as grid edits; Enter commits and moves down, Tab
commits and moves right, Ctrl/Cmd+Enter commits without moving the selected
cell, and Escape cancels the pending formula-bar edit without writing an
operation. Enter, Tab,
and Ctrl/Cmd+Enter formula-bar commits now restore keyboard focus to the target
grid cell after rerender, so subsequent cell navigation continues without a
mouse. Ctrl/Cmd+L in
Sheets mode focuses the selected cell's formula bar so
keyboard users can edit source values without making every grid cell look like a
form field. The formula bar input and name box are exposed as accessible
textbox controls with explicit labels while remaining projection-only UI.
Formula-bar typing now also handles ordinary `beforeinput`
insertion and deletion as controlled projection-only drafts, preserving focus
and avoiding source operations until the user commits. Plain-text paste in the
formula bar now uses the same controlled draft path, normalizes pasted line
endings, preserves formula-bar focus, and commits the pasted source only on the
normal commit shortcut or blur. Controlled formula-bar edits now restore the
text caret after `beforeinput` and paste rerenders, matching the active grid
cell editor's draft-edit behavior. Active grid-cell edit mode now has its own Sheets-like projection:
the selected `td` gains `editing-cell`, the contenteditable child exposes
`data-cell-editor-caret`, and CSS draws a compact green edit outline/caret while
the surrounding cell still carries spreadsheet selection semantics. Typing `=`
on a selected display-mode spreadsheet cell now starts in-cell formula editing
with `=` as the source draft; committing returns the cell to display mode with
the computed result visible while retaining formula-source metadata. The name
box is also editable and moves the selected cell as
projection-only UI state; ordinary `beforeinput` edits update a controlled
draft with focus and caret retained, Ctrl/Cmd+G focuses it, Enter commits a
cell/range jump, and Escape cancels typed name-box text without changing
selection. Plain-text paste in the name box also updates the projection-only
draft, preserves focus/caret, and does not change selected cells until the
address or range is committed. Valid name-box commits now restore keyboard
focus to the selected grid cell, using the first normalized cell for range
jumps. Invalid name-box commits preserve the invalid draft with a warning and
return focus/caret to the name box so the address can be corrected without
losing the current sheet selection. Escape cancel for both the formula bar and name
box now restores focus to the same controlled editor after discarding the draft,
so keyboard users do not lose their place.
The formula bar and name box now also handle browser-native
`beforeinput insertFromPaste` and `deleteByCut` through those controlled draft
helpers, so pasted/cut selected text stays in RAM with restored focus/caret and
does not commit a cell source update or selection jump until the user accepts
the draft. Actual browser text drops now use the same controlled draft helpers
for both fields, preventing DOM-only insertion while preserving focus and caret
projection.
Shared plain-text paste handling for smaller contenteditable fields now also
respects the active caret or selected range instead of appending blindly; this
covers document titles, sheet titles, comments, suggestions, footnotes,
equation source, image alt text, and cell comments while preserving their
existing blur/composition commit semantics. The shared beforeinput edit helper
also handles browser-native `insertFromPaste` and `deleteByCut`, so side-panel
comments/suggestions, footnotes, equation source, and image alt text get
immediate operation-backed paste/cut where their handler commits per input.
Sheet titles and spreadsheet cell comments now also use controlled
operation-backed `beforeinput`, paste, and actual browser text-drop paths with
focus/caret restoration; invalid blank cell-comment drafts stay visible for
correction instead of committing a rejected source operation.
GUI smoke proves caret-aware plain paste on sheet-title rename and native
beforeinput paste/cut on comment editing, and now covers sheet-title and
cell-comment text drops.
Display-mode cells now also enter active edit mode when Enter, F2, or a
printable key is typed directly, including `=`, preserving the typed seed
and restoring the caret after that seed before commit so formula entry feels
like a spreadsheet while the grid stays display-first. Active cell editors
expose the controlled draft through `data-cell-editor-value`, giving tests and
debug tooling a projection-only way to inspect the edit buffer without treating
it as signed source before commit.
Active cell editors handle ordinary `beforeinput` text insertion plus
Backspace/Delete as controlled edit-draft updates, rerendering with focus still
in the cell and restoring the computed caret offset while deferring source
operations until Enter, Tab, blur, or the formula bar commits the value.
Active spreadsheet cell editors also handle browser-native
`beforeinput insertFromPaste` and `deleteByCut` through the same controlled RAM
draft path as typed insertion/deletion: pasted text is normalized, selected
ranges are replaced, cut ranges update the draft and caret, and no source cell
operation is committed until the user accepts the edit.
The Sheets selection context now exposes the selected sheet id/title, active
cell address/value, selected range, range shape, count, and numeric sum as
projection-only status markers, so the compact top/status UI can stay
spreadsheet-like without adding signed source fields.
Arrow keys now remain local to the active grid-cell text editor instead of
exiting edit mode or moving the spreadsheet selection; display-mode cells keep
the grid navigation and range-extension behavior.
Ctrl/Cmd+Enter inside an active grid-cell editor now inserts a source newline
into the cell draft, restores focus and caret to the edited cell, and still
defers persistence until a normal cell commit.
Plain-text paste inside an active grid-cell editor now behaves like text
editing rather than range paste: it normalizes line endings, replaces the active
cell selection in the projection-only draft, restores caret/focus, and defers
the source operation until commit. Display-mode paste still keeps spreadsheet
clipboard/TSV range semantics.
Committed source values containing line breaks now render with a `multiline-cell`
projection class and preserve line breaks in display mode while ordinary cells
keep the compact single-line grid layout.
Committed formula cells keep showing computed results in display mode, but the
grid now marks the containing `td` as `formula-cell` and renders a compact
non-interactive Sigma icon badge through `data-cell-formula-badge`; the raw
formula source remains available through formula-bar state and data attributes
rather than visible grid text.
Spreadsheet cell validation, comment, and protected-range indicators now use
compact shared SVG icons (`list-check`, `message`, and `warning`) with stable
badge dimensions and accessible labels/titles instead of text chips like
`val`, `note`, or `warn`, keeping dense cells closer to a Sheets-like grid
while preserving all source-level data attributes for tests and tooling.
Spreadsheet keyboard navigation and edit exits now restore DOM focus to the
selected grid cell after rerender: Arrow keys, Shift+Arrow range extension,
Tab/Shift+Tab, Enter/Shift+Enter, Home/End row-boundary movement,
Ctrl/Cmd+Home/End sheet-corner movement, Shift-extended Home/End ranges,
PageUp/PageDown visible-page movement, Shift-extended PageUp/PageDown ranges,
Escape cancel, and blur commit/no-op can continue from the active cell without
a mouse. These navigation paths are projection-only until a separate cell edit
or clipboard command writes operations.
Escape on a focused display-mode cell now collapses an active selected range
back to that cell, restores grid focus, and leaves workbook source untouched.
Display-mode cells also support keyboard range selection without reaching for
header buttons: Ctrl/Cmd+Space selects the current column, Shift+Space selects
the current row, and Ctrl/Cmd+Shift+Space or Ctrl/Cmd+A selects the visible
sheet range, all as projection-only selection state with focus restored to the
range start.
Focused display-mode cells also handle Ctrl/Cmd+Z, Ctrl/Cmd+Y, and
Ctrl/Cmd+Shift+Z through the shared edit-history commands and restore focus to
the active grid cell after rerender, so undo/redo does not require leaving the
sheet canvas for menu chrome.
Display-mode cells now clear the active selected range, not just the focused
cell, on Delete/Backspace by dispatching the same operation-backed batch cell
clear path used by range cut; GUI smoke verifies `B2:B3` clears together and
then restores fixture values for later formula checks.
Display-mode cells now support standard keyboard clipboard shortcuts:
Ctrl/Cmd+C stores the selected cell/range as TSV in runtime-only clipboard
state, Ctrl/Cmd+X stores the same TSV and clears the source cells through normal
spreadsheet cell operations, and Ctrl/Cmd+V pastes the internal TSV at the
active cell, falling back to browser clipboard text when available. The
clipboard cache is projection/runtime state only and is not part of signed
workbook source. The spreadsheet workspace now exposes nonvisual clipboard
state/shape markers so GUI smoke can prove multi-cell range copy, including
`B2:B3` as `2x1`, without mutating fixture cells just to inspect the cache.
Spreadsheet cell right-click menus now expose normal Cut, Copy, and Paste
clipboard actions that reuse the same runtime TSV cache and operation-backed
paste/clear behavior as the keyboard shortcuts, while the older copy-to-right
workflow is labeled separately as a duplicate-range action. GUI smoke covers
context-menu copy, paste, and cut against the selected cell.
Arrow-key navigation moves the selected spreadsheet cell locally without
committing source data. Frozen rows/columns and basic
filters now project into spreadsheet row/column headers with deterministic
markers derived from sheet state. The Docs view now renders a document
workspace with a ruler and a heading-derived outline, both as projection-only UI
state outside source signatures. The ruler now reflects the selected block with
projection-only style and indent markers, so layout chrome tracks editor
selection without adding signed document state. It also shows a compact
projection-only page-size chip for the current Letter canvas, making the page
model visible without adding page setup to signed source yet. The active
toolbar now includes compact zoom-out, zoom-in, and view zoom/scale slider
controls for the document/workbook workspace; they scale the rendered surface
as display state and do not dispatch source operations. GUI smoke covers the
toolbar button path as well as direct slider input, so zooming is available
without opening the View menu.
The Fit width zoom preset now uses a concrete `columns` SVG icon path instead
of falling back to text, and the GUI smoke/static visual contracts keep mapped
toolbar/menu actions on the icon-first path required by the product chrome.
The remaining View-menu projection actions now avoid text fallback icons too:
actual-size reset, outline toggle, and ruler toggle have dedicated SVG icon
paths pinned by GUI smoke and visual-contract checks.
The View menu also exposes zoom out, actual-size reset, and zoom in actions
that share the same projection-only zoom state as the slider, with GUI smoke
coverage proving custom zoom survives Docs/Sheets mode switches and Fit width
or Fit page presets are recalculated against the newly active Docs/Sheets
surface. Standard Ctrl/Cmd+plus, Ctrl/Cmd+minus, and Ctrl/Cmd+0 shortcuts
update the same projection-only zoom state while the editor view is active,
preventing browser zoom from stealing common document/spreadsheet view
commands. The same zoom state now supports Fit width and Fit page presets from
both the View menu and compact toolbar icon buttons, keeping office-style scale
presets outside source, hashes, and signatures. The document
outline can also be hidden and restored from the View menu as projection-only
workspace state; hiding it collapses the outline column and does not affect
source, operations, hashes, or signatures. The document ruler can likewise be
hidden and restored from the View menu as projection-only workspace state,
giving space back to the page stack without changing document source. The
topbar status dropdown now uses
a compact save-state chip with explicit saved/dirty data markers and an icon,
so unsaved operation-backed edits are visible without spending another toolbar
row. The same status control now includes a compact signature-state chip with
explicit signature-state and signature-count markers, giving ordinary users the
lightweight signed/unsigned visual indication without blocking unsigned
documents. Repository/storage state now uses the same compact chip pattern with
explicit backend and namespace markers, showing local drafts versus saved
repository-backed documents without adding another toolbar row. These chips are
also actionable, keyboard-accessible workflow entry points: save opens
repository controls for drafts or saves the current repository-backed document,
signature opens the signatures panel, and repository opens the
repository/sharing/signing controls. The same compact status chips now carry
explicit action-oriented accessible labels, with GUI smoke and visual-contract
coverage proving the icon-first chrome still exposes save, signature, and
repository actions clearly without adding another visible row. The editor
chrome now also renders the document title as a compact editable field next to
the mode switch; typing, paste, actual browser text drops, and native title
copy/cut use controlled source-backed title editing with `set_document_title`
and focus/caret restoration, while the File > Rename prompt remains as a
fallback command path. The status dropdown also computes
projection-only document word and character counts from the current source
block tree, including nested table text, equation source, and image alt text,
without storing those counts in signed source. In Sheets mode, the same
dropdown shows the selected sheet/range count and numeric sum from the current
projection, leaving spreadsheet source, operation hashes, and signatures
unchanged.
The compact status row now has explicit max-width, min-width, overflow, and
ellipsis guards for chip labels and toolbar summary text, with visual-contract
coverage to keep long repository namespaces, titles, or statuses from
overlapping neighboring first-viewport chrome.
Outline entries are clickable and update
projection-only selected block state, highlight the active outline target and
document block shell, and feed selected-block fallback into heading, paragraph,
list, table row/cell, and delete commands while stale block/inline selections
are reconciled after document open/create/import/close. Document block shells
are directly selectable, and table row/cell toolbar actions resolve the
selected or containing table before falling back to the first table, so table
editing is no longer limited to an implicit first-table target. The inspector
is now a Docs-like tabbed side
panel for comments, suggestions, citations, attachments, and audit/recovery
views instead of one unstructured long list; comment, suggestion, citation,
attachment, signing/verification, deletion/restore, and audit actions route the
side panel to the relevant tab automatically, and tab badges surface live
comment, proposed-suggestion, citation, attachment, warning, and recovery counts
without requiring users to open each panel. The citations panel also has a
projection-only filter over document-local references, bibliography entries,
and citation groups, preserving source/signature state while making larger
bibliographies navigable. Repository, sharing, and signing
internals are moved behind a collapsible advanced panel, while save, autosave,
verify, audit, share, and close entry points live in the compact menu chrome.
Docs toolbar selectors are now operation-backed: paragraph style reflects the
selected paragraph, heading level, or list item and only mutates compatible
existing block state rather than creating new blocks; font family and font size
apply source text marks to the selected inline, and shift-click inline range
selection plus Shift+Arrow keyboard range extension dispatch
`add_text_mark_range` so formatting can apply across stable text range endpoints
instead of a single inline only. The editor
also renders a selection-context strip that reflects the active Docs inline/block
or Sheets sheet/cell/value as projection-only UI state. Google Docs/Sheets
export results now render as a typed, dismissible export panel with format and
byte-count metadata instead of an unlabeled raw textarea, and source-changing
commands clear stale export output. The editor quick-action chrome now includes
projection-only document find: queries report match counts, highlight matching
inlines, and `Next` moves the current inline selection without touching signed
source or operation history. The Docs page now
projects comment anchors into a block review gutter and proposed suggestions
into inline markers, with both marker types routing to the relevant side-panel
tab without changing source signatures. The app projection now exposes
`has_unsaved_changes`, derived from saved operation count versus current
operation journal, and the topbar/quick-action chrome renders saved, unsaved,
not-saved, or closed state from app API state instead of DOM inference. The
desktop visual contract now checks
that the required home/editor/Docs/Sheets/side-panel markup and layout CSS
constraints remain present, including wrapping, grid/minmax sizing, selected
cell state, frozen/filter markers, and no viewport-scaled font or negative
letter-spacing patterns. Verified by
`cargo test -p opendoc-app-api scans_local_repository_into_recent_documents_without_opening_document`,
`cargo test -p opendoc-app-api save_state_tracks_unsaved_operations_across_repository_lifecycle`,
`npm --prefix apps/desktop run command-contract`,
`npm --prefix apps/desktop run mock-contract`,
`npm --prefix apps/desktop run build`,
`npm --prefix apps/desktop run visual-contract`, and
`npm --prefix apps/desktop run gui-smoke`.

## Browser, HPC, And Service Workstream

All modes reuse the same source schema, binary records, app API, and operation semantics.

Browser local mode:

- Uses browser-appropriate storage.
- Opens unsigned and signed documents normally.
- May verify signatures before it can create them.
- Browser signing remains postponed.

Current implementation evidence: the browser-side app API mock persists saved
repository snapshots, blob bytes, deleted-blob audit data, recent documents,
and DOI lookup hints through browser `localStorage` when available, with
in-memory fallback for restricted runtimes and test harnesses. The runtime
contract verifies save/open, recent-document restoration, and DOI reopen across
module reloads for the flat browser-local repository shape. Candidate saves are
stored separately from normal heads, and the runtime contract verifies that a
candidate does not appear on normal open until `merge_flat_repository_candidates`
reconciles it after a reload. Corrupt browser-local candidate records do not
block normal open; they appear as deterministic audit/recovery candidate-head
problems. Stale browser-local DOI lookup indexes fall back to deterministic
same-store snapshot scanning and return an explicit warning instead of making
the document unreachable.

HPC single-user mode:

- Runs behind external authentication, such as an Open OnDemand-style environment.
- Can access local disk and S3-like storage.
- Assumes the authenticated user has full access to reachable repositories.
- Does not enforce document-level permissions.

Current implementation evidence: the desktop runtime capability smoke runs the
same GUI surface in `hpc-single-user` mode, verifies the externally
authenticated subject, confirms document-level permissions are not enforced,
confirms local, flat, and OpenDAL filesystem backends are visible, and exercises
local, flat, and OpenDAL filesystem save actions from that mode. Verified by
`npm --prefix apps/desktop run runtime-capability-smoke`.

Multi-user service mode:

- Owns authentication, permissions, sharing, presence, sync relay, lookup acceleration, and optional commit serialization.
- Enforces permissions outside the document format.
- Uses the same operation and storage semantics as local mode.

Current implementation evidence: `OpenDocRuntimeSession` in `opendoc-app-api`
models authenticated subject, service permission grants, presence peers, and
runtime warnings outside signed document source state. The same
`get_runtime_session` command is exposed through Tauri and the browser mock.
`authorize_runtime_command` maps commands to `read`, `comment`, or `write`,
checks runtime capabilities, and denies missing multi-user service grants
without adding permissions to document source state. The runtime contract
also proves that browser-local mode denies private-key signing while still
allowing signature verification as read-only access, preserving the decision
that browser key handling is postponed but signed documents remain openable.
The runtime contract
verifies that service wrappers can pass configured grants through
`runtimeConfig()` into `get_runtime_session`; the Rust app dispatcher and Tauri
bridge preserve supplied service grants for multi-user sessions and ignore
runtime grants outside service mode with an explicit warning. Rust and browser
runtime sessions also normalize supplied service grants, ignore malformed grant
entries, and preserve the normalized grants for later authorization matching.
Rust and browser command classifiers now agree that spreadsheet cell comment
restore is a `comment` action rather than a body-write action, so service
wrappers can grant audit/recovery comment repair without broad edit rights.
Presence peers are normalized the same way: malformed entries are ignored,
missing display names and roles are defaulted, and negative timestamps are
clamped so presence cannot make otherwise valid service sessions unopenable.
Rust and browser authorization normalize supplied service grants before
matching, so wrapper whitespace does not change service permission semantics.
Runtime DOI lookup now fails gracefully instead of silently choosing the first
match when a service index or serverless repository scan has duplicate DOI
entries; the Rust test
`runtime_document_lookup_prefers_service_index_and_scans_without_server`, the
browser `mock-contract`, and `runtime-contract` cover service-index ambiguity,
scan ambiguity, explicit unresolved state, and warning text.
The shared runtime contract now also verifies wrapper-configured capability
overrides: browser/HPC/service hosts normalize advertised storage backends,
deduplicate valid backend names, ignore invalid backend names, and authorize
local, flat, OpenDAL, and private-key signing commands against the same
capability profile shown to the UI. Rust authorization now classifies flat
repository commands explicitly alongside local and OpenDAL commands, with
`cargo test -p opendoc-app-api runtime_authorization_applies_runtime_capabilities_without_document_permissions`
proving browser-local mode allows flat storage while denying local storage.
The same shell capability fields now pass through the shared Tauri/browser
command contract into Rust runtime profile, session, authorization, share,
relay, and lookup commands; `runtime_dispatch_honors_shell_capability_overrides`
proves native dispatch honors configured storage/signing policy rather than
silently falling back to mode defaults.
Rust and browser runtime relay paths normalize malformed wrapper operation
inputs into invalid envelopes and reject them deterministically rather than
aborting an otherwise classifiable batch. Rust runtime relay and lookup paths
also normalize operation envelopes and lookup entries before classification or
result projection; malformed runtime lookup records are ignored with explicit
warnings before service-index matching or scan fallback. The GUI command boundary
preflights document commands in permission-enabled runtimes, and the runtime
capability smoke verifies that a read-only multi-user service subject cannot
mutate the document through the normal editing toolbar and that a subject
with read+comment grants can add comments through the normal comment toolbar
while still being denied body edits. It also verifies that a subject without a
read grant cannot export through the normal document menu or render the
audit/recovery projection.
`create_runtime_share_invite`
creates service-layer target grants only in multi-user service mode and fails
gracefully elsewhere. Rust tests also prove share invites normalize padded
issuer, target, document, and action inputs before authorization and grant
creation, deduplicate supported target actions, and ignore unsupported or
mixed-type action inputs without aborting the command boundary.
`relay_runtime_sync` classifies service-mode operation
envelopes into accepted, deferred-for-reconciliation, and rejected sets without
mutating signed document source state; it denies non-service modes and
deterministically rejects zero-sequence malformed envelopes and operation actor
names that do not match the authenticated service subject, while deferring
duplicate operation IDs and duplicate actor/sequence pairs. Verified by
`cargo test -p opendoc-app-api runtime_sync_relay`,
`cargo test -p opendoc-app-api runtime_sync_relay_dispatch_is_available_after_close_and_denies_bad_modes`,
`cargo test -p opendoc-app-api dispatches_desktop_command_contract`,
`npm --prefix apps/desktop run command-contract`,
`npm --prefix apps/desktop run runtime-contract`,
`npm --prefix apps/desktop run mock-contract`, and
`cargo check --manifest-path apps/desktop/src-tauri/Cargo.toml`.
`resolve_runtime_document_lookup` models service-index lookup acceleration and
serverless repository scan fallback outside signed document source state. It
prefers service indexes only in multi-user service mode, falls back to scan
records for local/HPC/serverless modes, and enforces read grants in
multi-user-service mode. Verified by
`cargo test -p opendoc-app-api runtime_document_lookup`,
`npm --prefix apps/desktop run runtime-contract`, and
`npm --prefix apps/desktop run mock-contract`. The GUI also exposes this path
through the runtime session panel and smoke-tests multi-user service-index DOI
resolution.

Native local, flat, and OpenDAL filesystem repositories also preserve DOI
open-by-scan behavior when direct lookup indexes are missing or stale, and the
app projection reports the same `doi-lookup-scan-fallback` warning.

Citation audit/recovery now includes source-level restore commands for deleted
bibliography references and citation groups. Restores are operation-backed
upserts, rerender dependent labels, are exposed through Tauri/browser command
contracts, are verified across local repository save/open, and converge in the
merge engine as higher-revision upserts over older deletes. The browser mock
contract exercises bibliography and citation delete/restore in the shared
command sweep while creating fresh deleted fixtures for audit-view verification.
Browser-local citation insertion, grouped-citation updates, footnote citation
insertion, bibliography update/delete/restore, and citation-group delete/restore
normalize wrapper whitespace on stable IDs before lookup and projection; the
browser mock contract sends padded reference, citation, footnote, and inline
anchor IDs through those paths.
Browser-local footnote body updates also trim padded footnote IDs before source
lookup and operation projection, with padded coverage in the browser command
sweep.
The desktop GUI smoke test now restores deleted bibliography references and
citation groups through the audit/recovery controls.

Spreadsheet audit/recovery includes operation-backed restore for deleted cell
comments, deleted sheets, deleted rows, deleted columns, deleted named ranges,
deleted protected ranges, cleared basic filters, cleared cell validations, and
unmerged cell ranges. Deleted comments stay hidden from normal sheet views but can be
reactivated from audit/recovery and persist through repository save/open.
Deleted sheets expose the sheet ID and deleting operation in audit/recovery;
the deleting operation retains the sheet snapshot and sheet-local named ranges
so `restore_spreadsheet_sheet` can return them to current workbook state
without adding a separate signed workbook tombstone field. Deleted rows retain
their row axis metadata, row cells, row-intersecting merges, filters,
protected ranges, and named ranges in the delete operation payload; the audit
view exposes that payload and `restore_spreadsheet_row` returns it to current
state when the sheet still exists and the row label has not been reused.
Deleted columns use the same operation-payload pattern for column axis
metadata, column cells, and column-intersecting merges, filters, protected
ranges, and named ranges, with `restore_spreadsheet_column` returning retained
source records when the sheet still exists and the column label has not been
reused.
Deleted named ranges and protected ranges similarly retain their source records
in deleting operation payloads, expose them through audit/recovery, and restore
them with `restore_spreadsheet_named_range` or
`restore_spreadsheet_protected_range` when the target sheet still exists and
the name/range has not been reused. Cleared basic filters retain their range,
criteria, and sort specs in the clear operation payload and restore when the
target sheet has no current filter. Cleared cell validations retain their
validation source record in the clear operation payload and restore when the
target cell has no current validation. Unmerged cell ranges retain their merge
source record in the unmerge operation payload and restore when the sheet still
exists without an overlapping current merge. The browser mock contract now
exercises spreadsheet sheet deletion audit rows, sheet restore with cell and
named-range recovery, row deletion audit rows, row restore with retained cell,
merge, protected-range, and named-range recovery, column deletion audit rows,
column restore with retained cell, merge, protected-range, and named-range
recovery, named-range delete/restore audit rows, protected-range delete/restore audit rows, cleared-filter
delete/restore audit rows, cleared-validation restore audit rows,
unmerged-range restore audit rows, plus spreadsheet
cell-comment delete and restore in the shared command sweep while keeping a
separate deleted-comment fixture for audit-view verification. The desktop GUI
smoke test now restores deleted spreadsheet rows, deleted spreadsheet columns,
deleted named ranges, deleted protected ranges, cleared filters, cleared
validations, unmerged ranges, and deleted cell comments through the
audit/recovery controls, verifying that retained row cells, column cells,
range/filter/validation/merge records, and comment bodies return to current
spreadsheet state.
Spreadsheet basic filters now include signed source-state criteria and sort
specs, with Google Sheets-shaped import/export coverage for `basicFilter`
criteria and `sortSpecs`. Local, flat, and OpenDAL filesystem repository
candidate-merge tests prove filter options survive concurrent saves, and local
and flat warning tests prove malformed filter options degrade to persisted audit
warnings. Rust source and operation validation now reject duplicate filter
criterion columns and duplicate filter sort columns before app-source snapshots
or durable operation segments are trusted; covered by
`spreadsheet_source_validation_rejects_invalid_structural_payloads`,
`spreadsheet_basic_filter_persists_replay_and_is_signed_source_state`, and
`save_rejects_padded_spreadsheet_filter_option_operation_before_writing_objects`.
The browser runtime and mock contract enforce the same duplicate-column
projection boundary. The desktop GUI smoke test now applies filter criteria and
sort options through the spreadsheet controls and verifies the rendered
source-state projection before clearing the filter. Spreadsheet basic filters
also apply to the rendered grid as projection-only row filtering and sort
ordering: the source row set stays intact, while GUI smoke now proves
non-matching rows are hidden, matching rows render in sorted order, cleared rows
return after clearing the filter, and audit recovery restores the cleared
filter. Warning-only protected ranges now render
as protected cell affordances and selected-cell protection details in the
spreadsheet GUI, with smoke coverage proving protected cells remain visible
without blocking edits. The browser mock contract now imports a
Google Sheets-shaped fixture with frozen panes, merged ranges, filter criteria
and sort specs, warning-only protected ranges, named ranges, list validation,
formulas, plain Google notes, and `opendocCellComments`, then parses export JSON
to prove those v0 structures round-trip through the shared frontend contract.
Spreadsheet structural candidate-merge tests now cover concurrent current-head
cell/formula edits against candidate frozen panes, cell validations, merged
ranges, and warning-only protected ranges across local, flat, and OpenDAL
filesystem repositories. The tests verify merged projection state, reopened
state, operation provenance, dependency invalidation, and idempotent second-pass
candidate reconciliation. The focused tests
`local_repository_merges_three_spreadsheet_candidate_editors`,
`flat_repository_merges_three_spreadsheet_candidate_editors`, and
`opendal_fs_repository_merges_three_spreadsheet_candidate_editors` extend this
to a three-editor no-server scenario where a current-head data edit and two
stale candidate streams for formula/formatting and
named-range/validation/filter structure converge, recompute formulas, retain
operation provenance, reopen from storage, and make a second merge pass
idempotent. Candidate spreadsheet replay also validates the
workbook after every replayed envelope, so aggregate invalid states that pass
per-operation parsing, such as adding a distinct sheet with a duplicate title,
degrade to `invalid-spreadsheet-source` warnings and preserve the prior valid
workbook. Verified by
`cargo test -p opendoc-app-api --features opendal-store candidate_merge_persists_structural_spreadsheet_warnings`.

Rich-document audit/recovery includes operation-backed restore for deleted
comment threads and individual deleted comments. Restore operations are exposed
through the shared Tauri/browser command contract, persist through save/open,
and converge automatically with concurrent deletes in the merge engine. The
focused Rust test `audit_view_exposes_retained_recovery_state` proves deleted
comment recovery rows, resolved suggestion audit rows, and their operation
history remain visible after local repository save/open. The
browser mock contract now exercises accepted and rejected suggestions instead
of treating suggestion resolution as deferred, and verifies rejected suggestions
through the audit view. Rich comment source now rejects empty threads,
duplicate thread/comment IDs, empty bodies, and whitespace-padded authors before
trusted app snapshots are accepted, while Rust and browser-local command paths
trim user-entered comment authors before storing signed source. The browser mock
contract validates the same comment source boundary and sends padded
thread/comment IDs through comment update/delete/restore commands, with
browser-local handlers trimming those IDs before lookup to match Rust stable-ID
parsing. Degraded nearest-block anchors now reject empty or whitespace-padded
warning payloads before trusted source or Google Docs-shaped review extensions
are accepted; covered by
`comment_and_suggestion_anchors_require_auditable_payloads`,
`google_docs_extension_lists_must_be_arrays`,
`malformed_google_docs_review_metadata_does_not_replace_current_document`, and
the browser mock malformed review cases. Suggestion source now
rejects duplicate IDs, whitespace-padded authors, empty or whitespace-padded
provenance entries, unsupported states/kinds, and variant-incoherent
insert/delete/format payloads before trusted app snapshots or Google
Docs-shaped review extensions are accepted; covered by
`suggestions_require_auditable_payloads`,
`malformed_review_extension_source_metadata_aborts`,
`malformed_google_docs_review_metadata_does_not_replace_current_document`, and
the browser mock malformed review cases. Rust and browser-local suggestion
creation paths trim user-entered authors before storing signed source, and the
browser command sweep exercises padded suggestion author input. Suggestion
accept/reject command
inputs trim wrapper whitespace from suggestion IDs and resolution actor names
before operation provenance is recorded; the browser mock contract verifies the
documented `acceptedBy`/`rejectedBy` fields rather than fallback author fields,
and `save_rejects_padded_suggestion_resolution_operation_fields_before_writing_objects`
proves injected durable accept/reject operation actors are rejected before
repository writes.
Browser-local suggestion update, delete-suggestion, and format-suggestion target
IDs now use the same wrapper-whitespace trimming as Rust stable-ID parsing, and
the browser command sweep sends padded IDs through those paths.
It also exercises
`delete_comment_thread` in the shared browser command sweep plus
`restore_comment_thread` and `restore_comment` after comment deletion, while the
dedicated audit block verifies live delete/restore behavior. The desktop GUI
audit view exposes operation history count and
actor/sequence operation identities, and the GUI smoke test verifies those
audit operation records alongside restoring both an individual deleted comment
and a deleted comment thread through audit/recovery controls.

Binary attachment audit/recovery includes operation-backed restore for deleted
blob references by content hash. Restores reuse retained operation-history
metadata, preserve exact-byte and typed semantic signature sidecars, are exposed
through the shared Tauri/browser command contract, and persist through local
repository save/open without rewriting the underlying binary object. The
Rust app API now records `record-blob-archive-tombstone` as a durable blob
operation for current and retained deleted blob refs while keeping tombstones
outside signed document source, and
`app_api_records_archive_tombstones_for_current_and_deleted_blobs` asserts that
both paths append the audit operation.
`local_repository_merges_archive_tombstone_blob_operations` and
`flat_repository_merges_archive_tombstone_blob_operations` plus the
feature-gated `opendal_fs_repository_merges_archive_tombstone_blob_operations`
prove a candidate-recorded tombstone merges through local, flat-store, and
OpenDAL filesystem operation logs, persists after reopen, and remains visible
on the affected blob. The browser mock contract exercises padded content-hash
arguments across blob signing, typed signing, metadata update, delete/restore,
archive tombstone,
image insertion, and image replacement paths, plus binary delete/restore in a
dedicated exact-state restore check. The desktop GUI smoke test now deletes a
signed FASTQ/image blob, opens the audit view, restores the blob through the
`restore-blob` recovery action, and verifies the restored name and media type
render in current state.
Archive/tape recovery metadata can be recorded through the app API for current
or retained deleted blob references. Tombstones are binary repository sidecars,
not authored source state, and the GUI/API expose locator, restore hint, signer,
and audit visibility while keeping content verification dependent on recalled
bytes matching the original hash. Rust app tests and the browser mock contract
now prove archive tombstone locator, restore hint, and signer fields are trimmed
before repository storage and audit projection. The desktop GUI smoke test now
saves a local repository, records an archive tombstone for a current blob
through the `archive-blob` action, and verifies the archive locator and restore
hint render back in the app.
Google Docs-shaped app export now includes an `opendocBlobs` top-level
extension for current attachment/image blob refs. Import treats those refs as
shallow metadata without object bytes, preserves typed semantic signature
envelopes as untrusted until bytes are available, and warns when exact-byte
signature display metadata appears without the repository sidecar bytes needed
for verification. Imported archive tombstones remain metadata-only in app
projection, but when the Google-shaped `opendocBlobs` tombstone extension
includes signature bytes, the Rust importer keeps a strict internal tombstone
record and repository save writes it as a signed sidecar; malformed tombstone
fields or empty tombstone signatures abort import before replacing current
state. Imported image blob hashes, blob reference hashes, and typed signature
source hashes are trimmed before validation and projection so shallow refs line
up with content-addressed storage and sidecar signatures. Google
Docs-shaped OpenDoc extension IDs for lists, blocks, inlines, equations,
footnotes, citations/references, comments/suggestions, nearest-block anchors,
and text-range endpoints are now also trimmed before source projection so
wrapper whitespace cannot create distinct merge/audit anchors. The browser mock
command contract also verifies that Google Docs-shaped export includes
`opendocBlobs` and typed FASTQ signature metadata, and that browser import
creates shallow unsigned blob refs, preserves typed semantic signatures as
untrusted metadata, keeps exact-byte signatures sidecar-only, canonicalizes
nested citation item references, suggestion ranges, and imported blob
name/media-type metadata plus exact-byte and typed signature hash fields,
rejects duplicate exact-byte signatures, duplicate typed signatures, duplicate
blob refs, malformed `opendocBlobs` collections, missing required blob fields,
and non-byte typed-signature byte arrays before import, and reports the
exact-signature metadata warning. The Rust regression
`malformed_google_docs_blob_metadata_does_not_replace_current_document` proves
malformed `opendocBlobs` collections, missing required blob fields, malformed
typed-signature metadata, duplicate blob references, and empty typed-signature
byte arrays, malformed archive tombstone metadata, and empty tombstone
signature byte arrays abort import before replacing the current document state.

Done when browser, HPC, and service wrappers pass the same app API contract and repository conformance tests.

## Current Prototype Direction

Keep semantic verification API/test-first, but make the visible product proof
GUI-first. The GUI must become a serious Docs/Sheets-style editor while the
formats and merge semantics are still being proven, because the editor surface
is part of the design constraint.

Immediate priorities, with text entry as the active implementation front:

1. Put real text entry ahead of additional backend expansion. A newly created
   blank document must open as an editable page with a focused empty paragraph,
   visible caret, and ordinary typing/deletion/Enter behavior in the canvas.
   Proper paragraph layout is part of this priority and must not wait for later
   polish: paragraphs, headings, and list items must occupy the full usable page
   text column, with the review gutter outside the writing area, so click
   targets, caret movement, selection, wrapping, and paste/drop behavior feel
   like a document page rather than narrow inline widgets. The current CSS now
   makes document block shells, block content, and paragraph/list/heading bodies
   span the available text column explicitly, positions the review gutter
   outside that column, and the visual contract guards against regressions.
   Any remaining paragraph shrink-wrap behavior should be handled before
   broader toolbar, backend, or test expansion, because incorrect paragraph
   width breaks the core typing, clicking, selection, and wrapping model.
   The page box model is now explicit too: the document page uses border-box
   sizing, and direct child block shells have no residual max-width cap, so the
   writable paragraph target spans the full page text column rather than a
   narrow inline widget. The page and ruler now share explicit page-width,
   page-padding, and text-column CSS variables; wrapped paragraph shells,
   editable block content, paragraphs, headings, and lists explicitly remain
   block-level, full-width, non-shrink-wrapped writing targets, and
   `node apps/desktop/scripts/visual-contract.mjs` rejects shrink-wrapped or
   inline paragraph flow. Nested inline-run editors inside the full-width block
   editor must not draw their own tiny contenteditable hover/focus boxes; the
   visible writing surface is the page-width paragraph, with inline runs only
   carrying text marks, selection, object, and source IDs. Non-empty paragraphs
   must rely on the browser/native caret inside the text, while the synthetic
   green caret is reserved for empty full-width paragraphs where there is no
   text node to anchor visually. Rendered block shells now expose
   `data-doc-block-flow="full-width"` and editable content exposes
   `data-doc-text-column="full-width"`, with
   `npm --prefix apps/desktop run gui-smoke` asserting that normal editor
   startup includes those full-width writing targets. Full-width text-column
   clicks now have an explicit caret fallback too: if a coordinate-bearing
   browser click cannot be mapped to a point-derived caret offset, the click
   focuses the paragraph at its source-backed text end instead of reusing stale
   caret metadata from an earlier edit. Explicit event caret offsets now share
   the same numeric and numeric-string parsing path as canvas fallback drops,
   so browser and test-driven caret placement stay consistent. Restored caret
   offsets and projected keyboard-selection ranges use the same parser and clamp
   to the current source-backed paragraph bounds, so stale or out-of-range
   selection metadata degrades to a valid edit range instead of falling back to
   unrelated caret state. Clicks on the full-width block shell for editable
   paragraphs, headings, and list items now delegate focus into the inner editor
   node, use an explicit event caret offset when supplied, and otherwise restore
   the caret at the source text end, so clicking the blank/right side of a
   page-width line behaves like a writable paragraph rather than merely
   selecting the block shell. Responsive page
   metrics now reduce the page padding
   at 900px and 720px viewport widths, and the mobile document workspace uses
   tighter padding, so the full-width paragraph contract remains usable on
   narrow windows instead of preserving desktop print margins until the text
   column becomes cramped.
   The next implementation rounds should stay on the normal writing loop first:
   click-to-place-caret with the actual paragraph offset reflected in render
   state, type, select, replace, delete, copy/cut/paste, split/join
   paragraphs, continue lists/headings, undo/redo, and preserve
   formatting marks while doing those edits automatically through operations.
   Collapsed-caret formatting must behave like a normal editor: toggling Bold,
   Italic, or similar marks before typing must affect subsequent typed and
   pasted text without mutating existing text, and turning a mark off must keep
   the next inserted text plain even when it follows a formatted run. Browser
   `beforeinput` formatting events such as `formatBold` must follow the same
   direct-inline and block-editor semantics as keyboard shortcuts, and native
   list commands such as `insertOrderedList`/`insertUnorderedList` must toggle
   list state without losing draft text or caret position; native
   `formatIndent`/`formatOutdent` plus Tab/Shift+Tab must adjust list depth
   through the same operation-backed list item path.
   Paragraph creation, heading/list continuation, page breaks, inline objects,
   comments, suggestions, citations, and basic formatting must be reachable from
   keyboard/slash/context interactions. Canvas fallback focus must also support
   Ctrl/Cmd+Enter page-break insertion through source operations while restoring
   focus to the originating paragraph, and common typed shortcuts such as
   `# `, `- `, `* `, and numbered list markers must transform the focused
   paragraph through the same source shortcut path instead of committing marker
   text. Backspace at the start of canvas-focused headings and list items must
   follow normal editor semantics by converting headings to paragraphs,
   outdenting or exiting list items, and never deleting text from the end of the
   block when the caret is at offset zero. Direct inline text runs and whole-block
   keyboard editors must expose the same writing semantics where users can see
   them: keyboard shortcuts such as Ctrl/Cmd+Enter for page breaks must commit
   the visible draft and restore the caret without requiring toolbar actions,
   and slash commands typed into direct inline text must execute the same
   operation-backed block/list/page-break transformations as the block editor.
   Selected structural objects such as page breaks, images, equations, and
   tables must also accept actual browser paste/drop events by inserting
   operation-backed paragraphs after the object and preserving compatible
   single-line rich marks.
   Canvas fallback ArrowUp/ArrowDown must also move between adjacent editable
   document blocks with nearest-column caret restoration, so page-level focus
   behaves like document editing rather than a detached shortcut surface.
   Shift+ArrowUp/Down from the same canvas fallback should project the shared
   inline-range selection across adjacent paragraphs, so selection, copy,
   formatting, deletion, and replacement use one operation-backed range model.
   Canvas keyboard/browser-native formatting commands and format removal must
   target that range before falling back to collapsed-caret pending marks.
   Printable canvas typing, IME composition commits, browser-native insert
   text/paste/drop, soft line break, Enter, Ctrl/Cmd+Enter page-break insertion,
   and Backspace/Delete over that range must consume the selected source range
   instead of inserting at a stale fallback caret. Escape over the same range
   must collapse it back to a normal caret without changing source text.
   Plain-text copy/cut for paragraph-spanning ranges must preserve paragraph
   boundaries with newline separators, rather than concatenating unrelated
   paragraphs. Native list commands over paragraph-spanning canvas ranges must
   convert or toggle every selected paragraph/list item through
   `set_block_text_style`, preserving a single range model and predictable
   editor focus. Native indent/outdent commands over selected list-item ranges
   plus Tab/Shift+Tab from canvas focus must update every selected list item
   level together.
   Canvas fallback Shift+ArrowDown and Shift+ArrowUp now both have smoke
   coverage for paragraph-spanning selection, including the reverse direction
   where focus starts in the later paragraph and lands on the previous
   paragraph editor. Browser-native typing over that reverse range replaces
   the document-ordered source range and restores focus/caret to the first
   selected paragraph. Browser-native `beforeinput deleteContentBackward` over
   the same canvas paragraph range now deletes the selected source range and
   collapses focus/caret to the first selected paragraph, matching keydown
   Backspace. Browser-native `beforeinput insertParagraph` over a reverse canvas
   paragraph range now clears the first selected paragraph, deletes later
   selected inlines, inserts a following paragraph, and restores focus/caret to
   that inserted paragraph. Shift+Enter and browser-native `beforeinput
   insertLineBreak` over a canvas paragraph range now replace the selected
   source range with a soft line break in the first selected paragraph, delete
   later selected inlines, and restore the caret after that newline. Native
   `copy` events over a canvas
   paragraph range now write newline-separated paragraph text without consuming
   the selected range. Native `cut` events over the same range copy the same
   text, delete the selected source range, and restore focus/caret to the first
   selected paragraph at the deletion point. Rich browser-native
   `beforeinput insertFromPaste` and native `drop` over a reverse canvas
   paragraph range, plus browser-native `beforeinput insertFromDrop` over a
   forward canvas paragraph range, now also replace the document-ordered
   selected source range, preserve compatible inline marks, delete later
   selected inlines, and restore focus/caret to the first selected paragraph.
   Plain ArrowLeft/ArrowRight inside editable document blocks are now
   source-aware too: they commit visible draft text, preserve document focus,
   update the restored caret offset, and avoid leaving toolbar or subsequent
   edit commands pointed at stale caret state.
   ArrowUp/ArrowDown inside a paragraph with soft line breaks now move between
   source line offsets before crossing block boundaries, from both block focus
   and canvas fallback focus, so multi-line paragraphs keep predictable caret
   state after render. Shift+ArrowUp/Down over those same soft lines now
   projects a source-backed selection range before escalating to
   cross-paragraph selection.
   Arrow-key boundary navigation must also commit visible draft text and move
   cleanly between direct inline text, editable paragraphs, and structural
   objects such as page breaks. Rich paragraphs with multiple inline runs must
   preserve clicked caret offsets inside individual runs, allow left/right
   caret movement across formatting/link/citation boundaries without trapping
   the user inside one run, and Shift+Arrow selection across
   those boundaries must preserve visible draft text before applying selection,
   formatting, clipboard, or deletion operations. Backspace/Delete at those
   inline-run boundaries must also remove neighboring characters through normal
   operation-backed text updates before falling back to paragraph joins.
   Multi-line paste/drop into
   direct inline document text must follow normal document semantics: the first
   pasted line stays in the current paragraph and later lines become following
   paragraphs, while table cells keep inline newlines. Direct inline
   paste/drop must handle both browser `beforeinput` and actual `paste`/`drop`
   events through the same operation-backed rich/plain insertion paths,
   including active inline-range replacement and caret restoration. Whole-block
   keyboard editors must do the same for actual browser `drop` events, not only
   `beforeinput insertFromDrop`, so dragging text onto the paragraph canvas
   preserves compatible formatting and restores the source caret.
   The product criterion for the next rounds is not more coverage volume; it is
   that a user can click the page and write without manually adding paragraph
   blocks from toolbar/menu controls. Treat tests as change-local guards for the
   behavior just implemented; broad fuzz, merge, and compatibility suites can
   wait until the writing surface has ordinary editor feel. Empty paragraphs created during editing
   must also disappear through normal Backspace semantics from either the block
   editor or direct inline text, including browser-native `beforeinput`
   deletion paths. Keep verification mostly limited to narrow smoke checks that
   protect the current writing loop; broader GUI, merge, fuzz, and conformance
   tests can be added later after ordinary typing, selection, clipboard,
   paragraph flow, and visible caret behavior feel like a normal document
   editor. Empty or textless imported/opened documents must also fail into a
   writable state: the page canvas itself now exposes a focusable empty-caret
   target, click/Enter creates the first empty paragraph, printable key input
   and browser-native `beforeinput insertText` create the first paragraph with
   typed text, IME composition stays browser-local until `compositionend`, and
   paste/drop through native events or `beforeinput` creates normal top-level
   paragraphs for each pasted or dropped line before restoring normal caret
   focus. Actual browser `drop` on that empty canvas is part of the writable
   page contract: it must prevent browser-only DOM insertion, create
   operation-backed paragraphs, and leave focus/caret in the final dropped
   paragraph so writing can continue without an add-paragraph toolbox action.
   Paragraph keydown text insertion now refuses to synthesize ordinary
   printable-character operations in modern browsers, even if prior focus
   restoration left `data-caret-offset` metadata on the editable block.
   Browser text entry must flow through `beforeinput`/`input`, where the real
   DOM selection is available; synthetic keydown insertion remains only for
   non-browser smoke/fallback environments. This prevents ordinary keypresses
   from jumping insertion back to the start of a paragraph.
   Vertical ArrowUp/ArrowDown between full-width paragraphs now moves to the
   adjacent editable paragraph from the current source column, not only from
   paragraph start/end boundaries, so single-line paragraphs no longer trap the
   caret in the current block.
   Backspace/Delete-driven paragraph deletion and joins are normal editor
   actions, not audit navigation: `delete_block` no longer opens the right-side
   audit panel automatically, and smoke coverage keeps Backspace deletion in
   the document canvas with the side panel closed.
   Compatible single-line rich paste/drop into the same empty-canvas state must
   create operation-backed inline text runs with preserved marks, rather than
   flattening formatting or relying on transient browser DOM.
   When the page canvas itself receives focus in an existing document, ordinary
   typing, browser-native `beforeinput`, Enter, paste/drop, Backspace, and
   deferred IME composition route into the selected or last editable paragraph, and
   canvas-level Ctrl/Cmd+Z/Y plus `beforeinput historyUndo/historyRedo` route
   through app history instead of being ignored by the canvas. Canvas-level
   formatting shortcuts and browser-native `beforeinput format*` events must
   follow the focused fallback paragraph caret or projected text selection,
   rather than the paragraph end: selected text is formatted through normal
   range operations, and collapsed-caret toggles affect only subsequent inserted
   text at that caret. Canvas-level format removal must clear the projected
   selection or pending caret marks without mutating unrelated text.
   Canvas-level Ctrl/Cmd+C/X/V must use document clipboard semantics too:
   copy/cut use an active inline range when present or the whole fallback
   paragraph otherwise, cut clears the source text through operations, and
   paste inserts the internal document clipboard at the fallback caret.
   Actual browser `copy` and `cut` events on the focused document canvas now
   share that same source-backed clipboard transfer path, including OS clipboard
   data when the event exposes `clipboardData`.
   Keyboard-editable paragraph clipboard helpers now also use source-backed
   block text metadata after rerender when transient DOM `textContent` is
   unavailable, so projected selection copy/cut/paste does not depend on
   browser-only DOM state.
   The same metadata fallback must be used by multi-run selected-range
   replacement and deletion, so beforeinput text/paste/drop/delete over a
   rendered projected selection clamps against source text rather than empty
   transient DOM text.
   Browser-native word and line deletion in keyboard-editable paragraphs must
   use the same source-backed paragraph text after rerender, so
   `beforeinput deleteWord*` and line-deletion events do not become no-ops just
   because transient DOM `textContent` is empty.
   Collapsed-caret edits inside formatted multi-run paragraphs must also build
   projected block text from source metadata after rerender, so ordinary
   `beforeinput insertText`, Backspace, Delete, and transpose operations remain
   operation-backed even when the browser DOM has not repopulated text content.
   The generic simple-paragraph `beforeinput` fallback follows that rule too:
   insertion, replacement, Backspace, Delete, cut, and drag deletion must edit
   the source paragraph text when transient DOM text is empty after render.
   Explicit caret offsets from tests, click mapping, browser events, and
   restored focus must be clamped against source-backed editable text, not just
   current DOM `textContent`, so mid-paragraph edits after render do not jump to
   offset zero. The shared browser-native edit projection helper must use the
   same restored/source-backed caret fallback for typed text, replacement text,
   paste/drop, cut, drag deletion, and character deletion, so rerendered
   full-width paragraphs and direct inline runs edit at the intended source
   offset rather than appending at the end.
   Enter and browser-native `beforeinput insertParagraph` must also split from
   source-backed paragraph text after rerender, and multiline paste into
   formatted multi-run paragraphs must carry the source suffix into the trailing
   inserted paragraph instead of dropping it when DOM text is empty. Block-level
   `beforeinput insertParagraph` must accept restored/source-backed caret
   metadata too, so a rerendered full-width paragraph splits at the expected
   source offset even when its transient DOM `textContent` is empty. Plain
   keydown Enter uses the same fallback, so the non-`beforeinput` keyboard path
   does not diverge from browser-native paragraph splitting after rerender.
   Paragraph boundary ArrowLeft/Right/Up/Down navigation and Backspace/Delete
   joins must also use restored/source-backed caret metadata, so full-width
   paragraphs can move across text, page breaks, and adjacent paragraphs after a
   render pass without needing a live DOM selection.
   Multi-run formatted paragraphs must use restored/source-backed caret
   metadata for collapsed browser-native text insertion and multiline paste as
   well, so formatting run boundaries do not make normal writing append at the
   wrong position after a render pass.
   List toggles, empty-block checks, Delete-at-end paragraph joins, slash
   palette text, shortcut draft commits, navigation boundaries, and
   Ctrl/Cmd+Enter page-break insertion must use the same source-backed
   paragraph text after rerender, so a full-width paragraph target behaves like
   a real writing surface rather than a transient DOM buffer. Generic full-width
   paragraph `input`/blur draft commits now use that source-backed paragraph
   reader too, so an empty transient contenteditable node after rerender cannot
   be persisted as document deletion while real non-empty draft edits still
   commit normally. Full-width paragraph blur now uses the same commit path
   without forcing focus back to the blurred paragraph, so click-away commits
   source edits while restored transient empty DOM still degrades safely.
   Direct inline text runs must follow the same rule: Home/End, word and
   character selection, paste/drop, formatting removal, selected-range collapse,
   run-boundary navigation, and run-boundary deletion must use source-backed
   inline text after rerender so formatted paragraphs stay editable even when
   the browser has temporarily cleared a direct inline text node. Direct inline
   draft commits must also use the source-backed value so navigation, list
   conversion, range indentation, page-break insertion, and other operation
   handoffs cannot accidentally persist an empty transient DOM node as real
   document deletion. Direct inline clicks now use the same event/restored/source
   caret fallback helper as block clicks, including numeric-string event caret
   offsets. Coordinate-bearing inline clicks that cannot be point-mapped now
   fall back to the inline source text end instead of stale caret metadata, so
   formatted/link runs and paragraph blocks do not diverge in caret placement
   semantics.
   Direct inline browser-native `beforeinput insertParagraph`, pending-mark
   insertion, shortcut formatting, and soft-line insertion must also use
   restored/source-backed caret metadata after a render pass; an empty transient
   inline `textContent` is not allowed to turn Enter into a whole-run move,
   source-text deletion, formatting at a stale caret, or newline at the wrong
   offset. Direct inline `Shift+Enter` and browser-native `beforeinput
   insertLineBreak` now both commit soft line breaks through the source-backed
   inline text path, with smoke coverage for empty transient text nodes that
   still carry restored caret metadata.
   Canvas-level Ctrl/Cmd+A must project a fallback paragraph text selection
   that typing, paste, Backspace/Delete, copy, cut, and Enter consume through
   source operations rather than browser-only selection; Enter over that
   projected selection must remove the selected text
   before inserting and focusing the following paragraph. Canvas-level
   Shift+Enter and browser-native `beforeinput insertLineBreak` must insert soft
   line breaks into the fallback paragraph, including replacing projected
   fallback text selections, instead of accidentally creating a new paragraph.
   Escape over a canvas-projected fallback selection must clear only the
   projected selection and restore the paragraph caret without mutating source
   text. IME composition on the focused canvas must also preserve and replace a
   projected fallback selection when composition commits. Canvas-level Home/End
   and Left/Right arrows, including Ctrl/Cmd word navigation, must restore a
   real fallback paragraph caret offset, Shift+Left/Right and Shift+Home/End
   must project fallback text selections from that offset, and subsequent
   typing, soft line breaks, Backspace/Delete, Ctrl/Cmd+Backspace/Delete, and
   browser-native
   `beforeinput deleteWord*` must edit at that offset or replace that selection
   rather than always appending to or deleting from the paragraph end. Multi-line
   paste/drop through the focused canvas must also split paragraphs at the
   fallback caret offset and carry the suffix into the final inserted paragraph.
   Spreadsheet
   cells must behave like spreadsheet cells: show computed results or literal
   values by default, enter edit mode explicitly, expose a visible cell
   cursor/selection while editing, and keep formula source editing in the
   formula bar or active cell edit state instead of making every grid cell look
   like a generic form field. Entering edit mode with F2, Enter, or double-click
   must restore the caret at the end of the source value, including formulas
   whose grid cell displays a computed result. Browser-native `beforeinput`
   events on display-mode spreadsheet cells must match keyboard behavior too:
   insert text starts source editing with the typed seed, paste/drop writes TSV
   cells through spreadsheet operations, and Backspace/Delete clears the
   selected cell or range while keeping the grid in display mode. Native
   active-cell `beforeinput insertText` must use restored/source-backed caret
   metadata after rerender, so formula/value editing inside the grid inserts at
   the intended source offset even when transient editor text is empty. Formula
   bar and name-box draft editing must use the same restored-caret fallback for
   insertion and paste, keeping the spreadsheet editor deterministic when the
   browser selection is unavailable after render.
   Native
   `beforeinput insertParagraph`/`insertLineBreak` on a selected display cell
   must enter source editing with the caret at the end of the cell source,
   matching explicit Enter/F2 edit entry. Browser-native
   `beforeinput formatBold`/`formatItalic`/`formatRemove` on a display cell
   must also route through spreadsheet formatting operations, not browser-only
   contenteditable formatting, and keyboard/native formatting from a focused
   display cell must preserve and apply to the active selected range when that
   focused cell is inside the range. Actual browser `dragover`/`drop` events on
   spreadsheet cells must follow the same rule: display-mode drops write TSV
   through spreadsheet operations, while drops inside an active cell editor
   update the pending source edit and keep edit mode active until commit.
   Actual browser `copy`/`cut` events on display-mode spreadsheet cells now use
   the same TSV range clipboard model as keyboard shortcuts and menus: copying
   a focused cell inside a selected range writes that range, while cutting also
   clears the selected source cells through spreadsheet operations. In active
   cell edit mode, native `copy`/`cut` events operate on the selected draft text
   inside the cell editor instead: copy leaves the draft untouched, cut removes
   the selected draft substring, restores the cell-editor caret, keeps edit mode
   active, and does not commit the source cell until the normal commit event.
   The formula bar follows the same draft-first clipboard rule: native copy/cut
   use the selected formula/source draft text, preserve formula-bar focus, keep
   the change pending, and leave source-cell commitment to Enter/blur.
   The name box follows the same draft-aware native clipboard rule for selected
   range text: copy/cut work on the visible or restored draft, cut keeps focus
   in the name box, restores the caret, and does not navigate the sheet until
   the user commits the draft. Editable sheet titles now also handle native
   copy/cut through the controlled title-edit path: copy exports the selected
   title text, while cut removes the selected title substring with the same
   operation-backed rename, focus, and caret restoration semantics as paste,
   drop, and browser-native `beforeinput`.
2. Finish visible document text entry before broadening the prototype again.
   The next slices should make direct inline editing cover the remaining normal
   editor behaviors: caret restoration after every source-backed command and
   reliable plain/rich paste into existing paragraphs. Direct inline IME
   composition is source-backed; Ctrl/Cmd+Home/End now commits the visible inline
   draft and navigates to the first/last editable inline in the document;
   Shift+ArrowUp/Down now commits direct-inline draft text and extends selection
   into the previous/next paragraph inline through the shared inline-range model,
   and keyboard/browser-native/toolbar formatting, browser-native format
   removal, copy, cut, direct typing, native replacement text, native
   paste/drop, single-line rich HTML paste/drop for common text marks in direct
   inline spans, simple keyboard-editable paragraphs, and collapsed-carets in
   multi-run keyboard-editable paragraphs, plus selected multi-run rich
   replacement in keyboard-editable paragraphs, selected active-inline-range
   rich replacement from keyboard-editable paragraphs, selected direct-inline
   range rich replacement, canvas active-range rich replacement, and
   canvas-fallback paragraph caret paste,
   browser-native drag deletion, Backspace, Delete, Enter, or
   Shift+Enter/line-break over that paragraph-spanning range formats, clears
   formatting, reads, replaces, clears, splits, or inserts a soft break through
   source operations while preserving the expected editor focus. Comments and
   insert suggestions created from the same direct-inline range now preserve the
   selected text-range UUID anchor instead of falling back to document-level
   review state, expose those anchors in the side panel for audit/recovery
   visibility, and have matching Rust app API, browser mock, Tauri command
   manifest, and command-contract coverage. Delete and format suggestions now
   have the same range-aware source commands, GUI routing, and visible UUID
   range labels, and browser fallback acceptance now applies them over the same
   inclusive document-order inline range as the merge engine, so track-changes
   actions do not collapse paragraph-spanning selections to one inline. Escape
   over the same range clears only the range,
   commits any visible
   direct-inline draft, and restores direct-inline focus without rolling text
   back; and
   word-wise Backspace/Delete now crosses adjacent formatted text runs from both
   keydown and browser-native `beforeinput deleteWord*` paths. Direct-inline
   native list commands over paragraph-spanning ranges must convert or toggle
   every selected paragraph/list item through `set_block_text_style`, matching
   the canvas range model instead of collapsing to the focused inline.
   Tab/Shift+Tab and browser-native `beforeinput formatIndent`/`formatOutdent`
   over a selected list-item range must also indent/outdent every selected list
   item through the same operation-backed list path as canvas fallback and
   native indent/outdent commands. This is the near-term gate before investing
   in large new test suites or secondary GUI polish.
   Direct-inline browser-native rich multiline paste now splits pasted formatted
   lines into operation-backed paragraphs, preserves marks on the inserted runs,
   moves the original suffix to the final paragraph, and restores the caret
   before that suffix from both `beforeinput insertFromPaste` and native
   `paste` events, with matching native `drop` coverage for formatted multiline
   text drops, including actual direct-inline drops over a paragraph-spanning
   selection; `timeout 260s node apps/desktop/scripts/gui-smoke.mjs` covers
   these paths.
   Keyboard-editable full-width paragraph paste now follows the same rich
   multiline semantics: formatted pasted lines become operation-backed
   paragraphs, marks are preserved on inserted runs, the original suffix moves
   to the final inserted paragraph, and the caret is restored before that suffix;
   the same GUI smoke command covers this path. Actual browser `drop` on the
   same full-width keyboard editor is covered for both plain multiline text and
   compatible rich multiline HTML: the operation-backed path splits paragraphs,
   preserves marks when available, carries the original suffix to the final
   inserted paragraph, and restores source focus/caret according to the insertion
   mode.
   Empty-canvas rich multiline paste also creates one operation-backed paragraph
   per pasted formatted line, preserves the first/second line marks, focuses the
   final inserted paragraph, and restores its caret at the pasted line end; the
   same GUI smoke command covers this path.
   Active inline-range rich multiline replacement now uses the same source
   operation model instead of falling back to plain text: the first pasted rich
   line replaces the first selected inline, later rich lines become following
   operation-backed paragraphs, selected trailing inlines are deleted, inserted
   marks are preserved, and focus/caret lands on the final pasted rich run. The
   GUI smoke test covers this with direct-inline and keyboard-block
   paragraph-spanning selections, including a final pasted line split across
   multiple formatted runs so keyboard focus restores a block-local caret at the
   end of the full line rather than an inline-local offset.
3. Add context-dependent right-click menus for document, table, spreadsheet,
   citation, comment, suggestion, and blob targets. Move low-frequency or
   target-specific commands out of always-visible toolbar chrome where that
   reduces button count, especially table row/column insertion/removal and
   spreadsheet row/column actions. Context menus must be operation-backed,
   keyboard-accessible, and covered by GUI smoke tests.
4. Replace raw-only formatting controls with compact native-feeling controls:
   color commands must expose a small palette plus raw color entry and an
   uncolor/clear-color option; bold and similar binary marks must behave as
   selected-state toggle buttons; remove/delete actions should be consolidated
   into context menus, dropdowns, or selection-aware controls where possible
   instead of many separate `Remove X` buttons.
5. Continue refining menu and toolbar chrome toward Google Docs/Sheets shape:
   keep file/edit/insert/format/sheet/review/storage actions in menu chrome,
   selection-sensitive document and spreadsheet actions in the active toolbar,
   use icon-first buttons for almost all toolbar and right-side chrome actions,
   show icons beside menu items, and prevent the removed left command rail from
   returning. Done when smoke and visual-contract checks prove all formerly
   rail-only commands remain reachable through menu/toolbar entries, menus close
   when another menu opens or the user clicks outside them, and icon buttons keep
   accessible labels/tooltips without increasing chrome height.
   The shared View toolbar must include a compact document/workbook zoom/scale
   slider specifically for view zooming only, with 75-150% range, visible percent,
   fit-width and fit-page presets, and smoke coverage proving direct slider
   input updates the rendered Docs/Sheets workspace without changing signed
   source state.
   Disabled actions must be suppressed and cancel their originating event so
   unavailable runtime features cannot leak fallback commands from nested home,
   menu, or toolbar controls.
6. Rework the Tauri/browser GUI from prototype dashboard into a minimal
   home/document picker plus primary Google Docs/Sheets-like workspaces:
   the first page lists openable documents and has create/open/import actions,
   while the editor view provides document canvas, spreadsheet grid, dense
   menu/toolbar chrome, title/status area, panels, and real workflow entry
   points only after a document is opened, created, or imported. The desktop
   web dev server now defaults to `0.0.0.0:10084` for browser testing, while
   Tauri dev connects to `127.0.0.1:10084`; this is the default test endpoint
   for interactive GUI work.
7. Expand operation coverage until every visible GUI control is operation-backed
   and renders immediately without persistence batching.
8. Add GUI smoke/visual checks that prove the launch view has open/create/import
   actions and the editor first viewports are Docs-like and Sheets-like,
   including toolbar, canvas/grid, selection, status, and no overlapping text.
9. Finish canonical binary records and validation for all v0 source nodes.
10. Build merge simulations and fuzz tests for formatted documents before relying on UI behavior.
11. Complete local disk object repository semantics before making S3 default.
12. Continue signing work for manifests, exact-byte blobs, and typed semantic profiles in Rust; postpone browser signing.
13. Use `citum` for citations now and leave CSL adapters for later.

## Execution Roadmap

Work in this order unless a test failure proves the order wrong:

1. Lock the v0 source model and binary records for documents, spreadsheets, citations, equations, comments, suggestions, blobs, manifests, operation segments, lookup records, tombstones, packs, and signatures.
2. Make paragraph page flow correct before broadening toolbar/panel work: every
   paragraph, heading, and list item must span the full usable page text column,
   wrap naturally, expose a full-line click target, keep the caret at the clicked
   text offset, and prove this through GUI smoke or visual-contract checks.
   This is the next GUI priority if the rendered app shows narrow paragraph
   widgets: the product must treat narrow paragraphs as a blocker, keep zoom and
   side-panel layout from shrinking the writing column, and only resume toolbar
   polish after the page-width paragraph contract is visually and mechanically
   proven. Narrow paragraphs have no intended product semantics in the Docs
   editor; if they appear, the renderer, CSS, zoom shell, or editable-block
   wrapper is wrong and the fix belongs before additional editor features. GUI
   smoke must assert that rendered block shells and keyboard-editable paragraphs
   carry full-width page-flow markers, while visual checks must reject shrink-wrap
   CSS such as `fit-content`, `max-content`, or inline paragraph display.
   Restored carets for these full-width editable blocks must clamp against the
   canonical source block text, not a transient contenteditable DOM snapshot, so
   a rerender cannot make a real paragraph look empty or force the caret to the
   start of the line. Document zoom must apply only to the ruler/page stack or
   sheet grid, not to editor chrome or the document outline.
3. Make the Tauri/browser document canvas writable in the normal Google Docs sense before broadening: focused empty pages, visible caret, click-to-type, typing, selection, replacement, deletion, Enter, clipboard, inline formatting, IME, and caret restoration must work without block toolbox actions.
   Browser keydown handlers must not synthesize printable character insertion
   when `beforeinput`/`input` is available; this keeps typing at the browser
   caret instead of jumping to the start of the paragraph. Backspace/Delete
   joins and selected-object deletion must not open the audit/right-side panel.
4. Bring the surrounding GUI into Google Docs/Sheets shape early: minimal home/document picker, app chrome, document canvas, spreadsheet grid, toolbars, panels, selection affordances, status indicators, and smoke-testable first viewports.
   Debug-only selection metadata and green focus/selection boxes must be hidden
   by default and exposed through a View-menu debugging toggle. The default
   document surface should be paged, with multiple page boxes when content
   overflows; an infinite-page mode may exist later but must not be the default.
   Empty toolbar slots should be removed or collapsed so the toolbar reads as
   dense product chrome rather than a prototype layout.
5. Make every schema feature editable through `opendoc-app-api` operations, including undo/redo and deterministic warning behavior, and wire each GUI control to those operations.
6. Build merge simulations before optimizing persistence: concurrent typing, formatting, comments, suggestions, citations, equations, tables, and spreadsheet edits must merge automatically.
7. Harden local on-disk repositories first: save/open/reopen, candidate heads, DOI/UUID lookup, scan fallback, shallow clone warnings, blob reuse, pack compaction, and archive tombstones.
8. Add S3/OpenDAL conformance only after the local object-store semantics pass the same repository tests.
9. Finish signing in Rust: version manifests, exact-byte blobs, typed semantic profiles, multiple signers, trust states, and tamper detection. Browser signing remains deferred.
10. Expand citations around the document-local bibliography database and `citum`; keep CSL/CSL-JSON as import/export adapters.
11. Expand spreadsheets until formulas, dependencies, named ranges, comments, filters, protected ranges, validations, frozen panes, merged ranges, and import/export fixtures pass.
12. Build Google Docs/Sheets-shaped import/export fixtures and practical `.doc`/`.docx` import fixtures; unsupported unsafe imports abort, recoverable gaps warn.
13. Reuse the same app API in browser-local mode, HPC single-user mode, and multi-user service mode. Permissions stay service/runtime state, never signed document state.
14. Add multi-user service capabilities last: authentication, authorization, sharing, presence, lookup acceleration, sync relay, and optional server-side commit serialization.

Each roadmap item is complete only when the implementation is linked from this file with the exact verification command or fixture name that proves it.

## Open Risks

- The merge model may need a custom state-machine design rather than an off-the-shelf CRDT.
- Formatting ranges, comments, suggestions, and citations around concurrent text edits are the hardest merge cases.
- Local filesystems dislike many small objects; pack files and compaction must be tested early.
- Raw S3 without a commit server needs candidate-head reconciliation and scan fallback to be reliable.
- Tape/archive recovery needs enough tombstone metadata to be useful without making normal repositories heavy.
- Browser key handling for signing is intentionally undecided.
- Google Docs/Sheets import/export subset must be proven by fixtures, not assumptions.

## Trackable Completion Matrix

Use this matrix to tell whether the plan is finished:

| Gate | Required proof |
| --- | --- |
| Research | ADRs or test spikes select or defer merge model, binary format, storage layout, signing boundaries, citation model, spreadsheet subset, import/export scope, and runtime modes. |
| Source | Canonical binary round-trip tests cover every v0 source node and reject or repair invalid states with deterministic warnings. |
| Operations | Every visible edit in the app is represented as an operation and can render immediately without persistence batching. |
| Merge | Synthetic scenarios and fuzz tests for 1-3 active editors converge byte-for-byte at canonical projection level for rich docs and sheets. |
| Storage | Local and flat object-store tests pass for heads, candidates, manifests, snapshots, operation segments, blobs, lookup, packs, shallow clone, and tombstones. |
| Signing | Save/open tests verify unsigned open, multiple signatures, version signatures, blob sidecars, semantic profiles, trust states, and tamper detection. |
| Citations | Tests cover document-local bibliography updates, structured citation labels, `citum` rendering, save/open, deletion/restore, and merge behavior. |
| Spreadsheets | Tests cover formulas, dependencies, named ranges, comments, filters, protections, validations, frozen panes, merged ranges, signatures, and Sheets-shaped fixtures. |
| Import/export | Google Docs/Sheets-shaped and `.doc`/`.docx` fixtures pass, warn, or abort deterministically. |
| Frontend | Tauri and browser command contracts expose every v0 feature; GUI smoke/visual checks prove Docs-like and Sheets-like first viewports, operation-backed controls, document and spreadsheet workflows, and no overlapping text. |
| Runtime modes | Browser-local, HPC single-user, and multi-user service wrappers pass shared app API and repository conformance tests. |

## Completion Gates

1. Research gate: merge approach, binary encoding, storage layout, signing boundaries, citation model, spreadsheet subset, import/export scope, and runtime modes have recorded decisions or test spikes.
2. Source gate: all v0 source nodes and invariants round-trip through canonical binary encoding.
3. Operation gate: every user-visible edit is represented as an operation and renders without waiting for persistence batching.
4. Merge gate: synthetic and fuzz tests converge automatically for 1-3 active editors across rich documents and spreadsheets.
5. Storage gate: local disk and flat object-store conformance tests pass for heads, candidates, manifests, snapshots, operation segments, blobs, lookup, packs, shallow clone, and tombstones.
6. Signing gate: unsigned open, manifest/version signatures, blob sidecars, semantic profiles, trust states, and tamper detection pass after save/open.
7. Citation gate: document-local bibliography, structured labels, `citum` rendering, merge, update, and save/open behavior are tested.
8. Spreadsheet gate: formulas, dependency invalidation, named ranges, comments, filters, protections, validations, frozen panes, merged ranges, and Google Sheets-shaped fixtures pass.
9. Import/export gate: Google Docs/Sheets-shaped fixtures and practical `.doc`/`.docx` fixtures pass or abort/warn deterministically.
10. Product gate: Tauri, browser, HPC, and service modes pass shared app API and repository tests; Tauri GUI looks and behaves like a serious Docs/Sheets-style editor and can create/edit every v0 schema feature.

The product is finished only when all gates pass in CI or named local verification commands and this file links each gate to the corresponding implementation evidence.
