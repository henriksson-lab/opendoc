use super::*;
use std::ops::{Deref, DerefMut};

impl OpenDocApp {
    pub fn save_to_local_repository(
        &mut self,
        root: impl Into<PathBuf>,
    ) -> Result<AppDocument, AppApiError> {
        self.repository_service().save_to_local_repository(root)
    }

    pub fn save_to_local_repository_or_candidate(
        &mut self,
        root: impl Into<PathBuf>,
    ) -> Result<AppDocument, AppApiError> {
        self.repository_service()
            .save_to_local_repository_or_candidate(root)
    }

    pub fn save_to_flat_repository(
        &mut self,
        root: impl Into<PathBuf>,
        namespace: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        self.repository_service()
            .save_to_flat_repository(root, namespace)
    }

    pub fn save_to_flat_repository_or_candidate(
        &mut self,
        root: impl Into<PathBuf>,
        namespace: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        self.repository_service()
            .save_to_flat_repository_or_candidate(root, namespace)
    }

    #[cfg(feature = "opendal-store")]
    pub fn save_to_opendal_fs_repository(
        &mut self,
        root: impl Into<PathBuf>,
        namespace: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        self.repository_service()
            .save_to_opendal_fs_repository(root, namespace)
    }

    #[cfg(feature = "opendal-store")]
    pub fn save_to_opendal_fs_repository_or_candidate(
        &mut self,
        root: impl Into<PathBuf>,
        namespace: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        self.repository_service()
            .save_to_opendal_fs_repository_or_candidate(root, namespace)
    }

    #[cfg(not(feature = "opendal-store"))]
    pub fn save_to_opendal_fs_repository(
        &mut self,
        _root: impl Into<PathBuf>,
        _namespace: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        Err(AppApiError::Conflict(
            "OpenDAL repository support is not enabled".to_string(),
        ))
    }

    #[cfg(not(feature = "opendal-store"))]
    pub fn save_to_opendal_fs_repository_or_candidate(
        &mut self,
        _root: impl Into<PathBuf>,
        _namespace: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        Err(AppApiError::Conflict(
            "OpenDAL repository support is not enabled".to_string(),
        ))
    }

    pub fn autosave_current_repository(&mut self) -> Result<AppDocument, AppApiError> {
        self.repository_service().autosave_current_repository()
    }

    pub fn compact_local_repository(
        &mut self,
        root: impl Into<PathBuf>,
        pack_name: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        self.repository_service()
            .compact_local_repository(root, pack_name)
    }

    pub fn open_saved_projection_by_doi(
        &mut self,
        root: impl Into<PathBuf>,
        doi: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        self.repository_service()
            .open_saved_projection_by_doi(root, doi)
    }

    pub fn open_flat_projection_by_doi(
        &mut self,
        root: impl Into<PathBuf>,
        namespace: impl Into<String>,
        doi: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        self.repository_service()
            .open_flat_projection_by_doi(root, namespace, doi)
    }

    #[cfg(feature = "opendal-store")]
    pub fn open_opendal_fs_projection_by_doi(
        &mut self,
        root: impl Into<PathBuf>,
        namespace: impl Into<String>,
        doi: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        self.repository_service()
            .open_opendal_fs_projection_by_doi(root, namespace, doi)
    }

    #[cfg(not(feature = "opendal-store"))]
    pub fn open_opendal_fs_projection_by_doi(
        &mut self,
        _root: impl Into<PathBuf>,
        _namespace: impl Into<String>,
        _doi: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        Err(AppApiError::Conflict(
            "OpenDAL repository support is not enabled".to_string(),
        ))
    }

    pub fn open_saved_projection(
        &mut self,
        root: impl Into<PathBuf>,
        document_uuid: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        self.repository_service()
            .open_saved_projection(root, document_uuid)
    }

    pub fn scan_local_repository(
        &mut self,
        root: impl Into<PathBuf>,
    ) -> Result<AppDocument, AppApiError> {
        self.repository_service().scan_local_repository(root)
    }

    pub fn open_flat_projection(
        &mut self,
        root: impl Into<PathBuf>,
        namespace: impl Into<String>,
        document_uuid: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        self.repository_service()
            .open_flat_projection(root, namespace, document_uuid)
    }

    #[cfg(feature = "opendal-store")]
    pub fn open_opendal_fs_projection(
        &mut self,
        root: impl Into<PathBuf>,
        namespace: impl Into<String>,
        document_uuid: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        self.repository_service()
            .open_opendal_fs_projection(root, namespace, document_uuid)
    }

    #[cfg(not(feature = "opendal-store"))]
    pub fn open_opendal_fs_projection(
        &mut self,
        _root: impl Into<PathBuf>,
        _namespace: impl Into<String>,
        _document_uuid: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        Err(AppApiError::Conflict(
            "OpenDAL repository support is not enabled".to_string(),
        ))
    }

    pub fn merge_local_repository_candidates(
        &mut self,
        root: impl Into<PathBuf>,
        document_uuid: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        self.repository_service()
            .merge_local_repository_candidates(root, document_uuid)
    }

    pub fn merge_flat_repository_candidates(
        &mut self,
        root: impl Into<PathBuf>,
        namespace: impl Into<String>,
        document_uuid: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        self.repository_service()
            .merge_flat_repository_candidates(root, namespace, document_uuid)
    }

    #[cfg(feature = "opendal-store")]
    pub fn merge_opendal_fs_repository_candidates(
        &mut self,
        root: impl Into<PathBuf>,
        namespace: impl Into<String>,
        document_uuid: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        self.repository_service()
            .merge_opendal_fs_repository_candidates(root, namespace, document_uuid)
    }

    #[cfg(not(feature = "opendal-store"))]
    pub fn merge_opendal_fs_repository_candidates(
        &mut self,
        _root: impl Into<PathBuf>,
        _namespace: impl Into<String>,
        _document_uuid: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        Err(AppApiError::Conflict(
            "OpenDAL repository support is not enabled".to_string(),
        ))
    }

    fn repository_service(&mut self) -> RepositoryService<'_> {
        RepositoryService::new(self)
    }

    pub(crate) fn projected_blobs(&self) -> Vec<AppBlobRef> {
        self.projection_service().projected_blobs()
    }

    pub(crate) fn signature_count(&self) -> usize {
        self.signatures.len()
            + self
                .blob_signatures
                .values()
                .map(|signatures| signatures.len())
                .sum::<usize>()
    }

    pub(crate) fn snapshot_payload(&self) -> Result<Vec<u8>, AppApiError> {
        RepositoryService::snapshot_payload_for_document(
            RepositoryService::signing_document_from_snapshot(self.snapshot_document()),
        )
    }

    fn snapshot_document(&self) -> AppDocument {
        let mut document = AppDocument::from_core(&self.document);
        document.workbook = self.workbook.evaluated();
        document.blobs = self.blobs.clone();
        document
    }

    fn decode_snapshot_object(bytes: &[u8]) -> Result<SnapshotRecord<AppDocument>, AppApiError> {
        RepositoryService::decode_snapshot_object(bytes)
    }
}

struct RepositoryService<'a> {
    app: &'a mut OpenDocApp,
}

impl<'a> RepositoryService<'a> {
    fn new(app: &'a mut OpenDocApp) -> Self {
        Self { app }
    }

    fn save_to_local_repository(
        &mut self,
        root: impl Into<PathBuf>,
    ) -> Result<AppDocument, AppApiError> {
        self.save_to_local_repository_inner(root.into(), false)
    }

    fn save_to_local_repository_or_candidate(
        &mut self,
        root: impl Into<PathBuf>,
    ) -> Result<AppDocument, AppApiError> {
        self.save_to_local_repository_inner(root.into(), true)
    }

    fn save_to_flat_repository(
        &mut self,
        root: impl Into<PathBuf>,
        namespace: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let root = root.into();
        let namespace = namespace.into();
        let store = FlatObjectStore::new(&root, namespace)
            .map_err(|err| AppApiError::Store(err.to_string()))?;
        let namespace = store.namespace().to_string();
        let repo = Repository::new(store);
        self.save_to_repository_inner(root, repo, false, "flat", Some(namespace))
    }

    fn save_to_flat_repository_or_candidate(
        &mut self,
        root: impl Into<PathBuf>,
        namespace: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let root = root.into();
        let namespace = namespace.into();
        let store = FlatObjectStore::new(&root, namespace)
            .map_err(|err| AppApiError::Store(err.to_string()))?;
        let namespace = store.namespace().to_string();
        let repo = Repository::new(store);
        self.save_to_repository_inner(root, repo, true, "flat", Some(namespace))
    }

    #[cfg(feature = "opendal-store")]
    fn save_to_opendal_fs_repository(
        &mut self,
        root: impl Into<PathBuf>,
        namespace: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let root = root.into();
        let store = OpenDalObjectStore::from_fs_root(&root, namespace)
            .map_err(|err| AppApiError::Store(err.to_string()))?;
        let namespace = store.namespace().to_string();
        let repo = Repository::new(store);
        self.save_to_repository_inner(root, repo, false, "opendal-fs", Some(namespace))
    }

    #[cfg(feature = "opendal-store")]
    fn save_to_opendal_fs_repository_or_candidate(
        &mut self,
        root: impl Into<PathBuf>,
        namespace: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let root = root.into();
        let store = OpenDalObjectStore::from_fs_root(&root, namespace)
            .map_err(|err| AppApiError::Store(err.to_string()))?;
        let namespace = store.namespace().to_string();
        let repo = Repository::new(store);
        self.save_to_repository_inner(root, repo, true, "opendal-fs", Some(namespace))
    }

    fn autosave_current_repository(&mut self) -> Result<AppDocument, AppApiError> {
        let root = self.repository_root.clone().ok_or_else(|| {
            AppApiError::Conflict("autosave needs an opened or saved repository".to_string())
        })?;
        if !self.has_pending_save_changes() {
            return Ok(self.document());
        }
        match self.repository_backend.as_deref() {
            Some("local") => self.save_to_local_repository_or_candidate(root),
            Some("flat") => {
                let namespace = self.repository_namespace.clone().ok_or_else(|| {
                    AppApiError::Conflict("flat autosave needs a repository namespace".to_string())
                })?;
                self.save_to_flat_repository_or_candidate(root, namespace)
            }
            #[cfg(feature = "opendal-store")]
            Some("opendal-fs") => {
                let namespace = self.repository_namespace.clone().ok_or_else(|| {
                    AppApiError::Conflict(
                        "OpenDAL FS autosave needs a repository namespace".to_string(),
                    )
                })?;
                self.save_to_opendal_fs_repository_or_candidate(root, namespace)
            }
            Some(other) => Err(AppApiError::Conflict(format!(
                "autosave does not support repository backend {other}"
            ))),
            None => Err(AppApiError::Conflict(
                "autosave needs a repository backend".to_string(),
            )),
        }
    }

    fn compact_local_repository(
        &mut self,
        root: impl Into<PathBuf>,
        pack_name: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let root = root.into();
        let store = LocalObjectStore::new(&root);
        let stats = store
            .compact_loose_objects_to_pack(pack_name.as_ref())
            .map_err(|err| AppApiError::Store(err.to_string()))?;
        self.repository_root = Some(root);
        self.repository_backend = Some("local".to_string());
        self.repository_namespace = None;
        let mut document = self.document();
        document.warnings.push(AppWarning {
            code: "local-repository-compacted".to_string(),
            message: format!(
                "compacted {} local repository objects into pack {} ({} bytes)",
                stats.objects, stats.pack, stats.bytes
            ),
        });
        Ok(document)
    }

    fn save_to_local_repository_inner(
        &mut self,
        root: PathBuf,
        allow_candidate: bool,
    ) -> Result<AppDocument, AppApiError> {
        let repo = Repository::new(LocalObjectStore::new(&root));
        self.save_to_repository_inner(root, repo, allow_candidate, "local", None)
    }

    fn save_to_repository_inner<S: ObjectStore>(
        &mut self,
        root: PathBuf,
        repo: Repository<S>,
        allow_candidate: bool,
        repository_backend: impl Into<String>,
        repository_namespace: Option<String>,
    ) -> Result<AppDocument, AppApiError> {
        self.invalidate_projection();
        let pending_operations = self.operation_envelopes[self.saved_operation_count..].to_vec();
        validate_operation_envelopes(&pending_operations)?;
        let target_head = repo
            .store()
            .read_head(self.document.uuid.as_str(), SNAPSHOT_BRANCH)
            .map_err(|err| AppApiError::Store(err.to_string()))?;
        let (expected, operations, history_in_memory) = match &self.last_manifest {
            Some(last_manifest) => {
                let last_hash = opendoc_core::HashRef::parse(last_manifest)
                    .map_err(|err| AppApiError::Model(err.to_string()))?;
                let known_here = repo
                    .read_manifest(&last_hash)
                    .map_err(|err| AppApiError::Store(err.to_string()))?
                    .is_some();
                if known_here {
                    (Some(last_hash), pending_operations, true)
                } else if target_head.is_none() {
                    // "Save as" into a repository that has never seen this
                    // document: write the complete history as a fresh chain.
                    (None, self.operation_envelopes.clone(), true)
                } else {
                    return Err(AppApiError::Conflict(
                        "target repository already holds a different version of this document; open it there and merge instead".to_string(),
                    ));
                }
            }
            None => (target_head, pending_operations, false),
        };
        if history_in_memory {
            validate_operation_envelopes(&self.operation_envelopes)?;
        } else if let Some(expected) = expected.as_ref() {
            let parent_manifest = repo
                .read_manifest(expected)
                .map_err(|err| AppApiError::Store(err.to_string()))?
                .ok_or_else(|| {
                    AppApiError::NotFound("parent manifest object was not found".to_string())
                })?;
            let mut full_history = self.read_operation_envelopes(&repo, &parent_manifest)?;
            full_history.extend(operations.clone());
            validate_operation_envelopes(&full_history)?;
        }
        let snapshot_source = self.snapshot_document();
        snapshot_source.validate_source()?;
        let snapshot = SnapshotRecord::new(
            self.document.uuid.to_string(),
            APP_DOCUMENT_FORMAT,
            snapshot_source,
        );
        snapshot
            .validate()
            .map_err(|err| AppApiError::Format(err.to_string()))?;
        let snapshot_bytes = Self::encode_snapshot_object(&snapshot)?;
        let snapshot_hash = digest_bytes("sha256", &snapshot_bytes)
            .map_err(|err| AppApiError::Model(err.to_string()))?;
        repo.store()
            .put_if_absent(&snapshot_hash, &snapshot_bytes)
            .map_err(|err| AppApiError::Store(err.to_string()))?;
        let operation_segment_hashes = if operations.is_empty() {
            Vec::new()
        } else {
            let previous_segment = self.previous_operation_segment(&repo, expected.as_ref())?;
            let operation_segment = OperationSegmentRecord::new(
                self.document.uuid.to_string(),
                SNAPSHOT_BRANCH,
                previous_segment,
                expected.as_ref().map(ToString::to_string),
                operations,
            );
            operation_segment
                .validate()
                .map_err(|err| AppApiError::Format(err.to_string()))?;
            let operation_segment_bytes =
                Self::encode_operation_segment_object(&operation_segment)?;
            let operation_segment_hash = digest_bytes("sha256", &operation_segment_bytes)
                .map_err(|err| AppApiError::Model(err.to_string()))?;
            repo.store()
                .put_if_absent(&operation_segment_hash, &operation_segment_bytes)
                .map_err(|err| AppApiError::Store(err.to_string()))?;
            vec![operation_segment_hash]
        };
        let mut signature_hashes = Vec::new();
        let mut blob_hashes = Vec::new();
        for blob in &self.blobs {
            let hash = opendoc_core::HashRef::parse(&blob.hash)
                .map_err(|err| AppApiError::Model(err.to_string()))?;
            blob_hashes.push(hash);
        }
        for (hash_text, bytes) in &self.blob_bytes {
            let hash = opendoc_core::HashRef::parse(hash_text)
                .map_err(|err| AppApiError::Model(err.to_string()))?;
            let actual = digest_bytes(hash.algorithm(), bytes)
                .map_err(|err| AppApiError::Model(err.to_string()))?;
            if actual != hash {
                return Err(AppApiError::Store("blob hash mismatch".to_string()));
            }
            repo.store()
                .put_if_absent(&hash, bytes)
                .map_err(|err| AppApiError::Store(err.to_string()))?;
        }
        for (hash_text, signatures) in &self.blob_signatures {
            let hash = opendoc_core::HashRef::parse(hash_text)
                .map_err(|err| AppApiError::Model(err.to_string()))?;
            for signature in signatures {
                repo.write_blob_signature(&hash, signature)
                    .map_err(|err| AppApiError::Store(err.to_string()))?;
            }
        }
        for record in self.blob_tombstone_records.values() {
            repo.write_tombstone(record)
                .map_err(|err| AppApiError::Store(err.to_string()))?;
        }
        let signing_payload = self.snapshot_payload()?;
        let signing_target = digest_bytes("sha256", &signing_payload)
            .map_err(|err| AppApiError::Model(err.to_string()))?;
        for signature in &self.signatures {
            signature
                .validate()
                .map_err(|err| AppApiError::Format(err.to_string()))?;
            if signature.target != signing_target {
                return Err(AppApiError::Store(
                    "manifest signature target does not match signing payload".to_string(),
                ));
            }
            let signature_bytes = encode_record(signature);
            let signature_hash = digest_bytes("sha256", &signature_bytes)
                .map_err(|err| AppApiError::Model(err.to_string()))?;
            repo.store()
                .put_if_absent(&signature_hash, &signature_bytes)
                .map_err(|err| AppApiError::Store(err.to_string()))?;
            signature_hashes.push(signature_hash);
        }
        let manifest = ManifestRecord {
            document_uuid: self.document.uuid.to_string(),
            branch: SNAPSHOT_BRANCH.to_string(),
            parent: expected.clone(),
            snapshot: snapshot_hash,
            operation_segments: operation_segment_hashes,
            signatures: signature_hashes,
            blobs: blob_hashes,
            created_at_ms: now_ms(),
        };
        let (manifest_hash, committed_to_head) = if allow_candidate {
            match repo
                .commit_manifest_or_candidate(&manifest, expected.as_ref())
                .map_err(|err| AppApiError::Store(err.to_string()))?
            {
                opendoc_store::CommitOutcome::Committed(hash) => (hash, true),
                opendoc_store::CommitOutcome::Candidate { manifest, .. } => (manifest, false),
            }
        } else {
            let Some(manifest_hash) = repo
                .commit_manifest(&manifest, expected.as_ref())
                .map_err(|err| AppApiError::Store(err.to_string()))?
            else {
                return Err(AppApiError::Conflict("branch head changed".to_string()));
            };
            (manifest_hash, true)
        };
        let aliases = self
            .document
            .doi
            .iter()
            .map(|doi| LookupAliasRecord {
                scheme: "doi".to_string(),
                value: doi.clone(),
            })
            .collect();
        if committed_to_head {
            repo.write_lookup_record(&LookupRecord {
                document_uuid: self.document.uuid.to_string(),
                branch: SNAPSHOT_BRANCH.to_string(),
                manifest: manifest_hash.clone(),
                aliases,
                created_at_ms: now_ms(),
            })
            .map_err(|err| AppApiError::Store(err.to_string()))?;
        }
        self.repository_root = Some(root);
        self.repository_backend = Some(repository_backend.into());
        self.repository_namespace = repository_namespace;
        self.last_manifest = Some(manifest_hash.to_string());
        self.record_recent_document(now_ms());
        self.saved_operation_count = self.operation_journal.len();
        self.saved_signature_count = self.signature_count();
        Ok(self.document())
    }

    pub(crate) fn signature_count(&self) -> usize {
        self.signatures.len()
            + self
                .blob_signatures
                .values()
                .map(|signatures| signatures.len())
                .sum::<usize>()
    }

    pub(crate) fn has_pending_save_changes(&self) -> bool {
        self.saved_operation_count != self.operation_journal.len()
            || self.saved_signature_count != self.signature_count()
    }

    /// Continue this actor's sequence numbers after the highest one already
    /// present in the loaded journal so a reopened document never re-mints an
    /// operation id that the repository already holds.
    fn rebase_next_seq_from_journal(&mut self) {
        let max_seq = self
            .operation_journal
            .iter()
            .filter(|record| record.actor == self.actor_id)
            .map(|record| record.seq)
            .max();
        self.next_seq = max_seq.map_or(1, |seq| seq + 1).max(self.next_seq);
    }

    pub fn open_saved_projection_by_doi(
        &mut self,
        root: impl Into<PathBuf>,
        doi: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let root = root.into();
        let doi = doi.as_ref().trim().to_string();
        let repo = Repository::new(LocalObjectStore::new(&root));
        let lookup = lookup_by_doi_or_scan(&repo, &doi)?
            .ok_or_else(|| AppApiError::NotFound("DOI lookup record was not found".to_string()))?;
        let document = self.open_saved_projection(root, lookup.record.document_uuid)?;
        Ok(self.document_with_doi_lookup_warning(
            document,
            &doi,
            lookup.used_scan,
            lookup.scan_warnings,
        ))
    }

    pub fn open_flat_projection_by_doi(
        &mut self,
        root: impl Into<PathBuf>,
        namespace: impl Into<String>,
        doi: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let root = root.into();
        let namespace = namespace.into();
        let store = FlatObjectStore::new(&root, namespace)
            .map_err(|err| AppApiError::Store(err.to_string()))?;
        let namespace = store.namespace().to_string();
        let doi = doi.as_ref().trim().to_string();
        let repo = Repository::new(store);
        let lookup = lookup_by_doi_or_scan(&repo, &doi)?
            .ok_or_else(|| AppApiError::NotFound("DOI lookup record was not found".to_string()))?;
        let document = self.open_projection_from_repository(
            root,
            repo,
            lookup.record.document_uuid,
            "flat",
            Some(namespace),
        )?;
        Ok(self.document_with_doi_lookup_warning(
            document,
            &doi,
            lookup.used_scan,
            lookup.scan_warnings,
        ))
    }

    #[cfg(feature = "opendal-store")]
    pub fn open_opendal_fs_projection_by_doi(
        &mut self,
        root: impl Into<PathBuf>,
        namespace: impl Into<String>,
        doi: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let root = root.into();
        let store = OpenDalObjectStore::from_fs_root(&root, namespace)
            .map_err(|err| AppApiError::Store(err.to_string()))?;
        let namespace = store.namespace().to_string();
        let doi = doi.as_ref().trim().to_string();
        let repo = Repository::new(store);
        let lookup = lookup_by_doi_or_scan(&repo, &doi)?
            .ok_or_else(|| AppApiError::NotFound("DOI lookup record was not found".to_string()))?;
        let document = self.open_projection_from_repository(
            root,
            repo,
            lookup.record.document_uuid,
            "opendal-fs",
            Some(namespace),
        )?;
        Ok(self.document_with_doi_lookup_warning(
            document,
            &doi,
            lookup.used_scan,
            lookup.scan_warnings,
        ))
    }

    pub fn open_saved_projection(
        &mut self,
        root: impl Into<PathBuf>,
        document_uuid: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let root = root.into();
        let repo = Repository::new(LocalObjectStore::new(&root));
        self.open_projection_from_repository(
            root,
            repo,
            document_uuid.as_ref().trim(),
            "local",
            None,
        )
    }

    pub fn scan_local_repository(
        &mut self,
        root: impl Into<PathBuf>,
    ) -> Result<AppDocument, AppApiError> {
        let root = root.into();
        let repo = Repository::new(LocalObjectStore::new(&root));
        let scan = repo
            .scan_lookup_entries()
            .map_err(|err| AppApiError::Store(err.to_string()))?;
        let mut discovered = Vec::new();
        let root_text = root.to_string_lossy().to_string();
        for lookup in scan.records {
            if lookup.branch != SNAPSHOT_BRANCH {
                continue;
            }
            let (title, doi) = match read_repository_snapshot_summary(&repo, &lookup) {
                Ok(summary) => summary,
                Err(err) => {
                    self.push_model_warning(
                        "local-repository-scan-problem",
                        format!(
                            "local repository scan could not read {} at {}: {err}",
                            lookup.document_uuid, lookup.manifest
                        ),
                    );
                    (lookup.document_uuid.clone(), lookup_doi_alias(&lookup))
                }
            };
            discovered.push(AppRecentDocument {
                uuid: lookup.document_uuid,
                title,
                doi,
                repository_root: root_text.clone(),
                repository_backend: "local".to_string(),
                repository_namespace: None,
                last_manifest: Some(lookup.manifest.to_string()),
                updated_at_ms: lookup.created_at_ms,
            });
        }
        for problem in scan.invalid {
            self.push_model_warning(
                "local-repository-scan-problem",
                format!(
                    "local repository scan ignored invalid index {}: {}",
                    problem.path, problem.reason
                ),
            );
        }
        discovered.sort_by(|left, right| {
            right
                .updated_at_ms
                .cmp(&left.updated_at_ms)
                .then(left.title.cmp(&right.title))
                .then(left.uuid.cmp(&right.uuid))
        });
        for entry in discovered.into_iter().rev() {
            self.recent_documents.retain(|recent| {
                !(recent.uuid == entry.uuid
                    && recent.repository_root == entry.repository_root
                    && recent.repository_backend == entry.repository_backend
                    && recent.repository_namespace == entry.repository_namespace)
            });
            self.recent_documents.insert(0, entry);
        }
        self.recent_documents.truncate(20);
        self.repository_root = Some(root);
        self.repository_backend = Some("local".to_string());
        self.repository_namespace = None;
        Ok(self.document())
    }

    pub fn open_flat_projection(
        &mut self,
        root: impl Into<PathBuf>,
        namespace: impl Into<String>,
        document_uuid: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let root = root.into();
        let namespace = namespace.into();
        let store = FlatObjectStore::new(&root, namespace)
            .map_err(|err| AppApiError::Store(err.to_string()))?;
        let namespace = store.namespace().to_string();
        let repo = Repository::new(store);
        self.open_projection_from_repository(
            root,
            repo,
            document_uuid.as_ref().trim(),
            "flat",
            Some(namespace),
        )
    }

    #[cfg(feature = "opendal-store")]
    pub fn open_opendal_fs_projection(
        &mut self,
        root: impl Into<PathBuf>,
        namespace: impl Into<String>,
        document_uuid: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let root = root.into();
        let store = OpenDalObjectStore::from_fs_root(&root, namespace)
            .map_err(|err| AppApiError::Store(err.to_string()))?;
        let namespace = store.namespace().to_string();
        let repo = Repository::new(store);
        self.open_projection_from_repository(
            root,
            repo,
            document_uuid.as_ref().trim(),
            "opendal-fs",
            Some(namespace),
        )
    }

    fn open_projection_from_repository<S: ObjectStore>(
        &mut self,
        root: PathBuf,
        repo: Repository<S>,
        document_uuid: impl AsRef<str>,
        repository_backend: impl Into<String>,
        repository_namespace: Option<String>,
    ) -> Result<AppDocument, AppApiError> {
        let head = repo
            .store()
            .read_head(document_uuid.as_ref(), SNAPSHOT_BRANCH)
            .map_err(|err| AppApiError::Store(err.to_string()))?
            .ok_or_else(|| AppApiError::NotFound("document head was not found".to_string()))?;
        let manifest = repo
            .read_manifest(&head)
            .map_err(|err| AppApiError::Store(err.to_string()))?
            .ok_or_else(|| AppApiError::NotFound("manifest object was not found".to_string()))?;
        let snapshot_bytes = repo
            .store()
            .get(&manifest.snapshot)
            .map_err(|err| match err {
                StoreError::HashMismatch => {
                    AppApiError::Store("snapshot hash mismatch".to_string())
                }
                other => AppApiError::Store(other.to_string()),
            })?
            .ok_or_else(|| AppApiError::NotFound("snapshot object was not found".to_string()))?;
        let actual = digest_bytes(manifest.snapshot.algorithm(), &snapshot_bytes)
            .map_err(|err| AppApiError::Model(err.to_string()))?;
        if actual != manifest.snapshot {
            return Err(AppApiError::Store("snapshot hash mismatch".to_string()));
        }
        let snapshot = Self::decode_snapshot_object(&snapshot_bytes)?;
        snapshot
            .validate()
            .map_err(|err| AppApiError::Format(err.to_string()))?;
        validate_snapshot_matches_manifest(&snapshot, &manifest)?;
        if snapshot.source_format != APP_DOCUMENT_FORMAT {
            return Err(AppApiError::Format(format!(
                "unsupported snapshot source format {}",
                snapshot.source_format
            )));
        }
        snapshot.source.validate_source()?;
        self.operation_envelopes = self.read_operation_envelopes(&repo, &manifest)?;
        self.operation_journal = self
            .operation_envelopes
            .iter()
            .map(|envelope| envelope.record.clone())
            .collect();
        self.saved_operation_count = self.operation_journal.len();
        self.rebase_next_seq_from_journal();
        let source = snapshot.source;
        let signing_payload = Self::snapshot_payload_for_document(
            Self::signing_document_from_snapshot(source.clone()),
        )?;
        let signing_target = digest_bytes("sha256", &signing_payload)
            .map_err(|err| AppApiError::Model(err.to_string()))?;
        self.document = source.to_core()?;
        self.workbook = source.workbook.evaluated();
        self.blobs = source.blobs.clone();
        let mut source_warnings = source.warnings.clone();
        restore_referenced_image_blobs(
            &mut self.blobs,
            &source.blocks,
            &[source.blobs],
            &mut source_warnings,
        )?;
        self.document.warnings = source_warnings
            .iter()
            .map(AppWarning::to_core)
            .collect::<Vec<_>>();
        self.is_open = true;
        self.blob_bytes.clear();
        self.blob_signatures.clear();
        self.blob_tombstones.clear();
        self.blob_tombstone_records.clear();
        self.verify_manifest_blobs(&repo, &manifest)?;
        self.read_retained_deleted_blob_sidecars(&repo)?;
        self.signatures = self.read_signatures(&repo, &manifest, &signing_target)?;
        self.saved_signature_count = self.signature_count();
        self.invalidate_projection();
        self.repository_root = Some(root);
        self.repository_backend = Some(repository_backend.into());
        self.repository_namespace = repository_namespace;
        self.last_manifest = Some(head.to_string());
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.record_recent_document(now_ms());
        Ok(self.document())
    }

    pub fn merge_local_repository_candidates(
        &mut self,
        root: impl Into<PathBuf>,
        document_uuid: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        self.merge_repository_candidates_inner(
            root.into(),
            document_uuid.as_ref().trim().to_string(),
            AppRepositoryTarget::Local,
        )
    }

    pub fn merge_flat_repository_candidates(
        &mut self,
        root: impl Into<PathBuf>,
        namespace: impl Into<String>,
        document_uuid: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        self.merge_repository_candidates_inner(
            root.into(),
            document_uuid.as_ref().trim().to_string(),
            AppRepositoryTarget::Flat {
                namespace: namespace.into(),
            },
        )
    }

    #[cfg(feature = "opendal-store")]
    pub fn merge_opendal_fs_repository_candidates(
        &mut self,
        root: impl Into<PathBuf>,
        namespace: impl Into<String>,
        document_uuid: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        self.merge_repository_candidates_inner(
            root.into(),
            document_uuid.as_ref().trim().to_string(),
            AppRepositoryTarget::OpenDalFs {
                namespace: namespace.into(),
            },
        )
    }

    fn merge_repository_candidates_inner(
        &mut self,
        root: PathBuf,
        document_uuid: String,
        target: AppRepositoryTarget,
    ) -> Result<AppDocument, AppApiError> {
        for _ in 0..32 {
            let repo = target.repository(&root)?;
            repo.reconcile_candidate_heads(&document_uuid, SNAPSHOT_BRANCH)
                .map_err(|err| AppApiError::Store(err.to_string()))?;
            let plans = repo
                .plan_candidate_merges(&document_uuid, SNAPSHOT_BRANCH)
                .map_err(|err| AppApiError::Store(err.to_string()))?;
            let Some(plan) = plans.plans.iter().find(|plan| {
                self.candidate_has_unmerged_operations(&repo, plan)
                    .unwrap_or(true)
            }) else {
                return target.open(self, root, document_uuid);
            };
            let Some(current_hash) = plan.current.as_ref() else {
                return Err(AppApiError::Conflict(
                    "cannot merge candidate without a current branch head".to_string(),
                ));
            };

            let base_source = if let Some(base_hash) = &plan.merge_base {
                Some(self.read_app_document_at_manifest(&repo, base_hash)?)
            } else {
                None
            };
            let base_document = match &base_source {
                Some(source) => source.to_core()?,
                None => Document::new("Merged OpenDoc"),
            };
            let current_source = self.read_app_document_at_manifest(&repo, current_hash)?;
            let candidate_source = self.read_app_document_at_manifest(&repo, &plan.candidate)?;
            let current_manifest = repo
                .read_manifest(current_hash)
                .map_err(|err| AppApiError::Store(err.to_string()))?
                .ok_or_else(|| {
                    AppApiError::NotFound("current manifest was not found".to_string())
                })?;
            let candidate_manifest = repo
                .read_manifest(&plan.candidate)
                .map_err(|err| AppApiError::Store(err.to_string()))?
                .ok_or_else(|| {
                    AppApiError::NotFound("candidate manifest was not found".to_string())
                })?;
            let base_manifest = plan
                .merge_base
                .as_ref()
                .map(|base| {
                    repo.read_manifest(base)
                        .map_err(|err| AppApiError::Store(err.to_string()))?
                        .ok_or_else(|| {
                            AppApiError::NotFound("base manifest was not found".to_string())
                        })
                })
                .transpose()?;
            let current_delta =
                self.operation_delta_since_base(&repo, base_manifest.as_ref(), &current_manifest)?;
            let candidate_delta = self.operation_delta_since_base(
                &repo,
                base_manifest.as_ref(),
                &candidate_manifest,
            )?;
            let current_ops = merge_operations_from_envelopes(&current_delta);
            let candidate_ops = merge_operations_from_envelopes(&candidate_delta);
            let merge_result = merge_operations(&base_document, &[current_ops, candidate_ops])
                .map_err(|err| AppApiError::Model(err.to_string()))?;

            let mut merged_source = current_source.clone();
            let merged_document = AppDocument::from_core(&merge_result.document);
            merged_source.title = merged_document.title;
            merged_source.doi = merged_document.doi;
            merged_source.blocks = merged_document.blocks;
            merged_source.comments = merged_document.comments;
            merged_source.suggestions = merged_document.suggestions;
            merged_source.citations = merged_document.citations;
            let (merged_workbook, spreadsheet_warnings) = merge_spreadsheet_envelope_streams(
                base_source
                    .as_ref()
                    .map(|source| source.workbook.clone())
                    .unwrap_or_else(AppSpreadsheetWorkbook::sample),
                &[&current_delta, &candidate_delta],
            )?;
            merged_source.workbook = merged_workbook;
            merged_source.workbook = merged_source.workbook.evaluated();
            merged_source.warnings = merge_result
                .warnings
                .iter()
                .map(AppWarning::from_core)
                .chain(spreadsheet_warnings)
                .chain(current_source.warnings)
                .chain(candidate_source.warnings)
                .collect();
            let base_blobs = base_source
                .as_ref()
                .map(|source| source.blobs.clone())
                .unwrap_or_default();
            let current_blobs = current_source.blobs.clone();
            let candidate_blobs = candidate_source.blobs.clone();
            merged_source.blobs = merge_blob_envelopes(
                base_blobs.clone(),
                current_blobs.clone(),
                candidate_blobs.clone(),
                &[current_delta.as_slice(), candidate_delta.as_slice()],
            )?;
            restore_referenced_image_blobs(
                &mut merged_source.blobs,
                &merged_source.blocks,
                &[base_blobs, current_blobs, candidate_blobs],
                &mut merged_source.warnings,
            )?;

            self.document = merged_source.to_core()?;
            self.workbook = merged_source.workbook.evaluated();
            self.blobs = merged_source.blobs;
            self.is_open = true;
            self.blob_bytes.clear();
            self.blob_signatures.clear();
            self.blob_tombstones.clear();
            self.blob_tombstone_records.clear();
            self.invalidate_source_state();
            self.operation_envelopes = self.read_operation_envelopes(&repo, &current_manifest)?;
            self.operation_journal = self
                .operation_envelopes
                .iter()
                .map(|envelope| envelope.record.clone())
                .collect();
            self.saved_operation_count = self.operation_envelopes.len();
            self.operation_envelopes.extend(candidate_delta.clone());
            self.operation_journal
                .extend(candidate_delta.into_iter().map(|envelope| envelope.record));
            self.undo_stack.clear();
            self.redo_stack.clear();
            self.repository_root = Some(root.clone());
            self.last_manifest = Some(current_hash.to_string());
            target.save(self, &root)?;
        }
        Err(AppApiError::Conflict(
            "candidate merge loop exceeded safety limit".to_string(),
        ))
    }

    fn read_operation_envelopes<S: ObjectStore>(
        &self,
        repo: &Repository<S>,
        manifest: &ManifestRecord,
    ) -> Result<Vec<AppOperationEnvelope>, AppApiError> {
        let mut operations = Vec::new();
        let mut last_segment = if let Some(parent_hash) = &manifest.parent {
            let parent = repo
                .read_manifest(parent_hash)
                .map_err(|err| AppApiError::Store(err.to_string()))?
                .ok_or_else(|| {
                    AppApiError::NotFound("parent manifest object was not found".to_string())
                })?;
            operations = self.read_operation_envelopes(repo, &parent)?;
            self.previous_operation_segment(repo, Some(parent_hash))?
        } else {
            None
        };
        for segment_hash in &manifest.operation_segments {
            let Some(bytes) = repo.store().get(segment_hash).map_err(|err| match err {
                StoreError::HashMismatch => {
                    AppApiError::Store("operation segment hash mismatch".to_string())
                }
                other => AppApiError::Store(other.to_string()),
            })?
            else {
                return Err(AppApiError::NotFound(
                    "operation segment object was not found".to_string(),
                ));
            };
            let actual = digest_bytes(segment_hash.algorithm(), &bytes)
                .map_err(|err| AppApiError::Model(err.to_string()))?;
            if actual != *segment_hash {
                return Err(AppApiError::Store(
                    "operation segment hash mismatch".to_string(),
                ));
            }
            let segment = Self::decode_operation_segment_object(&bytes)?;
            segment
                .validate()
                .map_err(|err| AppApiError::Format(err.to_string()))?;
            validate_operation_segment_envelopes(&segment.operations)?;
            if segment.document_uuid != manifest.document_uuid || segment.branch != manifest.branch
            {
                return Err(AppApiError::Format(
                    "operation segment document or branch mismatch".to_string(),
                ));
            }
            if segment.previous_segment != last_segment {
                return Err(AppApiError::Format(
                    "operation segment chain mismatch".to_string(),
                ));
            }
            let expected_base_manifest = manifest.parent.as_ref().map(ToString::to_string);
            if segment.base_manifest != expected_base_manifest {
                return Err(AppApiError::Format(
                    "operation segment base manifest mismatch".to_string(),
                ));
            }
            operations.extend(segment.operations);
            last_segment = Some(segment_hash.clone());
        }
        validate_operation_envelopes(&operations)?;
        Ok(operations)
    }

    fn operation_delta_since_base<S: ObjectStore>(
        &self,
        repo: &Repository<S>,
        base: Option<&ManifestRecord>,
        tip: &ManifestRecord,
    ) -> Result<Vec<AppOperationEnvelope>, AppApiError> {
        let base_operations = if let Some(base) = base {
            self.read_operation_envelopes(repo, base)?
        } else {
            Vec::new()
        };
        let tip_operations = self.read_operation_envelopes(repo, tip)?;
        if tip_operations.len() < base_operations.len() {
            return Err(AppApiError::Format(
                "tip operation journal is shorter than base journal".to_string(),
            ));
        }
        if !tip_operations
            .iter()
            .zip(&base_operations)
            .all(|(tip, base)| tip == base)
        {
            return Err(AppApiError::Format(
                "tip operation journal does not extend base journal".to_string(),
            ));
        }
        Ok(tip_operations[base_operations.len()..].to_vec())
    }

    fn candidate_has_unmerged_operations<S: ObjectStore>(
        &self,
        repo: &Repository<S>,
        plan: &CandidateMergePlan,
    ) -> Result<bool, AppApiError> {
        let Some(current_hash) = plan.current.as_ref() else {
            return Ok(true);
        };
        let current_manifest = repo
            .read_manifest(current_hash)
            .map_err(|err| AppApiError::Store(err.to_string()))?
            .ok_or_else(|| AppApiError::NotFound("current manifest was not found".to_string()))?;
        let candidate_manifest = repo
            .read_manifest(&plan.candidate)
            .map_err(|err| AppApiError::Store(err.to_string()))?
            .ok_or_else(|| AppApiError::NotFound("candidate manifest was not found".to_string()))?;
        let base_manifest = plan
            .merge_base
            .as_ref()
            .map(|base| {
                repo.read_manifest(base)
                    .map_err(|err| AppApiError::Store(err.to_string()))?
                    .ok_or_else(|| AppApiError::NotFound("base manifest was not found".to_string()))
            })
            .transpose()?;
        let current_delta =
            self.operation_delta_since_base(repo, base_manifest.as_ref(), &current_manifest)?;
        let candidate_delta =
            self.operation_delta_since_base(repo, base_manifest.as_ref(), &candidate_manifest)?;
        if candidate_delta.is_empty() {
            return Ok(false);
        }
        let mut current_by_id: BTreeMap<String, Vec<&AppOperationEnvelope>> = BTreeMap::new();
        for envelope in &current_delta {
            current_by_id
                .entry(envelope_id_key(envelope))
                .or_default()
                .push(envelope);
        }
        Ok(candidate_delta.iter().any(|envelope| {
            current_by_id
                .get(&envelope_id_key(envelope))
                .map(|current| {
                    !current
                        .iter()
                        .any(|current| envelope_payload_matches(current, envelope))
                })
                .unwrap_or(true)
        }))
    }

    fn read_app_document_at_manifest<S: ObjectStore>(
        &self,
        repo: &Repository<S>,
        manifest_hash: &opendoc_core::HashRef,
    ) -> Result<AppDocument, AppApiError> {
        let manifest = repo
            .read_manifest(manifest_hash)
            .map_err(|err| AppApiError::Store(err.to_string()))?
            .ok_or_else(|| AppApiError::NotFound("manifest object was not found".to_string()))?;
        let snapshot_bytes = repo
            .store()
            .get(&manifest.snapshot)
            .map_err(|err| match err {
                StoreError::HashMismatch => {
                    AppApiError::Store("snapshot hash mismatch".to_string())
                }
                other => AppApiError::Store(other.to_string()),
            })?
            .ok_or_else(|| AppApiError::NotFound("snapshot object was not found".to_string()))?;
        let actual = digest_bytes(manifest.snapshot.algorithm(), &snapshot_bytes)
            .map_err(|err| AppApiError::Model(err.to_string()))?;
        if actual != manifest.snapshot {
            return Err(AppApiError::Store("snapshot hash mismatch".to_string()));
        }
        let snapshot = Self::decode_snapshot_object(&snapshot_bytes)?;
        snapshot
            .validate()
            .map_err(|err| AppApiError::Format(err.to_string()))?;
        validate_snapshot_matches_manifest(&snapshot, &manifest)?;
        if snapshot.source_format != APP_DOCUMENT_FORMAT {
            return Err(AppApiError::Format(format!(
                "unsupported snapshot source format {}",
                snapshot.source_format
            )));
        }
        snapshot.source.validate_source()?;
        Ok(snapshot.source)
    }

    fn previous_operation_segment<S: ObjectStore>(
        &self,
        repo: &Repository<S>,
        manifest_hash: Option<&opendoc_core::HashRef>,
    ) -> Result<Option<opendoc_core::HashRef>, AppApiError> {
        let Some(manifest_hash) = manifest_hash else {
            return Ok(None);
        };
        let manifest = repo
            .read_manifest(manifest_hash)
            .map_err(|err| AppApiError::Store(err.to_string()))?
            .ok_or_else(|| {
                AppApiError::NotFound("parent manifest object was not found".to_string())
            })?;
        if let Some(segment) = manifest.operation_segments.last() {
            return Ok(Some(segment.clone()));
        }
        self.previous_operation_segment(repo, manifest.parent.as_ref())
    }

    fn verify_manifest_blobs<S: ObjectStore>(
        &mut self,
        repo: &Repository<S>,
        manifest: &ManifestRecord,
    ) -> Result<(), AppApiError> {
        let expected = manifest
            .blobs
            .iter()
            .map(ToString::to_string)
            .collect::<BTreeSet<_>>();
        let app = &mut self.app;
        for blob in &mut app.blobs {
            let hash = opendoc_core::HashRef::parse(&blob.hash)
                .map_err(|err| AppApiError::Model(err.to_string()))?;
            if !expected.contains(&blob.hash) {
                blob.available = false;
                app.document.warnings.push(ModelWarning {
                    code: "blob-not-in-manifest".to_string(),
                    message: format!(
                        "blob {} is not referenced by the current manifest",
                        blob.name
                    ),
                });
                continue;
            }
            if let Some(tombstone) = repo
                .read_tombstone(&hash)
                .map_err(|err| AppApiError::Store(err.to_string()))?
            {
                app.blob_tombstone_records
                    .insert(blob.hash.clone(), tombstone.clone());
                app.blob_tombstones.insert(
                    blob.hash.clone(),
                    AppArchiveTombstone::from_record(&tombstone),
                );
            }
            let bytes = repo
                .store()
                .get(&hash)
                .map_err(|err| AppApiError::Store(err.to_string()))?;
            blob.available = bytes.is_some();
            if !blob.available {
                app.document.warnings.push(ModelWarning {
                    code: "missing-blob".to_string(),
                    message: format!(
                        "blob {} is missing and will render as a placeholder",
                        blob.name
                    ),
                });
            } else if let Some(bytes) = bytes {
                let actual = digest_bytes(hash.algorithm(), &bytes)
                    .map_err(|err| AppApiError::Model(err.to_string()))?;
                if actual != hash {
                    blob.available = false;
                    app.document.warnings.push(ModelWarning {
                        code: "blob-hash-mismatch".to_string(),
                        message: format!("blob {} failed content-hash verification", blob.name),
                    });
                } else {
                    app.blob_bytes.insert(blob.hash.clone(), bytes);
                }
            }
            let (signature, warning) =
                Self::read_valid_blob_signature_sidecar(repo, &hash, &blob.hash, &blob.name)?;
            if let Some(warning) = warning {
                app.document.warnings.push(warning);
            }
            if let Some(signature) = signature {
                app.blob_signatures
                    .entry(blob.hash.clone())
                    .or_default()
                    .push(signature);
            }
        }
        for hash in expected {
            if !app.blobs.iter().any(|blob| blob.hash == hash) {
                app.document.warnings.push(ModelWarning {
                    code: "manifest-blob-not-in-snapshot".to_string(),
                    message: format!(
                        "manifest blob {hash} is not present in the snapshot metadata"
                    ),
                });
            }
        }
        Ok(())
    }

    fn read_valid_blob_signature_sidecar<S: ObjectStore>(
        repo: &Repository<S>,
        hash: &opendoc_core::HashRef,
        hash_text: &str,
        blob_name: &str,
    ) -> Result<
        (
            Option<opendoc_format::SignatureRecord>,
            Option<ModelWarning>,
        ),
        AppApiError,
    > {
        let signature = match repo.read_blob_signature(hash) {
            Ok(signature) => signature,
            Err(err) => {
                return Ok((
                    None,
                    Some(ModelWarning {
                        code: "invalid-blob-signature-sidecar".to_string(),
                        message: format!(
                            "blob {blob_name} has an invalid signature sidecar: {err}"
                        ),
                    }),
                ));
            }
        };
        let Some(signature) = signature else {
            return Ok((None, None));
        };
        let state =
            verify_record_for_target_with_public_key(&signature, hash, hash_text.as_bytes())
                .map_err(|err| AppApiError::Sign(err.to_string()))?;
        if state == SignatureState::Broken {
            return Ok((
                None,
                Some(ModelWarning {
                    code: "broken-blob-signature".to_string(),
                    message: format!("blob {blob_name} has a broken signature"),
                }),
            ));
        }
        Ok((Some(signature), None))
    }

    fn read_retained_deleted_blob_sidecars<S: ObjectStore>(
        &mut self,
        repo: &Repository<S>,
    ) -> Result<(), AppApiError> {
        for (hash_text, blob) in self.retained_deleted_blob_refs() {
            let hash = opendoc_core::HashRef::parse(&hash_text)
                .map_err(|err| AppApiError::Model(err.to_string()))?;
            if let Some(bytes) = repo
                .store()
                .get(&hash)
                .map_err(|err| AppApiError::Store(err.to_string()))?
            {
                self.blob_bytes.insert(hash_text.clone(), bytes);
            }
            let (signature, warning) =
                Self::read_valid_blob_signature_sidecar(repo, &hash, &hash_text, &blob.name)?;
            if let Some(warning) = warning {
                self.document.warnings.push(warning);
            }
            if let Some(signature) = signature {
                self.blob_signatures
                    .entry(hash_text.clone())
                    .or_default()
                    .push(signature);
            }
            if let Some(tombstone) = repo
                .read_tombstone(&hash)
                .map_err(|err| AppApiError::Store(err.to_string()))?
            {
                self.blob_tombstone_records
                    .insert(hash_text.clone(), tombstone.clone());
                self.blob_tombstones
                    .insert(hash_text, AppArchiveTombstone::from_record(&tombstone));
            }
        }
        Ok(())
    }

    fn record_recent_document(&mut self, updated_at_ms: u64) {
        let Some(root) = self.repository_root.as_ref() else {
            return;
        };
        let Some(backend) = self.repository_backend.clone() else {
            return;
        };
        let Some(last_manifest) = self.last_manifest.clone() else {
            return;
        };
        let root = root.to_string_lossy().to_string();
        let uuid = self.document.uuid.to_string();
        let title = self.document.title.clone();
        let doi = self.document.doi.clone();
        let namespace = self.repository_namespace.clone();
        self.recent_documents.retain(|recent| {
            !(recent.uuid == uuid
                && recent.repository_root == root
                && recent.repository_backend == backend
                && recent.repository_namespace == namespace)
        });
        self.recent_documents.insert(
            0,
            AppRecentDocument {
                uuid,
                title,
                doi,
                repository_root: root,
                repository_backend: backend,
                repository_namespace: namespace,
                last_manifest: Some(last_manifest),
                updated_at_ms,
            },
        );
        self.recent_documents.truncate(20);
    }

    fn read_signatures<S: ObjectStore>(
        &self,
        repo: &Repository<S>,
        manifest: &ManifestRecord,
        expected_target: &opendoc_core::HashRef,
    ) -> Result<Vec<opendoc_format::SignatureRecord>, AppApiError> {
        let mut signatures = Vec::new();
        for signature_hash in &manifest.signatures {
            let bytes = repo
                .store()
                .get(signature_hash)
                .map_err(|err| match err {
                    StoreError::HashMismatch => {
                        AppApiError::Store("signature hash mismatch".to_string())
                    }
                    other => AppApiError::Store(other.to_string()),
                })?
                .ok_or_else(|| {
                    AppApiError::NotFound("signature object was not found".to_string())
                })?;
            let actual = digest_bytes(signature_hash.algorithm(), &bytes)
                .map_err(|err| AppApiError::Model(err.to_string()))?;
            if actual != *signature_hash {
                return Err(AppApiError::Store("signature hash mismatch".to_string()));
            }
            let signature: opendoc_format::SignatureRecord = opendoc_format::decode_record(&bytes)
                .map_err(|err| AppApiError::Format(err.to_string()))?;
            signature
                .validate()
                .map_err(|err| AppApiError::Format(err.to_string()))?;
            if &signature.target != expected_target {
                return Err(AppApiError::Store(
                    "manifest signature target does not match signing payload".to_string(),
                ));
            }
            signatures.push(signature);
        }
        Ok(signatures)
    }

    pub(crate) fn snapshot_payload(&self) -> Result<Vec<u8>, AppApiError> {
        Self::snapshot_payload_for_document(self.signing_document())
    }

    fn snapshot_payload_for_document(document: AppDocument) -> Result<Vec<u8>, AppApiError> {
        document.validate_source()?;
        let snapshot = SnapshotRecord::new(document.uuid.clone(), APP_DOCUMENT_FORMAT, document);
        snapshot
            .validate()
            .map_err(|err| AppApiError::Format(err.to_string()))?;
        Self::encode_snapshot_object(&snapshot)
    }

    fn encode_snapshot_object(
        snapshot: &SnapshotRecord<AppDocument>,
    ) -> Result<Vec<u8>, AppApiError> {
        snapshot
            .validate()
            .map_err(|err| AppApiError::Format(err.to_string()))?;
        snapshot.source.validate_source()?;
        let source = encode_canonical_cbor(&snapshot.source)
            .map_err(|err| AppApiError::Format(err.to_string()))?;
        let envelope = SnapshotRecord::new(
            snapshot.document_uuid.clone(),
            snapshot.source_format.clone(),
            source,
        );
        envelope
            .validate()
            .map_err(|err| AppApiError::Format(err.to_string()))?;
        Ok(encode_record(&envelope))
    }

    fn decode_snapshot_object(bytes: &[u8]) -> Result<SnapshotRecord<AppDocument>, AppApiError> {
        if let Ok(envelope) = decode_record::<SnapshotRecord<Vec<u8>>>(bytes) {
            envelope
                .validate()
                .map_err(|err| AppApiError::Format(err.to_string()))?;
            let source: AppDocument = decode_cbor(&envelope.source)
                .map_err(|err| AppApiError::Format(err.to_string()))?;
            let snapshot =
                SnapshotRecord::new(envelope.document_uuid, envelope.source_format, source);
            snapshot
                .validate()
                .map_err(|err| AppApiError::Format(err.to_string()))?;
            return Ok(snapshot);
        }
        decode_cbor(bytes).map_err(|err| AppApiError::Format(err.to_string()))
    }

    fn encode_operation_segment_object(
        segment: &OperationSegmentRecord<AppOperationEnvelope>,
    ) -> Result<Vec<u8>, AppApiError> {
        segment
            .validate()
            .map_err(|err| AppApiError::Format(err.to_string()))?;
        validate_operation_segment_envelopes(&segment.operations)?;
        let operations = segment
            .operations
            .iter()
            .map(|operation| {
                encode_canonical_cbor(operation).map_err(|err| AppApiError::Format(err.to_string()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let envelope = OperationSegmentRecord::new(
            segment.document_uuid.clone(),
            segment.branch.clone(),
            segment.previous_segment.clone(),
            segment.base_manifest.clone(),
            operations,
        );
        envelope
            .validate()
            .map_err(|err| AppApiError::Format(err.to_string()))?;
        Ok(encode_record(&envelope))
    }

    fn decode_operation_segment_object(
        bytes: &[u8],
    ) -> Result<OperationSegmentRecord<AppOperationEnvelope>, AppApiError> {
        if let Ok(envelope) = decode_record::<OperationSegmentRecord<Vec<u8>>>(bytes) {
            envelope
                .validate()
                .map_err(|err| AppApiError::Format(err.to_string()))?;
            let operations = envelope
                .operations
                .iter()
                .map(|operation| {
                    decode_cbor(operation).map_err(|err| AppApiError::Format(err.to_string()))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let segment = OperationSegmentRecord::new(
                envelope.document_uuid,
                envelope.branch,
                envelope.previous_segment,
                envelope.base_manifest,
                operations,
            );
            segment
                .validate()
                .map_err(|err| AppApiError::Format(err.to_string()))?;
            return Ok(segment);
        }
        decode_cbor(bytes).map_err(|err| AppApiError::Format(err.to_string()))
    }

    fn signing_document(&self) -> AppDocument {
        Self::signing_document_from_snapshot(self.snapshot_document())
    }

    fn signing_document_from_snapshot(mut document: AppDocument) -> AppDocument {
        document.visible_text.clear();
        document.warnings.clear();
        clear_citation_projection_payload(&mut document.blocks);
        for citation in &mut document.citations.citations {
            citation.rendered_cache = None;
        }
        document.workbook.dependency_graph.clear();
        for sheet in &mut document.workbook.sheets {
            sheet.row_axes.clear();
            sheet.column_axes.clear();
            for cell in &mut sheet.cells {
                cell.computed_kind.clear();
                cell.computed_value.clear();
                cell.dependencies.clear();
            }
        }
        document
    }

    fn snapshot_document(&self) -> AppDocument {
        let mut document = AppDocument::from_core(&self.document);
        document.workbook = self.workbook.evaluated();
        document.blobs = self.blobs.clone();
        document
    }
}

impl Deref for RepositoryService<'_> {
    type Target = OpenDocApp;

    fn deref(&self) -> &Self::Target {
        self.app
    }
}

impl DerefMut for RepositoryService<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.app
    }
}

fn validate_snapshot_matches_manifest(
    snapshot: &SnapshotRecord<AppDocument>,
    manifest: &ManifestRecord,
) -> Result<(), AppApiError> {
    if snapshot.document_uuid != manifest.document_uuid {
        return Err(AppApiError::Format(
            "snapshot document UUID does not match manifest".to_string(),
        ));
    }
    if snapshot.source.uuid != manifest.document_uuid {
        return Err(AppApiError::Format(
            "snapshot source document UUID does not match manifest".to_string(),
        ));
    }
    Ok(())
}

fn lookup_by_doi_or_scan<S: ObjectStore>(
    repo: &Repository<S>,
    doi: &str,
) -> Result<Option<DoiLookupResult>, AppApiError> {
    let normalized = doi.trim();
    if normalized.is_empty() {
        return Ok(None);
    }
    match repo.read_doi_lookup(normalized) {
        Ok(Some(lookup)) => {
            if lookup_has_doi_alias(&lookup, normalized) {
                return Ok(Some(DoiLookupResult {
                    record: lookup,
                    used_scan: false,
                    scan_warnings: Vec::new(),
                }));
            }
        }
        Ok(None) | Err(opendoc_store::StoreError::LookupMismatch) => {}
        Err(err) => return Err(AppApiError::Store(err.to_string())),
    }
    let scan = repo
        .scan_lookup_entries()
        .map_err(|err| AppApiError::Store(err.to_string()))?;
    let scan_warnings = scan
        .invalid
        .iter()
        .map(|problem| {
            format!(
                "DOI lookup scan ignored invalid index {}: {}",
                problem.path, problem.reason
            )
        })
        .collect::<Vec<_>>();
    for lookup in scan.records {
        if lookup_has_doi_alias(&lookup, normalized) {
            return Ok(Some(DoiLookupResult {
                record: lookup,
                used_scan: true,
                scan_warnings,
            }));
        }
    }
    Ok(None)
}

fn read_repository_snapshot_summary<S: ObjectStore>(
    repo: &Repository<S>,
    lookup: &LookupRecord,
) -> Result<(String, Option<String>), AppApiError> {
    let manifest = repo
        .read_manifest(&lookup.manifest)
        .map_err(|err| AppApiError::Store(err.to_string()))?
        .ok_or_else(|| AppApiError::NotFound("manifest object was not found".to_string()))?;
    let snapshot_bytes = repo
        .store()
        .get(&manifest.snapshot)
        .map_err(|err| AppApiError::Store(err.to_string()))?
        .ok_or_else(|| AppApiError::NotFound("snapshot object was not found".to_string()))?;
    let actual = digest_bytes(manifest.snapshot.algorithm(), &snapshot_bytes)
        .map_err(|err| AppApiError::Model(err.to_string()))?;
    if actual != manifest.snapshot {
        return Err(AppApiError::Store("snapshot hash mismatch".to_string()));
    }
    let snapshot = OpenDocApp::decode_snapshot_object(&snapshot_bytes)?;
    snapshot
        .validate()
        .map_err(|err| AppApiError::Format(err.to_string()))?;
    validate_snapshot_matches_manifest(&snapshot, &manifest)?;
    if snapshot.source_format != APP_DOCUMENT_FORMAT {
        return Err(AppApiError::Format(format!(
            "unsupported snapshot source format {}",
            snapshot.source_format
        )));
    }
    let mut title = snapshot.source.title.trim().to_string();
    if title.is_empty() {
        title = lookup.document_uuid.clone();
    }
    Ok((
        title,
        snapshot.source.doi.or_else(|| lookup_doi_alias(lookup)),
    ))
}

fn lookup_doi_alias(lookup: &LookupRecord) -> Option<String> {
    lookup
        .aliases
        .iter()
        .find(|alias| alias.scheme.trim().eq_ignore_ascii_case("doi"))
        .map(|alias| alias.value.clone())
}

struct DoiLookupResult {
    record: LookupRecord,
    used_scan: bool,
    scan_warnings: Vec<String>,
}

fn lookup_has_doi_alias(lookup: &LookupRecord, normalized_doi: &str) -> bool {
    lookup.aliases.iter().any(|alias| {
        alias.scheme.trim().eq_ignore_ascii_case("doi")
            && alias.value.trim().eq_ignore_ascii_case(normalized_doi)
    })
}
