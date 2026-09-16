//! The bundled document faces and their metrics.
//!
//! Deterministic pagination needs font metrics, and metrics are only useful
//! if the browser renders with *the same* font. That loop is closed by
//! bundling one set of faces and shipping them twice from a single subset:
//! the TrueType bytes embedded here, and the WOFF2 files in
//! `apps/desktop/src/fonts/` that the stylesheet loads through `@font-face`.
//! Both come out of one run of `fonts/generate.py`, so their `hmtx` advances
//! are the same numbers by construction.
//!
//! The subsets carry **no `GSUB`/`GPOS` and no `kern`**, so there is no
//! kerning and no ligature for the browser to apply that this module has not
//! accounted for: a run's width is the sum of its glyphs' advances, exactly.

use ttf_parser::Face;

/// The family name the stylesheet must ask for. The upstream faces are
/// Liberation, whose OFL reserves that name for unmodified versions; a subset
/// is a modification, so the bundled family is renamed.
pub const SANS_FAMILY: &str = "OpenDoc Sans";
/// The family used for `MarkKind::Code` runs.
pub const MONO_FAMILY: &str = "OpenDoc Mono";

// `static`, not `const`: a `const` is inlined at each use site, and each
// inlining is a *fresh* anonymous allocation of the bytes. These faces are
// used from two places — `Fonts::load`, which measures with them, and
// `FaceId::bytes`, which hands them to a PDF to embed — so as consts they
// landed in the WebAssembly data section twice, ~150 KB of duplicated
// TrueType. A `static` has one address and is emitted once.
static SANS_REGULAR: &[u8] = include_bytes!("../fonts/opendoc-sans-regular.ttf");
static SANS_BOLD: &[u8] = include_bytes!("../fonts/opendoc-sans-bold.ttf");
static SANS_ITALIC: &[u8] = include_bytes!("../fonts/opendoc-sans-italic.ttf");
static SANS_BOLD_ITALIC: &[u8] = include_bytes!("../fonts/opendoc-sans-bolditalic.ttf");
static MONO_REGULAR: &[u8] = include_bytes!("../fonts/opendoc-mono-regular.ttf");

/// Chrome's layout grid: a `LayoutUnit` is 1/64 of a CSS pixel, and every
/// vertical length Blink computes is quantised onto it. A line box's height
/// is therefore not a real number the browser rounds at paint time — it is an
/// exact multiple of 1/64 px that the browser *computes with*, so a layout
/// engine that wants to agree with the browser has to compute on the same
/// grid. [`Fonts::line_extent`] does.
pub const LAYOUT_UNITS_PER_PX: i64 = 64;

/// Milli-twips in one CSS reference pixel: 1px is 0.75pt is 15 twips.
pub const MILLI_TWIPS_PER_PX: i64 = 15_000;

/// Milli-twips onto Chrome's 1/64-px grid, rounded down — which is what
/// `LayoutUnit(float)` does to a computed length.
pub fn milli_twips_to_layout_units(milli_twips: i64) -> i64 {
    (milli_twips * LAYOUT_UNITS_PER_PX).div_euclid(MILLI_TWIPS_PER_PX)
}

/// Chrome's 1/64-px grid back to milli-twips, rounded to the nearest.
///
/// Exact for any length that is a whole number of eighths of a pixel, which
/// covers every line box the bundled type scale produces; the residue on
/// anything else is 1/64 px at worst, and it is taken once per converted
/// value rather than accumulated.
pub fn layout_units_to_milli_twips(units: i64) -> i64 {
    (units * MILLI_TWIPS_PER_PX + LAYOUT_UNITS_PER_PX / 2).div_euclid(LAYOUT_UNITS_PER_PX)
}

/// Chrome's 1/64-px grid to whole twips, for a consumer that draws in twips.
pub fn layout_units_to_twips(units: i64) -> i32 {
    crate::to_twips(layout_units_to_milli_twips(units))
}

/// How far above and below its baseline one inline box reaches, in layout
/// units.
///
/// A line box is the **union** of these over the block's strut and every
/// inline box on the line — not one leading applied to the whole block. That
/// distinction is the difference between 22px and 36px on a line carrying an
/// 18pt run, and between 22px and 23px on a line carrying a monospace one.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LineExtent {
    /// Distance from the baseline up to the top of the box.
    pub above: i64,
    /// Distance from the baseline down to its bottom.
    pub below: i64,
}

impl LineExtent {
    /// The line box two boxes sharing a baseline need between them.
    pub fn union(self, other: Self) -> Self {
        Self {
            above: self.above.max(other.above),
            below: self.below.max(other.below),
        }
    }

    pub fn height(self) -> i64 {
        self.above + self.below
    }

    /// The same box, raised (positive) or lowered (negative) off the
    /// baseline it shares with its parent.
    pub fn shifted(self, rise: i64) -> Self {
        Self {
            above: self.above + rise,
            below: self.below - rise,
        }
    }
}

/// Which way a `vertical-align` keyword moves a box off its parent baseline.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Script {
    Super,
    Sub,
}

/// The baseline shift Chrome gives `vertical-align: super` / `sub`, in layout
/// units, positive upwards.
///
/// Blink derives it from the **parent's** font size and from nothing else —
/// not from the raised run's own size, and not from any font metric: a
/// superscript inside an 11pt monospace paragraph is shifted by exactly as
/// much as one inside an 11pt sans paragraph. That was measured in Chrome 147
/// across ten sizes from 8pt to 48pt and two families, and the integer form
/// below reproduces all twenty measurements exactly, including the
/// truncations.
pub fn script_shift_units(parent_size_twips: i32, script: Script) -> i64 {
    let font = round_div(
        i64::from(parent_size_twips) * LAYOUT_UNITS_PER_PX,
        MILLI_TWIPS_PER_PX / 1_000,
    );
    match script {
        Script::Super => font / 3 + LAYOUT_UNITS_PER_PX,
        Script::Sub => -(font / 5 + LAYOUT_UNITS_PER_PX),
    }
}

/// Integer division rounded half away from zero, which is what `lroundf` —
/// the call Blink rounds a font's ascent and descent with — does.
fn round_div(value: i64, divisor: i64) -> i64 {
    if value >= 0 {
        (value + divisor / 2) / divisor
    } else {
        (value - divisor / 2) / divisor
    }
}

/// Design units per em shared by every bundled face.
///
/// Sharing one value is what lets a line's width be accumulated as an exact
/// integer across runs of different sizes and faces: the accumulator counts
/// `twips * UNITS_PER_EM`, so no division — and therefore no rounding — ever
/// happens inside a line. [`Fonts::load`] checks the invariant, and
/// `font_faces_share_units_per_em` pins it.
pub const UNITS_PER_EM: i64 = 2048;

/// How a run of text is drawn: everything that changes its measured width.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TextStyle {
    /// Used font size in twips, after any superscript/subscript reduction.
    pub size_twips: i32,
    pub bold: bool,
    pub italic: bool,
    /// A `MarkKind::Code` run, drawn in the bundled monospace face.
    pub mono: bool,
}

impl TextStyle {
    pub fn new(size_twips: i32) -> Self {
        Self {
            size_twips,
            bold: false,
            italic: false,
            mono: false,
        }
    }
}

/// The bundled faces, parsed.
///
/// Parsing is a table-directory walk, so constructing this per layout pass is
/// cheap and avoids any global mutable state — which matters because the same
/// code runs in a browser, in a Tauri shell and in tests.
pub struct Fonts {
    faces: [Face<'static>; 5],
    /// Advance used for a character no bundled face covers.
    missing: [u16; 5],
    /// Advances *and coverage* for printable ASCII, per face.
    ///
    /// Every lookup outside this range costs a `cmap` binary search, and a
    /// document is laid out again on every keystroke, so the characters that
    /// almost all text is made of are resolved once when the faces are
    /// parsed. It is a cache of the same numbers, not a second source: it is
    /// filled from `glyph_index`/`glyph_hor_advance` and
    /// `ascii_cache_agrees_with_the_face` pins that.
    ///
    /// Coverage is cached alongside the advance because the line breaker asks
    /// both questions about every character it measures — the advance, and
    /// whether the face covers it (a character it does not cover is drawn by
    /// the browser in a font this crate cannot measure, so the block's height
    /// stops being exact). Answering the second from the `cmap` undid the
    /// first: the two lookups were the same binary search, done twice.
    ascii: [[AsciiMetric; ASCII_SPAN]; 5],
}

/// One printable-ASCII character's cached metrics for one face.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct AsciiMetric {
    /// The face's advance, or the missing-character advance when the face
    /// does not cover the character — the same fallback the uncached path
    /// applies, so a cached lookup and an uncached one cannot disagree.
    advance: u16,
    /// Whether the face actually covers the character.
    covered: bool,
}

/// U+0020 through U+007E.
const ASCII_FIRST: u32 = 0x20;
const ASCII_LAST: u32 = 0x7E;
const ASCII_SPAN: usize = (ASCII_LAST - ASCII_FIRST + 1) as usize;

const REGULAR: usize = 0;
const BOLD: usize = 1;
const ITALIC: usize = 2;
const BOLD_ITALIC: usize = 3;
const MONO: usize = 4;

/// Which bundled face a run is drawn with.
///
/// A [`TextStyle`] resolves to exactly one of these, and a consumer that has
/// to *draw* rather than merely measure (the PDF writer) needs to name the
/// face it is embedding. Making it an enum rather than an index means a new
/// face cannot be added without every match site deciding what it is.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum FaceId {
    SansRegular,
    SansBold,
    SansItalic,
    SansBoldItalic,
    MonoRegular,
}

impl FaceId {
    pub const ALL: [FaceId; 5] = [
        FaceId::SansRegular,
        FaceId::SansBold,
        FaceId::SansItalic,
        FaceId::SansBoldItalic,
        FaceId::MonoRegular,
    ];

    fn index(self) -> usize {
        match self {
            FaceId::SansRegular => REGULAR,
            FaceId::SansBold => BOLD,
            FaceId::SansItalic => ITALIC,
            FaceId::SansBoldItalic => BOLD_ITALIC,
            FaceId::MonoRegular => MONO,
        }
    }

    /// The face that resolves this style. The same mapping the measurement
    /// uses, so a drawn run is drawn with the face it was measured with.
    pub fn of(style: TextStyle) -> Self {
        FaceId::ALL[Fonts::index(style)]
    }

    /// The TrueType bytes to embed. These are the same bytes the metrics
    /// above were read from, so an embedding consumer cannot ship a face that
    /// disagrees with the pagination.
    pub fn bytes(self) -> &'static [u8] {
        match self {
            FaceId::SansRegular => SANS_REGULAR,
            FaceId::SansBold => SANS_BOLD,
            FaceId::SansItalic => SANS_ITALIC,
            FaceId::SansBoldItalic => SANS_BOLD_ITALIC,
            FaceId::MonoRegular => MONO_REGULAR,
        }
    }

    /// A PostScript-safe name for the embedded font, unique per face.
    pub fn postscript_name(self) -> &'static str {
        match self {
            FaceId::SansRegular => "OpenDocSans",
            FaceId::SansBold => "OpenDocSans-Bold",
            FaceId::SansItalic => "OpenDocSans-Italic",
            FaceId::SansBoldItalic => "OpenDocSans-BoldItalic",
            FaceId::MonoRegular => "OpenDocMono",
        }
    }

    pub fn is_bold(self) -> bool {
        matches!(self, FaceId::SansBold | FaceId::SansBoldItalic)
    }

    pub fn is_italic(self) -> bool {
        matches!(self, FaceId::SansItalic | FaceId::SansBoldItalic)
    }

    pub fn is_monospaced(self) -> bool {
        matches!(self, FaceId::MonoRegular)
    }
}

/// The metrics a font descriptor needs, in design units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FaceMetrics {
    pub ascent: i16,
    /// Negative, as the font states it.
    pub descent: i16,
    pub cap_height: i16,
    pub x_min: i16,
    pub y_min: i16,
    pub x_max: i16,
    pub y_max: i16,
    /// Degrees, negative for the usual forward slant.
    pub italic_angle: f32,
    /// Where the face puts an underline and a strikethrough, and how thick.
    /// The browser draws these from the face's own tables, so a PDF that
    /// guessed would draw the rule in a different place from the screen.
    pub underline_position: i16,
    pub underline_thickness: i16,
    pub strikeout_position: i16,
    pub strikeout_thickness: i16,
}

impl Fonts {
    /// Parses the embedded faces.
    ///
    /// The bytes are compiled in, so a parse failure is a corrupt build
    /// artifact rather than a runtime condition a caller could handle — hence
    /// the panic rather than a `Result` threaded through every layout call.
    pub fn load() -> Self {
        let parse = |bytes: &'static [u8], name: &str| {
            Face::parse(bytes, 0).unwrap_or_else(|err| panic!("bundled face {name}: {err}"))
        };
        let faces = [
            parse(SANS_REGULAR, "sans regular"),
            parse(SANS_BOLD, "sans bold"),
            parse(SANS_ITALIC, "sans italic"),
            parse(SANS_BOLD_ITALIC, "sans bold italic"),
            parse(MONO_REGULAR, "mono regular"),
        ];
        for face in &faces {
            assert_eq!(
                i64::from(face.units_per_em()),
                UNITS_PER_EM,
                "a bundled face does not use {UNITS_PER_EM} units per em"
            );
        }
        // A character outside the subset is drawn by the browser in whatever
        // font it falls back to, which this crate cannot measure. Using the
        // width of U+FFFD keeps the estimate finite and stable rather than
        // zero, and `BlockLayout::exact` reports the uncertainty upward.
        let missing = std::array::from_fn(|index| {
            let face: &Face<'static> = &faces[index];
            face.glyph_index('\u{FFFD}')
                .or_else(|| face.glyph_index('?'))
                .and_then(|glyph| face.glyph_hor_advance(glyph))
                .unwrap_or(0)
        });
        let ascii = std::array::from_fn(|index| {
            let face: &Face<'static> = &faces[index];
            std::array::from_fn(|offset| {
                let ch = char::from_u32(ASCII_FIRST + offset as u32).expect("ascii");
                let glyph = face.glyph_index(ch);
                AsciiMetric {
                    advance: glyph
                        .and_then(|glyph| face.glyph_hor_advance(glyph))
                        .unwrap_or(missing[index]),
                    covered: glyph.is_some(),
                }
            })
        });
        Self {
            faces,
            missing,
            ascii,
        }
    }

    /// The cached metrics for a printable ASCII character, if this is one.
    fn cached(&self, ch: char, face: usize) -> Option<AsciiMetric> {
        let code = u32::from(ch);
        (ASCII_FIRST..=ASCII_LAST)
            .contains(&code)
            .then(|| self.ascii[face][(code - ASCII_FIRST) as usize])
    }

    fn index(style: TextStyle) -> usize {
        if style.mono {
            MONO
        } else {
            match (style.bold, style.italic) {
                (false, false) => REGULAR,
                (true, false) => BOLD,
                (false, true) => ITALIC,
                (true, true) => BOLD_ITALIC,
            }
        }
    }

    /// True when every bundled face that could draw this character does.
    pub fn covers(&self, ch: char, style: TextStyle) -> bool {
        let index = Self::index(style);
        if let Some(metric) = self.cached(ch, index) {
            return metric.covered;
        }
        self.faces[index].glyph_index(ch).is_some()
    }

    /// The advance of one character in design units.
    pub fn advance_units(&self, ch: char, style: TextStyle) -> i64 {
        let index = Self::index(style);
        if let Some(metric) = self.cached(ch, index) {
            return i64::from(metric.advance);
        }
        let face = &self.faces[index];
        let advance = face
            .glyph_index(ch)
            .and_then(|glyph| face.glyph_hor_advance(glyph))
            .unwrap_or(self.missing[index]);
        i64::from(advance)
    }

    /// The advance of one character in the line accumulator's unit,
    /// `twips * UNITS_PER_EM`. No division happens here, so a line width is
    /// an exact integer however many runs and sizes it mixes.
    pub fn advance_numerator(&self, ch: char, style: TextStyle) -> i64 {
        self.advance_units(ch, style) * i64::from(style.size_twips)
    }

    /// A run's drawn width in twips.
    ///
    /// The same advances the line breaker summed, divided down once at the
    /// end. This is a *drawing* width: it positions the next run on a line
    /// that has already been broken, so its rounding cannot move a break.
    pub fn text_advance_twips(&self, text: &str, style: TextStyle) -> i32 {
        let units: i64 = text.chars().map(|ch| self.advance_units(ch, style)).sum();
        let numerator = units * i64::from(style.size_twips);
        let half = UNITS_PER_EM / 2;
        let rounded = if numerator >= 0 {
            (numerator + half) / UNITS_PER_EM
        } else {
            (numerator - half) / UNITS_PER_EM
        };
        rounded.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
    }

    /// The glyph a character resolves to in one face, or `0` (`.notdef`) when
    /// the face does not cover it.
    ///
    /// Drawing consumers need the glyph *id*, not the advance: an embedded
    /// TrueType font is addressed by glyph, which is also what makes a
    /// character outside the browser's fallback impossible to misdraw — it
    /// comes out as `.notdef`, visibly, rather than as some other letter.
    pub fn glyph_id(&self, ch: char, face: FaceId) -> u16 {
        self.faces[face.index()]
            .glyph_index(ch)
            .map(|glyph| glyph.0)
            .unwrap_or(0)
    }

    /// A glyph's advance in design units.
    pub fn glyph_advance_units(&self, glyph: u16, face: FaceId) -> u16 {
        self.faces[face.index()]
            .glyph_hor_advance(ttf_parser::GlyphId(glyph))
            .unwrap_or(0)
    }

    /// The face's own metrics, for a font descriptor.
    pub fn metrics(&self, face: FaceId) -> FaceMetrics {
        let parsed = &self.faces[face.index()];
        let bbox = parsed.global_bounding_box();
        let underline = parsed.underline_metrics();
        let strikeout = parsed.strikeout_metrics();
        FaceMetrics {
            underline_position: underline.map(|m| m.position).unwrap_or(-150),
            underline_thickness: underline.map(|m| m.thickness).unwrap_or(100),
            // Chrome falls back to half the x-height above the baseline when
            // the face states no strikeout, which is what these stand-ins are.
            strikeout_position: strikeout
                .map(|m| m.position)
                .unwrap_or_else(|| parsed.x_height().unwrap_or(1_000) / 2),
            strikeout_thickness: strikeout.map(|m| m.thickness).unwrap_or(100),
            ascent: parsed.ascender(),
            descent: parsed.descender(),
            cap_height: parsed.capital_height().unwrap_or(parsed.ascender()),
            x_min: bbox.x_min,
            y_min: bbox.y_min,
            x_max: bbox.x_max,
            y_max: bbox.y_max,
            italic_angle: parsed.italic_angle(),
        }
    }

    /// How far this style's inline box reaches above and below the baseline,
    /// in a line box `line_height_units` tall.
    ///
    /// This is Blink's arithmetic, not an approximation of it. Blink rounds a
    /// face's ascent and descent to **whole pixels** (`lroundf`), splits the
    /// remaining leading in half, and then *floors the result to a whole
    /// pixel* before taking the descent as whatever is left of the line
    /// height (`FontHeight::AddLeading`). Every one of those three
    /// quantisations is visible on screen — the whole-pixel ascent is what
    /// makes a monospace run one pixel taller than the sans run beside it —
    /// so reproducing them is the difference between agreeing with Chrome and
    /// resembling it. Checked against Chrome 147 over sizes, faces, unitless
    /// and fixed line heights.
    pub fn line_extent(&self, style: TextStyle, line_height_units: i64) -> LineExtent {
        let metrics = self.metrics(FaceId::of(style));
        let per_px = UNITS_PER_EM * (MILLI_TWIPS_PER_PX / 1_000);
        let whole_px = |units: i16| -> i64 {
            round_div(i64::from(units) * i64::from(style.size_twips), per_px) * LAYOUT_UNITS_PER_PX
        };
        let ascent = whole_px(metrics.ascent);
        let descent = whole_px(-metrics.descent);
        // `LayoutUnit::operator/` truncates towards zero, which matters when
        // the line height is shorter than the face's own content box.
        let half_leading = (line_height_units - ascent - descent) / 2;
        let above = (ascent + half_leading).div_euclid(LAYOUT_UNITS_PER_PX) * LAYOUT_UNITS_PER_PX;
        LineExtent {
            above,
            below: line_height_units - above,
        }
    }

    /// The box a run paints its background over: the face's ascent above the
    /// baseline and its descent below, at the used size, in twips.
    ///
    /// This is the inline box's *content* box — what Chrome fills with a
    /// highlight — and not the line box, which also carries half-leading.
    /// Quantised to whole pixels for the same reason the line box is: that is
    /// what the browser draws.
    pub fn content_box_twips(&self, style: TextStyle) -> (i32, i32) {
        let metrics = self.metrics(FaceId::of(style));
        let per_px = UNITS_PER_EM * (MILLI_TWIPS_PER_PX / 1_000);
        let whole_px = |units: i16| -> i32 {
            (round_div(i64::from(units) * i64::from(style.size_twips), per_px)
                * (MILLI_TWIPS_PER_PX / 1_000)) as i32
        };
        (whole_px(metrics.ascent), whole_px(-metrics.descent))
    }

    /// A face-stated offset from the baseline, at the used size, in twips.
    /// Positive is above the baseline, which is how the face states it.
    pub fn design_units_to_twips(&self, units: i16, style: TextStyle) -> i32 {
        (i64::from(units) * i64::from(style.size_twips) / UNITS_PER_EM) as i32
    }

    /// The distance from a line box's top to the baseline it carries, in the
    /// same milli-twip unit the block flow uses.
    ///
    /// The strut's own half-leading: used for the one-line boxes that carry a
    /// single style (an image caption, a list marker, an equation's source).
    pub fn baseline_offset_milli(&self, style: TextStyle, leading_milli: i64) -> i64 {
        let units = milli_twips_to_layout_units(leading_milli);
        layout_units_to_milli_twips(self.line_extent(style, units).above)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_faces_share_units_per_em() {
        // `load` asserts it; this states the invariant as a test so the
        // failure names the reason rather than arriving as a panic in an
        // unrelated layout test.
        let fonts = Fonts::load();
        assert!(fonts.covers('A', TextStyle::new(220)));
    }

    #[test]
    fn bold_is_wider_than_regular() {
        let fonts = Fonts::load();
        let regular = TextStyle::new(220);
        let bold = TextStyle {
            bold: true,
            ..regular
        };
        assert!(
            fonts.advance_units('m', bold) > fonts.advance_units('m', regular),
            "the bold face is not a different face"
        );
    }

    #[test]
    fn monospace_advances_are_uniform() {
        let fonts = Fonts::load();
        let style = TextStyle {
            mono: true,
            ..TextStyle::new(220)
        };
        let i = fonts.advance_units('i', style);
        let m = fonts.advance_units('m', style);
        assert_eq!(i, m, "the bundled mono face is not monospaced");
    }

    #[test]
    fn an_uncovered_character_still_has_a_width() {
        let fonts = Fonts::load();
        let style = TextStyle::new(220);
        assert!(!fonts.covers('漢', style));
        assert!(fonts.advance_units('漢', style) > 0);
    }

    #[test]
    fn ascii_cache_agrees_with_the_face() {
        // The cache exists for speed, so it must be the same numbers the face
        // reports; anything else is a second source of truth for the one
        // thing this crate cannot be wrong about.
        let fonts = Fonts::load();
        for (index, face) in fonts.faces.iter().enumerate() {
            for code in ASCII_FIRST..=ASCII_LAST {
                let ch = char::from_u32(code).expect("ascii");
                let glyph = face.glyph_index(ch);
                let direct = AsciiMetric {
                    advance: glyph
                        .and_then(|glyph| face.glyph_hor_advance(glyph))
                        .unwrap_or(fonts.missing[index]),
                    covered: glyph.is_some(),
                };
                assert_eq!(
                    fonts.ascii[index][(code - ASCII_FIRST) as usize],
                    direct,
                    "face {index} disagrees with its cache for {ch:?}"
                );
            }
        }
    }

    #[test]
    fn cached_coverage_agrees_with_the_face_for_every_style() {
        // `covers` answers from the ASCII cache, and the line breaker asks it
        // about every character it measures — so if the cache disagreed with
        // the `cmap` for one character in one face, a block's `exact` flag
        // would flip on text that is perfectly measurable. The two paths must
        // give the same answer, character for character and face for face.
        let fonts = Fonts::load();
        let styles = [
            TextStyle::new(220),
            TextStyle {
                bold: true,
                ..TextStyle::new(220)
            },
            TextStyle {
                italic: true,
                ..TextStyle::new(220)
            },
            TextStyle {
                bold: true,
                italic: true,
                ..TextStyle::new(220)
            },
            TextStyle {
                mono: true,
                ..TextStyle::new(220)
            },
        ];
        for style in styles {
            let face = &fonts.faces[Fonts::index(style)];
            for code in ASCII_FIRST..=ASCII_LAST {
                let ch = char::from_u32(code).expect("ascii");
                assert_eq!(
                    fonts.covers(ch, style),
                    face.glyph_index(ch).is_some(),
                    "cached coverage disagrees with the face for {ch:?}"
                );
            }
            // And the answer for a character outside the cache still comes
            // from the face, so the fast path did not become the only path.
            assert!(!fonts.covers('\u{4E2D}', style));
        }
    }

    /// The three glyphs `list-style-type` draws, in every bundled face.
    ///
    /// These are the disc, the hollow circle and the square, stated as
    /// literal code points rather than read back from `lists::bullet_glyph`:
    /// a test that asked the list module which glyphs it wanted and then
    /// asked the font about exactly those would still pass with all three
    /// dropped from `fonts/generate.py`, which is the failure it exists to
    /// catch. `opendoc-layout` paints these on paper and Chrome draws them
    /// from the same subset on screen, so a face that does not carry one
    /// makes the two disagree — which is what U+25E6 did until it was added
    /// to `UNICODES`: the browser drew `◦` from a fallback font and the PDF
    /// drew `•`.
    ///
    /// Every face, not only the regular one: a `::marker` inherits the list
    /// item's own weight and style, so a bold item's bullet is drawn with the
    /// bold face.
    #[test]
    fn the_subset_carries_every_glyph_the_bullet_cycle_asks_for() {
        let fonts = Fonts::load();
        let base = TextStyle::new(220);
        let styles = [
            ("sans regular", base),
            ("sans bold", TextStyle { bold: true, ..base }),
            (
                "sans italic",
                TextStyle {
                    italic: true,
                    ..base
                },
            ),
            (
                "sans bold italic",
                TextStyle {
                    bold: true,
                    italic: true,
                    ..base
                },
            ),
            ("mono regular", TextStyle { mono: true, ..base }),
        ];
        for (face, style) in styles {
            for (ch, keyword) in [
                ('\u{2022}', "disc"),
                ('\u{25E6}', "circle"),
                ('\u{25A0}', "square"),
            ] {
                assert!(
                    fonts.covers(ch, style),
                    "the {face} subset does not carry {ch:?} (U+{:04X}), which CSS draws for \
                     `list-style-type: {keyword}` — the marker on paper and the marker on \
                     screen would be different glyphs. Add the code point to `UNICODES` in \
                     crates/opendoc-layout/fonts/generate.py and regenerate.",
                    u32::from(ch),
                );
            }
        }
    }

    #[test]
    fn latin_coverage_includes_accents_and_punctuation() {
        let fonts = Fonts::load();
        let style = TextStyle::new(220);
        for ch in ['A', 'z', 'é', 'ß', 'Ł', '—', '“', '”', '…', '€', '•'] {
            assert!(fonts.covers(ch, style), "{ch} is not in the bundled subset");
        }
    }
}
