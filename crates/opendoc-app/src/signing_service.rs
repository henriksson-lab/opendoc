use crate::signing::asserted_image_pixels;
use crate::{sign_fastq_profile, sign_image_pixels_profile, AppApiError, AppBlobRef};
use opendoc_core::digest_bytes;
use opendoc_sign::{
    sign_blob_hash, sign_target, FastqFullProfile, FastqSequenceProfile, OpenSshSigner,
    SignatureState, Signer,
};
use std::collections::BTreeMap;

pub(crate) struct SignatureService<'a> {
    blobs: &'a mut Vec<AppBlobRef>,
    blob_bytes: &'a BTreeMap<String, Vec<u8>>,
    blob_signatures: &'a mut BTreeMap<String, Vec<opendoc_format::SignatureRecord>>,
    signatures: &'a mut Vec<opendoc_format::SignatureRecord>,
}

impl<'a> SignatureService<'a> {
    pub(crate) fn new(
        blobs: &'a mut Vec<AppBlobRef>,
        blob_bytes: &'a BTreeMap<String, Vec<u8>>,
        blob_signatures: &'a mut BTreeMap<String, Vec<opendoc_format::SignatureRecord>>,
        signatures: &'a mut Vec<opendoc_format::SignatureRecord>,
    ) -> Self {
        Self {
            blobs,
            blob_bytes,
            blob_signatures,
            signatures,
        }
    }

    pub(crate) fn sign_document(
        &mut self,
        payload: &[u8],
        title: String,
        private_key_pem: impl AsRef<[u8]>,
        signer_display: impl Into<String>,
    ) -> Result<(), AppApiError> {
        let backend = openssh_signer(private_key_pem)?;
        let signer = signer(&backend, signer_display)?;
        let target =
            digest_bytes("sha256", payload).map_err(|err| AppApiError::Model(err.to_string()))?;
        self.signatures.push(
            sign_target(&backend, target, title, signer, payload)
                .map_err(|err| AppApiError::Sign(err.to_string()))?,
        );
        Ok(())
    }

    pub(crate) fn verify_document_with_private_key(
        signatures: &[opendoc_format::SignatureRecord],
        payload: &[u8],
        private_key_pem: impl AsRef<[u8]>,
    ) -> Result<String, AppApiError> {
        if signatures.is_empty() {
            return Ok("unsigned".to_string());
        };
        let backend = openssh_signer(private_key_pem)?;
        let target =
            digest_bytes("sha256", payload).map_err(|err| AppApiError::Model(err.to_string()))?;
        let mut saw_trusted = false;
        let mut saw_untrusted = false;
        for signature in signatures {
            let embedded_state =
                opendoc_sign::verify_record_for_target_with_public_key(signature, &target, payload)
                    .map_err(|err| AppApiError::Sign(err.to_string()))?;
            if embedded_state == SignatureState::Broken {
                return Ok("broken".to_string());
            }
            if embedded_state == SignatureState::Unsigned {
                continue;
            }
            let trusted_state =
                opendoc_sign::verify_record_for_target(&backend, signature, &target, payload)
                    .map_err(|err| AppApiError::Sign(err.to_string()))?;
            if trusted_state == SignatureState::Signed {
                saw_trusted = true;
            } else {
                saw_untrusted = true;
            }
        }
        if saw_trusted {
            Ok("trusted".to_string())
        } else if saw_untrusted {
            Ok("untrusted".to_string())
        } else {
            Ok("unsigned".to_string())
        }
    }

    pub(crate) fn verify_document_with_embedded_public_keys(
        signatures: &[opendoc_format::SignatureRecord],
        payload: &[u8],
    ) -> Result<String, AppApiError> {
        if signatures.is_empty() {
            return Ok("unsigned".to_string());
        }
        let target =
            digest_bytes("sha256", payload).map_err(|err| AppApiError::Model(err.to_string()))?;
        let mut saw_signed = false;
        for signature in signatures {
            match opendoc_sign::verify_record_for_target_with_public_key(
                signature, &target, payload,
            )
            .map_err(|err| AppApiError::Sign(err.to_string()))?
            {
                SignatureState::Signed | SignatureState::Trusted => saw_signed = true,
                SignatureState::Unsigned => {}
                SignatureState::Untrusted | SignatureState::Broken => {
                    return Ok("broken".to_string());
                }
            }
        }
        if saw_signed {
            Ok("signed".to_string())
        } else {
            Ok("unsigned".to_string())
        }
    }

    pub(crate) fn sign_blob(
        &mut self,
        blob_hash: impl AsRef<str>,
        private_key_pem: impl AsRef<[u8]>,
        signer_display: impl Into<String>,
    ) -> Result<(), AppApiError> {
        let hash = opendoc_core::HashRef::parse(blob_hash.as_ref().trim())
            .map_err(|err| AppApiError::Model(err.to_string()))?;
        let Some(blob) = self.blobs.iter().find(|blob| blob.hash == hash.to_string()) else {
            return Err(AppApiError::NotFound("blob was not found".to_string()));
        };
        let backend = openssh_signer(private_key_pem)?;
        let record = sign_blob_hash(
            &backend,
            hash,
            blob.name.clone(),
            signer(&backend, signer_display)?,
        )
        .map_err(|err| AppApiError::Sign(err.to_string()))?;
        self.blob_signatures
            .entry(record.target.to_string())
            .or_default()
            .push(record);
        Ok(())
    }

    pub(crate) fn sign_fastq_blob(
        &mut self,
        blob_hash: impl AsRef<str>,
        profile: impl AsRef<str>,
        private_key_pem: impl AsRef<[u8]>,
        signer_display: impl Into<String>,
    ) -> Result<(), AppApiError> {
        let hash = opendoc_core::HashRef::parse(blob_hash.as_ref().trim())
            .map_err(|err| AppApiError::Model(err.to_string()))?;
        let blob_index = self
            .blobs
            .iter()
            .position(|blob| blob.hash == hash.to_string())
            .ok_or_else(|| AppApiError::NotFound("blob was not found".to_string()))?;
        let hash_text = hash.to_string();
        let bytes = self
            .blob_bytes
            .get(&hash_text)
            .ok_or_else(|| {
                AppApiError::NotFound(
                    "blob bytes are unavailable for typed FASTQ signing".to_string(),
                )
            })?
            .clone();
        let backend = openssh_signer(private_key_pem)?;
        let signer = signer(&backend, signer_display)?;
        let profile_label = profile.as_ref().trim();
        let typed = match profile_label {
            "sequence" | "opendoc.fastq.sequence.v0" => sign_fastq_profile(
                &backend,
                &FastqSequenceProfile,
                hash.clone(),
                self.blobs[blob_index].name.clone(),
                signer,
                &bytes,
            )?,
            "full" | "opendoc.fastq.full.v0" => sign_fastq_profile(
                &backend,
                &FastqFullProfile,
                hash.clone(),
                self.blobs[blob_index].name.clone(),
                signer,
                &bytes,
            )?,
            other => {
                return Err(AppApiError::Format(format!(
                    "unsupported FASTQ typed signature profile {other}"
                )));
            }
        };
        self.blobs[blob_index].typed_signatures.push(typed);
        Ok(())
    }

    pub(crate) fn sign_image_pixels_blob(
        &mut self,
        blob_hash: impl AsRef<str>,
        width: u32,
        height: u32,
        pixels: Vec<u8>,
        private_key_pem: impl AsRef<[u8]>,
        signer_display: impl Into<String>,
    ) -> Result<(), AppApiError> {
        let hash = opendoc_core::HashRef::parse(blob_hash.as_ref().trim())
            .map_err(|err| AppApiError::Model(err.to_string()))?;
        let blob_index = self
            .blobs
            .iter()
            .position(|blob| blob.hash == hash.to_string())
            .ok_or_else(|| AppApiError::NotFound("blob was not found".to_string()))?;
        // The bytes are required, exactly as they are for the FASTQ profiles.
        // `width`, `height` and `pixels` are the caller's word about what this
        // blob decodes to, and the signature is only worth anything if it names
        // the blob those pixels are claimed to come from — which means the blob
        // has to be here to be digested.
        let bytes = self
            .blob_bytes
            .get(&hash.to_string())
            .ok_or_else(|| {
                AppApiError::NotFound(
                    "blob bytes are unavailable for typed image signing".to_string(),
                )
            })?
            .clone();
        let image = asserted_image_pixels(&bytes, width, height, pixels)?;
        let backend = openssh_signer(private_key_pem)?;
        let typed = sign_image_pixels_profile(
            &backend,
            hash,
            self.blobs[blob_index].name.clone(),
            signer(&backend, signer_display)?,
            &image,
        )?;
        self.blobs[blob_index].typed_signatures.push(typed);
        Ok(())
    }
}

fn openssh_signer(private_key_pem: impl AsRef<[u8]>) -> Result<OpenSshSigner, AppApiError> {
    OpenSshSigner::from_private_key_pem(private_key_pem)
        .map_err(|err| AppApiError::Sign(err.to_string()))
}

fn signer(
    backend: &OpenSshSigner,
    signer_display: impl Into<String>,
) -> Result<Signer, AppApiError> {
    Ok(Signer {
        key_identity: backend
            .public_key_openssh()
            .map_err(|err| AppApiError::Sign(err.to_string()))?,
        display_name: signer_display.into(),
    })
}
