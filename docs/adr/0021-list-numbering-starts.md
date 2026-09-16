# ADR 0021: List numbering starts belong to list runs

Status: accepted.

## Context

An ordered list may continue from an earlier sequence or restart at a chosen
number.  Storing that number on whichever list item is currently first looks
small, but it makes an insertion before that item silently change the source
of truth.  It also gives moves and collaborative re-identification two
competing answers about which item owns a list setting.

## Decision

`Document::list_properties` is keyed by the existing stable `list_id`.  Its
`ListProperties::ordered_starts` map is keyed by nesting level and supplies
the first ordinal for an ordered wrapper; absence means one.  The model allows
only levels 0 through 8 and positive starts.  It does not add custom marker
formats: the supported decimal/alpha/roman cycle remains the shared screen and
paper rule.

HTML writes `start` on the corresponding `ol` and `value` on each item.  Page
layout obtains its ordinal from the same `ListNumbering` call, so the marker
painted into PDF agrees with the browser.

## Consequences

Importers that cannot identify an explicit source restart must leave this map
empty and emit a warning rather than infer one from a displayed glyph.  An
operation/UI for changing the setting is a separate vertical slice: this ADR
first makes persistence and rendering unambiguous, and prevents an ad-hoc
per-block field from becoming a public format.
