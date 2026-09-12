//! The document-local citation database, groups and rendered summaries.

use crate::ids::validate_stable_id;
use crate::ids::StableId;
use crate::warning::ModelError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CitationDatabase {
    pub style: String,
    pub locale: String,
    pub references: Vec<BibliographyReference>,
    pub citations: Vec<CitationGroup>,
}

impl Default for CitationDatabase {
    fn default() -> Self {
        Self {
            style: "apa-7th".to_string(),
            locale: "en-US".to_string(),
            references: Vec::new(),
            citations: Vec::new(),
        }
    }
}

impl CitationDatabase {
    pub fn validate(&self, live_footnotes: &BTreeSet<StableId>) -> Result<(), ModelError> {
        if self.style.trim().is_empty() {
            return Err(ModelError::InvalidDocument("citation style is empty"));
        }
        if self.style.trim() != self.style {
            return Err(ModelError::InvalidDocument(
                "citation style has surrounding whitespace",
            ));
        }
        if self.locale.trim().is_empty() {
            return Err(ModelError::InvalidDocument("citation locale is empty"));
        }
        if self.locale.trim() != self.locale {
            return Err(ModelError::InvalidDocument(
                "citation locale has surrounding whitespace",
            ));
        }

        let mut reference_ids = BTreeSet::new();
        for reference in &self.references {
            reference.validate()?;
            if !reference_ids.insert(reference.id.clone()) {
                return Err(ModelError::InvalidDocument(
                    "duplicate bibliography reference id",
                ));
            }
        }

        let mut citation_ids = BTreeSet::new();
        for citation in &self.citations {
            citation.validate(live_footnotes)?;
            if !citation_ids.insert(citation.id.clone()) {
                return Err(ModelError::InvalidDocument("duplicate citation group id"));
            }
        }

        Ok(())
    }

    pub fn upsert_reference(&mut self, reference: BibliographyReference) {
        if let Some(existing) = self
            .references
            .iter_mut()
            .find(|item| item.id == reference.id)
        {
            if reference.revision >= existing.revision {
                *existing = reference;
            }
        } else {
            self.references.push(reference);
            self.references
                .sort_by(|left, right| left.id.cmp(&right.id));
        }
    }

    pub fn upsert_citation(&mut self, citation: CitationGroup) {
        if let Some(existing) = self
            .citations
            .iter_mut()
            .find(|item| item.id == citation.id)
        {
            if citation.revision >= existing.revision {
                *existing = citation;
            }
        } else {
            self.citations.push(citation);
            self.citations.sort_by(|left, right| left.id.cmp(&right.id));
        }
    }

    pub fn delete_reference(&mut self, reference_id: &StableId, revision: u64) -> bool {
        if let Some(reference) = self
            .references
            .iter_mut()
            .find(|item| &item.id == reference_id)
        {
            if revision >= reference.revision {
                reference.revision = revision;
                reference.deleted = true;
            }
            true
        } else {
            false
        }
    }

    pub fn delete_citation(&mut self, citation_id: &StableId, revision: u64) -> bool {
        if let Some(citation) = self
            .citations
            .iter_mut()
            .find(|item| &item.id == citation_id)
        {
            if revision >= citation.revision {
                citation.revision = revision;
                citation.deleted = true;
                citation.rendered_cache = None;
            }
            true
        } else {
            false
        }
    }

    pub fn rendered_citation(&self, citation_id: &StableId) -> Option<&String> {
        self.citations
            .iter()
            .find(|item| &item.id == citation_id && !item.deleted)
            .and_then(|item| item.rendered_cache.as_ref())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BibliographyReference {
    pub id: StableId,
    pub revision: u64,
    pub source: CitationSource,
    pub summary: CitationSummary,
    pub deleted: bool,
}

impl BibliographyReference {
    pub fn validate(&self) -> Result<(), ModelError> {
        validate_stable_id("bibliography reference id", &self.id)?;
        if self
            .source
            .bytes
            .iter()
            .all(|byte| byte.is_ascii_whitespace())
        {
            return Err(ModelError::InvalidDocument("bibliography source is empty"));
        }
        if let CitationSourceFormat::Unknown(value) = &self.source.format {
            if value.trim().is_empty() {
                return Err(ModelError::InvalidDocument(
                    "bibliography source format is empty",
                ));
            }
            if value.trim() != value {
                return Err(ModelError::InvalidDocument(
                    "bibliography source format has surrounding whitespace",
                ));
            }
        }
        if self.summary.title.trim() != self.summary.title {
            return Err(ModelError::InvalidDocument(
                "bibliography summary field has surrounding whitespace",
            ));
        }
        for author in &self.summary.authors {
            if author.trim().is_empty() {
                return Err(ModelError::InvalidDocument(
                    "bibliography summary field is empty",
                ));
            }
            if author.trim() != author {
                return Err(ModelError::InvalidDocument(
                    "bibliography summary field has surrounding whitespace",
                ));
            }
        }
        for value in [
            self.summary.issued.as_deref(),
            self.summary.doi.as_deref(),
            self.summary.url.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            if value.trim().is_empty() {
                return Err(ModelError::InvalidDocument(
                    "bibliography summary field is empty",
                ));
            }
            if value.trim() != value {
                return Err(ModelError::InvalidDocument(
                    "bibliography summary field has surrounding whitespace",
                ));
            }
        }
        if self.summary.title.trim().is_empty()
            && self.summary.authors.is_empty()
            && self.summary.issued.is_none()
            && self.summary.doi.is_none()
            && self.summary.url.is_none()
        {
            return Err(ModelError::InvalidDocument("bibliography summary is empty"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CitationSource {
    pub format: CitationSourceFormat,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum CitationSourceFormat {
    CitumNative,
    CslJson,
    Bibtex,
    Ris,
    Unknown(String),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CitationSummary {
    pub title: String,
    pub authors: Vec<String>,
    pub issued: Option<String>,
    pub doi: Option<String>,
    pub url: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CitationGroup {
    pub id: StableId,
    pub revision: u64,
    pub items: Vec<CitationItem>,
    pub placement: CitationPlacement,
    pub rendered_cache: Option<String>,
    pub deleted: bool,
}

impl CitationGroup {
    pub fn validate_payload(&self) -> Result<(), ModelError> {
        validate_stable_id("citation group id", &self.id)?;
        if self.items.is_empty() {
            return Err(ModelError::InvalidDocument("citation group has no items"));
        }
        for item in &self.items {
            item.validate()?;
        }
        Ok(())
    }

    fn validate(&self, live_footnotes: &BTreeSet<StableId>) -> Result<(), ModelError> {
        self.validate_payload()?;
        if let CitationPlacement::Footnote { footnote_id } = &self.placement {
            validate_stable_id("footnote citation id", footnote_id)?;
            if !self.deleted && !live_footnotes.contains(footnote_id) {
                return Err(ModelError::InvalidDocument(
                    "footnote citation target is missing",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CitationItem {
    pub reference_id: StableId,
    pub locator: Option<String>,
    pub label: Option<String>,
    pub prefix: Option<String>,
    pub suffix: Option<String>,
    pub suppress_author: bool,
}

impl CitationItem {
    pub fn validate(&self) -> Result<(), ModelError> {
        validate_stable_id("citation item reference id", &self.reference_id)?;
        for value in [
            self.locator.as_deref(),
            self.label.as_deref(),
            self.prefix.as_deref(),
            self.suffix.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            if value.trim().is_empty() {
                return Err(ModelError::InvalidDocument("citation item field is empty"));
            }
            if value.trim() != value {
                return Err(ModelError::InvalidDocument(
                    "citation item field has surrounding whitespace",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum CitationPlacement {
    Inline,
    Footnote { footnote_id: StableId },
}
