use crate::*;

pub(crate) struct ImportExportService<'a> {
    app: &'a mut OpenDocApp,
}

impl<'a> ImportExportService<'a> {
    pub(crate) fn new(app: &'a mut OpenDocApp) -> Self {
        Self { app }
    }

    pub(crate) fn import_google_docs_json_text(
        &mut self,
        title: String,
        json_text: &str,
    ) -> Result<AppDocument, AppApiError> {
        let imported_blobs = import_opendoc_blob_refs_from_google_docs_json(json_text)?;
        let report = import_google_docs_json(title, json_text.as_bytes())
            .map_err(|err| AppApiError::Import(err.to_string()))?;
        self.app.document = report.document;
        self.reset_after_document_import();

        let imported_blob_templates = imported_blobs
            .iter()
            .map(|imported| imported.blob.clone())
            .collect::<Vec<_>>();
        for imported in imported_blobs {
            let mut blob = imported.blob;
            if !blob.signatures.is_empty() {
                self.app.document.warnings.push(ModelWarning {
                    code: "opendoc-blob-exact-signature-metadata-only".to_string(),
                    message: format!(
                        "OpenDoc blob {} included exact-byte signature metadata without sidecar bytes",
                        blob.hash
                    ),
                });
            }
            if let Some(tombstone) = blob.archive_tombstone.take() {
                self.app
                    .blob_tombstones
                    .insert(blob.hash.clone(), tombstone);
            }
            if let Some(tombstone_record) = imported.tombstone_record {
                self.app
                    .blob_tombstone_records
                    .insert(blob.hash.clone(), tombstone_record);
            }
            blob.signatures.clear();
            blob.signature_state = "unsigned".to_string();
            blob.available = false;
            self.app.blobs.push(blob);
        }
        self.restore_referenced_image_blobs(&[imported_blob_templates])?;
        self.app
            .push_app_operation("import-google-docs-json", "import Google Docs JSON");
        Ok(self.app.document())
    }

    pub(crate) fn import_doc_or_docx_path(
        &mut self,
        path: PathBuf,
    ) -> Result<AppDocument, AppApiError> {
        let report =
            import_doc_or_docx(&path).map_err(|err| AppApiError::Import(err.to_string()))?;
        self.adopt_import_report(report)
    }

    pub(crate) fn adopt_import_report(
        &mut self,
        report: opendoc_import::ImportReport,
    ) -> Result<AppDocument, AppApiError> {
        self.app.document = report.document;
        self.reset_after_document_import();
        self.app.restore_imported_blobs(report.blobs);
        self.restore_referenced_image_blobs(&[])?;
        self.app
            .push_app_operation("import-doc-or-docx", "import Word document");
        Ok(self.app.document())
    }

    pub(crate) fn import_google_sheets_json_text(
        &mut self,
        json_text: &str,
    ) -> Result<AppDocument, AppApiError> {
        let imported = import_google_sheets_workbook(json_text)?;
        self.app
            .mutate_spreadsheet(SpreadsheetEvaluationPolicy::Force, |workbook| {
                *workbook = imported.workbook;
                Ok(())
            })?;
        for warning in imported.warnings {
            self.app.push_model_warning(&warning.code, warning.message);
        }
        self.app
            .push_app_operation("import-google-sheets-json", "import Google Sheets JSON");
        Ok(self.app.document())
    }

    fn reset_after_document_import(&mut self) {
        // FS-19: an imported document must not carry demo spreadsheet content.
        self.app.workbook = OpenDocApp::blank_workbook("Untitled");
        self.app.is_open = true;
        self.app.invalidate_source_state();
        self.app.clear_blob_state();
        self.app.clear_repository_binding();
        self.app.clear_edit_history();
    }

    fn restore_referenced_image_blobs(
        &mut self,
        templates: &[Vec<AppBlobRef>],
    ) -> Result<(), AppApiError> {
        let blocks = AppDocument::from_core(&self.app.document).blocks;
        let mut warnings = self
            .app
            .document
            .warnings
            .iter()
            .map(AppWarning::from_core)
            .collect::<Vec<_>>();
        restore_referenced_image_blobs(&mut self.app.blobs, &blocks, templates, &mut warnings)?;
        self.app.document.warnings = warnings.iter().map(AppWarning::to_core).collect();
        Ok(())
    }
}

pub(crate) struct ImportExportReadService<'a> {
    app: &'a OpenDocApp,
}

impl<'a> ImportExportReadService<'a> {
    pub(crate) fn new(app: &'a OpenDocApp) -> Self {
        Self { app }
    }

    /// Google Docs-shaped JSON, plus everything Google's schema could not
    /// carry.
    ///
    /// Nothing here takes `&mut self`: an export reads the document and the
    /// warnings travel out in the result, so exporting cannot dirty source
    /// state or move the bytes a signature covers (ADR 0010).
    pub(crate) fn export_google_docs_json(&self) -> Result<AppExport, AppApiError> {
        let (bytes, warnings) = export_google_docs_json_with_warnings(&self.app.document)
            .map_err(|err| AppApiError::Import(err.to_string()))?;
        let text = export_google_docs_json_with_opendoc_blobs(bytes, self.app.projected_blobs())?;
        Ok(AppExport::text(
            text,
            "application/json",
            "json",
            model_warnings(warnings),
        ))
    }

    pub(crate) fn export_google_sheets_json(&self) -> Result<AppExport, AppApiError> {
        Ok(AppExport::text(
            export_google_sheets_workbook(&self.app.workbook.evaluated())?,
            "application/json",
            "json",
            Vec::new(),
        ))
    }

    /// The document as a `.docx` package (FS-22), plus the 24-odd things
    /// WordprocessingML cannot carry exactly.
    ///
    /// Image blocks store only a content hash, so the bytes behind every
    /// reachable image are handed to the writer here; a block whose blob is
    /// not held locally is written as its alt text and named in the export
    /// warnings rather than vanishing.
    pub(crate) fn export_docx(&self) -> Result<AppExport, AppApiError> {
        let images = self.docx_images();
        let (bytes, warnings) = export_docx_with_warnings(&self.app.document, &images)
            .map_err(|err| AppApiError::Import(err.to_string()))?;
        Ok(AppExport::binary(
            &bytes,
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            "docx",
            model_warnings(warnings),
        ))
    }

    /// Every blob the document's image blocks point at, with its media type.
    fn docx_images(&self) -> BTreeMap<String, DocxImage> {
        let media_types: BTreeMap<&str, &str> = self
            .app
            .blobs
            .iter()
            .map(|blob| (blob.hash.as_str(), blob.media_type.as_str()))
            .collect();
        let mut images = BTreeMap::new();
        collect_image_hashes(
            &self.app.document.blocks,
            &mut images,
            &media_types,
            self.app,
        );
        for slot in HeaderFooterSlot::ALL {
            collect_image_hashes(
                self.app.document.furniture(slot),
                &mut images,
                &media_types,
                self.app,
            );
        }
        images
    }
}

/// Walks blocks (including table cells) collecting the bytes behind every
/// image block that the app still holds.
fn collect_image_hashes(
    blocks: &[opendoc_core::Block],
    images: &mut BTreeMap<String, DocxImage>,
    media_types: &BTreeMap<&str, &str>,
    app: &OpenDocApp,
) {
    for block in blocks {
        match &block.kind {
            opendoc_core::BlockKind::Image { blob_hash, .. } => {
                if images.contains_key(blob_hash) {
                    continue;
                }
                let Some(bytes) = app.blob_bytes.get(blob_hash) else {
                    continue;
                };
                images.insert(
                    blob_hash.clone(),
                    DocxImage {
                        media_type: media_types
                            .get(blob_hash.as_str())
                            .copied()
                            .unwrap_or("application/octet-stream")
                            .to_string(),
                        bytes: bytes.clone(),
                    },
                );
            }
            opendoc_core::BlockKind::Table { rows, .. } => {
                for row in rows {
                    for cell in &row.cells {
                        collect_image_hashes(&cell.blocks, images, media_types, app);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Exporter warnings as the app's own warning DTO. They are the same shape as
/// a `ModelWarning` but they never become one: a `ModelWarning` belongs to a
/// document, and these belong to one export.
fn model_warnings(warnings: Vec<ModelWarning>) -> Vec<AppWarning> {
    warnings.iter().map(AppWarning::from_core).collect()
}
