//! Block-level comparison between two document snapshots.
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
use opendoc_core::{Block, BlockKind, Document, Equation, Inline, Mark};
use std::collections::BTreeMap;

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
    index_blocks(&before.blocks, "", &mut old_index, &mut order);
    let mut new_index = BTreeMap::new();
    let mut order = 0usize;
    index_blocks(&after.blocks, "", &mut new_index, &mut order);

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
        Inline::Mention { label, .. } => InlineShape::Mention(label),
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

fn block_kind_label(kind: &BlockKind) -> String {
    match kind {
        BlockKind::Paragraph => "paragraph".to_string(),
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
    }
}

/// Human-readable text for one block, excluding nested table content — nested
/// blocks appear as their own diff entries.
fn block_text(block: &Block) -> String {
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
        Inline::Mention { label, .. } => label.clone(),
        Inline::Equation { equation, .. } => equation.source.clone(),
        Inline::PageNumber { field, .. } => format!("[{}]", field.as_str()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opendoc_core::{Block, BlockKind, Document, StableId, TableCell, TableRow};

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
}
