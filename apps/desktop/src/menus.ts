// The menu bar: one table of action names, rendered into `<details>` groups.
//
// Nothing here knows what an action does — every item is a `data-action` that
// the delegated dispatcher in `actions.ts` routes. `scope` greys out an item
// that belongs to the other mode rather than hiding it, so the bar keeps its
// shape when the user switches between document and spreadsheet.
import { morphChildren } from "./editor";
import { escapeHtml } from "./ui";
import { APP_LINE_SPACING_PRESETS } from "./generated/document";
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
      { action: "import-google-doc-url", label: "Import public Google Doc…" },
      { action: "import-json", label: "Import Google Docs JSON…" },
      { action: "save", label: "Save", shortcut: "Ctrl+S" },
      { action: "save-as", label: "Save as…", shortcut: "Ctrl+Shift+S" },
      { action: "export-json", label: "Download as Google Docs JSON" },
      { action: "export-docx", label: "Download as Word (.docx)", scope: "docs" },
      { action: "export-odt", label: "Download as OpenDocument (.odt)", scope: "docs" },
      { action: "export-pdf", label: "Download as PDF (.pdf)", scope: "docs" },
      { action: "export-html", label: "Download as HTML" },
      { action: "export-text", label: "Download as plain text" },
      { action: "page-setup", label: "Page setup…", scope: "docs" },
      { action: "print", label: "Print…", shortcut: "Ctrl+P" },
      { action: "rename", label: "Rename…" },
      { action: "details", label: "Details…" },
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
      { action: "toggle-panel:outline", label: "Document outline", scope: "docs" },
      { action: "toggle-panel:bookmarks", label: "Bookmarks", scope: "docs" },
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
      { action: "insert-image-url", label: "Image by URL…", scope: "docs" },
      { action: "insert-table", label: "Table…", scope: "docs" },
      { action: "insert-link", label: "Link…", shortcut: "Ctrl+K", scope: "docs" },
      { action: "insert-footnote", label: "Footnote", scope: "docs" },
      { action: "insert-endnote", label: "Endnote", scope: "docs" },
      { action: "insert-equation", label: "Equation…", scope: "docs" },
      { action: "insert-equation-block", label: "Equation block…", scope: "docs" },
      { action: "insert-page-break", label: "Page break", shortcut: "Ctrl+Enter", scope: "docs" },
      { action: "insert-horizontal-rule", label: "Horizontal rule", scope: "docs" },
      { action: "insert-table-of-contents", label: "Table of contents", scope: "docs" },
      { action: "insert-bibliography", label: "Bibliography", scope: "docs" },
      { action: "insert-bookmark", label: "Bookmark…", scope: "docs" },
      { action: "page-furniture:header", label: "Header…", scope: "docs" },
      { action: "page-furniture:footer", label: "Footer…", scope: "docs" },
      { action: "page-furniture:first-page-header", label: "First-page header…", scope: "docs" },
      { action: "page-furniture:first-page-footer", label: "First-page footer…", scope: "docs" },
      { action: "page-furniture:even-page-header", label: "Even-page header…", scope: "docs" },
      { action: "page-furniture:even-page-footer", label: "Even-page footer…", scope: "docs" },
      { action: "insert-citation", label: "Citation…", scope: "docs" },
      { action: "insert-mention", label: "Mention…", scope: "docs" },
      { action: "insert-date-chip", label: "Date…", scope: "docs" },
      { action: "comment", label: "Comment", scope: "docs" },
      { action: "add-sheet", label: "Sheet", scope: "sheets" },
      { action: "add-row", label: "Row below", scope: "sheets" },
      { action: "add-column", label: "Column right", scope: "sheets" },
      { action: "row-height", label: "Row height…", scope: "sheets" },
      { action: "column-width", label: "Column width…", scope: "sheets" },
      { action: "hide-rows", label: "Hide rows", scope: "sheets" },
      { action: "unhide-rows", label: "Unhide rows", scope: "sheets" },
      { action: "hide-columns", label: "Hide columns", scope: "sheets" },
      { action: "unhide-columns", label: "Unhide columns", scope: "sheets" },
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
      { action: "table-row-height", label: "Minimum row height…", scope: "docs" },
      { action: "table-row-height-auto", label: "Row height: auto", scope: "docs" },
      { action: "table-toggle-header", label: "Toggle header row", scope: "docs" },
      { action: "table-sort-ascending", label: "Sort column A–Z", scope: "docs" },
      { action: "table-sort-descending", label: "Sort column Z–A", scope: "docs" },
      { action: "table-border", label: "Table border…", scope: "docs" },
      { action: "table-border-inherit", label: "Table border: inherit", scope: "docs" },
      { action: "table-merge-cells", label: "Merge cells", scope: "docs" },
      { action: "table-split-cell", label: "Split cell", scope: "docs" },
      { action: "table-move-cell-block-up", label: "Move cell block up", scope: "docs" },
      { action: "table-move-cell-block-down", label: "Move cell block down", scope: "docs" },
      { action: "table-move-cell-block-previous-cell", label: "Move cell block to previous cell", scope: "docs" },
      { action: "table-move-cell-block-next-cell", label: "Move cell block to next cell", scope: "docs" },
      { action: "table-cell-background", label: "Cell background…", scope: "docs" },
      { action: "table-cell-border", label: "Cell border…", scope: "docs" },
      { action: "table-cell-vertical-align", label: "Cell vertical alignment…", scope: "docs" },
      { action: "table-cell-row-header", label: "Set row header…", scope: "docs" },
      { action: "table-cell-padding", label: "Cell padding…", scope: "docs" },
      { action: "table-cell-clear-style", label: "Clear cell style…", scope: "docs" },
      { action: "table-align-start", label: "Align table left", scope: "docs" },
      { action: "table-align-center", label: "Align table centre", scope: "docs" },
      { action: "table-align-end", label: "Align table right", scope: "docs" },
      { action: "table-align-inherit", label: "Table alignment: inherit", scope: "docs" },
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
      { action: "mark-remove:color", label: "Clear text colour", scope: "docs" },
      { action: "mark-remove:background", label: "Clear highlight", scope: "docs" },
      { action: "mark-remove:font", label: "Clear font", scope: "docs" },
      { action: "mark-remove:size", label: "Clear font size", scope: "docs" },
      { action: "clear-marks", label: "Clear formatting", shortcut: "Ctrl+\\", scope: "docs" },
      { action: "style:paragraph", label: "Normal text", shortcut: "Ctrl+Alt+0", scope: "docs" },
      { action: "style:title", label: "Title", scope: "docs" },
      { action: "style:subtitle", label: "Subtitle", scope: "docs" },
      { action: "style:heading:1", label: "Heading 1", shortcut: "Ctrl+Alt+1", scope: "docs" },
      { action: "style:heading:2", label: "Heading 2", shortcut: "Ctrl+Alt+2", scope: "docs" },
      { action: "style:heading:3", label: "Heading 3", shortcut: "Ctrl+Alt+3", scope: "docs" },
      { action: "style:heading:4", label: "Heading 4", scope: "docs" },
      { action: "style:heading:5", label: "Heading 5", scope: "docs" },
      { action: "style:heading:6", label: "Heading 6", scope: "docs" },
      { action: "style:list:bullet", label: "Bulleted list", shortcut: "Ctrl+Shift+8", scope: "docs" },
      { action: "style:list:ordered", label: "Numbered list", shortcut: "Ctrl+Shift+7", scope: "docs" },
      { action: "style:list:checklist", label: "Checklist", scope: "docs" },
      { action: "align:start", label: "Align left", shortcut: "Ctrl+Shift+L", scope: "docs" },
      { action: "align:center", label: "Centre", shortcut: "Ctrl+Shift+E", scope: "docs" },
      { action: "align:end", label: "Align right", shortcut: "Ctrl+Shift+R", scope: "docs" },
      { action: "align:justify", label: "Justify", shortcut: "Ctrl+Shift+J", scope: "docs" },
      { action: "direction:ltr", label: "Left-to-right paragraph", scope: "docs" },
      { action: "direction:rtl", label: "Right-to-left paragraph", scope: "docs" },
      // Built from the Rust-generated preset list: the menu names the same
      // spacings the toolbar offers, labelled the same way, because both read
      // the one list instead of each writing it out.
      ...APP_LINE_SPACING_PRESETS.map((preset, index) => ({
        action: `line-spacing:${index}`,
        label: `${preset.label} line spacing`,
        scope: "docs" as const,
      })),
      { action: "line-spacing:", label: "Default line spacing", scope: "docs" },
      { action: "paragraph-first-line-indent", label: "First-line indent…", scope: "docs" },
      { action: "indent", label: "Increase indent", shortcut: "Ctrl+]", scope: "docs" },
      { action: "outdent", label: "Decrease indent", shortcut: "Ctrl+[", scope: "docs" },
      { action: "image-size", label: "Image size…", scope: "docs" },
      { action: "image-reset-size", label: "Reset image size", scope: "docs" },
      { action: "image-properties", label: "Image properties…", scope: "docs" },
      { action: "replace-image", label: "Replace image…", scope: "docs" },
      { action: "save-image", label: "Save original image…", scope: "docs" },
      { action: "image-placement:block", label: "Image on its own line", scope: "docs" },
      { action: "image-placement:wrap-start", label: "Wrap text right of image", scope: "docs" },
      { action: "image-placement:wrap-end", label: "Wrap text left of image", scope: "docs" },
      { action: "image-position", label: "Position image…", scope: "docs" },
      { action: "image-clear-position", label: "Return image to text flow", scope: "docs" },
      { action: "citation-style", label: "Citation style…" },
    ],
  },
  {
    // Spreadsheet-only. Import/export live here rather than under File
    // because they act on the open workbook, not on the document file.
    label: "Data",
    items: [
      { action: "add-cell-note", label: "Add cell note…", scope: "sheets" },
      { action: "edit-cell-note", label: "Edit cell note…", scope: "sheets" },
      { action: "cell-validation", label: "Data validation…", scope: "sheets" },
      { action: "clear-cell-validation", label: "Remove data validation", scope: "sheets" },
      { action: "named-ranges", label: "Named ranges…", scope: "sheets" },
      { action: "protected-ranges", label: "Advisory protected ranges…", scope: "sheets" },
      { action: "set-print-area", label: "Set print area to selection", scope: "sheets" },
      { action: "clear-print-area", label: "Clear print area", scope: "sheets" },
      { action: "print-orientation", label: "Print orientation…", scope: "sheets" },
      { action: "sort-range", label: "Sort range…", scope: "sheets" },
      { action: "fill-down", label: "Fill down", shortcut: "Ctrl+D", scope: "sheets" },
      { action: "fill-right", label: "Fill right", shortcut: "Ctrl+R", scope: "sheets" },
      { action: "import-csv", label: "Import CSV/TSV…", scope: "sheets" },
      { action: "import-xlsx", label: "Import Excel (.xlsx)…", scope: "sheets" },
      { action: "import-google-sheet-url", label: "Import public Google Sheet…", scope: "sheets" },
      { action: "export-sheets-json", label: "Download as Google Sheets JSON", scope: "sheets" },
      { action: "export-csv", label: "Download this sheet as CSV", scope: "sheets" },
      { action: "export-xlsx", label: "Download as Excel (.xlsx)", scope: "sheets" },
      { action: "export-spreadsheet-pdf", label: "Download as PDF (.pdf)", scope: "sheets" },
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
