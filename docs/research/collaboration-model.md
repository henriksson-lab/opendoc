# Collaboration Model Research

Status: initial complete draft.

## Sources

- Google Drive Realtime API launch note: https://developers.googleblog.com/en/build-collaborative-apps-with-google-drive-realtime-api/
- Google Realtime API retirement note: https://workspaceupdates.googleblog.com/2017/11/committed-to-storage-apis-retiring.html
- Yjs docs: https://docs.yjs.dev/
- Automerge rich text docs: https://automerge.org/docs/reference/documents/rich-text/

## Findings

Google's older public Realtime API documentation stated that it used operational transformation. That confirms the general family of techniques historically used for Google-like collaborative editing, but it does not provide the internal Google Docs storage model.

For OpenDoc, offline mode and raw S3-compatible storage are first-order requirements. That shifts the default from server-centered OT toward CRDT updates, snapshots, and compaction.

Batching must be separate from viewing. The editor applies local and remote operations immediately and renders from the live collaboration document. Operation segments, snapshots, and signed manifests are persistence artifacts produced asynchronously.

## Comparison

| Model | Offline-first | Raw object storage | Rich text | Spreadsheet structure | History/signing | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| Central OT | weak without server | weak | proven | possible | server defines truth | closest to historical Google style, but conflicts with raw S3 mode |
| Yjs-style CRDT | strong | good | excellent ecosystem | possible | update logs need compaction | best web editor ecosystem, JS-first |
| Automerge-style CRDT | strong | good | rich text marks/block markers | needs validation | strong history fit | best initial Rust/local-first research path |
| Rust-native custom CRDT | unknown | good | unknown | unknown | controllable | too risky before prototypes |

## Recommendation

Follow ADR 0001: start with an Automerge-style CRDT core, keep schema independent, and preserve a Yjs bridge option for frontend integrations.

Do not assume a DOM or tree is the canonical merge representation. The renderer may project a tree, but the merge model can be a state machine, CRDT sequence, block graph, or hybrid if that makes automatic merging more robust. Paragraph-like blocks should have invisible stable IDs so split, merge, move, format, comment, and citation operations have durable anchors.

## Rejected Options

- **Central OT as the first core.** Rejected because raw S3 and offline mode need useful behavior without a central authority.
- **Yjs as the canonical Rust core.** Rejected for now because it is JavaScript-first, despite its strong ecosystem.
- **Custom CRDT before testing existing systems.** Rejected because rich text and spreadsheets already contain enough unknowns.

## Presence

Presence is not persisted document state. It includes:

- active user identity display data
- cursor position
- selection range
- viewport or active sheet
- last-seen timestamp

Presence expires and is never included in signed manifests.

## Compaction

The storage model should allow:

1. append CRDT operation/change segments
2. periodically materialize a snapshot
3. write a manifest that references the snapshot and retained segments
4. sign the manifest
5. garbage collect unreachable old segments only after retention policy allows

## Automatic Rich-Text Merge Requirements

All editing merges must be fully automatic. A manual merge conflict is a failure of the core document model.

The document model must satisfy:

- text is represented as a CRDT sequence with stable element identities
- formatting marks are anchored to stable elements, not byte offsets
- marks can overlap and nest without requiring tree reshaping
- mark add/remove operations converge deterministically
- block properties converge deterministically
- comments, citations, footnotes, and suggestions use stable anchors
- renderer projection is derived after merge and does not own canonical merge state

Required test cases:

- concurrent insertions into the same word
- concurrent delete and style change over overlapping text
- overlapping bold, italic, link, and citation spans
- insertion at both sides of a mark boundary
- concurrent paragraph split and mark extension
- concurrent list indentation and text edit
- concurrent table row insertion and cell formatting
- concurrent comment creation, reply, resolution, and anchor movement
- concurrent suggestion insertion, deletion, acceptance, rejection, and overlapping style change
- fuzz-generated edit streams with deterministic replay and convergence checks

The acceptable result is deterministic convergence to a valid document. It does not have to match either user's local formatting intention perfectly in every scalar-property conflict, but it must preserve all non-deleted content and keep the document openable/editable. Bad cases should create warnings or review metadata rather than manual merge conflicts.

Suggestions should behave like Google Docs-style track changes: proposed insertions, deletions, and style changes are explicit document state that can be accepted or rejected. User mentions are not required in v0 except as highlighted text syntax.

Comments are normal signed document state, but users may delete them from the current visible state. History retains the deleted comment records. Accepted/rejected suggestions should preserve attribution/history metadata where available so later signed versions can show provenance.

Deleted text and comments are hidden from normal editing views and exposed only in audit or recovery views. Deleted comments are restorable or auditable when retained data is available. Comments support simple threads/replies in v0. Accepted suggestions become provenance metadata rather than persistent visible track-change markup. Suggestions support formatting changes as well as text insert/delete. The first merge fuzzer should generate operation streams directly before expanding to imported-document-shaped scenarios.

Passive viewers should be treated as potential editors where practical. A viewer can become an editor, so simulations should not assume a hard architectural divide between passive and active replicas.

Comment anchors should be text ranges tied to stable UUID-backed document positions. If exact anchors cannot be resolved after edits, attach the comment to the nearest surviving block and surface a warning. Suggestions are allowed inside comments. Equations are atomic merge objects, not editable token streams.

## Prototype Evidence

- Rich text convergence prototype: `prototypes/richtext-crdt`.
- Spreadsheet convergence prototype: `prototypes/spreadsheet-grid`.

These prototypes are deliberately small and std-only. They prove convergence mechanics and stable anchors at a toy level, not production Automerge behavior.

## Open Risks

- Automerge Rust crate maturity and rich-text API surface must be tested with real dependencies.
- Large document memory use and sync latency are unknown.
- Spreadsheet formulas over CRDT row/column structures require deeper design.
- Rich-text mark semantics must be validated with the actual CRDT library, not only toy prototypes.
- The canonical merge representation is unresolved and must be researched before committing to a DOM-like tree model.
- Comments and suggestions are included in v0 merge validation because they constrain anchor semantics.
