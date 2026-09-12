//! Image layout: intrinsic size, placement and aspect-ratio locking.

use crate::measure::Length;
use crate::warning::ModelError;
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
}

impl ImageLayout {
    /// True when the block says nothing about geometry, which is the state an
    /// image is in until it is resized or moved.
    pub fn is_empty(&self) -> bool {
        *self == ImageLayout::default()
    }

    /// The placement actually used when the document does not state one.
    pub fn effective_placement(&self) -> ImagePlacement {
        self.placement.unwrap_or_default()
    }

    /// A drawn size has to be strictly positive: zero or negative is not a
    /// smaller picture, it is an image that cannot be laid out at all.
    pub fn validate(&self) -> Result<(), ModelError> {
        for (axis, length) in [("image width", self.width), ("image height", self.height)] {
            if let Some(length) = length {
                length.validate(axis)?;
                if length.twips() <= 0 {
                    return Err(ModelError::InvalidDocument("image size is not positive"));
                }
            }
        }
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
