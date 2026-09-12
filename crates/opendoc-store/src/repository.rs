//! The repository: manifests, heads, candidate heads, lookups and tombstones.

use crate::error::StoreError;
use crate::keys::{
    blob_signature_path, candidate_manifest_from_path, clean_key_segment, doi_lookup_path,
    lookup_record_matches_doi_path, lookup_record_matches_index_path, tombstone_object_from_path,
    tombstone_path, uuid_lookup_path, version_label_path,
};
use crate::object_store::{ObjectStore, ObjectStoreLayout};
use crate::repository_types::{
    BlobDependencyStatus, CandidateAdvance, CandidateHeadEntries, CandidateHeadProblem,
    CandidateHeadStatus, CandidateMergePlan, CandidateMergePlans, CandidateReconciliation,
    CandidateResolution, CandidateStatus, CommitOutcome, LookupScan, LookupScanProblem,
    ManifestDependencyAudit, ObjectDependencyStatus, TombstoneScan, TombstoneScanProblem,
    VersionEntry, VersionHistory, VersionHistoryProblem, VERSION_HISTORY_TRAVERSAL_LIMIT,
};
use opendoc_core::{digest_bytes, HashRef};
use opendoc_format::{
    decode_record, encode_record, BranchHeadRecord, LookupRecord, ManifestRecord, SignatureRecord,
    TombstoneRecord, VersionLabelRecord,
};
use std::collections::BTreeSet;

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

    pub fn write_version_label(&self, record: &VersionLabelRecord) -> Result<String, StoreError> {
        record
            .validate()
            .map_err(|err| StoreError::Format(err.to_string()))?;
        let path = version_label_path(&record.manifest);
        self.store.put_named(&path, &encode_record(record))?;
        Ok(path)
    }

    pub fn read_version_label(
        &self,
        manifest: &HashRef,
    ) -> Result<Option<VersionLabelRecord>, StoreError> {
        let Some(bytes) = self.store.get_named(&version_label_path(manifest))? else {
            return Ok(None);
        };
        let record: VersionLabelRecord =
            decode_record(&bytes).map_err(|err| StoreError::Format(err.to_string()))?;
        record
            .validate()
            .map_err(|err| StoreError::Format(err.to_string()))?;
        if &record.manifest != manifest {
            return Err(StoreError::HashMismatch);
        }
        Ok(Some(record))
    }

    /// Walk the manifest parent chain from the branch head, newest first.
    ///
    /// This never fails. ADR 0003 makes lookup and index rules soft: a missing
    /// object or a broken link must degrade into a reported problem, not into a
    /// refusal to show the history that *is* readable. Every failure therefore
    /// lands in `VersionHistory::problems` and the walk either continues (for
    /// per-entry extras such as signatures and labels) or stops cleanly at the
    /// first unreadable link (for the chain itself).
    pub fn list_versions(
        &self,
        document_uuid: &str,
        branch: &str,
        limit: Option<usize>,
    ) -> VersionHistory {
        let mut history = VersionHistory::default();
        let head = match self.store.read_head(document_uuid, branch) {
            Ok(head) => head,
            Err(err) => {
                history.problems.push(VersionHistoryProblem {
                    manifest: None,
                    reason: format!("branch head could not be read: {err}"),
                });
                return history;
            }
        };
        history.head = head.clone();
        let Some(head) = head else {
            return history;
        };

        let mut seen = BTreeSet::new();
        let mut cursor = Some(head.clone());
        while let Some(hash) = cursor {
            if !seen.insert(hash.to_string()) {
                history.problems.push(VersionHistoryProblem {
                    manifest: Some(hash),
                    reason: "manifest ancestry contains a cycle".to_string(),
                });
                break;
            }
            if seen.len() > VERSION_HISTORY_TRAVERSAL_LIMIT {
                history.truncated = true;
                history.problems.push(VersionHistoryProblem {
                    manifest: Some(hash),
                    reason: "manifest ancestry exceeded traversal limit".to_string(),
                });
                break;
            }
            let manifest = match self.read_manifest(&hash) {
                Ok(Some(manifest)) => manifest,
                Ok(None) => {
                    history.truncated = true;
                    history.problems.push(VersionHistoryProblem {
                        manifest: Some(hash),
                        reason: "manifest object is missing; history stops here".to_string(),
                    });
                    break;
                }
                Err(err) => {
                    history.truncated = true;
                    history.problems.push(VersionHistoryProblem {
                        manifest: Some(hash),
                        reason: format!("manifest object could not be read: {err}"),
                    });
                    break;
                }
            };
            if manifest.document_uuid != document_uuid || manifest.branch != branch {
                history.truncated = true;
                history.problems.push(VersionHistoryProblem {
                    manifest: Some(hash),
                    reason: format!(
                        "manifest belongs to {}/{}, not {document_uuid}/{branch}",
                        manifest.document_uuid, manifest.branch
                    ),
                });
                break;
            }

            let snapshot_present = match self.store.exists(&manifest.snapshot) {
                Ok(present) => {
                    if !present {
                        history.problems.push(VersionHistoryProblem {
                            manifest: Some(hash.clone()),
                            reason: "snapshot object is missing; this version cannot be opened"
                                .to_string(),
                        });
                    }
                    present
                }
                Err(err) => {
                    history.problems.push(VersionHistoryProblem {
                        manifest: Some(hash.clone()),
                        reason: format!("snapshot object could not be checked: {err}"),
                    });
                    false
                }
            };

            let mut signatures = Vec::new();
            for signature_hash in &manifest.signatures {
                match self.read_signature(signature_hash) {
                    Ok(Some(record)) => signatures.push(record),
                    Ok(None) => history.problems.push(VersionHistoryProblem {
                        manifest: Some(hash.clone()),
                        reason: format!("signature object {signature_hash} is missing"),
                    }),
                    Err(err) => history.problems.push(VersionHistoryProblem {
                        manifest: Some(hash.clone()),
                        reason: format!(
                            "signature object {signature_hash} could not be read: {err}"
                        ),
                    }),
                }
            }

            let label = match self.read_version_label(&hash) {
                Ok(label) => label,
                Err(err) => {
                    history.problems.push(VersionHistoryProblem {
                        manifest: Some(hash.clone()),
                        reason: format!("version label could not be read: {err}"),
                    });
                    None
                }
            };

            let parent = manifest.parent.clone();
            history.entries.push(VersionEntry {
                is_head: hash == head,
                manifest: hash,
                parent: parent.clone(),
                snapshot: manifest.snapshot,
                operation_segments: manifest.operation_segments,
                created_at_ms: manifest.created_at_ms,
                signatures,
                label,
                snapshot_present,
            });

            if let Some(limit) = limit {
                if history.entries.len() >= limit {
                    history.truncated = parent.is_some();
                    break;
                }
            }
            cursor = parent;
        }
        history
    }

    /// Read a signature object referenced by a manifest's `signatures` list.
    pub fn read_signature(&self, hash: &HashRef) -> Result<Option<SignatureRecord>, StoreError> {
        let Some(bytes) = self.store.get(hash)? else {
            return Ok(None);
        };
        let record: SignatureRecord =
            decode_record(&bytes).map_err(|err| StoreError::Format(err.to_string()))?;
        record
            .validate()
            .map_err(|err| StoreError::Format(err.to_string()))?;
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
