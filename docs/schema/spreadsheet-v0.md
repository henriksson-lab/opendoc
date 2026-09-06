# Spreadsheet Schema v0

Status: draft.

Purpose: define a sparse, collaboration-friendly workbook model that covers the practical Google Sheets subset before charts, data sources, and advanced pivots.

## Top-Level Shape

```json
{
  "schema": "opendoc.spreadsheet.v0",
  "id": "sheetbook_01",
  "properties": {
    "title": "Budget",
    "locale": "en-US",
    "timezone": "Europe/Stockholm",
    "recalc": "on_change"
  },
  "sheets": [],
  "named_ranges": [],
  "comments": []
}
```

## Cell Model

Cells are stored sparsely by stable sheet ID, row ID, and column ID. UI
coordinates are projections. Sheet titles are editable source metadata; renames
preserve sheet IDs and therefore preserve cell, formula, and named-range
anchors.

Sheets can be removed from current workbook state by stable sheet ID while
history retains the delete operation and prior sheet content. Deleting a sheet
also removes current named ranges whose `sheet_id` points at the deleted sheet.
The visible workbook must retain at least one sheet; replay skips duplicate or
racing deletes that would otherwise remove the final sheet so merged histories
remain openable.

Rows and columns have explicit lifecycle operations by stable sheet ID and
visible label. Adding an axis creates durable axis metadata even before cells
exist on that row or column. Deleting an axis removes current cells on that axis
and removes named ranges that intersect it. v0 does not implement positional
spreadsheet insertion semantics or formula-source shifting; formulas that still
refer to deleted cells degrade through deterministic formula errors or range
evaluation rules.

Sheet viewport metadata includes `frozen_rows` and `frozen_columns`, mapped to
Google Sheets `gridProperties.frozenRowCount` and `frozenColumnCount` during
API-shaped import/export. Counts are signed source state and are clamped to the
current visible row/column counts.

Cell comments are signed source state on sparse cells. Each comment has a
stable comment ID, author, body, and deleted flag. Normal views hide deleted
cell comments; audit/recovery views and operation history retain enough data to
show what changed. A retained deleted cell comment can be restored by stable ID
as an operation-backed source change. Deleting a row, column, or sheet removes
those comments from current state with the deleted cell, while the operation
history retains prior content.

Google Sheets-shaped import maps native `CellData.note` into a simple signed
cell comment with a deterministic `note-<address>` ID and `Google Sheets note`
author. Full OpenDoc cell-comment metadata, including author IDs and deleted
comment retention, is preserved through an explicit `opendocCellComments`
extension on the Google-shaped cell object. Export emits both a native `note`
for the first visible note-like comment and the full extension array for
round-trip fidelity.

Cell validations are optional signed source state on sparse cells. v0 supports
list validations with bounded string values, a `strict` flag for hard vs warning
semantics, and a `show_dropdown` projection hint. The validation `kind` remains
explicit so number, text, range, and formula validations can use the same cell
operation shape later. Clearing a validation removes it from current state while
the operation history retains the change.

Merged cell ranges are signed sheet source state. Each merge has a stable ID and
an A1 range. v0 rejects single-cell and overlapping merge ranges during
interactive edits. Replay treats duplicate merge ranges as already applied and
removes affected current merge metadata when rows, columns, or sheets are
deleted.

Basic filters are signed sheet source state. v0 supports one filter range per
sheet and maps it to Google Sheets `basicFilter`. Filter criteria support
`text_contains`, `text_equals`, `number_greater`, `number_less`, and
`number_equal` on filter-range columns. Sort specs support filter-range columns
with ascending or descending order. Clearing a filter removes it from current
state while operation history retains the change.

Protected ranges are signed sheet source state. v0 supports only warning-only
protected range metadata with a stable ID, A1 range, description, and
`warning_only: true`. Local/serverless modes do not enforce edit permissions;
command or Google Sheets import inputs that ask for enforced protected ranges
are downgraded to warning-only metadata with an explicit warning. This preserves
provenance, Google Sheets-shaped interchange, and future service-side
permission checks without adding false local permission semantics. Deleting
rows, columns, or sheets removes affected current protected-range metadata
while operation history retains the change.

```json
{
  "id": "cell_r1_c1",
  "row": "row_1",
  "column": "col_1",
  "user_value": { "kind": "formula", "value": "=SUM(B1:B3)" },
  "computed_value": { "kind": "number", "value": 12 },
  "format": { "number": "decimal", "bold": false },
  "validation": {
    "kind": "list",
    "values": ["Yes", "No"],
    "strict": false,
    "show_dropdown": true
  },
  "comments": [
    {
      "id": "cell_comment_01",
      "author": "Reviewer",
      "body": "Check source data",
      "deleted": false
    }
  ]
}
```

## Supported Values

- `empty`
- `number`
- `string`
- `bool`
- `formula`
- `error`

Dates and times are typed formatting over numbers in v0, matching the Google Sheets API convention of serial-number values.

## Minimal Example

```json
{
  "schema": "opendoc.spreadsheet.v0",
  "id": "sheetbook_minimal",
  "properties": {
    "title": "Minimal",
    "locale": "en-US",
    "timezone": "UTC",
    "recalc": "on_change"
  },
  "sheets": [
    {
      "id": "sheet_1",
      "title": "Sheet1",
      "frozen_rows": 1,
      "frozen_columns": 0,
      "grid": {
        "rows": [{ "id": "row_1" }, { "id": "row_2" }],
        "columns": [{ "id": "col_a" }, { "id": "col_b" }],
        "frozen_rows": 1,
        "frozen_columns": 0
      },
      "cells": {
        "row_1:col_a": {
          "user_value": { "kind": "string", "value": "Item" },
          "computed_value": { "kind": "string", "value": "Item" },
          "format": { "bold": true }
        },
        "row_2:col_a": {
          "user_value": { "kind": "string", "value": "Apples" },
          "computed_value": { "kind": "string", "value": "Apples" }
        },
        "row_2:col_b": {
          "user_value": { "kind": "number", "value": 3 },
          "computed_value": { "kind": "number", "value": 3 }
        }
      },
      "merges": [{ "id": "merge-a1-b1", "range": "A1:B1" }],
      "filters": [{
        "id": "basic-filter",
        "range": "A1:B2",
        "criteria": [{ "column": "A", "condition": "text_contains", "value": "Apple" }],
        "sort_specs": [{ "column": "B", "descending": false }]
      }],
      "protected_ranges": [
        {
          "id": "protected-a1-b2",
          "range": "A1:B2",
          "description": "Warning-only protected range",
          "warning_only": true
        }
      ],
      "validations": []
    }
  ],
  "named_ranges": [],
  "comments": []
}
```

## Formula Example

```json
{
  "schema": "opendoc.spreadsheet.v0",
  "id": "sheetbook_formula",
  "properties": {
    "title": "Formula",
    "locale": "en-US",
    "timezone": "UTC",
    "recalc": "on_change"
  },
  "sheets": [
    {
      "id": "sheet_1",
      "title": "Sheet1",
      "grid": {
        "rows": [{ "id": "row_1" }, { "id": "row_2" }, { "id": "row_3" }],
        "columns": [{ "id": "col_a" }, { "id": "col_b" }]
      },
      "cells": {
        "row_1:col_b": {
          "user_value": { "kind": "number", "value": 5 },
          "computed_value": { "kind": "number", "value": 5 }
        },
        "row_2:col_b": {
          "user_value": { "kind": "number", "value": 7 },
          "computed_value": { "kind": "number", "value": 7 }
        },
        "row_3:col_b": {
          "user_value": { "kind": "formula", "value": "=SUM(B1:B2)" },
          "computed_value": { "kind": "number", "value": 12 },
          "dependencies": ["sheet_1!B1", "sheet_1!B2"]
        }
      },
      "merges": [],
      "filters": [],
      "protected_ranges": [],
      "validations": []
    }
  ],
  "named_ranges": [],
  "comments": []
}
```

## Initial Formula Corpus

The initial formula test corpus lives at `examples/formulas/v0.tsv` and covers:

- numeric literals
- arithmetic precedence
- cell references
- absolute A1 references, such as `$B$2`, `$B2`, and `B$2`
- rectangular ranges
- sheet-title or sheet-ID qualified references, such as `Raw!B2`
- quoted Google-style sheet-title references, such as `'Raw Data'!B2`
- escaped apostrophes in quoted sheet titles, such as `'Bob''s Data'!A1`
- unquoted stable sheet-ID references, such as `sheet-2!A1`
- sheet-title or sheet-ID qualified ranges, such as `Raw!B1:B2` and `'Raw Data'!$B$1:$B$2`
- `SUM`
- `AVERAGE`
- `MIN`
- `MAX`
- `COUNT`
- `PRODUCT`
- `MEDIAN`
- `MODE`
- `SUMSQ`
- `STDEV`
- `STDEVP`
- `STDEV.S`
- `STDEV.P`
- `VAR`
- `VARP`
- `VAR.S`
- `VAR.P`
- `AVEDEV`
- `DEVSQ`
- `GEOMEAN`
- `HARMEAN`
- `LARGE`
- `SMALL`
- `CORREL`
- `PEARSON`
- `COVAR`
- `COVARIANCE.P`
- `COVARIANCE.S`
- `SLOPE`
- `INTERCEPT`
- `RSQ`
- `FORECAST`
- `PERCENTILE`
- `PERCENTILE.INC`
- `PERCENTILE.EXC`
- `QUARTILE`
- `QUARTILE.INC`
- `QUARTILE.EXC`
- `PERCENTRANK`
- `PERCENTRANK.INC`
- `PERCENTRANK.EXC`
- `RANK`
- `RANK.EQ`
- `RANK.AVG`
- `COMBIN`
- `COMBINA`
- `PERMUT`
- `PERMUTATIONA`
- `FACT`
- `FACTDOUBLE`
- `GCD`
- `LCM`
- `STANDARDIZE`
- `FISHER`
- `FISHERINV`
- `DELTA`
- `GESTEP`
- `ERF`
- `ERFC`
- `TRUE`
- `FALSE`
- `IF`
- `IFERROR`
- `IFNA`
- `ISERROR`
- `ISERR`
- `ISNA`
- `ISNUMBER`
- `ISBLANK`
- `ISFORMULA`
- `ISTEXT`
- `ISNONTEXT`
- `ISLOGICAL`
- `NA`
- `N`
- `VALUE`
- `NUMBERVALUE`
- `LEN`
- `FIND`
- `SEARCH`
- `EXACT`
- `LEFT`
- `RIGHT`
- `MID`
- `CONCAT`
- `CONCATENATE`
- `LOWER`
- `UPPER`
- `TRIM`
- `SUBSTITUTE`
- `REPLACE`
- `REPT`
- `TO_TEXT`
- `CHAR`
- `UNICHAR`
- `CODE`
- `UNICODE`
- `JOIN`
- `TEXTJOIN`
- `DATE`
- `DATEVALUE`
- `YEAR`
- `MONTH`
- `DAY`
- `TIME`
- `TIMEVALUE`
- `HOUR`
- `MINUTE`
- `SECOND`
- `DAYS`
- `DATEDIF`
- `NETWORKDAYS`
- `WORKDAY`
- `ISOWEEKNUM`
- `WEEKDAY`
- `EDATE`
- `EOMONTH`
- `INDEX`
- `MATCH`
- `HLOOKUP`
- `VLOOKUP`
- `XLOOKUP`
- `ROW`
- `COLUMN`
- `ROWS`
- `COLUMNS`
- `ISEVEN`
- `ISODD`
- `COUNTA`
- `COUNTBLANK`
- `COUNTIF`
- `SUMIF`
- `COUNTIFS`
- `SUMIFS`
- `AVERAGEIF`
- `AVERAGEIFS`
- `MINIFS`
- `MAXIFS`
- `AND`
- `OR`
- `NOT`
- `IFS`
- `CHOOSE`
- `SWITCH`
- `EQ`
- `NE`
- `GT`
- `GTE`
- `LT`
- `LTE`
- `ADD`
- `MINUS`
- `MULTIPLY`
- `DIVIDE`
- `POW`
- `UMINUS`
- `UNARY_PERCENT`
- comparison operators `=`, `<>`, `>`, `>=`, `<`, and `<=`
- `ABS`
- `SQRT`
- `ROUND`
- `TRUNC`
- `ROUNDUP`
- `ROUNDDOWN`
- `FLOOR`
- `CEILING`
- `MROUND`
- `POWER`
- `MOD`
- `QUOTIENT`
- `EVEN`
- `ODD`
- `INT`
- `SIGN`
- `LN`
- `LOG`
- `LOG10`
- `EXP`
- `SIN`
- `COS`
- `TAN`
- `SEC`
- `CSC`
- `COT`
- `SINH`
- `COSH`
- `TANH`
- `SECH`
- `CSCH`
- `COTH`
- `ASINH`
- `ACOSH`
- `ATANH`
- `ASIN`
- `ACOS`
- `ATAN`
- `ACOT`
- `ACOTH`
- `ATAN2`
- `RADIANS`
- `DEGREES`
- `PI`
- `E`
- stale or missing references

## Named Ranges

Named ranges are workbook-level source records with stable IDs, normalized
names, sheet IDs, and A1 ranges. Formula evaluation expands live named ranges
into concrete cell dependencies. Deleting a named range removes the source
record and re-evaluates formulas; formulas that still reference the deleted name
produce deterministic error values instead of retaining stale computed caches.

## Google Sheets-Shaped Interchange

The v0 compatibility proof supports a constrained Google Sheets API-shaped JSON
adapter:

- spreadsheet `properties.title`, `properties.locale`, and `properties.timeZone`
- sheet `properties.sheetId`, `properties.title`, and `gridProperties`
- grid `data[].rowData[].values[].userEnteredValue`
- basic `userEnteredFormat` for bold, italic, horizontal alignment, and number format
- cell `dataValidation` for the selected v0 list-validation subset
- native cell `note` and OpenDoc `opendocCellComments` extension arrays
- sheet `merges`
- sheet `basicFilter`
- sheet `protectedRanges`
- workbook `namedRanges`

The adapter imports formula source and regenerates computed values from OpenDoc
state, including sheet-qualified references and ranges in multi-sheet
workbooks, escaped apostrophes in quoted sheet names, unquoted stable sheet-ID
references, and `$` absolute-reference anchors. Aggregate functions over ranges
ignore empty and non-numeric cells where the selected v0 function semantics call
for it, while direct missing references produce deterministic formula errors.
Formula copying shifts relative cell references and preserves absolute column
or row anchors. `COUNTIF`, `SUMIF`, `AVERAGEIF`, `COUNTIFS`, `SUMIFS`,
`AVERAGEIFS`, `MINIFS`, and `MAXIFS` support numeric criteria plus
case-insensitive text equality, inequality, and `*`/`?` wildcards with `~`
escaping. It exports the same constrained subset. Charts, pivots,
data-source sheets, filter views, high-risk cell fields such as `pivotTable`
and `dataSourceFormula`, and other high-risk structures abort import until they
have explicit source schema support.

## Deliberately Deferred

- Charts.
- Pivot tables.
- Data-source sheets.
- External import functions.
- Full locale-specific formula names.
- Protected range enforcement and editor permission lists beyond metadata-only import/export.
