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
import { escapeHtml, promptDialog } from "./ui";
import { invoke, setWindowTitle } from "./invoke";
import type { EditorInput, EditorResult, EditorSelection } from "./types";
import type { Mode } from "./state";
import { app, editorHost, state } from "./state";
import { bindStatic } from "./bindings";
import { edit, findInline, focusBlock, query, showError, wordStats } from "./shared";
import { renderHome } from "./home";
import { renderMenus } from "./menus";
import { ALIGNMENT_SHORTCUTS, renderToolbar } from "./toolbar";
import { refreshFind, renderFind } from "./find";
import { applyPageGeometry, paginate } from "./pagination";
import { renderSidePanel } from "./panels";
import { clearRemotePresenceOverlay, refreshRemotePresence } from "./collab";
import { renderSheets } from "./spreadsheet";
import { clearFocusedImageSelection, insertImageFiles, resizeFocusedImageFromKeyboard } from "./images";
import { runAction } from "./actions";
import { appendTableRowFromLastCell, moveCaretToAdjacentCell } from "./tables";

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
              <div class="status" data-status role="status" aria-live="polite" aria-atomic="true"></div>
            </div>
          </div>
          <div class="topbar-right">
            <div class="mode-switch" role="tablist">
              <button type="button" role="tab" data-action="mode-docs">Document</button>
              <button type="button" role="tab" data-action="mode-sheets">Spreadsheet</button>
            </div>
            <input class="author" data-author aria-label="Your name" value="${escapeHtml(state.authorName)}" title="Name used for comments and suggestions">
            <label class="sr-only" for="document-editing-mode">Editing mode</label><select id="document-editing-mode" data-document-editing-mode title="Editing mode" ${state.runtimeSession?.service_session?.role === "viewer" ? "disabled" : ""}><option value="edit" ${state.documentEditingMode === "edit" ? "selected" : ""}>Edit</option><option value="suggest" ${state.documentEditingMode === "suggest" ? "selected" : ""}>Suggest</option><option value="view" ${state.documentEditingMode === "view" ? "selected" : ""}>View</option></select>
            <button type="button" class="primary" data-action="share">Share</button>
          </div>
        </header>
        <nav class="menu-bar" role="menubar" data-menu-bar></nav>
        <div class="toolbar" role="toolbar" data-toolbar></div>
        <div class="find-bar" data-find hidden></div>
        <div class="error-banner" role="alert" data-error hidden></div>
        <p class="sr-only" role="status" aria-live="polite" aria-atomic="true" data-activity-status></p>
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
  applyWindowTitle();
}

/**
 * The OS window title: the document's name, and the unsaved marker.
 *
 * Every path that changes `has_unsaved_changes` has to come through here.
 * `renderAll` did it and `editorHooks.onResult` did not, so the first
 * keystroke on a saved document left the titlebar saying the document was
 * clean — the status line said "Unsaved changes" and the window disagreed.
 */
function applyWindowTitle(): void {
  const doc = state.doc;
  if (!doc) return;
  void setWindowTitle(`${doc.title}${doc.has_unsaved_changes ? " •" : ""} – OpenDoc`);
}

export function renderStatus(): void {
  const status = query("[data-status]");
  const title = query<HTMLInputElement>("[data-doc-title]");
  const error = query("[data-error]");
  const workspace = query<HTMLElement>("[data-workspace]");
  const activity = query<HTMLElement>("[data-activity-status]");
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
  // A command may re-render the whole projection while it is still in
  // flight. Reading the count here (rather than setting this only at command
  // start) re-attests the state on the newly created workspace too.
  const busy = state.pendingOperations > 0;
  workspace?.setAttribute("aria-busy", String(busy));
  if (activity) activity.textContent = busy ? "Working…" : "";
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
    // A contenteditable textbox has no implicit accessible name. Keep this
    // local projection in sync with the durable title so a screen reader does
    // not announce only an anonymous multi-line edit field on entry.
    editorHost.setAttribute("aria-label", `Document editor: ${doc.title || "Untitled document"}`);
    // Browser spellcheck and grammar services select their dictionary from
    // the nearest language.  The document locale is durable source metadata,
    // so projecting it here keeps native assistance aligned with the document
    // rather than the browser UI language, without persisting browser state.
    editorHost.setAttribute("lang", doc.locale);
    state.editor?.setEditable(state.documentEditingMode !== "view");
    // The remote overlay is resolved against the old DOM. Clear it before a
    // source morph so a deleted remote range is never painted for one more
    // animation frame; `refreshRemotePresence` below repaints only live text.
    clearRemotePresenceOverlay();
    state.editor?.setFragments(doc.body_fragments);
    // Presence is projection-only and lives outside the contenteditable host.
    // A morph can move the text its remote anchor names, so re-resolve after
    // the document itself is safely up to date.
    refreshRemotePresence();
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

async function activateDateChip(inlineId: string): Promise<void> {
  const inline = findInline(inlineId)?.inline;
  if (!inline?.date) return;
  const result = await promptDialog({
    title: "Edit date",
    fields: [{ name: "date", label: "Date", type: "date", value: inline.date }],
    submit: "Update",
  });
  if (result?.date && result.date !== inline.date) {
    await edit("update_date_chip", { inlineId, date: result.date });
  }
}

async function activateDropdown(inlineId: string): Promise<void> {
  const inline = findInline(inlineId)?.inline;
  if (!inline?.dropdown_options?.length || !inline.selected_option_id) return;
  const result = await promptDialog({
    title: "Choose dropdown value",
    fields: [{
      name: "optionId",
      label: "Value",
      type: "select",
      value: inline.selected_option_id,
      options: inline.dropdown_options.map((option) => ({ value: option.id, label: option.label })),
    }],
    submit: "Select",
  });
  if (result?.optionId) await edit("select_dropdown_option", { inlineId, optionId: result.optionId });
}

export const editorHooks = {
  apply: async (input: EditorInput): Promise<EditorResult> => {
    if (state.documentEditingMode === "suggest") {
      const range = input.selection.anchor.inline_id && input.selection.focus.inline_id
        ? { startInlineId: input.selection.anchor.inline_id, endInlineId: input.selection.focus.inline_id }
        : null;
      // `beforeinput` formatting commands are emitted by browser editing
      // affordances (including accessibility tools), bypassing our Ctrl-key
      // shortcut path. They have exactly the same whole-run semantics as the
      // corresponding toolbar toggles, so retain them as review proposals.
      const nativeFormatActions: Record<string, string> = {
        formatBold: "mark:bold",
        formatItalic: "mark:italic",
        formatUnderline: "mark:underline",
        formatStrikeThrough: "mark:strike",
      };
      const nativeFormatAction = nativeFormatActions[input.input_type];
      if (nativeFormatAction) {
        await runAction(nativeFormatAction);
        if (!state.doc) throw new Error("No document is open.");
        return { document: state.doc, selection: input.selection, handled: true };
      }
      let document: typeof state.doc = null;
      if (input.input_type === "insertText" && input.data) {
        document = await invoke("add_block_suggestion", { blockId: input.selection.focus.block_id, author: state.authorName, text: input.data });
      } else if (
        input.input_type === "insertFromPaste"
        && input.data
        // An Insert suggestion currently names one plain inline sequence.
        // Treating rich or multi-paragraph clipboard content as that sequence
        // would silently lose marks, blocks, and table structure.
        && !input.html
        && !/[\r\n]/.test(input.data)
      ) {
        document = await invoke("add_block_suggestion", { blockId: input.selection.focus.block_id, author: state.authorName, text: input.data });
      } else if (input.input_type === "insertParagraph" && isParagraphEndEnter(input.selection)) {
        document = await invoke("add_block_insert_suggestion", { blockId: input.selection.focus.block_id, author: state.authorName, text: "" });
      } else if (input.input_type.startsWith("delete") && range) {
        document = await invoke("add_text_range_delete_suggestion", { ...range, author: state.authorName });
      }
      if (document) return { document, selection: input.selection, handled: true };
      throw new Error("Suggest mode supports typing, a plain one-line paste, and deleting a selected range; use the review panel for other suggestion types.");
    }
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
    state.editor?.setFragments(doc.body_fragments);
    state.editor?.setSelection(result.selection);
    state.selection = result.selection;
    renderStatus();
    renderToolbar();
    applyWindowTitle();
    const notes = query("[data-footnotes]");
    if (notes) {
      notes.innerHTML = doc.footnotes_html;
      notes.hidden = !doc.footnotes_html;
    }
    // Typing changes how tall the flow is, so the page boxes have to be
    // measured again. `setFragments` has already patched the DOM above.
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
  onAtomicSelectionRemoved: () => {
    // Selection-change events can report a transient null while a browser is
    // applying a programmatic caret. This explicit editor signal is the one
    // null that is a durable fact: its atomic source block was removed.
    state.selection = null;
    renderToolbar();
  },
  onError: (message: string) => showError(message),
  onInsertFiles: async (files: File[], afterBlockId: string | null): Promise<void> => {
    await insertImageFiles(files, afterBlockId);
  },
  onDropdownActivate: async (inlineId: string): Promise<void> => {
    await activateDropdown(inlineId);
  },
  onDateChipActivate: async (inlineId: string): Promise<void> => {
    await activateDateChip(inlineId);
  },
  onKeydown: async (event: KeyboardEvent, current: EditorSelection | null): Promise<boolean> => {
    if (current) state.selection = current;
    if (await resizeFocusedImageFromKeyboard(event)) return true;
    const activeChecklist = document.activeElement instanceof Element
      ? document.activeElement.closest<HTMLElement>("[data-action='toggle-checklist-item'][data-checklist-block-id]")
      : null;
    if (activeChecklist && (event.key === "Enter" || event.key === " ")) {
      if (state.documentEditingMode !== "view") {
        await runAction("toggle-checklist-item", activeChecklist.dataset);
      }
      return true;
    }
    const activeDropdown = document.activeElement instanceof Element
      ? document.activeElement.closest<HTMLElement>("[data-inline-kind=dropdown][data-inline-id]")
      : null;
    if (activeDropdown && (event.key === "Enter" || event.key === " ")) {
      await activateDropdown(activeDropdown.dataset.inlineId ?? "");
      return true;
    }
    const activeDateChip = document.activeElement instanceof Element
      ? document.activeElement.closest<HTMLElement>("[data-inline-kind=date-chip][data-inline-id]")
      : null;
    if (activeDateChip && (event.key === "Enter" || event.key === " ")) {
      await activateDateChip(activeDateChip.dataset.inlineId ?? "");
      return true;
    }
    const ctrl = event.ctrlKey || event.metaKey;
    const key = event.key.toLowerCase();
    // Tab, and the focus trap it used to be.
    //
    // The old rule consumed Tab unconditionally and, outside a list, typed a
    // literal "\t" into the document. A keyboard-only user could therefore
    // not leave the document at all, Tab did not move between table cells,
    // and the tab character only looked like one because `.doc-body` sets
    // `white-space: pre-wrap` — which `opendoc-layout` then has to measure.
    //
    // Measured in real Chrome: when this handler does *not* consume Tab,
    // Chrome moves focus out of the editable body to the next control in the
    // strip and inserts nothing. So the escape hatch costs nothing but
    // letting the key through, and the rule below is three cases:
    //
    // * in a list item, Tab and Shift+Tab indent and outdent, as in Docs;
    // * in a table, they step to the next and previous cell; Tab at the final
    //   cell appends a row as Docs does, while Shift+Tab at the first cell can
    //   still leave the table;
    // * anywhere else they are let through, and focus leaves the document.
    //
    // A list is the one place Tab still never escapes, so Escape blurs the
    // editor — the answer every accessible editor gives, and the one the
    // shortcut sheet can state.
    if (event.key === "Tab") {
      const block = focusBlock();
      if (block?.kind === "list-item") {
        await runAction(event.shiftKey ? "outdent" : "indent");
        return true;
      }
      if (moveCaretToAdjacentCell(!event.shiftKey)) return true;
      return event.shiftKey ? false : await appendTableRowFromLastCell();
    }
    if (event.key === "Escape" && !event.shiftKey && !ctrl && clearFocusedImageSelection()) {
      return true;
    }
    if (event.key === "Escape" && !event.shiftKey && !ctrl && state.editor?.clearFocusedAtomicSelection()) {
      return true;
    }
    if (event.key === "Escape" && !event.shiftKey && !ctrl) {
      (document.activeElement as HTMLElement | null)?.blur();
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

/**
 * The only Enter shape the current structural-proposal vocabulary can name:
 * a collapsed caret after the final text run of a body paragraph. It becomes
 * an exact empty `BlockInsert` after that sibling. Mid-paragraph Enter would
 * split content, while heading/list/table-cell Enter carries extra semantics.
 */
function isParagraphEndEnter(selection: EditorSelection): boolean {
  if (
    selection.anchor.block_id !== selection.focus.block_id
    || selection.anchor.inline_id !== selection.focus.inline_id
    || selection.anchor.offset !== selection.focus.offset
    || !selection.focus.inline_id
  ) return false;
  const block = state.doc?.blocks.find((candidate) => candidate.id === selection.focus.block_id);
  if (block?.kind !== "paragraph") return false;
  const last = block.content.at(-1);
  return last?.id === selection.focus.inline_id && Array.from(last.text).length === selection.focus.offset;
}


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
  const editingMode = query<HTMLSelectElement>("[data-document-editing-mode]");
  editingMode?.addEventListener("change", () => {
    state.documentEditingMode = editingMode.value as typeof state.documentEditingMode;
    state.editor?.setEditable(state.documentEditingMode !== "view");
    // The review panel's mutation controls share this mode guard. Rebuild it
    // immediately so View does not leave an enabled-looking control behind
    // until an unrelated document render happens.
    renderSidePanel();
  });
}
