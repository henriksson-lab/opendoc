// The home screen: what OpenDoc shows when no document is open in the editor.
import { escapeHtml } from "./ui";
import { app, runtime, state } from "./state";

export function renderHome(): void {
  const recents = state.doc?.recent_documents ?? [];
  app.innerHTML = `
    <main class="shell home-shell">
      <header class="home-topbar"><h1>OpenDoc</h1><span class="runtime-label">${escapeHtml(state.profile?.label ?? runtime.mode)}</span></header>
      <section class="home-panel">
        <h2>Start a new document</h2>
        <div class="home-actions">
          <button type="button" class="tile" data-action="new-document"><span class="tile-preview doc"></span><span>Blank document</span></button>
          <button type="button" class="tile" data-action="new-spreadsheet"><span class="tile-preview sheet"></span><span>Blank spreadsheet</span></button>
          <button type="button" class="tile" data-action="import-word"><span class="tile-preview import"></span><span>Import Word (.docx)</span></button>
          <button type="button" class="tile" data-action="open-repository"><span class="tile-preview folder"></span><span>Open folder…</span></button>
        </div>
      </section>
      <section class="home-panel">
        <h2>Recent documents</h2>
        ${
          recents.length === 0
            ? `<p class="empty">No recent documents yet.</p>`
            : `<ul class="recent-list">${recents
                .map(
                  (recent) => `<li><button type="button" data-action="open-recent" data-uuid="${escapeHtml(recent.uuid)}" data-root="${escapeHtml(recent.repository_root ?? "")}" data-backend="${escapeHtml(recent.repository_backend ?? "")}" data-namespace="${escapeHtml(recent.repository_namespace ?? "")}">
                    <span class="recent-title">${escapeHtml(recent.title || "Untitled document")}</span>
                    <span class="recent-meta">${escapeHtml(recent.repository_root ?? "")}</span>
                  </button></li>`,
                )
                .join("")}</ul>`
        }
      </section>
    </main>`;
  // No per-node listeners here: bindStatic() delegates [data-action] clicks for
  // the whole app. Binding both made every home action fire twice.
}
