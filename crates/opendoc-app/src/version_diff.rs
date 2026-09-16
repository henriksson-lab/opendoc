//! Comparison between two document snapshots.
//!
//! `opendoc-merge` is an apply/merge path only — its whole public surface is
//! `merge_operations` and `byte_index_for_char_offset`, and it has no
//! two-state comparison to reuse (see the Phase C report). What it *does* give
//! us for free is the guarantee this module rests on: every mutation addresses
//! blocks by `StableId` and never re-mints one, so ids are stable across
//! versions and id-keyed matching is sound.
//!
//! The block tree nests only through tables (`BlockKind::Table` → rows → cells
//! → blocks), so the walk below has exactly one recursive case.

use crate::version::AppVersionDiffEntry;
use opendoc_core::{
    Block, BlockKind, Document, Equation, HeaderFooterSlot, Inline, InsertPosition, Mark, StableId,
};
use opendoc_merge::OperationKind;
use std::collections::{BTreeMap, BTreeSet};

/// A block found in one snapshot, with where it sits and what it says.
struct IndexedBlock<'a> {
    block: &'a Block,
    path: String,
    order: usize,
}

/// Compare two snapshots and report added, removed and changed blocks.
///
/// Ordering is by position in the *new* document, with removals reported at the
/// position they had in the old one, so the result reads top to bottom.
pub(crate) fn diff_documents(before: &Document, after: &Document) -> Vec<AppVersionDiffEntry> {
    let mut old_index = BTreeMap::new();
    let mut order = 0usize;
    index_document_blocks(before, &mut old_index, &mut order);
    let mut new_index = BTreeMap::new();
    let mut order = 0usize;
    index_document_blocks(after, &mut new_index, &mut order);

    let mut entries: Vec<(usize, AppVersionDiffEntry)> = Vec::new();

    for (id, new_entry) in &new_index {
        match old_index.get(id) {
            None => entries.push((
                new_entry.order,
                AppVersionDiffEntry {
                    change: "added".to_string(),
                    block_id: id.clone(),
                    kind: block_kind_label(&new_entry.block.kind),
                    path: new_entry.path.clone(),
                    before_text: String::new(),
                    after_text: block_text(new_entry.block),
                },
            )),
            Some(old_entry) => {
                if blocks_differ(old_entry.block, new_entry.block) {
                    entries.push((
                        new_entry.order,
                        AppVersionDiffEntry {
                            change: "changed".to_string(),
                            block_id: id.clone(),
                            kind: block_kind_label(&new_entry.block.kind),
                            path: new_entry.path.clone(),
                            before_text: block_text(old_entry.block),
                            after_text: block_text(new_entry.block),
                        },
                    ));
                } else if old_entry.path != new_entry.path {
                    entries.push((
                        new_entry.order,
                        AppVersionDiffEntry {
                            change: "changed".to_string(),
                            block_id: id.clone(),
                            kind: block_kind_label(&new_entry.block.kind),
                            path: format!("{} → {}", old_entry.path, new_entry.path),
                            before_text: block_text(old_entry.block),
                            after_text: block_text(new_entry.block),
                        },
                    ));
                }
            }
        }
    }

    for (id, old_entry) in &old_index {
        if new_index.contains_key(id) {
            continue;
        }
        entries.push((
            old_entry.order,
            AppVersionDiffEntry {
                change: "removed".to_string(),
                block_id: id.clone(),
                kind: block_kind_label(&old_entry.block.kind),
                path: old_entry.path.clone(),
                before_text: block_text(old_entry.block),
                after_text: String::new(),
            },
        ));
    }

    index_footnotes(before, after, &mut entries);
    index_document_surfaces(before, after, &mut entries);

    // Primary key is position in the new document. A removed block has no
    // position there, so it is anchored at its old index and reported *after*
    // whatever now occupies that slot.
    entries.sort_by(|left, right| {
        let rank = |entry: &AppVersionDiffEntry| u8::from(entry.change == "removed");
        left.0
            .cmp(&right.0)
            .then_with(|| rank(&left.1).cmp(&rank(&right.1)))
            .then_with(|| left.1.block_id.cmp(&right.1.block_id))
    });
    entries.into_iter().map(|(_, entry)| entry).collect()
}

/// Index every block tree that is part of the saved document, not merely the
/// body.  Furniture has normal stable block IDs and can be changed through the
/// same operations as body content; leaving it out of a version comparison
/// made a header edit indistinguishable from no edit.  Footnotes are handled
/// separately below because their body is inline-only rather than a block
/// tree.
fn index_document_blocks<'a>(
    document: &'a Document,
    out: &mut BTreeMap<String, IndexedBlock<'a>>,
    order: &mut usize,
) {
    index_blocks(&document.blocks, "", out, order);
    index_blocks(&document.header, "header", out, order);
    index_blocks(&document.footer, "footer", out, order);
    if let Some(blocks) = &document.first_page_header {
        index_blocks(blocks, "first-page header", out, order);
    }
    if let Some(blocks) = &document.first_page_footer {
        index_blocks(blocks, "first-page footer", out, order);
    }
    if let Some(blocks) = &document.even_page_header {
        index_blocks(blocks, "even-page header", out, order);
    }
    if let Some(blocks) = &document.even_page_footer {
        index_blocks(blocks, "even-page footer", out, order);
    }
}

/// Append a diff entry for each changed footnote.  A footnote is not a block,
/// but its stable id is durable and its inline body is exactly what a reader
/// sees in a note.  Prefixing the id makes its different namespace explicit to
/// the client without minting a pretend block id.
fn index_footnotes(
    before: &Document,
    after: &Document,
    entries: &mut Vec<(usize, AppVersionDiffEntry)>,
) {
    let old = before
        .footnotes
        .iter()
        .map(|note| (note.id.to_string(), note))
        .collect::<BTreeMap<_, _>>();
    let new = after
        .footnotes
        .iter()
        .map(|note| (note.id.to_string(), note))
        .collect::<BTreeMap<_, _>>();
    let mut order = before.blocks.len().max(after.blocks.len()) + 10_000;
    for id in old.keys().chain(new.keys()).collect::<BTreeSet<_>>() {
        let (change, before_text, after_text) = match (old.get(id), new.get(id)) {
            (None, Some(note)) => ("added", String::new(), inline_texts(&note.body)),
            (Some(note), None) => ("removed", inline_texts(&note.body), String::new()),
            (Some(old_note), Some(new_note)) if old_note != new_note => (
                "changed",
                inline_texts(&old_note.body),
                inline_texts(&new_note.body),
            ),
            _ => continue,
        };
        entries.push((
            order,
            AppVersionDiffEntry {
                change: change.to_string(),
                block_id: format!("footnote:{id}"),
                kind: "footnote".to_string(),
                path: format!("footnote {id}"),
                before_text,
                after_text,
            },
        ));
        order += 1;
    }
}

/// Surface changes that do not have individual block identities.  These are
/// intentionally concise, but are not silently dropped: a reviewer can see
/// that review state, citations, or document settings changed and open either
/// version for the full read-only projection.  The summaries avoid presenting
/// a lossy reconstruction as a per-operation audit trail.
fn index_document_surfaces(
    before: &Document,
    after: &Document,
    entries: &mut Vec<(usize, AppVersionDiffEntry)>,
) {
    let base_order = 20_000usize;
    if metadata_differs(before, after) {
        entries.push(surface_entry(
            base_order,
            "document:metadata",
            "document metadata",
            "document › metadata",
            metadata_summary(before),
            metadata_summary(after),
        ));
    }
    if before.comments != after.comments
        || before.comment_history != after.comment_history
        || before.comment_activity != after.comment_activity
        || before.suggestions != after.suggestions
    {
        entries.push(surface_entry(
            base_order + 1,
            "document:review",
            "review metadata",
            "document › review",
            review_summary(before),
            review_summary(after),
        ));
    }
    if before.citation_database != after.citation_database {
        entries.push(surface_entry(
            base_order + 2,
            "document:citations",
            "citations",
            "document › citations",
            citation_summary(before),
            citation_summary(after),
        ));
    }
}

fn surface_entry(
    order: usize,
    id: &str,
    kind: &str,
    path: &str,
    before_text: String,
    after_text: String,
) -> (usize, AppVersionDiffEntry) {
    (
        order,
        AppVersionDiffEntry {
            change: "changed".to_string(),
            block_id: id.to_string(),
            kind: kind.to_string(),
            path: path.to_string(),
            before_text,
            after_text,
        },
    )
}

fn metadata_differs(before: &Document, after: &Document) -> bool {
    before.title != after.title
        || before.locale != after.locale
        || before.doi != after.doi
        || before.page_setup != after.page_setup
        || before.list_properties != after.list_properties
        || before.bookmarks != after.bookmarks
        || before.endnote_ids != after.endnote_ids
}

fn metadata_summary(document: &Document) -> String {
    format!(
        "title: {}; locale: {}; DOI: {}; page setup: {}; list settings: {}; bookmarks: {}; endnotes: {}",
        document.title,
        document.locale,
        document.doi.as_deref().unwrap_or("none"),
        if document.page_setup == Default::default() { "default" } else { "custom" },
        document.list_properties.len(),
        document.bookmarks.len(),
        document.endnote_ids.len(),
    )
}

fn review_summary(document: &Document) -> String {
    let deleted_threads = document
        .comments
        .iter()
        .filter(|thread| thread.deleted)
        .count();
    let resolved_threads = document
        .comments
        .iter()
        .filter(|thread| matches!(thread.state, opendoc_core::CommentThreadState::Resolved))
        .count();
    format!(
        "threads: {} ({} resolved, {} deleted); comment history: {}; comment activity: {}; suggestions: {}",
        document.comments.len(),
        resolved_threads,
        deleted_threads,
        document.comment_history.len(),
        document.comment_activity.len(),
        document.suggestions.len(),
    )
}

fn citation_summary(document: &Document) -> String {
    format!(
        "style: {}; locale: {}; references: {}; citation groups: {}",
        document.citation_database.style,
        document.citation_database.locale,
        document.citation_database.references.len(),
        document.citation_database.citations.len(),
    )
}

fn index_blocks<'a>(
    blocks: &'a [Block],
    prefix: &str,
    out: &mut BTreeMap<String, IndexedBlock<'a>>,
    order: &mut usize,
) {
    for (index, block) in blocks.iter().enumerate() {
        let path = if prefix.is_empty() {
            format!("{}", index + 1)
        } else {
            format!("{prefix} › {}", index + 1)
        };
        out.insert(
            block.id.to_string(),
            IndexedBlock {
                block,
                path: path.clone(),
                order: *order,
            },
        );
        *order += 1;
        if let BlockKind::Table { rows, .. } = &block.kind {
            for (row_index, row) in rows.iter().enumerate() {
                for (cell_index, cell) in row.cells.iter().enumerate() {
                    let cell_prefix =
                        format!("{path} › row {} › cell {}", row_index + 1, cell_index + 1);
                    index_blocks(&cell.blocks, &cell_prefix, out, order);
                }
            }
        }
    }
}

/// What a reader can see in one inline run: its text, its formatting and what
/// it points at — but never its identity.
///
/// Inline ids are stable for an untouched block today, so comparing whole
/// `Inline` values would usually agree with this. It is the failure mode that
/// argues against it: anything that rebuilds runs while preserving the text —
/// an importer round-trip, a paste, a merge that re-splits a run — re-mints the
/// ids, and an id-sensitive comparison would then report every such block as
/// changed and bury the real edits.
#[derive(PartialEq)]
enum InlineShape<'a> {
    Text(&'a str, &'a [Mark]),
    Link(&'a str, &'a str, &'a [Mark]),
    Citation(&'a str, Option<&'a str>),
    FootnoteRef(&'a str),
    Mention(&'a str),
    Dropdown(&'a str, &'a str),
    DateChip(&'a str),
    Equation(&'a Equation),
    PageNumberField(opendoc_core::PageNumberField),
}

fn content_shape(content: &[Inline]) -> Vec<InlineShape<'_>> {
    content.iter().map(inline_shape).collect()
}

fn inline_shape(inline: &Inline) -> InlineShape<'_> {
    match inline {
        Inline::Text { text, marks, .. } => InlineShape::Text(text, marks),
        Inline::Link {
            text, href, marks, ..
        } => InlineShape::Link(text, href, marks),
        Inline::Citation {
            citation_id,
            rendered_cache,
            ..
        } => InlineShape::Citation(citation_id.as_str(), rendered_cache.as_deref()),
        Inline::FootnoteRef { footnote_id, .. } => InlineShape::FootnoteRef(footnote_id.as_str()),
        Inline::Mention { label, .. }
        | Inline::GooglePersonChip { label, .. }
        | Inline::GoogleRichLinkChip { label, .. } => InlineShape::Mention(label),
        Inline::Dropdown {
            options,
            selected_option_id,
            ..
        } => InlineShape::Dropdown(
            selected_option_id,
            options
                .iter()
                .find(|option| option.id == *selected_option_id)
                .map(|option| option.label.as_str())
                .unwrap_or_default(),
        ),
        Inline::DateChip { date, .. } => InlineShape::DateChip(date),
        Inline::Equation { equation, .. } => InlineShape::Equation(equation),
        Inline::PageNumber { field, .. } => InlineShape::PageNumberField(*field),
    }
}

/// Whether a block changed *in itself*.
///
/// A table's derived `PartialEq` covers every nested block, so comparing tables
/// whole would report the table as changed for any edit inside any cell and
/// then report the edited block again. Tables are therefore compared on their
/// own shape only — row and cell identity and count — and nested blocks report
/// themselves.
fn blocks_differ(before: &Block, after: &Block) -> bool {
    if content_shape(&before.content) != content_shape(&after.content)
        || before.properties != after.properties
    {
        return true;
    }
    match (&before.kind, &after.kind) {
        (BlockKind::Table { rows: old_rows, .. }, BlockKind::Table { rows: new_rows, .. }) => {
            table_shape(old_rows) != table_shape(new_rows)
        }
        (old_kind, new_kind) => old_kind != new_kind,
    }
}

fn table_shape(rows: &[opendoc_core::TableRow]) -> Vec<(String, Vec<(String, usize)>)> {
    rows.iter()
        .map(|row| {
            (
                row.id.to_string(),
                row.cells
                    .iter()
                    .map(|cell| (cell.id.to_string(), cell.blocks.len()))
                    .collect(),
            )
        })
        .collect()
}

pub(crate) fn block_kind_label(kind: &BlockKind) -> String {
    match kind {
        BlockKind::Paragraph => "paragraph".to_string(),
        BlockKind::Title => "title".to_string(),
        BlockKind::Subtitle => "subtitle".to_string(),
        BlockKind::Heading { level } => format!("heading {level}"),
        BlockKind::ListItem { kind, level, .. } => {
            let marker = match kind {
                opendoc_core::ListKind::Bullet => "bulleted",
                opendoc_core::ListKind::Ordered => "ordered",
                opendoc_core::ListKind::Checklist { .. } => "checklist",
            };
            format!("{marker} list item (level {level})")
        }
        BlockKind::Table { rows, .. } => format!("table ({} rows)", rows.len()),
        BlockKind::EquationBlock { .. } => "equation".to_string(),
        BlockKind::Image { .. } => "image".to_string(),
        BlockKind::PageBreak => "page break".to_string(),
        BlockKind::HorizontalRule => "horizontal rule".to_string(),
        BlockKind::TableOfContents { .. } => "table of contents".to_string(),
        BlockKind::Bibliography => "bibliography".to_string(),
    }
}

/// Human-readable text for one block, excluding nested table content — nested
/// blocks appear as their own diff entries.
///
/// This is the one place that decides what a block "says": the equation source
/// stands in for an equation, the alt text for a picture, and a citation
/// renders as its cached text or its id. Every preview and every diff entry
/// reads it from here, so no view has to pick between those fields itself.
pub(crate) fn block_text(block: &Block) -> String {
    let mut text = block
        .content
        .iter()
        .map(inline_text)
        .collect::<Vec<_>>()
        .join("");
    match &block.kind {
        BlockKind::EquationBlock { equation } => text.push_str(&equation.source),
        BlockKind::Image { alt_text, .. } => text.push_str(alt_text),
        _ => {}
    }
    text.chars().take(200).collect()
}

fn inline_text(inline: &Inline) -> String {
    match inline {
        Inline::Text { text, .. } | Inline::Link { text, .. } => text.clone(),
        Inline::Citation {
            rendered_cache: Some(text),
            ..
        } => text.clone(),
        Inline::Citation { citation_id, .. } => format!("[{citation_id}]"),
        Inline::FootnoteRef { footnote_id, .. } => format!("[{footnote_id}]"),
        Inline::Mention { label, .. }
        | Inline::GooglePersonChip { label, .. }
        | Inline::GoogleRichLinkChip { label, .. } => label.clone(),
        Inline::Dropdown {
            options,
            selected_option_id,
            ..
        } => options
            .iter()
            .find(|option| option.id == *selected_option_id)
            .map(|option| option.label.clone())
            .unwrap_or_default(),
        Inline::DateChip { date, .. } => date.clone(),
        Inline::Equation { equation, .. } => equation.source.clone(),
        Inline::PageNumber { field, .. } => format!("[{}]", field.as_str()),
    }
}

fn inline_texts(inlines: &[Inline]) -> String {
    inlines
        .iter()
        .map(inline_text)
        .collect::<Vec<_>>()
        .join("")
        .chars()
        .take(200)
        .collect()
}

/// The typed operations that turn `current` into `restored`, or `None` when
/// the operation vocabulary cannot say it.
///
/// **Why a restore needs operations at all.** A repository's head is a
/// snapshot *and* an operation segment chain, and the two are supposed to
/// describe the same document: `merge_repository_candidates_inner` merges a
/// divergent candidate by replaying each side's operation delta onto the
/// snapshot at their common ancestor. `restore_version` used to replace the
/// document wholesale and journal one untyped `restore-version` marker, so the
/// delta across a restore was empty — and a candidate that branched before the
/// restore merged as though the restore had never happened, resurrecting
/// exactly the content the user had restored away.
///
/// **Why `None` is a real answer.** The vocabulary is an *editing* vocabulary,
/// and some differences between two arbitrary versions are not edits it can
/// express: a comment thread or a suggestion that exists now and not in the
/// restored version cannot be removed (`DeleteCommentThread` marks a thread
/// deleted, it does not unmake it), and a footnote or citation whose revision
/// has moved past the restored one cannot be moved back. The caller treats
/// `None` — and any plan that does not reproduce `restored` exactly — as "this
/// restore is not in the log", records it as such, and
/// `merge_repository_candidates_inner` refuses to merge a candidate across it
/// rather than silently resurrecting content.
///
/// Every plan this returns is still *verified* by the caller before it is
/// committed: the operations are applied and the result compared with
/// `restored`. That is what makes partial coverage safe rather than merely
/// optimistic — a case this function gets wrong degrades to the marker, it
/// does not commit a wrong document.
pub(crate) fn restore_operations(
    current: &Document,
    restored: &Document,
) -> Option<Vec<OperationKind>> {
    let mut operations = Vec::new();

    if current.title != restored.title {
        operations.push(OperationKind::SetDocumentTitle {
            title: restored.title.clone(),
        });
    }
    if current.locale != restored.locale {
        operations.push(OperationKind::SetDocumentLocale {
            locale: restored.locale.clone(),
        });
    }
    if current.doi != restored.doi {
        operations.push(OperationKind::SetDocumentDoi {
            doi: restored.doi.clone(),
        });
    }
    if current.page_setup != restored.page_setup {
        operations.push(OperationKind::SetPageSetup {
            page_setup: restored.page_setup,
        });
    }
    // Whole-slot, which is how page furniture is edited anyway: header blocks
    // are not reachable by the block-addressed operations (ADR 0009).
    for slot in HeaderFooterSlot::ALL {
        if current.furniture(slot) != restored.furniture(slot)
            || current.has_furniture_override(slot) != restored.has_furniture_override(slot)
        {
            operations.push(OperationKind::SetPageFurniture {
                slot,
                blocks: restored.furniture(slot).to_vec(),
            });
        }
    }

    operations.extend(restore_block_operations(&current.blocks, &restored.blocks));
    operations.extend(restore_footnote_operations(current, restored)?);
    operations.extend(restore_annotation_operations(current, restored)?);

    // Citations are the one collection with no partial support: every entry is
    // soft-deleted rather than removed, so "present now, absent in the
    // restored version" has no operation, and `UpsertCitationGroup` drops the
    // rendered cache on the way through, which would leave the log replaying
    // to a document that differs from the snapshot in a field the reader can
    // see.
    if current.citation_database != restored.citation_database {
        return None;
    }

    Some(operations)
}

/// Delete/insert operations that turn one top-level block list into another.
///
/// Blocks that are already equal **and already in the right relative order**
/// are left alone — matched greedily, which yields a common subsequence — so a
/// restore that changes one paragraph journals one delete and one insert
/// rather than rewriting the whole body into the segment. Everything else is
/// deleted and re-inserted under its own id, which is exactly what a
/// wholesale replacement is.
///
/// The deletes all precede the inserts, so a block that is re-inserted under
/// an id it already had is gone by the time the insert runs (the merge refuses
/// a duplicate id). Each insert anchors on the restored block before it, which
/// is either one of the kept blocks or one this batch already inserted.
fn restore_block_operations(current: &[Block], restored: &[Block]) -> Vec<OperationKind> {
    let mut kept: BTreeSet<StableId> = BTreeSet::new();
    let mut cursor = 0usize;
    for block in restored {
        if let Some(offset) = current[cursor..]
            .iter()
            .position(|candidate| candidate.id == block.id && candidate == block)
        {
            cursor += offset + 1;
            kept.insert(block.id.clone());
        }
    }

    let mut operations = Vec::new();
    for block in current {
        if !kept.contains(&block.id) {
            operations.push(OperationKind::DeleteBlock {
                block_id: block.id.clone(),
            });
        }
    }
    let mut previous: Option<StableId> = None;
    for block in restored {
        if !kept.contains(&block.id) {
            operations.push(OperationKind::InsertBlock {
                position: match &previous {
                    Some(anchor) => InsertPosition::After(anchor.clone()),
                    None => InsertPosition::First,
                },
                block: block.clone(),
            });
        }
        previous = Some(block.id.clone());
    }
    operations
}

/// Footnotes are revision-gated upserts, so a restore can only move one
/// *forward*. A footnote the restored version does not have at all, or one
/// whose restored revision is behind the current one, has no operation.
fn restore_footnote_operations(
    current: &Document,
    restored: &Document,
) -> Option<Vec<OperationKind>> {
    if current.footnotes == restored.footnotes {
        return Some(Vec::new());
    }
    for footnote in &current.footnotes {
        if !restored
            .footnotes
            .iter()
            .any(|target| target.id == footnote.id)
        {
            return None;
        }
    }
    let mut operations = Vec::new();
    for footnote in &restored.footnotes {
        match current
            .footnotes
            .iter()
            .find(|existing| existing.id == footnote.id)
        {
            Some(existing) if existing == footnote => {}
            Some(existing) if existing.revision > footnote.revision => return None,
            _ => operations.push(OperationKind::UpsertFootnote {
                footnote: footnote.clone(),
            }),
        }
    }
    Some(operations)
}

/// Comment threads and suggestions can be *added* back, and a thread's deleted
/// flag can be moved either way. Anything else — a thread or suggestion that
/// exists now and not in the restored version, or one whose body differs — has
/// no operation that produces it.
fn restore_annotation_operations(
    current: &Document,
    restored: &Document,
) -> Option<Vec<OperationKind>> {
    let mut operations = Vec::new();
    for thread in &current.comments {
        if !restored
            .comments
            .iter()
            .any(|target| target.id == thread.id)
        {
            return None;
        }
    }
    for thread in &restored.comments {
        match current.comments.iter().find(|item| item.id == thread.id) {
            None => operations.push(OperationKind::AddCommentThread {
                thread: thread.clone(),
            }),
            Some(existing) if existing == thread => {}
            Some(existing) => {
                let mut probe = existing.clone();
                probe.deleted = thread.deleted;
                if &probe != thread {
                    return None;
                }
                operations.push(if thread.deleted {
                    OperationKind::DeleteCommentThread {
                        thread_id: thread.id.clone(),
                    }
                } else {
                    OperationKind::RestoreCommentThread {
                        thread_id: thread.id.clone(),
                    }
                });
            }
        }
    }

    for suggestion in &current.suggestions {
        if !restored
            .suggestions
            .iter()
            .any(|target| target == suggestion)
        {
            return None;
        }
    }
    for suggestion in &restored.suggestions {
        if !current.suggestions.iter().any(|item| item == suggestion) {
            operations.push(OperationKind::AddSuggestion {
                suggestion: suggestion.clone(),
            });
        }
    }
    Some(operations)
}

#[cfg(test)]
mod tests {
    use super::*;
    use opendoc_core::{Block, BlockKind, Document, Footnote, StableId, TableCell, TableRow};

    fn paragraph(id: &str, text: &str) -> Block {
        let mut block = Block::paragraph(text);
        block.id = StableId::parse(id).expect("test block id is valid");
        block
    }

    fn table_block(rows: Vec<TableRow>) -> Block {
        let mut block = Block::paragraph("");
        block.id = StableId::parse("block-table").expect("test block id is valid");
        block.kind = BlockKind::table(rows);
        block.content = Vec::new();
        block
    }

    fn document(blocks: Vec<Block>) -> Document {
        let mut document = Document::new("Doc");
        document.blocks = blocks;
        document
    }

    #[test]
    fn reports_added_removed_and_changed_blocks_by_stable_id() {
        let before = document(vec![
            paragraph("block-1", "one"),
            paragraph("block-2", "two"),
            paragraph("block-3", "three"),
        ]);
        let after = document(vec![
            paragraph("block-1", "one"),
            paragraph("block-2", "two edited"),
            paragraph("block-4", "four"),
        ]);

        let entries = diff_documents(&before, &after);
        let summary: Vec<_> = entries
            .iter()
            .map(|entry| (entry.change.as_str(), entry.block_id.as_str()))
            .collect();
        assert_eq!(
            summary,
            vec![
                ("changed", "block-2"),
                ("added", "block-4"),
                ("removed", "block-3"),
            ]
        );
        let changed = &entries[0];
        assert_eq!(changed.before_text, "two");
        assert_eq!(changed.after_text, "two edited");
        assert_eq!(changed.path, "2");
        assert_eq!(changed.kind, "paragraph");
    }

    #[test]
    fn identical_documents_have_no_entries() {
        let doc = document(vec![paragraph("block-1", "one")]);
        assert!(diff_documents(&doc, &doc).is_empty());
    }

    #[test]
    fn a_moved_block_is_reported_once_with_both_positions() {
        let before = document(vec![
            paragraph("block-1", "one"),
            paragraph("block-2", "two"),
        ]);
        let after = document(vec![
            paragraph("block-2", "two"),
            paragraph("block-1", "one"),
        ]);
        let entries = diff_documents(&before, &after);
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().all(|entry| entry.change == "changed"));
        assert_eq!(entries[0].block_id, "block-2");
        assert_eq!(entries[0].path, "2 → 1");
    }

    #[test]
    fn a_table_reports_only_the_edited_cell_block_not_the_whole_table() {
        let table = |cell_text: &str| {
            table_block(vec![TableRow {
                id: StableId::parse("row-1").expect("test row id is valid"),
                height: None,
                header: false,
                cells: vec![TableCell {
                    id: StableId::parse("cell-1").expect("test cell id is valid"),
                    span: Default::default(),
                    properties: Default::default(),
                    blocks: vec![paragraph("block-inner", cell_text)],
                }],
            }])
        };
        let before = document(vec![table("before")]);
        let after = document(vec![table("after")]);
        let entries = diff_documents(&before, &after);
        assert_eq!(entries.len(), 1, "{entries:?}");
        assert_eq!(entries[0].block_id, "block-inner");
        assert_eq!(entries[0].path, "1 › row 1 › cell 1 › 1");
        assert_eq!(entries[0].before_text, "before");
        assert_eq!(entries[0].after_text, "after");
    }

    #[test]
    fn a_structural_table_change_is_reported_on_the_table() {
        let one_row = table_block(vec![TableRow {
            id: StableId::parse("row-1").expect("test row id is valid"),
            height: None,
            header: false,
            cells: vec![TableCell {
                id: StableId::parse("cell-1").expect("test cell id is valid"),
                span: Default::default(),
                properties: Default::default(),
                blocks: vec![paragraph("block-inner", "text")],
            }],
        }]);
        let mut two_rows = one_row.clone();
        if let BlockKind::Table { rows, .. } = &mut two_rows.kind {
            rows.push(TableRow {
                id: StableId::parse("row-2").expect("test row id is valid"),
                height: None,
                header: false,
                cells: vec![TableCell {
                    id: StableId::parse("cell-2").expect("test cell id is valid"),
                    span: Default::default(),
                    properties: Default::default(),
                    blocks: vec![paragraph("block-inner-2", "second")],
                }],
            });
        }
        let entries = diff_documents(&document(vec![one_row]), &document(vec![two_rows]));
        let summary: Vec<_> = entries
            .iter()
            .map(|entry| (entry.change.as_str(), entry.block_id.as_str()))
            .collect();
        assert_eq!(
            summary,
            vec![("changed", "block-table"), ("added", "block-inner-2")]
        );
    }

    #[test]
    fn includes_furniture_and_footnote_bodies_not_just_document_blocks() {
        let mut before = document(vec![paragraph("block-body", "body")]);
        before.header = vec![paragraph("block-header", "before header")];
        before.footnotes = vec![Footnote {
            id: StableId::parse("note-1").expect("test id"),
            revision: 1,
            body: vec![opendoc_core::Inline::text("before note")],
            deleted: false,
        }];
        let mut after = before.clone();
        after.header[0] = paragraph("block-header", "after header");
        after.footnotes[0].body = vec![opendoc_core::Inline::text("after note")];
        after.footnotes[0].revision = 2;

        let entries = diff_documents(&before, &after);
        assert!(entries.iter().any(|entry| {
            entry.block_id == "block-header"
                && entry.path == "header › 1"
                && entry.before_text == "before header"
                && entry.after_text == "after header"
        }));
        assert!(entries.iter().any(|entry| {
            entry.block_id == "footnote:note-1"
                && entry.kind == "footnote"
                && entry.before_text == "before note"
                && entry.after_text == "after note"
        }));
    }

    #[test]
    fn includes_concise_entries_for_non_block_durable_surfaces() {
        let before = document(vec![paragraph("block-1", "one")]);
        let mut after = before.clone();
        after.title = "Renamed".to_string();
        after.citation_database.style = "chicago-author-date".to_string();

        let entries = diff_documents(&before, &after);
        assert!(entries.iter().any(|entry| {
            entry.block_id == "document:metadata"
                && entry.kind == "document metadata"
                && entry.before_text.contains("title: Doc")
                && entry.after_text.contains("title: Renamed")
        }));
        assert!(entries.iter().any(|entry| {
            entry.block_id == "document:citations"
                && entry.kind == "citations"
                && entry.before_text.contains("style: apa")
                && entry.after_text.contains("style: chicago-author-date")
        }));
    }
}
