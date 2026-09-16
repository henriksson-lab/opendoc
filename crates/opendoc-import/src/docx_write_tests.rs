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

use crate::{export_docx_with_warnings, import_docx_bytes, mark, ExportImage, ImportReport};
use opendoc_core::{
    Alignment, Block, BlockKind, BlockProperties, Bookmark, BorderStyle, CellBorder, CellSpan,
    Color, Document, Equation, EquationSourceFormat, Footnote, ImageCrop, ImageLayout,
    ImagePlacement, Inline, Length, LineSpacing, ListKind, Mark, MarkKind, ModelWarning,
    PageNumberField, PageSetup, PositionedImage, PositionedImageAnchor, PositionedImageLayer,
    StableId, TableCell, TableColumn, TableRow, TextDirection, VerticalAlignment,
};
use std::collections::BTreeMap;
use std::io::Read;

const TITLE: &str = "Round Trip";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn no_images() -> BTreeMap<String, ExportImage> {
    BTreeMap::new()
}

fn export(document: &Document) -> (Vec<u8>, Vec<ModelWarning>) {
    export_docx_with_warnings(document, &no_images()).expect("export failed")
}

fn export_with(
    document: &Document,
    images: &BTreeMap<String, ExportImage>,
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
    let endnote_ids = document.endnote_ids.clone();
    // Footnote identities are referenced from the body, so they are numbered
    // first and the references pick up the same value.
    for footnote in &mut document.footnotes {
        footnote.id = renumber.id(&footnote.id.clone());
    }
    document.endnote_ids = endnote_ids.iter().map(|id| renumber.id(id)).collect();
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
            BlockKind::Table { columns, rows, .. } => {
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
            BlockKind::Paragraph
            | BlockKind::Title
            | BlockKind::Subtitle
            | BlockKind::Heading { .. }
            | BlockKind::Image { .. } => {}
            BlockKind::HorizontalRule
            | BlockKind::TableOfContents { .. }
            | BlockKind::Bibliography
            | BlockKind::PageBreak => {}
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
            | Inline::GooglePersonChip { id, .. }
            | Inline::GoogleRichLinkChip { id, .. }
            | Inline::Dropdown { id, .. }
            | Inline::DateChip { id, .. }
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
            keep_with_next: Some(true),
            ..BlockProperties::default()
        },
        BlockProperties {
            background: Some(Color::parse("#336699").unwrap()),
            ..BlockProperties::default()
        },
        BlockProperties {
            border: Some(
                CellBorder::new(
                    BorderStyle::Dashed,
                    twips(20),
                    Color::parse("#336699").unwrap(),
                )
                .unwrap(),
            ),
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
            keep_with_next: None,
            background: None,
            border: None,
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

#[test]
fn round_trip_preserves_ordered_list_starts_at_each_level() {
    let list = StableId::parse("continued-list").unwrap();
    let mut source = document(vec![
        list_item("li-one", &list, 0, ListKind::Ordered, "seven"),
        list_item("li-two", &list, 1, ListKind::Ordered, "nested eleven"),
    ]);
    let properties = source.list_properties.entry(list).or_default();
    properties.ordered_starts.insert(0, 7);
    properties.ordered_starts.insert(1, 11);

    let (bytes, warnings) = export(&source);
    let imported = reimport(&bytes);
    let imported_list = imported.document.blocks[0]
        .list_id()
        .expect("first block is a list item");
    assert_eq!(
        imported
            .document
            .list_properties
            .get(imported_list)
            .expect("imported list settings"),
        source
            .list_properties
            .values()
            .next()
            .expect("source list settings")
    );
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn round_trip_preserves_explicit_ordered_list_formats() {
    let list = StableId::parse("formatted-list").unwrap();
    let mut source = document(vec![list_item(
        "li-one",
        &list,
        0,
        ListKind::Ordered,
        "first",
    )]);
    source
        .list_properties
        .entry(list)
        .or_default()
        .ordered_formats
        .insert(0, opendoc_core::OrderedListFormat::UpperRoman);

    let (bytes, warnings) = export(&source);
    let imported = reimport(&bytes);
    let imported_list = imported.document.blocks[0].list_id().unwrap();
    assert_eq!(
        imported.document.list_properties[imported_list].format_for(0),
        opendoc_core::OrderedListFormat::UpperRoman
    );
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
                height: None,
                header: true,
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
                height: None,
                header: false,
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

/// A three-by-three grid with the two merges LibreOffice produces from an
/// HTML table: one `colspan`, one `rowspan`. Column widths, a shaded cell,
/// per-cell borders and a vertical alignment ride along, because all of them
/// used to leave the exporter in silence.
fn merged_table() -> Block {
    let mut cells: Vec<Vec<TableCell>> = (0..3)
        .map(|row| {
            (0..3)
                .map(|column| TableCell {
                    id: StableId::parse(format!("cell-{row}-{column}")).unwrap(),
                    span: Default::default(),
                    properties: Default::default(),
                    blocks: vec![paragraph(
                        &format!("block-{row}-{column}"),
                        &format!("r{row}c{column}"),
                    )],
                })
                .collect()
        })
        .collect();
    cells[0][0].span = CellSpan::new(1, 2).unwrap();
    cells[0][0].properties.background = Some(Color::parse("#ffcc00").unwrap());
    cells[0][0].properties.vertical_alignment = Some(VerticalAlignment::Middle);
    cells[0][0].properties.border_top = Some(
        CellBorder::new(
            BorderStyle::Double,
            twips(5),
            Color::parse("#808080").unwrap(),
        )
        .unwrap(),
    );
    cells[0][0].properties.border_start = Some(CellBorder::none());
    cells[0][0].properties.padding_start = Some(twips(120));
    // Nothing is drawn here, so nothing is written; the model keeps the cell
    // and the export has to say the content went nowhere.
    cells[0][1].blocks = vec![paragraph("block-0-1", "")];
    cells[1][0].span = CellSpan::new(2, 1).unwrap();
    cells[2][0].blocks = vec![paragraph("block-2-0", "")];
    Block {
        id: StableId::parse("block-merged-table").unwrap(),
        kind: BlockKind::Table {
            columns: vec![
                TableColumn {
                    id: StableId::parse("column-a").unwrap(),
                    width: Some(twips(1670)),
                },
                TableColumn {
                    id: StableId::parse("column-b").unwrap(),
                    width: Some(twips(455)),
                },
                TableColumn {
                    id: StableId::parse("column-c").unwrap(),
                    width: Some(twips(1220)),
                },
            ],
            properties: Default::default(),
            rows: cells
                .into_iter()
                .enumerate()
                .map(|(index, cells)| TableRow {
                    id: StableId::parse(format!("row-{index}")).unwrap(),
                    height: None,
                    header: false,
                    cells,
                })
                .collect(),
        },
        content: Vec::new(),
        properties: BlockProperties::default(),
    }
}

/// P1-4's second half. `w:gridSpan`, `w:vMerge`, `w:gridCol`, `w:shd`,
/// `w:tcBorders`, `w:tcMar` and `w:vAlign` all have exact WordprocessingML
/// spellings, so the whole table survives its own round trip — merges,
/// widths and styling included. It used to survive none of them, silently.
#[test]
fn a_merged_styled_table_round_trips_through_the_readers_own_grid() {
    let source = document(vec![merged_table()]);
    let (bytes, _) = export(&source);
    let body = part(&bytes, "word/document.xml");
    assert!(body.contains(r#"<w:gridCol w:w="1670"/>"#), "{body}");
    assert!(body.contains(r#"<w:gridCol w:w="455"/>"#), "{body}");
    assert!(body.contains(r#"<w:gridCol w:w="1220"/>"#), "{body}");
    assert!(body.contains(r#"<w:gridSpan w:val="2"/>"#), "{body}");
    assert!(body.contains(r#"<w:vMerge w:val="restart"/>"#), "{body}");
    assert!(body.contains(r#"<w:vMerge w:val="continue"/>"#), "{body}");
    assert!(
        body.contains(r#"<w:shd w:val="clear" w:color="auto" w:fill="ffcc00"/>"#),
        "{body}"
    );
    assert!(
        body.contains(r#"<w:top w:val="double" w:sz="2" w:space="0" w:color="808080"/>"#),
        "{body}"
    );
    assert!(body.contains(r#"<w:left w:val="nil"/>"#), "{body}");
    assert!(
        body.contains(r#"<w:left w:w="120" w:type="dxa"/>"#),
        "{body}"
    );
    assert!(body.contains(r#"<w:vAlign w:val="center"/>"#), "{body}");
    // A merged row writes fewer `w:tc` elements than the grid has columns,
    // and the widths of the columns a span covers are added up.
    assert!(
        body.contains(r#"<w:tcW w:w="2125" w:type="dxa"/>"#),
        "{body}"
    );
    assert_round_trips(&source);
}

/// ADR 0013 states the cost plainly: a format that flattens the grid loses
/// the covered cells' content. WordprocessingML is such a format, so the one
/// thing that cannot cross has to be named (ADR 0010) rather than vanish.
#[test]
fn content_underneath_a_merged_cell_is_named_rather_than_vanishing() {
    let mut table = merged_table();
    let BlockKind::Table { rows, .. } = &mut table.kind else {
        panic!("expected a table");
    };
    // One covered by a vertical merge, one swallowed by a `w:gridSpan`:
    // neither has a `w:tc` of its own to hold anything.
    rows[2].cells[0].blocks = vec![paragraph("block-hidden", "hidden by the merge")];
    rows[0].cells[1].blocks = vec![paragraph("block-swallowed", "swallowed by the span")];
    let source = document(vec![table]);
    let (bytes, warnings) = export(&source);
    assert!(
        codes(&warnings).contains(&"docx-export-dropped-covered-cell-content"),
        "{warnings:?}"
    );
    let body = part(&bytes, "word/document.xml");
    assert!(
        !body.contains("hidden by the merge") && !body.contains("swallowed by the span"),
        "a covered cell's content reached the package"
    );
}

/// An *auto* column has no `w:gridCol` spelling, so the writer shares the
/// width out and says which mode it is in; the reader reads the declaration,
/// not the numbers, so auto stays auto across the round trip.
#[test]
fn auto_width_columns_stay_auto_across_the_round_trip() {
    let source = document(vec![Block {
        id: StableId::parse("block-auto-table").unwrap(),
        kind: BlockKind::table(vec![TableRow {
            id: StableId::parse("row-auto").unwrap(),
            height: None,
            header: false,
            cells: vec![
                TableCell {
                    id: StableId::parse("cell-auto-a").unwrap(),
                    span: Default::default(),
                    properties: Default::default(),
                    blocks: vec![paragraph("block-auto-a", "A")],
                },
                TableCell {
                    id: StableId::parse("cell-auto-b").unwrap(),
                    span: Default::default(),
                    properties: Default::default(),
                    blocks: vec![paragraph("block-auto-b", "B")],
                },
            ],
        }]),
        content: Vec::new(),
        properties: BlockProperties::default(),
    }]);
    let (bytes, _) = export(&source);
    let body = part(&bytes, "word/document.xml");
    assert!(
        body.contains(r#"<w:tblLayout w:type="autofit"/>"#),
        "{body}"
    );
    let report = reimport(&bytes);
    let BlockKind::Table { columns, .. } = &report.document.blocks[0].kind else {
        panic!("expected a table");
    };
    assert!(
        columns.iter().all(|column| column.width.is_none()),
        "a shared-out width came back as a chosen one: {columns:?}"
    );
}

/// A thickness the model can state and `w:sz` cannot — it counts eighths of a
/// point, so 2.5 twips is its finest step — is rounded and says so.
#[test]
fn a_cell_border_thickness_w_sz_cannot_state_is_rounded_and_named() {
    let mut table = merged_table();
    let BlockKind::Table { rows, .. } = &mut table.kind else {
        panic!("expected a table");
    };
    rows[0].cells[2].properties.border_top =
        Some(CellBorder::new(BorderStyle::Solid, twips(6), Color::BLACK).unwrap());
    let (bytes, warnings) = export(&document(vec![table]));
    assert!(
        codes(&warnings).contains(&"docx-export-approximated-cell-border"),
        "{warnings:?}"
    );
    // 6 twips is 2.4 eighths of a point, and the nearest statable value is 2.
    assert!(
        part(&bytes, "word/document.xml")
            .contains(r#"<w:top w:val="single" w:sz="2" w:space="0" w:color="000000"/>"#),
        "the rounded thickness is not what was written"
    );
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
fn horizontal_rule_exports_as_a_native_word_paragraph_border() {
    let source = document(vec![Block {
        id: StableId::parse("block-rule").unwrap(),
        kind: BlockKind::HorizontalRule,
        content: Vec::new(),
        properties: BlockProperties::default(),
    }]);
    let (bytes, warnings) = export(&source);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(part(&bytes, "word/document.xml").contains(
        r#"<w:pBdr><w:bottom w:val="single" w:sz="6" w:space="1" w:color="6B7280"/></w:pBdr>"#
    ));
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
fn round_trip_preserves_native_endnotes_in_their_own_docx_part() {
    let endnote_id = StableId::parse("endnote-one").unwrap();
    let mut source = draft(vec![Block {
        id: StableId::parse("block-endnote").unwrap(),
        kind: BlockKind::Paragraph,
        content: vec![
            text_inline("claim", Vec::new()),
            Inline::FootnoteRef {
                id: StableId::new("endnote-ref"),
                footnote_id: endnote_id.clone(),
            },
        ],
        properties: BlockProperties::default(),
    }]);
    source.footnotes = vec![Footnote {
        id: endnote_id.clone(),
        revision: 1,
        body: vec![text_inline("the endnote evidence", Vec::new())],
        deleted: false,
    }];
    source.endnote_ids.insert(endnote_id);
    source.validate().unwrap();

    let (bytes, warnings) = export(&source);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(entries(&bytes).contains(&"word/endnotes.xml".to_string()));
    assert!(part(&bytes, "word/document.xml").contains("w:endnoteReference"));
    assert!(part(&bytes, "word/endnotes.xml").contains("w:endnoteRef"));
    assert!(part(&bytes, "word/_rels/document.xml.rels").contains("/endnotes"));
    assert!(part(&bytes, "[Content_Types].xml").contains("/word/endnotes.xml"));

    let report = reimport(&bytes);
    assert_eq!(normalized(&source), normalized(&report.document));
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
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
        ExportImage {
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

#[test]
fn image_media_type_parameters_do_not_drop_a_supported_docx_part() {
    // The bytes intentionally have no sniffable image signature.  The writer
    // must use the declared MIME essence, as raw-image save and PDF export do,
    // rather than misrepresent this valid WebP declaration as unsupported.
    let hash = "sha256:parameterized-webp".to_string();
    let images = BTreeMap::from([(
        hash.clone(),
        ExportImage {
            media_type: " Image/WebP; codecs=vp8 ".to_string(),
            bytes: b"webp source bytes".to_vec(),
        },
    )]);
    let source = document(vec![Block {
        id: StableId::parse("block-parameterized-webp").unwrap(),
        kind: BlockKind::Image {
            blob_hash: hash,
            alt_text: "a WebP figure".to_string(),
            layout: Default::default(),
        },
        content: Vec::new(),
        properties: BlockProperties::default(),
    }]);

    let (bytes, warnings) = export_with(&source, &images);
    assert!(
        !codes(&warnings).contains(&"docx-export-unsupported-image-media-type"),
        "parameterized WebP fell back to alt text: {warnings:?}"
    );
    assert!(entries(&bytes).contains(&"word/media/image1.webp".to_string()));
    assert!(part(&bytes, "[Content_Types].xml")
        .contains("Extension=\"webp\" ContentType=\"image/webp\""),);
}

#[test]
fn image_without_alt_text_does_not_export_a_generated_name_as_description() {
    let png = tiny_png();
    let hash = opendoc_core::digest_bytes("sha256", &png)
        .unwrap()
        .to_string();
    let images = BTreeMap::from([(
        hash.clone(),
        ExportImage {
            media_type: "image/png".to_string(),
            bytes: png,
        },
    )]);
    let source = document(vec![Block {
        id: StableId::parse("block-image").unwrap(),
        kind: BlockKind::Image {
            blob_hash: hash,
            alt_text: "".to_string(),
            layout: Default::default(),
        },
        content: Vec::new(),
        properties: BlockProperties::default(),
    }]);

    let (bytes, warnings) = export_with(&source, &images);
    assert!(warnings.is_empty(), "{warnings:?}");
    let xml = part(&bytes, "word/document.xml");
    assert!(xml.contains("wp:docPr id=\"1\" name=\"Image 1\""), "{xml}");
    assert!(xml.contains("pic:cNvPr id=\"0\" name=\"Image 1\""), "{xml}");
    assert!(!xml.contains("descr="), "{xml}");
    let report = reimport(&bytes);
    assert!(matches!(
        &report.document.blocks[0].kind,
        BlockKind::Image { alt_text, .. } if alt_text.is_empty()
    ));
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
        ExportImage {
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

#[test]
fn image_layout_uses_drawingml_geometry_and_a_wrapped_anchor() {
    let png = tiny_png();
    let hash = opendoc_core::digest_bytes("sha256", &png)
        .unwrap()
        .to_string();
    let images = BTreeMap::from([(
        hash.clone(),
        ExportImage {
            media_type: "image/png".to_string(),
            bytes: png,
        },
    )]);
    let border = CellBorder::new(
        BorderStyle::Dashed,
        Length::from_twips(20).unwrap(),
        Color::parse("#123456").unwrap(),
    )
    .unwrap();
    let layout = ImageLayout {
        width: Some(Length::from_twips(1440).unwrap()),
        height: Some(Length::from_twips(720).unwrap()),
        placement: Some(ImagePlacement::WrapEnd),
        wrap_clearance: None,
        rotation_degrees: Some(90),
        opacity_percent: Some(60),
        crop: Some(ImageCrop {
            top_percent: 10,
            right_percent: 20,
            bottom_percent: 30,
            left_percent: 5,
        }),
        caption: Some("A real caption".to_string()),
        border: Some(border),
        positioned: None,
    };
    let source = document(vec![Block {
        id: StableId::parse("block-image-layout").unwrap(),
        kind: BlockKind::Image {
            blob_hash: hash,
            alt_text: "diagram".to_string(),
            layout: layout.clone(),
        },
        content: Vec::new(),
        properties: BlockProperties::default(),
    }]);
    let (bytes, warnings) = export_with(&source, &images);
    assert!(warnings.is_empty(), "{warnings:?}");
    let xml = part(&bytes, "word/document.xml");
    assert!(xml.contains("<wp:anchor"), "{xml}");
    assert!(xml.contains("<wp:align>right</wp:align>"), "{xml}");
    assert!(xml.contains("cx=\"914400\" cy=\"457200\""), "{xml}");
    assert!(
        xml.contains("<a:srcRect l=\"5000\" t=\"10000\" r=\"20000\" b=\"30000\"/>"),
        "{xml}"
    );
    assert!(xml.contains("<a:xfrm rot=\"5400000\">"), "{xml}");
    assert!(xml.contains("<a:alphaModFix amt=\"60000\"/>"), "{xml}");
    assert!(xml.contains("<a:ln w=\"12700\">"), "{xml}");
    assert!(xml.contains("<w:pStyle w:val=\"Caption\"/>"), "{xml}");
    let report = reimport(&bytes);
    let BlockKind::Image {
        layout: imported, ..
    } = &report.document.blocks[0].kind
    else {
        panic!("expected image")
    };
    assert_eq!(imported.width, layout.width);
    assert_eq!(imported.height, layout.height);
    assert_eq!(imported.placement, layout.placement);
    assert_eq!(imported.rotation_degrees, layout.rotation_degrees);
    assert_eq!(imported.opacity_percent, layout.opacity_percent);
    assert_eq!(imported.crop, layout.crop);
    assert_eq!(imported.border, layout.border);
    assert_eq!(imported.caption.as_deref(), Some("A real caption"));
    assert_eq!(
        report.document.blocks.len(),
        1,
        "caption should reattach to image"
    );
}

#[test]
fn bookmarked_caption_stays_a_visible_block_instead_of_losing_its_anchor() {
    let png = tiny_png();
    let hash = opendoc_core::digest_bytes("sha256", &png)
        .unwrap()
        .to_string();
    let images = BTreeMap::from([(
        hash.clone(),
        ExportImage {
            media_type: "image/png".to_string(),
            bytes: png,
        },
    )]);
    let source = document(vec![Block {
        id: StableId::parse("bookmarked-caption-image").unwrap(),
        kind: BlockKind::Image {
            blob_hash: hash,
            alt_text: "diagram".to_string(),
            layout: ImageLayout {
                caption: Some("Caption carrying bookmark".to_string()),
                ..ImageLayout::default()
            },
        },
        content: Vec::new(),
        properties: BlockProperties::default(),
    }]);
    let (bytes, warnings) = export_with(&source, &images);
    assert!(warnings.is_empty(), "{warnings:?}");
    let caption = r#"<w:p><w:pPr><w:pStyle w:val="Caption"/></w:pPr><w:r><w:t xml:space="preserve">Caption carrying bookmark</w:t></w:r></w:p>"#;
    let bookmarked_caption = r#"<w:p><w:pPr><w:pStyle w:val="Caption"/></w:pPr><w:bookmarkStart w:id="7" w:name="FigureCaption"/><w:r><w:t xml:space="preserve">Caption carrying bookmark</w:t></w:r><w:bookmarkEnd w:id="7"/></w:p>"#;
    let patched = rewrite_part(&bytes, "word/document.xml", |xml| {
        assert!(xml.contains(caption), "caption shape changed: {xml}");
        xml.replacen(caption, bookmarked_caption, 1)
    });

    let report = reimport(&patched);
    assert!(matches!(
        report.document.blocks.as_slice(),
        [
            Block { kind: BlockKind::Image { layout, .. }, .. },
            Block { kind: BlockKind::Paragraph, content, .. },
        ] if layout.caption.is_none()
            && matches!(content.as_slice(), [Inline::Text { text, marks, .. }]
                if text == "Caption carrying bookmark" && marks.is_empty())
    ));
    assert!(matches!(
        report.document.bookmarks.as_slice(),
        [Bookmark { name, block_id, .. }]
            if name == "FigureCaption" && block_id == &report.document.blocks[1].id
    ));
    assert!(!codes(&report.warnings).contains(&"docx-bookmark-range-unrepresentable"));
}

#[test]
fn double_image_border_warns_before_drawingml_solid_fallback() {
    let png = tiny_png();
    let hash = opendoc_core::digest_bytes("sha256", &png)
        .unwrap()
        .to_string();
    let images = BTreeMap::from([(
        hash.clone(),
        ExportImage {
            media_type: "image/png".to_string(),
            bytes: png,
        },
    )]);
    let border = CellBorder::new(
        BorderStyle::Double,
        Length::from_twips(20).unwrap(),
        Color::parse("#123456").unwrap(),
    )
    .unwrap();
    let source = document(vec![Block {
        id: StableId::parse("block-image-double-border").unwrap(),
        kind: BlockKind::Image {
            blob_hash: hash,
            alt_text: "double-bordered diagram".to_string(),
            layout: ImageLayout {
                border: Some(border),
                ..Default::default()
            },
        },
        content: Vec::new(),
        properties: BlockProperties::default(),
    }]);

    let (bytes, warnings) = export_with(&source, &images);
    assert_eq!(
        vec!["docx-export-image-double-border-as-solid"],
        codes(&warnings)
    );
    let xml = part(&bytes, "word/document.xml");
    assert!(xml.contains("<a:prstDash val=\"solid\"/>"), "{xml}");
}

#[test]
fn page_content_positioned_image_exports_as_native_docx_anchor() {
    let png = tiny_png();
    let hash = opendoc_core::digest_bytes("sha256", &png)
        .unwrap()
        .to_string();
    let images = BTreeMap::from([(
        hash.clone(),
        ExportImage {
            media_type: "image/png".to_string(),
            bytes: png,
        },
    )]);
    let source = document(vec![Block {
        id: StableId::parse("positioned-image").unwrap(),
        kind: BlockKind::Image {
            blob_hash: hash,
            alt_text: "positioned".to_string(),
            layout: ImageLayout {
                positioned: Some(PositionedImage {
                    anchor: PositionedImageAnchor::PageContent,
                    horizontal_offset: Length::from_twips(-240).unwrap(),
                    vertical_offset: Length::from_twips(480).unwrap(),
                    layer: PositionedImageLayer::BehindText,
                }),
                ..ImageLayout::default()
            },
        },
        content: Vec::new(),
        properties: BlockProperties::default(),
    }]);
    let (bytes, warnings) = export_with(&source, &images);
    assert!(warnings.is_empty(), "{warnings:?}");
    let xml = part(&bytes, "word/document.xml");
    assert!(xml.contains("<wp:anchor"), "{xml}");
    assert!(xml.contains("behindDoc=\"1\""), "{xml}");
    assert!(xml.contains("relativeFrom=\"margin\""), "{xml}");
    assert!(
        xml.contains("<wp:posOffset>-152400</wp:posOffset>"),
        "{xml}"
    );
    assert!(xml.contains("<wp:posOffset>304800</wp:posOffset>"), "{xml}");
    assert!(xml.contains("<wp:wrapNone"), "{xml}");
    let report = reimport(&bytes);
    let BlockKind::Image { layout, .. } = &report.document.blocks[0].kind else {
        panic!("positioned image was not re-imported as an image");
    };
    assert_eq!(
        layout.positioned.as_ref(),
        Some(&PositionedImage {
            anchor: PositionedImageAnchor::PageContent,
            horizontal_offset: Length::from_twips(-240).unwrap(),
            vertical_offset: Length::from_twips(480).unwrap(),
            layer: PositionedImageLayer::BehindText,
        })
    );
}

#[test]
fn block_anchored_positioned_image_warns_before_in_flow_docx_fallback() {
    let png = tiny_png();
    let hash = opendoc_core::digest_bytes("sha256", &png)
        .unwrap()
        .to_string();
    let images = BTreeMap::from([(
        hash.clone(),
        ExportImage {
            media_type: "image/png".to_string(),
            bytes: png,
        },
    )]);
    let anchor = StableId::parse("anchor-paragraph").unwrap();
    let source = document(vec![
        paragraph("anchor-paragraph", "anchor"),
        Block {
            id: StableId::parse("positioned-image").unwrap(),
            kind: BlockKind::Image {
                blob_hash: hash,
                alt_text: "positioned".to_string(),
                layout: ImageLayout {
                    positioned: Some(PositionedImage {
                        anchor: PositionedImageAnchor::Block(anchor),
                        horizontal_offset: Length::from_twips(-240).unwrap(),
                        vertical_offset: Length::from_twips(480).unwrap(),
                        layer: PositionedImageLayer::BehindText,
                    }),
                    ..ImageLayout::default()
                },
            },
            content: Vec::new(),
            properties: BlockProperties::default(),
        },
    ]);
    let (bytes, warnings) = export_with(&source, &images);
    assert_eq!(
        codes(&warnings),
        vec!["docx-export-positioned-image-as-inline"]
    );
    assert!(part(&bytes, "word/document.xml").contains("<wp:inline"));
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
fn title_and_subtitle_round_trip_through_standard_word_styles() {
    let source = document(vec![
        Block {
            id: StableId::parse("title-block").unwrap(),
            kind: BlockKind::Title,
            content: vec![text_inline("Title", Vec::new())],
            properties: BlockProperties::default(),
        },
        Block {
            id: StableId::parse("subtitle-block").unwrap(),
            kind: BlockKind::Subtitle,
            content: vec![text_inline("Subtitle", Vec::new())],
            properties: BlockProperties::default(),
        },
    ]);
    let (bytes, warnings) = export(&source);
    assert!(warnings.is_empty(), "{warnings:?}");
    let report = reimport(&bytes);
    assert!(matches!(report.document.blocks[0].kind, BlockKind::Title));
    assert!(matches!(
        report.document.blocks[1].kind,
        BlockKind::Subtitle
    ));
}

/// The bullet glyph WordprocessingML draws for the list item whose text is
/// `text`: `w:numId` off the paragraph, then that numbering definition's
/// `w:lvlText`. Going through the `w:numId` is the whole point — it is what
/// ties a glyph to the item that uses it.
fn list_glyph(body: &str, numbering: &str, text: &str) -> String {
    let paragraph_end = body
        .find(&format!("<w:t xml:space=\"preserve\">{text}</w:t>"))
        .or_else(|| body.find(&format!("<w:t>{text}</w:t>")))
        .unwrap_or_else(|| panic!("no list item reading {text:?} in {body}"));
    let paragraph_start = body[..paragraph_end]
        .rfind("<w:p>")
        .expect("a list item outside any paragraph");
    let num_id = between(
        &body[paragraph_start..paragraph_end],
        "<w:numId w:val=\"",
        "\"",
    )
    .unwrap_or_else(|| panic!("the paragraph reading {text:?} carries no w:numId"));
    let definition_start = numbering
        .find(&format!("<w:abstractNum w:abstractNumId=\"{num_id}\">"))
        .unwrap_or_else(|| panic!("no numbering definition {num_id} in {numbering}"));
    between(&numbering[definition_start..], "<w:lvlText w:val=\"", "\"")
        .expect("a numbering definition with no w:lvlText")
}

fn between(haystack: &str, open: &str, close: &str) -> Option<String> {
    let start = haystack.find(open)? + open.len();
    let end = haystack[start..].find(close)? + start;
    Some(haystack[start..end].to_string())
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
    // The two glyphs have to be tied to the two *states*, not merely both
    // present: a test that asserts only presence passes just as happily with
    // the constants swapped, and every done item then exports as an empty
    // box.
    let numbering = part(&bytes, "word/numbering.xml");
    let body = part(&bytes, "word/document.xml");
    assert_eq!(
        "\u{2610}",
        list_glyph(&body, &numbering, "todo"),
        "an unticked item did not export as an empty ballot box"
    );
    assert_eq!(
        "\u{2612}",
        list_glyph(&body, &numbering, "done"),
        "a ticked item did not export as a crossed ballot box"
    );

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
fn missing_image_effects_are_named_when_docx_uses_alt_text_fallback() {
    let border = CellBorder::new(
        BorderStyle::Dotted,
        twips(20),
        Color::parse("#336699").unwrap(),
    )
    .unwrap();
    let source = document(vec![Block {
        id: StableId::parse("missing-image-effects").unwrap(),
        kind: BlockKind::Image {
            blob_hash: "sha256:deadbeef".to_string(),
            alt_text: "the missing chart".to_string(),
            layout: ImageLayout {
                rotation_degrees: Some(30),
                opacity_percent: Some(60),
                border: Some(border),
                ..ImageLayout::default()
            },
        },
        content: Vec::new(),
        properties: BlockProperties::default(),
    }]);
    let (bytes, warnings) = export(&source);
    assert_eq!(
        codes(&warnings),
        vec![
            "docx-export-missing-image-blob",
            "docx-export-image-effects-unrepresentable",
        ]
    );
    assert!(warnings[1].message.contains("rotation, opacity, border"));
    let xml = part(&bytes, "word/document.xml");
    assert!(!xml.contains("w:drawing"));
    assert!(xml.contains("the missing chart"));
}

#[test]
fn missing_image_noop_effects_do_not_claim_visual_loss_in_docx_fallback() {
    let source = document(vec![Block {
        id: StableId::parse("missing-image-noop-effects").unwrap(),
        kind: BlockKind::Image {
            blob_hash: "sha256:deadbeef".to_string(),
            alt_text: "the missing chart".to_string(),
            layout: ImageLayout {
                opacity_percent: Some(100),
                border: Some(CellBorder::none()),
                ..ImageLayout::default()
            },
        },
        content: Vec::new(),
        properties: BlockProperties::default(),
    }]);
    let (_bytes, warnings) = export(&source);
    assert_eq!(vec!["docx-export-missing-image-blob"], codes(&warnings));
}

#[test]
fn a_missing_image_keeps_its_caption_as_visible_word_caption_text() {
    let source = document(vec![Block {
        id: StableId::parse("missing-image-with-caption").unwrap(),
        kind: BlockKind::Image {
            blob_hash: "sha256:deadbeef".to_string(),
            alt_text: "the missing chart".to_string(),
            layout: ImageLayout {
                caption: Some("Figure 1: retained caption".to_string()),
                ..ImageLayout::default()
            },
        },
        content: Vec::new(),
        properties: BlockProperties::default(),
    }]);

    let (bytes, warnings) = export(&source);
    assert_eq!(
        codes(&warnings),
        vec![
            "docx-export-missing-image-blob",
            "docx-export-image-caption-without-image",
        ]
    );
    let xml = part(&bytes, "word/document.xml");
    assert!(xml.contains("<w:pStyle w:val=\"Caption\"/>"), "{xml}");
    assert!(xml.contains("Figure 1: retained caption"), "{xml}");
    let report = reimport(&bytes);
    assert_eq!(
        report.document.visible_text(),
        "the missing chart\nFigure 1: retained caption\n"
    );
}

/// The DOCX writer produces no comments part and no tracked changes, so a
/// document carrying either loses it. The fixture has to *carry* both, or the
/// test asserts only that a document with nothing to drop drops nothing —
/// which is how `docx-export-dropped-comments` and
/// `docx-export-dropped-suggestions` came to appear in no test at all.
#[test]
fn comments_suggestions_and_the_doi_are_named_rather_than_vanishing() {
    let mut source = draft(vec![paragraph("block-one", "body")]);
    source.doi = Some("10.1234/opendoc".to_string());
    source.comments = vec![opendoc_core::CommentThread {
        id: StableId::parse("thread-one").unwrap(),
        anchor: opendoc_core::Anchor::Document,
        comments: vec![opendoc_core::Comment {
            id: StableId::parse("comment-one").unwrap(),
            author: "Ada".to_string(),
            body: vec![text_inline("a note", Vec::new())],
            created_at_ms: 0,
            deleted: false,
        }],
        state: opendoc_core::CommentThreadState::Open,
        resolved_by: None,
        resolved_at_ms: None,
        action_assignee: None,
        action_due_at_ms: None,
        action_completed_by: None,
        action_completed_at_ms: None,
        reactions: Vec::new(),
        deleted: false,
    }];
    source.suggestions = vec![opendoc_core::Suggestion {
        id: StableId::parse("suggestion-one").unwrap(),
        author: "Ada".to_string(),
        kind: opendoc_core::SuggestionKind::Insert {
            anchor: opendoc_core::Anchor::Document,
            content: vec![text_inline("a proposal", Vec::new())],
        },
        state: opendoc_core::SuggestionState::Proposed,
        provenance: Vec::new(),
    }];
    source.validate().unwrap();
    let (bytes, warnings) = export(&source);
    let mut found = codes(&warnings);
    found.sort_unstable();
    assert_eq!(
        vec![
            "docx-export-dropped-comments",
            "docx-export-dropped-doi",
            "docx-export-dropped-suggestions",
        ],
        found
    );
    // And the package really does not carry them: the warning is not a
    // consolation for something that arrived anyway.
    assert!(!entries(&bytes).iter().any(|name| name.contains("comments")));
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
        ExportImage {
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
        ..PageSetup::default()
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

/// Furniture is a block fragment, not a string.  This pins the guarantee the
/// desktop plain-furniture form relies on: a header carrying an inline mark
/// and a paragraph property survives DOCX export and import without being
/// normalized to plain text.
#[test]
fn rich_header_furniture_round_trips_as_blocks() {
    let mut source = document(vec![paragraph("body-one", "body")]);
    let mut header = Block {
        id: StableId::parse("header-rich").unwrap(),
        kind: BlockKind::Paragraph,
        content: vec![
            text_inline("Running ", Vec::new()),
            text_inline("head", vec![mark(MarkKind::Bold, None)]),
        ],
        properties: BlockProperties::default(),
    };
    header.properties.alignment = Some(Alignment::Center);
    source.header = vec![header];
    source.validate().unwrap();

    let warnings = assert_round_trips(&source);
    assert!(warnings.is_empty(), "{warnings:?}");
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
        ..PageSetup::default()
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

#[test]
fn generated_toc_exports_as_a_native_word_field_without_static_cache() {
    let toc = Block {
        id: StableId::parse("toc").unwrap(),
        kind: BlockKind::TableOfContents { max_level: 3 },
        content: Vec::new(),
        properties: BlockProperties::default(),
    };
    let heading = Block {
        id: StableId::parse("heading").unwrap(),
        kind: BlockKind::Heading { level: 1 },
        content: vec![text_inline("A heading", Vec::new())],
        properties: BlockProperties::default(),
    };
    let source = document(vec![toc, heading]);
    let (bytes, warnings) = export(&source);
    let xml = part(&bytes, "word/document.xml");
    assert!(
        xml.contains(r#"<w:fldSimple w:instr=" TOC \o &quot;1-3&quot; \h \z \u ">"#),
        "{xml}"
    );
    assert!(xml.contains("Table of contents"), "{xml}");
    assert!(!xml.contains("docx-export-toc-as-static-placeholder"));
    assert!(warnings.is_empty(), "{warnings:?}");
    let imported = reimport(&bytes);
    assert!(matches!(
        imported.document.blocks.as_slice(),
        [
            Block {
                kind: BlockKind::TableOfContents { max_level: 3 },
                ..
            },
            Block {
                kind: BlockKind::Heading { level: 1 },
                ..
            }
        ]
    ));
}

/// First-page furniture is document-wide (not section-local), but it has a
/// native WordprocessingML representation and must not be applied to every
/// page while importing.
#[test]
fn a_first_page_header_variant_imports_as_a_first_page_override() {
    let mut source = document(vec![paragraph("block-one", "body")]);
    source.header = vec![paragraph("header-one", "default header")];
    source.validate().unwrap();
    let (bytes, _) = export(&source);
    // Re-point the default header reference at a `first` one.
    let patched = rewrite_part(&bytes, "word/document.xml", |xml| {
        xml.replace(r#"w:type="default""#, r#"w:type="first""#)
    });
    let report = import_docx_bytes(TITLE, &patched).expect("import failed");
    assert!(
        report.document.header.is_empty(),
        "the first-page value became default furniture"
    );
    assert!(matches!(
        report.document.first_page_header.as_deref(),
        Some([Block { content, .. }])
            if matches!(content.as_slice(), [Inline::Text { text, .. }] if text == "default header")
    ));
    assert!(!codes(&report.warnings).contains(&"docx-dropped-header-footer"));
}

#[test]
fn even_page_furniture_round_trips_with_word_setting() {
    let mut source = draft(vec![paragraph("block-one", "body")]);
    source.header = vec![paragraph("header-odd", "odd header")];
    source.even_page_header = Some(vec![paragraph("header-even", "even header")]);
    source.even_page_footer = Some(vec![paragraph("footer-even", "even footer")]);
    source.validate().unwrap();
    let (bytes, warnings) = export(&source);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(part(&bytes, "word/document.xml").contains(r#"w:type="even""#));
    assert!(part(&bytes, "word/settings.xml").contains("w:evenAndOddHeaders"));
    let imported = reimport(&bytes).document;
    assert!(matches!(
        imported.even_page_header.as_deref(),
        Some([Block { content, .. }]) if matches!(content.as_slice(), [Inline::Text { text, .. }] if text == "even header")
    ));
    assert!(matches!(
        imported.even_page_footer.as_deref(),
        Some([Block { content, .. }]) if matches!(content.as_slice(), [Inline::Text { text, .. }] if text == "even footer")
    ));
}

#[test]
fn empty_first_and_even_furniture_overrides_round_trip_as_explicit_suppression() {
    let mut source = draft(vec![paragraph("block-one", "body")]);
    source.header = vec![paragraph("header-odd", "ordinary header")];
    source.footer = vec![paragraph("footer-odd", "ordinary footer")];
    source.first_page_header = Some(Vec::new());
    source.even_page_footer = Some(Vec::new());
    source.validate().unwrap();

    let (bytes, warnings) = export(&source);
    assert!(warnings.is_empty(), "{warnings:?}");
    let xml = part(&bytes, "word/document.xml");
    assert!(xml.contains(r#"w:type="first""#), "{xml}");
    assert!(xml.contains(r#"w:type="even""#), "{xml}");
    assert!(entries(&bytes).contains(&"word/header2.xml".to_string()));
    assert!(entries(&bytes).contains(&"word/footer3.xml".to_string()));
    assert!(part(&bytes, "word/header2.xml").contains("<w:p/>"));
    assert!(part(&bytes, "word/footer3.xml").contains("<w:p/>"));

    let imported = reimport(&bytes).document;
    assert!(matches!(
        imported.header.as_slice(),
        [Block { content, .. }] if matches!(content.as_slice(), [Inline::Text { text, .. }] if text == "ordinary header")
    ));
    assert!(matches!(
        imported.footer.as_slice(),
        [Block { content, .. }] if matches!(content.as_slice(), [Inline::Text { text, .. }] if text == "ordinary footer")
    ));
    assert_eq!(imported.first_page_header, Some(Vec::new()));
    assert_eq!(imported.even_page_footer, Some(Vec::new()));
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

// ---------------------------------------------------------------------------
// A foreign producer's table, out through this writer and back
// ---------------------------------------------------------------------------

/// The same LibreOffice 7.3 package `docx_tests` imports, exported again.
///
/// A round trip that starts from a document this file built proves the writer
/// agrees with the reader; it proves nothing about a table anybody else wrote.
/// This one starts from bytes LibreOffice produced, so the grid under test was
/// authored somewhere else entirely.
const LIBREOFFICE_PACKAGE: &[u8] = include_bytes!("../fixtures/libreoffice-73-merged-table.docx");

/// P1-4's second half, against a table this repository did not author.
///
/// The export used to write no `w:gridSpan`, no `w:vMerge`, an equalised grid
/// and no cell styling at all, and said nothing about any of it — so a
/// LibreOffice table opened and saved came back a different table, silently.
#[test]
fn a_libreoffice_table_survives_being_exported_and_read_back() {
    let imported = import_docx_bytes("libreoffice", LIBREOFFICE_PACKAGE)
        .expect("the package is readable")
        .document;
    let (bytes, warnings) = export(&imported);
    let body = part(&bytes, "word/document.xml");
    // LibreOffice's own twips, written through unchanged rather than shared
    // out equally between three columns.
    assert!(
        body.contains(r#"<w:gridCol w:w="1819"/>"#)
            && body.contains(r#"<w:gridCol w:w="495"/>"#)
            && body.contains(r#"<w:gridCol w:w="500"/>"#),
        "the column widths did not survive: {body}"
    );
    for (what, expected) in [
        ("the column span", r#"<w:gridSpan w:val="2"/>"#),
        ("the row span", r#"<w:vMerge w:val="restart"/>"#),
        (
            "the row span's continuation cell",
            r#"<w:vMerge w:val="continue"/>"#,
        ),
        (
            "the cell background",
            r#"<w:shd w:val="clear" w:color="auto" w:fill="ffcc00"/>"#,
        ),
        (
            "the red top border",
            r#"<w:top w:val="single" w:sz="18" w:space="0" w:color="ff0000"/>"#,
        ),
        (
            "the blue dashed bottom border",
            r#"<w:bottom w:val="dashed" w:sz="2" w:space="0" w:color="0000ff"/>"#,
        ),
        ("the vertical alignment", r#"<w:vAlign w:val="center"/>"#),
        (
            "the inherited cell padding",
            r#"<w:left w:w="0" w:type="dxa"/>"#,
        ),
    ] {
        assert!(
            body.contains(expected),
            "{what} was not written as {expected}: {body}"
        );
    }
    // Nothing in this table is beyond WordprocessingML, so nothing about it
    // is reported: ADR 0010 asks for a name per loss, not noise.
    assert!(
        !codes(&warnings).contains(&"docx-export-dropped-covered-cell-content"),
        "{warnings:?}"
    );

    // And the grid itself, read back out of the package this writer produced.
    let report = reimport(&bytes);
    let BlockKind::Table { columns, rows, .. } = &report.document.blocks[1].kind else {
        panic!("expected a table, got {:?}", report.document.blocks[1].kind);
    };
    assert_eq!(
        vec![Some(1819), Some(495), Some(500)],
        columns
            .iter()
            .map(|column| column.width.map(|width| width.twips()))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        vec![
            vec!["A1+B1 merged across", "", "C1"],
            vec!["A2 spans down", "B2", "C2"],
            vec!["", "B3", "C3"],
        ],
        rows.iter()
            .map(|row| row
                .cells
                .iter()
                .map(|cell| cell
                    .blocks
                    .iter()
                    .flat_map(|block| block.content.iter())
                    .map(|inline| match inline {
                        Inline::Text { text, .. } => text.as_str(),
                        _ => "",
                    })
                    .collect::<String>())
                .collect::<Vec<_>>())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        (1, 2),
        (
            rows[0].cells[0].span.rows(),
            rows[0].cells[0].span.columns()
        )
    );
    assert_eq!(
        (2, 1),
        (
            rows[1].cells[0].span.rows(),
            rows[1].cells[0].span.columns()
        )
    );
    assert_eq!(
        Some("#ffcc00".to_string()),
        rows[0].cells[0].properties.background.map(|c| c.as_hex())
    );
    assert_eq!(
        Some(VerticalAlignment::Middle),
        rows[0].cells[0].properties.vertical_alignment
    );
    let top = rows[0].cells[0]
        .properties
        .border_top
        .expect("a top border");
    assert_eq!(
        (BorderStyle::Solid, 45, "#ff0000".to_string()),
        (top.style(), top.width().twips(), top.color().as_hex())
    );
    assert_eq!(
        Some(60),
        rows[1].cells[0].properties.padding_top.map(|p| p.twips())
    );
}

// ---------------------------------------------------------------------------
// The border grid the export used to invent
// ---------------------------------------------------------------------------

/// The export used to write a `single sz=4 color=auto` `w:tblBorders` on
/// **every** table, whatever the document said.
///
/// It was an attempt to materialise the editor's own `.doc-table td` hairline,
/// and it made the export lie: the model has no table-level border, so the
/// grid was not something the document said, and it overrode nothing the
/// document *did* say only because the reader ignored `w:tblBorders`
/// entirely. Now that the reader resolves that grid onto the cells, writing
/// one unconditionally would give every re-imported table four borders per
/// cell that its author never asked for.
#[test]
fn a_table_nobody_set_a_border_on_is_exported_without_one() {
    let table = Block {
        id: StableId::parse("block-plain-table").unwrap(),
        kind: BlockKind::Table {
            columns: vec![TableColumn::auto(), TableColumn::auto()],
            properties: Default::default(),
            rows: vec![
                TableRow {
                    id: StableId::parse("row-0").unwrap(),
                    height: None,
                    header: false,
                    cells: vec![
                        TableCell::new(vec![paragraph("block-0-0", "a")]),
                        TableCell::new(vec![paragraph("block-0-1", "b")]),
                    ],
                },
                TableRow {
                    id: StableId::parse("row-1").unwrap(),
                    height: None,
                    header: false,
                    cells: vec![
                        TableCell::new(vec![paragraph("block-1-0", "c")]),
                        TableCell::new(vec![paragraph("block-1-1", "d")]),
                    ],
                },
            ],
        },
        content: Vec::new(),
        properties: BlockProperties::default(),
    };
    let source = document(vec![table]);
    let (bytes, warnings) = export(&source);
    let body = part(&bytes, "word/document.xml");
    assert!(
        !body.contains("w:tblBorders"),
        "the export invented a table border grid the document never states: {body}"
    );
    assert!(
        !body.contains("w:tcBorders"),
        "the export invented a cell border the document never states: {body}"
    );
    // Nothing was lost, so nothing is reported: the editor's gridlines are a
    // view default, not a property of the document (ADR 0010 asks for a name
    // per loss, not noise).
    assert!(warnings.is_empty(), "{warnings:?}");
    // And the table still comes back as the table that went in.
    assert_round_trips(&source);
}

#[test]
fn a_uniform_table_border_round_trips_as_table_state() {
    let border = CellBorder::new(
        opendoc_core::BorderStyle::Dashed,
        twips(20),
        opendoc_core::Color::parse("#336699").unwrap(),
    )
    .unwrap();
    let source = document(vec![Block {
        id: StableId::parse("table-border-round-trip").unwrap(),
        kind: BlockKind::Table {
            columns: vec![TableColumn::auto(), TableColumn::auto()],
            properties: opendoc_core::TableProperties {
                border: Some(border),
                alignment: Some(opendoc_core::TableAlignment::Center),
            },
            rows: vec![TableRow {
                id: StableId::parse("row-border-round-trip").unwrap(),
                height: None,
                header: false,
                cells: vec![
                    TableCell::new(vec![paragraph("border-a", "a")]),
                    TableCell::new(vec![paragraph("border-b", "b")]),
                ],
            }],
        },
        content: Vec::new(),
        properties: BlockProperties::default(),
    }]);
    let (bytes, warnings) = export(&source);
    assert!(warnings.is_empty(), "{warnings:?}");
    let body = part(&bytes, "word/document.xml");
    assert!(body.contains("<w:tblBorders>"), "{body}");
    assert!(body.contains("<w:jc w:val=\"center\"/>"), "{body}");
    assert!(body.contains(r#"w:val="dashed""#), "{body}");

    let imported = reimport(&bytes);
    let BlockKind::Table {
        properties, rows, ..
    } = &imported.document.blocks[0].kind
    else {
        panic!("expected table");
    };
    assert_eq!(properties.border, Some(border));
    assert_eq!(
        properties.alignment,
        Some(opendoc_core::TableAlignment::Center)
    );
    assert!(rows
        .iter()
        .flat_map(|row| &row.cells)
        .all(|cell| cell.properties.is_empty()));
}

/// The writer and the reader are inverses on borders, in both directions.
///
/// A table-level grid on the way *in* lands on the cells; those cells' edges
/// on the way *out* are `w:tcBorders`; reading that back gives the same
/// model. The second round trip is what proves the pair closed — the first
/// could be satisfied by a writer that dropped every border and a reader that
/// invented the same ones back.
#[test]
fn a_table_level_grid_survives_as_cell_borders_and_stops_moving() {
    let source = import_docx_bytes(TITLE, &package_with_table_border_grid())
        .expect("the package is readable")
        .document;
    let (bytes, _) = export(&source);
    let body = part(&bytes, "word/document.xml");
    assert!(
        !body.contains("w:tblBorders"),
        "the model has no table-level border, so the export has none to write: {body}"
    );
    assert!(
        body.contains(r#"<w:top w:val="dashed" w:sz="16" w:space="0" w:color="ff0000"/>"#),
        "the table's own top edge did not reach the first row's cells: {body}"
    );
    let again = reimport(&bytes);
    assert_eq!(
        normalized(&source),
        normalized(&again.document),
        "a table whose borders came from a w:tblBorders did not survive the round trip"
    );
}

/// A `w:tbl` whose only borders are a table-level grid: two rows, two
/// columns, an outer edge and an interior one that are told apart by colour.
fn package_with_table_border_grid() -> Vec<u8> {
    let body = r#"<w:tbl>
    <w:tblPr>
      <w:tblBorders>
        <w:top w:val="dashed" w:sz="16" w:space="0" w:color="FF0000"/>
        <w:bottom w:val="dashed" w:sz="16" w:space="0" w:color="FF0000"/>
        <w:left w:val="dashed" w:sz="16" w:space="0" w:color="FF0000"/>
        <w:right w:val="dashed" w:sz="16" w:space="0" w:color="FF0000"/>
        <w:insideH w:val="dotted" w:sz="8" w:space="0" w:color="0000FF"/>
        <w:insideV w:val="dotted" w:sz="8" w:space="0" w:color="0000FF"/>
      </w:tblBorders>
    </w:tblPr>
    <w:tblGrid><w:gridCol w:w="1200"/><w:gridCol w:w="1200"/></w:tblGrid>
    <w:tr><w:tc><w:p><w:r><w:t>a</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>b</w:t></w:r></w:p></w:tc></w:tr>
    <w:tr><w:tc><w:p><w:r><w:t>c</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>d</w:t></w:r></w:p></w:tc></w:tr>
  </w:tbl>"#;
    // Built by dropping the table into a package this writer produced, so
    // everything around it — content types, relationships, `w:sectPr` — is a
    // package the reader already accepts, and the only thing under test is
    // the `w:tbl`.
    let (bytes, _) = export(&document(vec![paragraph("block-anchor", "anchor")]));
    rewrite_part(&bytes, "word/document.xml", |xml| {
        let marker = if xml.contains("<w:sectPr") {
            "<w:sectPr"
        } else {
            "</w:body>"
        };
        xml.replacen(marker, &format!("{body}{marker}"), 1)
    })
}

/// The LibreOffice package's table is borderless, and it is still borderless
/// after a trip through this writer.
///
/// This is the assertion the round trip through *LibreOffice* makes from the
/// outside: converting the exported `.docx` back to `.odt` used to turn every
/// `fo:border="none"` into `fo:border="0.5pt solid #000000"`. Re-importing it
/// here catches the same thing without needing `soffice` on the machine.
#[test]
fn a_borderless_libreoffice_table_is_still_borderless_after_a_round_trip() {
    let imported = import_docx_bytes("libreoffice", LIBREOFFICE_PACKAGE)
        .expect("the package is readable")
        .document;
    let (bytes, _) = export(&imported);
    let report = reimport(&bytes);
    let BlockKind::Table { rows, .. } = &report.document.blocks[1].kind else {
        panic!("expected a table, got {:?}", report.document.blocks[1].kind);
    };
    // The one cell that states borders keeps exactly the two it states.
    let stated = &rows[0].cells[0].properties;
    assert_eq!(
        (
            stated.border_top.map(|b| b.width().twips()),
            stated.border_bottom.map(|b| b.width().twips()),
            stated.border_start,
            stated.border_end,
        ),
        (Some(45), Some(5), None, None),
        "the merged cell's stated borders changed, or it gained ones it never had"
    );
    for (row_index, row) in rows.iter().enumerate() {
        for (column_index, cell) in row.cells.iter().enumerate() {
            if (row_index, column_index) == (0, 0) {
                continue;
            }
            let properties = &cell.properties;
            assert_eq!(
                (None, None, None, None),
                (
                    properties.border_top,
                    properties.border_bottom,
                    properties.border_start,
                    properties.border_end,
                ),
                "cell ({row_index}, {column_index}) came back with a border the package never had"
            );
        }
    }
}

#[test]
fn bookmarks_on_text_blocks_are_native_zero_width_docx_ranges() {
    let mut source = document(vec![paragraph("target", "here")]);
    source.bookmarks.push(Bookmark {
        id: StableId::parse("bookmark-intro").unwrap(),
        name: "Intro".to_string(),
        block_id: StableId::parse("target").unwrap(),
        revision: 1,
        deleted: false,
    });
    let (bytes, warnings) = export(&source);
    let xml = part(&bytes, "word/document.xml");
    assert!(xml.contains("<w:bookmarkStart w:id=\"1\" w:name=\"Intro\"/>"));
    assert!(xml.contains("<w:bookmarkEnd w:id=\"1\"/>"));
    assert!(!codes(&warnings).contains(&"docx-export-unplaced-bookmarks"));
}

#[test]
fn explicit_cell_row_headers_warn_instead_of_becoming_word_first_column_formatting() {
    let mut rows = vec![TableRow::empty(1)];
    rows[0].cells[0].properties.row_header = Some(true);
    let source = document(vec![Block {
        id: StableId::new("table"),
        kind: BlockKind::table(rows),
        content: Vec::new(),
        properties: BlockProperties::default(),
    }]);

    let (_, warnings) = export(&source);
    assert!(codes(&warnings).contains(&"docx-export-dropped-table-row-header"));
}
