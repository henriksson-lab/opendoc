mod annotation_commands;
mod annotation_dto;
mod annotation_support;
mod audit;
mod audit_view;
#[cfg(test)]
mod batch_fold_tests;
mod blob;
mod blob_commands;
mod blob_io;
mod blob_service;
mod block_commands;
mod block_dto;
mod citation_dto;
mod clock;
mod collaboration;
mod command_result;
mod command_values;
mod dispatch;
mod document;
#[cfg(test)]
mod document_command_tests;
mod document_commands;
mod document_service;
mod document_tree;
#[cfg(test)]
mod dto_contract_tests;
#[cfg(test)]
mod dto_shape;
#[cfg(test)]
mod dto_ts;
mod editor;
mod editor_selection_service;
mod encoding;
mod error;
mod export;
mod files;
mod find;
mod image_block_service;
mod import_export;
mod import_export_service;
mod inline_commands;
mod inline_dto;
mod journal_service;
mod label;
mod layout_service;
mod lifecycle_service;
mod list_commands;
mod mark_dto;
mod mutation_service;
mod operation;
mod projection_service;
mod projection_support;
mod recent;
mod recovery_journal;
mod render;
mod render_service;
mod repository;
mod repository_io;
mod signing;
mod signing_service;
mod spreadsheet_commands;
mod spreadsheet_replay;
mod spreadsheet_service;
#[cfg(test)]
mod spreadsheet_tests;
mod spreadsheet_ui;
mod state;
mod table_commands;
mod table_dto;
mod version;
mod version_diff;
mod version_service;
mod warning;
pub(crate) use annotation_dto::parse_paragraph_style;
pub use annotation_dto::{AppComment, AppCommentThread, AppSuggestion};
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
pub use block_dto::{AppBlock, AppBlockProperties};
pub(crate) use citation_dto::app_citation_item_to_core;
pub use citation_dto::{
    AppBibliographyEntry, AppBibliographyReference, AppCitationDatabase, AppCitationGroup,
};
pub(crate) use clock::now_ms;
pub use collaboration::AppRemoteIntake;
pub use command_result::AppCommandResult;
pub(crate) use command_values::{
    parse_alignment, parse_block_property_key, parse_color, parse_direction,
    parse_header_footer_slot, parse_insert_position, parse_length, parse_line_spacing,
    parse_optional_page_number_field, INDENT_STEP_TWIPS,
};
pub use document::{
    AppBodyFragment, AppDocument, AppFootnote, AppPageLayout, AppPageSetup, AppPageSizePreset,
};
pub(crate) use document_service::DocumentOperationService;
use document_tree::*;
pub use editor::EditorResult;
pub(crate) use editor_selection_service::EditorSelectionService;
pub use encoding::{base64_decode, base64_encode};
pub use error::AppApiError;
pub use export::{AppExport, AppExportEncoding};
pub(crate) use image_block_service::{
    image_block_layout, image_display_length, parse_image_placement, ImageBlockService,
};
pub(crate) use import_export_service::{ImportExportReadService, ImportExportService};
pub use inline_dto::AppInline;
pub(crate) use label::{
    anchor_display_label, anchor_label, citation_source_format, citation_source_format_from_label,
    parse_anchor_label, parse_id, refresh_inline_citation_cache, suggestion_anchor_display_label,
    suggestion_text,
};
pub use layout_service::{AppBlockPlacement, AppDocumentLayout};
pub(crate) use list_commands::{list_kind, list_run_split_operations};
pub(crate) use mark_dto::{
    block_style_value, byte_offset_for_char_offset, inline_id, mark_label, mark_projection,
    parse_mark_kind, parse_marks, validate_format_replacement_payload, validate_mark_payload,
    validate_mark_removal_payload,
};
pub use opendoc_api::{
    parse_json_command, undo_coalesce_key, AppCitationItem, AppEditorSelection, AppFindMatch,
    AppFindMatches, EditorInlineRange, EditorInput, EditorMarkInput, EditorPosition,
    EditorSelection, FindOptions, InsertTableAfterArgs, OpenDocAuthorizationDecision,
    OpenDocAuthorizationSource, OpenDocCommand as AppCommand, OpenDocPermissionAuthority,
    OpenDocPresencePeer, OpenDocRelayOperation, OpenDocRuntimeLookupEntry,
    OpenDocRuntimeLookupResult, OpenDocRuntimeMode, OpenDocRuntimeProfile, OpenDocRuntimeSession,
    OpenDocServiceRole, OpenDocServiceSession, OpenDocShareInvite, OpenDocStorageBackend,
    OpenDocSyncBatchOutcome, OpenDocSyncRelayResult, DISCARD_UNSAVED_CHANGES_ARG,
};
pub use opendoc_spreadsheet::{FormulaError, FormulaValue};
pub(crate) use operation::{
    normalize_filter_criteria, normalize_filter_sort_specs, rich_document_operation_kind,
    validate_filter_option_payload, validate_operation_envelopes,
    validate_operation_segment_envelopes,
};
pub use operation::{
    AppBlobOperation, AppOperationEnvelope, AppOperationRecord, AppSpreadsheetOperation,
};
pub(crate) use projection_service::AppProjectionService;
pub(crate) use projection_support::{
    append_spreadsheet_formula_warnings, clear_citation_projection_payload, is_default,
};
#[cfg(not(target_arch = "wasm32"))]
pub use recent::FileRecentDocumentStore;
pub use recent::{
    AppRecentDocument, RecentDocumentStore, RecentDocuments, VolumeRecentDocumentStore,
    RECENT_DOCUMENT_LIMIT,
};
#[cfg(not(target_arch = "wasm32"))]
pub use recovery_journal::FileRecoveryJournalStore;
pub(crate) use recovery_journal::RecoveryJournal;
pub use recovery_journal::{AppRecoverySession, RecoveryJournalStore, VolumeRecoveryJournalStore};
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
pub(crate) use table_dto::{AppTable, AppTableCell, AppTableCellProperties, AppTableColumn};
pub use version::{
    AppDocumentVersion, AppVersionDiff, AppVersionDiffEntry, AppVersionPreview, AppVersionSigner,
    AppVersionView,
};
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
pub type AppSheetPrintOrientation = opendoc_spreadsheet::SheetPrintOrientation;
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
    validate_protected_range_description, validate_sheet_color, MAX_AXIS_SIZE_PX,
};

use opendoc_citations::{citation_source_bytes, render_bibliography};
use opendoc_core::{
    digest_bytes, new_list_id, Anchor, BibliographyReference, Block, BlockKind, BlockProperties,
    BlockProperty, BlockPropertyKey, CitationGroup, CitationItem, CitationPlacement,
    CitationSource, CitationSourceFormat, CitationSummary, Comment, CommentThread, Equation,
    EquationSourceFormat, Footnote, HeaderFooterSlot, Inline, InsertPosition, ListKind, Mark,
    MarkExpand, MarkKind, ModelWarning, ParagraphStyle, StableId, Suggestion, SuggestionKind,
    SuggestionState, TextRange,
};
// The canonical source model. Public because a collaboration client hands the
// service's merge base straight to `join_collaboration_session`.
pub use opendoc_core::Document;
use opendoc_format::{
    decode_cbor, decode_record, encode_canonical_cbor, encode_record, LookupAliasRecord,
    LookupRecord, ManifestRecord, OperationSegmentRecord, SnapshotRecord,
};
use opendoc_import::{
    export_docx_with_warnings, export_google_docs_json_with_warnings, import_doc_or_docx,
    import_google_docs_json, DocxImage,
};
// The typed operation vocabulary a collaboration client exchanges with a
// service, re-exported so a runtime needs only `opendoc-app` on its path.
use opendoc_merge::{
    invert_text_operations, merge_operations, text_run_of, BlockTextStyle, Inversion,
};
pub use opendoc_merge::{
    ActorId, CausalContext, Operation, OperationId, OperationKind, VectorClock,
};
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
/// The snapshot payload format a local save writes.
///
/// **`v1` because the payload changed shape.** `v0` carried the document's
/// text as a `visible_text` field; `AppDocument` no longer has one (it is
/// derived — see `AppDocument::visible_text`), and it carried the rendered
/// body as one `body_html` string where it now carries the ordered fragments
/// the body is composed of. Both are projections, but the snapshot is also
/// what a signature is computed over: the canonical CBOR of a `v0` record
/// and of a `v1` record with the same document are different bytes, so a `v0`
/// record read as if it were this shape would report every signature it holds
/// as broken and give no reason. `is_readable_snapshot_format` compares
/// against this constant, so bumping it is what refuses one by name instead —
/// the same move `opendoc.recovery-segment.v0` -> `v1` made, and the reason
/// `legacy-operation-sequence-gap` exists for the change that could be read
/// as written.
///
/// **`v2` because the payload's *encoding* changed.** `AppBlock` and
/// `AppBlockProperties` now skip their unset fields instead of writing them
/// as `null`, so the canonical CBOR of the same document is different bytes
/// from what `v1` produced — for every document, not only a formatted one,
/// because a plain paragraph has an unset `level`. Unlike the `v0` -> `v1`
/// bump the *shape* is unchanged and a `v1` payload still decodes exactly, so
/// `v1` is read rather than refused ([`APP_DOCUMENT_FORMATS_READ_ONLY`]). What
/// it cannot do is verify: a signature made over a `v1` payload cannot match a
/// target recomputed over a `v2` one, and reporting that as
/// `broken-document-signature` would tell a user their document had been
/// altered when only this crate's encoder moved. Declaring the format is what
/// lets the reader say `signature-predates-payload-format` instead.
pub(crate) const APP_DOCUMENT_FORMAT: &str = "opendoc.app-document.v2";
/// Earlier app snapshot payload formats that are still *read*.
///
/// A format belongs here only when its payload still decodes into today's
/// `AppDocument` unambiguously — which `v1` does, because `v2` changed which
/// fields the encoder omits and not which fields exist. `v0` is deliberately
/// absent: its payload had fields this one does not, so it is refused by name.
///
/// Reading is not verifying. A document opened from one of these keeps its
/// signatures and reports them as `signature-predates-payload-format`; the
/// next save rewrites the snapshot at [`APP_DOCUMENT_FORMAT`].
pub(crate) const APP_DOCUMENT_FORMATS_READ_ONLY: &[&str] = &["opendoc.app-document.v1"];

/// True when `source_format` names an app payload this build reads but no
/// longer writes, so a signature over it cannot be compared with a target
/// recomputed here.
pub(crate) fn is_superseded_app_document_format(source_format: &str) -> bool {
    APP_DOCUMENT_FORMATS_READ_ONLY.contains(&source_format)
}
/// The snapshot payload format `opendoc-service` writes.
///
/// `opendoc-service` owns this string (`opendoc_service::SERVICE_DOCUMENT_FORMAT`)
/// and is deliberately not a dependency of this crate — the service must not
/// pull the app facade, and the app must not pull a network daemon. So the
/// constant is restated here and the service's test suite asserts the two are
/// the same, which is the check that keeps a copied string honest.
pub const SERVICE_DOCUMENT_FORMAT: &str = "opendoc.service-document.v0";

pub use state::OpenDocApp;
pub(crate) use state::{UNDO_COALESCE_WINDOW_MS, UNDO_STACK_LIMIT};

/// Every command name accepted by [`OpenDocApp::dispatch_command`].
pub fn command_names() -> Vec<String> {
    opendoc_api::command_names()
}

// Monolith-level app API tests were removed during the restructure.
// Rebuild coverage beside the extracted domain modules and generated API boundary.
