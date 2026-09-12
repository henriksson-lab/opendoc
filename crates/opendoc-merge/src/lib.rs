//! Merge and rebase over typed document operations.
//!
//! The crate is DOM-independent: it takes a merge base [`Document`] plus one
//! operation stream per actor and folds them into a single document plus the
//! warnings that explain every degradation. Ordering is causal
//! (`docs/adr/0007-causal-ordering-and-text-convergence.md`), character edits
//! converge through the sequence CRDT in [`text_sequence`], and every domain
//! (comments, suggestions, citations, tables, marks) has its own apply and
//! repair rules in the module named after it.

mod anchors;
mod apply;
mod block_edit;
mod blocks;
mod causal;
mod citations;
mod footnotes;
mod inline_edit;
mod inline_ops;
mod marks;
mod merge;
mod operation;
mod suggestions;
mod tables;
mod text_sequence;
mod validate;

pub use causal::{ActorId, CausalContext, OperationId, VectorClock};
pub use inline_edit::byte_index_for_char_offset;
pub use merge::{merge_operations, MergeResult};
pub use operation::{BlockTextStyle, Operation, OperationKind};

#[cfg(test)]
mod test_support;

#[cfg(test)]
mod block_property_tests;
#[cfg(test)]
mod causal_convergence_tests;
#[cfg(test)]
mod citation_cache_tests;
#[cfg(test)]
mod citation_tests;
#[cfg(test)]
mod comment_tests;
#[cfg(test)]
mod inline_edit_tests;
#[cfg(test)]
mod mark_range_tests;
#[cfg(test)]
mod operation_tests;
#[cfg(test)]
mod replay_fuzz_tests;
#[cfg(test)]
mod structured_inline_tests;
#[cfg(test)]
mod suggestion_tests;
#[cfg(test)]
mod table_grid_tests;
#[cfg(test)]
mod table_tests;
