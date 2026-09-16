// Collaboration: the connection, the connection state, and who else is here.
//
// This module owns a socket and a DOM region. It does not own a single
// document fact. Every frame that arrives is handed to the Rust driver
// verbatim (`crates/opendoc-wasm/src/collab.rs` in the browser, the service
// crate's own client behind `src-tauri` in the native shell) and every frame
// that leaves was composed there. What is left here is what TypeScript is for:
// opening the socket, retrying it, reading the caret out of the DOM, and
// drawing a pill and some chips. `docs/adr/0018` is the decision.
//
// Two rules shape the code more than anything else:
//
// 1. **Listener stacking is the local bug.** There is exactly one delegated
//    click listener, bound once on `app` (which the page creates and never
//    replaces). Every control in the region is a `data-collab-action`
//    attribute, so a re-render can never attach a second handler. A socket's
//    own handlers are *assignments* on a freshly created socket, and the
//    timers are cleared before they are set.
// 2. **The service decides; this renders the answer.** Role, peers, actor and
//    the acknowledged watermark come back from Rust, which got them from the
//    service's frames. Nothing here computes them, and a refusal is shown in
//    the service's own words rather than translated into optimism.
import { escapeHtml, confirmDialog, promptDialog, toast } from "./ui";
import { collabCore, invoke, isTauri, nativeCommand, onNativeEvent } from "./invoke";
import type { CollabCore } from "./invoke";
import { app, runtime, state } from "./state";
import type { EditorSelection, OpenDocPresencePeer, OpenDocServiceRole } from "./types";
import { applyDocument } from "./shared";

// ---- What Rust hands back ------------------------------------------------
//
// Mirrors `collab::CollabStatus` and `OpenDocServiceSession`. Hand-written
// because this is not a command and so not in the generated contract; the
// field names are pinned from the other side by
// `the_status_serializes_with_the_fields_the_frontend_reads`.

export type CollabPhase = "idle" | "connecting" | "live" | "reconnecting" | "closed";

export type CollabNotice = {
  kind: string;
  message: string;
  resumable: boolean;
};

export type CollabPeer = {
  subject: string;
  actor: string;
  display_name: string;
  role: string;
  cursor_anchor: string | null;
  selection_anchor: string | null;
  last_seen_ms: number;
  connections: number;
};

export type CollabSession = {
  subject: string;
  actor: string;
  document_uuid: string;
  role: string;
  peers: CollabPeer[];
  acknowledged_seq: number;
};

export type CollabStatus = {
  phase: CollabPhase;
  document_uuid: string;
  display_name: string;
  commit_seq: number;
  acknowledged_seq: number;
  pending_operations: number;
  can_submit: boolean;
  /**
   * Rust asking for a fresh welcome: the service refused a batch, or a commit
   * could not be applied, and the session cannot go on over the socket it has.
   *
   * Only the browser path sets it — Rust owns the frames there and this module
   * owns the socket, so this is the one thing Rust cannot do for itself. The
   * native shell owns its own socket and reconnects without asking. Read once
   * on the Rust side, so acting on it twice is not possible.
   */
  reconnect_requested: boolean;
  document_changed: boolean;
  // The caret, rebased by Rust across the remote edit that just landed. Taken
  // once on read, exactly as `document_changed` is, so a later tick cannot
  // drag it back.
  selection: EditorSelection | null;
  notice: CollabNotice | null;
  session: CollabSession | null;
};

export type ConnectOptions = {
  serviceUrl: string;
  subject: string;
  apiKey: string;
  /** Blank creates a new document on the service from the open one's title. */
  documentUuid?: string;
  displayName?: string;
};

const IDLE: CollabStatus = {
  phase: "idle",
  document_uuid: "",
  display_name: "",
  commit_seq: 0,
  acknowledged_seq: 0,
  pending_operations: 0,
  can_submit: false,
  reconnect_requested: false,
  document_changed: false,
  selection: null,
  notice: null,
  session: null,
};

/** How often outbound work is swept up. See `flush`. */
const PUMP_MS = 250;

// Connection hints improve the ordinary reconnect path, but credentials must
// never share that storage boundary. In particular, an API key is deliberately
// neither read nor written here.
const RECENT_SERVICE_URL_KEY = "opendoc.collab.recent-service-url.v1";
const RECENT_SUBJECT_KEY = "opendoc.collab.recent-subject.v1";
/** A reconnect hint is UI convenience, not an unbounded user-input cache. */
const MAX_RECENT_CONNECTION_HINT_LENGTH = 2048;

/** Backoff for a dropped socket, in milliseconds, then the last value repeats. */
const RETRY_MS = [500, 1000, 2000, 4000];

/**
 * How many times a dropped socket is retried before the session is declared
 * over.
 *
 * There is a cap because a browser cannot read *why* a WebSocket handshake
 * failed — the HTTP status of a refused upgrade is deliberately hidden from
 * the page — so an expired session, a revoked grant and an unplugged cable are
 * one event here. Retrying for ever would show "reconnecting…" against a
 * service that will never answer, which is the dishonest version. After the
 * cap the pill says the session is over and how much work never left.
 */
const MAX_ATTEMPTS = 8;

let status: CollabStatus = IDLE;
let socket: WebSocket | null = null;
let pumpTimer: number | null = null;
let retryTimer: number | null = null;
let attempt = 0;
let flushing = false;
let listenerBound = false;
let nativeStatusBound = false;
let presenceOverlayFrame: number | null = null;
let presenceOverlayListenersBound = false;
/**
 * Presence changes are useful to a screen-reader user, but a busy document
 * must not speak every join/reconnect unless that user asked for it. This is
 * deliberately browser view state, not a document or service setting: one
 * reader muting announcements must not change what anybody else hears.
 */
const PRESENCE_ANNOUNCEMENTS_KEY = "opendoc.presence-announcements";
let presenceAnnouncementsEnabled = readPresenceAnnouncementsPreference();
let announcedPresenceSession: string | null = null;
let announcedPeers = new Map<string, string>();
/** What a reconnect needs. `null` means nobody asked to be connected. */
let wanted: { socketUrl: string; documentUuid: string; displayName: string } | null = null;
/**
 * The browser's short-lived service credential, retained only while its
 * collaboration session is live. It is deliberately not put in a document,
 * URL, localStorage, or the share link: grants are server state and a bearer
 * token is not an invitation.
 */
let sharingSession: { base: string; token: string; documentUuid: string } | null = null;
/** The last thing that went wrong outside Rust: a refused fetch, a bad URL. */
let localNotice: CollabNotice | null = null;
/**
 * The author name this document had before a session attested a better one.
 *
 * Restored on disconnect, so leaving a session does not leave the local
 * document signing comments with a subject that no longer means anything here.
 */
let authorNameBeforeSession: string | null = null;

// ---- The region ----------------------------------------------------------

const PHASE_LABEL: Record<CollabPhase, string> = {
  idle: "Not shared",
  connecting: "Connecting…",
  live: "Live",
  reconnecting: "Reconnecting…",
  closed: "Disconnected",
};

function initials(name: string): string {
  const parts = name.trim().split(/\s+/).filter(Boolean);
  if (parts.length === 0) return "?";
  if (parts.length === 1) return parts[0].slice(0, 2).toUpperCase();
  return (parts[0][0] + parts[parts.length - 1][0]).toUpperCase();
}

/**
 * A stable colour per actor, so the same person is the same colour in every
 * tab without anyone having to agree on a palette. The actor id is server
 * state, which is exactly why it is the right key: a display name a peer
 * chose could collide or change mid-session.
 */
function peerHue(actor: string): number {
  let hash = 0;
  for (const character of actor) {
    hash = (hash * 31 + character.codePointAt(0)!) % 360;
  }
  return hash;
}

function peerChip(peer: CollabPeer, self: string | null): string {
  const isSelf = peer.actor === self;
  // A non-empty wire field is not necessarily a text cursor. In particular,
  // malformed/atomic anchors have no point that Follow could safely resolve.
  // Parse before exposing an enabled consent action; click-time resolution
  // below still handles a valid anchor made stale by a concurrent edit.
  const followable = parseCursorAnchor(peer.cursor_anchor) !== null;
  const where = followable ? `, caret at ${peer.cursor_anchor}` : "";
  const tabs = peer.connections > 1 ? ` · ${peer.connections} connections` : "";
  const title = `${peer.display_name} (${peer.subject}) — ${peer.role}${tabs}${where}`;
  // `title` is a pointer hint, not a dependable accessible description. The
  // visible chip intentionally stays compact, so carry the server-attested
  // role and whether its Follow control has a real text destination in an
  // adjacent screen-reader-only phrase. Initials are decorative duplicate
  // text and must not be announced before the person's actual name.
  const accessibleStatus = `Role: ${peer.role}. ${followable ? "Current text cursor available." : "No current text cursor."}${peer.connections > 1 ? ` ${peer.connections} connections.` : ""}`;
  return `<span class="collab-peer${isSelf ? " self" : ""}" style="--peer-hue: ${peerHue(peer.actor)}" data-collab-peer="${escapeHtml(peer.actor)}" title="${escapeHtml(title)}">
    <span class="collab-peer-mark" aria-hidden="true">${escapeHtml(initials(peer.display_name))}</span>
    <span class="collab-peer-name">${escapeHtml(peer.display_name)}${isSelf ? " (you)" : ""}</span>
    <span class="sr-only"> ${escapeHtml(accessibleStatus)}</span>
    ${!isSelf ? `<button type="button" class="collab-follow" data-collab-action="follow-once" data-collab-actor="${escapeHtml(peer.actor)}"${followable ? "" : " disabled"} title="${escapeHtml(followable ? `Scroll once to ${peer.display_name}'s current cursor` : `${peer.display_name} has no current text cursor to follow`)}" aria-label="${escapeHtml(followable ? `Follow ${peer.display_name}'s cursor once` : `${peer.display_name} has no current cursor to follow`)}">Follow</button>` : ""}
    ${peer.connections > 1 ? `<span class="collab-peer-count">${peer.connections}</span>` : ""}
  </span>`;
}

function readPresenceAnnouncementsPreference(): boolean {
  try {
    // Default on: membership is meaningful collaboration context, and the
    // visible control gives the reader an immediate, persistent way to mute
    // it. Cursor moves themselves are never announced.
    return window.localStorage.getItem(PRESENCE_ANNOUNCEMENTS_KEY) !== "off";
  } catch {
    // Sandboxed and privacy-restricted browser contexts can reject storage.
    // Keep the accessible default without treating an unavailable preference
    // store as a collaboration failure.
    return true;
  }
}

function writePresenceAnnouncementsPreference(enabled: boolean): void {
  presenceAnnouncementsEnabled = enabled;
  try {
    window.localStorage.setItem(PRESENCE_ANNOUNCEMENTS_KEY, enabled ? "on" : "off");
  } catch {
    // The in-memory choice still applies for this page lifetime.
  }
}

/**
 * Returns an arrival/departure summary only for a later presence frame. The
 * first frame of a session establishes context; announcing every collaborator
 * already in the document after a reload is noise, not an event.
 */
function presenceMembershipAnnouncement(peers: CollabPeer[], self: string | null): string {
  const next = new Map(
    peers
      .filter((peer) => peer.actor !== self)
      .map((peer) => [peer.actor, peer.display_name]),
  );
  const sessionKey = status.session ? `${status.session.actor}:${status.session.document_uuid}` : null;
  if (sessionKey !== announcedPresenceSession) {
    announcedPresenceSession = sessionKey;
    announcedPeers = next;
    return "";
  }
  const joined = [...next].filter(([actor]) => !announcedPeers.has(actor)).map(([, name]) => name);
  const left = [...announcedPeers].filter(([actor]) => !next.has(actor)).map(([, name]) => name);
  announcedPeers = next;
  if (!presenceAnnouncementsEnabled || (!joined.length && !left.length)) return "";
  const describe = (names: string[]) => names.join(", ");
  const parts: string[] = [];
  if (joined.length) parts.push(`${describe(joined)} ${joined.length === 1 ? "joined" : "joined"} the document`);
  if (left.length) parts.push(`${describe(left)} ${left.length === 1 ? "left" : "left"} the document`);
  return `${parts.join(". ")}.`;
}

// ---- Remote cursors ------------------------------------------------------

/**
 * A service presence frame contains one opaque *caret* anchor, never another
 * person's selection. Keep that boundary visible in the UI: this projects a
 * cursor only, and does not invent a selection from two unrelated facts.
 *
 * The overlay is appended to `body`, rather than to `editorHost`. In
 * particular it is not a node in the contenteditable tree: a remote presence
 * update must not change the local selection, become copyable document text,
 * or be seen by the editor's keyed DOM reconciliation.
 */
const REMOTE_OVERLAY = "opendoc-remote-presence-overlay";

type ParsedCursorAnchor = { blockId: string; inlineId: string; offset: number };

/** Parse the `block:inline:offset` wire form conservatively. */
function parseCursorAnchor(value: string | null): ParsedCursorAnchor | null {
  if (!value) return null;
  const offsetSeparator = value.lastIndexOf(":");
  if (offsetSeparator < 1) return null;
  const inlineSeparator = value.lastIndexOf(":", offsetSeparator - 1);
  if (inlineSeparator < 1) return null;
  const blockId = value.slice(0, inlineSeparator);
  const inlineId = value.slice(inlineSeparator + 1, offsetSeparator);
  const offsetText = value.slice(offsetSeparator + 1);
  // An absent inline id is a valid local selection for an atomic block, but
  // there is deliberately no textual point to paint there. Do not guess.
  if (!blockId || !inlineId || !/^\d+$/.test(offsetText)) return null;
  const offset = Number(offsetText);
  return Number.isSafeInteger(offset) ? { blockId, inlineId, offset } : null;
}

function utf16Offset(text: string, codePoints: number): number {
  let utf16 = 0;
  let seen = 0;
  for (const character of text) {
    if (seen === codePoints) break;
    utf16 += character.length;
    seen += 1;
  }
  return utf16;
}

/**
 * Resolves a cursor to a collapsed DOM range without touching the browser
 * selection. A stale presence anchor is ordinary under concurrent edits, so
 * every failed lookup simply omits that cursor until the peer sends a newer
 * one.
 */
function cursorPoint(anchor: ParsedCursorAnchor): [Text, number] | null {
  const block = document.querySelector<HTMLElement>(`[data-block-id="${CSS.escape(anchor.blockId)}"]`);
  if (!block) return null;
  const inline = block.querySelector<HTMLElement>(`[data-inline-id="${CSS.escape(anchor.inlineId)}"]`);
  if (!inline || inline.closest("[data-block-id]") !== block) return null;
  const walker = document.createTreeWalker(inline, NodeFilter.SHOW_TEXT);
  let remaining = anchor.offset;
  let text: Text | null;
  while ((text = walker.nextNode() as Text | null)) {
    const length = Array.from(text.data).length;
    if (remaining <= length) {
      return [text, utf16Offset(text.data, remaining)];
    }
    remaining -= length;
  }
  return null;
}

function cursorRect(anchor: ParsedCursorAnchor): DOMRect | null {
  const point = cursorPoint(anchor);
  if (!point) return null;
  const range = document.createRange();
  range.setStart(...point);
  range.collapse(true);
  const rect = range.getClientRects()[0] ?? range.getBoundingClientRect();
  return rect.width || rect.height ? rect : null;
}

/**
 * Follow is deliberately one explicit scroll, never a background navigation
 * subscription. That keeps the local reader in control: a collaborator can
 * expose a cursor but cannot move another person's viewport merely by typing.
 * A cursor can become stale between rendering its button and this click, so
 * resolution happens again here and failure is a visible no-op.
 */
function followPeerOnce(actor: string): void {
  const session = status.session;
  if (!session || status.phase !== "live") {
    toast("Collaboration is no longer connected.");
    return;
  }
  const peer = session?.peers.find((candidate) => candidate.actor === actor);
  if (!peer || peer.actor === session.actor) {
    // A service frame can remove a peer after its Follow button was rendered
    // but before this delegated click runs. This is a local no-op, but it
    // must not be silent: the old control described a real person a moment
    // ago, and guessing a replacement destination would move the reader.
    toast("That collaborator is no longer available.");
    return;
  }
  const anchor = parseCursorAnchor(peer.cursor_anchor);
  const point = anchor && cursorPoint(anchor);
  const inline = point?.[0].parentElement?.closest<HTMLElement>("[data-inline-id]");
  if (!inline) {
    toast(`${peer.display_name}'s cursor is no longer available.`);
    return;
  }
  inline.scrollIntoView({ block: "center", inline: "nearest", behavior: "auto" });
  toast(`Followed ${peer.display_name}'s current cursor.`);
}

/**
 * Returns the actual painted rectangles of a remote text selection. Endpoints
 * must both resolve to current rendered text. A reversed browser selection is
 * normal; an endpoint made stale by an edit is not repaired or guessed.
 */
function selectionRects(anchor: ParsedCursorAnchor, focus: ParsedCursorAnchor): DOMRect[] {
  const start = cursorPoint(anchor);
  const end = cursorPoint(focus);
  if (!start || !end || (start[0] === end[0] && start[1] === end[1])) return [];
  const range = document.createRange();
  range.setStart(...start);
  range.setEnd(...end);
  if (range.collapsed) {
    range.setStart(...end);
    range.setEnd(...start);
  }
  if (range.collapsed) return [];
  return [...range.getClientRects()].filter((rect) => rect.width > 0 && rect.height > 0);
}

function presenceOverlay(): HTMLElement {
  let overlay = document.getElementById(REMOTE_OVERLAY);
  if (!overlay) {
    overlay = document.createElement("div");
    overlay.id = REMOTE_OVERLAY;
    overlay.className = "remote-presence-overlay";
    overlay.dataset.remotePresenceOverlay = "true";
    // A cursor is useful visual context, not an announcement every 250 ms.
    // The named collaborator remains available in the existing peer list.
    overlay.setAttribute("aria-hidden", "true");
    document.body.appendChild(overlay);
  }
  return overlay;
}

function renderRemotePresenceNow(): void {
  presenceOverlayFrame = null;
  const overlay = presenceOverlay();
  overlay.replaceChildren();
  if (status.phase !== "live" || !status.session || state.mode !== "docs") return;
  for (const peer of status.session.peers) {
    if (peer.actor === status.session.actor) continue;
    const parsed = parseCursorAnchor(peer.cursor_anchor);
    const rect = parsed && cursorRect(parsed);
    const selection = parseCursorAnchor(peer.selection_anchor);
    if (parsed && selection) {
      for (const selectionRect of selectionRects(selection, parsed)) {
        const highlight = document.createElement("span");
        highlight.className = "remote-selection";
        highlight.dataset.remoteSelection = peer.actor;
        highlight.style.setProperty("--peer-hue", String(peerHue(peer.actor)));
        highlight.style.left = `${Math.round(selectionRect.left)}px`;
        highlight.style.top = `${Math.round(selectionRect.top)}px`;
        highlight.style.width = `${Math.round(selectionRect.width)}px`;
        highlight.style.height = `${Math.round(selectionRect.height)}px`;
        overlay.appendChild(highlight);
      }
    }
    if (!rect) continue;
    const cursor = document.createElement("span");
    cursor.className = "remote-caret";
    cursor.dataset.remoteCaret = peer.actor;
    cursor.title = `${peer.display_name}'s cursor`;
    cursor.style.setProperty("--peer-hue", String(peerHue(peer.actor)));
    cursor.style.left = `${Math.round(rect.left)}px`;
    cursor.style.top = `${Math.round(rect.top)}px`;
    cursor.style.height = `${Math.max(12, Math.round(rect.height || 16))}px`;
    const label = document.createElement("span");
    label.className = "remote-caret-label";
    label.textContent = initials(peer.display_name);
    cursor.appendChild(label);
    overlay.appendChild(cursor);
  }
}

/** Schedules after DOM reconciliation and coalesces scroll/layout bursts. */
export function refreshRemotePresence(): void {
  if (presenceOverlayFrame !== null) return;
  presenceOverlayFrame = window.requestAnimationFrame(renderRemotePresenceNow);
}

/**
 * Drop the previous viewport projection before the document it was resolved
 * against is morphed. A later animation frame will resolve fresh anchors, but
 * a rectangle naming text that a remote operation just deleted must not remain
 * visible for that frame. This never creates the optional overlay itself.
 */
export function clearRemotePresenceOverlay(): void {
  document.getElementById(REMOTE_OVERLAY)?.replaceChildren();
}

function bindPresenceOverlayOnce(): void {
  if (presenceOverlayListenersBound) return;
  presenceOverlayListenersBound = true;
  window.addEventListener("resize", refreshRemotePresence, { passive: true });
  window.addEventListener("scroll", refreshRemotePresence, { passive: true, capture: true });
  // Pagination is deliberately isolated from collaboration. It announces only
  // that rendered geometry changed; this module remains the sole owner of the
  // overlay and re-resolves anchors on the next frame.
  window.addEventListener("opendoc:document-layout", refreshRemotePresence);
}

/**
 * Renders the collaboration region into the topbar, creating it on first call.
 *
 * Called from `renderSidePanel`, which runs at the end of every render pass,
 * so the region survives a shell rebuild without this module having to know
 * when one happened.
 */
export function renderCollaborationRegion(): void {
  bindOnce();
  bindPresenceOverlayOnce();
  const topbar = app.querySelector(".topbar-right");
  if (!topbar) return;
  let region = topbar.querySelector("[data-collab]") as HTMLElement | null;
  if (!region) {
    region = document.createElement("div");
    region.className = "collab";
    region.setAttribute("data-collab", "");
    const share = topbar.querySelector('[data-action="share"]');
    topbar.insertBefore(region, share ?? null);
  }
  const notice = localNotice ?? status.notice;
  const peers = status.session?.peers ?? [];
  const self = status.session?.actor ?? null;
  const connected = status.phase !== "idle";
  const membershipAnnouncement = presenceMembershipAnnouncement(peers, self);
  const attemptLabel = status.phase === "reconnecting" && attempt > 0 ? ` (attempt ${attempt})` : "";
  const pending = status.pending_operations > 0 ? `<span class="collab-pending" title="Changes the service has not acknowledged yet">${status.pending_operations} unsent</span>` : "";
  region.innerHTML = `
    <button type="button" class="collab-pill phase-${escapeHtml(status.phase)}" data-collab-action="${connected ? "disconnect" : "connect"}" aria-live="polite" title="${escapeHtml(connected ? "Leave the collaboration session" : "Connect to a collaboration service")}">
      <span class="collab-dot" aria-hidden="true"></span>
      <span class="collab-phase" data-collab-phase="${escapeHtml(status.phase)}">${escapeHtml(PHASE_LABEL[status.phase])}${escapeHtml(attemptLabel)}</span>
    </button>
    ${status.session ? `<span class="collab-role" data-collab-role="${escapeHtml(status.session.role)}" title="The role the service granted this subject">${escapeHtml(status.session.role)}</span>` : ""}
    ${pending}
    <div class="collab-peers" data-collab-peers>${peers.map((peer) => peerChip(peer, self)).join("")}</div>
    ${status.session ? `<button type="button" class="collab-copy-link" data-collab-action="copy-service-link" title="Copy the credential-free service document link">Copy link</button>
    <button type="button" class="collab-presence-alerts" data-collab-action="presence-announcements" aria-pressed="${presenceAnnouncementsEnabled}" title="${presenceAnnouncementsEnabled ? "Turn off collaborator arrival and departure announcements" : "Turn on collaborator arrival and departure announcements"}">${presenceAnnouncementsEnabled ? "Presence alerts on" : "Presence alerts off"}</button>
    <span class="collab-presence-announcement" data-collab-presence-announcement role="status" aria-live="polite" aria-atomic="true">${escapeHtml(membershipAnnouncement)}</span>` : ""}
    ${notice ? `<span class="collab-notice${notice.resumable ? "" : " fatal"}" data-collab-notice="${escapeHtml(notice.kind)}" title="${escapeHtml(notice.message)}">${escapeHtml(notice.message)}</span>` : ""}`;
  refreshRemotePresence();
}

/**
 * The one delegated listener. `app` outlives every render, so this is bound
 * exactly once — the invariant `bindings.ts` exists to protect.
 */
function bindOnce(): void {
  if (listenerBound) return;
  listenerBound = true;
  app.addEventListener("click", (event) => {
    const target = (event.target as Element).closest<HTMLElement>("[data-collab-action]");
    if (!target || target.hasAttribute("disabled")) return;
    event.preventDefault();
    if (target.dataset.collabAction === "connect") void promptAndConnect();
    if (target.dataset.collabAction === "disconnect") void disconnect();
    if (target.dataset.collabAction === "presence-announcements") {
      writePresenceAnnouncementsPreference(!presenceAnnouncementsEnabled);
      renderCollaborationRegion();
    }
    if (target.dataset.collabAction === "follow-once") {
      followPeerOnce(target.dataset.collabActor ?? "");
    }
    if (target.dataset.collabAction === "copy-service-link") {
      void copyCurrentServiceLink();
    }
  });
}

/** Browser-smoke seam: status framing remains Rust-owned in production. */
export function setCollaborationStatusForTest(next: CollabStatus): void {
  status = next;
  renderCollaborationRegion();
}

// ---- Status plumbing -----------------------------------------------------

function readStatus(json: string): CollabStatus {
  try {
    return { ...IDLE, ...(JSON.parse(json) as Partial<CollabStatus>) };
  } catch {
    return IDLE;
  }
}

/**
 * Adopts a status from Rust and re-renders whatever it says changed.
 *
 * `document_changed` is set once per frame that moved the document, and Rust
 * clears it as it is read, so this asks for the document exactly once per
 * change rather than on every tick.
 */
function adopt(next: CollabStatus): void {
  status = next;
  if (next.notice) localNotice = null;
  adoptAttestedAuthor(next);
  adoptAttestedRuntimeRole(next);
  // Rust decided this session needs the service's log again. Dropping the
  // socket runs the ordinary reconnect path, and the welcome that follows is
  // the resynchronisation: it re-derives the acknowledged watermark from the
  // service's own log, so the tail replayed afterwards is the tail the service
  // is actually missing. Without this a refusal left the session `live` with
  // `can_submit: false` for ever.
  if (next.reconnect_requested) dropSocket("resynchronising with the service");
  if (next.document_changed) {
    void invoke("get_document")
      .then((document) => {
        applyDocument(document);
        // After the re-render, or the morph would drop it again.
        if (next.selection) {
          state.selection = next.selection;
          state.editor?.setSelection(next.selection);
        }
      })
      .catch(() => renderCollaborationRegion());
    return;
  }
  renderCollaborationRegion();
}

/**
 * The service can re-attest a live connection in a Presence frame after a
 * grant change. Rust has already matched subject and actor before exposing
 * this status; reflect only that answer into the desktop runtime projection
 * so its editor/view guard cannot remain at the role read during startup.
 */
function adoptAttestedRuntimeRole(next: CollabStatus): void {
  const session = next.session;
  if (!state.runtimeSession) return;
  if (!session) {
    if (next.phase === "idle" || next.phase === "closed") {
      state.runtimeSession = { ...state.runtimeSession, service_session: null };
    }
    return;
  }
  const serviceRole = (value: string): value is OpenDocServiceRole =>
    (["viewer", "commenter", "editor", "owner"] as string[]).includes(value);
  if (!serviceRole(session.role)) return;
  const role = session.role;
  const peers: OpenDocPresencePeer[] = session.peers.flatMap((peer) =>
    serviceRole(peer.role) ? [{ ...peer, role: peer.role }] : [],
  );
  const priorRole = state.runtimeSession.service_session?.role;
  state.runtimeSession = {
    ...state.runtimeSession,
    service_session: { ...session, role, peers },
  };
  if (role !== priorRole && role === "viewer") {
    // A service downgrade is immediate local safety feedback. An upgrade does
    // not invent an editing intent: the user can deliberately leave View.
    state.documentEditingMode = "view";
    state.editor?.setEditable(false);
    document.querySelector<HTMLSelectElement>("[data-document-editing-mode]")?.setAttribute("disabled", "");
  }
}

/**
 * Takes the author name from the service, once it has said who this is.
 *
 * Comments and suggestions carry an author *string* inside the operation, and
 * the service binds that string to the authenticated subject: a payload
 * attributing words to somebody else is refused, because it would otherwise
 * land in signed source state under a name its author chose. So the name this
 * document writes into a comment has to be the subject the service attested,
 * not the one the page happened to start with — which is
 * `runtime.subject ?? "Local user"`, and is `"Local user"` in a browser whose
 * host injected no runtime config. Left alone, every comment and every
 * suggestion made during a session would be refused, and a refusal now costs
 * three resynchronisation attempts and then the session.
 *
 * This is not the page deciding who the user is. The subject is server state,
 * arriving on the welcome and read back out of the app; the page is copying an
 * answer, which is the whole of this module's job.
 */
function adoptAttestedAuthor(next: CollabStatus): void {
  const subject = next.session?.subject;
  if (!subject) return;
  if (state.authorName === subject) return;
  if (authorNameBeforeSession === null) authorNameBeforeSession = state.authorName;
  state.authorName = subject;
}

function restoreAuthorName(): void {
  if (authorNameBeforeSession === null) return;
  state.authorName = authorNameBeforeSession;
  authorNameBeforeSession = null;
}

function fail(kind: string, message: string, resumable = false): void {
  localNotice = { kind, message, resumable };
  renderCollaborationRegion();
}

/**
 * The status as the region shows it.
 *
 * `localNotice` is merged in rather than kept beside: a caller that reads the
 * status must see the same explanation the user does, and some of them — a
 * refused fetch, a bad address, a reconnect that gave up — happen outside
 * Rust and so have no frame to arrive on.
 */
export function collaborationStatus(): CollabStatus {
  return localNotice ? { ...status, notice: localNotice } : status;
}

// ---- The caret this user contributes to presence -------------------------

/**
 * Where the caret is, as an opaque string.
 *
 * The service relays a cursor anchor and never resolves it (ADR 0015), so its
 * *shape* is the client's business — and mapping a caret to and from the DOM
 * is squarely TypeScript's half of the boundary. This is the same
 * block/inline/offset triple `EditorSelection` carries, flattened.
 */
function cursorAnchor(): string {
  const focus = state.selection?.focus;
  if (!focus) return "";
  return `${focus.block_id}:${focus.inline_id ?? ""}:${focus.offset}`;
}

/** The selection's fixed endpoint; focus remains the cursor anchor above. */
function selectionAnchor(): string {
  const anchor = state.selection?.anchor;
  if (!anchor) return "";
  return `${anchor.block_id}:${anchor.inline_id ?? ""}:${anchor.offset}`;
}

// ---- The browser path: the page owns the socket --------------------------

function clearTimers(): void {
  if (pumpTimer !== null) {
    window.clearInterval(pumpTimer);
    pumpTimer = null;
  }
  if (retryTimer !== null) {
    window.clearTimeout(retryTimer);
    retryTimer = null;
  }
}

/**
 * Drops the socket the way a network blip does: the socket goes, the intention
 * to be connected stays, and everything from here is the production reconnect
 * path.
 *
 * Distinct from `tearDownSocket`, which is what `disconnect` uses: that one
 * ends the session, this one restarts its connection.
 */
function dropSocket(reason: string): void {
  if (!socket) return;
  const dying = socket;
  socket = null;
  clearTimers();
  dying.onopen = null;
  dying.onmessage = null;
  dying.onerror = null;
  dying.onclose = null;
  try {
    dying.close();
  } catch {
    /* already closing */
  }
  void handleClose("1006", reason);
}

/** Drops the socket's handlers before closing it, so a late event cannot fire. */
function tearDownSocket(): void {
  if (!socket) return;
  socket.onopen = null;
  socket.onmessage = null;
  socket.onclose = null;
  socket.onerror = null;
  try {
    socket.close();
  } catch {
    /* already closing */
  }
  socket = null;
}

/**
 * Sweeps up everything outbound: the caret, and any batch Rust is holding.
 *
 * A timer rather than an edit hook. A local gesture becomes an operation
 * inside `dispatch`, and the module that routes gestures belongs to another
 * surface; polling `collab_outbox` costs one `local_operations_after` per tick
 * and cannot miss an edit. `flushing` guards the overlap an async tick would
 * otherwise allow.
 */
async function flush(): Promise<void> {
  if (flushing) return;
  flushing = true;
  try {
    const core = await collabCore();
    if (!core) return;
    core.cursor(cursorAnchor());
    core.selectionAnchor(selectionAnchor());
    // Rust rebases this across whatever lands next; without it a
    // collaborator typing before the caret leaves it on the wrong character.
    core.selection(JSON.stringify(state.selection));
    // Ask for frames only when there is somewhere to put them. Composing a
    // frame advances Rust's submitted-through watermark, so calling `outbox`
    // and then discovering the socket is CLOSING — which is what this used to
    // do — left Rust believing work had gone out that never did, recoverable
    // only if a `close` event happened to arrive. The native transport has
    // always awaited its submit before moving its watermark; this is that same
    // order, and the asymmetry between the two is gone.
    const live = socket;
    if (live && live.readyState === WebSocket.OPEN) {
      for (const frame of core.outbox()) {
        try {
          live.send(frame);
        } catch (error) {
          // A send that throws is a socket that is gone. Ending the connection
          // is also what rolls the watermark back, so the frames this loop
          // never reached are resubmitted on the next welcome rather than lost.
          dropSocket(`sending to the service failed: ${String(error)}`);
          break;
        }
      }
    }
    const next = readStatus(core.status());
    if (next.reconnect_requested || next.document_changed || next.pending_operations !== status.pending_operations || next.notice?.message !== status.notice?.message || next.phase !== status.phase) {
      adopt(next);
    } else {
      status = next;
    }
  } catch (error) {
    fail("pump-failed", `Sending local changes failed: ${String(error)}`, true);
  } finally {
    flushing = false;
  }
}

function scheduleRetry(): void {
  if (!wanted || retryTimer !== null) return;
  if (attempt >= MAX_ATTEMPTS) {
    wanted = null;
    status = { ...status, phase: "closed" };
    const unsent = status.pending_operations;
    fail(
      "reconnect-exhausted",
      `Could not reach the service after ${MAX_ATTEMPTS} attempts, so this session is over.${unsent > 0 ? ` ${unsent} local change(s) were never sent; save the document to keep them.` : ""} A browser is not told why a socket handshake failed, so this may be the service being down, the session having expired, or access having been revoked — reconnect to find out which.`,
      false,
    );
    return;
  }
  const delay = RETRY_MS[Math.min(attempt, RETRY_MS.length - 1)];
  attempt += 1;
  renderCollaborationRegion();
  retryTimer = window.setTimeout(() => {
    retryTimer = null;
    void openSocket();
  }, delay);
}

async function openSocket(): Promise<void> {
  const core = await collabCore();
  if (!core || !wanted) {
    fail("no-core", "This build has no collaboration core, so it cannot connect.");
    return;
  }
  clearTimers();
  tearDownSocket();
  adopt(readStatus(core.begin(wanted.documentUuid, wanted.displayName)));

  const next = new WebSocket(wanted.socketUrl);
  socket = next;
  // Assignments, never addEventListener: a socket is created per attempt, and
  // an assignment cannot stack even if one were not.
  next.onopen = () => {
    attempt = 0;
    // The welcome arrives on its own; the pump only carries outbound work.
    clearTimers();
    pumpTimer = window.setInterval(() => void flush(), PUMP_MS);
  };
  next.onmessage = (event) => {
    if (typeof event.data !== "string") return;
    void handleFrame(event.data);
  };
  next.onclose = (event) => {
    if (socket !== next) return;
    socket = null;
    clearTimers();
    void handleClose(String(event.code || 1006), event.reason ?? "");
  };
  next.onerror = () => {
    // `error` is always followed by `close`, which carries the code. Nothing
    // to do here but avoid an unhandled event.
  };
}

async function handleFrame(text: string): Promise<void> {
  const core = await collabCore();
  if (!core) return;
  adopt(readStatus(core.frame(text)));
}

async function handleClose(code: string, reason: string): Promise<void> {
  const core = await collabCore();
  if (!core) return;
  const next = readStatus(core.closed(code, reason));
  adopt(next);
  // Rust decides whether another attempt could help. A revoked grant or a
  // protocol mismatch refuses the next handshake in exactly the same way, and
  // saying "reconnecting…" for those would be a lie.
  if (next.phase === "closed" || next.notice?.resumable === false) {
    wanted = null;
    return;
  }
  scheduleRetry();
}

// ---- Credentials, which are HTTP and carry no document meaning -----------

function normaliseServiceAddress(url: string): string {
  const trimmed = url.trim();
  if (!trimmed) throw new Error("a service address is required");
  const candidate = /^https?:\/\//i.test(trimmed) ? trimmed : `http://${trimmed}`;
  let parsed: URL;
  try {
    parsed = new URL(candidate);
  } catch {
    throw new Error("the service address must be a valid HTTP(S) address");
  }
  if (parsed.protocol !== "http:" && parsed.protocol !== "https:") {
    throw new Error("the service address must use HTTP or HTTPS");
  }
  // This app has a distinct API-key field.  Treating user-info or a query as
  // part of the service base would both create malformed endpoint URLs and
  // contradict the promise that browser-local reconnect hints hold no secret.
  if (parsed.username || parsed.password || parsed.search || parsed.hash) {
    throw new Error("the service address must not contain credentials, a query, or a fragment; use the API key field instead");
  }
  parsed.pathname = parsed.pathname.replace(/\/+$/, "");
  return parsed.toString().replace(/\/+$/, "");
}

function serviceBase(url: string): string {
  return normaliseServiceAddress(url);
}

function recentConnectionHint(key: string): string | null {
  try {
    const value = window.localStorage.getItem(key)?.trim() ?? "";
    // A preference is only a small UI convenience, never a place to retain an
    // unbounded string supplied by a page or an extension.
    if (value && value.length <= MAX_RECENT_CONNECTION_HINT_LENGTH) return value;
    // Do not merely decline to display a legacy oversized value: leaving it
    // behind means an unbounded string still occupies this reconnect namespace
    // indefinitely. This is deliberately the same repair as unsafe legacy
    // service URLs receive below.
    if (value) window.localStorage.removeItem(key);
    return null;
  } catch {
    return null;
  }
}

/** Read a stored endpoint only if it still meets the same privacy boundary as
 * a new connection. Older builds could store URL user-info/query strings, so
 * discard rather than re-present potentially secret data in a dialog. */
function recentServiceUrlHint(): string | null {
  const value = recentConnectionHint(RECENT_SERVICE_URL_KEY);
  if (!value) return null;
  try {
    return normaliseServiceAddress(value);
  } catch {
    try {
      window.localStorage.removeItem(RECENT_SERVICE_URL_KEY);
    } catch {
      // A disabled storage backend has no hint to clean up.
    }
    return null;
  }
}

function rememberConnectionHints(serviceUrl: string, subject: string): void {
  try {
    if (serviceUrl) {
      try {
        const base = normaliseServiceAddress(serviceUrl);
        if (base.length > MAX_RECENT_CONNECTION_HINT_LENGTH) {
          window.localStorage.removeItem(RECENT_SERVICE_URL_KEY);
        } else {
          window.localStorage.setItem(RECENT_SERVICE_URL_KEY, base);
        }
      } catch {
        // Do not persist an invalid, credential-bearing, or oversized
        // attempted address.
        window.localStorage.removeItem(RECENT_SERVICE_URL_KEY);
      }
    }
    if (subject && subject.length <= MAX_RECENT_CONNECTION_HINT_LENGTH) {
      window.localStorage.setItem(RECENT_SUBJECT_KEY, subject);
    } else {
      // A one-off oversized subject may still be sent to the service for it to
      // validate; it must never displace a bounded reconnect convenience hint.
      window.localStorage.removeItem(RECENT_SUBJECT_KEY);
    }
  } catch {
    // Storage can be disabled; connecting remains fully functional.
  }
}

function socketUrl(base: string, documentUuid: string, token: string, displayName: string): string {
  const scheme = base.startsWith("https://") ? "wss://" : "ws://";
  const authority = base.replace(/^https?:\/\//i, "");
  return `${scheme}${authority}/v1/documents/${encodeURIComponent(documentUuid)}/socket?token=${encodeURIComponent(token)}&display_name=${encodeURIComponent(displayName)}`;
}

async function serviceJson<T>(url: string, init: RequestInit): Promise<T> {
  const response = await fetch(url, init);
  const text = await response.text();
  if (!response.ok) {
    let message = text;
    try {
      const body = JSON.parse(text) as { code?: string; message?: string };
      message = `${body.code ?? response.status}: ${body.message ?? text}`;
    } catch {
      /* not the service's error shape */
    }
    throw new Error(message || `HTTP ${response.status}`);
  }
  return JSON.parse(text) as T;
}

/**
 * Copies a credential-free service link only after an explicit user action.
 * Clipboard permission is a browser capability, not a sharing decision: when
 * it is unavailable the same link is shown in a selectable read-only field
 * instead of claiming it was copied.
 */
export async function copyServiceLinkToClipboard(serviceLink: string): Promise<boolean> {
  try {
    if (!navigator.clipboard?.writeText) throw new Error("Clipboard access is unavailable");
    await navigator.clipboard.writeText(serviceLink);
    toast("Service link copied. It does not grant access by itself.");
    return true;
  } catch {
    await promptDialog({
      title: "Copy service link",
      body: "Clipboard access is unavailable. Select and copy this credential-free link; it does not grant access by itself.",
      fields: [{ name: "serviceLink", label: "Service document link", value: serviceLink, readonly: true }],
      submit: "Close",
    });
    return false;
  }
}

async function copyCurrentServiceLink(): Promise<void> {
  if (status.phase !== "live" || !status.session) {
    toast("Connect to a collaboration service before copying its link.");
    return;
  }
  try {
    const serviceLink = isTauri()
      ? await nativeCommand<string>("collab_share_link")
      : sharingSession
        ? `${sharingSession.base}/v1/documents/${encodeURIComponent(sharingSession.documentUuid)}`
        : null;
    if (!serviceLink) throw new Error("the active session has no available service link");
    await copyServiceLinkToClipboard(serviceLink);
  } catch (error) {
    toast(`Could not copy service link: ${String(error)}`);
  }
}

type GrantView = { subject: string; role: "viewer" | "commenter" | "editor" | "owner" };
type GrantAuditView = {
  sequence: number;
  actor_subject: string;
  target_subject: string;
  previous_role: GrantView["role"] | null;
  role: GrantView["role"] | null;
};

function grantAuditText(events: GrantAuditView[]): string {
  if (!events.length) return "No recorded access changes.";
  return events
    .map((event) => {
      const before = event.previous_role ?? "no access";
      const after = event.role ?? "no access";
      return `#${event.sequence}: ${event.actor_subject} changed ${event.target_subject} from ${before} to ${after}`;
    })
    .join("\n");
}

/**
 * Opens the owner-facing grant manager for the document currently connected
 * to a browser service session. The service remains the authority: this code
 * only displays its grant table and submits a requested mutation.
 */
export async function openSharingDialog(): Promise<void> {
  const session = sharingSession;
  if (status.phase !== "live" || !status.session || (!isTauri() && !session)) {
    toast("Connect to a collaboration service before managing access.");
    return;
  }
  if (status.session.role !== "owner") {
    toast("Only a document owner can view or change access. The service enforces this too.");
    return;
  }

  let grants: GrantView[];
  let serviceLink: string;
  let audit: GrantAuditView[];
  try {
    if (isTauri()) {
      // The shell holds the bearer token and exposes only these narrow,
      // server-authoritative calls. `null` is impossible in Tauri but remains
      // an explicit answer for a test or a partially loaded shell.
      grants = (await nativeCommand<GrantView[]>("collab_list_grants")) ?? [];
      audit = (await nativeCommand<GrantAuditView[]>("collab_list_grant_audit")) ?? [];
      serviceLink = (await nativeCommand<string>("collab_share_link")) ?? "";
      if (!serviceLink) throw new Error("the native collaboration bridge is unavailable");
    } else {
      grants = await serviceJson<GrantView[]>(`${session!.base}/v1/documents/${encodeURIComponent(session!.documentUuid)}/grants`, {
        method: "GET",
        headers: { authorization: `Bearer ${session!.token}` },
      });
      audit = await serviceJson<GrantAuditView[]>(`${session!.base}/v1/documents/${encodeURIComponent(session!.documentUuid)}/grants/audit`, {
        method: "GET",
        headers: { authorization: `Bearer ${session!.token}` },
      });
      serviceLink = `${session!.base}/v1/documents/${encodeURIComponent(session!.documentUuid)}`;
    }
  } catch (error) {
    toast(`Could not load access: ${String(error)}`);
    return;
  }

  const access = grants.length
    ? grants.map((grant) => `${grant.subject} — ${grant.role}`).join("\n")
    : "No one has access.";
  const answer = await promptDialog({
    title: "Share and access",
    body: "People must sign in to this service and use the document id below when connecting. The link contains no credentials and is safe to copy; it does not grant access by itself.",
    fields: [
      { name: "serviceLink", label: "Service document link (copyable)", value: serviceLink, readonly: true },
      { name: "documentId", label: "Document id", value: status.document_uuid, readonly: true },
      { name: "access", label: "People with access", type: "textarea", value: access, readonly: true },
      { name: "audit", label: "Recent access changes", type: "textarea", value: grantAuditText(audit), readonly: true },
      { name: "subject", label: "Person / service subject to change", placeholder: "for example, alice@example.com" },
      {
        name: "role",
        label: "Access level",
        type: "select",
        value: "viewer",
        options: [
          { value: "viewer", label: "Viewer — can read" },
          { value: "commenter", label: "Commenter — can comment" },
          { value: "editor", label: "Editor — can edit" },
          { value: "remove", label: "Remove access" },
        ],
      },
    ],
    submit: "Save access",
    cancel: "Close",
  });
  if (!answer || !answer.subject.trim()) return;

  const subject = answer.subject.trim();
  // `promptDialog` returns strings from DOM controls. Treat the select as
  // untrusted presentation input even though it is populated above: a changed
  // value in a devtools-mutated dialog must not turn into an arbitrary service
  // request.
  const role = answer.role === "remove" ? null : answer.role;
  if (role !== null && role !== "viewer" && role !== "commenter" && role !== "editor") {
    toast("Choose a supported access level.");
    return;
  }
  if (
    role === null &&
    !(await confirmDialog(
      "Remove access?",
      `Remove ${subject}'s access to this document? Their current session may be refused on its next service check.`,
      "Remove access",
    ))
  ) {
    return;
  }
  try {
    if (isTauri()) {
      await nativeCommand<void>("collab_set_grant", { subject, role });
    } else {
      await serviceJson<unknown>(`${session!.base}/v1/documents/${encodeURIComponent(session!.documentUuid)}/grants`, {
        method: "PUT",
        headers: { "content-type": "application/json", authorization: `Bearer ${session!.token}` },
        body: JSON.stringify({ subject, role }),
      });
    }
    toast(role ? `Access for ${subject} set to ${role}.` : `Access removed for ${subject}.`);
  } catch (error) {
    // A stale owner grant and an expired/revoked bearer token both have to be
    // reported as the service's refusal; the UI never guesses permission.
    toast(`Could not change access: ${String(error)}`);
  }
}

// ---- Connect / disconnect ------------------------------------------------

/**
 * Joining replaces the open document with the service's, so unsaved local work
 * has to be consented to first. `join_collaboration_session` does not ask —
 * from its side the service's document simply *is* the document from then on —
 * so this is where the user is asked.
 */
async function confirmReplacingLocalWork(): Promise<boolean> {
  if (!state.doc?.has_unsaved_changes) return true;
  return confirmDialog(
    "Join and discard unsaved changes?",
    `"${state.doc.title}" has changes that are not saved. Joining a collaboration session replaces this document with the service's copy, and those changes are lost.`,
    "Join anyway",
  );
}

async function promptAndConnect(): Promise<void> {
  const rememberedServiceUrl = recentServiceUrlHint();
  const rememberedSubject = recentConnectionHint(RECENT_SUBJECT_KEY);
  const answer = await promptDialog({
    title: "Connect to a collaboration service",
    fields: [
      { name: "serviceUrl", label: "Service address", type: "text", value: rememberedServiceUrl ?? "http://127.0.0.1:8787", placeholder: "http://host:port" },
      { name: "subject", label: "Subject", type: "text", value: runtime.subject ?? rememberedSubject ?? "" },
      { name: "apiKey", label: "API key", type: "password", value: "" },
      { name: "documentUuid", label: "Document id (blank creates a new one)", type: "text", value: "" },
      { name: "displayName", label: "Show others this name", type: "text", value: state.authorName },
    ],
    submit: "Connect",
    cancel: "Cancel",
  });
  if (!answer) return;
  // Only these non-secret values are remembered. The API key stays in this
  // one submission and is handed directly to the service/native bridge.
  rememberConnectionHints(answer.serviceUrl.trim(), answer.subject.trim());
  await connect({
    serviceUrl: answer.serviceUrl,
    subject: answer.subject,
    apiKey: answer.apiKey,
    documentUuid: answer.documentUuid,
    displayName: answer.displayName,
  });
}

/**
 * Connects, and reports why not if it cannot.
 *
 * The same function the pill's button calls, so a harness driving this drives
 * the shipped path rather than a parallel one.
 */
export async function connect(options: ConnectOptions): Promise<CollabStatus> {
  localNotice = null;
  if (!(await confirmReplacingLocalWork())) return status;
  const displayName = (options.displayName ?? state.authorName ?? options.subject).trim() || options.subject;

  if (isTauri()) {
    // The native shell owns the whole conversation, including the credential
    // exchange: a token that never enters the webview is a token a page bug
    // cannot leak.
    await bindNativeStatus();
    try {
      const next = await nativeCommand<CollabStatus>("collab_connect", {
        options: {
          serviceUrl: options.serviceUrl,
          subject: options.subject,
          apiKey: options.apiKey,
          documentUuid: options.documentUuid?.trim() || null,
          displayName,
          title: state.doc?.title ?? "Shared document",
        },
      });
      if (next) adopt(next);
      startNativePump();
    } catch (error) {
      fail("connect-failed", `Could not connect: ${String(error)}`, true);
    }
    return status;
  }

  let base: string;
  try {
    base = serviceBase(options.serviceUrl);
  } catch (error) {
    fail("bad-address", String(error));
    return status;
  }
  try {
    const session = await serviceJson<{ token: string }>(`${base}/v1/sessions`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ subject: options.subject.trim(), api_key: options.apiKey }),
    });
    let documentUuid = options.documentUuid?.trim() ?? "";
    if (!documentUuid) {
      const created = await serviceJson<{ document_uuid: string }>(`${base}/v1/documents`, {
        method: "POST",
        headers: { "content-type": "application/json", authorization: `Bearer ${session.token}` },
        body: JSON.stringify({ title: state.doc?.title ?? "Shared document" }),
      });
      documentUuid = created.document_uuid;
    }
    attempt = 0;
    wanted = {
      socketUrl: socketUrl(base, documentUuid, session.token, displayName),
      documentUuid,
      displayName,
    };
    sharingSession = { base, token: session.token, documentUuid };
    await openSocket();
  } catch (error) {
    // A refused CORS preflight, a wrong key and an unreachable host all land
    // here, and the browser deliberately does not tell a page which. Saying so
    // is better than guessing: the service refuses an origin it was not
    // configured with (OPENDOC_SERVICE_ORIGINS), and that is the usual cause.
    fail(
      "connect-failed",
      `Could not reach the service: ${String(error)}. If the address is right, check that the service allows this page's origin (${window.location.origin}).`,
      true,
    );
  }
  return status;
}

export async function disconnect(): Promise<CollabStatus> {
  wanted = null;
  attempt = 0;
  clearTimers();
  tearDownSocket();
  sharingSession = null;
  localNotice = null;
  restoreAuthorName();
  if (isTauri()) {
    const next = await nativeCommand<CollabStatus>("collab_disconnect", {});
    adopt(next ?? IDLE);
    return status;
  }
  const core = await collabCore();
  adopt(core ? readStatus(core.leave()) : IDLE);
  // Leaving drops the service's merge base, so the document projection is a
  // plain local document again and the shell must be told.
  void invoke("get_document")
    .then((document) => applyDocument(document))
    .catch(() => {});
  return status;
}

// ---- The native path: the shell owns the socket --------------------------

async function bindNativeStatus(): Promise<void> {
  if (nativeStatusBound) return;
  const unlisten = await onNativeEvent("opendoc://collab-status", (payload) => {
    adopt({ ...IDLE, ...(payload as Partial<CollabStatus>) });
  });
  // Bound for the process's lifetime, deliberately: unsubscribing on
  // disconnect and resubscribing on connect is precisely the re-runnable path
  // that stacks listeners.
  nativeStatusBound = unlisten !== null;
}

function startNativePump(): void {
  clearTimers();
  pumpTimer = window.setInterval(() => {
    void nativeCommand("collab_cursor", { anchor: cursorAnchor() }).catch(() => {});
    void nativeCommand("collab_selection_anchor", { anchor: selectionAnchor() }).catch(() => {});
    void nativeCommand("collab_selection", { selection: state.selection }).catch(() => {});
  }, PUMP_MS);
}

// ---- Harness hook --------------------------------------------------------
//
// The same functions the button calls, reachable without a dialog so a
// browser-level test can drive two real clients. `main.ts` exposes `__test`
// for the same reason; this module cannot add to it, so it publishes its own.
declare global {
  interface Window {
    __OPENDOC_COLLAB__?: {
      connect: (options: ConnectOptions) => Promise<CollabStatus>;
      disconnect: () => Promise<CollabStatus>;
      status: () => CollabStatus;
      dropSocket: () => void;
      /** Browser-smoke seam; never accepts a bearer outside this process. */
      setBrowserSharingSessionForTest: (session: { base: string; token: string; documentUuid: string } | null) => void;
    };
  }
}

/**
 * Drops the socket the way a network blip does.
 *
 * A hook rather than a mock — it calls the same `dropSocket` a resynchronising
 * session calls, so everything it triggers is shipped code — and the only
 * thing a browser-level test cannot otherwise cause from outside the page.
 */
function dropSocketForTest(): void {
  dropSocket("dropped by the harness");
}

function setBrowserSharingSessionForTest(session: { base: string; token: string; documentUuid: string } | null): void {
  sharingSession = session;
}

window.__OPENDOC_COLLAB__ = {
  connect,
  disconnect,
  status: collaborationStatus,
  dropSocket: dropSocketForTest,
  setBrowserSharingSessionForTest,
};
