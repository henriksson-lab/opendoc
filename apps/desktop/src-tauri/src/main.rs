//! OpenDoc desktop shell. The whole app API is reached through one generic
//! `dispatch` command (the same surface the WebAssembly build exposes); the
//! shell only adds what a browser page cannot do itself: native file
//! dialogs, file IO, and window lifecycle.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use opendoc_app::{
    AppCommandResult, FileRecentDocumentStore, FileRecoveryJournalStore, OpenDocApp,
};
use serde_json::Value;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};
use tauri_plugin_dialog::{DialogExt, FilePath};

mod bridge;
mod collab;
mod fetch;
mod fileaccess;

pub(crate) use fileaccess::{media_type_for, Access, FileContents, FileGrants};

// Imported by name so `generate_handler!` keeps a flat list of command names:
// that list is the ACL surface, and `scripts/native-check.mjs` reads it to
// prove every command has a permission and every permission has a command.
use fetch::fetch_url_base64;
use fileaccess::{read_file_base64, write_file_base64, write_file_text};

struct DesktopState {
    /// `Arc` because the collaboration session thread holds the document too
    /// (`collab.rs`). It takes the same lock the `dispatch` command takes, and
    /// never across an `await`.
    app: Arc<Mutex<OpenDocApp>>,
}

/// The collaboration transport, if a session was ever started.
///
/// Managed separately from `DesktopState` because it needs an `AppHandle` to
/// push statuses with, which does not exist until `setup`.
struct CollabState {
    collab: Mutex<Option<Arc<collab::NativeCollab>>>,
}

fn lock_app(state: &DesktopState) -> Result<std::sync::MutexGuard<'_, OpenDocApp>, String> {
    state
        .app
        .lock()
        .map_err(|_| "application state is poisoned".to_string())
}

#[tauri::command]
fn dispatch(
    state: tauri::State<'_, DesktopState>,
    command: String,
    args: Option<Value>,
) -> Result<AppCommandResult, String> {
    let mut app = lock_app(&state)?;
    app.dispatch_command(&command, args.unwrap_or(Value::Object(Default::default())))
        .map_err(|err| err.to_string())
}

// ---- Collaboration -------------------------------------------------------
//
// The native shell owns the socket, using `opendoc-service`'s own client
// (`collab.rs`, docs/adr/0018). The page never sees a frame and never sees the
// session token; it asks for a connection and renders the statuses that come
// back on `opendoc://collab-status`.

fn collab_session(
    app: &tauri::AppHandle,
    state: &CollabState,
) -> Result<Arc<collab::NativeCollab>, String> {
    let mut slot = state
        .collab
        .lock()
        .map_err(|_| "collaboration state is poisoned".to_string())?;
    if let Some(existing) = slot.as_ref() {
        return Ok(Arc::clone(existing));
    }
    let document = Arc::clone(&app.state::<DesktopState>().app);
    let emitter = app.clone();
    let session = Arc::new(collab::NativeCollab::new(
        document,
        Arc::new(move |status| {
            let _ = emitter.emit("opendoc://collab-status", status);
        }),
    ));
    *slot = Some(Arc::clone(&session));
    Ok(session)
}

#[tauri::command]
fn collab_connect(
    app: tauri::AppHandle,
    state: tauri::State<'_, CollabState>,
    options: collab::ConnectOptions,
) -> Result<collab::CollabStatus, String> {
    collab_session(&app, &state)?.connect(options)
}

#[tauri::command]
fn collab_disconnect(
    app: tauri::AppHandle,
    state: tauri::State<'_, CollabState>,
) -> Result<collab::CollabStatus, String> {
    Ok(collab_session(&app, &state)?.disconnect())
}

#[tauri::command]
fn collab_cursor(
    app: tauri::AppHandle,
    state: tauri::State<'_, CollabState>,
    anchor: Option<String>,
) -> Result<(), String> {
    collab_session(&app, &state)?.set_cursor(anchor.filter(|value| !value.trim().is_empty()));
    Ok(())
}

#[tauri::command]
fn collab_selection_anchor(
    app: tauri::AppHandle,
    state: tauri::State<'_, CollabState>,
    anchor: Option<String>,
) -> Result<(), String> {
    collab_session(&app, &state)?
        .set_selection_anchor(anchor.filter(|value| !value.trim().is_empty()));
    Ok(())
}

#[tauri::command]
fn collab_selection(
    app: tauri::AppHandle,
    state: tauri::State<'_, CollabState>,
    selection: Option<opendoc_app::EditorSelection>,
) -> Result<(), String> {
    collab_session(&app, &state)?.set_selection(selection);
    Ok(())
}

#[tauri::command]
fn collab_status(
    app: tauri::AppHandle,
    state: tauri::State<'_, CollabState>,
) -> Result<collab::CollabStatus, String> {
    Ok(collab_session(&app, &state)?.status())
}

/// Native ACL bridge. The page has no service bearer token: these commands
/// use the authenticated client retained by `NativeCollab`, and the service
/// remains the authority for both listing and mutation.
#[tauri::command]
async fn collab_list_grants(
    app: tauri::AppHandle,
    state: tauri::State<'_, CollabState>,
) -> Result<Vec<opendoc_service::GrantView>, String> {
    let collab = collab_session(&app, &state)?;
    collab.list_grants().await
}

#[tauri::command]
async fn collab_list_grant_audit(
    app: tauri::AppHandle,
    state: tauri::State<'_, CollabState>,
) -> Result<Vec<opendoc_service::GrantAuditView>, String> {
    let collab = collab_session(&app, &state)?;
    collab.list_grant_audit().await
}

#[tauri::command]
async fn collab_set_grant(
    app: tauri::AppHandle,
    state: tauri::State<'_, CollabState>,
    subject: String,
    role: Option<String>,
) -> Result<(), String> {
    let collab = collab_session(&app, &state)?;
    collab.set_grant(subject, role).await
}

#[tauri::command]
fn collab_share_link(
    app: tauri::AppHandle,
    state: tauri::State<'_, CollabState>,
) -> Result<String, String> {
    collab_session(&app, &state)?.share_link()
}

fn file_path_string(path: FilePath) -> Option<String> {
    match path {
        FilePath::Path(path) => Some(path.to_string_lossy().to_string()),
        FilePath::Url(url) => url
            .to_file_path()
            .ok()
            .map(|p| p.to_string_lossy().to_string()),
    }
}

/// Native "open" dialog. `extensions` filters by file extension (without
/// dots); `directory` picks a folder instead of a file.
///
/// Picking a *file* also mints the one-shot read grant that
/// [`fileaccess::read_file_base64`] spends — the user choosing a file in a
/// native dialog is the only thing that authorises reading it. Picking a
/// folder mints nothing: a folder is not read through this command, it is
/// handed to the app as a repository root.
#[tauri::command]
async fn pick_open_path(
    app: tauri::AppHandle,
    grants: tauri::State<'_, FileGrants>,
    title: Option<String>,
    extensions: Option<Vec<String>>,
    directory: Option<bool>,
) -> Result<Option<String>, String> {
    let mut dialog = app.dialog().file();
    if let Some(title) = title {
        dialog = dialog.set_title(title);
    }
    if let Some(extensions) = extensions.filter(|list| !list.is_empty()) {
        let refs: Vec<&str> = extensions.iter().map(String::as_str).collect();
        dialog = dialog.add_filter("Supported files", &refs);
    }
    let (tx, rx) = std::sync::mpsc::channel();
    if directory.unwrap_or(false) {
        dialog.pick_folder(move |path| {
            let _ = tx.send(path);
        });
    } else {
        dialog.pick_file(move |path| {
            let _ = tx.send(path);
        });
    }
    let picked = rx.recv().map_err(|err| err.to_string())?;
    let picked = picked.and_then(file_path_string);
    if let Some(path) = &picked {
        if !directory.unwrap_or(false) {
            grants.mint(Path::new(path), Access::Read);
        }
    }
    Ok(picked)
}

/// Native "save as" dialog. Mints the one-shot write grant the write commands
/// spend; see [`fileaccess`].
#[tauri::command]
async fn pick_save_path(
    app: tauri::AppHandle,
    grants: tauri::State<'_, FileGrants>,
    title: Option<String>,
    default_name: Option<String>,
    extensions: Option<Vec<String>>,
) -> Result<Option<String>, String> {
    let mut dialog = app.dialog().file();
    if let Some(title) = title {
        dialog = dialog.set_title(title);
    }
    if let Some(name) = default_name {
        dialog = dialog.set_file_name(name);
    }
    if let Some(extensions) = extensions.filter(|list| !list.is_empty()) {
        let refs: Vec<&str> = extensions.iter().map(String::as_str).collect();
        dialog = dialog.add_filter("Supported files", &refs);
    }
    let (tx, rx) = std::sync::mpsc::channel();
    dialog.save_file(move |path| {
        let _ = tx.send(path);
    });
    let picked = rx.recv().map_err(|err| err.to_string())?;
    let picked = picked.and_then(file_path_string);
    if let Some(path) = &picked {
        grants.mint(Path::new(path), Access::Write);
    }
    Ok(picked)
}

/// Close the main window even if there are unsaved changes (the frontend
/// asks the user first).
#[tauri::command]
fn close_window(window: tauri::Window) -> Result<(), String> {
    window.destroy().map_err(|err| err.to_string())
}

/// Set the OS window title (document title plus dirty marker).
#[tauri::command]
fn set_window_title(window: tauri::Window, title: String) -> Result<(), String> {
    window.set_title(&title).map_err(|err| err.to_string())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        // `withGlobalTauri` is off; this puts back the two functions the page
        // actually uses. See `bridge.rs`.
        .plugin(bridge::init())
        .manage(FileGrants::default())
        .manage(DesktopState {
            app: Arc::new(Mutex::new(OpenDocApp::new_empty_document())),
        })
        .manage(CollabState {
            collab: Mutex::new(None),
        })
        .setup(|app| {
            // Both of these are local-runtime capabilities whose storage is a
            // path only this shell knows: the app-data directory comes from
            // the bundle identifier and differs per platform, so the app is
            // *given* a store rather than guessing one (ADR 0005, and
            // `opendoc-app/src/recent.rs` for the same decision about
            // recents). A browser build installs its own over the IndexedDB
            // volume; a runtime that installs neither keeps working, without
            // crash protection and without recents that outlive the process.
            match app.path().app_data_dir() {
                Ok(dir) => match app.state::<DesktopState>().app.lock() {
                    Ok(mut opendoc) => {
                        let recovery =
                            Arc::new(FileRecoveryJournalStore::new(dir.join("recovery")));
                        if let Err(err) = opendoc.install_recovery_journal(recovery) {
                            eprintln!("crash recovery journal unavailable: {err}");
                        }
                        // Beside the recovery segments, and with no `Result`
                        // to ignore: an unreadable list is reported to the
                        // user as a model warning, never as a failure to
                        // start (`OpenDocApp::install_recent_documents`).
                        opendoc.install_recent_documents(Arc::new(FileRecentDocumentStore::new(
                            dir.join("recent-documents"),
                        )));
                    }
                    Err(_) => eprintln!("durable local storage unavailable: state poisoned"),
                },
                Err(err) => eprintln!("durable local storage unavailable: {err}"),
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // Same dirty-state signal the dispatcher's replacement guard
                // and the autosave loop use, read straight from the Rust app.
                let unsaved = window
                    .state::<DesktopState>()
                    .app
                    .lock()
                    .map(|app| app.has_unsaved_changes())
                    .unwrap_or(false);
                if unsaved {
                    api.prevent_close();
                    let _ = window.emit("opendoc://close-requested", ());
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            dispatch,
            pick_open_path,
            pick_save_path,
            read_file_base64,
            fetch_url_base64,
            write_file_base64,
            write_file_text,
            close_window,
            set_window_title,
            collab_connect,
            collab_disconnect,
            collab_cursor,
            collab_selection_anchor,
            collab_selection,
            collab_status,
            collab_list_grants,
            collab_list_grant_audit,
            collab_set_grant,
            collab_share_link,
        ])
        .run(tauri::generate_context!())
        .expect("error while running OpenDoc desktop");
}
