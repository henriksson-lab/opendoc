//! The gate that `npm run generate:commands -- --check` cannot be.
//!
//! `opendoc-api/src/bin/generate_command_contract.rs` emits the command surface
//! from Rust metadata, but it emits the **DTO** types as hand-written string
//! literals that mirror Rust structs — it has to, because `opendoc-api` cannot
//! depend on `opendoc-app`, where most of those structs live. `--check` then
//! compares the generator's output against the file the generator wrote, so it
//! is green by construction whenever a Rust struct and its TypeScript mirror
//! disagree: add a field to `AppRecoverySession` and it reaches every client
//! through serde, never appears in `src/generated/audit.ts`, and `--check`
//! notices nothing.
//!
//! This module is the missing half. For every mirrored type it derives the
//! real wire shape from the type's own serde impls (see [`crate::dto_shape`] —
//! no hand-written sample values, so the description cannot itself go stale),
//! reads the generated `.ts` back (see [`crate::dto_ts`]), and fails naming the
//! type and the field whenever the two disagree about
//!
//! * which fields exist,
//! * whether a field can be absent (`?`),
//! * whether a field can be `null`,
//! * which members a string-union enum has.
//!
//! It also refuses to let a new hand-written literal appear unchecked: every
//! `export type` in the generated DTO files must be in the registry below or in
//! [`UNMIRRORED`], with a reason.

use crate::dto_shape::{enum_variants, input_shape, wire_shape, WireShape};
use crate::dto_ts::{TsDecl, TsField, TsTypes};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

/// The generated files this module owns. `commands.ts` is included so a new
/// hand-written literal cannot hide there either.
const GENERATED_FILES: &[&str] = &[
    "audit.ts",
    "blob.ts",
    "citation.ts",
    "commands.ts",
    "document.ts",
    "editor.ts",
    "export.ts",
    "layout.ts",
    "runtime.ts",
    "spreadsheet.ts",
    "version.ts",
];

/// Declarations in those files that do not mirror a Rust type, and why.
///
/// A name may only sit here because there is nothing to compare it against —
/// never because comparing was inconvenient. The test fails on a stale entry,
/// so a type that gains a Rust mirror cannot stay exempt.
const UNMIRRORED: &[(&str, &str)] = &[
    (
        "AppCommandResult",
        "a union over the DTOs below, built by the generator from CommandReturn",
    ),
    (
        "DesktopCommandArgs",
        "built from the COMMANDS registry's arg metadata",
    ),
    (
        "DesktopCommandName",
        "keyof DesktopCommandArgs, built by the generator",
    ),
    (
        "CommandArgs",
        "a generic alias over DesktopCommandArgs, built by the generator",
    ),
    (
        "CommandResult",
        "a generic alias over the command-name unions, built by the generator",
    ),
    (
        "SpreadsheetCellEdit",
        "the shape `arg_spreadsheet_cell_edits` parses; parsed from serde_json::Value \
         by hand in opendoc-api, so there is no struct to mirror",
    ),
    (
        "AppLineSpacingPreset",
        "projects the mode()/value()/label() triple of opendoc_core::LineSpacing; \
         the values are generated from LineSpacing::PRESETS, not from a struct",
    ),
];

/// Unions of command *names*, all built by the generator's `union_type` from
/// the `COMMANDS` registry keyed by return type. They carry no Rust type of
/// their own and a new return category legitimately adds one.
fn is_generated_command_name_union(name: &str) -> bool {
    name.ends_with("CommandName")
}

/// How a TypeScript literal is judged against its Rust type.
#[derive(Clone, Copy, Eq, PartialEq)]
enum Rule {
    /// Rust serializes this type, so the literal must describe exactly what
    /// arrives: `?` iff Rust can omit the key, `| null` iff Rust can send null.
    /// Being stricter is as wrong as being laxer — a consumer that cannot
    /// compile against a value it will receive, or that guards against one it
    /// never will.
    Projection,
    /// Rust only *accepts* this type. The literal must not promise more
    /// latitude than serde grants — a `?` serde would reject, or a `null` it
    /// would refuse. It may be narrower: `EditorInput.data` is optional to
    /// serde and required by the type, because every caller has one and a
    /// type that asks for it catches the caller that forgot.
    Argument,
}

struct ObjectEntry {
    file: &'static str,
    ts: &'static str,
    rule: Rule,
    shape: fn() -> Result<WireShape, String>,
}

struct UnionEntry {
    file: &'static str,
    ts: &'static str,
    variants: fn() -> Result<Vec<String>, String>,
}

macro_rules! objects {
    ($($file:literal / $ts:literal => $rust:ty),* $(,)?) => {
        &[$(ObjectEntry {
            file: $file,
            ts: $ts,
            rule: Rule::Projection,
            shape: wire_shape::<$rust>,
        }),*]
    };
}

/// A DTO Rust never serializes, only accepts. Its TypeScript type is a
/// contract about what a client may *send*, so it is compared against what
/// serde deserializes rather than against what serde would emit.
macro_rules! arguments {
    ($($file:literal / $ts:literal => $rust:ty),* $(,)?) => {
        &[$(ObjectEntry {
            file: $file,
            ts: $ts,
            rule: Rule::Argument,
            shape: input_shape::<$rust>,
        }),*]
    };
}

macro_rules! unions {
    ($($file:literal / $ts:literal => $rust:ty),* $(,)?) => {
        &[$(UnionEntry {
            file: $file,
            ts: $ts,
            variants: enum_variants::<$rust>,
        }),*]
    };
}

/// Every hand-written object literal in the generated DTO files, paired with
/// the Rust type it claims to describe.
#[rustfmt::skip]
const OBJECTS: &[ObjectEntry] = objects![
    "audit.ts" / "AppAuditView" => crate::AppAuditView,
    "audit.ts" / "AppRepositoryTombstone" => crate::AppRepositoryTombstone,
    "audit.ts" / "AppCandidateHeadProblem" => crate::AppCandidateHeadProblem,
    "audit.ts" / "AppDeletedSheet" => crate::AppDeletedSheet,
    "audit.ts" / "AppDeletedNamedRange" => crate::AppDeletedNamedRange,
    "audit.ts" / "AppDeletedProtectedRange" => crate::AppDeletedProtectedRange,
    "audit.ts" / "AppDeletedBasicFilter" => crate::AppDeletedBasicFilter,
    "audit.ts" / "AppDeletedMerge" => crate::AppDeletedMerge,
    "audit.ts" / "AppDeletedCellValidation" => crate::AppDeletedCellValidation,
    "audit.ts" / "AppDeletedRow" => crate::AppDeletedRow,
    "audit.ts" / "AppDeletedColumn" => crate::AppDeletedColumn,
    "audit.ts" / "AppAuditBlobSignature" => crate::AppAuditBlobSignature,
    "audit.ts" / "AppArchiveTombstone" => crate::AppArchiveTombstone,
    "audit.ts" / "AppRecentDocument" => crate::AppRecentDocument,
    "audit.ts" / "AppWarning" => crate::AppWarning,
    "audit.ts" / "AppOperationRecord" => crate::AppOperationRecord,
    "audit.ts" / "AppRecoverySession" => crate::AppRecoverySession,
    "audit.ts" / "AppSignature" => crate::AppSignature,

    "blob.ts" / "AppBlobRef" => crate::AppBlobRef,
    "blob.ts" / "AppTypedContentSignature" => crate::AppTypedContentSignature,

    "citation.ts" / "AppCitationItem" => crate::AppCitationItem,

    "document.ts" / "AppDocument" => crate::AppDocument,
    "document.ts" / "AppListProperties" => opendoc_core::ListProperties,
    "document.ts" / "AppBookmark" => opendoc_core::Bookmark,
    "document.ts" / "AppCommentHistoryEntry" => opendoc_core::CommentHistoryEntry,
    "document.ts" / "AppCommentActivityEntry" => opendoc_core::CommentActivityEntry,
    "document.ts" / "AppBodyFragment" => crate::AppBodyFragment,
    "document.ts" / "AppPageSetup" => crate::AppPageSetup,
    "document.ts" / "AppPageLayout" => crate::AppPageLayout,
    "document.ts" / "AppPageSizePreset" => crate::AppPageSizePreset,
    "document.ts" / "AppBlock" => crate::AppBlock,
    "document.ts" / "AppPositionedImage" => opendoc_core::PositionedImage,
    "document.ts" / "AppTable" => crate::AppTable,
    "document.ts" / "AppTableColumn" => crate::AppTableColumn,
    "document.ts" / "AppTableCell" => crate::AppTableCell,
    "document.ts" / "AppTableCellProperties" => crate::AppTableCellProperties,
    "document.ts" / "AppCellBorder" => crate::table_dto::AppCellBorder,
    "document.ts" / "AppBlockProperties" => crate::AppBlockProperties,
    "document.ts" / "AppInline" => crate::AppInline,
    "document.ts" / "AppDropdownOption" => crate::inline_dto::AppDropdownOption,
    "document.ts" / "AppFootnote" => crate::AppFootnote,
    "document.ts" / "AppCitationDatabase" => crate::AppCitationDatabase,
    "document.ts" / "AppBibliographyEntry" => crate::AppBibliographyEntry,
    "document.ts" / "AppBibliographyReference" => crate::AppBibliographyReference,
    "document.ts" / "AppCitationGroup" => crate::AppCitationGroup,
    "document.ts" / "AppCommentThread" => crate::AppCommentThread,
    "document.ts" / "AppCommentThreadReaction" => crate::annotation_dto::AppCommentThreadReaction,
    "document.ts" / "AppComment" => crate::AppComment,
    "document.ts" / "AppSuggestion" => crate::AppSuggestion,
    "document.ts" / "EditorResult" => crate::EditorResult,

    "editor.ts" / "EditorPosition" => crate::EditorPosition,
    "editor.ts" / "EditorSelection" => crate::EditorSelection,
    "editor.ts" / "EditorInlineRange" => crate::EditorInlineRange,
    "editor.ts" / "AppEditorSelection" => crate::AppEditorSelection,
    "editor.ts" / "AppFindMatch" => crate::AppFindMatch,
    "editor.ts" / "AppFindMatches" => crate::AppFindMatches,

    "export.ts" / "AppExport" => crate::AppExport,

    "layout.ts" / "AppBlockPlacement" => crate::AppBlockPlacement,
    "layout.ts" / "AppDocumentLayout" => crate::AppDocumentLayout,

    "runtime.ts" / "OpenDocRuntimeProfile" => crate::OpenDocRuntimeProfile,
    "runtime.ts" / "OpenDocServiceSession" => crate::OpenDocServiceSession,
    "runtime.ts" / "OpenDocRuntimeSession" => crate::OpenDocRuntimeSession,
    "runtime.ts" / "OpenDocPresencePeer" => crate::OpenDocPresencePeer,
    "runtime.ts" / "OpenDocAuthorizationDecision" => crate::OpenDocAuthorizationDecision,
    "runtime.ts" / "OpenDocShareInvite" => crate::OpenDocShareInvite,
    "runtime.ts" / "OpenDocRelayOperation" => crate::OpenDocRelayOperation,
    "runtime.ts" / "OpenDocSyncRelayResult" => crate::OpenDocSyncRelayResult,
    "runtime.ts" / "OpenDocRuntimeLookupEntry" => crate::OpenDocRuntimeLookupEntry,
    "runtime.ts" / "OpenDocRuntimeLookupResult" => crate::OpenDocRuntimeLookupResult,

    "spreadsheet.ts" / "AppSpreadsheetWorkbook" => crate::AppSpreadsheetWorkbook,
    "spreadsheet.ts" / "AppSpreadsheetEvaluationContext" => crate::AppSpreadsheetEvaluationContext,
    "spreadsheet.ts" / "AppSpreadsheetSelection" => crate::AppSpreadsheetSelection,
    "spreadsheet.ts" / "AppWarning" => crate::AppWarning,
    "spreadsheet.ts" / "AppNamedRange" => crate::AppNamedRange,
    "spreadsheet.ts" / "AppCellDependency" => crate::AppCellDependency,
    "spreadsheet.ts" / "AppSheet" => crate::AppSheet,
    "spreadsheet.ts" / "AppSheetPrintSettings" => opendoc_spreadsheet::SheetPrintSettings,
    "spreadsheet.ts" / "AppSheetAxis" => crate::AppSheetAxis,
    "spreadsheet.ts" / "AppSheetImage" => opendoc_spreadsheet::SheetImage,
    "spreadsheet.ts" / "AppSheetMerge" => crate::AppSheetMerge,
    "spreadsheet.ts" / "AppSheetFilter" => crate::AppSheetFilter,
    "spreadsheet.ts" / "AppSheetFilterCriterion" => crate::AppSheetFilterCriterion,
    "spreadsheet.ts" / "AppSheetFilterSortSpec" => crate::AppSheetFilterSortSpec,
    "spreadsheet.ts" / "AppSheetProtectedRange" => crate::AppSheetProtectedRange,
    "spreadsheet.ts" / "AppCell" => crate::AppCell,
    "spreadsheet.ts" / "AppCellValidation" => crate::AppCellValidation,
    "spreadsheet.ts" / "AppCellComment" => crate::AppCellComment,
    "spreadsheet.ts" / "AppDeletedCellComment" => crate::AppDeletedCellComment,
    "spreadsheet.ts" / "AppCellFormat" => crate::AppCellFormat,
    "spreadsheet.ts" / "AppDeletedRowPayload" => crate::AppDeletedRowPayload,
    "spreadsheet.ts" / "AppDeletedColumnPayload" => crate::AppDeletedColumnPayload,

    "version.ts" / "AppVersionSigner" => crate::AppVersionSigner,
    "version.ts" / "AppDocumentVersion" => crate::AppDocumentVersion,
    "version.ts" / "AppVersionPreview" => crate::AppVersionPreview,
    "version.ts" / "AppVersionPreviewBlock" => crate::version::AppVersionPreviewBlock,
    "version.ts" / "AppVersionDiffEntry" => crate::AppVersionDiffEntry,
    "version.ts" / "AppVersionDiff" => crate::AppVersionDiff,
    "version.ts" / "AppVersionView" => crate::AppVersionView,
];

/// The one DTO in the generated files that travels only towards Rust.
///
/// Everything in [`OBJECTS`] is something Rust serializes, so its TypeScript
/// type is judged by what arrives. `EditorInput` is built by `editor.ts` and
/// never projected, so the only thing its type can be judged by is what serde
/// accepts.
#[rustfmt::skip]
const ARGUMENTS: &[ObjectEntry] = arguments![
    "editor.ts" / "EditorInput" => crate::EditorInput,
];

/// Every hand-written string-union literal in the generated DTO files.
#[rustfmt::skip]
const UNIONS: &[UnionEntry] = unions![
    "editor.ts" / "AppFindRegion" => opendoc_api::AppFindRegion,
    "export.ts" / "AppExportEncoding" => crate::AppExportEncoding,
    "runtime.ts" / "OpenDocRuntimeMode" => crate::OpenDocRuntimeMode,
    "runtime.ts" / "OpenDocStorageBackend" => crate::OpenDocStorageBackend,
    "runtime.ts" / "OpenDocPermissionAuthority" => crate::OpenDocPermissionAuthority,
    "runtime.ts" / "OpenDocServiceRole" => crate::OpenDocServiceRole,
    "runtime.ts" / "OpenDocAuthorizationSource" => crate::OpenDocAuthorizationSource,
    "runtime.ts" / "OpenDocSyncBatchOutcome" => crate::OpenDocSyncBatchOutcome,
];

fn generated_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("opendoc-app should live under crates/")
        .join("apps/desktop/src/generated")
}

fn parsed_files() -> BTreeMap<&'static str, TsTypes> {
    let dir = generated_dir();
    GENERATED_FILES
        .iter()
        .map(|file| {
            let path = dir.join(file);
            let source = std::fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
            (*file, TsTypes::parse(&source))
        })
        .collect()
}

/// The contract's DTO literals describe what Rust actually serializes.
#[test]
fn generated_dto_types_match_the_rust_types_they_mirror() {
    let files = parsed_files();
    let mut drift: Vec<String> = Vec::new();

    for entry in OBJECTS.iter().chain(ARGUMENTS) {
        let where_ = format!("{}: {}", entry.file, entry.ts);
        let Some(decl) = files[entry.file].decls.get(entry.ts) else {
            drift.push(format!(
                "{where_} — declared in Rust but absent from the generated file"
            ));
            continue;
        };
        let TsDecl::Object(ts_fields) = decl else {
            drift.push(format!(
                "{where_} — is not an object literal in the generated file"
            ));
            continue;
        };
        let shape = match (entry.shape)() {
            Ok(shape) => shape,
            Err(error) => {
                drift.push(format!(
                    "{where_} — could not read the Rust wire shape: {error}"
                ));
                continue;
            }
        };
        for anomaly in &shape.anomalies {
            drift.push(format!("{where_} — {anomaly}"));
        }
        drift.extend(
            field_drift(&shape, ts_fields, entry.rule)
                .into_iter()
                .map(|line| format!("{where_} — {line}")),
        );
    }

    for entry in UNIONS {
        let where_ = format!("{}: {}", entry.file, entry.ts);
        let Some(decl) = files[entry.file].decls.get(entry.ts) else {
            drift.push(format!(
                "{where_} — declared in Rust but absent from the generated file"
            ));
            continue;
        };
        let TsDecl::StringUnion(members) = decl else {
            drift.push(format!(
                "{where_} — is not a string-literal union in the generated file"
            ));
            continue;
        };
        let variants = match (entry.variants)() {
            Ok(variants) => variants,
            Err(error) => {
                drift.push(format!(
                    "{where_} — could not read the Rust variants: {error}"
                ));
                continue;
            }
        };
        let rust: BTreeSet<&String> = variants.iter().collect();
        let ts: BTreeSet<&String> = members.iter().collect();
        for missing in rust.difference(&ts) {
            drift.push(format!(
                "{where_} — Rust has variant \"{missing}\", the TypeScript union does not"
            ));
        }
        for extra in ts.difference(&rust) {
            drift.push(format!(
                "{where_} — the TypeScript union has member \"{extra}\", Rust has no such variant"
            ));
        }
    }

    assert!(
        drift.is_empty(),
        "the generated TypeScript DTOs no longer describe what Rust serializes.\n\
         These literals are hand-written in \
         crates/opendoc-api/src/bin/generate_command_contract.rs and \
         `generate:commands --check` cannot see this class of drift.\n{}",
        drift.join("\n")
    );
}

/// No hand-written literal may appear in the generated files without a check.
#[test]
fn every_generated_declaration_is_either_mirrored_or_explained() {
    let files = parsed_files();
    let checked: BTreeSet<&str> = OBJECTS
        .iter()
        .chain(ARGUMENTS)
        .map(|entry| entry.ts)
        .chain(UNIONS.iter().map(|entry| entry.ts))
        .collect();
    let exempt: BTreeSet<&str> = UNMIRRORED.iter().map(|(name, _)| *name).collect();

    let mut unaccounted = Vec::new();
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for (file, types) in &files {
        for name in types.decls.keys() {
            seen.insert(name.as_str());
            if checked.contains(name.as_str())
                || exempt.contains(name.as_str())
                || is_generated_command_name_union(name)
            {
                continue;
            }
            unaccounted.push(format!(
                "{file}: {name} — add it to OBJECTS/UNIONS in dto_contract_tests.rs, \
                 or to UNMIRRORED with the reason it has no Rust type"
            ));
        }
    }
    assert!(
        unaccounted.is_empty(),
        "generated DTO declarations with nothing checking them:\n{}",
        unaccounted.join("\n")
    );

    let stale: Vec<&str> = UNMIRRORED
        .iter()
        .map(|(name, _)| *name)
        .filter(|name| !seen.contains(name))
        .collect();
    assert!(
        stale.is_empty(),
        "UNMIRRORED names that no longer appear in the generated files: {stale:?}"
    );
    let double_counted: Vec<&str> = UNMIRRORED
        .iter()
        .map(|(name, _)| *name)
        .filter(|name| checked.contains(name))
        .collect();
    assert!(
        double_counted.is_empty(),
        "UNMIRRORED names that are in fact checked: {double_counted:?}"
    );
}

/// Proof that the comparison above can fail, and fails by name.
///
/// [`generated_dto_types_match_the_rust_types_they_mirror`] is only worth
/// anything if [`field_drift`] reports drift rather than shrugging at it, so
/// this drives that same function — not a second copy of the rules — over a
/// literal doctored in the five ways a real edit shows up: a field dropped, a
/// field invented, a field renamed, a `| null` lost, and a `?` gained. Each
/// has to come back naming the field it is about; "something changed" would
/// pass an assertion and help nobody.
#[test]
fn the_comparison_reports_each_kind_of_drift_by_name() {
    let compare = |shape: &WireShape, source: &str| -> Vec<String> {
        let types = TsTypes::parse(source);
        match types.decls.values().next() {
            Some(TsDecl::Object(fields)) => field_drift(shape, fields, Rule::Projection),
            _ => vec!["the doctored literal did not parse as an object".to_string()],
        }
    };
    let shape = wire_shape::<crate::AppRecoverySession>().expect("wire shape");
    let honest = r#"
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
"#;
    assert!(
        compare(&shape, honest).is_empty(),
        "the honest literal should not drift: {:?}",
        compare(&shape, honest)
    );

    let removed = honest.replace("  base_manifest: string | null;\n", "");
    let report = compare(&shape, &removed).join("\n");
    assert!(
        report.contains("Rust serializes field `base_manifest`"),
        "a dropped field must be named: {report}"
    );

    let added = honest.replace(
        "  truncated: boolean;",
        "  truncated: boolean;\n  invented: string;",
    );
    let report = compare(&shape, &added).join("\n");
    assert!(
        report.contains("declares field `invented`"),
        "an invented field must be named: {report}"
    );

    let renamed = honest.replace("  title: string;", "  heading: string;");
    let report = compare(&shape, &renamed).join("\n");
    assert!(
        report.contains("Rust serializes field `title`") && report.contains("field `heading`"),
        "a rename must name both sides: {report}"
    );

    let unwrapped = honest.replace(
        "  repository_root: string | null;",
        "  repository_root: string;",
    );
    let report = compare(&shape, &unwrapped).join("\n");
    assert!(
        report.contains("field `repository_root`") && report.contains("as null"),
        "a lost `| null` must be named: {report}"
    );

    let made_optional = honest.replace("  truncated: boolean;", "  truncated?: boolean;");
    let report = compare(&shape, &made_optional).join("\n");
    assert!(
        report.contains("field `truncated`") && report.contains("omit the key"),
        "a spurious `?` must be named: {report}"
    );
}

/// The one rule set both [`generated_dto_types_match_the_rust_types_they_mirror`]
/// and [`the_comparison_reports_each_kind_of_drift_by_name`] apply, so the
/// mutation proof exercises exactly the comparison that guards the contract
/// rather than a second copy of it that could drift from it.
fn field_drift(
    shape: &WireShape,
    ts_fields: &BTreeMap<String, TsField>,
    rule: Rule,
) -> Vec<String> {
    let mut drift = Vec::new();
    let rust_names: BTreeSet<&String> = shape.fields.keys().collect();
    let ts_names: BTreeSet<&String> = ts_fields.keys().collect();
    for missing in rust_names.difference(&ts_names) {
        drift.push(format!(
            "Rust serializes field `{missing}`, the TypeScript literal has no such field"
        ));
    }
    for extra in ts_names.difference(&rust_names) {
        drift.push(format!(
            "the TypeScript literal declares field `{extra}`, Rust never serializes it"
        ));
    }
    for (name, rust) in &shape.fields {
        let Some(ts) = ts_fields.get(name) else {
            continue;
        };
        let optional_drift = match rule {
            Rule::Projection => ts.optional != rust.omissible,
            Rule::Argument => ts.optional && !rust.omissible,
        };
        if optional_drift {
            drift.push(match rule {
                Rule::Projection => format!(
                    "field `{name}`: Rust {} omit the key, TypeScript says `{name}{}: …`",
                    if rust.omissible { "can" } else { "never does" },
                    if ts.optional { "?" } else { "" },
                ),
                Rule::Argument => format!(
                    "field `{name}`: TypeScript says `{name}?: …`, but serde rejects \
                     the key being absent"
                ),
            });
        }
        let nullable_drift = match rule {
            Rule::Projection => ts.nullable != rust.nullable,
            Rule::Argument => ts.nullable && !rust.nullable,
        };
        if nullable_drift {
            drift.push(match rule {
                Rule::Projection => format!(
                    "field `{name}`: Rust {} serialize it as null, TypeScript type is `{}`",
                    if rust.nullable { "can" } else { "never does" },
                    ts.ty,
                ),
                Rule::Argument => format!(
                    "field `{name}`: TypeScript type is `{}`, but serde rejects null",
                    ts.ty
                ),
            });
        }
    }
    drift
}
