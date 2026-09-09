use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{AppApiError, AppArchiveTombstone, AppSignature};
use opendoc_sign::{ImagePixelsProfile, TypedSignatureProfile};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppBlobRef {
    pub id: String,
    pub name: String,
    pub media_type: String,
    pub hash: String,
    pub size: u64,
    pub available: bool,
    pub signature_state: String,
    pub signatures: Vec<AppSignature>,
    #[serde(default)]
    pub typed_signatures: Vec<AppTypedContentSignature>,
    #[serde(default)]
    pub archive_tombstone: Option<AppArchiveTombstone>,
}

impl AppBlobRef {
    pub(crate) fn validate_source(&self) -> Result<(), AppApiError> {
        if self.id.trim().is_empty() {
            return Err(AppApiError::Format("blob id is empty".to_string()));
        }
        if self.id.trim() != self.id {
            return Err(AppApiError::Format(
                "blob id has surrounding whitespace".to_string(),
            ));
        }
        if self.name.trim().is_empty() {
            return Err(AppApiError::Format("blob name is empty".to_string()));
        }
        if self.name.trim() != self.name {
            return Err(AppApiError::Format(
                "blob name has surrounding whitespace".to_string(),
            ));
        }
        if self.media_type.trim().is_empty() {
            return Err(AppApiError::Format("blob media type is empty".to_string()));
        }
        if self.media_type.trim() != self.media_type {
            return Err(AppApiError::Format(
                "blob media type has surrounding whitespace".to_string(),
            ));
        }
        opendoc_core::HashRef::parse(&self.hash)
            .map_err(|err| AppApiError::Format(err.to_string()))?;
        validate_signature_state(&self.signature_state)?;
        if self.signatures.is_empty()
            && matches!(self.signature_state.as_str(), "signed" | "trusted")
        {
            return Err(AppApiError::Format(format!(
                "blob signature_state {} requires blob signatures",
                self.signature_state
            )));
        }
        if !self.signatures.is_empty() && self.signature_state == "unsigned" {
            return Err(AppApiError::Format(
                "blob signature_state unsigned cannot have blob signatures".to_string(),
            ));
        }
        let mut exact_signature_keys = BTreeSet::new();
        for signature in &self.signatures {
            signature.validate_source()?;
            if signature.target != self.hash {
                return Err(AppApiError::Format(format!(
                    "blob signature target {} does not match blob {}",
                    signature.target, self.hash
                )));
            }
            let key = (signature.target.clone(), signature.signer.clone());
            if !exact_signature_keys.insert(key) {
                return Err(AppApiError::Format(format!(
                    "duplicate blob signature for {} by {}",
                    signature.target, signature.signer
                )));
            }
        }
        let mut typed_signature_keys = BTreeSet::new();
        for typed in &self.typed_signatures {
            typed.validate_source(&self.hash)?;
            let key = (
                typed.source_blob.clone(),
                typed.profile.clone(),
                typed.semantic_digest.clone(),
                typed.signature.signer.clone(),
            );
            if !typed_signature_keys.insert(key) {
                return Err(AppApiError::Format(format!(
                    "duplicate typed signature for {} {} {} by {}",
                    typed.source_blob, typed.profile, typed.semantic_digest, typed.signature.signer
                )));
            }
        }
        if let Some(tombstone) = &self.archive_tombstone {
            tombstone.validate_source()?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppTypedContentSignature {
    pub source_blob: String,
    pub profile: String,
    pub semantic_digest: String,
    pub included_fields: Vec<String>,
    pub excluded_fields: Vec<String>,
    pub signature_state: String,
    pub signature: AppSignature,
    pub signature_bytes: Vec<u8>,
    #[serde(default)]
    pub profile_payload: Option<Vec<u8>>,
}

impl AppTypedContentSignature {
    pub(crate) fn validate_source(&self, blob_hash: &str) -> Result<(), AppApiError> {
        if self.source_blob != blob_hash {
            return Err(AppApiError::Format(format!(
                "typed signature source blob {} does not match blob {}",
                self.source_blob, blob_hash
            )));
        }
        if self.profile.trim().is_empty() {
            return Err(AppApiError::Format(
                "typed signature profile is empty".to_string(),
            ));
        }
        if self.profile.trim() != self.profile {
            return Err(AppApiError::Format(
                "typed signature profile has surrounding whitespace".to_string(),
            ));
        }
        if self.semantic_digest.trim() != self.semantic_digest {
            return Err(AppApiError::Format(
                "typed signature semantic digest has surrounding whitespace".to_string(),
            ));
        }
        opendoc_core::HashRef::parse(&self.semantic_digest)
            .map_err(|err| AppApiError::Format(err.to_string()))?;
        validate_signature_state(&self.signature_state)?;
        if self.signature_state == "unsigned" {
            return Err(AppApiError::Format(
                "typed signature_state unsigned cannot have signature bytes".to_string(),
            ));
        }
        self.signature.validate_source()?;
        if self.signature.target != self.semantic_digest {
            return Err(AppApiError::Format(format!(
                "typed signature target {} does not match semantic digest {}",
                self.signature.target, self.semantic_digest
            )));
        }
        if self.signature_bytes.is_empty() {
            return Err(AppApiError::Format(
                "typed signature bytes are empty".to_string(),
            ));
        }
        if self.profile_payload.as_ref().is_some_and(Vec::is_empty) {
            return Err(AppApiError::Format(
                "typed signature profile payload is empty".to_string(),
            ));
        }
        match self.profile.as_str() {
            "opendoc.image.pixels.v0" => {
                let Some(payload) = self.profile_payload.as_deref() else {
                    return Err(AppApiError::Format(
                        "image pixel typed signature requires profile payload".to_string(),
                    ));
                };
                let payload_digest = ImagePixelsProfile
                    .semantic_digest(payload)
                    .map_err(|err| AppApiError::Format(err.to_string()))?;
                if payload_digest.to_string() != self.semantic_digest {
                    return Err(AppApiError::Format(format!(
                        "image pixel typed signature semantic digest {} does not match profile payload {}",
                        self.semantic_digest, payload_digest
                    )));
                }
            }
            "opendoc.fastq.sequence.v0" | "opendoc.fastq.full.v0"
                if self.profile_payload.is_some() =>
            {
                return Err(AppApiError::Format(
                    "FASTQ typed signature cannot have profile payload".to_string(),
                ));
            }
            _ => {}
        }
        validate_non_empty_string_list("typed signature included field", &self.included_fields)?;
        validate_non_empty_string_list("typed signature excluded field", &self.excluded_fields)?;
        validate_disjoint_string_lists(
            "typed signature included field",
            &self.included_fields,
            "typed signature excluded field",
            &self.excluded_fields,
        )
    }
}

pub(crate) fn validate_signature_state(value: &str) -> Result<(), AppApiError> {
    match value {
        "unsigned" | "signed" | "trusted" | "untrusted" | "broken" => Ok(()),
        other => Err(AppApiError::Format(format!(
            "unsupported signature state {other}"
        ))),
    }
}

fn validate_non_empty_string_list(label: &str, values: &[String]) -> Result<(), AppApiError> {
    let mut seen = BTreeSet::new();
    for (index, value) in values.iter().enumerate() {
        if value.trim().is_empty() {
            return Err(AppApiError::Format(format!("{label} {index} is empty")));
        }
        if value.trim() != value {
            return Err(AppApiError::Format(format!(
                "{label} {index} has surrounding whitespace"
            )));
        }
        let key = value.as_str();
        if !seen.insert(key.to_string()) {
            return Err(AppApiError::Format(format!("duplicate {label} {key}")));
        }
    }
    Ok(())
}

fn validate_disjoint_string_lists(
    left_label: &str,
    left: &[String],
    right_label: &str,
    right: &[String],
) -> Result<(), AppApiError> {
    let right_values = right
        .iter()
        .map(|value| value.to_string())
        .collect::<BTreeSet<_>>();
    for value in left {
        if right_values.contains(value) {
            return Err(AppApiError::Format(format!(
                "{left_label} {value} also appears as {right_label}"
            )));
        }
    }
    Ok(())
}
