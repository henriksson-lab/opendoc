# Signed Manifest v0

Status: draft.

Purpose: define the object that binds a storage manifest to an author identity and verification material.

The example below is a diagnostic JSON projection. The canonical signed bytes are the deterministic binary encoding of `opendoc.storage-manifest.v0`.

```json
{
  "schema": "opendoc.signed-manifest.v0",
  "manifest_hash": "sha256:...",
  "canonicalization": "binary-deterministic-cbor",
  "hash": "sha256",
  "signatures": [
    {
      "type": "local-ed25519",
      "key_id": "did:key:z...",
      "signature": "base64..."
    },
    {
      "type": "sigstore-bundle",
      "identity": "user@example.com",
      "issuer": "https://accounts.google.com",
      "bundle_path": "documents/doc_01/signatures/sha256-....sigstore.json"
    }
  ]
}
```

## Signed Boundary

The default signed unit is the canonical storage manifest. The manifest references snapshots, operation segments, attachments, and citation libraries by cryptographic hash.

Operation segments are hash-chained. Individual operations are not signed in v0 unless a deployment requires per-author non-repudiation.

## Signed Blob v0

Arbitrary binary blobs can also have their own signature envelope. For exact byte signatures, the signed payload is the blob hash plus optional typed metadata, not the S3 key.

Diagnostic projection:

```json
{
  "schema": "opendoc.signed-blob.v0",
  "blob_hash": "sha256:...",
  "media_type": "image/png",
  "size": 1048576,
  "canonicalization": "binary-deterministic-cbor",
  "signatures": [
    {
      "type": "local-ed25519",
      "key_id": "did:key:z...",
      "signature": "base64..."
    }
  ]
}
```

Default storage is a detached sidecar at `objects/sha256/{first_two}/{rest}.sig`. If the blob format supports a safe embedded signature container, the sidecar can be omitted and the manifest records `signature.mode = "embedded"`.

## Typed Content Signature v0

Some data types support signatures over semantic content rather than exact storage bytes. This is required when the same content can be stored in multiple encodings or when some fields are often discarded intentionally.

Diagnostic projection:

```json
{
  "schema": "opendoc.typed-content-signature.v0",
  "source_blob_hash": "sha256:...",
  "profile": "opendoc.fastq.sequence.v0",
  "semantic_digest": "sha256:...",
  "included_fields": ["read_id", "sequence"],
  "excluded_fields": ["phred_quality"],
  "canonicalization": "profile-defined-binary",
  "signatures": [
    {
      "type": "local-ed25519",
      "key_id": "did:key:z...",
      "signature": "base64..."
    }
  ]
}
```

Typed signatures can be stored as sidecars next to a blob, in a shared signature index keyed by semantic digest, or embedded when the format safely supports it. The envelope must include the profile ID and excluded fields so verifiers know exactly what the signature claims.

## Image Pixel Profile v0

`opendoc.image.pixels.v0` signs decoded image semantics instead of a PNG, JPEG,
WebP, TIFF, or S3 object byte stream. The profile includes width, height, the
normalized color model, and normalized pixel bytes. The initial implemented
normal form is `rgba8-srgb`: four bytes per pixel in row-major order.

The profile explicitly excludes compression bytes, container metadata, and
storage paths. Format-specific decoders are adapters that must produce the same
decoded pixel frame before signing or verification. This keeps image signatures
reusable across recompression and object-store layout changes while exact-byte
blob signatures remain available for byte-for-byte evidence.

## FASTQ Profiles v0

`opendoc.fastq.sequence.v0` signs a canonical sequence-only profile. It
includes read IDs and normalized nucleotide sequences, and explicitly excludes
PHRED quality scores and comments. This lets a signature survive workflows that
drop, rewrite, or regenerate quality scores while preserving read identity and
sequence content.

`opendoc.fastq.full.v0` signs read IDs, normalized nucleotide sequences, and
PHRED quality scores. It is the stricter profile for workflows where quality
scores are evidence, not disposable processing metadata.

Both profiles canonicalize independently of storage paths and line endings,
hash their profile-defined binary representation, and then sign the semantic
digest. Invalid or incomplete FASTQ records fail verification rather than being
silently repaired.

## Verification Workflow

1. Fetch branch head.
2. Fetch manifest.
3. Decode and re-encode the manifest using deterministic binary canonicalization.
4. Hash manifest and compare with branch head and signature bundle.
5. Verify signature.
6. Fetch every referenced blob and verify its hash.
7. If a referenced blob has a declared sidecar or embedded exact-byte signature, verify the blob signature.
8. If a referenced blob has a typed content signature, decode it with the declared profile, recompute the semantic digest, and verify the signature over that semantic digest.
9. Reject if any fetched hash, required signature, key status, profile canonicalization, or identity policy check fails.
