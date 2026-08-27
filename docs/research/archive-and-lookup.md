# Archive And Lookup Model

Status: draft.

## Problem

OpenDoc needs to resolve document cross references and missing objects without assuming a continuously available server. It also needs to support magnetic tape or other cold archives where the hot S3-compatible object may be absent for long periods.

The design must support:

- stable document identity
- optional DOI metadata
- UUID-based cross references
- server-backed lookup when available
- bucket-only lookup when no server is available
- expensive but possible bucket scans as a fallback
- object recall from magnetic tape
- compatibility with manifest and blob signing

## Identity

Every document has a stable document UUID generated at creation time. Use UUIDv7 unless there is a strong reason not to; it is sortable and still globally unique enough for bucket-scale indexing.

DOI is optional and should not be the primary internal identifier:

- A DOI may identify a published version, not the mutable working document.
- A document may have no DOI.
- A document may gain a DOI later.
- Multiple internal versions may relate to the same DOI through publication metadata.

Cross references should use:

```text
opendoc://uuid/{document_uuid}
opendoc://uuid/{document_uuid}?branch=main
opendoc://uuid/{document_uuid}?manifest=sha256:...
```

The UUID resolves to one or more manifests. The manifest and signatures then establish integrity.

Resolution should degrade gracefully. References default to latest known branch head with warnings, and users may pin exact manifest versions when needed. If indexes are stale or missing, a client may scan. If blobs are archived, a client may show tombstone restore state. Broken references should not corrupt or block opening the source document.

## Lookup Layers

Use a hybrid model.

### 1. Local Cache

Each client keeps a local lookup cache built from manifests, index records, and scans.

The cache is never authoritative. It accelerates lookup only.

### 2. Server Lookup

When available, a server can maintain:

- UUID -> current branch heads
- DOI -> document UUID or published manifest
- semantic digest -> blob signature record
- archive locator -> recall state

The server returns signed records or records that lead to signed manifests. Clients still verify manifests and signatures.

### 3. Bucket Index Records

Serverless deployments store signed binary index records in the bucket:

```text
indexes/by-uuid/{first4}/{next4}/{document_uuid}.idx
indexes/by-doi/{doi_hash}.idx
indexes/by-semantic/{profile}/{digest_prefix}/{semantic_digest}.idx
```

Index records contain candidate manifest hashes and branch head locations. They are hints, not authority. A client must fetch the referenced manifest and verify that it contains the expected UUID or DOI.

### 4. Bucket Scan Fallback

If no server and no index record is available, a client can scan bucket prefixes:

```text
documents/*/manifests/*.manifest
indexes/by-uuid/**
indexes/by-doi/**
```

The client builds a local cache by decoding manifests and reading document UUID/DOI fields.

This is expensive but important for disaster recovery, raw-bucket portability, and tape restores.

## Magnetic Tape And Cold Archive

Cold archive is represented by signed tombstones.

For HPC2N-like environments, public documentation points to IBM tape libraries managed with IBM Spectrum Protect/TSM, project storage on Lustre, and SweStore/dCache for research storage. The v0 archive model should therefore not assume LTFS paths as the only concrete locator. Use generic signed locators first, with profile-specific support for IBM Spectrum Protect/TSM restore IDs, dCache/SweStore paths, LTFS paths, tar bundle members, and institutional archive request IDs.

If an object is moved to tape, the hot content object may be replaced by:

```text
archive/tombstones/{object_hash}.tombstone
```

A tombstone records:

- original object hash
- object role: manifest, snapshot, operation segment, blob, signature, index
- media type or format profile
- archive set ID
- tape volume ID
- LTFS path, inventory barcode, or external archive locator
- archive system profile, such as `ibm-spectrum-protect`, `dcache`, `ltfs`, `tar-bundle`, or `generic`
- restore policy
- expected latency
- restore contact or operator metadata, if appropriate
- tombstone creation time
- tombstone signer

Tombstones are signed. They do not prove the original content, only the recall metadata. After recovery, restored bytes must hash to the original object hash and pass any required signatures.

## Recall Flow

1. Client fetches manifest and sees a missing object hash.
2. Client checks for `archive/tombstones/{object_hash}.tombstone`.
3. Client verifies the tombstone signature.
4. Client displays or queues restore using archive locator and restore policy.
5. Operator or archive service restores object bytes.
6. Client verifies restored bytes against the original hash.
7. Client verifies byte-level or typed-content signatures.
8. Client updates local cache and optionally writes a restored hot object.

## Tombstone Versus Central Lookup

| Option | Strength | Weakness | Use |
| --- | --- | --- | --- |
| Tombstone per archived object | travels with missing object, precise recall metadata | more objects and metadata | object-level cold storage |
| Central lookup service | compact and fast | requires server availability | managed deployments |
| Bucket index records | serverless lookup acceleration | must be maintained and can become stale | raw S3 mode |
| Bucket scan | no extra service or index required | expensive at scale | disaster recovery and fallback |

Recommendation: use all four layers. Manifests are authoritative. Indexes and servers accelerate lookup. Tombstones handle object recall. Scans are the final fallback.

## Signing Compatibility

Archive and lookup records are signed sidecar-style records.

- The manifest signs referenced object hashes.
- Index records are signed hints and must be checked against manifests.
- Tombstones are signed recall hints and must be checked against restored bytes.
- Blob byte signatures and typed content signatures remain valid after tape restore if restored data matches the signed hash or semantic digest.

This preserves signing semantics while allowing hot storage, cold storage, and lookup systems to vary independently.

## Open Questions

- Whether UUIDv7 should be mandatory or merely recommended.
- Exact binary encoding for index and tombstone records.
- Whether tombstones should live next to original object paths, in `archive/tombstones/`, or both.
- How much archive operator metadata belongs in signed records.
- Whether a bucket-level inventory file should be generated periodically to avoid full scans.
