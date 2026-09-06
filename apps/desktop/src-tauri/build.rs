fn main() {
    const COMMANDS: &[&str] = &[
        "dispatch",
        "pick_open_path",
        "pick_save_path",
        "read_file_base64",
        "write_file_base64",
        "write_file_text",
        "close_window",
        "set_window_title",
    ];
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("failed to build OpenDoc Tauri command manifest");
}
