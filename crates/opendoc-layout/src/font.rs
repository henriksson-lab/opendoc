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

const SANS_REGULAR: &[u8] = include_bytes!("../fonts/opendoc-sans-regular.ttf");
const SANS_BOLD: &[u8] = include_bytes!("../fonts/opendoc-sans-bold.ttf");
const SANS_ITALIC: &[u8] = include_bytes!("../fonts/opendoc-sans-italic.ttf");
const SANS_BOLD_ITALIC: &[u8] = include_bytes!("../fonts/opendoc-sans-bolditalic.ttf");
const MONO_REGULAR: &[u8] = include_bytes!("../fonts/opendoc-mono-regular.ttf");

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
    /// Advances for printable ASCII, per face.
    ///
    /// Every lookup outside this range costs a `cmap` binary search, and a
    /// document is laid out again on every keystroke, so the characters that
    /// almost all text is made of are resolved once when the faces are
    /// parsed. It is a cache of the same numbers, not a second source: it is
    /// filled from `glyph_hor_advance` and `ascii_cache_agrees_with_the_face`
    /// pins that.
    ascii: [[u16; ASCII_SPAN]; 5],
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
                face.glyph_index(ch)
                    .and_then(|glyph| face.glyph_hor_advance(glyph))
                    .unwrap_or(missing[index])
            })
        });
        Self {
            faces,
            missing,
            ascii,
        }
    }

    /// The cached advance for a printable ASCII character, if this is one.
    fn cached(&self, ch: char, face: usize) -> Option<u16> {
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
        self.faces[Self::index(style)].glyph_index(ch).is_some()
    }

    /// The advance of one character in design units.
    pub fn advance_units(&self, ch: char, style: TextStyle) -> i64 {
        let index = Self::index(style);
        if let Some(advance) = self.cached(ch, index) {
            return i64::from(advance);
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
                let direct = face
                    .glyph_index(ch)
                    .and_then(|glyph| face.glyph_hor_advance(glyph))
                    .unwrap_or(fonts.missing[index]);
                assert_eq!(
                    fonts.ascii[index][(code - ASCII_FIRST) as usize],
                    direct,
                    "face {index} disagrees with its cache for {ch:?}"
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
