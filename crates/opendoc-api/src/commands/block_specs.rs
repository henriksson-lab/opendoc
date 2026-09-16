//! Block property, checklist and find/replace commands.

use crate::command_types::{CommandArg, CommandArgType, CommandReturn, CommandSpec};

pub(crate) const SPECS: &[CommandSpec] = &[
    // ---- Block properties (PLAN77 B3) -----------------------------------
    // One command per property rather than a generic (key, value) pair: the
    // value's type is the command's identity, so a payload cannot pair an
    // indent key with a line-spacing value. `opendoc_core::BlockProperty`
    // keeps the same invariant inside the model. Clearing needs no value, so
    // one keyed command covers every property there.
    command!(
        "set_block_alignment",
        AppDocument,
        [arg!("blockId", String), arg!("alignment", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_editor_selection_block_alignment",
        AppDocument,
        [
            arg!("selection", EditorSelection),
            arg!("alignment", String)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_block_indent_start",
        AppDocument,
        [arg!("blockId", String), arg!("twips", Number)],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_editor_selection_block_indent_start",
        AppDocument,
        [arg!("selection", EditorSelection), arg!("twips", Number)],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_block_indent_end",
        AppDocument,
        [arg!("blockId", String), arg!("twips", Number)],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_editor_selection_block_indent_end",
        AppDocument,
        [arg!("selection", EditorSelection), arg!("twips", Number)],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_block_indent_first_line",
        AppDocument,
        [arg!("blockId", String), arg!("twips", Number)],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_editor_selection_block_indent_first_line",
        AppDocument,
        [arg!("selection", EditorSelection), arg!("twips", Number)],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_block_line_spacing",
        AppDocument,
        [
            arg!("blockId", String),
            arg!("spacingMode", String),
            arg!("spacingValue", Number)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_editor_selection_block_line_spacing",
        AppDocument,
        [
            arg!("selection", EditorSelection),
            arg!("spacingMode", String),
            arg!("spacingValue", Number)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_block_space_before",
        AppDocument,
        [arg!("blockId", String), arg!("twips", Number)],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_editor_selection_block_space_before",
        AppDocument,
        [arg!("selection", EditorSelection), arg!("twips", Number)],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_block_space_after",
        AppDocument,
        [arg!("blockId", String), arg!("twips", Number)],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_editor_selection_block_space_after",
        AppDocument,
        [arg!("selection", EditorSelection), arg!("twips", Number)],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_block_direction",
        AppDocument,
        [arg!("blockId", String), arg!("direction", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_editor_selection_block_direction",
        AppDocument,
        [
            arg!("selection", EditorSelection),
            arg!("direction", String)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_block_keep_with_next",
        AppDocument,
        [arg!("blockId", String), arg!("keepWithNext", Boolean)],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_editor_selection_block_keep_with_next",
        AppDocument,
        [
            arg!("selection", EditorSelection),
            arg!("keepWithNext", Boolean)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_block_background",
        AppDocument,
        [arg!("blockId", String), arg!("color", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_editor_selection_block_background",
        AppDocument,
        [arg!("selection", EditorSelection), arg!("color", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_block_border",
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
    command!(
        "set_editor_selection_block_border",
        AppDocument,
        [
            arg!("selection", EditorSelection),
            arg!("style", String),
            arg!("twips", Number),
            arg!("color", String)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "clear_block_property",
        AppDocument,
        [arg!("blockId", String), arg!("key", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "clear_editor_selection_block_property",
        AppDocument,
        [arg!("selection", EditorSelection), arg!("key", String)],
        true,
        false,
        Some("write")
    ),
    // Checklist state. `set_block_text_style` and friends carry the marker
    // kind; the checkbox is toggled on its own so converting a list to
    // bullets and back cannot smuggle a stale checked state along.
    command!(
        "set_list_item_checked",
        AppDocument,
        [arg!("blockId", String), arg!("checked", Boolean)],
        true,
        false,
        Some("write")
    ),
    // Find and replace (parity ED-24/ED-25). Matching is document semantics,
    // so the query and its toggles are answered here and the shell only
    // renders the find bar. `find_in_document` reads; the two replace
    // commands are ordinary undoable edits, and replace-all is one dispatch
    // and therefore one undo step however many matches it rewrites.
    command!(
        "find_in_document",
        AppFindMatches,
        [
            arg!("query", String),
            arg!("matchCase", Boolean),
            arg!("wholeWord", Boolean),
            arg!("regex", Boolean)
        ],
        false,
        false,
        Some("read")
    ),
    command!(
        "replace_match_in_document",
        AppDocument,
        [
            arg!("query", String),
            arg!("matchCase", Boolean),
            arg!("wholeWord", Boolean),
            arg!("regex", Boolean),
            arg!("replacement", String),
            arg!("matchIndex", Number)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "replace_all_in_document",
        AppDocument,
        [
            arg!("query", String),
            arg!("matchCase", Boolean),
            arg!("wholeWord", Boolean),
            arg!("regex", Boolean),
            arg!("replacement", String)
        ],
        true,
        false,
        Some("write")
    ),
    // Toolbar indent/outdent: a list item changes level, anything else
    // shifts its start indent. Which one it is is a document question, so it
    // is decided in Rust and not by the caller.
    command!(
        "adjust_editor_selection_indent",
        AppDocument,
        [arg!("selection", EditorSelection), arg!("delta", Number)],
        true,
        false,
        Some("write")
    ),
];
