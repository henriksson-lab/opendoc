# ADR 0012: Image Display Geometry and the Positioning Subset

Status: accepted for v0.

## Context

`BlockKind::Image` was `{ blob_hash, alt_text }`. A picture rendered at
whatever its bytes decoded to and could not be resized, aligned or moved —
`docs/GOOGLE_DOCS_PARITY_TODO.md` OB-17. There was also no way to get a
picture in other than through the file picker (OB-18): the `drop` handler in
`editor.ts` was a bare `preventDefault`.

Google Docs' image model is large: size, crop, rotation, borders, and five
positioning modes (in line, wrap text, break text, behind text, in front of
text) with page anchoring and margin offsets. Shipping a half-working version
of all of it would be worse than shipping a working part of it, because the
half-working parts are indistinguishable from bugs and silently wrong on
export.

## Decision

### 1. Display size lives on the image block, in twips

```rust
BlockKind::Image { blob_hash, alt_text, layout: ImageLayout }

pub struct ImageLayout {
    pub width: Option<Length>,
    pub height: Option<Length>,
    pub placement: Option<ImagePlacement>,
}
```

`Length` is twips, like page geometry and block indents, so DOCX `wp:extent`
(EMUs, 1 twip = 635 EMU) and Google Docs `size` (points, 1pt = 20 twips)
convert exactly.

The *intrinsic* size stays a property of the blob and is never written here.
`None` means "the size the bytes decode to" and must not be materialised into
a default: doing so would freeze a projection of the blob into the source, and
re-encoding the blob would then silently contradict the document. The two axes
are independent — a width with no height means "scale the height", which is
what a side-handle drag does and what `height: auto` renders.

An empty layout is skipped by serde, so an image that was never resized
serializes to exactly the bytes it did before this field existed.

### 2. Placement is the in-flow subset, and the rest is omitted, not faked

```rust
pub enum ImagePlacement { Block, WrapStart, WrapEnd }
```

* `Block` (the default): the image owns its line. Google's "in line" and
  "break text" both land here, because an OpenDoc image *is* a block.
* `WrapStart` / `WrapEnd`: the figure floats to the start/end edge of the
  column and the following blocks flow beside it ("wrap text").

Deliberately **not** modelled: behind-text and in-front-of-text (out-of-flow
positioning plus a z-order the block model has no place for), absolute page
anchoring with margin offsets, crop, rotation and borders, and true
in-paragraph anchoring — an image sitting *inside* a run of text would have to
be an `Inline`, not a `Block`. An importer that meets any of these maps to the
nearest value here and emits a `ModelWarning`; nothing pretends to round-trip.

### 3. Alignment is not duplicated

`BlockProperties::alignment` already applies to an image block and already
round-trips through DOCX and Google Docs. The renderer emits it on the
`<figure>`, which centres or right-aligns the picture inside it. Adding an
alignment field to `ImageLayout` would have created a second source of truth
for the same fact. A floated image ignores alignment, as it does in every
word processor.

### 4. One whole-value operation, not one per axis

```rust
OperationKind::UpdateImageLayout { block_id, layout }
```

Whole-value last-writer-wins, for the reason `SetPageSetup` is: a resize is
one intent. Merging one replica's width with another's height would produce a
shape neither replica dragged, and with the axes coupled by an aspect ratio it
would also distort the picture. Concurrent resizes of the *same* image
therefore converge on a shape somebody actually chose.

### 5. Five commands, one per property, and one per gesture

`set_image_block_width`, `set_image_block_height`, `set_image_block_size`,
`clear_image_block_size`, `set_image_block_placement`. `set_image_block_size`
is not redundant with the first two: a corner drag is one gesture and has to
be one undoable operation, or an undo would leave the picture at a shape the
user never saw.

`set_image_block_placement("block")` stores `None` rather than
`Some(Block)`: it is what an image does when the document says nothing, so
writing it down would only produce two byte-different documents that mean the
same thing.

A non-positive size is refused, not clamped — zero is not a smaller picture.

### 6. Insertion by drop and paste reuses the existing pipeline

Dropping or pasting image files runs `add_binary_blob` then
`insert_image_block_after`, the same two commands the file picker uses, so a
dropped picture is content-addressed, deduplicated and signable exactly like
an attached one. `editor.ts` recognises the gesture and finds the position;
turning bytes into content stays with the command layer.

### 7. Insert-from-URL is a shell capability, and is not built here

Fetching a URL is network I/O with policy attached: CORS, redirects,
private-network access (an SSRF against the user's own LAN when run natively),
credentials, content sniffing and size limits. None of that is document
semantics, so it does not belong in `opendoc-core` or `opendoc-app`, and
putting `fetch` in the WASM core would give the local-first core network
access that ADR 0004 deliberately defers.

The right home is the Tauri shell, beside the file dialogs and file IO it
already owns: it fetches bytes, enforces a size cap, and hands them to the
same `add_binary_blob` + `insert_image_block_after` pair — so no new document
command is needed at all. Until that exists, OpenDoc has no
insert-from-URL, and says so, rather than shipping a command that works in one
runtime and fails in the other.

## Consequences

* DOCX and Google Docs import/export can map size and wrap to a typed model
  rather than dropping it; everything outside the subset in §2 must warn.
* The editor resizes by dragging a computed hit zone. There are **no handle
  elements**: the document surface is re-rendered through a keyed morph that
  deletes anything the renderer did not produce, so a handle element would be
  eaten by the next render. The drag previews into the very `style` attribute
  the renderer emits, and commits exactly one command on mouse-up.
* Pagination measures a floated figure's box but cannot know how the text
  beside it reflows across a page boundary. Wrapped images near a page break
  may paginate imprecisely; that is a known limit of measuring a flow rather
  than laying one out, and is not hidden behind a plausible-looking guess.
