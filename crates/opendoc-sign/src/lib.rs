use opendoc_core::{digest_bytes, HashRef};
use opendoc_format::SignatureRecord;
use ssh_key::{HashAlg, LineEnding, PrivateKey, PublicKey, SshSig};
use std::collections::hash_map::DefaultHasher;
use std::fmt;
use std::hash::{Hash, Hasher};
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
    Ok(SignatureRecord {
        target,
        signer: signer.key_identity,
        signer_display: signer.display_name,
        title: title.into(),
        signed_at_ms: now_ms(),
        signature: backend.sign(OPENSSH_NAMESPACE, payload)?,
    })
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

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
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
