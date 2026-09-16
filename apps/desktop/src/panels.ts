// The side panels: comments, suggestions, citations, footnotes, attachments,
// history, signatures, versions and warnings.
//
// Every control inside a panel is a `data-action` button routed by the
// delegated dispatcher, so a re-render can never stack a listener. The version
// panel is large enough to own its own module; this one asks it for its markup
// and its badge count.
import { escapeHtml } from "./ui";
import type { AppDocument, AppWarning } from "./types";
import type { CommentFilter, Panel, SuggestionFilter } from "./state";
import { matchesCommentFilter, matchesSuggestionFilter, state } from "./state";
import { findBlock, query } from "./shared";
import { renderVersionsPanel, versionCount } from "./versions";
import { exportWarningReport } from "./files";
import { renderCollaborationRegion } from "./collab";

export const PANELS: { id: Panel; label: string; icon: string }[] = [
  { id: "outline", label: "Outline", icon: "☷" },
  { id: "bookmarks", label: "Bookmarks", icon: "🔖" },
  { id: "comments", label: "Comments", icon: "💬" },
  { id: "suggestions", label: "Suggestions", icon: "✎" },
  { id: "citations", label: "Citations", icon: "❝" },
  { id: "footnotes", label: "Footnotes", icon: "¹" },
  { id: "files", label: "Attachments", icon: "📎" },
  { id: "history", label: "History", icon: "🕒" },
  { id: "signatures", label: "Signatures", icon: "🔏" },
  { id: "versions", label: "Versions", icon: "🗂" },
  { id: "warnings", label: "Warnings", icon: "⚠" },
];

export function renderSidePanel(): void {
  // The collaboration region lives in the topbar rather than in a panel — a
  // connection state and who else is here are always-visible facts, not
  // something to open a drawer for. It is rendered from here because this
  // function runs at the end of every render pass, so `collab.ts` does not
  // have to know when the shell was rebuilt under it.
  renderCollaborationRegion();
  const strip = query("[data-side-strip]");
  const side = query("[data-side-panel]");
  const doc = state.doc;
  if (!strip || !side || !doc) return;
  // A remote projection rebuilds the panel below.  Keyboard navigation puts
  // focus on a review card, so remember that narrow focus contract before the
  // old DOM is detached.  We intentionally do not preserve arbitrary panel
  // control focus: those controls may have just changed meaning or vanished.
  const reviewFocus = state.pendingReviewFocus ?? reviewFocusBeforePanelRender(side);
  state.pendingReviewFocus = null;
  const counts: Record<Panel, number> = {
    outline: 0,
    bookmarks: doc.bookmarks.filter((bookmark) => !bookmark.deleted).length,
    comments: doc.comments.filter((thread) => !thread.deleted).length,
    suggestions: doc.suggestions.filter((item) => item.state === "proposed").length,
    citations: doc.citations.references.length,
    footnotes: doc.footnotes.filter((note) => !note.deleted).length,
    files: doc.blobs.length,
    history: doc.operation_count,
    signatures: doc.signatures.length,
    versions: versionCount(),
    warnings: doc.warnings.length + (exportWarningReport()?.warnings.length ?? 0),
  };
  strip.innerHTML = PANELS.map(
    (item) => `<button type="button" class="strip-button${state.panel === item.id ? " active" : ""}" data-action="toggle-panel:${item.id}" title="${escapeHtml(item.label)}" aria-label="${escapeHtml(item.label)}">${item.icon}${counts[item.id] ? `<span class="badge">${counts[item.id]}</span>` : ""}</button>`,
  ).join("");
  const panel = state.panel;
  side.hidden = !panel;
  if (!panel) return;
  const body = renderPanelBody(panel);
  // Closing is intentionally not expressed as another toggle.  The close
  // affordance means "no side panel", even if a render or remote projection
  // changed which panel was open between pointer-down and dispatch.
  side.innerHTML = `<header class="panel-header"><h2>${escapeHtml(PANELS.find((item) => item.id === panel)?.label ?? "")}</h2><button type="button" data-action="close-panel" aria-label="Close">✕</button></header><div class="panel-body">${body}</div>`;
  restoreReviewFocusAfterPanelRender(side, reviewFocus);
}

type ReviewFocus = NonNullable<typeof state.pendingReviewFocus>;

function reviewFocusBeforePanelRender(side: HTMLElement): ReviewFocus | null {
  const active = document.activeElement;
  if (!(active instanceof HTMLElement) || !side.contains(active)) return null;
  const thread = active.closest<HTMLElement>("[data-thread].current");
  if (thread?.dataset.thread) return { panel: "comments", id: thread.dataset.thread };
  const suggestion = active.closest<HTMLElement>("[data-suggestion-id].current");
  if (suggestion?.dataset.suggestionId) return { panel: "suggestions", id: suggestion.dataset.suggestionId };
  // A first render can already have moved focus to this fallback before a
  // nested document-surface render asks for the panel again. Preserve the
  // known review fallback across that second pass as well.
  if (active.dataset.action === "comment-next") return { panel: "comments", id: "" };
  if (active.dataset.action === "suggestion-next") return { panel: "suggestions", id: "" };
  return null;
}

function restoreReviewFocusAfterPanelRender(side: HTMLElement, previous: ReviewFocus | null): void {
  if (!previous || state.panel !== previous.panel) return;
  // `renderMain` can synchronously request a second panel render while its
  // document fragments settle.  Wait for that render turn, then resolve the
  // target again in the currently connected panel rather than focusing a DOM
  // node the second pass will immediately detach.
  requestAnimationFrame(() => {
    if (!side.isConnected || state.panel !== previous.panel) return;
    const selector = previous.panel === "comments"
      ? `[data-thread="${CSS.escape(previous.id)}"].current`
      : `[data-suggestion-id="${CSS.escape(previous.id)}"].current`;
    const card = side.querySelector<HTMLElement>(selector);
    // A remote resolution/delete can make the prior card unavailable. Keep
    // the reviewer in the same navigation group rather than pretending a
    // different card became current.
    const fallback = side.querySelector<HTMLElement>(
      `[data-action="${previous.panel === "comments" ? "comment-next" : "suggestion-next"}"]`,
    );
    (card ?? fallback)?.focus({ preventScroll: true });
  });
}

function renderPanelBody(which: Panel): string {
  const doc = state.doc;
  if (!doc) return "";
  switch (which) {
    case "outline": {
      const headings = doc.blocks.filter((block) => block.kind === "heading");
      if (headings.length === 0) {
        return `<p class="empty">Add headings to build a document outline.</p>`;
      }
      return `<nav class="document-outline" aria-label="Document outline">${renderOutline(headings)}</nav>`;
    }
    case "bookmarks": {
      const bookmarks = doc.bookmarks
        .filter((bookmark) => !bookmark.deleted)
        .sort((left, right) => left.name.localeCompare(right.name) || left.id.localeCompare(right.id));
      if (bookmarks.length === 0) {
        return `<p class="empty">No bookmarks yet.</p>`;
      }
      return `<nav class="document-bookmarks" aria-label="Bookmarks"><ul>${bookmarks
        .map((bookmark) => {
          // A bookmark may validly outlive its target after a concurrent
          // delete. It remains durable (an undo can restore the block), but a
          // normal navigation button would otherwise fail with no explanation.
          const targetExists = findBlock(bookmark.block_id) !== null;
          const unavailable = targetExists
            ? ""
            : ` disabled aria-label="${escapeHtml(bookmark.name)} (target deleted)"`;
          const status = targetExists ? "" : `<span class="meta">Target deleted</span>`;
          return `<li><button type="button" data-action="bookmark-go" data-block-id="${escapeHtml(bookmark.block_id)}"${unavailable}>${escapeHtml(bookmark.name)}</button>${status}<button type="button" data-action="bookmark-delete" data-bookmark-id="${escapeHtml(bookmark.id)}" aria-label="Delete bookmark ${escapeHtml(bookmark.name)}">Delete</button></li>`;
        })
        .join("")}</ul></nav>`;
    }
    case "comments": {
      // This is only a local presentation guard. The service remains the
      // authority for a session role, and the command layer still validates
      // every attempted mutation.
      const commentsWritable = state.documentEditingMode !== "view" && state.runtimeSession?.service_session?.role !== "viewer";
      const visibleThreads = doc.comments.filter((thread) =>
        matchesCommentFilter(thread, state.commentFilter, state.authorName),
      );
      // A filter action handles its own state transition, but this projection
      // can also change after a remote merge, restore/delete, or ordinary
      // re-render. Do not retain a local current/reply target for a thread the
      // current durable projection no longer exposes: returning to a filter
      // must not silently reopen a reply composer the reviewer left behind.
      if (!visibleThreads.some((thread) => thread.id === state.activeCommentThreadId)) {
        state.activeCommentThreadId = null;
      }
      if (!visibleThreads.some((thread) => thread.id === state.activeCommentReplyThreadId)) {
        state.activeCommentReplyThreadId = null;
      }
      const currentIndex = visibleThreads.findIndex((thread) => thread.id === state.activeCommentThreadId);
      const current = currentIndex < 0 ? null : visibleThreads[currentIndex];
      const currentAuthor = current?.comments.find((comment) => !comment.deleted)?.author;
      const commentNoun = visibleThreads.length === 1 ? "comment" : "comments";
      const filterStatus = state.commentFilter === "for-you"
        ? `Showing ${visibleThreads.length === 0 ? "no" : visibleThreads.length} ${commentNoun} assigned to you.`
        : state.commentFilter === "all"
          ? `Showing ${visibleThreads.length === 0 ? "no" : visibleThreads.length} ${commentNoun}.`
          : `Showing ${visibleThreads.length === 0 ? "no" : visibleThreads.length} ${state.commentFilter} ${commentNoun}.`;
      const navigationStatus = current
        ? `${filterStatus} Current comment ${currentIndex + 1} of ${visibleThreads.length}${currentAuthor ? `: ${currentAuthor}.` : "."}`
        : filterStatus;
      const filterButton = (filter: CommentFilter, label: string) =>
        `<button type="button" class="comment-filter${state.commentFilter === filter ? " active" : ""}" data-action="comment-filter:${filter}" aria-pressed="${state.commentFilter === filter}">${label}</button>`;
      return `
        <button type="button" class="panel-action" data-action="comment">Add comment on selection</button>
        <div class="comment-filters" role="group" aria-label="Comment status">${filterButton("open", "Open")}${filterButton("for-you", "For you")}${filterButton("resolved", "Resolved")}${filterButton("all", "All")}</div>
        <div class="comment-navigation" role="group" aria-label="Comment navigation">
          <button type="button" data-action="comment-previous" aria-keyshortcuts="Control+Alt+ArrowUp" title="Previous comment (Ctrl+Alt+Up)">Previous</button>
          <button type="button" data-action="comment-next" aria-keyshortcuts="Control+Alt+ArrowDown" title="Next comment (Ctrl+Alt+Down)">Next</button>
        </div>
        <p class="sr-only" role="status" aria-live="polite" aria-atomic="true" data-comment-navigation-status>${escapeHtml(navigationStatus)}</p>
        ${renderCommentActivity(doc)}
        ${visibleThreads.length === 0 ? `<p class="empty">${doc.comments.some((thread) => !thread.deleted) ? "No comments match this filter." : "No comments yet."}</p>` : ""}
        ${visibleThreads
          .map(
            (thread) => `
          <article class="thread${state.activeCommentThreadId === thread.id ? " current" : ""}" data-thread="${escapeHtml(thread.id)}" tabindex="-1"${state.activeCommentThreadId === thread.id ? ' aria-current="true"' : ""}>
            ${thread.anchor === "orphaned" ? renderOrphanedCommentAnchor(thread) : `<button type="button" class="anchor comment-anchor-link" data-action="comment-go" data-id="${escapeHtml(thread.id)}" data-anchor="${escapeHtml(thread.anchor)}">${escapeHtml(thread.anchor_label)}</button>`}
            ${thread.state === "resolved" ? `<p class="anchor">Resolved${thread.resolved_by ? ` by ${escapeHtml(thread.resolved_by)}` : ""}</p>` : ""}
            ${thread.action_assignee ? `<p class="anchor">Action item · assigned to ${escapeHtml(thread.action_assignee)}${thread.action_due_at_ms ? ` · ${renderActionDueDate(thread.action_due_at_ms)}` : ""}${thread.action_completed_by ? ` · completed by ${escapeHtml(thread.action_completed_by)}` : ""}</p>` : ""}
            <div class="comment-reactions" role="group" aria-label="Comment reactions">
              ${[...new Set(["👍", "❤️", "🎉", ...(thread.reactions ?? []).map((item) => item.emoji)])]
                .map((emoji) => {
                  // Older saved/local smoke projections predate reactions.
                  // Treat their absent field as the empty durable set.  Keep
                  // imported/custom reactions visible too: silently showing
                  // only the preset trio would misrepresent the durable set.
                  const reaction = (thread.reactions ?? []).find((item) => item.emoji === emoji);
                  const reacted = reaction?.actors.includes(state.authorName) ?? false;
                  const count = reaction?.actors.length ?? 0;
                  const stateLabel = reacted ? "selected" : "not selected";
                  return `<button type="button" data-action="comment-reaction" data-id="${escapeHtml(thread.id)}" data-emoji="${emoji}" data-present="${!reacted}" aria-pressed="${reacted}" aria-label="${escapeHtml(`${emoji} reaction, ${count} ${count === 1 ? "person" : "people"}, ${stateLabel}`)}"${commentsWritable ? "" : " disabled"}>${emoji}${count ? ` ${count}` : ""}</button>`;
                })
                .join("")}
            </div>
            ${thread.comments
              .filter((comment) => !comment.deleted)
              .map(
                (comment) => `<div class="comment" data-comment-id="${escapeHtml(comment.id)}" tabindex="-1"><strong>${escapeHtml(comment.author)}</strong><p>${escapeHtml(comment.body)}</p>
                  <div class="thread-actions comment-actions">
                    <button type="button" data-action="edit-comment" data-id="${escapeHtml(thread.id)}" data-comment-id="${escapeHtml(comment.id)}" aria-label="Edit comment by ${escapeHtml(comment.author)}"${commentsWritable ? "" : " disabled"}>Edit</button>
                    <button type="button" data-action="delete-comment" data-id="${escapeHtml(thread.id)}" data-comment-id="${escapeHtml(comment.id)}" aria-label="Delete comment by ${escapeHtml(comment.author)}"${commentsWritable ? "" : " disabled"}>Delete</button>
                  </div>
                </div>`,
              )
              .join("")}
            ${renderDeletedCommentRestorations(thread, commentsWritable)}
            ${renderCommentProvenance(thread, doc.comment_history ?? [])}
            ${state.activeCommentReplyThreadId === thread.id ? renderInlineCommentReply(thread.id, commentsWritable) : ""}
            <div class="thread-actions">
              <button type="button" data-action="reply-comment" data-id="${escapeHtml(thread.id)}"${commentsWritable ? "" : " disabled"}>Reply</button>
              ${thread.state === "resolved" ? `<button type="button" data-action="reopen-comment" data-id="${escapeHtml(thread.id)}"${commentsWritable ? "" : " disabled"}>Reopen</button>` : `<button type="button" data-action="resolve-comment" data-id="${escapeHtml(thread.id)}"${commentsWritable ? "" : " disabled"}>Resolve</button>`}
              <button type="button" data-action="comment-action" data-id="${escapeHtml(thread.id)}" aria-label="${escapeHtml(actionItemButtonLabel(thread))}"${commentsWritable ? "" : " disabled"}>${thread.action_assignee ? "Manage action" : "Assign action"}</button>
            </div>
          </article>`,
          )
          .join("")}`;
    }
    case "suggestions": {
      const proposedItems = doc.suggestions.filter((item) => item.state === "proposed");
      const items = doc.suggestions.filter((item) => matchesSuggestionFilter(item, state.suggestionFilter));
      // Remote resolution can remove the card the keyboard navigator last
      // selected. Reconcile against the durable projection before computing
      // its index so the next shortcut begins at a real visible proposal
      // instead of wrapping from a stale -1 position.
      if (!proposedItems.some((item) => item.id === state.activeSuggestionId)) {
        state.activeSuggestionId = null;
      }
      const currentIndex = proposedItems.findIndex((item) => item.id === state.activeSuggestionId);
      const current = currentIndex < 0 ? null : proposedItems[currentIndex];
      const navigationStatus = current
        ? `Suggestion ${currentIndex + 1} of ${proposedItems.length}: ${current.author}, ${current.kind}.`
        : state.suggestionFilter === "proposed"
          ? `Showing ${proposedItems.length === 0 ? "no" : proposedItems.length} open suggestions.`
          : `Showing ${items.length === 0 ? "no" : items.length} ${state.suggestionFilter === "resolved" ? "resolved" : "total"} suggestions. Open-suggestion navigation is unavailable in this view.`;
      const filterButton = (filter: SuggestionFilter, label: string) =>
        `<button type="button" class="comment-filter${state.suggestionFilter === filter ? " active" : ""}" data-action="suggestion-filter:${filter}" aria-pressed="${state.suggestionFilter === filter}">${label}</button>`;
      const reviewingOpenSuggestions = state.suggestionFilter === "proposed";
      return `
        <div class="panel-actions"><button type="button" class="panel-action" data-action="suggest">Suggest replacement…</button><button type="button" class="panel-action" data-action="suggest-delete">Suggest deletion</button><button type="button" class="panel-action" data-action="suggest-block-delete">Suggest block deletion</button><button type="button" class="panel-action" data-action="suggest-block-insert">Suggest paragraph after…</button><button type="button" class="panel-action" data-action="suggest-block-replace">Suggest paragraph replacement…</button></div>
        <div class="comment-filters" role="group" aria-label="Suggestion status">${filterButton("proposed", "Open")}${filterButton("resolved", "Resolved")}${filterButton("all", "All")}</div>
        ${reviewingOpenSuggestions && proposedItems.length > 1 ? `<div class="panel-actions" role="group" aria-label="Resolve all open suggestions"><button type="button" data-action="accept-all">Accept all (${proposedItems.length})</button><button type="button" data-action="reject-all">Reject all (${proposedItems.length})</button></div>` : ""}
        ${reviewingOpenSuggestions ? `<div class="comment-navigation" role="group" aria-label="Open suggestion navigation">
          <button type="button" data-action="suggestion-previous" aria-keyshortcuts="Control+Alt+ArrowLeft" title="Previous suggestion (Ctrl+Alt+Left)">Previous</button>
          <button type="button" data-action="suggestion-next" aria-keyshortcuts="Control+Alt+ArrowRight" title="Next suggestion (Ctrl+Alt+Right)">Next</button>
        </div>` : ""}
        <p class="sr-only" role="status" aria-live="polite" aria-atomic="true" data-suggestion-navigation-status>${escapeHtml(navigationStatus)}</p>
        ${state.suggestionFilter !== "proposed" ? `<p class="meta">Resolved suggestions retain their stored state and any available lifecycle evidence. This evidence has no timestamp unless a future typed review-history record supplies one.</p>` : ""}
        ${items.length === 0 ? `<p class="empty">${doc.suggestions.length === 0 ? "No suggestions yet." : state.suggestionFilter === "proposed" ? "No open suggestions." : "No suggestions match this filter."}</p>` : ""}
        ${items
          .map(
            (item) => `
          <article class="suggestion${item.state === "proposed" && state.activeSuggestionId === item.id ? " current" : ""}" data-suggestion-id="${escapeHtml(item.id)}" tabindex="-1"${item.state === "proposed" && state.activeSuggestionId === item.id ? ' aria-current="true"' : ""}>
            <p><strong>${escapeHtml(item.author)}</strong> · ${escapeHtml(item.kind)}</p>
            ${item.state === "proposed" ? "" : `<p class="anchor">${escapeHtml(suggestionResolutionLabel(item.state))}</p>`}
            ${renderSuggestionAnchor(item)}
            ${item.kind === "paragraph_style_change" ? `<p class="meta">${escapeHtml(item.paragraph_style_expected ?? "unknown")} → ${escapeHtml(item.paragraph_style_proposed ?? "unknown")}</p>` : ""}
            <p>${escapeHtml(item.text)}</p>
            ${renderSuggestionProvenance(item)}
            ${item.state === "proposed" ? `<div class="thread-actions">
              ${item.kind === "insert" ? `<button type="button" data-action="edit-suggestion" data-id="${escapeHtml(item.id)}">Edit proposal</button>` : ""}
              <button type="button" data-action="preview-suggestion" data-id="${escapeHtml(item.id)}" data-resolution="accept">Preview accept</button>
              <button type="button" data-action="preview-suggestion" data-id="${escapeHtml(item.id)}" data-resolution="reject">Preview reject</button>
              <button type="button" data-action="accept-suggestion" data-id="${escapeHtml(item.id)}">Accept</button>
              <button type="button" data-action="reject-suggestion" data-id="${escapeHtml(item.id)}">Reject</button>
            </div>` : ""}
          </article>`,
          )
          .join("")}`;
    }
    case "citations":
      return `
        <div class="panel-actions"><button type="button" class="panel-action" data-action="add-reference">Add reference…</button><button type="button" class="panel-action" data-action="import-bibtex">Import BibTeX…</button></div>
        <p class="meta">Style: ${escapeHtml(doc.citations.style)} · ${escapeHtml(doc.citations.locale)} <button type="button" data-action="citation-style">Change</button></p>
        ${doc.citations.references.length === 0 ? `<p class="empty">No references yet.</p>` : ""}
        ${doc.citations.references
          .map(
            (reference) => `
          <article class="reference">
            <p><strong>${escapeHtml(reference.title)}</strong></p>
            <p class="meta">${escapeHtml(reference.authors.join(", "))}${reference.issued ? ` (${escapeHtml(reference.issued)})` : ""}</p>
            <div class="thread-actions">
              <button type="button" data-action="cite" data-id="${escapeHtml(reference.id)}">Cite</button>
              <button type="button" data-action="cite-footnote" data-id="${escapeHtml(reference.id)}">Cite in footnote</button>
              <button type="button" data-action="delete-reference" data-id="${escapeHtml(reference.id)}">Delete</button>
            </div>
          </article>`,
          )
          .join("")}
        ${doc.citations.bibliography.length > 0 ? `<h3>Bibliography</h3><ol class="bibliography">${doc.citations.bibliography.map((entry) => `<li>${escapeHtml(entry.text)}</li>`).join("")}</ol>` : ""}`;
    case "footnotes": {
      const endnoteIds = new Set(doc.endnote_ids ?? []);
      const notes = doc.footnotes.filter((note) => !note.deleted && !endnoteIds.has(note.id));
      const endnotes = doc.footnotes.filter((note) => !note.deleted && endnoteIds.has(note.id));
      const renderNotes = (items: typeof notes, label: string) =>
        `${items.length ? `<h3>${label}</h3>` : ""}${items
          .map(
            (note, index) => `<article class="footnote"><p><sup>${index + 1}</sup> ${escapeHtml(note.body.map((inline) => inline.text).join(""))}</p><button type="button" data-action="edit-footnote" data-id="${escapeHtml(note.id)}">Edit</button></article>`,
          )
          .join("")}`;
      return `
        <button type="button" class="panel-action" data-action="insert-footnote">Insert footnote at caret</button>
        <button type="button" class="panel-action" data-action="insert-endnote">Insert endnote at caret</button>
        ${notes.length === 0 && endnotes.length === 0 ? `<p class="empty">No notes.</p>` : ""}
        ${renderNotes(notes, "Footnotes")}
        ${renderNotes(endnotes, "Endnotes")}`;
    }
    case "files":
      return `
        <button type="button" class="panel-action" data-action="attach-file">Attach file…</button>
        ${doc.blobs.length === 0 ? `<p class="empty">No attachments.</p>` : ""}
        ${doc.blobs
          .map(
            (blob) => `
          <article class="blob">
            <p><strong>${escapeHtml(blob.name)}</strong> <span class="meta">${escapeHtml(blob.media_type)} · ${blob.size} bytes${blob.available ? "" : " · missing"}</span></p>
            <div class="thread-actions">
              ${blob.media_type.startsWith("image/") ? `<button type="button" data-action="insert-blob-image" data-hash="${escapeHtml(blob.hash)}" data-name="${escapeHtml(blob.name)}">Insert image</button>` : ""}
              <button type="button" data-action="delete-blob" data-hash="${escapeHtml(blob.hash)}">Delete</button>
            </div>
          </article>`,
          )
          .join("")}`;
    case "history":
      return `
        <div class="panel-actions"><button type="button" data-action="undo">Undo</button><button type="button" data-action="redo">Redo</button></div>
        <p class="meta">${doc.operation_count} operations · ${doc.last_manifest ? `version ${escapeHtml(doc.last_manifest.slice(0, 19))}…` : "not saved"}</p>
        <ol class="operations">${doc.operations
          .slice()
          .reverse()
          .slice(0, 100)
          .map((op) => `<li><span class="meta">${escapeHtml(new Date(op.created_at_ms).toLocaleTimeString())}</span> ${escapeHtml(op.summary)}</li>`)
          .join("")}</ol>`;
    case "signatures":
      return `
        <div class="panel-actions"><button type="button" class="panel-action" data-action="sign">Sign document…</button><button type="button" class="panel-action" data-action="verify">Verify</button></div>
        <p class="meta">State: ${escapeHtml(doc.signature_state)}</p>
        ${doc.signatures.length === 0 ? `<p class="empty">No signatures.</p>` : ""}
        ${doc.signatures.map((signature) => `<article class="signature"><p><strong>${escapeHtml(signature.signer_display)}</strong></p><p class="meta">${escapeHtml(signature.signer)} · ${escapeHtml(new Date(signature.signed_at_ms).toLocaleString())}</p></article>`).join("")}`;
    case "versions":
      return renderVersionsPanel();
    case "warnings": {
      const list = (warnings: AppWarning[]) =>
        `<ul class="warnings">${warnings.map((warning) => `<li><code>${escapeHtml(warning.code)}</code> ${escapeHtml(warning.message)}</li>`).join("")}</ul>`;
      // The last export's warnings are shown above the document's and stay
      // labelled as its own: they describe what one export dropped, not
      // something wrong with the document, and they are never written into it.
      const report = exportWarningReport();
      const exported =
        report && report.warnings.length > 0
          ? `<h3 class="panel-section">${escapeHtml(report.label)} export</h3>${list(report.warnings)}<h3 class="panel-section">Document</h3>`
          : "";
      if (doc.warnings.length === 0) {
        return `${exported}<p class="empty">No warnings.</p>`;
      }
      return `${exported}${list(doc.warnings)}`;
    }
  }
}

/**
 * Suggestions retain compact, append-only lifecycle evidence (`accepted-by:…`,
 * `auto-rejected:…`, and imported values). It is not the typed comment-history
 * ledger, so it must not be presented as a timestamped audit record. Unknown
 * imported values stay visible verbatim and escaped rather than guessed at.
 */
function renderSuggestionProvenance(item: AppDocument["suggestions"][number]): string {
  if (item.provenance.length === 0) return "";
  const label = (entry: string): string => {
    if (entry.startsWith("accepted-by:")) return `Accepted by ${entry.slice("accepted-by:".length)}`;
    if (entry.startsWith("rejected-by:")) return `Rejected by ${entry.slice("rejected-by:".length)}`;
    if (entry.startsWith("auto-rejected:")) return `Automatically rejected: ${entry.slice("auto-rejected:".length)}`;
    if (entry.startsWith("auto-degraded:")) return `Automatically degraded: ${entry.slice("auto-degraded:".length)}`;
    return entry;
  };
  const count = item.provenance.length;
  return `<details class="comment-provenance suggestion-provenance" aria-label="Lifecycle evidence for suggestion by ${escapeHtml(item.author)}"><summary>Lifecycle evidence (${count})</summary><ul data-suggestion-provenance>${item.provenance.map((entry) => `<li>${escapeHtml(label(entry))}</li>`).join("")}</ul></details>`;
}

/**
 * Suggestions are durable review evidence, but not every suggestion names a
 * live source node: a proposed insertion has its own future block identity,
 * while a `first`/`last` structural position has no stable target at all.
 * Offer a keyboard-reachable source jump only for the projection's existing,
 * resolvable identities rather than guessing from its display label.
 */
function renderSuggestionAnchor(item: AppDocument["suggestions"][number]): string {
  const anchor = suggestionReviewAnchor(item);
  if (!item.anchor_label) return "";
  if (!anchor) return `<p class="anchor">${escapeHtml(item.anchor_label)}</p>`;
  return `<button type="button" class="anchor comment-anchor-link" data-action="suggestion-go" data-id="${escapeHtml(item.id)}" data-state="${escapeHtml(item.state)}" data-anchor="${escapeHtml(anchor)}">${escapeHtml(item.anchor_label)}</button>`;
}

function suggestionResolutionLabel(state: string): string {
  if (state === "accepted") return "Accepted";
  if (state === "rejected") return "Rejected";
  return `Resolved (${state})`;
}

function suggestionReviewAnchor(item: AppDocument["suggestions"][number]): string | null {
  if (item.anchor) return item.anchor;
  if (item.range_start && item.range_end) return `${item.range_start}..${item.range_end}`;
  if (item.link_inline_id) return `${item.link_inline_id}..${item.link_inline_id}`;
  if (item.block_id) return `nearest:${item.block_id}`;
  // A structural insert has no live proposed block yet, but its durable
  // before/after placement can name an adjacent current block.
  const position = item.block_position;
  if (position?.startsWith("before:") || position?.startsWith("after:")) {
    const blockId = position.slice(position.indexOf(":") + 1);
    return blockId ? `nearest:${blockId}` : null;
  }
  return null;
}

/**
 * An actual form rather than a contenteditable imitation: it gives assistive
 * technology a labelled multiline field, lets Enter/submit use browser form
 * semantics, and contains no body in a data attribute where it could become
 * markup on the next render.  `bindings.ts` delegates its one submit listener
 * from the stable app root.
 */
function renderInlineCommentReply(threadId: string, writable: boolean): string {
  if (!writable) return "";
  const id = escapeHtml(threadId);
  return `<form class="inline-comment-reply" data-comment-reply-form data-thread-id="${id}">
    <label>Reply<textarea name="body" rows="3" required aria-label="Reply to comment" placeholder="Write a reply"></textarea></label>
    <div class="thread-actions"><button type="submit">Reply</button><button type="button" data-action="cancel-comment-reply" data-id="${id}">Cancel</button></div>
  </form>`;
}

/**
 * An orphan is evidence, not a nearby live target.  `anchor_label` is
 * intentionally truncated for ordinary list labels, but the durable quote and
 * context are the reason this thread was retained at all.  Show both exact
 * source strings as escaped, read-only text instead of reducing the quote to
 * that label or turning either one into a guessed navigation control.
 */
function renderOrphanedCommentAnchor(thread: AppDocument["comments"][number]): string {
  const quote = thread.orphaned_quote ?? thread.anchor_label;
  const context = thread.orphaned_context ?? "";
  const warning = thread.orphaned_warning ?? "The original target was deleted.";
  return `<section class="comment-orphaned-anchor" aria-label="Deleted comment anchor">
    <p class="anchor">This comment's original target was deleted and cannot be navigated.</p>
    <p class="anchor meta">${escapeHtml(warning)}</p>
    <p class="anchor"><span class="meta">Deleted text:</span> ${escapeHtml(quote)}</p>
    <p class="anchor"><span class="meta">Original context:</span> ${escapeHtml(context)}</p>
  </section>`;
}

/**
 * Deleted comments are durable state but are deliberately not shown as live
 * conversation text.  Keep restoration discoverable from that state, without
 * requiring an optional provenance projection or exposing the removed body.
 */
function renderDeletedCommentRestorations(
  thread: AppDocument["comments"][number],
  writable: boolean,
): string {
  const deleted = thread.comments.filter((comment) => comment.deleted);
  if (deleted.length === 0) return "";
  const count = `${deleted.length} deleted comment${deleted.length === 1 ? "" : "s"}`;
  return `<section class="comment-deleted" aria-label="${count}"><p class="anchor">${count}</p>${writable ? `<div class="thread-actions">${deleted
    .map(
      (comment) => `<button type="button" data-action="restore-comment" data-id="${escapeHtml(thread.id)}" data-comment-id="${escapeHtml(comment.id)}" aria-label="Restore deleted comment by ${escapeHtml(comment.author)}">Restore comment by ${escapeHtml(comment.author)}</button>`,
    )
    .join("")}</div>` : ""}</section>`;
}

/**
 * Render the immutable audit trail underneath the one review conversation it
 * describes.  This deliberately has no action attributes: provenance is
 * evidence, not an alternative route for changing/deleting a comment.  The
 * model's logical timestamps are not wall-clock values, so presenting them as
 * a date would manufacture information offline replicas do not possess.
 */
function renderCommentProvenance(
  thread: AppDocument["comments"][number],
  history: AppDocument["comment_history"],
): string {
  const entries = history
    .filter((entry) => entry.thread_id === thread.id)
    .slice()
    .sort((left, right) => left.at_ms - right.at_ms || left.comment_id.localeCompare(right.comment_id) || left.kind.localeCompare(right.kind));
  if (entries.length === 0) return "";
  const eventLabel = (kind: string): string => {
    switch (kind) {
      case "edited":
        return "Edited";
      case "deleted":
        return "Deleted";
      case "restored":
        return "Restored";
      default:
        // The core validates this closed vocabulary. Keep a safe fallback so
        // an older/newer projection never becomes an HTML injection surface.
        return "Changed";
    }
  };
  const priorText = (entry: AppDocument["comment_history"][number]): string =>
    entry.previous_body?.map((inline) => inline.text).join("") ?? "";
  const commentFor = (commentId: string) => thread.comments.find((comment) => comment.id === commentId);
  return `<details class="comment-provenance" aria-label="History for ${escapeHtml(thread.anchor_label)}"><summary>History (${entries.length})</summary><ol aria-label="Comment history for ${escapeHtml(thread.anchor_label)}">${entries
    .map(
      (entry) => {
        const comment = commentFor(entry.comment_id);
        const commentLabel = comment ? `comment by ${comment.author}` : "a removed comment";
        const goToComment = comment && !comment.deleted
          ? ` <button type="button" class="comment-history-go" data-action="comment-history-go" data-thread-id="${escapeHtml(thread.id)}" data-comment-id="${escapeHtml(comment.id)}" aria-label="View current ${escapeHtml(commentLabel)}">View current comment</button>`
          : "";
        return `<li data-comment-history-id="${escapeHtml(entry.comment_id)}"><span class="comment-history-event">${escapeHtml(eventLabel(entry.kind))}</span> ${escapeHtml(commentLabel)} by <strong>${escapeHtml(entry.actor)}</strong> <span class="meta">at logical time ${entry.at_ms}</span>${goToComment}${entry.previous_body ? `<p class="comment-history-prior"><span class="meta">Previous text:</span> ${escapeHtml(priorText(entry))}</p>` : ""}</li>`;
      },
    )
    .join("")}</ol></details>`;
}

/** A document-local, read-only activity projection. It deliberately makes no
 * delivery/read/recipient claim and retains records whose target is now a
 * tombstone. */
function renderCommentActivity(doc: AppDocument): string {
  const entries = (doc.comment_activity ?? []).slice().sort((left, right) =>
    right.at_ms - left.at_ms || right.operation_actor.localeCompare(left.operation_actor) || right.operation_seq - left.operation_seq,
  );
  if (entries.length === 0) return "";
  const label: Record<AppDocument["comment_activity"][number]["kind"], string> = {
    thread_created: "Created thread",
    reply_added: "Added reply",
    thread_resolved: "Resolved thread",
    thread_reopened: "Reopened thread",
    thread_deleted: "Deleted thread",
    thread_restored: "Restored thread",
    comment_edited: "Edited comment",
    comment_deleted: "Deleted comment",
    comment_restored: "Restored comment",
    action_set: "Updated action",
    reaction_added: "Added reaction",
    reaction_removed: "Removed reaction",
  };
  return `<details class="comment-provenance comment-activity" aria-label="Document-local comment activity"><summary>Activity (${entries.length})</summary><ol aria-label="Comment activity">${entries.map((entry) => {
    const thread = doc.comments.find((candidate) => candidate.id === entry.thread_id);
    const target = thread?.deleted ? "deleted thread" : thread ? "live thread" : "unavailable thread";
    // Activity is immutable evidence, but live targets can still provide a
    // useful return path into the current review conversation. A tombstone is
    // deliberately just evidence: it must not look like a restore action.
    const liveComment = entry.comment_id && thread && !thread.deleted
      ? thread.comments.find((comment) => comment.id === entry.comment_id && !comment.deleted)
      : undefined;
    const liveThread = thread && !thread.deleted;
    const jump = liveThread
      ? ` <button type="button" class="comment-history-go" data-action="comment-activity-go" data-thread-id="${escapeHtml(thread.id)}"${liveComment ? ` data-comment-id="${escapeHtml(liveComment.id)}"` : ""} aria-label="${escapeHtml(liveComment ? "View current comment for this activity" : "View current thread for this activity")}">View current ${liveComment ? "comment" : "thread"}</button>`
      : "";
    return `<li data-comment-activity-operation="${escapeHtml(`${entry.operation_actor}:${entry.operation_seq}`)}"><span class="comment-history-event">${escapeHtml(label[entry.kind])}</span> on ${escapeHtml(target)} by <strong>${escapeHtml(entry.actor)}</strong> <span class="meta">at logical time ${entry.at_ms}</span>${jump}</li>`;
  }).join("")}</ol></details>`;
}

/**
 * Action due dates are stored as UTC calendar midnights. Rendering through
 * `toLocaleDateString()` makes the same persisted day show as yesterday in a
 * west-of-UTC browser, so the review panel deliberately shows the canonical
 * UTC calendar date instead. The defensive invalid branch keeps a malformed
 * old projection from breaking the whole comments panel.
 */
function renderActionDueDate(atMs: number): string {
  const date = new Date(atMs);
  if (!Number.isFinite(date.getTime())) return "due date unavailable";
  const day = date.toISOString().slice(0, 10);
  return `<time datetime="${day}" aria-label="Due ${day} UTC">due ${day} UTC</time>`;
}

/**
 * The visible action control is intentionally compact, but a review panel
 * commonly contains many identical controls.  Include the durable assignee
 * and completion state in its accessible name so a reader can tell which
 * action item it is about without having to navigate backward through every
 * comment in the thread.  This remains presentation only: it never decides
 * whether an action is permitted.
 */
function actionItemButtonLabel(thread: AppDocument["comments"][number]): string {
  const author = thread.comments.find((comment) => !comment.deleted)?.author ?? "this comment thread";
  if (!thread.action_assignee) return `Assign action item for comment by ${author}`;
  const state = thread.action_completed_by ? `completed by ${thread.action_completed_by}` : "open";
  return `Manage ${state} action item assigned to ${thread.action_assignee} for comment by ${author}`;
}

type OutlineNode = { id: string; text: string; level: number; children: OutlineNode[] };

/**
 * A heading label is navigation text, not an internal serialization of its
 * inline sequence.  Footnote references and page-number fields deliberately
 * project opaque durable IDs (or no text) in the command DTO because their
 * visible values depend on where the document is rendered.  Including that
 * fallback in the outline leaks an implementation identifier into a reader's
 * navigation landmark.  Neither contributes title text, so omit both here.
 */
function outlineLabel(content: { kind: string; text: string }[]): string {
  return content
    .filter((inline) => inline.kind !== "footnote-ref" && inline.kind !== "page-number")
    .map((inline) => inline.text)
    .join("")
    .trim() || "Untitled heading";
}

/**
 * Build an actual outline tree from heading levels rather than using CSS
 * indentation on one flat list.  A document is allowed to skip a heading
 * level; attach that heading one level below the preceding visible ancestor,
 * which keeps the navigation tree valid and avoids manufacturing missing
 * headings solely for presentation.
 */
function renderOutline(headings: { id: string; level?: number; content: { kind: string; text: string }[] }[]): string {
  const root: OutlineNode = { id: "", text: "", level: 0, children: [] };
  const ancestors: OutlineNode[] = [root];
  for (const heading of headings) {
    const requestedLevel = Math.max(1, Math.min(6, heading.level ?? 1));
    while (ancestors.length > 1 && ancestors.at(-1)!.level >= requestedLevel) ancestors.pop();
    const parent = ancestors.at(-1)!;
    const node: OutlineNode = {
      id: heading.id,
      text: outlineLabel(heading.content),
      level: Math.min(requestedLevel, parent.level + 1),
      children: [],
    };
    parent.children.push(node);
    ancestors.push(node);
  }
  const renderNodes = (nodes: OutlineNode[]): string => `<ol>${nodes.map((node) =>
    `<li><button type="button" data-action="outline-go" data-id="${escapeHtml(node.id)}">${escapeHtml(node.text)}</button>${node.children.length ? renderNodes(node.children) : ""}</li>`,
  ).join("")}</ol>`;
  return renderNodes(root.children);
}
