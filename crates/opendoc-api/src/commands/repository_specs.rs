//! Saving, opening, scanning and reconciling repositories.

use crate::command_types::{CommandArg, CommandArgType, CommandReturn, CommandSpec};

pub(crate) const SPECS: &[CommandSpec] = &[
    command!(
        "save_local_repository",
        AppDocument,
        [arg!("path", String)],
        false,
        false,
        Some("write")
    ),
    command!(
        "save_local_repository_or_candidate",
        AppDocument,
        [arg!("path", String)],
        false,
        false,
        Some("write")
    ),
    command!(
        "save_flat_repository",
        AppDocument,
        [arg!("path", String), arg!("namespace", String)],
        false,
        false,
        Some("write")
    ),
    command!(
        "save_flat_repository_or_candidate",
        AppDocument,
        [arg!("path", String), arg!("namespace", String)],
        false,
        false,
        Some("write")
    ),
    command!(
        "save_opendal_fs_repository",
        AppDocument,
        [arg!("path", String), arg!("namespace", String)],
        false,
        false,
        Some("write")
    ),
    command!(
        "save_opendal_fs_repository_or_candidate",
        AppDocument,
        [arg!("path", String), arg!("namespace", String)],
        false,
        false,
        Some("write")
    ),
    command!(
        "autosave_current_repository",
        AppDocument,
        [],
        false,
        false,
        Some("write")
    ),
    command!(
        "open_local_repository",
        AppDocument,
        [arg!("path", String), arg!("documentUuid", String)],
        false,
        true,
        Some("read")
    ),
    command!(
        "recover_session",
        AppDocument,
        [arg!("sessionId", String)],
        false,
        true,
        Some("write")
    ),
    command!(
        "discard_recovery_session",
        AppDocument,
        [arg!("sessionId", String)],
        false,
        true,
        Some("write")
    ),
    command!(
        "scan_local_repository",
        AppDocument,
        [arg!("path", String)],
        false,
        false,
        None
    ),
    command!(
        "open_flat_repository",
        AppDocument,
        [
            arg!("path", String),
            arg!("namespace", String),
            arg!("documentUuid", String)
        ],
        false,
        true,
        Some("read")
    ),
    command!(
        "open_opendal_fs_repository",
        AppDocument,
        [
            arg!("path", String),
            arg!("namespace", String),
            arg!("documentUuid", String)
        ],
        false,
        true,
        Some("read")
    ),
    command!(
        "merge_local_repository_candidates",
        AppDocument,
        [arg!("path", String), arg!("documentUuid", String)],
        false,
        true,
        Some("write")
    ),
    command!(
        "compact_local_repository",
        AppDocument,
        [arg!("path", String), arg!("packName", String)],
        false,
        true,
        Some("write")
    ),
    command!(
        "merge_flat_repository_candidates",
        AppDocument,
        [
            arg!("path", String),
            arg!("namespace", String),
            arg!("documentUuid", String)
        ],
        false,
        true,
        Some("write")
    ),
    command!(
        "merge_opendal_fs_repository_candidates",
        AppDocument,
        [
            arg!("path", String),
            arg!("namespace", String),
            arg!("documentUuid", String)
        ],
        false,
        true,
        Some("write")
    ),
    command!(
        "open_local_repository_by_doi",
        AppDocument,
        [arg!("path", String), arg!("doi", String)],
        false,
        true,
        Some("read")
    ),
    command!(
        "open_flat_repository_by_doi",
        AppDocument,
        [
            arg!("path", String),
            arg!("namespace", String),
            arg!("doi", String)
        ],
        false,
        true,
        Some("read")
    ),
    command!(
        "open_opendal_fs_repository_by_doi",
        AppDocument,
        [
            arg!("path", String),
            arg!("namespace", String),
            arg!("doi", String)
        ],
        false,
        true,
        Some("read")
    ),
];
