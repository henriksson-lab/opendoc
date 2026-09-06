use opendoc_core::{digest_bytes, HashRef};
use opendoc_format::{
    decode_record, encode_record, BranchHeadRecord, LookupAliasRecord, LookupRecord,
    ManifestRecord, PackIndexEntryRecord, PackIndexRecord, SignatureRecord, TombstoneRecord,
};
use std::fmt;
use std::fs;
use std::io::{ErrorKind, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
#[cfg(feature = "opendal")]
use std::sync::Arc;

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

#[derive(Clone, Debug)]
pub struct Repository<S> {
    store: S,
}

impl<S: ObjectStore> Repository<S> {
    pub fn new(store: S) -> Self {
        Self { store }
    }

    pub fn store(&self) -> &S {
        &self.store
    }

    pub fn write_manifest(&self, manifest: &ManifestRecord) -> Result<HashRef, StoreError> {
        manifest
            .validate()
            .map_err(|err| StoreError::Format(err.to_string()))?;
        let bytes = encode_record(manifest);
        let hash = digest_bytes("sha256", &bytes).map_err(|_| StoreError::UnsupportedHash)?;
        self.store.put_if_absent(&hash, &bytes)?;
        Ok(hash)
    }

    pub fn read_manifest(&self, hash: &HashRef) -> Result<Option<ManifestRecord>, StoreError> {
        let Some(bytes) = self.store.get(hash)? else {
            return Ok(None);
        };
        let actual =
            digest_bytes(hash.algorithm(), &bytes).map_err(|_| StoreError::UnsupportedHash)?;
        if &actual != hash {
            return Err(StoreError::HashMismatch);
        }
        let record: ManifestRecord =
            decode_record(&bytes).map_err(|err| StoreError::Format(err.to_string()))?;
        record
            .validate()
            .map_err(|err| StoreError::Format(err.to_string()))?;
        Ok(Some(record))
    }

    pub fn commit_manifest(
        &self,
        manifest: &ManifestRecord,
        expected: Option<&HashRef>,
    ) -> Result<Option<HashRef>, StoreError> {
        let manifest_hash = self.write_manifest(manifest)?;
        let head = BranchHeadRecord {
            document_uuid: manifest.document_uuid.clone(),
            branch: manifest.branch.clone(),
            manifest: manifest_hash.clone(),
        };
        let head_hash = digest_bytes("sha256", &encode_record(&head))
            .map_err(|_| StoreError::UnsupportedHash)?;
        self.store
            .put_if_absent(&head_hash, &encode_record(&head))?;
        if self.store.compare_and_swap_head(
            &manifest.document_uuid,
            &manifest.branch,
            expected,
            &manifest_hash,
        )? {
            Ok(Some(manifest_hash))
        } else {
            Ok(None)
        }
    }

    pub fn commit_manifest_or_candidate(
        &self,
        manifest: &ManifestRecord,
        expected: Option<&HashRef>,
    ) -> Result<CommitOutcome, StoreError> {
        let manifest_hash = self.write_manifest(manifest)?;
        let head = BranchHeadRecord {
            document_uuid: manifest.document_uuid.clone(),
            branch: manifest.branch.clone(),
            manifest: manifest_hash.clone(),
        };
        let head_hash = digest_bytes("sha256", &encode_record(&head))
            .map_err(|_| StoreError::UnsupportedHash)?;
        self.store
            .put_if_absent(&head_hash, &encode_record(&head))?;

        if self.store.capabilities().compare_and_swap_head
            && self.store.compare_and_swap_head(
                &manifest.document_uuid,
                &manifest.branch,
                expected,
                &manifest_hash,
            )?
        {
            Ok(CommitOutcome::Committed(manifest_hash))
        } else {
            let path = self.write_candidate_head(&head)?;
            Ok(CommitOutcome::Candidate {
                manifest: manifest_hash,
                path,
            })
        }
    }

    pub fn write_candidate_head(&self, head: &BranchHeadRecord) -> Result<String, StoreError> {
        head.validate()
            .map_err(|err| StoreError::Format(err.to_string()))?;
        let path = ObjectStoreLayout::candidate_head_key(
            &head.document_uuid,
            &head.branch,
            &head.manifest,
        )?;
        self.store.put_named(&path, &encode_record(head))?;
        Ok(path)
    }

    pub fn list_candidate_heads(
        &self,
        document_uuid: &str,
        branch: &str,
    ) -> Result<Vec<BranchHeadRecord>, StoreError> {
        Ok(self
            .list_candidate_head_entries(document_uuid, branch)?
            .records)
    }

    pub fn resolve_candidate_heads(
        &self,
        document_uuid: &str,
        branch: &str,
    ) -> Result<CandidateResolution, StoreError> {
        let current = self.store.read_head(document_uuid, branch)?;
        let mut candidates = Vec::new();
        let candidate_entries = self.list_candidate_head_entries(document_uuid, branch)?;
        for head in candidate_entries.records {
            let status = if current.as_ref() == Some(&head.manifest) {
                CandidateStatus::AlreadyCurrent
            } else {
                match self.read_manifest(&head.manifest)? {
                    Some(manifest) => {
                        if manifest.document_uuid != document_uuid || manifest.branch != branch {
                            return Err(StoreError::CorruptHead);
                        }
                        if self.manifest_is_ancestor_of_current(
                            document_uuid,
                            branch,
                            &head.manifest,
                            current.as_ref(),
                        )? {
                            CandidateStatus::IntegratedAncestor
                        } else if manifest.parent.as_ref() == current.as_ref() {
                            CandidateStatus::FastForward
                        } else {
                            CandidateStatus::NeedsMerge
                        }
                    }
                    None => CandidateStatus::MissingManifest,
                }
            };
            candidates.push(CandidateHeadStatus {
                manifest: head.manifest,
                status,
            });
        }
        candidates.sort_by(|left, right| {
            left.status
                .cmp(&right.status)
                .then(left.manifest.to_string().cmp(&right.manifest.to_string()))
        });
        Ok(CandidateResolution {
            current,
            candidates,
            invalid_candidates: candidate_entries.invalid,
        })
    }

    fn list_candidate_head_entries(
        &self,
        document_uuid: &str,
        branch: &str,
    ) -> Result<CandidateHeadEntries, StoreError> {
        let prefix = ObjectStoreLayout::candidate_head_prefix(document_uuid, branch)?;
        let mut records = Vec::new();
        let mut invalid = Vec::new();
        for path in self.store.list_prefix(&prefix)? {
            if !path.ends_with(".head") {
                continue;
            }
            let full_path = format!("{prefix}/{path}");
            let Some(bytes) = self.store.get_named(&full_path)? else {
                continue;
            };
            match decode_record::<BranchHeadRecord>(&bytes) {
                Ok(record) if record.document_uuid == document_uuid && record.branch == branch => {
                    match candidate_manifest_from_path(&full_path) {
                        Ok(expected) if expected == record.manifest => records.push(record),
                        Ok(expected) => invalid.push(CandidateHeadProblem {
                            path,
                            reason: format!(
                                "candidate head path targets {} but record targets {}",
                                expected, record.manifest
                            ),
                        }),
                        Err(err) => invalid.push(CandidateHeadProblem {
                            path,
                            reason: format!("{err:?}"),
                        }),
                    }
                }
                Ok(record) => invalid.push(CandidateHeadProblem {
                    path,
                    reason: format!(
                        "candidate head targets {}:{}",
                        record.document_uuid, record.branch
                    ),
                }),
                Err(err) => invalid.push(CandidateHeadProblem {
                    path,
                    reason: err.to_string(),
                }),
            }
        }
        records.sort_by_key(|left| left.manifest.to_string());
        records.dedup_by(|left, right| left.manifest == right.manifest);
        invalid.sort_by(|left, right| left.path.cmp(&right.path));
        Ok(CandidateHeadEntries { records, invalid })
    }

    pub fn reconcile_candidate_heads(
        &self,
        document_uuid: &str,
        branch: &str,
    ) -> Result<CandidateReconciliation, StoreError> {
        let initial = self.resolve_candidate_heads(document_uuid, branch)?;
        let mut advanced = Vec::new();

        loop {
            let resolution = self.resolve_candidate_heads(document_uuid, branch)?;
            let Some(candidate) = resolution
                .candidates
                .iter()
                .find(|candidate| candidate.status == CandidateStatus::FastForward)
            else {
                return Ok(CandidateReconciliation {
                    initial,
                    final_resolution: resolution,
                    advanced,
                });
            };

            match self.try_fast_forward_candidate(document_uuid, branch, &candidate.manifest)? {
                CandidateAdvance::Advanced(hash) => advanced.push(hash),
                CandidateAdvance::HeadChanged { .. } | CandidateAdvance::AlreadyCurrent(_) => {
                    continue;
                }
                CandidateAdvance::NeedsMerge(_) | CandidateAdvance::MissingManifest(_) => {
                    return Ok(CandidateReconciliation {
                        initial,
                        final_resolution: self.resolve_candidate_heads(document_uuid, branch)?,
                        advanced,
                    });
                }
            }
        }
    }

    pub fn plan_candidate_merges(
        &self,
        document_uuid: &str,
        branch: &str,
    ) -> Result<CandidateMergePlans, StoreError> {
        let resolution = self.resolve_candidate_heads(document_uuid, branch)?;
        let mut plans = Vec::new();
        for candidate in resolution
            .candidates
            .iter()
            .filter(|candidate| candidate.status == CandidateStatus::NeedsMerge)
        {
            let merge_base = self.find_merge_base(
                document_uuid,
                branch,
                resolution.current.as_ref(),
                &candidate.manifest,
            )?;
            plans.push(CandidateMergePlan {
                current: resolution.current.clone(),
                candidate: candidate.manifest.clone(),
                merge_base: merge_base.clone(),
                current_since_base: self.path_since_base(
                    document_uuid,
                    branch,
                    resolution.current.as_ref(),
                    merge_base.as_ref(),
                )?,
                candidate_since_base: self.path_since_base(
                    document_uuid,
                    branch,
                    Some(&candidate.manifest),
                    merge_base.as_ref(),
                )?,
            });
        }
        plans.sort_by_key(|left| left.candidate.to_string());
        Ok(CandidateMergePlans { resolution, plans })
    }

    pub fn try_fast_forward_candidate(
        &self,
        document_uuid: &str,
        branch: &str,
        candidate: &HashRef,
    ) -> Result<CandidateAdvance, StoreError> {
        let current = self.store.read_head(document_uuid, branch)?;
        if current.as_ref() == Some(candidate) {
            return Ok(CandidateAdvance::AlreadyCurrent(candidate.clone()));
        }

        let Some(manifest) = self.read_manifest(candidate)? else {
            return Ok(CandidateAdvance::MissingManifest(candidate.clone()));
        };
        if manifest.document_uuid != document_uuid || manifest.branch != branch {
            return Err(StoreError::CorruptHead);
        }
        if manifest.parent.as_ref() != current.as_ref() {
            return Ok(CandidateAdvance::NeedsMerge(candidate.clone()));
        }

        if self
            .store
            .compare_and_swap_head(document_uuid, branch, current.as_ref(), candidate)?
        {
            Ok(CandidateAdvance::Advanced(candidate.clone()))
        } else {
            Ok(CandidateAdvance::HeadChanged {
                previous: current,
                current: self.store.read_head(document_uuid, branch)?,
                candidate: candidate.clone(),
            })
        }
    }

    fn find_merge_base(
        &self,
        document_uuid: &str,
        branch: &str,
        current: Option<&HashRef>,
        candidate: &HashRef,
    ) -> Result<Option<HashRef>, StoreError> {
        let current_chain = self.manifest_chain_to_root(document_uuid, branch, current)?;
        let candidate_chain =
            self.manifest_chain_to_root(document_uuid, branch, Some(candidate))?;
        for candidate_ancestor in candidate_chain {
            if current_chain.iter().any(|hash| hash == &candidate_ancestor) {
                return Ok(Some(candidate_ancestor));
            }
        }
        Ok(None)
    }

    fn path_since_base(
        &self,
        document_uuid: &str,
        branch: &str,
        start: Option<&HashRef>,
        base: Option<&HashRef>,
    ) -> Result<Vec<HashRef>, StoreError> {
        let mut path = Vec::new();
        let mut cursor = start.cloned();
        while let Some(hash) = cursor {
            if base == Some(&hash) {
                break;
            }
            let manifest = self.read_manifest(&hash)?.ok_or(StoreError::CorruptHead)?;
            if manifest.document_uuid != document_uuid || manifest.branch != branch {
                return Err(StoreError::CorruptHead);
            }
            path.push(hash);
            cursor = manifest.parent;
        }
        path.reverse();
        Ok(path)
    }

    fn manifest_chain_to_root(
        &self,
        document_uuid: &str,
        branch: &str,
        start: Option<&HashRef>,
    ) -> Result<Vec<HashRef>, StoreError> {
        let mut chain = Vec::new();
        let mut cursor = start.cloned();
        for _ in 0..4096 {
            let Some(hash) = cursor else {
                return Ok(chain);
            };
            let manifest = self.read_manifest(&hash)?.ok_or(StoreError::CorruptHead)?;
            if manifest.document_uuid != document_uuid || manifest.branch != branch {
                return Err(StoreError::CorruptHead);
            }
            chain.push(hash);
            cursor = manifest.parent;
        }
        Err(StoreError::Format(
            "manifest ancestry exceeded traversal limit".to_string(),
        ))
    }

    fn manifest_is_ancestor_of_current(
        &self,
        document_uuid: &str,
        branch: &str,
        ancestor: &HashRef,
        current: Option<&HashRef>,
    ) -> Result<bool, StoreError> {
        let Some(mut cursor) = current.cloned() else {
            return Ok(false);
        };
        for _ in 0..4096 {
            let Some(manifest) = self.read_manifest(&cursor)? else {
                return Ok(false);
            };
            if manifest.document_uuid != document_uuid || manifest.branch != branch {
                return Err(StoreError::CorruptHead);
            }
            let Some(parent) = manifest.parent else {
                return Ok(false);
            };
            if &parent == ancestor {
                return Ok(true);
            }
            cursor = parent;
        }
        Err(StoreError::Format(
            "manifest ancestry exceeded traversal limit".to_string(),
        ))
    }

    pub fn write_lookup_record(&self, record: &LookupRecord) -> Result<Vec<String>, StoreError> {
        record
            .validate()
            .map_err(|err| StoreError::Format(err.to_string()))?;
        let bytes = encode_record(record);
        let mut paths = vec![uuid_lookup_path(&record.document_uuid)?];
        for alias in &record.aliases {
            if alias.scheme.trim().eq_ignore_ascii_case("doi") {
                paths.push(doi_lookup_path(&alias.value)?);
            }
        }
        paths.sort();
        paths.dedup();
        for path in &paths {
            self.store.put_named(path, &bytes)?;
        }
        Ok(paths)
    }

    pub fn read_uuid_lookup(
        &self,
        document_uuid: &str,
    ) -> Result<Option<LookupRecord>, StoreError> {
        let path = uuid_lookup_path(document_uuid)?;
        let Some(record) = self.read_lookup_at(&path)? else {
            return Ok(None);
        };
        if record.document_uuid != clean_key_segment(document_uuid)? {
            return Err(StoreError::LookupMismatch);
        }
        Ok(Some(record))
    }

    pub fn read_doi_lookup(&self, doi: &str) -> Result<Option<LookupRecord>, StoreError> {
        let path = doi_lookup_path(doi)?;
        let Some(record) = self.read_lookup_at(&path)? else {
            return Ok(None);
        };
        if !lookup_record_matches_doi_path(&record, &path)? {
            return Err(StoreError::LookupMismatch);
        }
        Ok(Some(record))
    }

    pub fn scan_lookup_records(&self) -> Result<Vec<LookupRecord>, StoreError> {
        Ok(self.scan_lookup_entries()?.records)
    }

    pub fn scan_lookup_entries(&self) -> Result<LookupScan, StoreError> {
        let mut records = Vec::new();
        let mut invalid = Vec::new();
        for path in self.store.list_prefix("indexes")? {
            if !path.ends_with(".idx") {
                continue;
            }
            let full_path = format!("indexes/{path}");
            let Some(bytes) = self.store.get_named(&full_path)? else {
                continue;
            };
            let record = match decode_record::<LookupRecord>(&bytes) {
                Ok(record) => record,
                Err(err) => {
                    invalid.push(LookupScanProblem {
                        path,
                        reason: err.to_string(),
                    });
                    continue;
                }
            };
            if let Err(err) = record.validate() {
                invalid.push(LookupScanProblem {
                    path,
                    reason: err.to_string(),
                });
                continue;
            }
            match lookup_record_matches_index_path(&record, &full_path) {
                Ok(true) => records.push(record),
                Ok(false) => invalid.push(LookupScanProblem {
                    path,
                    reason: "lookup record does not match index path".to_string(),
                }),
                Err(err) => invalid.push(LookupScanProblem {
                    path,
                    reason: format!("{err:?}"),
                }),
            }
        }
        records.sort_by(|left, right| {
            left.document_uuid
                .cmp(&right.document_uuid)
                .then(left.branch.cmp(&right.branch))
                .then(left.manifest.to_string().cmp(&right.manifest.to_string()))
        });
        records.dedup();
        invalid.sort_by(|left, right| left.path.cmp(&right.path));
        Ok(LookupScan { records, invalid })
    }

    pub fn write_tombstone(&self, record: &TombstoneRecord) -> Result<String, StoreError> {
        record
            .validate()
            .map_err(|err| StoreError::Format(err.to_string()))?;
        let path = tombstone_path(&record.object);
        self.store.put_named(&path, &encode_record(record))?;
        Ok(path)
    }

    pub fn read_tombstone(&self, object: &HashRef) -> Result<Option<TombstoneRecord>, StoreError> {
        let Some(bytes) = self.store.get_named(&tombstone_path(object))? else {
            return Ok(None);
        };
        let record: TombstoneRecord =
            decode_record(&bytes).map_err(|err| StoreError::Format(err.to_string()))?;
        record
            .validate()
            .map_err(|err| StoreError::Format(err.to_string()))?;
        if &record.object != object {
            return Err(StoreError::HashMismatch);
        }
        Ok(Some(record))
    }

    pub fn scan_tombstone_records(&self) -> Result<Vec<TombstoneRecord>, StoreError> {
        let scan = self.scan_tombstone_entries()?;
        if let Some(problem) = scan.invalid.into_iter().next() {
            return Err(if problem.reason.contains("HashMismatch") {
                StoreError::HashMismatch
            } else if problem.reason.contains("InvalidPath") {
                StoreError::InvalidPath
            } else {
                StoreError::Format(problem.reason)
            });
        }
        Ok(scan.records)
    }

    pub fn scan_tombstone_entries(&self) -> Result<TombstoneScan, StoreError> {
        let prefix = "archive/tombstones";
        let mut records = Vec::new();
        let mut invalid = Vec::new();
        for path in self.store.list_prefix(prefix)? {
            if !path.ends_with(".tombstone") {
                continue;
            }
            let full_path = format!("{prefix}/{path}");
            let expected = match tombstone_object_from_path(&full_path) {
                Ok(expected) => expected,
                Err(err) => {
                    invalid.push(TombstoneScanProblem {
                        path,
                        reason: err.to_string(),
                    });
                    continue;
                }
            };
            let Some(bytes) = self.store.get_named(&full_path)? else {
                continue;
            };
            let record: TombstoneRecord = match decode_record(&bytes) {
                Ok(record) => record,
                Err(err) => {
                    invalid.push(TombstoneScanProblem {
                        path,
                        reason: err.to_string(),
                    });
                    continue;
                }
            };
            if let Err(err) = record.validate() {
                invalid.push(TombstoneScanProblem {
                    path,
                    reason: err.to_string(),
                });
                continue;
            }
            if record.object != expected {
                invalid.push(TombstoneScanProblem {
                    path,
                    reason: StoreError::HashMismatch.to_string(),
                });
                continue;
            }
            records.push(record);
        }
        records.sort_by(|left, right| {
            left.object
                .to_string()
                .cmp(&right.object.to_string())
                .then(left.archive_locator.cmp(&right.archive_locator))
                .then(left.restore_hint.cmp(&right.restore_hint))
        });
        records.dedup();
        invalid.sort_by(|left, right| left.path.cmp(&right.path));
        Ok(TombstoneScan { records, invalid })
    }

    pub fn write_blob_signature(
        &self,
        blob: &HashRef,
        record: &SignatureRecord,
    ) -> Result<String, StoreError> {
        record
            .validate()
            .map_err(|err| StoreError::Format(err.to_string()))?;
        if &record.target != blob {
            return Err(StoreError::HashMismatch);
        }
        let path = blob_signature_path(blob);
        self.store.put_named(&path, &encode_record(record))?;
        Ok(path)
    }

    pub fn read_blob_signature(
        &self,
        blob: &HashRef,
    ) -> Result<Option<SignatureRecord>, StoreError> {
        let Some(bytes) = self.store.get_named(&blob_signature_path(blob))? else {
            return Ok(None);
        };
        let record: SignatureRecord =
            decode_record(&bytes).map_err(|err| StoreError::Format(err.to_string()))?;
        record
            .validate()
            .map_err(|err| StoreError::Format(err.to_string()))?;
        if &record.target != blob {
            return Err(StoreError::HashMismatch);
        }
        Ok(Some(record))
    }

    pub fn audit_manifest_dependencies(
        &self,
        manifest: &ManifestRecord,
    ) -> Result<ManifestDependencyAudit, StoreError> {
        manifest
            .validate()
            .map_err(|err| StoreError::Format(err.to_string()))?;
        let snapshot = self.object_status(&manifest.snapshot)?;
        let mut operation_segments = Vec::new();
        for hash in &manifest.operation_segments {
            operation_segments.push(self.object_status(hash)?);
        }
        let mut version_signatures = Vec::new();
        for hash in &manifest.signatures {
            version_signatures.push(self.object_status(hash)?);
        }
        let mut blobs = Vec::new();
        for hash in &manifest.blobs {
            blobs.push(BlobDependencyStatus {
                hash: hash.clone(),
                bytes_present: self.store.exists(hash)?,
                signature_sidecar_present: self.read_blob_signature(hash)?.is_some(),
                archive_tombstone: self.read_tombstone(hash)?,
            });
        }
        Ok(ManifestDependencyAudit {
            snapshot,
            operation_segments,
            version_signatures,
            blobs,
        })
    }

    fn object_status(&self, hash: &HashRef) -> Result<ObjectDependencyStatus, StoreError> {
        Ok(ObjectDependencyStatus {
            hash: hash.clone(),
            present: self.store.exists(hash)?,
        })
    }

    fn read_lookup_at(&self, path: &str) -> Result<Option<LookupRecord>, StoreError> {
        let Some(bytes) = self.store.get_named(path)? else {
            return Ok(None);
        };
        let record: LookupRecord =
            decode_record(&bytes).map_err(|err| StoreError::Format(err.to_string()))?;
        record
            .validate()
            .map_err(|err| StoreError::Format(err.to_string()))?;
        Ok(Some(record))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommitOutcome {
    Committed(HashRef),
    Candidate { manifest: HashRef, path: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateResolution {
    pub current: Option<HashRef>,
    pub candidates: Vec<CandidateHeadStatus>,
    pub invalid_candidates: Vec<CandidateHeadProblem>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LookupScan {
    pub records: Vec<LookupRecord>,
    pub invalid: Vec<LookupScanProblem>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LookupScanProblem {
    pub path: String,
    pub reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateReconciliation {
    pub initial: CandidateResolution,
    pub final_resolution: CandidateResolution,
    pub advanced: Vec<HashRef>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateMergePlans {
    pub resolution: CandidateResolution,
    pub plans: Vec<CandidateMergePlan>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateMergePlan {
    pub current: Option<HashRef>,
    pub candidate: HashRef,
    pub merge_base: Option<HashRef>,
    pub current_since_base: Vec<HashRef>,
    pub candidate_since_base: Vec<HashRef>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateHeadStatus {
    pub manifest: HashRef,
    pub status: CandidateStatus,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateHeadProblem {
    pub path: String,
    pub reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManifestDependencyAudit {
    pub snapshot: ObjectDependencyStatus,
    pub operation_segments: Vec<ObjectDependencyStatus>,
    pub version_signatures: Vec<ObjectDependencyStatus>,
    pub blobs: Vec<BlobDependencyStatus>,
}

impl ManifestDependencyAudit {
    pub fn missing_hashes(&self) -> Vec<HashRef> {
        let mut missing = Vec::new();
        if !self.snapshot.present {
            missing.push(self.snapshot.hash.clone());
        }
        for status in &self.operation_segments {
            if !status.present {
                missing.push(status.hash.clone());
            }
        }
        for status in &self.version_signatures {
            if !status.present {
                missing.push(status.hash.clone());
            }
        }
        for status in &self.blobs {
            if !status.bytes_present {
                missing.push(status.hash.clone());
            }
        }
        missing.sort_by_key(|hash| hash.to_string());
        missing.dedup();
        missing
    }

    pub fn recoverable_missing_blobs(&self) -> Vec<HashRef> {
        let mut recoverable = self
            .blobs
            .iter()
            .filter(|status| !status.bytes_present && status.archive_tombstone.is_some())
            .map(|status| status.hash.clone())
            .collect::<Vec<_>>();
        recoverable.sort_by_key(|hash| hash.to_string());
        recoverable.dedup();
        recoverable
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectDependencyStatus {
    pub hash: HashRef,
    pub present: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlobDependencyStatus {
    pub hash: HashRef,
    pub bytes_present: bool,
    pub signature_sidecar_present: bool,
    pub archive_tombstone: Option<TombstoneRecord>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TombstoneScan {
    pub records: Vec<TombstoneRecord>,
    pub invalid: Vec<TombstoneScanProblem>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TombstoneScanProblem {
    pub path: String,
    pub reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CandidateHeadEntries {
    records: Vec<BranchHeadRecord>,
    invalid: Vec<CandidateHeadProblem>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum CandidateStatus {
    FastForward,
    NeedsMerge,
    AlreadyCurrent,
    IntegratedAncestor,
    MissingManifest,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CandidateAdvance {
    Advanced(HashRef),
    AlreadyCurrent(HashRef),
    NeedsMerge(HashRef),
    MissingManifest(HashRef),
    HeadChanged {
        previous: Option<HashRef>,
        current: Option<HashRef>,
        candidate: HashRef,
    },
}

#[derive(Clone, Debug)]
pub struct FlatObjectStore {
    root: PathBuf,
    namespace: String,
}

impl FlatObjectStore {
    pub fn new(root: impl Into<PathBuf>, namespace: impl Into<String>) -> Result<Self, StoreError> {
        let namespace = namespace.into();
        let namespace = clean_object_prefix(&namespace)?;
        Ok(Self {
            root: root.into(),
            namespace,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    fn key_path(&self, key: &str) -> Result<PathBuf, StoreError> {
        let mut path = self.root.clone();
        if !self.namespace.is_empty() {
            path = path.join(clean_relative_path(&self.namespace)?);
        }
        Ok(path.join(clean_relative_path(key)?))
    }

    pub fn compact_loose_objects_to_pack(&self, pack_name: &str) -> Result<PackStats, StoreError> {
        let pack_name = clean_pack_name(pack_name)?;
        let loose = self.loose_objects()?;
        let mut objects = self.objects_in_pack(pack_name)?;
        let mut packed_loose = Vec::new();
        for hash in loose {
            let Some(bytes) = self.get_loose(&hash)? else {
                continue;
            };
            packed_loose.push((hash.clone(), bytes.clone()));
            objects.push((hash, bytes));
        }
        let stats = self.write_pack(pack_name, &objects)?;
        for (hash, bytes) in packed_loose {
            if self.get_packed(&hash)?.as_deref() == Some(bytes.as_slice()) {
                remove_file_if_exists(&self.key_path(&ObjectStoreLayout::object_key(&hash))?)?;
            }
        }
        Ok(stats)
    }

    pub fn write_pack(
        &self,
        pack_name: &str,
        objects: &[(HashRef, Vec<u8>)],
    ) -> Result<PackStats, StoreError> {
        let pack_name = clean_pack_name(pack_name)?;
        let pack_dir = self.key_path("packs")?;
        write_pack_files(&pack_dir, pack_name, objects)
    }

    fn objects_in_pack(&self, pack_name: &str) -> Result<Vec<(HashRef, Vec<u8>)>, StoreError> {
        let pack_name = clean_pack_name(pack_name)?;
        read_objects_from_pack_dir(&self.key_path("packs")?, pack_name)
    }

    fn get_loose(&self, hash: &HashRef) -> Result<Option<Vec<u8>>, StoreError> {
        match fs::read(self.key_path(&ObjectStoreLayout::object_key(hash))?) {
            Ok(bytes) => {
                let actual = digest_bytes(hash.algorithm(), &bytes)
                    .map_err(|_| StoreError::UnsupportedHash)?;
                if &actual != hash {
                    return Err(StoreError::HashMismatch);
                }
                Ok(Some(bytes))
            }
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
            Err(err) => Err(StoreError::Io(err.to_string())),
        }
    }

    fn get_packed(&self, hash: &HashRef) -> Result<Option<Vec<u8>>, StoreError> {
        get_packed_from_dir(&self.key_path("packs")?, hash)
    }

    fn loose_objects(&self) -> Result<Vec<HashRef>, StoreError> {
        loose_objects_from_root(&self.key_path("objects")?)
    }
}

impl ObjectStore for FlatObjectStore {
    fn capabilities(&self) -> StoreCapabilities {
        StoreCapabilities {
            idempotent_content_put: true,
            compare_and_swap_head: true,
            list_prefix: true,
            atomic_named_overwrite: true,
            local_pack_files: true,
        }
    }

    fn put_if_absent(&self, hash: &HashRef, bytes: &[u8]) -> Result<bool, StoreError> {
        let actual =
            digest_bytes(hash.algorithm(), bytes).map_err(|_| StoreError::UnsupportedHash)?;
        if &actual != hash {
            return Err(StoreError::HashMismatch);
        }
        if self.exists(hash)? {
            return Ok(false);
        }
        let path = self.key_path(&ObjectStoreLayout::object_key(hash))?;
        if path.exists() {
            return Ok(false);
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension(format!("tmp-{}", process_tag()));
        {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&tmp)?;
            file.write_all(bytes)?;
            file.sync_all()?;
        }
        match fs::hard_link(&tmp, &path) {
            Ok(()) => {
                let _ = fs::remove_file(&tmp);
                Ok(true)
            }
            Err(err) if err.kind() == ErrorKind::AlreadyExists => {
                let _ = fs::remove_file(&tmp);
                Ok(false)
            }
            Err(_) => {
                fs::rename(&tmp, &path)?;
                Ok(true)
            }
        }
    }

    fn get(&self, hash: &HashRef) -> Result<Option<Vec<u8>>, StoreError> {
        if let Some(bytes) = self.get_loose(hash)? {
            return Ok(Some(bytes));
        }
        self.get_packed(hash)
    }

    fn exists(&self, hash: &HashRef) -> Result<bool, StoreError> {
        if self.get_loose(hash)?.is_some() {
            return Ok(true);
        }
        Ok(self.get_packed(hash)?.is_some())
    }

    fn compact_loose_objects_to_pack(&self, pack_name: &str) -> Result<PackStats, StoreError> {
        FlatObjectStore::compact_loose_objects_to_pack(self, pack_name)
    }

    fn put_named(&self, path: &str, bytes: &[u8]) -> Result<(), StoreError> {
        let path = self.key_path(path)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension(format!("tmp-{}", process_tag()));
        {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&tmp)?;
            file.write_all(bytes)?;
            file.sync_all()?;
        }
        fs::rename(tmp, path)?;
        Ok(())
    }

    fn get_named(&self, path: &str) -> Result<Option<Vec<u8>>, StoreError> {
        match fs::read(self.key_path(path)?) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
            Err(err) => Err(StoreError::Io(err.to_string())),
        }
    }

    fn list_prefix(&self, prefix: &str) -> Result<Vec<String>, StoreError> {
        let base = self.key_path(prefix)?;
        let mut out = Vec::new();
        if !base.exists() {
            return Ok(out);
        }
        collect_paths(&base, &base, &mut out)?;
        out.sort();
        Ok(out)
    }

    fn compare_and_swap_head(
        &self,
        document_uuid: &str,
        branch: &str,
        expected: Option<&HashRef>,
        new: &HashRef,
    ) -> Result<bool, StoreError> {
        let head_key = ObjectStoreLayout::head_key(document_uuid, branch)?;
        let head_path = self.key_path(&head_key)?;
        if let Some(parent) = head_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let _lock = HeadLock::acquire(&head_lock_path(&head_path))?;
        let current = self.read_head(document_uuid, branch)?;
        if current.as_ref() != expected {
            return Ok(false);
        }
        self.put_named(&head_key, new.to_string().as_bytes())?;
        Ok(true)
    }

    fn read_head(&self, document_uuid: &str, branch: &str) -> Result<Option<HashRef>, StoreError> {
        let Some(bytes) = self.get_named(&ObjectStoreLayout::head_key(document_uuid, branch)?)?
        else {
            return Ok(None);
        };
        let value = String::from_utf8(bytes).map_err(|err| StoreError::Format(err.to_string()))?;
        Ok(Some(parse_head_value(&value)?))
    }
}

#[cfg(feature = "opendal")]
#[derive(Clone, Debug)]
pub struct OpenDalObjectStore {
    operator: opendal::blocking::Operator,
    namespace: String,
    _runtime: Option<Arc<tokio::runtime::Runtime>>,
}

#[cfg(feature = "opendal")]
impl OpenDalObjectStore {
    pub fn new(
        operator: opendal::blocking::Operator,
        namespace: impl Into<String>,
    ) -> Result<Self, StoreError> {
        let namespace = clean_object_prefix(&namespace.into())?;
        Ok(Self {
            operator,
            namespace,
            _runtime: None,
        })
    }

    pub fn from_fs_root(
        root: impl AsRef<Path>,
        namespace: impl Into<String>,
    ) -> Result<Self, StoreError> {
        let builder = opendal::services::Fs::default().root(
            root.as_ref()
                .to_str()
                .ok_or_else(|| StoreError::InvalidPath)?,
        );
        let operator =
            opendal::Operator::new(builder).map_err(|err| StoreError::Io(err.to_string()))?;
        let runtime = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .map_err(|err| StoreError::Io(err.to_string()))?,
        );
        let _guard = runtime.enter();
        let operator = opendal::blocking::Operator::new(operator)
            .map_err(|err| StoreError::Io(err.to_string()))?;
        let mut store = Self::new(operator, namespace)?;
        store._runtime = Some(runtime);
        Ok(store)
    }

    pub fn operator(&self) -> &opendal::blocking::Operator {
        &self.operator
    }

    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    fn key(&self, key: &str) -> Result<String, StoreError> {
        join_object_key(&self.namespace, key)
    }
}

#[cfg(feature = "opendal")]
impl ObjectStore for OpenDalObjectStore {
    fn capabilities(&self) -> StoreCapabilities {
        StoreCapabilities {
            idempotent_content_put: true,
            compare_and_swap_head: false,
            list_prefix: true,
            atomic_named_overwrite: false,
            local_pack_files: false,
        }
    }

    fn put_if_absent(&self, hash: &HashRef, bytes: &[u8]) -> Result<bool, StoreError> {
        let actual =
            digest_bytes(hash.algorithm(), bytes).map_err(|_| StoreError::UnsupportedHash)?;
        if &actual != hash {
            return Err(StoreError::HashMismatch);
        }
        let key = self.key(&ObjectStoreLayout::object_key(hash))?;
        if self.exists(hash)? {
            return Ok(false);
        }
        self.operator
            .write(&key, bytes.to_vec())
            .map_err(|err| StoreError::Io(err.to_string()))?;
        Ok(true)
    }

    fn get(&self, hash: &HashRef) -> Result<Option<Vec<u8>>, StoreError> {
        match self
            .operator
            .read(&self.key(&ObjectStoreLayout::object_key(hash))?)
        {
            Ok(bytes) => {
                let bytes = bytes.to_vec();
                let actual = digest_bytes(hash.algorithm(), &bytes)
                    .map_err(|_| StoreError::UnsupportedHash)?;
                if &actual != hash {
                    return Err(StoreError::HashMismatch);
                }
                Ok(Some(bytes))
            }
            Err(err) if is_opendal_not_found(&err) => Ok(None),
            Err(err) => Err(StoreError::Io(err.to_string())),
        }
    }

    fn exists(&self, hash: &HashRef) -> Result<bool, StoreError> {
        Ok(self.get(hash)?.is_some())
    }

    fn put_named(&self, path: &str, bytes: &[u8]) -> Result<(), StoreError> {
        self.operator
            .write(&self.key(path)?, bytes.to_vec())
            .map_err(|err| StoreError::Io(err.to_string()))?;
        Ok(())
    }

    fn get_named(&self, path: &str) -> Result<Option<Vec<u8>>, StoreError> {
        match self.operator.read(&self.key(path)?) {
            Ok(bytes) => Ok(Some(bytes.to_vec())),
            Err(err) if is_opendal_not_found(&err) => Ok(None),
            Err(err) => Err(StoreError::Io(err.to_string())),
        }
    }

    fn list_prefix(&self, prefix: &str) -> Result<Vec<String>, StoreError> {
        let clean_prefix = opendal_prefix_key(prefix)?;
        let key_prefix = join_object_key(&self.namespace, &clean_prefix)?;
        let mut entries = self
            .operator
            .list_options(
                &key_prefix,
                opendal::options::ListOptions {
                    recursive: true,
                    ..Default::default()
                },
            )
            .map_err(|err| StoreError::Io(err.to_string()))?;
        entries.sort_by(|left, right| left.path().cmp(right.path()));
        let mut out = Vec::new();
        for entry in entries {
            if entry.metadata().mode() == opendal::EntryMode::DIR {
                continue;
            }
            let path = entry.path();
            let Some(relative) = path.strip_prefix(&key_prefix) else {
                continue;
            };
            if !relative.is_empty() {
                out.push(relative.to_string());
            }
        }
        out.sort();
        Ok(out)
    }

    fn compare_and_swap_head(
        &self,
        document_uuid: &str,
        branch: &str,
        expected: Option<&HashRef>,
        new: &HashRef,
    ) -> Result<bool, StoreError> {
        let current = self.read_head(document_uuid, branch)?;
        if current.as_ref() != expected {
            return Ok(false);
        }
        self.put_named(
            &ObjectStoreLayout::head_key(document_uuid, branch)?,
            new.to_string().as_bytes(),
        )?;
        Ok(true)
    }

    fn read_head(&self, document_uuid: &str, branch: &str) -> Result<Option<HashRef>, StoreError> {
        let Some(bytes) = self.get_named(&ObjectStoreLayout::head_key(document_uuid, branch)?)?
        else {
            return Ok(None);
        };
        let value = String::from_utf8(bytes).map_err(|err| StoreError::Format(err.to_string()))?;
        Ok(Some(parse_head_value(&value)?))
    }
}

#[derive(Clone, Debug)]
pub struct LocalObjectStore {
    root: PathBuf,
}

impl LocalObjectStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn object_path(&self, hash: &HashRef) -> PathBuf {
        self.root.join(ObjectStoreLayout::object_key(hash))
    }

    fn head_path(&self, document_uuid: &str, branch: &str) -> PathBuf {
        self.root.join(
            ObjectStoreLayout::head_key(document_uuid, branch)
                .expect("document uuid and branch are valid relative key segments"),
        )
    }

    pub fn compact_loose_objects_to_pack(&self, pack_name: &str) -> Result<PackStats, StoreError> {
        let pack_name = clean_pack_name(pack_name)?;
        let loose = self.loose_objects()?;
        let mut objects = self.objects_in_pack(pack_name)?;
        let mut packed_loose = Vec::new();
        for hash in loose {
            let Some(bytes) = self.get_loose(&hash)? else {
                continue;
            };
            packed_loose.push((hash.clone(), bytes.clone()));
            objects.push((hash, bytes));
        }
        let stats = self.write_pack(pack_name, &objects)?;
        // Only reclaim a loose object once it is readable from the pack.
        for (hash, bytes) in packed_loose {
            if self.get_packed(&hash)?.as_deref() == Some(bytes.as_slice()) {
                remove_file_if_exists(&self.object_path(&hash))?;
            }
        }
        Ok(stats)
    }

    pub fn write_pack(
        &self,
        pack_name: &str,
        objects: &[(HashRef, Vec<u8>)],
    ) -> Result<PackStats, StoreError> {
        let pack_name = clean_pack_name(pack_name)?;
        write_pack_files(&self.root.join("packs"), pack_name, objects)
    }

    fn objects_in_pack(&self, pack_name: &str) -> Result<Vec<(HashRef, Vec<u8>)>, StoreError> {
        let pack_name = clean_pack_name(pack_name)?;
        read_objects_from_pack_dir(&self.root.join("packs"), pack_name)
    }

    fn get_loose(&self, hash: &HashRef) -> Result<Option<Vec<u8>>, StoreError> {
        let path = self.object_path(hash);
        match fs::read(path) {
            Ok(bytes) => {
                let actual = digest_bytes(hash.algorithm(), &bytes)
                    .map_err(|_| StoreError::UnsupportedHash)?;
                if &actual != hash {
                    return Err(StoreError::HashMismatch);
                }
                Ok(Some(bytes))
            }
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
            Err(err) => Err(StoreError::Io(err.to_string())),
        }
    }

    fn get_packed(&self, hash: &HashRef) -> Result<Option<Vec<u8>>, StoreError> {
        get_packed_from_dir(&self.root.join("packs"), hash)
    }

    fn loose_objects(&self) -> Result<Vec<HashRef>, StoreError> {
        loose_objects_from_root(&self.root.join("objects"))
    }
}

fn write_pack_files(
    pack_dir: &Path,
    pack_name: &str,
    objects: &[(HashRef, Vec<u8>)],
) -> Result<PackStats, StoreError> {
    let pack_name = clean_pack_name(pack_name)?;
    fs::create_dir_all(pack_dir)?;
    cleanup_pack_temp_files(pack_dir, pack_name)?;
    let pack_path = pack_dir.join(format!("{pack_name}.pack"));
    let index_path = pack_dir.join(format!("{pack_name}.idx"));
    let tmp_pack = pack_path.with_extension(format!("pack.tmp-{}", process_tag()));
    let tmp_index = index_path.with_extension(format!("idx.tmp-{}", process_tag()));

    let mut sorted = objects.to_vec();
    sorted.sort_by_key(|(left, _)| left.to_string());
    sorted.dedup_by(|(left, _), (right, _)| left == right);
    for (hash, bytes) in &sorted {
        let actual =
            digest_bytes(hash.algorithm(), bytes).map_err(|_| StoreError::UnsupportedHash)?;
        if &actual != hash {
            return Err(StoreError::HashMismatch);
        }
    }

    let mut index = Vec::new();
    let mut offset = 0u64;
    {
        let mut pack = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp_pack)?;
        pack.write_all(b"ODP0")?;
        offset += 4;
        for (hash, bytes) in &sorted {
            pack.write_all(bytes)?;
            index.push(PackIndexEntry {
                hash: hash.clone(),
                pack: pack_name.to_string(),
                offset,
                length: bytes.len() as u64,
            });
            offset += bytes.len() as u64;
        }
        pack.sync_all()?;
    }

    fs::write(&tmp_index, encode_pack_index(pack_name, &index))?;
    verify_pack_index_paths(&tmp_pack, &index)?;
    fs::rename(&tmp_pack, &pack_path)?;
    fs::rename(&tmp_index, &index_path)?;
    Ok(PackStats {
        pack: pack_name.to_string(),
        objects: index.len(),
        bytes: offset,
    })
}

fn read_objects_from_pack_dir(
    pack_dir: &Path,
    pack_name: &str,
) -> Result<Vec<(HashRef, Vec<u8>)>, StoreError> {
    let pack_name = clean_pack_name(pack_name)?;
    let index_path = pack_dir.join(format!("{pack_name}.idx"));
    if !index_path.exists() {
        return Ok(Vec::new());
    }
    let pack_path = pack_dir.join(format!("{pack_name}.pack"));
    let index_bytes = fs::read(index_path)?;
    let mut objects = Vec::new();
    for entry in decode_pack_index(&index_bytes)? {
        if entry.pack != pack_name {
            return Err(StoreError::Format(format!(
                "pack index entry targets unexpected pack {}",
                entry.pack
            )));
        }
        objects.push((entry.hash.clone(), read_pack_entry(&pack_path, &entry)?));
    }
    Ok(objects)
}

fn get_packed_from_dir(pack_dir: &Path, hash: &HashRef) -> Result<Option<Vec<u8>>, StoreError> {
    for index_path in pack_index_paths(pack_dir)? {
        let expected_pack = pack_name_from_index_path(&index_path)?;
        let index_bytes = fs::read(&index_path)?;
        for entry in decode_pack_index(&index_bytes)? {
            if entry.pack != expected_pack {
                return Err(StoreError::Format(format!(
                    "pack index file targets unexpected pack {}",
                    entry.pack
                )));
            }
            if &entry.hash != hash {
                continue;
            }
            let pack_path = index_path.with_file_name(format!("{}.pack", entry.pack));
            return Ok(Some(read_pack_entry(&pack_path, &entry)?));
        }
    }
    Ok(None)
}

fn pack_index_paths(pack_dir: &Path) -> Result<Vec<PathBuf>, StoreError> {
    if !pack_dir.exists() {
        return Ok(Vec::new());
    }
    let mut paths = Vec::new();
    for entry in fs::read_dir(pack_dir)? {
        let path = entry?.path();
        if path.extension().and_then(|value| value.to_str()) == Some("idx") {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

fn loose_objects_from_root(object_root: &Path) -> Result<Vec<HashRef>, StoreError> {
    let mut out = Vec::new();
    if !object_root.exists() {
        return Ok(out);
    }
    for algorithm_entry in fs::read_dir(object_root)? {
        let algorithm_entry = algorithm_entry?;
        if !algorithm_entry.path().is_dir() {
            continue;
        }
        let algorithm = algorithm_entry.file_name().to_string_lossy().to_string();
        for prefix_entry in fs::read_dir(algorithm_entry.path())? {
            let prefix_entry = prefix_entry?;
            if !prefix_entry.path().is_dir() {
                continue;
            }
            for object_entry in fs::read_dir(prefix_entry.path())? {
                let object_entry = object_entry?;
                let path = object_entry.path();
                if !path.is_file() || path.extension().is_some() {
                    continue;
                }
                let digest = object_entry.file_name().to_string_lossy().to_string();
                let hash_text = format!("{algorithm}:{digest}");
                out.push(HashRef::parse(&hash_text).map_err(|_| StoreError::UnsupportedHash)?);
            }
        }
    }
    out.sort_by_key(|hash| hash.to_string());
    Ok(out)
}

impl ObjectStore for LocalObjectStore {
    fn capabilities(&self) -> StoreCapabilities {
        StoreCapabilities {
            idempotent_content_put: true,
            compare_and_swap_head: true,
            list_prefix: true,
            atomic_named_overwrite: true,
            local_pack_files: true,
        }
    }

    fn put_if_absent(&self, hash: &HashRef, bytes: &[u8]) -> Result<bool, StoreError> {
        let actual =
            digest_bytes(hash.algorithm(), bytes).map_err(|_| StoreError::UnsupportedHash)?;
        if &actual != hash {
            return Err(StoreError::HashMismatch);
        }
        let path = self.object_path(hash);
        if self.exists(hash)? {
            return Ok(false);
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension(format!("tmp-{}", process_tag()));
        {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&tmp)?;
            file.write_all(bytes)?;
            file.sync_all()?;
        }
        match fs::hard_link(&tmp, &path) {
            Ok(()) => {
                let _ = fs::remove_file(&tmp);
                Ok(true)
            }
            Err(err) if err.kind() == ErrorKind::AlreadyExists => {
                let _ = fs::remove_file(&tmp);
                Ok(false)
            }
            Err(_) => {
                fs::rename(&tmp, &path)?;
                Ok(true)
            }
        }
    }

    fn get(&self, hash: &HashRef) -> Result<Option<Vec<u8>>, StoreError> {
        if let Some(bytes) = self.get_loose(hash)? {
            return Ok(Some(bytes));
        }
        self.get_packed(hash)
    }

    fn exists(&self, hash: &HashRef) -> Result<bool, StoreError> {
        if self.get_loose(hash)?.is_some() {
            return Ok(true);
        }
        Ok(self.get_packed(hash)?.is_some())
    }

    fn compact_loose_objects_to_pack(&self, pack_name: &str) -> Result<PackStats, StoreError> {
        LocalObjectStore::compact_loose_objects_to_pack(self, pack_name)
    }

    fn put_named(&self, path: &str, bytes: &[u8]) -> Result<(), StoreError> {
        let path = self.root.join(clean_relative_path(path)?);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension(format!("tmp-{}", process_tag()));
        {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&tmp)?;
            file.write_all(bytes)?;
            file.sync_all()?;
        }
        fs::rename(tmp, path)?;
        Ok(())
    }

    fn get_named(&self, path: &str) -> Result<Option<Vec<u8>>, StoreError> {
        let path = self.root.join(clean_relative_path(path)?);
        match fs::read(path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
            Err(err) => Err(StoreError::Io(err.to_string())),
        }
    }

    fn list_prefix(&self, prefix: &str) -> Result<Vec<String>, StoreError> {
        let base = self.root.join(clean_relative_path(prefix)?);
        let mut out = Vec::new();
        if !base.exists() {
            return Ok(out);
        }
        collect_paths(&base, &base, &mut out)?;
        out.sort();
        Ok(out)
    }

    fn compare_and_swap_head(
        &self,
        document_uuid: &str,
        branch: &str,
        expected: Option<&HashRef>,
        new: &HashRef,
    ) -> Result<bool, StoreError> {
        let path = self.head_path(document_uuid, branch);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let _lock = HeadLock::acquire(&head_lock_path(&path))?;
        let current = self.read_head(document_uuid, branch)?;
        if current.as_ref() != expected {
            return Ok(false);
        }
        let tmp = path.with_extension(format!("tmp-{}", process_tag()));
        fs::write(&tmp, new.to_string())?;
        fs::rename(tmp, path)?;
        Ok(true)
    }

    fn read_head(&self, document_uuid: &str, branch: &str) -> Result<Option<HashRef>, StoreError> {
        let path = self.head_path(document_uuid, branch);
        match fs::read_to_string(path) {
            Ok(value) => Ok(Some(parse_head_value(&value)?)),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
            Err(err) => Err(StoreError::Io(err.to_string())),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackStats {
    pub pack: String,
    pub objects: usize,
    pub bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PackIndexEntry {
    hash: HashRef,
    pack: String,
    offset: u64,
    length: u64,
}

fn encode_pack_index(pack_name: &str, entries: &[PackIndexEntry]) -> Vec<u8> {
    let record = PackIndexRecord {
        pack: pack_name.to_string(),
        entries: entries
            .iter()
            .map(|entry| PackIndexEntryRecord {
                hash: entry.hash.clone(),
                offset: entry.offset,
                length: entry.length,
            })
            .collect(),
    };
    encode_record(&record)
}

fn decode_pack_index(value: &[u8]) -> Result<Vec<PackIndexEntry>, StoreError> {
    let record: PackIndexRecord =
        decode_record(value).map_err(|err| StoreError::Format(err.to_string()))?;
    record
        .validate()
        .map_err(|err| StoreError::Format(err.to_string()))?;
    let pack = clean_pack_name(&record.pack)?.to_string();
    Ok(record
        .entries
        .into_iter()
        .map(|entry| PackIndexEntry {
            hash: entry.hash,
            pack: pack.clone(),
            offset: entry.offset,
            length: entry.length,
        })
        .collect())
}

fn verify_pack_index_paths(pack_path: &Path, index: &[PackIndexEntry]) -> Result<(), StoreError> {
    let mut file = fs::File::open(pack_path)?;
    let mut magic = [0; 4];
    file.read_exact(&mut magic)?;
    if &magic != b"ODP0" {
        return Err(StoreError::Format("unsupported pack file".to_string()));
    }
    for entry in index {
        validate_pack_entry_range(&file, entry)?;
        file.seek(SeekFrom::Start(entry.offset))?;
        let length = usize::try_from(entry.length).map_err(|_| {
            StoreError::Format("pack index entry length exceeds platform limit".to_string())
        })?;
        let mut bytes = vec![0; length];
        file.read_exact(&mut bytes)?;
        let actual = digest_bytes(entry.hash.algorithm(), &bytes)
            .map_err(|_| StoreError::UnsupportedHash)?;
        if actual != entry.hash {
            return Err(StoreError::HashMismatch);
        }
    }
    Ok(())
}

fn read_pack_entry(pack_path: &Path, entry: &PackIndexEntry) -> Result<Vec<u8>, StoreError> {
    let mut file = fs::File::open(pack_path)?;
    validate_pack_entry_range(&file, entry)?;
    file.seek(SeekFrom::Start(entry.offset))?;
    let length = usize::try_from(entry.length).map_err(|_| {
        StoreError::Format("pack index entry length exceeds platform limit".to_string())
    })?;
    let mut bytes = vec![0; length];
    file.read_exact(&mut bytes)?;
    let actual =
        digest_bytes(entry.hash.algorithm(), &bytes).map_err(|_| StoreError::UnsupportedHash)?;
    if actual != entry.hash {
        return Err(StoreError::HashMismatch);
    }
    Ok(bytes)
}

fn validate_pack_entry_range(file: &fs::File, entry: &PackIndexEntry) -> Result<(), StoreError> {
    let end = entry
        .offset
        .checked_add(entry.length)
        .ok_or_else(|| StoreError::Format("pack index entry range overflows".to_string()))?;
    let pack_len = file.metadata()?.len();
    if end > pack_len {
        return Err(StoreError::Format(
            "pack index entry range exceeds pack size".to_string(),
        ));
    }
    Ok(())
}

fn pack_name_from_index_path(path: &Path) -> Result<String, StoreError> {
    let Some(stem) = path.file_stem().and_then(|value| value.to_str()) else {
        return Err(StoreError::InvalidPath);
    };
    Ok(clean_pack_name(stem)?.to_string())
}

fn cleanup_pack_temp_files(pack_dir: &Path, pack_name: &str) -> Result<(), StoreError> {
    let pack_tmp_prefix = format!("{pack_name}.pack.tmp-");
    let index_tmp_prefix = format!("{pack_name}.idx.tmp-");
    for entry in fs::read_dir(pack_dir)? {
        let path = entry?.path();
        let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if file_name.starts_with(&pack_tmp_prefix) || file_name.starts_with(&index_tmp_prefix) {
            match fs::remove_file(&path) {
                Ok(()) => {}
                Err(err) if err.kind() == ErrorKind::NotFound => {}
                Err(err) => return Err(StoreError::Io(err.to_string())),
            }
        }
    }
    Ok(())
}

fn clean_pack_name(value: &str) -> Result<&str, StoreError> {
    let valid = !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_');
    if valid {
        Ok(value)
    } else {
        Err(StoreError::InvalidPath)
    }
}

fn clean_object_prefix(value: &str) -> Result<String, StoreError> {
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

fn clean_key_segment(value: &str) -> Result<&str, StoreError> {
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

/// Process id for temp-file names; browsers have no processes.
#[cfg(not(target_arch = "wasm32"))]
fn process_tag() -> u32 {
    std::process::id()
}

#[cfg(target_arch = "wasm32")]
fn process_tag() -> u32 {
    0
}

fn remove_file_if_exists(path: &Path) -> Result<(), StoreError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
        Err(err) => Err(StoreError::Io(err.to_string())),
    }
}

fn head_lock_path(head_path: &Path) -> PathBuf {
    let mut name = head_path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    name.push_str(".lock");
    head_path.with_file_name(name)
}

/// Exclusive advisory lock around a branch-head compare-and-swap, taken with
/// `O_EXCL` so two processes on the same filesystem cannot both observe the
/// old head and both "win" the swap. Stale locks left by a crashed process
/// are reclaimed after [`HEAD_LOCK_STALE_MS`].
struct HeadLock {
    path: PathBuf,
}

const HEAD_LOCK_STALE_MS: u128 = 30_000;
const HEAD_LOCK_ATTEMPTS: u32 = 200;

impl HeadLock {
    fn acquire(path: &Path) -> Result<Self, StoreError> {
        for _ in 0..HEAD_LOCK_ATTEMPTS {
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
            {
                Ok(mut file) => {
                    let _ = file.write_all(process_tag().to_string().as_bytes());
                    return Ok(Self {
                        path: path.to_path_buf(),
                    });
                }
                Err(err) if err.kind() == ErrorKind::AlreadyExists => {
                    let stale = fs::metadata(path)
                        .and_then(|meta| meta.modified())
                        .ok()
                        .and_then(|modified| modified.elapsed().ok())
                        .map(|age| age.as_millis() > HEAD_LOCK_STALE_MS)
                        .unwrap_or(false);
                    if stale {
                        let _ = fs::remove_file(path);
                        continue;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                Err(err) => return Err(StoreError::Io(err.to_string())),
            }
        }
        Err(StoreError::Io(format!(
            "branch head lock {} is busy",
            path.display()
        )))
    }
}

impl Drop for HeadLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn parse_head_value(value: &str) -> Result<HashRef, StoreError> {
    if value.trim() != value {
        return Err(StoreError::CorruptHead);
    }
    HashRef::parse(value).map_err(|_| StoreError::CorruptHead)
}

fn uuid_lookup_path(document_uuid: &str) -> Result<String, StoreError> {
    let document_uuid = clean_key_segment(document_uuid)?;
    let prefix = &document_uuid[..document_uuid.len().min(2)];
    Ok(format!("indexes/by-uuid/{prefix}/{document_uuid}.idx"))
}

fn doi_lookup_path(doi: &str) -> Result<String, StoreError> {
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

fn lookup_record_matches_index_path(record: &LookupRecord, path: &str) -> Result<bool, StoreError> {
    if path == uuid_lookup_path(&record.document_uuid)? {
        return Ok(true);
    }
    lookup_record_matches_doi_path(record, path)
}

fn lookup_record_matches_doi_path(record: &LookupRecord, path: &str) -> Result<bool, StoreError> {
    for alias in &record.aliases {
        if alias.scheme.trim().eq_ignore_ascii_case("doi") && doi_lookup_path(&alias.value)? == path
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn candidate_manifest_from_path(path: &str) -> Result<HashRef, StoreError> {
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

fn tombstone_path(object: &HashRef) -> String {
    let digest = object.digest();
    let prefix = &digest[..digest.len().min(2)];
    format!(
        "archive/tombstones/{}/{}/{}.tombstone",
        object.algorithm(),
        prefix,
        digest
    )
}

fn tombstone_object_from_path(path: &str) -> Result<HashRef, StoreError> {
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

fn blob_signature_path(object: &HashRef) -> String {
    let digest = object.digest();
    let prefix = &digest[..digest.len().min(2)];
    format!("objects/{}/{}/{}.sig", object.algorithm(), prefix, digest)
}

fn clean_relative_path(path: &str) -> Result<&Path, StoreError> {
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
fn join_object_key(namespace: &str, key: &str) -> Result<String, StoreError> {
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
fn opendal_prefix_key(prefix: &str) -> Result<String, StoreError> {
    let mut prefix = clean_relative_path(prefix)?
        .to_string_lossy()
        .replace('\\', "/");
    if !prefix.is_empty() && !prefix.ends_with('/') {
        prefix.push('/');
    }
    Ok(prefix)
}

#[cfg(feature = "opendal")]
fn is_opendal_not_found(err: &opendal::Error) -> bool {
    err.kind() == opendal::ErrorKind::NotFound
}

fn collect_paths(base: &Path, current: &Path, out: &mut Vec<String>) -> Result<(), StoreError> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_paths(base, &path, out)?;
        } else if let Ok(relative) = path.strip_prefix(base) {
            out.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(())
}

#[derive(Debug)]
pub enum StoreError {
    Io(String),
    CorruptHead,
    Format(String),
    HashMismatch,
    LookupMismatch,
    InvalidPath,
    UnsupportedHash,
}

impl From<std::io::Error> for StoreError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value.to_string())
    }
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for StoreError {}

#[cfg(test)]
mod tests {
    use super::*;
    use opendoc_format::LookupAliasRecord;

    #[test]
    fn local_store_round_trips_object_and_head() {
        let root = std::env::temp_dir().join(format!("opendoc-store-{}", process_tag()));
        let _ = fs::remove_dir_all(&root);
        let store = LocalObjectStore::new(&root);
        assert_eq!(
            store.capabilities(),
            StoreCapabilities {
                idempotent_content_put: true,
                compare_and_swap_head: true,
                list_prefix: true,
                atomic_named_overwrite: true,
                local_pack_files: true,
            }
        );
        let hash = digest_bytes("sha256", b"hello").unwrap();
        assert!(store.put_if_absent(&hash, b"hello").unwrap());
        assert!(!store.put_if_absent(&hash, b"hello").unwrap());
        assert_eq!(store.get(&hash).unwrap(), Some(b"hello".to_vec()));
        assert!(store
            .compare_and_swap_head("doc", "main", None, &hash)
            .unwrap());
        assert_eq!(store.read_head("doc", "main").unwrap(), Some(hash.clone()));
        let other = HashRef::parse("sha256:123456").unwrap();
        assert!(!store
            .compare_and_swap_head("doc", "main", None, &other)
            .unwrap());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn object_store_layout_is_backend_neutral_and_s3_safe() {
        let hash = digest_bytes("sha256", b"layout").unwrap();
        assert_eq!(
            ObjectStoreLayout::object_key(&hash),
            format!("objects/sha256/{}/{}", &hash.digest()[..2], hash.digest())
        );
        assert_eq!(
            ObjectStoreLayout::blob_signature_key(&hash),
            format!(
                "objects/sha256/{}/{}.sig",
                &hash.digest()[..2],
                hash.digest()
            )
        );
        assert_eq!(
            ObjectStoreLayout::head_key("doc-1", "main").unwrap(),
            "documents/doc-1/heads/main.head"
        );
        assert_eq!(
            ObjectStoreLayout::candidate_head_key("doc-1", "main", &hash).unwrap(),
            format!(
                "documents/doc-1/head-candidates/main/sha256/{}.head",
                hash.digest()
            )
        );
        assert!(matches!(
            ObjectStoreLayout::head_key("../doc", "main"),
            Err(StoreError::InvalidPath)
        ));
        assert!(matches!(
            ObjectStoreLayout::head_key("..", "main"),
            Err(StoreError::InvalidPath)
        ));
        assert!(matches!(
            ObjectStoreLayout::head_key("doc", "feature/x"),
            Err(StoreError::InvalidPath)
        ));
        assert!(matches!(
            ObjectStoreLayout::candidate_head_key("doc", "..", &hash),
            Err(StoreError::InvalidPath)
        ));
        assert!(matches!(
            ObjectStoreLayout::uuid_lookup_key("../doc"),
            Err(StoreError::InvalidPath)
        ));
        assert!(matches!(
            ObjectStoreLayout::uuid_lookup_key(".."),
            Err(StoreError::InvalidPath)
        ));
        assert!(matches!(
            ObjectStoreLayout::doi_lookup_key(" "),
            Err(StoreError::InvalidPath)
        ));
    }

    #[test]
    fn local_store_satisfies_reusable_object_store_contract() {
        let root =
            std::env::temp_dir().join(format!("opendoc-store-conformance-{}", process_tag()));
        let _ = fs::remove_dir_all(&root);
        let store = LocalObjectStore::new(&root);
        verify_object_store_contract(&store, "local").unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn flat_store_satisfies_reusable_object_store_contract() {
        let root =
            std::env::temp_dir().join(format!("opendoc-flat-store-conformance-{}", process_tag()));
        let _ = fs::remove_dir_all(&root);
        let store = FlatObjectStore::new(&root, "/bucket/prefix/").unwrap();
        assert_eq!(store.namespace(), "bucket/prefix");
        assert_eq!(
            store.capabilities(),
            StoreCapabilities {
                idempotent_content_put: true,
                compare_and_swap_head: true,
                list_prefix: true,
                atomic_named_overwrite: true,
                local_pack_files: true,
            }
        );
        verify_object_store_contract(&store, "flat").unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(feature = "opendal")]
    #[test]
    fn opendal_fs_store_satisfies_reusable_object_store_contract() {
        let root = std::env::temp_dir().join(format!(
            "opendoc-opendal-fs-store-conformance-{}",
            process_tag()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let store = OpenDalObjectStore::from_fs_root(&root, "/bucket/prefix/").unwrap();
        assert_eq!(store.namespace(), "bucket/prefix");
        assert_eq!(
            store.capabilities(),
            StoreCapabilities {
                idempotent_content_put: true,
                compare_and_swap_head: false,
                list_prefix: true,
                atomic_named_overwrite: false,
                local_pack_files: false,
            }
        );
        verify_object_store_contract(&store, "opendal").unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn flat_store_uses_backend_neutral_namespaced_key_layout() {
        let root =
            std::env::temp_dir().join(format!("opendoc-flat-store-layout-{}", process_tag()));
        let _ = fs::remove_dir_all(&root);
        let store = FlatObjectStore::new(&root, "bucket/prefix").unwrap();
        let bytes = b"s3-shaped flat object";
        let hash = digest_bytes("sha256", bytes).unwrap();

        assert!(store.put_if_absent(&hash, bytes).unwrap());
        assert!(root
            .join("bucket/prefix")
            .join(ObjectStoreLayout::object_key(&hash))
            .exists());
        store
            .put_named("documents/doc/metadata.bin", b"named")
            .unwrap();
        assert_eq!(
            fs::read(root.join("bucket/prefix/documents/doc/metadata.bin")).unwrap(),
            b"named"
        );
        assert_eq!(
            store.list_prefix("documents").unwrap(),
            vec!["doc/metadata.bin".to_string()]
        );
        assert!(matches!(
            FlatObjectStore::new(&root, "../bad"),
            Err(StoreError::InvalidPath)
        ));
        assert!(matches!(
            FlatObjectStore::new(&root, "bucket//bad"),
            Err(StoreError::InvalidPath)
        ));
        assert!(matches!(
            store.put_if_absent(&hash, b"wrong bytes"),
            Err(StoreError::HashMismatch)
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn local_store_rejects_hash_mismatched_loose_object_writes_and_reads() {
        let root = std::env::temp_dir().join(format!(
            "opendoc-local-store-loose-integrity-{}",
            process_tag()
        ));
        let _ = fs::remove_dir_all(&root);
        let store = LocalObjectStore::new(&root);
        let bytes = b"local loose integrity";
        let hash = digest_bytes("sha256", bytes).unwrap();

        assert!(matches!(
            store.put_if_absent(&hash, b"wrong bytes"),
            Err(StoreError::HashMismatch)
        ));
        assert!(store.put_if_absent(&hash, bytes).unwrap());
        fs::write(store.object_path(&hash), b"tampered loose bytes").unwrap();

        assert!(matches!(store.get(&hash), Err(StoreError::HashMismatch)));
        assert!(matches!(store.exists(&hash), Err(StoreError::HashMismatch)));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn flat_store_rejects_corrupt_content_addressed_object_reads() {
        let root = std::env::temp_dir().join(format!(
            "opendoc-flat-store-object-integrity-{}",
            process_tag()
        ));
        let _ = fs::remove_dir_all(&root);
        let store = FlatObjectStore::new(&root, "bucket/prefix").unwrap();
        let bytes = b"flat object integrity";
        let hash = digest_bytes("sha256", bytes).unwrap();

        assert!(store.put_if_absent(&hash, bytes).unwrap());
        fs::write(
            store
                .key_path(&ObjectStoreLayout::object_key(&hash))
                .unwrap(),
            b"tampered flat object",
        )
        .unwrap();

        assert!(matches!(store.get(&hash), Err(StoreError::HashMismatch)));
        assert!(matches!(store.exists(&hash), Err(StoreError::HashMismatch)));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn stores_reject_corrupt_branch_head_records() {
        let local_root =
            std::env::temp_dir().join(format!("opendoc-store-corrupt-head-{}", process_tag()));
        let flat_root =
            std::env::temp_dir().join(format!("opendoc-flat-store-corrupt-head-{}", process_tag()));
        let _ = fs::remove_dir_all(&local_root);
        let _ = fs::remove_dir_all(&flat_root);

        let local = LocalObjectStore::new(&local_root);
        let local_head = local.head_path("doc-corrupt", "main");
        fs::create_dir_all(local_head.parent().unwrap()).unwrap();
        fs::write(&local_head, b"not-a-hash").unwrap();
        assert!(matches!(
            local.read_head("doc-corrupt", "main"),
            Err(StoreError::CorruptHead)
        ));
        let padded = HashRef::parse("sha256:abc123").unwrap();
        fs::write(&local_head, format!(" {padded} ")).unwrap();
        assert!(matches!(
            local.read_head("doc-corrupt", "main"),
            Err(StoreError::CorruptHead)
        ));

        let flat = FlatObjectStore::new(&flat_root, "bucket/prefix").unwrap();
        flat.put_named(
            &ObjectStoreLayout::head_key("doc-corrupt", "main").unwrap(),
            b"not-a-hash",
        )
        .unwrap();
        assert!(matches!(
            flat.read_head("doc-corrupt", "main"),
            Err(StoreError::CorruptHead)
        ));
        flat.put_named(
            &ObjectStoreLayout::head_key("doc-corrupt", "main").unwrap(),
            format!("\n{padded}").as_bytes(),
        )
        .unwrap();
        assert!(matches!(
            flat.read_head("doc-corrupt", "main"),
            Err(StoreError::CorruptHead)
        ));

        let _ = fs::remove_dir_all(local_root);
        let _ = fs::remove_dir_all(flat_root);
    }

    #[test]
    fn local_store_reads_objects_from_pack_after_loose_files_are_removed() {
        let root = std::env::temp_dir().join(format!("opendoc-store-pack-{}", process_tag()));
        let _ = fs::remove_dir_all(&root);
        let store = LocalObjectStore::new(&root);
        let first = digest_bytes("sha256", b"first packed object").unwrap();
        let second = digest_bytes("sha256", b"second packed object").unwrap();
        assert!(store.put_if_absent(&first, b"first packed object").unwrap());
        assert!(store
            .put_if_absent(&second, b"second packed object")
            .unwrap());

        let stats = store.compact_loose_objects_to_pack("main-pack").unwrap();
        assert_eq!(stats.pack, "main-pack");
        assert_eq!(stats.objects, 2);
        assert!(stats.bytes > 4);
        let index_bytes = fs::read(root.join("packs/main-pack.idx")).unwrap();
        assert_eq!(index_bytes.get(0..4), Some(&b"ODF0"[..]));
        assert_eq!(decode_pack_index(&index_bytes).unwrap().len(), 2);

        assert!(!store.object_path(&first).exists());
        assert!(!store.object_path(&second).exists());
        assert!(store.exists(&first).unwrap());
        assert_eq!(
            store.get(&first).unwrap(),
            Some(b"first packed object".to_vec())
        );
        assert_eq!(
            store.get(&second).unwrap(),
            Some(b"second packed object".to_vec())
        );
        assert!(!store.put_if_absent(&first, b"first packed object").unwrap());
        assert!(matches!(
            store.compact_loose_objects_to_pack("../bad"),
            Err(StoreError::InvalidPath)
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn flat_store_reads_namespaced_objects_from_pack_after_loose_files_are_removed() {
        let root = std::env::temp_dir().join(format!("opendoc-flat-store-pack-{}", process_tag()));
        let _ = fs::remove_dir_all(&root);
        let store = FlatObjectStore::new(&root, "bucket/prefix").unwrap();
        assert!(store.capabilities().local_pack_files);
        let first = digest_bytes("sha256", b"first flat packed object").unwrap();
        let second = digest_bytes("sha256", b"second flat packed object").unwrap();
        assert!(store
            .put_if_absent(&first, b"first flat packed object")
            .unwrap());
        assert!(store
            .put_if_absent(&second, b"second flat packed object")
            .unwrap());

        let stats = store.compact_loose_objects_to_pack("flat-pack").unwrap();

        assert_eq!(stats.pack, "flat-pack");
        assert_eq!(stats.objects, 2);
        assert!(stats.bytes > 4);
        let index_path = root.join("bucket/prefix/packs/flat-pack.idx");
        let index_bytes = fs::read(index_path).unwrap();
        assert_eq!(index_bytes.get(0..4), Some(&b"ODF0"[..]));
        assert_eq!(decode_pack_index(&index_bytes).unwrap().len(), 2);
        assert!(!store
            .key_path(&ObjectStoreLayout::object_key(&first))
            .unwrap()
            .exists());
        assert!(!store
            .key_path(&ObjectStoreLayout::object_key(&second))
            .unwrap()
            .exists());
        assert!(store.exists(&first).unwrap());
        assert_eq!(
            store.get(&first).unwrap(),
            Some(b"first flat packed object".to_vec())
        );
        assert_eq!(
            store.get(&second).unwrap(),
            Some(b"second flat packed object".to_vec())
        );
        assert!(!store
            .put_if_absent(&first, b"first flat packed object")
            .unwrap());
        assert!(matches!(
            store.compact_loose_objects_to_pack("../bad"),
            Err(StoreError::InvalidPath)
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn local_pack_recompaction_preserves_existing_packed_objects() {
        let root = std::env::temp_dir().join(format!(
            "opendoc-store-pack-rewrite-preserve-{}",
            process_tag()
        ));
        let _ = fs::remove_dir_all(&root);
        let store = LocalObjectStore::new(&root);
        let first = digest_bytes("sha256", b"first packed object").unwrap();
        store.put_if_absent(&first, b"first packed object").unwrap();
        let first_stats = store.compact_loose_objects_to_pack("main-pack").unwrap();
        assert_eq!(first_stats.objects, 1);
        assert!(!store.object_path(&first).exists());

        let second = digest_bytes("sha256", b"second packed object").unwrap();
        store
            .put_if_absent(&second, b"second packed object")
            .unwrap();
        let second_stats = store.compact_loose_objects_to_pack("main-pack").unwrap();

        assert_eq!(second_stats.objects, 2);
        assert!(!store.object_path(&second).exists());
        assert_eq!(
            store.get(&first).unwrap(),
            Some(b"first packed object".to_vec())
        );
        assert_eq!(
            store.get(&second).unwrap(),
            Some(b"second packed object".to_vec())
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn local_pack_write_recovers_from_stale_temp_files() {
        let root =
            std::env::temp_dir().join(format!("opendoc-store-pack-stale-temp-{}", process_tag()));
        let _ = fs::remove_dir_all(&root);
        let store = LocalObjectStore::new(&root);
        let hash = digest_bytes("sha256", b"recoverable packed object").unwrap();
        store
            .put_if_absent(&hash, b"recoverable packed object")
            .unwrap();
        let pack_dir = root.join("packs");
        fs::create_dir_all(&pack_dir).unwrap();
        let stale_pack = pack_dir.join(format!("main-pack.pack.tmp-{}", process_tag()));
        let stale_index = pack_dir.join(format!("main-pack.idx.tmp-{}", process_tag()));
        fs::write(&stale_pack, b"interrupted pack write").unwrap();
        fs::write(&stale_index, b"interrupted index write").unwrap();

        let stats = store.compact_loose_objects_to_pack("main-pack").unwrap();

        assert_eq!(stats.objects, 1);
        assert!(!stale_pack.exists());
        assert!(!stale_index.exists());
        assert_eq!(
            store.get(&hash).unwrap(),
            Some(b"recoverable packed object".to_vec())
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn local_pack_write_validates_hashes_before_creating_temp_files() {
        let root = std::env::temp_dir().join(format!(
            "opendoc-store-pack-bad-hash-no-temp-{}",
            process_tag()
        ));
        let _ = fs::remove_dir_all(&root);
        let store = LocalObjectStore::new(&root);
        let hash = digest_bytes("sha256", b"expected packed object").unwrap();

        assert!(matches!(
            store.write_pack("main-pack", &[(hash, b"different bytes".to_vec())]),
            Err(StoreError::HashMismatch)
        ));

        let pack_dir = root.join("packs");
        assert!(pack_dir.exists());
        assert!(fs::read_dir(&pack_dir).unwrap().next().is_none());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn local_store_rejects_corrupt_packed_object_bytes() {
        let root =
            std::env::temp_dir().join(format!("opendoc-store-corrupt-pack-{}", process_tag()));
        let _ = fs::remove_dir_all(&root);
        let store = LocalObjectStore::new(&root);
        let hash = digest_bytes("sha256", b"packed integrity object").unwrap();
        assert!(store
            .put_if_absent(&hash, b"packed integrity object")
            .unwrap());

        let stats = store
            .compact_loose_objects_to_pack("integrity-pack")
            .unwrap();
        assert_eq!(stats.objects, 1);
        assert!(!store.object_path(&hash).exists());

        let index = fs::read(root.join("packs/integrity-pack.idx")).unwrap();
        let entries = decode_pack_index(&index).unwrap();
        let entry = entries
            .iter()
            .find(|entry| entry.hash == hash)
            .expect("packed object is indexed");
        let mut pack = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(root.join("packs/integrity-pack.pack"))
            .unwrap();
        pack.seek(SeekFrom::Start(entry.offset)).unwrap();
        pack.write_all(b"X").unwrap();
        pack.sync_all().unwrap();

        assert!(matches!(store.get(&hash), Err(StoreError::HashMismatch)));
        assert!(matches!(store.exists(&hash), Err(StoreError::HashMismatch)));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn local_store_rejects_corrupt_binary_pack_index() {
        let root = std::env::temp_dir().join(format!(
            "opendoc-store-corrupt-pack-index-{}",
            process_tag()
        ));
        let _ = fs::remove_dir_all(&root);
        let store = LocalObjectStore::new(&root);
        let hash = digest_bytes("sha256", b"packed index object").unwrap();
        assert!(store.put_if_absent(&hash, b"packed index object").unwrap());

        let stats = store.compact_loose_objects_to_pack("index-pack").unwrap();
        assert_eq!(stats.objects, 1);
        assert!(!store.object_path(&hash).exists());
        fs::write(
            root.join("packs/index-pack.idx"),
            b"not a binary pack index",
        )
        .unwrap();

        assert!(matches!(
            store.get(&hash),
            Err(StoreError::Format(message)) if message.contains("InvalidMagic")
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn local_store_rejects_pack_index_that_targets_different_pack() {
        let root = std::env::temp_dir().join(format!(
            "opendoc-store-wrong-pack-index-target-{}",
            process_tag()
        ));
        let _ = fs::remove_dir_all(&root);
        let store = LocalObjectStore::new(&root);
        let hash = digest_bytes("sha256", b"packed wrong target object").unwrap();
        assert!(store
            .put_if_absent(&hash, b"packed wrong target object")
            .unwrap());

        let stats = store.compact_loose_objects_to_pack("main-pack").unwrap();
        assert_eq!(stats.objects, 1);
        assert!(!store.object_path(&hash).exists());
        let index_path = root.join("packs/main-pack.idx");
        let entries = decode_pack_index(&fs::read(&index_path).unwrap()).unwrap();
        fs::write(&index_path, encode_pack_index("other-pack", &entries)).unwrap();

        assert!(matches!(
            store.get(&hash),
            Err(StoreError::Format(message))
                if message.contains("pack index file targets unexpected pack other-pack")
        ));
        fs::write(
            &index_path,
            encode_record(&PackIndexRecord {
                pack: "other.pack".to_string(),
                entries: entries
                    .iter()
                    .map(|entry| PackIndexEntryRecord {
                        hash: entry.hash.clone(),
                        offset: entry.offset,
                        length: entry.length,
                    })
                    .collect(),
            }),
        )
        .unwrap();
        assert!(matches!(
            store.get(&hash),
            Err(StoreError::Format(message))
                if message.contains("pack index pack is not a pack name")
        ));
        let mut invalid_offset_entries = entries.clone();
        invalid_offset_entries[0].offset = 0;
        fs::write(
            &index_path,
            encode_pack_index("main-pack", &invalid_offset_entries),
        )
        .unwrap();
        assert!(matches!(
            store.get(&hash),
            Err(StoreError::Format(message))
                if message.contains("pack index entry offset is before pack payload")
        ));
        let mut overflowing_entries = entries.clone();
        overflowing_entries[0].offset = u64::MAX;
        overflowing_entries[0].length = 1;
        fs::write(
            &index_path,
            encode_pack_index("main-pack", &overflowing_entries),
        )
        .unwrap();
        assert!(matches!(
            store.get(&hash),
            Err(StoreError::Format(message))
                if message.contains("byte range overflows")
        ));
        let mut oversized_entries = entries.clone();
        oversized_entries[0].length = 1_000_000;
        fs::write(
            &index_path,
            encode_pack_index("main-pack", &oversized_entries),
        )
        .unwrap();
        assert!(matches!(
            store.get(&hash),
            Err(StoreError::Format(message))
                if message.contains("pack index entry range exceeds pack size")
        ));
        let mut duplicate_entries = entries.clone();
        duplicate_entries.push(entries[0].clone());
        fs::write(
            &index_path,
            encode_pack_index("main-pack", &duplicate_entries),
        )
        .unwrap();
        assert!(matches!(
            store.get(&hash),
            Err(StoreError::Format(message))
                if message.contains("duplicate pack index entry hash")
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn repository_commits_and_reads_manifest() {
        let root = std::env::temp_dir().join(format!("opendoc-repo-{}", process_tag()));
        let _ = fs::remove_dir_all(&root);
        let repo = Repository::new(LocalObjectStore::new(&root));
        let manifest = ManifestRecord {
            document_uuid: "doc".to_string(),
            branch: "main".to_string(),
            parent: None,
            snapshot: HashRef::parse("sha256:aaa").unwrap(),
            operation_segments: Vec::new(),
            signatures: Vec::new(),
            blobs: Vec::new(),
            created_at_ms: 1,
        };
        let hash = repo.commit_manifest(&manifest, None).unwrap().unwrap();
        assert_eq!(repo.read_manifest(&hash).unwrap(), Some(manifest));
        assert_eq!(repo.store().read_head("doc", "main").unwrap(), Some(hash));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn repository_rejects_corrupt_manifest_object_bytes() {
        let root =
            std::env::temp_dir().join(format!("opendoc-repo-corrupt-manifest-{}", process_tag()));
        let _ = fs::remove_dir_all(&root);
        let store = LocalObjectStore::new(&root);
        let repo = Repository::new(store.clone());
        let manifest = ManifestRecord {
            document_uuid: "doc-corrupt-manifest".to_string(),
            branch: "main".to_string(),
            parent: None,
            snapshot: HashRef::parse("sha256:aaa").unwrap(),
            operation_segments: Vec::new(),
            signatures: Vec::new(),
            blobs: Vec::new(),
            created_at_ms: 1,
        };
        let hash = repo.commit_manifest(&manifest, None).unwrap().unwrap();
        fs::write(store.object_path(&hash), b"corrupted manifest bytes").unwrap();

        assert!(matches!(
            repo.read_manifest(&hash),
            Err(StoreError::HashMismatch)
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn repository_commits_and_plans_candidates_on_flat_store() {
        let root = std::env::temp_dir().join(format!("opendoc-flat-repo-{}", process_tag()));
        let _ = fs::remove_dir_all(&root);
        let repo = Repository::new(FlatObjectStore::new(&root, "bucket/repo").unwrap());
        let base = ManifestRecord {
            document_uuid: "doc-flat".to_string(),
            branch: "main".to_string(),
            parent: None,
            snapshot: HashRef::parse("sha256:aaa").unwrap(),
            operation_segments: Vec::new(),
            signatures: Vec::new(),
            blobs: Vec::new(),
            created_at_ms: 1,
        };
        let base_hash = repo.commit_manifest(&base, None).unwrap().unwrap();
        let current = ManifestRecord {
            document_uuid: "doc-flat".to_string(),
            branch: "main".to_string(),
            parent: Some(base_hash.clone()),
            snapshot: HashRef::parse("sha256:bbb").unwrap(),
            operation_segments: Vec::new(),
            signatures: Vec::new(),
            blobs: Vec::new(),
            created_at_ms: 2,
        };
        let current_hash = repo
            .commit_manifest(&current, Some(&base_hash))
            .unwrap()
            .unwrap();
        let candidate = ManifestRecord {
            document_uuid: "doc-flat".to_string(),
            branch: "main".to_string(),
            parent: Some(base_hash.clone()),
            snapshot: HashRef::parse("sha256:ccc").unwrap(),
            operation_segments: Vec::new(),
            signatures: Vec::new(),
            blobs: Vec::new(),
            created_at_ms: 3,
        };
        let candidate_hash = match repo
            .commit_manifest_or_candidate(&candidate, Some(&base_hash))
            .unwrap()
        {
            CommitOutcome::Candidate { manifest, .. } => manifest,
            other => panic!("expected stale flat-store commit to become candidate, got {other:?}"),
        };

        assert_eq!(
            repo.store().read_head("doc-flat", "main").unwrap(),
            Some(current_hash.clone())
        );
        assert_eq!(
            repo.read_manifest(&candidate_hash).unwrap(),
            Some(candidate)
        );
        let plans = repo.plan_candidate_merges("doc-flat", "main").unwrap();
        assert_eq!(
            plans.plans,
            vec![CandidateMergePlan {
                current: Some(current_hash.clone()),
                candidate: candidate_hash.clone(),
                merge_base: Some(base_hash),
                current_since_base: vec![current_hash],
                candidate_since_base: vec![candidate_hash],
            }]
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn repository_can_write_candidate_head_when_cas_fails() {
        let root = std::env::temp_dir().join(format!("opendoc-repo-candidate-{}", process_tag()));
        let _ = fs::remove_dir_all(&root);
        let repo = Repository::new(LocalObjectStore::new(&root));
        let first = ManifestRecord {
            document_uuid: "doc-candidate".to_string(),
            branch: "main".to_string(),
            parent: None,
            snapshot: HashRef::parse("sha256:aaa").unwrap(),
            operation_segments: Vec::new(),
            signatures: Vec::new(),
            blobs: Vec::new(),
            created_at_ms: 1,
        };
        let second = ManifestRecord {
            document_uuid: "doc-candidate".to_string(),
            branch: "main".to_string(),
            parent: None,
            snapshot: HashRef::parse("sha256:bbb").unwrap(),
            operation_segments: Vec::new(),
            signatures: Vec::new(),
            blobs: Vec::new(),
            created_at_ms: 2,
        };

        let first_hash = match repo.commit_manifest_or_candidate(&first, None).unwrap() {
            CommitOutcome::Committed(hash) => hash,
            other => panic!("expected committed first manifest, got {other:?}"),
        };
        let outcome = repo.commit_manifest_or_candidate(&second, None).unwrap();
        let (second_hash, path) = match outcome {
            CommitOutcome::Candidate { manifest, path } => (manifest, path),
            other => panic!("expected candidate second manifest, got {other:?}"),
        };

        assert_eq!(
            repo.store().read_head("doc-candidate", "main").unwrap(),
            Some(first_hash)
        );
        assert!(path.contains("/head-candidates/main/sha256/"));
        let candidates = repo.list_candidate_heads("doc-candidate", "main").unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].manifest, second_hash);
        assert_eq!(repo.read_manifest(&second_hash).unwrap(), Some(second));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn repository_resolves_candidate_heads_deterministically() {
        let root =
            std::env::temp_dir().join(format!("opendoc-repo-resolve-candidates-{}", process_tag()));
        let _ = fs::remove_dir_all(&root);
        let repo = Repository::new(LocalObjectStore::new(&root));
        let base = ManifestRecord {
            document_uuid: "doc-resolve".to_string(),
            branch: "main".to_string(),
            parent: None,
            snapshot: HashRef::parse("sha256:aaa").unwrap(),
            operation_segments: Vec::new(),
            signatures: Vec::new(),
            blobs: Vec::new(),
            created_at_ms: 1,
        };
        let base_hash = repo.commit_manifest(&base, None).unwrap().unwrap();
        let fast_forward = ManifestRecord {
            document_uuid: "doc-resolve".to_string(),
            branch: "main".to_string(),
            parent: Some(base_hash.clone()),
            snapshot: HashRef::parse("sha256:bbb").unwrap(),
            operation_segments: Vec::new(),
            signatures: Vec::new(),
            blobs: Vec::new(),
            created_at_ms: 2,
        };
        let divergent = ManifestRecord {
            document_uuid: "doc-resolve".to_string(),
            branch: "main".to_string(),
            parent: None,
            snapshot: HashRef::parse("sha256:ccc").unwrap(),
            operation_segments: Vec::new(),
            signatures: Vec::new(),
            blobs: Vec::new(),
            created_at_ms: 3,
        };
        let fast_forward_hash = repo.write_manifest(&fast_forward).unwrap();
        let divergent_hash = repo.write_manifest(&divergent).unwrap();
        for manifest in [
            fast_forward_hash.clone(),
            divergent_hash.clone(),
            base_hash.clone(),
        ] {
            repo.write_candidate_head(&BranchHeadRecord {
                document_uuid: "doc-resolve".to_string(),
                branch: "main".to_string(),
                manifest,
            })
            .unwrap();
        }
        let missing_hash = HashRef::parse("sha256:dddddd").unwrap();
        repo.write_candidate_head(&BranchHeadRecord {
            document_uuid: "doc-resolve".to_string(),
            branch: "main".to_string(),
            manifest: missing_hash.clone(),
        })
        .unwrap();

        let resolution = repo.resolve_candidate_heads("doc-resolve", "main").unwrap();
        assert_eq!(resolution.current, Some(base_hash.clone()));
        assert_eq!(
            resolution
                .candidates
                .iter()
                .map(|candidate| (&candidate.manifest, candidate.status))
                .collect::<Vec<_>>(),
            vec![
                (&fast_forward_hash, CandidateStatus::FastForward),
                (&divergent_hash, CandidateStatus::NeedsMerge),
                (&base_hash, CandidateStatus::AlreadyCurrent),
                (&missing_hash, CandidateStatus::MissingManifest),
            ]
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn repository_resolves_candidate_heads_while_reporting_invalid_records() {
        let root =
            std::env::temp_dir().join(format!("opendoc-repo-invalid-candidates-{}", process_tag()));
        let _ = fs::remove_dir_all(&root);
        let repo = Repository::new(LocalObjectStore::new(&root));
        let base = ManifestRecord {
            document_uuid: "doc-invalid-candidates".to_string(),
            branch: "main".to_string(),
            parent: None,
            snapshot: HashRef::parse("sha256:aaa").unwrap(),
            operation_segments: Vec::new(),
            signatures: Vec::new(),
            blobs: Vec::new(),
            created_at_ms: 1,
        };
        let base_hash = repo.commit_manifest(&base, None).unwrap().unwrap();
        let fast_forward = ManifestRecord {
            document_uuid: "doc-invalid-candidates".to_string(),
            branch: "main".to_string(),
            parent: Some(base_hash.clone()),
            snapshot: HashRef::parse("sha256:bbb").unwrap(),
            operation_segments: Vec::new(),
            signatures: Vec::new(),
            blobs: Vec::new(),
            created_at_ms: 2,
        };
        let fast_forward_hash = repo.write_manifest(&fast_forward).unwrap();
        repo.write_candidate_head(&BranchHeadRecord {
            document_uuid: "doc-invalid-candidates".to_string(),
            branch: "main".to_string(),
            manifest: fast_forward_hash.clone(),
        })
        .unwrap();

        let malformed_hash = HashRef::parse("sha256:cccccc").unwrap();
        repo.store()
            .put_named(
                &ObjectStoreLayout::candidate_head_key(
                    "doc-invalid-candidates",
                    "main",
                    &malformed_hash,
                )
                .unwrap(),
                b"not a branch head record",
            )
            .unwrap();
        let misplaced_hash = HashRef::parse("sha256:dddddd").unwrap();
        repo.store()
            .put_named(
                &ObjectStoreLayout::candidate_head_key(
                    "doc-invalid-candidates",
                    "main",
                    &misplaced_hash,
                )
                .unwrap(),
                &encode_record(&BranchHeadRecord {
                    document_uuid: "other-doc".to_string(),
                    branch: "main".to_string(),
                    manifest: misplaced_hash,
                }),
            )
            .unwrap();
        let path_hash = HashRef::parse("sha256:eeeeee").unwrap();
        let record_hash = HashRef::parse("sha256:ffffff").unwrap();
        repo.store()
            .put_named(
                &ObjectStoreLayout::candidate_head_key(
                    "doc-invalid-candidates",
                    "main",
                    &path_hash,
                )
                .unwrap(),
                &encode_record(&BranchHeadRecord {
                    document_uuid: "doc-invalid-candidates".to_string(),
                    branch: "main".to_string(),
                    manifest: record_hash,
                }),
            )
            .unwrap();

        let resolution = repo
            .resolve_candidate_heads("doc-invalid-candidates", "main")
            .unwrap();
        assert_eq!(resolution.current, Some(base_hash));
        assert_eq!(resolution.candidates.len(), 1);
        assert_eq!(resolution.candidates[0].manifest, fast_forward_hash);
        assert_eq!(
            resolution.candidates[0].status,
            CandidateStatus::FastForward
        );
        assert_eq!(resolution.invalid_candidates.len(), 3);
        assert!(resolution
            .invalid_candidates
            .iter()
            .any(|candidate| candidate.reason.contains("InvalidMagic")));
        assert!(resolution
            .invalid_candidates
            .iter()
            .any(|candidate| candidate.reason.contains("other-doc:main")));
        assert!(resolution
            .invalid_candidates
            .iter()
            .any(|candidate| candidate.reason.contains(
                "candidate head path targets sha256:eeeeee but record targets sha256:ffffff"
            )));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn repository_lists_valid_candidate_heads_while_ignoring_invalid_records() {
        let root = std::env::temp_dir().join(format!(
            "opendoc-repo-list-valid-candidates-{}",
            process_tag()
        ));
        let _ = fs::remove_dir_all(&root);
        let repo = Repository::new(LocalObjectStore::new(&root));
        let valid_manifest = HashRef::parse("sha256:abc123").unwrap();
        repo.write_candidate_head(&BranchHeadRecord {
            document_uuid: "doc-list-candidates".to_string(),
            branch: "main".to_string(),
            manifest: valid_manifest.clone(),
        })
        .unwrap();
        let invalid_manifest = HashRef::parse("sha256:def456").unwrap();
        repo.store()
            .put_named(
                &ObjectStoreLayout::candidate_head_key(
                    "doc-list-candidates",
                    "main",
                    &invalid_manifest,
                )
                .unwrap(),
                b"not a branch head record",
            )
            .unwrap();

        let listed = repo
            .list_candidate_heads("doc-list-candidates", "main")
            .unwrap();
        assert_eq!(
            listed,
            vec![BranchHeadRecord {
                document_uuid: "doc-list-candidates".to_string(),
                branch: "main".to_string(),
                manifest: valid_manifest,
            }]
        );
        let resolution = repo
            .resolve_candidate_heads("doc-list-candidates", "main")
            .unwrap();
        assert_eq!(resolution.invalid_candidates.len(), 1);
        assert!(resolution.invalid_candidates[0]
            .reason
            .contains("InvalidMagic"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn repository_fast_forwards_candidate_heads_with_cas() {
        let root = std::env::temp_dir().join(format!(
            "opendoc-repo-fast-forward-candidates-{}",
            process_tag()
        ));
        let _ = fs::remove_dir_all(&root);
        let repo = Repository::new(LocalObjectStore::new(&root));
        let base = ManifestRecord {
            document_uuid: "doc-advance".to_string(),
            branch: "main".to_string(),
            parent: None,
            snapshot: HashRef::parse("sha256:aaa").unwrap(),
            operation_segments: Vec::new(),
            signatures: Vec::new(),
            blobs: Vec::new(),
            created_at_ms: 1,
        };
        let base_hash = repo.commit_manifest(&base, None).unwrap().unwrap();
        let fast_forward = ManifestRecord {
            document_uuid: "doc-advance".to_string(),
            branch: "main".to_string(),
            parent: Some(base_hash.clone()),
            snapshot: HashRef::parse("sha256:bbb").unwrap(),
            operation_segments: Vec::new(),
            signatures: Vec::new(),
            blobs: Vec::new(),
            created_at_ms: 2,
        };
        let divergent = ManifestRecord {
            document_uuid: "doc-advance".to_string(),
            branch: "main".to_string(),
            parent: None,
            snapshot: HashRef::parse("sha256:ccc").unwrap(),
            operation_segments: Vec::new(),
            signatures: Vec::new(),
            blobs: Vec::new(),
            created_at_ms: 3,
        };
        let fast_forward_hash = repo.write_manifest(&fast_forward).unwrap();
        let divergent_hash = repo.write_manifest(&divergent).unwrap();
        for manifest in [fast_forward_hash.clone(), divergent_hash.clone()] {
            repo.write_candidate_head(&BranchHeadRecord {
                document_uuid: "doc-advance".to_string(),
                branch: "main".to_string(),
                manifest,
            })
            .unwrap();
        }

        assert_eq!(
            repo.try_fast_forward_candidate("doc-advance", "main", &fast_forward_hash)
                .unwrap(),
            CandidateAdvance::Advanced(fast_forward_hash.clone())
        );
        assert_eq!(
            repo.store().read_head("doc-advance", "main").unwrap(),
            Some(fast_forward_hash.clone())
        );
        assert_eq!(
            repo.try_fast_forward_candidate("doc-advance", "main", &fast_forward_hash)
                .unwrap(),
            CandidateAdvance::AlreadyCurrent(fast_forward_hash)
        );
        assert_eq!(
            repo.try_fast_forward_candidate("doc-advance", "main", &divergent_hash)
                .unwrap(),
            CandidateAdvance::NeedsMerge(divergent_hash)
        );
        let missing_hash = HashRef::parse("sha256:dddddd").unwrap();
        assert_eq!(
            repo.try_fast_forward_candidate("doc-advance", "main", &missing_hash)
                .unwrap(),
            CandidateAdvance::MissingManifest(missing_hash)
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn repository_reconciles_fast_forward_candidate_chain() {
        let root = std::env::temp_dir().join(format!(
            "opendoc-repo-reconcile-candidates-{}",
            process_tag()
        ));
        let _ = fs::remove_dir_all(&root);
        let repo = Repository::new(LocalObjectStore::new(&root));
        let base = ManifestRecord {
            document_uuid: "doc-reconcile".to_string(),
            branch: "main".to_string(),
            parent: None,
            snapshot: HashRef::parse("sha256:aaa").unwrap(),
            operation_segments: Vec::new(),
            signatures: Vec::new(),
            blobs: Vec::new(),
            created_at_ms: 1,
        };
        let base_hash = repo.commit_manifest(&base, None).unwrap().unwrap();
        let first = ManifestRecord {
            document_uuid: "doc-reconcile".to_string(),
            branch: "main".to_string(),
            parent: Some(base_hash.clone()),
            snapshot: HashRef::parse("sha256:bbb").unwrap(),
            operation_segments: Vec::new(),
            signatures: Vec::new(),
            blobs: Vec::new(),
            created_at_ms: 2,
        };
        let first_hash = repo.write_manifest(&first).unwrap();
        let second = ManifestRecord {
            document_uuid: "doc-reconcile".to_string(),
            branch: "main".to_string(),
            parent: Some(first_hash.clone()),
            snapshot: HashRef::parse("sha256:ccc").unwrap(),
            operation_segments: Vec::new(),
            signatures: Vec::new(),
            blobs: Vec::new(),
            created_at_ms: 3,
        };
        let divergent = ManifestRecord {
            document_uuid: "doc-reconcile".to_string(),
            branch: "main".to_string(),
            parent: None,
            snapshot: HashRef::parse("sha256:ddd").unwrap(),
            operation_segments: Vec::new(),
            signatures: Vec::new(),
            blobs: Vec::new(),
            created_at_ms: 4,
        };
        let second_hash = repo.write_manifest(&second).unwrap();
        let divergent_hash = repo.write_manifest(&divergent).unwrap();
        for manifest in [
            first_hash.clone(),
            second_hash.clone(),
            divergent_hash.clone(),
        ] {
            repo.write_candidate_head(&BranchHeadRecord {
                document_uuid: "doc-reconcile".to_string(),
                branch: "main".to_string(),
                manifest,
            })
            .unwrap();
        }

        let reconciliation = repo
            .reconcile_candidate_heads("doc-reconcile", "main")
            .unwrap();
        assert_eq!(reconciliation.initial.current, Some(base_hash));
        assert_eq!(
            reconciliation.advanced,
            vec![first_hash.clone(), second_hash.clone()]
        );
        assert_eq!(
            repo.store().read_head("doc-reconcile", "main").unwrap(),
            Some(second_hash.clone())
        );
        let statuses = reconciliation
            .final_resolution
            .candidates
            .iter()
            .map(|candidate| (&candidate.manifest, candidate.status))
            .collect::<Vec<_>>();
        assert!(statuses.contains(&(&first_hash, CandidateStatus::IntegratedAncestor)));
        assert!(statuses.contains(&(&second_hash, CandidateStatus::AlreadyCurrent)));
        assert!(statuses.contains(&(&divergent_hash, CandidateStatus::NeedsMerge)));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn flat_repository_reconciles_fast_forward_candidate_chain() {
        let root = std::env::temp_dir().join(format!(
            "opendoc-flat-reconcile-candidates-{}",
            process_tag()
        ));
        let _ = fs::remove_dir_all(&root);
        let repo = Repository::new(FlatObjectStore::new(&root, "bucket/repo").unwrap());
        let base = ManifestRecord {
            document_uuid: "doc-flat-reconcile".to_string(),
            branch: "main".to_string(),
            parent: None,
            snapshot: HashRef::parse("sha256:aaa").unwrap(),
            operation_segments: Vec::new(),
            signatures: Vec::new(),
            blobs: Vec::new(),
            created_at_ms: 1,
        };
        let base_hash = repo.commit_manifest(&base, None).unwrap().unwrap();
        let first = ManifestRecord {
            document_uuid: "doc-flat-reconcile".to_string(),
            branch: "main".to_string(),
            parent: Some(base_hash.clone()),
            snapshot: HashRef::parse("sha256:bbb").unwrap(),
            operation_segments: Vec::new(),
            signatures: Vec::new(),
            blobs: Vec::new(),
            created_at_ms: 2,
        };
        let first_hash = repo.write_manifest(&first).unwrap();
        let second = ManifestRecord {
            document_uuid: "doc-flat-reconcile".to_string(),
            branch: "main".to_string(),
            parent: Some(first_hash.clone()),
            snapshot: HashRef::parse("sha256:ccc").unwrap(),
            operation_segments: Vec::new(),
            signatures: Vec::new(),
            blobs: Vec::new(),
            created_at_ms: 3,
        };
        let divergent = ManifestRecord {
            document_uuid: "doc-flat-reconcile".to_string(),
            branch: "main".to_string(),
            parent: None,
            snapshot: HashRef::parse("sha256:ddd").unwrap(),
            operation_segments: Vec::new(),
            signatures: Vec::new(),
            blobs: Vec::new(),
            created_at_ms: 4,
        };
        let second_hash = repo.write_manifest(&second).unwrap();
        let divergent_hash = repo.write_manifest(&divergent).unwrap();
        for manifest in [
            first_hash.clone(),
            second_hash.clone(),
            divergent_hash.clone(),
        ] {
            repo.write_candidate_head(&BranchHeadRecord {
                document_uuid: "doc-flat-reconcile".to_string(),
                branch: "main".to_string(),
                manifest,
            })
            .unwrap();
        }

        let reconciliation = repo
            .reconcile_candidate_heads("doc-flat-reconcile", "main")
            .unwrap();

        assert_eq!(reconciliation.initial.current, Some(base_hash));
        assert_eq!(
            reconciliation.advanced,
            vec![first_hash.clone(), second_hash.clone()]
        );
        assert_eq!(
            repo.store()
                .read_head("doc-flat-reconcile", "main")
                .unwrap(),
            Some(second_hash.clone())
        );
        let statuses = reconciliation
            .final_resolution
            .candidates
            .iter()
            .map(|candidate| (&candidate.manifest, candidate.status))
            .collect::<Vec<_>>();
        assert!(statuses.contains(&(&first_hash, CandidateStatus::IntegratedAncestor)));
        assert!(statuses.contains(&(&second_hash, CandidateStatus::AlreadyCurrent)));
        assert!(statuses.contains(&(&divergent_hash, CandidateStatus::NeedsMerge)));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn repository_plans_candidate_merge_from_common_base() {
        let root = std::env::temp_dir().join(format!("opendoc-repo-plan-merge-{}", process_tag()));
        let _ = fs::remove_dir_all(&root);
        let repo = Repository::new(LocalObjectStore::new(&root));
        let base = ManifestRecord {
            document_uuid: "doc-plan".to_string(),
            branch: "main".to_string(),
            parent: None,
            snapshot: HashRef::parse("sha256:aaa").unwrap(),
            operation_segments: Vec::new(),
            signatures: Vec::new(),
            blobs: Vec::new(),
            created_at_ms: 1,
        };
        let base_hash = repo.commit_manifest(&base, None).unwrap().unwrap();
        let current = ManifestRecord {
            document_uuid: "doc-plan".to_string(),
            branch: "main".to_string(),
            parent: Some(base_hash.clone()),
            snapshot: HashRef::parse("sha256:bbb").unwrap(),
            operation_segments: Vec::new(),
            signatures: Vec::new(),
            blobs: Vec::new(),
            created_at_ms: 2,
        };
        let current_hash = repo
            .commit_manifest(&current, Some(&base_hash))
            .unwrap()
            .unwrap();
        let candidate = ManifestRecord {
            document_uuid: "doc-plan".to_string(),
            branch: "main".to_string(),
            parent: Some(base_hash.clone()),
            snapshot: HashRef::parse("sha256:ccc").unwrap(),
            operation_segments: Vec::new(),
            signatures: Vec::new(),
            blobs: Vec::new(),
            created_at_ms: 3,
        };
        let candidate_hash = repo.write_manifest(&candidate).unwrap();
        repo.write_candidate_head(&BranchHeadRecord {
            document_uuid: "doc-plan".to_string(),
            branch: "main".to_string(),
            manifest: candidate_hash.clone(),
        })
        .unwrap();

        let plans = repo.plan_candidate_merges("doc-plan", "main").unwrap();
        assert_eq!(plans.plans.len(), 1);
        assert_eq!(
            plans.plans[0],
            CandidateMergePlan {
                current: Some(current_hash.clone()),
                candidate: candidate_hash.clone(),
                merge_base: Some(base_hash),
                current_since_base: vec![current_hash],
                candidate_since_base: vec![candidate_hash],
            }
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn repository_plans_candidate_merge_without_common_base() {
        let root =
            std::env::temp_dir().join(format!("opendoc-repo-plan-root-merge-{}", process_tag()));
        let _ = fs::remove_dir_all(&root);
        let repo = Repository::new(LocalObjectStore::new(&root));
        let current = ManifestRecord {
            document_uuid: "doc-root-plan".to_string(),
            branch: "main".to_string(),
            parent: None,
            snapshot: HashRef::parse("sha256:aaa").unwrap(),
            operation_segments: Vec::new(),
            signatures: Vec::new(),
            blobs: Vec::new(),
            created_at_ms: 1,
        };
        let current_hash = repo.commit_manifest(&current, None).unwrap().unwrap();
        let candidate = ManifestRecord {
            document_uuid: "doc-root-plan".to_string(),
            branch: "main".to_string(),
            parent: None,
            snapshot: HashRef::parse("sha256:bbb").unwrap(),
            operation_segments: Vec::new(),
            signatures: Vec::new(),
            blobs: Vec::new(),
            created_at_ms: 2,
        };
        let candidate_hash = repo.write_manifest(&candidate).unwrap();
        repo.write_candidate_head(&BranchHeadRecord {
            document_uuid: "doc-root-plan".to_string(),
            branch: "main".to_string(),
            manifest: candidate_hash.clone(),
        })
        .unwrap();

        let plans = repo.plan_candidate_merges("doc-root-plan", "main").unwrap();
        assert_eq!(plans.plans.len(), 1);
        assert_eq!(
            plans.plans[0],
            CandidateMergePlan {
                current: Some(current_hash.clone()),
                candidate: candidate_hash.clone(),
                merge_base: None,
                current_since_base: vec![current_hash],
                candidate_since_base: vec![candidate_hash],
            }
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn repository_writes_lookup_indexes_and_tombstones() {
        let root = std::env::temp_dir().join(format!("opendoc-repo-lookup-{}", process_tag()));
        let _ = fs::remove_dir_all(&root);
        let repo = Repository::new(LocalObjectStore::new(&root));
        let manifest = HashRef::parse("sha256:aaa").unwrap();
        let lookup = LookupRecord {
            document_uuid: "doc-lookup".to_string(),
            branch: "main".to_string(),
            manifest: manifest.clone(),
            aliases: vec![LookupAliasRecord {
                scheme: "doi".to_string(),
                value: "10.1234/Example".to_string(),
            }],
            created_at_ms: 12,
        };

        let paths = repo.write_lookup_record(&lookup).unwrap();
        assert_eq!(paths.len(), 2);
        assert_eq!(
            repo.read_uuid_lookup("doc-lookup").unwrap(),
            Some(lookup.clone())
        );
        assert_eq!(
            repo.read_doi_lookup("10.1234/example").unwrap(),
            Some(lookup.clone())
        );
        assert_eq!(repo.scan_lookup_records().unwrap(), vec![lookup]);

        let upper_scheme_lookup = LookupRecord {
            document_uuid: "doc-upper-doi".to_string(),
            branch: "main".to_string(),
            manifest: manifest.clone(),
            aliases: vec![LookupAliasRecord {
                scheme: "DOI".to_string(),
                value: "10.5678/Upper".to_string(),
            }],
            created_at_ms: 12,
        };
        let paths = repo.write_lookup_record(&upper_scheme_lookup).unwrap();
        assert_eq!(paths.len(), 2);
        assert_eq!(
            repo.read_doi_lookup(" 10.5678/upper ").unwrap(),
            Some(upper_scheme_lookup.clone())
        );
        assert!(repo
            .scan_lookup_records()
            .unwrap()
            .contains(&upper_scheme_lookup));

        let multi_alias_lookup = LookupRecord {
            document_uuid: "doc-multi-doi".to_string(),
            branch: "main".to_string(),
            manifest: manifest.clone(),
            aliases: vec![
                LookupAliasRecord {
                    scheme: "doi".to_string(),
                    value: "10.7777/Primary".to_string(),
                },
                LookupAliasRecord {
                    scheme: "DOI".to_string(),
                    value: "10.7777/Secondary".to_string(),
                },
            ],
            created_at_ms: 13,
        };
        let paths = repo.write_lookup_record(&multi_alias_lookup).unwrap();
        assert_eq!(paths.len(), 3);
        assert_eq!(
            repo.read_doi_lookup("10.7777/primary").unwrap(),
            Some(multi_alias_lookup.clone())
        );
        assert_eq!(
            repo.read_doi_lookup("10.7777/secondary").unwrap(),
            Some(multi_alias_lookup.clone())
        );
        let scanned = repo.scan_lookup_records().unwrap();
        assert_eq!(
            scanned
                .iter()
                .filter(|record| record.document_uuid == "doc-multi-doi")
                .count(),
            1
        );

        let tombstone = TombstoneRecord {
            object: HashRef::parse("sha256:bbb").unwrap(),
            archive_locator: "tape://pool/slot/object".to_string(),
            restore_hint: "request recall".to_string(),
            created_at_ms: 13,
            signer: "ssh-ed25519 AAAA".to_string(),
            signature: vec![4, 5, 6],
        };
        let path = repo.write_tombstone(&tombstone).unwrap();
        assert!(path.starts_with("archive/tombstones/sha256/bb/"));
        assert_eq!(
            repo.read_tombstone(&tombstone.object).unwrap(),
            Some(tombstone.clone())
        );
        assert_eq!(
            repo.scan_tombstone_records().unwrap(),
            vec![tombstone.clone()]
        );
        let extra_tombstone = TombstoneRecord {
            object: HashRef::parse("sha256:eeeeee").unwrap(),
            archive_locator: "tape://pool/slot/extra-object".to_string(),
            restore_hint: "request extra recall".to_string(),
            created_at_ms: 14,
            signer: "ssh-ed25519 AAAA".to_string(),
            signature: vec![4, 5, 6],
        };
        repo.write_tombstone(&extra_tombstone).unwrap();
        let wrong_tombstone = TombstoneRecord {
            object: HashRef::parse("sha256:ddd").unwrap(),
            archive_locator: "tape://pool/slot/object".to_string(),
            restore_hint: "request recall".to_string(),
            created_at_ms: 13,
            signer: "ssh-ed25519 AAAA".to_string(),
            signature: vec![4, 5, 6],
        };
        repo.store()
            .put_named(&path, &encode_record(&wrong_tombstone))
            .unwrap();
        assert!(matches!(
            repo.read_tombstone(&tombstone.object),
            Err(StoreError::HashMismatch)
        ));
        assert!(matches!(
            repo.scan_tombstone_records(),
            Err(StoreError::HashMismatch)
        ));
        let scan = repo.scan_tombstone_entries().unwrap();
        assert_eq!(scan.records, vec![extra_tombstone]);
        assert_eq!(scan.invalid.len(), 1);
        assert!(scan.invalid[0].reason.contains("HashMismatch"));

        let signature = SignatureRecord {
            target: HashRef::parse("sha256:ccc").unwrap(),
            signer: "ssh-ed25519 AAAA".to_string(),
            signer_display: "Alice".to_string(),
            title: "blob".to_string(),
            signed_at_ms: 14,
            signature: vec![7, 8, 9],
        };
        let path = repo
            .write_blob_signature(&signature.target, &signature)
            .unwrap();
        assert_eq!(path, "objects/sha256/cc/ccc.sig");
        assert_eq!(
            repo.read_blob_signature(&signature.target).unwrap(),
            Some(signature.clone())
        );
        let wrong_signature = SignatureRecord {
            target: HashRef::parse("sha256:ddd").unwrap(),
            signer: "ssh-ed25519 AAAA".to_string(),
            signer_display: "Alice".to_string(),
            title: "blob".to_string(),
            signed_at_ms: 14,
            signature: vec![7, 8, 9],
        };
        assert!(matches!(
            repo.write_blob_signature(&signature.target, &wrong_signature),
            Err(StoreError::HashMismatch)
        ));
        repo.store()
            .put_named(&path, &encode_record(&wrong_signature))
            .unwrap();
        assert!(matches!(
            repo.read_blob_signature(&signature.target),
            Err(StoreError::HashMismatch)
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn repository_audits_manifest_dependencies_for_shallow_clone_recovery() {
        let root =
            std::env::temp_dir().join(format!("opendoc-repo-dependency-audit-{}", process_tag()));
        let _ = fs::remove_dir_all(&root);
        let repo = Repository::new(LocalObjectStore::new(&root));

        let snapshot_bytes = b"canonical snapshot";
        let snapshot = digest_bytes("sha256", snapshot_bytes).unwrap();
        repo.store()
            .put_if_absent(&snapshot, snapshot_bytes)
            .unwrap();
        let present_segment_bytes = b"operation segment one";
        let present_segment = digest_bytes("sha256", present_segment_bytes).unwrap();
        repo.store()
            .put_if_absent(&present_segment, present_segment_bytes)
            .unwrap();
        let missing_segment = digest_bytes("sha256", b"missing operation segment").unwrap();
        let signature_bytes = b"version signature record";
        let version_signature = digest_bytes("sha256", signature_bytes).unwrap();
        repo.store()
            .put_if_absent(&version_signature, signature_bytes)
            .unwrap();
        let present_blob_bytes = b"present image blob";
        let present_blob = digest_bytes("sha256", present_blob_bytes).unwrap();
        repo.store()
            .put_if_absent(&present_blob, present_blob_bytes)
            .unwrap();
        let present_blob_signature = SignatureRecord {
            target: present_blob.clone(),
            signer: "ssh-ed25519 AAAA".to_string(),
            signer_display: "Blob Signer".to_string(),
            title: "present image".to_string(),
            signed_at_ms: 15,
            signature: vec![1, 2, 3],
        };
        repo.write_blob_signature(&present_blob, &present_blob_signature)
            .unwrap();
        let recoverable_blob = digest_bytes("sha256", b"archived image blob").unwrap();
        let recoverable_tombstone = TombstoneRecord {
            object: recoverable_blob.clone(),
            archive_locator: "tape://pool/slot/recoverable-image".to_string(),
            restore_hint: "request tape recall".to_string(),
            created_at_ms: 16,
            signer: "ssh-ed25519 AAAA".to_string(),
            signature: vec![4, 5, 6],
        };
        repo.write_tombstone(&recoverable_tombstone).unwrap();
        let unrecoverable_blob = digest_bytes("sha256", b"unrecoverable image blob").unwrap();
        let manifest = ManifestRecord {
            document_uuid: "doc-dependency-audit".to_string(),
            branch: "main".to_string(),
            parent: None,
            snapshot: snapshot.clone(),
            operation_segments: vec![present_segment.clone(), missing_segment.clone()],
            signatures: vec![version_signature.clone()],
            blobs: vec![
                present_blob.clone(),
                recoverable_blob.clone(),
                unrecoverable_blob.clone(),
            ],
            created_at_ms: 17,
        };

        let audit = repo.audit_manifest_dependencies(&manifest).unwrap();

        assert_eq!(
            audit.snapshot,
            ObjectDependencyStatus {
                hash: snapshot,
                present: true,
            }
        );
        assert_eq!(
            audit.operation_segments,
            vec![
                ObjectDependencyStatus {
                    hash: present_segment,
                    present: true,
                },
                ObjectDependencyStatus {
                    hash: missing_segment.clone(),
                    present: false,
                },
            ]
        );
        assert_eq!(
            audit.version_signatures,
            vec![ObjectDependencyStatus {
                hash: version_signature,
                present: true,
            }]
        );
        assert_eq!(audit.blobs.len(), 3);
        assert_eq!(audit.blobs[0].hash, present_blob);
        assert!(audit.blobs[0].bytes_present);
        assert!(audit.blobs[0].signature_sidecar_present);
        assert_eq!(audit.blobs[0].archive_tombstone, None);
        assert_eq!(audit.blobs[1].hash, recoverable_blob);
        assert!(!audit.blobs[1].bytes_present);
        assert!(!audit.blobs[1].signature_sidecar_present);
        assert_eq!(
            audit.blobs[1].archive_tombstone,
            Some(recoverable_tombstone)
        );
        assert_eq!(audit.blobs[2].hash, unrecoverable_blob);
        assert!(!audit.blobs[2].bytes_present);
        let mut expected_missing = vec![
            missing_segment,
            recoverable_blob.clone(),
            unrecoverable_blob,
        ];
        expected_missing.sort_by_key(|hash| hash.to_string());
        assert_eq!(audit.missing_hashes(), expected_missing);
        assert_eq!(audit.recoverable_missing_blobs(), vec![recoverable_blob]);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn repository_rejects_lookup_records_from_wrong_index_paths() {
        let root = std::env::temp_dir().join(format!(
            "opendoc-repo-lookup-path-mismatch-{}",
            process_tag()
        ));
        let _ = fs::remove_dir_all(&root);
        let repo = Repository::new(LocalObjectStore::new(&root));
        let lookup = LookupRecord {
            document_uuid: "doc-correct".to_string(),
            branch: "main".to_string(),
            manifest: HashRef::parse("sha256:aaa").unwrap(),
            aliases: vec![LookupAliasRecord {
                scheme: "doi".to_string(),
                value: "10.1234/correct".to_string(),
            }],
            created_at_ms: 12,
        };

        repo.store()
            .put_named(
                &uuid_lookup_path("doc-wrong").unwrap(),
                &encode_record(&lookup),
            )
            .unwrap();
        assert!(matches!(
            repo.read_uuid_lookup("doc-wrong"),
            Err(StoreError::LookupMismatch)
        ));
        assert!(repo.scan_lookup_records().unwrap().is_empty());

        repo.store()
            .put_named(
                &doi_lookup_path("10.1234/wrong").unwrap(),
                &encode_record(&lookup),
            )
            .unwrap();
        assert!(matches!(
            repo.read_doi_lookup("10.1234/wrong"),
            Err(StoreError::LookupMismatch)
        ));
        assert!(repo.scan_lookup_records().unwrap().is_empty());

        repo.write_lookup_record(&lookup).unwrap();
        assert_eq!(
            repo.read_uuid_lookup("doc-correct").unwrap(),
            Some(lookup.clone())
        );
        assert_eq!(
            repo.read_doi_lookup("10.1234/correct").unwrap(),
            Some(lookup.clone())
        );
        assert_eq!(repo.scan_lookup_records().unwrap(), vec![lookup]);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn repository_scans_valid_lookup_records_while_reporting_invalid_indexes() {
        let root = std::env::temp_dir().join(format!(
            "opendoc-repo-lookup-scan-invalid-{}",
            process_tag()
        ));
        let _ = fs::remove_dir_all(&root);
        let repo = Repository::new(LocalObjectStore::new(&root));
        let lookup = LookupRecord {
            document_uuid: "doc-scan-valid".to_string(),
            branch: "main".to_string(),
            manifest: HashRef::parse("sha256:aaa").unwrap(),
            aliases: vec![LookupAliasRecord {
                scheme: "doi".to_string(),
                value: "10.1234/scan-valid".to_string(),
            }],
            created_at_ms: 3,
        };
        repo.write_lookup_record(&lookup).unwrap();
        repo.store()
            .put_named("indexes/by-uuid/zz/corrupt.idx", b"not a lookup record")
            .unwrap();
        let mismatched = LookupRecord {
            document_uuid: "doc-scan-mismatch".to_string(),
            branch: "main".to_string(),
            manifest: HashRef::parse("sha256:bbb").unwrap(),
            aliases: Vec::new(),
            created_at_ms: 4,
        };
        repo.store()
            .put_named(
                "indexes/by-uuid/zz/mismatch.idx",
                &encode_record(&mismatched),
            )
            .unwrap();

        assert_eq!(repo.scan_lookup_records().unwrap(), vec![lookup.clone()]);
        let scan = repo.scan_lookup_entries().unwrap();
        assert_eq!(scan.records, vec![lookup]);
        assert_eq!(scan.invalid.len(), 2);
        assert!(scan
            .invalid
            .iter()
            .any(|problem| problem.reason.contains("InvalidMagic")));
        assert!(scan
            .invalid
            .iter()
            .any(|problem| problem.reason == "lookup record does not match index path"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn repository_rejects_semantically_invalid_binary_records() {
        let root =
            std::env::temp_dir().join(format!("opendoc-repo-invalid-records-{}", process_tag()));
        let _ = fs::remove_dir_all(&root);
        let repo = Repository::new(LocalObjectStore::new(&root));
        let manifest_hash = HashRef::parse("sha256:aaa").unwrap();

        let invalid_manifest = ManifestRecord {
            document_uuid: String::new(),
            branch: "main".to_string(),
            parent: None,
            snapshot: manifest_hash.clone(),
            operation_segments: Vec::new(),
            signatures: Vec::new(),
            blobs: Vec::new(),
            created_at_ms: 1,
        };
        assert!(matches!(
            repo.write_manifest(&invalid_manifest),
            Err(StoreError::Format(message)) if message.contains("manifest document_uuid is empty")
        ));
        let whitespace_manifest = ManifestRecord {
            document_uuid: " doc ".to_string(),
            branch: "main".to_string(),
            parent: None,
            snapshot: manifest_hash.clone(),
            operation_segments: Vec::new(),
            signatures: Vec::new(),
            blobs: Vec::new(),
            created_at_ms: 1,
        };
        assert!(matches!(
            repo.write_manifest(&whitespace_manifest),
            Err(StoreError::Format(message))
                if message.contains("manifest document_uuid has surrounding whitespace")
        ));
        let invalid_branch_manifest = ManifestRecord {
            document_uuid: "doc".to_string(),
            branch: "main branch".to_string(),
            parent: None,
            snapshot: manifest_hash.clone(),
            operation_segments: Vec::new(),
            signatures: Vec::new(),
            blobs: Vec::new(),
            created_at_ms: 1,
        };
        assert!(matches!(
            repo.write_manifest(&invalid_branch_manifest),
            Err(StoreError::Format(message))
                if message.contains("manifest branch is not a repository key segment")
        ));
        let duplicate_ref_manifest = ManifestRecord {
            document_uuid: "doc".to_string(),
            branch: "main".to_string(),
            parent: None,
            snapshot: manifest_hash.clone(),
            operation_segments: vec![manifest_hash.clone(), manifest_hash.clone()],
            signatures: Vec::new(),
            blobs: Vec::new(),
            created_at_ms: 1,
        };
        assert!(matches!(
            repo.write_manifest(&duplicate_ref_manifest),
            Err(StoreError::Format(message))
                if message.contains("duplicate manifest operation segment reference")
        ));
        let duplicate_ref_bytes = encode_record(&duplicate_ref_manifest);
        let duplicate_ref_hash = digest_bytes("sha256", &duplicate_ref_bytes).unwrap();
        repo.store()
            .put_if_absent(&duplicate_ref_hash, &duplicate_ref_bytes)
            .unwrap();
        assert!(matches!(
            repo.read_manifest(&duplicate_ref_hash),
            Err(StoreError::Format(message))
                if message.contains("duplicate manifest operation segment reference")
        ));

        let invalid_head = BranchHeadRecord {
            document_uuid: "doc".to_string(),
            branch: String::new(),
            manifest: manifest_hash.clone(),
        };
        assert!(matches!(
            repo.write_candidate_head(&invalid_head),
            Err(StoreError::Format(message)) if message.contains("branch head branch is empty")
        ));
        let invalid_path_head = BranchHeadRecord {
            document_uuid: "doc".to_string(),
            branch: "..".to_string(),
            manifest: manifest_hash.clone(),
        };
        assert!(matches!(
            repo.write_candidate_head(&invalid_path_head),
            Err(StoreError::Format(message))
                if message.contains("branch head branch is not a repository key segment")
        ));
        let whitespace_head = BranchHeadRecord {
            document_uuid: " doc ".to_string(),
            branch: "main".to_string(),
            manifest: manifest_hash.clone(),
        };
        assert!(matches!(
            repo.write_candidate_head(&whitespace_head),
            Err(StoreError::Format(message))
                if message.contains("branch head document_uuid has surrounding whitespace")
        ));

        let invalid_lookup = LookupRecord {
            document_uuid: "doc".to_string(),
            branch: "main".to_string(),
            manifest: manifest_hash.clone(),
            aliases: vec![LookupAliasRecord {
                scheme: "doi".to_string(),
                value: String::new(),
            }],
            created_at_ms: 2,
        };
        assert!(matches!(
            repo.write_lookup_record(&invalid_lookup),
            Err(StoreError::Format(message)) if message.contains("lookup alias value is empty")
        ));
        assert!(matches!(
            repo.read_uuid_lookup("../doc"),
            Err(StoreError::InvalidPath)
        ));
        assert!(matches!(
            repo.read_doi_lookup(" "),
            Err(StoreError::InvalidPath)
        ));
        let invalid_path_lookup = LookupRecord {
            document_uuid: "../doc".to_string(),
            branch: "main".to_string(),
            manifest: manifest_hash.clone(),
            aliases: Vec::new(),
            created_at_ms: 2,
        };
        assert!(matches!(
            repo.write_lookup_record(&invalid_path_lookup),
            Err(StoreError::InvalidPath)
        ));
        let whitespace_lookup = LookupRecord {
            document_uuid: " doc ".to_string(),
            branch: "main".to_string(),
            manifest: manifest_hash.clone(),
            aliases: Vec::new(),
            created_at_ms: 2,
        };
        assert!(matches!(
            repo.write_lookup_record(&whitespace_lookup),
            Err(StoreError::Format(message))
                if message.contains("lookup document_uuid has surrounding whitespace")
        ));
        let invalid_branch_lookup = LookupRecord {
            document_uuid: "doc".to_string(),
            branch: "main/branch".to_string(),
            manifest: manifest_hash.clone(),
            aliases: Vec::new(),
            created_at_ms: 2,
        };
        assert!(matches!(
            repo.write_lookup_record(&invalid_branch_lookup),
            Err(StoreError::Format(message))
                if message.contains("lookup branch is not a repository key segment")
        ));

        let mut lookup = invalid_lookup.clone();
        lookup.aliases[0].value = "10.1234/example".to_string();
        let path = uuid_lookup_path(&lookup.document_uuid).unwrap();
        repo.store()
            .put_named(&path, &encode_record(&invalid_lookup))
            .unwrap();
        assert!(matches!(
            repo.read_uuid_lookup("doc"),
            Err(StoreError::Format(message)) if message.contains("lookup alias value is empty")
        ));
        repo.store()
            .put_named(
                &uuid_lookup_path("doc").unwrap(),
                &encode_record(&whitespace_lookup),
            )
            .unwrap();
        assert!(matches!(
            repo.read_uuid_lookup("doc"),
            Err(StoreError::Format(message))
                if message.contains("lookup document_uuid has surrounding whitespace")
        ));

        let whitespace_alias_lookup = LookupRecord {
            document_uuid: "doc-whitespace-alias".to_string(),
            branch: "main".to_string(),
            manifest: manifest_hash.clone(),
            aliases: vec![LookupAliasRecord {
                scheme: "doi".to_string(),
                value: " 10.1234/example ".to_string(),
            }],
            created_at_ms: 2,
        };
        assert!(matches!(
            repo.write_lookup_record(&whitespace_alias_lookup),
            Err(StoreError::Format(message))
                if message.contains("lookup alias value has surrounding whitespace")
        ));
        repo.store()
            .put_named(
                &uuid_lookup_path(&whitespace_alias_lookup.document_uuid).unwrap(),
                &encode_record(&whitespace_alias_lookup),
            )
            .unwrap();
        assert!(matches!(
            repo.read_uuid_lookup("doc-whitespace-alias"),
            Err(StoreError::Format(message))
                if message.contains("lookup alias value has surrounding whitespace")
        ));

        let duplicate_lookup = LookupRecord {
            document_uuid: "doc-duplicate".to_string(),
            branch: "main".to_string(),
            manifest: manifest_hash.clone(),
            aliases: vec![
                LookupAliasRecord {
                    scheme: "doi".to_string(),
                    value: "10.1234/example".to_string(),
                },
                LookupAliasRecord {
                    scheme: "doi".to_string(),
                    value: "10.1234/example".to_string(),
                },
            ],
            created_at_ms: 2,
        };
        assert!(matches!(
            repo.write_lookup_record(&duplicate_lookup),
            Err(StoreError::Format(message)) if message.contains("duplicate lookup alias doi:10.1234/example")
        ));
        let path = uuid_lookup_path(&duplicate_lookup.document_uuid).unwrap();
        repo.store()
            .put_named(&path, &encode_record(&duplicate_lookup))
            .unwrap();
        assert!(matches!(
            repo.read_uuid_lookup("doc-duplicate"),
            Err(StoreError::Format(message)) if message.contains("duplicate lookup alias doi:10.1234/example")
        ));

        let duplicate_doi_lookup = LookupRecord {
            document_uuid: "doc-duplicate-doi".to_string(),
            branch: "main".to_string(),
            manifest: manifest_hash.clone(),
            aliases: vec![
                LookupAliasRecord {
                    scheme: "doi".to_string(),
                    value: "10.1234/Example".to_string(),
                },
                LookupAliasRecord {
                    scheme: "doi".to_string(),
                    value: "10.1234/example".to_string(),
                },
            ],
            created_at_ms: 2,
        };
        assert!(matches!(
            repo.write_lookup_record(&duplicate_doi_lookup),
            Err(StoreError::Format(message)) if message.contains("duplicate lookup alias doi:10.1234/example")
        ));
        let path = uuid_lookup_path(&duplicate_doi_lookup.document_uuid).unwrap();
        repo.store()
            .put_named(&path, &encode_record(&duplicate_doi_lookup))
            .unwrap();
        assert!(matches!(
            repo.read_uuid_lookup("doc-duplicate-doi"),
            Err(StoreError::Format(message)) if message.contains("duplicate lookup alias doi:10.1234/example")
        ));

        let duplicate_doi_scheme_lookup = LookupRecord {
            document_uuid: "doc-duplicate-doi-scheme".to_string(),
            branch: "main".to_string(),
            manifest: manifest_hash.clone(),
            aliases: vec![
                LookupAliasRecord {
                    scheme: "DOI".to_string(),
                    value: "10.1234/Example".to_string(),
                },
                LookupAliasRecord {
                    scheme: "doi".to_string(),
                    value: "10.1234/example".to_string(),
                },
            ],
            created_at_ms: 2,
        };
        assert!(matches!(
            repo.write_lookup_record(&duplicate_doi_scheme_lookup),
            Err(StoreError::Format(message)) if message.contains("duplicate lookup alias doi:10.1234/example")
        ));
        let path = uuid_lookup_path(&duplicate_doi_scheme_lookup.document_uuid).unwrap();
        repo.store()
            .put_named(&path, &encode_record(&duplicate_doi_scheme_lookup))
            .unwrap();
        assert!(matches!(
            repo.read_uuid_lookup("doc-duplicate-doi-scheme"),
            Err(StoreError::Format(message)) if message.contains("duplicate lookup alias doi:10.1234/example")
        ));

        let invalid_tombstone = TombstoneRecord {
            object: HashRef::parse("sha256:bbb").unwrap(),
            archive_locator: "tape://pool/object".to_string(),
            restore_hint: "request recall".to_string(),
            created_at_ms: 3,
            signer: "ssh-ed25519 AAAA".to_string(),
            signature: Vec::new(),
        };
        assert!(matches!(
            repo.write_tombstone(&invalid_tombstone),
            Err(StoreError::Format(message)) if message.contains("tombstone signature is empty")
        ));

        let mut valid_tombstone = invalid_tombstone.clone();
        valid_tombstone.signature = vec![1];
        let path = tombstone_path(&valid_tombstone.object);
        repo.store()
            .put_named(&path, &encode_record(&invalid_tombstone))
            .unwrap();
        assert!(matches!(
            repo.read_tombstone(&valid_tombstone.object),
            Err(StoreError::Format(message)) if message.contains("tombstone signature is empty")
        ));
        let padded_tombstone = TombstoneRecord {
            object: HashRef::parse("sha256:bbd").unwrap(),
            archive_locator: " tape://pool/object".to_string(),
            restore_hint: "request recall".to_string(),
            created_at_ms: 3,
            signer: "ssh-ed25519 AAAA".to_string(),
            signature: vec![1],
        };
        assert!(matches!(
            repo.write_tombstone(&padded_tombstone),
            Err(StoreError::Format(message))
                if message.contains("tombstone archive_locator has surrounding whitespace")
        ));
        repo.store()
            .put_named(
                &tombstone_path(&padded_tombstone.object),
                &encode_record(&padded_tombstone),
            )
            .unwrap();
        assert!(matches!(
            repo.read_tombstone(&padded_tombstone.object),
            Err(StoreError::Format(message))
                if message.contains("tombstone archive_locator has surrounding whitespace")
        ));

        let invalid_signature = SignatureRecord {
            target: HashRef::parse("sha256:ccc").unwrap(),
            signer: String::new(),
            signer_display: "Alice".to_string(),
            title: "blob".to_string(),
            signed_at_ms: 4,
            signature: vec![1],
        };
        assert!(matches!(
            repo.write_blob_signature(&invalid_signature.target, &invalid_signature),
            Err(StoreError::Format(message)) if message.contains("signature signer is empty")
        ));

        repo.store()
            .put_named(
                &blob_signature_path(&invalid_signature.target),
                &encode_record(&invalid_signature),
            )
            .unwrap();
        assert!(matches!(
            repo.read_blob_signature(&invalid_signature.target),
            Err(StoreError::Format(message)) if message.contains("signature signer is empty")
        ));

        let padded_signature = SignatureRecord {
            target: HashRef::parse("sha256:ddd").unwrap(),
            signer: " ssh-ed25519 AAAA".to_string(),
            signer_display: "Alice".to_string(),
            title: "blob".to_string(),
            signed_at_ms: 5,
            signature: vec![1],
        };
        assert!(matches!(
            repo.write_blob_signature(&padded_signature.target, &padded_signature),
            Err(StoreError::Format(message))
                if message.contains("signature signer has surrounding whitespace")
        ));
        repo.store()
            .put_named(
                &blob_signature_path(&padded_signature.target),
                &encode_record(&padded_signature),
            )
            .unwrap();
        assert!(matches!(
            repo.read_blob_signature(&padded_signature.target),
            Err(StoreError::Format(message))
                if message.contains("signature signer has surrounding whitespace")
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn named_paths_cannot_escape_repository_root() {
        let root = std::env::temp_dir().join(format!("opendoc-repo-path-safety-{}", process_tag()));
        let _ = fs::remove_dir_all(&root);
        let store = LocalObjectStore::new(&root);
        assert!(matches!(
            store.put_named("../outside", b"nope"),
            Err(StoreError::InvalidPath)
        ));
        assert!(matches!(
            store.get_named("/absolute"),
            Err(StoreError::InvalidPath)
        ));
        let _ = fs::remove_dir_all(root);
    }
}
