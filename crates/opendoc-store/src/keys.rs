//! Key and path naming: lookup indexes, tombstones, version labels, prefixes.

use crate::error::StoreError;
use opendoc_core::{digest_bytes, HashRef};
use opendoc_format::LookupRecord;
use std::path::Path;

pub(crate) fn clean_object_prefix(value: &str) -> Result<String, StoreError> {
    let value = value.trim().trim_matches('/');
    if value.is_empty() {
        return Ok(String::new());
    }
    for segment in value.split('/') {
        if segment == "." || segment == ".." {
            return Err(StoreError::InvalidPath);
        }
        clean_key_segment(segment)?;
    }
    Ok(value.to_string())
}

pub(crate) fn clean_key_segment(value: &str) -> Result<&str, StoreError> {
    let valid = !value.is_empty()
        && value != "."
        && value != ".."
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.');
    if valid {
        Ok(value)
    } else {
        Err(StoreError::InvalidPath)
    }
}

pub(crate) fn uuid_lookup_path(document_uuid: &str) -> Result<String, StoreError> {
    let document_uuid = clean_key_segment(document_uuid)?;
    let prefix = &document_uuid[..document_uuid.len().min(2)];
    Ok(format!("indexes/by-uuid/{prefix}/{document_uuid}.idx"))
}

pub(crate) fn doi_lookup_path(doi: &str) -> Result<String, StoreError> {
    let doi = doi.trim();
    if doi.is_empty() {
        return Err(StoreError::InvalidPath);
    }
    let hash = digest_bytes("sha256", doi.to_ascii_lowercase().as_bytes())
        .map_err(|_| StoreError::UnsupportedHash)?;
    let digest = hash.digest();
    Ok(format!(
        "indexes/by-doi/{}/{}.idx",
        &digest[..digest.len().min(2)],
        digest
    ))
}

pub(crate) fn lookup_record_matches_index_path(
    record: &LookupRecord,
    path: &str,
) -> Result<bool, StoreError> {
    if path == uuid_lookup_path(&record.document_uuid)? {
        return Ok(true);
    }
    lookup_record_matches_doi_path(record, path)
}

pub(crate) fn lookup_record_matches_doi_path(
    record: &LookupRecord,
    path: &str,
) -> Result<bool, StoreError> {
    for alias in &record.aliases {
        if alias.scheme.trim().eq_ignore_ascii_case("doi") && doi_lookup_path(&alias.value)? == path
        {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(crate) fn candidate_manifest_from_path(path: &str) -> Result<HashRef, StoreError> {
    let Some(rest) = path.strip_prefix("documents/") else {
        return Err(StoreError::InvalidPath);
    };
    let mut parts = rest.split('/');
    let Some(document_uuid) = parts.next() else {
        return Err(StoreError::InvalidPath);
    };
    if parts.next() != Some("head-candidates") {
        return Err(StoreError::InvalidPath);
    }
    let Some(branch) = parts.next() else {
        return Err(StoreError::InvalidPath);
    };
    let Some(algorithm) = parts.next() else {
        return Err(StoreError::InvalidPath);
    };
    let Some(file_name) = parts.next() else {
        return Err(StoreError::InvalidPath);
    };
    if parts.next().is_some() {
        return Err(StoreError::InvalidPath);
    }
    clean_key_segment(document_uuid)?;
    clean_key_segment(branch)?;
    clean_key_segment(algorithm)?;
    let Some(digest) = file_name.strip_suffix(".head") else {
        return Err(StoreError::InvalidPath);
    };
    clean_key_segment(digest)?;
    HashRef::parse(&format!("{algorithm}:{digest}")).map_err(|_| StoreError::InvalidPath)
}

pub(crate) fn tombstone_path(object: &HashRef) -> String {
    let digest = object.digest();
    let prefix = &digest[..digest.len().min(2)];
    format!(
        "archive/tombstones/{}/{}/{}.tombstone",
        object.algorithm(),
        prefix,
        digest
    )
}

pub(crate) fn tombstone_object_from_path(path: &str) -> Result<HashRef, StoreError> {
    let Some(rest) = path.strip_prefix("archive/tombstones/") else {
        return Err(StoreError::InvalidPath);
    };
    let mut parts = rest.split('/');
    let Some(algorithm) = parts.next() else {
        return Err(StoreError::InvalidPath);
    };
    let Some(prefix) = parts.next() else {
        return Err(StoreError::InvalidPath);
    };
    let Some(file_name) = parts.next() else {
        return Err(StoreError::InvalidPath);
    };
    if parts.next().is_some() {
        return Err(StoreError::InvalidPath);
    }
    let Some(digest) = file_name.strip_suffix(".tombstone") else {
        return Err(StoreError::InvalidPath);
    };
    if prefix != &digest[..digest.len().min(2)] {
        return Err(StoreError::InvalidPath);
    }
    HashRef::parse(&format!("{algorithm}:{digest}")).map_err(|_| StoreError::InvalidPath)
}

pub(crate) fn blob_signature_path(object: &HashRef) -> String {
    let digest = object.digest();
    let prefix = &digest[..digest.len().min(2)];
    format!("objects/{}/{}/{}.sig", object.algorithm(), prefix, digest)
}

/// Sidecar path for a manifest's human label, keyed by the manifest hash in the
/// same way `blob_signature_path` keys a signature to its blob.
pub(crate) fn version_label_path(manifest: &HashRef) -> String {
    let digest = manifest.digest();
    let prefix = &digest[..digest.len().min(2)];
    format!(
        "objects/{}/{}/{}.label",
        manifest.algorithm(),
        prefix,
        digest
    )
}

/// Sidecar path for the coverage record a version signature is taken over,
/// keyed by the manifest hash exactly as `version_label_path` is.
///
/// The record is fully derived from the manifest, so writing it twice writes
/// the same bytes. It is stored anyway because it is the only form in which a
/// signed version's *claim about its own history* survives the loss of the
/// manifest — which is precisely the case a truncation audit has to report on.
pub(crate) fn version_coverage_path(manifest: &HashRef) -> String {
    let digest = manifest.digest();
    let prefix = &digest[..digest.len().min(2)];
    format!(
        "objects/{}/{}/{}.coverage",
        manifest.algorithm(),
        prefix,
        digest
    )
}

/// Prefix holding every version signature over one manifest.
///
/// ADR 0003: "A version may have multiple signatures." Blob signatures get one
/// sidecar per blob and a second signer overwrites the first; version
/// signatures are keyed by signer as well, so they accumulate.
pub(crate) fn version_signature_prefix(manifest: &HashRef) -> String {
    let digest = manifest.digest();
    let prefix = &digest[..digest.len().min(2)];
    format!(
        "signatures/versions/{}/{}/{}",
        manifest.algorithm(),
        prefix,
        digest
    )
}

pub(crate) fn version_signature_path(
    manifest: &HashRef,
    signer: &str,
) -> Result<String, StoreError> {
    if signer.trim().is_empty() {
        return Err(StoreError::InvalidPath);
    }
    let signer_digest = digest_bytes("sha256", signer.as_bytes())
        .map_err(|_| StoreError::UnsupportedHash)?
        .digest()
        .to_string();
    Ok(format!(
        "{}/{}.vsig",
        version_signature_prefix(manifest),
        signer_digest
    ))
}

pub(crate) fn clean_relative_path(path: &str) -> Result<&Path, StoreError> {
    let path = Path::new(path);
    if path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(StoreError::InvalidPath);
    }
    Ok(path)
}

#[cfg(feature = "opendal")]
pub(crate) fn join_object_key(namespace: &str, key: &str) -> Result<String, StoreError> {
    let key = clean_relative_path(key)?
        .to_string_lossy()
        .replace('\\', "/");
    if namespace.is_empty() {
        Ok(key)
    } else {
        Ok(format!("{namespace}/{key}"))
    }
}

#[cfg(feature = "opendal")]
pub(crate) fn opendal_prefix_key(prefix: &str) -> Result<String, StoreError> {
    let mut prefix = clean_relative_path(prefix)?
        .to_string_lossy()
        .replace('\\', "/");
    if !prefix.is_empty() && !prefix.ends_with('/') {
        prefix.push('/');
    }
    Ok(prefix)
}

#[cfg(feature = "opendal")]
pub(crate) fn is_opendal_not_found(err: &opendal::Error) -> bool {
    err.kind() == opendal::ErrorKind::NotFound
}
