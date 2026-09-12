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
struct Atom {
    ch: char,
    /// Index into `edits` of the insert that produced this character, or
    /// `None` for a character that was already in the merge base.
    inserted_by: Option<usize>,
    /// Indices into `edits` of every delete covering this character. A
    /// non-empty set is a tombstone: the character stays in the sequence so
    /// that concurrent inserts anchored on it still resolve, but it is not
    /// rendered.
    deleted_by: Vec<usize>,
}

/// Resolve `base` plus every edit in `edits` into the converged run text.
///
/// `edits` is normally supplied in causal order, but the result does not
/// depend on that: placement uses `RunEdit::rank` and each edit's own context,
/// both of which are properties of the operation set. Feeding concurrent edits
/// in either order gives the same string — which is the property the whole
/// design exists for, and which `crate` tests assert directly.
pub(crate) fn resolve_run(base: &str, edits: &[RunEdit]) -> String {
    if edits.is_empty() {
        return base.to_string();
    }
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
        .iter()
        .filter(|atom| atom.deleted_by.is_empty())
        .map(|atom| atom.ch)
        .collect()
}
