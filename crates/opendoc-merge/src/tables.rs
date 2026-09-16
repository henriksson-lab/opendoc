//! Table grid mutation, plus the geometry repair every table edit runs through.

use crate::blocks::find_block_mut;
use opendoc_core::{
    Block, BlockKind, CellSpan, Document, InsertPosition, Length, ModelWarning, StableId,
    TableCell, TableColumn, TableRow,
};
use std::collections::{BTreeMap, BTreeSet};

/// Which column each cell of an [`OperationKind::InsertTableRow`] payload was
/// written into, keyed on the cell's own identity.
///
/// [`OperationKind::InsertTableRow`]: crate::OperationKind::InsertTableRow
pub(crate) type CellColumnBindings = BTreeMap<StableId, StableId>;

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
        BlockKind::Table { columns, rows, .. } => Ok((columns, rows)),
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
        // Derived from the **cell** that has no column, never from the index
        // the hole is at. An index-derived identity was a live merge failure:
        // synthesising at index 3, deleting a column before it and
        // synthesising at index 3 again minted the same column id twice, and
        // the table then decoded as `duplicate table column id`. A cell id is
        // unique in a document, so a column derived from one is unique too.
        let index = columns.len();
        let seed = rows.iter().find_map(|row| row.cells.get(index));
        let mut column = match seed {
            Some(cell) => TableColumn::for_cell(&cell.id),
            // No cell anywhere to derive from: the table has no grid at all,
            // so it gets the one column a table is never without. Reached at
            // most once, because it is only reachable while `columns` is
            // empty.
            None => TableColumn::filling(table_block_id),
        };
        // Deriving from a unique cell cannot collide with another derived
        // column (a cell sits at its own column's index, never past the end),
        // but a payload is not obliged to be well formed, and a duplicate
        // column id here would be the very failure this replaces. Walking to
        // the first free derivation stays a pure function of the grid.
        while columns.iter().any(|existing| existing.id == column.id) {
            column = TableColumn::for_cell(&column.id);
        }
        columns.push(column);
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

/// What binding a payload row's cells to the live columns had to do.
///
/// Every field is something the user is told about: a cell that named a
/// deleted column is content that is gone, and a payload with no bindings at
/// all is an operation read by a rule weaker than the one it was written
/// under.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct RowBinding {
    /// The payload carried no bindings, so its cells were read positionally —
    /// an operation from a journal written before the binding existed.
    pub(crate) legacy: bool,
    /// Cells whose column no longer exists. Their content is gone, because
    /// the column it was in is gone.
    pub(crate) dropped_cells: usize,
    /// Columns the payload said nothing about, filled with a derived empty
    /// cell: a column another actor added concurrently.
    pub(crate) filled_cells: usize,
}

/// Places a payload row's cells in the columns they were **written into**,
/// rather than in the columns that happen to sit at the same indices now.
///
/// This is the cell -> column binding ADR 0013 left out. A row payload is
/// generated against the columns one replica could see; by the time the merge
/// applies it, another replica may have deleted a column before them or added
/// one between them, and a positional reading then silently moves every cell
/// one column to the left. Every replica agreed on the same wrong table — the
/// ADR 0007 failure mode, one level down — and reported it only as a generic
/// `table-geometry-repaired` warning.
///
/// The result is exactly one cell per live column, in column order, so the
/// grid this returns is rectangular by construction and the index-addressed
/// operations downstream (a column delete removes `cells[index]`) stay sound.
fn bind_row_cells(
    row: TableRow,
    bindings: &CellColumnBindings,
    columns: &[TableColumn],
) -> (TableRow, RowBinding) {
    let mut report = RowBinding::default();
    if columns.is_empty() {
        // Nothing to bind to. The repair builds the columns from the cells.
        return (row, report);
    }
    let TableRow {
        id,
        height,
        header,
        cells,
    } = row;
    if bindings.is_empty() && !cells.is_empty() {
        // An operation written before the binding existed. It is read exactly
        // as it was written — positionally — and the merge says so, the way a
        // repository with a pre-split operation sequence is read as written
        // and reports `legacy-operation-sequence-gap`.
        report.legacy = true;
        let mut cells = cells;
        while cells.len() < columns.len() {
            let column_id = columns[cells.len()].id.clone();
            cells.push(TableCell::filling(&id, &column_id));
            report.filled_cells += 1;
        }
        return (
            TableRow {
                id,
                height,
                header,
                cells,
            },
            report,
        );
    }
    let mut by_column: BTreeMap<StableId, TableCell> = BTreeMap::new();
    for cell in cells {
        match bindings.get(&cell.id) {
            Some(column_id) => {
                // Two cells claiming one column is a malformed payload. The
                // first keeps the column; the second is a cell with nowhere
                // to be, and is counted as dropped like any other.
                if by_column.insert(column_id.clone(), cell).is_some() {
                    report.dropped_cells += 1;
                }
            }
            None => report.dropped_cells += 1,
        }
    }
    let mut bound = Vec::with_capacity(columns.len());
    for column in columns {
        match by_column.remove(&column.id) {
            Some(cell) => bound.push(cell),
            None => {
                report.filled_cells += 1;
                bound.push(TableCell::filling(&id, &column.id));
            }
        }
    }
    // Whatever is left named a column that is no longer in the grid, so the
    // column it was typed into has been deleted and its content with it. That
    // is what a column delete means; the alternative — resurrecting the
    // column — would make the result depend on whether the row insert was
    // ordered before or after the delete, which is the bug.
    report.dropped_cells += by_column.len();
    (
        TableRow {
            id,
            height,
            header,
            cells: bound,
        },
        report,
    )
}

pub(crate) fn insert_table_row(
    document: &mut Document,
    table_block_id: &StableId,
    position: InsertPosition,
    row: TableRow,
    bindings: &CellColumnBindings,
) -> (TableEditResult, RowBinding) {
    let (columns, rows) = match table_parts_mut(document, table_block_id) {
        Ok(parts) => parts,
        Err(result) => return (result, RowBinding::default()),
    };
    if rows.iter().any(|item| item.id == row.id) {
        return (TableEditResult::Duplicate, RowBinding::default());
    }
    let columns = columns.clone();
    let (row, binding) = bind_row_cells(row, bindings, &columns);
    let anchor_index = position
        .anchor()
        .map(|target| rows.iter().position(|item| &item.id == target));
    // Only an anchored insert can degrade, and only when the anchor is gone:
    // `First` and `Last` name no sibling that could have been deleted.
    let result = match anchor_index {
        Some(None) => TableEditResult::AnchorDegraded,
        _ => TableEditResult::Applied,
    };
    let insert_at = position.index(rows.len(), anchor_index.flatten());
    grow_spans_across_row(rows, insert_at);
    rows.insert(insert_at, row);
    (result, binding)
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
    position: InsertPosition,
    column: TableColumn,
) -> TableEditResult {
    let (columns, rows) = match table_parts_mut(document, table_block_id) {
        Ok(parts) => parts,
        Err(result) => return result,
    };
    if columns.iter().any(|item| item.id == column.id) {
        return TableEditResult::Duplicate;
    }
    let anchor_index = position
        .anchor()
        .map(|target| columns.iter().position(|item| &item.id == target));
    let result = match anchor_index {
        Some(None) => TableEditResult::AnchorDegraded,
        _ => TableEditResult::Applied,
    };
    let insert_at = position.index(columns.len(), anchor_index.flatten());
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
        let column = TableColumn::filling(table_block_id);
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

pub(crate) fn set_table_row_height(
    document: &mut Document,
    table_block_id: &StableId,
    row_id: &StableId,
    height: Option<Length>,
) -> TableEditResult {
    let (_, rows) = match table_parts_mut(document, table_block_id) {
        Ok(parts) => parts,
        Err(result) => return result,
    };
    let Some(row) = rows.iter_mut().find(|row| &row.id == row_id) else {
        return TableEditResult::MissingRow;
    };
    if let Some(height) = height {
        if height.is_negative() || height.twips() == 0 {
            return TableEditResult::InvalidPayload;
        }
    }
    row.height = height;
    TableEditResult::Applied
}

pub(crate) fn set_table_row_header(
    document: &mut Document,
    table_block_id: &StableId,
    row_id: &StableId,
    header: bool,
) -> TableEditResult {
    let Some(block) = find_block_mut(&mut document.blocks, table_block_id) else {
        return TableEditResult::MissingTable;
    };
    let BlockKind::Table { rows, .. } = &mut block.kind else {
        return TableEditResult::NonTableBlock;
    };
    let Some(row) = rows.iter_mut().find(|row| row.id == *row_id) else {
        return TableEditResult::MissingRow;
    };
    row.header = header;
    TableEditResult::Applied
}

pub(crate) fn reorder_table_rows(
    document: &mut Document,
    table_block_id: &StableId,
    row_ids: &[StableId],
) -> TableEditResult {
    let (_, rows) = match table_parts_mut(document, table_block_id) {
        Ok(parts) => parts,
        Err(error) => return error,
    };
    let requested: BTreeSet<&StableId> = row_ids.iter().collect();
    let existing: BTreeSet<&StableId> = rows.iter().map(|row| &row.id).collect();
    if row_ids.len() != rows.len() || requested != existing {
        return TableEditResult::InvalidPayload;
    }
    let mut by_id: BTreeMap<StableId, TableRow> = std::mem::take(rows)
        .into_iter()
        .map(|row| (row.id.clone(), row))
        .collect();
    let mut reordered = Vec::with_capacity(row_ids.len());
    for id in row_ids {
        let row = by_id.remove(id).expect("validated table row id");
        reordered.push(row);
    }
    *rows = reordered;
    TableEditResult::Applied
}

pub(crate) fn set_table_border(
    document: &mut Document,
    table_block_id: &StableId,
    border: Option<opendoc_core::CellBorder>,
) -> TableEditResult {
    let Some(table) = find_block_mut(&mut document.blocks, table_block_id) else {
        return TableEditResult::MissingTable;
    };
    let BlockKind::Table { properties, .. } = &mut table.kind else {
        return TableEditResult::NonTableBlock;
    };
    if let Some(border) = border {
        if border.validate().is_err() {
            return TableEditResult::InvalidPayload;
        }
    }
    properties.border = border;
    TableEditResult::Applied
}

pub(crate) fn set_table_alignment(
    document: &mut Document,
    table_block_id: &StableId,
    alignment: Option<opendoc_core::TableAlignment>,
) -> TableEditResult {
    let Some(table) = find_block_mut(&mut document.blocks, table_block_id) else {
        return TableEditResult::MissingTable;
    };
    let BlockKind::Table { properties, .. } = &mut table.kind else {
        return TableEditResult::NonTableBlock;
    };
    properties.alignment = alignment;
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
    position: InsertPosition,
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
    // The column adding a cell to one row creates is named after that cell,
    // the same rule `repair_table_geometry` uses for a column it has to
    // invent, so the two cannot disagree about which column a cell is in.
    let column = TableColumn::for_cell(&cell.id);
    // The anchor is a *cell*, so its position is the column index the cell
    // sits at. Only an anchored insert can degrade; `First` and `Last` name no
    // cell that could have been deleted, which is what lets an undo put a
    // deleted first cell back at the front.
    let anchor_index = position
        .anchor()
        .map(|target| column_index_of_cell(rows, target));
    let result = match anchor_index {
        Some(None) => TableEditResult::AnchorDegraded,
        _ => TableEditResult::Applied,
    };
    let insert_at = position.index(columns.len(), anchor_index.flatten());
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
