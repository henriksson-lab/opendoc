export type DialogField = {
  name: string;
  label: string;
  type?: "text" | "textarea" | "number" | "date" | "select" | "password" | "color";
  value?: string;
  options?: { value: string; label: string }[];
  placeholder?: string;
  /** Granularity of a `number` field. Defaults to `"any"`; see below. */
  step?: string;
  /**
   * Shows a value the dialog will not change.
   *
   * A read-only field is still a field: it is focusable, selectable and
   * copyable, and it is still reported back in the result, so a caller reads
   * it the same way it reads any other. What it is not is editable — which is
   * the honest version of File ▸ Details, where a plain `textarea` invited an
   * edit that silently did nothing.
   *
   * Not supported on `select`: a `<select>` has no read-only state (only
   * `disabled`, which drops the value out of the form), so a fixed choice is
   * a `text` field showing the chosen label.
   */
  readonly?: boolean;
};

/**
 * `step` for a `number` field, defaulting to `"any"`.
 *
 * Without it the default step is 1 and the step *base* is the field's initial
 * value, so a dialog pre-filled with "1.00" silently refuses to submit once
 * the user types "0.50": the form fails constraint validation and the submit
 * does nothing at all, with no message. Every numeric dialog in this app
 * offers a real-world measurement, so "any" is the right default and a field
 * that genuinely wants whole numbers says so.
 */
function numberStep(field: DialogField): string {
  if (field.type !== "number") return "";
  return ` step="${escapeHtml(field.step ?? "any")}"`;
}

let afterDialogClose: () => void = () => {};

/** Titles of dialogs that are currently on screen, used to reject duplicates. */
const openDialogTitles = new Set<string>();

/**
 * Native `<dialog>` does not derive an accessible name from a contained
 * heading.  Keep generated targets unique even when two different commands
 * happen to open dialogs at once, rather than reusing a pleasant-looking but
 * duplicate `id`.
 */
let nextDialogA11yId = 0;

/**
 * Input types that support text selection. `setSelectionRange` is not defined on
 * `<select>` and throws on the other input types (number, color, date, ...).
 */
const CARET_INPUT_TYPES = new Set(["text", "search", "url", "tel", "password"]);

export function setDialogAfterClose(handler: () => void): void {
  afterDialogClose = handler;
}

/**
 * Focus a text field and put the caret after the last character, so that typing
 * appends to a pre-filled value instead of prepending to it. Safe on every
 * element type: non-text controls are only focused.
 */
export function focusFieldAtEnd(field: HTMLElement | null | undefined): void {
  if (!field) return;
  field.focus();
  const tag = field.tagName;
  if (tag !== "INPUT" && tag !== "TEXTAREA") return;
  const control = field as HTMLInputElement | HTMLTextAreaElement;
  if (tag === "INPUT" && !CARET_INPUT_TYPES.has((control as HTMLInputElement).type)) return;
  const end = control.value.length;
  try {
    control.setSelectionRange(end, end);
  } catch {
    // Some engines still refuse selection on exotic input types; focus is enough.
  }
}

export function escapeHtml(value: string): string {
  return value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

export function toast(message: string): void {
  const node = document.createElement("div");
  node.className = "toast";
  node.setAttribute("role", "status");
  node.textContent = message;
  document.body.appendChild(node);
  window.setTimeout(() => node.remove(), 2500);
}

export function promptDialog(options: {
  title: string;
  /**
   * Prose above the fields: a question to answer, or a value to read.
   *
   * A dialog that only has something to *say* is all body and no fields —
   * see `confirmDialog`. Saying it through a dummy field instead is what put
   * an empty one-line text box under every confirm in the app.
   */
  body?: string;
  fields: DialogField[];
  submit?: string;
  cancel?: string;
}): Promise<Record<string, string> | null> {
  return new Promise((resolve) => {
    // Defence in depth: a duplicate dialog can only come from a double-fired
    // action, and stacking two of them makes the user dismiss each one.
    if (openDialogTitles.has(options.title)) {
      resolve(null);
      return;
    }
    openDialogTitles.add(options.title);
    const dialog = document.createElement("dialog");
    dialog.className = "modal";
    const dialogA11yId = ++nextDialogA11yId;
    const titleId = `opendoc-dialog-title-${dialogA11yId}`;
    const descriptionId = `opendoc-dialog-description-${dialogA11yId}`;
    dialog.setAttribute("aria-labelledby", titleId);
    if (options.body) dialog.setAttribute("aria-describedby", descriptionId);
    dialog.innerHTML = `
      <form method="dialog" class="modal-form">
        <h2 id="${titleId}">${escapeHtml(options.title)}</h2>
        ${options.body ? `<p id="${descriptionId}" class="modal-body">${escapeHtml(options.body)}</p>` : ""}
        ${options.fields
          .map((field) => {
            const id = `field-${field.name}`;
            // `readonly` is the attribute, not `disabled`: a disabled control
            // is skipped by form submission and cannot be selected, and both
            // of those are wrong for a value the user is here to read.
            const readonly = field.readonly && field.type !== "select" ? " readonly" : "";
            const control =
              field.type === "textarea"
                ? `<textarea id="${id}" name="${escapeHtml(field.name)}" rows="5"${readonly} placeholder="${escapeHtml(field.placeholder ?? "")}">${escapeHtml(field.value ?? "")}</textarea>`
                : field.type === "select"
                  ? `<select id="${id}" name="${escapeHtml(field.name)}">${(field.options ?? [])
                      .map((option) => `<option value="${escapeHtml(option.value)}"${option.value === field.value ? " selected" : ""}>${escapeHtml(option.label)}</option>`)
                      .join("")}</select>`
                  : `<input id="${id}" name="${escapeHtml(field.name)}" type="${field.type ?? "text"}"${numberStep(field)}${readonly} value="${escapeHtml(field.value ?? "")}" placeholder="${escapeHtml(field.placeholder ?? "")}">`;
            return `<label for="${id}"><span>${escapeHtml(field.label)}</span>${control}</label>`;
          })
          .join("")}
        <div class="modal-actions">
          <button type="button" value="cancel" data-cancel>${escapeHtml(options.cancel ?? "Cancel")}</button>
          <button type="submit" value="ok" class="primary">${escapeHtml(options.submit ?? "OK")}</button>
        </div>
      </form>`;
    document.body.appendChild(dialog);
    const form = dialog.querySelector("form") as HTMLFormElement;
    let result: Record<string, string> | null = null;
    form.addEventListener("submit", () => {
      result = {};
      for (const field of options.fields) {
        const control = form.elements.namedItem(field.name) as
          | HTMLInputElement
          | HTMLTextAreaElement
          | HTMLSelectElement
          | null;
        result[field.name] = control?.value ?? "";
      }
    });
    dialog.querySelector("[data-cancel]")?.addEventListener("click", () => dialog.close());
    dialog.addEventListener("close", () => {
      dialog.remove();
      openDialogTitles.delete(options.title);
      resolve(result);
      afterDialogClose();
    });
    dialog.showModal();
    // A dialog with no fields has nothing to type in, so the primary button
    // takes focus instead: that is the control Enter should then answer, and
    // leaving focus on Cancel would make Enter mean "no".
    focusFieldAtEnd(
      form.querySelector<HTMLElement>("input, textarea, select") ?? dialog.querySelector<HTMLElement>("button.primary"),
    );
  });
}

/**
 * A yes/no question. Resolves true only if the user accepted it.
 *
 * A confirm has **no fields**: the question is the dialog's body and the
 * answer is which control closed it — the submit button resolves the form,
 * while Cancel, Escape and any other close resolve `null`. It used to be a
 * `promptDialog` carrying one dummy text field, which is why "Discard unsaved
 * changes?" — and every other confirm, including the window-close guard —
 * showed an unused one-line text box under the question.
 *
 * Everything else is `promptDialog`'s: the dialog is modal and so
 * focus-trapped, Escape cancels it, duplicate titles cannot stack, and
 * `setDialogAfterClose` returns focus to the editor once it is gone.
 */
export async function confirmDialog(title: string, body: string, okLabel = "OK"): Promise<boolean> {
  const result = await promptDialog({ title, body, fields: [], submit: okLabel });
  return result !== null;
}
