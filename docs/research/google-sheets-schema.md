# Google Sheets Schema Research

Status: initial complete draft.

## Sources

- Google Sheets API `spreadsheets` resource: https://developers.google.com/workspace/sheets/api/reference/rest/v4/spreadsheets
- Google Sheets `Sheet`, `RowData`, and `CellData`: https://developers.google.com/workspace/sheets/api/reference/rest/v4/spreadsheets/sheets
- Google Sheets `ExtendedValue`: https://developers.google.com/workspace/sheets/api/reference/rest/v4/spreadsheets/other

## Findings

The public Google Sheets API exposes a spreadsheet as workbook properties, sheets, grid data, row data, and cell data. Cells contain a user-entered value, effective/computed value, format, validation, notes, rich text runs, and optional formula state.

The core product risk is not cell storage. It is formula compatibility, recalculation, and collaborative structural edits that shift references.

## Subset Matrix

| Area | Supported v0 | Deferred | Excluded v0 |
| --- | --- | --- | --- |
| Workbook | title, locale, timezone, recalc policy | themes | Google Drive metadata |
| Sheets | sheet ID, title, order, grid dimensions | hidden sheets | data-source sheets |
| Cells | number, string, bool, formula, error | rich text runs in cells | chips and external data values |
| Formatting | bold, italic, number format, alignment, fill | borders, wrapping, rotation | exact Google theme fidelity |
| Structure | sparse cells, rows, columns, frozen panes | protected ranges | Google permissions model |
| Ranges | named ranges, merges | filter views | slicers |
| Formulas | arithmetic, references, ranges, SUM | broad function library | external import functions |
| Charts | metadata placeholder | basic charts | chart rendering v0 |
| Comments | anchored comment metadata | threaded UI | Google account identity model |

## Formula Options

| Option | Strength | Weakness | Recommendation |
| --- | --- | --- | --- |
| Rust-native parser/evaluator | portable, offline, signed deterministic behavior | may require substantial implementation | preferred for v0 subset |
| Embed JS spreadsheet engine | faster compatibility path | weak Rust core ownership, packaging complexity | possible later |
| Delegate to LibreOffice | high compatibility | heavyweight, not local-first friendly | reject for core |
| Store formulas only, compute in frontend | simple storage | inconsistent results, hard signing story | reject for core |

Formula evaluation must be deterministic across platforms. Exact Google Sheets compatibility is a target, but deterministic OpenDoc behavior is more important than copying every edge case during v0.

Formula source is the signed authored content. Cached computed values are performance artifacts and are not part of normal formula-content signatures. Recalculation may be lazy on view, eager in background, or hybrid, depending on measured performance.

Computed values are not stored durably in v0. They may be cached in RAM for view/update performance.

## Row And Column Semantics

Rows and columns should have stable IDs. A1 notation is a projection. Concurrent row insertion orders by CRDT sequence order, then UI projection assigns visible indexes. Formula references are stored in canonical reference form and reprojected after structural edits.

For v0, formulas are signed as user-authored text plus optional computed value cache. Verification treats computed values as cache unless a signed export explicitly includes recalculated values.

## Recommendation

Use `docs/schema/spreadsheet-v0.md` and build a small Rust formula subset before adopting a broad formula engine. Keep cells sparse and address structural edits by stable row/column IDs.

## Rejected Options

- **Store dense row arrays as the canonical model.** Rejected because sparse documents and collaborative row/column edits become expensive.
- **Treat A1 notation as canonical identity.** Rejected because references shift when rows and columns are inserted.
- **Implement all Google Sheets formulas before the editor.** Rejected because formula scope must be incremental.

## Open Risks

- Formula compatibility can dominate the project if scope is not constrained.
- Locale-specific parsing may affect user expectations.
- Collaborative formula-reference repair needs careful testing.

## Completion Evidence

- Supported/deferred/excluded matrix: present.
- Draft schema: `docs/schema/spreadsheet-v0.md`.
- Formula corpus: `examples/formulas/v0.tsv`.
- Row/column semantics: stable IDs plus projection.
