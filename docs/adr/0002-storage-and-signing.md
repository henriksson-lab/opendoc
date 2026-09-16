# ADR 0002: Storage And Signing

Status: provisional decision. **Implementation status corrected 2026-09-13** — see
"What the code actually does" below. Several sentences in this ADR described a
manifest signature that did not exist; they are now marked with what is built,
what is wired, and what is neither.

## Context

OpenDoc must support raw S3-compatible storage without a server, local offline mode, and collaborative deployments with a server. It must also support cryptographic signing of document versions.

## Decision

Store immutable content-addressed snapshots, operation segments, and binary blobs, then sign canonical binary manifests that reference those objects by hash. Branch heads are mutable binary pointers to manifests.

A version signature cannot live *inside* the manifest it covers: the manifest is content addressed, so adding the signature would change the hash the signature names. It is therefore a **sidecar keyed by the manifest hash**, exactly as a version label and a blob signature are, and it is stored together with the `VersionCoverageRecord` that is the signed payload. That record is derived from the manifest — never authored — so a signer cannot assert coverage the manifest does not have, and it restates the parent, snapshot, segment and blob hashes so the signed bytes remain readable after the manifest they describe is gone.

Binary blobs may also have reusable independent signatures keyed by blob hash. Data types may additionally define typed content signatures over semantic canonical forms that are independent of storage encoding.

## Rationale

- Signing one canonical manifest avoids signing large mutable document blobs. (The application's *current* document signature does the opposite — it signs the whole encoded snapshot. See below.)
- Immutable object writes map well to S3 and local filesystem storage.
- Hash-addressed objects support deduplication and integrity checks.
- Signed manifests can cover snapshots, operation logs, attachments and the whole parent chain in one verification boundary, because a manifest names each of those by content hash and names its parent, which names *its* parent.
- Binary manifests and operation segments avoid JSON overhead in the hot path.
- Content-addressed blobs allow images and arbitrary binary objects to be shared across documents and versions.
- Blob-level signatures can be reused wherever the same hash appears, which supports cheap shallow clones.
- Typed content signatures support workflows where equivalent content has multiple byte encodings, such as recompressed images or FASTQ data with dropped PHRED scores.

## What a manifest signature does and does not reach

Covering the manifest hash transitively covers, by induction over content
hashes:

- the parent manifest, and therefore the entire ancestry;
- the snapshot object, and therefore the document, workbook and blob metadata;
- every operation segment this manifest names (and each segment names its own
  predecessor);
- the content digest of every blob.

It does **not** reach:

- the branch head, which is a mutable pointer rather than content;
- anything stored as a sidecar keyed by hash — version labels, blob signature
  sidecars, archive tombstones — nor lookup index records or candidate heads,
  because a manifest does not name any of them;
- **presence**. A signature is a statement about bytes. If the ancestor
  manifests a signed version named have simply been deleted, the signed
  manifest is unchanged and its signature still verifies. Detecting that is a
  walk over the repository, not a cryptographic check:
  `Repository::audit_manifest_chain` takes the signed coverage record and
  reports every manifest, snapshot and operation segment it named that the
  store no longer holds. Absent *blob bytes* are reported separately and are
  not treated as truncation, because deferring large blob downloads is a
  supported mode.

## What the code actually does (2026-09-13)

Built, in `opendoc-format`, `opendoc-sign` and `opendoc-store`:

- `VersionCoverageRecord` — the derived, canonical-CBOR payload a version
  signature is taken over.
- `opendoc_sign::sign_version` / `verify_version_signature` /
  `verify_version_coverage_with_public_key` — target is the manifest hash,
  payload is the coverage record.
- `Repository::{write_version_signature, read_signed_version,
  read_version_coverage, audit_manifest_chain}` — the sidecars and the
  truncation walk.

**Wired 2026-09-14.** The explicit
`sign_current_repository_version_with_openssh_private_key` command signs the
already-committed manifest and writes its coverage/signature sidecars; it does
not retain private-key material for a later save. Repository open verifies the
sidecars and audits the ancestry they name. Unreadable sidecars, failed
verification and incomplete ancestry are warnings, never access gates.
Snapshot signatures remain supported as signatures of the snapshot only; they
are not evidence of retained history.

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

- A prototype must reject a manifest if referenced content changes. **Met at
  the crate level** — `opendoc-sign`'s
  `a_version_signature_sees_a_rewritten_rootless_manifest_where_a_snapshot_signature_cannot`
  builds a real manifest chain, removes the parent link and the segments, and
  shows the version signature refusing while the snapshot signature does not.
  Not met at the application level, which does not sign manifests.
- A prototype must reject a signature after manifest mutation. **Met at the
  crate level**, same test plus
  `a_record_that_names_another_manifest_is_refused_even_when_its_payload_matches`.
- A prototype must verify an independently signed blob by hash. Met.
- A prototype must verify a typed content signature for at least one
  lossy/semantic profile. Met.
- Shallow clone tests must open document structure while deferring large blob
  downloads. Partly: `audit_manifest_chain` distinguishes absent blob bytes
  from truncated history
  (`absent_blob_bytes_are_reported_without_being_called_truncation`), but
  nothing in the application defers a download.
- Storage tests must cover filesystem and at least one S3-compatible backend or
  documented stand-in. Met (`opendal` feature).
