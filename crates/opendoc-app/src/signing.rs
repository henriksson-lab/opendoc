use super::*;
use opendoc_sign::{
    sign_typed_content, AssertedImagePixels, ImagePixelsProfile, SignatureState,
    TypedSignatureProfile,
};

pub(crate) fn sign_fastq_profile(
    backend: &OpenSshSigner,
    profile: &impl TypedSignatureProfile,
    source_blob: opendoc_core::HashRef,
    title: impl Into<String>,
    signer: Signer,
    bytes: &[u8],
) -> Result<AppTypedContentSignature, AppApiError> {
    let typed = sign_typed_content(backend, profile, source_blob, title, signer, bytes)
        .map_err(|err| AppApiError::Sign(err.to_string()))?;
    Ok(AppTypedContentSignature {
        source_blob: typed.source_blob.to_string(),
        profile: typed.profile,
        semantic_digest: typed.semantic_digest.to_string(),
        included_fields: typed.included_fields,
        excluded_fields: typed.excluded_fields,
        signature_state: "signed".to_string(),
        signature: AppSignature::from_record(&typed.signature),
        signature_bytes: typed.signature.signature,
        profile_payload: None,
    })
}

/// Bind caller-supplied RGBA8 pixels to the container bytes they are claimed
/// to be the decoding of.
///
/// Lives here rather than in `signing_service` so that constructing the frame
/// and signing it are the same step: there is no point in the codebase where a
/// pixel frame exists without the container digest that gives it meaning.
pub(crate) fn asserted_image_pixels(
    container: &[u8],
    width: u32,
    height: u32,
    pixels: Vec<u8>,
) -> Result<AssertedImagePixels, AppApiError> {
    let decoded = DecodedImagePixels::rgba8(width, height, pixels)
        .map_err(|err| AppApiError::Sign(err.to_string()))?;
    AssertedImagePixels::from_container(container, decoded)
        .map_err(|err| AppApiError::Sign(err.to_string()))
}

/// Sign a frame of pixels *as an assertion about a particular blob*.
///
/// `image` carries the container's content digest, and that digest is inside
/// the signed frame — so the attestation names the bytes it is about and
/// [`verify_app_typed_signature`] can check that name against the blob it
/// actually holds. Nothing here decodes the container; the profile's
/// `excluded_fields` say so.
pub(crate) fn sign_image_pixels_profile(
    backend: &OpenSshSigner,
    source_blob: opendoc_core::HashRef,
    title: impl Into<String>,
    signer: Signer,
    image: &AssertedImagePixels,
) -> Result<AppTypedContentSignature, AppApiError> {
    let profile = ImagePixelsProfile;
    let typed = sign_typed_content(
        backend,
        &profile,
        source_blob,
        title,
        signer,
        &opendoc_sign::image_pixels_frame(image),
    )
    .map_err(|err| AppApiError::Sign(err.to_string()))?;
    Ok(AppTypedContentSignature {
        source_blob: typed.source_blob.to_string(),
        profile: typed.profile,
        semantic_digest: typed.semantic_digest.to_string(),
        included_fields: typed.included_fields,
        excluded_fields: typed.excluded_fields,
        signature_state: "signed".to_string(),
        signature: AppSignature::from_record(&typed.signature),
        signature_bytes: typed.signature.signature,
        profile_payload: Some(opendoc_sign::image_pixels_frame(image)),
    })
}

pub(crate) fn verify_app_typed_signature(
    signature: &AppTypedContentSignature,
    blob_hash: &str,
    bytes: &[u8],
) -> String {
    if signature.source_blob != blob_hash {
        return "broken".to_string();
    }
    let digest = match signature.profile.as_str() {
        "opendoc.fastq.sequence.v0" => FastqSequenceProfile.semantic_digest(bytes),
        "opendoc.fastq.full.v0" => FastqFullProfile.semantic_digest(bytes),
        "opendoc.image.pixels.v0" => {
            let Some(payload) = signature.profile_payload.as_deref() else {
                return "untrusted".to_string();
            };
            // The payload is the signer's frame, so recomputing the digest from
            // it can never disagree with the digest stored beside it — that is
            // what made this attestation vacuous. The check that means
            // something is against the blob's *actual* bytes: the frame names a
            // container digest, and this is where that name is tested.
            let Ok(asserted) = ImagePixelsProfile.asserted_source(payload) else {
                return "broken".to_string();
            };
            let Ok(actual) = opendoc_core::digest_bytes("sha256", bytes) else {
                return "broken".to_string();
            };
            if asserted != actual {
                return "broken".to_string();
            }
            ImagePixelsProfile.semantic_digest(payload)
        }
        _ => return "untrusted".to_string(),
    };
    let Ok(digest) = digest else {
        return "broken".to_string();
    };
    if digest.to_string() != signature.semantic_digest {
        return "broken".to_string();
    }
    let Ok(target) = opendoc_core::HashRef::parse(&signature.semantic_digest) else {
        return "broken".to_string();
    };
    if signature.signature.target != signature.semantic_digest {
        return "broken".to_string();
    }
    let record = opendoc_format::SignatureRecord {
        target,
        signer: signature.signature.signer.clone(),
        signer_display: signature.signature.signer_display.clone(),
        title: signature.signature.title.clone(),
        signed_at_ms: signature.signature.signed_at_ms,
        signature: signature.signature_bytes.clone(),
    };
    match verify_record_for_target_with_public_key(
        &record,
        &record.target,
        signature.semantic_digest.as_bytes(),
    ) {
        Ok(SignatureState::Signed) => "signed".to_string(),
        Ok(SignatureState::Unsigned) => "unsigned".to_string(),
        Ok(SignatureState::Trusted) => "trusted".to_string(),
        Ok(SignatureState::Untrusted) => "untrusted".to_string(),
        Ok(SignatureState::Broken) | Err(_) => "broken".to_string(),
    }
}

impl OpenDocApp {
    pub fn sign_with_openssh_private_key(
        &mut self,
        private_key_pem: impl AsRef<[u8]>,
        signer_display: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        self.invalidate_projection();
        let payload = self.snapshot_payload()?;
        let title = self.document.title.clone();
        self.signature_service()
            .sign_document(&payload, title, private_key_pem, signer_display)?;
        Ok(self.document())
    }

    pub fn verify_current_signature(
        &self,
        private_key_pem: impl AsRef<[u8]>,
    ) -> Result<String, AppApiError> {
        let payload = self.snapshot_payload()?;
        SignatureService::verify_document_with_private_key(
            &self.signatures,
            &payload,
            private_key_pem,
        )
    }

    pub fn verify_current_signatures(&self) -> Result<String, AppApiError> {
        let payload = self.snapshot_payload()?;
        SignatureService::verify_document_with_embedded_public_keys(&self.signatures, &payload)
    }

    pub fn sign_blob_with_openssh_private_key(
        &mut self,
        blob_hash: impl AsRef<str>,
        private_key_pem: impl AsRef<[u8]>,
        signer_display: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        self.signature_service()
            .sign_blob(blob_hash, private_key_pem, signer_display)?;
        Ok(self.document())
    }

    pub fn sign_fastq_blob_with_openssh_private_key(
        &mut self,
        blob_hash: impl AsRef<str>,
        profile: impl AsRef<str>,
        private_key_pem: impl AsRef<[u8]>,
        signer_display: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        self.signature_service().sign_fastq_blob(
            blob_hash,
            profile,
            private_key_pem,
            signer_display,
        )?;
        Ok(self.document())
    }

    pub fn sign_image_pixels_blob_with_openssh_private_key(
        &mut self,
        blob_hash: impl AsRef<str>,
        width: u32,
        height: u32,
        pixels: Vec<u8>,
        private_key_pem: impl AsRef<[u8]>,
        signer_display: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        self.signature_service().sign_image_pixels_blob(
            blob_hash,
            width,
            height,
            pixels,
            private_key_pem,
            signer_display,
        )?;
        Ok(self.document())
    }

    fn signature_service(&mut self) -> SignatureService<'_> {
        SignatureService::new(
            &mut self.blobs,
            &self.blob_bytes,
            &mut self.blob_signatures,
            &mut self.signatures,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // `DecodedImagePixels`, `Signer` and the profile helpers come through
    // `use super::*`; only the signer backend is not already in scope.
    use opendoc_sign::OpenSshSigner;

    const TEST_ED25519_PRIVATE_KEY: &str = r#"
-----BEGIN OPENSSH PRIVATE KEY-----
b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW
QyNTUxOQAAACCzPq7zfqLffKoBDe/eo04kH2XxtSmk9D7RQyf1xUqrYgAAAJgAIAxdACAM
XQAAAAtzc2gtZWQyNTUxOQAAACCzPq7zfqLffKoBDe/eo04kH2XxtSmk9D7RQyf1xUqrYg
AAAEC2BsIi0QwW2uFscKTUUXNHLsYX4FxlaSDSblbAj7WR7bM+rvN+ot98qgEN796jTiQf
ZfG1KaT0PtFDJ/XFSqtiAAAAEHVzZXJAZXhhbXBsZS5jb20BAgMEBQ==
-----END OPENSSH PRIVATE KEY-----
"#;

    fn hash_of(bytes: &[u8]) -> String {
        opendoc_core::digest_bytes("sha256", bytes)
            .expect("digest")
            .to_string()
    }

    /// One signed attestation that `container` decodes to a single red pixel.
    fn attestation_for(container: &[u8]) -> AppTypedContentSignature {
        let backend =
            OpenSshSigner::from_private_key_pem(TEST_ED25519_PRIVATE_KEY).expect("test key");
        let asserted = AssertedImagePixels::from_container(
            container,
            DecodedImagePixels::rgba8(1, 1, vec![255, 0, 0, 255]).expect("one pixel"),
        )
        .expect("asserted pixels");
        let signer = Signer {
            key_identity: backend.public_key_openssh().expect("public key"),
            display_name: "Ada".to_string(),
        };
        sign_image_pixels_profile(
            &backend,
            opendoc_core::digest_bytes("sha256", container).expect("digest"),
            "figure.png",
            signer,
            &asserted,
        )
        .expect("signed")
    }

    /// The bug this replaces: the attestation was computed from `width`,
    /// `height` and `pixels` as the caller supplied them, and verified by
    /// recomputing the digest from the very frame stored beside the signature.
    /// The two could never disagree, so a signature could be renamed onto any
    /// other image and still verify.
    #[test]
    fn an_image_pixel_attestation_is_bound_to_the_bytes_it_names() {
        let png = b"PNG container bytes".as_slice();
        let webp = b"WEBP container bytes".as_slice();
        let signature = attestation_for(png);

        assert_eq!(
            verify_app_typed_signature(&signature, &hash_of(png), png),
            "signed"
        );

        // Transplanted: the same signature, the same frame, renamed onto a
        // different image. `source_blob` is not covered by the signature, so
        // rewriting it costs nothing — the frame's own binding is what refuses.
        let mut transplanted = signature.clone();
        transplanted.source_blob = hash_of(webp);
        assert_eq!(
            verify_app_typed_signature(&transplanted, &hash_of(webp), webp),
            "broken"
        );

        // And the container changing under a signature that still names it.
        let edited = b"PNG container bytes, edited".as_slice();
        let mut renamed = signature.clone();
        renamed.source_blob = hash_of(edited);
        assert_eq!(
            verify_app_typed_signature(&renamed, &hash_of(edited), edited),
            "broken"
        );
    }

    /// The profile says what it attests to, and what it does not.
    #[test]
    fn the_profile_names_the_container_digest_and_disclaims_the_decode() {
        let signature = attestation_for(b"PNG container bytes");
        assert!(
            signature
                .included_fields
                .contains(&"source_blob_digest".to_string()),
            "{:?}",
            signature.included_fields
        );
        assert!(
            signature
                .excluded_fields
                .contains(&"independent_decode_of_the_container".to_string()),
            "{:?}",
            signature.excluded_fields
        );
        // Every pixel field is named as the signer's assertion, not as a fact
        // OpenDoc established.
        for field in [
            "asserted_width",
            "asserted_height",
            "asserted_normalized_pixels",
        ] {
            assert!(
                signature.included_fields.contains(&field.to_string()),
                "{:?}",
                signature.included_fields
            );
        }
    }
}
