# ADR 0045: Spreadsheet images are sheet-owned two-cell drawings

Spreadsheet drawings are not cells. A `Sheet` owns an ordered `images`
collection; each item refers to an application-owned image blob by content hash
and has a start and end cell plus non-negative pixel offsets. This is the
intersection of an XLSX `xdr:twoCellAnchor` picture and a Google Sheets
`EmbeddedObjectPosition` anchored to a grid range. The end must follow the
start in row-major order, so source state cannot contain a negative or
zero-area drawing.

The spreadsheet model intentionally owns only identity, image bytes by
reference, and geometry. It does not reuse document `ImageLayout`: page/block
placement, text wrapping, captions, borders, rotation, crop, opacity and z
index do not mean the same thing in a cell grid. The app owns and validates the
referenced blob; the spreadsheet crate validates the stable hash-shaped
reference without claiming it has the bytes.

XLSX and Google Sheets adapters may retain only raster pictures that supply a
faithful two-cell anchor and embedded bytes. Charts, shapes, connectors,
diagrams, externally linked pictures, one-cell/absolute anchors, transforms
and unsupported picture effects remain explicit import/export disclosures.
They must never be converted to images or silently folded into cells. Grid and
PDF projections place supported images over the corresponding cell rectangle;
they do not invent spreadsheet print scaling, clipping, or text-flow behavior.
