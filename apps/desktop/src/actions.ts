// The delegated action dispatcher.
//
// Every UI gesture in OpenDoc names an action — a `data-action` attribute, a
// keyboard shortcut, a dialog choice — and every one of them arrives here.
// That is what lets the whole app get by with a single click listener on the
// never-replaced `app` element (see `bindStatic` in `main.ts`) instead of a
// listener per control that a re-render could stack.
//
// Actions that belong to one surface are routed to that surface's own handler,
// which returns false for a name it does not own; what is left below is the
// shared vocabulary — panels, marks, styles, insertion, citations, zoom.
import { confirmDialog, promptDialog, toast } from "./ui";
import { fetchUrlFile, invoke, openFile } from "./invoke";
import type { CommentFilter, ListKindName, Panel, SuggestionFilter } from "./state";
import { matchesCommentFilter, matchesSuggestionFilter, runtime, state } from "./state";
import {
  ACTION_CANCELLED,
  applyDocument,
  bytesFromBase64,
  describeEditorSelection,
  edit,
  findBlock,
  focusBlock,
  focusInline,
  native,
  query,
  requireBlock,
  run,
  selectionText,
  showError,
  textFromBase64,
  wordStats,
} from "./shared";
import { editorHooks, renderAll, renderMain, renderStatus } from "./shell";
import { MENUS } from "./menus";
import { applyMark, renderToolbar, setBlockAlignment, setBlockDirection, setBlockStyle, setLineSpacing, toggleKeepWithNext, toggleParagraphBorder } from "./toolbar";
import { closeFind, openCurrentFindMatchRegion, openFind, replaceFromFindBar, stepFind } from "./find";
import { promptPageFurniture } from "./pagination";
import { renderSidePanel } from "./panels";
import { loadVersions, runVersionAction } from "./versions";
import { runTableAction } from "./tables";
import {
  clearImagePosition,
  focusImageBlock,
  promptImagePosition,
  promptImageProperties,
  promptImageSize,
  replaceFocusedImage,
  saveFocusedImage,
} from "./images";
import { runFileAction } from "./files";
import { runSpreadsheetAction } from "./spreadsheet";
import { openSharingDialog } from "./collab";

/**
 * Every UI action funnels through here. The wrapper exists so that declining
 * the unsaved-work prompt stops the action quietly instead of surfacing as a
 * rejected promise at whichever listener started it.
 */
export async function runAction(action: string, data: DOMStringMap = {}): Promise<void> {
  try {
    await performAction(action, data);
  } catch (error) {
    if (error === ACTION_CANCELLED) return;
    // The last resort, because there is nowhere else for this to go: actions
    // are launched as `void runAction(...)` from the delegated click listener,
    // so anything that escapes here is an unhandled rejection in the console
    // and nothing on screen. `run()` and `native()` in `shared.ts` have already
    // reported what they know about — re-reporting is the same sentence
    // twice, which costs nothing — and what is left is the failure no layer
    // claimed, which the user is still entitled to see.
    console.error(`action ${action} failed`, error);
    showError(error instanceof Error ? error.message : String(error));
  }
}

/** Apply a common bullet marker to the list item selected when its control
 * opened. Native selects may move focus before their later change event, so
 * toolbar callers can provide that preserved selection rather than letting a
 * newly focused list receive the choice. */
export async function setBulletMarkerPreset(marker: string, selection = state.selection): Promise<void> {
  if (state.mode === "sheets") return;
  if (!(["disc", "circle", "square"] as string[]).includes(marker)) {
    throw new Error(`Unsupported bullet-marker preset ${marker}.`);
  }
  const block = selection ? findBlock(selection.focus.block_id) : null;
  if (block?.kind !== "list-item" || block.list_kind !== "bullet") {
    toast("Place the caret in a bulleted list item first.");
    return;
  }
  await edit("set_bullet_list_marker", { blockId: block.id, marker });
}

/**
 * List dialogs deliberately keep operating on the run/level the reader
 * opened, even if browser focus moves while a native modal is up. A remote
 * conversion or deletion can make that captured item mean something else,
 * though. Re-resolve its durable identity before writing, rather than asking
 * the command layer to reject stale ids after the user supplied an answer.
 */
function liveListDialogTarget(block: ReturnType<typeof focusBlock>, kind: ListKindName): typeof block | null {
  if (!block || block.kind !== "list-item" || block.list_kind !== kind || block.list_id == null || block.level == null) return null;
  const live = findBlock(block.id);
  if (
    live?.kind !== "list-item" ||
    live.list_kind !== kind ||
    live.list_id !== block.list_id ||
    live.level !== block.level
  ) {
    showError("That list changed while its settings were open. Place the caret in it and try again.");
    return null;
  }
  return live;
}

async function performAction(action: string, data: DOMStringMap = {}): Promise<void> {
  if (action === "outline-go") {
    const blockId = data.id;
    if (!blockId) return;
    // A panel normally rerenders as soon as a concurrent edit removes or
    // restyles its heading. A delegated click already queued against the old
    // panel, however, can arrive after that render. Revalidate the durable
    // source fact instead of silently scrolling nowhere (or to a block which
    // is no longer an outline item).
    const heading = findBlock(blockId);
    if (!heading || heading.kind !== "heading") {
      showError("This outline heading was deleted or changed. Refresh the outline before navigating.");
      return;
    }
    const target = document.querySelector<HTMLElement>(`[data-block-id="${CSS.escape(blockId)}"]`);
    if (!target) {
      showError("This outline heading is not currently rendered. Refresh the outline before navigating.");
      return;
    }
    target.scrollIntoView({ block: "center", behavior: "smooth" });
    return;
  }
  if (action === "bookmark-go") {
    const blockId = data.blockId;
    if (!blockId) return;
    if (!findBlock(blockId)) {
      showError("This bookmark's target was deleted. Remove the bookmark or restore its target before navigating.");
      return;
    }
    const target = document.querySelector<HTMLElement>(`[data-block-id="${CSS.escape(blockId)}"]`);
    // The source target can survive a remote projection while its fragment is
    // temporarily absent during a keyed editor morph. Do not make a live
    // bookmark click look successful when the browser has nowhere to scroll.
    if (!target) {
      showError("This bookmark's target is not currently rendered. Refresh the document before navigating.");
      return;
    }
    target.scrollIntoView({ block: "center", behavior: "smooth" });
    return;
  }
  if (action === "bookmark-delete") {
    const bookmarkId = data.bookmarkId;
    const bookmark = state.doc?.bookmarks.find((item) => item.id === bookmarkId && !item.deleted);
    if (!bookmark) return;
    const accepted = await confirmDialog(
      "Delete bookmark?",
      `Delete the bookmark \"${bookmark.name}\"? Links to it will stop resolving.`,
      "Delete",
    );
    if (accepted) await edit("delete_bookmark", { bookmarkId: bookmark.id });
    return;
  }
  if (action === "comment-go") {
    navigateToReviewAnchor(data.anchor, data.id);
    return;
  }
  if (action === "suggestion-go") {
    if (!data.anchor) return;
    // Historical suggestions are navigable source evidence, but they are not
    // an open review target. Do not make the proposed-only queue look as if
    // it selected a resolved card.
    if (data.state === "proposed") state.activeSuggestionId = data.id || null;
    if (state.panel === "suggestions") renderSidePanel();
    navigateToReviewAnchor(data.anchor);
    return;
  }
  if (action === "edit-suggestion") {
    const suggestion = state.doc?.suggestions.find((item) =>
      item.id === data.id && item.state === "proposed" && item.kind === "insert",
    );
    if (!suggestion) {
      // A panel click can race a remote resolution or replacement.  The
      // insert-content operation is deliberately narrow, so never use a
      // cached card body to revive or retarget a now-ineligible proposal.
      toast("This insert suggestion is no longer open.");
      return;
    }
    const result = await promptDialog({
      title: "Edit suggested text",
      body: "This changes the open proposal only; source text remains unchanged until it is accepted.",
      fields: [{ name: "text", label: "Suggested text", type: "textarea", value: suggestion.text }],
      submit: "Save proposal",
    });
    if (!result) return;
    const revised = result.text;
    // Do not journal a no-op merely because a reviewer opened and confirmed
    // the editor. Re-resolve the durable record after the modal as a remote
    // reviewer could have resolved it while it was open.
    const live = state.doc?.suggestions.find((item) =>
      item.id === suggestion.id && item.state === "proposed" && item.kind === "insert",
    );
    if (!live) {
      toast("This insert suggestion is no longer open.");
      return;
    }
    if (revised === live.text) return;
    await edit("update_suggestion", { suggestionId: live.id, text: revised });
    return;
  }
  if (action === "comment-history-go") {
    if (!data.threadId || !data.commentId) return;
    // History is immutable evidence, but a reviewer should be able to return
    // from an event to the current live comment it describes. A deleted
    // comment intentionally has no target rather than being resurrected or
    // guessed from a stale body string.
    // Do not interpolate a document-supplied stable id into a CSS selector:
    // stable ids are valid model data, not necessarily CSS identifiers, and
    // the history jump should never fail merely because an importer used a
    // punctuation-bearing id. Compare the already-rendered data attributes.
    const thread = [...document.querySelectorAll<HTMLElement>("[data-thread]")]
      .find((candidate) => candidate.dataset.thread === data.threadId);
    const target = thread
      ? [...thread.querySelectorAll<HTMLElement>("[data-comment-id]")]
        .find((candidate) => candidate.dataset.commentId === data.commentId)
      : undefined;
    target?.scrollIntoView({ block: "nearest", behavior: "smooth" });
    target?.focus({ preventScroll: true });
    return;
  }
  if (action === "comment-activity-go") {
    if (!data.threadId) return;
    // Unlike per-thread body history, the document-wide activity list can be
    // read while Open or For you hides the source conversation. Move to All
    // only after resolving the durable target, so a remote delete cannot make
    // a stale activity entry select an unrelated current card.
    const liveThread = state.doc?.comments.find((thread) =>
      thread.id === data.threadId && !thread.deleted,
    );
    if (!liveThread) {
      toast("This activity target is no longer available.");
      return;
    }
    const liveComment = data.commentId
      ? liveThread.comments.find((comment) => comment.id === data.commentId && !comment.deleted)
      : undefined;
    state.commentFilter = "all";
    state.activeCommentThreadId = liveThread.id;
    if (state.activeCommentReplyThreadId !== liveThread.id) state.activeCommentReplyThreadId = null;
    renderSidePanel();
    // Compare attributes rather than constructing a selector from imported
    // stable ids. This keeps a valid punctuation-bearing id navigable.
    const thread = [...document.querySelectorAll<HTMLElement>("[data-thread]")]
      .find((candidate) => candidate.dataset.thread === liveThread.id);
    const target = liveComment && thread
      ? [...thread.querySelectorAll<HTMLElement>("[data-comment-id]")]
        .find((candidate) => candidate.dataset.commentId === liveComment.id)
      : thread;
    target?.scrollIntoView({ block: "nearest", behavior: "smooth" });
    target?.focus({ preventScroll: true });
    return;
  }
  if (action === "comment-next" || action === "comment-previous") {
    navigateCommentThread(action === "comment-next" ? 1 : -1);
    return;
  }
  if (action === "suggestion-next" || action === "suggestion-previous") {
    navigateSuggestion(action === "suggestion-next" ? 1 : -1);
    return;
  }
  if (action.startsWith("comment-filter:")) {
    const filter = action.slice("comment-filter:".length);
    if (filter === "open" || filter === "resolved" || filter === "for-you" || filter === "all") {
      state.commentFilter = filter as CommentFilter;
      // The current review target is local view state, but it must never name
      // a card the newly selected filter does not render.  Clearing it makes
      // the next/previous starting point deterministic instead of retaining
      // an invisible, stale thread id.
      if (!state.doc?.comments.some((thread) =>
        thread.id === state.activeCommentThreadId
        && matchesCommentFilter(thread, state.commentFilter, state.authorName),
      )) {
        state.activeCommentThreadId = null;
      }
      renderSidePanel();
    }
    return;
  }
  if (action.startsWith("suggestion-filter:")) {
    const filter = action.slice("suggestion-filter:".length);
    if (filter === "proposed" || filter === "resolved" || filter === "all") {
      state.suggestionFilter = filter as SuggestionFilter;
      // The local active id names the open review queue only. It cannot remain
      // current in a history-only view with no available review actions.
      if (!state.doc?.suggestions.some((suggestion) =>
        suggestion.id === state.activeSuggestionId
        && suggestion.state === "proposed"
        && matchesSuggestionFilter(suggestion, state.suggestionFilter),
      )) {
        state.activeSuggestionId = null;
      }
      renderSidePanel();
    }
    return;
  }
  if (action === "close-panel") {
    state.panel = null;
    renderSidePanel();
    return;
  }
  if (action.startsWith("toggle-panel:")) {
    const which = action.slice("toggle-panel:".length) as Panel;
    state.panel = state.panel === which ? null : which;
    renderSidePanel();
    if (state.panel === "versions") void loadVersions();
    return;
  }
  if (action.startsWith("version:")) {
    await runVersionAction(action.slice("version:".length), data);
    return;
  }
  if (action.startsWith("mark:")) {
    if (state.mode === "sheets") return;
    await applyMark(action.slice(5), null, "toggle");
    return;
  }
  if (action.startsWith("mark-remove:")) {
    if (state.mode === "sheets") return;
    await applyMark(action.slice("mark-remove:".length), null, "remove");
    return;
  }
  if (action.startsWith("style:")) {
    const parts = action.split(":");
    if (parts[1] === "heading") await setBlockStyle("heading", Number(parts[2] ?? 1), "bullet");
    else if (parts[1] === "title" || parts[1] === "subtitle") await setBlockStyle(parts[1], 0, "bullet");
    else if (parts[1] === "list") await setBlockStyle("list-item", focusBlock()?.level ?? 0, (parts[2] ?? "bullet") as ListKindName);
    else await setBlockStyle("paragraph", 0, "bullet");
    return;
  }
  if (action.startsWith("align:")) {
    if (state.mode === "sheets") return;
    await setBlockAlignment(action.slice("align:".length));
    return;
  }
  if (action.startsWith("direction:")) {
    if (state.mode === "sheets") return;
    await setBlockDirection(action.slice("direction:".length));
    return;
  }
  if (action === "keep-with-next") {
    if (state.mode === "sheets") return;
    await toggleKeepWithNext();
  } else if (action === "paragraph-first-line-indent") {
    if (state.mode === "sheets" || !state.selection) return;
    const selection = state.selection;
    const currentTwips = focusBlock()?.properties?.indent_first_line_twips ?? 0;
    const result = await promptDialog({
      title: "First-line indent",
      fields: [{
        name: "points",
        label: "Points (negative for hanging indent)",
        type: "number",
        step: "0.5",
        value: String(currentTwips / 20),
      }],
      submit: "Apply",
    });
    if (!result) return;
    const points = Number(result.points);
    if (!Number.isFinite(points)) {
      showError("First-line indent must be a number of points.");
      return;
    }
    await edit("set_editor_selection_block_indent_first_line", {
      selection,
      twips: Math.round(points * 20),
    });
    state.editor?.setSelection(selection);
    state.editor?.focus();
    return;
  } else if (action === "paragraph-border") {
    await toggleParagraphBorder();
    return;
  }
  if (action.startsWith("image-placement:")) {
    if (state.mode === "sheets") return;
    const block = focusImageBlock();
    if (block) await edit("set_image_block_placement", { blockId: block.id, placement: action.slice("image-placement:".length) });
    return;
  }
  if (action === "image-reset-size") {
    const block = focusImageBlock();
    if (block) await edit("clear_image_block_size", { blockId: block.id });
    return;
  }
  if (action === "image-size") {
    await promptImageSize();
    return;
  }
  if (action === "image-position") {
    if (state.mode === "docs") await promptImagePosition();
    return;
  }
  if (action === "image-clear-position") {
    if (state.mode === "docs") await clearImagePosition();
    return;
  }
  if (action === "save-image") {
    await saveFocusedImage();
    return;
  }
  if (action === "image-properties") {
    await promptImageProperties();
    return;
  }
  if (action === "replace-image") {
    await replaceFocusedImage();
    return;
  }
  if (action.startsWith("line-spacing:")) {
    await setLineSpacing(action.slice("line-spacing:".length));
    return;
  }
  if (action.startsWith("page-furniture:")) {
    if (state.mode === "sheets") return;
    await promptPageFurniture(action.slice("page-furniture:".length));
    return;
  }
  if (action === "toggle-checklist-item") {
    if (state.documentEditingMode === "view") return;
    // The renderer stamps the model's state on the wrapper, so the new value
    // is read from the document projection rather than from the checkbox the
    // user just clicked — the input is decoration, the document is the fact.
    const blockId = data?.checklistBlockId;
    if (!blockId) return;
    await edit("set_list_item_checked", { blockId, checked: data?.checked !== "true" });
    return;
  }
  if (action === "list-start") {
    if (state.mode === "sheets") return;
    const block = focusBlock();
    if (block?.kind !== "list-item" || block.list_kind !== "ordered" || block.list_id == null || block.level == null) {
      toast("Place the caret in a numbered list item first.");
      return;
    }
    const current = state.doc?.list_properties?.[block.list_id]?.ordered_starts?.[String(block.level)] ?? 1;
    const result = await promptDialog({
      title: "Set numbering start",
      body: "This changes the start of the whole numbered-list run at this nesting level. Use 1 to restore the default.",
      fields: [{ name: "start", label: "Start at", type: "number", step: "1", value: String(current) }],
      submit: "Set start",
    });
    if (!result) return;
    const start = Number(result.start);
    if (!Number.isSafeInteger(start) || start < 1 || start > 0xffff_ffff) {
      throw new Error("Numbering start must be a whole number from 1 to 4,294,967,295.");
    }
    const live = liveListDialogTarget(block, "ordered");
    if (!live) return;
    await edit("set_ordered_list_start", { blockId: live.id, start });
    return;
  }
  if (action === "list-format") {
    if (state.mode === "sheets") return;
    const block = focusBlock();
    if (block?.kind !== "list-item" || block.list_kind !== "ordered" || block.list_id == null || block.level == null) {
      toast("Place the caret in a numbered list item first.");
      return;
    }
    const current = state.doc?.list_properties?.[block.list_id]?.ordered_formats?.[String(block.level)] ?? "inherited";
    const result = await promptDialog({
      title: "Set numbering format",
      body: "This changes the counter style for the whole numbered-list run at this nesting level.",
      fields: [{
        name: "format", label: "Format", type: "select", value: current,
        options: [
          { value: "inherited", label: "Inherited depth cycle" },
          { value: "decimal", label: "1, 2, 3" },
          { value: "lower-alpha", label: "a, b, c" },
          { value: "upper-alpha", label: "A, B, C" },
          { value: "lower-roman", label: "i, ii, iii" },
          { value: "upper-roman", label: "I, II, III" },
        ],
      }],
      submit: "Set format",
    });
    if (!result) return;
    const live = liveListDialogTarget(block, "ordered");
    if (!live) return;
    const inherited = ["decimal", "lower-alpha", "lower-roman"][Number(live.level) % 3];
    await edit("set_ordered_list_format", { blockId: live.id, format: result.format === "inherited" ? inherited : result.format });
    return;
  }
  if (action === "list-bullet-marker") {
    if (state.mode === "sheets") return;
    const block = focusBlock();
    if (block?.kind !== "list-item" || block.list_kind !== "bullet" || block.list_id == null || block.level == null) {
      toast("Place the caret in a bulleted list item first.");
      return;
    }
    const current = state.doc?.list_properties?.[block.list_id]?.bullet_markers?.[String(block.level)] ?? "inherited";
    const result = await promptDialog({
      title: "Set bullet marker",
      body: "This changes the glyph for the whole bulleted-list run at this nesting level. Use disc, circle, or square, or enter up to 16 safe Unicode glyph characters.",
      fields: [{ name: "marker", label: "Marker", type: "text", value: current }],
      submit: "Set marker",
    });
    if (!result) return;
    const live = liveListDialogTarget(block, "bullet");
    if (!live) return;
    await edit("set_bullet_list_marker", { blockId: live.id, marker: result.marker });
    return;
  }
  if (action.startsWith("list-bullet-marker:")) {
    const marker = action.slice("list-bullet-marker:".length);
    await setBulletMarkerPreset(marker);
    return;
  }

  // Surfaces that own their own state answer first; each returns false for a
  // name it does not own, so an unknown action still reaches the branch below
  // that says so.
  if (await runFileAction(action, data)) return;
  if (await runSpreadsheetAction(action, data)) return;
  switch (action) {
    case "go-home":
      state.view = "home";
      renderAll();
      break;
    case "mode-docs":
    case "mode-sheets": {
      state.mode = action === "mode-docs" ? "docs" : "sheets";
      const main = query("[data-main]");
      if (main) main.innerHTML = "";
      renderAll();
      break;
    }
    case "dismiss-error":
      state.lastError = null;
      renderStatus();
      break;
    case "undo":
    case "redo":
      await stepHistory(action);
      break;
    case "select-all": {
      if (!state.doc || state.mode !== "docs") return;
      const result = await invoke("select_all_editor_content");
      editorHooks.onResult(result);
      break;
    }
    case "find":
      await openFind();
      break;
    case "find-close":
      closeFind();
      break;
    case "find-next":
    case "find-prev":
      stepFind(action === "find-next" ? 1 : -1);
      break;
    case "find-open-region":
      await openCurrentFindMatchRegion();
      break;
    case "replace-one":
    case "replace-all":
      await replaceFromFindBar(action === "replace-all");
      break;
    case "comment": {
      const context = await describeEditorSelection();
      const range = context?.inline_range ?? null;
      const target = context?.focus_block_id ? findBlock(context.focus_block_id) : requireBlock();
      if (!target) return;
      const result = await promptDialog({ title: "Add comment", fields: [{ name: "body", label: "Comment", type: "textarea" }], submit: "Comment" });
      if (!result?.body.trim()) return;
      if (range) await edit("add_text_range_comment", { startInlineId: range.start, endInlineId: range.end, author: state.authorName, body: result.body });
      else await edit("add_block_comment", { blockId: target.id, author: state.authorName, body: result.body });
      state.panel = "comments";
      renderSidePanel();
      break;
    }
    case "reply-comment": {
      if (data.id && state.documentEditingMode !== "view") {
        state.activeCommentReplyThreadId = data.id;
        renderSidePanel();
        // The panel was just rebuilt, so focus only after its form exists.
        query<HTMLTextAreaElement>("[data-comment-reply-form] textarea")?.focus();
      }
      break;
    }
    case "cancel-comment-reply": {
      if (data.id && state.activeCommentReplyThreadId === data.id) {
        state.activeCommentReplyThreadId = null;
        renderSidePanel();
      }
      break;
    }
    case "submit-comment-reply": {
      const body = data.body?.trim();
      if (!data.id || state.documentEditingMode === "view") break;
      // A delegated form submit can be queued just as a remote projection
      // deletes or hides its thread. The panel clears the composer on that
      // projection, but the old form event still exists. Do not send a reply
      // command for a target the current review projection no longer exposes.
      const threadIsVisible = state.doc?.comments.some((thread) =>
        thread.id === data.id && matchesCommentFilter(thread, state.commentFilter, state.authorName),
      ) ?? false;
      if (!threadIsVisible) {
        if (state.activeCommentReplyThreadId === data.id) state.activeCommentReplyThreadId = null;
        renderSidePanel();
        toast("This comment thread is no longer available.");
        break;
      }
      if (!body) {
        toast("Reply cannot be empty.");
        break;
      }
      // `run` applies the authoritative projection and reports a service
      // refusal. Only close the composer after the mutation actually landed;
      // a rejected session-role or identity check must not discard the reply.
      try {
        await run("add_comment_reply", { threadId: data.id, author: state.authorName, body });
        state.activeCommentReplyThreadId = null;
        renderSidePanel();
      } catch {
        // run() already put the backend reason in the visible error region.
      }
      break;
    }
    case "edit-comment": {
      if (!data.id || !data.commentId || state.documentEditingMode === "view") break;
      const thread = state.doc?.comments.find((item) => item.id === data.id && !item.deleted);
      const comment = thread?.comments.find((item) => item.id === data.commentId && !item.deleted);
      if (!comment) break;
      const result = await promptDialog({
        title: "Edit comment",
        fields: [{ name: "body", label: "Comment", type: "textarea", value: comment.body }],
        submit: "Save",
      });
      const body = result?.body.trim();
      if (!body) {
        if (result) toast("Comment cannot be empty.");
        break;
      }
      // Use `run`, not the fire-and-forget convenience wrapper: a role or
      // service-authority refusal must leave the editor's existing comment on
      // screen rather than looking like a successful local edit.
      await run("update_comment", { threadId: data.id, commentId: data.commentId, body });
      break;
    }
    case "delete-comment": {
      if (!data.id || !data.commentId || state.documentEditingMode === "view") break;
      const thread = state.doc?.comments.find((item) => item.id === data.id && !item.deleted);
      const comment = thread?.comments.find((item) => item.id === data.commentId && !item.deleted);
      if (!comment) break;
      const confirmed = await confirmDialog(
        "Delete comment?",
        "The comment will be removed from this conversation, but its signed review history remains available.",
        "Delete",
      );
      if (!confirmed) break;
      await run("delete_comment", { threadId: data.id, commentId: data.commentId });
      break;
    }
    case "restore-comment": {
      if (!data.id || !data.commentId || state.documentEditingMode === "view") break;
      // The comment stays hidden from the live conversation until the durable
      // restore operation succeeds. `run` keeps a concurrent service refusal
      // visible and leaves the immutable audit trail intact.
      await run("restore_comment", { threadId: data.id, commentId: data.commentId });
      break;
    }
    case "resolve-comment":
      if (data.id) await edit("resolve_comment_thread", { threadId: data.id, resolvedBy: state.authorName });
      break;
    case "reopen-comment":
      if (data.id) await edit("reopen_comment_thread", { threadId: data.id });
      break;
    case "comment-action": {
      if (!data.id) break;
      const thread = state.doc?.comments.find((item) => item.id === data.id);
      if (!thread) break;
      // The durable operation already owns validation, provenance and service
      // authorization. This dialog only exposes its complete tuple: assignment
      // and due date used to become uneditable as soon as an item existed.
      const result = await promptDialog({
        title: thread.action_assignee ? "Manage action item" : "Assign action item",
        body: "Leave assignee blank to remove the action item.",
        fields: [
          { name: "assignee", label: "Assignee", type: "text", value: thread.action_assignee ?? "" },
          {
            name: "dueDate",
            label: "Due date",
            type: "date",
            // Dates are stored as a UTC instant at midnight. Formatting in UTC
            // avoids turning a saved due day into the previous local day.
            value: thread.action_due_at_ms ? new Date(thread.action_due_at_ms).toISOString().slice(0, 10) : "",
          },
          {
            name: "status",
            label: "Status",
            type: "select",
            value: thread.action_completed_by ? "complete" : "open",
            options: [{ value: "open", label: "Open" }, { value: "complete", label: "Complete" }],
          },
        ],
        submit: "Save action",
      });
      if (!result) break;
      const assignee = result.assignee.trim() || null;
      if (result.dueDate && !assignee) {
        toast("An assignee is required when an action item has a due date.");
        break;
      }
      const dueAtMs = result.dueDate ? Date.parse(`${result.dueDate}T00:00:00.000Z`) : null;
      // A panel modal can outlive a remote thread deletion. The values the
      // reader entered belong to this durable review target, not to an
      // arbitrary replacement or a command the core must reject afterwards.
      const liveThread = state.doc?.comments.find((item) => item.id === data.id && !item.deleted);
      if (!liveThread) {
        toast("This comment was deleted while its action item was open. Reopen a live comment to manage its action.");
        break;
      }
      const completed = assignee !== null && result.status === "complete";
      await edit("set_comment_thread_action", {
        threadId: data.id,
        assignee,
        dueAtMs,
        completed,
        completedBy: completed ? (liveThread.action_completed_by ?? state.authorName) : null,
      });
      break;
    }
    case "comment-reaction": {
      if (!data.id || !data.emoji || state.documentEditingMode === "view") break;
      await edit("set_comment_thread_reaction", {
        threadId: data.id,
        emoji: data.emoji,
        actor: state.authorName,
        present: data.present === "true",
      });
      break;
    }
    case "suggest": {
      const range = (await describeEditorSelection())?.inline_range ?? null;
      if (!range) {
        showError("Select the text to replace first.");
        return;
      }
      const result = await promptDialog({ title: "Suggest replacement", fields: [{ name: "text", label: "Replacement text", type: "textarea" }], submit: "Suggest" });
      if (result?.text) await edit("add_text_range_suggestion", { startInlineId: range.start, endInlineId: range.end, author: state.authorName, text: result.text });
      state.panel = "suggestions";
      renderSidePanel();
      break;
    }
    case "suggest-delete": {
      const range = (await describeEditorSelection())?.inline_range ?? null;
      if (!range) {
        showError("Select the text to delete first.");
        return;
      }
      await edit("add_text_range_delete_suggestion", { startInlineId: range.start, endInlineId: range.end, author: state.authorName });
      state.panel = "suggestions";
      renderSidePanel();
      break;
    }
    case "suggest-block-delete": {
      const block = focusBlock();
      if (!block) {
        showError("Place the cursor in the block to propose deleting first.");
        return;
      }
      const confirmed = await confirmDialog(
        "Suggest block deletion",
        "This will create a review proposal; it will not delete the block until someone accepts it.",
        "Suggest deletion",
      );
      if (!confirmed) return;
      await edit("add_block_delete_suggestion", { blockId: block.id, author: state.authorName });
      state.panel = "suggestions";
      renderSidePanel();
      break;
    }
    case "suggest-block-insert": {
      const block = focusBlock();
      if (!block) { showError("Place the cursor in the paragraph to insert after first."); return; }
      const result = await promptDialog({ title: "Suggest paragraph insertion", fields: [{ name: "text", label: "New paragraph", type: "textarea" }], submit: "Suggest insertion" });
      if (!result?.text) return;
      await edit("add_block_insert_suggestion", { blockId: block.id, author: state.authorName, text: result.text });
      state.panel = "suggestions"; renderSidePanel();
      break;
    }
    case "suggest-block-replace": {
      const block = focusBlock();
      if (!block) { showError("Place the cursor in the plain paragraph to replace first."); return; }
      const result = await promptDialog({ title: "Suggest paragraph replacement", fields: [{ name: "text", label: "Replacement paragraph", type: "textarea" }], submit: "Suggest replacement" });
      if (!result?.text) return;
      await edit("add_block_replace_suggestion", { blockId: block.id, author: state.authorName, text: result.text });
      state.panel = "suggestions"; renderSidePanel();
      break;
    }
    case "accept-suggestion":
      if (data.id && openSuggestionStillVisible(data.id)) {
        await edit("accept_suggestion", { suggestionId: data.id, acceptedBy: state.authorName });
      }
      break;
    case "preview-suggestion": {
      if (!data.id || (data.resolution !== "accept" && data.resolution !== "reject")) break;
      if (!openSuggestionStillVisible(data.id)) break;
      const html = await run("render_suggestion_preview_html", { suggestionId: data.id, resolution: data.resolution });
      showSuggestionPreview(data.resolution, String(html));
      break;
    }
    case "reject-suggestion":
      if (data.id && openSuggestionStillVisible(data.id)) {
        await edit("reject_suggestion", { suggestionId: data.id, rejectedBy: state.authorName });
      }
      break;
    case "accept-all":
      if (!(await confirmBulkSuggestionResolution("accept"))) return;
      await edit("accept_all_suggestions", { acceptedBy: state.authorName });
      break;
    case "reject-all":
      if (!(await confirmBulkSuggestionResolution("reject"))) return;
      await edit("reject_all_suggestions", { rejectedBy: state.authorName });
      break;
    case "insert-table": {
      const target = requireBlock();
      if (!target) return;
      const result = await promptDialog({ title: "Insert table", fields: [{ name: "rows", label: "Rows", type: "number", step: "1", value: "3" }, { name: "columns", label: "Columns", type: "number", step: "1", value: "3" }], submit: "Insert" });
      if (!result) return;
      await edit("insert_table_after", { afterBlockId: target.id, rows: Number(result.rows) || 2, columns: Number(result.columns) || 2 });
      break;
    }
    case "insert-page-break": {
      const target = requireBlock();
      if (target) await edit("insert_page_break_after", { afterBlockId: target.id });
      break;
    }
    case "insert-horizontal-rule": {
      const target = requireBlock();
      if (target) await edit("insert_horizontal_rule_after", { afterBlockId: target.id });
      break;
    }
    case "insert-table-of-contents": {
      const target = requireBlock();
      if (target) await edit("insert_table_of_contents_after", { afterBlockId: target.id });
      break;
    }
    case "insert-bibliography": {
      const target = requireBlock();
      if (target) await edit("insert_bibliography_after", { afterBlockId: target.id });
      break;
    }
    case "table-insert-column-right":
    case "table-insert-column-left":
    case "table-delete-column":
    case "table-insert-row-below":
    case "table-insert-row-above":
    case "table-delete-row":
    case "table-column-width":
    case "table-column-width-auto":
    case "table-row-height":
    case "table-row-height-auto":
    case "table-toggle-header":
    case "table-sort-ascending":
    case "table-sort-descending":
    case "table-border":
    case "table-border-inherit":
    case "table-align-start":
    case "table-align-center":
    case "table-align-end":
    case "table-align-inherit":
    case "table-merge-cells":
    case "table-split-cell":
    case "table-move-cell-block-up":
    case "table-move-cell-block-down":
    case "table-move-cell-block-previous-cell":
    case "table-move-cell-block-next-cell":
    case "table-cell-background":
    case "table-cell-border":
    case "table-cell-vertical-align":
    case "table-cell-row-header":
    case "table-cell-padding":
    case "table-cell-clear-style":
      await runTableAction(action);
      break;
    case "insert-link": {
      const target = requireBlock();
      if (!target) return;
      const inline = focusInline();
      const existing = inline?.kind === "link" ? inline : null;
      const selectedText = selectionText();
      const result = await promptDialog({
        title: existing ? "Edit link" : "Insert link",
        fields: [
          { name: "text", label: "Text", value: existing?.text ?? selectedText },
          { name: "href", label: "Link", value: existing?.href ?? "", placeholder: "https://" },
        ],
        submit: existing ? "Update" : "Insert",
      });
      if (!result) return;
      if (existing) await edit("update_link_href", { inlineId: existing.id, href: result.href });
      else if (selectedText && state.selection) await applyMark("link", result.href, "set");
      else await edit("insert_link_after", { blockId: target.id, afterInlineId: state.selection?.focus.inline_id ?? null, text: result.text || result.href, href: result.href });
      break;
    }
    case "insert-footnote": {
      const target = requireBlock();
      if (target) await edit("insert_footnote_ref_after", { blockId: target.id, afterInlineId: state.selection?.focus.inline_id ?? null });
      state.panel = "footnotes";
      renderSidePanel();
      break;
    }
    case "insert-endnote": {
      const target = requireBlock();
      if (target) await edit("insert_endnote_ref_after", { blockId: target.id, afterInlineId: state.selection?.focus.inline_id ?? null });
      state.panel = "footnotes";
      renderSidePanel();
      break;
    }
    case "edit-footnote": {
      const note = state.doc?.footnotes.find((item) => item.id === data.id);
      const result = await promptDialog({ title: "Edit footnote", fields: [{ name: "body", label: "Footnote text", type: "textarea", value: note?.body.map((inline) => inline.text).join("") ?? "" }], submit: "Save" });
      if (result && data.id) await edit("update_footnote_body", { footnoteId: data.id, body: result.body });
      break;
    }
    case "insert-equation":
    case "insert-equation-block": {
      const target = requireBlock();
      if (!target) return;
      const result = await promptDialog({ title: "Insert equation", fields: [{ name: "source", label: "LaTeX", placeholder: "\\frac{a}{b}" }], submit: "Insert" });
      if (!result?.source) return;
      if (action === "insert-equation") await edit("insert_equation_after", { blockId: target.id, afterInlineId: state.selection?.focus.inline_id ?? null, source: result.source });
      else await edit("insert_equation_block_after", { afterBlockId: target.id, source: result.source });
      break;
    }
    case "insert-mention": {
      const target = requireBlock();
      if (!target) return;
      const result = await promptDialog({ title: "Mention", fields: [{ name: "label", label: "Name", value: "@" }], submit: "Insert" });
      if (result?.label) await edit("insert_mention_after", { blockId: target.id, afterInlineId: state.selection?.focus.inline_id ?? null, label: result.label });
      break;
    }
    case "insert-date-chip": {
      const target = requireBlock();
      if (!target) return;
      const result = await promptDialog({
        title: "Insert date",
        body: "The date is stored as a calendar value, so it stays the same for every collaborator.",
        fields: [{ name: "date", label: "Date", type: "date" }],
        submit: "Insert",
      });
      if (result?.date) await edit("insert_date_chip_after", { blockId: target.id, afterInlineId: state.selection?.focus.inline_id ?? null, date: result.date });
      break;
    }
    case "insert-bookmark": {
      const target = requireBlock();
      if (!target) return;
      const result = await promptDialog({
        title: "Add bookmark",
        body: "Bookmarks name this whole block. Use letters, digits, hyphens, or underscores; the first character must be a letter or underscore.",
        fields: [{ name: "name", label: "Bookmark name" }],
        submit: "Save bookmark",
      });
      const name = result?.name.trim();
      if (!name) return;
      // Naming an existing bookmark again deliberately retargets that stable
      // record instead of manufacturing a competing live name. The merge
      // layer still resolves a concurrent same-name write deterministically.
      const existing = state.doc?.bookmarks.find((bookmark) => !bookmark.deleted && bookmark.name === name);
      await edit("set_bookmark", { bookmarkId: existing?.id ?? null, name, blockId: target.id });
      break;
    }
    case "insert-image":
    case "attach-file": {
      // Two native calls behind one function (the dialog, then the read), and
      // either can be refused; `null` is the user closing the dialog.
      const file = await native(action === "insert-image" ? "Could not open that image" : "Could not open that file", () =>
        openFile(action === "insert-image" ? ["png", "jpg", "jpeg", "gif", "webp", "svg"] : ["*"]),
      );
      if (!file) return;
      const updated = await run("add_binary_blob", { name: file.name, mediaType: file.media_type, bytes: bytesFromBase64(file.base64) });
      const blob = updated.blobs.find((item) => item.name === file.name);
      const target = requireBlock();
      // A file name identifies an attachment, not what its pixels mean. Keep
      // it on the blob for the Files panel and raw save, but do not turn it
      // into author-provided alternative text (or an image hover title).
      if (action === "insert-image" && blob && target) await edit("insert_image_block_after", { afterBlockId: target.id, blobHash: blob.hash, altText: "" });
      if (action === "attach-file") {
        state.panel = "files";
        renderSidePanel();
      }
      break;
    }
    case "insert-image-url": {
      const target = requireBlock();
      if (!target) return;
      const answer = await promptDialog({
        title: "Insert image by URL",
        fields: [{ name: "url", label: "Image URL", placeholder: "https://" }],
        submit: "Insert",
      });
      const url = answer?.url.trim();
      if (!url) return;
      // The shell fetches; the document commands are the same two an attached
      // or dropped picture goes through, so nothing new reaches the model.
      //
      // Two failures, two messages: a rejection is *this* download failing
      // (an unreachable host, a refused scheme, a body over the cap, or the
      // shell command not being granted at all) and `native` reports it with
      // the shell's own reason, while `null` is the runtime saying it has no
      // network of its own — the browser build's answer, and not a failure.
      const file = await native("Could not download that image", () => fetchUrlFile(url));
      if (!file) {
        showError("Fetching an image by URL needs the OpenDoc desktop app — this runtime has no network access of its own.");
        return;
      }
      const updated = await run("add_binary_blob", { name: file.name, mediaType: file.media_type, bytes: bytesFromBase64(file.base64) });
      // Blobs are content-addressed, so the blob to point at is the one whose
      // hash the core just reported, never one recomputed here.
      const blob = updated.blobs.find((item) => item.name === file.name && item.size === file.size);
      if (!blob) {
        showError("The image was downloaded but could not be stored.");
        return;
      }
      await edit("insert_image_block_after", { afterBlockId: target.id, blobHash: blob.hash, altText: "" });
      break;
    }
    case "insert-blob-image": {
      const target = requireBlock();
      if (target && data.hash) await edit("insert_image_block_after", { afterBlockId: target.id, blobHash: data.hash, altText: "" });
      break;
    }
    case "delete-blob":
      if (data.hash) await edit("delete_binary_blob", { blobHash: data.hash });
      break;
    case "insert-citation":
    case "cite":
    case "cite-footnote": {
      const doc = state.doc;
      if (!doc) return;
      let referenceId = data.id ?? "";
      let locator = "";
      if (!referenceId) {
        if (doc.citations.references.length === 0) {
          toast("Add a reference first (Citations panel).");
          state.panel = "citations";
          renderSidePanel();
          return;
        }
        const chosen = await promptDialog({
          title: "Insert citation",
          fields: [
            { name: "reference", label: "Reference", type: "select", options: doc.citations.references.map((reference) => ({ value: reference.id, label: reference.title })) },
            { name: "locator", label: "Page / locator (optional)" },
          ],
          submit: "Cite",
        });
        if (!chosen) return;
        referenceId = chosen.reference;
        locator = chosen.locator;
      }
      const target = requireBlock();
      if (!target) return;
      if (action === "cite-footnote") {
        await edit("insert_footnote_citation_after", {
          blockId: target.id,
          afterInlineId: state.selection?.focus.inline_id ?? null,
          items: [{ reference_id: referenceId, locator: locator || null, label: null, prefix: null, suffix: null, suppress_author: false }],
        });
      } else {
        await edit("insert_citation", { referenceId, afterInlineId: state.selection?.focus.inline_id ?? null, locator: locator || null, label: null, prefix: null, suffix: null, suppressAuthor: false });
      }
      break;
    }
    case "add-reference": {
      const result = await promptDialog({
        title: "Add reference",
        fields: [
          { name: "title", label: "Title" },
          { name: "authors", label: "Authors (one per line)", type: "textarea" },
          { name: "issued", label: "Year" },
          { name: "doi", label: "DOI" },
          { name: "url", label: "URL" },
        ],
        submit: "Add",
      });
      if (!result?.title) return;
      await edit("add_bibliography_reference", {
        title: result.title,
        authors: result.authors.split("\n").map((line) => line.trim()).filter(Boolean),
        issued: result.issued || null,
        doi: result.doi || null,
        url: result.url || null,
      });
      break;
    }
    case "import-bibtex": {
      const file = await native("Could not read that BibTeX file", () => openFile(["bib", "bibtex"]));
      if (!file) return;
      await run("import_bibtex", { source: textFromBase64(file.base64) });
      toast("Imported local BibTeX library.");
      break;
    }
    case "delete-reference":
      if (data.id) await edit("delete_bibliography_reference", { referenceId: data.id });
      break;
    case "citation-style": {
      const result = await promptDialog({
        title: "Citation style",
        fields: [
          {
            name: "style",
            label: "Style",
            type: "select",
            value: state.doc?.citations.style,
            options: ["apa", "mla", "chicago-author-date", "chicago-notes", "ieee", "vancouver", "harvard", "nature", "author-year", "numeric"].map((style) => ({ value: style, label: style })),
          },
          { name: "locale", label: "Locale", value: state.doc?.citations.locale ?? "en-US" },
        ],
        submit: "Apply",
      });
      if (result) await edit("set_citation_style", { style: result.style, locale: result.locale });
      break;
    }
    case "clear-marks":
      await applyMark("all", null, "remove");
      break;
    case "indent":
    case "outdent": {
      if (!state.selection) return;
      // Rust decides what indenting means for each selected block: a list
      // item moves a level, anything else shifts its start indent.
      await edit("adjust_editor_selection_indent", { selection: state.selection, delta: action === "indent" ? 1 : -1 });
      state.editor?.setSelection(state.selection);
      break;
    }
    case "zoom-in":
    case "zoom-out":
    case "zoom-reset":
      state.zoom = action === "zoom-reset" ? 1 : Math.min(3, Math.max(0.5, state.zoom + (action === "zoom-in" ? 0.1 : -0.1)));
      renderToolbar();
      renderMain();
      break;
    case "word-count":
      // Nothing to fill in, so nothing to fill in with: these three dialogs
      // state a value and close. Each used to say it through a field's label
      // and leave the field itself empty on screen.
      await promptDialog({ title: "Word count", body: wordStats(), fields: [], submit: "Close" });
      break;
    case "shortcuts":
      await promptDialog({
        title: "Keyboard shortcuts",
        body: MENUS.flatMap((menu) => menu.items.filter((item) => item.shortcut).map((item) => `${item.shortcut}: ${item.label}`)).join("\n"),
        fields: [],
        submit: "Close",
      });
      break;
    case "about":
      await promptDialog({ title: "About OpenDoc", body: `OpenDoc – an open source, Rust-first document and spreadsheet editor. Runtime: ${state.profile?.label ?? runtime.mode}.`, fields: [], submit: "Close" });
      break;
    case "share":
      await openSharingDialog();
      break;
    case "sign": {
      const result = await promptDialog({
        title: "Sign document",
        fields: [
          { name: "signer", label: "Your name", value: state.authorName },
          { name: "key", label: "OpenSSH private key (ed25519)", type: "textarea", placeholder: "-----BEGIN OPENSSH PRIVATE KEY-----" },
        ],
        submit: "Sign",
      });
      if (result?.key) await edit("sign_with_openssh_private_key", { privateKeyPem: result.key, signerDisplay: result.signer });
      break;
    }
    case "verify": {
      const text = await run("verify_current_signatures");
      toast(text);
      break;
    }
    default:
      showError(`Unknown action ${action}`);
  }
}

/** Bulk resolution is already one Rust-owned, undoable batch. The UI asks for
 * deliberate consent immediately before it sends that batch, using a fresh
 * count so a stale side panel cannot quietly resolve a different set. */
async function confirmBulkSuggestionResolution(resolution: "accept" | "reject"): Promise<boolean> {
  const count = state.doc?.suggestions.filter((suggestion) => suggestion.state === "proposed").length ?? 0;
  if (count === 0) {
    toast("There are no open suggestions to resolve.");
    return false;
  }
  const verb = resolution === "accept" ? "Accept" : "Reject";
  return confirmDialog(
    `${verb} all open suggestions?`,
    `${verb} ${count} open suggestion${count === 1 ? "" : "s"} in one review action. This records your resolution on each proposal and can be undone as one gesture.`,
    `${verb} all`,
  );
}

/**
 * A panel click is delegated from the permanent app root, so it can arrive
 * after a remote projection resolved or auto-rejected a structural proposal.
 * Do not turn that stale UI event into a no-op durable review operation (or a
 * preview that looks actionable). The merge layer remains the authority; this
 * merely makes the current projection's state explicit at the UI boundary.
 */
function openSuggestionStillVisible(suggestionId: string): boolean {
  const open = state.doc?.suggestions.some((item) => item.id === suggestionId && item.state === "proposed") ?? false;
  if (open) return true;
  if (state.activeSuggestionId === suggestionId) state.activeSuggestionId = null;
  if (state.panel === "suggestions") renderSidePanel();
  toast("This suggestion is no longer open.");
  return false;
}

/**
 * A preview is deliberately a detached modal, not a temporary application of
 * an operation to the live editor.  Rust has rendered this HTML from a cloned
 * document using the normal renderer, so it contains no user-provided markup
 * and closing it cannot race or undo a real review decision.
 */
function showSuggestionPreview(resolution: "accept" | "reject", html: string): void {
  const dialog = document.createElement("dialog");
  dialog.className = "modal suggestion-preview";
  dialog.innerHTML = `<form method="dialog" class="modal-form"><h2>Preview ${resolution === "accept" ? "acceptance" : "rejection"}</h2><p class="modal-body">This is a read-only projection. It does not change the document.</p><div class="suggestion-preview-document" aria-label="Suggestion preview"></div><div class="modal-actions"><button type="submit" class="primary">Close</button></div></form>`;
  // The renderer escapes every source string and does not emit executable
  // content; inserting its already-rendered document is what lets the preview
  // faithfully show formatting rather than a quoted HTML source string.
  const body = dialog.querySelector<HTMLElement>(".suggestion-preview-document");
  if (body) body.innerHTML = html;
  document.body.appendChild(dialog);
  dialog.addEventListener("close", () => dialog.remove(), { once: true });
  dialog.showModal();
  dialog.querySelector<HTMLButtonElement>("button")?.focus();
}

/**
 * Navigate from a review item to the durable model anchor carried in its
 * projection. `anchor_label` is deliberately only prose; `anchor` is the
 * stable identifier encoding (`inline..inline`, `nearest:block`, document).
 * A repaired/missing projection is harmless: the sidebar stays usable and we
 * simply leave the user where they are instead of guessing from display text.
 */
function navigateToReviewAnchor(anchor: string | undefined, threadId?: string): void {
  if (!anchor) return;
  if (threadId) {
    state.activeCommentThreadId = threadId;
    // The current marker is intentionally view-only. Re-rendering the panel
    // makes it available to sighted and screen-reader users without changing
    // the durable comment thread or its anchor.
    if (state.panel === "comments") renderSidePanel();
  }
  let target: HTMLElement | null = null;
  if (anchor.startsWith("nearest:")) {
    const blockId = anchor.slice("nearest:".length);
    target = document.querySelector<HTMLElement>(`[data-block-id="${CSS.escape(blockId)}"]`);
  } else if (anchor !== "document") {
    const [start] = anchor.split("..", 1);
    if (start) target = document.querySelector<HTMLElement>(`[data-inline-id="${CSS.escape(start)}"]`);
  } else {
    target = document.querySelector<HTMLElement>("[data-editor-shell]");
  }
  if (!target) return;
  target.scrollIntoView({ block: "center", behavior: "smooth" });
  target.classList.add("comment-anchor-target");
  window.setTimeout(() => target?.classList.remove("comment-anchor-target"), 1600);
}

/**
 * Step through exactly the threads the current sidebar filter exposes.  This
 * means "next" never surprises a reviewer by landing on a resolved thread
 * while they are reviewing open work, and it gives keyboard users the same
 * navigation order as the visible controls.
 */
function navigateCommentThread(direction: 1 | -1): void {
  const doc = state.doc;
  if (!doc) return;
  const threads = doc.comments.filter((thread) =>
    matchesCommentFilter(thread, state.commentFilter, state.authorName),
  );
  if (threads.length === 0) {
    toast("No comments match this filter.");
    return;
  }
  state.panel = "comments";
  const current = threads.findIndex((thread) => thread.id === state.activeCommentThreadId);
  const index = current < 0 ? (direction === 1 ? 0 : threads.length - 1) : (current + direction + threads.length) % threads.length;
  const thread = threads[index];
  navigateToReviewAnchor(thread.anchor, thread.id);
  // `navigateToReviewAnchor` synchronously re-renders the comments panel.
  // Move screen-reader focus to its current card after source navigation.
  // This remains view-only local state.
  requestAnimationFrame(() => {
    const card = document.querySelector<HTMLElement>(`[data-thread="${CSS.escape(thread.id)}"]`);
    card?.focus({ preventScroll: true });
  });
}

/**
 * Move through the same proposed-only sequence the Suggestions panel renders.
 * This is deliberately local review state: navigation must not select text,
 * change a proposal, or emit a collaboration operation merely because someone
 * is inspecting their queue with a keyboard.
 */
function navigateSuggestion(direction: 1 | -1): void {
  const items = state.doc?.suggestions.filter((item) => item.state === "proposed") ?? [];
  if (items.length === 0) {
    toast("There are no open suggestions.");
    return;
  }
  const current = items.findIndex((item) => item.id === state.activeSuggestionId);
  const index = current < 0 ? (direction === 1 ? 0 : items.length - 1) : (current + direction + items.length) % items.length;
  state.activeSuggestionId = items[index].id;
  state.panel = "suggestions";
  renderSidePanel();
  // Focus the newly rendered review card after it exists.  The card is an
  // explicit tabindex=-1 target, so this does not add another stop to normal
  // Tab order but does give screen-reader users the announced context.
  requestAnimationFrame(() => {
    const target = [...document.querySelectorAll<HTMLElement>("[data-suggestion-id]")]
      .find((candidate) => candidate.dataset.suggestionId === state.activeSuggestionId);
    target?.scrollIntoView({ block: "nearest", behavior: "smooth" });
    target?.focus({ preventScroll: true });
  });
}

/**
 * Undo or redo, reporting what the core actually said.
 *
 * This used to be `.catch(() => toast("Nothing to undo"))`, which is a lie in
 * every case but one. `state.rs::step_edit_history` raises two different
 * `Conflict`s: an empty stack — the only one "Nothing to undo" describes —
 * and, inside a collaboration session, a refusal to undo a step that can only
 * be undone by restoring a whole-state snapshot. ADR 0017 is explicit that
 * that second one must be "an error the user can be told about rather than a
 * silent rollback", and telling them "Nothing to undo" instead both hides the
 * refusal and denies the history they can still see in front of them.
 *
 * Where the message goes is the difference between the two, too. An empty
 * stack is an ordinary, expected answer, so it is a toast; a refusal is a
 * command Rust would not run, so it goes to the error banner every other
 * refused command uses (`runDispatch` in `shared.ts`).
 */
async function stepHistory(step: "undo" | "redo"): Promise<void> {
  try {
    applyDocument(await invoke(step === "undo" ? "undo_current_edit" : "redo_current_edit"));
  } catch (error) {
    // `step_edit_history` writes exactly this sentence for an empty stack, and
    // that is the only thing "Nothing to undo" describes. Anything else — a
    // refusal, or a failure that is not one of Rust's at all — is reported as
    // itself rather than dressed up as an empty stack.
    const reason = coreErrorMessage(error);
    if (reason === `nothing to ${step}`) toast(`Nothing to ${step}`);
    else showError(sentence(reason ?? (error instanceof Error ? error.message : String(error))));
  }
  state.editor?.setSelection(state.selection);
}

/**
 * The sentence inside an `AppApiError`.
 *
 * `AppApiError` reaches a transport as its `Debug` form — `Conflict("…")` —
 * which is what lets `isUnsavedChangesError` recognise a variant without
 * reading prose. Here the variant is not the question and the prose is, so
 * the payload is unwrapped; `null` when the error is not one of Rust's, which
 * is the caller's cue that it has nothing better than its own wording.
 *
 * It belongs beside `isUnsavedChangesError` in `invoke.ts`, which owns this
 * wire form. It is here because that file is not this change's to edit.
 */
function coreErrorMessage(error: unknown): string | null {
  const text = (error instanceof Error ? error.message : String(error)).trim();
  const match = /^[A-Z][A-Za-z]*\((.*)\)$/s.exec(text);
  if (!match) return null;
  const payload = match[1];
  if (!payload.startsWith('"') || !payload.endsWith('"')) return payload || null;
  try {
    return String(JSON.parse(payload)) || null;
  } catch {
    return payload.slice(1, -1) || null;
  }
}

/** Rust writes its messages lower-case; a banner starts with a capital. */
function sentence(message: string): string {
  return message.charAt(0).toUpperCase() + message.slice(1);
}
