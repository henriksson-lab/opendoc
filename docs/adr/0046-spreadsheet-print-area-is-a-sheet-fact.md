# ADR 0046: Bounded spreadsheet paper facts are sheet facts

## Status

Accepted, 2026-09-16.

## Context

The spreadsheet PDF exporter had a useful-grid heuristic: it derived paper
from every visible cell with a value. That is not a print contract. A workbook
can intentionally print a smaller rectangle, include blank cells in that
rectangle, or keep populated working data outside it.

At the same time, a complete Sheets/Excel page-setup model is not a harmless
bag of optional fields. Paper size, orientation, scaling, margins,
headers/footers, repeating titles and manual breaks must agree across the UI,
XLSX, Google JSON, and PDF before any of them can be called durable.

## Decision

`Sheet` owns `SheetPrintSettings`. Its bounded durable fields are an optional
inclusive canonical A1 `print_area` and a closed `SheetPrintOrientation`
(`landscape` or `portrait`). Landscape is the compatibility default.

* Missing means the PDF continues to derive the used visible range.
* Present means rows and columns inside that rectangle are paper candidates,
  including blank cells. Hidden axes still remain hidden.
* The range must already be canonical and both endpoints must be in the
  sheet's grid. `Sheet::set_print_area` normalizes a supplied value and stages
  validation before changing the durable setting.
* The PDF renderer reads precisely this state before its pagination pass. It
  does not scale the rectangle to fit a page; ordinary axis pagination retains
  stored pixel geometry.
* Orientation changes the actual Letter PDF media box for that sheet. It is
  not a rotate-after-render approximation: pagination calculates against the
  selected box, so portrait may introduce a different column break.
* The app exposes orientation through the generated command contract and a
  Data-menu dialog. Its typed, last-writer-wins journal operation is replayed
  with the rest of the workbook rather than becoming a UI-local preference.

The PDF emits `pdf-spreadsheet-print-settings-unavailable` whenever it uses a
non-default bounded paper fact (a print area or portrait orientation). That
warning names the deliberately unsupported paper settings so the supported
subset does not falsely imply full Sheets print fidelity.

## Consequences

The setting is part of the serialized workbook and the app's generated DTO,
so imports/repositories can carry it without a PDF-local side channel. This
ADR does **not** claim XLSX/Google page-setup round-trip support. Those
integrations must be added with their own explicit coverage rather than
treating absent settings as defaults.
