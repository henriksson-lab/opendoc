// Shared view state for the desktop frontend.
//
// This is *view* state, never document state: what screen is up, which panel is
// open, where the caret is, what the last error said. The document itself is a
// projection handed back by Rust and simply parked here so every surface reads
// the same copy.
//
// It is one mutable object rather than a set of module-level `let`s because an
// imported binding is read-only: a surface module has to be able to write
// `state.panel = "warnings"` and have every other module see it.
import type { DocumentEditor } from "./editor";
import { runtimeConfig } from "./invoke";
import type { AppAuditView, AppDocument, EditorSelection, OpenDocRuntimeProfile } from "./types";

export type View = "home" | "editor";
export type Mode = "docs" | "sheets";
export type Panel = "comments" | "suggestions" | "citations" | "footnotes" | "files" | "warnings" | "history" | "signatures" | "versions";
export type ListKindName = "bullet" | "ordered" | "checklist";

export const runtime = runtimeConfig();

/**
 * The application root. It is created by the page and never replaced, which is
 * what makes it the right place for the one delegated `[data-action]` listener.
 */
export const app = document.getElementById("app") as HTMLElement;

/**
 * The contenteditable host. Module-level, so it survives every shell rebuild —
 * which is why the image-resize listeners on it are assignments rather than
 * `addEventListener`, and why a new `DocumentEditor` must destroy the old one.
 */
export const editorHost = document.createElement("div");
editorHost.className = "doc-body";

export type AppState = {
  doc: AppDocument | null;
  profile: OpenDocRuntimeProfile | null;
  audit: AppAuditView | null;
  view: View;
  mode: Mode;
  panel: Panel | null;
  zoom: number;
  selection: EditorSelection | null;
  lastError: string | null;
  editor: DocumentEditor | null;
  authorName: string;
};

export const state: AppState = {
  doc: null,
  profile: null,
  audit: null,
  view: "home",
  mode: "docs",
  panel: null,
  zoom: 1,
  selection: null,
  lastError: null,
  editor: null,
  authorName: runtime.subject ?? "Local user",
};
