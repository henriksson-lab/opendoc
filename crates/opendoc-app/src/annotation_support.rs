use crate::AppApiError;
use opendoc_core::CitationGroup;

pub(crate) fn render_citation_cache(
    database: &opendoc_core::CitationDatabase,
    citation: &CitationGroup,
) -> Option<String> {
    let rendered = opendoc_citations::render_citation_group(database, citation);
    if rendered == format!("[{}]", citation.id) {
        None
    } else {
        Some(rendered)
    }
}

pub(crate) fn normalize_citation_style(value: String) -> Result<String, AppApiError> {
    let value = value.trim().to_ascii_lowercase();
    if value.is_empty() {
        Err(AppApiError::Format("citation style is empty".to_string()))
    } else {
        Ok(value)
    }
}

pub(crate) fn normalize_citation_locale(value: String) -> Result<String, AppApiError> {
    let value = value.trim().to_string();
    if value.is_empty() {
        Err(AppApiError::Format("citation locale is empty".to_string()))
    } else {
        Ok(value)
    }
}

pub(crate) fn normalize_source_text(value: String, label: &str) -> Result<String, AppApiError> {
    if value.trim().is_empty() {
        Err(AppApiError::Format(format!("{label} is empty")))
    } else {
        Ok(value)
    }
}

pub(crate) fn normalize_source_author(value: String, label: &str) -> Result<String, AppApiError> {
    let value = value.trim().to_string();
    if value.is_empty() {
        Err(AppApiError::Format(format!("{label} is empty")))
    } else {
        Ok(value)
    }
}

pub(crate) fn normalize_bibliography_authors(
    authors: Vec<String>,
) -> Result<Vec<String>, AppApiError> {
    let authors = authors
        .into_iter()
        .map(|author| author.trim().to_string())
        .filter(|author| !author.is_empty())
        .collect::<Vec<_>>();
    if authors.is_empty() {
        Err(AppApiError::Format(
            "bibliography reference requires at least one author".to_string(),
        ))
    } else {
        Ok(authors)
    }
}

pub(crate) fn normalize_optional_source_string(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}
