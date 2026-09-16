# ADR 0034: Date chips are atomic canonical calendar days

## Decision

`Inline::DateChip` stores one stable inline ID and one canonical
`YYYY-MM-DD` calendar date. It is deliberately not a locale label, timestamp,
or free-form smart-chip payload: each has distinct time-zone, formatting, and
merge semantics.

`UpdateDateChip` replaces the whole validated calendar date and has a direct
inverse for undo. Character operations cannot edit a date chip. Leap days are
validated by the model, so malformed imported or concurrent values do not
become durable document state.

The renderer projects an atomic labelled HTML `time` element; layout, PDF,
search, and plain-text export use the same ISO value. Google-shaped OpenDoc
JSON preserves the typed data in `opendocDateChip`; native Google JSON has no
portable public date-chip shape. DOCX and ODT deliberately degrade to ISO text
with named warnings.

## Consequences

This is a bounded date primitive, not a general smart-chip framework. File,
event, place, people, building blocks and document tabs remain separate model
designs rather than untyped fields bolted onto dates.
