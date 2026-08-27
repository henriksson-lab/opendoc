# Citation Schema v0

Status: draft.

Purpose: represent structured citations as special inline labels while storing bibliographic metadata in a document-local database.

Decision: citations are not links. Paperpile appears to use Google Docs links because it cannot extend the Google Docs schema; OpenDoc can, so citations are first-class inline nodes.

## Citation Node

```json
{
  "type": "citation_label",
  "id": "label_1",
  "citation_id": "cite_1",
  "rendered_cache": null
}
```

The label is the visible inline object in the rich-text stream. It references a document-local citation group. It does not store the full bibliographic source.

## Citation Group

```json
{
  "id": "cite_1",
  "revision": 1,
  "placement": "inline",
  "items": [
    {
      "reference_id": "ref_doe_2020",
      "locator": "42",
      "label": "page",
      "prefix": "",
      "suffix": "",
      "suppress_author": false
    }
  ],
  "rendered_cache": "(see Doe 2020, 42)",
  "deleted": false
}
```

## Bibliography Store

```json
{
  "style": "apa-7th",
  "locale": "en-US",
  "references": {
    "ref_doe_2020": {
      "revision": 1,
      "source": {
        "format": "citum-native",
        "bytes": "<binary or text bytes accepted by the citum adapter>"
      },
      "summary": {
        "title": "Example Article",
        "authors": ["Doe"],
        "issued": "2020",
        "doi": null,
        "url": null
      },
      "deleted": false
    }
  }
}
```

## Rules

- Citation IDs are stable document-local IDs.
- Reference IDs are stable bibliography-local IDs.
- A reference can be used by many citation groups.
- Rendered text is a cache and may be regenerated.
- `citum-native` is the v0 source format.
- CSL-JSON import/export is an adapter, not the signed core schema.
- Unknown source fields are preserved inside source bytes.
- Footnote citations use `placement: "footnote"` and point to a footnote block.
- Reference updates and citation-group updates are version-control operations.
- Concurrent updates resolve by deterministic revision/order semantics and must keep the document openable.
