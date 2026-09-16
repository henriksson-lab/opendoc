//! Spreadsheet grid projection: sheets, addresses and merges.
//!
//! ## Hidden rows and columns are not drawn here
//!
//! A hidden axis is a fact about the sheet (`hidden_rows` / `hidden_columns`),
//! so deciding what the grid contains is this projection's job, not the
//! frontend's. The markup a hidden row or column would have produced is simply
//! never written: no `<tr>`, no `<th>`, no `<td>`, and no `<col>`. Hiding is
//! still not deletion — the cells, their formulas and their stored sizes are
//! untouched in the model, and `SUBTOTAL(101..)` reading past a hidden row is
//! the evaluator's business.
//!
//! Two things have to stay in step with the omission: the `<colgroup>`, whose
//! `<col>` elements map to columns *by position* and would therefore shift
//! every width one column left if a hidden column kept its entry; and the
//! column spans of a merge, which count cells that may no longer be drawn.

use crate::html::{attr, css_value};
use crate::*;

/// Render one workbook sheet as an HTML table.
pub fn render_workbook_html(
    workbook: &SpreadsheetWorkbook,
    sheet_id: &str,
) -> Result<String, RenderError> {
    let sheet = workbook
        .sheets
        .iter()
        .find(|sheet| sheet.id == sheet_id)
        .ok_or_else(|| RenderError::NotFound(format!("sheet {sheet_id} was not found")))?;
    let cells: BTreeMap<&str, &Cell> = sheet
        .cells
        .iter()
        .map(|cell| (cell.address.as_str(), cell))
        .collect();
    let hidden_rows: BTreeSet<&str> = sheet.hidden_rows.iter().map(String::as_str).collect();
    let hidden_columns: BTreeSet<&str> = sheet.hidden_columns.iter().map(String::as_str).collect();
    let drawn_columns: Vec<(usize, &String)> = sheet
        .columns
        .iter()
        .enumerate()
        .filter(|(_, column)| !hidden_columns.contains(column.as_str()))
        .collect();

    // A merge is a rectangle of addresses; hiding an axis takes a slice out of
    // it. The spans therefore count only the rows and columns that are still
    // drawn, and the block is emitted at the first drawn corner of the
    // rectangle rather than always at its top-left — otherwise hiding the
    // top-left column would leave the rest of the rectangle covered by a cell
    // that is never written, and the row would come out one `<td>` short of
    // the header.
    //
    // The content and formatting still come from the merge's own anchor, which
    // is where the model stores them, so what the block shows does not change
    // when the column it is drawn in does.
    let mut covered: BTreeSet<String> = BTreeSet::new();
    let mut spans: BTreeMap<String, (usize, usize, String)> = BTreeMap::new();
    for merge in &sheet.merges {
        if let Some(((c1, r1), (c2, r2))) = parse_range(&merge.range) {
            let columns: Vec<usize> = (c1..=c2)
                .filter(|column| !hidden_columns.contains(column_label(*column).as_str()))
                .collect();
            let rows: Vec<usize> = (r1..=r2)
                .filter(|row| !hidden_rows.contains(row.to_string().as_str()))
                .collect();
            let (Some(first_column), Some(first_row)) = (columns.first(), rows.first()) else {
                // Every cell of the merge is hidden; nothing is drawn and
                // nothing needs covering.
                continue;
            };
            let anchor = format!("{}{}", column_label(c1), r1);
            let drawn_at = format!("{}{}", column_label(*first_column), first_row);
            spans.insert(drawn_at.clone(), (columns.len(), rows.len(), anchor));
            for column in &columns {
                for row in &rows {
                    let address = format!("{}{}", column_label(*column), row);
                    if address != drawn_at {
                        covered.insert(address);
                    }
                }
            }
        }
    }

    let mut out = String::new();
    let _ = write!(
        out,
        "<table class=\"sheet-grid\" data-sheet-id=\"{}\">",
        attr(&sheet.id)
    );
    // Explicit column widths ride on a <colgroup>; sheets without any stored
    // width on a drawn column emit none at all and fall back to the stylesheet
    // default. One `<col>` per drawn column, in drawn order: the mapping is
    // positional, so a hidden column must not keep a placeholder.
    if drawn_columns
        .iter()
        .any(|(_, column)| sheet.column_widths.contains_key(*column))
    {
        out.push_str("<colgroup><col class=\"row-header-col\">");
        for (_, column) in &drawn_columns {
            match sheet.column_widths.get(*column) {
                Some(width) => {
                    let _ = write!(
                        out,
                        "<col data-column=\"{}\" style=\"width:{width}px\">",
                        attr(column)
                    );
                }
                None => {
                    let _ = write!(out, "<col data-column=\"{}\">", attr(column));
                }
            }
        }
        out.push_str("</colgroup>");
    }
    out.push_str("<thead><tr><th class=\"corner row-header\"></th>");
    // `index` is the column's position in the sheet, not in the drawn set:
    // "the first two columns are frozen" is a fact about the sheet, and hiding
    // one of them does not promote a third.
    for (index, column) in drawn_columns.iter().copied() {
        let frozen = (index as u32) < sheet.frozen_columns;
        let _ = write!(
            out,
            "<th class=\"column-header{}\" data-column=\"{}\">{}</th>",
            if frozen { " frozen" } else { "" },
            attr(column),
            escape_html(column)
        );
    }
    out.push_str("</tr></thead><tbody>");
    for (row_index, row) in sheet.rows.iter().enumerate() {
        if hidden_rows.contains(row.as_str()) {
            continue;
        }
        let frozen_row = (row_index as u32) < sheet.frozen_rows;
        let _ = write!(
            out,
            "<tr data-row=\"{}\"{}{}><th class=\"row-header\">{}</th>",
            attr(row),
            if frozen_row {
                " class=\"frozen-row\""
            } else {
                ""
            },
            // Only rows with a stored height carry one; the rest inherit the
            // stylesheet default.
            match sheet.row_heights.get(row) {
                Some(height) => format!(" style=\"height:{height}px\""),
                None => String::new(),
            },
            escape_html(row)
        );
        for (column_index, column) in drawn_columns.iter().copied() {
            let position = format!("{column}{row}");
            if covered.contains(&position) {
                continue;
            }
            let span = spans.get(&position);
            // A merged block shows its anchor's cell wherever it is drawn, and
            // names that anchor: the address on the element is the one a click
            // must select and the one the formula bar must edit.
            let address = match span {
                Some((_, _, anchor)) => anchor.clone(),
                None => position.clone(),
            };
            let frozen = (column_index as u32) < sheet.frozen_columns;
            let cell = cells.get(address.as_str()).copied();
            let kind = cell
                .map(|cell| cell.computed_kind.as_str())
                .unwrap_or("empty");
            // `display_value` is the formatted projection the evaluator
            // derives from `computed_value` and the cell's number format
            // (and the one that spells booleans TRUE/FALSE). It is empty
            // only for cells the evaluator has never seen, in which case
            // the raw computed value is the best available text.
            let display = cell
                .map(|cell| {
                    if cell.computed_kind == "empty" {
                        cell.user_value.clone()
                    } else if cell.display_value.is_empty() {
                        cell.computed_value.clone()
                    } else {
                        cell.display_value.clone()
                    }
                })
                .unwrap_or_default();
            let mut classes = Vec::new();
            if frozen {
                classes.push("frozen");
            }
            let mut style = String::new();
            if let Some(cell) = cell {
                if cell.format.bold {
                    classes.push("mark-bold");
                }
                if cell.format.italic {
                    classes.push("mark-italic");
                }
                if let Some(color) = cell.format.text_color.as_deref().and_then(css_value) {
                    let _ = write!(style, "color:{color};");
                }
                if let Some(color) = cell.format.background_color.as_deref().and_then(css_value) {
                    let _ = write!(style, "background-color:{color};");
                }
                if let Some(align) = cell.format.horizontal_align.as_deref() {
                    if matches!(align, "left" | "center" | "right") {
                        let _ = write!(style, "text-align:{align};");
                    }
                }
                if cell.format.wrap_strategy.as_deref() == Some("wrap") {
                    classes.push("text-wrap");
                }
                if let Some(align) = cell.format.vertical_align.as_deref() {
                    let _ = write!(style, "vertical-align:{align};");
                }
            }
            let _ = write!(
                out,
                "<td data-address=\"{}\" data-kind=\"{}\"",
                attr(&address),
                attr(kind)
            );
            if !classes.is_empty() {
                let _ = write!(out, " class=\"{}\"", classes.join(" "));
            }
            if !style.is_empty() {
                let _ = write!(out, " style=\"{style}\"");
            }
            if let Some((colspan, rowspan, _)) = span {
                // A one-by-one remnant of a merge is not a span at all; the
                // attributes would be `colspan="1" rowspan="1"`, which says
                // nothing the markup does not already say.
                if *colspan > 1 || *rowspan > 1 {
                    let _ = write!(out, " colspan=\"{colspan}\" rowspan=\"{rowspan}\"");
                }
            }
            if let Some(cell) = cell {
                if cell.user_value.starts_with('=') {
                    let _ = write!(out, " data-formula=\"{}\"", attr(&cell.user_value));
                }
                if let Some(validation) = &cell.validation {
                    let _ = write!(out, " data-validation=\"{}\"", attr(&validation.kind));
                }
            }
            out.push('>');
            out.push_str(&escape_html(&display));
            if cell
                .map(|cell| cell.comments.iter().any(|comment| !comment.deleted))
                .unwrap_or(false)
            {
                out.push_str("<span class=\"cell-comment-marker\" title=\"Has comments\"></span>");
            }
            out.push_str("</td>");
        }
        out.push_str("</tr>");
    }
    out.push_str("</tbody></table>");
    // The renderer intentionally emits geometry and blob identity only.  The
    // app owns image bytes and fills a safe data URL after this pure workbook
    // projection returns (ADR 0045).
    out.push_str("<div class=\"sheet-image-layer\" aria-label=\"Spreadsheet images\">");
    for image in &sheet.images {
        let _ = write!(
            out,
            "<img class=\"sheet-image\" data-sheet-image-id=\"{}\" data-blob-hash=\"{}\" data-start-column=\"{}\" data-start-row=\"{}\" data-start-offset-x=\"{}\" data-start-offset-y=\"{}\" data-end-column=\"{}\" data-end-row=\"{}\" data-end-offset-x=\"{}\" data-end-offset-y=\"{}\" alt=\"\">",
            attr(&image.id), attr(&image.blob_hash), attr(&image.start_column), attr(&image.start_row),
            image.start_offset_x_px, image.start_offset_y_px, attr(&image.end_column), attr(&image.end_row),
            image.end_offset_x_px, image.end_offset_y_px,
        );
    }
    out.push_str("</div>");
    Ok(out)
}

pub(crate) fn column_label(mut index: usize) -> String {
    let mut label = String::new();
    while index > 0 {
        let remainder = (index - 1) % 26;
        label.insert(0, (b'A' + remainder as u8) as char);
        index = (index - 1) / 26;
    }
    label
}

pub(crate) fn parse_address(address: &str) -> Option<(usize, usize)> {
    let letters: String = address
        .chars()
        .take_while(|ch| ch.is_ascii_alphabetic())
        .collect();
    let digits: String = address.chars().skip(letters.len()).collect();
    if letters.is_empty() || digits.is_empty() {
        return None;
    }
    let mut column = 0usize;
    for ch in letters.chars() {
        column = column * 26 + (ch.to_ascii_uppercase() as usize - 'A' as usize + 1);
    }
    Some((column, digits.parse().ok()?))
}

pub(crate) fn parse_range(range: &str) -> Option<((usize, usize), (usize, usize))> {
    let (start, end) = range.split_once(':')?;
    Some((parse_address(start)?, parse_address(end)?))
}
