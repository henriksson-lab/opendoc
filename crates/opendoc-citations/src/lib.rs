//! Citation parsing and rendering for OpenDoc documents.
//!
//! Real CSL styles (APA, MLA, Chicago, IEEE, Vancouver, Harvard, Nature, and
//! every other style bundled with `hayagriva`) are rendered through the
//! [`hayagriva`] CSL engine. Reference sources in BibTeX/BibLaTeX, RIS, and
//! CSL-JSON are parsed into a rich reference model. The pre-existing
//! `render_citation_group` / `render_bibliography` entry points remain and fall
//! back to the original lightweight renderer only for style names that no CSL
//! style is known for (such as the historical `apa-7th` default).

mod model;
mod parse;
mod render;
mod styles;

use std::fmt;

pub use hayagriva;
pub use model::{
    details_from_entry, entry_from_details, entry_from_summary, reference_details, reference_entry,
    summary_from_details, ReferenceAuthor, ReferenceDetails,
};
pub use parse::{
    detect_source_format, parse_bibtex, parse_csl_json, parse_reference_date,
    parse_reference_details, parse_reference_source, parse_ris, ReferenceParseError,
};
pub use render::{
    render_bibliography_rich, render_citation, render_citations, render_database,
    render_footnote_citation, segments_text, RenderedCitation, RenderedDatabase,
    RichBibliographyEntry, RichSegment,
};
pub use styles::{
    all_style_names, available_locales, available_styles, load_style, resolve_locale,
    resolve_style_name, style_info, CitationStyleInfo,
};

use opendoc_core::{
    BibliographyReference, CitationDatabase, CitationGroup, CitationSourceFormat, CitationSummary,
};

/// Errors produced while rendering citations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CitationError {
    /// No CSL style is known for the given name.
    UnknownStyle(String),
    /// The CSL engine could not produce output.
    Render(String),
}

impl fmt::Display for CitationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownStyle(style) => write!(f, "unknown citation style {style}"),
            Self::Render(message) => write!(f, "citation rendering failed: {message}"),
        }
    }
}

impl std::error::Error for CitationError {}

/// Style names whose plain-text output from [`render_citation_group`] and
/// [`render_bibliography`] existing callers (document projections, caches,
/// and their fixtures) depend on. These keep the legacy renderer in the two
/// compatibility entry points; [`render_citation`], [`render_database`], and
/// [`render_bibliography_rich`] always use the CSL engine (`ieee` included).
pub const LEGACY_WRAPPER_STYLES: &[&str] = &["apa-7th", "numeric", "ieee"];

/// Whether the compatibility entry points keep the legacy renderer for a style.
pub fn is_legacy_wrapper_style(style: &str) -> bool {
    let style = style.trim().to_ascii_lowercase();
    LEGACY_WRAPPER_STYLES.contains(&style.as_str())
}

/// Whether [`render_citation_group`] and [`render_bibliography`] render the
/// given style through the CSL engine rather than the legacy renderer.
pub fn style_uses_csl(style: &str) -> bool {
    !is_legacy_wrapper_style(style) && resolve_style_name(style).is_some()
}

/// Render a citation group to plain text.
///
/// Known CSL styles render through `hayagriva` with numbering and
/// disambiguation computed over the whole database; unknown styles and the
/// [`LEGACY_WRAPPER_STYLES`] use the legacy renderer. Groups referencing
/// missing or uncitable references render as the `[citation-id]` placeholder
/// in both cases.
pub fn render_citation_group(database: &CitationDatabase, citation: &CitationGroup) -> String {
    if is_legacy_wrapper_style(&database.style) {
        return render_citation_group_legacy(database, citation);
    }
    match render::render_citation(database, citation) {
        Ok(rendered) => rendered.text,
        Err(_) => render_citation_group_legacy(database, citation),
    }
}

/// Render the bibliography to plain text entries, sorted per the style.
pub fn render_bibliography(database: &CitationDatabase) -> Vec<RenderedBibliographyEntry> {
    if is_legacy_wrapper_style(&database.style) {
        return render_bibliography_legacy(database);
    }
    match render::render_bibliography_rich(database) {
        Ok(entries) => entries
            .into_iter()
            .map(|entry| RenderedBibliographyEntry {
                reference_id: entry.reference_id,
                text: entry.text,
            })
            .collect(),
        Err(_) => render_bibliography_legacy(database),
    }
}

/// The original renderer, kept for style names without a CSL style.
pub fn render_citation_group_legacy(
    database: &CitationDatabase,
    citation: &CitationGroup,
) -> String {
    if matches!(database.style.as_str(), "numeric" | "ieee") {
        let numbers = citation
            .items
            .iter()
            .map(|item| {
                database
                    .references
                    .iter()
                    .position(|reference| reference.id == item.reference_id && !reference.deleted)
                    .map(|index| (index + 1).to_string())
                    .unwrap_or_else(|| item.reference_id.to_string())
            })
            .collect::<Vec<_>>();
        return format!("[{}]", numbers.join(", "));
    }

    let mut unresolved_reference = false;
    let items = citation
        .items
        .iter()
        .map(|item| {
            let reference = database
                .references
                .iter()
                .find(|reference| reference.id == item.reference_id && !reference.deleted);
            if reference.is_none()
                || reference.is_some_and(|reference| {
                    reference.summary.authors.is_empty() && reference.summary.issued.is_none()
                })
            {
                unresolved_reference = true;
            }
            let mut label = if item.suppress_author {
                reference
                    .and_then(|reference| reference.summary.issued.clone())
                    .unwrap_or_else(|| item.reference_id.to_string())
            } else {
                reference
                    .and_then(|reference| reference.summary.authors.first().cloned())
                    .unwrap_or_else(|| item.reference_id.to_string())
            };

            if let Some(reference) = reference {
                if !item.suppress_author {
                    if let Some(issued) = &reference.summary.issued {
                        label.push(' ');
                        label.push_str(issued);
                    }
                } else if label.is_empty() {
                    label = item.reference_id.to_string();
                }
            }
            if let Some(locator) = &item.locator {
                label.push_str(", ");
                if let Some(locator_label) = &item.label {
                    label.push_str(locator_label);
                    label.push(' ');
                }
                label.push_str(locator);
            }
            if let Some(prefix) = &item.prefix {
                label = format!("{prefix} {label}");
            }
            if let Some(suffix) = &item.suffix {
                label.push(' ');
                label.push_str(suffix);
            }
            label
        })
        .collect::<Vec<_>>();
    if unresolved_reference {
        return format!("[{}]", citation.id);
    }
    format!("({})", items.join("; "))
}

/// The original bibliography renderer, kept for style names without a CSL style.
pub fn render_bibliography_legacy(database: &CitationDatabase) -> Vec<RenderedBibliographyEntry> {
    let numeric = matches!(database.style.as_str(), "numeric" | "ieee");
    database
        .references
        .iter()
        .filter(|reference| !reference.deleted)
        .enumerate()
        .map(|(index, reference)| RenderedBibliographyEntry {
            reference_id: reference.id.to_string(),
            text: render_bibliography_reference(reference, index + 1, numeric),
        })
        .collect()
}

/// A plain-text bibliography entry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderedBibliographyEntry {
    pub reference_id: String,
    pub text: String,
}

pub(crate) fn render_bibliography_reference(
    reference: &BibliographyReference,
    number: usize,
    numeric: bool,
) -> String {
    let authors = if reference.summary.authors.is_empty() {
        reference.id.to_string()
    } else {
        reference.summary.authors.join(", ")
    };
    let year = reference.summary.issued.as_deref().unwrap_or("n.d.");
    let title = if reference.summary.title.trim().is_empty() {
        "Untitled".to_string()
    } else {
        reference.summary.title.clone()
    };

    if numeric {
        return format!("[{number}] {authors}. {title}. {year}");
    }

    let mut text = format!("{authors} ({year}). {title}.");
    if let Some(doi) = &reference.summary.doi {
        text.push_str(" doi:");
        text.push_str(doi);
    } else if let Some(url) = &reference.summary.url {
        text.push(' ');
        text.push_str(url);
    }
    text
}

pub fn citation_source_bytes(reference: &BibliographyReference) -> Vec<u8> {
    match &reference.source.format {
        CitationSourceFormat::CitumNative => citum_native_source_bytes(reference),
        CitationSourceFormat::CslJson
        | CitationSourceFormat::Bibtex
        | CitationSourceFormat::Ris
        | CitationSourceFormat::Unknown(_) => reference.source.bytes.clone(),
    }
}

pub fn citum_native_source_bytes(reference: &BibliographyReference) -> Vec<u8> {
    let mut out = format!("title: {}\n", escape_citum_value(&reference.summary.title));
    if !reference.summary.authors.is_empty() {
        out.push_str(&format!(
            "author: {}\n",
            reference
                .summary
                .authors
                .iter()
                .map(|author| escape_citum_value(author))
                .collect::<Vec<_>>()
                .join("; ")
        ));
    }
    if let Some(issued) = &reference.summary.issued {
        out.push_str(&format!("year: {}\n", escape_citum_value(issued)));
    }
    if let Some(doi) = &reference.summary.doi {
        out.push_str(&format!("doi: {}\n", escape_citum_value(doi)));
    }
    if let Some(url) = &reference.summary.url {
        out.push_str(&format!("url: {}\n", escape_citum_value(url)));
    }
    out.into_bytes()
}

pub fn parse_citum_native_summary(bytes: &[u8]) -> Option<CitationSummary> {
    let text = std::str::from_utf8(bytes).ok()?;
    let mut summary = CitationSummary {
        title: String::new(),
        authors: Vec::new(),
        issued: None,
        doi: None,
        url: None,
    };

    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        match key.trim().to_ascii_lowercase().as_str() {
            "title" => summary.title = unescape_citum_value(value),
            "author" | "authors" => {
                summary.authors = split_citum_author_list(value)
                    .into_iter()
                    .filter(|author| !author.is_empty())
                    .collect();
            }
            "year" | "issued" => summary.issued = Some(unescape_citum_value(value)),
            "doi" => summary.doi = Some(unescape_citum_value(value)),
            "url" => summary.url = Some(unescape_citum_value(value)),
            _ => {}
        }
    }

    if summary.title.trim().is_empty()
        && summary.authors.is_empty()
        && summary.issued.is_none()
        && summary.doi.is_none()
        && summary.url.is_none()
    {
        None
    } else {
        Some(summary)
    }
}

pub fn merge_summary_with_citum_native_source(
    mut summary: CitationSummary,
    source_bytes: &[u8],
) -> CitationSummary {
    let Some(parsed) = parse_citum_native_summary(source_bytes) else {
        return summary;
    };

    if summary.title.trim().is_empty() {
        summary.title = parsed.title;
    }
    if summary.authors.is_empty() {
        summary.authors = parsed.authors;
    }
    if summary.issued.is_none() {
        summary.issued = parsed.issued;
    }
    if summary.doi.is_none() {
        summary.doi = parsed.doi;
    }
    if summary.url.is_none() {
        summary.url = parsed.url;
    }
    summary
}

fn escape_citum_value(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            ';' => out.push_str("\\;"),
            _ => out.push(ch),
        }
    }
    out
}

fn unescape_citum_value(value: &str) -> String {
    let mut out = String::new();
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('\\') => out.push('\\'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some(';') => out.push(';'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

fn split_citum_author_list(value: &str) -> Vec<String> {
    let mut authors = Vec::new();
    let mut current = String::new();
    let mut escaped = false;
    for ch in value.chars() {
        if escaped {
            current.push('\\');
            current.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == ';' {
            let author = unescape_citum_value(current.trim());
            if !author.is_empty() {
                authors.push(author);
            }
            current.clear();
        } else {
            current.push(ch);
        }
    }
    if escaped {
        current.push('\\');
    }
    let author = unescape_citum_value(current.trim());
    if !author.is_empty() {
        authors.push(author);
    }
    authors
}

#[cfg(test)]
mod tests;
