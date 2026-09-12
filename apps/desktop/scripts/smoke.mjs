// Browser-level smoke test: loads the built frontend into jsdom with the
// real Rust core (WebAssembly) and drives the editor through the same DOM
// events a user produces. Run after `npm run build:wasm && npm run build`.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import { JSDOM } from "jsdom";

const root = new URL("..", import.meta.url).pathname;
const dist = join(root, "dist");

// ---- Load the WASM core in Node --------------------------------------------
const wasmGlue = await import(pathToFileURL(join(dist, "assets/wasm/opendoc_wasm.js")).href);
const wasmBytes = readFileSync(join(dist, "assets/wasm/opendoc_wasm_bg.wasm"));
wasmGlue.initSync({ module: wasmBytes });

// ---- DOM -------------------------------------------------------------------
const html = readFileSync(join(dist, "index.html"), "utf8");
const dom = new JSDOM(html, { url: "http://localhost/", pretendToBeVisual: true, runScripts: "outside-only" });
const { window } = dom;
globalThis.window = window;
globalThis.document = window.document;
for (const key of ["Node", "Element", "HTMLElement", "HTMLDetailsElement", "HTMLInputElement", "KeyboardEvent", "InputEvent", "Event", "CustomEvent", "MouseEvent", "TextDecoder", "TextEncoder", "getComputedStyle", "DOMParser", "CSS"]) {
  if (window[key] !== undefined && globalThis[key] === undefined) {
    globalThis[key] = window[key];
  }
}
globalThis.navigator = window.navigator;
globalThis.HTMLDialogElement = window.HTMLDialogElement;
window.HTMLDialogElement.prototype.showModal = function showModal() {
  this.setAttribute("open", "");
};
window.HTMLDialogElement.prototype.close = function close() {
  this.removeAttribute("open");
  this.dispatchEvent(new window.Event("close"));
};
window.HTMLElement.prototype.scrollIntoView = () => {};
window.CSS = window.CSS ?? { escape: (value) => value.replace(/["\\]/g, "\\$&") };
globalThis.CSS = window.CSS;
globalThis.requestAnimationFrame = (fn) => setTimeout(fn, 0);

const invokeModule = await import(pathToFileURL(join(dist, "assets/invoke.js")).href);
invokeModule.useWasmModule(wasmGlue);
const main = await import(pathToFileURL(join(dist, "assets/main.js")).href);
const { __test } = main;

const tick = () => new Promise((resolve) => setTimeout(resolve, 0));
async function settle(times = 6) {
  for (let i = 0; i < times; i += 1) await tick();
}

function $(selector) {
  const node = window.document.querySelector(selector);
  assert.ok(node, `expected element ${selector}`);
  return node;
}

function click(selector) {
  $(selector).dispatchEvent(new window.MouseEvent("click", { bubbles: true, cancelable: true }));
}

/** Submit any open modal dialog with the given field values. */
async function answerDialog(values = {}) {
  await settle(3);
  const dialog = window.document.querySelector("dialog.modal[open]");
  if (!dialog) return false;
  for (const [name, value] of Object.entries(values)) {
    const control = dialog.querySelector(`[name="${name}"]`);
    if (control) control.value = value;
  }
  const form = dialog.querySelector("form");
  form.dispatchEvent(new window.Event("submit", { bubbles: true, cancelable: true }));
  dialog.close();
  await settle(3);
  return true;
}

function beforeInput(target, inputType, data = null) {
  const event = new window.InputEvent("beforeinput", { bubbles: true, cancelable: true, inputType, data });
  target.dispatchEvent(event);
}

function placeCaret(inlineId, offset) {
  const span = window.document.querySelector(`[data-inline-id="${inlineId}"]`);
  assert.ok(span, `inline ${inlineId} rendered`);
  const text = span.firstChild ?? span;
  const range = window.document.createRange();
  range.setStart(text, Math.min(offset, text.textContent?.length ?? 0));
  range.collapse(true);
  const selection = window.getSelection();
  selection.removeAllRanges();
  selection.addRange(range);
  window.document.dispatchEvent(new window.Event("selectionchange"));
}

// ---- Boot ------------------------------------------------------------------
await settle(20);
assert.ok(window.document.querySelector(".home-shell"), "home screen renders");
click("[data-action='new-document']");
await answerDialog();
await settle(20);
assert.ok(window.document.querySelector("[data-editor-shell]"), "editor shell renders after New document");
let state = __test.getState();
assert.equal(state.view, "editor");
assert.equal(state.doc.title, "Untitled document");
const body = $(".doc-body");
assert.equal(body.getAttribute("contenteditable"), "true");
const firstBlock = state.doc.blocks[0];
assert.ok(firstBlock, "new document has a block");
assert.ok(body.querySelector(`[data-block-id="${firstBlock.id}"]`), "body html is rendered by Rust");

// ---- Typing through beforeinput ---------------------------------------------
placeCaret(firstBlock.content[0].id, 0);
beforeInput(body, "insertText", "Hello");
await settle(10);
beforeInput(body, "insertText", " world");
await settle(10);
state = __test.getState();
assert.equal(state.doc.blocks[0].content[0].text, "Hello world", "typed text reaches the Rust core");
assert.ok(body.textContent.includes("Hello world"), "DOM is patched from Rust HTML");

// Enter splits, Backspace joins.
beforeInput(body, "insertParagraph");
await settle(10);
state = __test.getState();
assert.equal(state.doc.blocks.length, 2, "Enter creates a second block");
beforeInput(body, "insertText", "second");
await settle(10);
beforeInput(body, "deleteContentBackward");
await settle(10);
state = __test.getState();
assert.equal(state.doc.blocks[1].content[0].text, "secon");

// Undo coalesces typing into one step.
click("[data-action='undo']");
await settle(10);
state = __test.getState();
assert.equal(state.doc.blocks[1].content[0].text, "second", "undo restores the burst deletion");

// ---- Formatting through the toolbar -----------------------------------------
__test.setSelection({
  anchor: { block_id: state.doc.blocks[0].id, inline_id: state.doc.blocks[0].content[0].id, offset: 0 },
  focus: { block_id: state.doc.blocks[0].id, inline_id: state.doc.blocks[0].content[0].id, offset: 5 },
});
click("[data-action='mark:bold']");
await settle(10);
state = __test.getState();
const bolded = state.doc.blocks[0].content.find((inline) => inline.text === "Hello");
assert.ok(bolded && bolded.marks.some((mark) => mark.startsWith("bold")), "bold applies to the selected characters only");
assert.ok(body.querySelector(".mark-bold"), "bold run is rendered");

// Heading style.
__test.setSelection({
  anchor: { block_id: state.doc.blocks[0].id, inline_id: state.doc.blocks[0].content[0].id, offset: 0 },
  focus: { block_id: state.doc.blocks[0].id, inline_id: state.doc.blocks[0].content[0].id, offset: 0 },
});
click("[data-action='style:heading:1']");
await settle(10);
state = __test.getState();
assert.equal(state.doc.blocks[0].kind, "heading");
assert.ok(body.querySelector("h1[data-block-id]"), "heading renders as h1");

// ---- Side panels --------------------------------------------------------------
click("[data-action='toggle-panel:history']");
await settle(5);
assert.ok(!$("[data-side-panel]").hidden, "history panel opens");
assert.ok($("[data-side-panel]").textContent.includes("operations"));

// ---- Spreadsheet mode ---------------------------------------------------------
click("[data-action='mode-sheets']");
await settle(20);
assert.ok(window.document.querySelector(".sheet-grid"), "sheet grid rendered by Rust");
const cell = window.document.querySelector("[data-address='A1']");
assert.ok(cell, "A1 exists");
cell.dispatchEvent(new window.MouseEvent("mousedown", { bubbles: true }));
window.dispatchEvent(new window.MouseEvent("mouseup", { bubbles: true }));
await settle(5);
const grid = $("[data-grid]");
grid.dispatchEvent(new window.KeyboardEvent("keydown", { key: "4", bubbles: true }));
await settle(5);
const editorInput = $("[data-cell-editor]");
editorInput.value = "42";
editorInput.dispatchEvent(new window.Event("input", { bubbles: true }));
editorInput.dispatchEvent(new window.KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
await settle(20);
state = __test.getState();
const a1 = state.doc.workbook.sheets[0].cells.find((c) => c.address === "A1");
assert.ok(a1 && a1.user_value === "42", `cell edit reaches the core (${a1?.user_value})`);

// ---- Row heights and column widths ---------------------------------------------
// jsdom has no layout, so every rect is zero and the header hit test would never
// fire. Give the two headers under test a real geometry; everything else (the
// hit zone, the delta maths, the single command on mouseup, the morph) is the
// shipped code.
function stubRect(node, rect) {
  node.getBoundingClientRect = () => ({ ...rect, x: rect.left, y: rect.top, toJSON: () => rect });
}

function mouse(node, type, init) {
  node.dispatchEvent(new window.MouseEvent(type, { bubbles: true, cancelable: true, ...init }));
}

const sheetIdUnderTest = __test.getState().doc.workbook.sheets[0].id;
const columnHeader = $("th[data-column='B']");
stubRect(columnHeader, { left: 100, right: 200, top: 0, bottom: 22, width: 100, height: 22 });

// Nothing stored yet: the renderer stays sparse.
assert.ok(!window.document.querySelector(".sheet-grid colgroup"), "no colgroup before any width is stored");
assert.equal(window.document.querySelector("tr[data-row='2']").getAttribute("style"), null, "no row height before any is stored");

// A drag on the header edge must commit exactly one command, on mouseup.
const opsBeforeDrag = __test.getState().doc.operation_count;
mouse(columnHeader, "mousedown", { clientX: 198, clientY: 10 });
mouse(window, "mousemove", { clientX: 218, clientY: 10 });
mouse(window, "mousemove", { clientX: 238, clientY: 10 });
mouse(window, "mousemove", { clientX: 258, clientY: 10 });
await settle(3);
assert.equal(__test.getState().doc.operation_count, opsBeforeDrag, "dragging does not dispatch a command per mousemove");
assert.equal(window.document.querySelector(".sheet-grid col[data-column='B']").style.width, "160px", "the drag previews live");
mouse(window, "mouseup", { clientX: 258, clientY: 10 });
await settle(30);
state = __test.getState();
assert.equal(state.doc.workbook.sheets[0].column_widths.B, 160, "mouseup stores the dragged column width");
assert.equal(state.doc.operation_count, opsBeforeDrag + 1, "one drag is exactly one undoable operation");

// The re-render goes through morphChildren; the stored width must come back
// from the Rust HTML, not from the preview the drag left behind.
assert.equal(window.document.querySelector(".sheet-grid col[data-column='B']").getAttribute("style"), "width:160px", "the rendered colgroup carries the stored width");

// Row drag on the row-header bottom edge.
const rowHeader = $("tr[data-row='2'] th.row-header");
stubRect(rowHeader, { left: 0, right: 46, top: 40, bottom: 64, width: 46, height: 24 });
mouse(rowHeader, "mousedown", { clientX: 20, clientY: 62 });
mouse(window, "mousemove", { clientX: 20, clientY: 98 });
mouse(window, "mouseup", { clientX: 20, clientY: 98 });
await settle(30);
state = __test.getState();
assert.equal(state.doc.workbook.sheets[0].row_heights["2"], 60, "mouseup stores the dragged row height");
assert.equal(window.document.querySelector("tr[data-row='2']").getAttribute("style"), "height:60px", "the rendered row carries the stored height");

// The hit zone is only the trailing edge: a press in the middle of a header
// selects, it must never resize.
const widthBeforeMiddlePress = __test.getState().doc.workbook.sheets[0].column_widths.B;
mouse($("th[data-column='B']"), "mousedown", { clientX: 120, clientY: 10 });
mouse(window, "mousemove", { clientX: 300, clientY: 10 });
mouse(window, "mouseup", { clientX: 300, clientY: 10 });
await settle(20);
assert.equal(__test.getState().doc.workbook.sheets[0].column_widths.B, widthBeforeMiddlePress, "pressing the middle of a header does not resize");

// A drag that ends where it began changes nothing.
const opsAfterDrags = state.doc.operation_count;
stubRect($("th[data-column='B']"), { left: 100, right: 260, top: 0, bottom: 22, width: 160, height: 22 });
mouse($("th[data-column='B']"), "mousedown", { clientX: 258, clientY: 10 });
mouse(window, "mouseup", { clientX: 258, clientY: 10 });
await settle(10);
assert.equal(__test.getState().doc.operation_count, opsAfterDrags, "a zero-length drag dispatches nothing");

// Listener hygiene: after mouseup the window handlers are gone, so a stray
// mousemove must not resize anything.
mouse(window, "mousemove", { clientX: 900, clientY: 900 });
await settle(5);
assert.equal(__test.getState().doc.workbook.sheets[0].column_widths.B, 160, "no drag state survives mouseup");

// Double-click on the same edge resets to the default.
mouse($("th[data-column='B']"), "dblclick", { clientX: 258, clientY: 10 });
await settle(30);
state = __test.getState();
assert.equal(state.doc.workbook.sheets[0].column_widths.B, undefined, "double-click clears the stored column width");
assert.ok(!window.document.querySelector(".sheet-grid colgroup"), "clearing the last width drops the colgroup again");

// Menu path: prefilled with the current effective size, clamped on the way out.
$("[data-address='C3']").dispatchEvent(new window.MouseEvent("mousedown", { bubbles: true }));
window.dispatchEvent(new window.MouseEvent("mouseup", { bubbles: true }));
await settle(10);
// Not awaited: the action's promise only settles once the dialog is answered.
const columnWidthAction = __test.runAction("column-width");
await settle(3);
const widthField = $("dialog.modal[open] input[name='size']");
assert.equal(widthField.value, "100", "the dialog prefills the effective column width");
await answerDialog({ size: "9000" });
await columnWidthAction;
await settle(30);
state = __test.getState();
assert.equal(state.doc.workbook.sheets[0].column_widths.C, 2000, "the dialog clamps to the maximum axis size");

const rowHeightAction = __test.runAction("row-height");
await settle(3);
assert.equal($("dialog.modal[open] input[name='size']").value, "24", "the dialog prefills the effective row height");
await answerDialog({ size: "72" });
await rowHeightAction;
await settle(30);
state = __test.getState();
assert.equal(state.doc.workbook.sheets[0].row_heights["3"], 72, "the dialog stores the requested row height");
assert.equal(window.document.querySelector("tr[data-row='3']").getAttribute("style"), "height:72px", "the stored height reaches the DOM");

// Undo must put both axes back, which proves the commands are ordinary
// undoable operations rather than view state.
await __test.runAction("undo");
await settle(30);
state = __test.getState();
assert.equal(state.doc.workbook.sheets[0].row_heights["3"], undefined, "undo reverts a row height");
assert.equal(state.doc.workbook.sheets[0].column_widths.C, 2000, "undo reverts exactly one step");
assert.equal(sheetIdUnderTest, state.doc.workbook.sheets[0].id, "the axis tests stayed on one sheet");

// ---- Back to docs: gestures must survive an editor-host rebuild -----------------
// Switching modes rebuilds [data-page] and re-creates the DocumentEditor on the
// same host element. When the old instance left its listeners behind, every
// gesture ran twice: one keystroke typed two characters, one Enter split twice,
// and Enter in a list item made a bullet and immediately left the list again.
click("[data-action='mode-docs']");
await settle(20);

state = __test.getState();
const tailBlock = state.doc.blocks[state.doc.blocks.length - 1];
placeCaret(tailBlock.content[0].id, tailBlock.content[0].text.length);
beforeInput(body, "insertText", "!");
await settle(10);
state = __test.getState();
assert.equal(state.doc.blocks[state.doc.blocks.length - 1].content.map((run) => run.text).join(""), "second!", "one keystroke types one character after a mode switch");

const blockCount = state.doc.blocks.length;
beforeInput(body, "insertParagraph");
await settle(10);
state = __test.getState();
assert.equal(state.doc.blocks.length, blockCount + 1, "one Enter creates exactly one new block");

// ---- Enter inside a list item --------------------------------------------------
for (const listKind of ["bullet", "ordered", "checklist"]) {
  state = __test.getState();
  let item = state.doc.blocks[state.doc.blocks.length - 1];
  __test.setSelection({
    anchor: { block_id: item.id, inline_id: item.content[0].id, offset: 0 },
    focus: { block_id: item.id, inline_id: item.content[0].id, offset: 0 },
  });
  click(`[data-toolbar] [data-action='style:list:${listKind}']`);
  await settle(15);
  state = __test.getState();
  item = state.doc.blocks[state.doc.blocks.length - 1];
  assert.equal(item.kind, "list-item", "the list button makes a list item");
  assert.equal(item.list_kind, listKind, "the list button applies the requested marker");
  assert.equal(item.checked, listKind === "checklist" ? false : null, "only a checklist item carries a checkbox, and a new one is open");

  placeCaret(item.content[0].id, 0);
  beforeInput(body, "insertText", "item");
  await settle(10);
  state = __test.getState();
  const listCount = state.doc.blocks.length;
  placeCaret(state.doc.blocks[listCount - 1].content[0].id, 4);
  beforeInput(body, "insertParagraph");
  await settle(10);
  state = __test.getState();
  assert.equal(state.doc.blocks.length, listCount + 1, "Enter in a list item adds exactly one block");
  const continuation = state.doc.blocks[state.doc.blocks.length - 1];
  assert.equal(continuation.kind, "list-item", "Enter in a list item continues the list");
  assert.equal(continuation.list_kind, listKind, "the continuation keeps the marker");
  assert.equal(continuation.level, state.doc.blocks[state.doc.blocks.length - 2].level, "the continuation keeps the level");

  // Enter on the empty continuation leaves the list instead of adding a bullet.
  placeCaret(continuation.content[0].id, 0);
  beforeInput(body, "insertParagraph");
  await settle(10);
  state = __test.getState();
  assert.equal(state.doc.blocks.length, listCount + 1, "leaving the list adds no block");
  assert.equal(state.doc.blocks[state.doc.blocks.length - 1].kind, "paragraph", "Enter on an empty list item leaves the list");
}

// ---- Unsaved-work guard (FS-6) ---------------------------------------------------
// The guard lives in the Rust dispatcher, so every document-replacing command
// is refused over unsaved work no matter which action asked for it. These four
// used to replace the open document silently.
state = __test.getState();
assert.ok(state.doc.has_unsaved_changes, "the document under test has unsaved work");
const replacing = [
  ["open_local_repository", { path: "/nonexistent", documentUuid: "doc" }],
  ["open_flat_repository", { path: "/nonexistent", namespace: "ns", documentUuid: "doc" }],
  ["import_doc_or_docx_path", { path: "/nonexistent.docx" }],
  ["import_docx_base64", { name: "x.docx", base64: "" }],
  ["import_google_docs_json", { title: "x", jsonText: "{}" }],
  ["create_document", { title: "Replacement" }],
];
for (const [command, args] of replacing) {
  let refused = null;
  try {
    await invokeModule.dispatch(command, args);
  } catch (error) {
    refused = String(error?.message ?? error);
  }
  assert.ok(refused, `${command} must be refused while there are unsaved changes`);
  assert.ok(refused.includes("UnsavedChanges"), `${command} refused with: ${refused}`);
}
state = __test.getState();
assert.ok(state.doc.has_unsaved_changes, "a refused replacement leaves the work alone");

// Declining the prompt cancels the action instead of losing the document.
const titleBefore = state.doc.title;
const blocksBefore = state.doc.blocks.length;
const declined = __test.runAction("new-document");
await settle(5);
const prompt = $("dialog.modal[open]");
assert.ok(prompt.textContent.includes("Discard unsaved changes?"), "the guard raises one prompt");
prompt.querySelector("[data-cancel]").dispatchEvent(new window.MouseEvent("click", { bubbles: true }));
prompt.close();
await declined;
await settle(10);
state = __test.getState();
assert.equal(state.doc.title, titleBefore, "declining keeps the document");
assert.equal(state.doc.blocks.length, blocksBefore, "declining keeps the content");
assert.equal(window.document.querySelectorAll("dialog.modal[open]").length, 0, "the prompt is not stacked");

// ---- Back to home ---------------------------------------------------------------
await settle(10);
click("[data-action='go-home']");
await settle(10);
assert.ok(window.document.querySelector(".home-shell"), "home screen returns");

// ---- Re-entering the editor must not double-bind [data-action] ------------------
// `app` outlives every render, so its delegated click listener may be attached
// only once. Entering the editor a second time is what used to duplicate it, and
// after that one click ran its action twice (two stacked dialogs, two sheets).
click("[data-action='new-spreadsheet']");
await answerDialog(); // "Discard unsaved changes?" confirmation
await settle(30);
click("[data-action='mode-sheets']");
await settle(20);
state = __test.getState();
assert.equal(state.view, "editor");
const sheetsBefore = state.doc.workbook.sheets.length;
click(".sheet-tab.add");
await settle(6);
assert.equal(window.document.querySelectorAll("dialog.modal[open]").length, 1, "one click on + opens exactly one dialog");

// The pre-filled name must be focused with the caret after its last character.
const sheetName = $("dialog.modal[open] input[name='title']");
assert.ok(sheetName.value.length > 0, "add-sheet suggests a name");
assert.equal(window.document.activeElement, sheetName, "the pre-filled field is focused");
assert.equal(sheetName.selectionStart, sheetName.value.length, "caret starts at the end of the pre-filled text");
assert.equal(sheetName.selectionEnd, sheetName.value.length, "nothing is selected");

let dialogsAnswered = 0;
while (await answerDialog()) {
  dialogsAnswered += 1;
  assert.ok(dialogsAnswered < 5, "add-sheet dialog keeps re-opening");
}
await settle(30);
state = __test.getState();
assert.equal(dialogsAnswered, 1, "one click on + raises exactly one dialog");
assert.equal(state.doc.workbook.sheets.length, sheetsBefore + 1, "one click on + adds exactly one sheet");

// ---- Versions panel (CO-16 / UI-27) ----------------------------------------
// The browser runtime has no filesystem repository, so the panel is rendered
// against a known view through the test seam; the repository-backed path is
// covered by the Rust tests in app/version_service.rs.
click("[data-action='toggle-panel:versions']");
await settle(6);
assert.match(
  $("[data-side-panel]").textContent,
  /Save this document to a repository/,
  "versions panel explains the no-repository case",
);
assert.ok(
  $("[data-error]").hidden || $("[data-error]").textContent.trim() === "",
  "opening versions on an unsaved document raises no error banner",
);

__test.setVersionView({
  document_uuid: "doc-1",
  branch: "main",
  repository_root: "/tmp/repo",
  repository_backend: "local",
  head: "sha256:aaaa",
  current: "sha256:aaaa",
  truncated: false,
  warnings: [{ code: "version-history-problem", message: "sha256:cccc: snapshot object is missing" }],
  versions: [
    { manifest: "sha256:aaaa", parent: "sha256:bbbb", snapshot: "sha256:1111", created_at_ms: 1757500000000, signers: [{ signer: "ssh-ed25519 AAAA", signer_display: "Ada", title: "Doc", signed_at_ms: 1757500000000 }], label: "Sent to legal", label_author: "Ada", snapshot_present: true, is_head: true, is_current: true },
    { manifest: "sha256:bbbb", parent: null, snapshot: "sha256:2222", created_at_ms: 1757400000000, signers: [], label: null, label_author: null, snapshot_present: true, is_head: false, is_current: false },
    { manifest: "sha256:cccc", parent: null, snapshot: "sha256:3333", created_at_ms: 1757300000000, signers: [], label: null, label_author: null, snapshot_present: false, is_head: false, is_current: false },
  ],
  preview: null,
  diff: {
    from_manifest: "sha256:bbbb",
    to_manifest: "sha256:aaaa",
    added: 1,
    removed: 1,
    changed: 1,
    entries: [
      { change: "added", block_id: "b2", kind: "paragraph", path: "2", before_text: "", after_text: "new text" },
      { change: "removed", block_id: "b3", kind: "heading 2", path: "3", before_text: "gone", after_text: "" },
      { change: "changed", block_id: "b1", kind: "paragraph", path: "1", before_text: "old", after_text: "new" },
    ],
  },
});
await settle(6);

const versionsPanel = $("[data-side-panel]");
assert.match(versionsPanel.textContent, /Sent to legal/, "a named version shows its label, not its digest");
assert.match(versionsPanel.textContent, /Ada/, "signers are listed");
assert.match(versionsPanel.textContent, /snapshot missing/, "a version with no snapshot object is badged");
assert.match(versionsPanel.textContent, /snapshot object is missing/, "history warnings are surfaced");
assert.match(versionsPanel.textContent, /1 added/, "diff counts are shown");

// Restore must be refused for the open version and for one whose snapshot is gone.
const restores = Array.from(versionsPanel.querySelectorAll(".version [data-action='version:restore']"));
assert.deepEqual(
  restores.map((button) => button.disabled),
  [true, false, true],
  "restore is disabled for the current version and the one missing its snapshot",
);

console.log("desktop smoke passed");
process.exit(0);
