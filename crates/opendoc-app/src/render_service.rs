use crate::{AppBlobRef, AppSpreadsheetWorkbook};
use opendoc_core::{Document, HeaderFooterSlot};
use opendoc_render::{RenderError, RenderImage, Rendering};
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

    /// Body markup plus the warnings the projection produced (unrenderable
    /// equations, unknown LaTeX commands, dangling `\\ref` labels). Callers
    /// that show warnings to the user want this one.
    pub(crate) fn render_document(&self) -> Rendering {
        opendoc_render::render_document(self.document, self.render_images())
    }

    /// Footnote markup plus its projection warnings; footnote bodies can hold
    /// inline equations, so they warn for the same reasons the body does.
    pub(crate) fn render_footnotes(&self) -> Rendering {
        opendoc_render::render_footnotes(self.document, self.render_images())
    }

    /// Header or footer markup plus its projection warnings. Rendered once;
    /// repeating it per page belongs to whatever paginates. See ADR 0009.
    pub(crate) fn render_page_furniture(&self, slot: HeaderFooterSlot) -> Rendering {
        opendoc_render::render_page_furniture(self.document, slot, self.render_images())
    }

    /// Page geometry as CSS custom properties.
    pub(crate) fn page_setup_css_variables(&self) -> String {
        opendoc_render::page_setup_css_variables(&self.document.page_setup)
    }

    /// Page geometry as an `@page` rule, for printing.
    pub(crate) fn page_setup_print_css(&self) -> String {
        opendoc_render::page_setup_print_css(&self.document.page_setup)
    }

    pub(crate) fn render_document_html(&self) -> String {
        self.render_document().html
    }

    pub(crate) fn render_footnotes_html(&self) -> String {
        self.render_footnotes().html
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
