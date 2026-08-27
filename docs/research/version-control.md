# Version Control Model

Status: draft.

## Goal

OpenDoc version control must support:

- collaborative editing
- single-user use
- local offline mode
- raw S3-compatible storage
- signed versions
- efficient handling of keypress-scale edits

## Main Answer

A keypress can be an edit operation, but it should not normally be a signed version-control commit and should not normally create an S3 object by itself.

The system has four granularities:

| Layer | Granularity | Stored as | Signed by default | Purpose |
| --- | --- | --- | --- | --- |
| Edit operation | keypress, delete, style change, row insert | in-memory CRDT change or local WAL frame | no | immediate collaboration and undo |
| Operation segment | batch of operations | immutable binary blob | covered by manifest hash | efficient append/sync unit |
| Snapshot | compact state checkpoint | immutable binary blob | covered by manifest hash | fast load and compaction |
| Manifest commit | version graph node | immutable binary manifest | yes | durable signed version |

## Commit Granularity

Default policy:

- Batch edits into operation segments.
- Flush operation segments on any of:
  - explicit user save
  - collaboration transaction boundary
  - idle timer, initially around 1-5 seconds
  - segment size threshold, initially around 64 KiB to 1 MiB after compression
  - offline app shutdown
- Create manifest commits on any of:
  - explicit user save or named version
  - autosave checkpoint, initially around 30-120 seconds
  - before sync upload completes
  - before signing/exporting
- before compaction drops older segments

The exact thresholds should be measured. The important invariant is that operation capture is fine-grained, while durable signed commits are batched.

Users should not need to know about compaction. For now, deleted content and historical operation data are retained; garbage collection is deferred.

## Live Editing Versus Storage Batching

Batching is only a durability, sync, and object-storage optimization. It must never be required for local rendering.

The editor pipeline is:

1. User action creates one or more edit operations.
2. Operations are applied immediately to the local collaboration document.
3. The renderer observes the updated projection immediately.
4. The same operations are appended to a local write-ahead log.
5. A background flusher groups WAL operations into binary operation segments.
6. A later checkpoint groups segments and snapshots into a signed manifest commit.

This means typing latency is bounded by local operation application and render projection, not by S3 writes, manifest signing, or segment flushes.

Remote collaborators follow the same rule:

1. Receive remote operation frames as soon as transport delivers them.
2. Apply them immediately to the collaboration document.
3. Render the updated projection immediately.
4. Persist them into local segments asynchronously.

The system may batch for network efficiency, but transport batches are not semantic commits. A batch can be split or combined without changing document meaning as long as operation causal metadata is preserved.

## Parallel Writers And Limited Batching

Parallel users limit how long unsent local edits can be held. The batching strategy must therefore use small live-operation batches for collaboration and larger background batches for storage.

Recommended policy:

- Local UI applies every operation immediately.
- Collaboration transport flushes quickly, initially on animation-frame or short timers such as 16-50 ms, and immediately for structural operations.
- Storage operation segments flush less frequently, initially 1-5 seconds or size threshold.
- Manifest commits remain coarser, initially explicit save, named version, sync boundary, or 30-120 second autosave checkpoint.

This gives low-latency collaboration without creating one S3 object or signed commit per keypress.

## Binary Format

Canonical version-control storage must be binary:

- Branch heads: deterministic compact binary records.
- Manifests: deterministic binary records, with deterministic CBOR as the v0 candidate.
- Operation segments: CRDT-native binary changes if using Automerge, or OpenDoc-native binary frames if custom.
- Snapshots: binary state images, optionally compressed.
- Attachments: raw content-addressed bytes.
- Small objects on local disk: logical objects packed into larger physical pack files when needed.

JSON is allowed only as:

- debug projection
- test fixture projection
- API response format
- import/export format

## Version Graph

Each manifest commit contains:

- document ID
- branch name
- parent manifest hash or hashes
- snapshot hash
- ordered operation segment hashes since the snapshot or parent
- attachment hashes
- citation library hashes
- author identity
- creation timestamp
- format versions

Normal history is a linear chain. Divergence creates multiple heads. Merge creates a manifest with multiple parents after CRDT reconciliation.

## Efficiency Notes

Efficient:

- immutable content-addressed blobs
- binary operation segments
- batching keypresses into segments
- signing manifests instead of every operation
- snapshots for fast loading
- hash coverage for all referenced content

Inefficient and rejected:

- JSON as canonical operation log
- signing every keypress
- one S3 object per keypress
- rewriting the full document object on every edit
- branch-head updates on every keypress

## Merge Policy

Documents:

- Merge operation segments through the collaboration core.
- Materialize a converged snapshot.
- Write a merge manifest with both parent manifest hashes.
- Merges must be fully automatic. If the collaboration core cannot converge a document without manual conflict resolution, it is not acceptable for the core editing model.
- User-facing review can show what changed, but it cannot be required to make the document valid.

### Rich Text And Formatting Merge Policy

Text content is an ordered CRDT sequence. Formatting is not stored as mutable byte ranges. Formatting is stored as CRDT-aware marks anchored to stable text positions or block markers.

Rules:

- Concurrent text insertions both survive and are ordered deterministically by CRDT metadata.
- Deletes tombstone text elements; marks attached only to deleted text disappear from the visible projection.
- Marks can overlap arbitrarily.
- Applying a mark creates or updates a mark interval over stable element IDs, not current indexes.
- Removing a mark creates a clearing operation over stable element IDs.
- Inserted text at a mark boundary follows the mark's expansion policy:
  - bold/italic/underline usually expand at both boundaries
  - links and citations usually do not expand unless explicitly edited
  - code spans expand only within the same inline code context
- Concurrent mark add and mark remove over the same text resolves by deterministic operation ordering unless the CRDT library provides native mark conflict semantics. The rendered projection must be identical on every replica.
- Block formatting, such as heading level or list indentation, is last-writer-wins per block property only when the property is scalar and cannot be merged structurally.
- Table edits use stable row, column, and cell IDs. Formatting on cells is property-based and converges by deterministic register semantics.
- Comments, citations, and footnotes are anchored inline objects or stable anchors, not plain formatting ranges.

Formatting conflicts must not produce manual merge conflicts. At worst, the automatic result may contain both users' surviving text and a deterministic formatting winner for scalar properties.

Spreadsheets:

- Merge row/column/cell operations by stable IDs.
- Recompute formula projections after structural merge.
- Surface formula conflicts only when semantic intent cannot be inferred.
- Even formula conflicts should not block convergence. The cell should converge to a deterministic formula or error value, with review metadata if needed.

## Open Questions

- Final manifest binary encoding: deterministic CBOR vs a stricter custom/protobuf-like format.
- Exact segment flush thresholds.
- Whether local WAL frames should use the same format as sync operation segments.
- How much user-visible version history to retain after compaction.
- Exact pack-file format for local small-object mitigation.
