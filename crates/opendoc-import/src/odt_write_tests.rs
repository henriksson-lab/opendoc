//! ODT writer tests.
//!
//! There is no ODF reader here, so the round trip that pins `docx_write.rs`
//! is not available and is not faked. Three things stand in for it.
//!
//! * **The XML is asserted directly.** Every mapping that could drift — the
//!   twip-to-point conversion, the three line-spacing rules, the
//!   right-to-left indent swap, merged cells and their covered positions, the
//!   header height arithmetic, whitespace — is checked as the exact string
//!   the writer produces, over values no other conversion could reproduce.
//! * **The package shape is asserted**, because ODF is strict about it: the
//!   `mimetype` entry has to be first and uncompressed, and the manifest has
//!   to list every other part.
//! * **External readers were used outside the test suite**, and what they
//!   said is recorded in the module documentation of `odt_write.rs` rather
//!   than pretended here: `xmllint --relaxng` against the OpenDocument 1.3
//!   schema, LibreOffice (which rendered it and converted it back to `.docx`,
//!   re-imported through this crate's own DOCX reader) and pandoc.

use crate::{export_odt_with_warnings, mark, ExportImage, ODT_MEDIA_TYPE};
use opendoc_core::{
    Alignment, Block, BlockKind, BlockProperties, Bookmark, BorderStyle, CellBorder, CellSpan,
    Color, Document, Equation, EquationSourceFormat, Footnote, ImageCrop, ImageLayout,
    ImagePlacement, Inline, Length, LineHeightMultiple, LineSpacing, ListKind, Mark, MarkKind,
    ModelWarning, PageNumberField, PageSetup, PositionedImage, PositionedImageAnchor,
    PositionedImageLayer, StableId, TableCell, TableCellProperties, TableColumn, TableRow,
    TextDirection, VerticalAlignment,
};
use std::collections::BTreeMap;
use std::io::Read;

const TITLE: &str = "Open Document";
/// A content hash of the shape the model validates.
const HASH: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const MISSING_HASH: &str =
    "sha256:2222222222222222222222222222222222222222222222222222222222222222";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn no_images() -> BTreeMap<String, ExportImage> {
    BTreeMap::new()
}

struct Package {
    parts: Vec<(String, Vec<u8>)>,
    stored: Vec<String>,
    warnings: Vec<ModelWarning>,
}

impl Package {
    fn part(&self, name: &str) -> String {
        String::from_utf8(self.bytes(name)).expect("part is not UTF-8")
    }

    fn bytes(&self, name: &str) -> Vec<u8> {
        self.parts
            .iter()
            .find(|(path, _)| path == name)
            .map(|(_, bytes)| bytes.clone())
            .unwrap_or_else(|| panic!("no part {name} in {:?}", self.names()))
    }

    fn names(&self) -> Vec<&str> {
        self.parts.iter().map(|(path, _)| path.as_str()).collect()
    }

    fn content(&self) -> String {
        self.part("content.xml")
    }

    fn styles(&self) -> String {
        self.part("styles.xml")
    }

    fn codes(&self) -> Vec<&str> {
        self.warnings
            .iter()
            .map(|warning| warning.code.as_str())
            .collect()
    }
}

fn export(document: &Document) -> Package {
    export_with(document, &no_images())
}

#[test]
fn generated_toc_exports_as_a_native_odf_index_without_static_entries() {
    let toc = Block {
        id: StableId::parse("toc").unwrap(),
        kind: BlockKind::TableOfContents { max_level: 3 },
        content: Vec::new(),
        properties: BlockProperties::default(),
    };
    let heading = Block {
        id: StableId::parse("heading").unwrap(),
        kind: BlockKind::Heading { level: 1 },
        content: vec![Inline::text("A heading")],
        properties: BlockProperties::default(),
    };
    let package = export(&document(vec![toc, heading]));
    let content = package.content();
    assert!(
        content.contains(
            r#"<text:table-of-content text:name="OpenDocTOC_toc" text:protected="true">"#
        ),
        "{content}"
    );
    assert!(
        content.contains(r#"<text:table-of-content-source text:outline-level="3">"#),
        "{content}"
    );
    assert!(
        content.contains(r#"<text:index-entry-page-number/>"#),
        "{content}"
    );
    assert_eq!(content.matches("A heading").count(), 1, "{content}");
    assert!(package.warnings.is_empty(), "{:?}", package.warnings);
}

fn export_with(document: &Document, images: &BTreeMap<String, ExportImage>) -> Package {
    let (bytes, warnings) = export_odt_with_warnings(document, images).expect("export failed");
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("export is not a zip package");
    let mut parts = Vec::new();
    let mut stored = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).expect("zip entry");
        let name = entry.name().to_string();
        if entry.compression() == zip::CompressionMethod::Stored {
            stored.push(name.clone());
        }
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).expect("zip entry body");
        parts.push((name, bytes));
    }
    Package {
        parts,
        stored,
        warnings,
    }
}

fn document(blocks: Vec<Block>) -> Document {
    let document = draft(blocks);
    document.validate().expect("test document is invalid");
    document
}

fn draft(blocks: Vec<Block>) -> Document {
    let mut document = Document::new(TITLE);
    document.blocks = blocks;
    document
}

fn paragraph(text: &str) -> Block {
    block(BlockKind::Paragraph, vec![text_inline(text, Vec::new())])
}

fn styled_paragraph(text: &str, properties: BlockProperties) -> Block {
    Block {
        properties,
        ..paragraph(text)
    }
}

fn block(kind: BlockKind, content: Vec<Inline>) -> Block {
    Block {
        id: StableId::new("block"),
        kind,
        content,
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

fn list_item(text: &str, list_id: &StableId, level: u8, kind: ListKind) -> Block {
    Block {
        id: StableId::new("block"),
        kind: BlockKind::ListItem {
            list_id: list_id.clone(),
            level,
            kind,
        },
        content: vec![text_inline(text, Vec::new())],
        properties: BlockProperties::default(),
    }
}

fn twips(value: i32) -> Length {
    Length::from_twips(value).unwrap()
}

fn cell(text: &str) -> TableCell {
    TableCell::new(vec![paragraph(text)])
}

fn row(cells: Vec<TableCell>) -> TableRow {
    TableRow {
        id: StableId::new("row"),
        height: None,
        header: false,
        cells,
    }
}

/// A 2x3 PNG, so the pixel size in the bytes is unmistakable.
fn png() -> ExportImage {
    ExportImage {
        media_type: "image/png".to_string(),
        bytes: vec![
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x03, 0x08, 0x02, 0x00, 0x00,
            0x00, 0x36, 0x88, 0x49, 0xd6, 0x00, 0x00, 0x00, 0x10, 0x49, 0x44, 0x41, 0x54, 0x78,
            0xda, 0x63, 0x38, 0x61, 0x64, 0x04, 0x44, 0x0c, 0x28, 0x14, 0x00, 0x4d, 0x6d, 0x07,
            0x09, 0x59, 0xaa, 0x18, 0x7e, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae,
            0x42, 0x60, 0x82,
        ],
    }
}

// ---------------------------------------------------------------------------
// Package shape
// ---------------------------------------------------------------------------

/// ODF puts the media type at a fixed offset inside the zip so a reader can
/// identify the file without unzipping it. That only works if `mimetype` is
/// the first entry and is stored rather than deflated.
#[test]
fn the_package_starts_with_an_uncompressed_mimetype_entry() {
    let package = export(&document(vec![paragraph("Hello")]));
    assert_eq!("mimetype", package.names()[0]);
    assert_eq!(ODT_MEDIA_TYPE, package.part("mimetype"));
    assert_eq!(
        vec!["mimetype".to_string()],
        package.stored,
        "only mimetype may be stored uncompressed"
    );
    assert_eq!(
        vec![
            "mimetype",
            "META-INF/manifest.xml",
            "content.xml",
            "styles.xml",
            "meta.xml"
        ],
        package.names()
    );
}

/// A part a reader cannot find in the manifest is a part it may refuse to
/// read, so the listing is not optional.
#[test]
fn the_manifest_lists_every_part_including_pictures() {
    let document = document(vec![block(
        BlockKind::Image {
            blob_hash: HASH.to_string(),
            alt_text: "a square".to_string(),
            layout: ImageLayout::default(),
        },
        Vec::new(),
    )]);
    let images = BTreeMap::from([(HASH.to_string(), png())]);
    let package = export_with(&document, &images);
    let manifest = package.part("META-INF/manifest.xml");
    for path in ["content.xml", "styles.xml", "meta.xml"] {
        assert!(
            manifest.contains(&format!(
                "<manifest:file-entry manifest:full-path=\"{path}\" manifest:media-type=\"text/xml\"/>"
            )),
            "{path} is not in the manifest: {manifest}"
        );
    }
    assert!(manifest.contains(
        "<manifest:file-entry manifest:full-path=\"Pictures/image1.png\" manifest:media-type=\"image/png\"/>"
    ));
    assert!(
        manifest.contains("manifest:full-path=\"/\" manifest:version=\"1.3\""),
        "the package root entry carries the document's media type"
    );
    assert!(
        !manifest.contains("mimetype"),
        "mimetype is the magic number, not a part"
    );
    assert_eq!(png().bytes, package.bytes("Pictures/image1.png"));
}

#[test]
fn image_media_type_parameters_do_not_drop_a_supported_odt_picture() {
    // No format signature is supplied, so this covers the declared MIME
    // essence rather than the separate conservative byte-sniff fallback.
    let document = document(vec![block(
        BlockKind::Image {
            blob_hash: HASH.to_string(),
            alt_text: "a WebP figure".to_string(),
            layout: ImageLayout::default(),
        },
        Vec::new(),
    )]);
    let source = b"webp source bytes".to_vec();
    let images = BTreeMap::from([(
        HASH.to_string(),
        ExportImage {
            media_type: " Image/WebP; codecs=vp8 ".to_string(),
            bytes: source.clone(),
        },
    )]);

    let package = export_with(&document, &images);
    assert!(
        !package
            .codes()
            .contains(&"odt-export-unsupported-image-media-type"),
        "parameterized WebP fell back to alt text: {:?}",
        package.warnings
    );
    assert_eq!(source, package.bytes("Pictures/image1.webp"));
    assert!(package.part("META-INF/manifest.xml").contains(
        "<manifest:file-entry manifest:full-path=\"Pictures/image1.webp\" manifest:media-type=\"image/webp\"/>"
    ));
}

/// No clock is read anywhere, so one document is one package.
#[test]
fn the_same_document_exports_to_the_same_bytes() {
    let document = document(vec![paragraph("Deterministic")]);
    let first = export_odt_with_warnings(&document, &no_images()).unwrap().0;
    let second = export_odt_with_warnings(&document, &no_images()).unwrap().0;
    assert_eq!(first, second);
}

#[test]
fn the_title_and_locale_travel_in_the_metadata() {
    let package = export(&document(vec![paragraph("Hello")]));
    let meta = package.part("meta.xml");
    assert!(meta.contains("<dc:title>Open Document</dc:title>"));
    assert!(meta.contains("<dc:language>en-US</dc:language>"));
    assert!(meta.contains("<meta:generator>OpenDoc</meta:generator>"));
}

// ---------------------------------------------------------------------------
// Text
// ---------------------------------------------------------------------------

#[test]
fn paragraphs_and_headings_use_their_named_styles() {
    let package = export(&document(vec![
        block(
            BlockKind::Heading { level: 2 },
            vec![text_inline("Title", Vec::new())],
        ),
        paragraph("Body"),
    ]));
    let content = package.content();
    assert!(content.contains(
        "<text:h text:style-name=\"Heading_20_2\" text:outline-level=\"2\">Title</text:h>"
    ));
    assert!(content.contains("<text:p text:style-name=\"Standard\">Body</text:p>"));
    assert!(package.codes().is_empty(), "{:?}", package.warnings);
    // The heading styles carry the same sizes the DOCX writer's do, so the
    // two exports of one document look alike.
    assert!(package
        .styles()
        .contains("style:name=\"Heading_20_2\" style:display-name=\"Heading 2\""));
    assert!(package
        .styles()
        .contains("<style:text-properties fo:font-size=\"16pt\" fo:font-weight=\"bold\"/>"));
}

#[test]
fn title_and_subtitle_use_named_non_outline_styles() {
    let package = export(&document(vec![
        block(BlockKind::Title, vec![text_inline("Title", Vec::new())]),
        block(
            BlockKind::Subtitle,
            vec![text_inline("Subtitle", Vec::new())],
        ),
    ]));
    assert!(package
        .content()
        .contains("<text:p text:style-name=\"Title\">Title</text:p>"));
    assert!(package
        .content()
        .contains("<text:p text:style-name=\"Subtitle\">Subtitle</text:p>"));
    assert!(package
        .styles()
        .contains("style:name=\"Title\" style:display-name=\"Title\""));
    assert!(package
        .styles()
        .contains("style:name=\"Subtitle\" style:display-name=\"Subtitle\""));
}

/// OpenDocument collapses whitespace the way XML does, so spaces have to
/// become elements or a paragraph indented with them loses them silently.
#[test]
fn runs_of_spaces_tabs_and_breaks_become_elements() {
    let package = export(&document(vec![paragraph(
        "  two leading, three   inside,\tafter a tab\nand a break, one trailing ",
    )]));
    let content = package.content();
    assert!(
        content.contains("<text:s text:c=\"2\"/>two leading, three"),
        "leading spaces were not preserved: {content}"
    );
    assert!(content.contains("three<text:s text:c=\"3\"/>inside,<text:tab/>after a tab"));
    assert!(content.contains("after a tab<text:line-break/>and a break"));
    assert!(content.contains("one trailing<text:s text:c=\"1\"/></text:p>"));
}

#[test]
fn a_cr_lf_pair_is_one_line_break() {
    let package = export(&document(vec![paragraph("before\r\nafter")]));
    assert!(package.content().contains("before<text:line-break/>after"));
}

#[test]
fn marks_become_automatic_text_styles_and_identical_runs_share_one() {
    let package = export(&document(vec![block(
        BlockKind::Paragraph,
        vec![
            text_inline("bold", vec![mark(MarkKind::Bold, None)]),
            text_inline("italic", vec![mark(MarkKind::Italic, None)]),
            text_inline("also bold", vec![mark(MarkKind::Bold, None)]),
            text_inline(
                "coloured",
                vec![mark(MarkKind::Color, Some("#FF8000".to_string()))],
            ),
            text_inline("up", vec![mark(MarkKind::Superscript, None)]),
        ],
    )]));
    let content = package.content();
    assert!(content.contains("<style:style style:name=\"T1\" style:family=\"text\"><style:text-properties fo:font-weight=\"bold\"/></style:style>"));
    assert!(content.contains("<style:style style:name=\"T2\" style:family=\"text\"><style:text-properties fo:font-style=\"italic\"/></style:style>"));
    assert!(content.contains("fo:color=\"#ff8000\""));
    assert!(content.contains("style:text-position=\"super 58%\""));
    // Three marked runs, two distinct formats before the colour and the
    // superscript: the bold run is not written twice.
    assert!(content.contains("<text:span text:style-name=\"T1\">bold</text:span><text:span text:style-name=\"T2\">italic</text:span><text:span text:style-name=\"T1\">also bold</text:span>"));
    assert_eq!(4, content.matches("style:family=\"text\"").count());
}

#[test]
fn a_link_becomes_a_text_anchor_and_a_code_mark_becomes_monospace() {
    let package = export(&document(vec![block(
        BlockKind::Paragraph,
        vec![
            Inline::Link {
                id: StableId::new("link"),
                text: "OpenDoc".to_string(),
                href: "https://example.org/?a=1&b=2".to_string(),
                marks: Vec::new(),
            },
            text_inline("code()", vec![mark(MarkKind::Code, None)]),
        ],
    )]));
    let content = package.content();
    assert!(content.contains(
        "<text:a xlink:type=\"simple\" xlink:href=\"https://example.org/?a=1&amp;b=2\">OpenDoc</text:a>"
    ));
    assert!(content.contains("style:font-name=\"Liberation Mono\""));
    assert_eq!(vec!["odt-export-code-mark-as-monospace"], package.codes());
}

// ---------------------------------------------------------------------------
// Block properties and units
// ---------------------------------------------------------------------------

/// The conversion the whole writer rests on: 20 twips *is* one point, so
/// every length lands on the 0.05pt grid exactly. The values here are ones no
/// route through centimetres, inches or pixels could reproduce.
#[test]
fn lengths_are_written_as_exact_points() {
    let package = export(&document(vec![styled_paragraph(
        "Measured",
        BlockProperties {
            indent_start: Some(twips(19)),
            indent_end: Some(twips(241)),
            indent_first_line: Some(twips(-360)),
            space_before: Some(twips(31679)),
            space_after: Some(twips(0)),
            ..BlockProperties::default()
        },
    )]));
    let content = package.content();
    assert!(
        content.contains(
            "<style:paragraph-properties fo:margin-left=\"0.95pt\" fo:margin-right=\"12.05pt\" fo:text-indent=\"-18pt\" fo:margin-top=\"1583.95pt\" fo:margin-bottom=\"0pt\"/>"
        ),
        "lengths were not exact: {content}"
    );
}

#[test]
fn uniform_paragraph_border_uses_the_odf_frame_property() {
    let package = export(&document(vec![styled_paragraph(
        "Framed",
        BlockProperties {
            border: Some(
                CellBorder::new(
                    BorderStyle::Double,
                    twips(40),
                    Color::parse("#336699").unwrap(),
                )
                .unwrap(),
            ),
            ..BlockProperties::default()
        },
    )]));
    assert!(
        package
            .content()
            .contains("fo:border=\"2pt double #336699\""),
        "{}",
        package.content()
    );
}

#[test]
fn alignment_uses_the_logical_xsl_values_the_model_already_speaks() {
    for (alignment, expected) in [
        (Alignment::Start, "fo:text-align=\"start\""),
        (Alignment::Center, "fo:text-align=\"center\""),
        (Alignment::End, "fo:text-align=\"end\""),
        (
            Alignment::Justify,
            "fo:text-align=\"justify\" fo:text-align-last=\"start\"",
        ),
    ] {
        let package = export(&document(vec![styled_paragraph(
            "Aligned",
            BlockProperties {
                alignment: Some(alignment),
                ..BlockProperties::default()
            },
        )]));
        assert!(
            package.content().contains(expected),
            "{alignment:?} did not produce {expected}"
        );
    }
}

/// ODF has a spelling for all three of the model's rules, which CSS does not
/// — `opendoc-render` has to approximate `Exact` with `line-height`.
#[test]
fn all_three_line_spacing_rules_map_exactly() {
    let cases = [
        (
            LineSpacing::Multiple(LineHeightMultiple::from_thousandths(1500).unwrap()),
            "fo:line-height=\"150%\"",
        ),
        (
            LineSpacing::Multiple(LineHeightMultiple::from_thousandths(1333).unwrap()),
            "fo:line-height=\"133.3%\"",
        ),
        (LineSpacing::Exact(twips(283)), "fo:line-height=\"14.15pt\""),
        (
            LineSpacing::AtLeast(twips(283)),
            "style:line-height-at-least=\"14.15pt\"",
        ),
    ];
    for (spacing, expected) in cases {
        let package = export(&document(vec![styled_paragraph(
            "Spaced",
            BlockProperties {
                line_spacing: Some(spacing),
                ..BlockProperties::default()
            },
        )]));
        assert!(
            package.content().contains(expected),
            "{spacing:?} did not produce {expected}: {}",
            package.content()
        );
        assert!(
            package.codes().is_empty(),
            "line spacing warned: {:?}",
            package.warnings
        );
    }
}

/// Which ODF properties LibreOffice resolves logically and which physically
/// is measured, not assumed — a probe ODT rendered through LibreOffice showed
/// that `fo:margin-left` and `fo:text-indent` follow the writing mode while
/// `fo:text-align="start"` always draws on the left. So the indents go
/// across unswapped and the alignment is swapped.
#[test]
fn a_right_to_left_block_keeps_its_indents_and_swaps_its_alignment() {
    let package = export(&document(vec![styled_paragraph(
        "عربي",
        BlockProperties {
            direction: Some(TextDirection::RightToLeft),
            alignment: Some(Alignment::Start),
            indent_start: Some(twips(720)),
            indent_end: Some(twips(360)),
            indent_first_line: Some(twips(180)),
            ..BlockProperties::default()
        },
    )]));
    assert!(package.content().contains(
        "<style:paragraph-properties style:writing-mode=\"rl-tb\" fo:text-align=\"end\" fo:margin-left=\"36pt\" fo:margin-right=\"18pt\" fo:text-indent=\"9pt\"/>"
    ), "{}", package.content());

    // The same properties in a left-to-right block spell the alignment the
    // other way round and keep the indents where they were.
    let package = export(&document(vec![styled_paragraph(
        "latin",
        BlockProperties {
            direction: Some(TextDirection::LeftToRight),
            alignment: Some(Alignment::Start),
            indent_start: Some(twips(720)),
            indent_end: Some(twips(360)),
            ..BlockProperties::default()
        },
    )]));
    assert!(package.content().contains(
        "<style:paragraph-properties style:writing-mode=\"lr-tb\" fo:text-align=\"start\" fo:margin-left=\"36pt\" fo:margin-right=\"18pt\"/>"
    ), "{}", package.content());
}

/// LibreOffice does not derive alignment from the writing mode, so an
/// unstated alignment on a right-to-left block has to be written down or the
/// text renders flush left — which is not "inherit", it is wrong.
#[test]
fn a_right_to_left_block_with_no_stated_alignment_still_starts_on_the_right() {
    let package = export(&document(vec![styled_paragraph(
        "عربي",
        BlockProperties {
            direction: Some(TextDirection::RightToLeft),
            ..BlockProperties::default()
        },
    )]));
    assert!(
        package
            .content()
            .contains("style:writing-mode=\"rl-tb\" fo:text-align=\"end\""),
        "{}",
        package.content()
    );
    // A left-to-right block with no alignment says nothing, because the
    // reader's own default is already correct there.
    let package = export(&document(vec![paragraph("latin")]));
    assert!(!package.content().contains("fo:text-align"));
}

#[test]
fn identical_paragraph_properties_share_one_automatic_style() {
    let properties = BlockProperties {
        alignment: Some(Alignment::Center),
        ..BlockProperties::default()
    };
    let package = export(&document(vec![
        styled_paragraph("one", properties.clone()),
        styled_paragraph("two", properties),
        paragraph("plain"),
    ]));
    let content = package.content();
    assert_eq!(1, content.matches("style:family=\"paragraph\"").count());
    assert_eq!(2, content.matches("text:style-name=\"P1\"").count());
    // A paragraph with nothing to say references the named style directly
    // rather than minting an empty automatic style for it.
    assert!(content.contains("<text:p text:style-name=\"Standard\">plain</text:p>"));
}

// ---------------------------------------------------------------------------
// Lists
// ---------------------------------------------------------------------------

#[test]
fn a_nested_list_run_becomes_nested_text_lists() {
    let list = StableId::new("list");
    let package = export(&document(vec![
        list_item("one", &list, 0, ListKind::Ordered),
        list_item("one a", &list, 1, ListKind::Ordered),
        list_item("one b", &list, 1, ListKind::Ordered),
        list_item("two", &list, 0, ListKind::Ordered),
        paragraph("after"),
    ]));
    let content = package.content();
    assert!(content.contains(concat!(
        "<text:list text:style-name=\"L1\">",
        "<text:list-item><text:p text:style-name=\"Standard\">one</text:p>",
        "<text:list><text:list-item><text:p text:style-name=\"Standard\">one a</text:p></text:list-item>",
        "<text:list-item><text:p text:style-name=\"Standard\">one b</text:p></text:list-item></text:list>",
        "</text:list-item>",
        "<text:list-item><text:p text:style-name=\"Standard\">two</text:p></text:list-item>",
        "</text:list>",
    )), "{content}");
    assert!(content.contains("<text:list-level-style-number text:level=\"1\""));
    assert!(content.contains("<text:list-level-style-number text:level=\"2\""));
    assert!(package.codes().is_empty(), "{:?}", package.warnings);
}

/// Two adjacent list runs are two lists, which is the whole point of FM-5's
/// list identity.
#[test]
fn two_list_runs_are_two_lists() {
    let first = StableId::new("list");
    let second = StableId::new("list");
    let package = export(&document(vec![
        list_item("a", &first, 0, ListKind::Bullet),
        list_item("b", &second, 0, ListKind::Bullet),
    ]));
    assert_eq!(2, package.content().matches("<text:list ").count());
}

/// A run whose first item is deeper than level 0 still has to be well formed:
/// ODF allows a list item that holds nothing but a nested list.
#[test]
fn a_run_starting_below_the_top_level_is_still_well_formed() {
    let list = StableId::new("list");
    let package = export(&document(vec![list_item(
        "deep",
        &list,
        2,
        ListKind::Bullet,
    )]));
    let content = package.content();
    assert!(
        content.contains(concat!(
            "<text:list text:style-name=\"L1\"><text:list-item>",
            "<text:list><text:list-item>",
            "<text:list><text:list-item><text:p text:style-name=\"Standard\">deep</text:p>",
            "</text:list-item></text:list>",
            "</text:list-item></text:list>",
            "</text:list-item></text:list>",
        )),
        "{content}"
    );
}

#[test]
fn ordered_list_style_carries_its_start_value() {
    let list = StableId::new("continued-list");
    let mut source = document(vec![list_item("seven", &list, 0, ListKind::Ordered)]);
    source
        .list_properties
        .entry(list)
        .or_default()
        .ordered_starts
        .insert(0, 7);
    let content = export(&source).content();
    assert!(
        content.contains("text:list-level-style-number text:level=\"1\" text:style-name=\"Numbering_20_Symbols\" style:num-suffix=\".\" style:num-format=\"1\" text:start-value=\"7\""),
        "{content}"
    );
}

/// The bullet glyph the list item reading `text` draws: the `text:list` it
/// sits in names a list style, and that style's level definition carries the
/// character. ODF puts the marker on the level rather than on the item, so
/// this indirection *is* the mapping under test.
fn list_glyph(content: &str, text: &str) -> String {
    let at = content
        .find(&format!(">{text}<"))
        .unwrap_or_else(|| panic!("no list item reading {text:?} in {content}"));
    let list_start = content[..at]
        .rfind("<text:list ")
        .expect("a list item outside any text:list");
    let style = between(&content[list_start..], "text:style-name=\"", "\"")
        .expect("a text:list with no style name");
    let definition = content
        .find(&format!("<text:list-style style:name=\"{style}\">"))
        .unwrap_or_else(|| panic!("no list style {style} in {content}"));
    between(&content[definition..], "text:bullet-char=\"", "\"")
        .unwrap_or_else(|| panic!("list style {style} draws no bullet character"))
}

fn between(haystack: &str, open: &str, close: &str) -> Option<String> {
    let start = haystack.find(open)? + open.len();
    let end = haystack[start..].find(close)? + start;
    Some(haystack[start..end].to_string())
}

/// ODF puts the marker on the list *level*, so a ticked and an unticked item
/// at one level cannot draw different glyphs. The run is split rather than
/// one of the two states being silently lost, and the numbering continues
/// across the cut.
#[test]
fn a_checklist_splits_where_odf_cannot_draw_two_markers_at_one_level() {
    let list = StableId::new("list");
    let package = export(&document(vec![
        list_item("done", &list, 0, ListKind::Checklist { checked: true }),
        list_item("todo", &list, 0, ListKind::Checklist { checked: false }),
        list_item(
            "also todo",
            &list,
            0,
            ListKind::Checklist { checked: false },
        ),
    ]));
    let content = package.content();
    // Both glyphs being present proves nothing: swap the two constants and
    // that assertion still holds while every done item draws an empty box.
    // The glyph has to be reached *through* the item that uses it.
    assert_eq!(
        "\u{2612}",
        list_glyph(&content, "done"),
        "a ticked item did not draw a crossed ballot box"
    );
    assert_eq!(
        "\u{2610}",
        list_glyph(&content, "todo"),
        "an unticked item did not draw an empty ballot box"
    );
    assert_eq!(2, content.matches("<text:list ").count());
    assert!(content.contains("text:continue-numbering=\"true\""));
    let mut codes = package.codes();
    codes.sort_unstable();
    assert_eq!(
        vec![
            "odt-export-checklist-as-bullet",
            "odt-export-split-mixed-list"
        ],
        codes
    );
}

// ---------------------------------------------------------------------------
// Tables
// ---------------------------------------------------------------------------

#[test]
fn leading_header_rows_use_the_odf_repeat_header_wrapper() {
    let mut heading = row(vec![cell("Heading")]);
    heading.header = true;
    let package = export(&document(vec![block(
        BlockKind::table(vec![heading, row(vec![cell("Body")])]),
        Vec::new(),
    )]));
    let content = package.content();
    assert!(
        content.contains("<table:table-header-rows><table:table-row>"),
        "{content}"
    );
    assert!(package.codes().is_empty(), "{:?}", package.warnings);
}

/// The model's grid is rectangular with merged cells as spans on the origin
/// cell, and ODF says exactly the same thing — so merging stays invertible
/// through the export rather than becoming missing cells.
#[test]
fn a_merged_cell_becomes_a_span_plus_real_covered_cells() {
    let mut rows = vec![
        row(vec![cell("merged"), cell("b"), cell("c")]),
        row(vec![cell("d"), cell("e"), cell("f")]),
    ];
    rows[0].cells[0].span = CellSpan::new(2, 2).unwrap();
    let columns = vec![
        TableColumn::sized(twips(2880)).unwrap(),
        TableColumn::auto(),
        TableColumn::sized(twips(1441)).unwrap(),
    ];
    let package = export(&document(vec![block(
        BlockKind::Table {
            columns,
            properties: Default::default(),
            rows,
        },
        Vec::new(),
    )]));
    let content = package.content();
    assert!(
        content.contains("table:number-columns-spanned=\"2\" table:number-rows-spanned=\"2\""),
        "{content}"
    );
    assert_eq!(
        3,
        content.matches("<table:covered-table-cell/>").count(),
        "a 2x2 span covers three positions besides its own: {content}"
    );
    // Column widths in exact points, and the model's `None` as ODF's own
    // word for auto rather than a width the writer invented.
    assert!(content.contains("style:column-width=\"144pt\""));
    assert!(content.contains("style:column-width=\"72.05pt\""));
    assert!(content.contains("style:use-optimal-column-width=\"true\""));
}

#[test]
fn cell_styling_maps_onto_the_cell_properties_odf_already_has() {
    let mut cell = cell("styled");
    cell.properties = TableCellProperties {
        background: Some(Color::parse("#ffcc00").unwrap()),
        border_top: Some(
            CellBorder::new(
                opendoc_core::BorderStyle::Double,
                twips(40),
                Color::parse("#123456").unwrap(),
            )
            .unwrap(),
        ),
        border_start: Some(CellBorder::none()),
        vertical_alignment: Some(VerticalAlignment::Middle),
        padding_start: Some(twips(57)),
        ..TableCellProperties::default()
    };
    let package = export(&document(vec![block(
        BlockKind::Table {
            columns: vec![TableColumn::auto()],
            properties: Default::default(),
            rows: vec![row(vec![cell])],
        },
        Vec::new(),
    )]));
    let content = package.content();
    assert!(
        content.contains("fo:background-color=\"#ffcc00\""),
        "{content}"
    );
    assert!(content.contains("fo:border-top=\"2pt double #123456\""));
    // An explicitly absent border is "none" and is written; an *unstated*
    // edge is written as nothing at all, which ODF also reads as no border.
    // The export used to write a 0.5pt black hairline there, inventing a line
    // the document never had — the same defect the DOCX writer had with its
    // unconditional `w:tblBorders`.
    assert!(content.contains("fo:border-left=\"none\""));
    assert!(
        !content.contains("fo:border-bottom"),
        "an edge the document does not state was written anyway: {content}"
    );
    assert!(
        !content.contains("fo:border-right"),
        "an edge the document does not state was written anyway: {content}"
    );
    assert!(content.contains("style:vertical-align=\"middle\""));
    assert!(content.contains("fo:padding-left=\"2.85pt\""));
}

#[test]
fn a_tables_own_block_formatting_has_nowhere_to_go_and_says_so() {
    let table = Block {
        properties: BlockProperties {
            alignment: Some(Alignment::Center),
            ..BlockProperties::default()
        },
        ..block(
            BlockKind::Table {
                columns: vec![TableColumn::auto()],
                properties: Default::default(),
                rows: vec![row(vec![cell("x")])],
            },
            Vec::new(),
        )
    };
    let package = export(&document(vec![table]));
    assert_eq!(vec!["odt-export-dropped-block-properties"], package.codes());
}

// ---------------------------------------------------------------------------
// Page geometry, furniture and page numbers
// ---------------------------------------------------------------------------

/// The one place the two page models differ, and the arithmetic is the whole
/// of it: `PageSetup` measures `margin_top` to the *body*, ODF measures
/// `fo:margin-top` to the *header*. So with a header the page margin is
/// `margin_header` and the header is given a fixed height of the difference,
/// which puts the body back at `margin_top`.
#[test]
fn the_header_height_is_what_puts_the_body_where_the_model_says() {
    let mut document = draft(vec![paragraph("Body")]);
    document.page_setup = PageSetup::default()
        .with_margins(twips(1440), twips(1440), twips(1080), twips(1080))
        .unwrap()
        .with_furniture_margins(twips(720), twips(600))
        .unwrap();
    document.header = vec![paragraph("A header")];
    document.footer = vec![block(
        BlockKind::Paragraph,
        vec![
            text_inline("Page ", Vec::new()),
            Inline::PageNumber {
                id: StableId::new("page"),
                field: PageNumberField::CurrentPage,
            },
            text_inline(" of ", Vec::new()),
            Inline::PageNumber {
                id: StableId::new("page"),
                field: PageNumberField::PageCount,
            },
        ],
    )];
    document.validate().unwrap();
    let package = export(&document);
    let styles = package.styles();

    // Letter at exact points, with the margins measured to the furniture.
    assert!(
        styles.contains(concat!(
            "<style:page-layout-properties fo:page-width=\"612pt\" fo:page-height=\"792pt\"",
            " style:print-orientation=\"portrait\" fo:margin-top=\"36pt\" fo:margin-bottom=\"30pt\"",
            " fo:margin-left=\"54pt\" fo:margin-right=\"54pt\" style:writing-mode=\"lr-tb\"/>",
        )),
        "{styles}"
    );
    // 1440 - 720 = 720 twips = 36pt of header, so the body starts at 72pt.
    assert!(styles.contains("<style:header-style><style:header-footer-properties fo:min-height=\"36pt\" fo:margin-bottom=\"0pt\" style:dynamic-spacing=\"false\"/></style:header-style>"), "{styles}");
    // 1440 - 600 = 840 twips = 42pt of footer.
    assert!(styles.contains("<style:footer-style><style:header-footer-properties fo:min-height=\"42pt\" fo:margin-top=\"0pt\" style:dynamic-spacing=\"false\"/></style:footer-style>"), "{styles}");

    // The furniture content lives in the master page, and its automatic
    // styles live in styles.xml with it — ODF resolves a style name inside
    // the file that uses it.
    assert!(
        styles.contains(
            "<style:header><text:p text:style-name=\"Standard\">A header</text:p></style:header>"
        ),
        "{styles}"
    );
    // ODF has a real page-number field, so nothing is frozen into a number.
    assert!(styles.contains("<text:page-number text:select-page=\"current\">1</text:page-number>"));
    assert!(styles.contains("<text:page-count>1</text:page-count>"));
    assert!(package
        .codes()
        .contains(&"odt-export-page-number-placeholder"));
}

/// Without furniture the page margin is the body margin and there is no
/// header height to compute.
#[test]
fn a_document_with_no_furniture_measures_its_margins_to_the_body() {
    let package = export(&document(vec![paragraph("Body")]));
    let styles = package.styles();
    assert!(
        styles.contains("fo:margin-top=\"72pt\" fo:margin-bottom=\"72pt\""),
        "{styles}"
    );
    assert!(!styles.contains("style:header-style"));
    assert!(styles.contains("<style:header style:display=\"false\"/>"));
}

#[test]
fn a_landscape_page_derives_its_orientation_from_its_dimensions() {
    let mut document = draft(vec![paragraph("Wide")]);
    document.page_setup = PageSetup::default()
        .with_size(twips(15840), twips(12240))
        .unwrap();
    document.validate().unwrap();
    assert!(export(&document)
        .styles()
        .contains("style:print-orientation=\"landscape\""));
}

/// A header margin that leaves no room for the header cannot be written as a
/// negative height, so it is clamped and named.
#[test]
fn a_header_margin_with_no_room_is_clamped_with_a_warning() {
    let mut document = draft(vec![paragraph("Body")]);
    document.page_setup = PageSetup::default()
        .with_furniture_margins(twips(2160), twips(720))
        .unwrap();
    document.header = vec![paragraph("Squeezed")];
    document.validate().unwrap();
    let package = export(&document);
    assert!(package.styles().contains("fo:min-height=\"0pt\""));
    assert!(package
        .codes()
        .contains(&"odt-export-furniture-margin-clamped"));
}

/// In ODF a page break is a property of the paragraph that opens the page, so
/// an explicit break block folds into the next block rather than becoming an
/// empty paragraph nobody asked for.
#[test]
fn a_page_break_folds_into_the_block_that_follows_it() {
    let package = export(&document(vec![
        paragraph("before"),
        block(BlockKind::PageBreak, Vec::new()),
        block(
            BlockKind::Heading { level: 1 },
            vec![text_inline("after", Vec::new())],
        ),
    ]));
    let content = package.content();
    assert!(
        content.contains(
            "<style:style style:name=\"P1\" style:family=\"paragraph\" style:parent-style-name=\"Heading_20_1\"><style:paragraph-properties fo:break-before=\"page\"/></style:style>"
        ),
        "{content}"
    );
    assert!(
        content.contains("<text:h text:style-name=\"P1\" text:outline-level=\"1\">after</text:h>")
    );
    assert_eq!(
        2,
        content.matches("<text:p").count() + content.matches("<text:h").count(),
        "the break became a block of its own: {content}"
    );
    assert!(package.codes().is_empty(), "{:?}", package.warnings);
}

/// A break with nothing after it has no paragraph to attach to, so it becomes
/// one — and says so.
#[test]
fn a_trailing_page_break_becomes_an_empty_paragraph_and_warns() {
    let package = export(&document(vec![
        paragraph("last"),
        block(BlockKind::PageBreak, Vec::new()),
    ]));
    let content = package.content();
    assert!(
        content.contains("<text:p text:style-name=\"P1\"/>"),
        "{content}"
    );
    assert!(content.contains("fo:break-before=\"page\""));
    assert_eq!(vec!["odt-export-trailing-page-break"], package.codes());
}

/// A page break before a table lands on the table, which in ODF takes the
/// break property too.
#[test]
fn a_page_break_before_a_table_lands_on_the_table() {
    let package = export(&document(vec![
        block(BlockKind::PageBreak, Vec::new()),
        block(
            BlockKind::Table {
                columns: vec![TableColumn::auto()],
                properties: Default::default(),
                rows: vec![row(vec![cell("x")])],
            },
            Vec::new(),
        ),
    ]));
    assert!(
        package
            .content()
            .contains("<style:table-properties style:rel-width=\"100%\" table:align=\"margins\" fo:break-before=\"page\"/>"),
        "{}",
        package.content()
    );
}

// ---------------------------------------------------------------------------
// Images
// ---------------------------------------------------------------------------

#[test]
fn an_image_carries_its_stated_size_and_its_own_paragraph_formatting() {
    let document = draft(vec![Block {
        properties: BlockProperties {
            alignment: Some(Alignment::Center),
            ..BlockProperties::default()
        },
        ..block(
            BlockKind::Image {
                blob_hash: HASH.to_string(),
                alt_text: "a square".to_string(),
                layout: ImageLayout {
                    width: Some(twips(2881)),
                    height: Some(twips(1440)),
                    placement: Some(ImagePlacement::Block),
                    ..ImageLayout::default()
                },
            },
            Vec::new(),
        )
    }]);
    document.validate().unwrap();
    let package = export_with(&document, &BTreeMap::from([(HASH.to_string(), png())]));
    let content = package.content();
    assert!(
        content.contains("svg:width=\"144.05pt\" svg:height=\"72pt\""),
        "{content}"
    );
    assert!(content.contains("text:anchor-type=\"as-char\""));
    assert!(content.contains("<draw:image xlink:href=\"Pictures/image1.png\" xlink:type=\"simple\" xlink:show=\"embed\" xlink:actuate=\"onLoad\"/>"));
    assert!(content.contains("<svg:title>a square</svg:title>"));
    // Unlike WordprocessingML's drawing, the frame sits in an ordinary
    // paragraph, so the image block's own alignment survives.
    assert!(content.contains("fo:text-align=\"center\""));
    assert!(package.codes().is_empty(), "{:?}", package.warnings);
}

#[test]
fn positioned_image_export_warns_before_falling_back_to_in_flow_frame() {
    let document = draft(vec![block(
        BlockKind::Image {
            blob_hash: HASH.to_string(),
            alt_text: "positioned".to_string(),
            layout: ImageLayout {
                positioned: Some(PositionedImage {
                    anchor: PositionedImageAnchor::PageContent,
                    horizontal_offset: twips(-240),
                    vertical_offset: twips(480),
                    layer: PositionedImageLayer::BehindText,
                }),
                ..ImageLayout::default()
            },
        },
        Vec::new(),
    )]);
    document.validate().unwrap();
    let package = export_with(&document, &BTreeMap::from([(HASH.to_string(), png())]));
    assert_eq!(
        package.codes(),
        vec!["odt-export-positioned-image-as-inline"]
    );
    assert!(package.content().contains("text:anchor-type=\"as-char\""));
}

#[test]
fn image_visual_effects_use_the_odt_frame_and_style_vocabulary() {
    let border = CellBorder::new(
        BorderStyle::Dashed,
        twips(30),
        Color::parse("#336699").unwrap(),
    )
    .unwrap();
    let document = document(vec![block(
        BlockKind::Image {
            blob_hash: HASH.to_string(),
            alt_text: String::new(),
            layout: ImageLayout {
                width: Some(twips(1440)),
                height: Some(twips(720)),
                rotation_degrees: Some(-45),
                opacity_percent: Some(37),
                border: Some(border),
                ..ImageLayout::default()
            },
        },
        Vec::new(),
    )]);
    let package = export_with(&document, &BTreeMap::from([(HASH.to_string(), png())]));
    assert!(package
        .content()
        .contains("draw:transform=\"rotate (-45)\""));
    assert!(package.content().contains("draw:image-opacity=\"37%\""));
    assert!(package
        .content()
        .contains("fo:border=\"1.50pt dashed #336699\""));
    assert!(package.codes().is_empty(), "{:?}", package.warnings);
}

#[test]
fn image_crop_and_caption_are_visible_degradations_not_silent_loss() {
    let document = document(vec![block(
        BlockKind::Image {
            blob_hash: HASH.to_string(),
            alt_text: String::new(),
            layout: ImageLayout {
                crop: Some(ImageCrop {
                    top_percent: 10,
                    right_percent: 20,
                    bottom_percent: 5,
                    left_percent: 15,
                }),
                caption: Some("Figure 1: a source-preserving fallback".to_string()),
                ..ImageLayout::default()
            },
        },
        Vec::new(),
    )]);
    let package = export_with(&document, &BTreeMap::from([(HASH.to_string(), png())]));
    assert!(package
        .content()
        .contains("Figure 1: a source-preserving fallback"));
    assert!(!package.content().contains("fo:clip="));
    assert_eq!(
        package.codes(),
        vec![
            "odt-export-image-crop-unrepresentable",
            "odt-export-image-caption-as-paragraph",
        ]
    );
}

/// `None` on one axis means "scale to keep the aspect ratio", which is a
/// function of the picture's own bytes and is never written into the model.
#[test]
fn an_axis_the_model_leaves_open_is_scaled_from_the_pictures_own_pixels() {
    let document = draft(vec![block(
        BlockKind::Image {
            blob_hash: HASH.to_string(),
            alt_text: String::new(),
            layout: ImageLayout {
                width: Some(twips(1440)),
                height: None,
                placement: None,
                ..ImageLayout::default()
            },
        },
        Vec::new(),
    )]);
    document.validate().unwrap();
    let package = export_with(&document, &BTreeMap::from([(HASH.to_string(), png())]));
    // The PNG is 2x3, so a one-inch width is a one-and-a-half-inch height.
    assert!(
        package
            .content()
            .contains("svg:width=\"72pt\" svg:height=\"108pt\""),
        "{}",
        package.content()
    );
}

#[test]
fn wrapped_placement_flows_text_down_the_opposite_edge() {
    for (placement, expected) in [
        (
            ImagePlacement::WrapStart,
            "style:wrap=\"right\" style:horizontal-pos=\"left\"",
        ),
        (
            ImagePlacement::WrapEnd,
            "style:wrap=\"left\" style:horizontal-pos=\"right\"",
        ),
    ] {
        let document = draft(vec![block(
            BlockKind::Image {
                blob_hash: HASH.to_string(),
                alt_text: String::new(),
                layout: ImageLayout {
                    width: Some(twips(1440)),
                    height: Some(twips(1440)),
                    placement: Some(placement),
                    ..ImageLayout::default()
                },
            },
            Vec::new(),
        )]);
        document.validate().unwrap();
        let package = export_with(&document, &BTreeMap::from([(HASH.to_string(), png())]));
        assert!(package.content().contains(expected), "{placement:?}");
        assert!(package.content().contains("text:anchor-type=\"paragraph\""));
    }
}

#[test]
fn an_image_whose_bytes_are_missing_becomes_its_alt_text_and_says_so() {
    let document = draft(vec![block(
        BlockKind::Image {
            blob_hash: MISSING_HASH.to_string(),
            alt_text: "a diagram".to_string(),
            layout: ImageLayout::default(),
        },
        Vec::new(),
    )]);
    document.validate().unwrap();
    let package = export(&document);
    assert!(package.content().contains("a diagram"));
    assert!(!package.content().contains("draw:frame"));
    assert_eq!(vec!["odt-export-missing-image-blob"], package.codes());
}

#[test]
fn image_effects_are_named_when_an_asset_falls_back_to_alt_text() {
    let border = CellBorder::new(
        BorderStyle::Dotted,
        twips(20),
        Color::parse("#336699").unwrap(),
    )
    .unwrap();
    let document = draft(vec![block(
        BlockKind::Image {
            blob_hash: MISSING_HASH.to_string(),
            alt_text: "a diagram".to_string(),
            layout: ImageLayout {
                rotation_degrees: Some(30),
                opacity_percent: Some(60),
                border: Some(border),
                ..ImageLayout::default()
            },
        },
        Vec::new(),
    )]);
    document.validate().unwrap();
    let package = export(&document);
    assert!(package.content().contains("a diagram"));
    assert!(!package.content().contains("draw:frame"));
    assert_eq!(
        package.codes(),
        vec![
            "odt-export-missing-image-blob",
            "odt-export-image-effects-unrepresentable",
        ]
    );
    assert!(package.warnings[1]
        .message
        .contains("rotation, opacity, border"));
}

#[test]
fn missing_image_noop_effects_do_not_claim_visual_loss_in_odt_fallback() {
    let document = draft(vec![block(
        BlockKind::Image {
            blob_hash: MISSING_HASH.to_string(),
            alt_text: "a diagram".to_string(),
            layout: ImageLayout {
                opacity_percent: Some(100),
                border: Some(CellBorder::none()),
                ..ImageLayout::default()
            },
        },
        Vec::new(),
    )]);
    document.validate().unwrap();
    let package = export(&document);
    assert_eq!(vec!["odt-export-missing-image-blob"], package.codes());
}

#[test]
fn an_image_asset_fallback_still_explains_its_crop_and_keeps_its_caption() {
    let document = document(vec![block(
        BlockKind::Image {
            blob_hash: MISSING_HASH.to_string(),
            alt_text: "a diagram".to_string(),
            layout: ImageLayout {
                crop: Some(ImageCrop {
                    top_percent: 10,
                    ..ImageCrop::default()
                }),
                caption: Some("Figure 2: diagram fallback".to_string()),
                ..ImageLayout::default()
            },
        },
        Vec::new(),
    )]);
    let package = export(&document);
    assert!(package.content().contains("a diagram"));
    assert!(package.content().contains("Figure 2: diagram fallback"));
    assert_eq!(
        package.codes(),
        vec![
            "odt-export-missing-image-blob",
            "odt-export-image-crop-unrepresentable",
            "odt-export-image-caption-as-paragraph",
        ]
    );
}

#[test]
fn an_image_with_no_stated_or_readable_size_gets_a_default_and_warns() {
    let document = draft(vec![block(
        BlockKind::Image {
            blob_hash: HASH.to_string(),
            alt_text: String::new(),
            layout: ImageLayout::default(),
        },
        Vec::new(),
    )]);
    document.validate().unwrap();
    let unreadable = ExportImage {
        media_type: "image/png".to_string(),
        bytes: b"\x89PNG\r\n\x1a\n".to_vec(),
    };
    let package = export_with(&document, &BTreeMap::from([(HASH.to_string(), unreadable)]));
    assert!(package
        .content()
        .contains("svg:width=\"288pt\" svg:height=\"216pt\""));
    assert_eq!(vec!["odt-export-unknown-image-size"], package.codes());
}

// ---------------------------------------------------------------------------
// Footnotes, citations, equations
// ---------------------------------------------------------------------------

/// ODF puts a note where it is referenced, exactly as the model does, so the
/// footnote needs no separate part and no id table.
#[test]
fn a_footnote_is_written_where_it_is_referenced() {
    let footnote_id = StableId::new("footnote");
    let mut document = draft(vec![block(
        BlockKind::Paragraph,
        vec![
            text_inline("claim", Vec::new()),
            Inline::FootnoteRef {
                id: StableId::new("ref"),
                footnote_id: footnote_id.clone(),
            },
        ],
    )]);
    document.footnotes = vec![Footnote {
        id: footnote_id,
        revision: 1,
        body: vec![text_inline("the source", Vec::new())],
        deleted: false,
    }];
    document.validate().unwrap();
    let package = export(&document);
    assert!(
        package.content().contains(concat!(
        "<text:note text:id=\"ftn1\" text:note-class=\"footnote\">",
        "<text:note-citation>1</text:note-citation>",
        "<text:note-body><text:p text:style-name=\"Footnote\">the source</text:p></text:note-body>",
        "</text:note>",
    )),
        "{}",
        package.content()
    );
    assert!(package.codes().is_empty(), "{:?}", package.warnings);
}

#[test]
fn a_citation_becomes_its_rendered_text_and_says_so() {
    let document = draft(vec![block(
        BlockKind::Paragraph,
        vec![Inline::Citation {
            id: StableId::new("inline"),
            citation_id: StableId::parse("cite-one").unwrap(),
            rendered_cache: Some("(Doe, 2020)".to_string()),
        }],
    )]);
    document.validate().unwrap();
    let package = export(&document);
    assert!(package.content().contains("(Doe, 2020)"));
    assert_eq!(vec!["odt-export-citation-as-text"], package.codes());
}

#[test]
fn an_equation_becomes_its_source_and_says_so() {
    let package = export(&document(vec![block(
        BlockKind::EquationBlock {
            equation: Equation {
                id: StableId::new("equation"),
                source_format: EquationSourceFormat::LatexLike,
                source: "a^2 + b^2 = c^2".to_string(),
            },
        },
        Vec::new(),
    )]));
    assert!(package.content().contains("a^2 + b^2 = c^2"));
    assert_eq!(vec!["odt-export-equation-as-source"], package.codes());
}

/// Which CSL styles are bundled is a product decision, and a document using
/// one that is not must degrade with a warning rather than render wrongly
/// (ADR 0003). The export is a surface where that can be said.
#[test]
fn a_citation_style_opendoc_does_not_bundle_is_named_in_the_export_warnings() {
    let mut document = draft(vec![paragraph("cited")]);
    document.citation_database.style = "american-chemical-society".to_string();
    document.citation_database.locale = "en-US".to_string();
    document
        .citation_database
        .upsert_reference(opendoc_core::BibliographyReference {
            id: StableId::parse("ref-one").unwrap(),
            revision: 1,
            source: opendoc_core::CitationSource {
                format: opendoc_core::CitationSourceFormat::CitumNative,
                bytes: b"title: Example".to_vec(),
            },
            summary: opendoc_core::CitationSummary {
                title: "Example".to_string(),
                authors: vec!["Doe".to_string()],
                issued: Some("2020".to_string()),
                doi: None,
                url: None,
            },
            deleted: false,
        });
    document.validate().unwrap();
    let package = export(&document);
    assert!(
        package.codes().contains(&"citation-style-not-bundled"),
        "{:?}",
        package.warnings
    );
    let warning = package
        .warnings
        .iter()
        .find(|warning| warning.code == "citation-style-not-bundled")
        .unwrap();
    assert!(warning.message.contains("american-chemical-society"));
    assert!(warning.message.contains("apa"));
}

// ---------------------------------------------------------------------------
// Document-level degradations
// ---------------------------------------------------------------------------

#[test]
fn comments_suggestions_and_the_doi_are_named_rather_than_vanishing() {
    let mut document = draft(vec![paragraph("Annotated")]);
    document.doi = Some("10.1/example".to_string());
    document.comments = vec![opendoc_core::CommentThread {
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
    // The fixture has to carry a suggestion too, or the suggestion warning is
    // asserted by a document with nothing to suggest.
    document.suggestions = vec![opendoc_core::Suggestion {
        id: StableId::parse("suggestion-one").unwrap(),
        author: "Ada".to_string(),
        kind: opendoc_core::SuggestionKind::Insert {
            anchor: opendoc_core::Anchor::Document,
            content: vec![text_inline("a proposal", Vec::new())],
        },
        state: opendoc_core::SuggestionState::Proposed,
        provenance: Vec::new(),
    }];
    document.validate().unwrap();
    let package = export(&document);
    let mut codes = package.codes();
    codes.sort_unstable();
    assert_eq!(
        vec![
            "odt-export-dropped-comments",
            "odt-export-dropped-doi",
            "odt-export-dropped-suggestions",
        ],
        codes
    );
}

#[test]
fn a_character_xml_cannot_carry_is_removed_and_named() {
    let package = export(&document(vec![paragraph("bell\u{7}ringer")]));
    assert!(package.content().contains("bellringer"));
    assert_eq!(
        vec!["odt-export-dropped-control-character"],
        package.codes()
    );
}

#[test]
fn a_document_with_no_blocks_still_produces_a_paragraph() {
    let mut document = Document::new(TITLE);
    document.blocks.clear();
    document
        .validate()
        .expect("an empty body is a valid document");
    let package = export(&document);
    assert!(package
        .content()
        .contains("<office:text><text:p text:style-name=\"Standard\"/></office:text>"));
}

#[test]
fn bookmarks_on_text_blocks_are_native_zero_width_odt_ranges() {
    let mut source = document(vec![paragraph("here")]);
    source.blocks[0].id = StableId::parse("target").unwrap();
    source.bookmarks.push(Bookmark {
        id: StableId::parse("bookmark-intro").unwrap(),
        name: "Intro".to_string(),
        block_id: StableId::parse("target").unwrap(),
        revision: 1,
        deleted: false,
    });
    let package = export(&source);
    assert!(package.content().contains(
        "<text:bookmark-start text:name=\"Intro\"/>here<text:bookmark-end text:name=\"Intro\"/>"
    ));
    assert!(!package.codes().contains(&"odt-export-unplaced-bookmarks"));
}

#[test]
fn explicit_cell_row_headers_warn_instead_of_claiming_odf_header_columns() {
    let mut header = cell("Label");
    header.properties.row_header = Some(true);
    let package = export(&document(vec![block(
        BlockKind::table(vec![row(vec![header])]),
        Vec::new(),
    )]));

    assert!(package
        .codes()
        .contains(&"odt-export-dropped-table-row-header"));
}
