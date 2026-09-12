// The delegated action dispatcher.
//
// Every UI gesture in OpenDoc names an action — a `data-action` attribute, a
// keyboard shortcut, a dialog choice — and every one of them arrives here.
// That is what lets the whole app get by with a single click listener on the
// never-replaced `app` element (see `bindStatic` in `main.ts`) instead of a
// listener per control that a re-render could stack.
//
// Actions that belong to one surface are routed to that surface's own handler,
// which returns false for a name it does not own; what is left below is the
// shared vocabulary — panels, marks, styles, insertion, citations, zoom.
import { promptDialog, toast } from "./ui";
import { invoke, openFile } from "./invoke";
import type { ListKindName, Panel } from "./state";
import { runtime, state } from "./state";
import {
  ACTION_CANCELLED,
  applyDocument,
  bytesFromBase64,
  describeEditorSelection,
  edit,
  findBlock,
  focusBlock,
  focusInline,
  query,
  requireBlock,
  run,
  selectionText,
  showError,
  wordStats,
} from "./shared";
import { editorHooks, renderAll, renderMain, renderStatus } from "./shell";
import { MENUS } from "./menus";
import { applyMark, renderToolbar, setBlockAlignment, setBlockStyle, setLineSpacing } from "./toolbar";
import { closeFind, openFind, replaceFromFindBar, stepFind } from "./find";
import { promptPageFurniture } from "./pagination";
import { renderSidePanel } from "./panels";
import { loadVersions, runVersionAction } from "./versions";
import { runTableAction } from "./tables";
import { focusImageBlock, promptImageSize } from "./images";
import { runFileAction } from "./files";
import { runSpreadsheetAction } from "./spreadsheet";

/**
 * Every UI action funnels through here. The wrapper exists so that declining
 * the unsaved-work prompt stops the action quietly instead of surfacing as a
 * rejected promise at whichever listener started it.
 */
export async function runAction(action: string, data: DOMStringMap = {}): Promise<void> {
  try {
    await performAction(action, data);
  } catch (error) {
    if (error !== ACTION_CANCELLED) throw error;
  }
}

async function performAction(action: string, data: DOMStringMap = {}): Promise<void> {
  if (action.startsWith("toggle-panel:")) {
    const which = action.slice("toggle-panel:".length) as Panel;
    state.panel = state.panel === which ? null : which;
    renderSidePanel();
    if (state.panel === "versions") void loadVersions();
    return;
  }
  if (action.startsWith("version:")) {
    await runVersionAction(action.slice("version:".length), data);
    return;
  }
  if (action.startsWith("mark:")) {
    if (state.mode === "sheets") return;
    await applyMark(action.slice(5), null, "toggle");
    return;
  }
  if (action.startsWith("style:")) {
    const parts = action.split(":");
    if (parts[1] === "heading") await setBlockStyle("heading", Number(parts[2] ?? 1), "bullet");
    else if (parts[1] === "list") await setBlockStyle("list-item", focusBlock()?.level ?? 0, (parts[2] ?? "bullet") as ListKindName);
    else await setBlockStyle("paragraph", 0, "bullet");
    return;
  }
  if (action.startsWith("align:")) {
    if (state.mode === "sheets") return;
    await setBlockAlignment(action.slice("align:".length));
    return;
  }
  if (action.startsWith("image-placement:")) {
    if (state.mode === "sheets") return;
    const block = focusImageBlock();
    if (block) await edit("set_image_block_placement", { blockId: block.id, placement: action.slice("image-placement:".length) });
    return;
  }
  if (action === "image-reset-size") {
    const block = focusImageBlock();
    if (block) await edit("clear_image_block_size", { blockId: block.id });
    return;
  }
  if (action === "image-size") {
    await promptImageSize();
    return;
  }
  if (action.startsWith("line-spacing:")) {
    await setLineSpacing(action.slice("line-spacing:".length));
    return;
  }
  if (action.startsWith("page-furniture:")) {
    if (state.mode === "sheets") return;
    await promptPageFurniture(action.slice("page-furniture:".length));
    return;
  }
  if (action === "toggle-checklist-item") {
    // The renderer stamps the model's state on the wrapper, so the new value
    // is read from the document projection rather than from the checkbox the
    // user just clicked — the input is decoration, the document is the fact.
    const blockId = data?.checklistBlockId;
    if (!blockId) return;
    await edit("set_list_item_checked", { blockId, checked: data?.checked !== "true" });
    return;
  }

  // Surfaces that own their own state answer first; each returns false for a
  // name it does not own, so an unknown action still reaches the branch below
  // that says so.
  if (await runFileAction(action, data)) return;
  if (await runSpreadsheetAction(action, data)) return;
  switch (action) {
    case "go-home":
      state.view = "home";
      renderAll();
      break;
    case "mode-docs":
    case "mode-sheets": {
      state.mode = action === "mode-docs" ? "docs" : "sheets";
      const main = query("[data-main]");
      if (main) main.innerHTML = "";
      renderAll();
      break;
    }
    case "dismiss-error":
      state.lastError = null;
      renderStatus();
      break;
    case "undo":
      await invoke("undo_current_edit").then(applyDocument).catch(() => toast("Nothing to undo"));
      state.editor?.setSelection(state.selection);
      break;
    case "redo":
      await invoke("redo_current_edit").then(applyDocument).catch(() => toast("Nothing to redo"));
      state.editor?.setSelection(state.selection);
      break;
    case "select-all": {
      if (!state.doc || state.mode !== "docs") return;
      const result = await invoke("select_all_editor_content");
      editorHooks.onResult(result);
      break;
    }
    case "find":
      await openFind();
      break;
    case "find-close":
      closeFind();
      break;
    case "find-next":
    case "find-prev":
      stepFind(action === "find-next" ? 1 : -1);
      break;
    case "replace-one":
    case "replace-all":
      await replaceFromFindBar(action === "replace-all");
      break;
    case "comment": {
      const context = await describeEditorSelection();
      const range = context?.inline_range ?? null;
      const target = context?.focus_block_id ? findBlock(context.focus_block_id) : requireBlock();
      if (!target) return;
      const result = await promptDialog({ title: "Add comment", fields: [{ name: "body", label: "Comment", type: "textarea" }], submit: "Comment" });
      if (!result?.body.trim()) return;
      if (range) await edit("add_text_range_comment", { startInlineId: range.start, endInlineId: range.end, author: state.authorName, body: result.body });
      else await edit("add_block_comment", { blockId: target.id, author: state.authorName, body: result.body });
      state.panel = "comments";
      renderSidePanel();
      break;
    }
    case "reply-comment": {
      const result = await promptDialog({ title: "Reply", fields: [{ name: "body", label: "Reply", type: "textarea" }], submit: "Reply" });
      if (result?.body.trim() && data.id) await edit("add_comment_reply", { threadId: data.id, author: state.authorName, body: result.body });
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
      if (result?.text) await edit("add_text_range_suggestion", { startInlineId: range.start, endInlineId: range.end, author: state.authorName, text: result.text });
      state.panel = "suggestions";
      renderSidePanel();
      break;
    }
    case "suggest-delete": {
      const range = (await describeEditorSelection())?.inline_range ?? null;
      if (!range) {
        showError("Select the text to delete first.");
        return;
      }
      await edit("add_text_range_delete_suggestion", { startInlineId: range.start, endInlineId: range.end, author: state.authorName });
      state.panel = "suggestions";
      renderSidePanel();
      break;
    }
    case "accept-suggestion":
      if (data.id) await edit("accept_suggestion", { suggestionId: data.id, acceptedBy: state.authorName });
      break;
    case "reject-suggestion":
      if (data.id) await edit("reject_suggestion", { suggestionId: data.id, rejectedBy: state.authorName });
      break;
    case "accept-all":
      await edit("accept_all_suggestions", { acceptedBy: state.authorName });
      break;
    case "reject-all":
      await edit("reject_all_suggestions", { rejectedBy: state.authorName });
      break;
    case "insert-table": {
      const target = requireBlock();
      if (!target) return;
      const result = await promptDialog({ title: "Insert table", fields: [{ name: "rows", label: "Rows", type: "number", step: "1", value: "3" }, { name: "columns", label: "Columns", type: "number", step: "1", value: "3" }], submit: "Insert" });
      if (!result) return;
      await edit("insert_table_after", { afterBlockId: target.id, rows: Number(result.rows) || 2, columns: Number(result.columns) || 2 });
      break;
    }
    case "insert-page-break": {
      const target = requireBlock();
      if (target) await edit("insert_page_break_after", { afterBlockId: target.id });
      break;
    }
    case "table-insert-column-right":
    case "table-insert-column-left":
    case "table-delete-column":
    case "table-insert-row-below":
    case "table-insert-row-above":
    case "table-delete-row":
    case "table-column-width":
    case "table-column-width-auto":
    case "table-merge-cells":
    case "table-split-cell":
    case "table-cell-background":
    case "table-cell-border":
    case "table-cell-vertical-align":
    case "table-cell-padding":
    case "table-cell-clear-style":
      await runTableAction(action);
      break;
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
      else if (selectedText && state.selection) await applyMark("link", result.href, "set");
      else await edit("insert_link_after", { blockId: target.id, afterInlineId: state.selection?.focus.inline_id ?? null, text: result.text || result.href, href: result.href });
      break;
    }
    case "insert-footnote": {
      const target = requireBlock();
      if (target) await edit("insert_footnote_ref_after", { blockId: target.id, afterInlineId: state.selection?.focus.inline_id ?? null });
      state.panel = "footnotes";
      renderSidePanel();
      break;
    }
    case "edit-footnote": {
      const note = state.doc?.footnotes.find((item) => item.id === data.id);
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
      if (action === "insert-equation") await edit("insert_equation_after", { blockId: target.id, afterInlineId: state.selection?.focus.inline_id ?? null, source: result.source });
      else await edit("insert_equation_block_after", { afterBlockId: target.id, source: result.source });
      break;
    }
    case "insert-mention": {
      const target = requireBlock();
      if (!target) return;
      const result = await promptDialog({ title: "Mention", fields: [{ name: "label", label: "Name", value: "@" }], submit: "Insert" });
      if (result?.label) await edit("insert_mention_after", { blockId: target.id, afterInlineId: state.selection?.focus.inline_id ?? null, label: result.label });
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
        state.panel = "files";
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
      const doc = state.doc;
      if (!doc) return;
      let referenceId = data.id ?? "";
      let locator = "";
      if (!referenceId) {
        if (doc.citations.references.length === 0) {
          toast("Add a reference first (Citations panel).");
          state.panel = "citations";
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
          afterInlineId: state.selection?.focus.inline_id ?? null,
          items: [{ reference_id: referenceId, locator: locator || null, label: null, prefix: null, suffix: null, suppress_author: false }],
        });
      } else {
        await edit("insert_citation", { referenceId, afterInlineId: state.selection?.focus.inline_id ?? null, locator: locator || null, label: null, prefix: null, suffix: null, suppressAuthor: false });
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
            value: state.doc?.citations.style,
            options: ["apa", "mla", "chicago-author-date", "chicago-notes", "ieee", "vancouver", "harvard", "nature", "author-year", "numeric"].map((style) => ({ value: style, label: style })),
          },
          { name: "locale", label: "Locale", value: state.doc?.citations.locale ?? "en-US" },
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
      if (!state.selection) return;
      // Rust decides what indenting means for each selected block: a list
      // item moves a level, anything else shifts its start indent.
      await edit("adjust_editor_selection_indent", { selection: state.selection, delta: action === "indent" ? 1 : -1 });
      state.editor?.setSelection(state.selection);
      break;
    }
    case "zoom-in":
    case "zoom-out":
    case "zoom-reset":
      state.zoom = action === "zoom-reset" ? 1 : Math.min(3, Math.max(0.5, state.zoom + (action === "zoom-in" ? 0.1 : -0.1)));
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
      await promptDialog({ title: "About OpenDoc", fields: [{ name: "about", label: `OpenDoc – an open source, Rust-first document and spreadsheet editor. Runtime: ${state.profile?.label ?? runtime.mode}.`, value: "" }], submit: "Close" });
      break;
    case "share":
      toast("Sharing needs a collaboration server, which is not available in this runtime yet.");
      break;
    case "sign": {
      const result = await promptDialog({
        title: "Sign document",
        fields: [
          { name: "signer", label: "Your name", value: state.authorName },
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
    default:
      showError(`Unknown action ${action}`);
  }
}
