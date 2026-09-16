//! Fill-handle series expansion (SH-28).
//!
//! The UI hands over the source range and the range the drag covered; every
//! decision about *what* lands in the filled cells is made here. Series
//! inference is document semantics, not presentation, so it lives in Rust and
//! the frontend only reports geometry.
//!
//! Supported series, in the order they are recognised per filled line:
//!
//! * **linear numeric** — two or more numbers extrapolate by their common
//!   difference, or by a least-squares fit when the differences vary.
//! * **date** — a single date-formatted number steps by one day; several step
//!   like any other numeric series (so a two-day or one-week cadence carries).
//! * **name list** — a month or weekday name cycles through its list in the
//!   source's own spelling and case: `Jan` fills `Feb, Mar, Apr`, `MONDAY`
//!   fills `TUESDAY`. Two sources set the stride, so `Jan, Mar` fills
//!   `May, Jul`. English only, because the workbook locale carries decimal
//!   and date-order rules but no month names.
//! * **trailing integer** — text ending in digits counts up: `Item 1` fills
//!   `Item 2, Item 3`, and `Item 01` keeps its padding.
//! * **copy / constant** — anything else repeats the source cells cyclically,
//!   which covers text with no counter, booleans, a lone number, and mixed
//!   blocks.
//! * **formula with shift** — repeated formulas have their relative
//!   references moved to the cell they land in; absolute (`$`) parts stay put.
//!
//! Formatting, validation and number formats travel with the value; cell
//! comments do not, because a comment is a conversation about one cell.

use std::collections::BTreeMap;

use super::address::{cell_address, parse_cell_range, CellRange};
use super::format::is_date_format;
use super::structure::{transform_formula_for_paste, upsert_sheet_cell};
use crate::{Cell, Sheet, SpreadsheetError};

/// The axis a fill runs along.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FillAxis {
    /// Down or up: each source column is its own series.
    Vertical,
    /// Right or left: each source row is its own series.
    Horizontal,
}

/// How one line of a fill produces values outside the source block.
#[derive(Clone, Debug, PartialEq)]
enum SeriesPlan {
    /// Repeat the source cells cyclically, shifting formulas as they move.
    Repeat,
    /// `start + step * offset`, where `offset` is the signed distance from the
    /// first source cell of the line.
    Linear { start: f64, step: f64 },
    /// A cyclic list of names — the months, or the days of the week. The
    /// index wraps, so December is followed by January, and the text is
    /// written back in the case the source used.
    Names {
        list: &'static [&'static str],
        start: i64,
        step: i64,
        casing: Casing,
    },
    /// Text ending in digits: the digits count and the text before them is
    /// carried unchanged. `width` is the zero-padded width to keep, or `0`
    /// for a counter that was not padded.
    Counted {
        prefix: String,
        start: i64,
        step: i64,
        width: usize,
    },
}

/// The letter case a name series writes its names in — taken from the source
/// cell, so `JAN` fills `FEB` and `jan` fills `feb`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Casing {
    Lower,
    Upper,
    Title,
}

impl Casing {
    fn of(text: &str) -> Self {
        let letters = || text.chars().filter(|character| character.is_alphabetic());
        if letters().all(char::is_lowercase) {
            Self::Lower
        } else if letters().all(char::is_uppercase) {
            Self::Upper
        } else {
            Self::Title
        }
    }

    /// `name` is a canonical title-case entry from one of the lists.
    fn apply(self, name: &str) -> String {
        match self {
            Self::Lower => name.to_lowercase(),
            Self::Upper => name.to_uppercase(),
            Self::Title => name.to_string(),
        }
    }
}

/// The name lists, longest spelling first so `May` is read as the month
/// rather than as an abbreviation of itself — which is what makes it fill
/// `June` rather than `Jun`, as it does in Sheets.
const NAME_LISTS: [&[&str]; 4] = [
    &[
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ],
    &[
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
        "Sunday",
    ],
    &[
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ],
    &["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"],
];

/// Expands `source_range` across `target_range`.
///
/// `target_range` is the region the drag covered: it may include the source
/// block (the usual case) or sit directly beside it. It must line up with the
/// source on one axis and extend on exactly one side, so the direction of the
/// fill is never ambiguous.
pub fn fill_sheet_range(
    sheet: &mut Sheet,
    source_range: &str,
    target_range: &str,
) -> Result<(), SpreadsheetError> {
    let source = parse_cell_range(source_range)?;
    let target = parse_cell_range(target_range)?;
    let Some((axis, first, last)) = resolve_fill(source, target)? else {
        // The drag never left the source block: nothing to fill.
        return Ok(());
    };

    let existing: BTreeMap<String, Cell> = sheet
        .cells
        .iter()
        .map(|cell| (cell.address.clone(), cell.clone()))
        .collect();

    let vertical = axis == FillAxis::Vertical;
    let line_count = if vertical {
        source.width
    } else {
        source.height
    };
    let source_len = if vertical {
        source.height
    } else {
        source.width
    };
    let source_origin = if vertical {
        source.start_row
    } else {
        source.start_column
    };

    let mut filled = Vec::new();
    for line in 0..line_count {
        let address_at = |position: u32| -> Result<String, SpreadsheetError> {
            if vertical {
                cell_address(source.start_column + line, position)
            } else {
                cell_address(position, source.start_row + line)
            }
        };
        let mut sources = Vec::with_capacity(source_len as usize);
        for index in 0..source_len {
            let address = address_at(source_origin + index)?;
            sources.push((address.clone(), existing.get(&address).cloned()));
        }
        let plan = infer_series(&sources);
        for position in first..=last {
            let offset = i64::from(position) - i64::from(source_origin);
            let template_index = offset.rem_euclid(i64::from(source_len)) as u32;
            let (template_address, template) = &sources[template_index as usize];
            filled.push(build_cell(
                template.as_ref(),
                template_address,
                &address_at(position)?,
                &plan,
                offset,
            )?);
        }
    }
    for cell in filled {
        upsert_sheet_cell(sheet, cell);
    }
    Ok(())
}

/// Picks the fill axis and the inclusive span of positions to write.
///
/// `Ok(None)` means the target adds nothing to the source.
fn resolve_fill(
    source: CellRange,
    target: CellRange,
) -> Result<Option<(FillAxis, u32, u32)>, SpreadsheetError> {
    let vertical = source.start_column == target.start_column && source.width == target.width;
    let horizontal = source.start_row == target.start_row && source.height == target.height;
    if !vertical && !horizontal {
        return Err(SpreadsheetError::Format(
            "a fill target must line up with its source on one axis".to_string(),
        ));
    }
    let vertical_span = if vertical {
        fill_span(
            source.start_row,
            source.start_row + source.height - 1,
            target.start_row,
            target.start_row + target.height - 1,
        )?
    } else {
        None
    };
    let horizontal_span = if horizontal {
        fill_span(
            source.start_column,
            source.start_column + source.width - 1,
            target.start_column,
            target.start_column + target.width - 1,
        )?
    } else {
        None
    };
    match (vertical_span, horizontal_span) {
        (Some(_), Some(_)) => Err(SpreadsheetError::Format(
            "a fill target must extend along one axis only".to_string(),
        )),
        (Some((first, last)), None) => Ok(Some((FillAxis::Vertical, first, last))),
        (None, Some((first, last))) => Ok(Some((FillAxis::Horizontal, first, last))),
        (None, None) => Ok(None),
    }
}

/// The inclusive span the target adds on one axis, or `None` when it adds
/// nothing. Targets that straddle both sides, or that leave a gap, are
/// rejected: the fill direction would be a guess.
fn fill_span(
    source_first: u32,
    source_last: u32,
    target_first: u32,
    target_last: u32,
) -> Result<Option<(u32, u32)>, SpreadsheetError> {
    let grows_after = target_last > source_last;
    let grows_before = target_first < source_first;
    match (grows_before, grows_after) {
        (false, false) => Ok(None),
        (false, true) => {
            if target_first > source_last + 1 {
                return Err(SpreadsheetError::Format(
                    "a fill target must touch its source range".to_string(),
                ));
            }
            Ok(Some((source_last + 1, target_last)))
        }
        (true, false) => {
            if target_last + 1 < source_first {
                return Err(SpreadsheetError::Format(
                    "a fill target must touch its source range".to_string(),
                ));
            }
            Ok(Some((target_first, source_first - 1)))
        }
        (true, true) => Err(SpreadsheetError::Format(
            "a fill target must extend on one side of its source only".to_string(),
        )),
    }
}

/// Chooses the series for one line of source cells.
///
/// The families are tried in order and the first that recognises *every*
/// source cell wins. Numbers first, because a numeric source can never be a
/// name or a counter; then names, because `Q1` is a counter but `Jan` is not;
/// then trailing counters; then a plain repeat, which always succeeds.
fn infer_series(sources: &[(String, Option<Cell>)]) -> SeriesPlan {
    numeric_series(sources)
        .or_else(|| name_series(sources))
        .or_else(|| counted_series(sources))
        .unwrap_or(SeriesPlan::Repeat)
}

/// The trimmed text of every source cell, or `None` if any is missing or is
/// not stored as text. A name or a counter is text; a number is not, and
/// neither is a formula, whose result is not known here.
fn source_texts(sources: &[(String, Option<Cell>)]) -> Option<Vec<String>> {
    if sources.is_empty() {
        return None;
    }
    sources
        .iter()
        .map(|(_, cell)| {
            let cell = cell.as_ref()?;
            (cell.user_kind == "string").then(|| cell.user_value.trim().to_string())
        })
        .collect()
}

/// A month or weekday series, when every source names an entry of the same
/// list. One source steps by one; several set the stride, which is read
/// modulo the list so `Nov, Jan` steps by two rather than by minus ten.
fn name_series(sources: &[(String, Option<Cell>)]) -> Option<SeriesPlan> {
    let texts = source_texts(sources)?;
    let list = NAME_LISTS.into_iter().find(|list| {
        texts
            .iter()
            .all(|text| list.iter().any(|name| name.eq_ignore_ascii_case(text)))
    })?;
    let length = list.len() as i64;
    let indices = texts
        .iter()
        .map(|text| {
            list.iter()
                .position(|name| name.eq_ignore_ascii_case(text))
                .expect("the list was chosen because it contains every source") as i64
        })
        .collect::<Vec<_>>();
    let step = match indices.as_slice() {
        [_] => 1,
        [first, second, ..] => {
            let step = (second - first).rem_euclid(length);
            let uniform = indices
                .windows(2)
                .all(|pair| (pair[1] - pair[0]).rem_euclid(length) == step);
            if !uniform {
                return None;
            }
            step
        }
        [] => return None,
    };
    Some(SeriesPlan::Names {
        list,
        start: indices[0],
        step,
        casing: Casing::of(&texts[0]),
    })
}

/// A trailing-integer series, when every source is the same text followed by
/// digits. `Item 1, Item 3` steps by two; `Item 1` alone steps by one.
fn counted_series(sources: &[(String, Option<Cell>)]) -> Option<SeriesPlan> {
    let texts = source_texts(sources)?;
    let parts = texts
        .iter()
        .map(|text| split_trailing_counter(text))
        .collect::<Option<Vec<_>>>()?;
    let (prefix, first_digits) = parts.first()?;
    if parts.iter().any(|(other, _)| other != prefix) {
        return None;
    }
    let counters = parts
        .iter()
        .map(|(_, digits)| digits.parse::<i64>().ok())
        .collect::<Option<Vec<_>>>()?;
    let step = match counters.as_slice() {
        [_] => 1,
        [first, second, ..] => {
            let step = second - first;
            if !counters.windows(2).all(|pair| pair[1] - pair[0] == step) {
                return None;
            }
            step
        }
        [] => return None,
    };
    Some(SeriesPlan::Counted {
        prefix: prefix.clone(),
        start: counters[0],
        step,
        // Padding is only kept when the source actually had some: `Item 01`
        // stays two wide, `Item 1` is free to reach `Item 10`.
        width: if first_digits.len() > 1 && first_digits.starts_with('0') {
            first_digits.len()
        } else {
            0
        },
    })
}

/// `text` split into everything before its trailing run of ASCII digits and
/// the digits themselves. `None` when it does not end in a digit, or when it
/// is *only* digits — that is a number stored as text, not a counter, and
/// repeating it is the safer answer.
fn split_trailing_counter(text: &str) -> Option<(String, String)> {
    let start = text
        .char_indices()
        .rev()
        .take_while(|(_, character)| character.is_ascii_digit())
        .last()
        .map(|(index, _)| index)?;
    if start == 0 {
        return None;
    }
    // More than 15 digits will not survive an i64 round trip intact.
    let digits = &text[start..];
    if digits.len() > 15 {
        return None;
    }
    Some((text[..start].to_string(), digits.to_string()))
}

fn padded_counter(value: i64, width: usize) -> String {
    if width == 0 {
        return value.to_string();
    }
    match value < 0 {
        true => format!("-{:0>width$}", value.unsigned_abs(), width = width),
        false => format!("{value:0>width$}"),
    }
}

/// A numeric series, when every source cell holds a number. `None` hands the
/// line on to the text families.
fn numeric_series(sources: &[(String, Option<Cell>)]) -> Option<SeriesPlan> {
    let mut values = Vec::with_capacity(sources.len());
    let mut all_dates = true;
    for (_, cell) in sources {
        let cell = cell.as_ref()?;
        if cell.user_kind != "number" {
            return None;
        }
        let value = cell.user_value.trim().parse::<f64>().ok()?;
        if !value.is_finite() {
            return None;
        }
        all_dates &= cell
            .format
            .number_format
            .as_deref()
            .is_some_and(is_date_format);
        values.push(value);
    }
    match values.len() {
        0 => None,
        // A lone number copies; a lone date steps by a day, as it does in
        // every other spreadsheet.
        1 if all_dates => Some(SeriesPlan::Linear {
            start: values[0],
            step: 1.0,
        }),
        1 => Some(SeriesPlan::Repeat),
        _ => {
            let step = values[1] - values[0];
            let uniform = values
                .windows(2)
                .all(|pair| nearly_equal(pair[1] - pair[0], step));
            if uniform {
                Some(SeriesPlan::Linear {
                    start: values[0],
                    step,
                })
            } else {
                Some(least_squares(&values))
            }
        }
    }
}

/// Least-squares fit of `value = start + step * index`, used when the source
/// numbers are not an exact arithmetic progression.
fn least_squares(values: &[f64]) -> SeriesPlan {
    let count = values.len() as f64;
    let mean_index = (count - 1.0) / 2.0;
    let mean_value = values.iter().sum::<f64>() / count;
    let mut covariance = 0.0;
    let mut variance = 0.0;
    for (index, value) in values.iter().enumerate() {
        let delta_index = index as f64 - mean_index;
        covariance += delta_index * (value - mean_value);
        variance += delta_index * delta_index;
    }
    let step = if variance == 0.0 {
        0.0
    } else {
        covariance / variance
    };
    SeriesPlan::Linear {
        start: mean_value - step * mean_index,
        step,
    }
}

fn nearly_equal(left: f64, right: f64) -> bool {
    let scale = left.abs().max(right.abs()).max(1.0);
    (left - right).abs() <= scale * 1e-9
}

/// Builds one filled cell from its template and the line's series plan.
fn build_cell(
    template: Option<&Cell>,
    template_address: &str,
    address: &str,
    plan: &SeriesPlan,
    offset: i64,
) -> Result<Cell, SpreadsheetError> {
    let mut cell = match template {
        Some(template) => template.clone(),
        None => Cell::new(address, "empty", ""),
    };
    cell.address = address.to_string();
    // A filled cell inherits formatting and validation, never the source
    // cell's conversation or a spill it happened to sit in.
    cell.comments = Vec::new();
    cell.spill_source = None;
    cell.dependencies = Vec::new();
    match plan {
        SeriesPlan::Repeat => {
            if cell.user_kind == "formula" {
                cell.user_value =
                    transform_formula_for_paste(&cell.user_value, template_address, address)?;
            }
        }
        SeriesPlan::Linear { start, step } => {
            cell.user_kind = "number".to_string();
            cell.user_value = series_number_text(start + step * offset as f64);
        }
        SeriesPlan::Names {
            list,
            start,
            step,
            casing,
        } => {
            let index = (start + step * offset).rem_euclid(list.len() as i64) as usize;
            cell.user_kind = "string".to_string();
            cell.user_value = casing.apply(list[index]);
            // A name is text, not the number some template happened to hold.
            cell.format.number_format = None;
        }
        SeriesPlan::Counted {
            prefix,
            start,
            step,
            width,
        } => {
            let counter = start + step * offset;
            cell.user_kind = "string".to_string();
            cell.user_value = format!("{prefix}{}", padded_counter(counter, *width));
            cell.format.number_format = None;
        }
    }
    cell.computed_kind = cell.user_kind.clone();
    cell.computed_value = cell.user_value.clone();
    cell.display_value = String::new();
    Ok(cell)
}

/// Canonical text for a generated series number. Repeated addition and a
/// least-squares fit both leave binary noise that must not reach stored
/// source state, so the value is rounded to 12 significant digits first.
fn series_number_text(value: f64) -> String {
    if !value.is_finite() {
        return "0".to_string();
    }
    let rounded = round_significant(value, 12);
    if rounded == 0.0 {
        return "0".to_string();
    }
    if rounded.fract() == 0.0 && rounded.abs() < 9e15 {
        format!("{}", rounded as i64)
    } else {
        rounded.to_string()
    }
}

fn round_significant(value: f64, digits: i32) -> f64 {
    if value == 0.0 || !value.is_finite() {
        return value;
    }
    let magnitude = value.abs().log10().floor() as i32;
    let exponent = digits - 1 - magnitude;
    if !(-300..=300).contains(&exponent) {
        return value;
    }
    let factor = 10f64.powi(exponent);
    (value * factor).round() / factor
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::blank_sheet;

    fn sheet_with(cells: &[(&str, &str)]) -> Sheet {
        let mut sheet = blank_sheet("sheet-1", "Sheet1", 20, 10);
        for (address, value) in cells {
            super::super::structure::set_sheet_cell(
                &mut sheet,
                address,
                (*value).to_string(),
                &crate::format::Locale::for_tag("en-US"),
            );
        }
        sheet
    }

    fn value(sheet: &Sheet, address: &str) -> String {
        sheet
            .cells
            .iter()
            .find(|cell| cell.address == address)
            .map(|cell| cell.user_value.clone())
            .unwrap_or_default()
    }

    #[test]
    fn two_numbers_extrapolate_linearly_downwards() {
        let mut sheet = sheet_with(&[("A1", "1"), ("A2", "3")]);

        fill_sheet_range(&mut sheet, "A1:A2", "A1:A5").unwrap();

        assert_eq!(value(&sheet, "A3"), "5");
        assert_eq!(value(&sheet, "A4"), "7");
        assert_eq!(value(&sheet, "A5"), "9");
    }

    #[test]
    fn filling_upwards_extrapolates_backwards() {
        let mut sheet = sheet_with(&[("A5", "10"), ("A6", "20")]);

        fill_sheet_range(&mut sheet, "A5:A6", "A3:A6").unwrap();

        assert_eq!(value(&sheet, "A4"), "0");
        assert_eq!(value(&sheet, "A3"), "-10");
    }

    #[test]
    fn a_single_number_is_copied_but_a_single_date_steps_by_a_day() {
        let mut sheet = sheet_with(&[("A1", "7"), ("B1", "45000")]);
        super::super::structure::set_sheet_cell_format(
            &mut sheet,
            "B1",
            "number_format",
            "date".to_string(),
        )
        .unwrap();

        fill_sheet_range(&mut sheet, "A1:A1", "A1:A3").unwrap();
        fill_sheet_range(&mut sheet, "B1:B1", "B1:B3").unwrap();

        assert_eq!(value(&sheet, "A2"), "7");
        assert_eq!(value(&sheet, "A3"), "7");
        assert_eq!(value(&sheet, "B2"), "45001");
        assert_eq!(value(&sheet, "B3"), "45002");
        // The date format travels with the series.
        assert_eq!(
            sheet
                .cells
                .iter()
                .find(|cell| cell.address == "B3")
                .and_then(|cell| cell.format.number_format.clone()),
            Some("date".to_string())
        );
    }

    /// Text that names no series still repeats cyclically. (This test used to
    /// use `Mon, Tue` — which is a weekday series, and now fills as one.)
    #[test]
    fn text_repeats_cyclically() {
        let mut sheet = sheet_with(&[("A1", "Red"), ("A2", "Green")]);

        fill_sheet_range(&mut sheet, "A1:A2", "A1:A6").unwrap();

        assert_eq!(value(&sheet, "A3"), "Red");
        assert_eq!(value(&sheet, "A4"), "Green");
        assert_eq!(value(&sheet, "A5"), "Red");
        assert_eq!(value(&sheet, "A6"), "Green");
    }

    /// `Jan` filled `Jan, Jan, Jan`. Month and weekday names are the two most
    /// used text series there are.
    #[test]
    fn a_month_name_fills_the_following_months() {
        let mut sheet = sheet_with(&[("A1", "Jan")]);

        fill_sheet_range(&mut sheet, "A1:A1", "A1:A4").unwrap();

        assert_eq!(value(&sheet, "A2"), "Feb");
        assert_eq!(value(&sheet, "A3"), "Mar");
        assert_eq!(value(&sheet, "A4"), "Apr");
    }

    /// The list wraps, the spelling is the source's, and so is the case.
    #[test]
    fn a_name_series_keeps_its_spelling_and_case_and_wraps() {
        let mut sheet = sheet_with(&[("A1", "NOVEMBER")]);
        fill_sheet_range(&mut sheet, "A1:A1", "A1:A4").unwrap();
        assert_eq!(value(&sheet, "A2"), "DECEMBER");
        assert_eq!(value(&sheet, "A3"), "JANUARY");
        assert_eq!(value(&sheet, "A4"), "FEBRUARY");

        let mut sheet = sheet_with(&[("B1", "mon")]);
        fill_sheet_range(&mut sheet, "B1:B1", "B1:B3").unwrap();
        assert_eq!(value(&sheet, "B2"), "tue");
        assert_eq!(value(&sheet, "B3"), "wed");

        // `May` is both a full month name and its own abbreviation; the full
        // list is read first, so it fills `June` rather than `Jun`.
        let mut sheet = sheet_with(&[("C1", "May")]);
        fill_sheet_range(&mut sheet, "C1:C1", "C1:C2").unwrap();
        assert_eq!(value(&sheet, "C2"), "June");
    }

    /// Two sources set the stride, read modulo the list so a wrap does not
    /// turn a step of two into a step of minus ten.
    #[test]
    fn two_names_set_the_stride() {
        let mut sheet = sheet_with(&[("A1", "Jan"), ("A2", "Mar")]);
        fill_sheet_range(&mut sheet, "A1:A2", "A1:A4").unwrap();
        assert_eq!(value(&sheet, "A3"), "May");
        assert_eq!(value(&sheet, "A4"), "Jul");

        let mut sheet = sheet_with(&[("B1", "Nov"), ("B2", "Jan")]);
        fill_sheet_range(&mut sheet, "B1:B2", "B1:B4").unwrap();
        assert_eq!(value(&sheet, "B3"), "Mar");
        assert_eq!(value(&sheet, "B4"), "May");
    }

    /// `Item 1` filled `Item 1, Item 1, Item 1`.
    #[test]
    fn text_ending_in_digits_counts_up() {
        let mut sheet = sheet_with(&[("A1", "Item 1")]);

        fill_sheet_range(&mut sheet, "A1:A1", "A1:A3").unwrap();

        assert_eq!(value(&sheet, "A2"), "Item 2");
        assert_eq!(value(&sheet, "A3"), "Item 3");
    }

    #[test]
    fn a_counter_keeps_its_padding_and_takes_its_stride_from_the_source() {
        let mut sheet = sheet_with(&[("A1", "Q01")]);
        fill_sheet_range(&mut sheet, "A1:A1", "A1:A3").unwrap();
        assert_eq!(value(&sheet, "A2"), "Q02");
        assert_eq!(value(&sheet, "A3"), "Q03");

        let mut sheet = sheet_with(&[("B1", "row 2"), ("B2", "row 4")]);
        fill_sheet_range(&mut sheet, "B1:B2", "B1:B4").unwrap();
        assert_eq!(value(&sheet, "B3"), "row 6");
        assert_eq!(value(&sheet, "B4"), "row 8");
    }

    /// A counter fills backwards through zero, and text that is *only* digits
    /// is a number stored as text rather than a counter — it repeats, because
    /// inventing a prefix for it would be a guess.
    #[test]
    fn a_counter_has_limits_that_keep_it_from_guessing() {
        let mut sheet = sheet_with(&[("A3", "Item 1")]);
        fill_sheet_range(&mut sheet, "A3:A3", "A1:A3").unwrap();
        assert_eq!(value(&sheet, "A2"), "Item 0");
        assert_eq!(value(&sheet, "A1"), "Item -1");

        // Two texts with different prefixes name no series.
        let mut sheet = sheet_with(&[("B1", "Item 1"), ("B2", "Thing 2")]);
        fill_sheet_range(&mut sheet, "B1:B2", "B1:B4").unwrap();
        assert_eq!(value(&sheet, "B3"), "Item 1");
        assert_eq!(value(&sheet, "B4"), "Thing 2");
    }

    #[test]
    fn formulas_shift_relative_references_and_keep_absolute_ones() {
        let mut sheet = sheet_with(&[("C1", "=A1+$B$1")]);

        fill_sheet_range(&mut sheet, "C1:C1", "C1:C3").unwrap();

        assert_eq!(value(&sheet, "C2"), "=A2+$B$1");
        assert_eq!(value(&sheet, "C3"), "=A3+$B$1");
    }

    #[test]
    fn filling_right_runs_one_series_per_row() {
        let mut sheet = sheet_with(&[("A1", "1"), ("B1", "2"), ("A2", "x")]);

        fill_sheet_range(&mut sheet, "A1:B2", "A1:D2").unwrap();

        assert_eq!(value(&sheet, "C1"), "3");
        assert_eq!(value(&sheet, "D1"), "4");
        // The second row is not numeric, so it repeats instead.
        assert_eq!(value(&sheet, "C2"), "x");
    }

    #[test]
    fn uneven_numbers_use_a_least_squares_trend() {
        let mut sheet = sheet_with(&[("A1", "1"), ("A2", "2"), ("A3", "4")]);

        fill_sheet_range(&mut sheet, "A1:A3", "A1:A4").unwrap();

        // Slope 1.5 through (0,1) (1,2) (2,4): intercept 0.8333333333.
        assert_eq!(value(&sheet, "A4"), "5.33333333333");
    }

    #[test]
    fn a_target_inside_the_source_writes_nothing() {
        let mut sheet = sheet_with(&[("A1", "1"), ("A2", "2")]);

        fill_sheet_range(&mut sheet, "A1:A2", "A1:A2").unwrap();

        assert_eq!(value(&sheet, "A3"), "");
    }

    #[test]
    fn misaligned_or_two_sided_targets_are_rejected() {
        let mut sheet = sheet_with(&[("A1", "1")]);

        // Diagonal drag: neither axis lines up.
        assert!(fill_sheet_range(&mut sheet, "A1:A1", "A1:B2").is_err());
        // Extends above and below at once.
        assert!(fill_sheet_range(&mut sheet, "A2:A3", "A1:A5").is_err());
        // Leaves a gap.
        assert!(fill_sheet_range(&mut sheet, "A1:A2", "A5:A6").is_err());
    }

    #[test]
    fn a_fractional_step_does_not_accumulate_binary_noise() {
        let mut sheet = sheet_with(&[("A1", "0"), ("A2", "0.1")]);

        fill_sheet_range(&mut sheet, "A1:A2", "A1:A5").unwrap();

        assert_eq!(value(&sheet, "A3"), "0.2");
        assert_eq!(value(&sheet, "A4"), "0.3");
        assert_eq!(value(&sheet, "A5"), "0.4");
    }
}
