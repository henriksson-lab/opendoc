# Storage Manifest v0

Status: draft.

Purpose: define the signed, content-addressed root object for one document branch.

The examples below are diagnostic JSON projections only. The canonical stored and signed representation must be binary.

## Canonical Encoding

- Manifests are encoded as deterministic binary records.
- The v0 candidate is deterministic CBOR for manifests and head pointers because it is schema-flexible, inspectable with tooling, and has a canonical form suitable for signatures.
- Large document snapshots and operation segments use domain-native binary frames, not generic JSON.
- Operation segments should be compressed in blocks with zstd once dependency-backed prototypes begin.
- JSON may be emitted for debugging, tests, API responses, and export, but not as the canonical storage or signature format.

```json
{
  "schema": "opendoc.storage-manifest.v0",
  "document_id": "doc_01",
  "document_uuid": "018f5f5c-6f54-7c00-9a7e-7dd21c07e2b1",
  "doi": "10.1234/example.doi",
  "branch": "main",
  "parent_manifest": "sha256:...",
  "snapshot": {
    "hash": "sha256:...",
    "path": "objects/sha256/ab/cd..."
  },
  "operation_segments": [
    {
      "hash": "sha256:...",
      "path": "objects/sha256/ef/01...",
      "first_operation": "op_100",
      "last_operation": "op_200"
    }
  ],
  "blobs": [
    {
      "hash": "sha256:...",
      "path": "objects/sha256/12/34...",
      "media_type": "image/png",
      "logical_name": "figures/overview.png",
      "size": 1048576,
      "signature": {
        "mode": "sidecar",
        "path": "objects/sha256/12/34....sig",
        "profile": "opendoc.blob.bytes.v0"
      },
      "typed_signatures": [
        {
          "profile": "opendoc.image.pixels.v0",
          "semantic_digest": "sha256:...",
          "mode": "sidecar",
          "path": "objects/semantic/sha256/ab/cd....sig"
        }
      ]
    }
  ],
  "attachments": [],
  "citation_libraries": [],
  "lookup": {
    "aliases": [
      { "scheme": "uuid", "value": "018f5f5c-6f54-7c00-9a7e-7dd21c07e2b1" },
      { "scheme": "doi", "value": "10.1234/example.doi" }
    ],
    "index_records": [
      "indexes/by-uuid/018f/5f5c/018f5f5c-6f54-7c00-9a7e-7dd21c07e2b1.idx"
    ]
  },
  "archive_tombstones": [
    {
      "object_hash": "sha256:...",
      "object_role": "blob",
      "archive_id": "tape-set-2026-08",
      "locator": "ltfs://shelf-a/volume-0007/objects/sha256/ab/cd...",
      "restore": {
        "policy": "manual-or-operator",
        "expected_latency": "hours-days"
      }
    }
  ],
  "created_at": "2026-08-25T00:00:00Z",
  "author": "did:key:z..."
}
```

## Object Layout

- `documents/{document_id}/heads/{branch}.head`: mutable binary branch head pointer.
- `documents/{document_id}/manifests/{manifest_hash}.manifest`: immutable binary manifest.
- `objects/sha256/{first_two}/{rest}`: immutable content blobs.
- `objects/sha256/{first_two}/{rest}.sig`: optional detached sidecar signature for a content blob.
- `indexes/by-uuid/{prefix}/{document_uuid}.idx`: optional signed lookup record for document UUIDs.
- `indexes/by-doi/{doi_hash}.idx`: optional signed lookup record for DOI aliases.
- `archive/tombstones/{object_hash}.tombstone`: optional signed recall metadata for data moved to tape or cold storage.
- `documents/{document_id}/signatures/{manifest_hash}.sig`: detached binary signature bundle.
- `documents/{document_id}/presence/`: optional ephemeral server-mediated state, never required for raw S3 mode.

## Identity And Lookup

Every document must have a stable UUID. DOI is optional metadata and may point to a published version rather than the mutable working document.

- Cross references should use document UUID plus optional branch/version selector.
- DOI should be treated as an external publication alias, not the primary internal key.
- Server deployments may maintain a database-backed lookup service.
- Serverless deployments maintain signed index records in the bucket.
- If no index is available, clients may scan manifests in a bucket and build a local lookup cache.

Signed lookup records are hints, not authority. The referenced manifest must still verify and contain the expected UUID or DOI.

## Tape And Cold Archive

Objects may be moved out of hot S3-compatible storage to magnetic tape or another cold archive. In that case, the hot object can be replaced by a signed tombstone.

A tombstone records:

- original object hash
- original object role
- archive set ID
- tape volume or external locator
- restore policy
- expected latency
- optional operator/contact metadata
- tombstone signature

Tombstones do not satisfy content verification. They only prove that the system knows where the missing object should be recoverable. After recall, the restored bytes must hash to the original object hash and any required byte or typed-content signatures must verify.

## Blob References

Documents may reference arbitrary binary blobs, including images, embedded files, generated previews, imported PDFs, and spreadsheet attachments.

Blob references are content-addressed by hash and may be reused across documents and versions. A manifest references the hash and optional metadata; the blob bytes live in the global object store.

This supports shallow clone:

- fetch branch head
- fetch manifests
- fetch operation segments and snapshots needed for structure
- defer fetching large blobs until viewed, exported, or explicitly materialized

Shallow clones remain verifiable because the manifest contains blob hashes. A missing blob is an availability issue, not an integrity failure. If the blob is later fetched, its hash and optional blob signature are verified before use.

## Blob Signatures

Binary objects can be signed independently of document manifests.

- Default exact-byte signature: detached sidecar signature next to the content-addressed blob.
- Optional: embedded signature if the binary format has a safe native signature container.
- Manifest coverage: the manifest signs the blob hash and metadata.
- Blob signature coverage: the blob signature signs the blob hash and optional typed metadata, not the path.
- Typed content signature coverage: a data-type-specific semantic digest plus profile metadata, not the storage bytes.

Reusable blob signatures avoid re-signing and repeated full-object hashing when the same image or binary object is referenced by many manifests. Verification can cache `hash -> signature status`.

Typed content signatures can be reused across different byte blobs that have the same semantic digest. For example, a PNG and a WebP may share an image pixel signature if the declared image profile decodes them to the same normalized pixels. A FASTQ file with PHRED scores and a derived FASTQ/FASTA-like object without PHRED scores may share a sequence-only signature if the declared profile excludes quality scores.

## Write Rules

- Immutable object writes are idempotent.
- Blob sidecar signature writes are idempotent and keyed by the blob hash.
- Branch head writes must use conditional put when the backend supports it.
- If conditional put is unavailable, writers create candidate heads and require manual or server-mediated reconciliation.
- A signed manifest is valid only if every referenced hash matches downloaded bytes.

## Commit Granularity

A keypress is an edit operation, not necessarily a version-control commit.

- **Edit operation:** the smallest collaboration change, such as inserting a character, deleting a range, applying a mark, or moving a row.
- **Operation segment:** a binary batch of operations, usually flushed after a size threshold, short idle timer, transaction boundary, or explicit save.
- **Snapshot:** compact binary materialization of current document state.
- **Manifest commit:** the signed version-control unit that points to a snapshot and operation segments.
- **Named version:** a user-visible manifest commit given a label.

The default implementation should batch many keypresses into one operation segment and many operation segments into one signed manifest commit. Per-keypress signing or per-keypress S3 objects are explicitly not the default.
