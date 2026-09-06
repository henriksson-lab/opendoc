//! Browser entry point: the same `dispatch_command` surface the Tauri shell
//! uses, compiled to WebAssembly so the web build runs the real Rust core
//! instead of a JavaScript re-implementation.

use opendoc_app_api::OpenDocApp;
use std::cell::RefCell;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
}

thread_local! {
    static APP: RefCell<Option<OpenDocApp>> = const { RefCell::new(None) };
}

fn with_app<T>(f: impl FnOnce(&mut OpenDocApp) -> T) -> Result<T, JsValue> {
    APP.with(|cell| {
        let mut slot = cell
            .try_borrow_mut()
            .map_err(|_| JsValue::from_str("OpenDoc core is busy (re-entrant call)"))?;
        let app = slot.get_or_insert_with(OpenDocApp::new_sample);
        Ok(f(app))
    })
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
    let result = with_app(|app| app.dispatch_command(command, args))?
        .map_err(|err| JsValue::from_str(&err.to_string()))?;
    serde_json::to_string(&result).map_err(|err| JsValue::from_str(&err.to_string()))
}

/// Replace the in-memory app with a fresh sample document (tests/demo).
#[wasm_bindgen]
pub fn reset() {
    APP.with(|cell| {
        *cell.borrow_mut() = Some(OpenDocApp::new_sample());
    });
}

/// Names of every command accepted by `dispatch`.
#[wasm_bindgen]
pub fn command_names() -> String {
    serde_json::to_string(&opendoc_app_api::command_names()).unwrap_or_else(|_| "[]".to_string())
}
