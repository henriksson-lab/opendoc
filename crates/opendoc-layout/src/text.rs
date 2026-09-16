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

use crate::font::{milli_twips_to_layout_units, Fonts, LineExtent, TextStyle, UNITS_PER_EM};
use crate::paint::RunDecoration;

/// What a CSS `line-height` means for the boxes on a line.
///
/// The distinction matters and is not cosmetic: a *unitless* line height
/// multiplies each inline box's **own** font size, so an 18pt run inside an
/// 11pt paragraph asks for a 36px line and gets one, while a line height
/// stated as a length is inherited as that same length by every box on the
/// line. Both forms still produce a different half-leading per box, because
/// the faces' content boxes differ.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Leading {
    /// Thousandths of each box's own size — a unitless `line-height`.
    Multiple(u32),
    /// The same used value for every box, in twips.
    Fixed(i32),
}

impl Leading {
    /// This leading's used value for a box of `size_twips`, on Chrome's
    /// 1/64-px grid.
    pub fn line_height_units(self, size_twips: i32) -> i64 {
        let milli = match self {
            Leading::Multiple(thousandths) => i64::from(size_twips) * i64::from(thousandths),
            Leading::Fixed(twips) => i64::from(twips) * 1_000,
        };
        milli_twips_to_layout_units(milli)
    }
}

/// One piece of a paragraph's inline content.
#[derive(Clone, Debug)]
pub enum Item {
    /// Text to shape, in a single style.
    Text {
        text: String,
        style: TextStyle,
        /// What the run looks like beyond its advances, and how far off the
        /// baseline it sits. The shift is part of the *measurement* — a
        /// raised box makes the line box taller — so it travels with the
        /// item rather than being applied when the line is drawn.
        decoration: RunDecoration,
    },
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
            decoration: RunDecoration::default(),
        }
    }

    pub fn decorated(text: impl Into<String>, style: TextStyle, decoration: RunDecoration) -> Self {
        Self::Text {
            text: text.into(),
            style,
            decoration,
        }
    }
}

/// The result of breaking one block's content.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Measured {
    /// Line boxes, never fewer than one: an empty paragraph still draws a
    /// line, because the renderer emits a `<br>` in it.
    pub lines: u32,
    /// The block's text height in layout units: the sum of the line boxes,
    /// each of which is the union of the strut and every inline box on it.
    /// **Not** a line count times one leading — that is only true of a block
    /// whose every run is set at the block's own size in the block's own
    /// face.
    pub height_units: i64,
    /// False when some run's width was estimated rather than measured — a
    /// character outside the bundled subset, or an inline box of unknown
    /// size. The caller propagates it so a consumer can tell which page
    /// assignments are guaranteed and which are merely likely.
    pub exact: bool,
}

/// One drawable piece of a broken line, in the order it is drawn.
///
/// This is the *same* content the width accumulator above consumed, recorded
/// as it is consumed rather than reconstructed afterwards: a second pass that
/// re-split the text would be a second line breaker, which is exactly the
/// thing ADR 0014 exists to avoid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Piece {
    Text {
        text: String,
        style: TextStyle,
        decoration: RunDecoration,
    },
    /// An inline box: it occupies width but this module does not know how to
    /// draw it. The caller that supplied it does.
    Box { width_twips: i32 },
}

/// One line of a broken block, with the pieces that sit on it.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct LineBox {
    pub pieces: Vec<Piece>,
    /// The line's drawn width, with trailing spaces hanging (they are not
    /// drawn and must not shift a centred or right-aligned line).
    pub width_twips: i32,
    /// How far this line box reaches above and below the baseline it carries,
    /// in layout units. The union of the strut and every box on the line.
    pub extent: LineExtent,
}

/// Records the pieces the breaker commits, line by line.
///
/// Every method mirrors one width operation on [`Breaker`] exactly, so the
/// recorded content cannot disagree with the recorded line count.
#[derive(Default)]
struct Capture {
    lines: Vec<LineBox>,
    line: Vec<Piece>,
    pending: Vec<Piece>,
    word: Vec<Piece>,
}

impl Capture {
    fn push(target: &mut Vec<Piece>, piece: Piece) {
        // Consecutive characters in one style become one piece: a PDF run, a
        // single `Tj`. Merging here rather than in the consumer keeps the
        // line a faithful record of what was measured.
        if let (
            Some(Piece::Text {
                text,
                style,
                decoration,
            }),
            Piece::Text {
                text: more,
                style: next,
                decoration: next_decoration,
            },
        ) = (target.last_mut(), &piece)
        {
            if style == next && decoration == next_decoration {
                text.push_str(more);
                return;
            }
        }
        target.push(piece);
    }

    fn wrap(&mut self, width_twips: i32, extent: LineExtent) {
        self.lines.push(LineBox {
            pieces: std::mem::take(&mut self.line),
            width_twips,
            extent,
        });
        // Pending spaces hang: they are not drawn on the line they end.
        self.pending.clear();
    }

    fn commit(&mut self) {
        for piece in self.pending.drain(..).chain(self.word.drain(..)) {
            Self::push(&mut self.line, piece);
        }
    }

    /// The word carries to the next line whole; the committed part becomes a
    /// line on its own.
    fn break_inside_word(&mut self) {
        self.line = std::mem::take(&mut self.word);
    }
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
    /// Every line box starts as the block's strut, which participates
    /// whatever else is on the line — a paragraph of 8pt text is still 22px
    /// per line in an 11pt block, and Chrome says so.
    strut: LineExtent,
    /// The boxes already committed to this line, as one extent.
    line_extent: LineExtent,
    /// The spaces since the last committed word, and the current unbreakable
    /// chunk. They move between lines with the widths they belong to, so the
    /// extent of a word carried onto the next line is carried with it.
    pending_extent: LineExtent,
    word_extent: LineExtent,
    /// The finished line boxes' heights, summed.
    height: i64,
    /// The pieces behind those widths, recorded only when a caller asked for
    /// them. `None` on the pagination path, which runs on every keystroke and
    /// needs no content at all.
    capture: Option<Capture>,
}

impl Breaker {
    fn new(first_twips: i32, rest_twips: i32, capture: bool, strut: LineExtent) -> Self {
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
            strut,
            line_extent: strut,
            pending_extent: strut,
            word_extent: strut,
            height: 0,
            capture: capture.then(Capture::default),
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
    ///
    /// The spaces hang off the *width*, not out of the line: the run they
    /// belong to still has a box on this line, so its extent stays in the
    /// union. Otherwise a line that happened to break after a big space would
    /// be shorter than the same line broken one glyph earlier.
    fn wrap(&mut self) {
        let width = self.line;
        let extent = self.line_extent.union(self.pending_extent);
        self.height += extent.height();
        if let Some(capture) = self.capture.as_mut() {
            capture.wrap(to_twips(width), extent);
        }
        self.lines += 1;
        self.line = 0;
        self.pending = 0;
        self.line_extent = self.strut;
        self.pending_extent = self.strut;
    }

    fn hard_break(&mut self) {
        self.line += self.pending + self.word;
        self.word = 0;
        self.line_extent = self.line_extent.union(self.word_extent);
        self.word_extent = self.strut;
        if let Some(capture) = self.capture.as_mut() {
            capture.commit();
        }
        self.wrap();
    }

    fn space(&mut self, width: i64, extent: LineExtent, piece: impl FnOnce() -> Piece) {
        if self.word > 0 {
            self.line += self.pending + self.word;
            self.pending = 0;
            self.word = 0;
            self.line_extent = self
                .line_extent
                .union(self.pending_extent)
                .union(self.word_extent);
            self.pending_extent = self.strut;
            self.word_extent = self.strut;
            if let Some(capture) = self.capture.as_mut() {
                capture.commit();
            }
        }
        self.pending += width;
        self.pending_extent = self.pending_extent.union(extent);
        if let Some(capture) = self.capture.as_mut() {
            Capture::push(&mut capture.pending, piece());
        }
    }

    /// Adds one unbreakable advance to the current word.
    ///
    /// The piece is a thunk rather than a value because the pagination path
    /// does not capture: building a `Piece::Text` there allocated a `String`
    /// per character and dropped it unread.
    fn glyph(&mut self, width: i64, extent: LineExtent, piece: impl FnOnce() -> Piece) {
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
                self.line_extent = self.line_extent.union(self.word_extent);
                self.word_extent = self.strut;
                if let Some(capture) = self.capture.as_mut() {
                    capture.break_inside_word();
                }
                self.wrap();
            }
        }
        self.word += width;
        self.word_extent = self.word_extent.union(extent);
        if let Some(capture) = self.capture.as_mut() {
            Capture::push(&mut capture.word, piece());
        }
    }

    /// Marks a wrap opportunity after the glyph just added, with no hanging.
    fn commit_word(&mut self) {
        self.line += self.pending + self.word;
        self.pending = 0;
        self.word = 0;
        self.line_extent = self
            .line_extent
            .union(self.pending_extent)
            .union(self.word_extent);
        self.pending_extent = self.strut;
        self.word_extent = self.strut;
        if let Some(capture) = self.capture.as_mut() {
            capture.commit();
        }
    }

    fn finish(mut self) -> (u32, i64, Vec<LineBox>) {
        // Trailing spaces hang on the last line exactly as they do on every
        // other, so the drawn width excludes them.
        let drawn = self.line + self.word;
        self.line += self.pending + self.word;
        self.lines += 1;
        let extent = self
            .line_extent
            .union(self.pending_extent)
            .union(self.word_extent);
        self.height += extent.height();
        let lines = self.lines;
        let height = self.height;
        let captured = match self.capture.take() {
            Some(mut capture) => {
                capture.commit();
                capture.lines.push(LineBox {
                    pieces: std::mem::take(&mut capture.line),
                    width_twips: to_twips(drawn),
                    extent,
                });
                capture.lines
            }
            None => Vec::new(),
        };
        (lines, height, captured)
    }
}

/// The accumulator's unit (`twips * UNITS_PER_EM`) back to twips.
///
/// This is the one division in the module, and it is deliberately only on the
/// *painting* path: a drawn width positions a centred line, it never decides
/// where a line breaks, so rounding it cannot move a page boundary.
fn to_twips(accumulated: i64) -> i32 {
    let half = UNITS_PER_EM / 2;
    let rounded = if accumulated >= 0 {
        (accumulated + half) / UNITS_PER_EM
    } else {
        (accumulated - half) / UNITS_PER_EM
    };
    rounded.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
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
    strut: Strut,
) -> Measured {
    break_content(
        items,
        fonts,
        first_width_twips,
        rest_width_twips,
        strut,
        false,
    )
    .0
}

/// Breaks a block's inline content into lines **and hands back the lines**.
///
/// Same function, same arithmetic, same decisions as [`measure`] — the only
/// difference is that the pieces are recorded as they are committed. A
/// consumer that wants to draw the text (the PDF writer) therefore draws
/// exactly what was paginated, rather than re-breaking it and hoping to
/// agree.
pub fn break_lines(
    items: &[Item],
    fonts: &Fonts,
    first_width_twips: i32,
    rest_width_twips: i32,
    strut: Strut,
) -> (Measured, Vec<LineBox>) {
    break_content(
        items,
        fonts,
        first_width_twips,
        rest_width_twips,
        strut,
        true,
    )
}

/// The block's own line box: the size its text is set at and the
/// `line-height` every box on its lines resolves against.
///
/// Named rather than passed as two integers because the pair is what decides
/// a line's height, and splitting them invites a caller to measure with one
/// block's size and another block's leading.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Strut {
    pub size_twips: i32,
    pub leading: Leading,
}

impl Strut {
    pub fn new(size_twips: i32, leading: Leading) -> Self {
        Self {
            size_twips,
            leading,
        }
    }

    pub(crate) fn line_extent(self, fonts: &Fonts) -> LineExtent {
        fonts.line_extent(
            TextStyle::new(self.size_twips),
            self.leading.line_height_units(self.size_twips),
        )
    }

    /// The extent one inline box of `style` contributes to a line of this
    /// block, before its baseline shift.
    fn box_extent(self, fonts: &Fonts, style: TextStyle) -> LineExtent {
        fonts.line_extent(style, self.leading.line_height_units(style.size_twips))
    }
}

fn break_content(
    items: &[Item],
    fonts: &Fonts,
    first_width_twips: i32,
    rest_width_twips: i32,
    strut: Strut,
    capture: bool,
) -> (Measured, Vec<LineBox>) {
    let mut breaker = Breaker::new(
        first_width_twips,
        rest_width_twips,
        capture,
        strut.line_extent(fonts),
    );
    let mut exact = true;
    for item in items {
        match item {
            Item::Box {
                width_twips,
                exact: known,
            } => {
                exact &= *known;
                // An inline box takes room on the line but states no height
                // here: the two that exist — a checkbox and a rendered
                // equation — are shorter than the strut and taller than this
                // crate can know, respectively, and the second is already
                // reported as an estimate.
                breaker.glyph(
                    i64::from(*width_twips) * UNITS_PER_EM,
                    LineExtent::default(),
                    || Piece::Box {
                        width_twips: *width_twips,
                    },
                );
            }
            Item::Text {
                text,
                style,
                decoration,
            } => {
                let extent = strut
                    .box_extent(fonts, *style)
                    .shifted(decoration.rise_units);
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
                    let piece = || Piece::Text {
                        text: ch.to_string(),
                        style: *style,
                        decoration: decoration.clone(),
                    };
                    if is_wrap_space(ch) {
                        breaker.space(width, extent, piece);
                    } else {
                        breaker.glyph(width, extent, piece);
                        if breaks_after(ch) {
                            breaker.commit_word();
                        }
                    }
                }
            }
        }
    }
    let (lines, height_units, captured) = breaker.finish();
    (
        Measured {
            lines,
            height_units,
            exact,
        },
        captured,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The block strut every test in this module measures against: 11pt text
    /// on a unitless 1.5 line height, which is the document's own.
    fn strut() -> Strut {
        Strut::new(220, Leading::Multiple(1_500))
    }

    /// `super::measure` against that strut. A local item shadows the glob
    /// import, so every call below reads as it did before the strut became an
    /// argument.
    fn measure(items: &[Item], fonts: &Fonts, first: i32, rest: i32) -> Measured {
        super::measure(items, fonts, first, rest, strut())
    }

    fn break_lines(
        items: &[Item],
        fonts: &Fonts,
        first: i32,
        rest: i32,
    ) -> (Measured, Vec<LineBox>) {
        super::break_lines(items, fonts, first, rest, strut())
    }

    /// Everything the capture records, as one string.
    fn captured_text(lines: &[LineBox]) -> String {
        lines
            .iter()
            .map(|line| {
                line.pieces
                    .iter()
                    .map(|piece| match piece {
                        Piece::Text { text, .. } => text.as_str(),
                        Piece::Box { .. } => "",
                    })
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn capturing_the_lines_does_not_change_where_they_break() {
        // The whole point of recording inside the breaker rather than beside
        // it: a caller that wants the content gets the same decisions as a
        // caller that wants only the count.
        let fonts = Fonts::load();
        let text = "The quick brown fox jumps over the lazy dog and keeps on jumping for a while";
        for width in [600, 1_200, 2_400, 4_800, 9_360] {
            let items = [Item::text(text, body())];
            let counted = measure(&items, &fonts, width, width);
            let (broken, lines) = break_lines(&items, &fonts, width, width);
            assert_eq!(counted, broken, "width {width}");
            assert_eq!(
                counted.lines as usize,
                lines.len(),
                "width {width}: {} lines counted, {} captured",
                counted.lines,
                lines.len()
            );
        }
    }

    #[test]
    fn every_word_survives_the_break() {
        let fonts = Fonts::load();
        let text = "alpha beta gamma delta epsilon zeta eta theta iota kappa";
        let (_, lines) = break_lines(&[Item::text(text, body())], &fonts, 1_600, 1_600);
        assert!(lines.len() > 1, "the fixture did not wrap");
        let captured = captured_text(&lines).replace('\n', " ");
        for word in text.split(' ') {
            assert!(captured.contains(word), "{word:?} was lost: {captured:?}");
        }
    }

    #[test]
    fn a_word_broken_inside_itself_keeps_both_halves() {
        let fonts = Fonts::load();
        // No wrap opportunity anywhere, so `overflow-wrap: anywhere` applies.
        let text = "abcdefghijklmnopqrstuvwxyz";
        let (measured, lines) = break_lines(&[Item::text(text, body())], &fonts, 700, 700);
        assert!(measured.lines > 1, "the fixture did not break inside");
        assert_eq!(text, captured_text(&lines).replace('\n', ""));
    }

    #[test]
    fn trailing_spaces_are_not_drawn_on_the_line_they_end() {
        // They hang: they must not widen the line, or a centred line would
        // sit left of centre by half the spaces.
        let fonts = Fonts::load();
        let (_, hung) = break_lines(&[Item::text("hi   ", body())], &fonts, 9_360, 9_360);
        let (_, bare) = break_lines(&[Item::text("hi", body())], &fonts, 9_360, 9_360);
        assert_eq!(bare[0].width_twips, hung[0].width_twips);
    }

    #[test]
    fn the_spaces_a_line_breaks_at_hang_off_it() {
        // The space between two words sits at the end of the first line. It
        // must neither be drawn there nor counted in that line's width, or a
        // centred paragraph would drift left on every wrapped line.
        let fonts = Fonts::load();
        let width = fonts.text_advance_twips("alpha", body()) + 40;
        let (measured, lines) =
            break_lines(&[Item::text("alpha beta", body())], &fonts, width, width);
        assert_eq!(2, measured.lines, "the fixture did not wrap at the space");
        assert_eq!(
            fonts.text_advance_twips("alpha", body()),
            lines[0].width_twips,
            "the break space was counted into the first line"
        );
        assert_eq!("alpha", captured_text(&lines[..1]));
    }

    #[test]
    fn one_run_of_one_style_is_captured_as_one_piece() {
        // A piece per character would be a `Tj` per character in a PDF.
        let fonts = Fonts::load();
        let (_, lines) = break_lines(&[Item::text("hello world", body())], &fonts, 9_360, 9_360);
        assert_eq!(1, lines[0].pieces.len(), "{:?}", lines[0].pieces);
    }

    #[test]
    fn a_style_change_starts_a_new_piece() {
        let fonts = Fonts::load();
        let bold = TextStyle {
            bold: true,
            ..body()
        };
        let (_, lines) = break_lines(
            &[Item::text("plain", body()), Item::text("strong", bold)],
            &fonts,
            9_360,
            9_360,
        );
        assert_eq!(2, lines[0].pieces.len(), "{:?}", lines[0].pieces);
    }

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
    fn bold_text_needs_more_lines_than_regular_somewhere() {
        // `>=` alone is satisfied by equality — that is, by the bold face
        // never being consulted at all, which is exactly the bug this test is
        // meant to catch. So the claim is made over a family of widths: bold
        // may never be *narrower*, and at some width it has to be wider.
        let fonts = Fonts::load();
        let text = "The quick brown fox jumps over the lazy dog";
        let bold = TextStyle {
            bold: true,
            ..body()
        };
        let mut differed = 0;
        for width in (1_000..4_000).step_by(50) {
            let regular = measure(&[Item::text(text, body())], &fonts, width, width);
            let heavy = measure(&[Item::text(text, bold)], &fonts, width, width);
            assert!(
                heavy.lines >= regular.lines,
                "at {width} twips bold took {} lines and regular {}",
                heavy.lines,
                regular.lines
            );
            if heavy.lines > regular.lines {
                differed += 1;
            }
        }
        assert!(
            differed > 0,
            "bold never cost a line at any width, so it was measured with the regular face"
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
