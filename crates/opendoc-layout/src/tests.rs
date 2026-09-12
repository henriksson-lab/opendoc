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

#[test]
fn an_empty_document_is_one_page() {
    let layout = layout_document(&document(Vec::new()));
    assert_eq!(layout.page_count, 1);
    assert!(layout.blocks.is_empty());
    assert!(layout.exact);
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
    let text = "The quick brown fox jumps over the lazy dog and then turns around again.";
    let narrow = PageSetup {
        margin_start: Length::from_twips(3_600).unwrap(),
        margin_end: Length::from_twips(3_600).unwrap(),
        ..PageSetup::default()
    };

    let plain = paragraph("block-0", text);
    let mut bold = plain.clone();
    if let Some(Inline::Text { marks, .. }) = bold.content.first_mut() {
        marks.push(Mark {
            kind: MarkKind::Bold,
            value: None,
            expand: opendoc_core::MarkExpand::None,
        });
    }
    let mut plain_doc = document(vec![plain]);
    plain_doc.page_setup = narrow;
    let mut bold_doc = document(vec![bold]);
    bold_doc.page_setup = narrow;
    assert!(
        layout_document(&bold_doc).blocks[0].lines >= layout_document(&plain_doc).blocks[0].lines
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
