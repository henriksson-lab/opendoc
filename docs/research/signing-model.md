# Signing Model Research

Status: initial complete draft.

## Sources

- Sigstore overview: https://docs.sigstore.dev/
- Cosign blob signing: https://docs.sigstore.dev/cosign/signing/signing_with_blobs/
- Sigstore Rust client notes: https://docs.sigstore.dev/language_clients/rust/
- Sigstore Rust crates: https://github.com/sigstore/sigstore-rust
- OpenSSH `ssh-keygen` signing and allowed signers documentation: https://www.man7.org/linux/man-pages/man1/ssh-keygen.1%40%40openssh.html

## Findings

Sigstore is a strong fit for identity-bound, auditable signatures in connected workflows because it combines short-lived identity certificates, signatures, and transparency-log evidence. It does not replace local signatures, because OpenDoc must also support private and offline documents. The common signed boundary should therefore be independent of the signing backend.

The canonical signed bytes should be binary, not JSON. JSON can be exported for inspection, but signed manifests need deterministic binary encoding.

Some signatures should be independent of storage bytes. If a data type has a stable semantic representation, OpenDoc should support typed content signatures over that representation. This lets a signature survive storage-level changes such as recompression, container conversion, or intentional field dropping.

V0 should reuse OpenSSH-style signing identities. OpenSSH already has widely deployed key material, `ssh-keygen -Y sign`, `ssh-keygen -Y verify`, namespaces, and allowed signers files. This avoids inventing a new local-key identity system.

## Threat Model

Signatures protect against:

- undetected modification of signed manifests
- undetected modification of referenced snapshots, operation segments, attachments, and citation libraries
- undetected modification of independently signed binary blobs
- undetected modification of typed semantic content when a typed content signature is present
- false attribution when verifier has the correct identity or key policy

Signatures do not protect against:

- compromised author devices before signing
- malicious content intentionally signed by a trusted author
- loss of private keys
- broken UI rendering unsigned content as trusted
- S3 deletion or rollback unless transparency, retention, or branch-head policy catches it
- fields deliberately excluded from a typed content signature profile

## Signed Boundary

V0 signs canonical storage manifests. The manifest references immutable content by SHA-256 hash. Operation segments are hash-chained and covered by the manifest. Individual operation signatures are deferred.

Arbitrary binary blobs can be signed independently. The byte-level blob signature signs the blob hash and optional typed metadata. It does not sign the S3 path, so the same signed blob can be reused across documents, versions, branches, and shallow clones.

Typed content signatures are also supported. They sign a data-type-specific semantic digest instead of the storage byte digest. The signed envelope must state the content type, canonicalization profile, included fields, excluded fields, and digest.

Examples:

- Image semantic signature: sign decoded normalized pixels plus chosen color/profile metadata, independent of PNG/JPEG/WebP compression bytes.
- FASTQ sequence signature: sign read IDs and nucleotide sequences while explicitly excluding PHRED quality scores, so the signature survives workflows that drop quality scores.
- FASTQ full signature: sign read IDs, sequences, and PHRED scores when quality scores are part of the attested data.

## Canonicalization

Use deterministic binary canonicalization. The v0 candidate is deterministic CBOR for manifests and signature envelopes. Operation segments and snapshots can use their own binary encodings because the manifest signs their cryptographic hashes, not their parsed fields.

## Signature Modes

| Mode | Use | Status |
| --- | --- | --- |
| Local Ed25519 | offline/private documents | required |
| OpenSSH signing | v0 user/collaborator identity and signatures | required |
| minisign/signify-compatible | simple detached file signatures | evaluate |
| Sigstore identity bundle | public/auditable publishing | recommended connected mode |
| KMS-backed signatures | enterprise deployments | deferred |

## Signature Placement

| Placement | Use | Pros | Cons | Recommendation |
| --- | --- | --- | --- | --- |
| Sidecar object | any blob or manifest | format-agnostic, reusable, simple verification | increases S3 object count | default |
| Embedded signature | formats with safe signature container | fewer S3 objects, travels with file | format-specific, may break content hash semantics | optional |
| Manifest-only coverage | blobs referenced by signed manifest | no extra signature object | signature is version-specific, not reusable as blob attestation | acceptable for low-trust internal blobs |
| Typed content sidecar | semantically canonicalized data | storage-independent, survives recompression or field dropping | requires type-specific canonicalizers | recommended when available |

Default policy:

- Manifests have detached sidecar signatures.
- Blobs may have detached sidecar signatures.
- Data types may provide typed content signatures in detached sidecars or embedded native containers.
- Embedded signatures are allowed only when they do not alter the canonical bytes being content-addressed, or when the format defines a stable signed container.
- Verification caches successful byte-signature checks by blob hash and typed-signature checks by semantic digest plus canonicalization profile.
- Unsigned documents open normally. Signature state is surfaced as a visual trust/compliance indicator, not as a default access-control gate.
- Comments are signed as document state by default.
- Anyone may sign a document version, and no signature is required. “Official” status is derived from signature presence and trust-policy validation, not a separate manual flag.
- Formula signatures cover formula source, not cached computed values.
- Signature display state should distinguish `unsigned`, `signed`, `trusted`, `untrusted`, and `broken`.
- Multiple signatures over the same version are allowed.
- Signatures cover current state plus retained history reachable from the signed manifest, not arbitrary unreachable history.
- Signature envelopes include user-visible metadata such as document title, signer display name, signer key identity, and signing timestamp.
- Signer metadata may be self-declared, derived from OpenSSH allowed-signers, or both. OpenSSH-compatible signatures prove possession of a key, not inherent trust.
- Audit/recovery views require access to retained history, not a trusted signature.

## Typed Content Signature Profiles

Typed content signatures must be explicit and profile-driven. A verifier must never infer what was signed from a MIME type alone.

Each profile defines:

- profile ID, for example `opendoc.image.pixels.v0` or `opendoc.fastq.sequence.v0`
- accepted input encodings
- canonical decoder behavior
- normalized semantic representation
- included fields
- excluded fields
- digest algorithm
- whether lossy transformations are allowed
- test vectors

Suggested initial profiles:

| Profile | Signs | Excludes | Survives |
| --- | --- | --- | --- |
| `opendoc.blob.bytes.v0` | exact bytes | nothing | object relocation only |
| `opendoc.image.pixels.v0` | dimensions, normalized pixel values, chosen color metadata | compression bytes, container metadata | PNG/JPEG/WebP recompression if decoded pixels match profile |
| `opendoc.fastq.sequence.v0` | read IDs, sequences, order policy | PHRED scores, comments unless included | dropping quality scores |
| `opendoc.fastq.full.v0` | read IDs, sequences, PHRED scores | incidental wrapping/compression | gzip/plain FASTQ conversion |

Typed signatures are not a replacement for byte hashes in manifests. A manifest still references the stored object by byte hash for integrity and retrieval. The typed signature adds a semantic attestation that may remain valid across different stored objects representing the same signed content.

Invisible structural IDs used for editing, such as paragraph/block UUIDs, should not be included in document-content signatures by default. They are implementation anchors. A separate forensic or full-structure signature profile may include them if needed.

## Recommendation

Implement Rust-native OpenSSH-compatible local signatures first for manifests, byte-level blobs, and typed content profiles. Offer optional `ssh-keygen` integration as a fallback or verification aid. Then add Sigstore verification and optional Sigstore signing. Keep the signature object extensible enough to carry Sigstore bundles.

## Rejected Options

- **Sign rendered exports only.** Rejected because collaborators need signed native document versions.
- **Sign every operation by default.** Rejected for v0 because manifest-level signing plus hash-chained segments gives integrity with lower complexity.
- **Depend only on Sigstore.** Rejected because offline mode needs local-key signing.
- **Canonical JSON signatures.** Rejected because binary manifests are smaller and avoid JSON canonicalization overhead.
- **Only sign blobs through document manifests.** Rejected because reusable binary objects should be independently attestable.
- **Only sign exact storage bytes.** Rejected because important workflows need signatures over semantic content independent of storage encoding.
- **Implicit semantic signing by MIME type.** Rejected because verifiers need explicit included/excluded fields.
- **Invent a new user-key system.** Rejected because OpenSSH keys and allowed-signers workflows already cover the v0 local identity need.
- **Sign spreadsheet cached values as authoritative.** Rejected for v0 because computed values are cache; formula source is the durable authored content.

## Open Risks

- Rust Sigstore crates are still evolving.
- Binary canonicalization must be tested carefully.
- Key rotation and revocation need product-level UX decisions.
- Sidecar signatures increase object counts; embedded signatures reduce object count but need per-format handling.
- Typed content signatures require robust parsers/canonicalizers and careful test vectors.

## Prototype Evidence

`prototypes/signed-manifest` is a std-only integrity prototype that signs by hashing a manifest plus local secret and verifies that tampering fails. It is not cryptographic signing, and exists only to prove the manifest boundary and tamper-detection workflow without external crates.
