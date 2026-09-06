//! Spreadsheet function library operating on [`FormulaValue`]s.

use std::collections::{BTreeMap, BTreeSet};

use super::format::{self, Locale};
use super::formula::{parse_reference, Evaluator, Expr, RefExpr, ResolvedRange};
use super::value::{
    compare_values, parse_number_text, values_match, FormulaArray, FormulaError, FormulaValue,
};
use crate::{
    clean_text, correlation_value, covariance_value, datedif_value, encode_formula_base_digits,
    evaluate_function, forecast_value, formula_source_is_date_producing, is_supported_email_address,
    is_supported_url, networkdays_value_with_holidays, number_to_column, parse_formula_datevalue,
    parse_formula_decimal, parse_formula_numbervalue, parse_formula_text_number,
    parse_formula_timevalue, percentrank_value, proper_case_text, regression_value, trim_number,
    wildcard_search_position, wildcard_text_matches, workday_value_with_holidays,
};

type FResult = Result<FormulaValue, FormulaError>;

/// Serial number of the Unix epoch in the 1899-12-30 date system.
pub const UNIX_EPOCH_SERIAL: f64 = 25569.0;
const MS_PER_DAY: f64 = 86_400_000.0;

fn legacy(result: Result<f64, String>) -> FResult {
    match result {
        Ok(value) if value.is_finite() => Ok(FormulaValue::Number(value)),
        Ok(_) => Err(FormulaError::Num),
        Err(message) => Err(FormulaError::from_legacy(&message)),
    }
}

fn number(value: f64) -> FResult {
    if value.is_finite() {
        Ok(FormulaValue::Number(value))
    } else {
        Err(FormulaError::Num)
    }
}

fn arity(args: &[Expr], min: usize, max: usize) -> Result<(), FormulaError> {
    if args.len() < min || args.len() > max {
        Err(FormulaError::Value)
    } else {
        Ok(())
    }
}

fn arity_values(values: &[FormulaValue], min: usize, max: usize) -> Result<(), FormulaError> {
    if values.len() < min || values.len() > max {
        Err(FormulaError::Value)
    } else {
        Ok(())
    }
}

/// Functions returning booleans when delegated to the numeric core.
fn returns_bool(name: &str) -> bool {
    matches!(
        name,
        "AND"
            | "OR"
            | "XOR"
            | "NOT"
            | "TRUE"
            | "FALSE"
            | "ISEVEN"
            | "ISODD"
            | "ISBETWEEN"
            | "EQ"
            | "NE"
            | "GT"
            | "GTE"
            | "LT"
            | "LTE"
    )
}

/// Functions where blank direct references are skipped rather than
/// coerced to zero.
fn is_aggregate(name: &str) -> bool {
    matches!(
        name,
        "SUM"
            | "AVERAGE"
            | "MIN"
            | "MAX"
            | "COUNT"
            | "PRODUCT"
            | "MEDIAN"
            | "MODE"
            | "SUMSQ"
            | "STDEV"
            | "STDEVP"
            | "STDEV.S"
            | "STDEV.P"
            | "VAR"
            | "VARP"
            | "VAR.S"
            | "VAR.P"
            | "AVEDEV"
            | "DEVSQ"
            | "GEOMEAN"
            | "HARMEAN"
            | "LARGE"
            | "SMALL"
            | "GCD"
            | "LCM"
            | "AND"
            | "OR"
            | "XOR"
            | "PERCENTILE"
            | "PERCENTILE.INC"
            | "PERCENTILE.EXC"
            | "QUARTILE"
            | "QUARTILE.INC"
            | "QUARTILE.EXC"
    )
}

/// Functions that consume whole ranges and must not be lifted element-wise
/// inside `ARRAYFORMULA`.
fn is_range_aware(name: &str) -> bool {
    is_aggregate(name)
        || matches!(
            name,
            "COUNTA"
                | "COUNTBLANK"
                | "COUNTUNIQUE"
                | "AVERAGEA"
                | "MINA"
                | "MAXA"
                | "SUMPRODUCT"
                | "COUNTIF"
                | "SUMIF"
                | "AVERAGEIF"
                | "COUNTIFS"
                | "SUMIFS"
                | "AVERAGEIFS"
                | "MINIFS"
                | "MAXIFS"
                | "MATCH"
                | "VLOOKUP"
                | "HLOOKUP"
                | "XLOOKUP"
                | "LOOKUP"
                | "RANK"
                | "RANK.EQ"
                | "RANK.AVG"
                | "PERCENTRANK"
                | "PERCENTRANK.INC"
                | "PERCENTRANK.EXC"
                | "CORREL"
                | "PEARSON"
                | "COVAR"
                | "COVARIANCE.P"
                | "COVARIANCE.S"
                | "SLOPE"
                | "INTERCEPT"
                | "RSQ"
                | "FORECAST"
                | "TRANSPOSE"
                | "UNIQUE"
                | "SORT"
                | "FILTER"
                | "FLATTEN"
                | "JOIN"
                | "TEXTJOIN"
                | "CONCATENATE"
                | "NETWORKDAYS"
                | "WORKDAY"
                | "SEQUENCE"
        )
}

/// Evaluates a function call.
pub fn call(ev: &mut Evaluator<'_>, name: &str, args: &[Expr]) -> FormulaValue {
    match call_inner(ev, name, args) {
        Ok(value) => value,
        Err(error) => FormulaValue::Error(error),
    }
}

fn call_inner(ev: &mut Evaluator<'_>, name: &str, args: &[Expr]) -> FResult {
    match name {
        "IF" => return if_function(ev, args),
        "IFS" => return ifs_function(ev, args),
        "CHOOSE" => return choose_function(ev, args),
        "SWITCH" => return switch_function(ev, args),
        "IFERROR" | "IFNA" => return error_handler(ev, name, args),
        "ISERROR" | "ISERR" | "ISNA" | "ISNUMBER" | "ISTEXT" | "ISNONTEXT" | "ISLOGICAL" => {
            return value_predicate(ev, name, args)
        }
        "ISBLANK" | "ISFORMULA" => return cell_state_predicate(ev, name, args),
        "ISREF" => return isref(ev, args),
        "ISDATE" => return isdate(ev, args),
        "TYPE" => {
            arity(args, 1, 1)?;
            let value = ev.eval(&args[0]);
            return Ok(FormulaValue::Number(value.type_code()));
        }
        "ERROR.TYPE" => {
            arity(args, 1, 1)?;
            let value = ev.eval_scalar(&args[0]);
            return match value {
                FormulaValue::Error(error) => Ok(FormulaValue::Number(error.type_code())),
                _ => Err(FormulaError::Na),
            };
        }
        "ROW" | "COLUMN" | "ROWS" | "COLUMNS" => return address_info(ev, name, args),
        "INDEX" => return index_function(ev, args),
        "OFFSET" | "INDIRECT" => {
            let range = resolve_dynamic_range(ev, name, args)?;
            return Ok(ev.range_value(&range));
        }
        "ADDRESS" => return address_function(ev, args),
        "ARRAYFORMULA" => {
            arity(args, 1, 1)?;
            let previous = ev.array_mode;
            ev.array_mode = true;
            let value = ev.eval(&args[0]);
            ev.array_mode = previous;
            return Ok(value);
        }
        "SUBTOTAL" => return subtotal(ev, args),
        "TODAY" | "NOW" => {
            arity(args, 0, 0)?;
            let serial = ev.env.now_ms() as f64 / MS_PER_DAY + UNIX_EPOCH_SERIAL;
            return number(if name == "TODAY" {
                serial.floor()
            } else {
                serial
            });
        }
        "RAND" => {
            arity(args, 0, 0)?;
            return number(ev_random(ev));
        }
        "RANDBETWEEN" => {
            arity(args, 2, 2)?;
            let low = ev.eval_number(&args[0])?.ceil();
            let high = ev.eval_number(&args[1])?.floor();
            if high < low {
                return Err(FormulaError::Num);
            }
            let span = high - low + 1.0;
            return number(low + (ev_random(ev) * span).floor().min(span - 1.0));
        }
        "SEQUENCE" => return sequence(ev, args),
        "SORT" => return sort_function(ev, args),
        "FILTER" => return filter_function(ev, args),
        "UNIQUE" => return unique_function(ev, args),
        "NETWORKDAYS" | "WORKDAY" => return workday_function(ev, name, args),
        "DATEDIF" => {
            arity(args, 3, 3)?;
            let start = ev.eval_number(&args[0])?;
            let end = ev.eval_number(&args[1])?;
            let unit = ev.eval_text(&args[2])?;
            return legacy(datedif_value(start, end, &unit));
        }
        "COUNTIF" | "SUMIF" | "AVERAGEIF" => return conditional_aggregate(ev, name, args),
        "COUNTIFS" | "SUMIFS" | "AVERAGEIFS" | "MINIFS" | "MAXIFS" => {
            return multi_conditional_aggregate(ev, name, args)
        }
        "MATCH" => return match_function(ev, args),
        "VLOOKUP" | "HLOOKUP" => return vh_lookup(ev, name, args),
        "XLOOKUP" => return xlookup(ev, args),
        "LOOKUP" => return lookup_function(ev, args),
        "RANK" | "RANK.EQ" | "RANK.AVG" => return rank_function(ev, name == "RANK.AVG", args),
        "PERCENTRANK" | "PERCENTRANK.INC" | "PERCENTRANK.EXC" => {
            return percentrank_function(ev, name == "PERCENTRANK.EXC", args)
        }
        "CORREL" | "PEARSON" | "COVAR" | "COVARIANCE.P" | "COVARIANCE.S" | "SLOPE"
        | "INTERCEPT" | "RSQ" => {
            arity(args, 2, 2)?;
            let (left, right) = paired_numbers(ev, &args[0], &args[1])?;
            return legacy(match name {
                "CORREL" | "PEARSON" => correlation_value(&left, &right),
                "COVAR" | "COVARIANCE.P" => covariance_value(&left, &right, false),
                "COVARIANCE.S" => covariance_value(&left, &right, true),
                _ => regression_value(&left, &right, name),
            });
        }
        "FORECAST" => {
            arity(args, 3, 3)?;
            let x = ev.eval_number(&args[0])?;
            let (known_y, known_x) = paired_numbers(ev, &args[1], &args[2])?;
            return legacy(forecast_value(x, &known_y, &known_x));
        }
        _ => {}
    }
    let values: Vec<FormulaValue> = args.iter().map(|arg| ev.eval(arg)).collect();
    if ev.array_mode && !is_range_aware(name) {
        if let Some(lifted) = lift_over_arrays(ev, name, &values)? {
            return Ok(lifted);
        }
    }
    call_values(ev, name, values)
}

/// Element-wise application of a scalar function inside `ARRAYFORMULA`.
fn lift_over_arrays(
    ev: &mut Evaluator<'_>,
    name: &str,
    values: &[FormulaValue],
) -> Result<Option<FormulaValue>, FormulaError> {
    let mut rows = 1usize;
    let mut cols = 1usize;
    let mut has_array = false;
    for value in values {
        if let FormulaValue::Array(array) = value {
            has_array = true;
            rows = rows.max(array.rows);
            cols = cols.max(array.cols);
        }
    }
    if !has_array {
        return Ok(None);
    }
    let mut out = Vec::with_capacity(rows * cols);
    for row in 0..rows {
        for col in 0..cols {
            let scalar_args = values
                .iter()
                .map(|value| match value {
                    FormulaValue::Array(array) => {
                        let r = if array.rows == 1 { 0 } else { row };
                        let c = if array.cols == 1 { 0 } else { col };
                        if r < array.rows && c < array.cols {
                            array.get(r, c).clone()
                        } else {
                            FormulaValue::Error(FormulaError::Na)
                        }
                    }
                    other => other.clone(),
                })
                .collect();
            out.push(match call_values(ev, name, scalar_args) {
                Ok(value) => value,
                Err(error) => FormulaValue::Error(error),
            });
        }
    }
    Ok(Some(FormulaValue::Array(FormulaArray::new(rows, cols, out))))
}

/// Deterministic pseudo-random number in `[0, 1)` derived from the
/// evaluation seed and the cell position.
fn ev_random(ev: &mut Evaluator<'_>) -> f64 {
    ev.random_counter += 1;
    let mut state = ev.env.seed() ^ 0x9E37_79B9_7F4A_7C15;
    for salt in [
        ev.sheet as u64 + 1,
        ev.col as u64,
        ev.row as u64,
        ev.random_counter,
    ] {
        state ^= salt.wrapping_mul(0xBF58_476D_1CE4_E5B9);
        state = state.rotate_left(23).wrapping_mul(0x94D0_49BB_1331_11EB);
        state ^= state >> 31;
    }
    (state >> 11) as f64 / (1u64 << 53) as f64
}

fn call_values(ev: &mut Evaluator<'_>, name: &str, values: Vec<FormulaValue>) -> FResult {
    match name {
        "NA" => Err(FormulaError::Na),
        "TRUE" => Ok(FormulaValue::Bool(true)),
        "FALSE" => Ok(FormulaValue::Bool(false)),
        "N" => {
            arity_values(&values, 1, 1)?;
            Ok(FormulaValue::Number(match values[0].scalar() {
                FormulaValue::Number(value) => value,
                FormulaValue::Bool(value) => {
                    if value {
                        1.0
                    } else {
                        0.0
                    }
                }
                FormulaValue::Error(error) => return Err(error),
                _ => 0.0,
            }))
        }
        "VALUE" => {
            arity_values(&values, 1, 1)?;
            match values[0].scalar() {
                FormulaValue::Number(value) => number(value),
                FormulaValue::Bool(value) => number(if value { 1.0 } else { 0.0 }),
                FormulaValue::Blank => number(0.0),
                FormulaValue::Error(error) => Err(error),
                FormulaValue::Text(text) => {
                    if let Ok(value) = parse_formula_text_number(&text) {
                        return number(value);
                    }
                    if let Ok(value) = parse_formula_datevalue(&text) {
                        return number(value);
                    }
                    if let Ok(value) = parse_formula_timevalue(&text) {
                        return number(value);
                    }
                    parse_number_text(&text)
                        .map(FormulaValue::Number)
                        .ok_or(FormulaError::Value)
                }
                FormulaValue::Array(_) => Err(FormulaError::Value),
            }
        }
        "NUMBERVALUE" => {
            arity_values(&values, 1, 3)?;
            let input = text_of(&values[0])?;
            let decimal = values
                .get(1)
                .map(text_of)
                .transpose()?
                .unwrap_or_else(|| ".".to_string());
            let group = values
                .get(2)
                .map(text_of)
                .transpose()?
                .unwrap_or_else(|| ",".to_string());
            legacy(parse_formula_numbervalue(&input, &decimal, &group))
        }
        "DECIMAL" => {
            arity_values(&values, 2, 2)?;
            let input = text_of(&values[0])?;
            let radix = number_of(&values[1])?;
            legacy(parse_formula_decimal(&input, radix))
        }
        "DATEVALUE" => {
            arity_values(&values, 1, 1)?;
            let input = text_of(&values[0])?;
            legacy(parse_formula_datevalue(&input))
        }
        "TIMEVALUE" => {
            arity_values(&values, 1, 1)?;
            let input = text_of(&values[0])?;
            legacy(parse_formula_timevalue(&input))
        }
        "LEN" => {
            arity_values(&values, 1, 1)?;
            let text = text_of(&values[0])?;
            number(text.chars().count() as f64)
        }
        "FIND" | "SEARCH" => text_search(name, &values),
        "EXACT" => {
            arity_values(&values, 2, 2)?;
            Ok(FormulaValue::Bool(
                text_of(&values[0])? == text_of(&values[1])?,
            ))
        }
        "ISEMAIL" => {
            arity_values(&values, 1, 1)?;
            Ok(FormulaValue::Bool(is_supported_email_address(&text_of(
                &values[0],
            )?)))
        }
        "ISURL" => {
            arity_values(&values, 1, 1)?;
            Ok(FormulaValue::Bool(is_supported_url(&text_of(&values[0])?)))
        }
        "CODE" | "UNICODE" => {
            arity_values(&values, 1, 1)?;
            let text = text_of(&values[0])?;
            let ch = text.chars().next().ok_or(FormulaError::Value)?;
            number(ch as u32 as f64)
        }
        "CHAR" | "UNICHAR" => {
            arity_values(&values, 1, 1)?;
            let code = number_of(&values[0])?;
            if code < 1.0 {
                return Err(FormulaError::Value);
            }
            let ch = char::from_u32(code.floor() as u32).ok_or(FormulaError::Value)?;
            Ok(FormulaValue::Text(ch.to_string()))
        }
        "LEFT" | "RIGHT" => {
            arity_values(&values, 1, 2)?;
            let text = text_of(&values[0])?;
            let count = match values.get(1) {
                Some(value) => count_of(value)?,
                None => 1,
            };
            let chars: Vec<char> = text.chars().collect();
            let count = count.min(chars.len());
            Ok(FormulaValue::Text(if name == "LEFT" {
                chars.into_iter().take(count).collect()
            } else {
                let start = chars.len() - count;
                chars.into_iter().skip(start).collect()
            }))
        }
        "MID" => {
            arity_values(&values, 3, 3)?;
            let text = text_of(&values[0])?;
            let start = number_of(&values[1])?;
            if start < 1.0 {
                return Err(FormulaError::Value);
            }
            let count = count_of(&values[2])?;
            let chars: Vec<char> = text.chars().collect();
            let start = start.floor() as usize;
            if start > chars.len() {
                return Ok(FormulaValue::Text(String::new()));
            }
            Ok(FormulaValue::Text(
                chars.into_iter().skip(start - 1).take(count).collect(),
            ))
        }
        "CONCAT" => {
            arity_values(&values, 2, 2)?;
            Ok(FormulaValue::Text(format!(
                "{}{}",
                text_of(&values[0])?,
                text_of(&values[1])?
            )))
        }
        "CONCATENATE" => {
            arity_values(&values, 1, usize::MAX)?;
            let mut out = String::new();
            for value in &values {
                for item in flatten(value) {
                    out.push_str(&item.to_text()?);
                }
            }
            Ok(FormulaValue::Text(out))
        }
        "LOWER" | "UPPER" | "TRIM" | "PROPER" | "CLEAN" => {
            arity_values(&values, 1, 1)?;
            let text = text_of(&values[0])?;
            Ok(FormulaValue::Text(match name {
                "LOWER" => text.to_lowercase(),
                "UPPER" => text.to_uppercase(),
                "TRIM" => text.split_whitespace().collect::<Vec<_>>().join(" "),
                "PROPER" => proper_case_text(&text),
                _ => clean_text(&text),
            }))
        }
        "SUBSTITUTE" => substitute(&values),
        "REPLACE" => {
            arity_values(&values, 4, 4)?;
            let text = text_of(&values[0])?;
            let position = positive_count_of(&values[1])?;
            let length = count_of(&values[2])?;
            let replacement = text_of(&values[3])?;
            let mut chars: Vec<char> = text.chars().collect();
            let start = (position - 1).min(chars.len());
            let end = (start + length).min(chars.len());
            chars.splice(start..end, replacement.chars());
            Ok(FormulaValue::Text(chars.into_iter().collect()))
        }
        "REPT" => {
            arity_values(&values, 2, 2)?;
            let text = text_of(&values[0])?;
            let count = count_of(&values[1])?;
            if count > 32_767 {
                return Err(FormulaError::Value);
            }
            Ok(FormulaValue::Text(text.repeat(count)))
        }
        "TO_TEXT" => {
            arity_values(&values, 1, 1)?;
            Ok(FormulaValue::Text(text_of(&values[0])?))
        }
        "T" => {
            arity_values(&values, 1, 1)?;
            Ok(FormulaValue::Text(match values[0].scalar() {
                FormulaValue::Text(text) => text,
                FormulaValue::Error(error) => return Err(error),
                _ => String::new(),
            }))
        }
        "BASE" => {
            arity_values(&values, 2, 3)?;
            let value = number_of(&values[0])?;
            let radix = number_of(&values[1])?;
            let min_length = match values.get(2) {
                Some(value) => number_of(value)?,
                None => 0.0,
            };
            if value < 0.0
                || !(2.0..37.0).contains(&radix)
                || min_length < 0.0
                || value.trunc() > u64::MAX as f64
                || min_length.trunc() > 1024.0
            {
                return Err(FormulaError::Num);
            }
            let mut digits = encode_formula_base_digits(value.trunc() as u64, radix.trunc() as u32)
                .map_err(|message| FormulaError::from_legacy(&message))?;
            let min_length = min_length.trunc() as usize;
            if digits.len() < min_length {
                digits = format!("{}{digits}", "0".repeat(min_length - digits.len()));
            }
            Ok(FormulaValue::Text(digits))
        }
        "JOIN" | "TEXTJOIN" => text_join(name, &values),
        "TEXT" => {
            arity_values(&values, 2, 2)?;
            let pattern = text_of(&values[1])?;
            let locale = Locale::for_tag(&ev.env.locale());
            match values[0].scalar() {
                FormulaValue::Error(error) => Err(error),
                FormulaValue::Text(text) if parse_number_text(&text).is_none() => {
                    Ok(FormulaValue::Text(text))
                }
                value => {
                    let number = value.to_number()?;
                    format::format_with_pattern(number, &pattern, &locale)
                        .map(FormulaValue::Text)
                        .ok_or(FormulaError::Value)
                }
            }
        }
        "DOLLAR" => {
            arity_values(&values, 1, 2)?;
            let value = number_of(&values[0])?;
            let decimals = match values.get(1) {
                Some(decimals) => number_of(decimals)?.trunc() as i32,
                None => 2,
            };
            let locale = Locale::for_tag(&ev.env.locale());
            let formatted = format::format_fixed(value.abs(), decimals.max(0) as usize, true, &locale);
            let symbol = locale.currency;
            Ok(FormulaValue::Text(if value < 0.0 {
                format!("({symbol}{formatted})")
            } else {
                format!("{symbol}{formatted}")
            }))
        }
        "FIXED" => {
            arity_values(&values, 1, 3)?;
            let value = number_of(&values[0])?;
            let decimals = match values.get(1) {
                Some(decimals) => number_of(decimals)?.trunc() as i32,
                None => 2,
            };
            let no_commas = match values.get(2) {
                Some(flag) => flag.to_bool()?,
                None => false,
            };
            let locale = Locale::for_tag(&ev.env.locale());
            Ok(FormulaValue::Text(format::format_fixed(
                value,
                decimals.max(0) as usize,
                !no_commas,
                &locale,
            )))
        }
        "SPLIT" => split_function(&values),
        "REGEXMATCH" | "REGEXEXTRACT" | "REGEXREPLACE" => regex_function(name, &values),
        "COUNTA" => {
            arity_values(&values, 1, usize::MAX)?;
            let mut count = 0.0;
            for value in &values {
                for item in flatten(value) {
                    if !item.is_blank() {
                        count += 1.0;
                    }
                }
            }
            number(count)
        }
        "COUNTBLANK" => {
            arity_values(&values, 1, usize::MAX)?;
            let mut count = 0.0;
            for value in &values {
                for item in flatten(value) {
                    if item.is_blank() || matches!(item, FormulaValue::Text(text) if text.is_empty())
                    {
                        count += 1.0;
                    }
                }
            }
            number(count)
        }
        "COUNTUNIQUE" => {
            arity_values(&values, 1, usize::MAX)?;
            let mut seen: Vec<FormulaValue> = Vec::new();
            for value in &values {
                for item in flatten(value) {
                    if item.is_blank() {
                        continue;
                    }
                    if !seen.iter().any(|existing| existing == item) {
                        seen.push(item.clone());
                    }
                }
            }
            number(seen.len() as f64)
        }
        "AVERAGEA" | "MINA" | "MAXA" => {
            arity_values(&values, 1, usize::MAX)?;
            let mut numbers = Vec::new();
            for value in &values {
                match value {
                    FormulaValue::Array(array) => {
                        for item in &array.values {
                            match item {
                                FormulaValue::Blank => {}
                                FormulaValue::Error(error) => return Err(*error),
                                FormulaValue::Number(number) => numbers.push(*number),
                                FormulaValue::Bool(flag) => {
                                    numbers.push(if *flag { 1.0 } else { 0.0 })
                                }
                                _ => numbers.push(0.0),
                            }
                        }
                    }
                    FormulaValue::Blank => {}
                    FormulaValue::Error(error) => return Err(*error),
                    FormulaValue::Text(text) => {
                        numbers.push(parse_formula_text_number(text).unwrap_or(0.0))
                    }
                    other => numbers.push(other.to_number()?),
                }
            }
            if numbers.is_empty() {
                return Err(if name == "AVERAGEA" {
                    FormulaError::Div0
                } else {
                    FormulaError::Value
                });
            }
            number(match name {
                "AVERAGEA" => numbers.iter().sum::<f64>() / numbers.len() as f64,
                "MINA" => numbers.into_iter().fold(f64::INFINITY, f64::min),
                _ => numbers.into_iter().fold(f64::NEG_INFINITY, f64::max),
            })
        }
        "SUMPRODUCT" => {
            arity_values(&values, 1, usize::MAX)?;
            let arrays: Vec<Vec<FormulaValue>> = values
                .iter()
                .map(|value| flatten(value).into_iter().cloned().collect())
                .collect();
            let length = arrays[0].len();
            if arrays.iter().any(|array| array.len() != length) {
                return Err(FormulaError::Value);
            }
            let mut total = 0.0;
            for index in 0..length {
                let mut product = 1.0;
                for array in &arrays {
                    product *= match &array[index] {
                        FormulaValue::Number(value) => *value,
                        FormulaValue::Error(error) => return Err(*error),
                        FormulaValue::Bool(flag) => {
                            if *flag {
                                1.0
                            } else {
                                0.0
                            }
                        }
                        _ => 0.0,
                    };
                }
                total += product;
            }
            number(total)
        }
        "TRANSPOSE" => {
            arity_values(&values, 1, 1)?;
            Ok(match &values[0] {
                FormulaValue::Array(array) => FormulaValue::Array(array.transpose()),
                other => other.clone(),
            })
        }
        "FLATTEN" => {
            arity_values(&values, 1, usize::MAX)?;
            let mut out = Vec::new();
            for value in &values {
                out.extend(flatten(value).into_iter().cloned());
            }
            Ok(FormulaValue::Array(FormulaArray::column(out)))
        }
        "ISEVEN" | "ISODD" | "ISBETWEEN" | "AND" | "OR" | "XOR" | "NOT" | "EQ" | "NE" | "GT"
        | "GTE" | "LT" | "LTE" => {
            let numbers = collect_numbers(name, &values)?;
            let result = legacy(evaluate_function(name, numbers))?;
            Ok(FormulaValue::Bool(result.to_number()? != 0.0))
        }
        _ => {
            let numbers = collect_numbers(name, &values)?;
            match evaluate_function(name, numbers) {
                Ok(value) if value.is_finite() => Ok(if returns_bool(name) {
                    FormulaValue::Bool(value != 0.0)
                } else {
                    FormulaValue::Number(value)
                }),
                Ok(_) => Err(FormulaError::Num),
                Err(message) if message.starts_with("#NAME") => Err(FormulaError::Name),
                Err(message) => Err(FormulaError::from_legacy(&message)),
            }
        }
    }
}

/// Flattens a value into scalar items (arrays yield their elements).
fn flatten(value: &FormulaValue) -> Vec<&FormulaValue> {
    match value {
        FormulaValue::Array(array) => array.values.iter().collect(),
        other => vec![other],
    }
}

fn text_of(value: &FormulaValue) -> Result<String, FormulaError> {
    value.scalar().to_text()
}

fn number_of(value: &FormulaValue) -> Result<f64, FormulaError> {
    value.scalar().to_number()
}

fn count_of(value: &FormulaValue) -> Result<usize, FormulaError> {
    let number = number_of(value)?;
    if number < 0.0 {
        return Err(FormulaError::Value);
    }
    Ok(number.floor() as usize)
}

fn positive_count_of(value: &FormulaValue) -> Result<usize, FormulaError> {
    let number = number_of(value)?;
    if number < 1.0 {
        return Err(FormulaError::Value);
    }
    Ok(number.floor() as usize)
}

/// Collects numeric arguments for the numeric function core. Ranges
/// contribute only numeric cells; scalar arguments are coerced.
fn collect_numbers(name: &str, values: &[FormulaValue]) -> Result<Vec<f64>, FormulaError> {
    let aggregate = is_aggregate(name) || name == "COUNT";
    let mut numbers = Vec::new();
    for value in values {
        match value {
            FormulaValue::Array(array) => {
                for item in &array.values {
                    match item {
                        FormulaValue::Number(number) => numbers.push(*number),
                        FormulaValue::Error(error) => return Err(*error),
                        _ => {}
                    }
                }
            }
            FormulaValue::Blank if aggregate => {}
            FormulaValue::Text(text) if name == "COUNT" => {
                if let Some(number) = parse_number_text(text) {
                    numbers.push(number);
                }
            }
            other => numbers.push(other.to_number()?),
        }
    }
    Ok(numbers)
}

fn if_function(ev: &mut Evaluator<'_>, args: &[Expr]) -> FResult {
    arity(args, 2, 3)?;
    let condition = ev.eval(&args[0]);
    if let FormulaValue::Array(array) = &condition {
        if ev.array_mode {
            let mut out = Vec::with_capacity(array.values.len());
            let when_true = ev.eval(&args[1]);
            let when_false = args
                .get(2)
                .map(|arg| ev.eval(arg))
                .unwrap_or(FormulaValue::Bool(false));
            for (index, item) in array.values.iter().enumerate() {
                let pick = |value: &FormulaValue| match value {
                    FormulaValue::Array(inner) => inner
                        .values
                        .get(index)
                        .cloned()
                        .unwrap_or(FormulaValue::Error(FormulaError::Na)),
                    other => other.clone(),
                };
                out.push(match item.to_bool() {
                    Ok(true) => pick(&when_true),
                    Ok(false) => pick(&when_false),
                    Err(error) => FormulaValue::Error(error),
                });
            }
            return Ok(FormulaValue::Array(FormulaArray::new(
                array.rows, array.cols, out,
            )));
        }
    }
    if condition.scalar().to_bool()? {
        Ok(ev.eval(&args[1]))
    } else if let Some(when_false) = args.get(2) {
        Ok(ev.eval(when_false))
    } else {
        Ok(FormulaValue::Bool(false))
    }
}

fn ifs_function(ev: &mut Evaluator<'_>, args: &[Expr]) -> FResult {
    if args.is_empty() || args.len() % 2 != 0 {
        return Err(FormulaError::Value);
    }
    for pair in args.chunks(2) {
        if ev.eval_bool(&pair[0])? {
            return Ok(ev.eval(&pair[1]));
        }
    }
    Err(FormulaError::Na)
}

fn choose_function(ev: &mut Evaluator<'_>, args: &[Expr]) -> FResult {
    arity(args, 2, usize::MAX)?;
    let index = ev.eval_number(&args[0])?.trunc();
    if index < 1.0 || index > (args.len() - 1) as f64 {
        return Err(FormulaError::Num);
    }
    Ok(ev.eval(&args[index as usize]))
}

fn switch_function(ev: &mut Evaluator<'_>, args: &[Expr]) -> FResult {
    arity(args, 3, usize::MAX)?;
    let expression = ev.eval_scalar(&args[0]);
    if let FormulaValue::Error(error) = expression {
        return Err(error);
    }
    let has_default = args.len() % 2 == 0;
    let case_end = if has_default {
        args.len() - 1
    } else {
        args.len()
    };
    for index in (1..case_end).step_by(2) {
        let case_key = ev.eval_scalar(&args[index]);
        if let FormulaValue::Error(error) = case_key {
            return Err(error);
        }
        if values_match(&expression, &case_key) {
            return Ok(ev.eval(&args[index + 1]));
        }
    }
    if has_default {
        Ok(ev.eval(&args[args.len() - 1]))
    } else {
        Err(FormulaError::Na)
    }
}

fn error_handler(ev: &mut Evaluator<'_>, name: &str, args: &[Expr]) -> FResult {
    arity(args, 1, 2)?;
    let value = ev.eval(&args[0]);
    let caught = match value.scalar() {
        FormulaValue::Error(error) => name == "IFERROR" || error == FormulaError::Na,
        _ => false,
    };
    if !caught {
        return Ok(value);
    }
    match args.get(1) {
        Some(fallback) => Ok(ev.eval(fallback)),
        None => Ok(FormulaValue::Blank),
    }
}

fn value_predicate(ev: &mut Evaluator<'_>, name: &str, args: &[Expr]) -> FResult {
    arity(args, 1, 1)?;
    let value = ev.eval_scalar(&args[0]);
    let result = match name {
        "ISERROR" => value.is_error(),
        "ISERR" => value.error().is_some_and(|error| error != FormulaError::Na),
        "ISNA" => value.error() == Some(FormulaError::Na),
        "ISNUMBER" => matches!(value, FormulaValue::Number(_)),
        "ISTEXT" => matches!(value, FormulaValue::Text(_)),
        "ISNONTEXT" => !matches!(value, FormulaValue::Text(_)),
        "ISLOGICAL" => matches!(value, FormulaValue::Bool(_)),
        _ => unreachable!(),
    };
    Ok(FormulaValue::Bool(result))
}

fn cell_state_predicate(ev: &mut Evaluator<'_>, name: &str, args: &[Expr]) -> FResult {
    arity(args, 1, 1)?;
    match ev.resolve_range_expr(&args[0]) {
        Ok(range) => {
            let meta = ev.env.cell_meta(range.sheet, range.col_start, range.row_start);
            Ok(FormulaValue::Bool(match name {
                "ISBLANK" => meta.is_none_or(|cell| {
                    cell.user_kind == "empty" && cell.spill_source.is_none()
                }),
                _ => meta.is_some_and(|cell| cell.user_kind == "formula"),
            }))
        }
        Err(_) => {
            let value = ev.eval_scalar(&args[0]);
            if let FormulaValue::Error(error) = value {
                return Err(error);
            }
            Ok(FormulaValue::Bool(name == "ISBLANK" && value.is_blank()))
        }
    }
}

fn isref(ev: &mut Evaluator<'_>, args: &[Expr]) -> FResult {
    arity(args, 1, 1)?;
    Ok(FormulaValue::Bool(ev.resolve_range_expr(&args[0]).is_ok()))
}

fn isdate(ev: &mut Evaluator<'_>, args: &[Expr]) -> FResult {
    arity(args, 1, 1)?;
    if let Ok(range) = ev.resolve_range_expr(&args[0]) {
        let value = ev.env.cell_value(range.sheet, range.col_start, range.row_start);
        let meta = ev.env.cell_meta(range.sheet, range.col_start, range.row_start);
        return Ok(FormulaValue::Bool(match value {
            FormulaValue::Text(text) => parse_formula_datevalue(&text).is_ok(),
            FormulaValue::Number(_) => meta.is_some_and(|cell| {
                (cell.user_kind == "formula" && formula_source_is_date_producing(&cell.user_value))
                    || cell
                        .format
                        .number_format
                        .as_deref()
                        .is_some_and(format::is_date_format)
            }),
            _ => false,
        }));
    }
    if let Expr::Call { name, .. } = &args[0] {
        if formula_source_is_date_producing(&format!("{name}(")) {
            return Ok(FormulaValue::Bool(matches!(
                ev.eval_scalar(&args[0]),
                FormulaValue::Number(_)
            )));
        }
    }
    match ev.eval_scalar(&args[0]) {
        FormulaValue::Text(text) => Ok(FormulaValue::Bool(parse_formula_datevalue(&text).is_ok())),
        _ => Ok(FormulaValue::Bool(false)),
    }
}

fn address_info(ev: &mut Evaluator<'_>, name: &str, args: &[Expr]) -> FResult {
    if args.is_empty() {
        return match name {
            "ROW" => number(ev.row as f64),
            "COLUMN" => number(ev.col as f64),
            _ => Err(FormulaError::Value),
        };
    }
    arity(args, 1, 1)?;
    let range = ev.resolve_range_expr(&args[0])?;
    number(match name {
        "ROW" => range.row_start as f64,
        "COLUMN" => range.col_start as f64,
        "ROWS" => range.height() as f64,
        _ => range.width() as f64,
    })
}

fn index_function(ev: &mut Evaluator<'_>, args: &[Expr]) -> FResult {
    arity(args, 2, 3)?;
    let range = ev.resolve_range_expr(&args[0])?;
    let row = ev.eval_number(&args[1])?;
    let column = match args.get(2) {
        Some(column) if !matches!(column, Expr::Blank) => ev.eval_number(column)?,
        _ => {
            if range.width() == 1 || row != 0.0 {
                1.0
            } else {
                0.0
            }
        }
    };
    if row < 0.0 || column < 0.0 {
        return Err(FormulaError::Value);
    }
    let row = row.trunc() as u32;
    let column = column.trunc() as u32;
    if row > range.height() || column > range.width() {
        return Err(FormulaError::Ref);
    }
    let array = ev.range_array(&range);
    if row == 0 && column == 0 {
        return Ok(FormulaValue::Array(array));
    }
    if row == 0 {
        return Ok(FormulaValue::Array(FormulaArray::column(
            array.column_values(column as usize - 1),
        )));
    }
    if column == 0 {
        return Ok(FormulaValue::Array(FormulaArray::row(
            array.row_values(row as usize - 1).to_vec(),
        )));
    }
    Ok(array.get(row as usize - 1, column as usize - 1).clone())
}

/// Resolves `OFFSET`/`INDIRECT` calls to a workbook range.
pub fn resolve_dynamic_range(
    ev: &mut Evaluator<'_>,
    name: &str,
    args: &[Expr],
) -> Result<ResolvedRange, FormulaError> {
    match name {
        "OFFSET" => {
            arity(args, 3, 5)?;
            let base = ev.resolve_range_expr(&args[0])?;
            let rows = ev.eval_number(&args[1])?.trunc() as i64;
            let cols = ev.eval_number(&args[2])?.trunc() as i64;
            let height = match args.get(3) {
                Some(arg) if !matches!(arg, Expr::Blank) => ev.eval_number(arg)?.trunc() as i64,
                _ => base.height() as i64,
            };
            let width = match args.get(4) {
                Some(arg) if !matches!(arg, Expr::Blank) => ev.eval_number(arg)?.trunc() as i64,
                _ => base.width() as i64,
            };
            if height < 1 || width < 1 {
                return Err(FormulaError::Value);
            }
            let row_start = base.row_start as i64 + rows;
            let col_start = base.col_start as i64 + cols;
            if row_start < 1 || col_start < 1 {
                return Err(FormulaError::Ref);
            }
            let range = ResolvedRange {
                sheet: base.sheet,
                col_start: col_start as u32,
                col_end: (col_start + width - 1) as u32,
                row_start: row_start as u32,
                row_end: (row_start + height - 1) as u32,
            };
            Ok(range)
        }
        "INDIRECT" => {
            arity(args, 1, 2)?;
            let text = ev.eval_text(&args[0])?;
            if let Some(reference) = parse_reference(&text) {
                return ev.resolve_ref(&reference);
            }
            ev.resolve_name(&text.trim().to_ascii_uppercase())
                .map_err(|_| FormulaError::Ref)
        }
        _ => Err(FormulaError::Value),
    }
}

fn address_function(ev: &mut Evaluator<'_>, args: &[Expr]) -> FResult {
    arity(args, 2, 5)?;
    let row = ev.eval_number(&args[0])?;
    let column = ev.eval_number(&args[1])?;
    if row < 1.0 || column < 1.0 {
        return Err(FormulaError::Value);
    }
    let mode = match args.get(2) {
        Some(arg) if !matches!(arg, Expr::Blank) => ev.eval_number(arg)?.trunc() as i32,
        _ => 1,
    };
    let use_a1 = match args.get(3) {
        Some(arg) if !matches!(arg, Expr::Blank) => ev.eval_bool(arg)?,
        _ => true,
    };
    let sheet = match args.get(4) {
        Some(arg) if !matches!(arg, Expr::Blank) => Some(ev.eval_text(arg)?),
        _ => None,
    };
    let (col_abs, row_abs) = match mode {
        1 => (true, true),
        2 => (false, true),
        3 => (true, false),
        4 => (false, false),
        _ => return Err(FormulaError::Value),
    };
    let row = row.trunc() as u32;
    let column = column.trunc() as u32;
    let body = if use_a1 {
        let coord = super::formula::CellCoord {
            col: Some(column),
            row: Some(row),
            col_abs,
            row_abs,
        };
        coord.to_text()
    } else {
        format!(
            "R{}C{}",
            if row_abs {
                row.to_string()
            } else {
                format!("[{row}]")
            },
            if col_abs {
                column.to_string()
            } else {
                format!("[{column}]")
            }
        )
    };
    Ok(FormulaValue::Text(match sheet {
        Some(sheet) if !sheet.is_empty() => {
            format!("{}!{body}", crate::formula_sheet_title_prefix(&sheet))
        }
        _ => body,
    }))
}

fn subtotal(ev: &mut Evaluator<'_>, args: &[Expr]) -> FResult {
    arity(args, 2, usize::MAX)?;
    let code = ev.eval_number(&args[0])?.trunc() as i32;
    let (function, skip_hidden) = match code {
        1..=11 => (code, false),
        101..=111 => (code - 100, true),
        _ => return Err(FormulaError::Value),
    };
    let mut numbers = Vec::new();
    let mut non_blank = 0.0;
    for arg in &args[1..] {
        let range = ev.resolve_range_expr(arg)?;
        for (col, row) in range.cells() {
            if skip_hidden && ev.env.row_hidden(range.sheet, row) {
                continue;
            }
            match ev.env.cell_value(range.sheet, col, row) {
                FormulaValue::Number(value) => {
                    numbers.push(value);
                    non_blank += 1.0;
                }
                FormulaValue::Error(error) => return Err(error),
                FormulaValue::Blank => {}
                _ => non_blank += 1.0,
            }
        }
    }
    let name = match function {
        1 => "AVERAGE",
        2 => "COUNT",
        3 => return number(non_blank),
        4 => "MAX",
        5 => "MIN",
        6 => "PRODUCT",
        7 => "STDEV",
        8 => "STDEVP",
        9 => "SUM",
        10 => "VAR",
        _ => "VARP",
    };
    legacy(evaluate_function(name, numbers))
}

fn sequence(ev: &mut Evaluator<'_>, args: &[Expr]) -> FResult {
    arity(args, 1, 4)?;
    let rows = ev.eval_number(&args[0])?.trunc();
    let cols = match args.get(1) {
        Some(arg) if !matches!(arg, Expr::Blank) => ev.eval_number(arg)?.trunc(),
        _ => 1.0,
    };
    let start = match args.get(2) {
        Some(arg) if !matches!(arg, Expr::Blank) => ev.eval_number(arg)?,
        _ => 1.0,
    };
    let step = match args.get(3) {
        Some(arg) if !matches!(arg, Expr::Blank) => ev.eval_number(arg)?,
        _ => 1.0,
    };
    if rows < 1.0 || cols < 1.0 || rows * cols > 100_000.0 {
        return Err(FormulaError::Value);
    }
    let (rows, cols) = (rows as usize, cols as usize);
    let values = (0..rows * cols)
        .map(|index| FormulaValue::Number(start + step * index as f64))
        .collect();
    Ok(FormulaValue::Array(FormulaArray::new(rows, cols, values)))
}

fn array_of(value: FormulaValue) -> Result<FormulaArray, FormulaError> {
    match value {
        FormulaValue::Array(array) => Ok(array),
        FormulaValue::Error(error) => Err(error),
        other => Ok(FormulaArray::new(1, 1, vec![other])),
    }
}

fn sort_function(ev: &mut Evaluator<'_>, args: &[Expr]) -> FResult {
    arity(args, 1, usize::MAX)?;
    let array = array_of(ev.eval(&args[0]))?;
    let mut keys: Vec<(usize, bool)> = Vec::new();
    let mut index = 1;
    while index < args.len() {
        let column = ev.eval_number(&args[index])?.trunc();
        if column < 1.0 || column as usize > array.cols {
            return Err(FormulaError::Value);
        }
        let ascending = match args.get(index + 1) {
            Some(arg) => ev.eval_bool(arg)?,
            None => true,
        };
        keys.push((column as usize - 1, ascending));
        index += 2;
    }
    if keys.is_empty() {
        keys.push((0, true));
    }
    let mut rows: Vec<Vec<FormulaValue>> = (0..array.rows)
        .map(|row| array.row_values(row).to_vec())
        .collect();
    rows.sort_by(|left, right| {
        for (column, ascending) in &keys {
            let ordering = compare_values(&left[*column], &right[*column])
                .unwrap_or(std::cmp::Ordering::Equal);
            if ordering.is_ne() {
                return if *ascending {
                    ordering
                } else {
                    ordering.reverse()
                };
            }
        }
        std::cmp::Ordering::Equal
    });
    Ok(FormulaValue::Array(FormulaArray::from_rows(rows)))
}

fn filter_function(ev: &mut Evaluator<'_>, args: &[Expr]) -> FResult {
    arity(args, 2, usize::MAX)?;
    let array = array_of(ev.eval(&args[0]))?;
    let mut conditions = Vec::new();
    for arg in &args[1..] {
        conditions.push(array_of(ev.eval(arg))?);
    }
    let by_rows = conditions
        .iter()
        .all(|condition| condition.values.len() == array.rows);
    let by_cols = !by_rows
        && conditions
            .iter()
            .all(|condition| condition.values.len() == array.cols);
    if !by_rows && !by_cols {
        return Err(FormulaError::Value);
    }
    let count = if by_rows { array.rows } else { array.cols };
    let mut keep = Vec::with_capacity(count);
    for index in 0..count {
        let mut matched = true;
        for condition in &conditions {
            matched &= condition.values[index].to_bool()?;
        }
        keep.push(matched);
    }
    if keep.iter().all(|flag| !flag) {
        return Err(FormulaError::Na);
    }
    let rows: Vec<Vec<FormulaValue>> = if by_rows {
        (0..array.rows)
            .filter(|row| keep[*row])
            .map(|row| array.row_values(row).to_vec())
            .collect()
    } else {
        (0..array.rows)
            .map(|row| {
                (0..array.cols)
                    .filter(|col| keep[*col])
                    .map(|col| array.get(row, col).clone())
                    .collect()
            })
            .collect()
    };
    Ok(FormulaValue::Array(FormulaArray::from_rows(rows)))
}

fn unique_function(ev: &mut Evaluator<'_>, args: &[Expr]) -> FResult {
    arity(args, 1, 3)?;
    let mut array = array_of(ev.eval(&args[0]))?;
    let by_column = match args.get(1) {
        Some(arg) if !matches!(arg, Expr::Blank) => ev.eval_bool(arg)?,
        _ => false,
    };
    let exactly_once = match args.get(2) {
        Some(arg) if !matches!(arg, Expr::Blank) => ev.eval_bool(arg)?,
        _ => false,
    };
    if by_column {
        array = array.transpose();
    }
    let rows: Vec<Vec<FormulaValue>> = (0..array.rows)
        .map(|row| array.row_values(row).to_vec())
        .collect();
    let row_matches = |left: &[FormulaValue], right: &[FormulaValue]| {
        left.iter()
            .zip(right.iter())
            .all(|(left, right)| values_match(left, right))
    };
    let mut out: Vec<Vec<FormulaValue>> = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        let occurrences = rows.iter().filter(|other| row_matches(row, other)).count();
        if exactly_once && occurrences != 1 {
            continue;
        }
        let first = rows.iter().position(|other| row_matches(row, other));
        if first == Some(index) {
            out.push(row.clone());
        }
    }
    if out.is_empty() {
        return Err(FormulaError::Na);
    }
    let mut result = FormulaArray::from_rows(out);
    if by_column {
        result = result.transpose();
    }
    Ok(FormulaValue::Array(result))
}

fn workday_function(ev: &mut Evaluator<'_>, name: &str, args: &[Expr]) -> FResult {
    arity(args, 2, 3)?;
    let start = ev.eval_number(&args[0])?;
    let end_or_days = ev.eval_number(&args[1])?;
    let mut holidays = BTreeSet::new();
    if let Some(arg) = args.get(2) {
        for item in flatten(&ev.eval(arg)) {
            match item {
                FormulaValue::Number(value) => {
                    holidays.insert(value.trunc() as i64);
                }
                FormulaValue::Error(error) => return Err(*error),
                _ => {}
            }
        }
    }
    legacy(if name == "NETWORKDAYS" {
        networkdays_value_with_holidays(start, end_or_days, &holidays)
    } else {
        workday_value_with_holidays(start, end_or_days, &holidays)
    })
}

fn text_search(name: &str, values: &[FormulaValue]) -> FResult {
    arity_values(values, 2, 3)?;
    let pattern = text_of(&values[0])?;
    let haystack = text_of(&values[1])?;
    let start = match values.get(2) {
        Some(value) => number_of(value)?,
        None => 1.0,
    };
    if start < 1.0 {
        return Err(FormulaError::Value);
    }
    let start = start.floor() as usize;
    let mut positions: Vec<usize> = haystack.char_indices().map(|(index, _)| index).collect();
    positions.push(haystack.len());
    if start > positions.len() {
        return Err(FormulaError::Value);
    }
    let byte_start = positions[start - 1];
    if name == "SEARCH" {
        return legacy(wildcard_search_position(&haystack, &pattern, byte_start));
    }
    let found = haystack[byte_start..]
        .find(&pattern)
        .ok_or(FormulaError::Value)?;
    number((haystack[..byte_start + found].chars().count() + 1) as f64)
}

fn substitute(values: &[FormulaValue]) -> FResult {
    arity_values(values, 3, 4)?;
    let text = text_of(&values[0])?;
    let old = text_of(&values[1])?;
    let new = text_of(&values[2])?;
    if old.is_empty() {
        return Ok(FormulaValue::Text(text));
    }
    let Some(occurrence) = values.get(3) else {
        return Ok(FormulaValue::Text(text.replace(&old, &new)));
    };
    let occurrence = positive_count_of(occurrence)?;
    let mut seen = 0usize;
    let mut cursor = 0usize;
    let mut out = String::new();
    while let Some(offset) = text[cursor..].find(&old) {
        let start = cursor + offset;
        seen += 1;
        out.push_str(&text[cursor..start]);
        if seen == occurrence {
            out.push_str(&new);
        } else {
            out.push_str(&old);
        }
        cursor = start + old.len();
    }
    out.push_str(&text[cursor..]);
    Ok(FormulaValue::Text(out))
}

fn text_join(name: &str, values: &[FormulaValue]) -> FResult {
    let (delimiter, ignore_empty, start) = if name == "TEXTJOIN" {
        arity_values(values, 3, usize::MAX)?;
        (text_of(&values[0])?, values[1].to_bool()?, 2)
    } else {
        arity_values(values, 2, usize::MAX)?;
        (text_of(&values[0])?, false, 1)
    };
    let mut parts = Vec::new();
    for value in &values[start..] {
        for item in flatten(value) {
            let text = item.to_text()?;
            if ignore_empty && text.is_empty() {
                continue;
            }
            parts.push(text);
        }
    }
    Ok(FormulaValue::Text(parts.join(&delimiter)))
}

fn split_function(values: &[FormulaValue]) -> FResult {
    arity_values(values, 2, 4)?;
    let text = text_of(&values[0])?;
    let delimiter = text_of(&values[1])?;
    let split_by_each = match values.get(2) {
        Some(value) => value.to_bool()?,
        None => true,
    };
    let remove_empty = match values.get(3) {
        Some(value) => value.to_bool()?,
        None => true,
    };
    if delimiter.is_empty() {
        return Ok(FormulaValue::Array(FormulaArray::row(vec![FormulaValue::Text(
            text,
        )])));
    }
    let parts: Vec<String> = if split_by_each {
        let delimiters: Vec<char> = delimiter.chars().collect();
        text.split(|ch| delimiters.contains(&ch))
            .map(str::to_string)
            .collect()
    } else {
        text.split(delimiter.as_str()).map(str::to_string).collect()
    };
    let items: Vec<FormulaValue> = parts
        .into_iter()
        .filter(|part| !(remove_empty && part.is_empty()))
        .map(|part| match parse_number_text(&part) {
            Some(number) if !part.trim().is_empty() && !part.contains('%') => {
                FormulaValue::Number(number)
            }
            _ => FormulaValue::Text(part),
        })
        .collect();
    if items.is_empty() {
        return Ok(FormulaValue::Array(FormulaArray::row(vec![FormulaValue::Text(
            String::new(),
        )])));
    }
    Ok(FormulaValue::Array(FormulaArray::row(items)))
}

fn regex_function(name: &str, values: &[FormulaValue]) -> FResult {
    let text = text_of(&values[0])?;
    let pattern = text_of(values.get(1).ok_or(FormulaError::Value)?)?;
    let regex = regex::Regex::new(&pattern).map_err(|_| FormulaError::Value)?;
    match name {
        "REGEXMATCH" => {
            arity_values(values, 2, 2)?;
            Ok(FormulaValue::Bool(regex.is_match(&text)))
        }
        "REGEXEXTRACT" => {
            arity_values(values, 2, 2)?;
            let captures = regex.captures(&text).ok_or(FormulaError::Na)?;
            let matched = if captures.len() > 1 {
                captures.get(1)
            } else {
                captures.get(0)
            };
            Ok(FormulaValue::Text(
                matched.map(|m| m.as_str().to_string()).unwrap_or_default(),
            ))
        }
        _ => {
            arity_values(values, 3, 3)?;
            let replacement = text_of(&values[2])?;
            Ok(FormulaValue::Text(
                regex.replace_all(&text, replacement.as_str()).into_owned(),
            ))
        }
    }
}

/// A `COUNTIF`-style criterion.
enum Criterion {
    Number(CriterionOp, f64),
    Text(CriterionOp, String),
    Bool(CriterionOp, bool),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CriterionOp {
    Eq,
    Ne,
    Gt,
    Gte,
    Lt,
    Lte,
}

impl CriterionOp {
    fn matches(self, ordering: std::cmp::Ordering) -> bool {
        match self {
            CriterionOp::Eq => ordering.is_eq(),
            CriterionOp::Ne => ordering.is_ne(),
            CriterionOp::Gt => ordering.is_gt(),
            CriterionOp::Gte => ordering.is_ge(),
            CriterionOp::Lt => ordering.is_lt(),
            CriterionOp::Lte => ordering.is_le(),
        }
    }
}

fn parse_criterion(value: &FormulaValue) -> Result<Criterion, FormulaError> {
    let text = match value.scalar() {
        FormulaValue::Number(number) => return Ok(Criterion::Number(CriterionOp::Eq, number)),
        FormulaValue::Bool(flag) => return Ok(Criterion::Bool(CriterionOp::Eq, flag)),
        FormulaValue::Error(error) => return Err(error),
        FormulaValue::Blank => String::new(),
        FormulaValue::Text(text) => text,
        FormulaValue::Array(_) => return Err(FormulaError::Value),
    };
    let text = text.trim();
    let (op, rest) = if let Some(rest) = text.strip_prefix(">=") {
        (CriterionOp::Gte, rest)
    } else if let Some(rest) = text.strip_prefix("<=") {
        (CriterionOp::Lte, rest)
    } else if let Some(rest) = text.strip_prefix("<>") {
        (CriterionOp::Ne, rest)
    } else if let Some(rest) = text.strip_prefix('>') {
        (CriterionOp::Gt, rest)
    } else if let Some(rest) = text.strip_prefix('<') {
        (CriterionOp::Lt, rest)
    } else if let Some(rest) = text.strip_prefix('=') {
        (CriterionOp::Eq, rest)
    } else {
        (CriterionOp::Eq, text)
    };
    let rest = rest.trim();
    if let Ok(number) = rest.parse::<f64>() {
        if number.is_finite() {
            return Ok(Criterion::Number(op, number));
        }
        return Err(FormulaError::Num);
    }
    match rest.to_ascii_uppercase().as_str() {
        "TRUE" => return Ok(Criterion::Bool(op, true)),
        "FALSE" => return Ok(Criterion::Bool(op, false)),
        _ => {}
    }
    if !matches!(op, CriterionOp::Eq | CriterionOp::Ne) {
        return Err(FormulaError::Value);
    }
    Ok(Criterion::Text(op, rest.to_string()))
}

impl Criterion {
    fn matches(&self, value: &FormulaValue) -> bool {
        match self {
            Criterion::Number(op, target) => match value {
                FormulaValue::Number(number) => op.matches(
                    number
                        .partial_cmp(target)
                        .unwrap_or(std::cmp::Ordering::Equal),
                ),
                _ => *op == CriterionOp::Ne,
            },
            Criterion::Bool(op, target) => match value {
                FormulaValue::Bool(flag) => op.matches(flag.cmp(target)),
                _ => *op == CriterionOp::Ne,
            },
            Criterion::Text(op, pattern) => {
                let text = match value {
                    FormulaValue::Blank => String::new(),
                    FormulaValue::Error(error) => error.code().to_string(),
                    other => other.to_text().unwrap_or_default(),
                };
                let matched = wildcard_text_matches(
                    text.to_ascii_lowercase().as_bytes(),
                    pattern.to_ascii_lowercase().as_bytes(),
                );
                match op {
                    CriterionOp::Eq => matched,
                    _ => !matched,
                }
            }
        }
    }
}

fn conditional_aggregate(ev: &mut Evaluator<'_>, name: &str, args: &[Expr]) -> FResult {
    if name == "COUNTIF" {
        arity(args, 2, 2)?;
    } else {
        arity(args, 2, 3)?;
    }
    let criteria_range = ev.resolve_range_expr(&args[0])?;
    let criterion = parse_criterion(&ev.eval(&args[1]))?;
    let sum_range = match args.get(2) {
        Some(arg) => Some(ev.resolve_range_expr(arg)?),
        None => None,
    };
    let criteria_cells: Vec<(u32, u32)> = criteria_range.cells().collect();
    let sum_cells: Vec<(u32, u32)> = match &sum_range {
        Some(range) => {
            let cells: Vec<(u32, u32)> = range.cells().collect();
            if cells.len() != criteria_cells.len() {
                return Err(FormulaError::Value);
            }
            cells
        }
        None => criteria_cells.clone(),
    };
    let sum_sheet = sum_range.map(|range| range.sheet).unwrap_or(criteria_range.sheet);
    let mut total = 0.0;
    let mut count = 0.0;
    for (index, (col, row)) in criteria_cells.iter().enumerate() {
        let value = ev.env.cell_value(criteria_range.sheet, *col, *row);
        if !criterion.matches(&value) {
            continue;
        }
        if name == "COUNTIF" {
            total += 1.0;
            continue;
        }
        let (sum_col, sum_row) = sum_cells[index];
        match ev.env.cell_value(sum_sheet, sum_col, sum_row) {
            FormulaValue::Number(number) => {
                total += number;
                count += 1.0;
            }
            FormulaValue::Error(error) => return Err(error),
            _ => {}
        }
    }
    if name == "AVERAGEIF" {
        if count == 0.0 {
            return Err(FormulaError::Div0);
        }
        total /= count;
    }
    number(total)
}

fn multi_conditional_aggregate(ev: &mut Evaluator<'_>, name: &str, args: &[Expr]) -> FResult {
    let (aggregate_range, first_criteria_index) = if name == "COUNTIFS" {
        if args.len() < 2 || args.len() % 2 != 0 {
            return Err(FormulaError::Value);
        }
        (None, 0usize)
    } else {
        if args.len() < 3 || args.len() % 2 == 0 {
            return Err(FormulaError::Value);
        }
        (Some(ev.resolve_range_expr(&args[0])?), 1usize)
    };
    let mut criteria = Vec::new();
    let mut index = first_criteria_index;
    while index < args.len() {
        let range = ev.resolve_range_expr(&args[index])?;
        let criterion = parse_criterion(&ev.eval(&args[index + 1]))?;
        criteria.push((range, criterion));
        index += 2;
    }
    let cell_lists: Vec<Vec<(u32, u32)>> = criteria
        .iter()
        .map(|(range, _)| range.cells().collect())
        .collect();
    let width = cell_lists[0].len();
    if cell_lists.iter().any(|cells| cells.len() != width) {
        return Err(FormulaError::Value);
    }
    let aggregate_cells: Option<Vec<(u32, u32)>> = aggregate_range.map(|range| range.cells().collect());
    if let Some(cells) = &aggregate_cells {
        if cells.len() != width {
            return Err(FormulaError::Value);
        }
    }
    let mut total = 0.0;
    let mut count = 0.0;
    let mut best: Option<f64> = None;
    for position in 0..width {
        let mut matched = true;
        for ((range, criterion), cells) in criteria.iter().zip(cell_lists.iter()) {
            let (col, row) = cells[position];
            let value = ev.env.cell_value(range.sheet, col, row);
            if !criterion.matches(&value) {
                matched = false;
                break;
            }
        }
        if !matched {
            continue;
        }
        if name == "COUNTIFS" {
            total += 1.0;
            continue;
        }
        let range = aggregate_range.expect("aggregate range");
        let (col, row) = aggregate_cells.as_ref().expect("aggregate cells")[position];
        match ev.env.cell_value(range.sheet, col, row) {
            FormulaValue::Number(number) => {
                total += number;
                count += 1.0;
                best = Some(match (name, best) {
                    ("MINIFS", Some(current)) => current.min(number),
                    ("MAXIFS", Some(current)) => current.max(number),
                    _ => number,
                });
            }
            FormulaValue::Error(error) => return Err(error),
            _ => {}
        }
    }
    if name == "MINIFS" || name == "MAXIFS" {
        return number(best.unwrap_or(0.0));
    }
    if name == "AVERAGEIFS" {
        if count == 0.0 {
            return Err(FormulaError::Div0);
        }
        total /= count;
    }
    number(total)
}

fn lookup_key(ev: &mut Evaluator<'_>, expr: &Expr) -> Result<FormulaValue, FormulaError> {
    match ev.eval_scalar(expr) {
        FormulaValue::Error(error) => Err(error),
        value => Ok(value),
    }
}

/// Finds the position of `key` in `values` using Google Sheets match modes:
/// `0` exact, `1` largest value <= key, `-1` smallest value >= key.
fn match_position(key: &FormulaValue, values: &[FormulaValue], mode: i32) -> Option<usize> {
    match mode {
        0 => values.iter().position(|value| values_match(key, value)),
        1 => {
            let mut best = None;
            for (index, value) in values.iter().enumerate() {
                if value.is_blank() || value.is_error() {
                    continue;
                }
                match compare_values(value, key) {
                    Ok(ordering) if ordering.is_le() => best = Some(index),
                    Ok(_) => break,
                    Err(_) => {}
                }
            }
            best
        }
        _ => {
            let mut best = None;
            for (index, value) in values.iter().enumerate() {
                if value.is_blank() || value.is_error() {
                    continue;
                }
                match compare_values(value, key) {
                    Ok(ordering) if ordering.is_ge() => best = Some(index),
                    Ok(_) => break,
                    Err(_) => {}
                }
            }
            best
        }
    }
}

fn match_function(ev: &mut Evaluator<'_>, args: &[Expr]) -> FResult {
    arity(args, 2, 3)?;
    let key = lookup_key(ev, &args[0])?;
    let range = ev.resolve_range_expr(&args[1])?;
    if range.width() != 1 && range.height() != 1 {
        return Err(FormulaError::Value);
    }
    let mode = match args.get(2) {
        Some(arg) if !matches!(arg, Expr::Blank) => ev.eval_number(arg)?.trunc() as i32,
        _ => 1,
    };
    let values = ev.range_array(&range).values;
    match match_position(&key, &values, mode.signum()) {
        Some(index) => number((index + 1) as f64),
        None => Err(FormulaError::Na),
    }
}

fn vh_lookup(ev: &mut Evaluator<'_>, name: &str, args: &[Expr]) -> FResult {
    arity(args, 3, 4)?;
    let key = lookup_key(ev, &args[0])?;
    let range = ev.resolve_range_expr(&args[1])?;
    let index = ev.eval_number(&args[2])?;
    if index < 1.0 {
        return Err(FormulaError::Value);
    }
    let index = index.trunc() as usize;
    let sorted = match args.get(3) {
        Some(arg) if !matches!(arg, Expr::Blank) => ev.eval_bool(arg)?,
        _ => true,
    };
    let array = ev.range_array(&range);
    let (keys, limit) = if name == "VLOOKUP" {
        (array.column_values(0), array.cols)
    } else {
        (array.row_values(0).to_vec(), array.rows)
    };
    if index > limit {
        return Err(FormulaError::Ref);
    }
    let position =
        match_position(&key, &keys, if sorted { 1 } else { 0 }).ok_or(FormulaError::Na)?;
    Ok(if name == "VLOOKUP" {
        array.get(position, index - 1).clone()
    } else {
        array.get(index - 1, position).clone()
    })
}

fn xlookup(ev: &mut Evaluator<'_>, args: &[Expr]) -> FResult {
    arity(args, 3, 6)?;
    let key = lookup_key(ev, &args[0])?;
    let lookup_range = ev.resolve_range_expr(&args[1])?;
    let result_range = ev.resolve_range_expr(&args[2])?;
    if lookup_range.width() != 1 && lookup_range.height() != 1 {
        return Err(FormulaError::Value);
    }
    let lookup_values = ev.range_array(&lookup_range).values;
    let result_array = ev.range_array(&result_range);
    if result_array.values.len() != lookup_values.len() {
        return Err(FormulaError::Value);
    }
    let match_mode = match args.get(4) {
        Some(arg) if !matches!(arg, Expr::Blank) => ev.eval_number(arg)?.trunc() as i32,
        _ => 0,
    };
    let search_mode = match args.get(5) {
        Some(arg) if !matches!(arg, Expr::Blank) => ev.eval_number(arg)?.trunc() as i32,
        _ => 1,
    };
    if !matches!(search_mode, 1 | -1 | 2 | -2) {
        return Err(FormulaError::Value);
    }
    let mut ordered: Vec<usize> = (0..lookup_values.len()).collect();
    if search_mode < 0 {
        ordered.reverse();
    }
    let found = match match_mode {
        0 => ordered
            .iter()
            .copied()
            .find(|index| values_match(&key, &lookup_values[*index])),
        2 => {
            let pattern = key.to_text()?.to_ascii_lowercase();
            ordered.iter().copied().find(|index| {
                lookup_values[*index]
                    .to_text()
                    .is_ok_and(|text| {
                        wildcard_text_matches(text.to_ascii_lowercase().as_bytes(), pattern.as_bytes())
                    })
            })
        }
        -1 | 1 => {
            let exact = ordered
                .iter()
                .copied()
                .find(|index| values_match(&key, &lookup_values[*index]));
            exact.or_else(|| {
                let mut best: Option<usize> = None;
                for index in &ordered {
                    let value = &lookup_values[*index];
                    let Ok(ordering) = compare_values(value, &key) else {
                        continue;
                    };
                    let candidate = if match_mode == -1 {
                        ordering.is_lt()
                    } else {
                        ordering.is_gt()
                    };
                    if !candidate {
                        continue;
                    }
                    best = match best {
                        None => Some(*index),
                        Some(current) => {
                            let better = compare_values(value, &lookup_values[current])
                                .map(|ordering| {
                                    if match_mode == -1 {
                                        ordering.is_gt()
                                    } else {
                                        ordering.is_lt()
                                    }
                                })
                                .unwrap_or(false);
                            Some(if better { *index } else { current })
                        }
                    };
                }
                best
            })
        }
        _ => return Err(FormulaError::Value),
    };
    match found {
        Some(index) => Ok(result_array.values[index].clone()),
        None => match args.get(3) {
            Some(missing) if !matches!(missing, Expr::Blank) => Ok(ev.eval(missing)),
            _ => Err(FormulaError::Na),
        },
    }
}

fn lookup_function(ev: &mut Evaluator<'_>, args: &[Expr]) -> FResult {
    arity(args, 2, 3)?;
    let key = lookup_key(ev, &args[0])?;
    let search_range = ev.resolve_range_expr(&args[1])?;
    let search_array = ev.range_array(&search_range);
    let (keys, results) = match args.get(2) {
        Some(arg) => {
            let result_range = ev.resolve_range_expr(arg)?;
            (search_array.values.clone(), ev.range_array(&result_range).values)
        }
        None => {
            if search_array.cols >= search_array.rows {
                (
                    search_array.row_values(0).to_vec(),
                    search_array.row_values(search_array.rows - 1).to_vec(),
                )
            } else {
                (
                    search_array.column_values(0),
                    search_array.column_values(search_array.cols - 1),
                )
            }
        }
    };
    let position = match_position(&key, &keys, 1).ok_or(FormulaError::Na)?;
    results.get(position).cloned().ok_or(FormulaError::Na)
}

fn rank_function(ev: &mut Evaluator<'_>, average_ties: bool, args: &[Expr]) -> FResult {
    arity(args, 2, 3)?;
    let target = ev.eval_number(&args[0])?;
    let range = ev.resolve_range_expr(&args[1])?;
    let ascending = match args.get(2) {
        Some(arg) if !matches!(arg, Expr::Blank) => ev.eval_number(arg)?.trunc() != 0.0,
        _ => false,
    };
    let mut rank = 1usize;
    let mut ties = 0usize;
    let mut found = false;
    for value in ev.range_array(&range).values {
        let FormulaValue::Number(value) = value else {
            continue;
        };
        if value == target {
            found = true;
            ties += 1;
        } else if (!ascending && value > target) || (ascending && value < target) {
            rank += 1;
        }
    }
    if !found {
        return Err(FormulaError::Na);
    }
    number(if average_ties {
        (rank + rank + ties - 1) as f64 / 2.0
    } else {
        rank as f64
    })
}

fn percentrank_function(ev: &mut Evaluator<'_>, exclusive: bool, args: &[Expr]) -> FResult {
    arity(args, 2, 3)?;
    let range = ev.resolve_range_expr(&args[0])?;
    let values: Vec<f64> = ev
        .range_array(&range)
        .values
        .into_iter()
        .filter_map(|value| match value {
            FormulaValue::Number(number) => Some(number),
            _ => None,
        })
        .collect();
    let target = ev.eval_number(&args[1])?;
    let significant_digits = match args.get(2) {
        Some(arg) if !matches!(arg, Expr::Blank) => {
            let digits = ev.eval_number(arg)?;
            if digits < 1.0 {
                return Err(FormulaError::Num);
            }
            Some(digits.trunc() as i32)
        }
        _ => None,
    };
    legacy(percentrank_value(values, target, significant_digits, exclusive))
}

fn paired_numbers(
    ev: &mut Evaluator<'_>,
    left: &Expr,
    right: &Expr,
) -> Result<(Vec<f64>, Vec<f64>), FormulaError> {
    let left_range = ev.resolve_range_expr(left)?;
    let right_range = ev.resolve_range_expr(right)?;
    let left_values = ev.range_array(&left_range).values;
    let right_values = ev.range_array(&right_range).values;
    if left_values.len() != right_values.len() {
        return Err(FormulaError::Value);
    }
    let mut lefts = Vec::new();
    let mut rights = Vec::new();
    for (left, right) in left_values.iter().zip(right_values.iter()) {
        if let (FormulaValue::Number(left), FormulaValue::Number(right)) = (left, right) {
            lefts.push(*left);
            rights.push(*right);
        }
    }
    Ok((lefts, rights))
}

/// Converts a resolved range to its A1 text (used by error messages and
/// projections).
pub fn range_to_a1(range: &ResolvedRange) -> String {
    let start = format!(
        "{}{}",
        number_to_column(range.col_start).unwrap_or_default(),
        range.row_start
    );
    if range.is_single() {
        start
    } else {
        format!(
            "{start}:{}{}",
            number_to_column(range.col_end).unwrap_or_default(),
            range.row_end
        )
    }
}

/// Text rendering of a value for lookups that need exact text (kept for
/// parity with legacy projections).
pub fn value_text(value: &FormulaValue) -> String {
    match value {
        FormulaValue::Number(number) => trim_number(*number),
        other => other.to_text().unwrap_or_default(),
    }
}

#[allow(dead_code)]
fn unused_ref_helpers(_reference: &RefExpr, _map: &BTreeMap<String, String>) {}
