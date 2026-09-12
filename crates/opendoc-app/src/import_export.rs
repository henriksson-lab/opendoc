use super::*;

impl OpenDocApp {
    pub fn import_google_docs_json_text(
        &mut self,
        title: impl Into<String>,
        json_text: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        ImportExportService::new(self)
            .import_google_docs_json_text(title.into(), json_text.as_ref())
    }

    pub fn import_doc_or_docx_path(
        &mut self,
        path: impl Into<PathBuf>,
    ) -> Result<AppDocument, AppApiError> {
        ImportExportService::new(self).import_doc_or_docx_path(path.into())
    }

    /// Replace the open document with an imported one (Word import from a
    /// path or from in-memory bytes).
    pub(crate) fn adopt_import_report(
        &mut self,
        report: opendoc_import::ImportReport,
    ) -> Result<AppDocument, AppApiError> {
        ImportExportService::new(self).adopt_import_report(report)
    }

    pub fn export_google_docs_json(&self) -> Result<AppExport, AppApiError> {
        ImportExportReadService::new(self).export_google_docs_json()
    }

    pub fn import_google_sheets_json_text(
        &mut self,
        json_text: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        ImportExportService::new(self).import_google_sheets_json_text(json_text.as_ref())
    }

    pub fn export_google_sheets_json(&self) -> Result<AppExport, AppApiError> {
        ImportExportReadService::new(self).export_google_sheets_json()
    }

    /// The open document as a `.docx` package (FS-22).
    pub fn export_docx(&self) -> Result<AppExport, AppApiError> {
        ImportExportReadService::new(self).export_docx()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn app() -> OpenDocApp {
        let mut app = OpenDocApp::new_empty_document();
        app.new_document("Exported");
        app
    }

    fn export_result(app: &mut OpenDocApp) -> AppExport {
        let result = app
            .dispatch_command("export_docx", json!({}))
            .expect("export_docx failed");
        let AppCommandResult::Export(export) = result else {
            panic!("export_docx returns an export");
        };
        export
    }

    fn export(app: &mut OpenDocApp) -> Vec<u8> {
        let export = export_result(app);
        assert_eq!(AppExportEncoding::Base64, export.encoding);
        assert_eq!("docx", export.file_extension);
        base64_decode(&export.content).expect("export_docx did not return base64")
    }

    #[test]
    fn export_docx_returns_a_word_package() {
        let mut app = app();
        app.add_paragraph("A paragraph");
        let bytes = export(&mut app);
        assert!(bytes.starts_with(b"PK"), "not a zip package");
        // The bytes must survive the trip back through the reader that owns
        // the other direction.
        let report = opendoc_import::import_docx_bytes("Exported", &bytes).unwrap();
        assert!(report.document.visible_text().contains("A paragraph"));
    }

    #[test]
    fn export_docx_embeds_the_bytes_behind_an_image_block() {
        let mut app = app();
        let png = vec![
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x03, 0x08, 0x02, 0x00, 0x00,
            0x00, 0x36, 0x88, 0x49, 0xd6, 0x00, 0x00, 0x00, 0x10, 0x49, 0x44, 0x41, 0x54, 0x78,
            0xda, 0x63, 0x38, 0x61, 0x64, 0x04, 0x44, 0x0c, 0x28, 0x14, 0x00, 0x4d, 0x6d, 0x07,
            0x09, 0x59, 0xaa, 0x18, 0x7e, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae,
            0x42, 0x60, 0x82,
        ];
        app.add_binary_blob("square.png", "image/png", png.clone())
            .unwrap();
        let hash = app.blobs[0].hash.clone();
        app.add_image_block(&hash, "a square").unwrap();
        let bytes = export(&mut app);
        let report = opendoc_import::import_docx_bytes("Exported", &bytes).unwrap();
        assert_eq!(1, report.blobs.len(), "the image blob was not embedded");
        assert_eq!(png, report.blobs[0].bytes);
        assert_eq!(hash, report.blobs[0].hash);
    }

    /// Exporting reads the document; it must not need the unsaved-changes
    /// guard and must not be refused while the document has unsaved edits.
    #[test]
    fn export_docx_does_not_replace_the_open_document() {
        let mut app = app();
        app.add_paragraph("unsaved");
        assert!(!export(&mut app).is_empty());
    }

    #[test]
    fn export_warnings_reach_the_caller_in_the_command_result() {
        let mut app = app();
        // A checklist is the case PLAN77 named: Google's schema has no
        // checkbox, so the item silently became a plain bullet and the user
        // was told nothing.
        app.add_list_item("buy milk", 0, "checklist").unwrap();
        let AppCommandResult::Export(export) = app
            .dispatch_command("export_google_docs_json", json!({}))
            .expect("export_google_docs_json failed")
        else {
            panic!("export_google_docs_json returns an export");
        };
        assert_eq!(AppExportEncoding::Text, export.encoding);
        assert_eq!("application/json", export.media_type);
        assert_eq!("json", export.file_extension);
        assert!(
            export
                .warnings
                .iter()
                .any(|warning| warning.code.contains("checklist")),
            "a checklist exported to Google JSON said nothing: {:?}",
            export.warnings
        );
    }

    /// ADR 0010. An export reads the document; it must not write to it.
    ///
    /// The wrong fix for "export warnings have nowhere to go" is
    /// `push_model_warning`, which appends to `document.warnings` — signed
    /// source state. This asserts the property that fix would break, over a
    /// document whose exports really do warn, so it cannot pass by exporting
    /// something with nothing to report.
    #[test]
    fn exporting_a_signed_document_changes_neither_its_bytes_nor_its_signature() {
        const TEST_ED25519_PRIVATE_KEY: &str = r#"
-----BEGIN OPENSSH PRIVATE KEY-----
b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW
QyNTUxOQAAACCzPq7zfqLffKoBDe/eo04kH2XxtSmk9D7RQyf1xUqrYgAAAJgAIAxdACAM
XQAAAAtzc2gtZWQyNTUxOQAAACCzPq7zfqLffKoBDe/eo04kH2XxtSmk9D7RQyf1xUqrYg
AAAEC2BsIi0QwW2uFscKTUUXNHLsYX4FxlaSDSblbAj7WR7bM+rvN+ot98qgEN796jTiQf
ZfG1KaT0PtFDJ/XFSqtiAAAAEHVzZXJAZXhhbXBsZS5jb20BAgMEBQ==
-----END OPENSSH PRIVATE KEY-----
"#;

        let mut app = app();
        app.add_paragraph("signed and exported");
        app.add_list_item("buy milk", 0, "checklist").unwrap();
        app.sign_with_openssh_private_key(TEST_ED25519_PRIVATE_KEY, "Ada")
            .expect("signing failed");
        let signature_before = app
            .verify_current_signature(TEST_ED25519_PRIVATE_KEY)
            .expect("verification failed");
        assert_eq!(
            "trusted", signature_before,
            "the fixture is not signed, so this test proves nothing"
        );

        let payload_before = app.snapshot_payload().expect("snapshot before");
        let warnings_before = app.document.warnings.clone();
        let dirty_before = app.has_unsaved_changes();

        for command in [
            "export_google_docs_json",
            "export_docx",
            "export_google_sheets_json",
        ] {
            let AppCommandResult::Export(export) = app
                .dispatch_command(command, json!({}))
                .unwrap_or_else(|err| panic!("{command} failed: {err}"))
            else {
                panic!("{command} returns an export");
            };
            assert!(!export.content.is_empty(), "{command} produced nothing");
        }
        // The two document exports must actually have had something to
        // report, or the assertions below would hold for the wrong reason.
        let AppCommandResult::Export(docx) = app
            .dispatch_command("export_docx", json!({}))
            .expect("export_docx failed")
        else {
            panic!("export_docx returns an export");
        };
        assert!(
            !docx.warnings.is_empty(),
            "the fixture exports without warnings, so this test proves nothing"
        );

        assert_eq!(
            payload_before,
            app.snapshot_payload().expect("snapshot after"),
            "exporting changed the bytes the signature is taken over"
        );
        assert_eq!(
            warnings_before, app.document.warnings,
            "an export wrote its warnings into the document"
        );
        assert_eq!(
            dirty_before,
            app.has_unsaved_changes(),
            "exporting marked the document as edited"
        );
        assert_eq!(
            signature_before,
            app.verify_current_signature(TEST_ED25519_PRIVATE_KEY)
                .expect("verification after export failed"),
            "exporting broke the signature"
        );
    }
}
