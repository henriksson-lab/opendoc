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
    command!("export_odt", AppExport, [], false, false, Some("read")),
    // FS-21. Paginated by `opendoc-layout`, so the PDF breaks where the
    // editor breaks; ADR 0003 keeps rendered output unsigned.
    command!("export_pdf", AppExport, [], false, false, Some("read")),
    // The standalone HTML and plain-text exports were the last two built in
    // TypeScript. They return `AppExport` like every other export, so they
    // carry their own warnings and state their own media type (ADR 0010).
    command!("export_html", AppExport, [], false, false, Some("read")),
    command!("export_text", AppExport, [], false, false, Some("read")),
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
