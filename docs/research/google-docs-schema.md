# Google Docs Schema Research

Status: initial complete draft.

## Sources

- Google Docs API `documents` resource: https://developers.google.com/workspace/docs/api/reference/rest/v1/documents
- Google Docs structure guide: https://developers.google.com/workspace/docs/api/concepts/structure

## Findings

The public Google Docs API exposes a document as a hierarchy of tabs, body content, structural elements, and paragraph elements. The important practical model is:

- document metadata and styles
- body content as ordered structural elements
- paragraphs terminated by newlines
- text runs with uniform text style
- paragraph styles and named styles
- lists keyed by list IDs
- tables with nested structural content
- footnotes, headers, footers, and inline objects
- named ranges
- suggested insertions, deletions, and style changes

Google's public API uses UTF-16 code unit indexes for ranges. That is an interoperability detail, not a good internal model for a Rust editor because Rust strings are UTF-8 and collaborative positions need to survive concurrent edits.

## Subset Matrix

| Area | Supported v0 | Deferred | Excluded v0 |
| --- | --- | --- | --- |
| Metadata | id, title, locale | tabs as full UI objects | Drive sharing model |
| Blocks | paragraph, heading, list item, table, page break | table of contents, section breaks | pixel-perfect pagination |
| Inline | text, link, citation, footnote reference, mention placeholder | rich smart chips, equations | drawings as editable Docs-native objects |
| Marks | bold, italic, underline, strike, code, superscript, subscript, color, background, font, size | advanced OpenType, language spans | arbitrary CSS |
| Styles | named paragraph/text styles | full theme inheritance | exact Google default style matching |
| Lists | unordered, ordered, nesting level | custom glyph presets | every Google list preset |
| Tables | rows, cells, text content | cell borders, width constraints | complex layout fidelity |
| Comments | anchored comments | resolved history | Google account identity model |
| Suggestions | insertion/deletion/style overlays | full review UI | exact Google suggestion semantics |
| Imports | Google API JSON mapping | OOXML/ODF import | full Word/OpenOffice schema support |

## Recommendation

Use `docs/schema/document-v0.md` as the internal shape and treat the Google Docs API as one importer/exporter. Persist CRDT-native positions and derive UTF-16 indexes only when interacting with Google-compatible APIs.

For early `.doc` import proof, external conversion tools are acceptable if they are easy to install on Linux, macOS, and Windows, or at worst usable on Linux without root access.

Import should preserve comments and suggestions when the conversion path exposes them. Google or converter-internal IDs do not need to be preserved, but their design should be studied for useful identity and anchoring ideas.

The original `.doc` source file is not retained by default after conversion. If provenance requires retaining it later, it can be added as an explicit signed blob.

First import proof can target `.doc` and `.docx` if tooling allows. Without Google credentials, “Google Docs import” means converting into the OpenDoc schema shaped by the Google Docs API model, not calling the Google API.

Repeat imports do not need to reproduce the same invisible block UUID structure. Imported IDs can be regenerated as needed.

## Rejected Options

- **Store Google Docs API JSON directly.** Rejected because the API shape is an integration format, contains Google-specific concepts, and uses UTF-16 positional ranges.
- **Use OOXML or ODF as the core model.** Rejected for v0 because those schemas are much larger than the desired Google Docs subset.
- **Use HTML as the source of truth.** Rejected because comments, suggestions, citations, and collaborative anchors become ambiguous.

## Open Risks

- Tables may require nested block content sooner than expected.
- Suggestions can become complex if legal or academic editing workflows are a core market.
- Export fidelity will depend on careful style inheritance rules.

## Completion Evidence

- Supported/deferred/excluded matrix: present.
- Draft schema: `docs/schema/document-v0.md`.
- Required examples: present in the schema draft.
- Position model: CRDT element IDs with UTF-16 conversion at import/export boundaries.
- Import/export gaps: documented above.
