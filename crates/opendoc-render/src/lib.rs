//! HTML rendering of the document body. The frontend inserts this markup
//! into a single `contenteditable` host and maps DOM selections back to
//! block/inline ids using the `data-block-id` / `data-inline-id`
//! attributes, so every shell shares one renderer and the TypeScript layer
//! stays a thin DOM adapter.

use base64::Engine;
use opendoc_core::{
    Anchor, Block, BlockKind, Document, Inline, Mark, MarkKind, StableId, Suggestion,
    SuggestionKind, SuggestionState,
};
use opendoc_spreadsheet::{Cell, SpreadsheetWorkbook};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

/// Per-render lookup tables derived from document state.
struct RenderContext<'a> {
    document: &'a Document,
    /// Footnote id -> 1-based number in order of first reference.
    footnote_numbers: BTreeMap<String, usize>,
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
}

pub fn escape_html(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Image bytes available to the renderer, keyed by content hash.
pub struct RenderImage<'a> {
    pub hash: &'a str,
    pub media_type: &'a str,
    pub bytes: &'a [u8],
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum RenderError {
    NotFound(String),
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for RenderError {}

/// Render the document body as HTML.
pub fn render_document_html<'a>(
    document: &'a Document,
    images: impl IntoIterator<Item = RenderImage<'a>>,
) -> String {
    let context = RenderContext::new(document, images);
    let mut out = String::new();
    context.render_blocks(&document.blocks, &mut out);
    out
}

/// Render footnote bodies, numbered in order of first reference.
pub fn render_footnotes_html<'a>(
    document: &'a Document,
    images: impl IntoIterator<Item = RenderImage<'a>>,
) -> String {
    let context = RenderContext::new(document, images);
    let mut numbered: Vec<(usize, &opendoc_core::Footnote)> = document
        .footnotes
        .iter()
        .filter(|footnote| !footnote.deleted)
        .filter_map(|footnote| {
            context
                .footnote_numbers
                .get(footnote.id.as_str())
                .map(|number| (*number, footnote))
        })
        .collect();
    numbered.sort_by_key(|(number, _)| *number);
    let mut out = String::new();
    if numbered.is_empty() {
        return out;
    }
    out.push_str("<ol class=\"doc-footnotes\">");
    for (number, footnote) in numbered {
        let _ = write!(
            out,
            "<li class=\"doc-footnote\" value=\"{number}\" data-footnote-id=\"{}\">",
            attr(footnote.id.as_str())
        );
        for inline in &footnote.body {
            context.render_inline(inline, &mut out);
        }
        out.push_str("</li>");
    }
    out.push_str("</ol>");
    out
}

/// Render one workbook sheet as an HTML table.
pub fn render_workbook_html(
    workbook: &SpreadsheetWorkbook,
    sheet_id: &str,
) -> Result<String, RenderError> {
    let sheet = workbook
        .sheets
        .iter()
        .find(|sheet| sheet.id == sheet_id)
        .ok_or_else(|| RenderError::NotFound(format!("sheet {sheet_id} was not found")))?;
    let cells: BTreeMap<&str, &Cell> = sheet
        .cells
        .iter()
        .map(|cell| (cell.address.as_str(), cell))
        .collect();

    let mut covered: BTreeSet<String> = BTreeSet::new();
    let mut spans: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for merge in &sheet.merges {
        if let Some(((c1, r1), (c2, r2))) = parse_range(&merge.range) {
            let anchor = format!("{}{}", column_label(c1), r1);
            spans.insert(anchor.clone(), (c2 - c1 + 1, r2 - r1 + 1));
            for column in c1..=c2 {
                for row in r1..=r2 {
                    let address = format!("{}{}", column_label(column), row);
                    if address != anchor {
                        covered.insert(address);
                    }
                }
            }
        }
    }

    let mut out = String::new();
    let _ = write!(
        out,
        "<table class=\"sheet-grid\" data-sheet-id=\"{}\"><thead><tr><th class=\"corner row-header\"></th>",
        attr(&sheet.id)
    );
    for (index, column) in sheet.columns.iter().enumerate() {
        let frozen = (index as u32) < sheet.frozen_columns;
        let _ = write!(
            out,
            "<th class=\"column-header{}\" data-column=\"{}\">{}</th>",
            if frozen { " frozen" } else { "" },
            attr(column),
            escape_html(column)
        );
    }
    out.push_str("</tr></thead><tbody>");
    for (row_index, row) in sheet.rows.iter().enumerate() {
        let frozen_row = (row_index as u32) < sheet.frozen_rows;
        let _ = write!(
            out,
            "<tr data-row=\"{}\"{}><th class=\"row-header\">{}</th>",
            attr(row),
            if frozen_row {
                " class=\"frozen-row\""
            } else {
                ""
            },
            escape_html(row)
        );
        for (column_index, column) in sheet.columns.iter().enumerate() {
            let address = format!("{column}{row}");
            if covered.contains(&address) {
                continue;
            }
            let frozen = (column_index as u32) < sheet.frozen_columns;
            let cell = cells.get(address.as_str()).copied();
            let kind = cell
                .map(|cell| cell.computed_kind.as_str())
                .unwrap_or("empty");
            let display = cell
                .map(|cell| {
                    if cell.computed_kind == "empty" {
                        cell.user_value.clone()
                    } else {
                        cell.computed_value.clone()
                    }
                })
                .unwrap_or_default();
            let mut classes = Vec::new();
            if frozen {
                classes.push("frozen");
            }
            let mut style = String::new();
            if let Some(cell) = cell {
                if cell.format.bold {
                    classes.push("mark-bold");
                }
                if cell.format.italic {
                    classes.push("mark-italic");
                }
                if let Some(color) = cell.format.text_color.as_deref().and_then(css_value) {
                    let _ = write!(style, "color:{color};");
                }
                if let Some(color) = cell.format.background_color.as_deref().and_then(css_value) {
                    let _ = write!(style, "background-color:{color};");
                }
                if let Some(align) = cell.format.horizontal_align.as_deref() {
                    if matches!(align, "left" | "center" | "right") {
                        let _ = write!(style, "text-align:{align};");
                    }
                }
            }
            let _ = write!(
                out,
                "<td data-address=\"{}\" data-kind=\"{}\"",
                attr(&address),
                attr(kind)
            );
            if !classes.is_empty() {
                let _ = write!(out, " class=\"{}\"", classes.join(" "));
            }
            if !style.is_empty() {
                let _ = write!(out, " style=\"{style}\"");
            }
            if let Some((colspan, rowspan)) = spans.get(&address) {
                let _ = write!(out, " colspan=\"{colspan}\" rowspan=\"{rowspan}\"");
            }
            if let Some(cell) = cell {
                if cell.user_value.starts_with('=') {
                    let _ = write!(out, " data-formula=\"{}\"", attr(&cell.user_value));
                }
                if let Some(validation) = &cell.validation {
                    let _ = write!(out, " data-validation=\"{}\"", attr(&validation.kind));
                }
            }
            out.push('>');
            out.push_str(&escape_html(&display));
            if cell
                .map(|cell| cell.comments.iter().any(|comment| !comment.deleted))
                .unwrap_or(false)
            {
                out.push_str("<span class=\"cell-comment-marker\" title=\"Has comments\"></span>");
            }
            out.push_str("</td>");
        }
        out.push_str("</tr>");
    }
    out.push_str("</tbody></table>");
    Ok(out)
}

fn column_label(mut index: usize) -> String {
    let mut label = String::new();
    while index > 0 {
        let remainder = (index - 1) % 26;
        label.insert(0, (b'A' + remainder as u8) as char);
        index = (index - 1) / 26;
    }
    label
}

fn parse_address(address: &str) -> Option<(usize, usize)> {
    let letters: String = address
        .chars()
        .take_while(|ch| ch.is_ascii_alphabetic())
        .collect();
    let digits: String = address.chars().skip(letters.len()).collect();
    if letters.is_empty() || digits.is_empty() {
        return None;
    }
    let mut column = 0usize;
    for ch in letters.chars() {
        column = column * 26 + (ch.to_ascii_uppercase() as usize - 'A' as usize + 1);
    }
    Some((column, digits.parse().ok()?))
}

fn parse_range(range: &str) -> Option<((usize, usize), (usize, usize))> {
    let (start, end) = range.split_once(':')?;
    Some((parse_address(start)?, parse_address(end)?))
}

fn base64_encode(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn attr(value: &str) -> String {
    escape_html(value)
}

/// Keep only characters that are safe inside a CSS declaration value.
fn css_value(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.len() > 64 {
        return None;
    }
    if trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || " #,.()%-_'".contains(ch))
    {
        Some(trimmed.to_string())
    } else {
        None
    }
}

fn safe_href(href: &str) -> String {
    let trimmed = href.trim();
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("mailto:")
        || lower.starts_with("doi:")
        || lower.starts_with('#')
    {
        trimmed.to_string()
    } else if !trimmed.is_empty() && !lower.contains(':') {
        format!("https://{trimmed}")
    } else {
        "#".to_string()
    }
}

fn inline_id(inline: &Inline) -> &StableId {
    match inline {
        Inline::Text { id, .. }
        | Inline::Link { id, .. }
        | Inline::Citation { id, .. }
        | Inline::FootnoteRef { id, .. }
        | Inline::Mention { id, .. }
        | Inline::Equation { id, .. } => id,
    }
}

fn walk_inlines<'a>(blocks: &'a [Block], out: &mut Vec<(&'a StableId, &'a StableId)>) {
    for block in blocks {
        for inline in &block.content {
            out.push((&block.id, inline_id(inline)));
        }
        if let BlockKind::Table { rows } = &block.kind {
            for row in rows {
                for cell in &row.cells {
                    walk_inlines(&cell.blocks, out);
                }
            }
        }
    }
}

impl<'a> RenderContext<'a> {
    fn new(document: &'a Document, images: impl IntoIterator<Item = RenderImage<'a>>) -> Self {
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
        }
    }

    fn render_blocks(&self, blocks: &[Block], out: &mut String) {
        let mut list = ListWriter::default();
        for block in blocks {
            if let BlockKind::ListItem {
                list_id,
                level,
                ordered,
            } = &block.kind
            {
                let value = list.open_item(out, list_id, *level, *ordered);
                let extra = value
                    .map(|value| format!(" value=\"{value}\""))
                    .unwrap_or_default();
                self.render_block_attributes(block, "li", &extra, out);
                self.render_inlines(block, out);
                list.item_content_written();
            } else {
                list.close_all(out);
                self.render_block(block, out);
            }
        }
        list.close_all(out);
    }

    fn render_block_attributes(&self, block: &Block, tag: &str, extra: &str, out: &mut String) {
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
                ordered,
            } => {
                let _ = write!(
                    out,
                    " data-level=\"{level}\" data-list-id=\"{}\" data-ordered=\"{ordered}\"",
                    attr(list_id.as_str())
                );
            }
            _ => {}
        }
        if let Some(threads) = self.comment_blocks.get(block.id.as_str()) {
            let _ = write!(out, " data-comment-ids=\"{}\"", attr(&threads.join(" ")));
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
            BlockKind::ListItem { .. } => {
                // Handled by render_blocks through ListWriter.
                let mut list = ListWriter::default();
                if let BlockKind::ListItem {
                    list_id,
                    level,
                    ordered,
                } = &block.kind
                {
                    let value = list.open_item(out, list_id, *level, *ordered);
                    let extra = value
                        .map(|value| format!(" value=\"{value}\""))
                        .unwrap_or_default();
                    self.render_block_attributes(block, "li", &extra, out);
                    self.render_inlines(block, out);
                    list.item_content_written();
                }
                list.close_all(out);
            }
            BlockKind::Table { rows } => {
                self.render_block_attributes(block, "table", "", out);
                out.push_str("<tbody>");
                for row in rows {
                    let _ = write!(out, "<tr data-row-id=\"{}\">", attr(row.id.as_str()));
                    for cell in &row.cells {
                        let _ = write!(out, "<td data-cell-id=\"{}\">", attr(cell.id.as_str()));
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
            BlockKind::EquationBlock { equation } => {
                self.render_block_attributes(block, "div", " contenteditable=\"false\"", out);
                let _ = write!(
                    out,
                    "<span class=\"equation-source\" contenteditable=\"false\" data-equation-source=\"{}\">{}</span></div>",
                    attr(&equation.source),
                    escape_html(&equation.source)
                );
            }
            BlockKind::Image {
                blob_hash,
                alt_text,
            } => {
                self.render_block_attributes(block, "figure", " contenteditable=\"false\"", out);
                match self.images.get(blob_hash.as_str()) {
                    Some((media_type, bytes)) => {
                        let _ = write!(
                            out,
                            "<img src=\"data:{};base64,{}\" alt=\"{}\" data-blob-hash=\"{}\" draggable=\"false\">",
                            attr(media_type),
                            base64_encode(bytes),
                            attr(alt_text),
                            attr(blob_hash)
                        );
                    }
                    None => {
                        let _ = write!(
                            out,
                            "<div class=\"doc-image-placeholder\" data-blob-hash=\"{}\">{}</div>",
                            attr(blob_hash),
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

    fn render_inline(&self, inline: &Inline, out: &mut String) {
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
                out.push_str("<span");
                self.run_attributes(id, "equation", &["equation-inline"], out);
                let _ = write!(
                    out,
                    " contenteditable=\"false\" data-equation-source=\"{}\">{}</span>",
                    attr(&equation.source),
                    escape_html(&equation.source)
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
                Inline::Equation { equation, .. } => out.push_str(&escape_html(&equation.source)),
                Inline::Citation { citation_id, .. } => {
                    out.push_str(&escape_html(&format!("[{citation_id}]")))
                }
                Inline::FootnoteRef { .. } => out.push_str("[note]"),
            }
        }
        out.push_str("</span>");
    }
}

fn render_text_content(text: &str, out: &mut String) {
    let mut first = true;
    for line in text.split('\n') {
        if !first {
            out.push_str("<br data-soft-break=\"true\">");
        }
        first = false;
        out.push_str(&escape_html(line));
    }
}

fn block_kind_label(kind: &BlockKind) -> &'static str {
    match kind {
        BlockKind::Paragraph => "paragraph",
        BlockKind::Heading { .. } => "heading",
        BlockKind::ListItem { .. } => "list-item",
        BlockKind::Table { .. } => "table",
        BlockKind::EquationBlock { .. } => "equation-block",
        BlockKind::Image { .. } => "image",
        BlockKind::PageBreak => "page-break",
    }
}

fn mark_classes(marks: &[Mark]) -> Vec<&'static str> {
    let mut classes = Vec::new();
    for mark in marks {
        let class = match mark.kind {
            MarkKind::Bold => "mark-bold",
            MarkKind::Italic => "mark-italic",
            MarkKind::Underline => "mark-underline",
            MarkKind::Strike => "mark-strike",
            MarkKind::Code => "mark-code",
            MarkKind::Superscript => "mark-superscript",
            MarkKind::Subscript => "mark-subscript",
            MarkKind::Color => "mark-color",
            MarkKind::Background => "mark-background",
            MarkKind::Font => "mark-font",
            MarkKind::Size => "mark-size",
            MarkKind::Link | MarkKind::Citation => continue,
        };
        if !classes.contains(&class) {
            classes.push(class);
        }
    }
    classes
}

fn write_mark_style(marks: &[Mark], out: &mut String) {
    let mut style = String::new();
    for mark in marks {
        let Some(value) = mark.value.as_deref().and_then(css_value) else {
            continue;
        };
        match mark.kind {
            MarkKind::Color => {
                let _ = write!(style, "color:{value};");
            }
            MarkKind::Background => {
                let _ = write!(style, "background-color:{value};");
            }
            MarkKind::Font => {
                let _ = write!(style, "font-family:{value};");
            }
            MarkKind::Size => {
                let size = if value.chars().all(|ch| ch.is_ascii_digit() || ch == '.') {
                    format!("{value}pt")
                } else {
                    value
                };
                let _ = write!(style, "font-size:{size};");
            }
            _ => {}
        }
    }
    if !style.is_empty() {
        let _ = write!(out, " style=\"{style}\"");
    }
}

fn collect_footnote_numbers(block: &Block, numbers: &mut BTreeMap<String, usize>) {
    for inline in &block.content {
        if let Inline::FootnoteRef { footnote_id, .. } = inline {
            let next = numbers.len() + 1;
            numbers.entry(footnote_id.to_string()).or_insert(next);
        }
    }
    if let BlockKind::Table { rows } = &block.kind {
        for row in rows {
            for cell in &row.cells {
                for nested in &cell.blocks {
                    collect_footnote_numbers(nested, numbers);
                }
            }
        }
    }
}

/// Emits nested `<ol>`/`<ul>` structure for consecutive list items and
/// numbers ordered items per list and level (restarting deeper levels).
#[derive(Default)]
struct ListWriter {
    stack: Vec<ListLevel>,
    counters: BTreeMap<(String, u8), usize>,
}

struct ListLevel {
    level: u8,
    ordered: bool,
    item_open: bool,
}

impl ListWriter {
    fn open_item(
        &mut self,
        out: &mut String,
        list_id: &StableId,
        level: u8,
        ordered: bool,
    ) -> Option<usize> {
        // Close deeper levels and mismatched lists at this level.
        while let Some(top) = self.stack.last() {
            if top.level > level || (top.level == level && top.ordered != ordered) {
                self.close_top(out);
            } else {
                break;
            }
        }
        // Open lists up to the requested level.
        while self
            .stack
            .last()
            .map(|top| top.level < level)
            .unwrap_or(true)
        {
            let next_level = match self.stack.last() {
                Some(top) => top.level + 1,
                None => level,
            };
            let tag = if ordered { "ol" } else { "ul" };
            let _ = write!(
                out,
                "<{tag} class=\"doc-list depth-{next_level}\" data-level=\"{next_level}\">"
            );
            self.stack.push(ListLevel {
                level: next_level,
                ordered,
                item_open: false,
            });
        }
        if let Some(top) = self.stack.last_mut() {
            if top.item_open {
                out.push_str("</li>");
                top.item_open = false;
            }
        }
        // Reset counters for deeper levels of this list.
        let deeper: Vec<(String, u8)> = self
            .counters
            .keys()
            .filter(|(id, item_level)| id == list_id.as_str() && *item_level > level)
            .cloned()
            .collect();
        for key in deeper {
            self.counters.remove(&key);
        }
        let counter = self
            .counters
            .entry((list_id.to_string(), level))
            .or_insert(0);
        *counter += 1;
        let value = *counter;
        // The li tag itself is written by the caller through
        // render_block_attributes with the numbering value.
        ordered.then_some(value)
    }

    fn item_content_written(&mut self) {
        if let Some(top) = self.stack.last_mut() {
            top.item_open = true;
        }
    }

    fn close_top(&mut self, out: &mut String) {
        if let Some(top) = self.stack.pop() {
            if top.item_open {
                out.push_str("</li>");
            }
            out.push_str(if top.ordered { "</ol>" } else { "</ul>" });
        }
    }

    fn close_all(&mut self, out: &mut String) {
        while !self.stack.is_empty() {
            self.close_top(out);
        }
        self.counters.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opendoc_core::{MarkExpand, TextRange};

    fn document_with(paragraphs: &[&str]) -> Document {
        let mut document = Document::new("render");
        document.blocks.clear();
        for text in paragraphs {
            document.blocks.push(Block::paragraph(*text));
        }
        document
    }

    #[test]
    fn renders_paragraphs_with_ids_and_escapes() {
        let document = document_with(&["a < b & c"]);
        let html = render_document_html(&document, []);
        let id = document.blocks[0].id.to_string();
        assert!(html.contains(&format!("data-block-id=\"{id}\"")));
        assert!(html.contains("a &lt; b &amp; c"));
        assert!(html.starts_with("<p class=\"doc-block doc-paragraph\""));
    }

    #[test]
    fn valued_marks_become_inline_styles_and_unsafe_values_are_dropped() {
        let mut document = document_with(&["styled"]);
        if let Inline::Text { marks, .. } = &mut document.blocks[0].content[0] {
            marks.push(Mark {
                kind: MarkKind::Color,
                value: Some("#d93025".to_string()),
                expand: MarkExpand::Both,
            });
            marks.push(Mark {
                kind: MarkKind::Size,
                value: Some("11".to_string()),
                expand: MarkExpand::Both,
            });
            marks.push(Mark {
                kind: MarkKind::Font,
                value: Some("url(evil)\"; x".to_string()),
                expand: MarkExpand::Both,
            });
            marks.push(Mark {
                kind: MarkKind::Bold,
                value: None,
                expand: MarkExpand::Both,
            });
        }
        let html = render_document_html(&document, []);
        assert!(html.contains("color:#d93025;"));
        assert!(html.contains("font-size:11pt;"));
        assert!(!html.contains("evil"));
        assert!(html.contains("mark-bold"));
    }

    #[test]
    fn consecutive_list_items_form_nested_numbered_lists() {
        let mut document = document_with(&["one", "two", "sub", "three", "after"]);
        let list = StableId::new("list");
        for (index, level) in [(0, 0u8), (1, 0), (2, 1), (3, 0)] {
            document.blocks[index].kind = BlockKind::ListItem {
                list_id: list.clone(),
                level,
                ordered: true,
            };
        }
        let html = render_document_html(&document, []);
        assert_eq!(html.matches("<ol").count(), 2, "{html}");
        assert!(html.contains("value=\"1\""));
        assert!(html.contains("value=\"3\""));
        assert!(html.contains("<li class=\"doc-block doc-list-item\""));
        assert!(html.ends_with("after</span></p>"));
        assert_eq!(html.matches("<li").count(), html.matches("</li>").count());
        assert_eq!(html.matches("<ol").count(), html.matches("</ol>").count());
    }

    #[test]
    fn footnotes_are_numbered_links_are_sanitized_and_empty_blocks_get_a_break() {
        let mut document = document_with(&["see", ""]);
        let footnote = opendoc_core::Footnote {
            id: StableId::new("footnote"),
            revision: 1,
            body: vec![Inline::text("note body")],
            deleted: false,
        };
        document.blocks[0].content.push(Inline::FootnoteRef {
            id: StableId::new("ref"),
            footnote_id: footnote.id.clone(),
        });
        document.blocks[0].content.push(Inline::Link {
            id: StableId::new("link"),
            text: "x".to_string(),
            href: "javascript:alert(1)".to_string(),
            marks: Vec::new(),
        });
        document.footnotes.push(footnote);
        let html = render_document_html(&document, []);
        assert!(html.contains("footnote-ref\" data-inline-id"));
        assert!(html.contains(">1</sup>"));
        assert!(html.contains("href=\"#\""));
        assert!(html.contains("<br data-caret-anchor=\"true\">"));
        let notes = render_footnotes_html(&document, []);
        assert!(notes.contains("value=\"1\""));
        assert!(notes.contains("note body"));
    }

    #[test]
    fn comments_and_suggestions_are_marked_on_runs() {
        let mut document = document_with(&["alpha", "beta"]);
        let first = inline_id(&document.blocks[0].content[0]).clone();
        let second = inline_id(&document.blocks[1].content[0]).clone();
        document.comments.push(opendoc_core::CommentThread {
            id: StableId::new("thread"),
            anchor: Anchor::TextRange(TextRange {
                start: first.clone(),
                end: second.clone(),
            }),
            comments: Vec::new(),
            deleted: false,
        });
        document.suggestions.push(Suggestion {
            id: StableId::new("suggestion"),
            author: "Editor".to_string(),
            kind: SuggestionKind::Insert {
                anchor: Anchor::TextRange(TextRange {
                    start: first.clone(),
                    end: first.clone(),
                }),
                content: vec![Inline::text(" inserted")],
            },
            state: SuggestionState::Proposed,
            provenance: Vec::new(),
        });
        let html = render_document_html(&document, []);
        assert_eq!(html.matches("has-comment").count(), 2);
        assert!(html.contains("suggested-insert"));
        assert!(html.contains(" inserted</span>"));
    }

    #[test]
    fn renders_spreadsheet_grid_with_addresses_and_merges() {
        let workbook = SpreadsheetWorkbook::sample();
        let sheet_id = workbook.sheets[0].id.clone();
        let html = render_workbook_html(&workbook, &sheet_id).unwrap();
        assert!(html.starts_with("<table class=\"sheet-grid\""));
        assert!(html.contains("data-address=\"A1\""));
        assert!(render_workbook_html(&workbook, "missing").is_err());
        assert_eq!(column_label(28), "AB");
        assert_eq!(parse_address("AB12"), Some((28, 12)));
    }
}
