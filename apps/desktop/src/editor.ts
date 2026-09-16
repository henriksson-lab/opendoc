// Single-host document editor. The Rust core renders the body HTML and
// decides what every input gesture does (`apply_editor_input`); this module
// only maps DOM selections to document positions, forwards gestures, and
// patches the DOM in place so the caret and IME state survive updates.

import type { AppBodyFragment, EditorInput, EditorPosition, EditorResult, EditorSelection } from "./types";
import { selectTableBand } from "./tables";
import { state } from "./state";

export type EditorHooks = {
  apply: (input: EditorInput) => Promise<EditorResult>;
  /** Called after every applied gesture with the new document state. */
  onResult: (result: EditorResult) => void;
  onSelectionChange: (selection: EditorSelection | null) => void;
  /** A selected focusable atomic block disappeared during a remote morph. */
  onAtomicSelectionRemoved?: () => void;
  onError: (message: string) => void;
  /** Keyboard shortcuts the chrome handles (returns true when consumed). */
  onKeydown: (event: KeyboardEvent, selection: EditorSelection | null) => Promise<boolean>;
  /**
   * Files dropped on, or pasted into, the document. `afterBlockId` is the
   * block the gesture landed on, or null when it landed nowhere in
   * particular.
   *
   * This module recognises the gesture and finds the position; it does not
   * know what a blob or an image block is. Turning the bytes into document
   * content is a command sequence, and commands are the chrome's business.
   */
  onInsertFiles: (files: File[], afterBlockId: string | null) => Promise<void>;
  /** An atomic dropdown was activated. The shell owns command dispatch. */
  onDropdownActivate: (inlineId: string) => Promise<void>;
  /** An atomic date chip was activated. The shell owns command dispatch. */
  onDateChipActivate: (inlineId: string) => Promise<void>;
};

type Gesture = {
  inputType: string;
  data: string | null;
  selection: EditorSelection | null;
  /** The clipboard's `text/html` flavour, for a paste that carried one. */
  html?: string | null;
};

const KEYED_ATTRIBUTES = ["data-block-id", "data-inline-id", "data-row-id", "data-cell-id", "data-suggestion-id"];

function codePointLength(text: string): number {
  let count = 0;
  for (const _ of text) {
    count += 1;
  }
  return count;
}

function utf16OffsetForCodePoints(text: string, codePoints: number): number {
  let index = 0;
  let seen = 0;
  for (const ch of text) {
    if (seen >= codePoints) {
      break;
    }
    index += ch.length;
    seen += 1;
  }
  return index;
}

function isAtomic(element: Element): boolean {
  return element.getAttribute("contenteditable") === "false";
}

/**
 * Which side of an atomic block a DOM boundary is on: 0 before it, 1 after.
 *
 * Those are the only two positions such a block has — Rust gives it length 1
 * — and which one it is decides what a delete key does, so it is read rather
 * than assumed. When the boundary is named against the block itself the
 * offset is a child index and the answer is whether anything of the block
 * precedes it; when it is named deeper (Chrome hit-tests a click on a figure
 * to the figure, but a caret restored into one can land on the `<img>`) there
 * is no side to read and the block counts as entered from the front.
 */
function sideOfBlock(block: Element, node: Node, offset: number): number {
  if (node === block) {
    return offset > 0 ? 1 : 0;
  }
  return 0;
}

/** True while a drag carries files, which is what a dragover has to allow. */
function carriesFiles(transfer: DataTransfer | null): boolean {
  return Array.from(transfer?.types ?? []).includes("Files");
}

/** Keep modifier-click navigation aligned with imported-link safety policy.
 * Link hrefs are document data and can originate outside the editor, so never
 * hand an executable or unknown URI scheme to `window.open`. */
function navigationHrefAllowed(href: string): boolean {
  if (!href || href.trim() !== href || /[\u0000-\u001f\u007f]/.test(href)) return false;
  const lowered = href.toLowerCase();
  if (lowered.startsWith("#") || lowered.startsWith("/") || lowered.startsWith(".")) return true;
  const separator = lowered.indexOf(":");
  return separator < 0 || ["http", "https", "mailto", "tel", "ftp"].includes(lowered.slice(0, separator));
}

/**
 * The image files on a clipboard or a drag.
 *
 * `DataTransfer.files` is empty for a clipboard paste in Chrome, and
 * `.items` is empty for some drags, so both are read and de-duplicated.
 */
function imageFiles(transfer: DataTransfer | null): File[] {
  if (!transfer) {
    return [];
  }
  const found: File[] = [];
  const seen = new Set<string>();
  const add = (file: File | null) => {
    if (!file || !file.type.startsWith("image/")) return;
    const key = `${file.name}:${file.size}:${file.type}`;
    if (seen.has(key)) return;
    seen.add(key);
    found.push(file);
  };
  for (const item of Array.from(transfer.items ?? [])) {
    if (item.kind === "file") add(item.getAsFile());
  }
  for (const file of Array.from(transfer.files ?? [])) add(file);
  return found;
}

/**
 * Decodes the narrow HTML-only image shape browsers put on a clipboard when
 * `DataTransfer.files` is empty.  This is intentionally not a general URL
 * importer: only an image-only `data:` fragment with one of the raster MIME
 * types we can own as bytes is accepted. Mixed prose plus bounded raster
 * data images goes through Rust's rich-paste parser, which preserves their
 * block order in the same editor gesture. `https:`, `blob:` and SVG still
 * become one named degradation rather than a URL-backed document image.
 */
const DATA_IMAGE_MIME_TYPES: Record<string, string> = {
  "image/png": "png",
  "image/jpeg": "jpg",
  "image/gif": "gif",
  "image/webp": "webp",
  "image/bmp": "bmp",
  "image/tiff": "tiff",
};
const MAX_HTML_DATA_IMAGE_BYTES = 4 * 1024 * 1024;
const MAX_HTML_DATA_IMAGES = 20;

function imageFilesFromHtmlDataUris(html: string, plainText: string): File[] {
  // A rich fragment with words has meaningful ordering which the current
  // image-block command cannot represent. Leave it to the parser so it keeps
  // the prose and reports the dropped object, rather than moving a picture to
  // an arbitrary side of its caption.
  if (plainText.trim() || html.length > MAX_HTML_DATA_IMAGE_BYTES) return [];
  const parsed = new DOMParser().parseFromString(html, "text/html");
  if (parsed.body.textContent?.trim()) return [];
  if (parsed.querySelector("math, svg, canvas, object, embed, iframe, video, audio")) return [];
  const images = Array.from(parsed.querySelectorAll("img[src]"));
  if (images.length === 0 || images.length > MAX_HTML_DATA_IMAGES) return [];

  const files: File[] = [];
  for (const [index, image] of images.entries()) {
    const source = image.getAttribute("src") ?? "";
    const match = /^data:(image\/(?:png|jpeg|gif|webp|bmp|tiff));base64,([A-Za-z0-9+/]+={0,2})$/i.exec(source);
    if (!match) return [];
    const mime = match[1].toLowerCase();
    const extension = DATA_IMAGE_MIME_TYPES[mime];
    if (!extension) return [];
    // Base64 expands by at least 4/3. Check before `atob` so clipboard markup
    // cannot force a surprising allocation; the app has the same 64 MiB blob
    // cap, while the tighter HTML cap makes this branch bounded to ~3 MiB.
    const encoded = match[2];
    if (encoded.length > Math.ceil(MAX_HTML_DATA_IMAGE_BYTES * 4 / 3)) return [];
    try {
      const decoded = atob(encoded);
      const bytes = new Uint8Array(decoded.length);
      for (let byte = 0; byte < decoded.length; byte += 1) bytes[byte] = decoded.charCodeAt(byte);
      files.push(new File([bytes], `pasted-image-${index + 1}.${extension}`, { type: mime }));
    } catch {
      return [];
    }
  }
  return files;
}

export class DocumentEditor {
  private queue: Gesture[] = [];
  private inFlight = false;
  private modelSelection: EditorSelection | null = null;
  private composing = false;
  private compositionSelection: EditorSelection | null = null;
  /**
   * The markup last applied, per fragment key.
   *
   * This is what makes an update cost the block that changed rather than the
   * document: a fragment whose markup string is the one already applied to
   * its live element needs no work at all — not even parsing — because that
   * element was made to match that string and nothing has touched it since.
   * See `setFragments`.
   *
   * Strings, not nodes. The previous scheme kept the whole parsed body as a
   * detached tree so it could compare subtrees with `isEqualNode`; comparing
   * per-block strings needs no tree, which is 389 KB of DOM this instance no
   * longer holds and one `innerHTML` parse of the whole body it no longer
   * does.
   */
  private appliedFragments = new Map<string, string>();
  /**
   * Undo callbacks for every listener this instance registered. The host
   * element outlives the instance (it is re-parented whenever the page is
   * rebuilt), so a `destroy()` that left listeners behind meant the next
   * instance saw each gesture twice: one Enter split the paragraph twice,
   * one keystroke typed the character twice.
   */
  private cleanups: (() => void)[] = [];
  private destroyed = false;
  private dropTarget: Element | null = null;
  /**
   * A block anchor is resolved from live border boxes. Font metrics, image
   * decodes and viewport geometry can change those boxes without a document
   * operation, so retain one observer for the host's layout lifetime.
   */
  private positionedImageObserver: ResizeObserver | null = null;

  constructor(
    private host: HTMLElement,
    private hooks: EditorHooks,
  ) {
    host.setAttribute("contenteditable", "true");
    host.setAttribute("spellcheck", "true");
    host.setAttribute("role", "textbox");
    host.setAttribute("aria-multiline", "true");
    this.listen(host, "beforeinput", (event) => this.onBeforeInput(event as InputEvent));
    this.listen(host, "compositionstart", () => this.onCompositionStart());
    this.listen(host, "compositionend", (event) => this.onCompositionEnd(event as CompositionEvent));
    this.listen(host, "keydown", (event) => void this.onKeydown(event as KeyboardEvent));
    this.listen(host, "paste", (event) => this.onPaste(event as ClipboardEvent));
    // A drop only fires at all if dragover is cancelled — without this the
    // browser navigates to the dropped file and the document is gone.
    this.listen(host, "dragover", (event) => this.onDragOver(event as DragEvent));
    this.listen(host, "dragleave", () => this.setDropTarget(null));
    this.listen(host, "drop", (event) => this.onDrop(event as DragEvent));
    this.listen(host, "click", (event) => this.onClick(event as MouseEvent));
    this.listen(host, "mousedown", (event) => this.onMouseDown(event as MouseEvent));
    this.listen(window, "resize", () => this.positionPositionedImages());
    // `load` does not bubble, but capture sees an image decode below the
    // host. Such a decode can move the named anchor even when the document
    // itself has not changed.
    this.listenCapture(host, "load", () => this.positionPositionedImages());
    // An available blob is not necessarily browser-decodable: malformed
    // bytes, an unsupported codec, or a browser policy can still make an
    // `<img>` fail after Rust has safely retained its original bytes. Capture
    // gives that failure a visible, named fallback without changing the model
    // or silently converting the source asset.
    this.listenCapture(host, "error", (event) => this.onImageLoadError(event));
    if (typeof ResizeObserver !== "undefined") {
      this.positionedImageObserver = new ResizeObserver(() => this.positionPositionedImages());
      this.positionedImageObserver.observe(host);
    }
    void document.fonts?.ready?.then(() => {
      if (!this.destroyed) this.positionPositionedImages();
    });
    // Atomic blocks are keyboard-reachable themselves.  A Tab focus must
    // select the same object a click selects, otherwise Format > Image has no
    // target for keyboard-only users even though the figure is focusable.
    this.listen(host, "focusin", (event) => this.selectAtomicBlock(event.target as Element | null));
    this.listen(document, "selectionchange", () => {
      if (this.composing || this.inFlight) {
        return;
      }
      this.hooks.onSelectionChange(this.selection());
    });
  }

  /** Keep navigation/selection available in View mode while refusing DOM edits. */
  setEditable(editable: boolean): void {
    this.host.setAttribute("contenteditable", editable ? "true" : "false");
    this.host.setAttribute("aria-readonly", editable ? "false" : "true");
  }

  private listen(target: EventTarget, type: string, handler: (event: Event) => void): void {
    target.addEventListener(type, handler);
    this.cleanups.push(() => target.removeEventListener(type, handler));
  }

  private listenCapture(target: EventTarget, type: string, handler: (event: Event) => void): void {
    target.addEventListener(type, handler, true);
    this.cleanups.push(() => target.removeEventListener(type, handler, true));
  }

  destroy(): void {
    this.destroyed = true;
    this.positionedImageObserver?.disconnect();
    this.positionedImageObserver = null;
    this.setDropTarget(null);
    this.queue.length = 0;
    for (const cleanup of this.cleanups.splice(0)) {
      cleanup();
    }
  }

  /**
   * Patch the host so it matches the body `fragments` describe.
   *
   * The body arrives as its top-level elements rather than as one string, so
   * only the elements whose markup changed are **parsed** — not just morphed.
   * The comparison is between the new markup and the markup last applied to
   * that key, never against the live DOM, so it is one string equality per
   * top-level element instead of a walk of every block on every keystroke.
   *
   * That is sound only because **a block's DOM is a function of the markup
   * last applied to it**, and keeping it that way is a standing constraint on
   * everything else that runs here:
   *
   * - Pagination's placement (`pagination.ts`) writes the physical
   *   `margin-top` and `data-page-index`, neither of which this projection
   *   ever contains, and re-applies them to every block after every update —
   *   so it can never leave a stale page break on a block this function
   *   skipped, and can never destroy a value the renderer wrote.
   * - The drop-target class is added and removed by this module.
   * - An image resize borrows the figure's `style` attribute and puts it
   *   back.
   * - The browser owns the DOM during IME composition, which is the one case
   *   that cannot be reasoned about, so `compositionstart` drops the cache
   *   and the next update re-parses and re-morphs every fragment.
   *
   * Node identity comes from the live DOM, keyed on `data-block-id`, not from
   * anything remembered here — so a fragment that moved, appeared or
   * disappeared is handled by the same keyed matching `morphChildren` does,
   * and a caret inside an element that only moved is not disturbed.
   */
  setFragments(fragments: readonly AppBodyFragment[]): void {
    if (this.nothingChanged(fragments)) {
      this.positionPositionedImages();
      return;
    }
    // A remote delete can remove a focusable atomic object (especially an
    // image) while the browser still retains its Range and active element.
    // Capture the live node, not just its id: after the keyed morph we can
    // prove whether that exact selected object survived.
    const selectedAtomic = this.selectedAtomicBlock();
    const selectedAtomicHadFocus = selectedAtomic?.contains(document.activeElement) ?? false;
    const live = liveFragments(this.host);
    const applied = new Map<string, string>();
    let cursor: Node | null = this.host.firstChild;
    for (const fragment of fragments) {
      const key = fragment.block_id;
      applied.set(key, fragment.html);
      const node = live.get(key);
      if (!node) {
        // Nothing live holds this key, so there is nothing to preserve:
        // parse it and put it in. Inserting a `DocumentFragment` moves its
        // children in and leaves `cursor` pointing where it did.
        this.host.insertBefore(parseFragment(fragment.html), cursor);
        continue;
      }
      live.delete(key);
      if (node !== cursor) {
        this.host.insertBefore(node, cursor);
      }
      if (this.appliedFragments.get(key) === fragment.html) {
        // Unchanged: not parsed, not walked, not touched.
        cursor = node.nextSibling;
        continue;
      }
      const parsed = parseFragment(fragment.html);
      const only = parsed.childNodes.length === 1 ? parsed.firstChild : null;
      if (only && sameNodeShape(node, only)) {
        morphNode(node, only);
        cursor = node.nextSibling;
      } else {
        // A fragment is one element (`opendoc-render` pins that), so this is
        // the shape-changed case — a paragraph that became a table. Replace
        // rather than guess, and carry on from what followed it.
        cursor = node.nextSibling;
        this.host.insertBefore(parsed, node);
        this.host.removeChild(node);
      }
    }
    // Whatever is left after the last fragment is a block the document no
    // longer has.
    while (cursor) {
      const next: Node | null = cursor.nextSibling;
      this.host.removeChild(cursor);
      cursor = next;
    }
    this.appliedFragments = applied;
    this.clearRemovedAtomicSelection(selectedAtomic, selectedAtomicHadFocus);
    this.positionPositionedImages();
  }

  private selectedAtomicBlock(): HTMLElement | null {
    // `state.selection` is updated by browser selection changes. The model
    // copy is only a fallback for a programmatic selection which has not yet
    // produced that event.
    const blockId = state.selection?.focus.block_id ?? this.modelSelection?.focus.block_id;
    if (!blockId) return null;
    const block = this.host.querySelector<HTMLElement>(`[data-block-id="${cssEscape(blockId)}"]`);
    return block && isAtomic(block) && !block.querySelector("[data-inline-id]") ? block : null;
  }

  private clearRemovedAtomicSelection(selected: HTMLElement | null, hadFocus: boolean): void {
    if (!selected || this.host.contains(selected)) return;
    document.getSelection()?.removeAllRanges();
    this.modelSelection = null;
    state.selection = null;
    // A deleted figure cannot remain a keyboard stop. Return a keyboard user
    // to the document surface only when that object actually owned focus;
    // an unrelated click elsewhere should not be stolen by a remote update.
    if (hadFocus) this.host.focus({ preventScroll: true });
    this.hooks.onAtomicSelectionRemoved?.();
  }

  /**
   * Resolve ADR 0022 block anchors after the renderer's fragments are live.
   * CSS can position a page-content anchor by itself, but it has no portable
   * way to name a stable document block as an anchor.  The DOM adapter only
   * reads the renderer's data attributes and writes presentational geometry;
   * it never derives or persists a document coordinate.
   */
  positionPositionedImages(): void {
    const hostBox = this.host.getBoundingClientRect();
    // `zoom` scales geometry returned by getBoundingClientRect but not the
    // CSS px values we assign. offsetWidth remains pre-zoom, so this is the
    // exact conversion back to the containing block's coordinate space.
    const scale = this.host.offsetWidth > 0 ? hostBox.width / this.host.offsetWidth : 1;
    for (const figure of Array.from(this.host.querySelectorAll<HTMLElement>('figure[data-positioned="true"]'))) {
      const restoreAnchorDescription = () => {
        const original = figure.dataset.positionAnchorDescription;
        if (original === undefined) return;
        if (original) figure.setAttribute("aria-description", original);
        else figure.removeAttribute("aria-description");
        delete figure.dataset.positionAnchorDescription;
        delete figure.dataset.positionAnchorFallback;
      };
      const exposeMissingAnchor = () => {
        const original = figure.dataset.positionAnchorDescription
          ?? figure.getAttribute("aria-description")
          ?? "";
        figure.dataset.positionAnchorDescription = original;
        figure.dataset.positionAnchorFallback = "true";
        const fallback = "Position anchor is unavailable; shown relative to page content.";
        figure.setAttribute("aria-description", original ? `${original} ${fallback}` : fallback);
      };
      const xTwips = Number(figure.dataset.positionXTwips ?? "0");
      const yTwips = Number(figure.dataset.positionYTwips ?? "0");
      const offset = (twips: number) => `${twips / 20}pt`;
      const anchor = figure.dataset.positionAnchor ?? "page-content";
      if (anchor.startsWith("block:")) {
        const id = anchor.slice("block:".length);
        const target = this.host.querySelector<HTMLElement>(`[data-block-id="${CSS.escape(id)}"]`);
        if (target && target !== figure) {
          restoreAnchorDescription();
          const targetBox = target.getBoundingClientRect();
          const left = (targetBox.left - hostBox.left) / scale;
          const top = (targetBox.top - hostBox.top) / scale;
          figure.style.insetInlineStart = `calc(${left}px + ${offset(xTwips)})`;
          figure.style.top = `calc(${top}px + ${offset(yTwips)})`;
          continue;
        }
        // Layout/PDF makes this fallback a named warning. The editor has no
        // warning stream during a mere resize/reflow, so expose the same fact
        // on the affected atomic object instead of silently guessing a new
        // block target or repeatedly toasting while the viewport moves.
        exposeMissingAnchor();
      } else {
        restoreAnchorDescription();
      }
      // The page host starts at the first page's content rectangle. Page
      // breaks are a document fact projected by the paginator, so this reads
      // its stamped page index instead of trying to infer a page from pixels.
      const page = Number(figure.dataset.pageIndex ?? "0");
      figure.style.insetInlineStart = offset(xTwips);
      figure.style.top = `calc(${page} * (var(--page-height) + var(--page-gap)) + ${offset(yTwips)})`;
    }
  }

  /**
   * True when every fragment's markup is the one already applied to its key,
   * and there are no keys either side does not have.
   *
   * The DOM is then already what `fragments` describes, by the same argument
   * `setFragments` rests on, so there is nothing to do and nothing to read
   * out of the DOM to find out. Without this an update that changes nothing
   * still walked the host's children to key them, which on a 1,500-block
   * document is a millisecond spent proving the obvious — the `setHtml` this
   * replaced compared one string and returned.
   */
  private nothingChanged(fragments: readonly AppBodyFragment[]): boolean {
    if (this.appliedFragments.size !== fragments.length) {
      return false;
    }
    for (const fragment of fragments) {
      if (this.appliedFragments.get(fragment.block_id) !== fragment.html) {
        return false;
      }
    }
    return true;
  }

  focus(): void {
    this.host.focus({ preventScroll: true });
  }

  /** Clear a focused non-image atomic block without leaving a stale range. */
  clearFocusedAtomicSelection(): boolean {
    const focused = document.activeElement instanceof Element ? document.activeElement : null;
    const block = focused?.closest<HTMLElement>("[data-block-id][contenteditable=false]");
    if (!block || !this.host.contains(block) || block.matches("figure.doc-image") || block.querySelector("[data-inline-id]")) {
      return false;
    }
    document.getSelection()?.removeAllRanges();
    this.modelSelection = null;
    state.selection = null;
    this.hooks.onSelectionChange(null);
    (document.activeElement as HTMLElement | null)?.blur();
    return true;
  }

  // ---- Selection mapping --------------------------------------------------

  selection(): EditorSelection | null {
    const selection = document.getSelection();
    if (!selection || selection.rangeCount === 0 || !selection.anchorNode) {
      return null;
    }
    if (!this.host.contains(selection.anchorNode) || !this.host.contains(selection.focusNode)) {
      return null;
    }
    const anchor = this.positionFromDom(selection.anchorNode, selection.anchorOffset);
    const focus = this.positionFromDom(selection.focusNode ?? selection.anchorNode, selection.focusOffset);
    if (!anchor || !focus) {
      return null;
    }
    return { anchor, focus };
  }

  setSelection(selection: EditorSelection | null): void {
    if (!selection) {
      return;
    }
    const anchor = this.domPointFor(selection.anchor);
    const focus = this.domPointFor(selection.focus);
    if (!anchor || !focus) {
      return;
    }
    const domSelection = document.getSelection();
    if (!domSelection) {
      return;
    }
    if (typeof domSelection.setBaseAndExtent === "function") {
      domSelection.setBaseAndExtent(anchor.node, anchor.offset, focus.node, focus.offset);
    } else {
      const range = document.createRange();
      range.setStart(anchor.node, anchor.offset);
      range.collapse(true);
      domSelection.removeAllRanges();
      domSelection.addRange(range);
    }
    this.modelSelection = selection;
    scrollCaretIntoView(focus.node);
  }

  private positionFromDom(node: Node, offset: number): EditorPosition | null {
    // A range over an atomic block has its two DOM boundaries on the block's
    // parent. In a table cell that parent still has the outer table's block
    // id, so resolving the nearest ancestor first would incorrectly report
    // the table rather than the selected image. Recognise the direct atomic
    // sibling before climbing ancestors.
    const beside = this.positionBesideBlock(node, offset);
    if (beside) {
      return beside;
    }
    const element = node.nodeType === Node.ELEMENT_NODE ? (node as Element) : node.parentElement;
    if (!element) {
      return null;
    }
    const inlineElement = element.closest("[data-inline-id]");
    const blockElement = (inlineElement ?? element).closest("[data-block-id]");
    if (!blockElement) {
      // No block ancestor at all, which means the boundary named here is
      // *between* the host's own children — the shape of "this whole block is
      // selected", and what `Range.selectNode` on a top-level block produces.
      return this.positionBesideBlock(node, offset);
    }
    const block_id = blockElement.getAttribute("data-block-id") ?? "";
    if (inlineElement && this.host.contains(inlineElement)) {
      if (isAtomic(inlineElement)) {
        return { block_id, inline_id: inlineElement.getAttribute("data-inline-id"), offset: 0 };
      }
      return {
        block_id,
        inline_id: inlineElement.getAttribute("data-inline-id"),
        offset: codePointsBefore(inlineElement, node, offset),
      };
    }
    // Caret directly inside a block element: look at the neighbouring child.
    if (node.nodeType === Node.ELEMENT_NODE) {
      const container = node as Element;
      const before = container.childNodes[offset - 1];
      const after = container.childNodes[offset];
      const beforeInline = before instanceof Element ? before.closest("[data-inline-id]") ?? before.querySelector("[data-inline-id]") : null;
      if (beforeInline) {
        const id = beforeInline.getAttribute("data-inline-id");
        return {
          block_id,
          inline_id: id,
          offset: isAtomic(beforeInline) ? 1 : codePointLength(beforeInline.textContent ?? ""),
        };
      }
      const afterInline = after instanceof Element ? after.closest("[data-inline-id]") ?? after.querySelector("[data-inline-id]") : null;
      if (afterInline) {
        return { block_id, inline_id: afterInline.getAttribute("data-inline-id"), offset: 0 };
      }
      // An empty paragraph in a table cell has no inline child. A caret that
      // Chrome places directly on its `<td>` would otherwise fall through to
      // the outer table's first inline below, so typing into a new/empty cell
      // edits an unrelated cell. Resolve the cell's own direct rendered block
      // (or a list wrapper's first/last block) before considering the table.
      const blockInChild = (child: ChildNode | undefined): Element | null => {
        if (!(child instanceof Element)) return null;
        return child.hasAttribute("data-block-id") ? child : child.querySelector("[data-block-id]");
      };
      const beforeBlock = blockInChild(before);
      if (beforeBlock) {
        return {
          block_id: beforeBlock.getAttribute("data-block-id") ?? "",
          inline_id: null,
          offset: isAtomic(beforeBlock) ? 1 : 0,
        };
      }
      const afterBlock = blockInChild(after);
      if (afterBlock) {
        return {
          block_id: afterBlock.getAttribute("data-block-id") ?? "",
          inline_id: null,
          offset: 0,
        };
      }
    }
    const firstInline = blockElement.querySelector(":scope [data-inline-id]");
    if (firstInline && firstInline.closest("[data-block-id]") === blockElement) {
      return { block_id, inline_id: firstInline.getAttribute("data-inline-id"), offset: 0 };
    }
    // A block with no inline runs of its own. When it is atomic — an image,
    // an equation block, a page break — it has exactly two positions, before
    // it and after it, and Rust reads the offset as which one. It used to be
    // hard-coded to 0, so the position Backspace needs (`delete_backward`
    // acts only when the caret is *after* the object) could not be produced
    // from the DOM at all and no image in any document could be deleted
    // backwards. An empty paragraph also lands here, and Rust clamps its
    // offset to the block's length of 0 either way.
    return {
      block_id,
      inline_id: null,
      offset: isAtomic(blockElement) ? sideOfBlock(blockElement, node, offset) : 0,
    };
  }

  /**
   * The position named by a boundary beside an atomic block.
   *
   * `Range.selectNode(block)` anchors on the block's *parent* with the offsets
   * either side of the child. For a body image that parent is the host, but
   * for an image in a table cell it is the `<td>`. Neither boundary has the
   * image's `[data-block-id]` ancestor to close over. Reading its direct child
   * on each side turns that back into the two positions of the block it
   * brackets, at either tree depth.
   */
  private positionBesideBlock(node: Node, offset: number): EditorPosition | null {
    if (node.nodeType !== Node.ELEMENT_NODE || !this.host.contains(node)) {
      return null;
    }
    const container = node as Element;
    // Only an *atomic* block is addressed from outside itself. A boundary
    // beside a paragraph is still a caret that belongs inside one of its
    // runs, and answering "offset 1 of that paragraph" instead would put
    // typed text after its first character — so that case stays unresolvable,
    // exactly as it was.
    const beside = (child: ChildNode | undefined, side: number) => {
      if (!(child instanceof Element) || !child.hasAttribute("data-block-id") || !isAtomic(child)) {
        return null;
      }
      return { block_id: child.getAttribute("data-block-id") ?? "", inline_id: null, offset: side };
    };
    return beside(container.childNodes[offset], 0) ?? beside(container.childNodes[offset - 1], 1);
  }

  /**
   * Puts the selection on a whole `contenteditable="false"` block when one is
   * clicked, because Chrome will not.
   *
   * Measured in the e2e harness rather than assumed: a mouse click anywhere
   * on an image figure inside the editable host leaves the selection exactly
   * where it already was. It is not moved onto the figure, not moved onto the
   * host, and not cleared — Chrome's own `caretRangeFromPoint` answers
   * `(figure, 0)` at those coordinates, so the hit test knows where the click
   * landed; the click simply does not act on it. The consequences were that
   * no delete key could ever reach an image, and that every Format ▸ Image
   * menu item — each of which reads `state.selection.focus.block_id` — said
   * "Select an image first." however hard the image was clicked.
   *
   * The selection made is the whole object, not a caret at one of its edges,
   * because that is what is true: there is nowhere inside an atomic block for
   * a caret to stand, so a range over it is the only honest shape, and it is
   * what makes both delete keys and a typed replacement mean the one thing
   * they can mean. Rust deletes the block for such a range
   * (`EditPlan::delete_range`).
   */
  private selectAtomicBlock(target: Element | null): void {
    const block = target?.closest("[data-block-id]");
    if (!block || !this.host.contains(block)) {
      return;
    }
    if (!isAtomic(block) || block.querySelector("[data-inline-id]")) {
      return;
    }
    const domSelection = document.getSelection();
    if (!domSelection) {
      return;
    }
    const range = document.createRange();
    range.selectNode(block);
    domSelection.removeAllRanges();
    domSelection.addRange(range);
  }

  private domPointFor(position: EditorPosition): { node: Node; offset: number } | null {
    const blockElement = this.host.querySelector(`[data-block-id="${cssEscape(position.block_id)}"]`);
    if (!blockElement) {
      return null;
    }
    if (!position.inline_id) {
      const parent = blockElement.parentNode;
      if (blockElement.getAttribute("contenteditable") === "false" && parent) {
        const index = Array.prototype.indexOf.call(parent.childNodes, blockElement);
        return { node: parent, offset: index + Math.min(position.offset, 1) };
      }
      return { node: blockElement, offset: 0 };
    }
    const inlineElement = blockElement.querySelector(`[data-inline-id="${cssEscape(position.inline_id)}"]`);
    if (!inlineElement) {
      return { node: blockElement, offset: 0 };
    }
    if (isAtomic(inlineElement)) {
      const parent = inlineElement.parentNode;
      if (!parent) {
        return null;
      }
      const index = Array.prototype.indexOf.call(parent.childNodes, inlineElement);
      return { node: parent, offset: index + Math.min(position.offset, 1) };
    }
    return domPointInInline(inlineElement, position.offset);
  }

  // ---- Input handling -----------------------------------------------------

  private onBeforeInput(event: InputEvent): void {
    const inputType = event.inputType;
    if (inputType === "insertCompositionText" || inputType === "deleteCompositionText") {
      return; // Browser owns the DOM until compositionend.
    }
    if (inputType === "historyUndo" || inputType === "historyRedo") {
      event.preventDefault();
      this.enqueue(inputType, null);
      return;
    }
    if (inputType.startsWith("format")) {
      event.preventDefault();
      this.enqueue(inputType, null);
      return;
    }
    event.preventDefault();
    let data: string | null = event.data ?? null;
    if (data === null && event.dataTransfer) {
      data = event.dataTransfer.getData("text/plain") || null;
    }
    this.enqueue(inputType, data);
  }

  private onPaste(event: ClipboardEvent): void {
    event.preventDefault();
    // A screenshot on the clipboard arrives as a file with no useful text
    // alternative, so files are checked before text: pasting a picture must
    // not silently insert its filename instead.
    const files = imageFiles(event.clipboardData);
    if (files.length > 0) {
      void this.hooks.onInsertFiles(files, this.selection()?.focus.block_id ?? null);
      return;
    }
    const html = event.clipboardData?.getData("text/html") ?? "";
    const text = event.clipboardData?.getData("text/plain") ?? "";
    const htmlImageFiles = html ? imageFilesFromHtmlDataUris(html, text) : [];
    if (htmlImageFiles.length > 0) {
      void this.hooks.onInsertFiles(htmlImageFiles, this.selection()?.focus.block_id ?? null);
      return;
    }
    if (!html && !text) {
      return;
    }
    this.enqueue("insertFromPaste", text, html || null);
  }

  private onDragOver(event: DragEvent): void {
    if (!carriesFiles(event.dataTransfer)) {
      this.setDropTarget(null);
      return;
    }
    event.preventDefault();
    if (event.dataTransfer) {
      event.dataTransfer.dropEffect = "copy";
    }
    this.setDropTarget(this.blockElementAt(event.clientX, event.clientY));
  }

  private onDrop(event: DragEvent): void {
    // Cancelled whatever the payload: an un-cancelled drop of a file replaces
    // the page with the file, and an un-cancelled drop of text would have the
    // browser edit the DOM behind the core's back.
    event.preventDefault();
    const target = this.blockElementAt(event.clientX, event.clientY);
    this.setDropTarget(null);
    const files = imageFiles(event.dataTransfer);
    if (files.length === 0) {
      return;
    }
    void this.hooks.onInsertFiles(files, target?.getAttribute("data-block-id") ?? null);
  }

  /** The block the pointer is over, used as the drop position. */
  private blockElementAt(x: number, y: number): Element | null {
    const element = document.elementFromPoint(x, y);
    if (!element || !this.host.contains(element)) {
      return null;
    }
    return element.closest("[data-block-id]");
  }

  /**
   * Marks the block a drop would land after.
   *
   * A class on a rendered element, never an inserted node: the renderer owns
   * the children of the host, and anything this module added there would be
   * deleted by the next morph.
   */
  private setDropTarget(element: Element | null): void {
    if (this.dropTarget === element) {
      return;
    }
    this.dropTarget?.classList.remove("drop-target");
    element?.classList.add("drop-target");
    this.dropTarget = element;
  }

  private onCompositionStart(): void {
    this.composing = true;
    this.compositionSelection = this.selection();
    // The browser edits the DOM itself until `compositionend`, so from here
    // on the host no longer matches the markup last applied to it and the
    // per-fragment comparison in `setFragments` would be reasoning from a
    // stale premise. Dropping the cache makes the next update re-parse and
    // re-morph every fragment against the live DOM, which is also what
    // repairs whatever the composition left behind.
    this.appliedFragments.clear();
  }

  private onCompositionEnd(event: CompositionEvent): void {
    this.composing = false;
    const data = event.data ?? "";
    const selection = this.compositionSelection ?? this.modelSelection;
    this.compositionSelection = null;
    if (data.length === 0) {
      // Composition cancelled: re-render will restore the DOM.
      this.queue.push({ inputType: "noop", data: null, selection });
      void this.pump();
      return;
    }
    this.queue.push({ inputType: "insertText", data, selection });
    void this.pump();
  }

  private async onKeydown(event: KeyboardEvent): Promise<void> {
    if (this.composing) {
      return;
    }
    const selection = this.inFlight ? this.modelSelection : this.selection();
    if (await this.hooks.onKeydown(event, selection)) {
      event.preventDefault();
    }
  }

  private onClick(event: MouseEvent): void {
    const target = event.target as Element | null;
    const dropdown = target?.closest<HTMLElement>("[data-inline-kind=dropdown][data-inline-id]");
    if (dropdown) {
      event.preventDefault();
      void this.hooks.onDropdownActivate(dropdown.dataset.inlineId ?? "");
      return;
    }
    const dateChip = target?.closest<HTMLElement>("[data-inline-kind=date-chip][data-inline-id]");
    if (dateChip) {
      event.preventDefault();
      void this.hooks.onDateChipActivate(dateChip.dataset.inlineId ?? "");
      return;
    }
    const link = target?.closest("a[data-href]");
    if (link && (event.ctrlKey || event.metaKey)) {
      event.preventDefault();
      const href = link.getAttribute("href") ?? "";
      if (href !== "#" && navigationHrefAllowed(href)) {
        window.open(href, "_blank", "noopener");
      }
      return;
    }
    this.selectAtomicBlock(target);
  }

  /** Replace only a browser-failed image projection with the renderer's
   * existing unavailable-image surface. The content-addressed blob remains
   * untouched, so Save original image still returns exactly the supplied
   * bytes and a later render may retry decoding them. */
  private onImageLoadError(event: Event): void {
    const target = event.target;
    if (!(target instanceof Element) || target.tagName !== "IMG") return;
    const image = target as HTMLImageElement;
    const figure = image.closest<HTMLElement>("figure.doc-image");
    if (!figure || !this.host.contains(figure)) return;
    const placeholder = document.createElement("div");
    placeholder.className = "doc-image-placeholder";
    const hash = image.dataset.blobHash;
    if (hash) placeholder.dataset.blobHash = hash;
    const style = image.getAttribute("style");
    if (style) placeholder.setAttribute("style", style);
    const alt = image.alt.trim();
    placeholder.textContent = alt || "Image unavailable";
    figure.setAttribute("aria-label", alt ? `Image unavailable: ${alt}` : "Image unavailable");
    figure.dataset.imageRenderFailed = "true";
    image.replaceWith(placeholder);
    this.positionPositionedImages();
  }

  private onMouseDown(event: MouseEvent): void {
    if (event.button === 0 && selectTableBand(event)) {
      event.preventDefault();
    }
  }

  private enqueue(inputType: string, data: string | null, html: string | null = null): void {
    // Only the first gesture in a burst can trust the DOM selection; later
    // ones use the caret the core returned for the previous gesture.
    const selection = this.inFlight || this.queue.length > 0 ? null : this.selection();
    this.queue.push({ inputType, data, selection, html });
    void this.pump();
  }

  private async pump(): Promise<void> {
    if (this.inFlight || this.destroyed) {
      return;
    }
    this.inFlight = true;
    try {
      while (this.queue.length > 0 && !this.destroyed) {
        const gesture = this.queue.shift() as Gesture;
        const selection = gesture.selection ?? this.modelSelection ?? this.selection();
        if (!selection) {
          continue;
        }
        if (gesture.inputType === "noop") {
          this.modelSelection = selection;
          continue;
        }
        try {
          const input: EditorInput = {
            selection,
            input_type: gesture.inputType,
            data: gesture.data,
          };
          if (gesture.html) {
            input.html = gesture.html;
          }
          const result = await this.hooks.apply(input);
          this.modelSelection = result.selection;
          this.hooks.onResult(result);
        } catch (error) {
          this.hooks.onError(error instanceof Error ? error.message : String(error));
        }
      }
    } finally {
      this.inFlight = false;
      if (!this.destroyed) {
        this.hooks.onSelectionChange(this.selection());
      }
    }
  }
}

// ---- DOM helpers -----------------------------------------------------------

function cssEscape(value: string): string {
  return typeof CSS !== "undefined" && typeof CSS.escape === "function"
    ? CSS.escape(value)
    : value.replace(/["\\]/g, "\\$&");
}

function codePointsBefore(inlineElement: Element, node: Node, offset: number): number {
  let count = 0;
  const walk = (current: Node): boolean => {
    if (current === node) {
      if (current.nodeType === Node.TEXT_NODE) {
        count += codePointLength((current.textContent ?? "").slice(0, offset));
      } else {
        for (let index = 0; index < offset && index < current.childNodes.length; index += 1) {
          count += nodeLength(current.childNodes[index]);
        }
      }
      return true;
    }
    if (current.nodeType === Node.TEXT_NODE) {
      count += codePointLength(current.textContent ?? "");
      return false;
    }
    if (current instanceof Element && current.tagName === "BR") {
      count += current.hasAttribute("data-soft-break") ? 1 : 0;
      return false;
    }
    for (const child of Array.from(current.childNodes)) {
      if (walk(child)) {
        return true;
      }
    }
    return false;
  };
  walk(inlineElement);
  return count;
}

function nodeLength(node: Node): number {
  if (node.nodeType === Node.TEXT_NODE) {
    return codePointLength(node.textContent ?? "");
  }
  if (node instanceof Element && node.tagName === "BR") {
    return node.hasAttribute("data-soft-break") ? 1 : 0;
  }
  let total = 0;
  for (const child of Array.from(node.childNodes)) {
    total += nodeLength(child);
  }
  return total;
}

function domPointInInline(inlineElement: Element, offset: number): { node: Node; offset: number } {
  let remaining = offset;
  let last: { node: Node; offset: number } = { node: inlineElement, offset: 0 };
  const visit = (current: Node): { node: Node; offset: number } | null => {
    if (current.nodeType === Node.TEXT_NODE) {
      const text = current.textContent ?? "";
      const length = codePointLength(text);
      if (remaining <= length) {
        return { node: current, offset: utf16OffsetForCodePoints(text, remaining) };
      }
      remaining -= length;
      last = { node: current, offset: text.length };
      return null;
    }
    if (current instanceof Element && current.tagName === "BR") {
      const parent = current.parentNode as Node;
      const index = Array.prototype.indexOf.call(parent.childNodes, current);
      if (current.hasAttribute("data-soft-break")) {
        if (remaining === 0) {
          return { node: parent, offset: index };
        }
        remaining -= 1;
        last = { node: parent, offset: index + 1 };
        return null;
      }
      last = { node: parent, offset: index };
      return null;
    }
    for (const child of Array.from(current.childNodes)) {
      const found = visit(child);
      if (found) {
        return found;
      }
    }
    return null;
  };
  return visit(inlineElement) ?? last;
}

function scrollCaretIntoView(node: Node): void {
  const element = node.nodeType === Node.ELEMENT_NODE ? (node as Element) : node.parentElement;
  if (element && typeof element.scrollIntoView === "function") {
    const rect = element.getBoundingClientRect();
    if (rect.bottom > window.innerHeight || rect.top < 0) {
      element.scrollIntoView({ block: "nearest" });
    }
  }
}

function keyOf(node: Node): string | null {
  if (!(node instanceof Element)) {
    return null;
  }
  for (const attribute of KEYED_ATTRIBUTES) {
    const value = node.getAttribute(attribute);
    if (value) {
      return `${attribute}=${value}`;
    }
  }
  return null;
}

/**
 * The host's top-level elements, keyed the way a body fragment is keyed.
 *
 * A fragment's key is the id of the first block it renders, and that is also
 * the first `data-block-id` written inside its markup — on the element itself
 * for a paragraph, heading, table, image, equation or page break, and on the
 * first `<li>` for a list run, whose `<ol>`/`<ul>` wrapper carries none.
 * `opendoc-render` pins that property
 * (`every_fragments_first_block_id_is_its_key`), which is what makes reading
 * the key back out of the live DOM sound rather than a guess.
 *
 * Read fresh on every update, deliberately: the live DOM is then the only
 * authority on which element holds which block, so an element the browser
 * moved during a composition, or one the user's own editing reordered, is
 * found where it actually is.
 */
function liveFragments(host: Node): Map<string, Node> {
  const found = new Map<string, Node>();
  for (const child of Array.from(host.childNodes)) {
    const key = fragmentKeyOf(child);
    if (key !== null && !found.has(key)) {
      found.set(key, child);
    }
  }
  return found;
}

function fragmentKeyOf(node: Node): string | null {
  if (!(node instanceof Element)) {
    return null;
  }
  const own = node.getAttribute("data-block-id");
  if (own !== null) {
    return own;
  }
  return node.querySelector("[data-block-id]")?.getAttribute("data-block-id") ?? null;
}

/** One fragment's markup, parsed. A `<template>` parses `<li>` and `<td>` in
 *  the content model they need, which `innerHTML` on a `<div>` would not. */
function parseFragment(html: string): DocumentFragment {
  const template = document.createElement("template");
  template.innerHTML = html;
  return template.content;
}

/**
 * Minimal keyed DOM morph: keeps matching nodes, replaces the rest.
 *
 * `applied` is the source tree that was last morphed into `target`. Where a
 * new source subtree is deep-equal to the one applied in the same place, the
 * live nodes beneath it already match it and the subtree is skipped whole.
 * Passing no `applied` tree, or failing to pair a node with its predecessor,
 * morphs exactly as before — skipping is an optimisation and never the thing
 * that makes an update correct.
 */
export function morphChildren(target: Node, source: Node, applied?: Node | null): void {
  const targetChildren = Array.from(target.childNodes);
  const sourceChildren = Array.from(source.childNodes);
  const appliedFor = pairApplied(targetChildren, applied);
  const targetByKey = new Map<string, Node>();
  for (const child of targetChildren) {
    const key = keyOf(child);
    if (key && !targetByKey.has(key)) {
      targetByKey.set(key, child);
    }
  }
  let cursor: Node | null = target.firstChild;
  for (const sourceChild of sourceChildren) {
    const key = keyOf(sourceChild);
    let match: Node | null = null;
    if (key && targetByKey.has(key)) {
      match = targetByKey.get(key) ?? null;
      targetByKey.delete(key);
    } else if (!key && cursor && !keyOf(cursor) && sameNodeShape(cursor, sourceChild)) {
      match = cursor;
    }
    if (match) {
      if (match !== cursor) {
        target.insertBefore(match, cursor);
      }
      const appliedChild = appliedFor?.get(match) ?? null;
      if (!appliedChild || !sourceChild.isEqualNode(appliedChild)) {
        morphNode(match, sourceChild, appliedChild);
      }
      cursor = match.nextSibling;
    } else {
      const imported = document.importNode(sourceChild, true);
      target.insertBefore(imported, cursor);
    }
  }
  // Remove leftovers.
  while (cursor) {
    const next: Node | null = cursor.nextSibling;
    target.removeChild(cursor);
    cursor = next;
  }
}

/**
 * Pairs each of `target`'s children with the source node that was last
 * morphed into it.
 *
 * A completed morph leaves `target`'s children matching the source's one for
 * one, in order, so the pairing is by index. When the two disagree on length
 * something moved that this function cannot account for, and it declines to
 * pair at all rather than guess — the caller then morphs everything, which is
 * always correct.
 */
function pairApplied(targetChildren: Node[], applied: Node | null | undefined): Map<Node, Node> | null {
  if (!applied) {
    return null;
  }
  const appliedChildren = applied.childNodes;
  if (appliedChildren.length !== targetChildren.length) {
    return null;
  }
  const pairs = new Map<Node, Node>();
  for (let index = 0; index < targetChildren.length; index += 1) {
    pairs.set(targetChildren[index], appliedChildren[index]);
  }
  return pairs;
}

function sameNodeShape(a: Node, b: Node): boolean {
  if (a.nodeType !== b.nodeType) {
    return false;
  }
  if (a instanceof Element && b instanceof Element) {
    return a.tagName === b.tagName;
  }
  return true;
}

function morphNode(target: Node, source: Node, applied?: Node | null): void {
  if (target.nodeType === Node.TEXT_NODE) {
    if (target.textContent !== source.textContent) {
      target.textContent = source.textContent;
    }
    return;
  }
  if (!(target instanceof Element) || !(source instanceof Element)) {
    return;
  }
  if (target.tagName !== source.tagName) {
    target.replaceWith(document.importNode(source, true));
    return;
  }
  // A tag mismatch above means the live node was replaced, so the source last
  // applied to it no longer describes anything; below, it still does.
  const appliedElement = applied instanceof Element && applied.tagName === source.tagName ? applied : null;
  for (const attribute of Array.from(target.attributes)) {
    if (!source.hasAttribute(attribute.name)) {
      target.removeAttribute(attribute.name);
    }
  }
  for (const attribute of Array.from(source.attributes)) {
    if (target.getAttribute(attribute.name) !== attribute.value) {
      target.setAttribute(attribute.name, attribute.value);
    }
  }
  morphChildren(target, source, appliedElement);
}
