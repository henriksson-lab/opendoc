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
        self.app.workbook = AppSpreadsheetWorkbook::sample();
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

    pub(crate) fn export_google_docs_json_text(&self) -> Result<String, AppApiError> {
        let bytes = export_google_docs_json(&self.app.document)
            .map_err(|err| AppApiError::Import(err.to_string()))?;
        export_google_docs_json_with_opendoc_blobs(bytes, self.app.projected_blobs())
    }

    pub(crate) fn export_google_sheets_json_text(&self) -> Result<String, AppApiError> {
        Ok(export_google_sheets_workbook(
            &self.app.workbook.evaluated(),
        )?)
    }
}
