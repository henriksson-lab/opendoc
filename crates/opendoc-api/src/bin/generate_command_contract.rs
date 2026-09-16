use std::fs;
use std::path::PathBuf;

use opendoc_api::commands::{CommandArgType, CommandReturn, COMMANDS};

fn main() {
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("opendoc-api should live under crates/")
        .to_path_buf();
    let desktop_root = workspace.join("apps/desktop");
    let contract_path = desktop_root.join("commands.v0.json");
    let ts_path = desktop_root.join("src/generated/commands.ts");
    let audit_types_path = desktop_root.join("src/generated/audit.ts");
    let blob_types_path = desktop_root.join("src/generated/blob.ts");
    let citation_types_path = desktop_root.join("src/generated/citation.ts");
    let document_types_path = desktop_root.join("src/generated/document.ts");
    let editor_types_path = desktop_root.join("src/generated/editor.ts");
    let export_types_path = desktop_root.join("src/generated/export.ts");
    let layout_types_path = desktop_root.join("src/generated/layout.ts");
    let runtime_types_path = desktop_root.join("src/generated/runtime.ts");
    let spreadsheet_types_path = desktop_root.join("src/generated/spreadsheet.ts");
    let version_types_path = desktop_root.join("src/generated/version.ts");
    let docs_path = workspace.join("docs/APP_API_CONTRACT_V0.md");
    let check = std::env::args().any(|arg| arg == "--check");

    let contract = command_contract_json();
    let ts = command_bindings_ts();
    let audit_types = audit_types_ts();
    let blob_types = blob_types_ts();
    let citation_types = citation_types_ts();
    let document_types = document_types_ts();
    let editor_types = editor_types_ts();
    let export_types = export_types_ts();
    let layout_types = layout_types_ts();
    let runtime_types = runtime_types_ts();
    let spreadsheet_types = spreadsheet_types_ts();
    let version_types = version_types_ts();
    let command_reference = command_reference_markdown();

    if check {
        assert_same(&contract_path, &contract, "command registry JSON");
        assert_same(&ts_path, &ts, "TypeScript command bindings");
        assert_same(
            &audit_types_path,
            &audit_types,
            "TypeScript audit DTO bindings",
        );
        assert_same(
            &blob_types_path,
            &blob_types,
            "TypeScript blob DTO bindings",
        );
        assert_same(
            &citation_types_path,
            &citation_types,
            "TypeScript citation DTO bindings",
        );
        assert_same(
            &document_types_path,
            &document_types,
            "TypeScript document DTO bindings",
        );
        assert_same(
            &editor_types_path,
            &editor_types,
            "TypeScript editor DTO bindings",
        );
        assert_same(
            &export_types_path,
            &export_types,
            "TypeScript export DTO bindings",
        );
        assert_same(
            &layout_types_path,
            &layout_types,
            "TypeScript layout DTO bindings",
        );
        assert_same(
            &runtime_types_path,
            &runtime_types,
            "TypeScript runtime DTO bindings",
        );
        assert_same(
            &spreadsheet_types_path,
            &spreadsheet_types,
            "TypeScript spreadsheet DTO bindings",
        );
        assert_same(
            &version_types_path,
            &version_types,
            "TypeScript version DTO bindings",
        );
        assert_generated_section_same(&docs_path, &command_reference, "API command reference");
    } else {
        fs::write(&contract_path, contract).expect("write command registry JSON");
        fs::write(&ts_path, ts).expect("write TypeScript command bindings");
        fs::write(&audit_types_path, audit_types).expect("write TypeScript audit DTO bindings");
        fs::write(&blob_types_path, blob_types).expect("write TypeScript blob DTO bindings");
        fs::write(&citation_types_path, citation_types)
            .expect("write TypeScript citation DTO bindings");
        fs::write(&document_types_path, document_types)
            .expect("write TypeScript document DTO bindings");
        fs::write(&editor_types_path, editor_types).expect("write TypeScript editor DTO bindings");
        fs::write(&export_types_path, export_types).expect("write TypeScript export DTO bindings");
        fs::write(&layout_types_path, layout_types).expect("write TypeScript layout DTO bindings");
        fs::write(&runtime_types_path, runtime_types)
            .expect("write TypeScript runtime DTO bindings");
        fs::write(&spreadsheet_types_path, spreadsheet_types)
            .expect("write TypeScript spreadsheet DTO bindings");
        fs::write(&version_types_path, version_types)
            .expect("write TypeScript version DTO bindings");
        update_generated_section(&docs_path, &command_reference)
            .expect("write API command reference");
    }
}

fn assert_same(path: &PathBuf, expected: &str, label: &str) {
    let current = fs::read_to_string(path).unwrap_or_default();
    if current != expected {
        panic!("{label} is stale; run npm run generate:commands");
    }
}

fn command_contract_json() -> String {
    let mut out = String::from(
        "{\n  \"version\": 0,\n  \"projection\": \"../../docs/APP_API_CONTRACT_V0.md\",\n  \"commands\": [\n",
    );
    for (index, command) in COMMANDS.iter().enumerate() {
        out.push_str("    {\n");
        out.push_str(&format!("      \"name\": \"{}\",\n", command.name));
        out.push_str(&format!(
            "      \"returns\": \"{}\",\n",
            command.returns.as_contract_type()
        ));
        out.push_str("      \"args\": {");
        if command.args.is_empty() {
            out.push_str("}\n");
        } else {
            out.push('\n');
            for (arg_index, arg) in command.args.iter().enumerate() {
                let comma = if arg_index + 1 == command.args.len() {
                    ""
                } else {
                    ","
                };
                let ty = if arg.optional {
                    format!("{}?", arg.ty.as_contract_type())
                } else {
                    arg.ty.as_contract_type().to_string()
                };
                out.push_str(&format!("        \"{}\": \"{}\"{}\n", arg.name, ty, comma));
            }
            out.push_str("      }\n");
        }
        let comma = if index + 1 == COMMANDS.len() { "" } else { "," };
        out.push_str(&format!("    }}{}\n", comma));
    }
    out.push_str("  ]\n}\n");
    out
}

fn command_bindings_ts() -> String {
    let mut names_by_return = std::collections::BTreeMap::<CommandReturn, Vec<&str>>::new();
    for command in COMMANDS {
        names_by_return
            .entry(command.returns)
            .or_default()
            .push(command.name);
    }

    format!(
        r#"// Generated by cargo run -p opendoc-api --bin generate_command_contract from Rust command metadata.
// Do not edit by hand.

import type {{ AppAuditView }} from "./audit";
import type {{ AppCitationItem }} from "./citation";
import type {{ AppDocument, EditorResult }} from "./document";
import type {{ AppEditorSelection, AppFindMatches, EditorSelection }} from "./editor";
import type {{ AppExport }} from "./export";
import type {{ AppDocumentLayout }} from "./layout";
import type {{ AppSpreadsheetSelection }} from "./spreadsheet";
import type {{ AppVersionView }} from "./version";
import type {{
  OpenDocAuthorizationDecision,
  OpenDocRuntimeLookupResult,
  OpenDocRuntimeMode,
  OpenDocRuntimeProfile,
  OpenDocRuntimeSession,
  OpenDocShareInvite,
  OpenDocSyncRelayResult,
}} from "./runtime";

export type SpreadsheetCellEdit = {{ address: string; value: string }};

export type AppCommandResult =
  | {{ kind: "Document"; value: AppDocument }}
  | {{ kind: "Editor"; value: EditorResult }}
  | {{ kind: "Text"; value: string }}
  | {{ kind: "AuditView"; value: AppAuditView }}
  | {{ kind: "RuntimeProfile"; value: OpenDocRuntimeProfile }}
  | {{ kind: "RuntimeSession"; value: OpenDocRuntimeSession }}
  | {{ kind: "AuthorizationDecision"; value: OpenDocAuthorizationDecision }}
  | {{ kind: "ShareInvite"; value: OpenDocShareInvite }}
  | {{ kind: "SyncRelay"; value: OpenDocSyncRelayResult }}
  | {{ kind: "RuntimeLookup"; value: OpenDocRuntimeLookupResult }}
  | {{ kind: "SpreadsheetSelection"; value: AppSpreadsheetSelection }}
  | {{ kind: "EditorSelection"; value: AppEditorSelection }}
  | {{ kind: "FindMatches"; value: AppFindMatches }}
  | {{ kind: "VersionView"; value: AppVersionView }}
  | {{ kind: "Export"; value: AppExport }}
  | {{ kind: "DocumentLayout"; value: AppDocumentLayout }};

export type DesktopCommandArgs = {{
{}
}};

export type DesktopCommandName = keyof DesktopCommandArgs;
export type TextCommandName =
{};
export type EditorCommandName =
{};
export type AuditCommandName =
{};
export type RuntimeProfileCommandName =
{};
export type RuntimeSessionCommandName =
{};
export type AuthorizationCommandName =
{};
export type ShareInviteCommandName =
{};
export type SyncRelayCommandName =
{};
export type RuntimeLookupCommandName =
{};
export type SpreadsheetSelectionCommandName =
{};
export type EditorSelectionCommandName =
{};
export type FindMatchesCommandName =
{};
export type VersionViewCommandName =
{};
export type ExportCommandName =
{};
export type DocumentLayoutCommandName =
{};
export type DocumentCommandName = Exclude<
  DesktopCommandName,
  | TextCommandName
  | EditorCommandName
  | AuditCommandName
  | RuntimeProfileCommandName
  | RuntimeSessionCommandName
  | AuthorizationCommandName
  | ShareInviteCommandName
  | SyncRelayCommandName
  | RuntimeLookupCommandName
  | SpreadsheetSelectionCommandName
  | EditorSelectionCommandName
  | FindMatchesCommandName
  | VersionViewCommandName
  | ExportCommandName
  | DocumentLayoutCommandName
>;
export type CommandArgs<K extends DesktopCommandName> = DesktopCommandArgs[K];
export type CommandResult<K extends DesktopCommandName> = K extends TextCommandName
  ? string
  : K extends EditorCommandName
    ? EditorResult
    : K extends AuditCommandName
      ? AppAuditView
      : K extends RuntimeProfileCommandName
        ? OpenDocRuntimeProfile
        : K extends RuntimeSessionCommandName
          ? OpenDocRuntimeSession
          : K extends AuthorizationCommandName
            ? OpenDocAuthorizationDecision
            : K extends ShareInviteCommandName
              ? OpenDocShareInvite
              : K extends SyncRelayCommandName
                ? OpenDocSyncRelayResult
                : K extends RuntimeLookupCommandName
                ? OpenDocRuntimeLookupResult
                : K extends SpreadsheetSelectionCommandName
                  ? AppSpreadsheetSelection
                  : K extends EditorSelectionCommandName
                    ? AppEditorSelection
                    : K extends FindMatchesCommandName
                      ? AppFindMatches
                      : K extends VersionViewCommandName
                        ? AppVersionView
                        : K extends ExportCommandName
                          ? AppExport
                          : K extends DocumentLayoutCommandName
                            ? AppDocumentLayout
                            : AppDocument;
"#,
        COMMANDS
            .iter()
            .map(|command| format!("  {}: {};", command.name, args_type(command.args)))
            .collect::<Vec<_>>()
            .join("\n"),
        union_type(
            names_by_return
                .get(&CommandReturn::String)
                .map(Vec::as_slice)
                .unwrap_or(&[])
        ),
        union_type(
            names_by_return
                .get(&CommandReturn::EditorResult)
                .map(Vec::as_slice)
                .unwrap_or(&[])
        ),
        union_type(
            names_by_return
                .get(&CommandReturn::AppAuditView)
                .map(Vec::as_slice)
                .unwrap_or(&[])
        ),
        union_type(
            names_by_return
                .get(&CommandReturn::OpenDocRuntimeProfile)
                .map(Vec::as_slice)
                .unwrap_or(&[])
        ),
        union_type(
            names_by_return
                .get(&CommandReturn::OpenDocRuntimeSession)
                .map(Vec::as_slice)
                .unwrap_or(&[])
        ),
        union_type(
            names_by_return
                .get(&CommandReturn::OpenDocAuthorizationDecision)
                .map(Vec::as_slice)
                .unwrap_or(&[])
        ),
        union_type(
            names_by_return
                .get(&CommandReturn::OpenDocShareInvite)
                .map(Vec::as_slice)
                .unwrap_or(&[])
        ),
        union_type(
            names_by_return
                .get(&CommandReturn::OpenDocSyncRelayResult)
                .map(Vec::as_slice)
                .unwrap_or(&[])
        ),
        union_type(
            names_by_return
                .get(&CommandReturn::OpenDocRuntimeLookupResult)
                .map(Vec::as_slice)
                .unwrap_or(&[])
        ),
        union_type(
            names_by_return
                .get(&CommandReturn::AppSpreadsheetSelection)
                .map(Vec::as_slice)
                .unwrap_or(&[])
        ),
        union_type(
            names_by_return
                .get(&CommandReturn::AppEditorSelection)
                .map(Vec::as_slice)
                .unwrap_or(&[])
        ),
        union_type(
            names_by_return
                .get(&CommandReturn::AppFindMatches)
                .map(Vec::as_slice)
                .unwrap_or(&[])
        ),
        union_type(
            names_by_return
                .get(&CommandReturn::AppVersionView)
                .map(Vec::as_slice)
                .unwrap_or(&[])
        ),
        union_type(
            names_by_return
                .get(&CommandReturn::AppExport)
                .map(Vec::as_slice)
                .unwrap_or(&[])
        ),
        union_type(
            names_by_return
                .get(&CommandReturn::AppDocumentLayout)
                .map(Vec::as_slice)
                .unwrap_or(&[])
        ),
    )
}

fn version_types_ts() -> String {
    r#"// Generated by cargo run -p opendoc-api --bin generate_command_contract from Rust version DTOs.
// Do not edit by hand.

import type { AppDocument } from "./document";
import type { AppWarning } from "./audit";

export type AppVersionSigner = {
  signer: string;
  signer_display: string;
  title: string;
  signed_at_ms: number;
};

export type AppDocumentVersion = {
  manifest: string;
  parent: string | null;
  snapshot: string;
  created_at_ms: number;
  signers: AppVersionSigner[];
  label: string | null;
  label_author: string | null;
  snapshot_present: boolean;
  is_head: boolean;
  is_current: boolean;
};

export type AppVersionPreview = {
  manifest: string;
  read_only: boolean;
  document: AppDocument;
  /** One line per top-level block, already reduced to a kind label and the
   * text that block says — which of content / equation source / alt text
   * stands in for a block is Rust's rule, not the panel's. */
  blocks: AppVersionPreviewBlock[];
};

export type AppVersionPreviewBlock = {
  /** "paragraph", "heading 2", "table (3 rows)", … */
  kind: string;
  /** What the block says, capped to a preview-sized excerpt. */
  text: string;
};

export type AppVersionDiffEntry = {
  change: string;
  block_id: string;
  kind: string;
  path: string;
  before_text: string;
  after_text: string;
};

export type AppVersionDiff = {
  from_manifest: string;
  to_manifest: string;
  added: number;
  removed: number;
  changed: number;
  entries: AppVersionDiffEntry[];
};

export type AppVersionView = {
  document_uuid: string;
  branch: string;
  repository_root: string | null;
  repository_backend: string | null;
  head: string | null;
  current: string | null;
  versions: AppDocumentVersion[];
  truncated: boolean;
  preview: AppVersionPreview | null;
  diff: AppVersionDiff | null;
  warnings: AppWarning[];
};
"#
    .to_string()
}

fn audit_types_ts() -> String {
    r#"// Generated by cargo run -p opendoc-api --bin generate_command_contract from Rust audit DTOs.
// Do not edit by hand.

import type {
  AppBibliographyReference,
  AppCitationGroup,
  AppCommentThread,
  AppSuggestion,
} from "./document";
import type {
  AppCellValidation,
  AppDeletedCellComment,
  AppDeletedColumnPayload,
  AppDeletedRowPayload,
  AppNamedRange,
  AppSheetFilter,
  AppSheetMerge,
  AppSheetProtectedRange,
} from "./spreadsheet";
import type { AppTypedContentSignature } from "./blob";

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

export type AppDeletedColumn = {
  sheet_id: string;
  column: string;
  payload: AppDeletedColumnPayload;
  operation: AppOperationRecord;
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

export type AppRecoverySession = {
  id: string;
  document_uuid: string;
  title: string;
  started_at_ms: number;
  operation_count: number;
  repository_root: string | null;
  repository_backend: string | null;
  base_manifest: string | null;
  operations: AppOperationRecord[];
  truncated: boolean;
};

export type AppSignature = {
  target: string;
  signer: string;
  signer_display: string;
  title: string;
  signed_at_ms: number;
};
"#
    .to_string()
}

fn blob_types_ts() -> String {
    r#"// Generated by cargo run -p opendoc-api --bin generate_command_contract from Rust blob DTOs.
// Do not edit by hand.

import type { AppArchiveTombstone, AppSignature } from "./audit";

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
"#
    .to_string()
}

fn citation_types_ts() -> String {
    r#"// Generated by cargo run -p opendoc-api --bin generate_command_contract from Rust citation DTOs.
// Do not edit by hand.

export type AppCitationItem = {
  reference_id: string;
  locator: string | null;
  label: string | null;
  prefix: string | null;
  suffix: string | null;
  suppress_author: boolean;
};
"#
    .to_string()
}

fn document_types_ts() -> String {
    let insert_first = format!("{:?}", opendoc_core::InsertPosition::FIRST_KEYWORD);
    let line_spacing_presets = opendoc_core::LineSpacing::PRESETS
        .iter()
        .map(|preset| {
            format!(
                "  {{ mode: {:?}, value: {}, label: {:?} }},",
                preset.mode(),
                preset.value(),
                preset.label()
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let constants = format!(
        r#"
// Unit factors of `opendoc_core::Length` — the unit every document length in
// this contract is stated in. Divide by these; never write 20 or 1440 out.
export const APP_TWIPS_PER_POINT = {twips_per_point};
export const APP_TWIPS_PER_INCH = {twips_per_inch};
// Bounds `opendoc_core::Length` accepts. Indents and first-line offsets may be
// negative (a hanging indent, or content bled into the margin).
export const APP_MIN_LENGTH_TWIPS = {min_length};
export const APP_MAX_LENGTH_TWIPS = {max_length};
// Bounds accepted by set_image_block_width / set_image_block_height /
// set_image_block_size. Outside them the command fails rather than clamps.
export const APP_MIN_IMAGE_TWIPS = {min_image};
export const APP_MAX_IMAGE_TWIPS = {max_image};
// The value an `afterRow` / `afterColumnId` argument takes to mean "before
// every existing sibling". Omitting the argument still means *append* — that
// is what makes a replayed insert whose anchor was deleted degrade to the end
// instead of vanishing — so the one position an id cannot name has a keyword
// of its own instead of a second optional argument beside it.
export const APP_INSERT_FIRST = {insert_first};

/** One entry of {{@link APP_LINE_SPACING_PRESETS}}. `mode` and `value` are the
 *  pair set_block_line_spacing and set_editor_selection_block_line_spacing
 *  take; `label` is how that spacing reads to a person. */
export type AppLineSpacingPreset = {{
  mode: string;
  value: number;
  label: string;
}};

/** The line spacings the UI offers, in the order it should offer them. The
 *  mode/value pair is the model's own encoding, so nothing has to join it into
 *  a string or take one apart again. */
export const APP_LINE_SPACING_PRESETS: AppLineSpacingPreset[] = [
{line_spacing_presets}
];
"#,
        twips_per_point = opendoc_core::Length::TWIPS_PER_POINT,
        twips_per_inch = opendoc_core::Length::TWIPS_PER_INCH,
        min_length = opendoc_core::Length::MIN_TWIPS,
        max_length = opendoc_core::Length::MAX_TWIPS,
        min_image = opendoc_core::ImageLayout::MIN_TWIPS,
        max_image = opendoc_core::ImageLayout::MAX_TWIPS,
    );
    let types = r#"// Generated by cargo run -p opendoc-api --bin generate_command_contract from Rust document DTOs.
// Do not edit by hand.

import type {
  AppOperationRecord,
  AppRecentDocument,
  AppRecoverySession,
  AppSignature,
  AppWarning,
} from "./audit";
import type { AppBlobRef } from "./blob";
import type { AppCitationItem } from "./citation";
import type { EditorSelection } from "./editor";
import type { AppSpreadsheetWorkbook } from "./spreadsheet";

export type AppDocument = {
  is_open: boolean;
  uuid: string;
  title: string;
  locale: string;
  doi: string | null;
  /** The sheet the document is laid out on. Source state: stored, signed and
   * merged. Lengths are twips (twentieths of a point). */
  page_setup: AppPageSetup;
  /** Blocks repeated at the top of every page. Source state. */
  header: AppBlock[];
  /** Blocks repeated at the bottom of every page. Source state. */
  footer: AppBlock[];
  /** `null` inherits `header`; `[]` intentionally suppresses page-one's header. */
  first_page_header?: AppBlock[];
  /** `null` inherits `footer`; `[]` intentionally suppresses page-one's footer. */
  first_page_footer?: AppBlock[];
  /** `null` inherits `header`; `[]` intentionally suppresses even-page headers. */
  even_page_header?: AppBlock[];
  /** `null` inherits `footer`; `[]` intentionally suppresses even-page footers. */
  even_page_footer?: AppBlock[];
  /** Numbering starts keyed by stable list-run id. Missing level entries start
   * at one; this is source state rather than a renderer inference. */
  list_properties?: Record<string, AppListProperties>;
  /** Durable character-token source keyed by editable text/link inline id.
   * Missing means a legacy snapshot which has not yet been materialized. */
  text_sequences?: Record<string, AppTextSequence>;
  /** Durable named navigation targets. Deleted entries are retained as
   * tombstones so an old replica cannot resurrect them. */
  bookmarks: AppBookmark[];
  /** Everything about the page that is derived rather than stored. Projection
   * only: it never reaches a snapshot or a signature. */
  page_layout: AppPageLayout;
  /** Word and character counts of the document text. The text itself is not
   * carried: it was 83 KB of every keystroke's payload with no reader here. */
  word_count: number;
  character_count: number;
  blocks: AppBlock[];
  footnotes: AppFootnote[];
  /** Footnotes rendered after the ordinary footnote trailer. */
  endnote_ids: string[];
  comments: AppCommentThread[];
  /** Durable review provenance for comment edits, deletion, and restoration. */
  comment_history: AppCommentHistoryEntry[];
  /** Bounded, read-only document-local review activity. */
  comment_activity: AppCommentActivityEntry[];
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
  recovery_sessions: AppRecoverySession[];
  /** The rendered body as its ordered top-level elements, not as one string.
   * A consumer applies the body a fragment at a time and re-parses only the
   * ones whose markup changed; the pieces concatenate, in order, to exactly
   * the whole body. */
  body_fragments: AppBodyFragment[];
  footnotes_html: string;
  /** Rendered header markup, rendered once. Repeating it on every page is
   * pagination's job — see docs/adr/0009-pagination-and-page-geometry.md. */
  header_html: string;
  /** Rendered footer markup, rendered once. */
  footer_html: string;
  /** Rendered first-page header override, once. */
  first_page_header_html: string;
  /** Rendered first-page footer override, once. */
  first_page_footer_html: string;
  /** Rendered even-page header override, once. */
  even_page_header_html: string;
  /** Rendered even-page footer override, once. */
  even_page_footer_html: string;
};

export type AppListProperties = {
  ordered_starts?: Record<string, number>;
  /** Explicit list-run counter styles. Missing levels use the depth cycle. */
  ordered_formats?: Record<string, "decimal" | "lower-alpha" | "upper-alpha" | "lower-roman" | "upper-roman">;
  /** Explicit safe Unicode unordered-list glyphs. Missing levels use disc/circle/square. */
  bullet_markers?: Record<string, string>;
};

/** The persisted token sequence for one editable text/link inline. */
export type AppTextSequence = {
  tokens: AppTextToken[];
};

/** One scalar atom in a persisted text sequence. The token identity is serde's
 * externally tagged `TextTokenId` enum; predecessors are retained even when
 * a token is tombstoned. */
export type AppTextToken = {
  id: { Baseline: { document_uuid: string; inline_id: string; ordinal: number } } | { Operation: { actor: string; sequence: number; ordinal: number } };
  predecessor: { Baseline: { document_uuid: string; inline_id: string; ordinal: number } } | { Operation: { actor: string; sequence: number; ordinal: number } } | null;
  scalar: string;
  tombstoned: boolean;
};

/** A named stable block target with a revisioned deletion tombstone. */
export type AppBookmark = {
  id: string;
  name: string;
  block_id: string;
  revision: number;
  deleted: boolean;
};

/** Immutable review evidence for a comment lifecycle event. */
export type AppCommentHistoryEntry = {
  thread_id: string;
  comment_id: string;
  /** "edited" | "deleted" | "restored" */
  kind: string;
  actor: string;
  at_ms: number;
  previous_body: AppInline[] | null;
};

/** One replayable event in the document-local review activity stream. */
export type AppCommentActivityEntry = {
  operation_actor: string;
  operation_seq: number;
  actor: string;
  /** Deterministic logical time, not wall-clock time. */
  at_ms: number;
  thread_id: string;
  comment_id: string | null;
  kind: "thread_created" | "reply_added" | "thread_resolved" | "thread_reopened" | "thread_deleted" | "thread_restored" | "comment_edited" | "comment_deleted" | "comment_restored" | "action_set" | "reaction_added" | "reaction_removed";
};

/** One top-level element of the rendered body.
 *
 * The finest cut the renderer can make: a paragraph, a table, an image — or a
 * whole list run, since a nested item's closing tag is written after its child
 * list closes. `blocks` is how many blocks the element covers, one for
 * everything but a list run. `block_id` is the first block it renders, unique
 * across a body and also the first `data-block-id` inside `html`, which is how
 * a consumer finds the live element the fragment belongs to. */
export type AppBodyFragment = {
  block_id: string;
  blocks: number;
  html: string;
};

/** Page geometry in twips (twentieths of a point), the unit the model stores,
 * so the projection cannot drift by rounding. `start`/`end` margins are
 * direction-relative, like block indents. */
export type AppPageSetup = {
  width_twips: number;
  height_twips: number;
  margin_top_twips: number;
  margin_bottom_twips: number;
  margin_start_twips: number;
  margin_end_twips: number;
  margin_header_twips: number;
  margin_footer_twips: number;
  /** Displayed number for the first physical page. */
  page_number_start: number;
};

/** Everything about the page derived from AppPageSetup rather than stored
 * beside it. Projection only. */
export type AppPageLayout = {
  /** The standard size these dimensions are, in either orientation, or null
   * for a custom page. Recovered by measuring. */
  size_name: string | null;
  /** "portrait" | "landscape", derived from the dimensions. */
  orientation: string;
  /** Page geometry as CSS custom properties, ready for a style attribute. */
  style: string;
  /** The same geometry as an @page rule. Custom properties do not apply inside
   * @page, so the print box needs its own concrete projection. */
  print_style: string;
  /** The sizes the page-setup dialog offers, so the frontend never hard-codes
   * a paper dimension. */
  size_presets: AppPageSizePreset[];
};

export type AppPageSizePreset = {
  name: string;
  label: string;
  width_twips: number;
  height_twips: number;
};

export type AppBlock = {
  id: string;
  kind: string;
  level?: number;
  /** Projection of `list_kind`: true only for an ordered list item. A checklist
   * item is false here — read `list_kind` to tell a checklist from a bullet. */
  ordered?: boolean;
  /** The list run this item belongs to. Adjacent items sharing this id are one
   * list; a different id starts a new list and restarts numbering. */
  list_id?: string;
  /** "bullet" | "ordered" | "checklist" */
  list_kind?: string;
  /** Checkbox state, present only for checklist items. */
  checked?: boolean;
  properties?: AppBlockProperties;
  style_value: string;
  equation_source?: string;
  blob_hash?: string;
  alt_text?: string;
  /** Display width of an image block, in twips. Absent means the image is drawn
   * at the size its bytes decode to — never filled in with that size. */
  image_width_twips?: number;
  /** Display height, in twips. Absent with a width present means "scale to keep
   * the aspect ratio". */
  image_height_twips?: number;
  /** "block" | "wrap-start" | "wrap-end" */
  image_placement?: string;
  /** Authored logical wrap clearance, in twips; absent uses target defaults. */
  image_wrap_clearance?: { top: number; end: number; bottom: number; start: number };
  /** Clockwise visual rotation in whole degrees. */
  image_rotation_degrees?: number;
  /** Image opacity in the inclusive range 0..100. */
  image_opacity_percent?: number;
  image_crop_top_percent?: number;
  image_crop_right_percent?: number;
  image_crop_bottom_percent?: number;
  image_crop_left_percent?: number;
  image_caption?: string;
  image_border?: AppCellBorder;
  /** Durable positioned-object geometry (ADR 0022). */
  image_positioned?: AppPositionedImage;
  content: AppInline[];
  /** The contents of a table block's grid, cell by cell. Absent on every other
   * kind of block. */
  rows?: AppBlock[][][];
  row_ids?: string[];
  cell_ids?: string[][];
  /** The grid's shape — columns and per-cell spans and styling — present only
   * on a table block. `rows`, `row_ids` and `cell_ids` carry its contents. */
  table?: AppTable;
};

/** An image coordinate system and layer. Values retain Rust's serde shape. */
export type AppPositionedImage = {
  anchor: "PageContent" | { Block: string };
  horizontal_offset: number;
  vertical_offset: number;
  layer: "BehindText" | "InFrontOfText";
};

/** The shape of a table block. `cells` is in the same order as `rows`. */
export type AppTable = {
  columns: AppTableColumn[];
  /** The table-wide border inherited by cell edges that state none of their
   * own. Absent means the document leaves the table border unspecified. */
  border?: AppCellBorder;
  /** "start" | "center" | "end". This positions the table box, not cell text. */
  alignment?: string;
  /** Explicit row heights in twips, parallel with `row_ids`. An absent item
   * means the row grows to its content. */
  row_heights_twips?: (number | undefined)[];
  /** Semantic header flags, parallel with `row_ids`. */
  row_headers?: boolean[];
  cells: AppTableCell[][];
};

export type AppTableColumn = {
  id: string;
  /** Absent means auto: the view shares out what the sized columns leave. */
  width_twips?: number;
};

export type AppTableCell = {
  row_span: number;
  column_span: number;
  /** Whether this cell is hidden underneath a merged neighbour. Derived from
   * the spans in Rust, so the view never works the geometry out itself. */
  covered: boolean;
  properties?: AppTableCellProperties;
};

/** Cell-level formatting. Lengths are twips, colours are hex (#rrggbb), and an
 * absent field means the cell inherits that property. */
export type AppTableCellProperties = {
  background?: string;
  border_top?: AppCellBorder;
  border_bottom?: AppCellBorder;
  border_start?: AppCellBorder;
  border_end?: AppCellBorder;
  /** "top" | "middle" | "bottom" */
  vertical_alignment?: string;
  /** Explicit semantic row-header state; absent has no authored role. */
  row_header?: boolean;
  padding_top_twips?: number;
  padding_bottom_twips?: number;
  padding_start_twips?: number;
  padding_end_twips?: number;
};

export type AppCellBorder = {
  /** "none" | "solid" | "dashed" | "dotted" | "double" */
  style: string;
  twips: number;
  color: string;
};

/** Block-level paragraph formatting. Lengths are twips (twentieths of a point),
 * the unit the model stores, so the projection cannot drift by rounding.
 * An absent field means the block inherits that property; a block that
 * inherits every one of them has no properties object at all. */
export type AppBlockProperties = {
  /** "start" | "center" | "end" | "justify" */
  alignment?: string;
  indent_start_twips?: number;
  indent_end_twips?: number;
  /** Negative means a hanging indent. */
  indent_first_line_twips?: number;
  /** "multiple" | "exact" | "at-least" */
  line_spacing_mode?: string;
  /** Thousandths of a line for "multiple", twips for the other two modes. */
  line_spacing_value?: number;
  /** How the pair above reads to a person — "Single", "1.15×", "Exactly 24 pt".
   * A projection: the commands never take it, and setting it changes nothing. */
  line_spacing_label?: string;
  space_before_twips?: number;
  space_after_twips?: number;
  /** "ltr" | "rtl" */
  direction?: string;
  /** Keep this paragraph with its next sibling when pagination permits. */
  keep_with_next?: boolean;
  /** Flat paragraph background in canonical #rrggbb form. */
  background?: string;
  /** One uniform paragraph frame; table cells retain per-edge borders. */
  border?: AppCellBorder;
};

export type AppInline = {
  id: string;
  kind: string;
  text: string;
  href?: string;
  target_id?: string;
  marks?: string[];
  mark_kinds?: string[];
  mark_values: Record<string, string>;
  dropdown_options?: AppDropdownOption[];
  selected_option_id?: string;
  /** Canonical YYYY-MM-DD value of an atomic date chip. */
  date?: string;
  /** Read-only Google person identity carried by an imported smart chip. */
  google_person_email?: string;
  google_person_id?: string;
  /** Read-only provider metadata carried by an imported rich-link chip. */
  google_rich_link_id?: string;
  google_rich_link_mime_type?: string;
};

export type AppDropdownOption = {
  id: string;
  label: string;
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

export type AppCommentThread = {
  id: string;
  anchor: string;
  anchor_label: string;
  /** Evidence retained when a comment's original target was deleted. */
  orphaned_quote: string | null;
  orphaned_context: string | null;
  /** Provenance warning retained with deleted-anchor evidence. */
  orphaned_warning: string | null;
  comments: AppComment[];
  state: string;
  resolved_by: string | null;
  resolved_at_ms: number | null;
  /** Offline display-name assignment; identity resolution belongs to a service. */
  action_assignee: string | null;
  action_due_at_ms: number | null;
  action_completed_by: string | null;
  action_completed_at_ms: number | null;
  reactions: AppCommentThreadReaction[];
  deleted: boolean;
};

/** A document-local emoji reaction and the actors that hold it. */
export type AppCommentThreadReaction = {
  emoji: string;
  actors: string[];
};

export type AppComment = {
  id: string;
  author: string;
  body: string;
  deleted: boolean;
  created_at_ms: number;
};

export type AppSuggestion = {
  id: string;
  author: string;
  kind: string;
  text: string;
  state: string;
  anchor: string | null;
  anchor_label: string | null;
  range_start: string | null;
  range_end: string | null;
  /** Stable target for a structural suggestion, when applicable. */
  block_id: string | null;
  /** Proposed paragraph identity for a structural insert or replacement. */
  structural_block_id: string | null;
  /** Exact source paragraph retained for a block-replacement compare-and-set. */
  structural_expected_block: AppBlock | null;
  /** Proposed insertion placement, when this is a structural insert. */
  block_position: string | null;
  /** Atomic target for a proposed link add, removal, or replacement. */
  link_inline_id: string | null;
  link_expected_href: string | null;
  link_href: string | null;
  /** Source style retained as an acceptance precondition. */
  paragraph_style_expected: string | null;
  /** Style a reviewer may apply to the same surviving block. */
  paragraph_style_proposed: string | null;
  /** Source value retained by a value-bearing format replacement. */
  format_expected_value: string | null;
  marks: string[];
  content: AppInline[];
  provenance: string[];
};

export type EditorResult = {
  document: AppDocument;
  selection: EditorSelection;
  handled: boolean;
};
"#;
    format!("{types}{constants}")
}

fn editor_types_ts() -> String {
    r#"// Generated by cargo run -p opendoc-api --bin generate_command_contract from Rust editor DTOs.
// Do not edit by hand.

export type EditorPosition = {
  block_id: string;
  inline_id: string | null;
  offset: number;
};

export type EditorSelection = {
  anchor: EditorPosition;
  focus: EditorPosition;
};

export type EditorInlineRange = {
  start: string;
  end: string;
};

export type AppEditorSelection = {
  selected_block_ids: string[];
  focus_block_id: string | null;
  inline_range: EditorInlineRange | null;
};

/** The one DTO here that only travels towards Rust: `editor.ts` builds it and
 *  nothing projects it. `?` therefore means "a client may leave this out", and
 *  it may be narrower than what serde would in fact accept — `data` is optional
 *  to serde but every caller has one, so the type asks for it. */
export type EditorInput = {
  selection: EditorSelection;
  input_type: string;
  data: string | null;
  html?: string | null;
};

/** Which part of the document a match is in. Only `"body"` is selectable in
 *  the editor surface: page furniture and footnote bodies are not in
 *  `document.blocks`, so their block ids are not in the editor DOM. */
export type AppFindRegion = "body" | "header" | "footer" | "first-page-header" | "first-page-footer" | "even-page-header" | "even-page-footer" | "footnote";

export type AppFindMatch = {
  start: EditorPosition;
  end: EditorPosition;
  text: string;
  region: AppFindRegion;
};

export type AppFindMatches = {
  matches: AppFindMatch[];
};
"#
    .to_string()
}

fn export_types_ts() -> String {
    r#"// Generated by cargo run -p opendoc-api --bin generate_command_contract from Rust export DTOs.
// Do not edit by hand.

import type { AppWarning } from "./audit";

export type AppExportEncoding = "text" | "base64";

export type AppExport = {
  content: string;
  encoding: AppExportEncoding;
  media_type: string;
  file_extension: string;
  warnings: AppWarning[];
};
"#
    .to_string()
}

/// The layout DTOs. Positions are twips — the model's own unit, so a test or
/// a PDF writer can assert on them without converting — while the one value
/// the frontend applies verbatim is already a CSS length, so the frontend
/// does no arithmetic.
fn layout_types_ts() -> String {
    r#"// Generated by cargo run -p opendoc-api --bin generate_command_contract from Rust layout DTOs.
// Do not edit by hand.

export type AppBlockPlacement = {
  block_id: string;
  page: number;
  top_twips: number;
  height_twips: number;
  lines: number;
  page_break_margin?: string;
  exact: boolean;
};

export type AppDocumentLayout = {
  page_count: number;
  exact: boolean;
  style: string;
  /** The `list-style-type` cycle for nested lists, as complete CSS rules.
   *  Written by the layout engine from the same function the painted page
   *  picks its markers with, so the stylesheet cannot state a different
   *  last rule from the one the PDF cycles to. */
  list_style: string;
  blocks: AppBlockPlacement[];
};
"#
    .to_string()
}

fn runtime_types_ts() -> String {
    r#"// Generated by cargo run -p opendoc-api --bin generate_command_contract from Rust runtime DTOs.
// Do not edit by hand.

export type OpenDocRuntimeMode =
  | "tauri-local"
  | "browser-local"
  | "hpc-single-user"
  | "multi-user-service";

export type OpenDocStorageBackend = "local" | "flat" | "opendal-fs";

export type OpenDocPermissionAuthority = "local-advisory" | "service";

export type OpenDocServiceRole = "viewer" | "commenter" | "editor" | "owner";

export type OpenDocRuntimeProfile = {
  mode: OpenDocRuntimeMode;
  label: string;
  permission_authority: OpenDocPermissionAuthority;
  signing_enabled: boolean;
  browser_signing_deferred: boolean;
  default_repository_root: string;
  default_flat_namespace: string;
  storage_backends: OpenDocStorageBackend[];
};

// The collaboration service's answers about this client's session, exactly as
// they arrived. Nothing here can be supplied by a command argument.
export type OpenDocServiceSession = {
  subject: string;
  actor: string;
  document_uuid: string;
  role: OpenDocServiceRole;
  peers: OpenDocPresencePeer[];
  acknowledged_seq: number;
};

export type OpenDocRuntimeSession = {
  profile: OpenDocRuntimeProfile;
  service_session: OpenDocServiceSession | null;
  warnings: string[];
};

export type OpenDocPresencePeer = {
  subject: string;
  actor: string;
  display_name: string;
  role: OpenDocServiceRole;
  cursor_anchor: string | null;
  selection_anchor: string | null;
  last_seen_ms: number;
  connections: number;
};

export type OpenDocAuthorizationSource =
  | "runtime-capability"
  | "service-answer"
  | "service-answer-missing";

export type OpenDocAuthorizationDecision = {
  mode: OpenDocRuntimeMode;
  command: string;
  required_action: string;
  decided_by: OpenDocAuthorizationSource;
  subject: string | null;
  document_uuid: string | null;
  role: OpenDocServiceRole | null;
  allowed: boolean;
  reason: string;
  warnings: string[];
};

export type OpenDocShareInvite = {
  authorization: OpenDocAuthorizationDecision;
  issuer: string | null;
  target_subject: string | null;
  document_uuid: string | null;
  requested_role: OpenDocServiceRole | null;
  created_at_ms: number;
  warnings: string[];
};

export type OpenDocRelayOperation = {
  actor: string;
  seq: number;
  kind: string;
};

// A batch is accepted, refused, or a byte-identical retry. There is no
// deferred state: a batch the service cannot place is refused and nothing is
// stored.
export type OpenDocSyncBatchOutcome = "accepted" | "refused" | "retried";

export type OpenDocSyncRelayResult = {
  authorization: OpenDocAuthorizationDecision;
  document_uuid: string | null;
  outcome: OpenDocSyncBatchOutcome;
  accepted_operations: string[];
  retried_operations: string[];
  refused_operations: string[];
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
"#
    .to_string()
}

fn spreadsheet_types_ts() -> String {
    let sizing = format!(
        r#"
// Rendered size in CSS pixels for rows and columns without an explicit
// entry in `AppSheet.row_heights` / `AppSheet.column_widths`.
export const APP_DEFAULT_ROW_HEIGHT_PX = {default_row_height};
export const APP_DEFAULT_COLUMN_WIDTH_PX = {default_column_width};
// Bounds accepted by set_spreadsheet_row_height / set_spreadsheet_column_width.
// A size of 0 clears the explicit size and restores the default.
export const APP_MIN_AXIS_SIZE_PX = {min_axis_size};
export const APP_MAX_AXIS_SIZE_PX = {max_axis_size};
"#,
        default_row_height = opendoc_spreadsheet::DEFAULT_ROW_HEIGHT_PX,
        default_column_width = opendoc_spreadsheet::DEFAULT_COLUMN_WIDTH_PX,
        min_axis_size = opendoc_spreadsheet::MIN_AXIS_SIZE_PX,
        max_axis_size = opendoc_spreadsheet::MAX_AXIS_SIZE_PX,
    );
    let types = r#"// Generated by cargo run -p opendoc-api --bin generate_command_contract from Rust spreadsheet DTOs.
// Do not edit by hand.

export type AppSpreadsheetWorkbook = {
  title: string;
  locale: string;
  timezone: string;
  named_ranges: AppNamedRange[];
  dependency_graph: AppCellDependency[];
  sheets: AppSheet[];
  evaluation_context?: AppSpreadsheetEvaluationContext;
  evaluation_warnings: AppWarning[];
};

export type AppSpreadsheetEvaluationContext = {
  now_ms: number;
  seed: number;
};

export type AppSpreadsheetSelection = {
  anchor: string;
  focus: string;
  range: string;
  from_address: string;
  to_address: string;
  from_col: number;
  from_row: number;
  to_col: number;
  to_row: number;
  numeric_count: number;
  numeric_sum: string;
  numeric_average: string | null;
  summary_label: string;
  selected_tsv: string;
  selected_addresses: string[];
};

export type AppWarning = {
  code: string;
  message: string;
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
  images: AppSheetImage[];
  row_heights: Record<string, number>;
  column_widths: Record<string, number>;
  hidden_rows: string[];
  hidden_columns: string[];
  hidden: boolean;
  tab_color?: string;
  print_settings: AppSheetPrintSettings;
};

export type AppSheetPrintSettings = {
  print_area?: string;
  orientation: "portrait" | "landscape";
};

export type AppSheetAxis = {
  id: string;
  label: string;
};

export type AppSheetMerge = {
  id: string;
  range: string;
};

export type AppSheetImage = {
  id: string;
  blob_hash: string;
  start_column: string;
  start_row: string;
  start_offset_x_px: number;
  start_offset_y_px: number;
  end_column: string;
  end_row: string;
  end_offset_x_px: number;
  end_offset_y_px: number;
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
  display_value: string;
  dependencies: string[];
  comments: AppCellComment[];
  spill_source?: string;
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
  wrap_strategy: string | null;
  vertical_align: string | null;
  number_format: string | null;
};

export type AppDeletedRowPayload = {
  row_axis: AppSheetAxis;
  row_height?: number;
  cells: AppCell[];
  merges: AppSheetMerge[];
  filters: AppSheetFilter[];
  protected_ranges: AppSheetProtectedRange[];
  named_ranges: AppNamedRange[];
};

export type AppDeletedColumnPayload = {
  column_axis: AppSheetAxis;
  column_width?: number;
  cells: AppCell[];
  merges: AppSheetMerge[];
  filters: AppSheetFilter[];
  protected_ranges: AppSheetProtectedRange[];
  named_ranges: AppNamedRange[];
};
"#;
    format!("{types}{sizing}")
}

fn command_reference_markdown() -> String {
    let mut out = String::from(
        "## Generated Command Reference\n\n\
This section is generated from `opendoc-api` Rust command metadata. Do not edit it by hand.\n\n\
| Command | Returns | Args | Policy |\n\
| --- | --- | --- | --- |\n",
    );
    for command in COMMANDS {
        out.push_str(&format!(
            "| `{}` | `{}` | {} | {} |\n",
            command.name,
            command.returns.as_contract_type(),
            command_args_markdown(command.args),
            command_policy_markdown(command)
        ));
    }
    out.push('\n');
    out
}

fn command_args_markdown(args: &[opendoc_api::commands::CommandArg]) -> String {
    if args.is_empty() {
        return "none".to_string();
    }
    args.iter()
        .map(|arg| {
            let ty = if arg.optional {
                format!("{}?", arg.ty.as_contract_type())
            } else {
                arg.ty.as_contract_type().to_string()
            };
            format!("`{}: {}`", arg.name, ty)
        })
        .collect::<Vec<_>>()
        .join("<br>")
}

fn command_policy_markdown(command: &opendoc_api::commands::CommandSpec) -> String {
    let mut policies = Vec::new();
    if command.undoable {
        policies.push("undoable".to_string());
    }
    if command.allowed_without_open_document {
        policies.push("closed-state".to_string());
    }
    if let Some(action) = command.required_action {
        policies.push(format!("requires `{action}`"));
    } else {
        policies.push("no runtime action".to_string());
    }
    policies.join("<br>")
}

fn assert_generated_section_same(path: &PathBuf, expected: &str, label: &str) {
    let current = fs::read_to_string(path).expect("read generated-section target");
    let updated = replace_generated_section(&current, expected).expect("replace generated section");
    if updated != current {
        panic!("{label} is stale; run npm run generate:commands");
    }
}

fn update_generated_section(path: &PathBuf, expected: &str) -> std::io::Result<()> {
    let current = fs::read_to_string(path)?;
    let updated = replace_generated_section(&current, expected)
        .expect("generated command reference markers should exist");
    fs::write(path, updated)
}

fn replace_generated_section(current: &str, expected: &str) -> Option<String> {
    let begin = "<!-- BEGIN GENERATED COMMAND REFERENCE -->";
    let end = "<!-- END GENERATED COMMAND REFERENCE -->";
    let begin_at = current.find(begin)?;
    let content_at = begin_at + begin.len();
    let end_offset = current[content_at..].find(end)?;
    let end_at = content_at + end_offset;

    let mut updated = String::new();
    updated.push_str(&current[..content_at]);
    updated.push('\n');
    updated.push_str(expected);
    updated.push_str(end);
    updated.push_str(&current[end_at + end.len()..]);
    Some(updated)
}

fn args_type(args: &[opendoc_api::commands::CommandArg]) -> String {
    if args.is_empty() {
        return "Record<string, never>".to_string();
    }
    format!(
        "{{\n{}\n  }}",
        args.iter()
            .map(|arg| {
                format!(
                    "    {}{}: {};",
                    arg.name,
                    if arg.optional { "?" } else { "" },
                    ts_type(arg.ty)
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    )
}

fn ts_type(ty: CommandArgType) -> &'static str {
    match ty {
        CommandArgType::String => "string",
        CommandArgType::NullableString => "string | null",
        CommandArgType::Number => "number",
        CommandArgType::NullableNumber => "number | null",
        CommandArgType::Boolean => "boolean",
        CommandArgType::NullableBoolean => "boolean | null",
        CommandArgType::StringArray => "string[]",
        CommandArgType::NumberArray => "number[]",
        CommandArgType::Object => "Record<string, unknown>",
        CommandArgType::ObjectArray => "unknown[]",
        CommandArgType::RuntimeMode => "OpenDocRuntimeMode",
        CommandArgType::EditorSelection => "EditorSelection",
        CommandArgType::CitationItems => "AppCitationItem[]",
        CommandArgType::SpreadsheetCellEdit => "SpreadsheetCellEdit",
        CommandArgType::SpreadsheetCellEdits => "SpreadsheetCellEdit[]",
    }
}

fn union_type(names: &[&str]) -> String {
    if names.is_empty() {
        return "never".to_string();
    }
    names
        .iter()
        .map(|name| format!("  | \"{name}\""))
        .collect::<Vec<_>>()
        .join("\n")
}
