//! Whole-document import and export commands.

use crate::command_types::{CommandArg, CommandArgType, CommandReturn, CommandSpec};

pub(crate) const SPECS: &[CommandSpec] = &[
    command!(
        "import_google_docs_json",
        AppDocument,
        [arg!("title", String), arg!("jsonText", String)],
        false,
        true,
        Some("write")
    ),
    command!(
        "import_doc_or_docx_path",
        AppDocument,
        [arg!("path", String)],
        false,
        true,
        Some("write")
    ),
    command!(
        "export_google_docs_json",
        AppExport,
        [],
        false,
        false,
        Some("read")
    ),
    command!("export_docx", AppExport, [], false, false, Some("read")),
    command!(
        "import_google_sheets_json",
        AppDocument,
        [arg!("jsonText", String)],
        false,
        false,
        Some("write")
    ),
    command!(
        "export_google_sheets_json",
        AppExport,
        [],
        false,
        false,
        Some("read")
    ),
];
