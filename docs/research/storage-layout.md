# Storage Layout Research

Status: initial complete draft.

## Sources

- OpenDAL Rust crate: https://docs.rs/opendal/latest/opendal/
- zarrs crate documentation: https://docs.rs/zarrs/latest/zarrs/
- zarrs stores documentation: https://book.zarrs.dev/stores.html

## Findings

OpenDAL provides one Rust API over many object and file backends, including S3-compatible storage and local filesystem. The OpenDoc storage design should use the lowest common denominator: immutable puts, reads by key, listing for discovery, and conditional writes when available.

JSON is not acceptable as the canonical storage format. It is too verbose for operation logs and awkward for stable signing. JSON examples in schema files are projections for humans only.

For the first implementation pass, on-disk object storage is the required backend. S3-compatible storage is the long-term default target but can be validated after the local object layout is stable. zarrs is relevant because it has filesystem and OpenDAL-backed stores, but the OpenDoc object model should remain independent of Zarr array semantics unless a future binary-array use case needs them.

## Object Layout

See `docs/schema/storage-manifest-v0.md`.

Key classes:

- immutable content blobs under hash-addressed keys
- immutable manifests under manifest hash keys
- mutable branch head pointers
- detached signature bundles
- optional blob-level signature sidecars
- optional signed lookup records
- optional tape/cold-storage tombstones
- optional ephemeral server presence outside the signed state

Canonical encodings:

- branch heads: compact binary record
- manifests: deterministic binary record, v0 candidate deterministic CBOR
- operation segments: CRDT-native or OpenDoc-native binary frames
- snapshots: binary format matching the document type
- binary blobs: raw bytes, content-addressed by hash
- blob signatures: deterministic binary sidecar envelopes unless embedded signatures are available
- lookup records and archive tombstones: deterministic binary signed records
- debug/API/export: JSON projection allowed

## Blob Storage And Shallow Clone

Images and arbitrary binary attachments are first-class content-addressed blobs. They are not copied per document version. A document manifest links to them by hash and metadata.

Shallow clone downloads enough state to open and edit the document structure without eagerly downloading every large blob:

1. Fetch branch head and manifests.
2. Fetch snapshots and operation segments needed for current structure.
3. Record blob references as unresolved content-addressed objects.
4. Fetch image/blob bytes lazily when the viewport, export path, or user action needs them.
5. Verify fetched blobs by hash and, when required, by blob signature.

Because blob references are hash-addressed, the same image can be shared across documents, versions, and branches. A blob signature can be reused anywhere the same hash appears.

Missing attachments or images are allowed in normal editing mode. Renderers should show placeholders with warning/restore state rather than blocking document editing.

## Identity, Lookup, And Archive

Detailed design: `docs/research/archive-and-lookup.md`.

Summary:

- each document has a stable UUID
- DOI is optional and treated as publication metadata or alias
- cross references use UUIDs rather than bucket paths or DOIs
- server deployments can resolve UUIDs through a lookup service
- serverless deployments use signed bucket index records
- no-index deployments can scan bucket manifests to build a local lookup cache
- tape or cold-storage recall is represented by signed tombstones that point to an archive locator

## Modes

### Raw S3-Compatible Mode

1. Write immutable snapshot and operation segment objects.
2. Write any new binary blobs under content-addressed keys.
3. Write optional blob signature sidecars.
4. Write immutable manifest object.
5. Write detached manifest signature object.
6. Conditionally update branch head from old manifest hash to new manifest hash.
7. On stale head failure, fetch remote head and reconcile.

### Local Offline Mode

1. Use the same object layout on filesystem storage.
2. Allow branch head updates without network.
3. Queue sync by comparing manifest graph and object hashes.
4. Reconcile divergent branch heads during later sync.

This is the first-class v0 implementation mode. It should support single-editor use directly and use deterministic simulations to test multi-editor operation merge, branch divergence, and reconciliation.

### Server-Mediated Collaboration Mode

1. Server accepts live operations and presence.
2. Server writes operation segments and snapshots.
3. Server or clients write signed manifests depending on trust policy.
4. Presence remains ephemeral and unsigned.

## Backend Requirements

| Capability | Required | Notes |
| --- | --- | --- |
| Read object by key | yes | all backends |
| Write object by key | yes | immutable writes must be idempotent |
| List prefix | yes | used for repair/discovery |
| Conditional put | preferred | needed for safe raw S3 branch heads |
| Multipart upload | later | large attachments and snapshots |
| Object versioning | optional | useful recovery layer |
| Object lock | optional | compliance feature, not v0 core |
| Prefix scan | required for serverless lookup fallback | needed when no server or index exists |

## Recommendation

Use content-addressed immutable objects plus signed binary manifests. Treat images and arbitrary binary files as reusable content-addressed blobs with optional independent signatures. Use OpenDAL for backend abstraction, but keep raw S3 conflict handling explicit because backend guarantees vary.

For lookup and archive, use a hybrid design: signed per-document manifests remain authoritative, signed index records provide efficient lookup, and signed tombstones provide tape/cold-storage recall hints when hot objects are absent.

Implementation order:

1. Define a small object-store trait around put-if-absent, get, list-prefix, and compare-and-swap head update.
2. Implement it for local filesystem/on-disk storage.
3. Add simulation tests for concurrent branch heads and merge commits.
4. Add OpenDAL filesystem/S3-compatible adapters once the local format is stable.
5. Evaluate zarrs only where its storage abstraction or chunking model provides measurable value.

Local disk small-file mitigation:

- keep the logical object model content-addressed
- pack small immutable objects into append-only pack files
- keep a signed or hash-checked pack index
- allow loose objects during active editing
- compact loose small objects into packs in the background
- keep large blobs as separate objects for streaming and shallow clone
- keep the storage API independent of the physical pack format so the local layout can change later
- allow local maintenance to rewrite pack files while preserving logical object IDs, hashes, and verification results
- rewrite packs crash-safely by writing a new pack, verifying it, then atomically swapping the pack index

Compaction is an internal maintenance task. Users should not need to understand or trigger it.

## Rejected Options

- **Single mutable document object.** Rejected because signing, history, and conflict recovery are weak.
- **Database-only storage.** Rejected because raw S3-compatible mode is a requirement.
- **Server-only branch authority.** Rejected for v0 because local offline and single-user modes must work without a server.
- **Canonical JSON storage.** Rejected because operation logs and manifests need compact deterministic binary encoding.
- **Copy blobs into each document version.** Rejected because it prevents cheap shallow cloning and deduplication.
- **Require embedded signatures for all blobs.** Rejected because many binary formats do not have a safe native signature container.
- **Central lookup service only.** Rejected because raw bucket and offline deployments must work without a server.
- **Tombstones only.** Rejected because lookup would require expensive scans for normal cross-reference resolution.
- **Index only.** Rejected because moved/cold objects need object-level recall metadata that can travel with the missing object reference.

## Open Risks

- Conditional write support differs across S3-compatible systems.
- Listing consistency can affect discovery logic.
- Garbage collection needs retention and signature policy.
- Blob availability is separate from manifest integrity; shallow clones may have valid manifests but missing lazy blobs.
- Bucket scans can be expensive at large scale; signed indexes should be maintained opportunistically.
- Tape recall may be manual and slow; tombstones must make latency and restore policy explicit.
- Local object stores need pack files or similar mitigation for many small immutable objects.
- Pack-file signing policy is unresolved: sign whole packs for transfer integrity, logical contained objects for semantic integrity, or both.
- Permission enforcement is not part of raw/local v0 storage. Without a server, filesystem or bucket permissions are the real enforcement boundary. Keep metadata/extensibility for future server-backed permissions.
- Permission metadata may exist as non-enforcing hints in local/raw modes if cheap to support.

## Prototype Evidence

The signed-manifest prototype writes a manifest-like artifact and verifies hash integrity locally. A full OpenDAL proof awaits dependency installation.
