//! A deliberately separate paper view of a workbook.
//!
//! A spreadsheet is not an OpenDoc text document with a very large table: it
//! has its own evaluated display values, tabs, hidden axes and useful print
//! boundaries.  Keeping this renderer here makes that distinction explicit.

use std::collections::{BTreeMap, BTreeSet};

use opendoc_core::ModelWarning;
use opendoc_spreadsheet::{
    range_contains_column, range_contains_row, CellFormat, Sheet, SheetPrintOrientation,
    SpreadsheetWorkbook, DEFAULT_COLUMN_WIDTH_PX, DEFAULT_ROW_HEIGHT_PX,
};
use pdf_writer::{Content, Finish, Name, Pdf, Rect, Ref, Str};

use crate::PdfExport;

const LETTER_WIDTH: f32 = 612.0;
const LETTER_HEIGHT: f32 = 792.0;
const LEFT: f32 = 36.0;
const TOP: f32 = 36.0;
const RIGHT: f32 = 36.0;
const BOTTOM: f32 = 36.0;
const HEADER_HEIGHT: f32 = 20.0;
/// CSS pixels are the spreadsheet model's unit. PDF user space is points.
const PX_TO_PT: f32 = 0.75;
const ROW_LABEL_WIDTH: f32 = 30.0;

/// Exports every visible sheet as a paginated Letter PDF using its own
/// durable orientation.
///
/// Cells use their evaluated `display_value`, falling back to the computed
/// value for workbooks written by an older importer. Empty trailing grid is
/// intentionally omitted: paper contains the used range, not a million blank
/// spreadsheet rows.
pub fn export_spreadsheet_pdf(workbook: &SpreadsheetWorkbook) -> PdfExport {
    let workbook = workbook.evaluated();
    let mut warnings = Vec::new();
    if workbook.sheets.iter().any(|sheet| {
        !sheet.hidden
            && (sheet.print_settings.print_area.is_some()
                || sheet.print_settings.orientation != SheetPrintOrientation::Landscape)
    }) {
        warnings.push(ModelWarning {
            code: "pdf-spreadsheet-print-settings-unavailable".to_string(),
            message: "spreadsheet PDF uses fixed Letter paper and each sheet's selected orientation and print area; scaling, margins, headers/footers, print titles, and manual page breaks are unavailable".to_string(),
        });
    }
    for sheet in workbook.sheets.iter().filter(|sheet| !sheet.hidden) {
        if !sheet.images.is_empty() {
            warnings.push(ModelWarning {
                code: "pdf-spreadsheet-images-unavailable".to_string(),
                message: format!(
                    "sheet {:?} has {} floating image{}; spreadsheet PDF has no blob-store input yet, so the image{} was not painted",
                    sheet.title,
                    sheet.images.len(),
                    if sheet.images.len() == 1 { "" } else { "s" },
                    if sheet.images.len() == 1 { "" } else { "s" },
                ),
            });
        }
    }
    let pages = workbook
        .sheets
        .iter()
        .filter(|sheet| !sheet.hidden)
        .flat_map(|sheet| sheet_pages(sheet, &mut warnings))
        .collect::<Vec<_>>();

    if pages.is_empty() {
        warnings.push(ModelWarning {
            code: "pdf-spreadsheet-no-visible-sheets".to_string(),
            message: "the workbook has no visible sheets to export".to_string(),
        });
    }
    write_pdf(&pages, &mut warnings)
}

#[derive(Default)]
struct Page {
    paper: Paper,
    title: String,
    columns: Vec<String>,
    column_widths: Vec<f32>,
    rows: Vec<String>,
    row_heights: Vec<f32>,
    values: BTreeMap<(String, String), String>,
    formats: BTreeMap<(String, String), CellFormat>,
}

#[derive(Clone, Copy)]
struct Paper {
    width: f32,
    height: f32,
}

impl Paper {
    fn for_sheet(sheet: &Sheet) -> Self {
        match sheet.print_settings.orientation {
            SheetPrintOrientation::Portrait => Self {
                width: LETTER_WIDTH,
                height: LETTER_HEIGHT,
            },
            SheetPrintOrientation::Landscape => Self {
                width: LETTER_HEIGHT,
                height: LETTER_WIDTH,
            },
        }
    }

    const LANDSCAPE: Self = Self {
        width: LETTER_HEIGHT,
        height: LETTER_WIDTH,
    };
}

impl Default for Paper {
    fn default() -> Self {
        Self::LANDSCAPE
    }
}

fn sheet_pages(sheet: &Sheet, warnings: &mut Vec<ModelWarning>) -> Vec<Page> {
    let paper = Paper::for_sheet(sheet);
    let hidden_rows = sheet.hidden_rows.iter().collect::<BTreeSet<_>>();
    let hidden_columns = sheet.hidden_columns.iter().collect::<BTreeSet<_>>();
    let print_area = sheet.print_settings.print_area.as_deref();
    let rows = sheet
        .rows
        .iter()
        .filter(|row| {
            !hidden_rows.contains(row)
                && print_area.is_none_or(|range| range_contains_row(range, row))
        })
        .cloned()
        .collect::<Vec<_>>();
    let columns = sheet
        .columns
        .iter()
        .filter(|column| {
            !hidden_columns.contains(column)
                && print_area.is_none_or(|range| range_contains_column(range, column))
        })
        .cloned()
        .collect::<Vec<_>>();
    let values = sheet
        .cells
        .iter()
        .filter_map(|cell| {
            let (column, row) = split_address(&cell.address)?;
            if hidden_rows.contains(&row)
                || hidden_columns.contains(&column)
                || print_area.is_some_and(|range| {
                    !range_contains_row(range, &row) || !range_contains_column(range, &column)
                })
            {
                return None;
            }
            let value = if cell.display_value.is_empty() {
                &cell.computed_value
            } else {
                &cell.display_value
            };
            (!value.is_empty()).then(|| ((column, row), value.clone()))
        })
        .collect::<BTreeMap<_, _>>();
    // The paper view owns its own compact projection, rather than reaching
    // back into a live workbook while writing PDF bytes. Background paint is
    // meaningful even for an otherwise blank cell, so retain every visible
    // cell format; the used-range calculation below admits the ones that
    // actually have a background. Other format-only facts remain geometry or
    // text behaviour and cannot by themselves create paper content.
    let formats = sheet
        .cells
        .iter()
        .filter_map(|cell| {
            let (column, row) = split_address(&cell.address)?;
            (!hidden_rows.contains(&row)
                && !hidden_columns.contains(&column)
                && print_area.is_none_or(|range| {
                    range_contains_row(range, &row) && range_contains_column(range, &column)
                }))
            .then(|| ((column, row), cell.format.clone()))
        })
        .collect::<BTreeMap<_, _>>();
    let used_rows = rows
        .iter()
        .filter(|row| {
            values.keys().any(|(_, value_row)| value_row == *row)
                || formats.iter().any(|((_, format_row), format)| {
                    format_row == *row && format.background_color.is_some()
                })
        })
        .cloned()
        .collect::<Vec<_>>();
    let used_columns = columns
        .iter()
        .filter(|column| {
            values
                .keys()
                .any(|(value_column, _)| value_column == *column)
                || formats.iter().any(|((format_column, _), format)| {
                    format_column == *column && format.background_color.is_some()
                })
        })
        .cloned()
        .collect::<Vec<_>>();
    if used_rows.is_empty() || used_columns.is_empty() {
        return vec![Page {
            title: sheet.title.clone(),
            paper,
            ..Default::default()
        }];
    }

    let available_width = paper.width - LEFT - RIGHT - ROW_LABEL_WIDTH;
    let available_height = paper.height - TOP - BOTTOM - 50.0 - HEADER_HEIGHT;
    let column_pages = paginate_axis(&used_columns, available_width, |column| {
        axis_points(
            sheet.column_widths.get(column).copied(),
            DEFAULT_COLUMN_WIDTH_PX,
        )
    });
    // Frozen rows are the only existing sheet-owned declaration that has an
    // unambiguous paper meaning.  Repeating them here deliberately reuses the
    // durable viewport state rather than inventing a PDF-only "header row"
    // switch.  Hidden frozen rows are not printed (the paper view already
    // omits hidden axes); the remaining leading visible frozen rows form the
    // repeated heading band.
    let frozen_rows = sheet
        .rows
        .iter()
        .take(sheet.frozen_rows as usize)
        .filter(|row| !hidden_rows.contains(*row))
        .cloned()
        .collect::<Vec<_>>();
    let frozen_height = frozen_rows
        .iter()
        .map(|row| axis_points(sheet.row_heights.get(row).copied(), DEFAULT_ROW_HEIGHT_PX))
        .sum::<f32>();
    let body_rows = used_rows
        .iter()
        .filter(|row| !frozen_rows.contains(*row))
        .cloned()
        .collect::<Vec<_>>();
    if frozen_height >= available_height && !frozen_rows.is_empty() {
        warnings.push(ModelWarning {
            code: "pdf-spreadsheet-frozen-rows-overflow".to_string(),
            message: format!(
                "sheet {} frozen rows exceed one printable page; rows are clipped without a scale policy",
                sheet.title
            ),
        });
    }
    let mut row_pages = paginate_axis(
        &body_rows,
        (available_height - frozen_height).max(1.0),
        |row| axis_points(sheet.row_heights.get(row).copied(), DEFAULT_ROW_HEIGHT_PX),
    );
    // A sheet containing only its frozen heading still has one printable page.
    if row_pages.is_empty() {
        row_pages.push((Vec::new(), Vec::new()));
    }
    for (page_rows, page_heights) in &mut row_pages {
        let mut repeated_rows = frozen_rows.clone();
        let mut repeated_heights = frozen_rows
            .iter()
            .map(|row| axis_points(sheet.row_heights.get(row).copied(), DEFAULT_ROW_HEIGHT_PX))
            .collect::<Vec<_>>();
        repeated_rows.append(page_rows);
        repeated_heights.append(page_heights);
        *page_rows = repeated_rows;
        *page_heights = repeated_heights;
    }
    let mut pages = Vec::new();
    for (columns, column_widths) in &column_pages {
        for (rows, row_heights) in &row_pages {
            pages.push(Page {
                title: sheet.title.clone(),
                paper,
                columns: columns.clone(),
                column_widths: column_widths.clone(),
                rows: rows.clone(),
                row_heights: row_heights.clone(),
                values: values.clone(),
                formats: formats.clone(),
            });
        }
    }
    if sheet
        .cells
        .iter()
        .any(|cell| !is_pdf_text(&cell.display_value))
    {
        warnings.push(ModelWarning {
            code: "pdf-spreadsheet-non-latin-text".to_string(),
            message: format!(
                "sheet {} contains text outside the PDF base-font character set; unsupported characters were replaced",
                sheet.title
            ),
        });
    }
    pages
}

/// Splits an axis at the PDF's usable edge while retaining every stored size.
/// A single axis item wider/taller than one printable page is put on its own
/// page and clipped at the edge; the model permits a 2,000px row/column, while
/// a letter-size page plainly cannot represent that length without inventing a
/// scale policy. Normal explicit dimensions therefore round-trip into paper
/// exactly, and pathological ones remain visibly bounded rather than causing
/// an empty page or an arithmetic overflow.
fn paginate_axis<F>(labels: &[String], available: f32, mut size: F) -> Vec<(Vec<String>, Vec<f32>)>
where
    F: FnMut(&String) -> f32,
{
    let mut pages = Vec::new();
    let mut page_labels = Vec::new();
    let mut page_sizes = Vec::new();
    let mut used = 0.0;
    for label in labels {
        let points = size(label);
        if !page_labels.is_empty() && used + points > available {
            pages.push((
                std::mem::take(&mut page_labels),
                std::mem::take(&mut page_sizes),
            ));
            used = 0.0;
        }
        used += points;
        page_labels.push(label.clone());
        page_sizes.push(points);
    }
    if !page_labels.is_empty() {
        pages.push((page_labels, page_sizes));
    }
    pages
}

fn axis_points(stored: Option<u32>, default_px: u32) -> f32 {
    stored.unwrap_or(default_px) as f32 * PX_TO_PT
}

fn write_pdf(pages: &[Page], warnings: &mut Vec<ModelWarning>) -> PdfExport {
    let mut pdf = Pdf::new();
    let mut next = 1i32;
    let mut allocate = || {
        let id = Ref::new(next);
        next += 1;
        id
    };
    let catalog = allocate();
    let tree = allocate();
    let font = allocate();
    let bold_font = allocate();
    let italic_font = allocate();
    let bold_italic_font = allocate();
    let page_ids = (0..pages.len().max(1))
        .map(|_| allocate())
        .collect::<Vec<_>>();
    let content_ids = page_ids.iter().map(|_| allocate()).collect::<Vec<_>>();
    pdf.catalog(catalog).pages(tree);
    pdf.pages(tree)
        .kids(page_ids.iter().copied())
        .count(page_ids.len() as i32);
    pdf.type1_font(font).base_font(Name(b"Helvetica"));
    pdf.type1_font(bold_font).base_font(Name(b"Helvetica-Bold"));
    pdf.type1_font(italic_font)
        .base_font(Name(b"Helvetica-Oblique"));
    pdf.type1_font(bold_italic_font)
        .base_font(Name(b"Helvetica-BoldOblique"));
    for (index, (page_id, content_id)) in page_ids.iter().zip(&content_ids).enumerate() {
        let paper = pages
            .get(index)
            .map(|page| page.paper)
            .unwrap_or(Paper::LANDSCAPE);
        let mut pdf_page = pdf.page(*page_id);
        pdf_page
            .parent(tree)
            .media_box(Rect::new(0.0, 0.0, paper.width, paper.height))
            .contents(*content_id);
        let mut resources = pdf_page.resources();
        let mut fonts = resources.fonts();
        fonts.pair(Name(b"F1"), font);
        fonts.pair(Name(b"F2"), bold_font);
        fonts.pair(Name(b"F3"), italic_font);
        fonts.pair(Name(b"F4"), bold_italic_font);
        fonts.finish();
        resources.finish();
        pdf_page.finish();
        let content = pages
            .get(index)
            .map(draw_page)
            .unwrap_or_else(|| empty_page(Paper::LANDSCAPE));
        pdf.stream(*content_id, &content);
    }
    if pages.is_empty() {
        warnings.push(ModelWarning {
            code: "pdf-spreadsheet-empty-export".to_string(),
            message: "the PDF contains one blank page because no sheet was visible".to_string(),
        });
    }
    PdfExport {
        bytes: pdf.finish(),
        warnings: std::mem::take(warnings),
    }
}

fn empty_page(paper: Paper) -> Vec<u8> {
    let mut content = Content::new();
    text(
        &mut content,
        "No visible sheets",
        LEFT,
        paper.height - TOP,
        14.0,
        Name(b"F1"),
    );
    content.finish().to_vec()
}

fn draw_page(page: &Page) -> Vec<u8> {
    let mut content = Content::new();
    text(
        &mut content,
        &page.title,
        LEFT,
        page.paper.height - TOP,
        14.0,
        Name(b"F1"),
    );
    if page.columns.is_empty() {
        text(
            &mut content,
            "This sheet has no populated visible cells.",
            LEFT,
            page.paper.height - TOP - 28.0,
            10.0,
            Name(b"F1"),
        );
        return content.finish().to_vec();
    }
    let grid_top = page.paper.height - TOP - 28.0;
    let grid_width = ROW_LABEL_WIDTH + page.column_widths.iter().sum::<f32>();
    let grid_height = HEADER_HEIGHT + page.row_heights.iter().sum::<f32>();
    // Paint durable cell backgrounds before the gridlines so the grid remains
    // legible. This includes intentionally blank, formatted cells in a print
    // area; `sheet_pages` has already restricted the page to its visible
    // rectangular projection.
    let mut fill_top = grid_top - HEADER_HEIGHT;
    for (row_index, row) in page.rows.iter().enumerate() {
        let height = page.row_heights[row_index];
        let mut fill_x = LEFT + ROW_LABEL_WIDTH;
        for (column_index, column) in page.columns.iter().enumerate() {
            let width = page.column_widths[column_index];
            if let Some(color) = page
                .formats
                .get(&(column.clone(), row.clone()))
                .and_then(|format| format.background_color.as_deref())
                .and_then(pdf_rgb)
            {
                content
                    .set_fill_rgb(color[0], color[1], color[2])
                    .rect(fill_x, fill_top - height, width, height)
                    .fill_nonzero();
            }
            fill_x += width;
        }
        fill_top -= height;
    }
    content.set_line_width(0.35).set_stroke_gray(0.65);
    let mut x = LEFT + ROW_LABEL_WIDTH;
    content
        .move_to(x, grid_top)
        .line_to(x, grid_top - grid_height)
        .stroke();
    for width in &page.column_widths {
        x += width;
        content
            .move_to(x, grid_top)
            .line_to(x, grid_top - grid_height)
            .stroke();
    }
    content
        .move_to(LEFT, grid_top)
        .line_to(LEFT, grid_top - grid_height)
        .stroke();
    let mut y = grid_top;
    content
        .move_to(LEFT, y)
        .line_to(LEFT + grid_width, y)
        .stroke();
    y -= HEADER_HEIGHT;
    content
        .move_to(LEFT, y)
        .line_to(LEFT + grid_width, y)
        .stroke();
    for height in &page.row_heights {
        y -= height;
        content
            .move_to(LEFT, y)
            .line_to(LEFT + grid_width, y)
            .stroke();
    }
    // Background painting changes the PDF fill colour; headers and row labels
    // are workbook chrome, not cell text, and remain neutral black.
    content.set_fill_gray(0.0);
    let mut x = LEFT + ROW_LABEL_WIDTH;
    for (index, column) in page.columns.iter().enumerate() {
        text(
            &mut content,
            column,
            x + 3.0,
            grid_top - 14.0,
            8.0,
            Name(b"F1"),
        );
        x += page.column_widths[index];
    }
    let mut row_top = grid_top - HEADER_HEIGHT;
    for (row_index, row) in page.rows.iter().enumerate() {
        let baseline = row_top - 13.0;
        text(&mut content, row, LEFT + 3.0, baseline, 8.0, Name(b"F1"));
        let mut x = LEFT + ROW_LABEL_WIDTH;
        for (column_index, column) in page.columns.iter().enumerate() {
            let value = page
                .values
                .get(&(column.clone(), row.clone()))
                .map(String::as_str)
                .unwrap_or("");
            let format = page.formats.get(&(column.clone(), row.clone()));
            let width = page.column_widths[column_index];
            let lines = if format.and_then(|format| format.wrap_strategy.as_deref()) == Some("wrap")
            {
                wrap_pdf_text(value, ((width - 6.0) / 4.5) as usize)
            } else {
                vec![truncate_pdf_text(value, ((width - 6.0) / 4.5) as usize)]
            };
            // 9pt leading leaves a narrow but deliberate readable gap at 8pt.
            // A fixed-height spreadsheet row clips surplus wrapped lines just
            // as an explicitly sized spreadsheet row does on screen.
            let max_lines = (page.row_heights[row_index] / 9.0).floor().max(1.0) as usize;
            let lines = &lines[..lines.len().min(max_lines)];
            let text_height = lines.len() as f32 * 9.0;
            let first_baseline = match format.and_then(|format| format.vertical_align.as_deref()) {
                Some("top") => row_top - 10.0,
                Some("middle") => row_top - (page.row_heights[row_index] - text_height) / 2.0 - 7.0,
                _ => row_top - page.row_heights[row_index] + text_height - 2.0,
            };
            for (line_index, line) in lines.iter().enumerate() {
                let text_width = line.chars().count() as f32 * 4.5;
                let text_x = match format.and_then(|format| format.horizontal_align.as_deref()) {
                    Some("center") => x + ((width - text_width) / 2.0).max(3.0),
                    Some("right") => x + (width - text_width - 3.0).max(3.0),
                    _ => x + 3.0,
                };
                if let Some(color) = format
                    .and_then(|format| format.text_color.as_deref())
                    .and_then(pdf_rgb)
                {
                    content.set_fill_rgb(color[0], color[1], color[2]);
                } else {
                    content.set_fill_gray(0.0);
                }
                text(
                    &mut content,
                    line,
                    text_x,
                    first_baseline - line_index as f32 * 9.0,
                    8.0,
                    spreadsheet_font(format),
                );
            }
            x += page.column_widths[column_index];
        }
        row_top -= page.row_heights[row_index];
    }
    content.finish().to_vec()
}

fn text(content: &mut Content, value: &str, x: f32, y: f32, size: f32, font: Name<'_>) {
    content
        .begin_text()
        .set_font(font, size)
        .next_line(x, y)
        .show(Str(&pdf_text(value)))
        .end_text();
}

/// The durable spreadsheet model has only boolean bold/italic font facts.
/// Map exactly those four combinations to PDF's Base-14 Helvetica faces. We
/// deliberately do not infer a family, weight, or slant that the model does
/// not carry.
fn spreadsheet_font(format: Option<&CellFormat>) -> Name<'static> {
    match format {
        Some(format) if format.bold && format.italic => Name(b"F4"),
        Some(format) if format.bold => Name(b"F2"),
        Some(format) if format.italic => Name(b"F3"),
        _ => Name(b"F1"),
    }
}

/// Converts the model's already-validated `#rrggbb` paint into PDF DeviceRGB
/// components. Keep this defensive because callers can construct a workbook
/// directly before source validation; an invalid colour must not make PDF
/// generation emit malformed numeric operators.
fn pdf_rgb(color: &str) -> Option<[f32; 3]> {
    let value = color.strip_prefix('#')?;
    if value.len() != 6 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    Some([
        u8::from_str_radix(&value[0..2], 16).ok()? as f32 / 255.0,
        u8::from_str_radix(&value[2..4], 16).ok()? as f32 / 255.0,
        u8::from_str_radix(&value[4..6], 16).ok()? as f32 / 255.0,
    ])
}

fn split_address(address: &str) -> Option<(String, String)> {
    let point = address.find(|character: char| character.is_ascii_digit())?;
    let (column, row) = address.split_at(point);
    (!column.is_empty() && !row.is_empty()).then(|| (column.to_string(), row.to_string()))
}

fn is_pdf_text(value: &str) -> bool {
    value
        .chars()
        .all(|character| character.is_ascii() || ('\u{a0}'..='\u{ff}').contains(&character))
}

fn pdf_text(value: &str) -> Vec<u8> {
    value
        .chars()
        .map(|character| {
            if character.is_ascii() || ('\u{a0}'..='\u{ff}').contains(&character) {
                character as u8
            } else {
                b'?'
            }
        })
        .collect()
}

fn truncate_pdf_text(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_string();
    }
    value
        .chars()
        .take(max.saturating_sub(1))
        .collect::<String>()
        + "..."
}

/// Word-wrap a PDF cell without claiming an automatic row-height policy. Long
/// unbroken values are split at the same coarse glyph width used by the paper
/// grid's existing truncation calculation.
fn wrap_pdf_text(value: &str, max: usize) -> Vec<String> {
    let max = max.max(1);
    let mut lines = Vec::new();
    for paragraph in value.split('\n') {
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            let word_len = word.chars().count();
            let needed = if line.is_empty() {
                word_len
            } else {
                word_len + 1
            };
            if !line.is_empty() && line.chars().count() + needed > max {
                lines.push(std::mem::take(&mut line));
            }
            if word.chars().count() > max {
                if !line.is_empty() {
                    lines.push(std::mem::take(&mut line));
                }
                let chars = word.chars().collect::<Vec<_>>();
                for chunk in chars.chunks(max) {
                    lines.push(chunk.iter().collect());
                }
            } else {
                if !line.is_empty() {
                    line.push(' ');
                }
                line.push_str(word);
            }
        }
        if !line.is_empty() || paragraph.is_empty() {
            lines.push(line);
        }
    }
    lines
}
