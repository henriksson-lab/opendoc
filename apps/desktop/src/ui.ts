export type DialogField = {
  name: string;
  label: string;
  type?: "text" | "textarea" | "number" | "select" | "password" | "color";
  value?: string;
  options?: { value: string; label: string }[];
  placeholder?: string;
};

let afterDialogClose: () => void = () => {};

export function setDialogAfterClose(handler: () => void): void {
  afterDialogClose = handler;
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
      resolve(result);
      afterDialogClose();
    });
    dialog.showModal();
    const first = form.querySelector("input, textarea, select") as HTMLElement | null;
    first?.focus();
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
