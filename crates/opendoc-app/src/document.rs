//! The document projection DTO: metadata, page setup and footnotes.

use super::*;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppDocument {
    pub is_open: bool,
    pub uuid: String,
    pub title: String,
    pub locale: String,
    pub doi: Option<String>,
    /// The sheet the document is laid out on. Source state: it is stored,
    /// signed and merged. Lengths are twips, the unit the model stores.
    #[serde(default)]
    pub page_setup: AppPageSetup,
    /// Blocks repeated at the top of every page. Source state.
    #[serde(default)]
    pub header: Vec<AppBlock>,
    /// Blocks repeated at the bottom of every page. Source state.
    #[serde(default)]
    pub footer: Vec<AppBlock>,
    /// `None` inherits `header`; an empty vector suppresses the first-page
    /// header explicitly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_page_header: Option<Vec<AppBlock>>,
    /// `None` inherits `footer`; an empty vector suppresses the first-page
    /// footer explicitly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_page_footer: Option<Vec<AppBlock>>,
    /// `None` inherits `header`; an empty vector suppresses even-page headers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub even_page_header: Option<Vec<AppBlock>>,
    /// `None` inherits `footer`; an empty vector suppresses even-page footers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub even_page_footer: Option<Vec<AppBlock>>,
    /// List-run numbering settings.  The key is the stable `list_id`, not a
    /// block id: a restart survives inserts which change the run's first
    /// item.  This is source state and therefore deliberately not derived in
    /// the renderer.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub list_properties: BTreeMap<String, opendoc_core::ListProperties>,
    /// Durable named navigation targets. A deleted entry is a source-state
    /// tombstone, not a UI-only absence.
    #[serde(default)]
    pub bookmarks: Vec<opendoc_core::Bookmark>,
    /// Everything about the page that is *derived* rather than stored: the
    /// name of the standard size these dimensions are, the orientation, the
    /// CSS the page box is drawn with, and the sizes the UI offers. Filled in
    /// by the projection service, never by `from_core`, so it never reaches a
    /// snapshot or a signature.
    #[serde(default)]
    pub page_layout: AppPageLayout,
    /// Word and character counts, derived from the document text.
    ///
    /// The text itself is **not** carried: it was 83 KB of every keystroke's
    /// payload with no reader on the frontend and none in Rust outside tests,
    /// and `signing_document_from_snapshot` had to erase it before signing
    /// precisely because it is derived rather than stored. These two numbers
    /// are what the UI actually shows; [`AppDocument::visible_text`] computes
    /// the text on demand for anything that wants it.
    ///
    /// They count `opendoc_core::Document::counted_text` — the prose on the
    /// page — and deliberately not `visible_text`, which is the `.txt`
    /// export's projection and carries stand-ins for content that is not
    /// text. See `opendoc_core::TextScope`.
    pub word_count: usize,
    pub character_count: usize,
    /// Always serialised, empty or not: the frontend recognises a document
    /// result by the presence of this key (`runDispatch` in `shared.ts`), so a
    /// `skip_serializing_if` here would silently stop the UI applying the
    /// projection of an empty document — closing one, for instance.
    pub blocks: Vec<AppBlock>,
    pub footnotes: Vec<AppFootnote>,
    #[serde(default)]
    pub endnote_ids: Vec<String>,
    pub comments: Vec<AppCommentThread>,
    /// Durable, append-only per-comment review provenance.  The desktop UI
    /// does not yet expose an editor for it, but snapshots must carry it so a
    /// save/open cycle cannot erase history written by another client.
    #[serde(default)]
    pub comment_history: Vec<opendoc_core::CommentHistoryEntry>,
    /// Bounded, read-only document-local review activity.
    #[serde(default)]
    pub comment_activity: Vec<opendoc_core::CommentActivityEntry>,
    pub suggestions: Vec<AppSuggestion>,
    pub citations: AppCitationDatabase,
    pub workbook: AppSpreadsheetWorkbook,
    pub blobs: Vec<AppBlobRef>,
    pub warnings: Vec<AppWarning>,
    pub signature_state: String,
    pub signature: Option<AppSignature>,
    pub signatures: Vec<AppSignature>,
    pub repository_root: Option<String>,
    pub repository_backend: Option<String>,
    pub repository_namespace: Option<String>,
    pub recent_documents: Vec<AppRecentDocument>,
    pub last_manifest: Option<String>,
    pub has_unsaved_changes: bool,
    pub operation_count: usize,
    pub operations: Vec<AppOperationRecord>,
    /// Unclean prior sessions the crash journal can replay. Projection only:
    /// always empty in a snapshot record.
    #[serde(default)]
    pub recovery_sessions: Vec<AppRecoverySession>,
    /// The rendered body, as its ordered top-level fragments.
    ///
    /// Not one string: the frontend applies the body a fragment at a time and
    /// re-parses only the ones whose markup changed, which is why this is a
    /// list and why the whole-body string is *not* carried beside it —
    /// carrying both would double the 389 KB the body costs on every
    /// keystroke. The pieces concatenate, in order, to exactly what
    /// `render_document_html` produces (pinned by
    /// `opendoc_render::fragment_tests::fragments_compose_to_the_whole_body`),
    /// so [`AppDocument::body_html`] reassembles it for a caller that wants
    /// it whole.
    #[serde(default)]
    pub body_fragments: Vec<AppBodyFragment>,
    #[serde(default)]
    pub footnotes_html: String,
    /// Rendered header markup, once. Repeating it per page is pagination's
    /// job; see `docs/adr/0009-pagination-and-page-geometry.md`.
    #[serde(default)]
    pub header_html: String,
    /// Rendered footer markup, once. See [`AppDocument::header_html`].
    #[serde(default)]
    pub footer_html: String,
    /// Rendered first-page header override. Empty means either inherit the
    /// ordinary header or intentionally render no first-page header; source
    /// state above distinguishes those cases.
    #[serde(default)]
    pub first_page_header_html: String,
    /// Rendered first-page footer override. See [`Self::first_page_header_html`].
    #[serde(default)]
    pub first_page_footer_html: String,
    /// Rendered even-page header override, once.
    #[serde(default)]
    pub even_page_header_html: String,
    /// Rendered even-page footer override, once.
    #[serde(default)]
    pub even_page_footer_html: String,
}

/// One top-level element of the rendered body, keyed by the first block it
/// renders.
///
/// A fragment is the finest cut the renderer can make: a paragraph, a table,
/// an image — or a whole list run, because a nested item's `</li>` is written
/// after its child list closes and cutting a run per item would produce
/// unbalanced markup. `blocks` is how many blocks the element covers, one for
/// everything but a list run.
///
/// `block_id` is the key a consumer matches its own state on. Block ids are
/// unique, so it is unique across a body, and it is also the **first**
/// `data-block-id` written inside `html` — which is what lets the frontend
/// find the live element a fragment belongs to (pinned by
/// `opendoc_render::fragment_tests::every_fragments_first_block_id_is_its_key`).
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppBodyFragment {
    pub block_id: String,
    pub blocks: usize,
    pub html: String,
}

impl AppBodyFragment {
    pub(crate) fn from_render(fragment: opendoc_render::BodyFragment) -> Self {
        Self {
            block_id: fragment.block_id,
            blocks: fragment.blocks,
            html: fragment.html,
        }
    }
}

/// Page geometry, in twips — the unit `opendoc_core::Length` stores, so the
/// projection cannot drift by rounding.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppPageSetup {
    pub width_twips: i32,
    pub height_twips: i32,
    pub margin_top_twips: i32,
    pub margin_bottom_twips: i32,
    /// Leading edge (left in a left-to-right document).
    pub margin_start_twips: i32,
    /// Trailing edge (right in a left-to-right document).
    pub margin_end_twips: i32,
    pub margin_header_twips: i32,
    pub margin_footer_twips: i32,
    /// Displayed number for the first physical page.
    #[serde(default = "opendoc_core::PageSetup::default_page_number_start")]
    pub page_number_start: u32,
}

impl Default for AppPageSetup {
    fn default() -> Self {
        Self::from_core(&opendoc_core::PageSetup::default())
    }
}

impl AppPageSetup {
    pub(crate) fn from_core(setup: &opendoc_core::PageSetup) -> Self {
        Self {
            width_twips: setup.width.twips(),
            height_twips: setup.height.twips(),
            margin_top_twips: setup.margin_top.twips(),
            margin_bottom_twips: setup.margin_bottom.twips(),
            margin_start_twips: setup.margin_start.twips(),
            margin_end_twips: setup.margin_end.twips(),
            margin_header_twips: setup.margin_header.twips(),
            margin_footer_twips: setup.margin_footer.twips(),
            page_number_start: setup.page_number_start,
        }
    }

    pub(crate) fn to_core(&self) -> Result<opendoc_core::PageSetup, AppApiError> {
        let length = |twips: i32| {
            opendoc_core::Length::from_twips(twips)
                .map_err(|err| AppApiError::Format(err.to_string()))
        };
        Ok(opendoc_core::PageSetup {
            width: length(self.width_twips)?,
            height: length(self.height_twips)?,
            margin_top: length(self.margin_top_twips)?,
            margin_bottom: length(self.margin_bottom_twips)?,
            margin_start: length(self.margin_start_twips)?,
            margin_end: length(self.margin_end_twips)?,
            margin_header: length(self.margin_header_twips)?,
            margin_footer: length(self.margin_footer_twips)?,
            page_number_start: self.page_number_start,
        })
    }
}

/// Everything about the page that is derived from [`AppPageSetup`] rather
/// than stored beside it. Projection only.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppPageLayout {
    /// The standard size these dimensions are, in either orientation, or
    /// `null` for a custom page. Recovered by measuring, so a document that
    /// was never told it is A4 still reports A4.
    #[serde(default)]
    pub size_name: Option<String>,
    /// `"portrait"` or `"landscape"`, derived from the dimensions.
    #[serde(default)]
    pub orientation: String,
    /// The page geometry as CSS custom properties, ready for a `style`
    /// attribute. Generated by `opendoc-render` so the page's shape has one
    /// source and it is the document.
    #[serde(default)]
    pub style: String,
    /// The same geometry as an `@page` rule. Custom properties do not apply
    /// inside `@page`, so the print box needs its own concrete projection.
    #[serde(default)]
    pub print_style: String,
    /// The standard sizes the page-setup dialog offers. Sent from Rust so the
    /// frontend never hard-codes a paper dimension.
    #[serde(default)]
    pub size_presets: Vec<AppPageSizePreset>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppPageSizePreset {
    pub name: String,
    pub label: String,
    pub width_twips: i32,
    pub height_twips: i32,
}

impl AppDocument {
    /// The canonical model projected into the app's source DTO — the
    /// payload an `opendoc.app-document.v2` snapshot stores.
    pub fn from_core(document: &Document) -> Self {
        // The text is measured and dropped: the counts are what the payload
        // carries, not the 83 KB they were counted from.
        //
        // `counted_text`, not `visible_text`. The latter is the `.txt`
        // export's projection, which degrades content that is not text to a
        // textual stand-in — an image to its alt text, a formula to the LaTeX
        // it is written in — and counting stand-ins reported a document whose
        // prose is "one two" as nine words and eighty-eight characters. See
        // `opendoc_core::TextScope`.
        let counted_text = document.counted_text();
        let word_count = counted_text
            .split_whitespace()
            .filter(|word| !word.is_empty())
            .count();
        let character_count = counted_text.chars().count();
        Self {
            is_open: true,
            uuid: document.uuid.to_string(),
            title: document.title.clone(),
            locale: document.locale.clone(),
            doi: document.doi.clone(),
            page_setup: AppPageSetup::from_core(&document.page_setup),
            header: document
                .header
                .iter()
                .map(|block| AppBlock::from_core(block, &document.citation_database))
                .collect(),
            footer: document
                .footer
                .iter()
                .map(|block| AppBlock::from_core(block, &document.citation_database))
                .collect(),
            first_page_header: document.first_page_header.as_ref().map(|blocks| {
                blocks
                    .iter()
                    .map(|block| AppBlock::from_core(block, &document.citation_database))
                    .collect()
            }),
            first_page_footer: document.first_page_footer.as_ref().map(|blocks| {
                blocks
                    .iter()
                    .map(|block| AppBlock::from_core(block, &document.citation_database))
                    .collect()
            }),
            even_page_header: document.even_page_header.as_ref().map(|blocks| {
                blocks
                    .iter()
                    .map(|block| AppBlock::from_core(block, &document.citation_database))
                    .collect()
            }),
            even_page_footer: document.even_page_footer.as_ref().map(|blocks| {
                blocks
                    .iter()
                    .map(|block| AppBlock::from_core(block, &document.citation_database))
                    .collect()
            }),
            list_properties: document
                .list_properties
                .iter()
                .map(|(id, properties)| (id.to_string(), properties.clone()))
                .collect(),
            bookmarks: document.bookmarks.clone(),
            // Projection only: see the field's documentation.
            page_layout: AppPageLayout::default(),
            word_count,
            character_count,
            blocks: document
                .blocks
                .iter()
                .map(|block| AppBlock::from_core(block, &document.citation_database))
                .collect(),
            footnotes: document
                .footnotes
                .iter()
                .map(AppFootnote::from_core)
                .collect(),
            endnote_ids: document
                .endnote_ids
                .iter()
                .map(ToString::to_string)
                .collect(),
            comments: document
                .comments
                .iter()
                .map(|thread| AppCommentThread::from_core(thread, &document.blocks))
                .collect(),
            comment_history: document.comment_history.clone(),
            comment_activity: document.comment_activity.clone(),
            suggestions: document
                .suggestions
                .iter()
                .map(|suggestion| {
                    AppSuggestion::from_core(
                        suggestion,
                        &document.citation_database,
                        &document.blocks,
                    )
                })
                .collect(),
            citations: AppCitationDatabase::from_core(&document.citation_database),
            // A blank workbook, **never** `AppSpreadsheetWorkbook::sample()`.
            //
            // `opendoc_core::Document` has no spreadsheet, so there is nothing
            // here to project one from, and the "Prototype Sheet" demo is a
            // test fixture — `state.rs` says so ("Demo content is a test
            // fixture (`new_sample`), never the state an editor starts in")
            // and puts `#[cfg(test)]` on `new_sample` to make booting into it
            // a compile error. This constructor routed around that: every
            // caller that does not overwrite `workbook` afterwards — the
            // `SERVICE_DOCUMENT_FORMAT` branch of
            // `repository_io::decode_snapshot_object` is one, and
            // `open_projection_from_repository` then adopts what it returns —
            // injected six fabricated cells into the user's document, which
            // the next save committed inside the snapshot a signature covers.
            //
            // The same shape `create_document` starts from, so a document
            // whose source carries no spreadsheet opens with one empty sheet
            // rather than with someone else's data.
            workbook: crate::OpenDocApp::blank_workbook(&document.title),
            blobs: Vec::new(),
            warnings: document
                .warnings
                .iter()
                .map(AppWarning::from_core)
                .collect(),
            signature_state: "unsigned".to_string(),
            signature: None,
            signatures: Vec::new(),
            repository_root: None,
            repository_backend: None,
            repository_namespace: None,
            recent_documents: Vec::new(),
            last_manifest: None,
            has_unsaved_changes: false,
            operation_count: 0,
            operations: Vec::new(),
            recovery_sessions: Vec::new(),
            body_fragments: Vec::new(),
            footnotes_html: String::new(),
            header_html: String::new(),
            footer_html: String::new(),
            first_page_header_html: String::new(),
            first_page_footer_html: String::new(),
            even_page_header_html: String::new(),
            even_page_footer_html: String::new(),
        }
    }

    /// The inverse of [`AppDocument::from_core`].
    pub fn to_core(&self) -> Result<Document, AppApiError> {
        Ok(Document {
            uuid: opendoc_core::DocumentUuid::parse(self.uuid.clone())
                .map_err(|err| AppApiError::Model(err.to_string()))?,
            title: self.title.clone(),
            locale: self.locale.clone(),
            doi: self.doi.clone(),
            page_setup: self.page_setup.to_core()?,
            header: self
                .header
                .iter()
                .map(AppBlock::to_core)
                .collect::<Result<Vec<_>, _>>()?,
            footer: self
                .footer
                .iter()
                .map(AppBlock::to_core)
                .collect::<Result<Vec<_>, _>>()?,
            first_page_header: self
                .first_page_header
                .as_ref()
                .map(|blocks| blocks.iter().map(AppBlock::to_core).collect())
                .transpose()?,
            first_page_footer: self
                .first_page_footer
                .as_ref()
                .map(|blocks| blocks.iter().map(AppBlock::to_core).collect())
                .transpose()?,
            even_page_header: self
                .even_page_header
                .as_ref()
                .map(|blocks| blocks.iter().map(AppBlock::to_core).collect())
                .transpose()?,
            even_page_footer: self
                .even_page_footer
                .as_ref()
                .map(|blocks| blocks.iter().map(AppBlock::to_core).collect())
                .transpose()?,
            list_properties: self
                .list_properties
                .iter()
                .map(|(id, properties)| {
                    Ok((
                        opendoc_core::StableId::parse(id.clone())
                            .map_err(|err| AppApiError::Model(err.to_string()))?,
                        properties.clone(),
                    ))
                })
                .collect::<Result<_, AppApiError>>()?,
            bookmarks: self.bookmarks.clone(),
            blocks: self
                .blocks
                .iter()
                .map(AppBlock::to_core)
                .collect::<Result<Vec<_>, _>>()?,
            footnotes: self
                .footnotes
                .iter()
                .map(AppFootnote::to_core)
                .collect::<Result<Vec<_>, _>>()?,
            endnote_ids: self
                .endnote_ids
                .iter()
                .map(|id| parse_id(id))
                .collect::<Result<_, _>>()?,
            comments: self
                .comments
                .iter()
                .map(AppCommentThread::to_core)
                .collect::<Result<Vec<_>, _>>()?,
            comment_history: self.comment_history.clone(),
            comment_activity: self.comment_activity.clone(),
            suggestions: self
                .suggestions
                .iter()
                .map(AppSuggestion::to_core)
                .collect::<Result<Vec<_>, _>>()?,
            citation_database: self.citations.to_core()?,
            warnings: self.warnings.iter().map(AppWarning::to_core).collect(),
        })
    }

    /// This projection reduced to the **source state** it carries: everything
    /// derived from that source, or belonging to the session rather than to
    /// the document, set back to its zero value.
    ///
    /// This is what a signature is computed over
    /// (`repository_io::signing_document_from_snapshot`), and the reason it
    /// exists as one exhaustive function rather than a handful of statements
    /// at the signing site is that the previous spelling missed fields.
    /// `word_count` and `character_count` were inside the signing payload, so
    /// **changing how the document's text is counted moved the bytes every
    /// existing signature covers** — and `Cell::display_value` /
    /// `Cell::spill_source` put a recalculation inside it too. ADR 0003 is
    /// explicit that a signature covers source state: "Formula signatures
    /// cover formula source, not cached computed values", "Formula computed
    /// values are not stored durably".
    ///
    /// Every field of `AppDocument` is named here, in declaration order, and
    /// each one is either kept with a reason or cleared with a reason. A field
    /// added later has to be added here too — there is no catch-all — and the
    /// test `every_projection_only_field_is_outside_the_signing_payload` fails
    /// if a derived value survives.
    pub(crate) fn into_source_state(mut self) -> Self {
        // Session state, not document state: whether a document is open, and
        // which repository this process opened it from.
        self.is_open = false;
        self.repository_root = None;
        self.repository_backend = None;
        self.repository_namespace = None;
        self.recent_documents = Vec::new();
        self.last_manifest = None;
        self.has_unsaved_changes = false;
        self.recovery_sessions = Vec::new();

        // Kept: uuid, title, locale, doi, page_setup, header, footer, blocks,
        // footnotes, comments, suggestions — the model's own fields.

        // Derived from `page_setup` by the projection service.
        self.page_layout = AppPageLayout::default();
        // Derived from the blocks. These two are why this function exists.
        self.word_count = 0;
        self.character_count = 0;
        // Derived from merge/open/render; a warning is a report about the
        // state, not part of it.
        self.warnings = Vec::new();
        // Rendered citation text is a projection of the citation database.
        clear_citation_projection_payload(&mut self.blocks);
        clear_citation_projection_payload(&mut self.header);
        clear_citation_projection_payload(&mut self.footer);
        if let Some(header) = &mut self.first_page_header {
            clear_citation_projection_payload(header);
        }
        if let Some(footer) = &mut self.first_page_footer {
            clear_citation_projection_payload(footer);
        }
        if let Some(header) = &mut self.even_page_header {
            clear_citation_projection_payload(header);
        }
        if let Some(footer) = &mut self.even_page_footer {
            clear_citation_projection_payload(footer);
        }
        for citation in &mut self.citations.citations {
            citation.rendered_cache = None;
        }
        // Recalculation output. The formulas stay; what they evaluated to on
        // this machine, in this session, does not.
        self.workbook.dependency_graph.clear();
        for sheet in &mut self.workbook.sheets {
            sheet.row_axes.clear();
            sheet.column_axes.clear();
            for cell in &mut sheet.cells {
                cell.computed_kind.clear();
                cell.computed_value.clear();
                cell.display_value.clear();
                cell.dependencies.clear();
                cell.spill_source = None;
            }
        }
        // Blob *refs* are source state; everything the open path learns about
        // them from the store is not. `available` is set by
        // `verify_manifest_blobs` from whether the bytes are present, so
        // leaving it in would mean a blob going missing broke the signature.
        for blob in &mut self.blobs {
            blob.available = true;
            blob.signature_state = "unsigned".to_string();
            blob.signatures = Vec::new();
            blob.archive_tombstone = None;
            for typed in &mut blob.typed_signatures {
                // Not "unsigned": `AppTypedContentSignature::validate_source`
                // refuses that state for a record that carries signature
                // bytes, and the bytes are source state.
                typed.signature_state = "untrusted".to_string();
            }
        }
        // A document cannot sign its own signatures.
        self.signature_state = "unsigned".to_string();
        self.signature = None;
        self.signatures = Vec::new();
        // The operation log is committed as segments beside the snapshot, not
        // inside it.
        self.operation_count = 0;
        self.operations = Vec::new();
        // Rendered HTML.
        self.body_fragments = Vec::new();
        self.footnotes_html = String::new();
        self.header_html = String::new();
        self.footer_html = String::new();
        self.first_page_header_html = String::new();
        self.first_page_footer_html = String::new();
        self.even_page_header_html = String::new();
        self.even_page_footer_html = String::new();
        self
    }

    /// The document's text, derived.
    ///
    /// This used to be a field. It was 83 KB of the payload on every
    /// keystroke, nothing on the frontend read it, nothing in Rust read it
    /// outside tests, and the signing path had to *clear* it before hashing
    /// because a derived value has no business inside a signature. It is a
    /// function of the blocks, so it is computed from them, through the same
    /// `opendoc_core::Document::visible_text` that produced the field.
    ///
    /// This is the plain-text *export* projection, not the one
    /// [`Self::word_count`] counts: an image contributes its alt text here
    /// and an equation its LaTeX source, which is what a `.txt` file wants
    /// and what a word count must not see. `opendoc_core::TextScope` names
    /// the two questions and one walk answers both, so they can disagree only
    /// where they are meant to.
    ///
    /// A projection that cannot be turned back into a model has no text to
    /// report; [`Self::validate_source`] is what rejects one, and it runs at
    /// every boundary a projection arrives from.
    pub fn visible_text(&self) -> String {
        self.to_core()
            .map(|document| document.visible_text())
            .unwrap_or_default()
    }

    /// The whole rendered body as one string, reassembled from
    /// [`Self::body_fragments`].
    ///
    /// The fragments concatenate to exactly what `render_document_html`
    /// produces — that is the property the per-fragment projection is built
    /// on and `opendoc-render` pins it byte for byte — so this is the body,
    /// not an approximation of it. It is *not* carried on the wire: the
    /// frontend wants the pieces, and sending both doubled the largest item
    /// in the payload.
    pub fn body_html(&self) -> String {
        self.body_fragments
            .iter()
            .map(|fragment| fragment.html.as_str())
            .collect()
    }

    pub(crate) fn validate_source(&self) -> Result<(), AppApiError> {
        let document = self.to_core()?;
        document
            .validate()
            .map_err(|err| AppApiError::Model(err.to_string()))?;
        let mut comment_thread_ids = BTreeSet::new();
        for thread in &self.comments {
            thread.validate_source()?;
            if !comment_thread_ids.insert(thread.id.clone()) {
                return Err(AppApiError::Format(format!(
                    "duplicate app comment thread id {}",
                    thread.id
                )));
            }
        }
        let mut suggestion_ids = BTreeSet::new();
        for suggestion in &self.suggestions {
            suggestion.validate_source()?;
            if !suggestion_ids.insert(suggestion.id.clone()) {
                return Err(AppApiError::Format(format!(
                    "duplicate app suggestion id {}",
                    suggestion.id
                )));
            }
        }
        self.citations.validate_source()?;
        self.workbook.validate_source()?;
        for warning in &self.warnings {
            warning.validate_source()?;
        }
        validate_signature_state(&self.signature_state)?;
        if let Some(signature) = &self.signature {
            signature.validate_source()?;
            let Some(first_signature) = self.signatures.first() else {
                return Err(AppApiError::Format(
                    "current document signature is set without document signatures".to_string(),
                ));
            };
            if signature != first_signature {
                return Err(AppApiError::Format(
                    "current document signature does not match first document signature"
                        .to_string(),
                ));
            }
        } else if !self.signatures.is_empty() {
            return Err(AppApiError::Format(
                "current document signature is missing".to_string(),
            ));
        }
        if self.signatures.is_empty() && self.signature_state != "unsigned" {
            return Err(AppApiError::Format(format!(
                "document signature_state {} requires document signatures",
                self.signature_state
            )));
        }
        if !self.signatures.is_empty() && self.signature_state == "unsigned" {
            return Err(AppApiError::Format(
                "document signature_state unsigned cannot have document signatures".to_string(),
            ));
        }
        let mut document_signature_keys = BTreeSet::new();
        for signature in &self.signatures {
            signature.validate_source()?;
            let key = (signature.target.clone(), signature.signer.clone());
            if !document_signature_keys.insert(key) {
                return Err(AppApiError::Format(format!(
                    "duplicate document signature for {} by {}",
                    signature.target, signature.signer
                )));
            }
        }
        let mut blob_ids = BTreeSet::new();
        let mut blob_hashes = BTreeSet::new();
        for blob in &self.blobs {
            blob.validate_source()?;
            if !blob_ids.insert(blob.id.clone()) {
                return Err(AppApiError::Format(format!(
                    "duplicate blob id {}",
                    blob.id
                )));
            }
            if !blob_hashes.insert(blob.hash.clone()) {
                return Err(AppApiError::Format(format!(
                    "duplicate blob hash {}",
                    blob.hash
                )));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppFootnote {
    pub id: String,
    pub revision: u64,
    pub body: Vec<AppInline>,
    pub deleted: bool,
}

impl AppFootnote {
    fn from_core(footnote: &Footnote) -> Self {
        Self {
            id: footnote.id.to_string(),
            revision: footnote.revision,
            body: footnote
                .body
                .iter()
                .map(|inline| {
                    AppInline::from_core(inline, &opendoc_core::CitationDatabase::default())
                })
                .collect(),
            deleted: footnote.deleted,
        }
    }

    fn to_core(&self) -> Result<Footnote, AppApiError> {
        Ok(Footnote {
            id: parse_id(&self.id)?,
            revision: self.revision,
            body: self
                .body
                .iter()
                .map(AppInline::to_core)
                .collect::<Result<Vec<_>, _>>()?,
            deleted: self.deleted,
        })
    }
}
#[cfg(test)]
mod tests {
    use crate::{AppDocument, OpenDocApp, APP_DOCUMENT_FORMAT};

    #[test]
    fn comment_creation_times_survive_the_app_projection_round_trip() {
        let mut app = OpenDocApp::new_empty_document();
        app.add_comment("Reviewer", "Imported comment")
            .expect("comment");
        let comment = &mut app.document.comments[0].comments[0];
        // Native comment formats carry their source creation instant.  Use a
        // value unlike the local logical sequence to prove this is projected,
        // rather than regenerated on the way back to core state.
        comment.created_at_ms = 1_706_155_200_123;

        let projected = AppDocument::from_core(&app.document);
        assert_eq!(
            projected.comments[0].comments[0].created_at_ms,
            1_706_155_200_123
        );
        let restored = projected.to_core().expect("projection is source state");
        assert_eq!(
            restored.comments[0].comments[0].created_at_ms,
            1_706_155_200_123
        );
    }

    /// The snapshot payload changed shape, and the format string it declares
    /// itself as changed with it.
    ///
    /// `v0` named a payload with the document's text in a `visible_text`
    /// field and the rendered body in one `body_html` string. Both are gone.
    /// The snapshot is what a signature is computed over, so a `v0` record
    /// read as if it were this shape would report every signature it holds as
    /// broken and say nothing about why; `is_readable_snapshot_format`
    /// compares against this constant, so the bump is what refuses one by
    /// name. The two halves are asserted together on purpose: the shape and
    /// the name have to change at the same time or one of them is a lie.
    #[test]
    fn the_snapshot_payload_has_no_document_text_and_says_so_in_its_format() {
        let mut app = OpenDocApp::new_empty_document();
        app.add_paragraph("the text of the document")
            .expect("paragraph");
        let snapshot = AppDocument::from_core(&app.document);
        let json = serde_json::to_string(&snapshot).expect("serialise");
        assert!(
            !json.contains("visible_text"),
            "the snapshot payload still carries the document text"
        );
        assert!(
            !json.contains("body_html"),
            "the snapshot payload still carries the body as one string"
        );
        assert_ne!(
            APP_DOCUMENT_FORMAT, "opendoc.app-document.v0",
            "the payload no longer has v0's shape, so it must not claim v0"
        );
        assert_eq!(APP_DOCUMENT_FORMAT, "opendoc.app-document.v2");
    }

    /// The text is derived from the projection's own blocks, through the same
    /// `opendoc_core::Document::visible_text` the field was filled from — so
    /// the counts the payload carries and the text this returns cannot drift.
    ///
    /// Checked over a document with a table, a footnote reference and a list,
    /// because those are where a hand-written second walk over `AppBlock`
    /// would have disagreed.
    #[test]
    fn the_derived_text_is_the_models_own() {
        let mut app = OpenDocApp::new_empty_document();
        app.add_paragraph("a paragraph").expect("paragraph");
        app.add_heading("a heading", 1).expect("heading");
        app.add_list_item("an item", 0, "bullet").expect("item");
        let last = app.document.blocks.last().expect("a block").id.to_string();
        app.insert_table_after(&last).expect("table");
        let projected = app.document();
        assert_eq!(
            projected.visible_text(),
            app.document.visible_text(),
            "the projection's text is not the model's"
        );
        assert!(projected.visible_text().contains("a heading"));
    }

    /// The body reassembles from its fragments byte for byte. `opendoc-render`
    /// pins the composition at the renderer; this pins that carrying the
    /// pieces across the projection boundary preserved it.
    #[test]
    fn the_body_reassembles_from_its_fragments() {
        let mut app = OpenDocApp::new_empty_document();
        app.add_paragraph("one").expect("paragraph");
        app.add_list_item("first", 0, "ordered").expect("first");
        app.add_list_item("second", 1, "ordered").expect("second");
        app.add_paragraph("two").expect("paragraph");
        let projected = app.document();
        assert_eq!(projected.body_html(), app.render_document_html());
        assert!(
            projected.body_html().contains("<ol"),
            "{}",
            projected.body_html()
        );
    }
}

/// What the word and character counts count, and what they do not.
///
/// The counts used to be taken from `Document::visible_text`, which is the
/// `.txt` export's projection: it substitutes a textual stand-in for content
/// that is not text — an image's alt text, an equation's LaTeX source — and it
/// used to substitute a footnote reference's `StableId` as well. None of that
/// is on the page, so all of it was wrong in a count, and a document whose
/// prose is "one two" reported nine words.
///
/// The two questions are now two `opendoc_core::TextScope`s answered by one
/// walk, so these tests pin both halves together: the count must not see the
/// stand-ins, and the export must still get them.
#[cfg(test)]
mod text_scope_tests {
    use crate::{AppDocument, OpenDocApp};
    use opendoc_core::{
        Block, BlockKind, BlockProperties, Equation, EquationSourceFormat, Footnote, Inline,
        StableId,
    };

    /// One paragraph of two words, plus every kind of non-text content that
    /// used to be counted as prose.
    fn document_with_two_words_of_prose() -> opendoc_core::Document {
        let mut app = OpenDocApp::new_empty_document();
        app.document.blocks.clear();
        let footnote_id = StableId::new("footnote");
        app.document.footnotes.push(Footnote {
            id: footnote_id.clone(),
            revision: 1,
            body: vec![Inline::text("the body of the footnote")],
            deleted: false,
        });
        app.document.blocks.push(Block {
            id: StableId::new("block"),
            kind: BlockKind::Paragraph,
            content: vec![
                Inline::text("one two"),
                Inline::Equation {
                    id: StableId::new("equation"),
                    equation: Equation {
                        id: StableId::new("equation"),
                        source_format: EquationSourceFormat::LatexLike,
                        source: "E = mc^2".to_string(),
                    },
                },
                Inline::FootnoteRef {
                    id: StableId::new("footnote-ref"),
                    footnote_id,
                },
            ],
            properties: BlockProperties::default(),
        });
        app.document.blocks.push(Block {
            id: StableId::new("block"),
            kind: BlockKind::EquationBlock {
                equation: Equation {
                    id: StableId::new("equation"),
                    source_format: EquationSourceFormat::LatexLike,
                    source: "\\frac{a}{b}".to_string(),
                },
            },
            content: Vec::new(),
            properties: BlockProperties::default(),
        });
        app.document.blocks.push(Block {
            id: StableId::new("block"),
            kind: BlockKind::Image {
                blob_hash: "sha256:deadbeef".to_string(),
                alt_text: "a photograph of a cat".to_string(),
                layout: Default::default(),
            },
            content: Vec::new(),
            properties: BlockProperties::default(),
        });
        app.document.validate().expect("a valid document");
        app.document
    }

    #[test]
    fn the_counts_see_the_prose_and_nothing_else() {
        let document = document_with_two_words_of_prose();
        let projected = AppDocument::from_core(&document);
        assert_eq!(
            projected.word_count,
            2,
            "counted: {:?}",
            document.counted_text()
        );
        // "one two" and the newline the block contributes. Nothing else on
        // the page has any text.
        assert_eq!(
            projected.character_count,
            8,
            "counted: {:?}",
            document.counted_text()
        );
        let counted = document.counted_text();
        for leaked in ["mc^2", "frac", "photograph", "footnote-"] {
            assert!(
                !counted.contains(leaked),
                "{leaked:?} is not on the page but is in {counted:?}"
            );
        }
    }

    /// The other half. A `.txt` export still degrades non-text content to its
    /// stand-in, because a file with a hole where the figure was is worse than
    /// one that says what was there — and `render_plain_text` names each loss
    /// as an ADR 0010 warning besides.
    #[test]
    fn the_plain_text_export_still_gets_the_stand_ins() {
        let document = document_with_two_words_of_prose();
        let exported = document.visible_text();
        assert!(exported.contains("one two"), "{exported:?}");
        assert!(exported.contains("E = mc^2"), "{exported:?}");
        assert!(exported.contains("\\frac{a}{b}"), "{exported:?}");
        assert!(exported.contains("a photograph of a cat"), "{exported:?}");
    }

    /// A footnote reference is the one thing that is wrong in *both* scopes: a
    /// `StableId` is neither on the page nor a stand-in a text file wants.
    #[test]
    fn a_footnote_reference_leaks_its_identifier_into_neither_projection() {
        let document = document_with_two_words_of_prose();
        let id = document.footnotes[0].id.to_string();
        assert!(!document.counted_text().contains(&id));
        assert!(!document.visible_text().contains(&id));
    }
}
