use crate::context::inline_id;
use crate::footnotes::render_footnotes_html;
use crate::test_support::*;
use crate::*;
use opendoc_core::{MarkExpand, TextRange};

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
