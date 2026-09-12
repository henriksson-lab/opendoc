use crate::{
    append_spreadsheet_formula_warnings, push_unique_warning, verify_app_typed_signature,
    AppArchiveTombstone, AppBlobRef, AppDocument, AppOperationRecord, AppPageLayout,
    AppPageSizePreset, AppRecentDocument, AppRenderService, AppSignature, AppWarning,
};
use opendoc_core::{Document, HeaderFooterSlot};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Builds UI-facing projections from source state and runtime sidecars.
pub(crate) struct AppProjectionService<'a> {
    document: &'a Document,
    workbook: &'a crate::AppSpreadsheetWorkbook,
    blobs: &'a [AppBlobRef],
    blob_bytes: &'a BTreeMap<String, Vec<u8>>,
    blob_signatures: &'a BTreeMap<String, Vec<opendoc_format::SignatureRecord>>,
    blob_tombstones: &'a BTreeMap<String, AppArchiveTombstone>,
    signatures: &'a [opendoc_format::SignatureRecord],
    operation_journal: &'a [AppOperationRecord],
    is_open: bool,
    saved_projection: &'a Option<AppDocument>,
    repository_root: &'a Option<PathBuf>,
    repository_backend: &'a Option<String>,
    repository_namespace: &'a Option<String>,
    recent_documents: &'a [AppRecentDocument],
    last_manifest: &'a Option<String>,
    saved_operation_count: usize,
    saved_signature_count: usize,
    defer_spreadsheet_evaluation: bool,
}

impl<'a> AppProjectionService<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        document: &'a Document,
        workbook: &'a crate::AppSpreadsheetWorkbook,
        blobs: &'a [AppBlobRef],
        blob_bytes: &'a BTreeMap<String, Vec<u8>>,
        blob_signatures: &'a BTreeMap<String, Vec<opendoc_format::SignatureRecord>>,
        blob_tombstones: &'a BTreeMap<String, AppArchiveTombstone>,
        signatures: &'a [opendoc_format::SignatureRecord],
        operation_journal: &'a [AppOperationRecord],
        is_open: bool,
        saved_projection: &'a Option<AppDocument>,
        repository_root: &'a Option<PathBuf>,
        repository_backend: &'a Option<String>,
        repository_namespace: &'a Option<String>,
        recent_documents: &'a [AppRecentDocument],
        last_manifest: &'a Option<String>,
        saved_operation_count: usize,
        saved_signature_count: usize,
        defer_spreadsheet_evaluation: bool,
    ) -> Self {
        Self {
            document,
            workbook,
            blobs,
            blob_bytes,
            blob_signatures,
            blob_tombstones,
            signatures,
            operation_journal,
            is_open,
            saved_projection,
            repository_root,
            repository_backend,
            repository_namespace,
            recent_documents,
            last_manifest,
            saved_operation_count,
            saved_signature_count,
            defer_spreadsheet_evaluation,
        }
    }

    pub(crate) fn document(&self, operation_limit: usize) -> AppDocument {
        let mut document = self
            .saved_projection
            .clone()
            .unwrap_or_else(|| AppDocument::from_core(self.document));
        document.repository_root = self
            .repository_root
            .as_ref()
            .map(|path| path.to_string_lossy().to_string());
        document.repository_backend = self.repository_backend.clone();
        document.repository_namespace = self.repository_namespace.clone();
        document.recent_documents = self.recent_documents.to_vec();
        document.last_manifest = self.last_manifest.clone();
        document.has_unsaved_changes = self.is_open && self.has_pending_save_changes();
        if document.operations.len() > operation_limit {
            let skip = document.operations.len() - operation_limit;
            document.operations.drain(..skip);
        }
        document.is_open = self.is_open;
        if self.is_open {
            let renderer =
                AppRenderService::new(self.document, self.workbook, self.blobs, self.blob_bytes);
            // The renderer is a pure projection: it returns its warnings
            // rather than writing them into the document, so this is the one
            // place they become visible to the user. Without it an equation
            // renders with an error marker and the warnings panel says
            // nothing about why.
            let body = renderer.render_document();
            let footnotes = renderer.render_footnotes();
            let header = renderer.render_page_furniture(HeaderFooterSlot::Header);
            let footer = renderer.render_page_furniture(HeaderFooterSlot::Footer);
            document.body_html = body.html;
            document.footnotes_html = footnotes.html;
            document.header_html = header.html;
            document.footer_html = footer.html;
            document.page_layout = AppPageLayout {
                size_name: self.document.page_setup.size_name().map(str::to_string),
                orientation: self.document.page_setup.orientation().as_str().to_string(),
                style: renderer.page_setup_css_variables(),
                print_style: renderer.page_setup_print_css(),
                size_presets: opendoc_core::PAGE_SIZE_PRESETS
                    .iter()
                    .map(|preset| AppPageSizePreset {
                        name: preset.name.to_string(),
                        label: preset.label.to_string(),
                        width_twips: preset.width_twips,
                        height_twips: preset.height_twips,
                    })
                    .collect(),
            };
            for warning in body
                .warnings
                .iter()
                .chain(footnotes.warnings.iter())
                .chain(header.warnings.iter())
                .chain(footer.warnings.iter())
            {
                push_unique_warning(
                    &mut document.warnings,
                    &warning.code,
                    warning.message.clone(),
                );
            }
        }
        document.signature_state = self.signature_state_label().to_string();
        document.signatures = self
            .signatures
            .iter()
            .map(AppSignature::from_record)
            .collect();
        document.signature = document.signatures.first().cloned();
        document.workbook = if self.defer_spreadsheet_evaluation {
            self.workbook.clone()
        } else {
            self.workbook.evaluated()
        };
        append_spreadsheet_formula_warnings(&mut document);
        document.blobs = self.projected_blobs();
        for blob in &document.blobs {
            for typed in &blob.typed_signatures {
                if typed.signature_state == "broken" {
                    let message = format!(
                        "blob {} has a broken {} typed signature",
                        blob.name, typed.profile
                    );
                    if !document.warnings.iter().any(|warning| {
                        warning.code == "broken-typed-blob-signature" && warning.message == message
                    }) {
                        document.warnings.push(AppWarning {
                            code: "broken-typed-blob-signature".to_string(),
                            message,
                        });
                    }
                }
            }
        }
        document.operation_count = self.operation_journal.len();
        document.operations = self.operation_journal.to_vec();
        document
    }

    pub(crate) fn projected_blobs(&self) -> Vec<AppBlobRef> {
        self.blobs
            .iter()
            .map(|blob| {
                let mut blob = blob.clone();
                blob.signatures = self
                    .blob_signatures
                    .get(&blob.hash)
                    .into_iter()
                    .flatten()
                    .map(AppSignature::from_record)
                    .collect();
                blob.signature_state = if blob.signatures.is_empty() {
                    "unsigned".to_string()
                } else {
                    "signed".to_string()
                };
                blob.archive_tombstone = self.blob_tombstones.get(&blob.hash).cloned();
                for typed in &mut blob.typed_signatures {
                    typed.signature_state = if let Some(bytes) = self.blob_bytes.get(&blob.hash) {
                        verify_app_typed_signature(typed, &blob.hash, bytes)
                    } else {
                        "untrusted".to_string()
                    };
                }
                blob
            })
            .collect()
    }

    fn signature_count(&self) -> usize {
        self.signatures.len()
            + self
                .blob_signatures
                .values()
                .map(|signatures| signatures.len())
                .sum::<usize>()
    }

    pub(crate) fn has_pending_save_changes(&self) -> bool {
        self.saved_operation_count != self.operation_journal.len()
            || self.saved_signature_count != self.signature_count()
    }

    fn signature_state_label(&self) -> &'static str {
        if !self.signatures.is_empty() {
            "signed"
        } else {
            "unsigned"
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::OpenDocApp;
    use opendoc_core::{Equation, EquationSourceFormat, Footnote, Inline, StableId};

    /// An equation the renderer can parse but only partly understand renders
    /// with an error marker inside it. Before this plumbing existed the user
    /// saw the marker and the warnings panel said nothing, so there was no way
    /// to learn what was wrong.
    #[test]
    fn unknown_equation_command_is_reported_in_the_document_warnings() {
        let mut app = OpenDocApp::new_empty_document();
        app.add_equation_inline(r"a + \notarealcommand{b}")
            .expect("equation source is non-empty");
        let document = app.document();
        assert!(
            document
                .warnings
                .iter()
                .any(|warning| warning.code == "equation-unknown-command"),
            "{:?}",
            document.warnings
        );
    }

    #[test]
    fn unrenderable_equation_is_reported_in_the_document_warnings() {
        let mut app = OpenDocApp::new_empty_document();
        app.add_equation_block(r"\frac{a}{")
            .expect("equation source is non-empty");
        let document = app.document();
        let warning = document
            .warnings
            .iter()
            .find(|warning| warning.code == "equation-render-failed")
            .unwrap_or_else(|| panic!("no render-failure warning in {:?}", document.warnings));
        // The message must name the equation, or a document with several
        // equations cannot be acted on.
        assert!(!warning.message.trim().is_empty());
    }

    /// Footnote bodies hold inline equations too, and they are rendered by a
    /// second call that returns its own warnings.
    #[test]
    fn footnote_equation_warnings_reach_the_document_warnings() {
        let mut app = OpenDocApp::new_empty_document();
        let footnote_id = StableId::new("footnote");
        app.document.blocks[0].content.push(Inline::FootnoteRef {
            id: StableId::new("inline"),
            footnote_id: footnote_id.clone(),
        });
        app.document.footnotes.push(Footnote {
            id: footnote_id,
            revision: 0,
            body: vec![Inline::Equation {
                id: StableId::new("inline"),
                equation: Equation {
                    id: StableId::new("equation"),
                    source_format: EquationSourceFormat::LatexLike,
                    source: r"\frac{a}{".to_string(),
                },
            }],
            deleted: false,
        });
        let document = app.document();
        assert!(
            document
                .warnings
                .iter()
                .any(|warning| warning.code == "equation-render-failed"),
            "{:?}",
            document.warnings
        );
    }

    /// Projection purity: an equation warning reaches the projection on every
    /// call and is never written back into the signed source document. The
    /// obvious wrong implementation is `push_model_warning`, which would put
    /// a *rendering* outcome into state that gets hashed and signed.
    #[test]
    fn equation_warnings_are_projected_never_written_into_source_state() {
        let mut app = OpenDocApp::new_empty_document();
        app.add_equation_inline(r"a + \notarealcommand{b}")
            .expect("equation source is non-empty");
        let first = app.document();
        let second = app.document();
        assert_eq!(first.warnings, second.warnings, "projection is not stable");
        assert!(
            second
                .warnings
                .iter()
                .any(|warning| warning.code == "equation-unknown-command"),
            "{:?}",
            second.warnings
        );
        assert!(
            !app.document
                .warnings
                .iter()
                .any(|warning| warning.code.starts_with("equation-")),
            "renderer warnings must not be written into source state: {:?}",
            app.document.warnings
        );
    }
}
