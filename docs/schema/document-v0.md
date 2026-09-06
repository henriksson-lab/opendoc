# Document Schema v0

Status: draft.

Purpose: define the first internal OpenDoc document model. It is intentionally smaller than Google Docs, but mapped against the public Google Docs API model so import/export can be built later.

## Position Model

Canonical persisted positions are CRDT element IDs, not byte offsets. Plain indexes are allowed only at API boundaries and in examples. JSON in this file is a human-readable projection of the schema, not the canonical storage encoding.

- Internal edits address text by stable sequence IDs owned by the collaboration layer.
- Renderer projections may expose grapheme indexes.
- Google Docs import/export converts from or to UTF-16 code unit indexes because the Google Docs API exposes positions that way.
- Document snapshots store logical block and inline structure, not transient cursor positions.

## Top-Level Shape

```json
{
  "schema": "opendoc.document.v0",
  "id": "doc_01",
  "title": "Example",
  "locale": "en-US",
  "default_style": "normal",
  "styles": {},
  "sections": [],
  "comments": [],
  "suggestions": [],
  "bibliography": {
    "style": "apa",
    "items": {}
  }
}
```

## Supported Nodes

Block nodes:

- `paragraph`
- `heading`
- `list_item`
- `table`
- `image`
- `page_break`

Page breaks are standalone source blocks. Google Docs-shaped export uses a
paragraph-level `pageBreak` element. Import accepts page-break-only paragraphs
and older `sectionBreak` fixture records, while mixed page-break/content
paragraphs abort instead of silently changing document order.

Table cells contain nested OpenDoc blocks. Google Docs-shaped table-cell import
and export use the same supported block adapters as top-level body content for
paragraphs, page breaks, images, and block equations. Nested tables are
explicitly unsupported on import and export in v0.

Inline nodes:

- `text`
- `link`
- `citation`
- `footnote_ref`
- `mention`
- `equation`

Footnote references point to document-local footnote records. A footnote record
contains a stable ID, revision, inline body content, and deleted flag. Footnote
body text is source state and is edited through operations, while reference
labels are render projections.

Citations are inline source nodes, not links. A citation node references a
document-local citation group; the group references one or more bibliography
records. Rendered citation text is a projection/cache and is excluded from
normal source-content signatures. See `docs/schema/citation-v0.md`.

Google Docs-shaped citation fixtures use an explicit OpenDoc extension because
the public Google Docs API does not expose an arbitrary first-class inline node
for OpenDoc citations. Paragraph elements may contain `opendocCitation`, and
the document may contain a top-level `opendocCitations` database. This adapter
shape is for import/export tests and interoperability, not the canonical binary
document format.

Comments and suggestions are also source state, not renderer-only annotations.
They anchor to document, nearest-block, or stable text-range anchors and are
signed with the document source state. Google Docs-shaped fixtures carry them
as top-level `opendocComments` and `opendocSuggestions` extension arrays so
the adapter can prove full round-trip behavior without reducing them to plain
text, DOM spans, or Google-specific transient IDs.
App/API suggestion projections preserve insert anchors, structured insert inline
content, delete/format range endpoints, format mark labels, state, and
provenance. Insert suggestion `text` is a compact editable projection;
`content` is the source-shaped inline payload.
Deleted comment threads and individual deleted comments remain retained source
state for audit/recovery and can be restored by stable IDs as operation-backed
source changes.

Warning records are retained source-side audit metadata for deterministic
degradation. Each warning must carry a non-empty stable code and non-empty
message; empty warning payloads are invalid after import or binary decode.

Mentions are typed inline source nodes. Google Docs-shaped fixtures carry them
as `opendocMention` paragraph elements with `inlineId` and `label` so adapters
do not flatten them into plain text.

Marks:

- `bold`
- `italic`
- `underline`
- `strike`
- `code`
- `superscript`
- `subscript`
- `color`
- `background`
- `font`
- `size`

`color`, `background`, `font`, and `size` carry string values in canonical
source state. The first prototype treats those values as editor-facing source
properties and keeps rendering details as projection.

## Equations

Equations use one canonical source store. V0 should use a TeX/LaTeX-like source representation and derive MathML or rendered browser output when needed.

```json
{
  "type": "equation",
  "id": "eq_1",
  "source_format": "latex",
  "source": "E = mc^2",
  "rendered_cache": null
}
```

Rendered output is not part of the normal document-content signature.

Equations may be inline or block-level:

- inline equations are inline nodes inside paragraphs
- block equations are block nodes with equation source and display metadata

Inline and block equation source edits are dedicated structured operations, not
generic text edits. This keeps equation source atomic for merge, audit, and
signature reasoning.

Google Docs API import can see equation elements, but that API shape does not
expose TeX/LaTeX source. V0 imports those elements as placeholder equations
with explicit warnings; Google Docs-shaped export can emit an equation marker
but cannot prove source-preserving equation round-trip through that API alone.

Source-preserving Google Docs-shaped fixtures use explicit `opendocEquation`
and `opendocEquationBlock` extensions. The adapter stores stable inline/block
IDs, `equationId`, `sourceFormat`, and `source`, and rejects unknown source
formats rather than silently changing equation semantics.

## Images And Attachments

Images are source blocks that reference content-addressed blobs. The document
stores the blob hash and source-level alt text; bytes, compression details,
availability, and exact-byte sidecar signatures remain in the blob layer.

```json
{
  "type": "image",
  "id": "img_1",
  "blob_hash": "sha256:...",
  "alt_text": "Gel electrophoresis figure"
}
```

Image alt text is edited as source state by stable image block ID. Updating alt
text does not change the blob hash and therefore does not invalidate detached
exact-byte blob signatures. Replacing an image updates the `blob_hash` source
reference while preserving the image block ID and alt text, so comments and
history anchored to the image can remain stable.

Missing image blobs do not make the document invalid. Clients render a
placeholder, keep the hash reference, and surface a deterministic warning so a
shallow clone can be completed later by restoring the blob by hash.

Google Docs-shaped image fixtures use an explicit OpenDoc extension because the
public Google Docs API image/object model is not the canonical OpenDoc blob
model. Body content may contain `opendocImage` with `blockId`, `blobHash`, and
`altText`; the adapter imports and exports that extension while keeping native
Google inline object import as high-risk unsupported work until blob recovery
semantics can be preserved.

## Examples

### Minimal

```json
{
  "schema": "opendoc.document.v0",
  "id": "doc_minimal",
  "title": "Minimal",
  "locale": "en-US",
  "sections": [
    {
      "id": "sec_1",
      "blocks": [
        {
          "id": "blk_1",
          "type": "paragraph",
          "content": [
            { "type": "text", "text": "Hello world.", "marks": [] }
          ]
        }
      ]
    }
  ],
  "comments": [],
  "suggestions": [],
  "bibliography": { "style": "apa", "items": {} }
}
```

### Styled

```json
{
  "schema": "opendoc.document.v0",
  "id": "doc_styled",
  "title": "Styled",
  "locale": "en-US",
  "styles": {
    "heading_1": { "font": "Arial", "size": 20, "bold": true },
    "normal": { "font": "Arial", "size": 11 }
  },
  "sections": [
    {
      "id": "sec_1",
      "blocks": [
        {
          "id": "blk_h1",
          "type": "heading",
          "level": 1,
          "style": "heading_1",
          "content": [{ "type": "text", "text": "Title", "marks": [] }]
        },
        {
          "id": "blk_p1",
          "type": "paragraph",
          "style": "normal",
          "content": [
            { "type": "text", "text": "A styled ", "marks": [] },
            { "type": "text", "text": "word", "marks": [{ "type": "bold" }] },
            { "type": "text", "text": ".", "marks": [] }
          ]
        }
      ]
    }
  ],
  "comments": [],
  "suggestions": []
}
```

### List Heavy

```json
{
  "schema": "opendoc.document.v0",
  "id": "doc_list",
  "title": "List",
  "locale": "en-US",
  "sections": [
    {
      "id": "sec_1",
      "blocks": [
        {
          "id": "li_1",
          "type": "list_item",
          "list_id": "list_a",
          "level": 0,
          "ordered": false,
          "content": [{ "type": "text", "text": "First", "marks": [] }]
        },
        {
          "id": "li_2",
          "type": "list_item",
          "list_id": "list_a",
          "level": 1,
          "ordered": false,
          "content": [{ "type": "text", "text": "Nested", "marks": [] }]
        }
      ]
    }
  ],
  "comments": [],
  "suggestions": []
}
```

### Table

```json
{
  "schema": "opendoc.document.v0",
  "id": "doc_table",
  "title": "Table",
  "locale": "en-US",
  "sections": [
    {
      "id": "sec_1",
      "blocks": [
        {
          "id": "tbl_1",
          "type": "table",
          "rows": [
            [
              [{ "type": "text", "text": "Name", "marks": [{ "type": "bold" }] }],
              [{ "type": "text", "text": "Value", "marks": [{ "type": "bold" }] }]
            ],
            [
              [{ "type": "text", "text": "Alpha", "marks": [] }],
              [{ "type": "text", "text": "1", "marks": [] }]
            ]
          ]
        }
      ]
    }
  ],
  "comments": [],
  "suggestions": []
}
```

### Citation Heavy

```json
{
  "schema": "opendoc.document.v0",
  "id": "doc_citations",
  "title": "Citations",
  "locale": "en-US",
  "sections": [
    {
      "id": "sec_1",
      "blocks": [
        {
          "id": "blk_1",
          "type": "paragraph",
          "content": [
            { "type": "text", "text": "This was shown earlier ", "marks": [] },
            {
              "type": "citation",
              "id": "cit_1",
              "items": [
                { "csl_id": "doe-2020", "locator": "42", "label": "page" }
              ],
              "prefix": "see",
              "suffix": "",
              "suppress_author": false,
              "placement": "inline",
              "rendered": "(see Doe 2020, 42)"
            },
            { "type": "text", "text": ".", "marks": [] }
          ]
        }
      ]
    }
  ],
  "comments": [],
  "suggestions": [],
  "bibliography": {
    "style": "apa",
    "items": {
      "doe-2020": {
        "id": "doe-2020",
        "type": "article-journal",
        "title": "Example Article",
        "author": [{ "family": "Doe", "given": "Jane" }],
        "issued": { "date-parts": [[2020]] }
      }
    }
  }
}
```

## Deliberately Deferred

- Pixel-perfect page layout.
- Drawings, equations, smart chips beyond opaque mention/link placeholders.
- Full suggestions UI semantics.
- Native import/export for OOXML and ODF.
