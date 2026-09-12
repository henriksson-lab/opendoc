//! Session, runtime profile and undo/redo commands.

use crate::command_types::{CommandArg, CommandArgType, CommandReturn, CommandSpec};

pub(crate) const SPECS: &[CommandSpec] = &[
    command!(
        "create_document",
        AppDocument,
        [arg!("title", String)],
        false,
        true,
        Some("write")
    ),
    command!(
        "close_document",
        AppDocument,
        [],
        false,
        true,
        Some("write")
    ),
    command!("get_document", AppDocument, [], false, true, Some("read")),
    command!(
        "get_audit_view",
        AppAuditView,
        [],
        false,
        true,
        Some("read")
    ),
    command!(
        "get_runtime_profile",
        OpenDocRuntimeProfile,
        [
            arg!("mode", RuntimeMode),
            arg!("storageBackends", ObjectArray),
            arg!("signingEnabled", NullableBoolean)
        ],
        false,
        true,
        Some("read")
    ),
    command!(
        "get_runtime_session",
        OpenDocRuntimeSession,
        [
            arg!("mode", RuntimeMode),
            arg!("storageBackends", ObjectArray),
            arg!("signingEnabled", NullableBoolean),
            arg!("subject", NullableString),
            arg!("documentUuid", NullableString),
            arg!("presence", ObjectArray),
            arg!("permissions", ObjectArray)
        ],
        false,
        true,
        Some("read")
    ),
    command!(
        "authorize_runtime_command",
        OpenDocAuthorizationDecision,
        [
            arg!("mode", RuntimeMode),
            arg!("storageBackends", ObjectArray),
            arg!("signingEnabled", NullableBoolean),
            arg!("subject", NullableString),
            arg!("documentUuid", NullableString),
            arg!("commandName", String),
            arg!("permissions", ObjectArray)
        ],
        false,
        true,
        Some("read")
    ),
    command!(
        "create_runtime_share_invite",
        OpenDocShareInvite,
        [
            arg!("mode", RuntimeMode),
            arg!("storageBackends", ObjectArray),
            arg!("signingEnabled", NullableBoolean),
            arg!("subject", NullableString),
            arg!("documentUuid", NullableString),
            arg!("targetSubject", NullableString),
            arg!("actions", StringArray),
            arg!("permissions", ObjectArray)
        ],
        false,
        true,
        Some("share")
    ),
    command!(
        "relay_runtime_sync",
        OpenDocSyncRelayResult,
        [
            arg!("mode", RuntimeMode),
            arg!("storageBackends", ObjectArray),
            arg!("signingEnabled", NullableBoolean),
            arg!("subject", NullableString),
            arg!("documentUuid", NullableString),
            arg!("baseManifest", NullableString),
            arg!("operations", ObjectArray),
            arg!("permissions", ObjectArray),
            arg!("presence", ObjectArray)
        ],
        false,
        true,
        Some("write")
    ),
    command!(
        "resolve_runtime_document_lookup",
        OpenDocRuntimeLookupResult,
        [
            arg!("mode", RuntimeMode),
            arg!("storageBackends", ObjectArray),
            arg!("signingEnabled", NullableBoolean),
            arg!("subject", NullableString),
            arg!("documentUuid", NullableString),
            arg!("doi", NullableString),
            arg!("permissions", ObjectArray),
            arg!("serviceIndex", ObjectArray),
            arg!("scannedDocuments", ObjectArray)
        ],
        false,
        true,
        Some("read")
    ),
    command!(
        "undo_current_edit",
        AppDocument,
        [],
        false,
        false,
        Some("write")
    ),
    command!(
        "redo_current_edit",
        AppDocument,
        [],
        false,
        false,
        Some("write")
    ),
];
