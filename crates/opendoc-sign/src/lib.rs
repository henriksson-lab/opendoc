use opendoc_core::{digest_bytes, HashRef};
use opendoc_format::{ManifestRecord, SignatureRecord, VersionCoverageRecord};
use ssh_key::{HashAlg, LineEnding, PrivateKey, PublicKey, SshSig};
use std::collections::hash_map::DefaultHasher;
use std::fmt;
use std::hash::{Hash, Hasher};
#[cfg(not(target_arch = "wasm32"))]
use std::time::{SystemTime, UNIX_EPOCH};

pub const OPENSSH_NAMESPACE: &str = "opendoc-v0";

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SignatureState {
    Unsigned,
    Signed,
    Trusted,
    Untrusted,
    Broken,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Signer {
    pub key_identity: String,
    pub display_name: String,
}

pub trait SignerBackend {
    fn sign(&self, namespace: &str, payload: &[u8]) -> Result<Vec<u8>, SignError>;
    fn verify(&self, namespace: &str, payload: &[u8], signature: &[u8]) -> Result<bool, SignError>;
}

pub fn openssh_signed_payload(namespace: &str, payload: &[u8]) -> Result<Vec<u8>, SignError> {
    SshSig::signed_data(namespace, HashAlg::Sha256, payload)
        .map_err(|err| SignError::Backend(err.to_string()))
}

#[derive(Clone, Debug)]
pub struct OpenSshSigner {
    private_key: PrivateKey,
    public_key: PublicKey,
}

impl OpenSshSigner {
    pub fn from_private_key_pem(pem: impl AsRef<[u8]>) -> Result<Self, SignError> {
        let private_key =
            PrivateKey::from_openssh(pem).map_err(|err| SignError::Backend(err.to_string()))?;
        let public_key = private_key.public_key().clone();
        Ok(Self {
            private_key,
            public_key,
        })
    }

    pub fn from_keypair(private_key: PrivateKey, public_key: PublicKey) -> Self {
        Self {
            private_key,
            public_key,
        }
    }

    pub fn public_key(&self) -> &PublicKey {
        &self.public_key
    }

    pub fn public_key_openssh(&self) -> Result<String, SignError> {
        self.public_key
            .to_openssh()
            .map_err(|err| SignError::Backend(err.to_string()))
    }
}

impl SignerBackend for OpenSshSigner {
    fn sign(&self, namespace: &str, payload: &[u8]) -> Result<Vec<u8>, SignError> {
        let signature = self
            .private_key
            .sign(namespace, HashAlg::Sha256, payload)
            .map_err(|err| SignError::Backend(err.to_string()))?;
        signature
            .to_pem(LineEnding::LF)
            .map(|pem| pem.into_bytes())
            .map_err(|err| SignError::Backend(err.to_string()))
    }

    fn verify(&self, namespace: &str, payload: &[u8], signature: &[u8]) -> Result<bool, SignError> {
        verify_openssh_signature(&self.public_key, namespace, payload, signature)
    }
}

pub fn verify_openssh_signature(
    public_key: &PublicKey,
    namespace: &str,
    payload: &[u8],
    signature: &[u8],
) -> Result<bool, SignError> {
    let pem = std::str::from_utf8(signature).map_err(|err| SignError::Backend(err.to_string()))?;
    let signature = SshSig::from_pem(pem).map_err(|err| SignError::Backend(err.to_string()))?;
    match public_key.verify(namespace, payload, &signature) {
        Ok(()) => Ok(true),
        Err(_) => Ok(false),
    }
}

#[derive(Clone, Debug)]
pub struct ResearchSigner {
    secret: String,
}

impl ResearchSigner {
    pub fn new(secret: impl Into<String>) -> Self {
        Self {
            secret: secret.into(),
        }
    }
}

impl SignerBackend for ResearchSigner {
    fn sign(&self, namespace: &str, payload: &[u8]) -> Result<Vec<u8>, SignError> {
        Ok(research_digest(namespace, payload, &self.secret).into_bytes())
    }

    fn verify(&self, namespace: &str, payload: &[u8], signature: &[u8]) -> Result<bool, SignError> {
        Ok(self.sign(namespace, payload)? == signature)
    }
}

pub fn sign_target(
    backend: &impl SignerBackend,
    target: HashRef,
    title: impl Into<String>,
    signer: Signer,
    payload: &[u8],
) -> Result<SignatureRecord, SignError> {
    let signer_key = signer.key_identity.trim().to_string();
    let signer_display = signer.display_name.trim().to_string();
    let title = title.into().trim().to_string();
    SignatureRecord {
        target: target.clone(),
        signer: signer_key.clone(),
        signer_display: signer_display.clone(),
        title: title.clone(),
        signed_at_ms: 0,
        signature: vec![0],
    }
    .validate()
    .map_err(|err| SignError::Backend(err.to_string()))?;
    let record = SignatureRecord {
        target,
        signer: signer_key,
        signer_display,
        title,
        signed_at_ms: now_ms(),
        signature: backend.sign(OPENSSH_NAMESPACE, payload)?,
    };
    record
        .validate()
        .map_err(|err| SignError::Backend(err.to_string()))?;
    Ok(record)
}

pub fn verify_record(
    backend: &impl SignerBackend,
    record: &SignatureRecord,
    payload: &[u8],
) -> Result<SignatureState, SignError> {
    if record.signature.is_empty() {
        return Ok(SignatureState::Unsigned);
    }
    if backend.verify(OPENSSH_NAMESPACE, payload, &record.signature)? {
        Ok(SignatureState::Signed)
    } else {
        Ok(SignatureState::Broken)
    }
}

pub fn verify_record_for_target(
    backend: &impl SignerBackend,
    record: &SignatureRecord,
    expected_target: &HashRef,
    payload: &[u8],
) -> Result<SignatureState, SignError> {
    if &record.target != expected_target {
        return Ok(SignatureState::Broken);
    }
    verify_record(backend, record, payload)
}

pub fn verify_record_with_public_key(
    record: &SignatureRecord,
    payload: &[u8],
) -> Result<SignatureState, SignError> {
    if record.signature.is_empty() {
        return Ok(SignatureState::Unsigned);
    }
    let public_key = PublicKey::from_openssh(&record.signer)
        .map_err(|err| SignError::Backend(err.to_string()))?;
    if verify_openssh_signature(&public_key, OPENSSH_NAMESPACE, payload, &record.signature)? {
        Ok(SignatureState::Signed)
    } else {
        Ok(SignatureState::Broken)
    }
}

pub fn verify_record_for_target_with_public_key(
    record: &SignatureRecord,
    expected_target: &HashRef,
    payload: &[u8],
) -> Result<SignatureState, SignError> {
    if &record.target != expected_target {
        return Ok(SignatureState::Broken);
    }
    verify_record_with_public_key(record, payload)
}

/// Sign a committed version: the manifest, and through it the history the
/// manifest names.
///
/// # What the target is
///
/// The signature's `target` is the content hash of the encoded
/// [`ManifestRecord`]. The signed payload is the canonical CBOR encoding of
/// the [`VersionCoverageRecord`] derived from that manifest — so the payload
/// both *is* determined by the manifest and *says*, in readable fields, what
/// it determined: the parent, the snapshot, the operation segments and the
/// blob digests.
///
/// This is a different target from [`sign_target`] as the application uses it
/// today, which signs the encoded snapshot and nothing else. That signature is
/// blind to everything outside the snapshot: rewrite the head manifest with no
/// parent and no operation segments and the snapshot is byte-identical, so it
/// still verifies over a document whose history has been erased.
///
/// The caller must store the returned coverage record alongside the signature
/// — `opendoc_store::Repository::write_version_signature` does both — because
/// it is the only stored form of the payload, and the only form in which the
/// version's claim about its own ancestry survives the loss of the manifest.
pub fn sign_version(
    backend: &impl SignerBackend,
    manifest: &ManifestRecord,
    title: impl Into<String>,
    signer: Signer,
) -> Result<(VersionCoverageRecord, SignatureRecord), SignError> {
    let coverage = version_coverage(manifest)?;
    let payload = version_payload(&coverage)?;
    let record = sign_target(backend, coverage.manifest.clone(), title, signer, &payload)?;
    Ok((coverage, record))
}

/// Verify a version signature against the manifest a verifier actually holds.
///
/// # What is checked
///
/// 1. The coverage record is **re-derived** from `manifest`; nothing the
///    signer or the repository supplied is trusted to describe it.
/// 2. `record.target` must equal that manifest's hash, so a signature cannot
///    be lifted from one version onto another.
/// 3. The signature must verify over the canonical encoding of the derived
///    coverage.
///
/// # What this detects that a snapshot signature cannot
///
/// **Truncated history.** The parent hash, the operation segment hashes and
/// the blob digests are all inside the manifest, so they are all inside the
/// target. Dropping the parent link, dropping a segment, or swapping a blob
/// changes the manifest, changes its hash, and the signature no longer names
/// this version at all.
///
/// # What it still does not detect
///
/// **Deletion.** This is a statement about bytes, not about availability: if
/// the ancestor manifests the signed chain names have simply been removed from
/// the store, this manifest is unchanged and this check still says `Signed`.
/// Pair it with `opendoc_store::Repository::audit_manifest_chain`, which walks
/// the signed coverage's ancestry and reports what the repository no longer
/// holds.
pub fn verify_version_signature(
    backend: &impl SignerBackend,
    record: &SignatureRecord,
    manifest: &ManifestRecord,
) -> Result<SignatureState, SignError> {
    let coverage = version_coverage(manifest)?;
    verify_record_for_target(
        backend,
        record,
        &coverage.manifest,
        &version_payload(&coverage)?,
    )
}

/// [`verify_version_signature`] using the public key carried in the record.
pub fn verify_version_signature_with_public_key(
    record: &SignatureRecord,
    manifest: &ManifestRecord,
) -> Result<SignatureState, SignError> {
    let coverage = version_coverage(manifest)?;
    verify_record_for_target_with_public_key(
        record,
        &coverage.manifest,
        &version_payload(&coverage)?,
    )
}

/// Verify a version signature against a *stored* coverage record, for the case
/// where the manifest itself is no longer in the repository.
///
/// The coverage record is not trusted for being stored: it is the payload, so
/// a coverage record that has been edited — a parent link removed, a segment
/// dropped — does not verify. What it buys is that a version's claim about its
/// own ancestry remains readable and provable after the manifest it describes
/// is gone, which is exactly the state a truncated repository is in.
pub fn verify_version_coverage_with_public_key(
    record: &SignatureRecord,
    coverage: &VersionCoverageRecord,
) -> Result<SignatureState, SignError> {
    verify_record_for_target_with_public_key(
        record,
        &coverage.manifest,
        &version_payload(coverage)?,
    )
}

fn version_coverage(manifest: &ManifestRecord) -> Result<VersionCoverageRecord, SignError> {
    VersionCoverageRecord::for_manifest(manifest).map_err(|err| SignError::Backend(err.to_string()))
}

fn version_payload(coverage: &VersionCoverageRecord) -> Result<Vec<u8>, SignError> {
    coverage
        .signing_payload()
        .map_err(|err| SignError::Backend(err.to_string()))
}

pub fn sign_blob_hash(
    backend: &impl SignerBackend,
    blob_hash: HashRef,
    name: impl Into<String>,
    signer: Signer,
) -> Result<SignatureRecord, SignError> {
    let payload = blob_hash.to_string();
    sign_target(backend, blob_hash, name, signer, payload.as_bytes())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypedContentSignature {
    pub source_blob: HashRef,
    pub profile: String,
    pub semantic_digest: HashRef,
    pub included_fields: Vec<String>,
    pub excluded_fields: Vec<String>,
    pub signature: SignatureRecord,
}

pub trait TypedSignatureProfile {
    fn profile_id(&self) -> &'static str;
    fn semantic_digest(&self, bytes: &[u8]) -> Result<HashRef, SignError>;
    fn included_fields(&self) -> Vec<String>;
    fn excluded_fields(&self) -> Vec<String>;
}

#[derive(Clone, Debug, Default)]
pub struct ResearchUtf8NoWhitespaceProfile;

impl TypedSignatureProfile for ResearchUtf8NoWhitespaceProfile {
    fn profile_id(&self) -> &'static str {
        "opendoc.research.utf8-no-whitespace.v0"
    }

    fn semantic_digest(&self, bytes: &[u8]) -> Result<HashRef, SignError> {
        let text = std::str::from_utf8(bytes).map_err(|err| SignError::Backend(err.to_string()))?;
        let normalized: String = text.chars().filter(|ch| !ch.is_whitespace()).collect();
        digest_bytes("sha256", normalized.as_bytes())
            .map_err(|err| SignError::Backend(err.to_string()))
    }

    fn included_fields(&self) -> Vec<String> {
        vec!["utf8_non_whitespace_codepoints".to_string()]
    }

    fn excluded_fields(&self) -> Vec<String> {
        vec!["whitespace".to_string()]
    }
}

#[derive(Clone, Debug, Default)]
pub struct FastqSequenceProfile;

impl TypedSignatureProfile for FastqSequenceProfile {
    fn profile_id(&self) -> &'static str {
        "opendoc.fastq.sequence.v0"
    }

    fn semantic_digest(&self, bytes: &[u8]) -> Result<HashRef, SignError> {
        let records = parse_fastq_records(bytes)?;
        digest_bytes(
            "sha256",
            &canonical_fastq_records(&records, FastqCanonicalMode::SequenceOnly),
        )
        .map_err(|err| SignError::Backend(err.to_string()))
    }

    fn included_fields(&self) -> Vec<String> {
        vec!["read_id".to_string(), "sequence".to_string()]
    }

    fn excluded_fields(&self) -> Vec<String> {
        vec!["phred_quality".to_string(), "comments".to_string()]
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecodedImagePixels {
    pub width: u32,
    pub height: u32,
    pub color_model: String,
    pub pixels: Vec<u8>,
}

impl DecodedImagePixels {
    pub fn rgba8(width: u32, height: u32, pixels: Vec<u8>) -> Result<Self, SignError> {
        let expected = width
            .checked_mul(height)
            .and_then(|count| count.checked_mul(4))
            .ok_or_else(|| SignError::Backend("image dimensions overflow".to_string()))?
            as usize;
        if pixels.len() != expected {
            return Err(SignError::Backend(format!(
                "RGBA8 image expected {expected} bytes, got {}",
                pixels.len()
            )));
        }
        Ok(Self {
            width,
            height,
            color_model: "rgba8-srgb".to_string(),
            pixels,
        })
    }
}

/// Pixels the *signer asserts* are the decoding of a particular container.
///
/// OpenDoc has no image decoder in this graph — the pixels arrive as command
/// arguments, from whatever decoded the container — so this profile cannot
/// claim to have checked that they really are `source`'s pixels. What it can
/// do, and what it now does, is bind the two together: the container's content
/// digest is part of the frame that gets signed, so the attestation says
/// "*this* signer says *these* pixels come from *that* blob" and cannot be
/// lifted onto a different blob.
///
/// Before this type existed, `width`, `height` and `pixels` came from the
/// caller and verification recomputed the digest from the frame stored beside
/// the signature — the same caller-supplied bytes — so the check could never
/// disagree and a signature could be transplanted onto any image at all.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssertedImagePixels {
    /// Content digest of the container bytes the pixels are asserted to decode
    /// from. Verification recomputes this from the blob it actually holds.
    pub source: HashRef,
    pub pixels: DecodedImagePixels,
}

impl AssertedImagePixels {
    /// Bind `pixels` to the bytes they are asserted to have been decoded from.
    pub fn from_container(container: &[u8], pixels: DecodedImagePixels) -> Result<Self, SignError> {
        Ok(Self {
            source: digest_bytes("sha256", container)
                .map_err(|err| SignError::Backend(err.to_string()))?,
            pixels,
        })
    }
}

#[derive(Clone, Debug, Default)]
pub struct ImagePixelsProfile;

impl ImagePixelsProfile {
    pub fn semantic_digest_pixels(
        &self,
        image: &AssertedImagePixels,
    ) -> Result<HashRef, SignError> {
        digest_bytes("sha256", &canonical_image_pixels(image))
            .map_err(|err| SignError::Backend(err.to_string()))
    }

    /// The container digest a stored frame binds itself to.
    ///
    /// The verifier needs this to compare against the blob it actually has;
    /// reading it through the profile keeps the frame format in one place.
    pub fn asserted_source(&self, frame: &[u8]) -> Result<HashRef, SignError> {
        Ok(parse_image_pixels_frame(frame)?.source)
    }
}

impl TypedSignatureProfile for ImagePixelsProfile {
    fn profile_id(&self) -> &'static str {
        "opendoc.image.pixels.v0"
    }

    fn semantic_digest(&self, bytes: &[u8]) -> Result<HashRef, SignError> {
        let image = parse_image_pixels_frame(bytes)?;
        self.semantic_digest_pixels(&image)
    }

    fn included_fields(&self) -> Vec<String> {
        // Named for what they are: a digest of the container, which is checked,
        // and a frame of pixels, which is the signer's word.
        vec![
            "source_blob_digest".to_string(),
            "asserted_width".to_string(),
            "asserted_height".to_string(),
            "asserted_color_model".to_string(),
            "asserted_normalized_pixels".to_string(),
        ]
    }

    fn excluded_fields(&self) -> Vec<String> {
        vec![
            "compression".to_string(),
            "container_metadata".to_string(),
            "storage_path".to_string(),
            // Stated rather than implied: nothing in OpenDoc decodes the
            // container to check the pixels against it.
            "independent_decode_of_the_container".to_string(),
        ]
    }
}

pub fn image_pixels_frame(image: &AssertedImagePixels) -> Vec<u8> {
    canonical_image_pixels(image)
}

fn parse_image_pixels_frame(bytes: &[u8]) -> Result<AssertedImagePixels, SignError> {
    const MAGIC: &[u8] = b"opendoc.image.pixels.v0\n";
    if !bytes.starts_with(MAGIC) {
        return Err(SignError::Backend(
            "image pixel profile frame has invalid magic".to_string(),
        ));
    }
    let mut offset = MAGIC.len();
    let source = read_bytes_field(bytes, &mut offset, b"source")?;
    let width = read_u32_field(bytes, &mut offset, b"width")?;
    let height = read_u32_field(bytes, &mut offset, b"height")?;
    let color_model = read_bytes_field(bytes, &mut offset, b"color")?;
    let pixels = read_bytes_field(bytes, &mut offset, b"pixels")?;
    if offset != bytes.len() {
        return Err(SignError::Backend(
            "image pixel profile frame has trailing bytes".to_string(),
        ));
    }
    let source = String::from_utf8(source)
        .map_err(|err| SignError::Backend(format!("image source digest is not UTF-8: {err}")))?;
    let source = HashRef::parse(&source)
        .map_err(|err| SignError::Backend(format!("image source digest is invalid: {err}")))?;
    let color_model = String::from_utf8(color_model)
        .map_err(|err| SignError::Backend(format!("image color model is not UTF-8: {err}")))?;
    let image = AssertedImagePixels {
        source,
        pixels: DecodedImagePixels {
            width,
            height,
            color_model,
            pixels,
        },
    };
    validate_image_pixels(&image.pixels)?;
    Ok(image)
}

fn canonical_image_pixels(image: &AssertedImagePixels) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"opendoc.image.pixels.v0\n");
    append_length_prefixed(&mut out, b"source", image.source.to_string().as_bytes());
    append_length_prefixed(
        &mut out,
        b"width",
        image.pixels.width.to_string().as_bytes(),
    );
    append_length_prefixed(
        &mut out,
        b"height",
        image.pixels.height.to_string().as_bytes(),
    );
    append_length_prefixed(&mut out, b"color", image.pixels.color_model.as_bytes());
    append_length_prefixed(&mut out, b"pixels", &image.pixels.pixels);
    out
}

fn validate_image_pixels(image: &DecodedImagePixels) -> Result<(), SignError> {
    if image.width == 0 || image.height == 0 {
        return Err(SignError::Backend(
            "image dimensions must be non-zero".to_string(),
        ));
    }
    if image.color_model != "rgba8-srgb" {
        return Err(SignError::Backend(format!(
            "unsupported image color model {}",
            image.color_model
        )));
    }
    let expected = image
        .width
        .checked_mul(image.height)
        .and_then(|count| count.checked_mul(4))
        .ok_or_else(|| SignError::Backend("image dimensions overflow".to_string()))?
        as usize;
    if image.pixels.len() != expected {
        return Err(SignError::Backend(format!(
            "image pixel length expected {expected}, got {}",
            image.pixels.len()
        )));
    }
    Ok(())
}

fn read_u32_field(bytes: &[u8], offset: &mut usize, label: &[u8]) -> Result<u32, SignError> {
    let value = read_bytes_field(bytes, offset, label)?;
    let value = std::str::from_utf8(&value)
        .map_err(|err| SignError::Backend(format!("field is not UTF-8: {err}")))?;
    value
        .parse::<u32>()
        .map_err(|err| SignError::Backend(format!("field is not u32: {err}")))
}

fn read_bytes_field(bytes: &[u8], offset: &mut usize, label: &[u8]) -> Result<Vec<u8>, SignError> {
    if *offset >= bytes.len() {
        return Err(SignError::Backend("missing profile field".to_string()));
    }
    if !bytes[*offset..].starts_with(label) {
        return Err(SignError::Backend(format!(
            "expected profile field {}",
            String::from_utf8_lossy(label)
        )));
    }
    *offset += label.len();
    if bytes.get(*offset) != Some(&b':') {
        return Err(SignError::Backend(
            "profile field missing length separator".to_string(),
        ));
    }
    *offset += 1;
    let length_start = *offset;
    while *offset < bytes.len() && bytes[*offset].is_ascii_digit() {
        *offset += 1;
    }
    if length_start == *offset || bytes.get(*offset) != Some(&b':') {
        return Err(SignError::Backend(
            "profile field has invalid length".to_string(),
        ));
    }
    let length = std::str::from_utf8(&bytes[length_start..*offset])
        .map_err(|err| SignError::Backend(format!("profile length is not UTF-8: {err}")))?
        .parse::<usize>()
        .map_err(|err| SignError::Backend(format!("profile length is invalid: {err}")))?;
    *offset += 1;
    let end = (*offset)
        .checked_add(length)
        .filter(|end| *end <= bytes.len())
        .ok_or_else(|| SignError::Backend("profile field length exceeds frame".to_string()))?;
    let value = bytes[*offset..end].to_vec();
    *offset = end;
    if bytes.get(*offset) != Some(&b'\n') {
        return Err(SignError::Backend(
            "profile field missing terminator".to_string(),
        ));
    }
    *offset += 1;
    Ok(value)
}

#[derive(Clone, Debug, Default)]
pub struct FastqFullProfile;

impl TypedSignatureProfile for FastqFullProfile {
    fn profile_id(&self) -> &'static str {
        "opendoc.fastq.full.v0"
    }

    fn semantic_digest(&self, bytes: &[u8]) -> Result<HashRef, SignError> {
        let records = parse_fastq_records(bytes)?;
        digest_bytes(
            "sha256",
            &canonical_fastq_records(&records, FastqCanonicalMode::Full),
        )
        .map_err(|err| SignError::Backend(err.to_string()))
    }

    fn included_fields(&self) -> Vec<String> {
        vec![
            "read_id".to_string(),
            "sequence".to_string(),
            "phred_quality".to_string(),
        ]
    }

    fn excluded_fields(&self) -> Vec<String> {
        vec!["comments".to_string()]
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct FastqRecord {
    read_id: String,
    sequence: String,
    quality: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FastqCanonicalMode {
    SequenceOnly,
    Full,
}

fn parse_fastq_records(bytes: &[u8]) -> Result<Vec<FastqRecord>, SignError> {
    let text = std::str::from_utf8(bytes).map_err(|err| SignError::Backend(err.to_string()))?;
    let lines = text
        .lines()
        .map(|line| line.trim_end_matches('\r'))
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    if lines.is_empty() || lines.len() % 4 != 0 {
        return Err(SignError::Backend(
            "FASTQ profile requires complete four-line records".to_string(),
        ));
    }
    let mut records = Vec::new();
    for chunk in lines.as_chunks::<4>().0 {
        let header = chunk[0];
        let sequence = chunk[1].trim();
        let separator = chunk[2].trim();
        let quality = chunk[3].trim();
        let Some(read_id) = header.strip_prefix('@') else {
            return Err(SignError::Backend(
                "FASTQ record header must start with @".to_string(),
            ));
        };
        if !separator.starts_with('+') {
            return Err(SignError::Backend(
                "FASTQ record separator must start with +".to_string(),
            ));
        }
        if read_id.trim().is_empty() || sequence.is_empty() {
            return Err(SignError::Backend(
                "FASTQ record read ID and sequence must be non-empty".to_string(),
            ));
        }
        if sequence.len() != quality.len() {
            return Err(SignError::Backend(
                "FASTQ sequence and quality lengths differ".to_string(),
            ));
        }
        records.push(FastqRecord {
            read_id: read_id.trim().to_string(),
            sequence: sequence.to_ascii_uppercase(),
            quality: quality.to_string(),
        });
    }
    Ok(records)
}

fn canonical_fastq_records(records: &[FastqRecord], mode: FastqCanonicalMode) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(match mode {
        FastqCanonicalMode::SequenceOnly => b"opendoc.fastq.sequence.v0\n".as_slice(),
        FastqCanonicalMode::Full => b"opendoc.fastq.full.v0\n".as_slice(),
    });
    for record in records {
        append_length_prefixed(&mut out, b"id", record.read_id.as_bytes());
        append_length_prefixed(&mut out, b"seq", record.sequence.as_bytes());
        if mode == FastqCanonicalMode::Full {
            append_length_prefixed(&mut out, b"qual", record.quality.as_bytes());
        }
    }
    out
}

fn append_length_prefixed(out: &mut Vec<u8>, label: &[u8], value: &[u8]) {
    out.extend_from_slice(label);
    out.push(b':');
    out.extend_from_slice(value.len().to_string().as_bytes());
    out.push(b':');
    out.extend_from_slice(value);
    out.push(b'\n');
}

pub fn sign_typed_content(
    backend: &impl SignerBackend,
    profile: &impl TypedSignatureProfile,
    source_blob: HashRef,
    title: impl Into<String>,
    signer: Signer,
    bytes: &[u8],
) -> Result<TypedContentSignature, SignError> {
    let semantic_digest = profile.semantic_digest(bytes)?;
    let signature = sign_target(
        backend,
        semantic_digest.clone(),
        title,
        signer,
        semantic_digest.to_string().as_bytes(),
    )?;
    Ok(TypedContentSignature {
        source_blob,
        profile: profile.profile_id().to_string(),
        semantic_digest,
        included_fields: profile.included_fields(),
        excluded_fields: profile.excluded_fields(),
        signature,
    })
}

fn research_digest(namespace: &str, payload: &[u8], secret: &str) -> String {
    let mut hasher = DefaultHasher::new();
    namespace.hash(&mut hasher);
    payload.hash(&mut hasher);
    secret.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

#[cfg(not(target_arch = "wasm32"))]
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(target_arch = "wasm32")]
fn now_ms() -> u64 {
    (js_sys::Date::now() * 1.0) as u64
}

#[derive(Debug)]
pub enum SignError {
    Backend(String),
}

impl fmt::Display for SignError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for SignError {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    struct CountingSigner {
        calls: Cell<usize>,
    }

    impl CountingSigner {
        fn new() -> Self {
            Self {
                calls: Cell::new(0),
            }
        }
    }

    impl SignerBackend for CountingSigner {
        fn sign(&self, _namespace: &str, _payload: &[u8]) -> Result<Vec<u8>, SignError> {
            self.calls.set(self.calls.get() + 1);
            Ok(b"signature".to_vec())
        }

        fn verify(
            &self,
            _namespace: &str,
            _payload: &[u8],
            _signature: &[u8],
        ) -> Result<bool, SignError> {
            Ok(true)
        }
    }

    #[test]
    fn openssh_signer_detects_tampering() {
        let backend = OpenSshSigner::from_private_key_pem(TEST_ED25519_PRIVATE_KEY).unwrap();
        let public_key = backend.public_key_openssh().unwrap();
        let target = HashRef::parse("sha256:abc").unwrap();
        let signer = Signer {
            key_identity: public_key,
            display_name: "Alice".to_string(),
        };
        let record = sign_target(&backend, target, "Doc", signer, b"payload").unwrap();
        assert_eq!(
            verify_record(&backend, &record, b"payload").unwrap(),
            SignatureState::Signed
        );
        assert_eq!(
            verify_record(&backend, &record, b"changed").unwrap(),
            SignatureState::Broken
        );
        assert!(std::str::from_utf8(&record.signature)
            .unwrap()
            .starts_with("-----BEGIN SSH SIGNATURE-----"));
        assert_eq!(
            verify_record_with_public_key(&record, b"payload").unwrap(),
            SignatureState::Signed
        );
    }

    #[test]
    fn signature_detects_tampering() {
        let backend = ResearchSigner::new("secret");
        let target = HashRef::parse("sha256:abc").unwrap();
        let signer = Signer {
            key_identity: "ssh-ed25519 AAA".to_string(),
            display_name: "Alice".to_string(),
        };
        let record = sign_target(&backend, target, "Doc", signer, b"payload").unwrap();
        assert_eq!(
            verify_record(&backend, &record, b"payload").unwrap(),
            SignatureState::Signed
        );
        assert_eq!(
            verify_record(&backend, &record, b"changed").unwrap(),
            SignatureState::Broken
        );
    }

    #[test]
    fn target_aware_verification_rejects_reused_sidecar_for_wrong_hash() {
        let backend = ResearchSigner::new("secret");
        let target = HashRef::parse("sha256:abc").unwrap();
        let other_target = HashRef::parse("sha256:def").unwrap();
        let signer = Signer {
            key_identity: "ssh-ed25519 AAA".to_string(),
            display_name: "Alice".to_string(),
        };
        let record = sign_blob_hash(&backend, target.clone(), "blob", signer).unwrap();

        assert_eq!(
            verify_record_for_target(&backend, &record, &target, target.to_string().as_bytes())
                .unwrap(),
            SignatureState::Signed
        );
        assert_eq!(
            verify_record(&backend, &record, target.to_string().as_bytes()).unwrap(),
            SignatureState::Signed
        );
        assert_eq!(
            verify_record_for_target(
                &backend,
                &record,
                &other_target,
                target.to_string().as_bytes()
            )
            .unwrap(),
            SignatureState::Broken
        );
    }

    #[test]
    fn openssh_target_aware_verification_rejects_wrong_manifest_target() {
        let backend = OpenSshSigner::from_private_key_pem(TEST_ED25519_PRIVATE_KEY).unwrap();
        let public_key = backend.public_key_openssh().unwrap();
        let payload = b"canonical manifest signing payload";
        let target = digest_bytes("sha256", payload).unwrap();
        let other_target = HashRef::parse("sha256:wrong").unwrap();
        let signer = Signer {
            key_identity: public_key,
            display_name: "Alice".to_string(),
        };
        let record = sign_target(&backend, target.clone(), "manifest", signer, payload).unwrap();

        assert_eq!(
            verify_record_for_target_with_public_key(&record, &target, payload).unwrap(),
            SignatureState::Signed
        );
        assert_eq!(
            verify_record_for_target_with_public_key(&record, &other_target, payload).unwrap(),
            SignatureState::Broken
        );
    }

    #[test]
    fn sign_target_canonicalizes_signature_metadata() {
        let backend = ResearchSigner::new("secret");
        let target = HashRef::parse("sha256:abc").unwrap();
        let record = sign_target(
            &backend,
            target,
            " Signed Doc ",
            Signer {
                key_identity: " signer-key ".to_string(),
                display_name: "\tAlice\n".to_string(),
            },
            b"payload",
        )
        .unwrap();

        assert_eq!(record.signer, "signer-key");
        assert_eq!(record.signer_display, "Alice");
        assert_eq!(record.title, "Signed Doc");
        record.validate().unwrap();
    }

    #[test]
    fn sign_target_rejects_invalid_metadata_before_backend_signing() {
        let backend = CountingSigner::new();
        let target = HashRef::parse("sha256:abc").unwrap();
        let result = sign_target(
            &backend,
            target,
            "Signed Doc",
            Signer {
                key_identity: "  ".to_string(),
                display_name: "Alice".to_string(),
            },
            b"payload",
        );

        assert!(matches!(
            result,
            Err(SignError::Backend(message)) if message.contains("signature signer is empty")
        ));
        assert_eq!(backend.calls.get(), 0);
    }

    #[test]
    fn openssh_payload_envelope_is_available() {
        let payload = openssh_signed_payload(OPENSSH_NAMESPACE, b"payload").unwrap();
        assert!(payload.starts_with(b"SSHSIG"));
    }

    #[test]
    fn typed_signature_survives_storage_irrelevant_changes() {
        let backend = ResearchSigner::new("secret");
        let profile = ResearchUtf8NoWhitespaceProfile;
        let source = HashRef::parse("sha256:source").unwrap();
        let signer = Signer {
            key_identity: "ssh-ed25519 AAA".to_string(),
            display_name: "Alice".to_string(),
        };
        let one =
            sign_typed_content(&backend, &profile, source, "Doc", signer, b"A C G T").unwrap();
        let digest_two = profile.semantic_digest(b"ACGT").unwrap();
        assert_eq!(one.semantic_digest, digest_two);
        assert_eq!(
            verify_record(&backend, &one.signature, digest_two.to_string().as_bytes()).unwrap(),
            SignatureState::Signed
        );
    }

    #[test]
    fn fastq_sequence_signature_excludes_phred_scores() {
        let backend = ResearchSigner::new("secret");
        let profile = FastqSequenceProfile;
        let source = HashRef::parse("sha256:source-fastq").unwrap();
        let signer = Signer {
            key_identity: "ssh-ed25519 AAA".to_string(),
            display_name: "Alice".to_string(),
        };
        let original = b"@read-1\nacgt\n+\n!!!!\n@read-2\nTTAA\n+\n####\n";
        let dropped_or_changed_quality =
            b"@read-1\r\nACGT\r\n+\r\nIIII\r\n@read-2\r\nTTAA\r\n+\r\nJJJJ\r\n";

        let signed =
            sign_typed_content(&backend, &profile, source, "FASTQ", signer, original).unwrap();
        let recomputed = profile.semantic_digest(dropped_or_changed_quality).unwrap();

        assert_eq!(signed.profile, "opendoc.fastq.sequence.v0");
        assert_eq!(
            signed.included_fields,
            vec!["read_id".to_string(), "sequence".to_string()]
        );
        assert_eq!(
            signed.excluded_fields,
            vec!["phred_quality".to_string(), "comments".to_string()]
        );
        assert_eq!(signed.semantic_digest, recomputed);
        assert_eq!(
            verify_record(
                &backend,
                &signed.signature,
                recomputed.to_string().as_bytes()
            )
            .unwrap(),
            SignatureState::Signed
        );
    }

    #[test]
    fn fastq_full_signature_includes_phred_scores() {
        let sequence_profile = FastqSequenceProfile;
        let full_profile = FastqFullProfile;
        let original = b"@read-1\nACGT\n+\n!!!!\n";
        let changed_quality = b"@read-1\nACGT\n+\nIIII\n";

        assert_eq!(
            sequence_profile.semantic_digest(original).unwrap(),
            sequence_profile.semantic_digest(changed_quality).unwrap()
        );
        assert_ne!(
            full_profile.semantic_digest(original).unwrap(),
            full_profile.semantic_digest(changed_quality).unwrap()
        );
        assert_eq!(
            full_profile.included_fields(),
            vec![
                "read_id".to_string(),
                "sequence".to_string(),
                "phred_quality".to_string()
            ]
        );
    }

    #[test]
    fn fastq_profiles_reject_malformed_records() {
        let profile = FastqSequenceProfile;
        assert!(profile.semantic_digest(b"@read-1\nACGT\n+\n!!!\n").is_err());
        assert!(profile.semantic_digest(b"read-1\nACGT\n+\n!!!!\n").is_err());
        assert!(profile
            .semantic_digest(b"@read-1\nACGT\n-\n!!!!\n")
            .is_err());
        assert!(profile.semantic_digest(b"@read-1\nACGT\n+\n").is_err());
    }

    /// The signature must be *bound* to the container it names.
    ///
    /// This replaces `image_pixel_signature_ignores_container_bytes`, which
    /// built `same_pixels_different_blob` field-for-field identical to `image`
    /// and so could not fail. It also asserted the property this profile now
    /// deliberately does not have: ignoring the container bytes is exactly what
    /// let an attestation be transplanted onto a different image.
    #[test]
    fn an_image_pixel_signature_cannot_be_lifted_onto_other_image_bytes() {
        let backend = ResearchSigner::new("secret");
        let profile = ImagePixelsProfile;
        let png = b"PNG container bytes".as_slice();
        let webp = b"WEBP container bytes".as_slice();
        let signer = Signer {
            key_identity: "ssh-ed25519 AAA".to_string(),
            display_name: "Alice".to_string(),
        };
        let pixels = vec![
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
        ];
        let decoded = DecodedImagePixels::rgba8(2, 2, pixels).unwrap();
        let from_png = AssertedImagePixels::from_container(png, decoded.clone()).unwrap();
        let from_webp = AssertedImagePixels::from_container(webp, decoded).unwrap();

        let signed = sign_typed_content(
            &backend,
            &profile,
            from_png.source.clone(),
            "Decoded image",
            signer,
            &image_pixels_frame(&from_png),
        )
        .unwrap();

        assert_eq!(signed.profile, "opendoc.image.pixels.v0");
        assert_eq!(
            signed.semantic_digest,
            profile.semantic_digest_pixels(&from_png).unwrap()
        );
        // The same pixels, asserted against a different container, are a
        // different attestation. This is the assertion the old test could not
        // make, because its two values were the same value.
        assert_ne!(
            profile.semantic_digest_pixels(&from_png).unwrap(),
            profile.semantic_digest_pixels(&from_webp).unwrap()
        );
        // And the container the frame names is recoverable, so a verifier can
        // check it against the blob it actually holds.
        assert_eq!(
            profile
                .asserted_source(&image_pixels_frame(&from_png))
                .unwrap(),
            digest_bytes("sha256", png).unwrap()
        );
        assert_ne!(
            profile
                .asserted_source(&image_pixels_frame(&from_png))
                .unwrap(),
            digest_bytes("sha256", webp).unwrap()
        );
        assert_eq!(
            signed.excluded_fields,
            vec![
                "compression".to_string(),
                "container_metadata".to_string(),
                "storage_path".to_string(),
                "independent_decode_of_the_container".to_string(),
            ]
        );
        assert_eq!(
            verify_record(
                &backend,
                &signed.signature,
                signed.semantic_digest.to_string().as_bytes()
            )
            .unwrap(),
            SignatureState::Signed
        );
    }

    #[test]
    fn image_pixel_profile_detects_semantic_changes_and_bad_frames() {
        let profile = ImagePixelsProfile;
        let container = b"container".as_slice();
        let original = AssertedImagePixels::from_container(
            container,
            DecodedImagePixels::rgba8(1, 1, vec![1, 2, 3, 255]).unwrap(),
        )
        .unwrap();
        let changed = AssertedImagePixels::from_container(
            container,
            DecodedImagePixels::rgba8(1, 1, vec![1, 2, 4, 255]).unwrap(),
        )
        .unwrap();

        assert_ne!(
            profile.semantic_digest_pixels(&original).unwrap(),
            profile.semantic_digest_pixels(&changed).unwrap()
        );
        assert!(DecodedImagePixels::rgba8(1, 1, vec![1, 2, 3]).is_err());
        assert!(profile
            .semantic_digest(b"not an image pixels frame")
            .is_err());

        let mut frame = image_pixels_frame(&original);
        frame.extend_from_slice(b"trailing");
        assert!(profile.semantic_digest(&frame).is_err());
    }

    #[test]
    fn blob_hash_signatures_verify_without_rehashing_blob_bytes() {
        let backend = OpenSshSigner::from_private_key_pem(TEST_ED25519_PRIVATE_KEY).unwrap();
        let public_key = backend.public_key_openssh().unwrap();
        let blob = digest_bytes("sha256", b"large blob bytes").unwrap();
        let record = sign_blob_hash(
            &backend,
            blob.clone(),
            "figure.png",
            Signer {
                key_identity: public_key,
                display_name: "Alice".to_string(),
            },
        )
        .unwrap();
        assert_eq!(record.target, blob);
        assert_eq!(
            verify_record_with_public_key(&record, record.target.to_string().as_bytes()).unwrap(),
            SignatureState::Signed
        );
        assert_eq!(
            verify_record_with_public_key(&record, b"sha256:changed").unwrap(),
            SignatureState::Broken
        );
    }

    pub(super) const TEST_ED25519_PRIVATE_KEY: &str = r#"
-----BEGIN OPENSSH PRIVATE KEY-----
b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW
QyNTUxOQAAACCzPq7zfqLffKoBDe/eo04kH2XxtSmk9D7RQyf1xUqrYgAAAJgAIAxdACAM
XQAAAAtzc2gtZWQyNTUxOQAAACCzPq7zfqLffKoBDe/eo04kH2XxtSmk9D7RQyf1xUqrYg
AAAEC2BsIi0QwW2uFscKTUUXNHLsYX4FxlaSDSblbAj7WR7bM+rvN+ot98qgEN796jTiQf
ZfG1KaT0PtFDJ/XFSqtiAAAAEHVzZXJAZXhhbXBsZS5jb20BAgMEBQ==
-----END OPENSSH PRIVATE KEY-----
"#;
}

/// What a version signature proves that a snapshot signature does not.
///
/// Every other signature test in this crate signs an opaque byte string and
/// then changes that byte string — which shows the backend works, and says
/// nothing about *what* is inside the payload. These tests build a real
/// repository with a real manifest chain, truncate it the two ways a
/// repository can be truncated, and check what each signature can see.
#[cfg(test)]
mod version_signature_tests {
    use super::tests::TEST_ED25519_PRIVATE_KEY;
    use super::*;
    use opendoc_core::digest_bytes;
    use opendoc_store::{LocalObjectStore, ObjectStore, ObjectStoreLayout, Repository};
    use std::fs;
    use std::path::PathBuf;

    const DOCUMENT: &str = "version-signature-doc";
    const BRANCH: &str = "main";

    fn temp_root(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "opendoc-version-signature-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&root);
        root
    }

    fn signer(backend: &OpenSshSigner) -> Signer {
        Signer {
            key_identity: backend.public_key_openssh().unwrap(),
            display_name: "Tester".to_string(),
        }
    }

    fn put(repo: &Repository<LocalObjectStore>, bytes: &[u8]) -> HashRef {
        let hash = digest_bytes("sha256", bytes).unwrap();
        repo.store().put_if_absent(&hash, bytes).unwrap();
        hash
    }

    /// Two committed versions, each with its own snapshot and operation
    /// segment, returned newest last.
    fn build_chain(repo: &Repository<LocalObjectStore>) -> (Vec<HashRef>, Vec<HashRef>) {
        let mut manifests = Vec::new();
        let mut snapshots = Vec::new();
        let mut parent = None;
        for index in 0..2 {
            let snapshot = put(repo, format!("snapshot {index}").as_bytes());
            let segment = put(repo, format!("segment {index}").as_bytes());
            let manifest = ManifestRecord {
                document_uuid: DOCUMENT.to_string(),
                branch: BRANCH.to_string(),
                parent: parent.clone(),
                snapshot: snapshot.clone(),
                operation_segments: vec![segment],
                signatures: Vec::new(),
                blobs: Vec::new(),
                created_at_ms: 1_000 + index as u64,
            };
            let hash = repo
                .commit_manifest(&manifest, parent.as_ref())
                .unwrap()
                .unwrap();
            parent = Some(hash.clone());
            manifests.push(hash);
            snapshots.push(snapshot);
        }
        (manifests, snapshots)
    }

    /// Truncation by *rewrite*: the snapshot is untouched, the parent link and
    /// the operation segments are gone, and the branch head moves to the
    /// result. This is the shape that leaves a repository that opens normally.
    #[test]
    fn a_version_signature_sees_a_rewritten_rootless_manifest_where_a_snapshot_signature_cannot() {
        let root = temp_root("rewrite");
        let repo = Repository::new(LocalObjectStore::new(&root));
        let (manifests, snapshots) = build_chain(&repo);
        let head = manifests[1].clone();
        let manifest = repo.read_manifest(&head).unwrap().unwrap();
        let backend = OpenSshSigner::from_private_key_pem(TEST_ED25519_PRIVATE_KEY).unwrap();

        let (coverage, version_signature) =
            sign_version(&backend, &manifest, "Version", signer(&backend)).unwrap();
        assert_eq!(version_signature.target, head);
        assert_eq!(coverage.parent, Some(manifests[0].clone()));

        // The signature the application takes today: over the snapshot bytes.
        let snapshot_bytes = repo.store().get(&snapshots[1]).unwrap().unwrap();
        let snapshot_signature = sign_target(
            &backend,
            snapshots[1].clone(),
            "Snapshot",
            signer(&backend),
            &snapshot_bytes,
        )
        .unwrap();

        let rootless = ManifestRecord {
            parent: None,
            operation_segments: Vec::new(),
            ..manifest.clone()
        };
        let rootless_hash = repo.write_manifest(&rootless).unwrap();
        assert_ne!(rootless_hash, head);
        assert_eq!(
            rootless.snapshot, manifest.snapshot,
            "the truncation leaves the document itself untouched"
        );

        // The snapshot signature cannot tell the two apart: its payload is the
        // snapshot, and the snapshot did not move.
        assert_eq!(
            verify_record_for_target_with_public_key(
                &snapshot_signature,
                &snapshots[1],
                &snapshot_bytes
            )
            .unwrap(),
            SignatureState::Signed,
            "the snapshot signature verifies over a version with no history at all"
        );

        // The version signature does.
        assert_eq!(
            verify_version_signature_with_public_key(&version_signature, &manifest).unwrap(),
            SignatureState::Signed
        );
        assert_eq!(
            verify_version_signature_with_public_key(&version_signature, &rootless).unwrap(),
            SignatureState::Broken,
            "a version signature must not verify over a manifest with the history removed"
        );
        assert_eq!(
            verify_version_signature(&backend, &version_signature, &rootless).unwrap(),
            SignatureState::Broken
        );

        // And dropping one segment while keeping the parent is refused too.
        let short = ManifestRecord {
            operation_segments: Vec::new(),
            ..manifest.clone()
        };
        assert_eq!(
            verify_version_signature_with_public_key(&version_signature, &short).unwrap(),
            SignatureState::Broken
        );

        let _ = fs::remove_dir_all(root);
    }

    /// Truncation by *deletion*: the signed manifest is byte-identical, so the
    /// signature still verifies. The repository walk is what refuses.
    #[test]
    fn a_deleted_ancestor_is_refused_even_though_the_signature_still_verifies() {
        let root = temp_root("deletion");
        let repo = Repository::new(LocalObjectStore::new(&root));
        let (manifests, _) = build_chain(&repo);
        let head = manifests[1].clone();
        let manifest = repo.read_manifest(&head).unwrap().unwrap();
        let backend = OpenSshSigner::from_private_key_pem(TEST_ED25519_PRIVATE_KEY).unwrap();

        let (coverage, record) =
            sign_version(&backend, &manifest, "Version", signer(&backend)).unwrap();
        repo.write_version_signature(&coverage, &record).unwrap();

        // Before: signed, and the history it named is all here.
        let signed = repo
            .read_signed_version(&head)
            .unwrap()
            .expect("a signed version is its signature *and* the payload it was taken over");
        assert_eq!(signed.signatures, vec![record.clone()]);
        assert_eq!(
            verify_version_coverage_with_public_key(&signed.signatures[0], &signed.coverage)
                .unwrap(),
            SignatureState::Signed
        );
        assert!(!repo
            .audit_manifest_chain(&signed.coverage)
            .unwrap()
            .history_is_truncated());

        fs::remove_file(root.join(ObjectStoreLayout::object_key(&manifests[0]))).unwrap();

        // After: the signature is untouched and still verifies — that is the
        // whole point of this test, not a defect.
        let signed = repo
            .read_signed_version(&head)
            .unwrap()
            .expect("a signed version is its signature *and* the payload it was taken over");
        assert_eq!(
            verify_version_coverage_with_public_key(&signed.signatures[0], &signed.coverage)
                .unwrap(),
            SignatureState::Signed,
            "deleting an object cannot change bytes that were already signed"
        );
        assert_eq!(
            verify_version_signature_with_public_key(
                &signed.signatures[0],
                &repo.read_manifest(&head).unwrap().unwrap()
            )
            .unwrap(),
            SignatureState::Signed
        );

        // And the audit refuses it, naming the manifest that is gone.
        let audit = repo.audit_manifest_chain(&signed.coverage).unwrap();
        assert!(
            audit.history_is_truncated(),
            "a repository missing a manifest the signature named is truncated"
        );
        assert_eq!(audit.missing_manifests(), vec![manifests[0].clone()]);

        let _ = fs::remove_dir_all(root);
    }

    /// The stored coverage record is the signed payload, so editing it to hide
    /// the truncation breaks the signature instead.
    #[test]
    fn an_edited_coverage_sidecar_does_not_verify() {
        let root = temp_root("edited-coverage");
        let repo = Repository::new(LocalObjectStore::new(&root));
        let (manifests, _) = build_chain(&repo);
        let head = manifests[1].clone();
        let manifest = repo.read_manifest(&head).unwrap().unwrap();
        let backend = OpenSshSigner::from_private_key_pem(TEST_ED25519_PRIVATE_KEY).unwrap();
        let (coverage, record) =
            sign_version(&backend, &manifest, "Version", signer(&backend)).unwrap();
        repo.write_version_signature(&coverage, &record).unwrap();

        let mut forged = coverage.clone();
        forged.parent = None;
        forged.operation_segments = Vec::new();
        repo.store()
            .put_named(
                &ObjectStoreLayout::version_coverage_key(&head),
                &forged.signing_payload().unwrap(),
            )
            .unwrap();

        let stored = repo.read_version_coverage(&head).unwrap().unwrap();
        assert_eq!(stored, forged, "the sidecar really was replaced");
        assert_eq!(
            verify_version_coverage_with_public_key(&record, &stored).unwrap(),
            SignatureState::Broken,
            "a coverage record that claims less history than was signed must not verify"
        );

        let _ = fs::remove_dir_all(root);
    }

    /// The record's `target` is not decoration: the repository files a
    /// signature sidecar under it, so a record whose target names a different
    /// version would be read back as that version's signature.
    #[test]
    fn a_record_that_names_another_manifest_is_refused_even_when_its_payload_matches() {
        let root = temp_root("wrong-target");
        let repo = Repository::new(LocalObjectStore::new(&root));
        let (manifests, _) = build_chain(&repo);
        let manifest = repo.read_manifest(&manifests[1]).unwrap().unwrap();
        let backend = OpenSshSigner::from_private_key_pem(TEST_ED25519_PRIVATE_KEY).unwrap();
        let (coverage, record) =
            sign_version(&backend, &manifest, "Version", signer(&backend)).unwrap();

        let mut relabelled = record.clone();
        relabelled.target = manifests[0].clone();
        // The signature bytes still verify over the payload; only the claim
        // about which version this is has been changed.
        assert_eq!(
            verify_record_with_public_key(&relabelled, &coverage.signing_payload().unwrap())
                .unwrap(),
            SignatureState::Signed
        );
        assert_eq!(
            verify_version_signature_with_public_key(&relabelled, &manifest).unwrap(),
            SignatureState::Broken,
            "a record naming another manifest must not pass as this version's signature"
        );
        assert_eq!(
            verify_version_coverage_with_public_key(&relabelled, &coverage).unwrap(),
            SignatureState::Broken
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_version_signature_cannot_be_lifted_onto_another_version() {
        let root = temp_root("lift");
        let repo = Repository::new(LocalObjectStore::new(&root));
        let (manifests, _) = build_chain(&repo);
        let backend = OpenSshSigner::from_private_key_pem(TEST_ED25519_PRIVATE_KEY).unwrap();
        let older = repo.read_manifest(&manifests[0]).unwrap().unwrap();
        let newer = repo.read_manifest(&manifests[1]).unwrap().unwrap();
        let (_, record) = sign_version(&backend, &newer, "Version", signer(&backend)).unwrap();

        assert_eq!(
            verify_version_signature_with_public_key(&record, &older).unwrap(),
            SignatureState::Broken
        );
        let _ = fs::remove_dir_all(root);
    }

    /// The signed payload is readable on its own: an investigator holding only
    /// these bytes learns that the version claimed a parent, without the
    /// repository that has since lost it.
    #[test]
    fn the_signed_payload_says_what_it_covered() {
        let root = temp_root("self-describing");
        let repo = Repository::new(LocalObjectStore::new(&root));
        let (manifests, snapshots) = build_chain(&repo);
        let manifest = repo.read_manifest(&manifests[1]).unwrap().unwrap();
        let backend = OpenSshSigner::from_private_key_pem(TEST_ED25519_PRIVATE_KEY).unwrap();
        let (coverage, record) =
            sign_version(&backend, &manifest, "Version", signer(&backend)).unwrap();
        let payload = coverage.signing_payload().unwrap();

        // Everything below is read back from the payload bytes alone.
        let read = VersionCoverageRecord::from_signing_payload(&payload).unwrap();
        assert_eq!(
            verify_version_coverage_with_public_key(&record, &read).unwrap(),
            SignatureState::Signed
        );
        assert_eq!(read.document_uuid, DOCUMENT);
        assert_eq!(read.branch, BRANCH);
        assert_eq!(read.manifest, manifests[1]);
        assert_eq!(read.parent, Some(manifests[0].clone()));
        assert_eq!(read.snapshot, snapshots[1]);
        assert_eq!(read.operation_segments.len(), 1);
        let _ = fs::remove_dir_all(root);
    }
}
