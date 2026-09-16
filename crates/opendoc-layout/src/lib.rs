//! Deterministic document layout and pagination.
//!
//! This crate answers one question: **which page does each block land on, and
//! where**. It answers it in Rust, from the document and its `PageSetup`
//! alone, with no host measurement and no floating point — so the browser,
//! the Tauri shell and a headless export all get the same answer for the same
//! input. It supersedes the browser-side `paginate()` of ADR 0009; the
//! reasoning is in `docs/adr/0014-pagination-in-rust.md`.
//!
//! The design rests on three things:
//!
//! 1. **The font is bundled** ([`font`]). Metrics computed against a font the
//!    browser does not have would be metrics for a document nobody sees, so
//!    the same subset ships as TrueType for this crate and as WOFF2 for the
//!    stylesheet, out of one generator run.
//! 2. **The type scale is projected, not duplicated** ([`style`]). The sizes
//!    and spacing measured here are the ones the stylesheet uses, because the
//!    stylesheet reads them as custom properties this crate writes.
//! 3. **Widths are exact integers** ([`text`]). A line's width is a sum of
//!    `advance_units * font_size_twips`, compared against
//!    `available_twips * units_per_em`. Nothing is divided, so nothing
//!    rounds, so two machines break a paragraph identically.
//!
//! Internally every vertical length is in **milli-twips** (20 000 to the
//! point). Twips alone would round a 1.15 line height on an 11pt paragraph;
//! milli-twips keep leading exact and are still integers.
//!
//! ## What is laid out exactly, and what is estimated
//!
//! Paragraphs, headings, list items (including nesting and checkboxes) and
//! explicit page breaks are laid out exactly: their height is a line count
//! times a leading, both computed from the bundled metrics.
//!
//! Tables, images without a stated height, block equations, and any text
//! outside the bundled font's coverage are **estimated**, and say so:
//! [`BlockPlacement::exact`] is false for them and for every block whose page
//! assignment depends on one. Nothing here silently guesses.

mod borders;
pub mod cache;
pub mod font;
pub mod lists;
pub mod paint;
pub mod style;
pub mod text;

use std::collections::BTreeMap;

use opendoc_core::{
    Alignment, Block, BlockKind, BulletListMarker, Document, HeaderFooterSlot, Inline, LineSpacing,
    ListKind, ListProperties, Mark, MarkKind, ModelWarning, PageNumberField, PageSetup,
    PositionedImageAnchor, PositionedImageLayer, StableId, TableCell, TableColumn, TableRow,
    TextDirection,
};

use borders::Border;
pub use cache::{CacheStats, LayoutCache};
use font::{layout_units_to_milli_twips, FaceId, Fonts, Script, TextStyle};
pub use lists::{
    bullet_glyph, list_style_type, list_style_type_rules, marker_cycle, ordered_marker, ListEdge,
    ListItemNumber, ListMarker, ListNumbering, OpenList, STYLED_LIST_DEPTHS,
};
pub use paint::{
    Estimate, EstimateReason, PaintItem, PaintRun, PaintedDocument, PaintedPage, Rgb, RunDecoration,
};
use paint::{Local, LocalRun};
use style::TypeScale;
use text::{Item, Leading, Piece, Strut};

/// Milli-twips in one twip.
const MILLI: i64 = 1_000;

/// Where one block ended up.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlockPlacement {
    pub block_id: String,
    /// Zero-based page index.
    pub page: u32,
    /// Top of the block's border box, in twips from the top of the sheet
    /// stack with **no page gutter**: the viewer adds its own gutter, which
    /// is a property of the screen and not of the document.
    pub top_twips: i32,
    /// Border-box height in twips.
    pub height_twips: i32,
    /// Line boxes the block's own content occupies. Zero for a block whose
    /// height is not a line count (a table, an image, a page-break rule).
    pub lines: u32,
    /// Set on the block that opens a page: the `margin-top` the frontend must
    /// apply so the block's border box starts exactly where this crate says
    /// it does. Expressed in milli-twips; [`BlockPlacement::page_break_css`]
    /// turns it into the exact CSS length.
    pub page_break_margin: Option<i64>,
    /// False when this block's height, or the height of anything above it on
    /// the page, was estimated rather than measured.
    pub exact: bool,
}

impl BlockPlacement {
    /// The margin that opens a page, as a CSS length — exact, because a
    /// milli-twip is five decimal places of a point and no more.
    pub fn page_break_css(&self) -> Option<String> {
        self.page_break_margin.map(style::css_pt_milli)
    }
}

/// The whole layout of one document.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocumentLayout {
    /// Always at least one: an empty document is one empty page.
    pub page_count: u32,
    pub blocks: Vec<BlockPlacement>,
    /// True when every block's height was measured rather than estimated.
    pub exact: bool,
}

impl DocumentLayout {
    pub fn placement(&self, block_id: &str) -> Option<&BlockPlacement> {
        self.blocks.iter().find(|block| block.block_id == block_id)
    }
}

/// The type scale as CSS custom properties. The stylesheet reads these rather
/// than repeating the numbers this crate measures with.
pub fn type_scale_css_variables() -> String {
    TypeScale::default().css_variables()
}

/// Lays out a document's body into pages.
///
/// Pure: the same document produces the same result on every platform, every
/// time. Headers and footers are not part of the body flow — they live in the
/// page margins and repeat — so they are not laid out here.
pub fn layout_document(document: &Document) -> DocumentLayout {
    let fonts = Fonts::load();
    engine(&fonts, document, false)
        .layout(
            &document.blocks,
            &document.page_setup,
            &mut MeasureEveryTime,
        )
        .0
}

/// Lays a document out **and draws it**: every page, every line of text, at
/// twip coordinates on the sheet.
///
/// Same engine, same decisions, same numbers as [`layout_document`] — the
/// difference is only that the content is recorded rather than discarded. A
/// consumer with no browser (the PDF writer) therefore draws precisely the
/// document the browser paginates, which is the whole reason ADR 0014 put
/// pagination in Rust.
///
/// Headers and footers are laid out once and repeated on every page with
/// their page-number fields resolved, which is the job the renderer
/// deliberately leaves to whatever paginates.
pub fn layout_painted_document(document: &Document) -> PaintedDocument {
    let fonts = Fonts::load();
    let engine = engine(&fonts, document, true);
    let setup = &document.page_setup;
    let frame = Frame {
        left: 0,
        width: setup.content_width().twips(),
    };
    let mut fragments = engine.fragments(&document.blocks, frame, &mut MeasureEveryTime);
    // The footnote bodies are not part of the page flow on screen — the shell
    // puts them in a `.footnote-area` under the whole page stack — but they
    // are part of the document, and a PDF that dropped them would lose text
    // the user wrote. They are laid out as a trailer to the body, in the same
    // flow, so they land on the last page or on one of their own. The export
    // warns that the placement is the PDF's rather than the screen's.
    let body = fragments.len();
    fragments.extend(engine.footnote_fragments(document, frame));
    let (mut layout, mut pages, estimates) = engine.paginate(&fragments, setup);
    let exact = layout.exact;
    // The trailer's placements are not placements of blocks a caller can name,
    // so they do not go out with the ones that are.
    layout.blocks.truncate(body);
    engine.paint_furniture(document, &layout, &mut pages);
    let warnings = engine.paint_positioned_images(document, &layout, &mut pages, setup);
    PaintedDocument {
        page_count: layout.page_count,
        exact,
        pages,
        blocks: layout.blocks,
        estimates,
        warnings,
    }
}

fn engine<'a>(fonts: &'a Fonts, document: &Document, paint: bool) -> Engine<'a> {
    Engine {
        fonts,
        scale: TypeScale::default(),
        footnote_numbers: footnote_numbers(document),
        list_properties: document.list_properties.clone(),
        toc_entries: toc_entries(document),
        bibliography_entries: bibliography_entries(document),
        // A document carrying suggestions renders extra inline content that
        // is not in `Block::content`; rather than model the suggestion
        // projection twice, the layout says it is estimating.
        suggestions_pending: !document.suggestions.is_empty(),
        paint,
    }
}

/// Where a top-level block's [`Fragment`] comes from.
///
/// The flow walk asks this for every top-level block instead of measuring the
/// block itself, which is the seam [`cache::LayoutCache`] reuses a previous
/// pass through. The implementation here measures every time and is what
/// [`layout_document`] and [`layout_painted_document`] use, so the
/// uncached path is unchanged by the seam existing.
///
/// The unit is exactly "what `text_fragment`/`block_fragment` returns for
/// this block in this frame", which is a pure function of the block, the
/// frame, the type scale, the bundled fonts, whether the document carries
/// suggestions and whether the engine is painting — and of nothing else.
/// Everything that depends on the *run* the block sits in (a list item's
/// marker glyph and ordinal, the bottom margin a finished list run inherits)
/// is applied by the caller to the value this returns, so the run state can
/// never leak into something a cache would have to key on.
pub(crate) trait FragmentSource {
    fn fragment(
        &mut self,
        block: &Block,
        frame: Frame,
        measure: &mut dyn FnMut() -> Fragment,
    ) -> Fragment;
}

/// The plain source: measure the block, every time.
pub(crate) struct MeasureEveryTime;

impl FragmentSource for MeasureEveryTime {
    fn fragment(
        &mut self,
        _block: &Block,
        _frame: Frame,
        measure: &mut dyn FnMut() -> Fragment,
    ) -> Fragment {
        measure()
    }
}

/// One block's contribution to the vertical flow, before pagination.
#[derive(Clone, Debug)]
pub(crate) struct Fragment {
    block_id: String,
    /// Collapsible margin above, in milli-twips.
    margin_top: i64,
    /// Collapsible margin below.
    margin_bottom: i64,
    /// Border-box height.
    height: i64,
    lines: u32,
    exact: bool,
    /// The durable paragraph pagination relationship. It is examined before
    /// placing this fragment, because moving the following block then is too
    /// late: the promise is about the break *after this one*.
    keep_with_next: bool,
    /// True for the rule an explicit page break draws: the block after it
    /// starts a new page.
    breaks_after: bool,
    /// Why *this* block's own height is an estimate. A block that is merely
    /// after an estimated one carries `None`: its page is uncertain, but
    /// nothing about it was guessed.
    estimate: Option<EstimateReason>,
    /// The baseline of the fragment's first line, in milli-twips from the
    /// border-box top. Zero for a fragment with no text. A list marker sits
    /// on it, and the marker is applied by the run walk rather than by the
    /// memoized measurement, so it has to be carried out rather than
    /// recomputed from a leading the block may not have used.
    first_baseline: i64,
    /// What to draw, in this fragment's own frame. Empty unless painting.
    paint: Vec<Local>,
    /// A measured table is expanded into row fragments immediately before the
    /// flow walk. Keeping the measurement as one cacheable value preserves
    /// the cache seam while letting pagination make its decision at a row
    /// boundary.
    table_flow: Option<TableFlow>,
    /// Leading table rows drawn again before this body row when pagination
    /// opens a continuation page.
    repeat_header: Option<RepeatHeader>,
}

#[derive(Clone, Debug)]
struct TableFlow {
    rows: Vec<TableRowFlow>,
}

#[derive(Clone, Debug)]
struct TableRowFlow {
    height: i64,
    header: bool,
    paint: Vec<Local>,
}

#[derive(Clone, Debug)]
struct RepeatHeader {
    height: i64,
    paint: Vec<Local>,
}

/// Derived ownership of every rectangular table-grid position. The model
/// keeps covered cells as real records so splitting a merge loses no content;
/// layout needs the complementary view: exactly one anchor owns each drawn
/// position.
#[derive(Clone, Debug)]
struct TableGrid {
    anchors: Vec<TableAnchor>,
    owner: Vec<Vec<Option<usize>>>,
}

#[derive(Clone, Debug)]
struct TableAnchor {
    row: usize,
    column: usize,
    row_end: usize,
    column_end: usize,
}

impl TableGrid {
    fn new(rows: &[TableRow], columns: usize) -> Self {
        let mut owner = vec![vec![None; columns]; rows.len()];
        let covered = opendoc_core::table_covered_positions(rows);
        let mut anchors = Vec::new();
        for (row, source_row) in rows.iter().enumerate() {
            for (column, cell) in source_row.cells.iter().enumerate().take(columns) {
                if covered.contains(&(row, column)) {
                    continue;
                }
                let anchor = TableAnchor {
                    row,
                    column,
                    row_end: (row + cell.span.rows() as usize).min(rows.len()),
                    column_end: (column + cell.span.columns() as usize).min(columns),
                };
                let index = anchors.len();
                for owned_row in owner.iter_mut().take(anchor.row_end).skip(row) {
                    for owned in owned_row.iter_mut().take(anchor.column_end).skip(column) {
                        *owned = Some(index);
                    }
                }
                anchors.push(anchor);
            }
        }
        Self { anchors, owner }
    }

    fn anchor_at(&self, row: usize, column: usize) -> Option<&TableAnchor> {
        self.owner
            .get(row)?
            .get(column)?
            .and_then(|index| self.anchors.get(index))
    }

    fn is_anchor(&self, row: usize, column: usize) -> bool {
        self.anchor_at(row, column)
            .is_some_and(|anchor| anchor.row == row && anchor.column == column)
    }
}

/// The horizontal box a block is laid out in.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Frame {
    /// Left edge, in twips from the origin paint items are expressed in — the
    /// page's content box at the top level, the cell's content box inside a
    /// table.
    pub(crate) left: i32,
    /// Content width in twips.
    pub(crate) width: i32,
}

/// The image-specific portion of a paint request. Keeping it together avoids
/// a long positional helper signature as image capabilities grow.
struct ImagePaint<'a> {
    blob_hash: &'a str,
    alt_text: &'a str,
    caption: Option<&'a str>,
    border: Option<opendoc_core::CellBorder>,
    rotation_degrees: i16,
    crop: Option<opendoc_core::ImageCrop>,
    opacity_percent: u8,
}

/// Where one table row's cells ended up, kept so the border grid can be
/// walked by *boundary* after every row's height is known. A boundary between
/// two rows belongs to neither of them, which is why it cannot be drawn inside
/// the row loop.
#[derive(Clone, Debug)]
struct RowGeometry {
    /// Milli-twips from the fragment's border-box top.
    top: i64,
    height: i64,
    /// `(left edge, width)` per cell, in twips, exactly as drawn.
    cells: Vec<(i32, i32)>,
}

/// The already-measured content of one cell, retained until the row's final
/// height is known so vertical alignment can use spare space without a second
/// measurement pass.
#[derive(Clone, Debug)]
struct CellPaint {
    x: i32,
    width: i32,
    row_end: usize,
    background: Option<Rgb>,
    content_height: i64,
    top_offset: i64,
    bottom_padding: i64,
    alignment: Option<opendoc_core::VerticalAlignment>,
    items: Vec<Local>,
}

/// One of a cell's four physical edges. Physical, not logical: the model's
/// `start`/`end` are resolved against the table's direction before a boundary
/// is looked up, because a boundary is a place on the page.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CellEdge {
    Top,
    Bottom,
    Left,
    Right,
}

/// The horizontal geometry of one block's lines, after its own indents.
#[derive(Clone, Copy, Debug)]
struct LineFrame {
    /// Left edge of the content, in the frame's coordinates.
    left: i32,
    width: i32,
    first_width: i32,
    /// How far the first line is pushed in. A hanging indent is negative in
    /// the model and widens the first line instead, so it contributes zero
    /// here and the extra width is already in `first_width`.
    first_offset: i32,
}

/// Records the first reason a block's geometry stopped being exact.
///
/// First rather than last on purpose: the earliest cause is the one a reader
/// can act on, and listing every consequence of it would bury it.
fn note(slot: &mut Option<EstimateReason>, reason: Option<EstimateReason>) {
    if slot.is_none() {
        *slot = reason;
    }
}

pub(crate) struct Engine<'a> {
    pub(crate) fonts: &'a Fonts,
    pub(crate) scale: TypeScale,
    pub(crate) suggestions_pending: bool,
    /// Footnote number by footnote id, in order of first reference — the
    /// rule `opendoc-render` numbers by, reproduced here because the number
    /// is *drawn*, so its width decides where a line breaks.
    pub(crate) footnote_numbers: BTreeMap<String, u32>,
    pub(crate) list_properties: BTreeMap<StableId, ListProperties>,
    /// The generated TOC depends on every heading, so it is an explicit
    /// layout input rather than an accidental read from a cache closure.
    pub(crate) toc_entries: Vec<(u8, String)>,
    /// The generated bibliography depends on citation source, style and
    /// locale, so it is explicit cache input like the generated TOC.
    pub(crate) bibliography_entries: Vec<String>,
    /// Whether to record what to draw. Off on the pagination path, which runs
    /// on every keystroke and would otherwise allocate a string per run.
    pub(crate) paint: bool,
}

impl Engine<'_> {
    pub(crate) fn layout(
        &self,
        blocks: &[Block],
        setup: &PageSetup,
        source: &mut dyn FragmentSource,
    ) -> (DocumentLayout, Vec<PaintedPage>, Vec<Estimate>) {
        let frame = Frame {
            left: 0,
            width: setup.content_width().twips(),
        };
        let fragments = self.fragments(blocks, frame, source);
        self.paginate(&fragments, setup)
    }

    // ---- pagination ----------------------------------------------------

    /// Walks the flow, opening a page whenever a block would cross the bottom
    /// of the content box or an explicit break demands it.
    ///
    /// Page *k*'s content box runs from `k * page_height + margin_top` to
    /// `+ content_height`, with the viewer's gutter deliberately left out:
    /// adding it here would put a screen affordance into a document fact, and
    /// the frontend can add `var(--page-gap)` to a length this function
    /// produced without needing to know anything else.
    fn paginate(
        &self,
        fragments: &[Fragment],
        setup: &PageSetup,
    ) -> (DocumentLayout, Vec<PaintedPage>, Vec<Estimate>) {
        let page_height = i64::from(setup.height.twips()) * MILLI;
        let margin_top = i64::from(setup.margin_top.twips()) * MILLI;
        let content_height = i64::from(setup.content_height().twips()) * MILLI;

        let content_left = setup.margin_start.twips();
        let mut pages: Vec<PaintedPage> = Vec::new();
        let mut estimates: Vec<Estimate> = Vec::new();
        let mut placements = Vec::with_capacity(fragments.len());
        let mut page: u32 = 0;
        // Bottom of the previous block's border box.
        let mut pen = margin_top;
        let mut previous_margin_bottom = 0i64;
        let mut page_opened_at = 0usize;
        let mut forced = false;
        // Once a page carries an estimated height, every page number after it
        // is an estimate too.
        let mut exact_so_far = !self.suggestions_pending;

        for (index, fragment) in fragments.iter().enumerate() {
            let mut top = if index == 0 {
                // The first block's margin collapses out of the editable host
                // and pushes the whole flow down, exactly as it does in CSS.
                margin_top + fragment.margin_top
            } else {
                pen + previous_margin_bottom.max(fragment.margin_top)
            };
            let mut page_break_margin = None;
            let page_bottom = i64::from(page) * page_height + margin_top + content_height;
            let mut continuation = false;
            // A block already at the top of its page is never pushed again:
            // a block taller than the content box takes a page and overflows,
            // which is visible and honest, rather than looping forever.
            let next_requires_same_page = fragment.keep_with_next
                && fragments.get(index + 1).is_some_and(|next| {
                    let next_top =
                        top + fragment.height + fragment.margin_bottom.max(next.margin_top);
                    next_top + next.height > page_bottom
                });
            if index > page_opened_at
                && (forced || top + fragment.height > page_bottom || next_requires_same_page)
            {
                page += 1;
                let opened = i64::from(page) * page_height + margin_top;
                // The margin is measured from the previous border box, so it
                // is what CSS margin collapsing will actually produce: it is
                // always larger than the margin-bottom it collapses with,
                // because it spans at least one page's bottom and top margin.
                page_break_margin = Some(opened - pen);
                top = opened;
                page_opened_at = index;
                continuation = true;
            }
            // Header rows are source state, not a PDF decoration. A body row
            // which had to move to another page therefore brings the exact
            // leading header fragment the table measurement produced. This
            // is deliberately here, beside the page-break decision, rather
            // than in the PDF writer: page geometry has one owner.
            if continuation {
                if let Some(header) = &fragment.repeat_header {
                    if self.paint {
                        let page_top = i64::from(page) * page_height;
                        let sheet = page as usize;
                        if pages.len() <= sheet {
                            pages.resize_with(sheet + 1, PaintedPage::default);
                        }
                        for item in &header.paint {
                            pages[sheet]
                                .items
                                .push(item.placed(content_left, top - page_top));
                        }
                    }
                    top += header.height;
                }
            }
            exact_so_far &= fragment.exact;
            if let Some(reason) = fragment.estimate {
                estimates.push(Estimate {
                    block_id: fragment.block_id.clone(),
                    reason,
                });
            }
            if !fragment.paint.is_empty() {
                let page_top = i64::from(page) * page_height;
                let sheet = page as usize;
                if pages.len() <= sheet {
                    pages.resize_with(sheet + 1, PaintedPage::default);
                }
                for item in &fragment.paint {
                    pages[sheet]
                        .items
                        .push(item.placed(content_left, top - page_top));
                }
            }
            placements.push(BlockPlacement {
                block_id: fragment.block_id.clone(),
                page,
                top_twips: to_twips(top),
                height_twips: to_twips(fragment.height),
                lines: fragment.lines,
                page_break_margin,
                exact: exact_so_far,
            });
            pen = top + fragment.height;
            previous_margin_bottom = fragment.margin_bottom;
            forced = fragment.breaks_after;
        }

        let page_count = page + 1;
        if self.paint {
            pages.resize_with(page_count as usize, PaintedPage::default);
        }
        // A row is an internal flow fragment, not a second document block.
        // Keep the public layout's one-placement-per-block contract for the
        // editor/DTO boundary; the page count and all later block positions
        // have already been decided from the full row stream above.
        let mut logical_placements = Vec::with_capacity(placements.len());
        let mut seen = BTreeMap::new();
        for placement in placements {
            if seen
                .insert(placement.block_id.clone(), logical_placements.len())
                .is_none()
            {
                logical_placements.push(placement);
            }
        }
        (
            DocumentLayout {
                page_count,
                exact: logical_placements
                    .last()
                    .map(|last| last.exact)
                    .unwrap_or(true),
                blocks: logical_placements,
            },
            pages,
            estimates,
        )
    }

    /// Adds out-of-flow images after normal pagination has established every
    /// anchor's sheet and border-box origin.  Positioned objects deliberately
    /// contribute no flow height: their dimensions must not push text merely
    /// because a renderer happens to support them.
    ///
    /// `BlockPlacement` names top-level block border boxes.  That is exactly
    /// the stable block namespace an ADR 0022 positioned image can currently
    /// target; an absent target falls back to the page-content origin on the
    /// image's own page and remains visible rather than being retargeted to a
    /// nearby sibling.
    fn paint_positioned_images(
        &self,
        document: &Document,
        layout: &DocumentLayout,
        pages: &mut [PaintedPage],
        setup: &PageSetup,
    ) -> Vec<ModelWarning> {
        let mut behind: Vec<Vec<PaintItem>> = vec![Vec::new(); pages.len()];
        let mut warnings = Vec::new();
        let default_width = setup.content_width().twips();

        for block in &document.blocks {
            let BlockKind::Image {
                blob_hash,
                alt_text,
                layout: image_layout,
            } = &block.kind
            else {
                continue;
            };
            let Some(positioned) = &image_layout.positioned else {
                continue;
            };
            let Some(own_placement) = layout.placement(block.id.as_str()) else {
                // This can only happen if a caller supplied a document whose
                // top-level sequence changed during layout.  Keep the guard
                // total rather than inventing a page index.
                continue;
            };

            let (page, origin_x, origin_y) = match &positioned.anchor {
                PositionedImageAnchor::PageContent => (
                    own_placement.page,
                    setup.margin_start.twips(),
                    setup.margin_top.twips(),
                ),
                PositionedImageAnchor::Block(anchor) => {
                    if let Some(target) = layout.placement(anchor.as_str()) {
                        (target.page, setup.margin_start.twips(), target.top_twips)
                    } else {
                        warnings.push(ModelWarning {
                            code: "positioned-image-anchor-fallback".to_string(),
                            message: format!(
                                "positioned image {} anchors to missing block {}; page-content on its own page was used",
                                block.id, anchor
                            ),
                        });
                        (
                            own_placement.page,
                            setup.margin_start.twips(),
                            setup.margin_top.twips(),
                        )
                    }
                }
            };
            let width = image_layout
                .width
                .map(|value| value.twips())
                .unwrap_or(default_width);
            // The layout pass intentionally does not decode blobs.  This is
            // the same deterministic fallback rectangle used for in-flow
            // unsized images; a stated height always wins.
            let height = image_layout
                .height
                .map(|value| value.twips())
                .unwrap_or(default_width / 2);
            let item = PaintItem::Image {
                blob_hash: blob_hash.to_string(),
                alt_text: alt_text.to_string(),
                x_twips: origin_x + positioned.horizontal_offset.twips(),
                y_twips: origin_y + positioned.vertical_offset.twips(),
                width_twips: width,
                height_twips: height,
                rotation_degrees: image_layout.rotation_degrees.unwrap_or(0),
                crop: image_layout.crop,
                opacity_percent: image_layout.opacity_percent.unwrap_or(100),
            };
            let Some(sheet) = pages.get_mut(page as usize) else {
                continue;
            };
            match positioned.layer {
                PositionedImageLayer::BehindText => behind[page as usize].push(item),
                PositionedImageLayer::InFrontOfText => sheet.items.push(item),
            }
        }
        for (sheet, mut items) in pages.iter_mut().zip(behind) {
            if !items.is_empty() {
                items.append(&mut sheet.items);
                sheet.items = items;
            }
        }
        warnings
    }

    // ---- block boxes ---------------------------------------------------

    /// Turns a sequence of blocks into fragments, resolving list runs.
    ///
    /// The list structure is the one `opendoc-render` writes: a maximal run of
    /// adjacent list items becomes nested `<ul>`/`<ol>` elements, and the
    /// stack discipline below is the same one `ListWriter` uses, because the
    /// indent a given item ends up with depends on it.
    fn fragments(
        &self,
        blocks: &[Block],
        frame: Frame,
        source: &mut dyn FragmentSource,
    ) -> Vec<Fragment> {
        let mut out: Vec<Fragment> = Vec::with_capacity(blocks.len());
        // The wrapper stack and the ordinals in it, run by the one
        // implementation `opendoc-render` writes its `<ol>`s from.
        let mut lists = ListNumbering::default();
        // Index into `out` of the first item of the list run currently open.
        let mut run_start: Option<usize> = None;

        for block in blocks {
            match &block.kind {
                BlockKind::ListItem {
                    list_id,
                    level,
                    kind: list_kind,
                } => {
                    let marker = ListMarker::of(*list_kind);
                    // Nothing is drawn as a wrapper here, so the edges are
                    // not listened to: the indent comes from the stack they
                    // leave behind and the space from `closed_root`.
                    let start = self
                        .list_properties
                        .get(list_id)
                        .map(|properties| properties.start_for(*level))
                        .unwrap_or(1);
                    let format = self
                        .list_properties
                        .get(list_id)
                        .map(|properties| properties.format_for(*level))
                        .unwrap_or_else(|| opendoc_core::OrderedListFormat::inherited_at(*level));
                    let bullet_marker = self
                        .list_properties
                        .get(list_id)
                        .map(|properties| properties.bullet_marker_for(*level))
                        .unwrap_or_else(|| BulletListMarker::inherited_at(*level));
                    let number =
                        lists.open_item_with_start(list_id, *level, marker, start, &mut |_| {});
                    if number.closed_root {
                        // `ListWriter` closes the outermost `.doc-list` and
                        // opens the next one whenever the marker or the list
                        // changes at a level — a bulleted run followed by an
                        // ordered one is two wrappers, not one. The closed
                        // wrapper's `margin-bottom` collapses against the new
                        // wrapper's zero `margin-top`, so the space below the
                        // finished sub-run is real and the flow has to
                        // account for it.
                        self.close_list_run(&mut out);
                    }
                    let indent: i32 = lists
                        .open_levels()
                        .iter()
                        .map(|open| match open.marker {
                            ListMarker::Checklist => self.scale.checklist_indent,
                            _ => self.scale.list_indent,
                        })
                        .sum();
                    let inner = Frame {
                        left: frame.left + indent,
                        width: (frame.width - indent).max(1),
                    };
                    // Only the block and its frame reach the source; the
                    // marker and the run's trailing margin are applied to
                    // what comes back, so nothing that depends on the run
                    // state is inside the memoized unit.
                    let mut fragment = source.fragment(block, inner, &mut || {
                        self.text_fragment(block, inner, self.scale.body_size)
                    });
                    if self.paint {
                        self.paint_list_marker(
                            marker,
                            format,
                            bullet_marker,
                            number.ordinal,
                            inner.left,
                            &mut fragment,
                        );
                    }
                    // `li` carries no margin of its own: the space around a
                    // list belongs to the wrapper, and is put back on the run
                    // as a whole by `close_list_run`. Items butt together.
                    if block.properties.space_after.is_none() {
                        fragment.margin_bottom = 0;
                    }
                    if run_start.is_none() {
                        run_start = Some(out.len());
                    }
                    out.push(fragment);
                }
                _ => {
                    if run_start.take().is_some() {
                        self.close_list_run(&mut out);
                        lists.close_all(&mut |_| {});
                    }
                    let fragment =
                        source.fragment(block, frame, &mut || self.block_fragment(block, frame));
                    out.extend(self.expand_table_fragment(fragment));
                }
            }
        }
        if run_start.is_some() {
            self.close_list_run(&mut out);
        }
        out
    }

    /// Gives a finished list run the wrapper's bottom margin.
    ///
    /// `.doc-list` has `margin: 0 0 <block space>` and nested lists have none,
    /// so whatever depth the run ends at, the space below it is the outer
    /// wrapper's and it collapses onto the last item.
    fn close_list_run(&self, out: &mut [Fragment]) {
        if let Some(last) = out.last_mut() {
            last.margin_bottom = last
                .margin_bottom
                .max(i64::from(self.scale.block_space_after) * MILLI);
        }
    }

    /// Turns the cacheable measurement of a table into flow fragments. The
    /// headers remain one atomic leading fragment; every body row becomes a
    /// legal page-break boundary and carries the already-measured header for
    /// use when it starts a continuation page.
    fn expand_table_fragment(&self, mut fragment: Fragment) -> Vec<Fragment> {
        let Some(flow) = fragment.table_flow.take() else {
            return vec![fragment];
        };
        let header_count = flow.rows.iter().take_while(|row| row.header).count();
        let (header_rows, body_rows) = flow.rows.split_at(header_count);
        let header = if header_rows.is_empty() {
            None
        } else {
            let mut height = 0;
            let mut paint = Vec::new();
            for row in header_rows {
                paint.extend(row.paint.iter().map(|item| item.translated(0, height)));
                height += row.height;
            }
            Some(RepeatHeader { height, paint })
        };
        let mut out = Vec::with_capacity(body_rows.len() + usize::from(header.is_some()));
        if let Some(header) = &header {
            out.push(Fragment {
                block_id: fragment.block_id.clone(),
                margin_top: fragment.margin_top,
                margin_bottom: if body_rows.is_empty() {
                    fragment.margin_bottom
                } else {
                    0
                },
                height: header.height,
                lines: 0,
                exact: fragment.exact,
                keep_with_next: false,
                breaks_after: false,
                estimate: fragment.estimate,
                first_baseline: 0,
                paint: header.paint.clone(),
                table_flow: None,
                repeat_header: None,
            });
        }
        for (index, row) in body_rows.iter().enumerate() {
            out.push(Fragment {
                block_id: fragment.block_id.clone(),
                margin_top: if index == 0 && header.is_none() {
                    fragment.margin_top
                } else {
                    0
                },
                margin_bottom: if index + 1 == body_rows.len() {
                    fragment.margin_bottom
                } else {
                    0
                },
                height: row.height,
                lines: 0,
                exact: fragment.exact,
                keep_with_next: false,
                breaks_after: false,
                estimate: if index == 0 && header.is_none() {
                    fragment.estimate
                } else {
                    None
                },
                first_baseline: 0,
                paint: row.paint.clone(),
                table_flow: None,
                repeat_header: header.clone(),
            });
        }
        out
    }

    fn block_fragment(&self, block: &Block, frame: Frame) -> Fragment {
        match &block.kind {
            BlockKind::Paragraph => self.text_fragment(block, frame, self.scale.body_size),
            BlockKind::Title => self.text_fragment(block, frame, self.scale.title_size),
            BlockKind::Subtitle => self.text_fragment(block, frame, self.scale.subtitle_size),
            BlockKind::Heading { level } => {
                let mut fragment = self.text_fragment(block, frame, self.scale.heading(*level));
                if block.properties.space_before.is_none() {
                    fragment.margin_top = i64::from(self.scale.heading_space(*level)) * MILLI;
                }
                fragment
            }
            BlockKind::ListItem { .. } => {
                // Reached only for a lone item outside `fragments`' run walk.
                self.text_fragment(block, frame, self.scale.body_size)
            }
            BlockKind::PageBreak => Fragment {
                block_id: block.id.to_string(),
                margin_top: i64::from(self.scale.page_break_space) * MILLI,
                margin_bottom: i64::from(self.scale.page_break_space) * MILLI,
                height: i64::from(self.scale.page_break_rule) * MILLI,
                lines: 0,
                exact: !self.suggestions_pending,
                keep_with_next: false,
                breaks_after: true,
                estimate: self.suggestion_estimate(),
                first_baseline: 0,
                paint: if self.paint {
                    // `.doc-page-break` is a *dashed* border-top on screen, so
                    // a solid bar on paper would be a different mark from the
                    // one the document carries.
                    vec![Local::Fill {
                        x: frame.left,
                        y: 0,
                        w: frame.width,
                        h: i64::from(self.scale.page_break_rule) * MILLI,
                        color: None,
                        dashed: true,
                    }]
                } else {
                    Vec::new()
                },
                table_flow: None,
                repeat_header: None,
            },
            BlockKind::HorizontalRule => Fragment {
                block_id: block.id.to_string(),
                margin_top: 12 * 20 * MILLI,
                margin_bottom: 12 * 20 * MILLI,
                height: i64::from(self.scale.page_break_rule) * MILLI,
                lines: 0,
                exact: !self.suggestions_pending,
                keep_with_next: false,
                breaks_after: false,
                estimate: self.suggestion_estimate(),
                first_baseline: 0,
                paint: if self.paint {
                    vec![Local::Fill {
                        x: frame.left,
                        y: 0,
                        w: frame.width,
                        h: i64::from(self.scale.page_break_rule) * MILLI,
                        color: None,
                        dashed: false,
                    }]
                } else {
                    Vec::new()
                },
                table_flow: None,
                repeat_header: None,
            },
            BlockKind::TableOfContents { max_level } => {
                let entries = self
                    .toc_entries
                    .iter()
                    .filter(|(level, _)| level <= max_level)
                    .map(|(level, text)| {
                        format!("{}{}", "  ".repeat(usize::from(*level - 1)), text)
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                let projected = Block {
                    id: block.id.clone(),
                    kind: BlockKind::Paragraph,
                    content: vec![Inline::text(if entries.is_empty() {
                        "Table of contents".to_string()
                    } else {
                        format!("Table of contents\n{entries}")
                    })],
                    properties: block.properties.clone(),
                };
                self.text_fragment(&projected, frame, self.scale.body_size)
            }
            BlockKind::Bibliography => {
                let entries = self.bibliography_entries.join("\n");
                let projected = Block {
                    id: block.id.clone(),
                    kind: BlockKind::Paragraph,
                    content: vec![Inline::text(if entries.is_empty() {
                        "Bibliography".to_string()
                    } else {
                        format!("Bibliography\n{entries}")
                    })],
                    properties: block.properties.clone(),
                };
                self.text_fragment(&projected, frame, self.scale.body_size)
            }
            BlockKind::Image {
                blob_hash,
                alt_text,
                layout,
            } => {
                // ADR 0022 objects participate in the document sequence so
                // they retain a deterministic fallback page, but they are
                // not floats: reserving their rectangle in the vertical flow
                // would move text and contradict both layer choices.  Their
                // actual paint is installed once pagination has resolved the
                // target block's page and top edge.
                if layout.positioned.is_some() {
                    return Fragment {
                        block_id: block.id.to_string(),
                        margin_top: 0,
                        margin_bottom: 0,
                        height: 0,
                        lines: 0,
                        exact: !self.suggestions_pending,
                        keep_with_next: false,
                        breaks_after: false,
                        estimate: self.suggestion_estimate(),
                        first_baseline: 0,
                        paint: Vec::new(),
                        table_flow: None,
                        repeat_header: None,
                    };
                }
                let space = i64::from(self.scale.float_block_space_before) * MILLI;
                // A stated height is a document fact and is honoured exactly.
                // Without one the drawn size depends on the image's own
                // pixels, which this crate does not have — so it is an
                // estimate and says so.
                let (height, exact) = match layout.height {
                    Some(height) => (i64::from(height.twips()) * MILLI, true),
                    None => (i64::from(frame.width) * MILLI / 2, false),
                };
                let caption = if layout.caption.is_none() {
                    0
                } else {
                    self.leading(self.scale.caption_size, None)
                };
                let estimate = if exact {
                    self.suggestion_estimate()
                } else {
                    Some(EstimateReason::ImageWithoutHeight)
                };
                Fragment {
                    block_id: block.id.to_string(),
                    margin_top: space,
                    margin_bottom: space,
                    height: height + caption,
                    lines: 0,
                    exact: estimate.is_none(),
                    keep_with_next: false,
                    breaks_after: false,
                    estimate,
                    first_baseline: 0,
                    paint: if self.paint {
                        self.paint_image(
                            ImagePaint {
                                blob_hash,
                                alt_text,
                                caption: layout.caption.as_deref(),
                                border: layout.border,
                                rotation_degrees: layout.rotation_degrees.unwrap_or(0),
                                crop: layout.crop,
                                opacity_percent: layout.opacity_percent.unwrap_or(100),
                            },
                            frame,
                            height,
                        )
                    } else {
                        Vec::new()
                    },
                    table_flow: None,
                    repeat_header: None,
                }
            }
            BlockKind::EquationBlock { equation, .. } => {
                let space = i64::from(self.scale.float_block_space_before) * MILLI;
                let leading = self.leading(self.scale.body_size, None);
                Fragment {
                    block_id: block.id.to_string(),
                    margin_top: space,
                    margin_bottom: space,
                    // A rendered MathML box's height depends on the browser's
                    // math layout, which is not modelled: one line is the
                    // floor, and the estimate is flagged.
                    height: leading,
                    lines: 0,
                    exact: false,
                    keep_with_next: false,
                    breaks_after: false,
                    estimate: Some(EstimateReason::Equation),
                    first_baseline: 0,
                    // On paper the equation is its source, set in the mono
                    // face: there is no math typesetter here, and printing
                    // nothing would lose the content silently.
                    paint: if self.paint {
                        self.paint_equation_source(&equation.source, frame, leading)
                    } else {
                        Vec::new()
                    },
                    table_flow: None,
                    repeat_header: None,
                }
            }
            BlockKind::Table {
                columns,
                properties,
                rows,
            } => self.table_fragment(block, columns, properties, rows, frame),
        }
    }

    /// A block whose height is the sum of its line boxes.
    ///
    /// Not a line count times one leading: a line is as tall as the tallest
    /// inline box on it, and a run carrying a size mark or a monospace face
    /// is a taller box than the block's own strut. Chrome computes the union;
    /// so does this.
    fn text_fragment(&self, block: &Block, frame: Frame, size_twips: i32) -> Fragment {
        let properties = &block.properties;
        let indent_start = properties.indent_start.map(|l| l.twips()).unwrap_or(0);
        let indent_end = properties.indent_end.map(|l| l.twips()).unwrap_or(0);
        let first_indent = properties.indent_first_line.map(|l| l.twips()).unwrap_or(0);
        // CSS puts a paragraph's uniform border inside its margin box. Reserve
        // that used width before breaking lines, otherwise PDF would paint a
        // frame around text that the screen correctly wrapped more narrowly.
        let border = properties
            .border
            .map(Border::stated)
            .filter(|border| border.style != opendoc_core::BorderStyle::None && border.width > 0);
        let border_width = border.map(|border| border.width).unwrap_or(0);
        let outer_left = frame.left + indent_start;
        let outer_width = (frame.width - indent_start - indent_end).max(1);
        let width = (outer_width - 2 * border_width).max(1);
        // A positive first-line indent narrows the first line; a negative one
        // (the model's only hanging indent) widens it. Same sign convention as
        // CSS `text-indent`, which the renderer projects it to.
        let first_width = (width - first_indent).max(1);

        let mut items = Vec::new();
        let mut estimate = self.suggestion_estimate();
        let checklist = matches!(
            &block.kind,
            BlockKind::ListItem {
                kind: ListKind::Checklist { .. },
                ..
            }
        );
        if checklist {
            // The checkbox is an inline box before the item's text.
            let gap = size_twips * self.scale.checkbox_gap_thousandths as i32 / 1_000;
            items.push(Item::Box {
                width_twips: self.scale.checkbox_size + gap,
                exact: true,
            });
        }
        for inline in &block.content {
            self.push_inline(inline, size_twips, &mut items, &mut estimate);
        }
        let strut = Strut::new(size_twips, self.leading_of(properties.line_spacing));
        let (measured, lines) = if self.paint {
            text::break_lines(&items, self.fonts, first_width, width, strut)
        } else {
            (
                text::measure(&items, self.fonts, first_width, width, strut),
                Vec::new(),
            )
        };
        if !measured.exact && estimate.is_none() {
            estimate = Some(EstimateReason::TextOutsideBundledFont);
        }
        let height = layout_units_to_milli_twips(measured.height_units);
        // The first line's baseline, which the list marker sits on. Computed
        // from the strut when the block was not painted, because the run walk
        // asks for it either way and the strut is the floor of every line.
        let first_baseline = match lines.first() {
            Some(line) => layout_units_to_milli_twips(line.extent.above),
            None => layout_units_to_milli_twips(strut.line_extent(self.fonts).above),
        };
        let paint = if self.paint {
            self.paint_lines(
                block,
                &lines,
                LineFrame {
                    left: outer_left + border_width,
                    width,
                    first_width,
                    first_offset: first_indent.max(0),
                },
                checklist,
            )
        } else {
            Vec::new()
        };
        // Paint under text, then the border above it. The border's centre is
        // half its used width inside the border box, matching CSS's box model
        // rather than a table's collapsed-boundary rule.
        let border_height = i64::from(border_width) * 2 * MILLI;
        let mut paint = paint
            .into_iter()
            .map(|item| item.translated(0, i64::from(border_width) * MILLI))
            .collect::<Vec<_>>();
        let total_height = height + border_height;
        if let Some(background) = properties.background {
            let (red, green, blue) = background.rgb();
            paint.insert(
                0,
                Local::Fill {
                    x: outer_left,
                    y: 0,
                    w: outer_width,
                    h: total_height,
                    color: Some(Rgb { red, green, blue }),
                    dashed: false,
                },
            );
        }
        if let Some(border) = border {
            let half = border.width / 2;
            let left = outer_left + half;
            let right = outer_left + outer_width - half;
            let top = i64::from(half) * MILLI;
            let bottom = total_height - i64::from(half) * MILLI;
            paint.extend(borders::draw(border, (left, top), (right, top)));
            paint.extend(borders::draw(border, (right, top), (right, bottom)));
            paint.extend(borders::draw(border, (right, bottom), (left, bottom)));
            paint.extend(borders::draw(border, (left, bottom), (left, top)));
        }
        Fragment {
            block_id: block.id.to_string(),
            margin_top: i64::from(properties.space_before.map(|l| l.twips()).unwrap_or(0)) * MILLI,
            margin_bottom: i64::from(
                properties
                    .space_after
                    .map(|l| l.twips())
                    .unwrap_or(self.scale.block_space_after),
            ) * MILLI,
            height: total_height,
            lines: measured.lines,
            exact: estimate.is_none(),
            keep_with_next: properties.keep_with_next.unwrap_or(false),
            breaks_after: false,
            estimate,
            first_baseline: first_baseline + i64::from(border_width) * MILLI,
            paint,
            table_flow: None,
            repeat_header: None,
        }
    }

    /// The footnote bodies, as a trailer to the body flow.
    ///
    /// A rule and then one numbered paragraph per referenced footnote, set at
    /// the caption size the stylesheet sets `.footnote-area` at and numbered
    /// by the same order-of-first-reference rule the renderer uses. A
    /// footnote nothing references is not drawn, because nothing on screen
    /// draws it either.
    fn footnote_fragments(&self, document: &Document, frame: Frame) -> Vec<Fragment> {
        if !self.paint {
            return Vec::new();
        }
        let mut numbered: Vec<(u32, &opendoc_core::Footnote)> = document
            .footnotes
            .iter()
            .filter(|footnote| !footnote.deleted)
            .filter_map(|footnote| {
                self.footnote_numbers
                    .get(footnote.id.as_str())
                    .map(|number| (*number, footnote))
            })
            .collect();
        numbered.sort_by_key(|(number, _)| *number);
        if numbered.is_empty() {
            return Vec::new();
        }
        let rule = i64::from(self.scale.page_break_rule) * MILLI;
        let mut out = vec![Fragment {
            block_id: "footnote-rule".to_string(),
            margin_top: i64::from(self.scale.footnote_space_before) * MILLI,
            margin_bottom: i64::from(self.scale.footnote_space_before) * MILLI,
            height: rule,
            lines: 0,
            exact: !self.suggestions_pending,
            keep_with_next: false,
            breaks_after: false,
            estimate: self.suggestion_estimate(),
            first_baseline: 0,
            paint: vec![Local::Fill {
                x: frame.left,
                y: 0,
                w: frame.width,
                h: rule,
                color: None,
                dashed: false,
            }],
            table_flow: None,
            repeat_header: None,
        }];
        let indent = self.scale.footnote_indent;
        let inner = Frame {
            left: frame.left + indent,
            width: (frame.width - indent).max(1),
        };
        for (number, footnote) in numbered {
            let block = Block {
                id: footnote.id.clone(),
                kind: BlockKind::Paragraph,
                content: footnote.body.clone(),
                properties: opendoc_core::BlockProperties::default(),
            };
            let mut fragment = self.text_fragment(&block, inner, self.scale.caption_size);
            let style = TextStyle::new(self.scale.caption_size);
            let text = format!("{number}.");
            let width = self.fonts.text_advance_twips(&text, style);
            let gap = self.scale.caption_size / 4;
            fragment.paint.insert(
                0,
                Local::Text {
                    baseline: fragment.first_baseline,
                    runs: vec![LocalRun {
                        x: inner.left - width - gap,
                        text,
                        style,
                        decoration: RunDecoration::default(),
                        width,
                    }],
                },
            );
            out.push(fragment);
        }
        out
    }

    fn suggestion_estimate(&self) -> Option<EstimateReason> {
        self.suggestions_pending
            .then_some(EstimateReason::Suggestions)
    }

    /// Places the broken lines of one block.
    ///
    /// Nothing is re-measured here: the line contents, their heights and the
    /// baselines are the ones pagination used, and the only arithmetic is
    /// where on the line each run starts.
    fn paint_lines(
        &self,
        block: &Block,
        lines: &[text::LineBox],
        frame: LineFrame,
        checklist: bool,
    ) -> Vec<Local> {
        let rtl = block.properties.direction == Some(TextDirection::RightToLeft);
        let alignment = block.properties.alignment.unwrap_or(Alignment::Start);
        let mut out = Vec::with_capacity(lines.len());
        // Only the first box of a checklist item is the checkbox; an equation
        // later on the same line is a box too.
        let mut checkbox_pending = checklist;
        // Line boxes stack, and they are not all the same height once a run
        // carries its own size, so the pen walks down rather than multiplying.
        let mut top = 0i64;
        for (index, line) in lines.iter().enumerate() {
            let baseline = top + layout_units_to_milli_twips(line.extent.above);
            top += layout_units_to_milli_twips(line.extent.height());
            let available = if index == 0 {
                frame.first_width
            } else {
                frame.width
            };
            let indent = if index == 0 { frame.first_offset } else { 0 };
            let slack = (available - line.width_twips).max(0);
            // `Start` and `End` are direction-relative in the model, exactly
            // as the CSS keywords of the same name are. Bidi *reordering* is
            // not attempted — see the module docs — so a right-to-left line is
            // placed at the right edge but its runs stay in logical order.
            let offset = match (alignment, rtl) {
                (Alignment::Start, false) | (Alignment::Justify, _) | (Alignment::End, true) => 0,
                (Alignment::Start, true) | (Alignment::End, false) => slack,
                (Alignment::Center, _) => slack / 2,
            };
            let mut pen = frame.left + indent + offset;
            let mut runs = Vec::new();
            for piece in &line.pieces {
                match piece {
                    Piece::Text {
                        text,
                        style,
                        decoration,
                    } => {
                        let width = self.fonts.text_advance_twips(text, *style);
                        runs.push(LocalRun {
                            x: pen,
                            text: text.clone(),
                            style: *style,
                            decoration: decoration.clone(),
                            width,
                        });
                        pen += width;
                    }
                    Piece::Box { width_twips } => {
                        if checkbox_pending {
                            checkbox_pending = false;
                            out.push(self.checkbox(block, pen, baseline));
                        }
                        pen += width_twips;
                    }
                }
            }
            if !runs.is_empty() {
                out.push(Local::Text { baseline, runs });
            }
        }
        out
    }

    /// The box a checklist item draws in place of a bullet.
    ///
    /// The screen draws a real `<input type="checkbox">`, whose look is the
    /// platform's; a stroked square with a filled centre when ticked is the
    /// honest paper equivalent and occupies exactly the width the line
    /// breaker reserved for it.
    fn checkbox(&self, block: &Block, x: i32, baseline: i64) -> Local {
        let size = self.scale.checkbox_size;
        let box_height = i64::from(size) * MILLI;
        // Centred on the middle of the ascent, which is where a browser puts
        // an `align-items: center` inline-flex box on a line of text.
        let metrics = self.fonts.metrics(FaceId::SansRegular);
        let ascent = i64::from(metrics.ascent) * i64::from(self.scale.body_size) * MILLI
            / font::UNITS_PER_EM;
        let top = baseline - ascent / 2 - box_height / 2;
        let checked = matches!(
            &block.kind,
            BlockKind::ListItem {
                kind: ListKind::Checklist { checked: true },
                ..
            }
        );
        if checked {
            Local::Fill {
                x,
                y: top,
                w: size,
                h: box_height,
                color: None,
                dashed: false,
            }
        } else {
            Local::Stroke {
                x,
                y: top,
                w: size,
                h: box_height,
                line: self.scale.page_break_rule,
            }
        }
    }

    /// Draws the marker a list item would get from `list-style-type`.
    ///
    /// The browser draws these; on paper they have to be glyphs, so the
    /// marker is right-aligned just inside the indent the item already has,
    /// which is where `list-style-position: outside` puts it.
    fn paint_list_marker(
        &self,
        marker: ListMarker,
        format: opendoc_core::OrderedListFormat,
        bullet_marker: BulletListMarker,
        ordinal: u32,
        content_left: i32,
        fragment: &mut Fragment,
    ) {
        let text = match marker {
            // The checkbox is the marker; `.doc-checklist` drops the bullet.
            ListMarker::Checklist => return,
            ListMarker::Bullet => bullet_marker.glyph().to_string(),
            ListMarker::Ordered => format!("{}.", lists::ordered_marker_format(format, ordinal)),
        };
        let size = self.scale.body_size;
        let style = TextStyle::new(size);
        // The marker sits on the item's own first baseline, which is not the
        // strut's when something on that line is set larger than the item is.
        let baseline = fragment.first_baseline;
        let width = self.fonts.text_advance_twips(&text, style);
        // A quarter em between the marker and the text, the gap Chrome leaves
        // for an outside marker at this size.
        let gap = size / 4;
        fragment.paint.insert(
            0,
            Local::Text {
                baseline,
                runs: vec![LocalRun {
                    x: content_left - width - gap,
                    text,
                    style,
                    decoration: RunDecoration::default(),
                    width,
                }],
            },
        );
    }

    /// What a block's line spacing means for the boxes on its lines.
    fn leading_of(&self, spacing: Option<LineSpacing>) -> Leading {
        match spacing {
            Some(LineSpacing::Multiple(multiple)) => Leading::Multiple(multiple.thousandths()),
            // CSS has one line-height rule and it behaves as `AtLeast`, which
            // is the approximation `opendoc-render` already documents; the
            // layout makes the same one so the two agree.
            Some(LineSpacing::Exact(height)) | Some(LineSpacing::AtLeast(height)) => {
                Leading::Fixed(height.twips())
            }
            None => Leading::Multiple(self.scale.line_height_thousandths),
        }
    }

    /// The height of one line box carrying nothing but the block's own strut,
    /// in milli-twips. Used for the single-style one-line boxes — an image
    /// caption, a list marker, an equation's source — where the union of the
    /// line is the strut by construction.
    fn leading(&self, size_twips: i32, spacing: Option<LineSpacing>) -> i64 {
        layout_units_to_milli_twips(self.leading_of(spacing).line_height_units(size_twips))
    }

    fn push_inline(
        &self,
        inline: &Inline,
        size_twips: i32,
        items: &mut Vec<Item>,
        estimate: &mut Option<EstimateReason>,
    ) {
        match inline {
            Inline::Text { text, marks, .. } => {
                let (style, decoration, reason) = self.style_for(marks, size_twips, None);
                note(estimate, reason);
                items.push(Item::decorated(text.clone(), style, decoration));
            }
            Inline::Link {
                text, marks, href, ..
            } => {
                let (style, decoration, reason) =
                    self.style_for(marks, size_twips, Some(href.as_str()));
                note(estimate, reason);
                items.push(Item::decorated(text.clone(), style, decoration));
            }
            Inline::Citation {
                citation_id,
                rendered_cache,
                ..
            } => {
                let label = rendered_cache
                    .clone()
                    .unwrap_or_else(|| format!("[{citation_id}]"));
                items.push(Item::decorated(
                    label,
                    TextStyle::new(size_twips),
                    RunDecoration {
                        color: Some(self.scale.citation_color),
                        background: Some(self.scale.citation_background),
                        ..RunDecoration::default()
                    },
                ));
            }
            Inline::Mention { label, .. }
            | Inline::GooglePersonChip { label, .. }
            | Inline::GoogleRichLinkChip { label, .. } => {
                items.push(Item::decorated(
                    label.clone(),
                    TextStyle::new(size_twips),
                    RunDecoration {
                        background: Some(self.scale.mention_background),
                        ..RunDecoration::default()
                    },
                ));
            }
            Inline::Dropdown {
                options,
                selected_option_id,
                ..
            } => {
                // The selected option is the actual inline text; the other
                // choices are interaction metadata and do not affect flow.
                let label = options
                    .iter()
                    .find(|option| option.id == *selected_option_id)
                    .map(|option| option.label.as_str())
                    .unwrap_or_default();
                items.push(Item::decorated(
                    format!("{label} ▾"),
                    TextStyle::new(size_twips),
                    RunDecoration {
                        background: Some(self.scale.mention_background),
                        ..RunDecoration::default()
                    },
                ));
            }
            Inline::DateChip { date, .. } => {
                items.push(Item::decorated(
                    date.clone(),
                    TextStyle::new(size_twips),
                    RunDecoration {
                        background: Some(self.scale.mention_background),
                        ..RunDecoration::default()
                    },
                ));
            }
            Inline::FootnoteRef { footnote_id, .. } => {
                // A `<sup>` at the script size, carrying the number the
                // renderer puts in it — numbered by order of first reference,
                // the same rule `opendoc-render` numbers by. Measuring a
                // placeholder instead would mean a one-digit reference for
                // every note in a hundred-note document.
                let number = self
                    .footnote_numbers
                    .get(footnote_id.as_str())
                    .copied()
                    .unwrap_or(0);
                items.push(Item::decorated(
                    number.to_string(),
                    TextStyle::new(self.script_size(size_twips)),
                    RunDecoration {
                        rise_units: font::script_shift_units(size_twips, Script::Super),
                        color: Some(self.scale.link_color),
                        ..RunDecoration::default()
                    },
                ));
            }
            Inline::Equation { equation, .. } => {
                // A rendered MathML box. Its width is the browser's math
                // layout, not a sum of advances; half an em per source
                // character is a stated placeholder, flagged as estimated.
                note(estimate, Some(EstimateReason::Equation));
                let characters = equation.source.chars().count() as i32;
                items.push(Item::Box {
                    width_twips: characters.saturating_mul(size_twips) / 2,
                    exact: false,
                });
            }
            Inline::PageNumber { .. } => {
                // In the body a page-number field has no page to resolve
                // against, so the stylesheet draws it as `#` — exactly one
                // glyph wide.
                items.push(Item::text("#", TextStyle::new(size_twips)));
            }
        }
    }

    /// Reads a run's marks into a measurable style and a drawn decoration.
    ///
    /// The estimate reason is set when a mark changes the drawn size or
    /// family in a way the bundled faces cannot reproduce — **not** for a
    /// superscript or a subscript, whose shift and line box Chrome derives
    /// from the parent's font size by a rule this crate now reproduces
    /// exactly.
    fn style_for(
        &self,
        marks: &[Mark],
        size_twips: i32,
        href: Option<&str>,
    ) -> (TextStyle, RunDecoration, Option<EstimateReason>) {
        let mut style = TextStyle::new(size_twips);
        let mut decoration = RunDecoration {
            link: href.map(str::to_string),
            ..RunDecoration::default()
        };
        let mut reason = None;
        let mut script = None;
        let mut sized = false;
        for mark in marks {
            match mark.kind {
                MarkKind::Bold => style.bold = true,
                MarkKind::Italic => style.italic = true,
                MarkKind::Code => style.mono = true,
                MarkKind::Superscript => script = Some(Script::Super),
                MarkKind::Subscript => script = Some(Script::Sub),
                MarkKind::Size => {
                    if let Some(points) = mark.value.as_deref().and_then(parse_points) {
                        style.size_twips = points;
                        sized = true;
                    } else {
                        note(&mut reason, Some(EstimateReason::UnmeasurableMark));
                    }
                }
                MarkKind::Underline => decoration.underline = true,
                MarkKind::Strike => decoration.strike = true,
                MarkKind::Color => match mark.value.as_deref().and_then(parse_rgb) {
                    Some(rgb) => decoration.color = Some(rgb),
                    None => note(&mut reason, Some(EstimateReason::UnmeasurableMark)),
                },
                MarkKind::Background => match mark.value.as_deref().and_then(parse_rgb) {
                    Some(rgb) => decoration.background = Some(rgb),
                    None => note(&mut reason, Some(EstimateReason::UnmeasurableMark)),
                },
                MarkKind::Link => {
                    if let Some(value) = mark.value.as_deref() {
                        decoration.link = Some(value.to_string());
                    }
                }
                // An arbitrary family is not bundled, so it cannot be
                // measured; the fallback is the document face and the
                // measurement says it is an estimate.
                MarkKind::Font => note(&mut reason, Some(EstimateReason::UnmeasurableMark)),
                MarkKind::Citation => {}
            }
        }
        if decoration.link.is_some() {
            // `.run-link` draws every link in the link colour and underlines
            // it, so a PDF that drew a link as plain black text would be
            // showing something the screen does not.
            decoration.color = decoration.color.or(Some(self.scale.link_color));
            decoration.underline = true;
        }
        if let Some(script) = script {
            // `.mark-superscript` states its size in a stylesheet rule and a
            // size mark states its own in an inline style, so the inline one
            // wins in the browser however the marks happen to be ordered.
            if !sized {
                style.size_twips = self.script_size(style.size_twips);
            }
            decoration.rise_units = font::script_shift_units(size_twips, script);
        }
        (style, decoration, reason)
    }

    fn script_size(&self, size_twips: i32) -> i32 {
        (size_twips * self.scale.script_size_thousandths as i32 / 1_000).max(1)
    }

    /// An image source and the rectangle document flow assigned it.  A
    /// successful backend draw deliberately has no visible filename or alt
    /// text; both are metadata. A backend that lacks the bytes can render a
    /// labelled frame from the same item instead of losing the asset silently.
    fn paint_image(&self, image: ImagePaint<'_>, frame: Frame, height: i64) -> Vec<Local> {
        let mut paint = vec![Local::Image {
            blob_hash: image.blob_hash.to_string(),
            alt_text: image.alt_text.to_string(),
            x: frame.left,
            y: 0,
            w: frame.width,
            h: height,
            rotation_degrees: image.rotation_degrees,
            crop: image.crop,
            opacity_percent: image.opacity_percent,
        }];
        if let Some(caption) = image.caption {
            let style = TextStyle::new(self.scale.caption_size);
            let leading = self.leading(self.scale.caption_size, None);
            paint.push(Local::Text {
                baseline: height + self.fonts.baseline_offset_milli(style, leading),
                runs: vec![LocalRun {
                    x: frame.left,
                    text: caption.to_string(),
                    style,
                    decoration: RunDecoration::default(),
                    width: self.fonts.text_advance_twips(caption, style),
                }],
            });
        }
        if let Some(border) = image.border.filter(|border| {
            border.style() != opendoc_core::BorderStyle::None && border.width().twips() > 0
        }) {
            let (red, green, blue) = border.color().rgb();
            let thickness = border.width().twips();
            let dash = match border.style() {
                opendoc_core::BorderStyle::Dashed => Some([thickness * 3, thickness * 2]),
                opendoc_core::BorderStyle::Dotted => Some([thickness, thickness]),
                _ => None,
            };
            let color = Rgb { red, green, blue };
            let bottom = height;
            paint.extend([
                Local::Edge {
                    x1: frame.left,
                    y1: 0,
                    x2: frame.left + frame.width,
                    y2: 0,
                    thickness,
                    color,
                    dash,
                },
                Local::Edge {
                    x1: frame.left,
                    y1: bottom,
                    x2: frame.left + frame.width,
                    y2: bottom,
                    thickness,
                    color,
                    dash,
                },
                Local::Edge {
                    x1: frame.left,
                    y1: 0,
                    x2: frame.left,
                    y2: bottom,
                    thickness,
                    color,
                    dash,
                },
                Local::Edge {
                    x1: frame.left + frame.width,
                    y1: 0,
                    x2: frame.left + frame.width,
                    y2: bottom,
                    thickness,
                    color,
                    dash,
                },
            ]);
        }
        paint
    }

    /// A block equation, as its LaTeX source in the mono face.
    ///
    /// `opendoc-render` projects equations to MathML and the browser lays
    /// them out; there is no math typesetter here, and ADR 0003 makes the
    /// source canonical anyway. Printing the source is lossy and says so.
    fn paint_equation_source(&self, source: &str, frame: Frame, leading: i64) -> Vec<Local> {
        let style = TextStyle {
            mono: true,
            ..TextStyle::new(self.scale.body_size)
        };
        vec![Local::Text {
            baseline: self.fonts.baseline_offset_milli(style, leading),
            runs: vec![LocalRun {
                x: frame.left,
                text: source.to_string(),
                style,
                decoration: RunDecoration::default(),
                width: self.fonts.text_advance_twips(source, style),
            }],
        }]
    }

    /// Draws the header and footer on every page, with page-number fields
    /// resolved.
    ///
    /// `opendoc-render` emits the furniture once and leaves the field values
    /// empty, because only a paginator knows what page this is. That job
    /// lands here.
    fn paint_furniture(
        &self,
        document: &Document,
        layout: &DocumentLayout,
        pages: &mut [PaintedPage],
    ) {
        let setup = &document.page_setup;
        let frame = Frame {
            left: 0,
            width: setup.content_width().twips(),
        };
        let content_left = setup.margin_start.twips();
        // Furniture is set at the furniture scale, not the body scale: the
        // stylesheet draws headers and footers smaller and tighter, and a PDF
        // that used the body size would be a PDF that disagrees with the
        // screen in the one place this whole design exists to make agree.
        // `.page-sheet-header p` also carries no margin, so the run's blocks
        // butt together.
        let furniture = Engine {
            fonts: self.fonts,
            scale: TypeScale {
                body_size: self.scale.furniture_size,
                line_height_thousandths: self.scale.furniture_line_height_thousandths,
                block_space_after: 0,
                ..self.scale.clone()
            },
            suggestions_pending: self.suggestions_pending,
            footnote_numbers: self.footnote_numbers.clone(),
            list_properties: self.list_properties.clone(),
            toc_entries: self.toc_entries.clone(),
            bibliography_entries: self.bibliography_entries.clone(),
            paint: self.paint,
        };
        for slot in [HeaderFooterSlot::Header, HeaderFooterSlot::Footer] {
            for (index, page) in pages.iter_mut().enumerate() {
                let blocks = document.furniture_for_page(slot, index);
                if blocks.is_empty() {
                    continue;
                }
                let numbered = resolve_page_fields(
                    blocks,
                    u64::from(setup.page_number_start) + index as u64,
                    u64::from(layout.page_count),
                );
                let (height, items) = furniture.stack_layout(&numbered, frame);
                let top = match slot {
                    HeaderFooterSlot::Header => i64::from(setup.margin_header.twips()) * MILLI,
                    // The stylesheet anchors the footer by its *bottom* edge,
                    // so its top depends on how tall it turned out to be.
                    HeaderFooterSlot::Footer => {
                        i64::from(setup.height.twips() - setup.margin_footer.twips()) * MILLI
                            - height
                    }
                    HeaderFooterSlot::FirstPageHeader
                    | HeaderFooterSlot::FirstPageFooter
                    | HeaderFooterSlot::EvenPageHeader
                    | HeaderFooterSlot::EvenPageFooter => {
                        unreachable!("layout iterates ordinary slots")
                    }
                };
                for item in &items {
                    page.items.push(item.placed(content_left, top));
                }
            }
        }
    }

    // ---- tables --------------------------------------------------------

    /// A table's height.
    ///
    /// `table-layout: fixed` with `width: 100%` and a `min-width` of the sum
    /// of the stated column widths makes the column geometry predictable
    /// enough to lay the cells out for real rather than guess a row height —
    /// but the distribution of leftover width and the interaction with merged
    /// cells are not pinned down by the model, so the result is flagged as an
    /// estimate.
    ///
    /// The box model here uses the type scale's border on every edge, which is
    /// what the stylesheet's `td` rule states. A cell that states a *different*
    /// width is drawn at the width it states but measured at the default one,
    /// so the row heights and the column positions of such a table are the
    /// screen's only to within the difference. `opendoc-pdf` names that in a
    /// warning of its own; the table is an estimate either way.
    fn table_fragment(
        &self,
        block: &Block,
        columns: &[TableColumn],
        properties: &opendoc_core::TableProperties,
        rows: &[TableRow],
        frame: Frame,
    ) -> Fragment {
        let widths = self.column_widths(columns, frame.width);
        let table_width: i32 = widths.iter().sum();
        let free = (frame.width - table_width).max(0);
        let rtl = block.properties.direction == Some(TextDirection::RightToLeft);
        let offset = match properties
            .alignment
            .unwrap_or(opendoc_core::TableAlignment::Start)
        {
            opendoc_core::TableAlignment::Start if rtl => free,
            opendoc_core::TableAlignment::End if !rtl => free,
            opendoc_core::TableAlignment::Center => free / 2,
            _ => 0,
        };
        let border = self.scale.cell_border;
        let grid = TableGrid::new(rows, widths.len());
        let mut height = i64::from(border) * MILLI;
        let mut paint: Vec<Local> = Vec::new();
        let mut geometry: Vec<RowGeometry> = Vec::new();
        // A row-spanning cell cannot decide its final vertical alignment until
        // every row it covers has its minimum height. Retain the measured cell
        // until that second, table-wide pass.
        let mut pending_cells: Vec<(usize, usize, CellPaint)> = Vec::new();
        for (row_index, row) in rows.iter().enumerate() {
            let row_top = height;
            let mut tallest = 0i64;
            let mut cell_paint: Vec<CellPaint> = Vec::new();
            let mut cells: Vec<(i32, i32)> = Vec::with_capacity(widths.len());
            let mut column_x = frame.left + offset;
            for width in &widths {
                cells.push((column_x, *width));
                column_x += width;
            }
            for (index, cell) in row.cells.iter().enumerate() {
                if !grid.is_anchor(row_index, index) {
                    continue;
                }
                let Some(anchor) = grid.anchor_at(row_index, index) else {
                    continue;
                };
                let width: i32 = widths[index..anchor.column_end].iter().sum();
                let x = cells
                    .get(index)
                    .map(|(x, _)| *x)
                    .unwrap_or(frame.left + offset);
                let padding_top = cell
                    .properties
                    .padding_top
                    .map(|value| value.twips())
                    .unwrap_or(self.scale.cell_padding_block);
                let padding_bottom = cell
                    .properties
                    .padding_bottom
                    .map(|value| value.twips())
                    .unwrap_or(self.scale.cell_padding_block);
                let padding_start = cell
                    .properties
                    .padding_start
                    .map(|value| value.twips())
                    .unwrap_or(self.scale.cell_padding_inline);
                let padding_end = cell
                    .properties
                    .padding_end
                    .map(|value| value.twips())
                    .unwrap_or(self.scale.cell_padding_inline);
                let inner = Frame {
                    left: x + padding_start + border,
                    width: (width - padding_start - padding_end - 2 * border).max(1),
                };
                let (content, items) = self.stack_layout(&cell.blocks, inner);
                let background = cell.properties.background.map(|color| {
                    let (red, green, blue) = color.rgb();
                    Rgb { red, green, blue }
                });
                let measured = CellPaint {
                    x,
                    width,
                    row_end: anchor.row_end,
                    background,
                    content_height: content,
                    top_offset: i64::from(padding_top + border) * MILLI,
                    bottom_padding: i64::from(padding_bottom) * MILLI,
                    alignment: cell.properties.vertical_alignment,
                    items,
                };
                if anchor.row_end == row_index + 1 {
                    tallest = tallest.max(
                        measured.content_height + measured.top_offset + measured.bottom_padding,
                    );
                }
                cell_paint.push(measured);
            }
            // CSS `tr { height }` is a minimum: overflowing cell content
            // grows the row. The PDF layout uses exactly that rule.
            let row_height = row
                .height
                // `Length::twips()` is already the document's physical unit.
                // Dividing by 20 here turned a 72pt requested row into 3.6pt.
                .map(|height| i64::from(height.twips()) * MILLI)
                .unwrap_or(0)
                .max(tallest);
            for cell in cell_paint {
                pending_cells.push((row_index, cell.row_end, cell));
            }
            geometry.push(RowGeometry {
                top: row_top,
                height: row_height,
                cells,
            });
            height += row_height;
        }
        // A spanning cell's content contributes to the combined height of
        // the rows it owns. Put any deficit on the anchor row: it preserves
        // every stated following row minimum, keeps the anchor's paint in its
        // own flow fragment, and gives a stable,
        // deterministic answer where CSS leaves distribution implementation
        // defined.
        for (row, row_end, cell) in &pending_cells {
            let used = cell.content_height + cell.top_offset + cell.bottom_padding;
            let available: i64 = geometry[*row..*row_end].iter().map(|row| row.height).sum();
            if used > available {
                if let Some(anchor_row) = geometry.get_mut(*row) {
                    anchor_row.height += used - available;
                }
            }
        }
        height = i64::from(border) * MILLI;
        for row in &mut geometry {
            row.top = height;
            height += row.height;
        }
        if self.paint {
            // Draw each anchor exactly once over its complete rectangular
            // range. The covered records remain in the model for a future
            // split, but never reach the paper as duplicate content.
            for (row, row_end, cell) in &pending_cells {
                let top = geometry[*row].top;
                let height: i64 = geometry[*row..*row_end].iter().map(|row| row.height).sum();
                if let Some(color) = cell.background {
                    paint.push(Local::Fill {
                        x: cell.x,
                        y: top,
                        w: cell.width,
                        h: height,
                        color: Some(color),
                        dashed: false,
                    });
                }
                let used = cell.content_height + cell.top_offset + cell.bottom_padding;
                let spare = (height - used).max(0);
                let vertical_offset = match cell
                    .alignment
                    .unwrap_or(opendoc_core::VerticalAlignment::Top)
                {
                    opendoc_core::VerticalAlignment::Top => 0,
                    opendoc_core::VerticalAlignment::Middle => spare / 2,
                    opendoc_core::VerticalAlignment::Bottom => spare,
                };
                paint.extend(
                    cell.items
                        .iter()
                        .map(|item| item.translated(0, top + cell.top_offset + vertical_offset)),
                );
            }
        }
        if self.paint {
            // After the content, because a border is drawn over what it
            // encloses — and because the two never overlap, the order is a
            // statement of intent rather than a repair.
            paint.extend(self.table_borders(
                rows,
                &geometry,
                &grid,
                rtl,
                self.default_cell_border(properties),
            ));
        }
        let mut flow_rows = Vec::new();
        let mut start = 0usize;
        while start < geometry.len() {
            // A row span makes all rows it covers one unbreakable page-flow
            // unit. Close transitively: a second span beginning inside the
            // first range may extend the group farther still.
            let mut end = start + 1;
            loop {
                let extended = grid
                    .anchors
                    .iter()
                    .filter(|anchor| anchor.row < end && anchor.row_end > end)
                    .map(|anchor| anchor.row_end)
                    .max()
                    .unwrap_or(end);
                if extended == end {
                    break;
                }
                end = extended;
            }
            flow_rows.push(TableRowFlow {
                height: geometry[start..end].iter().map(|row| row.height).sum(),
                header: rows[start..end].iter().all(|row| row.header),
                paint: if self.paint {
                    self.table_range_paint(&paint, start, end, &geometry)
                } else {
                    Vec::new()
                },
            });
            start = end;
        }
        let table_flow = TableFlow { rows: flow_rows };
        Fragment {
            block_id: block.id.to_string(),
            margin_top: 0,
            margin_bottom: i64::from(self.scale.block_space_after) * MILLI,
            height,
            lines: 0,
            exact: false,
            keep_with_next: false,
            breaks_after: false,
            estimate: Some(EstimateReason::Table),
            first_baseline: 0,
            paint: Vec::new(),
            table_flow: Some(table_flow),
            repeat_header: None,
        }
    }

    /// Extracts one unbreakable row range's page-local paint from the
    /// table-wide geometry.
    /// Boundaries on a shared horizontal edge belong to the row below, except
    /// for the table's final edge, so a continuation page gets a complete
    /// closed grid without drawing the shared edge twice.
    fn table_range_paint(
        &self,
        paint: &[Local],
        start: usize,
        end: usize,
        all_rows: &[RowGeometry],
    ) -> Vec<Local> {
        let row = &all_rows[start];
        paint
            .iter()
            .filter(|item| {
                let y = match item {
                    Local::Image { y, .. } => *y,
                    Local::Text { baseline, .. } => *baseline,
                    Local::Fill { y, .. } | Local::Stroke { y, .. } => *y,
                    Local::Edge { y1, y2, .. } => (*y1).min(*y2),
                };
                let next = all_rows.get(end).map(|next| next.top);
                // A collapsed outer border may extend half its thickness
                // above the first cell's content origin. It is still the
                // first row's rim, not paint to discard between fragments.
                let lower = if start == 0 { i64::MIN } else { row.top };
                y >= lower && next.is_none_or(|bottom| y < bottom)
            })
            .map(|item| item.translated(0, -row.top))
            .collect()
    }

    /// The border the stylesheet gives every cell edge that states nothing:
    /// `.doc-table td { border: var(--doc-cell-border) solid … }`.
    fn default_cell_border(&self, properties: &opendoc_core::TableProperties) -> Border {
        properties.border.map(Border::stated).unwrap_or(Border {
            style: opendoc_core::BorderStyle::Solid,
            width: self.scale.cell_border,
            color: self.scale.cell_border_color,
        })
    }

    /// What one cell contributes to one of its four edges: the border it
    /// states, or the stylesheet's default. `None` means there is no cell on
    /// that side of the boundary at all — the table's rim.
    ///
    /// `start` and `end` are direction-relative in the model, exactly as the
    /// CSS logical properties `opendoc-render` projects them to are, so a
    /// right-to-left table's leading edge is its right one. What this crate
    /// does *not* do for such a table is reverse the column order, which the
    /// browser does; that predates this and is unchanged.
    fn cell_border(
        &self,
        cell: Option<&TableCell>,
        edge: CellEdge,
        rtl: bool,
        default: Border,
    ) -> Option<Border> {
        let properties = &cell?.properties;
        let stated = match (edge, rtl) {
            (CellEdge::Top, _) => properties.border_top,
            (CellEdge::Bottom, _) => properties.border_bottom,
            (CellEdge::Left, false) | (CellEdge::Right, true) => properties.border_start,
            (CellEdge::Right, false) | (CellEdge::Left, true) => properties.border_end,
        };
        Some(stated.map(Border::stated).unwrap_or(default))
    }

    /// The table's collapsed border grid: one line per *boundary*, not four
    /// per cell.
    ///
    /// Walking boundaries rather than cells is what makes a shared edge a
    /// single decision. Horizontal boundaries run between consecutive rows —
    /// and above the first and below the last — with the cell above
    /// contributing its bottom border and the cell below its top; vertical
    /// boundaries run between consecutive cells of one row, with the left cell
    /// contributing its end border and the right cell its start border.
    fn table_borders(
        &self,
        rows: &[TableRow],
        geometry: &[RowGeometry],
        grid: &TableGrid,
        rtl: bool,
        default: Border,
    ) -> Vec<Local> {
        let mut out = Vec::new();
        let cells_of = |index: usize| -> Option<&[TableCell]> {
            rows.get(index).map(|row| row.cells.as_slice())
        };
        for boundary in 0..=geometry.len() {
            let above = boundary.checked_sub(1).and_then(|row| geometry.get(row));
            let below = geometry.get(boundary);
            // The boundary's own y, and the cell spans it is cut into. Every
            // row has one cell per column — `validate_table` refuses a
            // document where one does not — so either row's spans are the
            // whole boundary, and the one below is used wherever there is one.
            let (y, spans) = match (above, below) {
                (_, Some(below)) => (below.top, &below.cells),
                (Some(above), None) => (above.top + above.height, &above.cells),
                (None, None) => continue,
            };
            for (column, &(x, width)) in spans.iter().enumerate() {
                if boundary > 0
                    && boundary < geometry.len()
                    && grid
                        .anchor_at(boundary - 1, column)
                        .map(|anchor| (anchor.row, anchor.column))
                        == grid
                            .anchor_at(boundary, column)
                            .map(|anchor| (anchor.row, anchor.column))
                {
                    // This horizontal boundary is inside one row-spanning
                    // cell, so CSS has no edge to collapse or draw.
                    continue;
                }
                let earlier = self.cell_border(
                    boundary.checked_sub(1).and_then(|row| {
                        grid.anchor_at(row, column)
                            .and_then(|anchor| cells_of(anchor.row)?.get(anchor.column))
                    }),
                    CellEdge::Bottom,
                    rtl,
                    default,
                );
                let later = self.cell_border(
                    grid.anchor_at(boundary, column)
                        .and_then(|anchor| cells_of(anchor.row)?.get(anchor.column)),
                    CellEdge::Top,
                    rtl,
                    default,
                );
                if let Some(border) = borders::collapse(earlier, later) {
                    out.extend(borders::draw(border, (x, y), (x + width, y)));
                }
            }
        }
        for (index, row) in geometry.iter().enumerate() {
            let top = row.top;
            let bottom = row.top + row.height;
            for boundary in 0..=row.cells.len() {
                let x = match row.cells.get(boundary) {
                    Some(&(x, _)) => x,
                    None => match row.cells.last() {
                        Some(&(x, width)) => x + width,
                        None => continue,
                    },
                };
                if boundary > 0
                    && boundary < row.cells.len()
                    && grid
                        .anchor_at(index, boundary - 1)
                        .map(|anchor| (anchor.row, anchor.column))
                        == grid
                            .anchor_at(index, boundary)
                            .map(|anchor| (anchor.row, anchor.column))
                {
                    // This vertical boundary is inside one column-spanning
                    // cell rather than between two competing cell edges.
                    continue;
                }
                let earlier = self.cell_border(
                    boundary.checked_sub(1).and_then(|column| {
                        grid.anchor_at(index, column)
                            .and_then(|anchor| cells_of(anchor.row)?.get(anchor.column))
                    }),
                    CellEdge::Right,
                    rtl,
                    default,
                );
                let later = self.cell_border(
                    grid.anchor_at(index, boundary)
                        .and_then(|anchor| cells_of(anchor.row)?.get(anchor.column)),
                    CellEdge::Left,
                    rtl,
                    default,
                );
                if let Some(border) = borders::collapse(earlier, later) {
                    out.extend(borders::draw(border, (x, top), (x, bottom)));
                }
            }
        }
        out
    }

    fn column_widths(&self, columns: &[TableColumn], available: i32) -> Vec<i32> {
        if columns.is_empty() {
            return Vec::new();
        }
        let default = 72 * style::TWIPS_PER_PT;
        let stated: i32 = columns
            .iter()
            .map(|column| column.width.map(|w| w.twips()).unwrap_or(default))
            .sum();
        // `min-width` keeps the table from being squeezed below the widths the
        // document stated.
        let total = available.max(stated);
        let fixed: i32 = columns
            .iter()
            .filter_map(|column| column.width.map(|w| w.twips()))
            .sum();
        let auto = columns
            .iter()
            .filter(|column| column.width.is_none())
            .count();
        let share = if auto > 0 {
            ((total - fixed).max(0)) / auto as i32
        } else {
            0
        };
        columns
            .iter()
            .map(|column| column.width.map(|w| w.twips()).unwrap_or(share).max(1))
            .collect()
    }

    /// The height a sequence of blocks occupies inside a container, with
    /// sibling margins collapsed. Used for table cells, where the last
    /// paragraph's bottom margin is suppressed by the stylesheet.
    fn stack_layout(&self, blocks: &[Block], frame: Frame) -> (i64, Vec<Local>) {
        // Nested flows — a table cell's blocks, a header's — are not
        // memoized: the block that contains them is, as one value, so a cache
        // that also held the pieces would hold the same answer twice.
        let fragments = self.fragments(blocks, frame, &mut MeasureEveryTime);
        let mut height = 0i64;
        let mut previous_margin_bottom = 0i64;
        let mut paint = Vec::new();
        for (index, fragment) in fragments.iter().enumerate() {
            if index > 0 {
                height += previous_margin_bottom.max(fragment.margin_top);
            }
            if self.paint {
                paint.extend(fragment.paint.iter().map(|item| item.translated(0, height)));
            }
            height += fragment.height;
            previous_margin_bottom = fragment.margin_bottom;
        }
        (height, paint)
    }
}

fn toc_entries(document: &Document) -> Vec<(u8, String)> {
    document
        .blocks
        .iter()
        .filter_map(|block| {
            let BlockKind::Heading { level } = block.kind else {
                return None;
            };
            let text = block
                .content
                .iter()
                .filter_map(|inline| match inline {
                    Inline::Text { text, .. } | Inline::Link { text, .. } => Some(text.as_str()),
                    Inline::Mention { label, .. }
                    | Inline::GooglePersonChip { label, .. }
                    | Inline::GoogleRichLinkChip { label, .. } => Some(label.as_str()),
                    Inline::Dropdown {
                        options,
                        selected_option_id,
                        ..
                    } => options
                        .iter()
                        .find(|option| option.id == *selected_option_id)
                        .map(|option| option.label.as_str()),
                    Inline::DateChip { date, .. } => Some(date.as_str()),
                    Inline::Citation {
                        rendered_cache,
                        citation_id,
                        ..
                    } => rendered_cache.as_deref().or(Some(citation_id.as_str())),
                    Inline::Equation { equation, .. } => Some(equation.source.as_str()),
                    Inline::FootnoteRef { .. } | Inline::PageNumber { .. } => None,
                })
                .collect::<String>()
                .trim()
                .to_string();
            Some((
                level,
                if text.is_empty() {
                    "Untitled heading".to_string()
                } else {
                    text
                },
            ))
        })
        .collect()
}

fn bibliography_entries(document: &Document) -> Vec<String> {
    opendoc_citations::render_cited_bibliography(&document.citation_database)
        .into_iter()
        .map(|entry| entry.text)
        .collect()
}

/// Copies furniture blocks with their page-number fields resolved for one
/// page.
///
/// A copy rather than a mutation: the document is borrowed immutably and a
/// layout pass must never change it. The fields are the *only* thing that
/// differs between one page's header and another's, so everything else is
/// cloned unchanged and measured again — the cost is one small block list per
/// page, and the alternative is a second placement rule for "the same lines,
/// one glyph wider".
fn resolve_page_fields(blocks: &[Block], page: u64, page_count: u64) -> Vec<Block> {
    blocks
        .iter()
        .map(|block| {
            let mut copy = block.clone();
            for inline in &mut copy.content {
                if let Inline::PageNumber { id, field } = inline {
                    let value = match field {
                        PageNumberField::CurrentPage => page,
                        PageNumberField::PageCount => page_count,
                    };
                    *inline = Inline::Text {
                        id: id.clone(),
                        text: value.to_string(),
                        marks: Vec::new(),
                    };
                }
            }
            copy
        })
        .collect()
}

/// Milli-twips to twips, rounding half away from zero. The only rounding in
/// the crate, and it happens once, at the boundary.
pub(crate) fn to_twips(milli: i64) -> i32 {
    let rounded = if milli >= 0 {
        (milli + MILLI / 2) / MILLI
    } else {
        (milli - MILLI / 2) / MILLI
    };
    rounded.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

/// Numbers a document's footnotes by order of first reference.
///
/// The same rule `opendoc_render::footnotes::collect_footnote_numbers` uses,
/// and it has to be the same: the number is what the `<sup>` draws, so a
/// different rule here would measure a different string from the one on
/// screen. Reproduced rather than shared because neither crate depends on the
/// other, and `opendoc-render`'s `footnote_numbering_matches_the_layouts`
/// pins that they agree.
pub fn footnote_numbers(document: &Document) -> BTreeMap<String, u32> {
    let mut numbers = BTreeMap::new();
    fn walk(blocks: &[Block], numbers: &mut BTreeMap<String, u32>) {
        for block in blocks {
            for inline in &block.content {
                if let Inline::FootnoteRef { footnote_id, .. } = inline {
                    let next = numbers.len() as u32 + 1;
                    numbers.entry(footnote_id.to_string()).or_insert(next);
                }
            }
            if let BlockKind::Table { rows, .. } = &block.kind {
                for row in rows {
                    for cell in &row.cells {
                        walk(&cell.blocks, numbers);
                    }
                }
            }
        }
    }
    walk(&document.blocks, &mut numbers);
    numbers
}

/// A `MarkKind::Color` / `MarkKind::Background` value as three bytes.
///
/// `#rgb` and `#rrggbb` only: those are what the colour inputs in the toolbar
/// produce and what the importers write. Anything else — a named colour, a
/// `color-mix()`, a gradient — is *not* guessed at; it is reported as a mark
/// the layout could not reproduce, and the export names it. Silently drawing
/// black would be the failure this whole pass exists to remove.
fn parse_rgb(value: &str) -> Option<Rgb> {
    let value = value.trim();
    let digits = value.strip_prefix('#')?;
    if !digits.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |text: &str| u8::from_str_radix(text, 16).ok();
    match digits.len() {
        3 => {
            let mut chars = digits.chars();
            let mut nibble = || {
                let ch = chars.next()?;
                byte(&format!("{ch}{ch}"))
            };
            Some(Rgb {
                red: nibble()?,
                green: nibble()?,
                blue: nibble()?,
            })
        }
        6 => Some(Rgb {
            red: byte(&digits[0..2])?,
            green: byte(&digits[2..4])?,
            blue: byte(&digits[4..6])?,
        }),
        _ => None,
    }
}

/// A `MarkKind::Size` value, which the renderer treats as points when it is
/// bare digits.
fn parse_points(value: &str) -> Option<i32> {
    let value = value.trim();
    if value.is_empty() || !value.chars().all(|ch| ch.is_ascii_digit() || ch == '.') {
        return None;
    }
    let points: f64 = value.parse().ok()?;
    if !points.is_finite() || points <= 0.0 || points > 1_600.0 {
        return None;
    }
    // Points to twips, rounded to the twip: the same grid the model stores
    // every other length on.
    Some((points * 20.0).round() as i32)
}

#[cfg(test)]
mod cache_tests;
#[cfg(test)]
mod tests;
