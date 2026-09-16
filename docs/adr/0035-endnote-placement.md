# ADR 0035: Endnotes are note placement, not a second inline kind

## Decision

An endnote uses the existing stable note record and `Inline::FootnoteRef`.
`Document::endnote_ids` durably records which note records render at the end
of the document. `SetEndnotePlacement` uses that note's revision clock, so a
placement change is journalled, mergeable, and invertible rather than a UI
projection or an id-prefix convention.

## Consequences

HTML has distinct footnote and endnote trailers. DOCX import and export retain
native endnote placement through its separate `endnotes.xml` part; ODT writes
its native endnote class. Google Docs has no endnote primitive, so that
interchange remains footnote-only rather than silently claiming placement.
