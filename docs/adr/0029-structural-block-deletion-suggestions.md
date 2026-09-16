# ADR 0029: Structural block-deletion suggestions name one stable block

## Decision

`SuggestionKind::BlockDelete` carries the target block's `StableId`, rather
than an ordinal position, a text range, or a replacement block list.

Accepting a proposed suggestion deletes that exact block with the ordinary
identity-based block deletion primitive and records acceptance provenance.
Rejecting it changes only the suggestion state. If another operation removed
the target before the proposal can be reviewed, anchor repair automatically
rejects it with `auto-rejected:missing-block`; it must not be redirected to an
adjacent block.

The OpenDoc Google-shaped interchange extension encodes this as
`{ "type": "block_delete", "blockId": ... }`. This records an OpenDoc
review proposal faithfully but does not claim that the public Google Docs API
can create a tracked structural deletion. DOCX/ODT continue to name the
existing generic dropped-suggestions warning rather than silently flatten it.

## Context

Text-range suggestions could not safely model an operation such as removing a
paragraph, list item, table, or nested table-cell block. Using its visible text
as the target would make a concurrent insertion or a repeated paragraph select
the wrong structure. Using a live index has the same failure once replicas
insert or delete before it.

## Consequences

This is the first structural tracked-change slice. It provides a review-panel
action for the focused block and a generated API command, while leaving the
document unmodified until acceptance. It deliberately does not imply that
whole-block insertion, moves, table edits, image edits, or generic block-style
changes share its semantics; each needs its own payload and acceptance rule.
