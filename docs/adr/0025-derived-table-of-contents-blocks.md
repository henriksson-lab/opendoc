# ADR 0025: A Table of Contents Is an Atomic Derived Block

## Decision

`BlockKind::TableOfContents { max_level }` is durable source state identifying
where a generated table of contents belongs and the deepest top-level heading
level it includes. It has no inline content or block properties. The entries
are derived on every render and layout from the document's current top-level
headings. Each HTML entry links to the heading's stable block id.

There is intentionally no `UpdateTableOfContents` operation. A heading insert,
delete, rename, reorder, or level change changes the next projection on every
replica, so a separate copied list of text or frozen page numbers would create
a second, stale collaborative source of truth. “Update” means re-rendering or
re-exporting the same durable block.

The layout cache treats the complete derived heading entry set as an explicit
document-wide input. This prevents a cached TOC height surviving a change to a
different heading block.

## Interchange

Google-shaped OpenDoc JSON preserves this through `opendocTableOfContents`,
with an explicit warning because it does not issue Google’s native TOC
request. DOCX writes Word's native `TOC \o "1-N" \h \z \u` field, which
Word updates from its headings and page layout; it deliberately carries no
OpenDoc-derived cached entries. ODT writes ODF's native
`text:table-of-content` index declaration, scoped to `max_level` and likewise
without copied entries. An office suite supplies page numbers when it updates
the index. Native Google Docs TOC requests and arbitrary native TOC import
remain future work because they require format-specific source-location and
update semantics.

## Consequences

This makes collaborative updates deterministic without inventing bookmark
ranges or persisting derived content. It deliberately covers top-level
headings only; tree-scoped/nested heading navigation remains a separate model
decision.
