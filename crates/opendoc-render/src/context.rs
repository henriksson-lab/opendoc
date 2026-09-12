//! Per-render lookup tables and the block/inline HTML walk.

use crate::css::{
    block_kind_label, block_property_css, image_size_css, mark_classes, points, table_cell_css,
    write_mark_style, DEFAULT_TABLE_COLUMN_POINTS,
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

pub(crate) fn inline_id(inline: &Inline) -> &StableId {
    match inline {
        Inline::Text { id, .. }
        | Inline::Link { id, .. }
        | Inline::Citation { id, .. }
        | Inline::FootnoteRef { id, .. }
        | Inline::Mention { id, .. }
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
                Anchor::Document => {}
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
                SuggestionKind::Format { range, .. } => {
                    for id in inline_ids_in_range(&range.start, &range.end) {
                        format_suggestions
                            .entry(id)
                            .or_default()
                            .push(suggestion.id.to_string());
                    }
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
                    Anchor::Document => {
                        if let Some(last) = document.blocks.last() {
                            insert_in_block
                                .entry(last.id.to_string())
                                .or_default()
                                .push(suggestion);
                        }
                    }
                },
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
        let mut list = ListWriter::default();
        for block in blocks {
            if let BlockKind::ListItem {
                list_id,
                level,
                kind,
            } = &block.kind
            {
                self.render_list_item(block, list_id, *level, *kind, &mut list, out);
            } else {
                list.close_all(out);
                self.render_block(block, out);
            }
        }
        list.close_all(out);
    }

    fn render_list_item(
        &self,
        block: &Block,
        list_id: &StableId,
        level: u8,
        kind: ListKind,
        list: &mut ListWriter,
        out: &mut String,
    ) {
        let value = list.open_item(out, list_id, level, ListMarker::of(kind));
        let extra = value
            .map(|value| format!(" value=\"{value}\""))
            .unwrap_or_default();
        self.render_block_attributes(block, "li", &extra, out);
        if let Some(checked) = kind.checked() {
            render_checkbox(block.id.as_str(), checked, out);
        }
        self.render_inlines(block, out);
        list.item_content_written();
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
        let layout = format!(
            "table-layout:fixed;width:100%;min-width:{};",
            points(minimum)
        );
        self.render_block_attributes_styled(block, "table", &layout, "", out);
        out.push_str("<colgroup>");
        for column in columns {
            let _ = write!(out, "<col data-column-id=\"{}\"", attr(column.id.as_str()));
            if let Some(width) = column.width {
                let _ = write!(out, " style=\"width:{}\"", points(width.points()));
            }
            out.push('>');
        }
        out.push_str("</colgroup><tbody>");
        let covered = opendoc_core::table_covered_positions(rows);
        for (row_index, row) in rows.iter().enumerate() {
            let _ = write!(out, "<tr data-row-id=\"{}\">", attr(row.id.as_str()));
            for (column_index, cell) in row.cells.iter().enumerate() {
                if covered.contains(&(row_index, column_index)) {
                    continue;
                }
                let _ = write!(out, "<td data-cell-id=\"{}\"", attr(cell.id.as_str()));
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
                out.push_str("</td>");
            }
            out.push_str("</tr>");
        }
        out.push_str("</tbody></table>");
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
                    attr(block.id.as_str())
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
    }

    fn render_block(&self, block: &Block, out: &mut String) {
        match &block.kind {
            BlockKind::Paragraph => {
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
                self.render_list_item(block, list_id, *level, *kind, &mut list, out);
                list.close_all(out);
            }
            BlockKind::Table { columns, rows } => {
                self.render_table(block, columns, rows, out);
            }
            BlockKind::EquationBlock { equation } => {
                let rendered = self.project_equation(equation, EquationDisplay::Block);
                self.render_block_attributes(block, "div", " contenteditable=\"false\"", out);
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
                let extra = format!(
                    " contenteditable=\"false\" data-placement=\"{}\"",
                    layout.effective_placement().as_str()
                );
                self.render_block_attributes(block, "figure", &extra, out);
                let size = image_size_css(layout);
                match self.images.get(blob_hash.as_str()) {
                    Some((media_type, bytes)) => {
                        let _ = write!(
                            out,
                            "<img src=\"data:{};base64,{}\" alt=\"{}\" data-blob-hash=\"{}\" draggable=\"false\"{}>",
                            attr(media_type),
                            base64_encode(bytes),
                            attr(alt_text),
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
                if !alt_text.is_empty() {
                    let _ = write!(out, "<figcaption>{}</figcaption>", escape_html(alt_text));
                }
                out.push_str("</figure>");
            }
            BlockKind::PageBreak => {
                self.render_block_attributes(block, "hr", " contenteditable=\"false\"", out);
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
            " class=\"{}\" data-inline-id=\"{}\" data-kind=\"{kind}\"",
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
                Inline::Mention { label, .. } => out.push_str(&escape_html(label)),
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
