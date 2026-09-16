//! DOCX (WordprocessingML) writer — the mirror of [`crate::docx`].
//!
//! The reader in `docx.rs` parses a package into the OpenDoc model; this
//! module turns the model back into a package. The two are deliberately kept
//! in correspondence: every construct written here is one the reader knows how
//! to read, so a document can be exported and re-imported without passing
//! through a third implementation to find out what was lost.
//!
//! Two rules govern the mapping.
//!
//! * **Twips are exact.** DOCX measures paragraph geometry in twips and so
//!   does [`Length`]; every indent and spacing value is written as the integer
//!   the model holds, with no unit conversion and therefore no rounding drift.
//!   The one place a conversion is unavoidable — line spacing, which the model
//!   keeps in thousandths and `w:line` counts in 240ths — checks that the
//!   conversion is exact and emits a warning when it is not.
//! * **Nothing is dropped silently.** Anything the model can express and
//!   WordprocessingML cannot (or can only approximate) produces a
//!   [`ModelWarning`] naming it, exactly as the reader does in the other
//!   direction.
//! * **A table is written as WordprocessingML's own grid.** `w:gridCol` takes
//!   the column widths unchanged, a column span becomes `w:gridSpan`, a row
//!   span becomes `w:vMerge` restart plus a continuation cell per covered
//!   row, and `w:shd`/`w:tcBorders`/`w:tcMar`/`w:vAlign` carry the cell
//!   properties — so the reader in `docx.rs` reads back the grid that was
//!   written, merges and styling included (ADR 0013). The one thing the
//!   format has nowhere for is the *content* of a covered cell, which the
//!   model retains, so exporting one warns.

use crate::xml_write::{is_writable_xml_char, Xml};
use crate::{ExportImage, ImportError};
use opendoc_core::{
    Alignment, Block, BlockKind, BlockProperties, BorderStyle, Document, Footnote,
    HeaderFooterSlot, ImageLayout, ImagePlacement, Inline, Length, LineSpacing, ListKind,
    ListProperties, Mark, MarkKind, ModelWarning, OrderedListFormat, PageNumberField,
    PageOrientation, PositionedImageAnchor, PositionedImageLayer, StableId, TableCell,
    TableCellProperties, TableColumn, TableRow, TextDirection, VerticalAlignment,
};
use std::collections::{BTreeMap, BTreeSet};

// ---------------------------------------------------------------------------
// Public payloads
// ---------------------------------------------------------------------------

pub(crate) struct DocxExport {
    pub(crate) bytes: Vec<u8>,
    pub(crate) warnings: Vec<ModelWarning>,
}

// ---------------------------------------------------------------------------
// Warning codes
// ---------------------------------------------------------------------------

const CHECKLIST_AS_BULLET: &str = "docx-export-checklist-as-bullet";
const SPLIT_MIXED_LIST: &str = "docx-export-split-mixed-list";
const APPROXIMATED_LINE_SPACING: &str = "docx-export-approximated-line-spacing";
const DROPPED_BLOCK_PROPERTIES: &str = "docx-export-dropped-block-properties";
const CODE_AS_MONOSPACE: &str = "docx-export-code-mark-as-monospace";
const DROPPED_MARK: &str = "docx-export-dropped-mark";
const DROPPED_MARK_VALUE: &str = "docx-export-dropped-mark-value";
const CITATION_AS_TEXT: &str = "docx-export-citation-as-text";
const MENTION_AS_TEXT: &str = "docx-export-mention-as-text";
const DROPDOWN_AS_TEXT: &str = "docx-export-dropdown-as-text";
const EQUATION_AS_SOURCE: &str = "docx-export-equation-as-source";
const DROPPED_EQUATION_CONTENT: &str = "docx-export-dropped-equation-content";
const MISSING_IMAGE_BLOB: &str = "docx-export-missing-image-blob";
const UNSUPPORTED_IMAGE_MEDIA_TYPE: &str = "docx-export-unsupported-image-media-type";
const UNKNOWN_IMAGE_SIZE: &str = "docx-export-unknown-image-size";
const POSITIONED_IMAGE_AS_INLINE: &str = "docx-export-positioned-image-as-inline";
const IMAGE_DOUBLE_BORDER_AS_SOLID: &str = "docx-export-image-double-border-as-solid";
const IMAGE_CAPTION_WITHOUT_IMAGE: &str = "docx-export-image-caption-without-image";
const IMAGE_EFFECTS_UNREPRESENTABLE: &str = "docx-export-image-effects-unrepresentable";
const DROPPED_CONTROL_CHARACTER: &str = "docx-export-dropped-control-character";
const DROPPED_COMMENTS: &str = "docx-export-dropped-comments";
const DROPPED_SUGGESTIONS: &str = "docx-export-dropped-suggestions";
const DROPPED_CITATION_DATABASE: &str = "docx-export-dropped-citation-database";
const DROPPED_DOI: &str = "docx-export-dropped-doi";
const DROPPED_FOOTNOTE_STATE: &str = "docx-export-dropped-footnote-state";
const UNPLACED_BOOKMARKS: &str = "docx-export-unplaced-bookmarks";
const NESTED_FOOTNOTE_REFERENCE: &str = "docx-export-nested-footnote-reference";
const MISSING_FOOTNOTE: &str = "docx-export-missing-footnote";
const CLAMPED_HEADING_LEVEL: &str = "docx-export-clamped-heading-level";
const PAGE_NUMBER_PLACEHOLDER: &str = "docx-export-page-number-placeholder";
const DROPPED_COVERED_CELL_CONTENT: &str = "docx-export-dropped-covered-cell-content";
const APPROXIMATED_CELL_BORDER: &str = "docx-export-approximated-cell-border";
const DROPPED_TABLE_ROW_HEADER: &str = "docx-export-dropped-table-row-header";

// ---------------------------------------------------------------------------
// Namespaces and content types
// ---------------------------------------------------------------------------

const NS_W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const NS_R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const NS_M: &str = "http://schemas.openxmlformats.org/officeDocument/2006/math";
const NS_WP: &str = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";
const NS_A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const NS_PIC: &str = "http://schemas.openxmlformats.org/drawingml/2006/picture";
const NS_PACKAGE_RELS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const NS_CONTENT_TYPES: &str = "http://schemas.openxmlformats.org/package/2006/content-types";

const REL_OFFICE_DOCUMENT: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";
const REL_CORE_PROPERTIES: &str =
    "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties";
const REL_STYLES: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles";
const REL_NUMBERING: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering";
const REL_FOOTNOTES: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes";
const REL_ENDNOTES: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/endnotes";
const REL_HEADER: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/header";
const REL_FOOTER: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer";
const REL_SETTINGS: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings";
const REL_IMAGE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";
const REL_HYPERLINK: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink";

const CT_DOCUMENT: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml";
const CT_STYLES: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml";
const CT_NUMBERING: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml";
const CT_FOOTNOTES: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml";
const CT_ENDNOTES: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.endnotes+xml";
const CT_HEADER: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml";
const CT_FOOTER: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml";
const CT_SETTINGS: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml";
const CT_CORE_PROPERTIES: &str = "application/vnd.openxmlformats-package.core-properties+xml";
const CT_RELATIONSHIPS: &str = "application/vnd.openxmlformats-package.relationships+xml";

/// Every run in a heading carries the heading style's own character
/// formatting once it is read back, because WordprocessingML has no way to
/// mark a paragraph as a heading except through a style, and a style with no
/// formatting produces a document that does not look like it has headings.
/// These are the two properties the style sets, and therefore the two marks a
/// heading's text gains on re-import.
const HEADING_SIZES_HALF_POINTS: [u32; 6] = [40, 32, 28, 24, 22, 20];

/// 1px at 96dpi in English Metric Units, the unit `wp:extent` counts in.
const EMU_PER_PIXEL: i64 = 9525;
const EMU_PER_INCH: i64 = 914_400;
/// Letter width less one-inch margins: the widest an inline image may be
/// before it is scaled down to fit the text column.
const MAX_IMAGE_WIDTH_EMU: i64 = EMU_PER_INCH * 13 / 2;

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub(crate) fn export_docx_bytes(
    document: &Document,
    images: &BTreeMap<String, ExportImage>,
) -> Result<DocxExport, ImportError> {
    document
        .validate()
        .map_err(|err| ImportError::InvalidDocument(err.to_string()))?;
    let mut exporter = Exporter::new(images);
    let bytes = exporter.run(document)?;
    let mut warnings = exporter.into_warnings();
    dedupe(&mut warnings);
    Ok(DocxExport { bytes, warnings })
}

fn dedupe(warnings: &mut Vec<ModelWarning>) {
    let mut seen = BTreeSet::new();
    warnings.retain(|warning| seen.insert((warning.code.clone(), warning.message.clone())));
}

// ---------------------------------------------------------------------------
// Exporter
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum ListFlavour {
    Bullet(opendoc_core::BulletListMarker),
    Ordered(OrderedListFormat),
    Unchecked,
    Checked,
}

impl ListFlavour {
    fn of(
        kind: ListKind,
        format: OrderedListFormat,
        bullet_marker: opendoc_core::BulletListMarker,
    ) -> Self {
        match kind {
            ListKind::Bullet => ListFlavour::Bullet(bullet_marker),
            ListKind::Ordered => ListFlavour::Ordered(format),
            ListKind::Checklist { checked: false } => ListFlavour::Unchecked,
            ListKind::Checklist { checked: true } => ListFlavour::Checked,
        }
    }

    /// `w:numFmt` plus the `w:lvlText` used at every level.
    fn level_format(&self) -> (&'static str, Option<&str>) {
        match self {
            ListFlavour::Bullet(marker) => (
                "bullet",
                Some(match marker {
                    opendoc_core::BulletListMarker::Disc => "\u{2022}",
                    opendoc_core::BulletListMarker::Circle => "\u{25e6}",
                    opendoc_core::BulletListMarker::Square => "\u{25a0}",
                    opendoc_core::BulletListMarker::Custom(glyph) => glyph,
                }),
            ),
            ListFlavour::Ordered(format) => (format.docx_name(), None),
            ListFlavour::Unchecked => ("bullet", Some("\u{2610}")),
            ListFlavour::Checked => ("bullet", Some("\u{2612}")),
        }
    }
}

struct Rel {
    id: String,
    rel_type: &'static str,
    target: String,
    external: bool,
}

struct Exporter<'a> {
    images: &'a BTreeMap<String, ExportImage>,
    warnings: Vec<ModelWarning>,
    rels: Vec<Rel>,
    media: Vec<(String, Vec<u8>)>,
    media_extensions: BTreeMap<String, String>,
    image_parts: BTreeMap<String, String>,
    lists: BTreeMap<(StableId, ListFlavour), u32>,
    list_flavours: BTreeMap<StableId, BTreeSet<ListFlavour>>,
    list_properties: BTreeMap<StableId, ListProperties>,
    footnote_ids: BTreeMap<StableId, u32>,
    endnote_ids: BTreeMap<StableId, u32>,
    bookmark_ranges: BTreeMap<StableId, Vec<(StableId, u32, String)>>,
    emitted_bookmarks: BTreeSet<StableId>,
    next_rel: u32,
    next_doc_pr: u32,
    next_bookmark: u32,
}

/// The relationships used by a document-wide set of Word header/footer
/// variants. Keeping this together prevents the section serializer from
/// growing a positional argument for every new, independently optional slot.
#[derive(Clone, Copy, Default)]
struct FurnitureReferences<'a> {
    header: Option<&'a str>,
    footer: Option<&'a str>,
    first_page_header: Option<&'a str>,
    first_page_footer: Option<&'a str>,
    even_page_header: Option<&'a str>,
    even_page_footer: Option<&'a str>,
}

/// The optional package parts that need content-type overrides.
#[derive(Clone, Copy, Default)]
struct ContentTypeParts {
    lists: bool,
    footnotes: bool,
    endnotes: bool,
    header: bool,
    footer: bool,
    first_page_header: bool,
    first_page_footer: bool,
    even_page_header: bool,
    even_page_footer: bool,
    settings: bool,
}

impl<'a> Exporter<'a> {
    fn new(images: &'a BTreeMap<String, ExportImage>) -> Self {
        Self {
            images,
            warnings: Vec::new(),
            rels: Vec::new(),
            media: Vec::new(),
            media_extensions: BTreeMap::new(),
            image_parts: BTreeMap::new(),
            lists: BTreeMap::new(),
            list_flavours: BTreeMap::new(),
            list_properties: BTreeMap::new(),
            footnote_ids: BTreeMap::new(),
            endnote_ids: BTreeMap::new(),
            bookmark_ranges: BTreeMap::new(),
            emitted_bookmarks: BTreeSet::new(),
            next_rel: 0,
            next_doc_pr: 0,
            next_bookmark: 0,
        }
    }

    fn into_warnings(self) -> Vec<ModelWarning> {
        self.warnings
    }

    fn warn(&mut self, code: &'static str, message: impl Into<String>) {
        self.warnings.push(ModelWarning {
            code: code.to_string(),
            message: message.into(),
        });
    }

    fn run(&mut self, document: &Document) -> Result<Vec<u8>, ImportError> {
        self.report_unrepresentable_document_parts(document);
        self.install_bookmark_ranges(document);
        self.list_properties = document.list_properties.clone();
        let mut next_footnote_id = 1;
        let mut next_endnote_id = 1;
        for footnote in &document.footnotes {
            if document.endnote_ids.contains(&footnote.id) {
                self.endnote_ids
                    .insert(footnote.id.clone(), next_endnote_id);
                next_endnote_id += 1;
            } else {
                self.footnote_ids
                    .insert(footnote.id.clone(), next_footnote_id);
                next_footnote_id += 1;
            }
        }

        // The body is written first so that relationships, numbering
        // definitions and media parts are all discovered before the parts that
        // list them are serialized.
        let mut body = self.write_body(document);
        let header = self.write_furniture(document, HeaderFooterSlot::Header);
        let footer = self.write_furniture(document, HeaderFooterSlot::Footer);
        let first_page_header = self.write_furniture(document, HeaderFooterSlot::FirstPageHeader);
        let first_page_footer = self.write_furniture(document, HeaderFooterSlot::FirstPageFooter);
        let even_page_header = self.write_furniture(document, HeaderFooterSlot::EvenPageHeader);
        let even_page_footer = self.write_furniture(document, HeaderFooterSlot::EvenPageFooter);
        let furniture = FurnitureReferences {
            header: header.as_ref().map(|(id, _)| id.as_str()),
            footer: footer.as_ref().map(|(id, _)| id.as_str()),
            first_page_header: first_page_header.as_ref().map(|(id, _)| id.as_str()),
            first_page_footer: first_page_footer.as_ref().map(|(id, _)| id.as_str()),
            even_page_header: even_page_header.as_ref().map(|(id, _)| id.as_str()),
            even_page_footer: even_page_footer.as_ref().map(|(id, _)| id.as_str()),
        };
        body.push_str(&self.section_properties(document, furniture));
        let footnotes = self.write_footnotes(document);
        let endnotes = self.write_endnotes(document);
        self.report_unplaced_bookmarks(document);

        let mut parts: Vec<(String, Vec<u8>)> = Vec::new();
        let has_lists = !self.lists.is_empty();
        let has_footnotes = footnotes.is_some();
        let has_endnotes = endnotes.is_some();

        self.add_rel(REL_STYLES, "styles.xml", false);
        if has_lists {
            self.add_rel(REL_NUMBERING, "numbering.xml", false);
        }
        if has_footnotes {
            self.add_rel(REL_FOOTNOTES, "footnotes.xml", false);
        }
        if has_endnotes {
            self.add_rel(REL_ENDNOTES, "endnotes.xml", false);
        }
        if even_page_header.is_some() || even_page_footer.is_some() {
            self.add_rel(REL_SETTINGS, "settings.xml", false);
        }

        parts.push((
            "[Content_Types].xml".to_string(),
            self.content_types(ContentTypeParts {
                lists: has_lists,
                footnotes: has_footnotes,
                endnotes: has_endnotes,
                header: header.is_some(),
                footer: footer.is_some(),
                first_page_header: first_page_header.is_some(),
                first_page_footer: first_page_footer.is_some(),
                even_page_header: even_page_header.is_some(),
                even_page_footer: even_page_footer.is_some(),
                settings: even_page_header.is_some() || even_page_footer.is_some(),
            }),
        ));
        parts.push(("_rels/.rels".to_string(), root_rels()));
        parts.push(("docProps/core.xml".to_string(), core_properties(document)));
        parts.push(("word/document.xml".to_string(), document_part(&body)));
        parts.push((
            "word/_rels/document.xml.rels".to_string(),
            self.document_rels(),
        ));
        parts.push(("word/styles.xml".to_string(), styles_part(document)));
        if even_page_header.is_some() || even_page_footer.is_some() {
            parts.push(("word/settings.xml".to_string(), settings_part(true)));
        }
        if has_lists {
            parts.push(("word/numbering.xml".to_string(), self.numbering_part()));
        }
        if let Some(footnotes) = footnotes {
            parts.push(("word/footnotes.xml".to_string(), footnotes));
        }
        if let Some(endnotes) = endnotes {
            parts.push(("word/endnotes.xml".to_string(), endnotes));
        }
        if let Some((_, bytes)) = header {
            parts.push(("word/header1.xml".to_string(), bytes));
        }
        if let Some((_, bytes)) = footer {
            parts.push(("word/footer1.xml".to_string(), bytes));
        }
        if let Some((_, bytes)) = first_page_header {
            parts.push(("word/header2.xml".to_string(), bytes));
        }
        if let Some((_, bytes)) = first_page_footer {
            parts.push(("word/footer2.xml".to_string(), bytes));
        }
        if let Some((_, bytes)) = even_page_header {
            parts.push(("word/header3.xml".to_string(), bytes));
        }
        if let Some((_, bytes)) = even_page_footer {
            parts.push(("word/footer3.xml".to_string(), bytes));
        }
        for (path, bytes) in std::mem::take(&mut self.media) {
            parts.push((path, bytes));
        }
        zip_parts(&parts)
    }

    // -- document-level bookkeeping ---------------------------------------

    fn report_unrepresentable_document_parts(&mut self, document: &Document) {
        if !document.comments.is_empty() {
            self.warn(
                DROPPED_COMMENTS,
                format!(
                    "{} OpenDoc comment thread(s) were dropped: the DOCX writer does not produce a comments part",
                    document.comments.len()
                ),
            );
        }
        if !document.suggestions.is_empty() {
            self.warn(
                DROPPED_SUGGESTIONS,
                format!(
                    "{} OpenDoc suggestion(s) were dropped: the DOCX writer does not produce tracked changes",
                    document.suggestions.len()
                ),
            );
        }
        if document.doi.is_some() {
            self.warn(
                DROPPED_DOI,
                "the document DOI has no WordprocessingML equivalent and was dropped",
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
        // Which CSL styles OpenDoc bundles is a product decision, so a
        // document formatted by the fallback renderer says so here too.
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
            self.next_bookmark += 1;
            self.bookmark_ranges
                .entry(bookmark.block_id.clone())
                .or_default()
                .push((
                    bookmark.id.clone(),
                    self.next_bookmark,
                    bookmark.name.clone(),
                ));
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
                "{unplaced} OpenDoc bookmark(s) target a non-text block and could not be represented as DOCX inline bookmark ranges"
            ));
        }
    }

    fn add_rel(&mut self, rel_type: &'static str, target: &str, external: bool) -> String {
        self.next_rel += 1;
        let id = format!("rId{}", self.next_rel);
        self.rels.push(Rel {
            id: id.clone(),
            rel_type,
            target: target.to_string(),
            external,
        });
        id
    }

    // -- body --------------------------------------------------------------

    fn write_body(&mut self, document: &Document) -> String {
        let mut xml = Xml::fragment();
        for block in &document.blocks {
            self.write_block(&mut xml, document, block);
        }
        if xml.is_empty() {
            xml.empty("w:p", &[]);
        }
        xml.into_string()
    }

    fn write_block(&mut self, xml: &mut Xml, document: &Document, block: &Block) {
        match &block.kind {
            BlockKind::Paragraph => self.write_text_paragraph(xml, document, block, None, None),
            BlockKind::Title => {
                self.write_text_paragraph(xml, document, block, Some("Title"), None)
            }
            BlockKind::Subtitle => {
                self.write_text_paragraph(xml, document, block, Some("Subtitle"), None)
            }
            BlockKind::Heading { level } => {
                let clamped = (*level).clamp(1, 6);
                if clamped != *level {
                    self.warn(
                        CLAMPED_HEADING_LEVEL,
                        format!("heading level {level} was clamped to {clamped}"),
                    );
                }
                let style = format!("Heading{clamped}");
                self.write_text_paragraph(xml, document, block, Some(&style), None);
            }
            BlockKind::ListItem {
                list_id,
                level,
                kind,
            } => {
                let format = document
                    .list_properties
                    .get(list_id)
                    .map(|properties| properties.format_for(*level))
                    .unwrap_or_else(|| OrderedListFormat::inherited_at(*level));
                let bullet_marker = document
                    .list_properties
                    .get(list_id)
                    .map(|properties| properties.bullet_marker_for(*level))
                    .unwrap_or_else(|| opendoc_core::BulletListMarker::inherited_at(*level));
                let num_id = self.list_num_id(list_id, *kind, format, bullet_marker);
                self.write_text_paragraph(
                    xml,
                    document,
                    block,
                    Some("ListParagraph"),
                    Some((num_id, (*level).min(8))),
                );
            }
            BlockKind::Table {
                columns,
                properties,
                rows,
            } => {
                self.reject_block_properties(block, "table");
                self.write_table(xml, document, columns, properties, rows);
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
                    "equations are written as their source text inside an Office Math zone; Word will show the source, not a typeset formula",
                );
                xml.open("w:p", &[]);
                self.write_paragraph_properties(xml, None, None, &block.properties);
                xml.open("m:oMathPara", &[]);
                write_math(xml, &self.sanitize(&equation.source, "equation source"));
                xml.close("m:oMathPara");
                xml.close("w:p");
            }
            BlockKind::Image {
                blob_hash,
                alt_text,
                layout,
            } => {
                self.write_image_paragraph(xml, blob_hash, alt_text, layout, &block.properties);
            }
            BlockKind::HorizontalRule => {
                // Word has no `<hr>` block: its native equivalent is a
                // paragraph bottom border.  The empty paragraph is only the
                // carrier; the OpenDoc rule itself remains content-free.
                xml.open("w:p", &[]);
                xml.open("w:pPr", &[]);
                xml.open("w:pBdr", &[]);
                xml.empty(
                    "w:bottom",
                    &[
                        ("w:val", "single"),
                        ("w:sz", "6"),
                        ("w:space", "1"),
                        ("w:color", "6B7280"),
                    ],
                );
                xml.close("w:pBdr");
                xml.close("w:pPr");
                xml.close("w:p");
            }
            BlockKind::TableOfContents { max_level } => {
                // A field is deliberately preferable to cached entry text:
                // Word recomputes heading labels and page numbers after its
                // own edits, while OpenDoc's source block remains derived on
                // the next import/render. `\\h` gives Word hyperlinks and
                // `\\z` suppresses those link decorations in web layout.
                let instruction = format!(" TOC \\o \"1-{max_level}\" \\h \\z \\u ");
                xml.open("w:p", &[]);
                xml.open("w:fldSimple", &[("w:instr", &instruction)]);
                xml.open("w:r", &[]);
                xml.open("w:t", &[]);
                xml.text("Table of contents");
                xml.close("w:t");
                xml.close("w:r");
                xml.close("w:fldSimple");
                xml.close("w:p");
            }
            BlockKind::Bibliography => {
                // Word's BIBLIOGRAPHY field has no portable source-record
                // payload. Emit the current deterministic projection rather
                // than a field that would silently turn empty in Word.
                self.warn(
                    "docx-export-bibliography-as-static-text",
                    "the generated bibliography was exported as current formatted paragraphs because DOCX bibliography fields require an external source-record store",
                );
                for text in std::iter::once("Bibliography".to_string()).chain(
                    opendoc_citations::render_cited_bibliography(&document.citation_database)
                        .into_iter()
                        .map(|entry| entry.text),
                ) {
                    xml.open("w:p", &[]);
                    xml.open("w:r", &[]);
                    xml.open("w:t", &[]);
                    xml.text(&text);
                    xml.close("w:t");
                    xml.close("w:r");
                    xml.close("w:p");
                }
            }
            BlockKind::PageBreak => {
                self.reject_block_properties(block, "page break");
                xml.open("w:p", &[]);
                xml.open("w:r", &[]);
                xml.empty("w:br", &[("w:type", "page")]);
                xml.close("w:r");
                xml.close("w:p");
            }
            BlockKind::SectionBreak { .. } => {
                // This first vertical slice preserves the forced new page.
                // A later DOCX section writer will emit the section's
                // `sectPr` rather than pretending its setup is global.
                self.warn(
                    "docx-export-section-setup-unmapped",
                    "a section boundary was exported as a page break; per-section setup and furniture are retained in OpenDoc source but are not yet emitted as DOCX sectPr",
                );
                xml.open("w:p", &[]);
                xml.open("w:r", &[]);
                xml.empty("w:br", &[("w:type", "page")]);
                xml.close("w:r");
                xml.close("w:p");
            }
        }
    }

    /// A block kind whose DOCX shape has nowhere to put paragraph formatting.
    fn reject_block_properties(&mut self, block: &Block, what: &str) {
        if !block.properties.is_empty() {
            let keys: Vec<&str> = block
                .properties
                .iter()
                .map(|property| property.key().as_str())
                .collect();
            self.warn(
                DROPPED_BLOCK_PROPERTIES,
                format!(
                    "block formatting ({}) on a {what} block was dropped: WordprocessingML has no paragraph to carry it",
                    keys.join(", ")
                ),
            );
        }
    }

    fn write_text_paragraph(
        &mut self,
        xml: &mut Xml,
        document: &Document,
        block: &Block,
        style: Option<&str>,
        num: Option<(u32, u8)>,
    ) {
        xml.open("w:p", &[]);
        self.write_paragraph_properties(xml, style, num, &block.properties);
        self.write_bookmark_starts(xml, &block.id);
        for inline in &block.content {
            self.write_inline(xml, document, inline);
        }
        self.write_bookmark_ends(xml, &block.id);
        xml.close("w:p");
    }

    /// OpenDoc bookmarks identify an entire stable block.  A Word bookmark is
    /// an inline range, so use a zero-width range at the beginning of the
    /// corresponding paragraph rather than inventing a character offset.
    fn write_bookmark_starts(&mut self, xml: &mut Xml, block_id: &StableId) {
        let Some(ranges) = self.bookmark_ranges.get(block_id).cloned() else {
            return;
        };
        for (bookmark_id, number, name) in ranges {
            self.emitted_bookmarks.insert(bookmark_id);
            let number = number.to_string();
            xml.empty("w:bookmarkStart", &[("w:id", &number), ("w:name", &name)]);
        }
    }

    fn write_bookmark_ends(&mut self, xml: &mut Xml, block_id: &StableId) {
        let Some(ranges) = self.bookmark_ranges.get(block_id) else {
            return;
        };
        for (_, number, _) in ranges {
            let number = number.to_string();
            xml.empty("w:bookmarkEnd", &[("w:id", &number)]);
        }
    }

    /// `w:pPr` children are a fixed sequence in the schema: `pStyle`, `numPr`,
    /// `bidi`, `spacing`, `ind`, `jc`. Word refuses a package that reorders
    /// them, so the order here is load-bearing, not stylistic.
    fn write_paragraph_properties(
        &mut self,
        xml: &mut Xml,
        style: Option<&str>,
        num: Option<(u32, u8)>,
        properties: &BlockProperties,
    ) {
        let spacing = self.spacing_attributes(properties);
        let indent = indent_attributes(properties);
        if style.is_none() && num.is_none() && properties.is_empty() {
            return;
        }
        xml.open("w:pPr", &[]);
        if let Some(style) = style {
            xml.empty("w:pStyle", &[("w:val", style)]);
        }
        if let Some((num_id, level)) = num {
            xml.open("w:numPr", &[]);
            xml.empty("w:ilvl", &[("w:val", &level.to_string())]);
            xml.empty("w:numId", &[("w:val", &num_id.to_string())]);
            xml.close("w:numPr");
        }
        if let Some(direction) = properties.direction {
            let value = match direction {
                TextDirection::RightToLeft => "1",
                TextDirection::LeftToRight => "0",
            };
            xml.empty("w:bidi", &[("w:val", value)]);
        }
        if let Some(keep_with_next) = properties.keep_with_next {
            // An explicit false is meaningful: it cancels a style's
            // `keepNext`, whereas omitting the element inherits that style.
            xml.empty(
                "w:keepNext",
                &[("w:val", if keep_with_next { "1" } else { "0" })],
            );
        }
        if let Some(border) = properties.border {
            xml.open("w:pBdr", &[]);
            for edge in ["w:top", "w:left", "w:bottom", "w:right"] {
                if border.style() == BorderStyle::None {
                    xml.empty(edge, &[("w:val", "nil")]);
                } else {
                    let value = match border.style() {
                        BorderStyle::Solid => "single",
                        BorderStyle::Dashed => "dashed",
                        BorderStyle::Dotted => "dotted",
                        BorderStyle::Double => "double",
                        BorderStyle::None => unreachable!(),
                    };
                    let size = self.border_eighths(border.width());
                    let color = border.color().as_hex();
                    xml.empty(
                        edge,
                        &[
                            ("w:val", value),
                            ("w:sz", &size),
                            ("w:space", "0"),
                            ("w:color", &color[1..]),
                        ],
                    );
                }
            }
            xml.close("w:pBdr");
        }
        if let Some(background) = properties.background {
            let fill = background.as_hex().trim_start_matches('#').to_string();
            xml.empty(
                "w:shd",
                &[("w:val", "clear"), ("w:color", "auto"), ("w:fill", &fill)],
            );
        }
        write_attribute_element(xml, "w:spacing", &spacing);
        write_attribute_element(xml, "w:ind", &indent);
        if let Some(alignment) = properties.alignment {
            // The transitional spellings are what Word itself writes and what
            // every version reads; `w:start`/`w:end` only arrived with the
            // strict schema.
            let value = match alignment {
                Alignment::Start => "left",
                Alignment::Center => "center",
                Alignment::End => "right",
                Alignment::Justify => "both",
            };
            xml.empty("w:jc", &[("w:val", value)]);
        }
        xml.close("w:pPr");
    }

    fn spacing_attributes(&mut self, properties: &BlockProperties) -> Vec<(&'static str, String)> {
        let mut attrs: Vec<(&'static str, String)> = Vec::new();
        if let Some(before) = properties.space_before {
            attrs.push(("w:before", before.twips().to_string()));
        }
        if let Some(after) = properties.space_after {
            attrs.push(("w:after", after.twips().to_string()));
        }
        match properties.line_spacing {
            None => {}
            Some(LineSpacing::Multiple(multiple)) => {
                // The model counts thousandths of a line, `w:line` counts
                // 240ths. Exact only when the thousandths divide cleanly.
                let thousandths = i64::from(multiple.thousandths());
                let line = (thousandths * 240 + 500) / 1000;
                if thousandths * 240 % 1000 != 0 {
                    self.warn(
                        APPROXIMATED_LINE_SPACING,
                        format!(
                            "line spacing {}× is not expressible in 240ths of a line and was rounded to {}/240",
                            multiple.ratio(),
                            line
                        ),
                    );
                }
                attrs.push(("w:line", line.to_string()));
                attrs.push(("w:lineRule", "auto".to_string()));
            }
            Some(LineSpacing::Exact(height)) => {
                attrs.push(("w:line", height.twips().to_string()));
                attrs.push(("w:lineRule", "exact".to_string()));
            }
            Some(LineSpacing::AtLeast(height)) => {
                attrs.push(("w:line", height.twips().to_string()));
                attrs.push(("w:lineRule", "atLeast".to_string()));
            }
        }
        attrs
    }

    fn list_num_id(
        &mut self,
        list_id: &StableId,
        kind: ListKind,
        format: OrderedListFormat,
        bullet_marker: opendoc_core::BulletListMarker,
    ) -> u32 {
        // A Word numbering instance contains a definition for every nesting
        // level.  Ordered counter style is therefore a level property of one
        // instance, not part of its identity.  Keying it by `format` splits a
        // perfectly ordinary decimal / lower-alpha nested list into separate
        // `numId`s and loses its run identity on import.
        let flavour = match kind {
            ListKind::Ordered => ListFlavour::Ordered(OrderedListFormat::Decimal),
            // A Word numbering instance owns definitions for every nesting
            // level. Bullet glyph is likewise a level property, so it must
            // not split one run into separate numIds merely because level 1
            // inherits a hollow circle while level 0 inherits a disc.
            ListKind::Bullet => ListFlavour::Bullet(opendoc_core::BulletListMarker::Disc),
            _ => ListFlavour::of(kind, format, bullet_marker),
        };
        if matches!(kind, ListKind::Checklist { .. }) {
            self.warn(
                CHECKLIST_AS_BULLET,
                "checklist items were written as a bulleted list whose bullet is a ballot-box glyph; WordprocessingML has no checklist, so re-importing gives a bullet list and the tick state is only visible, not structural",
            );
        }
        let flavours = self.list_flavours.entry(list_id.clone()).or_default();
        flavours.insert(flavour.clone());
        if flavours.len() > 1 {
            self.warn(
                SPLIT_MIXED_LIST,
                "a single OpenDoc list run mixes markers; WordprocessingML numbers a list by definition, so it was split into one numbering per marker and re-imports as several lists",
            );
        }
        let next = self.lists.len() as u32 + 1;
        *self.lists.entry((list_id.clone(), flavour)).or_insert(next)
    }

    // -- inlines -----------------------------------------------------------

    fn write_inline(&mut self, xml: &mut Xml, document: &Document, inline: &Inline) {
        match inline {
            Inline::Text { text, marks, .. } => {
                let text = self.sanitize(text, "text");
                self.write_run(xml, &text, marks);
            }
            Inline::Link {
                text, href, marks, ..
            } => {
                let text = self.sanitize(text, "link text");
                let href = self.sanitize(href, "link target");
                let anchor = href.strip_prefix('#').map(str::to_string);
                match anchor {
                    Some(anchor) if !anchor.is_empty() => {
                        xml.open("w:hyperlink", &[("w:anchor", &anchor)]);
                    }
                    _ => {
                        let rel = self.add_rel(REL_HYPERLINK, &href, true);
                        xml.open("w:hyperlink", &[("r:id", &rel)]);
                    }
                }
                self.write_run(xml, &text, marks);
                xml.close("w:hyperlink");
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
                    "citations were written as their rendered text; the citation itself is not representable in WordprocessingML and re-imports as plain text",
                );
                let rendered = self.sanitize(&rendered, "citation");
                self.write_run(xml, &rendered, &[]);
            }
            Inline::FootnoteRef { footnote_id, .. } => {
                let (id, reference) = if let Some(id) = self.footnote_ids.get(footnote_id) {
                    (id, "w:footnoteReference")
                } else if let Some(id) = self.endnote_ids.get(footnote_id) {
                    (id, "w:endnoteReference")
                } else {
                    self.warn(
                        MISSING_FOOTNOTE,
                        format!(
                            "footnote reference to {} was dropped: the document has no such footnote",
                            footnote_id.as_str()
                        ),
                    );
                    return;
                };
                let id = id.to_string();
                xml.open("w:r", &[]);
                xml.open("w:rPr", &[]);
                xml.empty("w:rStyle", &[("w:val", "FootnoteReference")]);
                xml.close("w:rPr");
                xml.empty(reference, &[("w:id", &id)]);
                xml.close("w:r");
            }
            Inline::Mention { label, .. }
            | Inline::GooglePersonChip { label, .. }
            | Inline::GoogleRichLinkChip { label, .. } => {
                self.warn(
                    MENTION_AS_TEXT,
                    "mentions were written as plain text; WordprocessingML has no mention and they re-import as text",
                );
                let label = self.sanitize(label, "mention");
                self.write_run(xml, &label, &[]);
            }
            Inline::Dropdown {
                options,
                selected_option_id,
                ..
            } => {
                self.warn(
                    DROPDOWN_AS_TEXT,
                    "dropdowns were written as their selected text; WordprocessingML has no portable inline dropdown",
                );
                if let Some(option) = options
                    .iter()
                    .find(|option| option.id == *selected_option_id)
                {
                    let label = self.sanitize(&option.label, "dropdown option");
                    self.write_run(xml, &label, &[]);
                }
            }
            Inline::DateChip { date, .. } => {
                self.warn(
                    "date-chip-as-text",
                    "date chips were written as ISO calendar text; WordprocessingML has no portable date chip",
                );
                let date = self.sanitize(date, "date chip");
                self.write_run(xml, &date, &[]);
            }
            Inline::Equation { equation, .. } => {
                self.warn(
                    EQUATION_AS_SOURCE,
                    "equations are written as their source text inside an Office Math zone; Word will show the source, not a typeset formula",
                );
                write_math(xml, &self.sanitize(&equation.source, "equation source"));
            }
            // WordprocessingML models a page number the same way OpenDoc
            // does — as a field whose value the layout engine computes — so
            // the field stays a field rather than becoming a frozen number.
            // Word needs a cached result to show anything before it next
            // repaginates, and that cached result is the one thing here that
            // is not part of the model.
            Inline::PageNumber { field, .. } => {
                let instruction = match field {
                    PageNumberField::CurrentPage => " PAGE ",
                    PageNumberField::PageCount => " NUMPAGES ",
                };
                self.warn(
                    PAGE_NUMBER_PLACEHOLDER,
                    "a page-number field was written with a cached placeholder result; Word recomputes it, but a reader that does not evaluate fields sees the placeholder as literal text",
                );
                xml.open("w:r", &[]);
                xml.empty("w:fldChar", &[("w:fldCharType", "begin")]);
                xml.close("w:r");
                xml.open("w:r", &[]);
                xml.text_element("w:instrText", &[("xml:space", "preserve")], instruction);
                xml.close("w:r");
                xml.open("w:r", &[]);
                xml.empty("w:fldChar", &[("w:fldCharType", "separate")]);
                xml.close("w:r");
                xml.open("w:r", &[]);
                xml.text_element("w:t", &[("xml:space", "preserve")], "1");
                xml.close("w:r");
                xml.open("w:r", &[]);
                xml.empty("w:fldChar", &[("w:fldCharType", "end")]);
                xml.close("w:r");
            }
        }
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

    fn write_run(&mut self, xml: &mut Xml, text: &str, marks: &[Mark]) {
        if text.is_empty() {
            return;
        }
        let format = self.run_format(marks);
        xml.open("w:r", &[]);
        format.write(xml);
        for piece in split_run_text(text) {
            match piece {
                RunPiece::Text(value) => {
                    xml.text_element("w:t", &[("xml:space", "preserve")], value)
                }
                RunPiece::Tab => xml.empty("w:tab", &[]),
                RunPiece::Break => xml.empty("w:br", &[]),
            }
        }
        xml.close("w:r");
    }

    fn run_format(&mut self, marks: &[Mark]) -> RunFormat {
        let mut format = RunFormat::default();
        for mark in marks {
            match mark.kind {
                MarkKind::Bold => format.bold = true,
                MarkKind::Italic => format.italic = true,
                MarkKind::Underline => format.underline = true,
                MarkKind::Strike => format.strike = true,
                MarkKind::Superscript => format.vertical = Some("superscript"),
                MarkKind::Subscript => format.vertical = Some("subscript"),
                MarkKind::Code => {
                    format.code = true;
                    self.warn(
                        CODE_AS_MONOSPACE,
                        "code spans were written as a monospace character style; WordprocessingML has no code mark, so they re-import as a font mark",
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
                    Some(points) => format.size_half_points = Some(points * 2),
                    None => self.drop_mark_value("size", mark.value.as_deref()),
                },
                MarkKind::Link => self.warn(
                    DROPPED_MARK,
                    "a link mark on plain text was dropped; only a link inline becomes a WordprocessingML hyperlink",
                ),
                MarkKind::Citation => self.warn(
                    DROPPED_MARK,
                    "a citation mark was dropped; WordprocessingML has no equivalent",
                ),
            }
        }
        format
    }

    fn drop_mark_value(&mut self, what: &str, value: Option<&str>) {
        self.warn(
            DROPPED_MARK_VALUE,
            format!(
                "a {what} mark carrying {} was dropped: WordprocessingML cannot express that value",
                value
                    .map(|value| format!("\"{value}\""))
                    .unwrap_or_else(|| "no value".to_string())
            ),
        );
    }

    // -- tables ------------------------------------------------------------

    /// A table as WordprocessingML's own grid.
    ///
    /// The model's grid is rectangular and a merged cell is a span on the
    /// cell it starts at (ADR 0013); WordprocessingML says the same thing
    /// with `w:gridSpan` across a row and `w:vMerge` down a column, so the
    /// mapping is exact in both directions and the reader in `docx.rs` reads
    /// back what is written here. The one thing that does not survive is the
    /// *content* of a covered cell, which the model retains and
    /// WordprocessingML has nowhere to put — so it is named rather than
    /// dropped in silence.
    ///
    /// # No `w:tblBorders`
    ///
    /// This used to write a `single sz=4 color=auto` grid on **every** table,
    /// whatever the document said, to materialise the editor's own `.doc-table
    /// td` hairline. It made the export lie twice over. A table a producer
    /// wrote as borderless came back bordered — round-tripping the
    /// LibreOffice fixture through LibreOffice turned `fo:border="none"` into
    /// `0.5pt solid #000000` — and a cell that said `BorderStyle::None` on
    /// only one edge got the invented line back on the other three.
    ///
    /// The model has no table-level border, so there is nothing here to
    /// write: every border the document states is on a cell and is written by
    /// [`Self::write_cell_properties`], and an edge nothing states is written
    /// as nothing, which is what WordprocessingML reads as *no border*. That
    /// makes the writer the exact inverse of the reader, which resolves a
    /// `w:tblBorders` onto the cells on the way in (see `crate::docx::table`).
    ///
    /// The cost is stated rather than hidden: a table nobody has set a border
    /// on exports without one, while the editor still draws its grey
    /// gridlines on screen. Those gridlines are a view default — the same
    /// thing Word draws for a borderless table — and not a property of the
    /// document, so the export cannot carry them and does not claim to. A
    /// user who wants lines in Word sets them, with `set_table_cell_border`.
    fn write_table(
        &mut self,
        xml: &mut Xml,
        document: &Document,
        columns: &[TableColumn],
        properties: &opendoc_core::TableProperties,
        rows: &[opendoc_core::TableRow],
    ) {
        if rows
            .iter()
            .flat_map(|row| &row.cells)
            .any(|cell| cell.properties.row_header.is_some())
        {
            self.warn(
                DROPPED_TABLE_ROW_HEADER,
                "WordprocessingML has no semantic per-cell row-header role; explicit OpenDoc row-header state was not exported",
            );
        }
        let widths = column_widths(columns, rows);
        let fixed = columns.iter().any(|column| column.width.is_some());
        let total: i64 = widths.iter().sum();
        xml.open("w:tbl", &[]);
        xml.open("w:tblPr", &[]);
        if fixed {
            xml.empty("w:tblW", &[("w:w", &total.to_string()), ("w:type", "dxa")]);
        } else {
            xml.empty("w:tblW", &[("w:w", "0"), ("w:type", "auto")]);
        }
        // The model's *auto* has no `w:gridCol` spelling — the schema wants a
        // number for every column — so the layout mode is what carries the
        // difference: an autofit table's grid is a cached measurement, and
        // the reader treats it as auto for exactly that reason.
        xml.empty(
            "w:tblLayout",
            &[("w:type", if fixed { "fixed" } else { "autofit" })],
        );
        xml.empty(
            "w:tblLook",
            &[
                ("w:val", "04A0"),
                ("w:firstRow", "1"),
                ("w:lastRow", "0"),
                ("w:firstColumn", "1"),
                ("w:lastColumn", "0"),
                ("w:noHBand", "0"),
                ("w:noVBand", "1"),
            ],
        );
        if let Some(alignment) = properties.alignment {
            let value = match alignment {
                opendoc_core::TableAlignment::Start => "left",
                opendoc_core::TableAlignment::Center => "center",
                opendoc_core::TableAlignment::End => "right",
            };
            xml.empty("w:jc", &[("w:val", value)]);
        }
        if let Some(border) = properties.border {
            xml.open("w:tblBorders", &[]);
            for edge in [
                "w:top",
                "w:left",
                "w:bottom",
                "w:right",
                "w:insideH",
                "w:insideV",
            ] {
                if border.style() == BorderStyle::None {
                    xml.empty(edge, &[("w:val", "nil")]);
                } else {
                    let value = match border.style() {
                        BorderStyle::Solid => "single",
                        BorderStyle::Dashed => "dashed",
                        BorderStyle::Dotted => "dotted",
                        BorderStyle::Double => "double",
                        BorderStyle::None => unreachable!("handled above"),
                    };
                    xml.empty(
                        edge,
                        &[
                            ("w:val", value),
                            ("w:sz", &self.border_eighths(border.width())),
                            ("w:space", "0"),
                            ("w:color", &border.color().as_hex()[1..]),
                        ],
                    );
                }
            }
            xml.close("w:tblBorders");
        }
        xml.close("w:tblPr");
        xml.open("w:tblGrid", &[]);
        for width in &widths {
            xml.empty("w:gridCol", &[("w:w", &width.to_string())]);
        }
        xml.close("w:tblGrid");

        let coverage = cell_coverage(rows);
        for (row_index, row) in rows.iter().enumerate() {
            xml.open("w:tr", &[]);
            if row.height.is_some() || row.header {
                xml.open("w:trPr", &[]);
                if let Some(height) = row.height {
                    xml.empty(
                        "w:trHeight",
                        &[
                            ("w:val", &height.twips().to_string()),
                            ("w:hRule", "atLeast"),
                        ],
                    );
                }
                if row.header {
                    xml.empty("w:tblHeader", &[]);
                }
                xml.close("w:trPr");
            }
            if row.cells.is_empty() {
                self.write_cell(
                    xml,
                    document,
                    &TableCell::empty(),
                    CellPlacement {
                        width: widths.first().copied().unwrap_or(0),
                        grid_span: 1,
                        vertical_merge: None,
                        content: true,
                    },
                );
            }
            let mut column_index = 0usize;
            while column_index < row.cells.len() {
                let cell = &row.cells[column_index];
                let (grid_span, vertical_merge, content) =
                    match coverage.get(&(row_index, column_index)) {
                        None => {
                            let span = cell.span;
                            let merge = (span.rows() > 1).then_some("restart");
                            (span.columns() as usize, merge, true)
                        }
                        // The left edge of a rectangle that started higher up:
                        // this is the row's continuation cell, and it consumes
                        // as many grid columns as the rectangle is wide.
                        Some(origin) if origin.column == column_index => {
                            (origin.columns, Some("continue"), false)
                        }
                        // Swallowed by a `w:gridSpan` already written: no
                        // `w:tc` at all, so this cell's content has nowhere to
                        // go either.
                        Some(_) => {
                            self.report_covered_cell_content(cell);
                            column_index += 1;
                            continue;
                        }
                    };
                if !content {
                    self.report_covered_cell_content(cell);
                }
                let width: i64 = widths
                    .iter()
                    .skip(column_index)
                    .take(grid_span)
                    .copied()
                    .sum();
                self.write_cell(
                    xml,
                    document,
                    cell,
                    CellPlacement {
                        width,
                        grid_span,
                        vertical_merge,
                        content,
                    },
                );
                column_index += grid_span.max(1);
            }
            xml.close("w:tr");
        }
        xml.close("w:tbl");
    }

    /// A covered cell's blocks are retained by the model and invisible to
    /// every reader; WordprocessingML has no place for them at all, so
    /// exporting one is a loss the export has to name (ADR 0010).
    fn report_covered_cell_content(&mut self, cell: &opendoc_core::TableCell) {
        let has_content = cell.blocks.iter().any(|block| {
            !matches!(block.kind, BlockKind::Paragraph)
                || block.content.iter().any(|inline| match inline {
                    Inline::Text { text, .. } => !text.is_empty(),
                    _ => true,
                })
        });
        if has_content {
            self.warn(
                DROPPED_COVERED_CELL_CONTENT,
                "content held by a cell underneath a merged cell was dropped: WordprocessingML keeps no content in a covered cell",
            );
        }
    }

    fn write_cell(
        &mut self,
        xml: &mut Xml,
        document: &Document,
        cell: &opendoc_core::TableCell,
        placement: CellPlacement,
    ) {
        let CellPlacement {
            width,
            grid_span,
            vertical_merge,
            content,
        } = placement;
        xml.open("w:tc", &[]);
        // WordprocessingML fixes the order of `w:tcPr`'s children and Word
        // refuses a package that reorders them: width, span, merge, borders,
        // shading, margins, vertical alignment.
        xml.open("w:tcPr", &[]);
        xml.empty("w:tcW", &[("w:w", &width.to_string()), ("w:type", "dxa")]);
        if grid_span > 1 {
            xml.empty("w:gridSpan", &[("w:val", &grid_span.to_string())]);
        }
        if let Some(merge) = vertical_merge {
            xml.empty("w:vMerge", &[("w:val", merge)]);
        }
        self.write_cell_properties(xml, &cell.properties);
        xml.close("w:tcPr");
        let mut ends_with_paragraph = false;
        if content {
            for block in &cell.blocks {
                ends_with_paragraph = !matches!(block.kind, BlockKind::Table { .. });
                self.write_block(xml, document, block);
            }
        }
        // A table cell must end with a paragraph; Word treats a cell whose
        // last child is a table as corrupt.
        if !content || cell.blocks.is_empty() || !ends_with_paragraph {
            xml.empty("w:p", &[]);
        }
        xml.close("w:tc");
    }

    fn write_cell_properties(&mut self, xml: &mut Xml, properties: &TableCellProperties) {
        let borders = [
            ("w:top", properties.border_top),
            ("w:left", properties.border_start),
            ("w:bottom", properties.border_bottom),
            ("w:right", properties.border_end),
        ];
        if borders.iter().any(|(_, border)| border.is_some()) {
            xml.open("w:tcBorders", &[]);
            for (name, border) in borders {
                let Some(border) = border else {
                    continue;
                };
                if border.style() == BorderStyle::None {
                    xml.empty(name, &[("w:val", "nil")]);
                    continue;
                }
                xml.empty(
                    name,
                    &[
                        (
                            "w:val",
                            match border.style() {
                                BorderStyle::Solid => "single",
                                BorderStyle::Dashed => "dashed",
                                BorderStyle::Dotted => "dotted",
                                BorderStyle::Double => "double",
                                BorderStyle::None => unreachable!("handled above"),
                            },
                        ),
                        ("w:sz", &self.border_eighths(border.width())),
                        ("w:space", "0"),
                        ("w:color", &border.color().as_hex()[1..]),
                    ],
                );
            }
            xml.close("w:tcBorders");
        }
        if let Some(background) = properties.background {
            xml.empty(
                "w:shd",
                &[
                    ("w:val", "clear"),
                    ("w:color", "auto"),
                    ("w:fill", &background.as_hex()[1..]),
                ],
            );
        }
        let margins = [
            ("w:top", properties.padding_top),
            ("w:left", properties.padding_start),
            ("w:bottom", properties.padding_bottom),
            ("w:right", properties.padding_end),
        ];
        if margins.iter().any(|(_, padding)| padding.is_some()) {
            xml.open("w:tcMar", &[]);
            for (name, padding) in margins {
                let Some(padding) = padding else {
                    continue;
                };
                xml.empty(
                    name,
                    &[("w:w", &padding.twips().to_string()), ("w:type", "dxa")],
                );
            }
            xml.close("w:tcMar");
        }
        if let Some(alignment) = properties.vertical_alignment {
            xml.empty(
                "w:vAlign",
                &[(
                    "w:val",
                    match alignment {
                        VerticalAlignment::Top => "top",
                        VerticalAlignment::Middle => "center",
                        VerticalAlignment::Bottom => "bottom",
                    },
                )],
            );
        }
    }

    /// `w:sz` counts eighths of a point and the model counts twips: one
    /// eighth is exactly 2.5 twips, so an odd number of twips cannot be
    /// stated and is rounded — the only inexact step in the table mapping,
    /// and it says so.
    fn border_eighths(&mut self, width: Length) -> String {
        let twips = width.twips();
        if (twips * 8) % 20 != 0 {
            self.warn(
                APPROXIMATED_CELL_BORDER,
                format!(
                    "a {}-twip cell border was rounded to the nearest eighth of a point, which is the finest thickness WordprocessingML states",
                    twips
                ),
            );
        }
        (((twips * 8) as f64 / 20.0).round() as i64).to_string()
    }

    // -- images ------------------------------------------------------------

    fn write_image_paragraph(
        &mut self,
        xml: &mut Xml,
        blob_hash: &str,
        alt_text: &str,
        layout: &ImageLayout,
        properties: &BlockProperties,
    ) {
        // Word's margin-relative, no-wrap anchor maps exactly to the model's
        // page-content tuple. A stable block target does not: it would require
        // moving this block into another paragraph and would fabricate a
        // relationship on re-import, so retain the explicit in-flow fallback.
        let positioned = layout
            .positioned
            .as_ref()
            .filter(|positioned| matches!(positioned.anchor, PositionedImageAnchor::PageContent));
        if layout.positioned.is_some() && positioned.is_none() {
            self.warn(
                POSITIONED_IMAGE_AS_INLINE,
                "a block-anchored OpenDoc image was exported as an in-flow DOCX image; its target anchor, offsets and layer are not mapped",
            );
        }
        let alt = self.sanitize(alt_text, "image alt text");
        let Some(image) = self.images.get(blob_hash) else {
            self.warn(
                MISSING_IMAGE_BLOB,
                format!(
                    "image blob {blob_hash} was not supplied, so the image was written as its alt text"
                ),
            );
            xml.open("w:p", &[]);
            self.write_paragraph_properties(xml, None, None, properties);
            self.write_run(xml, &alt, &[]);
            xml.close("w:p");
            self.write_image_effects_fallback(layout);
            self.write_image_caption_without_image(xml, layout);
            return;
        };
        let Some(extension) = image_extension(&image.media_type, &image.bytes) else {
            self.warn(
                UNSUPPORTED_IMAGE_MEDIA_TYPE,
                format!(
                    "image blob {blob_hash} has media type {} which WordprocessingML has no part type for; it was written as its alt text",
                    image.media_type
                ),
            );
            xml.open("w:p", &[]);
            self.write_paragraph_properties(xml, None, None, properties);
            self.write_run(xml, &alt, &[]);
            xml.close("w:p");
            self.write_image_effects_fallback(layout);
            self.write_image_caption_without_image(xml, layout);
            return;
        };
        let (intrinsic_width, intrinsic_height, known) = image_extent(&image.bytes);
        if !known {
            self.warn(
                UNKNOWN_IMAGE_SIZE,
                format!(
                    "image blob {blob_hash} has no readable pixel size, so a default display size was written"
                ),
            );
        }
        let rel_id = match self.image_parts.get(blob_hash) {
            Some(rel_id) => rel_id.clone(),
            None => {
                let index = self.media.len() + 1;
                let name = format!("image{index}.{extension}");
                self.media
                    .push((format!("word/media/{name}"), image.bytes.clone()));
                self.media_extensions
                    .insert(extension.to_string(), content_type_for(extension));
                let rel_id = self.add_rel(REL_IMAGE, &format!("media/{name}"), false);
                self.image_parts
                    .insert(blob_hash.to_string(), rel_id.clone());
                rel_id
            }
        };
        self.next_doc_pr += 1;
        let doc_pr = self.next_doc_pr.to_string();
        // Word uses EMUs (635 per twip).  When just one axis is set, retain
        // the intrinsic aspect ratio, as the editor's side-resize handle does.
        let requested_width = layout.width.map(|value| i64::from(value.twips()) * 635);
        let requested_height = layout.height.map(|value| i64::from(value.twips()) * 635);
        let width_emu = requested_width.unwrap_or_else(|| {
            requested_height
                .map(|height| (height * intrinsic_width / intrinsic_height).max(1))
                .unwrap_or(intrinsic_width)
        });
        let height_emu = requested_height.unwrap_or_else(|| {
            requested_width
                .map(|width| (width * intrinsic_height / intrinsic_width).max(1))
                .unwrap_or(intrinsic_height)
        });
        let width = width_emu.to_string();
        let height = height_emu.to_string();
        let name = format!("Image {doc_pr}");
        // DrawingML requires a nonvisual object name, but that generated
        // bookkeeping name is not alternative text.  Emit `descr` only when
        // the model actually has accessible text; otherwise a later import
        // would falsely claim that "Image N" came from the source author.
        let description = (!alt.trim().is_empty()).then_some(alt.as_str());

        xml.open("w:p", &[]);
        self.write_paragraph_properties(xml, None, None, properties);
        xml.open("w:r", &[]);
        xml.open("w:drawing", &[]);
        let wrapped = positioned.is_none()
            && matches!(
                layout.effective_placement(),
                ImagePlacement::WrapStart | ImagePlacement::WrapEnd
            );
        if wrapped || positioned.is_some() {
            let clearance = layout.wrap_clearance.unwrap_or_default();
            let behind_doc = match positioned.map(|positioned| positioned.layer) {
                Some(PositionedImageLayer::BehindText) => "1",
                Some(PositionedImageLayer::InFrontOfText) | None => "0",
            };
            xml.open(
                "wp:anchor",
                &[
                    (
                        "distT",
                        &(i64::from(clearance.top.twips()) * 635).to_string(),
                    ),
                    (
                        "distB",
                        &(i64::from(clearance.bottom.twips()) * 635).to_string(),
                    ),
                    (
                        "distL",
                        &(i64::from(clearance.start.twips()) * 635).to_string(),
                    ),
                    (
                        "distR",
                        &(i64::from(clearance.end.twips()) * 635).to_string(),
                    ),
                    ("simplePos", "0"),
                    ("relativeHeight", "0"),
                    ("behindDoc", behind_doc),
                    ("locked", "0"),
                    ("layoutInCell", "1"),
                    ("allowOverlap", "1"),
                ],
            );
            xml.empty("wp:simplePos", &[("x", "0"), ("y", "0")]);
            let horizontal_relative_from = if positioned.is_some() {
                "margin"
            } else {
                "column"
            };
            xml.open(
                "wp:positionH",
                &[("relativeFrom", horizontal_relative_from)],
            );
            if let Some(positioned) = positioned {
                xml.text_element(
                    "wp:posOffset",
                    &[],
                    &(i64::from(positioned.horizontal_offset.twips()) * 635).to_string(),
                );
            } else {
                xml.text_element(
                    "wp:align",
                    &[],
                    match layout.effective_placement() {
                        ImagePlacement::WrapStart => "left",
                        ImagePlacement::WrapEnd => "right",
                        ImagePlacement::Block => {
                            unreachable!("only wrapped placements use anchors")
                        }
                    },
                );
            }
            xml.close("wp:positionH");
            let vertical_relative_from = if positioned.is_some() {
                "margin"
            } else {
                "paragraph"
            };
            xml.open("wp:positionV", &[("relativeFrom", vertical_relative_from)]);
            let vertical_offset = positioned
                .map(|positioned| (i64::from(positioned.vertical_offset.twips()) * 635).to_string())
                .unwrap_or_else(|| "0".to_string());
            xml.text_element("wp:posOffset", &[], &vertical_offset);
            xml.close("wp:positionV");
        } else {
            xml.open(
                "wp:inline",
                &[
                    ("distT", "0"),
                    ("distB", "0"),
                    ("distL", "0"),
                    ("distR", "0"),
                ],
            );
        }
        xml.empty("wp:extent", &[("cx", &width), ("cy", &height)]);
        xml.empty(
            "wp:effectExtent",
            &[("l", "0"), ("t", "0"), ("r", "0"), ("b", "0")],
        );
        // `wp:wrapSquare` precedes docPr in CT_Anchor.  Keeping it here is
        // not cosmetic: Word rejects otherwise well-formed XML with children
        // in the wrong schema order.
        if wrapped {
            xml.empty("wp:wrapSquare", &[("wrapText", "bothSides")]);
        } else if positioned.is_some() {
            xml.empty("wp:wrapNone", &[]);
        }
        let mut doc_pr_attributes = vec![("id", doc_pr.as_str()), ("name", name.as_str())];
        if let Some(description) = description {
            doc_pr_attributes.push(("descr", description));
        }
        xml.empty("wp:docPr", &doc_pr_attributes);
        xml.open("wp:cNvGraphicFramePr", &[]);
        xml.empty(
            "a:graphicFrameLocks",
            &[("xmlns:a", NS_A), ("noChangeAspect", "1")],
        );
        xml.close("wp:cNvGraphicFramePr");
        xml.open("a:graphic", &[("xmlns:a", NS_A)]);
        xml.open(
            "a:graphicData",
            &[(
                "uri",
                "http://schemas.openxmlformats.org/drawingml/2006/picture",
            )],
        );
        xml.open("pic:pic", &[("xmlns:pic", NS_PIC)]);
        xml.open("pic:nvPicPr", &[]);
        let mut picture_properties = vec![("id", "0"), ("name", name.as_str())];
        if let Some(description) = description {
            picture_properties.push(("descr", description));
        }
        xml.empty("pic:cNvPr", &picture_properties);
        xml.open("pic:cNvPicPr", &[]);
        xml.empty("a:picLocks", &[("noChangeAspect", "1")]);
        xml.close("pic:cNvPicPr");
        xml.close("pic:nvPicPr");
        xml.open("pic:blipFill", &[]);
        xml.open("a:blip", &[("r:embed", &rel_id)]);
        if let Some(opacity) = layout.opacity_percent {
            xml.empty(
                "a:alphaModFix",
                &[("amt", &(u32::from(opacity) * 1_000).to_string())],
            );
        }
        xml.close("a:blip");
        if let Some(crop) = layout.crop.filter(|crop| !crop.is_empty()) {
            xml.empty(
                "a:srcRect",
                &[
                    ("l", &(u32::from(crop.left_percent) * 1_000).to_string()),
                    ("t", &(u32::from(crop.top_percent) * 1_000).to_string()),
                    ("r", &(u32::from(crop.right_percent) * 1_000).to_string()),
                    ("b", &(u32::from(crop.bottom_percent) * 1_000).to_string()),
                ],
            );
        }
        xml.open("a:stretch", &[]);
        xml.empty("a:fillRect", &[]);
        xml.close("a:stretch");
        xml.close("pic:blipFill");
        xml.open("pic:spPr", &[]);
        let rotation = layout
            .rotation_degrees
            .filter(|degrees| *degrees != 0)
            .map(|degrees| (i32::from(degrees) * 60_000).to_string());
        let transform_attributes = rotation
            .as_deref()
            .map(|rotation| vec![("rot", rotation)])
            .unwrap_or_default();
        xml.open("a:xfrm", &transform_attributes);
        xml.empty("a:off", &[("x", "0"), ("y", "0")]);
        xml.empty("a:ext", &[("cx", &width), ("cy", &height)]);
        xml.close("a:xfrm");
        xml.open("a:prstGeom", &[("prst", "rect")]);
        xml.empty("a:avLst", &[]);
        xml.close("a:prstGeom");
        if let Some(border) = layout.border {
            if border.style() != BorderStyle::None {
                let width = (i64::from(border.width().twips()) * 635).to_string();
                xml.open("a:ln", &[("w", &width)]);
                xml.open("a:solidFill", &[]);
                xml.empty("a:srgbClr", &[("val", &border.color().as_hex()[1..])]);
                xml.close("a:solidFill");
                let dash = match border.style() {
                    BorderStyle::Solid => "solid",
                    BorderStyle::Dashed => "dash",
                    BorderStyle::Dotted => "dot",
                    BorderStyle::Double => {
                        self.warn(
                            IMAGE_DOUBLE_BORDER_AS_SOLID,
                            "an image's double border was exported as a solid DrawingML outline because the supported DOCX image-border subset has no double-stroke form",
                        );
                        "solid"
                    }
                    BorderStyle::None => unreachable!("handled above"),
                };
                xml.empty("a:prstDash", &[("val", dash)]);
                xml.close("a:ln");
            }
        }
        xml.close("pic:spPr");
        xml.close("pic:pic");
        xml.close("a:graphicData");
        xml.close("a:graphic");
        if wrapped || positioned.is_some() {
            xml.close("wp:anchor");
        } else {
            xml.close("wp:inline");
        }
        xml.close("w:drawing");
        xml.close("w:r");
        xml.close("w:p");
        if let Some(caption) = layout.caption.as_deref() {
            // Word's native caption is a following paragraph marked with the
            // built-in Caption style.  The adjacency is intentional: on
            // import we associate this exact simple shape back with the
            // preceding image rather than exposing it as unrelated prose.
            xml.open("w:p", &[]);
            xml.open("w:pPr", &[]);
            xml.empty("w:pStyle", &[("w:val", "Caption")]);
            xml.close("w:pPr");
            let caption = self.sanitize(caption, "image caption");
            self.write_run(xml, &caption, &[]);
            xml.close("w:p");
        }
    }

    /// A caption remains meaningful text even when the image bytes cannot be
    /// packaged. Keep it as Word's native following-caption shape, but name
    /// the lost attachment rather than pretending the fallback prose is an
    /// image that an importer could reattach it to.
    fn write_image_caption_without_image(&mut self, xml: &mut Xml, layout: &ImageLayout) {
        let Some(caption) = layout.caption.as_deref() else {
            return;
        };
        self.warn(
            IMAGE_CAPTION_WITHOUT_IMAGE,
            "an image caption was written as a Caption paragraph, but its image was unavailable and could not be exported",
        );
        xml.open("w:p", &[]);
        xml.open("w:pPr", &[]);
        xml.empty("w:pStyle", &[("w:val", "Caption")]);
        xml.close("w:pPr");
        let caption = self.sanitize(caption, "image caption");
        self.write_run(xml, &caption, &[]);
        xml.close("w:p");
    }

    /// The fallback has prose but no DrawingML object, so its visual image
    /// effects cannot be emitted. Keep this distinct from the missing-asset
    /// warning, which explains the absent bytes but not the lost authored
    /// presentation.
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
        // `CellBorder::none()` deliberately clears a line; unlike a drawn
        // outline it has no image-level visual effect to report as lost.
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

    // -- footnotes ---------------------------------------------------------

    fn write_footnotes(&mut self, document: &Document) -> Option<Vec<u8>> {
        self.write_notes(document, false)
    }

    fn write_endnotes(&mut self, document: &Document) -> Option<Vec<u8>> {
        self.write_notes(document, true)
    }

    fn write_notes(&mut self, document: &Document, endnote: bool) -> Option<Vec<u8>> {
        let notes: Vec<Footnote> = document
            .footnotes
            .iter()
            .filter(|note| document.endnote_ids.contains(&note.id) == endnote)
            .cloned()
            .collect();
        if notes.is_empty() {
            return None;
        }
        let (root, note, reference, separator, continuation_separator) = if endnote {
            (
                "w:endnotes",
                "w:endnote",
                "w:endnoteRef",
                "w:separator",
                "w:continuationSeparator",
            )
        } else {
            (
                "w:footnotes",
                "w:footnote",
                "w:footnoteRef",
                "w:separator",
                "w:continuationSeparator",
            )
        };
        let mut xml = Xml::part();
        xml.open(
            root,
            &[("xmlns:w", NS_W), ("xmlns:r", NS_R), ("xmlns:m", NS_M)],
        );
        for (kind, id) in [("separator", "-1"), ("continuationSeparator", "0")] {
            xml.open(note, &[("w:type", kind), ("w:id", id)]);
            xml.open("w:p", &[]);
            xml.open("w:r", &[]);
            xml.empty(
                if kind == "separator" {
                    separator
                } else {
                    continuation_separator
                },
                &[],
            );
            xml.close("w:r");
            xml.close("w:p");
            xml.close(note);
        }
        for footnote in &notes {
            self.write_note(&mut xml, document, footnote, endnote, note, reference);
        }
        xml.close(root);
        Some(xml.into_bytes())
    }

    fn write_note(
        &mut self,
        xml: &mut Xml,
        document: &Document,
        footnote: &Footnote,
        endnote: bool,
        note_element: &str,
        reference_element: &str,
    ) {
        if footnote.deleted || footnote.revision != 1 {
            self.warn(
                DROPPED_FOOTNOTE_STATE,
                "a footnote's deletion flag and revision number were dropped: WordprocessingML footnotes carry neither",
            );
        }
        let ids = if endnote {
            &self.endnote_ids
        } else {
            &self.footnote_ids
        };
        let id = ids
            .get(&footnote.id)
            .copied()
            .unwrap_or_default()
            .to_string();
        xml.open(note_element, &[("w:id", &id)]);
        xml.open("w:p", &[]);
        xml.open("w:pPr", &[]);
        xml.empty("w:pStyle", &[("w:val", "FootnoteText")]);
        xml.close("w:pPr");
        xml.open("w:r", &[]);
        xml.open("w:rPr", &[]);
        xml.empty("w:rStyle", &[("w:val", "FootnoteReference")]);
        xml.close("w:rPr");
        xml.empty(reference_element, &[]);
        xml.close("w:r");
        for inline in &footnote.body {
            if matches!(inline, Inline::FootnoteRef { .. }) {
                self.warn(
                    NESTED_FOOTNOTE_REFERENCE,
                    "a footnote referencing another footnote was dropped: WordprocessingML does not nest notes",
                );
                continue;
            }
            self.write_inline(xml, document, inline);
        }
        xml.close("w:p");
        xml.close(note_element);
    }

    // -- page furniture and section properties -----------------------------

    /// Writes one header or footer part, returning the relationship id the
    /// section properties must point at.
    fn write_furniture(
        &mut self,
        document: &Document,
        slot: HeaderFooterSlot,
    ) -> Option<(String, Vec<u8>)> {
        let blocks = document.furniture(slot);
        // A missing variant inherits ordinary furniture, but an explicitly
        // present empty variant deliberately suppresses it. Word represents
        // that distinction with an empty referenced header/footer part, so
        // do not collapse `Some(vec![])` to no relationship.
        if blocks.is_empty() && !document.has_furniture_override(slot) {
            return None;
        }
        let (root, target, rel_type) = match slot {
            HeaderFooterSlot::Header => ("w:hdr", "header1.xml", REL_HEADER),
            HeaderFooterSlot::Footer => ("w:ftr", "footer1.xml", REL_FOOTER),
            HeaderFooterSlot::FirstPageHeader => ("w:hdr", "header2.xml", REL_HEADER),
            HeaderFooterSlot::FirstPageFooter => ("w:ftr", "footer2.xml", REL_FOOTER),
            HeaderFooterSlot::EvenPageHeader => ("w:hdr", "header3.xml", REL_HEADER),
            HeaderFooterSlot::EvenPageFooter => ("w:ftr", "footer3.xml", REL_FOOTER),
        };
        let mut body = Xml::fragment();
        for block in blocks {
            self.write_block(&mut body, document, block);
        }
        let mut xml = Xml::part();
        xml.open(
            root,
            &[
                ("xmlns:w", NS_W),
                ("xmlns:r", NS_R),
                ("xmlns:m", NS_M),
                ("xmlns:wp", NS_WP),
                ("xmlns:a", NS_A),
                ("xmlns:pic", NS_PIC),
            ],
        );
        if body.is_empty() {
            xml.empty("w:p", &[]);
        } else {
            xml.raw(&body.into_string());
        }
        xml.close(root);
        let rel = self.add_rel(rel_type, target, false);
        Some((rel, xml.into_bytes()))
    }

    /// `w:sectPr` carries the page geometry. Every dimension is twips on both
    /// sides, so the mapping is the identity — no conversion, no drift.
    ///
    /// `margin_start`/`margin_end` are logical and `w:left`/`w:right` are
    /// physical. OpenDoc has no document-level writing direction — direction
    /// is a block property — so the leading edge is taken to be the left one,
    /// which is what every left-to-right document means by it.
    fn section_properties(
        &mut self,
        document: &Document,
        furniture: FurnitureReferences<'_>,
    ) -> String {
        let setup = &document.page_setup;
        let mut xml = Xml::fragment();
        xml.open("w:sectPr", &[]);
        if let Some(header) = furniture.header {
            xml.empty(
                "w:headerReference",
                &[("w:type", "default"), ("r:id", header)],
            );
        }
        if let Some(footer) = furniture.footer {
            xml.empty(
                "w:footerReference",
                &[("w:type", "default"), ("r:id", footer)],
            );
        }
        if let Some(header) = furniture.first_page_header {
            xml.empty(
                "w:headerReference",
                &[("w:type", "first"), ("r:id", header)],
            );
        }
        if let Some(footer) = furniture.first_page_footer {
            xml.empty(
                "w:footerReference",
                &[("w:type", "first"), ("r:id", footer)],
            );
        }
        if let Some(header) = furniture.even_page_header {
            xml.empty("w:headerReference", &[("w:type", "even"), ("r:id", header)]);
        }
        if let Some(footer) = furniture.even_page_footer {
            xml.empty("w:footerReference", &[("w:type", "even"), ("r:id", footer)]);
        }
        if furniture.first_page_header.is_some() || furniture.first_page_footer.is_some() {
            xml.empty("w:titlePg", &[]);
        }
        let width = setup.width.twips().to_string();
        let height = setup.height.twips().to_string();
        let mut page_size: Vec<(&str, &str)> = vec![("w:w", &width), ("w:h", &height)];
        if setup.orientation() == PageOrientation::Landscape {
            page_size.push(("w:orient", "landscape"));
        }
        xml.empty("w:pgSz", &page_size);
        let top = setup.margin_top.twips().to_string();
        let right = setup.margin_end.twips().to_string();
        let bottom = setup.margin_bottom.twips().to_string();
        let left = setup.margin_start.twips().to_string();
        let header_margin = setup.margin_header.twips().to_string();
        let footer_margin = setup.margin_footer.twips().to_string();
        xml.empty(
            "w:pgMar",
            &[
                ("w:top", &top),
                ("w:right", &right),
                ("w:bottom", &bottom),
                ("w:left", &left),
                ("w:header", &header_margin),
                ("w:footer", &footer_margin),
                ("w:gutter", "0"),
            ],
        );
        xml.empty("w:cols", &[("w:space", "720")]);
        xml.empty("w:docGrid", &[("w:linePitch", "360")]);
        xml.close("w:sectPr");
        xml.into_string()
    }

    // -- package parts -----------------------------------------------------

    fn content_types(&self, parts: ContentTypeParts) -> Vec<u8> {
        let mut xml = Xml::part();
        xml.open("Types", &[("xmlns", NS_CONTENT_TYPES)]);
        xml.empty(
            "Default",
            &[("Extension", "rels"), ("ContentType", CT_RELATIONSHIPS)],
        );
        xml.empty(
            "Default",
            &[("Extension", "xml"), ("ContentType", "application/xml")],
        );
        for (extension, content_type) in &self.media_extensions {
            xml.empty(
                "Default",
                &[("Extension", extension), ("ContentType", content_type)],
            );
        }
        let mut overrides = vec![
            ("/word/document.xml", CT_DOCUMENT),
            ("/word/styles.xml", CT_STYLES),
            ("/docProps/core.xml", CT_CORE_PROPERTIES),
        ];
        if parts.lists {
            overrides.push(("/word/numbering.xml", CT_NUMBERING));
        }
        if parts.footnotes {
            overrides.push(("/word/footnotes.xml", CT_FOOTNOTES));
        }
        if parts.endnotes {
            overrides.push(("/word/endnotes.xml", CT_ENDNOTES));
        }
        if parts.header {
            overrides.push(("/word/header1.xml", CT_HEADER));
        }
        if parts.footer {
            overrides.push(("/word/footer1.xml", CT_FOOTER));
        }
        if parts.first_page_header {
            overrides.push(("/word/header2.xml", CT_HEADER));
        }
        if parts.first_page_footer {
            overrides.push(("/word/footer2.xml", CT_FOOTER));
        }
        if parts.even_page_header {
            overrides.push(("/word/header3.xml", CT_HEADER));
        }
        if parts.even_page_footer {
            overrides.push(("/word/footer3.xml", CT_FOOTER));
        }
        if parts.settings {
            overrides.push(("/word/settings.xml", CT_SETTINGS));
        }
        overrides.sort_unstable();
        for (part, content_type) in overrides {
            xml.empty(
                "Override",
                &[("PartName", part), ("ContentType", content_type)],
            );
        }
        xml.close("Types");
        xml.into_bytes()
    }

    fn document_rels(&self) -> Vec<u8> {
        let mut xml = Xml::part();
        xml.open("Relationships", &[("xmlns", NS_PACKAGE_RELS)]);
        for rel in &self.rels {
            let mut attrs: Vec<(&str, &str)> = vec![
                ("Id", rel.id.as_str()),
                ("Type", rel.rel_type),
                ("Target", rel.target.as_str()),
            ];
            if rel.external {
                attrs.push(("TargetMode", "External"));
            }
            xml.empty("Relationship", &attrs);
        }
        xml.close("Relationships");
        xml.into_bytes()
    }

    fn numbering_part(&self) -> Vec<u8> {
        let mut xml = Xml::part();
        xml.open("w:numbering", &[("xmlns:w", NS_W), ("xmlns:r", NS_R)]);
        let mut definitions: Vec<(u32, &StableId, ListFlavour)> = self
            .lists
            .iter()
            .map(|((list_id, flavour), num_id)| (*num_id, list_id, flavour.clone()))
            .collect();
        definitions.sort_unstable();
        for (num_id, list_id, flavour) in &definitions {
            xml.open("w:abstractNum", &[("w:abstractNumId", &num_id.to_string())]);
            xml.empty("w:multiLevelType", &[("w:val", "hybridMultilevel")]);
            for level in 0..9u8 {
                let level_flavour = match flavour {
                    ListFlavour::Ordered(_) => ListFlavour::Ordered(
                        self.list_properties
                            .get(*list_id)
                            .map(|properties| properties.format_for(level))
                            .unwrap_or_else(|| OrderedListFormat::inherited_at(level)),
                    ),
                    ListFlavour::Bullet(_) => ListFlavour::Bullet(
                        self.list_properties
                            .get(*list_id)
                            .map(|properties| properties.bullet_marker_for(level))
                            .unwrap_or_else(|| opendoc_core::BulletListMarker::inherited_at(level)),
                    ),
                    other => other.clone(),
                };
                let (format, bullet) = level_flavour.level_format();
                xml.open("w:lvl", &[("w:ilvl", &level.to_string())]);
                let start = matches!(flavour, ListFlavour::Ordered(_))
                    .then(|| {
                        self.list_properties
                            .get(*list_id)
                            .map(|properties| properties.start_for(level))
                            .unwrap_or(1)
                    })
                    .unwrap_or(1);
                xml.empty("w:start", &[("w:val", &start.to_string())]);
                xml.empty("w:numFmt", &[("w:val", format)]);
                let text = match bullet {
                    Some(bullet) => bullet.to_string(),
                    None => format!("%{}.", level + 1),
                };
                xml.empty("w:lvlText", &[("w:val", &text)]);
                xml.empty("w:lvlJc", &[("w:val", "left")]);
                xml.open("w:pPr", &[]);
                xml.empty(
                    "w:ind",
                    &[
                        ("w:left", &(720 * (i32::from(level) + 1)).to_string()),
                        ("w:hanging", "360"),
                    ],
                );
                xml.close("w:pPr");
                if bullet.is_some() {
                    xml.open("w:rPr", &[]);
                    xml.empty(
                        "w:rFonts",
                        &[
                            ("w:ascii", "Segoe UI Symbol"),
                            ("w:hAnsi", "Segoe UI Symbol"),
                            ("w:hint", "default"),
                        ],
                    );
                    xml.close("w:rPr");
                }
                xml.close("w:lvl");
            }
            xml.close("w:abstractNum");
        }
        for (num_id, _, _) in &definitions {
            xml.open("w:num", &[("w:numId", &num_id.to_string())]);
            xml.empty("w:abstractNumId", &[("w:val", &num_id.to_string())]);
            xml.close("w:num");
        }
        xml.close("w:numbering");
        xml.into_bytes()
    }
}

// ---------------------------------------------------------------------------
// Run formatting
// ---------------------------------------------------------------------------

#[derive(Default)]
struct RunFormat {
    code: bool,
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
    vertical: Option<&'static str>,
    color: Option<String>,
    background: Option<String>,
    font: Option<String>,
    size_half_points: Option<u32>,
}

impl RunFormat {
    fn is_empty(&self) -> bool {
        !self.code
            && !self.bold
            && !self.italic
            && !self.underline
            && !self.strike
            && self.vertical.is_none()
            && self.color.is_none()
            && self.background.is_none()
            && self.font.is_none()
            && self.size_half_points.is_none()
    }

    /// `w:rPr` children are a fixed sequence in the schema; this is that order.
    fn write(&self, xml: &mut Xml) {
        if self.is_empty() {
            return;
        }
        xml.open("w:rPr", &[]);
        if self.code {
            xml.empty("w:rStyle", &[("w:val", "Code")]);
        }
        let font = self
            .font
            .clone()
            .or_else(|| self.code.then(|| "Consolas".to_string()));
        if let Some(font) = font {
            xml.empty(
                "w:rFonts",
                &[("w:ascii", &font), ("w:hAnsi", &font), ("w:cs", &font)],
            );
        }
        if self.bold {
            xml.empty("w:b", &[]);
            xml.empty("w:bCs", &[]);
        }
        if self.italic {
            xml.empty("w:i", &[]);
            xml.empty("w:iCs", &[]);
        }
        if self.strike {
            xml.empty("w:strike", &[]);
        }
        if let Some(color) = &self.color {
            xml.empty("w:color", &[("w:val", color)]);
        }
        if let Some(size) = self.size_half_points {
            xml.empty("w:sz", &[("w:val", &size.to_string())]);
            xml.empty("w:szCs", &[("w:val", &size.to_string())]);
        }
        if self.underline {
            xml.empty("w:u", &[("w:val", "single")]);
        }
        if let Some(background) = &self.background {
            xml.empty(
                "w:shd",
                &[
                    ("w:val", "clear"),
                    ("w:color", "auto"),
                    ("w:fill", background),
                ],
            );
        }
        if let Some(vertical) = self.vertical {
            xml.empty("w:vertAlign", &[("w:val", vertical)]);
        }
        xml.close("w:rPr");
    }
}

enum RunPiece<'a> {
    Text(&'a str),
    Tab,
    Break,
}

/// Tabs and newlines are their own WordprocessingML elements, not text.
fn split_run_text(text: &str) -> Vec<RunPiece<'_>> {
    let mut pieces = Vec::new();
    let mut start = 0;
    for (index, ch) in text.char_indices() {
        let piece = match ch {
            '\t' => RunPiece::Tab,
            '\n' => RunPiece::Break,
            '\r' => RunPiece::Break,
            _ => continue,
        };
        if index > start {
            pieces.push(RunPiece::Text(&text[start..index]));
        }
        // A CRLF pair is one line break, not two.
        if ch == '\n' && text[..index].ends_with('\r') {
            pieces.pop();
        } else {
            pieces.push(piece);
        }
        start = index + ch.len_utf8();
    }
    if start < text.len() {
        pieces.push(RunPiece::Text(&text[start..]));
    }
    pieces
}

/// The nominal text column of a Letter page at OpenDoc's default margins:
/// what an auto-width column shares out, and what a table with no widths at
/// all has always been given.
const DEFAULT_TABLE_WIDTH_TWIPS: i64 = 9360;

/// One `w:gridCol` width per column, in twips.
///
/// A column the model sized is written exactly — both formats count twips, so
/// there is no conversion and no rounding. An *auto* column has no
/// WordprocessingML spelling, so it is given an equal share of whatever the
/// sized columns leave; `w:tblLayout` is what tells the reader that share was
/// invented here rather than authored.
fn column_widths(columns: &[TableColumn], rows: &[TableRow]) -> Vec<i64> {
    let count = columns
        .len()
        .max(rows.iter().map(|row| row.cells.len()).max().unwrap_or(0))
        .max(1);
    let explicit: i64 = columns
        .iter()
        .filter_map(|column| column.width)
        .map(|width| i64::from(width.twips()))
        .sum();
    let auto_count = count.saturating_sub(columns.iter().filter(|c| c.width.is_some()).count());
    let auto_width = if auto_count == 0 {
        0
    } else {
        ((DEFAULT_TABLE_WIDTH_TWIPS - explicit).max(0) / auto_count as i64)
            .max(i64::from(TableColumn::MIN_WIDTH_TWIPS))
    };
    (0..count)
        .map(
            |index| match columns.get(index).and_then(|column| column.width) {
                Some(width) => i64::from(width.twips()),
                None => auto_width,
            },
        )
        .collect()
}

/// Where one `w:tc` sits in the grid: how wide it is, how many grid columns
/// it swallows, whether it continues a merge from the row above, and whether
/// it is the cell that carries the content.
#[derive(Clone, Copy)]
struct CellPlacement {
    width: i64,
    grid_span: usize,
    vertical_merge: Option<&'static str>,
    content: bool,
}

/// Where a merged cell starts, for every grid position it covers.
///
/// Derived from the spans exactly as [`opendoc_core::table_covered_positions`]
/// is, but keeping the origin's column and width: writing a row needs to know
/// whether a covered position is the *left edge* of the rectangle — which
/// becomes a `w:vMerge` continuation cell — or a position a `w:gridSpan` has
/// already swallowed, which becomes nothing at all.
#[derive(Clone, Copy)]
struct MergeOrigin {
    column: usize,
    columns: usize,
}

fn cell_coverage(rows: &[TableRow]) -> BTreeMap<(usize, usize), MergeOrigin> {
    let mut coverage = BTreeMap::new();
    for (row_index, row) in rows.iter().enumerate() {
        for (column_index, cell) in row.cells.iter().enumerate() {
            if cell.span.is_single() {
                continue;
            }
            let origin = MergeOrigin {
                column: column_index,
                columns: cell.span.columns() as usize,
            };
            for covered_row in row_index..row_index + cell.span.rows() as usize {
                for covered_column in column_index..column_index + origin.columns {
                    if (covered_row, covered_column) != (row_index, column_index) {
                        coverage.insert((covered_row, covered_column), origin);
                    }
                }
            }
        }
    }
    coverage
}

fn hex_value(value: Option<&str>) -> Option<String> {
    let value = value?.trim().trim_start_matches('#');
    if value.len() == 6 && value.chars().all(|ch| ch.is_ascii_hexdigit()) {
        Some(value.to_ascii_uppercase())
    } else {
        None
    }
}

fn write_attribute_element(xml: &mut Xml, name: &str, attrs: &[(&'static str, String)]) {
    if attrs.is_empty() {
        return;
    }
    let borrowed: Vec<(&str, &str)> = attrs
        .iter()
        .map(|(name, value)| (*name, value.as_str()))
        .collect();
    xml.empty(name, &borrowed);
}

/// `w:hanging` is the negative direction of the model's first-line indent, and
/// the two are mutually exclusive in `w:ind`.
fn indent_attributes(properties: &BlockProperties) -> Vec<(&'static str, String)> {
    let mut attrs: Vec<(&'static str, String)> = Vec::new();
    if let Some(start) = properties.indent_start {
        attrs.push(("w:left", start.twips().to_string()));
    }
    if let Some(end) = properties.indent_end {
        attrs.push(("w:right", end.twips().to_string()));
    }
    match properties.indent_first_line {
        None => {}
        Some(first_line) if first_line.is_negative() => {
            attrs.push(("w:hanging", (-first_line.twips()).to_string()));
        }
        Some(first_line) => attrs.push(("w:firstLine", first_line.twips().to_string())),
    }
    attrs
}

fn write_math(xml: &mut Xml, source: &str) {
    xml.open("m:oMath", &[]);
    xml.open("m:r", &[]);
    xml.text_element("m:t", &[("xml:space", "preserve")], source);
    xml.close("m:r");
    xml.close("m:oMath");
}

// ---------------------------------------------------------------------------
// Static parts
// ---------------------------------------------------------------------------

fn document_part(body: &str) -> Vec<u8> {
    let mut xml = Xml::part();
    xml.open(
        "w:document",
        &[
            ("xmlns:w", NS_W),
            ("xmlns:r", NS_R),
            ("xmlns:m", NS_M),
            ("xmlns:wp", NS_WP),
            ("xmlns:a", NS_A),
            ("xmlns:pic", NS_PIC),
        ],
    );
    xml.open("w:body", &[]);
    xml.raw(body);
    xml.close("w:body");
    xml.close("w:document");
    xml.into_bytes()
}

/// Word only applies `w:type="even"` header/footer references when this
/// document setting is present.  Emit it exactly when an OpenDoc even-page
/// override exists; omitting it would serialize a named variant that Word
/// silently never displays.
fn settings_part(even_and_odd_headers: bool) -> Vec<u8> {
    let mut xml = Xml::part();
    xml.open("w:settings", &[("xmlns:w", NS_W)]);
    if even_and_odd_headers {
        xml.empty("w:evenAndOddHeaders", &[]);
    }
    xml.close("w:settings");
    xml.into_bytes()
}

fn root_rels() -> Vec<u8> {
    let mut xml = Xml::part();
    xml.open("Relationships", &[("xmlns", NS_PACKAGE_RELS)]);
    xml.empty(
        "Relationship",
        &[
            ("Id", "rId1"),
            ("Type", REL_OFFICE_DOCUMENT),
            ("Target", "word/document.xml"),
        ],
    );
    xml.empty(
        "Relationship",
        &[
            ("Id", "rId2"),
            ("Type", REL_CORE_PROPERTIES),
            ("Target", "docProps/core.xml"),
        ],
    );
    xml.close("Relationships");
    xml.into_bytes()
}

/// The only place the document title survives the trip: `w:document` has no
/// title, and the reader takes the title from the file name.
fn core_properties(document: &Document) -> Vec<u8> {
    let mut xml = Xml::part();
    xml.open(
        "cp:coreProperties",
        &[
            (
                "xmlns:cp",
                "http://schemas.openxmlformats.org/package/2006/metadata/core-properties",
            ),
            ("xmlns:dc", "http://purl.org/dc/elements/1.1/"),
            ("xmlns:dcterms", "http://purl.org/dc/terms/"),
            ("xmlns:xsi", "http://www.w3.org/2001/XMLSchema-instance"),
        ],
    );
    xml.text_element("dc:title", &[], &document.title);
    xml.text_element("dc:language", &[], &document.locale);
    xml.close("cp:coreProperties");
    xml.into_bytes()
}

fn styles_part(document: &Document) -> Vec<u8> {
    let mut xml = Xml::part();
    xml.open("w:styles", &[("xmlns:w", NS_W)]);
    xml.open("w:docDefaults", &[]);
    xml.open("w:rPrDefault", &[]);
    xml.open("w:rPr", &[]);
    xml.empty(
        "w:rFonts",
        &[
            ("w:ascii", "Calibri"),
            ("w:hAnsi", "Calibri"),
            ("w:cs", "Calibri"),
        ],
    );
    xml.empty("w:sz", &[("w:val", "22")]);
    xml.empty("w:szCs", &[("w:val", "22")]);
    xml.empty("w:lang", &[("w:val", &document.locale)]);
    xml.close("w:rPr");
    xml.close("w:rPrDefault");
    xml.open("w:pPrDefault", &[]);
    xml.close("w:pPrDefault");
    xml.close("w:docDefaults");

    xml.open(
        "w:style",
        &[
            ("w:type", "paragraph"),
            ("w:default", "1"),
            ("w:styleId", "Normal"),
        ],
    );
    xml.empty("w:name", &[("w:val", "Normal")]);
    xml.empty("w:qFormat", &[]);
    xml.close("w:style");

    for (id, name, size, color) in [
        ("Title", "Title", "52", None),
        ("Subtitle", "Subtitle", "30", Some("666666")),
    ] {
        xml.open("w:style", &[("w:type", "paragraph"), ("w:styleId", id)]);
        xml.empty("w:name", &[("w:val", name)]);
        xml.empty("w:basedOn", &[("w:val", "Normal")]);
        xml.empty("w:qFormat", &[]);
        xml.open("w:rPr", &[]);
        xml.empty("w:sz", &[("w:val", size)]);
        xml.empty("w:szCs", &[("w:val", size)]);
        if let Some(color) = color {
            xml.empty("w:color", &[("w:val", color)]);
        }
        xml.close("w:rPr");
        xml.close("w:style");
    }

    for level in 1..=6u8 {
        let id = format!("Heading{level}");
        let size = HEADING_SIZES_HALF_POINTS[usize::from(level) - 1].to_string();
        xml.open("w:style", &[("w:type", "paragraph"), ("w:styleId", &id)]);
        xml.empty("w:name", &[("w:val", &format!("heading {level}"))]);
        xml.empty("w:basedOn", &[("w:val", "Normal")]);
        xml.empty("w:qFormat", &[]);
        xml.open("w:pPr", &[]);
        xml.empty("w:outlineLvl", &[("w:val", &(level - 1).to_string())]);
        xml.close("w:pPr");
        xml.open("w:rPr", &[]);
        xml.empty("w:b", &[]);
        xml.empty("w:bCs", &[]);
        xml.empty("w:sz", &[("w:val", &size)]);
        xml.empty("w:szCs", &[("w:val", &size)]);
        xml.close("w:rPr");
        xml.close("w:style");
    }

    xml.open(
        "w:style",
        &[("w:type", "paragraph"), ("w:styleId", "ListParagraph")],
    );
    xml.empty("w:name", &[("w:val", "List Paragraph")]);
    xml.empty("w:basedOn", &[("w:val", "Normal")]);
    xml.empty("w:qFormat", &[]);
    xml.open("w:pPr", &[]);
    xml.empty("w:contextualSpacing", &[]);
    xml.close("w:pPr");
    xml.close("w:style");

    xml.open(
        "w:style",
        &[("w:type", "paragraph"), ("w:styleId", "FootnoteText")],
    );
    xml.empty("w:name", &[("w:val", "footnote text")]);
    xml.empty("w:basedOn", &[("w:val", "Normal")]);
    xml.close("w:style");

    xml.open(
        "w:style",
        &[("w:type", "character"), ("w:styleId", "FootnoteReference")],
    );
    xml.empty("w:name", &[("w:val", "footnote reference")]);
    xml.open("w:rPr", &[]);
    xml.empty("w:vertAlign", &[("w:val", "superscript")]);
    xml.close("w:rPr");
    xml.close("w:style");

    xml.open("w:style", &[("w:type", "character"), ("w:styleId", "Code")]);
    xml.empty("w:name", &[("w:val", "Code")]);
    xml.open("w:rPr", &[]);
    xml.empty(
        "w:rFonts",
        &[
            ("w:ascii", "Consolas"),
            ("w:hAnsi", "Consolas"),
            ("w:cs", "Consolas"),
        ],
    );
    xml.close("w:rPr");
    xml.close("w:style");

    xml.close("w:styles");
    xml.into_bytes()
}

// ---------------------------------------------------------------------------
// Images
// ---------------------------------------------------------------------------

fn image_extension(media_type: &str, bytes: &[u8]) -> Option<&'static str> {
    // MIME tokens are case-insensitive and parameters describe the source
    // representation, not the file kind a package part must declare.  Keep
    // the original declaration on the blob; this is only the package-name
    // projection, matching raw-image save and PDF export.
    let essence = media_type
        .split_once(';')
        .map_or(media_type, |(essence, _)| essence)
        .trim()
        .to_ascii_lowercase();
    let by_media_type = match essence.as_str() {
        "image/png" => Some("png"),
        "image/jpeg" | "image/jpg" => Some("jpeg"),
        "image/gif" => Some("gif"),
        "image/bmp" => Some("bmp"),
        "image/tiff" => Some("tiff"),
        "image/svg+xml" => Some("svg"),
        "image/webp" => Some("webp"),
        _ => None,
    };
    by_media_type.or_else(|| sniff_image_extension(bytes))
}

fn sniff_image_extension(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Some("jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("gif")
    } else if bytes.starts_with(b"BM") {
        Some("bmp")
    } else {
        None
    }
}

fn content_type_for(extension: &str) -> String {
    match extension {
        "png" => "image/png",
        "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "tiff" => "image/tiff",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        _ => "application/octet-stream",
    }
    .to_string()
}

/// The model stores no image size, so the display size comes from the pixel
/// dimensions in the file read at 96dpi, scaled down to fit the text column.
/// Returns `(width, height, size_was_known)`.
fn image_extent(bytes: &[u8]) -> (i64, i64, bool) {
    let Some((pixel_width, pixel_height)) = image_pixels(bytes) else {
        return (EMU_PER_INCH * 4, EMU_PER_INCH * 3, false);
    };
    let mut width = i64::from(pixel_width) * EMU_PER_PIXEL;
    let mut height = i64::from(pixel_height) * EMU_PER_PIXEL;
    if width > MAX_IMAGE_WIDTH_EMU {
        height = height * MAX_IMAGE_WIDTH_EMU / width;
        width = MAX_IMAGE_WIDTH_EMU;
    }
    (width.max(1), height.max(1), true)
}

fn image_pixels(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") && bytes.len() >= 24 {
        let width = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
        let height = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
        return (width > 0 && height > 0).then_some((width, height));
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        if bytes.len() < 10 {
            return None;
        }
        let width = u32::from(u16::from_le_bytes([bytes[6], bytes[7]]));
        let height = u32::from(u16::from_le_bytes([bytes[8], bytes[9]]));
        return (width > 0 && height > 0).then_some((width, height));
    }
    if bytes.starts_with(b"\xff\xd8\xff") {
        return jpeg_pixels(bytes);
    }
    None
}

fn jpeg_pixels(bytes: &[u8]) -> Option<(u32, u32)> {
    let mut index = 2;
    while index + 9 < bytes.len() {
        if bytes[index] != 0xff {
            index += 1;
            continue;
        }
        let marker = bytes[index + 1];
        // Start-of-frame markers carry the dimensions; SOF4/SOF8/SOF12 are
        // not frame headers.
        if (0xc0..=0xcf).contains(&marker) && !matches!(marker, 0xc4 | 0xc8 | 0xcc) {
            let height = u32::from(u16::from_be_bytes([bytes[index + 5], bytes[index + 6]]));
            let width = u32::from(u16::from_be_bytes([bytes[index + 7], bytes[index + 8]]));
            return (width > 0 && height > 0).then_some((width, height));
        }
        let length = u16::from_be_bytes([bytes[index + 2], bytes[index + 3]]) as usize;
        if length < 2 {
            return None;
        }
        index += 2 + length;
    }
    None
}

// ---------------------------------------------------------------------------
// Packaging
// ---------------------------------------------------------------------------

fn zip_parts(parts: &[(String, Vec<u8>)]) -> Result<Vec<u8>, ImportError> {
    // `zip::ZipWriter` stores its selected Rust deflater inline. Its state is
    // large enough to overflow an ordinary 2 MiB test-thread stack merely by
    // starting the next DOCX part, even though all package data is already
    // owned by this Vec. DOCX permits ZIP's `stored` method, so write the
    // small deterministic subset we need directly: local headers, bytes, a
    // central directory and the end record. This keeps packages portable and
    // makes the in-memory writer stack-bounded on native and wasm builds.
    const LOCAL_FILE_HEADER: u32 = 0x0403_4b50;
    const CENTRAL_DIRECTORY_HEADER: u32 = 0x0201_4b50;
    const END_OF_CENTRAL_DIRECTORY: u32 = 0x0605_4b50;
    const VERSION_NEEDED: u16 = 20;
    const UTF8_NAMES: u16 = 1 << 11;
    // DOS date values encode the day as one-based, so zero is invalid rather
    // than 1980-01-01. Keep the same fixed stamp zip's writer chose.
    const DOS_EPOCH_DATE: u16 = 0x0021;

    struct Entry<'a> {
        name: &'a [u8],
        crc32: u32,
        size: u32,
        local_offset: u32,
    }

    let mut package = Vec::new();
    let mut entries = Vec::with_capacity(parts.len());
    let mut names = BTreeSet::new();
    for (name, bytes) in parts {
        let name_bytes = name.as_bytes();
        if name_bytes.is_empty()
            || u16::try_from(name_bytes.len()).is_err()
            || !names.insert(name.as_str())
        {
            return Err(ImportError::InvalidDocument(format!(
                "DOCX part {name:?} has an invalid or duplicate ZIP path"
            )));
        }
        let size = u32::try_from(bytes.len()).map_err(|_| {
            ImportError::InvalidDocument(format!("DOCX part {name} exceeds ZIP's 4 GiB limit"))
        })?;
        let local_offset = u32::try_from(package.len()).map_err(|_| {
            ImportError::InvalidDocument("DOCX package exceeds ZIP's 4 GiB limit".to_string())
        })?;
        push_zip_u32(&mut package, LOCAL_FILE_HEADER);
        push_zip_u16(&mut package, VERSION_NEEDED);
        push_zip_u16(&mut package, UTF8_NAMES);
        push_zip_u16(&mut package, 0); // stored; no deflater or data descriptor
        push_zip_u16(&mut package, 0); // 00:00, deterministic
        push_zip_u16(&mut package, DOS_EPOCH_DATE);
        push_zip_u32(&mut package, zip_crc32(bytes));
        push_zip_u32(&mut package, size);
        push_zip_u32(&mut package, size);
        push_zip_u16(&mut package, name_bytes.len() as u16);
        push_zip_u16(&mut package, 0);
        package.extend_from_slice(name_bytes);
        package.extend_from_slice(bytes);
        entries.push(Entry {
            name: name_bytes,
            crc32: zip_crc32(bytes),
            size,
            local_offset,
        });
    }

    let central_offset = u32::try_from(package.len()).map_err(|_| {
        ImportError::InvalidDocument("DOCX package exceeds ZIP's 4 GiB limit".to_string())
    })?;
    for entry in &entries {
        push_zip_u32(&mut package, CENTRAL_DIRECTORY_HEADER);
        push_zip_u16(&mut package, VERSION_NEEDED);
        push_zip_u16(&mut package, VERSION_NEEDED);
        push_zip_u16(&mut package, UTF8_NAMES);
        push_zip_u16(&mut package, 0);
        push_zip_u16(&mut package, 0);
        push_zip_u16(&mut package, DOS_EPOCH_DATE);
        push_zip_u32(&mut package, entry.crc32);
        push_zip_u32(&mut package, entry.size);
        push_zip_u32(&mut package, entry.size);
        push_zip_u16(&mut package, entry.name.len() as u16);
        push_zip_u16(&mut package, 0);
        push_zip_u16(&mut package, 0);
        push_zip_u16(&mut package, 0);
        push_zip_u16(&mut package, 0);
        push_zip_u32(&mut package, 0);
        push_zip_u32(&mut package, entry.local_offset);
        package.extend_from_slice(entry.name);
    }
    let central_size = u32::try_from(package.len())
        .ok()
        .and_then(|end| end.checked_sub(central_offset))
        .ok_or_else(|| {
            ImportError::InvalidDocument("DOCX package exceeds ZIP's 4 GiB limit".to_string())
        })?;
    let count = u16::try_from(entries.len()).map_err(|_| {
        ImportError::InvalidDocument("DOCX package has more than 65,535 parts".to_string())
    })?;
    push_zip_u32(&mut package, END_OF_CENTRAL_DIRECTORY);
    push_zip_u16(&mut package, 0);
    push_zip_u16(&mut package, 0);
    push_zip_u16(&mut package, count);
    push_zip_u16(&mut package, count);
    push_zip_u32(&mut package, central_size);
    push_zip_u32(&mut package, central_offset);
    push_zip_u16(&mut package, 0);
    Ok(package)
}

fn push_zip_u16(output: &mut Vec<u8>, value: u16) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn push_zip_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_le_bytes());
}

/// ZIP's standard IEEE CRC-32. The package writer handles only bounded,
/// caller-owned part bytes; a direct table-free implementation keeps that
/// boundary dependency-free and avoids a second compression/runtime backend.
fn zip_crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & u32::wrapping_neg(crc & 1));
        }
    }
    !crc
}
