//! Citation database, reference and group projection DTOs.

use super::*;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppCitationDatabase {
    pub style: String,
    pub locale: String,
    pub references: Vec<AppBibliographyReference>,
    pub bibliography: Vec<AppBibliographyEntry>,
    pub citations: Vec<AppCitationGroup>,
}

impl AppCitationDatabase {
    pub(crate) fn from_core(database: &opendoc_core::CitationDatabase) -> Self {
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

    pub(crate) fn to_core(&self) -> Result<opendoc_core::CitationDatabase, AppApiError> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The desktop/API route projects an `AppDocument` after each command.
    /// Keep this cold default-APA case here rather than testing the direct
    /// mutation method: `AppCitationDatabase::from_core` is where a previous
    /// empty-document projection reached the deeply nested CSL decoder on an
    /// already-shallow test stack. A real reference must continue to render;
    /// the empty-bibliography fast path must not weaken APA behavior.
    #[test]
    fn cold_default_apa_reference_projects_through_the_app_dispatch_route() {
        let mut app = OpenDocApp::new_empty_document();
        app.dispatch_command("create_document", json!({ "title": "Citations" }))
            .expect("create the document");
        let AppCommandResult::Document(projected) = app
            .dispatch_command(
                "add_bibliography_reference",
                json!({
                    "title": "A projection reference",
                    "authors": ["Doe"],
                    "issued": "2024",
                    "doi": null,
                    "url": null,
                }),
            )
            .expect("add and project an APA reference")
        else {
            panic!("bibliography command must return a document");
        };

        assert_eq!(projected.citations.style, "apa");
        assert_eq!(projected.citations.bibliography.len(), 1);
        assert!(
            projected.citations.bibliography[0].text.contains("Doe"),
            "the default APA bibliography projection retained its rendered entry"
        );
    }
}
