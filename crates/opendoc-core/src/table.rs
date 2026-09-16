//! Table geometry, cell spans, and the typed cell style vocabulary.

use crate::block::{Block, BlockKind};
use crate::block_properties::BlockProperties;
use crate::ids::validate_stable_id;
use crate::ids::{derived_stable_id, StableId};
use crate::inline::Inline;
use crate::measure::Length;
use crate::warning::ModelError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// One column of a table: its identity and its width.
///
/// The identity is what column operations anchor on — `insert column after
/// this one`, `resize this one` — so two replicas that both edit a table
/// agree on *which* column each meant, exactly as row operations anchor on a
/// row id rather than an index.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TableColumn {
    pub id: StableId,
    /// `None` means *auto*: the view gives the column an equal share of what
    /// the explicitly sized columns leave over. The model never invents a
    /// width, for the same reason [`BlockProperties`] never invents a default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<Length>,
}

impl TableColumn {
    /// The narrowest column a person can still click into: 0.1in.
    pub const MIN_WIDTH_TWIPS: i32 = 144;

    /// A fresh auto-width column.
    pub fn auto() -> Self {
        Self {
            id: StableId::new("column"),
            width: None,
        }
    }

    /// The one column a table with no columns at all is given, whose identity
    /// is *derived* from the table block. See [`derived_stable_id`].
    ///
    /// It takes **no index**. An index-derived identity was a live bug: a
    /// column synthesised at index 3 and a later synthesis at index 3 of a
    /// grid a delete had shifted minted the *same* id, so the table decoded
    /// as `duplicate table column id` and the merge failed outright. Every
    /// other invented column is derived from the cell that needs it
    /// ([`TableColumn::for_cell`]), which is unique by construction. This one
    /// is minted at most once per table, because it exists only when the
    /// column list is empty.
    pub fn filling(table_block_id: &StableId) -> Self {
        Self {
            id: derived_stable_id("column", &[table_block_id.as_str()]),
            width: None,
        }
    }

    /// An auto-width column whose identity is *derived* from the cell that
    /// needs it: a cell sitting at a grid position no column covers.
    ///
    /// A cell id is unique in a document, so this is unique too — which is
    /// exactly what an index-derived identity was not. It is the same rule
    /// the `InsertTableCell` operation uses to name the column that adding a
    /// cell to one row creates, so a column invented by a repair and a column
    /// created by that operation cannot disagree about a cell.
    pub fn for_cell(cell_id: &StableId) -> Self {
        Self {
            id: derived_stable_id("column", &[cell_id.as_str()]),
            width: None,
        }
    }

    /// A fresh column of an explicit width.
    pub fn sized(width: Length) -> Result<Self, ModelError> {
        let column = Self {
            id: StableId::new("column"),
            width: Some(width),
        };
        column.validate()?;
        Ok(column)
    }

    /// Re-checks the width. The smart constructors already guarantee it, but
    /// a deserialized payload has not been through them.
    pub fn validate(&self) -> Result<(), ModelError> {
        validate_stable_id("table column id", &self.id)?;
        if let Some(width) = self.width {
            width.validate("table column width")?;
            if width.twips() < Self::MIN_WIDTH_TWIPS {
                return Err(ModelError::InvalidDocument(
                    "table column width is below 0.1in",
                ));
            }
        }
        Ok(())
    }
}

/// Where a table whose columns have an explicit total width sits in its text
/// column. This deliberately differs from paragraph alignment: it moves the
/// table box, never the text in each cell.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TableAlignment {
    Start,
    Center,
    End,
}

impl TableAlignment {
    pub fn parse(value: &str) -> Result<Self, ModelError> {
        match value {
            "start" | "left" => Ok(Self::Start),
            "center" => Ok(Self::Center),
            "end" | "right" => Ok(Self::End),
            _ => Err(ModelError::InvalidDocument("unknown table alignment")),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Center => "center",
            Self::End => "end",
        }
    }
}

/// Formatting inherited by every table-cell edge that does not state its own
/// border. `None` intentionally means the document is silent: the view may
/// show an editing grid, but an exporter must not turn that grid into content.
/// `Some(CellBorder::none())` is the distinct, authored instruction that the
/// table has no rules.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct TableProperties {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border: Option<CellBorder>,
    /// `None` follows the text direction's start edge. Alignment only changes
    /// a table with a fully stated width; an auto-width table fills its frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alignment: Option<TableAlignment>,
}

impl TableProperties {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    pub fn validate(&self) -> Result<(), ModelError> {
        if let Some(border) = self.border {
            border.validate()?;
        }
        Ok(())
    }
}

/// How far a cell reaches from its own grid position.
///
/// A merged cell is a span on the cell it starts at; the cells it covers stay
/// in the grid, keep their identity and keep their content, and simply are
/// not drawn. Splitting is therefore the exact inverse of merging and loses
/// nothing — the same reason OOXML keeps its `vMerge` continuation cells.
/// Which positions are covered is *derived* from the spans (see
/// [`table_covered_positions`]); no cell carries a "covered" flag that could
/// disagree with them.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CellSpan {
    rows: u32,
    columns: u32,
}

impl Default for CellSpan {
    fn default() -> Self {
        Self::SINGLE
    }
}

impl CellSpan {
    /// One row by one column: the cell occupies only its own position.
    pub const SINGLE: CellSpan = CellSpan {
        rows: 1,
        columns: 1,
    };
    /// No table OpenDoc renders is larger than this in either direction, and
    /// the product of two of them cannot overflow a `usize` on a 32-bit host.
    pub const MAX: u32 = 4_096;

    pub fn new(rows: u32, columns: u32) -> Result<Self, ModelError> {
        let span = Self { rows, columns };
        span.validate()?;
        Ok(span)
    }

    pub fn rows(self) -> u32 {
        self.rows
    }

    pub fn columns(self) -> u32 {
        self.columns
    }

    /// Whether this cell is unmerged.
    pub fn is_single(self) -> bool {
        self == Self::SINGLE
    }

    /// `skip_serializing_if` hands the field by reference.
    fn is_single_ref(&self) -> bool {
        self.is_single()
    }

    pub fn validate(self) -> Result<(), ModelError> {
        if !(1..=Self::MAX).contains(&self.rows) || !(1..=Self::MAX).contains(&self.columns) {
            return Err(ModelError::InvalidDocument(
                "cell span is outside 1..=4096 in one direction",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TableRow {
    pub id: StableId,
    /// An explicit minimum row height. `None` lets the cell contents decide
    /// the height, as they do for a newly inserted Google Docs row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<Length>,
    /// A header row is semantic table state, not merely bold cell content.
    /// It projects to `<th>` cells and to Word's repeat-on-new-page flag.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub header: bool,
    pub cells: Vec<TableCell>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TableCell {
    pub id: StableId,
    /// How many grid positions this cell occupies. [`CellSpan::SINGLE`] for an
    /// unmerged cell, which is the overwhelming majority, so it is skipped on
    /// the wire.
    #[serde(default, skip_serializing_if = "CellSpan::is_single_ref")]
    pub span: CellSpan,
    #[serde(default, skip_serializing_if = "TableCellProperties::is_empty")]
    pub properties: TableCellProperties,
    pub blocks: Vec<Block>,
}

impl Default for TableCell {
    fn default() -> Self {
        Self::empty()
    }
}

impl TableCell {
    /// A fresh unmerged, unstyled cell holding one empty paragraph — the
    /// smallest thing that satisfies "a table cell has at least one block".
    pub fn empty() -> Self {
        Self::new(vec![Block::paragraph("")])
    }

    /// A cell whose identity is *derived* from the row and column it fills.
    ///
    /// Merge has to fill grid positions no operation created — a row inserted
    /// concurrently with a column has nothing where the two cross — and every
    /// replica must invent the *same* cell there, or replicas that agree on
    /// every character disagree on the bytes and on the document hash.
    /// Deriving the id from the two identities the position sits between is
    /// how that is done without a coordinator: it is a pure function of the
    /// grid, so it needs no counter and no clock.
    pub fn filling(row_id: &StableId, column_id: &StableId) -> Self {
        let id = derived_stable_id("cell", &[row_id.as_str(), column_id.as_str()]);
        // The empty paragraph inside it has to be derived too: an id minted
        // from a counter here would make two replicas that agree on the
        // whole grid disagree on the bytes.
        let block = Block {
            id: derived_stable_id("block", &[id.as_str()]),
            kind: BlockKind::Paragraph,
            content: vec![Inline::Text {
                id: derived_stable_id("text", &[id.as_str()]),
                text: String::new(),
                marks: Vec::new(),
            }],
            properties: BlockProperties::default(),
        };
        Self {
            id,
            span: CellSpan::SINGLE,
            properties: TableCellProperties::default(),
            blocks: vec![block],
        }
    }

    /// A fresh unmerged, unstyled cell around the given blocks.
    pub fn new(blocks: Vec<Block>) -> Self {
        Self {
            id: StableId::new("cell"),
            span: CellSpan::SINGLE,
            properties: TableCellProperties::default(),
            blocks,
        }
    }
}

impl TableRow {
    /// A fresh row of `column_count` empty cells.
    pub fn empty(column_count: usize) -> Self {
        Self {
            id: StableId::new("row"),
            height: None,
            header: false,
            cells: (0..column_count).map(|_| TableCell::empty()).collect(),
        }
    }

    /// A row whose identity — and whose cells' identities — are *derived*
    /// from the table it belongs to, for the row merge has to invent when the
    /// last real row is deleted. See [`derived_stable_id`].
    pub fn filling(table_block_id: &StableId, columns: &[TableColumn]) -> Self {
        let id = derived_stable_id("row", &[table_block_id.as_str()]);
        let cells = columns
            .iter()
            .map(|column| TableCell::filling(&id, &column.id))
            .collect();
        Self {
            id,
            height: None,
            header: false,
            cells,
        }
    }
}

/// The grid positions hidden underneath a merged cell.
///
/// A position `(row_index, column_index)` is covered when some *other* cell's
/// span rectangle contains it. Only a cell whose span is not
/// [`CellSpan::SINGLE`] can cover anything, and [`validate_table_geometry`]
/// rejects a grid where two such rectangles overlap, so this is unambiguous.
pub fn table_covered_positions(rows: &[TableRow]) -> BTreeSet<(usize, usize)> {
    let mut covered = BTreeSet::new();
    for (row_index, row) in rows.iter().enumerate() {
        for (column_index, cell) in row.cells.iter().enumerate() {
            if cell.span.is_single() {
                continue;
            }
            for covered_row in row_index..row_index + cell.span.rows() as usize {
                for covered_column in column_index..column_index + cell.span.columns() as usize {
                    if (covered_row, covered_column) != (row_index, column_index) {
                        covered.insert((covered_row, covered_column));
                    }
                }
            }
        }
    }
    covered
}

/// An opaque sRGB colour.
///
/// Three bytes, not a string: the model keeps `Eq`, two replicas that picked
/// the same colour serialize identical bytes, and `#RRGGBB` is a spelling of
/// it rather than the thing itself. Inline marks still carry CSS colour
/// strings (`MarkKind::Color`); that is the older, untyped surface and this
/// type is deliberately not it.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub struct Color {
    red: u8,
    green: u8,
    blue: u8,
}

impl Color {
    pub const BLACK: Color = Color {
        red: 0,
        green: 0,
        blue: 0,
    };

    pub fn from_rgb(red: u8, green: u8, blue: u8) -> Self {
        Self { red, green, blue }
    }

    /// Parses `#rgb` or `#rrggbb`, case-insensitively. Nothing else: a colour
    /// name or an `rgb()` function would be a second spelling to keep in sync.
    pub fn parse(value: &str) -> Result<Self, ModelError> {
        let digits = value
            .strip_prefix('#')
            .ok_or(ModelError::InvalidDocument("colour is not #rgb or #rrggbb"))?;
        if !digits.chars().all(|ch| ch.is_ascii_hexdigit()) {
            return Err(ModelError::InvalidDocument("colour is not #rgb or #rrggbb"));
        }
        let channel = |slice: &str| u8::from_str_radix(slice, 16).unwrap_or_default();
        match digits.len() {
            3 => {
                let doubled: Vec<String> = digits.chars().map(|ch| format!("{ch}{ch}")).collect();
                Ok(Self::from_rgb(
                    channel(&doubled[0]),
                    channel(&doubled[1]),
                    channel(&doubled[2]),
                ))
            }
            6 => Ok(Self::from_rgb(
                channel(&digits[0..2]),
                channel(&digits[2..4]),
                channel(&digits[4..6]),
            )),
            _ => Err(ModelError::InvalidDocument("colour is not #rgb or #rrggbb")),
        }
    }

    /// The canonical `#rrggbb` spelling, always lower case and always six
    /// digits, so a round trip through [`Color::parse`] is the identity.
    pub fn as_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.red, self.green, self.blue)
    }

    pub fn rgb(self) -> (u8, u8, u8) {
        (self.red, self.green, self.blue)
    }
}

/// How a cell border is drawn. `None` is a border explicitly turned off,
/// which is not the same as an absent border property (inherit).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum BorderStyle {
    None,
    Solid,
    Dashed,
    Dotted,
    Double,
}

impl BorderStyle {
    pub const ALL: [BorderStyle; 5] = [
        BorderStyle::None,
        BorderStyle::Solid,
        BorderStyle::Dashed,
        BorderStyle::Dotted,
        BorderStyle::Double,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            BorderStyle::None => "none",
            BorderStyle::Solid => "solid",
            BorderStyle::Dashed => "dashed",
            BorderStyle::Dotted => "dotted",
            BorderStyle::Double => "double",
        }
    }

    pub fn parse(value: &str) -> Result<Self, ModelError> {
        BorderStyle::ALL
            .into_iter()
            .find(|style| style.as_str() == value)
            .ok_or(ModelError::InvalidDocument("unknown border style"))
    }
}

/// One edge of a cell's border: style, thickness and colour travel together
/// because a thickness without a style draws nothing and a style without a
/// colour has to invent one.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CellBorder {
    style: BorderStyle,
    width: Length,
    color: Color,
}

impl CellBorder {
    /// 6pt, the thickest border Word's table dialog offers.
    pub const MAX_WIDTH_TWIPS: i32 = 120;

    pub fn new(style: BorderStyle, width: Length, color: Color) -> Result<Self, ModelError> {
        let border = Self {
            style,
            width,
            color,
        };
        border.validate()?;
        Ok(border)
    }

    /// An explicitly absent border — the way to say "no line here" when the
    /// table's default draws one.
    pub fn none() -> Self {
        Self {
            style: BorderStyle::None,
            width: Length::ZERO,
            color: Color::BLACK,
        }
    }

    pub fn style(self) -> BorderStyle {
        self.style
    }

    pub fn width(self) -> Length {
        self.width
    }

    pub fn color(self) -> Color {
        self.color
    }

    pub fn validate(self) -> Result<(), ModelError> {
        self.width.validate("cell border width")?;
        if self.width.is_negative() || self.width.twips() > Self::MAX_WIDTH_TWIPS {
            return Err(ModelError::InvalidDocument(
                "cell border width is outside 0..=6pt",
            ));
        }
        Ok(())
    }
}

/// Where a cell's content sits when the row is taller than the content.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum VerticalAlignment {
    Top,
    Middle,
    Bottom,
}

impl VerticalAlignment {
    pub const ALL: [VerticalAlignment; 3] = [
        VerticalAlignment::Top,
        VerticalAlignment::Middle,
        VerticalAlignment::Bottom,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            VerticalAlignment::Top => "top",
            VerticalAlignment::Middle => "middle",
            VerticalAlignment::Bottom => "bottom",
        }
    }

    pub fn parse(value: &str) -> Result<Self, ModelError> {
        VerticalAlignment::ALL
            .into_iter()
            .find(|alignment| alignment.as_str() == value)
            .ok_or(ModelError::InvalidDocument("unknown vertical alignment"))
    }
}

/// Names the slot a [`TableCellProperty`] occupies. Clearing a property and
/// last-writer-wins merge are both keyed on this, so it stays in exact
/// correspondence with `TableCellProperty` — [`TableCellProperty::key`] is the
/// single place that mapping lives.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub enum TableCellPropertyKey {
    Background,
    BorderTop,
    BorderBottom,
    BorderStart,
    BorderEnd,
    VerticalAlignment,
    RowHeader,
    PaddingTop,
    PaddingBottom,
    PaddingStart,
    PaddingEnd,
}

impl TableCellPropertyKey {
    pub const ALL: [TableCellPropertyKey; 11] = [
        TableCellPropertyKey::Background,
        TableCellPropertyKey::BorderTop,
        TableCellPropertyKey::BorderBottom,
        TableCellPropertyKey::BorderStart,
        TableCellPropertyKey::BorderEnd,
        TableCellPropertyKey::VerticalAlignment,
        TableCellPropertyKey::RowHeader,
        TableCellPropertyKey::PaddingTop,
        TableCellPropertyKey::PaddingBottom,
        TableCellPropertyKey::PaddingStart,
        TableCellPropertyKey::PaddingEnd,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            TableCellPropertyKey::Background => "background",
            TableCellPropertyKey::BorderTop => "border-top",
            TableCellPropertyKey::BorderBottom => "border-bottom",
            TableCellPropertyKey::BorderStart => "border-start",
            TableCellPropertyKey::BorderEnd => "border-end",
            TableCellPropertyKey::VerticalAlignment => "vertical-alignment",
            TableCellPropertyKey::RowHeader => "row-header",
            TableCellPropertyKey::PaddingTop => "padding-top",
            TableCellPropertyKey::PaddingBottom => "padding-bottom",
            TableCellPropertyKey::PaddingStart => "padding-start",
            TableCellPropertyKey::PaddingEnd => "padding-end",
        }
    }

    pub fn parse(value: &str) -> Result<Self, ModelError> {
        TableCellPropertyKey::ALL
            .into_iter()
            .find(|key| key.as_str() == value)
            .ok_or(ModelError::InvalidDocument(
                "unknown table cell property key",
            ))
    }
}

/// One cell-level formatting property together with its value. As with
/// [`BlockProperty`], the value carries the key, so a payload cannot pair
/// `PaddingTop` with a colour.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum TableCellProperty {
    Background(Color),
    BorderTop(CellBorder),
    BorderBottom(CellBorder),
    /// The leading edge of the cell (left in a LTR document), named the same
    /// way block indents and page margins are.
    BorderStart(CellBorder),
    BorderEnd(CellBorder),
    VerticalAlignment(VerticalAlignment),
    /// Explicit row-header semantics; this is never inferred from position.
    RowHeader(bool),
    PaddingTop(Length),
    PaddingBottom(Length),
    PaddingStart(Length),
    PaddingEnd(Length),
}

impl TableCellProperty {
    pub fn key(&self) -> TableCellPropertyKey {
        match self {
            TableCellProperty::Background(_) => TableCellPropertyKey::Background,
            TableCellProperty::BorderTop(_) => TableCellPropertyKey::BorderTop,
            TableCellProperty::BorderBottom(_) => TableCellPropertyKey::BorderBottom,
            TableCellProperty::BorderStart(_) => TableCellPropertyKey::BorderStart,
            TableCellProperty::BorderEnd(_) => TableCellPropertyKey::BorderEnd,
            TableCellProperty::VerticalAlignment(_) => TableCellPropertyKey::VerticalAlignment,
            TableCellProperty::RowHeader(_) => TableCellPropertyKey::RowHeader,
            TableCellProperty::PaddingTop(_) => TableCellPropertyKey::PaddingTop,
            TableCellProperty::PaddingBottom(_) => TableCellPropertyKey::PaddingBottom,
            TableCellProperty::PaddingStart(_) => TableCellPropertyKey::PaddingStart,
            TableCellProperty::PaddingEnd(_) => TableCellPropertyKey::PaddingEnd,
        }
    }

    /// Re-checks the value's range. Smart constructors already guarantee it,
    /// but a deserialized payload has not been through them.
    pub fn validate(&self) -> Result<(), ModelError> {
        match self {
            TableCellProperty::Background(_)
            | TableCellProperty::VerticalAlignment(_)
            | TableCellProperty::RowHeader(_) => Ok(()),
            TableCellProperty::BorderTop(border)
            | TableCellProperty::BorderBottom(border)
            | TableCellProperty::BorderStart(border)
            | TableCellProperty::BorderEnd(border) => border.validate(),
            TableCellProperty::PaddingTop(length)
            | TableCellProperty::PaddingBottom(length)
            | TableCellProperty::PaddingStart(length)
            | TableCellProperty::PaddingEnd(length) => {
                length.validate("cell padding")?;
                if length.is_negative() {
                    return Err(ModelError::InvalidDocument("cell padding is negative"));
                }
                Ok(())
            }
        }
    }
}

/// Cell-level formatting: background, borders, vertical alignment, padding.
///
/// Same shape and same rules as [`BlockProperties`]: typed `Option` fields,
/// `None` means *inherit*, no untyped bag. This is the model the deleted
/// `TableCell.properties` string bag should have been.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct TableCellProperties {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border_top: Option<CellBorder>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border_bottom: Option<CellBorder>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border_start: Option<CellBorder>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border_end: Option<CellBorder>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vertical_alignment: Option<VerticalAlignment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row_header: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub padding_top: Option<Length>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub padding_bottom: Option<Length>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub padding_start: Option<Length>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub padding_end: Option<Length>,
}

impl TableCellProperties {
    pub fn is_empty(&self) -> bool {
        *self == TableCellProperties::default()
    }

    pub fn get(&self, key: TableCellPropertyKey) -> Option<TableCellProperty> {
        match key {
            TableCellPropertyKey::Background => self.background.map(TableCellProperty::Background),
            TableCellPropertyKey::BorderTop => self.border_top.map(TableCellProperty::BorderTop),
            TableCellPropertyKey::BorderBottom => {
                self.border_bottom.map(TableCellProperty::BorderBottom)
            }
            TableCellPropertyKey::BorderStart => {
                self.border_start.map(TableCellProperty::BorderStart)
            }
            TableCellPropertyKey::BorderEnd => self.border_end.map(TableCellProperty::BorderEnd),
            TableCellPropertyKey::VerticalAlignment => self
                .vertical_alignment
                .map(TableCellProperty::VerticalAlignment),
            TableCellPropertyKey::RowHeader => self.row_header.map(TableCellProperty::RowHeader),
            TableCellPropertyKey::PaddingTop => self.padding_top.map(TableCellProperty::PaddingTop),
            TableCellPropertyKey::PaddingBottom => {
                self.padding_bottom.map(TableCellProperty::PaddingBottom)
            }
            TableCellPropertyKey::PaddingStart => {
                self.padding_start.map(TableCellProperty::PaddingStart)
            }
            TableCellPropertyKey::PaddingEnd => self.padding_end.map(TableCellProperty::PaddingEnd),
        }
    }

    /// Writes `property`, returning whatever occupied that key before.
    pub fn set(&mut self, property: TableCellProperty) -> Option<TableCellProperty> {
        let previous = self.get(property.key());
        match property {
            TableCellProperty::Background(value) => self.background = Some(value),
            TableCellProperty::BorderTop(value) => self.border_top = Some(value),
            TableCellProperty::BorderBottom(value) => self.border_bottom = Some(value),
            TableCellProperty::BorderStart(value) => self.border_start = Some(value),
            TableCellProperty::BorderEnd(value) => self.border_end = Some(value),
            TableCellProperty::VerticalAlignment(value) => self.vertical_alignment = Some(value),
            TableCellProperty::RowHeader(value) => self.row_header = Some(value),
            TableCellProperty::PaddingTop(value) => self.padding_top = Some(value),
            TableCellProperty::PaddingBottom(value) => self.padding_bottom = Some(value),
            TableCellProperty::PaddingStart(value) => self.padding_start = Some(value),
            TableCellProperty::PaddingEnd(value) => self.padding_end = Some(value),
        }
        previous
    }

    /// Returns the cell to inheriting `key`, returning what was cleared.
    pub fn clear(&mut self, key: TableCellPropertyKey) -> Option<TableCellProperty> {
        let previous = self.get(key);
        match key {
            TableCellPropertyKey::Background => self.background = None,
            TableCellPropertyKey::BorderTop => self.border_top = None,
            TableCellPropertyKey::BorderBottom => self.border_bottom = None,
            TableCellPropertyKey::BorderStart => self.border_start = None,
            TableCellPropertyKey::BorderEnd => self.border_end = None,
            TableCellPropertyKey::VerticalAlignment => self.vertical_alignment = None,
            TableCellPropertyKey::RowHeader => self.row_header = None,
            TableCellPropertyKey::PaddingTop => self.padding_top = None,
            TableCellPropertyKey::PaddingBottom => self.padding_bottom = None,
            TableCellPropertyKey::PaddingStart => self.padding_start = None,
            TableCellPropertyKey::PaddingEnd => self.padding_end = None,
        }
        previous
    }

    /// Every set property, in [`TableCellPropertyKey::ALL`] order.
    pub fn iter(&self) -> impl Iterator<Item = TableCellProperty> + '_ {
        TableCellPropertyKey::ALL
            .into_iter()
            .filter_map(|key| self.get(key))
    }

    pub fn validate(&self) -> Result<(), ModelError> {
        for property in self.iter() {
            property.validate()?;
        }
        Ok(())
    }
}

/// Rejects a table whose grid does not hold together.
///
/// Four things have to be true at once, and a decoded document that breaks
/// any of them is not a table anyone can edit:
///
/// 1. every row carries exactly one cell per column, so a cell's index *is*
///    its column;
/// 2. every span stays inside the grid;
/// 3. no two spans claim the same position;
/// 4. no span starts on a position another span already covers.
pub fn validate_table_geometry(
    columns: &[TableColumn],
    rows: &[TableRow],
) -> Result<(), ModelError> {
    let mut column_ids = BTreeSet::new();
    for column in columns {
        column.validate()?;
        if !column_ids.insert(column.id.clone()) {
            return Err(ModelError::InvalidDocument("duplicate table column id"));
        }
    }
    for row in rows {
        if row.cells.len() != columns.len() {
            return Err(ModelError::InvalidDocument(
                "table row does not have one cell per column",
            ));
        }
    }
    let mut claimed: BTreeSet<(usize, usize)> = BTreeSet::new();
    for (row_index, row) in rows.iter().enumerate() {
        for (column_index, cell) in row.cells.iter().enumerate() {
            cell.span.validate()?;
            let last_row = row_index + cell.span.rows() as usize - 1;
            let last_column = column_index + cell.span.columns() as usize - 1;
            if last_row >= rows.len() || last_column >= columns.len() {
                return Err(ModelError::InvalidDocument(
                    "table cell span reaches outside the grid",
                ));
            }
            if cell.span.is_single() {
                continue;
            }
            for covered_row in row_index..=last_row {
                for covered_column in column_index..=last_column {
                    // A spanning cell claims every position it covers,
                    // its own included. A second span reaching any of them —
                    // whether by overlapping it or by *starting* on a covered
                    // position — collides here.
                    if !claimed.insert((covered_row, covered_column)) {
                        return Err(ModelError::InvalidDocument("table cell spans overlap"));
                    }
                }
            }
        }
    }
    Ok(())
}
