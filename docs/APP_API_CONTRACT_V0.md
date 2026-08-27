# App API Contract v0

This is the prototype projection contract between the Rust app core, the Tauri
command layer, the browser demo backend, and future server modes. The Rust
source of truth is `opendoc_app_api::AppDocument`.

Compatibility is not promised yet. Changes are allowed while OpenDoc remains a
research prototype, but every change to this contract must update the Rust
projection tests and the TypeScript rendering/mock backend together.

## Top-Level Document

`AppDocument` contains:

- `uuid`, `title`, `locale`, `visible_text`
- `blocks`
- `comments`
- `suggestions`
- `citations`
- `workbook`
- `warnings`
- `signature_state`
- `signature`
- `signatures`
- `repository_root`
- `last_manifest`
- `operation_count`
- `operations`

`visible_text` is a display/search projection only. Signing uses the canonical
snapshot payload, not rendered text.

## Blocks

Each block contains:

- `id`
- `kind`
- `level`
- `ordered`
- `equation_source`
- `content`
- `rows`

Supported `kind` values:

- `paragraph`
- `heading`
- `list-item`
- `table`
- `equation-block`
- `page-break`

Table rows are nested as `rows[row][cell][block]`. Nested blocks use the same
`AppBlock` shape as top-level blocks.

## Inlines

Each inline contains:

- `id`
- `kind`
- `text`
- `href`
- `target_id`
- `marks`

Supported `kind` values:

- `text`
- `link`
- `citation`
- `footnote-ref`
- `mention`
- `equation`

Citation inline text is a rendered cache from the document-local citation
database. Citation edits must update the database and rerender dependent
citation groups instead of mutating citation label text directly.

## Comments And Suggestions

Comment threads contain:

- `id`
- `anchor`
- `comments`
- `deleted`

Suggestion records contain:

- `id`
- `author`
- `kind`
- `state`

Deleted comments stay in history and are hidden by normal views. Suggestions
support proposed, accepted, and rejected states.

## Citations

The citation database contains:

- `style`
- `locale`
- `references`
- `citations`

References contain the document-local source bytes as UTF-8 text in `source`,
plus parsed summary fields:

- `id`
- `revision`
- `format`
- `source`
- `title`
- `authors`
- `issued`
- `doi`
- `url`
- `deleted`

The v0 preferred source format label is `citum-native`.

## Spreadsheet

The workbook projection contains sheets, rows, columns, and cells. Each cell
contains:

- `address`
- `user_kind`
- `user_value`
- `computed_kind`
- `computed_value`
- `dependencies`

Formula source lives in `user_value` and is part of persisted/signed state.
Computed values and dependencies are deterministic projections.

## Signatures

`signature_state` is one of:

- `unsigned`
- `signed`

`signature` is the first signature for legacy/simple UI display. `signatures`
contains all sidecar signature records:

- `target`
- `signer`
- `signer_display`
- `title`
- `signed_at_ms`

Any document operation clears signatures in the current editable state.

## Operations

`operations` is the app-facing journal segment projection. Each record contains:

- `actor`
- `seq`
- `kind`
- `summary`
- `created_at_ms`

The v0 GUI uses operation records for audit display. Merge semantics live in
`opendoc-merge` operation types; full replayable operation payload persistence
remains future work.
