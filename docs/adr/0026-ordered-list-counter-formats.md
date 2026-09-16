# ADR 0026: Ordered-list counter formats belong to list runs

An ordered-list counter style is source state keyed by the same `(list_id,
level)` pair as its start value. It is not a toolbar-only CSS preference: PDF
painting, HTML, DOCX and ODT must consume the same typed value.

The bounded vocabulary is `decimal`, lower/upper alpha and lower/upper Roman.
Each has exact CSS, OOXML and ODF spellings. A missing entry retains the
historical depth cycle (decimal, lower alpha, lower Roman), and applying that
inherited value removes an explicit entry so equivalent documents serialize
identically. Custom marker strings and arbitrary glyphs are deliberately not
misrepresented as number formats; they require a separate list-marker model.

`SetListFormat` is whole-value LWW per list run and nesting level and its
inverse restores the prior resolved value. This gives undo and concurrent
edits the same unit of intent as list-start changes.
