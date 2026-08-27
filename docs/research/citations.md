# Citation Research

Status: initial complete draft.

## Sources

- Paperpile Google Docs workflow: https://paperpile.com/h/get-started-google-docs/
- Paperpile citation features: https://paperpile.com/features/google-docs-citations-bibliography/
- Citum core: https://github.com/citum/citum-core
- Citation Style Language schemas: https://github.com/citation-style-language/schema
- Rust `citeworks_csl` crate: https://docs.rs/citeworks-csl

## Findings

Paperpile's visible Google Docs behavior is useful, but OpenDoc should not copy the link encoding:

- Insert a linked placeholder citation in the document.
- Keep structured metadata outside plain text.
- Support citation groups.
- Support locators such as page/chapter.
- Support prefix, suffix, and suppress-author options.
- Reformat citations and bibliography after style changes.
- Store document-local copies of citation metadata so collaborators can edit without changing a user's personal library.

Paperpile likely uses links because Google Docs does not let extensions add arbitrary first-class inline node types. OpenDoc can extend its own schema, so citations should be special inline labels rather than links.

## Citation Data Model

Use a document-local bibliography database. Store citation labels as structured inline nodes that reference citation groups in that database; rendered text is only a cache.

Use `citum` as the v0 citation engine and allow the citation source bytes to be `citum-native`. CSL-JSON remains useful for import/export, but it is not the core signed storage contract.

See `docs/schema/citation-v0.md`.

## Rust Options

| Option | Strength | Weakness | Recommendation |
| --- | --- | --- | --- |
| `citum` / `citum-core` | Rust-native citation tooling; matches v0 direction | top-level crate is CLI-first, direct library API needs adapter work | use first |
| `citeworks_csl` | Rust serde types for CSL-JSON | not a complete citeproc renderer | use for data typing |
| Native Rust citeproc engine | ideal if complete | maturity must be verified | research further |
| External `citeproc` executable | proven CSL rendering path | process dependency | acceptable fallback |
| WASM citeproc-js | high compatibility | JS/WASM bridge | viable frontend/server fallback |

## Recommendation

Adopt a document-local reference database plus structured citation labels. Use `citum` first. Keep CSL-JSON as an adapter for import/export and interoperability.

## Rejected Options

- **Store citations as plain formatted text.** Rejected because reformatting and collaboration would lose metadata.
- **Store citations as links only.** Rejected because links are a Google Docs workaround; OpenDoc can represent citations directly.
- **Duplicate full reference metadata in every inline citation.** Rejected because one reference is commonly cited many times and should update once.

## Open Risks

- CSL rendering is subtle for ibid, footnote styles, and disambiguation.
- Collaborative edits can duplicate or delete citation anchors.
- Export to Word/EndNote/LaTeX requires additional mappings.

## Prototype Evidence

`prototypes/citation-render` renders a small author-date citation and bibliography from a simplified CSL-like record. It proves the structured-node workflow, not full CSL compliance.
