//! Table commands: rows, columns, cells, spans and cell styling.

use super::*;

impl OpenDocApp {
    pub fn add_table(&mut self) -> Result<AppDocument, AppApiError> {
        self.apply(
            "insert-block",
            "table",
            OperationKind::InsertBlock {
                position: InsertPosition::after_or_last(
                    self.document.blocks.last().map(|block| block.id.clone()),
                ),
                block: default_table_block(),
            },
        )
    }

    pub fn insert_table_after(
        &mut self,
        after_block_id: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let after = parse_id(after_block_id.as_ref())?;
        if find_block_in_blocks(&self.document.blocks, &after).is_none() {
            return Err(AppApiError::NotFound(format!(
                "block {after} was not found"
            )));
        }
        self.apply(
            "insert-block",
            "table after block",
            OperationKind::InsertBlock {
                position: InsertPosition::After(after),
                block: default_table_block(),
            },
        )
    }

    /// Adds a row at `after_row`: the id of the row to follow, the literal
    /// [`InsertPosition::FIRST_KEYWORD`] to go above every row, or `None` to
    /// append. The keyword is the one position an id cannot name.
    pub fn add_table_row(
        &mut self,
        table_block_id: impl AsRef<str>,
        after_row: Option<String>,
        text: impl Into<String>,
    ) -> Result<AppDocument, AppApiError> {
        let table_block_id = parse_id(table_block_id.as_ref())?;
        let position = parse_insert_position(after_row.as_deref())?;
        // One cell per column, not one cell: a row that is short is a ragged
        // table, and merge would have to repair it — and say so — on every
        // ordinary "insert row".
        //
        // And every cell **names the column it was typed into**. The columns
        // read here are the ones *this replica* can see; another replica may
        // delete one before this operation is merged, and without the binding
        // every cell after the deleted one would silently move one column to
        // the left on every replica at once. ADR 0019.
        let columns = table_column_ids(&self.document.blocks, &table_block_id);
        let mut cells = vec![opendoc_core::TableCell::new(vec![Block::paragraph(text)])];
        while cells.len() < columns.len() {
            cells.push(opendoc_core::TableCell::empty());
        }
        let cell_columns: BTreeMap<StableId, StableId> = cells
            .iter()
            .zip(columns.iter())
            .map(|(cell, column_id)| (cell.id.clone(), column_id.clone()))
            .collect();
        self.apply(
            "insert-table-row",
            "table row",
            OperationKind::InsertTableRow {
                table_block_id,
                position,
                row: opendoc_core::TableRow {
                    id: StableId::new("row"),
                    height: None,
                    header: false,
                    cells,
                },
                cell_columns,
            },
        )
    }

    pub fn delete_table_row(
        &mut self,
        table_block_id: impl AsRef<str>,
        row_id: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        self.apply(
            "delete-table-row",
            "delete table row",
            OperationKind::DeleteTableRow {
                table_block_id: parse_id(table_block_id.as_ref())?,
                row_id: parse_id(row_id.as_ref())?,
            },
        )
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
        self.apply(
            "insert-table-cell",
            "table cell",
            OperationKind::InsertTableCell {
                table_block_id,
                row_id,
                // The command surface still names a cell to follow, or
                // nothing at all for an append. `InsertPosition::First` is
                // reachable through the operation but not yet through this
                // argument — the anchor's meaning is documented in
                // `opendoc-api`, so widening it belongs there.
                position: InsertPosition::after_or_last(after_cell),
                cell: opendoc_core::TableCell::new(vec![Block::paragraph(text)]),
            },
        )
    }

    pub fn delete_table_cell(
        &mut self,
        table_block_id: impl AsRef<str>,
        row_id: impl AsRef<str>,
        cell_id: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        self.apply(
            "delete-table-cell",
            "delete table cell",
            OperationKind::DeleteTableCell {
                table_block_id: parse_id(table_block_id.as_ref())?,
                row_id: parse_id(row_id.as_ref())?,
                cell_id: parse_id(cell_id.as_ref())?,
            },
        )
    }

    // ---- Table structure (PLAN77 E2, ADR 0013) ---------------------------
    //
    // A table is a rectangular grid, so a column is a first-class thing with
    // an identity, and inserting one is one operation over the whole table
    // rather than one per row. The cells the new column needs are derived
    // from the row and column ids inside merge, which is what lets a column
    // inserted here survive a row inserted on another replica at the same
    // moment.

    /// Adds a column at `after_column_id`, read the same way
    /// [`OpenDocApp::add_table_row`] reads its anchor.
    pub fn insert_table_column(
        &mut self,
        table_block_id: impl AsRef<str>,
        after_column_id: Option<String>,
    ) -> Result<AppDocument, AppApiError> {
        let table_block_id = parse_id(table_block_id.as_ref())?;
        let position = parse_insert_position(after_column_id.as_deref())?;
        self.apply(
            "insert-table-column",
            "table column",
            OperationKind::InsertTableColumn {
                table_block_id,
                position,
                column: opendoc_core::TableColumn::auto(),
            },
        )
    }

    pub fn delete_table_column(
        &mut self,
        table_block_id: impl AsRef<str>,
        column_id: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        self.apply(
            "delete-table-column",
            "delete table column",
            OperationKind::DeleteTableColumn {
                table_block_id: parse_id(table_block_id.as_ref())?,
                column_id: parse_id(column_id.as_ref())?,
            },
        )
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
        self.apply(
            "set-table-column-width",
            "table column width",
            OperationKind::SetTableColumnWidth {
                table_block_id,
                column_id,
                width: Some(width),
            },
        )
    }

    /// Returns the column to auto width. Clearing is spelled as its own
    /// command rather than as a nullable width, so a caller cannot ask for
    /// "auto" by forgetting the number.
    pub fn clear_table_column_width(
        &mut self,
        table_block_id: impl AsRef<str>,
        column_id: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        self.apply(
            "set-table-column-width",
            "auto table column width",
            OperationKind::SetTableColumnWidth {
                table_block_id: parse_id(table_block_id.as_ref())?,
                column_id: parse_id(column_id.as_ref())?,
                width: None,
            },
        )
    }

    pub fn set_table_row_height(
        &mut self,
        table_block_id: impl AsRef<str>,
        row_id: impl AsRef<str>,
        twips: i32,
    ) -> Result<AppDocument, AppApiError> {
        let height = opendoc_core::Length::from_twips(twips)
            .map_err(|err| AppApiError::Format(err.to_string()))?;
        if height.is_negative() || height.twips() == 0 {
            return Err(AppApiError::Format(
                "table row height must be positive".to_string(),
            ));
        }
        self.apply(
            "set-table-row-height",
            "table row height",
            OperationKind::SetTableRowHeight {
                table_block_id: parse_id(table_block_id.as_ref())?,
                row_id: parse_id(row_id.as_ref())?,
                height: Some(height),
            },
        )
    }

    pub fn clear_table_row_height(
        &mut self,
        table_block_id: impl AsRef<str>,
        row_id: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        self.apply(
            "set-table-row-height",
            "auto table row height",
            OperationKind::SetTableRowHeight {
                table_block_id: parse_id(table_block_id.as_ref())?,
                row_id: parse_id(row_id.as_ref())?,
                height: None,
            },
        )
    }

    pub fn set_table_row_header(
        &mut self,
        table_block_id: impl AsRef<str>,
        row_id: impl AsRef<str>,
        header: bool,
    ) -> Result<AppDocument, AppApiError> {
        self.apply(
            "set-table-row-header",
            if header {
                "table header row"
            } else {
                "table body row"
            },
            OperationKind::SetTableRowHeader {
                table_block_id: parse_id(table_block_id.as_ref())?,
                row_id: parse_id(row_id.as_ref())?,
                header,
            },
        )
    }

    /// Commits an application-derived table row order. The payload is row
    /// identities, not sort keys, so every replica replays exactly this order.
    pub fn reorder_table_rows(
        &mut self,
        table_block_id: impl AsRef<str>,
        row_ids: Vec<String>,
    ) -> Result<AppDocument, AppApiError> {
        let row_ids = row_ids
            .iter()
            .map(|id| parse_id(id))
            .collect::<Result<Vec<_>, _>>()?;
        self.apply(
            "reorder-table-rows",
            "table row order",
            OperationKind::ReorderTableRows {
                table_block_id: parse_id(table_block_id.as_ref())?,
                row_ids,
            },
        )
    }

    /// Sorts body rows by the source-visible text of one column. Header rows
    /// are a leading pinned prefix and do not participate in the comparison.
    pub fn sort_table_rows(
        &mut self,
        table_block_id: impl AsRef<str>,
        column_id: impl AsRef<str>,
        descending: bool,
    ) -> Result<AppDocument, AppApiError> {
        let table_block_id = parse_id(table_block_id.as_ref())?;
        let column_id = parse_id(column_id.as_ref())?;
        let block =
            crate::document_tree::find_block_in_blocks(&self.document.blocks, &table_block_id)
                .ok_or_else(|| {
                    AppApiError::NotFound(format!("table block {table_block_id} was not found"))
                })?;
        let opendoc_core::BlockKind::Table { columns, rows, .. } = &block.kind else {
            return Err(AppApiError::Conflict(format!(
                "block {table_block_id} is not a table"
            )));
        };
        let column = columns
            .iter()
            .position(|column| column.id == column_id)
            .ok_or_else(|| {
                AppApiError::NotFound(format!("table column {column_id} was not found"))
            })?;
        let header_count = rows.iter().take_while(|row| row.header).count();
        let citations = self.document.citation_database.clone();
        let mut body: Vec<_> = rows[header_count..].iter().collect();
        body.sort_by(|left, right| {
            let left_key = table_sort_key(&left.cells[column].blocks, &citations);
            let right_key = table_sort_key(&right.cells[column].blocks, &citations);
            let order = left_key
                .cmp(&right_key)
                .then_with(|| left.id.cmp(&right.id));
            if descending {
                order.reverse()
            } else {
                order
            }
        });
        let row_ids = rows[..header_count]
            .iter()
            .chain(body)
            .map(|row| row.id.to_string())
            .collect();
        self.reorder_table_rows(table_block_id.to_string(), row_ids)
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
        self.apply(
            "set-table-cell-span",
            "merge table cells",
            OperationKind::SetTableCellSpan { cell_id, span },
        )
    }

    pub fn split_table_cell(
        &mut self,
        cell_id: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        self.apply(
            "set-table-cell-span",
            "split table cell",
            OperationKind::SetTableCellSpan {
                cell_id: parse_id(cell_id.as_ref())?,
                span: opendoc_core::CellSpan::SINGLE,
            },
        )
    }

    pub fn set_table_cell_background(
        &mut self,
        cell_id: impl AsRef<str>,
        color: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let cell_id = parse_id(cell_id.as_ref())?;
        let color = parse_color(color.as_ref())?;
        self.set_table_cell_property(cell_id, opendoc_core::TableCellProperty::Background(color))
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
        self.set_table_cell_property(cell_id, property)
    }

    pub fn set_table_border(
        &mut self,
        table_block_id: impl AsRef<str>,
        style: impl AsRef<str>,
        twips: i32,
        color: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let style = opendoc_core::BorderStyle::parse(style.as_ref().trim())
            .map_err(|err| AppApiError::Format(err.to_string()))?;
        let width = opendoc_core::Length::from_twips(twips)
            .map_err(|err| AppApiError::Format(err.to_string()))?;
        let border = opendoc_core::CellBorder::new(style, width, parse_color(color.as_ref())?)
            .map_err(|err| AppApiError::Format(err.to_string()))?;
        self.apply(
            "set-table-border",
            "table border",
            OperationKind::SetTableBorder {
                table_block_id: parse_id(table_block_id.as_ref())?,
                border: Some(border),
            },
        )
    }

    pub fn clear_table_border(
        &mut self,
        table_block_id: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        self.apply(
            "set-table-border",
            "inherited table border",
            OperationKind::SetTableBorder {
                table_block_id: parse_id(table_block_id.as_ref())?,
                border: None,
            },
        )
    }

    pub fn set_table_alignment(
        &mut self,
        table_block_id: impl AsRef<str>,
        alignment: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let alignment = opendoc_core::TableAlignment::parse(alignment.as_ref().trim())
            .map_err(|err| AppApiError::Format(err.to_string()))?;
        self.apply(
            "set-table-alignment",
            "table alignment",
            OperationKind::SetTableAlignment {
                table_block_id: parse_id(table_block_id.as_ref())?,
                alignment: Some(alignment),
            },
        )
    }

    pub fn clear_table_alignment(
        &mut self,
        table_block_id: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        self.apply(
            "set-table-alignment",
            "inherited table alignment",
            OperationKind::SetTableAlignment {
                table_block_id: parse_id(table_block_id.as_ref())?,
                alignment: None,
            },
        )
    }

    pub fn set_table_cell_vertical_alignment(
        &mut self,
        cell_id: impl AsRef<str>,
        alignment: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let cell_id = parse_id(cell_id.as_ref())?;
        let alignment = opendoc_core::VerticalAlignment::parse(alignment.as_ref().trim())
            .map_err(|err| AppApiError::Format(err.to_string()))?;
        self.set_table_cell_property(
            cell_id,
            opendoc_core::TableCellProperty::VerticalAlignment(alignment),
        )
    }

    /// Sets the explicit row-header role. It is never inferred from position.
    pub fn set_table_cell_row_header(
        &mut self,
        cell_id: impl AsRef<str>,
        row_header: bool,
    ) -> Result<AppDocument, AppApiError> {
        self.set_table_cell_property(
            parse_id(cell_id.as_ref())?,
            opendoc_core::TableCellProperty::RowHeader(row_header),
        )
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
        self.set_table_cell_property(cell_id, property)
    }

    pub fn clear_table_cell_property(
        &mut self,
        cell_id: impl AsRef<str>,
        key: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let cell_id = parse_id(cell_id.as_ref())?;
        let key = opendoc_core::TableCellPropertyKey::parse(key.as_ref().trim())
            .map_err(|err| AppApiError::Format(err.to_string()))?;
        self.apply(
            "clear-table-cell-property",
            "clear table cell property",
            OperationKind::ClearTableCellProperty { cell_id, key },
        )
    }

    fn set_table_cell_property(
        &mut self,
        cell_id: StableId,
        property: opendoc_core::TableCellProperty,
    ) -> Result<AppDocument, AppApiError> {
        self.apply(
            "set-table-cell-property",
            "table cell style",
            OperationKind::SetTableCellProperty { cell_id, property },
        )
    }
}

fn table_sort_key(blocks: &[Block], citations: &opendoc_core::CitationDatabase) -> String {
    let mut document = opendoc_core::Document::new("table sort key");
    document.blocks = blocks.to_vec();
    document.citation_database = citations.clone();
    document.visible_text().to_lowercase()
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

/// The identities of the named table's columns, left to right, wherever the
/// table is nested. Empty when there is no such table — the operation then
/// degrades in merge, which is where that decision already lives.
fn table_column_ids(blocks: &[Block], table_block_id: &StableId) -> Vec<StableId> {
    for block in blocks {
        if let opendoc_core::BlockKind::Table { columns, rows, .. } = &block.kind {
            if &block.id == table_block_id {
                return columns.iter().map(|column| column.id.clone()).collect();
            }
            for row in rows {
                for cell in &row.cells {
                    let nested = table_column_ids(&cell.blocks, table_block_id);
                    if !nested.is_empty() {
                        return nested;
                    }
                }
            }
        }
    }
    Vec::new()
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

#[cfg(test)]
mod tests {
    use crate::{OpenDocApp, OperationKind};
    use serde_json::json;

    /// The gesture that generates unbound cells, checked where it is
    /// generated.
    ///
    /// `add_table_row` builds one cell per column *as this replica sees them*.
    /// Until ADR 0019 those cells named no column at all, so the merge could
    /// only place them by index — and a column deleted on another replica
    /// before this operation landed moved every cell after it one column to
    /// the left, on every replica at once. The operation now carries the
    /// binding, and this is the seam where it has to be attached.
    #[test]
    fn adding_a_table_row_binds_every_cell_to_the_column_it_was_typed_into() {
        let mut app = OpenDocApp::new_empty_document();
        app.dispatch_command("create_document", json!({ "title": "Doc" }))
            .expect("create");
        let anchor = app.source_document().blocks[0].id.to_string();
        app.dispatch_command("insert_table_after", json!({ "afterBlockId": anchor }))
            .expect("a table");
        let table_block = app.source_document().blocks[1].clone();
        let columns: Vec<String> = match &table_block.kind {
            opendoc_core::BlockKind::Table { columns, .. } => {
                columns.iter().map(|column| column.id.to_string()).collect()
            }
            other => panic!("expected a table, got {other:?}"),
        };
        assert!(columns.len() > 1, "the fixture needs several columns");

        app.dispatch_command(
            "add_table_row",
            json!({ "tableBlockId": table_block.id.to_string(), "text": "typed" }),
        )
        .expect("a row");

        let operation = app
            .collaboration_operations()
            .into_iter()
            .rev()
            .find(|operation| matches!(operation.kind, OperationKind::InsertTableRow { .. }))
            .expect("the row insert is in the log");
        match &operation.kind {
            OperationKind::InsertTableRow {
                row, cell_columns, ..
            } => {
                assert_eq!(
                    cell_columns.len(),
                    row.cells.len(),
                    "every cell names a column"
                );
                for (cell, column_id) in row.cells.iter().zip(columns.iter()) {
                    assert_eq!(
                        cell_columns
                            .get(&cell.id)
                            .map(|column| column.to_string())
                            .as_deref(),
                        Some(column_id.as_str()),
                        "cell {} names the column it was typed into",
                        cell.id
                    );
                }
            }
            other => panic!("expected InsertTableRow, got {other:?}"),
        }
    }
}
