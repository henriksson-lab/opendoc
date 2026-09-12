// Import, export and the File menu.
//
// What a file *is* — its media type, its extension, whether it is text or
// base64, and what the format could not carry — all comes back from Rust in the
// command result. Nothing here keeps a table that could disagree with the
// exporter about what it just wrote.
import { escapeHtml, promptDialog, toast } from "./ui";
import { isTauri, openFile, pickOpenPath, saveFile } from "./invoke";
import type { AppExport } from "./generated/export";
import type { AppWarning } from "./types";
import { state } from "./state";
import { edit, run, textFromBase64 } from "./shared";
import { enterEditor, renderAll } from "./shell";
import { renderSidePanel } from "./panels";
import { promptPageSetup } from "./pagination";

/**
 * What the last export could not carry, and which export it was.
 *
 * View state, deliberately: these warnings belong to one command's result, not
 * to the document. Putting them on the document would mean writing into state
 * that gets hashed and signed, so exporting would dirty a document nobody
 * edited. See docs/adr/0010-export-results-carry-their-own-warnings.md.
 */
let exportReport: { label: string; warnings: AppWarning[] } | null = null;

/** What the warnings panel shows above the document's own warnings. */
export function exportWarningReport(): { label: string; warnings: AppWarning[] } | null {
  return exportReport;
}

/**
 * Runs one export command and writes what it produced to a file.
 *
 * The media type, the file extension and the encoding all come back from Rust
 * in the command result, so there is no table here that can disagree with the
 * exporter about what it just wrote.
 */
export async function downloadExport(command: "export_google_docs_json" | "export_docx" | "export_google_sheets_json", label: string): Promise<void> {
  const result = (await run(command)) as AppExport | undefined;
  if (!result) return;
  const saved = await saveFile({
    defaultName: `${state.doc?.title ?? "document"}.${result.file_extension}`,
    extensions: [result.file_extension],
    mediaType: result.media_type,
    ...(result.encoding === "base64" ? { base64: result.content } : { text: result.content }),
  });
  if (saved === null) return;
  reportExportWarnings(label, result.warnings);
}

/** Shows what an export could not carry, without touching the document. */
function reportExportWarnings(label: string, warnings: AppWarning[]): void {
  exportReport = { label, warnings };
  if (warnings.length === 0) {
    toast(`Exported as ${label}`);
    renderSidePanel();
    return;
  }
  // The warnings panel rather than a modal: there can be two dozen of them,
  // and they are worth reading, not worth blocking on.
  state.panel = "warnings";
  toast(`Exported as ${label} — ${warnings.length} thing${warnings.length === 1 ? "" : "s"} the format could not carry`);
  renderSidePanel();
}

/** The stylesheet the standalone HTML export carries with it. */
function exportStyles(): string {
  return `.doc-body{font-family:Arial,sans-serif;max-width:52em;margin:2em auto;line-height:1.5}.mark-bold{font-weight:700}.mark-italic{font-style:italic}.mark-underline{text-decoration:underline}.mark-strike{text-decoration:line-through}.mark-code{font-family:monospace}.mark-superscript{vertical-align:super;font-size:.8em}.mark-subscript{vertical-align:sub;font-size:.8em}table{border-collapse:collapse}td{border:1px solid #999;padding:4px 8px}`;
}

/**
 * The File menu, plus the two recovery actions the crash-recovery dialog
 * dispatches. Returns false for a name this surface does not own.
 */
export async function runFileAction(action: string, data: DOMStringMap): Promise<boolean> {
  switch (action) {
    case "new-document":
    case "new-spreadsheet": {
      // No guard here on purpose: `create_document` is classified as a
      // document-replacing command, so `run` has already asked.
      await run("create_document", { title: "Untitled document" });
      enterEditor(action === "new-spreadsheet" ? "sheets" : "docs");
      break;
    }
    case "open-repository": {
      const path = isTauri()
        ? await pickOpenPath({ title: "Open OpenDoc folder", directory: true })
        : ((await promptDialog({ title: "Open repository", fields: [{ name: "path", label: "Repository path", value: state.doc?.repository_root ?? "" }] }))?.path ?? null);
      if (!path) return true;
      const scanned = await run("scan_local_repository", { path });
      const documents = scanned.recent_documents.filter((recent) => recent.repository_root === path);
      if (documents.length === 0) {
        toast("No OpenDoc documents found in that folder.");
        return true;
      }
      const choice =
        documents.length === 1
          ? documents[0].uuid
          : (await promptDialog({ title: "Open document", fields: [{ name: "uuid", label: "Document", type: "select", options: documents.map((item) => ({ value: item.uuid, label: item.title || item.uuid })) }] }))?.uuid;
      if (!choice) return true;
      await run("open_local_repository", { path, documentUuid: choice });
      enterEditor("docs");
      break;
    }
    case "open-recent": {
      const root = data.root ?? "";
      const uuid = data.uuid ?? "";
      if (!root || !uuid) return true;
      if (data.backend === "flat") await run("open_flat_repository", { path: root, namespace: data.namespace ?? "", documentUuid: uuid });
      else await run("open_local_repository", { path: root, documentUuid: uuid });
      enterEditor("docs");
      break;
    }
    case "import-word": {
      if (isTauri()) {
        const path = await pickOpenPath({ title: "Import Word document", extensions: ["docx", "doc"] });
        if (!path) return true;
        await run("import_doc_or_docx_path", { path });
      } else {
        const file = await openFile(["docx", "doc"]);
        if (!file) return true;
        await run("import_docx_base64", { name: file.name, base64: file.base64 });
      }
      enterEditor("docs");
      break;
    }
    case "import-json": {
      const file = await openFile(["json"]);
      if (!file) return true;
      await run("import_google_docs_json", { title: file.name.replace(/\.json$/i, ""), jsonText: textFromBase64(file.base64) });
      enterEditor("docs");
      break;
    }
    case "save":
    case "save-as": {
      const doc = state.doc;
      if (!doc) return true;
      let path = action === "save" ? doc.repository_root : null;
      if (!path) {
        path = isTauri()
          ? await pickOpenPath({ title: "Choose a folder to save the document in", directory: true })
          : ((await promptDialog({ title: "Save", fields: [{ name: "path", label: "Repository path", value: doc.repository_root ?? "opendoc-repo" }] }))?.path ?? null);
      }
      if (!path) return true;
      await run("save_local_repository", { path });
      toast("Saved");
      break;
    }
    case "export-json":
      await downloadExport("export_google_docs_json", "Google Docs JSON");
      break;
    case "export-docx":
      await downloadExport("export_docx", "Word (.docx)");
      break;
    case "export-html": {
      const html = `<!doctype html><meta charset="utf-8"><title>${escapeHtml(state.doc?.title ?? "")}</title><style>${exportStyles()}</style><article class="doc-body">${state.doc?.body_html ?? ""}</article>${state.doc?.footnotes_html ?? ""}`;
      await saveFile({ defaultName: `${state.doc?.title ?? "document"}.html`, extensions: ["html"], text: html, mediaType: "text/html" });
      break;
    }
    case "export-text":
      await saveFile({ defaultName: `${state.doc?.title ?? "document"}.txt`, extensions: ["txt"], text: state.doc?.visible_text ?? "", mediaType: "text/plain" });
      break;
    case "print":
      window.print();
      break;
    case "page-setup": {
      await promptPageSetup();
      break;
    }
    case "rename": {
      const result = await promptDialog({ title: "Rename document", fields: [{ name: "title", label: "Title", value: state.doc?.title ?? "" }] });
      if (result) await edit("set_document_title", { title: result.title });
      break;
    }
    case "recover-session": {
      const sessionId = data.sessionId ?? "";
      if (!sessionId) return true;
      await run("recover_session", { sessionId });
      enterEditor("docs");
      break;
    }
    case "discard-recovery-session": {
      const sessionId = data.sessionId ?? "";
      if (!sessionId) return true;
      await run("discard_recovery_session", { sessionId });
      break;
    }
    case "close-document":
      await run("close_document");
      state.view = "home";
      renderAll();
      break;
    default:
      return false;
  }
  return true;
}
