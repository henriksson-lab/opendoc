//! Editor selection, input, and the projection commands.

use crate::command_types::{CommandArg, CommandArgType, CommandReturn, CommandSpec};

pub(crate) const SPECS: &[CommandSpec] = &[
    command!(
        "describe_editor_selection",
        AppEditorSelection,
        [arg!("selection", EditorSelection)],
        false,
        false,
        Some("read")
    ),
    command!(
        "select_all_editor_content",
        EditorResult,
        [],
        false,
        false,
        Some("read")
    ),
    command!(
        "apply_editor_input",
        EditorResult,
        [
            arg!("selection", EditorSelection),
            arg!("input_type", String),
            arg!("data", NullableString),
            arg!("html", NullableString)
        ],
        true,
        false,
        None
    ),
    command!("render_document_html", String, [], false, false, None),
    command!(
        "render_suggestion_preview_html",
        String,
        [arg!("suggestionId", String), arg!("resolution", String)],
        false,
        false,
        Some("read")
    ),
    command!(
        "apply_editor_mark",
        EditorResult,
        [
            arg!("selection", EditorSelection),
            arg!("mark_kind", String),
            arg!("value", NullableString),
            arg!("action", String, optional)
        ],
        true,
        false,
        None
    ),
    command!(
        "render_workbook_html",
        String,
        [arg!("sheetId", String)],
        false,
        false,
        None
    ),
    command!(
        "import_docx_base64",
        AppDocument,
        [arg!("name", String), arg!("base64", String)],
        true,
        false,
        None
    ),
];
