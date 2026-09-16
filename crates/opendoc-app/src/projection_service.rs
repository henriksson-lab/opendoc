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
        document.is_open = self.is_open;
        if self.is_open {
            let renderer =
                AppRenderService::new(self.document, self.workbook, self.blobs, self.blob_bytes);
            // The renderer is a pure projection: it returns its warnings
            // rather than writing them into the document, so this is the one
            // place they become visible to the user. Without it an equation
            // renders with an error marker and the warnings panel says
            // nothing about why.
            // One walk of the body, producing the fragments the frontend
            // applies *and* the warnings — not two. `render_document` would
            // render the same markup a second time to reach the same string
            // this one already sliced.
            let body = renderer.render_document_body();
            let footnotes = renderer.render_footnotes();
            let header = renderer.render_page_furniture(HeaderFooterSlot::Header);
            let footer = renderer.render_page_furniture(HeaderFooterSlot::Footer);
            let first_page_header =
                renderer.render_page_furniture(HeaderFooterSlot::FirstPageHeader);
            let first_page_footer =
                renderer.render_page_furniture(HeaderFooterSlot::FirstPageFooter);
            let even_page_header = renderer.render_page_furniture(HeaderFooterSlot::EvenPageHeader);
            let even_page_footer = renderer.render_page_furniture(HeaderFooterSlot::EvenPageFooter);
            document.body_fragments = body
                .fragments
                .into_iter()
                .map(crate::AppBodyFragment::from_render)
                .collect();
            document.footnotes_html = footnotes.html;
            document.header_html = header.html;
            document.footer_html = footer.html;
            document.first_page_header_html = first_page_header.html;
            document.first_page_footer_html = first_page_footer.html;
            document.even_page_header_html = even_page_header.html;
            document.even_page_footer_html = even_page_footer.html;
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
            // What the bundled CSL data cannot serve exactly, told to the
            // person *editing* the document.
            //
            // `citation_support_warnings` was called only from the import and
            // export paths, so a document whose style or locale falls back to
            // the built-in renderer looked completely normal in the editor —
            // the citations and the bibliography were already being drawn by
            // the fallback on screen — and the first word of it came when
            // someone exported. It is a projection of source state like every
            // other warning here: computed on read, never written into the
            // document, so it cannot dirty a signature.
            for warning in
                opendoc_citations::citation_support_warnings(&self.document.citation_database)
            {
                push_unique_warning(&mut document.warnings, &warning.code, warning.message);
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
        // The history panel shows the tail of the journal, so only the tail is
        // projected; `operation_count` above stays the full length, which is
        // what every count in the UI reads.
        //
        // `operation_limit` had been applied earlier in this function, to the
        // operations carried by the cloned saved projection — a list this
        // assignment then replaced outright. So nothing had ever been
        // trimmed, and a long editing session put its whole journal on the
        // wire: 193 KB of the 1.9 MB a 1,500-block document crossed the WASM
        // boundary with on every keystroke.
        let skip = self.operation_journal.len().saturating_sub(operation_limit);
        document.operations = self.operation_journal[skip..].to_vec();
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
mod wire_form_tests {
    use crate::{AppBlock, AppDocument, OpenDocApp};

    /// The projection omits what a block does not have instead of spelling out
    /// its absence, and an omitted field deserialises back to exactly the value
    /// that was skipped.
    ///
    /// This is the whole safety argument for `skip_serializing_if`: the wire
    /// form is smaller, and the value it decodes to is the same one. A field
    /// that gained `skip_serializing_if` without `serde(default)` would fail to
    /// decode here rather than silently losing data at a repository boundary —
    /// `AppDocument` is the `opendoc.app-document.v2` snapshot payload as well
    /// as the command result.
    #[test]
    fn a_plain_paragraph_projects_without_its_absent_fields_and_round_trips() {
        let mut app = OpenDocApp::new_empty_document();
        app.add_paragraph("hello").expect("paragraph");
        let document = app.document();
        // Scoped to the blocks: the workbook DTO has fields of its own that
        // share these names, and it is the per-block payload that is repeated
        // 1,500 times on the wire.
        let json = serde_json::to_string(&document.blocks).expect("serialise");
        for absent in [
            "\"level\"",
            "\"ordered\"",
            "\"list_id\"",
            "\"list_kind\"",
            "\"checked\"",
            "\"properties\"",
            "\"equation_source\"",
            "\"blob_hash\"",
            "\"alt_text\"",
            "\"image_width_twips\"",
            "\"image_height_twips\"",
            "\"image_placement\"",
            "\"image_rotation_degrees\"",
            "\"image_opacity_percent\"",
            "\"image_crop_top_percent\"",
            "\"image_crop_right_percent\"",
            "\"image_crop_bottom_percent\"",
            "\"image_crop_left_percent\"",
            "\"image_caption\"",
            "\"image_border\"",
            "\"rows\"",
            "\"row_ids\"",
            "\"cell_ids\"",
            "\"table\"",
            "\"href\"",
            "\"target_id\"",
            "\"marks\"",
            "\"mark_kinds\"",
        ] {
            assert!(
                !json.contains(absent),
                "a paragraph-only document still writes {absent} out: {json}"
            );
        }
        let decoded: Vec<AppBlock> = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(
            decoded, document.blocks,
            "the wire form is not the same value"
        );
        let whole = serde_json::to_string(&document).expect("serialise");
        let round_tripped: AppDocument = serde_json::from_str(&whole).expect("deserialise");
        assert_eq!(round_tripped, document, "the document does not round trip");
    }

    /// The fields are only *absent when empty*: a block that has them still
    /// projects them. Without this the previous test would pass just as well
    /// against a projection that dropped the data.
    #[test]
    fn a_block_that_has_those_fields_still_projects_them() {
        let mut app = OpenDocApp::new_empty_document();
        let block_id = app.document.blocks[0].id.to_string();
        app.set_block_text_style(&block_id, "list-item", 1, "ordered")
            .expect("list style");
        app.set_block_alignment(&block_id, "center")
            .expect("alignment");
        app.insert_table_after(&block_id).expect("table insert");
        let document = app.document();
        let json = serde_json::to_string(&document.blocks).expect("serialise");
        for present in [
            "\"level\"",
            "\"ordered\"",
            "\"list_id\"",
            "\"list_kind\"",
            "\"properties\"",
            "\"alignment\"",
            "\"rows\"",
            "\"row_ids\"",
            "\"cell_ids\"",
            "\"table\"",
        ] {
            assert!(
                json.contains(present),
                "a list item beside a table no longer projects {present}"
            );
        }
        let decoded: Vec<AppBlock> = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(decoded, document.blocks);
    }

    /// A table block's nested cell blocks go through the same `AppBlock`
    /// serialisation, so the recursion has to decode as well as the top level.
    #[test]
    fn nested_table_cell_blocks_round_trip_through_the_wire_form() {
        let mut app = OpenDocApp::new_empty_document();
        let block_id = app.document.blocks[0].id.to_string();
        app.insert_table_after(&block_id).expect("table insert");
        let table_id = app
            .document
            .blocks
            .iter()
            .find(|block| matches!(block.kind, opendoc_core::BlockKind::Table { .. }))
            .expect("a table block")
            .id
            .to_string();
        app.insert_table_column(&table_id, None)
            .expect("a third column");
        let document = app.document();
        let table = document
            .blocks
            .iter()
            .find(|block| block.kind == "table")
            .expect("a table block");
        let json = serde_json::to_string(table).expect("serialise");
        let decoded: AppBlock = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(decoded, *table);
        assert_eq!(decoded.rows.len(), 2);
        assert_eq!(decoded.rows[0].len(), 3);
    }

    /// Every command on the typing path answers with the whole document
    /// projection, so the size of one block's wire form is multiplied by the
    /// length of the document on every keystroke. At 1,500 blocks it was
    /// 1.9 MB per keystroke — serialised in Rust, copied across the WASM
    /// boundary and parsed again in JavaScript, three costs that are all
    /// linear in this number.
    ///
    /// The budget is deliberately close to what a paragraph actually needs
    /// (an id, a kind, a style, and an inline run with an id and its text):
    /// an always-serialised field added to `AppBlock` or `AppInline` costs
    /// this much per block of every document, and should have to say so.
    #[test]
    fn a_paragraph_costs_about_what_it_says_on_the_wire() {
        const BUDGET_BYTES: usize = 300;
        let mut app = OpenDocApp::new_empty_document();
        let blocks = 300;
        for index in 0..blocks {
            app.add_paragraph(format!(
                "Paragraph number {index} with a little bit of text in it."
            ))
            .expect("paragraph");
        }
        let document = app.document();
        let bytes = serde_json::to_string(&document.blocks)
            .expect("serialise")
            .len();
        let per_block = bytes / document.blocks.len();
        assert!(
            per_block <= BUDGET_BYTES,
            "a plain paragraph now costs {per_block} bytes on the wire, over the \
             {BUDGET_BYTES}-byte budget; {bytes} bytes for {} blocks",
            document.blocks.len()
        );
    }

    /// The history panel shows the tail of the journal, and `operation_count`
    /// reports the whole length. Projecting the whole journal instead put
    /// 193 KB of it on the wire on every keystroke of a long editing session.
    #[test]
    fn the_projection_carries_only_the_tail_of_the_operation_journal() {
        let mut app = OpenDocApp::new_empty_document();
        for index in 0..260 {
            app.add_paragraph(format!("paragraph {index}"))
                .expect("paragraph");
        }
        let document = app.document();
        assert_eq!(
            document.operation_count,
            app.operation_journal.len(),
            "the count has to be the full journal length"
        );
        assert!(
            document.operations.len() < document.operation_count,
            "nothing was trimmed: {} of {}",
            document.operations.len(),
            document.operation_count
        );
        // The tail, not the head: the newest operations are the ones a user is
        // looking for in the history panel.
        let journal = &app.operation_journal;
        assert_eq!(
            document.operations.last().map(|record| record.seq),
            journal.last().map(|record| record.seq),
            "the newest operation was trimmed away"
        );
        let skip = journal.len() - document.operations.len();
        assert_eq!(document.operations.as_slice(), &journal[skip..]);
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
    /// The projection carries the body as fragments, and they are the whole
    /// body.
    ///
    /// This is the property the frontend applies one block at a time on:
    /// concatenating the fragments in order must give exactly what
    /// `render_document_html` gives, or a block the frontend left alone is not
    /// a block whose markup was unchanged. `opendoc-render` pins it at the
    /// renderer; this pins that the *projection* does not disturb it — that it
    /// carries the fragments of the body it would otherwise have rendered,
    /// rather than fragments of something else.
    #[test]
    fn the_projected_body_fragments_are_exactly_the_rendered_body() {
        let mut app = OpenDocApp::new_empty_document();
        app.add_paragraph("first").expect("paragraph");
        app.add_heading("a heading", 2).expect("heading");
        app.add_list_item("one", 0, "bullet").expect("bullet one");
        app.add_list_item("two", 0, "bullet").expect("bullet two");
        app.add_paragraph("after the list").expect("paragraph");
        let document = app.document();
        assert!(
            document.body_fragments.len() > 1,
            "a five-block document projects more than one fragment"
        );
        assert_eq!(
            document.body_html(),
            app.render_document_html(),
            "the fragments do not compose to the rendered body"
        );
        // And they partition the blocks, in order: a fragment per top-level
        // element, the list run counted once, every block covered exactly
        // once.
        let covered: usize = document
            .body_fragments
            .iter()
            .map(|fragment| fragment.blocks)
            .sum();
        assert_eq!(covered, document.blocks.len());
        let mut index = 0;
        for fragment in &document.body_fragments {
            assert_eq!(
                fragment.block_id, document.blocks[index].id,
                "a fragment is keyed on the first block it renders"
            );
            index += fragment.blocks;
        }
    }

    /// The payload does not carry the document's text.
    ///
    /// It used to, as `visible_text`: 83 KB of every keystroke on a
    /// 1,500-block document, with nothing on the frontend reading it. The
    /// counts derived from it stay, because the status line shows them. The
    /// text is still reachable — derived, from the blocks that are on the
    /// wire anyway.
    #[test]
    fn the_wire_form_carries_the_counts_but_not_the_text() {
        let mut app = OpenDocApp::new_empty_document();
        app.add_paragraph("a sentence of five words")
            .expect("paragraph");
        let document = app.document();
        let json = serde_json::to_string(&document).expect("serialise");
        assert!(
            !json.contains("visible_text"),
            "the projection still carries the document text"
        );
        assert!(
            !json.contains("body_html"),
            "the projection carries the body twice"
        );
        assert!(document.visible_text().contains("a sentence of five words"));
        assert_eq!(
            document.word_count,
            document.visible_text().split_whitespace().count(),
            "the stored count and the derived text disagree"
        );
        assert_eq!(
            document.character_count,
            document.visible_text().chars().count()
        );
    }
}

/// Citation degradation reaches the person editing, not only the person
/// exporting.
#[cfg(test)]
mod citation_support_tests {
    use crate::OpenDocApp;

    /// A document with one reference and the styles a user can actually pick.
    fn app_with_a_reference() -> OpenDocApp {
        let mut app = OpenDocApp::new_empty_document();
        app.add_bibliography_reference("Source", vec!["Author".to_string()], None, None, None)
            .expect("a reference");
        app
    }

    /// Choosing a style OpenDoc does not bundle CSL data for is a silent
    /// downgrade on screen: the citations and the bibliography are drawn by
    /// the built-in renderer instead, and they look like citations. The
    /// warning existed and was computed nowhere an editing user could see it.
    #[test]
    fn an_unbundled_citation_style_is_reported_while_editing() {
        let mut app = app_with_a_reference();
        app.set_citation_style("american-chemical-society", "en-US")
            .expect("an unbundled style is accepted");
        let document = app.document();
        let warning = document
            .warnings
            .iter()
            .find(|warning| warning.code == "citation-style-not-bundled")
            .unwrap_or_else(|| panic!("no style warning in {:?}", document.warnings));
        assert!(
            warning.message.contains("american-chemical-society"),
            "{warning:?}"
        );
    }

    /// The same for a locale whose *language* the bundle does not carry.
    #[test]
    fn an_unbundled_citation_locale_is_reported_while_editing() {
        let mut app = app_with_a_reference();
        app.set_citation_style("apa", "ja-JP")
            .expect("an unbundled locale is accepted");
        let document = app.document();
        let warning = document
            .warnings
            .iter()
            .find(|warning| warning.code == "citation-locale-not-bundled")
            .unwrap_or_else(|| panic!("no locale warning in {:?}", document.warnings));
        assert!(warning.message.contains("ja-JP"), "{warning:?}");
    }

    /// The negative control, and the one that matters most: the default
    /// document must not nag.
    ///
    /// `CitationDatabase::default()` used to be `apa-7th`, which
    /// `resolve_style_name` has never resolved — so wiring this into the
    /// projection while that was true would have put a permanent
    /// "your citations were formatted by the built-in renderer" warning on
    /// every document anyone cited anything in. The default is `apa` now, and
    /// this is what keeps it that way.
    #[test]
    fn a_document_on_the_default_style_and_locale_reports_nothing() {
        let app = app_with_a_reference();
        let document = app.document();
        assert!(
            !document
                .warnings
                .iter()
                .any(|warning| warning.code.starts_with("citation-")),
            "the default citation style and locale must be ones the bundle serves: {:?}",
            document.warnings
        );
    }

    /// A document that cites nothing is not affected by which styles exist,
    /// so an unbundled style on an empty database says nothing.
    #[test]
    fn a_document_with_no_citations_reports_nothing_about_its_style() {
        let mut app = OpenDocApp::new_empty_document();
        app.set_citation_style("american-chemical-society", "ja-JP")
            .expect("style is set");
        let document = app.document();
        assert!(
            !document
                .warnings
                .iter()
                .any(|warning| warning.code.starts_with("citation-")),
            "{:?}",
            document.warnings
        );
    }

    /// Projection purity: the warning is recomputed on every read and never
    /// written into the source state a signature covers.
    #[test]
    fn citation_warnings_are_projected_never_written_into_source_state() {
        let mut app = app_with_a_reference();
        app.set_citation_style("american-chemical-society", "en-US")
            .expect("style");
        let first = app.document();
        let second = app.document();
        assert_eq!(first.warnings, second.warnings, "projection is not stable");
        // Both halves, or the absence assertion below passes for a projection
        // that never produced the warning at all.
        assert!(
            second
                .warnings
                .iter()
                .any(|warning| warning.code == "citation-style-not-bundled"),
            "the second read lost the warning: {:?}",
            second.warnings
        );
        assert!(
            !app.document
                .warnings
                .iter()
                .any(|warning| warning.code.starts_with("citation-")),
            "citation support warnings must not be written into source state: {:?}",
            app.document.warnings
        );
    }
}
