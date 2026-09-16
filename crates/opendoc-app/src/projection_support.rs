use crate::{push_unique_warning, AppBlock, AppDocument};

/// `skip_serializing_if` predicate for a nested DTO that is entirely default.
///
/// A projection of a plain paragraph is mostly absence: no alignment, no
/// indent, no line spacing, no image geometry. Writing that absence out as
/// explicit `null`s made the wire form of a 1,500-block document 1.9 MB, of
/// which 0.9 MB was nulls and empty containers — paid on every keystroke in
/// serialisation, in the WASM string copy and again in `JSON.parse`. The
/// fields are `#[serde(default)]`, so an absent field deserialises to exactly
/// the value that was skipped and the round trip is unchanged.
pub(crate) fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

pub(crate) fn clear_citation_projection_payload(blocks: &mut [AppBlock]) {
    for block in blocks {
        for inline in &mut block.content {
            if inline.kind == "citation" {
                inline.text.clear();
            }
        }
        for row in &mut block.rows {
            for cell in row {
                clear_citation_projection_payload(cell);
            }
        }
    }
}

pub(crate) fn append_spreadsheet_formula_warnings(document: &mut AppDocument) {
    for sheet in &document.workbook.sheets {
        for cell in &sheet.cells {
            if cell.user_kind == "formula" && cell.computed_kind == "error" {
                push_unique_warning(
                    &mut document.warnings,
                    "spreadsheet-formula-error",
                    format!(
                        "formula {}!{} evaluated to {}",
                        sheet.id, cell.address, cell.computed_value
                    ),
                );
            }
        }
    }
}
