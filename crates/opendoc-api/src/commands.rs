pub use crate::command_types::{CommandArg, CommandArgType, CommandReturn, CommandSpec};

macro_rules! arg {
    ($name:literal, $ty:ident) => {
        CommandArg {
            name: $name,
            ty: CommandArgType::$ty,
            optional: false,
        }
    };
    ($name:literal, $ty:ident, optional) => {
        CommandArg {
            name: $name,
            ty: CommandArgType::$ty,
            optional: true,
        }
    };
}

macro_rules! command {
    ($name:literal, $returns:ident, [$($arg:expr),* $(,)?], $undoable:literal, $closed:literal, $action:expr) => {
        CommandSpec {
            name: $name,
            returns: CommandReturn::$returns,
            args: &[$($arg),*],
            undoable: $undoable,
            allowed_without_open_document: $closed,
            required_action: $action,
        }
    };
}

pub static COMMANDS: &[CommandSpec] = &[
    command!(
        "create_document",
        AppDocument,
        [arg!("title", String)],
        false,
        true,
        Some("write")
    ),
    command!(
        "close_document",
        AppDocument,
        [],
        false,
        true,
        Some("write")
    ),
    command!("get_document", AppDocument, [], false, true, Some("read")),
    command!(
        "get_audit_view",
        AppAuditView,
        [],
        false,
        true,
        Some("read")
    ),
    command!(
        "get_runtime_profile",
        OpenDocRuntimeProfile,
        [
            arg!("mode", RuntimeMode),
            arg!("storageBackends", ObjectArray),
            arg!("signingEnabled", NullableBoolean)
        ],
        false,
        true,
        Some("read")
    ),
    command!(
        "get_runtime_session",
        OpenDocRuntimeSession,
        [
            arg!("mode", RuntimeMode),
            arg!("storageBackends", ObjectArray),
            arg!("signingEnabled", NullableBoolean),
            arg!("subject", NullableString),
            arg!("documentUuid", NullableString),
            arg!("presence", ObjectArray),
            arg!("permissions", ObjectArray)
        ],
        false,
        true,
        Some("read")
    ),
    command!(
        "authorize_runtime_command",
        OpenDocAuthorizationDecision,
        [
            arg!("mode", RuntimeMode),
            arg!("storageBackends", ObjectArray),
            arg!("signingEnabled", NullableBoolean),
            arg!("subject", NullableString),
            arg!("documentUuid", NullableString),
            arg!("commandName", String),
            arg!("permissions", ObjectArray)
        ],
        false,
        true,
        Some("read")
    ),
    command!(
        "create_runtime_share_invite",
        OpenDocShareInvite,
        [
            arg!("mode", RuntimeMode),
            arg!("storageBackends", ObjectArray),
            arg!("signingEnabled", NullableBoolean),
            arg!("subject", NullableString),
            arg!("documentUuid", NullableString),
            arg!("targetSubject", NullableString),
            arg!("actions", StringArray),
            arg!("permissions", ObjectArray)
        ],
        false,
        true,
        Some("share")
    ),
    command!(
        "relay_runtime_sync",
        OpenDocSyncRelayResult,
        [
            arg!("mode", RuntimeMode),
            arg!("storageBackends", ObjectArray),
            arg!("signingEnabled", NullableBoolean),
            arg!("subject", NullableString),
            arg!("documentUuid", NullableString),
            arg!("baseManifest", NullableString),
            arg!("operations", ObjectArray),
            arg!("permissions", ObjectArray),
            arg!("presence", ObjectArray)
        ],
        false,
        true,
        Some("write")
    ),
    command!(
        "resolve_runtime_document_lookup",
        OpenDocRuntimeLookupResult,
        [
            arg!("mode", RuntimeMode),
            arg!("storageBackends", ObjectArray),
            arg!("signingEnabled", NullableBoolean),
            arg!("subject", NullableString),
            arg!("documentUuid", NullableString),
            arg!("doi", NullableString),
            arg!("permissions", ObjectArray),
            arg!("serviceIndex", ObjectArray),
            arg!("scannedDocuments", ObjectArray)
        ],
        false,
        true,
        Some("read")
    ),
    command!(
        "undo_current_edit",
        AppDocument,
        [],
        false,
        false,
        Some("write")
    ),
    command!(
        "redo_current_edit",
        AppDocument,
        [],
        false,
        false,
        Some("write")
    ),
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
        String,
        [],
        false,
        false,
        Some("read")
    ),
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
        String,
        [],
        false,
        false,
        Some("read")
    ),
    command!(
        "add_binary_blob",
        AppDocument,
        [
            arg!("name", String),
            arg!("mediaType", String),
            arg!("bytes", NumberArray)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "update_binary_blob_metadata",
        AppDocument,
        [
            arg!("blobHash", String),
            arg!("name", String),
            arg!("mediaType", String)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "add_image_block",
        AppDocument,
        [arg!("blobHash", String), arg!("altText", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "insert_image_block_after",
        AppDocument,
        [
            arg!("afterBlockId", String),
            arg!("blobHash", String),
            arg!("altText", String)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "sign_blob_with_openssh_private_key",
        AppDocument,
        [
            arg!("blobHash", String),
            arg!("privateKeyPem", String),
            arg!("signerDisplay", String)
        ],
        false,
        false,
        Some("write")
    ),
    command!(
        "sign_fastq_blob_with_openssh_private_key",
        AppDocument,
        [
            arg!("blobHash", String),
            arg!("profile", String),
            arg!("privateKeyPem", String),
            arg!("signerDisplay", String)
        ],
        false,
        false,
        Some("write")
    ),
    command!(
        "sign_image_pixels_blob_with_openssh_private_key",
        AppDocument,
        [
            arg!("blobHash", String),
            arg!("width", Number),
            arg!("height", Number),
            arg!("pixels", NumberArray),
            arg!("privateKeyPem", String),
            arg!("signerDisplay", String)
        ],
        false,
        false,
        Some("write")
    ),
    command!(
        "delete_binary_blob",
        AppDocument,
        [arg!("blobHash", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "restore_binary_blob",
        AppDocument,
        [arg!("blobHash", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "simulate_shallow_clone",
        AppDocument,
        [],
        false,
        false,
        Some("write")
    ),
    command!(
        "record_blob_archive_tombstone",
        AppDocument,
        [
            arg!("blobHash", String),
            arg!("archiveLocator", String),
            arg!("restoreHint", String),
            arg!("signer", String),
            arg!("signature", NumberArray)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_document_doi",
        AppDocument,
        [arg!("doi", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_document_title",
        AppDocument,
        [arg!("title", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_document_locale",
        AppDocument,
        [arg!("locale", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "add_paragraph",
        AppDocument,
        [arg!("text", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "insert_paragraph_after",
        AppDocument,
        [arg!("afterBlockId", NullableString), arg!("text", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "split_paragraph_at_inline",
        AppDocument,
        [arg!("inlineId", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "split_paragraph_at_text_offset",
        AppDocument,
        [
            arg!("blockId", String),
            arg!("inlineId", String),
            arg!("offset", Number)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "join_paragraph_with_previous",
        AppDocument,
        [arg!("blockId", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "delete_block",
        AppDocument,
        [arg!("blockId", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_block_text_style",
        AppDocument,
        [
            arg!("blockId", String),
            arg!("style", String),
            arg!("level", Number),
            arg!("ordered", Boolean)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_editor_selection_block_style",
        AppDocument,
        [
            arg!("selection", EditorSelection),
            arg!("style", String),
            arg!("level", Number),
            arg!("ordered", Boolean)
        ],
        true,
        false,
        Some("write")
    ),
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
        "update_mention_label",
        AppDocument,
        [arg!("inlineId", String), arg!("label", String)],
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
            arg!("ordered", Boolean)
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
            arg!("ordered", Boolean)
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
            arg!("ordered", Boolean)
        ],
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
    command!("add_citation", AppDocument, [], true, false, Some("write")),
    command!(
        "insert_citation",
        AppDocument,
        [
            arg!("referenceId", String),
            arg!("afterInlineId", NullableString),
            arg!("locator", NullableString),
            arg!("label", NullableString),
            arg!("prefix", NullableString),
            arg!("suffix", NullableString),
            arg!("suppressAuthor", Boolean)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "insert_citation_group",
        AppDocument,
        [
            arg!("items", CitationItems),
            arg!("afterInlineId", NullableString)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "insert_footnote_citation_group",
        AppDocument,
        [arg!("footnoteId", String), arg!("items", CitationItems)],
        true,
        false,
        Some("write")
    ),
    command!(
        "insert_footnote_citation_after",
        AppDocument,
        [
            arg!("blockId", String),
            arg!("afterInlineId", NullableString),
            arg!("items", CitationItems)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "update_citation_group_items",
        AppDocument,
        [arg!("citationId", String), arg!("items", CitationItems)],
        true,
        false,
        Some("write")
    ),
    command!(
        "set_citation_style",
        AppDocument,
        [arg!("style", String), arg!("locale", String)],
        true,
        false,
        Some("write")
    ),
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
            arg!("text", String)
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
    command!(
        "update_bibliography_reference",
        AppDocument,
        [
            arg!("referenceId", String),
            arg!("title", String),
            arg!("issued", NullableString)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "update_bibliography_reference_metadata",
        AppDocument,
        [
            arg!("referenceId", String),
            arg!("title", String),
            arg!("authors", StringArray),
            arg!("issued", NullableString),
            arg!("doi", NullableString),
            arg!("url", NullableString)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "add_bibliography_reference",
        AppDocument,
        [
            arg!("title", String),
            arg!("authors", StringArray),
            arg!("issued", NullableString),
            arg!("doi", NullableString),
            arg!("url", NullableString)
        ],
        true,
        false,
        Some("write")
    ),
    command!(
        "delete_bibliography_reference",
        AppDocument,
        [arg!("referenceId", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "restore_bibliography_reference",
        AppDocument,
        [arg!("referenceId", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "delete_citation_group",
        AppDocument,
        [arg!("citationId", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "restore_citation_group",
        AppDocument,
        [arg!("citationId", String)],
        true,
        false,
        Some("write")
    ),
    command!(
        "save_local_repository",
        AppDocument,
        [arg!("path", String)],
        false,
        false,
        Some("write")
    ),
    command!(
        "save_local_repository_or_candidate",
        AppDocument,
        [arg!("path", String)],
        false,
        false,
        Some("write")
    ),
    command!(
        "save_flat_repository",
        AppDocument,
        [arg!("path", String), arg!("namespace", String)],
        false,
        false,
        Some("write")
    ),
    command!(
        "save_flat_repository_or_candidate",
        AppDocument,
        [arg!("path", String), arg!("namespace", String)],
        false,
        false,
        Some("write")
    ),
    command!(
        "save_opendal_fs_repository",
        AppDocument,
        [arg!("path", String), arg!("namespace", String)],
        false,
        false,
        Some("write")
    ),
    command!(
        "save_opendal_fs_repository_or_candidate",
        AppDocument,
        [arg!("path", String), arg!("namespace", String)],
        false,
        false,
        Some("write")
    ),
    command!(
        "autosave_current_repository",
        AppDocument,
        [],
        false,
        false,
        Some("write")
    ),
    command!(
        "open_local_repository",
        AppDocument,
        [arg!("path", String), arg!("documentUuid", String)],
        false,
        true,
        Some("read")
    ),
    command!(
        "scan_local_repository",
        AppDocument,
        [arg!("path", String)],
        false,
        false,
        None
    ),
    command!(
        "open_flat_repository",
        AppDocument,
        [
            arg!("path", String),
            arg!("namespace", String),
            arg!("documentUuid", String)
        ],
        false,
        true,
        Some("read")
    ),
    command!(
        "open_opendal_fs_repository",
        AppDocument,
        [
            arg!("path", String),
            arg!("namespace", String),
            arg!("documentUuid", String)
        ],
        false,
        true,
        Some("read")
    ),
    command!(
        "merge_local_repository_candidates",
        AppDocument,
        [arg!("path", String), arg!("documentUuid", String)],
        false,
        true,
        Some("write")
    ),
    command!(
        "compact_local_repository",
        AppDocument,
        [arg!("path", String), arg!("packName", String)],
        false,
        true,
        Some("write")
    ),
    command!(
        "merge_flat_repository_candidates",
        AppDocument,
        [
            arg!("path", String),
            arg!("namespace", String),
            arg!("documentUuid", String)
        ],
        false,
        true,
        Some("write")
    ),
    command!(
        "merge_opendal_fs_repository_candidates",
        AppDocument,
        [
            arg!("path", String),
            arg!("namespace", String),
            arg!("documentUuid", String)
        ],
        false,
        true,
        Some("write")
    ),
    command!(
        "open_local_repository_by_doi",
        AppDocument,
        [arg!("path", String), arg!("doi", String)],
        false,
        true,
        Some("read")
    ),
    command!(
        "open_flat_repository_by_doi",
        AppDocument,
        [
            arg!("path", String),
            arg!("namespace", String),
            arg!("doi", String)
        ],
        false,
        true,
        Some("read")
    ),
    command!(
        "open_opendal_fs_repository_by_doi",
        AppDocument,
        [
            arg!("path", String),
            arg!("namespace", String),
            arg!("doi", String)
        ],
        false,
        true,
        Some("read")
    ),
    command!(
        "sign_with_openssh_private_key",
        AppDocument,
        [arg!("privateKeyPem", String), arg!("signerDisplay", String)],
        false,
        false,
        Some("write")
    ),
    command!(
        "verify_current_signature",
        String,
        [arg!("privateKeyPem", String)],
        false,
        false,
        Some("read")
    ),
    command!(
        "verify_current_signatures",
        String,
        [],
        false,
        false,
        Some("read")
    ),
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

pub fn command_spec(command: &str) -> Option<&'static CommandSpec> {
    COMMANDS.iter().find(|spec| spec.name == command)
}

pub fn command_names() -> Vec<String> {
    COMMANDS.iter().map(|spec| spec.name.to_string()).collect()
}

pub fn is_undoable_command(command: &str) -> bool {
    command_spec(command)
        .map(|spec| spec.undoable)
        .unwrap_or(false)
}

pub fn is_closed_state_command(command: &str) -> bool {
    command_spec(command)
        .map(|spec| spec.allowed_without_open_document)
        .unwrap_or(false)
}

pub fn runtime_command_required_action(command: &str) -> Option<&'static str> {
    command_spec(command).and_then(|spec| spec.required_action)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_names_are_unique() {
        let mut names = COMMANDS.iter().map(|spec| spec.name).collect::<Vec<_>>();
        let original_len = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), original_len);
    }
}
