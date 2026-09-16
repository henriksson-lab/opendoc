# ADR 0030: Inline dropdown chips select stable local option IDs

## Decision

`Inline::Dropdown` is an atomic inline object containing a nonempty list of
canonical `{ id, label }` options and one `selected_option_id`.  The selected
value names an option ID, never a display label or position.  This keeps a
selection meaningful when labels are changed and prevents a concurrent
reorder from selecting a different value.

`SelectDropdownOption` is the only mutation in this slice.  It accepts only
an option already present on that exact dropdown, merges as the ordinary
operation order dictates, and has a direct inverse using the prior selected
ID.  Editing the option catalogue is intentionally deferred: it needs its own
set-wide merge semantics and must not be smuggled into a selection command.

The HTML projection is an atomic, labelled button with `aria-haspopup=listbox`.
The desktop opens a labelled native selection dialog and sends the typed
command.  Thus a chip does not pretend to be editable text and remains
reachable by keyboard and assistive technology.

Google Docs' public document JSON has no portable dropdown paragraph element.
Google-shaped OpenDoc interchange therefore retains this as a namespaced
`opendocDropdown` extension. DOCX and ODT export the selected label with a
specific degradation warning; they do not invent a form-control object.

## Consequences

The selected label is ordinary visible text for search, text export, layout,
and PDF, while the option set remains durable document data for OpenDoc
save/open, merge, undo, app projection, and extension interchange. Date,
file, event, place and person chips are separate semantic constructs: they
must not be encoded as arbitrary dropdown strings merely because they share a
pill-shaped UI.
