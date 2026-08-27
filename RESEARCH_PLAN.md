# OpenDoc Research Plan

This project aims to design an open source, Rust-first alternative to Google Docs and Google Sheets with collaborative editing, single-user use, local offline use, object-storage persistence, and cryptographically signed document versions.

The research should produce design constraints and prototype decisions, not a clone of Google's internal implementation. Google Docs and Sheets APIs are useful because they expose the public document model and update surface, even though they do not expose Google's full storage or collaboration internals.

## Definition Of Done

This research plan is finished when every item below is true:

- Each research track has a completed note under `docs/research/` with source links, findings, rejected options, open risks, and a recommendation.
- Each required schema draft under `docs/schema/` exists and contains enough JSON examples to validate the intended shape.
- Each prototype listed in this plan exists, can be run locally, and has a short README or command note.
- Each core question in this file has a direct answer recorded in `docs/adr/` or in the relevant research note.
- Any feature deliberately excluded from the Google Docs or Sheets subset is listed with a reason.
- Cryptographic signing has a documented threat model, signed object boundary, canonicalization rule, and verification workflow.
- Storage has a documented object layout for raw S3-compatible mode, local offline mode, and server-mediated collaboration mode.
- Citations have a documented internal model based on CSL-JSON or a justified alternative.
- The final summary file `docs/research/summary.md` links all outputs and states whether the project should proceed to implementation.

The plan is not finished merely because sources have been read. It is finished only when the repository contains the artifacts above and the ADRs make the major design choices explicit.

## Completion Checklist

- [x] `docs/research/google-docs-schema.md`
- [x] `docs/research/google-sheets-schema.md`
- [x] `docs/research/collaboration-model.md`
- [x] `docs/research/storage-layout.md`
- [x] `docs/research/signing-model.md`
- [x] `docs/research/citations.md`
- [x] `docs/research/frontend-options.md`
- [x] `docs/research/summary.md`
- [x] `docs/schema/document-v0.md`
- [x] `docs/schema/spreadsheet-v0.md`
- [x] `docs/schema/storage-manifest-v0.md`
- [x] `docs/schema/signed-manifest-v0.md`
- [x] `docs/schema/citation-v0.md`
- [x] `docs/adr/0001-collaboration-core.md`
- [x] `docs/adr/0002-storage-and-signing.md`
- [x] Rich-text collaboration prototype
- [x] Spreadsheet grid/collaboration prototype
- [x] Signed manifest verification prototype
- [x] Citation rendering prototype

## Core Questions

1. What minimal document schema covers the practical Google Docs subset?
2. What minimal spreadsheet schema covers the practical Google Sheets subset?
3. Should the collaboration layer be operation-based, CRDT-based, or a hybrid of CRDT updates plus signed snapshots?
4. How should document versions be stored in raw S3-compatible object storage, including offline and single-user modes?
5. What exactly should be signed: every operation, every snapshot, a manifest, or an append-only version log?
6. How should citation metadata be embedded so citations survive collaborative edits, reformatting, export, and local offline use?
7. Which Rust crates or external standards are mature enough to adopt rather than implement from scratch?

## Research Tracks

### 1. Google Docs Public Model

Primary source:

- Google Docs API `documents` resource: https://developers.google.com/workspace/docs/api/reference/rest/v1/documents
- Google Docs structure guide: https://developers.google.com/workspace/docs/api/concepts/structure

Initial observations:

- Public Docs model is a tree of tabs, body, structural elements, paragraphs, paragraph elements, text runs, tables, footnotes, headers, footers, inline objects, positioned objects, lists, named ranges, named styles, and suggestions.
- Text addressing is exposed as UTF-16 code unit indexes.
- Formatting is split across document style, named styles, paragraph style, text style, list metadata, and object properties.
- Suggested changes are first-class overlays, not merely comments.

Research tasks:

- Build a matrix of Docs API entities and classify each as `must-have`, `later`, or `out-of-scope`.
- Define a compact internal document schema for:
  - document metadata
  - tabs or sections
  - block nodes: paragraph, heading, list item, table, page break
  - inline nodes: text, link, citation, footnote reference, mention/rich-link placeholder
  - marks: bold, italic, underline, strike, code, superscript, subscript, color, background, font, size
  - comments and suggestions
- Decide whether internal positions use UTF-8 byte offsets, Unicode scalar indexes, grapheme indexes, or CRDT-native IDs, and define conversion rules for Google-style imports/exports.
- Test import/export against small real Google Docs exports via the API once credentials exist.

Deliverables:

- `docs/research/google-docs-schema.md`
- `docs/schema/document-v0.md`
- JSON examples for one minimal doc, one styled doc, one list-heavy doc, one table doc, and one citation-heavy doc.

Done when:

- The supported Docs subset is listed as a table with `supported`, `deferred`, and `excluded` columns.
- The schema draft defines block nodes, inline nodes, marks, comments, suggestions, lists, tables, footnotes, and citation anchors.
- The position model is chosen and its Unicode conversion behavior is documented.
- The five required JSON examples validate against the draft schema by inspection or a small local validator.
- Import/export gaps against the Google Docs API are documented.

### 2. Google Sheets Public Model

Primary source:

- Google Sheets API `spreadsheets` resource: https://developers.google.com/workspace/sheets/api/reference/rest/v4/spreadsheets
- Google Sheets `Sheet`, `RowData`, and `CellData`: https://developers.google.com/workspace/sheets/api/reference/rest/v4/spreadsheets/sheets
- Google Sheets `ExtendedValue`: https://developers.google.com/workspace/sheets/api/reference/rest/v4/spreadsheets/other

Initial observations:

- Public Sheets model is workbook -> sheets -> grid data -> rows -> cells.
- Cell values are a union of number, string, bool, formula, or error.
- Formatting, validation, filters, charts, protected ranges, named ranges, merges, and comments are separate layers over the grid.
- Formulas are stored as strings; a compatible formula parser/evaluator is a major separate subsystem.

Research tasks:

- Define spreadsheet subset:
  - workbook metadata, locale, timezone
  - sheets, grid dimensions, frozen rows/columns
  - sparse cells with raw value, formula, computed value, format, note/comment anchor
  - merged cells, named ranges, basic filters, data validation
  - formula dependency graph and recalculation model
- Compare formula engines available from Rust or embeddable runtimes.
- Decide how collaborative edits compose for rows/columns/cells/formulas.
- Decide whether formulas are signed as user-authored text only, or whether computed values are also included in signed snapshots.

Deliverables:

- `docs/research/google-sheets-schema.md`
- `docs/schema/spreadsheet-v0.md`
- Formula compatibility test corpus.

Done when:

- The supported Sheets subset is listed as a table with `supported`, `deferred`, and `excluded` columns.
- The schema draft defines workbook metadata, sheets, sparse cells, styles, formulas, computed values, named ranges, merges, filters, comments, and validation.
- Formula parsing/evaluation options are compared with a recommendation.
- Row/column insertion semantics and formula reference update rules are documented.
- A formula compatibility corpus exists with expected parse and evaluation outcomes for the initial subset.

### 3. Collaboration Model

Primary sources:

- Google Drive Realtime API launch note, which states it used operational transformation: https://developers.googleblog.com/en/build-collaborative-apps-with-google-drive-realtime-api/
- Google Realtime API retirement timeline: https://workspaceupdates.googleblog.com/2017/11/committed-to-storage-apis-retiring.html
- Yjs shared types and editor ecosystem: https://docs.yjs.dev/
- Automerge rich text model: https://automerge.org/docs/reference/documents/rich-text/

Initial hypothesis:

- Google Docs historically used operational transformation-style collaboration, but this project should evaluate CRDTs seriously because offline-first and raw object storage modes are first-order goals.
- Yjs has the richest editor ecosystem, but it is JavaScript-first.
- Automerge has stronger local-first/version-history alignment and explicit rich-text marks/block markers, but editor integration and performance need validation for this use case.

Research tasks:

- Prototype two collaboration paths:
  - Automerge document with rich text marks and block markers.
  - Yjs-compatible model through a Rust boundary, or Rust CRDT alternative if one is viable.
- Validate against hard editing cases:
  - concurrent text insert/delete
  - overlapping style marks
  - split/merge paragraphs
  - list indentation changes
  - table row/column edits
  - comment anchors and citation anchors surviving edits
  - spreadsheet row insertion with formula references
- Define presence separately from persisted document state. Presence should include cursors, selections, active users, and transient awareness.
- Define compaction: raw operation log -> checkpoint snapshot -> retained signed manifest.

Deliverables:

- `docs/research/collaboration-model.md`
- Small Rust prototype for one rich-text document and one spreadsheet grid.
- Decision record: `docs/adr/0001-collaboration-core.md`

Done when:

- OT, Yjs-style CRDT, Automerge-style CRDT, and any Rust-native candidate are compared against offline-first, raw object storage, rich text, spreadsheets, history, and performance needs.
- The prototype demonstrates concurrent edits and deterministic convergence for rich text and spreadsheet cases.
- Presence/cursors are specified as ephemeral state outside the persisted document model.
- Compaction from operation log to snapshot is specified.
- `docs/adr/0001-collaboration-core.md` selects the collaboration core or explicitly records why the decision is deferred.

### 4. Storage And Versioning

Primary source:

- OpenDAL Rust crate and supported backends: https://docs.rs/opendal/latest/opendal/

Initial hypothesis:

- Storage should be content-addressed where possible, with a signed mutable head pointer or signed manifest for each branch/version line.
- Raw S3 mode cannot rely on server-side transactions beyond object conditional writes, object versioning, and eventual/list consistency behavior of the selected backend.

Research tasks:

- Design object layout:
  - document root manifest
  - operation segments
  - compacted snapshots
  - attachment blobs
  - citation library blobs
  - signatures and transparency metadata
  - branch/head pointers
- Evaluate S3-compatible primitives needed:
  - conditional put
  - object versioning
  - multipart upload
  - listing consistency
  - object locks/legal hold, if relevant
- Define conflict handling for raw S3 single-user and accidental multi-writer cases.
- Confirm OpenDAL behavior and gaps across AWS S3, MinIO, filesystem, and memory backends.

Deliverables:

- `docs/research/storage-layout.md`
- `docs/schema/storage-manifest-v0.md`
- MinIO/local filesystem proof of concept.

Done when:

- Object keys, immutable blobs, mutable heads, snapshots, operation segments, attachments, and signatures are specified.
- Raw S3-compatible, filesystem, and server-mediated modes have separate write/read/sync flows.
- Conflict handling for stale heads and accidental multi-writer updates is documented.
- OpenDAL backend requirements and backend-specific gaps are listed.
- A local proof of concept writes and reads a manifest, snapshot, and operation segment through OpenDAL or a documented stand-in.

### 5. Cryptographic Signing

Primary sources:

- Sigstore overview: https://docs.sigstore.dev/
- Cosign blob signing: https://docs.sigstore.dev/cosign/signing/signing_with_blobs/
- Sigstore Rust client notes: https://docs.sigstore.dev/language_clients/rust/
- Sigstore Rust crates: https://github.com/sigstore/sigstore-rust

Initial hypothesis:

- Support two signing modes:
  - local key signatures for offline and private deployments
  - Sigstore-style identity signatures for public/auditable publishing
- The signed unit should probably be a canonical manifest that references immutable content hashes, not a mutable raw document file.

Research tasks:

- Define canonical serialization for signed manifests, likely deterministic JSON or CBOR.
- Decide hash algorithm and signature suites.
- Evaluate Rust support for:
  - Ed25519 local signatures
  - minisign/signify-compatible detached signatures
  - Sigstore verification and future signing
- Define trust model:
  - document owner keys
  - collaborator keys
  - version signatures
  - export signatures
  - revocation or key rotation
- Decide whether every operation is individually signed or whether signed checkpoints plus hash-chained operation segments are sufficient.

Deliverables:

- `docs/research/signing-model.md`
- `docs/schema/signed-manifest-v0.md`
- CLI prototype: create document, write snapshot, sign manifest, verify manifest.

Done when:

- The threat model states what signatures do and do not protect.
- The signed object boundary is fixed: operation, segment, snapshot, manifest, branch head, export, or a combination.
- Canonical serialization and hash algorithms are selected.
- Local-key and Sigstore-style identity signing paths are compared.
- The prototype can sign and verify a manifest, and fails verification after a deliberate content change.

### 6. Citations

Primary sources:

- Paperpile Google Docs citation workflow: https://paperpile.com/h/get-started-google-docs/
- Paperpile citation features: https://paperpile.com/features/google-docs-citations-bibliography/
- Citation Style Language schemas: https://github.com/citation-style-language/schema
- Rust `citeworks_csl` crate: https://docs.rs/citeworks-csl

Initial observations:

- Paperpile inserts Google Docs citations as linked placeholder text, then formats citations and bibliography later.
- Paperpile supports citation groups, page/location metadata, prefix/suffix, suppress-author, footnote citations, local document-specific citation metadata, CSL styles, BibTeX/RIS export, and clean-copy export.
- CSL-JSON is the likely bibliographic metadata baseline.

Research tasks:

- Define citation node schema:
  - stable citation ID
  - ordered citation items
  - CSL item IDs
  - locator/page metadata
  - prefix/suffix
  - suppress author
  - footnote or inline placement
  - rendered text cache
- Define document-local bibliography database using CSL-JSON.
- Evaluate Rust CSL support:
  - `citeworks_csl` for serde types
  - whether a Rust citeproc engine exists and is complete enough
  - fallback to external citeproc executable or WASM
- Define bibliography regeneration algorithm and collaborative conflict behavior.

Deliverables:

- `docs/research/citations.md`
- `docs/schema/citation-v0.md`
- Prototype: insert placeholder citation, render bibliography using CSL, re-render after style change.

Done when:

- Paperpile's visible Google Docs behavior is summarized with implications for our model.
- CSL-JSON or an alternative is selected for bibliography item storage.
- Citation node schema supports citation groups, locators, prefix/suffix, suppress-author, footnote placement, rendered text cache, and local document bibliography metadata.
- Rust citation-processing options are evaluated with a recommendation or fallback.
- The prototype renders citations and bibliography, then re-renders after a style change without losing structured citation metadata.

### 7. Frontend And Editor Surface

Potential implementation routes:

- Leptos web app with WASM core.
- Tauri app using a web editor surface plus Rust local storage/sync/signing.
- Shared Rust core with multiple frontends.

Research tasks:

- Evaluate editor frameworks:
  - ProseMirror/Tiptap schema compatibility
  - CodeMirror-style text core for plain mode
  - custom canvas/grid for spreadsheet
  - WASM boundary costs for Rust document core
- Define how the frontend observes document state and emits edits without owning the canonical model.
- Validate offline workflow:
  - create local doc
  - edit while disconnected
  - write local snapshots
  - later sync to S3/server
  - verify signed versions

Deliverables:

- `docs/research/frontend-options.md`
- One minimal collaborative editor spike.
- One minimal spreadsheet grid spike.

Done when:

- Leptos web, Tauri, and shared Rust core approaches are compared against offline support, collaboration integration, performance, packaging, and editor ecosystem.
- The editor surface recommendation identifies what owns canonical state and how edits cross the frontend/core boundary.
- The rich-text and spreadsheet spikes can load from the draft schemas or document why that is deferred.
- The offline workflow is demonstrated or specified step by step.

## Suggested Order

1. Write `document-v0` and `spreadsheet-v0` schema drafts from the Google API public models.
2. Prototype rich-text collaboration with Automerge and one editor binding.
3. Prototype sparse spreadsheet storage and row/column operations.
4. Design S3 object layout and signed manifest format.
5. Add citation schema and CSL rendering spike.
6. Revisit frontend choice after the data model and collaboration constraints are clearer.
7. Write `docs/research/summary.md` and mark this plan complete only after all checklist items above are satisfied.

## Immediate Next Files

- `docs/research/google-docs-schema.md`
- `docs/research/google-sheets-schema.md`
- `docs/research/collaboration-model.md`
- `docs/research/storage-layout.md`
- `docs/research/signing-model.md`
- `docs/research/citations.md`
- `docs/research/frontend-options.md`
- `docs/research/summary.md`
- `docs/schema/document-v0.md`
- `docs/schema/spreadsheet-v0.md`
- `docs/schema/storage-manifest-v0.md`
- `docs/schema/signed-manifest-v0.md`
- `docs/schema/citation-v0.md`
- `docs/adr/0001-collaboration-core.md`
- `docs/adr/0002-storage-and-signing.md`

## Current Biases To Test

- Use CRDTs, not classic OT, unless prototypes show unacceptable complexity or performance.
- Store immutable content-addressed chunks plus signed manifests in S3-compatible storage.
- Keep presence/cursors out of persisted document state.
- Use CSL-JSON for bibliography metadata.
- Treat citations as structured inline nodes with rendered text as a cache, not as plain text links.
- Keep Google Docs/Sheets compatibility as a schema target, not as a requirement to reproduce every API feature.
