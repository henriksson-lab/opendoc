# ADR 0047: section boundaries and per-page context

## Status

Accepted.

## Context

`Document` currently owns one `PageSetup` and six document-wide furniture
slots. A later DOCX or Google section cannot therefore be represented without
silently applying its geometry and headers to unrelated pages. A `PageBreak`
only carries flow, not ownership of page geometry, furniture, or parity.

## Decision

The document will have an ordered section sequence. Existing source decodes as
one deterministic root section derived from the document UUID. Every later
section is introduced by an atomic top-level `BlockKind::SectionBreak {
section_id }`; it has no content or properties and means the following body
block begins a new page in that section. It is forbidden in furniture or table
cells, cannot be first, last, or adjacent to another section break, and cannot
be deleted or moved by generic block operations.

Each `Section` owns a `PageSetup` and the existing six furniture slots,
including the distinction between inherited (`None`) and intentionally empty
first/even overrides. Section IDs and the break's section ID are stable source
identities; settings operations use `(section_id, setting)` causal-LWW keys.
A deleted section ignores stale settings writes. Section insertion/deletion and
its boundary are atomic, and undo restores both exact source records.

Layout emits per-page `{ section_id, section_page_index, page_setup }` context.
It forces a page boundary at `SectionBreak`, resets first/even furniture per
section, and gives PDF that page's own MediaBox. Page numbers remain global in
the first vertical slice; one-column sections only. Columns, page-number
restarts, continuous/column breaks, and ODT master-page sequencing remain
explicit follow-up work.

DOCX and Google import/export map only supported one-column next-page sections.
Unsupported section settings are retained as warnings, never projected onto a
different section. HTML exposes section/page context as projection metadata,
not a second editable source representation.

## Consequences

The implementation must migrate old document-level setup/furniture to the root
section before signing or authoring a section operation. It requires core,
merge/inverse, layout/PDF, app DTO, and DOCX/Google tests as one vertical
slice; a section UI must not precede mixed-size-page layout.
