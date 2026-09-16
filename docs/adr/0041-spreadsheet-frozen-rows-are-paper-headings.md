# ADR 0041: Spreadsheet frozen rows are paper headings

Frozen row and column counts are already durable sheet viewport state, imported
from and exported to Google Sheets `gridProperties`. For the first spreadsheet
paper-fidelity slice, visible leading frozen rows are also repeated at the top
of every vertical PDF page. This gives a Sheets author one durable declaration
whose screen and paper meanings agree; it does not create a second PDF-only
header-row flag that could drift from an imported workbook.

The repeated band contains exactly the leading visible rows selected by
`frozen_rows`, including blank rows. Hidden rows remain absent from paper just
as they are from the grid. Its actual stored row heights consume printable page
space before body pagination. If that band is taller than a page, the exporter
keeps body progress bounded and emits `pdf-spreadsheet-frozen-rows-overflow`;
it must not silently scale or discard rows.

This decision does not claim a complete Google Sheets print contract. Print
area, manually selected repeat ranges, margins, scaling, headers/footers,
breaks, styled cells, frozen columns, and horizontally repeated columns remain
unmodeled. In particular, a future explicit print-header range needs its own
durable sheet print-settings model and must not overload frozen panes.
