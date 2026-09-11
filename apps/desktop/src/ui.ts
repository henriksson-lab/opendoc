export type DialogField = {
  name: string;
  label: string;
  type?: "text" | "textarea" | "number" | "select" | "password" | "color";
  value?: string;
  options?: { value: string; label: string }[];
  placeholder?: string;
};

let afterDialogClose: () => void = () => {};

/** Titles of dialogs that are currently on screen, used to reject duplicates. */
const openDialogTitles = new Set<string>();

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
    dialog.innerHTML = `
      <form method="dialog" class="modal-form">
        <h2>${escapeHtml(options.title)}</h2>
        ${options.fields
          .map((field) => {
            const id = `field-${field.name}`;
            const control =
              field.type === "textarea"
                ? `<textarea id="${id}" name="${escapeHtml(field.name)}" rows="5" placeholder="${escapeHtml(field.placeholder ?? "")}">${escapeHtml(field.value ?? "")}</textarea>`
                : field.type === "select"
                  ? `<select id="${id}" name="${escapeHtml(field.name)}">${(field.options ?? [])
                      .map((option) => `<option value="${escapeHtml(option.value)}"${option.value === field.value ? " selected" : ""}>${escapeHtml(option.label)}</option>`)
                      .join("")}</select>`
                  : `<input id="${id}" name="${escapeHtml(field.name)}" type="${field.type ?? "text"}" value="${escapeHtml(field.value ?? "")}" placeholder="${escapeHtml(field.placeholder ?? "")}">`;
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
    focusFieldAtEnd(form.querySelector<HTMLElement>("input, textarea, select"));
  });
}

export async function confirmDialog(title: string, body: string, okLabel = "OK"): Promise<boolean> {
  const result = await promptDialog({
    title,
    fields: [{ name: "note", label: body, type: "text", value: "" }],
    submit: okLabel,
  });
  return result !== null;
}
