// Helpers every surface needs: running a command, reporting an error, and
// looking something up in the document projection.
//
// Nothing here decides a document fact. `findBlock` and friends walk the
// projection Rust just handed back in order to name an id for the next command;
// they do not compute structure, and they must not grow to.
import { confirmDialog } from "./ui";
import type { CommandArgs, DesktopCommandName, DocumentCommandName } from "./commands";
import { invoke, isUnsavedChangesError } from "./invoke";
import type { AppBlock, AppDocument, AppEditorSelection, AppInline } from "./types";
import { app, state } from "./state";
import { renderAll, renderStatus } from "./shell";

export function query<T extends Element = HTMLElement>(selector: string, root: ParentNode = app): T | null {
  return root.querySelector(selector) as T | null;
}

export function showError(message: string): void {
  state.lastError = message;
  renderStatus();
}

/** Mark command-backed work as in progress for the current local projection.
 *
 * Commands can overlap (for example, an image insertion can await bytes while
 * a user invokes a formatting command), so this intentionally tracks a count
 * instead of letting one completion clear another command's busy state.
 * `renderStatus` updates only small live/projection attributes; it does not
 * rebuild the editor or move focus.
 */
function setPendingOperations(change: 1 | -1): void {
  state.pendingOperations = Math.max(0, state.pendingOperations + change);
  renderStatus();
}

/** Browser-smoke seam for the a11y counter, not document data. */
export function adjustPendingOperationsForTest(change: 1 | -1): void {
  setPendingOperations(change);
}

/**
 * Raised when the user declines to discard unsaved work. Not an error: the
 * action simply stops, which `runAction` swallows.
 */
export const ACTION_CANCELLED = new Error("action cancelled");

/**
 * Runs one native shell capability (a file dialog, file IO, a URL fetch) and
 * turns a rejection into a visible message.
 *
 * It lives here, beside `run()` and `showError()`, because it is the same kind
 * of thing: the one place a whole class of failure is turned into a sentence
 * on screen. (Not in `ui.ts` — that is the layer `shared.ts` imports, and
 * putting it there would make the cycle.) Every surface that reaches a shell
 * capability needs it: `actions.ts`, `files.ts` and `spreadsheet.ts`.
 *
 * Commands go through `run()`, which reports whatever Rust refuses. The
 * shell's own capabilities do not: `invoke.ts` hands their promise straight
 * back, and an action started from a delegated click is launched as
 * `void runAction(...)`, so a rejected native call ends its life as an
 * unhandled rejection in the console — no image, no file, no message. That is
 * exactly how `fetch_url_base64` being absent from the Tauri capability file
 * stayed invisible: Insert ▸ Image by URL did nothing at all, and the missing
 * ACL grant only surfaced when someone read `generate_handler!`. The same
 * silence covered a refused `write_file_base64`: an export produced neither
 * the "Exported as …" toast nor an error (PLAN77, 2026-09-12).
 *
 * A `null` answer is never a failure and is passed through for the caller to
 * read: for a dialog it means the user cancelled, and for a native-only
 * capability it means this runtime does not have it — two answers that need
 * two different messages, which is why this wrapper does not invent one.
 * After reporting, the action stops the way a declined prompt stops it.
 */
export async function native<T>(whatFailed: string, call: () => Promise<T | null>): Promise<T | null> {
  try {
    return await call();
  } catch (error) {
    showError(`${whatFailed}: ${error instanceof Error ? error.message : String(error)}`);
    throw ACTION_CANCELLED;
  }
}

export async function run<K extends DesktopCommandName>(command: K, args: CommandArgs<K> = {} as CommandArgs<K>) {
  try {
    return await runDispatch(command, args, false);
  } catch (error) {
    // Rust refuses any command that would replace the open document while it
    // holds unsaved work (ADR 0005), whichever action asked for it. This is
    // the only place that prompt is written: a File-menu action added later
    // inherits it without knowing it exists.
    if (isUnsavedChangesError(error)) {
      const accepted = await confirmDialog(
        "Discard unsaved changes?",
        `"${state.doc?.title ?? "The document"}" has changes that are not saved. Continuing loses them.`,
        "Discard",
      );
      if (!accepted) throw ACTION_CANCELLED;
      return await runDispatch(command, args, true);
    }
    throw error;
  }
}

async function runDispatch<K extends DesktopCommandName>(command: K, args: CommandArgs<K>, discardUnsavedChanges: boolean) {
  setPendingOperations(1);
  try {
    const result = await invoke(command, args, { discardUnsavedChanges });
    state.lastError = null;
    // `blocks` is the marker for "this result is a document projection". It is
    // the one field of `AppDocument` that is always on the wire even when
    // empty, so that a closed or empty document still re-renders; see the
    // field's note in `opendoc-app/src/document.rs`.
    if (result && typeof result === "object" && "blocks" in (result as object)) {
      applyDocument(result as unknown as AppDocument);
    }
    return result;
  } catch (error) {
    if (isUnsavedChangesError(error)) throw error;
    showError(error instanceof Error ? error.message : String(error));
    throw error;
  } finally {
    setPendingOperations(-1);
  }
}

export async function edit<K extends DocumentCommandName>(command: K, args: CommandArgs<K> = {} as CommandArgs<K>) {
  try {
    await run(command, args);
  } catch {
    // reported by run()
  }
}

export function applyDocument(next: AppDocument): void {
  // `renderAll` refreshes the document surface before it rebuilds the side
  // panel, which can make a focused review card lose browser focus before the
  // panel renderer has a chance to inspect it. Capture this only for an
  // existing current card; arbitrary input/button focus must not be stolen.
  const active = document.activeElement;
  if (active instanceof HTMLElement) {
    const thread = active.closest<HTMLElement>("[data-thread].current");
    const suggestion = active.closest<HTMLElement>("[data-suggestion-id].current");
    state.pendingReviewFocus = thread?.dataset.thread
      ? { panel: "comments", id: thread.dataset.thread }
      : suggestion?.dataset.suggestionId
        ? { panel: "suggestions", id: suggestion.dataset.suggestionId }
        : null;
  } else {
    state.pendingReviewFocus = null;
  }
  state.doc = next;
  renderAll();
}

// ---- Document helpers -------------------------------------------------------

function* walkBlocks(blocks: AppBlock[]): Generator<AppBlock> {
  for (const block of blocks) {
    yield block;
    // `rows` is present only on a table block: the projection omits what a
    // block does not have rather than spelling out its absence.
    for (const row of block.rows ?? []) {
      for (const cell of row) {
        yield* walkBlocks(cell);
      }
    }
  }
}

export function findBlock(id: string): AppBlock | null {
  if (!state.doc) return null;
  for (const block of walkBlocks(state.doc.blocks)) {
    if (block.id === id) return block;
  }
  return null;
}

/** Find a projected inline in the body or any recursively nested table cell.
 *
 * Atomic controls use this before opening their typed editor. Keeping the
 * lookup alongside the block walker prevents a top-level-only UI path from
 * making a durable inline inside a table appear inert.
 */
export function findInline(id: string): { block: AppBlock; inline: AppInline } | null {
  if (!state.doc) return null;
  for (const block of walkBlocks(state.doc.blocks)) {
    const inline = block.content.find((item) => item.id === id);
    if (inline) return { block, inline };
  }
  return null;
}

export function focusBlock(): AppBlock | null {
  return state.selection ? findBlock(state.selection.focus.block_id) : null;
}

export function focusInline(): AppInline | null {
  if (!state.selection?.focus.inline_id) return null;
  return findInline(state.selection.focus.inline_id)?.inline ?? null;
}

export function wordStats(): string {
  return `${state.doc?.word_count ?? 0} words · ${state.doc?.character_count ?? 0} characters`;
}

export function requireBlock(): AppBlock | null {
  const block = focusBlock();
  // A missing selection is normal for a command invoked before the editor has
  // focused (for example immediately after opening a document), where the
  // final top-level block is a useful insertion default. A *present* selection
  // whose durable block disappeared is different: falling through to that
  // default would make an insertion land in an unrelated paragraph.
  if (state.selection && !block) {
    showError("The selected content was deleted or changed. Place the caret again before inserting.");
    return null;
  }
  const target = block ?? state.doc?.blocks[state.doc.blocks.length - 1] ?? null;
  if (!target) showError("Place the caret in the document first.");
  return target;
}

export async function describeEditorSelection(): Promise<AppEditorSelection | null> {
  if (!state.selection) return null;
  try {
    return await run("describe_editor_selection", { selection: state.selection });
  } catch {
    return null;
  }
}

export function selectionText(): string {
  const domSelection = document.getSelection();
  return domSelection && !domSelection.isCollapsed ? domSelection.toString() : "";
}

export function bytesFromBase64(base64: string): number[] {
  return Array.from(Uint8Array.from(atob(base64), (ch) => ch.charCodeAt(0)));
}

export function textFromBase64(base64: string): string {
  return new TextDecoder().decode(Uint8Array.from(atob(base64), (ch) => ch.charCodeAt(0)));
}
