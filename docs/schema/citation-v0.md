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
- A citation group can contain one or more citation items and is rendered as one
  citation occurrence label.
- `suppress_author` omits the author from author-year labels while retaining
  year, locator, prefix, and suffix. Numeric styles still render as numeric
  labels.
- Rendered text is a cache and may be regenerated.
- Citation style and locale are signed source metadata; changing either is a
  version-control operation that rerenders citation caches.
- Rendered citation labels and citation-group `rendered_cache` values are not
  part of the normal document-content signature payload.
- `citum-native` is the v0 source format.
- CSL-JSON import/export is an adapter, not the signed core schema.
- Unknown source fields are preserved inside source bytes.
- Footnote citations use `placement: "footnote"` plus a footnote UUID
  target. App/API projections expose this as `footnote_id`; Google-shaped
  citation extensions expose it as `footnoteId`. A footnote citation without
  that target is invalid because it would lose its source anchor.
- Reference updates and citation-group updates are version-control operations.
- Reference deletion is a version-control operation that marks the reference
  deleted, hides it from normal bibliography views, keeps it in audit/recovery
  state, and rerenders dependent citation labels with stable fallback text.
- Citation-group deletion is a version-control operation that marks the group
  deleted, hides it from normal citation views, keeps it in audit/recovery
  state, and leaves any remaining inline occurrence labels as stable
  missing-group placeholders until the occurrence is deleted or restored.
- Concurrent updates resolve by deterministic revision/order semantics and must keep the document openable.
