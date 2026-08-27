export type AppDocument = {
  uuid: string;
  title: string;
  locale: string;
  visible_text: string;
  blocks: AppBlock[];
  comments: AppCommentThread[];
  suggestions: AppSuggestion[];
  citations: AppCitationDatabase;
  workbook: AppSpreadsheetWorkbook;
  warnings: AppWarning[];
  signature_state: string;
  signature: AppSignature | null;
  signatures: AppSignature[];
  repository_root: string | null;
  last_manifest: string | null;
  operation_count: number;
  operations: AppOperationRecord[];
};

export type AppBlock = {
  id: string;
  kind: string;
  level: number | null;
  ordered: boolean | null;
  equation_source: string | null;
  content: AppInline[];
  rows: AppBlock[][][];
};

export type AppInline = {
  id: string;
  kind: string;
  text: string;
  href: string | null;
  target_id: string | null;
  marks: string[];
};

export type AppCitationDatabase = {
  style: string;
  locale: string;
  references: AppBibliographyReference[];
  citations: AppCitationGroup[];
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
  state: string;
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
  sheets: AppSheet[];
};

export type AppSheet = {
  id: string;
  title: string;
  rows: string[];
  columns: string[];
  cells: AppCell[];
};

export type AppCell = {
  address: string;
  user_kind: string;
  user_value: string;
  computed_kind: string;
  computed_value: string;
  dependencies: string[];
};
