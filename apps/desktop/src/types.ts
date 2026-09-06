export type AppDocument = {
  is_open: boolean;
  uuid: string;
  title: string;
  locale: string;
  doi: string | null;
  visible_text: string;
  blocks: AppBlock[];
  footnotes: AppFootnote[];
  comments: AppCommentThread[];
  suggestions: AppSuggestion[];
  citations: AppCitationDatabase;
  workbook: AppSpreadsheetWorkbook;
  blobs: AppBlobRef[];
  warnings: AppWarning[];
  signature_state: string;
  signature: AppSignature | null;
  signatures: AppSignature[];
  repository_root: string | null;
  repository_backend: string | null;
  repository_namespace: string | null;
  recent_documents: AppRecentDocument[];
  last_manifest: string | null;
  has_unsaved_changes: boolean;
  operation_count: number;
  operations: AppOperationRecord[];
  body_html: string;
  footnotes_html: string;
};

export type OpenDocRuntimeMode =
  | "tauri-local"
  | "browser-local"
  | "hpc-single-user"
  | "multi-user-service";

export type OpenDocStorageBackend = "local" | "flat" | "opendal-fs";

export type OpenDocRuntimeProfile = {
  mode: OpenDocRuntimeMode;
  label: string;
  permissions_enabled: boolean;
  signing_enabled: boolean;
  browser_signing_deferred: boolean;
  default_repository_root: string;
  default_flat_namespace: string;
  storage_backends: OpenDocStorageBackend[];
};

export type OpenDocRuntimeSession = {
  profile: OpenDocRuntimeProfile;
  authenticated_subject: string | null;
  document_uuid: string | null;
  permissions: OpenDocPermissionGrant[];
  presence: OpenDocPresencePeer[];
  warnings: string[];
};

export type OpenDocPermissionGrant = {
  subject: string;
  action: string;
  scope: string;
  document_uuid: string | null;
};

export type OpenDocPresencePeer = {
  subject: string;
  display_name: string;
  role: string;
  cursor_anchor: string | null;
  last_seen_ms: number;
};

export type OpenDocAuthorizationDecision = {
  mode: OpenDocRuntimeMode;
  command: string;
  required_action: string;
  subject: string | null;
  document_uuid: string | null;
  allowed: boolean;
  reason: string;
  warnings: string[];
};

export type OpenDocShareInvite = {
  authorization: OpenDocAuthorizationDecision;
  issuer: string | null;
  target_subject: string | null;
  document_uuid: string | null;
  grants: OpenDocPermissionGrant[];
  created_at_ms: number;
  warnings: string[];
};

export type OpenDocRelayOperation = {
  id: string;
  actor: string;
  seq: number;
  kind: string;
  base_manifest: string | null;
};

export type OpenDocSyncRelayResult = {
  authorization: OpenDocAuthorizationDecision;
  document_uuid: string | null;
  base_manifest: string | null;
  accepted_operations: string[];
  deferred_operations: string[];
  rejected_operations: string[];
  presence: OpenDocPresencePeer[];
  warnings: string[];
};

export type OpenDocRuntimeLookupEntry = {
  document_uuid: string;
  doi: string | null;
  manifest: string | null;
};

export type OpenDocRuntimeLookupResult = {
  authorization: OpenDocAuthorizationDecision;
  requested_document_uuid: string | null;
  requested_doi: string | null;
  resolved_document_uuid: string | null;
  manifest: string | null;
  lookup_source: string;
  used_scan: boolean;
  warnings: string[];
};

export type AppAuditView = {
  uuid: string;
  title: string;
  repository_root: string | null;
  repository_backend: string | null;
  repository_namespace: string | null;
  last_manifest: string | null;
  warnings: AppWarning[];
  signatures: AppSignature[];
  blob_signatures: AppAuditBlobSignature[];
  repository_tombstones: AppRepositoryTombstone[];
  repository_tombstone_problems: string[];
  deleted_comments: AppCommentThread[];
  deleted_sheets: AppDeletedSheet[];
  deleted_named_ranges: AppDeletedNamedRange[];
  deleted_protected_ranges: AppDeletedProtectedRange[];
  deleted_basic_filters: AppDeletedBasicFilter[];
  deleted_merges: AppDeletedMerge[];
  deleted_cell_validations: AppDeletedCellValidation[];
  deleted_rows: AppDeletedRow[];
  deleted_columns: AppDeletedColumn[];
  deleted_cell_comments: AppDeletedCellComment[];
  resolved_suggestions: AppSuggestion[];
  deleted_references: AppBibliographyReference[];
  deleted_citations: AppCitationGroup[];
  candidate_head_problems: AppCandidateHeadProblem[];
  operations: AppOperationRecord[];
};

export type AppRepositoryTombstone = {
  blob_hash: string;
  attached_to_blob_ref: boolean;
  archive_tombstone: AppArchiveTombstone;
};

export type AppCandidateHeadProblem = {
  path: string;
  reason: string;
};

export type AppDeletedSheet = {
  sheet_id: string;
  operation: AppOperationRecord;
};

export type AppDeletedNamedRange = {
  name: string;
  range: AppNamedRange;
  operation: AppOperationRecord;
};

export type AppDeletedProtectedRange = {
  sheet_id: string;
  protected_range: AppSheetProtectedRange;
  operation: AppOperationRecord;
};

export type AppDeletedBasicFilter = {
  sheet_id: string;
  filter: AppSheetFilter;
  operation: AppOperationRecord;
};

export type AppDeletedMerge = {
  sheet_id: string;
  merge: AppSheetMerge;
  operation: AppOperationRecord;
};

export type AppDeletedCellValidation = {
  sheet_id: string;
  address: string;
  validation: AppCellValidation;
  operation: AppOperationRecord;
};

export type AppDeletedRow = {
  sheet_id: string;
  row: string;
  payload: AppDeletedRowPayload;
  operation: AppOperationRecord;
};

export type AppDeletedRowPayload = {
  row_axis: AppSheetAxis;
  cells: AppCell[];
  merges: AppSheetMerge[];
  filters: AppSheetFilter[];
  protected_ranges: AppSheetProtectedRange[];
  named_ranges: AppNamedRange[];
};

export type AppDeletedColumn = {
  sheet_id: string;
  column: string;
  payload: AppDeletedColumnPayload;
  operation: AppOperationRecord;
};

export type AppDeletedColumnPayload = {
  column_axis: AppSheetAxis;
  cells: AppCell[];
  merges: AppSheetMerge[];
  filters: AppSheetFilter[];
  protected_ranges: AppSheetProtectedRange[];
  named_ranges: AppNamedRange[];
};

export type AppAuditBlobSignature = {
  blob_hash: string;
  blob_name: string;
  deleted: boolean;
  signature_state: string;
  archive_tombstone: AppArchiveTombstone | null;
  signatures: AppSignature[];
  typed_signatures: AppTypedContentSignature[];
};

export type AppArchiveTombstone = {
  archive_locator: string;
  restore_hint: string;
  created_at_ms: number;
  signer: string;
};

export type AppRecentDocument = {
  uuid: string;
  title: string;
  doi: string | null;
  repository_root: string;
  repository_backend: string;
  repository_namespace: string | null;
  last_manifest: string | null;
  updated_at_ms: number;
};

export type AppBlobRef = {
  id: string;
  name: string;
  media_type: string;
  hash: string;
  size: number;
  available: boolean;
  signature_state: string;
  signatures: AppSignature[];
  typed_signatures: AppTypedContentSignature[];
  archive_tombstone: AppArchiveTombstone | null;
};

export type AppTypedContentSignature = {
  source_blob: string;
  profile: string;
  semantic_digest: string;
  included_fields: string[];
  excluded_fields: string[];
  signature_state: string;
  signature: AppSignature;
  signature_bytes: number[];
  profile_payload: number[] | null;
};

export type AppBlock = {
  id: string;
  kind: string;
  level: number | null;
  ordered: boolean | null;
  equation_source: string | null;
  blob_hash?: string | null;
  alt_text?: string | null;
  content: AppInline[];
  rows: AppBlock[][][];
  row_ids?: string[];
  cell_ids?: string[][];
};

export type AppInline = {
  id: string;
  kind: string;
  text: string;
  href: string | null;
  target_id: string | null;
  marks: string[];
};

export type AppFootnote = {
  id: string;
  revision: number;
  body: AppInline[];
  deleted: boolean;
};

export type AppCitationDatabase = {
  style: string;
  locale: string;
  references: AppBibliographyReference[];
  bibliography: AppBibliographyEntry[];
  citations: AppCitationGroup[];
};

export type AppBibliographyEntry = {
  reference_id: string;
  text: string;
};

export type AppBibliographyReference = {
  id: string;
  revision: number;
  format: string;
  source: string;
  title: string;
  authors: string[];
  issued: string | null;
  doi: string | null;
  url: string | null;
  deleted: boolean;
};

export type AppCitationGroup = {
  id: string;
  revision: number;
  items: AppCitationItem[];
  placement: string;
  footnote_id: string | null;
  rendered_cache: string | null;
  deleted: boolean;
};

export type AppCitationItem = {
  reference_id: string;
  locator: string | null;
  label: string | null;
  prefix: string | null;
  suffix: string | null;
  suppress_author: boolean;
};

export type AppCommentThread = {
  id: string;
  anchor: string;
  comments: AppComment[];
  deleted: boolean;
};

export type AppComment = {
  id: string;
  author: string;
  body: string;
  deleted: boolean;
};

export type AppSuggestion = {
  id: string;
  author: string;
  kind: string;
  text: string;
  state: string;
  anchor: string | null;
  range_start: string | null;
  range_end: string | null;
  marks: string[];
  content: AppInline[];
  provenance: string[];
};

export type AppWarning = {
  code: string;
  message: string;
};

export type AppOperationRecord = {
  actor: string;
  seq: number;
  kind: string;
  summary: string;
  created_at_ms: number;
};

export type AppSignature = {
  target: string;
  signer: string;
  signer_display: string;
  title: string;
  signed_at_ms: number;
};

export type AppSpreadsheetWorkbook = {
  title: string;
  locale: string;
  timezone: string;
  named_ranges: AppNamedRange[];
  dependency_graph: AppCellDependency[];
  sheets: AppSheet[];
};

export type AppNamedRange = {
  id: string;
  name: string;
  sheet_id: string;
  range: string;
};

export type AppCellDependency = {
  sheet_id: string;
  address: string;
  dependencies: string[];
  dependents: string[];
  invalidation_order: string[];
};

export type AppSheet = {
  id: string;
  title: string;
  frozen_rows: number;
  frozen_columns: number;
  merges: AppSheetMerge[];
  filters: AppSheetFilter[];
  protected_ranges: AppSheetProtectedRange[];
  row_axes: AppSheetAxis[];
  column_axes: AppSheetAxis[];
  rows: string[];
  columns: string[];
  cells: AppCell[];
};

export type AppSheetAxis = {
  id: string;
  label: string;
};

export type AppSheetMerge = {
  id: string;
  range: string;
};

export type AppSheetFilter = {
  id: string;
  range: string;
  criteria: AppSheetFilterCriterion[];
  sort_specs: AppSheetFilterSortSpec[];
};

export type AppSheetFilterCriterion = {
  column: string;
  condition: string;
  value: string;
};

export type AppSheetFilterSortSpec = {
  column: string;
  descending: boolean;
};

export type AppSheetProtectedRange = {
  id: string;
  range: string;
  description: string;
  warning_only: boolean;
};

export type AppCell = {
  address: string;
  user_kind: string;
  user_value: string;
  format: AppCellFormat;
  validation: AppCellValidation | null;
  computed_kind: string;
  computed_value: string;
  dependencies: string[];
  comments: AppCellComment[];
};

export type AppCellValidation = {
  kind: string;
  values: string[];
  strict: boolean;
  show_dropdown: boolean;
};

export type AppCellComment = {
  id: string;
  author: string;
  body: string;
  deleted: boolean;
};

export type AppDeletedCellComment = {
  sheet_id: string;
  address: string;
  comment: AppCellComment;
};

export type AppCellFormat = {
  bold: boolean;
  italic: boolean;
  text_color: string | null;
  background_color: string | null;
  horizontal_align: string | null;
  number_format: string | null;
};

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

/** Tagged result envelope produced by the Rust dispatcher. */
export type AppCommandResult =
  | { kind: "Document"; value: AppDocument }
  | { kind: "Editor"; value: EditorResult }
  | { kind: "Text"; value: string }
  | { kind: "AuditView"; value: AppAuditView }
  | { kind: "RuntimeProfile"; value: OpenDocRuntimeProfile }
  | { kind: "RuntimeSession"; value: OpenDocRuntimeSession }
  | { kind: "AuthorizationDecision"; value: OpenDocAuthorizationDecision }
  | { kind: "ShareInvite"; value: OpenDocShareInvite }
  | { kind: "SyncRelay"; value: OpenDocSyncRelayResult }
  | { kind: "RuntimeLookup"; value: OpenDocRuntimeLookupResult };

export type EditorPosition = {
  block_id: string;
  inline_id: string | null;
  offset: number;
};

export type EditorSelection = {
  anchor: EditorPosition;
  focus: EditorPosition;
};

export type EditorInput = {
  selection: EditorSelection;
  input_type: string;
  data: string | null;
  html?: string | null;
};

export type EditorResult = {
  document: AppDocument;
  selection: EditorSelection;
  handled: boolean;
};
