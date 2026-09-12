//! The typed results a repository operation reports back.

use opendoc_core::HashRef;
use opendoc_format::{
    BranchHeadRecord, LookupRecord, SignatureRecord, TombstoneRecord, VersionLabelRecord,
};

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

/// Upper bound on manifests visited by [`Repository::list_versions`], matching
/// the limit the merge-base walks already use.
pub const VERSION_HISTORY_TRAVERSAL_LIMIT: usize = 4096;

/// One committed version, projected from its manifest plus best-effort sidecars.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VersionEntry {
    pub manifest: HashRef,
    pub parent: Option<HashRef>,
    pub snapshot: HashRef,
    pub operation_segments: Vec<HashRef>,
    pub created_at_ms: u64,
    pub signatures: Vec<SignatureRecord>,
    pub label: Option<VersionLabelRecord>,
    /// Whether the snapshot object backing this version is actually present, so
    /// a caller can offer "open" only where opening can succeed.
    pub snapshot_present: bool,
    pub is_head: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VersionHistoryProblem {
    pub manifest: Option<HashRef>,
    pub reason: String,
}

/// Result of walking a branch's manifest ancestry, newest first.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct VersionHistory {
    pub head: Option<HashRef>,
    pub entries: Vec<VersionEntry>,
    pub problems: Vec<VersionHistoryProblem>,
    /// Set when the walk stopped before reaching the root: either a limit was
    /// hit or the chain was unreadable past this point.
    pub truncated: bool,
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
pub(crate) struct CandidateHeadEntries {
    pub(crate) records: Vec<BranchHeadRecord>,
    pub(crate) invalid: Vec<CandidateHeadProblem>,
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
