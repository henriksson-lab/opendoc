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
import { openFile } from "./invoke";
import { downloadExport } from "./files";
import type { AppBlock } from "./types";
import {
  APP_MAX_IMAGE_TWIPS,
  APP_MIN_IMAGE_TWIPS,
  APP_TWIPS_PER_POINT,
} from "./generated/document";
import { editorHost, state } from "./state";
import { bytesFromBase64, edit, findBlock, focusBlock, native, query, requireBlock, run, showError } from "./shared";

/** How close to the picture's trailing edge the pointer starts a resize. */
const IMAGE_HANDLE_PX = 8;
const IMAGE_BORDER_STYLES = [
  { value: "none", label: "None" },
  { value: "solid", label: "Solid" },
  { value: "dashed", label: "Dashed" },
  { value: "dotted", label: "Dotted" },
  { value: "double", label: "Double" },
];

type ImageEdge = "east" | "south" | "corner";
type ImageTarget = { blockId: string; box: HTMLElement; edge: ImageEdge };
type ImagePositionAnchorOption = { value: string; label: string };

function imageAnchorLabel(candidate: AppBlock): string {
  const text = candidate.content.map((inline) => inline.text ?? "").join("").trim();
  return text ? text.slice(0, 56) : candidate.kind;
}

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
 * The keyboard resize increment.  One point is small enough for an ordinary
 * adjustment; holding Alt makes a larger ten-point move without introducing
 * a second, DOM-only size state.
 */
const KEYBOARD_RESIZE_STEP_TWIPS = APP_TWIPS_PER_POINT;

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

/** The drag stops exactly where the model's own bounds do, so a gesture can
 *  never produce a size the command would then refuse. */
function clampImageTwips(twips: number): number {
  return Math.min(APP_MAX_IMAGE_TWIPS, Math.max(APP_MIN_IMAGE_TWIPS, Math.round(twips)));
}

/** Twips as the length `opendoc-render` would emit, so a preview and a commit
 *  draw the same box. */
function twipsToPt(twips: number): string {
  return `${twips / APP_TWIPS_PER_POINT}pt`;
}

/** The rendered box is the authority when the document deliberately leaves
 * one or both dimensions automatic. */
function renderedImageTwips(box: HTMLElement, axis: "width" | "height"): number | null {
  const pixels = box.getBoundingClientRect()[axis];
  return pixels > 0 ? clampImageTwips(pixels * twipsPerPixel()) : null;
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

/**
 * Resize the focused image with Ctrl+Shift+Arrow.
 *
 * A figure is already an atomic, tab-focusable document object.  This handler
 * deliberately reads both its block id and current box from that object, then
 * commits one ordinary model command.  It does not add persistent resize
 * handles or remember a browser-only dimension, so keyboard, pointer, undo,
 * collaboration and export all share the same source of truth.
 *
 * Alt changes the increment from 1pt to 10pt.  Plain arrows remain document
 * navigation, and View mode remains non-mutating.
 */
export async function resizeFocusedImageFromKeyboard(event: KeyboardEvent): Promise<boolean> {
  if (!event.ctrlKey || !event.shiftKey || event.metaKey || state.documentEditingMode === "view") return false;
  const direction = event.key;
  if (!(["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"] as string[]).includes(direction)) return false;

  const focused = document.activeElement instanceof Element ? document.activeElement : null;
  const figure = focused?.closest<HTMLElement>("figure.doc-image");
  const blockId = figure?.getAttribute("data-block-id");
  if (!figure || !blockId || !editorHost.contains(figure)) return false;
  const block = findBlock(blockId);
  const box = figure.querySelector<HTMLElement>("img, .doc-image-placeholder");
  if (!block || block.kind !== "image" || !box) return false;

  const step = KEYBOARD_RESIZE_STEP_TWIPS * (event.altKey ? 10 : 1);
  if (direction === "ArrowLeft" || direction === "ArrowRight") {
    const current = block.image_width_twips ?? renderedImageTwips(box, "width");
    if (current === null) return false;
    const next = clampImageTwips(current + (direction === "ArrowRight" ? step : -step));
    if (next !== current) await edit("set_image_block_width", { blockId, twips: next });
    return true;
  }
  const current = block.image_height_twips ?? renderedImageTwips(box, "height");
  if (current === null) return false;
  const next = clampImageTwips(current + (direction === "ArrowDown" ? step : -step));
  if (next !== current) await edit("set_image_block_height", { blockId, twips: next });
  return true;
}

/**
 * Cancel an atomic image selection without turning Escape into a hidden model
 * edit. A selected figure is represented by a DOM range over the object; if
 * that range survives blur, Format > Image can still act on an object the
 * keyboard user has explicitly left. Clearing it is the only honest
 * deselection: images have no caret position inside them.
 */
export function clearFocusedImageSelection(): boolean {
  const focused = document.activeElement instanceof Element ? document.activeElement : null;
  const figure = focused?.closest<HTMLElement>("figure.doc-image");
  if (!figure || !editorHost.contains(figure)) return false;
  document.getSelection()?.removeAllRanges();
  state.selection = null;
  figure.blur();
  return true;
}

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
  const points = (twips: number | null | undefined) => (twips == null ? "" : String(twips / APP_TWIPS_PER_POINT));
  const result = await promptDialog({
    title: "Image size",
    fields: [
      { name: "width", label: "Width (pt)", type: "number", value: points(block.image_width_twips), placeholder: "auto" },
      { name: "height", label: "Height (pt)", type: "number", value: points(block.image_height_twips), placeholder: "auto" },
    ],
    submit: "Apply",
  });
  if (!result) return;
  // The dialog belongs to the image selected when it was opened. A remote
  // projection may delete that block (or replace it with another block kind)
  // before Apply; do not turn the saved id into an edit for stale content.
  const live = findBlock(block.id);
  if (live?.kind !== "image") {
    showError("This image was deleted or changed while its size dialog was open. Select an image and try again.");
    return;
  }
  const width = result.width.trim() === "" ? null : Math.round(Number(result.width) * APP_TWIPS_PER_POINT);
  const height = result.height.trim() === "" ? null : Math.round(Number(result.height) * APP_TWIPS_PER_POINT);
  if (width === null && height === null) {
    await edit("clear_image_block_size", { blockId: live.id });
    return;
  }
  if (width !== null && height !== null) {
    await edit("set_image_block_size", { blockId: live.id, widthTwips: width, heightTwips: height });
    return;
  }
  if (width !== null) await edit("set_image_block_width", { blockId: live.id, twips: width });
  else if (height !== null) await edit("set_image_block_height", { blockId: live.id, twips: height });
}

/**
 * Make the selected image out-of-flow using the durable ADR 0022 geometry.
 *
 * The anchor picker deliberately lists document blocks by their visible text
 * rather than exposing an opaque stable id as the primary UI.  Page content
 * is always available, and the image itself is excluded because the command
 * rejects self anchors.  This is an OpenDoc layout control, not a promise
 * that DOCX, ODT, or Google exports can retain their native drawing models.
 */
export async function promptImagePosition(): Promise<void> {
  const block = focusImageBlock();
  if (!block) return;
  const positioned = block.image_positioned;
  const anchoredBlock = typeof positioned?.anchor === "object" ? positioned.anchor.Block : "";
  const anchors = positionAnchorOptions(state.doc?.blocks ?? [], block.id, anchoredBlock);
  const points = (twips: number | undefined) => String((twips ?? 0) / APP_TWIPS_PER_POINT);
  const result = await promptDialog({
    title: "Position image",
    body: "Positioning is an OpenDoc layout setting. Exports may use an in-flow fallback when their native format cannot represent it.",
    fields: [
      { name: "anchor", label: "Anchor", type: "select", value: anchoredBlock, options: anchors },
      {
        name: "horizontal", label: "Horizontal offset (pt)", type: "number", step: "0.05",
        value: points(positioned?.horizontal_offset),
      },
      {
        name: "vertical", label: "Vertical offset (pt)", type: "number", step: "0.05",
        value: points(positioned?.vertical_offset),
      },
      {
        name: "layer", label: "Layer", type: "select",
        value: positioned?.layer === "InFrontOfText" ? "in-front-of-text" : "behind-text",
        options: [
          { value: "behind-text", label: "Behind text" },
          { value: "in-front-of-text", label: "In front of text" },
        ],
      },
    ],
    submit: "Apply",
  });
  if (!result) return;
  const horizontalOffsetTwips = Math.round(Number(result.horizontal) * APP_TWIPS_PER_POINT);
  const verticalOffsetTwips = Math.round(Number(result.vertical) * APP_TWIPS_PER_POINT);
  if (!Number.isSafeInteger(horizontalOffsetTwips) || !Number.isSafeInteger(verticalOffsetTwips)) {
    showError("Image offsets must be finite point measurements.");
    return;
  }
  // A remote replacement can remove the selected atomic object while its
  // position dialog is open. The dialog's ids describe the image the reader
  // opened, not an instruction to apply geometry to whatever now owns focus.
  const live = findBlock(block.id);
  if (live?.kind !== "image") {
    showError("This image was deleted or changed while its position dialog was open. Select an image and try again.");
    return;
  }
  await edit("set_image_block_positioned", {
    blockId: live.id,
    anchorBlockId: result.anchor || null,
    horizontalOffsetTwips,
    verticalOffsetTwips,
    layer: result.layer,
  });
}

/**
 * The dialog may edit a document that has converged after its image was last
 * positioned. A deleted or nested target is intentionally not a selectable
 * visual anchor, but it is still durable source state: omitting it makes a
 * browser `<select>` substitute the first (page-content) value on Apply.
 */
export function positionAnchorOptions(
  blocks: AppBlock[],
  imageBlockId: string,
  anchoredBlockId: string,
): ImagePositionAnchorOption[] {
  const options: ImagePositionAnchorOption[] = [
    { value: "", label: "Page content" },
    ...blocks
      .filter((candidate) => candidate.id !== imageBlockId)
      .map((candidate) => ({ value: candidate.id, label: `Block: ${imageAnchorLabel(candidate)}` })),
  ];
  if (anchoredBlockId && !options.some((option) => option.value === anchoredBlockId)) {
    options.splice(1, 0, {
      value: anchoredBlockId,
      label: "Unavailable block anchor (preserved; page-content fallback)",
    });
  }
  return options;
}

/** Return the selected out-of-flow image to ordinary document flow. */
export async function clearImagePosition(): Promise<void> {
  const block = focusImageBlock();
  if (block) await edit("clear_image_block_positioned", { blockId: block.id });
}

/** Save the selected image's source bytes without decoding or re-encoding it.
 * A PNG remains a PNG, a JPEG remains a JPEG, and formats we do not edit yet
 * (such as SVG) stay usable rather than becoming a lossy surprise. */
export async function saveFocusedImage(): Promise<void> {
  const block = focusImageBlock();
  if (!block?.blob_hash) return;
  const blob = state.doc?.blobs.find((candidate) => candidate.hash === block.blob_hash);
  // `defaultName` reaches a native save dialog. Blob names normally come from
  // a file picker, but imports can supply metadata, so keep separators and
  // control characters out of its suggested path.
  const baseName =
    (blob?.name || "image")
      .replace(/[\\/:\0-\x1f]/g, "-")
      .replace(/\.[^.]+$/, "")
      .trim() || "image";
  await downloadExport("export_image_blob", "original image", {
    args: { blobHash: block.blob_hash },
    defaultBaseName: baseName,
  });
}

/** Edit image-specific metadata and optionally replace only its source blob.
 * Replacing does not mutate the old asset: other image blocks can still point
 * at it, while this block receives one ordinary, undoable blob-reference
 * operation. */
export async function promptImageProperties(): Promise<void> {
  const block = focusImageBlock();
  if (!block?.blob_hash) return;
  // Blob names are source/attachment metadata. They are deliberately kept
  // outside the image's accessible name, but remain inspectable here and in
  // the Files panel so a person can identify what will be saved unchanged.
  const sourceName = state.doc?.blobs.find((blob) => blob.hash === block.blob_hash)?.name ?? "Unavailable source";
  const result = await promptDialog({
    title: "Image properties",
    body: "The source file name is attachment metadata. It is not shown as a caption or used as alternative text.",
    fields: [
      {
        name: "sourceName",
        label: "Source file",
        value: sourceName,
        readonly: true,
      },
      {
        name: "altText",
        label: "Alternative text",
        type: "textarea",
        value: block.alt_text ?? "",
        placeholder: "Describe it for readers who cannot see it; leave blank when decorative",
      },
      {
        name: "caption",
        label: "Caption",
        type: "textarea",
        value: block.image_caption ?? "",
        placeholder: "Visible text below the image (optional)",
      },
      {
        name: "wrapTop",
        label: "Wrap clearance top (pt)",
        type: "number",
        value: String((block.image_wrap_clearance?.top ?? 0) / APP_TWIPS_PER_POINT),
      },
      {
        name: "wrapEnd",
        label: "Wrap clearance end (pt)",
        type: "number",
        value: String((block.image_wrap_clearance?.end ?? 0) / APP_TWIPS_PER_POINT),
      },
      {
        name: "wrapBottom",
        label: "Wrap clearance bottom (pt)",
        type: "number",
        value: String((block.image_wrap_clearance?.bottom ?? 0) / APP_TWIPS_PER_POINT),
      },
      {
        name: "wrapStart",
        label: "Wrap clearance start (pt)",
        type: "number",
        value: String((block.image_wrap_clearance?.start ?? 0) / APP_TWIPS_PER_POINT),
      },
      {
        name: "borderStyle",
        label: "Border style",
        type: "select",
        value: block.image_border?.style ?? "none",
        options: IMAGE_BORDER_STYLES,
      },
      {
        name: "borderPoints",
        label: "Border thickness (points)",
        type: "number",
        step: "0.5",
        value: String((block.image_border?.twips ?? 20) / APP_TWIPS_PER_POINT),
      },
      {
        name: "borderColor",
        label: "Border colour",
        type: "color",
        value: block.image_border?.color ?? "#000000",
      },
      {
        name: "rotation",
        label: "Rotation (degrees)",
        type: "number",
        value: String(block.image_rotation_degrees ?? 0),
      },
      {
        name: "opacity",
        label: "Opacity (%)",
        type: "number",
        value: String(block.image_opacity_percent ?? 100),
      },
      {
        name: "cropTop",
        label: "Crop top (%)",
        type: "number",
        value: String(block.image_crop_top_percent ?? 0),
      },
      {
        name: "cropRight",
        label: "Crop right (%)",
        type: "number",
        value: String(block.image_crop_right_percent ?? 0),
      },
      {
        name: "cropBottom",
        label: "Crop bottom (%)",
        type: "number",
        value: String(block.image_crop_bottom_percent ?? 0),
      },
      {
        name: "cropLeft",
        label: "Crop left (%)",
        type: "number",
        value: String(block.image_crop_left_percent ?? 0),
      },
    ],
    submit: "Save",
  });
  if (!result) return;
  const rotation = Number(result.rotation);
  const opacity = Number(result.opacity);
  const crop = [
    result.cropTop,
    result.cropRight,
    result.cropBottom,
    result.cropLeft,
  ].map(Number);
  const borderPoints = Number(result.borderPoints);
  const wrapClearance = [result.wrapTop, result.wrapEnd, result.wrapBottom, result.wrapStart].map(Number);
  if (!Number.isInteger(rotation) || rotation < -360 || rotation > 360) {
    showError("Rotation must be a whole number from -360 to 360.");
    return;
  }
  if (!Number.isInteger(opacity) || opacity < 0 || opacity > 100) {
    showError("Opacity must be a whole percentage from 0 to 100.");
    return;
  }
  if (
    !crop.every((value) => Number.isInteger(value) && value >= 0 && value <= 100) ||
    crop[0] + crop[2] >= 100 ||
    crop[1] + crop[3] >= 100
  ) {
    showError("Crop values must be 0–100%, leaving some width and height visible.");
    return;
  }
  if (!Number.isFinite(borderPoints) || borderPoints < 0 || borderPoints > 6) {
    showError("Border thickness must be between 0 and 6 points.");
    return;
  }
  if (!wrapClearance.every((value) => Number.isFinite(value) && value >= 0 && value <= 1584)) {
    showError("Wrap clearance must be between 0 and 1584 points.");
    return;
  }
  // Validate every field before emitting an operation. In particular, an
  // invalid crop must not leave an updated alt text behind as a partial save.
  if (result.altText !== (block.alt_text ?? "")) {
    await edit("update_image_alt_text", { blockId: block.id, altText: result.altText });
  }
  if (result.caption !== (block.image_caption ?? "")) {
    await edit("set_image_block_caption", { blockId: block.id, caption: result.caption });
  }
  const wrapTwips = wrapClearance.map((value) => Math.round(value * APP_TWIPS_PER_POINT));
  const currentWrap = block.image_wrap_clearance;
  if (
    wrapTwips.some((value, index) => value !== [currentWrap?.top ?? 0, currentWrap?.end ?? 0, currentWrap?.bottom ?? 0, currentWrap?.start ?? 0][index])
  ) {
    if (block.image_placement !== "wrap-start" && block.image_placement !== "wrap-end") {
      showError("Choose a Wrap text placement before setting image wrap clearance.");
      return;
    }
    await edit("set_image_block_wrap_clearance", {
      blockId: block.id,
      topTwips: wrapTwips[0],
      endTwips: wrapTwips[1],
      bottomTwips: wrapTwips[2],
      startTwips: wrapTwips[3],
    });
  }
  const borderTwips = Math.round(borderPoints * APP_TWIPS_PER_POINT);
  if (
    result.borderStyle !== (block.image_border?.style ?? "none") ||
    (result.borderStyle !== "none" &&
      (borderTwips !== (block.image_border?.twips ?? 20) ||
        result.borderColor !== (block.image_border?.color ?? "#000000")))
  ) {
    await edit("set_image_block_border", {
      blockId: block.id,
      style: result.borderStyle,
      twips: borderTwips,
      color: result.borderColor,
    });
  }
  await edit("set_image_block_effects", {
    blockId: block.id,
    rotationDegrees: rotation,
    opacityPercent: opacity,
  });
  await edit("set_image_block_crop", {
    blockId: block.id,
    topPercent: crop[0],
    rightPercent: crop[1],
    bottomPercent: crop[2],
    leftPercent: crop[3],
  });
}

/** Replace the selected image while retaining its document position, layout,
 * and accessibility metadata. */
export async function replaceFocusedImage(): Promise<void> {
  const block = focusImageBlock();
  if (!block?.blob_hash) return;
  const file = await native("Could not open that image", () =>
    openFile(["png", "jpg", "jpeg", "gif", "webp", "svg", "bmp", "tif", "tiff"]),
  );
  if (!file) return;
  const known = new Set((state.doc?.blobs ?? []).map((blob) => blob.hash));
  const updated = await run("add_binary_blob", {
    name: file.name,
    mediaType: file.media_type,
    bytes: bytesFromBase64(file.base64),
  });
  const replacement =
    updated.blobs.find((blob) => !known.has(blob.hash)) ??
    updated.blobs.find((blob) => blob.name === file.name && blob.size === file.size);
  if (!replacement) {
    showError("The image was opened but could not be stored.");
    return;
  }
  await edit("update_image_blob_hash", { blockId: block.id, blobHash: replacement.hash });
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
      // A cell is a real block container.  Remember the ids before the
      // operation rather than looking only at `doc.blocks` afterwards: the
      // latter misses an image inserted in a table cell, leaving `after` at
      // the original paragraph and reversing the rest of a multi-file drop.
      const before = new Set(documentBlockIds(state.doc?.blocks ?? []));
      // `after` is narrowed before the asynchronous blob round-trip. Keep a
      // concrete anchor for this iteration; the next iteration receives the
      // newly inserted image below.
      const anchor = after;
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
      // A dropped file's name remains blob metadata, not an accidental alt
      // description. The Image properties dialog is where an author supplies
      // meaningful alternative text or a visible caption.
      await run("insert_image_block_after", { afterBlockId: anchor, blobHash: blob.hash, altText: "" });
      // Each picture lands after the previous one, so dropping several keeps
      // the order they were dropped in, including when that sibling is in a
      // cell rather than the top-level body.
      const inserted = Array.from(documentBlocks(state.doc?.blocks ?? [])).find(
        (block) => !before.has(block.id) && block.kind === "image" && block.blob_hash === blob.hash,
      );
      after = inserted?.id ?? after;
    } catch {
      // reported by run()
    }
  }
}

/** Walk the projection's body and every table-cell block in document order. */
function* documentBlocks(blocks: AppBlock[]): Generator<AppBlock> {
  for (const block of blocks) {
    yield block;
    for (const row of block.rows ?? []) {
      for (const cell of row) yield* documentBlocks(cell);
    }
  }
}

function documentBlockIds(blocks: AppBlock[]): string[] {
  return [...documentBlocks(blocks)].map((block) => block.id);
}
