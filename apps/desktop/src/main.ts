import "./styles.css";
import { DocumentEditor, morphChildren } from "./editor";
import {
  closeWindow,
  dispatch,
  invoke,
  isTauri,
  onCloseRequested,
  openFile,
  pickOpenPath,
  runtimeConfig,
  saveFile,
  setWindowTitle,
} from "./invoke";
import type { CommandArgs, DesktopCommandName, DocumentCommandName } from "./commands";
import type {
  AppAuditView,
  AppBlock,
  AppDocument,
  AppEditorSelection,
  AppInline,
  AppSpreadsheetSelection,
  EditorInput,
  EditorResult,
  EditorSelection,
  OpenDocRuntimeProfile,
} from "./types";
import { confirmDialog, escapeHtml, promptDialog, setDialogAfterClose, toast } from "./ui";

// ---- State -----------------------------------------------------------------

type View = "home" | "editor";
type Mode = "docs" | "sheets";
type Panel = "comments" | "suggestions" | "citations" | "footnotes" | "files" | "warnings" | "history" | "signatures";

const runtime = runtimeConfig();
let doc: AppDocument | null = null;
let profile: OpenDocRuntimeProfile | null = null;
let audit: AppAuditView | null = null;
let view: View = "home";
let mode: Mode = "docs";
let panel: Panel | null = null;
let zoom = 1;
let selection: EditorSelection | null = null;
let lastError: string | null = null;
let editor: DocumentEditor | null = null;
let authorName = runtime.subject ?? "Local user";
let workbookHtml = "";
let sheetId: string | null = null;
let cellAnchor = "A1";
let cellFocus = "A1";
let spreadsheetSelection: AppSpreadsheetSelection | null = null;
let cellEditing: { address: string; draft: string } | null = null;
let findQuery = "";
let findIndex = 0;
let showFind = false;

const app = document.getElementById("app") as HTMLElement;
const editorHost = document.createElement("div");
editorHost.className = "doc-body";
setDialogAfterClose(() => editor?.focus());

// ---- Utilities -------------------------------------------------------------

function query<T extends Element = HTMLElement>(selector: string, root: ParentNode = app): T | null {
  return root.querySelector(selector) as T | null;
}

function showError(message: string): void {
  lastError = message;
  renderStatus();
}

async function run<K extends DesktopCommandName>(command: K, args: CommandArgs<K> = {} as CommandArgs<K>) {
  try {
    const result = await invoke(command, args);
    lastError = null;
    if (result && typeof result === "object" && "blocks" in (result as object)) {
      applyDocument(result as unknown as AppDocument);
    }
    return result;
  } catch (error) {
    showError(error instanceof Error ? error.message : String(error));
    throw error;
  }
}

async function edit<K extends DocumentCommandName>(command: K, args: CommandArgs<K> = {} as CommandArgs<K>) {
  try {
    await run(command, args);
  } catch {
    // reported by run()
  }
}

function applyDocument(next: AppDocument): void {
  doc = next;
  renderAll();
}

// ---- Document helpers -------------------------------------------------------

function* walkBlocks(blocks: AppBlock[]): Generator<AppBlock> {
  for (const block of blocks) {
    yield block;
    for (const row of block.rows) {
      for (const cell of row) {
        yield* walkBlocks(cell);
      }
    }
  }
}

function findBlock(id: string): AppBlock | null {
  if (!doc) return null;
  for (const block of walkBlocks(doc.blocks)) {
    if (block.id === id) return block;
  }
  return null;
}

function findInline(id: string): { block: AppBlock; inline: AppInline } | null {
  if (!doc) return null;
  for (const block of walkBlocks(doc.blocks)) {
    const inline = block.content.find((item) => item.id === id);
    if (inline) return { block, inline };
  }
  return null;
}

function focusBlock(): AppBlock | null {
  return selection ? findBlock(selection.focus.block_id) : null;
}

function focusInline(): AppInline | null {
  if (!selection?.focus.inline_id) return null;
  return findInline(selection.focus.inline_id)?.inline ?? null;
}

function wordStats(): string {
  return `${doc?.word_count ?? 0} words · ${doc?.character_count ?? 0} characters`;
}

// ---- Rendering -------------------------------------------------------------

function renderAll(): void {
  if (!doc) {
    app.innerHTML = `<main class="shell"><p class="loading">Loading…</p></main>`;
    return;
  }
  if (view === "home") {
    renderHome();
    return;
  }
  if (!query("[data-editor-shell]")) {
    app.innerHTML = `
      <main class="shell editor-shell" data-editor-shell>
        <header class="topbar">
          <div class="topbar-left">
            <button type="button" class="icon-button" data-action="go-home" title="Home">⌂</button>
            <div class="title-block">
              <input class="doc-title" data-doc-title aria-label="Document title" value="">
              <div class="status" data-status></div>
            </div>
          </div>
          <div class="topbar-right">
            <div class="mode-switch" role="tablist">
              <button type="button" role="tab" data-action="mode-docs">Document</button>
              <button type="button" role="tab" data-action="mode-sheets">Spreadsheet</button>
            </div>
            <input class="author" data-author aria-label="Your name" value="${escapeHtml(authorName)}" title="Name used for comments and suggestions">
            <button type="button" class="primary" data-action="share">Share</button>
          </div>
        </header>
        <nav class="menu-bar" role="menubar" data-menu-bar></nav>
        <div class="toolbar" role="toolbar" data-toolbar></div>
        <div class="find-bar" data-find hidden></div>
        <div class="error-banner" role="alert" data-error hidden></div>
        <section class="workspace" data-workspace>
          <div class="main-surface" data-main></div>
          <aside class="side-strip" data-side-strip></aside>
          <aside class="side-panel" data-side-panel hidden></aside>
        </section>
      </main>`;
    bindStatic();
  }
  renderMenus();
  renderToolbar();
  renderStatus();
  renderMain();
  renderSidePanel();
  void setWindowTitle(`${doc.title}${doc.has_unsaved_changes ? " •" : ""} – OpenDoc`);
}

function renderHome(): void {
  const recents = doc?.recent_documents ?? [];
  app.innerHTML = `
    <main class="shell home-shell">
      <header class="home-topbar"><h1>OpenDoc</h1><span class="runtime-label">${escapeHtml(profile?.label ?? runtime.mode)}</span></header>
      <section class="home-panel">
        <h2>Start a new document</h2>
        <div class="home-actions">
          <button type="button" class="tile" data-action="new-document"><span class="tile-preview doc"></span><span>Blank document</span></button>
          <button type="button" class="tile" data-action="new-spreadsheet"><span class="tile-preview sheet"></span><span>Blank spreadsheet</span></button>
          <button type="button" class="tile" data-action="import-word"><span class="tile-preview import"></span><span>Import Word (.docx)</span></button>
          <button type="button" class="tile" data-action="open-repository"><span class="tile-preview folder"></span><span>Open folder…</span></button>
        </div>
      </section>
      <section class="home-panel">
        <h2>Recent documents</h2>
        ${
          recents.length === 0
            ? `<p class="empty">No recent documents yet.</p>`
            : `<ul class="recent-list">${recents
                .map(
                  (recent) => `<li><button type="button" data-action="open-recent" data-uuid="${escapeHtml(recent.uuid)}" data-root="${escapeHtml(recent.repository_root ?? "")}" data-backend="${escapeHtml(recent.repository_backend ?? "")}" data-namespace="${escapeHtml(recent.repository_namespace ?? "")}">
                    <span class="recent-title">${escapeHtml(recent.title || "Untitled document")}</span>
                    <span class="recent-meta">${escapeHtml(recent.repository_root ?? "")}</span>
                  </button></li>`,
                )
                .join("")}</ul>`
        }
      </section>
    </main>`;
  app.querySelectorAll<HTMLElement>("[data-action]").forEach((node) => {
    node.addEventListener("click", () => void runAction(node.dataset.action ?? "", node.dataset));
  });
}

const MENUS: { label: string; items: { action: string; label: string; shortcut?: string; scope?: Mode }[] }[] = [
  {
    label: "File",
    items: [
      { action: "new-document", label: "New document", shortcut: "Ctrl+N" },
      { action: "new-spreadsheet", label: "New spreadsheet" },
      { action: "open-repository", label: "Open folder…", shortcut: "Ctrl+O" },
      { action: "import-word", label: "Import Word (.docx)…" },
      { action: "import-json", label: "Import Google Docs JSON…" },
      { action: "save", label: "Save", shortcut: "Ctrl+S" },
      { action: "save-as", label: "Save as…", shortcut: "Ctrl+Shift+S" },
      { action: "export-json", label: "Download as Google Docs JSON" },
      { action: "export-html", label: "Download as HTML" },
      { action: "export-text", label: "Download as plain text" },
      { action: "print", label: "Print…", shortcut: "Ctrl+P" },
      { action: "rename", label: "Rename…" },
      { action: "close-document", label: "Close document" },
    ],
  },
  {
    label: "Edit",
    items: [
      { action: "undo", label: "Undo", shortcut: "Ctrl+Z" },
      { action: "redo", label: "Redo", shortcut: "Ctrl+Y" },
      { action: "select-all", label: "Select all", shortcut: "Ctrl+A" },
      { action: "find", label: "Find and replace", shortcut: "Ctrl+F" },
      { action: "comment", label: "Comment", shortcut: "Ctrl+Alt+M", scope: "docs" },
      { action: "suggest", label: "Suggest replacement…", scope: "docs" },
      { action: "suggest-delete", label: "Suggest deletion", scope: "docs" },
    ],
  },
  {
    label: "View",
    items: [
      { action: "toggle-panel:comments", label: "Comments" },
      { action: "toggle-panel:suggestions", label: "Suggestions" },
      { action: "toggle-panel:citations", label: "Citations" },
      { action: "toggle-panel:footnotes", label: "Footnotes", scope: "docs" },
      { action: "toggle-panel:files", label: "Attachments" },
      { action: "toggle-panel:history", label: "History" },
      { action: "toggle-panel:signatures", label: "Signatures" },
      { action: "toggle-panel:warnings", label: "Warnings" },
      { action: "zoom-in", label: "Zoom in", shortcut: "Ctrl++" },
      { action: "zoom-out", label: "Zoom out", shortcut: "Ctrl+-" },
      { action: "zoom-reset", label: "Zoom 100%", shortcut: "Ctrl+0" },
    ],
  },
  {
    label: "Insert",
    items: [
      { action: "insert-image", label: "Image…", scope: "docs" },
      { action: "insert-table", label: "Table…", scope: "docs" },
      { action: "insert-link", label: "Link…", shortcut: "Ctrl+K", scope: "docs" },
      { action: "insert-footnote", label: "Footnote", scope: "docs" },
      { action: "insert-equation", label: "Equation…", scope: "docs" },
      { action: "insert-equation-block", label: "Equation block…", scope: "docs" },
      { action: "insert-page-break", label: "Page break", shortcut: "Ctrl+Enter", scope: "docs" },
      { action: "insert-citation", label: "Citation…", scope: "docs" },
      { action: "insert-mention", label: "Mention…", scope: "docs" },
      { action: "comment", label: "Comment", scope: "docs" },
      { action: "add-sheet", label: "Sheet", scope: "sheets" },
      { action: "add-row", label: "Row below", scope: "sheets" },
      { action: "add-column", label: "Column right", scope: "sheets" },
    ],
  },
  {
    label: "Format",
    items: [
      { action: "mark:bold", label: "Bold", shortcut: "Ctrl+B" },
      { action: "mark:italic", label: "Italic", shortcut: "Ctrl+I" },
      { action: "mark:underline", label: "Underline", shortcut: "Ctrl+U" },
      { action: "mark:strike", label: "Strikethrough", shortcut: "Alt+Shift+5" },
      { action: "mark:superscript", label: "Superscript", shortcut: "Ctrl+." },
      { action: "mark:subscript", label: "Subscript", shortcut: "Ctrl+," },
      { action: "mark:code", label: "Code", scope: "docs" },
      { action: "clear-marks", label: "Clear formatting", shortcut: "Ctrl+\\", scope: "docs" },
      { action: "style:paragraph", label: "Normal text", shortcut: "Ctrl+Alt+0", scope: "docs" },
      { action: "style:heading:1", label: "Heading 1", shortcut: "Ctrl+Alt+1", scope: "docs" },
      { action: "style:heading:2", label: "Heading 2", shortcut: "Ctrl+Alt+2", scope: "docs" },
      { action: "style:heading:3", label: "Heading 3", shortcut: "Ctrl+Alt+3", scope: "docs" },
      { action: "style:list:false", label: "Bulleted list", shortcut: "Ctrl+Shift+8", scope: "docs" },
      { action: "style:list:true", label: "Numbered list", shortcut: "Ctrl+Shift+7", scope: "docs" },
      { action: "indent", label: "Increase indent", shortcut: "Ctrl+]", scope: "docs" },
      { action: "outdent", label: "Decrease indent", shortcut: "Ctrl+[", scope: "docs" },
      { action: "citation-style", label: "Citation style…" },
    ],
  },
  {
    label: "Tools",
    items: [
      { action: "word-count", label: "Word count", shortcut: "Ctrl+Shift+C" },
      { action: "sign", label: "Sign document…" },
      { action: "verify", label: "Verify signatures" },
      { action: "toggle-panel:warnings", label: "Warnings" },
    ],
  },
  {
    label: "Help",
    items: [
      { action: "shortcuts", label: "Keyboard shortcuts", shortcut: "Ctrl+/" },
      { action: "about", label: "About OpenDoc" },
    ],
  },
];

function renderMenus(): void {
  const bar = query("[data-menu-bar]");
  if (!bar) return;
  const html = MENUS.map(
    (menu) => `
      <details class="menu-group" data-menu="${escapeHtml(menu.label)}">
        <summary role="menuitem" aria-haspopup="menu">${escapeHtml(menu.label)}</summary>
        <div class="menu-items" role="menu">
          ${menu.items
            .map((item) => {
              const disabled = item.scope && item.scope !== mode ? " disabled" : "";
              return `<button type="button" role="menuitem" data-action="${escapeHtml(item.action)}"${disabled}><span>${escapeHtml(item.label)}</span>${item.shortcut ? `<kbd>${escapeHtml(item.shortcut)}</kbd>` : ""}</button>`;
            })
            .join("")}
        </div>
      </details>`,
  ).join("");
  const template = document.createElement("template");
  template.innerHTML = html;
  morphChildren(bar, template.content);
}

const FONT_SIZES = ["8", "9", "10", "11", "12", "14", "18", "24", "36"];
const FONTS = ["Arial", "Georgia", "Times New Roman", "Courier New", "Verdana", "Inter"];

function renderToolbar(): void {
  const bar = query("[data-toolbar]");
  if (!bar || !doc) return;
  const inline = focusInline();
  const kinds = new Set(inline?.mark_kinds ?? []);
  const markValue = (kind: string, fallback: string) => inline?.mark_values[kind] ?? fallback;
  const block = focusBlock();
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
        ["heading:1", "Heading 1"],
        ["heading:2", "Heading 2"],
        ["heading:3", "Heading 3"],
        ["heading:4", "Heading 4"],
        ["list:false", "Bulleted list"],
        ["list:true", "Numbered list"],
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
    <span class="sep"></span>
    <button type="button" class="tb" data-action="insert-link" title="Insert link (Ctrl+K)">🔗</button>
    <button type="button" class="tb" data-action="comment" title="Add comment (Ctrl+Alt+M)">💬</button>
    <button type="button" class="tb" data-action="insert-image" title="Insert image">🖼</button>
    <span class="sep"></span>
    <button type="button" class="tb${block?.kind === "list-item" && !block.ordered ? " active" : ""}" data-action="style:list:false" title="Bulleted list">•≡</button>
    <button type="button" class="tb${block?.kind === "list-item" && block.ordered ? " active" : ""}" data-action="style:list:true" title="Numbered list">1≡</button>
    <button type="button" class="tb" data-action="outdent" title="Decrease indent (Ctrl+[)">⇤</button>
    <button type="button" class="tb" data-action="indent" title="Increase indent (Ctrl+])">⇥</button>
    <button type="button" class="tb" data-action="clear-marks" title="Clear formatting">Tx</button>
    <span class="grow"></span>
    <button type="button" class="tb" data-action="zoom-out" title="Zoom out">−</button>
    <span class="zoom-label">${Math.round(zoom * 100)}%</span>
    <button type="button" class="tb" data-action="zoom-in" title="Zoom in">+</button>`;
  const sheet = currentSheet();
  const cell = sheet?.cells.find((item) => item.address === cellFocus);
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
    <span class="cell-summary" data-cell-summary></span>`;
  const template = document.createElement("template");
  template.innerHTML = mode === "docs" ? docsToolbar : sheetsToolbar;
  morphChildren(bar, template.content);
  bar.querySelectorAll<HTMLSelectElement>("select[data-select]").forEach((select) => {
    select.onchange = () => void onSelectChange(select.dataset.select ?? "", select.value);
  });
  bar.querySelectorAll<HTMLInputElement>("input[data-color]").forEach((input) => {
    input.onchange = () => void applyMark(input.dataset.color ?? "color", input.value, "set");
  });
  bar.querySelectorAll<HTMLInputElement>("input[data-cell-color]").forEach((input) => {
    input.onchange = () => void setCellFormat(input.dataset.cellColor ?? "text_color", input.value);
  });
}

function renderStatus(): void {
  const status = query("[data-status]");
  const title = query<HTMLInputElement>("[data-doc-title]");
  const error = query("[data-error]");
  if (!doc) return;
  if (title && document.activeElement !== title && title.value !== doc.title) {
    title.value = doc.title;
  }
  if (status) {
    const save = doc.repository_root
      ? doc.has_unsaved_changes
        ? "Unsaved changes"
        : "Saved"
      : doc.has_unsaved_changes
        ? "Not saved yet"
        : "New document";
    status.innerHTML = `<span class="save-state ${doc.has_unsaved_changes ? "dirty" : "clean"}">${escapeHtml(save)}</span> · <span>${escapeHtml(wordStats())}</span>${doc.signature_state !== "unsigned" ? ` · <span class="sig ${escapeHtml(doc.signature_state)}">${escapeHtml(doc.signature_state)}</span>` : ""}`;
  }
  if (error) {
    error.hidden = !lastError;
    error.innerHTML = lastError ? `<span>${escapeHtml(lastError)}</span><button type="button" data-action="dismiss-error" aria-label="Dismiss">✕</button>` : "";
  }
  app.querySelectorAll<HTMLElement>("[data-action='mode-docs'],[data-action='mode-sheets']").forEach((tab) => {
    const active = (tab.dataset.action === "mode-docs") === (mode === "docs");
    tab.setAttribute("aria-selected", String(active));
    tab.classList.toggle("active", active);
  });
  renderFind();
}

function renderFind(): void {
  const bar = query("[data-find]");
  if (!bar) return;
  bar.hidden = !showFind;
  if (!showFind) return;
  const matches = findMatches();
  const existingInput = query<HTMLInputElement>("[data-find-input]", bar);
  if (existingInput) {
    const count = query(".find-count", bar);
    if (count) count.textContent = matches.length === 0 ? "No matches" : `${Math.min(findIndex + 1, matches.length)} of ${matches.length}`;
    return;
  }
  bar.innerHTML = `
    <input type="search" data-find-input value="${escapeHtml(findQuery)}" placeholder="Find in document" aria-label="Find">
    <span class="find-count">${matches.length === 0 ? "No matches" : `${Math.min(findIndex + 1, matches.length)} of ${matches.length}`}</span>
    <button type="button" data-action="find-prev" title="Previous match">↑</button>
    <button type="button" data-action="find-next" title="Next match">↓</button>
    <input type="text" data-replace-input placeholder="Replace with" aria-label="Replace with">
    <button type="button" data-action="replace-one">Replace</button>
    <button type="button" data-action="replace-all">Replace all</button>
    <button type="button" data-action="find-close" aria-label="Close">✕</button>`;
  const input = query<HTMLInputElement>("[data-find-input]", bar);
  input?.addEventListener("input", () => {
    findQuery = input.value;
    findIndex = 0;
    highlightMatch();
    renderFind();
  });
  input?.addEventListener("keydown", (event) => {
    if (event.key === "Enter") {
      event.preventDefault();
      void runAction(event.shiftKey ? "find-prev" : "find-next");
    }
    if (event.key === "Escape") {
      void runAction("find-close");
    }
  });
}

type Match = { block_id: string; inline_id: string; offset: number; length: number };

function findMatches(): Match[] {
  if (!doc || !findQuery) return [];
  const needle = findQuery.toLowerCase();
  const matches: Match[] = [];
  for (const block of walkBlocks(doc.blocks)) {
    for (const inline of block.content) {
      if (inline.kind !== "text" && inline.kind !== "link") continue;
      const haystack = inline.text.toLowerCase();
      let index = haystack.indexOf(needle);
      while (index >= 0) {
        matches.push({ block_id: block.id, inline_id: inline.id, offset: Array.from(inline.text.slice(0, index)).length, length: Array.from(findQuery).length });
        index = haystack.indexOf(needle, index + needle.length);
      }
    }
  }
  return matches;
}

function highlightMatch(): void {
  const matches = findMatches();
  if (matches.length === 0 || !editor) return;
  const match = matches[((findIndex % matches.length) + matches.length) % matches.length];
  editor.setSelection({
    anchor: { block_id: match.block_id, inline_id: match.inline_id, offset: match.offset },
    focus: { block_id: match.block_id, inline_id: match.inline_id, offset: match.offset + match.length },
  });
}

function renderMain(): void {
  const main = query("[data-main]");
  if (!main || !doc) return;
  if (!doc.is_open) {
    main.innerHTML = `<section class="closed-panel"><h2>No document open</h2><button type="button" data-action="go-home">Back to home</button></section>`;
    return;
  }
  if (mode === "docs") {
    let page = query("[data-page]", main);
    if (!page) {
      main.innerHTML = `<div class="page-stack" data-page-stack><article class="page" data-page></article><section class="footnote-area" data-footnotes></section></div>`;
      page = query("[data-page]", main) as HTMLElement;
      page.appendChild(editorHost);
      editor?.destroy();
      editor = new DocumentEditor(editorHost, editorHooks);
    }
    editor?.setHtml(doc.body_html);
    const notes = query("[data-footnotes]", main);
    if (notes) {
      notes.innerHTML = doc.footnotes_html;
      notes.hidden = !doc.footnotes_html;
    }
    const stack = query("[data-page-stack]", main);
    if (stack) stack.style.setProperty("--zoom", String(zoom));
  } else {
    void renderSheets(main);
  }
}

// ---- Editor hooks ------------------------------------------------------------

const editorHooks = {
  apply: async (input: EditorInput): Promise<EditorResult> => {
    return invoke("apply_editor_input", {
      selection: input.selection,
      input_type: input.input_type,
      data: input.data,
      html: input.html ?? null,
    });
  },
  onResult: (result: EditorResult) => {
    doc = result.document;
    lastError = null;
    editor?.setHtml(doc.body_html);
    editor?.setSelection(result.selection);
    selection = result.selection;
    renderStatus();
    renderToolbar();
    const notes = query("[data-footnotes]");
    if (notes) {
      notes.innerHTML = doc.footnotes_html;
      notes.hidden = !doc.footnotes_html;
    }
    if (panel) renderSidePanel();
  },
  onSelectionChange: (next: EditorSelection | null) => {
    if (next) {
      selection = next;
      renderToolbar();
    }
  },
  onError: (message: string) => showError(message),
  onKeydown: async (event: KeyboardEvent, current: EditorSelection | null): Promise<boolean> => {
    if (current) selection = current;
    const ctrl = event.ctrlKey || event.metaKey;
    const key = event.key.toLowerCase();
    if (event.key === "Tab") {
      const block = focusBlock();
      if (block?.kind === "list-item") {
        await runAction(event.shiftKey ? "outdent" : "indent");
      } else if (!event.shiftKey && selection) {
        await editorHooks
          .apply({ selection, input_type: "insertText", data: "\t" })
          .then(editorHooks.onResult)
          .catch((error) => showError(String(error)));
      }
      return true;
    }
    if (ctrl && event.key === "Enter") {
      await runAction("insert-page-break");
      return true;
    }
    if (!ctrl && event.altKey && event.shiftKey && event.key === "5") {
      await runAction("mark:strike");
      return true;
    }
    if (!ctrl) return false;
    if (event.altKey) {
      if (key === "m") {
        await runAction("comment");
        return true;
      }
      if (/^[0-6]$/.test(event.key)) {
        await runAction(event.key === "0" ? "style:paragraph" : `style:heading:${event.key}`);
        return true;
      }
      return false;
    }
    if (event.shiftKey && key === "c") {
      await runAction("word-count");
      return true;
    }
    if (event.shiftKey && (event.key === "7" || event.key === "&")) {
      await runAction("style:list:true");
      return true;
    }
    if (event.shiftKey && (event.key === "8" || event.key === "*")) {
      await runAction("style:list:false");
      return true;
    }
    if (event.shiftKey && key === "s") {
      await runAction("save-as");
      return true;
    }
    if (event.shiftKey && key === "z") {
      await runAction("redo");
      return true;
    }
    if (event.shiftKey) return false;
    const shortcuts: Record<string, string> = {
      b: "mark:bold",
      i: "mark:italic",
      u: "mark:underline",
      k: "insert-link",
      z: "undo",
      y: "redo",
      s: "save",
      f: "find",
      h: "find",
      p: "print",
      ".": "mark:superscript",
      ",": "mark:subscript",
      "\\": "clear-marks",
      "]": "indent",
      "[": "outdent",
      "/": "shortcuts",
      "=": "zoom-in",
      "+": "zoom-in",
      "-": "zoom-out",
      "0": "zoom-reset",
    };
    const action = shortcuts[key] ?? shortcuts[event.key];
    if (action) {
      await runAction(action);
      return true;
    }
    return false;
  },
};

// ---- Side panel --------------------------------------------------------------

const PANELS: { id: Panel; label: string; icon: string }[] = [
  { id: "comments", label: "Comments", icon: "💬" },
  { id: "suggestions", label: "Suggestions", icon: "✎" },
  { id: "citations", label: "Citations", icon: "❝" },
  { id: "footnotes", label: "Footnotes", icon: "¹" },
  { id: "files", label: "Attachments", icon: "📎" },
  { id: "history", label: "History", icon: "🕒" },
  { id: "signatures", label: "Signatures", icon: "🔏" },
  { id: "warnings", label: "Warnings", icon: "⚠" },
];

function renderSidePanel(): void {
  const strip = query("[data-side-strip]");
  const side = query("[data-side-panel]");
  if (!strip || !side || !doc) return;
  const counts: Record<Panel, number> = {
    comments: doc.comments.filter((thread) => !thread.deleted).length,
    suggestions: doc.suggestions.filter((item) => item.state === "proposed").length,
    citations: doc.citations.references.length,
    footnotes: doc.footnotes.filter((note) => !note.deleted).length,
    files: doc.blobs.length,
    history: doc.operation_count,
    signatures: doc.signatures.length,
    warnings: doc.warnings.length,
  };
  strip.innerHTML = PANELS.map(
    (item) => `<button type="button" class="strip-button${panel === item.id ? " active" : ""}" data-action="toggle-panel:${item.id}" title="${escapeHtml(item.label)}" aria-label="${escapeHtml(item.label)}">${item.icon}${counts[item.id] ? `<span class="badge">${counts[item.id]}</span>` : ""}</button>`,
  ).join("");
  side.hidden = !panel;
  if (!panel) return;
  const body = renderPanelBody(panel);
  side.innerHTML = `<header class="panel-header"><h2>${escapeHtml(PANELS.find((item) => item.id === panel)?.label ?? "")}</h2><button type="button" data-action="toggle-panel:${panel}" aria-label="Close">✕</button></header><div class="panel-body">${body}</div>`;
}

function renderPanelBody(which: Panel): string {
  if (!doc) return "";
  switch (which) {
    case "comments": {
      const threads = doc.comments.filter((thread) => !thread.deleted);
      return `
        <button type="button" class="panel-action" data-action="comment">Add comment on selection</button>
        ${threads.length === 0 ? `<p class="empty">No comments yet.</p>` : ""}
        ${threads
          .map(
            (thread) => `
          <article class="thread" data-thread="${escapeHtml(thread.id)}">
            <p class="anchor">${escapeHtml(thread.anchor_label)}</p>
            ${thread.comments
              .filter((comment) => !comment.deleted)
              .map((comment) => `<div class="comment"><strong>${escapeHtml(comment.author)}</strong><p>${escapeHtml(comment.body)}</p></div>`)
              .join("")}
            <div class="thread-actions">
              <button type="button" data-action="reply-comment" data-id="${escapeHtml(thread.id)}">Reply</button>
              <button type="button" data-action="resolve-comment" data-id="${escapeHtml(thread.id)}">Resolve</button>
            </div>
          </article>`,
          )
          .join("")}`;
    }
    case "suggestions": {
      const items = doc.suggestions.filter((item) => item.state === "proposed");
      return `
        <div class="panel-actions"><button type="button" class="panel-action" data-action="suggest">Suggest replacement…</button><button type="button" class="panel-action" data-action="suggest-delete">Suggest deletion</button></div>
        ${items.length > 1 ? `<div class="panel-actions"><button type="button" data-action="accept-all">Accept all</button><button type="button" data-action="reject-all">Reject all</button></div>` : ""}
        ${items.length === 0 ? `<p class="empty">No open suggestions.</p>` : ""}
        ${items
          .map(
            (item) => `
          <article class="suggestion">
            <p><strong>${escapeHtml(item.author)}</strong> · ${escapeHtml(item.kind)}</p>
            ${item.anchor_label ? `<p class="anchor">${escapeHtml(item.anchor_label)}</p>` : ""}
            <p>${escapeHtml(item.text)}</p>
            <div class="thread-actions">
              <button type="button" data-action="accept-suggestion" data-id="${escapeHtml(item.id)}">Accept</button>
              <button type="button" data-action="reject-suggestion" data-id="${escapeHtml(item.id)}">Reject</button>
            </div>
          </article>`,
          )
          .join("")}`;
    }
    case "citations":
      return `
        <div class="panel-actions"><button type="button" class="panel-action" data-action="add-reference">Add reference…</button><button type="button" class="panel-action" data-action="import-bibtex">Import BibTeX/RIS…</button></div>
        <p class="meta">Style: ${escapeHtml(doc.citations.style)} · ${escapeHtml(doc.citations.locale)} <button type="button" data-action="citation-style">Change</button></p>
        ${doc.citations.references.length === 0 ? `<p class="empty">No references yet.</p>` : ""}
        ${doc.citations.references
          .map(
            (reference) => `
          <article class="reference">
            <p><strong>${escapeHtml(reference.title)}</strong></p>
            <p class="meta">${escapeHtml(reference.authors.join(", "))}${reference.issued ? ` (${escapeHtml(reference.issued)})` : ""}</p>
            <div class="thread-actions">
              <button type="button" data-action="cite" data-id="${escapeHtml(reference.id)}">Cite</button>
              <button type="button" data-action="cite-footnote" data-id="${escapeHtml(reference.id)}">Cite in footnote</button>
              <button type="button" data-action="delete-reference" data-id="${escapeHtml(reference.id)}">Delete</button>
            </div>
          </article>`,
          )
          .join("")}
        ${doc.citations.bibliography.length > 0 ? `<h3>Bibliography</h3><ol class="bibliography">${doc.citations.bibliography.map((entry) => `<li>${escapeHtml(entry.text)}</li>`).join("")}</ol>` : ""}`;
    case "footnotes": {
      const notes = doc.footnotes.filter((note) => !note.deleted);
      return `
        <button type="button" class="panel-action" data-action="insert-footnote">Insert footnote at caret</button>
        ${notes.length === 0 ? `<p class="empty">No footnotes.</p>` : ""}
        ${notes
          .map(
            (note, index) => `<article class="footnote"><p><sup>${index + 1}</sup> ${escapeHtml(note.body.map((inline) => inline.text).join(""))}</p><button type="button" data-action="edit-footnote" data-id="${escapeHtml(note.id)}">Edit</button></article>`,
          )
          .join("")}`;
    }
    case "files":
      return `
        <button type="button" class="panel-action" data-action="attach-file">Attach file…</button>
        ${doc.blobs.length === 0 ? `<p class="empty">No attachments.</p>` : ""}
        ${doc.blobs
          .map(
            (blob) => `
          <article class="blob">
            <p><strong>${escapeHtml(blob.name)}</strong> <span class="meta">${escapeHtml(blob.media_type)} · ${blob.size} bytes${blob.available ? "" : " · missing"}</span></p>
            <div class="thread-actions">
              ${blob.media_type.startsWith("image/") ? `<button type="button" data-action="insert-blob-image" data-hash="${escapeHtml(blob.hash)}" data-name="${escapeHtml(blob.name)}">Insert image</button>` : ""}
              <button type="button" data-action="delete-blob" data-hash="${escapeHtml(blob.hash)}">Delete</button>
            </div>
          </article>`,
          )
          .join("")}`;
    case "history":
      return `
        <div class="panel-actions"><button type="button" data-action="undo">Undo</button><button type="button" data-action="redo">Redo</button></div>
        <p class="meta">${doc.operation_count} operations · ${doc.last_manifest ? `version ${escapeHtml(doc.last_manifest.slice(0, 19))}…` : "not saved"}</p>
        <ol class="operations">${doc.operations
          .slice()
          .reverse()
          .slice(0, 100)
          .map((op) => `<li><span class="meta">${escapeHtml(new Date(op.created_at_ms).toLocaleTimeString())}</span> ${escapeHtml(op.summary)}</li>`)
          .join("")}</ol>`;
    case "signatures":
      return `
        <div class="panel-actions"><button type="button" class="panel-action" data-action="sign">Sign document…</button><button type="button" class="panel-action" data-action="verify">Verify</button></div>
        <p class="meta">State: ${escapeHtml(doc.signature_state)}</p>
        ${doc.signatures.length === 0 ? `<p class="empty">No signatures.</p>` : ""}
        ${doc.signatures.map((signature) => `<article class="signature"><p><strong>${escapeHtml(signature.signer_display)}</strong></p><p class="meta">${escapeHtml(signature.signer)} · ${escapeHtml(new Date(signature.signed_at_ms).toLocaleString())}</p></article>`).join("")}`;
    case "warnings":
      return doc.warnings.length === 0
        ? `<p class="empty">No warnings.</p>`
        : `<ul class="warnings">${doc.warnings.map((warning) => `<li><code>${escapeHtml(warning.code)}</code> ${escapeHtml(warning.message)}</li>`).join("")}</ul>`;
  }
}

// ---- Spreadsheet ---------------------------------------------------------------

function currentSheet() {
  const sheets = doc?.workbook.sheets ?? [];
  return sheets.find((sheet) => sheet.id === sheetId) ?? sheets[0] ?? null;
}

async function renderSheets(main: HTMLElement): Promise<void> {
  if (!doc) return;
  const sheet = currentSheet();
  if (!sheet) {
    main.innerHTML = `<p class="empty">No sheets.</p>`;
    return;
  }
  sheetId = sheet.id;
  try {
    const result = await dispatch("render_workbook_html", { sheetId: sheet.id });
    workbookHtml = result.kind === "Text" ? result.value : "";
  } catch (error) {
    showError(error instanceof Error ? error.message : String(error));
    return;
  }
  let grid = query("[data-workbook]", main);
  if (!grid) {
    main.innerHTML = `
      <div class="workbook" data-workbook>
        <div class="formula-bar"><input class="name-box" data-name-box aria-label="Name box" value="${escapeHtml(cellFocus)}"><span class="fx">fx</span><input class="formula-input" data-formula-input aria-label="Formula"></div>
        <div class="grid-scroll" data-grid tabindex="0"></div>
        <div class="sheet-tabs" data-sheet-tabs></div>
      </div>`;
    grid = query("[data-workbook]", main) as HTMLElement;
    bindSheetEvents(grid);
  }
  const gridHost = query("[data-grid]", grid) as HTMLElement;
  const template = document.createElement("template");
  template.innerHTML = workbookHtml;
  morphChildren(gridHost, template.content);
  const tabs = query("[data-sheet-tabs]", grid) as HTMLElement;
  tabs.innerHTML = `${doc.workbook.sheets
    .map((item) => `<button type="button" class="sheet-tab${item.id === sheet.id ? " active" : ""}" data-action="select-sheet" data-id="${escapeHtml(item.id)}" data-title="${escapeHtml(item.title)}">${escapeHtml(item.title)}</button>`)
    .join("")}<button type="button" class="sheet-tab add" data-action="add-sheet" title="Add sheet">＋</button>`;
  await refreshSpreadsheetSelection();
  updateCellSelection();
  const formula = query<HTMLInputElement>("[data-formula-input]", grid);
  const cell = sheet.cells.find((item) => item.address === cellFocus);
  if (formula && document.activeElement !== formula) formula.value = cellEditing ? cellEditing.draft : (cell?.user_value ?? "");
  const nameBox = query<HTMLInputElement>("[data-name-box]", grid);
  if (nameBox && document.activeElement !== nameBox) nameBox.value = rangeLabel();
  renderCellEditor(gridHost);
}

function rangeLabel(): string {
  if (spreadsheetSelection?.anchor === cellAnchor && spreadsheetSelection.focus === cellFocus) return spreadsheetSelection.range;
  return cellFocus;
}

async function refreshSpreadsheetSelection(): Promise<void> {
  if (!sheetId) {
    spreadsheetSelection = null;
    return;
  }
  try {
    spreadsheetSelection = await invoke("describe_spreadsheet_selection", { sheetId, anchor: cellAnchor, focus: cellFocus });
    cellAnchor = spreadsheetSelection.anchor;
    cellFocus = spreadsheetSelection.focus;
  } catch (error) {
    spreadsheetSelection = null;
    showError(error instanceof Error ? error.message : String(error));
  }
}

function updateCellSelection(): void {
  const grid = query("[data-grid]");
  if (!grid) return;
  const selected = new Set(spreadsheetSelection?.anchor === cellAnchor && spreadsheetSelection.focus === cellFocus ? spreadsheetSelection.selected_addresses : [cellFocus]);
  grid.querySelectorAll<HTMLElement>("[data-address]").forEach((node) => {
    node.classList.toggle("selected", selected.has(node.dataset.address ?? ""));
    node.classList.toggle("focus", node.dataset.address === cellFocus);
  });
  const summary = query("[data-cell-summary]");
  if (summary) summary.textContent = spreadsheetSelection?.summary_label ?? rangeLabel();
  const focusCell = grid.querySelector<HTMLElement>(`[data-address="${cellFocus}"]`);
  if (focusCell && typeof focusCell.scrollIntoView === "function") focusCell.scrollIntoView({ block: "nearest", inline: "nearest" });
}

function renderCellEditor(gridHost: HTMLElement): void {
  const existing = query<HTMLInputElement>("[data-cell-editor]", gridHost);
  if (!cellEditing) {
    existing?.remove();
    return;
  }
  const cell = gridHost.querySelector<HTMLElement>(`[data-address="${cellEditing.address}"]`);
  if (!cell) return;
  if (existing) return;
  const input = document.createElement("input");
  input.className = "cell-editor";
  input.setAttribute("data-cell-editor", "true");
  input.setAttribute("aria-label", `Edit cell ${cellEditing.address}`);
  input.addEventListener("keydown", (event) => void onCellEditorKey(event));
  input.addEventListener("input", () => {
    if (cellEditing) cellEditing.draft = input.value;
    const formula = query<HTMLInputElement>("[data-formula-input]");
    if (formula) formula.value = cellEditing?.draft ?? "";
  });
  cell.appendChild(input);
  input.value = cellEditing.draft;
  input.focus();
  input.setSelectionRange(input.value.length, input.value.length);
}

async function commitCellEdit(move: { col: number; row: number } | null): Promise<void> {
  const editing = cellEditing;
  cellEditing = null;
  if (editing && sheetId) {
    await edit("set_spreadsheet_cell_in_sheet", { sheetId, address: editing.address, value: editing.draft });
  }
  if (move) {
    const direction = move.col < 0 ? "left" : move.col > 0 ? "right" : move.row < 0 ? "up" : "down";
    await moveFocus(direction, false);
  }
  query<HTMLElement>("[data-grid]")?.focus();
  await renderSheets(query("[data-main]") as HTMLElement);
}

async function onCellEditorKey(event: KeyboardEvent): Promise<void> {
  if (event.key === "Enter") {
    event.preventDefault();
    await commitCellEdit(event.shiftKey ? { col: 0, row: -1 } : { col: 0, row: 1 });
  } else if (event.key === "Tab") {
    event.preventDefault();
    await commitCellEdit(event.shiftKey ? { col: -1, row: 0 } : { col: 1, row: 0 });
  } else if (event.key === "Escape") {
    event.preventDefault();
    cellEditing = null;
    await renderSheets(query("[data-main]") as HTMLElement);
    query<HTMLElement>("[data-grid]")?.focus();
  }
}

async function applySpreadsheetSelectionAction(action: string, value: string, extend: boolean): Promise<void> {
  if (!sheetId) return;
  spreadsheetSelection = await invoke("reduce_spreadsheet_selection", { sheetId, anchor: cellAnchor, focus: cellFocus, action, value, extend });
  cellAnchor = spreadsheetSelection.anchor;
  cellFocus = spreadsheetSelection.focus;
  updateCellSelection();
  renderToolbar();
  const formula = query<HTMLInputElement>("[data-formula-input]");
  const cell = currentSheet()?.cells.find((item) => item.address === cellFocus);
  if (formula && document.activeElement !== formula) formula.value = cell?.user_value ?? "";
  const nameBox = query<HTMLInputElement>("[data-name-box]");
  if (nameBox) nameBox.value = rangeLabel();
}

async function moveFocus(direction: string, extend: boolean, edge = false): Promise<void> {
  await applySpreadsheetSelectionAction(edge ? "move-edge" : "move", direction, extend);
}

function bindSheetEvents(root: HTMLElement): void {
  const grid = query("[data-grid]", root) as HTMLElement;
  grid.addEventListener("mousedown", (event) => {
    const target = (event.target as Element).closest<HTMLElement>("[data-address]");
    if (!target?.dataset.address) return;
    if (cellEditing && target.dataset.address !== cellEditing.address) {
      void commitCellEdit(null);
    }
    void applySpreadsheetSelectionAction("set-focus", target.dataset.address, event.shiftKey);
    const dragging = (move: MouseEvent) => {
      const over = (move.target as Element | null)?.closest<HTMLElement>("[data-address]");
      if (over?.dataset.address && over.dataset.address !== cellFocus) {
        void applySpreadsheetSelectionAction("set-focus", over.dataset.address, true);
      }
    };
    const stop = () => {
      grid.removeEventListener("mousemove", dragging);
      window.removeEventListener("mouseup", stop);
    };
    grid.addEventListener("mousemove", dragging);
    window.addEventListener("mouseup", stop);
  });
  grid.addEventListener("dblclick", (event) => {
    const target = (event.target as Element).closest<HTMLElement>("[data-address]");
    if (!target?.dataset.address) return;
    startCellEdit(target.dataset.address, null);
  });
  grid.addEventListener("keydown", (event) => void onGridKey(event));
  grid.addEventListener("paste", (event) => {
    event.preventDefault();
    const text = event.clipboardData?.getData("text/plain") ?? "";
    void pasteCells(text);
  });
  grid.addEventListener("copy", (event) => {
    event.preventDefault();
    const tsv = selectedCellsTsv();
    if (tsv !== null) event.clipboardData?.setData("text/plain", tsv);
    else void copySelectedCellsTsv().then((text) => navigator.clipboard?.writeText(text).catch((error) => showError(String(error))));
  });
  grid.addEventListener("cut", (event) => {
    event.preventDefault();
    const tsv = selectedCellsTsv();
    if (tsv !== null) event.clipboardData?.setData("text/plain", tsv);
    else void copySelectedCellsTsv().then((text) => navigator.clipboard?.writeText(text).catch((error) => showError(String(error))));
    void clearSelectedCells();
  });
  const formula = query<HTMLInputElement>("[data-formula-input]", root);
  formula?.addEventListener("keydown", (event) => {
    if (event.key === "Enter") {
      event.preventDefault();
      cellEditing = { address: cellFocus, draft: formula.value };
      void commitCellEdit({ col: 0, row: 1 });
    }
    if (event.key === "Escape") {
      formula.blur();
      grid.focus();
    }
  });
  formula?.addEventListener("input", () => {
    if (!cellEditing) cellEditing = { address: cellFocus, draft: formula.value };
    else cellEditing.draft = formula.value;
  });
  const nameBox = query<HTMLInputElement>("[data-name-box]", root);
  nameBox?.addEventListener("keydown", (event) => {
    if (event.key !== "Enter") return;
    event.preventDefault();
    void applySpreadsheetSelectionAction("set-range", nameBox.value, false).then(() => grid.focus());
  });
}

function startCellEdit(address: string, seed: string | null): void {
  const cell = currentSheet()?.cells.find((item) => item.address === address);
  cellEditing = { address, draft: seed ?? cell?.user_value ?? "" };
  cellFocus = address;
  cellAnchor = address;
  spreadsheetSelection = null;
  updateCellSelection();
  renderCellEditor(query("[data-grid]") as HTMLElement);
}

async function onGridKey(event: KeyboardEvent): Promise<void> {
  if (cellEditing) return;
  const ctrl = event.ctrlKey || event.metaKey;
  const moveDirection = { ArrowUp: "up", ArrowDown: "down", ArrowLeft: "left", ArrowRight: "right" }[event.key];
  if (moveDirection) {
    event.preventDefault();
    await moveFocus(moveDirection, event.shiftKey, ctrl);
    return;
  }
  if (event.key === "Enter" || event.key === "F2") {
    event.preventDefault();
    startCellEdit(cellFocus, null);
    return;
  }
  if (event.key === "Tab") {
    event.preventDefault();
    await moveFocus(event.shiftKey ? "left" : "right", false);
    return;
  }
  if (event.key === "Delete" || event.key === "Backspace") {
    event.preventDefault();
    await clearSelectedCells();
    return;
  }
  if (event.key === "Home") {
    event.preventDefault();
    await applySpreadsheetSelectionAction("home", ctrl ? "sheet" : "row", event.shiftKey);
    return;
  }
  if (ctrl && event.key.toLowerCase() === "a") {
    event.preventDefault();
    await applySpreadsheetSelectionAction("select-all", "", false);
    return;
  }
  const ctrlActions: Record<string, string> = { b: "cell-format:bold", i: "cell-format:italic", z: event.shiftKey ? "redo" : "undo", y: "redo", f: "find", s: "save" };
  if (ctrl && ctrlActions[event.key.toLowerCase()]) {
    event.preventDefault();
    await runAction(ctrlActions[event.key.toLowerCase()]);
    return;
  }
  if (!ctrl && !event.altKey && event.key.length === 1) {
    event.preventDefault();
    startCellEdit(cellFocus, event.key);
  }
}

function selectedCellsTsv(): string | null {
  if (spreadsheetSelection?.anchor === cellAnchor && spreadsheetSelection.focus === cellFocus) return spreadsheetSelection.selected_tsv;
  return null;
}

async function copySelectedCellsTsv(): Promise<string> {
  if (!sheetId) return "";
  return await invoke("copy_spreadsheet_selection_tsv", { sheetId, anchor: cellAnchor, focus: cellFocus });
}

async function pasteCells(text: string): Promise<void> {
  if (!sheetId) return;
  await edit("paste_spreadsheet_tsv", { sheetId, origin: cellFocus, text });
  await renderSheets(query("[data-main]") as HTMLElement);
}

async function clearSelectedCells(): Promise<void> {
  if (!sheetId) return;
  await edit("clear_spreadsheet_selection", { sheetId, anchor: cellAnchor, focus: cellFocus });
  await renderSheets(query("[data-main]") as HTMLElement);
}

async function setCellFormat(property: string, value: string): Promise<void> {
  if (!sheetId) return;
  await edit("set_spreadsheet_selection_format", { sheetId, anchor: cellAnchor, focus: cellFocus, property, value });
  await renderSheets(query("[data-main]") as HTMLElement);
}

function currentSpreadsheetSelectionArgs(): { sheetId: string; anchor: string; focus: string } | null {
  return sheetId ? { sheetId, anchor: cellAnchor, focus: cellFocus } : null;
}

// ---- Actions -----------------------------------------------------------------

async function onSelectChange(kind: string, value: string): Promise<void> {
  if (kind === "style") {
    await runAction(value.startsWith("heading:") ? `style:heading:${value.split(":")[1]}` : value.startsWith("list:") ? `style:list:${value.split(":")[1]}` : "style:paragraph");
  } else if (kind === "font" || kind === "size") {
    await applyMark(kind, value, "set");
  } else if (kind === "align") {
    await setCellFormat("horizontal_align", value);
  } else if (kind === "number-format") {
    await setCellFormat("number_format", value);
  }
}

async function applyMark(kind: string, value: string | null, action: "toggle" | "set" | "remove"): Promise<void> {
  if (!selection) return;
  try {
    const result = await invoke("apply_editor_mark", { selection, mark_kind: kind, value, action });
    editorHooks.onResult(result);
    editor?.focus();
  } catch (error) {
    showError(error instanceof Error ? error.message : String(error));
  }
}

async function setBlockStyle(style: "paragraph" | "heading" | "list-item", level: number, ordered: boolean): Promise<void> {
  if (!selection) return;
  await edit("set_editor_selection_block_style", { selection, style, level, ordered });
  editor?.setSelection(selection);
  editor?.focus();
}

async function describeEditorSelection(): Promise<AppEditorSelection | null> {
  if (!selection) return null;
  try {
    return await run("describe_editor_selection", { selection });
  } catch {
    return null;
  }
}

function requireBlock(): AppBlock | null {
  const block = focusBlock() ?? doc?.blocks[doc.blocks.length - 1] ?? null;
  if (!block) showError("Place the caret in the document first.");
  return block;
}

function selectionText(): string {
  const domSelection = document.getSelection();
  return domSelection && !domSelection.isCollapsed ? domSelection.toString() : "";
}

function exportStyles(): string {
  return `.doc-body{font-family:Arial,sans-serif;max-width:52em;margin:2em auto;line-height:1.5}.mark-bold{font-weight:700}.mark-italic{font-style:italic}.mark-underline{text-decoration:underline}.mark-strike{text-decoration:line-through}.mark-code{font-family:monospace}.mark-superscript{vertical-align:super;font-size:.8em}.mark-subscript{vertical-align:sub;font-size:.8em}table{border-collapse:collapse}td{border:1px solid #999;padding:4px 8px}`;
}

function bytesFromBase64(base64: string): number[] {
  return Array.from(Uint8Array.from(atob(base64), (ch) => ch.charCodeAt(0)));
}

function textFromBase64(base64: string): string {
  return new TextDecoder().decode(Uint8Array.from(atob(base64), (ch) => ch.charCodeAt(0)));
}

function enterEditor(nextMode: Mode): void {
  view = "editor";
  mode = nextMode;
  app.innerHTML = "";
  renderAll();
  if (nextMode === "docs") editor?.focus();
}

async function runAction(action: string, data: DOMStringMap = {}): Promise<void> {
  if (action.startsWith("toggle-panel:")) {
    const which = action.slice("toggle-panel:".length) as Panel;
    panel = panel === which ? null : which;
    renderSidePanel();
    return;
  }
  if (action.startsWith("mark:")) {
    if (mode === "sheets") return;
    await applyMark(action.slice(5), null, "toggle");
    return;
  }
  if (action.startsWith("style:")) {
    const parts = action.split(":");
    if (parts[1] === "heading") await setBlockStyle("heading", Number(parts[2] ?? 1), false);
    else if (parts[1] === "list") await setBlockStyle("list-item", focusBlock()?.level ?? 0, parts[2] === "true");
    else await setBlockStyle("paragraph", 0, false);
    return;
  }
  if (action.startsWith("cell-format:")) {
    const property = action.slice("cell-format:".length);
    const cell = currentSheet()?.cells.find((item) => item.address === cellFocus);
    const current = property === "bold" ? cell?.format.bold : cell?.format.italic;
    await setCellFormat(property, current ? "false" : "true");
    return;
  }
  switch (action) {
    case "go-home":
      view = "home";
      renderAll();
      break;
    case "mode-docs":
    case "mode-sheets": {
      mode = action === "mode-docs" ? "docs" : "sheets";
      const main = query("[data-main]");
      if (main) main.innerHTML = "";
      renderAll();
      break;
    }
    case "dismiss-error":
      lastError = null;
      renderStatus();
      break;
    case "new-document":
    case "new-spreadsheet": {
      if (doc?.has_unsaved_changes && !(await confirmDialog("Discard unsaved changes?", "The current document has unsaved changes.", "Discard"))) return;
      await run("create_document", { title: "Untitled document" });
      enterEditor(action === "new-spreadsheet" ? "sheets" : "docs");
      break;
    }
    case "open-repository": {
      const path = isTauri()
        ? await pickOpenPath({ title: "Open OpenDoc folder", directory: true })
        : ((await promptDialog({ title: "Open repository", fields: [{ name: "path", label: "Repository path", value: doc?.repository_root ?? "" }] }))?.path ?? null);
      if (!path) return;
      const scanned = await run("scan_local_repository", { path });
      const documents = scanned.recent_documents.filter((recent) => recent.repository_root === path);
      if (documents.length === 0) {
        toast("No OpenDoc documents found in that folder.");
        return;
      }
      const choice =
        documents.length === 1
          ? documents[0].uuid
          : (await promptDialog({ title: "Open document", fields: [{ name: "uuid", label: "Document", type: "select", options: documents.map((item) => ({ value: item.uuid, label: item.title || item.uuid })) }] }))?.uuid;
      if (!choice) return;
      await run("open_local_repository", { path, documentUuid: choice });
      enterEditor("docs");
      break;
    }
    case "open-recent": {
      const root = data.root ?? "";
      const uuid = data.uuid ?? "";
      if (!root || !uuid) return;
      if (data.backend === "flat") await run("open_flat_repository", { path: root, namespace: data.namespace ?? "", documentUuid: uuid });
      else await run("open_local_repository", { path: root, documentUuid: uuid });
      enterEditor("docs");
      break;
    }
    case "import-word": {
      if (isTauri()) {
        const path = await pickOpenPath({ title: "Import Word document", extensions: ["docx", "doc"] });
        if (!path) return;
        await run("import_doc_or_docx_path", { path });
      } else {
        const file = await openFile(["docx", "doc"]);
        if (!file) return;
        await run("import_docx_base64", { name: file.name, base64: file.base64 });
      }
      enterEditor("docs");
      break;
    }
    case "import-json": {
      const file = await openFile(["json"]);
      if (!file) return;
      await run("import_google_docs_json", { title: file.name.replace(/\.json$/i, ""), jsonText: textFromBase64(file.base64) });
      enterEditor("docs");
      break;
    }
    case "save":
    case "save-as": {
      if (!doc) return;
      let path = action === "save" ? doc.repository_root : null;
      if (!path) {
        path = isTauri()
          ? await pickOpenPath({ title: "Choose a folder to save the document in", directory: true })
          : ((await promptDialog({ title: "Save", fields: [{ name: "path", label: "Repository path", value: doc.repository_root ?? "opendoc-repo" }] }))?.path ?? null);
      }
      if (!path) return;
      await run("save_local_repository", { path });
      toast("Saved");
      break;
    }
    case "export-json": {
      const text = await run("export_google_docs_json");
      await saveFile({ defaultName: `${doc?.title ?? "document"}.json`, extensions: ["json"], text, mediaType: "application/json" });
      break;
    }
    case "export-html": {
      const html = `<!doctype html><meta charset="utf-8"><title>${escapeHtml(doc?.title ?? "")}</title><style>${exportStyles()}</style><article class="doc-body">${doc?.body_html ?? ""}</article>${doc?.footnotes_html ?? ""}`;
      await saveFile({ defaultName: `${doc?.title ?? "document"}.html`, extensions: ["html"], text: html, mediaType: "text/html" });
      break;
    }
    case "export-text":
      await saveFile({ defaultName: `${doc?.title ?? "document"}.txt`, extensions: ["txt"], text: doc?.visible_text ?? "", mediaType: "text/plain" });
      break;
    case "print":
      window.print();
      break;
    case "rename": {
      const result = await promptDialog({ title: "Rename document", fields: [{ name: "title", label: "Title", value: doc?.title ?? "" }] });
      if (result) await edit("set_document_title", { title: result.title });
      break;
    }
    case "close-document":
      if (doc?.has_unsaved_changes && !(await confirmDialog("Close without saving?", "Unsaved changes will be lost.", "Close"))) return;
      await run("close_document");
      view = "home";
      renderAll();
      break;
    case "undo":
      await invoke("undo_current_edit").then(applyDocument).catch(() => toast("Nothing to undo"));
      editor?.setSelection(selection);
      break;
    case "redo":
      await invoke("redo_current_edit").then(applyDocument).catch(() => toast("Nothing to redo"));
      editor?.setSelection(selection);
      break;
    case "select-all": {
      if (!doc || mode !== "docs") return;
      const result = await invoke("select_all_editor_content");
      editorHooks.onResult(result);
      break;
    }
    case "find":
      showFind = true;
      renderFind();
      query<HTMLInputElement>("[data-find-input]")?.focus();
      break;
    case "find-close":
      showFind = false;
      renderFind();
      editor?.focus();
      break;
    case "find-next":
    case "find-prev":
      findIndex += action === "find-next" ? 1 : -1;
      highlightMatch();
      renderFind();
      break;
    case "replace-one":
    case "replace-all": {
      const replacement = query<HTMLInputElement>("[data-replace-input]")?.value ?? "";
      const matches = findMatches();
      if (matches.length === 0) return;
      const targets = action === "replace-all" ? matches.slice().reverse() : [matches[((findIndex % matches.length) + matches.length) % matches.length]];
      for (const match of targets) {
        const result = await editorHooks.apply({
          selection: {
            anchor: { block_id: match.block_id, inline_id: match.inline_id, offset: match.offset },
            focus: { block_id: match.block_id, inline_id: match.inline_id, offset: match.offset + match.length },
          },
          input_type: "insertText",
          data: replacement,
        });
        editorHooks.onResult(result);
      }
      renderFind();
      break;
    }
    case "comment": {
      const context = await describeEditorSelection();
      const range = context?.inline_range ?? null;
      const target = context?.focus_block_id ? findBlock(context.focus_block_id) : requireBlock();
      if (!target) return;
      const result = await promptDialog({ title: "Add comment", fields: [{ name: "body", label: "Comment", type: "textarea" }], submit: "Comment" });
      if (!result?.body.trim()) return;
      if (range) await edit("add_text_range_comment", { startInlineId: range.start, endInlineId: range.end, author: authorName, body: result.body });
      else await edit("add_block_comment", { blockId: target.id, author: authorName, body: result.body });
      panel = "comments";
      renderSidePanel();
      break;
    }
    case "reply-comment": {
      const result = await promptDialog({ title: "Reply", fields: [{ name: "body", label: "Reply", type: "textarea" }], submit: "Reply" });
      if (result?.body.trim() && data.id) await edit("add_comment_reply", { threadId: data.id, author: authorName, body: result.body });
      break;
    }
    case "resolve-comment":
      if (data.id) await edit("delete_comment_thread", { threadId: data.id });
      break;
    case "suggest": {
      const range = (await describeEditorSelection())?.inline_range ?? null;
      if (!range) {
        showError("Select the text to replace first.");
        return;
      }
      const result = await promptDialog({ title: "Suggest replacement", fields: [{ name: "text", label: "Replacement text", type: "textarea" }], submit: "Suggest" });
      if (result?.text) await edit("add_text_range_suggestion", { startInlineId: range.start, endInlineId: range.end, author: authorName, text: result.text });
      panel = "suggestions";
      renderSidePanel();
      break;
    }
    case "suggest-delete": {
      const range = (await describeEditorSelection())?.inline_range ?? null;
      if (!range) {
        showError("Select the text to delete first.");
        return;
      }
      await edit("add_text_range_delete_suggestion", { startInlineId: range.start, endInlineId: range.end, author: authorName });
      panel = "suggestions";
      renderSidePanel();
      break;
    }
    case "accept-suggestion":
      if (data.id) await edit("accept_suggestion", { suggestionId: data.id, acceptedBy: authorName });
      break;
    case "reject-suggestion":
      if (data.id) await edit("reject_suggestion", { suggestionId: data.id, rejectedBy: authorName });
      break;
    case "accept-all":
      await edit("accept_all_suggestions", { acceptedBy: authorName });
      break;
    case "reject-all":
      await edit("reject_all_suggestions", { rejectedBy: authorName });
      break;
    case "insert-table": {
      const target = requireBlock();
      if (!target) return;
      const result = await promptDialog({ title: "Insert table", fields: [{ name: "rows", label: "Rows", type: "number", value: "3" }, { name: "columns", label: "Columns", type: "number", value: "3" }], submit: "Insert" });
      if (!result) return;
      await edit("insert_table_after", { afterBlockId: target.id, rows: Number(result.rows) || 2, columns: Number(result.columns) || 2 });
      break;
    }
    case "insert-page-break": {
      const target = requireBlock();
      if (target) await edit("insert_page_break_after", { afterBlockId: target.id });
      break;
    }
    case "insert-link": {
      const target = requireBlock();
      if (!target) return;
      const inline = focusInline();
      const existing = inline?.kind === "link" ? inline : null;
      const selectedText = selectionText();
      const result = await promptDialog({
        title: existing ? "Edit link" : "Insert link",
        fields: [
          { name: "text", label: "Text", value: existing?.text ?? selectedText },
          { name: "href", label: "Link", value: existing?.href ?? "", placeholder: "https://" },
        ],
        submit: existing ? "Update" : "Insert",
      });
      if (!result) return;
      if (existing) await edit("update_link_href", { inlineId: existing.id, href: result.href });
      else if (selectedText && selection) await applyMark("link", result.href, "set");
      else await edit("insert_link_after", { blockId: target.id, afterInlineId: selection?.focus.inline_id ?? null, text: result.text || result.href, href: result.href });
      break;
    }
    case "insert-footnote": {
      const target = requireBlock();
      if (target) await edit("insert_footnote_ref_after", { blockId: target.id, afterInlineId: selection?.focus.inline_id ?? null });
      panel = "footnotes";
      renderSidePanel();
      break;
    }
    case "edit-footnote": {
      const note = doc?.footnotes.find((item) => item.id === data.id);
      const result = await promptDialog({ title: "Edit footnote", fields: [{ name: "body", label: "Footnote text", type: "textarea", value: note?.body.map((inline) => inline.text).join("") ?? "" }], submit: "Save" });
      if (result && data.id) await edit("update_footnote_body", { footnoteId: data.id, body: result.body });
      break;
    }
    case "insert-equation":
    case "insert-equation-block": {
      const target = requireBlock();
      if (!target) return;
      const result = await promptDialog({ title: "Insert equation", fields: [{ name: "source", label: "LaTeX", placeholder: "\\frac{a}{b}" }], submit: "Insert" });
      if (!result?.source) return;
      if (action === "insert-equation") await edit("insert_equation_after", { blockId: target.id, afterInlineId: selection?.focus.inline_id ?? null, source: result.source });
      else await edit("insert_equation_block_after", { afterBlockId: target.id, source: result.source });
      break;
    }
    case "insert-mention": {
      const target = requireBlock();
      if (!target) return;
      const result = await promptDialog({ title: "Mention", fields: [{ name: "label", label: "Name", value: "@" }], submit: "Insert" });
      if (result?.label) await edit("insert_mention_after", { blockId: target.id, afterInlineId: selection?.focus.inline_id ?? null, label: result.label });
      break;
    }
    case "insert-image":
    case "attach-file": {
      const file = await openFile(action === "insert-image" ? ["png", "jpg", "jpeg", "gif", "webp", "svg"] : ["*"]);
      if (!file) return;
      const updated = await run("add_binary_blob", { name: file.name, mediaType: file.media_type, bytes: bytesFromBase64(file.base64) });
      const blob = updated.blobs.find((item) => item.name === file.name);
      const target = requireBlock();
      if (action === "insert-image" && blob && target) await edit("insert_image_block_after", { afterBlockId: target.id, blobHash: blob.hash, altText: file.name });
      if (action === "attach-file") {
        panel = "files";
        renderSidePanel();
      }
      break;
    }
    case "insert-blob-image": {
      const target = requireBlock();
      if (target && data.hash) await edit("insert_image_block_after", { afterBlockId: target.id, blobHash: data.hash, altText: data.name ?? "" });
      break;
    }
    case "delete-blob":
      if (data.hash) await edit("delete_binary_blob", { blobHash: data.hash });
      break;
    case "insert-citation":
    case "cite":
    case "cite-footnote": {
      if (!doc) return;
      let referenceId = data.id ?? "";
      let locator = "";
      if (!referenceId) {
        if (doc.citations.references.length === 0) {
          toast("Add a reference first (Citations panel).");
          panel = "citations";
          renderSidePanel();
          return;
        }
        const chosen = await promptDialog({
          title: "Insert citation",
          fields: [
            { name: "reference", label: "Reference", type: "select", options: doc.citations.references.map((reference) => ({ value: reference.id, label: reference.title })) },
            { name: "locator", label: "Page / locator (optional)" },
          ],
          submit: "Cite",
        });
        if (!chosen) return;
        referenceId = chosen.reference;
        locator = chosen.locator;
      }
      const target = requireBlock();
      if (!target) return;
      if (action === "cite-footnote") {
        await edit("insert_footnote_citation_after", {
          blockId: target.id,
          afterInlineId: selection?.focus.inline_id ?? null,
          items: [{ reference_id: referenceId, locator: locator || null, label: null, prefix: null, suffix: null, suppress_author: false }],
        });
      } else {
        await edit("insert_citation", { referenceId, afterInlineId: selection?.focus.inline_id ?? null, locator: locator || null, label: null, prefix: null, suffix: null, suppressAuthor: false });
      }
      break;
    }
    case "add-reference": {
      const result = await promptDialog({
        title: "Add reference",
        fields: [
          { name: "title", label: "Title" },
          { name: "authors", label: "Authors (one per line)", type: "textarea" },
          { name: "issued", label: "Year" },
          { name: "doi", label: "DOI" },
          { name: "url", label: "URL" },
        ],
        submit: "Add",
      });
      if (!result?.title) return;
      await edit("add_bibliography_reference", {
        title: result.title,
        authors: result.authors.split("\n").map((line) => line.trim()).filter(Boolean),
        issued: result.issued || null,
        doi: result.doi || null,
        url: result.url || null,
      });
      break;
    }
    case "import-bibtex": {
      showError("Bibliography import is not available until the Rust command contract is regenerated.");
      break;
    }
    case "delete-reference":
      if (data.id) await edit("delete_bibliography_reference", { referenceId: data.id });
      break;
    case "citation-style": {
      const result = await promptDialog({
        title: "Citation style",
        fields: [
          {
            name: "style",
            label: "Style",
            type: "select",
            value: doc?.citations.style,
            options: ["apa", "mla", "chicago-author-date", "chicago-notes", "ieee", "vancouver", "harvard", "nature", "author-year", "numeric"].map((style) => ({ value: style, label: style })),
          },
          { name: "locale", label: "Locale", value: doc?.citations.locale ?? "en-US" },
        ],
        submit: "Apply",
      });
      if (result) await edit("set_citation_style", { style: result.style, locale: result.locale });
      break;
    }
    case "clear-marks":
      await applyMark("all", null, "remove");
      break;
    case "indent":
    case "outdent": {
      if (!selection) return;
      await edit("adjust_editor_selection_list_indent", { selection, delta: action === "indent" ? 1 : -1 });
      editor?.setSelection(selection);
      break;
    }
    case "zoom-in":
    case "zoom-out":
    case "zoom-reset":
      zoom = action === "zoom-reset" ? 1 : Math.min(3, Math.max(0.5, zoom + (action === "zoom-in" ? 0.1 : -0.1)));
      renderToolbar();
      renderMain();
      break;
    case "word-count":
      await promptDialog({ title: "Word count", fields: [{ name: "stats", label: wordStats(), value: "" }], submit: "Close" });
      break;
    case "shortcuts":
      await promptDialog({
        title: "Keyboard shortcuts",
        fields: [{ name: "list", label: MENUS.flatMap((menu) => menu.items.filter((item) => item.shortcut).map((item) => `${item.shortcut}: ${item.label}`)).join("\n"), value: "" }],
        submit: "Close",
      });
      break;
    case "about":
      await promptDialog({ title: "About OpenDoc", fields: [{ name: "about", label: `OpenDoc – an open source, Rust-first document and spreadsheet editor. Runtime: ${profile?.label ?? runtime.mode}.`, value: "" }], submit: "Close" });
      break;
    case "share":
      toast("Sharing needs a collaboration server, which is not available in this runtime yet.");
      break;
    case "sign": {
      const result = await promptDialog({
        title: "Sign document",
        fields: [
          { name: "signer", label: "Your name", value: authorName },
          { name: "key", label: "OpenSSH private key (ed25519)", type: "textarea", placeholder: "-----BEGIN OPENSSH PRIVATE KEY-----" },
        ],
        submit: "Sign",
      });
      if (result?.key) await edit("sign_with_openssh_private_key", { privateKeyPem: result.key, signerDisplay: result.signer });
      break;
    }
    case "verify": {
      const text = await run("verify_current_signatures");
      toast(text);
      break;
    }
    // Spreadsheet actions
    case "select-sheet":
      sheetId = data.id ?? sheetId;
      cellAnchor = "A1";
      cellFocus = "A1";
      await renderSheets(query("[data-main]") as HTMLElement);
      break;
    case "add-sheet": {
      const result = await promptDialog({ title: "Add sheet", fields: [{ name: "title", label: "Sheet name", value: `Sheet${(doc?.workbook.sheets.length ?? 0) + 1}` }], submit: "Add" });
      if (result?.title) await edit("add_spreadsheet_sheet", { title: result.title });
      break;
    }
    case "add-row":
    case "add-column":
    case "delete-row":
    case "delete-column": {
      const args = currentSpreadsheetSelectionArgs();
      if (!args) return;
      if (action === "add-row") await edit("add_spreadsheet_row_after_selection", args);
      if (action === "add-column") await edit("add_spreadsheet_column_after_selection", args);
      if (action === "delete-row") await edit("delete_spreadsheet_selection_row", args);
      if (action === "delete-column") await edit("delete_spreadsheet_selection_column", args);
      break;
    }
    case "merge-cells": {
      const args = currentSpreadsheetSelectionArgs();
      if (args) await edit("merge_spreadsheet_selection", args);
      break;
    }
    case "freeze": {
      const args = currentSpreadsheetSelectionArgs();
      if (args) await edit("freeze_spreadsheet_selection", args);
      break;
    }
    case "filter": {
      const args = currentSpreadsheetSelectionArgs();
      if (args) await edit("set_spreadsheet_selection_filter", args);
      break;
    }
    default:
      showError(`Unknown action ${action}`);
  }
}

// ---- Static bindings --------------------------------------------------------------

function bindStatic(): void {
  app.addEventListener("click", (event) => {
    const target = (event.target as Element).closest<HTMLElement>("[data-action]");
    if (!target || target.hasAttribute("disabled")) return;
    const menu = target.closest("details.menu-group");
    if (menu) menu.removeAttribute("open");
    void runAction(target.dataset.action ?? "", target.dataset);
  });
  const title = query<HTMLInputElement>("[data-doc-title]");
  title?.addEventListener("change", () => void edit("set_document_title", { title: title.value }));
  title?.addEventListener("keydown", (event) => {
    if (event.key === "Enter") {
      event.preventDefault();
      title.blur();
      editor?.focus();
    }
  });
  const author = query<HTMLInputElement>("[data-author]");
  author?.addEventListener("change", () => {
    authorName = author.value.trim() || "Local user";
  });
  app.addEventListener(
    "toggle",
    (event) => {
      const opened = event.target as HTMLElement;
      if (opened.tagName === "DETAILS" && (opened as HTMLDetailsElement).open) {
        app.querySelectorAll<HTMLDetailsElement>("details.menu-group[open]").forEach((other) => {
          if (other !== opened) other.open = false;
        });
      }
    },
    true,
  );
  document.addEventListener("click", (event) => {
    if (!(event.target as Element).closest("details.menu-group")) {
      app.querySelectorAll<HTMLDetailsElement>("details.menu-group[open]").forEach((other) => {
        other.open = false;
      });
    }
  });
  document.addEventListener("keydown", (event) => {
    if (event.key === "Escape") {
      app.querySelectorAll<HTMLDetailsElement>("details.menu-group[open]").forEach((other) => {
        other.open = false;
      });
    }
    const active = document.activeElement as HTMLElement | null;
    if (active?.closest("[data-menu-bar]")) {
      const items = Array.from(app.querySelectorAll<HTMLElement>("[data-menu-bar] summary, [data-menu-bar] details[open] [role=menuitem]"));
      const index = items.indexOf(active);
      if (event.key === "ArrowRight" || event.key === "ArrowDown") {
        event.preventDefault();
        items[(index + 1) % items.length]?.focus();
      } else if (event.key === "ArrowLeft" || event.key === "ArrowUp") {
        event.preventDefault();
        items[(index - 1 + items.length) % items.length]?.focus();
      }
    }
  });
}

// ---- Boot ----------------------------------------------------------------------

async function boot(): Promise<void> {
  try {
    profile = await invoke("get_runtime_profile", { mode: runtime.mode, storageBackends: runtime.storageBackends ?? [], signingEnabled: runtime.signingEnabled });
  } catch (error) {
    console.warn("runtime profile unavailable", error);
  }
  try {
    doc = await invoke("get_document");
    audit = null;
  } catch (error) {
    app.innerHTML = `<main class="shell"><p class="error-banner">Could not start OpenDoc: ${escapeHtml(String(error))}</p></main>`;
    return;
  }
  renderAll();
  window.setInterval(() => {
    if (!doc?.repository_root || !doc.has_unsaved_changes) return;
    invoke("autosave_current_repository")
      .then((updated) => {
        doc = updated;
        renderStatus();
      })
      .catch(() => {});
  }, 5000);
  window.addEventListener("beforeunload", (event) => {
    if (doc?.has_unsaved_changes && !isTauri()) {
      event.preventDefault();
    }
  });
  void onCloseRequested(async () => {
    const choice = await promptDialog({
      title: "Unsaved changes",
      fields: [
        {
          name: "choice",
          label: "What do you want to do?",
          type: "select",
          value: "save",
          options: [
            { value: "save", label: "Save and close" },
            { value: "discard", label: "Discard changes and close" },
          ],
        },
      ],
      submit: "Continue",
      cancel: "Keep editing",
    });
    if (!choice) return;
    if (choice.choice === "save") await runAction("save");
    if (!doc?.has_unsaved_changes || choice.choice === "discard") await closeWindow();
  });
}

export const __test = { runAction, getState: () => ({ doc, view, mode, panel, selection, audit }), setSelection: (next: EditorSelection | null) => (selection = next) };

void boot();
