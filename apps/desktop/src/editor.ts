// Single-host document editor. The Rust core renders the body HTML and
// decides what every input gesture does (`apply_editor_input`); this module
// only maps DOM selections to document positions, forwards gestures, and
// patches the DOM in place so the caret and IME state survive updates.

import type { EditorInput, EditorPosition, EditorResult, EditorSelection } from "./types";

export type EditorHooks = {
  apply: (input: EditorInput) => Promise<EditorResult>;
  /** Called after every applied gesture with the new document state. */
  onResult: (result: EditorResult) => void;
  onSelectionChange: (selection: EditorSelection | null) => void;
  onError: (message: string) => void;
  /** Keyboard shortcuts the chrome handles (returns true when consumed). */
  onKeydown: (event: KeyboardEvent, selection: EditorSelection | null) => Promise<boolean>;
};

type Gesture = { inputType: string; data: string | null; selection: EditorSelection | null };

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

export class DocumentEditor {
  private queue: Gesture[] = [];
  private inFlight = false;
  private modelSelection: EditorSelection | null = null;
  private composing = false;
  private compositionSelection: EditorSelection | null = null;
  /**
   * Undo callbacks for every listener this instance registered. The host
   * element outlives the instance (it is re-parented whenever the page is
   * rebuilt), so a `destroy()` that left listeners behind meant the next
   * instance saw each gesture twice: one Enter split the paragraph twice,
   * one keystroke typed the character twice.
   */
  private cleanups: (() => void)[] = [];
  private destroyed = false;

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
    this.listen(host, "drop", (event) => event.preventDefault());
    this.listen(host, "click", (event) => this.onClick(event as MouseEvent));
    this.listen(document, "selectionchange", () => {
      if (this.composing || this.inFlight) {
        return;
      }
      this.hooks.onSelectionChange(this.selection());
    });
  }

  private listen(target: EventTarget, type: string, handler: (event: Event) => void): void {
    target.addEventListener(type, handler);
    this.cleanups.push(() => target.removeEventListener(type, handler));
  }

  destroy(): void {
    this.destroyed = true;
    this.queue.length = 0;
    for (const cleanup of this.cleanups.splice(0)) {
      cleanup();
    }
  }

  /** Patch the host so it matches `html`, preserving unchanged nodes. */
  setHtml(html: string): void {
    const template = document.createElement("template");
    template.innerHTML = html;
    morphChildren(this.host, template.content);
  }

  focus(): void {
    this.host.focus({ preventScroll: true });
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
    const element = node.nodeType === Node.ELEMENT_NODE ? (node as Element) : node.parentElement;
    if (!element) {
      return null;
    }
    const inlineElement = element.closest("[data-inline-id]");
    const blockElement = (inlineElement ?? element).closest("[data-block-id]");
    if (!blockElement) {
      return null;
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
    }
    const firstInline = blockElement.querySelector(":scope [data-inline-id]");
    if (firstInline && firstInline.closest("[data-block-id]") === blockElement) {
      return { block_id, inline_id: firstInline.getAttribute("data-inline-id"), offset: 0 };
    }
    return { block_id, inline_id: null, offset: 0 };
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
    const html = event.clipboardData?.getData("text/html") ?? "";
    const text = event.clipboardData?.getData("text/plain") ?? "";
    if (!html && !text) {
      return;
    }
    this.enqueue("insertFromPaste", text, html || null);
  }

  private onCompositionStart(): void {
    this.composing = true;
    this.compositionSelection = this.selection();
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
    const link = target?.closest("a[data-href]");
    if (link && (event.ctrlKey || event.metaKey)) {
      event.preventDefault();
      const href = link.getAttribute("href") ?? "";
      if (href && href !== "#") {
        window.open(href, "_blank", "noopener");
      }
    }
  }

  private enqueue(inputType: string, data: string | null, html: string | null = null): void {
    // Only the first gesture in a burst can trust the DOM selection; later
    // ones use the caret the core returned for the previous gesture.
    const selection = this.inFlight || this.queue.length > 0 ? null : this.selection();
    this.queue.push({ inputType, data: html ? `${data ?? ""}` : data, selection });
    if (html) {
      this.queue[this.queue.length - 1] = { inputType, data, selection, ...({ html } as object) };
    }
    void this.pump();
  }

  private async pump(): Promise<void> {
    if (this.inFlight || this.destroyed) {
      return;
    }
    this.inFlight = true;
    try {
      while (this.queue.length > 0 && !this.destroyed) {
        const gesture = this.queue.shift() as Gesture & { html?: string };
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

/** Minimal keyed DOM morph: keeps matching nodes, replaces the rest. */
export function morphChildren(target: Node, source: Node): void {
  const targetChildren = Array.from(target.childNodes);
  const sourceChildren = Array.from(source.childNodes);
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
      morphNode(match, sourceChild);
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

function sameNodeShape(a: Node, b: Node): boolean {
  if (a.nodeType !== b.nodeType) {
    return false;
  }
  if (a instanceof Element && b instanceof Element) {
    return a.tagName === b.tagName;
  }
  return true;
}

function morphNode(target: Node, source: Node): void {
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
  morphChildren(target, source);
}
