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
for (const key of ["Node", "NodeFilter", "Element", "HTMLElement", "HTMLDetailsElement", "HTMLInputElement", "KeyboardEvent", "InputEvent", "Event", "CustomEvent", "MouseEvent", "TextDecoder", "TextEncoder", "getComputedStyle", "DOMParser", "CSS"]) {
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
let lastScrolledElement = null;
window.HTMLElement.prototype.scrollIntoView = function scrollIntoView() {
  lastScrolledElement = this;
};
window.CSS = window.CSS ?? { escape: (value) => value.replace(/["\\]/g, "\\$&") };
globalThis.CSS = window.CSS;
globalThis.requestAnimationFrame = (fn) => setTimeout(fn, 0);

const invokeModule = await import(pathToFileURL(join(dist, "assets/invoke.js")).href);
invokeModule.useWasmModule(wasmGlue);
const main = await import(pathToFileURL(join(dist, "assets/main.js")).href);
const collabModule = await import(pathToFileURL(join(dist, "assets/collab.js")).href);
const tablesModule = await import(pathToFileURL(join(dist, "assets/tables.js")).href);
const imagesModule = await import(pathToFileURL(join(dist, "assets/images.js")).href);
const filesModule = await import(pathToFileURL(join(dist, "assets/files.js")).href);
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

// Google copies account-context editor URLs when a person is signed into more
// than one account. The public export remains credential-free and must work
// for both Docs and Sheets rather than rejecting an otherwise normal sharing
// link before any network request is made.
assert.equal(
  filesModule.googlePublicExportUrl(
    "https://docs.google.com/document/u/0/d/doc_Example-1/edit?usp=sharing",
    "document",
    "docx",
  ),
  "https://docs.google.com/document/d/doc_Example-1/export?format=docx",
);
assert.equal(
  filesModule.googlePublicExportUrl(
    "https://docs.google.com/spreadsheets/u/12/d/sheet_Example-2/edit#gid=0",
    "spreadsheets",
    "xlsx",
  ),
  "https://docs.google.com/spreadsheets/d/sheet_Example-2/export?format=xlsx",
);
assert.equal(
  filesModule.googlePublicExportUrl(
    "https://docs.google.com/document/u/not-a-number/d/doc_Example-1/edit",
    "document",
    "docx",
  ),
  null,
);

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

function pastePlainText(target, text) {
  const event = new window.Event("paste", { bubbles: true, cancelable: true });
  Object.defineProperty(event, "clipboardData", {
    value: { getData: (kind) => kind === "text/plain" ? text : "" },
  });
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
const saveStatus = $("[data-status]");
assert.equal(saveStatus.getAttribute("role"), "status", "save state is announced to assistive technology");
assert.equal(saveStatus.getAttribute("aria-live"), "polite", "save announcements do not interrupt typing");
const activityStatus = $("[data-activity-status]");
const workspace = $("[data-workspace]");
assert.equal(activityStatus.getAttribute("role"), "status", "async work has its own accessible announcement channel");
assert.equal(activityStatus.getAttribute("aria-live"), "polite", "async work does not interrupt document reading");
assert.equal(workspace.getAttribute("aria-busy"), "false", "a settled editor is not announced as busy");
__test.adjustPendingOperationsForTest(1);
__test.adjustPendingOperationsForTest(1);
assert.equal(workspace.getAttribute("aria-busy"), "true", "overlapping commands keep the workspace busy");
assert.equal(activityStatus.textContent, "Working…", "async work has a concise live announcement");
__test.adjustPendingOperationsForTest(-1);
assert.equal(workspace.getAttribute("aria-busy"), "true", "one completion cannot clear another command's busy state");
__test.adjustPendingOperationsForTest(-1);
assert.equal(workspace.getAttribute("aria-busy"), "false", "the workspace becomes ready only after all work settles");
assert.equal(activityStatus.textContent, "", "the live activity message clears after work settles");
let state = __test.getState();
assert.equal(state.view, "editor");
assert.equal(state.doc.title, "Untitled document");
let copiedServiceLink = null;
Object.defineProperty(window.navigator, "clipboard", {
  configurable: true,
  value: { writeText: async (value) => { copiedServiceLink = value; } },
});
assert.equal(await collabModule.copyServiceLinkToClipboard("https://collab.example.test/v1/documents/example"), true, "an explicit copy reports clipboard success");
assert.equal(copiedServiceLink, "https://collab.example.test/v1/documents/example", "only the credential-free service link reaches the clipboard");
assert.equal([...window.document.querySelectorAll(".toast")].at(-1)?.textContent, "Service link copied. It does not grant access by itself.", "copy feedback does not imply a permission grant");
Object.defineProperty(window.navigator, "clipboard", { configurable: true, value: undefined });
const clipboardFallback = collabModule.copyServiceLinkToClipboard("https://collab.example.test/v1/documents/example");
await settle(3);
const copyFallbackDialog = $("dialog.modal[open]");
assert.equal(copyFallbackDialog.querySelector('[name="serviceLink"]')?.value, "https://collab.example.test/v1/documents/example", "clipboard failure keeps a selectable credential-free link available");
copyFallbackDialog.querySelector("[data-cancel]")?.dispatchEvent(new window.MouseEvent("click", { bubbles: true }));
assert.equal(await clipboardFallback, false, "clipboard fallback never claims a link was copied");
// A Share removal is destructive service state. The desktop must not emit the
// authenticated mutation merely because its first form was submitted; this
// runs the shipped browser path with a local HTTP seam, then declines the
// explicit confirmation.
const originalFetch = globalThis.fetch;
const sharingRequests = [];
globalThis.fetch = async (url, init = {}) => {
  sharingRequests.push({ url: String(url), method: init.method ?? "GET" });
  if (String(url).endsWith("/grants")) {
    return new Response(JSON.stringify([{ subject: "bob", role: "viewer" }]), { status: 200 });
  }
  if (String(url).endsWith("/grants/audit")) {
    return new Response(JSON.stringify([]), { status: 200 });
  }
  throw new Error(`unexpected sharing request ${url}`);
};
__test.setCollaborationStatus({
  phase: "live",
  document_uuid: "sharing-smoke-document",
  display_name: "Owner",
  commit_seq: 0,
  acknowledged_seq: 0,
  pending_operations: 0,
  can_submit: true,
  reconnect_requested: false,
  document_changed: false,
  selection: null,
  notice: null,
  session: { subject: "owner", actor: "owner-actor", document_uuid: "sharing-smoke-document", role: "owner", peers: [], acknowledged_seq: 0 },
});
window.__OPENDOC_COLLAB__.setBrowserSharingSessionForTest({ base: "https://collab.example.test", token: "test-bearer", documentUuid: "sharing-smoke-document" });
const declinedRemoval = collabModule.openSharingDialog();
await settle(3);
await answerDialog({ subject: "bob", role: "remove" });
await settle(3);
const removalConfirmation = $("dialog.modal[open]");
assert.equal(removalConfirmation.querySelector("h2")?.textContent, "Remove access?", "removing a grant asks for explicit confirmation");
removalConfirmation.querySelector("[data-cancel]")?.dispatchEvent(new window.MouseEvent("click", { bubbles: true }));
await declinedRemoval;
assert.deepEqual(sharingRequests.map((request) => request.method), ["GET", "GET"], "declining removal never sends an ACL mutation");
window.__OPENDOC_COLLAB__.setBrowserSharingSessionForTest(null);
__test.setCollaborationStatus({
  phase: "idle",
  document_uuid: "",
  display_name: "",
  commit_seq: 0,
  acknowledged_seq: 0,
  pending_operations: 0,
  can_submit: false,
  reconnect_requested: false,
  document_changed: false,
  selection: null,
  notice: null,
  session: null,
});
globalThis.fetch = originalFetch;
window.localStorage.setItem("opendoc.collab.recent-service-url.v1", "https://collab.example.test");
window.localStorage.setItem("opendoc.collab.recent-subject.v1", "recent-user");
click('[data-collab-action="connect"]');
await settle(3);
const connectDialog = $("dialog.modal[open]");
assert.equal(connectDialog.querySelector('[name="serviceUrl"]')?.value, "https://collab.example.test", "the reconnect dialog pre-fills the last non-secret service address");
assert.equal(connectDialog.querySelector('[name="subject"]')?.value, "recent-user", "the reconnect dialog pre-fills the last non-secret subject");
connectDialog.querySelector("[data-cancel]")?.dispatchEvent(new window.MouseEvent("click", { bubbles: true }));
await settle(3);
window.localStorage.removeItem("opendoc.collab.recent-service-url.v1");
window.localStorage.removeItem("opendoc.collab.recent-subject.v1");
window.localStorage.setItem("opendoc.collab.recent-service-url.v1", "https://leaked-user:leaked-secret@collab.example.test/?token=leaked#fragment");
click('[data-collab-action="connect"]');
await settle(3);
const unsafeConnectDialog = $("dialog.modal[open]");
assert.equal(unsafeConnectDialog.querySelector('[name="serviceUrl"]')?.value, "http://127.0.0.1:8787", "credential-bearing legacy service hints are never prefilled");
assert.equal(window.localStorage.getItem("opendoc.collab.recent-service-url.v1"), null, "credential-bearing legacy service hints are purged");
unsafeConnectDialog.querySelector("[data-cancel]")?.dispatchEvent(new window.MouseEvent("click", { bubbles: true }));
await settle(3);
window.localStorage.setItem("opendoc.collab.recent-subject.v1", "x".repeat(2049));
click('[data-collab-action="connect"]');
await settle(3);
const oversizedSubjectDialog = $("dialog.modal[open]");
assert.equal(oversizedSubjectDialog.querySelector('[name="subject"]')?.value, "", "an oversized legacy reconnect subject is never prefilled");
assert.equal(window.localStorage.getItem("opendoc.collab.recent-subject.v1"), null, "oversized legacy reconnect subjects are purged");
oversizedSubjectDialog.querySelector("[data-cancel]")?.dispatchEvent(new window.MouseEvent("click", { bubbles: true }));
await settle(3);
const body = $(".doc-body");
assert.equal(body.getAttribute("contenteditable"), "true");
assert.equal(body.getAttribute("role"), "textbox", "the document surface exposes an editing role");
assert.equal(body.getAttribute("aria-label"), "Document editor: Untitled document", "the document surface has a useful accessible name");
assert.equal(body.getAttribute("lang"), state.doc.locale, "the document surface supplies its durable locale to native spelling and grammar tools");
const firstBlock = state.doc.blocks[0];
const stalePositionAnchors = imagesModule.positionAnchorOptions([firstBlock], "image-not-in-document", "deleted-anchor");
assert.deepEqual(
  stalePositionAnchors.map((option) => option.value),
  ["", "deleted-anchor", firstBlock.id],
  "a stale positioned-image anchor remains a selected dialog value rather than falling through to page content",
);
assert.match(stalePositionAnchors[1].label, /preserved; page-content fallback/, "the position dialog explains the stale-anchor fallback");
assert.ok(firstBlock, "new document has a block");
assert.ok(body.querySelector(`[data-block-id="${firstBlock.id}"]`), "body html is rendered by Rust");

// Tab traversal belongs to the table whose cell holds the caret. Descendant
// cells from a nested table are not siblings in the outer grid.
const nestedGestureTable = window.document.createElement("table");
nestedGestureTable.dataset.blockId = "outer-gesture-table";
const outerRow = nestedGestureTable.insertRow();
const outerFirst = outerRow.insertCell();
outerFirst.dataset.cellId = "outer-first";
outerFirst.dataset.columnId = "outer-column-a";
const outerFirstRun = window.document.createElement("span");
outerFirstRun.dataset.inlineId = "outer-first-run";
outerFirstRun.textContent = "Outer first";
outerFirst.append(outerFirstRun);
const nestedTable = window.document.createElement("table");
const nestedRow = nestedTable.insertRow();
const nestedCell = nestedRow.insertCell();
nestedCell.dataset.cellId = "nested-cell";
nestedCell.dataset.columnId = "nested-column";
const nestedRun = window.document.createElement("span");
nestedRun.dataset.inlineId = "nested-run";
nestedRun.textContent = "Nested";
nestedCell.append(nestedRun);
outerFirst.append(nestedTable);
const outerSecond = outerRow.insertCell();
outerSecond.dataset.cellId = "outer-second";
outerSecond.dataset.columnId = "outer-column-b";
const outerSecondRun = window.document.createElement("span");
outerSecondRun.dataset.inlineId = "outer-second-run";
outerSecondRun.textContent = "Outer second";
outerSecond.append(outerSecondRun);
body.append(nestedGestureTable);
const outerRange = window.document.createRange();
outerRange.setStart(outerFirstRun.firstChild, 0);
outerRange.collapse(true);
window.getSelection().removeAllRanges();
window.getSelection().addRange(outerRange);
assert.equal(tablesModule.moveCaretToAdjacentCell(true), true, "Tab finds an outer-table sibling");
const tabFocus = window.getSelection().focusNode;
const tabFocusElement = tabFocus instanceof window.Element ? tabFocus : tabFocus.parentElement;
assert.equal(
  tabFocusElement.closest("[data-inline-id]")?.dataset.inlineId,
  "outer-second-run",
  "Tab from an outer cell skips nested-grid cells",
);
// A remote structural morph can remove the focused cell before the browser
// has discarded its old selection node. That detached grid must not consume
// Tab and move an invisible caret through stale siblings.
const detachedRange = window.document.createRange();
detachedRange.setStart(outerFirstRun.firstChild, 0);
detachedRange.collapse(true);
outerFirst.remove();
window.getSelection().removeAllRanges();
window.getSelection().addRange(detachedRange);
assert.equal(tablesModule.moveCaretToAdjacentCell(true), false, "Tab ignores a cell removed by a remote table update");
nestedGestureTable.remove();

// A table menu dialog can remain open while a collaborator removes its outer
// column. The answer must not be routed through the remembered table-cell hint
// (or, in a nested grid, accidentally retarget an inner table): it names the
// cell that was current when the dialog opened and must stop honestly once
// that exact model coordinate has gone away.
const documentBeforeStaleTableDialog = JSON.parse(JSON.stringify(__test.getState().doc));
const staleTableId = "remote-morphed-outer-table";
const staleRowId = "remote-morphed-outer-row";
const staleColumnId = "remote-morphed-outer-column";
const staleCellId = "remote-morphed-outer-cell";
const staleParagraphId = "remote-morphed-outer-paragraph";
const staleInlineId = "remote-morphed-outer-inline";
const staleNestedTableId = "remote-morphed-inner-table";
const documentWithStaleTableDialog = JSON.parse(JSON.stringify(documentBeforeStaleTableDialog));
documentWithStaleTableDialog.blocks.push({
  id: staleTableId,
  kind: "table",
  content: [],
  row_ids: [staleRowId],
  cell_ids: [[staleCellId]],
  rows: [[[
    {
      id: staleParagraphId,
      kind: "paragraph",
      content: [{ id: staleInlineId, kind: "text", text: "Outer cell", marks: [] }],
    },
    {
      id: staleNestedTableId,
      kind: "table",
      content: [],
      row_ids: ["remote-morphed-inner-row"],
      cell_ids: [["remote-morphed-inner-cell"]],
      rows: [[[{ id: "remote-morphed-inner-paragraph", kind: "paragraph", content: [{ id: "remote-morphed-inner-inline", kind: "text", text: "Inner cell", marks: [] }] }]]],
      table: { columns: [{ id: "remote-morphed-inner-column" }], cells: [[{ row_span: 1, column_span: 1, covered: false }]] },
    },
  ]]],
  table: { columns: [{ id: staleColumnId }], cells: [[{ row_span: 1, column_span: 1, covered: false }]] },
});
documentWithStaleTableDialog.body_fragments.push({
  block_id: staleTableId,
  blocks: 1,
  html: `<table class="doc-table" data-block-id="${staleTableId}"><tbody><tr data-row-id="${staleRowId}"><td data-cell-id="${staleCellId}" data-column-id="${staleColumnId}"><p data-block-id="${staleParagraphId}"><span data-inline-id="${staleInlineId}">Outer cell</span></p><table class="doc-table" data-block-id="${staleNestedTableId}"><tbody><tr data-row-id="remote-morphed-inner-row"><td data-cell-id="remote-morphed-inner-cell" data-column-id="remote-morphed-inner-column"><p data-block-id="remote-morphed-inner-paragraph"><span data-inline-id="remote-morphed-inner-inline">Inner cell</span></p></td></tr></tbody></table></td></tr></tbody></table>`,
});
__test.applyDocumentForTest(documentWithStaleTableDialog);
await settle(3);
const staleOuterRun = $(`[data-inline-id="${staleInlineId}"]`);
const staleOuterRange = window.document.createRange();
staleOuterRange.selectNodeContents(staleOuterRun);
staleOuterRange.collapse(true);
window.getSelection().removeAllRanges();
window.getSelection().addRange(staleOuterRange);
const staleColumnDialog = tablesModule.runTableAction("table-column-width");
await settle(3);
assert.ok(window.document.querySelector("dialog.modal[open]"), "column width opens against the outer table cell");
const documentAfterRemoteColumnDelete = JSON.parse(JSON.stringify(documentWithStaleTableDialog));
const staleTableBlock = documentAfterRemoteColumnDelete.blocks.find((block) => block.id === staleTableId);
staleTableBlock.table.columns = [];
staleTableBlock.table.cells = [[]];
staleTableBlock.rows = [[]];
staleTableBlock.cell_ids = [[]];
documentAfterRemoteColumnDelete.body_fragments = documentAfterRemoteColumnDelete.body_fragments.map((fragment) =>
  fragment.block_id === staleTableId
    ? { ...fragment, html: `<table class="doc-table" data-block-id="${staleTableId}"><tbody><tr data-row-id="${staleRowId}"></tr></tbody></table>` }
    : fragment,
);
__test.applyDocumentForTest(documentAfterRemoteColumnDelete);
await answerDialog({ points: "144" });
await staleColumnDialog;
assert.match($("[data-error]").textContent, /selected table column changed/i, "a remote column delete cancels the stale table dialog with a local explanation");
__test.applyDocumentForTest(documentWithStaleTableDialog);
await settle(3);
const staleRowRun = $(`[data-inline-id="${staleInlineId}"]`);
const staleRowRange = window.document.createRange();
staleRowRange.selectNodeContents(staleRowRun);
staleRowRange.collapse(true);
window.getSelection().removeAllRanges();
window.getSelection().addRange(staleRowRange);
const staleRowDialog = tablesModule.runTableAction("table-row-height");
await settle(3);
const documentAfterRemoteRowDelete = JSON.parse(JSON.stringify(documentWithStaleTableDialog));
const staleRowTableBlock = documentAfterRemoteRowDelete.blocks.find((block) => block.id === staleTableId);
staleRowTableBlock.row_ids = [];
staleRowTableBlock.table.cells = [];
staleRowTableBlock.rows = [];
staleRowTableBlock.cell_ids = [];
documentAfterRemoteRowDelete.body_fragments = documentAfterRemoteRowDelete.body_fragments.map((fragment) =>
  fragment.block_id === staleTableId
    ? { ...fragment, html: `<table class="doc-table" data-block-id="${staleTableId}"></table>` }
    : fragment,
);
__test.applyDocumentForTest(documentAfterRemoteRowDelete);
await answerDialog({ points: "36" });
await staleRowDialog;
assert.match($("[data-error]").textContent, /selected table row changed/i, "a remote row delete cancels the stale table dialog with a local explanation");
__test.applyDocumentForTest(documentWithStaleTableDialog);
await settle(3);
const staleCellRun = $(`[data-inline-id="${staleInlineId}"]`);
const staleCellRange = window.document.createRange();
staleCellRange.selectNodeContents(staleCellRun);
staleCellRange.collapse(true);
window.getSelection().removeAllRanges();
window.getSelection().addRange(staleCellRange);
const staleCellDialog = tablesModule.runTableAction("table-cell-background");
await settle(3);
assert.ok(window.document.querySelector("dialog.modal[open]"), "cell background opens against the outer table cell");
__test.applyDocumentForTest(documentAfterRemoteRowDelete);
await answerDialog({ color: "#ffcc00" });
await staleCellDialog;
assert.match($("[data-error]").textContent, /selected table cell changed/i, "a remote cell delete cancels a stale cell-style dialog with a local explanation");
__test.applyDocumentForTest(documentBeforeStaleTableDialog);
await settle(3);

// Following a collaborator is one user-requested scroll, not a subscription:
// their later typing cannot move this viewport. The anchor is re-resolved on
// click, so a stale service presence frame becomes a visible no-op.
const localActor = "local-actor";
const remoteActor = "remote-actor";
const liveCollab = (cursorAnchor) => ({
  phase: "live",
  document_uuid: state.doc.uuid,
  display_name: "Local",
  commit_seq: 0,
  acknowledged_seq: 0,
  pending_operations: 0,
  can_submit: true,
  reconnect_requested: false,
  document_changed: false,
  selection: null,
  notice: null,
  session: {
    subject: "local-subject",
    actor: localActor,
    document_uuid: state.doc.uuid,
    role: "editor",
    acknowledged_seq: 0,
    peers: [{ subject: "remote-subject", actor: remoteActor, display_name: "Remote Ada", role: "editor", cursor_anchor: cursorAnchor, selection_anchor: null, last_seen_ms: 1, connections: 1 }],
  },
});
// Blob ownership and browser decoding are separate: a malformed or locally
// unsupported image must become a visible named placeholder, not an invisible
// broken-image icon. The source model is untouched, so Save original image
// still has its raw bytes.
const failedImageFigure = window.document.createElement("figure");
failedImageFigure.className = "doc-image";
failedImageFigure.setAttribute("contenteditable", "false");
failedImageFigure.setAttribute("data-block-id", firstBlock.id);
failedImageFigure.setAttribute("aria-label", "Image: Chart source");
const failedImage = window.document.createElement("img");
failedImage.dataset.blobHash = "sha256:malformed";
failedImage.alt = "Chart source";
failedImage.setAttribute("style", "width: 72pt;");
failedImageFigure.append(failedImage);
body.append(failedImageFigure);
failedImage.dispatchEvent(new window.Event("error"));
assert.equal(failedImageFigure.getAttribute("aria-label"), "Image unavailable: Chart source", "a decode failure updates the object name");
const failedPlaceholder = failedImageFigure.querySelector(".doc-image-placeholder");
assert.ok(failedPlaceholder, "a decode failure is visible rather than a broken-image icon");
assert.equal(failedPlaceholder.textContent, "Chart source", "the fallback preserves authored alternative text");
assert.equal(failedPlaceholder.getAttribute("style"), "width: 72pt;", "the fallback preserves rendered geometry");
failedImageFigure.remove();

// Atomic figures are Tab-focusable, but Escape must cancel their selection.
// Keeping its Range after blur would leave Format > Image armed for an object
// the keyboard user explicitly left.
const keyboardImage = window.document.createElement("figure");
keyboardImage.className = "doc-image";
keyboardImage.tabIndex = 0;
keyboardImage.setAttribute("contenteditable", "false");
keyboardImage.setAttribute("data-block-id", firstBlock.id);
body.append(keyboardImage);
const imageRange = window.document.createRange();
imageRange.selectNode(keyboardImage);
const imageSelection = window.getSelection();
imageSelection.removeAllRanges();
imageSelection.addRange(imageRange);
keyboardImage.focus();
const imageEscape = new window.KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true });
keyboardImage.dispatchEvent(imageEscape);
await settle(3);
assert.equal(imageEscape.defaultPrevented, true, "Escape is handled for a focused image");
assert.equal(imageSelection.rangeCount, 0, "Escape clears the atomic image selection");
assert.equal(__test.getState().selection, null, "a deselected image cannot remain a menu target");
keyboardImage.remove();

// A remote projection can delete an image while it owns keyboard focus. The
// keyed fragment morph must not leave a detached figure as the active object
// or retain its durable selection for the toolbar.
const remoteImageId = "remote-deleted-image";
const documentWithFocusedImage = JSON.parse(JSON.stringify(__test.getState().doc));
documentWithFocusedImage.blocks.push({ id: remoteImageId, kind: "image", content: [] });
documentWithFocusedImage.body_fragments.push({
  block_id: remoteImageId,
  blocks: 1,
  html: `<figure class="doc-image" data-block-id="${remoteImageId}" contenteditable="false" tabindex="0" aria-label="Image: remote chart"><div class="doc-image-placeholder">remote chart</div></figure>`,
});
__test.applyDocumentForTest(documentWithFocusedImage);
await settle(3);
const remotelyDeletedImage = $(`[data-block-id="${remoteImageId}"]`);
remotelyDeletedImage.focus();
const remoteImageRange = window.document.createRange();
remoteImageRange.selectNode(remotelyDeletedImage);
window.getSelection().removeAllRanges();
window.getSelection().addRange(remoteImageRange);
__test.setSelection({
  anchor: { block_id: remoteImageId, inline_id: null, offset: 0 },
  focus: { block_id: remoteImageId, inline_id: null, offset: 1 },
});
// Size, like position, captures the atomic object's durable id. A remote
// deletion while its dialog is open must not dispatch a stale size edit.
const staleSizeDialog = imagesModule.promptImageSize();
await settle(3);
assert.ok(window.document.querySelector("dialog.modal[open]"), "sizing opens for the selected atomic image");
const documentAfterRemoteImageDelete = JSON.parse(JSON.stringify(documentWithFocusedImage));
documentAfterRemoteImageDelete.blocks = documentAfterRemoteImageDelete.blocks.filter((block) => block.id !== remoteImageId);
documentAfterRemoteImageDelete.body_fragments = documentAfterRemoteImageDelete.body_fragments.filter((fragment) => fragment.block_id !== remoteImageId);
__test.applyDocumentForTest(documentAfterRemoteImageDelete);
await answerDialog({ width: "72", height: "36" });
await staleSizeDialog;
assert.match($("[data-error]").textContent, /image was deleted or changed while its size dialog was open/i, "a remote image deletion cancels a stale size dialog locally");

// Restore and select the same fixture image so the positioned-image regression
// remains independently covered too.
__test.applyDocumentForTest(documentWithFocusedImage);
await settle(3);
const restoredRemoteImage = $(`[data-block-id="${remoteImageId}"]`);
restoredRemoteImage.focus();
const restoredRemoteImageRange = window.document.createRange();
restoredRemoteImageRange.selectNode(restoredRemoteImage);
window.getSelection().removeAllRanges();
window.getSelection().addRange(restoredRemoteImageRange);
__test.setSelection({
  anchor: { block_id: remoteImageId, inline_id: null, offset: 0 },
  focus: { block_id: remoteImageId, inline_id: null, offset: 1 },
});
// A position dialog captures this image's durable id. If a remote projection
// removes it before Apply, the answer must stop locally rather than dispatch
// an image-layout command for an object that no longer exists.
const stalePositionDialog = imagesModule.promptImagePosition();
await settle(3);
assert.ok(window.document.querySelector("dialog.modal[open]"), "positioning opens for the selected atomic image");
__test.applyDocumentForTest(documentAfterRemoteImageDelete);
await answerDialog({ anchor: "", horizontal: "12", vertical: "24", layer: "behind-text" });
await stalePositionDialog;
assert.match($("[data-error]").textContent, /image was deleted or changed while its position dialog was open/i, "a remote image deletion cancels a stale position dialog locally");
await settle(3);
assert.equal(window.document.querySelector(`[data-block-id="${remoteImageId}"]`), null, "the remote projection removes its image element");
const selectionAfterRemoteImageDelete = window.getSelection();
assert.ok(
  selectionAfterRemoteImageDelete.rangeCount === 0 || body.contains(selectionAfterRemoteImageDelete.getRangeAt(0).commonAncestorContainer),
  "a deleted focused image leaves no detached DOM range",
);
assert.equal(__test.getState().selection, null, "a deleted focused image leaves no stale command target");
assert.equal(window.document.activeElement, body, "focus returns to the document editor after the selected image disappears");

// Horizontal rules and block equations use the same model-atomic selection,
// but are not image controls. Escape must clear their selected range too.
const keyboardRule = window.document.createElement("hr");
keyboardRule.className = "doc-horizontal-rule";
keyboardRule.tabIndex = 0;
keyboardRule.setAttribute("contenteditable", "false");
keyboardRule.setAttribute("data-block-id", firstBlock.id);
keyboardRule.setAttribute("aria-label", "Horizontal rule");
body.append(keyboardRule);
keyboardRule.focus();
await settle(3);
const ruleSelection = window.getSelection();
assert.equal(ruleSelection.rangeCount, 1, "focusing a non-image atomic object selects its whole block");
const ruleEscape = new window.KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true });
keyboardRule.dispatchEvent(ruleEscape);
await settle(3);
assert.equal(ruleEscape.defaultPrevented, true, "Escape is handled for a focused horizontal rule");
assert.equal(ruleSelection.rangeCount, 0, "Escape clears a non-image atomic selection");
assert.equal(__test.getState().selection, null, "a deselected horizontal rule cannot remain a command target");
keyboardRule.remove();

// A block equation is a focusable outer group around non-focusable MathML.
// Exercise that actual nesting shape: Escape belongs to the atomic group, not
// to an imagined text caret inside the formula.
const keyboardEquation = window.document.createElement("div");
keyboardEquation.className = "doc-equation-block";
keyboardEquation.tabIndex = 0;
keyboardEquation.setAttribute("contenteditable", "false");
keyboardEquation.setAttribute("data-block-id", firstBlock.id);
keyboardEquation.setAttribute("role", "group");
keyboardEquation.setAttribute("aria-label", "Block equation");
keyboardEquation.innerHTML = '<span class="equation equation-block" contenteditable="false"><math display="block"><mi>x</mi></math></span>';
body.append(keyboardEquation);
keyboardEquation.focus();
await settle(3);
const equationSelection = window.getSelection();
assert.equal(equationSelection.rangeCount, 1, "focusing a block equation selects its whole atomic group");
const equationEscape = new window.KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true });
keyboardEquation.dispatchEvent(equationEscape);
await settle(3);
assert.equal(equationEscape.defaultPrevented, true, "Escape is handled for a focused block equation");
assert.equal(equationSelection.rangeCount, 0, "Escape clears a block equation's atomic selection");
assert.equal(__test.getState().selection, null, "a deselected block equation cannot remain a command target");
keyboardEquation.remove();

// A deleted positioned-image anchor falls back to page content in every
// projection. The editor cannot toast on every reflow, but its affected image
// must expose that fallback rather than silently appearing to retain an anchor.
const missingAnchorImage = window.document.createElement("figure");
missingAnchorImage.className = "doc-image";
missingAnchorImage.setAttribute("contenteditable", "false");
missingAnchorImage.setAttribute("data-block-id", firstBlock.id);
missingAnchorImage.dataset.positioned = "true";
missingAnchorImage.dataset.positionAnchor = "block:deleted-anchor";
missingAnchorImage.dataset.positionXTwips = "0";
missingAnchorImage.dataset.positionYTwips = "0";
missingAnchorImage.setAttribute("aria-description", "Press Escape to deselect the image.");
body.append(missingAnchorImage);
window.dispatchEvent(new window.Event("resize"));
await settle(3);
assert.equal(missingAnchorImage.dataset.positionAnchorFallback, "true", "a missing positioned anchor is explicitly exposed in the editor");
assert.match(missingAnchorImage.getAttribute("aria-description"), /Position anchor is unavailable/, "the editor names its page-content fallback to assistive technology");
missingAnchorImage.dataset.positionAnchor = "page-content";
window.dispatchEvent(new window.Event("resize"));
await settle(3);
assert.equal(missingAnchorImage.dataset.positionAnchorFallback, undefined, "a resolved page-content anchor clears the fallback state");
assert.equal(missingAnchorImage.getAttribute("aria-description"), "Press Escape to deselect the image.", "clearing fallback restores the renderer's original object instructions");
missingAnchorImage.remove();

// ---- Typing through beforeinput ---------------------------------------------
placeCaret(firstBlock.content[0].id, 0);
beforeInput(body, "insertText", "Hello");
await settle(10);
beforeInput(body, "insertText", " world");
await settle(10);
state = __test.getState();
assert.equal(state.doc.blocks[0].content[0].text, "Hello world", "typed text reaches the Rust core");
assert.ok(body.textContent.includes("Hello world"), "DOM is patched from Rust HTML");

const followedBlock = state.doc.blocks[0];
const followedInline = followedBlock.content[0];
// JSDOM does not lay text out, so give the viewport-only overlay one stable
// range rectangle. The production resolver still owns endpoint validation.
const originalRangeRect = window.Range.prototype.getBoundingClientRect;
const originalRangeRects = window.Range.prototype.getClientRects;
window.Range.prototype.getBoundingClientRect = () => ({ left: 40, top: 60, width: 2, height: 16 });
window.Range.prototype.getClientRects = () => [];
__test.setCollaborationStatus(liveCollab(`${followedBlock.id}:${followedInline.id}:0`));
await new Promise((resolve) => setTimeout(resolve, 25));
const remoteOverlay = $("#opendoc-remote-presence-overlay");
assert.ok(remoteOverlay.querySelector(`[data-remote-caret="${remoteActor}"]`), "a live remote cursor paints outside the editor");
const documentBeforeRemoteCursorDelete = JSON.parse(JSON.stringify(__test.getState().doc));
const documentAfterRemoteCursorDelete = JSON.parse(JSON.stringify(documentBeforeRemoteCursorDelete));
documentAfterRemoteCursorDelete.blocks = [];
documentAfterRemoteCursorDelete.body_fragments = [];
__test.applyDocumentForTest(documentAfterRemoteCursorDelete);
assert.equal(remoteOverlay.childElementCount, 0, "a remote document delete clears stale caret rectangles before the next frame");
__test.applyDocumentForTest(documentBeforeRemoteCursorDelete);
await settle(3);
window.Range.prototype.getBoundingClientRect = originalRangeRect;
window.Range.prototype.getClientRects = originalRangeRects;
const remotePresenceChip = $(`[data-collab-peer="${remoteActor}"]`);
assert.equal(remotePresenceChip.querySelector(".collab-peer-mark")?.getAttribute("aria-hidden"), "true", "decorative peer initials do not duplicate the collaborator name for a screen reader");
assert.match(remotePresenceChip.querySelector(".sr-only")?.textContent, /Role: editor\. Current text cursor available\./, "peer presence exposes the service-attested role and follow availability without relying on a title tooltip");
lastScrolledElement = null;
click(`[data-collab-action="follow-once"][data-collab-actor="${remoteActor}"]`);
assert.equal(lastScrolledElement, body.querySelector(`[data-inline-id="${followedInline.id}"]`), "Follow scrolls once to the currently resolved remote cursor");
__test.setCollaborationStatus(liveCollab("missing-block:missing-inline:0"));
lastScrolledElement = null;
click(`[data-collab-action="follow-once"][data-collab-actor="${remoteActor}"]`);
assert.equal(lastScrolledElement, null, "a stale remote cursor never causes a guessed scroll");
assert.equal([...window.document.querySelectorAll(".toast")].at(-1)?.textContent, "Remote Ada's cursor is no longer available.", "stale follow is explained to the local reader");
// A delegated click may race a service frame that removes the peer entirely.
// Keep an old-looking button in the live region to exercise that click-time
// state recheck rather than relying on the re-rendered (now absent) control.
const peerGone = liveCollab(null);
peerGone.session.peers = [];
__test.setCollaborationStatus(peerGone);
const stalePeerFollow = window.document.createElement("button");
stalePeerFollow.dataset.collabAction = "follow-once";
stalePeerFollow.dataset.collabActor = remoteActor;
$("[data-collab]").append(stalePeerFollow);
stalePeerFollow.click();
assert.equal([...window.document.querySelectorAll(".toast")].at(-1)?.textContent, "That collaborator is no longer available.", "a departed peer is explained rather than silently ignoring stale Follow");
__test.setCollaborationStatus(liveCollab(null));
const unavailableFollow = $(`[data-collab-action="follow-once"][data-collab-actor="${remoteActor}"]`);
assert.equal(unavailableFollow.hasAttribute("disabled"), true, "a peer without a cursor cannot offer a misleading follow action");
__test.setCollaborationStatus(liveCollab("not-a-text-cursor"));
const malformedFollow = $(`[data-collab-action="follow-once"][data-collab-actor="${remoteActor}"]`);
assert.equal(malformedFollow.hasAttribute("disabled"), true, "a malformed presence anchor cannot advertise a follow destination");
assert.match(malformedFollow.getAttribute("aria-label"), /no current cursor/i, "the disabled follow explanation remains available to assistive technology");
__test.setCollaborationStatus({ phase: "idle", document_uuid: "", display_name: "", commit_seq: 0, acknowledged_seq: 0, pending_operations: 0, can_submit: false, reconnect_requested: false, document_changed: false, selection: null, notice: null, session: null });

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
click("[data-action='style:heading:6']");
await settle(10);
state = __test.getState();
assert.equal(state.doc.blocks[0].kind, "heading");
assert.equal(state.doc.blocks[0].level, 6, "all model-supported heading levels are reachable in the UI");

// Opening a native colour picker can move focus before its change event. The
// paragraph background must retain the paragraph selected when it opened, not
// the block that became current during that browser interaction.
const paragraphBackground = $("[data-toolbar] input[data-paragraph-background]");
paragraphBackground.dispatchEvent(new window.Event("pointerdown", { bubbles: true }));
__test.setSelection({
  anchor: { block_id: "focus-moved-away", inline_id: "focus-moved-away", offset: 0 },
  focus: { block_id: "focus-moved-away", inline_id: "focus-moved-away", offset: 0 },
});
paragraphBackground.value = "#ff0000";
paragraphBackground.dispatchEvent(new window.Event("change", { bubbles: true }));
await settle(10);
state = __test.getState();
assert.equal(state.doc.blocks[0].properties.background, "#ff0000", "paragraph background keeps the selection present when its picker opened");
__test.setSelection({
  anchor: { block_id: state.doc.blocks[0].id, inline_id: state.doc.blocks[0].content[0].id, offset: 0 },
  focus: { block_id: state.doc.blocks[0].id, inline_id: state.doc.blocks[0].content[0].id, offset: 0 },
});
const paragraphSpaceBefore = $("[data-toolbar] [data-select='space-before']");
paragraphSpaceBefore.dispatchEvent(new window.Event("pointerdown", { bubbles: true }));
__test.setSelection({
  anchor: { block_id: "focus-moved-away", inline_id: "focus-moved-away", offset: 0 },
  focus: { block_id: "focus-moved-away", inline_id: "focus-moved-away", offset: 0 },
});
paragraphSpaceBefore.value = "120";
paragraphSpaceBefore.dispatchEvent(new window.Event("change", { bubbles: true }));
await settle(10);
state = __test.getState();
assert.equal(state.doc.blocks[0].properties.space_before_twips, 120, "paragraph spacing keeps the selection present when its picker opened");
__test.setSelection({
  anchor: { block_id: state.doc.blocks[0].id, inline_id: state.doc.blocks[0].content[0].id, offset: 0 },
  focus: { block_id: state.doc.blocks[0].id, inline_id: state.doc.blocks[0].content[0].id, offset: 0 },
});

// Bookmarks name a whole stable block. Reusing a name retargets its existing
// durable record rather than manufacturing a competing live name.
click("[data-action='insert-bookmark']");
await answerDialog({ name: "intro_target" });
await settle(10);
state = __test.getState();
const bookmark = state.doc.bookmarks.find((item) => item.name === "intro_target" && !item.deleted);
assert.ok(bookmark, "Insert > Bookmark writes a durable bookmark");
assert.equal(bookmark.block_id, state.doc.blocks[0].id, "bookmark targets the focused block");
assert.ok(body.querySelector('#intro_target.doc-bookmark-anchor'), "live bookmark projects to its named HTML anchor");
__test.setSelection({
  anchor: { block_id: state.doc.blocks[1].id, inline_id: state.doc.blocks[1].content[0].id, offset: 0 },
  focus: { block_id: state.doc.blocks[1].id, inline_id: state.doc.blocks[1].content[0].id, offset: 0 },
});
click("[data-action='insert-bookmark']");
await answerDialog({ name: "intro_target" });
await settle(10);
state = __test.getState();
const retargetedBookmark = state.doc.bookmarks.find((item) => item.name === "intro_target" && !item.deleted);
assert.equal(retargetedBookmark?.id, bookmark.id, "reused bookmark name retains its stable identity");
assert.equal(retargetedBookmark?.block_id, state.doc.blocks[1].id, "reused bookmark name retargets the selected block");
// A model-valid target can briefly have no DOM fragment while a remote keyed
// render is reconciling. Navigation must name that transient state, not
// silently claim the bookmark was followed.
const documentBeforeMissingBookmarkFragment = JSON.parse(JSON.stringify(state.doc));
const missingBookmarkTarget = $(`[data-block-id="${retargetedBookmark.block_id}"]`);
missingBookmarkTarget.remove();
lastScrolledElement = null;
await __test.runAction("bookmark-go", { blockId: retargetedBookmark.block_id });
assert.equal(lastScrolledElement, null, "a bookmark never scrolls to a guessed replacement while its fragment is absent");
assert.match($("[data-error]").textContent, /bookmark's target is not currently rendered/i, "a model-valid bookmark with no rendered target explains the transient state");
__test.applyDocumentForTest(documentBeforeMissingBookmarkFragment);
await settle(3);

// A concurrently deleted selection must not make a block insertion silently
// fall back to the document's final block. TOC, bookmarks, and notes share
// this target resolver, so exercise the atomic TOC insertion here.
const blocksBeforeStaleToc = state.doc.blocks.length;
__test.setSelection({
  anchor: { block_id: "deleted-selection", inline_id: "deleted-selection", offset: 0 },
  focus: { block_id: "deleted-selection", inline_id: "deleted-selection", offset: 0 },
});
await __test.runAction("insert-table-of-contents");
await settle(5);
state = __test.getState();
assert.equal(state.doc.blocks.length, blocksBeforeStaleToc, "a stale selection cannot insert a TOC at an unrelated fallback block");
assert.match($("[data-error]").textContent, /selected content was deleted or changed/i, "stale insertion explains why it stopped");
__test.setSelection({
  anchor: { block_id: state.doc.blocks[1].id, inline_id: state.doc.blocks[1].content[0].id, offset: 0 },
  focus: { block_id: state.doc.blocks[1].id, inline_id: state.doc.blocks[1].content[0].id, offset: 0 },
});

click("[data-action='toggle-panel:outline']");
await settle(5);
assert.match($("[data-side-panel]").textContent, /Hello world/, "outline projects document headings");
// The close affordance is deliberately an idempotent close, not a second
// toggle tied to whichever panel happened to render during the click.
click("[data-action='close-panel']");
await settle(2);
assert.equal($("[data-side-panel]").hidden, true, "the side-panel close control hides the right panel");
click("[data-action='toggle-panel:outline']");
await settle(2);
// A footnote reference is a structural marker, not heading title text. Its
// DTO fallback carries an internal note ID, which must never escape into the
// reader-facing outline landmark.
const documentBeforeOutlineFootnote = JSON.parse(JSON.stringify(__test.getState().doc));
const outlineFootnoteId = "outline-footnote-internal-id";
const headingForOutline = documentBeforeOutlineFootnote.blocks.find((block) => block.kind === "heading");
assert.ok(headingForOutline, "smoke document has a heading for outline projection");
headingForOutline.content.push({
  id: "outline-footnote-ref",
  kind: "footnote-ref",
  text: `[${outlineFootnoteId}]`,
  target_id: outlineFootnoteId,
  mark_values: {},
});
__test.applyDocumentForTest(documentBeforeOutlineFootnote);
await settle(5);
assert.doesNotMatch($("[data-side-panel]").textContent, new RegExp(outlineFootnoteId), "outline omits opaque footnote identifiers from heading labels");
__test.applyDocumentForTest(JSON.parse(JSON.stringify(state.doc)));
await settle(5);
const outlineButton = $("[data-side-panel]").querySelector("[data-action='outline-go']");
assert.ok(outlineButton, "outline headings are navigable");
const staleOutlineTarget = outlineButton.dataset.id;
assert.ok(staleOutlineTarget, "outline navigation names a durable heading block");
// Rendering removes a deleted heading from the outline, but a delegated click
// can have been queued before that render. Its old action must report the
// concurrent loss, not silently scroll nowhere or to a restyled block.
const afterOutlineDelete = JSON.parse(wasmGlue.dispatch("delete_block", JSON.stringify({ blockId: staleOutlineTarget })));
__test.applyDocumentForTest(afterOutlineDelete.value);
await settle(10);
assert.equal($("[data-side-panel]").querySelector("[data-action='outline-go']"), null, "a deleted heading leaves no live outline control");
await __test.runAction("outline-go", { id: staleOutlineTarget });
assert.match($("[data-error]").textContent, /outline heading was deleted or changed/, "a raced outline navigation explains its stale target");
const afterOutlineRestore = JSON.parse(wasmGlue.dispatch("undo_current_edit", "{}"));
__test.applyDocumentForTest(afterOutlineRestore.value);
await settle(10);

click("[data-action='toggle-panel:bookmarks']");
await settle(5);
assert.match($("[data-side-panel]").textContent, /intro_target/, "bookmarks panel lists live durable names");
assert.equal(
  $("[data-side-panel]").querySelector("[data-action='bookmark-go']")?.dataset.blockId,
  state.doc.blocks[1].id,
  "bookmark navigation names the stable retargeted block",
);
// A concurrent delete may leave the durable bookmark valid but its stable
// target absent. The panel must explain that state rather than offer a button
// that silently fails; a raced direct action gives the same honest answer.
const staleBookmarkTarget = retargetedBookmark?.block_id;
assert.ok(staleBookmarkTarget, "the retargeted bookmark has a durable target id");
const afterTargetDelete = JSON.parse(wasmGlue.dispatch("delete_block", JSON.stringify({ blockId: staleBookmarkTarget })));
__test.applyDocumentForTest(afterTargetDelete.value);
await settle(10);
const staleBookmarkButton = $("[data-side-panel]").querySelector("[data-action='bookmark-go']");
assert.ok(staleBookmarkButton?.disabled, "a deleted bookmark target disables navigation");
assert.match($("[data-side-panel]").textContent, /Target deleted/, "the stale target is explained in the bookmarks panel");
await __test.runAction("bookmark-go", { blockId: staleBookmarkTarget });
assert.match($("[data-error]").textContent, /bookmark's target was deleted/, "a stale navigation event explains why it cannot navigate");
const afterTargetRestore = JSON.parse(wasmGlue.dispatch("undo_current_edit", "{}"));
__test.applyDocumentForTest(afterTargetRestore.value);
await settle(10);
click("[data-side-panel] [data-action='bookmark-delete']");
assert.match($("dialog.modal").textContent, /Delete bookmark\?/, "bookmark deletion asks for confirmation");
await answerDialog();
await settle(10);
state = __test.getState();
assert.ok(state.doc.bookmarks.some((item) => item.id === bookmark.id && item.deleted), "bookmark panel writes the durable tombstone");

// Opening a representable furniture slot must not normalize away authored
// whitespace: this textarea is an editor for model text, not a label.
const paddedFurniture = "  Padded header  \n\nFinal  ";
click("[data-action='page-furniture:header']");
await answerDialog({ text: paddedFurniture, field: "none", alignment: "start" });
await settle(10);
click("[data-action='page-furniture:header']");
await settle(5);
assert.equal(
  $("dialog.modal textarea[name='text']").value,
  paddedFurniture,
  "opening the plain furniture dialog preserves paragraph whitespace and trailing blank structure",
);
await answerDialog();
await settle(10);
state = __test.getState();
assert.equal(
  state.doc.header.map((block) => block.content.filter((inline) => inline.kind === "text").map((inline) => inline.text).join("")).join("\n"),
  paddedFurniture,
  "applying the opened furniture dialog retains exact representable text",
);

// First/even variants distinguish absent inheritance from an explicit empty
// override. The dialog must expose the distinction and restore absence, not
// convert inheritance into suppression as a side effect of Apply or undo.
click("[data-action='page-furniture:first-page-header']");
await settle(5);
assert.equal($("dialog.modal select[name='variantMode']").value, "inherit", "an absent first-page slot is shown as inheriting ordinary furniture");
// The dialog must re-read override presence on Apply: a collaborator can add
// one after an inherited dialog opens, and an explicit Inherit choice must
// still remove that new variant rather than using the stale open-time flag.
const firstHeaderAddedWhileDialogOpen = JSON.parse(wasmGlue.dispatch("set_page_furniture", JSON.stringify({ slot: "first-page-header", text: "Remote first page", field: "none", alignment: "start" })));
__test.applyDocumentForTest(firstHeaderAddedWhileDialogOpen.value);
await answerDialog({ variantMode: "inherit" });
await settle(10);
state = __test.getState();
assert.equal(state.doc.first_page_header, undefined, "inherit removes a first-page override added while its dialog was open");
click("[data-action='page-furniture:first-page-header']");
await settle(5);
assert.equal($("dialog.modal select[name='variantMode']").value, "inherit", "the cleared first-page slot remains inherited");
await answerDialog({ text: "First page", field: "none", alignment: "start", variantMode: "override" });
await settle(10);
state = __test.getState();
assert.ok(Array.isArray(state.doc.first_page_header), "writing a first-page variant creates an explicit override");
click("[data-action='page-furniture:first-page-header']");
await settle(5);
assert.equal($("dialog.modal select[name='variantMode']").value, "override", "an explicit first-page fragment remains distinguishable from inheritance");
await answerDialog({ variantMode: "inherit" });
await settle(10);
state = __test.getState();
assert.equal(state.doc.first_page_header, undefined, "choosing inherit removes the variant instead of suppressing ordinary furniture");
// The preservation dialog for a rich variant must also offer inheritance. A
// rich fragment cannot be sent through the plain form merely to remove it.
const richFirstHeader = JSON.parse(wasmGlue.dispatch("set_page_furniture_html", JSON.stringify({ slot: "first-page-header", html: "<p><strong>Rich first page</strong></p>" })));
__test.applyDocumentForTest(richFirstHeader.value);
await settle(10);
click("[data-action='page-furniture:first-page-header']");
await settle(5);
assert.match($("dialog.modal").textContent, /Inherit ordinary header\/footer/, "a rich first-page override can restore inheritance without flattening it first");
await answerDialog({ action: "inherit" });
await settle(10);
state = __test.getState();
assert.equal(state.doc.first_page_header, undefined, "rich variant inheritance removes the override rather than replacing it with an empty fragment");

// Suggest mode must record an attributed proposal instead of editing the
// document directly. This exercises the top-bar mode control and the same
// beforeinput route ordinary typing uses.
const editingMode = $("[data-document-editing-mode]");
editingMode.value = "suggest";
editingMode.dispatchEvent(new window.Event("change", { bubbles: true }));
// Paragraph-style changes carry both reviewed values, but do not write the
// source before a reviewer accepts them.
__test.setSelection({
  anchor: { block_id: state.doc.blocks[0].id, inline_id: state.doc.blocks[0].content[0].id, offset: 0 },
  focus: { block_id: state.doc.blocks[0].id, inline_id: state.doc.blocks[0].content[0].id, offset: 0 },
});
const styleBeforeSuggestedChange = state.doc.blocks[0].style_value;
const suggestionsBeforeStyleSuggestion = state.doc.suggestions.length;
const styleSelect = $("[data-select='style']");
styleSelect.value = "heading:2";
styleSelect.dispatchEvent(new window.Event("change", { bubbles: true }));
await settle(10);
state = __test.getState();
assert.equal(state.doc.blocks[0].style_value, styleBeforeSuggestedChange, "Suggest-mode paragraph style selection leaves source unchanged");
assert.equal(state.doc.suggestions.length, suggestionsBeforeStyleSuggestion + 1, "Suggest-mode paragraph style selection creates one proposal");
assert.equal(state.doc.suggestions.at(-1).kind, "paragraph_style_change", "style proposal has its dedicated review kind");
assert.equal(state.doc.suggestions.at(-1).paragraph_style_proposed, "heading:2", "style proposal retains its proposed style");
placeCaret(state.doc.blocks[0].content[0].id, 0);
const sourceBeforeSuggestion = state.doc.blocks[0].content.map((inline) => inline.text).join("");
const suggestionsBefore = state.doc.suggestions.length;
beforeInput(body, "insertText", "Proposed ");
await settle(10);
state = __test.getState();
assert.equal(state.doc.suggestions.length, suggestionsBefore + 1, "Suggest mode creates a proposal");
assert.equal(state.doc.blocks[0].content.map((inline) => inline.text).join(""), sourceBeforeSuggestion, "Suggest mode leaves source text unchanged");
// A plain one-line paste has the same lossless representation as ordinary
// typing: one text insertion proposal. Rich or multi-paragraph clipboard
// content deliberately remains outside the current whole-inline vocabulary.
const suggestionsBeforePasteSuggestion = state.doc.suggestions.length;
pastePlainText(body, " pasted");
await settle(10);
state = __test.getState();
assert.equal(state.doc.suggestions.length, suggestionsBeforePasteSuggestion + 1, "Suggest mode records a plain-text paste as a proposal");
assert.equal(state.doc.suggestions.at(-1)?.content.map((inline) => inline.text).join(""), " pasted", "paste proposal retains its complete plain text");
assert.equal(state.doc.blocks[0].content.map((inline) => inline.text).join(""), sourceBeforeSuggestion, "Suggest-mode paste leaves source text unchanged");
// A whole run is the finest range the current Format suggestion model names.
// The normal Bold action must create that proposal rather than change source.
const unboldedRun = state.doc.blocks[0].content.find((inline) => inline.text === " world");
assert.ok(unboldedRun, "an unformatted run remains available for a format suggestion");
__test.setSelection({
  anchor: { block_id: state.doc.blocks[0].id, inline_id: unboldedRun.id, offset: 0 },
  focus: { block_id: state.doc.blocks[0].id, inline_id: unboldedRun.id, offset: Array.from(unboldedRun.text).length },
});
const sourceBeforeFormatSuggestion = state.doc.blocks[0].content.map((inline) => inline.text).join("");
click("[data-action='mark:bold']");
await settle(10);
state = __test.getState();
const formatSuggestion = state.doc.suggestions.at(-1);
assert.equal(formatSuggestion?.kind, "format", "Suggest-mode Bold creates a format suggestion");
assert.ok(formatSuggestion?.marks.some((mark) => mark.startsWith("bold:")), "the proposal records the requested bold mark");
assert.equal(state.doc.blocks[0].content.map((inline) => inline.text).join(""), sourceBeforeFormatSuggestion, "Suggest-mode Bold does not change source text");
// Browser/native formatting gestures (including accessibility affordances)
// take the beforeinput route rather than the Ctrl-key shortcut route. They
// must create the same safe whole-run boolean proposal.
const suggestionsBeforeNativeFormat = state.doc.suggestions.length;
beforeInput(body, "formatItalic");
await settle(10);
state = __test.getState();
assert.equal(state.doc.suggestions.length, suggestionsBeforeNativeFormat + 1, "Suggest mode records a native format gesture as a proposal");
assert.equal(state.doc.suggestions.at(-1)?.kind, "format", "native format gesture creates a format proposal");
assert.ok(state.doc.suggestions.at(-1)?.marks.some((mark) => mark.startsWith("italic:")), "native format gesture retains the requested italic mark");
assert.equal(state.doc.blocks[0].content.map((inline) => inline.text).join(""), sourceBeforeFormatSuggestion, "native Suggest-mode formatting does not change source text");
// `FormatRemove` already has whole-run accept/reject semantics for a
// value-bearing mark. Give this run a direct source colour, then ensure the
// Format menu proposes replacing or clearing it without touching the source
// in Suggest mode.
editingMode.value = "edit";
editingMode.dispatchEvent(new window.Event("change", { bubbles: true }));
__test.setSelection({
  anchor: { block_id: state.doc.blocks[0].id, inline_id: unboldedRun.id, offset: 0 },
  focus: { block_id: state.doc.blocks[0].id, inline_id: unboldedRun.id, offset: Array.from(unboldedRun.text).length },
});
const sourceColour = $("[data-toolbar] input[data-color='color']");
sourceColour.value = "#123456";
sourceColour.dispatchEvent(new window.Event("change", { bubbles: true }));
await settle(10);
state = __test.getState();
const colouredRun = state.doc.blocks[0].content.find((inline) => inline.text === " world");
assert.ok(colouredRun?.marks.some((mark) => mark === "color:#123456:none"), "the removal fixture has a durable source colour");
editingMode.value = "suggest";
editingMode.dispatchEvent(new window.Event("change", { bubbles: true }));
__test.setSelection({
  anchor: { block_id: state.doc.blocks[0].id, inline_id: colouredRun.id, offset: 0 },
  focus: { block_id: state.doc.blocks[0].id, inline_id: colouredRun.id, offset: Array.from(colouredRun.text).length },
});
const suggestionsBeforeColourReplacement = state.doc.suggestions.length;
const replacementColour = $("[data-toolbar] input[data-color='color']");
replacementColour.value = "#654321";
replacementColour.dispatchEvent(new window.Event("change", { bubbles: true }));
await settle(10);
state = __test.getState();
const colourReplacementSuggestion = state.doc.suggestions.at(-1);
assert.equal(state.doc.suggestions.length, suggestionsBeforeColourReplacement + 1, `Suggest-mode text-colour replacement creates one proposal (error: ${state.lastError ?? "none"})`);
assert.equal(colourReplacementSuggestion?.kind, "format_replace", "replacement uses the compare-and-set format-replacement kind");
assert.equal(colourReplacementSuggestion?.format_expected_value, "#123456", "replacement retains the reviewed source colour");
assert.ok(colourReplacementSuggestion?.marks.includes("color:#654321:both"), "replacement records the proposed colour");
assert.ok(state.doc.blocks[0].content.find((inline) => inline.text === " world")?.marks.some((mark) => mark === "color:#123456:none"), "Suggest-mode replacement leaves source formatting unchanged");
const suggestionsBeforeColourRemoval = state.doc.suggestions.length;
click("[data-action='mark-remove:color']");
await settle(10);
state = __test.getState();
const colourRemovalSuggestion = state.doc.suggestions.at(-1);
assert.equal(state.doc.suggestions.length, suggestionsBeforeColourRemoval + 1, "Suggest-mode clear text colour creates one proposal");
assert.equal(colourRemovalSuggestion?.kind, "format_remove", "clear text colour uses the durable format-removal kind");
assert.deepEqual(colourRemovalSuggestion?.marks, ["color:both"], "the removal proposal records a value-agnostic colour removal");
assert.ok(state.doc.blocks[0].content.find((inline) => inline.text === " world")?.marks.some((mark) => mark === "color:#123456:none"), "Suggest-mode clear text colour leaves source formatting unchanged");
editingMode.value = "edit";
editingMode.dispatchEvent(new window.Event("change", { bubbles: true }));
click("[data-action='toggle-panel:suggestions']");
await settle(3);
const openSuggestionCount = __test.getState().doc.suggestions.filter((suggestion) => suggestion.state === "proposed").length;
assert.match($("[data-side-panel]").textContent, new RegExp(`Accept all \\(${openSuggestionCount}\\)`), "bulk review names the current proposed-suggestion count");
assert.equal($("[data-side-panel] [data-action='suggestion-next']").getAttribute("aria-keyshortcuts"), "Control+Alt+ArrowRight", "next suggestion is exposed to assistive technology");
click("[data-side-panel] [data-action='suggestion-next']");
await settle(3);
assert.equal($("[data-side-panel] .suggestion.current").getAttribute("aria-current"), "true", "suggestion navigation marks the current proposal");
assert.match($("[data-suggestion-navigation-status]").textContent, new RegExp(`Suggestion 1 of ${openSuggestionCount}: .*?, `), "suggestion navigation announces its ordinal and reviewer context");
const suggestionSourceLink = $("[data-side-panel] [data-action='suggestion-go']");
lastScrolledElement = null;
click("[data-side-panel] [data-action='suggestion-go']");
await settle(3);
assert.ok(lastScrolledElement, "a suggestion's stable target is keyboard-reachable from review");
assert.equal($("[data-side-panel] .suggestion.current").getAttribute("data-suggestion-id"), suggestionSourceLink.getAttribute("data-id"), "jumping to source keeps the corresponding review proposal current");
// An open insertion is editable as an operation on its own review record;
// source text remains untouched until a reviewer accepts it.
const editableSuggestion = __test.getState().doc.suggestions.find((suggestion) => suggestion.state === "proposed" && suggestion.kind === "insert");
assert.ok(editableSuggestion, "Suggest-mode typing creates an editable insert proposal");
const sourceBeforeSuggestionEdit = __test.getState().doc.blocks[0].content.map((inline) => inline.text).join("");
click(`[data-side-panel] [data-action='edit-suggestion'][data-id='${editableSuggestion.id}']`);
assert.equal($("dialog.modal[open]").querySelector('textarea[name="text"]').value, editableSuggestion.text, "proposal editor starts with the durable insert payload");
await answerDialog({ text: "revised proposal" });
await settle(10);
state = __test.getState();
assert.equal(state.doc.suggestions.find((suggestion) => suggestion.id === editableSuggestion.id)?.text, "revised proposal", "editing an insert proposal uses the durable update operation");
assert.equal(state.doc.blocks[0].content.map((inline) => inline.text).join(""), sourceBeforeSuggestionEdit, "editing a proposal does not directly change source text");
const suggestionShortcut = new window.KeyboardEvent("keydown", { key: "ArrowRight", ctrlKey: true, altKey: true, bubbles: true, cancelable: true });
window.document.dispatchEvent(suggestionShortcut);
await settle(3);
assert.equal(suggestionShortcut.defaultPrevented, true, "suggestion keyboard navigation consumes its review shortcut");
assert.match($("[data-suggestion-navigation-status]").textContent, new RegExp(`Suggestion 2 of ${openSuggestionCount}: .*?, `), "suggestion keyboard navigation advances and announces the next proposal");
assert.equal(window.document.activeElement?.getAttribute("data-suggestion-id"), $("[data-side-panel] .suggestion.current").getAttribute("data-suggestion-id"), "keyboard navigation focuses the current proposal without changing the document selection");
click("[data-action='accept-all']");
await settle(3);
const bulkConfirm = $("dialog.modal[open]");
assert.match(bulkConfirm.textContent, new RegExp(`Accept ${openSuggestionCount} open suggestions in one review action`), "bulk resolution requires informed confirmation");
bulkConfirm.querySelector("[data-cancel]")?.dispatchEvent(new window.MouseEvent("click", { bubbles: true }));
await settle(3);
assert.equal(__test.getState().doc.suggestions.filter((suggestion) => suggestion.state === "proposed").length, openSuggestionCount, "cancelling bulk resolution leaves every proposal open");

// A remote projection rebuilds the side panel.  A card reached by keyboard
// review must retain its focus when it still exists rather than stranding
// focus on the detached pre-merge DOM node.
click("[data-side-panel] [data-action='suggestion-next']");
await settle(3);
assert.equal(window.document.activeElement?.getAttribute("data-suggestion-id"), __test.getState().activeSuggestionId, "review navigation establishes the card focus that a remote projection must preserve");
const remoteSuggestionRefresh = JSON.parse(JSON.stringify(__test.getState().doc));
remoteSuggestionRefresh.has_unsaved_changes = !remoteSuggestionRefresh.has_unsaved_changes;
__test.applyDocumentForTest(remoteSuggestionRefresh);
await settle(3);
assert.equal(window.document.activeElement?.getAttribute("data-suggestion-id"), __test.getState().activeSuggestionId, "a remote panel refresh restores focus to the surviving current suggestion card");

// A remote projection may resolve the proposal currently reached through
// keyboard navigation. The Suggestions panel must discard that local target
// instead of retaining an id for a card it no longer renders.
const remotelyResolvedSuggestion = JSON.parse(JSON.stringify(__test.getState().doc));
const selectedSuggestion = __test.getState().activeSuggestionId;
const remotelyResolvedSuggestionRecord = remotelyResolvedSuggestion.suggestions.find((suggestion) => suggestion.id === selectedSuggestion);
remotelyResolvedSuggestionRecord.state = "accepted";
// Lifecycle provenance is compact evidence, not the typed timestamped comment
// ledger. A hostile imported reviewer spelling must still be text, never HTML.
remotelyResolvedSuggestionRecord.provenance = ["accepted-by:<Reviewer & friend>"];
__test.applyDocumentForTest(remotelyResolvedSuggestion);
await settle(3);
assert.equal(__test.getState().activeSuggestionId, null, "a remote resolution clears the stale local suggestion target");
assert.equal(window.document.querySelectorAll('[data-side-panel] .suggestion.current').length, 0, "a resolved remote suggestion no longer leaves a current review card");
assert.equal($("[data-side-panel] [data-action='suggestion-filter:proposed']").getAttribute("aria-pressed"), "true", "open suggestions remain the initial review queue");
click("[data-side-panel] [data-action='suggestion-filter:resolved']");
await settle(3);
assert.equal($("[data-side-panel] [data-action='suggestion-filter:resolved']").getAttribute("aria-pressed"), "true", "the resolved-history filter exposes its selected state");
assert.match($("[data-side-panel]").textContent, /Accepted/, "resolved suggestions remain inspectable rather than disappearing from review");
assert.match($("[data-side-panel]").textContent, /stored state and any available lifecycle evidence/, "resolved history distinguishes compact evidence from timestamped review history");
const suggestionProvenance = $("[data-side-panel] .suggestion-provenance");
assert.match(suggestionProvenance.textContent, /Lifecycle evidence \(1\).*Accepted by <Reviewer & friend>/s, "resolved suggestions retain readable lifecycle evidence");
assert.equal(suggestionProvenance.querySelector("script"), null, "suggestion lifecycle evidence is escaped instead of rendered as markup");
assert.ok(suggestionProvenance.querySelector("summary"), "suggestion lifecycle evidence is a keyboard-accessible native disclosure");
assert.equal(window.document.querySelector("[data-side-panel] [data-action='accept-suggestion']"), null, "resolved-history cards do not expose stale resolution actions");
assert.equal(window.document.querySelector("[data-side-panel] [data-action='suggestion-next']"), null, "proposed-only navigation is not relabelled as history traversal");
click("[data-side-panel] [data-action='suggestion-filter:all']");
await settle(3);
assert.match($("[data-side-panel]").textContent, /Accepted/, "all-history view retains resolved suggestions");
click("[data-side-panel] [data-action='suggestion-filter:proposed']");
await settle(3);
assert.ok(window.document.querySelector("[data-side-panel] [data-action='suggestion-next']"), "returning to open suggestions restores the proposed-only navigation controls");
// A delegated card action may already be queued when a remote structural
// update resolves the proposal. It must not create a no-op review operation
// or open a preview that suggests the old proposal can still be accepted.
const operationsBeforeStaleSuggestionAction = __test.getState().doc.operation_count;
await __test.runAction("accept-suggestion", { id: selectedSuggestion });
await settle(3);
assert.equal(__test.getState().doc.operation_count, operationsBeforeStaleSuggestionAction, "a stale accept does not create a review operation");
assert.equal([...window.document.querySelectorAll(".toast")].at(-1)?.textContent, "This suggestion is no longer open.", "a stale suggestion action explains its resolved state");
await __test.runAction("preview-suggestion", { id: selectedSuggestion, resolution: "accept" });
await settle(3);
assert.equal(window.document.querySelector("dialog.suggestion-preview[open]"), null, "a stale proposal cannot open an actionable acceptance preview");

// ---- Side panels --------------------------------------------------------------
click("[data-action='toggle-panel:history']");
await settle(5);
assert.ok(!$("[data-side-panel]").hidden, "history panel opens");
assert.ok($("[data-side-panel]").textContent.includes("operations"));
click('[data-side-panel] [aria-label="Close"]');
await settle(5);
assert.ok($("[data-side-panel]").hidden, "the panel header close button hides the panel");

// Comment filters use lifecycle state without dropping the durable anchor.
__test.setSelection({
  anchor: { block_id: state.doc.blocks[0].id, inline_id: state.doc.blocks[0].content[0].id, offset: 0 },
  focus: { block_id: state.doc.blocks[0].id, inline_id: state.doc.blocks[0].content[0].id, offset: 5 },
});
click("[data-action='comment']");
await answerDialog({ body: "Needs a source" });
await settle(10);
assert.ok($("[data-side-panel]").textContent.includes("Needs a source"), "new comments appear in the open filter");
// Editing is a labelled modal routed to the durable update operation.  The
// previous body must move to immutable provenance rather than disappear.
state = __test.getState();
const authoredThread = state.doc.comments.at(-1);
const authoredComment = authoredThread.comments.at(-1);
click(`[data-action='edit-comment'][data-comment-id='${authoredComment.id}']`);
await answerDialog({ body: "Needs a stronger source" });
await settle(10);
state = __test.getState();
assert.equal(state.doc.comments.at(-1).comments.at(-1).body, "Needs a stronger source", "comment edit uses the durable update command");
// Replies stay in the review surface: composing and submitting must not open
// a modal, and the form reaches the same durable reply command as the former
// dialog route.
click("[data-action='reply-comment']");
await settle(3);
const replyForm = $("[data-comment-reply-form]");
assert.equal(window.document.querySelector("dialog.modal[open]"), null, "comment replies compose inline rather than in a modal");
const replyField = replyForm.querySelector('textarea[name="body"]');
assert.ok(replyField, "inline reply has a labelled multiline field");
replyField.value = "I will add one.";
replyForm.dispatchEvent(new window.Event("submit", { bubbles: true, cancelable: true }));
await settle(10);
state = __test.getState();
assert.equal(state.doc.comments.at(-1).comments.at(-1).body, "I will add one.", "inline reply uses the durable comment reply command");
assert.equal($("[data-side-panel]").querySelector("[data-comment-reply-form]"), null, "successful inline reply closes its composer");
const replyComment = state.doc.comments.at(-1).comments.at(-1);
click(`[data-action='delete-comment'][data-comment-id='${replyComment.id}']`);
await answerDialog();
await settle(10);
state = __test.getState();
assert.equal(state.doc.comments.at(-1).comments.at(-1).deleted, true, "comment delete uses the durable non-destructive command");
assert.equal($("[data-side-panel]").textContent.includes("I will add one."), false, "deleted comment text is not rendered as a live conversation item");
const restoreComment = $("[data-side-panel] [data-action='restore-comment']");
assert.equal(restoreComment.dataset.commentId, replyComment.id, "deleted-state controls identify the exact comment to restore");
restoreComment.dispatchEvent(new window.MouseEvent("click", { bubbles: true, cancelable: true }));
await settle(10);
state = __test.getState();
assert.equal(state.doc.comments.at(-1).comments.at(-1).deleted, false, "deleted-state controls restore a comment through the durable command");
assert.ok($("[data-side-panel]").textContent.includes("I will add one."), "restored comment returns to the live conversation");
click(`[data-action='delete-comment'][data-comment-id='${replyComment.id}']`);
await answerDialog();
await settle(10);
state = __test.getState();
assert.equal(state.doc.comments.at(-1).comments.at(-1).deleted, true, "a restored comment can return to durable deleted state");
click("[data-action='reply-comment']");
await settle(3);
const replyEscape = $("[data-comment-reply-form] textarea");
replyEscape.dispatchEvent(new window.KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
await settle(3);
assert.equal($("[data-side-panel]").querySelector("[data-comment-reply-form]"), null, "Escape closes an inline reply without writing it");
// A remote deletion clears an in-flight reply composer. If its submit event
// was already queued, it must not call the reply command against the deleted
// thread; restoring the thread later also must not resurrect that discarded
// draft.
click("[data-action='reply-comment']");
await settle(3);
assert.ok($("[data-comment-reply-form]"), "a live thread can open a reply composer before the remote update");
const remotelyDeletedThread = JSON.parse(JSON.stringify(__test.getState().doc));
remotelyDeletedThread.comments.find((thread) => thread.id === authoredThread.id).deleted = true;
__test.applyDocumentForTest(remotelyDeletedThread);
await settle(3);
assert.equal(__test.getState().activeCommentReplyThreadId, null, "a remote thread deletion clears its local reply target");
assert.equal($("[data-side-panel]").querySelector("[data-comment-reply-form]"), null, "a deleted thread no longer renders a reply composer");
const repliesBeforeStaleSubmit = __test.getState().doc.comments.find((thread) => thread.id === authoredThread.id).comments.length;
await __test.runAction("submit-comment-reply", { id: authoredThread.id, body: "Reply from a deleted thread" });
await settle(3);
assert.equal(__test.getState().doc.comments.find((thread) => thread.id === authoredThread.id).comments.length, repliesBeforeStaleSubmit, "a queued deleted-thread reply does not reach the document command");
assert.equal([...window.document.querySelectorAll(".toast")].at(-1)?.textContent, "This comment thread is no longer available.", "a stale reply names its unavailable target");
const remotelyRestoredThread = JSON.parse(JSON.stringify(__test.getState().doc));
remotelyRestoredThread.comments.find((thread) => thread.id === authoredThread.id).deleted = false;
__test.applyDocumentForTest(remotelyRestoredThread);
await settle(3);
assert.equal(__test.getState().activeCommentReplyThreadId, null, "a remote restore does not resurrect the discarded reply target");
assert.equal($("[data-side-panel]").querySelector("[data-comment-reply-form]"), null, "a remote restore does not reopen an abandoned reply composer");
// The immutable provenance projection is read-only plain text, including an
// old body that looks like markup.  The production value comes from Rust; the
// focused UI smoke installs one representative entry so this test does not
// need a separate editing command merely to exercise rendering.
state = __test.getState();
const reviewThread = state.doc.comments.at(-1);
const reviewComment = reviewThread.comments.at(-1);
(state.doc.comment_history ??= []).push({
  thread_id: reviewThread.id,
  comment_id: reviewComment.id,
  kind: "edited",
  actor: "Reviewer",
  at_ms: 42,
  previous_body: [{ id: "inline-provenance", kind: "text", text: "<old & private>", mark_values: {} }],
});
const liveReviewComment = reviewThread.comments.find((comment) => !comment.deleted);
(state.doc.comment_history ??= []).push({
  thread_id: reviewThread.id,
  comment_id: liveReviewComment.id,
  kind: "edited",
  actor: "Reviewer",
  at_ms: 43,
  previous_body: [{ id: "inline-provenance-live", kind: "text", text: "Earlier live wording", mark_values: {} }],
});
(state.doc.comment_activity ??= []).push({
  operation_actor: "Reviewer",
  operation_seq: 44,
  actor: "Reviewer",
  at_ms: 44,
  thread_id: reviewThread.id,
  comment_id: liveReviewComment.id,
  kind: "comment_edited",
});
(state.doc.comment_activity ??= []).push({
  operation_actor: "Reviewer",
  operation_seq: 45,
  actor: "Reviewer",
  at_ms: 45,
  thread_id: "missing-activity-target",
  comment_id: null,
  kind: "thread_deleted",
});
click("[data-action='comment-filter:open']");
await settle(5);
// The document-wide Activity disclosure uses the same base styling class as
// per-thread History.  Select the latter explicitly so this regression keeps
// proving that immutable edit provenance remains grouped with its thread.
const provenance = $("[data-side-panel] .comment-provenance:not(.comment-activity)");
assert.match(provenance.textContent, /History \(\d+\).*Edited comment by .*Reviewer.*logical time 42/s, "review provenance is grouped with its visible thread");
assert.equal(provenance.querySelector("script"), null, "historical text is escaped instead of rendered as markup");
assert.ok(provenance.querySelector("summary"), "history is a keyboard-accessible native disclosure");
assert.match(provenance.getAttribute("aria-label"), /History for/, "history identifies its thread to assistive technology");
const historyGo = provenance.querySelector("[data-action='comment-history-go']");
assert.ok(historyGo, "a live comment history entry can return to its current comment");
assert.equal(historyGo.dataset.commentId, liveReviewComment.id, "history navigation names the exact live comment");
historyGo.dispatchEvent(new window.MouseEvent("click", { bubbles: true, cancelable: true }));
await settle(2);
assert.equal(document.activeElement?.dataset.commentId, liveReviewComment.id, "history navigation focuses the current comment");
// Document-wide activity is a separate immutable ledger. Its return control
// must make the live target visible even if the reviewer is currently on a
// filter that excludes it, without manufacturing a target for tombstones.
click("[data-action='comment-filter:resolved']");
await settle(3);
const activity = $("[data-side-panel] .comment-activity");
const activityGo = activity.querySelector("[data-action='comment-activity-go']");
assert.ok(activityGo, "live comment activity exposes a return control for its exact durable target");
assert.equal(activityGo.dataset.threadId, reviewThread.id, "activity return names the durable source thread");
assert.equal(activityGo.dataset.commentId, liveReviewComment.id, "activity return names the durable source comment");
const unavailableActivity = [...activity.querySelectorAll("[data-comment-activity-operation]")]
  .find((entry) => entry.dataset.commentActivityOperation === "Reviewer:45");
assert.ok(unavailableActivity, "an activity record whose target is unavailable remains visible evidence");
assert.equal(unavailableActivity.querySelector("button"), null, "an unavailable activity target never pretends it can be reopened or navigated");
activityGo.dispatchEvent(new window.MouseEvent("click", { bubbles: true, cancelable: true }));
await settle(3);
assert.equal($("[data-side-panel] [data-action='comment-filter:all']").getAttribute("aria-pressed"), "true", "activity return reveals its target through the all-thread review projection");
assert.equal(document.activeElement?.dataset.commentId, liveReviewComment.id, "activity return focuses the current durable comment");
// "For you" is a personal review queue, not another spelling of Open.  It
// includes the active action item bound to the current subject and excludes a
// completed/resolved one without changing its durable comment state.
const documentBeforeStaleActionItem = JSON.parse(JSON.stringify(__test.getState().doc));
click("[data-action='comment-action']");
await settle(3);
assert.ok(window.document.querySelector("dialog.modal[open]"), "an action-item dialog opens for the live comment thread");
const documentAfterRemoteActionThreadDelete = JSON.parse(JSON.stringify(documentBeforeStaleActionItem));
const staleActionThreadId = documentAfterRemoteActionThreadDelete.comments.at(-1).id;
documentAfterRemoteActionThreadDelete.comments = documentAfterRemoteActionThreadDelete.comments.filter((thread) => thread.id !== staleActionThreadId);
__test.applyDocumentForTest(documentAfterRemoteActionThreadDelete);
await answerDialog({ assignee: state.authorName ?? "Local user", dueDate: "2030-01-02", status: "open" });
assert.equal([...window.document.querySelectorAll(".toast")].at(-1)?.textContent, "This comment was deleted while its action item was open. Reopen a live comment to manage its action.", "a remote thread delete cancels a stale action-item dialog locally");
__test.applyDocumentForTest(documentBeforeStaleActionItem);
await settle(3);
click("[data-action='comment-action']");
await answerDialog({ assignee: state.authorName ?? "Local user", dueDate: "2030-01-02", status: "open" });
await settle(10);
state = __test.getState();
assert.equal(state.doc.comments.at(-1).action_assignee, state.authorName ?? "Local user", "action assignment is durable");
assert.equal(state.doc.comments.at(-1).action_due_at_ms, Date.parse("2030-01-02T00:00:00.000Z"), "action due date is durable UTC midnight");
const dueDate = $("[data-side-panel] time[datetime='2030-01-02']");
assert.equal(dueDate.textContent, "due 2030-01-02 UTC", "due date displays its persisted UTC calendar day without locale drift");
assert.equal(dueDate.getAttribute("aria-label"), "Due 2030-01-02 UTC", "due date names its timezone to assistive technology");
// An existing action is not a one-way toggle: its manager pre-fills the
// durable values and can reassign/edit its due date without completing it.
click("[data-action='comment-action']");
await settle(3);
assert.equal($("dialog.modal[open] [name='assignee']").value, state.authorName ?? "Local user", "action manager pre-fills the assignee");
assert.equal($("dialog.modal[open] [name='dueDate']").value, "2030-01-02", "action manager preserves the UTC calendar day");
await answerDialog({ assignee: "Ada", dueDate: "2030-01-03", status: "open" });
await settle(10);
state = __test.getState();
assert.equal(state.doc.comments.at(-1).action_assignee, "Ada", "action manager reassigns existing work");
assert.equal(state.doc.comments.at(-1).action_due_at_ms, Date.parse("2030-01-03T00:00:00.000Z"), "action manager edits an existing due date");
click("[data-action='comment-filter:for-you']");
await settle(5);
assert.equal($("[data-side-panel] [data-action='comment-filter:for-you']").getAttribute("aria-pressed"), "true", "personal action-item filter exposes its selected state");
assert.match($("[data-side-panel]").textContent, /No comments match this filter/, "reassigned work leaves the prior assignee's personal queue");
click("[data-action='comment-filter:open']");
await settle(5);
// A reply draft is view state for one visible thread. Hiding that thread must
// discard the draft rather than making the composer reappear unexpectedly
// when the reviewer later returns to Open.
click("[data-action='reply-comment']");
await settle(3);
assert.ok($("[data-side-panel] [data-comment-reply-form]"), "reply opens an inline composer for the visible thread");
click("[data-action='comment-filter:resolved']");
await settle(3);
click("[data-action='comment-filter:open']");
await settle(3);
assert.equal($("[data-side-panel]").querySelector("[data-comment-reply-form]"), null, "a reply composer does not survive hiding its thread from the review projection");
click("[data-action='comment-action']");
await answerDialog({ assignee: state.authorName ?? "Local user", dueDate: "2030-01-03", status: "open" });
await settle(10);
click("[data-action='comment-filter:for-you']");
await settle(5);
assert.match($("[data-side-panel]").textContent, /Needs a stronger source/, "reassigned work enters the new assignee's personal queue");
click("[data-action='comment-action']");
await answerDialog({ status: "complete" });
await settle(10);
assert.match($("[data-side-panel]").textContent, /No comments match this filter/, "completed action items leave the personal queue");
click("[data-action='comment-filter:open']");
await settle(5);
click("[data-action='comment-go']");
assert.equal(lastScrolledElement?.getAttribute("data-inline-id"), state.doc.blocks[0].content[0].id, "comment navigation uses the projected stable inline anchor");
click("[data-action='resolve-comment']");
await settle(10);
assert.match($("[data-side-panel]").textContent, /No comments match this filter/, "resolved comments leave the default open filter");
click("[data-action='comment-filter:resolved']");
await settle(5);
assert.ok($("[data-side-panel]").textContent.includes("Needs a stronger source"), "resolved filter shows resolved threads");
click("[data-action='comment-filter:open']");
await settle(5);
assert.equal(__test.getState().activeCommentThreadId ?? null, null, "changing to a filter that hides the current thread clears its stale local review target");
assert.match($("[data-comment-navigation-status]").textContent, /^Showing (?:no|\d+) open comments?\.$/, "comment filter changes announce their visible result count");
click("[data-action='comment-filter:resolved']");
await settle(5);
click("[data-action='comment-filter:all']");
await settle(5);
assert.equal($("[data-side-panel] [data-action='comment-filter:all']").getAttribute("aria-pressed"), "true", "all filter exposes its selected state");
assert.equal($("[data-side-panel] [data-action='comment-next']").getAttribute("aria-keyshortcuts"), "Control+Alt+ArrowDown", "next comment is exposed to assistive technology");
click("[data-side-panel] [data-action='comment-next']");
await settle(5);
const currentCommentThread = $("[data-side-panel] .thread.current");
assert.ok(currentCommentThread, "comment navigation marks the current thread in the review panel");
assert.equal(window.document.activeElement, currentCommentThread, "comment navigation moves screen-reader focus to the current review card");
assert.match($("[data-comment-navigation-status]").textContent, /^Showing .* Current comment \d+ of \d+: /, "comment navigation announces the current visible thread");
window.document.dispatchEvent(new window.KeyboardEvent("keydown", { key: "ArrowUp", ctrlKey: true, altKey: true, bubbles: true, cancelable: true }));
await settle(5);
assert.ok(lastScrolledElement, "comment keyboard navigation follows the projected stable anchor");
// Orphaned anchors are evidence of deleted source, not a request to guess a
// nearby target. The compact label is deliberately truncated, but review must
// expose the full retained quote and containing context as escaped read-only
// text.
// Commands above replace the projection, so take the current object rather
// than retaining the earlier snapshot this smoke section began with.
const orphanThread = __test.getState().doc.comments.at(-1);
const savedOrphanAnchor = {
  anchor: orphanThread.anchor,
  anchor_label: orphanThread.anchor_label,
  orphaned_quote: orphanThread.orphaned_quote,
  orphaned_context: orphanThread.orphaned_context,
  orphaned_warning: orphanThread.orphaned_warning,
  reactions: orphanThread.reactions,
};
const orphanQuote = `${"Q".repeat(100)} <not-markup>`;
orphanThread.anchor = "orphaned";
orphanThread.anchor_label = 'Deleted text: "QQQQ…"';
orphanThread.orphaned_quote = orphanQuote;
orphanThread.orphaned_context = "Containing paragraph before deletion";
orphanThread.orphaned_warning = "Imported source confirms the comment target was removed.";
orphanThread.reactions = [{ emoji: "🚀", actors: ["Remote reviewer"] }];
click("[data-action='comment-filter:all']");
await settle(5);
const orphanPanel = $("[data-side-panel]");
assert.ok(orphanPanel.textContent.includes(orphanQuote), "orphan review exposes the full retained quote rather than only a truncated label");
assert.match(orphanPanel.textContent, /original target was deleted and cannot be navigated/i, "orphan review does not imply a guessed navigation target");
assert.ok(orphanPanel.textContent.includes(orphanThread.orphaned_warning), "orphan review discloses its retained provenance warning rather than replacing it with a generic message");
assert.equal(orphanPanel.querySelector("not-markup"), null, "orphan evidence is escaped rather than inserted as markup");
const importedReaction = [...orphanPanel.querySelectorAll('[data-action="comment-reaction"]')]
  .find((button) => button.dataset.emoji === "🚀");
assert.ok(importedReaction, "review renders an imported custom reaction instead of hiding it behind the preset choices");
assert.equal(importedReaction.getAttribute("aria-label"), "🚀 reaction, 1 person, not selected", "custom reaction count and local selection state are exposed accessibly");
Object.assign(orphanThread, savedOrphanAnchor);
editingMode.value = "view";
editingMode.dispatchEvent(new window.Event("change", { bubbles: true }));
await settle(5);
assert.ok($("[data-action='edit-comment']").hasAttribute("disabled"), "view mode does not offer comment editing controls");
assert.ok($("[data-action='delete-comment']").hasAttribute("disabled"), "view mode does not offer comment deletion controls");
assert.equal($("[data-side-panel]").querySelector("[data-action='restore-comment']"), null, "view mode does not offer comment restoration controls");
editingMode.value = "edit";
editingMode.dispatchEvent(new window.Event("change", { bubbles: true }));

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
assert.equal(grid.getAttribute("role"), "region", "the keyboard spreadsheet surface is a named navigation region");
assert.match(grid.getAttribute("aria-label") ?? "", /^Spreadsheet grid:/, "the active spreadsheet gives the keyboard surface a name");
assert.match(grid.getAttribute("aria-keyshortcuts") ?? "", /ArrowRight/, "the grid exposes its arrow-key navigation");
const gridStatus = $("[data-cell-summary]");
assert.equal(gridStatus.getAttribute("role"), "status", "the active spreadsheet address is announced politely");
assert.match(gridStatus.textContent ?? "", /A1/, "the initial focused cell is exposed to assistive technology");
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

// Cell notes and validation are metadata operations, not cell-value shortcuts:
// the Data menu must carry their complete durable command tuples for the
// focused cell.  These checks intentionally use the rendered menu actions,
// rather than calling the command seam directly.
click("[data-action='add-cell-note']");
await answerDialog({ author: "Sheet reviewer", body: "Check the total" });
await settle(12);
state = __test.getState();
// Enter moved the focus to A2; metadata follows that focus instead of using
// the prior edited cell as a stale browser-side target.
const notedA2 = state.doc.workbook.sheets[0].cells.find((c) => c.address === "A2");
assert.deepEqual(notedA2?.comments.map((comment) => [comment.author, comment.body, comment.deleted]), [["Sheet reviewer", "Check the total", false]], "Data ▸ Add cell note keeps the durable author/body tuple");

click("[data-action='cell-validation']");
await answerDialog({ kind: "list", values: "Approved\nNeeds review", strict: "reject" });
await settle(12);
state = __test.getState();
const validatedA2 = state.doc.workbook.sheets[0].cells.find((c) => c.address === "A2");
assert.deepEqual(validatedA2?.validation && [validatedA2.validation.kind, validatedA2.validation.values, validatedA2.validation.strict], ["list", ["Approved", "Needs review"], true], "Data ▸ Data validation maps its rule, inputs and strictness through the durable command");

click("[data-action='clear-cell-validation']");
await settle(12);
state = __test.getState();
assert.equal(state.doc.workbook.sheets[0].cells.find((c) => c.address === "A2")?.validation, null, "Data ▸ Remove data validation clears only the focused cell rule");

// Named ranges are sheet metadata, so their manager must route the range and
// name to the durable commands instead of faking them as a formula alias.
click("[data-action='named-ranges']");
await answerDialog({ operation: "add" });
await answerDialog({ name: "ReviewCells", range: "A2:B3" });
await settle(12);
state = __test.getState();
assert.deepEqual(state.doc.workbook.named_ranges.map((range) => [range.name, range.sheet_id, range.range]), [["REVIEWCELLS", state.doc.workbook.sheets[0].id, "A2:B3"]], "Data ▸ Named ranges adds the active sheet's durable named range");

// The protected-range UI deliberately has no permission language: it must
// author the existing warning-only metadata command from the live selection,
// not pretend it can prevent a later cell edit.
click("[data-action='protected-ranges']");
await answerDialog({ operation: "add" });
await answerDialog({ description: "Review before editing" });
await settle(12);
state = __test.getState();
assert.deepEqual(
  state.doc.workbook.sheets[0].protected_ranges.map((range) => [range.range, range.description, range.warning_only]),
  [["A2", "Review before editing", true]],
  "Data ▸ Advisory protected ranges creates only durable warning-only metadata from the focused selection",
);

click("[data-action='named-ranges']");
await answerDialog({ operation: "update" });
await answerDialog({ name: "REVIEWCELLS" });
await answerDialog({ range: "C3:D4" });
await settle(12);
state = __test.getState();
assert.equal(state.doc.workbook.named_ranges[0]?.range, "C3:D4", "Data ▸ Named ranges updates the selected durable range");

click("[data-action='named-ranges']");
await answerDialog({ operation: "delete" });
await answerDialog({ name: "REVIEWCELLS" });
await settle(12);
state = __test.getState();
assert.deepEqual(state.doc.workbook.named_ranges, [], "Data ▸ Named ranges deletes the selected durable range");

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
  assert.equal(__test.getState().selection?.focus.block_id, item.id, "list conversion preserves the toolbar's focused block");
  // `checked` is declared `checked?: boolean | null`: the projection omits
  // what a block does not have rather than writing its absence out, so absent
  // and null are the same answer here.
  assert.equal(item.checked ?? null, listKind === "checklist" ? false : null, "only a checklist item carries a checkbox, and a new one is open");
  if (listKind === "checklist") {
    const checkbox = body.querySelector(`[data-action="toggle-checklist-item"][data-checklist-block-id="${item.id}"]`);
    assert.ok(checkbox, "a checklist item exposes its durable checkbox control");
    assert.equal(checkbox.getAttribute("role"), "checkbox", "the wrapper has checkbox semantics");
    assert.equal(checkbox.getAttribute("tabindex"), "0", "the wrapper is keyboard reachable");
    checkbox.focus();
    checkbox.dispatchEvent(new window.KeyboardEvent("keydown", { key: " ", bubbles: true, cancelable: true }));
    await settle(15);
    state = __test.getState();
    item = state.doc.blocks[state.doc.blocks.length - 1];
    assert.equal(item.checked, true, "Space toggles the persisted checklist state");
  }
  if (listKind === "bullet") {
    // Google-style common marker choices are a direct toolbar preset, not a
    // free-text dialog; the selected value is still owned by the list run.
    const bulletPreset = $("[data-toolbar] [data-select='bullet-marker']");
    bulletPreset.dispatchEvent(new window.Event("pointerdown", { bubbles: true }));
    __test.setSelection({
      anchor: { block_id: "focus-moved-away", inline_id: "focus-moved-away", offset: 0 },
      focus: { block_id: "focus-moved-away", inline_id: "focus-moved-away", offset: 0 },
    });
    bulletPreset.value = "square";
    bulletPreset.dispatchEvent(new window.Event("change", { bubbles: true }));
    await settle(15);
    state = __test.getState();
    item = state.doc.blocks[state.doc.blocks.length - 1];
    assert.equal(state.doc.list_properties?.[item.list_id]?.bullet_markers?.[String(item.level)], "square", `bullet preset keeps the list selection from when it opened (${ $("[data-error]").textContent })`);
  }
  if (listKind === "ordered") {
    // Dialog-backed list settings name the list run that was current when the
    // dialog opened. A remote conversion must not let its eventual answer
    // write through that stale item id or a newly assigned run.
    const documentBeforeRemoteListChange = JSON.parse(JSON.stringify(__test.getState().doc));
    click("[data-toolbar] [data-action='list-start']");
    await settle(3);
    assert.ok(window.document.querySelector("dialog.modal[open]"), "numbering start opens for an ordered item");
    const documentAfterRemoteListChange = JSON.parse(JSON.stringify(documentBeforeRemoteListChange));
    documentAfterRemoteListChange.blocks.find((block) => block.id === item.id).kind = "paragraph";
    __test.applyDocumentForTest(documentAfterRemoteListChange);
    await answerDialog({ start: "7" });
    assert.match($("[data-error]").textContent, /list changed while its settings were open/i, "a remote list conversion cancels the stale numbering dialog locally");
    __test.applyDocumentForTest(documentBeforeRemoteListChange);
    await settle(3);
  }

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

// A modal's visible heading/body are not automatically its accessible
// name/description. The dialog explicitly references the unique elements so
// assistive technology announces the question before its accepting control.
const promptTitleId = prompt.getAttribute("aria-labelledby");
const promptDescriptionId = prompt.getAttribute("aria-describedby");
assert.ok(promptTitleId, "the confirm has an accessible-name target");
assert.ok(promptDescriptionId, "the confirm has an accessible-description target");
assert.equal(window.document.getElementById(promptTitleId)?.textContent, "Discard unsaved changes?", "the confirm names its question");
assert.match(window.document.getElementById(promptDescriptionId)?.textContent ?? "", /changes that are not saved/, "the confirm describes the consequence");

// A confirm has no fields. It used to be a promptDialog carrying one dummy
// text field, so every question the app asks — this guard included — showed an
// unused one-line text box under it.
assert.equal(prompt.querySelectorAll("input, textarea, select").length, 0, "a confirm dialog renders no fields");
assert.match(prompt.textContent, /changes that are not saved/, "the question itself is the dialog's body");
const primary = prompt.querySelector("button.primary");
assert.ok(primary, "the confirm keeps a primary button to accept it");
assert.equal(window.document.activeElement, primary, "with nothing to type in, focus starts on the accepting button");

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

// ---- A refused native shell call must be visible --------------------------
// `fetch_url_base64` was in `generate_handler!` but in neither build.rs's
// command manifest nor the Tauri capability file, so Insert ▸ Image by URL was
// dead in the native shell — and the action lost the rejection, so the user
// got no image and no message at all (PLAN77, 2026-09-12). Stand in for that
// shell: the one native call this action makes is refused the way the missing
// grant refused it.
state = __test.getState();
const imageUrlTarget = state.doc.blocks.find((block) => block.content.length > 0);
assert.ok(imageUrlTarget, "the native-image failure fixture has a text block for its caret");
__test.setSelection({
  anchor: { block_id: imageUrlTarget.id, inline_id: imageUrlTarget.content[0].id, offset: 0 },
  focus: { block_id: imageUrlTarget.id, inline_id: imageUrlTarget.content[0].id, offset: 0 },
});
const nativeCalls = [];
window.__TAURI__ = {
  core: {
    invoke: (command) => {
      nativeCalls.push(command);
      return Promise.reject(new Error(`${command} not allowed by the capability file`));
    },
  },
};
try {
  const failing = __test.runAction("insert-image-url");
  assert.ok(await answerDialog({ url: "https://example.invalid/logo.png" }), "insert-image-url asks for a URL");
  await failing;
  await settle(10);
  assert.deepEqual(nativeCalls, ["fetch_url_base64"], "the action stops at the refused call");
  const banner = $("[data-error]");
  assert.ok(!banner.hidden, "a refused native call raises the error banner");
  assert.match(banner.textContent, /Could not download that image/, "the banner says which step failed");
  assert.match(banner.textContent, /not allowed by the capability file/, "and repeats the shell's own reason");
} finally {
  delete window.__TAURI__;
}

// Outside Tauri the same action has a different answer to give: this runtime
// has no network of its own, which is not a failure of the download.
await __test.runAction("dismiss-error");
await settle(3);
const noNetwork = __test.runAction("insert-image-url");
assert.ok(await answerDialog({ url: "https://example.invalid/logo.png" }), "insert-image-url asks for a URL");
await noNetwork;
await settle(10);
assert.match($("[data-error]").textContent, /needs the OpenDoc desktop app/, "a runtime with no network says so instead of reporting a failure");

// ---- A refused export write must be visible too ---------------------------
// The same silence covered the export path, and it was worse there because the
// failure looked like a success: `downloadExport` called `saveFile` bare, so a
// refused `write_file_base64` rejected out of `runAction` and the user got
// neither the file, nor the "Exported as …" toast, nor an error (PLAN77,
// 2026-09-12). The save dialog answers and the *write* is the step that is
// refused, which is exactly the shape a missing capability grant has.
//
// The app still has to work while this shell is installed, so the mock routes
// `dispatch` into the same WebAssembly core the rest of this file drives and
// only stands in for the shell's own file capabilities.
const FILE_CAPABILITIES = ["pick_open_path", "pick_save_path", "read_file_base64", "write_file_base64", "write_file_text", "fetch_url_base64"];

function installShell(handler) {
  const calls = [];
  window.__TAURI__ = {
    core: {
      invoke: (command, args = {}) => {
        if (command === "dispatch") {
          try {
            return Promise.resolve(JSON.parse(wasmGlue.dispatch(args.command, JSON.stringify(args.args ?? {}))));
          } catch (error) {
            return Promise.reject(new Error(typeof error === "string" ? error : String(error)));
          }
        }
        if (FILE_CAPABILITIES.includes(command)) {
          calls.push(command);
          return handler(command, args);
        }
        // Window title and the rest of the shell: present and uninteresting.
        return Promise.resolve(null);
      },
    },
  };
  return calls;
}

function exportedToasts() {
  return Array.from(window.document.querySelectorAll(".toast")).filter((node) => /Exported as/.test(node.textContent));
}

await __test.runAction("dismiss-error");
await settle(3);
const refusedWrite = installShell((command) =>
  command === "pick_save_path"
    ? Promise.resolve("/tmp/smoke-export.docx")
    : Promise.reject(new Error(`${command} not allowed by the capability file`)),
);
try {
  await __test.runAction("export-docx");
  await settle(10);
  assert.deepEqual(
    refusedWrite,
    ["pick_save_path", "write_file_base64"],
    "the export asks where to save and then tries to write",
  );
  const banner = $("[data-error]");
  assert.ok(!banner.hidden, "a refused export write raises the error banner");
  assert.match(banner.textContent, /Could not write the Word \(\.docx\) file/, "the banner names the export that failed");
  assert.match(banner.textContent, /not allowed by the capability file/, "and repeats the shell's own reason");
  assert.equal(exportedToasts().length, 0, "an export that never reached a file must not claim it was exported");
} finally {
  delete window.__TAURI__;
}

// Closing the save dialog is the other answer `null` can mean, and it is not a
// failure: no file, no toast, no banner.
await __test.runAction("dismiss-error");
await settle(3);
const cancelledSave = installShell(() => Promise.resolve(null));
try {
  await __test.runAction("export-docx");
  await settle(10);
  assert.deepEqual(cancelledSave, ["pick_save_path"], "a closed dialog stops before the write");
  assert.ok($("[data-error]").hidden, "cancelling a save is not an error");
  assert.equal(exportedToasts().length, 0, "and it is not a success either");
} finally {
  delete window.__TAURI__;
}

console.log("desktop smoke passed");
process.exit(0);
