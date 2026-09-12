//! Projection DTOs for the version-history surface (PLAN77 phase C).
//!
//! These are pure projections over manifests, their sidecars and their
//! snapshots. Nothing here mutates repository state; `version_service` owns the
//! reads and the one write (restore) that the panel can trigger.

use crate::{AppDocument, AppWarning};
use serde::{Deserialize, Serialize};

/// A signer of one committed version, projected from the manifest's signature
/// objects.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppVersionSigner {
    pub signer: String,
    pub signer_display: String,
    pub title: String,
    pub signed_at_ms: u64,
}

/// One entry in the manifest parent chain.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppDocumentVersion {
    pub manifest: String,
    pub parent: Option<String>,
    pub snapshot: String,
    pub created_at_ms: u64,
    pub signers: Vec<AppVersionSigner>,
    pub label: Option<String>,
    pub label_author: Option<String>,
    /// Whether the snapshot object still exists. A version whose snapshot is
    /// gone is listed for audit purposes but cannot be opened or restored.
    pub snapshot_present: bool,
    /// This version is the repository's current branch head.
    pub is_head: bool,
    /// This version is the one the open document was loaded from or last saved
    /// as. It differs from `is_head` when another writer advanced the branch.
    pub is_current: bool,
}

/// A version opened for reading only.
///
/// `read_only` is always `true`: it is carried explicitly so the UI disables
/// editing because the projection said so, not because of a convention that a
/// later change could silently break.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppVersionPreview {
    pub manifest: String,
    pub read_only: bool,
    pub document: AppDocument,
}

/// One block-level change between two versions.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppVersionDiffEntry {
    /// `added`, `removed` or `changed`.
    pub change: String,
    pub block_id: String,
    /// Block kind label of the surviving side (`paragraph`, `heading`, …).
    pub kind: String,
    /// Position of the block in the tree, e.g. `3` or `2 › row 1 › cell 2 › 0`.
    pub path: String,
    pub before_text: String,
    pub after_text: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppVersionDiff {
    pub from_manifest: String,
    pub to_manifest: String,
    pub added: u32,
    pub removed: u32,
    pub changed: u32,
    pub entries: Vec<AppVersionDiffEntry>,
}

/// The version panel's whole view of the repository.
///
/// Every version command returns this shape so the panel is always consistent
/// after any of them: `versions` is always populated, `preview` is set by
/// `open_document_at_version` and `diff` by `diff_document_versions`.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppVersionView {
    pub document_uuid: String,
    pub branch: String,
    pub repository_root: Option<String>,
    pub repository_backend: Option<String>,
    /// Repository branch head, if the repository has one.
    pub head: Option<String>,
    /// Manifest the open document corresponds to.
    pub current: Option<String>,
    pub versions: Vec<AppDocumentVersion>,
    /// The walk stopped before the root: a limit was hit, or the chain broke.
    pub truncated: bool,
    pub preview: Option<AppVersionPreview>,
    pub diff: Option<AppVersionDiff>,
    /// Soft failures (missing objects, unreadable sidecars). Per ADR 0003 these
    /// never block the listing.
    pub warnings: Vec<AppWarning>,
}
