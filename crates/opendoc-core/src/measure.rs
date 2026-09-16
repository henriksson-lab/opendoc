//! Typed measurements: lengths in twips, line spacing, alignment, direction.

use crate::warning::ModelError;
use serde::{Deserialize, Serialize};

/// A typed block-level length.
///
/// Stored in twips (twentieths of a point): the DOCX unit, exact on the
/// 0.05pt grid, and integral so `Document` keeps `Eq` and so two replicas
/// that computed the same length serialize identical bytes. Never store a
/// bare `f64` here — the unit is the type, not a comment.
///
/// 1pt = 20 twips, 1in = 1440 twips, 1cm = 566.929… twips.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Length(pub(crate) i32);

impl Length {
    pub const ZERO: Length = Length(0);
    /// 22 inches: wider than any page OpenDoc supports, and far from the
    /// range where twip arithmetic could overflow `i32`.
    pub const MAX_TWIPS: i32 = 22 * Self::TWIPS_PER_INCH;
    /// Indents and first-line offsets may be negative (a hanging indent, or
    /// content deliberately bled into the margin).
    pub const MIN_TWIPS: i32 = -Self::MAX_TWIPS;
    /// 1pt = 20 twips. Public because every surface that shows a length in
    /// points — the toolbar, the table dialogs, the image dialog, the
    /// generated TypeScript bindings — must divide by *this* number and not
    /// by a copy of it.
    pub const TWIPS_PER_POINT: i32 = 20;
    /// 1in = 1440 twips, for the same reason.
    pub const TWIPS_PER_INCH: i32 = 1440;
    const TWIPS_PER_POINT_F64: f64 = Self::TWIPS_PER_POINT as f64;
    const TWIPS_PER_INCH_F64: f64 = Self::TWIPS_PER_INCH as f64;
    const TWIPS_PER_CENTIMETER: f64 = Self::TWIPS_PER_INCH_F64 / 2.54;

    pub fn from_twips(twips: i32) -> Result<Self, ModelError> {
        if !(Self::MIN_TWIPS..=Self::MAX_TWIPS).contains(&twips) {
            return Err(ModelError::InvalidDocument("length is outside ±22in"));
        }
        Ok(Self(twips))
    }

    pub fn from_points(points: f64) -> Result<Self, ModelError> {
        Self::from_scaled(points, Self::TWIPS_PER_POINT_F64)
    }

    pub fn from_inches(inches: f64) -> Result<Self, ModelError> {
        Self::from_scaled(inches, Self::TWIPS_PER_INCH_F64)
    }

    pub fn from_centimeters(centimeters: f64) -> Result<Self, ModelError> {
        Self::from_scaled(centimeters, Self::TWIPS_PER_CENTIMETER)
    }

    fn from_scaled(value: f64, twips_per_unit: f64) -> Result<Self, ModelError> {
        if !value.is_finite() {
            return Err(ModelError::InvalidDocument("length is not a finite number"));
        }
        let twips = (value * twips_per_unit).round();
        if !(Self::MIN_TWIPS as f64..=Self::MAX_TWIPS as f64).contains(&twips) {
            return Err(ModelError::InvalidDocument("length is outside ±22in"));
        }
        Ok(Self(twips as i32))
    }

    pub fn twips(self) -> i32 {
        self.0
    }

    pub fn points(self) -> f64 {
        f64::from(self.0) / Self::TWIPS_PER_POINT_F64
    }

    pub fn inches(self) -> f64 {
        f64::from(self.0) / Self::TWIPS_PER_INCH_F64
    }

    pub fn is_negative(self) -> bool {
        self.0 < 0
    }

    pub(crate) fn validate(self, what: &'static str) -> Result<(), ModelError> {
        if (Self::MIN_TWIPS..=Self::MAX_TWIPS).contains(&self.0) {
            Ok(())
        } else {
            let _ = what;
            Err(ModelError::InvalidDocument("length is outside ±22in"))
        }
    }
}

/// A line height expressed as a multiple of the natural line height, in
/// thousandths: 1000 is single spacing, 1500 is 1.5×, 2000 is double.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LineHeightMultiple(pub(crate) u32);

impl LineHeightMultiple {
    pub const SINGLE: LineHeightMultiple = LineHeightMultiple(1_000);
    pub const MIN_THOUSANDTHS: u32 = 100;
    pub const MAX_THOUSANDTHS: u32 = 10_000;

    pub fn from_thousandths(thousandths: u32) -> Result<Self, ModelError> {
        if !(Self::MIN_THOUSANDTHS..=Self::MAX_THOUSANDTHS).contains(&thousandths) {
            return Err(ModelError::InvalidDocument(
                "line height multiple is outside 0.1..=10",
            ));
        }
        Ok(Self(thousandths))
    }

    pub fn from_ratio(ratio: f64) -> Result<Self, ModelError> {
        if !ratio.is_finite() {
            return Err(ModelError::InvalidDocument(
                "line height multiple is not a finite number",
            ));
        }
        let thousandths = (ratio * 1_000.0).round();
        if !(Self::MIN_THOUSANDTHS as f64..=Self::MAX_THOUSANDTHS as f64).contains(&thousandths) {
            return Err(ModelError::InvalidDocument(
                "line height multiple is outside 0.1..=10",
            ));
        }
        Ok(Self(thousandths as u32))
    }

    pub fn thousandths(self) -> u32 {
        self.0
    }

    pub fn ratio(self) -> f64 {
        f64::from(self.0) / 1_000.0
    }

    pub(crate) fn validate(self) -> Result<(), ModelError> {
        if (Self::MIN_THOUSANDTHS..=Self::MAX_THOUSANDTHS).contains(&self.0) {
            Ok(())
        } else {
            Err(ModelError::InvalidDocument(
                "line height multiple is outside 0.1..=10",
            ))
        }
    }
}

/// How tall each line in a block is.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum LineSpacing {
    /// A multiple of the natural line height; grows with the font size.
    Multiple(LineHeightMultiple),
    /// Exactly this height, whatever the content needs.
    Exact(Length),
    /// At least this height; taller content grows the line.
    AtLeast(Length),
}

impl LineSpacing {
    pub fn single() -> Self {
        LineSpacing::Multiple(LineHeightMultiple::SINGLE)
    }

    pub fn multiple(ratio: f64) -> Result<Self, ModelError> {
        Ok(LineSpacing::Multiple(LineHeightMultiple::from_ratio(
            ratio,
        )?))
    }

    pub fn exactly(height: Length) -> Result<Self, ModelError> {
        Self::positive(height).map(LineSpacing::Exact)
    }

    pub fn at_least(height: Length) -> Result<Self, ModelError> {
        Self::positive(height).map(LineSpacing::AtLeast)
    }

    fn positive(height: Length) -> Result<Length, ModelError> {
        if height.twips() <= 0 {
            return Err(ModelError::InvalidDocument("line height is not positive"));
        }
        Ok(height)
    }

    pub(crate) fn validate(self) -> Result<(), ModelError> {
        match self {
            LineSpacing::Multiple(multiple) => multiple.validate(),
            LineSpacing::Exact(height) | LineSpacing::AtLeast(height) => {
                height.validate("line height")?;
                Self::positive(height).map(|_| ())
            }
        }
    }

    /// The line spacings a UI offers, in the order it should offer them.
    ///
    /// They live here, next to the type, because the wire encoding and the
    /// label have to be defined exactly once: the contract generator exports
    /// this list to TypeScript, so no view ever writes `"multiple:1500"` by
    /// hand or takes it apart again.
    pub const PRESETS: [LineSpacing; 4] = [
        LineSpacing::Multiple(LineHeightMultiple(1_000)),
        LineSpacing::Multiple(LineHeightMultiple(1_150)),
        LineSpacing::Multiple(LineHeightMultiple(1_500)),
        LineSpacing::Multiple(LineHeightMultiple(2_000)),
    ];

    /// The rule's canonical name — the `mode` half of the command surface and
    /// of the block projection.
    pub fn mode(self) -> &'static str {
        match self {
            LineSpacing::Multiple(_) => "multiple",
            LineSpacing::Exact(_) => "exact",
            LineSpacing::AtLeast(_) => "at-least",
        }
    }

    /// The rule's number: thousandths of a line for `multiple`, twips for the
    /// other two. The unit travels with [`LineSpacing::mode`], never alone.
    pub fn value(self) -> i32 {
        match self {
            LineSpacing::Multiple(multiple) => multiple.thousandths() as i32,
            LineSpacing::Exact(height) | LineSpacing::AtLeast(height) => height.twips(),
        }
    }

    /// Rebuilds a spacing from the `mode`/`value` pair [`LineSpacing::mode`]
    /// and [`LineSpacing::value`] produce. The only parser for that pair.
    pub fn parse(mode: &str, value: i32) -> Result<Self, ModelError> {
        match mode.trim() {
            "multiple" => {
                let thousandths = u32::try_from(value).map_err(|_| {
                    ModelError::InvalidDocument("line spacing multiple is negative")
                })?;
                LineHeightMultiple::from_thousandths(thousandths).map(LineSpacing::Multiple)
            }
            "exact" => LineSpacing::exactly(Length::from_twips(value)?),
            "at-least" => LineSpacing::at_least(Length::from_twips(value)?),
            _ => Err(ModelError::InvalidDocument("unknown line spacing rule")),
        }
    }

    /// How this spacing reads to a person: the label a menu or a `<select>`
    /// shows. A projection rule, so it is Rust's and not the view's.
    pub fn label(self) -> String {
        match self {
            LineSpacing::Multiple(multiple) => match multiple.thousandths() {
                1_000 => "Single".to_string(),
                2_000 => "Double".to_string(),
                _ => format!("{}\u{d7}", trim_trailing_zeros(multiple.ratio())),
            },
            LineSpacing::Exact(height) => {
                format!("Exactly {} pt", trim_trailing_zeros(height.points()))
            }
            LineSpacing::AtLeast(height) => {
                format!("At least {} pt", trim_trailing_zeros(height.points()))
            }
        }
    }
}

/// `1.50` -> `1.5`, `12.00` -> `12`. Two decimals is exactly the resolution a
/// twip has in points (0.05pt), so nothing is rounded away that was stored.
pub(crate) fn trim_trailing_zeros(value: f64) -> String {
    let text = format!("{value:.2}");
    let text = text.trim_end_matches('0');
    text.trim_end_matches('.').to_string()
}

/// Horizontal alignment of a block's content.
///
/// `Start`/`End` are direction-relative: in a left-to-right block they mean
/// left/right, and they flip with [`TextDirection`]. The `"left"` and
/// `"right"` spellings are accepted by [`Alignment::parse`] as aliases for
/// importers and UI code, but the canonical spelling is always emitted.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub enum Alignment {
    Start,
    Center,
    End,
    Justify,
}

impl Alignment {
    pub const ALL: [Alignment; 4] = [
        Alignment::Start,
        Alignment::Center,
        Alignment::End,
        Alignment::Justify,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Alignment::Start => "start",
            Alignment::Center => "center",
            Alignment::End => "end",
            Alignment::Justify => "justify",
        }
    }

    pub fn parse(value: &str) -> Result<Self, ModelError> {
        match value {
            "start" | "left" => Ok(Alignment::Start),
            "center" => Ok(Alignment::Center),
            "end" | "right" => Ok(Alignment::End),
            "justify" | "justified" => Ok(Alignment::Justify),
            _ => Err(ModelError::InvalidDocument("unknown block alignment")),
        }
    }
}

/// Base writing direction of a block.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub enum TextDirection {
    LeftToRight,
    RightToLeft,
}

impl TextDirection {
    pub const ALL: [TextDirection; 2] = [TextDirection::LeftToRight, TextDirection::RightToLeft];

    pub fn as_str(self) -> &'static str {
        match self {
            TextDirection::LeftToRight => "ltr",
            TextDirection::RightToLeft => "rtl",
        }
    }

    pub fn parse(value: &str) -> Result<Self, ModelError> {
        match value {
            "ltr" | "left-to-right" => Ok(TextDirection::LeftToRight),
            "rtl" | "right-to-left" => Ok(TextDirection::RightToLeft),
            _ => Err(ModelError::InvalidDocument("unknown text direction")),
        }
    }
}
