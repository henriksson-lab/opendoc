//! App-api rendering wrappers. HTML rendering is implemented by
//! `opendoc-render`.

use super::{parse_id, AppApiError, AppRenderService, OpenDocApp};
use opendoc_merge::{preview_suggestion_resolution, SuggestionPreviewResolution};
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

    /// Render a strictly read-only "what if I accept/reject this?" document.
    ///
    /// The merge crate owns the projection because its acceptance code has
    /// context-sensitive structural safeguards.  In particular, a preview
    /// cannot quietly turn a vanished insertion anchor into a body append.
    /// Neither the source document nor its operation journal is changed.
    pub fn render_suggestion_preview_html(
        &self,
        suggestion_id: String,
        resolution: String,
    ) -> Result<String, AppApiError> {
        let suggestion_id = parse_id(&suggestion_id)?;
        let resolution = match resolution.as_str() {
            "accept" => SuggestionPreviewResolution::Accept,
            "reject" => SuggestionPreviewResolution::Reject,
            other => {
                return Err(AppApiError::Format(format!(
                    "unsupported suggestion preview resolution {other}; expected accept or reject"
                )))
            }
        };
        let (document, _warnings) =
            preview_suggestion_resolution(&self.document, &suggestion_id, resolution);
        Ok(
            AppRenderService::new(&document, &self.workbook, &self.blobs, &self.blob_bytes)
                .render_document_html(),
        )
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
    ///
    /// Evaluates the live workbook *in place* first. Every spreadsheet
    /// mutation already leaves `self.workbook` evaluated (see
    /// `SpreadsheetMutationService::mutate`, which evaluates the staged copy
    /// before committing it), so this is normally a no-op recalculation over
    /// values that are already right; what it replaces is a full `clone` of
    /// every sheet per grid render, which `evaluated()` made and then dropped.
    /// It stays unconditional because an imported or freshly opened workbook
    /// is the one case where the axis metadata the grid draws with has not
    /// been filled in yet, and a render is not the place to guess.
    pub fn render_workbook_html(&mut self, sheet_id: &str) -> Result<String, super::AppApiError> {
        self.workbook.evaluate();
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

#[cfg(test)]
mod tests {
    use super::*;
    use opendoc_core::{Block, SuggestionState};

    #[test]
    fn suggestion_preview_renders_a_clone_and_keeps_the_live_proposal_open() {
        let mut app = OpenDocApp::new_sample();
        app.new_document("Preview");
        app.document.blocks.clear();
        app.document.blocks.push(Block::paragraph("before"));
        app.add_suggestion("Alice", "after")
            .expect("create proposal");
        let suggestion_id = app.document.suggestions[0].id.to_string();

        let accepted = app
            .render_suggestion_preview_html(suggestion_id.clone(), "accept".to_string())
            .expect("accepted preview");
        assert!(accepted.contains(">before<"), "{accepted}");
        assert!(accepted.contains(">after<"), "{accepted}");
        assert_eq!(app.document.suggestions[0].state, SuggestionState::Proposed);
        assert_eq!(app.document.visible_text(), "before\n");

        let rejected = app
            .render_suggestion_preview_html(suggestion_id, "reject".to_string())
            .expect("rejected preview");
        assert!(rejected.contains("before"), "{rejected}");
        assert!(!rejected.contains("after"), "{rejected}");
    }

    #[test]
    fn suggestion_preview_rejects_an_unknown_resolution_without_touching_source() {
        let app = OpenDocApp::new_sample();
        let error = app
            .render_suggestion_preview_html("suggestion".to_string(), "maybe".to_string())
            .expect_err("invalid resolution must not be coerced");
        assert!(error.to_string().contains("expected accept or reject"));
    }
}
