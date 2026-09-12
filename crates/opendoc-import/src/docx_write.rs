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

use crate::ImportError;
use opendoc_core::{
    Alignment, Block, BlockKind, BlockProperties, Document, Footnote, HeaderFooterSlot, Inline,
    LineSpacing, ListKind, Mark, MarkKind, ModelWarning, PageNumberField, PageOrientation,
    StableId, TextDirection,
};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Write};

// ---------------------------------------------------------------------------
// Public payloads
// ---------------------------------------------------------------------------

/// The bytes behind a [`BlockKind::Image`], supplied by the caller because the
/// model stores only the content hash.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocxImage {
    pub media_type: String,
    pub bytes: Vec<u8>,
}

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
const EQUATION_AS_SOURCE: &str = "docx-export-equation-as-source";
const DROPPED_EQUATION_CONTENT: &str = "docx-export-dropped-equation-content";
const MISSING_IMAGE_BLOB: &str = "docx-export-missing-image-blob";
const UNSUPPORTED_IMAGE_MEDIA_TYPE: &str = "docx-export-unsupported-image-media-type";
const UNKNOWN_IMAGE_SIZE: &str = "docx-export-unknown-image-size";
const DROPPED_CONTROL_CHARACTER: &str = "docx-export-dropped-control-character";
const DROPPED_COMMENTS: &str = "docx-export-dropped-comments";
const DROPPED_SUGGESTIONS: &str = "docx-export-dropped-suggestions";
const DROPPED_CITATION_DATABASE: &str = "docx-export-dropped-citation-database";
const DROPPED_DOI: &str = "docx-export-dropped-doi";
const DROPPED_FOOTNOTE_STATE: &str = "docx-export-dropped-footnote-state";
const NESTED_FOOTNOTE_REFERENCE: &str = "docx-export-nested-footnote-reference";
const MISSING_FOOTNOTE: &str = "docx-export-missing-footnote";
const CLAMPED_HEADING_LEVEL: &str = "docx-export-clamped-heading-level";
const PAGE_NUMBER_PLACEHOLDER: &str = "docx-export-page-number-placeholder";

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
const REL_HEADER: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/header";
const REL_FOOTER: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer";
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
const CT_HEADER: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml";
const CT_FOOTER: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml";
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
// XML writing
// ---------------------------------------------------------------------------

/// A minimal, correctly escaping XML serializer.
///
/// WordprocessingML is written by position: element order inside `w:pPr`,
/// `w:rPr` and `w:tblPr` is fixed by the schema and Word rejects a package
/// that gets it wrong. Building the text directly keeps that order visible in
/// the code that writes it.
struct Xml {
    out: String,
}

impl Xml {
    fn part() -> Self {
        Self {
            out: String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n"),
        }
    }

    fn fragment() -> Self {
        Self { out: String::new() }
    }

    fn open(&mut self, name: &str, attrs: &[(&str, &str)]) {
        self.out.push('<');
        self.out.push_str(name);
        self.push_attrs(attrs);
        self.out.push('>');
    }

    fn empty(&mut self, name: &str, attrs: &[(&str, &str)]) {
        self.out.push('<');
        self.out.push_str(name);
        self.push_attrs(attrs);
        self.out.push_str("/>");
    }

    fn close(&mut self, name: &str) {
        self.out.push_str("</");
        self.out.push_str(name);
        self.out.push('>');
    }

    fn text(&mut self, value: &str) {
        escape_into(&mut self.out, value, false);
    }

    fn text_element(&mut self, name: &str, attrs: &[(&str, &str)], value: &str) {
        self.open(name, attrs);
        self.text(value);
        self.close(name);
    }

    fn raw(&mut self, fragment: &str) {
        self.out.push_str(fragment);
    }

    fn push_attrs(&mut self, attrs: &[(&str, &str)]) {
        for (name, value) in attrs {
            self.out.push(' ');
            self.out.push_str(name);
            self.out.push_str("=\"");
            escape_into(&mut self.out, value, true);
            self.out.push('"');
        }
    }

    fn is_empty(&self) -> bool {
        self.out.is_empty()
    }

    fn into_bytes(self) -> Vec<u8> {
        self.out.into_bytes()
    }

    fn into_string(self) -> String {
        self.out
    }
}

fn escape_into(out: &mut String, value: &str, attribute: bool) {
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if attribute => out.push_str("&quot;"),
            '\t' if attribute => out.push_str("&#9;"),
            '\n' if attribute => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            _ => out.push(ch),
        }
    }
}

/// XML 1.0 forbids most C0 controls outright — a document holding one cannot
/// be written at all, so the character is removed and named rather than
/// producing a package no reader will open.
fn is_writable_xml_char(ch: char) -> bool {
    match ch {
        '\t' | '\n' | '\r' => true,
        ch if (ch as u32) < 0x20 => false,
        '\u{fffe}' | '\u{ffff}' => false,
        ch => !('\u{fdd0}'..='\u{fdef}').contains(&ch),
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub(crate) fn export_docx_bytes(
    document: &Document,
    images: &BTreeMap<String, DocxImage>,
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

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum ListFlavour {
    Bullet,
    Ordered,
    Unchecked,
    Checked,
}

impl ListFlavour {
    fn of(kind: ListKind) -> Self {
        match kind {
            ListKind::Bullet => ListFlavour::Bullet,
            ListKind::Ordered => ListFlavour::Ordered,
            ListKind::Checklist { checked: false } => ListFlavour::Unchecked,
            ListKind::Checklist { checked: true } => ListFlavour::Checked,
        }
    }

    /// `w:numFmt` plus the `w:lvlText` used at every level.
    fn level_format(self) -> (&'static str, Option<&'static str>) {
        match self {
            ListFlavour::Bullet => ("bullet", Some("\u{2022}")),
            ListFlavour::Ordered => ("decimal", None),
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
    images: &'a BTreeMap<String, DocxImage>,
    warnings: Vec<ModelWarning>,
    rels: Vec<Rel>,
    media: Vec<(String, Vec<u8>)>,
    media_extensions: BTreeMap<String, String>,
    image_parts: BTreeMap<String, String>,
    lists: BTreeMap<(StableId, ListFlavour), u32>,
    list_flavours: BTreeMap<StableId, BTreeSet<ListFlavour>>,
    footnote_ids: BTreeMap<StableId, u32>,
    next_rel: u32,
    next_doc_pr: u32,
}

impl<'a> Exporter<'a> {
    fn new(images: &'a BTreeMap<String, DocxImage>) -> Self {
        Self {
            images,
            warnings: Vec::new(),
            rels: Vec::new(),
            media: Vec::new(),
            media_extensions: BTreeMap::new(),
            image_parts: BTreeMap::new(),
            lists: BTreeMap::new(),
            list_flavours: BTreeMap::new(),
            footnote_ids: BTreeMap::new(),
            next_rel: 0,
            next_doc_pr: 0,
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
        for (index, footnote) in document.footnotes.iter().enumerate() {
            self.footnote_ids
                .insert(footnote.id.clone(), index as u32 + 1);
        }

        // The body is written first so that relationships, numbering
        // definitions and media parts are all discovered before the parts that
        // list them are serialized.
        let mut body = self.write_body(document);
        let header = self.write_furniture(document, HeaderFooterSlot::Header);
        let footer = self.write_furniture(document, HeaderFooterSlot::Footer);
        body.push_str(&self.section_properties(
            document,
            header.as_ref().map(|(id, _)| id.as_str()),
            footer.as_ref().map(|(id, _)| id.as_str()),
        ));
        let footnotes = self.write_footnotes(document);

        let mut parts: Vec<(String, Vec<u8>)> = Vec::new();
        let has_lists = !self.lists.is_empty();
        let has_footnotes = footnotes.is_some();

        self.add_rel(REL_STYLES, "styles.xml", false);
        if has_lists {
            self.add_rel(REL_NUMBERING, "numbering.xml", false);
        }
        if has_footnotes {
            self.add_rel(REL_FOOTNOTES, "footnotes.xml", false);
        }

        parts.push((
            "[Content_Types].xml".to_string(),
            self.content_types(has_lists, has_footnotes, header.is_some(), footer.is_some()),
        ));
        parts.push(("_rels/.rels".to_string(), root_rels()));
        parts.push(("docProps/core.xml".to_string(), core_properties(document)));
        parts.push(("word/document.xml".to_string(), document_part(&body)));
        parts.push((
            "word/_rels/document.xml.rels".to_string(),
            self.document_rels(),
        ));
        parts.push(("word/styles.xml".to_string(), styles_part(document)));
        if has_lists {
            parts.push(("word/numbering.xml".to_string(), self.numbering_part()));
        }
        if let Some(footnotes) = footnotes {
            parts.push(("word/footnotes.xml".to_string(), footnotes));
        }
        if let Some((_, bytes)) = header {
            parts.push(("word/header1.xml".to_string(), bytes));
        }
        if let Some((_, bytes)) = footer {
            parts.push(("word/footer1.xml".to_string(), bytes));
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
                let num_id = self.list_num_id(list_id, *kind);
                self.write_text_paragraph(
                    xml,
                    document,
                    block,
                    Some("ListParagraph"),
                    Some((num_id, (*level).min(8))),
                );
            }
            BlockKind::Table { rows, .. } => {
                self.reject_block_properties(block, "table");
                self.write_table(xml, document, rows);
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
                ..
            } => {
                self.reject_block_properties(block, "image");
                self.write_image_paragraph(xml, blob_hash, alt_text);
            }
            BlockKind::PageBreak => {
                self.reject_block_properties(block, "page break");
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
        for inline in &block.content {
            self.write_inline(xml, document, inline);
        }
        xml.close("w:p");
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

    fn list_num_id(&mut self, list_id: &StableId, kind: ListKind) -> u32 {
        let flavour = ListFlavour::of(kind);
        if matches!(kind, ListKind::Checklist { .. }) {
            self.warn(
                CHECKLIST_AS_BULLET,
                "checklist items were written as a bulleted list whose bullet is a ballot-box glyph; WordprocessingML has no checklist, so re-importing gives a bullet list and the tick state is only visible, not structural",
            );
        }
        let flavours = self.list_flavours.entry(list_id.clone()).or_default();
        flavours.insert(flavour);
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
            Inline::FootnoteRef { footnote_id, .. } => match self.footnote_ids.get(footnote_id) {
                Some(id) => {
                    let id = id.to_string();
                    xml.open("w:r", &[]);
                    xml.open("w:rPr", &[]);
                    xml.empty("w:rStyle", &[("w:val", "FootnoteReference")]);
                    xml.close("w:rPr");
                    xml.empty("w:footnoteReference", &[("w:id", &id)]);
                    xml.close("w:r");
                }
                None => self.warn(
                    MISSING_FOOTNOTE,
                    format!(
                        "footnote reference to {} was dropped: the document has no such footnote",
                        footnote_id.as_str()
                    ),
                ),
            },
            Inline::Mention { label, .. } => {
                self.warn(
                    MENTION_AS_TEXT,
                    "mentions were written as plain text; WordprocessingML has no mention and they re-import as text",
                );
                let label = self.sanitize(label, "mention");
                self.write_run(xml, &label, &[]);
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

    fn write_table(&mut self, xml: &mut Xml, document: &Document, rows: &[opendoc_core::TableRow]) {
        let columns = rows
            .iter()
            .map(|row| row.cells.len())
            .max()
            .unwrap_or(1)
            .max(1);
        let width = 9360 / columns as i64;
        xml.open("w:tbl", &[]);
        xml.open("w:tblPr", &[]);
        xml.empty("w:tblW", &[("w:w", "0"), ("w:type", "auto")]);
        xml.open("w:tblBorders", &[]);
        for edge in [
            "w:top",
            "w:left",
            "w:bottom",
            "w:right",
            "w:insideH",
            "w:insideV",
        ] {
            xml.empty(
                edge,
                &[
                    ("w:val", "single"),
                    ("w:sz", "4"),
                    ("w:space", "0"),
                    ("w:color", "auto"),
                ],
            );
        }
        xml.close("w:tblBorders");
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
        xml.close("w:tblPr");
        xml.open("w:tblGrid", &[]);
        for _ in 0..columns {
            xml.empty("w:gridCol", &[("w:w", &width.to_string())]);
        }
        xml.close("w:tblGrid");
        for row in rows {
            xml.open("w:tr", &[]);
            if row.cells.is_empty() {
                self.write_cell(xml, document, &[], width);
            }
            for cell in &row.cells {
                self.write_cell(xml, document, &cell.blocks, width);
            }
            xml.close("w:tr");
        }
        xml.close("w:tbl");
    }

    fn write_cell(&mut self, xml: &mut Xml, document: &Document, blocks: &[Block], width: i64) {
        xml.open("w:tc", &[]);
        xml.open("w:tcPr", &[]);
        xml.empty("w:tcW", &[("w:w", &width.to_string()), ("w:type", "dxa")]);
        xml.close("w:tcPr");
        let mut ends_with_paragraph = false;
        for block in blocks {
            ends_with_paragraph = !matches!(block.kind, BlockKind::Table { .. });
            self.write_block(xml, document, block);
        }
        // A table cell must end with a paragraph; Word treats a cell whose
        // last child is a table as corrupt.
        if blocks.is_empty() || !ends_with_paragraph {
            xml.empty("w:p", &[]);
        }
        xml.close("w:tc");
    }

    // -- images ------------------------------------------------------------

    fn write_image_paragraph(&mut self, xml: &mut Xml, blob_hash: &str, alt_text: &str) {
        let alt = self.sanitize(alt_text, "image alt text");
        let Some(image) = self.images.get(blob_hash) else {
            self.warn(
                MISSING_IMAGE_BLOB,
                format!(
                    "image blob {blob_hash} was not supplied, so the image was written as its alt text"
                ),
            );
            xml.open("w:p", &[]);
            self.write_run(xml, &alt, &[]);
            xml.close("w:p");
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
            self.write_run(xml, &alt, &[]);
            xml.close("w:p");
            return;
        };
        let (width_emu, height_emu, known) = image_extent(&image.bytes);
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
        let width = width_emu.to_string();
        let height = height_emu.to_string();
        let name = format!("Image {doc_pr}");
        let descr = if alt.trim().is_empty() {
            name.clone()
        } else {
            alt
        };

        xml.open("w:p", &[]);
        xml.open("w:r", &[]);
        xml.open("w:drawing", &[]);
        xml.open(
            "wp:inline",
            &[
                ("distT", "0"),
                ("distB", "0"),
                ("distL", "0"),
                ("distR", "0"),
            ],
        );
        xml.empty("wp:extent", &[("cx", &width), ("cy", &height)]);
        xml.empty(
            "wp:effectExtent",
            &[("l", "0"), ("t", "0"), ("r", "0"), ("b", "0")],
        );
        xml.empty(
            "wp:docPr",
            &[("id", &doc_pr), ("name", &name), ("descr", &descr)],
        );
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
        xml.empty(
            "pic:cNvPr",
            &[("id", "0"), ("name", &name), ("descr", &descr)],
        );
        xml.open("pic:cNvPicPr", &[]);
        xml.empty("a:picLocks", &[("noChangeAspect", "1")]);
        xml.close("pic:cNvPicPr");
        xml.close("pic:nvPicPr");
        xml.open("pic:blipFill", &[]);
        xml.empty("a:blip", &[("r:embed", &rel_id)]);
        xml.open("a:stretch", &[]);
        xml.empty("a:fillRect", &[]);
        xml.close("a:stretch");
        xml.close("pic:blipFill");
        xml.open("pic:spPr", &[]);
        xml.open("a:xfrm", &[]);
        xml.empty("a:off", &[("x", "0"), ("y", "0")]);
        xml.empty("a:ext", &[("cx", &width), ("cy", &height)]);
        xml.close("a:xfrm");
        xml.open("a:prstGeom", &[("prst", "rect")]);
        xml.empty("a:avLst", &[]);
        xml.close("a:prstGeom");
        xml.close("pic:spPr");
        xml.close("pic:pic");
        xml.close("a:graphicData");
        xml.close("a:graphic");
        xml.close("wp:inline");
        xml.close("w:drawing");
        xml.close("w:r");
        xml.close("w:p");
    }

    // -- footnotes ---------------------------------------------------------

    fn write_footnotes(&mut self, document: &Document) -> Option<Vec<u8>> {
        if document.footnotes.is_empty() {
            return None;
        }
        let mut xml = Xml::part();
        xml.open(
            "w:footnotes",
            &[("xmlns:w", NS_W), ("xmlns:r", NS_R), ("xmlns:m", NS_M)],
        );
        for (kind, id) in [("separator", "-1"), ("continuationSeparator", "0")] {
            xml.open("w:footnote", &[("w:type", kind), ("w:id", id)]);
            xml.open("w:p", &[]);
            xml.open("w:r", &[]);
            xml.empty(
                if kind == "separator" {
                    "w:separator"
                } else {
                    "w:continuationSeparator"
                },
                &[],
            );
            xml.close("w:r");
            xml.close("w:p");
            xml.close("w:footnote");
        }
        let footnotes = document.footnotes.clone();
        for footnote in &footnotes {
            self.write_footnote(&mut xml, document, footnote);
        }
        xml.close("w:footnotes");
        Some(xml.into_bytes())
    }

    fn write_footnote(&mut self, xml: &mut Xml, document: &Document, footnote: &Footnote) {
        if footnote.deleted || footnote.revision != 1 {
            self.warn(
                DROPPED_FOOTNOTE_STATE,
                "a footnote's deletion flag and revision number were dropped: WordprocessingML footnotes carry neither",
            );
        }
        let id = self
            .footnote_ids
            .get(&footnote.id)
            .copied()
            .unwrap_or_default()
            .to_string();
        xml.open("w:footnote", &[("w:id", &id)]);
        xml.open("w:p", &[]);
        xml.open("w:pPr", &[]);
        xml.empty("w:pStyle", &[("w:val", "FootnoteText")]);
        xml.close("w:pPr");
        xml.open("w:r", &[]);
        xml.open("w:rPr", &[]);
        xml.empty("w:rStyle", &[("w:val", "FootnoteReference")]);
        xml.close("w:rPr");
        xml.empty("w:footnoteRef", &[]);
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
        xml.close("w:footnote");
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
        if blocks.is_empty() {
            return None;
        }
        let (root, target, rel_type) = match slot {
            HeaderFooterSlot::Header => ("w:hdr", "header1.xml", REL_HEADER),
            HeaderFooterSlot::Footer => ("w:ftr", "footer1.xml", REL_FOOTER),
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
        header: Option<&str>,
        footer: Option<&str>,
    ) -> String {
        let setup = &document.page_setup;
        let mut xml = Xml::fragment();
        xml.open("w:sectPr", &[]);
        if let Some(header) = header {
            xml.empty(
                "w:headerReference",
                &[("w:type", "default"), ("r:id", header)],
            );
        }
        if let Some(footer) = footer {
            xml.empty(
                "w:footerReference",
                &[("w:type", "default"), ("r:id", footer)],
            );
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

    fn content_types(
        &self,
        has_lists: bool,
        has_footnotes: bool,
        has_header: bool,
        has_footer: bool,
    ) -> Vec<u8> {
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
        if has_lists {
            overrides.push(("/word/numbering.xml", CT_NUMBERING));
        }
        if has_footnotes {
            overrides.push(("/word/footnotes.xml", CT_FOOTNOTES));
        }
        if has_header {
            overrides.push(("/word/header1.xml", CT_HEADER));
        }
        if has_footer {
            overrides.push(("/word/footer1.xml", CT_FOOTER));
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
        let mut definitions: Vec<(u32, ListFlavour)> = self
            .lists
            .iter()
            .map(|((_, flavour), num_id)| (*num_id, *flavour))
            .collect();
        definitions.sort_unstable();
        for (num_id, flavour) in &definitions {
            let (format, bullet) = flavour.level_format();
            xml.open("w:abstractNum", &[("w:abstractNumId", &num_id.to_string())]);
            xml.empty("w:multiLevelType", &[("w:val", "hybridMultilevel")]);
            for level in 0..9u8 {
                xml.open("w:lvl", &[("w:ilvl", &level.to_string())]);
                xml.empty("w:start", &[("w:val", "1")]);
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
        for (num_id, _) in &definitions {
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

    for level in 1..=6u8 {
        let id = format!("Heading{level}");
        let size = HEADING_SIZES_HALF_POINTS[usize::from(level) - 1].to_string();
        xml.open("w:style", &[("w:type", "paragraph"), ("w:styleId", &id)]);
        xml.empty("w:name", &[("w:val", &format!("heading {level}"))]);
        xml.empty("w:basedOn", &[("w:val", "Normal")]);
        xml.empty("w:qFormat", &[]);
        xml.open("w:pPr", &[]);
        xml.empty("w:keepNext", &[]);
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
    let by_media_type = match media_type.trim().to_ascii_lowercase().as_str() {
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
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    // `SimpleFileOptions::default()` stamps 1980-01-01 because the `time`
    // feature is off, so the writer never reads a clock — the same export runs
    // byte-for-byte identically, and nothing traps on wasm32.
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, bytes) in parts {
        writer
            .start_file(name.as_str(), options)
            .map_err(|err| ImportError::InvalidDocument(format!("DOCX part {name}: {err}")))?;
        writer
            .write_all(bytes)
            .map_err(|err| ImportError::InvalidDocument(format!("DOCX part {name}: {err}")))?;
    }
    let cursor = writer
        .finish()
        .map_err(|err| ImportError::InvalidDocument(format!("DOCX package: {err}")))?;
    Ok(cursor.into_inner())
}
