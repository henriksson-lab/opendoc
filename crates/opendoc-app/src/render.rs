//! App-api rendering wrappers. HTML rendering is implemented by
//! `opendoc-render`.

use super::{AppRenderService, OpenDocApp};
use opendoc_render::RenderError;

pub use opendoc_render::escape_html;

impl OpenDocApp {
    /// Render the document body as HTML. See `opendoc-render` for the
    /// markup contract.
    pub fn render_document_html(&self) -> String {
        AppRenderService::new(
            &self.document,
            &self.workbook,
            &self.blobs,
            &self.blob_bytes,
        )
        .render_document_html()
    }

    /// Render footnote bodies numbered in order of first reference.
    pub fn render_footnotes_html(&self) -> String {
        AppRenderService::new(
            &self.document,
            &self.workbook,
            &self.blobs,
            &self.blob_bytes,
        )
        .render_footnotes_html()
    }

    /// Render one workbook sheet as an HTML table.
    pub fn render_workbook_html(&self, sheet_id: &str) -> Result<String, super::AppApiError> {
        AppRenderService::new(
            &self.document,
            &self.workbook,
            &self.blobs,
            &self.blob_bytes,
        )
        .render_workbook_html(sheet_id)
        .map_err(Into::into)
    }
}

impl From<RenderError> for super::AppApiError {
    fn from(error: RenderError) -> Self {
        match error {
            RenderError::NotFound(message) => super::AppApiError::NotFound(message),
        }
    }
}
