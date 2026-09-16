//! Google Docs citation import/export and the repairs it needs.

use crate::error::ImportError;
use crate::json::{
    citation_source_format_from_label, expect_object, optional_array, optional_bool,
    optional_checked_string, optional_source_string, optional_str, optional_u64,
    parse_imported_stable_id, required_array, required_str,
};
use opendoc_citations::{merge_summary_with_citum_native_source, render_citation_group};
use opendoc_core::{
    BibliographyReference, Block, BlockKind, CitationDatabase, CitationGroup, CitationItem,
    CitationPlacement, CitationSource, CitationSourceFormat, CitationSummary, Document, Inline,
    ModelWarning, StableId,
};
use serde_json::{json, Value};
use std::collections::BTreeSet;

pub(crate) fn import_google_citations(
    value: &Value,
    warnings: &mut Vec<ModelWarning>,
) -> Result<CitationDatabase, ImportError> {
    let Some(citations) = value.get("opendocCitations") else {
        return Ok(CitationDatabase::default());
    };
    expect_object(citations, "opendocCitations")?;
    warnings.push(ModelWarning {
        code: "opendoc-google-citations-extension".to_string(),
        message: "imported document-local citation database from OpenDoc Google-shaped extension"
            .to_string(),
    });
    // The style and locale a Google payload leaves out are the model's own
    // defaults, read from the model rather than spelled again here: a second
    // copy of a default is a value that can disagree with the first.
    let defaults = CitationDatabase::default();
    let mut database = CitationDatabase {
        style: optional_str(citations, "style")?
            .unwrap_or(&defaults.style)
            .to_string(),
        locale: optional_str(citations, "locale")?
            .unwrap_or(&defaults.locale)
            .to_string(),
        references: Vec::new(),
        citations: Vec::new(),
    };
    if let Some(references) = optional_array(citations, "references")? {
        for reference in references {
            expect_object(reference, "citation reference")?;
            database
                .references
                .push(import_google_reference(reference)?);
        }
    }
    if let Some(groups) = optional_array(citations, "groups")? {
        for group in groups {
            expect_object(group, "citation group")?;
            database
                .citations
                .push(import_google_citation_group(group)?);
        }
    }
    database
        .references
        .sort_by(|left, right| left.id.cmp(&right.id));
    database
        .citations
        .sort_by(|left, right| left.id.cmp(&right.id));
    Ok(database)
}

pub(crate) fn import_google_reference(value: &Value) -> Result<BibliographyReference, ImportError> {
    let id = required_str(value, "id", "citation reference")?;
    let summary_value = value.get("summary").unwrap_or(&Value::Null);
    if !summary_value.is_null() {
        expect_object(summary_value, "citation summary")?;
    }
    let format =
        citation_source_format_from_label(optional_str(value, "format")?.unwrap_or("citum-native"));
    let source_bytes = optional_str(value, "bytesUtf8")?
        .unwrap_or_default()
        .as_bytes()
        .to_vec();
    let authors = optional_array(summary_value, "authors")?
        .map(|authors| {
            authors
                .iter()
                .map(|author| {
                    author.as_str().map(ToString::to_string).ok_or_else(|| {
                        ImportError::InvalidInput(
                            "citation summary authors entries must be strings".to_string(),
                        )
                    })
                })
                .collect()
        })
        .transpose()?
        .unwrap_or_default();
    let mut summary = CitationSummary {
        title: optional_str(summary_value, "title")?
            .unwrap_or_default()
            .to_string(),
        authors,
        issued: optional_source_string(summary_value, "issued", "bibliography summary field")?,
        doi: optional_source_string(summary_value, "doi", "bibliography summary field")?,
        url: optional_source_string(summary_value, "url", "bibliography summary field")?,
    };
    if format == CitationSourceFormat::CitumNative {
        summary = merge_summary_with_citum_native_source(summary, &source_bytes);
    }
    Ok(BibliographyReference {
        id: parse_imported_stable_id(id)?,
        revision: optional_u64(value, "revision")?.unwrap_or(1),
        source: CitationSource {
            format,
            bytes: source_bytes,
        },
        summary,
        deleted: optional_bool(value, "deleted")?.unwrap_or(false),
    })
}

pub(crate) fn import_google_citation_group(value: &Value) -> Result<CitationGroup, ImportError> {
    let id = required_str(value, "id", "citation group")?;
    let items = required_array(value, "items", "citation group")?
        .iter()
        .map(|item| {
            expect_object(item, "citation item")?;
            import_google_citation_item(item)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let placement = match optional_str(value, "placement")?.unwrap_or("inline") {
        "inline" => CitationPlacement::Inline,
        "footnote" => CitationPlacement::Footnote {
            footnote_id: parse_imported_stable_id(required_str(
                value,
                "footnoteId",
                "footnote citation",
            )?)?,
        },
        other => {
            return Err(ImportError::InvalidInput(format!(
                "unsupported citation placement {other}"
            )));
        }
    };
    Ok(CitationGroup {
        id: parse_imported_stable_id(id)?,
        revision: optional_u64(value, "revision")?.unwrap_or(1),
        items,
        placement,
        rendered_cache: optional_checked_string(value, "renderedCache")?,
        deleted: optional_bool(value, "deleted")?.unwrap_or(false),
    })
}

pub(crate) fn import_google_citation_item(value: &Value) -> Result<CitationItem, ImportError> {
    let reference_id = required_str(value, "referenceId", "citation item")?;
    Ok(CitationItem {
        reference_id: parse_imported_stable_id(reference_id)?,
        locator: optional_source_string(value, "locator", "citation item field")?,
        label: optional_source_string(value, "label", "citation item field")?,
        prefix: optional_source_string(value, "prefix", "citation item field")?,
        suffix: optional_source_string(value, "suffix", "citation item field")?,
        suppress_author: optional_bool(value, "suppressAuthor")?.unwrap_or(false),
    })
}

pub(crate) fn repair_imported_citation_placements(
    document: &mut Document,
    warnings: &mut Vec<ModelWarning>,
) {
    let live_footnotes = document
        .footnotes
        .iter()
        .filter(|footnote| !footnote.deleted)
        .map(|footnote| footnote.id.clone())
        .collect::<BTreeSet<_>>();
    let mut moved_inline = Vec::new();
    for citation in &mut document.citation_database.citations {
        if citation.deleted {
            continue;
        }
        let CitationPlacement::Footnote { footnote_id } = &citation.placement else {
            continue;
        };
        if live_footnotes.contains(footnote_id) {
            continue;
        }
        let missing_footnote_id = footnote_id.clone();
        citation.placement = CitationPlacement::Inline;
        citation.rendered_cache = None;
        moved_inline.push(citation.id.clone());
        warnings.push(ModelWarning {
            code: "citation-footnote-target-missing".to_string(),
            message: format!(
                "citation group {} moved inline because footnote {missing_footnote_id} was missing",
                citation.id
            ),
        });
    }
    for citation_id in moved_inline {
        clear_imported_citation_cache_everywhere(document, &citation_id);
    }
}

pub(crate) fn repair_imported_citation_references(
    document: &mut Document,
    warnings: &mut Vec<ModelWarning>,
) {
    let live_references = document
        .citation_database
        .references
        .iter()
        .filter(|reference| !reference.deleted)
        .map(|reference| reference.id.clone())
        .collect::<BTreeSet<_>>();
    let mut affected_citations = Vec::new();
    for citation in &mut document.citation_database.citations {
        if citation.deleted {
            continue;
        }
        if citation
            .items
            .iter()
            .any(|item| !live_references.contains(&item.reference_id))
        {
            citation.rendered_cache = None;
            affected_citations.push(citation.id.clone());
        }
    }
    affected_citations.sort();
    affected_citations.dedup();
    for citation_id in affected_citations {
        clear_imported_citation_cache_everywhere(document, &citation_id);
        warnings.push(ModelWarning {
            code: "citation-reference-missing".to_string(),
            message: format!(
                "citation group {citation_id} references a missing bibliography record"
            ),
        });
    }
}

pub(crate) fn clear_imported_inline_citation_cache(blocks: &mut [Block], citation_id: &StableId) {
    for block in blocks {
        clear_imported_inline_citation_cache_in_inlines(&mut block.content, citation_id);
        if let BlockKind::Table { rows, .. } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    clear_imported_inline_citation_cache(&mut cell.blocks, citation_id);
                }
            }
        }
    }
}

/// Citation labels are projections of the document-level bibliography, not
/// source text.  Google-shaped input can put an inline sequence in more than
/// the ordinary body: notes, review evidence, and suggested insertions all
/// travel through the same extension.  A broken group/reference must not
/// leave one of those less common surfaces displaying a stale imported label.
fn clear_imported_citation_cache_everywhere(document: &mut Document, citation_id: &StableId) {
    clear_imported_inline_citation_cache(&mut document.blocks, citation_id);
    clear_imported_inline_citation_cache(&mut document.header, citation_id);
    clear_imported_inline_citation_cache(&mut document.footer, citation_id);
    if let Some(blocks) = &mut document.first_page_header {
        clear_imported_inline_citation_cache(blocks, citation_id);
    }
    if let Some(blocks) = &mut document.first_page_footer {
        clear_imported_inline_citation_cache(blocks, citation_id);
    }
    if let Some(blocks) = &mut document.even_page_header {
        clear_imported_inline_citation_cache(blocks, citation_id);
    }
    if let Some(blocks) = &mut document.even_page_footer {
        clear_imported_inline_citation_cache(blocks, citation_id);
    }
    for footnote in &mut document.footnotes {
        clear_imported_inline_citation_cache_in_inlines(&mut footnote.body, citation_id);
    }
    for thread in &mut document.comments {
        for comment in &mut thread.comments {
            clear_imported_inline_citation_cache_in_inlines(&mut comment.body, citation_id);
        }
    }
    for entry in &mut document.comment_history {
        if let Some(body) = &mut entry.previous_body {
            clear_imported_inline_citation_cache_in_inlines(body, citation_id);
        }
    }
    for suggestion in &mut document.suggestions {
        if let opendoc_core::SuggestionKind::Insert { content, .. } = &mut suggestion.kind {
            clear_imported_inline_citation_cache_in_inlines(content, citation_id);
        }
    }
}

fn clear_imported_inline_citation_cache_in_inlines(inlines: &mut [Inline], citation_id: &StableId) {
    for inline in inlines {
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
}

pub(crate) fn repair_imported_inline_citation_labels(
    document: &mut Document,
    warnings: &mut Vec<ModelWarning>,
) {
    let live_citations = document
        .citation_database
        .citations
        .iter()
        .filter(|citation| !citation.deleted)
        .map(|citation| citation.id.clone())
        .collect::<BTreeSet<_>>();
    let mut affected = BTreeSet::new();
    clear_imported_missing_citation_caches_everywhere(document, &live_citations, &mut affected);
    for citation_id in affected {
        warnings.push(ModelWarning {
            code: "citation-group-missing".to_string(),
            message: format!(
                "inline citation label {citation_id} references a missing citation group"
            ),
        });
    }
}

pub(crate) fn refresh_imported_citation_projection_caches(document: &mut Document) {
    let live_references = document
        .citation_database
        .references
        .iter()
        .filter(|reference| !reference.deleted)
        .map(|reference| reference.id.clone())
        .collect::<BTreeSet<_>>();
    let database = document.citation_database.clone();
    for citation in &mut document.citation_database.citations {
        if citation.deleted
            || citation
                .items
                .iter()
                .any(|item| !live_references.contains(&item.reference_id))
        {
            citation.rendered_cache = None;
        } else {
            let rendered = render_citation_group(&database, citation);
            citation.rendered_cache = if rendered == format!("[{}]", citation.id) {
                None
            } else {
                Some(rendered)
            };
        }
    }
}

pub(crate) fn clear_imported_missing_inline_citation_caches(
    blocks: &mut [Block],
    live_citations: &BTreeSet<StableId>,
    affected: &mut BTreeSet<StableId>,
) {
    for block in blocks {
        clear_imported_missing_citation_caches_in_inlines(
            &mut block.content,
            live_citations,
            affected,
        );
        if let BlockKind::Table { rows, .. } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    clear_imported_missing_inline_citation_caches(
                        &mut cell.blocks,
                        live_citations,
                        affected,
                    );
                }
            }
        }
    }
}

fn clear_imported_missing_citation_caches_everywhere(
    document: &mut Document,
    live_citations: &BTreeSet<StableId>,
    affected: &mut BTreeSet<StableId>,
) {
    clear_imported_missing_inline_citation_caches(&mut document.blocks, live_citations, affected);
    clear_imported_missing_inline_citation_caches(&mut document.header, live_citations, affected);
    clear_imported_missing_inline_citation_caches(&mut document.footer, live_citations, affected);
    if let Some(blocks) = &mut document.first_page_header {
        clear_imported_missing_inline_citation_caches(blocks, live_citations, affected);
    }
    if let Some(blocks) = &mut document.first_page_footer {
        clear_imported_missing_inline_citation_caches(blocks, live_citations, affected);
    }
    if let Some(blocks) = &mut document.even_page_header {
        clear_imported_missing_inline_citation_caches(blocks, live_citations, affected);
    }
    if let Some(blocks) = &mut document.even_page_footer {
        clear_imported_missing_inline_citation_caches(blocks, live_citations, affected);
    }
    for footnote in &mut document.footnotes {
        clear_imported_missing_citation_caches_in_inlines(
            &mut footnote.body,
            live_citations,
            affected,
        );
    }
    for thread in &mut document.comments {
        for comment in &mut thread.comments {
            clear_imported_missing_citation_caches_in_inlines(
                &mut comment.body,
                live_citations,
                affected,
            );
        }
    }
    for entry in &mut document.comment_history {
        if let Some(body) = &mut entry.previous_body {
            clear_imported_missing_citation_caches_in_inlines(body, live_citations, affected);
        }
    }
    for suggestion in &mut document.suggestions {
        if let opendoc_core::SuggestionKind::Insert { content, .. } = &mut suggestion.kind {
            clear_imported_missing_citation_caches_in_inlines(content, live_citations, affected);
        }
    }
}

fn clear_imported_missing_citation_caches_in_inlines(
    inlines: &mut [Inline],
    live_citations: &BTreeSet<StableId>,
    affected: &mut BTreeSet<StableId>,
) {
    for inline in inlines {
        if let Inline::Citation {
            citation_id,
            rendered_cache,
            ..
        } = inline
        {
            if !live_citations.contains(citation_id) {
                *rendered_cache = None;
                affected.insert(citation_id.clone());
            }
        }
    }
}

/// Whether the Google export has to carry the citation extension at all.
///
/// A database that is exactly the model's default and holds nothing says
/// nothing a reader could not reconstruct. The comparison is against
/// [`CitationDatabase::default`] rather than against a copy of the default
/// spelled here, which is what let the two drift apart.
pub(crate) fn should_export_citations(database: &CitationDatabase) -> bool {
    let defaults = CitationDatabase::default();
    database.style != defaults.style
        || database.locale != defaults.locale
        || !database.references.is_empty()
        || !database.citations.is_empty()
}

pub(crate) fn export_google_citations(database: &CitationDatabase) -> Result<Value, ImportError> {
    if database.style.trim().is_empty() || database.locale.trim().is_empty() {
        return Err(ImportError::UnsupportedStructure(
            "OpenDoc citation database metadata is empty".to_string(),
        ));
    }
    let references = database
        .references
        .iter()
        .map(export_google_reference)
        .collect::<Result<Vec<_>, _>>()?;
    let groups = database
        .citations
        .iter()
        .map(export_google_citation_group)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(json!({
        "style": database.style,
        "locale": database.locale,
        "references": references,
        "groups": groups
    }))
}

pub(crate) fn export_google_reference(
    reference: &BibliographyReference,
) -> Result<Value, ImportError> {
    reference
        .validate()
        .map_err(|err| ImportError::UnsupportedStructure(format!("OpenDoc {err}")))?;
    Ok(json!({
        "id": reference.id.to_string(),
        "revision": reference.revision,
        "format": citation_source_format_label(&reference.source.format),
        "bytesUtf8": String::from_utf8_lossy(&reference.source.bytes),
        "summary": {
            "title": reference.summary.title,
            "authors": reference.summary.authors,
            "issued": reference.summary.issued,
            "doi": reference.summary.doi,
            "url": reference.summary.url,
        },
        "deleted": reference.deleted
    }))
}

pub(crate) fn export_google_citation_group(citation: &CitationGroup) -> Result<Value, ImportError> {
    citation
        .validate_payload()
        .map_err(|err| ImportError::UnsupportedStructure(format!("OpenDoc {err}")))?;
    let mut value = json!({
        "id": citation.id.to_string(),
        "revision": citation.revision,
        "items": citation.items.iter().map(export_google_citation_item).collect::<Vec<_>>(),
        "placement": match &citation.placement {
            CitationPlacement::Inline => "inline".to_string(),
            CitationPlacement::Footnote { .. } => "footnote".to_string(),
        },
        "renderedCache": citation.rendered_cache,
        "deleted": citation.deleted
    });
    if let CitationPlacement::Footnote { footnote_id } = &citation.placement {
        value["footnoteId"] = json!(footnote_id.to_string());
    }
    Ok(value)
}

pub(crate) fn export_google_citation_item(item: &CitationItem) -> Value {
    json!({
        "referenceId": item.reference_id.to_string(),
        "locator": item.locator,
        "label": item.label,
        "prefix": item.prefix,
        "suffix": item.suffix,
        "suppressAuthor": item.suppress_author
    })
}

pub(crate) fn citation_source_format_label(format: &CitationSourceFormat) -> String {
    match format {
        CitationSourceFormat::CitumNative => "citum-native".to_string(),
        CitationSourceFormat::CslJson => "csl-json".to_string(),
        CitationSourceFormat::Bibtex => "bibtex".to_string(),
        CitationSourceFormat::Ris => "ris".to_string(),
        CitationSourceFormat::Unknown(value) => value.clone(),
    }
}
