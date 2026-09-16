use crate::command_args::*;
use crate::{
    command_spec, CommandSpec, EditorInput, EditorMarkInput, EditorSelection, OpenDocRuntimeProfile,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OpenDocCommand {
    CreateDocument(CreateDocumentArgs),
    CloseDocument,
    UndoCurrentEdit,
    RedoCurrentEdit,
    GetDocument,
    GetAuditView,
    SetDocumentTitle(SetDocumentTitleArgs),
    SetDocumentLocale(SetDocumentLocaleArgs),
    SetBookmark(SetBookmarkArgs),
    DeleteBookmark(BookmarkIdArgs),
    AddParagraph(AddParagraphArgs),
    RenderDocumentHtml,
    RenderSuggestionPreviewHtml(SuggestionPreviewArgs),
    LayoutDocument,
    GetRuntimeProfile(OpenDocRuntimeProfile),
    GetRuntimeSession(GetRuntimeSessionArgs),
    AuthorizeRuntimeCommand(AuthorizeRuntimeCommandArgs),
    CreateRuntimeShareInvite(CreateRuntimeShareInviteArgs),
    RelayRuntimeSync(RelayRuntimeSyncArgs),
    ResolveRuntimeDocumentLookup(ResolveRuntimeDocumentLookupArgs),
    ImportGoogleDocsJson(ImportGoogleDocsJsonArgs),
    ImportDocOrDocxPath(ImportDocOrDocxPathArgs),
    ExportGoogleDocsJson,
    ExportDocx,
    ExportOdt,
    ExportPdf,
    ExportHtml,
    ExportText,
    ExportImageBlob(ExportImageBlobArgs),
    ImportGoogleSheetsJson(ImportGoogleSheetsJsonArgs),
    ExportGoogleSheetsJson,
    RenderWorkbookHtml(RenderWorkbookHtmlArgs),
    ImportDocxBase64(ImportDocxBase64Args),
    AddBinaryBlob(AddBinaryBlobArgs),
    SimulateShallowClone,
    UpdateBinaryBlobMetadata(UpdateBinaryBlobMetadataArgs),
    DeleteBinaryBlob(DeleteBinaryBlobArgs),
    RestoreBinaryBlob(RestoreBinaryBlobArgs),
    RecordBlobArchiveTombstone(RecordBlobArchiveTombstoneArgs),
    AddImageBlock(AddImageBlockArgs),
    InsertImageBlockAfter(InsertImageBlockAfterArgs),
    SignBlobWithOpenSshPrivateKey(SignBlobWithOpenSshPrivateKeyArgs),
    SignFastqBlobWithOpenSshPrivateKey(SignFastqBlobWithOpenSshPrivateKeyArgs),
    SignImagePixelsBlobWithOpenSshPrivateKey(SignImagePixelsBlobWithOpenSshPrivateKeyArgs),
    SignWithOpenSshPrivateKey(SignWithOpenSshPrivateKeyArgs),
    SignCurrentRepositoryVersionWithOpenSshPrivateKey(
        SignCurrentRepositoryVersionWithOpenSshPrivateKeyArgs,
    ),
    VerifyCurrentSignature(VerifyCurrentSignatureArgs),
    VerifyCurrentSignatures,
    SaveLocalRepository(RepositoryPathArgs),
    SaveLocalRepositoryOrCandidate(RepositoryPathArgs),
    SaveFlatRepository(RepositoryNamespaceArgs),
    SaveFlatRepositoryOrCandidate(RepositoryNamespaceArgs),
    SaveOpenDalFsRepository(RepositoryNamespaceArgs),
    SaveOpenDalFsRepositoryOrCandidate(RepositoryNamespaceArgs),
    AutosaveCurrentRepository,
    ListDocumentVersions(ListDocumentVersionsArgs),
    OpenDocumentAtVersion(DocumentVersionArgs),
    DiffDocumentVersions(DiffDocumentVersionsArgs),
    NameDocumentVersion(NameDocumentVersionArgs),
    RestoreDocumentVersion(DocumentVersionArgs),
    CompactLocalRepository(CompactLocalRepositoryArgs),
    OpenLocalRepository(RepositoryDocumentArgs),
    ScanLocalRepository(RepositoryPathArgs),
    OpenFlatRepository(RepositoryNamespaceDocumentArgs),
    OpenOpenDalFsRepository(RepositoryNamespaceDocumentArgs),
    MergeLocalRepositoryCandidates(RepositoryDocumentArgs),
    MergeFlatRepositoryCandidates(RepositoryNamespaceDocumentArgs),
    MergeOpenDalFsRepositoryCandidates(RepositoryNamespaceDocumentArgs),
    OpenLocalRepositoryByDoi(RepositoryDoiArgs),
    OpenFlatRepositoryByDoi(RepositoryNamespaceDoiArgs),
    OpenOpenDalFsRepositoryByDoi(RepositoryNamespaceDoiArgs),
    SetDocumentDoi(SetDocumentDoiArgs),
    InsertParagraphAfter(InsertParagraphAfterArgs),
    SplitParagraphAtInline(SplitParagraphAtInlineArgs),
    SplitParagraphAtTextOffset(SplitParagraphAtTextOffsetArgs),
    JoinParagraphWithPrevious(BlockIdArgs),
    DeleteBlock(BlockIdArgs),
    MoveBlock(MoveBlockArgs),
    SetBlockTextStyle(SetBlockTextStyleArgs),
    SetEditorSelectionBlockStyle(SetEditorSelectionBlockStyleArgs),
    SetBlockAlignment(SetBlockNamedValueArgs),
    SetEditorSelectionBlockAlignment(SetEditorSelectionBlockNamedValueArgs),
    SetBlockIndentStart(SetBlockLengthArgs),
    SetEditorSelectionBlockIndentStart(SetEditorSelectionBlockLengthArgs),
    SetBlockIndentEnd(SetBlockLengthArgs),
    SetEditorSelectionBlockIndentEnd(SetEditorSelectionBlockLengthArgs),
    SetBlockIndentFirstLine(SetBlockLengthArgs),
    SetEditorSelectionBlockIndentFirstLine(SetEditorSelectionBlockLengthArgs),
    SetBlockLineSpacing(SetBlockLineSpacingArgs),
    SetEditorSelectionBlockLineSpacing(SetEditorSelectionBlockLineSpacingArgs),
    SetBlockSpaceBefore(SetBlockLengthArgs),
    SetEditorSelectionBlockSpaceBefore(SetEditorSelectionBlockLengthArgs),
    SetBlockSpaceAfter(SetBlockLengthArgs),
    SetEditorSelectionBlockSpaceAfter(SetEditorSelectionBlockLengthArgs),
    SetBlockDirection(SetBlockNamedValueArgs),
    SetEditorSelectionBlockDirection(SetEditorSelectionBlockNamedValueArgs),
    SetBlockKeepWithNext(SetBlockBoolArgs),
    SetEditorSelectionBlockKeepWithNext(SetEditorSelectionBlockBoolArgs),
    SetBlockBackground(SetBlockColorArgs),
    SetEditorSelectionBlockBackground(SetEditorSelectionBlockColorArgs),
    SetBlockBorder(SetBlockBorderArgs),
    SetEditorSelectionBlockBorder(SetEditorSelectionBlockBorderArgs),
    SetPageSetup(SetPageSetupArgs),
    SetPageOrientation(SetPageOrientationArgs),
    SetPageFurniture(SetPageFurnitureArgs),
    SetPageFurnitureHtml(SetPageFurnitureHtmlArgs),
    ClearPageFurniture(PageFurnitureSlotArgs),
    ClearPageFurnitureOverride(PageFurnitureSlotArgs),
    ClearBlockProperty(ClearBlockPropertyArgs),
    ClearEditorSelectionBlockProperty(ClearEditorSelectionBlockPropertyArgs),
    SetListItemChecked(SetListItemCheckedArgs),
    FindInDocument(FindInDocumentArgs),
    ReplaceMatchInDocument(ReplaceMatchInDocumentArgs),
    ReplaceAllInDocument(ReplaceAllInDocumentArgs),
    AdjustEditorSelectionIndent(AdjustEditorSelectionListIndentArgs),
    AddHeading(AddHeadingArgs),
    UpdateHeadingLevel(UpdateHeadingLevelArgs),
    AddLink(AddLinkArgs),
    InsertLinkAfter(InsertLinkAfterArgs),
    AddMention(AddMentionArgs),
    InsertMentionAfter(InsertMentionAfterArgs),
    InsertDateChipAfter(InsertDateChipAfterArgs),
    AddFootnoteRef,
    AddEndnoteRef,
    InsertFootnoteRefAfter(InsertFootnoteRefAfterArgs),
    InsertEndnoteRefAfter(InsertFootnoteRefAfterArgs),
    UpdateFootnoteBody(UpdateFootnoteBodyArgs),
    AddEquation(AddEquationArgs),
    InsertEquationAfter(InsertEquationAfterArgs),
    AddEquationBlock(AddEquationArgs),
    InsertEquationBlockAfter(InsertEquationBlockAfterArgs),
    AddListItem(AddListItemArgs),
    InsertListItemAfter(InsertListItemAfterArgs),
    UpdateListItem(UpdateListItemArgs),
    SetOrderedListStart(SetOrderedListStartArgs),
    SetOrderedListFormat(SetOrderedListFormatArgs),
    SetBulletListMarker(SetBulletListMarkerArgs),
    AdjustEditorSelectionListIndent(AdjustEditorSelectionListIndentArgs),
    InsertPageBreakAfter(AfterBlockArgs),
    AddPageBreak,
    InsertHorizontalRuleAfter(AfterBlockArgs),
    AddHorizontalRule,
    InsertTableOfContentsAfter(AfterBlockArgs),
    InsertBibliographyAfter(AfterBlockArgs),
    InsertTableAfter(InsertTableAfterArgs),
    AddTable,
    AddTableRow(AddTableRowArgs),
    DeleteTableRow(DeleteTableRowArgs),
    AddTableCell(AddTableCellArgs),
    DeleteTableCell(DeleteTableCellArgs),
    InsertTableColumn(InsertTableColumnArgs),
    DeleteTableColumn(TableColumnArgs),
    SetTableColumnWidth(SetTableColumnWidthArgs),
    ClearTableColumnWidth(TableColumnArgs),
    SetTableRowHeight(SetTableRowHeightArgs),
    ClearTableRowHeight(TableRowArgs),
    SetTableRowHeader(SetTableRowHeaderArgs),
    SortTableRows(SortTableRowsArgs),
    SetTableBorder(SetTableBorderArgs),
    ClearTableBorder(TableArgs),
    SetTableAlignment(SetTableAlignmentArgs),
    ClearTableAlignment(TableArgs),
    MergeTableCells(MergeTableCellsArgs),
    SplitTableCell(TableCellArgs),
    SetTableCellBackground(SetTableCellBackgroundArgs),
    SetTableCellBorder(SetTableCellBorderArgs),
    SetTableCellVerticalAlignment(SetTableCellVerticalAlignmentArgs),
    SetTableCellRowHeader(SetTableCellRowHeaderArgs),
    SetTableCellPadding(SetTableCellPaddingArgs),
    ClearTableCellProperty(ClearTableCellPropertyArgs),
    AddCitation,
    InsertCitation(InsertCitationArgs),
    InsertCitationGroup(InsertCitationGroupArgs),
    InsertFootnoteCitationGroup(InsertFootnoteCitationGroupArgs),
    InsertFootnoteCitationAfter(InsertFootnoteCitationAfterArgs),
    UpdateCitationGroupItems(UpdateCitationGroupItemsArgs),
    SetCitationStyle(SetCitationStyleArgs),
    AddComment(AuthorBodyArgs),
    AddTextRangeComment(TextRangeAuthorBodyArgs),
    AddBlockComment(BlockAuthorBodyArgs),
    AddCommentReply(ThreadAuthorBodyArgs),
    ResolveCommentThread(ResolveCommentThreadArgs),
    ReopenCommentThread(ThreadIdArgs),
    SetCommentThreadAction(SetCommentThreadActionArgs),
    SetCommentThreadReaction(SetCommentThreadReactionArgs),
    DeleteCommentThread(ThreadIdArgs),
    RestoreCommentThread(ThreadIdArgs),
    DeleteComment(ThreadCommentIdArgs),
    RestoreComment(ThreadCommentIdArgs),
    UpdateComment(UpdateCommentArgs),
    AddSuggestion(AuthorTextArgs),
    AddTextRangeSuggestion(TextRangeAuthorTextArgs),
    AddBlockSuggestion(BlockAuthorTextArgs),
    AddBlockDeleteSuggestion(BlockDeleteSuggestionArgs),
    AddBlockInsertSuggestion(BlockInsertSuggestionArgs),
    AddBlockReplaceSuggestion(BlockReplaceSuggestionArgs),
    AddDeleteSuggestion(DeleteSuggestionArgs),
    AddTextRangeDeleteSuggestion(TextRangeAuthorArgs),
    AddFormatSuggestion(FormatSuggestionArgs),
    AddTextRangeFormatSuggestion(TextRangeFormatSuggestionArgs),
    AddTextRangeFormatRemovalSuggestion(TextRangeFormatRemovalSuggestionArgs),
    AddTextRangeFormatReplacementSuggestion(TextRangeFormatReplacementSuggestionArgs),
    AddLinkChangeSuggestion(LinkChangeSuggestionArgs),
    AddParagraphStyleSuggestion(ParagraphStyleSuggestionArgs),
    UpdateSuggestion(UpdateSuggestionArgs),
    AcceptSuggestion(AcceptSuggestionArgs),
    AcceptAllSuggestions(AcceptAllSuggestionsArgs),
    RejectSuggestion(RejectSuggestionArgs),
    RejectAllSuggestions(RejectAllSuggestionsArgs),
    DescribeEditorSelection(EditorSelection),
    SelectAllEditorContent,
    ApplyEditorInput(EditorInput),
    ApplyEditorMark(EditorMarkInput),
    ImportBibtex(ImportBibtexArgs),
    AddBibliographyReference(BibliographyReferenceMetadataArgs),
    UpdateBibliographyReference(UpdateBibliographyReferenceArgs),
    UpdateBibliographyReferenceMetadata(UpdateBibliographyReferenceMetadataArgs),
    DeleteBibliographyReference(ReferenceIdArgs),
    RestoreBibliographyReference(ReferenceIdArgs),
    DeleteCitationGroup(CitationIdArgs),
    RestoreCitationGroup(CitationIdArgs),
    UpdateInlineText(InlineTextArgs),
    UpdateInlineEquationSource(InlineSourceArgs),
    UpdateMentionLabel(InlineLabelArgs),
    SelectDropdownOption(SelectDropdownOptionArgs),
    UpdateDateChip(UpdateDateChipArgs),
    UpdateLinkHref(InlineHrefArgs),
    InsertInlineText(InsertInlineTextArgs),
    DeleteInline(InlineIdArgs),
    AddTextMark(TextMarkArgs),
    AddTextMarkRange(TextMarkRangeArgs),
    RemoveTextMark(TextMarkArgs),
    RemoveTextMarkRange(TextMarkRangeArgs),
    UpdateBlockEquationSource(BlockSourceArgs),
    UpdateImageAltText(BlockAltTextArgs),
    UpdateImageBlobHash(BlockBlobHashArgs),
    SetImageBlockWidth(ImageBlockLengthArgs),
    SetImageBlockHeight(ImageBlockLengthArgs),
    SetImageBlockSize(ImageBlockSizeArgs),
    ClearImageBlockSize(BlockIdArgs),
    SetImageBlockPlacement(ImageBlockPlacementArgs),
    SetImageBlockWrapClearance(ImageBlockWrapClearanceArgs),
    SetImageBlockPositioned(ImageBlockPositionedArgs),
    ClearImageBlockPositioned(BlockIdArgs),
    SetImageBlockEffects(ImageBlockEffectsArgs),
    SetImageBlockCrop(ImageBlockCropArgs),
    SetImageBlockCaption(ImageBlockCaptionArgs),
    SetImageBlockBorder(ImageBlockBorderArgs),
    DescribeSpreadsheetSelection(SpreadsheetSelectionArgs),
    ReduceSpreadsheetSelection(SpreadsheetSelectionActionArgs),
    CopySpreadsheetSelectionTsv(SpreadsheetSelectionArgs),
    PasteSpreadsheetTsv(SpreadsheetPasteTsvArgs),
    ClearSpreadsheetSelection(SpreadsheetSelectionArgs),
    SetSpreadsheetSelectionFormat(SpreadsheetSelectionFormatArgs),
    AddSpreadsheetRowAfterSelection(SpreadsheetSelectionArgs),
    AddSpreadsheetColumnAfterSelection(SpreadsheetSelectionArgs),
    DeleteSpreadsheetSelectionRow(SpreadsheetSelectionArgs),
    DeleteSpreadsheetSelectionColumn(SpreadsheetSelectionArgs),
    MergeSpreadsheetSelection(SpreadsheetSelectionArgs),
    FreezeSpreadsheetSelection(SpreadsheetSelectionArgs),
    SetSpreadsheetSelectionFilter(SpreadsheetSelectionArgs),
    SetSpreadsheetWorkbookMetadata(SpreadsheetWorkbookMetadataArgs),
    SetSpreadsheetCell(SpreadsheetCellArgs),
    SetSpreadsheetCells(SpreadsheetCellEditsArgs),
    AddSpreadsheetSheet(SheetTitleArgs),
    RenameSpreadsheetSheet(SheetRenameArgs),
    DeleteSpreadsheetSheet(SheetIdArgs),
    RestoreSpreadsheetSheet(SheetIdArgs),
    AddSpreadsheetRow(SheetRowArgs),
    DeleteSpreadsheetRow(SheetRowArgs),
    RestoreSpreadsheetRow(SheetRowArgs),
    AddSpreadsheetColumn(SheetColumnArgs),
    DeleteSpreadsheetColumn(SheetColumnArgs),
    RestoreSpreadsheetColumn(SheetColumnArgs),
    AddSpreadsheetCellComment(CellCommentCreateArgs),
    UpdateSpreadsheetCellComment(CellCommentUpdateArgs),
    DeleteSpreadsheetCellComment(CellCommentIdArgs),
    RestoreSpreadsheetCellComment(CellCommentIdArgs),
    SetSpreadsheetFrozenAxes(FrozenAxesArgs),
    SetSpreadsheetCellValidation(CellValidationArgs),
    ClearSpreadsheetCellValidation(SheetAddressArgs),
    RestoreSpreadsheetCellValidation(SheetAddressArgs),
    MergeSpreadsheetCells(SheetRangeArgs),
    UnmergeSpreadsheetCells(SheetRangeArgs),
    RestoreSpreadsheetMerge(SheetRangeArgs),
    SetSpreadsheetBasicFilter(SheetRangeArgs),
    SetSpreadsheetPrintArea(SheetRangeArgs),
    ClearSpreadsheetPrintArea(SheetIdArgs),
    SetSpreadsheetPrintOrientation(SheetPrintOrientationArgs),
    SetSpreadsheetBasicFilterOptions(FilterOptionsArgs),
    ClearSpreadsheetBasicFilter(SheetIdArgs),
    RestoreSpreadsheetBasicFilter(SheetIdArgs),
    AddSpreadsheetProtectedRange(ProtectedRangeArgs),
    UpdateSpreadsheetProtectedRange(ProtectedRangeArgs),
    DeleteSpreadsheetProtectedRange(SheetRangeArgs),
    RestoreSpreadsheetProtectedRange(SheetRangeArgs),
    SetSpreadsheetCellInSheet(SheetCellArgs),
    SetSpreadsheetCellsInSheet(SheetCellEditsArgs),
    SetSpreadsheetCellFormat(CellFormatArgs),
    SetSpreadsheetRowHeight(SheetRowHeightArgs),
    SetSpreadsheetColumnWidth(SheetColumnWidthArgs),
    SetSpreadsheetSelectionRowsHidden(SpreadsheetSelectionHiddenArgs),
    SetSpreadsheetSelectionColumnsHidden(SpreadsheetSelectionHiddenArgs),
    CopySpreadsheetRange(CopyRangeArgs),
    SortSpreadsheetRange(SortRangeArgs),
    FillSpreadsheetRange(FillRangeArgs),
    ImportSpreadsheetCsv(ImportSpreadsheetCsvArgs),
    ExportSpreadsheetCsv(ExportSpreadsheetCsvArgs),
    ImportSpreadsheetXlsx(ImportSpreadsheetXlsxArgs),
    ExportSpreadsheetXlsx,
    ExportSpreadsheetPdf,
    AddSpreadsheetNamedRange(NamedRangeArgs),
    UpdateSpreadsheetNamedRange(NamedRangeArgs),
    DeleteSpreadsheetNamedRange(NamedRangeNameArgs),
    RestoreSpreadsheetNamedRange(NamedRangeNameArgs),
    RecoverSession(RecoverySessionArgs),
    DiscardRecoverySession(RecoverySessionArgs),
}

/// Argument key that acknowledges the loss of unsaved work.
///
/// It is deliberately not part of any command's declared argument list: it is
/// a policy acknowledgement understood by the dispatcher for every command
/// `OpenDocCommand::replaces_open_document` reports, not a parameter any
/// individual command interprets.
pub const DISCARD_UNSAVED_CHANGES_ARG: &str = "discardUnsavedChanges";

impl OpenDocCommand {
    pub fn name(&self) -> &'static str {
        match self {
            Self::CreateDocument(..) => "create_document",
            Self::CloseDocument => "close_document",
            Self::UndoCurrentEdit => "undo_current_edit",
            Self::RedoCurrentEdit => "redo_current_edit",
            Self::GetDocument => "get_document",
            Self::GetAuditView => "get_audit_view",
            Self::SetDocumentTitle(..) => "set_document_title",
            Self::SetDocumentLocale(..) => "set_document_locale",
            Self::SetBookmark(..) => "set_bookmark",
            Self::DeleteBookmark(..) => "delete_bookmark",
            Self::AddParagraph(..) => "add_paragraph",
            Self::RenderDocumentHtml => "render_document_html",
            Self::RenderSuggestionPreviewHtml(..) => "render_suggestion_preview_html",
            Self::LayoutDocument => "layout_document",
            Self::GetRuntimeProfile(..) => "get_runtime_profile",
            Self::GetRuntimeSession(..) => "get_runtime_session",
            Self::AuthorizeRuntimeCommand(..) => "authorize_runtime_command",
            Self::CreateRuntimeShareInvite(..) => "create_runtime_share_invite",
            Self::RelayRuntimeSync(..) => "relay_runtime_sync",
            Self::ResolveRuntimeDocumentLookup(..) => "resolve_runtime_document_lookup",
            Self::ImportGoogleDocsJson(..) => "import_google_docs_json",
            Self::ImportDocOrDocxPath(..) => "import_doc_or_docx_path",
            Self::ExportGoogleDocsJson => "export_google_docs_json",
            Self::ExportDocx => "export_docx",
            Self::ExportOdt => "export_odt",
            Self::ExportPdf => "export_pdf",
            Self::ExportHtml => "export_html",
            Self::ExportText => "export_text",
            Self::ExportImageBlob(..) => "export_image_blob",
            Self::ImportGoogleSheetsJson(..) => "import_google_sheets_json",
            Self::ExportGoogleSheetsJson => "export_google_sheets_json",
            Self::RenderWorkbookHtml(..) => "render_workbook_html",
            Self::ImportDocxBase64(..) => "import_docx_base64",
            Self::AddBinaryBlob(..) => "add_binary_blob",
            Self::SimulateShallowClone => "simulate_shallow_clone",
            Self::UpdateBinaryBlobMetadata(..) => "update_binary_blob_metadata",
            Self::DeleteBinaryBlob(..) => "delete_binary_blob",
            Self::RestoreBinaryBlob(..) => "restore_binary_blob",
            Self::RecordBlobArchiveTombstone(..) => "record_blob_archive_tombstone",
            Self::AddImageBlock(..) => "add_image_block",
            Self::InsertImageBlockAfter(..) => "insert_image_block_after",
            Self::SignBlobWithOpenSshPrivateKey(..) => "sign_blob_with_openssh_private_key",
            Self::SignFastqBlobWithOpenSshPrivateKey(..) => {
                "sign_fastq_blob_with_openssh_private_key"
            }
            Self::SignImagePixelsBlobWithOpenSshPrivateKey(..) => {
                "sign_image_pixels_blob_with_openssh_private_key"
            }
            Self::SignWithOpenSshPrivateKey(..) => "sign_with_openssh_private_key",
            Self::SignCurrentRepositoryVersionWithOpenSshPrivateKey(..) => {
                "sign_current_repository_version_with_openssh_private_key"
            }
            Self::VerifyCurrentSignature(..) => "verify_current_signature",
            Self::VerifyCurrentSignatures => "verify_current_signatures",
            Self::SaveLocalRepository(..) => "save_local_repository",
            Self::SaveLocalRepositoryOrCandidate(..) => "save_local_repository_or_candidate",
            Self::SaveFlatRepository(..) => "save_flat_repository",
            Self::SaveFlatRepositoryOrCandidate(..) => "save_flat_repository_or_candidate",
            Self::SaveOpenDalFsRepository(..) => "save_opendal_fs_repository",
            Self::SaveOpenDalFsRepositoryOrCandidate(..) => {
                "save_opendal_fs_repository_or_candidate"
            }
            Self::AutosaveCurrentRepository => "autosave_current_repository",
            Self::ListDocumentVersions(..) => "list_document_versions",
            Self::OpenDocumentAtVersion(..) => "open_document_at_version",
            Self::DiffDocumentVersions(..) => "diff_document_versions",
            Self::NameDocumentVersion(..) => "name_document_version",
            Self::RestoreDocumentVersion(..) => "restore_document_version",
            Self::CompactLocalRepository(..) => "compact_local_repository",
            Self::OpenLocalRepository(..) => "open_local_repository",
            Self::ScanLocalRepository(..) => "scan_local_repository",
            Self::OpenFlatRepository(..) => "open_flat_repository",
            Self::OpenOpenDalFsRepository(..) => "open_opendal_fs_repository",
            Self::MergeLocalRepositoryCandidates(..) => "merge_local_repository_candidates",
            Self::MergeFlatRepositoryCandidates(..) => "merge_flat_repository_candidates",
            Self::MergeOpenDalFsRepositoryCandidates(..) => {
                "merge_opendal_fs_repository_candidates"
            }
            Self::OpenLocalRepositoryByDoi(..) => "open_local_repository_by_doi",
            Self::OpenFlatRepositoryByDoi(..) => "open_flat_repository_by_doi",
            Self::OpenOpenDalFsRepositoryByDoi(..) => "open_opendal_fs_repository_by_doi",
            Self::SetDocumentDoi(..) => "set_document_doi",
            Self::InsertParagraphAfter(..) => "insert_paragraph_after",
            Self::SplitParagraphAtInline(..) => "split_paragraph_at_inline",
            Self::SplitParagraphAtTextOffset(..) => "split_paragraph_at_text_offset",
            Self::JoinParagraphWithPrevious(..) => "join_paragraph_with_previous",
            Self::DeleteBlock(..) => "delete_block",
            Self::MoveBlock(..) => "move_block",
            Self::SetBlockTextStyle(..) => "set_block_text_style",
            Self::SetEditorSelectionBlockStyle(..) => "set_editor_selection_block_style",
            Self::SetBlockAlignment(..) => "set_block_alignment",
            Self::SetEditorSelectionBlockAlignment(..) => "set_editor_selection_block_alignment",
            Self::SetBlockIndentStart(..) => "set_block_indent_start",
            Self::SetEditorSelectionBlockIndentStart(..) => {
                "set_editor_selection_block_indent_start"
            }
            Self::SetBlockIndentEnd(..) => "set_block_indent_end",
            Self::SetEditorSelectionBlockIndentEnd(..) => "set_editor_selection_block_indent_end",
            Self::SetBlockIndentFirstLine(..) => "set_block_indent_first_line",
            Self::SetEditorSelectionBlockIndentFirstLine(..) => {
                "set_editor_selection_block_indent_first_line"
            }
            Self::SetPageSetup(..) => "set_page_setup",
            Self::SetPageOrientation(..) => "set_page_orientation",
            Self::SetPageFurniture(..) => "set_page_furniture",
            Self::SetPageFurnitureHtml(..) => "set_page_furniture_html",
            Self::ClearPageFurniture(..) => "clear_page_furniture",
            Self::ClearPageFurnitureOverride(..) => "clear_page_furniture_override",
            Self::SetBlockLineSpacing(..) => "set_block_line_spacing",
            Self::SetEditorSelectionBlockLineSpacing(..) => {
                "set_editor_selection_block_line_spacing"
            }
            Self::SetBlockSpaceBefore(..) => "set_block_space_before",
            Self::SetEditorSelectionBlockSpaceBefore(..) => {
                "set_editor_selection_block_space_before"
            }
            Self::SetBlockSpaceAfter(..) => "set_block_space_after",
            Self::SetEditorSelectionBlockSpaceAfter(..) => "set_editor_selection_block_space_after",
            Self::SetBlockDirection(..) => "set_block_direction",
            Self::SetEditorSelectionBlockDirection(..) => "set_editor_selection_block_direction",
            Self::SetBlockKeepWithNext(..) => "set_block_keep_with_next",
            Self::SetEditorSelectionBlockKeepWithNext(..) => {
                "set_editor_selection_block_keep_with_next"
            }
            Self::SetBlockBackground(..) => "set_block_background",
            Self::SetEditorSelectionBlockBackground(..) => "set_editor_selection_block_background",
            Self::SetBlockBorder(..) => "set_block_border",
            Self::SetEditorSelectionBlockBorder(..) => "set_editor_selection_block_border",
            Self::ClearBlockProperty(..) => "clear_block_property",
            Self::ClearEditorSelectionBlockProperty(..) => "clear_editor_selection_block_property",
            Self::SetListItemChecked(..) => "set_list_item_checked",
            Self::FindInDocument(..) => "find_in_document",
            Self::ReplaceMatchInDocument(..) => "replace_match_in_document",
            Self::ReplaceAllInDocument(..) => "replace_all_in_document",
            Self::AdjustEditorSelectionIndent(..) => "adjust_editor_selection_indent",
            Self::AddHeading(..) => "add_heading",
            Self::UpdateHeadingLevel(..) => "update_heading_level",
            Self::AddLink(..) => "add_link",
            Self::InsertLinkAfter(..) => "insert_link_after",
            Self::AddMention(..) => "add_mention",
            Self::InsertMentionAfter(..) => "insert_mention_after",
            Self::InsertDateChipAfter(..) => "insert_date_chip_after",
            Self::AddFootnoteRef => "add_footnote_ref",
            Self::AddEndnoteRef => "add_endnote_ref",
            Self::InsertFootnoteRefAfter(..) => "insert_footnote_ref_after",
            Self::InsertEndnoteRefAfter(..) => "insert_endnote_ref_after",
            Self::UpdateFootnoteBody(..) => "update_footnote_body",
            Self::AddEquation(..) => "add_equation",
            Self::InsertEquationAfter(..) => "insert_equation_after",
            Self::AddEquationBlock(..) => "add_equation_block",
            Self::InsertEquationBlockAfter(..) => "insert_equation_block_after",
            Self::AddListItem(..) => "add_list_item",
            Self::InsertListItemAfter(..) => "insert_list_item_after",
            Self::UpdateListItem(..) => "update_list_item",
            Self::SetOrderedListStart(..) => "set_ordered_list_start",
            Self::SetOrderedListFormat(..) => "set_ordered_list_format",
            Self::SetBulletListMarker(..) => "set_bullet_list_marker",
            Self::AdjustEditorSelectionListIndent(..) => "adjust_editor_selection_list_indent",
            Self::InsertPageBreakAfter(..) => "insert_page_break_after",
            Self::AddPageBreak => "add_page_break",
            Self::InsertHorizontalRuleAfter(..) => "insert_horizontal_rule_after",
            Self::AddHorizontalRule => "add_horizontal_rule",
            Self::InsertTableOfContentsAfter(..) => "insert_table_of_contents_after",
            Self::InsertBibliographyAfter(..) => "insert_bibliography_after",
            Self::InsertTableAfter(..) => "insert_table_after",
            Self::AddTable => "add_table",
            Self::AddTableRow(..) => "add_table_row",
            Self::DeleteTableRow(..) => "delete_table_row",
            Self::AddTableCell(..) => "add_table_cell",
            Self::DeleteTableCell(..) => "delete_table_cell",
            Self::InsertTableColumn(..) => "insert_table_column",
            Self::DeleteTableColumn(..) => "delete_table_column",
            Self::SetTableColumnWidth(..) => "set_table_column_width",
            Self::ClearTableColumnWidth(..) => "clear_table_column_width",
            Self::SetTableRowHeight(..) => "set_table_row_height",
            Self::ClearTableRowHeight(..) => "clear_table_row_height",
            Self::SetTableRowHeader(..) => "set_table_row_header",
            Self::SortTableRows(..) => "sort_table_rows",
            Self::SetTableBorder(..) => "set_table_border",
            Self::ClearTableBorder(..) => "clear_table_border",
            Self::SetTableAlignment(..) => "set_table_alignment",
            Self::ClearTableAlignment(..) => "clear_table_alignment",
            Self::MergeTableCells(..) => "merge_table_cells",
            Self::SplitTableCell(..) => "split_table_cell",
            Self::SetTableCellBackground(..) => "set_table_cell_background",
            Self::SetTableCellBorder(..) => "set_table_cell_border",
            Self::SetTableCellVerticalAlignment(..) => "set_table_cell_vertical_alignment",
            Self::SetTableCellRowHeader(..) => "set_table_cell_row_header",
            Self::SetTableCellPadding(..) => "set_table_cell_padding",
            Self::ClearTableCellProperty(..) => "clear_table_cell_property",
            Self::AddCitation => "add_citation",
            Self::InsertCitation(..) => "insert_citation",
            Self::InsertCitationGroup(..) => "insert_citation_group",
            Self::InsertFootnoteCitationGroup(..) => "insert_footnote_citation_group",
            Self::InsertFootnoteCitationAfter(..) => "insert_footnote_citation_after",
            Self::UpdateCitationGroupItems(..) => "update_citation_group_items",
            Self::SetCitationStyle(..) => "set_citation_style",
            Self::AddComment(..) => "add_comment",
            Self::AddTextRangeComment(..) => "add_text_range_comment",
            Self::AddBlockComment(..) => "add_block_comment",
            Self::AddCommentReply(..) => "add_comment_reply",
            Self::ResolveCommentThread(..) => "resolve_comment_thread",
            Self::ReopenCommentThread(..) => "reopen_comment_thread",
            Self::SetCommentThreadAction(..) => "set_comment_thread_action",
            Self::SetCommentThreadReaction(..) => "set_comment_thread_reaction",
            Self::DeleteCommentThread(..) => "delete_comment_thread",
            Self::RestoreCommentThread(..) => "restore_comment_thread",
            Self::DeleteComment(..) => "delete_comment",
            Self::RestoreComment(..) => "restore_comment",
            Self::UpdateComment(..) => "update_comment",
            Self::AddSuggestion(..) => "add_suggestion",
            Self::AddTextRangeSuggestion(..) => "add_text_range_suggestion",
            Self::AddBlockSuggestion(..) => "add_block_suggestion",
            Self::AddBlockDeleteSuggestion(..) => "add_block_delete_suggestion",
            Self::AddBlockInsertSuggestion(..) => "add_block_insert_suggestion",
            Self::AddBlockReplaceSuggestion(..) => "add_block_replace_suggestion",
            Self::AddDeleteSuggestion(..) => "add_delete_suggestion",
            Self::AddTextRangeDeleteSuggestion(..) => "add_text_range_delete_suggestion",
            Self::AddFormatSuggestion(..) => "add_format_suggestion",
            Self::AddTextRangeFormatSuggestion(..) => "add_text_range_format_suggestion",
            Self::AddTextRangeFormatRemovalSuggestion(..) => {
                "add_text_range_format_removal_suggestion"
            }
            Self::AddTextRangeFormatReplacementSuggestion(..) => {
                "add_text_range_format_replacement_suggestion"
            }
            Self::AddLinkChangeSuggestion(..) => "add_link_change_suggestion",
            Self::AddParagraphStyleSuggestion(..) => "add_paragraph_style_suggestion",
            Self::UpdateSuggestion(..) => "update_suggestion",
            Self::AcceptSuggestion(..) => "accept_suggestion",
            Self::AcceptAllSuggestions(..) => "accept_all_suggestions",
            Self::RejectSuggestion(..) => "reject_suggestion",
            Self::RejectAllSuggestions(..) => "reject_all_suggestions",
            Self::DescribeEditorSelection(..) => "describe_editor_selection",
            Self::SelectAllEditorContent => "select_all_editor_content",
            Self::ApplyEditorInput(..) => "apply_editor_input",
            Self::ApplyEditorMark(..) => "apply_editor_mark",
            Self::ImportBibtex(..) => "import_bibtex",
            Self::AddBibliographyReference(..) => "add_bibliography_reference",
            Self::UpdateBibliographyReference(..) => "update_bibliography_reference",
            Self::UpdateBibliographyReferenceMetadata(..) => {
                "update_bibliography_reference_metadata"
            }
            Self::DeleteBibliographyReference(..) => "delete_bibliography_reference",
            Self::RestoreBibliographyReference(..) => "restore_bibliography_reference",
            Self::DeleteCitationGroup(..) => "delete_citation_group",
            Self::RestoreCitationGroup(..) => "restore_citation_group",
            Self::UpdateInlineText(..) => "update_inline_text",
            Self::UpdateInlineEquationSource(..) => "update_inline_equation_source",
            Self::UpdateMentionLabel(..) => "update_mention_label",
            Self::SelectDropdownOption(..) => "select_dropdown_option",
            Self::UpdateDateChip(..) => "update_date_chip",
            Self::UpdateLinkHref(..) => "update_link_href",
            Self::InsertInlineText(..) => "insert_inline_text",
            Self::DeleteInline(..) => "delete_inline",
            Self::AddTextMark(..) => "add_text_mark",
            Self::AddTextMarkRange(..) => "add_text_mark_range",
            Self::RemoveTextMark(..) => "remove_text_mark",
            Self::RemoveTextMarkRange(..) => "remove_text_mark_range",
            Self::UpdateBlockEquationSource(..) => "update_block_equation_source",
            Self::UpdateImageAltText(..) => "update_image_alt_text",
            Self::UpdateImageBlobHash(..) => "update_image_blob_hash",
            Self::SetImageBlockWidth(..) => "set_image_block_width",
            Self::SetImageBlockHeight(..) => "set_image_block_height",
            Self::SetImageBlockSize(..) => "set_image_block_size",
            Self::ClearImageBlockSize(..) => "clear_image_block_size",
            Self::SetImageBlockPlacement(..) => "set_image_block_placement",
            Self::SetImageBlockWrapClearance(..) => "set_image_block_wrap_clearance",
            Self::SetImageBlockPositioned(..) => "set_image_block_positioned",
            Self::ClearImageBlockPositioned(..) => "clear_image_block_positioned",
            Self::SetImageBlockEffects(..) => "set_image_block_effects",
            Self::SetImageBlockCrop(..) => "set_image_block_crop",
            Self::SetImageBlockCaption(..) => "set_image_block_caption",
            Self::SetImageBlockBorder(..) => "set_image_block_border",
            Self::DescribeSpreadsheetSelection(..) => "describe_spreadsheet_selection",
            Self::ReduceSpreadsheetSelection(..) => "reduce_spreadsheet_selection",
            Self::CopySpreadsheetSelectionTsv(..) => "copy_spreadsheet_selection_tsv",
            Self::PasteSpreadsheetTsv(..) => "paste_spreadsheet_tsv",
            Self::ClearSpreadsheetSelection(..) => "clear_spreadsheet_selection",
            Self::SetSpreadsheetSelectionFormat(..) => "set_spreadsheet_selection_format",
            Self::AddSpreadsheetRowAfterSelection(..) => "add_spreadsheet_row_after_selection",
            Self::AddSpreadsheetColumnAfterSelection(..) => {
                "add_spreadsheet_column_after_selection"
            }
            Self::DeleteSpreadsheetSelectionRow(..) => "delete_spreadsheet_selection_row",
            Self::DeleteSpreadsheetSelectionColumn(..) => "delete_spreadsheet_selection_column",
            Self::MergeSpreadsheetSelection(..) => "merge_spreadsheet_selection",
            Self::FreezeSpreadsheetSelection(..) => "freeze_spreadsheet_selection",
            Self::SetSpreadsheetSelectionFilter(..) => "set_spreadsheet_selection_filter",
            Self::SetSpreadsheetWorkbookMetadata(..) => "set_spreadsheet_workbook_metadata",
            Self::SetSpreadsheetCell(..) => "set_spreadsheet_cell",
            Self::SetSpreadsheetCells(..) => "set_spreadsheet_cells",
            Self::AddSpreadsheetSheet(..) => "add_spreadsheet_sheet",
            Self::RenameSpreadsheetSheet(..) => "rename_spreadsheet_sheet",
            Self::DeleteSpreadsheetSheet(..) => "delete_spreadsheet_sheet",
            Self::RestoreSpreadsheetSheet(..) => "restore_spreadsheet_sheet",
            Self::AddSpreadsheetRow(..) => "add_spreadsheet_row",
            Self::DeleteSpreadsheetRow(..) => "delete_spreadsheet_row",
            Self::RestoreSpreadsheetRow(..) => "restore_spreadsheet_row",
            Self::AddSpreadsheetColumn(..) => "add_spreadsheet_column",
            Self::DeleteSpreadsheetColumn(..) => "delete_spreadsheet_column",
            Self::RestoreSpreadsheetColumn(..) => "restore_spreadsheet_column",
            Self::AddSpreadsheetCellComment(..) => "add_spreadsheet_cell_comment",
            Self::UpdateSpreadsheetCellComment(..) => "update_spreadsheet_cell_comment",
            Self::DeleteSpreadsheetCellComment(..) => "delete_spreadsheet_cell_comment",
            Self::RestoreSpreadsheetCellComment(..) => "restore_spreadsheet_cell_comment",
            Self::SetSpreadsheetFrozenAxes(..) => "set_spreadsheet_frozen_axes",
            Self::SetSpreadsheetCellValidation(..) => "set_spreadsheet_cell_validation",
            Self::ClearSpreadsheetCellValidation(..) => "clear_spreadsheet_cell_validation",
            Self::RestoreSpreadsheetCellValidation(..) => "restore_spreadsheet_cell_validation",
            Self::MergeSpreadsheetCells(..) => "merge_spreadsheet_cells",
            Self::UnmergeSpreadsheetCells(..) => "unmerge_spreadsheet_cells",
            Self::RestoreSpreadsheetMerge(..) => "restore_spreadsheet_merge",
            Self::SetSpreadsheetBasicFilter(..) => "set_spreadsheet_basic_filter",
            Self::SetSpreadsheetPrintArea(..) => "set_spreadsheet_print_area",
            Self::ClearSpreadsheetPrintArea(..) => "clear_spreadsheet_print_area",
            Self::SetSpreadsheetPrintOrientation(..) => "set_spreadsheet_print_orientation",
            Self::SetSpreadsheetBasicFilterOptions(..) => "set_spreadsheet_basic_filter_options",
            Self::ClearSpreadsheetBasicFilter(..) => "clear_spreadsheet_basic_filter",
            Self::RestoreSpreadsheetBasicFilter(..) => "restore_spreadsheet_basic_filter",
            Self::AddSpreadsheetProtectedRange(..) => "add_spreadsheet_protected_range",
            Self::UpdateSpreadsheetProtectedRange(..) => "update_spreadsheet_protected_range",
            Self::DeleteSpreadsheetProtectedRange(..) => "delete_spreadsheet_protected_range",
            Self::RestoreSpreadsheetProtectedRange(..) => "restore_spreadsheet_protected_range",
            Self::SetSpreadsheetCellInSheet(..) => "set_spreadsheet_cell_in_sheet",
            Self::SetSpreadsheetCellsInSheet(..) => "set_spreadsheet_cells_in_sheet",
            Self::SetSpreadsheetCellFormat(..) => "set_spreadsheet_cell_format",
            Self::SetSpreadsheetRowHeight(..) => "set_spreadsheet_row_height",
            Self::SetSpreadsheetColumnWidth(..) => "set_spreadsheet_column_width",
            Self::SetSpreadsheetSelectionRowsHidden(..) => "set_spreadsheet_selection_rows_hidden",
            Self::SetSpreadsheetSelectionColumnsHidden(..) => {
                "set_spreadsheet_selection_columns_hidden"
            }
            Self::CopySpreadsheetRange(..) => "copy_spreadsheet_range",
            Self::SortSpreadsheetRange(..) => "sort_spreadsheet_range",
            Self::FillSpreadsheetRange(..) => "fill_spreadsheet_range",
            Self::ImportSpreadsheetCsv(..) => "import_spreadsheet_csv",
            Self::ExportSpreadsheetCsv(..) => "export_spreadsheet_csv",
            Self::ImportSpreadsheetXlsx(..) => "import_spreadsheet_xlsx",
            Self::ExportSpreadsheetXlsx => "export_spreadsheet_xlsx",
            Self::ExportSpreadsheetPdf => "export_spreadsheet_pdf",
            Self::AddSpreadsheetNamedRange(..) => "add_spreadsheet_named_range",
            Self::UpdateSpreadsheetNamedRange(..) => "update_spreadsheet_named_range",
            Self::DeleteSpreadsheetNamedRange(..) => "delete_spreadsheet_named_range",
            Self::RestoreSpreadsheetNamedRange(..) => "restore_spreadsheet_named_range",
            Self::RecoverSession(..) => "recover_session",
            Self::DiscardRecoverySession(..) => "discard_recovery_session",
        }
    }

    /// Does running this command throw away whatever is in the open document
    /// and put different content in its place?
    ///
    /// `OpenDocApp::dispatch_command` refuses a replacing command while the
    /// open document holds changes the repository does not have, unless the
    /// caller passes `"discardUnsavedChanges": true` alongside the normal
    /// arguments. That check is the only unsaved-work guard in the system, so
    /// this classification is what keeps a user's work safe.
    ///
    /// The match is deliberately exhaustive with no catch-all arm. Adding a
    /// command therefore fails to compile until its author has decided which
    /// side of the line it falls on — the four actions that silently discarded
    /// unsaved work before 2026-09-11 (`open_local_repository`,
    /// `open_flat_repository` and the two Word/JSON imports) were all cases of
    /// a new action forgetting a guard that lived at the call site.
    pub fn replaces_open_document(&self) -> bool {
        match self {
            // Replaces the open document wholesale.
            Self::RecoverSession(..)
            | Self::CreateDocument(..) | Self::CloseDocument | Self::ImportGoogleDocsJson(..)
            | Self::ImportDocOrDocxPath(..) | Self::ImportDocxBase64(..)
            | Self::OpenLocalRepository(..) | Self::OpenFlatRepository(..)
            | Self::OpenOpenDalFsRepository(..) | Self::OpenLocalRepositoryByDoi(..)
            | Self::OpenFlatRepositoryByDoi(..) | Self::OpenOpenDalFsRepositoryByDoi(..)
            | Self::MergeLocalRepositoryCandidates(..) | Self::MergeFlatRepositoryCandidates(..)
            | Self::MergeOpenDalFsRepositoryCandidates(..)
            | Self::RestoreDocumentVersion(..)
            // Swaps the whole workbook for the imported one.
            | Self::ImportSpreadsheetXlsx(..) => true,
            // Leaves the open document in place.
            Self::DiscardRecoverySession(..)
            | Self::UndoCurrentEdit | Self::RedoCurrentEdit | Self::GetDocument
            | Self::GetAuditView | Self::SetDocumentTitle(..) | Self::SetDocumentLocale(..)
            | Self::SetBookmark(..) | Self::DeleteBookmark(..)
            | Self::AddParagraph(..) | Self::RenderDocumentHtml
            | Self::RenderSuggestionPreviewHtml(..) | Self::LayoutDocument
            | Self::GetRuntimeProfile(..)
            | Self::GetRuntimeSession(..) | Self::AuthorizeRuntimeCommand(..)
            | Self::CreateRuntimeShareInvite(..) | Self::RelayRuntimeSync(..)
            | Self::ResolveRuntimeDocumentLookup(..) | Self::ExportGoogleDocsJson
            | Self::ExportDocx | Self::ExportOdt | Self::ExportPdf | Self::ExportHtml
            | Self::ExportText | Self::ExportImageBlob(..)
            | Self::ImportGoogleSheetsJson(..) | Self::ExportGoogleSheetsJson
            | Self::SortSpreadsheetRange(..) | Self::FillSpreadsheetRange(..)
            | Self::ImportSpreadsheetCsv(..) | Self::ExportSpreadsheetCsv(..)
            | Self::ExportSpreadsheetXlsx
            | Self::ExportSpreadsheetPdf
            | Self::RenderWorkbookHtml(..) | Self::AddBinaryBlob(..)
            | Self::SimulateShallowClone | Self::UpdateBinaryBlobMetadata(..)
            | Self::DeleteBinaryBlob(..) | Self::RestoreBinaryBlob(..)
            | Self::RecordBlobArchiveTombstone(..) | Self::AddImageBlock(..)
            | Self::InsertImageBlockAfter(..) | Self::SignBlobWithOpenSshPrivateKey(..)
            | Self::SignFastqBlobWithOpenSshPrivateKey(..)
            | Self::SignImagePixelsBlobWithOpenSshPrivateKey(..)
            | Self::SignWithOpenSshPrivateKey(..)
            | Self::SignCurrentRepositoryVersionWithOpenSshPrivateKey(..)
            | Self::VerifyCurrentSignature(..)
            | Self::VerifyCurrentSignatures | Self::SaveLocalRepository(..)
            | Self::SaveLocalRepositoryOrCandidate(..) | Self::SaveFlatRepository(..)
            | Self::SaveFlatRepositoryOrCandidate(..) | Self::SaveOpenDalFsRepository(..)
            | Self::SaveOpenDalFsRepositoryOrCandidate(..) | Self::AutosaveCurrentRepository
            | Self::ListDocumentVersions(..) | Self::OpenDocumentAtVersion(..)
            | Self::DiffDocumentVersions(..) | Self::NameDocumentVersion(..)
            | Self::CompactLocalRepository(..) | Self::ScanLocalRepository(..)
            | Self::SetDocumentDoi(..) | Self::InsertParagraphAfter(..)
            | Self::SplitParagraphAtInline(..) | Self::SplitParagraphAtTextOffset(..)
            | Self::JoinParagraphWithPrevious(..) | Self::DeleteBlock(..) | Self::MoveBlock(..)
            | Self::SetBlockTextStyle(..) | Self::SetEditorSelectionBlockStyle(..)
            | Self::SetBlockAlignment(..)
            | Self::SetEditorSelectionBlockAlignment(..)
            | Self::SetBlockIndentStart(..)
            | Self::SetEditorSelectionBlockIndentStart(..)
            | Self::SetBlockIndentEnd(..)
            | Self::SetEditorSelectionBlockIndentEnd(..)
            | Self::SetBlockIndentFirstLine(..)
            | Self::SetEditorSelectionBlockIndentFirstLine(..)
            | Self::SetBlockLineSpacing(..)
            | Self::SetEditorSelectionBlockLineSpacing(..)
            | Self::SetBlockSpaceBefore(..)
            | Self::SetEditorSelectionBlockSpaceBefore(..)
            | Self::SetBlockSpaceAfter(..)
            | Self::SetEditorSelectionBlockSpaceAfter(..)
            | Self::SetBlockDirection(..)
            | Self::SetEditorSelectionBlockDirection(..)
            | Self::SetBlockKeepWithNext(..)
            | Self::SetEditorSelectionBlockKeepWithNext(..)
            | Self::SetBlockBackground(..)
            | Self::SetEditorSelectionBlockBackground(..)
            | Self::SetBlockBorder(..)
            | Self::SetEditorSelectionBlockBorder(..)
            | Self::SetPageSetup(..)
            | Self::SetPageOrientation(..)
            | Self::SetPageFurniture(..)
            | Self::SetPageFurnitureHtml(..)
            | Self::ClearPageFurniture(..)
            | Self::ClearPageFurnitureOverride(..)
            | Self::ClearBlockProperty(..)
            | Self::ClearEditorSelectionBlockProperty(..)
            | Self::SetListItemChecked(..)
            // Find reads the open document; replace edits it in place. Neither
            // swaps it for another one, so neither can discard unsaved work.
            | Self::FindInDocument(..) | Self::ReplaceMatchInDocument(..)
            | Self::ReplaceAllInDocument(..)
            | Self::AdjustEditorSelectionIndent(..)
            | Self::AddHeading(..) | Self::UpdateHeadingLevel(..) | Self::AddLink(..)
            | Self::InsertLinkAfter(..) | Self::AddMention(..) | Self::InsertMentionAfter(..)
            | Self::InsertDateChipAfter(..)
            | Self::AddFootnoteRef | Self::AddEndnoteRef | Self::InsertFootnoteRefAfter(..)
            | Self::InsertEndnoteRefAfter(..)
            | Self::UpdateFootnoteBody(..) | Self::AddEquation(..)
            | Self::InsertEquationAfter(..) | Self::AddEquationBlock(..)
            | Self::InsertEquationBlockAfter(..) | Self::AddListItem(..)
            | Self::InsertListItemAfter(..) | Self::UpdateListItem(..)
            | Self::SetOrderedListStart(..)
            | Self::SetOrderedListFormat(..)
            | Self::SetBulletListMarker(..)
            | Self::AdjustEditorSelectionListIndent(..) | Self::InsertPageBreakAfter(..)
            | Self::AddPageBreak | Self::InsertHorizontalRuleAfter(..) | Self::AddHorizontalRule
            | Self::InsertTableOfContentsAfter(..)
            | Self::InsertBibliographyAfter(..)
            | Self::InsertTableAfter(..) | Self::AddTable
            | Self::AddTableRow(..) | Self::DeleteTableRow(..) | Self::AddTableCell(..)
            | Self::DeleteTableCell(..)
            // Table structure and cell styling edit the open document in
            // place; none of them swaps it for another one.
            | Self::InsertTableColumn(..) | Self::DeleteTableColumn(..)
            | Self::SetTableColumnWidth(..) | Self::ClearTableColumnWidth(..)
            | Self::SetTableRowHeight(..) | Self::ClearTableRowHeight(..)
            | Self::SetTableRowHeader(..)
            | Self::SortTableRows(..)
            | Self::SetTableBorder(..) | Self::ClearTableBorder(..)
            | Self::SetTableAlignment(..) | Self::ClearTableAlignment(..)
            | Self::MergeTableCells(..) | Self::SplitTableCell(..)
            | Self::SetTableCellBackground(..) | Self::SetTableCellBorder(..)
            | Self::SetTableCellVerticalAlignment(..) | Self::SetTableCellPadding(..)
            | Self::SetTableCellRowHeader(..)
            | Self::ClearTableCellProperty(..)
            | Self::AddCitation | Self::InsertCitation(..)
            | Self::InsertCitationGroup(..) | Self::InsertFootnoteCitationGroup(..)
            | Self::InsertFootnoteCitationAfter(..) | Self::UpdateCitationGroupItems(..)
            | Self::SetCitationStyle(..) | Self::AddComment(..) | Self::AddTextRangeComment(..)
            | Self::AddBlockComment(..) | Self::AddCommentReply(..)
            | Self::ResolveCommentThread(..) | Self::ReopenCommentThread(..)
            | Self::SetCommentThreadAction(..)
            | Self::SetCommentThreadReaction(..)
            | Self::DeleteCommentThread(..) | Self::RestoreCommentThread(..)
            | Self::DeleteComment(..) | Self::RestoreComment(..) | Self::UpdateComment(..)
            | Self::AddSuggestion(..) | Self::AddTextRangeSuggestion(..)
            | Self::AddBlockSuggestion(..) | Self::AddDeleteSuggestion(..)
            | Self::AddBlockDeleteSuggestion(..)
            | Self::AddBlockInsertSuggestion(..)
            | Self::AddBlockReplaceSuggestion(..)
            | Self::AddTextRangeDeleteSuggestion(..) | Self::AddFormatSuggestion(..)
            | Self::AddTextRangeFormatSuggestion(..)
            | Self::AddTextRangeFormatRemovalSuggestion(..) | Self::UpdateSuggestion(..)
            | Self::AddTextRangeFormatReplacementSuggestion(..)
            | Self::AddLinkChangeSuggestion(..) | Self::AddParagraphStyleSuggestion(..)
            | Self::AcceptSuggestion(..) | Self::AcceptAllSuggestions(..)
            | Self::RejectSuggestion(..) | Self::RejectAllSuggestions(..)
            | Self::DescribeEditorSelection(..) | Self::SelectAllEditorContent
            | Self::ApplyEditorInput(..) | Self::ApplyEditorMark(..)
            | Self::ImportBibtex(..)
            | Self::AddBibliographyReference(..) | Self::UpdateBibliographyReference(..)
            | Self::UpdateBibliographyReferenceMetadata(..)
            | Self::DeleteBibliographyReference(..) | Self::RestoreBibliographyReference(..)
            | Self::DeleteCitationGroup(..) | Self::RestoreCitationGroup(..)
            | Self::UpdateInlineText(..) | Self::UpdateInlineEquationSource(..)
            | Self::UpdateMentionLabel(..) | Self::SelectDropdownOption(..)
            | Self::UpdateDateChip(..) | Self::UpdateLinkHref(..)
            | Self::InsertInlineText(..) | Self::DeleteInline(..) | Self::AddTextMark(..)
            | Self::AddTextMarkRange(..) | Self::RemoveTextMark(..)
            | Self::RemoveTextMarkRange(..) | Self::UpdateBlockEquationSource(..)
            | Self::UpdateImageAltText(..) | Self::UpdateImageBlobHash(..)
            // Image geometry edits the open document; none of them replace it.
            | Self::SetImageBlockWidth(..) | Self::SetImageBlockHeight(..)
            | Self::SetImageBlockSize(..) | Self::ClearImageBlockSize(..)
            | Self::SetImageBlockPlacement(..)
            | Self::SetImageBlockWrapClearance(..)
            | Self::SetImageBlockPositioned(..) | Self::ClearImageBlockPositioned(..)
            | Self::SetImageBlockEffects(..)
            | Self::SetImageBlockCrop(..)
            | Self::SetImageBlockCaption(..)
            | Self::SetImageBlockBorder(..)
            | Self::DescribeSpreadsheetSelection(..) | Self::ReduceSpreadsheetSelection(..)
            | Self::CopySpreadsheetSelectionTsv(..) | Self::PasteSpreadsheetTsv(..)
            | Self::ClearSpreadsheetSelection(..) | Self::SetSpreadsheetSelectionFormat(..)
            | Self::AddSpreadsheetRowAfterSelection(..)
            | Self::AddSpreadsheetColumnAfterSelection(..)
            | Self::DeleteSpreadsheetSelectionRow(..)
            | Self::DeleteSpreadsheetSelectionColumn(..) | Self::MergeSpreadsheetSelection(..)
            | Self::FreezeSpreadsheetSelection(..) | Self::SetSpreadsheetSelectionFilter(..)
            | Self::SetSpreadsheetWorkbookMetadata(..) | Self::SetSpreadsheetCell(..)
            | Self::SetSpreadsheetCells(..) | Self::AddSpreadsheetSheet(..)
            | Self::RenameSpreadsheetSheet(..) | Self::DeleteSpreadsheetSheet(..)
            | Self::RestoreSpreadsheetSheet(..) | Self::AddSpreadsheetRow(..)
            | Self::DeleteSpreadsheetRow(..) | Self::RestoreSpreadsheetRow(..)
            | Self::AddSpreadsheetColumn(..) | Self::DeleteSpreadsheetColumn(..)
            | Self::RestoreSpreadsheetColumn(..) | Self::AddSpreadsheetCellComment(..)
            | Self::UpdateSpreadsheetCellComment(..) | Self::DeleteSpreadsheetCellComment(..)
            | Self::RestoreSpreadsheetCellComment(..) | Self::SetSpreadsheetFrozenAxes(..)
            | Self::SetSpreadsheetCellValidation(..) | Self::ClearSpreadsheetCellValidation(..)
            | Self::RestoreSpreadsheetCellValidation(..) | Self::MergeSpreadsheetCells(..)
            | Self::UnmergeSpreadsheetCells(..) | Self::RestoreSpreadsheetMerge(..)
            | Self::SetSpreadsheetBasicFilter(..) | Self::SetSpreadsheetPrintArea(..)
            | Self::ClearSpreadsheetPrintArea(..) | Self::SetSpreadsheetPrintOrientation(..)
            | Self::SetSpreadsheetBasicFilterOptions(..)
            | Self::ClearSpreadsheetBasicFilter(..) | Self::RestoreSpreadsheetBasicFilter(..)
            | Self::AddSpreadsheetProtectedRange(..) | Self::UpdateSpreadsheetProtectedRange(..)
            | Self::DeleteSpreadsheetProtectedRange(..)
            | Self::RestoreSpreadsheetProtectedRange(..) | Self::SetSpreadsheetCellInSheet(..)
            | Self::SetSpreadsheetCellsInSheet(..) | Self::SetSpreadsheetCellFormat(..)
            | Self::SetSpreadsheetRowHeight(..) | Self::SetSpreadsheetColumnWidth(..)
            | Self::SetSpreadsheetSelectionRowsHidden(..)
            | Self::SetSpreadsheetSelectionColumnsHidden(..)
            | Self::CopySpreadsheetRange(..) | Self::AddSpreadsheetNamedRange(..)
            | Self::UpdateSpreadsheetNamedRange(..) | Self::DeleteSpreadsheetNamedRange(..)
            | Self::RestoreSpreadsheetNamedRange(..) => false,
        }
    }

    pub fn spec(&self) -> &'static CommandSpec {
        command_spec(self.name())
            .unwrap_or_else(|| panic!("missing command metadata for {}", self.name()))
    }
}

#[cfg(test)]
mod replacement_policy_tests {
    use super::*;
    use crate::{command_names, parse_json_command, COMMANDS};
    use serde_json::json;

    fn parse(name: &str) -> OpenDocCommand {
        try_parse(name).unwrap_or_else(|| panic!("{name} parses as a known command"))
    }

    fn try_parse(name: &str) -> Option<OpenDocCommand> {
        let args = json!({
            "path": "/tmp/x",
            "namespace": "ns",
            "documentUuid": "doc",
            "doi": "10.0/x",
            "title": "t",
            "text": "t",
            "locale": "en",
            "name": "x.docx",
            "base64": "",
            "jsonText": "{}",
            "sessionId": "recovery-0",
            // A manifest hash, so `restore_document_version` — a replacement —
            // is classified rather than skipped.
            "manifest": "sha256:aaaaaaaa",
            // Spreadsheet import/sort/fill arguments.
            "sheetId": "sheet-1",
            "origin": "A1",
            "range": "A1:B2",
            "column": "A",
            "sourceRange": "A1:B2",
            "targetRange": "A1:B4",
            "descending": false,
            "hasHeader": false,
            // Page setup and page furniture arguments.
            "widthTwips": 12240,
            "heightTwips": 15840,
            "marginTopTwips": 1440,
            "marginBottomTwips": 1440,
            "marginStartTwips": 1440,
            "marginEndTwips": 1440,
            "orientation": "portrait",
            "slot": "header",
            "field": "none",
            "alignment": "start",
        });
        parse_json_command(name, &args).ok().flatten()
    }

    /// FS-6: these four discarded unsaved work without asking until the guard
    /// moved into the dispatcher.
    #[test]
    fn the_actions_that_lost_work_are_classified_as_replacing() {
        for name in [
            "open_local_repository",
            "open_flat_repository",
            "import_doc_or_docx_path",
            "import_docx_base64",
            "import_google_docs_json",
        ] {
            assert!(
                parse(name).replaces_open_document(),
                "{name} replaces the open document"
            );
        }
    }

    #[test]
    fn editing_and_saving_are_not_replacements() {
        for name in [
            "add_paragraph",
            "set_document_title",
            "save_local_repository",
            "autosave_current_repository",
            "scan_local_repository",
            "undo_current_edit",
            "redo_current_edit",
            "get_document",
            "discard_recovery_session",
            // Page setup and page furniture edit the open document; they do
            // not swap it for another one, so they must not raise the
            // unsaved-changes guard.
            "set_page_setup",
            "set_page_orientation",
            "set_page_furniture",
            "clear_page_furniture",
            "clear_page_furniture_override",
        ] {
            assert!(
                !parse(name).replaces_open_document(),
                "{name} leaves the open document in place"
            );
        }
    }

    /// `recover_session` replaces the open document with a crashed session's
    /// state, so it must be guarded like any other replacement.
    #[test]
    fn recovering_a_crashed_session_is_a_replacement() {
        assert!(parse("recover_session").replaces_open_document());
    }

    /// Completeness is the compiler's job, not a test's:
    /// `replaces_open_document` is a `match` with no catch-all arm, so a
    /// command added later cannot reach `dispatch` without being classified.
    #[test]
    fn the_classification_covers_the_whole_command_enum() {
        assert_eq!(
            command_names().len(),
            COMMANDS.len(),
            "every command in the registry is reachable by name"
        );
        // …and the classification is exercised, not merely compiled. The name
        // of this test promised coverage of `replaces_open_document` while
        // the body never called it: `fn replaces_open_document(&self) -> bool
        // { false }` passed, and so did flipping any command the two name
        // lists above do not mention — `create_document`, `close_document`,
        // `restore_document_version`, the `open_*_by_doi` family, the
        // `merge_*_candidates` family and `import_spreadsheet_xlsx` were all
        // in that gap. PLAN88 §7.
        // `parse` feeds one canned argument bag to every command, and the
        // argument-heavy editing commands do not take it — so classify the
        // ones that parse, and assert separately that none of the commands
        // that matter here is among the ones skipped.
        let classified: Vec<&str> = COMMANDS
            .iter()
            .map(|spec| spec.name)
            .filter(|name| try_parse(name).is_some())
            .collect();
        assert!(
            classified.len() >= 40,
            "only {} of {} commands parsed with the canned arguments, which is \
             too few for the classification below to mean anything",
            classified.len(),
            COMMANDS.len()
        );
        let mut replacing: Vec<&str> = classified
            .iter()
            .copied()
            .filter(|name| parse(name).replaces_open_document())
            .collect();
        replacing.sort_unstable();
        assert_eq!(
            replacing,
            vec![
                "close_document",
                "create_document",
                "import_doc_or_docx_path",
                "import_docx_base64",
                "import_google_docs_json",
                "import_spreadsheet_xlsx",
                "merge_flat_repository_candidates",
                "merge_local_repository_candidates",
                "merge_opendal_fs_repository_candidates",
                "open_flat_repository",
                "open_flat_repository_by_doi",
                "open_local_repository",
                "open_local_repository_by_doi",
                "open_opendal_fs_repository",
                "open_opendal_fs_repository_by_doi",
                "recover_session",
                "restore_document_version",
            ],
            "the set of commands that replace the open document changed. Every \
             one of these raises the unsaved-changes guard in `dispatch`, so \
             adding to this list is a new prompt and removing from it is work \
             discarded without asking."
        );
        // …and none of them is in the skipped set, so the list above cannot be
        // satisfied by a command quietly failing to parse.
        for name in &replacing {
            assert!(classified.contains(name), "{name} was skipped");
        }
    }
}
