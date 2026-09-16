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
            arg!("signingEnabled", NullableBoolean)
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
            arg!("commandName", String)
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
            arg!("targetSubject", NullableString),
            arg!("role", NullableString)
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
            arg!("operations", ObjectArray)
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
            arg!("documentUuid", NullableString),
            arg!("doi", NullableString),
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
