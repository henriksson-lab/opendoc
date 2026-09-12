//! Comments and suggestions: adding them, then resolving them.

use crate::command_types::{CommandArg, CommandArgType, CommandReturn, CommandSpec};

pub(crate) const ADDITIONS: &[CommandSpec] = &[
    command!(
        "add_comment",
        AppDocument,
        [arg!("author", String), arg!("body", String)],
        true,
        false,
        Some("comment")
    ),
    command!(
        "add_text_range_comment",
        AppDocument,
        [
            arg!("startInlineId", String),
            arg!("endInlineId", String),
            arg!("author", String),
            arg!("body", String)
        ],
        true,
        false,
        Some("comment")
    ),
    command!(
        "add_block_comment",
        AppDocument,
        [
            arg!("blockId", String),
            arg!("author", String),
            arg!("body", String)
        ],
        true,
        false,
        Some("comment")
    ),
    command!(
        "add_comment_reply",
        AppDocument,
        [
            arg!("threadId", String),
            arg!("author", String),
            arg!("body", String)
        ],
        true,
        false,
        Some("comment")
    ),
    command!(
        "add_suggestion",
        AppDocument,
        [arg!("author", String), arg!("text", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "add_text_range_suggestion",
        AppDocument,
        [
            arg!("startInlineId", String),
            arg!("endInlineId", String),
            arg!("author", String),
            arg!("text", String)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "add_block_suggestion",
        AppDocument,
        [
            arg!("blockId", String),
            arg!("author", String),
            arg!("text", String)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "add_delete_suggestion",
        AppDocument,
        [arg!("author", String), arg!("inlineId", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "add_text_range_delete_suggestion",
        AppDocument,
        [
            arg!("startInlineId", String),
            arg!("endInlineId", String),
            arg!("author", String)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "add_format_suggestion",
        AppDocument,
        [
            arg!("author", String),
            arg!("inlineId", String),
            arg!("markKind", String),
            arg!("value", NullableString)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "add_text_range_format_suggestion",
        AppDocument,
        [
            arg!("startInlineId", String),
            arg!("endInlineId", String),
            arg!("author", String),
            arg!("markKind", String),
            arg!("value", NullableString)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "update_suggestion",
        AppDocument,
        [arg!("suggestionId", String), arg!("text", String)],
        true,
        false,
        Some("write")
    ),
];

pub(crate) const RESOLUTIONS: &[CommandSpec] = &[
    command!(
        "delete_comment_thread",
        AppDocument,
        [arg!("threadId", String)],
        true,
        false,
        Some("comment")
    ),
    command!(
        "restore_comment_thread",
        AppDocument,
        [arg!("threadId", String)],
        true,
        false,
        Some("comment")
    ),
    command!(
        "delete_comment",
        AppDocument,
        [arg!("threadId", String), arg!("commentId", String)],
        true,
        false,
        Some("comment")
    ),
    command!(
        "restore_comment",
        AppDocument,
        [arg!("threadId", String), arg!("commentId", String)],
        true,
        false,
        Some("comment")
    ),
    command!(
        "update_comment",
        AppDocument,
        [
            arg!("threadId", String),
            arg!("commentId", String),
            arg!("body", String)
        ],
        true,
        false,
        Some("comment")
    ),
    command!(
        "accept_suggestion",
        AppDocument,
        [arg!("suggestionId", String), arg!("acceptedBy", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "accept_all_suggestions",
        AppDocument,
        [arg!("acceptedBy", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "reject_suggestion",
        AppDocument,
        [arg!("suggestionId", String), arg!("rejectedBy", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "reject_all_suggestions",
        AppDocument,
        [arg!("rejectedBy", String)],
        true,
        false,
        Some("write")
    ),
];
