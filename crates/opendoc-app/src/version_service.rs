//! Version history over the manifest parent chain (PLAN77 phase C).
//!
//! Reads are soft: per ADR 0003 a missing object or a broken link degrades into
//! a `ModelWarning` on the projection rather than an error, so a damaged
//! repository still shows the history it can read. The one write — restore —
//! is strict, because it commits.

use crate::repository::{decode_app_snapshot_object, require_app_snapshot_format};
use crate::version::*;
use crate::version_diff::{block_kind_label, block_text, diff_documents, restore_operations};
use crate::{
    now_ms, restore_referenced_image_blobs, AppApiError, AppBlobRef, AppDocument, AppWarning,
    OpenDocApp, SNAPSHOT_BRANCH,
};
use opendoc_core::{digest_bytes, Document, HashRef};
use opendoc_format::{SnapshotRecord, VersionLabelRecord};
use opendoc_store::{ObjectStore, Repository, StoreError, VersionEntry};
use std::path::Path;

/// Versions listed by default. The panel pages by asking for more.
pub(crate) const DEFAULT_VERSION_LIMIT: usize = 100;

/// Journal kind of a restore the typed operations beside it fully describe.
///
/// Replaying the segment that carries them reproduces the snapshot the same
/// commit wrote, so a candidate that branched before the restore can be merged
/// across it like any other stretch of history.
pub(crate) const RESTORE_VERSION_KIND: &str = "restore-version";

/// Journal kind of a restore the operation log could *not* describe.
///
/// The snapshot is still correct — it is the restored state — but the segment
/// chain no longer replays to it, so the delta across this commit is not a
/// description of what changed. `merge_repository_candidates_inner` refuses to
/// merge a candidate whose delta crosses one of these rather than replaying an
/// incomplete history and resurrecting the restored-away content.
pub(crate) const RESTORE_VERSION_UNLOGGED_KIND: &str = "restore-version-unlogged";

/// Do two documents describe the same source state?
///
/// Warnings are excluded: they are a report *about* the state, produced by
/// whichever path arrived at it, and a merge that degrades an anchor says so
/// while still producing the right document. Everything else — including the
/// uuid — has to match, because this is the check that decides whether the
/// operations just journalled are an honest description of the restore.
fn documents_agree(left: &Document, right: &Document) -> bool {
    let mut left = left.clone();
    let mut right = right.clone();
    left.warnings.clear();
    right.warnings.clear();
    left == right
}

/// The part of a blob list the operation log describes: which blobs are
/// attached, by identity and content hash.
///
/// Not the whole `AppBlobRef`, because `available`, the signature fields and
/// the archive tombstone are all learned from the store at open time rather
/// than restored from a snapshot, and a difference in one of those is not a
/// change a blob operation would have had to journal.
fn blob_identity(blobs: &[AppBlobRef]) -> Vec<(String, String)> {
    let mut identity = blobs
        .iter()
        .map(|blob| (blob.id.clone(), blob.hash.clone()))
        .collect::<Vec<_>>();
    identity.sort();
    identity
}

pub(crate) struct VersionService<'a> {
    app: &'a mut OpenDocApp,
}

impl<'a> VersionService<'a> {
    pub(crate) fn new(app: &'a mut OpenDocApp) -> Self {
        Self { app }
    }

    pub(crate) fn list_versions(
        &mut self,
        limit: Option<usize>,
    ) -> Result<AppVersionView, AppApiError> {
        let (root, repo) = self.app.current_repository("listing versions")?;
        Ok(self.version_view(&root, &repo, limit))
    }

    pub(crate) fn open_at_version(
        &mut self,
        manifest: &str,
    ) -> Result<AppVersionView, AppApiError> {
        let (root, repo) = self.app.current_repository("opening a version")?;
        let hash = parse_manifest_hash(manifest)?;
        let snapshot = self.read_version_snapshot(&repo, &hash)?;
        let mut view = self.version_view(&root, &repo, Some(DEFAULT_VERSION_LIMIT));
        // The preview lines come from the *core* document, because
        // `block_text` is the rule the diff uses and it reads `BlockKind`.
        // A snapshot this runtime cannot project stays previewable with an
        // empty listing and a warning, per ADR 0003.
        let blocks = match snapshot.source.to_core() {
            Ok(document) => document
                .blocks
                .iter()
                .map(|block| AppVersionPreviewBlock {
                    kind: block_kind_label(&block.kind),
                    text: block_text(block),
                })
                .collect(),
            Err(err) => {
                view.warnings.push(AppWarning {
                    code: "version-preview-unprojectable".to_string(),
                    message: format!("this version could not be summarised: {err}"),
                });
                Vec::new()
            }
        };
        view.preview = Some(AppVersionPreview {
            manifest: hash.to_string(),
            // Explicit, not conventional: nothing in this path can produce an
            // editable projection, and the UI must be able to see that.
            read_only: true,
            blocks,
            document: snapshot.source,
        });
        Ok(view)
    }

    pub(crate) fn diff_versions(
        &mut self,
        from_manifest: &str,
        to_manifest: &str,
    ) -> Result<AppVersionView, AppApiError> {
        let (root, repo) = self.app.current_repository("diffing versions")?;
        let from_hash = parse_manifest_hash(from_manifest)?;
        let to_hash = parse_manifest_hash(to_manifest)?;
        let before = self.read_version_document(&repo, &from_hash)?;
        let after = self.read_version_document(&repo, &to_hash)?;
        let entries = diff_documents(&before, &after);
        let count = |change: &str| {
            entries
                .iter()
                .filter(|entry| entry.change == change)
                .count() as u32
        };
        let diff = AppVersionDiff {
            from_manifest: from_hash.to_string(),
            to_manifest: to_hash.to_string(),
            added: count("added"),
            removed: count("removed"),
            changed: count("changed"),
            entries,
        };
        let mut view = self.version_view(&root, &repo, Some(DEFAULT_VERSION_LIMIT));
        view.diff = Some(diff);
        Ok(view)
    }

    pub(crate) fn name_version(
        &mut self,
        manifest: &str,
        label: &str,
        author: &str,
    ) -> Result<AppVersionView, AppApiError> {
        let (root, repo) = self.app.current_repository("naming a version")?;
        let hash = parse_manifest_hash(manifest)?;
        // Refuse to label a manifest this branch does not actually hold, so a
        // label can never point at an object from another document.
        let stored = repo
            .read_manifest(&hash)
            .map_err(|err| AppApiError::Store(err.to_string()))?
            .ok_or_else(|| AppApiError::NotFound("version manifest was not found".to_string()))?;
        if stored.document_uuid != self.app.document.uuid.as_str()
            || stored.branch != SNAPSHOT_BRANCH
        {
            return Err(AppApiError::Conflict(
                "version manifest belongs to a different document or branch".to_string(),
            ));
        }
        let label = label.trim();
        if label.is_empty() {
            return Err(AppApiError::Format("version label is empty".to_string()));
        }
        let record = VersionLabelRecord {
            manifest: hash,
            document_uuid: self.app.document.uuid.to_string(),
            branch: SNAPSHOT_BRANCH.to_string(),
            label: label.to_string(),
            author: author.trim().to_string(),
            created_at_ms: now_ms(),
        };
        repo.write_version_label(&record)
            .map_err(|err| AppApiError::Store(err.to_string()))?;
        Ok(self.version_view(&root, &repo, Some(DEFAULT_VERSION_LIMIT)))
    }

    /// Load an older snapshot into the open document so the next commit records
    /// it as a *new* head. History is appended to, never rewritten.
    pub(crate) fn restore_version(&mut self, manifest: &str) -> Result<AppDocument, AppApiError> {
        let (root, target) = self.app.current_repository_target("restoring a version")?;
        let repo = target.repository(&root)?;
        let hash = parse_manifest_hash(manifest)?;
        let stored = repo
            .read_manifest(&hash)
            .map_err(|err| AppApiError::Store(err.to_string()))?
            .ok_or_else(|| AppApiError::NotFound("version manifest was not found".to_string()))?;
        if stored.document_uuid != self.app.document.uuid.as_str()
            || stored.branch != SNAPSHOT_BRANCH
        {
            return Err(AppApiError::Conflict(
                "version manifest belongs to a different document or branch".to_string(),
            ));
        }
        let snapshot = self.read_version_snapshot(&repo, &hash)?;
        let source = snapshot.source;
        source.validate_source()?;

        // The restore's audit line, wanted before the document moves because
        // the typed operations below are journalled under it.
        let label = repo
            .read_version_label(&hash)
            .ok()
            .flatten()
            .map(|record| record.label);
        let summary = match label {
            Some(label) => format!("restored document from version {hash} (\"{label}\")"),
            None => format!("restored document from version {hash}"),
        };

        self.app.invalidate_projection();
        let restored = source.to_core()?;

        // Say the restore in typed operations, so the committed snapshot and
        // the operation segment beside it describe the same document.
        //
        // They did not. The restore replaced the document wholesale and
        // journalled one untyped marker, so replaying the head's segment chain
        // produced the *pre-restore* document — and
        // `merge_repository_candidates_inner` merges a divergent candidate by
        // replaying exactly that chain onto the snapshot at the common
        // ancestor, so a candidate that branched before the restore brought
        // the restored-away content back.
        //
        // Verified rather than assumed: the plan is applied and the result
        // compared with the restored source. A restore the vocabulary cannot
        // express — and one that moved the workbook or the blob set, neither of
        // which the document operations describe — falls back to replacing the
        // state directly and is recorded as *unlogged*, which
        // `merge_repository_candidates_inner` refuses to merge across instead
        // of silently resurrecting content.
        let planned = restore_operations(&self.app.document, &restored);
        let document_is_logged = match planned {
            Some(operations) if operations.is_empty() => true,
            Some(operations) => {
                self.app
                    .apply_batch(
                        operations
                            .into_iter()
                            .map(|kind| (RESTORE_VERSION_KIND, summary.as_str(), kind))
                            .collect(),
                    )
                    .is_ok()
                    && documents_agree(&self.app.document, &restored)
            }
            None => false,
        };
        // Evaluated on both sides, so the comparison is of the workbooks and
        // not of whether a recalculation happened to have run: the in-memory
        // workbook is evaluated lazily on projection, and a snapshot's is
        // evaluated when it is stored.
        let workbook_is_logged = self.app.workbook.evaluated() == source.workbook.evaluated();
        let blobs_is_logged = blob_identity(&self.app.blobs) == blob_identity(&source.blobs);
        let restore_is_logged = document_is_logged && workbook_is_logged && blobs_is_logged;

        self.app.document = restored;
        self.app.workbook = source.workbook.evaluated();
        self.app.blobs = source.blobs.clone();
        let mut source_warnings = source.warnings.clone();
        restore_referenced_image_blobs(
            &mut self.app.blobs,
            &source.blocks,
            &[source.blobs],
            &mut source_warnings,
        )?;
        self.app.document.warnings = source_warnings
            .iter()
            .map(AppWarning::to_core)
            .collect::<Vec<_>>();

        // The restored content is byte-identical to what the old manifest's
        // signatures cover, so those signatures are still valid and travel with
        // it. Signatures over the content we just replaced are not.
        let signing_payload = self.app.snapshot_payload()?;
        let signing_target = digest_bytes("sha256", &signing_payload)
            .map_err(|err| AppApiError::Model(err.to_string()))?;
        let mut signatures = Vec::new();
        for signature_hash in &stored.signatures {
            match repo.read_signature(signature_hash) {
                Ok(Some(record)) if record.target == signing_target => signatures.push(record),
                Ok(Some(_)) => self.app.push_model_warning(
                    "version-restore-signature-mismatch",
                    format!(
                        "signature {signature_hash} does not cover the restored snapshot and was dropped"
                    ),
                ),
                Ok(None) => self.app.push_model_warning(
                    "version-restore-signature-missing",
                    format!("signature object {signature_hash} is missing from the repository"),
                ),
                Err(err) => self.app.push_model_warning(
                    "version-restore-signature-unreadable",
                    format!("signature object {signature_hash} could not be read: {err}"),
                ),
            }
        }
        self.app.signatures = signatures;

        // The restore names the version it came from in the journal, so the
        // new manifest's operation segment carries its audit trail whether or
        // not the change itself was expressible as operations. The *kind* is
        // the difference: `restore-version` is a restore the operations beside
        // it fully describe, `restore-version-unlogged` is one they do not.
        if restore_is_logged {
            self.app.push_app_operation(RESTORE_VERSION_KIND, &summary);
        } else {
            self.app
                .push_app_operation(RESTORE_VERSION_UNLOGGED_KIND, &summary);
            self.app.push_model_warning(
                "version-restore-not-in-log",
                format!(
                    "the restore of version {hash} replaced state the operation log cannot describe, so a divergent copy of this document cannot be merged across it automatically"
                ),
            );
        }

        // The document's base changed under the undo stack, exactly as it does
        // when a repository is opened, so the stack is discarded rather than
        // left able to replay pre-restore content over restored content. The
        // way back from an unwanted restore is to restore the other version.
        self.app.undo_stack.clear();
        self.app.redo_stack.clear();

        // `last_manifest` deliberately stays on the current head: the commit
        // below parents onto it, so the restored content becomes a new head and
        // every earlier version stays reachable.
        target.save(self.app, &root)
    }

    fn version_view(
        &self,
        root: &Path,
        repo: &Repository<Box<dyn ObjectStore>>,
        limit: Option<usize>,
    ) -> AppVersionView {
        let document_uuid = self.app.document.uuid.to_string();
        let history = repo.list_versions(&document_uuid, SNAPSHOT_BRANCH, limit);
        let current = self.app.last_manifest.clone();
        let versions = history
            .entries
            .iter()
            .map(|entry| project_version(entry, current.as_deref()))
            .collect();
        let warnings = history
            .problems
            .iter()
            .map(|problem| AppWarning {
                code: "version-history-problem".to_string(),
                message: match &problem.manifest {
                    Some(manifest) => format!("{manifest}: {}", problem.reason),
                    None => problem.reason.clone(),
                },
            })
            .collect();
        AppVersionView {
            document_uuid,
            branch: SNAPSHOT_BRANCH.to_string(),
            repository_root: Some(root.to_string_lossy().to_string()),
            repository_backend: self.app.repository_backend.clone(),
            head: history.head.map(|hash| hash.to_string()),
            current,
            versions,
            truncated: history.truncated,
            preview: None,
            diff: None,
            warnings,
        }
    }

    fn read_version_document<S: ObjectStore>(
        &self,
        repo: &Repository<S>,
        manifest: &HashRef,
    ) -> Result<Document, AppApiError> {
        self.read_version_snapshot(repo, manifest)?.source.to_core()
    }

    fn read_version_snapshot<S: ObjectStore>(
        &self,
        repo: &Repository<S>,
        manifest: &HashRef,
    ) -> Result<SnapshotRecord<AppDocument>, AppApiError> {
        let stored = repo
            .read_manifest(manifest)
            .map_err(|err| AppApiError::Store(err.to_string()))?
            .ok_or_else(|| AppApiError::NotFound("version manifest was not found".to_string()))?;
        let bytes = repo
            .store()
            .get(&stored.snapshot)
            .map_err(|err| match err {
                StoreError::HashMismatch => {
                    AppApiError::Store("snapshot hash mismatch".to_string())
                }
                other => AppApiError::Store(other.to_string()),
            })?
            .ok_or_else(|| {
                AppApiError::NotFound("snapshot object for this version was not found".to_string())
            })?;
        let actual = digest_bytes(stored.snapshot.algorithm(), &bytes)
            .map_err(|err| AppApiError::Model(err.to_string()))?;
        if actual != stored.snapshot {
            return Err(AppApiError::Store("snapshot hash mismatch".to_string()));
        }
        let snapshot = decode_app_snapshot_object(&bytes)?;
        snapshot
            .validate()
            .map_err(|err| AppApiError::Format(err.to_string()))?;
        require_app_snapshot_format(&snapshot)?;
        if snapshot.document_uuid != stored.document_uuid {
            return Err(AppApiError::Format(
                "snapshot document uuid does not match its manifest".to_string(),
            ));
        }
        snapshot.source.validate_source()?;
        Ok(snapshot)
    }
}

fn project_version(entry: &VersionEntry, current: Option<&str>) -> AppDocumentVersion {
    let manifest = entry.manifest.to_string();
    AppDocumentVersion {
        is_current: current == Some(manifest.as_str()),
        parent: entry.parent.as_ref().map(ToString::to_string),
        snapshot: entry.snapshot.to_string(),
        created_at_ms: entry.created_at_ms,
        signers: entry
            .signatures
            .iter()
            .map(|signature| AppVersionSigner {
                signer: signature.signer.clone(),
                signer_display: signature.signer_display.clone(),
                title: signature.title.clone(),
                signed_at_ms: signature.signed_at_ms,
            })
            .collect(),
        label: entry.label.as_ref().map(|record| record.label.clone()),
        label_author: entry.label.as_ref().map(|record| record.author.clone()),
        snapshot_present: entry.snapshot_present,
        is_head: entry.is_head,
        manifest,
    }
}

fn parse_manifest_hash(value: &str) -> Result<HashRef, AppApiError> {
    HashRef::parse(value.trim()).map_err(|err| AppApiError::Model(err.to_string()))
}

impl OpenDocApp {
    pub fn list_document_versions(
        &mut self,
        limit: Option<usize>,
    ) -> Result<AppVersionView, AppApiError> {
        VersionService::new(self).list_versions(limit.or(Some(DEFAULT_VERSION_LIMIT)))
    }

    pub fn open_document_at_version(
        &mut self,
        manifest: impl AsRef<str>,
    ) -> Result<AppVersionView, AppApiError> {
        VersionService::new(self).open_at_version(manifest.as_ref())
    }

    pub fn diff_document_versions(
        &mut self,
        from_manifest: impl AsRef<str>,
        to_manifest: impl AsRef<str>,
    ) -> Result<AppVersionView, AppApiError> {
        VersionService::new(self).diff_versions(from_manifest.as_ref(), to_manifest.as_ref())
    }

    pub fn name_document_version(
        &mut self,
        manifest: impl AsRef<str>,
        label: impl AsRef<str>,
        author: impl AsRef<str>,
    ) -> Result<AppVersionView, AppApiError> {
        VersionService::new(self).name_version(manifest.as_ref(), label.as_ref(), author.as_ref())
    }

    pub fn restore_document_version(
        &mut self,
        manifest: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        VersionService::new(self).restore_version(manifest.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opendoc_store::ObjectStoreLayout;
    use std::fs;
    use std::path::PathBuf;

    /// Same fixture key the signing crate's own tests use.
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
        let root =
            std::env::temp_dir().join(format!("opendoc-version-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        root
    }

    /// A document committed three times, with one more paragraph each time.
    fn app_with_three_versions(root: &PathBuf) -> OpenDocApp {
        let mut app = OpenDocApp::new_sample();
        app.new_document("Version test");
        app.add_paragraph("first").expect("paragraph");
        app.save_to_local_repository(root)
            .expect("first commit succeeds");
        app.add_paragraph("second").expect("paragraph");
        app.save_to_local_repository(root)
            .expect("second commit succeeds");
        app.add_paragraph("third").expect("paragraph");
        app.save_to_local_repository(root)
            .expect("third commit succeeds");
        app
    }

    #[test]
    fn lists_versions_newest_first_from_the_manifest_chain() {
        let root = temp_root("list");
        let mut app = app_with_three_versions(&root);

        let view = app
            .list_document_versions(None)
            .expect("history is readable");
        assert_eq!(view.versions.len(), 3);
        assert_eq!(view.branch, "main");
        assert!(view.warnings.is_empty(), "{:?}", view.warnings);
        assert!(!view.truncated);
        assert!(view.versions[0].is_head);
        assert!(view.versions[0].is_current);
        assert!(!view.versions[1].is_head);
        assert_eq!(view.head, view.current);
        assert_eq!(
            view.versions[0].parent.as_deref(),
            Some(view.versions[1].manifest.as_str())
        );
        assert_eq!(view.versions[2].parent, None);
        assert!(view.versions.iter().all(|version| version.snapshot_present));
        assert!(view
            .versions
            .iter()
            .all(|version| version.signers.is_empty()));
        assert!(view.preview.is_none() && view.diff.is_none());

        assert_eq!(
            app.list_document_versions(Some(2))
                .expect("limited history is readable")
                .versions
                .len(),
            2
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn opening_a_version_is_read_only_and_leaves_the_open_document_alone() {
        let root = temp_root("preview");
        let mut app = app_with_three_versions(&root);
        let oldest = app.list_document_versions(None).expect("history").versions[2]
            .manifest
            .clone();

        app.add_paragraph("unsaved work").expect("paragraph");
        let before = app.document();
        assert!(before.has_unsaved_changes);

        let view = app
            .open_document_at_version(&oldest)
            .expect("an ancestor version opens");
        let preview = view.preview.expect("preview is populated");
        assert!(preview.read_only, "read-only is explicit in the projection");
        assert_eq!(preview.manifest, oldest);
        let preview_text: Vec<String> = preview
            .document
            .blocks
            .iter()
            .map(|block| {
                block
                    .content
                    .iter()
                    .map(|inline| inline.text.clone())
                    .collect()
            })
            .collect();
        assert!(preview_text.iter().any(|text| text == "first"));
        assert!(
            !preview_text.iter().any(|text| text == "third"),
            "the oldest version predates the third paragraph"
        );

        // The panel reads `preview.blocks`, not the block tree: which of a
        // block's content, its equation source or its alt text stands in for
        // it is a projection rule, and it is the diff's rule, applied here.
        assert_eq!(preview.blocks.len(), preview.document.blocks.len());
        assert!(preview
            .blocks
            .iter()
            .any(|block| block.kind == "paragraph" && block.text == "first"));
        assert!(preview.blocks.iter().all(|block| !block.kind.is_empty()));

        // Nothing about the open document moved.
        let after = app.document();
        assert_eq!(after.blocks.len(), before.blocks.len());
        assert_eq!(after.operation_count, before.operation_count);
        assert_eq!(after.last_manifest, before.last_manifest);
        assert!(after.has_unsaved_changes);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn naming_a_version_stores_a_sidecar_without_rewriting_the_chain() {
        let root = temp_root("name");
        let mut app = app_with_three_versions(&root);
        let before = app.list_document_versions(None).expect("history");
        let oldest = before.versions[2].manifest.clone();

        let view = app
            .name_document_version(&oldest, "  First draft  ", "Ada")
            .expect("naming succeeds");
        assert_eq!(view.versions.len(), 3);
        assert_eq!(view.versions[2].label.as_deref(), Some("First draft"));
        assert_eq!(view.versions[2].label_author.as_deref(), Some("Ada"));
        assert_eq!(view.head, before.head, "naming never moves the head");
        let manifests: Vec<_> = view
            .versions
            .iter()
            .map(|version| version.manifest.clone())
            .collect();
        let previous: Vec<_> = before
            .versions
            .iter()
            .map(|version| version.manifest.clone())
            .collect();
        assert_eq!(manifests, previous, "naming never rewrites a manifest");

        assert!(app.name_document_version(&oldest, "   ", "Ada").is_err());
        assert!(app
            .name_document_version("sha256:deadbeef", "Nope", "Ada")
            .is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn diffs_two_versions_at_block_level() {
        let root = temp_root("diff");
        let mut app = app_with_three_versions(&root);
        let view = app.list_document_versions(None).expect("history");
        let oldest = view.versions[2].manifest.clone();
        let head = view.versions[0].manifest.clone();

        let diffed = app
            .diff_document_versions(&oldest, &head)
            .expect("diff succeeds");
        let diff = diffed.diff.expect("diff is populated");
        assert_eq!(diff.from_manifest, oldest);
        assert_eq!(diff.to_manifest, head);
        assert_eq!(diff.added, 2, "two paragraphs were appended");
        assert_eq!(diff.removed, 0);
        assert_eq!(diff.changed, 0);
        let added: Vec<_> = diff
            .entries
            .iter()
            .map(|entry| entry.after_text.as_str())
            .collect();
        assert!(added.contains(&"second"));
        assert!(added.contains(&"third"));
        assert!(diff.entries.iter().all(|entry| entry.change == "added"));

        // Reversing the direction turns additions into removals.
        let reversed = app
            .diff_document_versions(&head, &oldest)
            .expect("reverse diff succeeds")
            .diff
            .expect("diff is populated");
        assert_eq!(reversed.removed, 2);
        assert_eq!(reversed.added, 0);

        // A version against itself has nothing to report.
        assert!(app
            .diff_document_versions(&head, &head)
            .expect("self diff succeeds")
            .diff
            .expect("diff is populated")
            .entries
            .is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn restoring_commits_a_new_head_and_leaves_the_repository_signable() {
        let root = temp_root("restore");
        let mut app = app_with_three_versions(&root);
        let view = app.list_document_versions(None).expect("history");
        let oldest = view.versions[2].manifest.clone();
        let head_before = view.versions[0].manifest.clone();

        let restored = app
            .restore_document_version(&oldest)
            .expect("restore succeeds");
        assert!(!restored.has_unsaved_changes, "restore commits");
        let restored_text: Vec<String> = restored
            .blocks
            .iter()
            .map(|block| {
                block
                    .content
                    .iter()
                    .map(|inline| inline.text.clone())
                    .collect()
            })
            .collect();
        assert!(restored_text.iter().any(|text| text == "first"));
        assert!(!restored_text.iter().any(|text| text == "third"));

        // The restore is an ordinary journalled operation naming its source.
        let record = restored
            .operations
            .iter()
            .find(|operation| operation.kind == "restore-version")
            .expect("the restore is in the operation journal");
        assert!(record.summary.contains(&oldest), "{}", record.summary);

        // History grew; nothing was rewritten.
        let after = app.list_document_versions(None).expect("history");
        assert_eq!(after.versions.len(), 4);
        assert!(after.versions[0].is_head && after.versions[0].is_current);
        assert_eq!(
            after.versions[0].parent.as_deref(),
            Some(head_before.as_str()),
            "the restore parents onto the previous head"
        );
        assert!(after.warnings.is_empty(), "{:?}", after.warnings);
        assert!(after
            .versions
            .iter()
            .any(|version| version.manifest == oldest));
        assert!(after
            .versions
            .iter()
            .any(|version| version.manifest == head_before));

        // The chain still validates end to end and signing still works on it.
        assert_eq!(
            app.verify_current_signatures().expect("verification runs"),
            "unsigned"
        );
        app.sign_with_openssh_private_key(TEST_ED25519_PRIVATE_KEY, "Ada")
            .expect("signing after a restore succeeds");
        assert_eq!(
            app.verify_current_signatures().expect("verification runs"),
            "signed"
        );
        app.save_to_local_repository(&root)
            .expect("the signed restore commits");

        let signed = app.list_document_versions(None).expect("history");
        assert_eq!(signed.versions.len(), 5);
        assert_eq!(signed.versions[0].signers.len(), 1);
        assert_eq!(signed.versions[0].signers[0].signer_display, "Ada");
        assert!(signed
            .versions
            .iter()
            .all(|version| version.snapshot_present));

        // Reopening from the repository lands on the restored content.
        let mut reopened = OpenDocApp::new_sample();
        let opened = reopened
            .open_saved_projection(&root, restored.uuid.clone())
            .expect("the restored head reopens");
        assert_eq!(opened.signature_state, "signed");
        let opened_text: Vec<String> = opened
            .blocks
            .iter()
            .map(|block| {
                block
                    .content
                    .iter()
                    .map(|inline| inline.text.clone())
                    .collect()
            })
            .collect();
        assert!(!opened_text.iter().any(|text| text == "third"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_broken_chain_warns_instead_of_failing_to_list() {
        let root = temp_root("broken");
        let mut app = app_with_three_versions(&root);
        let view = app.list_document_versions(None).expect("history");
        let middle = HashRef::parse(&view.versions[1].manifest).expect("manifest hash parses");

        fs::remove_file(root.join(ObjectStoreLayout::object_key(&middle)))
            .expect("the middle manifest object is removable");

        let degraded = app
            .list_document_versions(None)
            .expect("a broken chain still lists");
        assert_eq!(degraded.versions.len(), 1, "the walk stops at the gap");
        assert!(degraded.truncated);
        assert_eq!(degraded.warnings.len(), 1);
        assert_eq!(degraded.warnings[0].code, "version-history-problem");
        assert!(
            degraded.warnings[0]
                .message
                .contains("manifest object is missing"),
            "{}",
            degraded.warnings[0].message
        );

        // A missing snapshot is softer still: the version stays listed, but is
        // marked unopenable, and asking to open it is a clean error.
        let root2 = temp_root("broken-snapshot");
        let mut app2 = app_with_three_versions(&root2);
        let view2 = app2.list_document_versions(None).expect("history");
        let oldest2 = view2.versions[2].manifest.clone();
        let snapshot2 = HashRef::parse(&view2.versions[2].snapshot).expect("snapshot hash parses");
        fs::remove_file(root2.join(ObjectStoreLayout::object_key(&snapshot2)))
            .expect("the snapshot object is removable");

        let soft = app2
            .list_document_versions(None)
            .expect("a missing snapshot still lists");
        assert_eq!(soft.versions.len(), 3, "the whole chain is still listed");
        assert!(!soft.truncated);
        assert!(!soft.versions[2].snapshot_present);
        assert!(soft
            .warnings
            .iter()
            .any(|warning| warning.message.contains("snapshot object is missing")));
        assert!(app2.open_document_at_version(&oldest2).is_err());
        assert!(app2.restore_document_version(&oldest2).is_err());

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(root2);
    }

    #[test]
    fn version_commands_need_a_repository_and_a_known_manifest() {
        let mut app = OpenDocApp::new_sample();
        app.new_document("Unsaved");
        assert!(app.list_document_versions(None).is_err());

        let root = temp_root("unknown");
        let mut app = app_with_three_versions(&root);
        assert!(app.open_document_at_version("not-a-hash").is_err());
        assert!(app
            .restore_document_version("sha256:0000000000000000")
            .is_err());
        let _ = fs::remove_dir_all(root);
    }
}

/// The restore's other half: the operation log it leaves behind.
///
/// A commit is a snapshot *and* a segment of the operation chain that produced
/// it, and `merge_repository_candidates_inner` merges a divergent copy by
/// replaying that chain onto the snapshot at the common ancestor. So the two
/// have to describe the same document, and a restore is the one write that can
/// make them disagree.
#[cfg(test)]
mod restore_log_tests {
    use super::*;
    use crate::{merge_operations_from_envelopes, OpenDocApp};
    use opendoc_merge::merge_operations;
    use std::fs;
    use std::path::PathBuf;

    fn temp_root(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "opendoc-restore-log-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&root);
        root
    }

    fn block_texts(document: &Document) -> Vec<String> {
        document.blocks.iter().map(block_text).collect()
    }

    /// Open the repository fresh and replay every operation it holds onto the
    /// genesis document, then compare with the snapshot the head committed.
    ///
    /// This is the invariant in one line: the head's snapshot is what the head's
    /// operation chain replays to.
    ///
    /// Two assertions, not one. The block text is asserted first because its
    /// failure message is readable; whole-document equality is what actually
    /// carries the invariant, because a restore that moved a heading level, a
    /// link target, the title or the page geometry and nothing else leaves the
    /// text identical on both sides. Warnings are excluded on both sides for
    /// the reason `documents_agree` gives: they are a report *about* the state
    /// produced by whichever path arrived at it, not part of it.
    fn assert_log_replays_to_the_snapshot(root: &PathBuf, uuid: &str, genesis: &Document) {
        let mut reopened = OpenDocApp::new_empty_document();
        reopened
            .open_saved_projection(root, uuid)
            .expect("the repository opens");
        let replayed = merge_operations(
            genesis,
            &[merge_operations_from_envelopes(
                &reopened.operation_envelopes,
            )],
        )
        .expect("the chain replays");
        assert_eq!(
            block_texts(&replayed.document),
            block_texts(&reopened.document),
            "the committed snapshot is not what the operation log replays to"
        );
        let mut replayed_document = replayed.document;
        let mut committed = reopened.document;
        replayed_document.warnings.clear();
        committed.warnings.clear();
        assert_eq!(
            replayed_document, committed,
            "the committed snapshot and the operation log agree on the text but not on the rest of the document"
        );
    }

    /// A restore leaves the snapshot and the operation log describing the same
    /// document.
    ///
    /// Before the fix the restore replaced the document wholesale and
    /// journalled one untyped `restore-version` marker, so this walk produced
    /// the *pre-restore* blocks while the head snapshot held the restored
    /// ones. The control assertion before the restore is what proves the walk
    /// itself is right.
    #[test]
    fn a_restore_leaves_the_operation_log_replaying_to_the_committed_snapshot() {
        let root = temp_root("agree");
        let mut app = OpenDocApp::new_empty_document();
        app.new_document("Replay test");
        let genesis = app.document.clone();
        let uuid = genesis.uuid.to_string();

        app.add_paragraph("version one").expect("paragraph");
        app.save_to_local_repository(&root).expect("first commit");
        let first = app.last_manifest.clone().expect("a manifest");
        app.add_paragraph("version two").expect("paragraph");
        app.save_to_local_repository(&root).expect("second commit");

        // Control: without a restore the two already agree, so a failure below
        // is the restore and not the walk.
        assert_log_replays_to_the_snapshot(&root, &uuid, &genesis);

        let restored = app
            .restore_document_version(&first)
            .expect("the version restores");
        assert!(
            !restored.visible_text().contains("version two"),
            "the restore did not roll the document back"
        );
        assert_log_replays_to_the_snapshot(&root, &uuid, &genesis);

        // And the restore is in the log as a restore, with typed operations
        // beside it rather than in place of them.
        let kinds = app
            .operation_journal
            .iter()
            .map(|record| record.kind.as_str())
            .collect::<Vec<_>>();
        assert!(
            kinds.contains(&RESTORE_VERSION_KIND),
            "the restore is not named in the journal: {kinds:?}"
        );
        assert!(
            !kinds.contains(&RESTORE_VERSION_UNLOGGED_KIND),
            "this restore is expressible and must not be recorded as unlogged"
        );
    }

    /// The same invariant for a restore that changes nothing a reader would
    /// call text.
    ///
    /// A heading's level, a link's target, the document title and the page
    /// geometry are all outside `block_text`, so a restore that lost any of
    /// them would replay to a document whose *text* still matched the
    /// snapshot. This is the case where only whole-document equality can
    /// fail, and it is asserted explicitly that the text half cannot.
    #[test]
    fn a_restore_of_structure_beneath_the_text_also_replays_to_the_snapshot() {
        let root = temp_root("structure");
        let mut app = OpenDocApp::new_empty_document();
        app.new_document("Structured restore");
        let genesis = app.document.clone();
        let uuid = genesis.uuid.to_string();

        app.add_heading("Chapter one", 1).expect("heading");
        let heading_id = app
            .document
            .blocks
            .last()
            .expect("the heading")
            .id
            .to_string();
        app.add_link("OpenDoc", "https://example.org/before")
            .expect("link");
        app.save_to_local_repository(&root).expect("first commit");
        let first = app.last_manifest.clone().expect("a manifest");
        let text_before = block_texts(&app.document);

        // Four changes, none of them visible to `block_text`.
        app.update_heading_level(&heading_id, 4)
            .expect("heading level");
        app.set_document_title("Structured restore, revised")
            .expect("title");
        app.set_document_locale("sv-SE").expect("locale");
        app.set_page_setup(11906, 16838, 1440, 1440, 1440, 1440)
            .expect("page setup");
        app.save_to_local_repository(&root).expect("second commit");
        assert_eq!(
            block_texts(&app.document),
            text_before,
            "the fixture must change only what block text cannot see"
        );
        assert_log_replays_to_the_snapshot(&root, &uuid, &genesis);

        let restored = app
            .restore_document_version(&first)
            .expect("the version restores");
        assert_eq!(restored.title, "Structured restore");
        assert_eq!(restored.locale, "en-US");
        assert_eq!(
            block_texts(&app.document),
            text_before,
            "the restore must leave the text where it already was"
        );
        assert_log_replays_to_the_snapshot(&root, &uuid, &genesis);

        assert_eq!(
            app.operation_journal
                .last()
                .expect("a journal entry")
                .kind
                .as_str(),
            RESTORE_VERSION_KIND,
            "a heading level, a title, a locale and a page setup are all in the vocabulary"
        );
    }

    /// A restore the operation vocabulary cannot express is recorded as such —
    /// and a divergent copy is refused rather than merged across it.
    ///
    /// A comment thread is the case: `DeleteCommentThread` marks a thread
    /// deleted, it does not unmake it, so rolling back to before a thread
    /// existed has no operation. The snapshot is still right; the delta across
    /// it is not a description of the change, and replaying it would bring the
    /// restored-away content back.
    #[test]
    fn an_unloggable_restore_is_named_and_blocks_a_replay_merge() {
        let root = temp_root("unlogged");
        let mut app = OpenDocApp::new_empty_document();
        app.new_document("Unlogged restore");
        let uuid = app.document.uuid.to_string();
        app.add_paragraph("before the comment").expect("paragraph");
        app.save_to_local_repository(&root).expect("first commit");
        let before_comment = app.last_manifest.clone().expect("a manifest");
        app.add_comment("Reviewer", "a thread that cannot be unmade")
            .expect("comment");
        app.save_to_local_repository(&root).expect("second commit");

        // A second replica at the same head, before the restore moves it.
        let mut other = OpenDocApp::new_empty_document();
        other
            .open_saved_projection(&root, &uuid)
            .expect("the second replica opens");

        let restored = app
            .restore_document_version(&before_comment)
            .expect("the version restores");
        assert!(restored.comments.is_empty(), "the comment is gone");
        assert_eq!(
            app.operation_journal
                .last()
                .expect("a journal entry")
                .kind
                .as_str(),
            RESTORE_VERSION_UNLOGGED_KIND
        );
        assert!(
            restored
                .warnings
                .iter()
                .any(|warning| warning.code == "version-restore-not-in-log"),
            "{:?}",
            restored.warnings
        );

        // The second replica diverges and lands as a candidate.
        other.add_paragraph("concurrent work").expect("paragraph");
        other
            .save_to_local_repository_or_candidate(&root)
            .expect("a candidate commit");

        let error = app
            .merge_local_repository_candidates(&root, &uuid)
            .expect_err("a replay merge across an unlogged restore is refused");
        assert!(
            matches!(&error, AppApiError::Conflict(message) if message.contains("restored")),
            "{error:?}"
        );
    }
}
