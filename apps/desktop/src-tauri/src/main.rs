use opendoc_app_api::{AppCommandResult, AppDocument, OpenDocApp};
use serde_json::{json, Value};
use std::sync::Mutex;

struct DesktopState {
    app: Mutex<OpenDocApp>,
}

#[tauri::command]
fn create_document(
    state: tauri::State<'_, DesktopState>,
    title: String,
) -> Result<AppDocument, String> {
    dispatch_document(state, "create_document", json!({ "title": title }))
}

#[tauri::command]
fn get_document(state: tauri::State<'_, DesktopState>) -> Result<AppDocument, String> {
    dispatch_document(state, "get_document", json!({}))
}

#[tauri::command]
fn add_paragraph(
    state: tauri::State<'_, DesktopState>,
    text: String,
) -> Result<AppDocument, String> {
    dispatch_document(state, "add_paragraph", json!({ "text": text }))
}

#[tauri::command]
fn add_heading(
    state: tauri::State<'_, DesktopState>,
    text: String,
    level: u8,
) -> Result<AppDocument, String> {
    dispatch_document(
        state,
        "add_heading",
        json!({ "text": text, "level": level }),
    )
}

#[tauri::command]
fn add_link(
    state: tauri::State<'_, DesktopState>,
    text: String,
    href: String,
) -> Result<AppDocument, String> {
    dispatch_document(state, "add_link", json!({ "text": text, "href": href }))
}

#[tauri::command]
fn add_mention(
    state: tauri::State<'_, DesktopState>,
    label: String,
) -> Result<AppDocument, String> {
    dispatch_document(state, "add_mention", json!({ "label": label }))
}

#[tauri::command]
fn add_footnote_ref(state: tauri::State<'_, DesktopState>) -> Result<AppDocument, String> {
    dispatch_document(state, "add_footnote_ref", json!({}))
}

#[tauri::command]
fn add_equation(
    state: tauri::State<'_, DesktopState>,
    source: String,
) -> Result<AppDocument, String> {
    dispatch_document(state, "add_equation", json!({ "source": source }))
}

#[tauri::command]
fn add_equation_block(
    state: tauri::State<'_, DesktopState>,
    source: String,
) -> Result<AppDocument, String> {
    dispatch_document(state, "add_equation_block", json!({ "source": source }))
}

#[tauri::command]
fn add_list_item(
    state: tauri::State<'_, DesktopState>,
    text: String,
    level: u8,
    ordered: bool,
) -> Result<AppDocument, String> {
    dispatch_document(
        state,
        "add_list_item",
        json!({ "text": text, "level": level, "ordered": ordered }),
    )
}

#[tauri::command]
fn add_page_break(state: tauri::State<'_, DesktopState>) -> Result<AppDocument, String> {
    dispatch_document(state, "add_page_break", json!({}))
}

#[tauri::command]
fn add_table(state: tauri::State<'_, DesktopState>) -> Result<AppDocument, String> {
    dispatch_document(state, "add_table", json!({}))
}

#[tauri::command]
fn add_citation(state: tauri::State<'_, DesktopState>) -> Result<AppDocument, String> {
    dispatch_document(state, "add_citation", json!({}))
}

#[tauri::command]
fn add_comment(
    state: tauri::State<'_, DesktopState>,
    author: String,
    body: String,
) -> Result<AppDocument, String> {
    dispatch_document(
        state,
        "add_comment",
        json!({ "author": author, "body": body }),
    )
}

#[tauri::command]
fn add_suggestion(
    state: tauri::State<'_, DesktopState>,
    author: String,
    text: String,
) -> Result<AppDocument, String> {
    dispatch_document(
        state,
        "add_suggestion",
        json!({ "author": author, "text": text }),
    )
}

#[tauri::command]
fn update_inline_text(
    state: tauri::State<'_, DesktopState>,
    inline_id: String,
    text: String,
) -> Result<AppDocument, String> {
    dispatch_document(
        state,
        "update_inline_text",
        json!({ "inlineId": inline_id, "text": text }),
    )
}

#[tauri::command]
fn delete_comment_thread(
    state: tauri::State<'_, DesktopState>,
    thread_id: String,
) -> Result<AppDocument, String> {
    dispatch_document(
        state,
        "delete_comment_thread",
        json!({ "threadId": thread_id }),
    )
}

#[tauri::command]
fn accept_suggestion(
    state: tauri::State<'_, DesktopState>,
    suggestion_id: String,
    accepted_by: String,
) -> Result<AppDocument, String> {
    dispatch_document(
        state,
        "accept_suggestion",
        json!({ "suggestionId": suggestion_id, "acceptedBy": accepted_by }),
    )
}

#[tauri::command]
fn reject_suggestion(
    state: tauri::State<'_, DesktopState>,
    suggestion_id: String,
    rejected_by: String,
) -> Result<AppDocument, String> {
    dispatch_document(
        state,
        "reject_suggestion",
        json!({ "suggestionId": suggestion_id, "rejectedBy": rejected_by }),
    )
}

#[tauri::command]
fn add_text_mark(
    state: tauri::State<'_, DesktopState>,
    inline_id: String,
    mark_kind: String,
) -> Result<AppDocument, String> {
    dispatch_document(
        state,
        "add_text_mark",
        json!({ "inlineId": inline_id, "markKind": mark_kind }),
    )
}

#[tauri::command]
fn update_block_equation_source(
    state: tauri::State<'_, DesktopState>,
    block_id: String,
    source: String,
) -> Result<AppDocument, String> {
    dispatch_document(
        state,
        "update_block_equation_source",
        json!({ "blockId": block_id, "source": source }),
    )
}

#[tauri::command]
fn set_spreadsheet_cell(
    state: tauri::State<'_, DesktopState>,
    address: String,
    value: String,
) -> Result<AppDocument, String> {
    dispatch_document(
        state,
        "set_spreadsheet_cell",
        json!({ "address": address, "value": value }),
    )
}

#[tauri::command]
fn update_bibliography_reference(
    state: tauri::State<'_, DesktopState>,
    reference_id: String,
    title: String,
    issued: Option<String>,
) -> Result<AppDocument, String> {
    dispatch_document(
        state,
        "update_bibliography_reference",
        json!({ "referenceId": reference_id, "title": title, "issued": issued }),
    )
}

#[tauri::command]
fn save_local_repository(
    state: tauri::State<'_, DesktopState>,
    path: String,
) -> Result<AppDocument, String> {
    dispatch_document(state, "save_local_repository", json!({ "path": path }))
}

#[tauri::command]
fn open_local_repository(
    state: tauri::State<'_, DesktopState>,
    path: String,
    document_uuid: String,
) -> Result<AppDocument, String> {
    dispatch_document(
        state,
        "open_local_repository",
        json!({ "path": path, "documentUuid": document_uuid }),
    )
}

#[tauri::command]
fn sign_with_openssh_private_key(
    state: tauri::State<'_, DesktopState>,
    private_key_pem: String,
    signer_display: String,
) -> Result<AppDocument, String> {
    dispatch_document(
        state,
        "sign_with_openssh_private_key",
        json!({ "privateKeyPem": private_key_pem, "signerDisplay": signer_display }),
    )
}

#[tauri::command]
fn verify_current_signature(
    state: tauri::State<'_, DesktopState>,
    private_key_pem: String,
) -> Result<String, String> {
    dispatch_text(
        state,
        "verify_current_signature",
        json!({ "privateKeyPem": private_key_pem }),
    )
}

fn dispatch_document(
    state: tauri::State<'_, DesktopState>,
    command: &str,
    args: Value,
) -> Result<AppDocument, String> {
    match dispatch(state, command, args)? {
        AppCommandResult::Document(document) => Ok(document),
        AppCommandResult::Text(_) => Err(format!("command {command} did not return a document")),
    }
}

fn dispatch_text(
    state: tauri::State<'_, DesktopState>,
    command: &str,
    args: Value,
) -> Result<String, String> {
    match dispatch(state, command, args)? {
        AppCommandResult::Text(value) => Ok(value),
        AppCommandResult::Document(_) => Err(format!("command {command} did not return text")),
    }
}

fn dispatch(
    state: tauri::State<'_, DesktopState>,
    command: &str,
    args: Value,
) -> Result<AppCommandResult, String> {
    let mut app = state.app.lock().map_err(|err| err.to_string())?;
    app.dispatch_command(command, args)
        .map_err(|err| err.to_string())
}

fn main() {
    tauri::Builder::default()
        .manage(DesktopState {
            app: Mutex::new(OpenDocApp::new_sample()),
        })
        .invoke_handler(tauri::generate_handler![
            create_document,
            get_document,
            add_paragraph,
            add_heading,
            add_link,
            add_mention,
            add_footnote_ref,
            add_equation,
            add_equation_block,
            add_list_item,
            add_page_break,
            add_table,
            add_citation,
            add_comment,
            add_suggestion,
            update_inline_text,
            delete_comment_thread,
            accept_suggestion,
            reject_suggestion,
            add_text_mark,
            update_block_equation_source,
            set_spreadsheet_cell,
            update_bibliography_reference,
            save_local_repository,
            open_local_repository,
            sign_with_openssh_private_key,
            verify_current_signature
        ])
        .run(tauri::generate_context!())
        .expect("failed to run OpenDoc desktop app");
}
