//! The per-fragment projection of the body.
//!
//! One property carries the whole feature: the fragments compose to exactly
//! the body `render_document` produces. A consumer that applies fragments one
//! at a time is then applying the same markup it would have applied whole,
//! and a block it leaves alone is a block whose markup did not change. If the
//! composition ever stopped being exact, that reasoning would be false and a
//! per-block DOM update could silently drop an edit — which is why this file
//! checks it over a document holding every construct the renderer has, not
//! over a paragraph.

use crate::test_support::*;
use crate::*;
use opendoc_core::{Alignment, BlockProperty, ListKind, MarkExpand, TextRange};

/// Elements the renderer writes without a closing tag. Used by the balance
/// checker below; a tag missing from here would make the checker reject a
/// perfectly good fragment, so it fails loudly rather than silently.
const VOID_TAGS: &[&str] = &["br", "img", "col", "input", "hr", "wbr", "meta", "source"];

/// A document with one of everything: headings, a paragraph carrying every
/// typed block property, three adjacent list runs (ordered with a nested
/// level, bulleted, checklist), a table whose cells hold a paragraph and a
/// list, an image, a block equation, an inline equation, a footnote
/// reference, a link, a comment thread spanning two blocks, an insert
/// suggestion and an explicit page break.
fn every_construct() -> Document {
    let mut document = document_with(&[
        "intro",
        "heading",
        "formatted",
        "one",
        "two",
        "nested",
        "three",
        "bullet a",
        "bullet b",
        "todo",
        "after the lists",
        "before the table",
        "tail",
    ]);

    document.blocks[1].kind = BlockKind::Heading { level: 2 };

    let properties = &mut document.blocks[2].properties;
    properties.set(BlockProperty::Alignment(Alignment::Center));
    properties.set(BlockProperty::SpaceBefore(
        opendoc_core::Length::from_twips(300).unwrap(),
    ));
    properties.set(BlockProperty::SpaceAfter(
        opendoc_core::Length::from_twips(120).unwrap(),
    ));
    properties.set(BlockProperty::IndentStart(
        opendoc_core::Length::from_twips(720).unwrap(),
    ));

    let ordered = StableId::new("list-ordered");
    for (index, level) in [(3usize, 0u8), (4, 0), (5, 1), (6, 0)] {
        document.blocks[index].kind = BlockKind::ListItem {
            list_id: ordered.clone(),
            level,
            kind: ListKind::Ordered,
        };
    }
    let bullets = StableId::new("list-bullets");
    for index in [7usize, 8] {
        document.blocks[index].kind = BlockKind::ListItem {
            list_id: bullets.clone(),
            level: 0,
            kind: ListKind::Bullet,
        };
    }
    document.blocks[9].kind = BlockKind::ListItem {
        list_id: StableId::new("list-checks"),
        level: 0,
        kind: ListKind::Checklist { checked: true },
    };

    // A table whose cells are not flat text: one holds a paragraph, one a
    // list run, so the nested `render_blocks` path is inside a fragment.
    let mut table = Block::paragraph("");
    table.id = StableId::parse("table-block").expect("valid id");
    table.content = Vec::new();
    let mut listed = Block::paragraph("cell list item");
    listed.id = StableId::parse("cell-list-item").expect("valid id");
    listed.kind = BlockKind::ListItem {
        list_id: StableId::new("list-in-cell"),
        level: 0,
        kind: ListKind::Bullet,
    };
    table.kind = BlockKind::table(vec![opendoc_core::TableRow {
        id: StableId::parse("table-row").expect("valid id"),
        height: None,
        header: false,
        cells: vec![
            opendoc_core::TableCell::new(vec![Block::paragraph("plain cell")]),
            opendoc_core::TableCell::new(vec![listed]),
        ],
    }]);
    document.blocks.push(table);

    let mut image = Block::paragraph("");
    image.id = StableId::parse("image-block").expect("valid id");
    image.content = Vec::new();
    image.kind = BlockKind::Image {
        blob_hash: "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
            .to_string(),
        alt_text: "a picture".to_string(),
        layout: opendoc_core::ImageLayout::default(),
    };
    document.blocks.push(image);

    let mut equation = Block::paragraph("");
    equation.id = StableId::parse("equation-block").expect("valid id");
    equation.content = Vec::new();
    equation.kind = BlockKind::EquationBlock {
        equation: latex_equation("equation-1", "x^2 + y^2 = z^2"),
    };
    document.blocks.push(equation);

    let mut page_break = Block::paragraph("");
    page_break.id = StableId::parse("page-break-block").expect("valid id");
    page_break.content = Vec::new();
    page_break.kind = BlockKind::PageBreak;
    document.blocks.push(page_break);

    // Inline variety on the first block: an equation, a footnote reference
    // and a link, so the fragment covering it exercises the inline walk.
    document.blocks[0].content.push(Inline::Equation {
        id: StableId::new("inline-equation"),
        equation: latex_equation("equation-2", "a_i"),
    });
    let footnote = opendoc_core::Footnote {
        id: StableId::new("footnote"),
        revision: 1,
        body: vec![Inline::text("note body")],
        deleted: false,
    };
    document.blocks[0].content.push(Inline::FootnoteRef {
        id: StableId::new("footnote-ref"),
        footnote_id: footnote.id.clone(),
    });
    document.footnotes.push(footnote);
    document.blocks[0].content.push(Inline::Link {
        id: StableId::new("link"),
        text: "docs".to_string(),
        href: "https://example.invalid/x".to_string(),
        marks: Vec::new(),
    });

    // A comment spanning two blocks, and a suggestion, both of which decorate
    // runs across a fragment boundary.
    let first = crate::context::inline_id(&document.blocks[0].content[0]).clone();
    let second = crate::context::inline_id(&document.blocks[1].content[0]).clone();
    document.comments.push(opendoc_core::CommentThread {
        id: StableId::new("thread"),
        anchor: Anchor::TextRange(TextRange {
            start: first.clone(),
            end: second,
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
                end: first,
            }),
            content: vec![Inline::text(" inserted")],
        },
        state: SuggestionState::Proposed,
        provenance: Vec::new(),
    });

    let mut marked = Mark {
        kind: MarkKind::Bold,
        value: None,
        expand: MarkExpand::Both,
    };
    if let Inline::Text { marks, .. } = &mut document.blocks[10].content[0] {
        marks.push(marked.clone());
        marked.kind = MarkKind::Code;
        marks.push(marked);
    }

    document
}

/// The one property the per-block DOM path rests on.
#[test]
fn fragments_compose_to_the_whole_body() {
    for document in [
        every_construct(),
        document_with(&[]),
        document_with(&["only one"]),
        table_document(),
        document_with_equation_block("\\frac{1}{2}"),
        document_with_inline_equation("\\alpha"),
    ] {
        let body = render_document_body(&document, []);
        let whole = render_document_html(&document, []);
        assert_eq!(body.html, whole, "the two paths render the same body");
        let composed: String = body.fragments.iter().map(|f| f.html.as_str()).collect();
        assert_eq!(
            composed, whole,
            "fragments must compose to exactly the whole body"
        );
    }
}

/// Composition alone would also hold for one fragment holding everything, so
/// the split has to be checked too: as many fragments as there are top-level
/// elements, each covering a contiguous run of blocks, together covering every
/// block exactly once and in document order.
#[test]
fn fragments_partition_the_blocks_in_document_order() {
    let document = every_construct();
    let body = render_document_body(&document, []);
    let covered: usize = body.fragments.iter().map(|fragment| fragment.blocks).sum();
    assert_eq!(
        covered,
        document.blocks.len(),
        "every top-level block belongs to exactly one fragment"
    );

    // The fragment keys, read in order, are the ids of the blocks that open
    // each element — a subsequence of the document's own block order.
    let mut expected = Vec::new();
    let mut index = 0;
    for fragment in &body.fragments {
        expected.push(document.blocks[index].id.to_string());
        index += fragment.blocks;
    }
    let keys: Vec<String> = body
        .fragments
        .iter()
        .map(|fragment| fragment.block_id.clone())
        .collect();
    assert_eq!(keys, expected);

    let unique: BTreeSet<&String> = keys.iter().collect();
    assert_eq!(unique.len(), keys.len(), "fragment keys must be unique");
    assert!(
        body.fragments
            .iter()
            .all(|fragment| !fragment.html.is_empty()),
        "a fragment with no markup would make the key sequence a lie"
    );
}

/// A list run is one element, so it is one fragment however many items it
/// holds and however deeply they nest — a nested item's `</li>` is written
/// after its child list closes, so cutting a run per item would produce
/// unbalanced markup. Three adjacent runs with three markers stay three
/// fragments rather than collapsing into one.
#[test]
fn a_list_run_is_one_fragment_and_adjacent_runs_stay_separate() {
    let document = every_construct();
    let body = render_document_body(&document, []);
    let runs: Vec<&BodyFragment> = body
        .fragments
        .iter()
        .filter(|fragment| fragment.html.starts_with("<ol") || fragment.html.starts_with("<ul"))
        .collect();
    assert_eq!(runs.len(), 3, "ordered, bulleted and checklist runs");
    assert_eq!(runs[0].blocks, 4, "four ordered items, one nested");
    assert_eq!(runs[0].block_id, document.blocks[3].id.to_string());
    assert_eq!(runs[0].html.matches("<li").count(), 4);
    assert_eq!(runs[1].blocks, 2);
    assert_eq!(runs[2].blocks, 1);
    assert!(runs[2].html.contains("doc-checklist"), "{}", runs[2].html);
}

/// Each fragment has to stand on its own, because a consumer parses it on its
/// own: one element, opened and closed inside the fragment.
#[test]
fn every_fragment_is_one_balanced_element() {
    let document = every_construct();
    for fragment in render_document_body(&document, []).fragments {
        let tags = tag_sequence(&fragment.html);
        if tags.is_empty() {
            // A page break is a single `<hr>`: one element, and nothing to
            // balance. Anything else with no tags at all is a bug.
            let name: String = fragment.html[1..]
                .chars()
                .take_while(char::is_ascii_alphanumeric)
                .collect();
            assert!(
                fragment.html.starts_with('<') && VOID_TAGS.contains(&name.as_str()),
                "a fragment with no balanced tags must be one void element: {}",
                fragment.html
            );
            continue;
        }
        let mut stack: Vec<String> = Vec::new();
        for (index, (name, closing)) in tags.iter().enumerate() {
            if *closing {
                assert_eq!(
                    stack.pop().as_deref(),
                    Some(name.as_str()),
                    "unbalanced markup in {}",
                    fragment.html
                );
            } else {
                assert!(
                    index == 0 || !stack.is_empty(),
                    "a fragment must be one element, not a sequence: {}",
                    fragment.html
                );
                stack.push(name.clone());
            }
        }
        assert!(stack.is_empty(), "unclosed markup in {}", fragment.html);
    }
}

/// The tags of an HTML string in order, as `(name, is_closing)`, skipping
/// void elements. Deliberately simple: the renderer escapes `<` in every text
/// node and attribute value it writes, so a `<` in this markup is always a
/// tag. Equations are the one source of foreign markup and MathML is
/// well-formed, which is what the balance check needs.
fn tag_sequence(html: &str) -> Vec<(String, bool)> {
    let mut tags = Vec::new();
    let bytes = html.as_bytes();
    let mut index = 0;
    while let Some(offset) = html[index..].find('<') {
        let start = index + offset + 1;
        let closing = bytes.get(start) == Some(&b'/');
        let name_start = if closing { start + 1 } else { start };
        let name_end = html[name_start..]
            .find(|ch: char| !ch.is_ascii_alphanumeric())
            .map(|at| name_start + at)
            .unwrap_or(html.len());
        let name = html[name_start..name_end].to_ascii_lowercase();
        let close = html[name_end..].find('>').map(|at| name_end + at).unwrap();
        let self_closing = bytes.get(close.wrapping_sub(1)) == Some(&b'/');
        if !name.is_empty() && !VOID_TAGS.contains(&name.as_str()) && !self_closing {
            tags.push((name, closing));
        }
        index = close + 1;
    }
    tags
}

/// The checker above has to be able to fail, or the test above proves nothing.
#[test]
fn the_balance_checker_rejects_a_split_list_run() {
    let tags = tag_sequence("<ul class=\"doc-list\"><li>one");
    assert_eq!(
        tags,
        vec![("ul".to_string(), false), ("li".to_string(), false)]
    );
    assert_eq!(
        tag_sequence("<p>a<br>b<img src=\"x\">c</p>"),
        vec![("p".to_string(), false), ("p".to_string(), true)],
        "void elements do not nest"
    );
}

/// A consumer applying fragments one at a time has to find the live node a
/// fragment belongs to, and the only thing in the markup that identifies one
/// is `data-block-id`. The key is therefore only usable if the **first**
/// `data-block-id` written inside a fragment is the fragment's own
/// `block_id`: a paragraph, heading, table, image, equation or page break
/// carries it on the element itself, and a list run carries it on its first
/// `<li>`, inside a wrapper that has none.
///
/// Without this, `editor.ts` would key a fragment onto the wrong element —
/// the first `<li>` of a run's *second* item, say — and typing in one block
/// would rewrite another.
#[test]
fn every_fragments_first_block_id_is_its_key() {
    for document in [
        every_construct(),
        document_with(&["only one"]),
        table_document(),
        document_with_equation_block("\\frac{1}{2}"),
        document_with_inline_equation("\\alpha"),
    ] {
        for fragment in render_document_body(&document, []).fragments {
            let needle = "data-block-id=\"";
            let at = fragment.html.find(needle).unwrap_or_else(|| {
                panic!(
                    "a fragment with no data-block-id cannot be keyed: {}",
                    fragment.html
                )
            });
            let rest = &fragment.html[at + needle.len()..];
            let end = rest.find('"').expect("attribute is closed");
            assert_eq!(
                &rest[..end],
                fragment.block_id,
                "the first data-block-id in a fragment must be its key: {}",
                fragment.html
            );
        }
    }
}
