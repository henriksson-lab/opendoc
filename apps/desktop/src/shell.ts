// The editor shell: the frame the document and spreadsheet surfaces live in,
// and the render pass that keeps it in step with the document projection.
//
// `renderAll` is the one entry point. It builds the shell markup once per
// editor session — `[data-editor-shell]` is the guard — then delegates to the
// surface renderers. Two listener rules meet here and must stay apart:
// `bindStatic()` (in `main.ts`) attaches once to nodes that outlive a render;
// `bindShellControls()` re-binds per shell build, which is safe precisely
// because the nodes it binds are discarded with the shell.
import { DocumentEditor } from "./editor";
import { escapeHtml } from "./ui";
import { invoke, setWindowTitle } from "./invoke";
import type { EditorInput, EditorResult, EditorSelection } from "./types";
import type { Mode } from "./state";
import { app, editorHost, state } from "./state";
import { bindStatic } from "./bindings";
import { edit, focusBlock, query, showError, wordStats } from "./shared";
import { renderHome } from "./home";
import { renderMenus } from "./menus";
import { ALIGNMENT_SHORTCUTS, renderToolbar } from "./toolbar";
import { refreshFind, renderFind } from "./find";
import { applyPageGeometry, paginate } from "./pagination";
import { renderSidePanel } from "./panels";
import { renderSheets } from "./spreadsheet";
import { insertImageFiles } from "./images";
import { runAction } from "./actions";

export function renderAll(): void {
  bindStatic();
  if (!state.doc) {
    app.innerHTML = `<main class="shell"><p class="loading">Loading…</p></main>`;
    return;
  }
  if (state.view === "home") {
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
            <input class="author" data-author aria-label="Your name" value="${escapeHtml(state.authorName)}" title="Name used for comments and suggestions">
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
    bindShellControls();
  }
  renderMenus();
  renderToolbar();
  renderStatus();
  renderMain();
  renderSidePanel();
  void setWindowTitle(`${state.doc.title}${state.doc.has_unsaved_changes ? " •" : ""} – OpenDoc`);
}

export function renderStatus(): void {
  const status = query("[data-status]");
  const title = query<HTMLInputElement>("[data-doc-title]");
  const error = query("[data-error]");
  const doc = state.doc;
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
    error.hidden = !state.lastError;
    error.innerHTML = state.lastError ? `<span>${escapeHtml(state.lastError)}</span><button type="button" data-action="dismiss-error" aria-label="Dismiss">✕</button>` : "";
  }
  app.querySelectorAll<HTMLElement>("[data-action='mode-docs'],[data-action='mode-sheets']").forEach((tab) => {
    const active = (tab.dataset.action === "mode-docs") === (state.mode === "docs");
    tab.setAttribute("aria-selected", String(active));
    tab.classList.toggle("active", active);
  });
  renderFind();
  // The document just changed under the find bar, so its match list has to be
  // re-asked for rather than re-derived here.
  void refreshFind();
}

export function renderMain(): void {
  const main = query("[data-main]");
  const doc = state.doc;
  if (!main || !doc) return;
  if (!doc.is_open) {
    main.innerHTML = `<section class="closed-panel"><h2>No document open</h2><button type="button" data-action="go-home">Back to home</button></section>`;
    return;
  }
  if (state.mode === "docs") {
    let page = query("[data-page]", main);
    if (!page) {
      main.innerHTML = `<div class="page-stack" data-page-stack><div class="page-canvas" data-page-canvas><div class="page-sheets" data-page-sheets aria-hidden="true"></div><article class="page" data-page></article></div><section class="footnote-area" data-footnotes></section></div>`;
      page = query("[data-page]", main) as HTMLElement;
      page.appendChild(editorHost);
      // `editorHost` is a module-level element that survives every rebuild, so
      // the old instance must let go of its listeners before a new one binds.
      state.editor?.destroy();
      state.editor = new DocumentEditor(editorHost, editorHooks);
    }
    state.editor?.setHtml(doc.body_html);
    const notes = query("[data-footnotes]", main);
    if (notes) {
      notes.innerHTML = doc.footnotes_html;
      notes.hidden = !doc.footnotes_html;
    }
    // `applyPageGeometry` writes the whole style attribute, zoom included, so
    // the page's geometry and its zoom cannot be set from two places.
    applyPageGeometry();
    void paginate();
  } else {
    void renderSheets(main);
  }
}


// ---- Editor hooks ------------------------------------------------------------

export const editorHooks = {
  apply: async (input: EditorInput): Promise<EditorResult> => {
    return invoke("apply_editor_input", {
      selection: input.selection,
      input_type: input.input_type,
      data: input.data,
      html: input.html ?? null,
    });
  },
  onResult: (result: EditorResult) => {
    const doc = result.document;
    state.doc = doc;
    state.lastError = null;
    state.editor?.setHtml(doc.body_html);
    state.editor?.setSelection(result.selection);
    state.selection = result.selection;
    renderStatus();
    renderToolbar();
    const notes = query("[data-footnotes]");
    if (notes) {
      notes.innerHTML = doc.footnotes_html;
      notes.hidden = !doc.footnotes_html;
    }
    // Typing changes how tall the flow is, so the page boxes have to be
    // measured again. `setHtml` has already patched the DOM above.
    applyPageGeometry();
    void paginate();
    if (state.panel) renderSidePanel();
  },
  onSelectionChange: (next: EditorSelection | null) => {
    if (next) {
      state.selection = next;
      renderToolbar();
    }
  },
  onError: (message: string) => showError(message),
  onInsertFiles: async (files: File[], afterBlockId: string | null): Promise<void> => {
    await insertImageFiles(files, afterBlockId);
  },
  onKeydown: async (event: KeyboardEvent, current: EditorSelection | null): Promise<boolean> => {
    if (current) state.selection = current;
    const ctrl = event.ctrlKey || event.metaKey;
    const key = event.key.toLowerCase();
    if (event.key === "Tab") {
      const block = focusBlock();
      if (block?.kind === "list-item") {
        await runAction(event.shiftKey ? "outdent" : "indent");
      } else if (!event.shiftKey && state.selection) {
        await editorHooks
          .apply({ selection: state.selection, input_type: "insertText", data: "\t" })
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
      await runAction("style:list:ordered");
      return true;
    }
    if (event.shiftKey && (event.key === "8" || event.key === "*")) {
      await runAction("style:list:bullet");
      return true;
    }
    if (event.shiftKey && ALIGNMENT_SHORTCUTS[key]) {
      await runAction(`align:${ALIGNMENT_SHORTCUTS[key]}`);
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


export function enterEditor(nextMode: Mode): void {
  state.view = "editor";
  state.mode = nextMode;
  app.innerHTML = "";
  renderAll();
  if (nextMode === "docs") state.editor?.focus();
}

/**
 * Listeners on controls that are recreated with the editor shell. Rebinding them
 * per shell build is safe: the previous nodes are discarded with their listeners.
 */
function bindShellControls(): void {
  const title = query<HTMLInputElement>("[data-doc-title]");
  title?.addEventListener("change", () => void edit("set_document_title", { title: title.value }));
  title?.addEventListener("keydown", (event) => {
    if (event.key === "Enter") {
      event.preventDefault();
      title.blur();
      state.editor?.focus();
    }
  });
  const author = query<HTMLInputElement>("[data-author]");
  author?.addEventListener("change", () => {
    state.authorName = author.value.trim() || "Local user";
  });
}

