// Tables in the document surface.
//
// Where the caret is inside a table is read off the rendered grid; what a table
// command means is Rust's. Every case below hands the core identities the
// renderer stamped on the DOM and computes no document fact of its own.
import { promptDialog } from "./ui";
import { edit, findBlock, showError } from "./shared";

/**
 * Where the caret is inside a table, read off the rendered grid.
 *
 * The renderer stamps every cell with its own id, its column's id and its
 * row's id, and skips the cells hidden under a merge — so walking up from the
 * caret gives the identities the table commands need without the view working
 * any geometry out for itself. Indices come from the DOM position, which is
 * the same order the model stores.
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

function tableContext(): TableContext | null {
  const node = document.getSelection()?.focusNode ?? null;
  const from = node instanceof Element ? node : (node?.parentElement ?? null);
  const live = contextForCell(from?.closest<HTMLTableCellElement>("td[data-cell-id]") ?? null);
  if (live) {
    lastTableCellId = live.cellId;
    return live;
  }
  if (!lastTableCellId) return null;
  return contextForCell(
    document.querySelector<HTMLTableCellElement>(
      `[contenteditable="true"] td[data-cell-id="${CSS.escape(lastTableCellId)}"]`,
    ),
  );
}

function contextForCell(cell: HTMLTableCellElement | null): TableContext | null {
  const row = cell?.closest<HTMLTableRowElement>("tr[data-row-id]") ?? null;
  const table = cell?.closest<HTMLTableElement>("table[data-block-id]") ?? null;
  if (!cell || !row || !table) return null;
  const columns = Array.from(table.querySelectorAll<HTMLElement>("colgroup > col"));
  const rows = Array.from(table.querySelectorAll<HTMLTableRowElement>("tbody > tr"));
  const columnId = cell.dataset.columnId ?? "";
  const columnIndex = columns.findIndex((column) => column.dataset.columnId === columnId);
  const rowIndex = rows.indexOf(row);
  return {
    tableBlockId: table.dataset.blockId ?? "",
    rowId: row.dataset.rowId ?? "",
    cellId: cell.dataset.cellId ?? "",
    columnId,
    columnIndex,
    rowIndex,
    columnCount: columns.length,
    rowCount: rows.length,
    previousColumnId: columnIndex > 0 ? (columns[columnIndex - 1]?.dataset.columnId ?? null) : null,
    previousRowId: rowIndex > 0 ? (rows[rowIndex - 1]?.dataset.rowId ?? null) : null,
  };
}

function requireTableContext(): TableContext | null {
  const context = tableContext();
  if (!context) showError("Place the caret inside a table cell first.");
  return context;
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
      if (!context.previousColumnId) {
        showError("OpenDoc cannot insert before the first column yet — insert to the right of it instead.");
        return;
      }
      await edit("insert_table_column", { tableBlockId, afterColumnId: context.previousColumnId });
      return;
    case "table-delete-column":
      await edit("delete_table_column", { tableBlockId, columnId });
      return;
    case "table-insert-row-below":
      await edit("add_table_row", { tableBlockId, afterRow: rowId, text: "" });
      return;
    case "table-insert-row-above":
      if (!context.previousRowId) {
        showError("OpenDoc cannot insert above the first row yet — insert below it instead.");
        return;
      }
      await edit("add_table_row", { tableBlockId, afterRow: context.previousRowId, text: "" });
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
      await edit("set_table_column_width", { tableBlockId, columnId, twips: Math.round(points * 20) });
      return;
    }
    case "table-column-width-auto":
      await edit("clear_table_column_width", { tableBlockId, columnId });
      return;
    case "table-merge-cells": {
      const result = await promptDialog({
        title: "Merge cells",
        fields: [
          { name: "rows", label: "Rows to cover", type: "number", step: "1", value: "1" },
          { name: "columns", label: "Columns to cover", type: "number", step: "1", value: "2" },
        ],
        submit: "Merge",
      });
      if (!result) return;
      const rowSpan = Math.max(1, Math.round(Number(result.rows) || 1));
      const columnSpan = Math.max(1, Math.round(Number(result.columns) || 1));
      if (rowSpan === 1 && columnSpan === 1) {
        showError("A merge has to cover more than one cell.");
        return;
      }
      await edit("merge_table_cells", { cellId, rowSpan, columnSpan });
      return;
    }
    case "table-split-cell":
      await edit("split_table_cell", { cellId });
      return;
    case "table-cell-background": {
      const result = await promptDialog({
        title: "Cell background",
        fields: [{ name: "color", label: "Colour", type: "color", value: "#fff2cc" }],
        submit: "Apply",
      });
      if (!result) return;
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
      await edit("set_table_cell_border", {
        cellId,
        edge: result.edge,
        style: result.style,
        twips: Math.round(points * 20),
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
      await edit("set_table_cell_vertical_alignment", { cellId, alignment: result.alignment });
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
      await edit("set_table_cell_padding", { cellId, edge: result.edge, twips: Math.round(points * 20) });
      return;
    }
    case "table-cell-clear-style": {
      const result = await promptDialog({
        title: "Clear cell style",
        fields: [{ name: "key", label: "Property", type: "select", value: "background", options: CELL_PROPERTY_OPTIONS }],
        submit: "Clear",
      });
      if (!result) return;
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
  return twips == null ? "" : String(Math.round((twips / 20) * 100) / 100);
}
