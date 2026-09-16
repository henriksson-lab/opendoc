//! The numbering rule, pinned against what a browser actually draws.
//!
//! Every expected sequence below was **measured**, not derived: Chrome
//! 147.0.7727.57 was given the markup `opendoc-render` emits and asked, over
//! the DevTools protocol, for its accessibility tree, in which a list item's
//! marker box is an `AXListMarker` node whose name is the string Chrome
//! painted. The measurements are quoted beside each case. That matters here
//! more than usual: a test that checked this crate against `opendoc-render`
//! would have passed for the whole time the two agreed on 3, 4 for a list
//! that `<ol>` says is 1, 2.

use super::super::*;
use super::{lower_alpha, lower_roman};
use opendoc_core::StableId;

fn id(text: &str) -> StableId {
    StableId::parse(text).expect("valid id")
}

/// One item to place: (list, level, marker).
type Item<'a> = (&'a str, u8, ListMarker);

/// Runs a sequence of items through the numbering and reports, for each one,
/// the marker string that would be *drawn* beside it — a number for an
/// ordered item, the CSS bullet for a bulleted one, nothing for a checklist.
///
/// The drawn string rather than the raw ordinal, so the expectations below
/// can be compared with Chrome's marker text directly.
fn drawn(items: &[Item<'_>]) -> Vec<String> {
    let mut numbering = ListNumbering::default();
    items
        .iter()
        .map(|(list, level, marker)| {
            let number = numbering.open_item(&id(list), *level, *marker, &mut |_| {});
            let depth = usize::from(number.level);
            match number.marker {
                ListMarker::Checklist => String::new(),
                ListMarker::Bullet => bullet_glyph(depth).to_string(),
                ListMarker::Ordered => format!("{}.", ordered_marker(depth, number.ordinal)),
            }
        })
        .collect()
}

/// The wrapper opens and closes a sequence of items causes, as a transcript.
fn edges(items: &[Item<'_>]) -> Vec<String> {
    let mut numbering = ListNumbering::default();
    let mut log = Vec::new();
    for (list, level, marker) in items {
        numbering.open_item(&id(list), *level, *marker, &mut |edge| {
            log.push(match edge {
                ListEdge::Opened { list, root } => {
                    format!("open {} depth-{}{}", list.marker.tag(), list.level, {
                        if root {
                            " root"
                        } else {
                            ""
                        }
                    })
                }
                ListEdge::Closed { list, .. } => format!("close {}", list.marker.tag()),
            });
        });
        log.push(format!("item level-{level}"));
    }
    numbering.close_all(&mut |edge| {
        if let ListEdge::Closed { list, .. } = edge {
            log.push(format!("close {}", list.marker.tag()));
        }
    });
    log
}

/// The defect this module exists for. Chrome, given
/// `<ul …><li>b1<li>b2</ul><ol …><li>o1<li>o2</ol>`, draws
/// `"•" "•" "1." "2."`: closing a wrapper discards its count, and the `<ol>`
/// that opens next is a new list. Both engines used to draw 3 and 4, because
/// both counted the bullets into a counter keyed by level that a reopened
/// wrapper did not reset.
#[test]
fn a_wrapper_reopened_by_a_marker_change_starts_at_one() {
    assert_eq!(
        drawn(&[
            ("list-1", 0, ListMarker::Bullet),
            ("list-1", 0, ListMarker::Bullet),
            ("list-1", 0, ListMarker::Ordered),
            ("list-1", 0, ListMarker::Ordered),
        ]),
        ["•", "•", "1.", "2."],
        "a bulleted run must not spend the numbers of the ordered run after it"
    );
}

#[test]
fn an_explicit_wrapper_start_applies_once_and_the_next_item_continues() {
    let mut numbering = ListNumbering::default();
    let first =
        numbering.open_item_with_start(&id("continued"), 0, ListMarker::Ordered, 7, &mut |_| {});
    let second =
        numbering.open_item_with_start(&id("continued"), 0, ListMarker::Ordered, 7, &mut |_| {});
    assert_eq!((first.ordinal, second.ordinal), (7, 8));
}

/// Measured: `<ol depth-0><li>one<ol depth-1><li><li></ol></li><li>two</ol>`
/// draws `"1." "a." "b." "2."`.
#[test]
fn a_nested_wrapper_restarts_and_the_one_above_it_carries_on() {
    assert_eq!(
        drawn(&[
            ("list-1", 0, ListMarker::Ordered),
            ("list-1", 1, ListMarker::Ordered),
            ("list-1", 1, ListMarker::Ordered),
            ("list-1", 0, ListMarker::Ordered),
        ]),
        ["1.", "a.", "b.", "2."]
    );
}

/// Measured: `<ol depth-0><li>one<ul depth-1><li>bullet</ul></li><li>two</ol>`
/// draws `"1." "◦" "2."`. The bullet is in a wrapper of its own, so it takes
/// no number from the `<ol>` around it — which is why "bullets consume an
/// ordinal" stops being expressible once the ordinal lives on the wrapper.
#[test]
fn a_bulleted_level_inside_an_ordered_one_spends_no_number() {
    assert_eq!(
        drawn(&[
            ("list-1", 0, ListMarker::Ordered),
            ("list-1", 1, ListMarker::Bullet),
            ("list-1", 0, ListMarker::Ordered),
        ]),
        ["1.", "◦", "2."]
    );
}

/// Two lists that happen to be adjacent are two lists. The list is part of a
/// wrapper's identity exactly as the marker is, so the second one is a second
/// `<ol>` element — which is what makes it start at 1 in a browser that was
/// never told the number.
#[test]
fn a_second_list_at_the_same_level_is_a_second_wrapper() {
    assert_eq!(
        drawn(&[
            ("list-1", 0, ListMarker::Ordered),
            ("list-1", 0, ListMarker::Ordered),
            ("list-2", 0, ListMarker::Ordered),
        ]),
        ["1.", "2.", "1."]
    );
    assert_eq!(
        edges(&[
            ("list-1", 0, ListMarker::Ordered),
            ("list-2", 0, ListMarker::Ordered),
        ]),
        [
            "open ol depth-0 root",
            "item level-0",
            "close ol",
            "open ol depth-0 root",
            "item level-0",
            "close ol",
        ]
    );
}

/// A run that starts deep opens one wrapper at its own level, and a run that
/// descends opens one per level it passes. The `depth-N` an item is numbered
/// and styled by is the wrapper's, not the stack's height.
#[test]
fn a_run_that_starts_deep_opens_one_wrapper_at_its_own_level() {
    assert_eq!(
        edges(&[("list-1", 2, ListMarker::Ordered)]),
        ["open ol depth-2 root", "item level-2", "close ol"]
    );
    // Measured: an `<ol class="depth-2">` draws lower-roman, so a run that
    // starts at level 2 starts at "i." and not at "1.".
    assert_eq!(drawn(&[("list-1", 2, ListMarker::Ordered)]), ["i."]);
    assert_eq!(
        edges(&[
            ("list-1", 0, ListMarker::Ordered),
            ("list-1", 2, ListMarker::Ordered),
        ]),
        [
            "open ol depth-0 root",
            "item level-0",
            "open ol depth-1",
            "open ol depth-2",
            "item level-2",
            "close ol",
            "close ol",
            "close ol",
        ]
    );
}

/// Leaving a deeper level and coming back to it starts it again: the wrapper
/// that held the count was closed.
#[test]
fn a_level_left_and_re_entered_starts_again() {
    assert_eq!(
        drawn(&[
            ("list-1", 0, ListMarker::Ordered),
            ("list-1", 1, ListMarker::Ordered),
            ("list-1", 1, ListMarker::Ordered),
            ("list-1", 0, ListMarker::Ordered),
            ("list-1", 1, ListMarker::Ordered),
        ]),
        ["1.", "a.", "b.", "2.", "a."]
    );
}

/// `closed_root` is what tells the flow that a finished wrapper's bottom
/// margin has to be paid. It is true only when the *outermost* wrapper ended
/// and another took its place — not when a nested level closes, because
/// nested `.doc-list`s carry no bottom margin.
#[test]
fn only_the_outermost_wrapper_reports_a_close() {
    let mut numbering = ListNumbering::default();
    let mut closed_root = |list: &str, level: u8, marker: ListMarker| {
        numbering
            .open_item(&id(list), level, marker, &mut |_| {})
            .closed_root
    };
    assert!(!closed_root("list-1", 0, ListMarker::Ordered), "the first");
    assert!(
        !closed_root("list-1", 1, ListMarker::Ordered),
        "going deeper opens, it does not close the root"
    );
    assert!(
        !closed_root("list-1", 0, ListMarker::Ordered),
        "coming back closes the nested wrapper only"
    );
    assert!(
        closed_root("list-1", 0, ListMarker::Bullet),
        "a marker change at the top level closes the outermost wrapper"
    );
    assert!(
        closed_root("list-2", 0, ListMarker::Bullet),
        "so does a different list at the top level"
    );
}

/// `lower-alpha` and `lower-roman` are CSS counter styles, and these are the
/// sequences CSS defines: bijective base-26, and Roman numerals that fall back
/// to decimal outside the representable range rather than inventing notation.
#[test]
fn lower_alpha_and_lower_roman_follow_css() {
    assert_eq!("a", lower_alpha(1));
    assert_eq!("z", lower_alpha(26));
    assert_eq!("aa", lower_alpha(27));
    assert_eq!("ab", lower_alpha(28));
    assert_eq!("i", lower_roman(1));
    assert_eq!("iv", lower_roman(4));
    assert_eq!("xlii", lower_roman(42));
    assert_eq!("mcmxcix", lower_roman(1_999));
    assert_eq!("4000", lower_roman(4_000));
}

/// The cut-off, stated in literal depths rather than in terms of
/// `STYLED_LIST_DEPTHS`.
///
/// Deliberately: a loop written as `for depth in 0..STYLED_LIST_DEPTHS` takes
/// its expectation from the constant it is checking, so raising the constant
/// keeps the test green while the paper starts drawing markers the screen
/// does not. These are the depths measured in Chrome — `depth-8` is styled,
/// `depth-9` falls back to the initial `disc`/`decimal`.
#[test]
fn the_marker_cycle_stops_where_the_stylesheet_stops_stating_it() {
    let cycles: Vec<usize> = (0..13).map(marker_cycle).collect();
    assert_eq!(
        cycles,
        [0, 1, 2, 0, 1, 2, 0, 1, 2, 0, 0, 0, 0],
        "three full turns and then nothing"
    );
    assert_eq!(ordered_marker(8, 2), "ii", "depth-8 is lower-roman");
    assert_eq!(ordered_marker(9, 2), "2", "depth-9 has no rule to follow");
    assert_eq!(ordered_marker(12, 2), "2");
    assert_eq!(bullet_glyph(8), '\u{25A0}');
    assert_eq!(bullet_glyph(9), '\u{2022}');
}

/// The ordered cycle, measured. Chrome drew
/// `"1." "a." "b." "i." "ii." "1." "2."` for a list nested to depth 3, so
/// depth 3 restarts the cycle at decimal.
#[test]
fn the_ordered_cycle_is_the_sequence_chrome_draws() {
    let nested: Vec<String> = [(0, 1), (1, 1), (1, 2), (2, 1), (2, 2), (3, 1), (3, 2)]
        .into_iter()
        .map(|(depth, ordinal)| format!("{}.", ordered_marker(depth, ordinal)))
        .collect();
    assert_eq!(nested, ["1.", "a.", "b.", "i.", "ii.", "1.", "2."]);
}

/// The bullet cycle, measured: depth 0 draws `•`, depth 1 `◦` (the `circle`
/// keyword), depth 2 the square.
#[test]
fn the_bullet_cycle_is_the_sequence_chrome_draws() {
    assert_eq!(bullet_glyph(0), '\u{2022}');
    assert_eq!(bullet_glyph(1), '\u{25E6}');
    assert_eq!(bullet_glyph(2), '\u{25A0}');
    assert_eq!(bullet_glyph(3), '\u{2022}', "the cycle restarts at depth 3");
}

/// The generated stylesheet rules, written out in full.
///
/// This exact text is what was loaded into Chrome to measure every marker
/// expectation in this file, and it is character for character the block that
/// used to be hand-written in `opendoc-render`'s standalone export — which is
/// what makes generating it a replacement rather than a change. Stated as a
/// literal on purpose: a rule set derived from `STYLED_LIST_DEPTHS` would
/// follow the constant wherever it went.
#[test]
fn the_generated_rules_are_the_stylesheet_the_markers_were_measured_against() {
    assert_eq!(
        list_style_type_rules(".doc-body "),
        concat!(
            ".doc-body ul.depth-1, .doc-body ul.depth-4, .doc-body ul.depth-7 { list-style-type: circle; }\n",
            ".doc-body ul.depth-2, .doc-body ul.depth-5, .doc-body ul.depth-8 { list-style-type: square; }\n",
            ".doc-body ol.depth-1, .doc-body ol.depth-4, .doc-body ol.depth-7 { list-style-type: lower-alpha; }\n",
            ".doc-body ol.depth-2, .doc-body ol.depth-5, .doc-body ol.depth-8 { list-style-type: lower-roman; }\n",
        )
    );
}

/// The markers actually *painted* on a page, through the whole engine rather
/// than through the numbering alone.
///
/// This is the half the numbering tests above cannot reach: `fragments` has
/// to hand `paint_list_marker` the wrapper's depth and the item's ordinal,
/// and handing it a constant for either produces a page that is wrong in a
/// way no unit test of `ListNumbering` would see. The expected sequence is
/// Chrome's, measured on the equivalent markup.
#[test]
fn the_painted_markers_are_the_ones_chrome_draws() {
    let mut document = Document::new("layout");
    document.blocks = [0u8, 1, 1, 2, 2, 1, 0]
        .into_iter()
        .enumerate()
        .map(|(index, level)| {
            let mut block = Block::paragraph("an item");
            block.id = id(&format!("block-{index}"));
            block.kind = BlockKind::ListItem {
                list_id: id("list-1"),
                level,
                kind: ListKind::Ordered,
            };
            block
        })
        .collect();
    let painted = crate::layout_painted_document(&document);
    let markers: Vec<String> = painted
        .pages
        .iter()
        .flat_map(|page| page.items.iter())
        .filter_map(|item| match item {
            PaintItem::Text { runs, .. } => Some(runs),
            _ => None,
        })
        .flatten()
        .map(|run| run.text.clone())
        .filter(|text| text != "an item")
        .collect();
    assert_eq!(markers, ["1.", "a.", "b.", "i.", "ii.", "c.", "2."]);
}
