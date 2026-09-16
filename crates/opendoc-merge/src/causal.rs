//! Operation identity and causal ordering.
//!
//! See `docs/adr/0007-causal-ordering-and-text-convergence.md`. The short
//! version: an operation may carry the causal context it was generated in, and
//! `causal_order` turns a set of operations into one deterministic total order
//! that is a linear extension of happened-before. Operations without a context
//! fall back to "concurrent with every other actor", which reproduces the
//! pre-ADR `(actor, seq)` ordering exactly.

use crate::Operation;
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap};

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct ActorId(pub String);

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct OperationId {
    pub actor: ActorId,
    pub seq: u64,
}

/// Highest sequence number observed per actor.
///
/// `observed(id)` is true when the operation `id` names is in the causal past
/// this clock describes. Per-actor sequence numbers are dense and monotonic,
/// so one integer per actor is exact — there is no need to name operations
/// individually.
#[derive(Clone, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct VectorClock(pub BTreeMap<ActorId, u64>);

impl VectorClock {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn get(&self, actor: &ActorId) -> u64 {
        self.0.get(actor).copied().unwrap_or(0)
    }

    /// True when `id` is in the causal past this clock describes.
    pub fn observed(&self, id: &OperationId) -> bool {
        id.seq != 0 && self.get(&id.actor) >= id.seq
    }

    pub fn observe(&mut self, id: &OperationId) {
        let entry = self.0.entry(id.actor.clone()).or_insert(0);
        *entry = (*entry).max(id.seq);
    }

    pub fn join(&mut self, other: &VectorClock) {
        for (actor, seq) in &other.0 {
            let entry = self.0.entry(actor.clone()).or_insert(0);
            *entry = (*entry).max(*seq);
        }
    }
}

/// What a replica had already applied when it generated an operation.
///
/// `lamport` orders operations that are causally related (a causal descendant
/// always has a strictly greater timestamp) and is the "last writer" in
/// last-writer-wins. `observed` decides concurrency, which a Lamport
/// timestamp on its own cannot.
#[derive(Clone, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct CausalContext {
    pub lamport: u64,
    pub observed: VectorClock,
}

impl CausalContext {
    /// The context of a replica that has applied exactly `applied`.
    ///
    /// The new operation's Lamport timestamp is one past the highest it has
    /// seen, which is the standard Lamport rule and gives the strict
    /// monotonicity `causal_order` and the text CRDT rely on.
    pub fn observing<'a>(applied: impl IntoIterator<Item = &'a Operation>) -> Self {
        crate::instrument::count_causal_context_built();
        let mut context = Self::default();
        for operation in applied {
            context.observed.observe(&operation.id);
            context.lamport = context.lamport.max(operation.lamport());
        }
        context.lamport += 1;
        context
    }
}

impl Operation {
    pub fn new(id: OperationId, kind: crate::OperationKind) -> Self {
        Self {
            id,
            kind,
            context: None,
        }
    }

    pub fn in_context(id: OperationId, kind: crate::OperationKind, context: CausalContext) -> Self {
        Self {
            id,
            kind,
            context: Some(context),
        }
    }

    pub fn lamport(&self) -> u64 {
        self.context
            .as_ref()
            .map(|context| context.lamport)
            .unwrap_or(0)
    }

    /// The decidable happened-before test: did this operation's author already
    /// have `other` applied when this operation was generated?
    ///
    /// An actor always observes its own earlier operations, context or not.
    /// Everything else needs an explicit vector clock; without one the answer
    /// is "no", which reads every other actor's work as concurrent. That is
    /// the conservative direction — it never claims a dependency that did not
    /// exist.
    pub fn observes(&self, other: &OperationId) -> bool {
        if other.actor == self.id.actor {
            return other.seq < self.id.seq;
        }
        self.context
            .as_ref()
            .is_some_and(|context| context.observed.observed(other))
    }

    /// True when neither operation was generated with knowledge of the other.
    pub fn concurrent_with(&self, other: &Operation) -> bool {
        !self.observes(&other.id) && !other.observes(&self.id)
    }

    fn ready_key(&self) -> (u64, &str, u64) {
        (self.lamport(), self.id.actor.0.as_str(), self.id.seq)
    }
}

/// Order `operations` into one deterministic total order that is a linear
/// extension of happened-before.
///
/// The returned vector holds indices into `operations`. Ties between
/// concurrent operations break on `(lamport, actor, seq)`, so with no causal
/// contexts anywhere the result is exactly the old
/// `BTreeMap<OperationId>` order: each actor's chain is the only constraint,
/// and the ready-set priority drains the lowest actor first.
///
/// `operations` must already be de-duplicated by `OperationId`.
pub(crate) fn causal_order(operations: &[Operation]) -> Vec<usize> {
    let mut by_actor: BTreeMap<&ActorId, Vec<(u64, usize)>> = BTreeMap::new();
    for (index, operation) in operations.iter().enumerate() {
        by_actor
            .entry(&operation.id.actor)
            .or_default()
            .push((operation.id.seq, index));
    }
    for chain in by_actor.values_mut() {
        chain.sort_unstable();
    }

    // Only the *immediate* predecessor per actor needs an edge: observing
    // `(actor, seq)` implies observing every lower sequence from that actor,
    // and those are already chained.
    let latest_at_or_below = |actor: &ActorId, seq: u64| -> Option<usize> {
        let chain = by_actor.get(actor)?;
        let position = chain.partition_point(|(candidate, _)| *candidate <= seq);
        (position > 0).then(|| chain[position - 1].1)
    };

    let mut dependents: Vec<Vec<usize>> = vec![Vec::new(); operations.len()];
    let mut indegree = vec![0usize; operations.len()];
    for (index, operation) in operations.iter().enumerate() {
        let mut predecessors = BTreeSet::new();
        if operation.id.seq > 0 {
            if let Some(previous) = latest_at_or_below(&operation.id.actor, operation.id.seq - 1) {
                predecessors.insert(previous);
            }
        }
        if let Some(context) = &operation.context {
            for (actor, seq) in &context.observed.0 {
                if actor == &operation.id.actor {
                    continue;
                }
                if let Some(previous) = latest_at_or_below(actor, *seq) {
                    predecessors.insert(previous);
                }
            }
        }
        predecessors.remove(&index);
        indegree[index] = predecessors.len();
        for predecessor in predecessors {
            dependents[predecessor].push(index);
        }
    }

    type ReadyEntry<'a> = Reverse<((u64, &'a str, u64), usize)>;
    let mut ready: BinaryHeap<ReadyEntry<'_>> = operations
        .iter()
        .enumerate()
        .filter(|(index, _)| indegree[*index] == 0)
        .map(|(index, operation)| Reverse((operation.ready_key(), index)))
        .collect();

    let mut ordered = Vec::with_capacity(operations.len());
    while let Some(Reverse((_, index))) = ready.pop() {
        ordered.push(index);
        for dependent in std::mem::take(&mut dependents[index]) {
            indegree[dependent] -= 1;
            if indegree[dependent] == 0 {
                ready.push(Reverse((operations[dependent].ready_key(), dependent)));
            }
        }
    }

    if ordered.len() != operations.len() {
        // A cycle can only come from a forged or corrupt vector clock. Fall
        // back to the total order that needs no graph at all rather than
        // dropping operations on the floor.
        let mut remaining: Vec<usize> = (0..operations.len())
            .filter(|index| !ordered.contains(index))
            .collect();
        remaining.sort_by(|left, right| {
            operations[*left]
                .ready_key()
                .cmp(&operations[*right].ready_key())
        });
        ordered.extend(remaining);
    }
    ordered
}
