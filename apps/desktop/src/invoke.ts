// Thin transport between the UI and the Rust core. In Tauri every call goes
// through the single `dispatch` command; in a plain browser the same Rust
// core runs as WebAssembly (crates/opendoc-wasm). No document logic lives here.

import type { CommandArgs, CommandResult, DesktopCommandName } from "./commands";
import type {
  AppCommandResult,
  OpenDocPermissionGrant,
  OpenDocPresencePeer,
  OpenDocRuntimeConfig,
  OpenDocRuntimeMode,
} from "./types";

type TauriInvoke = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;

declare global {
  interface Window {
    __OPENDOC_RUNTIME__?: OpenDocRuntimeConfig;
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
        await module.storage_ready?.();
      } catch (error) {
        console.warn("OpenDoc storage is unavailable; this session is not persistent", error);
      }
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

export async function onCloseRequested(handler: () => void): Promise<void> {
  const listen = window.__TAURI__?.event?.listen;
  if (listen) {
    await listen("opendoc://close-requested", () => handler());
  }
}

// ---- Runtime mode --------------------------------------------------------

export type RuntimeConfig = {
  mode: OpenDocRuntimeMode;
  subject: string | null;
  presence: OpenDocPresencePeer[];
  permissions: OpenDocPermissionGrant[];
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
    presence: configured?.presence ?? [],
    permissions: configured?.permissions ?? [],
    storageBackends: configured?.storageBackends ?? null,
    signingEnabled: configured?.signingEnabled ?? null,
  };
}
