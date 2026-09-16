// The one-time listener binding.
//
// This is the single most bug-prone invariant in the frontend: listener
// stacking has produced four separate user-visible bugs here, each of them a
// click that ran its action twice. The rule it encodes is that a listener may
// only be attached to a node that outlives every render, and then only once.
//
// `app` is created by the page and never replaced, so the whole application
// gets by with one delegated `[data-action]` click listener on it — which is
// also why no surface module may call `addEventListener` on a re-runnable
// path. Controls that *are* discarded with the editor shell are bound by
// `bindShellControls()` in `shell.ts` instead, where re-binding is safe
// precisely because the previous nodes went away with their listeners.
import { app, state } from "./state";
import { runAction } from "./actions";

let staticBound = false;

/**
 * Listeners on nodes that outlive a render (`app`, `document`). `app` is never
 * replaced, so these must be attached exactly once: re-attaching them on every
 * shell rebuild made one click run its action N times.
 */
export function bindStatic(): void {
  if (staticBound) return;
  staticBound = true;
  app.addEventListener("click", (event) => {
    const target = (event.target as Element).closest<HTMLElement>("[data-action]");
    if (!target || target.hasAttribute("disabled")) return;
    const menu = target.closest("details.menu-group");
    if (menu) menu.removeAttribute("open");
    void runAction(target.dataset.action ?? "", target.dataset);
  });
  app.addEventListener("submit", (event) => {
    const form = (event.target as Element | null)?.closest<HTMLFormElement>("[data-comment-reply-form]");
    if (!form) return;
    event.preventDefault();
    const body = form.querySelector<HTMLTextAreaElement>('textarea[name="body"]')?.value;
    const threadId = form.dataset.threadId;
    if (typeof body !== "string" || !threadId) return;
    // Form bodies are deliberately read at submission time, never encoded in
    // an HTML data attribute.  This keeps user text out of our HTML renderer.
    void runAction("submit-comment-reply", { id: threadId, body } as DOMStringMap);
  });
  app.addEventListener(
    "toggle",
    (event) => {
      const opened = event.target as HTMLElement;
      if (opened.tagName === "DETAILS" && (opened as HTMLDetailsElement).open) {
        app.querySelectorAll<HTMLDetailsElement>("details.menu-group[open]").forEach((other) => {
          if (other !== opened) other.open = false;
        });
      }
    },
    true,
  );
  document.addEventListener("click", (event) => {
    if (!(event.target as Element).closest("details.menu-group")) {
      app.querySelectorAll<HTMLDetailsElement>("details.menu-group[open]").forEach((other) => {
        other.open = false;
      });
    }
  });
  document.addEventListener("keydown", (event) => {
    if (event.key === "Escape") {
      const reply = (event.target as Element | null)?.closest<HTMLFormElement>("[data-comment-reply-form]");
      if (reply?.dataset.threadId) {
        event.preventDefault();
        void runAction("cancel-comment-reply", { id: reply.dataset.threadId } as DOMStringMap);
        return;
      }
      app.querySelectorAll<HTMLDetailsElement>("details.menu-group[open]").forEach((other) => {
        other.open = false;
      });
    }
    // These deliberately live above the contenteditable key handler: review
    // traversal must also work when focus is on the sidebar.  The chord does
    // not produce text and is announced on the controls themselves.
    if (state.mode === "docs" && (event.ctrlKey || event.metaKey) && event.altKey && !event.shiftKey) {
      if (event.key === "ArrowDown") {
        event.preventDefault();
        void runAction("comment-next");
        return;
      }
      if (event.key === "ArrowUp") {
        event.preventDefault();
        void runAction("comment-previous");
        return;
      }
      if (event.key === "ArrowRight") {
        event.preventDefault();
        void runAction("suggestion-next");
        return;
      }
      if (event.key === "ArrowLeft") {
        event.preventDefault();
        void runAction("suggestion-previous");
        return;
      }
    }
    const active = document.activeElement as HTMLElement | null;
    if (active?.closest("[data-menu-bar]")) {
      const items = Array.from(app.querySelectorAll<HTMLElement>("[data-menu-bar] summary, [data-menu-bar] details[open] [role=menuitem]"));
      const index = items.indexOf(active);
      if (event.key === "ArrowRight" || event.key === "ArrowDown") {
        event.preventDefault();
        items[(index + 1) % items.length]?.focus();
      } else if (event.key === "ArrowLeft" || event.key === "ArrowUp") {
        event.preventDefault();
        items[(index - 1 + items.length) % items.length]?.focus();
      }
    }
  });
}
