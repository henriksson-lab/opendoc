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
import type { AppAuditView, AppDocument, EditorSelection, OpenDocRuntimeProfile, OpenDocRuntimeSession } from "./types";

export type View = "home" | "editor";
export type Mode = "docs" | "sheets";
/** Editing intent for the document body. Server roles remain authoritative. */
export type DocumentEditingMode = "edit" | "suggest" | "view";
export type Panel = "outline" | "bookmarks" | "comments" | "suggestions" | "citations" | "footnotes" | "files" | "warnings" | "history" | "signatures" | "versions";
/** Which non-deleted comment threads the sidebar shows. This is view state. */
export type CommentFilter = "open" | "resolved" | "for-you" | "all";
/** Which durable suggestion records the sidebar shows. Resolution evidence is
 * limited to its stored state; it is not an activity ledger. */
export type SuggestionFilter = "proposed" | "resolved" | "all";
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
  runtimeSession: OpenDocRuntimeSession | null;
  audit: AppAuditView | null;
  view: View;
  mode: Mode;
  documentEditingMode: DocumentEditingMode;
  panel: Panel | null;
  commentFilter: CommentFilter;
  suggestionFilter: SuggestionFilter;
  /** Last comment reached through the review navigation, not document data. */
  activeCommentThreadId: string | null;
  /** Last proposed change reached through review navigation, never document data. */
  activeSuggestionId: string | null;
  /** Thread whose inline reply composer is open; never durable document data. */
  activeCommentReplyThreadId: string | null;
  /** Keyboard review card that a whole remote projection is about to replace. */
  pendingReviewFocus: { panel: "comments" | "suggestions"; id: string } | null;
  zoom: number;
  selection: EditorSelection | null;
  lastError: string | null;
  /**
   * In-flight command-backed work. This is view state: it is deliberately
   * neither persisted nor sent to another collaborator. A count, rather than
   * a boolean, keeps an earlier completion from falsely declaring the
   * workspace ready while another command is still awaiting its result.
   */
  pendingOperations: number;
  editor: DocumentEditor | null;
  authorName: string;
};

export const state: AppState = {
  doc: null,
  profile: null,
  runtimeSession: null,
  audit: null,
  view: "home",
  mode: "docs",
  documentEditingMode: "edit",
  panel: null,
  commentFilter: "open",
  suggestionFilter: "proposed",
  activeCommentThreadId: null,
  activeSuggestionId: null,
  activeCommentReplyThreadId: null,
  pendingReviewFocus: null,
  zoom: 1,
  selection: null,
  lastError: null,
  pendingOperations: 0,
  editor: null,
  authorName: runtime.subject ?? "Local user",
};

/**
 * Keep the sidebar and its Previous/Next controls on the identical thread
 * set.  An action that is complete is historical evidence, not outstanding
 * work, so it does not appear in the personal work queue even if the parent
 * comment is still open.
 */
export function matchesCommentFilter(
  thread: AppDocument["comments"][number],
  filter: CommentFilter,
  authorName: string,
): boolean {
  if (thread.deleted) return false;
  if (filter === "all") return true;
  if (filter === "for-you") {
    return (
      thread.state !== "resolved" &&
      thread.action_assignee === authorName &&
      !thread.action_completed_by
    );
  }
  const resolved = thread.state === "resolved";
  return filter === "resolved" ? resolved : !resolved;
}

/** Accepted/rejected records are resolved, but legacy free-form provenance is
 * intentionally not interpreted as reviewer or time data. */
export function matchesSuggestionFilter(
  suggestion: AppDocument["suggestions"][number],
  filter: SuggestionFilter,
): boolean {
  if (filter === "all") return true;
  if (filter === "proposed") return suggestion.state === "proposed";
  return suggestion.state !== "proposed";
}
