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
- `page_break`

Inline nodes:

- `text`
- `link`
- `citation`
- `footnote_ref`
- `mention`
- `equation`

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
