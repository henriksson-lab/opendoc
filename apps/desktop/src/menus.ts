// The menu bar: one table of action names, rendered into `<details>` groups.
//
// Nothing here knows what an action does — every item is a `data-action` that
// the delegated dispatcher in `actions.ts` routes. `scope` greys out an item
// that belongs to the other mode rather than hiding it, so the bar keeps its
// shape when the user switches between document and spreadsheet.
import { morphChildren } from "./editor";
import { escapeHtml } from "./ui";
import type { Mode } from "./state";
import { state } from "./state";
import { query } from "./shared";

export const MENUS: { label: string; items: { action: string; label: string; shortcut?: string; scope?: Mode }[] }[] = [
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
      { action: "export-docx", label: "Download as Word (.docx)", scope: "docs" },
      { action: "export-html", label: "Download as HTML" },
      { action: "export-text", label: "Download as plain text" },
      { action: "page-setup", label: "Page setup…", scope: "docs" },
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
      { action: "toggle-panel:versions", label: "Version history" },
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
      { action: "page-furniture:header", label: "Header…", scope: "docs" },
      { action: "page-furniture:footer", label: "Footer…", scope: "docs" },
      { action: "insert-citation", label: "Citation…", scope: "docs" },
      { action: "insert-mention", label: "Mention…", scope: "docs" },
      { action: "comment", label: "Comment", scope: "docs" },
      { action: "add-sheet", label: "Sheet", scope: "sheets" },
      { action: "add-row", label: "Row below", scope: "sheets" },
      { action: "add-column", label: "Column right", scope: "sheets" },
      { action: "row-height", label: "Row height…", scope: "sheets" },
      { action: "column-width", label: "Column width…", scope: "sheets" },
    ],
  },
  {
    label: "Table",
    items: [
      { action: "table-insert-column-right", label: "Insert column right", scope: "docs" },
      { action: "table-insert-column-left", label: "Insert column left", scope: "docs" },
      { action: "table-delete-column", label: "Delete column", scope: "docs" },
      { action: "table-insert-row-below", label: "Insert row below", scope: "docs" },
      { action: "table-insert-row-above", label: "Insert row above", scope: "docs" },
      { action: "table-delete-row", label: "Delete row", scope: "docs" },
      { action: "table-column-width", label: "Column width…", scope: "docs" },
      { action: "table-column-width-auto", label: "Column width: auto", scope: "docs" },
      { action: "table-merge-cells", label: "Merge cells…", scope: "docs" },
      { action: "table-split-cell", label: "Split cell", scope: "docs" },
      { action: "table-cell-background", label: "Cell background…", scope: "docs" },
      { action: "table-cell-border", label: "Cell border…", scope: "docs" },
      { action: "table-cell-vertical-align", label: "Cell vertical alignment…", scope: "docs" },
      { action: "table-cell-padding", label: "Cell padding…", scope: "docs" },
      { action: "table-cell-clear-style", label: "Clear cell style…", scope: "docs" },
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
      { action: "style:list:bullet", label: "Bulleted list", shortcut: "Ctrl+Shift+8", scope: "docs" },
      { action: "style:list:ordered", label: "Numbered list", shortcut: "Ctrl+Shift+7", scope: "docs" },
      { action: "style:list:checklist", label: "Checklist", scope: "docs" },
      { action: "align:start", label: "Align left", shortcut: "Ctrl+Shift+L", scope: "docs" },
      { action: "align:center", label: "Centre", shortcut: "Ctrl+Shift+E", scope: "docs" },
      { action: "align:end", label: "Align right", shortcut: "Ctrl+Shift+R", scope: "docs" },
      { action: "align:justify", label: "Justify", shortcut: "Ctrl+Shift+J", scope: "docs" },
      { action: "line-spacing:multiple:1000", label: "Single line spacing", scope: "docs" },
      { action: "line-spacing:multiple:1500", label: "1.5 line spacing", scope: "docs" },
      { action: "line-spacing:multiple:2000", label: "Double line spacing", scope: "docs" },
      { action: "line-spacing:", label: "Default line spacing", scope: "docs" },
      { action: "indent", label: "Increase indent", shortcut: "Ctrl+]", scope: "docs" },
      { action: "outdent", label: "Decrease indent", shortcut: "Ctrl+[", scope: "docs" },
      { action: "image-size", label: "Image size…", scope: "docs" },
      { action: "image-reset-size", label: "Reset image size", scope: "docs" },
      { action: "image-placement:block", label: "Image on its own line", scope: "docs" },
      { action: "image-placement:wrap-start", label: "Wrap text right of image", scope: "docs" },
      { action: "image-placement:wrap-end", label: "Wrap text left of image", scope: "docs" },
      { action: "citation-style", label: "Citation style…" },
    ],
  },
  {
    // Spreadsheet-only. Import/export live here rather than under File
    // because they act on the open workbook, not on the document file.
    label: "Data",
    items: [
      { action: "sort-range", label: "Sort range…", scope: "sheets" },
      { action: "fill-down", label: "Fill down", shortcut: "Ctrl+D", scope: "sheets" },
      { action: "fill-right", label: "Fill right", shortcut: "Ctrl+R", scope: "sheets" },
      { action: "import-csv", label: "Import CSV/TSV…", scope: "sheets" },
      { action: "import-xlsx", label: "Import Excel (.xlsx)…", scope: "sheets" },
      { action: "export-sheets-json", label: "Download as Google Sheets JSON", scope: "sheets" },
      { action: "export-csv", label: "Download this sheet as CSV", scope: "sheets" },
      { action: "export-xlsx", label: "Download as Excel (.xlsx)", scope: "sheets" },
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

export function renderMenus(): void {
  const bar = query("[data-menu-bar]");
  if (!bar) return;
  const html = MENUS.map(
    (menu) => `
      <details class="menu-group" data-menu="${escapeHtml(menu.label)}">
        <summary role="menuitem" aria-haspopup="menu">${escapeHtml(menu.label)}</summary>
        <div class="menu-items" role="menu">
          ${menu.items
            .map((item) => {
              const disabled = item.scope && item.scope !== state.mode ? " disabled" : "";
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
