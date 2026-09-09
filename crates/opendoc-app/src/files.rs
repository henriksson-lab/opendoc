//! File-shaped commands that do not need a filesystem: importing documents
//! from bytes the frontend already holds (browser uploads, drag and drop).

use super::{base64_decode, AppApiError, AppDocument, OpenDocApp};

impl OpenDocApp {
    /// Import a Word `.docx` package supplied as base64 (browser uploads).
    pub fn import_docx_base64(
        &mut self,
        name: impl AsRef<str>,
        base64: impl AsRef<str>,
    ) -> Result<AppDocument, AppApiError> {
        let bytes = base64_decode(base64.as_ref())
            .ok_or_else(|| AppApiError::Format("invalid base64 payload".to_string()))?;
        let name = name.as_ref();
        let title = std::path::Path::new(name)
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or(name);
        let report = opendoc_import::import_docx_bytes(title, &bytes)
            .map_err(|err| AppApiError::Import(err.to_string()))?;
        self.adopt_import_report(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_base64_and_non_docx_bytes() {
        let mut app = OpenDocApp::new_sample();
        assert!(app.import_docx_base64("x.docx", "!!!").is_err());
        let not_zip = super::super::base64_encode(b"not a zip");
        assert!(app.import_docx_base64("x.docx", &not_zip).is_err());
    }
}
