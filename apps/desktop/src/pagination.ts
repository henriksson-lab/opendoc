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
import { APP_TWIPS_PER_INCH, type AppBlock } from "./generated/document";
import { editorHost, state } from "./state";
import { edit, query } from "./shared";

type FurnitureSlot = "header" | "footer" | "first-page-header" | "first-page-footer" | "even-page-header" | "even-page-footer";

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
  // The disc/circle/square and decimal/alpha/roman cycle, from the same
  // function the painted page picks its markers with. It was the last copy of
  // that cycle written by hand — in `styles.css`, three full turns of it —
  // and a copy is a second statement of where the cycle stops. See
  // `AppDocumentLayout.list_style`.
  applyStyleRule("doc-list-style", layout.list_style);

  applyPlacement(layout);

  const pages = Math.max(1, layout.page_count);
  flow.style.setProperty(
    "--page-flow-height",
    `calc(${pages} * var(--page-height) + ${pages - 1} * var(--page-gap))`,
  );
  drawSheets(sheets, pages);
  // Consumers that project document coordinates into viewport coordinates
  // (currently remote cursor presence) must re-read after page margins and
  // sheet geometry changed. The event keeps pagination independent of the
  // collaboration module and carries no document state.
  window.dispatchEvent(new window.Event("opendoc:document-layout"));
}

/**
 * Writes Rust's page assignment onto the blocks, for **every** block in the
 * host — the ones it names and the ones it does not.
 *
 * Being total is the point. `editor.ts` leaves a block's DOM untouched when
 * its markup did not change, so a placement it stopped needing would
 * otherwise stay on it for ever and show a page break where the break no
 * longer is. Written this way, a block's placement after this function is
 * exactly what the layout says, including the absence of one, whatever it was
 * before — which is the invariant that lets the renderer's output and this
 * decoration be applied independently.
 *
 * The two owners never share a CSS property either. The renderer projects the
 * model's space-before and space-after as the *logical* `margin-block-start`
 * and `margin-block-end`; the physical `margin-top` written here belongs to
 * pagination alone, is a later declaration in the same inline style so it
 * wins while it is set, and leaves the document's own spacing standing when
 * it is cleared. Before that split, applying a layout deleted every
 * space-before in the document: the renderer wrote `margin-top` too, and
 * clearing the placement cleared both.
 */
function applyPlacement(layout: AppDocumentLayout): void {
  const placements = new Map(layout.blocks.map((placement) => [placement.block_id, placement]));
  for (const element of Array.from(editorHost.querySelectorAll<HTMLElement>("[data-block-id]"))) {
    const placement = placements.get(element.dataset.blockId ?? "");
    // The margin is what CSS margin collapsing will actually produce: it is
    // measured from the previous block's border box and is always larger than
    // the margin below it, so the collapse resolves to exactly this value.
    const margin = placement?.page_break_margin
      ? `calc(${placement.page_break_margin} + var(--page-gap))`
      : "";
    // Assigning unconditionally would dirty the inline style of every block
    // in the document on every keystroke, which is the cost this whole path
    // exists to avoid.
    if (element.style.marginTop !== margin) element.style.marginTop = margin;
    if (!placement) {
      // Nested blocks (a table cell's paragraphs) are never placed, and a
      // block that has just stopped opening a page must lose the attribute
      // rather than keep a page number that is now someone else's.
      delete element.dataset.pageIndex;
      continue;
    }
    // Rust's page assignment, written onto the block it belongs to. Inert —
    // no structure changes and no selection mapping sees it — but it is the
    // decision itself, so anything that wants to know which page a block is
    // on (a status line, a test asserting that the rendered box really is
    // inside that page) reads it instead of guessing.
    const page = String(placement.page);
    if (element.dataset.pageIndex !== page) element.dataset.pageIndex = page;
  }
  // Positioned-image page-content anchors need the authoritative page index
  // just stamped above; block anchors are refreshed too in case pagination
  // moved their target to another sheet.
  state.editor?.positionPositionedImages();
}

/** What the sheet stack was last drawn from.
 *
 *  A sheet is a function of three things and nothing else: how many pages
 *  there are, and the header and footer markup Rust rendered. Typing changes
 *  none of them, so rebuilding the stack on every keystroke threw away and
 *  recreated one `<div>` per page — sixty of them on the document this was
 *  measured against — to arrive at the markup that was already there. The
 *  host element is part of the memo because `renderMain` makes a new one when
 *  it rebuilds the shell, and the new one is empty however unchanged the
 *  values are. */
let drawnSheets: { host: HTMLElement; pageCount: number; header: string; footer: string; firstHeader: string; firstFooter: string; evenHeader: string; evenFooter: string; hasFirstHeader: boolean; hasFirstFooter: boolean; hasEvenHeader: boolean; hasEvenFooter: boolean } | null = null;

/** Draws one sheet per page with its header and footer, resolving every page
 *  number field against the page it is actually on. */
function drawSheets(sheets: HTMLElement, pageCount: number): void {
  const header = state.doc?.header_html ?? "";
  const footer = state.doc?.footer_html ?? "";
  const firstHeader = state.doc?.first_page_header_html ?? "";
  const firstFooter = state.doc?.first_page_footer_html ?? "";
  const hasFirstHeader = state.doc?.first_page_header !== undefined;
  const hasFirstFooter = state.doc?.first_page_footer !== undefined;
  const evenHeader = state.doc?.even_page_header_html ?? "";
  const evenFooter = state.doc?.even_page_footer_html ?? "";
  const hasEvenHeader = state.doc?.even_page_header !== undefined;
  const hasEvenFooter = state.doc?.even_page_footer !== undefined;
  if (
    drawnSheets &&
    drawnSheets.host === sheets &&
    drawnSheets.pageCount === pageCount &&
    drawnSheets.header === header &&
    drawnSheets.footer === footer &&
    drawnSheets.firstHeader === firstHeader &&
    drawnSheets.firstFooter === firstFooter &&
    drawnSheets.hasFirstHeader === hasFirstHeader &&
    drawnSheets.hasFirstFooter === hasFirstFooter
    && drawnSheets.evenHeader === evenHeader
    && drawnSheets.evenFooter === evenFooter
    && drawnSheets.hasEvenHeader === hasEvenHeader
    && drawnSheets.hasEvenFooter === hasEvenFooter
  ) {
    return;
  }
  drawnSheets = { host: sheets, pageCount, header, footer, firstHeader, firstFooter, evenHeader, evenFooter, hasFirstHeader, hasFirstFooter, hasEvenHeader, hasEvenFooter };
  const drawn: HTMLElement[] = [];
  for (let page = 0; page < pageCount; page += 1) {
    const sheet = document.createElement("div");
    sheet.className = "page-sheet";
    sheet.dataset.pageNumber = String(page + 1);
    sheet.style.top = `calc(${page} * (var(--page-height) + var(--page-gap)))`;
    const pageHeader = page === 0 && hasFirstHeader ? firstHeader : page % 2 === 1 && hasEvenHeader ? evenHeader : header;
    const pageFooter = page === 0 && hasFirstFooter ? firstFooter : page % 2 === 1 && hasEvenFooter ? evenFooter : footer;
    if (pageHeader) sheet.appendChild(furnitureBox("page-sheet-header", pageHeader, page + 1, pageCount));
    if (pageFooter) sheet.appendChild(furnitureBox("page-sheet-footer", pageFooter, page + 1, pageCount));
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

/** The dialog talks inches because nobody sets a margin in twentieths of a
 *  point; the document still stores twips, and the factor between them is
 *  `opendoc_core::Length`'s own (`APP_TWIPS_PER_INCH`), never a copy. */
const CUSTOM_PAGE_SIZE = "custom";

function inchesLabel(twips: number): string {
  return (twips / APP_TWIPS_PER_INCH).toFixed(2);
}

/** Reads a length back out of the dialog. Returns the original twips when the
 *  field was not touched, so opening the dialog and pressing OK cannot round a
 *  margin the user never typed — 2cm is 1134 twips, and 0.79in is not. */
function twipsFromField(value: string, original: number): number {
  if (value.trim() === inchesLabel(original)) return original;
  const inches = Number(value);
  if (!Number.isFinite(inches)) return original;
  return Math.round(inches * APP_TWIPS_PER_INCH);
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

/**
 * Whether the legacy paragraph form can represent this furniture exactly.
 *
 * Page furniture itself is deliberately a `Block[]`: DOCX/ODT/Google import
 * can put rich marks, links, tables, images and differently formatted
 * paragraphs there, and the renderers/exporters retain those blocks.  The
 * small form below only speaks plain text plus one field and one alignment.
 * Treating a rich slot as text would be a destructive *edit just by saving
 * the dialog*, so it is important that this check is conservative.  A newer
 * rich furniture editor can relax it as it gains a lossless representation.
 */
function isPlainFurniture(blocks: AppBlock[]): boolean {
  let alignment: string | undefined;
  let pageFieldCount = 0;
  return blocks.every((block, blockIndex) => {
    if (block.kind !== "paragraph") return false;
    const properties = block.properties ?? {};
    // The plain form controls alignment and nothing else.  Do not erase an
    // imported indent, direction, spacing, or any future property it does
    // not know how to author.
    if (Object.keys(properties).some((key) => key !== "alignment")) return false;
    const blockAlignment = properties.alignment ?? "start";
    if (alignment === undefined) alignment = blockAlignment;
    if (alignment !== blockAlignment) return false;
    return block.content.every((inline, inlineIndex) => {
      if (inline.kind === "text") {
        // One textarea newline means a new paragraph. A soft break inside a
        // paragraph therefore needs the rich replacement route; accepting it
        // here would silently change the block structure on Apply.
        return !inline.href
          && (inline.marks?.length ?? 0) === 0
          && Object.keys(inline.mark_values ?? {}).length === 0
          && !inline.text.includes("\n");
      }
      if (inline.kind !== "page-number") return false;
      // The plain command represents at most one page field, at the end of
      // the final paragraph. Anything else is real structured source and
      // must be preserved until the user explicitly chooses rich replacement.
      pageFieldCount += 1;
      return pageFieldCount === 1
        && blockIndex === blocks.length - 1
        && inlineIndex === block.content.length - 1;
    });
  });
}

/** Whether an optional variant is explicitly present in the current projection.
 * This must be read at submit time as well as dialog-open time: a collaborator
 * can add or remove an override while the modal is open. */
function hasFurnitureOverride(slot: FurnitureSlot): boolean {
  switch (slot) {
    case "first-page-header": return state.doc?.first_page_header !== undefined;
    case "first-page-footer": return state.doc?.first_page_footer !== undefined;
    case "even-page-header": return state.doc?.even_page_header !== undefined;
    case "even-page-footer": return state.doc?.even_page_footer !== undefined;
    default: return false;
  }
}

/** Header or footer: one paragraph per line, optionally with a page-number
 * field on the final paragraph. The field is inserted as a field — its value
 * is resolved by pagination, never typed in here. */
export async function promptPageFurniture(slot: string): Promise<void> {
  if (!["header", "footer", "first-page-header", "first-page-footer", "even-page-header", "even-page-footer"].includes(slot)) return;
  const furnitureSlot = slot as FurnitureSlot;
  const isHeader = furnitureSlot === "header" || furnitureSlot === "first-page-header" || furnitureSlot === "even-page-header";
  const isFirstPage = furnitureSlot === "first-page-header" || furnitureSlot === "first-page-footer";
  const isEvenPage = furnitureSlot === "even-page-header" || furnitureSlot === "even-page-footer";
  const slotBlocks = furnitureSlot === "header" ? state.doc?.header : furnitureSlot === "footer" ? state.doc?.footer : furnitureSlot === "first-page-header" ? state.doc?.first_page_header : furnitureSlot === "first-page-footer" ? state.doc?.first_page_footer : furnitureSlot === "even-page-header" ? state.doc?.even_page_header : state.doc?.even_page_footer;
  const isOverride = isFirstPage || isEvenPage;
  // `undefined` means inherit; `[]` is a deliberately empty override. Do
  // not erase that distinction by coalescing before the dialog has captured
  // it, or applying an untouched inherited dialog could suppress a header.
  const hasOverride = isOverride && slotBlocks !== undefined;
  let blocks = slotBlocks ?? [];
  if (!isPlainFurniture(blocks)) {
    const choice = await promptDialog({
      title: isHeader ? `Rich ${isFirstPage ? "first-page " : isEvenPage ? "even-page " : ""}header` : `Rich ${isFirstPage ? "first-page " : isEvenPage ? "even-page " : ""}footer`,
      body: "This content has formatting or structure that the plain paragraph form cannot represent. It is preserved unchanged. The structured HTML route makes an intentional rich replacement through OpenDoc's safe HTML importer; it never saves this dialog as flattened text.",
      fields: [
        {
          name: "action",
          label: "Edit",
          type: "select",
          value: "keep",
          options: [
            { value: "keep", label: "Keep rich content" },
            ...(isOverride && hasOverride
              ? [{ value: "inherit", label: "Inherit ordinary header/footer" }]
              : []),
            { value: "rich-html", label: "Replace from structured HTML…" },
            { value: "replace", label: "Replace with plain content…" },
          ],
        },
      ],
      submit: "Continue",
    });
    if (!choice || choice.action === "keep") return;
    if (choice.action === "inherit") {
      await edit("clear_page_furniture_override", { slot: furnitureSlot });
      return;
    }
    if (choice.action === "rich-html") {
      await promptRichFurnitureHtml(furnitureSlot);
      return;
    }
    // This is an intentional replacement, not an accidental conversion of
    // source blocks. Start empty rather than presenting a misleading lossy
    // transcription of them.
    blocks = [];
  }
  const block = blocks[0];
  const current = {
    // One textarea line is one paragraph. This both exposes imported
    // multi-paragraph furniture and lets a user create it without pretending
    // that a header/footer is one flattened string.
    // Textarea whitespace is source text. In particular, do not trim a
    // running head merely because a user opens its dialog; an Apply must
    // reproduce every representable paragraph byte-for-byte.
    text: blocks.map((paragraph) => (paragraph.content ?? []).filter((inline) => inline.kind === "text").map((inline) => inline.text).join("")).join("\n"),
    field: blocks.flatMap((paragraph) => paragraph.content ?? []).find((inline) => inline.kind === "page-number")?.target_id ?? "none",
    alignment: block?.properties?.alignment ?? "start",
  };
  const result = await promptDialog({
    title: isHeader ? `${isFirstPage ? "First-page " : isEvenPage ? "Even-page " : ""}header` : `${isFirstPage ? "First-page " : isEvenPage ? "Even-page " : ""}footer`,
    fields: [
      { name: "text", label: "Paragraphs (one per line)", type: "textarea", value: current.text },
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
      ...(isOverride
        ? [{
            name: "variantMode",
            label: "Variant",
            type: "select" as const,
            value: hasOverride ? "override" : "inherit",
            options: [
              { value: "inherit", label: "Inherit ordinary header/footer" },
              { value: "override", label: "Use this variant's content" },
            ],
          }]
        : []),
    ],
    submit: "Apply",
  });
  if (!result) return;
  if (isOverride && result.variantMode === "inherit") {
    // A missing override already inherits, so treating Apply as a no-op avoids
    // creating a semantically different empty override. Read the current
    // projection here, not the dialog-open snapshot: otherwise a remote
    // override added while this dialog was open survives a user's explicit
    // request to inherit. A present override is removed through its own
    // operation so undo restores it exactly.
    if (hasFurnitureOverride(furnitureSlot)) await edit("clear_page_furniture_override", { slot: furnitureSlot });
    return;
  }
  if (!result.text.trim() && result.field === "none") {
    await edit("clear_page_furniture", { slot: furnitureSlot });
    return;
  }
  await edit("set_page_furniture", {
    slot: furnitureSlot,
    text: result.text,
    field: result.field,
    alignment: result.alignment,
  });
}

/**
 * A deliberate rich-furniture authoring route.  The renderer's current HTML
 * is offered as a useful starting point, but submitting it is expressly a
 * replacement: unlike the plain dialog, nothing here claims to be a
 * lossless editor for future model features. Rust accepts only its closed
 * HTML subset and rejects a fragment that would degrade an embedded object.
 */
async function promptRichFurnitureHtml(slot: FurnitureSlot): Promise<void> {
  const current = slot === "header" ? state.doc?.header_html ?? "" : slot === "footer" ? state.doc?.footer_html ?? "" : slot === "first-page-header" ? state.doc?.first_page_header_html ?? "" : slot === "first-page-footer" ? state.doc?.first_page_footer_html ?? "" : slot === "even-page-header" ? state.doc?.even_page_header_html ?? "" : state.doc?.even_page_footer_html ?? "";
  const result = await promptDialog({
    title: slot.endsWith("header") ? "Structured header HTML" : "Structured footer HTML",
    body: "Use paragraphs, headings, lists, links, marks, a standalone table, and raster data images. This intentionally replaces the whole slot. Unsupported embedded objects and mixed table fragments are rejected and leave the existing header/footer unchanged.",
    fields: [
      {
        name: "html",
        label: "Safe HTML fragment",
        type: "textarea",
        value: current,
      },
    ],
    submit: "Replace rich content",
  });
  if (!result) return;
  await edit("set_page_furniture_html", { slot, html: result.html });
}
