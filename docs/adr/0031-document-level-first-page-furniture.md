# ADR 0031: document-level first-page furniture overrides

Status: accepted, 2026-09-15.

## Context

ADR 0009 deliberately began with one document-wide header and footer. That
made a first-page header imported from DOCX either disappear with a warning or
be dangerously applied on every page. A full section model remains necessary
for section-local geometry, columns, linked/unlinked furniture, and odd/even
variants, but it is not necessary to represent the common single-section
first-page choice faithfully.

The important distinction is three-valued: no first-page setting inherits the
ordinary header/footer, while an author can explicitly request no furniture on
the first page.

## Decision

`Document` has optional `first_page_header` and `first_page_footer` block
fragments. `None` means inherit the ordinary slot; `Some(Vec::new())` means
an intentional blank first-page slot. They share the document's global block
and inline identity space and the same furniture validation as ordinary
slots.

`HeaderFooterSlot` names those override slots directly, so the existing
whole-slot `SetPageFurniture` operation, inverse operation, causal LWW merge
key, save/open and service authorization semantics apply independently to
each. This is a document-level first-page feature, not a hidden section model.

Layout resolves an ordinary header/footer through `furniture_for_page` for
page zero and then the ordinary slots for later pages. The desktop first-page
commands use the same explicit slots and page preview selection. DOCX maps the
override to `w:type="first"` references and `w:titlePg`; importing those
references recreates the overrides. ODT currently has a single master-page
writer and emits a named warning rather than silently flattening an override.

## Consequences

This closes normal single-section first-page header/footer interchange without
claiming section-local support. A document-wide even-page variant follows the
same three-valued scheme: it applies only to zero-based odd page indices, after
the first-page choice has been resolved. DOCX maps it to native `even`
references and writes `w:evenAndOddHeaders`, which Word requires before it
uses those references. ODT still has one master-page writer and names every
variant it cannot retain. Future sections must supersede the document-level
lookup with a page-to-section lookup, while retaining the `None` versus
explicit-empty distinction.
