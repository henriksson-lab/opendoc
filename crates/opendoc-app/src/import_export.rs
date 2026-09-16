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

    /// The open document as an `.odt` (OpenDocument Text) package.
    ///
    /// Not yet reachable as a command: an `export_odt` spec belongs in
    /// `opendoc-api`. Wiring it is one dispatch arm once that exists.
    pub fn export_odt(&self) -> Result<AppExport, AppApiError> {
        ImportExportReadService::new(self).export_odt()
    }

    /// The open document as a PDF (FS-21), paginated by `opendoc-layout`.
    pub fn export_pdf(&self) -> Result<AppExport, AppApiError> {
        ImportExportReadService::new(self).export_pdf()
    }

    /// The open document as one standalone HTML file.
    pub fn export_html(&self) -> Result<AppExport, AppApiError> {
        ImportExportReadService::new(self).export_html()
    }

    /// The open document as plain text.
    pub fn export_text(&self) -> Result<AppExport, AppApiError> {
        ImportExportReadService::new(self).export_text()
    }

    /// The original bytes of one stored image. This does not transcode: the
    /// returned media type and extension name the actual source bytes.
    pub fn export_image_blob(&self, blob_hash: impl AsRef<str>) -> Result<AppExport, AppApiError> {
        ImportExportReadService::new(self).export_image_blob(blob_hash.as_ref())
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
        app.add_paragraph("A paragraph").expect("paragraph");
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

    #[test]
    fn export_image_blob_returns_the_original_png_bytes_and_type() {
        let mut app = app();
        let png = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        app.add_binary_blob("diagram.png", "image/png", png.clone())
            .unwrap();
        let hash = app.blobs[0].hash.clone();

        let export = app.export_image_blob(&hash).expect("image export");
        assert_eq!(AppExportEncoding::Base64, export.encoding);
        assert_eq!("image/png", export.media_type);
        assert_eq!("png", export.file_extension);
        assert_eq!(png, base64_decode(&export.content).expect("image base64"));
        assert!(export.warnings.is_empty());
    }

    #[test]
    fn export_image_blob_uses_the_declared_kind_for_a_parameterised_or_modern_raw_image() {
        let mut app = app();
        let svg = b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>".to_vec();
        app.add_binary_blob(
            "misleading.jpeg",
            "Image/SVG+XML; charset=utf-8",
            svg.clone(),
        )
        .unwrap();
        let svg_hash = app.blobs[0].hash.clone();
        let svg_export = app.export_image_blob(&svg_hash).expect("SVG export");
        assert_eq!("Image/SVG+XML; charset=utf-8", svg_export.media_type);
        assert_eq!("svg", svg_export.file_extension);
        assert_eq!(svg, base64_decode(&svg_export.content).expect("SVG base64"));

        let avif = b"raw avif bytes".to_vec();
        app.add_binary_blob("unnamed blob", "image/avif", avif.clone())
            .unwrap();
        let avif_hash = app.blobs[1].hash.clone();
        let avif_export = app.export_image_blob(&avif_hash).expect("AVIF export");
        assert_eq!("image/avif", avif_export.media_type);
        assert_eq!("avif", avif_export.file_extension);
        assert_eq!(
            avif,
            base64_decode(&avif_export.content).expect("AVIF base64")
        );
    }

    #[test]
    fn export_image_blob_does_not_project_an_unsafe_suffix_for_an_unknown_image_kind() {
        let mut app = app();
        let bytes = b"opaque future image format".to_vec();
        app.add_binary_blob("untrusted-name.EXE", "image/x-future", bytes.clone())
            .unwrap();
        let hash = app.blobs[0].hash.clone();

        let export = app.export_image_blob(&hash).expect("raw image export");
        assert_eq!("image/x-future", export.media_type);
        assert_eq!("img", export.file_extension);
        assert_eq!(bytes, base64_decode(&export.content).expect("source bytes"));

        app.add_binary_blob("still-an-image.qoi", "image/x-future", vec![1])
            .unwrap();
        let qoi_hash = app.blobs[1].hash.clone();
        assert_eq!(
            "qoi",
            app.export_image_blob(&qoi_hash)
                .expect("opaque image suffix")
                .file_extension
        );
    }

    #[test]
    fn export_image_blob_refuses_an_unavailable_or_non_image_blob() {
        let mut app = app();
        app.add_binary_blob("notes.txt", "text/plain", b"notes".to_vec())
            .unwrap();
        let text_hash = app.blobs[0].hash.clone();
        assert!(matches!(
            app.export_image_blob(&text_hash),
            Err(AppApiError::Format(_))
        ));

        app.add_binary_blob("photo.jpg", "image/jpeg", vec![0xff, 0xd8])
            .unwrap();
        let image_hash = app.blobs[1].hash.clone();
        app.blob_bytes.remove(&image_hash);
        assert!(matches!(
            app.export_image_blob(&image_hash),
            Err(AppApiError::NotFound(_))
        ));
    }

    /// Exporting reads the document; it must not need the unsaved-changes
    /// guard and must not be refused while the document has unsaved edits.
    #[test]
    fn export_docx_does_not_replace_the_open_document() {
        let mut app = app();
        app.add_paragraph("unsaved").expect("paragraph");
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

    fn export_of(app: &mut OpenDocApp, command: &str) -> AppExport {
        let AppCommandResult::Export(export) = app
            .dispatch_command(command, json!({}))
            .unwrap_or_else(|err| panic!("{command} failed: {err}"))
        else {
            panic!("{command} returns an export");
        };
        export
    }

    #[test]
    fn export_pdf_returns_a_pdf_that_states_its_own_type() {
        let mut app = app();
        app.add_paragraph("A paragraph on paper")
            .expect("paragraph");
        let export = export_of(&mut app, "export_pdf");
        assert_eq!(AppExportEncoding::Base64, export.encoding);
        assert_eq!("application/pdf", export.media_type);
        assert_eq!("pdf", export.file_extension);
        let bytes = base64_decode(&export.content).expect("export_pdf did not return base64");
        assert!(bytes.starts_with(b"%PDF-"), "not a PDF");
        assert!(
            export.warnings.is_empty(),
            "a plain paragraph warned: {:?}",
            export.warnings
        );
    }

    #[test]
    fn export_pdf_hands_a_reachable_png_to_the_bitmap_writer() {
        let mut app = app();
        let png = vec![
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x03, 0x08, 0x02, 0x00, 0x00,
            0x00, 0x36, 0x88, 0x49, 0xd6, 0x00, 0x00, 0x00, 0x10, 0x49, 0x44, 0x41, 0x54, 0x78,
            0xda, 0x63, 0x38, 0x61, 0x64, 0x04, 0x44, 0x0c, 0x28, 0x14, 0x00, 0x4d, 0x6d, 0x07,
            0x09, 0x59, 0xaa, 0x18, 0x7e, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae,
            0x42, 0x60, 0x82,
        ];
        app.add_binary_blob("square.png", "image/png", png).unwrap();
        let hash = app.blobs[0].hash.clone();
        app.add_image_block(&hash, "a square").unwrap();
        let export = app.export_pdf().expect("pdf export");
        assert!(
            !export
                .warnings
                .iter()
                .any(|warning| warning.code == "pdf-image-not-drawn"),
            "the app did not pass image bytes to PDF: {:?}",
            export.warnings
        );
        let bytes = base64_decode(&export.content).expect("pdf base64");
        assert!(
            String::from_utf8_lossy(&bytes).contains("/Subtype /Image"),
            "PDF has no image XObject"
        );
    }

    /// FS-21's whole point, stated as a test: the PDF has the pages the
    /// editor shows, because both come from `opendoc-layout` (ADR 0014).
    #[test]
    fn the_pdf_has_the_pages_the_editor_paginated() {
        let mut app = app();
        for index in 0..80 {
            app.add_paragraph(format!(
                "Paragraph {index}: the quick brown fox jumps over the lazy dog."
            ))
            .expect("paragraph");
        }
        let pages = app.layout_document().page_count;
        assert!(pages > 1, "the fixture fits on one page");
        let bytes = base64_decode(&export_of(&mut app, "export_pdf").content).expect("base64");
        let text = String::from_utf8_lossy(&bytes);
        assert_eq!(pages as usize, text.matches("/Type /Page\n").count());
    }

    /// ADR 0010 said an export carries its own warnings, and this one used to
    /// hand back a hard-coded empty list — which reads as "nothing was lost"
    /// and meant "nobody asked". Google's schema states a row height, so the
    /// OpenDoc writer must either project it or name the loss.
    #[test]
    fn export_google_sheets_json_names_what_its_shape_cannot_carry() {
        let mut app = app();
        app.set_spreadsheet_row_height("sheet-1", "1", 48)
            .expect("row height");
        let export = export_of(&mut app, "export_google_sheets_json");
        assert!(export.warnings.is_empty(), "{:?}", export.warnings);
        assert!(
            export.content.contains("\"pixelSize\": 48"),
            "{}",
            export.content
        );
    }

    /// The other half of the same rule: a workbook with nothing unstatable
    /// warns about nothing, so the channel cannot pass by warning always.
    #[test]
    fn export_google_sheets_json_is_silent_about_a_workbook_it_can_state() {
        let mut app = app();
        let export = export_of(&mut app, "export_google_sheets_json");
        assert!(
            export.warnings.is_empty(),
            "a plain workbook warned: {:?}",
            export.warnings
        );
    }

    /// ADR 0010 at the PDF's boundary: a page built on an estimate has to say
    /// so in the command result, not only inside the writer.
    #[test]
    fn export_pdf_passes_the_layouts_estimates_on_as_warnings() {
        let mut app = app();
        app.add_table().expect("a table");
        let export = export_of(&mut app, "export_pdf");
        assert!(
            export
                .warnings
                .iter()
                .any(|warning| warning.code == "pdf-estimated-table"),
            "a table's estimated height was exported in silence: {:?}",
            export.warnings
        );
    }

    /// `opendoc-pdf` reports one estimate per block, so a document with two
    /// unmeasurable marks handed the same code back twice and the warnings
    /// panel showed it twice. Collapsing by code is the DOCX reader's
    /// `DroppedCounter` rule applied at the export boundary.
    #[test]
    fn export_pdf_reports_each_warning_code_once() {
        let mut app = app();
        app.add_table().expect("a table");
        app.add_table().expect("a second table");
        let export = export_of(&mut app, "export_pdf");
        let mut codes: Vec<&str> = export
            .warnings
            .iter()
            .map(|warning| warning.code.as_str())
            .collect();
        let count = codes.len();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(
            count,
            codes.len(),
            "the same warning code was reported more than once: {:?}",
            export.warnings
        );
        assert!(
            codes.contains(&"pdf-estimated-table"),
            "the fixture produced no repeatable warning at all: {codes:?}"
        );
        // Collapsing must not hide how many there were.
        let table = export
            .warnings
            .iter()
            .find(|warning| warning.code == "pdf-estimated-table")
            .expect("the table warning");
        assert!(
            table.message.contains("and 1 more"),
            "the second table was collapsed away without trace: {}",
            table.message
        );
    }

    /// ODT is not a command yet — `opendoc-api` owns the spec — so this
    /// drives the facade directly. It is the seam the command will call, so
    /// wiring it later is one dispatch arm rather than new code.
    #[test]
    fn export_odt_returns_an_open_document_package_that_states_its_own_type() {
        let mut app = app();
        app.add_paragraph("A paragraph in OpenDocument")
            .expect("paragraph");
        let export = app.export_odt().expect("export_odt failed");
        assert_eq!(AppExportEncoding::Base64, export.encoding);
        assert_eq!("application/vnd.oasis.opendocument.text", export.media_type);
        assert_eq!("odt", export.file_extension);
        let bytes = base64_decode(&export.content).expect("export_odt did not return base64");
        assert!(bytes.starts_with(b"PK"), "not a zip package");
        // The ODF magic number: `mimetype`, stored uncompressed, is the first
        // entry, so the media type sits at a fixed offset in the file.
        assert_eq!(
            b"mimetypeapplication/vnd.oasis.opendocument.text",
            &bytes[30..77],
            "the package does not begin with a stored mimetype entry"
        );
        assert!(
            export.warnings.is_empty(),
            "a plain paragraph warned: {:?}",
            export.warnings
        );
    }

    /// Image blocks store only a hash, so the export has to be handed the
    /// bytes — the same problem and the same answer as the DOCX export. That
    /// the bytes land in the package verbatim is asserted in
    /// `opendoc-import`, which can open the zip; what belongs here is that
    /// the app finds them at all, so the test also shows the warning that
    /// appears when it cannot.
    #[test]
    fn export_odt_hands_the_writer_the_bytes_behind_an_image_block() {
        let mut app = app();
        let png = vec![
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x03, 0x08, 0x02, 0x00, 0x00,
            0x00, 0x36, 0x88, 0x49, 0xd6, 0x00, 0x00, 0x00, 0x10, 0x49, 0x44, 0x41, 0x54, 0x78,
            0xda, 0x63, 0x38, 0x61, 0x64, 0x04, 0x44, 0x0c, 0x28, 0x14, 0x00, 0x4d, 0x6d, 0x07,
            0x09, 0x59, 0xaa, 0x18, 0x7e, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae,
            0x42, 0x60, 0x82,
        ];
        app.add_binary_blob("square.png", "image/png", png).unwrap();
        let hash = app.blobs[0].hash.clone();
        app.add_image_block(&hash, "a square").unwrap();
        let export = app.export_odt().expect("export_odt failed");
        assert!(
            !export
                .warnings
                .iter()
                .any(|warning| warning.code == "odt-export-missing-image-blob"),
            "the app did not supply the image bytes: {:?}",
            export.warnings
        );

        // And when it genuinely cannot, the image becomes its alt text and
        // the export says so — which is what makes the assertion above mean
        // something.
        app.blob_bytes.clear();
        let export = app.export_odt().expect("export_odt failed");
        assert!(
            export
                .warnings
                .iter()
                .any(|warning| warning.code == "odt-export-missing-image-blob"),
            "a missing image blob exported in silence: {:?}",
            export.warnings
        );
    }

    #[test]
    fn export_html_is_a_whole_document_with_a_projected_stylesheet() {
        let mut app = app();
        app.add_paragraph("exported to the web").expect("paragraph");
        let export = export_of(&mut app, "export_html");
        assert_eq!(AppExportEncoding::Text, export.encoding);
        assert_eq!("text/html;charset=utf-8", export.media_type);
        assert_eq!("html", export.file_extension);
        assert!(export.content.starts_with("<!doctype html>"));
        assert!(export.content.contains("exported to the web"));
        // The sizes the layout engine measures with, not a second set.
        assert!(export.content.contains("--doc-h1-size"));
        assert!(export.content.contains("--page-width"));
    }

    #[test]
    fn export_text_is_the_documents_visible_text() {
        let mut app = app();
        app.add_paragraph("plain and simple").expect("paragraph");
        let export = export_of(&mut app, "export_text");
        assert_eq!(AppExportEncoding::Text, export.encoding);
        assert_eq!("text/plain;charset=utf-8", export.media_type);
        assert_eq!("txt", export.file_extension);
        assert_eq!(app.document.visible_text(), export.content);
    }

    /// The gap this port closed: the TypeScript HTML export pasted
    /// `body_html` into a string, so an equation the renderer could not
    /// project was exported in total silence.
    #[test]
    fn export_html_reports_what_the_projection_could_not_render() {
        let mut app = app();
        app.add_equation_block("\\nosuchcommand{x}").unwrap();
        let export = export_of(&mut app, "export_html");
        assert!(
            !export.warnings.is_empty(),
            "a broken equation exported in silence"
        );
    }

    #[test]
    fn export_text_reports_the_structure_plain_text_cannot_hold() {
        let mut app = app();
        app.add_table().expect("a table");
        let export = export_of(&mut app, "export_text");
        assert!(
            export
                .warnings
                .iter()
                .any(|warning| warning.code == "text-dropped-table"),
            "a table became text in silence: {:?}",
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
        app.add_paragraph("signed and exported").expect("paragraph");
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
            "export_pdf",
            "export_html",
            "export_text",
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
