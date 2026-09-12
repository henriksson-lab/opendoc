//! Browser entry point: the same `dispatch_command` surface the Tauri shell
//! uses, compiled to WebAssembly so the web build runs the real Rust core
//! instead of a JavaScript re-implementation.
//!
//! Storage lives in [`storage`]: an IndexedDB-backed volume behind the same
//! Rust traits the filesystem implements, so `save_local_repository` and the
//! crash-recovery journal work here as well (`docs/adr/0008-browser-storage-adapter.md`).

mod storage;

use opendoc_app::OpenDocApp;
use std::cell::RefCell;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
    // Synchronous, so the volume is in place before any command can run.
    // It becomes *durable* later, when `storage_ready` attaches IndexedDB.
    storage::install_volume();
}

thread_local! {
    static APP: RefCell<Option<OpenDocApp>> = const { RefCell::new(None) };
}

fn with_app<T>(f: impl FnOnce(&mut OpenDocApp) -> T) -> Result<T, JsValue> {
    APP.with(|cell| {
        let mut slot = cell
            .try_borrow_mut()
            .map_err(|_| JsValue::from_str("OpenDoc core is busy (re-entrant call)"))?;
        let app = slot.get_or_insert_with(OpenDocApp::new_empty_document);
        Ok(f(app))
    })
}

/// Give the app a recovery-journal store over the browser volume, and report
/// how many unclean sessions it found.
///
/// Installing is what makes journalling happen at all: without a store the
/// journal is inert, which is exactly why the browser had no crash protection
/// before (ADR 0005 §6).
fn install_recovery_journal() -> usize {
    let Some(store) = storage::recovery_journal_store() else {
        return 0;
    };
    with_app(|app| {
        app.install_recovery_journal(store)
            .map(|document| document.recovery_sessions.len())
            .unwrap_or(0)
    })
    .unwrap_or(0)
}

fn recovery_session_count() -> usize {
    with_app(|app| app.document().recovery_sessions.len()).unwrap_or(0)
}

/// Dispatch one app command. `args_json` is the JSON-encoded argument
/// object; the result is the JSON-encoded `AppCommandResult`, or the error
/// message as a rejected promise-like `Err`.
#[wasm_bindgen]
pub fn dispatch(command: &str, args_json: &str) -> Result<String, JsValue> {
    let args: serde_json::Value = if args_json.trim().is_empty() {
        serde_json::Value::Object(Default::default())
    } else {
        serde_json::from_str(args_json).map_err(|err| JsValue::from_str(&err.to_string()))?
    };
    let result = with_app(|app| app.dispatch_command(command, args));
    // Whatever the command did to the volume — a save, a recovery-journal
    // frame — goes to IndexedDB now, asynchronously. Scheduled even when the
    // command failed, because a failed command can still have journalled.
    storage::schedule_flush();
    let result = result?.map_err(|err| JsValue::from_str(&err.to_string()))?;
    serde_json::to_string(&result).map_err(|err| JsValue::from_str(&err.to_string()))
}

/// Attach durable storage and hydrate the volume from it.
///
/// Must be awaited before the first `dispatch`: IndexedDB is asynchronous, and
/// a command that ran first would read an empty volume. Resolves to
/// `{ persistent, entries, recoverySessions }`; `persistent: false` means this
/// runtime has no IndexedDB and documents live only in this tab.
#[wasm_bindgen]
pub async fn storage_ready() -> Result<JsValue, JsValue> {
    storage::ready().await
}

/// Durability watermark: `sequence` is the last mutation the core made,
/// `durableSeq` the last one IndexedDB has committed. They differ only while a
/// flush is in flight.
#[wasm_bindgen]
pub fn storage_status() -> JsValue {
    storage::status()
}

/// Discard the in-memory app and start over from the boot state: an empty
/// document, as if the page had just been loaded.
#[wasm_bindgen]
pub fn reset() {
    APP.with(|cell| {
        *cell.borrow_mut() = Some(OpenDocApp::new_empty_document());
    });
    // The new app has no journal store; give it the one the page already has,
    // or a reset would silently turn crash protection off.
    install_recovery_journal();
}

/// Names of every command accepted by `dispatch`.
#[wasm_bindgen]
pub fn command_names() -> String {
    serde_json::to_string(&opendoc_app::command_names()).unwrap_or_else(|_| "[]".to_string())
}
