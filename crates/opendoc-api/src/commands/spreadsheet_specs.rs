//! Every spreadsheet command: cells, structure, formats and ranges.

use crate::command_types::{CommandArg, CommandArgType, CommandReturn, CommandSpec};

pub(crate) const SPECS: &[CommandSpec] = &[
    command!(
        "set_spreadsheet_cell",
        AppDocument,
        [arg!("address", String), arg!("value", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "describe_spreadsheet_selection",
        AppSpreadsheetSelection,
        [
            arg!("sheetId", String),
            arg!("anchor", String),
            arg!("focus", String)
        ],
        false,
        false,
        Some("read")
    ),
    command!(
        "reduce_spreadsheet_selection",
        AppSpreadsheetSelection,
        [
            arg!("sheetId", String),
            arg!("anchor", String),
            arg!("focus", String),
            arg!("action", String),
            arg!("value", String),
            arg!("extend", Boolean)
        ],
        false,
        false,
        Some("read")
    ),
    command!(
        "copy_spreadsheet_selection_tsv",
        String,
        [
            arg!("sheetId", String),
            arg!("anchor", String),
            arg!("focus", String)
        ],
        false,
        false,
        Some("read")
    ),
    command!(
        "paste_spreadsheet_tsv",
        AppDocument,
        [
            arg!("sheetId", String),
            arg!("origin", String),
            arg!("text", String),
            arg!("sourceOrigin", NullableString, optional)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "clear_spreadsheet_selection",
        AppDocument,
        [
            arg!("sheetId", String),
            arg!("anchor", String),
            arg!("focus", String)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_spreadsheet_selection_format",
        AppDocument,
        [
            arg!("sheetId", String),
            arg!("anchor", String),
            arg!("focus", String),
            arg!("property", String),
            arg!("value", String)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "add_spreadsheet_row_after_selection",
        AppDocument,
        [
            arg!("sheetId", String),
            arg!("anchor", String),
            arg!("focus", String)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "add_spreadsheet_column_after_selection",
        AppDocument,
        [
            arg!("sheetId", String),
            arg!("anchor", String),
            arg!("focus", String)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "delete_spreadsheet_selection_row",
        AppDocument,
        [
            arg!("sheetId", String),
            arg!("anchor", String),
            arg!("focus", String)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "delete_spreadsheet_selection_column",
        AppDocument,
        [
            arg!("sheetId", String),
            arg!("anchor", String),
            arg!("focus", String)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "merge_spreadsheet_selection",
        AppDocument,
        [
            arg!("sheetId", String),
            arg!("anchor", String),
            arg!("focus", String)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "freeze_spreadsheet_selection",
        AppDocument,
        [
            arg!("sheetId", String),
            arg!("anchor", String),
            arg!("focus", String)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_spreadsheet_selection_filter",
        AppDocument,
        [
            arg!("sheetId", String),
            arg!("anchor", String),
            arg!("focus", String)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_spreadsheet_workbook_metadata",
        AppDocument,
        [
            arg!("title", String),
            arg!("locale", String),
            arg!("timezone", String)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_spreadsheet_cells",
        AppDocument,
        [arg!("cells", SpreadsheetCellEdits)],
        true,
        false,
        Some("write")
    ),
    command!(
        "add_spreadsheet_sheet",
        AppDocument,
        [arg!("title", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "rename_spreadsheet_sheet",
        AppDocument,
        [arg!("sheetId", String), arg!("title", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "delete_spreadsheet_sheet",
        AppDocument,
        [arg!("sheetId", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "restore_spreadsheet_sheet",
        AppDocument,
        [arg!("sheetId", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "add_spreadsheet_row",
        AppDocument,
        [arg!("sheetId", String), arg!("row", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "delete_spreadsheet_row",
        AppDocument,
        [arg!("sheetId", String), arg!("row", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "restore_spreadsheet_row",
        AppDocument,
        [arg!("sheetId", String), arg!("row", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "add_spreadsheet_column",
        AppDocument,
        [arg!("sheetId", String), arg!("column", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "delete_spreadsheet_column",
        AppDocument,
        [arg!("sheetId", String), arg!("column", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "restore_spreadsheet_column",
        AppDocument,
        [arg!("sheetId", String), arg!("column", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "add_spreadsheet_cell_comment",
        AppDocument,
        [
            arg!("sheetId", String),
            arg!("address", String),
            arg!("author", String),
            arg!("body", String)
        ],
        true,
        false,
        Some("comment")
    ),
    command!(
        "update_spreadsheet_cell_comment",
        AppDocument,
        [arg!("commentId", String), arg!("body", String)],
        true,
        false,
        Some("comment")
    ),
    command!(
        "delete_spreadsheet_cell_comment",
        AppDocument,
        [arg!("commentId", String)],
        true,
        false,
        Some("comment")
    ),
    command!(
        "restore_spreadsheet_cell_comment",
        AppDocument,
        [arg!("commentId", String)],
        true,
        false,
        Some("comment")
    ),
    command!(
        "set_spreadsheet_frozen_axes",
        AppDocument,
        [
            arg!("sheetId", String),
            arg!("frozenRows", Number),
            arg!("frozenColumns", Number)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_spreadsheet_cell_validation",
        AppDocument,
        [
            arg!("sheetId", String),
            arg!("address", String),
            arg!("kind", String),
            arg!("values", StringArray),
            arg!("strict", Boolean)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "clear_spreadsheet_cell_validation",
        AppDocument,
        [arg!("sheetId", String), arg!("address", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "restore_spreadsheet_cell_validation",
        AppDocument,
        [arg!("sheetId", String), arg!("address", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "merge_spreadsheet_cells",
        AppDocument,
        [arg!("sheetId", String), arg!("range", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "unmerge_spreadsheet_cells",
        AppDocument,
        [arg!("sheetId", String), arg!("range", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "restore_spreadsheet_merge",
        AppDocument,
        [arg!("sheetId", String), arg!("range", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_spreadsheet_basic_filter",
        AppDocument,
        [arg!("sheetId", String), arg!("range", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_spreadsheet_basic_filter_options",
        AppDocument,
        [
            arg!("sheetId", String),
            arg!("criteria", ObjectArray),
            arg!("sortSpecs", ObjectArray)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "clear_spreadsheet_basic_filter",
        AppDocument,
        [arg!("sheetId", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "restore_spreadsheet_basic_filter",
        AppDocument,
        [arg!("sheetId", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "add_spreadsheet_protected_range",
        AppDocument,
        [
            arg!("sheetId", String),
            arg!("range", String),
            arg!("description", String),
            arg!("warningOnly", Boolean)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "update_spreadsheet_protected_range",
        AppDocument,
        [
            arg!("sheetId", String),
            arg!("range", String),
            arg!("description", String),
            arg!("warningOnly", Boolean)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "delete_spreadsheet_protected_range",
        AppDocument,
        [arg!("sheetId", String), arg!("range", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "restore_spreadsheet_protected_range",
        AppDocument,
        [arg!("sheetId", String), arg!("range", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_spreadsheet_cell_in_sheet",
        AppDocument,
        [
            arg!("sheetId", String),
            arg!("address", String),
            arg!("value", String)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_spreadsheet_cells_in_sheet",
        AppDocument,
        [arg!("sheetId", String), arg!("cells", SpreadsheetCellEdits)],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_spreadsheet_cell_format",
        AppDocument,
        [
            arg!("sheetId", String),
            arg!("address", String),
            arg!("property", String),
            arg!("value", String)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_spreadsheet_row_height",
        AppDocument,
        [
            arg!("sheetId", String),
            arg!("row", String),
            arg!("height", Number)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_spreadsheet_column_width",
        AppDocument,
        [
            arg!("sheetId", String),
            arg!("column", String),
            arg!("width", Number)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "copy_spreadsheet_range",
        AppDocument,
        [
            arg!("sheetId", String),
            arg!("sourceRange", String),
            arg!("targetAddress", String)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "sort_spreadsheet_range",
        AppDocument,
        [
            arg!("sheetId", String),
            arg!("range", String),
            arg!("column", String),
            arg!("descending", Boolean),
            arg!("hasHeader", Boolean)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "fill_spreadsheet_range",
        AppDocument,
        [
            arg!("sheetId", String),
            arg!("sourceRange", String),
            arg!("targetRange", String)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "import_spreadsheet_csv",
        AppDocument,
        [
            arg!("sheetId", String),
            arg!("origin", String),
            arg!("text", String),
            arg!("delimiter", NullableString, optional)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "export_spreadsheet_csv",
        String,
        [
            arg!("sheetId", String),
            arg!("delimiter", NullableString, optional)
        ],
        false,
        false,
        Some("read")
    ),
    command!(
        "import_spreadsheet_xlsx",
        AppDocument,
        [arg!("title", String), arg!("base64", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "export_spreadsheet_xlsx",
        String,
        [],
        false,
        false,
        Some("read")
    ),
    command!(
        "add_spreadsheet_named_range",
        AppDocument,
        [
            arg!("sheetId", String),
            arg!("name", String),
            arg!("range", String)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "update_spreadsheet_named_range",
        AppDocument,
        [
            arg!("sheetId", String),
            arg!("name", String),
            arg!("range", String)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "delete_spreadsheet_named_range",
        AppDocument,
        [arg!("name", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "restore_spreadsheet_named_range",
        AppDocument,
        [arg!("name", String)],
        true,
        false,
        Some("write")
    ),
];
