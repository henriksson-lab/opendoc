const root = new URL("..", import.meta.url).pathname;

class FakeElement {
  constructor(tagName, attributes = {}, ownerDocument = null) {
    this.tagName = tagName.toUpperCase();
    this.attributes = attributes;
    this.ownerDocument = ownerDocument;
    this.listeners = new Map();
    this.dataset = datasetFromAttributes(attributes);
    this.value = attributes.value ?? "";
    this.textContent = "";
    this._innerHTML = "";
  }

  get innerHTML() {
    return this._innerHTML;
  }

  set innerHTML(value) {
    this._innerHTML = String(value);
    if (this.ownerDocument) {
      this.ownerDocument.reindex(this._innerHTML);
    }
  }

  addEventListener(type, listener) {
    const listeners = this.listeners.get(type) ?? [];
    listeners.push(listener);
    this.listeners.set(type, listeners);
  }

  dispatch(type, event = {}) {
    for (const listener of this.listeners.get(type) ?? []) {
      listener({
        key: "",
        ctrlKey: false,
        metaKey: false,
        preventDefault() {},
        ...event,
      });
    }
  }

  click() {
    this.dispatch("click");
  }

  blur() {
    this.dispatch("blur");
  }
}

class FakeAnchorElement extends FakeElement {}

class FakeDocument {
  constructor() {
    this.elements = [];
    this.app = new FakeElement("div", { id: "app" }, this);
  }

  querySelector(selector) {
    if (selector === "#app") {
      return this.app;
    }
    return this.querySelectorAll(selector)[0] ?? null;
  }

  querySelectorAll(selector) {
    if (selector === "#app") {
      return [this.app];
    }
    if (selector.startsWith("#")) {
      const id = selector.slice(1);
      return this.elements.filter((element) => element.attributes.id === id);
    }
    const dataSelector = selector.match(/^\[data-([a-z0-9-]+)(?:="([^"]*)")?\]$/i);
    if (dataSelector) {
      const [, dataName, expected] = dataSelector;
      const attr = `data-${dataName}`;
      return this.elements.filter((element) => {
        if (!(attr in element.attributes)) return false;
        return expected === undefined || element.attributes[attr] === expected;
      });
    }
    return [];
  }

  reindex(html) {
    this.elements = parseElements(html, this);
  }
}

globalThis.window = {};
globalThis.document = new FakeDocument();
globalThis.HTMLElement = FakeElement;
globalThis.HTMLAnchorElement = FakeAnchorElement;

await import(`${root}/dist/assets/main.js`);
await settle();

assertIncludes(document.app.innerHTML, "OpenDoc");
assertIncludes(document.app.innerHTML, "Schema coverage");
assertIncludes(document.app.innerHTML, "Citations");
assertIncludes(document.app.innerHTML, "Spreadsheet");
assertIncludes(document.app.innerHTML, "doc-table");
assertIncludes(document.app.innerHTML, "citation-label");
assertIncludes(document.app.innerHTML, "equation-inline");
assertIncludes(document.app.innerHTML, "cell-editor");

clickAction("add-paragraph");
await settle();
assertIncludes(document.app.innerHTML, "New paragraph");

const editableInline = document.querySelector("[data-edit-inline-id]");
if (!editableInline) {
  throw new Error("missing editable inline");
}
editableInline.click();
clickAction("mark-bold");
await settle();
assertIncludes(document.app.innerHTML, "mark-bold");

const editedInline = document.querySelector("[data-edit-inline-id]");
if (!editedInline) {
  throw new Error("missing editable inline after mark");
}
editedInline.textContent = "GUI edited inline";
editedInline.blur();
await settle();
assertIncludes(document.app.innerHTML, "GUI edited inline");

clickAction("add-page-break");
await settle();
assertIncludes(document.app.innerHTML, "page-break");

clickAction("add-equation-block");
await settle();
const equationBlock = document.querySelector("[data-edit-equation-block-id]");
if (!equationBlock) {
  throw new Error("missing editable block equation");
}
equationBlock.textContent = "x=42";
equationBlock.blur();
await settle();
assertIncludes(document.app.innerHTML, "x=42");

const cell = document.querySelector('[data-edit-cell-address="B2"]');
if (!cell) {
  throw new Error("missing editable spreadsheet cell B2");
}
cell.textContent = "9";
cell.blur();
await settle();
assertIncludes(document.app.innerHTML, ">9</small>");

const citationTitle = document.querySelector('[data-citation-title="ref-doe-2020"]');
if (!citationTitle) {
  throw new Error("missing editable citation reference title");
}
citationTitle.value = "GUI Smoke Article";
citationTitle.dispatch("change");
await settle();
assertIncludes(document.app.innerHTML, "GUI Smoke Article");

clickAction("add-comment");
await settle();
assertIncludes(document.app.innerHTML, "New comment thread");
clickAction("delete-comment-thread");
await settle();
assertIncludes(document.app.innerHTML, "deleted");

clickAction("add-suggestion");
await settle();
clickAction("accept-suggestion");
await settle();
assertIncludes(document.app.innerHTML, "accepted");
clickAction("add-suggestion");
await settle();
clickAction("reject-suggestion");
await settle();
assertIncludes(document.app.innerHTML, "rejected");

setInputValue("repo-path", "./gui-smoke-repo");
clickAction("save-local");
await settle();
assertIncludes(document.app.innerHTML, "mock-sha256");

setInputValue("private-key-pem", "");
clickAction("sign-current");
await settle();
assertIncludes(document.app.innerHTML, "OpenSSH private key is required");

setInputValue("private-key-pem", "mock-private-key");
setInputValue("signer-display", "GUI Smoke Signer");
clickAction("sign-current");
await settle();
assertIncludes(document.app.innerHTML, "signed (1 signature) by GUI Smoke Signer");
assertNotIncludes(document.app.innerHTML, "OpenSSH private key is required");

clickAction("verify-current");
await settle();
const verifyResult = document.querySelector("#verify-result");
if (verifyResult?.textContent !== "signed") {
  throw new Error(`expected verify result to be signed, got ${verifyResult?.textContent ?? "missing"}`);
}

window.prompt = () => "GUI Fresh Document";
clickAction("create-document");
await settle();
assertIncludes(document.app.innerHTML, "GUI Fresh Document");
assertNotIncludes(document.app.innerHTML, "Schema coverage");

clickAction("add-paragraph");
await settle();
assertIncludes(document.app.innerHTML, "New paragraph");

console.log("desktop GUI smoke check passed");

function clickAction(action) {
  const button = document.querySelector(`[data-action="${action}"]`);
  if (!button) {
    throw new Error(`missing button action ${action}`);
  }
  button.click();
}

function setInputValue(id, value) {
  const input = document.querySelector(`#${id}`);
  if (!input) {
    throw new Error(`missing input ${id}`);
  }
  input.value = value;
}

async function settle() {
  await Promise.resolve();
  await Promise.resolve();
  await new Promise((resolve) => setTimeout(resolve, 0));
}

function assertIncludes(value, expected) {
  if (!value.includes(expected)) {
    throw new Error(`expected rendered GUI to include ${expected}`);
  }
}

function assertNotIncludes(value, expected) {
  if (value.includes(expected)) {
    throw new Error(`expected rendered GUI not to include ${expected}`);
  }
}

function parseElements(html, ownerDocument) {
  const elements = [];
  const tagPattern = /<([a-z][a-z0-9-]*)([^>]*)>/gi;
  let match;
  while ((match = tagPattern.exec(html))) {
    const tagName = match[1];
    const attrs = parseAttributes(match[2] ?? "");
    if (Object.keys(attrs).length === 0) {
      continue;
    }
    const element =
      tagName.toLowerCase() === "a"
        ? new FakeAnchorElement(tagName, attrs, ownerDocument)
        : new FakeElement(tagName, attrs, ownerDocument);
    if (tagName.toLowerCase() === "textarea") {
      const valueStart = match.index + match[0].length;
      const valueEnd = html.indexOf("</textarea>", valueStart);
      if (valueEnd >= 0) {
        element.value = decodeHtml(html.slice(valueStart, valueEnd));
      }
    }
    elements.push(element);
  }
  return elements;
}

function parseAttributes(raw) {
  const attrs = {};
  for (const match of raw.matchAll(/([a-zA-Z_:][-a-zA-Z0-9_:.]*)(?:="([^"]*)")?/g)) {
    attrs[match[1]] = decodeHtml(match[2] ?? "");
  }
  return attrs;
}

function datasetFromAttributes(attrs) {
  const dataset = {};
  for (const [name, value] of Object.entries(attrs)) {
    if (!name.startsWith("data-")) {
      continue;
    }
    const key = name
      .slice(5)
      .replace(/-([a-z])/g, (_, letter) => letter.toUpperCase());
    dataset[key] = value;
  }
  return dataset;
}

function decodeHtml(value) {
  return value
    .replace(/&quot;/g, '"')
    .replace(/&#039;/g, "'")
    .replace(/&gt;/g, ">")
    .replace(/&lt;/g, "<")
    .replace(/&amp;/g, "&");
}
