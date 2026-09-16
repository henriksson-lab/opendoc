// Thin transport between the UI and the Rust core. In Tauri every call goes
// through the single `dispatch` command; in a plain browser the same Rust
// core runs as WebAssembly (crates/opendoc-wasm). No document logic lives here.

import type { CommandArgs, CommandResult, DesktopCommandName } from "./commands";
import type {
  AppCommandResult,
  OpenDocRuntimeConfig,
  OpenDocRuntimeMode,
} from "./types";

type TauriInvoke = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;

/**
 * What `storage_ready` reported about durable storage in this runtime.
 *
 * `persistent: false` means this tab's work is in memory only — no IndexedDB
 * at all, or another tab owns it (`notOwner` says which). Rust decides and
 * describes; the only thing this file does with the answer is keep it and let
 * the shell say it out loud, because a report nobody reads is the same as no
 * report (ADR 0008 §5).
 */
export type StorageReport = {
  persistent: boolean;
  entries?: number;
  recoverySessions?: number;
  notOwner?: string;
  error?: string;
};

let storageReportValue: StorageReport | null = null;

/** The last `storage_ready` report, or `null` before the core has booted. */
export function storageReport(): StorageReport | null {
  return storageReportValue;
}

declare global {
  interface Window {
    __OPENDOC_RUNTIME__?: OpenDocRuntimeConfig;
    /**
     * Durable-storage state, reachable without a dialog so a browser-level
     * test can drive two real tabs. `collab.ts` publishes its own hook for the
     * same reason.
     */
    __OPENDOC_STORAGE__?: {
      report: () => StorageReport | null;
      status: () => unknown;
    };
    __TAURI__?: {
      core?: { invoke?: TauriInvoke };
      event?: { listen?: (name: string, handler: (event: { payload: unknown }) => void) => Promise<() => void> };
    };
    __TAURI_INTERNALS__?: { invoke?: TauriInvoke };
  }
}

type WasmModule = {
  default: (input?: unknown) => Promise<unknown>;
  initSync?: (input: { module: BufferSource | WebAssembly.Module }) => unknown;
  dispatch: (command: string, argsJson: string) => string;
  reset: () => void;
  /**
   * Attaches IndexedDB to the Rust storage volume and hydrates it. Optional so
   * a preloaded module (the jsdom smoke test) still typechecks; absent or
   * failing, the core keeps working in memory.
   */
  storage_ready?: () => Promise<unknown>;
  storage_status?: () => unknown;
  /**
   * The collaboration driver (crates/opendoc-wasm/src/collab.rs). Optional for
   * the same reason storage is: a preloaded module in the jsdom smoke test
   * must still typecheck, and a build without these is a build with no
   * collaboration rather than a broken one.
   */
  collab_begin?: (documentUuid: string, displayName: string) => string;
  collab_frame?: (frame: string) => string;
  collab_closed?: (code: string, message: string) => string;
  collab_outbox?: () => string;
  collab_cursor?: (anchor: string) => void;
  collab_selection_anchor?: (anchor: string) => void;
  /**
   * This user's caret as `EditorSelection` JSON, so the core can rebase it
   * when a collaborator's work arrives. Not the presence anchor above: this
   * one never leaves the browser.
   */
  collab_selection?: (selection: string) => void;
  collab_leave?: () => string;
  collab_status?: () => string;
};

let wasmModule: WasmModule | null = null;
let wasmLoading: Promise<WasmModule> | null = null;

function tauriInvoke(): TauriInvoke | null {
  return window.__TAURI__?.core?.invoke ?? window.__TAURI_INTERNALS__?.invoke ?? null;
}

export function isTauri(): boolean {
  return tauriInvoke() !== null;
}

/** Lets tests (Node) provide a preloaded WASM module instead of fetching one. */
export function useWasmModule(module: WasmModule): void {
  wasmModule = module;
}

async function loadWasm(): Promise<WasmModule> {
  if (wasmModule) {
    return wasmModule;
  }
  if (!wasmLoading) {
    wasmLoading = (async () => {
      const module = (await import("./wasm/opendoc_wasm.js")) as unknown as WasmModule;
      await module.default();
      // Storage is Rust's (crates/opendoc-wasm/src/storage.rs); the only thing
      // TypeScript owes it is this await. IndexedDB is asynchronous and
      // dispatch is not, so the volume has to be hydrated before the first
      // command can read from it. A runtime without IndexedDB resolves too,
      // reporting that it is not persistent.
      try {
        const report = await module.storage_ready?.();
        storageReportValue = (report as StorageReport | undefined) ?? { persistent: false };
      } catch (error) {
        console.warn("OpenDoc storage is unavailable; this session is not persistent", error);
        storageReportValue = { persistent: false, error: String(error) };
      }
      window.__OPENDOC_STORAGE__ = {
        report: storageReport,
        status: () => module.storage_status?.() ?? null,
      };
      wasmModule = module;
      return module;
    })();
  }
  return wasmLoading;
}

/** Raw dispatch returning the tagged `AppCommandResult`. */
export async function dispatch(command: string, args: Record<string, unknown> = {}): Promise<AppCommandResult> {
  const native = tauriInvoke();
  if (native) {
    return native<AppCommandResult>("dispatch", { command, args });
  }
  const module = await loadWasm();
  let json: string;
  try {
    json = module.dispatch(command, JSON.stringify(args));
  } catch (error) {
    throw new Error(typeof error === "string" ? error : String(error));
  }
  return JSON.parse(json) as AppCommandResult;
}

/**
 * Typed dispatch that unwraps the result payload.
 *
 * `discardUnsavedChanges` is not a command argument: it is the dispatcher-wide
 * acknowledgement that lets a document-replacing command proceed over unsaved
 * work (ADR 0005). Only pass it once a user has actually accepted the loss.
 */
export async function invoke<K extends DesktopCommandName>(
  command: K,
  args: CommandArgs<K> = {} as CommandArgs<K>,
  options: { discardUnsavedChanges?: boolean } = {},
): Promise<CommandResult<K>> {
  const payload = options.discardUnsavedChanges
    ? { ...(args as Record<string, unknown>), discardUnsavedChanges: true }
    : (args as Record<string, unknown>);
  const result = await dispatch(command, payload);
  return result.value as CommandResult<K>;
}

/**
 * Did Rust refuse this command because it would discard unsaved work?
 *
 * `AppApiError` reaches a transport as its `Debug` form, so the variant name
 * is the wire marker. It is a distinct variant precisely so this check does
 * not have to read message prose.
 */
export function isUnsavedChangesError(error: unknown): boolean {
  const message = error instanceof Error ? error.message : String(error);
  return message.trimStart().startsWith("UnsavedChanges(");
}

// ---- Desktop-only capabilities (file dialogs, file IO, window) ----------

export type PickedFile = {
  name: string;
  path: string;
  media_type: string;
  size: number;
  base64: string;
};

export async function pickOpenPath(options: {
  title?: string;
  extensions?: string[];
  directory?: boolean;
}): Promise<string | null> {
  const native = tauriInvoke();
  if (!native) {
    return null;
  }
  return native<string | null>("pick_open_path", options);
}

export async function pickSavePath(options: {
  title?: string;
  defaultName?: string;
  extensions?: string[];
}): Promise<string | null> {
  const native = tauriInvoke();
  if (!native) {
    return null;
  }
  return native<string | null>("pick_save_path", {
    title: options.title,
    defaultName: options.defaultName,
    extensions: options.extensions,
  });
}

/** Open a file: native dialog in Tauri, `<input type="file">` in browsers. */
export async function openFile(extensions: string[]): Promise<PickedFile | null> {
  const native = tauriInvoke();
  if (native) {
    const path = await native<string | null>("pick_open_path", { extensions });
    if (!path) {
      return null;
    }
    return native<PickedFile>("read_file_base64", { path });
  }
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = extensions.map((ext) => `.${ext}`).join(",");
    input.addEventListener("change", () => {
      const file = input.files?.[0];
      if (!file) {
        resolve(null);
        return;
      }
      const reader = new FileReader();
      reader.onload = () => {
        const dataUrl = String(reader.result ?? "");
        const base64 = dataUrl.slice(dataUrl.indexOf(",") + 1);
        resolve({ name: file.name, path: file.name, media_type: file.type || "application/octet-stream", size: file.size, base64 });
      };
      reader.readAsDataURL(file);
    });
    input.click();
  });
}

/**
 * Downloads a URL and returns it in the same shape `openFile` does.
 *
 * Native only, and deliberately so: fetching is network IO with policy
 * attached — scheme, redirects, a size cap, and refusing to reach addresses on
 * the user's own machine or LAN — none of which is document semantics and none
 * of which a page can enforce for itself. The shell owns it beside the file
 * dialogs; `null` means this runtime cannot do it at all, which is the browser
 * build's honest answer rather than a half-working one.
 */
export async function fetchUrlFile(url: string, maxBytes?: number): Promise<PickedFile | null> {
  const native = tauriInvoke();
  if (!native) {
    return null;
  }
  return native<PickedFile>("fetch_url_base64", { url, maxBytes });
}

/** Save bytes/text: native dialog in Tauri, a download in browsers. */
export async function saveFile(options: {
  defaultName: string;
  extensions: string[];
  text?: string;
  base64?: string;
  mediaType?: string;
}): Promise<string | null> {
  const native = tauriInvoke();
  if (native) {
    const path = await native<string | null>("pick_save_path", {
      defaultName: options.defaultName,
      extensions: options.extensions,
    });
    if (!path) {
      return null;
    }
    if (options.base64 !== undefined) {
      await native("write_file_base64", { path, base64: options.base64 });
    } else {
      await native("write_file_text", { path, text: options.text ?? "" });
    }
    return path;
  }
  const mediaType = options.mediaType ?? "application/octet-stream";
  const href =
    options.base64 !== undefined
      ? `data:${mediaType};base64,${options.base64}`
      : URL.createObjectURL(new Blob([options.text ?? ""], { type: mediaType }));
  const anchor = document.createElement("a");
  anchor.href = href;
  anchor.download = options.defaultName;
  anchor.click();
  return options.defaultName;
}

export async function setWindowTitle(title: string): Promise<void> {
  const native = tauriInvoke();
  if (native) {
    await native("set_window_title", { title });
  }
  document.title = title;
}

export async function closeWindow(): Promise<void> {
  const native = tauriInvoke();
  if (native) {
    await native("close_window", {});
  } else {
    window.close();
  }
}

// ---- Collaboration transport -------------------------------------------
//
// Two runtimes, two places the socket lives, one shape above them
// (docs/adr/0018):
//
// * **Browser.** `opendoc-service`'s Rust client is built on tokio and must
//   stay out of the WebAssembly graph, so the page owns the socket and hands
//   every frame to the Rust driver below. These are that driver's exports —
//   opaque strings in, a status DTO out. Nothing here reads a frame.
// * **Tauri.** The native shell owns the socket with the service crate's own
//   client, so the page never sees a frame at all: it asks the shell to
//   connect and listens for the status it pushes back.
//
// `collab.ts` is the only caller, and it picks by `isTauri()`.

/** The Rust collaboration driver, in the browser build. `null` in Tauri. */
export type CollabCore = {
  begin(documentUuid: string, displayName: string): string;
  frame(text: string): string;
  closed(code: string, message: string): string;
  outbox(): string[];
  cursor(anchor: string): void;
  selectionAnchor(anchor: string): void;
  /**
   * Hands the core this user's caret, as `EditorSelection` JSON or `"null"`.
   *
   * The presence anchor (`cursor`) is what other people see; this is what
   * `apply_remote_operations` rebases so that a collaborator typing in front
   * of the caret moves it along with the text. The moved caret comes back as
   * `selection` in the next status, and a caller that shows a caret has to
   * apply it — see `CollabStatus::selection` in `opendoc-wasm/src/collab.rs`.
   */
  selection(json: string): void;
  leave(): string;
  status(): string;
};

export async function collabCore(): Promise<CollabCore | null> {
  if (isTauri()) {
    return null;
  }
  const module = await loadWasm();
  if (!module.collab_begin || !module.collab_frame || !module.collab_outbox) {
    return null;
  }
  return {
    begin: (documentUuid, displayName) => module.collab_begin!(documentUuid, displayName),
    frame: (text) => module.collab_frame!(text),
    closed: (code, message) => module.collab_closed?.(code, message) ?? "{}",
    outbox: () => JSON.parse(module.collab_outbox!()) as string[],
    cursor: (anchor) => module.collab_cursor?.(anchor),
    selectionAnchor: (anchor) => module.collab_selection_anchor?.(anchor),
    selection: (json) => module.collab_selection?.(json),
    leave: () => module.collab_leave?.() ?? "{}",
    status: () => module.collab_status?.() ?? "{}",
  };
}

/**
 * A native-only shell command. `null` means this runtime is not Tauri, which
 * is an answer and not a failure.
 */
export async function nativeCommand<T>(
  command: string,
  args: Record<string, unknown> = {},
): Promise<T | null> {
  const native = tauriInvoke();
  if (!native) {
    return null;
  }
  return native<T>(command, args);
}

/**
 * Subscribes to a native shell event. Returns the unsubscribe function, or
 * `null` outside Tauri.
 *
 * Bound once by the caller and never on a re-runnable path: a listener
 * attached per render is the bug this frontend has had four times.
 */
export async function onNativeEvent(
  name: string,
  handler: (payload: unknown) => void,
): Promise<(() => void) | null> {
  const listen = window.__TAURI__?.event?.listen;
  if (!listen) {
    return null;
  }
  return listen(name, (event) => handler(event.payload));
}

export async function onCloseRequested(handler: () => void): Promise<void> {
  const listen = window.__TAURI__?.event?.listen;
  if (listen) {
    await listen("opendoc://close-requested", () => handler());
  }
}

// ---- Runtime mode --------------------------------------------------------

/**
 * What the host says about itself: capabilities, and a display name for the
 * local user.
 *
 * Not permissions and not presence. In a service runtime those are the
 * service's answers, delivered over the collaboration transport to the Rust
 * core; a page that could declare them here would be declaring its own access,
 * which is exactly the shape `authorize_runtime_command` no longer has.
 */
export type RuntimeConfig = {
  mode: OpenDocRuntimeMode;
  subject: string | null;
  storageBackends: string[] | null;
  signingEnabled: boolean | null;
};

/** Mode and identity hints; profile details come from `get_runtime_profile`. */
export function runtimeConfig(): RuntimeConfig {
  const configured = window.__OPENDOC_RUNTIME__;
  const mode: OpenDocRuntimeMode = configured?.mode ?? (isTauri() ? "tauri-local" : "browser-local");
  return {
    mode,
    subject: configured?.subject ?? null,
    storageBackends: configured?.storageBackends ?? null,
    signingEnabled: configured?.signingEnabled ?? null,
  };
}
