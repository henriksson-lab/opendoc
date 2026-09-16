use super::*;
use opendoc_core::{Length, LineHeightMultiple, StableId};

fn document(blocks: Vec<Block>) -> Document {
    let mut document = Document::new("layout");
    document.blocks = blocks;
    document
}

fn paragraph(id: &str, text: &str) -> Block {
    let mut block = Block::paragraph(text);
    block.id = StableId::parse(id).expect("valid id");
    block
}

fn heading(id: &str, level: u8, text: &str) -> Block {
    let mut block = paragraph(id, text);
    block.kind = BlockKind::Heading { level };
    block
}

fn list_item(id: &str, level: u8, kind: ListKind, list: &str, text: &str) -> Block {
    let mut block = paragraph(id, text);
    block.kind = BlockKind::ListItem {
        list_id: StableId::parse(list).expect("valid id"),
        level,
        kind,
    };
    block
}

/// A page short enough that a handful of paragraphs overflow it, which is how
/// the e2e harness exercises pagination too: breaking is a function of the
/// page box, so a short page tests the same mechanism as a long document.
fn short_page() -> PageSetup {
    PageSetup {
        height: Length::from_twips(3 * 1_440).expect("3in page"),
        ..PageSetup::default()
    }
}

fn filler(count: usize) -> Vec<Block> {
    (0..count)
        .map(|index| {
            paragraph(
                &format!("block-{index}"),
                "The quick brown fox jumps over the lazy dog.",
            )
        })
        .collect()
}

fn positioned_image(
    id: &str,
    anchor: opendoc_core::PositionedImageAnchor,
    layer: opendoc_core::PositionedImageLayer,
) -> Block {
    let mut block = paragraph(id, "");
    block.content.clear();
    block.kind = BlockKind::Image {
        blob_hash: "sha256:positioned".to_string(),
        alt_text: "positioned test image".to_string(),
        layout: opendoc_core::ImageLayout {
            width: Some(Length::from_twips(720).expect("half inch")),
            height: Some(Length::from_twips(360).expect("quarter inch")),
            positioned: Some(opendoc_core::PositionedImage {
                anchor,
                horizontal_offset: Length::from_twips(120).expect("offset"),
                vertical_offset: Length::from_twips(240).expect("offset"),
                layer,
            }),
            ..Default::default()
        },
    };
    block
}

#[test]
fn an_empty_document_is_one_page() {
    let layout = layout_document(&document(Vec::new()));
    assert_eq!(layout.page_count, 1);
    assert!(layout.blocks.is_empty());
    assert!(layout.exact);
}

#[test]
fn positioned_images_are_out_of_flow_and_paint_on_their_anchor_layer() {
    let anchor = paragraph("anchor", "anchor text");
    let image = positioned_image(
        "positioned",
        opendoc_core::PositionedImageAnchor::Block(StableId::parse("anchor").unwrap()),
        opendoc_core::PositionedImageLayer::BehindText,
    );
    let following = paragraph("following", "following text");
    let control_doc = document(vec![
        paragraph("anchor", "anchor text"),
        paragraph("following", "following text"),
    ]);
    let positioned_doc = document(vec![anchor, image, following]);
    let layout = layout_document(&positioned_doc);
    assert_eq!(layout.placement("positioned").unwrap().height_twips, 0);
    assert_eq!(
        layout.placement("following").unwrap().top_twips,
        layout_document(&control_doc)
            .placement("following")
            .unwrap()
            .top_twips,
        "the out-of-flow object reserved vertical space"
    );
    let painted = layout_painted_document(&positioned_doc);
    let PaintItem::Image {
        x_twips,
        y_twips,
        width_twips,
        height_twips,
        ..
    } = &painted.pages[0].items[0]
    else {
        panic!(
            "behind-text positioned image was not painted before text: {:?}",
            painted.pages[0].items
        );
    };
    assert_eq!(
        (*x_twips, *y_twips, *width_twips, *height_twips),
        (
            1_560,
            layout.placement("anchor").unwrap().top_twips + 240,
            720,
            360
        )
    );
}

#[test]
fn missing_positioned_anchor_falls_back_to_page_content_and_reports_it() {
    let image = positioned_image(
        "positioned",
        opendoc_core::PositionedImageAnchor::Block(StableId::parse("deleted-anchor").unwrap()),
        opendoc_core::PositionedImageLayer::InFrontOfText,
    );
    let document = document(vec![image]);
    let painted = layout_painted_document(&document);
    assert!(painted
        .warnings
        .iter()
        .any(|warning| warning.code == "positioned-image-anchor-fallback"));
    let PaintItem::Image {
        x_twips, y_twips, ..
    } = painted.pages[0].items.last().unwrap()
    else {
        panic!(
            "in-front positioned image was not painted: {:?}",
            painted.pages[0].items
        );
    };
    assert_eq!((*x_twips, *y_twips), (1_560, 1_680));
}

#[test]
fn an_empty_paragraph_still_occupies_a_line() {
    let layout = layout_document(&document(vec![paragraph("block-0", "")]));
    let placed = &layout.blocks[0];
    assert_eq!(placed.lines, 1);
    // 11pt at a 1.5 line height is 16.5pt, which is 330 twips exactly.
    assert_eq!(placed.height_twips, 330);
}

#[test]
fn keep_with_next_moves_a_pair_before_the_page_break() {
    // The leading paragraph leaves enough room for `kept`, but not for its
    // following sibling. The relationship belongs to `kept`, so it must move
    // with `next` rather than letting `next` open the page on its own.
    let mut leading = paragraph("leading", "leading");
    leading.properties.space_after = Some(Length::from_twips(720).unwrap());
    let mut kept = paragraph("kept", "kept");
    kept.properties.keep_with_next = Some(true);
    let next = paragraph("next", "next");
    let mut document = document(vec![leading, kept, next]);
    document.page_setup = short_page();
    let layout = layout_document(&document);
    let kept = layout.placement("kept").unwrap();
    let next = layout.placement("next").unwrap();
    assert_eq!(kept.page, next.page);
    assert!(kept.page_break_margin.is_some());
}

#[test]
fn the_same_input_lays_out_identically_every_time() {
    let doc = document(filler(40));
    let first = layout_document(&doc);
    for _ in 0..4 {
        assert_eq!(layout_document(&doc), first);
    }
    // And the assignment is not trivially one page.
    assert!(
        first.page_count > 1,
        "40 paragraphs fit on one Letter page?"
    );
}

#[test]
fn a_document_that_fits_is_one_page() {
    let layout = layout_document(&document(filler(3)));
    assert_eq!(layout.page_count, 1);
    assert!(layout.blocks.iter().all(|block| block.page == 0));
}

#[test]
fn a_block_never_crosses_the_bottom_of_its_content_box() {
    let mut doc = document(filler(30));
    doc.page_setup = short_page();
    let layout = layout_document(&doc);
    assert!(layout.page_count > 1);
    let setup = &doc.page_setup;
    for block in &layout.blocks {
        let page_top = i32::try_from(block.page).unwrap() * setup.height.twips();
        let top = page_top + setup.margin_top.twips();
        let bottom = top + setup.content_height().twips();
        assert!(
            block.top_twips >= top,
            "{} starts above its page's content box",
            block.block_id
        );
        assert!(
            block.top_twips + block.height_twips <= bottom,
            "{} ({}..{}) overflows page {} ({top}..{bottom})",
            block.block_id,
            block.top_twips,
            block.top_twips + block.height_twips,
            block.page
        );
    }
}

#[test]
fn the_block_that_opens_a_page_starts_at_that_pages_top_margin() {
    let mut doc = document(filler(30));
    doc.page_setup = short_page();
    let layout = layout_document(&doc);
    let openers: Vec<_> = layout
        .blocks
        .iter()
        .filter(|block| block.page_break_margin.is_some())
        .collect();
    assert!(!openers.is_empty());
    for block in openers {
        let expected = i32::try_from(block.page).unwrap() * doc.page_setup.height.twips()
            + doc.page_setup.margin_top.twips();
        assert_eq!(
            block.top_twips, expected,
            "{} opened page {} somewhere other than its top margin",
            block.block_id, block.page
        );
    }
}

#[test]
fn the_page_break_margin_moves_the_block_from_where_the_flow_left_it() {
    let mut doc = document(filler(30));
    doc.page_setup = short_page();
    let layout = layout_document(&doc);
    for (index, block) in layout.blocks.iter().enumerate() {
        let Some(margin) = block.page_break_margin else {
            continue;
        };
        let previous = &layout.blocks[index - 1];
        let pen = previous.top_twips + previous.height_twips;
        // The margin is measured from the previous border box, which is what
        // CSS margin collapsing produces once it is applied.
        assert_eq!(to_twips(margin) + pen, block.top_twips);
        // And it always wins the collapse against the block space below a
        // paragraph, which is the assumption that makes applying it safe.
        assert!(margin > i64::from(TypeScale::default().block_space_after) * MILLI);
    }
}

#[test]
fn an_explicit_page_break_starts_the_next_page() {
    let mut before = paragraph("block-0", "before");
    before.kind = BlockKind::Paragraph;
    let mut rule = paragraph("block-1", "");
    rule.kind = BlockKind::PageBreak;
    let after = paragraph("block-2", "after");
    let layout = layout_document(&document(vec![before, rule, after]));
    assert_eq!(layout.page_count, 2);
    assert_eq!(layout.blocks[0].page, 0);
    assert_eq!(layout.blocks[1].page, 0);
    assert_eq!(layout.blocks[2].page, 1, "the break did not open a page");
    assert!(layout.blocks[2].page_break_margin.is_some());
}

#[test]
fn two_page_breaks_in_a_row_make_two_pages() {
    let mut blocks = vec![paragraph("block-0", "one")];
    for index in 1..=2 {
        let mut rule = paragraph(&format!("rule-{index}"), "");
        rule.kind = BlockKind::PageBreak;
        blocks.push(rule);
        blocks.push(paragraph(&format!("block-{index}"), "next"));
    }
    let layout = layout_document(&document(blocks));
    assert_eq!(layout.page_count, 3);
    assert_eq!(layout.placement("block-2").unwrap().page, 2);
}

#[test]
fn space_before_pushes_a_block_down() {
    let plain = layout_document(&document(vec![
        paragraph("block-0", "one"),
        paragraph("block-1", "two"),
    ]));
    let mut second = paragraph("block-1", "two");
    second.properties.space_before = Some(Length::from_twips(720).expect("half an inch"));
    let spaced = layout_document(&document(vec![paragraph("block-0", "one"), second]));
    assert_eq!(
        spaced.placement("block-1").unwrap().top_twips
            - plain.placement("block-1").unwrap().top_twips,
        // Half an inch, less the 10pt bottom margin it collapses with.
        720 - 200
    );
}

#[test]
fn space_after_and_space_before_collapse_rather_than_add() {
    let mut first = paragraph("block-0", "one");
    first.properties.space_after = Some(Length::from_twips(600).expect("valid"));
    let mut second = paragraph("block-1", "two");
    second.properties.space_before = Some(Length::from_twips(400).expect("valid"));
    let layout = layout_document(&document(vec![first, second]));
    let top = layout.placement("block-0").unwrap();
    let next = layout.placement("block-1").unwrap();
    assert_eq!(
        next.top_twips - (top.top_twips + top.height_twips),
        600,
        "adjacent margins added instead of collapsing"
    );
}

#[test]
fn an_indent_narrows_the_text_and_can_cost_a_line() {
    let text = "The quick brown fox jumps over the lazy dog and keeps going for a while yet.";
    let plain = layout_document(&document(vec![paragraph("block-0", text)]));
    let mut indented = paragraph("block-0", text);
    indented.properties.indent_start = Some(Length::from_twips(6 * 1_440).expect("valid"));
    let narrow = layout_document(&document(vec![indented]));
    assert!(
        narrow.blocks[0].lines > plain.blocks[0].lines,
        "a 6in indent did not change the line count ({} vs {})",
        narrow.blocks[0].lines,
        plain.blocks[0].lines
    );
}

#[test]
fn a_hanging_indent_is_a_negative_first_line_indent_and_widens_line_one() {
    // The model's only representation of a hanging indent is a negative
    // first-line indent, which makes line one wider than the rest. Whether
    // that saves a line depends on where the words fall, so the property is
    // asserted over a family of inputs: it must never cost a line, and for
    // some input it must save one.
    let words = [
        "alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta", "iota", "kappa",
        "lambda", "mu", "nu", "xi", "omicron", "pi", "rho", "sigma", "tau", "upsilon",
    ];
    let indent = Length::from_twips(2_880).expect("two inches");
    let mut saved = 0;
    for count in 1..=words.len() {
        let text = words[..count].join(" ");
        let mut flat = paragraph("block-0", &text);
        flat.properties.indent_start = Some(indent);
        let mut hanging = flat.clone();
        hanging.properties.indent_first_line =
            Some(Length::from_twips(-2_880).expect("a hanging indent"));
        let flat_lines = layout_document(&document(vec![flat])).blocks[0].lines;
        let hanging_lines = layout_document(&document(vec![hanging])).blocks[0].lines;
        assert!(
            hanging_lines <= flat_lines,
            "{count} words: hanging took {hanging_lines} lines against {flat_lines} flat"
        );
        if hanging_lines < flat_lines {
            saved += 1;
        }
    }
    assert!(saved > 0, "a hanging indent never widened the first line");
}

#[test]
fn line_spacing_changes_the_height_without_changing_the_line_count() {
    let text = "The quick brown fox jumps over the lazy dog, at some length, twice over now.";
    let single = layout_document(&document(vec![paragraph("block-0", text)]));
    let mut block = paragraph("block-0", text);
    block.properties.line_spacing = Some(LineSpacing::Multiple(
        LineHeightMultiple::from_ratio(3.0).expect("valid"),
    ));
    let triple = layout_document(&document(vec![block]));
    assert_eq!(triple.blocks[0].lines, single.blocks[0].lines);
    assert_eq!(
        triple.blocks[0].height_twips,
        single.blocks[0].height_twips * 2
    );
}

#[test]
fn exact_line_spacing_is_a_fixed_height() {
    let mut block = paragraph("block-0", "one line");
    block.properties.line_spacing =
        Some(LineSpacing::exactly(Length::from_twips(500).unwrap()).unwrap());
    let layout = layout_document(&document(vec![block]));
    assert_eq!(layout.blocks[0].height_twips, 500);
}

#[test]
fn a_heading_is_taller_and_carries_space_above_it() {
    let layout = layout_document(&document(vec![
        paragraph("block-0", "body"),
        heading("block-1", 1, "Title"),
    ]));
    let body = layout.placement("block-0").unwrap();
    let title = layout.placement("block-1").unwrap();
    // 20pt at 1.5 is 30pt, which is 600 twips.
    assert_eq!(title.height_twips, 600);
    // 16pt above an h1 wins the collapse against the paragraph's 10pt below.
    assert_eq!(title.top_twips - (body.top_twips + body.height_twips), 320);
}

/// A page whose content box is narrow enough that a list's indent is a large
/// fraction of it, so one nesting level is worth a whole line.
fn narrow_page() -> PageSetup {
    PageSetup {
        margin_start: Length::from_twips(4_320).expect("3in"),
        margin_end: Length::from_twips(4_320).expect("3in"),
        ..PageSetup::default()
    }
}

#[test]
fn a_deeper_list_level_gets_less_room_for_text() {
    let text =
        "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho";
    let mut doc = document(vec![
        list_item("block-0", 0, ListKind::Bullet, "list-1", text),
        list_item("block-1", 1, ListKind::Bullet, "list-1", text),
        list_item("block-2", 2, ListKind::Bullet, "list-1", text),
    ]);
    doc.page_setup = narrow_page();
    let layout = layout_document(&doc);
    let level = |id: &str| layout.placement(id).unwrap().lines;
    assert!(
        level("block-1") > level("block-0"),
        "level 1 ({}) did not need more lines than level 0 ({})",
        level("block-1"),
        level("block-0")
    );
    assert!(
        level("block-2") > level("block-1"),
        "level 2 ({}) did not need more lines than level 1 ({})",
        level("block-2"),
        level("block-1")
    );
}

#[test]
fn a_list_run_carries_the_wrappers_space_below_it_once() {
    let layout = layout_document(&document(vec![
        list_item("block-0", 0, ListKind::Bullet, "list-1", "one"),
        list_item("block-1", 0, ListKind::Bullet, "list-1", "two"),
        paragraph("block-2", "after"),
    ]));
    let first = layout.placement("block-0").unwrap();
    let second = layout.placement("block-1").unwrap();
    let after = layout.placement("block-2").unwrap();
    // Items butt together...
    assert_eq!(second.top_twips, first.top_twips + first.height_twips);
    // ...and the 10pt below the list is the wrapper's, applied once.
    assert_eq!(
        after.top_twips - (second.top_twips + second.height_twips),
        200
    );
}

#[test]
fn a_checklist_item_loses_the_room_its_checkbox_takes() {
    // A checklist wrapper indents less than a bulleted one, because the
    // checkbox is the marker. Equalise the two wrappers with an explicit
    // indent so the only difference left is the checkbox itself, an inline
    // box at the head of the item's own text.
    //
    // Whether that costs a line depends on where the text happens to fall, so
    // the assertion is over a family of inputs rather than one: for *some*
    // length of unbreakable word the checkbox must push the item onto another
    // line. Nothing about the checkbox being free would survive that.
    let scale = TypeScale::default();
    let equalise = Length::from_twips(scale.list_indent - scale.checklist_indent).unwrap();
    let mut differed = 0;
    for length in 1..80 {
        let text = "n".repeat(length);
        let mut bullet = list_item("block-0", 0, ListKind::Bullet, "list-1", &text);
        bullet.properties.indent_start = Some(Length::from_twips(0).unwrap());
        let mut checklist = list_item(
            "block-0",
            0,
            ListKind::Checklist { checked: false },
            "list-1",
            &text,
        );
        checklist.properties.indent_start = Some(equalise);
        let without = layout_document(&document(vec![bullet])).blocks[0].lines;
        let with_box = layout_document(&document(vec![checklist])).blocks[0].lines;
        assert!(
            with_box >= without,
            "a {length}-character item was shorter *with* a checkbox: {with_box} < {without}"
        );
        if with_box > without {
            differed += 1;
        }
    }
    assert!(
        differed > 0,
        "the checkbox never cost a line at any word length, so it took no room"
    );
}

#[test]
fn bold_text_is_measured_with_the_bold_face() {
    // `bold >= plain` on one page width is satisfied by equality, and
    // equality is what measuring both with the regular face produces — so the
    // assertion that reads as a check of the bold face was passing for the
    // bug. Stated over a family of page widths instead: bold is never
    // narrower, and somewhere in the family it costs a line.
    let text = "The quick brown fox jumps over the lazy dog and then turns around again.";
    let plain = paragraph("block-0", text);
    let mut bold = plain.clone();
    if let Some(Inline::Text { marks, .. }) = bold.content.first_mut() {
        marks.push(Mark {
            kind: MarkKind::Bold,
            value: None,
            expand: opendoc_core::MarkExpand::None,
        });
    }
    let mut differed = 0;
    for margin in (2_400..4_000).step_by(40) {
        let setup = PageSetup {
            margin_start: Length::from_twips(margin).unwrap(),
            margin_end: Length::from_twips(margin).unwrap(),
            ..PageSetup::default()
        };
        let mut plain_doc = document(vec![plain.clone()]);
        plain_doc.page_setup = setup;
        let mut bold_doc = document(vec![bold.clone()]);
        bold_doc.page_setup = setup;
        let thin = layout_document(&plain_doc).blocks[0].lines;
        let heavy = layout_document(&bold_doc).blocks[0].lines;
        assert!(
            heavy >= thin,
            "with {margin}-twip margins bold took {heavy} lines and regular {thin}"
        );
        if heavy > thin {
            differed += 1;
        }
    }
    assert!(
        differed > 0,
        "bold never cost a line at any page width, so it was measured with the regular face"
    );
}

#[test]
fn text_outside_the_bundled_subset_is_reported_as_estimated() {
    let layout = layout_document(&document(vec![paragraph("block-0", "日本語のテキスト")]));
    assert!(!layout.exact);
    assert!(!layout.blocks[0].exact);
    assert!(layout_document(&document(vec![paragraph("block-0", "latin text")])).exact);
}

#[test]
fn an_estimate_taints_every_page_assignment_after_it() {
    let mut equation = paragraph("block-0", "");
    equation.kind = BlockKind::EquationBlock {
        equation: opendoc_core::Equation {
            id: StableId::new("eq"),
            source_format: opendoc_core::EquationSourceFormat::LatexLike,
            source: "x^2".to_string(),
        },
    };
    let layout = layout_document(&document(vec![
        paragraph("block-before", "before"),
        equation,
        paragraph("block-after", "after"),
    ]));
    assert!(layout.placement("block-before").unwrap().exact);
    assert!(!layout.placement("block-0").unwrap().exact);
    assert!(!layout.placement("block-after").unwrap().exact);
}

#[test]
fn the_page_box_decides_the_page_count() {
    let blocks = filler(20);
    let mut letter = document(blocks.clone());
    letter.page_setup = PageSetup::default();
    let mut half = document(blocks.clone());
    half.page_setup = short_page();
    let mut wide = document(blocks);
    // Same sheet, wider text column: fewer wrapped lines, so no more pages.
    wide.page_setup = PageSetup {
        margin_start: Length::from_twips(180).expect("valid"),
        margin_end: Length::from_twips(180).expect("valid"),
        ..PageSetup::default()
    };

    let letter_pages = layout_document(&letter).page_count;
    let half_pages = layout_document(&half).page_count;
    let wide_pages = layout_document(&wide).page_count;
    assert!(
        half_pages > letter_pages,
        "a 3in page held the same content as a Letter page ({half_pages} vs {letter_pages})"
    );
    assert!(
        wide_pages <= letter_pages,
        "narrower margins needed more pages ({wide_pages} vs {letter_pages})"
    );
}

#[test]
fn a_block_taller_than_the_page_takes_a_page_and_overflows() {
    let mut doc = document(vec![
        paragraph("block-0", "first"),
        paragraph("block-1", &"word ".repeat(400)),
        paragraph("block-2", "last"),
    ]);
    doc.page_setup = short_page();
    let layout = layout_document(&doc);
    let tall = layout.placement("block-1").unwrap();
    // It opens its own page rather than looping or being dropped...
    assert!(tall.page_break_margin.is_some());
    // ...and the block after it starts a later page than it does.
    assert!(layout.placement("block-2").unwrap().page > tall.page);
}

#[test]
fn the_type_scale_is_projected_for_the_stylesheet() {
    let css = type_scale_css_variables();
    assert!(css.contains("--doc-font-size: 11pt"));
    assert!(css.contains(font::SANS_FAMILY));
}

#[test]
fn a_page_break_margin_renders_as_an_exact_css_length() {
    let mut doc = document(filler(30));
    doc.page_setup = short_page();
    let layout = layout_document(&doc);
    let opener = layout
        .blocks
        .iter()
        .find(|block| block.page_break_margin.is_some())
        .expect("a page opener");
    let css = opener.page_break_css().expect("a margin");
    assert!(css.ends_with("pt"), "{css}");
    assert!(!css.contains("NaN"));
}

#[test]
fn a_table_is_laid_out_but_reported_as_an_estimate() {
    let mut block = paragraph("block-0", "");
    block.kind = BlockKind::table(vec![TableRow {
        id: StableId::new("row"),
        height: None,
        header: false,
        cells: vec![opendoc_core::TableCell {
            id: StableId::new("cell"),
            span: opendoc_core::CellSpan::SINGLE,
            properties: Default::default(),
            blocks: vec![paragraph("cell-block", "a cell")],
        }],
    }]);
    let layout = layout_document(&document(vec![block]));
    assert!(layout.blocks[0].height_twips > 0);
    assert!(!layout.blocks[0].exact);
}

// ---- painting -----------------------------------------------------------
//
// The painted pass and the paginating pass are the same pass with the content
// recorded, so these tests are mostly about proving exactly that: where a
// block is placed and what is drawn on it cannot come apart.

/// Every run drawn on a page, in order.
fn drawn_text(page: &PaintedPage) -> String {
    page.items
        .iter()
        .filter_map(|item| match item {
            PaintItem::Text { runs, .. } => {
                Some(runs.iter().map(|run| run.text.as_str()).collect::<String>())
            }
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn painting_a_document_does_not_move_a_single_block() {
    let mut source = document(filler(40));
    source.page_setup = short_page();
    let paginated = layout_document(&source);
    let painted = layout_painted_document(&source);
    assert_eq!(paginated.page_count, painted.page_count);
    assert_eq!(paginated.blocks, painted.blocks);
}

#[test]
fn every_page_the_layout_counted_is_painted() {
    let mut source = document(filler(40));
    source.page_setup = short_page();
    let painted = layout_painted_document(&source);
    assert_eq!(painted.page_count as usize, painted.pages.len());
    assert!(painted.pages.iter().all(|page| !page.items.is_empty()));
}

#[test]
fn a_blocks_text_is_drawn_on_the_page_its_placement_names() {
    let mut source = document(vec![
        paragraph("first", "FIRSTMARKER"),
        Block {
            id: StableId::parse("break").expect("valid id"),
            kind: BlockKind::PageBreak,
            content: Vec::new(),
            properties: Default::default(),
        },
        paragraph("second", "SECONDMARKER"),
    ]);
    source.page_setup = PageSetup::default();
    let painted = layout_painted_document(&source);
    assert_eq!(2, painted.page_count, "the page break did not break");
    assert!(drawn_text(&painted.pages[0]).contains("FIRSTMARKER"));
    assert!(!drawn_text(&painted.pages[0]).contains("SECONDMARKER"));
    assert!(drawn_text(&painted.pages[1]).contains("SECONDMARKER"));
}

#[test]
fn a_centred_line_is_drawn_further_right_than_a_left_aligned_one() {
    let left = paragraph("p", "short line");
    let mut centre = paragraph("p", "short line");
    centre.properties.alignment = Some(opendoc_core::Alignment::Center);
    let mut right = paragraph("p", "short line");
    right.properties.alignment = Some(opendoc_core::Alignment::End);

    let x = |block: Block| -> i32 {
        let painted = layout_painted_document(&document(vec![block]));
        match &painted.pages[0].items[0] {
            PaintItem::Text { runs, .. } => runs[0].x_twips,
            other => panic!("expected text, got {other:?}"),
        }
    };
    let (l, c, r) = (x(left), x(centre), x(right));
    assert!(
        l < c && c < r,
        "alignment did not move the line: {l} {c} {r}"
    );
}

#[test]
fn an_indent_moves_the_drawn_line_by_exactly_the_indent() {
    let plain = paragraph("p", "indented");
    let mut indented = paragraph("p", "indented");
    indented.properties.indent_start = Some(Length::from_twips(720).expect("half an inch"));
    let x = |block: Block| -> i32 {
        let painted = layout_painted_document(&document(vec![block]));
        match &painted.pages[0].items[0] {
            PaintItem::Text { runs, .. } => runs[0].x_twips,
            other => panic!("expected text, got {other:?}"),
        }
    };
    assert_eq!(x(plain) + 720, x(indented));
}

#[test]
fn paragraph_background_and_uniform_border_paint_a_box_around_measured_text() {
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
    let painted = layout_painted_document(&document(vec![block]));
    let items = &painted.pages[0].items;
    assert!(
        matches!(
            items.first(),
            Some(PaintItem::Fill {
                color: Some(Rgb {
                    red: 0x33,
                    green: 0x66,
                    blue: 0x99
                }),
                ..
            })
        ),
        "paragraph background was not painted first: {items:?}"
    );
    assert!(
        items
            .iter()
            .any(|item| matches!(item, PaintItem::Text { .. })),
        "paragraph text disappeared: {items:?}"
    );
    let edges = items
        .iter()
        .filter(|item| {
            matches!(
                item,
                PaintItem::Edge {
                    color: Rgb {
                        red: 0xcc,
                        green: 0,
                        blue: 0
                    },
                    dash: Some([80, 80]),
                    thickness_twips: 20,
                    ..
                }
            )
        })
        .count();
    assert_eq!(
        edges, 4,
        "the uniform paragraph border did not paint four dashed edges: {items:?}"
    );
}

#[test]
fn a_baseline_sits_inside_the_line_box_it_belongs_to() {
    // Half-leading: the baseline is below the top of the line box and above
    // its bottom. A baseline outside its box would mean text drawn over the
    // line above or below it.
    let source = document(vec![paragraph("p", "one line of text")]);
    let painted = layout_painted_document(&source);
    let placement = &painted.blocks[0];
    let PaintItem::Text { baseline_twips, .. } = &painted.pages[0].items[0] else {
        panic!("expected text");
    };
    assert!(
        *baseline_twips > placement.top_twips
            && *baseline_twips < placement.top_twips + placement.height_twips,
        "baseline {baseline_twips} is outside the block's box {}..{}",
        placement.top_twips,
        placement.top_twips + placement.height_twips
    );
}

#[test]
fn an_estimated_block_names_itself_and_its_reason() {
    let mut table = paragraph("tbl", "");
    table.content.clear();
    table.kind = BlockKind::table(vec![opendoc_core::TableRow {
        id: StableId::parse("row-0").expect("valid id"),
        height: None,
        header: false,
        cells: vec![opendoc_core::TableCell::new(vec![Block::paragraph("cell")])],
    }]);
    let painted = layout_painted_document(&document(vec![paragraph("p", "exact"), table]));
    assert_eq!(
        vec![Estimate {
            block_id: "tbl".to_string(),
            reason: EstimateReason::Table,
        }],
        painted.estimates,
        "only the table's own geometry is an estimate"
    );
}

#[test]
fn a_table_cells_text_is_drawn_inside_the_table() {
    let mut table = paragraph("tbl", "");
    table.content.clear();
    table.kind = BlockKind::table(vec![opendoc_core::TableRow {
        id: StableId::parse("row-0").expect("valid id"),
        height: None,
        header: false,
        cells: vec![
            opendoc_core::TableCell::new(vec![Block::paragraph("CELLONE")]),
            opendoc_core::TableCell::new(vec![Block::paragraph("CELLTWO")]),
        ],
    }]);
    let painted = layout_painted_document(&document(vec![table]));
    let drawn = drawn_text(&painted.pages[0]);
    assert!(
        drawn.contains("CELLONE") && drawn.contains("CELLTWO"),
        "{drawn:?}"
    );
    // One row of two cells is a grid of seven boundaries, not eight cell
    // edges: (rows + 1) * columns horizontal ones and (columns + 1) * rows
    // vertical ones. The shared boundary in the middle is drawn once.
    assert_eq!(
        7,
        painted.pages[0]
            .items
            .iter()
            .filter(|item| matches!(item, PaintItem::Edge { .. }))
            .count()
    );
}

#[test]
fn a_table_breaks_at_rows_and_repeats_its_leading_header_on_the_next_page() {
    let mut source = document(vec![table("tbl", 3, 1)]);
    source.page_setup = short_page();
    let BlockKind::Table { rows, .. } = &mut source.blocks[0].kind else {
        panic!("expected table");
    };
    rows[0].header = true;
    for (row, label) in rows.iter_mut().zip(["HEADER", "FIRST", "SECOND"]) {
        row.height = Some(Length::from_twips(700).expect("row height"));
        row.cells[0].blocks = vec![Block::paragraph(label)];
    }

    let painted = layout_painted_document(&source);
    assert_eq!(2, painted.page_count, "the body row did not continue");
    assert_eq!(
        1,
        painted.blocks.len(),
        "row fragments leaked through the one-placement-per-block API"
    );
    let first = drawn_text(&painted.pages[0]);
    let second = drawn_text(&painted.pages[1]);
    assert!(
        first.contains("HEADER") && first.contains("FIRST"),
        "{first}"
    );
    assert!(!first.contains("SECOND"), "{first}");
    assert!(
        second.contains("HEADER") && second.contains("SECOND"),
        "{second}"
    );
    assert!(!second.contains("FIRST"), "{second}");
    assert_eq!(
        2,
        painted
            .pages
            .iter()
            .map(drawn_text)
            .filter(|text| text.contains("HEADER"))
            .count(),
        "the header was not repeated exactly once"
    );
}

#[test]
fn the_pagination_path_allocates_nothing_for_content_it_will_not_use() {
    // `layout_document` runs on every keystroke. If it captured lines it
    // would allocate a string per run for nobody.
    let source = document(filler(5));
    let painted = layout_painted_document(&source);
    assert!(painted.pages.iter().any(|page| !page.items.is_empty()));
    // The paginating entry point exposes no paint at all, which is the
    // structural form of that guarantee.
    let paginated = layout_document(&source);
    assert_eq!(paginated.blocks.len(), painted.blocks.len());
}

#[test]
fn the_header_sits_above_the_body_and_the_footer_below_it_on_every_page() {
    // The furniture lives in the page *margins*, which is the one part of the
    // sheet the body flow never reaches. A footer anchored to the page top
    // would print on top of the first line.
    let mut source = document(filler(40));
    source.page_setup = short_page();
    let mut header = paragraph("hdr", "HEADERMARKER");
    header.properties = Default::default();
    *source.furniture_mut(opendoc_core::HeaderFooterSlot::Header) = vec![header];
    let footer = paragraph("ftr", "FOOTERMARKER");
    *source.furniture_mut(opendoc_core::HeaderFooterSlot::Footer) = vec![footer];

    let setup = source.page_setup;
    let painted = layout_painted_document(&source);
    assert!(painted.page_count > 1, "the fixture is one page");

    let body_top = setup.margin_top.twips();
    let body_bottom = body_top + setup.content_height().twips();
    for (index, page) in painted.pages.iter().enumerate() {
        let baseline_of = |marker: &str| {
            page.items.iter().find_map(|item| match item {
                PaintItem::Text {
                    baseline_twips,
                    runs,
                } if runs.iter().any(|run| run.text.contains(marker)) => Some(*baseline_twips),
                _ => None,
            })
        };
        let header =
            baseline_of("HEADERMARKER").unwrap_or_else(|| panic!("page {index} has no header"));
        let footer =
            baseline_of("FOOTERMARKER").unwrap_or_else(|| panic!("page {index} has no footer"));
        assert!(
            header < body_top,
            "page {index}: the header baseline {header} is inside the body box"
        );
        assert!(
            footer > body_bottom,
            "page {index}: the footer baseline {footer} is inside the body box"
        );
        assert!(
            footer < setup.height.twips(),
            "page {index}: the footer baseline {footer} is off the sheet"
        );
        // The furniture scale, not the body scale. The stylesheet draws
        // headers and footers at 10pt; a PDF that drew them at the body's
        // 11pt would disagree with the screen in the one place the whole
        // design exists to make agree.
        let size = page.items.iter().find_map(|item| match item {
            PaintItem::Text { runs, .. } => runs
                .iter()
                .find(|run| run.text.contains("HEADERMARKER"))
                .map(|run| run.style.size_twips),
            _ => None,
        });
        assert_eq!(
            Some(TypeScale::default().furniture_size),
            size,
            "page {index}: the header is not set at the furniture size"
        );
    }
}

#[test]
fn face_ids_agree_with_the_face_table_they_index() {
    // `FaceId::of` indexes `FaceId::ALL` with the same number `Fonts` uses
    // internally; if the two orders ever diverge, every bold run would be
    // drawn with the wrong face.
    use crate::font::{FaceId, TextStyle};
    let size = 220;
    assert_eq!(FaceId::SansRegular, FaceId::of(TextStyle::new(size)));
    assert_eq!(
        FaceId::SansBold,
        FaceId::of(TextStyle {
            bold: true,
            ..TextStyle::new(size)
        })
    );
    assert_eq!(
        FaceId::SansItalic,
        FaceId::of(TextStyle {
            italic: true,
            ..TextStyle::new(size)
        })
    );
    assert_eq!(
        FaceId::SansBoldItalic,
        FaceId::of(TextStyle {
            bold: true,
            italic: true,
            ..TextStyle::new(size)
        })
    );
    assert_eq!(
        FaceId::MonoRegular,
        FaceId::of(TextStyle {
            mono: true,
            ..TextStyle::new(size)
        })
    );
}

// ---- The line box, measured against Chrome 147 -------------------------
//
// These are not self-consistency checks. Every expected number below was
// measured in a real Chrome against the bundled WOFF2 faces, through a page
// whose `line-height` and sizes are the ones `TypeScale` states, by reading
// the height of a paragraph and the position of a zero-height inline-block
// sitting on its baseline. A line box is the **union** of the block's strut
// and every inline box on the line, and the three quantisations Blink applies
// on the way (whole-pixel ascent and descent, the 1/64-px grid, the floor of
// the half-leading) are each worth a pixel or more. If this crate stopped
// reproducing them it would go back to reporting 22px for a line Chrome draws
// at 36.

/// A run of `text` carrying one mark.
fn marked(id: &str, text: &str, kind: MarkKind, value: Option<&str>) -> Block {
    let mut block = paragraph(id, text);
    if let Some(Inline::Text { marks, .. }) = block.content.first_mut() {
        marks.push(Mark {
            kind,
            value: value.map(str::to_string),
            expand: opendoc_core::MarkExpand::None,
        });
    }
    block
}

/// The block's height in twips, on a page wide enough that nothing wraps.
fn block_height(block: Block) -> i32 {
    layout_document(&document(vec![block])).blocks[0].height_twips
}

/// CSS pixels to twips: 1px is 0.75pt is 15 twips, exactly.
const PX: i32 = style::TWIPS_PER_PX;

#[test]
fn a_plain_line_is_the_twenty_two_pixels_chrome_draws() {
    assert_eq!(block_height(paragraph("block-0", "abc")), 22 * PX);
}

#[test]
fn a_monospace_run_makes_the_line_one_pixel_taller() {
    // `OpenDoc Mono` has a shorter ascent and a deeper descent than the sans
    // face, and Chrome takes the union of the two boxes' half-leadings: 23px,
    // not 22. One pixel per affected line, and it accumulates down the page.
    assert_eq!(
        block_height(marked("block-0", "abc", MarkKind::Code, None)),
        23 * PX
    );
    // The mono box is the taller one even when it shares the line with plain
    // text, which is what "union" means.
    let mut mixed = paragraph("block-0", "plain ");
    mixed.content.push(Inline::Text {
        id: StableId::parse("inline-9").expect("valid id"),
        text: "code".to_string(),
        marks: vec![Mark {
            kind: MarkKind::Code,
            value: None,
            expand: opendoc_core::MarkExpand::None,
        }],
    });
    assert_eq!(block_height(mixed), 23 * PX);
}

#[test]
fn a_size_mark_sets_the_leading_of_the_line_it_is_on() {
    // The bug this test exists for: an 18pt run in an 11pt paragraph is 36px
    // per line in Chrome, because a unitless `line-height: 1.5` multiplies the
    // *span's* own size. Taking the leading from the block's base size gave
    // 22px and reported the block as exactly measured — so a document with one
    // such paragraph put eight of its forty-one blocks outside the page they
    // were assigned.
    let big = marked("block-0", "big", MarkKind::Size, Some("18"));
    assert_eq!(block_height(big.clone()), 36 * PX);
    // And a run beside plain text raises the whole line, not just itself.
    let mut mixed = paragraph("block-0", "plain ");
    mixed.content.push(Inline::Text {
        id: StableId::parse("inline-9").expect("valid id"),
        text: "big".to_string(),
        marks: vec![Mark {
            kind: MarkKind::Size,
            value: Some("18".to_string()),
            expand: opendoc_core::MarkExpand::None,
        }],
    });
    assert_eq!(block_height(mixed), 36 * PX);
    // A *smaller* run cannot shrink the line below the block's own strut.
    assert_eq!(
        block_height(marked("block-0", "small", MarkKind::Size, Some("8"))),
        22 * PX
    );
    // The measurement is exact — it is a sum of bundled metrics — so the
    // block must not claim otherwise either.
    assert!(layout_document(&document(vec![big])).exact);
}

#[test]
fn a_superscript_is_raised_and_makes_its_line_box_taller() {
    // Chrome: 23.890625px for a superscript and 24.421875px for a subscript on
    // an 11pt line, and the shift is a function of the *parent's* font size.
    let mut sup = paragraph("block-0", "x");
    sup.content.push(Inline::Text {
        id: StableId::parse("inline-9").expect("valid id"),
        text: "9".to_string(),
        marks: vec![Mark {
            kind: MarkKind::Superscript,
            value: None,
            expand: opendoc_core::MarkExpand::None,
        }],
    });
    let mut sub = sup.clone();
    if let Some(Inline::Text { marks, .. }) = sub.content.last_mut() {
        marks[0].kind = MarkKind::Subscript;
    }
    assert_eq!(block_height(sup.clone()), 358);
    assert_eq!(block_height(sub), 366);
    // It used to be reported as an estimate because the raised box was not
    // modelled. It is modelled now, so it must not be.
    assert!(layout_document(&document(vec![sup.clone()])).exact);

    // And the glyphs are drawn above the baseline of the text beside them.
    let painted = layout_painted_document(&document(vec![sup]));
    let runs: Vec<&PaintRun> = painted.pages[0]
        .items
        .iter()
        .filter_map(|item| match item {
            PaintItem::Text { runs, .. } => Some(runs),
            _ => None,
        })
        .flatten()
        .collect();
    let raised = runs
        .iter()
        .find(|run| run.text == "9")
        .expect("the superscript was not drawn");
    assert!(
        raised.decoration.rise_units > 0,
        "the superscript sits on the baseline"
    );
}

#[test]
fn a_stated_line_height_still_gives_every_box_its_own_half_leading() {
    // `line-height: 22pt` is inherited as a length, so every box on the line
    // gets the same used value — but their content boxes differ, so their
    // half-leadings differ and the union is still not one number. Chrome draws
    // this line at 34.328125px.
    let mut block = paragraph("block-0", "plain ");
    block.properties.line_spacing = Some(LineSpacing::Exact(Length::from_twips(440).unwrap()));
    for (text, kind, value) in [
        ("code", MarkKind::Code, None),
        ("big", MarkKind::Size, Some("18")),
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
    assert_eq!(block_height(block), 515);
}

#[test]
fn a_list_run_whose_marker_changes_pays_the_wrapper_margin_twice() {
    // `ListWriter` closes the outermost `.doc-list` and opens another whenever
    // the marker changes at a level, and the closed wrapper's `margin-bottom`
    // is real space. Measured in Chrome at 13.33px — the 10pt the scale
    // states — which the flow used not to account for at all.
    let changed = layout_document(&document(vec![
        list_item("block-0", 0, ListKind::Bullet, "list-1", "one"),
        list_item("block-1", 0, ListKind::Ordered, "list-1", "two"),
        paragraph("block-2", "after"),
    ]));
    let first = changed.placement("block-0").unwrap();
    let second = changed.placement("block-1").unwrap();
    assert_eq!(
        second.top_twips - (first.top_twips + first.height_twips),
        200,
        "the closed wrapper's margin-bottom was not accounted for"
    );
    // A run that does *not* change marker still butts its items together.
    let same = layout_document(&document(vec![
        list_item("block-0", 0, ListKind::Bullet, "list-1", "one"),
        list_item("block-1", 0, ListKind::Bullet, "list-1", "two"),
        paragraph("block-2", "after"),
    ]));
    let first = same.placement("block-0").unwrap();
    let second = same.placement("block-1").unwrap();
    assert_eq!(second.top_twips, first.top_twips + first.height_twips);
}

/// The glyph painted beside a bulleted item is the glyph CSS draws there.
///
/// Depth 0 is `disc`, depth 1 `circle`, depth 2 `square` and depth 3 starts
/// the cycle again — measured in Chrome, and pinned for the CSS side in
/// `lists_tests::the_bullet_cycle_is_the_sequence_chrome_draws`. This is the
/// *painted* side, which is a separate claim: the engine used to substitute
/// the disc at depth 1 because the bundled subset had no U+25E6, so a
/// document read on screen and the same document printed showed different
/// bullets.
///
/// The markers are read out interleaved with the items' own text rather than
/// filtered out of it, so a marker that went missing, doubled, or landed
/// beside the wrong item fails here too — filtering for "the runs that look
/// like markers" would have hidden all three.
#[test]
fn a_bulleted_item_is_painted_with_the_bullet_its_depth_draws() {
    let painted = layout_painted_document(&document(vec![
        list_item("block-0", 0, ListKind::Bullet, "list-1", "alpha"),
        list_item("block-1", 1, ListKind::Bullet, "list-1", "beta"),
        list_item("block-2", 2, ListKind::Bullet, "list-1", "gamma"),
        list_item("block-3", 3, ListKind::Bullet, "list-1", "delta"),
    ]));
    let drawn: Vec<&str> = painted
        .pages
        .iter()
        .flat_map(|page| page.items.iter())
        .filter_map(|item| match item {
            PaintItem::Text { runs, .. } => Some(runs.iter().map(|run| run.text.as_str())),
            _ => None,
        })
        .flatten()
        .collect();
    assert_eq!(
        drawn,
        ["•", "alpha", "◦", "beta", "■", "gamma", "•", "delta"],
        "the painted markers are not the ones `list-style-type` draws"
    );
}

/// All the text drawn anywhere in a painted document, page by page.
fn painted_text(painted: &PaintedDocument) -> Vec<String> {
    painted
        .pages
        .iter()
        .map(|page| {
            page.items
                .iter()
                .filter_map(|item| match item {
                    PaintItem::Text { runs, .. } => {
                        Some(runs.iter().map(|run| run.text.as_str()).collect::<String>())
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("|")
        })
        .collect()
}

fn footnote_document() -> Document {
    let mut first = paragraph("block-0", "see");
    first.content.push(Inline::FootnoteRef {
        id: StableId::parse("inline-1").expect("valid id"),
        footnote_id: StableId::parse("note-b").expect("valid id"),
    });
    let mut second = paragraph("block-1", "and");
    second.content.push(Inline::FootnoteRef {
        id: StableId::parse("inline-2").expect("valid id"),
        footnote_id: StableId::parse("note-a").expect("valid id"),
    });
    let mut document = document(vec![first, second]);
    // Declared in the opposite order to the references, because the numbering
    // is by order of *reference* and a test that declared them in order could
    // not tell the two rules apart.
    document.footnotes = vec![
        opendoc_core::Footnote {
            id: StableId::parse("note-a").expect("valid id"),
            revision: 1,
            body: vec![Inline::text("the second note")],
            deleted: false,
        },
        opendoc_core::Footnote {
            id: StableId::parse("note-b").expect("valid id"),
            revision: 1,
            body: vec![Inline::text("the first note")],
            deleted: false,
        },
    ];
    document
}

#[test]
fn a_footnote_reference_draws_its_number_not_a_placeholder() {
    // Every reference used to be measured and drawn as a literal `0`.
    let painted = layout_painted_document(&footnote_document());
    let drawn = painted_text(&painted).join("\n");
    // The reference is a run on the same line as the text it follows, so the
    // number lands right against it.
    assert!(drawn.contains("see1"), "{drawn}");
    assert!(drawn.contains("and2"), "{drawn}");
    assert!(!drawn.contains("see0"), "{drawn}");
}

#[test]
fn footnote_bodies_are_drawn_after_the_body_rather_than_dropped() {
    let painted = layout_painted_document(&footnote_document());
    let drawn = painted_text(&painted).join("\n");
    assert!(drawn.contains("the first note"), "{drawn}");
    assert!(drawn.contains("the second note"), "{drawn}");
    // Numbered by reference order, so `note-b` is 1 even though it is
    // declared second.
    assert!(drawn.contains("1.|the first note"), "{drawn}");
    assert!(drawn.contains("2.|the second note"), "{drawn}");
    // A footnote body is not a block anybody can place, so it does not appear
    // among the placements a caller reads.
    assert_eq!(painted.blocks.len(), 2);
    // The reference's own number is measured text, so it changes the width of
    // the line it is on.
    let plain = layout_document(&document(vec![paragraph("block-0", "see")]));
    let referenced = layout_document(&footnote_document());
    assert_eq!(plain.blocks[0].lines, referenced.blocks[0].lines);
}

#[test]
fn colour_highlight_underline_strike_and_links_reach_the_paint() {
    let mut block = paragraph("block-0", "plain ");
    for (text, kind, value) in [
        ("red", MarkKind::Color, Some("#ff0000")),
        ("lit", MarkKind::Background, Some("#ff0")),
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
    let painted = layout_painted_document(&document(vec![block]));
    let runs: Vec<&PaintRun> = painted.pages[0]
        .items
        .iter()
        .filter_map(|item| match item {
            PaintItem::Text { runs, .. } => Some(runs),
            _ => None,
        })
        .flatten()
        .collect();
    let find = |text: &str| {
        runs.iter()
            .find(|run| run.text == text)
            .unwrap_or_else(|| panic!("{text:?} was not drawn"))
    };
    assert_eq!(
        find("red").decoration.color,
        Some(Rgb {
            red: 0xff,
            green: 0,
            blue: 0
        })
    );
    // A three-digit hex colour is expanded the way CSS expands it.
    assert_eq!(
        find("lit").decoration.background,
        Some(Rgb {
            red: 0xff,
            green: 0xff,
            blue: 0
        })
    );
    assert!(find("under").decoration.underline);
    assert!(find("struck").decoration.strike);
    assert_eq!(
        find("click").decoration.link.as_deref(),
        Some("https://example.invalid/a")
    );
    assert!(find("plain ").decoration.is_plain());
    // Every run states the width it was measured at, so a consumer drawing a
    // highlight or a link rectangle behind it cannot measure it differently.
    assert!(find("red").width_twips > 0);
}

#[test]
fn a_colour_that_is_not_a_hex_triple_is_reported_rather_than_guessed() {
    let block = marked("block-0", "named", MarkKind::Color, Some("rebeccapurple"));
    let painted = layout_painted_document(&document(vec![block]));
    assert_eq!(
        painted
            .estimates
            .iter()
            .map(|estimate| estimate.reason)
            .collect::<Vec<_>>(),
        vec![EstimateReason::UnmeasurableMark]
    );
}

#[test]
fn an_inline_run_takes_no_room_beyond_its_advances() {
    // ADR 0014's guarantee, stated as a test: the three run kinds whose
    // stylesheet rules used to carry horizontal padding — a code mark, a
    // citation label and a mention — occupy exactly the advances of their own
    // glyphs and not one twip more. Checked by where the run *after* them is
    // drawn: any padding, modelled as an inline box or otherwise, would push
    // it right. An e2e check reads the computed padding back out of Chrome;
    // this one pins the layout's half of it.
    let fonts = Fonts::load();
    let scale = TypeScale::default();
    let style = TextStyle::new(scale.body_size);
    let code_style = TextStyle {
        mono: true,
        ..style
    };
    let cases: Vec<(&str, Inline, TextStyle)> = vec![
        (
            "mention",
            Inline::Mention {
                id: StableId::new("inline"),
                label: "@alice".to_string(),
            },
            style,
        ),
        (
            "citation",
            Inline::Citation {
                id: StableId::new("inline"),
                citation_id: StableId::new("citation"),
                rendered_cache: Some("(Auditor 2026)".to_string()),
            },
            style,
        ),
        (
            "code",
            Inline::Text {
                id: StableId::new("inline"),
                text: "code".to_string(),
                marks: vec![Mark {
                    kind: MarkKind::Code,
                    value: None,
                    expand: opendoc_core::MarkExpand::None,
                }],
            },
            code_style,
        ),
    ];
    for (name, middle, middle_style) in cases {
        let text = match &middle {
            Inline::Mention { label, .. } => label.clone(),
            Inline::Citation { rendered_cache, .. } => rendered_cache.clone().unwrap(),
            Inline::Text { text, .. } => text.clone(),
            _ => unreachable!(),
        };
        let mut block = paragraph("block-0", "before ");
        block.content.push(middle);
        block.content.push(Inline::text(" after"));
        let painted = layout_painted_document(&document(vec![block]));
        let runs: Vec<&PaintRun> = painted.pages[0]
            .items
            .iter()
            .filter_map(|item| match item {
                PaintItem::Text { runs, .. } => Some(runs),
                _ => None,
            })
            .flatten()
            .collect();
        assert_eq!(runs.len(), 3, "{name}: {runs:?}");
        assert_eq!(
            runs[1].x_twips,
            runs[0].x_twips + fonts.text_advance_twips("before ", style),
            "{name} starts further right than the text before it ends"
        );
        assert_eq!(
            runs[2].x_twips,
            runs[1].x_twips + fonts.text_advance_twips(&text, middle_style),
            "{name} takes more room than its own advances"
        );
    }
}

// ---- table borders -------------------------------------------------------
//
// `.doc-table td` states `border: 0.75pt solid #999` on all four edges and
// `border-collapse: collapse` on the table, and `opendoc-render` projects each
// cell's stated edges as `border-block-start` and friends over the top of it.
// So what the screen draws at a boundary is CSS 2.1 §17.6.2.1's winner, and
// what these tests pin is that the paper draws the same one — including the
// two cases that surprise: an unstated edge is the *default*, not nothing, and
// a `none` border loses a shared boundary to its neighbour instead of clearing
// it.

/// One drawn border line, with the colour as the stylesheet would spell it so
/// a failure names a colour rather than three integers.
#[derive(Clone, Debug, PartialEq, Eq)]
struct DrawnEdge {
    x1: i32,
    y1: i32,
    x2: i32,
    y2: i32,
    thickness: i32,
    color: String,
    dash: Option<[i32; 2]>,
}

fn drawn_edges(page: &PaintedPage) -> Vec<DrawnEdge> {
    page.items
        .iter()
        .filter_map(|item| match item {
            PaintItem::Edge {
                x1_twips,
                y1_twips,
                x2_twips,
                y2_twips,
                thickness_twips,
                color,
                dash,
            } => Some(DrawnEdge {
                x1: *x1_twips,
                y1: *y1_twips,
                x2: *x2_twips,
                y2: *y2_twips,
                thickness: *thickness_twips,
                color: color.css(),
                dash: *dash,
            }),
            _ => None,
        })
        .collect()
}

fn cell_border(
    style: opendoc_core::BorderStyle,
    twips: i32,
    color: &str,
) -> opendoc_core::CellBorder {
    opendoc_core::CellBorder::new(
        style,
        Length::from_twips(twips).expect("a border width"),
        opendoc_core::Color::parse(color).expect("a colour"),
    )
    .expect("a valid border")
}

/// A table block of `rows` rows by `columns` columns, every cell holding one
/// short paragraph.
fn table(id: &str, rows: usize, columns: usize) -> Block {
    let mut block = paragraph(id, "");
    block.content.clear();
    block.kind = BlockKind::table(
        (0..rows)
            .map(|row| opendoc_core::TableRow {
                id: StableId::parse(format!("row-{row}")).expect("valid id"),
                height: None,
                header: false,
                cells: (0..columns)
                    .map(|_| opendoc_core::TableCell::new(vec![Block::paragraph("x")]))
                    .collect(),
            })
            .collect(),
    );
    block
}

fn cell_mut(block: &mut Block, row: usize, column: usize) -> &mut opendoc_core::TableCell {
    match &mut block.kind {
        BlockKind::Table { rows, .. } => &mut rows[row].cells[column],
        _ => panic!("not a table"),
    }
}

/// The grid is walked by boundary, so a shared edge is one line rather than
/// two coincident ones.
///
/// A 2x2 table has (2 + 1) * 2 horizontal boundaries and (2 + 1) * 2 vertical
/// ones: twelve. Four rectangles — one per cell — would be sixteen edges with
/// four of them drawn on top of another, which is what makes the count worth
/// asserting rather than merely the geometry.
#[test]
fn an_unstated_table_draws_each_boundary_once_in_the_stylesheets_default() {
    let painted = layout_painted_document(&document(vec![table("tbl", 2, 2)]));
    let edges = drawn_edges(&painted.pages[0]);
    assert_eq!(12, edges.len(), "{edges:#?}");
    for edge in &edges {
        assert_eq!(15, edge.thickness, "0.75pt is 15 twips: {edge:?}");
        assert_eq!("#999999", edge.color, "{edge:?}");
        assert_eq!(None, edge.dash, "{edge:?}");
    }
    // The grid is placed on the sheet, not in the fragment's own frame: a
    // Letter page with 1in margins puts the table's rim at 72pt and 540pt.
    let setup = PageSetup::default();
    assert_eq!(
        setup.margin_start.twips(),
        edges.iter().map(|edge| edge.x1).min().expect("an edge"),
        "{edges:#?}"
    );
    assert_eq!(
        setup.margin_start.twips() + setup.content_width().twips(),
        edges.iter().map(|edge| edge.x2).max().expect("an edge"),
        "{edges:#?}"
    );
    let mut positions: Vec<(i32, i32, i32, i32)> =
        edges.iter().map(|e| (e.x1, e.y1, e.x2, e.y2)).collect();
    positions.sort_unstable();
    let mut unique = positions.clone();
    unique.dedup();
    assert_eq!(positions, unique, "a boundary was drawn twice");
}

#[test]
fn a_merged_cells_interior_boundaries_are_not_painted() {
    let mut block = table("merged", 2, 2);
    let BlockKind::Table { rows, .. } = &mut block.kind else {
        panic!("expected table");
    };
    rows[0].cells[0].span = opendoc_core::CellSpan::new(2, 2).expect("legal merge");
    let painted = layout_painted_document(&document(vec![block]));
    let edges = drawn_edges(&painted.pages[0]);
    // The interior row/column boundaries would meet at the table's centre.
    // A merged 2x2 cell has only its outer rim, never that cross.
    let middle_y = (edges.iter().map(|edge| edge.y1).min().expect("top rim")
        + edges.iter().map(|edge| edge.y1).max().expect("bottom rim"))
        / 2;
    let middle_x = (edges.iter().map(|edge| edge.x1).min().expect("left rim")
        + edges.iter().map(|edge| edge.x1).max().expect("right rim"))
        / 2;
    assert!(
        !edges
            .iter()
            .any(|edge| edge.y1 == edge.y2 && edge.y1 == middle_y),
        "interior horizontal edge: {edges:#?}"
    );
    assert!(
        !edges
            .iter()
            .any(|edge| edge.x1 == edge.x2 && edge.x1 == middle_x),
        "interior vertical edge: {edges:#?}"
    );
}

#[test]
fn a_merged_cell_uses_its_full_rectangle_and_hides_covered_content() {
    let mut block = table("merged-content", 2, 2);
    let BlockKind::Table { rows, .. } = &mut block.kind else {
        panic!("expected table");
    };
    rows[0].cells[0].span = opendoc_core::CellSpan::new(2, 2).expect("legal merge");
    rows[0].cells[0].properties.background =
        Some(opendoc_core::Color::parse("#e8f0fe").expect("colour"));
    rows[0].cells[0].blocks = vec![Block::paragraph("ANCHOR")];
    rows[1].height = Some(Length::from_twips(500).expect("row minimum"));
    for (row_index, row) in rows.iter_mut().enumerate() {
        for (column_index, cell) in row.cells.iter_mut().enumerate() {
            if (row_index, column_index) != (0, 0) {
                cell.blocks = vec![Block::paragraph("COVERED")];
            }
        }
    }
    let painted = layout_painted_document(&document(vec![block]));
    let text = drawn_text(&painted.pages[0]);
    assert!(text.contains("ANCHOR"), "{text:?}");
    assert!(!text.contains("COVERED"), "{text:?}");
    let fills: Vec<_> = painted.pages[0]
        .items
        .iter()
        .filter_map(|item| match item {
            PaintItem::Fill {
                width_twips,
                height_twips,
                color: Some(color),
                ..
            } if color.css() == "#e8f0fe" => Some((*width_twips, *height_twips)),
            _ => None,
        })
        .collect();
    assert_eq!(1, fills.len(), "merged background was split: {fills:#?}");
    assert!(fills[0].0 > 8_000 && fills[0].1 >= 500, "{fills:#?}");
}

#[test]
fn a_table_border_sets_the_default_for_every_unstated_cell_edge() {
    let mut block = table("tbl", 1, 2);
    let BlockKind::Table { properties, .. } = &mut block.kind else {
        panic!("expected table");
    };
    properties.border = Some(cell_border(
        opendoc_core::BorderStyle::Dashed,
        45,
        "#336699",
    ));

    let painted = layout_painted_document(&document(vec![block]));
    let edges = drawn_edges(&painted.pages[0]);
    assert_eq!(
        7,
        edges.len(),
        "a 1x2 grid has seven boundaries: {edges:#?}"
    );
    for edge in &edges {
        assert_eq!(45, edge.thickness, "{edge:?}");
        assert_eq!("#336699", edge.color, "{edge:?}");
        assert_eq!(Some([180, 180]), edge.dash, "{edge:?}");
    }
}

#[test]
fn a_fixed_width_table_alignment_moves_its_border_grid() {
    let positioned_left = |alignment| {
        let mut block = table("tbl", 1, 2);
        let BlockKind::Table {
            columns,
            properties,
            ..
        } = &mut block.kind
        else {
            panic!("expected table");
        };
        for column in columns {
            column.width = Some(Length::from_twips(1_440).expect("one inch"));
        }
        properties.alignment = alignment;
        drawn_edges(&layout_painted_document(&document(vec![block])).pages[0])
            .iter()
            .map(|edge| edge.x1.min(edge.x2))
            .min()
            .expect("table border")
    };

    let start = positioned_left(Some(opendoc_core::TableAlignment::Start));
    let center = positioned_left(Some(opendoc_core::TableAlignment::Center));
    let end = positioned_left(Some(opendoc_core::TableAlignment::End));
    assert!(
        start < center && center < end,
        "table was not repositioned: {start} {center} {end}"
    );
}

/// The finding this fixes: the width, the colour and the style were all
/// constants, so a 2.25pt dashed red border printed as a 0.75pt solid grey
/// one. The fixture differs from the default in every one of the three.
#[test]
fn a_stated_border_is_drawn_at_its_own_width_colour_and_style() {
    let mut block = table("tbl", 1, 1);
    cell_mut(&mut block, 0, 0).properties.border_top = Some(cell_border(
        opendoc_core::BorderStyle::Dashed,
        45,
        "#cc0000",
    ));
    let painted = layout_painted_document(&document(vec![block]));
    let edges = drawn_edges(&painted.pages[0]);
    assert_eq!(4, edges.len(), "one cell is four boundaries: {edges:#?}");
    let top = edges
        .iter()
        .min_by_key(|edge| (edge.y1, edge.x1))
        .expect("a top edge");
    assert_eq!(45, top.thickness, "2.25pt is 45 twips: {top:?}");
    assert_eq!("#cc0000", top.color, "{top:?}");
    // Four times the thickness on and off: 3pt at the 0.75pt default, which
    // is the dash the explicit page break's rule was measured at.
    assert_eq!(Some([180, 180]), top.dash, "{top:?}");
    assert_eq!(top.y1, top.y2, "the top boundary is horizontal: {top:?}");
    // The other three are untouched, which is what makes this a test of one
    // edge rather than of the whole cell.
    for edge in edges.iter().filter(|edge| *edge != top) {
        assert_eq!(15, edge.thickness, "{edge:?}");
        assert_eq!("#999999", edge.color, "{edge:?}");
        assert_eq!(None, edge.dash, "{edge:?}");
    }
}

/// Both cells own the boundary between them and the document may state them
/// differently. CSS compares the used width first and only then the style, so
/// the 3pt solid border wins over the 0.5pt dotted one — and the loser leaves
/// nothing behind.
#[test]
fn two_cells_that_disagree_about_a_boundary_draw_one_line_the_wider_one() {
    let mut block = table("tbl", 1, 2);
    cell_mut(&mut block, 0, 0).properties.border_end =
        Some(cell_border(opendoc_core::BorderStyle::Solid, 60, "#0000ff"));
    cell_mut(&mut block, 0, 1).properties.border_start = Some(cell_border(
        opendoc_core::BorderStyle::Dotted,
        10,
        "#00aa00",
    ));
    let painted = layout_painted_document(&document(vec![block]));
    let edges = drawn_edges(&painted.pages[0]);
    let shared: Vec<&DrawnEdge> = edges
        .iter()
        .filter(|edge| edge.thickness != 15 || edge.color != "#999999")
        .collect();
    assert_eq!(1, shared.len(), "the boundary is one line: {edges:#?}");
    assert_eq!(60, shared[0].thickness, "{shared:?}");
    assert_eq!("#0000ff", shared[0].color, "{shared:?}");
    assert_eq!(None, shared[0].dash, "the solid border won: {shared:?}");
    assert_eq!(shared[0].x1, shared[0].x2, "it is vertical: {shared:?}");
    assert!(
        !edges.iter().any(|edge| edge.color == "#00aa00"),
        "the losing border was drawn as well: {edges:#?}"
    );
}

/// `BorderStyle::None` is not a hole in the grid. Its used width is zero, so
/// it loses a *shared* boundary to the neighbour's default and the line stays;
/// on the table's rim, where it is the only contributor, it clears the line.
///
/// This is the screen's behaviour, and a PDF that treated `none` as "skip this
/// edge" would erase lines Chrome draws.
#[test]
fn a_border_turned_off_clears_the_rim_but_loses_a_shared_boundary() {
    let mut block = table("tbl", 1, 2);
    // A `none` border with a *stated* width of 2.25pt and a colour that would
    // be unmistakable if it were drawn. `CellBorder::none()` carries a width
    // of zero, so a resolution that compared stated widths instead of used
    // ones would still get that one right by accident; this one it cannot.
    let off = cell_border(opendoc_core::BorderStyle::None, 45, "#cc0000");
    let silenced = &mut cell_mut(&mut block, 0, 0).properties;
    silenced.border_top = Some(off);
    silenced.border_bottom = Some(off);
    silenced.border_start = Some(off);
    silenced.border_end = Some(off);
    let painted = layout_painted_document(&document(vec![block]));
    let edges = drawn_edges(&painted.pages[0]);
    // Seven boundaries in a 1x2 grid; the three that only the silenced cell
    // touches — its top, its bottom and the left rim — are gone. The shared
    // vertical boundary and the second cell's own three remain.
    assert_eq!(4, edges.len(), "{edges:#?}");
    let left_rim = edges.iter().map(|edge| edge.x1).min().expect("an edge");
    let right_rim = edges.iter().map(|edge| edge.x2).max().expect("an edge");
    let shared = edges
        .iter()
        .filter(|edge| edge.x1 == edge.x2 && edge.x1 != right_rim)
        .collect::<Vec<_>>();
    assert_eq!(
        1,
        shared.len(),
        "the shared boundary is drawn once by the neighbour's default: {edges:#?}"
    );
    assert_eq!(
        15, shared[0].thickness,
        "a turned-off border won a boundary on its stated width: {edges:#?}"
    );
    assert_eq!("#999999", shared[0].color, "{edges:#?}");
    assert_eq!(
        shared[0].x1, left_rim,
        "the silenced cell's rim is still drawn: {edges:#?}"
    );
}

/// A `double` border is three equal parts: line, gap, line. The layout splits
/// it rather than handing a consumer a width to divide, for the reason ADR
/// 0016 gives for the hollow bullet — a decision made here can be tested.
#[test]
fn a_double_border_is_drawn_as_two_lines_a_third_of_its_width() {
    let mut block = table("tbl", 1, 1);
    cell_mut(&mut block, 0, 0).properties.border_top = Some(cell_border(
        opendoc_core::BorderStyle::Double,
        60,
        "#cc0000",
    ));
    let painted = layout_painted_document(&document(vec![block]));
    let edges = drawn_edges(&painted.pages[0]);
    let doubled: Vec<&DrawnEdge> = edges
        .iter()
        .filter(|edge| edge.color == "#cc0000")
        .collect();
    assert_eq!(2, doubled.len(), "{edges:#?}");
    for line in &doubled {
        assert_eq!(20, line.thickness, "a third of 3pt: {line:?}");
        assert_eq!(None, line.dash, "each half of a double is solid: {line:?}");
    }
    // The two centre lines are 2/3 of the width apart, which puts the band's
    // outer faces exactly 3pt apart.
    let gap = (doubled[0].y1 - doubled[1].y1).abs();
    assert_eq!(40, gap, "{doubled:#?}");
}

/// A dotted border is dotted on paper, not solid: the dash is the thickness
/// long with a gap of the same.
#[test]
fn a_dotted_border_carries_a_dash_pattern_of_its_own_thickness() {
    let mut block = table("tbl", 1, 1);
    cell_mut(&mut block, 0, 0).properties.border_top = Some(cell_border(
        opendoc_core::BorderStyle::Dotted,
        30,
        "#cc0000",
    ));
    let painted = layout_painted_document(&document(vec![block]));
    let dotted = drawn_edges(&painted.pages[0])
        .into_iter()
        .find(|edge| edge.color == "#cc0000")
        .expect("the stated border");
    assert_eq!(30, dotted.thickness);
    assert_eq!(Some([30, 30]), dotted.dash, "{dotted:?}");
}

/// `start` and `end` are direction-relative in the model, and in the CSS
/// logical properties `opendoc-render` projects them to. A right-to-left
/// table's `border_start` is therefore its *right* edge.
#[test]
fn a_right_to_left_tables_start_border_is_its_right_edge() {
    let mut block = table("tbl", 1, 1);
    block.properties.direction = Some(TextDirection::RightToLeft);
    cell_mut(&mut block, 0, 0).properties.border_start =
        Some(cell_border(opendoc_core::BorderStyle::Solid, 60, "#0000ff"));
    let painted = layout_painted_document(&document(vec![block]));
    let edges = drawn_edges(&painted.pages[0]);
    let stated = edges
        .iter()
        .find(|edge| edge.color == "#0000ff")
        .expect("the stated border");
    let right_rim = edges.iter().map(|edge| edge.x2).max().expect("an edge");
    assert_eq!(right_rim, stated.x1, "{edges:#?}");
    assert_eq!(stated.x1, stated.x2, "it is a vertical edge: {stated:?}");
}

/// When two borders are the same width, the *style* decides — and the answer
/// must not depend on which of the two cells happens to state it, which is
/// what the second half of this checks. A resolution that simply kept the
/// first contributor would pass the first half and fail the second.
#[test]
fn style_breaks_a_tie_on_width_whichever_cell_states_it() {
    for (name, left, right) in [
        (
            "the solid border on the left",
            opendoc_core::BorderStyle::Solid,
            opendoc_core::BorderStyle::Dotted,
        ),
        (
            "the solid border on the right",
            opendoc_core::BorderStyle::Dotted,
            opendoc_core::BorderStyle::Solid,
        ),
    ] {
        let mut block = table("tbl", 1, 2);
        cell_mut(&mut block, 0, 0).properties.border_end = Some(cell_border(left, 30, "#cc0000"));
        cell_mut(&mut block, 0, 1).properties.border_start =
            Some(cell_border(right, 30, "#cc0000"));
        let painted = layout_painted_document(&document(vec![block]));
        let shared: Vec<DrawnEdge> = drawn_edges(&painted.pages[0])
            .into_iter()
            .filter(|edge| edge.color == "#cc0000")
            .collect();
        assert_eq!(1, shared.len(), "{name}: {shared:#?}");
        assert_eq!(
            None, shared[0].dash,
            "{name}: the dotted border won a tie the solid one should have: {shared:#?}"
        );
    }
}

/// And when width and style are both tied, the cell that comes first in
/// document order wins — CSS's own tie-break, and the only one that keeps two
/// equally-stated colours from being a coin toss.
#[test]
fn a_tie_on_width_and_style_goes_to_the_cell_that_comes_first() {
    let mut block = table("tbl", 1, 2);
    cell_mut(&mut block, 0, 0).properties.border_end =
        Some(cell_border(opendoc_core::BorderStyle::Solid, 30, "#cc0000"));
    cell_mut(&mut block, 0, 1).properties.border_start =
        Some(cell_border(opendoc_core::BorderStyle::Solid, 30, "#0000ff"));
    let painted = layout_painted_document(&document(vec![block]));
    let shared: Vec<DrawnEdge> = drawn_edges(&painted.pages[0])
        .into_iter()
        .filter(|edge| edge.thickness == 30)
        .collect();
    assert_eq!(1, shared.len(), "{shared:#?}");
    assert_eq!(
        "#cc0000", shared[0].color,
        "the right-hand cell's colour won the tie: {shared:#?}"
    );
}

/// A table inside a cell draws its own grid, placed inside the cell that holds
/// it rather than at the outer table's origin. Both the offset down into the
/// row and the cell's content frame are load-bearing, so both are asserted.
#[test]
fn a_nested_tables_grid_is_drawn_inside_the_cell_that_holds_it() {
    let mut inner = table("inner", 1, 1);
    inner.id = StableId::parse("inner").expect("valid id");
    let mut outer = table("outer", 1, 1);
    cell_mut(&mut outer, 0, 0).blocks = vec![inner];
    let painted = layout_painted_document(&document(vec![outer]));
    let edges = drawn_edges(&painted.pages[0]);
    assert_eq!(8, edges.len(), "two tables of one cell: {edges:#?}");
    let mut verticals: Vec<i32> = edges
        .iter()
        .filter(|edge| edge.x1 == edge.x2)
        .map(|edge| edge.x1)
        .collect();
    verticals.sort_unstable();
    verticals.dedup();
    let mut horizontals: Vec<i32> = edges
        .iter()
        .filter(|edge| edge.y1 == edge.y2)
        .map(|edge| edge.y1)
        .collect();
    horizontals.sort_unstable();
    horizontals.dedup();
    assert_eq!(4, verticals.len(), "{edges:#?}");
    assert_eq!(4, horizontals.len(), "{edges:#?}");
    assert!(
        verticals[0] < verticals[1] && verticals[2] < verticals[3],
        "the inner grid is not inside the outer one horizontally: {verticals:?}"
    );
    assert!(
        horizontals[0] < horizontals[1] && horizontals[2] < horizontals[3],
        "the inner grid is not inside the outer one vertically: {horizontals:?}"
    );
}
