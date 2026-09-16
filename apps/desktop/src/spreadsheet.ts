// The spreadsheet surface: the grid, the formula bar, the selection, the fill
// handle and row/column resizing.
//
// The grid's HTML comes from Rust (`render_workbook_html`) and is applied with
// `morphChildren`, so nothing here may depend on DOM state the renderer does
// not own — there are no handle elements, only hit zones derived from the
// pointer position. Which cells a selection covers, what a drag would fill and
// what a paste means are all decided by commands; this module maps pointers and
// keys onto them.
//
// Everything about *where* the cursor is lives in this module and nowhere else:
// the toolbar asks `focusedCell()`, and the dispatcher routes the Data menu
// through `runSpreadsheetAction`.
import { morphChildren } from "./editor";
import { escapeHtml, promptDialog, toast } from "./ui";
import { dispatch, fetchUrlFile, invoke, isTauri, openFile } from "./invoke";
import type { AppSpreadsheetSelection } from "./types";
import {
  APP_DEFAULT_COLUMN_WIDTH_PX,
  APP_DEFAULT_ROW_HEIGHT_PX,
  APP_MAX_AXIS_SIZE_PX,
  APP_MIN_AXIS_SIZE_PX,
} from "./generated/spreadsheet";
import { state } from "./state";
import { edit, native, query, run, showError, textFromBase64 } from "./shared";
import { renderToolbar } from "./toolbar";
import { downloadExport, fetchPublicGoogleExport, googlePublicExportUrl } from "./files";
import { runAction } from "./actions";

let workbookHtml = "";
let sheetId: string | null = null;
let cellAnchor = "A1";
let cellFocus = "A1";
let spreadsheetSelection: AppSpreadsheetSelection | null = null;
let cellEditing: { address: string; draft: string } | null = null;

export function currentSheet() {
  const sheets = state.doc?.workbook.sheets ?? [];
  return sheets.find((sheet) => sheet.id === sheetId) ?? sheets[0] ?? null;
}

export async function renderSheets(main: HTMLElement): Promise<void> {
  const doc = state.doc;
  if (!doc) return;
  const sheet = currentSheet();
  if (!sheet) {
    main.innerHTML = `<p class="empty">No sheets.</p>`;
    return;
  }
  sheetId = sheet.id;
  try {
    const result = await dispatch("render_workbook_html", { sheetId: sheet.id });
    workbookHtml = result.kind === "Text" ? result.value : "";
  } catch (error) {
    showError(error instanceof Error ? error.message : String(error));
    return;
  }
  let grid = query("[data-workbook]", main);
  if (!grid) {
    main.innerHTML = `
      <div class="workbook" data-workbook>
        <div class="formula-bar"><input class="name-box" data-name-box aria-label="Name box" value="${escapeHtml(cellFocus)}"><span class="fx">fx</span><input class="formula-input" data-formula-input aria-label="Formula"></div>
        <div class="grid-scroll" data-grid tabindex="0" role="region" aria-describedby="spreadsheet-grid-status" aria-keyshortcuts="ArrowUp ArrowDown ArrowLeft ArrowRight Enter F2 Tab Shift+Tab"></div>
        <div class="sheet-tabs" data-sheet-tabs></div>
      </div>`;
    grid = query("[data-workbook]", main) as HTMLElement;
    bindSheetEvents(grid);
  }
  const gridHost = query("[data-grid]", grid) as HTMLElement;
  gridHost.setAttribute("aria-label", `Spreadsheet grid: ${sheet.title}`);
  const template = document.createElement("template");
  template.innerHTML = workbookHtml;
  morphChildren(gridHost, template.content);
  positionSheetImages(gridHost);
  const tabs = query("[data-sheet-tabs]", grid) as HTMLElement;
  tabs.innerHTML = `${doc.workbook.sheets
    .map((item) => `<button type="button" class="sheet-tab${item.id === sheet.id ? " active" : ""}" data-action="select-sheet" data-id="${escapeHtml(item.id)}" data-title="${escapeHtml(item.title)}">${escapeHtml(item.title)}</button>`)
    .join("")}<button type="button" class="sheet-tab add" data-action="add-sheet" title="Add sheet">＋</button>`;
  await refreshSpreadsheetSelection();
  updateCellSelection();
  const formula = query<HTMLInputElement>("[data-formula-input]", grid);
  const cell = sheet.cells.find((item) => item.address === cellFocus);
  if (formula && document.activeElement !== formula) formula.value = cellEditing ? cellEditing.draft : (cell?.user_value ?? "");
  const nameBox = query<HTMLInputElement>("[data-name-box]", grid);
  if (nameBox && document.activeElement !== nameBox) nameBox.value = rangeLabel();
  renderCellEditor(gridHost);
}

/** Positions raster drawings from their durable two-cell anchors.  Rust owns
 * the source geometry; the browser owns only the measured column/row pixels,
 * so zoom and user CSS cannot make a second, drifting layout model. */
function positionSheetImages(host: HTMLElement): void {
  const table = host.querySelector<HTMLElement>(".sheet-grid");
  if (!table) return;
  const hostRect = host.getBoundingClientRect();
  for (const image of host.querySelectorAll<HTMLImageElement>(".sheet-image")) {
    if (!image.getAttribute("src")) {
      image.hidden = true;
      continue;
    }
    const startColumn = image.dataset.startColumn;
    const startRow = image.dataset.startRow;
    const endColumn = image.dataset.endColumn;
    const endRow = image.dataset.endRow;
    const start = startColumn && startRow
      ? table.querySelector<HTMLElement>(`[data-address="${CSS.escape(startColumn + startRow)}"]`)
      : null;
    const end = endColumn && endRow
      ? table.querySelector<HTMLElement>(`[data-address="${CSS.escape(endColumn + endRow)}"]`)
      : null;
    if (!start || !end) {
      image.hidden = true;
      continue;
    }
    const startRect = start.getBoundingClientRect();
    const endRect = end.getBoundingClientRect();
    const sx = Number(image.dataset.startOffsetX ?? "0");
    const sy = Number(image.dataset.startOffsetY ?? "0");
    const ex = Number(image.dataset.endOffsetX ?? "0");
    const ey = Number(image.dataset.endOffsetY ?? "0");
    const left = startRect.left - hostRect.left + host.scrollLeft + sx;
    const top = startRect.top - hostRect.top + host.scrollTop + sy;
    const width = endRect.left - startRect.left + ex - sx;
    const height = endRect.top - startRect.top + ey - sy;
    if (!Number.isFinite(left) || !Number.isFinite(top) || width <= 0 || height <= 0) {
      image.hidden = true;
      continue;
    }
    image.hidden = false;
    image.style.left = `${left}px`;
    image.style.top = `${top}px`;
    image.style.width = `${width}px`;
    image.style.height = `${height}px`;
  }
}

function rangeLabel(): string {
  if (spreadsheetSelection?.anchor === cellAnchor && spreadsheetSelection.focus === cellFocus) return spreadsheetSelection.range;
  return cellFocus;
}

async function refreshSpreadsheetSelection(): Promise<void> {
  if (!sheetId) {
    spreadsheetSelection = null;
    return;
  }
  try {
    spreadsheetSelection = await invoke("describe_spreadsheet_selection", { sheetId, anchor: cellAnchor, focus: cellFocus });
    cellAnchor = spreadsheetSelection.anchor;
    cellFocus = spreadsheetSelection.focus;
  } catch (error) {
    spreadsheetSelection = null;
    showError(error instanceof Error ? error.message : String(error));
  }
}

function updateCellSelection(): void {
  const grid = query("[data-grid]");
  if (!grid) return;
  const selected = new Set(spreadsheetSelection?.anchor === cellAnchor && spreadsheetSelection.focus === cellFocus ? spreadsheetSelection.selected_addresses : [cellFocus]);
  // The fill handle is drawn by CSS on the selection's bottom-right cell;
  // nothing in this file inserts an element the renderer would morph away.
  const fillAnchor = spreadsheetSelection?.to_address ?? cellFocus;
  grid.querySelectorAll<HTMLElement>("[data-address]").forEach((node) => {
    node.classList.toggle("selected", selected.has(node.dataset.address ?? ""));
    node.classList.toggle("focus", node.dataset.address === cellFocus);
    node.classList.toggle("fill-anchor", node.dataset.address === fillAnchor);
  });
  const summary = query("[data-cell-summary]");
  if (summary) summary.textContent = spreadsheetSelection?.summary_label ?? rangeLabel();
  const focusCell = grid.querySelector<HTMLElement>(`[data-address="${cellFocus}"]`);
  if (focusCell && typeof focusCell.scrollIntoView === "function") focusCell.scrollIntoView({ block: "nearest", inline: "nearest" });
}

function renderCellEditor(gridHost: HTMLElement): void {
  const existing = query<HTMLInputElement>("[data-cell-editor]", gridHost);
  if (!cellEditing) {
    existing?.remove();
    return;
  }
  const cell = gridHost.querySelector<HTMLElement>(`[data-address="${cellEditing.address}"]`);
  if (!cell) return;
  if (existing) return;
  const input = document.createElement("input");
  input.className = "cell-editor";
  input.setAttribute("data-cell-editor", "true");
  input.setAttribute("aria-label", `Edit cell ${cellEditing.address}`);
  input.addEventListener("keydown", (event) => void onCellEditorKey(event));
  input.addEventListener("input", () => {
    if (cellEditing) cellEditing.draft = input.value;
    const formula = query<HTMLInputElement>("[data-formula-input]");
    if (formula) formula.value = cellEditing?.draft ?? "";
  });
  cell.appendChild(input);
  input.value = cellEditing.draft;
  input.focus();
  input.setSelectionRange(input.value.length, input.value.length);
}

async function commitCellEdit(move: { col: number; row: number } | null): Promise<void> {
  const editing = cellEditing;
  cellEditing = null;
  if (editing && sheetId) {
    await edit("set_spreadsheet_cell_in_sheet", { sheetId, address: editing.address, value: editing.draft });
  }
  if (move) {
    const direction = move.col < 0 ? "left" : move.col > 0 ? "right" : move.row < 0 ? "up" : "down";
    await moveFocus(direction, false);
  }
  query<HTMLElement>("[data-grid]")?.focus();
  await renderSheets(query("[data-main]") as HTMLElement);
}

async function onCellEditorKey(event: KeyboardEvent): Promise<void> {
  if (event.key === "Enter") {
    event.preventDefault();
    await commitCellEdit(event.shiftKey ? { col: 0, row: -1 } : { col: 0, row: 1 });
  } else if (event.key === "Tab") {
    event.preventDefault();
    await commitCellEdit(event.shiftKey ? { col: -1, row: 0 } : { col: 1, row: 0 });
  } else if (event.key === "Escape") {
    event.preventDefault();
    cellEditing = null;
    await renderSheets(query("[data-main]") as HTMLElement);
    query<HTMLElement>("[data-grid]")?.focus();
  }
}

async function applySpreadsheetSelectionAction(action: string, value: string, extend: boolean): Promise<void> {
  if (!sheetId) return;
  spreadsheetSelection = await invoke("reduce_spreadsheet_selection", { sheetId, anchor: cellAnchor, focus: cellFocus, action, value, extend });
  cellAnchor = spreadsheetSelection.anchor;
  cellFocus = spreadsheetSelection.focus;
  updateCellSelection();
  renderToolbar();
  const formula = query<HTMLInputElement>("[data-formula-input]");
  const cell = currentSheet()?.cells.find((item) => item.address === cellFocus);
  if (formula && document.activeElement !== formula) formula.value = cell?.user_value ?? "";
  const nameBox = query<HTMLInputElement>("[data-name-box]");
  if (nameBox) nameBox.value = rangeLabel();
}

async function moveFocus(direction: string, extend: boolean, edge = false): Promise<void> {
  await applySpreadsheetSelectionAction(edge ? "move-edge" : "move", direction, extend);
}

function bindSheetEvents(root: HTMLElement): void {
  const grid = query("[data-grid]", root) as HTMLElement;
  grid.addEventListener("mousedown", (event) => {
    const resizing = axisTargetAt(event);
    if (resizing) {
      startAxisResize(event, resizing);
      return;
    }
    if (startFillDrag(event)) return;
    const target = (event.target as Element).closest<HTMLElement>("[data-address]");
    if (!target?.dataset.address) return;
    if (cellEditing && target.dataset.address !== cellEditing.address) {
      void commitCellEdit(null);
    }
    void applySpreadsheetSelectionAction("set-focus", target.dataset.address, event.shiftKey);
    const dragging = (move: MouseEvent) => {
      const over = (move.target as Element | null)?.closest<HTMLElement>("[data-address]");
      if (over?.dataset.address && over.dataset.address !== cellFocus) {
        void applySpreadsheetSelectionAction("set-focus", over.dataset.address, true);
      }
    };
    const stop = () => {
      grid.removeEventListener("mousemove", dragging);
      window.removeEventListener("mouseup", stop);
    };
    grid.addEventListener("mousemove", dragging);
    window.addEventListener("mouseup", stop);
  });
  grid.addEventListener("dblclick", (event) => {
    const resizing = axisTargetAt(event);
    if (resizing) {
      // Conventional "reset to default": 0 clears the stored size.
      event.preventDefault();
      void applyAxisSize(resizing.axis, resizing.label, 0);
      return;
    }
    const target = (event.target as Element).closest<HTMLElement>("[data-address]");
    if (!target?.dataset.address) return;
    startCellEdit(target.dataset.address, null);
  });
  // Assignment, not addEventListener: re-binding a grid can only ever replace
  // this handler, never stack a second one.
  grid.onmousemove = (event) => {
    if (axisResize) return;
    const target = axisTargetAt(event);
    grid.classList.toggle("resize-column", target?.axis === "column");
    grid.classList.toggle("resize-row", target?.axis === "row");
  };
  grid.onmouseleave = () => {
    if (!axisResize) grid.classList.remove("resize-column", "resize-row");
  };
  grid.addEventListener("keydown", (event) => void onGridKey(event));
  grid.addEventListener("paste", (event) => {
    event.preventDefault();
    const text = event.clipboardData?.getData("text/plain") ?? "";
    void pasteCells(text);
  });
  grid.addEventListener("copy", (event) => {
    event.preventDefault();
    void writeClipboardCells(event);
  });
  grid.addEventListener("cut", (event) => {
    event.preventDefault();
    void writeClipboardCells(event).then(() => clearSelectedCells());
  });
  const formula = query<HTMLInputElement>("[data-formula-input]", root);
  formula?.addEventListener("keydown", (event) => {
    if (event.key === "Enter") {
      event.preventDefault();
      cellEditing = { address: cellFocus, draft: formula.value };
      void commitCellEdit({ col: 0, row: 1 });
    }
    if (event.key === "Escape") {
      formula.blur();
      grid.focus();
    }
  });
  formula?.addEventListener("input", () => {
    if (!cellEditing) cellEditing = { address: cellFocus, draft: formula.value };
    else cellEditing.draft = formula.value;
  });
  const nameBox = query<HTMLInputElement>("[data-name-box]", root);
  nameBox?.addEventListener("keydown", (event) => {
    if (event.key !== "Enter") return;
    event.preventDefault();
    void applySpreadsheetSelectionAction("set-range", nameBox.value, false).then(() => grid.focus());
  });
}

function startCellEdit(address: string, seed: string | null): void {
  const cell = currentSheet()?.cells.find((item) => item.address === address);
  cellEditing = { address, draft: seed ?? cell?.user_value ?? "" };
  cellFocus = address;
  cellAnchor = address;
  spreadsheetSelection = null;
  updateCellSelection();
  renderCellEditor(query("[data-grid]") as HTMLElement);
}

async function onGridKey(event: KeyboardEvent): Promise<void> {
  if (cellEditing) return;
  const ctrl = event.ctrlKey || event.metaKey;
  const moveDirection = { ArrowUp: "up", ArrowDown: "down", ArrowLeft: "left", ArrowRight: "right" }[event.key];
  if (moveDirection) {
    event.preventDefault();
    await moveFocus(moveDirection, event.shiftKey, ctrl);
    return;
  }
  if (event.key === "Enter" || event.key === "F2") {
    event.preventDefault();
    startCellEdit(cellFocus, null);
    return;
  }
  if (event.key === "Tab") {
    event.preventDefault();
    await moveFocus(event.shiftKey ? "left" : "right", false);
    return;
  }
  if (event.key === "Delete" || event.key === "Backspace") {
    event.preventDefault();
    await clearSelectedCells();
    return;
  }
  if (event.key === "Home") {
    event.preventDefault();
    await applySpreadsheetSelectionAction("home", ctrl ? "sheet" : "row", event.shiftKey);
    return;
  }
  if (ctrl && event.key.toLowerCase() === "a") {
    event.preventDefault();
    await applySpreadsheetSelectionAction("select-all", "", false);
    return;
  }
  const ctrlActions: Record<string, string> = { b: "cell-format:bold", i: "cell-format:italic", z: event.shiftKey ? "redo" : "undo", y: "redo", f: "find", s: "save", d: "fill-down", r: "fill-right" };
  if (ctrl && ctrlActions[event.key.toLowerCase()]) {
    event.preventDefault();
    await runAction(ctrlActions[event.key.toLowerCase()]);
    return;
  }
  if (!ctrl && !event.altKey && event.key.length === 1) {
    event.preventDefault();
    startCellEdit(cellFocus, event.key);
  }
}

function selectedCellsTsv(): string | null {
  if (spreadsheetSelection?.anchor === cellAnchor && spreadsheetSelection.focus === cellFocus) return spreadsheetSelection.selected_tsv;
  return null;
}

async function copySelectedCellsTsv(): Promise<string> {
  if (!sheetId) return "";
  return await invoke("copy_spreadsheet_selection_tsv", { sheetId, anchor: cellAnchor, focus: cellFocus });
}

/**
 * The block this workbook last copied. Pasting it back shifts the relative
 * references in its formulas; text from any other source is stored verbatim,
 * which is why the text itself is what identifies our own copy.
 */
let lastCopy: { origin: string; text: string } | null = null;

async function writeClipboardCells(event: ClipboardEvent): Promise<void> {
  const origin = spreadsheetSelection?.from_address ?? cellFocus;
  const ready = selectedCellsTsv();
  const text = ready ?? (await copySelectedCellsTsv());
  lastCopy = { origin, text };
  if (ready !== null) {
    event.clipboardData?.setData("text/plain", text);
    return;
  }
  try {
    await navigator.clipboard?.writeText(text);
  } catch (error) {
    showError(String(error));
  }
}

async function pasteCells(text: string): Promise<void> {
  if (!sheetId) return;
  const sourceOrigin = lastCopy?.text === text ? lastCopy.origin : null;
  await edit("paste_spreadsheet_tsv", { sheetId, origin: cellFocus, text, sourceOrigin });
  await renderSheets(query("[data-main]") as HTMLElement);
}

// ---- Fill handle (SH-28) -----------------------------------------------------
//
// There is no handle element: the grid is re-rendered from Rust-produced HTML
// through `morphChildren`, so anything added inside it here would be discarded.
// The handle is a `::after` on the cell that carries `.fill-anchor`, and the hit
// zone is derived from the pointer position, exactly as axis resizing does.
// Which values land in the filled cells is decided entirely in Rust.

const FILL_HANDLE_PX = 8;

type FillDrag = { sheetId: string; source: string; anchor: string; focus: string; target: string; pending: boolean };

let fillDrag: FillDrag | null = null;

/** Column labels covered by the selection, from the Rust-computed bounds. */
function columnsInSelection(selection: AppSpreadsheetSelection): string[] {
  return (currentSheet()?.columns ?? []).slice(selection.from_col - 1, selection.to_col);
}

function onFillHandle(event: MouseEvent): boolean {
  const cell = (event.target as Element | null)?.closest<HTMLElement>("td.fill-anchor");
  if (!cell) return false;
  const rect = cell.getBoundingClientRect();
  return event.clientX >= rect.right - FILL_HANDLE_PX && event.clientY >= rect.bottom - FILL_HANDLE_PX;
}

function startFillDrag(event: MouseEvent): boolean {
  const selection = spreadsheetSelection;
  if (!sheetId || !selection || cellEditing || !onFillHandle(event)) return false;
  event.preventDefault();
  fillDrag = { sheetId, source: selection.range, anchor: cellAnchor, focus: cellFocus, target: selection.range, pending: false };
  query("[data-grid]")?.classList.add("filling");
  // Added on mousedown and removed on mouseup, so they cannot stack.
  window.addEventListener("mousemove", onFillDragMove);
  window.addEventListener("mouseup", onFillDragEnd);
  window.addEventListener("keydown", onFillDragKey);
  return true;
}

/** Rust snaps the drag to one axis and reports the cells it would cover. */
function onFillDragMove(event: MouseEvent): void {
  const drag = fillDrag;
  if (!drag || drag.pending) return;
  const over = (event.target as Element | null)?.closest<HTMLElement>("[data-address]")?.dataset.address;
  if (!over) return;
  drag.pending = true;
  void invoke("reduce_spreadsheet_selection", { sheetId: drag.sheetId, anchor: drag.anchor, focus: drag.focus, action: "fill-target", value: over, extend: false })
    .then((summary) => {
      if (fillDrag !== drag) return;
      drag.target = summary.range;
      paintFillPreview(summary.selected_addresses);
    })
    .catch(() => undefined)
    .finally(() => {
      drag.pending = false;
    });
}

function onFillDragKey(event: KeyboardEvent): void {
  if (event.key === "Escape") finishFillDrag(true);
}

function onFillDragEnd(): void {
  finishFillDrag(false);
}

function paintFillPreview(addresses: string[]): void {
  const grid = query("[data-grid]");
  if (!grid) return;
  const covered = new Set(addresses);
  grid.querySelectorAll<HTMLElement>("[data-address]").forEach((node) => {
    node.classList.toggle("fill-preview", covered.has(node.dataset.address ?? ""));
  });
}

/** Ends a drag with exactly one command — never one per mousemove. */
function finishFillDrag(cancelled: boolean): void {
  const drag = fillDrag;
  fillDrag = null;
  window.removeEventListener("mousemove", onFillDragMove);
  window.removeEventListener("mouseup", onFillDragEnd);
  window.removeEventListener("keydown", onFillDragKey);
  query("[data-grid]")?.classList.remove("filling");
  paintFillPreview([]);
  if (!drag || cancelled || drag.target === drag.source) return;
  void applyFill(drag.sheetId, drag.source, drag.target);
}

async function applyFill(sheet: string, sourceRange: string, targetRange: string): Promise<void> {
  await edit("fill_spreadsheet_range", { sheetId: sheet, sourceRange, targetRange });
  await renderSheets(query("[data-main]") as HTMLElement);
}

/** Keyboard fill: copy the leading edge of the selection across the rest. */
async function fillFromSelectionEdge(direction: "down" | "right"): Promise<void> {
  if (!sheetId) return;
  if (!spreadsheetSelection) await refreshSpreadsheetSelection();
  const selection = spreadsheetSelection;
  if (!selection) return;
  const source = await invoke("reduce_spreadsheet_selection", { sheetId, anchor: cellAnchor, focus: cellFocus, action: "fill-source", value: direction, extend: false });
  if (source.range === selection.range) {
    toast(direction === "down" ? "Select the rows to fill into first." : "Select the columns to fill into first.");
    return;
  }
  await applyFill(sheetId, source.range, selection.range);
}

async function clearSelectedCells(): Promise<void> {
  if (!sheetId) return;
  await edit("clear_spreadsheet_selection", { sheetId, anchor: cellAnchor, focus: cellFocus });
  await renderSheets(query("[data-main]") as HTMLElement);
}

export async function setCellFormat(property: string, value: string): Promise<void> {
  if (!sheetId) return;
  await edit("set_spreadsheet_selection_format", { sheetId, anchor: cellAnchor, focus: cellFocus, property, value });
  await renderSheets(query("[data-main]") as HTMLElement);
}

// ---- Row heights and column widths ------------------------------------------
//
// The grid is re-rendered through `morphChildren`, so nothing here may depend on
// DOM state the renderer does not own. There are no handle elements: the hit
// zone is derived from the pointer position inside a header cell, and the drag
// preview is written to the very attributes the renderer emits (`<tr
// style="height">` / `<col style="width">`), which the next morph overwrites or
// removes on its own. The hover listener is an assignment, and the drag
// listeners are added on mousedown and removed on mouseup, so neither can stack.

/** How close to the trailing header edge the pointer starts a resize. */
const AXIS_HANDLE_PX = 5;

type AxisKind = "row" | "column";
type AxisTarget = { axis: AxisKind; label: string };

let axisResize: (AxisTarget & { sheetId: string; origin: number; base: number; size: number }) | null = null;

/** Stored size of a row/column, or `undefined` when it uses the default. */
function storedAxisSize(axis: AxisKind, label: string): number | undefined {
  const sheet = currentSheet();
  return axis === "row" ? sheet?.row_heights[label] : sheet?.column_widths[label];
}

/** Effective size in px: the stored one, else the core's default. */
function effectiveAxisSize(axis: AxisKind, label: string): number {
  return storedAxisSize(axis, label) ?? (axis === "row" ? APP_DEFAULT_ROW_HEIGHT_PX : APP_DEFAULT_COLUMN_WIDTH_PX);
}

function clampAxisSize(size: number): number {
  return Math.min(APP_MAX_AXIS_SIZE_PX, Math.max(APP_MIN_AXIS_SIZE_PX, Math.round(size)));
}

/** The row/column the pointer would resize, when it sits on a header edge. */
function axisTargetAt(event: MouseEvent): AxisTarget | null {
  const header = (event.target as Element | null)?.closest<HTMLElement>("th");
  if (!header) return null;
  const rect = header.getBoundingClientRect();
  const column = header.dataset.column;
  if (column) {
    return rect.width > 0 && event.clientX >= rect.right - AXIS_HANDLE_PX ? { axis: "column", label: column } : null;
  }
  if (!header.classList.contains("row-header")) return null;
  const row = header.closest<HTMLElement>("tr")?.dataset.row;
  if (!row) return null;
  return rect.height > 0 && event.clientY >= rect.bottom - AXIS_HANDLE_PX ? { axis: "row", label: row } : null;
}

/**
 * Finds the `<col>` for a column, building the `<colgroup>` when the sheet has
 * no stored width yet (the renderer omits it then). A hand-made colgroup is
 * transient: the next morph replaces it with the rendered one, or drops it.
 */
function columnElement(grid: HTMLElement, label: string): HTMLElement | null {
  const table = grid.querySelector("table.sheet-grid");
  if (!table) return null;
  let group = table.querySelector("colgroup");
  if (!group) {
    group = document.createElement("colgroup");
    const corner = document.createElement("col");
    corner.className = "row-header-col";
    group.appendChild(corner);
    for (const column of currentSheet()?.columns ?? []) {
      const col = document.createElement("col");
      col.setAttribute("data-column", column);
      group.appendChild(col);
    }
    table.insertBefore(group, table.firstChild);
  }
  return group.querySelector<HTMLElement>(`col[data-column="${label}"]`);
}

/** Paints a size onto the live grid; `null` restores what the renderer emits. */
function previewAxisSize(axis: AxisKind, label: string, size: number | null): void {
  const grid = query("[data-grid]");
  if (!grid) return;
  const stored = storedAxisSize(axis, label);
  const css = size !== null ? `${size}px` : stored !== undefined ? `${stored}px` : "";
  if (axis === "row") {
    const row = grid.querySelector<HTMLElement>(`tr[data-row="${label}"]`);
    if (row) row.style.height = css;
    return;
  }
  const col = columnElement(grid, label);
  if (col) col.style.width = css;
}

function startAxisResize(event: MouseEvent, target: AxisTarget): void {
  if (!sheetId) return;
  event.preventDefault();
  const base = effectiveAxisSize(target.axis, target.label);
  axisResize = { ...target, sheetId, origin: target.axis === "column" ? event.clientX : event.clientY, base, size: base };
  query("[data-grid]")?.classList.add("resizing");
  window.addEventListener("mousemove", onAxisResizeMove);
  window.addEventListener("mouseup", onAxisResizeEnd);
  window.addEventListener("keydown", onAxisResizeKey);
}

function onAxisResizeMove(event: MouseEvent): void {
  if (!axisResize) return;
  const pointer = axisResize.axis === "column" ? event.clientX : event.clientY;
  axisResize.size = clampAxisSize(axisResize.base + (pointer - axisResize.origin));
  previewAxisSize(axisResize.axis, axisResize.label, axisResize.size);
}

function onAxisResizeKey(event: KeyboardEvent): void {
  if (event.key === "Escape") finishAxisResize(true);
}

function onAxisResizeEnd(): void {
  finishAxisResize(false);
}

/** Ends a drag with exactly one command — never one per mousemove. */
function finishAxisResize(cancelled: boolean): void {
  const resize = axisResize;
  axisResize = null;
  window.removeEventListener("mousemove", onAxisResizeMove);
  window.removeEventListener("mouseup", onAxisResizeEnd);
  window.removeEventListener("keydown", onAxisResizeKey);
  query("[data-grid]")?.classList.remove("resizing", "resize-row", "resize-column");
  if (!resize) return;
  if (cancelled || resize.size === resize.base) {
    previewAxisSize(resize.axis, resize.label, null);
    return;
  }
  // Leave the preview in place: the re-render that follows the command either
  // confirms it or morphs it back to the stored value.
  void applyAxisSize(resize.axis, resize.label, resize.size, resize.sheetId);
}

/** `size === 0` clears the explicit size and restores the default. */
async function applyAxisSize(axis: AxisKind, label: string, size: number, sheet: string | null = sheetId): Promise<void> {
  if (!sheet) return;
  if (axis === "row") await edit("set_spreadsheet_row_height", { sheetId: sheet, row: label, height: size });
  else await edit("set_spreadsheet_column_width", { sheetId: sheet, column: label, width: size });
}

/** Row/column label of the focused cell, from the Rust-computed selection. */
async function selectedAxisLabel(axis: AxisKind): Promise<string | null> {
  if (!spreadsheetSelection) await refreshSpreadsheetSelection();
  const sheet = currentSheet();
  const selected = spreadsheetSelection;
  if (!sheet || !selected) return null;
  return (axis === "row" ? sheet.rows[selected.from_row - 1] : sheet.columns[selected.from_col - 1]) ?? null;
}

/** Menu path: prefill the current effective size and apply what comes back. */
async function promptAxisSize(axis: AxisKind): Promise<void> {
  const label = await selectedAxisLabel(axis);
  if (!label) {
    showError("Select a cell first.");
    return;
  }
  const result = await promptDialog({
    title: axis === "row" ? "Row height" : "Column width",
    fields: [
      {
        name: "size",
        label: `${axis === "row" ? "Row" : "Column"} ${label} in pixels (0 restores the default)`,
        type: "number",
        // Whole pixels: the command takes an integer, so let the field say so
        // rather than letting a fraction reach Rust and come back as an error.
        step: "1",
        value: String(effectiveAxisSize(axis, label)),
      },
    ],
    submit: "Apply",
  });
  if (!result) return;
  const requested = Number(result.size.trim());
  if (!Number.isFinite(requested)) {
    showError(`"${result.size}" is not a size in pixels.`);
    return;
  }
  await applyAxisSize(axis, label, requested <= 0 ? 0 : clampAxisSize(requested));
}

function currentSpreadsheetSelectionArgs(): { sheetId: string; anchor: string; focus: string } | null {
  return sheetId ? { sheetId, anchor: cellAnchor, focus: cellFocus } : null;
}

/** The current cell is the scope of the cell-metadata dialogs, never a guessed range. */
function focusedCellMetadataArgs(): { sheetId: string; address: string } | null {
  return sheetId ? { sheetId, address: cellFocus } : null;
}

/**
 * The durable comment model permits a conversation on a cell.  A short menu
 * action should not pretend that it is a single mutable note: choose the live
 * comment first, then edit its body through the command that preserves its
 * identity and audit trail.
 */
async function promptEditCellNote(): Promise<void> {
  const args = focusedCellMetadataArgs();
  const comments = focusedCell()?.comments.filter((comment) => !comment.deleted) ?? [];
  if (!args) return;
  if (!comments.length) {
    toast(`Cell ${args.address} has no live notes to edit.`);
    return;
  }
  const chosen = await promptDialog({
    title: `Edit note on ${args.address}`,
    fields: [{
      name: "commentId",
      label: "Note",
      type: "select",
      options: comments.map((comment) => ({ value: comment.id, label: `${comment.author}: ${comment.body.slice(0, 80)}` })),
    }],
    submit: "Continue",
  });
  if (!chosen?.commentId) return;
  const comment = comments.find((item) => item.id === chosen.commentId);
  if (!comment) return;
  const updated = await promptDialog({
    title: `Edit note on ${args.address}`,
    fields: [{ name: "body", label: "Note", type: "textarea", value: comment.body }],
    submit: "Save note",
  });
  if (!updated || !updated.body.trim()) return;
  await edit("update_spreadsheet_cell_comment", { commentId: comment.id, body: updated.body });
}

async function promptCellValidation(): Promise<void> {
  const args = focusedCellMetadataArgs();
  if (!args) return;
  const existing = focusedCell()?.validation;
  const result = await promptDialog({
    title: `Data validation for ${args.address}`,
    body: "Enter list choices or rule inputs one per line. A strict rule rejects values that do not match.",
    fields: [
      {
        name: "kind",
        label: "Rule",
        type: "select",
        value: existing?.kind ?? "list",
        options: [
          { value: "list", label: "List of choices" },
          { value: "number_greater", label: "Number greater than" },
          { value: "number_less", label: "Number less than" },
          { value: "number_between", label: "Number between" },
          { value: "text_contains", label: "Text contains" },
          { value: "custom_formula", label: "Custom formula" },
        ],
      },
      { name: "values", label: "Choices or rule inputs", type: "textarea", value: existing?.values.join("\n") ?? "" },
      {
        name: "strict",
        label: "Invalid input",
        type: "select",
        value: existing?.strict === false ? "allow" : "reject",
        options: [{ value: "reject", label: "Reject input" }, { value: "allow", label: "Allow with warning" }],
      },
    ],
    submit: "Apply validation",
  });
  if (!result) return;
  const values = result.values.split(/\r?\n/).map((value) => value.trim()).filter(Boolean);
  await edit("set_spreadsheet_cell_validation", {
    ...args,
    kind: result.kind,
    values,
    strict: result.strict === "reject",
  });
}

/**
 * Named ranges are document metadata, not a formatting shortcut. Keep their
 * manager deliberately sheet-local: the range picker is then honest about
 * which cells it can show and an update never accidentally moves a name across
 * sheets.
 */
async function promptNamedRanges(): Promise<void> {
  const sheet = currentSheet();
  if (!sheet) return;
  const ranges = state.doc?.workbook.named_ranges.filter((range) => range.sheet_id === sheet.id) ?? [];
  const choice = await promptDialog({
    title: "Named ranges",
    body: "Names refer to a rectangular range on this sheet. They are not access controls.",
    fields: [{
      name: "operation",
      label: "Action",
      type: "select",
      options: [
        { value: "add", label: "Add a named range" },
        { value: "update", label: "Change an existing range" },
        { value: "delete", label: "Delete an existing range" },
      ],
    }],
    submit: "Continue",
  });
  if (!choice) return;

  if (choice.operation === "add") {
    await refreshSpreadsheetSelection();
    const selectedRange = spreadsheetSelection?.range ?? cellFocus;
    const result = await promptDialog({
      title: "Add named range",
      fields: [
        { name: "name", label: "Name", placeholder: "QuarterlySales" },
        { name: "range", label: "Cell range", value: selectedRange },
      ],
      submit: "Add named range",
    });
    if (!result || !result.name.trim() || !result.range.trim()) return;
    await edit("add_spreadsheet_named_range", { sheetId: sheet.id, name: result.name, range: result.range });
    toast(`Named range ${result.name.trim()} added.`);
    return;
  }

  if (!ranges.length) {
    toast(`Sheet ${sheet.title} has no named ranges.`);
    return;
  }
  const selected = await promptDialog({
    title: choice.operation === "update" ? "Change named range" : "Delete named range",
    fields: [{
      name: "name",
      label: "Named range",
      type: "select",
      options: ranges.map((range) => ({ value: range.name, label: `${range.name} (${range.range})` })),
    }],
    submit: "Continue",
  });
  if (!selected?.name) return;
  const existing = ranges.find((range) => range.name === selected.name);
  if (!existing) return;
  if (choice.operation === "delete") {
    await edit("delete_spreadsheet_named_range", { name: existing.name });
    toast(`Named range ${existing.name} deleted.`);
    return;
  }
  const result = await promptDialog({
    title: `Change ${existing.name}`,
    fields: [{ name: "range", label: "Cell range", value: existing.range }],
    submit: "Save named range",
  });
  if (!result?.range.trim()) return;
  await edit("update_spreadsheet_named_range", { sheetId: sheet.id, name: existing.name, range: result.range });
  toast(`Named range ${existing.name} updated.`);
}

/**
 * Protection has a deliberately much narrower UI than a sharing surface.  The
 * v0 model records an advisory annotation only; it cannot prevent any local or
 * remote writer from changing a cell.  Make that fact the first sentence of
 * both dialogs, keep creation tied to the Rust-derived current selection, and
 * only offer edits that the existing command can faithfully make (the
 * description, not a misleading access policy or range move).
 */
async function promptProtectedRanges(): Promise<void> {
  const sheet = currentSheet();
  if (!sheet) return;
  const ranges = sheet.protected_ranges;
  const choice = await promptDialog({
    title: "Advisory protected ranges",
    body: "These ranges are warnings only. They do not restrict editing or grant access to anyone.",
    fields: [{
      name: "operation",
      label: "Action",
      type: "select",
      options: [
        { value: "add", label: "Add a warning range" },
        { value: "edit", label: "Edit a warning description" },
        { value: "delete", label: "Delete a warning range" },
      ],
    }],
    submit: "Continue",
  });
  if (!choice) return;
  if (choice.operation === "add") {
    await refreshSpreadsheetSelection();
    const range = spreadsheetSelection?.range ?? cellFocus;
    const added = await promptDialog({
      title: "Add advisory protected range",
      body: `The selected range ${range} will remain editable; this is document metadata only.`,
      fields: [{ name: "description", label: "Warning description", placeholder: "Check before changing" }],
      submit: "Add warning range",
    });
    if (!added?.description.trim()) return;
    await edit("add_spreadsheet_protected_range", {
      sheetId: sheet.id,
      range,
      description: added.description,
      warningOnly: true,
    });
    toast(`Advisory warning range ${range} added.`);
    return;
  }
  if (!ranges.length) {
    toast(`Sheet ${sheet.title} has no advisory protected ranges.`);
    return;
  }
  const picked = await promptDialog({
    title: choice.operation === "edit" ? "Edit advisory protected range" : "Delete advisory protected range",
    body: "This is warning metadata, not an access-control rule.",
    fields: [{
      name: "range",
      label: "Range",
      type: "select",
      options: ranges.map((range) => ({ value: range.range, label: `${range.range}: ${range.description}` })),
    }],
    submit: "Continue",
  });
  if (!picked?.range) return;
  const existing = ranges.find((range) => range.range === picked.range);
  if (!existing) return;
  if (choice.operation === "delete") {
    await edit("delete_spreadsheet_protected_range", { sheetId: sheet.id, range: existing.range });
    toast(`Advisory warning range ${existing.range} deleted.`);
    return;
  }
  const updated = await promptDialog({
    title: `Edit warning for ${existing.range}`,
    body: "The range stays editable. Only its warning description is changed.",
    fields: [{ name: "description", label: "Warning description", value: existing.description }],
    submit: "Save warning",
  });
  if (!updated?.description.trim()) return;
  await edit("update_spreadsheet_protected_range", {
    sheetId: sheet.id,
    range: existing.range,
    description: updated.description,
    warningOnly: true,
  });
  toast(`Advisory warning range ${existing.range} updated.`);
}

/** The cell the toolbar's format controls act on. */
export function focusedCell() {
  return currentSheet()?.cells.find((item) => item.address === cellFocus) ?? null;
}

/**
 * Every spreadsheet-scoped action, routed here by the dispatcher so that the
 * cursor, the open sheet and the workbook clipboard stay private to this
 * module. Returns false for a name this surface does not own, which is what
 * keeps the dispatcher's "unknown action" branch honest.
 */
export async function runSpreadsheetAction(action: string, data: DOMStringMap): Promise<boolean> {
  if (action.startsWith("cell-format:")) {
    const property = action.slice("cell-format:".length);
    const cell = focusedCell();
    if (property === "wrap") {
      await setCellFormat("wrap_strategy", cell?.format.wrap_strategy === "wrap" ? "" : "wrap");
      return true;
    }
    const current = property === "bold" ? cell?.format.bold : cell?.format.italic;
    await setCellFormat(property, current ? "false" : "true");
    return true;
  }
  switch (action) {
    case "select-sheet":
      sheetId = data.id ?? sheetId;
      cellAnchor = "A1";
      cellFocus = "A1";
      await renderSheets(query("[data-main]") as HTMLElement);
      break;
    case "add-sheet": {
      const result = await promptDialog({ title: "Add sheet", fields: [{ name: "title", label: "Sheet name", value: `Sheet${(state.doc?.workbook.sheets.length ?? 0) + 1}` }], submit: "Add" });
      if (result?.title) await edit("add_spreadsheet_sheet", { title: result.title });
      break;
    }
    case "add-cell-note": {
      const args = focusedCellMetadataArgs();
      if (!args) return true;
      const result = await promptDialog({
        title: `Add note to ${args.address}`,
        fields: [
          { name: "author", label: "Author", value: state.authorName || "Local user" },
          { name: "body", label: "Note", type: "textarea" },
        ],
        submit: "Add note",
      });
      if (!result || !result.author.trim() || !result.body.trim()) return true;
      await edit("add_spreadsheet_cell_comment", { ...args, author: result.author, body: result.body });
      await renderSheets(query("[data-main]") as HTMLElement);
      break;
    }
    case "edit-cell-note":
      await promptEditCellNote();
      await renderSheets(query("[data-main]") as HTMLElement);
      break;
    case "cell-validation":
      await promptCellValidation();
      await renderSheets(query("[data-main]") as HTMLElement);
      break;
    case "clear-cell-validation": {
      const args = focusedCellMetadataArgs();
      if (!args) return true;
      if (!focusedCell()?.validation) {
        toast(`Cell ${args.address} has no data validation.`);
        return true;
      }
      await edit("clear_spreadsheet_cell_validation", args);
      await renderSheets(query("[data-main]") as HTMLElement);
      break;
    }
    case "named-ranges":
      await promptNamedRanges();
      await renderSheets(query("[data-main]") as HTMLElement);
      break;
    case "protected-ranges":
      await promptProtectedRanges();
      await renderSheets(query("[data-main]") as HTMLElement);
      break;
    case "set-print-area": {
      const args = currentSpreadsheetSelectionArgs();
      if (!args) return true;
      // The selection is cached for painting, but the command must capture
      // the canonical range Rust derives from the current anchors.
      await refreshSpreadsheetSelection();
      if (!spreadsheetSelection) return true;
      await edit("set_spreadsheet_print_area", {
        sheetId: args.sheetId,
        range: spreadsheetSelection.range,
      });
      toast(`Print area set to ${spreadsheetSelection.range}.`);
      break;
    }
    case "clear-print-area": {
      const sheet = currentSheet();
      if (!sheet) return true;
      const area = sheet.print_settings.print_area;
      if (!area) {
        toast("This sheet has no print area.");
        return true;
      }
      await edit("clear_spreadsheet_print_area", { sheetId: sheet.id });
      toast(`Cleared print area ${area}.`);
      break;
    }
    case "print-orientation": {
      const sheet = currentSheet();
      if (!sheet) return true;
      const result = await promptDialog({
        title: "Print orientation",
        fields: [
          {
            name: "orientation",
            label: "Letter paper orientation",
            type: "select",
            value: sheet.print_settings.orientation,
            options: [
              { value: "landscape", label: "Landscape" },
              { value: "portrait", label: "Portrait" },
            ],
          },
        ],
        submit: "Set orientation",
      });
      if (!result) return true;
      await edit("set_spreadsheet_print_orientation", {
        sheetId: sheet.id,
        orientation: result.orientation,
      });
      toast(`Print orientation set to ${result.orientation}.`);
      break;
    }
    case "add-row":
    case "add-column":
    case "delete-row":
    case "delete-column": {
      const args = currentSpreadsheetSelectionArgs();
      if (!args) return true;
      if (action === "add-row") await edit("add_spreadsheet_row_after_selection", args);
      if (action === "add-column") await edit("add_spreadsheet_column_after_selection", args);
      if (action === "delete-row") await edit("delete_spreadsheet_selection_row", args);
      if (action === "delete-column") await edit("delete_spreadsheet_selection_column", args);
      break;
    }
    case "row-height":
      await promptAxisSize("row");
      break;
    case "column-width":
      await promptAxisSize("column");
      break;
    case "hide-rows":
    case "unhide-rows":
    case "hide-columns":
    case "unhide-columns": {
      const args = currentSpreadsheetSelectionArgs();
      if (!args) return true;
      // Unhiding works because a hidden row is still inside a selection that
      // spans it: select across the gap and reveal. Which rows the range
      // covers is Rust's answer, not one worked out here.
      const hidden = action.startsWith("hide-");
      const command = action.endsWith("-rows")
        ? "set_spreadsheet_selection_rows_hidden"
        : "set_spreadsheet_selection_columns_hidden";
      await edit(command, { ...args, hidden });
      await renderSheets(query("[data-main]") as HTMLElement);
      break;
    }
    case "merge-cells": {
      const args = currentSpreadsheetSelectionArgs();
      if (args) await edit("merge_spreadsheet_selection", args);
      break;
    }
    case "freeze": {
      const args = currentSpreadsheetSelectionArgs();
      if (args) await edit("freeze_spreadsheet_selection", args);
      break;
    }
    case "filter": {
      const args = currentSpreadsheetSelectionArgs();
      if (args) await edit("set_spreadsheet_selection_filter", args);
      break;
    }
    case "sort-range": {
      if (!sheetId) return true;
      if (!spreadsheetSelection) await refreshSpreadsheetSelection();
      const selection = spreadsheetSelection;
      if (!selection) return true;
      const columns = columnsInSelection(selection);
      const result = await promptDialog({
        title: `Sort range ${selection.range}`,
        fields: [
          { name: "column", label: "Sort by column", type: "select", options: columns.map((column) => ({ value: column, label: column })) },
          { name: "order", label: "Order", type: "select", options: [{ value: "ascending", label: "A → Z" }, { value: "descending", label: "Z → A" }] },
          { name: "header", label: "First row", type: "select", options: [{ value: "data", label: "Is data" }, { value: "header", label: "Is a header row" }] },
        ],
        submit: "Sort",
      });
      if (!result?.column) return true;
      await edit("sort_spreadsheet_range", {
        sheetId,
        range: selection.range,
        column: result.column,
        descending: result.order === "descending",
        hasHeader: result.header === "header",
      });
      await renderSheets(query("[data-main]") as HTMLElement);
      break;
    }
    case "fill-down":
    case "fill-right": {
      await fillFromSelectionEdge(action === "fill-down" ? "down" : "right");
      break;
    }
    case "import-csv": {
      if (!sheetId) return true;
      const file = await native("Could not read that CSV file", () => openFile(["csv", "tsv", "txt"]));
      // No file chosen: the dialog was closed, so there is nothing to import
      // and nothing to report.
      if (!file) return true;
      await edit("import_spreadsheet_csv", {
        sheetId,
        origin: cellFocus,
        text: textFromBase64(file.base64),
        delimiter: /\.tsv$/i.test(file.name) ? "tab" : ",",
      });
      await renderSheets(query("[data-main]") as HTMLElement);
      break;
    }
    case "import-xlsx": {
      const file = await native("Could not read that workbook", () => openFile(["xlsx"]));
      if (!file) return true;
      // `run` raises the discard prompt: replacing the workbook is guarded in
      // Rust like every other document-replacing command.
      await edit("import_spreadsheet_xlsx", { title: file.name.replace(/\.xlsx$/i, ""), base64: file.base64 });
      sheetId = null;
      cellAnchor = "A1";
      cellFocus = "A1";
      await renderSheets(query("[data-main]") as HTMLElement);
      break;
    }
    case "import-google-sheet-url": {
      const answer = await promptDialog({
        title: "Import public Google Sheet",
        fields: [{ name: "url", label: "Google Sheets sharing URL", value: "" }],
      });
      if (!answer) return true;
      const exportUrl = googlePublicExportUrl(answer.url, "spreadsheets", "xlsx");
      if (!exportUrl) {
        toast("Enter a public docs.google.com/spreadsheets/d/<id> sharing URL.");
        return true;
      }
      const file = isTauri()
        ? await native("Could not download that public Google Sheet", () => fetchUrlFile(exportUrl))
        : await fetchPublicGoogleExport(exportUrl);
      if (!file) return true;
      await edit("import_spreadsheet_xlsx", { title: "Google Sheet", base64: file.base64 });
      sheetId = null;
      cellAnchor = "A1";
      cellFocus = "A1";
      await renderSheets(query("[data-main]") as HTMLElement);
      break;
    }
    case "export-sheets-json":
      await downloadExport("export_google_sheets_json", "Google Sheets JSON");
      break;
    // Both of these go through `downloadExport` for the same reason every
    // other export does: the media type, the extension, the encoding and
    // what the format could not carry are facts the exporter states, and a
    // table here could disagree with it. This one used to — it wrote a
    // tab-delimited file as `.csv`, and it had nowhere to put a warning
    // because the command handed back a bare string.
    case "export-csv": {
      if (!sheetId) return true;
      await downloadExport("export_spreadsheet_csv", "CSV", {
        args: { sheetId, delimiter: "," },
        defaultBaseName: currentSheet()?.title ?? "sheet",
      });
      break;
    }
    case "export-xlsx": {
      await downloadExport("export_spreadsheet_xlsx", "Excel workbook", {
        defaultBaseName: state.doc?.workbook.title || state.doc?.title || "workbook",
      });
      break;
    }
    case "export-spreadsheet-pdf": {
      await downloadExport("export_spreadsheet_pdf", "spreadsheet PDF", {
        defaultBaseName: state.doc?.workbook.title || state.doc?.title || "workbook",
      });
      break;
    }
    default:
      return false;
  }
  return true;
}
