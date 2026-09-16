use super::*;
use std::ops::{Deref, DerefMut};

/// Title of the genesis state a candidate merge with no common ancestor bases
/// on. One constant because both halves of that merge — the document and the
/// workbook — have to start from the same empty place.
const MERGE_GENESIS_TITLE: &str = "Merged OpenDoc";

/// Encode one operation entry for a repository segment.
///
/// `cbor2` builds a canonical value tree while serializing.  An operation can
/// contain a whole inserted block, so that otherwise bounded work needs more
/// stack than Rust gives an individual native test worker.  Repository saves
/// are synchronous either way; on native hosts do this isolated, fallible
/// serialization on a deliberately sized worker rather than making callers
/// (or test runners) choose a larger process-wide stack.  WebAssembly has no
/// equivalent configurable native thread and is left on its runtime stack.
#[cfg(not(target_arch = "wasm32"))]
fn encode_operation_segment_entry(
    operation: &AppOperationEnvelope,
) -> Result<Vec<u8>, AppApiError> {
    let operation = operation.clone();
    std::thread::Builder::new()
        .name("opendoc-operation-encode".to_string())
        .stack_size(8 * 1024 * 1024)
        .spawn(move || {
            encode_canonical_cbor(&operation).map_err(|err| AppApiError::Format(err.to_string()))
        })
        .map_err(|err| AppApiError::Store(format!("could not start operation encoder: {err}")))?
        .join()
        .map_err(|_| AppApiError::Store("operation encoder thread panicked".to_string()))?
}

#[cfg(target_arch = "wasm32")]
fn encode_operation_segment_entry(
    operation: &AppOperationEnvelope,
) -> Result<Vec<u8>, AppApiError> {
    encode_canonical_cbor(operation).map_err(|err| AppApiError::Format(err.to_string()))
}

impl OpenDocApp {
    /// Sign the currently saved version, including its manifest ancestry.
    ///
    /// This is intentionally separate from snapshot signing: a private key is
    /// used for one explicit version-signing action and is not retained while
    /// later saves create different manifests.
    pub fn sign_current_repository_version_with_openssh_private_key(
        &mut self,
        private_key_pem: impl AsRef<[u8]>,
        signer_display: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        self.repository_service()
            .sign_current_repository_version(private_key_pem, signer_display.into())
    }

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

    pub(crate) fn snapshot_document(&self) -> AppDocument {
        let mut document = AppDocument::from_core(&self.document);
        document.workbook = self.workbook.evaluated();
        document.blobs = self.blobs.clone();
        document
    }

    /// Do the document signatures this app holds still cover the state it
    /// holds?
    ///
    /// See [`document_signatures_cover`], which this is the whole-app spelling
    /// of.
    pub(crate) fn document_signatures_cover_current_state(&self) -> bool {
        document_signatures_cover(
            &self.document,
            &self.workbook,
            &self.blobs,
            &self.signatures,
        )
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

    fn sign_current_repository_version(
        &mut self,
        private_key_pem: impl AsRef<[u8]>,
        signer_display: String,
    ) -> Result<AppDocument, AppApiError> {
        if self.has_pending_save_changes() {
            return Err(AppApiError::Conflict(
                "save the document before signing its current version".to_string(),
            ));
        }
        let root = self.repository_root.clone().ok_or_else(|| {
            AppApiError::Conflict("version signing needs an opened or saved repository".to_string())
        })?;
        let manifest = self.last_manifest.clone().ok_or_else(|| {
            AppApiError::Conflict("version signing needs a committed manifest".to_string())
        })?;
        let manifest = opendoc_core::HashRef::parse(&manifest)
            .map_err(|err| AppApiError::Store(err.to_string()))?;
        match self.repository_backend.as_deref() {
            Some("local") => self.sign_repository_version(
                Repository::new(crate::repository::local_object_store(&root)?),
                manifest,
                private_key_pem,
                signer_display,
            ),
            Some("flat") => {
                let namespace = self.repository_namespace.clone().ok_or_else(|| {
                    AppApiError::Conflict(
                        "flat version signing needs a repository namespace".to_string(),
                    )
                })?;
                self.sign_repository_version(
                    Repository::new(
                        FlatObjectStore::new(&root, namespace)
                            .map_err(|err| AppApiError::Store(err.to_string()))?,
                    ),
                    manifest,
                    private_key_pem,
                    signer_display,
                )
            }
            Some(other) => Err(AppApiError::Conflict(format!(
                "version signing does not support repository backend {other}"
            ))),
            None => Err(AppApiError::Conflict(
                "version signing needs a repository backend".to_string(),
            )),
        }
    }

    fn sign_repository_version<S: ObjectStore>(
        &mut self,
        repo: Repository<S>,
        manifest_hash: opendoc_core::HashRef,
        private_key_pem: impl AsRef<[u8]>,
        signer_display: String,
    ) -> Result<AppDocument, AppApiError> {
        let manifest = repo
            .read_manifest(&manifest_hash)
            .map_err(|err| AppApiError::Store(err.to_string()))?
            .ok_or_else(|| {
                AppApiError::NotFound(format!("manifest {manifest_hash} was not found"))
            })?;
        let backend = opendoc_sign::OpenSshSigner::from_private_key_pem(private_key_pem)
            .map_err(|err| AppApiError::Sign(err.to_string()))?;
        let signer = opendoc_sign::Signer {
            key_identity: backend
                .public_key_openssh()
                .map_err(|err| AppApiError::Sign(err.to_string()))?,
            display_name: signer_display,
        };
        let (coverage, signature) =
            opendoc_sign::sign_version(&backend, &manifest, self.document.title.clone(), signer)
                .map_err(|err| AppApiError::Sign(err.to_string()))?;
        repo.write_version_signature(&coverage, &signature)
            .map_err(|err| AppApiError::Store(err.to_string()))?;
        Ok(self.document())
    }

    fn compact_local_repository(
        &mut self,
        root: impl Into<PathBuf>,
        pack_name: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let root = root.into();
        let store = crate::repository::local_object_store(&root)?;
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
        let repo = Repository::new(crate::repository::local_object_store(&root)?);
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
        // A signature that does not cover the snapshot being written is
        // **dropped from this commit**, not written into it and not treated as
        // a reason to refuse the save.
        //
        // It used to be an `Err`, and with `read_signatures` no longer
        // refusing a broken signature at open that would have turned "the
        // document cannot be opened" into "the document cannot be saved" —
        // the same trap one step later. The signature it names still exists in
        // the manifest that *was* signed, which is where the evidence belongs;
        // attaching it to a snapshot it demonstrably does not cover would be a
        // claim, not a record. Editing already clears the list
        // (`invalidate_source_state`), so this only fires for a document
        // opened with a broken signature and saved without being edited.
        let dropped = self
            .signatures
            .iter()
            .filter(|signature| signature.target != signing_target)
            .map(|signature| signature.signer_display.clone())
            .collect::<Vec<_>>();
        self.signatures
            .retain(|signature| signature.target == signing_target);
        for signer in dropped {
            self.push_model_warning(
                "dropped-broken-signature",
                format!(
                    "the signature by {signer} does not cover the state being saved and was not carried into this version"
                ),
            );
        }
        for signature in &self.app.signatures {
            signature
                .validate()
                .map_err(|err| AppApiError::Format(err.to_string()))?;
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
        // The repository now holds the remote work too, so it is no longer a
        // separate class of "settled but unwritten".
        self.remote_operation_count = 0;
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
        self.app.has_unsaved_changes()
    }

    /// Say so when a loaded history's *operation* numbering has interior gaps.
    ///
    /// A repository written before envelope identity and operation identity
    /// were separated has them: one counter numbered both, so every envelope
    /// that carried no typed operation — an undo marker, a blob upload, a
    /// spreadsheet edit — consumed a document-operation id and left a hole.
    ///
    /// The history is read back exactly as it was written. Renumbering it is
    /// not on the table: an operation id is named by other operations' causal
    /// contexts, and both live inside content-addressed objects a manifest
    /// chain already commits to. What is not acceptable is reading it
    /// *silently*, as though it were the dense stream a collaboration service
    /// requires, so this names it instead. Nothing written after the split can
    /// produce this warning.
    ///
    /// Only interior gaps count. A history that starts above sequence 1 is a
    /// compacted repository, not a legacy one.
    fn report_legacy_operation_sequence_gaps(&mut self) {
        let mut by_actor: BTreeMap<&str, BTreeSet<u64>> = BTreeMap::new();
        for operation in self
            .operation_envelopes
            .iter()
            .filter_map(|envelope| envelope.operation.as_ref())
        {
            by_actor
                .entry(operation.id.actor.0.as_str())
                .or_default()
                .insert(operation.id.seq);
        }
        let gaps = by_actor
            .into_iter()
            .filter_map(|(actor, seqs)| {
                let first = *seqs.iter().next()?;
                let last = *seqs.iter().next_back()?;
                let missing = (last - first + 1) as usize - seqs.len();
                (missing > 0).then(|| (actor.to_string(), missing))
            })
            .collect::<Vec<_>>();
        for (actor, missing) in gaps {
            self.push_model_warning(
                "legacy-operation-sequence-gap",
                format!(
                    "actor {actor}'s operation history is missing {missing} sequence number(s); this repository was written before envelope and operation numbering were separated, so it is read as written and cannot be submitted to a collaboration service as one dense stream"
                ),
            );
        }
    }

    /// Continue this actor's numbering after the highest of each kind already
    /// present in the loaded journal, so a reopened document never re-mints an
    /// envelope identity or an operation id the repository already holds.
    ///
    /// Both are taken as a **maximum**, never as a count. A repository written
    /// before the two counters were split has operation sequences with gaps in
    /// them — every undo marker, blob upload and spreadsheet edit of that
    /// session consumed one — and counting the document operations instead of
    /// taking their maximum would re-mint ids the history already contains.
    /// The gaps themselves stay exactly as they were written: renumbering them
    /// would rewrite the causal contexts that name them, and those are inside
    /// signed, content-addressed objects.
    fn rebase_next_seq_from_journal(&mut self) {
        let max_envelope_seq = self
            .operation_journal
            .iter()
            .filter(|record| record.actor == self.actor_id)
            .map(|record| record.seq)
            .max();
        self.next_envelope_seq = max_envelope_seq
            .map_or(1, |seq| seq + 1)
            .max(self.next_envelope_seq);
        let max_operation_seq = self
            .operation_envelopes
            .iter()
            .filter_map(|envelope| envelope.operation.as_ref())
            .filter(|operation| operation.id.actor.0 == self.actor_id)
            .map(|operation| operation.id.seq)
            .max();
        self.next_operation_seq = max_operation_seq
            .map_or(1, |seq| seq + 1)
            .max(self.next_operation_seq);
    }

    pub fn open_saved_projection_by_doi(
        &mut self,
        root: impl Into<PathBuf>,
        doi: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let root = root.into();
        let doi = doi.as_ref().trim().to_string();
        let repo = Repository::new(crate::repository::local_object_store(&root)?);
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
        let repo = Repository::new(crate::repository::local_object_store(&root)?);
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
        let repo = Repository::new(crate::repository::local_object_store(&root)?);
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
        // `merge` owns the dedupe, the cap and the write-back: a scan is a use
        // of every document it found, so the results go to the front in their
        // own order and anything already listed is replaced by the freshly
        // read entry rather than listed twice (`recent.rs`).
        if let Err(err) = self.recent_documents.merge(discovered) {
            self.report_unwritable_recent_documents(&err);
        }
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
        if !is_readable_snapshot_format(&snapshot.source_format) {
            return Err(AppApiError::Format(format!(
                "unsupported snapshot source format {}",
                snapshot.source_format
            )));
        }
        snapshot.source.validate_source()?;
        let snapshot_source_format = snapshot.source_format.clone();
        self.operation_envelopes = self.read_operation_envelopes(&repo, &manifest)?;
        self.operation_journal = self
            .operation_envelopes
            .iter()
            .map(|envelope| envelope.record.clone())
            .collect();
        self.saved_operation_count = self.operation_journal.len();
        self.remote_operation_count = 0;
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
        let signatures = self.read_signatures(
            &repo,
            &head,
            &manifest,
            &signing_target,
            &snapshot_source_format,
        )?;
        self.signatures = signatures;
        self.saved_signature_count = self.signature_count();
        self.invalidate_projection();
        self.repository_root = Some(root);
        self.repository_backend = Some(repository_backend.into());
        self.repository_namespace = repository_namespace;
        self.last_manifest = Some(head.to_string());
        self.undo_stack.clear();
        self.redo_stack.clear();
        // After the document is installed, because that installs the snapshot's
        // own warning list over whatever was there.
        self.report_legacy_operation_sequence_gaps();
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
                None => Document::new(MERGE_GENESIS_TITLE),
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
            // Refuse to replay a delta that crosses a restore the operation
            // log could not describe.
            //
            // The merge below is a replay: `base_document` is the snapshot at
            // the common ancestor and each side's typed operations are folded
            // onto it. That is only sound while every commit in between can be
            // described by operations, and an unlogged restore
            // (`version_service::RESTORE_VERSION_UNLOGGED_KIND`) is exactly the
            // commit that cannot — it replaced state wholesale. Replaying
            // across one merges as though the restore had never happened,
            // which brings the restored-away content back. Saying so is the
            // honest answer; ADR 0003's "degrade gracefully" is about reads,
            // and this is a write that would lose the user's restore.
            if let Some(envelope) =
                current_delta
                    .iter()
                    .chain(candidate_delta.iter())
                    .find(|envelope| {
                        envelope.record.kind
                            == crate::version_service::RESTORE_VERSION_UNLOGGED_KIND
                    })
            {
                return Err(AppApiError::Conflict(format!(
                    "this document was restored to an earlier version in a way the operation log does not describe ({}), so a divergent copy cannot be merged across it automatically",
                    envelope.record.summary
                )));
            }
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
            // A blank workbook when there is no merge base, never the
            // "Prototype Sheet" demo: a three-way merge whose base is demo
            // data resolves every cell of it as "present in the base and
            // deleted by both sides", or worse keeps it. The document half of
            // this merge already bases on `Document::new("Merged OpenDoc")`,
            // and this is the same genesis on the spreadsheet side.
            let (merged_workbook, spreadsheet_warnings) = merge_spreadsheet_envelope_streams(
                base_source
                    .as_ref()
                    .map(|source| source.workbook.clone())
                    .unwrap_or_else(|| OpenDocApp::blank_workbook(MERGE_GENESIS_TITLE)),
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
            self.remote_operation_count = 0;
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
        if !is_readable_snapshot_format(&snapshot.source_format) {
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
        let entry = AppRecentDocument {
            uuid: self.document.uuid.to_string(),
            title: self.document.title.clone(),
            doi: self.document.doi.clone(),
            repository_root: root.to_string_lossy().to_string(),
            repository_backend: backend,
            repository_namespace: self.repository_namespace.clone(),
            last_manifest: Some(last_manifest),
            updated_at_ms,
        };
        // Recording *is* storing: `RecentDocuments::record` dedupes on the
        // target, caps the list and writes it back through whatever store the
        // shell installed, so there is no second call this path could forget.
        if let Err(err) = self.recent_documents.record(entry) {
            self.report_unwritable_recent_documents(&err);
        }
    }

    /// Say so when the recents list could not be stored.
    ///
    /// A save that reached the repository is not undone by a recents list that
    /// could not be rewritten — the document is safe and only the memory of
    /// having opened it is lost — but that is still a fact the user is
    /// entitled to see rather than something to swallow.
    fn report_unwritable_recent_documents(&mut self, err: &str) {
        self.push_model_warning(
            "recent-documents-unwritable",
            format!("the recent-documents list could not be stored: {err}"),
        );
    }

    /// The manifest's document signatures, read as a *report* rather than as
    /// an access gate.
    ///
    /// Every failure here used to be an `Err`, which aborted the open — so a
    /// document whose signature did not match the recomputed payload was
    /// simply unopenable, with its content intact in the store and no way to
    /// reach it. ADR 0003 says the opposite twice: "Unsigned documents are
    /// openable normally. Signatures are a visual trust/compliance indicator,
    /// not a general access gate", and "Signature UI state should support at
    /// least `unsigned`, `signed`, `trusted`, `untrusted`, and `broken`".
    /// `verify_manifest_blobs` had already been written this way — a missing
    /// or mis-hashed blob is a warning and a placeholder — and this is the
    /// same policy applied to the other half of the manifest.
    ///
    /// A signature whose target does not match is **kept**, not dropped: it is
    /// the evidence of who signed what and when, and it is what lets
    /// [`OpenDocApp::document_signatures_cover_current_state`] report `broken`
    /// instead of `unsigned`. A signature that cannot be read at all is
    /// reported and skipped, because there is nothing to keep.
    fn read_signatures<S: ObjectStore>(
        &mut self,
        repo: &Repository<S>,
        manifest_hash: &opendoc_core::HashRef,
        manifest: &ManifestRecord,
        expected_target: &opendoc_core::HashRef,
        snapshot_format: &str,
    ) -> Result<Vec<opendoc_format::SignatureRecord>, AppApiError> {
        let mut signatures = Vec::new();
        // Version signatures are repository evidence, not an access gate.
        // A damaged sidecar or a truncated ancestry must leave the document
        // openable and name the evidence that could not be verified.
        match repo.read_signed_version(manifest_hash) {
            Ok(Some(signed)) => {
                for signature in &signed.signatures {
                    match opendoc_sign::verify_version_signature_with_public_key(
                        signature, manifest,
                    ) {
                        Ok(opendoc_sign::SignatureState::Signed) => {}
                        Ok(state) => self.push_model_warning(
                            "broken-version-signature",
                            format!(
                                "version signature by {} did not verify: {state:?}",
                                signature.signer_display
                            ),
                        ),
                        Err(err) => self.push_model_warning(
                            "unverifiable-version-signature",
                            format!("version signature could not be verified: {err}"),
                        ),
                    }
                }
                match repo.audit_manifest_chain(&signed.coverage) {
                    Ok(audit) if !audit.problems.is_empty() => self.push_model_warning(
                        "signed-version-chain-incomplete",
                        format!("signed version history is incomplete: {:?}", audit.problems),
                    ),
                    Ok(_) => {}
                    Err(err) => self.push_model_warning(
                        "unverifiable-version-chain",
                        format!("signed version history could not be audited: {err}"),
                    ),
                }
            }
            Ok(None) => {}
            Err(err) => self.push_model_warning(
                "unreadable-version-signature",
                format!("version signature sidecars could not be read: {err}"),
            ),
        }
        for signature_hash in &manifest.signatures {
            let bytes = match repo.store().get(signature_hash) {
                Ok(Some(bytes)) => bytes,
                Ok(None) => {
                    self.push_model_warning(
                        "missing-document-signature",
                        format!("signature object {signature_hash} is missing from the repository"),
                    );
                    continue;
                }
                Err(err) => {
                    self.push_model_warning(
                        "unreadable-document-signature",
                        format!("signature object {signature_hash} could not be read: {err}"),
                    );
                    continue;
                }
            };
            let actual = digest_bytes(signature_hash.algorithm(), &bytes)
                .map_err(|err| AppApiError::Model(err.to_string()))?;
            if actual != *signature_hash {
                self.push_model_warning(
                    "document-signature-hash-mismatch",
                    format!("signature object {signature_hash} failed content-hash verification"),
                );
                continue;
            }
            let signature: opendoc_format::SignatureRecord =
                match opendoc_format::decode_record(&bytes) {
                    Ok(signature) => signature,
                    Err(err) => {
                        self.push_model_warning(
                            "invalid-document-signature",
                            format!(
                                "signature object {signature_hash} could not be decoded: {err}"
                            ),
                        );
                        continue;
                    }
                };
            if let Err(err) = signature.validate() {
                self.push_model_warning(
                    "invalid-document-signature",
                    format!("signature object {signature_hash} is not a valid signature: {err}"),
                );
                continue;
            }
            if &signature.target != expected_target {
                // Two different facts wear the same symptom. A target that
                // does not match usually means the source state moved under
                // the signature — that is `broken-document-signature`, and it
                // tells the reader someone altered the document. But it also
                // happens when *this crate's* payload encoding moved: the
                // signature covers a payload shape this build no longer
                // writes, so the target is recomputed over different bytes
                // from a document nothing touched. The snapshot says which
                // case it is, and calling the second one tampering is a lie
                // about the user's document.
                if crate::is_superseded_app_document_format(snapshot_format) {
                    self.push_model_warning(
                        "signature-predates-payload-format",
                        format!(
                            "the signature by {} was made over this document's {snapshot_format} \
                             payload; this build writes {APP_DOCUMENT_FORMAT}, so the signature \
                             cannot be checked against it. This does not mean the document was \
                             altered. Sign again to cover the current format.",
                            signature.signer_display
                        ),
                    );
                } else {
                    self.push_model_warning(
                        "broken-document-signature",
                        format!(
                            "the signature by {} does not cover this document's current source state",
                            signature.signer_display
                        ),
                    );
                }
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
            let source: AppDocument = if envelope.source_format == SERVICE_DOCUMENT_FORMAT {
                // A repository `opendoc-service` wrote. Its snapshot payload is
                // the canonical `opendoc_core::Document` rather than this
                // crate's projection DTO, deliberately (ADR 0015), so the app
                // projects it on the way in. The record envelope around it is
                // byte-identical to the one a local save writes.
                let document: Document = decode_cbor(&envelope.source)
                    .map_err(|err| AppApiError::Format(err.to_string()))?;
                AppDocument::from_core(&document)
            } else {
                decode_cbor(&envelope.source).map_err(|err| AppApiError::Format(err.to_string()))?
            };
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
            .map(encode_operation_segment_entry)
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
                .map(|operation| decode_segment_entry(operation))
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

    /// The bytes a signature covers: the snapshot with every derived value
    /// taken back out of it.
    ///
    /// The list used to live here, and it was incomplete — `word_count`,
    /// `character_count`, `Cell::display_value` and `Cell::spill_source` were
    /// all inside the payload. [`AppDocument::into_source_state`] is now the
    /// one exhaustive statement of what source state is, so a field added to
    /// the projection cannot quietly join the signing payload.
    fn signing_document_from_snapshot(document: AppDocument) -> AppDocument {
        document.into_source_state()
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

/// Do `signatures` cover the source state of this document, workbook and blob
/// list?
///
/// `false` is the `broken` of ADR 0003's signature states: a signature is
/// present but the bytes it names are not the bytes this state would produce
/// now. It is **recomputed, never remembered**, so it cannot go stale — a
/// signature loaded from a repository, one minted here and one carried through
/// a crash recovery all get the same answer, and an edit clears the signature
/// list (`invalidate_source_state`) so the question stops being asked.
///
/// An empty signature list answers `true`: there is nothing that fails to
/// cover anything. The caller decides that the *state* for that case is
/// `unsigned` rather than `signed`.
///
/// It takes the pieces rather than an `OpenDocApp` because the projection
/// service holds exactly these four borrows and no app.
pub(crate) fn document_signatures_cover(
    document: &Document,
    workbook: &AppSpreadsheetWorkbook,
    blobs: &[AppBlobRef],
    signatures: &[opendoc_format::SignatureRecord],
) -> bool {
    if signatures.is_empty() {
        return true;
    }
    let mut snapshot = AppDocument::from_core(document);
    snapshot.workbook = workbook.evaluated();
    snapshot.blobs = blobs.to_vec();
    let Ok(payload) =
        RepositoryService::snapshot_payload_for_document(snapshot.into_source_state())
    else {
        return false;
    };
    let Ok(target) = digest_bytes("sha256", &payload) else {
        return false;
    };
    signatures
        .iter()
        .all(|signature| signature.target == target)
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
    if !is_readable_snapshot_format(&snapshot.source_format) {
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

#[cfg(test)]
mod volume_repository_tests;

/// Snapshot payload formats this crate can open.
///
/// [`APP_DOCUMENT_FORMAT`] is what a local save writes;
/// [`APP_DOCUMENT_FORMATS_READ_ONLY`] are the earlier app payloads that still
/// decode exactly and so are read as written, while a `v0` repository is
/// refused by name because its payload is a different shape (see
/// `APP_DOCUMENT_FORMAT`). A repository
/// `opendoc-service` produced says `opendoc.service-document.v0` and carries
/// the canonical `opendoc_core::Document` instead of the app's source DTO —
/// a deliberate difference (`docs/adr/0015`), and the only one: the record
/// envelope, the manifest chain, the segment chaining rules and the head
/// compare-and-swap are the same on both sides, which is what makes a
/// service-written repository openable here at all.
///
/// Reading it costs the app nothing the service has to pay for: the
/// translation lives on this side, so the service keeps its four-crate
/// dependency list and a network daemon still cannot reach layout, render,
/// import or spreadsheet through it.
fn is_readable_snapshot_format(source_format: &str) -> bool {
    source_format == APP_DOCUMENT_FORMAT
        || source_format == SERVICE_DOCUMENT_FORMAT
        || crate::is_superseded_app_document_format(source_format)
}

/// One operation-segment entry, from either producer.
///
/// A segment record carries no format field, so the entry is identified by its
/// own shape: an app envelope is a map with a `record`, a service entry is the
/// typed operation itself with an `id`. Neither decodes as the other — both
/// would be missing a field with no default — so the fallback is unambiguous
/// rather than a guess.
fn decode_segment_entry(bytes: &[u8]) -> Result<AppOperationEnvelope, AppApiError> {
    if let Ok(envelope) = decode_cbor::<AppOperationEnvelope>(bytes) {
        return Ok(envelope);
    }
    let operation: Operation =
        decode_cbor(bytes).map_err(|err| AppApiError::Format(err.to_string()))?;
    // A service segment carries no envelopes, so this replica has to give each
    // entry an envelope number. The operation's own sequence is the right one:
    // the service keeps an actor's operations dense, so it is unique per actor
    // without this side having to invent a numbering the other side would not
    // recognise on the way back out.
    let envelope_seq = operation.id.seq;
    Ok(AppOperationEnvelope::from_operation(
        operation,
        envelope_seq,
    ))
}

#[cfg(test)]
mod legacy_numbering_tests {
    use crate::OpenDocApp;

    fn temp_root(tag: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "opendoc-legacy-seq-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        root
    }

    /// Rewrite an app's journal the way the single-counter build wrote it:
    /// every envelope numbered from one sequence, and each operation's id
    /// taken from its envelope's number — so an envelope carrying no operation
    /// leaves a hole in the operation numbering.
    ///
    /// This is what the old code produced, reproduced exactly rather than
    /// approximated, because the point of the test is that such a repository
    /// still opens.
    fn collapse_to_one_counter(app: &mut OpenDocApp) {
        for envelope in &mut app.operation_envelopes {
            if let Some(operation) = envelope.operation.as_mut() {
                operation.id.seq = envelope.record.seq;
            }
        }
        app.operation_journal = app
            .operation_envelopes
            .iter()
            .map(|envelope| envelope.record.clone())
            .collect();
    }

    /// A repository written before envelope identity and operation identity
    /// were separated opens, is read exactly as it was written, says that its
    /// operation numbering has holes in it, and can be edited on without
    /// re-minting an id it already holds.
    #[test]
    fn a_repository_written_before_the_split_opens_and_names_its_gaps() {
        let root = temp_root("open");
        let mut app = OpenDocApp::new_empty_document();
        app.dispatch_command("create_document", serde_json::json!({ "title": "Legacy" }))
            .expect("create");
        app.dispatch_command("add_paragraph", serde_json::json!({ "text": "alpha" }))
            .expect("paragraph");
        // The envelope that carried no operation, and therefore burned an
        // operation id under the old numbering.
        app.push_app_operation("marker", "a non-document envelope");
        app.dispatch_command("add_paragraph", serde_json::json!({ "text": "beta" }))
            .expect("paragraph");
        collapse_to_one_counter(&mut app);

        let expected_ids = app
            .operation_envelopes
            .iter()
            .filter_map(|envelope| envelope.operation.as_ref())
            .map(|operation| (operation.id.actor.0.clone(), operation.id.seq))
            .collect::<Vec<_>>();
        let holes = expected_ids
            .windows(2)
            .filter(|pair| pair[1].1 != pair[0].1 + 1)
            .count();
        assert!(holes > 0, "the fixture must actually have a gap in it");
        let uuid = app.document.uuid.to_string();
        app.save_to_local_repository(&root).expect("a local save");

        let mut reopened = OpenDocApp::new_empty_document();
        let document = reopened
            .open_saved_projection(&root, &uuid)
            .expect("a repository written before the split still opens");

        // Read exactly as written: no renumbering, no dropped operation.
        let actual_ids = reopened
            .operation_envelopes
            .iter()
            .filter_map(|envelope| envelope.operation.as_ref())
            .map(|operation| (operation.id.actor.0.clone(), operation.id.seq))
            .collect::<Vec<_>>();
        assert_eq!(actual_ids, expected_ids);

        // And said so, rather than being read as though it were dense.
        let warning = document
            .warnings
            .iter()
            .find(|warning| warning.code == "legacy-operation-sequence-gap")
            .expect("the gap is named");
        assert!(warning.message.contains("read as written"), "{warning:?}");

        // Editing on continues past the highest id the history holds, rather
        // than filling the hole or repeating an id.
        let highest = expected_ids.iter().map(|(_, seq)| *seq).max().unwrap();
        reopened.actor_id = app.actor_id.clone();
        reopened.next_operation_seq = reopened.next_operation_seq.max(highest + 1);
        reopened
            .dispatch_command("add_paragraph", serde_json::json!({ "text": "gamma" }))
            .expect("a local edit on a legacy repository");
        let minted = reopened
            .operation_envelopes
            .iter()
            .filter_map(|envelope| envelope.operation.as_ref())
            .filter(|operation| operation.id.actor.0 == reopened.actor_id)
            .map(|operation| operation.id.seq)
            .max()
            .expect("an operation");
        assert_eq!(minted, highest + 1);

        let _ = std::fs::remove_dir_all(&root);
    }

    /// A repository this build wrote has no gaps to report, so the warning is
    /// not something every user sees.
    #[test]
    fn a_repository_written_after_the_split_reports_no_gaps() {
        let root = temp_root("dense");
        let mut app = OpenDocApp::new_empty_document();
        app.dispatch_command("create_document", serde_json::json!({ "title": "Dense" }))
            .expect("create");
        app.dispatch_command("add_paragraph", serde_json::json!({ "text": "alpha" }))
            .expect("paragraph");
        app.push_app_operation("marker", "a non-document envelope");
        app.dispatch_command("add_paragraph", serde_json::json!({ "text": "beta" }))
            .expect("paragraph");
        let uuid = app.document.uuid.to_string();
        app.save_to_local_repository(&root).expect("a local save");

        let mut reopened = OpenDocApp::new_empty_document();
        let document = reopened
            .open_saved_projection(&root, &uuid)
            .expect("reopening");
        assert!(
            !document
                .warnings
                .iter()
                .any(|warning| warning.code == "legacy-operation-sequence-gap"),
            "{:?}",
            document.warnings
        );

        let _ = std::fs::remove_dir_all(&root);
    }
}

/// Data-integrity properties of the repository boundary: what a snapshot is
/// allowed to fabricate, what a signature is allowed to cover, and what a
/// broken signature is allowed to prevent.
#[cfg(test)]
mod integrity_tests {
    use super::*;
    use serde_json::json;

    const TEST_ED25519_PRIVATE_KEY: &str = r#"
-----BEGIN OPENSSH PRIVATE KEY-----
b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW
QyNTUxOQAAACCzPq7zfqLffKoBDe/eo04kH2XxtSmk9D7RQyf1xUqrYgAAAJgAIAxdACAM
XQAAAAtzc2gtZWQyNTUxOQAAACCzPq7zfqLffKoBDe/eo04kH2XxtSmk9D7RQyf1xUqrYg
AAAEC2BsIi0QwW2uFscKTUUXNHLsYX4FxlaSDSblbAj7WR7bM+rvN+ot98qgEN796jTiQf
ZfG1KaT0PtFDJ/XFSqtiAAAAEHVzZXJAZXhhbXBsZS5jb20BAgMEBQ==
-----END OPENSSH PRIVATE KEY-----
"#;

    fn temp_root(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "opendoc-integrity-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        root
    }

    fn repository(root: &std::path::Path) -> Repository<Box<dyn ObjectStore>> {
        Repository::new(crate::repository::local_object_store(root).expect("a local store"))
    }

    #[test]
    fn version_signing_writes_a_manifest_coverage_sidecar_without_retaining_the_key() {
        let root = temp_root("version-signing");
        let mut app = OpenDocApp::new_empty_document();
        app.dispatch_command("create_document", json!({ "title": "Versioned" }))
            .expect("create");
        app.dispatch_command("add_paragraph", json!({ "text": "signed history" }))
            .expect("paragraph");
        let uuid = app.document.uuid.to_string();
        app.save_to_local_repository(&root).expect("save");
        let manifest = opendoc_core::HashRef::parse(
            app.last_manifest
                .as_deref()
                .expect("the save recorded its manifest"),
        )
        .expect("manifest hash");

        app.sign_current_repository_version_with_openssh_private_key(
            TEST_ED25519_PRIVATE_KEY,
            "Tester",
        )
        .expect("sign saved version");

        let repo = repository(&root);
        let signed = repo
            .read_signed_version(&manifest)
            .expect("read sidecars")
            .expect("version signature");
        assert_eq!(signed.coverage.manifest, manifest);
        assert_eq!(signed.signatures.len(), 1);
        let stored_manifest = repo
            .read_manifest(&manifest)
            .expect("read manifest")
            .expect("manifest exists");
        assert_eq!(
            opendoc_sign::verify_version_signature_with_public_key(
                &signed.signatures[0],
                &stored_manifest,
            )
            .expect("verify"),
            opendoc_sign::SignatureState::Signed
        );

        let mut reopened = OpenDocApp::new_empty_document();
        let reopened = reopened
            .open_saved_projection(&root, &uuid)
            .expect("version-signed document opens");
        assert!(
            !reopened.warnings.iter().any(|warning| {
                matches!(
                    warning.code.as_str(),
                    "broken-version-signature"
                        | "unverifiable-version-signature"
                        | "signed-version-chain-incomplete"
                )
            }),
            "valid version signature must not warn: {:?}",
            reopened.warnings
        );

        app.dispatch_command("add_paragraph", json!({ "text": "not saved" }))
            .expect("edit");
        assert!(app
            .sign_current_repository_version_with_openssh_private_key(
                TEST_ED25519_PRIVATE_KEY,
                "Tester",
            )
            .is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Rewrite the head's snapshot through `edit` and commit the result as a
    /// new head carrying the same signatures.
    ///
    /// This is how a stored document is made to disagree with the signature
    /// over it without touching the signing code: exactly what happened when
    /// the text walk behind `word_count` changed under repositories that were
    /// already signed.
    fn rewrite_head_snapshot(
        root: &std::path::Path,
        uuid: &str,
        edit: impl FnOnce(&mut AppDocument),
    ) {
        let repo = repository(root);
        let head = repo
            .store()
            .read_head(uuid, SNAPSHOT_BRANCH)
            .expect("head read")
            .expect("a head");
        let mut manifest = repo
            .read_manifest(&head)
            .expect("manifest")
            .expect("present");
        let bytes = repo
            .store()
            .get(&manifest.snapshot)
            .expect("snapshot read")
            .expect("a snapshot");
        let mut snapshot =
            RepositoryService::decode_snapshot_object(&bytes).expect("the snapshot decodes");
        edit(&mut snapshot.source);
        let rewritten = RepositoryService::encode_snapshot_object(&snapshot).expect("re-encode");
        let hash = digest_bytes("sha256", &rewritten).expect("digest");
        repo.store()
            .put_if_absent(&hash, &rewritten)
            .expect("store the rewritten snapshot");
        manifest.parent = Some(head.clone());
        manifest.snapshot = hash;
        manifest.operation_segments = Vec::new();
        manifest.created_at_ms += 1;
        repo.commit_manifest(&manifest, Some(&head))
            .expect("commit")
            .expect("the head moved");
    }

    /// Rewrite the head so the stored snapshot declares `format` instead of
    /// the format this build writes, and replace the manifest's signatures
    /// with one made over the payload *that* format produces.
    ///
    /// This is what a repository saved by an earlier build looks like: the
    /// document is untouched, the declared payload format is older, and the
    /// signature covers bytes this build no longer produces.
    fn restate_head_snapshot_as_format(root: &std::path::Path, uuid: &str, format: &str) {
        let repo = repository(root);
        let head = repo
            .store()
            .read_head(uuid, SNAPSHOT_BRANCH)
            .expect("head read")
            .expect("a head");
        let mut manifest = repo
            .read_manifest(&head)
            .expect("manifest")
            .expect("present");
        let bytes = repo
            .store()
            .get(&manifest.snapshot)
            .expect("snapshot read")
            .expect("a snapshot");
        let mut snapshot =
            RepositoryService::decode_snapshot_object(&bytes).expect("the snapshot decodes");
        snapshot.source_format = format.to_string();
        let restated = RepositoryService::encode_snapshot_object(&snapshot).expect("re-encode");
        let snapshot_hash = digest_bytes("sha256", &restated).expect("digest");
        repo.store()
            .put_if_absent(&snapshot_hash, &restated)
            .expect("store the restated snapshot");

        // The payload the earlier build would have signed: its own signing
        // projection of this document, encoded under *its* format string.
        let signing = SnapshotRecord::new(
            snapshot.document_uuid.clone(),
            format,
            RepositoryService::signing_document_from_snapshot(snapshot.source.clone()),
        );
        let signing_payload =
            RepositoryService::encode_snapshot_object(&signing).expect("signing payload");
        let target = digest_bytes("sha256", &signing_payload).expect("digest");
        let backend = opendoc_sign::OpenSshSigner::from_private_key_pem(TEST_ED25519_PRIVATE_KEY)
            .expect("key");
        let signer = opendoc_sign::Signer {
            key_identity: backend.public_key_openssh().expect("public key"),
            display_name: "Tester".to_string(),
        };
        let signature = opendoc_sign::sign_target(
            &backend,
            target,
            snapshot.source.title.clone(),
            signer,
            &signing_payload,
        )
        .expect("sign the earlier payload");
        let signature_bytes = encode_record(&signature);
        let signature_hash = digest_bytes("sha256", &signature_bytes).expect("digest");
        repo.store()
            .put_if_absent(&signature_hash, &signature_bytes)
            .expect("store the signature");

        manifest.parent = Some(head.clone());
        manifest.snapshot = snapshot_hash;
        manifest.signatures = vec![signature_hash];
        manifest.operation_segments = Vec::new();
        manifest.created_at_ms += 1;
        repo.commit_manifest(&manifest, Some(&head))
            .expect("commit")
            .expect("the head moved");
    }

    // ---- 1. a snapshot may not fabricate content --------------------------

    /// A repository `opendoc-service` wrote opens with the document it holds
    /// and *nothing else*.
    ///
    /// `AppDocument::from_core` used to fill `workbook` with
    /// `AppSpreadsheetWorkbook::sample()` — the "Prototype Sheet" demo, six
    /// cells including a `=SUM(B2:B2)` formula. A service snapshot carries the
    /// canonical `opendoc_core::Document` (ADR 0015), so
    /// `decode_snapshot_object` projects it through exactly that constructor
    /// and `open_projection_from_repository` adopted the demo data as the
    /// user's spreadsheet, silently, with no warning — and the next save
    /// committed it inside the snapshot a signature covers.
    #[test]
    fn a_service_written_repository_opens_without_fabricated_spreadsheet_data() {
        let root = temp_root("service-snapshot");
        let mut author = OpenDocApp::new_empty_document();
        author
            .dispatch_command("create_document", json!({ "title": "Service doc" }))
            .expect("create");
        author
            .dispatch_command("add_paragraph", json!({ "text": "written by the service" }))
            .expect("paragraph");
        let uuid = author.document.uuid.to_string();

        // Written the way `opendoc_service::log::DocumentLog::create` writes
        // it: the canonical core document inside a `service-document` snapshot
        // envelope, with the same record framing a local save uses.
        let repo = repository(&root);
        let payload = encode_canonical_cbor(&author.document).expect("canonical cbor");
        let envelope = SnapshotRecord::new(uuid.clone(), SERVICE_DOCUMENT_FORMAT, payload);
        let bytes = encode_record(&envelope);
        let snapshot_hash = digest_bytes("sha256", &bytes).expect("digest");
        repo.store()
            .put_if_absent(&snapshot_hash, &bytes)
            .expect("store the snapshot");
        let manifest = ManifestRecord {
            document_uuid: uuid.clone(),
            branch: SNAPSHOT_BRANCH.to_string(),
            parent: None,
            snapshot: snapshot_hash,
            operation_segments: Vec::new(),
            signatures: Vec::new(),
            blobs: Vec::new(),
            created_at_ms: 1,
        };
        repo.commit_manifest(&manifest, None)
            .expect("commit")
            .expect("a fresh head");

        let mut app = OpenDocApp::new_empty_document();
        let document = app
            .open_saved_projection(&root, &uuid)
            .expect("a service-written repository opens");

        assert!(document.visible_text().contains("written by the service"));
        assert_ne!(
            document.workbook.title, "Prototype Sheet",
            "the demo workbook was injected into the user's document"
        );
        let cells = document
            .workbook
            .sheets
            .iter()
            .map(|sheet| sheet.cells.len())
            .sum::<usize>();
        assert_eq!(cells, 0, "a service snapshot carries no spreadsheet cells");
        assert_eq!(
            document.workbook.sheets.len(),
            1,
            "a document still opens with somewhere to put a formula"
        );

        // And the fabrication cannot reach the store on the next save either.
        app.dispatch_command("add_paragraph", json!({ "text": "and edited here" }))
            .expect("paragraph");
        app.save_to_local_repository(&root).expect("save");
        let mut reopened = OpenDocApp::new_empty_document();
        let after = reopened
            .open_saved_projection(&root, &uuid)
            .expect("reopen");
        assert_eq!(
            after
                .workbook
                .sheets
                .iter()
                .map(|sheet| sheet.cells.len())
                .sum::<usize>(),
            0,
            "the save committed fabricated cells"
        );
    }

    /// The projection constructor itself, directly: there is no path from a
    /// core document to demo data.
    #[test]
    fn projecting_a_core_document_never_produces_demo_spreadsheet_data() {
        let projected = AppDocument::from_core(&Document::new("Anything"));
        assert_ne!(projected.workbook.title, "Prototype Sheet");
        assert!(projected
            .workbook
            .sheets
            .iter()
            .all(|sheet| sheet.cells.is_empty()));
    }

    /// A candidate merge with no common ancestor bases the spreadsheet on a
    /// blank workbook, not on the demo one.
    ///
    /// `merge_repository_candidates_inner` used
    /// `.unwrap_or_else(AppSpreadsheetWorkbook::sample)` as the three-way
    /// merge base whenever `plan_candidate_merges` found no common ancestor,
    /// so a merge between two unrelated chains resolved every cell of the
    /// "Prototype Sheet" demo as content both sides had inherited — and wrote
    /// it into the merged head. The document half of the same merge has always
    /// based on an empty `Document::new`; this is the spreadsheet half of that
    /// same genesis.
    #[test]
    fn a_candidate_merge_with_no_common_ancestor_bases_on_a_blank_workbook() {
        let root = temp_root("merge-base");
        let mut app = OpenDocApp::new_empty_document();
        app.dispatch_command("create_document", json!({ "title": "Divergent" }))
            .expect("create");
        app.dispatch_command("add_paragraph", json!({ "text": "the head" }))
            .expect("paragraph");
        let uuid = app.document.uuid.to_string();
        app.save_to_local_repository(&root)
            .expect("the head commit");

        // A second chain for the same document with nothing in common: written
        // straight into the store as a candidate head whose manifest has no
        // parent at all, which is the shape `plan_candidate_merges` reports as
        // "no merge base".
        let mut other = OpenDocApp::new_empty_document();
        other.document.uuid = app.document.uuid.clone();
        other
            .dispatch_command("add_paragraph", json!({ "text": "the candidate" }))
            .expect("paragraph");
        let repo = repository(&root);
        let snapshot =
            SnapshotRecord::new(uuid.clone(), APP_DOCUMENT_FORMAT, other.snapshot_document());
        let snapshot_bytes =
            RepositoryService::encode_snapshot_object(&snapshot).expect("encode the snapshot");
        let snapshot_hash = digest_bytes("sha256", &snapshot_bytes).expect("digest");
        repo.store()
            .put_if_absent(&snapshot_hash, &snapshot_bytes)
            .expect("store the snapshot");
        let segment = OperationSegmentRecord::new(
            uuid.clone(),
            SNAPSHOT_BRANCH,
            None,
            None,
            other.operation_envelopes.clone(),
        );
        let segment_bytes = RepositoryService::encode_operation_segment_object(&segment)
            .expect("encode the segment");
        let segment_hash = digest_bytes("sha256", &segment_bytes).expect("digest");
        repo.store()
            .put_if_absent(&segment_hash, &segment_bytes)
            .expect("store the segment");
        let manifest = ManifestRecord {
            document_uuid: uuid.clone(),
            branch: SNAPSHOT_BRANCH.to_string(),
            parent: None,
            snapshot: snapshot_hash,
            operation_segments: vec![segment_hash],
            signatures: Vec::new(),
            blobs: Vec::new(),
            created_at_ms: 2,
        };
        match repo
            .commit_manifest_or_candidate(&manifest, None)
            .expect("the unrelated chain lands")
        {
            opendoc_store::CommitOutcome::Candidate { .. } => {}
            opendoc_store::CommitOutcome::Committed(_) => {
                panic!("the fixture must produce a candidate, not a new head")
            }
        }

        let merged = app
            .merge_local_repository_candidates(&root, &uuid)
            .expect("the candidate merges");
        assert_ne!(merged.workbook.title, "Prototype Sheet");
        assert_eq!(
            merged
                .workbook
                .sheets
                .iter()
                .map(|sheet| sheet.cells.len())
                .sum::<usize>(),
            0,
            "the merge base fabricated spreadsheet cells neither side wrote"
        );
    }

    // ---- 2. a signature covers source state, and only source state --------

    /// Two snapshots that differ only in *derived* values sign to the same
    /// bytes.
    ///
    /// This is the property the regression broke from the other side: a change
    /// to how the document's text is walked moved `word_count` and
    /// `character_count`, both of which were inside the signing payload, so
    /// every existing signature stopped matching the document it covered.
    /// `Cell::display_value` and `Cell::spill_source` put a recalculation in
    /// there too — a formula's *result*, which ADR 0003 says explicitly is not
    /// what a signature covers.
    #[test]
    fn the_signing_payload_ignores_every_derived_value() {
        let mut app = OpenDocApp::new_empty_document();
        app.dispatch_command("create_document", json!({ "title": "Signed" }))
            .expect("create");
        app.dispatch_command("add_paragraph", json!({ "text": "one two three" }))
            .expect("paragraph");
        app.dispatch_command(
            "set_spreadsheet_cell",
            json!({ "address": "A1", "value": "2" }),
        )
        .expect("cell");
        app.dispatch_command(
            "set_spreadsheet_cell",
            json!({ "address": "A2", "value": "=A1*3" }),
        )
        .expect("formula");

        let source = app.snapshot_document();
        let mut derived = source.clone();
        derived.word_count = 4242;
        derived.character_count = 4242;
        derived.page_layout.orientation = "landscape".to_string();
        derived.page_layout.size_name = Some("A4".to_string());
        derived.body_fragments.push(AppBodyFragment {
            block_id: "block-x".to_string(),
            blocks: 1,
            html: "<p>rendered</p>".to_string(),
        });
        derived.footnotes_html = "<ol></ol>".to_string();
        derived.header_html = "<header/>".to_string();
        derived.footer_html = "<footer/>".to_string();
        derived.warnings.push(AppWarning {
            code: "some-warning".to_string(),
            message: "a warning is a report about the state".to_string(),
        });
        derived.is_open = false;
        derived.has_unsaved_changes = true;
        derived.operation_count = 9;
        derived.last_manifest = Some("sha256:deadbeef".to_string());
        derived.repository_root = Some("/somewhere/else".to_string());
        derived.workbook.dependency_graph.clear();
        for sheet in &mut derived.workbook.sheets {
            sheet.row_axes.clear();
            sheet.column_axes.clear();
            for cell in &mut sheet.cells {
                cell.computed_kind = "error".to_string();
                cell.computed_value = "#REF!".to_string();
                cell.display_value = "something else entirely".to_string();
                cell.dependencies = vec!["Z99".to_string()];
                cell.spill_source = Some("A1".to_string());
            }
        }

        let signed_source = RepositoryService::snapshot_payload_for_document(
            RepositoryService::signing_document_from_snapshot(source),
        )
        .expect("payload");
        let signed_derived = RepositoryService::snapshot_payload_for_document(
            RepositoryService::signing_document_from_snapshot(derived),
        )
        .expect("payload");
        assert_eq!(
            signed_source, signed_derived,
            "a derived value is inside the signing payload"
        );
    }

    /// A signed document survives a round trip through a change to its derived
    /// values: it opens, and its signature still covers it.
    ///
    /// The stored snapshot is rewritten in place with different counts and
    /// different formula results — which is what a change to the text walk or
    /// to the evaluator does to every repository already on disk — and the
    /// signature, which was not re-made, still matches.
    #[test]
    fn a_signed_document_survives_a_change_to_its_derived_values() {
        let root = temp_root("derived-roundtrip");
        let mut app = OpenDocApp::new_empty_document();
        app.dispatch_command("create_document", json!({ "title": "Compliance" }))
            .expect("create");
        app.dispatch_command("add_paragraph", json!({ "text": "the signed prose" }))
            .expect("paragraph");
        app.dispatch_command(
            "set_spreadsheet_cell",
            json!({ "address": "A1", "value": "=1+1" }),
        )
        .expect("formula");
        let uuid = app.document.uuid.to_string();
        app.save_to_local_repository(&root).expect("save");
        let signed = app
            .sign_with_openssh_private_key(TEST_ED25519_PRIVATE_KEY, "Tester")
            .expect("sign");
        assert_eq!(signed.signature_state, "signed");
        app.save_to_local_repository(&root)
            .expect("save the signature");

        rewrite_head_snapshot(&root, &uuid, |source| {
            source.word_count += 77;
            source.character_count = 0;
            source.page_layout.orientation = "landscape".to_string();
            for sheet in &mut source.workbook.sheets {
                for cell in &mut sheet.cells {
                    cell.computed_value = "42".to_string();
                    cell.display_value = "forty-two".to_string();
                    cell.spill_source = Some("A1".to_string());
                }
            }
        });

        let mut reopened = OpenDocApp::new_empty_document();
        let document = reopened
            .open_saved_projection(&root, &uuid)
            .expect("a signed document whose derived values moved still opens");
        assert_eq!(document.signatures.len(), 1);
        assert!(
            reopened.document_signatures_cover_current_state(),
            "a derived value moved the bytes the signature covers"
        );
        assert!(
            !document
                .warnings
                .iter()
                .any(|warning| warning.code == "broken-document-signature"),
            "{:?}",
            document.warnings
        );
    }

    /// A signature that genuinely does not match is a *report*, not a locked
    /// door: the document opens, the signature is still listed, the state is
    /// recomputed as broken, and the document can still be edited and saved.
    #[test]
    fn a_document_with_a_broken_signature_opens_says_so_and_still_saves() {
        let root = temp_root("broken-signature");
        let mut app = OpenDocApp::new_empty_document();
        app.dispatch_command("create_document", json!({ "title": "Compliance" }))
            .expect("create");
        app.dispatch_command("add_paragraph", json!({ "text": "the signed prose" }))
            .expect("paragraph");
        let uuid = app.document.uuid.to_string();
        app.save_to_local_repository(&root).expect("save");
        app.sign_with_openssh_private_key(TEST_ED25519_PRIVATE_KEY, "Tester")
            .expect("sign");
        app.save_to_local_repository(&root)
            .expect("save the signature");

        // Source state moves under the signature. Nothing derived about a
        // title.
        rewrite_head_snapshot(&root, &uuid, |source| {
            source.title = "Tampered".to_string();
        });

        let mut reopened = OpenDocApp::new_empty_document();
        let document = reopened
            .open_saved_projection(&root, &uuid)
            .expect("a broken signature does not make a document unopenable");
        assert_eq!(document.title, "Tampered");
        assert_eq!(
            document.signatures.len(),
            1,
            "the evidence of who signed is kept, not discarded"
        );
        assert!(
            !reopened.document_signatures_cover_current_state(),
            "this signature does not cover this document"
        );
        let warning = document
            .warnings
            .iter()
            .find(|warning| warning.code == "broken-document-signature")
            .expect("the break is named");
        assert!(warning.message.contains("Tester"), "{warning:?}");

        // And the document is not trapped: it saves, dropping the signature it
        // cannot honestly carry forward.
        let saved = reopened
            .save_to_local_repository(&root)
            .expect("a document with a broken signature still saves");
        assert!(saved.signatures.is_empty());
        assert!(saved
            .warnings
            .iter()
            .any(|warning| warning.code == "dropped-broken-signature"));
    }

    /// A repository written before the payload encoding changed still opens,
    /// and its signature is reported as *unverifiable*, not as tampering.
    ///
    /// `broken-document-signature` says one thing: the source state moved
    /// under the signature. When this crate changes how it encodes
    /// `AppDocument` — which `v1` -> `v2` did, by dropping the `null`s for
    /// unset block fields — the target recomputed at open differs for every
    /// document already on disk, and nothing about any of them moved. Telling
    /// a user their document was altered because our encoder changed is the
    /// defect; the snapshot declares its format, so the reader can tell the
    /// two apart.
    #[test]
    fn a_snapshot_in_an_earlier_payload_format_reports_its_signature_as_unverifiable() {
        let root = temp_root("earlier-payload-format");
        let mut app = OpenDocApp::new_empty_document();
        app.dispatch_command("create_document", json!({ "title": "Compliance" }))
            .expect("create");
        app.dispatch_command("add_paragraph", json!({ "text": "the signed prose" }))
            .expect("paragraph");
        let block = app.document.blocks.last().expect("a block").id.to_string();
        app.dispatch_command(
            "set_block_alignment",
            json!({ "blockId": block, "alignment": "center" }),
        )
        .expect("block formatting");
        let uuid = app.document.uuid.to_string();
        app.save_to_local_repository(&root).expect("save");
        app.sign_with_openssh_private_key(TEST_ED25519_PRIVATE_KEY, "Tester")
            .expect("sign");
        app.save_to_local_repository(&root)
            .expect("save the signature");

        restate_head_snapshot_as_format(&root, &uuid, "opendoc.app-document.v1");

        let mut reopened = OpenDocApp::new_empty_document();
        let document = reopened
            .open_saved_projection(&root, &uuid)
            .expect("a repository from before the format bump still opens");
        // Nothing about the document moved.
        assert_eq!(document.title, "Compliance");
        assert_eq!(
            document.signatures.len(),
            1,
            "the evidence of who signed is kept"
        );
        let warning = document
            .warnings
            .iter()
            .find(|warning| warning.code == "signature-predates-payload-format")
            .unwrap_or_else(|| panic!("the format difference is named: {:?}", document.warnings));
        assert!(
            warning.message.contains("Tester")
                && warning.message.contains("opendoc.app-document.v1")
                && warning.message.contains(APP_DOCUMENT_FORMAT),
            "{warning:?}"
        );
        assert!(
            !document
                .warnings
                .iter()
                .any(|warning| warning.code == "broken-document-signature"),
            "our own encoding change must not be reported as tampering: {:?}",
            document.warnings
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// The same repository, restated under the format this build *does* write,
    /// verifies — so the test above is measuring the format difference and not
    /// some other breakage introduced by restating the head.
    ///
    /// Without this, `restate_head_snapshot_as_format` could be signing the
    /// wrong bytes entirely and the test above would still pass for the wrong
    /// reason.
    #[test]
    fn the_same_restated_head_verifies_when_it_declares_the_current_format() {
        let root = temp_root("restated-current-format");
        let mut app = OpenDocApp::new_empty_document();
        app.dispatch_command("create_document", json!({ "title": "Compliance" }))
            .expect("create");
        app.dispatch_command("add_paragraph", json!({ "text": "the signed prose" }))
            .expect("paragraph");
        let block = app.document.blocks.last().expect("a block").id.to_string();
        app.dispatch_command(
            "set_block_alignment",
            json!({ "blockId": block, "alignment": "center" }),
        )
        .expect("block formatting");
        let uuid = app.document.uuid.to_string();
        app.save_to_local_repository(&root).expect("save");
        app.sign_with_openssh_private_key(TEST_ED25519_PRIVATE_KEY, "Tester")
            .expect("sign");
        app.save_to_local_repository(&root)
            .expect("save the signature");

        restate_head_snapshot_as_format(&root, &uuid, APP_DOCUMENT_FORMAT);

        let mut reopened = OpenDocApp::new_empty_document();
        let document = reopened.open_saved_projection(&root, &uuid).expect("open");
        assert!(
            reopened.document_signatures_cover_current_state(),
            "the restated signature covers the state it was made over: {:?}",
            document.warnings
        );
        assert!(
            !document.warnings.iter().any(|warning| {
                warning.code == "signature-predates-payload-format"
                    || warning.code == "broken-document-signature"
            }),
            "{:?}",
            document.warnings
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// A payload format this build does not read is still refused by name.
    /// Widening the reader to accept `v1` must not widen it to accept
    /// anything.
    #[test]
    fn a_snapshot_in_an_unknown_payload_format_is_still_refused_by_name() {
        let root = temp_root("unknown-payload-format");
        let mut app = OpenDocApp::new_empty_document();
        app.dispatch_command("create_document", json!({ "title": "Compliance" }))
            .expect("create");
        app.dispatch_command("add_paragraph", json!({ "text": "prose" }))
            .expect("paragraph");
        let uuid = app.document.uuid.to_string();
        app.save_to_local_repository(&root).expect("save");

        restate_head_snapshot_as_format(&root, &uuid, "opendoc.app-document.v0");

        let mut reopened = OpenDocApp::new_empty_document();
        let error = reopened
            .open_saved_projection(&root, &uuid)
            .expect_err("a v0 payload is not readable");
        assert!(
            error.to_string().contains("opendoc.app-document.v0"),
            "{error}"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// A signature object the manifest names but the store does not hold is a
    /// warning, exactly as a missing blob is — not a refusal to open.
    #[test]
    fn a_missing_signature_object_degrades_to_a_warning() {
        let root = temp_root("missing-signature");
        let mut app = OpenDocApp::new_empty_document();
        app.dispatch_command("create_document", json!({ "title": "Compliance" }))
            .expect("create");
        app.dispatch_command("add_paragraph", json!({ "text": "prose" }))
            .expect("paragraph");
        let uuid = app.document.uuid.to_string();
        app.save_to_local_repository(&root).expect("save");

        // A manifest that names a signature nothing wrote.
        let repo = repository(&root);
        let head = repo
            .store()
            .read_head(&uuid, SNAPSHOT_BRANCH)
            .expect("head")
            .expect("a head");
        let mut manifest = repo
            .read_manifest(&head)
            .expect("manifest")
            .expect("present");
        manifest.parent = Some(head.clone());
        manifest.operation_segments = Vec::new();
        manifest.signatures = vec![digest_bytes("sha256", b"no such signature").expect("digest")];
        manifest.created_at_ms += 1;
        repo.commit_manifest(&manifest, Some(&head))
            .expect("commit")
            .expect("the head moved");

        let mut reopened = OpenDocApp::new_empty_document();
        let document = reopened
            .open_saved_projection(&root, &uuid)
            .expect("a missing signature object does not make a document unopenable");
        assert!(document.signatures.is_empty());
        assert!(
            document
                .warnings
                .iter()
                .any(|warning| warning.code == "missing-document-signature"),
            "{:?}",
            document.warnings
        );
    }
}
