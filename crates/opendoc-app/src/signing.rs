use super::*;
use opendoc_sign::{sign_typed_content, ImagePixelsProfile, SignatureState, TypedSignatureProfile};

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

pub(crate) fn sign_image_pixels_profile(
    backend: &OpenSshSigner,
    source_blob: opendoc_core::HashRef,
    title: impl Into<String>,
    signer: Signer,
    image: &DecodedImagePixels,
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
