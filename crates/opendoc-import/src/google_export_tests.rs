use crate::test_support::assert_export_error_contains;
use crate::*;
use opendoc_core::{
    Anchor, BibliographyReference, Block, BlockKind, BlockProperties, Bookmark, CitationGroup,
    CitationItem, CitationPlacement, CitationSource, CitationSourceFormat, CitationSummary,
    Comment, CommentHistoryEntry, CommentThread, Document, Equation, EquationSourceFormat,
    Footnote, Inline, MarkKind, StableId, Suggestion, SuggestionKind, SuggestionState, TextRange,
};
use opendoc_core::{
    ImageLayout, Length, ListKind, PositionedImage, PositionedImageAnchor, PositionedImageLayer,
};
use serde_json::Value;

#[test]
fn orphaned_comment_anchor_round_trips_without_becoming_a_live_target() {
    let source = Anchor::Orphaned {
        quote: "removed wording".to_string(),
        context: "Paragraph before deletion".to_string(),
        warning: "The comment target was deleted".to_string(),
    };
    let value = crate::google_export::export_google_anchor(&source);
    assert_eq!(value["type"], "orphaned");
    let imported = crate::google_import::import_google_anchor(&value).unwrap();
    assert_eq!(imported, source);
}

#[test]
fn bookmark_round_trips_through_the_explicit_opendoc_google_extension() {
    let mut document = Document::new("Bookmarks");
    let block = Block::paragraph("target");
    let target = block.id.clone();
    document.blocks.push(block);
    document.bookmarks.push(Bookmark {
        id: StableId::parse("bookmark-intro").unwrap(),
        name: "introduction".to_string(),
        block_id: target,
        revision: 1,
        deleted: false,
    });
    let (bytes, warnings) = export_google_docs_json_with_warnings(&document).unwrap();
    assert!(warnings
        .iter()
        .any(|warning| warning.code == "google-export-bookmarks-opendoc-extension"));
    let imported = import_google_docs_json("Bookmarks", &bytes).unwrap();
    assert_eq!(imported.document.bookmarks, document.bookmarks);
}

#[test]
fn paragraph_style_suggestion_round_trips_through_google_review_extension() {
    // `block_style_change` is OpenDoc review vocabulary, rather than a
    // Google-native tracked-change resource.  Its expected style is the
    // acceptance precondition, so losing either value would turn a review
    // proposal into an unconditional source edit on re-import.
    let mut document = Document::new("Paragraph style review");
    let mut block = Block::paragraph("A reviewed paragraph");
    block.id = StableId::parse("style-target").unwrap();
    document.blocks.push(block);
    document.suggestions.push(Suggestion {
        id: StableId::parse("style-suggestion").unwrap(),
        author: "Ada".to_string(),
        kind: SuggestionKind::ParagraphStyleChange {
            block_id: StableId::parse("style-target").unwrap(),
            expected: opendoc_core::ParagraphStyle::Paragraph,
            proposed: opendoc_core::ParagraphStyle::Heading { level: 2 },
        },
        state: SuggestionState::Proposed,
        provenance: vec!["created in suggest mode".to_string()],
    });

    let bytes = export_google_docs_json(&document).expect("export succeeds");
    let exported: Value = serde_json::from_slice(&bytes).expect("export is JSON");
    let kind = &exported["opendocSuggestions"][0]["kind"];
    assert_eq!(kind["type"], "block_style_change");
    assert_eq!(kind["blockId"], "style-target");

    let reimported = import_google_docs_json("Paragraph style review", &bytes)
        .expect("review extension re-imports");
    assert_eq!(reimported.document.suggestions, document.suggestions);
}

#[test]
fn block_replace_suggestion_round_trips_its_source_precondition() {
    let mut document = Document::new("Block replacement review");
    let mut expected = Block::paragraph("Reviewed source");
    expected.id = StableId::parse("replace-target").unwrap();
    let mut replacement = Block::paragraph("Proposed replacement");
    replacement.id = expected.id.clone();
    document.blocks.push(expected.clone());
    document.suggestions.push(Suggestion {
        id: StableId::parse("replace-suggestion").unwrap(),
        author: "Ada".to_string(),
        kind: SuggestionKind::BlockReplace {
            block_id: expected.id.clone(),
            expected: Box::new(expected.clone()),
            replacement: Box::new(replacement.clone()),
        },
        state: SuggestionState::Proposed,
        provenance: vec!["created in suggest mode".to_string()],
    });

    let bytes = export_google_docs_json(&document).expect("export succeeds");
    let exported: Value = serde_json::from_slice(&bytes).expect("export is JSON");
    let kind = &exported["opendocSuggestions"][0]["kind"];
    assert_eq!(kind["type"], "block_replace");
    assert_eq!(kind["expected"]["id"], "replace-target");
    assert_eq!(kind["replacement"]["id"], "replace-target");

    let reimported = import_google_docs_json("Block replacement review", &bytes)
        .expect("review extension re-imports");
    assert_eq!(reimported.document.suggestions, document.suggestions);
}

#[test]
fn date_chip_round_trips_through_native_google_date_element() {
    let mut document = Document::new("Dates");
    document.blocks.push(Block {
        id: StableId::parse("date-paragraph").unwrap(),
        kind: BlockKind::Paragraph,
        content: vec![Inline::DateChip {
            id: StableId::parse("due-date").unwrap(),
            date: "2028-02-29".to_string(),
        }],
        properties: BlockProperties::default(),
    });
    let bytes = export_google_docs_json(&document).unwrap();
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        value["body"]["content"][0]["paragraph"]["elements"][0]["dateElement"]
            ["dateElementProperties"]["timestamp"],
        "2028-02-29T00:00:00Z"
    );
    assert_eq!(
        value["body"]["content"][0]["paragraph"]["elements"][0]["dateElement"]
            ["dateElementProperties"]["dateFormat"],
        "DATE_FORMAT_ISO8601"
    );
    let imported = import_google_docs_json("Dates", &bytes).unwrap();
    assert!(
        matches!(&imported.document.blocks[0].content[0], Inline::DateChip { id, date } if id.as_str() == "due-date" && date == "2028-02-29")
    );
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
}

#[test]
fn title_and_subtitle_export_as_google_named_styles() {
    let mut document = Document::new("Named styles");
    for (kind, text) in [
        (BlockKind::Title, "A title"),
        (BlockKind::Subtitle, "A subtitle"),
    ] {
        document.blocks.push(Block {
            id: StableId::new("block"),
            kind,
            content: vec![Inline::text(text)],
            properties: BlockProperties::default(),
        });
    }
    let bytes = export_google_docs_json(&document).unwrap();
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        value["body"]["content"][0]["paragraph"]["paragraphStyle"]["namedStyleType"],
        "TITLE"
    );
    assert_eq!(
        value["body"]["content"][1]["paragraph"]["paragraphStyle"]["namedStyleType"],
        "SUBTITLE"
    );
    let imported = import_google_docs_json("Named styles", &bytes).unwrap();
    assert!(matches!(imported.document.blocks[0].kind, BlockKind::Title));
    assert!(matches!(
        imported.document.blocks[1].kind,
        BlockKind::Subtitle
    ));
}

#[test]
fn malformed_structured_payload_export_aborts_instead_of_emitting_invalid_extensions() {
    let mut document = Document::new("Bad Structured Export");
    document.blocks.push(Block {
        id: StableId::new("link-block"),
        kind: BlockKind::Paragraph,
        content: vec![Inline::Link {
            id: StableId::new("link"),
            text: "link".to_string(),
            href: String::new(),
            marks: Vec::new(),
        }],
        properties: BlockProperties::default(),
    });
    assert_export_error_contains(&document, "link href is empty");

    document.blocks[0] = Block {
        id: StableId::new("equation-block"),
        kind: BlockKind::EquationBlock {
            equation: Equation {
                id: StableId::new("eq"),
                source_format: EquationSourceFormat::LatexLike,
                source: " ".to_string(),
            },
        },
        content: Vec::new(),
        properties: BlockProperties::default(),
    };
    assert_export_error_contains(&document, "equation source");

    document.blocks[0] = Block {
        id: StableId::new("image"),
        kind: BlockKind::Image {
            blob_hash: "not-a-hash".to_string(),
            alt_text: "image".to_string(),
            layout: Default::default(),
        },
        content: Vec::new(),
        properties: BlockProperties::default(),
    };
    assert_export_error_contains(&document, "image blob");

    document.blocks.clear();
    document.comments.push(CommentThread {
        id: StableId::new("thread"),
        anchor: Anchor::Document,
        comments: vec![Comment {
            id: StableId::new("comment"),
            author: "Reviewer".to_string(),
            body: vec![Inline::Mention {
                id: StableId::new("mention"),
                label: " ".to_string(),
            }],
            created_at_ms: 1,
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
    });
    assert_export_error_contains(&document, "mention label is empty");

    document.comments.clear();
    document.suggestions.push(Suggestion {
        id: StableId::new("suggestion"),
        author: "Reviewer".to_string(),
        kind: SuggestionKind::Insert {
            anchor: Anchor::Document,
            content: vec![Inline::Equation {
                id: StableId::new("equation"),
                equation: Equation {
                    id: StableId::new("eq"),
                    source_format: EquationSourceFormat::LatexLike,
                    source: String::new(),
                },
            }],
        },
        state: SuggestionState::Proposed,
        provenance: Vec::new(),
    });
    assert_export_error_contains(&document, "equation source is empty");

    document.suggestions.clear();
    document.blocks.push(Block {
        id: StableId::new("mark-block"),
        kind: BlockKind::Paragraph,
        content: vec![Inline::Text {
            id: StableId::new("text"),
            text: "marked".to_string(),
            marks: vec![mark(MarkKind::Color, None)],
        }],
        properties: BlockProperties::default(),
    });
    assert_export_error_contains(&document, "mark value is missing");

    document.blocks[0].content = vec![Inline::Text {
        id: StableId::new("text"),
        text: "marked".to_string(),
        marks: vec![mark(MarkKind::Bold, Some("true".to_string()))],
    }];
    assert_export_error_contains(&document, "boolean mark has value");

    document.blocks[0].content = vec![Inline::Text {
        id: StableId::new("text"),
        text: "marked".to_string(),
        marks: vec![mark(MarkKind::Size, Some("large".to_string()))],
    }];
    assert_export_error_contains(&document, "size mark value is invalid");

    document.blocks[0].content.clear();
    document
        .citation_database
        .upsert_reference(BibliographyReference {
            id: StableId::new("bad-ref"),
            revision: 1,
            source: CitationSource {
                format: CitationSourceFormat::CitumNative,
                bytes: b"title: bad".to_vec(),
            },
            summary: CitationSummary {
                title: "Bad".to_string(),
                authors: vec![" ".to_string()],
                issued: None,
                doi: None,
                url: None,
            },
            deleted: false,
        });
    assert_export_error_contains(&document, "bibliography summary field is empty");

    document.citation_database.references[0].summary.authors = vec!["Doe".to_string()];
    document.citation_database.upsert_citation(CitationGroup {
        id: StableId::new("bad-citation"),
        revision: 1,
        items: vec![CitationItem {
            reference_id: StableId::new("bad-ref"),
            locator: None,
            label: None,
            prefix: Some(" ".to_string()),
            suffix: None,
            suppress_author: false,
        }],
        placement: CitationPlacement::Inline,
        rendered_cache: None,
        deleted: false,
    });
    assert_export_error_contains(&document, "citation item field is empty");
}

#[test]
fn google_export_refuses_unsafe_navigation_link_schemes() {
    for href in [
        "javascript:alert(1)",
        "data:text/html,unsafe",
        "https://example.test/\u{7f}",
    ] {
        let mut document = Document::new("Unsafe link");
        document.blocks.push(Block {
            id: StableId::new("link-block"),
            kind: BlockKind::Paragraph,
            content: vec![Inline::Link {
                id: StableId::new("link"),
                text: "unsafe".to_string(),
                href: href.to_string(),
                marks: Vec::new(),
            }],
            properties: BlockProperties::default(),
        });
        assert_export_error_contains(&document, "unsafe navigation scheme");
    }

    let mut document = Document::new("Whitespace link");
    document.blocks.push(Block {
        id: StableId::new("link-block-whitespace"),
        kind: BlockKind::Paragraph,
        content: vec![Inline::Link {
            id: StableId::new("link-whitespace"),
            text: "whitespace".to_string(),
            href: " https://example.test".to_string(),
            marks: Vec::new(),
        }],
        properties: BlockProperties::default(),
    });
    assert_export_error_contains(&document, "surrounding whitespace");
}

#[test]
fn google_export_refuses_unsafe_rich_link_navigation_schemes() {
    for href in ["javascript:alert(1)", "data:text/html,unsafe"] {
        let mut document = Document::new("Unsafe rich link");
        document.blocks.push(Block {
            id: StableId::new("rich-link-block"),
            kind: BlockKind::Paragraph,
            content: vec![Inline::GoogleRichLinkChip {
                id: StableId::new("rich-link"),
                label: "unsafe".to_string(),
                href: href.to_string(),
                rich_link_id: None,
                mime_type: None,
            }],
            properties: BlockProperties::default(),
        });
        assert_export_error_contains(&document, "unsafe navigation scheme");
    }
}

#[test]
fn google_image_export_names_the_required_authorised_resource_upload() {
    let mut document = Document::new("Image");
    document.blocks.push(Block {
        id: StableId::new("image"),
        kind: BlockKind::Image {
            blob_hash: "sha256:abc123".to_string(),
            alt_text: "Image".to_string(),
            layout: Default::default(),
        },
        content: Vec::new(),
        properties: BlockProperties::default(),
    });
    let (_, warnings) = export_google_docs_json_with_warnings(&document).unwrap();
    assert!(warnings
        .iter()
        .any(|warning| warning.code == "google-image-resource-required"
            && warning.message.contains("authorised API transport")));
}

#[test]
fn google_positioned_image_extension_preserves_layout_without_claiming_native_upload() {
    let mut document = Document::new("Positioned image");
    document.blocks.push(Block {
        id: StableId::new("image"),
        kind: BlockKind::Image {
            blob_hash: "sha256:abc123".to_string(),
            alt_text: "Image".to_string(),
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
    });
    let (bytes, warnings) = export_google_docs_json_with_warnings(&document).unwrap();
    let exported: Value = serde_json::from_slice(&bytes).unwrap();
    let layout = &exported["body"]["content"][0]["opendocImage"]["layout"];
    assert_eq!(layout["positioned"]["anchor"], "PageContent");
    assert_eq!(layout["positioned"]["horizontal_offset"], -240);
    assert_eq!(layout["positioned"]["vertical_offset"], 480);
    assert_eq!(layout["positioned"]["layer"], "BehindText");
    assert!(warnings
        .iter()
        .any(|warning| warning.code == "google-export-positioned-image-unmapped"));
    assert!(warnings
        .iter()
        .any(|warning| warning.code == "google-image-resource-required"));
}

#[test]
fn google_docs_export_validates_source_before_serializing() {
    let mut document = Document::new("Export Source Guard");
    document.title = " Export Source Guard ".to_string();
    document.blocks.push(Block::paragraph("Body"));

    assert_export_error_contains(&document, "title has surrounding whitespace");
}

#[test]
fn export_google_docs_represents_v0_subset() {
    let mut document = Document::new("Exported");
    document.blocks.push(Block {
        id: StableId::new("block"),
        kind: BlockKind::Heading { level: 1 },
        content: vec![opendoc_core::Inline::Text {
            id: StableId::new("text"),
            text: "Title".to_string(),
            marks: vec![mark(MarkKind::Bold, None)],
        }],
        properties: BlockProperties::default(),
    });
    document.blocks.push(Block {
        id: StableId::new("block"),
        kind: BlockKind::ListItem {
            list_id: StableId::parse("list-export").unwrap(),
            level: 1,
            kind: ListKind::Bullet,
        },
        content: vec![opendoc_core::Inline::Link {
            id: StableId::new("link"),
            text: "Link".to_string(),
            href: "https://example.invalid".to_string(),
            marks: Vec::new(),
        }],
        properties: BlockProperties::default(),
    });
    document.blocks.push(Block {
        id: StableId::new("block"),
        kind: BlockKind::Paragraph,
        content: vec![
            Inline::Text {
                id: StableId::new("text"),
                text: "With footnote".to_string(),
                marks: Vec::new(),
            },
            Inline::FootnoteRef {
                id: StableId::new("footnote-ref"),
                footnote_id: StableId::parse("fn-export").unwrap(),
            },
        ],
        properties: BlockProperties::default(),
    });
    document.blocks.push(Block {
        id: StableId::parse("equation-block-export").unwrap(),
        kind: BlockKind::EquationBlock {
            equation: Equation {
                id: StableId::parse("eq-block-export").unwrap(),
                source_format: EquationSourceFormat::LatexLike,
                source: "\\int_0^1 x^2 dx".to_string(),
            },
        },
        content: Vec::new(),
        properties: BlockProperties::default(),
    });
    document.blocks.push(Block {
        id: StableId::new("block"),
        kind: BlockKind::Paragraph,
        content: vec![
            Inline::Text {
                id: StableId::new("text"),
                text: "Equation".to_string(),
                marks: Vec::new(),
            },
            Inline::Equation {
                id: StableId::parse("inline-eq-export").unwrap(),
                equation: Equation {
                    id: StableId::parse("eq-inline-export").unwrap(),
                    source_format: EquationSourceFormat::LatexLike,
                    source: "E=mc^2".to_string(),
                },
            },
        ],
        properties: BlockProperties::default(),
    });
    document.blocks.push(Block {
        id: StableId::new("block"),
        kind: BlockKind::Paragraph,
        content: vec![
            Inline::Text {
                id: StableId::new("text"),
                text: "Mention ".to_string(),
                marks: Vec::new(),
            },
            Inline::Mention {
                id: StableId::parse("mention-export").unwrap(),
                label: "@Ada".to_string(),
            },
        ],
        properties: BlockProperties::default(),
    });
    document.blocks.push(Block {
        id: StableId::new("block"),
        kind: BlockKind::Paragraph,
        content: vec![
            Inline::Text {
                id: StableId::new("text"),
                text: "Cited ".to_string(),
                marks: Vec::new(),
            },
            Inline::Citation {
                id: StableId::new("citation-label"),
                citation_id: StableId::parse("cite-export").unwrap(),
                rendered_cache: Some("(Doe 2020)".to_string()),
            },
        ],
        properties: BlockProperties::default(),
    });
    document.blocks.push(Block {
        id: StableId::parse("image-export").unwrap(),
        kind: BlockKind::Image {
            blob_hash: "sha256:abc123".to_string(),
            alt_text: "Exported figure".to_string(),
            layout: Default::default(),
        },
        content: Vec::new(),
        properties: BlockProperties::default(),
    });
    document.footnotes.push(Footnote {
        id: StableId::parse("fn-export").unwrap(),
        revision: 1,
        body: vec![Inline::Text {
            id: StableId::new("text"),
            text: "Exported footnote".to_string(),
            marks: Vec::new(),
        }],
        deleted: false,
    });
    document
        .citation_database
        .upsert_reference(BibliographyReference {
            id: StableId::parse("ref-export").unwrap(),
            revision: 2,
            source: CitationSource {
                format: CitationSourceFormat::CitumNative,
                bytes: b"exported citation payload".to_vec(),
            },
            summary: CitationSummary {
                title: "Exported Article".to_string(),
                authors: vec!["Doe".to_string()],
                issued: Some("2020".to_string()),
                doi: None,
                url: None,
            },
            deleted: false,
        });
    document.citation_database.upsert_citation(CitationGroup {
        id: StableId::parse("cite-export").unwrap(),
        revision: 3,
        items: vec![CitationItem {
            reference_id: StableId::parse("ref-export").unwrap(),
            locator: Some("12".to_string()),
            label: Some("page".to_string()),
            prefix: None,
            suffix: None,
            suppress_author: false,
        }],
        placement: CitationPlacement::Inline,
        rendered_cache: Some("(Doe 2020)".to_string()),
        deleted: false,
    });
    document.citation_database.upsert_citation(CitationGroup {
        id: StableId::parse("cite-footnote-export").unwrap(),
        revision: 4,
        items: vec![CitationItem {
            reference_id: StableId::parse("ref-export").unwrap(),
            locator: Some("44".to_string()),
            label: Some("page".to_string()),
            prefix: None,
            suffix: None,
            suppress_author: false,
        }],
        placement: CitationPlacement::Footnote {
            footnote_id: StableId::parse("fn-export").unwrap(),
        },
        rendered_cache: Some("(Doe 2020, 44)".to_string()),
        deleted: false,
    });
    document.comments.push(CommentThread {
        id: StableId::parse("thread-export").unwrap(),
        anchor: Anchor::TextRange(TextRange {
            start: StableId::parse("text-start").unwrap(),
            end: StableId::parse("text-end").unwrap(),
        }),
        comments: vec![Comment {
            id: StableId::parse("comment-export").unwrap(),
            author: "Ada".to_string(),
            body: vec![Inline::Text {
                id: StableId::new("text"),
                text: "Exported comment".to_string(),
                marks: vec![mark(MarkKind::Italic, None)],
            }],
            created_at_ms: 22,
            deleted: false,
        }],
        state: opendoc_core::CommentThreadState::Open,
        resolved_by: None,
        resolved_at_ms: None,
        action_assignee: Some("Ada".to_string()),
        action_due_at_ms: Some(1_700_000_000_000),
        action_completed_by: Some("Reviewer".to_string()),
        action_completed_at_ms: Some(23),
        reactions: Vec::new(),
        deleted: false,
    });
    document.comment_history.push(CommentHistoryEntry {
        thread_id: StableId::parse("thread-export").unwrap(),
        comment_id: StableId::parse("comment-export").unwrap(),
        kind: "edited".to_string(),
        actor: "Grace".to_string(),
        at_ms: 24,
        previous_body: Some(vec![Inline::Text {
            id: StableId::new("text"),
            text: "Earlier exported comment".to_string(),
            marks: vec![mark(MarkKind::Bold, None)],
        }]),
    });
    document.comment_history.push(CommentHistoryEntry {
        thread_id: StableId::parse("thread-export").unwrap(),
        comment_id: StableId::parse("comment-export").unwrap(),
        kind: "restored".to_string(),
        actor: "Reviewer".to_string(),
        at_ms: 25,
        previous_body: None,
    });
    document.suggestions.push(Suggestion {
        id: StableId::parse("suggest-export").unwrap(),
        author: "Grace".to_string(),
        kind: SuggestionKind::Delete {
            range: TextRange {
                start: StableId::parse("text-start").unwrap(),
                end: StableId::parse("text-end").unwrap(),
            },
        },
        state: SuggestionState::Accepted,
        provenance: vec!["accepted during review".to_string()],
    });
    let bytes = export_google_docs_json(&document).unwrap();
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["title"], "Exported");
    assert_eq!(
        value["body"]["content"][0]["paragraph"]["paragraphStyle"]["namedStyleType"],
        "HEADING_1"
    );
    assert_eq!(
        value["body"]["content"][1]["paragraph"]["bullet"]["listId"],
        "list-export"
    );
    assert_eq!(
        value["body"]["content"][1]["paragraph"]["elements"][0]["textRun"]["textStyle"]["link"]
            ["url"],
        "https://example.invalid"
    );
    assert_eq!(
        value["body"]["content"][2]["paragraph"]["elements"][1]["footnoteReference"]["footnoteId"],
        "fn-export"
    );
    assert_eq!(
        value["footnotes"]["fn-export"]["content"][0]["paragraph"]["elements"][0]["textRun"]
            ["content"],
        "Exported footnote"
    );
    assert_eq!(
        value["body"]["content"][3]["opendocEquationBlock"]["blockId"],
        "equation-block-export"
    );
    assert_eq!(
        value["body"]["content"][3]["opendocEquationBlock"]["equationId"],
        "eq-block-export"
    );
    assert_eq!(
        value["body"]["content"][3]["opendocEquationBlock"]["sourceFormat"],
        "latex-like"
    );
    assert_eq!(
        value["body"]["content"][3]["opendocEquationBlock"]["source"],
        "\\int_0^1 x^2 dx"
    );
    assert_eq!(
        value["body"]["content"][4]["paragraph"]["elements"][1]["opendocEquation"]["inlineId"],
        "inline-eq-export"
    );
    assert_eq!(
        value["body"]["content"][4]["paragraph"]["elements"][1]["opendocEquation"]["equationId"],
        "eq-inline-export"
    );
    assert_eq!(
        value["body"]["content"][4]["paragraph"]["elements"][1]["opendocEquation"]["sourceFormat"],
        "latex-like"
    );
    assert_eq!(
        value["body"]["content"][4]["paragraph"]["elements"][1]["opendocEquation"]["source"],
        "E=mc^2"
    );
    assert_eq!(
        value["body"]["content"][5]["paragraph"]["elements"][1]["opendocMention"]["inlineId"],
        "mention-export"
    );
    assert_eq!(
        value["body"]["content"][5]["paragraph"]["elements"][1]["opendocMention"]["label"],
        "@Ada"
    );
    assert_eq!(
        value["body"]["content"][6]["paragraph"]["elements"][1]["opendocCitation"]["citationId"],
        "cite-export"
    );
    assert_eq!(
        value["body"]["content"][7]["opendocImage"]["blockId"],
        "image-export"
    );
    assert_eq!(
        value["body"]["content"][7]["opendocImage"]["blobHash"],
        "sha256:abc123"
    );
    assert_eq!(
        value["body"]["content"][7]["opendocImage"]["altText"],
        "Exported figure"
    );
    assert_eq!(
        value["opendocCitations"]["references"][0]["id"],
        "ref-export"
    );
    assert_eq!(
        value["opendocCitations"]["references"][0]["summary"]["title"],
        "Exported Article"
    );
    assert_eq!(
        value["opendocCitations"]["groups"][0]["items"][0]["referenceId"],
        "ref-export"
    );
    assert_eq!(
        value["opendocCitations"]["groups"][0]["items"][0]["suppressAuthor"],
        false
    );
    let groups = value["opendocCitations"]["groups"].as_array().unwrap();
    let footnote_group = groups
        .iter()
        .find(|group| group["id"] == "cite-footnote-export")
        .unwrap();
    assert_eq!(footnote_group["placement"], "footnote");
    assert_eq!(footnote_group["footnoteId"], "fn-export");
    assert_eq!(value["opendocComments"][0]["id"], "thread-export");
    assert_eq!(value["opendocComments"][0]["actionAssignee"], "Ada");
    assert_eq!(
        value["opendocComments"][0]["actionDueAtMs"],
        1_700_000_000_000u64
    );
    assert_eq!(value["opendocComments"][0]["actionCompletedBy"], "Reviewer");
    assert_eq!(value["opendocComments"][0]["actionCompletedAtMs"], 23);
    assert_eq!(
        value["opendocCommentHistory"][0]["threadId"],
        "thread-export"
    );
    assert_eq!(
        value["opendocCommentHistory"][0]["commentId"],
        "comment-export"
    );
    assert_eq!(value["opendocCommentHistory"][0]["kind"], "edited");
    assert_eq!(value["opendocCommentHistory"][0]["actor"], "Grace");
    assert_eq!(value["opendocCommentHistory"][0]["atMs"], 24);
    assert_eq!(
        value["opendocCommentHistory"][0]["previousBody"][0]["textRun"]["content"],
        "Earlier exported comment"
    );
    assert!(value["opendocCommentHistory"][1]["previousBody"].is_null());
    assert_eq!(
        value["opendocComments"][0]["comments"][0]["body"][0]["textRun"]["content"],
        "Exported comment"
    );
    assert_eq!(value["opendocSuggestions"][0]["id"], "suggest-export");
    assert_eq!(value["opendocSuggestions"][0]["state"], "accepted");
    assert_eq!(value["opendocSuggestions"][0]["kind"]["type"], "delete");
}

#[test]
fn google_export_names_endnote_flattening_to_footnotes() {
    let endnote_id = StableId::parse("endnote-export").unwrap();
    let mut document = Document::new("Endnotes");
    document.blocks.push(Block {
        id: StableId::parse("endnote-block").unwrap(),
        kind: BlockKind::Paragraph,
        content: vec![Inline::FootnoteRef {
            id: StableId::parse("endnote-ref").unwrap(),
            footnote_id: endnote_id.clone(),
        }],
        properties: BlockProperties::default(),
    });
    document.footnotes.push(Footnote {
        id: endnote_id.clone(),
        revision: 1,
        body: vec![Inline::text("endnote body")],
        deleted: false,
    });
    document.endnote_ids.insert(endnote_id);

    let (bytes, warnings) = export_google_docs_json_with_warnings(&document).unwrap();
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(
        value["footnotes"]["endnote-export"].is_object(),
        "Google export must retain the note body"
    );
    assert!(
        warnings
            .iter()
            .any(|warning| warning.code == "google-export-endnotes-as-footnotes"),
        "endnote placement loss must be named: {warnings:?}"
    );
}

#[test]
fn export_google_docs_preserves_ordered_list_start_in_the_definition() {
    let list_id = StableId::parse("continued-list").unwrap();
    let mut document = Document::new("Continued list");
    document.blocks.push(Block {
        id: StableId::new("continued-block"),
        kind: BlockKind::ListItem {
            list_id: list_id.clone(),
            level: 0,
            kind: ListKind::Ordered,
        },
        content: vec![Inline::text("Seven")],
        properties: BlockProperties::default(),
    });
    document
        .list_properties
        .entry(list_id)
        .or_default()
        .ordered_starts
        .insert(0, 7);

    let bytes = export_google_docs_json(&document).unwrap();
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        value["lists"]["continued-list"]["listProperties"]["nestingLevels"][0]["startNumber"],
        7
    );
}
