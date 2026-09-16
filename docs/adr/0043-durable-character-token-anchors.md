# ADR 0043: Durable Character Token Anchors

Status: accepted for staged implementation.

## Context

ADR 0007 resolves text through ephemeral RGA atoms. It then stores the result
back as a string, so a comment or suggestion cannot safely identify the
character selection it was made over. `{ inline_id, offset }` is rejected: it
drifts under a concurrent edit and is not source evidence.

## Decision

`opendoc-core::TextTokenId`, `TextToken`, `TextSequence`, and `TextGap` are
the model vocabulary. A token is either a deterministic legacy baseline
`(document UUID, inline id, ordinal)` or an operation `(actor, sequence,
ordinal)`. A sequence retains predecessor edges and tombstones; its visible
string is only a projection. An annotation endpoint is a pair of token gaps
with `Before`/`After` affinity, not a scalar offset.

The first landed step supplies deterministic legacy materialisation,
validation, and gap projection, without claiming that annotations can yet
author token gaps. The next change must add sequences to `Document`, include
them in signed snapshots, make merge operations mint/tombstone these exact
ids, and then add a token-range anchor variant. Until all those pieces land,
imports retain their existing truthful degraded anchors rather than storing
offsets.

## Consequences

Tombstoned endpoints remain resolvable to their former boundary; only removal
of the owning sequence generation makes an annotation orphaned. Tombstone
compaction requires a separate acknowledged-replica frontier and must refuse
while any annotation, undo target, or retained provenance references a token.
