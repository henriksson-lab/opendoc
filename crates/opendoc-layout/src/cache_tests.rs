//! The only thing that matters about [`LayoutCache`]: a cached layout is the
//! same layout.
//!
//! A cache that is merely *fast* is worthless here, because the browser, the
//! PDF writer and any headless runtime all have to agree about where the
//! pages fell. So the test below is not a handful of cases: it is a family of
//! generated documents, driven through a long sequence of edits, with the
//! cached answer compared field for field against a freshly computed one
//! **after every single edit**. `DocumentLayout` derives `Eq` over every
//! field of every placement, so "equal" here means the page, the top, the
//! height, the line count, the page-opening margin and the exactness flag all
//! match for every block.
//!
//! The generator deliberately produces the shapes that would break a cache
//! keyed on less than the whole input: list runs that split and re-form,
//! nested levels, blocks whose only difference is a mark or a property,
//! tables whose cells change, text outside the bundled subset, and edits that
//! insert, delete, reorder and re-id blocks.

use crate::{layout_document, LayoutCache};
use opendoc_core::{
    Alignment, Block, BlockKind, Document, Equation, EquationSourceFormat, ImageLayout, Inline,
    Length, LineHeightMultiple, LineSpacing, ListKind, Mark, MarkExpand, MarkKind, PageSetup,
    StableId, TableCell, TableRow, TextDirection,
};

/// A tiny deterministic generator. Not `rand`: this crate has one dependency
/// and it is a font parser, and a test that cannot be reproduced from its
/// seed is not evidence.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        // Odd increment, full-period LCG constants (Numerical Recipes).
        Self(seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1))
    }

    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 11
    }

    fn below(&mut self, bound: usize) -> usize {
        if bound == 0 {
            0
        } else {
            (self.next() % bound as u64) as usize
        }
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }
}

/// Words from inside and outside the bundled subset, so some blocks measure
/// exactly and some do not.
const WORDS: [&str; 14] = [
    "the",
    "quick",
    "brown",
    "fox",
    "jumps",
    "supercalifragilistic",
    "über",
    "naïve",
    "日本語",
    "Привет",
    "a",
    "—",
    "x²",
    "checklist",
];

fn text(rng: &mut Rng, words: usize) -> String {
    (0..words)
        .map(|_| WORDS[rng.below(WORDS.len())])
        .collect::<Vec<_>>()
        .join(" ")
}

fn marks(rng: &mut Rng) -> Vec<Mark> {
    const KINDS: [MarkKind; 6] = [
        MarkKind::Bold,
        MarkKind::Italic,
        MarkKind::Code,
        MarkKind::Superscript,
        MarkKind::Subscript,
        MarkKind::Underline,
    ];
    let mut out = Vec::new();
    if rng.chance(45) {
        out.push(Mark {
            kind: KINDS[rng.below(KINDS.len())].clone(),
            value: None,
            expand: MarkExpand::None,
        });
    }
    if rng.chance(15) {
        out.push(Mark {
            kind: MarkKind::Size,
            value: Some(format!("{}pt", 8 + rng.below(12))),
            expand: MarkExpand::None,
        });
    }
    out
}

fn inline(rng: &mut Rng, id: &str) -> Inline {
    let stable = StableId::parse(id).expect("valid id");
    match rng.below(7) {
        0 => Inline::Link {
            id: stable,
            text: {
                let words = 1 + rng.below(4);
                text(rng, words)
            },
            href: "https://example.invalid/".to_string(),
            marks: marks(rng),
        },
        1 => Inline::Citation {
            id: stable,
            citation_id: StableId::parse("cite-1").expect("valid id"),
            rendered_cache: rng.chance(50).then(|| "(Author 2020)".to_string()),
        },
        2 => Inline::FootnoteRef {
            id: stable,
            footnote_id: StableId::parse("note-1").expect("valid id"),
        },
        3 => Inline::Equation {
            id: stable,
            equation: Equation {
                id: StableId::parse("eq-1").expect("valid id"),
                source_format: EquationSourceFormat::LatexLike,
                source: "\\frac{a}{b}".to_string(),
            },
        },
        4 => Inline::Mention {
            id: stable,
            label: text(rng, 1),
        },
        _ => Inline::Text {
            id: stable,
            text: {
                let words = 1 + rng.below(12);
                text(rng, words)
            },
            marks: marks(rng),
        },
    }
}

fn properties(rng: &mut Rng, block: &mut Block) {
    if rng.chance(25) {
        block.properties.alignment = Some(
            [
                Alignment::Start,
                Alignment::Center,
                Alignment::End,
                Alignment::Justify,
            ][rng.below(4)],
        );
    }
    if rng.chance(20) {
        block.properties.indent_start =
            Some(Length::from_twips(rng.below(1_440) as i32).expect("valid"));
    }
    if rng.chance(15) {
        block.properties.indent_end =
            Some(Length::from_twips(rng.below(720) as i32).expect("valid"));
    }
    if rng.chance(20) {
        // Negative: a hanging indent, the one shape that widens a first line.
        let twips = rng.below(720) as i32 - 360;
        block.properties.indent_first_line = Some(Length::from_twips(twips).expect("valid"));
    }
    if rng.chance(20) {
        block.properties.space_before =
            Some(Length::from_twips(rng.below(600) as i32).expect("valid"));
    }
    if rng.chance(20) {
        block.properties.space_after =
            Some(Length::from_twips(rng.below(600) as i32).expect("valid"));
    }
    if rng.chance(20) {
        block.properties.line_spacing = Some(match rng.below(3) {
            0 => LineSpacing::Multiple(
                LineHeightMultiple::from_thousandths(1_000 + rng.below(1_500) as u32)
                    .expect("valid"),
            ),
            1 => LineSpacing::AtLeast(
                Length::from_twips(200 + rng.below(400) as i32).expect("valid"),
            ),
            _ => {
                LineSpacing::Exact(Length::from_twips(200 + rng.below(400) as i32).expect("valid"))
            }
        });
    }
    if rng.chance(10) {
        block.properties.direction = Some(TextDirection::RightToLeft);
    }
}

fn block(rng: &mut Rng, index: usize, list_run: usize) -> Block {
    let id = StableId::parse(format!("block-{index:04}")).expect("valid id");
    let mut block = Block::paragraph("");
    block.id = id;
    block.content = (0..1 + rng.below(3))
        .map(|run| inline(rng, &format!("inline-{index:04}-{run}")))
        .collect();
    match rng.below(12) {
        0 => {
            block.kind = BlockKind::Heading {
                level: 1 + rng.below(6) as u8,
            }
        }
        1..=4 => {
            block.kind = BlockKind::ListItem {
                list_id: StableId::parse(format!("list-{list_run}")).expect("valid id"),
                level: rng.below(4) as u8,
                kind: match rng.below(3) {
                    0 => ListKind::Bullet,
                    1 => ListKind::Ordered,
                    _ => ListKind::Checklist {
                        checked: rng.chance(50),
                    },
                },
            }
        }
        5 => {
            block.content.clear();
            block.kind = BlockKind::PageBreak;
        }
        6 => {
            block.content.clear();
            block.kind = BlockKind::Image {
                blob_hash: "sha256:0".to_string(),
                alt_text: if rng.chance(50) {
                    text(rng, 3)
                } else {
                    String::new()
                },
                layout: ImageLayout {
                    width: None,
                    height: rng
                        .chance(50)
                        .then(|| Length::from_twips(720 + rng.below(2_880) as i32).expect("valid")),
                    placement: None,
                    ..ImageLayout::default()
                },
            }
        }
        7 => {
            block.content.clear();
            block.kind = BlockKind::EquationBlock {
                equation: Equation {
                    id: StableId::parse(format!("eqb-{index:04}")).expect("valid id"),
                    source_format: EquationSourceFormat::LatexLike,
                    source: "\\int_0^1 x^2 dx".to_string(),
                },
            }
        }
        8 => {
            block.content.clear();
            let rows = 1 + rng.below(3);
            let columns = 1 + rng.below(3);
            block.kind = BlockKind::table(
                (0..rows)
                    .map(|row| TableRow {
                        id: StableId::parse(format!("row-{index:04}-{row}")).expect("valid id"),
                        height: None,
                        header: false,
                        cells: (0..columns)
                            .map(|column| {
                                let words = 1 + rng.below(8);
                                let mut cell = Block::paragraph(text(rng, words));
                                cell.id = StableId::parse(format!(
                                    "cell-block-{index:04}-{row}-{column}"
                                ))
                                .expect("valid id");
                                TableCell {
                                    id: StableId::parse(format!("cell-{index:04}-{row}-{column}"))
                                        .expect("valid id"),
                                    span: opendoc_core::CellSpan::SINGLE,
                                    properties: Default::default(),
                                    blocks: vec![cell],
                                }
                            })
                            .collect(),
                    })
                    .collect(),
            );
        }
        _ => {}
    }
    properties(rng, &mut block);
    block
}

fn generated_document(seed: u64, setup: PageSetup, blocks: usize) -> Document {
    let mut rng = Rng::new(seed);
    let mut document = Document::new("cache");
    document.page_setup = setup;
    let mut run = 0usize;
    document.blocks = (0..blocks)
        .map(|index| {
            if rng.chance(20) {
                run += 1;
            }
            block(&mut rng, index, run)
        })
        .collect();
    document
}

/// The three page boxes: a short page that breaks constantly, the default
/// letter page, and a wide landscape one. A break is a function of the page
/// box, so the same document on three boxes exercises three flows.
fn page_setups() -> [PageSetup; 3] {
    [
        PageSetup {
            height: Length::from_twips(3 * 1_440).expect("3in"),
            ..PageSetup::default()
        },
        PageSetup::default(),
        PageSetup {
            width: Length::from_twips(15_840).expect("11in"),
            height: Length::from_twips(12_240).expect("8.5in"),
            margin_start: Length::from_twips(2_880).expect("2in"),
            margin_end: Length::from_twips(2_880).expect("2in"),
            ..PageSetup::default()
        },
    ]
}

/// One edit of the kind a user makes. Returns false when there was nothing to
/// do, so the caller can try another.
fn edit(rng: &mut Rng, document: &mut Document, step: usize) -> bool {
    if document.blocks.is_empty() {
        document.blocks.push(block(rng, 9_000 + step, 99));
        return true;
    }
    let at = rng.below(document.blocks.len());
    match rng.below(10) {
        // Typing: one character into the first text run, which is the edit
        // the whole cache exists for.
        0..=4 => {
            let target = &mut document.blocks[at];
            match target.content.iter_mut().find_map(|inline| match inline {
                Inline::Text { text, .. } | Inline::Link { text, .. } => Some(text),
                _ => None,
            }) {
                Some(text) => text.push(char::from(b'a' + (step % 26) as u8)),
                None => target.content.push(Inline::Text {
                    id: StableId::parse(format!("inline-new-{step}")).expect("valid id"),
                    text: "typed".to_string(),
                    marks: Vec::new(),
                }),
            }
        }
        // Structural: a new block appears, which shifts every index after it.
        5 => document
            .blocks
            .insert(at, block(rng, 9_000 + step, 50 + step)),
        6 => {
            document.blocks.remove(at);
        }
        // Reorder, which no index-aligned cache can follow.
        7 => {
            let to = rng.below(document.blocks.len());
            let moved = document.blocks.remove(at);
            document.blocks.insert(to, moved);
        }
        // A property change with no text change: the shape a cache keyed on
        // text alone would get wrong.
        8 => properties(rng, &mut document.blocks[at]),
        // The page box itself moves, which changes every frame width.
        _ => {
            let width = 8_000 + rng.below(6_000) as i32;
            document.page_setup.width = Length::from_twips(width).expect("valid");
            if document.page_setup.margin_start.twips() + document.page_setup.margin_end.twips()
                >= width
            {
                document.page_setup.margin_start = Length::from_twips(720).expect("valid");
                document.page_setup.margin_end = Length::from_twips(720).expect("valid");
            }
        }
    }
    true
}

/// **The test.** 60 seeds x 3 page setups = 180 documents, each driven
/// through 20 edits, with the cached layout compared against a fresh one
/// after every edit: 3,780 comparisons of whole layouts.
#[test]
fn a_cached_layout_is_identical_to_an_uncached_one() {
    let mut reused = 0usize;
    let mut measured = 0usize;
    let mut compared = 0usize;
    for seed in 0..60u64 {
        for (index, setup) in page_setups().into_iter().enumerate() {
            let mut document = generated_document(seed, setup, 40 + (seed as usize % 30));
            // One cache across the whole edit sequence, exactly as the app
            // uses it: the interesting states are the ones a *previous* pass
            // left behind.
            let mut cache = LayoutCache::new();
            let mut rng = Rng::new(seed.wrapping_mul(31).wrapping_add(index as u64));
            for step in 0..20 {
                let cached = cache.layout(&document);
                let fresh = layout_document(&document);
                assert_eq!(
                    cached, fresh,
                    "seed {seed}, page setup {index}, step {step}: the cache changed the layout"
                );
                compared += 1;
                reused += cache.stats().reused;
                measured += cache.stats().measured;
                edit(&mut rng, &mut document, step);
            }
        }
    }
    assert_eq!(compared, 3_600);
    // A cache that never hit would pass every assertion above. It has to be
    // doing the thing it exists for.
    assert!(
        reused > measured,
        "the cache answered {reused} blocks from the previous pass and measured {measured}; \
         it is not being used"
    );
}

/// Typing one character into a 200-block document measures one block.
///
/// This is the property the whole change is for, stated as a number rather
/// than as a timing: everything else comes back from the previous pass.
#[test]
fn typing_one_character_measures_one_block() {
    let mut document = generated_document(7, PageSetup::default(), 200);
    // Something a page break cannot make ambiguous.
    document.blocks = document
        .blocks
        .into_iter()
        .map(|mut block| {
            block.kind = BlockKind::Paragraph;
            block
        })
        .collect();
    let mut cache = LayoutCache::new();
    let first = cache.layout(&document);
    assert_eq!(cache.stats().measured, document.blocks.len());
    assert_eq!(cache.stats().reused, 0);

    let target = 100;
    match document.blocks[target]
        .content
        .iter_mut()
        .find_map(|inline| match inline {
            Inline::Text { text, .. } | Inline::Link { text, .. } => Some(text),
            _ => None,
        }) {
        // Enough to push the block onto another line, so the layout really
        // has to differ and a cache that returned the old fragment would be
        // caught by the comparison below rather than passing by luck.
        Some(text) => text.push_str(" and then a good deal more text than there was before"),
        None => document.blocks[target].content.push(Inline::text(
            "and then a good deal more text than there was before",
        )),
    }
    let second = cache.layout(&document);
    assert_eq!(
        cache.stats().measured,
        1,
        "one keystroke should measure exactly the block it touched"
    );
    assert_eq!(cache.stats().reused, document.blocks.len() - 1);
    assert_eq!(second, layout_document(&document));
    assert_ne!(first, second, "the edit has to have changed something");
}

/// Inserting a block shifts every index after it, and the layout still has to
/// come out right — the fallback that finds a stored entry by id rather than
/// by position.
#[test]
fn inserting_a_block_keeps_the_rest_of_the_cache() {
    let mut document = generated_document(3, PageSetup::default(), 120);
    let mut cache = LayoutCache::new();
    cache.layout(&document);
    let mut inserted = Block::paragraph("a brand new paragraph nobody has measured before");
    inserted.id = StableId::parse("block-inserted").expect("valid id");
    document.blocks.insert(10, inserted);
    let cached = cache.layout(&document);
    assert_eq!(cached, layout_document(&document));
    assert_eq!(
        cache.stats().measured,
        1,
        "only the new block is new; the other 120 moved, they did not change"
    );
}

/// Deleting a block, and moving one, must not cost a re-measure of everything
/// after it.
///
/// This is what the by-id fallback is for, and the only thing that shows it:
/// after a deletion every following block is one place earlier than the cache
/// left it, so a purely position-aligned lookup misses on the first one and
/// then on every single one after it. Correctness survives that — a miss only
/// measures — which is exactly why it needs a test of its own that counts the
/// work rather than the answer.
#[test]
fn a_block_that_moved_is_found_where_it_went() {
    let mut document = generated_document(5, PageSetup::default(), 120);
    let mut cache = LayoutCache::new();
    cache.layout(&document);

    document.blocks.remove(4);
    let after_delete = cache.layout(&document);
    assert_eq!(after_delete, layout_document(&document));
    assert_eq!(
        cache.stats().measured,
        0,
        "a deletion introduces no new block, so nothing needs measuring"
    );

    let moved = document.blocks.remove(3);
    document.blocks.push(moved);
    let after_move = cache.layout(&document);
    assert_eq!(after_move, layout_document(&document));
    assert_eq!(
        cache.stats().measured,
        0,
        "a block that moved is the same block in the same frame"
    );
}

/// The page box is not part of a block, so changing it has to invalidate the
/// blocks whose frame it decides. A cache that kept the old widths would
/// report the old line counts and the old pages.
#[test]
fn a_narrower_page_re_measures_every_block() {
    let mut document = generated_document(11, PageSetup::default(), 80);
    let mut cache = LayoutCache::new();
    let wide = cache.layout(&document);
    document.page_setup.margin_start = Length::from_twips(4_320).expect("3in");
    document.page_setup.margin_end = Length::from_twips(4_320).expect("3in");
    let narrow = cache.layout(&document);
    assert_eq!(narrow, layout_document(&document));
    assert_ne!(wide, narrow, "a three-inch column wraps differently");
    assert_eq!(cache.stats().reused, 0, "every frame changed");
}

/// Two documents through one cache. The app has one cache per runtime and can
/// close one document and open another; a cache that answered from the wrong
/// document would be the worst possible bug this change could introduce.
#[test]
fn one_cache_serves_two_documents_without_mixing_them() {
    let first = generated_document(21, PageSetup::default(), 60);
    let second = generated_document(22, PageSetup::default(), 60);
    let mut cache = LayoutCache::new();
    for _ in 0..4 {
        assert_eq!(cache.layout(&first), layout_document(&first));
        assert_eq!(cache.layout(&second), layout_document(&second));
    }
    // The two documents share block ids (the generator numbers them the same
    // way), so this is the case where an id-keyed cache with no value check
    // would serve one document's heights for the other's blocks.
    assert_eq!(first.blocks[0].id, second.blocks[0].id);
}

/// A document carrying suggestions renders inline content that is not in
/// `Block::content`, so every block's height stops being exact. It is a
/// document-level input, not a block-level one — the cache has to drop
/// everything when it changes.
#[test]
fn suggestions_invalidate_the_whole_cache() {
    // Latin-only paragraphs, so every block is exact before the suggestion
    // arrives and the flag can only have been flipped by the suggestion.
    let mut document = Document::new("suggestions");
    document.blocks = (0..40)
        .map(|index| {
            let mut block = Block::paragraph("a paragraph the bundled subset covers completely");
            block.id = StableId::parse(format!("block-{index:04}")).expect("valid id");
            block
        })
        .collect();
    let mut cache = LayoutCache::new();
    let clean = cache.layout(&document);
    assert!(clean.blocks.iter().all(|block| block.exact));
    document.suggestions.push(opendoc_core::Suggestion {
        id: StableId::parse("sugg-1").expect("valid id"),
        author: "someone".to_string(),
        kind: opendoc_core::SuggestionKind::Insert {
            anchor: opendoc_core::Anchor::Document,
            content: vec![Inline::text("inserted")],
        },
        state: opendoc_core::SuggestionState::Proposed,
        provenance: Vec::new(),
    });
    let suggested = cache.layout(&document);
    assert_eq!(suggested, layout_document(&document));
    assert!(suggested.blocks.iter().all(|block| !block.exact));
    assert_eq!(cache.stats().reused, 0);
}

/// `clear` means what it says, and a cleared cache still produces the same
/// layout — which is the statement that the cache is an optimisation and not
/// a source of truth.
#[test]
fn clearing_the_cache_changes_nothing_but_the_work() {
    let document = generated_document(41, PageSetup::default(), 50);
    let mut cache = LayoutCache::new();
    let first = cache.layout(&document);
    cache.clear();
    assert_eq!(cache.stats(), Default::default());
    let second = cache.layout(&document);
    assert_eq!(first, second);
    assert_eq!(cache.stats().reused, 0);
    assert_eq!(cache.stats().measured, document.blocks.len());
}
