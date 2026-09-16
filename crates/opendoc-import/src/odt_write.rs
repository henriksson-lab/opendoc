//! ODT (OpenDocument Text) writer.
//!
//! There is no ODF *reader* in this crate, so unlike [`crate::docx_write`]
//! this writer cannot be checked by round-tripping through its own mirror.
//! What replaces that check is stated where it is done — `odt_write_tests.rs`
//! asserts the produced XML directly, and the export was validated against
//! the OpenDocument 1.3 RelaxNG schema and read back by LibreOffice (the
//! reference ODF implementation) and by pandoc. LibreOffice's own conversion
//! of the result to `.docx`, re-imported through this crate's DOCX reader,
//! is the closest thing to a round trip available.
//!
//! Three rules govern the mapping.
//!
//! * **Lengths are written in points, and the conversion is exact.** The
//!   model measures in twips and 20 twips *is* one point, so every length
//!   lands on the 0.05pt grid with no rounding and no assumed DPI — the same
//!   argument `opendoc-render` makes for projecting twips as `pt` rather than
//!   `px`. Centimetres would not be exact (1cm is 566.929… twips) and inches
//!   would not either (1 twip is 0.0006944… in), so neither is used
//!   anywhere. Line spacing is a percentage, which is exact for the same
//!   reason: the model counts thousandths and a percentage carries one
//!   decimal.
//! * **Nothing is dropped silently.** Anything the model can express and
//!   OpenDocument cannot (or can only approximate) produces a
//!   [`ModelWarning`] naming it, as every other adapter here does.
//! * **Where ODF is richer than WordprocessingML, the richer mapping is
//!   used.** Merged cells become a span plus real `table:covered-table-cell`
//!   elements, so the grid stays rectangular exactly as the model's is *and
//!   the covered cells keep their content*, which `docx_write` cannot do —
//!   a `w:gridSpan` leaves no element to hold it. Column widths, cell
//!   backgrounds, borders, padding and vertical alignment all have direct
//!   equivalents in both; alignment and the three line-spacing rules map
//!   one-to-one; and a page number is a field, not a field with a frozen
//!   cached result.

use std::collections::{BTreeMap, BTreeSet};

use opendoc_core::{
    table_covered_positions, Alignment, Block, BlockKind, BlockProperties, BorderStyle, Document,
    Footnote, HeaderFooterSlot, ImageLayout, ImagePlacement, Inline, Length, LineSpacing, ListKind,
    Mark, MarkKind, ModelWarning, PageNumberField, PageOrientation, StableId, TableCellProperties,
    TableColumn, TableRow, TextDirection, VerticalAlignment,
};

use crate::odt_package::{
    border_value, hex, hex_value, image_extension, image_pixels, line_height_percent, meta_part,
    picture_media_type, pt, scale_axis, twips_to_pt, write_font_faces, write_named_styles,
    zip_odf_parts, MONO, NAMESPACES, NS_MANIFEST, ODF_VERSION, ODT_MEDIA_TYPE,
};
use crate::xml_write::{is_writable_xml_char, Xml};
use crate::{ExportImage, ImportError};

pub(crate) struct OdtExport {
    pub(crate) bytes: Vec<u8>,
    pub(crate) warnings: Vec<ModelWarning>,
}

// ---------------------------------------------------------------------------
// Warning codes
// ---------------------------------------------------------------------------

const CHECKLIST_AS_BULLET: &str = "odt-export-checklist-as-bullet";
const SPLIT_MIXED_LIST: &str = "odt-export-split-mixed-list";
const DROPPED_BLOCK_PROPERTIES: &str = "odt-export-dropped-block-properties";
const CODE_AS_MONOSPACE: &str = "odt-export-code-mark-as-monospace";
const DROPPED_MARK: &str = "odt-export-dropped-mark";
const DROPPED_MARK_VALUE: &str = "odt-export-dropped-mark-value";
const CITATION_AS_TEXT: &str = "odt-export-citation-as-text";
const MENTION_AS_TEXT: &str = "odt-export-mention-as-text";
const DROPDOWN_AS_TEXT: &str = "odt-export-dropdown-as-text";
const EQUATION_AS_SOURCE: &str = "odt-export-equation-as-source";
const DROPPED_EQUATION_CONTENT: &str = "odt-export-dropped-equation-content";
const MISSING_IMAGE_BLOB: &str = "odt-export-missing-image-blob";
const UNSUPPORTED_IMAGE_MEDIA_TYPE: &str = "odt-export-unsupported-image-media-type";
const UNKNOWN_IMAGE_SIZE: &str = "odt-export-unknown-image-size";
const DROPPED_CONTROL_CHARACTER: &str = "odt-export-dropped-control-character";
const DROPPED_COMMENTS: &str = "odt-export-dropped-comments";
const DROPPED_SUGGESTIONS: &str = "odt-export-dropped-suggestions";
const DROPPED_CITATION_DATABASE: &str = "odt-export-dropped-citation-database";
const DROPPED_DOI: &str = "odt-export-dropped-doi";
const DROPPED_FIRST_PAGE_FURNITURE: &str = "odt-export-dropped-first-page-furniture";
const DROPPED_FOOTNOTE_STATE: &str = "odt-export-dropped-footnote-state";
const UNPLACED_BOOKMARKS: &str = "odt-export-unplaced-bookmarks";
const NESTED_FOOTNOTE_REFERENCE: &str = "odt-export-nested-footnote-reference";
const MISSING_FOOTNOTE: &str = "odt-export-missing-footnote";
const CLAMPED_HEADING_LEVEL: &str = "odt-export-clamped-heading-level";
const PAGE_NUMBER_PLACEHOLDER: &str = "odt-export-page-number-placeholder";
const TRAILING_PAGE_BREAK: &str = "odt-export-trailing-page-break";
const FURNITURE_MARGIN_CLAMPED: &str = "odt-export-furniture-margin-clamped";
const NON_LEADING_TABLE_HEADER: &str = "odt-export-non-leading-table-header";
const POSITIONED_IMAGE_AS_INLINE: &str = "odt-export-positioned-image-as-inline";
const IMAGE_CROP_UNREPRESENTABLE: &str = "odt-export-image-crop-unrepresentable";
const IMAGE_CAPTION_AS_PARAGRAPH: &str = "odt-export-image-caption-as-paragraph";
const IMAGE_EFFECTS_UNREPRESENTABLE: &str = "odt-export-image-effects-unrepresentable";
const DROPPED_TABLE_ROW_HEADER: &str = "odt-export-dropped-table-row-header";

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub(crate) fn export_odt_bytes(
    document: &Document,
    images: &BTreeMap<String, ExportImage>,
) -> Result<OdtExport, ImportError> {
    document
        .validate()
        .map_err(|err| ImportError::InvalidDocument(err.to_string()))?;
    let mut exporter = Exporter::new(images);
    let parts = exporter.run(document);
    let mut warnings = std::mem::take(&mut exporter.warnings);
    dedupe(&mut warnings);
    Ok(OdtExport {
        bytes: zip_odf_parts(&parts)?,
        warnings,
    })
}

fn dedupe(warnings: &mut Vec<ModelWarning>) {
    let mut seen = BTreeSet::new();
    warnings.retain(|warning| seen.insert((warning.code.clone(), warning.message.clone())));
}

// ---------------------------------------------------------------------------
// Automatic styles
// ---------------------------------------------------------------------------

/// An ODF automatic style: a name, a family, and the properties element that
/// makes it what it is.
struct AutoStyle {
    name: String,
    family: &'static str,
    parent: Option<String>,
    list_style: Option<String>,
    body: String,
}

/// The automatic styles of one part.
///
/// ODF keeps automatic styles in the file that references them — a paragraph
/// style used inside a header lives in `styles.xml`, not `content.xml` — so
/// there are two of these and the writer switches between them. Identical
/// property sets collapse onto one name, which is what keeps a 1,000-block
/// document from carrying 1,000 identical styles.
#[derive(Default)]
struct StyleTable {
    /// Distinguishes the two files' names, since ODF shares one namespace
    /// across them.
    prefix: &'static str,
    styles: Vec<AutoStyle>,
    index: BTreeMap<(&'static str, String), String>,
    lists: Vec<(String, String)>,
    list_index: BTreeMap<String, String>,
}

impl StyleTable {
    fn new(prefix: &'static str) -> Self {
        Self {
            prefix,
            ..Self::default()
        }
    }

    /// Returns the name of a style with these properties, creating it on
    /// first use.
    fn style(
        &mut self,
        kind: &'static str,
        family: &'static str,
        parent: Option<&str>,
        list_style: Option<&str>,
        body: String,
    ) -> String {
        let key = (
            kind,
            format!(
                "{}\u{1}{}\u{1}{body}",
                parent.unwrap_or_default(),
                list_style.unwrap_or_default()
            ),
        );
        if let Some(name) = self.index.get(&key) {
            return name.clone();
        }
        let ordinal = self
            .styles
            .iter()
            .filter(|style| style.name.starts_with(&format!("{}{kind}", self.prefix)))
            .count()
            + 1;
        let name = format!("{}{kind}{ordinal}", self.prefix);
        self.styles.push(AutoStyle {
            name: name.clone(),
            family,
            parent: parent.map(str::to_string),
            list_style: list_style.map(str::to_string),
            body,
        });
        self.index.insert(key, name.clone());
        name
    }

    /// Returns the name of a `text:list-style` with this level definition,
    /// creating it on first use.
    fn list_style(&mut self, levels: String) -> String {
        if let Some(name) = self.list_index.get(&levels) {
            return name.clone();
        }
        let name = format!("{}L{}", self.prefix, self.lists.len() + 1);
        self.lists.push((name.clone(), levels.clone()));
        self.list_index.insert(levels, name.clone());
        name
    }

    fn write(&self, xml: &mut Xml) {
        for (name, levels) in &self.lists {
            xml.open("text:list-style", &[("style:name", name)]);
            xml.raw(levels);
            xml.close("text:list-style");
        }
        for style in &self.styles {
            let mut attrs: Vec<(&str, &str)> = vec![
                ("style:name", style.name.as_str()),
                ("style:family", style.family),
            ];
            if let Some(parent) = &style.parent {
                attrs.push(("style:parent-style-name", parent.as_str()));
            }
            if let Some(list_style) = &style.list_style {
                attrs.push(("style:list-style-name", list_style.as_str()));
            }
            if style.body.is_empty() {
                xml.empty("style:style", &attrs);
                continue;
            }
            xml.open("style:style", &attrs);
            xml.raw(&style.body);
            xml.close("style:style");
        }
    }
}

/// Which file the automatic styles being created now belong to.
#[derive(Clone, Copy, Eq, PartialEq)]
enum StyleTarget {
    Content,
    Furniture,
}

// ---------------------------------------------------------------------------
// Exporter
// ---------------------------------------------------------------------------

/// The marker a list level draws. A checklist's tick state is part of the
/// marker because ODF puts the bullet character on the *level*, not the item.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum ListFlavour {
    Bullet(opendoc_core::BulletListMarker),
    Ordered,
    Unchecked,
    Checked,
}

impl ListFlavour {
    fn of(kind: ListKind) -> Self {
        match kind {
            ListKind::Bullet => ListFlavour::Bullet(opendoc_core::BulletListMarker::Disc),
            ListKind::Ordered => ListFlavour::Ordered,
            ListKind::Checklist { checked: false } => ListFlavour::Unchecked,
            ListKind::Checklist { checked: true } => ListFlavour::Checked,
        }
    }

    fn bullet_char(&self) -> Option<&str> {
        match self {
            ListFlavour::Bullet(marker) => Some(match marker {
                opendoc_core::BulletListMarker::Disc => "\u{2022}",
                opendoc_core::BulletListMarker::Circle => "\u{25e6}",
                opendoc_core::BulletListMarker::Square => "\u{25a0}",
                opendoc_core::BulletListMarker::Custom(glyph) => glyph,
            }),
            ListFlavour::Ordered => None,
            ListFlavour::Unchecked => Some("\u{2610}"),
            ListFlavour::Checked => Some("\u{2612}"),
        }
    }
}

struct Exporter<'a> {
    images: &'a BTreeMap<String, ExportImage>,
    warnings: Vec<ModelWarning>,
    content_styles: StyleTable,
    furniture_styles: StyleTable,
    target: StyleTarget,
    /// `Pictures/…` parts, in the order they were first referenced.
    pictures: Vec<(String, ExportImage)>,
    picture_paths: BTreeMap<String, String>,
    footnote_numbers: BTreeMap<StableId, usize>,
    bookmark_ranges: BTreeMap<StableId, Vec<(StableId, String)>>,
    emitted_bookmarks: BTreeSet<StableId>,
    /// A `BlockKind::PageBreak` has no ODF element of its own — a break is a
    /// property of the paragraph that starts the new page — so it is folded
    /// into the next block rather than becoming an empty paragraph.
    pending_page_break: bool,
    next_table: usize,
    next_frame: usize,
}

impl<'a> Exporter<'a> {
    fn new(images: &'a BTreeMap<String, ExportImage>) -> Self {
        Self {
            images,
            warnings: Vec::new(),
            content_styles: StyleTable::new(""),
            furniture_styles: StyleTable::new("M"),
            target: StyleTarget::Content,
            pictures: Vec::new(),
            picture_paths: BTreeMap::new(),
            footnote_numbers: BTreeMap::new(),
            bookmark_ranges: BTreeMap::new(),
            emitted_bookmarks: BTreeSet::new(),
            pending_page_break: false,
            next_table: 0,
            next_frame: 0,
        }
    }

    fn warn(&mut self, code: &'static str, message: impl Into<String>) {
        self.warnings.push(ModelWarning {
            code: code.to_string(),
            message: message.into(),
        });
    }

    fn styles(&mut self) -> &mut StyleTable {
        match self.target {
            StyleTarget::Content => &mut self.content_styles,
            StyleTarget::Furniture => &mut self.furniture_styles,
        }
    }

    fn run(&mut self, document: &Document) -> Vec<(String, Vec<u8>)> {
        self.report_unrepresentable_document_parts(document);
        self.install_bookmark_ranges(document);
        for (index, footnote) in document.footnotes.iter().enumerate() {
            self.footnote_numbers.insert(footnote.id.clone(), index + 1);
        }

        // The body and the furniture are written first: both discover
        // automatic styles and picture parts that the parts listing them must
        // already know about.
        let body = self.write_blocks_fragment(document, &document.blocks, true);
        self.target = StyleTarget::Furniture;
        let header = self.write_furniture(document, HeaderFooterSlot::Header);
        let footer = self.write_furniture(document, HeaderFooterSlot::Footer);
        self.report_unplaced_bookmarks(document);
        let page_layout = self.page_layout(document, header.is_some(), footer.is_some());
        self.target = StyleTarget::Content;

        // `mimetype` first: the ODF package's magic number sits at a fixed
        // offset, which only works if this entry leads and is stored.
        let mut parts: Vec<(String, Vec<u8>)> = vec![
            ("mimetype".to_string(), ODT_MEDIA_TYPE.as_bytes().to_vec()),
            ("content.xml".to_string(), self.content_part(&body)),
            (
                "styles.xml".to_string(),
                self.styles_part(&page_layout, header.as_deref(), footer.as_deref()),
            ),
            ("meta.xml".to_string(), meta_part(document)),
        ];
        for (path, image) in std::mem::take(&mut self.pictures) {
            parts.push((path, image.bytes));
        }
        let manifest = self.manifest_part(&parts);
        parts.insert(1, ("META-INF/manifest.xml".to_string(), manifest));
        parts
    }

    // -- document-level bookkeeping ---------------------------------------

    fn report_unrepresentable_document_parts(&mut self, document: &Document) {
        if document.first_page_header.is_some()
            || document.first_page_footer.is_some()
            || document.even_page_header.is_some()
            || document.even_page_footer.is_some()
        {
            self.warn(
                DROPPED_FIRST_PAGE_FURNITURE,
                "document-wide first- or even-page header/footer overrides were not written because this ODT exporter has one master page",
            );
        }
        if !document.comments.is_empty() {
            self.warn(
                DROPPED_COMMENTS,
                format!(
                    "{} OpenDoc comment thread(s) were dropped: the ODT writer does not produce office:annotation ranges",
                    document.comments.len()
                ),
            );
        }
        if !document.suggestions.is_empty() {
            self.warn(
                DROPPED_SUGGESTIONS,
                format!(
                    "{} OpenDoc suggestion(s) were dropped: the ODT writer does not produce tracked changes",
                    document.suggestions.len()
                ),
            );
        }
        if document.doi.is_some() {
            self.warn(
                DROPPED_DOI,
                "the document DOI has no OpenDocument equivalent and was dropped",
            );
        }
        if !document.citation_database.citations.is_empty()
            || !document.citation_database.references.is_empty()
        {
            self.warn(
                DROPPED_CITATION_DATABASE,
                "the citation database was dropped; citations are written as their rendered text",
            );
        }
        // Which CSL styles are bundled is a product decision, so a document
        // formatted by the fallback renderer says so here too rather than
        // only in the editor.
        for warning in opendoc_citations::citation_support_warnings(&document.citation_database) {
            self.warnings.push(warning);
        }
    }

    fn install_bookmark_ranges(&mut self, document: &Document) {
        let mut bookmarks: Vec<_> = document
            .bookmarks
            .iter()
            .filter(|bookmark| !bookmark.deleted)
            .collect();
        bookmarks.sort_by(|left, right| left.id.cmp(&right.id));
        for bookmark in bookmarks {
            self.bookmark_ranges
                .entry(bookmark.block_id.clone())
                .or_default()
                .push((bookmark.id.clone(), bookmark.name.clone()));
        }
    }

    fn report_unplaced_bookmarks(&mut self, document: &Document) {
        let unplaced = document
            .bookmarks
            .iter()
            .filter(|bookmark| !bookmark.deleted && !self.emitted_bookmarks.contains(&bookmark.id))
            .count();
        if unplaced > 0 {
            self.warn(UNPLACED_BOOKMARKS, format!(
                "{unplaced} OpenDoc bookmark(s) target a non-text block and could not be represented as OpenDocument inline bookmark ranges"
            ));
        }
    }

    // -- blocks ------------------------------------------------------------

    /// Writes a run of blocks, grouping adjacent list items into `text:list`
    /// elements. `top_level` marks the document body, where a trailing page
    /// break has nowhere to fold into.
    fn write_blocks_fragment(
        &mut self,
        document: &Document,
        blocks: &[Block],
        top_level: bool,
    ) -> String {
        let mut xml = Xml::fragment();
        let mut index = 0;
        while index < blocks.len() {
            match &blocks[index].kind {
                BlockKind::ListItem { list_id, .. } => {
                    let end = blocks[index..]
                        .iter()
                        .position(|block| !is_list_item_of(block, list_id))
                        .map(|offset| index + offset)
                        .unwrap_or(blocks.len());
                    let run: Vec<&Block> = blocks[index..end].iter().collect();
                    self.write_list_run(&mut xml, document, &run);
                    index = end;
                }
                _ => {
                    self.write_block(&mut xml, document, &blocks[index]);
                    index += 1;
                }
            }
        }
        if self.pending_page_break && top_level {
            // A page break as the last block of the document: there is no
            // following paragraph to carry `fo:break-before`, so it becomes
            // an empty paragraph that opens a page.
            self.pending_page_break = false;
            self.warn(
                TRAILING_PAGE_BREAK,
                "a page break with no content after it became an empty paragraph on a new page: in OpenDocument a break is a property of the paragraph that starts the page, so there has to be one",
            );
            let style = self.styles().style(
                "P",
                "paragraph",
                Some("Standard"),
                None,
                "<style:paragraph-properties fo:break-before=\"page\"/>".to_string(),
            );
            xml.empty("text:p", &[("text:style-name", &style)]);
        }
        xml.into_string()
    }

    fn write_block(&mut self, xml: &mut Xml, document: &Document, block: &Block) {
        match &block.kind {
            BlockKind::Paragraph => {
                let style = self.paragraph_style("Standard", &block.properties);
                xml.open("text:p", &[("text:style-name", &style)]);
                self.write_bookmark_starts(xml, &block.id);
                self.write_inlines(xml, document, &block.content);
                self.write_bookmark_ends(xml, &block.id);
                xml.close("text:p");
            }
            BlockKind::Title | BlockKind::Subtitle => {
                let parent = match &block.kind {
                    BlockKind::Title => "Title",
                    BlockKind::Subtitle => "Subtitle",
                    _ => unreachable!(),
                };
                let style = self.paragraph_style(parent, &block.properties);
                xml.open("text:p", &[("text:style-name", &style)]);
                self.write_bookmark_starts(xml, &block.id);
                self.write_inlines(xml, document, &block.content);
                self.write_bookmark_ends(xml, &block.id);
                xml.close("text:p");
            }
            BlockKind::Heading { level } => {
                // `Document::validate()` already rejects a level outside
                // 1..=6, so this is what makes the index into
                // `HEADING_SIZE_TWIPS` safe rather than a reachable
                // degradation; it warns anyway rather than being silent.
                let clamped = (*level).clamp(1, 6);
                if clamped != *level {
                    self.warn(
                        CLAMPED_HEADING_LEVEL,
                        format!("heading level {level} was clamped to {clamped}"),
                    );
                }
                let parent = format!("Heading_20_{clamped}");
                let style = self.paragraph_style(&parent, &block.properties);
                let level = clamped.to_string();
                xml.open(
                    "text:h",
                    &[("text:style-name", &style), ("text:outline-level", &level)],
                );
                self.write_bookmark_starts(xml, &block.id);
                self.write_inlines(xml, document, &block.content);
                self.write_bookmark_ends(xml, &block.id);
                xml.close("text:h");
            }
            BlockKind::ListItem { .. } => {
                // Reached only for a stray item outside a run (a table cell
                // holding a single list item, for instance).
                let run = [block];
                self.write_list_run(xml, document, &run);
            }
            BlockKind::Table {
                columns,
                properties,
                rows,
            } => {
                self.write_table(xml, document, block, columns, properties, rows);
            }
            BlockKind::EquationBlock { equation } => {
                if !block.content.is_empty() {
                    self.warn(
                        DROPPED_EQUATION_CONTENT,
                        "inline content alongside a display equation was dropped",
                    );
                }
                self.warn(
                    EQUATION_AS_SOURCE,
                    "equations are written as their LaTeX source text; an OpenDocument formula is an embedded MathML sub-document, which this writer does not produce, and ADR 0003 makes the source canonical anyway",
                );
                let style = self.paragraph_style("Standard", &block.properties);
                xml.open("text:p", &[("text:style-name", &style)]);
                let source = self.sanitize(&equation.source, "equation source");
                self.write_span(xml, &source, &[]);
                xml.close("text:p");
            }
            BlockKind::Image {
                blob_hash,
                alt_text,
                layout,
            } => {
                self.write_image_paragraph(xml, block, blob_hash, alt_text, layout);
            }
            BlockKind::HorizontalRule => {
                let style = self.horizontal_rule_style();
                xml.empty("text:p", &[("text:style-name", &style)]);
            }
            BlockKind::TableOfContents { max_level } => {
                self.write_table_of_contents(xml, block, *max_level);
            }
            BlockKind::Bibliography => {
                self.warn(
                    "odt-export-bibliography-as-static-text",
                    "the generated bibliography was exported as current formatted paragraphs because this ODT writer does not emit bibliography source records",
                );
                let style = self.paragraph_style("Standard", &block.properties);
                for text in std::iter::once("Bibliography".to_string()).chain(
                    opendoc_citations::render_cited_bibliography(&document.citation_database)
                        .into_iter()
                        .map(|entry| entry.text),
                ) {
                    xml.open("text:p", &[("text:style-name", &style)]);
                    self.write_span(xml, &text, &[]);
                    xml.close("text:p");
                }
            }
            BlockKind::PageBreak => {
                if !block.properties.is_empty() {
                    self.reject_block_properties(block, "page break");
                }
                self.pending_page_break = true;
            }
        }
    }

    /// A block kind whose ODF shape has nowhere to put paragraph formatting.
    fn reject_block_properties(&mut self, block: &Block, what: &str) {
        let keys: Vec<&str> = block
            .properties
            .iter()
            .map(|property| property.key().as_str())
            .collect();
        self.warn(
            DROPPED_BLOCK_PROPERTIES,
            format!(
                "block formatting ({}) on a {what} block was dropped: OpenDocument has no paragraph to carry it",
                keys.join(", ")
            ),
        );
    }

    /// An ODF TOC is an index declaration, not a copied list of paragraphs.
    /// The suite therefore contains no cached entries: a compliant reader
    /// generates them from the document's headings when its index is updated.
    fn write_table_of_contents(&mut self, xml: &mut Xml, block: &Block, max_level: u8) {
        let name = format!("OpenDocTOC_{}", block.id);
        let max_level_text = max_level.to_string();
        let title_style = self.paragraph_style("Standard", &BlockProperties::default());
        xml.open(
            "text:table-of-content",
            &[("text:name", &name), ("text:protected", "true")],
        );
        xml.open(
            "text:table-of-content-source",
            &[("text:outline-level", &max_level_text)],
        );
        xml.open(
            "text:index-title-template",
            &[("text:style-name", &title_style)],
        );
        self.write_span(xml, "Table of contents", &[]);
        xml.close("text:index-title-template");
        for level in 1..=max_level {
            let level = level.to_string();
            xml.open(
                "text:table-of-content-entry-template",
                &[
                    ("text:outline-level", &level),
                    ("text:style-name", &title_style),
                ],
            );
            xml.empty("text:index-entry-text", &[]);
            xml.empty(
                "text:index-entry-tab-stop",
                &[("style:type", "right"), ("style:leader-char", ".")],
            );
            xml.empty("text:index-entry-page-number", &[]);
            xml.close("text:table-of-content-entry-template");
        }
        xml.close("text:table-of-content-source");
        // ODF requires an index body even before an office suite has updated
        // it. Carry only its title, never fabricated heading/page entries.
        xml.open("text:index-body", &[]);
        xml.open("text:index-title", &[("text:name", &name)]);
        xml.open("text:p", &[("text:style-name", &title_style)]);
        self.write_span(xml, "Table of contents", &[]);
        xml.close("text:p");
        xml.close("text:index-title");
        xml.close("text:index-body");
        xml.close("text:table-of-content");
    }

    // -- paragraph styles --------------------------------------------------

    /// The automatic paragraph style for a block's properties, taking any
    /// pending page break with it.
    fn paragraph_style(&mut self, parent: &str, properties: &BlockProperties) -> String {
        self.paragraph_style_with(parent, properties, None)
    }

    fn horizontal_rule_style(&mut self) -> String {
        let mut attrs = Vec::new();
        if self.pending_page_break {
            self.pending_page_break = false;
            attrs.push("fo:break-before=\"page\"");
        }
        attrs.push("fo:border-bottom=\"0.75pt solid #6B7280\"");
        let body = format!("<style:paragraph-properties {}/>", attrs.join(" "));
        self.styles()
            .style("P", "paragraph", Some("Standard"), None, body)
    }

    fn paragraph_style_with(
        &mut self,
        parent: &str,
        properties: &BlockProperties,
        extra: Option<&str>,
    ) -> String {
        let mut attrs = paragraph_property_attributes(properties);
        if self.pending_page_break {
            self.pending_page_break = false;
            attrs.push(("fo:break-before", "page".to_string()));
        }
        if let Some(extra) = extra {
            attrs.push(("style:list-style-name", extra.to_string()));
        }
        let body = if attrs.is_empty() {
            String::new()
        } else {
            let mut inner = Xml::fragment();
            let borrowed: Vec<(&str, &str)> = attrs
                .iter()
                .map(|(name, value)| (*name, value.as_str()))
                .collect();
            inner.empty("style:paragraph-properties", &borrowed);
            inner.into_string()
        };
        if body.is_empty() {
            // Nothing to say: reference the named style directly rather than
            // minting an automatic style that only points at it.
            return parent.to_string();
        }
        self.styles()
            .style("P", "paragraph", Some(parent), None, body)
    }

    // -- lists -------------------------------------------------------------

    /// One list run, split where OpenDocument cannot keep it whole.
    ///
    /// ODF puts a level's marker on the *list style*, so two items at one
    /// level cannot draw different markers — and a checklist's ticked and
    /// unticked items are exactly that. The run is therefore cut into
    /// sub-runs whose marker per level is consistent, each its own
    /// `text:list`, with numbering continued across the cut.
    fn write_list_run(&mut self, xml: &mut Xml, document: &Document, run: &[&Block]) {
        let mut start = 0;
        let mut first = true;
        while start < run.len() {
            let mut flavours: BTreeMap<u8, ListFlavour> = BTreeMap::new();
            let mut end = start;
            while end < run.len() {
                let Some((level, kind)) = list_item_marker(run[end]) else {
                    break;
                };
                let flavour = ListFlavour::of(kind);
                match flavours.get(&level) {
                    Some(existing) if *existing != flavour => break,
                    _ => {
                        flavours.insert(level, flavour);
                        end += 1;
                    }
                }
            }
            if end == start {
                end += 1;
            }
            if !first {
                self.warn(
                    SPLIT_MIXED_LIST,
                    "a single OpenDoc list run draws more than one marker at one level; OpenDocument puts a marker on the list level rather than on the item, so the run was split into several lists with their numbering continued",
                );
            }
            let list_id = run[start]
                .list_id()
                .expect("list runs contain only list items");
            let levels = list_level_definitions(&flavours, document.list_properties.get(list_id));
            let style = self.styles().list_style(levels);
            if flavours
                .values()
                .any(|flavour| matches!(flavour, ListFlavour::Checked | ListFlavour::Unchecked))
            {
                self.warn(
                    CHECKLIST_AS_BULLET,
                    "checklist items were written as a bulleted list whose bullet is a ballot-box glyph; OpenDocument has no checklist, so the tick state is visible but not structural",
                );
            }
            // The outermost list is always level 0, whatever depth the run
            // starts at: a deeper first item becomes a list item holding
            // nothing but a nested list, which is how ODF says "indented".
            self.write_list(xml, document, &run[start..end], 0, Some(&style), !first);
            first = false;
            start = end;
        }
    }

    /// Emits one `text:list`, recursing for deeper levels. A deeper item is a
    /// list inside the list item above it, which is how ODF nests.
    fn write_list(
        &mut self,
        xml: &mut Xml,
        document: &Document,
        items: &[&Block],
        level: u8,
        style: Option<&str>,
        continue_numbering: bool,
    ) {
        let mut attrs: Vec<(&str, &str)> = Vec::new();
        if let Some(style) = style {
            attrs.push(("text:style-name", style));
        }
        if continue_numbering {
            attrs.push(("text:continue-numbering", "true"));
        }
        xml.open("text:list", &attrs);
        let mut index = 0;
        while index < items.len() {
            let item_level = list_item_marker(items[index])
                .map(|(level, _)| level)
                .unwrap_or(0);
            let deeper_end = |from: usize| {
                items[from..]
                    .iter()
                    .position(|block| {
                        list_item_marker(block)
                            .map(|(candidate, _)| candidate <= level)
                            .unwrap_or(true)
                    })
                    .map(|offset| from + offset)
                    .unwrap_or(items.len())
            };
            if item_level > level {
                // A run that starts deeper than its parent: ODF allows a
                // list item that holds only a nested list.
                let end = deeper_end(index);
                xml.open("text:list-item", &[]);
                self.write_list(xml, document, &items[index..end], level + 1, None, false);
                xml.close("text:list-item");
                index = end;
                continue;
            }
            xml.open("text:list-item", &[]);
            self.write_list_item_paragraph(xml, document, items[index]);
            index += 1;
            let end = deeper_end(index);
            if end > index {
                self.write_list(xml, document, &items[index..end], level + 1, None, false);
                index = end;
            }
            xml.close("text:list-item");
        }
        xml.close("text:list");
    }

    fn write_list_item_paragraph(&mut self, xml: &mut Xml, document: &Document, block: &Block) {
        let style = self.paragraph_style("Standard", &block.properties);
        xml.open("text:p", &[("text:style-name", &style)]);
        self.write_bookmark_starts(xml, &block.id);
        self.write_inlines(xml, document, &block.content);
        self.write_bookmark_ends(xml, &block.id);
        xml.close("text:p");
    }

    /// OpenDoc bookmarks identify whole stable blocks.  ODF bookmarks are
    /// inline ranges, so a zero-width pair at the paragraph start preserves
    /// the named target without fabricating a character offset.
    fn write_bookmark_starts(&mut self, xml: &mut Xml, block_id: &StableId) {
        let Some(ranges) = self.bookmark_ranges.get(block_id).cloned() else {
            return;
        };
        for (bookmark_id, name) in ranges {
            self.emitted_bookmarks.insert(bookmark_id);
            xml.empty("text:bookmark-start", &[("text:name", &name)]);
        }
    }

    fn write_bookmark_ends(&mut self, xml: &mut Xml, block_id: &StableId) {
        let Some(ranges) = self.bookmark_ranges.get(block_id) else {
            return;
        };
        for (_, name) in ranges {
            xml.empty("text:bookmark-end", &[("text:name", name)]);
        }
    }

    // -- tables ------------------------------------------------------------

    fn write_table(
        &mut self,
        xml: &mut Xml,
        document: &Document,
        block: &Block,
        columns: &[TableColumn],
        properties: &opendoc_core::TableProperties,
        rows: &[TableRow],
    ) {
        if rows
            .iter()
            .flat_map(|row| &row.cells)
            .any(|cell| cell.properties.row_header.is_some())
        {
            self.warn(
                DROPPED_TABLE_ROW_HEADER,
                "OpenDocument Text's table-header-columns is table-wide; explicit per-cell row-header state was not exported",
            );
        }
        if !block.properties.is_empty() {
            self.reject_block_properties(block, "table");
        }
        self.next_table += 1;
        let name = format!("Table{}", self.next_table);
        let all_columns_sized = columns.iter().all(|column| column.width.is_some());
        let alignment = match properties
            .alignment
            .unwrap_or(opendoc_core::TableAlignment::Start)
        {
            opendoc_core::TableAlignment::Start => "left",
            opendoc_core::TableAlignment::Center => "center",
            opendoc_core::TableAlignment::End => "right",
        };
        let mut table_properties = if all_columns_sized {
            format!("<style:table-properties table:align=\"{alignment}\"")
        } else {
            String::from("<style:table-properties style:rel-width=\"100%\" table:align=\"margins\"")
        };
        if self.pending_page_break {
            self.pending_page_break = false;
            table_properties.push_str(" fo:break-before=\"page\"");
        }
        if let Some(border) = properties.border {
            table_properties.push_str(" fo:border=\"");
            table_properties.push_str(&border_value(border));
            table_properties.push('\"');
        }
        table_properties.push_str("/>");
        let table_style = self
            .styles()
            .style("Ta", "table", None, None, table_properties);
        xml.open(
            "table:table",
            &[("table:name", &name), ("table:style-name", &table_style)],
        );
        for column in columns {
            let style = self.column_style(column);
            xml.empty("table:table-column", &[("table:style-name", &style)]);
        }
        let covered = table_covered_positions(rows);
        let header_rows = rows.iter().take_while(|row| row.header).count();
        if rows.iter().skip(header_rows).any(|row| row.header) {
            self.warn(
                NON_LEADING_TABLE_HEADER,
                "OpenDocument can repeat only leading table header rows; later header rows were written as ordinary rows",
            );
        }
        for (row_index, row) in rows.iter().enumerate() {
            if row_index == 0 && header_rows > 0 {
                xml.open("table:table-header-rows", &[]);
            }
            xml.open("table:table-row", &[]);
            for (column_index, cell) in row.cells.iter().enumerate() {
                if covered.contains(&(row_index, column_index)) {
                    // The cell is still in the model's grid and still holds
                    // its content; ODF says the same thing with a covered
                    // cell, so merging stays exactly invertible.
                    xml.empty("table:covered-table-cell", &[]);
                    continue;
                }
                let style = self.cell_style(&cell.properties);
                let rows_spanned = cell.span.rows().to_string();
                let columns_spanned = cell.span.columns().to_string();
                let mut attrs: Vec<(&str, &str)> = vec![
                    ("table:style-name", style.as_str()),
                    ("office:value-type", "string"),
                ];
                if !cell.span.is_single() {
                    attrs.push(("table:number-columns-spanned", &columns_spanned));
                    attrs.push(("table:number-rows-spanned", &rows_spanned));
                }
                xml.open("table:table-cell", &attrs);
                let body = self.write_blocks_fragment(document, &cell.blocks, false);
                xml.raw(&body);
                xml.close("table:table-cell");
            }
            xml.close("table:table-row");
            if row_index + 1 == header_rows {
                xml.close("table:table-header-rows");
            }
        }
        xml.close("table:table");
    }

    fn column_style(&mut self, column: &TableColumn) -> String {
        let body = match column.width {
            // `None` is the model's *auto*, and ODF has a word for it rather
            // than a width the writer would have to invent.
            None => "<style:table-column-properties style:use-optimal-column-width=\"true\"/>"
                .to_string(),
            Some(width) => format!(
                "<style:table-column-properties style:column-width=\"{}\"/>",
                pt(width)
            ),
        };
        self.styles().style("co", "table-column", None, None, body)
    }

    fn cell_style(&mut self, properties: &TableCellProperties) -> String {
        let mut attrs: Vec<(&'static str, String)> = Vec::new();
        if let Some(background) = properties.background {
            attrs.push(("fo:background-color", hex(background)));
        }
        // `fo:border-*` is physical and the model's start/end are logical. A
        // cell carries no direction of its own, so the leading edge is the
        // left one — the same reading `docx_write` gives `w:tcPr`.
        //
        // An edge the model does not state is written as nothing, not as a
        // default hairline. ODF reads an absent `fo:border-*` as *no border*,
        // which is what the model's absent edge means, so writing a line here
        // would invent one — the same defect `docx_write` used to have with
        // its unconditional `w:tblBorders`, and the reason a LibreOffice
        // table written with `fo:border="none"` came back `0.5pt solid
        // #000000`. The editor's grey gridlines are a view default, not a
        // property of the document, and an export cannot carry them.
        for (name, border) in [
            ("fo:border-top", properties.border_top),
            ("fo:border-bottom", properties.border_bottom),
            ("fo:border-left", properties.border_start),
            ("fo:border-right", properties.border_end),
        ] {
            if let Some(border) = border {
                attrs.push((name, border_value(border)));
            }
        }
        if let Some(alignment) = properties.vertical_alignment {
            attrs.push((
                "style:vertical-align",
                match alignment {
                    VerticalAlignment::Top => "top",
                    VerticalAlignment::Middle => "middle",
                    VerticalAlignment::Bottom => "bottom",
                }
                .to_string(),
            ));
        }
        for (name, padding) in [
            ("fo:padding-top", properties.padding_top),
            ("fo:padding-bottom", properties.padding_bottom),
            ("fo:padding-left", properties.padding_start),
            ("fo:padding-right", properties.padding_end),
        ] {
            if let Some(padding) = padding {
                attrs.push((name, pt(padding)));
            }
        }
        let mut inner = Xml::fragment();
        let borrowed: Vec<(&str, &str)> = attrs
            .iter()
            .map(|(name, value)| (*name, value.as_str()))
            .collect();
        inner.empty("style:table-cell-properties", &borrowed);
        self.styles()
            .style("ce", "table-cell", None, None, inner.into_string())
    }

    // -- images ------------------------------------------------------------

    fn write_image_paragraph(
        &mut self,
        xml: &mut Xml,
        block: &Block,
        blob_hash: &str,
        alt_text: &str,
        layout: &ImageLayout,
    ) {
        // ADR 0022's positioned tuple is not an ODT mapping yet.  The
        // fallback frame below is intentionally in-flow, and must say so.
        if layout.positioned.is_some() {
            self.warn(
                POSITIONED_IMAGE_AS_INLINE,
                "a positioned OpenDoc image was exported as an in-flow ODT image; its anchor, offsets and layer are not mapped",
            );
        }
        let alt = self.sanitize(alt_text, "image alt text");
        // Unlike WordprocessingML's drawing, an ODF frame sits inside an
        // ordinary paragraph, so an image block's own paragraph formatting
        // has somewhere to go and is not dropped.
        let style = self.paragraph_style("Standard", &block.properties);
        let Some(image) = self.images.get(blob_hash).cloned() else {
            self.warn(
                MISSING_IMAGE_BLOB,
                format!(
                    "image blob {blob_hash} was not supplied, so the image was written as its alt text"
                ),
            );
            xml.open("text:p", &[("text:style-name", &style)]);
            self.write_span(xml, &alt, &[]);
            xml.close("text:p");
            self.write_image_effects_fallback(layout);
            self.write_image_crop_and_caption_fallback(xml, layout);
            return;
        };
        let Some(extension) = image_extension(&image.media_type, &image.bytes) else {
            self.warn(
                UNSUPPORTED_IMAGE_MEDIA_TYPE,
                format!(
                    "image blob {blob_hash} has media type {} which OpenDocument has no picture part for; it was written as its alt text",
                    image.media_type
                ),
            );
            xml.open("text:p", &[("text:style-name", &style)]);
            self.write_span(xml, &alt, &[]);
            xml.close("text:p");
            self.write_image_effects_fallback(layout);
            self.write_image_crop_and_caption_fallback(xml, layout);
            return;
        };
        let (width, height, known) = self.frame_extent(blob_hash, layout, &image.bytes);
        if !known {
            self.warn(
                UNKNOWN_IMAGE_SIZE,
                format!(
                    "image blob {blob_hash} states no display size and has no readable pixel size, so a default was written"
                ),
            );
        }
        let href = match self.picture_paths.get(blob_hash) {
            Some(href) => href.clone(),
            None => {
                let href = format!("Pictures/image{}.{extension}", self.pictures.len() + 1);
                self.pictures.push((href.clone(), image));
                self.picture_paths
                    .insert(blob_hash.to_string(), href.clone());
                href
            }
        };
        let placement = layout.effective_placement();
        let frame_style = self.frame_style(placement, layout);
        self.next_frame += 1;
        let frame_name = format!("Image{}", self.next_frame);
        let anchor = match placement {
            ImagePlacement::Block => "as-char",
            ImagePlacement::WrapStart | ImagePlacement::WrapEnd => "paragraph",
        };
        xml.open("text:p", &[("text:style-name", &style)]);
        let rotation = layout
            .rotation_degrees
            .filter(|rotation| *rotation != 0)
            .map(|rotation| format!("rotate ({rotation})"));
        let mut frame_attributes = vec![
            ("draw:style-name", frame_style.as_str()),
            ("draw:name", frame_name.as_str()),
            ("text:anchor-type", anchor),
            ("svg:width", width.as_str()),
            ("svg:height", height.as_str()),
        ];
        if let Some(rotation) = rotation.as_deref() {
            // `draw:transform` is the ODF frame transform grammar.  The
            // model's whole-degree clockwise rotation maps without a unit or
            // a rounding conversion.
            frame_attributes.push(("draw:transform", rotation));
        }
        xml.open("draw:frame", &frame_attributes);
        xml.empty(
            "draw:image",
            &[
                ("xlink:href", &href),
                ("xlink:type", "simple"),
                ("xlink:show", "embed"),
                ("xlink:actuate", "onLoad"),
            ],
        );
        if !alt.trim().is_empty() {
            xml.text_element("svg:title", &[], &alt);
            xml.text_element("svg:desc", &[], &alt);
        }
        xml.close("draw:frame");
        xml.close("text:p");

        self.write_image_crop_and_caption_fallback(xml, layout);
    }

    /// Preserve the part of an image's image-level presentation that can be
    /// made visible after an ODT fallback.  This deliberately runs after all
    /// three image paths (embedded, missing asset and unsupported media), so
    /// an early asset fallback cannot silently swallow model data.
    fn write_image_crop_and_caption_fallback(&mut self, xml: &mut Xml, layout: &ImageLayout) {
        if layout.crop.is_some_and(|crop| !crop.is_empty()) {
            // ODF's `fo:clip` accepts physical source-image lengths rather
            // than percentages.  Image pixels do not establish those lengths
            // without choosing a DPI, so mapping our percentage crop would
            // invent geometry.  Keep the raw asset and say that no crop was
            // represented.
            self.warn(
                IMAGE_CROP_UNREPRESENTABLE,
                "an image crop was not exported because ODF fo:clip uses source-image lengths and the OpenDoc crop is DPI-independent percentages",
            );
        }
        if let Some(caption) = layout.caption.as_deref() {
            self.warn(
                IMAGE_CAPTION_AS_PARAGRAPH,
                "an image caption was written as the following paragraph because the block model has no ODT caption anchor",
            );
            let caption = self.sanitize(caption, "image caption");
            xml.open("text:p", &[("text:style-name", "Standard")]);
            self.write_span(xml, &caption, &[]);
            xml.close("text:p");
        }
    }

    /// An asset fallback writes only text, so image-only effects have no ODT
    /// object on which they could be expressed. Name that separate loss: the
    /// missing/unsupported-asset warning says why the picture vanished, not
    /// which authored presentation vanished with it.
    fn write_image_effects_fallback(&mut self, layout: &ImageLayout) {
        let mut effects = Vec::new();
        if layout
            .rotation_degrees
            .is_some_and(|rotation| rotation != 0)
        {
            effects.push("rotation");
        }
        // An explicit 100% alpha is a source value but not a presentation
        // effect. The text fallback has not lost anything visible in that
        // case, so do not turn a no-op into a misleading loss warning.
        if layout.opacity_percent.is_some_and(|opacity| opacity < 100) {
            effects.push("opacity");
        }
        // `CellBorder::none()` deliberately clears a line; it is not a
        // visible outline that the text fallback has failed to represent.
        if layout
            .border
            .is_some_and(|border| border.style() != BorderStyle::None)
        {
            effects.push("border");
        }
        if !effects.is_empty() {
            self.warn(
                IMAGE_EFFECTS_UNREPRESENTABLE,
                format!(
                    "an image could not be exported because its picture bytes were unavailable; its {} were not represented in the text fallback",
                    effects.join(", "),
                ),
            );
        }
    }

    /// `(width, height, size_was_known)`.
    ///
    /// The model's stated size wins; an axis it leaves open is scaled from
    /// the picture's own pixel aspect ratio, which is exactly what "`None`
    /// means intrinsic" says. Nothing is written back into the model.
    fn frame_extent(
        &mut self,
        blob_hash: &str,
        layout: &ImageLayout,
        bytes: &[u8],
    ) -> (String, String, bool) {
        let pixels = image_pixels(bytes);
        match (layout.width, layout.height) {
            (Some(width), Some(height)) => (pt(width), pt(height), true),
            (Some(width), None) => {
                let height = scale_axis(width.twips(), pixels, true);
                (pt(width), twips_to_pt(height), pixels.is_some())
            }
            (None, Some(height)) => {
                let width = scale_axis(height.twips(), pixels, false);
                (twips_to_pt(width), pt(height), pixels.is_some())
            }
            (None, None) => {
                let Some((pixel_width, pixel_height)) = pixels else {
                    let _ = blob_hash;
                    return (
                        twips_to_pt(4 * Length::TWIPS_PER_INCH),
                        twips_to_pt(3 * Length::TWIPS_PER_INCH),
                        false,
                    );
                };
                // 96dpi, then scaled down to the widest an inline picture may
                // be on a Letter page with one-inch margins.
                let max_width = 13 * Length::TWIPS_PER_INCH / 2;
                let mut width = i64::from(pixel_width) * 15;
                let mut height = i64::from(pixel_height) * 15;
                if width > i64::from(max_width) {
                    height = height * i64::from(max_width) / width;
                    width = i64::from(max_width);
                }
                (
                    twips_to_pt(width.max(1) as i32),
                    twips_to_pt(height.max(1) as i32),
                    true,
                )
            }
        }
    }

    fn frame_style(&mut self, placement: ImagePlacement, layout: &ImageLayout) -> String {
        // `style:wrap` names the side the *text* flows down, which is the
        // opposite edge from the one the picture is pulled to.
        let mut properties = match placement {
            ImagePlacement::Block => vec![
                ("style:wrap", "none".to_string()),
                ("style:vertical-pos", "top".to_string()),
                ("style:vertical-rel", "baseline".to_string()),
            ],
            ImagePlacement::WrapStart => vec![
                ("style:wrap", "right".to_string()),
                ("style:horizontal-pos", "left".to_string()),
                ("style:horizontal-rel", "paragraph".to_string()),
                ("style:vertical-pos", "top".to_string()),
                ("style:vertical-rel", "paragraph".to_string()),
            ],
            ImagePlacement::WrapEnd => vec![
                ("style:wrap", "left".to_string()),
                ("style:horizontal-pos", "right".to_string()),
                ("style:horizontal-rel", "paragraph".to_string()),
                ("style:vertical-pos", "top".to_string()),
                ("style:vertical-rel", "paragraph".to_string()),
            ],
        };
        if let Some(opacity) = layout.opacity_percent {
            properties.push(("draw:image-opacity", format!("{opacity}%")));
        }
        if let Some(border) = layout.border {
            properties.push(("fo:border", border_value(border)));
        }
        let attributes: Vec<(&str, &str)> = properties
            .iter()
            .map(|(name, value)| (*name, value.as_str()))
            .collect();
        let mut xml = Xml::fragment();
        xml.empty("style:graphic-properties", &attributes);
        self.styles()
            .style("fr", "graphic", None, None, xml.into_string())
    }

    // -- inlines -----------------------------------------------------------

    fn write_inlines(&mut self, xml: &mut Xml, document: &Document, inlines: &[Inline]) {
        for inline in inlines {
            self.write_inline(xml, document, inline);
        }
    }

    fn write_inline(&mut self, xml: &mut Xml, document: &Document, inline: &Inline) {
        match inline {
            Inline::Text { text, marks, .. } => {
                let text = self.sanitize(text, "text");
                self.write_span(xml, &text, marks);
            }
            Inline::Link {
                text, href, marks, ..
            } => {
                let text = self.sanitize(text, "link text");
                let href = self.sanitize(href, "link target");
                xml.open("text:a", &[("xlink:type", "simple"), ("xlink:href", &href)]);
                self.write_span(xml, &text, marks);
                xml.close("text:a");
            }
            Inline::Citation {
                citation_id,
                rendered_cache,
                ..
            } => {
                let rendered = rendered_cache
                    .clone()
                    .or_else(|| {
                        document
                            .citation_database
                            .rendered_citation(citation_id)
                            .cloned()
                    })
                    .unwrap_or_else(|| format!("[{}]", citation_id.as_str()));
                self.warn(
                    CITATION_AS_TEXT,
                    "citations were written as their rendered text; OpenDocument's bibliography marks carry a different model and would not round-trip",
                );
                let rendered = self.sanitize(&rendered, "citation");
                self.write_span(xml, &rendered, &[]);
            }
            Inline::FootnoteRef { footnote_id, .. } => {
                let Some(number) = self.footnote_numbers.get(footnote_id).copied() else {
                    self.warn(
                        MISSING_FOOTNOTE,
                        format!(
                            "footnote reference to {} was dropped: the document has no such footnote",
                            footnote_id.as_str()
                        ),
                    );
                    return;
                };
                let footnote = document
                    .footnotes
                    .iter()
                    .find(|footnote| &footnote.id == footnote_id)
                    .cloned();
                if let Some(footnote) = footnote {
                    self.write_footnote(xml, document, &footnote, number);
                }
            }
            Inline::Mention { label, .. }
            | Inline::GooglePersonChip { label, .. }
            | Inline::GoogleRichLinkChip { label, .. } => {
                self.warn(
                    MENTION_AS_TEXT,
                    "mentions were written as plain text; OpenDocument has no mention",
                );
                let label = self.sanitize(label, "mention");
                self.write_span(xml, &label, &[]);
            }
            Inline::Dropdown {
                options,
                selected_option_id,
                ..
            } => {
                self.warn(
                    DROPDOWN_AS_TEXT,
                    "dropdowns were written as their selected text; OpenDocument has no portable inline dropdown",
                );
                if let Some(option) = options
                    .iter()
                    .find(|option| option.id == *selected_option_id)
                {
                    let label = self.sanitize(&option.label, "dropdown option");
                    self.write_span(xml, &label, &[]);
                }
            }
            Inline::DateChip { date, .. } => {
                self.warn(
                    "date-chip-as-text",
                    "date chips were written as ISO calendar text; OpenDocument has no portable date chip",
                );
                let date = self.sanitize(date, "date chip");
                self.write_span(xml, &date, &[]);
            }
            Inline::Equation { equation, .. } => {
                self.warn(
                    EQUATION_AS_SOURCE,
                    "equations are written as their LaTeX source text; an OpenDocument formula is an embedded MathML sub-document, which this writer does not produce, and ADR 0003 makes the source canonical anyway",
                );
                let source = self.sanitize(&equation.source, "equation source");
                self.write_span(xml, &source, &[]);
            }
            // ODF models a page number as a field whose value the layout
            // computes, exactly as the model does. The element's content is
            // the last computed value, which is the one thing here that is
            // not part of the document.
            Inline::PageNumber { field, .. } => {
                self.warn(
                    PAGE_NUMBER_PLACEHOLDER,
                    "a page-number field was written with a placeholder as its last computed value; a reader that paginates recomputes it, one that does not shows the placeholder",
                );
                match field {
                    PageNumberField::CurrentPage => xml.text_element(
                        "text:page-number",
                        &[("text:select-page", "current")],
                        "1",
                    ),
                    PageNumberField::PageCount => xml.text_element("text:page-count", &[], "1"),
                }
            }
        }
    }

    fn write_footnote(
        &mut self,
        xml: &mut Xml,
        document: &Document,
        footnote: &Footnote,
        number: usize,
    ) {
        if footnote.deleted || footnote.revision != 1 {
            self.warn(
                DROPPED_FOOTNOTE_STATE,
                "a footnote's deletion flag and revision number were dropped: OpenDocument notes carry neither",
            );
        }
        let id = format!("ftn{number}");
        let number = number.to_string();
        let note_class = if document.endnote_ids.contains(&footnote.id) {
            "endnote"
        } else {
            "footnote"
        };
        xml.open(
            "text:note",
            &[("text:id", &id), ("text:note-class", note_class)],
        );
        xml.text_element("text:note-citation", &[], &number);
        xml.open("text:note-body", &[]);
        xml.open("text:p", &[("text:style-name", "Footnote")]);
        for inline in &footnote.body {
            if matches!(inline, Inline::FootnoteRef { .. }) {
                self.warn(
                    NESTED_FOOTNOTE_REFERENCE,
                    "a footnote referencing another footnote was dropped: OpenDocument does not nest notes",
                );
                continue;
            }
            self.write_inline(xml, document, inline);
        }
        xml.close("text:p");
        xml.close("text:note-body");
        xml.close("text:note");
    }

    /// Strips characters XML 1.0 cannot carry, naming what went.
    fn sanitize(&mut self, value: &str, what: &str) -> String {
        if value.chars().all(is_writable_xml_char) {
            return value.to_string();
        }
        self.warn(
            DROPPED_CONTROL_CHARACTER,
            format!("a character XML cannot encode was removed from {what}"),
        );
        value
            .chars()
            .filter(|ch| is_writable_xml_char(*ch))
            .collect()
    }

    /// One run of text, inside a `text:span` when it carries formatting.
    fn write_span(&mut self, xml: &mut Xml, text: &str, marks: &[Mark]) {
        if text.is_empty() {
            return;
        }
        let format = self.text_format(marks);
        let style = if format.is_empty() {
            None
        } else {
            let body = format.properties();
            Some(self.styles().style("T", "text", None, None, body))
        };
        if let Some(style) = &style {
            xml.open("text:span", &[("text:style-name", style)]);
        }
        write_text_content(xml, text);
        if style.is_some() {
            xml.close("text:span");
        }
    }

    fn text_format(&mut self, marks: &[Mark]) -> TextFormat {
        let mut format = TextFormat::default();
        for mark in marks {
            match mark.kind {
                MarkKind::Bold => format.bold = true,
                MarkKind::Italic => format.italic = true,
                MarkKind::Underline => format.underline = true,
                MarkKind::Strike => format.strike = true,
                MarkKind::Superscript => format.position = Some("super 58%"),
                MarkKind::Subscript => format.position = Some("sub 58%"),
                MarkKind::Code => {
                    format.code = true;
                    self.warn(
                        CODE_AS_MONOSPACE,
                        "code spans were written as a monospace run; OpenDocument has no code mark",
                    );
                }
                MarkKind::Color => match hex_value(mark.value.as_deref()) {
                    Some(color) => format.color = Some(color),
                    None => self.drop_mark_value("color", mark.value.as_deref()),
                },
                MarkKind::Background => match hex_value(mark.value.as_deref()) {
                    Some(color) => format.background = Some(color),
                    None => self.drop_mark_value("background", mark.value.as_deref()),
                },
                MarkKind::Font => match mark
                    .value
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    Some(font) => format.font = Some(font.to_string()),
                    None => self.drop_mark_value("font", mark.value.as_deref()),
                },
                MarkKind::Size => match mark
                    .value
                    .as_deref()
                    .map(str::trim)
                    .and_then(|value| value.parse::<u32>().ok())
                    .filter(|points| *points > 0 && *points <= 1638)
                {
                    Some(points) => format.size_points = Some(points),
                    None => self.drop_mark_value("size", mark.value.as_deref()),
                },
                MarkKind::Link => self.warn(
                    DROPPED_MARK,
                    "a link mark on plain text was dropped; only a link inline becomes a text:a",
                ),
                MarkKind::Citation => self.warn(
                    DROPPED_MARK,
                    "a citation mark was dropped; OpenDocument has no equivalent",
                ),
            }
        }
        format
    }

    fn drop_mark_value(&mut self, what: &str, value: Option<&str>) {
        self.warn(
            DROPPED_MARK_VALUE,
            format!(
                "a {what} mark carrying {} was dropped: OpenDocument cannot express that value",
                value
                    .map(|value| format!("\"{value}\""))
                    .unwrap_or_else(|| "no value".to_string())
            ),
        );
    }

    // -- page furniture ----------------------------------------------------

    /// One header or footer body, or `None` when the document has none.
    fn write_furniture(&mut self, document: &Document, slot: HeaderFooterSlot) -> Option<String> {
        let blocks = document.furniture(slot);
        if blocks.is_empty() {
            return None;
        }
        let body = self.write_blocks_fragment(document, blocks, false);
        Some(body)
    }

    /// `style:page-layout`, and the header/footer heights that make the body
    /// start exactly where the model says.
    ///
    /// The two models differ in one place and the arithmetic is the whole of
    /// it. `PageSetup` measures `margin_top` from the sheet to the *body* and
    /// `margin_header` from the sheet to the *header*, the way
    /// WordprocessingML does. ODF measures `fo:margin-top` from the sheet to
    /// whatever comes first — the header if there is one — and then the
    /// header's own height and spacing push the body down. So with a header
    /// the page margin is `margin_header` and the header is given a fixed
    /// height of `margin_top - margin_header` with no spacing below it, which
    /// puts the body at `margin_top`. Without a header the page margin is
    /// `margin_top` and nothing else is needed.
    fn page_layout(&mut self, document: &Document, header: bool, footer: bool) -> String {
        let setup = &document.page_setup;
        let mut xml = Xml::fragment();
        xml.open("style:page-layout", &[("style:name", "pm1")]);
        let top = if header {
            setup.margin_header
        } else {
            setup.margin_top
        };
        let bottom = if footer {
            setup.margin_footer
        } else {
            setup.margin_bottom
        };
        let orientation = match setup.orientation() {
            PageOrientation::Portrait => "portrait",
            PageOrientation::Landscape => "landscape",
        };
        xml.empty(
            "style:page-layout-properties",
            &[
                ("fo:page-width", &pt(setup.width)),
                ("fo:page-height", &pt(setup.height)),
                ("style:print-orientation", orientation),
                ("fo:margin-top", &pt(top)),
                ("fo:margin-bottom", &pt(bottom)),
                // `fo:margin-left`/`-right` are physical and the model's
                // margins are logical. OpenDoc has no document-level writing
                // direction — direction is a block property — so the leading
                // edge is the left one, which is what every left-to-right
                // document means by it.
                ("fo:margin-left", &pt(setup.margin_start)),
                ("fo:margin-right", &pt(setup.margin_end)),
                ("style:writing-mode", "lr-tb"),
            ],
        );
        let header_height = self.furniture_height(
            header,
            setup.margin_top.twips() - setup.margin_header.twips(),
            "header",
        );
        let footer_height = self.furniture_height(
            footer,
            setup.margin_bottom.twips() - setup.margin_footer.twips(),
            "footer",
        );
        if let Some(height) = header_height {
            xml.open("style:header-style", &[]);
            xml.empty(
                "style:header-footer-properties",
                &[
                    ("fo:min-height", &twips_to_pt(height)),
                    ("fo:margin-bottom", "0pt"),
                    ("style:dynamic-spacing", "false"),
                ],
            );
            xml.close("style:header-style");
        }
        if let Some(height) = footer_height {
            xml.open("style:footer-style", &[]);
            xml.empty(
                "style:header-footer-properties",
                &[
                    ("fo:min-height", &twips_to_pt(height)),
                    ("fo:margin-top", "0pt"),
                    ("style:dynamic-spacing", "false"),
                ],
            );
            xml.close("style:footer-style");
        }
        xml.close("style:page-layout");
        xml.into_string()
    }

    fn furniture_height(&mut self, present: bool, twips: i32, what: &str) -> Option<i32> {
        if !present {
            return None;
        }
        if twips < 0 {
            self.warn(
                FURNITURE_MARGIN_CLAMPED,
                format!(
                    "the {what} margin leaves no room between the sheet edge and the body, so the {what} was given no height of its own; OpenDocument measures the page margin to the {what} rather than to the body"
                ),
            );
            return Some(0);
        }
        Some(twips)
    }

    // -- package parts -----------------------------------------------------

    fn content_part(&self, body: &str) -> Vec<u8> {
        let mut xml = Xml::odf_part();
        let mut attrs: Vec<(&str, &str)> = NAMESPACES.to_vec();
        attrs.push(("office:version", ODF_VERSION));
        xml.open("office:document-content", &attrs);
        write_font_faces(&mut xml);
        xml.open("office:automatic-styles", &[]);
        self.content_styles.write(&mut xml);
        xml.close("office:automatic-styles");
        xml.open("office:body", &[]);
        xml.open("office:text", &[]);
        if body.is_empty() {
            xml.empty("text:p", &[("text:style-name", "Standard")]);
        } else {
            xml.raw(body);
        }
        xml.close("office:text");
        xml.close("office:body");
        xml.close("office:document-content");
        xml.into_bytes()
    }

    fn styles_part(
        &self,
        page_layout: &str,
        header: Option<&str>,
        footer: Option<&str>,
    ) -> Vec<u8> {
        let mut xml = Xml::odf_part();
        let mut attrs: Vec<(&str, &str)> = NAMESPACES.to_vec();
        attrs.push(("office:version", ODF_VERSION));
        xml.open("office:document-styles", &attrs);
        write_font_faces(&mut xml);
        write_named_styles(&mut xml);
        xml.open("office:automatic-styles", &[]);
        xml.raw(page_layout);
        self.furniture_styles.write(&mut xml);
        xml.close("office:automatic-styles");
        xml.open("office:master-styles", &[]);
        xml.open(
            "style:master-page",
            &[
                ("style:name", "Standard"),
                ("style:page-layout-name", "pm1"),
            ],
        );
        for (element, body) in [("style:header", header), ("style:footer", footer)] {
            match body {
                Some(body) => {
                    xml.open(element, &[]);
                    xml.raw(body);
                    xml.close(element);
                }
                None => xml.empty(element, &[("style:display", "false")]),
            }
        }
        xml.close("style:master-page");
        xml.close("office:master-styles");
        xml.close("office:document-styles");
        xml.into_bytes()
    }

    fn manifest_part(&self, parts: &[(String, Vec<u8>)]) -> Vec<u8> {
        let mut xml = Xml::odf_part();
        xml.open(
            "manifest:manifest",
            &[
                ("xmlns:manifest", NS_MANIFEST),
                ("manifest:version", ODF_VERSION),
            ],
        );
        xml.empty(
            "manifest:file-entry",
            &[
                ("manifest:full-path", "/"),
                ("manifest:version", ODF_VERSION),
                ("manifest:media-type", ODT_MEDIA_TYPE),
            ],
        );
        for (path, _) in parts {
            // `mimetype` is the package's magic number, not a part, and the
            // manifest never lists itself.
            if path == "mimetype" || path == "META-INF/manifest.xml" {
                continue;
            }
            let media_type = if path.ends_with(".xml") {
                "text/xml".to_string()
            } else {
                picture_media_type(path)
            };
            xml.empty(
                "manifest:file-entry",
                &[
                    ("manifest:full-path", path),
                    ("manifest:media-type", &media_type),
                ],
            );
        }
        xml.close("manifest:manifest");
        xml.into_bytes()
    }
}

// ---------------------------------------------------------------------------
// Block properties
// ---------------------------------------------------------------------------

/// `style:paragraph-properties` attributes for a block's properties.
///
/// ODF property values are *attributes*, so unlike WordprocessingML there is
/// no schema-fixed child order to get wrong; the order here only keeps the
/// bytes deterministic.
fn paragraph_property_attributes(properties: &BlockProperties) -> Vec<(&'static str, String)> {
    let mut attrs: Vec<(&'static str, String)> = Vec::new();
    let rtl = properties.direction == Some(TextDirection::RightToLeft);
    if let Some(direction) = properties.direction {
        attrs.push((
            "style:writing-mode",
            match direction {
                TextDirection::RightToLeft => "rl-tb",
                TextDirection::LeftToRight => "lr-tb",
            }
            .to_string(),
        ));
    }
    // `fo:text-align` is the one property LibreOffice — the reference ODF
    // implementation — resolves *physically*: `start` draws on the left and
    // `end` on the right whatever the writing mode says, and its own export
    // writes `fo:text-align="end"` for a right-to-left paragraph that is
    // logically start-aligned. Measured, not assumed: a probe ODT rendered
    // through LibreOffice put `start` on the left in both writing modes.
    // So a right-to-left block's logical start is spelled `end` here.
    let (start_align, end_align) = if rtl {
        ("end", "start")
    } else {
        ("start", "end")
    };
    match properties.alignment {
        Some(alignment) => {
            attrs.push((
                "fo:text-align",
                match alignment {
                    Alignment::Start => start_align,
                    Alignment::Center => "center",
                    Alignment::End => end_align,
                    Alignment::Justify => "justify",
                }
                .to_string(),
            ));
            if alignment == Alignment::Justify {
                attrs.push(("fo:text-align-last", start_align.to_string()));
            }
        }
        // The same measurement showed that LibreOffice does *not* derive a
        // paragraph's alignment from `style:writing-mode`: an `rl-tb`
        // paragraph with no `fo:text-align` renders flush left, which is
        // simply wrong for right-to-left text. An unstated alignment means
        // "wherever this block's direction starts", so for a right-to-left
        // block that has to be written down.
        None if rtl => attrs.push(("fo:text-align", start_align.to_string())),
        None => {}
    }
    // `fo:margin-left`/`-right` and `fo:text-indent` need no such swap:
    // LibreOffice resolves all three *logically*, so `fo:margin-left` is the
    // before-text edge in both writing modes — which is also what its own
    // export writes for a right-to-left paragraph indented from the right.
    if let Some(indent) = properties.indent_start {
        attrs.push(("fo:margin-left", pt(indent)));
    }
    if let Some(indent) = properties.indent_end {
        attrs.push(("fo:margin-right", pt(indent)));
    }
    if let Some(first_line) = properties.indent_first_line {
        attrs.push(("fo:text-indent", pt(first_line)));
    }
    if let Some(space) = properties.space_before {
        attrs.push(("fo:margin-top", pt(space)));
    }
    if let Some(space) = properties.space_after {
        attrs.push(("fo:margin-bottom", pt(space)));
    }
    if let Some(keep_with_next) = properties.keep_with_next {
        attrs.push((
            "fo:keep-with-next",
            if keep_with_next { "always" } else { "auto" }.to_string(),
        ));
    }
    if let Some(background) = properties.background {
        attrs.push(("fo:background-color", background.as_hex()));
    }
    if let Some(border) = properties.border {
        // One `fo:border` is exactly ODF's uniform paragraph frame. Per-side
        // ODF borders are intentionally not emitted because the model does
        // not claim to know four independent values.
        attrs.push(("fo:border", border_value(border)));
    }
    // All three of the model's rules have an exact ODF spelling, which CSS
    // does not (`opendoc-render` has to approximate `Exact` with
    // `line-height`).
    match properties.line_spacing {
        None => {}
        Some(LineSpacing::Multiple(multiple)) => attrs.push((
            "fo:line-height",
            line_height_percent(multiple.thousandths()),
        )),
        Some(LineSpacing::Exact(height)) => attrs.push(("fo:line-height", pt(height))),
        Some(LineSpacing::AtLeast(height)) => {
            attrs.push(("style:line-height-at-least", pt(height)))
        }
    }
    attrs
}

fn is_list_item_of(block: &Block, list_id: &StableId) -> bool {
    matches!(&block.kind, BlockKind::ListItem { list_id: id, .. } if id == list_id)
}

fn list_item_marker(block: &Block) -> Option<(u8, ListKind)> {
    match &block.kind {
        BlockKind::ListItem { level, kind, .. } => Some((*level, *kind)),
        _ => None,
    }
}

/// `text:list-level-style-*` elements for the levels a run uses.
///
/// Every level from 1 to the deepest is defined, because ODF numbers levels
/// from 1 and a gap would leave a level undefined. The indents match
/// `docx_write`'s numbering definitions (0.5in per level with a 0.25in
/// hanging indent) so the two exports of one document line up.
fn list_level_definitions(
    flavours: &BTreeMap<u8, ListFlavour>,
    properties: Option<&opendoc_core::ListProperties>,
) -> String {
    let deepest = flavours.keys().copied().max().unwrap_or(0);
    let mut xml = Xml::fragment();
    for level in 0..=deepest {
        let flavour = flavours
            .get(&level)
            .cloned()
            .or_else(|| flavours.values().next().cloned())
            .unwrap_or(ListFlavour::Bullet(opendoc_core::BulletListMarker::Disc));
        let flavour = match flavour {
            ListFlavour::Bullet(_) => ListFlavour::Bullet(
                properties
                    .map(|properties| properties.bullet_marker_for(level))
                    .unwrap_or_else(|| opendoc_core::BulletListMarker::inherited_at(level)),
            ),
            other => other,
        };
        let number = (u32::from(level) + 1).to_string();
        let margin = twips_to_pt((i32::from(level) + 1) * 720);
        let indent = twips_to_pt(-360);
        match flavour.bullet_char() {
            Some(bullet) => {
                xml.open(
                    "text:list-level-style-bullet",
                    &[
                        ("text:level", &number),
                        ("text:style-name", "Bullet_20_Symbol"),
                        ("text:bullet-char", bullet),
                    ],
                );
            }
            None => {
                let start = properties
                    .map(|properties| properties.start_for(level))
                    .unwrap_or(1);
                xml.open(
                    "text:list-level-style-number",
                    &[
                        ("text:level", &number),
                        ("text:style-name", "Numbering_20_Symbols"),
                        ("style:num-suffix", "."),
                        (
                            "style:num-format",
                            properties
                                .map(|properties| properties.format_for(level))
                                .unwrap_or_else(|| {
                                    opendoc_core::OrderedListFormat::inherited_at(level)
                                })
                                .odt_name(),
                        ),
                        ("text:start-value", &start.to_string()),
                    ],
                );
            }
        }
        // Without `label-alignment` the reader uses ODF's *legacy* label
        // positioning (`text:space-before`, `text:min-label-width`) and
        // ignores `style:list-level-label-alignment` entirely — which is
        // exactly what happened the first time: LibreOffice drew every level
        // flush against the page margin.
        xml.open(
            "style:list-level-properties",
            &[("text:list-level-position-and-space-mode", "label-alignment")],
        );
        xml.empty(
            "style:list-level-label-alignment",
            &[
                ("text:label-followed-by", "listtab"),
                ("fo:margin-left", &margin),
                ("fo:text-indent", &indent),
            ],
        );
        xml.close("style:list-level-properties");
        xml.close(match flavour.bullet_char() {
            Some(_) => "text:list-level-style-bullet",
            None => "text:list-level-style-number",
        });
    }
    xml.into_string()
}

// ---------------------------------------------------------------------------
// Text runs
// ---------------------------------------------------------------------------

#[derive(Default)]
struct TextFormat {
    code: bool,
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
    position: Option<&'static str>,
    color: Option<String>,
    background: Option<String>,
    font: Option<String>,
    size_points: Option<u32>,
}

impl TextFormat {
    fn is_empty(&self) -> bool {
        !self.code
            && !self.bold
            && !self.italic
            && !self.underline
            && !self.strike
            && self.position.is_none()
            && self.color.is_none()
            && self.background.is_none()
            && self.font.is_none()
            && self.size_points.is_none()
    }

    fn properties(&self) -> String {
        let mut attrs: Vec<(&'static str, String)> = Vec::new();
        if let Some(font) = &self.font {
            attrs.push(("fo:font-family", font.clone()));
        } else if self.code {
            attrs.push(("style:font-name", MONO.to_string()));
        }
        if self.bold {
            attrs.push(("fo:font-weight", "bold".to_string()));
        }
        if self.italic {
            attrs.push(("fo:font-style", "italic".to_string()));
        }
        if self.underline {
            attrs.push(("style:text-underline-style", "solid".to_string()));
            attrs.push(("style:text-underline-width", "auto".to_string()));
            attrs.push(("style:text-underline-color", "font-color".to_string()));
        }
        if self.strike {
            attrs.push(("style:text-line-through-style", "solid".to_string()));
            attrs.push(("style:text-line-through-type", "single".to_string()));
        }
        if let Some(color) = &self.color {
            attrs.push(("fo:color", color.clone()));
        }
        if let Some(background) = &self.background {
            attrs.push(("fo:background-color", background.clone()));
        }
        if let Some(points) = self.size_points {
            attrs.push(("fo:font-size", format!("{points}pt")));
        }
        if let Some(position) = self.position {
            attrs.push(("style:text-position", position.to_string()));
        }
        let mut xml = Xml::fragment();
        let borrowed: Vec<(&str, &str)> = attrs
            .iter()
            .map(|(name, value)| (*name, value.as_str()))
            .collect();
        xml.empty("style:text-properties", &borrowed);
        xml.into_string()
    }
}

/// Writes text into a paragraph, with the elements OpenDocument uses instead
/// of literal characters.
///
/// ODF collapses whitespace the way XML does, so a run of spaces is
/// `<text:s text:c="n"/>` rather than n space characters — without this a
/// paragraph indented with spaces loses them silently. Tabs and line breaks
/// are elements for the same reason.
fn write_text_content(xml: &mut Xml, text: &str) {
    let mut pending_spaces = 0usize;
    let mut wrote_anything = false;
    let flush = |xml: &mut Xml, pending: &mut usize, wrote: &mut bool| {
        if *pending == 0 {
            return;
        }
        // A single space between two words survives collapsing; a leading
        // space or a second consecutive one does not.
        if *pending == 1 && *wrote {
            xml.text(" ");
        } else {
            xml.empty("text:s", &[("text:c", &pending.to_string())]);
        }
        *pending = 0;
        *wrote = true;
    };
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            ' ' => pending_spaces += 1,
            '\t' => {
                flush(xml, &mut pending_spaces, &mut wrote_anything);
                xml.empty("text:tab", &[]);
                wrote_anything = true;
            }
            '\r' | '\n' => {
                flush(xml, &mut pending_spaces, &mut wrote_anything);
                // A CRLF pair is one line break, not two.
                if ch == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                xml.empty("text:line-break", &[]);
                wrote_anything = true;
            }
            _ => {
                flush(xml, &mut pending_spaces, &mut wrote_anything);
                let mut buffer = String::new();
                buffer.push(ch);
                xml.text(&buffer);
                wrote_anything = true;
            }
        }
    }
    // Trailing spaces have nothing after them to keep them, so they are
    // always elements.
    if pending_spaces > 0 {
        xml.empty("text:s", &[("text:c", &pending_spaces.to_string())]);
    }
}
