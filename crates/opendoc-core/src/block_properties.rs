//! The typed block property vocabulary and its validated property map.

use crate::measure::{Alignment, Length, LineSpacing, TextDirection};
use crate::table::{CellBorder, Color};
use crate::warning::ModelError;
use serde::{Deserialize, Serialize};

/// Names the slot a [`BlockProperty`] occupies. Clearing a property and
/// last-writer-wins merge are both keyed on this, so it must stay in exact
/// correspondence with `BlockProperty` — [`BlockProperty::key`] is the single
/// place that mapping lives.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub enum BlockPropertyKey {
    Alignment,
    IndentStart,
    IndentEnd,
    IndentFirstLine,
    LineSpacing,
    SpaceBefore,
    SpaceAfter,
    Direction,
    /// Prevent a page break between this block and its following sibling.
    KeepWithNext,
    /// Flat paragraph background colour (no pattern or theme indirection).
    Background,
    /// A uniform frame around the paragraph's border box. Per-edge and
    /// between-paragraph rules deliberately stay outside this bounded model.
    Border,
}

impl BlockPropertyKey {
    pub const ALL: [BlockPropertyKey; 11] = [
        BlockPropertyKey::Alignment,
        BlockPropertyKey::IndentStart,
        BlockPropertyKey::IndentEnd,
        BlockPropertyKey::IndentFirstLine,
        BlockPropertyKey::LineSpacing,
        BlockPropertyKey::SpaceBefore,
        BlockPropertyKey::SpaceAfter,
        BlockPropertyKey::Direction,
        BlockPropertyKey::KeepWithNext,
        BlockPropertyKey::Background,
        BlockPropertyKey::Border,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            BlockPropertyKey::Alignment => "alignment",
            BlockPropertyKey::IndentStart => "indent-start",
            BlockPropertyKey::IndentEnd => "indent-end",
            BlockPropertyKey::IndentFirstLine => "indent-first-line",
            BlockPropertyKey::LineSpacing => "line-spacing",
            BlockPropertyKey::SpaceBefore => "space-before",
            BlockPropertyKey::SpaceAfter => "space-after",
            BlockPropertyKey::Direction => "direction",
            BlockPropertyKey::KeepWithNext => "keep-with-next",
            BlockPropertyKey::Background => "background",
            BlockPropertyKey::Border => "border",
        }
    }

    pub fn parse(value: &str) -> Result<Self, ModelError> {
        BlockPropertyKey::ALL
            .into_iter()
            .find(|key| key.as_str() == value)
            .ok_or(ModelError::InvalidDocument("unknown block property key"))
    }
}

/// One block-level formatting property together with its value.
///
/// The value carries the key, so an operation payload cannot pair
/// `IndentStart` with a line-spacing value: the two cannot be separated.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum BlockProperty {
    Alignment(Alignment),
    /// Indent on the leading edge of the block (left in LTR).
    IndentStart(Length),
    /// Indent on the trailing edge of the block (right in LTR).
    IndentEnd(Length),
    /// Extra indent applied to the first line *relative to* `IndentStart`.
    /// A negative value is a hanging indent — the shape falls out of the sign
    /// rather than needing a separate flag. See
    /// [`BlockProperties::hanging_indent`].
    IndentFirstLine(Length),
    LineSpacing(LineSpacing),
    /// Space above the block.
    SpaceBefore(Length),
    /// Space below the block.
    SpaceAfter(Length),
    Direction(TextDirection),
    /// Keep this block and its next sibling on one page when they fit there.
    /// `false` is deliberately representable: it overrides a style/imported
    /// default, while clearing the key returns to inheriting.
    KeepWithNext(bool),
    /// Flat paragraph background. Patterned source shading is deliberately
    /// not collapsed into a colour because that loses a material fact.
    Background(Color),
    /// One border applied uniformly to all four paragraph edges. This is not
    /// a table-cell border: it merely reuses the validated line value so the
    /// two domains cannot disagree about colours, widths or line styles.
    Border(CellBorder),
}

impl BlockProperty {
    pub fn key(&self) -> BlockPropertyKey {
        match self {
            BlockProperty::Alignment(_) => BlockPropertyKey::Alignment,
            BlockProperty::IndentStart(_) => BlockPropertyKey::IndentStart,
            BlockProperty::IndentEnd(_) => BlockPropertyKey::IndentEnd,
            BlockProperty::IndentFirstLine(_) => BlockPropertyKey::IndentFirstLine,
            BlockProperty::LineSpacing(_) => BlockPropertyKey::LineSpacing,
            BlockProperty::SpaceBefore(_) => BlockPropertyKey::SpaceBefore,
            BlockProperty::SpaceAfter(_) => BlockPropertyKey::SpaceAfter,
            BlockProperty::Direction(_) => BlockPropertyKey::Direction,
            BlockProperty::KeepWithNext(_) => BlockPropertyKey::KeepWithNext,
            BlockProperty::Background(_) => BlockPropertyKey::Background,
            BlockProperty::Border(_) => BlockPropertyKey::Border,
        }
    }

    /// Re-checks the value's range. Smart constructors already guarantee it,
    /// but a deserialized payload has not been through them.
    pub fn validate(&self) -> Result<(), ModelError> {
        match self {
            BlockProperty::Alignment(_)
            | BlockProperty::Direction(_)
            | BlockProperty::KeepWithNext(_) => Ok(()),
            BlockProperty::Background(_) => Ok(()),
            BlockProperty::Border(border) => border.validate(),
            BlockProperty::IndentStart(length) => length.validate("indent start"),
            BlockProperty::IndentEnd(length) => length.validate("indent end"),
            BlockProperty::IndentFirstLine(length) => length.validate("first-line indent"),
            BlockProperty::LineSpacing(spacing) => spacing.validate(),
            BlockProperty::SpaceBefore(length) => {
                length.validate("space before")?;
                non_negative_spacing(*length)
            }
            BlockProperty::SpaceAfter(length) => {
                length.validate("space after")?;
                non_negative_spacing(*length)
            }
        }
    }
}

pub(crate) fn non_negative_spacing(length: Length) -> Result<(), ModelError> {
    if length.is_negative() {
        Err(ModelError::InvalidDocument("block spacing is negative"))
    } else {
        Ok(())
    }
}

/// Block-level paragraph formatting.
///
/// Every field is optional and `None` means "inherit" — the renderer and the
/// exporters decide the default, the model never invents one. There is no
/// untyped passthrough bag: an importer that meets a property OpenDoc cannot
/// represent emits a [`ModelWarning`] instead of smuggling a string through
/// the model.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct BlockProperties {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alignment: Option<Alignment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indent_start: Option<Length>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indent_end: Option<Length>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indent_first_line: Option<Length>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_spacing: Option<LineSpacing>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub space_before: Option<Length>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub space_after: Option<Length>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<TextDirection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep_with_next: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border: Option<CellBorder>,
}

impl BlockProperties {
    pub fn is_empty(&self) -> bool {
        *self == BlockProperties::default()
    }

    pub fn get(&self, key: BlockPropertyKey) -> Option<BlockProperty> {
        match key {
            BlockPropertyKey::Alignment => self.alignment.map(BlockProperty::Alignment),
            BlockPropertyKey::IndentStart => self.indent_start.map(BlockProperty::IndentStart),
            BlockPropertyKey::IndentEnd => self.indent_end.map(BlockProperty::IndentEnd),
            BlockPropertyKey::IndentFirstLine => {
                self.indent_first_line.map(BlockProperty::IndentFirstLine)
            }
            BlockPropertyKey::LineSpacing => self.line_spacing.map(BlockProperty::LineSpacing),
            BlockPropertyKey::SpaceBefore => self.space_before.map(BlockProperty::SpaceBefore),
            BlockPropertyKey::SpaceAfter => self.space_after.map(BlockProperty::SpaceAfter),
            BlockPropertyKey::Direction => self.direction.map(BlockProperty::Direction),
            BlockPropertyKey::KeepWithNext => self.keep_with_next.map(BlockProperty::KeepWithNext),
            BlockPropertyKey::Background => self.background.map(BlockProperty::Background),
            BlockPropertyKey::Border => self.border.map(BlockProperty::Border),
        }
    }

    /// Writes `property`, returning whatever occupied that key before.
    pub fn set(&mut self, property: BlockProperty) -> Option<BlockProperty> {
        let previous = self.get(property.key());
        match property {
            BlockProperty::Alignment(value) => self.alignment = Some(value),
            BlockProperty::IndentStart(value) => self.indent_start = Some(value),
            BlockProperty::IndentEnd(value) => self.indent_end = Some(value),
            BlockProperty::IndentFirstLine(value) => self.indent_first_line = Some(value),
            BlockProperty::LineSpacing(value) => self.line_spacing = Some(value),
            BlockProperty::SpaceBefore(value) => self.space_before = Some(value),
            BlockProperty::SpaceAfter(value) => self.space_after = Some(value),
            BlockProperty::Direction(value) => self.direction = Some(value),
            BlockProperty::KeepWithNext(value) => self.keep_with_next = Some(value),
            BlockProperty::Background(value) => self.background = Some(value),
            BlockProperty::Border(value) => self.border = Some(value),
        }
        previous
    }

    /// Returns the block to inheriting `key`, returning what was cleared.
    pub fn clear(&mut self, key: BlockPropertyKey) -> Option<BlockProperty> {
        let previous = self.get(key);
        match key {
            BlockPropertyKey::Alignment => self.alignment = None,
            BlockPropertyKey::IndentStart => self.indent_start = None,
            BlockPropertyKey::IndentEnd => self.indent_end = None,
            BlockPropertyKey::IndentFirstLine => self.indent_first_line = None,
            BlockPropertyKey::LineSpacing => self.line_spacing = None,
            BlockPropertyKey::SpaceBefore => self.space_before = None,
            BlockPropertyKey::SpaceAfter => self.space_after = None,
            BlockPropertyKey::Direction => self.direction = None,
            BlockPropertyKey::KeepWithNext => self.keep_with_next = None,
            BlockPropertyKey::Background => self.background = None,
            BlockPropertyKey::Border => self.border = None,
        }
        previous
    }

    /// Every set property, in [`BlockPropertyKey::ALL`] order.
    pub fn iter(&self) -> impl Iterator<Item = BlockProperty> + '_ {
        BlockPropertyKey::ALL
            .into_iter()
            .filter_map(|key| self.get(key))
    }

    /// The hanging indent, if the first line is pulled back from the body.
    /// This is just a negative [`BlockProperty::IndentFirstLine`] read the
    /// other way round; there is no second representation to keep in sync.
    pub fn hanging_indent(&self) -> Option<Length> {
        self.indent_first_line
            .filter(|first_line| first_line.is_negative())
            .and_then(|first_line| Length::from_twips(-first_line.twips()).ok())
    }

    pub fn validate(&self) -> Result<(), ModelError> {
        for property in self.iter() {
            property.validate()?;
        }
        Ok(())
    }
}

/// What a list item's marker is. Bullet, ordered and checklist are three
/// exhaustive cases rather than flags, so every match site has to decide what
/// a checklist does. The checkbox state lives inside the only variant where
/// it means anything — a bulleted item cannot be "checked".
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ListKind {
    Bullet,
    Ordered,
    Checklist { checked: bool },
}

impl ListKind {
    pub fn unchecked() -> Self {
        ListKind::Checklist { checked: false }
    }

    /// The marker name, without the checkbox state.
    pub fn as_str(self) -> &'static str {
        match self {
            ListKind::Bullet => "bullet",
            ListKind::Ordered => "ordered",
            ListKind::Checklist { .. } => "checklist",
        }
    }

    /// Builds a kind from a marker name plus the checkbox state that only a
    /// checklist uses.
    pub fn parse(marker: &str, checked: bool) -> Result<Self, ModelError> {
        match marker {
            "bullet" | "unordered" => Ok(ListKind::Bullet),
            "ordered" | "numbered" => Ok(ListKind::Ordered),
            "checklist" | "checkbox" => Ok(ListKind::Checklist { checked }),
            _ => Err(ModelError::InvalidDocument("unknown list kind")),
        }
    }

    pub fn is_ordered(self) -> bool {
        matches!(self, ListKind::Ordered)
    }

    pub fn checked(self) -> Option<bool> {
        match self {
            ListKind::Checklist { checked } => Some(checked),
            ListKind::Bullet | ListKind::Ordered => None,
        }
    }

    /// Sets the checkbox state, ignored by the markers that have none.
    pub fn with_checked(self, checked: bool) -> Self {
        match self {
            ListKind::Checklist { .. } => ListKind::Checklist { checked },
            other => other,
        }
    }
}
