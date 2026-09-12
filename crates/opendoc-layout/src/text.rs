//! Line breaking.
//!
//! The editable surface is `white-space: pre-wrap; overflow-wrap: anywhere`,
//! and this module reproduces what that means:
//!
//! - spaces are preserved, not collapsed, and a run of them is a soft wrap
//!   opportunity;
//! - spaces left at the end of a line **hang**: they do not count towards the
//!   line's width and so cannot themselves push a word onto the next line;
//! - a word that does not fit moves to the next line whole, and only a word
//!   with no wrap opportunity before it is broken inside.
//!
//! Widths are accumulated as `advance_units * font_size_twips`, never
//! divided, so mixing sizes and faces on one line costs no precision at all
//! and the comparison against the available width is exact integer
//! arithmetic. Two machines therefore break the same text in the same place;
//! that is the property the whole design rests on.
//!
//! Not implemented, and not pretended: justification, hyphenation, the full
//! UAX #14 class table (CJK, Thai and the other scripts that break without
//! spaces are outside the bundled font's coverage anyway), and bidi
//! reordering, which changes the order glyphs are drawn in but not the width
//! of the line.

use crate::font::{Fonts, TextStyle, UNITS_PER_EM};

/// One piece of a paragraph's inline content.
#[derive(Clone, Debug)]
pub enum Item {
    /// Text to shape, in a single style.
    Text { text: String, style: TextStyle },
    /// An inline box of a known width that cannot be broken — a checkbox, a
    /// rendered equation, a field. `exact` says whether the width is known
    /// or estimated.
    Box { width_twips: i32, exact: bool },
}

impl Item {
    pub fn text(text: impl Into<String>, style: TextStyle) -> Self {
        Self::Text {
            text: text.into(),
            style,
        }
    }
}

/// The result of breaking one block's content.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Measured {
    /// Line boxes, never fewer than one: an empty paragraph still draws a
    /// line, because the renderer emits a `<br>` in it.
    pub lines: u32,
    /// False when some run's width was estimated rather than measured — a
    /// character outside the bundled subset, or an inline box of unknown
    /// size. The caller propagates it so a consumer can tell which page
    /// assignments are guaranteed and which are merely likely.
    pub exact: bool,
}

/// True when a character both preserves its width and offers a wrap
/// opportunity after it. `pre-wrap` preserves the width; U+00A0 and the
/// narrow no-break space deliberately offer no opportunity.
fn is_wrap_space(ch: char) -> bool {
    matches!(
        ch,
        ' ' | '\t'
            | '\u{000C}'
            | '\u{1680}'
            | '\u{2000}'..='\u{2006}'
            | '\u{2008}'..='\u{200A}'
            | '\u{205F}'
            | '\u{3000}'
    )
}

/// True when a character offers a wrap opportunity *after* it without
/// hanging: a hyphen, an en/em dash, a zero-width space.
fn breaks_after(ch: char) -> bool {
    matches!(ch, '-' | '\u{2010}' | '\u{2013}' | '\u{2014}' | '\u{200B}')
}

/// Greedy line breaker over the accumulator described in the module docs.
struct Breaker {
    /// Available width for the first line, in `twips * UNITS_PER_EM`.
    first: i64,
    /// Available width for every later line.
    rest: i64,
    lines: u32,
    /// Width already committed to the current line, excluding `pending`
    /// and `word`.
    line: i64,
    /// Spaces since the last committed word. They hang if the line ends here.
    pending: i64,
    /// The current unbreakable chunk.
    word: i64,
}

impl Breaker {
    fn new(first_twips: i32, rest_twips: i32) -> Self {
        // A content box narrower than a single glyph is still a box; clamping
        // at one unit keeps the "a glyph alone always fits" rule from turning
        // into an unbounded line count.
        Self {
            first: i64::from(first_twips.max(1)) * UNITS_PER_EM,
            rest: i64::from(rest_twips.max(1)) * UNITS_PER_EM,
            lines: 0,
            line: 0,
            pending: 0,
            word: 0,
        }
    }

    fn available(&self) -> i64 {
        if self.lines == 0 {
            self.first
        } else {
            self.rest
        }
    }

    /// Ends the current line. Pending spaces hang and are discarded.
    fn wrap(&mut self) {
        self.lines += 1;
        self.line = 0;
        self.pending = 0;
    }

    fn hard_break(&mut self) {
        self.line += self.pending + self.word;
        self.word = 0;
        self.wrap();
    }

    fn space(&mut self, width: i64) {
        if self.word > 0 {
            self.line += self.pending + self.word;
            self.pending = 0;
            self.word = 0;
        }
        self.pending += width;
    }

    /// Adds one unbreakable advance to the current word.
    fn glyph(&mut self, width: i64) {
        let available = self.available();
        if self.line + self.pending + self.word + width > available {
            if self.line + self.pending > 0 {
                // There is a wrap opportunity before this word: take it, and
                // carry the word to the next line whole.
                self.wrap();
            }
            if self.word > 0 && self.word + width > self.available() {
                // No opportunity left, and the word alone still overflows:
                // `overflow-wrap: anywhere` breaks inside it.
                self.line = self.word;
                self.word = 0;
                self.wrap();
            }
        }
        self.word += width;
    }

    /// Marks a wrap opportunity after the glyph just added, with no hanging.
    fn commit_word(&mut self) {
        self.line += self.pending + self.word;
        self.pending = 0;
        self.word = 0;
    }

    fn finish(mut self) -> u32 {
        self.line += self.pending + self.word;
        self.lines += 1;
        self.lines
    }
}

/// Breaks a block's inline content into lines.
///
/// `first_width_twips` is the first line's content width, which differs from
/// the rest when the block has a first-line indent; a hanging indent makes it
/// the wider of the two.
pub fn measure(
    items: &[Item],
    fonts: &Fonts,
    first_width_twips: i32,
    rest_width_twips: i32,
) -> Measured {
    let mut breaker = Breaker::new(first_width_twips, rest_width_twips);
    let mut exact = true;
    for item in items {
        match item {
            Item::Box {
                width_twips,
                exact: known,
            } => {
                exact &= *known;
                breaker.glyph(i64::from(*width_twips) * UNITS_PER_EM);
            }
            Item::Text { text, style } => {
                for ch in text.chars() {
                    if ch == '\n' {
                        // `render_text_content` turns a newline into a `<br>`,
                        // so it is a hard break rather than preserved space.
                        breaker.hard_break();
                        continue;
                    }
                    if !fonts.covers(ch, *style) {
                        exact = false;
                    }
                    let width = fonts.advance_numerator(ch, *style);
                    if is_wrap_space(ch) {
                        breaker.space(width);
                    } else {
                        breaker.glyph(width);
                        if breaks_after(ch) {
                            breaker.commit_word();
                        }
                    }
                }
            }
        }
    }
    Measured {
        lines: breaker.finish(),
        exact,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body() -> TextStyle {
        TextStyle::new(220)
    }

    fn lines(text: &str, width_twips: i32) -> u32 {
        let fonts = Fonts::load();
        measure(
            &[Item::text(text, body())],
            &fonts,
            width_twips,
            width_twips,
        )
        .lines
    }

    #[test]
    fn empty_content_is_one_line() {
        let fonts = Fonts::load();
        assert_eq!(measure(&[], &fonts, 9360, 9360).lines, 1);
    }

    #[test]
    fn short_text_is_one_line() {
        assert_eq!(lines("hello world", 9360), 1);
    }

    #[test]
    fn a_narrow_box_wraps_at_spaces() {
        // Six words that cannot all fit in half an inch.
        let narrow = lines("alpha beta gamma delta epsilon zeta", 720);
        assert!(narrow >= 5, "expected one word per line, got {narrow}");
        assert_eq!(lines("alpha beta gamma delta epsilon zeta", 9360), 1);
    }

    #[test]
    fn line_breaking_is_deterministic() {
        let text = "The quick brown fox jumps over the lazy dog, repeatedly and at length.";
        let first = lines(text, 2400);
        for _ in 0..5 {
            assert_eq!(lines(text, 2400), first);
        }
        assert!(first > 1);
    }

    #[test]
    fn a_word_longer_than_the_line_breaks_inside_itself() {
        let broken = lines("supercalifragilisticexpialidocious", 720);
        assert!(broken > 1, "an over-wide word did not break, got {broken}");
    }

    /// A style whose every glyph is the same width, so a test can state an
    /// available width in characters and mean it exactly.
    fn mono() -> TextStyle {
        TextStyle {
            mono: true,
            ..TextStyle::new(220)
        }
    }

    /// The width of `count` monospace characters, in twips, rounded **up** so
    /// that exactly `count` characters fit and `count + 1` do not.
    fn mono_twips(count: i64) -> i32 {
        let fonts = Fonts::load();
        let advance = fonts.advance_units('a', mono());
        ((advance * count * 220 + UNITS_PER_EM - 1) / UNITS_PER_EM) as i32
    }

    fn mono_lines(text: &str, columns: i64) -> u32 {
        let fonts = Fonts::load();
        let width = mono_twips(columns);
        measure(&[Item::text(text, mono())], &fonts, width, width).lines
    }

    #[test]
    fn trailing_spaces_hang_instead_of_wrapping() {
        // A line with exactly as many characters as fit, then a pile of
        // spaces after it. Under `pre-wrap` the spaces hang off the end; they
        // must not create a second line.
        assert_eq!(mono_lines("aaaaaaaaaa", 10), 1);
        assert_eq!(mono_lines("aaaaaaaaaa          ", 10), 1);
        // One more non-space character does wrap, so the assertion above is
        // about the spaces and not about a box with room to spare.
        assert_eq!(mono_lines("aaaaaaaaaab", 10), 2);
    }

    #[test]
    fn an_inline_box_takes_room_from_the_line() {
        // A box five columns wide in front of ten columns of text: without it
        // the text is one line, with it the line has to break.
        let fonts = Fonts::load();
        let width = mono_twips(10);
        let text = || Item::text("aaaaaaaaaa", mono());
        assert_eq!(measure(&[text()], &fonts, width, width).lines, 1);
        let boxed = [
            Item::Box {
                width_twips: mono_twips(5),
                exact: true,
            },
            text(),
        ];
        assert_eq!(measure(&boxed, &fonts, width, width).lines, 2);
    }

    #[test]
    fn an_inline_box_of_unknown_width_makes_the_measurement_inexact() {
        let fonts = Fonts::load();
        let estimated = [Item::Box {
            width_twips: 100,
            exact: false,
        }];
        assert!(!measure(&estimated, &fonts, 9360, 9360).exact);
    }

    #[test]
    fn a_newline_is_a_hard_break() {
        assert_eq!(lines("a\nb\nc", 9360), 3);
    }

    #[test]
    fn bold_text_can_need_more_lines_than_regular() {
        let fonts = Fonts::load();
        let text = "The quick brown fox jumps over the lazy dog";
        let regular = measure(&[Item::text(text, body())], &fonts, 2200, 2200);
        let bold = measure(
            &[Item::text(
                text,
                TextStyle {
                    bold: true,
                    ..body()
                },
            )],
            &fonts,
            2200,
            2200,
        );
        assert!(
            bold.lines >= regular.lines,
            "bold {} < regular {}",
            bold.lines,
            regular.lines
        );
    }

    #[test]
    fn a_hanging_indent_gives_the_first_line_more_room() {
        let fonts = Fonts::load();
        let text = "alpha beta gamma delta";
        let hanging = measure(&[Item::text(text, body())], &fonts, 4000, 1200);
        let flat = measure(&[Item::text(text, body())], &fonts, 1200, 1200);
        assert!(hanging.lines < flat.lines);
    }

    #[test]
    fn an_uncovered_character_makes_the_measurement_inexact() {
        let fonts = Fonts::load();
        let measured = measure(&[Item::text("漢字", body())], &fonts, 9360, 9360);
        assert!(!measured.exact);
        assert!(measure(&[Item::text("latin", body())], &fonts, 9360, 9360).exact);
    }

    #[test]
    fn a_hyphen_is_a_wrap_opportunity_and_a_plain_letter_is_not() {
        // 19 characters in a 12-column box. Breaking anywhere fills both
        // lines and needs two; breaking *after the hyphen* strands the first
        // six characters on a line of their own and needs three. The line
        // count is what tells the two apart.
        assert_eq!(mono_lines("aaaaaxbbbbbbbbbbbbb", 12), 2);
        assert_eq!(mono_lines("aaaaa-bbbbbbbbbbbbb", 12), 3);
    }
}
