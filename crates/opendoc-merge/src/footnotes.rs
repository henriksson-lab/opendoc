//! Footnote reference bookkeeping and the repairs that keep bodies and refs in step.

use opendoc_core::{Block, BlockKind, CitationPlacement, Document, Inline, ModelWarning, StableId};
use std::collections::BTreeSet;

pub(crate) fn repair_unreferenced_footnotes(
    document: &mut Document,
    warnings: &mut Vec<ModelWarning>,
) {
    // Same shape as `repair_suggestion_anchors`: every footnote already
    // deleted is skipped below, so with none live there is no question for the
    // reference scan to answer — and that scan walks every block, every table
    // cell and the citation database to collect the ids it would compare
    // against nothing.
    if document.footnotes.iter().all(|footnote| footnote.deleted) {
        return;
    }
    let mut referenced = footnote_reference_ids(&document.blocks);
    referenced.extend(citation_footnote_reference_ids(document));
    for footnote in &mut document.footnotes {
        if footnote.deleted || referenced.contains(&footnote.id) {
            continue;
        }
        footnote.deleted = true;
        footnote.revision = footnote.revision.saturating_add(1);
        warnings.push(ModelWarning {
            code: "footnote-reference-missing".to_string(),
            message: format!("footnote {} has no surviving reference", footnote.id),
        });
    }
}

pub(crate) fn repair_missing_footnote_references(
    document: &mut Document,
    warnings: &mut Vec<ModelWarning>,
) {
    let live_footnotes = document
        .footnotes
        .iter()
        .filter(|footnote| !footnote.deleted)
        .map(|footnote| footnote.id.clone())
        .collect::<BTreeSet<_>>();
    let mut removed = BTreeSet::new();
    remove_missing_footnote_references_from_blocks(
        &mut document.blocks,
        &live_footnotes,
        &mut removed,
    );
    for footnote_id in removed {
        warnings.push(ModelWarning {
            code: "footnote-reference-target-missing".to_string(),
            message: format!(
                "footnote reference to {footnote_id} was removed because its target was missing"
            ),
        });
    }
}

pub(crate) fn remove_missing_footnote_references_from_blocks(
    blocks: &mut [Block],
    live_footnotes: &BTreeSet<StableId>,
    removed: &mut BTreeSet<StableId>,
) {
    for block in blocks {
        block.content.retain(|inline| {
            if let Inline::FootnoteRef { footnote_id, .. } = inline {
                if !live_footnotes.contains(footnote_id) {
                    removed.insert(footnote_id.clone());
                    return false;
                }
            }
            true
        });
        if let BlockKind::Table { rows, .. } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    remove_missing_footnote_references_from_blocks(
                        &mut cell.blocks,
                        live_footnotes,
                        removed,
                    );
                }
            }
        }
    }
}

pub(crate) fn citation_footnote_reference_ids(document: &Document) -> BTreeSet<StableId> {
    document
        .citation_database
        .citations
        .iter()
        .filter(|citation| !citation.deleted)
        .filter_map(|citation| match &citation.placement {
            CitationPlacement::Footnote { footnote_id } => Some(footnote_id.clone()),
            CitationPlacement::Inline => None,
        })
        .collect()
}

pub(crate) fn footnote_reference_ids(blocks: &[Block]) -> BTreeSet<StableId> {
    let mut ids = BTreeSet::new();
    collect_footnote_reference_ids(blocks, &mut ids);
    ids
}

pub(crate) fn collect_footnote_reference_ids(blocks: &[Block], ids: &mut BTreeSet<StableId>) {
    for block in blocks {
        for inline in &block.content {
            if let Inline::FootnoteRef { footnote_id, .. } = inline {
                ids.insert(footnote_id.clone());
            }
        }
        if let BlockKind::Table { rows, .. } = &block.kind {
            for row in rows {
                for cell in &row.cells {
                    collect_footnote_reference_ids(&cell.blocks, ids);
                }
            }
        }
    }
}
