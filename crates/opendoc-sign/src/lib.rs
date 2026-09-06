use opendoc_core::{digest_bytes, HashRef};
use opendoc_format::SignatureRecord;
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

#[derive(Clone, Debug, Default)]
pub struct ImagePixelsProfile;

impl ImagePixelsProfile {
    pub fn semantic_digest_pixels(&self, image: &DecodedImagePixels) -> Result<HashRef, SignError> {
        digest_bytes("sha256", &canonical_image_pixels(image))
            .map_err(|err| SignError::Backend(err.to_string()))
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
        vec![
            "width".to_string(),
            "height".to_string(),
            "color_model".to_string(),
            "normalized_pixels".to_string(),
        ]
    }

    fn excluded_fields(&self) -> Vec<String> {
        vec![
            "compression".to_string(),
            "container_metadata".to_string(),
            "storage_path".to_string(),
        ]
    }
}

pub fn image_pixels_frame(image: &DecodedImagePixels) -> Vec<u8> {
    canonical_image_pixels(image)
}

fn parse_image_pixels_frame(bytes: &[u8]) -> Result<DecodedImagePixels, SignError> {
    const MAGIC: &[u8] = b"opendoc.image.pixels.v0\n";
    if !bytes.starts_with(MAGIC) {
        return Err(SignError::Backend(
            "image pixel profile frame has invalid magic".to_string(),
        ));
    }
    let mut offset = MAGIC.len();
    let width = read_u32_field(bytes, &mut offset, b"width")?;
    let height = read_u32_field(bytes, &mut offset, b"height")?;
    let color_model = read_bytes_field(bytes, &mut offset, b"color")?;
    let pixels = read_bytes_field(bytes, &mut offset, b"pixels")?;
    if offset != bytes.len() {
        return Err(SignError::Backend(
            "image pixel profile frame has trailing bytes".to_string(),
        ));
    }
    let color_model = String::from_utf8(color_model)
        .map_err(|err| SignError::Backend(format!("image color model is not UTF-8: {err}")))?;
    let image = DecodedImagePixels {
        width,
        height,
        color_model,
        pixels,
    };
    validate_image_pixels(&image)?;
    Ok(image)
}

fn canonical_image_pixels(image: &DecodedImagePixels) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"opendoc.image.pixels.v0\n");
    append_length_prefixed(&mut out, b"width", image.width.to_string().as_bytes());
    append_length_prefixed(&mut out, b"height", image.height.to_string().as_bytes());
    append_length_prefixed(&mut out, b"color", image.color_model.as_bytes());
    append_length_prefixed(&mut out, b"pixels", &image.pixels);
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

    #[test]
    fn image_pixel_signature_ignores_container_bytes() {
        let backend = ResearchSigner::new("secret");
        let profile = ImagePixelsProfile;
        let source_png = HashRef::parse("sha256:source-png").unwrap();
        let source_webp = HashRef::parse("sha256:source-webp").unwrap();
        let signer = Signer {
            key_identity: "ssh-ed25519 AAA".to_string(),
            display_name: "Alice".to_string(),
        };
        let pixels = vec![
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
        ];
        let image = DecodedImagePixels::rgba8(2, 2, pixels.clone()).unwrap();
        let same_pixels_different_blob = DecodedImagePixels::rgba8(2, 2, pixels).unwrap();

        let signed = sign_typed_content(
            &backend,
            &profile,
            source_png,
            "Decoded image",
            signer,
            &image_pixels_frame(&image),
        )
        .unwrap();
        let recomputed = profile
            .semantic_digest_pixels(&same_pixels_different_blob)
            .unwrap();

        assert_eq!(signed.profile, "opendoc.image.pixels.v0");
        assert_eq!(
            signed.source_blob,
            HashRef::parse("sha256:source-png").unwrap()
        );
        assert_eq!(signed.semantic_digest, recomputed);
        assert_ne!(signed.source_blob, source_webp);
        assert_eq!(
            signed.excluded_fields,
            vec![
                "compression".to_string(),
                "container_metadata".to_string(),
                "storage_path".to_string()
            ]
        );
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
    fn image_pixel_profile_detects_semantic_changes_and_bad_frames() {
        let profile = ImagePixelsProfile;
        let original = DecodedImagePixels::rgba8(1, 1, vec![1, 2, 3, 255]).unwrap();
        let changed = DecodedImagePixels::rgba8(1, 1, vec![1, 2, 4, 255]).unwrap();

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

    const TEST_ED25519_PRIVATE_KEY: &str = r#"
-----BEGIN OPENSSH PRIVATE KEY-----
b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW
QyNTUxOQAAACCzPq7zfqLffKoBDe/eo04kH2XxtSmk9D7RQyf1xUqrYgAAAJgAIAxdACAM
XQAAAAtzc2gtZWQyNTUxOQAAACCzPq7zfqLffKoBDe/eo04kH2XxtSmk9D7RQyf1xUqrYg
AAAEC2BsIi0QwW2uFscKTUUXNHLsYX4FxlaSDSblbAj7WR7bM+rvN+ot98qgEN796jTiQf
ZfG1KaT0PtFDJ/XFSqtiAAAAEHVzZXJAZXhhbXBsZS5jb20BAgMEBQ==
-----END OPENSSH PRIVATE KEY-----
"#;
}
