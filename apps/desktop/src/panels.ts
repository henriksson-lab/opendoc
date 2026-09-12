// The side panels: comments, suggestions, citations, footnotes, attachments,
// history, signatures, versions and warnings.
//
// Every control inside a panel is a `data-action` button routed by the
// delegated dispatcher, so a re-render can never stack a listener. The version
// panel is large enough to own its own module; this one asks it for its markup
// and its badge count.
import { escapeHtml } from "./ui";
import type { AppWarning } from "./types";
import type { Panel } from "./state";
import { state } from "./state";
import { query } from "./shared";
import { renderVersionsPanel, versionCount } from "./versions";
import { exportWarningReport } from "./files";

export const PANELS: { id: Panel; label: string; icon: string }[] = [
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
  const strip = query("[data-side-strip]");
  const side = query("[data-side-panel]");
  const doc = state.doc;
  if (!strip || !side || !doc) return;
  const counts: Record<Panel, number> = {
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
  side.innerHTML = `<header class="panel-header"><h2>${escapeHtml(PANELS.find((item) => item.id === panel)?.label ?? "")}</h2><button type="button" data-action="toggle-panel:${panel}" aria-label="Close">✕</button></header><div class="panel-body">${body}</div>`;
}

function renderPanelBody(which: Panel): string {
  const doc = state.doc;
  if (!doc) return "";
  switch (which) {
    case "comments": {
      const threads = doc.comments.filter((thread) => !thread.deleted);
      return `
        <button type="button" class="panel-action" data-action="comment">Add comment on selection</button>
        ${threads.length === 0 ? `<p class="empty">No comments yet.</p>` : ""}
        ${threads
          .map(
            (thread) => `
          <article class="thread" data-thread="${escapeHtml(thread.id)}">
            <p class="anchor">${escapeHtml(thread.anchor_label)}</p>
            ${thread.comments
              .filter((comment) => !comment.deleted)
              .map((comment) => `<div class="comment"><strong>${escapeHtml(comment.author)}</strong><p>${escapeHtml(comment.body)}</p></div>`)
              .join("")}
            <div class="thread-actions">
              <button type="button" data-action="reply-comment" data-id="${escapeHtml(thread.id)}">Reply</button>
              <button type="button" data-action="resolve-comment" data-id="${escapeHtml(thread.id)}">Resolve</button>
            </div>
          </article>`,
          )
          .join("")}`;
    }
    case "suggestions": {
      const items = doc.suggestions.filter((item) => item.state === "proposed");
      return `
        <div class="panel-actions"><button type="button" class="panel-action" data-action="suggest">Suggest replacement…</button><button type="button" class="panel-action" data-action="suggest-delete">Suggest deletion</button></div>
        ${items.length > 1 ? `<div class="panel-actions"><button type="button" data-action="accept-all">Accept all</button><button type="button" data-action="reject-all">Reject all</button></div>` : ""}
        ${items.length === 0 ? `<p class="empty">No open suggestions.</p>` : ""}
        ${items
          .map(
            (item) => `
          <article class="suggestion">
            <p><strong>${escapeHtml(item.author)}</strong> · ${escapeHtml(item.kind)}</p>
            ${item.anchor_label ? `<p class="anchor">${escapeHtml(item.anchor_label)}</p>` : ""}
            <p>${escapeHtml(item.text)}</p>
            <div class="thread-actions">
              <button type="button" data-action="accept-suggestion" data-id="${escapeHtml(item.id)}">Accept</button>
              <button type="button" data-action="reject-suggestion" data-id="${escapeHtml(item.id)}">Reject</button>
            </div>
          </article>`,
          )
          .join("")}`;
    }
    case "citations":
      return `
        <div class="panel-actions"><button type="button" class="panel-action" data-action="add-reference">Add reference…</button><button type="button" class="panel-action" data-action="import-bibtex">Import BibTeX/RIS…</button></div>
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
      const notes = doc.footnotes.filter((note) => !note.deleted);
      return `
        <button type="button" class="panel-action" data-action="insert-footnote">Insert footnote at caret</button>
        ${notes.length === 0 ? `<p class="empty">No footnotes.</p>` : ""}
        ${notes
          .map(
            (note, index) => `<article class="footnote"><p><sup>${index + 1}</sup> ${escapeHtml(note.body.map((inline) => inline.text).join(""))}</p><button type="button" data-action="edit-footnote" data-id="${escapeHtml(note.id)}">Edit</button></article>`,
          )
          .join("")}`;
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
