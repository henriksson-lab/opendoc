//! The converter: WordprocessingML body into the canonical document model.

use crate::docx::package::{media_name, DocxParts};
use crate::docx::props::{
    overlay_para_props, parse_para_props, parse_run_props, toggle_value, RunProps,
};
use crate::docx::revisions::{
    source_author, CommentRange, DeleteCapture, FieldState, InsertCapture, ParagraphState,
    Revision, Segment,
};
use crate::docx::section::{
    furniture_references, page_number_field, parse_page_setup,
    section_has_unrepresentable_properties,
};
use crate::docx::styles::{parse_num_pr, HeadingStyle};
use crate::docx::util::{collect_wrapped, parse_iso_datetime_ms};
use crate::docx::warnings::{
    DroppedCounter, COMMENT_ANCHOR_DEGRADED, DROPPED_ALT_CHUNK, DROPPED_CELL_SPAN, DROPPED_DRAWING,
    DROPPED_FORMAT_CHANGE, DROPPED_FURNITURE_CONTENT, DROPPED_HEADER_FOOTER, DROPPED_NESTED_IMAGE,
    DROPPED_NESTED_REVISION, DROPPED_PARAGRAPH_CHANGE, DROPPED_RUN_PROPERTY,
    DROPPED_SECTION_PROPERTIES, DROPPED_TEXT_BOX, EMPTY_COMMENT, EMPTY_FOOTNOTE,
    ENDNOTES_AS_FOOTNOTES, INVALID_PAGE_SETUP, MISSING_FOOTNOTE, MISSING_IMAGE_BLOB, NESTED_TABLE,
    SPLIT_INLINE_IMAGE, SPLIT_PAGE_BREAK, TITLE_STYLE_AS_HEADING, UNKNOWN_LIST_DEFINITION,
};
use crate::docx::DocxImport;
use crate::xml::XmlElement;
use crate::ImportError;
use opendoc_core::{
    Anchor, Block, BlockKind, BlockProperties, Comment, CommentThread, Document, Equation,
    EquationSourceFormat, Footnote, Inline, ListKind, Mark, ModelWarning, StableId, Suggestion,
    SuggestionKind, SuggestionState, TableCell, TableRow, TextRange,
};
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct Converter<'a> {
    parts: &'a DocxParts,
    warnings: Vec<ModelWarning>,
    dropped: DroppedCounter,
    list_ids: BTreeMap<String, StableId>,
    note_ids: BTreeMap<(bool, String), StableId>,
    footnotes: Vec<Footnote>,
    comment_ranges: BTreeMap<String, CommentRange>,
    open_comment_ranges: Vec<String>,
    suggestions: Vec<Suggestion>,
    block_ids: BTreeSet<StableId>,
}

pub(super) fn convert_parts(title: &str, parts: &DocxParts) -> Result<DocxImport, ImportError> {
    let mut converter = Converter {
        parts,
        warnings: Vec::new(),
        dropped: DroppedCounter::default(),
        list_ids: BTreeMap::new(),
        note_ids: BTreeMap::new(),
        footnotes: Vec::new(),
        comment_ranges: BTreeMap::new(),
        open_comment_ranges: Vec::new(),
        suggestions: Vec::new(),
        block_ids: BTreeSet::new(),
    };
    if let Some(footnotes) = &parts.footnotes {
        converter.import_notes(footnotes, false);
    }
    if let Some(endnotes) = &parts.endnotes {
        converter.import_notes(endnotes, true);
    }

    let mut document = Document::new(title);
    let body = parts.document.child("body").unwrap_or(&parts.document);
    converter.walk_blocks(body, &mut document.blocks, 0);
    if document.blocks.is_empty() {
        return Err(ImportError::EmptyInput);
    }
    // Section properties are read after the body, not during the walk: the
    // body-level `w:sectPr` is the *document's* page, while a `w:sectPr`
    // inside a paragraph's `w:pPr` marks an extra section OpenDoc has no
    // model for, and only the position tells them apart.
    converter.import_section(body.child("sectPr"), &mut document);
    document.comments = converter.import_comments();
    document.footnotes = std::mem::take(&mut converter.footnotes);
    document.suggestions = std::mem::take(&mut converter.suggestions);
    let mut warnings = std::mem::take(&mut converter.warnings);
    warnings.extend(converter.dropped.into_warnings());
    document.warnings = warnings.clone();
    document
        .validate()
        .map_err(|err| ImportError::InvalidDocument(err.to_string()))?;

    let mut seen_hashes = BTreeSet::new();
    let blobs = parts
        .media
        .values()
        .filter(|blob| seen_hashes.insert(blob.hash.clone()))
        .cloned()
        .collect();
    Ok(DocxImport {
        document,
        warnings,
        blobs,
    })
}

// ---------------------------------------------------------------------------
// Section properties (w:sectPr)
// ---------------------------------------------------------------------------

pub(super) fn inline_id(inline: &Inline) -> &StableId {
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

pub(super) fn inlines_have_source(inlines: &[Inline]) -> bool {
    inlines.iter().any(|inline| match inline {
        Inline::Text { text, .. } | Inline::Link { text, .. } => !text.trim().is_empty(),
        _ => true,
    })
}

pub(super) fn math_source(element: &XmlElement) -> Option<String> {
    let mut out = String::new();
    for descendant in element.descendants() {
        if descendant.is("t") && descendant.prefix.as_deref() != Some("w") {
            out.push_str(&descendant.text());
        }
    }
    let trimmed = out.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

pub(super) fn equation_inline(source: String) -> Inline {
    Inline::Equation {
        id: StableId::new("equation"),
        equation: Equation {
            id: StableId::new("eq"),
            source_format: EquationSourceFormat::LatexLike,
            source,
        },
    }
}

pub(super) fn hyperlink_field_target(instruction: &str) -> Option<String> {
    let trimmed = instruction.trim();
    let rest = trimmed.strip_prefix("HYPERLINK")?;
    let quoted: Vec<&str> = rest
        .split('"')
        .enumerate()
        .filter(|(index, _)| index % 2 == 1)
        .map(|(_, value)| value)
        .collect();
    let mut is_local = false;
    let mut target: Option<&str> = None;
    let mut expect_local_value = false;
    for token in rest.split_whitespace() {
        if expect_local_value {
            expect_local_value = false;
            if let Some(value) = token.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
                target = Some(value);
                is_local = true;
            }
        } else if token == "\\l" {
            expect_local_value = true;
        }
    }
    let target = target
        .or_else(|| quoted.first().copied())
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    Some(if is_local {
        format!("#{target}")
    } else {
        target.to_string()
    })
}

impl<'a> Converter<'a> {
    // -- warnings ---------------------------------------------------------

    fn count(&mut self, code: &'static str) {
        self.dropped.count(code);
    }

    // -- notes ------------------------------------------------------------

    fn import_notes(&mut self, part: &XmlElement, endnote: bool) {
        let element_name = if endnote { "endnote" } else { "footnote" };
        let mut imported = 0;
        for note in part.children_named(element_name) {
            let Some(id) = note.attr("id").map(str::trim) else {
                continue;
            };
            if note.attr("type").is_some_and(|kind| {
                matches!(
                    kind.trim(),
                    "separator" | "continuationSeparator" | "continuationNotice"
                )
            }) {
                continue;
            }
            let body = self.nested_inline_body(note);
            if !inlines_have_source(&body) {
                self.count(EMPTY_FOOTNOTE);
                continue;
            }
            let stable_id = StableId::new(element_name);
            self.note_ids
                .insert((endnote, id.to_string()), stable_id.clone());
            self.footnotes.push(Footnote {
                id: stable_id,
                revision: 1,
                body,
                deleted: false,
            });
            imported += 1;
        }
        if endnote {
            self.dropped.count_n(ENDNOTES_AS_FOOTNOTES, imported);
        }
    }

    /// Converts every paragraph below `container` into one inline sequence,
    /// separating paragraphs with newline text (footnote and comment bodies).
    fn nested_inline_body(&mut self, container: &XmlElement) -> Vec<Inline> {
        let mut body = Vec::new();
        for paragraph in container
            .descendants()
            .into_iter()
            .filter(|element| element.is("p"))
        {
            let mut state = ParagraphState::new(
                BlockKind::Paragraph,
                BlockProperties::default(),
                RunProps::default(),
                true,
            );
            self.walk_paragraph_content(paragraph, &mut state);
            let mut inlines = Vec::new();
            for segment in state.segments {
                match segment {
                    Segment::Inline(inline) | Segment::MathPara(inline) => inlines.push(inline),
                    Segment::PageBreak => inlines.push(Inline::text("\n")),
                    Segment::Image { .. } => {}
                }
            }
            if inlines.is_empty() {
                continue;
            }
            if !body.is_empty() {
                body.push(Inline::text("\n"));
            }
            body.extend(inlines);
        }
        body
    }

    // -- comments ---------------------------------------------------------

    fn import_comments(&mut self) -> Vec<CommentThread> {
        let Some(part) = &self.parts.comments else {
            return Vec::new();
        };
        let parent_of: BTreeMap<String, String> = self
            .parts
            .comments_extended
            .as_ref()
            .map(|extended| {
                extended
                    .children_named("commentEx")
                    .filter_map(|entry| {
                        Some((
                            entry.attr("paraId")?.trim().to_string(),
                            entry.attr("paraIdParent")?.trim().to_string(),
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default();

        struct Record {
            docx_id: String,
            para_id: Option<String>,
            comment: Comment,
        }
        let mut records = Vec::new();
        for element in part.children_named("comment") {
            let Some(docx_id) = element.attr("id").map(str::trim) else {
                continue;
            };
            let body = self.nested_inline_body(element);
            if !inlines_have_source(&body) {
                self.count(EMPTY_COMMENT);
                continue;
            }
            let para_id = element
                .find_descendant("p")
                .and_then(|paragraph| paragraph.attr("paraId"))
                .map(|value| value.trim().to_string());
            records.push(Record {
                docx_id: docx_id.to_string(),
                para_id,
                comment: Comment {
                    id: StableId::new("comment"),
                    author: source_author(element.attr("author")),
                    body,
                    created_at_ms: element
                        .attr("date")
                        .and_then(parse_iso_datetime_ms)
                        .unwrap_or(0),
                    deleted: false,
                },
            });
        }

        // Group replies (commentsExtended parent links) under their root comment.
        let mut thread_of_para: BTreeMap<String, usize> = BTreeMap::new();
        let mut threads: Vec<(String, Vec<Comment>)> = Vec::new();
        for record in records {
            let parent_thread = record
                .para_id
                .as_ref()
                .and_then(|para_id| parent_of.get(para_id))
                .and_then(|parent| thread_of_para.get(parent))
                .copied();
            let index = match parent_thread {
                Some(index) => {
                    threads[index].1.push(record.comment);
                    index
                }
                None => {
                    threads.push((record.docx_id.clone(), vec![record.comment]));
                    threads.len() - 1
                }
            };
            if let Some(para_id) = record.para_id {
                thread_of_para.insert(para_id, index);
            }
        }

        threads
            .into_iter()
            .map(|(docx_id, comments)| CommentThread {
                id: StableId::new("comment-thread"),
                anchor: self.comment_anchor(&docx_id),
                comments,
                deleted: false,
            })
            .collect()
    }

    fn comment_anchor(&mut self, docx_id: &str) -> Anchor {
        let range = self
            .comment_ranges
            .get(docx_id)
            .cloned()
            .unwrap_or_default();
        if let (Some(start), Some(end)) = (range.start, range.end) {
            return Anchor::TextRange(TextRange { start, end });
        }
        self.count(COMMENT_ANCHOR_DEGRADED);
        match range.block_id.filter(|id| self.block_ids.contains(id)) {
            Some(block_id) => Anchor::NearestBlock {
                block_id,
                warning:
                    "DOCX comment range did not cover inline text; anchored to the nearest block"
                        .to_string(),
            },
            None => Anchor::Document,
        }
    }

    fn open_comment_range(&mut self, docx_id: &str) {
        self.comment_ranges.entry(docx_id.to_string()).or_default();
        if !self.open_comment_ranges.iter().any(|id| id == docx_id) {
            self.open_comment_ranges.push(docx_id.to_string());
        }
    }

    fn close_comment_range(&mut self, docx_id: &str) {
        self.open_comment_ranges.retain(|id| id != docx_id);
    }

    // -- block-level walk -------------------------------------------------

    // -- section properties ------------------------------------------------

    /// Reads the body-level `w:sectPr` into the document's page setup and its
    /// header and footer.
    ///
    /// A missing `w:sectPr` is not a warning: every dimension keeps the
    /// default the model already holds, which is what Word itself assumes.
    fn import_section(&mut self, sect_pr: Option<&XmlElement>, document: &mut Document) {
        let Some(sect_pr) = sect_pr else {
            return;
        };
        match parse_page_setup(sect_pr) {
            Some(setup) => document.page_setup = setup,
            None => self.count(INVALID_PAGE_SETUP),
        }
        if section_has_unrepresentable_properties(sect_pr) {
            self.count(DROPPED_SECTION_PROPERTIES);
        }
        for reference in furniture_references(sect_pr) {
            // One header and one footer for the whole document (ADR 0009), so
            // a first-page or even-page variant is named rather than silently
            // applied everywhere.
            if reference.variant != "default" {
                self.count(DROPPED_HEADER_FOOTER);
                continue;
            }
            let Some(part) = self.parts.furniture_parts.get(reference.rel_id) else {
                self.count(DROPPED_HEADER_FOOTER);
                continue;
            };
            let mut blocks = Vec::new();
            self.walk_blocks(part, &mut blocks, 0);
            self.strip_unfurnishable(&mut blocks);
            if blocks.is_empty() {
                continue;
            }
            *document.furniture_mut(reference.slot) = blocks;
        }
    }

    /// Removes what page furniture may not hold. `Document::validate()` would
    /// refuse the whole import over a page break in a header, which would cost
    /// the user a document for a decoration; dropping it with a warning is the
    /// same trade the rest of this reader makes.
    fn strip_unfurnishable(&mut self, blocks: &mut Vec<Block>) {
        let before = blocks.len();
        blocks.retain(|block| !matches!(block.kind, BlockKind::PageBreak));
        self.dropped
            .count_n(DROPPED_FURNITURE_CONTENT, before - blocks.len());
        for block in blocks.iter_mut() {
            let before = block.content.len();
            block
                .content
                .retain(|inline| !matches!(inline, Inline::FootnoteRef { .. }));
            self.dropped
                .count_n(DROPPED_FURNITURE_CONTENT, before - block.content.len());
            if let BlockKind::Table { rows, .. } = &mut block.kind {
                for row in rows.iter_mut() {
                    for cell in row.cells.iter_mut() {
                        self.strip_unfurnishable(&mut cell.blocks);
                    }
                }
            }
        }
        // A paragraph whose only content was a footnote reference is now
        // empty, and an empty text block is not something this reader ever
        // produces; an image or a table legitimately has no inline content.
        blocks.retain(|block| {
            !block.content.is_empty()
                || !matches!(
                    block.kind,
                    BlockKind::Paragraph | BlockKind::Heading { .. } | BlockKind::ListItem { .. }
                )
        });
    }

    fn walk_blocks(&mut self, container: &XmlElement, out: &mut Vec<Block>, table_depth: usize) {
        for element in container.elements() {
            match element.local.as_str() {
                "p" => self.convert_paragraph(element, out),
                "tbl" => {
                    if table_depth > 0 {
                        self.count(NESTED_TABLE);
                    }
                    let block = self.convert_table(element, table_depth);
                    self.block_ids.insert(block.id.clone());
                    out.push(block);
                }
                // Read by `import_section` once the body walk is done, so
                // counting it here would report the page as dropped.
                "sectPr" => {}
                "altChunk" => self.count(DROPPED_ALT_CHUNK),
                "commentRangeStart" => {
                    if let Some(id) = element.attr("id") {
                        self.open_comment_range(id.trim());
                    }
                }
                "commentRangeEnd" => {
                    if let Some(id) = element.attr("id") {
                        self.close_comment_range(id.trim());
                    }
                }
                "pPr" | "tblPr" | "tblGrid" | "trPr" | "tcPr" | "sdtPr" | "sdtEndPr"
                | "bookmarkStart" | "bookmarkEnd" | "proofErr" | "customXmlPr" => {}
                _ => self.walk_blocks(element, out, table_depth),
            }
        }
    }

    fn convert_table(&mut self, table: &XmlElement, table_depth: usize) -> Block {
        let mut rows = Vec::new();
        let mut row_elements = Vec::new();
        collect_wrapped(table, "tr", &mut row_elements);
        for row in row_elements {
            let mut cells = Vec::new();
            let mut cell_elements = Vec::new();
            collect_wrapped(row, "tc", &mut cell_elements);
            for cell in cell_elements {
                if let Some(tc_pr) = cell.child("tcPr") {
                    if tc_pr.child("gridSpan").is_some() || tc_pr.child("vMerge").is_some() {
                        self.count(DROPPED_CELL_SPAN);
                    }
                }
                let mut blocks = Vec::new();
                self.walk_blocks(cell, &mut blocks, table_depth + 1);
                if blocks.is_empty() {
                    let block = Block::paragraph("");
                    self.block_ids.insert(block.id.clone());
                    blocks.push(block);
                }
                cells.push(TableCell::new(blocks));
            }
            if cells.is_empty() {
                cells.push(self.empty_cell());
            }
            rows.push(TableRow {
                id: StableId::new("row"),
                cells,
            });
        }
        if rows.is_empty() {
            rows.push(TableRow {
                id: StableId::new("row"),
                cells: vec![self.empty_cell()],
            });
        }
        Block {
            id: StableId::new("block"),
            kind: BlockKind::table(rows),
            content: Vec::new(),
            properties: BlockProperties::default(),
        }
    }

    fn empty_cell(&mut self) -> TableCell {
        let block = Block::paragraph("");
        self.block_ids.insert(block.id.clone());
        TableCell::new(vec![block])
    }

    // -- paragraphs -------------------------------------------------------

    fn convert_paragraph(&mut self, paragraph: &XmlElement, out: &mut Vec<Block>) {
        let ppr = paragraph.child("pPr");
        let style = ppr
            .and_then(|ppr| ppr.child_val("pStyle"))
            .map(|id| self.parts.styles.resolve(id));
        let mut page_break_before = false;
        let mut properties = BlockProperties::default();
        if let Some(style) = style.as_ref() {
            overlay_para_props(&mut properties, &style.para_props);
            for code in style.para_dropped.clone() {
                self.count(code);
            }
        }
        if let Some(ppr) = ppr {
            let direct = parse_para_props(ppr);
            overlay_para_props(&mut properties, &direct.props);
            for code in direct.dropped {
                self.count(code);
            }
            for property in ppr.elements() {
                match property.local.as_str() {
                    "pPrChange" => self.count(DROPPED_PARAGRAPH_CHANGE),
                    "sectPr" => self.count(DROPPED_SECTION_PROPERTIES),
                    "pageBreakBefore" => page_break_before = toggle_value(property),
                    _ => {}
                }
            }
        }
        let direct_num_pr = ppr
            .and_then(|ppr| ppr.child("numPr"))
            .map(|num_pr| parse_num_pr(num_pr).filter(|num_pr| num_pr.num_id != "0"));
        let num_pr = match direct_num_pr {
            Some(direct) => direct,
            None => style.as_ref().and_then(|style| style.num_pr.clone()),
        };
        let kind = match style.as_ref().and_then(|style| style.heading) {
            Some(HeadingStyle::Title) => {
                self.count(TITLE_STYLE_AS_HEADING);
                BlockKind::Heading { level: 1 }
            }
            Some(HeadingStyle::Subtitle) => {
                self.count(TITLE_STYLE_AS_HEADING);
                BlockKind::Heading { level: 2 }
            }
            Some(HeadingStyle::Level(level)) => BlockKind::Heading {
                level: level.clamp(1, 6),
            },
            None => match num_pr {
                Some(num_pr) => {
                    let ordered = match self.parts.numbering.is_ordered(
                        &self.parts.styles,
                        &num_pr.num_id,
                        num_pr.level,
                    ) {
                        Some(ordered) => ordered,
                        None => {
                            self.count(UNKNOWN_LIST_DEFINITION);
                            false
                        }
                    };
                    let list_id = self
                        .list_ids
                        .entry(num_pr.num_id.clone())
                        .or_insert_with(|| StableId::new("docx-list"))
                        .clone();
                    BlockKind::ListItem {
                        list_id,
                        level: num_pr.level.min(8),
                        kind: if ordered {
                            ListKind::Ordered
                        } else {
                            ListKind::Bullet
                        },
                    }
                }
                None => BlockKind::Paragraph,
            },
        };
        let style_props = style.map(|style| style.run_props).unwrap_or_default();

        if page_break_before {
            out.push(self.page_break_block());
        }
        let mut state = ParagraphState::new(kind, properties, style_props, false);
        self.walk_paragraph_content(paragraph, &mut state);
        self.finish_paragraph(state, out);
    }

    fn page_break_block(&mut self) -> Block {
        let block = Block {
            id: StableId::new("block"),
            kind: BlockKind::PageBreak,
            content: Vec::new(),
            properties: BlockProperties::default(),
        };
        self.block_ids.insert(block.id.clone());
        block
    }

    fn finish_paragraph(&mut self, state: ParagraphState, out: &mut Vec<Block>) {
        let ParagraphState {
            kind,
            properties,
            segments,
            fragment_ids,
            ..
        } = state;
        if segments.len() == 1 {
            if let Some(Segment::MathPara(Inline::Equation { equation, .. })) = segments.first() {
                let block = Block {
                    id: fragment_ids[0].clone(),
                    kind: BlockKind::EquationBlock {
                        equation: equation.clone(),
                    },
                    content: Vec::new(),
                    properties,
                };
                self.block_ids.insert(block.id.clone());
                out.push(block);
                return;
            }
        }
        let is_text =
            |segment: &Segment| matches!(segment, Segment::Inline(_) | Segment::MathPara(_));
        let text_count = segments.iter().filter(|segment| is_text(segment)).count();
        let mut fragment = 0;
        let mut current: Vec<Inline> = Vec::new();
        for segment in segments {
            match segment {
                Segment::Inline(inline) | Segment::MathPara(inline) => current.push(inline),
                Segment::PageBreak => {
                    if text_count > 0 {
                        self.count(SPLIT_PAGE_BREAK);
                    }
                    self.flush_fragment(
                        &kind,
                        &properties,
                        &fragment_ids,
                        &mut fragment,
                        &mut current,
                        out,
                    );
                    let block = self.page_break_block();
                    out.push(block);
                }
                Segment::Image { rel_id, alt } => {
                    if text_count > 0 {
                        self.count(SPLIT_INLINE_IMAGE);
                    }
                    self.flush_fragment(
                        &kind,
                        &properties,
                        &fragment_ids,
                        &mut fragment,
                        &mut current,
                        out,
                    );
                    let block = self.image_block(&rel_id, alt);
                    out.push(block);
                }
            }
        }
        self.flush_fragment(
            &kind,
            &properties,
            &fragment_ids,
            &mut fragment,
            &mut current,
            out,
        );
    }

    fn flush_fragment(
        &mut self,
        kind: &BlockKind,
        properties: &BlockProperties,
        fragment_ids: &[StableId],
        fragment: &mut usize,
        current: &mut Vec<Inline>,
        out: &mut Vec<Block>,
    ) {
        let id = fragment_ids
            .get(*fragment)
            .cloned()
            .unwrap_or_else(|| StableId::new("block"));
        *fragment += 1;
        if current.is_empty() {
            return;
        }
        let block = Block {
            id,
            kind: kind.clone(),
            content: std::mem::take(current),
            properties: properties.clone(),
        };
        self.block_ids.insert(block.id.clone());
        out.push(block);
    }

    fn image_block(&mut self, rel_id: &str, alt: Option<String>) -> Block {
        let block = match self.parts.media.get(rel_id) {
            Some(blob) => Block {
                id: StableId::new("block"),
                kind: BlockKind::Image {
                    blob_hash: blob.hash.clone(),
                    alt_text: alt.unwrap_or_else(|| blob.name.clone()),
                    layout: Default::default(),
                },
                content: Vec::new(),
                properties: BlockProperties::default(),
            },
            None => {
                let target = self
                    .parts
                    .relationships
                    .get(rel_id)
                    .map(|rel| rel.target.clone())
                    .unwrap_or_else(|| rel_id.to_string());
                let name = media_name(&target);
                self.warnings.push(ModelWarning {
                    code: MISSING_IMAGE_BLOB.to_string(),
                    message: format!(
                        "DOCX image relationship {rel_id} target {target} could not be read"
                    ),
                });
                Block::paragraph(format!("[missing DOCX image: {name}]"))
            }
        };
        self.block_ids.insert(block.id.clone());
        block
    }

    // -- inline-level walk ------------------------------------------------

    fn walk_paragraph_content(&mut self, container: &XmlElement, state: &mut ParagraphState) {
        for element in container.elements() {
            match element.local.as_str() {
                "pPr" | "rPr" | "bookmarkStart" | "bookmarkEnd" | "proofErr" | "sdtPr"
                | "sdtEndPr" | "customXmlPr" => {}
                "r" => self.walk_run(element, state),
                "hyperlink" => self.walk_hyperlink(element, state),
                "fldSimple" => {
                    // A page number stated as `w:fldSimple` is the same field
                    // as the three-run `w:fldChar` form; the cached result
                    // inside it is dropped for the same reason.
                    if let Some(field) = element.attr("instr").and_then(page_number_field) {
                        self.emit(
                            state,
                            Inline::PageNumber {
                                id: StableId::new("page-number"),
                                field,
                            },
                        );
                        continue;
                    }
                    let href = element.attr("instr").and_then(hyperlink_field_target);
                    if let Some(href) = href {
                        state.links.push(href);
                        self.walk_paragraph_content(element, state);
                        state.links.pop();
                    } else {
                        self.walk_paragraph_content(element, state);
                    }
                }
                "ins" | "moveTo" => self.walk_insertion(element, state),
                "del" | "moveFrom" => self.walk_deletion(element, state),
                "oMathPara" => {
                    if let Some(source) = math_source(element) {
                        let inline = equation_inline(source);
                        self.emit_segment(state, Segment::MathPara(inline));
                    }
                }
                "oMath" => {
                    if let Some(source) = math_source(element) {
                        self.emit(state, equation_inline(source));
                    }
                }
                "commentRangeStart" => {
                    if !state.nested {
                        if let Some(id) = element.attr("id") {
                            self.open_comment_range(id.trim());
                        }
                    }
                }
                "commentRangeEnd" => {
                    if !state.nested {
                        if let Some(id) = element.attr("id") {
                            self.close_comment_range(id.trim());
                        }
                    }
                }
                "tbl" | "p" => {
                    // Paragraph content never nests block content directly; text boxes
                    // and similar wrappers are handled by the run walker.
                }
                _ => self.walk_paragraph_content(element, state),
            }
        }
    }

    fn walk_hyperlink(&mut self, element: &XmlElement, state: &mut ParagraphState) {
        let href = element
            .attr_prefixed("r", "id")
            .and_then(|id| self.parts.relationships.get(id.trim()))
            .map(|rel| rel.target.clone())
            .filter(|target| !target.trim().is_empty())
            .or_else(|| {
                element
                    .attr("anchor")
                    .map(str::trim)
                    .filter(|anchor| !anchor.is_empty())
                    .map(|anchor| format!("#{anchor}"))
            })
            .or_else(|| {
                element
                    .attr("docLocation")
                    .map(str::trim)
                    .filter(|location| !location.is_empty())
                    .map(|location| format!("#{location}"))
            });
        match href {
            Some(href) => {
                state.links.push(href);
                self.walk_paragraph_content(element, state);
                state.links.pop();
            }
            None => self.walk_paragraph_content(element, state),
        }
    }

    fn walk_insertion(&mut self, element: &XmlElement, state: &mut ParagraphState) {
        if state.nested {
            self.count(DROPPED_NESTED_REVISION);
            self.walk_paragraph_content(element, state);
            return;
        }
        let kind = if element.is("moveTo") {
            "moveTo"
        } else {
            "ins"
        };
        let revision = Revision::from_element(kind, element);
        let anchor = match state.last_inline_id.clone() {
            Some(id) => id,
            None => {
                let placeholder = Inline::text("");
                let id = inline_id(&placeholder).clone();
                self.emit(state, placeholder);
                id
            }
        };
        state.inserts.push(InsertCapture {
            revision,
            anchor,
            content: Vec::new(),
        });
        self.walk_paragraph_content(element, state);
        let Some(capture) = state.inserts.pop() else {
            return;
        };
        if !inlines_have_source(&capture.content) {
            return;
        }
        self.suggestions.push(Suggestion {
            id: StableId::new("suggestion"),
            author: capture.revision.author.clone(),
            kind: SuggestionKind::Insert {
                anchor: Anchor::TextRange(TextRange {
                    start: capture.anchor.clone(),
                    end: capture.anchor,
                }),
                content: capture.content,
            },
            state: SuggestionState::Proposed,
            provenance: capture.revision.provenance(),
        });
    }

    fn walk_deletion(&mut self, element: &XmlElement, state: &mut ParagraphState) {
        if state.nested || !state.inserts.is_empty() {
            self.count(DROPPED_NESTED_REVISION);
            self.walk_paragraph_content(element, state);
            return;
        }
        let kind = if element.is("moveFrom") {
            "moveFrom"
        } else {
            "del"
        };
        state.deletes.push(DeleteCapture {
            revision: Revision::from_element(kind, element),
            start: None,
            end: None,
        });
        self.walk_paragraph_content(element, state);
        let Some(capture) = state.deletes.pop() else {
            return;
        };
        if let (Some(start), Some(end)) = (capture.start, capture.end) {
            self.suggestions.push(Suggestion {
                id: StableId::new("suggestion"),
                author: capture.revision.author.clone(),
                kind: SuggestionKind::Delete {
                    range: TextRange { start, end },
                },
                state: SuggestionState::Proposed,
                provenance: capture.revision.provenance(),
            });
        }
    }

    fn walk_run(&mut self, run: &XmlElement, state: &mut ParagraphState) {
        let parsed = run.child("rPr").map(parse_run_props).unwrap_or_default();
        let mut base = state.style_props.clone();
        if let Some(style) = &parsed.style {
            base.overlay(&self.parts.styles.resolve(style).run_props);
        }
        let (props, format_change) = match &parsed.previous {
            Some(previous) => {
                let mut old = base.clone();
                old.overlay(previous);
                let mut new = base;
                new.overlay(&parsed.props);
                let old_marks = old.marks();
                let added: Vec<Mark> = new
                    .marks()
                    .into_iter()
                    .filter(|mark| !old_marks.contains(mark))
                    .collect();
                (old, Some(added))
            }
            None => {
                base.overlay(&parsed.props);
                (base, None)
            }
        };
        if !parsed.dropped.is_empty() {
            self.count(DROPPED_RUN_PROPERTY);
            self.dropped
                .run_property_names
                .extend(parsed.dropped.iter().copied());
        }
        let marks = props.marks();
        let mut text = String::new();
        let mut run_inline_ids: Vec<StableId> = Vec::new();

        for child in run.elements() {
            match child.local.as_str() {
                "rPr" => {}
                "t" | "delText" => {
                    if !state.in_field_instruction() && !state.in_suppressed_field_result() {
                        text.push_str(&child.text());
                    }
                }
                "tab" | "ptab" => text.push('\t'),
                "br" => {
                    let is_page = child
                        .attr("type")
                        .is_some_and(|kind| kind.trim().eq_ignore_ascii_case("page"));
                    if is_page && !state.nested && state.inserts.is_empty() {
                        self.flush_run_text(state, &mut text, &marks, &mut run_inline_ids);
                        self.emit_segment(state, Segment::PageBreak);
                    } else {
                        text.push('\n');
                    }
                }
                "cr" => text.push('\n'),
                "noBreakHyphen" => text.push('\u{2011}'),
                "sym" => {
                    if let Some(ch) = child
                        .attr("char")
                        .and_then(|value| u32::from_str_radix(value.trim(), 16).ok())
                        .and_then(char::from_u32)
                    {
                        text.push(ch);
                    }
                }
                "footnoteReference" | "endnoteReference" => {
                    self.flush_run_text(state, &mut text, &marks, &mut run_inline_ids);
                    let endnote = child.is("endnoteReference");
                    if let Some(id) = child.attr("id") {
                        self.emit_note_reference(state, endnote, id.trim());
                    }
                }
                "commentReference" => {
                    if !state.nested {
                        if let Some(id) = child.attr("id") {
                            let block_id = state.block_id();
                            self.comment_ranges
                                .entry(id.trim().to_string())
                                .or_default()
                                .block_id
                                .get_or_insert(block_id);
                        }
                    }
                }
                "fldChar" => {
                    self.flush_run_text(state, &mut text, &marks, &mut run_inline_ids);
                    self.handle_field_char(child, state);
                }
                "instrText" => {
                    if let Some(field) = state.fields.last_mut() {
                        if !field.in_result {
                            field.instruction.push_str(&child.text());
                        }
                    }
                }
                "drawing" | "pict" | "object" | "AlternateContent" => {
                    self.flush_run_text(state, &mut text, &marks, &mut run_inline_ids);
                    self.handle_image(child, state);
                }
                _ => {}
            }
        }
        self.flush_run_text(state, &mut text, &marks, &mut run_inline_ids);

        if let Some(added_marks) = format_change {
            match (run_inline_ids.first(), run_inline_ids.last()) {
                (Some(start), Some(end)) if !added_marks.is_empty() && !state.nested => {
                    let revision = run
                        .child("rPr")
                        .and_then(|rpr| rpr.child("rPrChange"))
                        .map(|change| Revision::from_element("rPrChange", change));
                    let revision = revision.unwrap_or_else(|| Revision {
                        kind: "rPrChange",
                        id: "?".to_string(),
                        author: "Unknown".to_string(),
                        date: None,
                    });
                    self.suggestions.push(Suggestion {
                        id: StableId::new("suggestion"),
                        author: revision.author.clone(),
                        kind: SuggestionKind::Format {
                            range: TextRange {
                                start: start.clone(),
                                end: end.clone(),
                            },
                            marks: added_marks,
                        },
                        state: SuggestionState::Proposed,
                        provenance: revision.provenance(),
                    });
                }
                _ => self.count(DROPPED_FORMAT_CHANGE),
            }
        }
    }

    fn flush_run_text(
        &mut self,
        state: &mut ParagraphState,
        text: &mut String,
        marks: &[Mark],
        run_inline_ids: &mut Vec<StableId>,
    ) {
        if text.is_empty() {
            return;
        }
        let content = std::mem::take(text);
        let inline = match state.links.last() {
            Some(href) => Inline::Link {
                id: StableId::new("link"),
                text: content,
                href: href.clone(),
                marks: marks.to_vec(),
            },
            None => Inline::Text {
                id: StableId::new("text"),
                text: content,
                marks: marks.to_vec(),
            },
        };
        run_inline_ids.push(inline_id(&inline).clone());
        self.emit(state, inline);
    }

    fn handle_field_char(&mut self, field_char: &XmlElement, state: &mut ParagraphState) {
        match field_char.attr("fldCharType").map(str::trim) {
            Some("begin") => state.fields.push(FieldState {
                instruction: String::new(),
                in_result: false,
                pushed_link: false,
                suppressed_result: false,
            }),
            Some("separate") => {
                let mut page_number = None;
                if let Some(field) = state.fields.last_mut() {
                    field.in_result = true;
                    if let Some(href) = hyperlink_field_target(&field.instruction) {
                        field.pushed_link = true;
                        state.links.push(href);
                    }
                    if let Some(which) = page_number_field(&field.instruction) {
                        field.suppressed_result = true;
                        page_number = Some(which);
                    }
                }
                if let Some(field) = page_number {
                    self.emit(
                        state,
                        Inline::PageNumber {
                            id: StableId::new("page-number"),
                            field,
                        },
                    );
                }
            }
            Some("end") => {
                if let Some(field) = state.fields.pop() {
                    if field.pushed_link {
                        state.links.pop();
                    }
                }
            }
            _ => {}
        }
    }

    fn emit_note_reference(&mut self, state: &mut ParagraphState, endnote: bool, id: &str) {
        if state.nested {
            return;
        }
        match self.note_ids.get(&(endnote, id.to_string())).cloned() {
            Some(footnote_id) => {
                let inline = Inline::FootnoteRef {
                    id: StableId::new("footnote-ref"),
                    footnote_id,
                };
                self.emit(state, inline);
            }
            None => self.count(MISSING_FOOTNOTE),
        }
    }

    fn handle_image(&mut self, element: &XmlElement, state: &mut ParagraphState) {
        if element.has_descendant("txbxContent") {
            self.count(DROPPED_TEXT_BOX);
            return;
        }
        let rel_id = element.descendants().into_iter().find_map(|node| {
            if node.is("blip") {
                node.attr_prefixed("r", "embed")
                    .or_else(|| node.attr_prefixed("r", "link"))
            } else if node.is("imagedata") {
                node.attr_prefixed("r", "id")
            } else {
                None
            }
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
        });
        let Some(rel_id) = rel_id else {
            self.count(DROPPED_DRAWING);
            return;
        };
        if state.nested || !state.inserts.is_empty() {
            self.count(DROPPED_NESTED_IMAGE);
            return;
        }
        let alt = element.find_descendant("docPr").and_then(|doc_pr| {
            ["descr", "title", "name"]
                .iter()
                .find_map(|attr| doc_pr.attr(attr))
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        });
        self.emit_segment(state, Segment::Image { rel_id, alt });
    }

    fn emit(&mut self, state: &mut ParagraphState, inline: Inline) {
        if let Some(capture) = state.inserts.last_mut() {
            capture.content.push(inline);
            return;
        }
        let id = inline_id(&inline).clone();
        if !state.nested {
            let block_id = state.block_id();
            for docx_id in &self.open_comment_ranges {
                let range = self.comment_ranges.entry(docx_id.clone()).or_default();
                if range.start.is_none() {
                    range.start = Some(id.clone());
                }
                range.end = Some(id.clone());
                if range.block_id.is_none() {
                    range.block_id = Some(block_id.clone());
                }
            }
            for capture in state.deletes.iter_mut() {
                if capture.start.is_none() {
                    capture.start = Some(id.clone());
                }
                capture.end = Some(id.clone());
            }
        }
        state.last_inline_id = Some(id);
        state.segments.push(Segment::Inline(inline));
    }

    fn emit_segment(&mut self, state: &mut ParagraphState, segment: Segment) {
        match segment {
            Segment::Inline(inline) => self.emit(state, inline),
            Segment::MathPara(inline) => {
                if state.inserts.last().is_some() {
                    self.emit(state, inline);
                    return;
                }
                // Register the equation like any inline so ranges can cover it,
                // but keep the display-math flag for standalone detection.
                self.emit(state, inline);
                if let Some(Segment::Inline(inline)) = state.segments.pop() {
                    state.segments.push(Segment::MathPara(inline));
                }
            }
            Segment::PageBreak | Segment::Image { .. } => {
                state.segments.push(segment);
                state.start_fragment();
            }
        }
    }
}
