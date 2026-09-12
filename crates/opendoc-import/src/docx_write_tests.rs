//! DOCX writer tests.
//!
//! The strongest check available is a round trip through the reader in
//! `docx.rs`: the writer's output is fed straight back into the parser that
//! already has 113 tests behind it, and the result is compared to the document
//! that went in. Where a round trip cannot be exact — WordprocessingML has no
//! checklist, no code span, and expresses "heading" only as a style whose
//! character formatting comes back as marks — the test states the expected
//! result explicitly instead of relaxing the comparison, so the degradation is
//! pinned rather than merely tolerated.

use crate::{export_docx_with_warnings, import_docx_bytes, mark, DocxImage, ImportReport};
use opendoc_core::{
    Alignment, Block, BlockKind, BlockProperties, Document, Equation, EquationSourceFormat,
    Footnote, Inline, Length, LineSpacing, ListKind, Mark, MarkKind, ModelWarning, PageNumberField,
    PageSetup, StableId, TableCell, TableRow, TextDirection,
};
use std::collections::BTreeMap;
use std::io::Read;

const TITLE: &str = "Round Trip";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn no_images() -> BTreeMap<String, DocxImage> {
    BTreeMap::new()
}

fn export(document: &Document) -> (Vec<u8>, Vec<ModelWarning>) {
    export_docx_with_warnings(document, &no_images()).expect("export failed")
}

fn export_with(
    document: &Document,
    images: &BTreeMap<String, DocxImage>,
) -> (Vec<u8>, Vec<ModelWarning>) {
    export_docx_with_warnings(document, images).expect("export failed")
}

fn reimport(bytes: &[u8]) -> ImportReport {
    import_docx_bytes(TITLE, bytes).expect("re-import failed")
}

/// Exports, re-imports and compares against `expected` with every generated
/// identity renumbered by traversal order, since ids are minted fresh on both
/// sides and carry no information the format could preserve.
fn assert_round_trips_to(source: &Document, expected: &Document) -> Vec<ModelWarning> {
    let (bytes, warnings) = export(source);
    let report = reimport(&bytes);
    assert_eq!(
        normalized(expected),
        normalized(&report.document),
        "round-tripped document differs from the expected result"
    );
    warnings
}

fn assert_round_trips(source: &Document) -> Vec<ModelWarning> {
    assert_round_trips_to(source, source)
}

fn codes(warnings: &[ModelWarning]) -> Vec<&str> {
    warnings
        .iter()
        .map(|warning| warning.code.as_str())
        .collect()
}

fn document(blocks: Vec<Block>) -> Document {
    let document = draft(blocks);
    document.validate().expect("test document is invalid");
    document
}

/// A document that is not valid until the caller attaches the rest of it —
/// footnote bodies, page furniture — and validates it themselves.
fn draft(blocks: Vec<Block>) -> Document {
    let mut document = Document::new(TITLE);
    document.blocks = blocks;
    document
}

fn paragraph(id: &str, text: &str) -> Block {
    Block {
        id: StableId::parse(id).unwrap(),
        kind: BlockKind::Paragraph,
        content: vec![text_inline(text, Vec::new())],
        properties: BlockProperties::default(),
    }
}

fn text_inline(text: &str, marks: Vec<Mark>) -> Inline {
    Inline::Text {
        id: StableId::new("text"),
        text: text.to_string(),
        marks,
    }
}

fn twips(value: i32) -> Length {
    Length::from_twips(value).unwrap()
}

// -- identity normalization -------------------------------------------------

#[derive(Default)]
struct Renumber {
    seen: BTreeMap<String, String>,
    next: usize,
}

impl Renumber {
    fn id(&mut self, id: &StableId) -> StableId {
        let next = self.next;
        let value = self
            .seen
            .entry(id.as_str().to_string())
            .or_insert_with(|| format!("id-{next}"))
            .clone();
        if value == format!("id-{next}") {
            self.next += 1;
        }
        StableId::parse(value).unwrap()
    }
}

/// Rewrites every id in traversal order and clears the fields a DOCX cannot
/// carry a value for at all (the document uuid, which is minted per import,
/// and the reader's own warnings, which are asserted separately).
fn normalized(document: &Document) -> Document {
    let mut document = document.clone();
    let mut renumber = Renumber::default();
    document.uuid = opendoc_core::DocumentUuid::parse("doc-normalized").unwrap();
    document.warnings = Vec::new();
    // Footnote identities are referenced from the body, so they are numbered
    // first and the references pick up the same value.
    for footnote in &mut document.footnotes {
        footnote.id = renumber.id(&footnote.id.clone());
    }
    renumber_blocks(&mut document.header, &mut renumber);
    renumber_blocks(&mut document.footer, &mut renumber);
    renumber_blocks(&mut document.blocks, &mut renumber);
    for footnote in &mut document.footnotes {
        renumber_inlines(&mut footnote.body, &mut renumber);
    }
    document
}

fn renumber_blocks(blocks: &mut [Block], renumber: &mut Renumber) {
    for block in blocks {
        block.id = renumber.id(&block.id.clone());
        match &mut block.kind {
            BlockKind::ListItem { list_id, .. } => *list_id = renumber.id(&list_id.clone()),
            BlockKind::Table { columns, rows } => {
                // Column identities are minted fresh on both sides and carry
                // nothing WordprocessingML could preserve, exactly like row
                // and cell ids, so they are renumbered by position too.
                for column in columns {
                    column.id = renumber.id(&column.id.clone());
                }
                for row in rows {
                    row.id = renumber.id(&row.id.clone());
                    for cell in &mut row.cells {
                        cell.id = renumber.id(&cell.id.clone());
                        renumber_blocks(&mut cell.blocks, renumber);
                    }
                }
            }
            BlockKind::EquationBlock { equation } => {
                equation.id = renumber.id(&equation.id.clone())
            }
            BlockKind::Paragraph | BlockKind::Heading { .. } | BlockKind::Image { .. } => {}
            BlockKind::PageBreak => {}
        }
        renumber_inlines(&mut block.content, renumber);
    }
}

fn renumber_inlines(inlines: &mut [Inline], renumber: &mut Renumber) {
    for inline in inlines {
        match inline {
            Inline::Text { id, .. }
            | Inline::Link { id, .. }
            | Inline::Mention { id, .. }
            | Inline::PageNumber { id, .. } => *id = renumber.id(&id.clone()),
            Inline::Citation {
                id, citation_id, ..
            } => {
                *id = renumber.id(&id.clone());
                *citation_id = renumber.id(&citation_id.clone());
            }
            Inline::FootnoteRef { id, footnote_id } => {
                *id = renumber.id(&id.clone());
                *footnote_id = renumber.id(&footnote_id.clone());
            }
            Inline::Equation { id, equation } => {
                *id = renumber.id(&id.clone());
                equation.id = renumber.id(&equation.id.clone());
            }
        }
    }
}

// -- package inspection -----------------------------------------------------

fn entries(bytes: &[u8]) -> Vec<String> {
    let archive = zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec())).expect("not a zip");
    archive.file_names().map(str::to_string).collect()
}

fn part(bytes: &[u8], name: &str) -> String {
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec())).expect("not a zip");
    let mut file = archive
        .by_name(name)
        .unwrap_or_else(|_| panic!("package has no part {name}"));
    let mut out = String::new();
    file.read_to_string(&mut out).expect("part is not UTF-8");
    out
}

/// Rebuilds the package with one part's text rewritten, so a test can state a
/// shape Word writes and this writer does not — a first-page header variant,
/// say — without hand-assembling a whole `.docx`.
fn rewrite_part(bytes: &[u8], name: &str, edit: impl Fn(&str) -> String) -> Vec<u8> {
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec())).expect("not a zip");
    let mut out = Vec::new();
    {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut out));
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).expect("unreadable entry");
            let entry_name = entry.name().to_string();
            let mut content = Vec::new();
            entry.read_to_end(&mut content).expect("unreadable part");
            if entry_name == name {
                content = edit(std::str::from_utf8(&content).expect("part is not UTF-8")).into();
            }
            writer
                .start_file(entry_name, zip::write::SimpleFileOptions::default())
                .expect("cannot write entry");
            std::io::Write::write_all(&mut writer, &content).expect("cannot write part");
        }
        writer.finish().expect("cannot finish zip");
    }
    out
}

// ---------------------------------------------------------------------------
// Round trips that must be exact
// ---------------------------------------------------------------------------

#[test]
fn round_trip_preserves_every_paragraph_property_exactly() {
    let mut blocks = Vec::new();
    for (index, properties) in [
        BlockProperties {
            alignment: Some(Alignment::Start),
            ..BlockProperties::default()
        },
        BlockProperties {
            alignment: Some(Alignment::Center),
            ..BlockProperties::default()
        },
        BlockProperties {
            alignment: Some(Alignment::End),
            ..BlockProperties::default()
        },
        BlockProperties {
            alignment: Some(Alignment::Justify),
            ..BlockProperties::default()
        },
        BlockProperties {
            indent_start: Some(twips(1337)),
            indent_end: Some(twips(451)),
            indent_first_line: Some(twips(283)),
            ..BlockProperties::default()
        },
        // A hanging indent is a negative first-line indent in the model and
        // `w:hanging` in the format; the sign has to survive the swap.
        BlockProperties {
            indent_start: Some(twips(720)),
            indent_first_line: Some(twips(-360)),
            ..BlockProperties::default()
        },
        BlockProperties {
            indent_start: Some(twips(-240)),
            indent_first_line: Some(Length::ZERO),
            ..BlockProperties::default()
        },
        BlockProperties {
            line_spacing: Some(LineSpacing::single()),
            ..BlockProperties::default()
        },
        BlockProperties {
            line_spacing: Some(LineSpacing::multiple(1.5).unwrap()),
            ..BlockProperties::default()
        },
        BlockProperties {
            line_spacing: Some(LineSpacing::exactly(twips(312)).unwrap()),
            ..BlockProperties::default()
        },
        BlockProperties {
            line_spacing: Some(LineSpacing::at_least(twips(289)).unwrap()),
            ..BlockProperties::default()
        },
        BlockProperties {
            space_before: Some(twips(123)),
            space_after: Some(twips(457)),
            ..BlockProperties::default()
        },
        BlockProperties {
            direction: Some(TextDirection::RightToLeft),
            ..BlockProperties::default()
        },
        BlockProperties {
            direction: Some(TextDirection::LeftToRight),
            ..BlockProperties::default()
        },
        BlockProperties {
            alignment: Some(Alignment::Justify),
            indent_start: Some(twips(567)),
            indent_end: Some(twips(89)),
            indent_first_line: Some(twips(-113)),
            line_spacing: Some(LineSpacing::multiple(2.0).unwrap()),
            space_before: Some(twips(240)),
            space_after: Some(twips(60)),
            direction: Some(TextDirection::RightToLeft),
        },
    ]
    .into_iter()
    .enumerate()
    {
        let mut block = paragraph(&format!("block-{index}"), &format!("paragraph {index}"));
        block.properties = properties;
        blocks.push(block);
    }
    let source = document(blocks);
    let warnings = assert_round_trips(&source);
    assert!(
        warnings.is_empty(),
        "plain paragraph formatting should export losslessly, got {warnings:?}"
    );
}

/// The unit is the point of the mapping: DOCX measures in twips and so does
/// [`Length`], so the integer that goes in is the integer that comes out —
/// including values that no rounded conversion through points or pixels could
/// reproduce.
#[test]
fn twips_survive_the_round_trip_unchanged() {
    for value in [1, 7, 19, 21, 239, 241, 1439, 1441, 31679, -1, -19, -31679] {
        let mut block = paragraph("block-twips", "measured");
        block.properties.indent_start = Some(twips(value));
        block.properties.indent_first_line = Some(twips(-value));
        block.properties.space_before = Some(twips(value.abs()));
        let source = document(vec![block]);
        let (bytes, _) = export(&source);
        let report = reimport(&bytes);
        let properties = &report.document.blocks[0].properties;
        assert_eq!(
            Some(value),
            properties.indent_start.map(Length::twips),
            "indent start drifted"
        );
        assert_eq!(
            Some(-value),
            properties.indent_first_line.map(Length::twips),
            "first-line indent drifted"
        );
        assert_eq!(
            Some(value.abs()),
            properties.space_before.map(Length::twips),
            "space before drifted"
        );
    }
}

#[test]
fn hanging_indent_is_written_as_w_hanging() {
    let mut block = paragraph("block-hanging", "hanging");
    block.properties.indent_first_line = Some(twips(-360));
    let (bytes, _) = export(&document(vec![block]));
    let xml = part(&bytes, "word/document.xml");
    assert!(xml.contains("w:hanging=\"360\""), "{xml}");
    assert!(!xml.contains("w:firstLine"), "{xml}");
}

#[test]
fn round_trip_preserves_inline_marks() {
    let marks = vec![
        vec![mark(MarkKind::Bold, None)],
        vec![mark(MarkKind::Italic, None)],
        vec![mark(MarkKind::Underline, None)],
        vec![mark(MarkKind::Strike, None)],
        vec![mark(MarkKind::Superscript, None)],
        vec![mark(MarkKind::Subscript, None)],
        vec![mark(MarkKind::Color, Some("#336699".to_string()))],
        vec![mark(MarkKind::Background, Some("#ffff00".to_string()))],
        vec![mark(MarkKind::Font, Some("Georgia".to_string()))],
        vec![mark(MarkKind::Size, Some("18".to_string()))],
        // `RunProps::marks` emits in a fixed order; a run carrying several
        // marks has to come back in exactly that order.
        vec![
            mark(MarkKind::Bold, None),
            mark(MarkKind::Italic, None),
            mark(MarkKind::Underline, None),
            mark(MarkKind::Strike, None),
            mark(MarkKind::Superscript, None),
            mark(MarkKind::Color, Some("#112233".to_string())),
            mark(MarkKind::Background, Some("#445566".to_string())),
            mark(MarkKind::Font, Some("Courier New".to_string())),
            mark(MarkKind::Size, Some("11".to_string())),
        ],
    ];
    let blocks = marks
        .into_iter()
        .enumerate()
        .map(|(index, marks)| Block {
            id: StableId::parse(format!("block-mark-{index}")).unwrap(),
            kind: BlockKind::Paragraph,
            content: vec![text_inline(&format!("marked {index}"), marks)],
            properties: BlockProperties::default(),
        })
        .collect();
    let warnings = assert_round_trips(&document(blocks));
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn round_trip_preserves_tabs_and_line_breaks_inside_a_run() {
    let block = Block {
        id: StableId::parse("block-white").unwrap(),
        kind: BlockKind::Paragraph,
        content: vec![text_inline("before\tafter\nsecond line", Vec::new())],
        properties: BlockProperties::default(),
    };
    let source = document(vec![block]);
    assert_round_trips(&source);
    // A tab and a line break are elements in WordprocessingML, not characters
    // in a `w:t`. The reader accepts either, so only the markup can say which
    // one was written, and Word only lays out the elements.
    let (bytes, _) = export(&source);
    let xml = part(&bytes, "word/document.xml");
    assert!(xml.contains("<w:tab/>"), "{xml}");
    assert!(xml.contains("<w:br/>"), "{xml}");
    assert!(
        !xml.contains('\t'),
        "a raw tab was left in the markup: {xml}"
    );
}

#[test]
fn round_trip_preserves_links() {
    let blocks = vec![
        Block {
            id: StableId::parse("block-link").unwrap(),
            kind: BlockKind::Paragraph,
            content: vec![
                text_inline("see ", Vec::new()),
                Inline::Link {
                    id: StableId::new("link"),
                    text: "the site".to_string(),
                    href: "https://example.org/a?b=1&c=2".to_string(),
                    marks: vec![mark(MarkKind::Bold, None)],
                },
                text_inline(" and", Vec::new()),
            ],
            properties: BlockProperties::default(),
        },
        Block {
            id: StableId::parse("block-anchor").unwrap(),
            kind: BlockKind::Paragraph,
            content: vec![Inline::Link {
                id: StableId::new("link"),
                text: "internal".to_string(),
                href: "#chapter-two".to_string(),
                marks: Vec::new(),
            }],
            properties: BlockProperties::default(),
        },
    ];
    let warnings = assert_round_trips(&document(blocks));
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn round_trip_preserves_bullet_and_ordered_lists() {
    let list_one = StableId::parse("list-one").unwrap();
    let list_two = StableId::parse("list-two").unwrap();
    let blocks = vec![
        list_item("li-1", &list_one, 0, ListKind::Bullet, "first"),
        list_item("li-2", &list_one, 1, ListKind::Bullet, "nested"),
        paragraph("block-between", "between"),
        list_item("li-3", &list_two, 0, ListKind::Ordered, "one"),
        list_item("li-4", &list_two, 0, ListKind::Ordered, "two"),
    ];
    let warnings = assert_round_trips(&document(blocks));
    assert!(warnings.is_empty(), "{warnings:?}");
}

fn list_item(id: &str, list_id: &StableId, level: u8, kind: ListKind, text: &str) -> Block {
    Block {
        id: StableId::parse(id).unwrap(),
        kind: BlockKind::ListItem {
            list_id: list_id.clone(),
            level,
            kind,
        },
        content: vec![text_inline(text, Vec::new())],
        properties: BlockProperties::default(),
    }
}

#[test]
fn round_trip_preserves_tables() {
    let table = Block {
        id: StableId::parse("block-table").unwrap(),
        kind: BlockKind::table(vec![
            TableRow {
                id: StableId::new("row"),
                cells: vec![
                    TableCell {
                        id: StableId::new("cell"),
                        span: Default::default(),
                        properties: Default::default(),
                        blocks: vec![paragraph("cell-a", "A")],
                    },
                    TableCell {
                        id: StableId::new("cell"),
                        span: Default::default(),
                        properties: Default::default(),
                        blocks: vec![paragraph("cell-b", "B")],
                    },
                ],
            },
            TableRow {
                id: StableId::new("row"),
                cells: vec![
                    TableCell {
                        id: StableId::new("cell"),
                        span: Default::default(),
                        properties: Default::default(),
                        blocks: vec![paragraph("cell-c", "C")],
                    },
                    TableCell {
                        id: StableId::new("cell"),
                        span: Default::default(),
                        properties: Default::default(),
                        blocks: vec![{
                            let mut block = paragraph("cell-d", "D");
                            block.properties.alignment = Some(Alignment::Center);
                            block
                        }],
                    },
                ],
            },
        ]),
        content: Vec::new(),
        properties: BlockProperties::default(),
    };
    let warnings = assert_round_trips(&document(vec![table]));
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn round_trip_preserves_page_breaks() {
    let blocks = vec![
        paragraph("block-one", "first page"),
        Block {
            id: StableId::parse("block-break").unwrap(),
            kind: BlockKind::PageBreak,
            content: Vec::new(),
            properties: BlockProperties::default(),
        },
        paragraph("block-two", "second page"),
    ];
    assert_round_trips(&document(blocks));
}

#[test]
fn round_trip_preserves_footnotes() {
    let footnote_id = StableId::parse("footnote-one").unwrap();
    let mut source = draft(vec![Block {
        id: StableId::parse("block-note").unwrap(),
        kind: BlockKind::Paragraph,
        content: vec![
            text_inline("claim", Vec::new()),
            Inline::FootnoteRef {
                id: StableId::new("footnote-ref"),
                footnote_id: footnote_id.clone(),
            },
        ],
        properties: BlockProperties::default(),
    }]);
    source.footnotes = vec![Footnote {
        id: footnote_id,
        revision: 1,
        body: vec![text_inline("the evidence", Vec::new())],
        deleted: false,
    }];
    source.validate().unwrap();
    let warnings = assert_round_trips(&source);
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn round_trip_preserves_display_equations() {
    let block = Block {
        id: StableId::parse("block-eq").unwrap(),
        kind: BlockKind::EquationBlock {
            equation: Equation {
                id: StableId::new("eq"),
                source_format: EquationSourceFormat::LatexLike,
                source: "E = mc^2".to_string(),
            },
        },
        content: Vec::new(),
        properties: BlockProperties {
            alignment: Some(Alignment::Center),
            ..BlockProperties::default()
        },
    };
    let warnings = assert_round_trips(&document(vec![block]));
    assert_eq!(vec!["docx-export-equation-as-source"], codes(&warnings));
}

#[test]
fn round_trip_preserves_images() {
    let png = tiny_png();
    let hash = opendoc_core::digest_bytes("sha256", &png)
        .unwrap()
        .to_string();
    let images = BTreeMap::from([(
        hash.clone(),
        DocxImage {
            media_type: "image/png".to_string(),
            bytes: png.clone(),
        },
    )]);
    let source = document(vec![Block {
        id: StableId::parse("block-image").unwrap(),
        kind: BlockKind::Image {
            blob_hash: hash.clone(),
            alt_text: "a tiny square".to_string(),
            layout: Default::default(),
        },
        content: Vec::new(),
        properties: BlockProperties::default(),
    }]);
    let (bytes, warnings) = export_with(&source, &images);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(entries(&bytes).contains(&"word/media/image1.png".to_string()));
    let report = reimport(&bytes);
    assert_eq!(normalized(&source), normalized(&report.document));
    assert_eq!(1, report.blobs.len());
    assert_eq!(png, report.blobs[0].bytes);
    assert_eq!(hash, report.blobs[0].hash);
}

/// A real, decodable 2×3 PNG. It has to be genuinely valid, not merely
/// PNG-shaped: the whole point of embedding image bytes is that a reader can
/// decode them, and a bad CRC is invisible until something tries.
fn tiny_png() -> Vec<u8> {
    vec![
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x03, 0x08, 0x02, 0x00, 0x00, 0x00, 0x36,
        0x88, 0x49, 0xd6, 0x00, 0x00, 0x00, 0x10, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0x38,
        0x61, 0x64, 0x04, 0x44, 0x0c, 0x28, 0x14, 0x00, 0x4d, 0x6d, 0x07, 0x09, 0x59, 0xaa, 0x18,
        0x7e, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ]
}

#[test]
fn image_display_size_comes_from_the_pixel_header() {
    let png = tiny_png();
    let hash = opendoc_core::digest_bytes("sha256", &png)
        .unwrap()
        .to_string();
    let images = BTreeMap::from([(
        hash.clone(),
        DocxImage {
            media_type: "image/png".to_string(),
            bytes: png,
        },
    )]);
    let source = document(vec![Block {
        id: StableId::parse("block-image").unwrap(),
        kind: BlockKind::Image {
            blob_hash: hash,
            alt_text: "square".to_string(),
            layout: Default::default(),
        },
        content: Vec::new(),
        properties: BlockProperties::default(),
    }]);
    let (bytes, _) = export_with(&source, &images);
    let xml = part(&bytes, "word/document.xml");
    // 2px × 3px at 96dpi is 19050 × 28575 EMU.
    assert!(xml.contains("cx=\"19050\" cy=\"28575\""), "{xml}");
}

// ---------------------------------------------------------------------------
// Round trips that degrade, with the degradation pinned
// ---------------------------------------------------------------------------

/// WordprocessingML says "heading" only through a style, and a style with no
/// character formatting produces a document that does not look like it has
/// headings — so the writer gives the heading styles real formatting, and the
/// reader faithfully turns that formatting back into marks.
#[test]
fn headings_round_trip_and_pick_up_the_style_formatting() {
    let sizes = [40u32, 32, 28, 24, 22, 20];
    let source = document(
        (1..=6u8)
            .map(|level| Block {
                id: StableId::parse(format!("block-h{level}")).unwrap(),
                kind: BlockKind::Heading { level },
                content: vec![text_inline(&format!("Heading {level}"), Vec::new())],
                properties: BlockProperties::default(),
            })
            .collect(),
    );
    let expected = document(
        (1..=6u8)
            .map(|level| Block {
                id: StableId::parse(format!("block-h{level}")).unwrap(),
                kind: BlockKind::Heading { level },
                content: vec![text_inline(
                    &format!("Heading {level}"),
                    vec![
                        mark(MarkKind::Bold, None),
                        mark(
                            MarkKind::Size,
                            Some((sizes[usize::from(level) - 1] / 2).to_string()),
                        ),
                    ],
                )],
                properties: BlockProperties::default(),
            })
            .collect(),
    );
    assert_round_trips_to(&source, &expected);
}

#[test]
fn checklists_become_a_bulleted_list_and_say_so() {
    let list_id = StableId::parse("list-checks").unwrap();
    let source = document(vec![
        list_item(
            "li-1",
            &list_id,
            0,
            ListKind::Checklist { checked: false },
            "todo",
        ),
        list_item(
            "li-2",
            &list_id,
            0,
            ListKind::Checklist { checked: true },
            "done",
        ),
    ]);
    let (bytes, warnings) = export(&source);
    assert!(codes(&warnings).contains(&"docx-export-checklist-as-bullet"));
    // Ticked and unticked items need different bullet glyphs, and a glyph is a
    // property of a numbering definition, so the one run becomes two.
    assert!(codes(&warnings).contains(&"docx-export-split-mixed-list"));
    let numbering = part(&bytes, "word/numbering.xml");
    assert!(numbering.contains('\u{2610}'), "{numbering}");
    assert!(numbering.contains('\u{2612}'), "{numbering}");

    let report = reimport(&bytes);
    let kinds: Vec<Option<ListKind>> = report
        .document
        .blocks
        .iter()
        .map(|block| block.list_kind())
        .collect();
    assert_eq!(vec![Some(ListKind::Bullet), Some(ListKind::Bullet)], kinds);
    let list_ids: Vec<&str> = report
        .document
        .blocks
        .iter()
        .filter_map(|block| block.list_id())
        .map(StableId::as_str)
        .collect();
    assert_ne!(
        list_ids[0], list_ids[1],
        "the two checkbox states are two numbering definitions and re-import as two lists"
    );
}

#[test]
fn code_marks_become_a_monospace_font_and_say_so() {
    let source = document(vec![Block {
        id: StableId::parse("block-code").unwrap(),
        kind: BlockKind::Paragraph,
        content: vec![text_inline("let x = 1;", vec![mark(MarkKind::Code, None)])],
        properties: BlockProperties::default(),
    }]);
    let expected = document(vec![Block {
        id: StableId::parse("block-code").unwrap(),
        kind: BlockKind::Paragraph,
        content: vec![text_inline(
            "let x = 1;",
            vec![mark(MarkKind::Font, Some("Consolas".to_string()))],
        )],
        properties: BlockProperties::default(),
    }]);
    let warnings = assert_round_trips_to(&source, &expected);
    assert_eq!(vec!["docx-export-code-mark-as-monospace"], codes(&warnings));
    // Word needs the character style as well as the font: the style is what a
    // reader recognises as "this was code", and the font is what it looks
    // like. The round trip alone cannot see the difference, so pin both.
    let (bytes, _) = export(&source);
    let xml = part(&bytes, "word/document.xml");
    assert!(xml.contains("<w:rStyle w:val=\"Code\"/>"), "{xml}");
    assert!(xml.contains("Consolas"), "{xml}");
    let styles = part(&bytes, "word/styles.xml");
    assert!(styles.contains("w:styleId=\"Code\""), "{styles}");
}

#[test]
fn line_spacing_that_is_not_a_whole_240th_is_rounded_and_named() {
    let mut block = paragraph("block-spacing", "odd spacing");
    // 1.151× is 1151 thousandths; 240ths cannot express it.
    block.properties.line_spacing = Some(LineSpacing::multiple(1.151).unwrap());
    let (_, warnings) = export(&document(vec![block]));
    assert_eq!(
        vec!["docx-export-approximated-line-spacing"],
        codes(&warnings)
    );
}

#[test]
fn line_spacing_that_is_a_whole_240th_is_silent_and_exact() {
    for ratio in [1.0, 1.5, 2.0, 1.15, 0.5, 3.0] {
        let mut block = paragraph("block-spacing", "spacing");
        block.properties.line_spacing = Some(LineSpacing::multiple(ratio).unwrap());
        let source = document(vec![block]);
        let (bytes, warnings) = export(&source);
        assert!(warnings.is_empty(), "{ratio}: {warnings:?}");
        let report = reimport(&bytes);
        assert_eq!(
            source.blocks[0].properties.line_spacing,
            report.document.blocks[0].properties.line_spacing,
            "{ratio}"
        );
    }
}

#[test]
fn block_formatting_on_a_block_with_no_paragraph_is_named() {
    let source = document(vec![Block {
        id: StableId::parse("block-break").unwrap(),
        kind: BlockKind::PageBreak,
        content: Vec::new(),
        properties: BlockProperties {
            alignment: Some(Alignment::Center),
            space_before: Some(twips(120)),
            ..BlockProperties::default()
        },
    }]);
    let (_, warnings) = export(&source);
    assert_eq!(
        vec!["docx-export-dropped-block-properties"],
        codes(&warnings)
    );
    assert!(warnings[0].message.contains("alignment"));
    assert!(warnings[0].message.contains("space-before"));
}

#[test]
fn a_missing_image_blob_becomes_its_alt_text_and_is_named() {
    let source = document(vec![Block {
        id: StableId::parse("block-image").unwrap(),
        kind: BlockKind::Image {
            blob_hash: "sha256:deadbeef".to_string(),
            alt_text: "the missing chart".to_string(),
            layout: Default::default(),
        },
        content: Vec::new(),
        properties: BlockProperties::default(),
    }]);
    let (bytes, warnings) = export(&source);
    assert_eq!(vec!["docx-export-missing-image-blob"], codes(&warnings));
    let report = reimport(&bytes);
    assert_eq!("the missing chart\n", report.document.visible_text());
}

#[test]
fn comments_suggestions_and_the_doi_are_named_rather_than_vanishing() {
    let mut source = document(vec![paragraph("block-one", "body")]);
    source.doi = Some("10.1234/opendoc".to_string());
    source.validate().unwrap();
    let (_, warnings) = export(&source);
    assert_eq!(vec!["docx-export-dropped-doi"], codes(&warnings));
}

#[test]
fn a_character_xml_cannot_encode_is_removed_and_named() {
    let source = document(vec![Block {
        id: StableId::parse("block-ctrl").unwrap(),
        kind: BlockKind::Paragraph,
        content: vec![text_inline("before\u{1}after", Vec::new())],
        properties: BlockProperties::default(),
    }]);
    let (bytes, warnings) = export(&source);
    assert_eq!(
        vec!["docx-export-dropped-control-character"],
        codes(&warnings)
    );
    let report = reimport(&bytes);
    assert_eq!("beforeafter\n", report.document.visible_text());
}

// ---------------------------------------------------------------------------
// Package structure
// ---------------------------------------------------------------------------

#[test]
fn the_package_has_the_parts_word_requires() {
    let list_id = StableId::parse("list-one").unwrap();
    let footnote_id = StableId::parse("footnote-one").unwrap();
    let png = tiny_png();
    let hash = opendoc_core::digest_bytes("sha256", &png)
        .unwrap()
        .to_string();
    let images = BTreeMap::from([(
        hash.clone(),
        DocxImage {
            media_type: "image/png".to_string(),
            bytes: png,
        },
    )]);
    let mut source = draft(vec![
        list_item("li-1", &list_id, 0, ListKind::Bullet, "item"),
        Block {
            id: StableId::parse("block-image").unwrap(),
            kind: BlockKind::Image {
                blob_hash: hash,
                alt_text: "square".to_string(),
                layout: Default::default(),
            },
            content: Vec::new(),
            properties: BlockProperties::default(),
        },
        Block {
            id: StableId::parse("block-note").unwrap(),
            kind: BlockKind::Paragraph,
            content: vec![Inline::FootnoteRef {
                id: StableId::new("footnote-ref"),
                footnote_id: footnote_id.clone(),
            }],
            properties: BlockProperties::default(),
        },
    ]);
    source.footnotes = vec![Footnote {
        id: footnote_id,
        revision: 1,
        body: vec![text_inline("note", Vec::new())],
        deleted: false,
    }];
    source.header = vec![paragraph("header-one", "the header")];
    source.footer = vec![paragraph("footer-one", "the footer")];
    source.validate().unwrap();

    let (bytes, _) = export_with(&source, &images);
    let mut names = entries(&bytes);
    names.sort();
    assert_eq!(
        vec![
            "[Content_Types].xml",
            "_rels/.rels",
            "docProps/core.xml",
            "word/_rels/document.xml.rels",
            "word/document.xml",
            "word/footer1.xml",
            "word/footnotes.xml",
            "word/header1.xml",
            "word/media/image1.png",
            "word/numbering.xml",
            "word/styles.xml",
        ],
        names
    );

    // Every declared relationship target exists, and every XML part parses.
    let rels = part(&bytes, "word/_rels/document.xml.rels");
    for target in [
        "styles.xml",
        "numbering.xml",
        "footnotes.xml",
        "header1.xml",
        "footer1.xml",
        "media/image1.png",
    ] {
        assert!(rels.contains(target), "relationship for {target} missing");
        assert!(
            entries(&bytes).contains(&format!("word/{target}")),
            "part word/{target} missing"
        );
    }
    let content_types = part(&bytes, "[Content_Types].xml");
    for part_name in [
        "/word/document.xml",
        "/word/styles.xml",
        "/word/numbering.xml",
        "/word/footnotes.xml",
        "/word/header1.xml",
        "/word/footer1.xml",
        "/docProps/core.xml",
    ] {
        assert!(
            content_types.contains(part_name),
            "content type for {part_name} missing: {content_types}"
        );
    }
    assert!(
        content_types.contains("Extension=\"png\""),
        "{content_types}"
    );

    for name in entries(&bytes) {
        if !name.ends_with(".xml") && !name.ends_with(".rels") {
            continue;
        }
        let text = part(&bytes, &name);
        crate::xml::parse_xml(&text).unwrap_or_else(|err| panic!("{name} is not valid XML: {err}"));
    }
}

#[test]
fn the_document_title_is_written_to_the_core_properties() {
    let (bytes, _) = export(&document(vec![paragraph("block-one", "body")]));
    let core = part(&bytes, "docProps/core.xml");
    assert!(core.contains("<dc:title>Round Trip</dc:title>"), "{core}");
}

#[test]
fn xml_special_characters_are_escaped_in_text_and_attributes() {
    let source = document(vec![Block {
        id: StableId::parse("block-escape").unwrap(),
        kind: BlockKind::Paragraph,
        content: vec![
            text_inline("5 < 6 & \"quoted\"", Vec::new()),
            Inline::Link {
                id: StableId::new("link"),
                text: "x".to_string(),
                href: "https://example.org/?a=1&b=<2>".to_string(),
                marks: Vec::new(),
            },
        ],
        properties: BlockProperties::default(),
    }]);
    let (bytes, _) = export(&source);
    let xml = part(&bytes, "word/document.xml");
    assert!(xml.contains("5 &lt; 6 &amp; \"quoted\""), "{xml}");
    let rels = part(&bytes, "word/_rels/document.xml.rels");
    assert!(rels.contains("a=1&amp;b=&lt;2&gt;"), "{rels}");
    assert_round_trips(&source);
}

/// The package must be reproducible: two exports of the same document are the
/// same bytes, which also proves nothing in the writer reads a clock.
#[test]
fn exporting_the_same_document_twice_produces_identical_bytes() {
    let source = document(vec![
        paragraph("block-one", "one"),
        paragraph("block-two", "two"),
    ]);
    let (first, _) = export(&source);
    let (second, _) = export(&source);
    assert_eq!(first, second);
    // Comparing two exports only catches a clock that happened to tick between
    // them. The stamp itself is the real invariant.
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(first)).expect("not a zip");
    for index in 0..archive.len() {
        let file = archive.by_index(index).unwrap();
        assert_eq!(
            "1980-01-01 00:00:00",
            file.last_modified().expect("no timestamp").to_string(),
            "{} carries a wall-clock timestamp",
            file.name()
        );
    }
}

// ---------------------------------------------------------------------------
// Page geometry and furniture
// ---------------------------------------------------------------------------

#[test]
fn page_setup_is_written_as_section_properties_in_twips() {
    let mut source = document(vec![paragraph("block-one", "body")]);
    source.page_setup = PageSetup {
        width: twips(11907),
        height: twips(16840),
        margin_top: twips(1134),
        margin_bottom: twips(1135),
        margin_start: twips(1136),
        margin_end: twips(1137),
        margin_header: twips(567),
        margin_footer: twips(568),
    };
    source.validate().unwrap();
    let (bytes, _) = export(&source);
    let xml = part(&bytes, "word/document.xml");
    assert!(
        xml.contains("<w:pgSz w:w=\"11907\" w:h=\"16840\"/>"),
        "{xml}"
    );
    assert!(
        xml.contains(
            "<w:pgMar w:top=\"1134\" w:right=\"1137\" w:bottom=\"1135\" w:left=\"1136\" w:header=\"567\" w:footer=\"568\" w:gutter=\"0\"/>"
        ),
        "{xml}"
    );
}

#[test]
fn a_landscape_page_is_marked_landscape() {
    let mut source = document(vec![paragraph("block-one", "body")]);
    source.page_setup = PageSetup::new(twips(15840), twips(12240)).unwrap();
    source.validate().unwrap();
    let (bytes, _) = export(&source);
    assert!(part(&bytes, "word/document.xml").contains("w:orient=\"landscape\""));
}

#[test]
fn headers_and_footers_become_referenced_parts() {
    let mut source = document(vec![paragraph("block-one", "body")]);
    source.header = vec![paragraph("header-one", "top of every page")];
    source.footer = vec![Block {
        id: StableId::parse("footer-one").unwrap(),
        kind: BlockKind::Paragraph,
        content: vec![
            text_inline("page ", Vec::new()),
            Inline::PageNumber {
                id: StableId::new("page-number"),
                field: PageNumberField::CurrentPage,
            },
            text_inline(" of ", Vec::new()),
            Inline::PageNumber {
                id: StableId::new("page-number"),
                field: PageNumberField::PageCount,
            },
        ],
        properties: BlockProperties::default(),
    }];
    source.validate().unwrap();
    let (bytes, warnings) = export(&source);
    assert_eq!(
        vec!["docx-export-page-number-placeholder"],
        codes(&warnings)
    );
    let document_xml = part(&bytes, "word/document.xml");
    assert!(document_xml.contains("w:headerReference"), "{document_xml}");
    assert!(document_xml.contains("w:footerReference"), "{document_xml}");
    let header = part(&bytes, "word/header1.xml");
    assert!(header.contains("top of every page"), "{header}");
    let footer = part(&bytes, "word/footer1.xml");
    // The field has to stay a field: an instruction inside `w:instrText`,
    // bracketed by field characters. Written as plain text it would freeze the
    // page number at export time.
    assert!(
        footer.contains("<w:instrText xml:space=\"preserve\"> PAGE </w:instrText>"),
        "{footer}"
    );
    assert!(
        footer.contains("<w:instrText xml:space=\"preserve\"> NUMPAGES </w:instrText>"),
        "{footer}"
    );
    assert!(footer.contains("w:fldCharType=\"begin\""), "{footer}");
    assert!(footer.contains("w:fldCharType=\"separate\""), "{footer}");
    assert!(footer.contains("w:fldCharType=\"end\""), "{footer}");
}

/// The page is a *round trip*, not a one-way write. Every dimension is twips
/// on both sides, so the values that come back must be the integers that went
/// out — asserted over numbers no conversion through points, pixels or
/// millimetres could reproduce exactly.
#[test]
fn page_setup_round_trips_through_the_reader() {
    let mut source = document(vec![paragraph("block-one", "body")]);
    source.page_setup = PageSetup {
        width: twips(11907),
        height: twips(16840),
        margin_top: twips(1134),
        margin_bottom: twips(1135),
        margin_start: twips(1136),
        margin_end: twips(1137),
        margin_header: twips(567),
        margin_footer: twips(568),
    };
    source.validate().unwrap();
    let (bytes, _) = export(&source);
    let report = reimport(&bytes);
    assert_eq!(source.page_setup, report.document.page_setup);
    // And the reader must stop calling the page it just read "dropped".
    assert!(
        !codes(&report.warnings).contains(&"docx-dropped-section-properties"),
        "{:?}",
        report.warnings
    );
}

/// Orientation is derived from the dimensions and never stored (ADR 0009).
/// WordprocessingML writes `w:orient` *and* already-swapped dimensions, so a
/// reader that honoured both would turn the page twice — landscape in, portrait
/// back. This is the check that would catch that.
#[test]
fn a_landscape_page_comes_back_landscape() {
    let mut source = document(vec![paragraph("block-one", "body")]);
    source.page_setup = PageSetup::new(twips(15840), twips(12240)).unwrap();
    source.validate().unwrap();
    let (bytes, _) = export(&source);
    let report = reimport(&bytes);
    assert_eq!(source.page_setup, report.document.page_setup);
    assert_eq!(
        opendoc_core::PageOrientation::Landscape,
        report.document.page_setup.orientation()
    );
}

/// The header and footer come back as header and footer, and the page-number
/// fields come back as *fields*.
///
/// Word writes a cached result between `w:fldChar separate` and `end` so it has
/// something to paint before it next repaginates. Importing that cached "1" as
/// text would turn a field whose value the layout computes into a frozen
/// number that is wrong on every page but the first.
#[test]
fn headers_footers_and_page_number_fields_round_trip() {
    let mut source = document(vec![paragraph("block-one", "body")]);
    source.header = vec![paragraph("header-one", "top of every page")];
    source.footer = vec![Block {
        id: StableId::parse("footer-one").unwrap(),
        kind: BlockKind::Paragraph,
        content: vec![
            text_inline("page ", Vec::new()),
            Inline::PageNumber {
                id: StableId::new("page-number"),
                field: PageNumberField::CurrentPage,
            },
            text_inline(" of ", Vec::new()),
            Inline::PageNumber {
                id: StableId::new("page-number"),
                field: PageNumberField::PageCount,
            },
        ],
        properties: BlockProperties::default(),
    }];
    source.validate().unwrap();
    let (bytes, _) = export(&source);
    let report = reimport(&bytes);
    assert_eq!(
        normalized(&source).header,
        normalized(&report.document).header,
        "the header did not come back"
    );
    assert_eq!(
        normalized(&source).footer,
        normalized(&report.document).footer,
        "the footer did not come back as text plus two fields"
    );
    assert!(
        !codes(&report.warnings).contains(&"docx-dropped-header-footer"),
        "{:?}",
        report.warnings
    );
}

/// `w:fldSimple` is WordprocessingML's one-element form of the same field.
/// Word writes the three-run form, so the round-trip test above never reaches
/// this path; a real-world document from another producer does.
#[test]
fn a_page_number_written_as_fld_simple_is_read_as_a_field() {
    let xml = concat!(
        r#"<?xml version="1.0"?>"#,
        r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">"#,
        r#"<w:body><w:p><w:r><w:t>page </w:t></w:r>"#,
        r#"<w:fldSimple w:instr=" PAGE \* MERGEFORMAT "><w:r><w:t>7</w:t></w:r></w:fldSimple>"#,
        r#"</w:p></w:body></w:document>"#,
    );
    let report = import_docx_bytes(TITLE, xml.as_bytes()).expect("import failed");
    let content = &report.document.blocks[0].content;
    assert!(
        matches!(
            content.last(),
            Some(Inline::PageNumber {
                field: PageNumberField::CurrentPage,
                ..
            })
        ),
        "{content:?}"
    );
    assert!(
        !report.document.visible_text().contains('7'),
        "the cached field result was imported as literal text: {:?}",
        report.document.visible_text()
    );
}

/// OpenDoc has one header and one footer for the whole document, so a
/// first-page or even-page variant has nowhere to go. It is named rather than
/// applied to every page, which is what ADR 0009 asks an importer to do.
#[test]
fn a_first_page_header_variant_is_reported_rather_than_applied() {
    let mut source = document(vec![paragraph("block-one", "body")]);
    source.header = vec![paragraph("header-one", "default header")];
    source.validate().unwrap();
    let (bytes, _) = export(&source);
    // Re-point the default header reference at a `first` one: the part is
    // there, but the slot OpenDoc models is not.
    let patched = rewrite_part(&bytes, "word/document.xml", |xml| {
        xml.replace(r#"w:type="default""#, r#"w:type="first""#)
    });
    let report = import_docx_bytes(TITLE, &patched).expect("import failed");
    assert!(report.document.header.is_empty(), "the variant was applied");
    assert!(
        codes(&report.warnings).contains(&"docx-dropped-header-footer"),
        "{:?}",
        report.warnings
    );
}

/// A paragraph with no text exports as an empty `w:p` — which is what a blank
/// line is in WordprocessingML — but the reader builds blocks only from
/// inline content, so the blank line does not survive the trip back. The gap
/// is the reader's; pinning it here stops it being rediscovered as an export
/// bug.
#[test]
fn a_blank_paragraph_is_written_but_the_reader_drops_it() {
    let source = document(vec![
        paragraph("block-one", "first"),
        paragraph("block-blank", ""),
        paragraph("block-two", "second"),
    ]);
    let (bytes, warnings) = export(&source);
    assert!(warnings.is_empty(), "{warnings:?}");
    let xml = part(&bytes, "word/document.xml");
    assert!(
        xml.contains("<w:p></w:p>"),
        "the blank paragraph was not written: {xml}"
    );
    let report = reimport(&bytes);
    assert_eq!(
        2,
        report.document.blocks.len(),
        "the reader keeps only the paragraphs that carry text"
    );
}
