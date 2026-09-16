//! What the emitted markup says a list item's number is, and whether a
//! browser given that markup would agree.
//!
//! The check that matters here is [`every_ordered_item_is_numbered_by_its_
//! position_in_its_own_wrapper`]: it re-counts the `<li>`s out of the emitted
//! string with a deliberately stupid scanner that knows nothing about this
//! crate, and compares that with the `value=` attributes written into it.
//! That is the HTML rule — Chrome 147, given the same markup with the
//! attributes removed, draws exactly those numbers — so a `value=` that
//! disagrees with it is a number the screen would not have produced on its
//! own.

use crate::*;

fn list_item(id: &str, list: &str, level: u8, kind: ListKind, text: &str) -> Block {
    let mut block = Block::paragraph(text);
    block.id = StableId::parse(id).expect("valid id");
    block.kind = BlockKind::ListItem {
        list_id: StableId::parse(list).expect("valid id"),
        level,
        kind,
    };
    block
}

fn document(blocks: Vec<Block>) -> Document {
    let mut document = Document::new("render");
    document.blocks = blocks;
    document
}

/// Every `<li>` in the markup, with the wrapper it is in and the `value=` it
/// carries.
///
/// Written by hand and on purpose knows nothing about `ListWriter`: it walks
/// the string, keeps a stack of open `<ol>`/`<ul>` elements, and counts the
/// items in each. That count is what a browser numbers an ordered item by.
///
/// It also insists the markup is well formed while it walks — an item opened
/// over its sibling, a `</li>` with no item open, a wrapper closed around one
/// — because the count only means anything if the elements are the ones a
/// browser will build.
struct ScannedItem {
    /// The tag of the wrapper holding the item.
    wrapper: String,
    /// The item's position among its wrapper's own children, from 1.
    position: u32,
    /// The `value=` attribute on the item, if it has one.
    value: Option<u32>,
}

fn scan_items(html: &str) -> Vec<ScannedItem> {
    struct OpenWrapper {
        tag: String,
        items: u32,
        item_open: bool,
    }
    let mut stack: Vec<OpenWrapper> = Vec::new();
    let mut items = Vec::new();
    let mut at = 0;
    while let Some(offset) = html[at..].find('<') {
        let start = at + offset;
        let end = match html[start..].find('>') {
            Some(length) => start + length,
            None => break,
        };
        let tag = &html[start + 1..end];
        at = end + 1;
        if let Some(name) = tag.strip_prefix('/') {
            match name {
                "li" => {
                    let top = stack.last_mut().expect("a </li> outside any wrapper");
                    assert!(top.item_open, "a </li> with no item open: {html}");
                    top.item_open = false;
                }
                "ol" | "ul" => {
                    let top = stack.pop().expect("a list closed that never opened");
                    assert_eq!(top.tag, name, "list tags crossed: {html}");
                    assert!(
                        !top.item_open,
                        "a <{name}> closed around an open item: {html}"
                    );
                }
                _ => {}
            }
            continue;
        }
        let name: String = tag
            .chars()
            .take_while(|character| character.is_ascii_alphanumeric())
            .collect();
        match name.as_str() {
            "ol" | "ul" => stack.push(OpenWrapper {
                tag: name,
                items: 0,
                item_open: false,
            }),
            "li" => {
                let top = stack.last_mut().expect("an item outside any wrapper");
                assert!(!top.item_open, "an item opened over its sibling: {html}");
                top.item_open = true;
                top.items += 1;
                let value = tag.find(" value=\"").map(|found| {
                    let digits = &tag[found + " value=\"".len()..];
                    let digits = &digits[..digits.find('"').expect("a closed attribute")];
                    digits.parse::<u32>().expect("a numeric value attribute")
                });
                items.push(ScannedItem {
                    wrapper: top.tag.clone(),
                    position: top.items,
                    value,
                });
            }
            _ => {}
        }
    }
    assert!(stack.is_empty(), "unbalanced list markup: {html}");
    items
}

/// A document with everything that can move a list item from one wrapper to
/// another: a marker change at a level, a nested level, a jump past a level
/// (which opens a wrapper that never holds an item of its own), a return to
/// the level above, a checklist, and a second list at the same level as the
/// first.
fn mixed_list_document() -> Document {
    let items = [
        (0u8, "list-1", ListKind::Bullet),
        (0, "list-1", ListKind::Bullet),
        (0, "list-1", ListKind::Ordered),
        (1, "list-1", ListKind::Ordered),
        (1, "list-1", ListKind::Ordered),
        (0, "list-1", ListKind::Ordered),
        // Straight from the top level to level 2: the level-1 wrapper is
        // opened to hold the level-2 one and has no item of its own yet.
        (2, "list-1", ListKind::Ordered),
        (1, "list-1", ListKind::Ordered),
        (0, "list-1", ListKind::Ordered),
        // The same jump, but this time the run comes straight back to the top
        // level — so the level-1 wrapper is opened, holds nothing at all, and
        // closes again. It is the one wrapper whose `</li>` must not be
        // written, because it never opened an item.
        (2, "list-1", ListKind::Ordered),
        (0, "list-1", ListKind::Ordered),
        (1, "list-1", ListKind::Bullet),
        (0, "list-1", ListKind::Ordered),
        (0, "list-1", ListKind::Checklist { checked: false }),
        (0, "list-2", ListKind::Ordered),
        (0, "list-2", ListKind::Ordered),
    ];
    document(
        items
            .into_iter()
            .enumerate()
            .map(|(index, (level, list, kind))| {
                list_item(&format!("block-{index}"), list, level, kind, "an item")
            })
            .collect(),
    )
}

/// The invariant: the number written on an item is the number a browser would
/// count for it, unaided. Both halves are independent of `ListWriter` — one
/// is the attribute it wrote, the other is a scan of the markup it wrote.
#[test]
fn every_ordered_item_is_numbered_by_its_position_in_its_own_wrapper() {
    let html = render_document_html(&mixed_list_document(), []);
    let items = scan_items(&html);
    assert_eq!(items.len(), 16, "every item is in the markup: {html}");
    for item in &items {
        match item.wrapper.as_str() {
            "ol" => assert_eq!(
                item.value,
                Some(item.position),
                "an ordered item is the {} of its <ol> and is numbered {:?}: {html}",
                item.position,
                item.value
            ),
            _ => assert_eq!(
                item.value, None,
                "an item in a <ul> carries a number the browser will not draw: {html}"
            ),
        }
    }
}

/// The same document, stated as the sequence of numbers rather than as an
/// invariant — so a change that made both halves above wrong in step still
/// has to explain itself. Hand-written from the structure, and confirmed by
/// asking Chrome what it draws for this export with the `value=` attributes
/// stripped out of it.
#[test]
fn the_numbers_the_markup_states_are_the_expected_sequence() {
    let html = render_document_html(&mixed_list_document(), []);
    let numbers: Vec<Option<u32>> = scan_items(&html).iter().map(|item| item.value).collect();
    assert_eq!(
        numbers,
        [
            None,    // a bullet
            None,    // a bullet
            Some(1), // the <ol> opening here is a new list, not a continuation
            Some(1), // a nested <ol> starts again
            Some(2),
            Some(2), // back in the outer <ol>, which kept its count
            Some(1), // two levels down, in a wrapper of its own
            // the level-1 wrapper's first item, though it was opened two
            // items ago to hold the level-2 one
            Some(1),
            Some(3),
            Some(1), // two levels down again
            Some(4), // and straight back out, past a wrapper that held nothing
            None,    // a nested bullet
            Some(5),
            None,    // the checklist is a <ul>
            Some(1), // a different list is a different <ol>
            Some(2),
        ],
        "{html}"
    );
}

/// A marker change and a list change both end a wrapper, so the run above
/// becomes four top-level elements rather than one.
#[test]
fn a_marker_or_list_change_ends_the_wrapper() {
    let html = render_document_html(&mixed_list_document(), []);
    assert_eq!(
        html.matches("<ul class=\"doc-list depth-0\"").count(),
        1,
        "the bulleted run at the top level: {html}"
    );
    assert_eq!(
        html.matches("<ol class=\"doc-list depth-0\"").count(),
        2,
        "two ordered lists at the top level, because list-2 is not list-1: {html}"
    );
    assert_eq!(
        html.matches("doc-checklist").count(),
        1,
        "the checklist is its own wrapper: {html}"
    );
    // The guard on the fixture: a level-1 wrapper that holds nothing but the
    // level-2 wrapper is what the jump produces, and it is the case the
    // `</li>` bookkeeping can get wrong. If the fixture ever stopped
    // containing one, the checks above would still pass and mean less.
    assert!(
        html.contains(
            "<ol class=\"doc-list depth-1\" data-level=\"1\"><ol class=\"doc-list depth-2\""
        ),
        "the fixture has no wrapper opened only to hold a deeper one: {html}"
    );
}

/// The exported stylesheet states the cycle for exactly the depths the
/// renderer puts on a wrapper, because both come from
/// `opendoc_layout::list_style_type`.
#[test]
fn the_exported_stylesheet_states_the_cycle_the_layout_paints() {
    let mut blocks = Vec::new();
    for level in 0..=2u8 {
        blocks.push(list_item(
            &format!("block-{level}"),
            "list-1",
            level,
            ListKind::Ordered,
            "item",
        ));
    }
    let exported = render_standalone_html(&document(blocks), []).html;
    for (depth, style) in [(1usize, "lower-alpha"), (2, "lower-roman")] {
        assert!(
            exported.contains(&format!("depth-{depth}\" data-level=\"{depth}\"")),
            "the body has no depth-{depth} wrapper: {exported}"
        );
        let rule = exported
            .lines()
            .find(|line| line.contains(&format!("ol.depth-{depth}")))
            .unwrap_or_else(|| panic!("no ol.depth-{depth} rule in the export"));
        assert!(
            rule.contains(&format!("list-style-type: {style};")),
            "depth-{depth} is styled {rule}, but the layout paints {style}"
        );
    }
}
