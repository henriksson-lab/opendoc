// Application bootstrap.
//
// What is left in this file is what belongs to the app as a whole: the boot
// sequence, crash recovery, and the hook the test harnesses drive the UI
// through. Every screen lives in its own module — `shell.ts` composes a render
// pass, `actions.ts` routes every gesture, `bindings.ts` owns the one-time
// listener binding.
import "./styles.css";
import { escapeHtml, promptDialog, setDialogAfterClose } from "./ui";
import { closeWindow, invoke, isTauri, onCloseRequested } from "./invoke";
import type { EditorSelection } from "./types";
import type { AppVersionView } from "./generated/version";
import { app, runtime, state } from "./state";
import { applyDocument } from "./shared";
import { renderAll, renderStatus } from "./shell";
import { runAction } from "./actions";
import { setVersionView } from "./versions";

setDialogAfterClose(() => state.editor?.focus());

// Printing needs no hook of its own any more. Every length the paginator
// applies is a `calc()` over `var(--page-gap)`, and the print stylesheet sets
// that gap to zero, so the sheet boundaries land on whole multiples of the
// page height without anything being recomputed. Before ADR 0014 this was two
// `beforeprint`/`afterprint` listeners re-running a measuring paginator.

// ---- Crash recovery ------------------------------------------------------------

/**
 * Offer back what an unclean session left behind (ADR 0005).
 *
 * Nothing is replayed without the user saying so, and the dialog lists the
 * operations a replay would apply so the offer can be inspected rather than
 * trusted. Declining keeps the segment: "decide later" is a real answer.
 */
async function offerRecoveredSessions(): Promise<void> {
  for (;;) {
    const session = state.doc?.recovery_sessions?.[0];
    if (!session) return;
    const changes = session.operations.map((operation) => operation.summary);
    const listed = changes.length > 40 ? [`… ${changes.length - 40} earlier changes`, ...changes.slice(-40)] : changes;
    const answer = await promptDialog({
      title: "Recover unsaved work?",
      fields: [
        {
          name: "choice",
          label: `"${session.title}" still had ${session.operation_count} unsaved change${session.operation_count === 1 ? "" : "s"} when OpenDoc stopped.${session.truncated ? " The very last change was cut short and cannot be replayed." : ""}`,
          type: "select",
          value: "recover",
          options: [
            { value: "recover", label: "Recover these changes" },
            { value: "later", label: "Decide later (keep them on disk)" },
            { value: "discard", label: "Discard these changes permanently" },
          ],
        },
        {
          name: "operations",
          label: "Changes that would be replayed",
          type: "textarea",
          value: listed.join("\n"),
        },
      ],
      submit: "Continue",
      cancel: "Decide later",
    });
    if (!answer || answer.choice === "later") return;
    if (answer.choice === "recover") {
      await runAction("recover-session", { sessionId: session.id });
      return;
    }
    const before = state.doc?.recovery_sessions?.length ?? 0;
    await runAction("discard-recovery-session", { sessionId: session.id });
    if ((state.doc?.recovery_sessions?.length ?? 0) >= before) return;
  }
}

// ---- Boot ----------------------------------------------------------------------

/** The faces `opendoc-layout` paginated against.
 *
 *  A browser fetches a web font only when something on the page uses it, so a
 *  document that acquires its first bold word mid-session would be drawn in a
 *  fallback face until the real one arrived. The layout does not change — Rust
 *  already measured the bundled face — but the screen would disagree with it
 *  for as long as the fetch took. Asking for all five up front removes that
 *  window. A failure is not fatal: the fallbacks in the stylesheet are
 *  metric-compatible enough to read, and the console says what was missed. */
function loadDocumentFaces(): void {
  const faces = [
    '11pt "OpenDoc Sans"',
    'bold 11pt "OpenDoc Sans"',
    'italic 11pt "OpenDoc Sans"',
    'bold italic 11pt "OpenDoc Sans"',
    '11pt "OpenDoc Mono"',
  ];
  // jsdom has no font loading API at all, and neither did browsers before
  // CSS Font Loading shipped; in both the stylesheet still fetches the faces
  // on first use, so the only thing lost is the head start.
  const fonts = document.fonts;
  if (!fonts) return;
  for (const face of faces) {
    fonts.load(face).catch((error) => console.warn("document face unavailable", face, error));
  }
}

async function boot(): Promise<void> {
  loadDocumentFaces();
  try {
    state.profile = await invoke("get_runtime_profile", { mode: runtime.mode, storageBackends: runtime.storageBackends ?? [], signingEnabled: runtime.signingEnabled });
  } catch (error) {
    console.warn("runtime profile unavailable", error);
  }
  try {
    state.doc = await invoke("get_document");
    state.audit = null;
  } catch (error) {
    app.innerHTML = `<main class="shell"><p class="error-banner">Could not start OpenDoc: ${escapeHtml(String(error))}</p></main>`;
    return;
  }
  renderAll();
  await offerRecoveredSessions();
  window.setInterval(() => {
    if (!state.doc?.repository_root || !state.doc.has_unsaved_changes) return;
    invoke("autosave_current_repository")
      .then((updated) => {
        state.doc = updated;
        renderStatus();
      })
      .catch(() => {});
  }, 5000);
  window.addEventListener("beforeunload", (event) => {
    if (state.doc?.has_unsaved_changes && !isTauri()) {
      event.preventDefault();
    }
  });
  void onCloseRequested(async () => {
    const choice = await promptDialog({
      title: "Unsaved changes",
      fields: [
        {
          name: "choice",
          label: "What do you want to do?",
          type: "select",
          value: "save",
          options: [
            { value: "save", label: "Save and close" },
            { value: "discard", label: "Discard changes and close" },
          ],
        },
      ],
      submit: "Continue",
      cancel: "Keep editing",
    });
    if (!choice) return;
    if (choice.choice === "save") await runAction("save");
    if (choice.choice === "discard") {
      // The user chose to lose this work, so lose it in Rust too: closing the
      // document clears its recovery segment, and the next launch does not
      // offer back changes that were deliberately thrown away.
      await invoke("close_document", {}, { discardUnsavedChanges: true })
        .then(applyDocument)
        .catch(() => {});
    }
    if (!state.doc?.has_unsaved_changes || choice.choice === "discard") await closeWindow();
  });
}

export const __test = {
  runAction,
  getState: () => ({ doc: state.doc, view: state.view, mode: state.mode, panel: state.panel, selection: state.selection, audit: state.audit }),
  setSelection: (next: EditorSelection | null) => (state.selection = next),
  // Lets a browser-level test render the version panel against a known view;
  // the WASM shell has no filesystem repository to read a real one from.
  setVersionView: (next: AppVersionView | null) => setVersionView(next),
};

void boot();
