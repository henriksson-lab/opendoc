// The formatting toolbar, and the commands its controls dispatch.
//
// Two toolbars share the strip: the document one and the spreadsheet one. Which
// is drawn follows `state.mode`. The buttons carry `data-action` and go through
// the delegated dispatcher; the `<select>`s and colour inputs cannot, so they
// get `onchange` **assignments** — never `addEventListener` — because the strip
// is re-rendered on every selection change and an added listener would stack.
import { morphChildren } from "./editor";
import { escapeHtml } from "./ui";
import { invoke } from "./invoke";
import type { AppBlock, AppBlockProperties } from "./types";
import type { ListKindName } from "./state";
import { state } from "./state";
import { edit, focusBlock, focusInline, query, showError } from "./shared";
import { editorHooks } from "./shell";
import { focusedCell, setCellFormat } from "./spreadsheet";
import { runAction } from "./actions";

/**
 * Alignment controls. The value is the model's own name — `start`/`end` are
 * direction-relative there and in CSS, so nothing here has to know whether the
 * paragraph runs left-to-right.
 */
const ALIGNMENTS: [string, string, string][] = [
  ["start", "Align left", "Ctrl+Shift+L"],
  ["center", "Centre", "Ctrl+Shift+E"],
  ["end", "Align right", "Ctrl+Shift+R"],
  ["justify", "Justify", "Ctrl+Shift+J"],
];

/** Ctrl+Shift+<key> -> alignment, the Google Docs bindings. */
export const ALIGNMENT_SHORTCUTS: Record<string, string> = {
  l: "start",
  e: "center",
  r: "end",
  j: "justify",
};

/** Three stacked bars, drawn per alignment. */
function alignIcon(value: string): string {
  const rows: Record<string, [number, number][]> = {
    start: [[1, 14], [1, 9], [1, 12]],
    center: [[1, 14], [4, 8], [2, 12]],
    end: [[1, 14], [6, 9], [3, 12]],
    justify: [[1, 14], [1, 14], [1, 14]],
  };
  const bars = (rows[value] ?? rows.start)
    .map(([x, width], index) => `<rect x="${x}" y="${2 + index * 5}" width="${width}" height="2" rx="1"/>`)
    .join("");
  return `<svg viewBox="0 0 16 16" width="15" height="15" fill="currentColor" aria-hidden="true">${bars}</svg>`;
}

function alignButton(value: string, label: string, shortcut: string, properties: AppBlockProperties): string {
  // No alignment set means the block inherits, and the inherited default is
  // `start`; showing nothing active there would make the toolbar look broken.
  const active = (properties.alignment ?? "start") === value;
  return `<button type="button" class="tb${active ? " active" : ""}" data-action="align:${value}" aria-pressed="${active}" title="${escapeHtml(`${label} (${shortcut})`)}">${alignIcon(value)}</button>`;
}

function listButton(kind: ListKindName, label: string, glyph: string, block: AppBlock | null): string {
  const active = block?.kind === "list-item" && block.list_kind === kind;
  return `<button type="button" class="tb${active ? " active" : ""}" data-action="style:list:${kind}" aria-pressed="${active}" title="${escapeHtml(label)}">${glyph}</button>`;
}

const FONT_SIZES = ["8", "9", "10", "11", "12", "14", "18", "24", "36"];
const FONTS = ["Arial", "Georgia", "Times New Roman", "Courier New", "Verdana", "Inter"];

export function renderToolbar(): void {
  const bar = query("[data-toolbar]");
  if (!bar || !state.doc) return;
  const inline = focusInline();
  const kinds = new Set(inline?.mark_kinds ?? []);
  const markValue = (kind: string, fallback: string) => inline?.mark_values[kind] ?? fallback;
  const block = focusBlock();
  const properties = focusBlockProperties();
  const styleValue = block?.style_value ?? "paragraph";
  const toggle = (kind: string, label: string, title: string) =>
    `<button type="button" data-action="mark:${kind}" class="tb${kinds.has(kind) ? " active" : ""}" aria-pressed="${kinds.has(kind)}" title="${escapeHtml(title)}">${label}</button>`;
  const docsToolbar = `
    <button type="button" class="tb" data-action="undo" title="Undo (Ctrl+Z)">↶</button>
    <button type="button" class="tb" data-action="redo" title="Redo (Ctrl+Y)">↷</button>
    <span class="sep"></span>
    <select class="tb-select" data-select="style" aria-label="Paragraph style">
      ${[
        ["paragraph", "Normal text"],
        ["heading:1", "Heading 1"],
        ["heading:2", "Heading 2"],
        ["heading:3", "Heading 3"],
        ["heading:4", "Heading 4"],
        ["list:bullet", "Bulleted list"],
        ["list:ordered", "Numbered list"],
        ["list:checklist", "Checklist"],
      ]
        .map(([value, label]) => `<option value="${value}"${value === styleValue ? " selected" : ""}>${label}</option>`)
        .join("")}
    </select>
    <select class="tb-select" data-select="font" aria-label="Font">
      ${FONTS.map((font) => `<option value="${font}"${markValue("font", "Arial") === font ? " selected" : ""}>${font}</option>`).join("")}
    </select>
    <select class="tb-select narrow" data-select="size" aria-label="Font size">
      ${FONT_SIZES.map((size) => `<option value="${size}"${markValue("size", "11") === size ? " selected" : ""}>${size}</option>`).join("")}
    </select>
    <span class="sep"></span>
    ${toggle("bold", "<b>B</b>", "Bold (Ctrl+B)")}
    ${toggle("italic", "<i>I</i>", "Italic (Ctrl+I)")}
    ${toggle("underline", "<u>U</u>", "Underline (Ctrl+U)")}
    ${toggle("strike", "<s>S</s>", "Strikethrough")}
    <label class="tb color" title="Text colour"><span style="border-bottom:3px solid ${escapeHtml(markValue("color", "#000"))}">A</span><input type="color" data-color="color" value="${escapeHtml(markValue("color", "#000000"))}"></label>
    <label class="tb color" title="Highlight"><span style="background:${escapeHtml(markValue("background", "transparent"))}">▮</span><input type="color" data-color="background" value="${escapeHtml(markValue("background", "#ffff00"))}"></label>
    <span class="sep"></span>
    <button type="button" class="tb" data-action="insert-link" title="Insert link (Ctrl+K)">🔗</button>
    <button type="button" class="tb" data-action="comment" title="Add comment (Ctrl+Alt+M)">💬</button>
    <button type="button" class="tb" data-action="insert-image" title="Insert image">🖼</button>
    <span class="sep"></span>
    ${ALIGNMENTS.map(([value, label, shortcut]) => alignButton(value, label, shortcut, properties)).join("")}
    ${presetSelect("line-spacing", "Line spacing", lineSpacingValue(properties), LINE_SPACING_PRESETS, describeSpacing)}
    ${presetSelect("space-before", "Space before paragraph", properties.space_before_twips == null ? "" : String(properties.space_before_twips), paragraphSpacePresets("Before"), describeTwips)}
    ${presetSelect("space-after", "Space after paragraph", properties.space_after_twips == null ? "" : String(properties.space_after_twips), paragraphSpacePresets("After"), describeTwips)}
    <span class="sep"></span>
    ${listButton("bullet", "Bulleted list", "•≡", block)}
    ${listButton("ordered", "Numbered list", "1≡", block)}
    ${listButton("checklist", "Checklist", "☑", block)}
    <button type="button" class="tb" data-action="outdent" title="Decrease indent (Ctrl+[)">⇤</button>
    <button type="button" class="tb" data-action="indent" title="Increase indent (Ctrl+])">⇥</button>
    <button type="button" class="tb" data-action="clear-marks" title="Clear formatting">Tx</button>
    <span class="grow"></span>
    <button type="button" class="tb" data-action="zoom-out" title="Zoom out">−</button>
    <span class="zoom-label">${Math.round(state.zoom * 100)}%</span>
    <button type="button" class="tb" data-action="zoom-in" title="Zoom in">+</button>`;
  const cell = focusedCell();
  const sheetsToolbar = `
    <button type="button" class="tb" data-action="undo" title="Undo">↶</button>
    <button type="button" class="tb" data-action="redo" title="Redo">↷</button>
    <span class="sep"></span>
    <button type="button" class="tb${cell?.format.bold ? " active" : ""}" data-action="cell-format:bold" title="Bold"><b>B</b></button>
    <button type="button" class="tb${cell?.format.italic ? " active" : ""}" data-action="cell-format:italic" title="Italic"><i>I</i></button>
    <label class="tb color" title="Text colour"><span>A</span><input type="color" data-cell-color="text_color" value="${escapeHtml(cell?.format.text_color ?? "#000000")}"></label>
    <label class="tb color" title="Fill"><span>▮</span><input type="color" data-cell-color="background_color" value="${escapeHtml(cell?.format.background_color ?? "#ffffff")}"></label>
    <select class="tb-select" data-select="align" aria-label="Alignment">
      ${["left", "center", "right"].map((value) => `<option value="${value}"${(cell?.format.horizontal_align ?? "left") === value ? " selected" : ""}>${value}</option>`).join("")}
    </select>
    <select class="tb-select" data-select="number-format" aria-label="Number format">
      ${["general", "number", "percent", "currency", "date", "time", "text"].map((value) => `<option value="${value}"${(cell?.format.number_format ?? "general") === value ? " selected" : ""}>${value}</option>`).join("")}
    </select>
    <span class="sep"></span>
    <button type="button" class="tb" data-action="add-row" title="Insert row below">＋row</button>
    <button type="button" class="tb" data-action="add-column" title="Insert column right">＋col</button>
    <button type="button" class="tb" data-action="delete-row" title="Delete row">−row</button>
    <button type="button" class="tb" data-action="delete-column" title="Delete column">−col</button>
    <button type="button" class="tb" data-action="merge-cells" title="Merge selection">⊞</button>
    <button type="button" class="tb" data-action="freeze" title="Freeze rows/columns above and left of the selection">❄</button>
    <button type="button" class="tb" data-action="filter" title="Create a filter on the selection">⏷</button>
    <span class="grow"></span>
    <span class="cell-summary" data-cell-summary></span>`;
  const template = document.createElement("template");
  template.innerHTML = state.mode === "docs" ? docsToolbar : sheetsToolbar;
  morphChildren(bar, template.content);
  // Toolbar buttons must not take focus: a focus change collapses the document
  // selection in some browsers, and the mark commands then run against an empty
  // range and do nothing. Assigned (not added) so re-rendering cannot stack it.
  bar.onmousedown = (event) => {
    if ((event.target as Element | null)?.closest("button[data-action]")) {
      event.preventDefault();
    }
  };
  bar.querySelectorAll<HTMLSelectElement>("select[data-select]").forEach((select) => {
    select.onchange = () => void onSelectChange(select.dataset.select ?? "", select.value);
  });
  bar.querySelectorAll<HTMLInputElement>("input[data-color]").forEach((input) => {
    input.onchange = () => void applyMark(input.dataset.color ?? "color", input.value, "set");
  });
  bar.querySelectorAll<HTMLInputElement>("input[data-cell-color]").forEach((input) => {
    input.onchange = () => void setCellFormat(input.dataset.cellColor ?? "text_color", input.value);
  });
}

async function onSelectChange(kind: string, value: string): Promise<void> {
  if (kind === "style") {
    await runAction(value.startsWith("heading:") ? `style:heading:${value.split(":")[1]}` : value.startsWith("list:") ? `style:list:${value.split(":")[1]}` : "style:paragraph");
  } else if (kind === "line-spacing") {
    await setLineSpacing(value);
  } else if (kind === "space-before" || kind === "space-after") {
    await setParagraphSpace(kind === "space-before" ? "space-before" : "space-after", value);
  } else if (kind === "font" || kind === "size") {
    await applyMark(kind, value, "set");
  } else if (kind === "align") {
    await setCellFormat("horizontal_align", value);
  } else if (kind === "number-format") {
    await setCellFormat("number_format", value);
  }
}

export async function applyMark(kind: string, value: string | null, action: "toggle" | "set" | "remove"): Promise<void> {
  if (!state.selection) return;
  try {
    const result = await invoke("apply_editor_mark", { selection: state.selection, mark_kind: kind, value, action });
    editorHooks.onResult(result);
    state.editor?.focus();
  } catch (error) {
    showError(error instanceof Error ? error.message : String(error));
  }
}


export async function setBlockStyle(
  style: "paragraph" | "heading" | "list-item",
  level: number,
  listKind: ListKindName,
): Promise<void> {
  if (!state.selection) return;
  await edit("set_editor_selection_block_style", { selection: state.selection, style, level, listKind });
  state.editor?.setSelection(state.selection);
  state.editor?.focus();
}

/**
 * Paragraph formatting always goes through a command: the empty value clears
 * the property so the block goes back to inheriting, which is a different
 * document state from an explicit zero and must not be conflated with one.
 */
export async function setLineSpacing(value: string): Promise<void> {
  if (!state.selection) return;
  if (value === "") {
    await edit("clear_editor_selection_block_property", { selection: state.selection, key: "line-spacing" });
  } else {
    const [spacingMode, raw] = value.split(":");
    await edit("set_editor_selection_block_line_spacing", { selection: state.selection, spacingMode, spacingValue: Number(raw) });
  }
  state.editor?.setSelection(state.selection);
  state.editor?.focus();
}

async function setParagraphSpace(key: "space-before" | "space-after", value: string): Promise<void> {
  if (!state.selection) return;
  if (value === "") {
    await edit("clear_editor_selection_block_property", { selection: state.selection, key });
  } else {
    const command = key === "space-before" ? "set_editor_selection_block_space_before" : "set_editor_selection_block_space_after";
    await edit(command, { selection: state.selection, twips: Number(value) });
  }
  state.editor?.setSelection(state.selection);
  state.editor?.focus();
}

export async function setBlockAlignment(alignment: string): Promise<void> {
  if (!state.selection) return;
  await edit("set_editor_selection_block_alignment", { selection: state.selection, alignment });
  state.editor?.setSelection(state.selection);
  state.editor?.focus();
}

/** Blocks the caret covers, for showing which paragraph controls are active. */
function focusBlockProperties(): AppBlockProperties {
  return focusBlock()?.properties ?? {};
}

/** `"multiple:1500"` and friends: the wire form of a line-spacing preset. */
function lineSpacingValue(properties: AppBlockProperties): string {
  const mode = properties.line_spacing_mode;
  const value = properties.line_spacing_value;
  return mode && value != null ? `${mode}:${value}` : "";
}

/**
 * Renders a `<select>` of preset values whose first entry is the empty value:
 * choosing it clears the property so the block inherits again, which is a
 * different document state from any explicit number. A value the presets do
 * not cover (an imported document, say) is appended as its own selected
 * option rather than being silently displayed as something else.
 */
function presetSelect(
  name: string,
  label: string,
  current: string,
  presets: [string, string][],
  describeOther: (value: string) => string,
): string {
  const known = presets.some(([value]) => value === current);
  const options = known || current === "" ? presets : [...presets, [current, describeOther(current)] as [string, string]];
  return `<select class="tb-select spacing" data-select="${name}" aria-label="${escapeHtml(label)}" title="${escapeHtml(label)}">
      ${options
        .map(([value, text]) => `<option value="${escapeHtml(value)}"${value === current ? " selected" : ""}>${escapeHtml(text)}</option>`)
        .join("")}
    </select>`;
}

const LINE_SPACING_PRESETS: [string, string][] = [
  // The empty value is "inherit", which is not the same document state as any
  // explicit number — picking it clears the property.
  ["", "Line ⇕"],
  ["multiple:1000", "Single"],
  ["multiple:1150", "1.15"],
  ["multiple:1500", "1.5"],
  ["multiple:2000", "Double"],
];

// 20 twips to the point, so these are 0 / 6pt / 12pt / 18pt.
const paragraphSpacePresets = (inheritLabel: string): [string, string][] => [
  ["", inheritLabel],
  ["0", "0 pt"],
  ["120", "6 pt"],
  ["240", "12 pt"],
  ["360", "18 pt"],
];

function describeSpacing(value: string): string {
  const [mode, raw] = value.split(":");
  const amount = Number(raw);
  if (mode === "multiple") return `${(amount / 1000).toFixed(2).replace(/0+$/, "").replace(/\.$/, "")}×`;
  return `${amount / 20} pt`;
}

function describeTwips(value: string): string {
  return `${Number(value) / 20} pt`;
}
