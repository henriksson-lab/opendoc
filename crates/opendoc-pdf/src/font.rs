//! Embedding the bundled document faces in a PDF.
//!
//! The faces are the ones `opendoc-layout` paginated with — literally the same
//! bytes, handed over by [`FaceId::bytes`]. That is what makes a PDF page
//! agree with the screen instead of resembling it: the advances the page
//! breaks were computed from are the advances the reader will use, because
//! they came out of one file.
//!
//! Every face is embedded as a **composite (Type0) font with Identity-H
//! encoding**, not a simple font. A simple font addresses at most 256 glyphs
//! through a byte encoding, and the bundled subset carries 453 — Latin-1,
//! Latin Extended-A, the punctuation and currency a word processor emits. A
//! composite font addresses glyphs directly, so a document with an é and a €
//! and a ≤ on one line needs no re-encoding tricks and no second font object.
//!
//! Direct glyph addressing costs one thing, and it is paid here: a PDF that
//! names glyphs rather than characters has no text to extract unless it also
//! carries a `ToUnicode` CMap. Without one, copying a paragraph out of the
//! PDF — or checking it with `pdftotext` — yields nothing. [`Subset::to_unicode`]
//! writes that map, and the export test extracts its own text back through it.

use std::collections::BTreeMap;

use opendoc_layout::font::{FaceId, Fonts, UNITS_PER_EM};

/// The glyphs one face actually draws in one document, and what they mean.
#[derive(Default)]
pub(crate) struct Subset {
    /// Glyph id -> the character it was drawn for. A glyph reached from two
    /// characters keeps the first; `ToUnicode` is a best-effort map by
    /// specification, and the bundled subset has no many-to-one mappings.
    glyphs: BTreeMap<u16, char>,
}

impl Subset {
    pub(crate) fn record(&mut self, glyph: u16, ch: char) {
        self.glyphs.entry(glyph).or_insert(ch);
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.glyphs.is_empty()
    }

    pub(crate) fn glyphs(&self) -> impl Iterator<Item = (u16, char)> + '_ {
        self.glyphs.iter().map(|(glyph, ch)| (*glyph, *ch))
    }

    /// The `/W` array: every used glyph's advance in PDF glyph space, which
    /// is thousandths of the em.
    pub(crate) fn widths(&self, fonts: &Fonts, face: FaceId) -> Vec<(u16, f32)> {
        self.glyphs
            .keys()
            .map(|glyph| {
                let units = i64::from(fonts.glyph_advance_units(*glyph, face));
                (*glyph, (units * 1_000) as f32 / UNITS_PER_EM as f32)
            })
            .collect()
    }

    /// A `ToUnicode` CMap: what each glyph says when it is copied out.
    ///
    /// Written by hand rather than through a helper because the format is
    /// three fixed stanzas around one `bfchar` list, and a dependency that
    /// generated it would be a dependency on somebody else's idea of which
    /// stanzas matter.
    pub(crate) fn to_unicode(&self) -> Vec<u8> {
        let mut out = String::from(
            "/CIDInit /ProcSet findresource begin\n\
             12 dict begin\n\
             begincmap\n\
             /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> def\n\
             /CMapName /Adobe-Identity-UCS def\n\
             /CMapType 2 def\n\
             1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
        );
        // `bfchar` sections are capped at 100 entries by the specification.
        let entries: Vec<(u16, char)> = self.glyphs().collect();
        for chunk in entries.chunks(100) {
            out.push_str(&format!("{} beginbfchar\n", chunk.len()));
            for (glyph, ch) in chunk {
                out.push_str(&format!("<{glyph:04X}> <"));
                let mut buffer = [0u16; 2];
                for unit in ch.encode_utf16(&mut buffer) {
                    out.push_str(&format!("{unit:04X}"));
                }
                out.push_str(">\n");
            }
            out.push_str("endbfchar\n");
        }
        out.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
        out.into_bytes()
    }
}

/// Design units as PDF glyph space (thousandths of an em).
pub(crate) fn to_glyph_space(units: i16) -> f32 {
    (i64::from(units) * 1_000) as f32 / UNITS_PER_EM as f32
}
