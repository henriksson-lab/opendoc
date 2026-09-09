//! Locale-aware number formatting and typed-input parsing.
//!
//! Dates and times use the 1899-12-30 serial epoch (day fraction = time).

use super::address::trim_number;
use serde_json::{json, Value};

use crate::{civil_from_days, days_from_civil, CellFormat, SpreadsheetError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DateOrder {
    MonthDayYear,
    DayMonthYear,
    YearMonthDay,
}

/// The locale conventions used for parsing and display.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Locale {
    pub decimal: char,
    pub group: char,
    pub currency: &'static str,
    pub date_order: DateOrder,
}

impl Locale {
    pub fn for_tag(tag: &str) -> Self {
        let tag = tag.trim().replace('_', "-");
        let lower = tag.to_ascii_lowercase();
        let language = lower.split('-').next().unwrap_or("en").to_string();
        let region = lower.split('-').nth(1).unwrap_or("").to_string();
        match language.as_str() {
            "en" => Locale {
                decimal: '.',
                group: ',',
                currency: match region.as_str() {
                    "gb" | "ie" => "£",
                    "au" | "ca" | "nz" | "sg" => "$",
                    "in" => "₹",
                    _ => "$",
                },
                date_order: match region.as_str() {
                    "us" | "" | "ph" => DateOrder::MonthDayYear,
                    "ca" => DateOrder::YearMonthDay,
                    _ => DateOrder::DayMonthYear,
                },
            },
            "sv" | "fi" | "nb" | "nn" | "da" | "no" => Locale {
                decimal: ',',
                group: ' ',
                currency: "kr",
                date_order: DateOrder::YearMonthDay,
            },
            "de" | "nl" | "it" | "es" | "pt" | "pl" | "cs" | "tr" | "id" => Locale {
                decimal: ',',
                group: '.',
                currency: "€",
                date_order: DateOrder::DayMonthYear,
            },
            "fr" | "ru" | "uk" => Locale {
                decimal: ',',
                group: ' ',
                currency: "€",
                date_order: DateOrder::DayMonthYear,
            },
            "ja" | "zh" | "ko" => Locale {
                decimal: '.',
                group: ',',
                currency: match language.as_str() {
                    "ja" => "¥",
                    "ko" => "₩",
                    _ => "¥",
                },
                date_order: DateOrder::YearMonthDay,
            },
            _ => Locale {
                decimal: '.',
                group: ',',
                currency: "$",
                date_order: DateOrder::MonthDayYear,
            },
        }
    }

    fn date_pattern(&self) -> &'static str {
        match self.date_order {
            DateOrder::MonthDayYear => "m/d/yyyy",
            DateOrder::DayMonthYear => "d/m/yyyy",
            DateOrder::YearMonthDay => "yyyy-mm-dd",
        }
    }
}

/// The result of parsing typed cell input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedInput {
    pub kind: &'static str,
    pub value: String,
    pub number_format: Option<String>,
}

impl CellFormat {
    pub fn set_property(&mut self, property: &str, value: String) -> Result<(), SpreadsheetError> {
        let property = property.trim();
        match property {
            "bold" => self.bold = parse_format_bool(&value)?,
            "italic" => self.italic = parse_format_bool(&value)?,
            "text_color" => {
                let value = optional_format_value(value);
                if let Some(color) = &value {
                    validate_sheet_color(color)?;
                }
                self.text_color = value;
            }
            "background_color" => {
                let value = optional_format_value(value);
                if let Some(color) = &value {
                    validate_sheet_color(color)?;
                }
                self.background_color = value;
            }
            "horizontal_align" => {
                let value = optional_format_value(value);
                if let Some(value) = &value {
                    if !matches!(value.as_str(), "left" | "center" | "right") {
                        return Err(SpreadsheetError::Format(format!(
                            "unsupported horizontal alignment {value}"
                        )));
                    }
                }
                self.horizontal_align = value;
            }
            "number_format" => self.number_format = optional_format_value(value),
            _ => {
                return Err(SpreadsheetError::Format(format!(
                    "unsupported cell format property {property}"
                )));
            }
        }
        Ok(())
    }

    pub fn validate_source(&self) -> Result<(), SpreadsheetError> {
        if let Some(color) = &self.text_color {
            validate_sheet_color(color)?;
        }
        if let Some(color) = &self.background_color {
            validate_sheet_color(color)?;
        }
        if let Some(align) = &self.horizontal_align {
            if !matches!(align.as_str(), "left" | "center" | "right") {
                return Err(SpreadsheetError::Format(format!(
                    "unsupported horizontal alignment {align}"
                )));
            }
        }
        if self
            .number_format
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(SpreadsheetError::Format(
                "number format is empty".to_string(),
            ));
        }
        if self
            .number_format
            .as_deref()
            .is_some_and(|value| value.trim() != value)
        {
            return Err(SpreadsheetError::Format(
                "number format has surrounding whitespace".to_string(),
            ));
        }
        Ok(())
    }
}

pub fn validate_sheet_color(value: &str) -> Result<(), SpreadsheetError> {
    let Some(hex) = value.strip_prefix('#') else {
        return Err(SpreadsheetError::Format(format!(
            "invalid sheet color {value}"
        )));
    };
    if hex.len() != 6 || !hex.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(SpreadsheetError::Format(format!(
            "invalid sheet color {value}"
        )));
    }
    Ok(())
}

pub fn import_sheet_rgb(
    value: Option<&Value>,
    field: &str,
) -> Result<Option<String>, SpreadsheetError> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    let red = import_sheet_rgb_channel(value, field, "red")?;
    let green = import_sheet_rgb_channel(value, field, "green")?;
    let blue = import_sheet_rgb_channel(value, field, "blue")?;
    Ok(Some(format!(
        "#{:02x}{:02x}{:02x}",
        sheet_float_color(red)?,
        sheet_float_color(green)?,
        sheet_float_color(blue)?
    )))
}

fn import_sheet_rgb_channel(
    value: &Value,
    field: &str,
    channel: &str,
) -> Result<f64, SpreadsheetError> {
    let Some(raw) = value.get(channel) else {
        return Ok(0.0);
    };
    let channel_value = raw.as_f64().ok_or_else(|| {
        SpreadsheetError::Import(format!("{field} {channel} channel must be a number"))
    })?;
    if !(0.0..=1.0).contains(&channel_value) {
        return Err(SpreadsheetError::Import(format!(
            "{field} {channel} channel must be between 0 and 1"
        )));
    }
    Ok(channel_value)
}

pub fn export_sheet_color(value: &str) -> Value {
    let value = value.trim_start_matches('#');
    let red = u8::from_str_radix(value.get(0..2).unwrap_or("00"), 16).unwrap_or(0);
    let green = u8::from_str_radix(value.get(2..4).unwrap_or("00"), 16).unwrap_or(0);
    let blue = u8::from_str_radix(value.get(4..6).unwrap_or("00"), 16).unwrap_or(0);
    json!({
        "red": red as f64 / 255.0,
        "green": green as f64 / 255.0,
        "blue": blue as f64 / 255.0,
    })
}

fn sheet_float_color(value: f64) -> Result<u8, SpreadsheetError> {
    if !(0.0..=1.0).contains(&value) {
        return Err(SpreadsheetError::Import(
            "sheet color channel must be between 0 and 1".to_string(),
        ));
    }
    Ok((value * 255.0).round() as u8)
}

pub fn trim_sheet_number(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{}", value as i64)
    } else {
        value.to_string()
    }
}

pub fn parse_format_bool(value: &str) -> Result<bool, SpreadsheetError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "on" | "yes" => Ok(true),
        "false" | "0" | "off" | "no" | "" => Ok(false),
        _ => Err(SpreadsheetError::Format(format!(
            "invalid boolean cell format value {value}"
        ))),
    }
}

fn optional_format_value(value: String) -> Option<String> {
    let value = value.trim().to_string();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn parsed(
    kind: &'static str,
    value: impl Into<String>,
    number_format: Option<&str>,
) -> ParsedInput {
    ParsedInput {
        kind,
        value: value.into(),
        number_format: number_format.map(str::to_string),
    }
}

fn is_plain_number(text: &str) -> bool {
    text.parse::<f64>().is_ok_and(f64::is_finite)
        && text
            .chars()
            .all(|ch| ch.is_ascii_digit() || matches!(ch, '.' | '-' | '+' | 'e' | 'E'))
}

fn decimals_in(text: &str, decimal: char) -> usize {
    text.rsplit_once(decimal)
        .map(|(_, rest)| rest.chars().take_while(|ch| ch.is_ascii_digit()).count())
        .unwrap_or(0)
}

/// Parses raw numeric text with locale separators (`1,000.5` / `1 000,5`).
fn parse_locale_number(text: &str, locale: &Locale) -> Option<(f64, bool)> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let mut grouped = false;
    let mut normalized = String::new();
    for ch in text.chars() {
        if ch == locale.group || (locale.group == ' ' && ch == '\u{a0}') {
            grouped = true;
            continue;
        }
        if ch == locale.decimal {
            normalized.push('.');
        } else if ch.is_ascii_digit() || matches!(ch, '-' | '+') {
            normalized.push(ch);
        } else {
            return None;
        }
    }
    if !normalized.chars().any(|ch| ch.is_ascii_digit()) {
        return None;
    }
    let value = normalized.parse::<f64>().ok()?;
    if !value.is_finite() {
        return None;
    }
    Some((value, grouped))
}

fn number_pattern(grouped: bool, decimals: usize) -> String {
    let int = if grouped { "#,##0" } else { "0" };
    if decimals == 0 {
        int.to_string()
    } else {
        format!("{int}.{}", "0".repeat(decimals))
    }
}

/// Parses `yyyy-mm-dd`, `m/d/yyyy`, `d/m/yyyy`, `d.m.yyyy` date text.
fn parse_date_text(text: &str, locale: &Locale) -> Option<(f64, &'static str)> {
    let text = text.trim();
    if let Some((year, rest)) = text.split_once('-') {
        if year.len() == 4 {
            let (month, day) = rest.split_once('-')?;
            let year = year.parse::<i32>().ok()?;
            let month = month.parse::<u32>().ok()?;
            let day = day.parse::<u32>().ok()?;
            return date_serial(year, month, day).map(|serial| (serial, "yyyy-mm-dd"));
        }
        return None;
    }
    let separator = if text.contains('/') {
        '/'
    } else if text.contains('.') {
        '.'
    } else {
        return None;
    };
    let parts: Vec<&str> = text.split(separator).collect();
    if parts.len() != 3 || parts.iter().any(|part| part.is_empty()) {
        return None;
    }
    let numbers: Vec<u32> = parts
        .iter()
        .map(|part| part.parse::<u32>().ok())
        .collect::<Option<Vec<_>>>()?;
    let (year, month, day, pattern) = match locale.date_order {
        DateOrder::MonthDayYear if parts[0].len() <= 2 => {
            (numbers[2], numbers[0], numbers[1], "m/d/yyyy")
        }
        DateOrder::DayMonthYear if parts[0].len() <= 2 => {
            (numbers[2], numbers[1], numbers[0], "d/m/yyyy")
        }
        _ if parts[0].len() == 4 => (numbers[0], numbers[1], numbers[2], "yyyy/m/d"),
        DateOrder::YearMonthDay => (numbers[2], numbers[1], numbers[0], "d/m/yyyy"),
        _ => return None,
    };
    let year = if parts.iter().any(|part| part.len() == 4) {
        year as i32
    } else if year < 30 {
        2000 + year as i32
    } else if year < 100 {
        1900 + year as i32
    } else {
        year as i32
    };
    let pattern = if separator == '.' {
        "d.m.yyyy"
    } else {
        pattern
    };
    date_serial(year, month, day).map(|serial| (serial, pattern))
}

/// Parses `h:mm`, `h:mm:ss`, optionally with `AM`/`PM`.
fn parse_time_text(text: &str) -> Option<(f64, &'static str)> {
    let text = text.trim();
    let upper = text.to_ascii_uppercase();
    let (body, meridiem) = if let Some(rest) = upper.strip_suffix("PM") {
        (rest.trim_end().to_string(), Some(true))
    } else if let Some(rest) = upper.strip_suffix("AM") {
        (rest.trim_end().to_string(), Some(false))
    } else {
        (upper.clone(), None)
    };
    let parts: Vec<&str> = body.split(':').collect();
    if parts.len() < 2 || parts.len() > 3 {
        return None;
    }
    if parts
        .iter()
        .any(|part| part.is_empty() || !part.chars().all(|ch| ch.is_ascii_digit() || ch == '.'))
    {
        return None;
    }
    let mut hour = parts[0].parse::<f64>().ok()?;
    let minute = parts[1].parse::<f64>().ok()?;
    let second = parts
        .get(2)
        .map(|part| part.parse::<f64>().ok())
        .unwrap_or(Some(0.0))?;
    if minute >= 60.0 || second >= 60.0 {
        return None;
    }
    match meridiem {
        Some(pm) => {
            if !(1.0..=12.0).contains(&hour) {
                return None;
            }
            if pm && hour < 12.0 {
                hour += 12.0;
            } else if !pm && hour == 12.0 {
                hour = 0.0;
            }
        }
        None => {
            if hour >= 24.0 {
                return None;
            }
        }
    }
    let fraction = (hour * 3600.0 + minute * 60.0 + second) / 86400.0;
    let pattern = match (parts.len(), meridiem.is_some()) {
        (2, false) => "h:mm",
        (2, true) => "h:mm AM/PM",
        (_, false) => "h:mm:ss",
        (_, true) => "h:mm:ss AM/PM",
    };
    Some((fraction, pattern))
}

/// Serial number for a civil date; `None` when the date is invalid.
pub fn date_serial(year: i32, month: u32, day: u32) -> Option<f64> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || !(0..=9999).contains(&year) {
        return None;
    }
    let days = days_from_civil(year, month, day) - days_from_civil(1899, 12, 30);
    let (check_year, check_month, check_day) =
        civil_from_days(days + days_from_civil(1899, 12, 30));
    if check_year != year || check_month != month || check_day != day {
        return None;
    }
    Some(days as f64)
}

/// Parses typed cell input using locale rules. Numbers, percents,
/// currency amounts, dates, times, and booleans become typed values with an
/// inferred number format; everything else is text.
pub fn parse_input(text: &str, locale: &Locale) -> ParsedInput {
    if text.is_empty() {
        return parsed("empty", "", None);
    }
    if text.starts_with('=') {
        return parsed("formula", text, None);
    }
    if let Some(rest) = text.strip_prefix('\'') {
        return parsed("string", rest, None);
    }
    if text.trim() != text {
        return parsed("string", text, None);
    }
    if is_plain_number(text) {
        return parsed("number", text, None);
    }
    match text.to_ascii_uppercase().as_str() {
        "TRUE" => return parsed("bool", "true", None),
        "FALSE" => return parsed("bool", "false", None),
        _ => {}
    }
    // Percent.
    if let Some(body) = text.strip_suffix('%') {
        if let Some((value, grouped)) = parse_locale_number(body, locale) {
            let decimals = decimals_in(body, locale.decimal);
            let pattern = format!("{}%", number_pattern(grouped, decimals));
            return parsed("number", trim_number(value / 100.0), Some(&pattern));
        }
    }
    // Currency.
    for symbol in [locale.currency, "$", "€", "£"] {
        let (negative, body) = match text.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, text),
        };
        let stripped = body
            .strip_prefix(symbol)
            .or_else(|| body.strip_suffix(symbol))
            .map(str::trim);
        if let Some(stripped) = stripped {
            if let Some((value, grouped)) = parse_locale_number(stripped, locale) {
                let decimals = decimals_in(stripped, locale.decimal);
                let pattern = format!(
                    "{symbol}{}",
                    number_pattern(true, if decimals == 0 { 0 } else { 2 })
                );
                let value = if negative { -value } else { value };
                let _ = grouped;
                return parsed("number", trim_number(value), Some(&pattern));
            }
        }
    }
    // Grouped / locale numbers.
    if let Some((value, grouped)) = parse_locale_number(text, locale) {
        let decimals = decimals_in(text, locale.decimal);
        let pattern = if grouped {
            Some(number_pattern(true, decimals))
        } else {
            None
        };
        return parsed("number", trim_number(value), pattern.as_deref());
    }
    // Date, optionally followed by a time.
    let (date_part, time_part) = match text.split_once(' ') {
        Some((date, time)) => (date, Some(time.trim())),
        None => (text, None),
    };
    if let Some((date_serial, date_pattern)) = parse_date_text(date_part, locale) {
        match time_part {
            None => return parsed("number", trim_number(date_serial), Some(date_pattern)),
            Some(time_text) => {
                if let Some((fraction, time_pattern)) = parse_time_text(time_text) {
                    let pattern = format!("{date_pattern} {time_pattern}");
                    return parsed(
                        "number",
                        trim_number(date_serial + fraction),
                        Some(&pattern),
                    );
                }
            }
        }
    }
    if let Some((fraction, pattern)) = parse_time_text(text) {
        return parsed("number", trim_number(fraction), Some(pattern));
    }
    parsed("string", text, None)
}

/// Formats a general (unformatted) number the way Sheets displays it:
/// up to ten significant decimals, scientific notation for extremes.
pub fn format_general(value: f64) -> String {
    if value == 0.0 {
        return "0".to_string();
    }
    let magnitude = value.abs();
    if magnitude >= 1e21 || magnitude < 1e-7 {
        let formatted = format!("{value:.5E}");
        let (mantissa, exponent) = formatted.split_once('E').unwrap_or((&formatted, "0"));
        let mantissa = trim_trailing_zeros(mantissa);
        let exponent: i32 = exponent.parse().unwrap_or(0);
        return format!(
            "{mantissa}E{}{:02}",
            if exponent < 0 { "-" } else { "+" },
            exponent.abs()
        );
    }
    let digits = 10i32 - magnitude.log10().floor().max(0.0) as i32;
    let digits = digits.clamp(0, 10) as usize;
    let rounded = format!("{value:.digits$}");
    trim_trailing_zeros(&rounded)
}

fn trim_trailing_zeros(text: &str) -> String {
    if !text.contains('.') {
        return text.to_string();
    }
    let trimmed = text.trim_end_matches('0').trim_end_matches('.');
    if trimmed.is_empty() || trimmed == "-" {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Inserts locale group separators into an integer digit string.
fn group_digits(digits: &str, locale: &Locale) -> String {
    let mut out = String::new();
    let count = digits.len();
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (count - index).is_multiple_of(3) {
            out.push(locale.group);
        }
        out.push(ch);
    }
    out
}

/// Fixed-decimal formatting with optional grouping (`FIXED`, `DOLLAR`).
pub fn format_fixed(value: f64, decimals: usize, commas: bool, locale: &Locale) -> String {
    let rounded = format!("{:.decimals$}", value.abs());
    let (int_part, frac_part) = rounded.split_once('.').unwrap_or((&rounded, ""));
    let int_part = if commas {
        group_digits(int_part, locale)
    } else {
        int_part.to_string()
    };
    let mut out = String::new();
    if value < 0.0 && rounded.chars().any(|ch| ch != '0' && ch != '.') {
        out.push('-');
    }
    out.push_str(&int_part);
    if !frac_part.is_empty() {
        out.push(locale.decimal);
        out.push_str(frac_part);
    }
    out
}

#[derive(Clone, Debug, Default)]
struct NumberSection {
    prefix: String,
    suffix: String,
    int_digits: usize,
    frac_min: usize,
    frac_max: usize,
    grouping: bool,
    percent: bool,
    scientific: Option<usize>,
    has_digits: bool,
}

fn split_sections(pattern: &str) -> Vec<String> {
    let mut sections = vec![String::new()];
    let mut quoted = false;
    let mut bracket = false;
    let mut chars = pattern.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '"' => {
                quoted = !quoted;
                sections.last_mut().expect("section").push(ch);
            }
            '\\' if !quoted => {
                sections.last_mut().expect("section").push(ch);
                if let Some(next) = chars.next() {
                    sections.last_mut().expect("section").push(next);
                }
            }
            '[' if !quoted => {
                bracket = true;
                sections.last_mut().expect("section").push(ch);
            }
            ']' if !quoted => {
                bracket = false;
                sections.last_mut().expect("section").push(ch);
            }
            ';' if !quoted && !bracket => sections.push(String::new()),
            _ => sections.last_mut().expect("section").push(ch),
        }
    }
    sections
}

fn parse_number_section(section: &str) -> NumberSection {
    let mut out = NumberSection::default();
    let mut seen_core = false;
    let mut in_fraction = false;
    let mut chars = section.chars().peekable();
    let push_literal = |out: &mut NumberSection, seen_core: bool, text: &str| {
        if seen_core {
            out.suffix.push_str(text);
        } else {
            out.prefix.push_str(text);
        }
    };
    while let Some(ch) = chars.next() {
        match ch {
            '"' => {
                let mut literal = String::new();
                for next in chars.by_ref() {
                    if next == '"' {
                        break;
                    }
                    literal.push(next);
                }
                push_literal(&mut out, seen_core, &literal);
            }
            '\\' => {
                if let Some(next) = chars.next() {
                    push_literal(&mut out, seen_core, &next.to_string());
                }
            }
            '[' => {
                let mut content = String::new();
                for next in chars.by_ref() {
                    if next == ']' {
                        break;
                    }
                    content.push(next);
                }
                if let Some(currency) = content.strip_prefix('$') {
                    let symbol = currency.split('-').next().unwrap_or("");
                    push_literal(&mut out, seen_core, symbol);
                }
            }
            '0' | '#' | '?' => {
                seen_core = true;
                out.has_digits = true;
                if in_fraction {
                    out.frac_max += 1;
                    if ch == '0' {
                        out.frac_min += 1;
                    }
                } else if ch == '0' {
                    out.int_digits += 1;
                }
            }
            ',' if seen_core && !in_fraction => out.grouping = true,
            '.' if !in_fraction
                && (seen_core
                    || chars
                        .peek()
                        .is_some_and(|next| matches!(next, '0' | '#' | '?'))) =>
            {
                seen_core = true;
                in_fraction = true;
            }
            '%' => {
                out.percent = true;
                push_literal(&mut out, seen_core, "%");
            }
            'E' | 'e' if chars.peek().is_some_and(|next| matches!(next, '+' | '-')) => {
                chars.next();
                let mut digits = 0;
                while chars.peek().is_some_and(|next| matches!(next, '0' | '#')) {
                    chars.next();
                    digits += 1;
                }
                out.scientific = Some(digits.max(1));
                in_fraction = false;
            }
            '_' => {
                chars.next();
                push_literal(&mut out, seen_core, " ");
            }
            '*' => {
                chars.next();
            }
            '@' => {
                seen_core = true;
            }
            other => push_literal(&mut out, seen_core, &other.to_string()),
        }
    }
    out
}

fn format_number_section(value: f64, section: &NumberSection, locale: &Locale) -> String {
    let mut value = value;
    if section.percent {
        value *= 100.0;
    }
    let mut body = String::new();
    if let Some(exponent_digits) = section.scientific {
        let exponent = if value == 0.0 {
            0
        } else {
            value.abs().log10().floor() as i32
        };
        let mantissa = value / 10f64.powi(exponent);
        let mantissa = format!("{:.prec$}", mantissa, prec = section.frac_max);
        let mantissa = if section.frac_min < section.frac_max {
            trim_to_min_decimals(&mantissa, section.frac_min)
        } else {
            mantissa
        };
        body.push_str(&mantissa.replace('.', &locale.decimal.to_string()));
        body.push('E');
        body.push(if exponent < 0 { '-' } else { '+' });
        body.push_str(&format!(
            "{:0width$}",
            exponent.abs(),
            width = exponent_digits
        ));
        return format!("{}{body}{}", section.prefix, section.suffix);
    }
    let rounded = format!("{:.prec$}", value.abs(), prec = section.frac_max);
    let (int_part, frac_part) = rounded.split_once('.').unwrap_or((&rounded, ""));
    let mut int_part = int_part.trim_start_matches('0').to_string();
    if int_part.len() < section.int_digits {
        int_part = format!(
            "{}{int_part}",
            "0".repeat(section.int_digits - int_part.len())
        );
    }
    if int_part.is_empty()
        && section.int_digits == 0
        && frac_part.chars().all(|ch| ch == '0')
        && section.frac_max == 0
    {
        int_part = "0".to_string();
    }
    if section.grouping {
        int_part = group_digits(&int_part, locale);
    }
    body.push_str(&int_part);
    let frac_part = if section.frac_min < section.frac_max {
        let trimmed = frac_part.trim_end_matches('0');
        if trimmed.len() < section.frac_min {
            format!("{trimmed}{}", "0".repeat(section.frac_min - trimmed.len()))
        } else {
            trimmed.to_string()
        }
    } else {
        frac_part.to_string()
    };
    if !frac_part.is_empty() {
        body.push(locale.decimal);
        body.push_str(&frac_part);
    }
    let is_zero = body.chars().all(|ch| !ch.is_ascii_digit() || ch == '0');
    let sign = if value < 0.0 && !is_zero { "-" } else { "" };
    format!("{sign}{}{body}{}", section.prefix, section.suffix)
}

fn trim_to_min_decimals(text: &str, min: usize) -> String {
    let Some((int_part, frac)) = text.split_once('.') else {
        return text.to_string();
    };
    let trimmed = frac.trim_end_matches('0');
    let frac = if trimmed.len() < min {
        format!("{trimmed}{}", "0".repeat(min - trimmed.len()))
    } else {
        trimmed.to_string()
    };
    if frac.is_empty() {
        int_part.to_string()
    } else {
        format!("{int_part}.{frac}")
    }
}

/// Formats a number with a custom pattern such as `#,##0.00`, `0%`,
/// `$#,##0.00;($#,##0.00)`, or `0.00E+00`.
pub fn format_number_pattern(value: f64, pattern: &str, locale: &Locale) -> String {
    let sections = split_sections(pattern);
    let parsed: Vec<NumberSection> = sections.iter().map(|s| parse_number_section(s)).collect();
    if parsed.is_empty() || !parsed[0].has_digits {
        return format_general(value);
    }
    if value < 0.0 {
        if let Some(negative) = parsed.get(1).filter(|section| section.has_digits) {
            return format_number_section(value.abs(), negative, locale);
        }
    }
    if value == 0.0 {
        if let Some(zero) = parsed.get(2).filter(|section| section.has_digits) {
            return format_number_section(0.0, zero, locale);
        }
    }
    format_number_section(value, &parsed[0], locale)
}

const MONTH_NAMES: [&str; 12] = [
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
];

const WEEKDAY_NAMES: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

#[derive(Clone, Debug, PartialEq, Eq)]
enum DateToken {
    Year4,
    Year2,
    MonthName,
    MonthAbbr,
    Month2,
    Month1,
    WeekdayName,
    WeekdayAbbr,
    Day2,
    Day1,
    Hour2,
    Hour1,
    Minute2,
    Minute1,
    Second2,
    Second1,
    ElapsedHours,
    Meridiem(bool),
    Literal(String),
}

fn tokenize_date_pattern(pattern: &str) -> Vec<DateToken> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut raw: Vec<(char, usize)> = Vec::new();
    let mut tokens: Vec<DateToken> = Vec::new();
    let mut index = 0;
    // First pass: group runs of the same letter, keeping literals.
    let mut runs: Vec<Result<(char, usize), String>> = Vec::new();
    while index < chars.len() {
        let ch = chars[index];
        if ch == '"' {
            let mut literal = String::new();
            index += 1;
            while index < chars.len() && chars[index] != '"' {
                literal.push(chars[index]);
                index += 1;
            }
            index += 1;
            runs.push(Err(literal));
            continue;
        }
        if ch == '\\' && index + 1 < chars.len() {
            runs.push(Err(chars[index + 1].to_string()));
            index += 2;
            continue;
        }
        if ch == '[' {
            let mut content = String::new();
            index += 1;
            while index < chars.len() && chars[index] != ']' {
                content.push(chars[index]);
                index += 1;
            }
            index += 1;
            if content.eq_ignore_ascii_case("h") || content.eq_ignore_ascii_case("hh") {
                runs.push(Ok(('[', 1)));
            }
            continue;
        }
        let upper_rest: String = chars[index..]
            .iter()
            .take(5)
            .collect::<String>()
            .to_ascii_uppercase();
        if upper_rest.starts_with("AM/PM") {
            runs.push(Ok(('A', 5)));
            index += 5;
            continue;
        }
        if upper_rest.starts_with("A/P") {
            runs.push(Ok(('A', 3)));
            index += 3;
            continue;
        }
        let lower = ch.to_ascii_lowercase();
        if matches!(lower, 'y' | 'm' | 'd' | 'h' | 's') {
            let mut count = 0;
            while index < chars.len() && chars[index].to_ascii_lowercase() == lower {
                count += 1;
                index += 1;
            }
            runs.push(Ok((lower, count)));
            continue;
        }
        runs.push(Err(ch.to_string()));
        index += 1;
    }
    let _ = &mut raw;
    for (position, run) in runs.iter().enumerate() {
        match run {
            Err(literal) => tokens.push(DateToken::Literal(literal.clone())),
            Ok(('y', count)) => tokens.push(if *count >= 4 {
                DateToken::Year4
            } else {
                DateToken::Year2
            }),
            Ok(('d', count)) => tokens.push(match count {
                1 => DateToken::Day1,
                2 => DateToken::Day2,
                3 => DateToken::WeekdayAbbr,
                _ => DateToken::WeekdayName,
            }),
            Ok(('h', count)) => tokens.push(if *count >= 2 {
                DateToken::Hour2
            } else {
                DateToken::Hour1
            }),
            Ok(('[', _)) => tokens.push(DateToken::ElapsedHours),
            Ok(('s', count)) => tokens.push(if *count >= 2 {
                DateToken::Second2
            } else {
                DateToken::Second1
            }),
            Ok(('A', count)) => tokens.push(DateToken::Meridiem(*count == 5)),
            Ok(('m', count)) => {
                let previous_time = runs[..position]
                    .iter()
                    .rev()
                    .find_map(|run| match run {
                        Ok((letter, _)) => Some(*letter),
                        Err(_) => None,
                    })
                    .is_some_and(|letter| letter == 'h' || letter == '[');
                let next_seconds = runs[position + 1..]
                    .iter()
                    .find_map(|run| match run {
                        Ok((letter, _)) => Some(*letter),
                        Err(_) => None,
                    })
                    .is_some_and(|letter| letter == 's');
                if *count <= 2 && (previous_time || next_seconds) {
                    tokens.push(if *count == 2 {
                        DateToken::Minute2
                    } else {
                        DateToken::Minute1
                    });
                } else {
                    tokens.push(match count {
                        1 => DateToken::Month1,
                        2 => DateToken::Month2,
                        3 => DateToken::MonthAbbr,
                        _ => DateToken::MonthName,
                    });
                }
            }
            Ok(_) => {}
        }
    }
    tokens
}

/// True when a number format pattern describes a date or time.
pub fn is_date_format(pattern: &str) -> bool {
    let named = pattern.trim().to_ascii_uppercase();
    if matches!(
        named.as_str(),
        "DATE" | "TIME" | "DATE_TIME" | "DATETIME" | "DURATION"
    ) {
        return true;
    }
    if matches!(
        named.as_str(),
        "GENERAL" | "AUTOMATIC" | "NUMBER" | "PERCENT" | "CURRENCY" | "TEXT" | "SCIENTIFIC"
    ) {
        return false;
    }
    let mut quoted = false;
    let mut bracket = false;
    for ch in pattern.chars() {
        match ch {
            '"' => quoted = !quoted,
            '[' if !quoted => bracket = true,
            ']' if !quoted => bracket = false,
            _ if quoted || bracket => {}
            'y' | 'Y' | 'd' | 'D' | 'h' | 'H' | 's' | 'S' => return true,
            _ => {}
        }
    }
    false
}

/// Splits a serial into civil date and clock components.
pub fn serial_components(serial: f64) -> (i32, u32, u32, u32, u32, u32, u32) {
    let mut days = serial.floor() as i64;
    let mut seconds = ((serial - serial.floor()) * 86_400.0).round() as i64;
    if seconds >= 86_400 {
        seconds -= 86_400;
        days += 1;
    }
    let (year, month, day) = civil_from_days(days + days_from_civil(1899, 12, 30));
    let weekday = ((days + days_from_civil(1899, 12, 30)).rem_euclid(7) + 4) % 7;
    let hour = (seconds / 3600) as u32;
    let minute = ((seconds % 3600) / 60) as u32;
    let second = (seconds % 60) as u32;
    (year, month, day, hour, minute, second, weekday as u32)
}

/// Formats a serial number with a date/time pattern.
pub fn format_date_pattern(serial: f64, pattern: &str) -> String {
    let tokens = tokenize_date_pattern(pattern);
    let (year, month, day, hour, minute, second, weekday) = serial_components(serial);
    let has_meridiem = tokens
        .iter()
        .any(|token| matches!(token, DateToken::Meridiem(_)));
    let clock_hour = if has_meridiem {
        match hour % 12 {
            0 => 12,
            other => other,
        }
    } else {
        hour
    };
    let mut out = String::new();
    for token in tokens {
        match token {
            DateToken::Year4 => out.push_str(&format!("{year:04}")),
            DateToken::Year2 => out.push_str(&format!("{:02}", year.rem_euclid(100))),
            DateToken::MonthName => out.push_str(MONTH_NAMES[(month - 1) as usize]),
            DateToken::MonthAbbr => out.push_str(&MONTH_NAMES[(month - 1) as usize][..3]),
            DateToken::Month2 => out.push_str(&format!("{month:02}")),
            DateToken::Month1 => out.push_str(&month.to_string()),
            DateToken::WeekdayName => out.push_str(WEEKDAY_NAMES[weekday as usize]),
            DateToken::WeekdayAbbr => out.push_str(&WEEKDAY_NAMES[weekday as usize][..3]),
            DateToken::Day2 => out.push_str(&format!("{day:02}")),
            DateToken::Day1 => out.push_str(&day.to_string()),
            DateToken::Hour2 => out.push_str(&format!("{clock_hour:02}")),
            DateToken::Hour1 => out.push_str(&clock_hour.to_string()),
            DateToken::ElapsedHours => {
                out.push_str(&((serial.floor() as i64) * 24 + hour as i64).to_string())
            }
            DateToken::Minute2 => out.push_str(&format!("{minute:02}")),
            DateToken::Minute1 => out.push_str(&minute.to_string()),
            DateToken::Second2 => out.push_str(&format!("{second:02}")),
            DateToken::Second1 => out.push_str(&second.to_string()),
            DateToken::Meridiem(long) => out.push_str(match (hour >= 12, long) {
                (true, true) => "PM",
                (false, true) => "AM",
                (true, false) => "P",
                (false, false) => "A",
            }),
            DateToken::Literal(text) => out.push_str(&text),
        }
    }
    out
}

/// Resolves a named format to its pattern.
fn named_pattern(name: &str, locale: &Locale) -> Option<String> {
    let upper = name.trim().to_ascii_uppercase();
    let (base, decimals) = match upper.split_once(':') {
        Some((base, decimals)) => (base.to_string(), decimals.parse::<usize>().ok()),
        None => (upper, None),
    };
    let decimals_pattern = |default: usize| {
        let count = decimals.unwrap_or(default);
        if count == 0 {
            String::new()
        } else {
            format!(".{}", "0".repeat(count))
        }
    };
    Some(match base.as_str() {
        "NUMBER" | "DECIMAL" => format!("#,##0{}", decimals_pattern(2)),
        "PERCENT" => format!("0{}%", decimals_pattern(2)),
        "CURRENCY" => format!("{}#,##0{}", locale.currency, decimals_pattern(2)),
        "SCIENTIFIC" => format!("0{}E+00", decimals_pattern(2)),
        "DATE" => locale.date_pattern().to_string(),
        "TIME" => "h:mm:ss AM/PM".to_string(),
        "DATE_TIME" | "DATETIME" => format!("{} h:mm:ss", locale.date_pattern()),
        "DURATION" => "[h]:mm:ss".to_string(),
        _ => return None,
    })
}

/// Formats a number with a named format or custom pattern.
pub fn format_with_pattern(value: f64, pattern: &str, locale: &Locale) -> Option<String> {
    let trimmed = pattern.trim();
    let upper = trimmed.to_ascii_uppercase();
    match upper.as_str() {
        "" | "GENERAL" | "AUTOMATIC" => return Some(format_general(value)),
        "TEXT" | "@" => return Some(trim_number(value)),
        _ => {}
    }
    let resolved = named_pattern(trimmed, locale).unwrap_or_else(|| trimmed.to_string());
    if is_date_format(&resolved) {
        if !value.is_finite() || value < -693_594.0 || value > 2_958_465.0 {
            return None;
        }
        return Some(format_date_pattern(value, &resolved));
    }
    Some(format_number_pattern(value, &resolved, locale))
}

/// Display text for a projected cell value.
pub fn display_value(
    computed_kind: &str,
    computed_value: &str,
    number_format: Option<&str>,
    locale: &Locale,
) -> String {
    match computed_kind {
        "number" => {
            let Ok(value) = computed_value.parse::<f64>() else {
                return computed_value.to_string();
            };
            match number_format {
                Some(pattern) => format_with_pattern(value, pattern, locale)
                    .unwrap_or_else(|| format_general(value)),
                None => format_general(value),
            }
        }
        "bool" => if computed_value.eq_ignore_ascii_case("true") {
            "TRUE"
        } else {
            "FALSE"
        }
        .to_string(),
        _ => computed_value.to_string(),
    }
}
