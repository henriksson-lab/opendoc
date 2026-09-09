use serde::{Deserialize, Serialize};

use crate::{
    AppApiError, AppBibliographyReference, AppCellValidation, AppCitationGroup,
    AppDeletedCellComment, AppDeletedColumnPayload, AppDeletedRowPayload, AppNamedRange,
    AppOperationRecord, AppSheetFilter, AppSheetMerge, AppSheetProtectedRange,
    AppTypedContentSignature, AppWarning,
};
use opendoc_store::CandidateHeadProblem;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppAuditView {
    pub uuid: String,
    pub title: String,
    pub repository_root: Option<String>,
    pub repository_backend: Option<String>,
    pub repository_namespace: Option<String>,
    pub last_manifest: Option<String>,
    pub warnings: Vec<AppWarning>,
    pub signatures: Vec<AppSignature>,
    pub blob_signatures: Vec<AppAuditBlobSignature>,
    pub repository_tombstones: Vec<AppRepositoryTombstone>,
    pub repository_tombstone_problems: Vec<String>,
    pub deleted_comments: Vec<crate::AppCommentThread>,
    pub deleted_sheets: Vec<AppDeletedSheet>,
    pub deleted_named_ranges: Vec<AppDeletedNamedRange>,
    pub deleted_protected_ranges: Vec<AppDeletedProtectedRange>,
    pub deleted_basic_filters: Vec<AppDeletedBasicFilter>,
    pub deleted_merges: Vec<AppDeletedMerge>,
    pub deleted_cell_validations: Vec<AppDeletedCellValidation>,
    pub deleted_rows: Vec<AppDeletedRow>,
    pub deleted_columns: Vec<AppDeletedColumn>,
    pub deleted_cell_comments: Vec<AppDeletedCellComment>,
    pub resolved_suggestions: Vec<crate::AppSuggestion>,
    pub deleted_references: Vec<AppBibliographyReference>,
    pub deleted_citations: Vec<AppCitationGroup>,
    pub candidate_head_problems: Vec<AppCandidateHeadProblem>,
    pub operations: Vec<AppOperationRecord>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppRepositoryTombstone {
    pub blob_hash: String,
    pub attached_to_blob_ref: bool,
    pub archive_tombstone: AppArchiveTombstone,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppCandidateHeadProblem {
    pub path: String,
    pub reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppDeletedSheet {
    pub sheet_id: String,
    pub operation: AppOperationRecord,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppDeletedNamedRange {
    pub name: String,
    pub range: AppNamedRange,
    pub operation: AppOperationRecord,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppDeletedProtectedRange {
    pub sheet_id: String,
    pub protected_range: AppSheetProtectedRange,
    pub operation: AppOperationRecord,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppDeletedBasicFilter {
    pub sheet_id: String,
    pub filter: AppSheetFilter,
    pub operation: AppOperationRecord,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppDeletedMerge {
    pub sheet_id: String,
    pub merge: AppSheetMerge,
    pub operation: AppOperationRecord,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppDeletedCellValidation {
    pub sheet_id: String,
    pub address: String,
    pub validation: AppCellValidation,
    pub operation: AppOperationRecord,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppDeletedRow {
    pub sheet_id: String,
    pub row: String,
    pub payload: AppDeletedRowPayload,
    pub operation: AppOperationRecord,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppDeletedColumn {
    pub sheet_id: String,
    pub column: String,
    pub payload: AppDeletedColumnPayload,
    pub operation: AppOperationRecord,
}

impl AppCandidateHeadProblem {
    pub(crate) fn from_store(problem: CandidateHeadProblem) -> Self {
        Self {
            path: problem.path,
            reason: problem.reason,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppAuditBlobSignature {
    pub blob_hash: String,
    pub blob_name: String,
    pub deleted: bool,
    pub signature_state: String,
    pub archive_tombstone: Option<AppArchiveTombstone>,
    pub signatures: Vec<AppSignature>,
    pub typed_signatures: Vec<AppTypedContentSignature>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppArchiveTombstone {
    pub archive_locator: String,
    pub restore_hint: String,
    pub created_at_ms: u64,
    pub signer: String,
}

impl AppArchiveTombstone {
    pub(crate) fn from_record(record: &opendoc_format::TombstoneRecord) -> Self {
        Self {
            archive_locator: record.archive_locator.clone(),
            restore_hint: record.restore_hint.clone(),
            created_at_ms: record.created_at_ms,
            signer: record.signer.clone(),
        }
    }

    pub(crate) fn validate_source(&self) -> Result<(), AppApiError> {
        if self.archive_locator.trim().is_empty() {
            return Err(AppApiError::Format(
                "archive tombstone locator is empty".to_string(),
            ));
        }
        if self.archive_locator.trim() != self.archive_locator {
            return Err(AppApiError::Format(
                "archive tombstone locator has surrounding whitespace".to_string(),
            ));
        }
        if self.restore_hint.trim().is_empty() {
            return Err(AppApiError::Format(
                "archive tombstone restore hint is empty".to_string(),
            ));
        }
        if self.restore_hint.trim() != self.restore_hint {
            return Err(AppApiError::Format(
                "archive tombstone restore hint has surrounding whitespace".to_string(),
            ));
        }
        if self.signer.trim().is_empty() {
            return Err(AppApiError::Format(
                "archive tombstone signer is empty".to_string(),
            ));
        }
        if self.signer.trim() != self.signer {
            return Err(AppApiError::Format(
                "archive tombstone signer has surrounding whitespace".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppSignature {
    pub target: String,
    pub signer: String,
    pub signer_display: String,
    pub title: String,
    pub signed_at_ms: u64,
}

impl AppSignature {
    pub(crate) fn from_record(record: &opendoc_format::SignatureRecord) -> Self {
        Self {
            target: record.target.to_string(),
            signer: record.signer.clone(),
            signer_display: record.signer_display.clone(),
            title: record.title.clone(),
            signed_at_ms: record.signed_at_ms,
        }
    }

    pub(crate) fn validate_source(&self) -> Result<(), AppApiError> {
        if self.target.trim().is_empty() {
            return Err(AppApiError::Format("signature target is empty".to_string()));
        }
        if self.target.trim() != self.target {
            return Err(AppApiError::Format(
                "signature target has surrounding whitespace".to_string(),
            ));
        }
        opendoc_core::HashRef::parse(&self.target)
            .map_err(|err| AppApiError::Format(err.to_string()))?;
        if self.signer.trim().is_empty() {
            return Err(AppApiError::Format("signature signer is empty".to_string()));
        }
        if self.signer.trim() != self.signer {
            return Err(AppApiError::Format(
                "signature signer has surrounding whitespace".to_string(),
            ));
        }
        if self.signer_display.trim().is_empty() {
            return Err(AppApiError::Format(
                "signature signer display is empty".to_string(),
            ));
        }
        if self.signer_display.trim() != self.signer_display {
            return Err(AppApiError::Format(
                "signature signer display has surrounding whitespace".to_string(),
            ));
        }
        if self.title.trim().is_empty() {
            return Err(AppApiError::Format("signature title is empty".to_string()));
        }
        if self.title.trim() != self.title {
            return Err(AppApiError::Format(
                "signature title has surrounding whitespace".to_string(),
            ));
        }
        Ok(())
    }
}
