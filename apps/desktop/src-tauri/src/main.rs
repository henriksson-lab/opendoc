//! OpenDoc desktop shell. The whole app API is reached through one generic
//! `dispatch` command (the same surface the WebAssembly build exposes); the
//! shell only adds what a browser page cannot do itself: native file
//! dialogs, file IO, and window lifecycle.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use opendoc_app::{
    base64_decode, base64_encode, AppCommandResult, FileRecoveryJournalStore, OpenDocApp,
};
use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};
use tauri_plugin_dialog::{DialogExt, FilePath};

struct DesktopState {
    app: Mutex<OpenDocApp>,
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

fn file_path_string(path: FilePath) -> Option<String> {
    match path {
        FilePath::Path(path) => Some(path.to_string_lossy().to_string()),
        FilePath::Url(url) => url.to_file_path().ok().map(|p| p.to_string_lossy().to_string()),
    }
}

/// Native "open" dialog. `extensions` filters by file extension (without
/// dots); `directory` picks a folder instead of a file.
#[tauri::command]
async fn pick_open_path(
    app: tauri::AppHandle,
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
    Ok(picked.and_then(file_path_string))
}

/// Native "save as" dialog.
#[tauri::command]
async fn pick_save_path(
    app: tauri::AppHandle,
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
    Ok(picked.and_then(file_path_string))
}

#[derive(Serialize)]
struct FileContents {
    name: String,
    path: String,
    media_type: String,
    size: usize,
    base64: String,
}

fn media_type_for(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml",
        Some("bmp") => "image/bmp",
        Some("docx") => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        Some("doc") => "application/msword",
        Some("xlsx") => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        Some("csv") => "text/csv",
        Some("tsv") => "text/tab-separated-values",
        Some("json") => "application/json",
        Some("md") => "text/markdown",
        Some("html") | Some("htm") => "text/html",
        Some("txt") => "text/plain",
        Some("pdf") => "application/pdf",
        _ => "application/octet-stream",
    }
}

#[tauri::command]
fn read_file_base64(path: String) -> Result<FileContents, String> {
    let path = PathBuf::from(path);
    let bytes = std::fs::read(&path).map_err(|err| format!("{}: {err}", path.display()))?;
    Ok(FileContents {
        name: path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default(),
        path: path.to_string_lossy().to_string(),
        media_type: media_type_for(&path).to_string(),
        size: bytes.len(),
        base64: base64_encode(&bytes),
    })
}

#[tauri::command]
fn write_file_base64(path: String, base64: String) -> Result<(), String> {
    let bytes = base64_decode(&base64).ok_or_else(|| "invalid base64 payload".to_string())?;
    std::fs::write(&path, bytes).map_err(|err| format!("{path}: {err}"))
}

#[tauri::command]
fn write_file_text(path: String, text: String) -> Result<(), String> {
    std::fs::write(&path, text).map_err(|err| format!("{path}: {err}"))
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
        .manage(DesktopState {
            app: Mutex::new(OpenDocApp::new_empty_document()),
        })
        .setup(|app| {
            // Crash recovery is a local-runtime capability: the store is a
            // directory this shell owns. A browser build installs nothing and
            // journalling stays inert (ADR 0005).
            match app.path().app_data_dir() {
                Ok(dir) => {
                    let store = Arc::new(FileRecoveryJournalStore::new(dir.join("recovery")));
                    match app.state::<DesktopState>().app.lock() {
                        Ok(mut opendoc) => {
                            if let Err(err) = opendoc.install_recovery_journal(store) {
                                eprintln!("crash recovery journal unavailable: {err}");
                            }
                        }
                        Err(_) => eprintln!("crash recovery journal unavailable: state poisoned"),
                    }
                }
                Err(err) => eprintln!("crash recovery journal unavailable: {err}"),
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
            write_file_base64,
            write_file_text,
            close_window,
            set_window_title,
        ])
        .run(tauri::generate_context!())
        .expect("error while running OpenDoc desktop");
}
