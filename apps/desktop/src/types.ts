import type {
  OpenDocPermissionGrant,
  OpenDocPresencePeer,
  OpenDocRuntimeMode,
} from "./generated/runtime";

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
  AppCitationDatabase,
  AppCitationGroup,
  AppComment,
  AppCommentThread,
  AppDocument,
  AppFootnote,
  AppInline,
  AppSuggestion,
  EditorResult,
} from "./generated/document";
export type {
  AppEditorSelection,
  EditorInput,
  EditorInlineRange,
  EditorPosition,
  EditorSelection,
} from "./generated/editor";
export type {
  OpenDocAuthorizationDecision,
  OpenDocPermissionGrant,
  OpenDocPresencePeer,
  OpenDocRelayOperation,
  OpenDocRuntimeLookupEntry,
  OpenDocRuntimeLookupResult,
  OpenDocRuntimeMode,
  OpenDocRuntimeProfile,
  OpenDocRuntimeSession,
  OpenDocShareInvite,
  OpenDocStorageBackend,
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

export type OpenDocRuntimeConfig = {
  mode?: OpenDocRuntimeMode;
  label?: string;
  permissionsEnabled?: boolean;
  signingEnabled?: boolean;
  subject?: string | null;
  presence?: OpenDocPresencePeer[];
  permissions?: OpenDocPermissionGrant[];
  defaultRepositoryRoot?: string;
  defaultFlatNamespace?: string;
  storageBackends?: string[];
  tombstoneScanProblems?: string[];
};
