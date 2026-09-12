// Page geometry, pagination and page placement.
//
// Pagination is computed in Rust (`crates/opendoc-layout`) and applied here.
// This module measures nothing: it asks the core where the pages fall and
// places what it is told. Why the decision moved out of the browser — and how
// the font loop is closed so that what Rust decided is what Chrome draws — is
// recorded in `docs/adr/0014-pagination-in-rust.md`, which supersedes 0009.
import { promptDialog } from "./ui";
import { invoke } from "./invoke";
import type { AppDocumentLayout } from "./generated/layout";
import { editorHost, state } from "./state";
import { edit, query } from "./shared";

/** Pushes the document's own page geometry onto the page stack and the print
 *  box. Both strings are projections produced by `opendoc-render`, so the
 *  page's shape has exactly one source and it is the model. */
export function applyPageGeometry(): void {
  const layout = state.doc?.page_layout;
  const stack = query("[data-page-stack]");
  if (stack) stack.setAttribute("style", `${layout?.style ?? ""} --zoom: ${state.zoom};`);
  // `@page` cannot read custom properties in any shipping engine, so the
  // print box needs a real rule. One element, rewritten in place, so printing
  // twice does not leave two rules behind.
  applyStyleRule("page-print-style", layout?.print_style ?? "");
}

/** Writes one document-level `<style>` element, creating it once.
 *
 *  Used for the two rules that cannot be custom properties on an element: the
 *  `@page` box, and the type scale, which has to reach `:root` without
 *  overwriting a `style` attribute somebody else owns. */
function applyStyleRule(id: string, text: string): void {
  let element = document.getElementById(id);
  if (!element) {
    element = document.createElement("style");
    element.id = id;
    document.head.appendChild(element);
  }
  if (element.textContent !== text) element.textContent = text;
}

/**
 * Asks Rust where the pages fall, and applies the answer.
 *
 * Nothing is measured and nothing is computed here. `layout_document` returns
 * the page count, a placement per block, and the exact CSS margin that opens
 * each page; this function copies those onto the DOM. The one thing it adds
 * is the page gutter — the gap between sheets is a property of the viewer,
 * not of the document, so Rust describes pages that touch and the gutter
 * enters as `var(--page-gap)` inside a `calc()`. That is also why printing
 * needs no special case: the print stylesheet sets the gap to zero.
 */
export async function paginate(): Promise<void> {
  if (!state.doc?.is_open || state.mode !== "docs") return;
  const sheets = query("[data-page-sheets]");
  const flow = query("[data-page]");
  if (!sheets || !flow) return;
  let layout: AppDocumentLayout;
  try {
    layout = await invoke("layout_document");
  } catch {
    // A layout failure must not take the editor down with it; the next render
    // asks again. The sheets simply stay as they were.
    return;
  }
  // Between the await and here the user may have closed the document or
  // switched to the spreadsheet; re-check rather than draw into a dead DOM.
  if (!state.doc?.is_open || state.mode !== "docs" || !sheets.isConnected) return;
  applyLayout(layout, sheets, flow);
}

function applyLayout(layout: AppDocumentLayout, sheets: HTMLElement, flow: HTMLElement): void {
  // The type scale reaches `:root` as a rule rather than as an inline style:
  // the sizes Rust measured with are the sizes the stylesheet draws with, and
  // this is how they get there. See `opendoc_layout::style`.
  applyStyleRule("doc-type-scale", layout.style ? `:root { ${layout.style} }` : "");

  const elements = new Map<string, HTMLElement>();
  for (const element of Array.from(editorHost.querySelectorAll<HTMLElement>("[data-block-id]"))) {
    const id = element.dataset.blockId;
    if (id && !elements.has(id)) elements.set(id, element);
  }
  for (const placement of layout.blocks) {
    const element = elements.get(placement.block_id);
    if (!element) continue;
    // The margin is what CSS margin collapsing will actually produce: it is
    // measured from the previous block's border box and is always larger than
    // the margin below it, so the collapse resolves to exactly this value.
    element.style.marginTop = placement.page_break_margin
      ? `calc(${placement.page_break_margin} + var(--page-gap))`
      : "";
    // Rust's page assignment, written onto the block it belongs to. Inert —
    // no structure changes, no selection mapping sees it, and `morphChildren`
    // strips it on the next render, after which this function puts it back —
    // but it is the decision itself, so anything that wants to know which
    // page a block is on (a status line, a test asserting that the rendered
    // box really is inside that page) reads it instead of guessing.
    element.dataset.pageIndex = String(placement.page);
  }

  const pages = Math.max(1, layout.page_count);
  flow.style.setProperty(
    "--page-flow-height",
    `calc(${pages} * var(--page-height) + ${pages - 1} * var(--page-gap))`,
  );
  drawSheets(sheets, pages);
}

/** Draws one sheet per page with its header and footer, resolving every page
 *  number field against the page it is actually on. */
function drawSheets(sheets: HTMLElement, pageCount: number): void {
  const header = state.doc?.header_html ?? "";
  const footer = state.doc?.footer_html ?? "";
  const drawn: HTMLElement[] = [];
  for (let page = 0; page < pageCount; page += 1) {
    const sheet = document.createElement("div");
    sheet.className = "page-sheet";
    sheet.dataset.pageNumber = String(page + 1);
    sheet.style.top = `calc(${page} * (var(--page-height) + var(--page-gap)))`;
    if (header) sheet.appendChild(furnitureBox("page-sheet-header", header, page + 1, pageCount));
    if (footer) sheet.appendChild(furnitureBox("page-sheet-footer", footer, page + 1, pageCount));
    drawn.push(sheet);
  }
  sheets.replaceChildren(...drawn);
}

/** One header or footer, with its fields resolved for this page.
 *
 *  The markup comes from Rust, rendered once; the *values* are filled in here
 *  because they depend on where the pages fell, which is not a document fact.
 *  An unknown field name is left alone rather than guessed at. */
function furnitureBox(className: string, html: string, page: number, pageCount: number): HTMLElement {
  const box = document.createElement("div");
  box.className = className;
  box.innerHTML = html;
  for (const field of Array.from(box.querySelectorAll<HTMLElement>("[data-field]"))) {
    if (field.dataset.field === "page-number") field.textContent = String(page);
    else if (field.dataset.field === "page-count") field.textContent = String(pageCount);
  }
  return box;
}

/** 1 inch = 1440 twips, the definition `opendoc_core::Length` is built on.
 *  The dialog talks inches because nobody sets a margin in twentieths of a
 *  point; the document still stores twips. */
const TWIPS_PER_INCH = 1440;
const CUSTOM_PAGE_SIZE = "custom";

function inchesLabel(twips: number): string {
  return (twips / TWIPS_PER_INCH).toFixed(2);
}

/** Reads a length back out of the dialog. Returns the original twips when the
 *  field was not touched, so opening the dialog and pressing OK cannot round a
 *  margin the user never typed — 2cm is 1134 twips, and 0.79in is not. */
function twipsFromField(value: string, original: number): number {
  if (value.trim() === inchesLabel(original)) return original;
  const inches = Number(value);
  if (!Number.isFinite(inches)) return original;
  return Math.round(inches * TWIPS_PER_INCH);
}

/** Page setup: paper size, orientation and margins.
 *
 *  The paper sizes come from Rust (`page_layout.size_presets`) so the frontend
 *  never hard-codes a sheet dimension, and orientation goes through its own
 *  command so the rotation stays a model operation rather than a swap done
 *  here. */
export async function promptPageSetup(): Promise<void> {
  const setup = state.doc?.page_setup;
  const layout = state.doc?.page_layout;
  if (!setup || !layout) return;
  const presets = layout.size_presets ?? [];
  const currentSize = layout.size_name ?? CUSTOM_PAGE_SIZE;
  const currentOrientation = layout.orientation ?? "portrait";
  const result = await promptDialog({
    title: "Page setup",
    fields: [
      {
        name: "size",
        label: "Paper size",
        type: "select",
        value: currentSize,
        options: [
          ...presets.map((preset) => ({ value: preset.name, label: preset.label })),
          { value: CUSTOM_PAGE_SIZE, label: "Custom" },
        ],
      },
      {
        name: "orientation",
        label: "Orientation",
        type: "select",
        value: currentOrientation,
        options: [
          { value: "portrait", label: "Portrait" },
          { value: "landscape", label: "Landscape" },
        ],
      },
      { name: "width", label: "Custom width (in)", type: "number", value: inchesLabel(setup.width_twips) },
      { name: "height", label: "Custom height (in)", type: "number", value: inchesLabel(setup.height_twips) },
      { name: "top", label: "Top margin (in)", type: "number", value: inchesLabel(setup.margin_top_twips) },
      { name: "bottom", label: "Bottom margin (in)", type: "number", value: inchesLabel(setup.margin_bottom_twips) },
      { name: "start", label: "Left margin (in)", type: "number", value: inchesLabel(setup.margin_start_twips) },
      { name: "end", label: "Right margin (in)", type: "number", value: inchesLabel(setup.margin_end_twips) },
    ],
    submit: "Apply",
  });
  if (!result) return;
  const preset = presets.find((candidate) => candidate.name === result.size);
  // A named size wins over the custom boxes; "Custom" falls back to them.
  const width = preset ? preset.width_twips : twipsFromField(result.width, setup.width_twips);
  const height = preset ? preset.height_twips : twipsFromField(result.height, setup.height_twips);
  await edit("set_page_setup", {
    widthTwips: width,
    heightTwips: height,
    marginTopTwips: twipsFromField(result.top, setup.margin_top_twips),
    marginBottomTwips: twipsFromField(result.bottom, setup.margin_bottom_twips),
    marginStartTwips: twipsFromField(result.start, setup.margin_start_twips),
    marginEndTwips: twipsFromField(result.end, setup.margin_end_twips),
  });
  // Rotating is Rust's job: the model derives orientation from the dimensions,
  // so swapping them here would be the same rule written twice.
  if (result.orientation !== currentOrientation) {
    await edit("set_page_orientation", { orientation: result.orientation });
  }
}

/** Header or footer: some text, optionally a page-number field, and an
 *  alignment. The field is inserted as a field — its value is resolved by
 *  pagination, never typed in here. */
export async function promptPageFurniture(slot: string): Promise<void> {
  if (slot !== "header" && slot !== "footer") return;
  const blocks = (slot === "header" ? state.doc?.header : state.doc?.footer) ?? [];
  const block = blocks[0];
  const current = {
    text: (block?.content ?? []).filter((inline) => inline.kind === "text").map((inline) => inline.text).join("").trim(),
    field: (block?.content ?? []).find((inline) => inline.kind === "page-number")?.target_id ?? "none",
    alignment: block?.properties?.alignment ?? "start",
  };
  const result = await promptDialog({
    title: slot === "header" ? "Header" : "Footer",
    fields: [
      { name: "text", label: "Text", value: current.text },
      {
        name: "field",
        label: "Page number",
        type: "select",
        value: current.field,
        options: [
          { value: "none", label: "None" },
          { value: "page-number", label: "Page number" },
          { value: "page-count", label: "Total pages" },
        ],
      },
      {
        name: "alignment",
        label: "Alignment",
        type: "select",
        value: current.alignment,
        options: [
          { value: "start", label: "Left" },
          { value: "center", label: "Centre" },
          { value: "end", label: "Right" },
        ],
      },
    ],
    submit: "Apply",
  });
  if (!result) return;
  if (!result.text.trim() && result.field === "none") {
    await edit("clear_page_furniture", { slot });
    return;
  }
  await edit("set_page_furniture", {
    slot,
    text: result.text,
    field: result.field,
    alignment: result.alignment,
  });
}
