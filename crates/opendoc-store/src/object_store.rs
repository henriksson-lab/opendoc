//! The backend-neutral object store contract and its key layout.

use crate::error::StoreError;
use crate::keys::{
    clean_key_segment, doi_lookup_path, tombstone_path, uuid_lookup_path, version_label_path,
};
use crate::pack::{clean_pack_name, PackStats};
use crate::repository::Repository;
use opendoc_core::{digest_bytes, HashRef};
use opendoc_format::{
    LookupAliasRecord, LookupRecord, ManifestRecord, SignatureRecord, TombstoneRecord,
};

pub trait ObjectStore {
    fn capabilities(&self) -> StoreCapabilities {
        StoreCapabilities::default()
    }

    fn put_if_absent(&self, hash: &HashRef, bytes: &[u8]) -> Result<bool, StoreError>;
    fn get(&self, hash: &HashRef) -> Result<Option<Vec<u8>>, StoreError>;
    fn exists(&self, hash: &HashRef) -> Result<bool, StoreError>;
    fn put_named(&self, path: &str, bytes: &[u8]) -> Result<(), StoreError>;
    fn get_named(&self, path: &str) -> Result<Option<Vec<u8>>, StoreError>;
    fn list_prefix(&self, prefix: &str) -> Result<Vec<String>, StoreError>;
    fn compare_and_swap_head(
        &self,
        document_uuid: &str,
        branch: &str,
        expected: Option<&HashRef>,
        new: &HashRef,
    ) -> Result<bool, StoreError>;
    fn read_head(&self, document_uuid: &str, branch: &str) -> Result<Option<HashRef>, StoreError>;
    fn compact_loose_objects_to_pack(&self, _pack_name: &str) -> Result<PackStats, StoreError> {
        Err(StoreError::Format("local pack compaction".to_string()))
    }
}

impl<T: ObjectStore + ?Sized> ObjectStore for Box<T> {
    fn capabilities(&self) -> StoreCapabilities {
        (**self).capabilities()
    }

    fn put_if_absent(&self, hash: &HashRef, bytes: &[u8]) -> Result<bool, StoreError> {
        (**self).put_if_absent(hash, bytes)
    }

    fn get(&self, hash: &HashRef) -> Result<Option<Vec<u8>>, StoreError> {
        (**self).get(hash)
    }

    fn exists(&self, hash: &HashRef) -> Result<bool, StoreError> {
        (**self).exists(hash)
    }

    fn put_named(&self, path: &str, bytes: &[u8]) -> Result<(), StoreError> {
        (**self).put_named(path, bytes)
    }

    fn get_named(&self, path: &str) -> Result<Option<Vec<u8>>, StoreError> {
        (**self).get_named(path)
    }

    fn list_prefix(&self, prefix: &str) -> Result<Vec<String>, StoreError> {
        (**self).list_prefix(prefix)
    }

    fn compare_and_swap_head(
        &self,
        document_uuid: &str,
        branch: &str,
        expected: Option<&HashRef>,
        new: &HashRef,
    ) -> Result<bool, StoreError> {
        (**self).compare_and_swap_head(document_uuid, branch, expected, new)
    }

    fn read_head(&self, document_uuid: &str, branch: &str) -> Result<Option<HashRef>, StoreError> {
        (**self).read_head(document_uuid, branch)
    }

    fn compact_loose_objects_to_pack(&self, pack_name: &str) -> Result<PackStats, StoreError> {
        (**self).compact_loose_objects_to_pack(pack_name)
    }
}

impl<T: ObjectStore + ?Sized> ObjectStore for &T {
    fn capabilities(&self) -> StoreCapabilities {
        (**self).capabilities()
    }

    fn put_if_absent(&self, hash: &HashRef, bytes: &[u8]) -> Result<bool, StoreError> {
        (**self).put_if_absent(hash, bytes)
    }

    fn get(&self, hash: &HashRef) -> Result<Option<Vec<u8>>, StoreError> {
        (**self).get(hash)
    }

    fn exists(&self, hash: &HashRef) -> Result<bool, StoreError> {
        (**self).exists(hash)
    }

    fn put_named(&self, path: &str, bytes: &[u8]) -> Result<(), StoreError> {
        (**self).put_named(path, bytes)
    }

    fn get_named(&self, path: &str) -> Result<Option<Vec<u8>>, StoreError> {
        (**self).get_named(path)
    }

    fn list_prefix(&self, prefix: &str) -> Result<Vec<String>, StoreError> {
        (**self).list_prefix(prefix)
    }

    fn compare_and_swap_head(
        &self,
        document_uuid: &str,
        branch: &str,
        expected: Option<&HashRef>,
        new: &HashRef,
    ) -> Result<bool, StoreError> {
        (**self).compare_and_swap_head(document_uuid, branch, expected, new)
    }

    fn read_head(&self, document_uuid: &str, branch: &str) -> Result<Option<HashRef>, StoreError> {
        (**self).read_head(document_uuid, branch)
    }

    fn compact_loose_objects_to_pack(&self, pack_name: &str) -> Result<PackStats, StoreError> {
        (**self).compact_loose_objects_to_pack(pack_name)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StoreCapabilities {
    pub idempotent_content_put: bool,
    pub compare_and_swap_head: bool,
    pub list_prefix: bool,
    pub atomic_named_overwrite: bool,
    pub local_pack_files: bool,
}

impl Default for StoreCapabilities {
    fn default() -> Self {
        Self {
            idempotent_content_put: true,
            compare_and_swap_head: false,
            list_prefix: false,
            atomic_named_overwrite: false,
            local_pack_files: false,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ObjectStoreLayout;

impl ObjectStoreLayout {
    pub fn object_key(hash: &HashRef) -> String {
        let digest = hash.digest();
        let prefix = &digest[..digest.len().min(2)];
        format!("objects/{}/{}/{}", hash.algorithm(), prefix, digest)
    }

    pub fn blob_signature_key(hash: &HashRef) -> String {
        format!("{}.sig", Self::object_key(hash))
    }

    pub fn head_key(document_uuid: &str, branch: &str) -> Result<String, StoreError> {
        let document_uuid = clean_key_segment(document_uuid)?;
        let branch = clean_key_segment(branch)?;
        Ok(format!("documents/{document_uuid}/heads/{branch}.head"))
    }

    pub fn candidate_head_prefix(document_uuid: &str, branch: &str) -> Result<String, StoreError> {
        let document_uuid = clean_key_segment(document_uuid)?;
        let branch = clean_key_segment(branch)?;
        Ok(format!(
            "documents/{document_uuid}/head-candidates/{branch}"
        ))
    }

    pub fn candidate_head_key(
        document_uuid: &str,
        branch: &str,
        manifest: &HashRef,
    ) -> Result<String, StoreError> {
        Ok(format!(
            "{}/{}/{}.head",
            Self::candidate_head_prefix(document_uuid, branch)?,
            clean_key_segment(manifest.algorithm())?,
            clean_key_segment(manifest.digest())?
        ))
    }

    pub fn uuid_lookup_key(document_uuid: &str) -> Result<String, StoreError> {
        uuid_lookup_path(document_uuid)
    }

    pub fn doi_lookup_key(doi: &str) -> Result<String, StoreError> {
        doi_lookup_path(doi)
    }

    pub fn tombstone_key(object: &HashRef) -> String {
        tombstone_path(object)
    }

    pub fn version_label_key(manifest: &HashRef) -> String {
        version_label_path(manifest)
    }
}

pub fn verify_object_store_contract<S: ObjectStore>(
    store: &S,
    namespace: &str,
) -> Result<(), StoreError> {
    let namespace = clean_pack_name(namespace)?;
    let object_bytes = format!("opendoc-store-conformance:{namespace}").into_bytes();
    let object_hash =
        digest_bytes("sha256", &object_bytes).map_err(|_| StoreError::UnsupportedHash)?;
    store.put_if_absent(&object_hash, &object_bytes)?;
    store.put_if_absent(&object_hash, &object_bytes)?;
    if store.get(&object_hash)? != Some(object_bytes.clone()) {
        return Err(StoreError::HashMismatch);
    }
    if !store.exists(&object_hash)? {
        return Err(StoreError::Format(
            "stored object did not exist".to_string(),
        ));
    }
    if store.capabilities().local_pack_files {
        let pack = store.compact_loose_objects_to_pack(&format!("{namespace}-conformance-pack"))?;
        if pack.objects == 0 {
            return Err(StoreError::Format(
                "pack compaction did not include any objects".to_string(),
            ));
        }
        if store.get(&object_hash)? != Some(object_bytes.clone()) {
            return Err(StoreError::Format(
                "packed conformance object did not remain addressable".to_string(),
            ));
        }
    }

    let named_prefix = format!("conformance/{namespace}");
    let named_path = format!("{named_prefix}/record.bin");
    store.put_named(&named_path, b"named record")?;
    if store.get_named(&named_path)? != Some(b"named record".to_vec()) {
        return Err(StoreError::Format(
            "named record did not round-trip".to_string(),
        ));
    }
    if !store
        .list_prefix(&named_prefix)?
        .iter()
        .any(|path| path == "record.bin")
    {
        return Err(StoreError::Format(
            "list_prefix did not expose named record".to_string(),
        ));
    }

    let document_uuid = format!("doc-conformance-{namespace}");
    let branch = "main";
    let current = store.read_head(&document_uuid, branch)?;
    if !store.compare_and_swap_head(&document_uuid, branch, current.as_ref(), &object_hash)? {
        return Err(StoreError::Format(
            "compare_and_swap_head rejected matching expected value".to_string(),
        ));
    }
    if store.read_head(&document_uuid, branch)? != Some(object_hash.clone()) {
        return Err(StoreError::CorruptHead);
    }
    let other = HashRef::parse("sha256:000000").map_err(|_| StoreError::UnsupportedHash)?;
    if store.compare_and_swap_head(&document_uuid, branch, None, &other)? {
        return Err(StoreError::Format(
            "compare_and_swap_head accepted stale expected value".to_string(),
        ));
    }

    let repo = Repository::new(store);
    let lookup = LookupRecord {
        document_uuid: format!("doc-lookup-conformance-{namespace}"),
        branch: branch.to_string(),
        manifest: object_hash.clone(),
        aliases: vec![
            LookupAliasRecord {
                scheme: "doi".to_string(),
                value: format!("10.1234/opendoc-{namespace}"),
            },
            LookupAliasRecord {
                scheme: "DOI".to_string(),
                value: format!("10.5678/opendoc-{namespace}"),
            },
        ],
        created_at_ms: 1,
    };
    let lookup_paths = repo.write_lookup_record(&lookup)?;
    if lookup_paths.len() != 3 {
        return Err(StoreError::Format(format!(
            "expected three lookup index paths, got {}",
            lookup_paths.len()
        )));
    }
    if repo.read_uuid_lookup(&lookup.document_uuid)? != Some(lookup.clone()) {
        return Err(StoreError::Format(
            "uuid lookup record did not round-trip".to_string(),
        ));
    }
    if repo.read_doi_lookup(&lookup.aliases[0].value)? != Some(lookup.clone()) {
        return Err(StoreError::Format(
            "primary doi lookup record did not round-trip".to_string(),
        ));
    }
    if repo.read_doi_lookup(&format!(" {} ", lookup.aliases[1].value))? != Some(lookup.clone()) {
        return Err(StoreError::Format(
            "secondary doi lookup record did not round-trip".to_string(),
        ));
    }
    if repo
        .scan_lookup_records()?
        .iter()
        .filter(|record| *record == &lookup)
        .count()
        != 1
    {
        return Err(StoreError::Format(
            "lookup scan did not expose exactly one conformance lookup".to_string(),
        ));
    }
    if !matches!(repo.read_doi_lookup(" "), Err(StoreError::InvalidPath)) {
        return Err(StoreError::Format(
            "empty doi lookup did not reject invalid path".to_string(),
        ));
    }

    let tombstone = TombstoneRecord {
        object: object_hash.clone(),
        archive_locator: format!("tape://conformance/{namespace}/object"),
        restore_hint: "request conformance recall".to_string(),
        created_at_ms: 2,
        signer: "conformance-indexer".to_string(),
        signature: vec![1, 2, 3],
    };
    repo.write_tombstone(&tombstone)?;
    if repo.read_tombstone(&object_hash)? != Some(tombstone.clone()) {
        return Err(StoreError::Format(
            "tombstone record did not round-trip".to_string(),
        ));
    }
    if repo
        .scan_tombstone_records()?
        .iter()
        .filter(|record| *record == &tombstone)
        .count()
        != 1
    {
        return Err(StoreError::Format(
            "tombstone scan did not expose exactly one conformance tombstone".to_string(),
        ));
    }

    let signature = SignatureRecord {
        target: object_hash.clone(),
        signer: "ssh-ed25519 AAAAconformance".to_string(),
        signer_display: "Conformance Signer".to_string(),
        title: "conformance blob".to_string(),
        signed_at_ms: 3,
        signature: vec![4, 5, 6],
    };
    repo.write_blob_signature(&object_hash, &signature)?;
    if repo.read_blob_signature(&object_hash)? != Some(signature) {
        return Err(StoreError::Format(
            "blob signature sidecar did not round-trip".to_string(),
        ));
    }

    let missing_blob = HashRef::parse("sha256:ffff00").map_err(|_| StoreError::UnsupportedHash)?;
    let missing_tombstone = TombstoneRecord {
        object: missing_blob.clone(),
        archive_locator: format!("tape://conformance/{namespace}/missing-object"),
        restore_hint: "request conformance missing-blob recall".to_string(),
        created_at_ms: 4,
        signer: "conformance-indexer".to_string(),
        signature: vec![7, 8, 9],
    };
    repo.write_tombstone(&missing_tombstone)?;
    let manifest = ManifestRecord {
        document_uuid,
        branch: branch.to_string(),
        parent: None,
        snapshot: object_hash.clone(),
        operation_segments: Vec::new(),
        signatures: Vec::new(),
        blobs: vec![object_hash.clone(), missing_blob.clone()],
        created_at_ms: 5,
    };
    let audit = repo.audit_manifest_dependencies(&manifest)?;
    if !audit.snapshot.present {
        return Err(StoreError::Format(
            "manifest dependency audit reported missing snapshot".to_string(),
        ));
    }
    if !audit.blobs.iter().any(|blob| {
        blob.hash == object_hash && blob.bytes_present && blob.signature_sidecar_present
    }) {
        return Err(StoreError::Format(
            "manifest dependency audit missed present signed blob".to_string(),
        ));
    }
    if audit.missing_hashes() != vec![missing_blob.clone()] {
        return Err(StoreError::Format(
            "manifest dependency audit did not report missing blob".to_string(),
        ));
    }
    if audit.recoverable_missing_blobs() != vec![missing_blob] {
        return Err(StoreError::Format(
            "manifest dependency audit did not report recoverable missing blob".to_string(),
        ));
    }
    Ok(())
}
