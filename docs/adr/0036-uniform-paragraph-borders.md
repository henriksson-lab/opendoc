# ADR 0036: Uniform paragraph borders are one typed property

`BlockProperty::Border(CellBorder)` is one uniform frame around a paragraph's
border box. It has the same validated line vocabulary as a cell border
(`none`, solid, dashed, dotted, double; 0–6 pt; opaque sRGB), but it is a
separate block-property slot and never participates in a table's collapsed
border grid.

`None` means inherit. `CellBorder::none()` is an explicit no-frame value, so
it can override an imported/style default and still merge and invert under the
ordinary per-property LWW rule.

HTML and ODF's `fo:border` have exact uniform spellings. DOCX imports and
exports `w:pBdr` only when top, left, bottom and right are identical supported
unpadded lines; between/bar rules, per-edge differences, shadows, theme/auto
colours and non-twip widths are named degradations. Google Docs is similarly
accepted only when all four API edges are identical supported zero-padding
lines and no `borderBetween` is present. Google has no exact `double` or
explicit-none spelling, so those exports warn rather than impersonating a
different frame.

Per-edge paragraph borders, border-between, padding, art borders and
pagination-sensitive line frames remain deliberately out of model scope.
