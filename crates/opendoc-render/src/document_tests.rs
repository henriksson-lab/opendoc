use crate::context::inline_id;
use crate::footnotes::render_footnotes_html;
use crate::test_support::*;
use crate::*;
use opendoc_core::{
    BibliographyReference, Bookmark, CitationGroup, CitationItem, CitationPlacement,
    CitationSource, CitationSourceFormat, CitationSummary, DropdownOption, ListProperties,
    MarkExpand, TextRange,
};

#[test]
fn renders_paragraphs_with_ids_and_escapes() {
    let document = document_with(&["a < b & c"]);
    let html = render_document_html(&document, []);
    let id = document.blocks[0].id.to_string();
    assert!(html.contains(&format!("data-block-id=\"{id}\"")));
    assert!(html.contains("a &lt; b &amp; c"));
    assert!(html.starts_with("<p class=\"doc-block doc-paragraph\""));
}

#[test]
fn date_chip_is_a_focusable_labelled_atomic_editor_control() {
    let mut document = document_with(&[""]);
    document.blocks[0].content = vec![Inline::DateChip {
        id: StableId::parse("date-chip-1").expect("valid id"),
        date: "2024-02-29".to_string(),
    }];

    let html = render_document_html(&document, []);

    assert!(
        html.contains(
            "data-inline-id=\"date-chip-1\" data-kind=\"date-chip\" data-inline-kind=\"date-chip\""
        ),
        "{html}"
    );
    assert!(
        html.contains("contenteditable=\"false\" tabindex=\"0\" role=\"button\""),
        "{html}"
    );
    assert!(
        html.contains("datetime=\"2024-02-29\" aria-label=\"Edit date: 2024-02-29\""),
        "{html}"
    );
}

#[test]
fn google_smart_chips_render_as_read_only_labelled_atoms_without_lookup() {
    let mut document = document_with(&[""]);
    document.blocks[0].content = vec![
        Inline::GooglePersonChip {
            id: StableId::parse("google-person-1").unwrap(),
            label: "Ada Lovelace".to_string(),
            email: "ada@example.invalid".to_string(),
            person_id: Some("people/ada".to_string()),
        },
        Inline::GoogleRichLinkChip {
            id: StableId::parse("google-rich-link-1").unwrap(),
            label: "Design brief".to_string(),
            href: "https://example.invalid/brief".to_string(),
            rich_link_id: Some("chip-42".to_string()),
            mime_type: Some("application/vnd.google-apps.document".to_string()),
        },
    ];
    let html = render_document_html(&document, []);
    assert!(
        html.contains("data-google-person-email=\"ada@example.invalid\""),
        "{html}"
    );
    assert!(
        html.contains("aria-label=\"Google person chip: Ada Lovelace (ada@example.invalid)\""),
        "{html}"
    );
    assert!(
        html.contains("data-google-rich-link-id=\"chip-42\""),
        "{html}"
    );
    assert!(html.contains("contenteditable=\"false\""), "{html}");
}

#[test]
fn google_rich_link_chip_sanitizes_legacy_unsafe_href_but_retains_navigation_data() {
    let mut document = document_with(&[""]);
    document.blocks[0].content = vec![Inline::GoogleRichLinkChip {
        id: StableId::parse("unsafe-google-rich-link").unwrap(),
        label: "Unsafe source".to_string(),
        href: "javascript:alert(1)".to_string(),
        rich_link_id: None,
        mime_type: None,
    }];

    let html = render_document_html(&document, []);
    assert!(html.contains("href=\"#\""), "{html}");
    assert!(html.contains("data-href=\"javascript:alert(1)\""), "{html}");
}

#[test]
fn dropdown_chip_is_a_focusable_labelled_atomic_editor_control() {
    let mut document = document_with(&[""]);
    document.blocks[0].content = vec![Inline::Dropdown {
        id: StableId::parse("dropdown-chip-1").expect("valid id"),
        options: vec![DropdownOption {
            id: "ready".to_string(),
            label: "Ready".to_string(),
        }],
        selected_option_id: "ready".to_string(),
    }];

    let html = render_document_html(&document, []);
    assert!(
        html.contains(
            "contenteditable=\"false\" tabindex=\"0\" role=\"button\" aria-haspopup=\"listbox\""
        ),
        "{html}"
    );
    assert!(html.contains("aria-label=\"Dropdown: Ready\""), "{html}");
}

#[test]
fn live_bookmark_projects_to_a_named_html_anchor_without_replacing_block_identity() {
    let document = document_with(&["target"]);
    let mut document = document;
    document.bookmarks.push(Bookmark {
        id: StableId::parse("bookmark-target").unwrap(),
        name: "intro_target".to_string(),
        block_id: document.blocks[0].id.clone(),
        revision: 1,
        deleted: false,
    });
    let html = render_document_html(&document, []);
    assert!(html.contains("id=\"intro_target\""), "{html}");
    assert!(
        html.contains(&format!("data-block-id=\"{}\"", document.blocks[0].id)),
        "{html}"
    );
}

#[test]
fn list_start_is_list_owned_and_projects_to_html_wrapper_and_items() {
    let mut document = document_with(&["seven", "eight"]);
    let list_id = StableId::parse("continued-list").expect("valid id");
    for block in &mut document.blocks {
        block.kind = BlockKind::ListItem {
            list_id: list_id.clone(),
            level: 0,
            kind: ListKind::Ordered,
        };
    }
    document.list_properties.insert(
        list_id,
        ListProperties {
            ordered_starts: [(0, 7)].into_iter().collect(),
            ..ListProperties::default()
        },
    );
    let html = render_document_html(&document, []);
    assert!(
        html.contains("<ol class=\"doc-list depth-0\" data-level=\"0\" start=\"7\">"),
        "{html}"
    );
    assert!(html.contains("value=\"7\""), "{html}");
    assert!(html.contains("value=\"8\""), "{html}");
}

#[test]
fn explicit_list_format_projects_to_matching_html_counter_style() {
    let mut document = document_with(&["first"]);
    let list_id = StableId::parse("formatted-list").expect("valid id");
    document.blocks[0].kind = BlockKind::ListItem {
        list_id: list_id.clone(),
        level: 0,
        kind: ListKind::Ordered,
    };
    document.list_properties.insert(
        list_id,
        ListProperties {
            ordered_formats: [(0, opendoc_core::OrderedListFormat::UpperRoman)]
                .into_iter()
                .collect(),
            ..ListProperties::default()
        },
    );
    let html = render_document_html(&document, []);
    assert!(
        html.contains("style=\"list-style-type: upper-roman\""),
        "{html}"
    );
}

#[test]
fn renders_horizontal_rule_as_named_atomic_divider() {
    let mut document = document_with(&[]);
    document.blocks.push(Block {
        id: StableId::parse("horizontal-rule").expect("valid id"),
        kind: BlockKind::HorizontalRule,
        content: Vec::new(),
        properties: BlockProperties::default(),
    });
    let html = render_document_html(&document, []);
    assert!(html.contains("doc-horizontal-rule"), "{html}");
    assert!(html.contains("aria-label=\"Horizontal rule\""), "{html}");
    assert!(html.contains("contenteditable=\"false\""), "{html}");
    assert!(html.contains("tabindex=\"0\""), "{html}");
    assert!(
        html.contains("Press Escape to deselect this horizontal rule."),
        "{html}"
    );
}

#[test]
fn table_of_contents_is_derived_from_live_headings_and_links_to_stable_ids() {
    let mut document = document_with(&["Intro", "Deep", "Body"]);
    document.blocks[0].kind = BlockKind::Heading { level: 1 };
    document.blocks[1].kind = BlockKind::Heading { level: 4 };
    document.blocks.push(Block {
        id: StableId::parse("contents").unwrap(),
        kind: BlockKind::TableOfContents { max_level: 3 },
        content: Vec::new(),
        properties: BlockProperties::default(),
    });
    let html = render_document_html(&document, []);
    assert!(html.contains("doc-table-of-contents"), "{html}");
    assert!(
        html.contains(&format!(
            "href=\"#opendoc-heading:{}\"",
            document.blocks[0].id
        )),
        "{html}"
    );
    assert!(
        html.contains(&format!("id=\"opendoc-heading:{}\"", document.blocks[0].id)),
        "TOC target is missing: {html}"
    );
    assert!(html.contains(">Intro</a>"), "{html}");
    assert!(!html.contains(">Deep</a>"), "{html}");
}

#[test]
fn bibliography_is_derived_from_live_citations_not_reference_list_tail() {
    let mut document = document_with(&["Body"]);
    document.citation_database.style = "numeric".to_string();
    for (id, title) in [
        ("ref-used", "Cited work"),
        ("ref-tail", "Uncited list tail"),
    ] {
        document
            .citation_database
            .references
            .push(BibliographyReference {
                id: StableId::parse(id).unwrap(),
                revision: 1,
                source: CitationSource {
                    format: CitationSourceFormat::CitumNative,
                    bytes: b"citation".to_vec(),
                },
                summary: CitationSummary {
                    title: title.to_string(),
                    authors: vec!["Author".to_string()],
                    issued: Some("2024".to_string()),
                    doi: None,
                    url: None,
                },
                deleted: false,
            });
    }
    document.citation_database.citations.push(CitationGroup {
        id: StableId::parse("citation-used").unwrap(),
        revision: 1,
        items: vec![CitationItem {
            reference_id: StableId::parse("ref-used").unwrap(),
            locator: None,
            label: None,
            prefix: None,
            suffix: None,
            suppress_author: false,
        }],
        placement: CitationPlacement::Inline,
        rendered_cache: None,
        deleted: false,
    });
    document.blocks.push(Block {
        id: StableId::parse("bibliography").unwrap(),
        kind: BlockKind::Bibliography,
        content: Vec::new(),
        properties: BlockProperties::default(),
    });

    let html = render_document_html(&document, []);
    assert!(html.contains("doc-bibliography"), "{html}");
    assert!(html.contains("Cited work"), "{html}");
    assert!(!html.contains("Uncited list tail"), "{html}");
}

#[test]
fn a_bookmark_named_like_a_heading_block_id_cannot_collide_with_a_toc_target() {
    let mut document = document_with(&["Heading"]);
    document.blocks[0].id = StableId::parse("heading-target").unwrap();
    document.blocks[0].kind = BlockKind::Heading { level: 1 };
    document.bookmarks.push(Bookmark {
        id: StableId::parse("bookmark-heading").unwrap(),
        name: "heading-target".to_string(),
        block_id: document.blocks[0].id.clone(),
        revision: 1,
        deleted: false,
    });
    document.blocks.push(Block {
        id: StableId::parse("contents").unwrap(),
        kind: BlockKind::TableOfContents { max_level: 1 },
        content: Vec::new(),
        properties: BlockProperties::default(),
    });

    let html = render_document_html(&document, []);
    assert_eq!(
        html.matches("<span class=\"doc-bookmark-anchor\" id=\"heading-target\"")
            .count(),
        1,
        "{html}"
    );
    assert_eq!(
        html.matches("id=\"opendoc-heading:heading-target\"")
            .count(),
        1,
        "{html}"
    );
    assert!(
        html.contains("href=\"#opendoc-heading:heading-target\""),
        "{html}"
    );
}

#[test]
fn table_of_contents_exposes_heading_nesting_to_navigation_readers() {
    let mut document = document_with(&["Chapter", "Section", "Skipped level"]);
    document.blocks[0].kind = BlockKind::Heading { level: 1 };
    document.blocks[1].kind = BlockKind::Heading { level: 2 };
    // A skipped source level must not fabricate a missing heading merely to
    // make the TOC tree look regular.
    document.blocks[2].kind = BlockKind::Heading { level: 4 };
    document.blocks.push(Block {
        id: StableId::parse("contents").unwrap(),
        kind: BlockKind::TableOfContents { max_level: 4 },
        content: Vec::new(),
        properties: BlockProperties::default(),
    });

    let html = render_document_html(&document, []);
    assert!(html.contains(
        &format!(
            "<li data-level=\"1\"><a href=\"#opendoc-heading:{}\">Chapter</a><ol><li data-level=\"2\"><a href=\"#opendoc-heading:{}\">Section</a><ol><li data-level=\"3\"><a href=\"#opendoc-heading:{}\">Skipped level</a>",
            document.blocks[0].id,
            document.blocks[1].id,
            document.blocks[2].id
        )
    ), "{html}");
}

#[test]
fn image_alt_text_is_accessible_metadata_not_a_visible_caption() {
    let mut document = document_with(&[]);
    let mut image = Block::paragraph("");
    image.id = StableId::parse("image").expect("valid id");
    image.content.clear();
    image.kind = BlockKind::Image {
        blob_hash: "sha256:image".to_string(),
        alt_text: "quarterly-chart.png".to_string(),
        layout: ImageLayout::default(),
    };
    document.blocks.push(image);

    let html = render_document_html(
        &document,
        [RenderImage {
            hash: "sha256:image",
            media_type: "image/png",
            bytes: &[0x89, b'P', b'N', b'G'],
        }],
    );
    assert!(html.contains("alt=\"quarterly-chart.png\""), "{html}");
    assert!(html.contains("title=\"quarterly-chart.png\""), "{html}");
    assert!(
        html.contains(
            "contenteditable=\"false\" tabindex=\"0\" aria-label=\"Image: quarterly-chart.png\""
        ),
        "atomic images must be keyboard reachable and named: {html}"
    );
    assert!(
        html.contains("aria-keyshortcuts=\"Control+Shift+ArrowLeft Control+Shift+ArrowRight Control+Shift+ArrowUp Control+Shift+ArrowDown\"")
            && html.contains("aria-description=\"Use Control Shift plus arrow keys to resize this image; add Alt for larger steps. Press Escape to deselect the image.\""),
        "keyboard image resizing is discoverable to assistive technology: {html}"
    );
    assert!(
        !html.contains("figcaption"),
        "image metadata must not become a visible document caption: {html}"
    );
}

#[test]
fn an_unavailable_image_is_still_a_named_keyboard_reachable_object() {
    let mut document = document_with(&[]);
    let mut image = Block::paragraph("");
    image.id = StableId::parse("missing-image").expect("valid id");
    image.content.clear();
    image.kind = BlockKind::Image {
        blob_hash: "sha256:missing".to_string(),
        alt_text: "Diagram of the experiment".to_string(),
        layout: ImageLayout::default(),
    };
    document.blocks.push(image);

    let html = render_document_html(&document, []);
    assert!(
        html.contains("tabindex=\"0\" aria-label=\"Image unavailable: Diagram of the experiment\""),
        "{html}"
    );
    assert!(html.contains("doc-image-placeholder"), "{html}");
}

#[test]
fn image_caption_is_visible_but_alt_text_is_not_reused_as_one() {
    let mut document = document_with(&[]);
    let mut image = Block::paragraph("");
    image.id = StableId::parse("captioned-image").expect("valid id");
    image.content.clear();
    image.kind = BlockKind::Image {
        blob_hash: "sha256:image".to_string(),
        alt_text: "chart source filename.png".to_string(),
        layout: ImageLayout {
            caption: Some("Figure 1. Quarterly results".to_string()),
            ..ImageLayout::default()
        },
    };
    document.blocks.push(image);
    // This was the generated caption ID before it entered a reserved
    // namespace. It is a valid bookmark name and would otherwise duplicate
    // the `aria-describedby` target in the same image figure.
    let legacy_caption_id = "image-caption-captioned-image";
    document.bookmarks.push(Bookmark {
        id: StableId::parse("bookmark-image-caption").expect("bookmark id"),
        name: legacy_caption_id.to_string(),
        block_id: StableId::parse("captioned-image").expect("image id"),
        revision: 1,
        deleted: false,
    });
    document
        .validate()
        .expect("image bookmark source state is valid");
    let html = render_document_html(
        &document,
        [RenderImage {
            hash: "sha256:image",
            media_type: "image/png",
            bytes: &[0x89, b'P', b'N', b'G'],
        }],
    );
    assert!(
        html.contains(
            "aria-describedby=\"opendoc-image-caption:captioned-image\""
        ) && html.contains(
            "<figcaption id=\"opendoc-image-caption:captioned-image\">Figure 1. Quarterly results</figcaption>"
        ),
        "{html}"
    );
    assert_eq!(
        html.matches(&format!("id=\"{legacy_caption_id}\"")).count(),
        1,
        "the bookmark target must not duplicate the generated caption ID: {html}"
    );
    assert_eq!(
        html.matches("chart source filename.png").count(),
        3,
        // Image alt metadata appears on the image, its hover title, and the
        // atomic figure's keyboard-accessible name — never as the caption.
        "{html}"
    );
}

#[test]
fn positioned_image_projects_durable_anchor_geometry_without_becoming_a_float() {
    let mut document = document_with(&["anchor"]);
    let mut image = Block::paragraph("");
    image.id = StableId::parse("positioned-image").unwrap();
    image.content.clear();
    image.kind = BlockKind::Image {
        blob_hash: "sha256:positioned".to_string(),
        alt_text: "overlay".to_string(),
        layout: ImageLayout {
            width: Some(opendoc_core::Length::from_twips(720).unwrap()),
            height: Some(opendoc_core::Length::from_twips(360).unwrap()),
            positioned: Some(opendoc_core::PositionedImage {
                anchor: opendoc_core::PositionedImageAnchor::Block(document.blocks[0].id.clone()),
                horizontal_offset: opendoc_core::Length::from_twips(-120).unwrap(),
                vertical_offset: opendoc_core::Length::from_twips(240).unwrap(),
                layer: opendoc_core::PositionedImageLayer::InFrontOfText,
            }),
            ..Default::default()
        },
    };
    document.blocks.push(image);
    let html = render_document_html(&document, []);
    assert!(html.contains("data-positioned=\"true\""), "{html}");
    assert!(html.contains("data-position-anchor=\"block:"), "{html}");
    assert!(html.contains("data-position-x-twips=\"-120\""), "{html}");
    assert!(
        html.contains("data-position-layer=\"in-front-of-text\""),
        "{html}"
    );
    assert!(html.contains("position: absolute;"), "{html}");
    assert!(!html.contains("data-placement=\"wrap-"), "{html}");
}

#[test]
fn valued_marks_become_inline_styles_and_unsafe_values_are_dropped() {
    let mut document = document_with(&["styled"]);
    if let Inline::Text { marks, .. } = &mut document.blocks[0].content[0] {
        marks.push(Mark {
            kind: MarkKind::Color,
            value: Some("#d93025".to_string()),
            expand: MarkExpand::Both,
        });
        marks.push(Mark {
            kind: MarkKind::Size,
            value: Some("11".to_string()),
            expand: MarkExpand::Both,
        });
        marks.push(Mark {
            kind: MarkKind::Font,
            value: Some("url(evil)\"; x".to_string()),
            expand: MarkExpand::Both,
        });
        marks.push(Mark {
            kind: MarkKind::Bold,
            value: None,
            expand: MarkExpand::Both,
        });
    }
    let html = render_document_html(&document, []);
    assert!(html.contains("color:#d93025;"));
    assert!(html.contains("font-size:11pt;"));
    assert!(!html.contains("evil"));
    assert!(html.contains("mark-bold"));
}

#[test]
fn consecutive_list_items_form_nested_numbered_lists() {
    let mut document = document_with(&["one", "two", "sub", "three", "after"]);
    let list = StableId::new("list");
    for (index, level) in [(0, 0u8), (1, 0), (2, 1), (3, 0)] {
        document.blocks[index].kind = BlockKind::ListItem {
            list_id: list.clone(),
            level,
            kind: opendoc_core::ListKind::Ordered,
        };
    }
    let html = render_document_html(&document, []);
    assert_eq!(html.matches("<ol").count(), 2, "{html}");
    assert!(html.contains("value=\"1\""));
    assert!(html.contains("value=\"3\""));
    assert!(html.contains("<li class=\"doc-block doc-list-item\""));
    assert!(html.ends_with("after</span></p>"));
    assert_eq!(html.matches("<li").count(), html.matches("</li>").count());
    assert_eq!(html.matches("<ol").count(), html.matches("</ol>").count());
}

#[test]
fn footnotes_are_numbered_links_are_sanitized_and_empty_blocks_get_a_break() {
    let mut document = document_with(&["see", ""]);
    let footnote = opendoc_core::Footnote {
        id: StableId::new("footnote"),
        revision: 1,
        body: vec![Inline::text("note body")],
        deleted: false,
    };
    document.blocks[0].content.push(Inline::FootnoteRef {
        id: StableId::new("ref"),
        footnote_id: footnote.id.clone(),
    });
    document.blocks[0].content.push(Inline::Link {
        id: StableId::new("link"),
        text: "x".to_string(),
        href: "javascript:alert(1)".to_string(),
        marks: Vec::new(),
    });
    document.footnotes.push(footnote);
    let html = render_document_html(&document, []);
    assert!(html.contains("footnote-ref\" data-inline-id"));
    assert!(html.contains(">1</sup>"));
    assert!(html.contains("href=\"#\""));
    assert!(html.contains("<br data-caret-anchor=\"true\">"));
    let notes = render_footnotes_html(&document, []);
    assert!(notes.contains("value=\"1\""));
    assert!(notes.contains("note body"));
}

#[test]
fn comments_and_suggestions_are_marked_on_runs() {
    let mut document = document_with(&["alpha", "beta"]);
    let first = inline_id(&document.blocks[0].content[0]).clone();
    let second = inline_id(&document.blocks[1].content[0]).clone();
    document.comments.push(opendoc_core::CommentThread {
        id: StableId::new("thread"),
        anchor: Anchor::TextRange(TextRange {
            start: first.clone(),
            end: second.clone(),
        }),
        comments: Vec::new(),
        state: opendoc_core::CommentThreadState::Open,
        resolved_by: None,
        resolved_at_ms: None,
        action_assignee: None,
        action_due_at_ms: None,
        action_completed_by: None,
        action_completed_at_ms: None,
        reactions: Vec::new(),
        deleted: false,
    });
    document.suggestions.push(Suggestion {
        id: StableId::new("suggestion"),
        author: "Editor".to_string(),
        kind: SuggestionKind::Insert {
            anchor: Anchor::TextRange(TextRange {
                start: first.clone(),
                end: first.clone(),
            }),
            content: vec![Inline::text(" inserted")],
        },
        state: SuggestionState::Proposed,
        provenance: Vec::new(),
    });
    let html = render_document_html(&document, []);
    assert_eq!(html.matches("has-comment").count(), 2);
    assert!(html.contains("suggested-insert"));
    assert!(html.contains(" inserted</span>"));
}

#[test]
fn footnote_numbering_matches_the_layouts() {
    // The number is *drawn*, so its width decides where a line breaks, and
    // `opendoc-layout` therefore has to know it. Neither crate can call the
    // other's numbering (layout does not depend on render, and render's is
    // written into HTML as it goes), so the rule exists twice — and this is
    // the test that keeps the two copies honest. They are deliberately
    // declared in the reverse of the order they are referenced in, because
    // numbering by declaration and numbering by first reference agree on any
    // document where the two orders coincide.
    let mut document = document_with(&["first", "second", "third"]);
    let ids = ["note-c", "note-a", "note-b"];
    for (block, id) in document.blocks.iter_mut().zip(ids) {
        block.content.push(Inline::FootnoteRef {
            id: StableId::new("inline"),
            footnote_id: StableId::parse(id).expect("valid id"),
        });
    }
    document.footnotes = ["note-a", "note-b", "note-c"]
        .into_iter()
        .map(|id| opendoc_core::Footnote {
            id: StableId::parse(id).expect("valid id"),
            revision: 1,
            body: vec![Inline::text(format!("body of {id}"))],
            deleted: false,
        })
        .collect();

    let numbers = opendoc_layout::footnote_numbers(&document);
    assert_eq!(numbers.len(), 3);
    let html = render_document_html(&document, []);
    for (id, number) in &numbers {
        assert!(
            html.contains(&format!("data-footnote-id=\"{id}\">{number}</sup>")),
            "the renderer numbered {id} differently from the layout ({number}): {html}"
        );
    }
    // And the footnote list is numbered the same way.
    let list = render_footnotes_html(&document, []);
    for (id, number) in &numbers {
        assert!(
            list.contains(&format!("value=\"{number}\" data-footnote-id=\"{id}\"")),
            "the footnote list numbered {id} differently from the layout ({number}): {list}"
        );
    }
    // Reference order, not declaration order.
    assert_eq!(numbers.get("note-c"), Some(&1));
}
