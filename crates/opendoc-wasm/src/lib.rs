//! Browser entry point: the same `dispatch_command` surface the Tauri shell
//! uses, compiled to WebAssembly so the web build runs the real Rust core
//! instead of a JavaScript re-implementation.
//!
//! Storage lives in [`storage`]: an IndexedDB-backed volume behind the same
//! Rust traits the filesystem implements, so `save_local_repository` and the
//! crash-recovery journal work here as well (`docs/adr/0008-browser-storage-adapter.md`).

mod collab;
#[cfg(test)]
mod collab_tests;
mod storage;

use collab::CollabDriver;
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

pub(crate) fn with_app<T>(f: impl FnOnce(&mut OpenDocApp) -> T) -> Result<T, JsValue> {
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

/// Give the app a recents store over the browser volume.
///
/// The same shape as the journal install above and for the same reason: the
/// key is this runtime's knowledge, not `opendoc-app`'s. There is no `Result`
/// to swallow — an unreadable stored list becomes a model warning inside the
/// app — and a runtime with no IndexedDB gets no store at all, so the home
/// screen's list is honestly this-tab-only rather than falsely durable.
fn install_recent_documents() {
    let Some(store) = storage::recent_documents_store() else {
        return;
    };
    let _ = with_app(|app| app.install_recent_documents(store));
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
    // The new app has no journal store and no recents store; give it the ones
    // the page already has, or a reset would silently turn crash protection
    // off and stop recents being remembered.
    install_recovery_journal();
    install_recent_documents();
}

/// Names of every command accepted by `dispatch`.
#[wasm_bindgen]
pub fn command_names() -> String {
    serde_json::to_string(&opendoc_app::command_names()).unwrap_or_else(|_| "[]".to_string())
}

// ---- Collaboration transport --------------------------------------------
//
// The socket is TypeScript's (`apps/desktop/src/collab.ts`); every byte inside
// it is Rust's. These six functions are the whole boundary: the page hands
// over frames it never reads, and gets back frames it never composes plus a
// status DTO to render. See `collab` and `docs/adr/0018`.

thread_local! {
    static COLLAB: RefCell<CollabDriver> = RefCell::new(CollabDriver::default());
}

fn with_collab<T>(f: impl FnOnce(&mut CollabDriver) -> T) -> Result<T, JsValue> {
    COLLAB.with(|cell| {
        let mut driver = cell
            .try_borrow_mut()
            .map_err(|_| JsValue::from_str("OpenDoc collaboration is busy (re-entrant call)"))?;
        Ok(f(&mut driver))
    })
}

/// Both borrows at once, in one order, so a frame handler and the app can
/// never be taken in the opposite order somewhere else.
fn with_collab_and_app<T>(
    f: impl FnOnce(&mut CollabDriver, &mut OpenDocApp) -> T,
) -> Result<T, JsValue> {
    with_collab(|driver| with_app(|app| f(driver, app)))?
}

fn status_json(driver: &mut CollabDriver, app: &OpenDocApp) -> String {
    serde_json::to_string(&driver.status(app)).unwrap_or_else(|_| "{}".to_string())
}

/// A socket is being opened for `document_uuid`. Returns the status JSON.
#[wasm_bindgen]
pub fn collab_begin(document_uuid: &str, display_name: &str) -> Result<String, JsValue> {
    with_collab_and_app(|driver, app| {
        driver.begin(document_uuid, display_name);
        status_json(driver, app)
    })
}

/// One server frame, verbatim. Returns the status JSON; `document_changed`
/// means the caller must re-render from `get_document`.
///
/// A frame this client cannot use is reported through the status `notice`
/// rather than thrown: a session mid-edit must not be torn down because one
/// message was unreadable, and the notice is what reaches the user.
#[wasm_bindgen]
pub fn collab_frame(frame: &str) -> Result<String, JsValue> {
    let status = with_collab_and_app(|driver, app| {
        let _ = driver.ingest(app, frame);
        status_json(driver, app)
    })?;
    // A frame can have journalled (remote work is in the recovery segment, so
    // a crash cannot roll a collaborator's edits back — ADR 0005), so the
    // volume has to reach IndexedDB the same way a command's does.
    storage::schedule_flush();
    Ok(status)
}

/// The socket closed. Returns the status JSON.
#[wasm_bindgen]
pub fn collab_closed(code: &str, message: &str) -> Result<String, JsValue> {
    with_collab_and_app(|driver, app| {
        driver.socket_closed(code, message);
        status_json(driver, app)
    })
}

/// The frames the socket should send now, as a JSON array of strings.
#[wasm_bindgen]
pub fn collab_outbox() -> Result<String, JsValue> {
    let frames = with_collab_and_app(|driver, app| driver.outbox(app))?;
    serde_json::to_string(&frames).map_err(|error| JsValue::from_str(&error.to_string()))
}

/// This user's caret, as an opaque anchor the service relays untouched.
#[wasm_bindgen]
pub fn collab_cursor(anchor: &str) -> Result<(), JsValue> {
    with_collab(|driver| {
        driver.set_cursor(if anchor.trim().is_empty() {
            None
        } else {
            Some(anchor.to_string())
        })
    })
}

/// The fixed endpoint of this user's selection, paired with `collab_cursor`.
#[wasm_bindgen]
pub fn collab_selection_anchor(anchor: &str) -> Result<(), JsValue> {
    with_collab(|driver| {
        driver.set_selection_anchor(if anchor.trim().is_empty() {
            None
        } else {
            Some(anchor.to_string())
        })
    })
}

/// This user's caret, as an `EditorSelection` JSON object (or `null`).
///
/// Not the same thing as [`collab_cursor`], which is the presence anchor the
/// service relays to *other* people. This one never leaves the browser: it is
/// what `OpenDocApp::apply_remote_operations` rebases when a collaborator's
/// work arrives, so that a colleague typing in front of the caret moves it
/// along with the text instead of leaving it at the same character index. The
/// moved caret comes back as `selection` in the status JSON the next
/// `collab_frame`/`collab_status` returns, and the caller must apply it.
///
/// An unparseable value is refused rather than ignored: silently keeping a
/// stale caret is the bug this exists to fix.
#[wasm_bindgen]
pub fn collab_selection(selection: &str) -> Result<(), JsValue> {
    let trimmed = selection.trim();
    let parsed: Option<opendoc_app::EditorSelection> = if trimmed.is_empty() || trimmed == "null" {
        None
    } else {
        Some(serde_json::from_str(trimmed).map_err(|error| {
            JsValue::from_str(&format!(
                "collab_selection is not an EditorSelection: {error}"
            ))
        })?)
    };
    with_collab(|driver| driver.set_selection(parsed))
}

/// Leave the session. The operation log stays; the service's answers do not.
#[wasm_bindgen]
pub fn collab_leave() -> Result<String, JsValue> {
    let status = with_collab_and_app(|driver, app| {
        driver.leave(app);
        status_json(driver, app)
    })?;
    storage::schedule_flush();
    Ok(status)
}

/// The current status, without handing over a frame.
#[wasm_bindgen]
pub fn collab_status() -> Result<String, JsValue> {
    with_collab_and_app(|driver, app| status_json(driver, app))
}
