// Import, export and the File menu.
//
// What a file *is* — its media type, its extension, whether it is text or
// base64, and what the format could not carry — all comes back from Rust in the
// command result. Nothing here keeps a table that could disagree with the
// exporter about what it just wrote.
import { promptDialog, toast } from "./ui";
import { fetchUrlFile, isTauri, openFile, pickOpenPath, saveFile } from "./invoke";
import type { AppExport } from "./generated/export";
import type { AppWarning } from "./types";
import { state } from "./state";
import { edit, native, run, textFromBase64 } from "./shared";
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
export async function downloadExport(
  command: "export_google_docs_json" | "export_docx" | "export_odt" | "export_pdf" | "export_html" | "export_text" | "export_image_blob" | "export_google_sheets_json" | "export_spreadsheet_csv" | "export_spreadsheet_xlsx" | "export_spreadsheet_pdf",
  label: string,
  options?: { args?: Record<string, unknown>; defaultBaseName?: string },
): Promise<void> {
  const result = (await run(command, options?.args as never)) as AppExport | undefined;
  if (!result) return;
  // The write is the shell's, not Rust's: a refused `write_file_base64` used
  // to reject out of here into `void runAction(...)`, so the export produced
  // neither the "Exported as …" toast nor any message at all.
  const saved = await native(`Could not write the ${label} file`, () =>
    saveFile({
      defaultName: `${options?.defaultBaseName ?? state.doc?.title ?? "document"}.${result.file_extension}`,
      extensions: [result.file_extension],
      mediaType: result.media_type,
      ...(result.encoding === "base64" ? { base64: result.content } : { text: result.content }),
    }),
  );
  // Not a failure: the user closed the save dialog, so there is nothing to
  // report about an export that was never written.
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

/**
 * File ▸ Details: what the document is, as the app already knows it.
 *
 * Every number here is read from a projection Rust computed — the word and
 * character counts come with `AppDocument`, the page count and whether the
 * layout was exact come from `layout_document`, the page size name and
 * orientation are derived in `opendoc-core` and projected in `page_layout`.
 * Nothing is counted or measured in this file.
 *
 * The dialog field is read-only: these are facts about the document, and the
 * only thing a person does with them is select them and copy them. It used to
 * be a plain `textarea`, which let the text be edited and then discarded the
 * edit in silence.
 */
async function showDocumentDetails(): Promise<void> {
  const doc = state.doc;
  if (!doc) return;
  const layout = await run("layout_document");
  const page = doc.page_layout;
  const size = page?.size_name ? page.size_name.toUpperCase() : "Custom";
  const lines = [
    `Title: ${doc.title || "(untitled)"}`,
    `Identifier: ${doc.uuid}`,
    doc.doi ? `DOI: ${doc.doi}` : null,
    `Locale: ${doc.locale || "(unset)"}`,
    "",
    `Words: ${doc.word_count}`,
    `Characters: ${doc.character_count}`,
    `Blocks: ${doc.blocks.length}`,
    `Pages: ${layout.page_count}${layout.exact ? "" : " (some heights are estimated)"}`,
    "",
    `Page size: ${size}, ${page?.orientation ?? "portrait"}`,
    `Footnotes: ${doc.footnotes.length}`,
    `Comments: ${doc.comments.length}`,
    `Suggestions: ${doc.suggestions.length}`,
    `Attachments: ${doc.blobs.length}`,
    `Versions recorded: ${doc.operation_count}`,
    "",
    `Saved in: ${doc.repository_root ?? "(not saved yet)"}`,
    `Unsaved changes: ${doc.has_unsaved_changes ? "yes" : "no"}`,
    `Signature: ${doc.signature_state}`,
  ].filter((line): line is string => line !== null);
  await promptDialog({
    title: "Document details",
    fields: [{ name: "details", label: "Select and copy if you need these", type: "textarea", readonly: true, value: lines.join("\n") }],
    submit: "Close",
  });
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
        ? await native("Could not open the folder chooser", () => pickOpenPath({ title: "Open OpenDoc folder", directory: true }))
        : ((await promptDialog({ title: "Open repository", fields: [{ name: "path", label: "Repository path", value: state.doc?.repository_root ?? "" }] }))?.path ?? null);
      // Nothing chosen: the dialog was closed, or the prompt was cancelled.
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
        const path = await native("Could not open the file chooser", () => pickOpenPath({ title: "Import Word document", extensions: ["docx", "doc"] }));
        if (!path) return true;
        await run("import_doc_or_docx_path", { path });
      } else {
        // The browser reads through a file input, which resolves rather than
        // rejects — but `openFile` is one function over both runtimes, so it
        // is wrapped here too: the two branches cannot drift, and a later
        // change inside `openFile` cannot make this silent again.
        const file = await native("Could not read that Word document", () => openFile(["docx", "doc"]));
        if (!file) return true;
        await run("import_docx_base64", { name: file.name, base64: file.base64 });
      }
      enterEditor("docs");
      break;
    }
    case "import-google-doc-url": {
      const answer = await promptDialog({
        title: "Import public Google Doc",
        fields: [{ name: "url", label: "Google Docs sharing URL", value: "" }],
      });
      if (!answer) return true;
      const exportUrl = googlePublicExportUrl(answer.url, "document", "docx");
      if (!exportUrl) {
        toast("Enter a public docs.google.com/document/d/<id> sharing URL.");
        return true;
      }
      const file = isTauri()
        ? await native("Could not download that public Google Doc", () => fetchUrlFile(exportUrl))
        : await fetchPublicGoogleExport(exportUrl);
      if (!file) return true;
      await run("import_docx_base64", { name: "Google Doc.docx", base64: file.base64 });
      enterEditor("docs");
      break;
    }
    case "import-json": {
      const file = await native("Could not read that file", () => openFile(["json"]));
      // No file chosen — the dialog was closed, which is not a failure.
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
          ? await native("Could not open the folder chooser", () => pickOpenPath({ title: "Choose a folder to save the document in", directory: true }))
          : ((await promptDialog({ title: "Save", fields: [{ name: "path", label: "Repository path", value: doc.repository_root ?? "opendoc-repo" }] }))?.path ?? null);
      }
      // No folder chosen, so nothing to save into and nothing to report.
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
    case "export-odt":
      await downloadExport("export_odt", "OpenDocument (.odt)");
      break;
    // HTML and plain text used to be assembled here — a hand-written
    // stylesheet, the body taken from the projection, no warnings and a media
    // type this file decided. They are Rust commands now, like every other
    // export, so they carry what the format could not hold (ADR 0010) and the
    // stylesheet is projected from the type scale rather than restated.
    case "export-pdf":
      await downloadExport("export_pdf", "PDF");
      break;
    case "export-html":
      await downloadExport("export_html", "HTML");
      break;
    case "export-text":
      await downloadExport("export_text", "Plain text");
      break;
    case "print":
      window.print();
      break;
    case "page-setup": {
      await promptPageSetup();
      break;
    }
    case "details": {
      await showDocumentDetails();
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

/** Turn a public Google editor/share URL into its credential-free export. */
export function googlePublicExportUrl(
  value: string,
  product: "document" | "spreadsheets",
  format: "docx" | "xlsx",
): string | null {
  let url: URL;
  try {
    url = new URL(value.trim());
  } catch {
    return null;
  }
  if (url.protocol !== "https:" || url.hostname !== "docs.google.com") return null;
  // A copied public editor link can include Google's account-context prefix
  // (`document/u/0/d/...`) when the person has more than one Google account
  // signed in. It does not grant or require that account: the generated
  // export URL remains the same credential-free public endpoint. Accept only
  // the bounded numeric context form, not arbitrary extra path components.
  const match = new RegExp(`^/${product}/(?:u/\\d+/)?d/([^/]+)(?:/|$)`).exec(url.pathname);
  if (!match || !/^[A-Za-z0-9_-]+$/.test(match[1])) return null;
  return `https://docs.google.com/${product}/d/${match[1]}/export?format=${format}`;
}

/** Download only a generated Google public export in the web shell. */
export async function fetchPublicGoogleExport(url: string): Promise<{ base64: string } | null> {
  const response = await fetch(url, { redirect: "follow" });
  if (!response.ok) throw new Error(`Google Docs returned HTTP ${response.status}`);
  const declared = Number(response.headers.get("content-length") ?? 0);
  const maxBytes = 16 * 1024 * 1024;
  if (declared > maxBytes) throw new Error("Google Doc export exceeds the 16 MiB import limit");
  const bytes = new Uint8Array(await response.arrayBuffer());
  if (bytes.byteLength > maxBytes) throw new Error("Google Doc export exceeds the 16 MiB import limit");
  let binary = "";
  for (let offset = 0; offset < bytes.length; offset += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + 0x8000));
  }
  return { base64: btoa(binary) };
}
