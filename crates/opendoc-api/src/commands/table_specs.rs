//! Page breaks and the table grid commands.

use crate::command_types::{CommandArg, CommandArgType, CommandReturn, CommandSpec};

pub(crate) const SPECS: &[CommandSpec] = &[
    command!(
        "insert_page_break_after",
        AppDocument,
        [arg!("afterBlockId", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "add_page_break",
        AppDocument,
        [],
        true,
        false,
        Some("write")
    ),
    command!(
        "insert_table_after",
        AppDocument,
        [
            arg!("afterBlockId", String),
            arg!("rows", Number, optional),
            arg!("columns", Number, optional)
        ],
        true,
        false,
        Some("write")
    ),
    command!("add_table", AppDocument, [], true, false, Some("write")),
    command!(
        "add_table_row",
        AppDocument,
        [
            arg!("tableBlockId", String),
            arg!("afterRow", NullableString),
            arg!("text", String)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "delete_table_row",
        AppDocument,
        [arg!("tableBlockId", String), arg!("rowId", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "add_table_cell",
        AppDocument,
        [
            arg!("tableBlockId", String),
            arg!("rowId", String),
            arg!("afterCell", NullableString),
            arg!("text", String)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "delete_table_cell",
        AppDocument,
        [
            arg!("tableBlockId", String),
            arg!("rowId", String),
            arg!("cellId", String)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "insert_table_column",
        AppDocument,
        [
            arg!("tableBlockId", String),
            arg!("afterColumnId", NullableString)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "delete_table_column",
        AppDocument,
        [arg!("tableBlockId", String), arg!("columnId", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_table_column_width",
        AppDocument,
        [
            arg!("tableBlockId", String),
            arg!("columnId", String),
            arg!("twips", Number)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "clear_table_column_width",
        AppDocument,
        [arg!("tableBlockId", String), arg!("columnId", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "merge_table_cells",
        AppDocument,
        [
            arg!("cellId", String),
            arg!("rowSpan", Number),
            arg!("columnSpan", Number)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "split_table_cell",
        AppDocument,
        [arg!("cellId", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_table_cell_background",
        AppDocument,
        [arg!("cellId", String), arg!("color", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_table_cell_border",
        AppDocument,
        [
            arg!("cellId", String),
            arg!("edge", String),
            arg!("style", String),
            arg!("twips", Number),
            arg!("color", String)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_table_cell_vertical_alignment",
        AppDocument,
        [arg!("cellId", String), arg!("alignment", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_table_cell_padding",
        AppDocument,
        [
            arg!("cellId", String),
            arg!("edge", String),
            arg!("twips", Number)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "clear_table_cell_property",
        AppDocument,
        [arg!("cellId", String), arg!("key", String)],
        true,
        false,
        Some("write")
    ),
    command!("add_citation", AppDocument, [], true, false, Some("write")),
];
