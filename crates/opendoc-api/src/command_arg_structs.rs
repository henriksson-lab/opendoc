//! Assembling the shared argument structs a command variant carries.

use crate::command_arg_values::*;
use crate::command_args::*;
use crate::command_parse::CommandParseError;
use serde_json::Value;

pub(crate) fn document_version(args: &Value) -> Result<DocumentVersionArgs, CommandParseError> {
    Ok(DocumentVersionArgs {
        manifest: arg_string(args, "manifest")?,
    })
}

pub(crate) fn repository_path(args: &Value) -> Result<RepositoryPathArgs, CommandParseError> {
    Ok(RepositoryPathArgs {
        path: arg_string(args, "path")?,
    })
}

pub(crate) fn repository_namespace(
    args: &Value,
) -> Result<RepositoryNamespaceArgs, CommandParseError> {
    Ok(RepositoryNamespaceArgs {
        path: arg_string(args, "path")?,
        namespace: arg_string(args, "namespace")?,
    })
}

pub(crate) fn repository_document(
    args: &Value,
) -> Result<RepositoryDocumentArgs, CommandParseError> {
    Ok(RepositoryDocumentArgs {
        path: arg_string(args, "path")?,
        document_uuid: arg_string(args, "documentUuid")?,
    })
}

pub(crate) fn repository_namespace_document(
    args: &Value,
) -> Result<RepositoryNamespaceDocumentArgs, CommandParseError> {
    Ok(RepositoryNamespaceDocumentArgs {
        path: arg_string(args, "path")?,
        namespace: arg_string(args, "namespace")?,
        document_uuid: arg_string(args, "documentUuid")?,
    })
}

pub(crate) fn repository_doi(args: &Value) -> Result<RepositoryDoiArgs, CommandParseError> {
    Ok(RepositoryDoiArgs {
        path: arg_string(args, "path")?,
        doi: arg_string(args, "doi")?,
    })
}

pub(crate) fn repository_namespace_doi(
    args: &Value,
) -> Result<RepositoryNamespaceDoiArgs, CommandParseError> {
    Ok(RepositoryNamespaceDoiArgs {
        path: arg_string(args, "path")?,
        namespace: arg_string(args, "namespace")?,
        doi: arg_string(args, "doi")?,
    })
}

pub(crate) fn block_id_args(args: &Value) -> Result<BlockIdArgs, CommandParseError> {
    Ok(BlockIdArgs {
        block_id: arg_string(args, "blockId")?,
    })
}

pub(crate) fn after_block_args(args: &Value) -> Result<AfterBlockArgs, CommandParseError> {
    Ok(AfterBlockArgs {
        after_block_id: arg_string(args, "afterBlockId")?,
    })
}

pub(crate) fn insert_table_after_args(
    args: &Value,
) -> Result<InsertTableAfterArgs, CommandParseError> {
    let after_block_id = arg_string(args, "afterBlockId")?;
    match (
        args.get("rows").and_then(Value::as_u64),
        args.get("columns").and_then(Value::as_u64),
    ) {
        (Some(rows), Some(columns)) => Ok(InsertTableAfterArgs::Sized {
            after_block_id,
            rows: rows as usize,
            columns: columns as usize,
        }),
        _ => Ok(InsertTableAfterArgs::Default { after_block_id }),
    }
}

pub(crate) fn author_body_args(args: &Value) -> Result<AuthorBodyArgs, CommandParseError> {
    Ok(AuthorBodyArgs {
        author: arg_string(args, "author")?,
        body: arg_string(args, "body")?,
    })
}

pub(crate) fn text_range_author_body_args(
    args: &Value,
) -> Result<TextRangeAuthorBodyArgs, CommandParseError> {
    Ok(TextRangeAuthorBodyArgs {
        start_inline_id: arg_string(args, "startInlineId")?,
        end_inline_id: arg_string(args, "endInlineId")?,
        author: arg_string(args, "author")?,
        body: arg_string(args, "body")?,
    })
}

pub(crate) fn block_author_body_args(
    args: &Value,
) -> Result<BlockAuthorBodyArgs, CommandParseError> {
    Ok(BlockAuthorBodyArgs {
        block_id: arg_string(args, "blockId")?,
        author: arg_string(args, "author")?,
        body: arg_string(args, "body")?,
    })
}

pub(crate) fn thread_author_body_args(
    args: &Value,
) -> Result<ThreadAuthorBodyArgs, CommandParseError> {
    Ok(ThreadAuthorBodyArgs {
        thread_id: arg_string(args, "threadId")?,
        author: arg_string(args, "author")?,
        body: arg_string(args, "body")?,
    })
}

pub(crate) fn thread_id_args(args: &Value) -> Result<ThreadIdArgs, CommandParseError> {
    Ok(ThreadIdArgs {
        thread_id: arg_string(args, "threadId")?,
    })
}

pub(crate) fn thread_comment_id_args(
    args: &Value,
) -> Result<ThreadCommentIdArgs, CommandParseError> {
    Ok(ThreadCommentIdArgs {
        thread_id: arg_string(args, "threadId")?,
        comment_id: arg_string(args, "commentId")?,
    })
}

pub(crate) fn author_text_args(args: &Value) -> Result<AuthorTextArgs, CommandParseError> {
    Ok(AuthorTextArgs {
        author: arg_string(args, "author")?,
        text: arg_string(args, "text")?,
    })
}

pub(crate) fn text_range_author_text_args(
    args: &Value,
) -> Result<TextRangeAuthorTextArgs, CommandParseError> {
    Ok(TextRangeAuthorTextArgs {
        start_inline_id: arg_string(args, "startInlineId")?,
        end_inline_id: arg_string(args, "endInlineId")?,
        author: arg_string(args, "author")?,
        text: arg_string(args, "text")?,
    })
}

pub(crate) fn block_author_text_args(
    args: &Value,
) -> Result<BlockAuthorTextArgs, CommandParseError> {
    Ok(BlockAuthorTextArgs {
        block_id: arg_string(args, "blockId")?,
        author: arg_string(args, "author")?,
        text: arg_string(args, "text")?,
    })
}

pub(crate) fn text_range_author_args(
    args: &Value,
) -> Result<TextRangeAuthorArgs, CommandParseError> {
    Ok(TextRangeAuthorArgs {
        start_inline_id: arg_string(args, "startInlineId")?,
        end_inline_id: arg_string(args, "endInlineId")?,
        author: arg_string(args, "author")?,
    })
}

pub(crate) fn format_suggestion_args(
    args: &Value,
) -> Result<FormatSuggestionArgs, CommandParseError> {
    Ok(FormatSuggestionArgs {
        author: arg_string(args, "author")?,
        inline_id: arg_string(args, "inlineId")?,
        mark_kind: arg_string(args, "markKind")?,
        value: arg_optional_string(args, "value")?,
    })
}

pub(crate) fn bibliography_reference_metadata_args(
    args: &Value,
) -> Result<BibliographyReferenceMetadataArgs, CommandParseError> {
    Ok(BibliographyReferenceMetadataArgs {
        title: arg_string(args, "title")?,
        authors: arg_string_vec(args, "authors")?,
        issued: arg_optional_string(args, "issued")?,
        doi: arg_optional_string(args, "doi")?,
        url: arg_optional_string(args, "url")?,
    })
}

pub(crate) fn reference_id_args(args: &Value) -> Result<ReferenceIdArgs, CommandParseError> {
    Ok(ReferenceIdArgs {
        reference_id: arg_string(args, "referenceId")?,
    })
}

pub(crate) fn citation_id_args(args: &Value) -> Result<CitationIdArgs, CommandParseError> {
    Ok(CitationIdArgs {
        citation_id: arg_string(args, "citationId")?,
    })
}

pub(crate) fn inline_id_args(args: &Value) -> Result<InlineIdArgs, CommandParseError> {
    Ok(InlineIdArgs {
        inline_id: arg_string(args, "inlineId")?,
    })
}

pub(crate) fn text_mark_args(args: &Value) -> Result<TextMarkArgs, CommandParseError> {
    Ok(TextMarkArgs {
        inline_id: arg_string(args, "inlineId")?,
        mark_kind: arg_string(args, "markKind")?,
        value: arg_optional_string(args, "value")?,
    })
}

pub(crate) fn text_mark_range_args(args: &Value) -> Result<TextMarkRangeArgs, CommandParseError> {
    Ok(TextMarkRangeArgs {
        start_inline_id: arg_string(args, "startInlineId")?,
        end_inline_id: arg_string(args, "endInlineId")?,
        mark_kind: arg_string(args, "markKind")?,
        value: arg_optional_string(args, "value")?,
    })
}

pub(crate) fn sheet_id_args(args: &Value) -> Result<SheetIdArgs, CommandParseError> {
    Ok(SheetIdArgs {
        sheet_id: arg_string(args, "sheetId")?,
    })
}

pub(crate) fn sheet_rename_args(args: &Value) -> Result<SheetRenameArgs, CommandParseError> {
    Ok(SheetRenameArgs {
        sheet_id: arg_string(args, "sheetId")?,
        title: arg_string(args, "title")?,
    })
}

pub(crate) fn sheet_row_args(args: &Value) -> Result<SheetRowArgs, CommandParseError> {
    Ok(SheetRowArgs {
        sheet_id: arg_string(args, "sheetId")?,
        row: arg_string(args, "row")?,
    })
}

pub(crate) fn sheet_column_args(args: &Value) -> Result<SheetColumnArgs, CommandParseError> {
    Ok(SheetColumnArgs {
        sheet_id: arg_string(args, "sheetId")?,
        column: arg_string(args, "column")?,
    })
}

pub(crate) fn spreadsheet_selection_args(
    args: &Value,
) -> Result<SpreadsheetSelectionArgs, CommandParseError> {
    Ok(SpreadsheetSelectionArgs {
        sheet_id: arg_string(args, "sheetId")?,
        anchor: arg_string(args, "anchor")?,
        focus: arg_string(args, "focus")?,
    })
}

pub(crate) fn cell_comment_id_args(args: &Value) -> Result<CellCommentIdArgs, CommandParseError> {
    Ok(CellCommentIdArgs {
        comment_id: arg_string(args, "commentId")?,
    })
}

pub(crate) fn sheet_address_args(args: &Value) -> Result<SheetAddressArgs, CommandParseError> {
    Ok(SheetAddressArgs {
        sheet_id: arg_string(args, "sheetId")?,
        address: arg_string(args, "address")?,
    })
}

pub(crate) fn sheet_range_args(args: &Value) -> Result<SheetRangeArgs, CommandParseError> {
    Ok(SheetRangeArgs {
        sheet_id: arg_string(args, "sheetId")?,
        range: arg_string(args, "range")?,
    })
}

pub(crate) fn protected_range_args(args: &Value) -> Result<ProtectedRangeArgs, CommandParseError> {
    Ok(ProtectedRangeArgs {
        sheet_id: arg_string(args, "sheetId")?,
        range: arg_string(args, "range")?,
        description: arg_string(args, "description")?,
        warning_only: arg_bool(args, "warningOnly")?,
    })
}

pub(crate) fn named_range_args(args: &Value) -> Result<NamedRangeArgs, CommandParseError> {
    Ok(NamedRangeArgs {
        sheet_id: arg_string(args, "sheetId")?,
        name: arg_string(args, "name")?,
        range: arg_string(args, "range")?,
    })
}

pub(crate) fn named_range_name_args(args: &Value) -> Result<NamedRangeNameArgs, CommandParseError> {
    Ok(NamedRangeNameArgs {
        name: arg_string(args, "name")?,
    })
}
