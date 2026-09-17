//! The typed operation vocabulary: what a document edit can say.

use crate::causal::{CausalContext, OperationId};
use opendoc_core::{
    BibliographyReference, Block, BlockProperty, BlockPropertyKey, Bookmark, CellSpan,
    CitationGroup, CommentThread, Footnote, HeaderFooterSlot, ImageLayout, Inline, InsertPosition,
    Length, ListKind, Mark, MarkKind, OrderedListFormat, PageSetup, Section, StableId, Suggestion,
    TableCell, TableCellProperty, TableCellPropertyKey, TableColumn, TableRow,
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
    Title,
    Subtitle,
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
    /// Writes a durable bookmark, including its deletion tombstone.  The
    /// revision is the LWW value for this bookmark id; a live name collision
    /// is resolved deterministically by the later operation's bookmark.
    UpsertBookmark {
        bookmark: Bookmark,
    },
    /// Adds a block at `position`, beside a sibling in either the document
    /// body or a table-cell block container.
    ///
    /// [`InsertPosition::First`] is what makes undoing the deletion of the
    /// *first body block* possible; [`InsertPosition::Before`] likewise makes
    /// the first nested cell block expressible through its next sibling.
    InsertBlock {
        position: InsertPosition,
        block: Block,
    },
    DeleteBlock {
        block_id: StableId,
    },
    /// Moves one existing block beside another existing block.  Unlike a
    /// delete/insert pair, this preserves the block identity (and therefore
    /// concurrent inline edits, comments and bookmarks) while changing only
    /// its sibling container and order.
    ///
    /// `First` and `Last` address the document body.  A cell-local first or
    /// last child is expressed relative to a sibling with `Before`/`After`;
    /// that keeps the operation path-free and stable under table edits.
    MoveBlock {
        block_id: StableId,
        position: InsertPosition,
    },
    /// Atomically inserts a section record and the boundary immediately before
    /// an existing top-level body block. `before_block_id` is required: a
    /// section boundary can never be the last body block, so unlike generic
    /// block insertion it must not degrade to an append when its anchor is
    /// gone.
    InsertSection {
        before_block_id: StableId,
        boundary_id: StableId,
        section: Section,
    },
    /// Atomically removes a non-root section and its unique boundary.
    DeleteSection {
        section_id: StableId,
    },
    /// Replaces one section's whole page geometry under the same all-or-
    /// nothing semantics as document-wide `SetPageSetup`.
    SetSectionPageSetup {
        section_id: StableId,
        page_setup: PageSetup,
    },
    /// Replaces one declared furniture slot of one section.
    SetSectionFurniture {
        section_id: StableId,
        slot: HeaderFooterSlot,
        blocks: Vec<Block>,
    },
    /// Restores a first/even section slot to its ordinary-slot inheritance.
    ClearSectionFurnitureOverride {
        section_id: StableId,
        slot: HeaderFooterSlot,
    },
    SetBlockTextStyle {
        block_id: StableId,
        style: BlockTextStyle,
    },
    /// Adds an inline to a block's content at `position` — the same anchoring
    /// [`InsertBlock`] uses, for the same reason.
    ///
    /// [`InsertBlock`]: OperationKind::InsertBlock
    InsertInline {
        block_id: StableId,
        position: InsertPosition,
        inline: Inline,
    },
    MoveInlineToBlock {
        inline_id: StableId,
        target_block_id: StableId,
        position: InsertPosition,
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
    ResolveCommentThread {
        thread_id: StableId,
        resolved_by: String,
        resolved_at_ms: u64,
    },
    ReopenCommentThread {
        thread_id: StableId,
    },
    SetCommentThreadAction {
        thread_id: StableId,
        assignee: Option<String>,
        due_at_ms: Option<u64>,
        completed_by: Option<String>,
        completed_at_ms: Option<u64>,
    },
    /// Sets whether `actor` has this emoji reaction on the thread.  This is a
    /// set rather than a toggle so undo and replay remain explicit.
    SetCommentThreadReaction {
        thread_id: StableId,
        emoji: String,
        actor: String,
        present: bool,
    },
    UpsertFootnote {
        footnote: Footnote,
    },
    /// Changes where an existing note is projected.  The revision shares the
    /// note's LWW clock so a delayed placement change cannot overwrite a
    /// newer note tombstone or body edit.
    SetEndnotePlacement {
        footnote_id: StableId,
        revision: u64,
        endnote: bool,
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
    /// Selects one existing option of an atomic dropdown.  Option definitions
    /// are immutable in this initial slice; the id prevents a concurrent
    /// display-label edit from changing what a choice means.
    SelectDropdownOption {
        inline_id: StableId,
        option_id: String,
    },
    /// Replaces an atomic date chip's canonical ISO calendar date.
    UpdateDateChip {
        inline_id: StableId,
        date: String,
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
    /// Sets the first displayed ordinal for one ordered wrapper in a list
    /// run. A start of one deliberately means the inherited default and is
    /// stored by removing the map entry, so equivalent documents have one
    /// canonical representation.
    SetListStart {
        list_id: StableId,
        level: u8,
        start: u32,
    },
    /// Sets the counter style for one ordered wrapper in a list run. The
    /// inherited depth-cycle value is represented by removing the entry.
    SetListFormat {
        list_id: StableId,
        level: u8,
        format: OrderedListFormat,
    },
    /// Sets the glyph for one unordered wrapper in a list run. The inherited
    /// disc/circle/square depth-cycle is represented by removing the entry.
    SetListBulletMarker {
        list_id: StableId,
        level: u8,
        marker: opendoc_core::BulletListMarker,
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
    /// Removes a first/even-page override, restoring inheritance from the
    /// ordinary header or footer. This is distinct from setting an empty
    /// vector, which deliberately suppresses inherited furniture.
    ClearPageFurnitureOverride {
        slot: HeaderFooterSlot,
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
    /// Adds a row at `position`. Anchoring is by identity, never by index:
    /// an index means different things on two replicas, an identity does not.
    /// [`InsertPosition::First`] is how "above the first row" is said — the
    /// older `after: Option<_>` spelling could not say it, because `None`
    /// already means *append*.
    ///
    /// `cell_columns` says, for each cell in `row`, **which column it was
    /// written into** — keyed on the cell's own id, so there is no parallel
    /// list whose length could drift from `row.cells`. The row is generated
    /// against the columns one replica can see; by the time the merge applies
    /// it another replica may have deleted a column before them, and without
    /// the binding every cell silently moves one column left. ADR 0019.
    ///
    /// An **empty** map means the operation predates the binding. It is then
    /// read positionally — exactly as it was written — and the merge says so
    /// with a `legacy-table-row-binding` warning, the way a repository with a
    /// pre-split operation sequence reports `legacy-operation-sequence-gap`.
    InsertTableRow {
        table_block_id: StableId,
        position: InsertPosition,
        row: TableRow,
        #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
        cell_columns: std::collections::BTreeMap<StableId, StableId>,
    },
    DeleteTableRow {
        table_block_id: StableId,
        row_id: StableId,
    },
    /// Adds a cell to one row at `position` — the same anchoring
    /// [`InsertTableRow`] uses, for the same reason.
    ///
    /// [`InsertTableRow`]: OperationKind::InsertTableRow
    InsertTableCell {
        table_block_id: StableId,
        row_id: StableId,
        position: InsertPosition,
        cell: TableCell,
    },
    DeleteTableCell {
        table_block_id: StableId,
        row_id: StableId,
        cell_id: StableId,
    },
    /// Adds a column at `position` — the same anchoring [`InsertTableRow`]
    /// uses, for the same reason.
    ///
    /// The cells the new column needs in each row are **not** in the payload.
    /// They are derived from the row and column ids
    /// ([`TableCell::filling`]), so a row another actor inserted concurrently
    /// gets its cell too, and gets the same one on every replica.
    ///
    /// [`InsertTableRow`]: OperationKind::InsertTableRow
    InsertTableColumn {
        table_block_id: StableId,
        position: InsertPosition,
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
    /// `None` returns the row to content-driven height.
    SetTableRowHeight {
        table_block_id: StableId,
        row_id: StableId,
        height: Option<Length>,
    },
    /// Whether this row is a semantic table header.
    SetTableRowHeader {
        table_block_id: StableId,
        row_id: StableId,
        header: bool,
    },
    /// Reorders every row by its stable identity. The application derives the
    /// order from a chosen column; the replicated operation records the exact
    /// result so replay and undo never re-run a locale-dependent comparison.
    ReorderTableRows {
        table_block_id: StableId,
        row_ids: Vec<StableId>,
    },
    /// The inherited border for unstated table-cell edges. `None` returns to
    /// document silence; `CellBorder::none()` is an explicit borderless table.
    SetTableBorder {
        table_block_id: StableId,
        border: Option<opendoc_core::CellBorder>,
    },
    /// Positions a fixed-width table in its text column. `None` returns to
    /// direction-relative start alignment.
    SetTableAlignment {
        table_block_id: StableId,
        alignment: Option<opendoc_core::TableAlignment>,
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
