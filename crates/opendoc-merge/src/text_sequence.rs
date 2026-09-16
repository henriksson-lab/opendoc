//! The sequence CRDT that makes concurrent character edits converge.
//!
//! See `docs/adr/0007-causal-ordering-and-text-convergence.md`. Offsets are
//! never transformed. Each character of a run is given an identity — "present
//! in the merge base", or "the n-th character inserted by operation X" — and
//! an operation's offset is resolved against the subsequence that was visible
//! *in that operation's own causal context*, which is what the offset meant
//! when it was written. Deletes are tombstones, so an insert anchored on a
//! concurrently deleted character still has somewhere to go.
//!
//! The identities exist only for the duration of a merge. The run goes back
//! into `Inline::Text`/`Inline::Link` as a plain `String`, so nothing about
//! the stored, signed document changes.

use crate::causal::{CausalContext, OperationId};
use crate::inline_ops::inline_id as inline_id_of;
use crate::operation::{Operation, OperationKind};
use opendoc_core::{Block, BlockKind, Inline, StableId, TextSequence, TextToken, TextTokenId};
use std::collections::BTreeMap;

/// One character operation, already narrowed to a single run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum RunEditKind {
    /// Insert `text` after the `offset`-th visible character.
    Insert { offset: usize, text: String },
    /// Tombstone the visible characters in `[start, end)`.
    Delete { start: usize, end: usize },
}

#[derive(Clone, Debug)]
pub(crate) struct RunEdit {
    /// Position of the owning operation in the merge's causal order. Strictly
    /// increasing along causal edges, which is what the RGA placement rule
    /// needs from a timestamp.
    pub rank: usize,
    pub id: OperationId,
    pub context: Option<CausalContext>,
    pub kind: RunEditKind,
}

impl RunEdit {
    /// Did this edit's author already have the operation behind `other`
    /// applied? Mirrors `Operation::observes`; the edits carry a copy of the
    /// identity and context rather than borrowing the operations, because the
    /// merge pass hands them over one run at a time.
    fn observes(&self, other: &OperationId) -> bool {
        if other.actor == self.id.actor {
            return other.seq < self.id.seq;
        }
        self.context
            .as_ref()
            .is_some_and(|context| context.observed.observed(other))
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Atom {
    pub(crate) ch: char,
    /// Index into `edits` of the insert that produced this character, or
    /// `None` for a character that was already in the merge base.
    pub(crate) inserted_by: Option<usize>,
    /// Indices into `edits` of every delete covering this character. A
    /// non-empty set is a tombstone: the character stays in the sequence so
    /// that concurrent inserts anchored on it still resolve, but it is not
    /// rendered.
    pub(crate) deleted_by: Vec<usize>,
}

impl Atom {
    /// Whether this character is rendered. A tombstoned atom keeps its place
    /// in the sequence so concurrent inserts anchored on it still resolve.
    pub(crate) fn visible(&self) -> bool {
        self.deleted_by.is_empty()
    }
}

/// Resolve `base` plus every edit in `edits` into the converged run text.
///
/// `edits` is normally supplied in causal order, but the result does not
/// depend on that: placement uses `RunEdit::rank` and each edit's own context,
/// both of which are properties of the operation set. Feeding concurrent edits
/// in either order gives the same string — which is the property the whole
/// design exists for, and which `crate` tests assert directly.
#[cfg(test)]
pub(crate) fn resolve_run(base: &str, edits: &[RunEdit]) -> String {
    if edits.is_empty() {
        return base.to_string();
    }
    resolve_run_atoms(base, edits)
        .iter()
        .filter(|atom| atom.visible())
        .map(|atom| atom.ch)
        .collect()
}

/// [`resolve_run`] stopping one step short of collapsing the sequence back to
/// a `String`.
///
/// The atom vector is what carries the character identities ADR 0007 derives
/// rather than stores: which edit inserted a character, and which edits
/// tombstoned it. Collapsing it is one `filter`; *inverting* an edit needs the
/// identities themselves, because "where is the text this operation inserted
/// **now**" is not a question an offset can answer once other actors have
/// edited the same run. See
/// `docs/adr/0017-collaborative-undo-as-inverse-operations.md`.
pub(crate) fn resolve_run_atoms(base: &str, edits: &[RunEdit]) -> Vec<Atom> {
    let mut atoms: Vec<Atom> = base
        .chars()
        .map(|ch| Atom {
            ch,
            inserted_by: None,
            deleted_by: Vec::new(),
        })
        .collect();

    for (index, edit) in edits.iter().enumerate() {
        // The characters this edit's author could see when it wrote its
        // offsets: base characters, plus characters inserted by operations it
        // observed, minus characters deleted by operations it observed.
        let visible: Vec<usize> = (0..atoms.len())
            .filter(|position| {
                let atom = &atoms[*position];
                let inserted_visible = match atom.inserted_by {
                    None => true,
                    Some(other) => edit.observes(&edits[other].id),
                };
                inserted_visible
                    && !atom
                        .deleted_by
                        .iter()
                        .any(|other| edit.observes(&edits[*other].id))
            })
            .collect();

        match &edit.kind {
            RunEditKind::Insert { offset, text } => {
                if text.is_empty() {
                    continue;
                }
                // Convert the offset into an anchor exactly once. From here on
                // the insert is positioned relative to a character identity,
                // so later edits can shift the text freely without moving it.
                let offset = (*offset).min(visible.len());
                let mut at = if offset == 0 {
                    0
                } else {
                    visible[offset - 1] + 1
                };
                // RGA: step over characters inserted by operations this edit
                // did not observe whose rank sorts after it, so two concurrent
                // inserts at one anchor land in rank order whichever arrives
                // first. Descendants of a skipped insert always outrank it, so
                // they are skipped with it and no run is ever split.
                while at < atoms.len() {
                    match atoms[at].inserted_by {
                        Some(other)
                            if edits[other].rank > edit.rank
                                && !edit.observes(&edits[other].id) =>
                        {
                            at += 1;
                        }
                        _ => break,
                    }
                }
                let inserted: Vec<Atom> = text
                    .chars()
                    .map(|ch| Atom {
                        ch,
                        inserted_by: Some(index),
                        deleted_by: Vec::new(),
                    })
                    .collect();
                atoms.splice(at..at, inserted);
            }
            RunEditKind::Delete { start, end } => {
                let start = (*start).min(visible.len());
                let end = (*end).min(visible.len());
                for position in &visible[start..end.max(start)] {
                    let deleted_by = &mut atoms[*position].deleted_by;
                    if !deleted_by.contains(&index) {
                        deleted_by.push(index);
                    }
                }
            }
        }
    }

    atoms
}

/// Resolve character edits while retaining the durable token source that
/// produced the visible string.  The temporary atom CRDT above remains the
/// compatibility oracle for legacy offsets; this spelling gives every new
/// scalar an operation-derived id and turns deletes into persisted
/// tombstones.
pub(crate) fn resolve_text_sequence(base: &TextSequence, edits: &[RunEdit]) -> TextSequence {
    #[derive(Clone)]
    struct DurableAtom {
        token: TextToken,
        inserted_by: Option<usize>,
        /// A delete that belongs to the merge base.  New operations never see
        /// it, whereas a delete from this `edits` set remains visible to an
        /// operation that did not causally observe it.
        tombstoned_before: bool,
        deleted_by: Vec<usize>,
    }

    let mut atoms = base
        .tokens
        .iter()
        .cloned()
        .map(|token| DurableAtom {
            inserted_by: None,
            tombstoned_before: token.tombstoned,
            deleted_by: Vec::new(),
            token,
        })
        .collect::<Vec<_>>();

    for (index, edit) in edits.iter().enumerate() {
        let visible = (0..atoms.len())
            .filter(|position| {
                let atom = &atoms[*position];
                let inserted_visible = match atom.inserted_by {
                    None => true,
                    Some(other) => edit.observes(&edits[other].id),
                };
                inserted_visible
                    // A sequence carried in the merge base has already
                    // resolved its historic deletes.  Its tombstones stay in
                    // the physical order for anchors, but must not reappear
                    // in the offsets of a new operation; `resolve_run` sees
                    // only the base's visible string for exactly this reason.
                    && !atom.tombstoned_before
                    && !atom
                        .deleted_by
                        .iter()
                        .any(|other| edit.observes(&edits[*other].id))
            })
            .collect::<Vec<_>>();
        match &edit.kind {
            RunEditKind::Insert { offset, text } => {
                if text.is_empty() {
                    continue;
                }
                let offset = (*offset).min(visible.len());
                let mut at = if offset == 0 {
                    0
                } else {
                    visible[offset - 1] + 1
                };
                while at < atoms.len() {
                    match atoms[at].inserted_by {
                        Some(other)
                            if edits[other].rank > edit.rank
                                && !edit.observes(&edits[other].id) =>
                        {
                            at += 1;
                        }
                        _ => break,
                    }
                }
                let mut predecessor = at
                    .checked_sub(1)
                    .map(|position| atoms[position].token.id.clone());
                let inserted = text
                    .chars()
                    .enumerate()
                    .map(|(ordinal, scalar)| {
                        let id = TextTokenId::Operation {
                            actor: edit.id.actor.0.clone(),
                            sequence: edit.id.seq,
                            ordinal: ordinal as u32,
                        };
                        let token = TextToken {
                            id: id.clone(),
                            predecessor: predecessor.clone(),
                            scalar,
                            tombstoned: false,
                        };
                        predecessor = Some(id);
                        DurableAtom {
                            token,
                            inserted_by: Some(index),
                            tombstoned_before: false,
                            deleted_by: Vec::new(),
                        }
                    })
                    .collect::<Vec<_>>();
                atoms.splice(at..at, inserted);
            }
            RunEditKind::Delete { start, end } => {
                let start = (*start).min(visible.len());
                let end = (*end).min(visible.len());
                for position in &visible[start..end.max(start)] {
                    if !atoms[*position].deleted_by.contains(&index) {
                        atoms[*position].deleted_by.push(index);
                    }
                }
            }
        }
    }

    TextSequence {
        tokens: atoms
            .into_iter()
            .map(|atom| TextToken {
                tombstoned: atom.tombstoned_before || !atom.deleted_by.is_empty(),
                ..atom.token
            })
            .collect(),
    }
}

/// Collect every character operation in `ordered` into per-run edit lists, and
/// note the rank at which any operation rewrote a run wholesale.
///
/// One pass over the causal order, shared by the merge and by the inverse
/// computation, so the two cannot disagree about which operations are
/// offset-addressed or about which whole-run write resets a run's base.
///
/// A *reset* is an operation that writes a run's whole text: `UpdateInlineText`,
/// and every operation whose payload carries the run itself — the
/// `InsertInline` that created it, and an `InsertBlock`/`InsertTableRow`/
/// `InsertTableCell` whose blocks contain it. Character operations ordered
/// before a reset lost to it, which is the same last-write-wins the sequential
/// path had (ADR 0007, "Consequences"). Undo is what makes the block and table
/// payloads matter: re-inserting a deleted block restores its runs' text, so
/// the character operations that shaped that text before the delete must not
/// be replayed against the restored copy.
pub(crate) fn collect_text_run_edits(
    ordered: &[&Operation],
) -> (BTreeMap<StableId, Vec<RunEdit>>, BTreeMap<StableId, usize>) {
    let mut edits: BTreeMap<StableId, Vec<RunEdit>> = BTreeMap::new();
    let mut resets: BTreeMap<StableId, usize> = BTreeMap::new();
    for (rank, operation) in ordered.iter().enumerate() {
        match &operation.kind {
            OperationKind::InsertText {
                inline_id,
                offset,
                text,
            } => {
                if !text.is_empty() {
                    edits.entry(inline_id.clone()).or_default().push(RunEdit {
                        rank,
                        id: operation.id.clone(),
                        context: operation.context.clone(),
                        kind: RunEditKind::Insert {
                            offset: *offset,
                            text: text.clone(),
                        },
                    });
                }
            }
            OperationKind::DeleteText {
                inline_id,
                start,
                end,
            } => {
                if end > start {
                    edits.entry(inline_id.clone()).or_default().push(RunEdit {
                        rank,
                        id: operation.id.clone(),
                        context: operation.context.clone(),
                        kind: RunEditKind::Delete {
                            start: *start,
                            end: *end,
                        },
                    });
                }
            }
            other => {
                for run in runs_written_wholesale(other) {
                    resets.insert(run, rank);
                }
            }
        }
    }
    (edits, resets)
}

/// The runs `kind` writes wholesale — the ones whose base a merge resets at
/// this operation's rank.
///
/// ADR 0007 named `UpdateInlineText` and the `InsertInline` that created a
/// run; ADR 0017 added every payload that *carries* the run, because
/// re-inserting a deleted block restores a snapshot of its text and the
/// character operations that shaped that text before the delete must not be
/// replayed onto the restored copy.
///
/// It is a function of one operation on purpose. Which runs a write covers is
/// a property of its payload; *whether a particular character operation loses
/// to it* is a property of the whole set, and that is the question
/// [`crate::discarded_by_a_later_whole_run_write`] answers out of this.
pub(crate) fn runs_written_wholesale(kind: &OperationKind) -> Vec<StableId> {
    match kind {
        OperationKind::UpdateInlineText { inline_id, .. } => vec![inline_id.clone()],
        OperationKind::InsertInline { inline, .. } => vec![inline_id_of(inline).clone()],
        OperationKind::InsertBlock { block, .. } => runs_in_blocks(std::slice::from_ref(block)),
        OperationKind::InsertTableRow { row, .. } => row
            .cells
            .iter()
            .flat_map(|cell| runs_in_blocks(&cell.blocks))
            .collect(),
        OperationKind::InsertTableCell { cell, .. } => runs_in_blocks(&cell.blocks),
        _ => Vec::new(),
    }
}

/// Whether `kind` addresses its target by an offset into a text run rather
/// than by identity. True for exactly the two character operations, which is
/// why they are the only ones whose inverse cannot be captured when the
/// operation is written.
pub(crate) fn is_offset_addressed(kind: &OperationKind) -> bool {
    matches!(
        kind,
        OperationKind::InsertText { .. } | OperationKind::DeleteText { .. }
    )
}

/// Every editable text run id reachable from `blocks`, including the ones
/// nested inside table cells.
pub(crate) fn runs_in_blocks(blocks: &[Block]) -> Vec<StableId> {
    let mut found = Vec::new();
    for block in blocks {
        for inline in &block.content {
            match inline {
                Inline::Text { id, .. } | Inline::Link { id, .. } => found.push(id.clone()),
                _ => {}
            }
        }
        if let BlockKind::Table { rows, .. } = &block.kind {
            for row in rows {
                for cell in &row.cells {
                    found.extend(runs_in_blocks(&cell.blocks));
                }
            }
        }
    }
    found
}
