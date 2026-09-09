//! Lookup, match, rank, and related spreadsheet functions.

use super::formula::{Evaluator, Expr};
use super::functions::{arity, legacy, number, FResult};
use super::value::{compare_values, values_match, FormulaError, FormulaValue};
use crate::functions_legacy_tail::percentrank_value;
use crate::wildcard_text_matches;

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

pub(super) fn match_function(ev: &mut Evaluator<'_>, args: &[Expr]) -> FResult {
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

pub(super) fn vh_lookup(ev: &mut Evaluator<'_>, name: &str, args: &[Expr]) -> FResult {
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

pub(super) fn xlookup(ev: &mut Evaluator<'_>, args: &[Expr]) -> FResult {
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
                lookup_values[*index].to_text().is_ok_and(|text| {
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

pub(super) fn lookup_function(ev: &mut Evaluator<'_>, args: &[Expr]) -> FResult {
    arity(args, 2, 3)?;
    let key = lookup_key(ev, &args[0])?;
    let search_range = ev.resolve_range_expr(&args[1])?;
    let search_array = ev.range_array(&search_range);
    let (keys, results) = match args.get(2) {
        Some(arg) => {
            let result_range = ev.resolve_range_expr(arg)?;
            (
                search_array.values.clone(),
                ev.range_array(&result_range).values,
            )
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

pub(super) fn rank_function(ev: &mut Evaluator<'_>, average_ties: bool, args: &[Expr]) -> FResult {
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

pub(super) fn percentrank_function(
    ev: &mut Evaluator<'_>,
    exclusive: bool,
    args: &[Expr],
) -> FResult {
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
    legacy(percentrank_value(
        values,
        target,
        significant_digits,
        exclusive,
    ))
}

pub(super) fn paired_numbers(
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
