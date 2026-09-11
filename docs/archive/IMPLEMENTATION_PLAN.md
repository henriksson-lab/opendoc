# OpenDoc Implementation Plan

Status: draft implementation plan.

Goal: build the first serious OpenDoc prototype around robust Google Docs-style rich-text editing, deterministic merge tests, on-disk object storage, binary schemas, and Google Docs-shaped import.

## V0 Target

V0 is not a full UI product. It is an API/object-format/schema prototype with tests that prove the hard parts:

- rich-text document model with comments, suggestions, citations, equations, tables, and formatting
- operation-level automatic merge semantics
- deterministic fuzz and synthetic merge tests
- on-disk object storage with binary manifests
- source-state signing with Rust-native OpenSSH-compatible signatures
- citations backed first by `citum`, with CSL import/export kept as adapters
- `.doc`/`.docx` import into our Google Docs-shaped schema through easy external tooling

S3, full UI, server-managed commits, and polished import/export come later.

## Phase 1: Workspace And Core Types

Deliverables:

- `crates/opendoc-core`
- `crates/opendoc-format`
- `crates/opendoc-store`
- `crates/opendoc-merge`
- `crates/opendoc-sign`
- `crates/opendoc-import`

Tasks:

- Define stable IDs:
  - document UUID, likely UUIDv7
  - block UUIDs
  - text element IDs
  - comment IDs
  - suggestion IDs
  - blob IDs by content hash
- Define hash-reference type with algorithm agility from day one.
- Define document model structs for:
  - blocks
  - inline content
  - marks
  - comments and threads
  - suggestions
  - citations
  - inline/block equations
  - tables
- Keep JSON only for debug projections and tests.

Exit criteria:

- Core model compiles.
- Round-trip debug projection tests pass.
- Hash references include explicit algorithm.

## Phase 2: Binary Format

Decision: use deterministic CBOR for research-stage manifests and metadata.

Deliverables:

- binary manifest encoding
- binary branch head encoding
- binary signature envelope encoding
- binary lookup record encoding
- binary tombstone encoding
- test vectors

Tasks:

- Pick concrete Rust CBOR crate.
- Implement deterministic encode/decode wrappers.
- Add golden test vectors.
- Reject non-canonical encodings where practical.
- Keep format versioning simple; backward compatibility is not required during research.

Exit criteria:

- Deterministic encode of same logical value always produces identical bytes.
- Corrupt/non-canonical input fails cleanly.
- Manifests, heads, signatures, lookup records, and tombstones have binary tests.

## Phase 3: On-Disk Object Store

Default v0 backend: local on-disk object repository.

Tasks:

- Try OpenDAL filesystem backend first.
- If OpenDAL adds friction, define a small object-store trait:
  - `put_if_absent`
  - `get`
  - `exists`
  - `list_prefix`
  - `compare_and_swap_head`
- Support multiple documents in one repository from day one.
- Store loose objects first.
- Keep API compatible with later pack-file implementation.
- Implement crash-safe head updates.

Small-file mitigation strategy:

- Start with loose files for simplicity.
- Add append-only pack files once object count becomes painful.
- Local maintenance may rewrite packs.
- Pack rewrite must be crash-safe: write new pack, verify, atomically swap index.

Open question:

- Sign whole packs, logical contained objects, or both.

Exit criteria:

- Local repository can create/open multiple documents.
- Manifest commit can be written and read.
- Branch head update is atomic on local disk.
- Missing blob produces placeholder/warning, not open failure.

## Phase 4: Merge Model Research Prototype

This is the highest-risk phase.

Approach:

- Do not assume DOM/tree merge is correct.
- Compare CRDT-native sequence/block identity, state-machine/event model, and block UUID graph.
- Renderer may project a tree, but merge state need not be a DOM.

Required semantics:

- every merge converges to a valid openable document
- bad cases create warnings or review metadata, not manual conflicts
- text ranges and comments use stable UUID-backed positions
- comment ranges may span multiple blocks
- if all referenced text is deleted, comment is deleted from current state unless history/audit view restores it
- equations merge as atomic inline/block objects
- accepted suggestions become provenance metadata

Test strategy:

- realistic synthetic scenarios
- operation-level fuzzing first
- deterministic replay
- convergence checks across actor orderings
- validity checks after every merge

Initial scenarios:

- concurrent insert/delete in same paragraph
- concurrent formatting over overlapping text
- split/merge paragraph with comments
- comments spanning blocks
- suggestion insertion/deletion/formatting
- suggestion discussion behavior following Google Docs where practical
- citation anchor movement
- table row/cell edits
- equation insertion/deletion as atomic object

Exit criteria:

- Fuzzer can generate operation streams for 1-3 active replicas and passive-viewer-equivalent replicas.
- All replicas converge byte-for-byte at canonical projection level.
- Invalid documents are treated as test failures.
- Graceful degradation warnings are explicit and deterministic.

## Phase 5: Comments And Suggestions

Tasks:

- Implement threaded comments without overcomplicating the model.
- Sign comments as document state.
- Hide deleted comments in normal view.
- Retain deleted comments in audit/recovery history.
- Implement Google Docs-style suggestions:
  - insertion
  - deletion
  - formatting change
  - accept/reject
  - provenance metadata
- Allow suggestions inside comments.

Exit criteria:

- Comments and suggestions are included in merge tests from the start.
- Accepted suggestions are no longer visible as track changes, but provenance remains.
- Deleted comments are audit-visible when retained.

## Phase 6: Signing

Default: unsigned documents open normally.

Tasks:

- Implement Rust-native OpenSSH-compatible signing.
- Support optional `ssh-keygen` verification/signing helper only if useful.
- Allow multiple signatures per version.
- Display signature states:
  - `unsigned`
  - `signed`
  - `trusted`
  - `untrusted`
  - `broken`
- Sign source document state plus retained history reachable from the manifest.
- Do not sign rendered PDF/output.
- Do not include invisible block UUIDs in normal content signatures.
- Sign formula source, not computed values.
- Include user-visible metadata in signature envelopes:
  - title
  - signer display name
  - signer key identity
  - signing timestamp

Exit criteria:

- Valid signature verifies.
- Tampered manifest fails.
- Multiple signatures verify independently.
- Unsigned document opens normally.

## Phase 7: Citations

Default: use `citum` as the first Rust-native citation engine.

Tasks:

- Keep the signed document model independent of `citum` internals.
- Store citation occurrences as anchored inline nodes with stable citation IDs.
- Store rendered citation text and bibliography output as cache/projection data.
- Store document-local bibliography metadata as source records that can later import/export CSL-JSON.
- Model Paperpile-like citation groups:
  - ordered citation items
  - locator/page metadata
  - prefix/suffix
  - suppress-author
  - footnote placement
- Preserve citation anchors through operation-level merges.
- Re-render citations and bibliography after style or source metadata changes.

Exit criteria:

- Prototype inserts a structured citation and renders it through `citum`.
- Re-rendering does not lose citation metadata.
- Citation anchor movement is covered by merge tests.
- Future CSL import/export can be added without changing signed document operations.

## Phase 7: Blob And Typed Content Signatures

Tasks:

- Implement exact-byte blob signatures.
- Implement typed signature profile interfaces.
- Add test-only profile for semantic signing.
- Keep image pixel and FASTQ sequence-only profiles as design constraints.
- Keep stored object byte hashes for retrieval/integrity even when typed signatures exist.

Exit criteria:

- Same blob hash reuses byte signature.
- Typed profile verifies semantic digest independent of storage path.
- Missing blob renders placeholder/warning.

## Phase 8: Google Docs-Shaped Import

No Google credentials are available.

Tasks:

- Import `.doc` and `.docx` if tooling allows.
- Use easy cross-platform tools where possible.
- Linux-only no-root tooling is acceptable as fallback.
- Convert into OpenDoc schema shaped by the Google Docs API.
- Preserve comments/suggestions if converter exposes them.
- Abort import on failure; do not create partial documents.
- Do not retain original `.doc` source by default.
- Do not preserve imported internal IDs unless useful.

Exit criteria:

- Import one synthetic `.docx` with paragraphs, headings, formatting, comments, suggestions if available, table, citation-like text, and equations.
- Import failure leaves no partial document.
- Imported document can be serialized, stored, reopened, and merged in tests.

## Phase 9: Equations

Tasks:

- Store one canonical equation source, likely TeX/LaTeX-like.
- Support inline and block equations.
- Treat equations as atomic merge objects.
- Rendered cache is optional and not signed as source truth.
- No display-text fallback required in v0.

Exit criteria:

- Inline equation round-trips.
- Block equation round-trips.
- Concurrent equation edit/delete converges deterministically.

## Phase 10: Spreadsheet Foundation

Spreadsheet is not the first prototype focus, but formula evaluation is required for spreadsheet v0.

Tasks:

- Implement sparse sheet model.
- Implement deterministic formula evaluator for initial subset:
  - literals
  - arithmetic
  - references
  - ranges
  - `SUM`
  - basic errors
- Cache computed values in RAM only.
- Sign formula source only.

Exit criteria:

- Formula corpus passes deterministically across platforms.
- Row/column stable-ID projection tests pass.

## Phase 11: Lookup And Archive

Tasks:

- Implement document UUID lookup records.
- Support optional DOI aliases.
- Resolve cross-document references to latest branch by default.
- Support exact manifest pinning.
- Degrade gracefully with warnings.
- Implement signed tombstone records.
- Use generic archive locators first.
- Research HPC2N-relevant IBM Spectrum Protect/TSM and SweStore/dCache patterns.

Exit criteria:

- Lookup works without server through local index.
- Missing index can be rebuilt by scan.
- Archived object tombstone verifies and points to restore metadata.
- Missing archived object does not block opening source document.

## Phase 12: Performance Strategy

Do not optimize prematurely, but keep the path clear.

Strategies:

- snapshots for fast open
- shallow blob loading
- loose object to pack-file compaction
- pack indexes
- binary manifests
- RAM formula cache
- operation batching separate from rendering

Exit criteria:

- Benchmarks exist for open, write manifest, merge replay, and object count.
- Performance problems have a named mitigation path.

## Immediate Next Work

1. Create real Rust crates from the prototype workspace.
2. Implement core ID/hash/document model.
3. Implement local object store and binary manifest/head encoding.
4. Build operation-level merge fuzzer.
5. Add comments/suggestions/equations to merge scenarios.
6. Add signing skeleton using Rust-native OpenSSH-compatible signatures.

## Remaining Decisions

- Pack signing policy: whole packs, logical objects only, or both.
- Exact CRDT/library choice after realistic merge tests.
- Exact import conversion toolchain.
- Exact deterministic CBOR crate.
- Exact Rust-native OpenSSH signature crate.
