use crate::*;
use opendoc_spreadsheet::{SheetFilterCriterion, SheetFilterSortSpec};
use serde_json::{json, Value};

#[test]
fn migrated_commands_exist_in_rust_metadata() {
    let commands = [
        OpenDocCommand::CreateDocument(CreateDocumentArgs {
            title: "Title".to_string(),
        }),
        OpenDocCommand::CloseDocument,
        OpenDocCommand::UndoCurrentEdit,
        OpenDocCommand::RedoCurrentEdit,
        OpenDocCommand::GetDocument,
        OpenDocCommand::GetAuditView,
        OpenDocCommand::SetDocumentTitle(SetDocumentTitleArgs {
            title: "Title".to_string(),
        }),
        OpenDocCommand::SetDocumentLocale(SetDocumentLocaleArgs {
            locale: "en-US".to_string(),
        }),
        OpenDocCommand::AddParagraph(AddParagraphArgs {
            text: "Paragraph".to_string(),
        }),
        OpenDocCommand::RenderDocumentHtml,
        OpenDocCommand::GetRuntimeProfile(
            runtime_profile_from_args(&json!({
                "mode": "tauri-local",
            }))
            .expect("runtime profile"),
        ),
        OpenDocCommand::GetRuntimeSession(GetRuntimeSessionArgs {
            profile: runtime_profile_from_args(&json!({
                "mode": "tauri-local",
            }))
            .expect("runtime profile"),
            subject: None,
            document_uuid: None,
            presence: Vec::new(),
            permissions: Vec::new(),
        }),
        OpenDocCommand::AuthorizeRuntimeCommand(AuthorizeRuntimeCommandArgs {
            profile: runtime_profile_from_args(&json!({
                "mode": "tauri-local",
            }))
            .expect("runtime profile"),
            subject: None,
            document_uuid: None,
            command_name: "get_document".to_string(),
            permissions: Vec::new(),
        }),
        OpenDocCommand::CreateRuntimeShareInvite(CreateRuntimeShareInviteArgs {
            profile: runtime_profile_from_args(&json!({
                "mode": "multi-user-service",
            }))
            .expect("runtime profile"),
            subject: None,
            document_uuid: None,
            target_subject: None,
            actions: Vec::new(),
            permissions: Vec::new(),
        }),
        OpenDocCommand::RelayRuntimeSync(RelayRuntimeSyncArgs {
            profile: runtime_profile_from_args(&json!({
                "mode": "multi-user-service",
            }))
            .expect("runtime profile"),
            subject: None,
            document_uuid: None,
            base_manifest: None,
            operations: Vec::new(),
            permissions: Vec::new(),
            presence: Vec::new(),
        }),
        OpenDocCommand::ResolveRuntimeDocumentLookup(ResolveRuntimeDocumentLookupArgs {
            profile: runtime_profile_from_args(&json!({
                "mode": "tauri-local",
            }))
            .expect("runtime profile"),
            subject: None,
            document_uuid: None,
            doi: None,
            permissions: Vec::new(),
            service_index: Vec::new(),
            scanned_documents: Vec::new(),
        }),
        OpenDocCommand::ImportGoogleDocsJson(ImportGoogleDocsJsonArgs {
            title: "Title".to_string(),
            json_text: "{}".to_string(),
        }),
        OpenDocCommand::ImportDocOrDocxPath(ImportDocOrDocxPathArgs {
            path: "document.docx".to_string(),
        }),
        OpenDocCommand::ExportGoogleDocsJson,
        OpenDocCommand::ImportGoogleSheetsJson(ImportGoogleSheetsJsonArgs {
            json_text: "{}".to_string(),
        }),
        OpenDocCommand::ExportGoogleSheetsJson,
        OpenDocCommand::RenderWorkbookHtml(RenderWorkbookHtmlArgs {
            sheet_id: "sheet-1".to_string(),
        }),
        OpenDocCommand::ImportDocxBase64(ImportDocxBase64Args {
            name: "document.docx".to_string(),
            base64: String::new(),
        }),
        OpenDocCommand::AddBinaryBlob(AddBinaryBlobArgs {
            name: "data.bin".to_string(),
            media_type: "application/octet-stream".to_string(),
            bytes: Vec::new(),
        }),
        OpenDocCommand::SimulateShallowClone,
        OpenDocCommand::UpdateBinaryBlobMetadata(UpdateBinaryBlobMetadataArgs {
            blob_hash: "sha256:00".to_string(),
            name: "data.bin".to_string(),
            media_type: "application/octet-stream".to_string(),
        }),
        OpenDocCommand::DeleteBinaryBlob(DeleteBinaryBlobArgs {
            blob_hash: "sha256:00".to_string(),
        }),
        OpenDocCommand::RestoreBinaryBlob(RestoreBinaryBlobArgs {
            blob_hash: "sha256:00".to_string(),
        }),
        OpenDocCommand::RecordBlobArchiveTombstone(RecordBlobArchiveTombstoneArgs {
            blob_hash: "sha256:00".to_string(),
            archive_locator: "archive".to_string(),
            restore_hint: "restore".to_string(),
            signer: "signer".to_string(),
            signature: Vec::new(),
        }),
        OpenDocCommand::AddImageBlock(AddImageBlockArgs {
            blob_hash: "sha256:00".to_string(),
            alt_text: "alt".to_string(),
        }),
        OpenDocCommand::InsertImageBlockAfter(InsertImageBlockAfterArgs {
            after_block_id: "block".to_string(),
            blob_hash: "sha256:00".to_string(),
            alt_text: "alt".to_string(),
        }),
        OpenDocCommand::SignBlobWithOpenSshPrivateKey(SignBlobWithOpenSshPrivateKeyArgs {
            blob_hash: "sha256:00".to_string(),
            private_key_pem: String::new(),
            signer_display: "Signer".to_string(),
        }),
        OpenDocCommand::SignFastqBlobWithOpenSshPrivateKey(
            SignFastqBlobWithOpenSshPrivateKeyArgs {
                blob_hash: "sha256:00".to_string(),
                profile: "sequence".to_string(),
                private_key_pem: String::new(),
                signer_display: "Signer".to_string(),
            },
        ),
        OpenDocCommand::SignImagePixelsBlobWithOpenSshPrivateKey(
            SignImagePixelsBlobWithOpenSshPrivateKeyArgs {
                blob_hash: "sha256:00".to_string(),
                width: 1,
                height: 1,
                pixels: vec![0, 0, 0, 0],
                private_key_pem: String::new(),
                signer_display: "Signer".to_string(),
            },
        ),
        OpenDocCommand::SignWithOpenSshPrivateKey(SignWithOpenSshPrivateKeyArgs {
            private_key_pem: String::new(),
            signer_display: "Signer".to_string(),
        }),
        OpenDocCommand::VerifyCurrentSignature(VerifyCurrentSignatureArgs {
            private_key_pem: String::new(),
        }),
        OpenDocCommand::VerifyCurrentSignatures,
        OpenDocCommand::SaveLocalRepository(RepositoryPathArgs {
            path: "repo".to_string(),
        }),
        OpenDocCommand::SaveLocalRepositoryOrCandidate(RepositoryPathArgs {
            path: "repo".to_string(),
        }),
        OpenDocCommand::SaveFlatRepository(RepositoryNamespaceArgs {
            path: "repo".to_string(),
            namespace: "bucket/prefix".to_string(),
        }),
        OpenDocCommand::SaveFlatRepositoryOrCandidate(RepositoryNamespaceArgs {
            path: "repo".to_string(),
            namespace: "bucket/prefix".to_string(),
        }),
        OpenDocCommand::SaveOpenDalFsRepository(RepositoryNamespaceArgs {
            path: "repo".to_string(),
            namespace: "bucket/prefix".to_string(),
        }),
        OpenDocCommand::SaveOpenDalFsRepositoryOrCandidate(RepositoryNamespaceArgs {
            path: "repo".to_string(),
            namespace: "bucket/prefix".to_string(),
        }),
        OpenDocCommand::AutosaveCurrentRepository,
        OpenDocCommand::CompactLocalRepository(CompactLocalRepositoryArgs {
            path: "repo".to_string(),
            pack_name: "pack".to_string(),
        }),
        OpenDocCommand::OpenLocalRepository(RepositoryDocumentArgs {
            path: "repo".to_string(),
            document_uuid: "doc".to_string(),
        }),
        OpenDocCommand::ScanLocalRepository(RepositoryPathArgs {
            path: "repo".to_string(),
        }),
        OpenDocCommand::OpenFlatRepository(RepositoryNamespaceDocumentArgs {
            path: "repo".to_string(),
            namespace: "bucket/prefix".to_string(),
            document_uuid: "doc".to_string(),
        }),
        OpenDocCommand::OpenOpenDalFsRepository(RepositoryNamespaceDocumentArgs {
            path: "repo".to_string(),
            namespace: "bucket/prefix".to_string(),
            document_uuid: "doc".to_string(),
        }),
        OpenDocCommand::MergeLocalRepositoryCandidates(RepositoryDocumentArgs {
            path: "repo".to_string(),
            document_uuid: "doc".to_string(),
        }),
        OpenDocCommand::MergeFlatRepositoryCandidates(RepositoryNamespaceDocumentArgs {
            path: "repo".to_string(),
            namespace: "bucket/prefix".to_string(),
            document_uuid: "doc".to_string(),
        }),
        OpenDocCommand::MergeOpenDalFsRepositoryCandidates(RepositoryNamespaceDocumentArgs {
            path: "repo".to_string(),
            namespace: "bucket/prefix".to_string(),
            document_uuid: "doc".to_string(),
        }),
        OpenDocCommand::OpenLocalRepositoryByDoi(RepositoryDoiArgs {
            path: "repo".to_string(),
            doi: "10.1/example".to_string(),
        }),
        OpenDocCommand::OpenFlatRepositoryByDoi(RepositoryNamespaceDoiArgs {
            path: "repo".to_string(),
            namespace: "bucket/prefix".to_string(),
            doi: "10.1/example".to_string(),
        }),
        OpenDocCommand::OpenOpenDalFsRepositoryByDoi(RepositoryNamespaceDoiArgs {
            path: "repo".to_string(),
            namespace: "bucket/prefix".to_string(),
            doi: "10.1/example".to_string(),
        }),
        OpenDocCommand::SetDocumentDoi(SetDocumentDoiArgs {
            doi: "10.1/example".to_string(),
        }),
        OpenDocCommand::InsertParagraphAfter(InsertParagraphAfterArgs {
            after_block_id: None,
            text: "Paragraph".to_string(),
        }),
        OpenDocCommand::SplitParagraphAtInline(SplitParagraphAtInlineArgs {
            inline_id: "inline".to_string(),
        }),
        OpenDocCommand::SplitParagraphAtTextOffset(SplitParagraphAtTextOffsetArgs {
            block_id: "block".to_string(),
            inline_id: "inline".to_string(),
            offset: 1,
        }),
        OpenDocCommand::JoinParagraphWithPrevious(BlockIdArgs {
            block_id: "block".to_string(),
        }),
        OpenDocCommand::DeleteBlock(BlockIdArgs {
            block_id: "block".to_string(),
        }),
        OpenDocCommand::SetBlockTextStyle(SetBlockTextStyleArgs {
            block_id: "block".to_string(),
            style: "heading".to_string(),
            level: 1,
            list_kind: "bullet".to_string(),
        }),
        OpenDocCommand::AddHeading(AddHeadingArgs {
            text: "Heading".to_string(),
            level: 1,
        }),
        OpenDocCommand::UpdateHeadingLevel(UpdateHeadingLevelArgs {
            block_id: "block".to_string(),
            level: 2,
        }),
        OpenDocCommand::AddLink(AddLinkArgs {
            text: "Link".to_string(),
            href: "https://example.invalid".to_string(),
        }),
        OpenDocCommand::InsertLinkAfter(InsertLinkAfterArgs {
            block_id: "block".to_string(),
            after_inline_id: None,
            text: "Link".to_string(),
            href: "https://example.invalid".to_string(),
        }),
        OpenDocCommand::AddMention(AddMentionArgs {
            label: "Ada".to_string(),
        }),
        OpenDocCommand::InsertMentionAfter(InsertMentionAfterArgs {
            block_id: "block".to_string(),
            after_inline_id: None,
            label: "Ada".to_string(),
        }),
        OpenDocCommand::AddFootnoteRef,
        OpenDocCommand::InsertFootnoteRefAfter(InsertFootnoteRefAfterArgs {
            block_id: "block".to_string(),
            after_inline_id: None,
        }),
        OpenDocCommand::UpdateFootnoteBody(UpdateFootnoteBodyArgs {
            footnote_id: "footnote".to_string(),
            body: "Body".to_string(),
        }),
        OpenDocCommand::AddEquation(AddEquationArgs {
            source: "E=mc^2".to_string(),
        }),
        OpenDocCommand::InsertEquationAfter(InsertEquationAfterArgs {
            block_id: "block".to_string(),
            after_inline_id: None,
            source: "E=mc^2".to_string(),
        }),
        OpenDocCommand::AddEquationBlock(AddEquationArgs {
            source: "E=mc^2".to_string(),
        }),
        OpenDocCommand::InsertEquationBlockAfter(InsertEquationBlockAfterArgs {
            after_block_id: "block".to_string(),
            source: "E=mc^2".to_string(),
        }),
        OpenDocCommand::AddListItem(AddListItemArgs {
            text: "Item".to_string(),
            level: 0,
            list_kind: "bullet".to_string(),
        }),
        OpenDocCommand::InsertListItemAfter(InsertListItemAfterArgs {
            after_block_id: "block".to_string(),
            text: "Item".to_string(),
            level: 0,
            list_kind: "bullet".to_string(),
        }),
        OpenDocCommand::UpdateListItem(UpdateListItemArgs {
            block_id: "block".to_string(),
            level: 0,
            list_kind: "bullet".to_string(),
        }),
        OpenDocCommand::InsertPageBreakAfter(AfterBlockArgs {
            after_block_id: "block".to_string(),
        }),
        OpenDocCommand::AddPageBreak,
        OpenDocCommand::InsertTableAfter(InsertTableAfterArgs::Default {
            after_block_id: "block".to_string(),
        }),
        OpenDocCommand::AddTable,
        OpenDocCommand::AddTableRow(AddTableRowArgs {
            table_block_id: "table".to_string(),
            after_row: None,
            text: "Row".to_string(),
        }),
        OpenDocCommand::DeleteTableRow(DeleteTableRowArgs {
            table_block_id: "table".to_string(),
            row_id: "row".to_string(),
        }),
        OpenDocCommand::AddTableCell(AddTableCellArgs {
            table_block_id: "table".to_string(),
            row_id: "row".to_string(),
            after_cell: None,
            text: "Cell".to_string(),
        }),
        OpenDocCommand::DeleteTableCell(DeleteTableCellArgs {
            table_block_id: "table".to_string(),
            row_id: "row".to_string(),
            cell_id: "cell".to_string(),
        }),
        OpenDocCommand::AddCitation,
        OpenDocCommand::InsertCitation(InsertCitationArgs {
            reference_id: "ref".to_string(),
            after_inline_id: None,
            locator: None,
            label: None,
            prefix: None,
            suffix: None,
            suppress_author: false,
        }),
        OpenDocCommand::InsertCitationGroup(InsertCitationGroupArgs {
            items: vec![sample_citation_item()],
            after_inline_id: None,
        }),
        OpenDocCommand::InsertFootnoteCitationGroup(InsertFootnoteCitationGroupArgs {
            footnote_id: "footnote".to_string(),
            items: vec![sample_citation_item()],
        }),
        OpenDocCommand::UpdateCitationGroupItems(UpdateCitationGroupItemsArgs {
            citation_id: "citation".to_string(),
            items: vec![sample_citation_item()],
        }),
        OpenDocCommand::SetCitationStyle(SetCitationStyleArgs {
            style: "apa".to_string(),
            locale: "en-US".to_string(),
        }),
        OpenDocCommand::AddComment(AuthorBodyArgs {
            author: "Author".to_string(),
            body: "Body".to_string(),
        }),
        OpenDocCommand::AddTextRangeComment(TextRangeAuthorBodyArgs {
            start_inline_id: "start".to_string(),
            end_inline_id: "end".to_string(),
            author: "Author".to_string(),
            body: "Body".to_string(),
        }),
        OpenDocCommand::AddBlockComment(BlockAuthorBodyArgs {
            block_id: "block".to_string(),
            author: "Author".to_string(),
            body: "Body".to_string(),
        }),
        OpenDocCommand::AddCommentReply(ThreadAuthorBodyArgs {
            thread_id: "thread".to_string(),
            author: "Author".to_string(),
            body: "Body".to_string(),
        }),
        OpenDocCommand::DeleteCommentThread(ThreadIdArgs {
            thread_id: "thread".to_string(),
        }),
        OpenDocCommand::RestoreCommentThread(ThreadIdArgs {
            thread_id: "thread".to_string(),
        }),
        OpenDocCommand::DeleteComment(ThreadCommentIdArgs {
            thread_id: "thread".to_string(),
            comment_id: "comment".to_string(),
        }),
        OpenDocCommand::RestoreComment(ThreadCommentIdArgs {
            thread_id: "thread".to_string(),
            comment_id: "comment".to_string(),
        }),
        OpenDocCommand::UpdateComment(UpdateCommentArgs {
            thread_id: "thread".to_string(),
            comment_id: "comment".to_string(),
            body: "Body".to_string(),
        }),
        OpenDocCommand::AddSuggestion(AuthorTextArgs {
            author: "Author".to_string(),
            text: "Text".to_string(),
        }),
        OpenDocCommand::AddTextRangeSuggestion(TextRangeAuthorTextArgs {
            start_inline_id: "start".to_string(),
            end_inline_id: "end".to_string(),
            author: "Author".to_string(),
            text: "Text".to_string(),
        }),
        OpenDocCommand::AddBlockSuggestion(BlockAuthorTextArgs {
            block_id: "block".to_string(),
            author: "Author".to_string(),
            text: "Text".to_string(),
        }),
        OpenDocCommand::AddDeleteSuggestion(DeleteSuggestionArgs {
            author: "Author".to_string(),
            inline_id: "inline".to_string(),
        }),
        OpenDocCommand::AddTextRangeDeleteSuggestion(TextRangeAuthorArgs {
            start_inline_id: "start".to_string(),
            end_inline_id: "end".to_string(),
            author: "Author".to_string(),
        }),
        OpenDocCommand::AddFormatSuggestion(FormatSuggestionArgs {
            author: "Author".to_string(),
            inline_id: "inline".to_string(),
            mark_kind: "bold".to_string(),
            value: None,
        }),
        OpenDocCommand::AddTextRangeFormatSuggestion(TextRangeFormatSuggestionArgs {
            start_inline_id: "start".to_string(),
            end_inline_id: "end".to_string(),
            author: "Author".to_string(),
            mark_kind: "bold".to_string(),
            value: None,
        }),
        OpenDocCommand::UpdateSuggestion(UpdateSuggestionArgs {
            suggestion_id: "suggestion".to_string(),
            text: "Text".to_string(),
        }),
        OpenDocCommand::AcceptSuggestion(AcceptSuggestionArgs {
            suggestion_id: "suggestion".to_string(),
            accepted_by: "Reviewer".to_string(),
        }),
        OpenDocCommand::RejectSuggestion(RejectSuggestionArgs {
            suggestion_id: "suggestion".to_string(),
            rejected_by: "Reviewer".to_string(),
        }),
        OpenDocCommand::ApplyEditorInput(EditorInput {
            selection: sample_editor_selection(),
            input_type: "insertText".to_string(),
            data: Some("x".to_string()),
            html: None,
        }),
        OpenDocCommand::ApplyEditorMark(EditorMarkInput {
            selection: sample_editor_selection(),
            mark_kind: "bold".to_string(),
            value: None,
            action: None,
        }),
        OpenDocCommand::AddBibliographyReference(sample_bibliography_metadata()),
        OpenDocCommand::UpdateBibliographyReference(UpdateBibliographyReferenceArgs {
            reference_id: "ref".to_string(),
            title: "Title".to_string(),
            issued: None,
        }),
        OpenDocCommand::UpdateBibliographyReferenceMetadata(
            UpdateBibliographyReferenceMetadataArgs {
                reference_id: "ref".to_string(),
                metadata: sample_bibliography_metadata(),
            },
        ),
        OpenDocCommand::DeleteBibliographyReference(ReferenceIdArgs {
            reference_id: "ref".to_string(),
        }),
        OpenDocCommand::RestoreBibliographyReference(ReferenceIdArgs {
            reference_id: "ref".to_string(),
        }),
        OpenDocCommand::DeleteCitationGroup(CitationIdArgs {
            citation_id: "citation".to_string(),
        }),
        OpenDocCommand::RestoreCitationGroup(CitationIdArgs {
            citation_id: "citation".to_string(),
        }),
        OpenDocCommand::UpdateInlineText(InlineTextArgs {
            inline_id: "inline".to_string(),
            text: "Text".to_string(),
        }),
        OpenDocCommand::UpdateInlineEquationSource(InlineSourceArgs {
            inline_id: "inline".to_string(),
            source: "x + y".to_string(),
        }),
        OpenDocCommand::UpdateMentionLabel(InlineLabelArgs {
            inline_id: "inline".to_string(),
            label: "Ada".to_string(),
        }),
        OpenDocCommand::UpdateLinkHref(InlineHrefArgs {
            inline_id: "inline".to_string(),
            href: "https://example.test".to_string(),
        }),
        OpenDocCommand::InsertInlineText(InsertInlineTextArgs {
            block_id: "block".to_string(),
            after_inline_id: None,
            text: "Text".to_string(),
        }),
        OpenDocCommand::DeleteInline(InlineIdArgs {
            inline_id: "inline".to_string(),
        }),
        OpenDocCommand::AddTextMark(TextMarkArgs {
            inline_id: "inline".to_string(),
            mark_kind: "bold".to_string(),
            value: None,
        }),
        OpenDocCommand::AddTextMarkRange(TextMarkRangeArgs {
            start_inline_id: "start".to_string(),
            end_inline_id: "end".to_string(),
            mark_kind: "bold".to_string(),
            value: None,
        }),
        OpenDocCommand::RemoveTextMark(TextMarkArgs {
            inline_id: "inline".to_string(),
            mark_kind: "bold".to_string(),
            value: None,
        }),
        OpenDocCommand::RemoveTextMarkRange(TextMarkRangeArgs {
            start_inline_id: "start".to_string(),
            end_inline_id: "end".to_string(),
            mark_kind: "bold".to_string(),
            value: None,
        }),
        OpenDocCommand::UpdateBlockEquationSource(BlockSourceArgs {
            block_id: "block".to_string(),
            source: "x + y".to_string(),
        }),
        OpenDocCommand::UpdateImageAltText(BlockAltTextArgs {
            block_id: "block".to_string(),
            alt_text: "Alt".to_string(),
        }),
        OpenDocCommand::UpdateImageBlobHash(BlockBlobHashArgs {
            block_id: "block".to_string(),
            blob_hash: "sha256:abc".to_string(),
        }),
        OpenDocCommand::SetSpreadsheetWorkbookMetadata(SpreadsheetWorkbookMetadataArgs {
            title: "Workbook".to_string(),
            locale: "en-US".to_string(),
            timezone: "UTC".to_string(),
        }),
        OpenDocCommand::SetSpreadsheetCell(SpreadsheetCellArgs {
            address: "A1".to_string(),
            value: "1".to_string(),
        }),
        OpenDocCommand::SetSpreadsheetCells(SpreadsheetCellEditsArgs {
            cells: vec![("A1".to_string(), "1".to_string())],
        }),
        OpenDocCommand::DescribeSpreadsheetSelection(SpreadsheetSelectionArgs {
            sheet_id: "sheet".to_string(),
            anchor: "A1".to_string(),
            focus: "B2".to_string(),
        }),
        OpenDocCommand::ReduceSpreadsheetSelection(SpreadsheetSelectionActionArgs {
            sheet_id: "sheet".to_string(),
            anchor: "A1".to_string(),
            focus: "B2".to_string(),
            action: "move".to_string(),
            value: "right".to_string(),
            extend: false,
        }),
        OpenDocCommand::CopySpreadsheetSelectionTsv(SpreadsheetSelectionArgs {
            sheet_id: "sheet".to_string(),
            anchor: "A1".to_string(),
            focus: "B2".to_string(),
        }),
        OpenDocCommand::PasteSpreadsheetTsv(SpreadsheetPasteTsvArgs {
            sheet_id: "sheet".to_string(),
            origin: "A1".to_string(),
            text: "1\t2".to_string(),
            source_origin: Some("B2".to_string()),
        }),
        OpenDocCommand::ClearSpreadsheetSelection(SpreadsheetSelectionArgs {
            sheet_id: "sheet".to_string(),
            anchor: "A1".to_string(),
            focus: "B2".to_string(),
        }),
        OpenDocCommand::SetSpreadsheetSelectionFormat(SpreadsheetSelectionFormatArgs {
            sheet_id: "sheet".to_string(),
            anchor: "A1".to_string(),
            focus: "B2".to_string(),
            property: "number_format".to_string(),
            value: "0.00".to_string(),
        }),
        OpenDocCommand::AddSpreadsheetRowAfterSelection(SpreadsheetSelectionArgs {
            sheet_id: "sheet".to_string(),
            anchor: "A1".to_string(),
            focus: "B2".to_string(),
        }),
        OpenDocCommand::AddSpreadsheetColumnAfterSelection(SpreadsheetSelectionArgs {
            sheet_id: "sheet".to_string(),
            anchor: "A1".to_string(),
            focus: "B2".to_string(),
        }),
        OpenDocCommand::DeleteSpreadsheetSelectionRow(SpreadsheetSelectionArgs {
            sheet_id: "sheet".to_string(),
            anchor: "A1".to_string(),
            focus: "B2".to_string(),
        }),
        OpenDocCommand::DeleteSpreadsheetSelectionColumn(SpreadsheetSelectionArgs {
            sheet_id: "sheet".to_string(),
            anchor: "A1".to_string(),
            focus: "B2".to_string(),
        }),
        OpenDocCommand::MergeSpreadsheetSelection(SpreadsheetSelectionArgs {
            sheet_id: "sheet".to_string(),
            anchor: "A1".to_string(),
            focus: "B2".to_string(),
        }),
        OpenDocCommand::FreezeSpreadsheetSelection(SpreadsheetSelectionArgs {
            sheet_id: "sheet".to_string(),
            anchor: "A1".to_string(),
            focus: "B2".to_string(),
        }),
        OpenDocCommand::SetSpreadsheetSelectionFilter(SpreadsheetSelectionArgs {
            sheet_id: "sheet".to_string(),
            anchor: "A1".to_string(),
            focus: "B2".to_string(),
        }),
        OpenDocCommand::AddSpreadsheetSheet(SheetTitleArgs {
            title: "Sheet".to_string(),
        }),
        OpenDocCommand::RenameSpreadsheetSheet(SheetRenameArgs {
            sheet_id: "sheet".to_string(),
            title: "Sheet".to_string(),
        }),
        OpenDocCommand::DeleteSpreadsheetSheet(SheetIdArgs {
            sheet_id: "sheet".to_string(),
        }),
        OpenDocCommand::RestoreSpreadsheetSheet(SheetIdArgs {
            sheet_id: "sheet".to_string(),
        }),
        OpenDocCommand::AddSpreadsheetRow(SheetRowArgs {
            sheet_id: "sheet".to_string(),
            row: "1".to_string(),
        }),
        OpenDocCommand::DeleteSpreadsheetRow(SheetRowArgs {
            sheet_id: "sheet".to_string(),
            row: "1".to_string(),
        }),
        OpenDocCommand::RestoreSpreadsheetRow(SheetRowArgs {
            sheet_id: "sheet".to_string(),
            row: "1".to_string(),
        }),
        OpenDocCommand::AddSpreadsheetColumn(SheetColumnArgs {
            sheet_id: "sheet".to_string(),
            column: "A".to_string(),
        }),
        OpenDocCommand::DeleteSpreadsheetColumn(SheetColumnArgs {
            sheet_id: "sheet".to_string(),
            column: "A".to_string(),
        }),
        OpenDocCommand::RestoreSpreadsheetColumn(SheetColumnArgs {
            sheet_id: "sheet".to_string(),
            column: "A".to_string(),
        }),
        OpenDocCommand::AddSpreadsheetCellComment(CellCommentCreateArgs {
            sheet_id: "sheet".to_string(),
            address: "A1".to_string(),
            author: "Author".to_string(),
            body: "Body".to_string(),
        }),
        OpenDocCommand::UpdateSpreadsheetCellComment(CellCommentUpdateArgs {
            comment_id: "comment".to_string(),
            body: "Body".to_string(),
        }),
        OpenDocCommand::DeleteSpreadsheetCellComment(CellCommentIdArgs {
            comment_id: "comment".to_string(),
        }),
        OpenDocCommand::RestoreSpreadsheetCellComment(CellCommentIdArgs {
            comment_id: "comment".to_string(),
        }),
        OpenDocCommand::SetSpreadsheetFrozenAxes(FrozenAxesArgs {
            sheet_id: "sheet".to_string(),
            frozen_rows: 1,
            frozen_columns: 1,
        }),
        OpenDocCommand::SetSpreadsheetCellValidation(CellValidationArgs {
            sheet_id: "sheet".to_string(),
            address: "A1".to_string(),
            kind: "list".to_string(),
            values: vec!["x".to_string()],
            strict: true,
        }),
        OpenDocCommand::ClearSpreadsheetCellValidation(SheetAddressArgs {
            sheet_id: "sheet".to_string(),
            address: "A1".to_string(),
        }),
        OpenDocCommand::RestoreSpreadsheetCellValidation(SheetAddressArgs {
            sheet_id: "sheet".to_string(),
            address: "A1".to_string(),
        }),
        OpenDocCommand::MergeSpreadsheetCells(SheetRangeArgs {
            sheet_id: "sheet".to_string(),
            range: "A1:B2".to_string(),
        }),
        OpenDocCommand::UnmergeSpreadsheetCells(SheetRangeArgs {
            sheet_id: "sheet".to_string(),
            range: "A1:B2".to_string(),
        }),
        OpenDocCommand::RestoreSpreadsheetMerge(SheetRangeArgs {
            sheet_id: "sheet".to_string(),
            range: "A1:B2".to_string(),
        }),
        OpenDocCommand::SetSpreadsheetBasicFilter(SheetRangeArgs {
            sheet_id: "sheet".to_string(),
            range: "A1:B2".to_string(),
        }),
        OpenDocCommand::SetSpreadsheetBasicFilterOptions(FilterOptionsArgs {
            sheet_id: "sheet".to_string(),
            criteria: vec![SheetFilterCriterion {
                column: "A".to_string(),
                condition: "text_contains".to_string(),
                value: "x".to_string(),
            }],
            sort_specs: vec![SheetFilterSortSpec {
                column: "A".to_string(),
                descending: false,
            }],
        }),
        OpenDocCommand::ClearSpreadsheetBasicFilter(SheetIdArgs {
            sheet_id: "sheet".to_string(),
        }),
        OpenDocCommand::RestoreSpreadsheetBasicFilter(SheetIdArgs {
            sheet_id: "sheet".to_string(),
        }),
        OpenDocCommand::AddSpreadsheetProtectedRange(ProtectedRangeArgs {
            sheet_id: "sheet".to_string(),
            range: "A1:B2".to_string(),
            description: "Locked".to_string(),
            warning_only: true,
        }),
        OpenDocCommand::UpdateSpreadsheetProtectedRange(ProtectedRangeArgs {
            sheet_id: "sheet".to_string(),
            range: "A1:B2".to_string(),
            description: "Locked".to_string(),
            warning_only: true,
        }),
        OpenDocCommand::DeleteSpreadsheetProtectedRange(SheetRangeArgs {
            sheet_id: "sheet".to_string(),
            range: "A1:B2".to_string(),
        }),
        OpenDocCommand::RestoreSpreadsheetProtectedRange(SheetRangeArgs {
            sheet_id: "sheet".to_string(),
            range: "A1:B2".to_string(),
        }),
        OpenDocCommand::SetSpreadsheetCellInSheet(SheetCellArgs {
            sheet_id: "sheet".to_string(),
            address: "A1".to_string(),
            value: "1".to_string(),
        }),
        OpenDocCommand::SetSpreadsheetCellsInSheet(SheetCellEditsArgs {
            sheet_id: "sheet".to_string(),
            cells: vec![("A1".to_string(), "1".to_string())],
        }),
        OpenDocCommand::SetSpreadsheetCellFormat(CellFormatArgs {
            sheet_id: "sheet".to_string(),
            address: "A1".to_string(),
            property: "number_format".to_string(),
            value: "0.00".to_string(),
        }),
        OpenDocCommand::CopySpreadsheetRange(CopyRangeArgs {
            sheet_id: "sheet".to_string(),
            source_range: "A1:B2".to_string(),
            target_address: "C1".to_string(),
        }),
        OpenDocCommand::AddSpreadsheetNamedRange(NamedRangeArgs {
            sheet_id: "sheet".to_string(),
            name: "Named".to_string(),
            range: "A1:B2".to_string(),
        }),
        OpenDocCommand::UpdateSpreadsheetNamedRange(NamedRangeArgs {
            sheet_id: "sheet".to_string(),
            name: "Named".to_string(),
            range: "A1:B2".to_string(),
        }),
        OpenDocCommand::DeleteSpreadsheetNamedRange(NamedRangeNameArgs {
            name: "Named".to_string(),
        }),
        OpenDocCommand::RestoreSpreadsheetNamedRange(NamedRangeNameArgs {
            name: "Named".to_string(),
        }),
    ];

    for command in commands {
        assert!(
            crate::command_spec(command.name()).is_some(),
            "missing command metadata for {}",
            command.name()
        );
    }
}

#[test]
fn parse_migrated_keeps_unknown_commands_for_legacy_dispatch() {
    let parsed = parse_json_command("__unsupported_command__", &json!({}))
        .expect("unknown commands should not fail typed parsing");
    assert!(parsed.is_none());
}

#[test]
fn every_rust_metadata_command_has_typed_parser() {
    for spec in crate::COMMANDS {
        let args = sample_args_for_spec(spec.args);
        let command = parse_json_command(spec.name, &args)
            .unwrap_or_else(|err| panic!("failed to parse {}: {err}", spec.name))
            .unwrap_or_else(|| panic!("missing typed parser for {}", spec.name));
        assert_eq!(command.name(), spec.name);
    }
}

fn sample_citation_item() -> AppCitationItem {
    AppCitationItem {
        reference_id: "ref".to_string(),
        locator: None,
        label: None,
        prefix: None,
        suffix: None,
        suppress_author: false,
    }
}

fn sample_editor_selection() -> crate::EditorSelection {
    crate::EditorSelection::collapsed(crate::EditorPosition {
        block_id: "block".to_string(),
        inline_id: None,
        offset: 0,
    })
}

fn sample_bibliography_metadata() -> BibliographyReferenceMetadataArgs {
    BibliographyReferenceMetadataArgs {
        title: "Title".to_string(),
        authors: vec!["Author".to_string()],
        issued: None,
        doi: None,
        url: None,
    }
}

fn sample_args_for_spec(args: &[CommandArg]) -> Value {
    let mut object = serde_json::Map::new();
    for arg in args {
        object.insert(arg.name.to_string(), sample_arg_value(arg));
    }
    Value::Object(object)
}

fn sample_arg_value(arg: &CommandArg) -> Value {
    if arg.name == "mode" {
        return json!("browser-local");
    }
    match arg.ty {
        CommandArgType::String | CommandArgType::RuntimeMode => json!(sample_string(arg.name)),
        CommandArgType::NullableString | CommandArgType::NullableBoolean => Value::Null,
        CommandArgType::Number => json!(1),
        CommandArgType::Boolean => json!(false),
        CommandArgType::StringArray => json!(["value"]),
        CommandArgType::NumberArray => json!([1]),
        CommandArgType::Object => json!({}),
        CommandArgType::ObjectArray => sample_object_array_arg(arg.name),
        CommandArgType::EditorSelection => json!({
            "anchor": { "block_id": "block", "inline_id": null, "offset": 0 },
            "focus": { "block_id": "block", "inline_id": null, "offset": 0 }
        }),
        CommandArgType::CitationItems => json!([{
            "reference_id": "ref",
            "locator": null,
            "label": null,
            "prefix": null,
            "suffix": null,
            "suppress_author": false
        }]),
        CommandArgType::SpreadsheetCellEdit => json!({ "address": "A1", "value": "1" }),
        CommandArgType::SpreadsheetCellEdits => json!([{ "address": "A1", "value": "1" }]),
    }
}

fn sample_object_array_arg(name: &str) -> Value {
    match name {
        "criteria" => json!([{ "column": "A", "condition": "text_contains", "value": "x" }]),
        "sortSpecs" => json!([{ "column": "A", "descending": false }]),
        _ => json!([]),
    }
}

fn sample_string(name: &str) -> &'static str {
    match name {
        "path" => "/tmp/opendoc-test",
        "namespace" => "namespace",
        "documentUuid" => "01890f65-3b9a-7cc2-a4bd-9bd0b62f5c00",
        "doi" => "10.1234/example",
        "commandName" => "get_document",
        "mediaType" => "text/plain",
        "blobHash" => "sha256:abc",
        "archiveLocator" => "archive://blob",
        "restoreHint" => "restore",
        "signer" => "signer",
        "privateKeyPem" => "private-key",
        "afterBlockId" | "blockId" => "block",
        "afterInlineId" | "inlineId" | "startInlineId" | "endInlineId" => "inline",
        "tableBlockId" => "table",
        "afterRow" | "rowId" | "row" => "row",
        "afterCell" | "cellId" => "cell",
        "referenceId" => "ref",
        "citationId" => "citation",
        "footnoteId" => "footnote",
        "threadId" => "thread",
        "commentId" => "comment",
        "suggestionId" => "suggestion",
        "acceptedBy" | "rejectedBy" | "author" => "Author",
        "markKind" => "bold",
        "listKind" => "bullet",
        "alignment" => "center",
        "direction" => "ltr",
        "spacingMode" => "multiple",
        "key" => "alignment",
        "style" => "apa",
        "locale" => "en-US",
        "timezone" => "UTC",
        "sheetId" => "sheet",
        "address" | "targetAddress" => "A1",
        "sourceRange" | "range" => "A1:B2",
        "column" => "A",
        "property" => "number_format",
        "kind" => "list",
        "input_type" => "insertText",
        "html" => "<p>x</p>",
        _ => "value",
    }
}
