//! Version history, diffing, naming and restoring.

use crate::command_types::{CommandArg, CommandArgType, CommandReturn, CommandSpec};

pub(crate) const SPECS: &[CommandSpec] = &[
    command!(
        "list_document_versions",
        AppVersionView,
        [arg!("limit", Number, optional)],
        false,
        false,
        Some("read")
    ),
    command!(
        "open_document_at_version",
        AppVersionView,
        [arg!("manifest", String)],
        false,
        false,
        Some("read")
    ),
    command!(
        "diff_document_versions",
        AppVersionView,
        [arg!("fromManifest", String), arg!("toManifest", String)],
        false,
        false,
        Some("read")
    ),
    command!(
        "name_document_version",
        AppVersionView,
        [
            arg!("manifest", String),
            arg!("label", String),
            arg!("author", String)
        ],
        false,
        false,
        Some("write")
    ),
    command!(
        "restore_document_version",
        AppDocument,
        [arg!("manifest", String)],
        false,
        false,
        Some("write")
    ),
];
