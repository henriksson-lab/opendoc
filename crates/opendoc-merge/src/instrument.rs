//! Counters that say how much whole-document work a merge did.
//!
//! These exist so a test can tell "the batch fold copies the document once"
//! from "once per operation in the batch" — a difference that is invisible in
//! the result and worth seconds on a long document. A wall-clock assertion
//! cannot make that distinction reliably on a shared machine; a count can, and
//! a count compared between two batch sizes needs no constant borrowed from
//! the implementation to be meaningful.
//!
//! The counters are **per thread**, because the test harness gives each test
//! its own thread and a process-wide counter would be whatever the other
//! tests happened to be doing. They are incremented where the work is done,
//! not at the call sites that ask for it, so moving the work somewhere else
//! does not move the count.

use std::cell::Cell;

thread_local! {
    static DOCUMENT_COPIES: Cell<u64> = const { Cell::new(0) };
    static CAUSAL_CONTEXTS_BUILT: Cell<u64> = const { Cell::new(0) };
}

/// What this thread has done since [`take_merge_counts`] last read them.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MergeCounts {
    /// Whole documents copied in order to merge into them: one per
    /// [`crate::merge_operations`] call, plus the scratch copy a caller takes
    /// to fold a batch one operation at a time.
    pub document_copies: u64,
    /// Causal contexts built by scanning a list of applied operations
    /// ([`crate::CausalContext::observing`]). Building one costs a pass over
    /// everything the replica has applied, so doing it once per operation in a
    /// batch is quadratic in the journal.
    pub causal_contexts_built: u64,
}

/// Read this thread's counters and reset them to zero.
pub fn take_merge_counts() -> MergeCounts {
    MergeCounts {
        document_copies: DOCUMENT_COPIES.with(|cell| cell.replace(0)),
        causal_contexts_built: CAUSAL_CONTEXTS_BUILT.with(|cell| cell.replace(0)),
    }
}

/// Record that a whole document was copied so that a merge could be folded
/// into the copy.
///
/// Public because there is exactly one such copy outside this crate — the
/// scratch `opendoc-app` folds a batch into to capture its inverses — and a
/// counter that could not see it would report "one copy per gesture" while the
/// gesture took two. Call it where the copy happens, never where one is
/// requested.
pub fn count_document_copy() {
    DOCUMENT_COPIES.with(|cell| cell.set(cell.get().saturating_add(1)));
}

pub(crate) fn count_causal_context_built() {
    CAUSAL_CONTEXTS_BUILT.with(|cell| cell.set(cell.get().saturating_add(1)));
}
