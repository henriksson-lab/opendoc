# Research Summary

Status: implementation research phase started; not ready for product implementation without dependency-backed prototypes.

## Outputs

- Google Docs schema research: `docs/research/google-docs-schema.md`
- Google Sheets schema research: `docs/research/google-sheets-schema.md`
- Collaboration model research: `docs/research/collaboration-model.md`
- Storage layout research: `docs/research/storage-layout.md`
- Archive and lookup research: `docs/research/archive-and-lookup.md`
- Signing model research: `docs/research/signing-model.md`
- Citation research: `docs/research/citations.md`
- Frontend options research: `docs/research/frontend-options.md`
- Document schema: `docs/schema/document-v0.md`
- Spreadsheet schema: `docs/schema/spreadsheet-v0.md`
- Storage manifest schema: `docs/schema/storage-manifest-v0.md`
- Signed manifest schema: `docs/schema/signed-manifest-v0.md`
- Citation schema: `docs/schema/citation-v0.md`
- Collaboration ADR: `docs/adr/0001-collaboration-core.md`
- Storage/signing ADR: `docs/adr/0002-storage-and-signing.md`
- V0 implementation scope ADR: `docs/adr/0003-v0-implementation-scope.md`
- Implementation plan: `docs/IMPLEMENTATION_PLAN.md`

## Core Question Answers

1. Minimal document schema: use the `document-v0` block/inline/mark model, with citations and comments as structured nodes/anchors.
2. Minimal spreadsheet schema: use sparse workbook/sheet/cell storage with stable row and column IDs and formulas as user-authored text plus computed cache.
3. Collaboration layer: start with an Automerge-style CRDT core and preserve a Yjs bridge option.
4. Document storage: use content-addressed immutable objects plus mutable branch heads in S3-compatible or filesystem storage. Images and arbitrary binary objects are reusable content-addressed blobs that support shallow clone. Documents have UUIDs, optional DOI aliases, signed lookup records, and signed tape/cold-storage tombstones.
5. Signing boundary: sign canonical manifests that reference snapshots, operation segments, attachments, and citation libraries by hash. Binary blobs may also have independent reusable signatures keyed by hash. Data types can additionally define typed content signatures over semantic canonical forms, such as decoded image pixels or FASTQ sequences without PHRED scores.
6. Citation metadata: use CSL-JSON for bibliography items and structured inline citation nodes for occurrences.
7. Rust crates/standards: evaluate OpenDAL, Automerge, Sigstore, local Ed25519 crates, CSL-JSON crates, and citeproc-compatible renderers during the dependency-backed prototype phase.

## V0 Scope Decisions

- Spreadsheet v0 includes formula evaluation.
- First serious prototype is a robust Google Docs-style rich-text editor with collaboration support.
- First-class storage target is on-disk object storage; S3-compatible storage remains the long-term target.
- Server-managed commits are not assumed for v0; multi-user behavior should be proved with simulations/tests.
- Google Docs/Sheets API import/export is the first compatibility proof.
- Image semantic signatures and FASTQ typed-content signatures are design constraints.
- Expected collaboration scale is 1-3 active editors and up to 5 passive viewers.
- Storage performance should be optimized for local on-disk objects and common S3-compatible stores.
- Unsigned documents open normally; signatures are visual trust/compliance indicators.
- V0 signer/collaborator identities use OpenSSH-style keys/signatures.
- First prototype may be API/object-format/schema focused with tests.
- Invisible paragraph/block UUIDs are allowed but excluded from normal document-content signatures.
- Merge validation should use realistic synthetic scenarios plus fuzz testing.
- Comments and suggestions are essential v0 merge features.
- Equation storage uses one TeX/LaTeX-like source store; rendered output is derived.
- Local object storage needs small-file mitigation, likely pack files.
- Deleted content is retained for now; garbage collection is deferred.
- Cross-document references degrade gracefully with warnings.
- Merges must always produce a valid openable document.
- Suggestions follow Google Docs-style track changes.
- Comments are signed document state.
- Formula evaluation must be deterministic across platforms.
- Formula signatures cover source only, not cached computed values.
- `.doc` import may use external conversion tools for the first proof.
- `.doc` import should preserve comments/suggestions when possible.
- Cross-document references default to latest branch with optional exact-version pinning.
- “Official” status follows from valid signatures, not a manual flag.
- Multiple signatures per version are allowed.
- Signature UI states include unsigned/signed/trusted/untrusted/broken.
- Formula computed values are RAM caches only.
- Missing attachments/images render as placeholders.
- First merge fuzzer operates at operation level.
- Deleted text/comments are normal-view hidden and audit/recovery visible.
- Signatures include user-visible title/author/timestamp metadata.
- V0 permissions rely on filesystem/bucket access but leave room for future server enforcement.
- Comments support simple threads/replies.
- Suggestions include formatting changes.
- Equations support inline and block forms.
- Local maintenance may rewrite pack files while preserving logical verification.
- Import proof may target `.doc` and `.docx` through external tooling.
- Comment anchors resolve to text ranges over stable UUID-backed positions; unresolved anchors attach to nearest surviving block.
- Suggestions are allowed inside comments.
- Equations are atomic merge objects.
- Passive viewers and active editors use the same update path where practical.
- Pack rewrites are crash-safe through write-new-pack, verify, then swap-index.

## Recommendation

Proceed to implementation only after replacing the std-only prototypes with dependency-backed prototypes using the actual candidate crates. The architecture direction is coherent enough to begin a Rust workspace, but collaboration performance, formula compatibility, canonical signing, and CSL rendering remain the highest-risk validation items.

## Remaining Validation

- Automerge Rust rich-text behavior with real crate APIs.
- Yjs bridge feasibility from Rust or WASM.
- OpenDAL conditional-write support on target S3-compatible backends.
- Ed25519 and Sigstore signing implementation.
- Formula parser/evaluator selection.
- Full CSL renderer selection.

## Completion Audit

| Requirement | Evidence |
| --- | --- |
| Google Docs subset and schema | `docs/research/google-docs-schema.md`, `docs/schema/document-v0.md` |
| Google Sheets subset and schema | `docs/research/google-sheets-schema.md`, `docs/schema/spreadsheet-v0.md`, `examples/formulas/v0.tsv` |
| Collaboration decision | `docs/research/collaboration-model.md`, `docs/adr/0001-collaboration-core.md` |
| Storage and versioning design | `docs/research/storage-layout.md`, `docs/schema/storage-manifest-v0.md`, `docs/adr/0002-storage-and-signing.md` |
| Archive, tape recall, UUID/DOI lookup | `docs/research/archive-and-lookup.md`, `docs/research/storage-layout.md`, `docs/schema/storage-manifest-v0.md` |
| Signing design | `docs/research/signing-model.md`, `docs/schema/signed-manifest-v0.md`, `prototypes/signed-manifest` |
| Blob references, shallow clone, blob signatures, typed content signatures | `docs/research/storage-layout.md`, `docs/research/signing-model.md`, `docs/schema/storage-manifest-v0.md`, `docs/schema/signed-manifest-v0.md`, `docs/adr/0002-storage-and-signing.md` |
| Citation design | `docs/research/citations.md`, `docs/schema/citation-v0.md`, `prototypes/citation-render` |
| Frontend recommendation | `docs/research/frontend-options.md` |
| Rich-text prototype | `cargo run -p richtext-crdt` |
| Spreadsheet prototype | `cargo run -p spreadsheet-grid` |
| Signed manifest prototype | `cargo run -p signed-manifest` |
| Citation prototype | `cargo run -p citation-render` |

## Verification Commands

```sh
cargo fmt
cargo check
cargo run -p richtext-crdt
cargo run -p spreadsheet-grid
cargo run -p signed-manifest
cargo run -p citation-render
```
