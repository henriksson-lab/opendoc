use crate::{push_unique_warning, AppBlock, AppDocument};

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
