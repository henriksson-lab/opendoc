use super::*;
use opendoc_core::{
    HeaderFooterSlot, Inline, Length, ListKind, Mark, MarkKind, PageNumberField, PageSetup,
    StableId,
};
use opendoc_spreadsheet::{SheetImage, SheetPrintOrientation, SpreadsheetWorkbook};
use std::collections::BTreeMap;

fn document(blocks: Vec<Block>) -> Document {
    let mut document = Document::new("pdf");
    document.blocks = blocks;
    document
}

fn paragraph(id: &str, text: &str) -> Block {
    let mut block = Block::paragraph(text);
    block.id = StableId::parse(id).expect("valid id");
    block
}

/// A 3-inch page, so a handful of paragraphs overflow it. Page breaking is a
/// function of the page box, so a short page exercises the same mechanism a
/// long document would, and faster.
fn short_page() -> PageSetup {
    PageSetup {
        height: Length::from_twips(3 * 1_440).expect("3in page"),
        ..PageSetup::default()
    }
}

/// The uncompressed content streams, concatenated. Everything drawn on a page
/// is in here as PDF operators, which is what lets these tests assert on what
/// was actually emitted rather than on what the writer intended.
fn operators(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn an_empty_document_is_one_pdf_page() {
    let pdf = export_pdf(&document(Vec::new()));
    assert!(pdf.bytes.starts_with(b"%PDF-1."), "not a PDF");
    assert!(pdf.bytes.ends_with(b"%%EOF\n") || pdf.bytes.ends_with(b"%%EOF"));
    assert_eq!(1, operators(&pdf.bytes).matches("/Type /Page\n").count());
}

#[test]
fn spreadsheet_pdf_discloses_unavailable_blob_backed_images() {
    let mut workbook = SpreadsheetWorkbook::sample();
    workbook.sheets[0].images.push(SheetImage {
        id: "picture-1".to_string(),
        blob_hash: "sha256:abc_DEF-123.png".to_string(),
        start_column: "A".to_string(),
        start_row: "1".to_string(),
        start_offset_x_px: 0,
        start_offset_y_px: 0,
        end_column: "B".to_string(),
        end_row: "2".to_string(),
        end_offset_x_px: 0,
        end_offset_y_px: 0,
    });
    let pdf = export_spreadsheet_pdf(&workbook);
    assert!(pdf
        .warnings
        .iter()
        .any(|warning| warning.code == "pdf-spreadsheet-images-unavailable"));
}

#[test]
fn paragraph_background_and_border_reach_pdf_paint() {
    let mut block = paragraph("framed", "framed text");
    block.properties.background = Some(opendoc_core::Color::parse("#336699").unwrap());
    block.properties.border = Some(
        opendoc_core::CellBorder::new(
            opendoc_core::BorderStyle::Dashed,
            Length::from_twips(20).unwrap(),
            opendoc_core::Color::parse("#cc0000").unwrap(),
        )
        .unwrap(),
    );
    let pdf = export_pdf(&document(vec![block]));
    let text = operators(&pdf.bytes);
    assert!(
        text.contains("0.2 0.4 0.6 rg"),
        "paragraph background absent from PDF operators: {text}"
    );
    assert!(
        text.contains("0.8 0 0 RG"),
        "paragraph border colour absent from PDF operators: {text}"
    );
    assert!(
        text.contains("[4 4] 0 d"),
        "paragraph dashed border absent from PDF operators: {text}"
    );
}

#[test]
fn spreadsheet_pdf_draws_evaluated_cells_on_a_landscape_grid() {
    let pdf = export_spreadsheet_pdf(&SpreadsheetWorkbook::sample());
    let text = operators(&pdf.bytes);
    assert!(pdf.bytes.starts_with(b"%PDF-1."), "not a PDF");
    assert!(text.contains("Sheet1"), "missing sheet title: {text}");
    assert!(text.contains("Apples"), "missing cell value: {text}");
    assert!(text.contains("5"), "missing evaluated cell value: {text}");
    assert!(
        text.contains("/MediaBox [0 0 792 612]"),
        "not landscape letter: {text}"
    );
    assert!(
        pdf.warnings.is_empty(),
        "unexpected warnings: {:?}",
        pdf.warnings
    );
}

#[test]
fn spreadsheet_pdf_uses_each_sheets_durable_portrait_orientation() {
    let mut workbook = SpreadsheetWorkbook::sample();
    let sheet_id = workbook.sheets[0].id.clone();
    workbook
        .set_print_orientation(&sheet_id, SheetPrintOrientation::Portrait)
        .expect("existing sheet");

    let pdf = export_spreadsheet_pdf(&workbook);
    let text = operators(&pdf.bytes);
    assert!(
        text.contains("/MediaBox [0 0 612 792]"),
        "not portrait Letter: {text}"
    );
    assert!(pdf.warnings.iter().any(|warning| {
        warning.code == "pdf-spreadsheet-print-settings-unavailable"
            && warning.message.contains("selected orientation")
    }));
}

#[test]
fn spreadsheet_pdf_uses_the_durable_print_area_including_blank_cells() {
    let mut workbook = SpreadsheetWorkbook::sample();
    let sheet_id = workbook.sheets[0].id.clone();
    workbook
        .set_cell_in_sheet(&sheet_id, "D4", "outside".to_string())
        .expect("existing sheet");
    workbook
        .set_print_area(&sheet_id, Some("A1:B3"))
        .expect("existing sheet")
        .expect("valid print area");

    let pdf = export_spreadsheet_pdf(&workbook);
    let text = operators(&pdf.bytes);
    assert!(
        text.contains("Apples"),
        "print-area value was omitted: {text}"
    );
    assert!(
        !text.contains("outside"),
        "cell outside the durable print area was painted: {text}"
    );
    assert!(pdf
        .warnings
        .iter()
        .any(|warning| { warning.code == "pdf-spreadsheet-print-settings-unavailable" }));
}

#[test]
fn spreadsheet_pdf_print_area_spans_multiple_pages_without_scaling() {
    let mut workbook = SpreadsheetWorkbook::sample();
    let sheet_id = workbook.sheets[0].id.clone();
    for row in 2..=40 {
        workbook
            .set_cell_in_sheet(&sheet_id, &format!("A{row}"), format!("print-{row}"))
            .expect("existing sheet");
    }
    workbook
        .set_print_area(&sheet_id, Some("A1:B40"))
        .expect("existing sheet")
        .expect("valid print area");
    let pdf = export_spreadsheet_pdf(&workbook);
    let text = operators(&pdf.bytes);
    assert!(
        text.matches("/Type /Page\n").count() >= 2,
        "print area did not paginate: {text}"
    );
    assert!(
        text.contains("print-40"),
        "last print-area row was lost: {text}"
    );
}

#[test]
fn spreadsheet_pdf_uses_the_workbooks_explicit_axis_sizes() {
    let mut workbook = SpreadsheetWorkbook::sample();
    let sheet_id = workbook.sheets[0].id.clone();
    workbook
        .set_column_width(&sheet_id, "B", 220)
        .expect("existing column")
        .expect("valid column width");
    workbook
        .set_row_height(&sheet_id, "2", 48)
        .expect("existing row")
        .expect("valid row height");

    let text = operators(&export_spreadsheet_pdf(&workbook).bytes);
    // 220 CSS px is 165pt.  The B right edge is 36pt page margin + 30pt
    // row labels + 75pt default A + 165pt B = 306pt; the second row is 36pt.
    assert!(
        text.contains("306 548 m"),
        "custom column width missing: {text}"
    );
    assert!(
        text.contains("36 474 m"),
        "custom row height missing: {text}"
    );
}

#[test]
fn spreadsheet_pdf_wraps_and_vertically_positions_the_bounded_cell_format() {
    let mut workbook = SpreadsheetWorkbook::sample();
    let sheet_id = workbook.sheets[0].id.clone();
    workbook
        .set_cell_in_sheet(&sheet_id, "A1", "one two three four".to_string())
        .unwrap();
    workbook
        .set_column_width(&sheet_id, "A", 40)
        .unwrap()
        .unwrap();
    workbook
        .set_row_height(&sheet_id, "1", 64)
        .unwrap()
        .unwrap();
    workbook
        .set_cell_format(&sheet_id, "A1", "wrap_strategy", "wrap".to_string())
        .unwrap()
        .unwrap();
    workbook
        .set_cell_format(&sheet_id, "A1", "vertical_align", "top".to_string())
        .unwrap()
        .unwrap();
    let text = operators(&export_spreadsheet_pdf(&workbook).bytes);
    assert!(text.contains("one"), "wrapped first line missing: {text}");
    assert!(text.contains("four"), "wrapped last line missing: {text}");
}

#[test]
fn spreadsheet_pdf_paints_durable_cell_colours_including_a_blank_background_cell() {
    let mut workbook = SpreadsheetWorkbook::sample();
    let sheet_id = workbook.sheets[0].id.clone();
    workbook
        .set_cell_format(&sheet_id, "A2", "text_color", "#cc0000".to_string())
        .unwrap()
        .unwrap();
    // C4 is intentionally blank. Its fill is still visible paper content and
    // must therefore extend the compact PDF used range instead of vanishing.
    workbook
        .set_cell_format(&sheet_id, "C4", "background_color", "#336699".to_string())
        .unwrap()
        .unwrap();

    let pdf = export_spreadsheet_pdf(&workbook);
    let text = operators(&pdf.bytes);
    assert!(
        text.contains("0.8 0 0 rg"),
        "durable text colour absent from PDF operators: {text}"
    );
    assert!(
        text.contains("0.2 0.4 0.6 rg"),
        "blank cell background absent from PDF operators: {text}"
    );
    assert!(
        text.contains("(C)"),
        "a blank but painted C4 did not extend the PDF grid: {text}"
    );
    assert!(
        text.contains("(4)"),
        "a blank but painted C4 did not extend the PDF grid: {text}"
    );
}

#[test]
fn spreadsheet_pdf_uses_durable_bold_and_italic_faces() {
    let mut workbook = SpreadsheetWorkbook::sample();
    let sheet_id = workbook.sheets[0].id.clone();
    workbook
        .set_cell_in_sheet(&sheet_id, "C1", "Both".to_string())
        .unwrap();
    for (cell, property) in [
        ("A1", "bold"),
        ("B1", "italic"),
        ("C1", "bold"),
        ("C1", "italic"),
    ] {
        workbook
            .set_cell_format(&sheet_id, cell, property, "true".to_string())
            .unwrap()
            .unwrap();
    }
    let text = operators(&export_spreadsheet_pdf(&workbook).bytes);
    for font in ["/F2 8 Tf", "/F3 8 Tf", "/F4 8 Tf"] {
        assert!(
            text.contains(font),
            "durable spreadsheet face {font} absent: {text}"
        );
    }
}

#[test]
fn spreadsheet_pdf_repeats_visible_frozen_rows_on_each_vertical_page() {
    let mut workbook = SpreadsheetWorkbook::sample();
    let sheet = &mut workbook.sheets[0];
    sheet.frozen_rows = 1;
    // Default 20px rows leave room for roughly twenty rows on one landscape
    // page.  This produces several vertical pages without relying on a
    // pathological row height, and proves the header's drawn text is present
    // in every page content stream rather than only in the source model.
    for row in 2..=30 {
        let address = format!("A{row}");
        let value = format!("body-{row}");
        sheet
            .cells
            .push(opendoc_spreadsheet::Cell::new(&address, "string", &value));
    }
    let pdf = export_spreadsheet_pdf(&workbook);
    let text = operators(&pdf.bytes);
    let page_count = text.matches("/Type /Page\n").count();
    assert!(page_count >= 2, "fixture did not span pages: {text}");
    assert_eq!(
        page_count,
        text.matches("Item").count(),
        "the frozen first row must be painted on every vertical page: {text}"
    );
    assert!(text.contains("body-30"), "last body row was lost: {text}");
    assert!(
        pdf.warnings.is_empty(),
        "ordinary frozen rows should not need a fidelity warning: {:?}",
        pdf.warnings
    );
}

#[test]
fn the_media_box_is_the_documents_own_page_size() {
    let mut source = document(vec![paragraph("p1", "a")]);
    source.page_setup = PageSetup {
        width: Length::from_twips(11_906).expect("a4 width"),
        height: Length::from_twips(16_838).expect("a4 height"),
        ..PageSetup::default()
    };
    let pdf = export_pdf(&source);
    // 11906 twips is 595.3pt, 16838 is 841.9pt: A4.
    let text = operators(&pdf.bytes);
    assert!(
        text.contains("/MediaBox [0 0 595.3 841.9]"),
        "the page box is not the document's: {}",
        text.lines()
            .find(|line| line.contains("MediaBox"))
            .unwrap_or("<none>")
    );
}

#[test]
fn a_long_document_produces_the_pages_the_layout_decided() {
    let blocks: Vec<Block> = (0..40)
        .map(|index| {
            paragraph(
                &format!("block-{index}"),
                "The quick brown fox jumps over the lazy dog.",
            )
        })
        .collect();
    let mut source = document(blocks);
    source.page_setup = short_page();

    let layout = opendoc_layout::layout_document(&source);
    assert!(layout.page_count > 3, "the fixture fits on one page");

    let pdf = export_pdf(&source);
    assert_eq!(
        layout.page_count as usize,
        operators(&pdf.bytes).matches("/Type /Page\n").count(),
        "the PDF has a different number of pages than the layout decided"
    );
}

#[test]
fn a_bold_run_is_drawn_with_the_bold_face() {
    let mut block = paragraph("p1", "");
    block.content = vec![
        Inline::text("plain"),
        Inline::Text {
            id: StableId::parse("run-bold").expect("valid id"),
            text: "strong".to_string(),
            marks: vec![Mark {
                kind: MarkKind::Bold,
                value: None,
                expand: opendoc_core::MarkExpand::None,
            }],
        },
    ];
    let pdf = export_pdf(&document(vec![block]));
    let text = operators(&pdf.bytes);
    assert!(
        text.contains("/OpenDocSans-Bold"),
        "the bold face was not embedded"
    );
    assert!(
        text.contains("/OpenDocSans\n") || text.contains("/OpenDocSans "),
        "the regular face was not embedded"
    );
}

#[test]
fn only_the_faces_a_document_uses_are_embedded() {
    let pdf = export_pdf(&document(vec![paragraph("p1", "plain text only")]));
    let text = operators(&pdf.bytes);
    assert!(text.contains("/OpenDocSans"));
    assert!(
        !text.contains("/OpenDocMono"),
        "a face nothing draws with was embedded anyway"
    );
    assert!(!text.contains("/OpenDocSans-Bold"));
}

#[test]
fn every_embedded_face_carries_a_tounicode_map() {
    // Without it the glyph ids an Identity-H font draws have no meaning, and
    // the text cannot be extracted, searched or copied.
    let pdf = export_pdf(&document(vec![paragraph("p1", "extractable")]));
    let text = operators(&pdf.bytes);
    // One unstyled paragraph draws with exactly one face, so both counts are
    // 1. Comparing the two counts alone is satisfied by 0 == 0 — an export
    // that embedded no font at all. PLAN88 §7.
    assert_eq!(
        text.matches("/Subtype /Type0").count(),
        1,
        "one unstyled paragraph should have drawn with exactly one face"
    );
    assert_eq!(
        text.matches("/ToUnicode").count(),
        1,
        "an embedded font has no ToUnicode map"
    );
    assert!(text.contains("beginbfchar"));
}

#[test]
fn a_baseline_is_written_in_pdf_user_space_measured_from_the_bottom() {
    // The layout measures y downwards from the top of the sheet, because that
    // is how a document reads and how the browser lays one out. PDF measures
    // it upwards from the bottom. Getting that flip wrong produces a file
    // that opens, has the right pages and the right glyphs, and prints the
    // text mirrored about the middle of the page.
    let source = document(vec![paragraph("p1", "flipped")]);
    let painted = opendoc_layout::layout_painted_document(&source);
    let PaintItem::Text { baseline_twips, .. } = &painted.pages[0].items[0] else {
        panic!("expected text");
    };
    let expected = (source.page_setup.height.twips() - baseline_twips) as f32 / 20.0;

    let pdf = export_pdf(&source);
    let text = operators(&pdf.bytes);
    let matrix = text
        .lines()
        .find(|line| line.ends_with(" Tm"))
        .expect("no text matrix was written");
    let operands: Vec<f32> = matrix
        .trim_end_matches(" Tm")
        .split_whitespace()
        .map(|token| token.parse().expect("a numeric operand"))
        .collect();
    assert_eq!(6, operands.len(), "{matrix:?}");
    let PaintItem::Text { runs, .. } = &painted.pages[0].items[0] else {
        unreachable!()
    };
    let expected_x = runs[0].x_twips as f32 / 20.0;
    assert!(
        (operands[4] - expected_x).abs() < 0.01,
        "the run was written at x={} pt, expected {expected_x} pt (from {matrix:?})",
        operands[4]
    );
    assert!(
        (operands[5] - expected).abs() < 0.01,
        "the baseline was written at {} pt, expected {expected} pt (from {matrix:?})",
        operands[5]
    );
}

#[test]
fn a_run_is_encoded_as_big_endian_glyph_ids() {
    // Identity-H means "two bytes per glyph, most significant first". Writing
    // them the other way round produces a PDF that still opens, still has the
    // right page count and still passes every structural check — and shows
    // the wrong letters. Nothing but this assertion catches it in Rust; the
    // external `pdftotext` run is the other half of the evidence.
    let fonts = Fonts::load();
    let style = TextStyle::new(220);
    let face = FaceId::of(style);
    let glyph = fonts.glyph_id('A', face);
    assert_ne!(0, glyph, "the bundled face has no 'A'");
    assert_eq!(
        vec![(glyph >> 8) as u8, (glyph & 0xff) as u8],
        glyph_string("A", style, &fonts)
    );
}

#[test]
fn the_tounicode_map_turns_every_drawn_glyph_back_into_its_character() {
    // The reader's half of the round trip: a `bfchar` entry per glyph, in the
    // UTF-16BE hex the specification asks for. Decoded here the way a reader
    // decodes it, so a wrong offset or a missing entry fails rather than
    // silently producing a PDF whose text cannot be copied.
    let fonts = Fonts::load();
    let style = TextStyle::new(220);
    let face = FaceId::of(style);
    let mut subset = crate::font::Subset::default();
    let sample = "Aé€ ≤z";
    for ch in sample.chars() {
        subset.record(fonts.glyph_id(ch, face), ch);
    }
    let cmap = String::from_utf8(subset.to_unicode()).expect("the cmap is ascii");
    for ch in sample.chars() {
        let glyph = fonts.glyph_id(ch, face);
        let mut utf16 = [0u16; 2];
        let hex: String = ch
            .encode_utf16(&mut utf16)
            .iter()
            .map(|unit| format!("{unit:04X}"))
            .collect();
        let entry = format!("<{glyph:04X}> <{hex}>");
        assert!(
            cmap.contains(&entry),
            "the cmap has no entry {entry} for {ch:?}"
        );
    }
    assert!(cmap.contains("endcmap"));
}

#[test]
fn a_header_with_a_page_number_field_is_drawn_on_every_page_with_its_own_number() {
    let blocks: Vec<Block> = (0..40)
        .map(|index| {
            paragraph(
                &format!("block-{index}"),
                "The quick brown fox jumps over the lazy dog.",
            )
        })
        .collect();
    let mut source = document(blocks);
    source.page_setup = short_page();
    source.page_setup.page_number_start = 12;
    let mut header = paragraph("hdr", "");
    header.content = vec![
        Inline::text("page "),
        Inline::PageNumber {
            id: StableId::parse("field-page").expect("valid id"),
            field: PageNumberField::CurrentPage,
        },
        Inline::text(" of "),
        Inline::PageNumber {
            id: StableId::parse("field-count").expect("valid id"),
            field: PageNumberField::PageCount,
        },
    ];
    *source.furniture_mut(HeaderFooterSlot::Header) = vec![header];

    let layout = opendoc_layout::layout_document(&source);
    let painted = opendoc_layout::layout_painted_document(&source);
    assert_eq!(layout.page_count as usize, painted.pages.len());

    // The furniture is drawn per page, so each page's header text differs by
    // exactly the resolved field. Reading it back out of the painted runs
    // rather than out of the PDF keeps this test about the placement, and
    // leaves the glyph round trip to the extraction test in `opendoc-app`.
    for (index, page) in painted.pages.iter().enumerate() {
        let drawn: String = page
            .items
            .iter()
            .filter_map(|item| match item {
                PaintItem::Text { runs, .. } => {
                    Some(runs.iter().map(|run| run.text.as_str()).collect::<String>())
                }
                _ => None,
            })
            .collect();
        let expected = format!("page {} of {}", index + 12, layout.page_count);
        assert!(
            drawn.contains(&expected),
            "page {} does not carry {expected:?}",
            index + 1
        );
    }
}

#[test]
fn a_checklist_item_draws_a_box_and_a_marker_free_line() {
    let mut block = paragraph("item", "buy milk");
    block.kind = BlockKind::ListItem {
        list_id: StableId::parse("list-1").expect("valid id"),
        level: 0,
        kind: ListKind::Checklist { checked: false },
    };
    let painted = opendoc_layout::layout_painted_document(&document(vec![block]));
    let strokes = painted.pages[0]
        .items
        .iter()
        .filter(|item| matches!(item, PaintItem::Stroke { .. }))
        .count();
    assert_eq!(1, strokes, "the checkbox was not drawn");
}

#[test]
fn an_ordered_list_numbers_its_items() {
    let items: Vec<Block> = (0..3)
        .map(|index| {
            let mut block = paragraph(&format!("item-{index}"), "an item");
            block.kind = BlockKind::ListItem {
                list_id: StableId::parse("list-1").expect("valid id"),
                level: 0,
                kind: ListKind::Ordered,
            };
            block
        })
        .collect();
    let painted = opendoc_layout::layout_painted_document(&document(items));
    let drawn: Vec<String> = painted.pages[0]
        .items
        .iter()
        .filter_map(|item| match item {
            PaintItem::Text { runs, .. } => {
                Some(runs.iter().map(|run| run.text.as_str()).collect::<String>())
            }
            _ => None,
        })
        .collect();
    for expected in ["1.", "2.", "3."] {
        assert!(
            drawn.iter().any(|line| line.starts_with(expected)),
            "no marker {expected:?} among {drawn:?}"
        );
    }
}

#[test]
fn an_image_block_warns_that_it_is_a_frame_rather_than_a_picture() {
    let mut block = paragraph("img", "");
    block.content.clear();
    block.kind = BlockKind::Image {
        blob_hash: "sha256:deadbeef".to_string(),
        alt_text: "a square".to_string(),
        layout: opendoc_core::ImageLayout::default(),
    };
    let pdf = export_pdf(&document(vec![block]));
    assert!(
        pdf.warnings
            .iter()
            .any(|warning| warning.code == "pdf-image-not-drawn"),
        "an undrawn image said nothing: {:?}",
        pdf.warnings
    );
}

#[test]
fn a_decorative_image_is_marked_as_a_pdf_artifact() {
    // Empty alternative text is the model's explicit decorative-image value,
    // not a missing accessible name.  Even a visible fallback frame must not
    // become untagged PDF content that an assistive reader has to guess at.
    let mut block = paragraph("decorative-img", "");
    block.content.clear();
    block.kind = BlockKind::Image {
        blob_hash: "sha256:unavailable-decoration".to_string(),
        alt_text: String::new(),
        layout: opendoc_core::ImageLayout::default(),
    };

    let pdf = export_pdf(&document(vec![block]));
    let operators = operators(&pdf.bytes);
    assert!(
        operators.contains("/Artifact BMC") && operators.contains("EMC"),
        "decorative image was left as untagged painted content: {operators}"
    );
    assert!(
        !operators.contains("/Figure <<") && !operators.contains("/ActualText"),
        "a decorative image gained an invented accessible description: {operators}"
    );
}

#[test]
fn a_missing_positioned_image_anchor_reaches_pdf_as_an_explicit_fallback_warning() {
    let mut block = paragraph("positioned", "");
    block.content.clear();
    block.kind = BlockKind::Image {
        blob_hash: "sha256:deadbeef".to_string(),
        alt_text: "overlay".to_string(),
        layout: opendoc_core::ImageLayout {
            width: Some(Length::from_twips(720).unwrap()),
            height: Some(Length::from_twips(360).unwrap()),
            positioned: Some(opendoc_core::PositionedImage {
                anchor: opendoc_core::PositionedImageAnchor::Block(
                    StableId::parse("concurrently-deleted-anchor").unwrap(),
                ),
                horizontal_offset: Length::from_twips(0).unwrap(),
                vertical_offset: Length::from_twips(0).unwrap(),
                layer: opendoc_core::PositionedImageLayer::InFrontOfText,
            }),
            ..Default::default()
        },
    };
    let pdf = export_pdf(&document(vec![block]));
    assert!(
        pdf.warnings
            .iter()
            .any(|warning| warning.code == "positioned-image-anchor-fallback"),
        "missing anchor was silently retargeted: {:?}",
        pdf.warnings
    );
}

#[test]
fn a_wrapped_image_names_the_pdfs_in_flow_fallback() {
    let mut block = paragraph("wrapped", "");
    block.content.clear();
    block.kind = BlockKind::Image {
        blob_hash: "sha256:deadbeef".to_string(),
        alt_text: "wrapped image".to_string(),
        layout: opendoc_core::ImageLayout {
            placement: Some(opendoc_core::ImagePlacement::WrapEnd),
            ..Default::default()
        },
    };

    let pdf = export_pdf(&document(vec![block]));
    assert!(
        pdf.warnings.iter().any(|warning| {
            warning.code == "pdf-image-wrap-not-laid-out"
                && warning.message.contains("wrapped")
                && warning.message.contains("wrap-end")
        }),
        "wrapped image was silently exported as a block: {:?}",
        pdf.warnings
    );
}

#[test]
fn a_supplied_png_is_embedded_as_an_image_xobject() {
    // A valid opaque 2×3 PNG. The bytes matter: this is not a test of a
    // resource dictionary that names an image which was never decoded.
    let png = vec![
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x03, 0x08, 0x02, 0x00, 0x00, 0x00, 0x36,
        0x88, 0x49, 0xd6, 0x00, 0x00, 0x00, 0x10, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0x38,
        0x61, 0x64, 0x04, 0x44, 0x0c, 0x28, 0x14, 0x00, 0x4d, 0x6d, 0x07, 0x09, 0x59, 0xaa, 0x18,
        0x7e, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];
    let mut block = paragraph("img", "");
    block.content.clear();
    block.kind = BlockKind::Image {
        blob_hash: "sha256:deadbeef".to_string(),
        alt_text: "a square".to_string(),
        layout: opendoc_core::ImageLayout {
            height: Some(Length::from_twips(1_440).unwrap()),
            ..Default::default()
        },
    };
    let images = BTreeMap::from([(
        "sha256:deadbeef".to_string(),
        PdfImage {
            media_type: " Image/PNG; profile=display-p3 ".to_string(),
            bytes: png,
        },
    )]);
    let pdf = export_pdf_with_images(&document(vec![block]), &images);
    let operators = operators(&pdf.bytes);
    assert!(
        operators.contains("/Subtype /Image"),
        "no image object: {operators}"
    );
    assert!(
        operators.contains("/Im0 Do"),
        "image was not painted: {operators}"
    );
    assert!(
        !pdf.warnings.iter().any(|warning| {
            warning.code == "pdf-image-not-drawn" || warning.code == "pdf-image-unsupported-format"
        }),
        "a parameterised case-variant PNG was not embedded: {:?}",
        pdf.warnings
    );
}

#[test]
fn a_supplied_bmp_is_rasterized_without_replacing_its_source_bytes() {
    // A valid, uncompressed 2×1 24-bit BMP: red then green, with the required
    // two bytes of row padding. BMP is accepted by every blob path, while PDF
    // must turn its container into RGB samples for an image XObject.
    let bmp = vec![
        0x42, 0x4d, 0x3e, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x36, 0x00, 0x00, 0x00, 0x28,
        0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x18, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x13, 0x0b, 0x00, 0x00, 0x13, 0x0b, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0x00, 0x00, 0x00,
        0xff, 0x00, 0x00, 0x00,
    ];
    let mut block = paragraph("img", "");
    block.content.clear();
    block.kind = BlockKind::Image {
        blob_hash: "sha256:bmp".to_string(),
        alt_text: "two pixels".to_string(),
        layout: opendoc_core::ImageLayout::default(),
    };
    let images = BTreeMap::from([(
        "sha256:bmp".to_string(),
        PdfImage {
            media_type: "image/bmp".to_string(),
            bytes: bmp.clone(),
        },
    )]);

    let pdf = export_pdf_with_images(&document(vec![block]), &images);
    let operators = operators(&pdf.bytes);
    assert!(
        operators.contains("/Subtype /Image"),
        "no image object: {operators}"
    );
    assert!(
        operators.contains("/Im0 Do"),
        "image was not painted: {operators}"
    );
    assert!(pdf
        .warnings
        .iter()
        .any(|warning| warning.code == "pdf-image-rasterized"));
    assert!(!pdf
        .warnings
        .iter()
        .any(|warning| warning.code == "pdf-image-unsupported-format"));
    assert_eq!(
        images["sha256:bmp"].bytes, bmp,
        "PDF must not rewrite blobs"
    );
}

#[test]
fn a_self_contained_svg_is_bounded_rasterized_without_replacing_its_source_bytes() {
    // SVG is deliberately rendered through a no-files/no-URLs static path.
    // The transparent half also proves that the fallback carries a soft mask
    // rather than silently flattening the image against an arbitrary colour.
    let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="2" height="1" viewBox="0 0 2 1"><rect width="1" height="1" fill="#f00"/><rect x="1" width="1" height="1" fill="#00f" fill-opacity=".5"/></svg>"##.to_vec();
    let mut block = paragraph("img", "");
    block.content.clear();
    block.kind = BlockKind::Image {
        blob_hash: "sha256:svg".to_string(),
        alt_text: "two vector pixels".to_string(),
        layout: opendoc_core::ImageLayout::default(),
    };
    let images = BTreeMap::from([(
        "sha256:svg".to_string(),
        PdfImage {
            media_type: "image/svg+xml".to_string(),
            bytes: svg.clone(),
        },
    )]);

    let pdf = export_pdf_with_images(&document(vec![block]), &images);
    let operators = operators(&pdf.bytes);
    assert!(
        operators.contains("/Subtype /Image"),
        "no image object: {operators}"
    );
    assert!(
        operators.contains("/SMask"),
        "transparent SVG lost its alpha: {operators}"
    );
    assert!(
        operators.contains("/Im0 Do"),
        "SVG was not painted: {operators}"
    );
    assert!(
        operators.contains("/Figure <<")
            && operators.contains("/ActualText (two vector pixels)")
            && operators.contains(">> BDC")
            && operators.contains("EMC"),
        "PDF image lost its model accessibility text: {operators}"
    );
    assert!(pdf.warnings.iter().any(|warning| {
        warning.code == "pdf-image-rasterized"
            && warning.message.contains("image/svg+xml")
            && warning.message.contains("not changed")
    }));
    assert!(!pdf
        .warnings
        .iter()
        .any(|warning| warning.code == "pdf-svg-not-rasterized"));
    assert_eq!(
        images["sha256:svg"].bytes, svg,
        "PDF must not rewrite blobs"
    );
}

#[test]
fn svg_with_embedded_or_external_images_stays_a_warned_placeholder() {
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"><image href="file:///not-permitted.png" width="1" height="1"/></svg>"#.to_vec();
    let mut block = paragraph("img", "");
    block.content.clear();
    block.kind = BlockKind::Image {
        blob_hash: "sha256:unsafe-svg".to_string(),
        alt_text: "unsafe SVG".to_string(),
        layout: opendoc_core::ImageLayout::default(),
    };
    let images = BTreeMap::from([(
        "sha256:unsafe-svg".to_string(),
        PdfImage {
            media_type: "image/svg+xml".to_string(),
            bytes: svg.clone(),
        },
    )]);

    let pdf = export_pdf_with_images(&document(vec![block]), &images);
    assert!(pdf.warnings.iter().any(|warning| {
        warning.code == "pdf-svg-not-rasterized"
            && warning.message.contains("embedded or external SVG images")
    }));
    let operators = operators(&pdf.bytes);
    assert!(!operators.contains("/Subtype /Image"));
    assert!(
        operators.contains("/Figure <<")
            && operators.contains("/ActualText (unsafe SVG)")
            && operators.contains(">> BDC")
            && operators.contains("EMC"),
        "a warned image placeholder lost its model accessibility text: {operators}"
    );
    assert_eq!(images["sha256:unsafe-svg"].bytes, svg);
}

#[test]
fn a_supplied_webp_is_bounded_rasterized_without_replacing_its_source_bytes() {
    // A valid 1×1 lossy WebP. PDF has no WebP image filter, so the bounded
    // decoder supplies RGB samples while the blob store retains this container.
    let webp = vec![
        0x52, 0x49, 0x46, 0x46, 0x3c, 0x00, 0x00, 0x00, 0x57, 0x45, 0x42, 0x50, 0x56, 0x50, 0x38,
        0x20, 0x30, 0x00, 0x00, 0x00, 0xd0, 0x01, 0x00, 0x9d, 0x01, 0x2a, 0x01, 0x00, 0x01, 0x00,
        0x02, 0x00, 0x34, 0x25, 0xa0, 0x02, 0x74, 0xba, 0x01, 0xf8, 0x00, 0x03, 0xb0, 0x00, 0xfe,
        0xf0, 0xc4, 0x0b, 0xff, 0x20, 0xb9, 0x61, 0x75, 0xc8, 0xd7, 0xff, 0x20, 0x3f, 0xe4, 0x07,
        0xfc, 0x80, 0xff, 0xf8, 0xf2, 0x00, 0x00, 0x00,
    ];
    let mut block = paragraph("img", "");
    block.content.clear();
    block.kind = BlockKind::Image {
        blob_hash: "sha256:webp".to_string(),
        alt_text: "one pixel".to_string(),
        layout: opendoc_core::ImageLayout::default(),
    };
    let images = BTreeMap::from([(
        "sha256:webp".to_string(),
        PdfImage {
            media_type: "image/webp".to_string(),
            bytes: webp.clone(),
        },
    )]);

    let pdf = export_pdf_with_images(&document(vec![block]), &images);
    assert!(
        operators(&pdf.bytes).contains("/Subtype /Image"),
        "no image object; warnings: {:?}",
        pdf.warnings
    );
    assert!(pdf.warnings.iter().any(|warning| {
        warning.code == "pdf-image-rasterized"
            && warning.message.contains("image/webp")
            && warning.message.contains("not changed")
    }));
    assert!(!pdf
        .warnings
        .iter()
        .any(|warning| warning.code == "pdf-image-unsupported-format"));
    assert_eq!(
        images["sha256:webp"].bytes, webp,
        "PDF must not rewrite blobs"
    );
}

#[test]
fn a_supplied_gif_is_first_frame_rasterized_without_replacing_its_source_bytes() {
    // A valid 1×1 GIF89a. PDF has no GIF image filter, so export deliberately
    // draws the decoder's first frame as RGB samples and leaves animation (if
    // any) in the caller-owned blob bytes.
    let gif = vec![
        0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 0x01, 0x00, 0x01, 0x00, 0x80, 0x00, 0x00, 0xff, 0xff,
        0xff, 0x00, 0x00, 0x00, 0x21, 0xf9, 0x04, 0x01, 0x00, 0x00, 0x00, 0x00, 0x2c, 0x00, 0x00,
        0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x02, 0x02, 0x44, 0x01, 0x00, 0x3b,
    ];
    let pdf = assert_raster_image_is_embedded("gif", "image/gif", gif);
    assert!(pdf.warnings.iter().any(|warning| {
        warning.code == "pdf-image-first-frame-only"
            && warning.message.contains("first frame")
            && warning.message.contains("image/gif")
    }));
}

#[test]
fn a_supplied_tiff_is_rasterized_without_replacing_its_source_bytes() {
    // A baseline little-endian 1×1 RGB TIFF, uncompressed. TIFF's container
    // cannot be embedded directly in PDF, so it takes the same bounded decode
    // route as BMP/WebP without changing the document blob.
    let tiff = vec![
        0x49, 0x49, 0x2a, 0x00, 0x08, 0x00, 0x00, 0x00, 0x09, 0x00, 0x00, 0x01, 0x04, 0x00, 0x01,
        0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x01, 0x04, 0x00, 0x01, 0x00, 0x00, 0x00,
        0x01, 0x00, 0x00, 0x00, 0x02, 0x01, 0x03, 0x00, 0x03, 0x00, 0x00, 0x00, 0x7a, 0x00, 0x00,
        0x00, 0x03, 0x01, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x06, 0x01,
        0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x11, 0x01, 0x04, 0x00, 0x01,
        0x00, 0x00, 0x00, 0x80, 0x00, 0x00, 0x00, 0x15, 0x01, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00,
        0x03, 0x00, 0x00, 0x00, 0x16, 0x01, 0x04, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00,
        0x00, 0x17, 0x01, 0x04, 0x00, 0x01, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x08, 0x00, 0x08, 0x00, 0x08, 0x00, 0xff, 0x00, 0x00,
    ];
    let pdf = assert_raster_image_is_embedded("tiff", "image/tiff", tiff);
    assert!(pdf.warnings.iter().any(|warning| {
        warning.code == "pdf-image-first-frame-only"
            && warning.message.contains("first page")
            && warning.message.contains("image/tiff")
    }));
}

fn assert_raster_image_is_embedded(label: &str, media_type: &str, bytes: Vec<u8>) -> PdfExport {
    let hash = format!("sha256:{label}");
    let mut block = paragraph("img", "");
    block.content.clear();
    block.kind = BlockKind::Image {
        blob_hash: hash.clone(),
        alt_text: "one pixel".to_string(),
        layout: opendoc_core::ImageLayout::default(),
    };
    let images = BTreeMap::from([(
        hash,
        PdfImage {
            media_type: media_type.to_string(),
            bytes: bytes.clone(),
        },
    )]);

    let pdf = export_pdf_with_images(&document(vec![block]), &images);
    assert!(
        operators(&pdf.bytes).contains("/Subtype /Image"),
        "no image object; warnings: {:?}",
        pdf.warnings
    );
    assert!(pdf.warnings.iter().any(|warning| {
        warning.code == "pdf-image-rasterized"
            && warning.message.contains(media_type)
            && warning.message.contains("not changed")
    }));
    assert!(!pdf
        .warnings
        .iter()
        .any(|warning| warning.code == "pdf-image-unsupported-format"));
    assert_eq!(images.values().next().unwrap().bytes, bytes);
    pdf
}

#[test]
fn image_effects_emit_pdf_crop_rotation_and_opacity_operators() {
    let png = vec![
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x03, 0x08, 0x02, 0x00, 0x00, 0x00, 0x36,
        0x88, 0x49, 0xd6, 0x00, 0x00, 0x00, 0x10, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0x38,
        0x61, 0x64, 0x04, 0x44, 0x0c, 0x28, 0x14, 0x00, 0x4d, 0x6d, 0x07, 0x09, 0x59, 0xaa, 0x18,
        0x7e, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];
    let mut block = paragraph("img", "");
    block.content.clear();
    block.kind = BlockKind::Image {
        blob_hash: "sha256:deadbeef".to_string(),
        alt_text: "a square".to_string(),
        layout: opendoc_core::ImageLayout {
            height: Some(Length::from_twips(1_440).unwrap()),
            rotation_degrees: Some(90),
            opacity_percent: Some(40),
            crop: Some(opendoc_core::ImageCrop {
                top_percent: 10,
                right_percent: 20,
                bottom_percent: 30,
                left_percent: 5,
            }),
            ..Default::default()
        },
    };
    let images = BTreeMap::from([(
        "sha256:deadbeef".to_string(),
        PdfImage {
            media_type: "image/png".to_string(),
            bytes: png,
        },
    )]);
    let pdf = export_pdf_with_images(&document(vec![block]), &images);
    let operators = operators(&pdf.bytes);
    assert!(
        operators.contains("/GS40 gs"),
        "no opacity state: {operators}"
    );
    assert!(operators.contains("/ca 0.4"), "no alpha state: {operators}");
    assert!(
        operators.contains("\nW\nn\n"),
        "no crop clipping path: {operators}"
    );
    // A right-angle rotation puts zeroes in the matrix instead of the normal
    // axis-aligned `width 0 0 height` form.
    assert!(
        operators.contains(" -72 "),
        "no rotated matrix: {operators}"
    );
}

#[test]
fn an_estimated_block_is_named_in_the_warnings() {
    // A table's row heights are an estimate — `opendoc-layout` says so, and a
    // PDF built on one must pass that on rather than presenting a guess as a
    // measurement.
    let mut block = paragraph("tbl", "");
    block.content.clear();
    block.kind = BlockKind::table(vec![opendoc_core::TableRow {
        id: StableId::parse("row-0").expect("valid id"),
        height: None,
        header: false,
        cells: vec![opendoc_core::TableCell::new(vec![Block::paragraph("cell")])],
    }]);
    let pdf = export_pdf(&document(vec![block]));
    let warning = pdf
        .warnings
        .iter()
        .find(|warning| warning.code == "pdf-estimated-table")
        .expect("an estimated table said nothing");
    assert!(
        warning.message.contains("tbl"),
        "the warning does not name the block: {}",
        warning.message
    );
}

#[test]
fn a_table_header_repeats_without_a_pdf_warning() {
    let mut block = paragraph("header-table", "");
    block.content.clear();
    block.kind = BlockKind::table(vec![
        opendoc_core::TableRow {
            id: StableId::parse("header-row").expect("valid id"),
            height: Some(Length::from_twips(700).expect("row height")),
            header: true,
            cells: vec![opendoc_core::TableCell::new(vec![Block::paragraph(
                "Heading",
            )])],
        },
        opendoc_core::TableRow {
            id: StableId::parse("body-row").expect("valid id"),
            height: Some(Length::from_twips(700).expect("row height")),
            header: false,
            cells: vec![opendoc_core::TableCell::new(vec![Block::paragraph("Body")])],
        },
        opendoc_core::TableRow {
            id: StableId::parse("body-row-two").expect("valid id"),
            height: Some(Length::from_twips(700).expect("row height")),
            header: false,
            cells: vec![opendoc_core::TableCell::new(vec![Block::paragraph(
                "Second body",
            )])],
        },
    ]);
    let mut source = document(vec![block]);
    source.page_setup = short_page();
    let pdf = export_pdf(&source);
    assert_eq!(2, operators(&pdf.bytes).matches("/Type /Page\n").count());
    assert!(!pdf
        .warnings
        .iter()
        .any(|warning| warning.code == "pdf-table-header-not-repeated"));
}

#[test]
fn a_table_header_that_fits_one_page_does_not_claim_it_cannot_repeat() {
    let mut block = paragraph("header-table-fits", "");
    block.content.clear();
    block.kind = BlockKind::table(vec![opendoc_core::TableRow {
        id: StableId::parse("header-row-fits").expect("valid id"),
        height: None,
        header: true,
        cells: vec![opendoc_core::TableCell::new(vec![Block::paragraph(
            "Heading",
        )])],
    }]);
    let pdf = export_pdf(&document(vec![block]));
    assert!(
        !pdf.warnings
            .iter()
            .any(|warning| warning.code == "pdf-table-header-not-repeated"),
        "a one-page table cannot need a repeated header: {:?}",
        pdf.warnings
    );
}

#[test]
fn a_merged_table_cell_reaches_pdf_without_a_stale_fidelity_warning() {
    let mut block = paragraph("merged-table", "");
    block.content.clear();
    block.kind = BlockKind::table(vec![
        opendoc_core::TableRow {
            id: StableId::parse("merged-row-0").expect("valid id"),
            height: None,
            header: false,
            cells: vec![
                opendoc_core::TableCell::new(vec![Block::paragraph("ANCHOR")]),
                opendoc_core::TableCell::new(vec![Block::paragraph("COVERED")]),
            ],
        },
        opendoc_core::TableRow {
            id: StableId::parse("merged-row-1").expect("valid id"),
            height: None,
            header: false,
            cells: vec![
                opendoc_core::TableCell::new(vec![Block::paragraph("COVERED")]),
                opendoc_core::TableCell::new(vec![Block::paragraph("COVERED")]),
            ],
        },
    ]);
    let BlockKind::Table { rows, .. } = &mut block.kind else {
        panic!("expected table");
    };
    rows[0].cells[0].span = opendoc_core::CellSpan::new(2, 2).expect("legal span");
    let pdf = export_pdf(&document(vec![block]));
    assert!(pdf.bytes.starts_with(b"%PDF-1."));
    assert!(
        !pdf.warnings
            .iter()
            .any(|warning| warning.code == "pdf-merged-cells-drawn-separately"),
        "{:?}",
        pdf.warnings
    );
}

#[test]
fn a_document_of_only_measured_blocks_warns_about_nothing() {
    let pdf = export_pdf(&document(vec![
        paragraph("p1", "plain paragraphs"),
        paragraph("p2", "and nothing else"),
    ]));
    assert!(
        pdf.warnings.is_empty(),
        "an exactly laid out document warned: {:?}",
        pdf.warnings
    );
}

#[test]
fn text_outside_the_bundled_subset_is_reported_rather_than_drawn_blank_in_silence() {
    let pdf = export_pdf(&document(vec![paragraph("p1", "日本語")]));
    assert!(
        pdf.warnings
            .iter()
            .any(|warning| warning.code == "pdf-glyph-outside-bundled-font"),
        "a character with no glyph said nothing: {:?}",
        pdf.warnings
    );
}

#[test]
fn the_same_document_exports_to_the_same_bytes() {
    // No clock, no file id, no hash-map iteration order. A non-deterministic
    // export cannot be diffed, cached or reproduced.
    //
    // `export_pdf(&x) == export_pdf(&x)` is what this used to say, and that is
    // satisfied by an exporter that returns a constant — `Vec::new()` passes
    // it — and by one that embeds the *same* clock reading twice. PLAN88 §7.
    // So: the two documents are built separately rather than shared, the
    // bytes have to differ when the content differs, and the file must carry
    // nothing that a second run could legitimately change.
    let build = || document(vec![paragraph("p1", "deterministic"), paragraph("p2", "x")]);
    let first = export_pdf(&build()).bytes;
    let second = export_pdf(&build()).bytes;
    assert_eq!(
        first,
        second,
        "two exports of the same content disagreed at byte {:?}",
        first
            .iter()
            .zip(second.iter())
            .position(|(left, right)| left != right)
    );
    assert!(!first.is_empty(), "the export produced no bytes at all");

    // Sensitivity: one character of difference has to reach the file, or
    // "the same document exports to the same bytes" is true of an exporter
    // that ignores the document.
    let changed = export_pdf(&document(vec![
        paragraph("p1", "deterministic"),
        paragraph("p2", "y"),
    ]))
    .bytes;
    assert_ne!(
        first, changed,
        "changing the text changed nothing in the exported file"
    );

    // …and nothing volatile is written. A `/CreationDate` or a random
    // `/ID` would make two *runs* differ while two calls in one process
    // agreed, which is the failure the assertion above cannot see.
    let text = operators(&first);
    for volatile in ["/CreationDate", "/ModDate", "/ID ", "/ID["] {
        assert!(
            !text.contains(volatile),
            "the export writes {volatile}, which cannot be reproduced by a later run"
        );
    }
}

#[test]
fn every_drawn_block_lands_inside_the_content_box_of_the_page_it_was_assigned() {
    // The property the whole design rests on: the painted coordinates and the
    // paginated ones come from the same pass, so a line can never be drawn on
    // a page its block does not belong to.
    let blocks: Vec<Block> = (0..40)
        .map(|index| {
            paragraph(
                &format!("block-{index}"),
                "The quick brown fox jumps over the lazy dog.",
            )
        })
        .collect();
    let mut source = document(blocks);
    source.page_setup = short_page();
    let setup = source.page_setup;
    let painted = opendoc_layout::layout_painted_document(&source);

    let top = setup.margin_top.twips();
    let bottom = top + setup.content_height().twips();
    let left = setup.margin_start.twips();
    let right = left + setup.content_width().twips();
    for (index, page) in painted.pages.iter().enumerate() {
        for item in &page.items {
            let PaintItem::Text {
                baseline_twips,
                runs,
            } = item
            else {
                continue;
            };
            assert!(
                *baseline_twips > top && *baseline_twips <= bottom,
                "page {index} draws a baseline at {baseline_twips}, outside {top}..{bottom}"
            );
            for run in runs {
                assert!(
                    run.x_twips >= left && run.x_twips < right,
                    "page {index} draws a run at x={}, outside {left}..{right}",
                    run.x_twips
                );
            }
        }
    }
}

// ---- What the PDF used to drop in silence -------------------------------

/// A paragraph whose runs carry one of every mark that changes how text is
/// *drawn* rather than how wide it is.
fn decorated_paragraph() -> Block {
    let mut block = paragraph("block-0", "plain ");
    for (text, kind, value) in [
        ("red", MarkKind::Color, Some("#cc0000")),
        ("lit", MarkKind::Background, Some("#ffff00")),
        ("under", MarkKind::Underline, None),
        ("struck", MarkKind::Strike, None),
    ] {
        block.content.push(Inline::Text {
            id: StableId::new("inline"),
            text: text.to_string(),
            marks: vec![Mark {
                kind,
                value: value.map(str::to_string),
                expand: opendoc_core::MarkExpand::None,
            }],
        });
    }
    block.content.push(Inline::Link {
        id: StableId::new("inline"),
        text: "click".to_string(),
        href: "https://example.invalid/a".to_string(),
        marks: Vec::new(),
    });
    block
}

#[test]
fn a_coloured_run_is_drawn_in_its_colour() {
    let pdf = export_pdf(&document(vec![decorated_paragraph()]));
    let drawn = operators(&pdf.bytes);
    // `rg` is PDF's device-RGB fill colour. #cc0000 is 0.8 0 0.
    assert!(
        drawn.contains("0.8 0 0 rg"),
        "the colour mark was dropped: {drawn}"
    );
    // And the run after it goes back to black rather than inheriting the red.
    assert!(
        drawn.contains("0 0 0 rg"),
        "the fill colour was never reset"
    );
}

#[test]
fn a_highlight_an_underline_and_a_strike_are_drawn_as_rules() {
    let pdf = export_pdf(&document(vec![decorated_paragraph()]));
    let drawn = operators(&pdf.bytes);
    // The highlight is a yellow rectangle filled before the text.
    assert!(
        drawn.contains("1 1 0 rg"),
        "the highlight was dropped: {drawn}"
    );
    // Three filled rectangles at least: the highlight, the underline and the
    // strikethrough. A document with none of those draws no `re … f` at all.
    let filled = drawn.matches(" re\n").count();
    let plain = export_pdf(&document(vec![paragraph("block-0", "plain")]));
    assert!(
        filled > operators(&plain.bytes).matches(" re\n").count() + 2,
        "underline and strike drew nothing: {filled} rectangles"
    );
}

#[test]
fn a_link_becomes_a_link_annotation_that_names_its_target() {
    let pdf = export_pdf(&document(vec![decorated_paragraph()]));
    let drawn = operators(&pdf.bytes);
    assert!(
        drawn.contains("/Subtype /Link"),
        "no link annotation was emitted"
    );
    assert!(drawn.contains("/S /URI"));
    assert!(drawn.contains("https://example.invalid/a"));
    assert!(
        drawn.contains("/Annots"),
        "the page does not carry its annotations"
    );
    // A document with no links carries no annotations at all.
    let plain = export_pdf(&document(vec![paragraph("block-0", "plain")]));
    assert!(!operators(&plain.bytes).contains("/Annots"));
}

#[test]
fn a_superscript_is_drawn_above_the_baseline_of_the_run_beside_it() {
    let mut block = paragraph("block-0", "x");
    block.content.push(Inline::Text {
        id: StableId::new("inline"),
        text: "9".to_string(),
        marks: vec![Mark {
            kind: MarkKind::Superscript,
            value: None,
            expand: opendoc_core::MarkExpand::None,
        }],
    });
    let doc = document(vec![block]);
    let painted = opendoc_layout::layout_painted_document(&doc);
    let runs: Vec<&opendoc_layout::PaintRun> = painted.pages[0]
        .items
        .iter()
        .filter_map(|item| match item {
            PaintItem::Text { runs, .. } => Some(runs),
            _ => None,
        })
        .flatten()
        .collect();
    let baseline = match &painted.pages[0].items[0] {
        PaintItem::Text { baseline_twips, .. } => *baseline_twips,
        other => panic!("expected text, got {other:?}"),
    };
    let raised = runs.iter().find(|run| run.text == "9").expect("no script");
    let rise = crate::run_rise(&raised.decoration);
    assert!(rise > 0, "the superscript carries no baseline shift");
    // In PDF user space y grows upwards, so the raised run's text matrix sits
    // higher up the page than the line's own baseline by exactly the shift.
    let page_height = PageSetup::default().height.twips();
    let drawn = operators(&export_pdf(&doc).bytes);
    let plain_y = pt(page_height - baseline);
    let raised_y = pt(page_height - (baseline - rise));
    assert!(raised_y > plain_y);
    assert!(
        drawn.contains(&format!("{raised_y} Tm")),
        "the superscript was drawn on the baseline: {drawn}"
    );
}

#[test]
fn a_footnote_prints_its_number_and_its_body() {
    let mut block = paragraph("block-0", "see");
    block.content.push(Inline::FootnoteRef {
        id: StableId::new("inline"),
        footnote_id: StableId::parse("note-a").expect("valid id"),
    });
    let mut doc = document(vec![block]);
    doc.footnotes = vec![opendoc_core::Footnote {
        id: StableId::parse("note-a").expect("valid id"),
        revision: 1,
        body: vec![Inline::text("a note about something")],
        deleted: false,
    }];
    let pdf = export_pdf(&doc);
    let painted = opendoc_layout::layout_painted_document(&doc);
    let drawn: String = painted
        .pages
        .iter()
        .flat_map(|page| page.items.iter())
        .filter_map(|item| match item {
            PaintItem::Text { runs, .. } => {
                Some(runs.iter().map(|run| run.text.as_str()).collect::<String>())
            }
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("|");
    assert!(drawn.contains("see1"), "the reference printed a 0: {drawn}");
    assert!(
        drawn.contains("a note about something"),
        "the footnote body was dropped: {drawn}"
    );
    // And the export says where it put them, because that is not where the
    // screen puts them.
    assert!(
        pdf.warnings
            .iter()
            .any(|warning| warning.code == "pdf-footnotes-after-the-body"),
        "{:?}",
        pdf.warnings
    );
}

#[test]
fn an_uncovered_character_extracts_as_nothing_rather_than_as_the_wrong_letter() {
    // `.notdef` is one glyph for every uncovered character. A `ToUnicode`
    // entry for it would claim they are all whichever one reached it first, so
    // an Arabic paragraph came back out of `pdftotext` as the same letter
    // repeated. Absent is honest; wrong is not.
    let pdf = export_pdf(&document(vec![paragraph("block-0", "نص عربي")]));
    let drawn = operators(&pdf.bytes);
    // `<0000>` appears once per embedded face as the low end of the
    // `codespacerange`; a *second* occurrence would be a `bfchar` entry
    // claiming that `.notdef` spells some particular character.
    assert_eq!(
        drawn.matches("endcodespacerange").count(),
        1,
        "the paragraph should have drawn with exactly one embedded face"
    );
    assert_eq!(
        drawn.matches("<0000>").count(),
        1,
        "the cmap claims to know what .notdef stands for"
    );
    assert!(
        pdf.warnings
            .iter()
            .any(|warning| warning.code == "pdf-glyph-outside-bundled-font"),
        "{:?}",
        pdf.warnings
    );
}

#[test]
fn an_explicit_page_break_is_drawn_dashed_as_the_stylesheet_draws_it() {
    let mut rule = paragraph("block-0", "");
    rule.kind = BlockKind::PageBreak;
    rule.content.clear();
    let pdf = export_pdf(&document(vec![
        paragraph("block-1", "before"),
        rule,
        paragraph("block-2", "after"),
    ]));
    let drawn = operators(&pdf.bytes);
    assert!(
        drawn.contains("] 0 d\n"),
        "the page-break rule is solid on paper and dashed on screen: {drawn}"
    );
}

// ---- table borders --------------------------------------------------------
//
// `opendoc-layout` resolves a table's collapsed border grid and hands over one
// `PaintItem::Edge` per boundary, carrying the width, colour and dash the cell
// stated. These check that all three survive the trip into the content stream,
// because until they did a 2.25pt dashed red border printed as a 0.75pt solid
// black one — the writer stroked a rectangle per cell with no colour set at
// all.

/// A table of one cell holding one word, with `properties` applied to it.
fn one_cell_table(properties: opendoc_core::TableCellProperties) -> Block {
    let mut block = paragraph("tbl", "");
    block.content.clear();
    let mut cell = opendoc_core::TableCell::new(vec![Block::paragraph("a")]);
    cell.properties = properties;
    block.kind = BlockKind::table(vec![opendoc_core::TableRow {
        id: StableId::parse("row-0").expect("valid id"),
        height: None,
        header: false,
        cells: vec![cell],
    }]);
    block
}

/// A cell that states one border, on its top edge.
fn top_border(edge: opendoc_core::CellBorder) -> opendoc_core::TableCellProperties {
    opendoc_core::TableCellProperties {
        border_top: Some(edge),
        ..Default::default()
    }
}

fn border(style: opendoc_core::BorderStyle, twips: i32, color: &str) -> opendoc_core::CellBorder {
    opendoc_core::CellBorder::new(
        style,
        Length::from_twips(twips).expect("a border width"),
        opendoc_core::Color::parse(color).expect("a colour"),
    )
    .expect("a valid border")
}

#[test]
fn a_stated_cell_border_is_stroked_in_its_own_colour_width_and_dash() {
    let properties = top_border(border(opendoc_core::BorderStyle::Dashed, 45, "#cc0000"));
    let pdf = export_pdf(&document(vec![one_cell_table(properties)]));
    let text = operators(&pdf.bytes);
    // 0xcc is 204/255 = 0.8; 45 twips is 2.25pt; the dash is four times the
    // thickness on and off, which is 9pt.
    let start = text
        .find("0.8 0 0 RG")
        .unwrap_or_else(|| panic!("the border was not stroked in the colour the cell states"));
    let segment: Vec<&str> = text[start..].lines().take(7).collect();
    let joined = segment.join(" | ");
    assert!(segment.contains(&"2.25 w"), "{joined}");
    assert!(segment.contains(&"[9 9] 0 d"), "{joined}");
    assert!(
        segment.iter().any(|line| line.ends_with(" m")),
        "no line was begun: {joined}"
    );
    assert!(
        segment.iter().any(|line| line.ends_with(" l")),
        "no line was drawn: {joined}"
    );
    assert!(
        segment.contains(&"S"),
        "the line was never stroked: {joined}"
    );
}

/// The default grid is the stylesheet's grey, which is the colour the screen
/// draws. It used to be black, because `PaintItem::Stroke` carried no colour
/// and nothing ever set one.
#[test]
fn the_default_border_grid_is_stroked_in_the_stylesheets_grey() {
    let pdf = export_pdf(&document(vec![one_cell_table(Default::default())]));
    let text = operators(&pdf.bytes);
    // 0x99 is 153/255 = 0.6, and one cell is four boundaries.
    assert_eq!(
        4,
        text.matches("0.6 0.6 0.6 RG").count(),
        "the four boundaries of one cell are not all stroked grey"
    );
    assert_eq!(4, text.matches("0.75 w").count(), "not at 0.75pt");
}

/// A border's colour and dash are graphics state. Left set, they would repaint
/// whatever is drawn next — so the edge is wrapped in `q`/`Q` and the proof is
/// that the restore lands before the next rectangle.
#[test]
fn a_cell_border_does_not_leak_its_dash_into_the_next_thing_drawn() {
    let properties = top_border(border(opendoc_core::BorderStyle::Dashed, 45, "#cc0000"));
    let mut checklist = paragraph("item", "buy milk");
    checklist.kind = BlockKind::ListItem {
        list_id: StableId::parse("list-1").expect("valid id"),
        level: 0,
        kind: ListKind::Checklist { checked: false },
    };
    let pdf = export_pdf(&document(vec![one_cell_table(properties), checklist]));
    let text = operators(&pdf.bytes);
    let dash = text.find("[9 9] 0 d").expect("the dashed border");
    let rest = &text[dash..];
    let restored = rest.find("\nQ\n").expect("the edge is not wrapped in q/Q");
    let checkbox = rest
        .find(" re\n")
        .expect("the checkbox rectangle is not drawn");
    assert!(
        restored < checkbox,
        "the dash pattern is still set when the checkbox is stroked"
    );
    // And the wrapping is balanced. A `Q` without its `q` restores a state
    // that was never saved, which is a malformed stream rather than a leak —
    // the same line of code getting it wrong in the other direction.
    assert_eq!(
        text.matches("\nq\n").count(),
        text.matches("\nQ\n").count(),
        "the content stream saves and restores a different number of times"
    );
}

/// The layout builds every cell's box out of the type scale's 0.75pt border,
/// so a table that states a different width is drawn right and measured wrong.
/// That is a loss, and ADR 0010's rule is that an export says what it lost.
///
/// A border turned *off* is the same loss with the width at the other end: its
/// used width is zero, so the screen's table is shorter than the box measured
/// here by however much of the grid it silenced.
#[test]
fn a_border_width_the_layout_did_not_reserve_room_for_is_warned_about() {
    for (name, edge) in [
        (
            "a thicker border",
            border(opendoc_core::BorderStyle::Solid, 45, "#cc0000"),
        ),
        ("a border turned off", opendoc_core::CellBorder::none()),
    ] {
        let pdf = export_pdf(&document(vec![one_cell_table(top_border(edge))]));
        let warning = pdf
            .warnings
            .iter()
            .find(|warning| warning.code == "pdf-cell-border-width-not-measured")
            .unwrap_or_else(|| panic!("{name}: no warning named the unmeasured border width"));
        assert!(
            warning.message.contains("tbl"),
            "{name}: {}",
            warning.message
        );
        assert!(
            warning.message.contains("0.75pt"),
            "{name}: {}",
            warning.message
        );
    }
}

/// A table border is the inherited value for every cell edge that does not
/// state one.  It used to escape the warning above because the PDF audit
/// inspected only `TableCellProperties`; the layout nevertheless paints and
/// sizes every one of these edges from the table property.
#[test]
fn a_table_border_width_the_layout_did_not_reserve_room_for_is_warned_about() {
    let mut table = one_cell_table(Default::default());
    let BlockKind::Table { properties, .. } = &mut table.kind else {
        panic!("expected table");
    };
    properties.border = Some(border(opendoc_core::BorderStyle::Dashed, 45, "#336699"));

    let pdf = export_pdf(&document(vec![table]));
    let warning = pdf
        .warnings
        .iter()
        .find(|warning| warning.code == "pdf-cell-border-width-not-measured")
        .expect("the inherited table border must be named as unmeasured");
    assert!(warning.message.contains("tbl"), "{}", warning.message);
    assert!(warning.message.contains("0.75pt"), "{}", warning.message);
}

/// ...and stays quiet when there is nothing to say, which is what makes the
/// warning above worth reading. A cell that states the default border, and a
/// cell that states nothing, are both measured exactly as drawn.
#[test]
fn a_border_of_the_width_the_layout_reserves_is_not_warned_about() {
    let properties = top_border(border(opendoc_core::BorderStyle::Dashed, 15, "#cc0000"));
    for (name, block) in [
        ("a stated default-width border", one_cell_table(properties)),
        (
            "no stated border at all",
            one_cell_table(Default::default()),
        ),
    ] {
        let pdf = export_pdf(&document(vec![block]));
        let codes: Vec<&str> = pdf
            .warnings
            .iter()
            .map(|warning| warning.code.as_str())
            .collect();
        assert!(
            codes.contains(&"pdf-estimated-table"),
            "{name}: the fixture stopped being a table: {codes:?}"
        );
        assert!(
            !codes.contains(&"pdf-cell-border-width-not-measured"),
            "{name}: warned about a width it did measure with: {codes:?}"
        );
    }
}

/// A stated background reaches the page now that the paint model carries a
/// fill colour, and stated padding participates in table measurement.
#[test]
fn a_cells_background_and_its_own_padding_reach_pdf_layout() {
    let background = opendoc_core::TableCellProperties {
        background: Some(opendoc_core::Color::parse("#e8f0fe").expect("a colour")),
        ..Default::default()
    };
    let padding = opendoc_core::TableCellProperties {
        padding_start: Some(Length::from_twips(240).expect("a padding")),
        ..Default::default()
    };
    let drawn = export_pdf(&document(vec![one_cell_table(background)]));
    assert!(
        !drawn
            .warnings
            .iter()
            .any(|warning| warning.code == "pdf-cell-background-not-drawn"),
        "background was still described as missing: {:?}",
        drawn.warnings
    );
    // #e8f0fe as PDF device RGB. Checking the operator proves it reached the
    // paper stream rather than merely making the old warning disappear.
    assert!(
        operators(&drawn.bytes).contains("0.9098039 0.9411765 0.99607843 rg"),
        "background colour absent from PDF operators: {}",
        operators(&drawn.bytes)
    );
    let padded = export_pdf(&document(vec![one_cell_table(padding)]));
    assert!(
        !padded
            .warnings
            .iter()
            .any(|warning| warning.code == "pdf-cell-padding-not-measured"),
        "padding was still described as unmeasured: {:?}",
        padded.warnings
    );
    let plain_layout =
        opendoc_layout::layout_document(&document(vec![one_cell_table(Default::default())]));
    let padded_layout = opendoc_layout::layout_document(&document(vec![one_cell_table(
        opendoc_core::TableCellProperties {
            padding_top: Some(Length::from_twips(240).expect("a padding")),
            padding_bottom: Some(Length::from_twips(240).expect("a padding")),
            ..Default::default()
        },
    )]));
    assert!(
        padded_layout.blocks[0].height_twips > plain_layout.blocks[0].height_twips,
        "stated padding did not increase the table box: {plain_layout:?} {padded_layout:?}"
    );

    let positioned_baseline = |alignment| {
        let mut table = one_cell_table(opendoc_core::TableCellProperties {
            vertical_alignment: Some(alignment),
            ..Default::default()
        });
        let BlockKind::Table { rows, .. } = &mut table.kind else {
            panic!("expected table");
        };
        rows[0].height = Some(Length::from_twips(1_440).expect("one inch"));
        opendoc_layout::layout_painted_document(&document(vec![table])).pages[0]
            .items
            .iter()
            .find_map(|item| match item {
                opendoc_layout::PaintItem::Text { baseline_twips, .. } => Some(*baseline_twips),
                _ => None,
            })
            .expect("cell text")
    };
    assert!(
        positioned_baseline(opendoc_core::VerticalAlignment::Top)
            < positioned_baseline(opendoc_core::VerticalAlignment::Middle)
            && positioned_baseline(opendoc_core::VerticalAlignment::Middle)
                < positioned_baseline(opendoc_core::VerticalAlignment::Bottom),
        "vertical cell alignment did not move the PDF text"
    );
}
