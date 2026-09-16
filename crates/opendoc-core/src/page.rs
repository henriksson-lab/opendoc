//! Page setup: size presets, margins, orientation and header/footer slots.

use crate::block::{Block, BlockKind};
use crate::inline::Inline;
use crate::measure::Length;
use crate::warning::ModelError;
use serde::{Deserialize, Serialize};

/// Which repeated page-furniture slot a block list occupies.
///
/// Two exhaustive cases rather than two `Vec<Block>` parameters everywhere:
/// a command, an operation and a renderer all name the slot, and none of them
/// can name a slot that does not exist.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub enum HeaderFooterSlot {
    Header,
    Footer,
    /// A document-wide first-page header override. `None` in the document
    /// means inherit `Header`; an explicitly empty vector means the first
    /// page intentionally has no header.
    FirstPageHeader,
    /// The footer counterpart of [`Self::FirstPageHeader`].
    FirstPageFooter,
    /// A document-wide even-page header override. `None` in the document
    /// inherits `Header`; an explicitly empty vector intentionally has no
    /// header on pages two, four, and so on.
    EvenPageHeader,
    /// The footer counterpart of [`Self::EvenPageHeader`].
    EvenPageFooter,
}

/// The page context that begins at a [`BlockKind::SectionBreak`].
///
/// A section owns physical page geometry and all repeated furniture.  The
/// optional first/even slots deliberately retain the distinction between
/// inheriting the ordinary slot (`None`) and explicitly suppressing it
/// (`Some(vec![])`).  Section ordering belongs to the document body: the
/// root section is implicit, and each later section is named by its boundary
/// block.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Section {
    pub id: crate::ids::StableId,
    #[serde(default)]
    pub page_setup: PageSetup,
    #[serde(default)]
    pub header: Vec<Block>,
    #[serde(default)]
    pub footer: Vec<Block>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_page_header: Option<Vec<Block>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_page_footer: Option<Vec<Block>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub even_page_header: Option<Vec<Block>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub even_page_footer: Option<Vec<Block>>,
}

impl Section {
    /// The blocks in a declared furniture slot, without inheritance.
    pub fn furniture(&self, slot: HeaderFooterSlot) -> &[Block] {
        match slot {
            HeaderFooterSlot::Header => &self.header,
            HeaderFooterSlot::Footer => &self.footer,
            HeaderFooterSlot::FirstPageHeader => self.first_page_header.as_deref().unwrap_or(&[]),
            HeaderFooterSlot::FirstPageFooter => self.first_page_footer.as_deref().unwrap_or(&[]),
            HeaderFooterSlot::EvenPageHeader => self.even_page_header.as_deref().unwrap_or(&[]),
            HeaderFooterSlot::EvenPageFooter => self.even_page_footer.as_deref().unwrap_or(&[]),
        }
    }

    /// The furniture which appears at this section-relative page index.
    pub fn furniture_for_page(&self, slot: HeaderFooterSlot, page_index: usize) -> &[Block] {
        match (slot.base_slot(), page_index == 0, page_index % 2 == 1) {
            (HeaderFooterSlot::Header, true, _) => {
                self.first_page_header.as_deref().unwrap_or(&self.header)
            }
            (HeaderFooterSlot::Footer, true, _) => {
                self.first_page_footer.as_deref().unwrap_or(&self.footer)
            }
            (HeaderFooterSlot::Header, false, true) => {
                self.even_page_header.as_deref().unwrap_or(&self.header)
            }
            (HeaderFooterSlot::Footer, false, true) => {
                self.even_page_footer.as_deref().unwrap_or(&self.footer)
            }
            (HeaderFooterSlot::Header, false, false) => &self.header,
            (HeaderFooterSlot::Footer, false, false) => &self.footer,
            _ => unreachable!("base_slot only returns an ordinary furniture slot"),
        }
    }
}

impl HeaderFooterSlot {
    pub const ALL: [HeaderFooterSlot; 6] = [
        HeaderFooterSlot::Header,
        HeaderFooterSlot::Footer,
        HeaderFooterSlot::FirstPageHeader,
        HeaderFooterSlot::FirstPageFooter,
        HeaderFooterSlot::EvenPageHeader,
        HeaderFooterSlot::EvenPageFooter,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            HeaderFooterSlot::Header => "header",
            HeaderFooterSlot::Footer => "footer",
            HeaderFooterSlot::FirstPageHeader => "first-page-header",
            HeaderFooterSlot::FirstPageFooter => "first-page-footer",
            HeaderFooterSlot::EvenPageHeader => "even-page-header",
            HeaderFooterSlot::EvenPageFooter => "even-page-footer",
        }
    }

    /// The ordinary slot this variant overrides.
    pub const fn base_slot(self) -> Self {
        match self {
            Self::Header | Self::FirstPageHeader | Self::EvenPageHeader => Self::Header,
            Self::Footer | Self::FirstPageFooter | Self::EvenPageFooter => Self::Footer,
        }
    }

    /// Whether this slot is an optional first/even-page override rather than
    /// ordinary furniture, which is always present.
    pub const fn is_override(self) -> bool {
        matches!(
            self,
            Self::FirstPageHeader
                | Self::FirstPageFooter
                | Self::EvenPageHeader
                | Self::EvenPageFooter
        )
    }

    pub fn parse(value: &str) -> Result<Self, ModelError> {
        HeaderFooterSlot::ALL
            .into_iter()
            .find(|slot| slot.as_str() == value)
            .ok_or(ModelError::InvalidDocument("unknown header/footer slot"))
    }
}

/// Which derived value a [`Inline::PageNumber`] field resolves to.
///
/// Both are *fields*: the value is produced by whatever lays the document out
/// into pages and is never stored, because the same document paginates
/// differently on a different page size. See ADR 0009.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub enum PageNumberField {
    /// The number of the page the field is printed on, counting from 1.
    CurrentPage,
    /// The total number of pages in the document.
    PageCount,
}

impl PageNumberField {
    pub const ALL: [PageNumberField; 2] =
        [PageNumberField::CurrentPage, PageNumberField::PageCount];

    pub fn as_str(self) -> &'static str {
        match self {
            PageNumberField::CurrentPage => "page-number",
            PageNumberField::PageCount => "page-count",
        }
    }

    pub fn parse(value: &str) -> Result<Self, ModelError> {
        PageNumberField::ALL
            .into_iter()
            .find(|field| field.as_str() == value)
            .ok_or(ModelError::InvalidDocument("unknown page number field"))
    }
}

/// Whether a sheet is taller than it is wide.
///
/// Derived from [`PageSetup::width`] and [`PageSetup::height`], never stored:
/// a stored orientation is a second representation of the same fact and can
/// disagree with the dimensions it claims to describe. A square page reports
/// [`PageOrientation::Portrait`].
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub enum PageOrientation {
    Portrait,
    Landscape,
}

impl PageOrientation {
    pub const ALL: [PageOrientation; 2] = [PageOrientation::Portrait, PageOrientation::Landscape];

    pub fn as_str(self) -> &'static str {
        match self {
            PageOrientation::Portrait => "portrait",
            PageOrientation::Landscape => "landscape",
        }
    }

    pub fn parse(value: &str) -> Result<Self, ModelError> {
        PageOrientation::ALL
            .into_iter()
            .find(|orientation| orientation.as_str() == value)
            .ok_or(ModelError::InvalidDocument("unknown page orientation"))
    }
}

/// A standard paper size, offered so a user picks "A4" instead of typing
/// 11906 by 16838.
///
/// A preset is a *UI affordance*, not a document fact: picking one writes the
/// dimensions it names into [`PageSetup`], and nothing records that a preset
/// was ever involved. [`PageSetup::size_name`] recovers a name by measuring,
/// so a document imported with A4 dimensions shows "A4" without having to
/// have been told.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PageSizePreset {
    /// Stable machine name, e.g. `"a4"`.
    pub name: &'static str,
    /// Human label, e.g. `"A4"`.
    pub label: &'static str,
    /// Portrait width in twips.
    pub width_twips: i32,
    /// Portrait height in twips.
    pub height_twips: i32,
}

/// The paper sizes the UI offers, portrait dimensions, in twips.
///
/// ISO sizes are the millimetre dimensions converted at 1mm = 1440/25.4 twips
/// and rounded, which is the same integer DOCX writers use.
pub const PAGE_SIZE_PRESETS: &[PageSizePreset] = &[
    PageSizePreset {
        name: "letter",
        label: "Letter (8.5 × 11 in)",
        width_twips: 12240,
        height_twips: 15840,
    },
    PageSizePreset {
        name: "legal",
        label: "Legal (8.5 × 14 in)",
        width_twips: 12240,
        height_twips: 20160,
    },
    PageSizePreset {
        name: "tabloid",
        label: "Tabloid (11 × 17 in)",
        width_twips: 15840,
        height_twips: 24480,
    },
    PageSizePreset {
        name: "a3",
        label: "A3 (297 × 420 mm)",
        width_twips: 16838,
        height_twips: 23811,
    },
    PageSizePreset {
        name: "a4",
        label: "A4 (210 × 297 mm)",
        width_twips: 11906,
        height_twips: 16838,
    },
    PageSizePreset {
        name: "a5",
        label: "A5 (148 × 210 mm)",
        width_twips: 8391,
        height_twips: 11906,
    },
    PageSizePreset {
        name: "b5",
        label: "B5 (176 × 250 mm)",
        width_twips: 9978,
        height_twips: 14173,
    },
];

/// The sheet a document is laid out on: physical size plus margins.
///
/// Lengths are [`Length`], so page geometry is in the same unit as every
/// other length in the model and `Document` keeps `Eq`. `start`/`end`
/// margins are direction-relative for the same reason block indents are:
/// they follow the document's writing direction rather than naming a
/// physical side.
///
/// Orientation and the name of a standard size are both *derived* from the
/// dimensions (see [`PageSetup::orientation`], [`PageSetup::size_name`]) so
/// there is exactly one representation of the page's shape.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PageSetup {
    pub width: Length,
    pub height: Length,
    pub margin_top: Length,
    pub margin_bottom: Length,
    /// Margin on the leading edge of the page (left in a LTR document).
    pub margin_start: Length,
    /// Margin on the trailing edge of the page (right in a LTR document).
    pub margin_end: Length,
    /// Distance from the top edge of the sheet to the top of the header.
    pub margin_header: Length,
    /// Distance from the bottom edge of the sheet to the bottom of the footer.
    pub margin_footer: Length,
    /// The displayed number of the first physical page. This is page
    /// numbering rather than geometry, but it belongs to the same
    /// document-wide setup: changing it changes every resolved page-number
    /// field together and must converge as one page-setup write.
    #[serde(default = "PageSetup::default_page_number_start")]
    pub page_number_start: u32,
}

impl Default for PageSetup {
    /// US Letter with one-inch margins and half-inch header/footer margins —
    /// the default Word and Google Docs both use.
    fn default() -> Self {
        Self {
            width: Length(12240),
            height: Length(15840),
            margin_top: Length(Self::DEFAULT_MARGIN_TWIPS),
            margin_bottom: Length(Self::DEFAULT_MARGIN_TWIPS),
            margin_start: Length(Self::DEFAULT_MARGIN_TWIPS),
            margin_end: Length(Self::DEFAULT_MARGIN_TWIPS),
            margin_header: Length(Self::DEFAULT_FURNITURE_MARGIN_TWIPS),
            margin_footer: Length(Self::DEFAULT_FURNITURE_MARGIN_TWIPS),
            page_number_start: Self::default_page_number_start(),
        }
    }
}

impl PageSetup {
    /// One inch.
    pub const DEFAULT_MARGIN_TWIPS: i32 = 1440;
    /// Half an inch.
    pub const DEFAULT_FURNITURE_MARGIN_TWIPS: i32 = 720;
    /// The conventional first displayed page number, shared by Google Docs
    /// and the document model's historical implicit numbering.
    pub const fn default_page_number_start() -> u32 {
        1
    }

    /// A page of the given size with the default margins.
    pub fn new(width: Length, height: Length) -> Result<Self, ModelError> {
        let setup = Self {
            width,
            height,
            ..Self::default()
        };
        setup.validate()?;
        Ok(setup)
    }

    /// The named standard size in portrait, with the default margins.
    pub fn from_size_name(name: &str) -> Result<Self, ModelError> {
        let preset =
            page_size_preset(name).ok_or(ModelError::InvalidDocument("unknown page size name"))?;
        Self::new(
            Length::from_twips(preset.width_twips)?,
            Length::from_twips(preset.height_twips)?,
        )
    }

    /// The same margins on a different sheet.
    pub fn with_size(self, width: Length, height: Length) -> Result<Self, ModelError> {
        let setup = Self {
            width,
            height,
            ..self
        };
        setup.validate()?;
        Ok(setup)
    }

    /// The same sheet with different margins.
    pub fn with_margins(
        self,
        top: Length,
        bottom: Length,
        start: Length,
        end: Length,
    ) -> Result<Self, ModelError> {
        let setup = Self {
            margin_top: top,
            margin_bottom: bottom,
            margin_start: start,
            margin_end: end,
            ..self
        };
        setup.validate()?;
        Ok(setup)
    }

    /// The same sheet with different header/footer offsets.
    pub fn with_furniture_margins(
        self,
        header: Length,
        footer: Length,
    ) -> Result<Self, ModelError> {
        let setup = Self {
            margin_header: header,
            margin_footer: footer,
            ..self
        };
        setup.validate()?;
        Ok(setup)
    }

    /// Rotates the sheet to the requested orientation, keeping the margins.
    /// Already-correct orientation is a no-op, so this is idempotent and
    /// cannot spin a page by being applied twice.
    pub fn with_orientation(self, orientation: PageOrientation) -> Self {
        if self.orientation() == orientation {
            return self;
        }
        Self {
            width: self.height,
            height: self.width,
            ..self
        }
    }

    /// Whether the sheet is taller than it is wide. Derived, never stored.
    pub fn orientation(&self) -> PageOrientation {
        if self.width.twips() > self.height.twips() {
            PageOrientation::Landscape
        } else {
            PageOrientation::Portrait
        }
    }

    /// The name of the standard size these dimensions are, in either
    /// orientation, or `None` for a custom page.
    pub fn size_name(&self) -> Option<&'static str> {
        let short = self.width.twips().min(self.height.twips());
        let long = self.width.twips().max(self.height.twips());
        PAGE_SIZE_PRESETS
            .iter()
            .find(|preset| preset.width_twips == short && preset.height_twips == long)
            .map(|preset| preset.name)
    }

    /// The width available to content between the side margins.
    pub fn content_width(&self) -> Length {
        Length(self.width.twips() - self.margin_start.twips() - self.margin_end.twips())
    }

    /// The height available to body content between the top and bottom
    /// margins. Headers and footers live in the margins, so they do not
    /// reduce it.
    pub fn content_height(&self) -> Length {
        Length(self.height.twips() - self.margin_top.twips() - self.margin_bottom.twips())
    }

    pub fn validate(&self) -> Result<(), ModelError> {
        self.width.validate("page width")?;
        self.height.validate("page height")?;
        if self.width.twips() <= 0 || self.height.twips() <= 0 {
            return Err(ModelError::InvalidDocument("page size is not positive"));
        }
        for margin in [
            self.margin_top,
            self.margin_bottom,
            self.margin_start,
            self.margin_end,
            self.margin_header,
            self.margin_footer,
        ] {
            margin.validate("page margin")?;
            if margin.is_negative() {
                return Err(ModelError::InvalidDocument("page margin is negative"));
            }
        }
        if self.content_width().twips() <= 0 {
            return Err(ModelError::InvalidDocument(
                "page side margins leave no content width",
            ));
        }
        if self.content_height().twips() <= 0 {
            return Err(ModelError::InvalidDocument(
                "page top and bottom margins leave no content height",
            ));
        }
        if self.margin_header.twips() + self.margin_footer.twips() >= self.height.twips() {
            return Err(ModelError::InvalidDocument(
                "header and footer margins leave no page",
            ));
        }
        Ok(())
    }
}

/// The preset with this machine name, if it is one OpenDoc offers.
pub fn page_size_preset(name: &str) -> Option<&'static PageSizePreset> {
    PAGE_SIZE_PRESETS.iter().find(|preset| preset.name == name)
}

/// Page furniture may not carry content whose meaning depends on where it
/// sits in the body flow: a page break inside a header has nothing to break,
/// and a footnote reference in a footer has no numbering context.
pub(crate) fn validate_furniture_payload(blocks: &[Block]) -> Result<(), ModelError> {
    for block in blocks {
        if matches!(block.kind, BlockKind::PageBreak) {
            return Err(ModelError::InvalidDocument(
                "page furniture cannot contain a page break",
            ));
        }
        if block
            .content
            .iter()
            .any(|inline| matches!(inline, Inline::FootnoteRef { .. }))
        {
            return Err(ModelError::InvalidDocument(
                "page furniture cannot contain a footnote reference",
            ));
        }
        if let BlockKind::Table { rows, .. } = &block.kind {
            for row in rows {
                for cell in &row.cells {
                    validate_furniture_payload(&cell.blocks)?;
                }
            }
        }
    }
    Ok(())
}
