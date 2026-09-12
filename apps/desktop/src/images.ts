// Images: insertion, sizing and placement.
//
// The document surface is re-rendered through `morphChildren`, which deletes
// any element the renderer did not produce, so — exactly like the grid's
// row/column resize — there are **no handle elements**. The hit zone is
// computed from the pointer position against the picture's own box, and the
// drag preview is written to the very `style` attribute `opendoc-render`
// emits, which the next morph overwrites or removes on its own.
//
// The two listeners are assignments on the module-level `editorHost`, which
// outlives every rebuild, so re-rendering cannot stack them; the drag
// listeners are added on mousedown and removed on mouseup.
import { promptDialog } from "./ui";
import type { AppBlock } from "./types";
import { editorHost, state } from "./state";
import { edit, focusBlock, query, requireBlock, run, showError } from "./shared";

/** How close to the picture's trailing edge the pointer starts a resize. */
const IMAGE_HANDLE_PX = 8;
/** Smallest picture worth having: a quarter inch. */
const IMAGE_MIN_TWIPS = 360;
/** The model refuses anything past 22in, so the drag stops there too. */
const IMAGE_MAX_TWIPS = 22 * 1440;

type ImageEdge = "east" | "south" | "corner";
type ImageTarget = { blockId: string; box: HTMLElement; edge: ImageEdge };

type ImageResize = ImageTarget & {
  originX: number;
  originY: number;
  baseWidth: number;
  baseHeight: number;
  /** What the renderer had put on the box, so Escape can restore it exactly. */
  style: string;
  startWidth: string;
  startHeight: string;
  width: number;
  height: number;
};

let imageResize: ImageResize | null = null;

/**
 * Twips per CSS pixel, measured rather than assumed.
 *
 * The page box is `--page-width` wide and the model says how many twips that
 * is, so dividing one by the other yields a factor that is already correct
 * under the page-stack zoom and whatever the device pixel ratio is. The
 * fallback is the definitional 1pt = 4/3px for the moment before the page has
 * been laid out.
 */
function twipsPerPixel(): number {
  const page = query("[data-page]");
  const pageWidth = state.doc?.page_setup?.width_twips;
  if (page && pageWidth) {
    const rect = page.getBoundingClientRect();
    if (rect.width > 0) return pageWidth / rect.width;
  }
  return 15;
}

function clampImageTwips(twips: number): number {
  return Math.min(IMAGE_MAX_TWIPS, Math.max(IMAGE_MIN_TWIPS, Math.round(twips)));
}

/** Twips as the length `opendoc-render` would emit, so a preview and a commit
 *  draw the same box. */
function twipsToPt(twips: number): string {
  return `${twips / 20}pt`;
}

/** The image the pointer would resize, when it sits on a trailing edge. */
function imageTargetAt(event: MouseEvent): ImageTarget | null {
  const target = event.target as Element | null;
  const figure = target?.closest<HTMLElement>("figure.doc-image");
  const blockId = figure?.getAttribute("data-block-id");
  if (!figure || !blockId || !editorHost.contains(figure)) return null;
  const box = figure.querySelector<HTMLElement>("img, .doc-image-placeholder");
  if (!box) return null;
  const rect = box.getBoundingClientRect();
  if (rect.width <= 0 || rect.height <= 0) return null;
  const withinX = event.clientX >= rect.left && event.clientX <= rect.right + IMAGE_HANDLE_PX;
  const withinY = event.clientY >= rect.top && event.clientY <= rect.bottom + IMAGE_HANDLE_PX;
  if (!withinX || !withinY) return null;
  const east = event.clientX >= rect.right - IMAGE_HANDLE_PX;
  const south = event.clientY >= rect.bottom - IMAGE_HANDLE_PX;
  if (east && south) return { blockId, box, edge: "corner" };
  if (east) return { blockId, box, edge: "east" };
  if (south) return { blockId, box, edge: "south" };
  return null;
}

/**
 * Paints a size onto the live picture. The morph after the command either
 * confirms it or replaces it with what the document actually stored.
 *
 * The axis that is not being dragged keeps whatever the renderer had on it —
 * `auto` when the document states no size for it, which is what makes a
 * one-axis drag scale rather than stretch.
 */
function previewImageSize(resize: ImageResize): void {
  const { box, edge } = resize;
  box.style.width = edge === "south" ? resize.startWidth || "auto" : twipsToPt(resize.width);
  box.style.height = edge === "east" ? resize.startHeight || "auto" : twipsToPt(resize.height);
}

function startImageResize(event: MouseEvent, target: ImageTarget): void {
  event.preventDefault();
  const factor = twipsPerPixel();
  const rect = target.box.getBoundingClientRect();
  const baseWidth = clampImageTwips(rect.width * factor);
  const baseHeight = clampImageTwips(rect.height * factor);
  imageResize = {
    ...target,
    originX: event.clientX,
    originY: event.clientY,
    baseWidth,
    baseHeight,
    // Captured so Escape can put back exactly what the renderer emitted,
    // including the case where it emitted no style at all.
    style: target.box.getAttribute("style") ?? "",
    startWidth: target.box.style.width,
    startHeight: target.box.style.height,
    width: baseWidth,
    height: baseHeight,
  };
  window.addEventListener("mousemove", onImageResizeMove);
  window.addEventListener("mouseup", onImageResizeEnd);
  window.addEventListener("keydown", onImageResizeKey);
}

function onImageResizeMove(event: MouseEvent): void {
  const resize = imageResize;
  if (!resize) return;
  const factor = twipsPerPixel();
  const width = clampImageTwips(resize.baseWidth + (event.clientX - resize.originX) * factor);
  const height = clampImageTwips(resize.baseHeight + (event.clientY - resize.originY) * factor);
  if (resize.edge === "corner") {
    // A corner drag keeps the aspect ratio: the width leads, because that is
    // the axis a page actually constrains.
    resize.width = width;
    resize.height = clampImageTwips((width * resize.baseHeight) / resize.baseWidth);
  } else if (resize.edge === "east") {
    resize.width = width;
  } else {
    resize.height = height;
  }
  previewImageSize(resize);
}

function onImageResizeKey(event: KeyboardEvent): void {
  if (event.key === "Escape") finishImageResize(true);
}

function onImageResizeEnd(): void {
  finishImageResize(false);
}

/** Ends a drag with exactly one command — never one per mousemove, because
 *  each one is an undoable, signature-clearing operation. */
function finishImageResize(cancelled: boolean): void {
  const resize = imageResize;
  imageResize = null;
  window.removeEventListener("mousemove", onImageResizeMove);
  window.removeEventListener("mouseup", onImageResizeEnd);
  window.removeEventListener("keydown", onImageResizeKey);
  if (!resize) return;
  const unchanged = resize.width === resize.baseWidth && resize.height === resize.baseHeight;
  if (cancelled || unchanged) {
    if (resize.style) resize.box.setAttribute("style", resize.style);
    else resize.box.removeAttribute("style");
    return;
  }
  // The preview stays on screen until the command comes back and the morph
  // either confirms it or corrects it.
  if (resize.edge === "east") void edit("set_image_block_width", { blockId: resize.blockId, twips: resize.width });
  else if (resize.edge === "south") void edit("set_image_block_height", { blockId: resize.blockId, twips: resize.height });
  else void edit("set_image_block_size", { blockId: resize.blockId, widthTwips: resize.width, heightTwips: resize.height });
}

// Assignments, not addEventListener: re-rendering the shell can only ever
// replace these, never add a second copy. This is the bug class that produced
// four user-visible duplicates in one day.
editorHost.onmousedown = (event: MouseEvent) => {
  const target = imageTargetAt(event);
  if (target) startImageResize(event, target);
};
editorHost.onmousemove = (event: MouseEvent) => {
  if (imageResize) return;
  const edge = imageTargetAt(event)?.edge ?? null;
  editorHost.classList.toggle("resize-image-east", edge === "east");
  editorHost.classList.toggle("resize-image-south", edge === "south");
  editorHost.classList.toggle("resize-image-corner", edge === "corner");
};
editorHost.onmouseleave = () => {
  if (imageResize) return;
  editorHost.classList.remove("resize-image-east", "resize-image-south", "resize-image-corner");
};

/** The image block the caret is on, with a message when it is not on one. */
export function focusImageBlock(): AppBlock | null {
  const block = focusBlock();
  if (block?.kind === "image") return block;
  showError("Select an image first.");
  return null;
}

/** Menu path: exact sizes in points, prefilled with what the document says. */
export async function promptImageSize(): Promise<void> {
  const block = focusImageBlock();
  if (!block) return;
  const points = (twips: number | null | undefined) => (twips == null ? "" : String(twips / 20));
  const result = await promptDialog({
    title: "Image size",
    fields: [
      { name: "width", label: "Width (pt)", type: "number", value: points(block.image_width_twips), placeholder: "auto" },
      { name: "height", label: "Height (pt)", type: "number", value: points(block.image_height_twips), placeholder: "auto" },
    ],
    submit: "Apply",
  });
  if (!result) return;
  const width = result.width.trim() === "" ? null : Math.round(Number(result.width) * 20);
  const height = result.height.trim() === "" ? null : Math.round(Number(result.height) * 20);
  if (width === null && height === null) {
    await edit("clear_image_block_size", { blockId: block.id });
    return;
  }
  if (width !== null && height !== null) {
    await edit("set_image_block_size", { blockId: block.id, widthTwips: width, heightTwips: height });
    return;
  }
  if (width !== null) await edit("set_image_block_width", { blockId: block.id, twips: width });
  else if (height !== null) await edit("set_image_block_height", { blockId: block.id, twips: height });
}

/**
 * Turns dropped or pasted image files into image blocks.
 *
 * The bytes go through the same two commands the Insert ▸ Image… path uses —
 * `add_binary_blob` then `insert_image_block_after` — so a dropped picture is
 * content-addressed, deduplicated and signable exactly like an attached one.
 * The gesture is new; the pipeline is not.
 */
export async function insertImageFiles(files: File[], afterBlockId: string | null): Promise<void> {
  if (files.length === 0) return;
  let after = afterBlockId ?? requireBlock()?.id ?? null;
  if (!after) return;
  for (const file of files) {
    try {
      const name = file.name || "pasted image";
      const bytes = Array.from(new Uint8Array(await file.arrayBuffer()));
      // Which hash the bytes have is Rust's answer, not one recomputed here:
      // whatever appeared in the blob list is the blob that was just written.
      const known = new Set((state.doc?.blobs ?? []).map((item) => item.hash));
      const updated = await run("add_binary_blob", { name, mediaType: file.type || "application/octet-stream", bytes });
      // Blobs are content-addressed, so re-dropping the same picture adds
      // nothing new and the existing entry is the right one to point at.
      const blob =
        updated.blobs.find((item) => !known.has(item.hash)) ??
        [...updated.blobs].reverse().find((item) => item.size === bytes.length && item.name === name);
      if (!blob) return;
      await edit("insert_image_block_after", { afterBlockId: after, blobHash: blob.hash, altText: name });
      // Each picture lands after the previous one, so dropping several keeps
      // the order they were dropped in.
      const inserted = [...(state.doc?.blocks ?? [])].reverse().find((block) => block.kind === "image" && block.blob_hash === blob.hash);
      after = inserted?.id ?? after;
    } catch {
      // reported by run()
    }
  }
}
