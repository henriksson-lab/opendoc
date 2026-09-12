# ADR 0010: An Export's Warnings Belong To The Export, Not To The Document

Status: accepted for v0. Closes the open decision recorded in PLAN77
("Export warnings have nowhere correct to go"), carried forward through three
waves.

## Context

Three producers emit warnings that describe what an *export* could not carry:

- `export_google_docs_json_with_warnings` — a checklist becomes a plain
  bullet, a line-spacing rule Google's schema cannot state is dropped, block
  formatting on an image or equation block has nowhere to go.
- `export_docx_with_warnings` — 24 warning codes, one per thing
  WordprocessingML cannot carry exactly: comments, suggestions, the DOI, a
  page-number field's cached result, an equation that becomes its source.
- The equation renderer — unknown LaTeX commands, unparseable sources.

Until now the commands threw all of it away. A user exporting a checklist to
Google JSON was told nothing at all.

### Why the obvious fix is wrong

`OpenDocApp::push_model_warning` appends to `document.warnings`. That field is
**source state**: it is part of the canonical CBOR record, it is hashed, and
it is what a signature covers. Calling it from an export path would mean:

- exporting a document marks it as having unsaved changes,
- exporting a document twice appends the same warnings twice,
- exporting a *signed* document moves the bytes the signature was taken over,
  so a document nobody edited stops verifying.

A read-only projection cannot be allowed to write into signed state. Two
independent reviews flagged this before it was implemented, which is why it
was left open rather than guessed at.

### The two candidate shapes

**(a) Carry the warnings in the command result.** Change `export_*`'s return
from `Text` to a structured result, and let it ripple through the generated
contract and `main.ts`.

**(b) Project them like the equation renderer's.** `render_document` returns a
`Rendering { html, warnings }` and `projection_service` folds those warnings
into the `AppDocument` projection on every call, never into
`app.document.warnings`. That is already how equation warnings reach the UI,
and it is correct *for a projection*.

(b) does not fit an export. A projection warning is recomputed on every
`get_document`, because the thing it describes — this document, rendered — is
still true next time. An export warning describes **one command's output**: the
`.docx` the user just saved. Hanging it on the document projection would mean
either recomputing an export on every `get_document` (absurd — the DOCX writer
would run on every keystroke), or keeping a scrap of "the last export said" in
app state, which is the same mutable side effect in a different field.

## Decision

**(a). An export command returns an `AppExport`, and its warnings ride in it.**

```rust
pub struct AppExport {
    pub content: String,
    pub encoding: AppExportEncoding,  // Text | Base64
    pub media_type: String,
    pub file_extension: String,
    pub warnings: Vec<AppWarning>,
}
```

`AppCommandResult` gains an `Export(AppExport)` variant; `CommandReturn` gains
`AppExport`; `export_google_docs_json`, `export_docx` and
`export_google_sheets_json` return it instead of `string`. The generated
contract projects it to `apps/desktop/src/generated/export.ts` like every other
DTO, so TypeScript cannot drift from it.

Consequences chosen deliberately:

- **The whole export path is `&self`.** `ImportExportReadService` never took
  `&mut`, and now nothing in it wants to: the warnings leave by the return
  value. That is the property the test asserts, and it is enforced by the
  borrow checker rather than by convention.
- **Encoding and media type come from Rust.** `main.ts` used to hold a table
  saying `export_docx` is base64 and means
  `application/vnd.openxmlformats-officedocument.wordprocessingml.document`.
  That table could disagree with the exporter about what it had just written.
  A format's extension and media type are facts about the format, so they are
  stated once, next to the writer.
- **One `downloadExport` in the frontend.** Three near-identical `case` arms
  collapsed into one call, and a fourth export added later gets the warning
  reporting without knowing it exists.
- **The warnings are shown as view state.** `main.ts` keeps the last export's
  warnings in a module variable and renders them in the warnings panel under
  their own heading ("Word (.docx) export"), above the document's own. They
  are never merged into `doc.warnings`, and nothing writes them anywhere.

### Not changed

`export_spreadsheet_csv` and `export_spreadsheet_xlsx` still return `string`.
They belong to the spreadsheet command group and were not converted here; the
uniform shape is the right end state and converting them is mechanical.

## The test that pins it

`exporting_a_signed_document_changes_neither_its_bytes_nor_its_signature` in
`crates/opendoc-app/src/import_export.rs`:

1. builds a document whose export *does* warn (a checklist, which Google JSON
   cannot state and DOCX can only approximate with ballot-box glyphs),
2. signs it with an OpenSSH key and verifies,
3. exports it to Google JSON, DOCX and Sheets JSON, asserting each result
   carries warnings — so the test cannot pass by exporting nothing,
4. asserts the canonical snapshot payload is **byte-identical** to the one
   taken before the exports, that `document.warnings` is unchanged, that the
   dirty flag is unchanged, and that the signature still verifies.

Step 3 is what makes step 4 mean something: without it the assertions would
also hold for an export that produced no warnings to mishandle.

## Consequences

- A new export command should return `AppExport`; returning `String` throws
  away the only channel its warnings have.
- Nothing in an export path may take `&mut OpenDocApp`. If it needs to, the
  thing it wants to write down is either a projection (belongs in
  `projection_service`) or a document edit (belongs in a command that says so).
- `push_model_warning` remains for *import*, where a warning genuinely is a
  fact about the document that was just created.
