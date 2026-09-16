use crate::*;

#[test]
fn default_page_is_us_letter_with_one_inch_margins() {
    let setup = PageSetup::default();
    assert_eq!(setup.width.inches(), 8.5);
    assert_eq!(setup.height.inches(), 11.0);
    assert_eq!(setup.margin_top.twips(), 1440);
    assert_eq!(setup.margin_end.twips(), 1440);
    assert_eq!(setup.size_name(), Some("letter"));
    assert_eq!(setup.orientation(), PageOrientation::Portrait);
    setup.validate().unwrap();
}

#[test]
fn a_named_size_stores_dimensions_not_the_name() {
    let setup = PageSetup::from_size_name("a4").unwrap();
    assert_eq!(setup.width.twips(), 11906);
    assert_eq!(setup.height.twips(), 16838);
    // The name is recovered by measuring, so a document that never said
    // "A4" still reports A4.
    let measured = PageSetup::new(
        Length::from_twips(11906).unwrap(),
        Length::from_twips(16838).unwrap(),
    )
    .unwrap();
    assert_eq!(measured.size_name(), Some("a4"));
    assert!(PageSetup::from_size_name("quarto").is_err());
}

#[test]
fn orientation_is_derived_and_rotation_is_idempotent() {
    let portrait = PageSetup::from_size_name("a4").unwrap();
    let landscape = portrait.with_orientation(PageOrientation::Landscape);
    assert_eq!(landscape.width.twips(), 16838);
    assert_eq!(landscape.height.twips(), 11906);
    assert_eq!(landscape.orientation(), PageOrientation::Landscape);
    // Still A4: the name does not depend on which way round it is.
    assert_eq!(landscape.size_name(), Some("a4"));
    // Applying the same orientation twice must not spin the page.
    assert_eq!(
        landscape.with_orientation(PageOrientation::Landscape),
        landscape
    );
    assert_eq!(
        landscape.with_orientation(PageOrientation::Portrait),
        portrait
    );
    // A square page is portrait, as the type documents.
    let side = Length::from_inches(8.0).unwrap();
    assert_eq!(
        PageSetup::new(side, side).unwrap().orientation(),
        PageOrientation::Portrait
    );
}

#[test]
fn margins_that_swallow_the_page_are_rejected() {
    let setup = PageSetup::default();
    let six_inches = Length::from_inches(6.0).unwrap();
    assert!(matches!(
        setup.with_margins(
            setup.margin_top,
            setup.margin_bottom,
            six_inches,
            six_inches
        ),
        Err(ModelError::InvalidDocument(
            "page side margins leave no content width"
        ))
    ));
    let ten_inches = Length::from_inches(10.0).unwrap();
    assert!(matches!(
        setup.with_margins(ten_inches, ten_inches, setup.margin_start, setup.margin_end),
        Err(ModelError::InvalidDocument(
            "page top and bottom margins leave no content height"
        ))
    ));
    let negative = Length::from_twips(-20).unwrap();
    assert!(matches!(
        setup.with_margins(
            negative,
            setup.margin_bottom,
            setup.margin_start,
            setup.margin_end
        ),
        Err(ModelError::InvalidDocument("page margin is negative"))
    ));
}

#[test]
fn content_box_is_the_page_less_its_margins() {
    let setup = PageSetup::default();
    assert_eq!(setup.content_width().twips(), 12240 - 2880);
    assert_eq!(setup.content_height().twips(), 15840 - 2880);
}

#[test]
fn document_validation_reaches_page_setup() {
    let mut doc = Document::new("Page");
    doc.blocks.push(Block::paragraph("body"));
    doc.validate().unwrap();
    doc.page_setup.width = Length::ZERO;
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument("page size is not positive"))
    ));
}

// ---- Headers, footers and page-number fields ------------------------

#[test]
fn header_and_footer_share_the_document_id_space() {
    let mut doc = Document::new("Page");
    let mut body = Block::paragraph("body");
    body.id = StableId::parse("block-shared").unwrap();
    doc.blocks.push(body.clone());
    doc.header.push(body);
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument("duplicate block id"))
    ));
}

#[test]
fn first_page_furniture_distinguishes_inheritance_from_explicit_suppression() {
    let mut doc = Document::new("Page");
    doc.blocks.push(Block::paragraph("body"));
    let mut ordinary = Block::paragraph("ordinary header");
    ordinary.id = StableId::parse("ordinary-header").unwrap();
    doc.header.push(ordinary);

    assert_eq!(
        doc.furniture_for_page(HeaderFooterSlot::Header, 0),
        doc.furniture(HeaderFooterSlot::Header),
        "a missing override inherits the ordinary header"
    );
    assert!(!doc.has_furniture_override(HeaderFooterSlot::FirstPageHeader));

    *doc.furniture_mut(HeaderFooterSlot::FirstPageHeader) = Vec::new();
    assert!(doc.has_furniture_override(HeaderFooterSlot::FirstPageHeader));
    assert!(doc
        .furniture_for_page(HeaderFooterSlot::Header, 0)
        .is_empty());
    assert_eq!(
        doc.furniture_for_page(HeaderFooterSlot::Header, 1),
        doc.furniture(HeaderFooterSlot::Header)
    );
    doc.validate().unwrap();
}

#[test]
fn even_page_furniture_inherits_and_does_not_override_page_one() {
    let mut doc = Document::new("Page");
    doc.blocks.push(Block::paragraph("body"));
    let mut ordinary = Block::paragraph("ordinary header");
    ordinary.id = StableId::parse("ordinary-even-header").unwrap();
    doc.header.push(ordinary);
    let mut even = Block::paragraph("even header");
    even.id = StableId::parse("even-header").unwrap();
    *doc.furniture_mut(HeaderFooterSlot::EvenPageHeader) = vec![even];

    assert_eq!(
        doc.furniture_for_page(HeaderFooterSlot::Header, 0),
        doc.furniture(HeaderFooterSlot::Header),
        "page one is odd, so an even override cannot affect it"
    );
    assert_eq!(
        doc.furniture_for_page(HeaderFooterSlot::Header, 1),
        doc.furniture(HeaderFooterSlot::EvenPageHeader)
    );
    assert_eq!(
        doc.furniture_for_page(HeaderFooterSlot::Header, 2),
        doc.furniture(HeaderFooterSlot::Header)
    );
    doc.validate().unwrap();
}

#[test]
fn page_furniture_rejects_content_that_needs_a_body_flow() {
    let mut doc = Document::new("Page");
    doc.blocks.push(Block::paragraph("body"));
    let mut page_break = Block::paragraph("");
    page_break.kind = BlockKind::PageBreak;
    page_break.content.clear();
    doc.header.push(page_break);
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "page furniture cannot contain a page break"
        ))
    ));

    let mut doc = Document::new("Page");
    doc.blocks.push(Block::paragraph("body"));
    doc.footnotes.push(Footnote {
        id: StableId::parse("note-1").unwrap(),
        revision: 1,
        body: vec![Inline::text("note")],
        deleted: false,
    });
    let mut footer = Block::paragraph("");
    footer.content = vec![Inline::FootnoteRef {
        id: StableId::new("ref"),
        footnote_id: StableId::parse("note-1").unwrap(),
    }];
    doc.footer.push(footer);
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "page furniture cannot contain a footnote reference"
        ))
    ));
}

#[test]
fn a_page_number_field_contributes_no_source_text() {
    let mut doc = Document::new("Page");
    let mut block = Block::paragraph("");
    block.content = vec![
        Inline::text("Page "),
        Inline::PageNumber {
            id: StableId::new("field"),
            field: PageNumberField::CurrentPage,
        },
    ];
    doc.blocks.push(block);
    doc.validate().unwrap();
    // The value is produced by pagination, so counting it would make the
    // word count depend on the page size.
    assert_eq!(doc.visible_text(), "Page \n");
}

#[test]
fn page_number_field_names_round_trip() {
    for field in PageNumberField::ALL {
        assert_eq!(PageNumberField::parse(field.as_str()).unwrap(), field);
    }
    assert!(PageNumberField::parse("section-number").is_err());
    for slot in HeaderFooterSlot::ALL {
        assert_eq!(HeaderFooterSlot::parse(slot.as_str()).unwrap(), slot);
    }
    assert!(HeaderFooterSlot::parse("watermark").is_err());
    for orientation in PageOrientation::ALL {
        assert_eq!(
            PageOrientation::parse(orientation.as_str()).unwrap(),
            orientation
        );
    }
    assert!(PageOrientation::parse("sideways").is_err());
}

#[test]
fn every_page_size_preset_is_a_valid_portrait_page() {
    for preset in PAGE_SIZE_PRESETS {
        let setup = PageSetup::from_size_name(preset.name).unwrap();
        setup.validate().unwrap();
        assert_eq!(
            setup.orientation(),
            PageOrientation::Portrait,
            "{}",
            preset.name
        );
        assert_eq!(setup.size_name(), Some(preset.name));
    }
}
