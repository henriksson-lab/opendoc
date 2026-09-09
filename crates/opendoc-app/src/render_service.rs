use crate::{AppBlobRef, AppSpreadsheetWorkbook};
use opendoc_core::Document;
use opendoc_render::{RenderError, RenderImage};
use std::collections::BTreeMap;

pub(crate) struct AppRenderService<'a> {
    document: &'a Document,
    workbook: &'a AppSpreadsheetWorkbook,
    blobs: &'a [AppBlobRef],
    blob_bytes: &'a BTreeMap<String, Vec<u8>>,
}

impl<'a> AppRenderService<'a> {
    pub(crate) fn new(
        document: &'a Document,
        workbook: &'a AppSpreadsheetWorkbook,
        blobs: &'a [AppBlobRef],
        blob_bytes: &'a BTreeMap<String, Vec<u8>>,
    ) -> Self {
        Self {
            document,
            workbook,
            blobs,
            blob_bytes,
        }
    }

    pub(crate) fn render_document_html(&self) -> String {
        opendoc_render::render_document_html(self.document, self.render_images())
    }

    pub(crate) fn render_footnotes_html(&self) -> String {
        opendoc_render::render_footnotes_html(self.document, self.render_images())
    }

    pub(crate) fn render_workbook_html(&self, sheet_id: &str) -> Result<String, RenderError> {
        opendoc_render::render_workbook_html(&self.workbook.evaluated(), sheet_id)
    }

    fn render_images(&self) -> Vec<RenderImage<'_>> {
        self.blobs
            .iter()
            .filter(|blob| blob.media_type.starts_with("image/"))
            .filter_map(|blob| {
                self.blob_bytes.get(&blob.hash).map(|bytes| RenderImage {
                    hash: blob.hash.as_str(),
                    media_type: blob.media_type.as_str(),
                    bytes: bytes.as_slice(),
                })
            })
            .collect()
    }
}
