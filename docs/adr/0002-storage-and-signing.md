# ADR 0002: Storage And Signing

Status: provisional decision.

## Context

OpenDoc must support raw S3-compatible storage without a server, local offline mode, and collaborative deployments with a server. It must also support cryptographic signing of document versions.

## Decision

Store immutable content-addressed snapshots, operation segments, and binary blobs, then sign canonical binary manifests that reference those objects by hash. Branch heads are mutable binary pointers to signed manifests. Binary blobs may also have reusable independent signatures keyed by blob hash. Data types may additionally define typed content signatures over semantic canonical forms that are independent of storage encoding.

## Rationale

- Signing one canonical manifest avoids signing large mutable document blobs.
- Immutable object writes map well to S3 and local filesystem storage.
- Hash-addressed objects support deduplication and integrity checks.
- Signed manifests can cover snapshots, operation logs, attachments, and citation libraries in one verification boundary.
- Binary manifests and operation segments avoid JSON overhead in the hot path.
- Content-addressed blobs allow images and arbitrary binary objects to be shared across documents and versions.
- Blob-level signatures can be reused wherever the same hash appears, which supports cheap shallow clones.
- Typed content signatures support workflows where equivalent content has multiple byte encodings, such as recompressed images or FASTQ data with dropped PHRED scores.

## Consequences

- Branch head updates need conditional writes or conflict recovery.
- Manifests need deterministic binary canonicalization.
- Offline users can produce valid signed versions but may later need branch reconciliation.
- Local-key signing is required for offline mode; Sigstore-style identity signing is optional for connected publishing.
- Sidecar signatures are the default for blobs and manifests because they are format-agnostic.
- Embedded blob signatures are allowed only for formats with safe native signature containers.
- Typed content signature profiles must explicitly define included and excluded fields.
- A byte hash is still required for stored object integrity even when a typed semantic signature exists.

## Validation Required

- A prototype must reject a manifest if referenced content changes.
- A prototype must reject a signature after manifest mutation.
- A prototype must verify an independently signed blob by hash.
- A prototype must verify a typed content signature for at least one lossy/semantic profile.
- Shallow clone tests must open document structure while deferring large blob downloads.
- Storage tests must cover filesystem and at least one S3-compatible backend or documented stand-in.
