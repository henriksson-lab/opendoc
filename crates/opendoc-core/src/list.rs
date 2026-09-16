//! Properties owned by a list run rather than copied onto its items.
//!
//! A `list_id` identifies a run of sibling list items.  Numbering restarts
//! belong to that run and nesting level, not to the first item which happens
//! to display them.  Keeping this small map at the document root makes a
//! later insert, move or merge unable to leave a stale per-item restart
//! behind.

use crate::warning::ModelError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The counter style used by an ordered-list wrapper.
///
/// This deliberately contains only formats with an exact CSS/HTML, DOCX and
/// ODT counterpart.  Arbitrary marker strings are not a number format: they
/// need a separate glyph model instead of pretending to be one.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OrderedListFormat {
    Decimal,
    LowerAlpha,
    UpperAlpha,
    LowerRoman,
    UpperRoman,
}

impl OrderedListFormat {
    pub const fn css_name(self) -> &'static str {
        match self {
            Self::Decimal => "decimal",
            Self::LowerAlpha => "lower-alpha",
            Self::UpperAlpha => "upper-alpha",
            Self::LowerRoman => "lower-roman",
            Self::UpperRoman => "upper-roman",
        }
    }

    /// ODF's `style:num-format` spelling for this exact counter style.
    pub const fn odt_name(self) -> &'static str {
        match self {
            Self::Decimal => "1",
            Self::LowerAlpha => "a",
            Self::UpperAlpha => "A",
            Self::LowerRoman => "i",
            Self::UpperRoman => "I",
        }
    }

    /// WordprocessingML's `w:numFmt` spelling for this exact style.
    pub const fn docx_name(self) -> &'static str {
        match self {
            Self::Decimal => "decimal",
            Self::LowerAlpha => "lowerLetter",
            Self::UpperAlpha => "upperLetter",
            Self::LowerRoman => "lowerRoman",
            Self::UpperRoman => "upperRoman",
        }
    }

    /// The historical depth cycle remains the inherited default, so old
    /// documents keep their appearance and selecting that value is
    /// canonicalized by removing the explicit setting.
    pub const fn inherited_at(level: u8) -> Self {
        match level % 3 {
            0 => Self::Decimal,
            1 => Self::LowerAlpha,
            _ => Self::LowerRoman,
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "decimal" => Some(Self::Decimal),
            "lower-alpha" => Some(Self::LowerAlpha),
            "upper-alpha" => Some(Self::UpperAlpha),
            "lower-roman" => Some(Self::LowerRoman),
            "upper-roman" => Some(Self::UpperRoman),
            _ => None,
        }
    }
}

/// A supported bullet glyph for one unordered-list wrapper.
///
/// The named markers have exact native representations. `Custom` is a small,
/// literal Unicode marker (never a font name, image, or CSS fragment). It is
/// deliberately source state: an exporter that cannot carry it must say so
/// rather than silently changing the document.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum BulletListMarker {
    Disc,
    Circle,
    Square,
    Custom(String),
}

// Unit variants in an untagged serde enum become `null`, not their variant
// names. These values travel through the app DTO as list-level source state,
// so named native markers must stay their canonical strings; custom markers
// share that same string representation rather than gaining an enum wrapper.
impl Serialize for BulletListMarker {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(match self {
            Self::Disc => "disc",
            Self::Circle => "circle",
            Self::Square => "square",
            Self::Custom(glyph) => glyph,
        })
    }
}

impl<'de> Deserialize<'de> for BulletListMarker {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Ok(Self::parse(&value).unwrap_or(Self::Custom(value)))
    }
}

impl BulletListMarker {
    pub fn glyph(&self) -> &str {
        match self {
            Self::Disc => "\u{2022}",
            Self::Circle => "\u{25E6}",
            Self::Square => "\u{25A0}",
            Self::Custom(glyph) => glyph,
        }
    }

    pub const fn css_name(&self) -> Option<&'static str> {
        match self {
            Self::Disc => Some("disc"),
            Self::Circle => Some("circle"),
            Self::Square => Some("square"),
            Self::Custom(_) => None,
        }
    }

    pub const fn inherited_at(level: u8) -> Self {
        match level % 3 {
            0 => Self::Disc,
            1 => Self::Circle,
            _ => Self::Square,
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "disc" => Some(Self::Disc),
            "circle" => Some(Self::Circle),
            "square" => Some(Self::Square),
            _ if Self::valid_custom(value) => Some(Self::Custom(value.to_string())),
            _ => None,
        }
    }

    /// Accept a compact, visible Unicode literal. Quotes, slash and controls
    /// are excluded because the HTML renderer puts the marker in CSS; this is
    /// not an escape hatch for styles, fonts, or assets.
    pub fn valid_custom(value: &str) -> bool {
        !value.is_empty()
            && value.chars().count() <= 16
            && value
                .chars()
                .all(|ch| !ch.is_control() && !matches!(ch, '\\' | '\'' | '"' | '<' | '>' | '&'))
    }
}

/// Numbering configuration for one list run.
///
/// Only ordered lists consume these starts.  A missing level starts at one;
/// an entry says the first ordered item in that wrapper starts at the given
/// positive ordinal. Explicit formats override the inherited depth cycle for
/// one wrapper and are source state, not a renderer-only preference.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ListProperties {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub ordered_starts: BTreeMap<u8, u32>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub ordered_formats: BTreeMap<u8, OrderedListFormat>,
    /// Explicit glyphs for unordered wrappers. Missing levels use the
    /// historical disc/circle/square depth cycle.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub bullet_markers: BTreeMap<u8, BulletListMarker>,
}

impl ListProperties {
    pub fn start_for(&self, level: u8) -> u32 {
        self.ordered_starts.get(&level).copied().unwrap_or(1)
    }

    pub fn format_for(&self, level: u8) -> OrderedListFormat {
        self.ordered_formats
            .get(&level)
            .copied()
            .unwrap_or_else(|| OrderedListFormat::inherited_at(level))
    }

    pub fn bullet_marker_for(&self, level: u8) -> BulletListMarker {
        self.bullet_markers
            .get(&level)
            .cloned()
            .unwrap_or_else(|| BulletListMarker::inherited_at(level))
    }

    pub fn is_empty(&self) -> bool {
        self.ordered_starts.is_empty()
            && self.ordered_formats.is_empty()
            && self.bullet_markers.is_empty()
    }

    pub fn validate(&self) -> Result<(), ModelError> {
        for (&level, &start) in &self.ordered_starts {
            if level > 8 {
                return Err(ModelError::InvalidDocument(
                    "list numbering level is outside 0..=8",
                ));
            }
            if start == 0 {
                return Err(ModelError::InvalidDocument("list numbering start is zero"));
            }
        }
        for &level in self.ordered_formats.keys() {
            if level > 8 {
                return Err(ModelError::InvalidDocument(
                    "list numbering level is outside 0..=8",
                ));
            }
        }
        for (&level, marker) in &self.bullet_markers {
            if level > 8 {
                return Err(ModelError::InvalidDocument(
                    "list bullet marker level is outside 0..=8",
                ));
            }
            if let BulletListMarker::Custom(glyph) = marker {
                if !BulletListMarker::valid_custom(glyph) {
                    return Err(ModelError::InvalidDocument(
                        "list custom bullet marker is not a safe Unicode glyph string",
                    ));
                }
            }
        }
        Ok(())
    }
}
