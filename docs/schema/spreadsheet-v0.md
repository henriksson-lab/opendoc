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

Cells are stored sparsely by stable sheet ID, row ID, and column ID. UI coordinates are projections.

```json
{
  "id": "cell_r1_c1",
  "row": "row_1",
  "column": "col_1",
  "user_value": { "kind": "formula", "value": "=SUM(B1:B3)" },
  "computed_value": { "kind": "number", "value": 12 },
  "format": { "number": "decimal", "bold": false },
  "validation": null,
  "comment_ids": []
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
      "merges": [],
      "filters": [],
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
- rectangular ranges
- `SUM`
- stale or missing references

## Deliberately Deferred

- Charts.
- Pivot tables.
- Data-source sheets.
- External import functions.
- Full locale-specific formula names.
- Protected ranges beyond metadata-only import/export.
