//! Headings, links, mentions, footnotes, equations and lists.

use crate::command_types::{CommandArg, CommandArgType, CommandReturn, CommandSpec};

pub(crate) const SPECS: &[CommandSpec] = &[
    command!(
        "add_heading",
        AppDocument,
        [arg!("text", String), arg!("level", Number)],
        true,
        false,
        Some("write")
    ),
    command!(
        "update_heading_level",
        AppDocument,
        [arg!("blockId", String), arg!("level", Number)],
        true,
        false,
        Some("write")
    ),
    command!(
        "add_link",
        AppDocument,
        [arg!("text", String), arg!("href", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "add_mention",
        AppDocument,
        [arg!("label", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "insert_mention_after",
        AppDocument,
        [
            arg!("blockId", String),
            arg!("afterInlineId", NullableString),
            arg!("label", String)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "insert_date_chip_after",
        AppDocument,
        [
            arg!("blockId", String),
            arg!("afterInlineId", NullableString),
            arg!("date", String)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "update_mention_label",
        AppDocument,
        [arg!("inlineId", String), arg!("label", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "select_dropdown_option",
        AppDocument,
        [arg!("inlineId", String), arg!("optionId", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "update_date_chip",
        AppDocument,
        [arg!("inlineId", String), arg!("date", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "add_footnote_ref",
        AppDocument,
        [],
        true,
        false,
        Some("write")
    ),
    command!(
        "add_endnote_ref",
        AppDocument,
        [],
        true,
        false,
        Some("write")
    ),
    command!(
        "insert_footnote_ref_after",
        AppDocument,
        [
            arg!("blockId", String),
            arg!("afterInlineId", NullableString)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "insert_endnote_ref_after",
        AppDocument,
        [
            arg!("blockId", String),
            arg!("afterInlineId", NullableString)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "update_footnote_body",
        AppDocument,
        [arg!("footnoteId", String), arg!("body", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "add_equation",
        AppDocument,
        [arg!("source", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "insert_equation_after",
        AppDocument,
        [
            arg!("blockId", String),
            arg!("afterInlineId", NullableString),
            arg!("source", String)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "add_equation_block",
        AppDocument,
        [arg!("source", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "insert_equation_block_after",
        AppDocument,
        [arg!("afterBlockId", String), arg!("source", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "add_list_item",
        AppDocument,
        [
            arg!("text", String),
            arg!("level", Number),
            arg!("listKind", String)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "insert_list_item_after",
        AppDocument,
        [
            arg!("afterBlockId", String),
            arg!("text", String),
            arg!("level", Number),
            arg!("listKind", String)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "update_list_item",
        AppDocument,
        [
            arg!("blockId", String),
            arg!("level", Number),
            arg!("listKind", String)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_ordered_list_start",
        AppDocument,
        [arg!("blockId", String), arg!("start", Number)],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_ordered_list_format",
        AppDocument,
        [arg!("blockId", String), arg!("format", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_bullet_list_marker",
        AppDocument,
        [arg!("blockId", String), arg!("marker", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "adjust_editor_selection_list_indent",
        AppDocument,
        [arg!("selection", EditorSelection), arg!("delta", Number)],
        true,
        false,
        Some("write")
    ),
];

pub(crate) const EDITS: &[CommandSpec] = &[
    command!(
        "update_inline_text",
        AppDocument,
        [arg!("inlineId", String), arg!("text", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "update_inline_equation_source",
        AppDocument,
        [arg!("inlineId", String), arg!("source", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "update_link_href",
        AppDocument,
        [arg!("inlineId", String), arg!("href", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "insert_inline_text",
        AppDocument,
        [
            arg!("blockId", String),
            arg!("afterInlineId", NullableString),
            arg!("text", String)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "insert_link_after",
        AppDocument,
        [
            arg!("blockId", String),
            arg!("afterInlineId", NullableString),
            arg!("text", String),
            arg!("href", String)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "delete_inline",
        AppDocument,
        [arg!("inlineId", String)],
        true,
        false,
        Some("write")
    ),
];
