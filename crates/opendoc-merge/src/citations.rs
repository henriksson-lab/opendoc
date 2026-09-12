//! Citation placement/reference repair and rendered-label cache invalidation.

use opendoc_core::{Block, BlockKind, CitationPlacement, Document, Inline, ModelWarning, StableId};
use std::collections::BTreeSet;

pub(crate) fn repair_citation_placements(
    document: &mut Document,
    warnings: &mut Vec<ModelWarning>,
) {
    let live_footnotes = document
        .footnotes
        .iter()
        .filter(|footnote| !footnote.deleted)
        .map(|footnote| footnote.id.clone())
        .collect::<BTreeSet<_>>();
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
        warnings.push(ModelWarning {
            code: "citation-footnote-target-missing".to_string(),
            message: format!(
                "citation group {} moved inline because footnote {missing_footnote_id} was missing",
                citation.id
            ),
        });
    }
}

pub(crate) fn repair_citation_references(
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
        let missing = citation
            .items
            .iter()
            .any(|item| !live_references.contains(&item.reference_id));
        if !missing {
            continue;
        }
        citation.rendered_cache = None;
        affected_citations.push(citation.id.clone());
    }
    affected_citations.sort();
    affected_citations.dedup();
    for citation_id in affected_citations {
        invalidate_inline_citation_caches(&mut document.blocks, &citation_id);
        warnings.push(ModelWarning {
            code: "citation-reference-missing".to_string(),
            message: format!(
                "citation group {citation_id} references a missing bibliography record"
            ),
        });
    }
}

pub(crate) fn repair_inline_citation_labels(
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
    clear_missing_inline_citation_caches(&mut document.blocks, &live_citations, &mut affected);
    for citation_id in affected {
        warnings.push(ModelWarning {
            code: "citation-group-missing".to_string(),
            message: format!(
                "inline citation label {citation_id} references a missing citation group"
            ),
        });
    }
}

pub(crate) fn refresh_citation_projection_caches(document: &mut Document) {
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
            continue;
        }
        citation.rendered_cache = Some(opendoc_citations::render_citation_group(
            &database, citation,
        ));
    }
}

pub(crate) fn clear_missing_inline_citation_caches(
    blocks: &mut [Block],
    live_citations: &BTreeSet<StableId>,
    affected: &mut BTreeSet<StableId>,
) {
    for block in blocks {
        for inline in &mut block.content {
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
        if let BlockKind::Table { rows, .. } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    clear_missing_inline_citation_caches(
                        &mut cell.blocks,
                        live_citations,
                        affected,
                    );
                }
            }
        }
    }
}

pub(crate) fn invalidate_citation_caches_for_reference(
    document: &mut Document,
    reference_id: &StableId,
) {
    let mut affected_citations = BTreeSet::new();
    for citation in &mut document.citation_database.citations {
        if citation.deleted {
            continue;
        }
        if citation
            .items
            .iter()
            .any(|item| &item.reference_id == reference_id)
        {
            citation.rendered_cache = None;
            affected_citations.insert(citation.id.clone());
        }
    }
    for citation_id in affected_citations {
        invalidate_inline_citation_caches(&mut document.blocks, &citation_id);
    }
}

pub(crate) fn invalidate_all_citation_caches(document: &mut Document) {
    for citation in &mut document.citation_database.citations {
        citation.rendered_cache = None;
    }
    invalidate_all_inline_citation_caches(&mut document.blocks);
}

pub(crate) fn invalidate_inline_citation_caches(blocks: &mut [Block], citation_id: &StableId) {
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
        if let BlockKind::Table { rows, .. } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    invalidate_inline_citation_caches(&mut cell.blocks, citation_id);
                }
            }
        }
    }
}

pub(crate) fn invalidate_all_inline_citation_caches(blocks: &mut [Block]) {
    for block in blocks {
        for inline in &mut block.content {
            if let Inline::Citation { rendered_cache, .. } = inline {
                *rendered_cache = None;
            }
        }
        if let BlockKind::Table { rows, .. } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    invalidate_all_inline_citation_caches(&mut cell.blocks);
                }
            }
        }
    }
}
