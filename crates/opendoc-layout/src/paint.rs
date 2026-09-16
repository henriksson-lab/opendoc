//! What to draw, and where — the same layout, expressed for a renderer that
//! has no browser.
//!
//! [`layout_document`](crate::layout_document) answers "which page does each
//! block land on". That is everything the frontend needs, because the browser
//! draws the text itself. A PDF writer has no browser, so it needs the next
//! question answered too: **which glyphs sit at which coordinates**.
//!
//! The answer is produced by the *same* pass. The line breaker records the
//! pieces it commits as it commits them ([`crate::text::break_lines`]), and
//! the block flow places those lines with the leading it already measured
//! with. Nothing here re-breaks, re-measures or re-decides anything: if a
//! painted page disagreed with a paginated one, there would be two engines
//! again, which is the failure ADR 0014 exists to prevent.
//!
//! Coordinates are **twips from the top-left corner of the sheet**, y
//! growing downwards — the document's own unit and the screen's own
//! direction. PDF's y-up user space is the writer's problem, not the layout's.

use crate::font::TextStyle;

/// A colour a mark states, as the renderer would write it into the page.
///
/// Parsed here rather than left as a string because a consumer that draws has
/// to have numbers, and the *layout* is the one place that reads the marks —
/// leaving the string to the PDF writer would mean a second parser that could
/// disagree with the one the screen uses.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rgb {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

impl Rgb {
    /// The colour as the stylesheet writes it.
    pub fn css(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.red, self.green, self.blue)
    }

    /// The three components as PDF's 0..1 device-RGB.
    pub fn components(self) -> [f32; 3] {
        [
            f32::from(self.red) / 255.0,
            f32::from(self.green) / 255.0,
            f32::from(self.blue) / 255.0,
        ]
    }
}

/// Everything about a run that changes how it is *drawn* but not how wide it
/// is.
///
/// Kept beside [`TextStyle`] rather than inside it because `TextStyle` is the
/// measuring style: it is what picks a face and what the advance cache is
/// keyed on, and a colour has no business in either. Two runs merge into one
/// piece only when both agree, so a colour change starts a new run exactly as
/// a weight change does.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RunDecoration {
    /// Baseline shift in layout units, positive upwards: what
    /// `vertical-align: super` and `sub` do.
    pub rise_units: i64,
    pub color: Option<Rgb>,
    pub background: Option<Rgb>,
    pub underline: bool,
    pub strike: bool,
    /// The target of an `Inline::Link` or a `MarkKind::Link`, which a PDF
    /// turns into a `/Link` annotation.
    pub link: Option<String>,
}

impl RunDecoration {
    /// True when there is nothing to draw beyond the glyphs themselves.
    pub fn is_plain(&self) -> bool {
        *self == RunDecoration::default()
    }
}

/// One run of text on a line: a single style, a single position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaintRun {
    /// Left edge of the run, in twips from the left edge of the sheet.
    pub x_twips: i32,
    pub text: String,
    pub style: TextStyle,
    /// Colour, highlight, underline, strike, link and baseline shift. A PDF
    /// writer that ignored this would drop them silently, which is the one
    /// thing an export whose purpose is matching the screen must not do.
    pub decoration: RunDecoration,
    /// The run's drawn width in twips, so a consumer can put a highlight, an
    /// underline or a link rectangle behind it without re-measuring — and
    /// therefore without any chance of measuring it differently.
    pub width_twips: i32,
}

/// One thing to draw on a page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaintItem {
    /// A raster asset placed by the document flow.  Layout names the source
    /// and its rectangle but never reads bytes: asset decoding belongs to the
    /// output backend, while this pass remains deterministic and filesystem
    /// free.
    Image {
        blob_hash: String,
        alt_text: String,
        x_twips: i32,
        y_twips: i32,
        width_twips: i32,
        height_twips: i32,
        rotation_degrees: i16,
        crop: Option<opendoc_core::ImageCrop>,
        opacity_percent: u8,
    },
    /// A line of text. The baseline is where the glyphs sit, which is what
    /// every text renderer wants; the line box's top is not interesting once
    /// the line has been placed.
    Text {
        baseline_twips: i32,
        runs: Vec<PaintRun>,
    },
    /// A filled rectangle: a page-break rule, a ticked checkbox, the rule
    /// above the footnotes. A table's borders are [`PaintItem::Edge`]s, which
    /// carry the colour and the style a cell states.
    Fill {
        x_twips: i32,
        y_twips: i32,
        width_twips: i32,
        height_twips: i32,
        /// `None` is the layout ink (black); a table cell background carries
        /// its own document colour.
        color: Option<Rgb>,
        /// Drawn as a dashed rule rather than a solid bar. The explicit
        /// page-break rule is `border-top: … dashed` on screen, and a PDF
        /// that drew it solid would be showing a different mark from the one
        /// the user put in the document.
        dashed: bool,
    },
    /// A stroked rectangle: a checkbox, the frame standing in for an image
    /// this crate cannot draw. One line width, one colour — black — because
    /// that is all either of those is.
    Stroke {
        x_twips: i32,
        y_twips: i32,
        width_twips: i32,
        height_twips: i32,
        line_twips: i32,
    },
    /// One straight line of a table's collapsed border grid: the border a
    /// single cell boundary resolved to.
    ///
    /// A [`PaintItem::Stroke`] cannot express a table border. It is one
    /// rectangle with one line width and no colour, and a cell states its
    /// four edges *separately*, each with its own style, width and colour —
    /// so a stroked rectangle can only ever draw the same line four times.
    /// Worse, each of the two cells that meet at a boundary owns an edge
    /// there, so a rectangle per cell draws every interior boundary twice and
    /// has nowhere to put the answer when the two disagree.
    ///
    /// An `Edge` is that answer: one segment per *boundary*, already
    /// resolved. The coordinates are the line's **centre**, which is where
    /// `border-collapse: collapse` puts a border — half in each cell — and
    /// they are a segment rather than a rectangle so that a consumer strokes
    /// it without deciding anything about thickness or direction.
    Edge {
        x1_twips: i32,
        y1_twips: i32,
        x2_twips: i32,
        y2_twips: i32,
        /// The stroked width, across the line. Never zero: a boundary that
        /// resolves to no border emits no `Edge` at all.
        thickness_twips: i32,
        color: Rgb,
        /// `None` for a solid line. `Some([on, off])` is a dash pattern in
        /// twips along the line, which is how `dashed` and `dotted` reach the
        /// page as something visibly other than solid.
        dash: Option<[i32; 2]>,
    },
}

/// Everything on one sheet, in drawing order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PaintedPage {
    pub items: Vec<PaintItem>,
}

/// Why a block's geometry is an estimate rather than a measurement.
///
/// [`crate::BlockPlacement::exact`] says *that* a placement rests on an
/// estimate; this says what was estimated, so an export can name it instead
/// of reporting an undifferentiated "something above here was guessed".
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum EstimateReason {
    /// Cell content is laid out for real, but leftover-width distribution and
    /// merged cells are not pinned down by the model.
    Table,
    /// An image with no stated height: the drawn size depends on the image's
    /// own pixels, which this crate does not have.
    ImageWithoutHeight,
    /// A MathML box's size is the browser's math layout.
    Equation,
    /// Text outside the bundled font subset: the browser falls back to a face
    /// this crate cannot measure.
    TextOutsideBundledFont,
    /// A mark this crate cannot reproduce: a `MarkKind::Font` naming a family
    /// that is not bundled, a `MarkKind::Size` that is not a length, or a
    /// colour that is not a hex triple.
    UnmeasurableMark,
    /// The document carries suggestions, which render inline content that is
    /// not in `Block::content`.
    Suggestions,
}

impl EstimateReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Table => "table",
            Self::ImageWithoutHeight => "image-without-stated-height",
            Self::Equation => "equation",
            Self::TextOutsideBundledFont => "text-outside-bundled-font",
            Self::UnmeasurableMark => "unmeasurable-mark",
            Self::Suggestions => "suggestions",
        }
    }

    /// Plain-language description, for an export warning's message.
    pub fn description(self) -> &'static str {
        match self {
            Self::Table => "a table's row heights are estimated",
            Self::ImageWithoutHeight => "an image states no height, so its drawn size is estimated",
            Self::Equation => "an equation's box is laid out by the browser's math engine",
            Self::TextOutsideBundledFont => {
                "text outside the bundled font subset is measured with a fallback advance"
            }
            Self::UnmeasurableMark => {
                "a font, size or colour mark this layout cannot reproduce exactly"
            }
            Self::Suggestions => "the document carries suggestions, which render extra content",
        }
    }
}

/// A block whose own geometry was estimated, and why.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Estimate {
    pub block_id: String,
    pub reason: EstimateReason,
}

/// A laid-out document with its pages drawn.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PaintedDocument {
    pub page_count: u32,
    /// True when no block's height was estimated.
    pub exact: bool,
    pub pages: Vec<PaintedPage>,
    pub blocks: Vec<crate::BlockPlacement>,
    /// The blocks whose own geometry was estimated, in document order. A
    /// block that is merely *after* an estimated one is not listed: its
    /// placement is uncertain, but nothing about it was guessed.
    pub estimates: Vec<Estimate>,
    /// Projection degradations discovered while turning durable document
    /// geometry into placed paint.  These are deliberately not stored on the
    /// document: for example, an anchor can be absent in one partial export
    /// while remaining present in another replica.
    pub warnings: Vec<opendoc_core::ModelWarning>,
}

/// A paint item in a fragment's own frame: x from the frame's left edge, y in
/// milli-twips from the fragment's border-box top.
///
/// Private because it only makes sense mid-flight; the public items above are
/// absolute on their sheet.
#[derive(Clone, Debug)]
pub(crate) enum Local {
    Image {
        blob_hash: String,
        alt_text: String,
        x: i32,
        y: i64,
        w: i32,
        h: i64,
        rotation_degrees: i16,
        crop: Option<opendoc_core::ImageCrop>,
        opacity_percent: u8,
    },
    Text {
        baseline: i64,
        runs: Vec<LocalRun>,
    },
    Fill {
        x: i32,
        y: i64,
        w: i32,
        h: i64,
        color: Option<Rgb>,
        dashed: bool,
    },
    Stroke {
        x: i32,
        y: i64,
        w: i32,
        h: i64,
        line: i32,
    },
    Edge {
        x1: i32,
        y1: i64,
        x2: i32,
        y2: i64,
        thickness: i32,
        color: Rgb,
        dash: Option<[i32; 2]>,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct LocalRun {
    pub x: i32,
    pub text: String,
    pub style: TextStyle,
    pub decoration: RunDecoration,
    pub width: i32,
}

impl Local {
    /// Moves an item into an enclosing frame: `dx` twips right, `dy`
    /// milli-twips down.
    pub(crate) fn translated(&self, dx: i32, dy: i64) -> Local {
        match self {
            Local::Image {
                blob_hash,
                alt_text,
                x,
                y,
                w,
                h,
                rotation_degrees,
                crop,
                opacity_percent,
            } => Local::Image {
                blob_hash: blob_hash.clone(),
                alt_text: alt_text.clone(),
                x: x + dx,
                y: y + dy,
                w: *w,
                h: *h,
                rotation_degrees: *rotation_degrees,
                crop: *crop,
                opacity_percent: *opacity_percent,
            },
            Local::Text { baseline, runs } => Local::Text {
                baseline: baseline + dy,
                runs: runs
                    .iter()
                    .map(|run| LocalRun {
                        x: run.x + dx,
                        text: run.text.clone(),
                        style: run.style,
                        decoration: run.decoration.clone(),
                        width: run.width,
                    })
                    .collect(),
            },
            Local::Fill {
                x,
                y,
                w,
                h,
                color,
                dashed,
            } => Local::Fill {
                x: x + dx,
                y: y + dy,
                w: *w,
                h: *h,
                color: *color,
                dashed: *dashed,
            },
            Local::Stroke { x, y, w, h, line } => Local::Stroke {
                x: x + dx,
                y: y + dy,
                w: *w,
                h: *h,
                line: *line,
            },
            Local::Edge {
                x1,
                y1,
                x2,
                y2,
                thickness,
                color,
                dash,
            } => Local::Edge {
                x1: x1 + dx,
                y1: y1 + dy,
                x2: x2 + dx,
                y2: y2 + dy,
                thickness: *thickness,
                color: *color,
                dash: *dash,
            },
        }
    }

    /// Places an item on a sheet: `origin_x` twips from the sheet's left
    /// edge, `origin_y` milli-twips from its top.
    pub(crate) fn placed(&self, origin_x: i32, origin_y: i64) -> PaintItem {
        match self {
            Local::Image {
                blob_hash,
                alt_text,
                x,
                y,
                w,
                h,
                rotation_degrees,
                crop,
                opacity_percent,
            } => PaintItem::Image {
                blob_hash: blob_hash.clone(),
                alt_text: alt_text.clone(),
                x_twips: origin_x + x,
                y_twips: crate::to_twips(origin_y + y),
                width_twips: *w,
                height_twips: crate::to_twips(*h),
                rotation_degrees: *rotation_degrees,
                crop: *crop,
                opacity_percent: *opacity_percent,
            },
            Local::Text { baseline, runs } => PaintItem::Text {
                baseline_twips: crate::to_twips(origin_y + baseline),
                runs: runs
                    .iter()
                    .map(|run| PaintRun {
                        x_twips: origin_x + run.x,
                        text: run.text.clone(),
                        style: run.style,
                        decoration: run.decoration.clone(),
                        width_twips: run.width,
                    })
                    .collect(),
            },
            Local::Fill {
                x,
                y,
                w,
                h,
                color,
                dashed,
            } => PaintItem::Fill {
                x_twips: origin_x + x,
                y_twips: crate::to_twips(origin_y + y),
                width_twips: *w,
                height_twips: crate::to_twips(*h),
                color: *color,
                dashed: *dashed,
            },
            Local::Stroke { x, y, w, h, line } => PaintItem::Stroke {
                x_twips: origin_x + x,
                y_twips: crate::to_twips(origin_y + y),
                width_twips: *w,
                height_twips: crate::to_twips(*h),
                line_twips: *line,
            },
            Local::Edge {
                x1,
                y1,
                x2,
                y2,
                thickness,
                color,
                dash,
            } => PaintItem::Edge {
                x1_twips: origin_x + x1,
                y1_twips: crate::to_twips(origin_y + y1),
                x2_twips: origin_x + x2,
                y2_twips: crate::to_twips(origin_y + y2),
                thickness_twips: *thickness,
                color: *color,
                dash: *dash,
            },
        }
    }
}
