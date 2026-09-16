//! Per-render lookup tables and the block/inline HTML walk.

use crate::css::{
    block_kind_label, block_property_css, image_size_css, image_wrap_clearance_css, mark_classes,
    points, table_cell_css, twips_to_css_pt, write_mark_style, DEFAULT_TABLE_COLUMN_POINTS,
};
use crate::footnotes::collect_footnote_numbers;
use crate::html::{attr, base64_encode, safe_href};
use crate::lists::{ListMarker, ListWriter};
use crate::text::{render_checkbox, render_text_content, write_equation_error};
use crate::*;

/// Per-render lookup tables derived from document state.
pub(crate) struct RenderContext<'a> {
    document: &'a Document,
    /// Footnote id -> 1-based number in order of first reference.
    pub(crate) footnote_numbers: BTreeMap<String, usize>,
    /// Inline id -> comment thread ids anchored on it.
    comment_inlines: BTreeMap<String, Vec<String>>,
    /// Block id -> comment thread ids anchored to the block.
    comment_blocks: BTreeMap<String, Vec<String>>,
    /// Block id -> live bookmark names. Rendered as empty named targets at
    /// the beginning of the block, so a `#bookmark-name` link works without
    /// changing the editor's stable `data-block-id` addressing.
    block_bookmarks: BTreeMap<String, Vec<String>>,
    /// Inline id -> proposed delete suggestion ids covering it.
    delete_suggestions: BTreeMap<String, Vec<String>>,
    /// Inline id -> proposed format suggestion ids covering it.
    format_suggestions: BTreeMap<String, Vec<String>>,
    /// Inline id -> insert suggestions rendered after it.
    insert_after_inline: BTreeMap<String, Vec<&'a Suggestion>>,
    /// Block id -> insert suggestions rendered at the end of the block.
    insert_in_block: BTreeMap<String, Vec<&'a Suggestion>>,
    /// Blob hash -> image data used for inline images.
    images: BTreeMap<&'a str, (&'a str, &'a [u8])>,
    /// Warnings produced while projecting this render pass. Interior mutability
    /// is confined to the renderer's own scratch state — the source document is
    /// never touched.
    warnings: RefCell<Vec<ModelWarning>>,
}

/// Where one top-level element of the rendered body lives in the output
/// string. Recorded by [`RenderContext::render_blocks_recording`] and turned
/// into a [`crate::BodyFragment`] by the caller.
pub(crate) struct FragmentSpan {
    pub(crate) block_id: String,
    pub(crate) blocks: usize,
    pub(crate) start: usize,
    pub(crate) end: usize,
}

fn close_span(
    spans: Option<&mut Vec<FragmentSpan>>,
    open: &mut Option<(usize, &StableId, usize)>,
    end: usize,
) {
    let Some((start, block_id, blocks)) = open.take() else {
        return;
    };
    if let Some(spans) = spans {
        spans.push(FragmentSpan {
            block_id: block_id.to_string(),
            blocks,
            start,
            end,
        });
    }
}

pub(crate) fn inline_id(inline: &Inline) -> &StableId {
    match inline {
        Inline::Text { id, .. }
        | Inline::Link { id, .. }
        | Inline::Citation { id, .. }
        | Inline::FootnoteRef { id, .. }
        | Inline::Mention { id, .. }
        | Inline::GooglePersonChip { id, .. }
        | Inline::GoogleRichLinkChip { id, .. }
        | Inline::Dropdown { id, .. }
        | Inline::DateChip { id, .. }
        | Inline::Equation { id, .. }
        | Inline::PageNumber { id, .. } => id,
    }
}

pub(crate) fn walk_inlines<'a>(blocks: &'a [Block], out: &mut Vec<(&'a StableId, &'a StableId)>) {
    for block in blocks {
        for inline in &block.content {
            out.push((&block.id, inline_id(inline)));
        }
        if let BlockKind::Table { rows, .. } = &block.kind {
            for row in rows {
                for cell in &row.cells {
                    walk_inlines(&cell.blocks, out);
                }
            }
        }
    }
}

/// Image alt text is metadata, not a caption.  Keeping it on the picture
/// preserves the accessible name and makes a supplied filename available on
/// hover without adding document-visible text that users did not author.
fn image_hover_title(alt_text: &str) -> String {
    if alt_text.is_empty() {
        String::new()
    } else {
        format!(" title=\"{}\"", attr(alt_text))
    }
}

/// The figure is an atomic editor object, so it must be reachable without a
/// pointer. Its image still carries the authored `alt`, while this name tells
/// a keyboard user what the focused object is. An empty alt remains an empty
/// alt on the image (the author may intend decoration), but the focus target
/// cannot be unnamed.
fn image_object_label(alt_text: &str, available: bool) -> String {
    match (alt_text.trim(), available) {
        ("", true) => "Image".to_string(),
        ("", false) => "Image unavailable".to_string(),
        (text, true) => format!("Image: {text}"),
        (text, false) => format!("Image unavailable: {text}"),
    }
}

fn table_header_id(table: &Block, cell: &opendoc_core::TableCell) -> String {
    // Both components are stable model identifiers. The colon namespace is
    // intentionally unavailable to portable bookmark names, so an authored
    // bookmark cannot duplicate an ARIA `headers` target in the same DOM.
    format!("opendoc-table-header:{}:{}", table.id, cell.id)
}

impl<'a> RenderContext<'a> {
    pub(crate) fn new(
        document: &'a Document,
        images: impl IntoIterator<Item = RenderImage<'a>>,
    ) -> Self {
        let mut order = Vec::new();
        walk_inlines(&document.blocks, &mut order);
        let position: BTreeMap<&str, usize> = order
            .iter()
            .enumerate()
            .map(|(index, (_, id))| (id.as_str(), index))
            .collect();
        let inline_ids_in_range = |start: &StableId, end: &StableId| -> Vec<String> {
            match (position.get(start.as_str()), position.get(end.as_str())) {
                (Some(&from), Some(&to)) => {
                    let (from, to) = if from <= to { (from, to) } else { (to, from) };
                    order[from..=to]
                        .iter()
                        .map(|(_, id)| id.to_string())
                        .collect()
                }
                (Some(&from), None) | (None, Some(&from)) => vec![order[from].1.to_string()],
                (None, None) => Vec::new(),
            }
        };

        let mut footnote_numbers = BTreeMap::new();
        for block in &document.blocks {
            collect_footnote_numbers(block, &mut footnote_numbers);
        }

        let mut comment_inlines: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut comment_blocks: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut block_bookmarks: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for bookmark in document
            .bookmarks
            .iter()
            .filter(|bookmark| !bookmark.deleted)
        {
            block_bookmarks
                .entry(bookmark.block_id.to_string())
                .or_default()
                .push(bookmark.name.clone());
        }
        for thread in document.comments.iter().filter(|thread| !thread.deleted) {
            match &thread.anchor {
                Anchor::TextRange(range) => {
                    for id in inline_ids_in_range(&range.start, &range.end) {
                        comment_inlines
                            .entry(id)
                            .or_default()
                            .push(thread.id.to_string());
                    }
                }
                Anchor::NearestBlock { block_id, .. } => comment_blocks
                    .entry(block_id.to_string())
                    .or_default()
                    .push(thread.id.to_string()),
                Anchor::Orphaned { .. } | Anchor::Document => {}
            }
        }

        let mut delete_suggestions: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut format_suggestions: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut insert_after_inline: BTreeMap<String, Vec<&Suggestion>> = BTreeMap::new();
        let mut insert_in_block: BTreeMap<String, Vec<&Suggestion>> = BTreeMap::new();
        for suggestion in document
            .suggestions
            .iter()
            .filter(|suggestion| suggestion.state == SuggestionState::Proposed)
        {
            match &suggestion.kind {
                SuggestionKind::Delete { range } => {
                    for id in inline_ids_in_range(&range.start, &range.end) {
                        delete_suggestions
                            .entry(id)
                            .or_default()
                            .push(suggestion.id.to_string());
                    }
                }
                SuggestionKind::Format { range, .. }
                | SuggestionKind::FormatRemove { range, .. }
                | SuggestionKind::FormatReplace { range, .. } => {
                    for id in inline_ids_in_range(&range.start, &range.end) {
                        format_suggestions
                            .entry(id)
                            .or_default()
                            .push(suggestion.id.to_string());
                    }
                }
                SuggestionKind::LinkChange { inline_id, .. } => {
                    format_suggestions
                        .entry(inline_id.to_string())
                        .or_default()
                        .push(suggestion.id.to_string());
                }
                SuggestionKind::Insert { anchor, .. } => match anchor {
                    Anchor::TextRange(range) => insert_after_inline
                        .entry(range.end.to_string())
                        .or_default()
                        .push(suggestion),
                    Anchor::NearestBlock { block_id, .. } => insert_in_block
                        .entry(block_id.to_string())
                        .or_default()
                        .push(suggestion),
                    Anchor::Orphaned { .. } | Anchor::Document => {
                        if let Some(last) = document.blocks.last() {
                            insert_in_block
                                .entry(last.id.to_string())
                                .or_default()
                                .push(suggestion);
                        }
                    }
                },
                // Structural suggestions are intentionally represented by
                // the review panel rather than a fake inline decoration.
                // The document itself stays unchanged until acceptance.
                SuggestionKind::BlockDelete { .. }
                | SuggestionKind::BlockInsert { .. }
                | SuggestionKind::BlockReplace { .. }
                | SuggestionKind::ParagraphStyleChange { .. } => {}
            }
        }

        let images = images
            .into_iter()
            .map(|image| (image.hash, (image.media_type, image.bytes)))
            .collect();

        Self {
            document,
            footnote_numbers,
            comment_inlines,
            comment_blocks,
            block_bookmarks,
            delete_suggestions,
            format_suggestions,
            insert_after_inline,
            insert_in_block,
            images,
            warnings: RefCell::new(Vec::new()),
        }
    }

    /// Record a projection warning. Deduplicated so a warning list stays stable
    /// and small when the same equation renders more than once.
    fn push_warnings(&self, warnings: Vec<ModelWarning>) {
        let mut collected = self.warnings.borrow_mut();
        for warning in warnings {
            if !collected.contains(&warning) {
                collected.push(warning);
            }
        }
    }

    pub(crate) fn into_warnings(self) -> Vec<ModelWarning> {
        self.warnings.into_inner()
    }

    /// Project an equation to markup and collect its warnings. The equation
    /// itself is only read.
    fn project_equation(
        &self,
        equation: &opendoc_core::Equation,
        display: EquationDisplay,
    ) -> RenderedEquation {
        let mut rendered = render_equation(equation, display);
        self.push_warnings(std::mem::take(&mut rendered.warnings));
        rendered
    }

    pub(crate) fn render_blocks(&self, blocks: &[Block], out: &mut String) {
        self.render_blocks_recording(blocks, out, None);
    }

    /// As [`Self::render_blocks`], and — when `spans` is given — recording
    /// where each top-level element of the output begins and ends.
    ///
    /// The spans partition the written region exactly: every byte belongs to
    /// one of them and none overlap, which is what lets a consumer treat the
    /// fragments as the body. That is only true because the units recorded
    /// are *top-level elements*, not blocks: a list run is one element
    /// however many items it holds, because a nested item's `</li>` is
    /// written after its child list closes and the run therefore cannot be
    /// cut any finer without producing unbalanced markup.
    ///
    /// Recording costs one allocation per top-level element and nothing at
    /// all when `spans` is `None`, which is the whole-body path.
    pub(crate) fn render_blocks_recording(
        &self,
        blocks: &[Block],
        out: &mut String,
        mut spans: Option<&mut Vec<FragmentSpan>>,
    ) {
        let mut list = ListWriter::default();
        // The element being written: where it started in `out`, the first
        // block in it, and how many blocks it covers.
        let mut open: Option<(usize, &StableId, usize)> = None;
        for block in blocks {
            if let BlockKind::ListItem {
                list_id,
                level,
                kind,
            } = &block.kind
            {
                let root_start =
                    self.render_list_item(block, list_id, *level, *kind, &mut list, out);
                match (root_start, open.as_mut()) {
                    // No outermost list opened, so the item joined the run
                    // already open — including when it nested a level deeper.
                    (None, Some((_, _, covered))) => *covered += 1,
                    // One did: whatever was open ended exactly where the new
                    // list began, which is mid-way through what this item
                    // wrote when a run changes marker.
                    (Some(start), _) => {
                        close_span(spans.as_deref_mut(), &mut open, start);
                        open = Some((start, &block.id, 1));
                    }
                    // Unreachable: an item with no run open opens one.
                    (None, None) => open = Some((out.len(), &block.id, 1)),
                }
            } else {
                if !list.is_empty() {
                    list.close_all(out);
                    close_span(spans.as_deref_mut(), &mut open, out.len());
                }
                open = Some((out.len(), &block.id, 1));
                self.render_block(block, out);
                close_span(spans.as_deref_mut(), &mut open, out.len());
            }
        }
        list.close_all(out);
        close_span(spans, &mut open, out.len());
    }

    /// Writes one list item, opening and closing whatever list structure it
    /// needs. Returns where a new outermost list began, when this item
    /// started one — see [`crate::lists::OpenedItem::root_start`].
    fn render_list_item(
        &self,
        block: &Block,
        list_id: &StableId,
        level: u8,
        kind: ListKind,
        list: &mut ListWriter,
        out: &mut String,
    ) -> Option<usize> {
        let start = self
            .document
            .list_properties
            .get(list_id)
            .map(|properties| properties.start_for(level))
            .unwrap_or(1);
        let formats = self.document.list_properties.get(list_id);
        let opened = list.open_item(
            out,
            list_id,
            level,
            ListMarker::of(kind),
            start,
            crate::lists::ListStyleResolver {
                ordered_format_for: |candidate_level| {
                    formats
                        .map(|properties| properties.format_for(candidate_level))
                        .unwrap_or_else(|| {
                            opendoc_core::OrderedListFormat::inherited_at(candidate_level)
                        })
                },
                bullet_marker_for: |candidate_level| {
                    formats
                        .map(|properties| properties.bullet_marker_for(candidate_level))
                        .unwrap_or_else(|| {
                            opendoc_core::BulletListMarker::inherited_at(candidate_level)
                        })
                },
            },
        );
        let extra = opened
            .value
            .map(|value| format!(" value=\"{value}\""))
            .unwrap_or_default();
        self.render_block_attributes(block, "li", &extra, out);
        if let Some(checked) = kind.checked() {
            render_checkbox(block.id.as_str(), checked, out);
        }
        self.render_inlines(block, out);
        list.item_content_written();
        opened.root_start
    }

    fn render_block_attributes(&self, block: &Block, tag: &str, extra: &str, out: &mut String) {
        self.render_block_attributes_styled(block, tag, "", extra, out);
    }

    /// Projects a table's grid into HTML.
    ///
    /// Three things are derived here rather than stored: the `<colgroup>`
    /// widths, the `colspan`/`rowspan` of each cell, and *which cells to skip*
    /// — a cell hidden under a merged neighbour is not drawn, but it is still
    /// in the model, which is what lets a split hand its content back.
    ///
    /// The layout is `table-layout: fixed` so that a column set to 2in is 2in
    /// and not "2in unless the content disagrees"; `min-width` keeps the
    /// browser from scaling every column down to fit, which is the failure
    /// jsdom cannot see and Chrome can.
    fn render_table(
        &self,
        block: &Block,
        columns: &[opendoc_core::TableColumn],
        properties: &opendoc_core::TableProperties,
        rows: &[opendoc_core::TableRow],
        out: &mut String,
    ) {
        let minimum: f64 = columns
            .iter()
            .map(|column| match column.width {
                Some(width) => width.points(),
                None => DEFAULT_TABLE_COLUMN_POINTS,
            })
            .sum();
        let fixed_width = columns.iter().all(|column| column.width.is_some());
        let width = if fixed_width { minimum } else { 100.0 };
        let width_unit = if fixed_width { "pt" } else { "%" };
        let mut layout = format!(
            "table-layout:fixed;width:{width}{width_unit};min-width:{};",
            points(minimum)
        );
        if fixed_width {
            match properties
                .alignment
                .unwrap_or(opendoc_core::TableAlignment::Start)
            {
                opendoc_core::TableAlignment::Start => {
                    layout.push_str("margin-inline-start:0;margin-inline-end:auto;")
                }
                opendoc_core::TableAlignment::Center => layout.push_str("margin-inline:auto;"),
                opendoc_core::TableAlignment::End => {
                    layout.push_str("margin-inline-start:auto;margin-inline-end:0;")
                }
            }
        }
        if let Some(border) = properties.border {
            if border.style() == opendoc_core::BorderStyle::None {
                layout.push_str("--doc-table-border:none;");
            } else {
                let _ = write!(
                    layout,
                    "--doc-table-border:{} {} {};",
                    points(border.width().points()),
                    border.style().as_str(),
                    border.color().as_hex(),
                );
            }
        }
        self.render_block_attributes_styled(block, "table", &layout, "", out);
        out.push_str("<colgroup>");
        for column in columns {
            let _ = write!(out, "<col data-column-id=\"{}\"", attr(column.id.as_str()));
            if let Some(width) = column.width {
                let _ = write!(out, " style=\"width:{}\"", points(width.points()));
            }
            out.push('>');
        }
        out.push_str("</colgroup>");
        let covered = opendoc_core::table_covered_positions(rows);
        let leading_headers = rows.iter().take_while(|row| row.header).count();
        // `TableRow::header` describes leading *column* header rows.  It
        // does not describe a row-header convention, so associations are
        // only emitted where the grid proves the relationship.  In
        // particular, a second header tier names its parent tier; this makes
        // a multi-level header usable by AT without guessing at a later
        // header row's meaning.
        let headers_for = |column_index: usize, span: usize, before_row: usize| {
            let cell_end = column_index + span;
            let mut header_ids = Vec::new();
            for (header_row_index, header_row) in rows
                .iter()
                .take(leading_headers.min(before_row))
                .enumerate()
            {
                for (header_column_index, header_cell) in header_row.cells.iter().enumerate() {
                    if covered.contains(&(header_row_index, header_column_index))
                        // An explicit cell-level row header is authoritative,
                        // even when its row is otherwise a leading column
                        // tier. Do not reinterpret it as a column header.
                        || header_cell.properties.row_header == Some(true)
                    {
                        continue;
                    }
                    let header_end = header_column_index + header_cell.span.columns() as usize;
                    if header_column_index < cell_end && column_index < header_end {
                        header_ids.push(table_header_id(block, header_cell));
                    }
                }
            }
            header_ids
        };
        // Unlike `TableRow::header`, this is explicit per-cell source state.
        // A row header can span several physical rows, so every row inside
        // its stated rectangle names it; no first-column convention is
        // guessed here.
        let row_headers_for = |row_index: usize, row_span: usize| {
            let mut header_ids = Vec::new();
            let cell_end = row_index + row_span;
            for (header_row_index, header_row) in rows.iter().enumerate() {
                for (header_column_index, header_cell) in header_row.cells.iter().enumerate() {
                    if covered.contains(&(header_row_index, header_column_index))
                        || header_cell.properties.row_header != Some(true)
                    {
                        continue;
                    }
                    let end = header_row_index + header_cell.span.rows() as usize;
                    // A data cell can itself span physical rows. The header
                    // relationship is true for every row its rectangle
                    // occupies, not merely its top-left coordinate: a row
                    // header beginning on a later covered row still labels
                    // that merged data cell. Conversely, use intersection
                    // rather than a positional convention, so an unrelated
                    // header before or after its rectangle is never named.
                    if header_row_index < cell_end && row_index < end {
                        header_ids.push(table_header_id(block, header_cell));
                    }
                }
            }
            header_ids
        };
        for (row_index, row) in rows.iter().enumerate() {
            if row_index == 0 && leading_headers > 0 {
                out.push_str("<thead>");
            } else if row_index == leading_headers && leading_headers > 0 {
                out.push_str("</thead><tbody>");
            } else if row_index == 0 {
                out.push_str("<tbody>");
            }
            let _ = write!(out, "<tr data-row-id=\"{}\"", attr(row.id.as_str()));
            if let Some(height) = row.height {
                let _ = write!(out, " style=\"height:{}\"", points(height.points()));
            }
            out.push('>');
            for (column_index, cell) in row.cells.iter().enumerate() {
                if covered.contains(&(row_index, column_index)) {
                    continue;
                }
                let is_row_header = cell.properties.row_header == Some(true);
                let tag = if row.header || is_row_header {
                    "th"
                } else {
                    "td"
                };
                let _ = write!(out, "<{tag} data-cell-id=\"{}\"", attr(cell.id.as_str()));
                // Only leading header rows are a safely derivable column
                // hierarchy. A later row marked as a header remains a `<th>`
                // (and may be styled as such), but guessing whether it is a
                // row header or a second table would make `headers` lie.
                if is_row_header {
                    let header_id = table_header_id(block, cell);
                    let _ = write!(out, " id=\"{}\" scope=\"row\"", attr(&header_id));
                    // An explicit row header can also sit under a proven
                    // leading column-header hierarchy. Its row scope remains
                    // authoritative, while `headers` tells AT which column
                    // heading contextualises that header itself. For a row
                    // header inside a leading tier, only earlier tiers can
                    // be its parents, avoiding a self-reference.
                    let header_ids = headers_for(
                        column_index,
                        cell.span.columns() as usize,
                        row_index.min(leading_headers),
                    );
                    if !header_ids.is_empty() {
                        let _ = write!(out, " headers=\"{}\"", attr(&header_ids.join(" ")));
                    }
                } else if row_index < leading_headers {
                    let header_id = table_header_id(block, cell);
                    let scope = if cell.span.columns() > 1 {
                        "colgroup"
                    } else {
                        "col"
                    };
                    let _ = write!(out, " id=\"{}\" scope=\"{scope}\"", attr(&header_id));
                    if row_index > 0 {
                        let header_ids =
                            headers_for(column_index, cell.span.columns() as usize, row_index);
                        if !header_ids.is_empty() {
                            let _ = write!(out, " headers=\"{}\"", attr(&header_ids.join(" ")));
                        }
                    }
                } else {
                    let row_header_ids = row_headers_for(row_index, cell.span.rows() as usize);
                    // A non-leading header row has no safely inferred column
                    // meaning, but an explicit row header remains a proven
                    // relationship for its sibling cells. Do not let visual
                    // row-header styling erase that authored association.
                    if !row.header || !row_header_ids.is_empty() {
                        // A cell can span several columns, so it names every
                        // leading header whose visible span intersects its own.
                        // This covers multi-tier and merged headers without
                        // assigning an invented header to a non-header row.
                        let mut header_ids = headers_for(
                            column_index,
                            cell.span.columns() as usize,
                            leading_headers,
                        );
                        header_ids.extend(row_header_ids);
                        if !header_ids.is_empty() {
                            header_ids.sort();
                            header_ids.dedup();
                            let _ = write!(out, " headers=\"{}\"", attr(&header_ids.join(" ")));
                        }
                    }
                }
                if let Some(column) = columns.get(column_index) {
                    let _ = write!(out, " data-column-id=\"{}\"", attr(column.id.as_str()));
                }
                if cell.span.rows() > 1 {
                    let _ = write!(out, " rowspan=\"{}\"", cell.span.rows());
                }
                if cell.span.columns() > 1 {
                    let _ = write!(out, " colspan=\"{}\"", cell.span.columns());
                }
                let style = table_cell_css(&cell.properties);
                if !style.is_empty() {
                    let _ = write!(out, " style=\"{}\"", attr(&style));
                }
                out.push('>');
                if cell.blocks.is_empty() {
                    out.push_str("<br>");
                } else {
                    self.render_blocks(&cell.blocks, out);
                }
                let _ = write!(out, "</{tag}>");
            }
            out.push_str("</tr>");
        }
        if leading_headers == rows.len() && leading_headers > 0 {
            out.push_str("</thead>");
        } else {
            out.push_str("</tbody>");
        }
        out.push_str("</table>");
    }

    /// As [`Self::render_block_attributes`], but with declarations the kind of
    /// block itself contributes — a table's layout, for instance — merged into
    /// the same `style` attribute as its block properties, so there is only
    /// ever one.
    fn render_block_attributes_styled(
        &self,
        block: &Block,
        tag: &str,
        kind_style: &str,
        extra: &str,
        out: &mut String,
    ) {
        let kind = block_kind_label(&block.kind);
        let _ = write!(
            out,
            "<{tag} class=\"doc-block doc-{kind}\" data-block-id=\"{}\" data-kind=\"{kind}\"",
            attr(block.id.as_str())
        );
        match &block.kind {
            BlockKind::Heading { level } => {
                let _ = write!(
                    out,
                    " data-level=\"{level}\" id=\"{}\"",
                    attr(&heading_anchor_id(&block.id))
                );
            }
            BlockKind::ListItem {
                list_id,
                level,
                kind,
            } => {
                let _ = write!(
                    out,
                    " data-level=\"{level}\" data-list-id=\"{}\" data-ordered=\"{}\" data-list-kind=\"{}\"",
                    attr(list_id.as_str()),
                    kind.is_ordered(),
                    kind.as_str()
                );
                if let Some(checked) = kind.checked() {
                    let _ = write!(out, " data-checked=\"{checked}\"");
                }
            }
            _ => {}
        }
        if let Some(threads) = self.comment_blocks.get(block.id.as_str()) {
            let _ = write!(out, " data-comment-ids=\"{}\"", attr(&threads.join(" ")));
        }
        let mut declarations = block_property_css(&block.properties);
        declarations.push_str(kind_style);
        if !declarations.is_empty() {
            let _ = write!(out, " style=\"{}\"", attr(&declarations));
        }
        out.push_str(extra);
        out.push('>');
        if let Some(names) = self.block_bookmarks.get(block.id.as_str()) {
            for name in names {
                let _ = write!(
                    out,
                    "<span class=\"doc-bookmark-anchor\" id=\"{}\" aria-hidden=\"true\"></span>",
                    attr(name)
                );
            }
        }
    }

    fn render_block(&self, block: &Block, out: &mut String) {
        match &block.kind {
            BlockKind::Paragraph => {
                self.render_block_attributes(block, "p", "", out);
                self.render_inlines(block, out);
                out.push_str("</p>");
            }
            BlockKind::Title | BlockKind::Subtitle => {
                self.render_block_attributes(block, "p", "", out);
                self.render_inlines(block, out);
                out.push_str("</p>");
            }
            BlockKind::Heading { level } => {
                let level = (*level).clamp(1, 6);
                let tag = format!("h{level}");
                self.render_block_attributes(block, &tag, "", out);
                self.render_inlines(block, out);
                let _ = write!(out, "</{tag}>");
            }
            BlockKind::ListItem {
                list_id,
                level,
                kind,
            } => {
                // A lone list item, rendered outside the run-aware path in
                // render_blocks; it still needs its own <ul>/<ol> wrapper.
                let mut list = ListWriter::default();
                let _ = self.render_list_item(block, list_id, *level, *kind, &mut list, out);
                list.close_all(out);
            }
            BlockKind::Table {
                columns,
                properties,
                rows,
            } => {
                self.render_table(block, columns, properties, rows, out);
            }
            BlockKind::EquationBlock { equation } => {
                let rendered = self.project_equation(equation, EquationDisplay::Block);
                // The outer group remains named as an editor object while its
                // MathML child retains the mathematical reading. It is a
                // focusable atomic block, so keyboard users never have to
                // guess at an internal caret the model does not have.
                self.render_block_attributes(
                    block,
                    "div",
                    " contenteditable=\"false\" tabindex=\"0\" role=\"group\" aria-label=\"Block equation\" aria-description=\"Press Escape to deselect this equation.\"",
                    out,
                );
                let _ = write!(
                    out,
                    "<span class=\"equation equation-block {}\" contenteditable=\"false\" data-equation-source=\"{}\"",
                    rendered.state_class,
                    attr(&equation.source)
                );
                write_equation_error(&rendered, out);
                out.push('>');
                out.push_str(&rendered.html);
                out.push_str("</span></div>");
            }
            BlockKind::Image {
                blob_hash,
                alt_text,
                layout,
            } => {
                // The placement is an attribute, not a declaration: which CSS
                // a float needs is the stylesheet's business, and the morph
                // syncs attributes, so a stale one cannot survive a re-render.
                let mut extra = format!(
                    " contenteditable=\"false\" tabindex=\"0\" aria-label=\"{}\" aria-keyshortcuts=\"Control+Shift+ArrowLeft Control+Shift+ArrowRight Control+Shift+ArrowUp Control+Shift+ArrowDown\" aria-description=\"Use Control Shift plus arrow keys to resize this image; add Alt for larger steps. Press Escape to deselect the image.\" data-placement=\"{}\"",
                    attr(&image_object_label(alt_text, self.images.contains_key(blob_hash.as_str()))),
                    layout.effective_placement().as_str(),
                );
                if let Some(clearance) = layout.wrap_clearance {
                    let _ = write!(
                        extra,
                        " data-wrap-clearance-top-twips=\"{}\" data-wrap-clearance-end-twips=\"{}\" data-wrap-clearance-bottom-twips=\"{}\" data-wrap-clearance-start-twips=\"{}\"",
                        clearance.top.twips(), clearance.end.twips(), clearance.bottom.twips(), clearance.start.twips(),
                    );
                }
                // The figure's explicit accessible name carries image alt
                // text, which can otherwise supersede the native figure /
                // figcaption naming relationship. Keep a visible authored
                // caption available as the focused object's description too,
                // without turning alt text into a visible caption.
                let caption_id = layout
                    .caption
                    .as_deref()
                    .filter(|caption| !caption.is_empty())
                    // Colons are unavailable to portable bookmark names, so
                    // an authored anchor cannot duplicate this ARIA target.
                    .map(|_| format!("opendoc-image-caption:{}", block.id));
                if let Some(caption_id) = &caption_id {
                    let _ = write!(extra, " aria-describedby=\"{}\"", attr(caption_id));
                }
                let mut positioned_style = image_wrap_clearance_css(layout);
                if let Some(positioned) = layout.positioned.as_ref() {
                    let anchor = match &positioned.anchor {
                        opendoc_core::PositionedImageAnchor::PageContent => {
                            "page-content".to_string()
                        }
                        opendoc_core::PositionedImageAnchor::Block(id) => format!("block:{}", id),
                    };
                    let _ = write!(
                        extra,
                        " data-positioned=\"true\" data-position-anchor=\"{}\" data-position-x-twips=\"{}\" data-position-y-twips=\"{}\" data-position-layer=\"{}\"",
                        attr(&anchor),
                        positioned.horizontal_offset.twips(),
                        positioned.vertical_offset.twips(),
                        positioned.layer.as_str(),
                    );
                    // The initial values make a page-content object useful in
                    // a standalone HTML render.  The editor refines a block
                    // anchor against its live border box after fragments are
                    // installed, because CSS has no interoperable stable-id
                    // anchor primitive yet.
                    positioned_style = format!(
                        "position: absolute; inset-inline-start: {}; top: {}; z-index: {};",
                        twips_to_css_pt(positioned.horizontal_offset.twips()),
                        twips_to_css_pt(positioned.vertical_offset.twips()),
                        if positioned.layer == opendoc_core::PositionedImageLayer::BehindText {
                            0
                        } else {
                            2
                        },
                    );
                }
                self.render_block_attributes_styled(
                    block,
                    "figure",
                    &positioned_style,
                    &extra,
                    out,
                );
                let size = image_size_css(layout);
                match self.images.get(blob_hash.as_str()) {
                    Some((media_type, bytes)) => {
                        let _ = write!(
                            out,
                            "<img src=\"data:{};base64,{}\" alt=\"{}\"{} data-blob-hash=\"{}\" draggable=\"false\"{}>",
                            attr(media_type),
                            base64_encode(bytes),
                            attr(alt_text),
                            image_hover_title(alt_text),
                            attr(blob_hash),
                            size
                        );
                    }
                    None => {
                        let _ = write!(
                            out,
                            "<div class=\"doc-image-placeholder\" data-blob-hash=\"{}\"{}>{}</div>",
                            attr(blob_hash),
                            size,
                            escape_html(if alt_text.is_empty() {
                                "Image unavailable"
                            } else {
                                alt_text
                            })
                        );
                    }
                }
                if let Some(caption) = layout.caption.as_deref() {
                    if let Some(caption_id) = &caption_id {
                        let _ = write!(
                            out,
                            "<figcaption id=\"{}\">{}</figcaption>",
                            attr(caption_id),
                            escape_html(caption)
                        );
                    } else {
                        let _ = write!(out, "<figcaption>{}</figcaption>", escape_html(caption));
                    }
                }
                out.push_str("</figure>");
            }
            BlockKind::PageBreak => {
                self.render_block_attributes(block, "hr", " contenteditable=\"false\"", out);
            }
            BlockKind::HorizontalRule => {
                self.render_block_attributes(
                    block,
                    "hr",
                    " class=\"doc-horizontal-rule\" contenteditable=\"false\" tabindex=\"0\" aria-label=\"Horizontal rule\" aria-description=\"Press Escape to deselect this horizontal rule.\"",
                    out,
                );
            }
            BlockKind::TableOfContents { max_level } => {
                self.render_block_attributes(
                    block,
                    "nav",
                    " class=\"doc-table-of-contents\" contenteditable=\"false\" aria-label=\"Table of contents\"",
                    out,
                );
                render_toc_entries(self.document, *max_level, out);
                out.push_str("</nav>");
            }
            BlockKind::Bibliography => {
                self.render_block_attributes(
                    block,
                    "section",
                    " class=\"doc-bibliography\" contenteditable=\"false\" aria-label=\"Bibliography\"",
                    out,
                );
                out.push_str("<h2>Bibliography</h2><ol>");
                for entry in
                    opendoc_citations::render_cited_bibliography(&self.document.citation_database)
                {
                    let _ = write!(out, "<li>{}</li>", escape_html(&entry.text));
                }
                out.push_str("</ol></section>");
            }
        }
    }

    fn render_inlines(&self, block: &Block, out: &mut String) {
        if block.content.is_empty() {
            out.push_str("<br>");
        }
        let text_len: usize = block
            .content
            .iter()
            .map(|inline| match inline {
                Inline::Text { text, .. } | Inline::Link { text, .. } => text.len(),
                _ => 1,
            })
            .sum();
        for inline in &block.content {
            self.render_inline(inline, out);
        }
        if text_len == 0 && !block.content.is_empty() {
            // Keep an empty block caret-addressable.
            out.push_str("<br data-caret-anchor=\"true\">");
        }
        if let Some(suggestions) = self.insert_in_block.get(block.id.as_str()) {
            for suggestion in suggestions {
                self.render_insert_suggestion(suggestion, out);
            }
        }
    }

    fn run_attributes(&self, id: &StableId, kind: &str, extra_classes: &[&str], out: &mut String) {
        let mut classes = vec!["run".to_string(), format!("run-{kind}")];
        classes.extend(extra_classes.iter().map(|class| (*class).to_string()));
        if self.comment_inlines.contains_key(id.as_str()) {
            classes.push("has-comment".to_string());
        }
        if self.delete_suggestions.contains_key(id.as_str()) {
            classes.push("suggested-delete".to_string());
        }
        if self.format_suggestions.contains_key(id.as_str()) {
            classes.push("suggested-format".to_string());
        }
        let _ = write!(
            out,
            " class=\"{}\" data-inline-id=\"{}\" data-kind=\"{kind}\" data-inline-kind=\"{kind}\"",
            classes.join(" "),
            attr(id.as_str())
        );
        if let Some(threads) = self.comment_inlines.get(id.as_str()) {
            let _ = write!(out, " data-comment-ids=\"{}\"", attr(&threads.join(" ")));
        }
        let mut suggestion_ids: Vec<&String> = Vec::new();
        if let Some(ids) = self.delete_suggestions.get(id.as_str()) {
            suggestion_ids.extend(ids);
        }
        if let Some(ids) = self.format_suggestions.get(id.as_str()) {
            suggestion_ids.extend(ids);
        }
        if !suggestion_ids.is_empty() {
            let joined = suggestion_ids
                .iter()
                .map(|id| id.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            let _ = write!(out, " data-suggestion-ids=\"{}\"", attr(&joined));
        }
    }

    pub(crate) fn render_inline(&self, inline: &Inline, out: &mut String) {
        match inline {
            Inline::Text { id, text, marks } => {
                out.push_str("<span");
                self.run_attributes(id, "text", &mark_classes(marks), out);
                write_mark_style(marks, out);
                out.push('>');
                render_text_content(text, out);
                out.push_str("</span>");
            }
            Inline::Link {
                id,
                text,
                href,
                marks,
            } => {
                out.push_str("<a");
                self.run_attributes(id, "link", &mark_classes(marks), out);
                write_mark_style(marks, out);
                let _ = write!(
                    out,
                    " href=\"{}\" data-href=\"{}\">",
                    attr(&safe_href(href)),
                    attr(href)
                );
                render_text_content(text, out);
                out.push_str("</a>");
            }
            Inline::Citation {
                id,
                citation_id,
                rendered_cache,
            } => {
                let label = rendered_cache
                    .clone()
                    .or_else(|| {
                        self.document
                            .citation_database
                            .rendered_citation(citation_id)
                            .cloned()
                    })
                    .unwrap_or_else(|| format!("[{citation_id}]"));
                out.push_str("<span");
                self.run_attributes(id, "citation", &["citation-label"], out);
                let _ = write!(
                    out,
                    " contenteditable=\"false\" data-citation-id=\"{}\">{}</span>",
                    attr(citation_id.as_str()),
                    escape_html(&label)
                );
            }
            Inline::FootnoteRef { id, footnote_id } => {
                let number = self
                    .footnote_numbers
                    .get(footnote_id.as_str())
                    .copied()
                    .unwrap_or(0);
                out.push_str("<sup");
                self.run_attributes(id, "footnote-ref", &["footnote-ref"], out);
                let _ = write!(
                    out,
                    " contenteditable=\"false\" data-footnote-id=\"{}\">{number}</sup>",
                    attr(footnote_id.as_str())
                );
            }
            Inline::Mention { id, label } => {
                out.push_str("<span");
                self.run_attributes(id, "mention", &["mention"], out);
                let _ = write!(
                    out,
                    " contenteditable=\"false\">{}</span>",
                    escape_html(label)
                );
            }
            Inline::GooglePersonChip {
                id, label, email, ..
            } => {
                out.push_str("<span");
                self.run_attributes(id, "google-person-chip", &["mention", "smart-chip"], out);
                let _ = write!(
                    out,
                    " contenteditable=\"false\" data-google-person-email=\"{}\" aria-label=\"Google person chip: {} ({})\" title=\"Google person: {}\">{}</span>",
                    attr(email), attr(label), attr(email), attr(email), escape_html(label)
                );
            }
            Inline::GoogleRichLinkChip {
                id,
                label,
                href,
                rich_link_id,
                mime_type,
            } => {
                out.push_str("<a");
                self.run_attributes(id, "google-rich-link-chip", &["link", "smart-chip"], out);
                let _ = write!(
                    out,
                    " contenteditable=\"false\" href=\"{}\" data-href=\"{}\" data-google-rich-link-id=\"{}\" data-google-rich-link-mime-type=\"{}\" aria-label=\"Google rich link chip: {}\" title=\"Google rich link{}\">{}</a>",
                    attr(&safe_href(href)),
                    attr(href),
                    attr(rich_link_id.as_deref().unwrap_or("")),
                    attr(mime_type.as_deref().unwrap_or("")),
                    attr(label),
                    mime_type.as_deref().map(|mime| format!(" ({mime})")).unwrap_or_default(),
                    escape_html(label)
                );
            }
            Inline::Dropdown {
                id,
                options,
                selected_option_id,
            } => {
                let Some(selected) = options
                    .iter()
                    .find(|option| option.id == *selected_option_id)
                else {
                    return;
                };
                out.push_str("<span");
                self.run_attributes(id, "dropdown", &["dropdown", "smart-chip"], out);
                let _ = write!(
                    out,
                    " contenteditable=\"false\" tabindex=\"0\" role=\"button\" aria-haspopup=\"listbox\" aria-label=\"Dropdown: {}\" data-dropdown-selected=\"{}\">{} <span aria-hidden=\"true\">▾</span></span>",
                    attr(&selected.label),
                    attr(selected_option_id),
                    escape_html(&selected.label),
                );
            }
            Inline::DateChip { id, date } => {
                out.push_str("<time");
                self.run_attributes(id, "date-chip", &["date-chip", "smart-chip"], out);
                let _ =
                    write!(
                    out,
                    " contenteditable=\"false\" tabindex=\"0\" role=\"button\" datetime=\"{}\" aria-label=\"Edit date: {}\">{}</time>",
                    attr(date), attr(date), escape_html(date)
                );
            }
            Inline::Equation { id, equation } => {
                let rendered = self.project_equation(equation, EquationDisplay::Inline);
                out.push_str("<span");
                self.run_attributes(
                    id,
                    "equation",
                    &["equation", "equation-inline", rendered.state_class],
                    out,
                );
                let _ = write!(
                    out,
                    " contenteditable=\"false\" data-equation-source=\"{}\"",
                    attr(&equation.source)
                );
                write_equation_error(&rendered, out);
                out.push('>');
                out.push_str(&rendered.html);
                out.push_str("</span>");
            }
            // A page-number field projects to an *empty* element carrying the
            // field name. The renderer cannot know the value — it depends on
            // where the layout engine broke the pages — and inventing one
            // would put a number in the markup that the document never said.
            // Whoever paginates fills the element in; until then the
            // stylesheet shows it as an unresolved field. See ADR 0009.
            Inline::PageNumber { id, field } => {
                out.push_str("<span");
                self.run_attributes(id, "page-number", &["doc-field", "doc-page-number"], out);
                let _ = write!(
                    out,
                    " contenteditable=\"false\" data-field=\"{}\"></span>",
                    attr(field.as_str())
                );
            }
        }
        if let Some(suggestions) = self.insert_after_inline.get(inline_id(inline).as_str()) {
            for suggestion in suggestions {
                self.render_insert_suggestion(suggestion, out);
            }
        }
    }

    fn render_insert_suggestion(&self, suggestion: &Suggestion, out: &mut String) {
        let SuggestionKind::Insert { content, .. } = &suggestion.kind else {
            return;
        };
        let _ = write!(
            out,
            "<span class=\"suggested-insert\" contenteditable=\"false\" data-suggestion-id=\"{}\" data-author=\"{}\">",
            attr(suggestion.id.as_str()),
            attr(&suggestion.author)
        );
        for inline in content {
            match inline {
                Inline::Text { text, .. } | Inline::Link { text, .. } => {
                    render_text_content(text, out)
                }
                Inline::Mention { label, .. }
                | Inline::GooglePersonChip { label, .. }
                | Inline::GoogleRichLinkChip { label, .. } => out.push_str(&escape_html(label)),
                Inline::Dropdown {
                    options,
                    selected_option_id,
                    ..
                } => {
                    if let Some(option) = options
                        .iter()
                        .find(|option| option.id == *selected_option_id)
                    {
                        out.push_str(&escape_html(&option.label));
                    }
                }
                Inline::DateChip { date, .. } => out.push_str(&escape_html(date)),
                Inline::Equation { equation, .. } => {
                    let rendered = self.project_equation(equation, EquationDisplay::Inline);
                    out.push_str(&rendered.html);
                }
                Inline::Citation { citation_id, .. } => {
                    out.push_str(&escape_html(&format!("[{citation_id}]")))
                }
                Inline::FootnoteRef { .. } => out.push_str("[note]"),
                // A field has no resolved value inside a suggestion preview:
                // the suggestion is not on a page yet.
                Inline::PageNumber { field, .. } => {
                    let _ = write!(out, "[{}]", field.as_str());
                }
            }
        }
        out.push_str("</span>");
    }
}

/// A TOC is navigation, so its heading hierarchy must be structural HTML, not
/// merely a flat list whose nesting is painted with CSS. Skipped source levels
/// attach to the preceding visible ancestor one level down, matching the
/// desktop outline and avoiding invented placeholder headings.
#[derive(Clone, Debug)]
struct TocEntry {
    id: String,
    text: String,
    level: u8,
    children: Vec<TocEntry>,
}

/// Heading targets live in a namespace bookmarks cannot enter: bookmark
/// names admit only letters, digits, hyphens and underscores, whereas `:` is
/// valid in an HTML ID. Without this separation a bookmark named after a
/// heading's stable block ID produced duplicate DOM IDs and made `#` links
/// depend on browser traversal order.
fn heading_anchor_id(id: &StableId) -> String {
    format!("opendoc-heading:{id}")
}

fn render_toc_entries(document: &Document, max_level: u8, out: &mut String) {
    let mut roots = Vec::new();
    // Each entry retains the path from the root to its node. Keeping paths
    // instead of references makes the tree construction wholly safe while
    // preserving source order.
    let mut ancestors: Vec<(u8, Vec<usize>)> = vec![(0, Vec::new())];
    for heading in document.blocks.iter().filter(
        |candidate| matches!(candidate.kind, BlockKind::Heading { level } if level <= max_level),
    ) {
        let requested_level = match heading.kind {
            BlockKind::Heading { level } => level,
            _ => unreachable!(),
        };
        while ancestors.len() > 1
            && ancestors
                .last()
                .is_some_and(|(level, _)| *level >= requested_level)
        {
            ancestors.pop();
        }
        let (parent_level, parent_path) = ancestors.last().cloned().expect("root ancestor");
        let level = requested_level.min(parent_level.saturating_add(1));
        let children = toc_children_at_path(&mut roots, &parent_path);
        let index = children.len();
        children.push(TocEntry {
            id: heading_anchor_id(&heading.id),
            text: toc_heading_text(heading),
            level,
            children: Vec::new(),
        });
        let mut path = parent_path;
        path.push(index);
        ancestors.push((level, path));
    }
    render_toc_entry_list(&roots, out);
}

fn toc_children_at_path<'a>(
    entries: &'a mut Vec<TocEntry>,
    path: &[usize],
) -> &'a mut Vec<TocEntry> {
    match path.split_first() {
        None => entries,
        Some((index, rest)) => toc_children_at_path(&mut entries[*index].children, rest),
    }
}

fn render_toc_entry_list(entries: &[TocEntry], out: &mut String) {
    out.push_str("<ol>");
    for entry in entries {
        let _ = write!(
            out,
            "<li data-level=\"{}\"><a href=\"#{}\">{}",
            entry.level,
            attr(&entry.id),
            escape_html(&entry.text),
        );
        out.push_str("</a>");
        if !entry.children.is_empty() {
            render_toc_entry_list(&entry.children, out);
        }
        out.push_str("</li>");
    }
    out.push_str("</ol>");
}

/// The TOC is a projection of headings, not a second editable text copy.
/// Keep this deliberately conservative for non-text inline objects: their
/// readable label is already carried by the heading's text/mention/link data.
fn toc_heading_text(block: &Block) -> String {
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
    if text.is_empty() {
        "Untitled heading".to_string()
    } else {
        text
    }
}
