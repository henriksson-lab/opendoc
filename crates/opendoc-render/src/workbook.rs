//! Spreadsheet grid projection: sheets, addresses and merges.

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

    let mut covered: BTreeSet<String> = BTreeSet::new();
    let mut spans: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for merge in &sheet.merges {
        if let Some(((c1, r1), (c2, r2))) = parse_range(&merge.range) {
            let anchor = format!("{}{}", column_label(c1), r1);
            spans.insert(anchor.clone(), (c2 - c1 + 1, r2 - r1 + 1));
            for column in c1..=c2 {
                for row in r1..=r2 {
                    let address = format!("{}{}", column_label(column), row);
                    if address != anchor {
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
    // width emit none at all and fall back to the stylesheet default.
    if !sheet.column_widths.is_empty() {
        out.push_str("<colgroup><col class=\"row-header-col\">");
        for column in &sheet.columns {
            match sheet.column_widths.get(column) {
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
    for (index, column) in sheet.columns.iter().enumerate() {
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
        for (column_index, column) in sheet.columns.iter().enumerate() {
            let address = format!("{column}{row}");
            if covered.contains(&address) {
                continue;
            }
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
            if let Some((colspan, rowspan)) = spans.get(&address) {
                let _ = write!(out, " colspan=\"{colspan}\" rowspan=\"{rowspan}\"");
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
