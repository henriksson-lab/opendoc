# ADR 0042: Spreadsheet cell wrap and vertical alignment are bounded cell format

Spreadsheet display format is signed cell source state. The first extension is
intentionally narrow: `wrap_strategy` is either absent (the existing clipped
single-line projection) or `wrap`; `vertical_align` is absent or one of `top`,
`middle`, and `bottom`. Absence preserves existing output and is not a second
"default" value in the document.

This maps exactly to Google Sheets `WRAP` and `TOP`/`MIDDLE`/`BOTTOM`, and to
XLSX text-wrap and vertical alignment. The grid and the paper renderer both
consume the property. Paper wrapping uses the cell's fixed stored row height;
it clips lines that do not fit and never invents an automatic row-height
policy. This is the same bounded behavior as an explicitly sized spreadsheet
row, and makes PDF output a projection of source state rather than an export
side mutation.

Overflow/spill, clip as an explicit source choice, shrink-to-fit, text
rotation, rich text, conditional formats, background/border paint and automatic
row resizing remain unmodeled. XLSX value import cannot read style records with
the current bounded importer, so it must not claim styles were recovered; XLSX
export does write this exact subset. Unsupported Google wrap values are not
coerced to `wrap`.
