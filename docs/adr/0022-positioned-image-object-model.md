# ADR 0022: Positioned Image Object Model

Status: accepted for the durable model; layout and interchange support are deferred.

## Context

ADR 0012 deliberately shipped only in-flow image placement. That was the right v0 boundary, but it means imports cannot faithfully retain Google Docs and Word objects that are anchored to a page or a paragraph and drawn behind or in front of text. Treating those as `wrap-start` or `wrap-end` is a lossy conversion that looks like a successful round trip.

We need a durable representation before importers begin accepting such objects. It must not claim that the current HTML layout engine, PDF writer, or exporters already render it.

## Decision

### 1. A positioned image remains an image block

`BlockKind::Image` keeps the blob, accessible name, intrinsic/display size, effects and fallback in-flow placement from ADR 0012. A new optional `ImageLayout::positioned` value says that the image is an out-of-flow object. It is deliberately not a general drawing variant: shapes, charts and diagrams will get their own object type later instead of overloading image bytes.

### 2. Anchor identity is durable; a missing target degrades predictably

`PositionedImageAnchor` is either `PageContent` — offsets start at the page content rectangle's top/start corner — or `Block(StableId)` — offsets start at the target block's border box.

The referenced block id is an identity, not a positional index. It may be missing after a concurrent deletion or after importing partial content. That does **not** invalidate the document or retarget it to an adjacent block; renderers that implement positioned layout must use `PageContent` as a deterministic fallback and report a warning. An object may not anchor to its own image block.

### 3. Geometry is document geometry, not CSS

Offsets are signed `Length` values in twips. The picture's dimensions remain the existing `ImageLayout.width` and `.height`, so there is exactly one source of truth for its display rectangle. Coordinates are permitted outside the content rectangle: clipping is a renderer/exporter concern and must be reported by targets that cannot represent it.

### 4. Layering is explicit and mutually exclusive with flow placement

`PositionedImageLayer` is `BehindText` or `InFrontOfText`. A positioned image must have no in-flow `ImagePlacement`; `None` means the in-flow default only when `positioned` is absent. This makes a source never ask both for wrapping and for absolute overlay.

### 5. Merge is whole-position last-writer-wins

Positioning travels in the existing whole-value `UpdateImageLayout` operation. An anchor move changes anchor, both offsets and layer as one gesture; merging fields independently could combine a destination selected by one author with coordinates selected against another. Existing causal/LWW operation ordering therefore decides concurrent object moves, and inverse operations restore the whole previous layout.

### 6. Initial support boundary is intentional

The first implementation owns model validation, serialization, operation validation/inversion/merge, and application commands. It exposes no desktop control and does not alter renderer, PDF, DOCX, ODT or Google output. Those targets must continue to warn when they encounter a positioned object rather than silently draw a different in-flow image. No importer may claim native positioned-object support until its target has a tested mapping to these fields.

The subsequent layout slice implements page-content and top-level stable-block
anchors in HTML/desktop and the Rust layout/PDF path. They are out of flow and
painted explicitly behind or in front of text; a missing block falls back to
page content on the object's own page and produces a projection warning. This
does not widen interchange support: DOCX, ODT and Google still report their
in-flow fallback rather than claiming native positioned-object fidelity.

## Consequences

* Saved documents and collaboration replicas can preserve a positioned-image intent before every renderer supports it.
* A direct API consumer can author or clear the value as one undoable edit; user-facing positioning controls wait for a renderer that can show their result.
* General drawings remain out of scope and cannot be smuggled in as images.
