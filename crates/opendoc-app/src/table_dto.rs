//! Table projection DTOs: columns, cells and cell styling.

use super::*;

/// The shape of a table block: its columns, and one entry per cell in the
/// same order as [`AppBlock::rows`].
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppTable {
    pub columns: Vec<AppTableColumn>,
    /// The border inherited by unstated cell edges. It is absent when the
    /// document deliberately says nothing about a table-wide rule.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border: Option<AppCellBorder>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alignment: Option<String>,
    /// Explicit row heights, in twips, parallel with `AppBlock::row_ids`.
    /// Absent entries are content-driven.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub row_heights_twips: Vec<Option<i32>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub row_headers: Vec<bool>,
    pub cells: Vec<Vec<AppTableCell>>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppTableColumn {
    pub id: String,
    /// Absent means auto: the view shares out what the sized columns leave.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width_twips: Option<i32>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppTableCell {
    pub row_span: u32,
    pub column_span: u32,
    /// Whether this cell is hidden underneath a merged neighbour. Derived
    /// from the spans in Rust, so the view never has to work the geometry
    /// out for itself.
    pub covered: bool,
    #[serde(default, skip_serializing_if = "crate::is_default")]
    pub properties: AppTableCellProperties,
}

impl Default for AppTableCell {
    fn default() -> Self {
        Self {
            row_span: 1,
            column_span: 1,
            covered: false,
            properties: AppTableCellProperties::default(),
        }
    }
}

/// Cell-level formatting. Lengths are twips, colours are `#rrggbb`, and a
/// null/absent field means the cell inherits that property.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppTableCellProperties {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border_top: Option<AppCellBorder>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border_bottom: Option<AppCellBorder>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border_start: Option<AppCellBorder>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border_end: Option<AppCellBorder>,
    /// `"top"`, `"middle"` or `"bottom"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vertical_alignment: Option<String>,
    /// Explicit semantic row-header state; absent means no authored role.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row_header: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub padding_top_twips: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub padding_bottom_twips: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub padding_start_twips: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub padding_end_twips: Option<i32>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppCellBorder {
    /// `"none"`, `"solid"`, `"dashed"`, `"dotted"` or `"double"`.
    pub style: String,
    pub twips: i32,
    pub color: String,
}

impl AppCellBorder {
    pub(crate) fn from_core(border: opendoc_core::CellBorder) -> Self {
        Self {
            style: border.style().as_str().to_string(),
            twips: border.width().twips(),
            color: border.color().as_hex(),
        }
    }

    pub(crate) fn to_core(&self) -> Result<opendoc_core::CellBorder, AppApiError> {
        let style = opendoc_core::BorderStyle::parse(self.style.trim())
            .map_err(|err| AppApiError::Format(err.to_string()))?;
        let width = opendoc_core::Length::from_twips(self.twips)
            .map_err(|err| AppApiError::Format(err.to_string()))?;
        let color = opendoc_core::Color::parse(self.color.trim())
            .map_err(|err| AppApiError::Format(err.to_string()))?;
        opendoc_core::CellBorder::new(style, width, color)
            .map_err(|err| AppApiError::Format(err.to_string()))
    }
}

impl AppTableCellProperties {
    pub(crate) fn from_core(properties: &opendoc_core::TableCellProperties) -> Self {
        Self {
            background: properties.background.map(|color| color.as_hex()),
            border_top: properties.border_top.map(AppCellBorder::from_core),
            border_bottom: properties.border_bottom.map(AppCellBorder::from_core),
            border_start: properties.border_start.map(AppCellBorder::from_core),
            border_end: properties.border_end.map(AppCellBorder::from_core),
            vertical_alignment: properties
                .vertical_alignment
                .map(|alignment| alignment.as_str().to_string()),
            row_header: properties.row_header,
            padding_top_twips: properties.padding_top.map(|length| length.twips()),
            padding_bottom_twips: properties.padding_bottom.map(|length| length.twips()),
            padding_start_twips: properties.padding_start.map(|length| length.twips()),
            padding_end_twips: properties.padding_end.map(|length| length.twips()),
        }
    }

    pub(crate) fn to_core(&self) -> Result<opendoc_core::TableCellProperties, AppApiError> {
        let color = |value: &Option<String>| -> Result<Option<opendoc_core::Color>, AppApiError> {
            value
                .as_deref()
                .map(|value| {
                    opendoc_core::Color::parse(value.trim())
                        .map_err(|err| AppApiError::Format(err.to_string()))
                })
                .transpose()
        };
        let border =
            |value: &Option<AppCellBorder>| -> Result<Option<opendoc_core::CellBorder>, AppApiError> {
                value.as_ref().map(AppCellBorder::to_core).transpose()
            };
        let length = |value: Option<i32>| -> Result<Option<opendoc_core::Length>, AppApiError> {
            value
                .map(|twips| {
                    opendoc_core::Length::from_twips(twips)
                        .map_err(|err| AppApiError::Format(err.to_string()))
                })
                .transpose()
        };
        Ok(opendoc_core::TableCellProperties {
            background: color(&self.background)?,
            border_top: border(&self.border_top)?,
            border_bottom: border(&self.border_bottom)?,
            border_start: border(&self.border_start)?,
            border_end: border(&self.border_end)?,
            vertical_alignment: self
                .vertical_alignment
                .as_deref()
                .map(|value| {
                    opendoc_core::VerticalAlignment::parse(value.trim())
                        .map_err(|err| AppApiError::Format(err.to_string()))
                })
                .transpose()?,
            row_header: self.row_header,
            padding_top: length(self.padding_top_twips)?,
            padding_bottom: length(self.padding_bottom_twips)?,
            padding_start: length(self.padding_start_twips)?,
            padding_end: length(self.padding_end_twips)?,
        })
    }
}
