use crate::{
    AppCitationItem, EditorSelection, FindOptions, OpenDocPermissionGrant, OpenDocPresencePeer,
    OpenDocRelayOperation, OpenDocRuntimeLookupEntry, OpenDocRuntimeProfile,
};
use opendoc_spreadsheet::{SheetFilterCriterion, SheetFilterSortSpec};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateDocumentArgs {
    pub title: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SetDocumentTitleArgs {
    pub title: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SetDocumentLocaleArgs {
    pub locale: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AddParagraphArgs {
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportGoogleDocsJsonArgs {
    pub title: String,
    pub json_text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportDocOrDocxPathArgs {
    pub path: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportGoogleSheetsJsonArgs {
    pub json_text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderWorkbookHtmlArgs {
    pub sheet_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportDocxBase64Args {
    pub name: String,
    pub base64: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryPathArgs {
    pub path: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryNamespaceArgs {
    pub path: String,
    pub namespace: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryDocumentArgs {
    pub path: String,
    pub document_uuid: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryNamespaceDocumentArgs {
    pub path: String,
    pub namespace: String,
    pub document_uuid: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryDoiArgs {
    pub path: String,
    pub doi: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryNamespaceDoiArgs {
    pub path: String,
    pub namespace: String,
    pub doi: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompactLocalRepositoryArgs {
    pub path: String,
    pub pack_name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AddBinaryBlobArgs {
    pub name: String,
    pub media_type: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpdateBinaryBlobMetadataArgs {
    pub blob_hash: String,
    pub name: String,
    pub media_type: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeleteBinaryBlobArgs {
    pub blob_hash: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RestoreBinaryBlobArgs {
    pub blob_hash: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordBlobArchiveTombstoneArgs {
    pub blob_hash: String,
    pub archive_locator: String,
    pub restore_hint: String,
    pub signer: String,
    pub signature: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AddImageBlockArgs {
    pub blob_hash: String,
    pub alt_text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InsertImageBlockAfterArgs {
    pub after_block_id: String,
    pub blob_hash: String,
    pub alt_text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SignBlobWithOpenSshPrivateKeyArgs {
    pub blob_hash: String,
    pub private_key_pem: String,
    pub signer_display: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SignFastqBlobWithOpenSshPrivateKeyArgs {
    pub blob_hash: String,
    pub profile: String,
    pub private_key_pem: String,
    pub signer_display: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SignImagePixelsBlobWithOpenSshPrivateKeyArgs {
    pub blob_hash: String,
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
    pub private_key_pem: String,
    pub signer_display: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SignWithOpenSshPrivateKeyArgs {
    pub private_key_pem: String,
    pub signer_display: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifyCurrentSignatureArgs {
    pub private_key_pem: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GetRuntimeSessionArgs {
    pub profile: OpenDocRuntimeProfile,
    pub subject: Option<String>,
    pub document_uuid: Option<String>,
    pub presence: Vec<OpenDocPresencePeer>,
    pub permissions: Vec<OpenDocPermissionGrant>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthorizeRuntimeCommandArgs {
    pub profile: OpenDocRuntimeProfile,
    pub subject: Option<String>,
    pub document_uuid: Option<String>,
    pub command_name: String,
    pub permissions: Vec<OpenDocPermissionGrant>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateRuntimeShareInviteArgs {
    pub profile: OpenDocRuntimeProfile,
    pub subject: Option<String>,
    pub document_uuid: Option<String>,
    pub target_subject: Option<String>,
    pub actions: Vec<String>,
    pub permissions: Vec<OpenDocPermissionGrant>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelayRuntimeSyncArgs {
    pub profile: OpenDocRuntimeProfile,
    pub subject: Option<String>,
    pub document_uuid: Option<String>,
    pub base_manifest: Option<String>,
    pub operations: Vec<OpenDocRelayOperation>,
    pub permissions: Vec<OpenDocPermissionGrant>,
    pub presence: Vec<OpenDocPresencePeer>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolveRuntimeDocumentLookupArgs {
    pub profile: OpenDocRuntimeProfile,
    pub subject: Option<String>,
    pub document_uuid: Option<String>,
    pub doi: Option<String>,
    pub permissions: Vec<OpenDocPermissionGrant>,
    pub service_index: Vec<OpenDocRuntimeLookupEntry>,
    pub scanned_documents: Vec<OpenDocRuntimeLookupEntry>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SetDocumentDoiArgs {
    pub doi: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InsertParagraphAfterArgs {
    pub after_block_id: Option<String>,
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SplitParagraphAtInlineArgs {
    pub inline_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SplitParagraphAtTextOffsetArgs {
    pub block_id: String,
    pub inline_id: String,
    pub offset: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlockIdArgs {
    pub block_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SetBlockTextStyleArgs {
    pub block_id: String,
    pub style: String,
    pub level: u8,
    pub list_kind: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SetEditorSelectionBlockStyleArgs {
    pub selection: EditorSelection,
    pub style: String,
    pub level: u8,
    pub list_kind: String,
}

/// A block-property write whose value is a name the model parses
/// (`Alignment`, `TextDirection`). Which property is being written is the
/// command variant, never a field, so the pair cannot be mismatched.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SetBlockNamedValueArgs {
    pub block_id: String,
    pub value: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SetEditorSelectionBlockNamedValueArgs {
    pub selection: EditorSelection,
    pub value: String,
}

/// A block-property write carrying a length in twips (1/20 pt), the unit the
/// model stores.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SetBlockLengthArgs {
    pub block_id: String,
    pub twips: i32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SetEditorSelectionBlockLengthArgs {
    pub selection: EditorSelection,
    pub twips: i32,
}

/// Line spacing is a sum type in the model (`Multiple`/`Exact`/`AtLeast`), so
/// the wire form is the rule plus its value: thousandths of a line for
/// `"multiple"`, twips for the other two.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SetBlockLineSpacingArgs {
    pub block_id: String,
    pub mode: String,
    pub value: i32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SetEditorSelectionBlockLineSpacingArgs {
    pub selection: EditorSelection,
    pub mode: String,
    pub value: i32,
}

/// A whole page geometry. Page setup is written as a unit rather than one
/// dimension at a time; see `docs/adr/0009-pagination-and-page-geometry.md`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SetPageSetupArgs {
    pub width_twips: i32,
    pub height_twips: i32,
    pub margin_top_twips: i32,
    pub margin_bottom_twips: i32,
    pub margin_start_twips: i32,
    pub margin_end_twips: i32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SetPageOrientationArgs {
    pub orientation: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SetPageFurnitureArgs {
    /// `"header"` or `"footer"`.
    pub slot: String,
    pub text: String,
    /// `"none"`, `"page-number"` or `"page-count"`.
    pub field: String,
    pub alignment: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PageFurnitureSlotArgs {
    pub slot: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClearBlockPropertyArgs {
    pub block_id: String,
    pub key: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClearEditorSelectionBlockPropertyArgs {
    pub selection: EditorSelection,
    pub key: String,
}

/// Find and replace share one options payload so the two can never disagree
/// about what the query means; see [`crate::FindOptions`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FindInDocumentArgs {
    pub find: FindOptions,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplaceMatchInDocumentArgs {
    pub find: FindOptions,
    pub replacement: String,
    /// Index into the match list `find_in_document` returned for the same
    /// options. Rust re-runs the search rather than trusting positions the
    /// frontend cached, so a stale highlight cannot edit the wrong range.
    pub match_index: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplaceAllInDocumentArgs {
    pub find: FindOptions,
    pub replacement: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SetListItemCheckedArgs {
    pub block_id: String,
    pub checked: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AddHeadingArgs {
    pub text: String,
    pub level: u8,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpdateHeadingLevelArgs {
    pub block_id: String,
    pub level: u8,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AddLinkArgs {
    pub text: String,
    pub href: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InsertLinkAfterArgs {
    pub block_id: String,
    pub after_inline_id: Option<String>,
    pub text: String,
    pub href: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AddMentionArgs {
    pub label: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InsertMentionAfterArgs {
    pub block_id: String,
    pub after_inline_id: Option<String>,
    pub label: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InsertFootnoteRefAfterArgs {
    pub block_id: String,
    pub after_inline_id: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpdateFootnoteBodyArgs {
    pub footnote_id: String,
    pub body: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AddEquationArgs {
    pub source: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InsertEquationAfterArgs {
    pub block_id: String,
    pub after_inline_id: Option<String>,
    pub source: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InsertEquationBlockAfterArgs {
    pub after_block_id: String,
    pub source: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AddListItemArgs {
    pub text: String,
    pub level: u8,
    pub list_kind: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InsertListItemAfterArgs {
    pub after_block_id: String,
    pub text: String,
    pub level: u8,
    pub list_kind: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpdateListItemArgs {
    pub block_id: String,
    pub level: u8,
    pub list_kind: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdjustEditorSelectionListIndentArgs {
    pub selection: EditorSelection,
    pub delta: i8,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AfterBlockArgs {
    pub after_block_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InsertTableAfterArgs {
    Default {
        after_block_id: String,
    },
    Sized {
        after_block_id: String,
        rows: usize,
        columns: usize,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AddTableRowArgs {
    pub table_block_id: String,
    pub after_row: Option<String>,
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeleteTableRowArgs {
    pub table_block_id: String,
    pub row_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AddTableCellArgs {
    pub table_block_id: String,
    pub row_id: String,
    pub after_cell: Option<String>,
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeleteTableCellArgs {
    pub table_block_id: String,
    pub row_id: String,
    pub cell_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InsertTableColumnArgs {
    pub table_block_id: String,
    pub after_column_id: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TableColumnArgs {
    pub table_block_id: String,
    pub column_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SetTableColumnWidthArgs {
    pub table_block_id: String,
    pub column_id: String,
    pub twips: i32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MergeTableCellsArgs {
    pub cell_id: String,
    pub row_span: u32,
    pub column_span: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TableCellArgs {
    pub cell_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SetTableCellBackgroundArgs {
    pub cell_id: String,
    pub color: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SetTableCellBorderArgs {
    pub cell_id: String,
    pub edge: String,
    pub style: String,
    pub twips: i32,
    pub color: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SetTableCellVerticalAlignmentArgs {
    pub cell_id: String,
    pub alignment: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SetTableCellPaddingArgs {
    pub cell_id: String,
    pub edge: String,
    pub twips: i32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClearTableCellPropertyArgs {
    pub cell_id: String,
    pub key: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InsertCitationArgs {
    pub reference_id: String,
    pub after_inline_id: Option<String>,
    pub locator: Option<String>,
    pub label: Option<String>,
    pub prefix: Option<String>,
    pub suffix: Option<String>,
    pub suppress_author: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InsertCitationGroupArgs {
    pub items: Vec<AppCitationItem>,
    pub after_inline_id: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InsertFootnoteCitationGroupArgs {
    pub footnote_id: String,
    pub items: Vec<AppCitationItem>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InsertFootnoteCitationAfterArgs {
    pub block_id: String,
    pub after_inline_id: Option<String>,
    pub items: Vec<AppCitationItem>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpdateCitationGroupItemsArgs {
    pub citation_id: String,
    pub items: Vec<AppCitationItem>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SetCitationStyleArgs {
    pub style: String,
    pub locale: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthorBodyArgs {
    pub author: String,
    pub body: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextRangeAuthorBodyArgs {
    pub start_inline_id: String,
    pub end_inline_id: String,
    pub author: String,
    pub body: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlockAuthorBodyArgs {
    pub block_id: String,
    pub author: String,
    pub body: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThreadAuthorBodyArgs {
    pub thread_id: String,
    pub author: String,
    pub body: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThreadIdArgs {
    pub thread_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThreadCommentIdArgs {
    pub thread_id: String,
    pub comment_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpdateCommentArgs {
    pub thread_id: String,
    pub comment_id: String,
    pub body: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthorTextArgs {
    pub author: String,
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextRangeAuthorTextArgs {
    pub start_inline_id: String,
    pub end_inline_id: String,
    pub author: String,
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlockAuthorTextArgs {
    pub block_id: String,
    pub author: String,
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeleteSuggestionArgs {
    pub author: String,
    pub inline_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextRangeAuthorArgs {
    pub start_inline_id: String,
    pub end_inline_id: String,
    pub author: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FormatSuggestionArgs {
    pub author: String,
    pub inline_id: String,
    pub mark_kind: String,
    pub value: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextRangeFormatSuggestionArgs {
    pub start_inline_id: String,
    pub end_inline_id: String,
    pub author: String,
    pub mark_kind: String,
    pub value: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpdateSuggestionArgs {
    pub suggestion_id: String,
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AcceptSuggestionArgs {
    pub suggestion_id: String,
    pub accepted_by: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AcceptAllSuggestionsArgs {
    pub accepted_by: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RejectSuggestionArgs {
    pub suggestion_id: String,
    pub rejected_by: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RejectAllSuggestionsArgs {
    pub rejected_by: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BibliographyReferenceMetadataArgs {
    pub title: String,
    pub authors: Vec<String>,
    pub issued: Option<String>,
    pub doi: Option<String>,
    pub url: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpdateBibliographyReferenceArgs {
    pub reference_id: String,
    pub title: String,
    pub issued: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpdateBibliographyReferenceMetadataArgs {
    pub reference_id: String,
    pub metadata: BibliographyReferenceMetadataArgs,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReferenceIdArgs {
    pub reference_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CitationIdArgs {
    pub citation_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InlineIdArgs {
    pub inline_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InlineTextArgs {
    pub inline_id: String,
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InlineSourceArgs {
    pub inline_id: String,
    pub source: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InlineLabelArgs {
    pub inline_id: String,
    pub label: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InlineHrefArgs {
    pub inline_id: String,
    pub href: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InsertInlineTextArgs {
    pub block_id: String,
    pub after_inline_id: Option<String>,
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextMarkArgs {
    pub inline_id: String,
    pub mark_kind: String,
    pub value: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextMarkRangeArgs {
    pub start_inline_id: String,
    pub end_inline_id: String,
    pub mark_kind: String,
    pub value: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlockSourceArgs {
    pub block_id: String,
    pub source: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlockAltTextArgs {
    pub block_id: String,
    pub alt_text: String,
}

/// A drawn size for one axis of an image block, in twips.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImageBlockLengthArgs {
    pub block_id: String,
    pub twips: i32,
}

/// Both axes of an image block at once, because a corner drag is one gesture.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImageBlockSizeArgs {
    pub block_id: String,
    pub width_twips: i32,
    pub height_twips: i32,
}

/// `"block"`, `"wrap-start"` or `"wrap-end"`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImageBlockPlacementArgs {
    pub block_id: String,
    pub placement: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlockBlobHashArgs {
    pub block_id: String,
    pub blob_hash: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpreadsheetWorkbookMetadataArgs {
    pub title: String,
    pub locale: String,
    pub timezone: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SheetIdArgs {
    pub sheet_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SheetTitleArgs {
    pub title: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SheetRenameArgs {
    pub sheet_id: String,
    pub title: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SheetRowArgs {
    pub sheet_id: String,
    pub row: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SheetColumnArgs {
    pub sheet_id: String,
    pub column: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpreadsheetSelectionArgs {
    pub sheet_id: String,
    pub anchor: String,
    pub focus: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpreadsheetSelectionActionArgs {
    pub sheet_id: String,
    pub anchor: String,
    pub focus: String,
    pub action: String,
    pub value: String,
    pub extend: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpreadsheetPasteTsvArgs {
    pub sheet_id: String,
    pub origin: String,
    pub text: String,
    /// Top-left cell the text was copied from, when the copy came from this
    /// workbook. Pasted formulas then shift their relative references by the
    /// paste offset; `None` stores the text verbatim.
    pub source_origin: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpreadsheetSelectionFormatArgs {
    pub sheet_id: String,
    pub anchor: String,
    pub focus: String,
    pub property: String,
    pub value: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpreadsheetCellArgs {
    pub address: String,
    pub value: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpreadsheetCellEditsArgs {
    pub cells: Vec<(String, String)>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SheetCellArgs {
    pub sheet_id: String,
    pub address: String,
    pub value: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SheetCellEditsArgs {
    pub sheet_id: String,
    pub cells: Vec<(String, String)>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CellCommentCreateArgs {
    pub sheet_id: String,
    pub address: String,
    pub author: String,
    pub body: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CellCommentUpdateArgs {
    pub comment_id: String,
    pub body: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CellCommentIdArgs {
    pub comment_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrozenAxesArgs {
    pub sheet_id: String,
    pub frozen_rows: u32,
    pub frozen_columns: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CellValidationArgs {
    pub sheet_id: String,
    pub address: String,
    pub kind: String,
    pub values: Vec<String>,
    pub strict: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SheetAddressArgs {
    pub sheet_id: String,
    pub address: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SheetRangeArgs {
    pub sheet_id: String,
    pub range: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FilterOptionsArgs {
    pub sheet_id: String,
    pub criteria: Vec<SheetFilterCriterion>,
    pub sort_specs: Vec<SheetFilterSortSpec>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtectedRangeArgs {
    pub sheet_id: String,
    pub range: String,
    pub description: String,
    pub warning_only: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NamedRangeArgs {
    pub sheet_id: String,
    pub name: String,
    pub range: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NamedRangeNameArgs {
    pub name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CellFormatArgs {
    pub sheet_id: String,
    pub address: String,
    pub property: String,
    pub value: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SheetRowHeightArgs {
    pub sheet_id: String,
    pub row: String,
    /// Row height in pixels; `0` clears the explicit height.
    pub height: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SheetColumnWidthArgs {
    pub sheet_id: String,
    pub column: String,
    /// Column width in pixels; `0` clears the explicit width.
    pub width: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CopyRangeArgs {
    pub sheet_id: String,
    pub source_range: String,
    pub target_address: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SortRangeArgs {
    pub sheet_id: String,
    pub range: String,
    /// Column label to sort by; must sit inside `range`.
    pub column: String,
    pub descending: bool,
    pub has_header: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FillRangeArgs {
    pub sheet_id: String,
    pub source_range: String,
    /// Range the fill-handle drag covered. It may include the source block.
    pub target_range: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportSpreadsheetCsvArgs {
    pub sheet_id: String,
    pub origin: String,
    pub text: String,
    /// `","` by default; `"tab"`/`"\t"` or any single ASCII character.
    pub delimiter: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExportSpreadsheetCsvArgs {
    pub sheet_id: String,
    pub delimiter: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportSpreadsheetXlsxArgs {
    pub title: String,
    pub base64: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListDocumentVersionsArgs {
    /// Maximum number of versions to walk back from the head; `None` uses the
    /// service default.
    pub limit: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocumentVersionArgs {
    pub manifest: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiffDocumentVersionsArgs {
    pub from_manifest: String,
    pub to_manifest: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NameDocumentVersionArgs {
    pub manifest: String,
    pub label: String,
    pub author: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecoverySessionArgs {
    pub session_id: String,
}
