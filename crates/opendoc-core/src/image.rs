//! Image layout: intrinsic size, placement and aspect-ratio locking.

use crate::measure::Length;
use crate::table::CellBorder;
use crate::warning::ModelError;
use crate::StableId;
use serde::{Deserialize, Serialize};

/// Display geometry of an image block.
///
/// The *intrinsic* size of a picture is a property of its bytes, so it is not
/// stored here: it is whatever the blob decodes to. This is the size the
/// document asks the image to be drawn at, which is why a `None` here means
/// "use the intrinsic size" and must never be materialised into a default —
/// writing the intrinsic size into the document would freeze a projection of
/// the blob into the source, and re-encoding the blob would then silently
/// contradict it.
///
/// Lengths are [`Length`] (twips), the same unit as page geometry and block
/// indents, so DOCX `wp:extent` (EMUs, 1 twip = 635 EMU) and Google Docs
/// `size` (points, 1pt = 20 twips) both convert exactly.
///
/// The axes are independent: a set `width` with no `height` means "scale the
/// height to keep the aspect ratio", which is what a side-handle drag does.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ImageLayout {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<Length>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<Length>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement: Option<ImagePlacement>,
    /// Authored clearance around an in-flow floated image. It is separate from
    /// the picture's frame: changing a border must not move surrounding text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wrap_clearance: Option<ImageWrapClearance>,
    /// Clockwise visual rotation in whole degrees. `None` deliberately means
    /// the document makes no statement, rather than storing a needless zero.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation_degrees: Option<i16>,
    /// Opacity on a 0–100 percentage scale. An absent value is fully opaque;
    /// it is kept absent so old documents remain byte-identical.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity_percent: Option<u8>,
    /// Fractions to remove from each visual edge, in whole percent. Keeping
    /// this typed means imports and UI cannot smuggle arbitrary CSS into a
    /// document and the model can reject a crop with no visible pixels.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crop: Option<ImageCrop>,
    /// Visible author-provided text beneath the picture. This is deliberately
    /// distinct from alt text: an accessible name must never become visible
    /// merely because an image was imported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caption: Option<String>,
    /// An optional frame around the image. Reusing the document's typed border
    /// value keeps width, colour and dash style validated everywhere.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border: Option<CellBorder>,
    /// Out-of-flow object geometry. Its absence keeps the image in the
    /// in-flow subset defined by ADR 0012; see ADR 0022 for the deliberately
    /// separate positioned-object contract.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub positioned: Option<PositionedImage>,
}

/// Empty space from each edge of a floated image to nearby text, in document
/// units. Logical start/end survive right-to-left documents.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ImageWrapClearance {
    pub top: Length,
    pub end: Length,
    pub bottom: Length,
    pub start: Length,
}

impl Default for ImageWrapClearance {
    fn default() -> Self {
        Self {
            top: Length::ZERO,
            end: Length::ZERO,
            bottom: Length::ZERO,
            start: Length::ZERO,
        }
    }
}

impl ImageWrapClearance {
    pub fn validate(self) -> Result<(), ModelError> {
        for (edge, length) in [
            ("image wrap clearance top", self.top),
            ("image wrap clearance end", self.end),
            ("image wrap clearance bottom", self.bottom),
            ("image wrap clearance start", self.start),
        ] {
            length.validate(edge)?;
            if length.is_negative() {
                return Err(ModelError::InvalidDocument(
                    "image wrap clearance cannot be negative",
                ));
            }
        }
        Ok(())
    }

    pub fn is_empty(self) -> bool {
        self == Self::default()
    }
}

/// The stable anchor, offsets and text layer of an out-of-flow image.
///
/// A missing block target is intentionally representable: a concurrent
/// deletion must not retarget the object to an arbitrary sibling. Renderers
/// that support this value use the page-content anchor as the deterministic
/// fallback and report that degradation (ADR 0022).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PositionedImage {
    pub anchor: PositionedImageAnchor,
    pub horizontal_offset: Length,
    pub vertical_offset: Length,
    pub layer: PositionedImageLayer,
}

/// The coordinate origin for a [`PositionedImage`].
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum PositionedImageAnchor {
    /// The start/top corner of the page's content rectangle.
    #[default]
    PageContent,
    /// The border box of a stable block identity.
    Block(StableId),
}

/// Which side of document text a positioned image is painted on.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum PositionedImageLayer {
    BehindText,
    InFrontOfText,
}

impl PositionedImageLayer {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BehindText => "behind-text",
            Self::InFrontOfText => "in-front-of-text",
        }
    }

    pub fn parse(value: &str) -> Result<Self, ModelError> {
        match value {
            "behind-text" => Ok(Self::BehindText),
            "in-front-of-text" => Ok(Self::InFrontOfText),
            _ => Err(ModelError::InvalidDocument(
                "unknown positioned image layer",
            )),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ImageCrop {
    pub top_percent: u8,
    pub right_percent: u8,
    pub bottom_percent: u8,
    pub left_percent: u8,
}

impl ImageCrop {
    pub fn validate(self) -> Result<(), ModelError> {
        if u16::from(self.top_percent) + u16::from(self.bottom_percent) >= 100
            || u16::from(self.left_percent) + u16::from(self.right_percent) >= 100
        {
            return Err(ModelError::InvalidDocument(
                "image crop removes its entire width or height",
            ));
        }
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

impl ImageLayout {
    /// The smallest drawn size the model accepts: a quarter inch on either
    /// axis. Below that a picture is not something a reader can see or a
    /// pointer can grab, and every format OpenDoc imports from expresses a
    /// quarter inch exactly, so nothing legitimate is turned away.
    pub const MIN_TWIPS: i32 = Length::TWIPS_PER_INCH / 4;
    /// The largest: [`Length`]'s own ceiling, so a drawn size can never be a
    /// length the model cannot hold.
    pub const MAX_TWIPS: i32 = Length::MAX_TWIPS;

    /// True when the block says nothing about geometry, which is the state an
    /// image is in until it is resized or moved.
    pub fn is_empty(&self) -> bool {
        *self == ImageLayout::default()
    }

    /// The placement actually used when the document does not state one.
    pub fn effective_placement(&self) -> ImagePlacement {
        self.placement.unwrap_or_default()
    }

    /// A drawn size has to sit between [`ImageLayout::MIN_TWIPS`] and
    /// [`ImageLayout::MAX_TWIPS`]: zero or negative is not a smaller picture
    /// but an image that cannot be laid out at all, and anything under a
    /// quarter inch is invisible rather than small.
    pub fn validate(&self) -> Result<(), ModelError> {
        for (axis, length) in [("image width", self.width), ("image height", self.height)] {
            if let Some(length) = length {
                length.validate(axis)?;
                if !(Self::MIN_TWIPS..=Self::MAX_TWIPS).contains(&length.twips()) {
                    return Err(ModelError::InvalidDocument(
                        "image size is outside a quarter inch to 22in",
                    ));
                }
            }
        }
        if let Some(rotation) = self.rotation_degrees {
            if !(-360..=360).contains(&rotation) {
                return Err(ModelError::InvalidDocument(
                    "image rotation is outside -360 to 360 degrees",
                ));
            }
        }
        if let Some(opacity) = self.opacity_percent {
            if opacity > 100 {
                return Err(ModelError::InvalidDocument(
                    "image opacity is outside 0 to 100 percent",
                ));
            }
        }
        if let Some(crop) = self.crop {
            crop.validate()?;
        }
        if let Some(clearance) = self.wrap_clearance {
            clearance.validate()?;
            if !matches!(
                self.effective_placement(),
                ImagePlacement::WrapStart | ImagePlacement::WrapEnd
            ) {
                return Err(ModelError::InvalidDocument(
                    "image wrap clearance requires a floated image placement",
                ));
            }
        }
        if self
            .caption
            .as_ref()
            .is_some_and(|caption| caption.trim().is_empty())
        {
            return Err(ModelError::InvalidDocument("image caption is empty"));
        }
        if let Some(border) = self.border {
            border.validate()?;
        }
        if let Some(positioned) = &self.positioned {
            positioned.validate()?;
            if self.placement.is_some() {
                return Err(ModelError::InvalidDocument(
                    "positioned image cannot also use in-flow placement",
                ));
            }
            if self.wrap_clearance.is_some() {
                return Err(ModelError::InvalidDocument(
                    "positioned image cannot have in-flow wrap clearance",
                ));
            }
        }
        Ok(())
    }
}

impl PositionedImage {
    pub fn validate(&self) -> Result<(), ModelError> {
        if let PositionedImageAnchor::Block(block_id) = &self.anchor {
            crate::ids::validate_stable_id("positioned image anchor block id", block_id)?;
        }
        self.horizontal_offset
            .validate("positioned image horizontal offset")?;
        self.vertical_offset
            .validate("positioned image vertical offset")?;
        Ok(())
    }
}

/// Where an image block sits in the column, and how the text around it flows.
///
/// This is deliberately the subset of Google Docs' positioning model that a
/// block-level picture can express exactly:
///
/// * `Block` — the image owns its line; the blocks before and after it do not
///   share it. Google's "in line" and "break text" both land here, because an
///   OpenDoc image *is* a block.
/// * `WrapStart` / `WrapEnd` — the image is pulled to the start/end edge of
///   the column and the following blocks flow beside it ("wrap text").
///
/// Not modelled, on purpose rather than half-implemented: "behind text" and
/// "in front of text" (they need out-of-flow positioning and a z-order the
/// block model has no place for), absolute page anchoring with margin offsets,
/// and true in-paragraph anchoring — an image sitting *inside* a run of text
/// would have to be an [`Inline`], not a [`Block`]. An importer meeting any of
/// those should map to the nearest value here and emit a `ModelWarning`, never
/// pretend it round-tripped.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum ImagePlacement {
    #[default]
    Block,
    WrapStart,
    WrapEnd,
}

impl ImagePlacement {
    pub const ALL: [ImagePlacement; 3] = [
        ImagePlacement::Block,
        ImagePlacement::WrapStart,
        ImagePlacement::WrapEnd,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            ImagePlacement::Block => "block",
            ImagePlacement::WrapStart => "wrap-start",
            ImagePlacement::WrapEnd => "wrap-end",
        }
    }

    pub fn parse(value: &str) -> Result<Self, ModelError> {
        match value {
            "block" | "inline" | "break-text" => Ok(ImagePlacement::Block),
            "wrap-start" | "wrap-left" => Ok(ImagePlacement::WrapStart),
            "wrap-end" | "wrap-right" => Ok(ImagePlacement::WrapEnd),
            _ => Err(ModelError::InvalidDocument("unknown image placement")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_clearance_needs_a_float_and_nonnegative_edges() {
        let clearance = ImageWrapClearance {
            top: Length::from_twips(20).unwrap(),
            end: Length::ZERO,
            bottom: Length::from_twips(40).unwrap(),
            start: Length::ZERO,
        };
        assert!(ImageLayout {
            placement: Some(ImagePlacement::WrapEnd),
            wrap_clearance: Some(clearance),
            ..ImageLayout::default()
        }
        .validate()
        .is_ok());
        assert!(ImageLayout {
            wrap_clearance: Some(clearance),
            ..ImageLayout::default()
        }
        .validate()
        .is_err());
        assert!(ImageWrapClearance {
            start: Length::from_twips(-1).unwrap(),
            ..clearance
        }
        .validate()
        .is_err());
    }
}
