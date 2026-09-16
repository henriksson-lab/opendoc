# ADR 0038: Whole-inline link suggestion identity

A suggested link change is an atomic edit to one `Text` or `Link` inline,
identified by that inline's stable id. It retains the source href that the
author reviewed (`None` for unlinked text) plus the proposed href (`None` for
a removal). This supports link addition, removal, and replacement without
pretending that a partial selection or caret can own a link independently.

Accepting the proposal requires both the exact target identity and its
recorded source href to survive. A deleted target or a concurrent link change
rejects the proposal and leaves document content untouched; it must never
retarget a neighbouring inline or overwrite a different href. The OpenDoc
Google-shaped interchange extension round-trips this as `link_change`. Native
Google, DOCX and ODT tracked-link-change interchange is still not claimed.
