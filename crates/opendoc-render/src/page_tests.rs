use crate::test_support::*;
use crate::*;
use opendoc_core::{Alignment, BlockProperty, Length, PageNumberField};

#[test]
fn page_setup_projects_to_pt_custom_properties() {
    let setup = opendoc_core::PageSetup::default();
    let css = page_setup_css_variables(&setup);
    // 12240 twips is 612pt exactly; a px projection would have to assume
    // a DPI to say 816.
    assert!(css.contains("--page-width: 612pt;"), "{css}");
    assert!(css.contains("--page-height: 792pt;"), "{css}");
    assert!(css.contains("--page-margin-top: 72pt;"), "{css}");
    assert!(css.contains("--page-margin-start: 72pt;"), "{css}");
    assert!(css.contains("--page-content-width: 468pt;"), "{css}");
    assert!(css.contains("--page-content-height: 648pt;"), "{css}");
    assert!(css.contains("--page-orientation: portrait;"), "{css}");
    assert_eq!(
        page_setup_print_css(&setup),
        "@page { size: 612pt 792pt; margin: 0; }"
    );
}

#[test]
fn a4_landscape_projects_its_own_dimensions() {
    let setup = opendoc_core::PageSetup::from_size_name("a4")
        .unwrap()
        .with_orientation(opendoc_core::PageOrientation::Landscape);
    let css = page_setup_css_variables(&setup);
    // 16838 twips = 841.90pt, exact on the 0.05pt grid.
    assert!(css.contains("--page-width: 841.90pt;"), "{css}");
    assert!(css.contains("--page-height: 595.30pt;"), "{css}");
    assert!(css.contains("--page-orientation: landscape;"), "{css}");
}

#[test]
fn an_empty_furniture_slot_renders_nothing() {
    let document = document_with(&["body"]);
    assert_eq!(
        render_page_furniture(&document, HeaderFooterSlot::Header, []),
        Rendering::default()
    );
}

#[test]
fn a_header_renders_its_blocks_and_leaves_the_body_alone() {
    let mut document = document_with(&["body"]);
    let mut header = Block::paragraph("");
    header.content = vec![
        Inline::text("Chapter "),
        Inline::PageNumber {
            id: StableId::parse("field-page").unwrap(),
            field: PageNumberField::CurrentPage,
        },
    ];
    header
        .properties
        .set(BlockProperty::Alignment(Alignment::Center));
    document.header.push(header);
    document.validate().unwrap();

    let header_html = render_page_furniture(&document, HeaderFooterSlot::Header, []).html;
    assert!(header_html.contains("Chapter "), "{header_html}");
    assert!(header_html.contains("text-align:center;"), "{header_html}");
    // The field is empty: the renderer cannot know the page number, and
    // inventing one would put a number in the markup the document never
    // said. Whoever paginates fills it in.
    assert!(
        header_html.contains("data-field=\"page-number\"></span>"),
        "{header_html}"
    );
    assert!(header_html.contains("doc-page-number"), "{header_html}");

    let body = render_document_html(&document, []);
    assert!(!body.contains("Chapter "), "{body}");
}

#[test]
fn header_markup_is_rendered_once_not_per_page() {
    let mut document = document_with(&["body"]);
    let mut header = Block::paragraph("Running head");
    header.id = StableId::parse("block-header").unwrap();
    document.header.push(header);
    // Even on a sheet only tall enough for a line or two, the projection
    // emits one copy: how many pages there are is not something a pure
    // projection can know.
    document.page_setup = document
        .page_setup
        .with_size(
            Length::from_inches(8.5).unwrap(),
            Length::from_inches(3.0).unwrap(),
        )
        .unwrap();
    let html = render_page_furniture(&document, HeaderFooterSlot::Header, []).html;
    assert_eq!(html.matches("Running head").count(), 1, "{html}");
}

#[test]
fn a_page_number_field_in_the_body_renders_as_an_unresolved_field() {
    let mut document = document_with(&[]);
    let mut block = Block::paragraph("");
    block.content = vec![Inline::PageNumber {
        id: StableId::parse("field-count").unwrap(),
        field: PageNumberField::PageCount,
    }];
    document.blocks.push(block);
    let html = render_document_html(&document, []);
    assert!(html.contains("data-field=\"page-count\""), "{html}");
    assert!(html.contains("contenteditable=\"false\""), "{html}");
    assert!(html.contains("data-inline-id=\"field-count\""), "{html}");
}
