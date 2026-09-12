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
    /// Everything about the page that is *derived* rather than stored: the
    /// name of the standard size these dimensions are, the orientation, the
    /// CSS the page box is drawn with, and the sizes the UI offers. Filled in
    /// by the projection service, never by `from_core`, so it never reaches a
    /// snapshot or a signature.
    #[serde(default)]
    pub page_layout: AppPageLayout,
    pub visible_text: String,
    pub word_count: usize,
    pub character_count: usize,
    pub blocks: Vec<AppBlock>,
    pub footnotes: Vec<AppFootnote>,
    pub comments: Vec<AppCommentThread>,
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
    /// Rendered document body (see `render.rs` for the markup contract).
    #[serde(default)]
    pub body_html: String,
    #[serde(default)]
    pub footnotes_html: String,
    /// Rendered header markup, once. Repeating it per page is pagination's
    /// job; see `docs/adr/0009-pagination-and-page-geometry.md`.
    #[serde(default)]
    pub header_html: String,
    /// Rendered footer markup, once. See [`AppDocument::header_html`].
    #[serde(default)]
    pub footer_html: String,
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
    pub(crate) fn from_core(document: &Document) -> Self {
        let visible_text = document.visible_text();
        let word_count = visible_text
            .split_whitespace()
            .filter(|word| !word.is_empty())
            .count();
        let character_count = visible_text.chars().count();
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
            // Projection only: see the field's documentation.
            page_layout: AppPageLayout::default(),
            visible_text,
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
            comments: document
                .comments
                .iter()
                .map(|thread| AppCommentThread::from_core(thread, &document.blocks))
                .collect(),
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
            workbook: AppSpreadsheetWorkbook::sample(),
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
            body_html: String::new(),
            footnotes_html: String::new(),
            header_html: String::new(),
            footer_html: String::new(),
        }
    }

    pub(crate) fn to_core(&self) -> Result<Document, AppApiError> {
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
            comments: self
                .comments
                .iter()
                .map(AppCommentThread::to_core)
                .collect::<Result<Vec<_>, _>>()?,
            suggestions: self
                .suggestions
                .iter()
                .map(AppSuggestion::to_core)
                .collect::<Result<Vec<_>, _>>()?,
            citation_database: self.citations.to_core()?,
            warnings: self.warnings.iter().map(AppWarning::to_core).collect(),
        })
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
