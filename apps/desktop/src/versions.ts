// The version-history panel.
//
// Owns the whole version-history surface: Rust computes the history, the
// read-only preview and the diff, this only renders them and maps clicks.
// Every control routes through the delegated `version:` prefix in `runAction`,
// so nothing here attaches a listener that a re-render could stack.
import { confirmDialog, escapeHtml, promptDialog, toast } from "./ui";
import type { AppVersionView } from "./generated/version";
import { state } from "./state";
import { run } from "./shared";
import { renderSidePanel } from "./panels";

let versionView: AppVersionView | null = null;
let versionLoading = false;
let versionCompareFrom: string | null = null;

function shortManifest(manifest: string): string {
  const digest = manifest.includes(":") ? manifest.slice(manifest.indexOf(":") + 1) : manifest;
  return digest.slice(0, 12);
}

function versionTitle(version: AppVersionView["versions"][number]): string {
  return version.label ?? shortManifest(version.manifest);
}

export async function loadVersions(): Promise<void> {
  if (!state.doc?.repository_root) {
    versionView = null;
    renderSidePanel();
    return;
  }
  versionLoading = true;
  renderSidePanel();
  try {
    versionView = await run("list_document_versions", {});
  } catch {
    versionView = null;
  } finally {
    versionLoading = false;
    renderSidePanel();
  }
}

function renderVersionPreview(preview: NonNullable<AppVersionView["preview"]>): string {
  // Rust already reduced each block to a kind label and the text it says —
  // which of the block's content, its equation source or its alt text stands
  // in for it is a projection rule, and it is the same one the version diff
  // uses. The only decision left here is how many lines to show.
  const blocks = preview.blocks
    .slice(0, 60)
    .map((block) => `<li><span class="meta">${escapeHtml(block.kind)}</span> ${escapeHtml(block.text)}</li>`)
    .join("");
  return `
    <article class="version-preview" ${preview.read_only ? 'data-read-only="true"' : ""}>
      <p class="version-banner">${preview.read_only ? "Read-only preview" : "Preview"} · ${escapeHtml(shortManifest(preview.manifest))}</p>
      <p class="meta">${escapeHtml(preview.document.title)}</p>
      <ol class="version-preview-blocks">${blocks || `<li class="empty">This version has no blocks.</li>`}</ol>
      <div class="thread-actions">
        <button type="button" data-action="version:restore" data-manifest="${escapeHtml(preview.manifest)}">Restore this version…</button>
        <button type="button" data-action="version:close-preview">Close preview</button>
      </div>
    </article>`;
}

function renderVersionDiff(diff: NonNullable<AppVersionView["diff"]>): string {
  const entries = diff.entries
    .slice(0, 200)
    .map(
      (entry) => `
      <li class="version-diff-entry" data-change="${escapeHtml(entry.change)}">
        <p><span class="version-change">${escapeHtml(entry.change)}</span> <span class="meta">${escapeHtml(entry.kind)} · ${escapeHtml(entry.path)}</span></p>
        ${entry.before_text ? `<p class="version-before">${escapeHtml(entry.before_text)}</p>` : ""}
        ${entry.after_text ? `<p class="version-after">${escapeHtml(entry.after_text)}</p>` : ""}
      </li>`,
    )
    .join("");
  return `
    <article class="version-diff">
      <p class="version-banner">${escapeHtml(shortManifest(diff.from_manifest))} → ${escapeHtml(shortManifest(diff.to_manifest))}</p>
      <p class="meta">${diff.added} added · ${diff.removed} removed · ${diff.changed} changed</p>
      <ol class="version-diff-entries">${entries || `<li class="empty">No durable document changes.</li>`}</ol>
      <div class="thread-actions"><button type="button" data-action="version:close-diff">Close comparison</button></div>
    </article>`;
}

export function renderVersionsPanel(): string {
  if (!state.doc?.repository_root) {
    return `<p class="empty">Save this document to a repository to keep version history.</p>`;
  }
  if (versionLoading && !versionView) {
    return `<p class="empty">Reading version history…</p>`;
  }
  if (!versionView) {
    return `
      <div class="panel-actions"><button type="button" class="panel-action" data-action="version:refresh">Reload versions</button></div>
      <p class="empty">Version history is unavailable.</p>`;
  }
  const warnings = versionView.warnings
    .map((warning) => `<li><code>${escapeHtml(warning.code)}</code> ${escapeHtml(warning.message)}</li>`)
    .join("");
  const versions = versionView.versions
    .map((version) => {
      const badges = [
        version.is_head ? `<span class="version-badge">head</span>` : "",
        version.is_current ? `<span class="version-badge">open</span>` : "",
        version.snapshot_present ? "" : `<span class="version-badge version-badge-warn">snapshot missing</span>`,
        versionCompareFrom === version.manifest ? `<span class="version-badge">comparing from</span>` : "",
      ].join("");
      const signers = version.signers.length
        ? version.signers.map((signer) => escapeHtml(signer.signer_display || signer.signer)).join(", ")
        : "unsigned";
      return `
      <article class="version" data-manifest="${escapeHtml(version.manifest)}">
        <p><strong>${escapeHtml(versionTitle(version))}</strong> ${badges}</p>
        <p class="meta">${escapeHtml(new Date(version.created_at_ms).toLocaleString())} · ${signers}</p>
        ${version.label ? `<p class="meta">named by ${escapeHtml(version.label_author ?? "")}</p>` : ""}
        <div class="thread-actions">
          <button type="button" data-action="version:preview" data-manifest="${escapeHtml(version.manifest)}"${version.snapshot_present ? "" : " disabled"}>Preview</button>
          <button type="button" data-action="version:compare" data-manifest="${escapeHtml(version.manifest)}"${version.snapshot_present ? "" : " disabled"}>${versionCompareFrom === version.manifest ? "Cancel compare" : versionCompareFrom ? "Compare with this" : "Compare from"}</button>
          <button type="button" data-action="version:name" data-manifest="${escapeHtml(version.manifest)}">Name…</button>
          <button type="button" data-action="version:restore" data-manifest="${escapeHtml(version.manifest)}"${version.snapshot_present && !version.is_current ? "" : " disabled"}>Restore…</button>
        </div>
      </article>`;
    })
    .join("");
  return `
    <div class="panel-actions"><button type="button" class="panel-action" data-action="version:refresh">Reload versions</button></div>
    <p class="meta">${versionView.versions.length} version${versionView.versions.length === 1 ? "" : "s"} on ${escapeHtml(versionView.branch)}${versionView.truncated ? " · older versions not shown" : ""}</p>
    ${warnings ? `<ul class="warnings">${warnings}</ul>` : ""}
    ${versionView.preview ? renderVersionPreview(versionView.preview) : ""}
    ${versionView.diff ? renderVersionDiff(versionView.diff) : ""}
    ${versions || `<p class="empty">No committed versions yet.</p>`}`;
}

export async function runVersionAction(action: string, data: DOMStringMap): Promise<void> {
  const manifest = data.manifest ?? "";
  switch (action) {
    case "refresh":
      versionCompareFrom = null;
      await loadVersions();
      break;
    case "close-preview":
    case "close-diff":
      if (versionView) {
        if (action === "close-preview") versionView.preview = null;
        else versionView.diff = null;
        renderSidePanel();
      }
      break;
    case "preview": {
      if (!manifest) break;
      try {
        versionView = await run("open_document_at_version", { manifest });
      } catch {
        // `run` already surfaced the message in the status bar.
      }
      renderSidePanel();
      break;
    }
    case "compare": {
      if (!manifest) break;
      if (!versionCompareFrom || versionCompareFrom === manifest) {
        versionCompareFrom = versionCompareFrom === manifest ? null : manifest;
        renderSidePanel();
        break;
      }
      const fromManifest = versionCompareFrom;
      versionCompareFrom = null;
      try {
        versionView = await run("diff_document_versions", { fromManifest, toManifest: manifest });
      } catch {
        // `run` already surfaced the message in the status bar.
      }
      renderSidePanel();
      break;
    }
    case "name": {
      if (!manifest) break;
      const existing = versionView?.versions.find((version) => version.manifest === manifest);
      const answer = await promptDialog({
        title: "Name this version",
        fields: [
          { name: "label", label: `Label for ${shortManifest(manifest)}`, value: existing?.label ?? "" },
        ],
        submit: "Save name",
      });
      const label = answer?.label.trim();
      if (!label) break;
      try {
        versionView = await run("name_document_version", { manifest, label, author: state.authorName });
        toast("Version named");
      } catch {
        // `run` already surfaced the message in the status bar.
      }
      renderSidePanel();
      break;
    }
    case "restore": {
      if (!manifest) break;
      const existing = versionView?.versions.find((version) => version.manifest === manifest);
      const confirmed = await confirmDialog(
        "Restore this version",
        `Commit "${existing ? versionTitle(existing) : shortManifest(manifest)}" as a new version. Nothing is rewritten, but unsaved changes are discarded.`,
        "Restore",
      );
      if (!confirmed) break;
      try {
        await run("restore_document_version", { manifest });
        toast("Restored as a new version");
      } catch {
        break;
      }
      versionCompareFrom = null;
      await loadVersions();
      break;
    }
  }
}

/** The badge count the side strip shows, without exposing the view itself. */
export function versionCount(): number {
  return versionView?.versions.length ?? 0;
}

/**
 * Lets a browser-level test render the version panel against a known view; the
 * WASM shell has no filesystem repository to read a real one from.
 */
export function setVersionView(next: AppVersionView | null): void {
  versionView = next;
  // A version view only ever exists for a repository-backed document, so
  // mirror that onto the in-memory document; otherwise the panel renders its
  // "save this document to a repository" branch instead of the history.
  if (next && state.doc) {
    state.doc.repository_root = next.repository_root;
    state.doc.repository_backend = next.repository_backend;
  }
  renderSidePanel();
}
