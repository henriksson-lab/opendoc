# ADR 0020: A horizontal rule is a structural block

Status: accepted.

## Context

Google Docs represents a horizontal rule as a paragraph element.  Treating it
as text (or as a paragraph bottom border) makes it disappear when that
paragraph is edited, and cannot round-trip a rule that has no text at all.

## Decision

`BlockKind::HorizontalRule` is a durable, atomic, content-free block.  It has
no style knobs in v1: renderers use the document's rule CSS, and import/export
maps it to each format's ordinary horizontal-rule representation.  Paragraph
properties and inline content are invalid for it rather than being silently
ignored.  Formats without a native shape must emit an explicit degradation
warning.

## Consequences

Rules participate in ordering, undo, collaboration and document serialization
like every other block.  They are not bookmarks, section breaks, or a generic
shape primitive; those need their own semantics and decisions.
