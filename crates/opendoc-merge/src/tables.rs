//! Table grid mutation, plus the geometry repair every table edit runs through.

use crate::blocks::find_block_mut;
use opendoc_core::{
    Block, BlockKind, CellSpan, Document, Length, ModelWarning, StableId, TableCell, TableColumn,
    TableRow,
};
use std::collections::BTreeSet;

/// The two halves of a table block, for the operations that edit its grid.
///
/// Every table mutation goes through here and then through
/// [`repair_table_geometry`], so no operation can leave a table that
/// `Document::validate` would reject.
pub(crate) fn table_parts_mut<'a>(
    document: &'a mut Document,
    table_block_id: &StableId,
) -> Result<(&'a mut Vec<TableColumn>, &'a mut Vec<TableRow>), TableEditResult> {
    let Some(table) = find_block_mut(&mut document.blocks, table_block_id) else {
        return Err(TableEditResult::MissingTable);
    };
    match &mut table.kind {
        BlockKind::Table { columns, rows } => Ok((columns, rows)),
        _ => Err(TableEditResult::NonTableBlock),
    }
}

/// Restores the grid invariants after any edit, the same way on every
/// replica.
///
/// Two actors editing one table concurrently can each produce an operation
/// that is correct against the grid it was written for and wrong against the
/// grid the merge actually built: a row inserted next to a column has no cell
/// where they cross, and two merges of overlapping rectangles claim the same
/// position. Rather than forbid those pairs, merge applies both and then
/// repairs, deterministically:
///
/// - the grid grows to the widest row, so a cell is never dropped to make the
///   rectangle fit — content loss is never the repair;
/// - missing cells are filled with [`TableCell::filling`], whose id is a
///   function of the row and column, so every replica fills them identically;
/// - spans are clamped to the grid and, where two of them collide, the one
///   earlier in row-major order keeps its rectangle and the other is split
///   back to a single cell.
///
/// See `docs/adr/0013-table-structure-and-merge.md`.
pub(crate) fn repair_table_geometry(
    table_block_id: &StableId,
    columns: &mut Vec<TableColumn>,
    rows: &mut Vec<TableRow>,
) -> TableRepair {
    let mut repair = TableRepair::default();
    let width = columns
        .len()
        .max(rows.iter().map(|row| row.cells.len()).max().unwrap_or(0))
        .max(usize::from(!rows.is_empty()));
    while columns.len() < width {
        repair.added_columns += 1;
        columns.push(TableColumn::filling(table_block_id, columns.len()));
    }
    if rows.is_empty() && !columns.is_empty() {
        repair.added_rows += 1;
        rows.push(TableRow::filling(table_block_id, columns));
    }
    for row in rows.iter_mut() {
        while row.cells.len() < width {
            let column_id = columns[row.cells.len()].id.clone();
            row.cells.push(TableCell::filling(&row.id, &column_id));
            repair.filled_cells += 1;
        }
    }

    let row_count = rows.len();
    let mut claimed: BTreeSet<(usize, usize)> = BTreeSet::new();
    for (row_index, row) in rows.iter_mut().enumerate() {
        for (column_index, cell) in row.cells.iter_mut().enumerate() {
            let span = cell.span;
            if span.is_single() {
                continue;
            }
            let clamped = CellSpan::new(
                span.rows().min((row_count - row_index) as u32),
                span.columns().min((width - column_index) as u32),
            )
            .unwrap_or(CellSpan::SINGLE);
            let rectangle: Vec<(usize, usize)> = (row_index..row_index + clamped.rows() as usize)
                .flat_map(|r| {
                    (column_index..column_index + clamped.columns() as usize).map(move |c| (r, c))
                })
                .collect();
            if clamped.is_single() || rectangle.iter().any(|position| claimed.contains(position)) {
                if !clamped.is_single() || span != clamped {
                    repair.reset_spans += 1;
                }
                cell.span = CellSpan::SINGLE;
                continue;
            }
            if clamped != span {
                repair.clamped_spans += 1;
            }
            cell.span = clamped;
            claimed.extend(rectangle);
        }
    }
    repair
}

/// What [`repair_table_geometry`] had to change. Anything non-zero is worth
/// telling the user about: it means two edits met.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct TableRepair {
    added_columns: usize,
    added_rows: usize,
    filled_cells: usize,
    clamped_spans: usize,
    reset_spans: usize,
}

impl TableRepair {
    fn is_clean(self) -> bool {
        self == TableRepair::default()
    }

    fn describe(self) -> String {
        let mut parts = Vec::new();
        if self.added_columns > 0 {
            parts.push(format!("{} column(s) added", self.added_columns));
        }
        if self.added_rows > 0 {
            parts.push(format!("{} row(s) added", self.added_rows));
        }
        if self.filled_cells > 0 {
            parts.push(format!("{} cell(s) filled in", self.filled_cells));
        }
        if self.clamped_spans > 0 {
            parts.push(format!(
                "{} merge(s) clamped to the grid",
                self.clamped_spans
            ));
        }
        if self.reset_spans > 0 {
            parts.push(format!("{} overlapping merge(s) split", self.reset_spans));
        }
        parts.join(", ")
    }
}

/// Repairs the table and reports it, so a geometry collision between two
/// concurrent edits is visible rather than silent.
pub(crate) fn repair_table(
    document: &mut Document,
    table_block_id: &StableId,
    warnings: &mut Vec<ModelWarning>,
) {
    let Ok((columns, rows)) = table_parts_mut(document, table_block_id) else {
        return;
    };
    let repair = repair_table_geometry(table_block_id, columns, rows);
    if !repair.is_clean() {
        warnings.push(ModelWarning {
            code: "table-geometry-repaired".to_string(),
            message: format!(
                "table {table_block_id} geometry was repaired after concurrent edits: {}",
                repair.describe()
            ),
        });
    }
}

/// Grows the spans that straddle a boundary a new row or column is being
/// inserted at, so a merged region keeps covering what it covered.
pub(crate) fn grow_spans_across_row(rows: &mut [TableRow], insert_at: usize) {
    for (row_index, row) in rows.iter_mut().enumerate().take(insert_at) {
        for cell in row.cells.iter_mut() {
            if row_index + cell.span.rows() as usize > insert_at {
                if let Ok(grown) = CellSpan::new(cell.span.rows() + 1, cell.span.columns()) {
                    cell.span = grown;
                }
            }
        }
    }
}

pub(crate) fn grow_spans_across_column(rows: &mut [TableRow], insert_at: usize) {
    for row in rows.iter_mut() {
        for (cell_index, cell) in row.cells.iter_mut().enumerate() {
            if cell_index < insert_at && cell_index + cell.span.columns() as usize > insert_at {
                if let Ok(grown) = CellSpan::new(cell.span.rows(), cell.span.columns() + 1) {
                    cell.span = grown;
                }
            }
        }
    }
}

/// Shrinks the spans that reach across a row or column being removed.
pub(crate) fn shrink_spans_across_row(rows: &mut [TableRow], removed: usize) {
    for (row_index, row) in rows.iter_mut().enumerate().take(removed) {
        for cell in row.cells.iter_mut() {
            if cell.span.rows() > 1 && row_index + cell.span.rows() as usize > removed {
                if let Ok(shrunk) = CellSpan::new(cell.span.rows() - 1, cell.span.columns()) {
                    cell.span = shrunk;
                }
            }
        }
    }
}

pub(crate) fn shrink_spans_across_column(rows: &mut [TableRow], removed: usize) {
    for row in rows.iter_mut() {
        for (cell_index, cell) in row.cells.iter_mut().enumerate() {
            if cell_index < removed
                && cell.span.columns() > 1
                && cell_index + cell.span.columns() as usize > removed
            {
                if let Ok(shrunk) = CellSpan::new(cell.span.rows(), cell.span.columns() - 1) {
                    cell.span = shrunk;
                }
            }
        }
    }
}

pub(crate) fn insert_table_row(
    document: &mut Document,
    table_block_id: &StableId,
    after_row: Option<StableId>,
    row: TableRow,
) -> TableEditResult {
    let (_, rows) = match table_parts_mut(document, table_block_id) {
        Ok(parts) => parts,
        Err(result) => return result,
    };
    if rows.iter().any(|item| item.id == row.id) {
        return TableEditResult::Duplicate;
    }
    let mut result = TableEditResult::Applied;
    let insert_at = match after_row {
        Some(target) => match rows.iter().position(|item| item.id == target) {
            Some(index) => index + 1,
            None => {
                result = TableEditResult::AnchorDegraded;
                rows.len()
            }
        },
        None => rows.len(),
    };
    grow_spans_across_row(rows, insert_at);
    rows.insert(insert_at, row);
    result
}

pub(crate) fn delete_table_row(
    document: &mut Document,
    table_block_id: &StableId,
    row_id: &StableId,
) -> TableEditResult {
    let (columns, rows) = match table_parts_mut(document, table_block_id) {
        Ok(parts) => parts,
        Err(result) => return result,
    };
    let Some(index) = rows.iter().position(|row| &row.id == row_id) else {
        return TableEditResult::MissingRow;
    };
    shrink_spans_across_row(rows, index);
    rows.remove(index);
    if rows.is_empty() {
        rows.push(TableRow::filling(table_block_id, columns));
        TableEditResult::AnchorDegraded
    } else {
        TableEditResult::Applied
    }
}

pub(crate) fn insert_table_column(
    document: &mut Document,
    table_block_id: &StableId,
    after_column: Option<StableId>,
    column: TableColumn,
) -> TableEditResult {
    let (columns, rows) = match table_parts_mut(document, table_block_id) {
        Ok(parts) => parts,
        Err(result) => return result,
    };
    if columns.iter().any(|item| item.id == column.id) {
        return TableEditResult::Duplicate;
    }
    let mut result = TableEditResult::Applied;
    let insert_at = match after_column {
        Some(target) => match columns.iter().position(|item| item.id == target) {
            Some(index) => index + 1,
            None => {
                result = TableEditResult::AnchorDegraded;
                columns.len()
            }
        },
        None => columns.len(),
    };
    grow_spans_across_column(rows, insert_at);
    for row in rows.iter_mut() {
        let cell = TableCell::filling(&row.id, &column.id);
        let at = insert_at.min(row.cells.len());
        row.cells.insert(at, cell);
    }
    columns.insert(insert_at, column);
    result
}

pub(crate) fn delete_table_column(
    document: &mut Document,
    table_block_id: &StableId,
    column_id: &StableId,
) -> TableEditResult {
    let (columns, rows) = match table_parts_mut(document, table_block_id) {
        Ok(parts) => parts,
        Err(result) => return result,
    };
    let Some(index) = columns.iter().position(|column| &column.id == column_id) else {
        return TableEditResult::MissingColumn;
    };
    shrink_spans_across_column(rows, index);
    for row in rows.iter_mut() {
        if index < row.cells.len() {
            row.cells.remove(index);
        }
    }
    columns.remove(index);
    if columns.is_empty() {
        let column = TableColumn::filling(table_block_id, 0);
        for row in rows.iter_mut() {
            row.cells.push(TableCell::filling(&row.id, &column.id));
        }
        columns.push(column);
        TableEditResult::AnchorDegraded
    } else {
        TableEditResult::Applied
    }
}

pub(crate) fn set_table_column_width(
    document: &mut Document,
    table_block_id: &StableId,
    column_id: &StableId,
    width: Option<Length>,
) -> TableEditResult {
    let (columns, _) = match table_parts_mut(document, table_block_id) {
        Ok(parts) => parts,
        Err(result) => return result,
    };
    let Some(column) = columns.iter_mut().find(|column| &column.id == column_id) else {
        return TableEditResult::MissingColumn;
    };
    let previous = column.width;
    column.width = width;
    if column.validate().is_err() {
        column.width = previous;
        return TableEditResult::InvalidPayload;
    }
    TableEditResult::Applied
}

/// The table block a cell belongs to, wherever it is nested.
pub(crate) fn table_block_id_for_cell(blocks: &[Block], cell_id: &StableId) -> Option<StableId> {
    for block in blocks {
        if let BlockKind::Table { rows, .. } = &block.kind {
            for row in rows {
                for cell in &row.cells {
                    if &cell.id == cell_id {
                        return Some(block.id.clone());
                    }
                    if let Some(found) = table_block_id_for_cell(&cell.blocks, cell_id) {
                        return Some(found);
                    }
                }
            }
        }
    }
    None
}

pub(crate) fn find_table_cell_mut<'a>(
    blocks: &'a mut [Block],
    cell_id: &StableId,
) -> Option<&'a mut TableCell> {
    for block in blocks {
        if let BlockKind::Table { rows, .. } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    if &cell.id == cell_id {
                        return Some(cell);
                    }
                    if let Some(found) = find_table_cell_mut(&mut cell.blocks, cell_id) {
                        return Some(found);
                    }
                }
            }
        }
    }
    None
}

/// Merges or splits a cell.
///
/// A merge absorbs whatever merges it swallows — the cells it now covers are
/// split back to single cells — so the result is a valid grid whether or not
/// the region was already merged. The absorbed cells keep their content; it
/// reappears when the outer merge is split.
pub(crate) fn set_table_cell_span(
    document: &mut Document,
    cell_id: &StableId,
    span: CellSpan,
) -> TableEditResult {
    let Some(table_block_id) = table_block_id_for_cell(&document.blocks, cell_id) else {
        return TableEditResult::MissingCell;
    };
    let Ok((columns, rows)) = table_parts_mut(document, &table_block_id) else {
        return TableEditResult::MissingCell;
    };
    let width = columns.len();
    let Some((row_index, column_index)) = rows.iter().enumerate().find_map(|(row_index, row)| {
        row.cells
            .iter()
            .position(|cell| &cell.id == cell_id)
            .map(|column_index| (row_index, column_index))
    }) else {
        return TableEditResult::MissingCell;
    };
    if row_index + span.rows() as usize > rows.len()
        || column_index + span.columns() as usize > width
    {
        return TableEditResult::SpanOutsideGrid;
    }
    rows[row_index].cells[column_index].span = span;
    for covered_row in row_index..row_index + span.rows() as usize {
        for covered_column in column_index..column_index + span.columns() as usize {
            if (covered_row, covered_column) != (row_index, column_index) {
                rows[covered_row].cells[covered_column].span = CellSpan::SINGLE;
            }
        }
    }
    TableEditResult::Applied
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TableEditResult {
    Applied,
    AnchorDegraded,
    Duplicate,
    MissingTable,
    NonTableBlock,
    MissingRow,
    MissingColumn,
    MissingCell,
    SpanOutsideGrid,
    InvalidPayload,
}

/// Adds a cell to one row — which, on a rectangular grid, is adding a
/// *column* and seeding one row's cell in it.
///
/// A table whose rows have different lengths is not a table anyone can edit,
/// so there is no longer an operation that lengthens one row alone. The
/// column this creates is identified by a value derived from the payload
/// cell, so two replicas replaying the operation create the same column.
pub(crate) fn insert_table_cell(
    document: &mut Document,
    table_block_id: &StableId,
    row_id: &StableId,
    after_cell: Option<StableId>,
    cell: TableCell,
) -> TableEditResult {
    let (columns, rows) = match table_parts_mut(document, table_block_id) {
        Ok(parts) => parts,
        Err(result) => return result,
    };
    if !rows.iter().any(|row| &row.id == row_id) {
        return TableEditResult::MissingRow;
    }
    if rows
        .iter()
        .any(|row| row.cells.iter().any(|item| item.id == cell.id))
    {
        return TableEditResult::Duplicate;
    }
    let column = TableColumn {
        id: opendoc_core::derived_stable_id("column", &[cell.id.as_str()]),
        width: None,
    };
    let mut result = TableEditResult::Applied;
    let insert_at = match &after_cell {
        Some(target) => match column_index_of_cell(rows, target) {
            Some(index) => index + 1,
            None => {
                result = TableEditResult::AnchorDegraded;
                columns.len()
            }
        },
        None => columns.len(),
    };
    grow_spans_across_column(rows, insert_at);
    let mut seed = Some(cell);
    for row in rows.iter_mut() {
        let new_cell = match (&row.id == row_id).then(|| seed.take()).flatten() {
            Some(cell) => cell,
            None => TableCell::filling(&row.id, &column.id),
        };
        let at = insert_at.min(row.cells.len());
        row.cells.insert(at, new_cell);
    }
    columns.insert(insert_at, column);
    result
}

pub(crate) fn column_index_of_cell(rows: &[TableRow], cell_id: &StableId) -> Option<usize> {
    rows.iter()
        .find_map(|row| row.cells.iter().position(|cell| &cell.id == cell_id))
}

/// Removes a cell — which, on a rectangular grid, removes the column it sits
/// in. See [`insert_table_cell`].
pub(crate) fn delete_table_cell(
    document: &mut Document,
    table_block_id: &StableId,
    row_id: &StableId,
    cell_id: &StableId,
) -> TableEditResult {
    let (columns, rows) = match table_parts_mut(document, table_block_id) {
        Ok(parts) => parts,
        Err(result) => return result,
    };
    if !rows.iter().any(|row| &row.id == row_id) {
        return TableEditResult::MissingRow;
    }
    let Some(index) = column_index_of_cell(rows, cell_id) else {
        return TableEditResult::MissingCell;
    };
    let Some(column_id) = columns.get(index).map(|column| column.id.clone()) else {
        return TableEditResult::MissingCell;
    };
    delete_table_column(document, table_block_id, &column_id)
}
