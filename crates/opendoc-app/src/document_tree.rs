use crate::{inline_id, AppBlock};
use opendoc_core::{new_list_id, Block, BlockKind, BlockProperties, Inline, ListKind, StableId};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn block_exists(blocks: &[Block], block_id: &StableId) -> bool {
    blocks.iter().any(|block| {
        &block.id == block_id
            || match &block.kind {
                BlockKind::Table { rows, .. } => rows
                    .iter()
                    .flat_map(|row| row.cells.iter())
                    .any(|cell| block_exists(&cell.blocks, block_id)),
                _ => false,
            }
    })
}

pub(crate) fn default_table_block() -> Block {
    let cell = |text: &str| opendoc_core::TableCell::new(vec![Block::paragraph(text)]);
    Block {
        id: StableId::new("block"),
        kind: BlockKind::table(vec![
            opendoc_core::TableRow {
                id: StableId::new("row"),
                height: None,
                header: false,
                cells: vec![cell("A1"), cell("B1")],
            },
            opendoc_core::TableRow {
                id: StableId::new("row"),
                height: None,
                header: false,
                cells: vec![cell("A2"), cell("B2")],
            },
        ]),
        content: Vec::new(),
        properties: BlockProperties::default(),
    }
}

// Block-tree nodes touched by block lookups on this thread.
//
// The cost this counts is the one that used to be quadratic: searching the
// tree for a block by id visits every block it passes, and callers were doing
// it inside loops over a *selection*, so the search ran once per selected
// block. Tests read the counter around an edit and check that it grows
// linearly with the document, which a per-selected-block rescan cannot do.
// Thread-local because the test harness runs tests in parallel threads in one
// process.
#[cfg(test)]
thread_local! {
    static BLOCK_LOOKUP_VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
#[inline]
pub(crate) fn record_block_lookup_visits(count: usize) {
    BLOCK_LOOKUP_VISITS.with(|visits| visits.set(visits.get() + count));
}

#[cfg(not(test))]
#[inline]
pub(crate) fn record_block_lookup_visits(_count: usize) {}

/// Run `body`, returning its value and the number of block-tree nodes that
/// block lookups visited while it ran.
#[cfg(test)]
pub(crate) fn measure_block_lookup_visits<T>(body: impl FnOnce() -> T) -> (T, usize) {
    let before = BLOCK_LOOKUP_VISITS.with(|visits| visits.get());
    let value = body();
    let after = BLOCK_LOOKUP_VISITS.with(|visits| visits.get());
    (value, after - before)
}

pub(crate) fn find_block_in_blocks<'a>(
    blocks: &'a [Block],
    block_id_to_find: &StableId,
) -> Option<&'a Block> {
    for block in blocks {
        record_block_lookup_visits(1);
        if &block.id == block_id_to_find {
            return Some(block);
        }
        if let BlockKind::Table { rows, .. } = &block.kind {
            for row in rows {
                for cell in &row.cells {
                    if let Some(found) = find_block_in_blocks(&cell.blocks, block_id_to_find) {
                        return Some(found);
                    }
                }
            }
        }
    }
    None
}

pub(crate) fn app_editable_inline_ids(blocks: &[AppBlock]) -> Vec<String> {
    blocks
        .iter()
        .flat_map(|block| {
            let direct = block
                .content
                .iter()
                .filter(|inline| {
                    matches!(
                        inline.kind.as_str(),
                        "text" | "link" | "equation" | "mention"
                    )
                })
                .map(|inline| inline.id.clone())
                .collect::<Vec<_>>();
            let nested = block
                .rows
                .iter()
                .flat_map(|row| row.iter())
                .flat_map(|cell| app_editable_inline_ids(cell))
                .collect::<Vec<_>>();
            direct.into_iter().chain(nested).collect::<Vec<_>>()
        })
        .collect()
}

pub(crate) fn blocks_reference_blob(blocks: &[Block], blob_hash: &str) -> bool {
    blocks.iter().any(|block| match &block.kind {
        BlockKind::Image {
            blob_hash: hash, ..
        } => hash == blob_hash,
        BlockKind::Table { rows, .. } => rows
            .iter()
            .flat_map(|row| row.cells.iter())
            .any(|cell| blocks_reference_blob(&cell.blocks, blob_hash)),
        _ => false,
    })
}

pub(crate) fn block_contains_inline(
    blocks: &[Block],
    block_id: &StableId,
    inline_id_to_find: &StableId,
) -> bool {
    blocks.iter().any(|block| {
        if &block.id == block_id {
            return block
                .content
                .iter()
                .any(|inline| inline_id(inline) == inline_id_to_find);
        }
        match &block.kind {
            BlockKind::Table { rows, .. } => rows
                .iter()
                .flat_map(|row| row.cells.iter())
                .any(|cell| block_contains_inline(&cell.blocks, block_id, inline_id_to_find)),
            _ => false,
        }
    })
}

pub(crate) fn find_inline_in_blocks<'a>(
    blocks: &'a [Block],
    inline_id_to_find: &StableId,
) -> Option<&'a Inline> {
    for block in blocks {
        if let Some(inline) = block
            .content
            .iter()
            .find(|inline| inline_id(inline) == inline_id_to_find)
        {
            return Some(inline);
        }
        if let BlockKind::Table { rows, .. } = &block.kind {
            for row in rows {
                for cell in &row.cells {
                    if let Some(inline) = find_inline_in_blocks(&cell.blocks, inline_id_to_find) {
                        return Some(inline);
                    }
                }
            }
        }
    }
    None
}

pub(crate) fn find_block_id_containing_inline(
    blocks: &[Block],
    inline_id_to_find: &StableId,
) -> Option<StableId> {
    for block in blocks {
        if block
            .content
            .iter()
            .any(|inline| inline_id(inline) == inline_id_to_find)
        {
            return Some(block.id.clone());
        }
        if let BlockKind::Table { rows, .. } = &block.kind {
            for row in rows {
                for cell in &row.cells {
                    if let Some(found) =
                        find_block_id_containing_inline(&cell.blocks, inline_id_to_find)
                    {
                        return Some(found);
                    }
                }
            }
        }
    }
    None
}

pub(crate) fn block_tree_contains_id(blocks: &[Block], block_id_to_find: &StableId) -> bool {
    for block in blocks {
        if &block.id == block_id_to_find {
            return true;
        }
        if let BlockKind::Table { rows, .. } = &block.kind {
            for row in rows {
                for cell in &row.cells {
                    if block_tree_contains_id(&cell.blocks, block_id_to_find) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// List identity
//
// A *list run* is a maximal sequence of adjacent sibling list items sharing one
// `list_id`. The run is what numbering, nesting and list-level styling are
// reasoned about, so two lists separated by a paragraph must not share an id —
// which is exactly what the old static `list-main` id got wrong.
// ---------------------------------------------------------------------------

/// The sibling slice that directly contains `block_id`, plus its index in it.
/// Blocks nested in table cells live in their cell's slice, not the document's.
/// The direct sibling container of a block, whether that is the document body
/// or a table cell. Structural editing must use this rather than assuming a
/// top-level block index.
pub(crate) fn sibling_slice<'a>(
    blocks: &'a [Block],
    block_id: &StableId,
) -> Option<(&'a [Block], usize)> {
    if let Some(index) = blocks.iter().position(|block| &block.id == block_id) {
        return Some((blocks, index));
    }
    for block in blocks {
        if let BlockKind::Table { rows, .. } = &block.kind {
            for row in rows {
                for cell in &row.cells {
                    if let Some(found) = sibling_slice(&cell.blocks, block_id) {
                        return Some(found);
                    }
                }
            }
        }
    }
    None
}

/// The run a newly inserted list item joins: the run of the block it is
/// inserted after, or — when it lands at the head of an existing run — that
/// run. With no list neighbour it starts a run of its own.
pub(crate) fn list_id_for_new_item(blocks: &[Block], after: Option<&StableId>) -> StableId {
    let Some(after) = after else {
        // Inserted at the very front of the document.
        return blocks
            .first()
            .and_then(Block::list_id)
            .cloned()
            .unwrap_or_else(new_list_id);
    };
    let Some((siblings, index)) = sibling_slice(blocks, after) else {
        return new_list_id();
    };
    if let Some(list_id) = siblings[index].list_id() {
        return list_id.clone();
    }
    siblings
        .get(index + 1)
        .and_then(Block::list_id)
        .cloned()
        .unwrap_or_else(new_list_id)
}

/// Run ids for a set of blocks all becoming list items in one gesture.
///
/// Blocks that end up adjacent share a run, a converted block next to an
/// existing run joins it, and a block that is already a list item keeps the run
/// it has. Computing the whole set at once is what stops a three-paragraph
/// selection from becoming three one-item lists.
pub(crate) fn list_ids_for_converted_blocks(
    blocks: &[Block],
    converting: &BTreeSet<StableId>,
) -> BTreeMap<StableId, StableId> {
    let mut assigned = BTreeMap::new();
    assign_list_ids_in_siblings(blocks, converting, &mut assigned);
    assigned
}

fn assign_list_ids_in_siblings(
    siblings: &[Block],
    converting: &BTreeSet<StableId>,
    assigned: &mut BTreeMap<StableId, StableId>,
) {
    // The run id of the previous sibling, once the conversion has happened.
    let mut run: Option<StableId> = None;
    for (index, block) in siblings.iter().enumerate() {
        if let BlockKind::Table { rows, .. } = &block.kind {
            for row in rows {
                for cell in &row.cells {
                    assign_list_ids_in_siblings(&cell.blocks, converting, assigned);
                }
            }
        }
        let converts = converting.contains(&block.id);
        let existing = block.list_id();
        if !converts && existing.is_none() {
            run = None;
            continue;
        }
        let list_id = match (converts, existing) {
            // Untouched list item: it keeps the run it already has.
            (false, Some(existing)) => existing.clone(),
            // Converted block adjacent to a run: join it.
            (true, _) if run.is_some() => run.clone().expect("run is some"),
            // Already a list item, just restyled: keep its run.
            (true, Some(existing)) => existing.clone(),
            // At the head of an existing run: join that instead of orphaning
            // a one-item list in front of it.
            (true, None) => siblings
                .get(index + 1)
                .filter(|next| !converting.contains(&next.id))
                .and_then(|next| next.list_id())
                .cloned()
                .unwrap_or_else(new_list_id),
            (false, None) => unreachable!("handled above"),
        };
        if converts {
            assigned.insert(block.id.clone(), list_id.clone());
        }
        run = Some(list_id);
    }
}

/// One list item that must be moved to a fresh run, as
/// `(block id, new list id, level, kind)`.
pub(crate) type ListRunReassignment = (StableId, StableId, u8, ListKind);

/// Re-identifies the tail of every list run that `leaving` blocks are about to
/// cut in half.
///
/// Converting a list item in the middle of a run into a paragraph leaves the
/// items after it carrying the old run's id, so numbering would keep counting
/// across the paragraph now separating the two halves. Each surviving segment
/// after a cut gets its own run id instead.
pub(crate) fn list_run_split_reassignments(
    blocks: &[Block],
    leaving: &BTreeSet<StableId>,
) -> Vec<ListRunReassignment> {
    let mut out = Vec::new();
    collect_list_run_splits(blocks, leaving, &mut out);
    out
}

fn collect_list_run_splits(
    siblings: &[Block],
    leaving: &BTreeSet<StableId>,
    out: &mut Vec<ListRunReassignment>,
) {
    let mut current_run: Option<StableId> = None;
    let mut cut = false;
    let mut replacement: Option<StableId> = None;
    for block in siblings {
        if let BlockKind::Table { rows, .. } = &block.kind {
            for row in rows {
                for cell in &row.cells {
                    collect_list_run_splits(&cell.blocks, leaving, out);
                }
            }
        }
        let BlockKind::ListItem {
            list_id,
            level,
            kind,
        } = &block.kind
        else {
            current_run = None;
            cut = false;
            replacement = None;
            continue;
        };
        if current_run.as_ref() != Some(list_id) {
            current_run = Some(list_id.clone());
            cut = false;
            replacement = None;
        }
        if leaving.contains(&block.id) {
            cut = true;
            // A later segment of the same run needs its own id, not this one's.
            replacement = None;
            continue;
        }
        if cut {
            let new_id = replacement.get_or_insert_with(new_list_id).clone();
            out.push((block.id.clone(), new_id, *level, *kind));
        }
    }
}

/// Every place two adjacent sibling list runs could be one run: the earlier
/// run's id paired with the later run's id, for runs that are directly
/// adjacent and wear the same marker.
///
/// Taken before and after an edit, the difference is exactly the set of runs
/// that edit brought together — see
/// [`list_run_merge_reassignments`].
pub(crate) fn adjacent_list_run_pairs(blocks: &[Block]) -> BTreeSet<(StableId, StableId)> {
    let mut out = BTreeSet::new();
    collect_adjacent_list_run_pairs(blocks, &mut out);
    out
}

fn collect_adjacent_list_run_pairs(siblings: &[Block], out: &mut BTreeSet<(StableId, StableId)>) {
    let mut previous: Option<(&StableId, &ListKind)> = None;
    for block in siblings {
        if let BlockKind::Table { rows, .. } = &block.kind {
            for row in rows {
                for cell in &row.cells {
                    collect_adjacent_list_run_pairs(&cell.blocks, out);
                }
            }
        }
        let BlockKind::ListItem { list_id, kind, .. } = &block.kind else {
            previous = None;
            continue;
        };
        if let Some((previous_id, previous_kind)) = previous {
            if previous_id != list_id && same_list_marker(previous_kind, kind) {
                out.insert((previous_id.clone(), list_id.clone()));
            }
        }
        previous = Some((list_id, kind));
    }
}

/// Whether two list items wear the same marker.
///
/// Compares the `ListKind` variant, not the value: a checklist's tick state
/// is per item, so a ticked item and an unticked one are the same kind of
/// list and must not be read as two lists.
fn same_list_marker(left: &ListKind, right: &ListKind) -> bool {
    std::mem::discriminant(left) == std::mem::discriminant(right)
}

/// Re-identifies list runs that an edit just made adjacent, so the later run
/// continues the earlier one.
///
/// The inverse of [`list_run_split_reassignments`]: deleting the paragraph
/// between two lists leaves two runs where the user now sees one list, and
/// numbering restarts in the middle of it. `newly_adjacent` is the difference
/// between [`adjacent_list_run_pairs`] taken after the edit and before it, so
/// only runs *this* edit brought together are merged — two adjacent lists
/// that arrived that way (an import that means them to be separate, say) are
/// left exactly as they are by unrelated edits.
pub(crate) fn list_run_merge_reassignments(
    blocks: &[Block],
    newly_adjacent: &BTreeSet<(StableId, StableId)>,
) -> Vec<ListRunReassignment> {
    let mut out = Vec::new();
    collect_list_run_merges(blocks, newly_adjacent, &mut out);
    out
}

fn collect_list_run_merges(
    siblings: &[Block],
    newly_adjacent: &BTreeSet<(StableId, StableId)>,
    out: &mut Vec<ListRunReassignment>,
) {
    // The previous list item's run id as stored, and the run id it ends up
    // on. They differ once a merge has started, which is what lets a chain of
    // three runs collapse onto the first rather than pairwise.
    let mut previous: Option<(StableId, StableId, ListKind)> = None;
    for block in siblings {
        if let BlockKind::Table { rows, .. } = &block.kind {
            for row in rows {
                for cell in &row.cells {
                    collect_list_run_merges(&cell.blocks, newly_adjacent, out);
                }
            }
        }
        let BlockKind::ListItem {
            list_id,
            level,
            kind,
        } = &block.kind
        else {
            previous = None;
            continue;
        };
        let effective = match &previous {
            Some((previous_stored, previous_effective, previous_kind)) => {
                if previous_stored == list_id {
                    // Same run as the item before it: whatever that item
                    // ended up on, this one follows.
                    previous_effective.clone()
                } else if same_list_marker(previous_kind, kind)
                    && newly_adjacent.contains(&(previous_stored.clone(), list_id.clone()))
                {
                    previous_effective.clone()
                } else {
                    list_id.clone()
                }
            }
            None => list_id.clone(),
        };
        if &effective != list_id {
            out.push((block.id.clone(), effective.clone(), *level, *kind));
        }
        previous = Some((list_id.clone(), effective, *kind));
    }
}

/// `SetBlockTextStyle` operations that move each newly adjacent run onto the
/// run it now continues. See [`list_run_merge_reassignments`].
pub(crate) fn list_run_merge_operations(
    blocks: &[Block],
    newly_adjacent: &BTreeSet<(StableId, StableId)>,
) -> Vec<(&'static str, &'static str, opendoc_merge::OperationKind)> {
    list_run_merge_reassignments(blocks, newly_adjacent)
        .into_iter()
        .map(|(block_id, list_id, level, kind)| {
            (
                "set-block-text-style",
                "merge list runs",
                opendoc_merge::OperationKind::SetBlockTextStyle {
                    block_id,
                    style: opendoc_merge::BlockTextStyle::ListItem {
                        list_id,
                        level,
                        kind,
                    },
                },
            )
        })
        .collect()
}
