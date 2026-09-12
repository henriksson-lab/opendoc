//! The typed operation vocabulary: what a document edit can say.

use crate::causal::{CausalContext, OperationId};
use opendoc_core::{
    BibliographyReference, Block, BlockProperty, BlockPropertyKey, CellSpan, CitationGroup,
    CommentThread, Footnote, HeaderFooterSlot, ImageLayout, Inline, Length, ListKind, Mark,
    MarkKind, PageSetup, StableId, Suggestion, TableCell, TableCellProperty, TableCellPropertyKey,
    TableColumn, TableRow,
};
use serde::{Deserialize, Serialize};

/// A typed document operation plus, optionally, the causal context it was
/// generated in.
///
/// `context: None` means "no causal information": the operation is read as
/// concurrent with every other actor's work, and merge ordering degrades to
/// the pre-ADR-0007 `(actor, seq)` order. See
/// `docs/adr/0007-causal-ordering-and-text-convergence.md`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Operation {
    pub id: OperationId,
    pub kind: OperationKind,
    #[serde(default)]
    pub context: Option<CausalContext>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum BlockTextStyle {
    Paragraph,
    Heading {
        level: u8,
    },
    ListItem {
        list_id: StableId,
        level: u8,
        kind: ListKind,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum OperationKind {
    SetDocumentTitle {
        title: String,
    },
    SetDocumentDoi {
        doi: Option<String>,
    },
    SetDocumentLocale {
        locale: String,
    },
    InsertBlock {
        after: Option<StableId>,
        block: Block,
    },
    DeleteBlock {
        block_id: StableId,
    },
    SetBlockTextStyle {
        block_id: StableId,
        style: BlockTextStyle,
    },
    InsertInline {
        block_id: StableId,
        after: Option<StableId>,
        inline: Inline,
    },
    MoveInlineToBlock {
        inline_id: StableId,
        target_block_id: StableId,
        after: Option<StableId>,
    },
    AddMark {
        text_id: StableId,
        mark: Mark,
    },
    RemoveMark {
        text_id: StableId,
        kind: MarkKind,
        value: Option<String>,
    },
    AddMarkRange {
        range: opendoc_core::TextRange,
        mark: Mark,
    },
    AddSuggestion {
        suggestion: Suggestion,
    },
    UpdateSuggestionInsertContent {
        suggestion_id: StableId,
        content: Vec<Inline>,
    },
    AddCommentThread {
        thread: CommentThread,
    },
    AddCommentReply {
        thread_id: StableId,
        comment: opendoc_core::Comment,
    },
    UpsertFootnote {
        footnote: Footnote,
    },
    UpsertBibliographyReference {
        reference: BibliographyReference,
    },
    DeleteBibliographyReference {
        reference_id: StableId,
        revision: u64,
    },
    UpsertCitationGroup {
        citation: CitationGroup,
    },
    DeleteCitationGroup {
        citation_id: StableId,
        revision: u64,
    },
    UpdateCitationStyle {
        style: String,
        locale: String,
    },
    DeleteCommentThread {
        thread_id: StableId,
    },
    RestoreCommentThread {
        thread_id: StableId,
    },
    DeleteComment {
        thread_id: StableId,
        comment_id: StableId,
    },
    RestoreComment {
        thread_id: StableId,
        comment_id: StableId,
    },
    UpdateCommentBody {
        thread_id: StableId,
        comment_id: StableId,
        body: Vec<Inline>,
    },
    UpdateInlineText {
        inline_id: StableId,
        text: String,
    },
    /// Character-level insert into a text or link run. `offset` counts
    /// Unicode scalar values from the start of the run and is clamped to
    /// the run length when concurrent edits shortened it.
    InsertText {
        inline_id: StableId,
        offset: usize,
        text: String,
    },
    /// Character-level delete of `[start, end)` from a text or link run.
    /// Offsets count Unicode scalar values and are clamped to the run.
    DeleteText {
        inline_id: StableId,
        start: usize,
        end: usize,
    },
    UpdateInlineEquationSource {
        inline_id: StableId,
        source: String,
    },
    UpdateMentionLabel {
        inline_id: StableId,
        label: String,
    },
    UpdateLinkHref {
        inline_id: StableId,
        href: String,
    },
    UpdateBlockEquationSource {
        block_id: StableId,
        source: String,
    },
    UpdateImageAltText {
        block_id: StableId,
        alt_text: String,
    },
    UpdateImageBlobHash {
        block_id: StableId,
        blob_hash: String,
    },
    /// Replaces the whole display geometry of an image block.
    ///
    /// Whole-value, like `SetPageSetup` and for the same reason: a resize is
    /// one intent. Merging one actor's width with another's height would
    /// produce a shape neither actor dragged, and with the axes coupled by an
    /// aspect ratio it would also distort the picture.
    UpdateImageLayout {
        block_id: StableId,
        layout: ImageLayout,
    },
    UpdateHeadingLevel {
        block_id: StableId,
        level: u8,
    },
    UpdateListItem {
        block_id: StableId,
        level: u8,
        kind: ListKind,
    },
    /// Writes one typed block property. Concurrent writes to the same
    /// property of the same block converge last-writer-wins; see
    /// `docs/adr/0006-block-property-merge.md`.
    SetBlockProperty {
        block_id: StableId,
        property: BlockProperty,
    },
    /// Returns one block property to inheriting its default.
    ClearBlockProperty {
        block_id: StableId,
        key: BlockPropertyKey,
    },
    /// Replaces the whole page geometry. Concurrent writes converge
    /// last-writer-wins on the *whole* `PageSetup`, not per dimension:
    /// "switch to A4" is one intent, and merging one actor's A4 width with
    /// another's Letter height would produce a page neither actor chose.
    /// See `docs/adr/0009-pagination-and-page-geometry.md`.
    SetPageSetup {
        page_setup: PageSetup,
    },
    /// Replaces the whole block list of one header/footer slot.
    /// Whole-slot replacement, so concurrent header edits converge
    /// last-writer-wins per slot rather than merging character by character;
    /// header blocks are not reachable by the block-addressed operations.
    /// See ADR 0009.
    SetPageFurniture {
        slot: HeaderFooterSlot,
        blocks: Vec<Block>,
    },
    AcceptSuggestion {
        suggestion_id: StableId,
        accepted_by: String,
    },
    RejectSuggestion {
        suggestion_id: StableId,
        rejected_by: String,
    },
    DeleteInline {
        inline_id: StableId,
    },
    InsertTableRow {
        table_block_id: StableId,
        after_row: Option<StableId>,
        row: TableRow,
    },
    DeleteTableRow {
        table_block_id: StableId,
        row_id: StableId,
    },
    InsertTableCell {
        table_block_id: StableId,
        row_id: StableId,
        after_cell: Option<StableId>,
        cell: TableCell,
    },
    DeleteTableCell {
        table_block_id: StableId,
        row_id: StableId,
        cell_id: StableId,
    },
    /// Adds a column to the right of `after_column`, or at the end of the
    /// table when it is `None` — the same anchoring [`InsertTableRow`] uses,
    /// for the same reason: an index means different things on two replicas,
    /// an identity does not.
    ///
    /// The cells the new column needs in each row are **not** in the payload.
    /// They are derived from the row and column ids
    /// ([`TableCell::filling`]), so a row another actor inserted concurrently
    /// gets its cell too, and gets the same one on every replica.
    ///
    /// [`InsertTableRow`]: OperationKind::InsertTableRow
    InsertTableColumn {
        table_block_id: StableId,
        after_column: Option<StableId>,
        column: TableColumn,
    },
    DeleteTableColumn {
        table_block_id: StableId,
        column_id: StableId,
    },
    /// `None` returns the column to auto width.
    SetTableColumnWidth {
        table_block_id: StableId,
        column_id: StableId,
        width: Option<Length>,
    },
    /// Merges (span > 1) or splits (`CellSpan::SINGLE`) the cell. One
    /// operation for both because a merge is exactly the inverse of a split:
    /// the covered cells keep their identity and their content either way.
    SetTableCellSpan {
        cell_id: StableId,
        span: CellSpan,
    },
    SetTableCellProperty {
        cell_id: StableId,
        property: TableCellProperty,
    },
    ClearTableCellProperty {
        cell_id: StableId,
        key: TableCellPropertyKey,
    },
}
