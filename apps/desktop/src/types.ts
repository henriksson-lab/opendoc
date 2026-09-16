import type { OpenDocRuntimeMode } from "./generated/runtime";

export type {
  AppArchiveTombstone,
  AppAuditBlobSignature,
  AppAuditView,
  AppCandidateHeadProblem,
  AppDeletedBasicFilter,
  AppDeletedCellValidation,
  AppDeletedColumn,
  AppDeletedMerge,
  AppDeletedNamedRange,
  AppDeletedProtectedRange,
  AppDeletedRow,
  AppDeletedSheet,
  AppOperationRecord,
  AppRecentDocument,
  AppRecoverySession,
  AppRepositoryTombstone,
  AppSignature,
  AppWarning,
} from "./generated/audit";
export type { AppBlobRef, AppTypedContentSignature } from "./generated/blob";
export type { AppCitationItem } from "./generated/citation";
export type { AppCommandResult } from "./generated/commands";
export type {
  AppBibliographyEntry,
  AppBibliographyReference,
  AppBlock,
  AppBlockProperties,
  AppBodyFragment,
  AppCitationDatabase,
  AppCitationGroup,
  AppComment,
  AppCommentThread,
  AppDocument,
  AppFootnote,
  AppInline,
  AppPageLayout,
  AppPageSetup,
  AppPageSizePreset,
  AppSuggestion,
  EditorResult,
} from "./generated/document";
export type {
  AppEditorSelection,
  AppFindMatch,
  AppFindMatches,
  EditorInput,
  EditorInlineRange,
  EditorPosition,
  EditorSelection,
} from "./generated/editor";
export type { AppExport, AppExportEncoding } from "./generated/export";
export type { AppBlockPlacement, AppDocumentLayout } from "./generated/layout";
export type {
  OpenDocAuthorizationDecision,
  OpenDocAuthorizationSource,
  OpenDocPermissionAuthority,
  OpenDocPresencePeer,
  OpenDocRelayOperation,
  OpenDocRuntimeLookupEntry,
  OpenDocRuntimeLookupResult,
  OpenDocRuntimeMode,
  OpenDocRuntimeProfile,
  OpenDocRuntimeSession,
  OpenDocServiceRole,
  OpenDocServiceSession,
  OpenDocShareInvite,
  OpenDocStorageBackend,
  OpenDocSyncBatchOutcome,
  OpenDocSyncRelayResult,
} from "./generated/runtime";
export type {
  AppCell,
  AppCellComment,
  AppCellDependency,
  AppCellFormat,
  AppCellValidation,
  AppDeletedCellComment,
  AppDeletedColumnPayload,
  AppDeletedRowPayload,
  AppNamedRange,
  AppSheet,
  AppSheetAxis,
  AppSheetFilter,
  AppSheetFilterCriterion,
  AppSheetFilterSortSpec,
  AppSheetMerge,
  AppSheetProtectedRange,
  AppSpreadsheetEvaluationContext,
  AppSpreadsheetSelection,
  AppSpreadsheetWorkbook,
} from "./generated/spreadsheet";
export type {
  AppDocumentVersion,
  AppVersionDiff,
  AppVersionDiffEntry,
  AppVersionPreview,
  AppVersionSigner,
  AppVersionView,
} from "./generated/version";

/// What the host tells the frontend about itself before anything is dispatched.
///
/// Capabilities only. There is deliberately no `permissions` and no `presence`
/// here: permissions in a service runtime are the service's answers, which
/// reach the app over its own transport, and presence is server state. A page
/// that could declare either would be declaring its own access.
export type OpenDocRuntimeConfig = {
  mode?: OpenDocRuntimeMode;
  label?: string;
  signingEnabled?: boolean;
  subject?: string | null;
  defaultRepositoryRoot?: string;
  defaultFlatNamespace?: string;
  storageBackends?: string[];
  tombstoneScanProblems?: string[];
};
