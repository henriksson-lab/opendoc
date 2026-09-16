# ADR 0027: Bullet markers belong to list runs, not counters

Status: accepted.

## Context

An unordered list may choose a filled circle, hollow circle, or square at one
nested level. A counter format cannot represent this: bullets have no ordinal.

## Decision

`Document::list_properties` stores `bullet_markers`, keyed by stable `list_id`
and nesting level alongside—but distinct from—ordered starts and counter
formats. Absence retains the historical disc/circle/square depth cycle;
inherited values are canonicalized away. `SetListBulletMarker` is a whole
run-level operation with LWW merge and inverse semantics.

The initial vocabulary is disc, circle and square. All are in the bundled PDF
font and map exactly to CSS, Google `glyphSymbol`, Word `lvlText`, and ODF
`text:bullet-char`.

The vocabulary additionally admits a literal custom marker of at most sixteen
safe, visible Unicode scalar values. It cannot name a font or asset, contain a
control character, quote, slash, or HTML-significant character. HTML emits it
as a fully CSS-escaped literal marker, while paper paints the literal through
the existing bundled-font coverage check. Google, DOCX and ODT carry it in
their native literal glyph fields. A PDF glyph outside that checked subset is
reported by the existing named `pdf-glyph-outside-bundled-font` degradation;
font- or asset-backed marker schemes remain unsupported.
