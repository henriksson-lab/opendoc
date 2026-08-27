import { invoke } from "./invoke";
import type { CommandArgs, DocumentCommandName } from "./commands";
import type {
  AppBlock,
  AppCell,
  AppCitationDatabase,
  AppCommentThread,
  AppDocument,
  AppInline,
  AppOperationRecord,
  AppSheet,
  AppSpreadsheetWorkbook,
  AppSuggestion,
  AppWarning,
} from "./types";
import "./styles.css";

const app = document.querySelector<HTMLDivElement>("#app");

if (!app) {
  throw new Error("missing app root");
}

let documentState: AppDocument | null = null;
let selectedInlineId: string | null = null;
let lastError: string | null = null;
let signerDisplayValue = "Local User";
let privateKeyPemValue = "";

async function command<K extends DocumentCommandName>(
  name: K,
  args: CommandArgs<K> = {} as CommandArgs<K>,
) {
  try {
    documentState = await invoke(name, args);
    lastError = null;
    render();
  } catch (error) {
    lastError = errorMessage(error);
    render();
  }
}

async function load() {
  try {
    documentState = await invoke("get_document");
    lastError = null;
  } catch (error) {
    lastError = errorMessage(error);
  }
  render();
}

function render() {
  if (!documentState) {
    app.innerHTML = `<main class="shell"><p>Loading...</p></main>`;
    return;
  }

  app.innerHTML = `
    <main class="shell">
      <header class="topbar">
        <div>
          <div class="product">OpenDoc</div>
          <h1>${escapeHtml(documentState.title)}</h1>
        </div>
        <div class="status">
          <span>${escapeHtml(documentState.locale)}</span>
          <span>${escapeHtml(documentState.signature_state)}</span>
        </div>
      </header>
      ${renderError()}
      <section class="repo-bar" aria-label="Repository">
        <label>
          Repository
          <input id="repo-path" value="${escapeHtml(documentState.repository_root ?? defaultRepoPath())}" />
        </label>
        <label>
          Document UUID
          <input id="document-uuid" value="${escapeHtml(documentState.uuid)}" />
        </label>
        ${button("create-document", "New", "Create a new blank document")}
        ${button("save-local", "Save", "Save to local object repository")}
        ${button("open-local", "Open", "Open from local object repository")}
        <div class="manifest">${escapeHtml(documentState.last_manifest ?? "not saved")}</div>
      </section>
      <section class="sign-bar" aria-label="Signing">
        <label>
          Signer
          <input id="signer-display" value="${escapeHtml(signerDisplayValue)}" />
        </label>
        <label>
          OpenSSH private key
          <textarea id="private-key-pem" rows="2" spellcheck="false">${escapeHtml(privateKeyPemValue)}</textarea>
        </label>
        ${button("sign-current", "Sign", "Sign current snapshot")}
        ${button("verify-current", "Verify", "Verify current signature")}
        <div id="verify-result" class="manifest">${escapeHtml(signatureLabel(documentState))}</div>
      </section>
      <section class="workspace">
        <aside class="toolbar" aria-label="Document tools">
          ${button("add-heading", "H2", "Add heading")}
          ${button("add-paragraph", "P", "Add paragraph")}
          ${button("add-link", "Link", "Add link")}
          ${button("add-mention", "@", "Add mention")}
          ${button("add-footnote-ref", "Fn", "Add footnote reference")}
          ${button("add-equation", "Eq", "Add equation")}
          ${button("add-equation-block", "EqBlk", "Add block equation")}
          ${button("add-list-item", "List", "Add list item")}
          ${button("add-page-break", "Break", "Add page break")}
          ${button("add-table", "Table", "Add table")}
          ${button("add-citation", "Cite", "Add citation")}
          ${button("add-comment", "Comment", "Add comment")}
          ${button("add-suggestion", "Suggest", "Add suggestion")}
          ${button("mark-bold", "B", "Bold selected inline")}
          ${button("mark-italic", "I", "Italic selected inline")}
          ${button("mark-underline", "U", "Underline selected inline")}
          ${button("mark-strike", "S", "Strike selected inline")}
          ${button("mark-code", "Code", "Code selected inline")}
        </aside>
        <div class="main-surface">
          <article class="document" aria-label="Document">
            ${documentState.blocks.map(renderBlock).join("")}
          </article>
          ${renderWorkbook(documentState.workbook)}
        </div>
        <aside class="inspector" aria-label="Document inspector">
          ${renderCitations(documentState.citations)}
          ${renderOperations(documentState.operations)}
          ${renderComments(documentState.comments)}
          ${renderSuggestions(documentState.suggestions)}
          ${renderWarnings(documentState.warnings)}
        </aside>
      </section>
    </main>
  `;

  bindCommands();
}

function button(action: string, label: string, title: string) {
  return `<button type="button" data-action="${action}" title="${title}">${label}</button>`;
}

function bindCommands() {
  bind("create-document", () => {
    const title = window.prompt?.("Document title", "Untitled OpenDoc") ?? "Untitled OpenDoc";
    void command("create_document", { title });
  });
  bind("add-heading", () => command("add_heading", { text: "New section", level: 2 }));
  bind("add-paragraph", () => command("add_paragraph", { text: "New paragraph" }));
  bind("add-link", () =>
    command("add_link", { text: "Reference link", href: "https://example.invalid" }),
  );
  bind("add-mention", () => command("add_mention", { label: "@local-user" }));
  bind("add-footnote-ref", () => command("add_footnote_ref"));
  bind("add-equation", () => command("add_equation", { source: "\\int_0^1 x^2 dx" }));
  bind("add-equation-block", () =>
    command("add_equation_block", { source: "\\sum_{i=1}^{n} i = n(n+1)/2" }),
  );
  bind("add-list-item", () =>
    command("add_list_item", { text: "New list item", level: 0, ordered: false }),
  );
  bind("add-page-break", () => command("add_page_break"));
  bind("add-table", () => command("add_table"));
  bind("add-citation", () => command("add_citation"));
  bind("add-comment", () =>
    command("add_comment", { author: "Local User", body: "New comment thread" }),
  );
  bind("add-suggestion", () =>
    command("add_suggestion", { author: "Local User", text: "Suggested text" }),
  );
  bindMark("mark-bold", "bold");
  bindMark("mark-italic", "italic");
  bindMark("mark-underline", "underline");
  bindMark("mark-strike", "strike");
  bindMark("mark-code", "code");
  bind("save-local", () => {
    const path = inputValue("repo-path");
    void command("save_local_repository", { path });
  });
  bind("open-local", () => {
    const path = inputValue("repo-path");
    const documentUuid = inputValue("document-uuid");
    void command("open_local_repository", { path, documentUuid });
  });
  bind("sign-current", () => {
    const privateKeyPem = syncPrivateKeyValue();
    const signerDisplay = syncSignerDisplayValue();
    void command("sign_with_openssh_private_key", { privateKeyPem, signerDisplay });
  });
  bind("verify-current", () => {
    const privateKeyPem = syncPrivateKeyValue();
    void invoke("verify_current_signature", { privateKeyPem })
      .then((state) => {
        lastError = null;
        const target = document.querySelector<HTMLDivElement>("#verify-result");
        if (target) target.textContent = state;
      })
      .catch((error) => {
        lastError = errorMessage(error);
        render();
      });
  });
  bindPerItemCommand("delete-comment-thread", "threadId", "delete_comment_thread");
  bindPerItemCommand("accept-suggestion", "suggestionId", "accept_suggestion", {
    acceptedBy: "Local User",
  });
  bindPerItemCommand("reject-suggestion", "suggestionId", "reject_suggestion", {
    rejectedBy: "Local User",
  });
  bindEditableInlines();
  bindEditableEquationBlocks();
  bindEditableSpreadsheetCells();
  bindEditableCitationReferences();
}

function bind(action: string, handler: () => void) {
  document.querySelector(`[data-action="${action}"]`)?.addEventListener("click", () => {
    void handler();
  });
}

function bindMark(action: string, markKind: string) {
  bind(action, () => {
    if (!selectedInlineId) return;
    void command("add_text_mark", { inlineId: selectedInlineId, markKind });
  });
}

function bindPerItemCommand(
  action: string,
  idKey: string,
  commandName: "delete_comment_thread" | "accept_suggestion" | "reject_suggestion",
  extraArgs: Record<string, unknown> = {},
) {
  document.querySelectorAll<HTMLButtonElement>(`[data-action="${action}"]`).forEach((button) => {
    button.addEventListener("click", () => {
      const targetId = button.dataset.targetId;
      if (!targetId) return;
      void command(commandName, { [idKey]: targetId, ...extraArgs } as CommandArgs<typeof commandName>);
    });
  });
}

function inputValue(id: string): string {
  return document.querySelector<HTMLInputElement>(`#${id}`)?.value.trim() ?? "";
}

function textAreaValue(id: string): string {
  return document.querySelector<HTMLTextAreaElement>(`#${id}`)?.value ?? "";
}

function syncSignerDisplayValue(): string {
  signerDisplayValue = inputValue("signer-display") || "Local User";
  return signerDisplayValue;
}

function syncPrivateKeyValue(): string {
  privateKeyPemValue = textAreaValue("private-key-pem");
  return privateKeyPemValue;
}

function renderError(): string {
  if (!lastError) {
    return "";
  }
  return `<section class="error-banner" role="alert">${escapeHtml(lastError)}</section>`;
}

function errorMessage(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }
  return String(error);
}

function defaultRepoPath(): string {
  return "./opendoc-repo";
}

function renderWorkbook(workbook: AppSpreadsheetWorkbook): string {
  const sheet = workbook.sheets[0];
  if (!sheet) {
    return "";
  }
  return `
    <section class="spreadsheet" aria-label="Spreadsheet">
      <header>
        <h2>${escapeHtml(workbook.title)}</h2>
        <span>${escapeHtml(sheet.title)} · ${escapeHtml(workbook.locale)}</span>
      </header>
      <table class="sheet-grid">
        <thead>
          <tr><th></th>${sheet.columns.map((column) => `<th>${escapeHtml(column)}</th>`).join("")}</tr>
        </thead>
        <tbody>
          ${sheet.rows
            .map(
              (row) => `
                <tr>
                  <th>${escapeHtml(row)}</th>
                  ${sheet.columns.map((column) => renderSheetCell(sheet, `${column}${row}`)).join("")}
                </tr>
              `,
            )
            .join("")}
        </tbody>
      </table>
    </section>
  `;
}

function renderSheetCell(sheet: AppSheet, address: string): string {
  const cell = sheet.cells.find((item) => item.address === address);
  const userValue = cell?.user_value ?? "";
  const computed = cell?.computed_value ?? "";
  const kind = cell?.computed_kind ?? "empty";
  return `
    <td>
      <div class="cell-editor" contenteditable="true" spellcheck="false"
        data-edit-cell-address="${escapeHtml(address)}"
        data-original-value="${escapeHtml(userValue)}">${escapeHtml(userValue)}</div>
      <small class="cell-computed ${escapeHtml(kind)}">${escapeHtml(computed)}</small>
    </td>
  `;
}

function signatureLabel(state: AppDocument): string {
  if (state.signatures.length === 0) {
    return state.signature_state;
  }
  const primary = state.signature ?? state.signatures[0];
  const count = state.signatures.length === 1 ? "1 signature" : `${state.signatures.length} signatures`;
  return `${state.signature_state} (${count}) by ${primary.signer_display} @ ${primary.target}`;
}

function renderBlock(block: AppBlock): string {
  if (block.kind === "table") {
    return `
      <table class="doc-table" data-id="${escapeHtml(block.id)}">
        <tbody>
          ${block.rows
            .map(
              (row) => `
                <tr>
                  ${row
                    .map(
                      (cell) => `
                        <td>${cell.map(renderBlock).join("")}</td>
                      `,
                    )
                    .join("")}
                </tr>
              `,
            )
            .join("")}
        </tbody>
      </table>
    `;
  }

  if (block.kind === "heading") {
    const level = Math.min(Math.max(block.level ?? 2, 1), 3);
    return `<h${level} data-id="${escapeHtml(block.id)}">${block.content.map(renderInline).join("")}</h${level}>`;
  }

  if (block.kind === "list-item") {
    const tag = block.ordered ? "ol" : "ul";
    const depth = Math.min(Math.max(block.level ?? 0, 0), 4);
    return `<${tag} class="doc-list depth-${depth}" data-id="${escapeHtml(block.id)}"><li>${block.content.map(renderInline).join("")}</li></${tag}>`;
  }

  if (block.kind === "equation-block") {
    return `<div class="equation-block" contenteditable="true" spellcheck="false" data-edit-equation-block-id="${escapeHtml(block.id)}" data-original-source="${escapeHtml(block.equation_source ?? "")}">${escapeHtml(block.equation_source ?? "")}</div>`;
  }

  if (block.kind === "page-break") {
    return `<hr class="page-break" data-id="${escapeHtml(block.id)}" />`;
  }

  return `<p data-id="${escapeHtml(block.id)}">${block.content.map(renderInline).join("")}</p>`;
}

function renderInline(inline: AppInline): string {
  const text = escapeHtml(inline.text);
  if (inline.kind === "link") {
    return editableInline(
      "a",
      inline,
      `href="${escapeHtml(inline.href ?? "#")}" data-link-href="${escapeHtml(inline.href ?? "")}"`,
      text,
    );
  }
  if (inline.kind === "citation") {
    return `<span class="citation-label" data-citation-id="${escapeHtml(inline.target_id ?? "")}">${text}</span>`;
  }
  if (inline.kind === "equation") {
    return editableInline("code", inline, "", text, ["equation-inline"]);
  }
  if (inline.kind === "mention") {
    return editableInline("span", inline, "", text, ["mention"]);
  }
  if (inline.kind === "footnote-ref") {
    return `<sup class="footnote-ref" data-footnote-id="${escapeHtml(inline.target_id ?? "")}">${text}</sup>`;
  }
  return editableInline("span", inline, "", text);
}

function editableInline(
  tag: string,
  inline: AppInline,
  attrs: string,
  text: string,
  baseClasses: string[] = [],
): string {
  const classes = [...baseClasses, ...inline.marks.map(markClass)].filter(Boolean);
  const mergedAttrs = [
    attrs,
    classes.length > 0 ? `class="${escapeHtml(classes.join(" "))}"` : "",
    `contenteditable="true"`,
    `spellcheck="true"`,
    `data-edit-inline-id="${escapeHtml(inline.id)}"`,
    `data-original-text="${escapeHtml(inline.text)}"`,
  ]
    .filter(Boolean)
    .join(" ");
  return `<${tag} ${mergedAttrs}>${text}</${tag}>`;
}

function bindEditableInlines() {
  document.querySelectorAll<HTMLElement>("[data-edit-inline-id]").forEach((node) => {
    node.addEventListener("click", (event) => {
      selectedInlineId = node.dataset.editInlineId ?? null;
      if (node instanceof HTMLAnchorElement) {
        event.preventDefault();
      }
    });
    node.addEventListener("focus", () => {
      selectedInlineId = node.dataset.editInlineId ?? null;
    });
    node.addEventListener("keydown", (event) => {
      if (event.key === "Enter") {
        event.preventDefault();
        node.blur();
      }
    });
    node.addEventListener("blur", () => {
      const inlineId = node.dataset.editInlineId;
      const original = node.dataset.originalText ?? "";
      const text = node.textContent ?? "";
      if (!inlineId || text === original) {
        return;
      }
      void command("update_inline_text", { inlineId, text });
    });
  });
}

function bindEditableEquationBlocks() {
  document.querySelectorAll<HTMLElement>("[data-edit-equation-block-id]").forEach((node) => {
    node.addEventListener("keydown", (event) => {
      if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
        event.preventDefault();
        node.blur();
      }
    });
    node.addEventListener("blur", () => {
      const blockId = node.dataset.editEquationBlockId;
      const original = node.dataset.originalSource ?? "";
      const source = node.textContent ?? "";
      if (!blockId || source === original) {
        return;
      }
      void command("update_block_equation_source", { blockId, source });
    });
  });
}

function bindEditableSpreadsheetCells() {
  document.querySelectorAll<HTMLElement>("[data-edit-cell-address]").forEach((node) => {
    node.addEventListener("keydown", (event) => {
      if (event.key === "Enter") {
        event.preventDefault();
        node.blur();
      }
    });
    node.addEventListener("blur", () => {
      const address = node.dataset.editCellAddress;
      const original = node.dataset.originalValue ?? "";
      const value = node.textContent ?? "";
      if (!address || value === original) {
        return;
      }
      void command("set_spreadsheet_cell", { address, value });
    });
  });
}

function bindEditableCitationReferences() {
  document.querySelectorAll<HTMLInputElement>("[data-citation-title]").forEach((titleInput) => {
    titleInput.addEventListener("change", () => {
      const referenceId = titleInput.dataset.citationTitle;
      if (!referenceId) return;
      const issuedInput = document.querySelector<HTMLInputElement>(
        `[data-citation-issued="${cssEscape(referenceId)}"]`,
      );
      void command("update_bibliography_reference", {
        referenceId,
        title: titleInput.value,
        issued: issuedInput?.value.trim() || null,
      });
    });
  });
  document.querySelectorAll<HTMLInputElement>("[data-citation-issued]").forEach((issuedInput) => {
    issuedInput.addEventListener("change", () => {
      const referenceId = issuedInput.dataset.citationIssued;
      if (!referenceId) return;
      const titleInput = document.querySelector<HTMLInputElement>(
        `[data-citation-title="${cssEscape(referenceId)}"]`,
      );
      void command("update_bibliography_reference", {
        referenceId,
        title: titleInput?.value ?? "",
        issued: issuedInput.value.trim() || null,
      });
    });
  });
}

function cssEscape(value: string): string {
  return value.replace(/["\\]/g, "\\$&");
}

function markClass(label: string): string {
  return `mark-${label.split(":")[0]}`;
}

function renderCitations(citations: AppCitationDatabase): string {
  return `
    <section>
      <h2>Citations</h2>
      <dl class="meta">
        <dt>Style</dt><dd>${escapeHtml(citations.style)}</dd>
        <dt>Locale</dt><dd>${escapeHtml(citations.locale)}</dd>
      </dl>
      ${citations.references
        .map(
          (ref) => `
            <div class="panel-item">
              <input class="inline-field" data-citation-title="${escapeHtml(ref.id)}" value="${escapeHtml(ref.title)}" />
              <input class="inline-field compact" data-citation-issued="${escapeHtml(ref.id)}" value="${escapeHtml(ref.issued ?? "")}" />
              <span>${escapeHtml(ref.authors.join(", "))}</span>
              <small>${escapeHtml(ref.format)} · ${escapeHtml(ref.id)}</small>
            </div>
          `,
        )
        .join("")}
      ${citations.citations
        .map(
          (citation) => `
            <div class="panel-item">
              <strong>${escapeHtml(citation.rendered_cache ?? citation.id)}</strong>
              <span>${escapeHtml(citation.placement)} · ${citation.items.length} item(s)</span>
              <small>${escapeHtml(citation.id)}</small>
            </div>
          `,
        )
        .join("")}
    </section>
  `;
}

function renderComments(comments: AppCommentThread[]): string {
  return `
    <section>
      <h2>Comments</h2>
      ${comments
        .map(
          (thread) => `
            <div class="panel-item">
              <strong>${escapeHtml(thread.comments[0]?.author ?? "Unknown")}</strong>
              <span>${escapeHtml(thread.comments[0]?.body ?? "")}</span>
              <small>${escapeHtml(thread.anchor)}</small>
              ${
                thread.deleted
                  ? `<small>deleted</small>`
                  : itemButton("delete-comment-thread", thread.id, "Delete", "Delete comment thread")
              }
            </div>
          `,
        )
        .join("")}
    </section>
  `;
}

function renderOperations(operations: AppOperationRecord[]): string {
  return `
    <section>
      <h2>Operations</h2>
      ${operations
        .slice(-8)
        .reverse()
        .map(
          (operation) => `
            <div class="panel-item">
              <strong>${escapeHtml(operation.kind)}</strong>
              <span>${escapeHtml(operation.summary)}</span>
              <small>${escapeHtml(operation.actor)} #${operation.seq}</small>
            </div>
          `,
        )
        .join("")}
    </section>
  `;
}

function renderSuggestions(suggestions: AppSuggestion[]): string {
  return `
    <section>
      <h2>Suggestions</h2>
      ${suggestions
        .map(
          (suggestion) => `
            <div class="panel-item">
              <strong>${escapeHtml(suggestion.kind)}</strong>
              <span>${escapeHtml(suggestion.author)}</span>
              <small>${escapeHtml(suggestion.state)}</small>
              ${
                suggestion.state === "proposed"
                  ? `<div class="item-actions">
                      ${itemButton("accept-suggestion", suggestion.id, "Accept", "Accept suggestion")}
                      ${itemButton("reject-suggestion", suggestion.id, "Reject", "Reject suggestion")}
                    </div>`
                  : ""
              }
            </div>
          `,
        )
        .join("")}
    </section>
  `;
}

function renderWarnings(warnings: AppWarning[]): string {
  return `
    <section>
      <h2>Warnings</h2>
      ${
        warnings.length === 0
          ? `<div class="empty">No warnings</div>`
          : warnings
              .map(
                (warning) => `
                  <div class="panel-item warning">
                    <strong>${escapeHtml(warning.code)}</strong>
                    <span>${escapeHtml(warning.message)}</span>
                  </div>
                `,
              )
              .join("")
      }
    </section>
  `;
}

function itemButton(action: string, targetId: string, label: string, title: string): string {
  return `<button type="button" data-action="${action}" data-target-id="${escapeHtml(targetId)}" title="${title}">${label}</button>`;
}

function escapeHtml(value: string): string {
  return value.replace(/[&<>"']/g, (char) => {
    const entities: Record<string, string> = {
      "&": "&amp;",
      "<": "&lt;",
      ">": "&gt;",
      '"': "&quot;",
      "'": "&#039;",
    };
    return entities[char] ?? char;
  });
}

void load();
