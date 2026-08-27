fn main() {
    const COMMANDS: &[&str] = &[
        "create_document",
        "get_document",
        "add_paragraph",
        "add_heading",
        "add_link",
        "add_mention",
        "add_footnote_ref",
        "add_equation",
        "add_equation_block",
        "add_list_item",
        "add_page_break",
        "add_table",
        "add_citation",
        "add_comment",
        "add_suggestion",
        "update_inline_text",
        "delete_comment_thread",
        "accept_suggestion",
        "reject_suggestion",
        "add_text_mark",
        "update_block_equation_source",
        "set_spreadsheet_cell",
        "update_bibliography_reference",
        "save_local_repository",
        "open_local_repository",
        "sign_with_openssh_private_key",
        "verify_current_signature",
    ];

    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("failed to build OpenDoc Tauri command manifest");
}
