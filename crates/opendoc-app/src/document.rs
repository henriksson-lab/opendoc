use super::*;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppDocument {
    pub is_open: bool,
    pub uuid: String,
    pub title: String,
    pub locale: String,
    pub doi: Option<String>,
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
    /// Rendered document body (see `render.rs` for the markup contract).
    #[serde(default)]
    pub body_html: String,
    #[serde(default)]
    pub footnotes_html: String,
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
            body_html: String::new(),
            footnotes_html: String::new(),
        }
    }

    pub(crate) fn to_core(&self) -> Result<Document, AppApiError> {
        Ok(Document {
            uuid: opendoc_core::DocumentUuid::parse(self.uuid.clone())
                .map_err(|err| AppApiError::Model(err.to_string()))?,
            title: self.title.clone(),
            locale: self.locale.clone(),
            doi: self.doi.clone(),
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppBlock {
    pub id: String,
    pub kind: String,
    pub level: Option<u8>,
    pub ordered: Option<bool>,
    pub style_value: String,
    pub equation_source: Option<String>,
    #[serde(default)]
    pub blob_hash: Option<String>,
    #[serde(default)]
    pub alt_text: Option<String>,
    pub content: Vec<AppInline>,
    pub rows: Vec<Vec<Vec<AppBlock>>>,
    #[serde(default)]
    pub row_ids: Vec<String>,
    #[serde(default)]
    pub cell_ids: Vec<Vec<String>>,
}

impl AppBlock {
    fn from_core(block: &Block, citations: &opendoc_core::CitationDatabase) -> Self {
        let (kind, level, ordered, equation_source, blob_hash, alt_text, rows, row_ids, cell_ids) =
            match &block.kind {
                BlockKind::Paragraph => (
                    "paragraph".to_string(),
                    None,
                    None,
                    None,
                    None,
                    None,
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                ),
                BlockKind::Heading { level } => (
                    "heading".to_string(),
                    Some(*level),
                    None,
                    None,
                    None,
                    None,
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                ),
                BlockKind::ListItem { level, ordered, .. } => (
                    "list-item".to_string(),
                    Some(*level),
                    Some(*ordered),
                    None,
                    None,
                    None,
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                ),
                BlockKind::Table { rows } => (
                    "table".to_string(),
                    None,
                    None,
                    None,
                    None,
                    None,
                    rows.iter()
                        .map(|row| {
                            row.cells
                                .iter()
                                .map(|cell| {
                                    cell.blocks
                                        .iter()
                                        .map(|block| AppBlock::from_core(block, citations))
                                        .collect()
                                })
                                .collect()
                        })
                        .collect(),
                    rows.iter().map(|row| row.id.to_string()).collect(),
                    rows.iter()
                        .map(|row| row.cells.iter().map(|cell| cell.id.to_string()).collect())
                        .collect(),
                ),
                BlockKind::EquationBlock { equation } => (
                    "equation-block".to_string(),
                    None,
                    None,
                    Some(equation.source.clone()),
                    None,
                    None,
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                ),
                BlockKind::Image {
                    blob_hash,
                    alt_text,
                } => (
                    "image".to_string(),
                    None,
                    None,
                    None,
                    Some(blob_hash.clone()),
                    Some(alt_text.clone()),
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                ),
                BlockKind::PageBreak => (
                    "page-break".to_string(),
                    None,
                    None,
                    None,
                    None,
                    None,
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                ),
            };
        Self {
            id: block.id.to_string(),
            style_value: block_style_value(&block.kind),
            kind,
            level,
            ordered,
            equation_source,
            blob_hash,
            alt_text,
            content: block
                .content
                .iter()
                .map(|inline| AppInline::from_core(inline, citations))
                .collect(),
            rows,
            row_ids,
            cell_ids,
        }
    }

    fn to_core(&self) -> Result<Block, AppApiError> {
        Ok(Block {
            id: parse_id(&self.id)?,
            kind: match self.kind.as_str() {
                "heading" => BlockKind::Heading {
                    level: self.level.unwrap_or(2),
                },
                "table" => {
                    let mut rows = self
                        .rows
                        .iter()
                        .enumerate()
                        .map(|(row_index, row)| {
                            let mut cells = row
                                .iter()
                                .enumerate()
                                .map(|(cell_index, cell)| {
                                    let mut blocks = cell
                                        .iter()
                                        .map(AppBlock::to_core)
                                        .collect::<Result<Vec<_>, AppApiError>>()?;
                                    if blocks.is_empty() {
                                        blocks.push(Block::paragraph(""));
                                    }
                                    Ok(opendoc_core::TableCell {
                                        id: self
                                            .cell_ids
                                            .get(row_index)
                                            .and_then(|ids| ids.get(cell_index))
                                            .map(|id| parse_id(id))
                                            .transpose()?
                                            .unwrap_or_else(|| StableId::new("cell")),
                                        blocks,
                                        properties: Vec::new(),
                                    })
                                })
                                .collect::<Result<Vec<_>, AppApiError>>()?;
                            if cells.is_empty() {
                                cells.push(opendoc_core::TableCell {
                                    id: StableId::new("cell"),
                                    blocks: vec![Block::paragraph("")],
                                    properties: Vec::new(),
                                });
                            }
                            Ok(opendoc_core::TableRow {
                                id: self
                                    .row_ids
                                    .get(row_index)
                                    .map(|id| parse_id(id))
                                    .transpose()?
                                    .unwrap_or_else(|| StableId::new("row")),
                                cells,
                            })
                        })
                        .collect::<Result<Vec<_>, AppApiError>>()?;
                    if rows.is_empty() {
                        rows.push(opendoc_core::TableRow {
                            id: StableId::new("row"),
                            cells: vec![opendoc_core::TableCell {
                                id: StableId::new("cell"),
                                blocks: vec![Block::paragraph("")],
                                properties: Vec::new(),
                            }],
                        });
                    }
                    BlockKind::Table { rows }
                }
                "page-break" => BlockKind::PageBreak,
                "list-item" => BlockKind::ListItem {
                    list_id: StableId::parse("list-main").expect("static list id"),
                    level: self.level.unwrap_or(0),
                    ordered: self.ordered.unwrap_or(false),
                },
                "equation-block" => {
                    let source = self.equation_source.clone().ok_or_else(|| {
                        AppApiError::Format("block equation source missing".to_string())
                    })?;
                    if source.trim().is_empty() {
                        return Err(AppApiError::Format(
                            "block equation source is empty".to_string(),
                        ));
                    }
                    if source.trim() != source {
                        return Err(AppApiError::Format(
                            "block equation source has surrounding whitespace".to_string(),
                        ));
                    }
                    BlockKind::EquationBlock {
                        equation: Equation {
                            id: StableId::new("eq"),
                            source_format: EquationSourceFormat::LatexLike,
                            source,
                        },
                    }
                }
                "image" => {
                    let blob_hash = self.blob_hash.clone().ok_or_else(|| {
                        AppApiError::Format("image blob hash missing".to_string())
                    })?;
                    opendoc_core::HashRef::parse(&blob_hash)
                        .map_err(|err| AppApiError::Format(err.to_string()))?;
                    BlockKind::Image {
                        blob_hash,
                        alt_text: self.alt_text.clone().unwrap_or_default(),
                    }
                }
                _ => BlockKind::Paragraph,
            },
            content: self
                .content
                .iter()
                .map(AppInline::to_core)
                .collect::<Result<Vec<_>, _>>()?,
            properties: Vec::new(),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppInline {
    pub id: String,
    pub kind: String,
    pub text: String,
    pub href: Option<String>,
    pub target_id: Option<String>,
    pub marks: Vec<String>,
    pub mark_kinds: Vec<String>,
    pub mark_values: BTreeMap<String, String>,
}

impl AppInline {
    fn from_core(inline: &Inline, citations: &opendoc_core::CitationDatabase) -> Self {
        match inline {
            Inline::Text { id, text, marks } => Self::textual(id, "text", text, None, None, marks),
            Inline::Link {
                id,
                text,
                href,
                marks,
            } => Self::textual(id, "link", text, Some(href.clone()), None, marks),
            Inline::Citation {
                id,
                citation_id,
                rendered_cache,
            } => Self {
                id: id.to_string(),
                kind: "citation".to_string(),
                text: rendered_cache
                    .clone()
                    .or_else(|| citations.rendered_citation(citation_id).cloned())
                    .unwrap_or_else(|| format!("[{citation_id}]")),
                href: None,
                target_id: Some(citation_id.to_string()),
                marks: Vec::new(),
                mark_kinds: Vec::new(),
                mark_values: BTreeMap::new(),
            },
            Inline::FootnoteRef { id, footnote_id } => Self {
                id: id.to_string(),
                kind: "footnote-ref".to_string(),
                text: format!("[{footnote_id}]"),
                href: None,
                target_id: Some(footnote_id.to_string()),
                marks: Vec::new(),
                mark_kinds: Vec::new(),
                mark_values: BTreeMap::new(),
            },
            Inline::Mention { id, label } => Self {
                id: id.to_string(),
                kind: "mention".to_string(),
                text: label.clone(),
                href: None,
                target_id: None,
                marks: Vec::new(),
                mark_kinds: Vec::new(),
                mark_values: BTreeMap::new(),
            },
            Inline::Equation { id, equation } => Self {
                id: id.to_string(),
                kind: "equation".to_string(),
                text: equation.source.clone(),
                href: None,
                target_id: Some(equation.id.to_string()),
                marks: Vec::new(),
                mark_kinds: Vec::new(),
                mark_values: BTreeMap::new(),
            },
        }
    }

    fn textual(
        id: &StableId,
        kind: &str,
        text: &str,
        href: Option<String>,
        target_id: Option<String>,
        marks: &[Mark],
    ) -> Self {
        let (mark_kinds, mark_values) = mark_projection(marks);
        Self {
            id: id.to_string(),
            kind: kind.to_string(),
            text: text.to_string(),
            href,
            target_id,
            marks: marks.iter().map(mark_label).collect(),
            mark_kinds,
            mark_values,
        }
    }

    fn to_core(&self) -> Result<Inline, AppApiError> {
        Ok(match self.kind.as_str() {
            "link" => Inline::Link {
                id: parse_id(&self.id)?,
                text: self.text.clone(),
                href: match self.href.as_ref() {
                    Some(href) if href.trim().is_empty() => {
                        return Err(AppApiError::Format("link href missing".to_string()));
                    }
                    Some(href) if href.trim() != href => {
                        return Err(AppApiError::Format(
                            "link href has surrounding whitespace".to_string(),
                        ));
                    }
                    Some(href) => href.clone(),
                    None => return Err(AppApiError::Format("link href missing".to_string())),
                },
                marks: parse_marks(&self.marks)?,
            },
            "citation" => {
                Inline::Citation {
                    id: parse_id(&self.id)?,
                    citation_id: parse_id(self.target_id.as_deref().ok_or_else(|| {
                        AppApiError::Format("citation target missing".to_string())
                    })?)?,
                    rendered_cache: Some(self.text.clone()),
                }
            }
            "footnote-ref" => {
                Inline::FootnoteRef {
                    id: parse_id(&self.id)?,
                    footnote_id: parse_id(self.target_id.as_deref().ok_or_else(|| {
                        AppApiError::Format("footnote target missing".to_string())
                    })?)?,
                }
            }
            "mention" => Inline::Mention {
                id: parse_id(&self.id)?,
                label: if self.text.trim().is_empty() {
                    return Err(AppApiError::Format("mention label is empty".to_string()));
                } else if self.text.trim() != self.text {
                    return Err(AppApiError::Format(
                        "mention label has surrounding whitespace".to_string(),
                    ));
                } else {
                    self.text.clone()
                },
            },
            "equation" => {
                if self.text.trim().is_empty() {
                    return Err(AppApiError::Format(
                        "inline equation source is empty".to_string(),
                    ));
                }
                if self.text.trim() != self.text {
                    return Err(AppApiError::Format(
                        "inline equation source has surrounding whitespace".to_string(),
                    ));
                }
                Inline::Equation {
                    id: parse_id(&self.id)?,
                    equation: Equation {
                        id: self
                            .target_id
                            .as_deref()
                            .map(parse_id)
                            .transpose()?
                            .unwrap_or_else(|| StableId::new("eq")),
                        source_format: EquationSourceFormat::LatexLike,
                        source: self.text.clone(),
                    },
                }
            }
            _ => Inline::Text {
                id: parse_id(&self.id)?,
                text: self.text.clone(),
                marks: parse_marks(&self.marks)?,
            },
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppCitationDatabase {
    pub style: String,
    pub locale: String,
    pub references: Vec<AppBibliographyReference>,
    pub bibliography: Vec<AppBibliographyEntry>,
    pub citations: Vec<AppCitationGroup>,
}

impl AppCitationDatabase {
    fn from_core(database: &opendoc_core::CitationDatabase) -> Self {
        Self {
            style: database.style.clone(),
            locale: database.locale.clone(),
            references: database
                .references
                .iter()
                .map(AppBibliographyReference::from_core)
                .collect(),
            bibliography: render_bibliography(database)
                .into_iter()
                .map(AppBibliographyEntry::from_rendered)
                .collect(),
            citations: database
                .citations
                .iter()
                .map(AppCitationGroup::from_core)
                .collect(),
        }
    }

    fn to_core(&self) -> Result<opendoc_core::CitationDatabase, AppApiError> {
        Ok(opendoc_core::CitationDatabase {
            style: self.style.clone(),
            locale: self.locale.clone(),
            references: self
                .references
                .iter()
                .map(AppBibliographyReference::to_core)
                .collect::<Result<Vec<_>, _>>()?,
            citations: self
                .citations
                .iter()
                .map(AppCitationGroup::to_core)
                .collect::<Result<Vec<_>, _>>()?,
        })
    }

    pub(crate) fn validate_source(&self) -> Result<(), AppApiError> {
        let mut reference_ids = BTreeSet::new();
        let mut live_reference_ids = BTreeSet::new();
        for reference in &self.references {
            reference.to_core()?;
            if !reference_ids.insert(reference.id.clone()) {
                return Err(AppApiError::Format(format!(
                    "duplicate app bibliography reference id {}",
                    reference.id
                )));
            }
            if !reference.deleted {
                live_reference_ids.insert(reference.id.clone());
            }
        }

        let mut citation_ids = BTreeSet::new();
        for citation in &self.citations {
            citation.to_core()?;
            if !citation_ids.insert(citation.id.clone()) {
                return Err(AppApiError::Format(format!(
                    "duplicate app citation group id {}",
                    citation.id
                )));
            }
            if citation.deleted {
                continue;
            }
            for item in &citation.items {
                if !live_reference_ids.contains(&item.reference_id)
                    && citation
                        .rendered_cache
                        .as_deref()
                        .is_some_and(|cache| !cache.trim().is_empty())
                {
                    return Err(AppApiError::Format(format!(
                        "citation group {} has stale rendered cache for missing bibliography reference {}",
                        citation.id, item.reference_id
                    )));
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppBibliographyEntry {
    pub reference_id: String,
    pub text: String,
}

impl AppBibliographyEntry {
    fn from_rendered(entry: opendoc_citations::RenderedBibliographyEntry) -> Self {
        Self {
            reference_id: entry.reference_id,
            text: entry.text,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppBibliographyReference {
    pub id: String,
    pub revision: u64,
    pub format: String,
    pub source: String,
    pub title: String,
    pub authors: Vec<String>,
    pub issued: Option<String>,
    pub doi: Option<String>,
    pub url: Option<String>,
    pub deleted: bool,
}

impl AppBibliographyReference {
    fn from_core(reference: &BibliographyReference) -> Self {
        Self {
            id: reference.id.to_string(),
            revision: reference.revision,
            format: citation_source_format(&reference.source.format),
            source: String::from_utf8_lossy(&reference.source.bytes).to_string(),
            title: reference.summary.title.clone(),
            authors: reference.summary.authors.clone(),
            issued: reference.summary.issued.clone(),
            doi: reference.summary.doi.clone(),
            url: reference.summary.url.clone(),
            deleted: reference.deleted,
        }
    }

    fn to_core(&self) -> Result<BibliographyReference, AppApiError> {
        let reference = BibliographyReference {
            id: parse_id(&self.id)?,
            revision: self.revision,
            source: CitationSource {
                format: citation_source_format_from_label(&self.format),
                bytes: self.source.as_bytes().to_vec(),
            },
            summary: CitationSummary {
                title: self.title.clone(),
                authors: self.authors.clone(),
                issued: self.issued.clone(),
                doi: self.doi.clone(),
                url: self.url.clone(),
            },
            deleted: self.deleted,
        };
        reference
            .validate()
            .map_err(|err| AppApiError::Format(err.to_string()))?;
        Ok(reference)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppCitationGroup {
    pub id: String,
    pub revision: u64,
    pub items: Vec<AppCitationItem>,
    pub placement: String,
    pub footnote_id: Option<String>,
    pub rendered_cache: Option<String>,
    pub deleted: bool,
}

impl AppCitationGroup {
    fn from_core(citation: &CitationGroup) -> Self {
        Self {
            id: citation.id.to_string(),
            revision: citation.revision,
            items: citation
                .items
                .iter()
                .map(app_citation_item_from_core)
                .collect(),
            placement: match citation.placement {
                CitationPlacement::Inline => "inline".to_string(),
                CitationPlacement::Footnote { .. } => "footnote".to_string(),
            },
            footnote_id: match &citation.placement {
                CitationPlacement::Inline => None,
                CitationPlacement::Footnote { footnote_id } => Some(footnote_id.to_string()),
            },
            rendered_cache: citation.rendered_cache.clone(),
            deleted: citation.deleted,
        }
    }

    fn to_core(&self) -> Result<CitationGroup, AppApiError> {
        let placement = match self.placement.as_str() {
            "inline" => CitationPlacement::Inline,
            "footnote" => CitationPlacement::Footnote {
                footnote_id: parse_id(self.footnote_id.as_deref().ok_or_else(|| {
                    AppApiError::Format(format!(
                        "citation group {} has footnote placement without footnote_id",
                        self.id
                    ))
                })?)?,
            },
            other => {
                return Err(AppApiError::Format(format!(
                    "unsupported citation placement {other}"
                )));
            }
        };
        let citation = CitationGroup {
            id: parse_id(&self.id)?,
            revision: self.revision,
            items: self
                .items
                .iter()
                .map(app_citation_item_to_core)
                .collect::<Result<Vec<_>, _>>()?,
            placement,
            rendered_cache: self.rendered_cache.clone(),
            deleted: self.deleted,
        };
        citation
            .validate_payload()
            .map_err(|err| AppApiError::Format(err.to_string()))?;
        Ok(citation)
    }
}

fn app_citation_item_from_core(item: &CitationItem) -> AppCitationItem {
    AppCitationItem {
        reference_id: item.reference_id.to_string(),
        locator: item.locator.clone(),
        label: item.label.clone(),
        prefix: item.prefix.clone(),
        suffix: item.suffix.clone(),
        suppress_author: item.suppress_author,
    }
}

pub(crate) fn app_citation_item_to_core(
    item: &AppCitationItem,
) -> Result<CitationItem, AppApiError> {
    let item = CitationItem {
        reference_id: parse_id(&item.reference_id)?,
        locator: item.locator.clone(),
        label: item.label.clone(),
        prefix: item.prefix.clone(),
        suffix: item.suffix.clone(),
        suppress_author: item.suppress_author,
    };
    item.validate()
        .map_err(|err| AppApiError::Format(err.to_string()))?;
    Ok(item)
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppCommentThread {
    pub id: String,
    pub anchor: String,
    #[serde(default)]
    pub anchor_label: String,
    pub comments: Vec<AppComment>,
    pub deleted: bool,
}

impl AppCommentThread {
    fn from_core(thread: &CommentThread, blocks: &[Block]) -> Self {
        Self {
            id: thread.id.to_string(),
            anchor: anchor_label(&thread.anchor),
            anchor_label: anchor_display_label(&thread.anchor, blocks),
            comments: thread.comments.iter().map(AppComment::from_core).collect(),
            deleted: thread.deleted,
        }
    }

    fn to_core(&self) -> Result<CommentThread, AppApiError> {
        Ok(CommentThread {
            id: parse_id(&self.id)?,
            anchor: parse_anchor_label(&self.anchor, "comment anchor")?,
            comments: self
                .comments
                .iter()
                .map(AppComment::to_core)
                .collect::<Result<Vec<_>, _>>()?,
            deleted: self.deleted,
        })
    }

    pub(crate) fn validate_source(&self) -> Result<(), AppApiError> {
        parse_id(&self.id)?;
        parse_anchor_label(&self.anchor, "comment anchor")?;
        if self.comments.is_empty() {
            return Err(AppApiError::Format(
                "comment thread has no comments".to_string(),
            ));
        }
        let mut comment_ids = BTreeSet::new();
        for comment in &self.comments {
            comment.validate_source()?;
            if !comment_ids.insert(comment.id.clone()) {
                return Err(AppApiError::Format(format!(
                    "duplicate app comment id {}",
                    comment.id
                )));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppComment {
    pub id: String,
    pub author: String,
    pub body: String,
    pub deleted: bool,
}

impl AppComment {
    fn from_core(comment: &Comment) -> Self {
        Self {
            id: comment.id.to_string(),
            author: comment.author.clone(),
            body: comment
                .body
                .iter()
                .map(|inline| match inline {
                    Inline::Text { text, .. } => text.clone(),
                    Inline::Link { text, .. } => text.clone(),
                    _ => String::new(),
                })
                .collect::<Vec<_>>()
                .join(""),
            deleted: comment.deleted,
        }
    }

    fn to_core(&self) -> Result<Comment, AppApiError> {
        Ok(Comment {
            id: parse_id(&self.id)?,
            author: self.author.clone(),
            body: vec![Inline::text(self.body.clone())],
            created_at_ms: 0,
            deleted: self.deleted,
        })
    }

    pub(crate) fn validate_source(&self) -> Result<(), AppApiError> {
        parse_id(&self.id)?;
        if self.author.trim().is_empty() {
            return Err(AppApiError::Format("comment author is empty".to_string()));
        }
        if self.author.trim() != self.author {
            return Err(AppApiError::Format(
                "comment author has surrounding whitespace".to_string(),
            ));
        }
        if self.body.trim().is_empty() {
            return Err(AppApiError::Format("comment body is empty".to_string()));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppSuggestion {
    pub id: String,
    pub author: String,
    pub kind: String,
    pub text: String,
    pub state: String,
    pub anchor: Option<String>,
    #[serde(default)]
    pub anchor_label: Option<String>,
    pub range_start: Option<String>,
    pub range_end: Option<String>,
    pub marks: Vec<String>,
    pub content: Vec<AppInline>,
    pub provenance: Vec<String>,
}

impl AppSuggestion {
    fn from_core(
        suggestion: &Suggestion,
        citations: &opendoc_core::CitationDatabase,
        blocks: &[Block],
    ) -> Self {
        let (anchor, range_start, range_end, marks, content) = match &suggestion.kind {
            SuggestionKind::Insert { anchor, content } => (
                Some(anchor_label(anchor)),
                None,
                None,
                Vec::new(),
                content
                    .iter()
                    .map(|inline| AppInline::from_core(inline, citations))
                    .collect(),
            ),
            SuggestionKind::Delete { range } => (
                None,
                Some(range.start.to_string()),
                Some(range.end.to_string()),
                Vec::new(),
                Vec::new(),
            ),
            SuggestionKind::Format { range, marks } => (
                None,
                Some(range.start.to_string()),
                Some(range.end.to_string()),
                marks.iter().map(mark_label).collect(),
                Vec::new(),
            ),
        };
        let anchor_label = suggestion_anchor_display_label(&suggestion.kind, blocks);
        Self {
            id: suggestion.id.to_string(),
            author: suggestion.author.clone(),
            kind: match &suggestion.kind {
                SuggestionKind::Insert { .. } => "insert".to_string(),
                SuggestionKind::Delete { .. } => "delete".to_string(),
                SuggestionKind::Format { .. } => "format".to_string(),
            },
            text: suggestion_text(&suggestion.kind),
            state: match suggestion.state {
                SuggestionState::Proposed => "proposed".to_string(),
                SuggestionState::Accepted => "accepted".to_string(),
                SuggestionState::Rejected => "rejected".to_string(),
            },
            anchor,
            anchor_label,
            range_start,
            range_end,
            marks,
            content,
            provenance: suggestion.provenance.clone(),
        }
    }

    fn to_core(&self) -> Result<Suggestion, AppApiError> {
        Ok(Suggestion {
            id: parse_id(&self.id)?,
            author: self.author.clone(),
            kind: match self.kind.as_str() {
                "delete" => SuggestionKind::Delete {
                    range: self.to_range()?,
                },
                "format" => SuggestionKind::Format {
                    range: self.to_range()?,
                    marks: parse_marks(&self.marks)?,
                },
                "insert" => SuggestionKind::Insert {
                    anchor: self
                        .anchor
                        .as_deref()
                        .map(|anchor| parse_anchor_label(anchor, "suggestion anchor"))
                        .transpose()?
                        .unwrap_or(Anchor::Document),
                    content: if self.content.is_empty() {
                        vec![Inline::text(self.text.clone())]
                    } else {
                        self.content
                            .iter()
                            .map(AppInline::to_core)
                            .collect::<Result<Vec<_>, _>>()?
                    },
                },
                other => {
                    return Err(AppApiError::Format(format!(
                        "unsupported suggestion kind {other}"
                    )));
                }
            },
            state: match self.state.as_str() {
                "proposed" => SuggestionState::Proposed,
                "accepted" => SuggestionState::Accepted,
                "rejected" => SuggestionState::Rejected,
                other => {
                    return Err(AppApiError::Format(format!(
                        "unsupported suggestion state {other}"
                    )));
                }
            },
            provenance: self.provenance.clone(),
        })
    }

    fn to_range(&self) -> Result<TextRange, AppApiError> {
        Ok(TextRange {
            start: parse_id(self.range_start.as_deref().ok_or_else(|| {
                AppApiError::Format(format!("suggestion {} missing range_start", self.id))
            })?)?,
            end: parse_id(self.range_end.as_deref().ok_or_else(|| {
                AppApiError::Format(format!("suggestion {} missing range_end", self.id))
            })?)?,
        })
    }

    pub(crate) fn validate_source(&self) -> Result<(), AppApiError> {
        parse_id(&self.id)?;
        if self.author.trim().is_empty() {
            return Err(AppApiError::Format(
                "suggestion author is empty".to_string(),
            ));
        }
        if self.author.trim() != self.author {
            return Err(AppApiError::Format(
                "suggestion author has surrounding whitespace".to_string(),
            ));
        }
        for item in &self.provenance {
            if item.trim().is_empty() {
                return Err(AppApiError::Format(
                    "suggestion provenance entry is empty".to_string(),
                ));
            }
            if item.trim() != item {
                return Err(AppApiError::Format(
                    "suggestion provenance entry has surrounding whitespace".to_string(),
                ));
            }
        }
        match self.state.as_str() {
            "proposed" | "accepted" | "rejected" => {}
            other => {
                return Err(AppApiError::Format(format!(
                    "unsupported suggestion state {other}"
                )));
            }
        }
        match self.kind.as_str() {
            "insert" => {
                if self
                    .anchor
                    .as_deref()
                    .is_none_or(|anchor| anchor.trim().is_empty())
                {
                    return Err(AppApiError::Format(format!(
                        "suggestion {} missing anchor",
                        self.id
                    )));
                }
                if let Some(anchor) = &self.anchor {
                    parse_anchor_label(anchor, "suggestion anchor")?;
                }
                if self.content.is_empty() && self.text.trim().is_empty() {
                    return Err(AppApiError::Format(
                        "insert suggestion content is empty".to_string(),
                    ));
                }
                for inline in &self.content {
                    inline.to_core()?;
                }
            }
            "delete" => {
                self.to_range()?;
                if !self.marks.is_empty() || !self.content.is_empty() {
                    return Err(AppApiError::Format(format!(
                        "delete suggestion {} has non-delete payload",
                        self.id
                    )));
                }
            }
            "format" => {
                self.to_range()?;
                if self.marks.is_empty() {
                    return Err(AppApiError::Format(
                        "format suggestion marks are empty".to_string(),
                    ));
                }
                parse_marks(&self.marks)?;
                if !self.content.is_empty() {
                    return Err(AppApiError::Format(format!(
                        "format suggestion {} has insert content",
                        self.id
                    )));
                }
            }
            other => {
                return Err(AppApiError::Format(format!(
                    "unsupported suggestion kind {other}"
                )));
            }
        }
        Ok(())
    }
}

pub(crate) fn inline_id(inline: &Inline) -> &StableId {
    match inline {
        Inline::Text { id, .. }
        | Inline::Link { id, .. }
        | Inline::Citation { id, .. }
        | Inline::FootnoteRef { id, .. }
        | Inline::Mention { id, .. }
        | Inline::Equation { id, .. } => id,
    }
}

pub(crate) fn byte_offset_for_char_offset(text: &str, offset: usize) -> Result<usize, AppApiError> {
    if offset == text.chars().count() {
        return Ok(text.len());
    }
    text.char_indices()
        .map(|(index, _)| index)
        .nth(offset)
        .ok_or_else(|| {
            AppApiError::Format(format!(
                "text split offset {offset} is outside text length {}",
                text.chars().count()
            ))
        })
}

fn mark_label(mark: &Mark) -> String {
    let expand = match mark.expand {
        MarkExpand::None => "none",
        MarkExpand::Start => "start",
        MarkExpand::End => "end",
        MarkExpand::Both => "both",
    };
    let kind = mark_kind_label(&mark.kind);
    match &mark.value {
        Some(value) => format!("{kind}:{value}:{expand}"),
        None => format!("{kind}:{expand}"),
    }
}

fn block_style_value(kind: &BlockKind) -> String {
    match kind {
        BlockKind::Heading { level } => format!("heading:{level}"),
        BlockKind::ListItem { ordered, .. } => format!("list:{ordered}"),
        _ => "paragraph".to_string(),
    }
}

fn mark_projection(marks: &[Mark]) -> (Vec<String>, BTreeMap<String, String>) {
    let mut kinds = Vec::new();
    let mut values = BTreeMap::new();
    for mark in marks {
        let kind = mark_kind_label(&mark.kind).to_string();
        if !kinds.contains(&kind) {
            kinds.push(kind.clone());
        }
        if let Some(value) = &mark.value {
            values.insert(kind, value.clone());
        }
    }
    (kinds, values)
}

fn mark_kind_label(kind: &MarkKind) -> &'static str {
    match kind {
        MarkKind::Bold => "bold",
        MarkKind::Italic => "italic",
        MarkKind::Underline => "underline",
        MarkKind::Strike => "strike",
        MarkKind::Code => "code",
        MarkKind::Superscript => "superscript",
        MarkKind::Subscript => "subscript",
        MarkKind::Color => "color",
        MarkKind::Background => "background",
        MarkKind::Font => "font",
        MarkKind::Size => "size",
        MarkKind::Link => "link",
        MarkKind::Citation => "citation",
    }
}

fn parse_marks(labels: &[String]) -> Result<Vec<Mark>, AppApiError> {
    let mut marks = Vec::new();
    for label in labels {
        if let Some(mark) = parse_mark_label(label)? {
            marks.push(mark);
        }
    }
    Ok(marks)
}

fn parse_mark_label(label: &str) -> Result<Option<Mark>, AppApiError> {
    let mut parts = label.split(':');
    let kind = parts.next().unwrap_or_default();
    let second = parts.next();
    let third = parts.next();
    if parts.next().is_some() {
        return Ok(None);
    }
    let (value, expand) = match (second, third) {
        (Some(expand), None) => (None, expand),
        (Some(value), Some(expand)) => (Some(value.to_string()), expand),
        _ => return Ok(None),
    };
    let kind = parse_mark_kind(kind)?;
    validate_mark_payload(&kind, value.as_deref())?;
    Ok(Some(Mark {
        kind,
        value,
        expand: match expand {
            "none" => MarkExpand::None,
            "start" => MarkExpand::Start,
            "end" => MarkExpand::End,
            "both" => MarkExpand::Both,
            _ => return Ok(None),
        },
    }))
}

pub(crate) fn validate_mark_payload(
    kind: &MarkKind,
    value: Option<&str>,
) -> Result<(), AppApiError> {
    let needs_value = matches!(
        kind,
        MarkKind::Color | MarkKind::Background | MarkKind::Font | MarkKind::Size
    );
    match (value, needs_value) {
        (Some(mark_value), true) if mark_value.trim().is_empty() => {
            Err(AppApiError::Format("mark value is empty".to_string()))
        }
        (None, true) => Err(AppApiError::Format("mark value is missing".to_string())),
        (Some(_), false) => Err(AppApiError::Format("boolean mark has value".to_string())),
        _ => Ok(()),
    }
}

pub(crate) fn validate_mark_removal_payload(
    kind: &MarkKind,
    value: Option<&str>,
) -> Result<(), AppApiError> {
    let supports_value = matches!(
        kind,
        MarkKind::Color | MarkKind::Background | MarkKind::Font | MarkKind::Size
    );
    match (value, supports_value) {
        (Some(mark_value), true) if mark_value.trim().is_empty() => {
            Err(AppApiError::Format("mark value is empty".to_string()))
        }
        (Some(_), false) => Err(AppApiError::Format("boolean mark has value".to_string())),
        _ => Ok(()),
    }
}

pub(crate) fn parse_mark_kind(kind: &str) -> Result<MarkKind, AppApiError> {
    match kind {
        "bold" => Ok(MarkKind::Bold),
        "italic" => Ok(MarkKind::Italic),
        "underline" => Ok(MarkKind::Underline),
        "strike" => Ok(MarkKind::Strike),
        "code" => Ok(MarkKind::Code),
        "superscript" => Ok(MarkKind::Superscript),
        "subscript" => Ok(MarkKind::Subscript),
        "color" => Ok(MarkKind::Color),
        "background" => Ok(MarkKind::Background),
        "font" => Ok(MarkKind::Font),
        "size" => Ok(MarkKind::Size),
        "link" => Ok(MarkKind::Link),
        "citation" => Ok(MarkKind::Citation),
        _ => Err(AppApiError::Format(format!("unsupported mark kind {kind}"))),
    }
}

fn anchor_label(anchor: &Anchor) -> String {
    match anchor {
        Anchor::TextRange(range) => format!("{}..{}", range.start, range.end),
        Anchor::NearestBlock { block_id, .. } => format!("nearest:{block_id}"),
        Anchor::Document => "document".to_string(),
    }
}

fn anchor_display_label(anchor: &Anchor, blocks: &[Block]) -> String {
    match anchor {
        Anchor::TextRange(range) => range_display_label(range, blocks),
        Anchor::NearestBlock { block_id, .. } => find_block_in_blocks(blocks, block_id)
            .map(|block| format!("On: \"{}\"", truncate_label(&block_display_text(block), 60)))
            .unwrap_or_else(|| "On a removed block".to_string()),
        Anchor::Document => "Document".to_string(),
    }
}

fn suggestion_anchor_display_label(kind: &SuggestionKind, blocks: &[Block]) -> Option<String> {
    match kind {
        SuggestionKind::Insert { anchor, .. } => Some(anchor_display_label(anchor, blocks)),
        SuggestionKind::Delete { range } | SuggestionKind::Format { range, .. } => {
            Some(range_display_label(range, blocks))
        }
    }
}

fn range_display_label(range: &TextRange, blocks: &[Block]) -> String {
    let first = find_inline_in_blocks(blocks, &range.start);
    let last = find_inline_in_blocks(blocks, &range.end);
    let Some(first) = first else {
        return "On removed text".to_string();
    };
    let text = if inline_id(first) == &range.end {
        inline_display_text(first)
    } else {
        format!(
            "{} ... {}",
            inline_display_text(first),
            last.map(inline_display_text).unwrap_or_default()
        )
    };
    format!("\"{}\"", truncate_label(&text, 80))
}

fn block_display_text(block: &Block) -> String {
    block
        .content
        .iter()
        .map(inline_display_text)
        .collect::<Vec<_>>()
        .join("")
}

fn inline_display_text(inline: &Inline) -> String {
    match inline {
        Inline::Text { text, .. } | Inline::Link { text, .. } => text.clone(),
        Inline::Citation {
            rendered_cache: Some(text),
            ..
        } => text.clone(),
        Inline::Citation { citation_id, .. } => format!("[{citation_id}]"),
        Inline::FootnoteRef { footnote_id, .. } => format!("[{footnote_id}]"),
        Inline::Mention { label, .. } => label.clone(),
        Inline::Equation { equation, .. } => equation.source.clone(),
    }
}

fn truncate_label(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

fn suggestion_text(kind: &SuggestionKind) -> String {
    match kind {
        SuggestionKind::Insert { content, .. } => content
            .iter()
            .map(inline_display_text)
            .collect::<Vec<_>>()
            .join(""),
        SuggestionKind::Delete { .. } | SuggestionKind::Format { .. } => String::new(),
    }
}

fn citation_source_format(format: &CitationSourceFormat) -> String {
    match format {
        CitationSourceFormat::CitumNative => "citum-native".to_string(),
        CitationSourceFormat::CslJson => "csl-json".to_string(),
        CitationSourceFormat::Bibtex => "bibtex".to_string(),
        CitationSourceFormat::Ris => "ris".to_string(),
        CitationSourceFormat::Unknown(value) => value.clone(),
    }
}

fn citation_source_format_from_label(label: &str) -> CitationSourceFormat {
    match label {
        "citum-native" => CitationSourceFormat::CitumNative,
        "csl-json" => CitationSourceFormat::CslJson,
        "bibtex" => CitationSourceFormat::Bibtex,
        "ris" => CitationSourceFormat::Ris,
        other => CitationSourceFormat::Unknown(other.to_string()),
    }
}

pub(crate) fn refresh_inline_citation_cache(blocks: &mut [Block], citation_id: &StableId) {
    for block in blocks {
        for inline in &mut block.content {
            if let Inline::Citation {
                citation_id: inline_citation_id,
                rendered_cache,
                ..
            } = inline
            {
                if inline_citation_id == citation_id {
                    *rendered_cache = None;
                }
            }
        }
        if let BlockKind::Table { rows } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    refresh_inline_citation_cache(&mut cell.blocks, citation_id);
                }
            }
        }
    }
}

pub(crate) fn parse_id(value: &str) -> Result<StableId, AppApiError> {
    StableId::parse(value.trim().to_string()).map_err(|err| AppApiError::Model(err.to_string()))
}

pub(crate) fn parse_anchor_label(value: &str, label: &str) -> Result<Anchor, AppApiError> {
    if value.trim().is_empty() {
        return Err(AppApiError::Format(format!("{label} is empty")));
    }
    if value.trim() != value {
        return Err(AppApiError::Format(format!(
            "{label} has surrounding whitespace"
        )));
    }
    if value == "document" {
        return Ok(Anchor::Document);
    }
    if let Some(block_id) = value.strip_prefix("nearest:") {
        return StableId::parse(block_id.to_string())
            .map(|block_id| Anchor::NearestBlock {
                block_id,
                warning: "anchor restored to nearest block".to_string(),
            })
            .map_err(|err| AppApiError::Model(err.to_string()));
    }
    if let Some((start, end)) = value.split_once("..") {
        return match (
            StableId::parse(start.to_string()),
            StableId::parse(end.to_string()),
        ) {
            (Ok(start), Ok(end)) => Ok(Anchor::TextRange(TextRange { start, end })),
            (Err(err), _) | (_, Err(err)) => Err(AppApiError::Model(err.to_string())),
        };
    }
    Err(AppApiError::Format(format!(
        "unsupported app anchor label {value}"
    )))
}
