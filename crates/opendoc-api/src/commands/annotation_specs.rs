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
        "add_block_delete_suggestion",
        AppDocument,
        [arg!("blockId", String), arg!("author", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "add_block_insert_suggestion",
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
        "add_block_replace_suggestion",
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
        "add_text_range_format_removal_suggestion",
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
        "add_text_range_format_replacement_suggestion",
        AppDocument,
        [
            arg!("startInlineId", String),
            arg!("endInlineId", String),
            arg!("author", String),
            arg!("markKind", String),
            arg!("expectedValue", String),
            arg!("value", String)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "add_link_change_suggestion",
        AppDocument,
        [
            arg!("inlineId", String),
            arg!("author", String),
            arg!("href", NullableString)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "add_paragraph_style_suggestion",
        AppDocument,
        [
            arg!("blockId", String),
            arg!("author", String),
            arg!("style", String)
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
        "resolve_comment_thread",
        AppDocument,
        [arg!("threadId", String), arg!("resolvedBy", String)],
        true,
        false,
        Some("comment")
    ),
    command!(
        "reopen_comment_thread",
        AppDocument,
        [arg!("threadId", String)],
        true,
        false,
        Some("comment")
    ),
    command!(
        "set_comment_thread_action",
        AppDocument,
        [
            arg!("threadId", String),
            arg!("assignee", NullableString),
            arg!("dueAtMs", NullableNumber),
            arg!("completed", Boolean),
            arg!("completedBy", NullableString)
        ],
        true,
        false,
        Some("comment")
    ),
    command!(
        "set_comment_thread_reaction",
        AppDocument,
        [
            arg!("threadId", String),
            arg!("emoji", String),
            arg!("actor", String),
            arg!("present", Boolean)
        ],
        true,
        false,
        Some("comment")
    ),
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
