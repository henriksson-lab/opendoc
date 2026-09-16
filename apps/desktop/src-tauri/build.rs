fn main() {
    const COMMANDS: &[&str] = &[
        "dispatch",
        "pick_open_path",
        "pick_save_path",
        "read_file_base64",
        "fetch_url_base64",
        "write_file_base64",
        "write_file_text",
        "close_window",
        "set_window_title",
        // Live-collaboration transport, added with the WebSocket client.
        "collab_connect",
        "collab_disconnect",
        "collab_cursor",
        "collab_selection_anchor",
        "collab_selection",
        "collab_status",
        "collab_list_grants",
        "collab_list_grant_audit",
        "collab_set_grant",
        "collab_share_link",
    ];
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("failed to build OpenDoc Tauri command manifest");
}
