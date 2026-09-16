//! Text marks, block equations and image block geometry.

use crate::command_types::{CommandArg, CommandArgType, CommandReturn, CommandSpec};

pub(crate) const SPECS: &[CommandSpec] = &[
    command!(
        "add_text_mark",
        AppDocument,
        [
            arg!("inlineId", String),
            arg!("markKind", String),
            arg!("value", NullableString)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "add_text_mark_range",
        AppDocument,
        [
            arg!("startInlineId", String),
            arg!("endInlineId", String),
            arg!("markKind", String),
            arg!("value", NullableString)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "remove_text_mark",
        AppDocument,
        [
            arg!("inlineId", String),
            arg!("markKind", String),
            arg!("value", NullableString)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "remove_text_mark_range",
        AppDocument,
        [
            arg!("startInlineId", String),
            arg!("endInlineId", String),
            arg!("markKind", String),
            arg!("value", NullableString)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "update_block_equation_source",
        AppDocument,
        [arg!("blockId", String), arg!("source", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "update_image_alt_text",
        AppDocument,
        [arg!("blockId", String), arg!("altText", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "update_image_blob_hash",
        AppDocument,
        [arg!("blockId", String), arg!("blobHash", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_image_block_width",
        AppDocument,
        [arg!("blockId", String), arg!("twips", Number)],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_image_block_height",
        AppDocument,
        [arg!("blockId", String), arg!("twips", Number)],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_image_block_size",
        AppDocument,
        [
            arg!("blockId", String),
            arg!("widthTwips", Number),
            arg!("heightTwips", Number)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "clear_image_block_size",
        AppDocument,
        [arg!("blockId", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_image_block_placement",
        AppDocument,
        [arg!("blockId", String), arg!("placement", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_image_block_wrap_clearance",
        AppDocument,
        [
            arg!("blockId", String),
            arg!("topTwips", Number),
            arg!("endTwips", Number),
            arg!("bottomTwips", Number),
            arg!("startTwips", Number)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_image_block_positioned",
        AppDocument,
        [
            arg!("blockId", String),
            arg!("anchorBlockId", NullableString, optional),
            arg!("horizontalOffsetTwips", Number),
            arg!("verticalOffsetTwips", Number),
            arg!("layer", String)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "clear_image_block_positioned",
        AppDocument,
        [arg!("blockId", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_image_block_effects",
        AppDocument,
        [
            arg!("blockId", String),
            arg!("rotationDegrees", Number),
            arg!("opacityPercent", Number)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_image_block_crop",
        AppDocument,
        [
            arg!("blockId", String),
            arg!("topPercent", Number),
            arg!("rightPercent", Number),
            arg!("bottomPercent", Number),
            arg!("leftPercent", Number)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_image_block_caption",
        AppDocument,
        [arg!("blockId", String), arg!("caption", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_image_block_border",
        AppDocument,
        [
            arg!("blockId", String),
            arg!("style", String),
            arg!("twips", Number),
            arg!("color", String)
        ],
        true,
        false,
        Some("write")
    ),
];
