mod annotation_commands;
mod annotation_support;
mod audit;
mod audit_view;
mod blob;
mod blob_commands;
mod blob_io;
mod blob_service;
mod clock;
mod command_result;
mod dispatch;
mod document;
mod document_commands;
mod document_service;
mod document_tree;
mod editor;
mod editor_selection_service;
mod encoding;
mod error;
mod files;
mod image_block_service;
mod import_export;
mod import_export_service;
mod journal_service;
mod lifecycle_service;
mod mutation_service;
mod operation;
mod projection_service;
mod projection_support;
mod recent;
mod render;
mod render_service;
mod repository;
mod repository_io;
mod signing;
mod signing_service;
mod spreadsheet_commands;
mod spreadsheet_replay;
mod spreadsheet_service;
mod spreadsheet_ui;
mod state;
mod warning;
pub(crate) use annotation_support::{
    normalize_bibliography_authors, normalize_citation_locale, normalize_citation_style,
    normalize_optional_source_string, normalize_source_author, normalize_source_text,
    render_citation_cache,
};
pub use audit::{
    AppArchiveTombstone, AppAuditBlobSignature, AppAuditView, AppCandidateHeadProblem,
    AppDeletedBasicFilter, AppDeletedCellValidation, AppDeletedColumn, AppDeletedMerge,
    AppDeletedNamedRange, AppDeletedProtectedRange, AppDeletedRow, AppDeletedSheet,
    AppRepositoryTombstone, AppSignature,
};
use blob::validate_signature_state;
pub use blob::{AppBlobRef, AppTypedContentSignature};
pub(crate) use blob_io::{
    clean_blob_media_type, clean_blob_name, export_google_docs_json_with_opendoc_blobs,
    import_opendoc_blob_refs_from_google_docs_json, merge_blob_envelopes,
    merge_operations_from_envelopes, missing_blob_ref, restore_referenced_image_blobs,
};
pub(crate) use blob_service::{BlobLifecycleJournal, BlobLifecycleService};
pub(crate) use clock::now_ms;
pub use command_result::AppCommandResult;
pub(crate) use document::{
    app_citation_item_to_core, byte_offset_for_char_offset, inline_id, parse_id, parse_mark_kind,
    refresh_inline_citation_cache, validate_mark_payload, validate_mark_removal_payload,
};
pub use document::{
    AppBibliographyEntry, AppBibliographyReference, AppBlock, AppCitationDatabase,
    AppCitationGroup, AppComment, AppCommentThread, AppDocument, AppFootnote, AppInline,
    AppSuggestion,
};
pub(crate) use document_service::DocumentOperationService;
use document_tree::*;
pub use editor::EditorResult;
pub(crate) use editor_selection_service::EditorSelectionService;
pub use encoding::{base64_decode, base64_encode};
pub use error::AppApiError;
pub(crate) use image_block_service::ImageBlockService;
pub(crate) use import_export_service::{ImportExportReadService, ImportExportService};
pub use opendoc_api::{
    parse_json_command, undo_coalesce_key, AppCitationItem, AppEditorSelection, EditorInlineRange,
    EditorInput, EditorMarkInput, EditorPosition, EditorSelection, InsertTableAfterArgs,
    OpenDocAuthorizationDecision, OpenDocCommand as AppCommand, OpenDocPermissionGrant,
    OpenDocPresencePeer, OpenDocRelayOperation, OpenDocRuntimeLookupEntry,
    OpenDocRuntimeLookupResult, OpenDocRuntimeMode, OpenDocRuntimeProfile, OpenDocRuntimeSession,
    OpenDocShareInvite, OpenDocStorageBackend, OpenDocSyncRelayResult,
};
pub use opendoc_spreadsheet::{FormulaError, FormulaValue};
pub use operation::AppOperationRecord;
pub(crate) use operation::{
    normalize_filter_criteria, normalize_filter_sort_specs, validate_filter_option_payload,
    validate_operation_envelopes, validate_operation_segment_envelopes, AppBlobOperation,
    AppOperationEnvelope, AppSpreadsheetOperation,
};
pub(crate) use projection_service::AppProjectionService;
pub(crate) use projection_support::{
    append_spreadsheet_formula_warnings, clear_citation_projection_payload,
};
pub use recent::AppRecentDocument;
pub use render::escape_html;
pub(crate) use render_service::AppRenderService;
use repository::AppRepositoryTarget;
pub(crate) use signing::{
    sign_fastq_profile, sign_image_pixels_profile, verify_app_typed_signature,
};
pub(crate) use signing_service::SignatureService;
pub(crate) use spreadsheet_replay::{
    envelope_id_key, envelope_payload_matches, merge_spreadsheet_envelope_streams,
};
pub(crate) use spreadsheet_service::SpreadsheetEvaluationPolicy;
pub use spreadsheet_ui::AppSpreadsheetSelection;
pub use warning::AppWarning;
use warning::{push_spreadsheet_warning, push_unique_warning};

pub type AppCell = opendoc_spreadsheet::Cell;
pub type AppCellComment = opendoc_spreadsheet::CellComment;
pub type AppCellDependency = opendoc_spreadsheet::CellDependency;
pub type AppCellFormat = opendoc_spreadsheet::CellFormat;
pub type AppCellValidation = opendoc_spreadsheet::CellValidation;
pub type AppDeletedCellComment = opendoc_spreadsheet::DeletedCellComment;
pub type AppDeletedColumnPayload = opendoc_spreadsheet::DeletedColumnPayload;
pub type AppDeletedRowPayload = opendoc_spreadsheet::DeletedRowPayload;
pub type AppNamedRange = opendoc_spreadsheet::NamedRange;
pub type AppSheet = opendoc_spreadsheet::Sheet;
pub type AppSheetAxis = opendoc_spreadsheet::SheetAxis;
pub type AppSheetFilter = opendoc_spreadsheet::SheetFilter;
pub type AppSheetFilterCriterion = opendoc_spreadsheet::SheetFilterCriterion;
pub type AppSheetFilterSortSpec = opendoc_spreadsheet::SheetFilterSortSpec;
pub type AppSheetMerge = opendoc_spreadsheet::SheetMerge;
pub type AppSheetProtectedRange = opendoc_spreadsheet::SheetProtectedRange;
pub type AppSpreadsheetEvaluationContext = opendoc_spreadsheet::SpreadsheetEvaluationContext;
pub type AppSpreadsheetWorkbook = opendoc_spreadsheet::SpreadsheetWorkbook;

pub(crate) use opendoc_spreadsheet::{
    cell_axis_labels, export_google_sheets_workbook, import_google_sheets_workbook,
    normalize_cell_address, normalize_cell_range, normalize_column_label, normalize_merge_range,
    normalize_named_range_name, normalize_protected_range_description, normalize_row_label,
    normalize_sheet_id, normalize_sheet_title, parse_format_bool, validate_canonical_cell_address,
    validate_canonical_cell_range, validate_canonical_column_label, validate_canonical_merge_range,
    validate_canonical_row_label, validate_canonical_sheet_id, validate_filter_condition,
    validate_protected_range_description, validate_sheet_color,
};

use opendoc_citations::{citation_source_bytes, render_bibliography};
use opendoc_core::{
    digest_bytes, Anchor, BibliographyReference, Block, BlockKind, CitationGroup, CitationItem,
    CitationPlacement, CitationSource, CitationSourceFormat, CitationSummary, Comment,
    CommentThread, Document, Equation, EquationSourceFormat, Footnote, Inline, Mark, MarkExpand,
    MarkKind, ModelWarning, StableId, Suggestion, SuggestionKind, SuggestionState, TextRange,
};
use opendoc_format::{
    decode_cbor, decode_record, encode_canonical_cbor, encode_record, LookupAliasRecord,
    LookupRecord, ManifestRecord, OperationSegmentRecord, SnapshotRecord,
};
use opendoc_import::{export_google_docs_json, import_doc_or_docx, import_google_docs_json};
use opendoc_merge::{merge_operations, BlockTextStyle, Operation, OperationKind};
use opendoc_sign::{
    verify_record_for_target_with_public_key, DecodedImagePixels, FastqFullProfile,
    FastqSequenceProfile, OpenSshSigner, SignatureState, Signer,
};
#[cfg(feature = "opendal-store")]
use opendoc_store::OpenDalObjectStore;
use opendoc_store::{
    CandidateMergePlan, FlatObjectStore, LocalObjectStore, ObjectStore, Repository, StoreError,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

pub(crate) const SNAPSHOT_BRANCH: &str = "main";
pub(crate) const APP_DOCUMENT_FORMAT: &str = "opendoc.app-document.v0";

pub use state::OpenDocApp;
pub(crate) use state::{UNDO_COALESCE_WINDOW_MS, UNDO_STACK_LIMIT};

/// Every command name accepted by [`OpenDocApp::dispatch_command`].
pub fn command_names() -> Vec<String> {
    opendoc_api::command_names()
}

// Monolith-level app API tests were removed during the restructure.
// Rebuild coverage beside the extracted domain modules and generated API boundary.
