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
import type { AppBlock, AppBlockProperties, AppInline } from "./types";
import { APP_LINE_SPACING_PRESETS, APP_TWIPS_PER_POINT } from "./generated/document";
import type { ListKindName } from "./state";
import { state } from "./state";
import { edit, findBlock, focusBlock, focusInline, query, showError } from "./shared";
import { editorHooks } from "./shell";
import { focusedCell, setCellFormat } from "./spreadsheet";
import { runAction, setBulletMarkerPreset } from "./actions";

// Native colour inputs can take focus between pointer-down and their later
// `change` event. Preserve the selection the user saw when opening the
// paragraph control so a focus collapse cannot colour a different paragraph.
let paragraphBackgroundSelection: typeof state.selection = null;
// Native selects can move focus while their popup is open. Keep the list
// selection from pointer-down through change so a marker remains attached to
// the list run the reader opened the picker for.
let bulletMarkerSelection: typeof state.selection = null;
// Paragraph-spacing selects also use a native popup. Its change must retain
// the paragraph selected when it opened, not whichever block browser focus
// happens to reach before it closes.
let paragraphSpacingSelection: typeof state.selection = null;

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

function orderedListStartButton(block: AppBlock | null): string {
  if (block?.kind !== "list-item" || block.list_kind !== "ordered" || block.list_id == null || block.level == null) return "";
  const start = state.doc?.list_properties?.[block.list_id]?.ordered_starts?.[String(block.level)] ?? 1;
  return `<button type="button" class="tb" data-action="list-start" title="Set numbering start">${escapeHtml(`${start}.`)}</button>`;
}

function orderedListFormatButton(block: AppBlock | null): string {
  if (block?.kind !== "list-item" || block.list_kind !== "ordered" || block.list_id == null || block.level == null) return "";
  const format = state.doc?.list_properties?.[block.list_id]?.ordered_formats?.[String(block.level)] ?? "inherited";
  const labels: Record<string, string> = {
    inherited: "1 a i",
    decimal: "1.",
    "lower-alpha": "a.",
    "upper-alpha": "A.",
    "lower-roman": "i.",
    "upper-roman": "I.",
  };
  return `<button type="button" class="tb" data-action="list-format" title="Set numbering format">${escapeHtml(labels[format] ?? "1.")}</button>`;
}

function bulletListMarkerButton(block: AppBlock | null): string {
  if (block?.kind !== "list-item" || block.list_kind !== "bullet" || block.list_id == null || block.level == null) return "";
  const marker = state.doc?.list_properties?.[block.list_id]?.bullet_markers?.[String(block.level)]
    ?? ["disc", "circle", "square"][Number(block.level) % 3];
  const glyphs: Record<string, string> = { disc: "•", circle: "◦", square: "■" };
  return `<select class="tb-select narrow" data-select="bullet-marker" aria-label="Bullet marker" title="Bullet marker for this list level">
    ${[["disc", "• Disc"], ["circle", "◦ Circle"], ["square", "■ Square"], ["custom", "Custom…"]]
      .map(([value, label]) => `<option value="${value}"${value === marker ? " selected" : ""}>${escapeHtml(label)}</option>`)
      .join("")}
  </select><button type="button" class="tb" data-action="list-bullet-marker" title="Set custom bullet marker">${escapeHtml(glyphs[marker] ?? marker)}</button>`;
}

/** Paragraph direction controls, in the order Google Docs shows them. */
const DIRECTIONS: [string, string][] = [
  ["ltr", "Left-to-right paragraph"],
  ["rtl", "Right-to-left paragraph"],
];

function directionButton(value: string, label: string, properties: AppBlockProperties): string {
  // Unset means the block inherits, and the inherited default is left-to-right.
  const active = (properties.direction ?? "ltr") === value;
  // Spelled rather than drawn: the two icons Docs uses are a pilcrow with a
  // small arrow, which at toolbar size are one glyph apart and unreadable.
  const glyph = value === "rtl" ? "RTL" : "LTR";
  return `<button type="button" class="tb${active ? " active" : ""}" data-action="direction:${value}" aria-pressed="${active}" title="${escapeHtml(label)}">${glyph}</button>`;
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
        ["title", "Title"],
        ["subtitle", "Subtitle"],
        ["heading:1", "Heading 1"],
        ["heading:2", "Heading 2"],
        ["heading:3", "Heading 3"],
        ["heading:4", "Heading 4"],
        ["heading:5", "Heading 5"],
        ["heading:6", "Heading 6"],
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
    <label class="tb color" title="Paragraph background"><span style="background:${escapeHtml(properties.background ?? "transparent")}">¶</span><input type="color" data-paragraph-background value="${escapeHtml(properties.background ?? "#ffffff")}"></label>
    <button type="button" class="tb${properties.border ? " active" : ""}" data-action="paragraph-border" aria-pressed="${!!properties.border}" title="Toggle 1 pt paragraph border">▣</button>
    <span class="sep"></span>
    <button type="button" class="tb" data-action="insert-link" title="Insert link (Ctrl+K)">🔗</button>
    <button type="button" class="tb" data-action="comment" title="Add comment (Ctrl+Alt+M)">💬</button>
    <button type="button" class="tb" data-action="insert-image" title="Insert image">🖼</button>
    <span class="sep"></span>
    ${ALIGNMENTS.map(([value, label, shortcut]) => alignButton(value, label, shortcut, properties)).join("")}
    ${DIRECTIONS.map(([value, label]) => directionButton(value, label, properties)).join("")}
    ${presetSelect("line-spacing", "Line spacing", lineSpacingValue(properties), lineSpacingOptions(properties))}
    ${presetSelect("space-before", "Space before paragraph", properties.space_before_twips == null ? "" : String(properties.space_before_twips), paragraphSpacePresets("Before"), describeTwips)}
    ${presetSelect("space-after", "Space after paragraph", properties.space_after_twips == null ? "" : String(properties.space_after_twips), paragraphSpacePresets("After"), describeTwips)}
    <button type="button" class="tb${properties.indent_first_line_twips ? " active" : ""}" data-action="paragraph-first-line-indent" aria-pressed="${!!properties.indent_first_line_twips}" title="Set first-line or hanging indent">↤¶</button>
    <button type="button" class="tb${properties.keep_with_next === true ? " active" : ""}" data-action="keep-with-next" aria-pressed="${properties.keep_with_next === true}" title="Keep with next paragraph">↳¶</button>
    <span class="sep"></span>
    ${listButton("bullet", "Bulleted list", "•≡", block)}
    ${listButton("ordered", "Numbered list", "1≡", block)}
    ${listButton("checklist", "Checklist", "☑", block)}
    ${bulletListMarkerButton(block)}
    ${orderedListStartButton(block)}
    ${orderedListFormatButton(block)}
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
    <select class="tb-select" data-select="vertical-align" aria-label="Vertical alignment">
      ${[["top", "Top"], ["middle", "Middle"], ["bottom", "Bottom"]].map(([value, label]) => `<option value="${value}"${(cell?.format.vertical_align ?? "bottom") === value ? " selected" : ""}>${label}</option>`).join("")}
    </select>
    <button type="button" class="tb${cell?.format.wrap_strategy === "wrap" ? " active" : ""}" data-action="cell-format:wrap" title="Wrap text">↵</button>
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
    <span class="cell-summary" data-cell-summary id="spreadsheet-grid-status" role="status" aria-live="polite" aria-atomic="true"></span>`;
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
    if (select.dataset.select === "bullet-marker" || select.dataset.select === "space-before" || select.dataset.select === "space-after") {
      select.onpointerdown = () => {
        if (select.dataset.select === "bullet-marker") bulletMarkerSelection = state.selection;
        else paragraphSpacingSelection = state.selection;
      };
    }
    select.onchange = () => {
      const kind = select.dataset.select ?? "";
      if (kind === "bullet-marker") {
        const selection = bulletMarkerSelection ?? state.selection;
        bulletMarkerSelection = null;
        void setBulletMarkerPreset(select.value, selection);
      } else if (kind === "space-before" || kind === "space-after") {
        const selection = paragraphSpacingSelection ?? state.selection;
        paragraphSpacingSelection = null;
        void setParagraphSpace(kind, select.value, selection);
      } else {
        void onSelectChange(kind, select.value);
      }
    };
  });
  bar.querySelectorAll<HTMLInputElement>("input[data-color]").forEach((input) => {
    input.onchange = () => void applyMark(input.dataset.color ?? "color", input.value, "set");
  });
  bar.querySelectorAll<HTMLInputElement>("input[data-paragraph-background]").forEach((input) => {
    input.onpointerdown = () => {
      paragraphBackgroundSelection = state.selection;
    };
    input.onchange = () => {
      const selection = paragraphBackgroundSelection ?? state.selection;
      paragraphBackgroundSelection = null;
      void setParagraphBackground(input.value, selection);
    };
  });
  bar.querySelectorAll<HTMLInputElement>("input[data-cell-color]").forEach((input) => {
    input.onchange = () => void setCellFormat(input.dataset.cellColor ?? "text_color", input.value);
  });
}

async function setParagraphBackground(color: string, selection = state.selection): Promise<void> {
  if (!selection) return;
  await edit("set_editor_selection_block_background", { selection, color });
  state.editor?.setSelection(selection);
  state.editor?.focus();
}

async function onSelectChange(kind: string, value: string): Promise<void> {
  if (kind === "style") {
    await runAction(value.startsWith("heading:") ? `style:heading:${value.split(":")[1]}` : value === "title" || value === "subtitle" ? `style:${value}` : value.startsWith("list:") ? `style:list:${value.split(":")[1]}` : "style:paragraph");
  } else if (kind === "line-spacing") {
    await setLineSpacing(value);
  } else if (kind === "space-before" || kind === "space-after") {
    await setParagraphSpace(kind === "space-before" ? "space-before" : "space-after", value);
  } else if (kind === "font" || kind === "size") {
    await applyMark(kind, value, "set");
  } else if (kind === "bullet-marker") {
    await runAction(value === "custom" ? "list-bullet-marker" : `list-bullet-marker:${value}`);
  } else if (kind === "align") {
    await setCellFormat("horizontal_align", value);
  } else if (kind === "number-format") {
    await setCellFormat("number_format", value);
  } else if (kind === "vertical-align") {
    await setCellFormat("vertical_align", value);
  }
}

export async function applyMark(kind: string, value: string | null, action: "toggle" | "set" | "remove"): Promise<void> {
  if (!state.selection) return;
  if (state.documentEditingMode === "suggest") {
    const proposal = formatSuggestionRange(kind, value, action);
    if (!proposal) return;
    await edit(
      proposal.remove
        ? "add_text_range_format_removal_suggestion"
        : proposal.expectedValue !== undefined
          ? "add_text_range_format_replacement_suggestion"
          : "add_text_range_format_suggestion",
      {
      startInlineId: proposal.startInlineId,
      endInlineId: proposal.endInlineId,
      author: state.authorName,
      markKind: kind,
      ...(proposal.expectedValue !== undefined ? { expectedValue: proposal.expectedValue } : {}),
      value: proposal.remove ? null : value,
      },
    );
    state.editor?.setSelection(state.selection);
    state.editor?.focus();
    return;
  }
  try {
    const result = await invoke("apply_editor_mark", { selection: state.selection, mark_kind: kind, value, action });
    editorHooks.onResult(result);
    state.editor?.focus();
  } catch (error) {
    showError(error instanceof Error ? error.message : String(error));
  }
}

/**
 * The suggestion model addresses a format proposal by complete inline ids,
 * whereas the live editor may select arbitrary character offsets.  Do not
 * widen a character selection here: that would make "bold this word" propose
 * bolding a larger run.  Direct formatting has a splitting pass for that;
 * suggestion mode deliberately refuses it until suggestions can carry the
 * same character-granular boundary.
 */
function formatSuggestionRange(
  kind: string,
  value: string | null,
  action: "toggle" | "set" | "remove",
): { startInlineId: string; endInlineId: string; remove: boolean; expectedValue?: string } | null {
  const selection = state.selection;
  if (!selection || kind === "all" || kind === "link") {
    showError("Suggest mode can propose text-format changes on a selected whole run; it cannot change links or clear every format at once.");
    return null;
  }
  // A value-bearing mark can be added or replaced whole-inline. Replacement
  // now carries the exact displayed source value as a compare-and-set
  // precondition, so review cannot overwrite a concurrent formatting edit.
  const isValueReplacement = action === "set"
    && value !== null
    && (kind === "color" || kind === "background" || kind === "font" || kind === "size");
  if (value !== null && action !== "remove" && !isValueReplacement) {
    showError("Suggest mode can add or replace a font, size, text colour, or highlight on complete text runs.");
    return null;
  }
  const { anchor, focus } = selection;
  if (!anchor.inline_id || !focus.inline_id) {
    showError("Select complete text runs before proposing a format change.");
    return null;
  }
  const inlines = documentInlines();
  const anchorIndex = inlines.findIndex((inline) => inline.id === anchor.inline_id);
  const focusIndex = inlines.findIndex((inline) => inline.id === focus.inline_id);
  if (anchorIndex < 0 || focusIndex < 0) {
    showError("The selected text is no longer present.");
    return null;
  }
  const anchorLength = Array.from(inlines[anchorIndex].text).length;
  const focusLength = Array.from(inlines[focusIndex].text).length;
  const forward = anchor.offset === 0 && focus.offset === focusLength;
  const backward = focus.offset === 0 && anchor.offset === anchorLength;
  if (!forward && !backward) {
    showError("Select complete text runs before proposing a format change; partial text formatting suggestions are not representable yet.");
    return null;
  }
  const start = Math.min(anchorIndex, focusIndex);
  const end = Math.max(anchorIndex, focusIndex);
  const selected = inlines.slice(start, end + 1);
  if (selected.length === 0 || selected.some((inline) => !inline.text)) {
    showError("Select text before proposing a format change.");
    return null;
  }
  const existingValues = selected.map((inline) => inline.mark_values[kind]);
  const expectedValue = isValueReplacement && existingValues.every((candidate) => candidate !== undefined)
    && existingValues.every((candidate) => candidate === existingValues[0])
    ? existingValues[0]
    : undefined;
  if (isValueReplacement && selected.some((inline) => inline.mark_kinds?.includes(kind)) && expectedValue === undefined) {
    showError("Select complete runs with the same current font, size, text colour, or highlight before proposing its replacement.");
    return null;
  }
  const remove = action === "remove" || (action === "toggle" && selected.every((inline) => inline.mark_kinds?.includes(kind)));
  // `FormatRemove` already owns the source-preserving whole-run semantics for
  // a value-bearing mark with no value: it removes every current value only
  // on acceptance. Replacement remains deliberately unsupported because a
  // `Format` add has no expected-old-value precondition. In either case every
  // selected run must visibly have the requested mark; otherwise the proposed
  // removal would claim to affect content the reviewer cannot see.
  if (remove && (value !== null || !selected.every((inline) => inline.mark_kinds?.includes(kind)))) {
    showError("Select complete runs that already have this format before proposing its removal.");
    return null;
  }
  return { startInlineId: inlines[start].id, endInlineId: inlines[end].id, remove, expectedValue };
}

/** Flatten body and table-cell inlines in the stable document order. */
function documentInlines(): AppInline[] {
  const result: AppInline[] = [];
  const visit = (blocks: AppBlock[]) => {
    for (const block of blocks) {
      result.push(...block.content);
      for (const row of block.rows ?? []) {
        for (const cell of row) visit(cell);
      }
    }
  };
  if (state.doc) visit(state.doc.blocks);
  return result;
}


export async function setBlockStyle(
  style: "paragraph" | "title" | "subtitle" | "heading" | "list-item",
  level: number,
  listKind: ListKindName,
): Promise<void> {
  // Applying a block style morphs the editor while this command is in flight.
  // Browser selection reconciliation may then clear `state.selection` before
  // the awaited call resumes. Keep the model position the toolbar action was
  // opened for, but only restore it if that same block survived the result.
  const selection = state.selection;
  if (!selection) return;
  const restoreSelection = () => {
    const live = findBlock(selection.focus.block_id) ? selection : null;
    state.selection = live;
    renderToolbar();
    state.editor?.setSelection(live);
    state.editor?.focus();
  };
  if (state.documentEditingMode === "suggest") {
    // Lists are excluded by ADR 0039: their durable run/level state cannot be
    // represented by changing just one block kind.  A suggestion also names
    // exactly the focused block rather than widening an arbitrary selection.
    if (style === "list-item") {
      showError("Suggest mode cannot propose list conversion; list-run changes need their own review operation.");
      return;
    }
    const block = focusBlock();
    if (!block) {
      showError("Place the caret in one paragraph before proposing its style.");
      return;
    }
    const proposed = style === "heading" ? `heading:${level}` : style;
    await edit("add_paragraph_style_suggestion", {
      blockId: block.id,
      author: state.authorName,
      style: proposed,
    });
    restoreSelection();
    return;
  }
  await edit("set_editor_selection_block_style", { selection, style, level, listKind });
  restoreSelection();
}

/**
 * Paragraph formatting always goes through a command: the empty value clears
 * the property so the block goes back to inheriting, which is a different
 * document state from an explicit zero and must not be conflated with one.
 *
 * `choice` is an index into `APP_LINE_SPACING_PRESETS` — the list Rust
 * generates — or `LINE_SPACING_INHERIT`. The mode/value pair the command takes
 * is read straight off the preset, so this module never joins those two into a
 * string or takes one apart again.
 */
export async function setLineSpacing(choice: string): Promise<void> {
  if (!state.selection) return;
  if (choice === LINE_SPACING_INHERIT) {
    await edit("clear_editor_selection_block_property", { selection: state.selection, key: "line-spacing" });
  } else {
    const preset = APP_LINE_SPACING_PRESETS[Number(choice)];
    // An index the presets do not cover is the "keep what the document has"
    // option, which is not a change at all.
    if (!preset) return;
    await edit("set_editor_selection_block_line_spacing", { selection: state.selection, spacingMode: preset.mode, spacingValue: preset.value });
  }
  state.editor?.setSelection(state.selection);
  state.editor?.focus();
}

async function setParagraphSpace(
  key: "space-before" | "space-after",
  value: string,
  selection = state.selection,
): Promise<void> {
  if (!selection) return;
  if (value === "") {
    await edit("clear_editor_selection_block_property", { selection, key });
  } else {
    const command = key === "space-before" ? "set_editor_selection_block_space_before" : "set_editor_selection_block_space_after";
    await edit(command, { selection, twips: Number(value) });
  }
  state.editor?.setSelection(selection);
  state.editor?.focus();
}

export async function setBlockAlignment(alignment: string): Promise<void> {
  if (!state.selection) return;
  await edit("set_editor_selection_block_alignment", { selection: state.selection, alignment });
  state.editor?.setSelection(state.selection);
  state.editor?.focus();
}

/**
 * Paragraph direction, for the blocks the selection covers.
 *
 * The whole of RTL was already finished in Rust — the command, the
 * `direction` block property, and `opendoc-render` writing `direction:rtl`
 * on the block — and nothing in the UI could reach any of it. Alignment is
 * already stated direction-relatively (`start`/`end`), so setting the
 * direction is all a right-to-left paragraph needs.
 */
export async function setBlockDirection(direction: string): Promise<void> {
  if (!state.selection) return;
  await edit("set_editor_selection_block_direction", { selection: state.selection, direction });
  state.editor?.setSelection(state.selection);
  state.editor?.focus();
}

/** Toggle the durable pagination relationship for every selected paragraph. */
export async function toggleKeepWithNext(): Promise<void> {
  if (!state.selection) return;
  const next = focusBlockProperties().keep_with_next !== true;
  await edit("set_editor_selection_block_keep_with_next", { selection: state.selection, keepWithNext: next });
  state.editor?.setSelection(state.selection);
  state.editor?.focus();
}

/** A compact toolbar affordance for the bounded uniform paragraph frame.
 * More elaborate per-edge/padding dialogs would promise semantics the model
 * deliberately does not own. */
export async function toggleParagraphBorder(): Promise<void> {
  if (!state.selection) return;
  if (focusBlockProperties().border) {
    await edit("clear_editor_selection_block_property", { selection: state.selection, key: "border" });
  } else {
    await edit("set_editor_selection_block_border", { selection: state.selection, style: "solid", twips: 20, color: "#000000" });
  }
  state.editor?.setSelection(state.selection);
  state.editor?.focus();
}

/** Blocks the caret covers, for showing which paragraph controls are active. */
function focusBlockProperties(): AppBlockProperties {
  return focusBlock()?.properties ?? {};
}

/** The `<option>` value standing for "no line spacing set; inherit". */
const LINE_SPACING_INHERIT = "";
/** The `<option>` value standing for a spacing no preset covers. */
const LINE_SPACING_OTHER = "other";

/** Which preset the block's spacing is, as an `<option>` value.
 *
 *  The spacing itself is compared as the model's own `mode`/`value` pair, so
 *  nothing here has to know how those two would be spelled together. */
function lineSpacingValue(properties: AppBlockProperties): string {
  if (properties.line_spacing_mode == null || properties.line_spacing_value == null) {
    return LINE_SPACING_INHERIT;
  }
  const index = APP_LINE_SPACING_PRESETS.findIndex(
    (preset) => preset.mode === properties.line_spacing_mode && preset.value === properties.line_spacing_value,
  );
  return index >= 0 ? String(index) : LINE_SPACING_OTHER;
}

/** The presets, plus the block's own spacing when no preset covers it — an
 *  imported document, say. Rust labels that one (`line_spacing_label`), because
 *  how a spacing reads is a projection rule and not the toolbar's. */
function lineSpacingOptions(properties: AppBlockProperties): [string, string][] {
  const presets: [string, string][] = [
    // The empty value is "inherit", which is not the same document state as
    // any explicit number — picking it clears the property.
    [LINE_SPACING_INHERIT, "Line ⇕"],
    ...APP_LINE_SPACING_PRESETS.map((preset, index) => [String(index), preset.label] as [string, string]),
  ];
  if (lineSpacingValue(properties) !== LINE_SPACING_OTHER) return presets;
  return [...presets, [LINE_SPACING_OTHER, properties.line_spacing_label ?? "Custom"]];
}

/**
 * Renders a `<select>` of preset values whose first entry is the empty value:
 * choosing it clears the property so the block inherits again, which is a
 * different document state from any explicit number. A value the presets do
 * not cover (an imported document, say) is appended as its own selected
 * option rather than being silently displayed as something else — either by
 * `describeOther` here, or by the caller when Rust already labelled it.
 */
function presetSelect(
  name: string,
  label: string,
  current: string,
  presets: [string, string][],
  describeOther?: (value: string) => string,
): string {
  const known = presets.some(([value]) => value === current);
  const options =
    known || current === "" || !describeOther
      ? presets
      : [...presets, [current, describeOther(current)] as [string, string]];
  return `<select class="tb-select spacing" data-select="${name}" aria-label="${escapeHtml(label)}" title="${escapeHtml(label)}">
      ${options
        .map(([value, text]) => `<option value="${escapeHtml(value)}"${value === current ? " selected" : ""}>${escapeHtml(text)}</option>`)
        .join("")}
    </select>`;
}

// Stated in points and converted with the model's own factor, so the labels
// and the stored twips cannot drift apart.
const paragraphSpacePresets = (inheritLabel: string): [string, string][] => [
  ["", inheritLabel],
  ...[0, 6, 12, 18].map((points) => [String(points * APP_TWIPS_PER_POINT), `${points} pt`] as [string, string]),
];

function describeTwips(value: string): string {
  return `${Number(value) / APP_TWIPS_PER_POINT} pt`;
}
