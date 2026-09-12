use crate::test_support::assert_export_error_contains;
use crate::*;
use opendoc_core::ListKind;
use opendoc_core::{
    Anchor, BibliographyReference, Block, BlockKind, BlockProperties, CitationGroup, CitationItem,
    CitationPlacement, CitationSource, CitationSourceFormat, CitationSummary, Comment,
    CommentThread, Document, Equation, EquationSourceFormat, Footnote, Inline, MarkKind, StableId,
    Suggestion, SuggestionKind, SuggestionState, TextRange,
};
use serde_json::Value;

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
        deleted: false,
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
    assert_eq!(
        value["opendocComments"][0]["comments"][0]["body"][0]["textRun"]["content"],
        "Exported comment"
    );
    assert_eq!(value["opendocSuggestions"][0]["id"], "suggest-export");
    assert_eq!(value["opendocSuggestions"][0]["state"], "accepted");
    assert_eq!(value["opendocSuggestions"][0]["kind"]["type"], "delete");
}
