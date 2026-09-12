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

pub mod font;
pub mod style;
pub mod text;

use opendoc_core::{
    Block, BlockKind, Document, Inline, LineSpacing, ListKind, Mark, MarkKind, PageSetup,
    TableColumn, TableRow,
};

use font::{Fonts, TextStyle};
use style::TypeScale;
use text::Item;

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
    let engine = Engine {
        fonts: Fonts::load(),
        scale: TypeScale::default(),
        // A document carrying suggestions renders extra inline content that
        // is not in `Block::content`; rather than model the suggestion
        // projection twice, the layout says it is estimating.
        suggestions_pending: !document.suggestions.is_empty(),
    };
    engine.layout(&document.blocks, &document.page_setup)
}

/// One block's contribution to the vertical flow, before pagination.
#[derive(Clone, Debug)]
struct Fragment {
    block_id: String,
    /// Collapsible margin above, in milli-twips.
    margin_top: i64,
    /// Collapsible margin below.
    margin_bottom: i64,
    /// Border-box height.
    height: i64,
    lines: u32,
    exact: bool,
    /// True for the rule an explicit page break draws: the block after it
    /// starts a new page.
    breaks_after: bool,
}

/// The horizontal box a block is laid out in.
#[derive(Clone, Copy, Debug)]
struct Frame {
    /// Content width in twips.
    width: i32,
}

struct Engine {
    fonts: Fonts,
    scale: TypeScale,
    suggestions_pending: bool,
}

impl Engine {
    fn layout(&self, blocks: &[Block], setup: &PageSetup) -> DocumentLayout {
        let frame = Frame {
            width: setup.content_width().twips(),
        };
        let fragments = self.fragments(blocks, frame);
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
    fn paginate(&self, fragments: &[Fragment], setup: &PageSetup) -> DocumentLayout {
        let page_height = i64::from(setup.height.twips()) * MILLI;
        let margin_top = i64::from(setup.margin_top.twips()) * MILLI;
        let content_height = i64::from(setup.content_height().twips()) * MILLI;

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
            // A block already at the top of its page is never pushed again:
            // a block taller than the content box takes a page and overflows,
            // which is visible and honest, rather than looping forever.
            if index > page_opened_at && (forced || top + fragment.height > page_bottom) {
                page += 1;
                let opened = i64::from(page) * page_height + margin_top;
                // The margin is measured from the previous border box, so it
                // is what CSS margin collapsing will actually produce: it is
                // always larger than the margin-bottom it collapses with,
                // because it spans at least one page's bottom and top margin.
                page_break_margin = Some(opened - pen);
                top = opened;
                page_opened_at = index;
            }
            exact_so_far &= fragment.exact;
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

        DocumentLayout {
            page_count: page + 1,
            exact: placements.last().map(|last| last.exact).unwrap_or(true),
            blocks: placements,
        }
    }

    // ---- block boxes ---------------------------------------------------

    /// Turns a sequence of blocks into fragments, resolving list runs.
    ///
    /// The list structure is the one `opendoc-render` writes: a maximal run of
    /// adjacent list items becomes nested `<ul>`/`<ol>` elements, and the
    /// stack discipline below is the same one `ListWriter` uses, because the
    /// indent a given item ends up with depends on it.
    fn fragments(&self, blocks: &[Block], frame: Frame) -> Vec<Fragment> {
        let mut out: Vec<Fragment> = Vec::with_capacity(blocks.len());
        let mut stack: Vec<(u8, ListMarker)> = Vec::new();
        // Index into `out` of the first item of the list run currently open.
        let mut run_start: Option<usize> = None;

        for block in blocks {
            match &block.kind {
                BlockKind::ListItem {
                    level,
                    kind: list_kind,
                    ..
                } => {
                    let marker = ListMarker::of(*list_kind);
                    open_list_levels(&mut stack, *level, marker);
                    let indent: i32 = stack
                        .iter()
                        .map(|(_, marker)| match marker {
                            ListMarker::Checklist => self.scale.checklist_indent,
                            _ => self.scale.list_indent,
                        })
                        .sum();
                    let inner = Frame {
                        width: (frame.width - indent).max(1),
                    };
                    let mut fragment = self.text_fragment(block, inner, self.scale.body_size);
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
                        stack.clear();
                    }
                    out.push(self.block_fragment(block, frame));
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

    fn block_fragment(&self, block: &Block, frame: Frame) -> Fragment {
        match &block.kind {
            BlockKind::Paragraph => self.text_fragment(block, frame, self.scale.body_size),
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
                exact: true,
                breaks_after: true,
            },
            BlockKind::Image {
                alt_text, layout, ..
            } => {
                let space = i64::from(self.scale.float_block_space_before) * MILLI;
                // A stated height is a document fact and is honoured exactly.
                // Without one the drawn size depends on the image's own
                // pixels, which this crate does not have — so it is an
                // estimate and says so.
                let (height, exact) = match layout.height {
                    Some(height) => (i64::from(height.twips()) * MILLI, true),
                    None => (i64::from(frame.width) * MILLI / 2, false),
                };
                let caption = if alt_text.is_empty() {
                    0
                } else {
                    self.leading(self.scale.caption_size, None)
                };
                Fragment {
                    block_id: block.id.to_string(),
                    margin_top: space,
                    margin_bottom: space,
                    height: height + caption,
                    lines: 0,
                    exact,
                    breaks_after: false,
                }
            }
            BlockKind::EquationBlock { .. } => {
                let space = i64::from(self.scale.float_block_space_before) * MILLI;
                Fragment {
                    block_id: block.id.to_string(),
                    margin_top: space,
                    margin_bottom: space,
                    // A rendered MathML box's height depends on the browser's
                    // math layout, which is not modelled: one line is the
                    // floor, and the estimate is flagged.
                    height: self.leading(self.scale.body_size, None),
                    lines: 0,
                    exact: false,
                    breaks_after: false,
                }
            }
            BlockKind::Table { columns, rows } => self.table_fragment(block, columns, rows, frame),
        }
    }

    /// A block whose height is a line count times a leading.
    fn text_fragment(&self, block: &Block, frame: Frame, size_twips: i32) -> Fragment {
        let properties = &block.properties;
        let indent_start = properties.indent_start.map(|l| l.twips()).unwrap_or(0);
        let indent_end = properties.indent_end.map(|l| l.twips()).unwrap_or(0);
        let first_indent = properties.indent_first_line.map(|l| l.twips()).unwrap_or(0);
        let width = (frame.width - indent_start - indent_end).max(1);
        // A positive first-line indent narrows the first line; a negative one
        // (the model's only hanging indent) widens it. Same sign convention as
        // CSS `text-indent`, which the renderer projects it to.
        let first_width = (width - first_indent).max(1);

        let mut items = Vec::new();
        let mut exact = true;
        if let BlockKind::ListItem {
            kind: ListKind::Checklist { .. },
            ..
        } = &block.kind
        {
            // The checkbox is an inline box before the item's text.
            let gap = size_twips * self.scale.checkbox_gap_thousandths as i32 / 1_000;
            items.push(Item::Box {
                width_twips: self.scale.checkbox_size + gap,
                exact: true,
            });
        }
        for inline in &block.content {
            self.push_inline(inline, size_twips, &mut items, &mut exact);
        }
        let measured = text::measure(&items, &self.fonts, first_width, width);
        let leading = self.leading(size_twips, properties.line_spacing);
        Fragment {
            block_id: block.id.to_string(),
            margin_top: i64::from(properties.space_before.map(|l| l.twips()).unwrap_or(0)) * MILLI,
            margin_bottom: i64::from(
                properties
                    .space_after
                    .map(|l| l.twips())
                    .unwrap_or(self.scale.block_space_after),
            ) * MILLI,
            height: i64::from(measured.lines) * leading,
            lines: measured.lines,
            exact: exact && measured.exact,
            breaks_after: false,
        }
    }

    /// The height of one line box, in milli-twips.
    ///
    /// A unitless CSS `line-height` multiplies the element's own font size, so
    /// `size_twips * thousandths` *is* milli-twips with no rounding at all.
    fn leading(&self, size_twips: i32, spacing: Option<LineSpacing>) -> i64 {
        match spacing {
            Some(LineSpacing::Multiple(multiple)) => {
                i64::from(size_twips) * i64::from(multiple.thousandths())
            }
            // CSS has one line-height rule and it behaves as `AtLeast`, which
            // is the approximation `opendoc-render` already documents; the
            // layout makes the same one so the two agree.
            Some(LineSpacing::Exact(height)) | Some(LineSpacing::AtLeast(height)) => {
                i64::from(height.twips()) * MILLI
            }
            None => i64::from(size_twips) * i64::from(self.scale.line_height_thousandths),
        }
    }

    fn push_inline(
        &self,
        inline: &Inline,
        size_twips: i32,
        items: &mut Vec<Item>,
        exact: &mut bool,
    ) {
        match inline {
            Inline::Text { text, marks, .. } | Inline::Link { text, marks, .. } => {
                let (style, height_known) = self.style_for(marks, size_twips);
                *exact &= height_known;
                items.push(Item::text(text.clone(), style));
            }
            Inline::Citation {
                citation_id,
                rendered_cache,
                ..
            } => {
                let label = rendered_cache
                    .clone()
                    .unwrap_or_else(|| format!("[{citation_id}]"));
                items.push(Item::text(label, TextStyle::new(size_twips)));
            }
            Inline::Mention { label, .. } => {
                items.push(Item::text(label.clone(), TextStyle::new(size_twips)));
            }
            Inline::FootnoteRef { .. } => {
                // A `<sup>` at 0.75em. The width is measured; the line box it
                // sits in can be taller than the strut, which is not modelled.
                *exact = false;
                items.push(Item::text(
                    "0",
                    TextStyle::new(self.script_size(size_twips)),
                ));
            }
            Inline::Equation { equation, .. } => {
                // A rendered MathML box. Its width is the browser's math
                // layout, not a sum of advances; half an em per source
                // character is a stated placeholder, flagged as estimated.
                *exact = false;
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

    /// Reads a run's marks into a measurable style. The boolean is false when
    /// a mark changes the drawn size or family in a way the bundled faces
    /// cannot reproduce.
    fn style_for(&self, marks: &[Mark], size_twips: i32) -> (TextStyle, bool) {
        let mut style = TextStyle::new(size_twips);
        let mut height_known = true;
        for mark in marks {
            match mark.kind {
                MarkKind::Bold => style.bold = true,
                MarkKind::Italic => style.italic = true,
                MarkKind::Code => style.mono = true,
                MarkKind::Superscript | MarkKind::Subscript => {
                    style.size_twips = self.script_size(style.size_twips);
                    // The raised box can grow the line box past the strut.
                    height_known = false;
                }
                MarkKind::Size => {
                    if let Some(points) = mark.value.as_deref().and_then(parse_points) {
                        style.size_twips = points;
                    } else {
                        height_known = false;
                    }
                }
                // An arbitrary family is not bundled, so it cannot be
                // measured; the fallback is the document face and the
                // measurement says it is an estimate.
                MarkKind::Font => height_known = false,
                MarkKind::Underline
                | MarkKind::Strike
                | MarkKind::Color
                | MarkKind::Background
                | MarkKind::Link
                | MarkKind::Citation => {}
            }
        }
        (style, height_known)
    }

    fn script_size(&self, size_twips: i32) -> i32 {
        (size_twips * self.scale.script_size_thousandths as i32 / 1_000).max(1)
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
    fn table_fragment(
        &self,
        block: &Block,
        columns: &[TableColumn],
        rows: &[TableRow],
        frame: Frame,
    ) -> Fragment {
        let widths = self.column_widths(columns, frame.width);
        let padding = 2 * self.scale.cell_padding_block + self.scale.cell_border;
        let mut height = i64::from(self.scale.cell_border) * MILLI;
        for row in rows {
            let mut tallest = 0i64;
            for (index, cell) in row.cells.iter().enumerate() {
                let width = widths.get(index).copied().unwrap_or(frame.width);
                let inner = Frame {
                    width: (width
                        - 2 * self.scale.cell_padding_inline
                        - 2 * self.scale.cell_border)
                        .max(1),
                };
                let content = self.stack_height(&cell.blocks, inner);
                tallest = tallest.max(content);
            }
            height += tallest + i64::from(padding) * MILLI;
        }
        Fragment {
            block_id: block.id.to_string(),
            margin_top: 0,
            margin_bottom: i64::from(self.scale.block_space_after) * MILLI,
            height,
            lines: 0,
            exact: false,
            breaks_after: false,
        }
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
    fn stack_height(&self, blocks: &[Block], frame: Frame) -> i64 {
        let fragments = self.fragments(blocks, frame);
        let mut height = 0i64;
        let mut previous_margin_bottom = 0i64;
        for (index, fragment) in fragments.iter().enumerate() {
            if index > 0 {
                height += previous_margin_bottom.max(fragment.margin_top);
            }
            height += fragment.height;
            previous_margin_bottom = fragment.margin_bottom;
        }
        height
    }
}

/// Marker identity, matching `opendoc-render`'s `ListMarker`: what decides
/// whether two adjacent items share a wrapper.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ListMarker {
    Bullet,
    Ordered,
    Checklist,
}

impl ListMarker {
    fn of(kind: ListKind) -> Self {
        match kind {
            ListKind::Bullet => ListMarker::Bullet,
            ListKind::Ordered => ListMarker::Ordered,
            ListKind::Checklist { .. } => ListMarker::Checklist,
        }
    }
}

/// The wrapper stack `opendoc-render` would have open for an item at this
/// level and marker. Reproduced rather than shared because the renderer's
/// writer emits HTML as it goes and has no value to hand over; the rule it
/// encodes — close deeper or mismatched levels, then open one wrapper per
/// missing level — is what decides an item's indent.
fn open_list_levels(stack: &mut Vec<(u8, ListMarker)>, level: u8, marker: ListMarker) {
    while let Some((top_level, top_marker)) = stack.last().copied() {
        if top_level > level || (top_level == level && top_marker != marker) {
            stack.pop();
        } else {
            break;
        }
    }
    while stack.last().map(|(top, _)| *top < level).unwrap_or(true) {
        let next = match stack.last() {
            Some((top, _)) => top + 1,
            None => level,
        };
        stack.push((next, marker));
    }
}

/// Milli-twips to twips, rounding half away from zero. The only rounding in
/// the crate, and it happens once, at the boundary.
fn to_twips(milli: i64) -> i32 {
    let rounded = if milli >= 0 {
        (milli + MILLI / 2) / MILLI
    } else {
        (milli - MILLI / 2) / MILLI
    };
    rounded.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
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
mod tests;
