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

    pub fn export_google_docs_json_text(&self) -> Result<String, AppApiError> {
        ImportExportReadService::new(self).export_google_docs_json_text()
    }

    pub fn import_google_sheets_json_text(
        &mut self,
        json_text: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        ImportExportService::new(self).import_google_sheets_json_text(json_text.as_ref())
    }

    pub fn export_google_sheets_json_text(&self) -> Result<String, AppApiError> {
        ImportExportReadService::new(self).export_google_sheets_json_text()
    }
}
