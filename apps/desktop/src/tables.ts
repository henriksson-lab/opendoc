// Tables in the document surface.
//
// Where the caret is inside a table is read off the rendered grid; what a table
// command means is Rust's. Every case below hands the core identities the
// renderer stamped on the DOM and computes no document fact of its own.
import { promptDialog } from "./ui";
import type { AppBlock } from "./types";
import { APP_INSERT_FIRST, APP_TWIPS_PER_POINT } from "./generated/document";
import { edit, findBlock, showError } from "./shared";
import { state } from "./state";

/**
 * Where the caret is inside a table.
 *
 * The renderer stamps every cell with its own id, its column's id and its
 * row's id, and skips the cells hidden under a merge — so walking up from the
 * caret names *which* cell the caret is in. Everything about the table's shape
 * — how many rows and columns it has, where this one sits in them, which
 * sibling comes before it — is then read out of the model projection, because
 * the grid the browser drew is a rendering of that shape and not a second
 * statement of it. A merge, a covered cell or a column the renderer collapsed
 * would each make the two disagree.
 */
type TableContext = {
  tableBlockId: string;
  rowId: string;
  cellId: string;
  columnId: string;
  columnIndex: number;
  rowIndex: number;
  columnCount: number;
  rowCount: number;
  header: boolean;
  previousColumnId: string | null;
  previousRowId: string | null;
};

/**
 * The cell the last table command acted on.
 *
 * A document edit re-renders the editor, and the caret does not always come
 * back inside it — so two table commands in a row would find a cell the first
 * time and nothing the second. The remembered cell is only ever a *hint*: it
 * is looked up in the live grid again, so a cell that a column delete removed
 * is simply not there and the caret rule applies as normal.
 */
let lastTableCellId: string | null = null;

/**
 * Turns a click in an editor-only row/column band into the browser range the
 * existing rectangular table selection reads. The bands are CSS pseudo-content
 * and never enter document or export HTML; they only choose real cell corners.
 */
export function selectTableBand(event: MouseEvent): boolean {
  const target = event.target instanceof Element ? event.target : null;
  const table = target?.closest<HTMLTableElement>('table.doc-table[data-block-id]') ?? null;
  if (!table) return false;
  const bounds = table.getBoundingClientRect();
  const band = 16;
  const inColumnBand = event.clientY >= bounds.top && event.clientY < bounds.top + band;
  const inRowBand = event.clientX >= bounds.left && event.clientX < bounds.left + band;
  if (!inColumnBand && !inRowBand) return false;

  const cells = cellsOwnedByTable(table);
  const chosen = inColumnBand
    ? cells.find((cell) => {
        const rect = cell.getBoundingClientRect();
        return event.clientX >= rect.left && event.clientX < rect.right;
      })
    : cells.find((cell) => {
        const rect = cell.getBoundingClientRect();
        return event.clientY >= rect.top && event.clientY < rect.bottom;
      });
  if (!chosen) return false;
  const key = inColumnBand ? chosen.dataset.columnId : chosen.closest("tr")?.dataset.rowId;
  if (!key) return false;
  const matching = cells.filter((cell) =>
    inColumnBand ? cell.dataset.columnId === key : cell.closest("tr")?.dataset.rowId === key,
  );
  if (matching.length === 0) return false;
  selectCellRange(matching[0], matching[matching.length - 1]);
  lastTableCellId = matching[0].dataset.cellId ?? null;
  return true;
}

function selectCellRange(first: HTMLTableCellElement, last: HTMLTableCellElement): void {
  const start = firstDirectCellRun(first) ?? first;
  const end = firstDirectCellRun(last) ?? last;
  const range = document.createRange();
  range.selectNodeContents(start);
  range.collapse(true);
  range.setEndAfter(end);
  const selection = document.getSelection();
  selection?.removeAllRanges();
  selection?.addRange(range);
}

/**
 * Moves the caret to the cell before or after the one it is in, and reports
 * whether there was one.
 *
 * Cell *order* is the grid's own: the renderer emits one `<td>` per drawn
 * cell, in reading order, and skips the ones a merge covers — so walking the
 * `<td>`s is walking the cells a user can actually put a caret in, and no
 * second statement of the table's shape is needed here. `false` means the
 * caret was in the last cell (or in no table at all), and the caller lets the
 * key through so focus leaves the document rather than cycling for ever.
 *
 * The caret lands at the *start* of the target cell rather than selecting its
 * contents. Selecting them is what Docs does, but it makes the next keystroke
 * replace the cell, and nothing in this editor yet draws a cell as selected —
 * an invisible destructive selection is the wrong thing to hand a keyboard
 * user.
 */
export function moveCaretToAdjacentCell(forward: boolean): boolean {
  const node = document.getSelection()?.focusNode ?? null;
  const from = node instanceof Element ? node : (node?.parentElement ?? null);
  const cell = from?.closest<HTMLTableCellElement>('[contenteditable="true"] :is(td, th)[data-cell-id]') ?? null;
  const table = cell?.closest<HTMLTableElement>("table[data-block-id]") ?? null;
  // A structural projection can remove the focused cell while the browser is
  // reconciling its selection range. Never consume Tab by navigating that old
  // detached grid: its next sibling is no longer a cell in the document the
  // user sees. Let the normal focus-escape rule run instead.
  if (!cell || !table || !cell.isConnected || !table.isConnected) return false;
  const cells = cellsOwnedByTable(table);
  const target = cells[cells.indexOf(cell) + (forward ? 1 : -1)];
  if (!target) return false;
  const run = firstDirectCellRun(target) ?? target;
  const range = document.createRange();
  range.selectNodeContents(run);
  range.collapse(true);
  const selection = document.getSelection();
  selection?.removeAllRanges();
  selection?.addRange(range);
  lastTableCellId = target.dataset.cellId ?? null;
  return true;
}

/**
 * Adds a row after the last row when Tab leaves the final visible table cell.
 *
 * This is intentionally a separate path from `moveCaretToAdjacentCell`: a
 * false return there also means "not in a table", which must still let focus
 * leave the editor. The table context comes from the live rendered cell, then
 * Rust receives stable table/row ids and creates the rectangular row.
 */
export async function appendTableRowFromLastCell(): Promise<boolean> {
  const context = tableContext();
  if (!context) {
    return false;
  }
  const table = document.querySelector<HTMLTableElement>(
    `[contenteditable="true"] table[data-block-id="${CSS.escape(context.tableBlockId)}"]`,
  );
  const cells = table
    ? cellsOwnedByTable(table)
    : [];
  if (cells[cells.length - 1]?.dataset.cellId !== context.cellId) return false;
  await edit("add_table_row", { tableBlockId: context.tableBlockId, afterRow: context.rowId, text: "" });
  const refreshed = document.querySelector<HTMLTableElement>(
    `[contenteditable="true"] table[data-block-id="${CSS.escape(context.tableBlockId)}"]`,
  );
  // The newly appended row always has one visible cell per column.  Taking
  // the final row-sized owned suffix avoids `querySelector` descending into a
  // nested table in the prior outer cell and moving Tab into that unrelated
  // grid instead of the new outer row.
  const target = refreshed
    ? cellsOwnedByTable(refreshed).slice(-context.columnCount)[0] ?? null
    : null;
  if (!target) return true;
  const run = firstDirectCellRun(target) ?? target;
  const range = document.createRange();
  range.selectNodeContents(run);
  range.collapse(true);
  const selection = document.getSelection();
  selection?.removeAllRanges();
  selection?.addRange(range);
  lastTableCellId = target.dataset.cellId ?? null;
  return true;
}

function tableContext(): TableContext | null {
  const node = document.getSelection()?.focusNode ?? null;
  const from = node instanceof Element ? node : (node?.parentElement ?? null);
  const live = contextForCell(from?.closest<HTMLTableCellElement>(":is(td, th)[data-cell-id]") ?? null);
  if (live) {
    lastTableCellId = live.cellId;
    return live;
  }
  if (!lastTableCellId) return null;
  return contextForCell(
    document.querySelector<HTMLTableCellElement>(
      `[contenteditable="true"] :is(td, th)[data-cell-id="${CSS.escape(lastTableCellId)}"]`,
    ),
  );
}

function contextForCell(cell: HTMLTableCellElement | null): TableContext | null {
  const row = cell?.closest<HTMLTableRowElement>("tr[data-row-id]") ?? null;
  const table = cell?.closest<HTMLTableElement>("table[data-block-id]") ?? null;
  // See `moveCaretToAdjacentCell`: a selection can briefly retain the old
  // node after a remote table shape change. Toolbar actions must not derive
  // stable command ids from that detached rendering either.
  if (!cell || !row || !table || !cell.isConnected || !row.isConnected || !table.isConnected) return null;
  const tableBlockId = table.dataset.blockId ?? "";
  const rowId = row.dataset.rowId ?? "";
  const columnId = cell.dataset.columnId ?? "";
  const block = findBlock(tableBlockId);
  const columns = block?.table?.columns ?? [];
  const rowIds = block?.row_ids ?? [];
  const columnIndex = columns.findIndex((column) => column.id === columnId);
  const rowIndex = rowIds.indexOf(rowId);
  // A cell the model does not know about is a stale rendering, not a cell:
  // acting on it would hand Rust an id it would reject anyway.
  if (columnIndex < 0 || rowIndex < 0) return null;
  return {
    tableBlockId,
    rowId,
    cellId: cell.dataset.cellId ?? "",
    columnId,
    columnIndex,
    rowIndex,
    columnCount: columns.length,
    rowCount: rowIds.length,
    header: block?.table?.row_headers?.[rowIndex] ?? false,
    previousColumnId: columns[columnIndex - 1]?.id ?? null,
    previousRowId: rowIds[rowIndex - 1] ?? null,
  };
}

/** A rectangle of cells in model coordinates (inclusive on both ends). */
type CellRectangle = {
  fromRow: number;
  toRow: number;
  fromColumn: number;
  toColumn: number;
};

function cellElementFor(node: Node | null): HTMLTableCellElement | null {
  const from = node instanceof Element ? node : (node?.parentElement ?? null);
  return from?.closest<HTMLTableCellElement>(":is(td, th)[data-cell-id]") ?? null;
}

/**
 * A table cell can itself contain a table.  `querySelectorAll` descends into
 * that child grid, but a keyboard/band operation on the outer table must
 * enumerate only the cells the outer table owns.  The nearest table is the
 * renderer's nesting boundary and works for `thead`/`tbody` as well as direct
 * rows, without reconstructing the model grid in the browser.
 */
function cellsOwnedByTable(table: HTMLTableElement): HTMLTableCellElement[] {
  return Array.from(table.querySelectorAll<HTMLTableCellElement>(":is(td, th)[data-cell-id]"))
    .filter((cell) => cell.closest("table") === table);
}

/** The first editable inline owned by this cell, never one in a nested grid. */
function firstDirectCellRun(cell: HTMLTableCellElement): HTMLElement | null {
  return Array.from(cell.querySelectorAll<HTMLElement>("[data-inline-id]"))
    .find((inline) => inline.closest(":is(td, th)[data-cell-id]") === cell) ?? null;
}

/**
 * The rectangle of cells the user has selected.
 *
 * A drag across a table leaves the browser selection anchored in one cell and
 * focused in another; the two corners are whichever cells those are, and the
 * rectangle is their bounding box. The indices come from the model for the
 * same reason `contextForCell` takes them from there. A selection that is not
 * wholly inside this table degrades to the caret's own cell, which the caller
 * reads as "nothing was selected to merge".
 */
function selectedCellRectangle(context: TableContext): CellRectangle {
  const selection = document.getSelection();
  const anchor = contextForCell(cellElementFor(selection?.anchorNode ?? null));
  const focus = contextForCell(cellElementFor(selection?.focusNode ?? null));
  // Both corners have to be in *this* table; anything else is a selection
  // that reaches outside the grid, and a merge cannot mean anything there.
  const corners =
    anchor && focus && anchor.tableBlockId === context.tableBlockId && focus.tableBlockId === context.tableBlockId
      ? [anchor, focus]
      : [context, context];
  return {
    fromRow: Math.min(corners[0].rowIndex, corners[1].rowIndex),
    toRow: Math.max(corners[0].rowIndex, corners[1].rowIndex),
    fromColumn: Math.min(corners[0].columnIndex, corners[1].columnIndex),
    toColumn: Math.max(corners[0].columnIndex, corners[1].columnIndex),
  };
}

/**
 * Moves a focused block against a sibling in its model cell container.
 *
 * A list item is rendered below a `<ul>`, while a paragraph is a direct cell
 * child. DOM sibling traversal would therefore claim that the list item has
 * no neighbour even though they are ordinary adjacent blocks in the cell.
 * The projection already carries that authoritative block order, including
 * for nested tables, so use it to name the existing stable move command.
 */
async function moveFocusedCellBlock(direction: "up" | "down"): Promise<void> {
  const blockId = state.selection?.focus.block_id;
  if (!blockId) {
    showError("Place the caret in a cell block first.");
    return;
  }
  const context = tableContext();
  if (!context) {
    showError("Place the caret in a table cell block first.");
    return;
  }
  const cellBlocks = findBlock(context.tableBlockId)?.rows?.[context.rowIndex]?.[context.columnIndex];
  const index = cellBlocks?.findIndex((block) => block.id === blockId) ?? -1;
  const sibling = cellBlocks?.[index + (direction === "up" ? -1 : 1)];
  if (!sibling) {
    showError(`There is no cell block ${direction === "up" ? "above" : "below"} this one.`);
    return;
  }
  await edit("move_block", {
    blockId,
    anchorBlockId: sibling.id,
    placement: direction === "up" ? "before" : "after",
  });
}

/**
 * Moves the focused *direct* cell block to the cell before or after it in the
 * rendered grid.  This deliberately follows the same visible-cell order as
 * Tab: covered cells are not destinations a person can select, and nested
 * table cells do not leak into the outer grid.  The operation is still the
 * ordinary identity-preserving `move_block`; the browser supplies only the
 * destination cell's first stable sibling as its anchor.
 */
async function moveFocusedCellBlockToAdjacentCell(forward: boolean): Promise<void> {
  const blockId = state.selection?.focus.block_id;
  if (!blockId) {
    showError("Place the caret in a cell block first.");
    return;
  }
  const context = tableContext();
  if (!context) {
    showError("Place the caret in a table cell block first.");
    return;
  }
  const table = document.querySelector<HTMLTableElement>(
    `[contenteditable="true"] table[data-block-id="${CSS.escape(context.tableBlockId)}"]`,
  );
  const cells = table ? cellsOwnedByTable(table) : [];
  const sourceIndex = cells.findIndex((cell) => cell.dataset.cellId === context.cellId);
  const target = sourceIndex < 0 ? null : cells[sourceIndex + (forward ? 1 : -1)] ?? null;
  const destination = contextForCell(target);
  if (!destination || destination.tableBlockId !== context.tableBlockId) {
    showError(`There is no ${forward ? "next" : "previous"} visible table cell.`);
    return;
  }
  const sourceBlocks = findBlock(context.tableBlockId)?.rows?.[context.rowIndex]?.[context.columnIndex];
  if (!sourceBlocks?.some((block) => block.id === blockId)) {
    showError("Place the caret in a direct block of this table cell first.");
    return;
  }
  // The merge operation independently enforces this invariant for a racing
  // remote update.  The local check explains the ordinary one-block case
  // before dispatch rather than asking the user to infer it from a warning.
  if (sourceBlocks.length <= 1) {
    showError("Add another block before moving this cell's only block.");
    return;
  }
  const anchor = findBlock(context.tableBlockId)?.rows?.[destination.rowIndex]?.[destination.columnIndex]?.[0];
  if (!anchor) {
    showError("The destination cell changed. Place the caret in a live table cell and try again.");
    return;
  }
  await edit("move_block", { blockId, anchorBlockId: anchor.id, placement: "before" });
}

/** The cell id at a model position, or null when the grid has no such cell. */
function cellIdAt(block: AppBlock | null, rowIndex: number, columnIndex: number): string | null {
  return block?.cell_ids?.[rowIndex]?.[columnIndex] ?? null;
}

function rowHeightPoints(context: TableContext): string {
  const twips = findBlock(context.tableBlockId)?.table?.row_heights_twips?.[context.rowIndex];
  return twips === undefined || twips === null ? "" : String(twips / APP_TWIPS_PER_POINT);
}

function requireTableContext(): TableContext | null {
  const context = tableContext();
  if (!context) showError("Place the caret inside a table cell first.");
  return context;
}

/**
 * A modal can remain open while collaboration replaces the table projection.
 * The command identities captured when it opened are safe only if that exact
 * cell still resolves to the same table, row, and column when its answer is
 * submitted.  Do not use `tableContext` here: its remembered-cell fallback is
 * intentionally useful between adjacent commands, but must not turn a
 * deleted modal target into a new implicit target.
 */
function isLiveTableContext(context: TableContext): boolean {
  return isLiveTable(context) && isLiveCell(context);
}

/** A table-level dialog needs its table to survive, but not necessarily its original cell. */
function isLiveTable(context: TableContext): boolean {
  const table = document.querySelector<HTMLTableElement>(
    `[contenteditable="true"] table[data-block-id="${CSS.escape(context.tableBlockId)}"]`,
  );
  return table?.isConnected === true && findBlock(context.tableBlockId)?.table !== undefined;
}

/** A cell-level dialog must still name the exact cell, row, column, and table it opened on. */
function isLiveCell(context: TableContext): boolean {
  const cell = document.querySelector<HTMLTableCellElement>(
    `[contenteditable="true"] :is(td, th)[data-cell-id="${CSS.escape(context.cellId)}"]`,
  );
  const live = contextForCell(cell);
  return (
    live?.tableBlockId === context.tableBlockId &&
    live.rowId === context.rowId &&
    live.columnId === context.columnId &&
    live.cellId === context.cellId
  );
}

function requireLiveTableTarget(context: TableContext): boolean {
  if (isLiveTable(context)) return true;
  showError("The selected table changed. Place the caret in a live table cell and try again.");
  return false;
}

function requireLiveCellTarget(context: TableContext): boolean {
  if (isLiveCell(context)) return true;
  showError("The selected table cell changed. Place the caret in a live table cell and try again.");
  return false;
}

/**
 * Every table command, dispatched from one place so the menu stays a list of
 * names. Each one reads the caret's cell and hands Rust identities; none of
 * them computes a document fact here.
 */
export async function runTableAction(action: string): Promise<void> {
  const context = requireTableContext();
  if (!context) return;
  const { tableBlockId, rowId, cellId, columnId } = context;
  switch (action) {
    case "table-insert-column-right":
      await edit("insert_table_column", { tableBlockId, afterColumnId: columnId });
      return;
    case "table-insert-column-left":
      // "Before the first column" is the one position an anchor id cannot
      // name, so the contract has a keyword for it (`APP_INSERT_FIRST`);
      // omitting the anchor still means *append*.
      await edit("insert_table_column", {
        tableBlockId,
        afterColumnId: context.previousColumnId ?? APP_INSERT_FIRST,
      });
      return;
    case "table-delete-column":
      await edit("delete_table_column", { tableBlockId, columnId });
      return;
    case "table-insert-row-below":
      await edit("add_table_row", { tableBlockId, afterRow: rowId, text: "" });
      return;
    case "table-insert-row-above":
      await edit("add_table_row", {
        tableBlockId,
        afterRow: context.previousRowId ?? APP_INSERT_FIRST,
        text: "",
      });
      return;
    case "table-delete-row":
      await edit("delete_table_row", { tableBlockId, rowId });
      return;
    case "table-column-width": {
      const current = columnWidthPoints(context);
      const result = await promptDialog({
        title: "Column width",
        fields: [{ name: "points", label: "Width (points)", type: "number", step: "1", value: current }],
        submit: "Set",
      });
      if (!result) return;
      const points = Number(result.points);
      if (!Number.isFinite(points) || points <= 0) {
        showError("Column width must be a positive number of points.");
        return;
      }
      if (!isLiveTableContext(context)) {
        showError("The selected table column changed. Place the caret in a live table cell and try again.");
        return;
      }
      await edit("set_table_column_width", { tableBlockId, columnId, twips: Math.round(points * APP_TWIPS_PER_POINT) });
      return;
    }
    case "table-column-width-auto":
      await edit("clear_table_column_width", { tableBlockId, columnId });
      return;
    case "table-row-height": {
      const current = rowHeightPoints(context);
      const result = await promptDialog({
        title: "Minimum row height",
        fields: [{ name: "points", label: "Height (points)", type: "number", step: "1", value: current }],
        submit: "Set",
      });
      if (!result) return;
      const points = Number(result.points);
      if (!Number.isFinite(points) || points <= 0) {
        showError("Row height must be a positive number of points.");
        return;
      }
      if (!isLiveTableContext(context)) {
        showError("The selected table row changed. Place the caret in a live table cell and try again.");
        return;
      }
      await edit("set_table_row_height", { tableBlockId, rowId, twips: Math.round(points * APP_TWIPS_PER_POINT) });
      return;
    }
    case "table-row-height-auto":
      await edit("clear_table_row_height", { tableBlockId, rowId });
      return;
    case "table-toggle-header":
      await edit("set_table_row_header", { tableBlockId, rowId, header: !context.header });
      return;
    case "table-sort-ascending":
      await edit("sort_table_rows", { tableBlockId, columnId, descending: false });
      return;
    case "table-sort-descending":
      await edit("sort_table_rows", { tableBlockId, columnId, descending: true });
      return;
    case "table-border": {
      const result = await promptDialog({
        title: "Table border",
        fields: [
          { name: "style", label: "Style", type: "select", value: "solid", options: BORDER_STYLE_OPTIONS },
          { name: "points", label: "Thickness (points)", type: "number", step: "0.5", value: "1" },
          { name: "color", label: "Colour", type: "color", value: "#000000" },
        ],
        submit: "Apply",
      });
      if (!result) return;
      const points = Number(result.points);
      if (!Number.isFinite(points) || points < 0) {
        showError("Border thickness must be zero or more points.");
        return;
      }
      if (!requireLiveTableTarget(context)) return;
      await edit("set_table_border", {
        tableBlockId,
        style: result.style,
        twips: Math.round(points * APP_TWIPS_PER_POINT),
        color: result.color,
      });
      return;
    }
    case "table-border-inherit":
      await edit("clear_table_border", { tableBlockId });
      return;
    case "table-align-start":
      await edit("set_table_alignment", { tableBlockId, alignment: "start" });
      return;
    case "table-align-center":
      await edit("set_table_alignment", { tableBlockId, alignment: "center" });
      return;
    case "table-align-end":
      await edit("set_table_alignment", { tableBlockId, alignment: "end" });
      return;
    case "table-align-inherit":
      await edit("clear_table_alignment", { tableBlockId });
      return;
    case "table-merge-cells": {
      // The span is the selection, not a number typed into a dialog: what the
      // user dragged across is what gets merged, and the top-left cell of that
      // rectangle is the one the merge is anchored to.
      const rectangle = selectedCellRectangle(context);
      const rowSpan = rectangle.toRow - rectangle.fromRow + 1;
      const columnSpan = rectangle.toColumn - rectangle.fromColumn + 1;
      if (rowSpan === 1 && columnSpan === 1) {
        showError("Select the cells to merge first — drag across two or more.");
        return;
      }
      const anchorCellId = cellIdAt(findBlock(tableBlockId), rectangle.fromRow, rectangle.fromColumn);
      if (!anchorCellId) {
        showError("That selection does not cover a whole rectangle of cells.");
        return;
      }
      await edit("merge_table_cells", { cellId: anchorCellId, rowSpan, columnSpan });
      return;
    }
    case "table-split-cell":
      await edit("split_table_cell", { cellId });
      return;
    case "table-move-cell-block-up":
      await moveFocusedCellBlock("up");
      return;
    case "table-move-cell-block-down":
      await moveFocusedCellBlock("down");
      return;
    case "table-move-cell-block-previous-cell":
      await moveFocusedCellBlockToAdjacentCell(false);
      return;
    case "table-move-cell-block-next-cell":
      await moveFocusedCellBlockToAdjacentCell(true);
      return;
    case "table-cell-background": {
      const result = await promptDialog({
        title: "Cell background",
        fields: [{ name: "color", label: "Colour", type: "color", value: "#fff2cc" }],
        submit: "Apply",
      });
      if (!result) return;
      if (!requireLiveCellTarget(context)) return;
      await edit("set_table_cell_background", { cellId, color: result.color });
      return;
    }
    case "table-cell-border": {
      const result = await promptDialog({
        title: "Cell border",
        fields: [
          { name: "edge", label: "Edge", type: "select", value: "top", options: CELL_EDGE_OPTIONS },
          { name: "style", label: "Style", type: "select", value: "solid", options: BORDER_STYLE_OPTIONS },
          { name: "points", label: "Thickness (points)", type: "number", step: "0.5", value: "1" },
          { name: "color", label: "Colour", type: "color", value: "#000000" },
        ],
        submit: "Apply",
      });
      if (!result) return;
      const points = Number(result.points);
      if (!Number.isFinite(points) || points < 0) {
        showError("Border thickness must be zero or more points.");
        return;
      }
      if (!requireLiveCellTarget(context)) return;
      await edit("set_table_cell_border", {
        cellId,
        edge: result.edge,
        style: result.style,
        twips: Math.round(points * APP_TWIPS_PER_POINT),
        color: result.color,
      });
      return;
    }
    case "table-cell-vertical-align": {
      const result = await promptDialog({
        title: "Cell vertical alignment",
        fields: [{ name: "alignment", label: "Alignment", type: "select", value: "middle", options: VERTICAL_ALIGN_OPTIONS }],
        submit: "Apply",
      });
      if (!result) return;
      if (!requireLiveCellTarget(context)) return;
      await edit("set_table_cell_vertical_alignment", { cellId, alignment: result.alignment });
      return;
    }
    case "table-cell-row-header": {
      const result = await promptDialog({
        title: "Row header",
        fields: [{ name: "rowHeader", label: "Treat this cell as a row header", type: "select", value: "true", options: [{ value: "true", label: "Yes" }, { value: "false", label: "No" }] }],
        submit: "Apply",
      });
      if (!result) return;
      if (!requireLiveCellTarget(context)) return;
      await edit("set_table_cell_row_header", { cellId, rowHeader: result.rowHeader === "true" });
      return;
    }
    case "table-cell-padding": {
      const result = await promptDialog({
        title: "Cell padding",
        fields: [
          { name: "edge", label: "Edge", type: "select", value: "top", options: CELL_EDGE_OPTIONS },
          { name: "points", label: "Padding (points)", type: "number", step: "1", value: "4" },
        ],
        submit: "Apply",
      });
      if (!result) return;
      const points = Number(result.points);
      if (!Number.isFinite(points) || points < 0) {
        showError("Cell padding must be zero or more points.");
        return;
      }
      if (!requireLiveCellTarget(context)) return;
      await edit("set_table_cell_padding", { cellId, edge: result.edge, twips: Math.round(points * APP_TWIPS_PER_POINT) });
      return;
    }
    case "table-cell-clear-style": {
      const result = await promptDialog({
        title: "Clear cell style",
        fields: [{ name: "key", label: "Property", type: "select", value: "background", options: CELL_PROPERTY_OPTIONS }],
        submit: "Clear",
      });
      if (!result) return;
      if (!requireLiveCellTarget(context)) return;
      await edit("clear_table_cell_property", { cellId, key: result.key });
      return;
    }
    default:
      return;
  }
}

const CELL_EDGE_OPTIONS = [
  { value: "top", label: "Top" },
  { value: "bottom", label: "Bottom" },
  { value: "start", label: "Leading (left in LTR)" },
  { value: "end", label: "Trailing (right in LTR)" },
];

const BORDER_STYLE_OPTIONS = [
  { value: "solid", label: "Solid" },
  { value: "dashed", label: "Dashed" },
  { value: "dotted", label: "Dotted" },
  { value: "double", label: "Double" },
  { value: "none", label: "None" },
];

const VERTICAL_ALIGN_OPTIONS = [
  { value: "top", label: "Top" },
  { value: "middle", label: "Middle" },
  { value: "bottom", label: "Bottom" },
];

const CELL_PROPERTY_OPTIONS = [
  { value: "background", label: "Background" },
  { value: "border-top", label: "Top border" },
  { value: "border-bottom", label: "Bottom border" },
  { value: "border-start", label: "Leading border" },
  { value: "border-end", label: "Trailing border" },
  { value: "vertical-alignment", label: "Vertical alignment" },
  { value: "padding-top", label: "Top padding" },
  { value: "padding-bottom", label: "Bottom padding" },
  { value: "padding-start", label: "Leading padding" },
  { value: "padding-end", label: "Trailing padding" },
];

/** The column's stored width in points, or the empty string when it is auto. */
function columnWidthPoints(context: TableContext): string {
  const block = findBlock(context.tableBlockId);
  const column = block?.table?.columns[context.columnIndex];
  const twips = column?.width_twips ?? null;
  return twips == null ? "" : String(Math.round((twips / APP_TWIPS_PER_POINT) * 100) / 100);
}
