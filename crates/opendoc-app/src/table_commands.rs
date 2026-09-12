//! Table commands: rows, columns, cells, spans and cell styling.

use super::*;

impl OpenDocApp {
    pub fn add_table(&mut self) -> AppDocument {
        self.apply(
            "insert-block",
            "table",
            OperationKind::InsertBlock {
                after: self.document.blocks.last().map(|block| block.id.clone()),
                block: default_table_block(),
            },
        )
    }

    pub fn insert_table_after(
        &mut self,
        after_block_id: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let after = parse_id(after_block_id.as_ref())?;
        if !self.document.blocks.iter().any(|block| block.id == after) {
            return Err(AppApiError::NotFound(format!(
                "top-level block {after} was not found"
            )));
        }
        Ok(self.apply(
            "insert-block",
            "table after block",
            OperationKind::InsertBlock {
                after: Some(after),
                block: default_table_block(),
            },
        ))
    }

    pub fn add_table_row(
        &mut self,
        table_block_id: impl AsRef<str>,
        after_row: Option<String>,
        text: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let table_block_id = parse_id(table_block_id.as_ref())?;
        let after_row = after_row.as_deref().map(parse_id).transpose()?;
        // One cell per column, not one cell: a row that is short is a ragged
        // table, and merge would have to repair it — and say so — on every
        // ordinary "insert row".
        let columns = table_column_count(&self.document.blocks, &table_block_id);
        let mut cells = vec![opendoc_core::TableCell::new(vec![Block::paragraph(text)])];
        while cells.len() < columns {
            cells.push(opendoc_core::TableCell::empty());
        }
        Ok(self.apply(
            "insert-table-row",
            "table row",
            OperationKind::InsertTableRow {
                table_block_id,
                after_row,
                row: opendoc_core::TableRow {
                    id: StableId::new("row"),
                    cells,
                },
            },
        ))
    }

    pub fn delete_table_row(
        &mut self,
        table_block_id: impl AsRef<str>,
        row_id: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        Ok(self.apply(
            "delete-table-row",
            "delete table row",
            OperationKind::DeleteTableRow {
                table_block_id: parse_id(table_block_id.as_ref())?,
                row_id: parse_id(row_id.as_ref())?,
            },
        ))
    }

    pub fn add_table_cell(
        &mut self,
        table_block_id: impl AsRef<str>,
        row_id: impl AsRef<str>,
        after_cell: Option<String>,
        text: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let table_block_id = parse_id(table_block_id.as_ref())?;
        let row_id = parse_id(row_id.as_ref())?;
        let after_cell = after_cell.as_deref().map(parse_id).transpose()?;
        Ok(self.apply(
            "insert-table-cell",
            "table cell",
            OperationKind::InsertTableCell {
                table_block_id,
                row_id,
                after_cell,
                cell: opendoc_core::TableCell::new(vec![Block::paragraph(text)]),
            },
        ))
    }

    pub fn delete_table_cell(
        &mut self,
        table_block_id: impl AsRef<str>,
        row_id: impl AsRef<str>,
        cell_id: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        Ok(self.apply(
            "delete-table-cell",
            "delete table cell",
            OperationKind::DeleteTableCell {
                table_block_id: parse_id(table_block_id.as_ref())?,
                row_id: parse_id(row_id.as_ref())?,
                cell_id: parse_id(cell_id.as_ref())?,
            },
        ))
    }

    // ---- Table structure (PLAN77 E2, ADR 0013) ---------------------------
    //
    // A table is a rectangular grid, so a column is a first-class thing with
    // an identity, and inserting one is one operation over the whole table
    // rather than one per row. The cells the new column needs are derived
    // from the row and column ids inside merge, which is what lets a column
    // inserted here survive a row inserted on another replica at the same
    // moment.

    pub fn insert_table_column(
        &mut self,
        table_block_id: impl AsRef<str>,
        after_column_id: Option<String>,
    ) -> Result<AppDocument, AppApiError> {
        let table_block_id = parse_id(table_block_id.as_ref())?;
        let after_column = after_column_id.as_deref().map(parse_id).transpose()?;
        Ok(self.apply(
            "insert-table-column",
            "table column",
            OperationKind::InsertTableColumn {
                table_block_id,
                after_column,
                column: opendoc_core::TableColumn::auto(),
            },
        ))
    }

    pub fn delete_table_column(
        &mut self,
        table_block_id: impl AsRef<str>,
        column_id: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        Ok(self.apply(
            "delete-table-column",
            "delete table column",
            OperationKind::DeleteTableColumn {
                table_block_id: parse_id(table_block_id.as_ref())?,
                column_id: parse_id(column_id.as_ref())?,
            },
        ))
    }

    pub fn set_table_column_width(
        &mut self,
        table_block_id: impl AsRef<str>,
        column_id: impl AsRef<str>,
        twips: i32,
    ) -> Result<AppDocument, AppApiError> {
        let table_block_id = parse_id(table_block_id.as_ref())?;
        let column_id = parse_id(column_id.as_ref())?;
        let width = opendoc_core::Length::from_twips(twips)
            .map_err(|err| AppApiError::Format(err.to_string()))?;
        if width.twips() < opendoc_core::TableColumn::MIN_WIDTH_TWIPS {
            return Err(AppApiError::Format(
                "table column width is below 0.1in".to_string(),
            ));
        }
        Ok(self.apply(
            "set-table-column-width",
            "table column width",
            OperationKind::SetTableColumnWidth {
                table_block_id,
                column_id,
                width: Some(width),
            },
        ))
    }

    /// Returns the column to auto width. Clearing is spelled as its own
    /// command rather than as a nullable width, so a caller cannot ask for
    /// "auto" by forgetting the number.
    pub fn clear_table_column_width(
        &mut self,
        table_block_id: impl AsRef<str>,
        column_id: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        Ok(self.apply(
            "set-table-column-width",
            "auto table column width",
            OperationKind::SetTableColumnWidth {
                table_block_id: parse_id(table_block_id.as_ref())?,
                column_id: parse_id(column_id.as_ref())?,
                width: None,
            },
        ))
    }

    /// Merges the rectangle that starts at `cell_id`. The cells it covers
    /// keep their content — [`Self::split_table_cell`] hands it straight
    /// back — so merging is never destructive.
    pub fn merge_table_cells(
        &mut self,
        cell_id: impl AsRef<str>,
        row_span: u32,
        column_span: u32,
    ) -> Result<AppDocument, AppApiError> {
        let cell_id = parse_id(cell_id.as_ref())?;
        let span = opendoc_core::CellSpan::new(row_span, column_span)
            .map_err(|err| AppApiError::Format(err.to_string()))?;
        if span.is_single() {
            return Err(AppApiError::Format(
                "merging one cell with itself is not a merge; use split_table_cell".to_string(),
            ));
        }
        Ok(self.apply(
            "set-table-cell-span",
            "merge table cells",
            OperationKind::SetTableCellSpan { cell_id, span },
        ))
    }

    pub fn split_table_cell(
        &mut self,
        cell_id: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        Ok(self.apply(
            "set-table-cell-span",
            "split table cell",
            OperationKind::SetTableCellSpan {
                cell_id: parse_id(cell_id.as_ref())?,
                span: opendoc_core::CellSpan::SINGLE,
            },
        ))
    }

    pub fn set_table_cell_background(
        &mut self,
        cell_id: impl AsRef<str>,
        color: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let cell_id = parse_id(cell_id.as_ref())?;
        let color = parse_color(color.as_ref())?;
        Ok(self
            .set_table_cell_property(cell_id, opendoc_core::TableCellProperty::Background(color)))
    }

    pub fn set_table_cell_border(
        &mut self,
        cell_id: impl AsRef<str>,
        edge: impl AsRef<str>,
        style: impl AsRef<str>,
        twips: i32,
        color: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let cell_id = parse_id(cell_id.as_ref())?;
        let edge = parse_cell_edge(edge.as_ref())?;
        let style = opendoc_core::BorderStyle::parse(style.as_ref().trim())
            .map_err(|err| AppApiError::Format(err.to_string()))?;
        let width = opendoc_core::Length::from_twips(twips)
            .map_err(|err| AppApiError::Format(err.to_string()))?;
        let border = opendoc_core::CellBorder::new(style, width, parse_color(color.as_ref())?)
            .map_err(|err| AppApiError::Format(err.to_string()))?;
        let property = match edge {
            CellEdge::Top => opendoc_core::TableCellProperty::BorderTop(border),
            CellEdge::Bottom => opendoc_core::TableCellProperty::BorderBottom(border),
            CellEdge::Start => opendoc_core::TableCellProperty::BorderStart(border),
            CellEdge::End => opendoc_core::TableCellProperty::BorderEnd(border),
        };
        Ok(self.set_table_cell_property(cell_id, property))
    }

    pub fn set_table_cell_vertical_alignment(
        &mut self,
        cell_id: impl AsRef<str>,
        alignment: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let cell_id = parse_id(cell_id.as_ref())?;
        let alignment = opendoc_core::VerticalAlignment::parse(alignment.as_ref().trim())
            .map_err(|err| AppApiError::Format(err.to_string()))?;
        Ok(self.set_table_cell_property(
            cell_id,
            opendoc_core::TableCellProperty::VerticalAlignment(alignment),
        ))
    }

    pub fn set_table_cell_padding(
        &mut self,
        cell_id: impl AsRef<str>,
        edge: impl AsRef<str>,
        twips: i32,
    ) -> Result<AppDocument, AppApiError> {
        let cell_id = parse_id(cell_id.as_ref())?;
        let edge = parse_cell_edge(edge.as_ref())?;
        let length = opendoc_core::Length::from_twips(twips)
            .map_err(|err| AppApiError::Format(err.to_string()))?;
        if length.is_negative() {
            return Err(AppApiError::Format("cell padding is negative".to_string()));
        }
        let property = match edge {
            CellEdge::Top => opendoc_core::TableCellProperty::PaddingTop(length),
            CellEdge::Bottom => opendoc_core::TableCellProperty::PaddingBottom(length),
            CellEdge::Start => opendoc_core::TableCellProperty::PaddingStart(length),
            CellEdge::End => opendoc_core::TableCellProperty::PaddingEnd(length),
        };
        Ok(self.set_table_cell_property(cell_id, property))
    }

    pub fn clear_table_cell_property(
        &mut self,
        cell_id: impl AsRef<str>,
        key: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let cell_id = parse_id(cell_id.as_ref())?;
        let key = opendoc_core::TableCellPropertyKey::parse(key.as_ref().trim())
            .map_err(|err| AppApiError::Format(err.to_string()))?;
        Ok(self.apply(
            "clear-table-cell-property",
            "clear table cell property",
            OperationKind::ClearTableCellProperty { cell_id, key },
        ))
    }

    fn set_table_cell_property(
        &mut self,
        cell_id: StableId,
        property: opendoc_core::TableCellProperty,
    ) -> AppDocument {
        self.apply(
            "set-table-cell-property",
            "table cell style",
            OperationKind::SetTableCellProperty { cell_id, property },
        )
    }
}

/// Which edge of a cell a border or padding command means. Direction-relative
/// like every other edge in this model: `start` is the leading edge.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CellEdge {
    Top,
    Bottom,
    Start,
    End,
}

/// How many columns the named table has, wherever it is nested. Zero when
/// there is no such table — the operation then degrades in merge, which is
/// where that decision already lives.
fn table_column_count(blocks: &[Block], table_block_id: &StableId) -> usize {
    for block in blocks {
        if let opendoc_core::BlockKind::Table { columns, rows } = &block.kind {
            if &block.id == table_block_id {
                return columns.len();
            }
            for row in rows {
                for cell in &row.cells {
                    let nested = table_column_count(&cell.blocks, table_block_id);
                    if nested > 0 {
                        return nested;
                    }
                }
            }
        }
    }
    0
}

fn parse_cell_edge(name: &str) -> Result<CellEdge, AppApiError> {
    match name.trim() {
        "top" => Ok(CellEdge::Top),
        "bottom" => Ok(CellEdge::Bottom),
        "start" => Ok(CellEdge::Start),
        "end" => Ok(CellEdge::End),
        other => Err(AppApiError::Format(format!("unknown cell edge {other:?}"))),
    }
}
