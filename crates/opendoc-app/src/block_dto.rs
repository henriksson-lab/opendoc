//! The block projection DTO and its typed block properties.

use super::*;
use crate::table_dto::AppCellBorder;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppBlock {
    pub id: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<u8>,
    /// Projection of `list_kind`: `Some(true)` only for an ordered list item.
    /// A checklist item is `Some(false)` here — read `list_kind` to tell a
    /// checklist from a bullet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ordered: Option<bool>,
    /// The list run this item belongs to. Adjacent items sharing this id are
    /// one list; a different id starts a new list and restarts numbering.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub list_id: Option<String>,
    /// `"bullet"`, `"ordered"` or `"checklist"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub list_kind: Option<String>,
    /// Checkbox state, present only for checklist items.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked: Option<bool>,
    #[serde(default, skip_serializing_if = "crate::is_default")]
    pub properties: AppBlockProperties,
    pub style_value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub equation_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blob_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt_text: Option<String>,
    /// Display width of an image block, in twips. Absent means the image is
    /// drawn at its intrinsic size — it is never filled in with the size the
    /// bytes happen to decode to, because that is a fact about the blob and
    /// not something the document said.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_width_twips: Option<i32>,
    /// Display height of an image block, in twips. Absent with a width
    /// present means "scale to keep the aspect ratio".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_height_twips: Option<i32>,
    /// `"block"`, `"wrap-start"` or `"wrap-end"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_placement: Option<String>,
    /// Logical clearance around a floated image, in twips. Absent means the
    /// target's ordinary float spacing, not four authored zeroes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_wrap_clearance: Option<opendoc_core::ImageWrapClearance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_rotation_degrees: Option<i16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_opacity_percent: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_crop_top_percent: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_crop_right_percent: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_crop_bottom_percent: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_crop_left_percent: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_caption: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_border: Option<AppCellBorder>,
    /// Durable positioned-object data (ADR 0022). The current desktop
    /// renderer intentionally does not consume it yet, but projections must
    /// preserve it so a read/modify/write client cannot erase the object.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_positioned: Option<opendoc_core::PositionedImage>,
    pub content: Vec<AppInline>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rows: Vec<Vec<Vec<AppBlock>>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub row_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cell_ids: Vec<Vec<String>>,
    /// The grid itself — column identities and widths, cell spans and cell
    /// styling — present only on a table block. `rows`, `row_ids` and
    /// `cell_ids` carry the *contents* of the grid; this carries its shape,
    /// and the two are parallel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table: Option<AppTable>,
}

impl AppBlock {
    pub(crate) fn from_core(block: &Block, citations: &opendoc_core::CitationDatabase) -> Self {
        // Read separately rather than through the tuple below: the tuple is
        // already nine wide and every arm would have to grow three more holes
        // to carry a field only one kind of block has.
        let image_layout = match &block.kind {
            BlockKind::Image { layout, .. } => Some(layout),
            _ => None,
        };
        // Same reason: the grid's shape is a fact only a table has, and
        // coverage is derived here so no view has to derive it again.
        let table = match &block.kind {
            BlockKind::Table {
                columns,
                properties,
                rows,
            } => {
                let covered = opendoc_core::table_covered_positions(rows);
                Some(AppTable {
                    columns: columns
                        .iter()
                        .map(|column| AppTableColumn {
                            id: column.id.to_string(),
                            width_twips: column.width.map(|width| width.twips()),
                        })
                        .collect(),
                    border: properties.border.map(AppCellBorder::from_core),
                    alignment: properties
                        .alignment
                        .map(|alignment| alignment.as_str().to_string()),
                    row_heights_twips: rows
                        .iter()
                        .map(|row| row.height.map(|height| height.twips()))
                        .collect(),
                    row_headers: rows.iter().map(|row| row.header).collect(),
                    cells: rows
                        .iter()
                        .enumerate()
                        .map(|(row_index, row)| {
                            row.cells
                                .iter()
                                .enumerate()
                                .map(|(column_index, cell)| AppTableCell {
                                    row_span: cell.span.rows(),
                                    column_span: cell.span.columns(),
                                    covered: covered.contains(&(row_index, column_index)),
                                    properties: AppTableCellProperties::from_core(&cell.properties),
                                })
                                .collect()
                        })
                        .collect(),
                })
            }
            _ => None,
        };
        let (kind, level, ordered, equation_source, blob_hash, alt_text, rows, row_ids, cell_ids) =
            match &block.kind {
                BlockKind::Paragraph => (
                    "paragraph".to_string(),
                    None,
                    None,
                    None,
                    None,
                    None,
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                ),
                BlockKind::Title => (
                    "title".to_string(),
                    None,
                    None,
                    None,
                    None,
                    None,
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                ),
                BlockKind::Subtitle => (
                    "subtitle".to_string(),
                    None,
                    None,
                    None,
                    None,
                    None,
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                ),
                BlockKind::Heading { level } => (
                    "heading".to_string(),
                    Some(*level),
                    None,
                    None,
                    None,
                    None,
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                ),
                BlockKind::ListItem { level, kind, .. } => (
                    "list-item".to_string(),
                    Some(*level),
                    Some(kind.is_ordered()),
                    None,
                    None,
                    None,
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                ),
                BlockKind::Table { rows, .. } => (
                    "table".to_string(),
                    None,
                    None,
                    None,
                    None,
                    None,
                    rows.iter()
                        .map(|row| {
                            row.cells
                                .iter()
                                .map(|cell| {
                                    cell.blocks
                                        .iter()
                                        .map(|block| AppBlock::from_core(block, citations))
                                        .collect()
                                })
                                .collect()
                        })
                        .collect(),
                    rows.iter().map(|row| row.id.to_string()).collect(),
                    rows.iter()
                        .map(|row| row.cells.iter().map(|cell| cell.id.to_string()).collect())
                        .collect(),
                ),
                BlockKind::EquationBlock { equation } => (
                    "equation-block".to_string(),
                    None,
                    None,
                    Some(equation.source.clone()),
                    None,
                    None,
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                ),
                BlockKind::Image {
                    blob_hash,
                    alt_text,
                    ..
                } => (
                    "image".to_string(),
                    None,
                    None,
                    None,
                    Some(blob_hash.clone()),
                    Some(alt_text.clone()),
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                ),
                BlockKind::HorizontalRule => (
                    "horizontal-rule".to_string(),
                    None,
                    None,
                    None,
                    None,
                    None,
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                ),
                BlockKind::TableOfContents { max_level } => (
                    "table-of-contents".to_string(),
                    Some(*max_level),
                    None,
                    None,
                    None,
                    None,
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                ),
                BlockKind::Bibliography => (
                    "bibliography".to_string(),
                    None,
                    None,
                    None,
                    None,
                    None,
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                ),
                BlockKind::PageBreak => (
                    "page-break".to_string(),
                    None,
                    None,
                    None,
                    None,
                    None,
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                ),
            };
        Self {
            id: block.id.to_string(),
            style_value: block_style_value(&block.kind),
            kind,
            level,
            ordered,
            list_id: block.list_id().map(StableId::to_string),
            list_kind: block.list_kind().map(|kind| kind.as_str().to_string()),
            checked: block.list_kind().and_then(opendoc_core::ListKind::checked),
            properties: AppBlockProperties::from_core(&block.properties),
            equation_source,
            blob_hash,
            alt_text,
            image_width_twips: image_layout
                .and_then(|layout| layout.width)
                .map(|w| w.twips()),
            image_height_twips: image_layout
                .and_then(|layout| layout.height)
                .map(|h| h.twips()),
            image_placement: image_layout
                .and_then(|layout| layout.placement)
                .map(|placement| placement.as_str().to_string()),
            image_wrap_clearance: image_layout.and_then(|layout| layout.wrap_clearance),
            image_rotation_degrees: image_layout.and_then(|layout| layout.rotation_degrees),
            image_opacity_percent: image_layout.and_then(|layout| layout.opacity_percent),
            image_crop_top_percent: image_layout
                .and_then(|layout| layout.crop.map(|crop| crop.top_percent)),
            image_crop_right_percent: image_layout
                .and_then(|layout| layout.crop.map(|crop| crop.right_percent)),
            image_crop_bottom_percent: image_layout
                .and_then(|layout| layout.crop.map(|crop| crop.bottom_percent)),
            image_crop_left_percent: image_layout
                .and_then(|layout| layout.crop.map(|crop| crop.left_percent)),
            image_caption: image_layout.and_then(|layout| layout.caption.clone()),
            image_border: image_layout
                .and_then(|layout| layout.border.map(AppCellBorder::from_core)),
            image_positioned: image_layout.and_then(|layout| layout.positioned.clone()),
            content: block
                .content
                .iter()
                .map(|inline| AppInline::from_core(inline, citations))
                .collect(),
            rows,
            row_ids,
            cell_ids,
            table,
        }
    }

    /// Parses the image geometry back through the model's own constructors,
    /// so a projection that travelled through JSON cannot reintroduce a size
    /// the model would reject. `None` stays `None`: an absent size means the
    /// intrinsic one and must not be materialised into a default.
    fn image_layout(&self) -> Result<opendoc_core::ImageLayout, AppApiError> {
        let length = |twips: Option<i32>| -> Result<Option<opendoc_core::Length>, AppApiError> {
            twips
                .map(opendoc_core::Length::from_twips)
                .transpose()
                .map_err(|err| AppApiError::Format(err.to_string()))
        };
        let layout = opendoc_core::ImageLayout {
            width: length(self.image_width_twips)?,
            height: length(self.image_height_twips)?,
            placement: self
                .image_placement
                .as_deref()
                .map(opendoc_core::ImagePlacement::parse)
                .transpose()
                .map_err(|err| AppApiError::Format(err.to_string()))?,
            wrap_clearance: self.image_wrap_clearance,
            rotation_degrees: self.image_rotation_degrees,
            opacity_percent: self.image_opacity_percent,
            crop: match (
                self.image_crop_top_percent,
                self.image_crop_right_percent,
                self.image_crop_bottom_percent,
                self.image_crop_left_percent,
            ) {
                (None, None, None, None) => None,
                (
                    Some(top_percent),
                    Some(right_percent),
                    Some(bottom_percent),
                    Some(left_percent),
                ) => Some(opendoc_core::ImageCrop {
                    top_percent,
                    right_percent,
                    bottom_percent,
                    left_percent,
                }),
                _ => {
                    return Err(AppApiError::Format(
                        "image crop must state all four edges".to_string(),
                    ))
                }
            },
            caption: self.image_caption.clone(),
            border: self
                .image_border
                .as_ref()
                .map(AppCellBorder::to_core)
                .transpose()?,
            positioned: self.image_positioned.clone(),
        };
        layout
            .validate()
            .map_err(|err| AppApiError::Format(err.to_string()))?;
        Ok(layout)
    }

    pub(crate) fn to_core(&self) -> Result<Block, AppApiError> {
        Ok(Block {
            id: parse_id(&self.id)?,
            kind: match self.kind.as_str() {
                "heading" => BlockKind::Heading {
                    level: self.level.unwrap_or(2),
                },
                "table" => {
                    let shape = self.table.clone().unwrap_or_default();
                    let mut rows = self
                        .rows
                        .iter()
                        .enumerate()
                        .map(|(row_index, row)| {
                            let mut cells = row
                                .iter()
                                .enumerate()
                                .map(|(cell_index, cell)| {
                                    let mut blocks = cell
                                        .iter()
                                        .map(AppBlock::to_core)
                                        .collect::<Result<Vec<_>, AppApiError>>()?;
                                    if blocks.is_empty() {
                                        blocks.push(Block::paragraph(""));
                                    }
                                    let cell_shape = shape
                                        .cells
                                        .get(row_index)
                                        .and_then(|row| row.get(cell_index))
                                        .cloned()
                                        .unwrap_or_default();
                                    Ok(opendoc_core::TableCell {
                                        id: self
                                            .cell_ids
                                            .get(row_index)
                                            .and_then(|ids| ids.get(cell_index))
                                            .map(|id| parse_id(id))
                                            .transpose()?
                                            .unwrap_or_else(|| StableId::new("cell")),
                                        span: opendoc_core::CellSpan::new(
                                            cell_shape.row_span,
                                            cell_shape.column_span,
                                        )
                                        .map_err(|err| AppApiError::Format(err.to_string()))?,
                                        properties: cell_shape.properties.to_core()?,
                                        blocks,
                                    })
                                })
                                .collect::<Result<Vec<_>, AppApiError>>()?;
                            if cells.is_empty() {
                                cells.push(opendoc_core::TableCell::empty());
                            }
                            Ok(opendoc_core::TableRow {
                                id: self
                                    .row_ids
                                    .get(row_index)
                                    .map(|id| parse_id(id))
                                    .transpose()?
                                    .unwrap_or_else(|| StableId::new("row")),
                                height: shape
                                    .row_heights_twips
                                    .get(row_index)
                                    .copied()
                                    .flatten()
                                    .map(opendoc_core::Length::from_twips)
                                    .transpose()
                                    .map_err(|err| AppApiError::Format(err.to_string()))?,
                                header: shape.row_headers.get(row_index).copied().unwrap_or(false),
                                cells,
                            })
                        })
                        .collect::<Result<Vec<_>, AppApiError>>()?;
                    if rows.is_empty() {
                        rows.push(opendoc_core::TableRow::empty(1));
                    }
                    // Columns come from the projected shape when it has them
                    // and are otherwise derived from the rows, so a hand-
                    // written projection cannot produce a table with no
                    // columns.
                    let width = rows.iter().map(|row| row.cells.len()).max().unwrap_or(0);
                    let mut columns = shape
                        .columns
                        .iter()
                        .map(|column| {
                            Ok(opendoc_core::TableColumn {
                                id: parse_id(&column.id)?,
                                width: column
                                    .width_twips
                                    .map(opendoc_core::Length::from_twips)
                                    .transpose()
                                    .map_err(|err| AppApiError::Format(err.to_string()))?,
                            })
                        })
                        .collect::<Result<Vec<_>, AppApiError>>()?;
                    columns.truncate(width);
                    while columns.len() < width {
                        columns.push(opendoc_core::TableColumn::auto());
                    }
                    BlockKind::Table {
                        columns,
                        properties: opendoc_core::TableProperties {
                            border: shape
                                .border
                                .as_ref()
                                .map(AppCellBorder::to_core)
                                .transpose()?,
                            alignment: shape
                                .alignment
                                .as_deref()
                                .map(opendoc_core::TableAlignment::parse)
                                .transpose()
                                .map_err(|err| AppApiError::Format(err.to_string()))?,
                        },
                        rows,
                    }
                }
                "page-break" => BlockKind::PageBreak,
                "horizontal-rule" => BlockKind::HorizontalRule,
                "table-of-contents" => BlockKind::TableOfContents {
                    max_level: self.level.unwrap_or(3),
                },
                "bibliography" => BlockKind::Bibliography,
                "list-item" => BlockKind::ListItem {
                    list_id: self
                        .list_id
                        .as_deref()
                        .map(parse_id)
                        .transpose()?
                        .unwrap_or_else(new_list_id),
                    level: self.level.unwrap_or(0),
                    kind: match self.list_kind.as_deref() {
                        Some(marker) => ListKind::parse(marker, self.checked.unwrap_or(false))
                            .map_err(|err| AppApiError::Format(err.to_string()))?,
                        None if self.ordered.unwrap_or(false) => ListKind::Ordered,
                        None => ListKind::Bullet,
                    },
                },
                "equation-block" => {
                    let source = self.equation_source.clone().ok_or_else(|| {
                        AppApiError::Format("block equation source missing".to_string())
                    })?;
                    if source.trim().is_empty() {
                        return Err(AppApiError::Format(
                            "block equation source is empty".to_string(),
                        ));
                    }
                    if source.trim() != source {
                        return Err(AppApiError::Format(
                            "block equation source has surrounding whitespace".to_string(),
                        ));
                    }
                    BlockKind::EquationBlock {
                        equation: Equation {
                            id: StableId::new("eq"),
                            source_format: EquationSourceFormat::LatexLike,
                            source,
                        },
                    }
                }
                "image" => {
                    let blob_hash = self.blob_hash.clone().ok_or_else(|| {
                        AppApiError::Format("image blob hash missing".to_string())
                    })?;
                    opendoc_core::HashRef::parse(&blob_hash)
                        .map_err(|err| AppApiError::Format(err.to_string()))?;
                    BlockKind::Image {
                        blob_hash,
                        alt_text: self.alt_text.clone().unwrap_or_default(),
                        layout: self.image_layout()?,
                    }
                }
                _ => BlockKind::Paragraph,
            },
            content: self
                .content
                .iter()
                .map(AppInline::to_core)
                .collect::<Result<Vec<_>, _>>()?,
            properties: self.properties.to_core()?,
        })
    }
}

/// Block-level formatting as the command surface sees it.
///
/// Lengths are twips (twentieths of a point), the same unit the model stores,
/// so the projection cannot drift from the source by rounding. Enumerable
/// values travel as their canonical names, never as free strings: `to_core`
/// parses them back through the model's smart constructors and fails loudly
/// on anything else.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppBlockProperties {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alignment: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indent_start_twips: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indent_end_twips: Option<i32>,
    /// Negative means a hanging indent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indent_first_line_twips: Option<i32>,
    /// `"multiple"`, `"exact"` or `"at-least"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_spacing_mode: Option<String>,
    /// Thousandths of a line for `"multiple"`, twips for the other two modes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_spacing_value: Option<i32>,
    /// How the spacing above reads to a person — `"Single"`, `"1.15\u{d7}"`,
    /// `"Exactly 24 pt"`. A projection, so `to_core` ignores it: it is here so
    /// a view can label an imported spacing no preset covers without
    /// inventing the wording itself.
    //
    // Skipped when unset like every field above it. Four of the ten used to
    // serialize `null` instead, which put four dead keys on every formatted
    // block of every projection and made this struct describe "inherit" two
    // different ways at once — the second of which no consumer could use,
    // because a block with *no* properties at all is omitted entirely by
    // `AppBlock::properties` and arrives as no object rather than as an object
    // full of nulls.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_spacing_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub space_before_twips: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub space_after_twips: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<String>,
    /// Whether this paragraph requests that its next sibling stay on the
    /// same page. Absent means inherit rather than false.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep_with_next: Option<bool>,
    /// Flat paragraph background colour in canonical `#rrggbb` form.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
    /// Uniform paragraph frame. Per-edge borders belong to table cells.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border: Option<AppCellBorder>,
}

impl AppBlockProperties {
    fn from_core(properties: &BlockProperties) -> Self {
        // The mode/value pair and its label are the model's own spelling of
        // itself, never a second encoding written here.
        let line_spacing_mode = properties
            .line_spacing
            .map(|spacing| spacing.mode().to_string());
        let line_spacing_value = properties.line_spacing.map(|spacing| spacing.value());
        let line_spacing_label = properties.line_spacing.map(|spacing| spacing.label());
        Self {
            alignment: properties
                .alignment
                .map(|alignment| alignment.as_str().to_string()),
            indent_start_twips: properties.indent_start.map(opendoc_core::Length::twips),
            indent_end_twips: properties.indent_end.map(opendoc_core::Length::twips),
            indent_first_line_twips: properties
                .indent_first_line
                .map(opendoc_core::Length::twips),
            line_spacing_mode,
            line_spacing_value,
            line_spacing_label,
            space_before_twips: properties.space_before.map(opendoc_core::Length::twips),
            space_after_twips: properties.space_after.map(opendoc_core::Length::twips),
            direction: properties
                .direction
                .map(|direction| direction.as_str().to_string()),
            keep_with_next: properties.keep_with_next,
            background: properties.background.map(opendoc_core::Color::as_hex),
            border: properties.border.map(AppCellBorder::from_core),
        }
    }

    fn to_core(&self) -> Result<BlockProperties, AppApiError> {
        let mut properties = BlockProperties::default();
        if let Some(alignment) = &self.alignment {
            properties.set(BlockProperty::Alignment(model(
                opendoc_core::Alignment::parse(alignment),
            )?));
        }
        for (twips, build) in [
            (
                self.indent_start_twips,
                BlockProperty::IndentStart as fn(opendoc_core::Length) -> BlockProperty,
            ),
            (self.indent_end_twips, BlockProperty::IndentEnd),
            (self.indent_first_line_twips, BlockProperty::IndentFirstLine),
            (self.space_before_twips, BlockProperty::SpaceBefore),
            (self.space_after_twips, BlockProperty::SpaceAfter),
        ] {
            if let Some(twips) = twips {
                properties.set(build(model(opendoc_core::Length::from_twips(twips))?));
            }
        }
        match (self.line_spacing_mode.as_deref(), self.line_spacing_value) {
            (None, None) => {}
            // `line_spacing_label` is deliberately not read: it is a
            // projection of the pair below, and trusting it would let a caller
            // state a spacing twice and disagree with itself.
            (Some(mode), Some(value)) => {
                properties.set(BlockProperty::LineSpacing(model(
                    opendoc_core::LineSpacing::parse(mode, value),
                )?));
            }
            _ => {
                return Err(AppApiError::Format(
                    "line spacing needs a known mode and a value".to_string(),
                ));
            }
        }
        if let Some(direction) = &self.direction {
            properties.set(BlockProperty::Direction(model(
                opendoc_core::TextDirection::parse(direction),
            )?));
        }
        if let Some(keep_with_next) = self.keep_with_next {
            properties.set(BlockProperty::KeepWithNext(keep_with_next));
        }
        if let Some(background) = &self.background {
            properties.set(BlockProperty::Background(model(
                opendoc_core::Color::parse(background),
            )?));
        }
        if let Some(border) = &self.border {
            properties.set(BlockProperty::Border(border.to_core()?));
        }
        Ok(properties)
    }
}

fn model<T>(result: Result<T, opendoc_core::ModelError>) -> Result<T, AppApiError> {
    result.map_err(|err| AppApiError::Format(err.to_string()))
}
