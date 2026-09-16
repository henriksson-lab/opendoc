# ADR 0044: Image Wrap Clearance Is Logical and In-Flow

Status: accepted.

## Decision

`ImageLayout::wrap_clearance` is an optional, typed four-edge
`ImageWrapClearance { top, end, bottom, start }`, measured in non-negative
twips. It is valid only with the existing in-flow `WrapStart` or `WrapEnd`
placement. An absent value means the destination's ordinary default float gap;
an all-zero user choice is normalised back to absent.

The existing whole-value `UpdateImageLayout` operation carries the value, so a
four-edge spacing edit is one gesture, one inverse, and one convergence unit.
`set_image_block_placement("block")` clears clearance because clearance with no
float has no meaning.

HTML projects the values as logical custom properties on the figure and the
stylesheet selects the physical float margins. DOCX maps the four values to
DrawingML `distT/R/B/L`. PDF explicitly reports that it cannot shape text
around floats (including authored clearance) rather than pretending its block
fallback is faithful.

## Non-decision

This does not add positioned wrap/break modes. Positioned images remain the
separate ADR 0022 behind/in-front object contract; combining their page/block
anchors with text flow requires a paginator-level exclusion geometry model.
