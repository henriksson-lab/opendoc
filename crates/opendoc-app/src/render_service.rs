use crate::{AppBlobRef, AppSpreadsheetWorkbook, OpenDocApp};
use base64::Engine;
use opendoc_core::{Document, HeaderFooterSlot};
use opendoc_render::{BodyRendering, RenderError, RenderImage, Rendering};
use std::collections::BTreeMap;

impl OpenDocApp {
    /// The renderer bound to this app's document, workbook and blobs.
    ///
    /// One constructor rather than the four-argument call repeated at every
    /// site: a new field the renderer needs is then added in one place.
    pub(crate) fn render_service(&self) -> AppRenderService<'_> {
        AppRenderService::new(
            &self.document,
            &self.workbook,
            &self.blobs,
            &self.blob_bytes,
        )
    }
}

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

    /// The body as its ordered top-level fragments, plus the same warnings
    /// [`Self::render_document`] reports.
    ///
    /// One walk produces both the whole string and the pieces, so they cannot
    /// disagree — see `opendoc_render::render_document_body`. This is what the
    /// document projection carries: the frontend applies the body a fragment
    /// at a time and only parses the ones whose markup changed, so shipping
    /// the 389 KB string as well would double the payload for a consumer that
    /// no longer wants it. Whoever does want it whole (HTML export, the PDF
    /// writer) keeps calling [`Self::render_document`].
    pub(crate) fn render_document_body(&self) -> BodyRendering {
        opendoc_render::render_document_body(self.document, self.render_images())
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

    /// The whole document as one standalone HTML file, with its projection
    /// warnings.
    ///
    /// The body markup is the same `render_document` produces, so the export
    /// cannot drift from what the screen shows; the stylesheet around it is
    /// projected from the type scale rather than restated.
    pub(crate) fn render_standalone_html(&self) -> Rendering {
        opendoc_render::render_standalone_html(self.document, self.render_images())
    }

    pub(crate) fn render_document_html(&self) -> String {
        self.render_document().html
    }

    pub(crate) fn render_footnotes_html(&self) -> String {
        self.render_footnotes().html
    }

    /// One sheet as an HTML table, drawn from the workbook as it stands.
    ///
    /// The workbook must already be evaluated — `OpenDocApp::render_workbook_html`
    /// is the one caller and evaluates in place before asking. It used to call
    /// `evaluated()` here, which is `clone` + `evaluate`: a whole extra copy of
    /// every sheet per grid render, thrown away immediately after. `opendoc-render`
    /// is a pure projection that must never force a recalculation (see its crate
    /// docs), so the recalculation belongs to whoever owns the workbook, not here.
    pub(crate) fn render_workbook_html(&self, sheet_id: &str) -> Result<String, RenderError> {
        let mut html = opendoc_render::render_workbook_html(self.workbook, sheet_id)?;
        // `opendoc-render` is deliberately blob-store blind. Its spreadsheet
        // image projection carries a stable hash marker; resolve it here only
        // when this app owns available image bytes (ADR 0045). Missing or
        // non-image blobs remain a harmless blank atomic object rather than a
        // guessed external URL.
        for blob in self
            .blobs
            .iter()
            .filter(|blob| blob.media_type.starts_with("image/"))
        {
            let Some(bytes) = self.blob_bytes.get(&blob.hash) else {
                continue;
            };
            let marker = format!("data-blob-hash=\"{}\"", blob.hash);
            let source = format!(
                "data-blob-hash=\"{}\" src=\"data:{};base64,{}\"",
                blob.hash,
                blob.media_type,
                base64::engine::general_purpose::STANDARD.encode(bytes),
            );
            html = html.replace(&marker, &source);
        }
        Ok(html)
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

#[cfg(test)]
mod tests {
    use crate::{AppRenderService, OpenDocApp};

    /// The rendered `<td>` for one address. Matching the whole grid would also
    /// match a row header — `>6<` is row six as well as the value six.
    fn cell(html: &str, address: &str) -> String {
        let needle = format!("data-address=\"{address}\"");
        let start = html
            .find(&needle)
            .unwrap_or_else(|| panic!("no cell {address} in the grid"));
        let end = html[start..]
            .find("</td>")
            .expect("an unterminated table cell");
        html[start..start + end].to_string()
    }

    /// Rendering a grid shows computed values even when the workbook reached
    /// this point without being evaluated — an import, or a repository opened
    /// from bytes written by something that never recalculated.
    ///
    /// The recalculation is the app's, in place, which is why it is worth a
    /// test: the obvious way to make the render cheap is to drop it, and a
    /// grid full of blank formula cells is what that looks like.
    #[test]
    fn rendering_a_grid_evaluates_a_workbook_that_was_never_evaluated() {
        let mut app = OpenDocApp::new_empty_document();
        // Straight onto the workbook, bypassing the mutation service: that is
        // the one path that does *not* evaluate on the way in.
        app.workbook.set_cell("A1", "2".to_string());
        app.workbook.set_cell("A2", "=A1*3".to_string());
        let sheet_id = app.workbook.sheets[0].id.clone();
        let html = app
            .render_workbook_html(&sheet_id)
            .expect("the first sheet renders");
        assert!(
            cell(&html, "A2").ends_with(">6"),
            "the formula cell rendered without its value: {}",
            cell(&html, "A2")
        );
    }

    /// The renderer itself never recalculates: it draws the workbook it was
    /// handed. `opendoc-render`'s crate contract says so, and the cost of
    /// breaking it was a full clone of every sheet per grid render.
    #[test]
    fn the_render_service_draws_the_workbook_it_was_handed_and_does_not_recalculate() {
        let mut app = OpenDocApp::new_empty_document();
        app.workbook.set_cell("A1", "2".to_string());
        app.workbook.set_cell("A2", "=A1*3".to_string());
        let sheet_id = app.workbook.sheets[0].id.clone();
        let html = AppRenderService::new(&app.document, &app.workbook, &app.blobs, &app.blob_bytes)
            .render_workbook_html(&sheet_id)
            .expect("the first sheet renders");
        assert_eq!(
            cell(&html, "A2"),
            "data-address=\"A2\" data-kind=\"formula\" data-formula=\"=A1*3\">=A1*3",
            "the renderer recalculated the workbook behind the app's back"
        );
    }
}
