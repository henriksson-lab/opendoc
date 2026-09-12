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

/**
 * Raised when the user declines to discard unsaved work. Not an error: the
 * action simply stops, which `runAction` swallows.
 */
export const ACTION_CANCELLED = new Error("action cancelled");

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
  try {
    const result = await invoke(command, args, { discardUnsavedChanges });
    state.lastError = null;
    if (result && typeof result === "object" && "blocks" in (result as object)) {
      applyDocument(result as unknown as AppDocument);
    }
    return result;
  } catch (error) {
    if (isUnsavedChangesError(error)) throw error;
    showError(error instanceof Error ? error.message : String(error));
    throw error;
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
  state.doc = next;
  renderAll();
}

// ---- Document helpers -------------------------------------------------------

function* walkBlocks(blocks: AppBlock[]): Generator<AppBlock> {
  for (const block of blocks) {
    yield block;
    for (const row of block.rows) {
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

function findInline(id: string): { block: AppBlock; inline: AppInline } | null {
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
  const block = focusBlock() ?? state.doc?.blocks[state.doc.blocks.length - 1] ?? null;
  if (!block) showError("Place the caret in the document first.");
  return block;
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
