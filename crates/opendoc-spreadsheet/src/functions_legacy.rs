use std::collections::BTreeSet;

use crate::functions_legacy_tail::*;

pub fn wildcard_search_position(
    value: &str,
    pattern: &str,
    byte_start: usize,
) -> Result<f64, String> {
    let lower_value = value.to_ascii_lowercase();
    let lower_pattern = pattern.to_ascii_lowercase();
    let value_positions = value
        .char_indices()
        .map(|(index, _)| index)
        .chain(std::iter::once(value.len()))
        .collect::<Vec<_>>();
    let Some(start_position) = value_positions
        .iter()
        .position(|index| *index == byte_start)
    else {
        return Err("#VALUE!".to_string());
    };
    for start in start_position..value_positions.len() {
        for end in start..value_positions.len() {
            let start_byte = value_positions[start];
            let end_byte = value_positions[end];
            let candidate = &lower_value[start_byte..end_byte];
            if wildcard_text_matches(candidate.as_bytes(), lower_pattern.as_bytes()) {
                return Ok((start + 1) as f64);
            }
        }
    }
    Err("#VALUE!".to_string())
}

pub fn proper_case_text(value: &str) -> String {
    let mut next_is_word_start = true;
    let mut out = String::new();
    for ch in value.chars() {
        if ch.is_alphabetic() {
            if next_is_word_start {
                out.extend(ch.to_uppercase());
            } else {
                out.extend(ch.to_lowercase());
            }
            next_is_word_start = false;
        } else {
            out.push(ch);
            next_is_word_start = !ch.is_numeric();
        }
    }
    out
}

pub fn clean_text(value: &str) -> String {
    value.chars().filter(|ch| !ch.is_ascii_control()).collect()
}

pub fn is_supported_email_address(input: &str) -> bool {
    let input = input.trim();
    if input.is_empty() || input.chars().any(char::is_whitespace) {
        return false;
    }
    let mut parts = input.split('@');
    let Some(local) = parts.next() else {
        return false;
    };
    let Some(domain) = parts.next() else {
        return false;
    };
    if parts.next().is_some()
        || local.is_empty()
        || domain.is_empty()
        || local.starts_with('.')
        || local.ends_with('.')
        || local.contains("..")
    {
        return false;
    }
    if !local
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '%' | '+' | '-'))
    {
        return false;
    }
    let labels = domain.split('.').collect::<Vec<_>>();
    if labels.len() < 2 {
        return false;
    }
    labels.iter().all(|label| {
        !label.is_empty()
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
    }) && labels
        .last()
        .is_some_and(|tld| tld.len() >= 2 && tld.chars().all(|ch| ch.is_ascii_alphabetic()))
}

pub fn is_supported_url(input: &str) -> bool {
    let input = input.trim();
    if input.is_empty() || input.chars().any(char::is_whitespace) {
        return false;
    }
    if let Some(address) = input.strip_prefix("mailto:") {
        return is_supported_email_address(address);
    }
    let (scheme, rest) = if let Some((scheme, rest)) = input.split_once("://") {
        (Some(scheme.to_ascii_lowercase()), rest)
    } else {
        (None, input)
    };
    if let Some(scheme) = scheme.as_deref() {
        if !matches!(
            scheme,
            "ftp" | "http" | "https" | "gopher" | "news" | "telnet" | "aim"
        ) {
            return false;
        }
    } else if input.contains(':') {
        return false;
    }
    let authority = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .trim_end_matches('.');
    if authority.is_empty() {
        return false;
    }
    let host = authority
        .rsplit_once('@')
        .map(|(_, host)| host)
        .unwrap_or(authority);
    let host = host
        .rsplit_once(':')
        .map(|(host, port)| {
            if port.chars().all(|ch| ch.is_ascii_digit()) && !port.is_empty() {
                host
            } else {
                ""
            }
        })
        .unwrap_or(host);
    let labels = host.split('.').collect::<Vec<_>>();
    if labels.len() < 2 {
        return false;
    }
    labels.iter().all(|label| {
        !label.is_empty()
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
    }) && labels
        .last()
        .is_some_and(|tld| tld.len() >= 2 && tld.chars().all(|ch| ch.is_ascii_alphabetic()))
}

pub fn formula_source_is_date_producing(input: &str) -> bool {
    let trimmed = input
        .trim()
        .strip_prefix('=')
        .unwrap_or(input.trim())
        .trim_start();
    let upper = trimmed.to_ascii_uppercase();
    ["DATE", "DATEVALUE", "EDATE", "EOMONTH", "WORKDAY"]
        .iter()
        .any(|function| {
            upper.starts_with(function) && trimmed[function.len()..].trim_start().starts_with('(')
        })
}

pub fn parse_formula_text_number(input: &str) -> Result<f64, String> {
    let value = input
        .trim()
        .replace(',', "")
        .parse::<f64>()
        .map_err(|_| "#VALUE!".to_string())?;
    if value.is_finite() {
        Ok(value)
    } else {
        Err("#NUM!".to_string())
    }
}

pub fn parse_formula_numbervalue(
    input: &str,
    decimal_separator: &str,
    group_separator: &str,
) -> Result<f64, String> {
    if decimal_separator.chars().count() != 1 || group_separator.chars().count() > 1 {
        return Err("#VALUE!".to_string());
    }
    if !group_separator.is_empty() && decimal_separator == group_separator {
        return Err("#VALUE!".to_string());
    }
    let decimal = decimal_separator
        .chars()
        .next()
        .ok_or_else(|| "#VALUE!".to_string())?;
    let group = group_separator.chars().next();
    let mut text = input.trim();
    let mut percent_count = 0u32;
    while let Some(rest) = text.strip_suffix('%') {
        percent_count += 1;
        text = rest.trim_end();
    }
    if text.contains('%') {
        return Err("#VALUE!".to_string());
    }
    let mut normalized = String::new();
    for ch in text.chars() {
        if Some(ch) == group {
            continue;
        }
        if ch == decimal {
            normalized.push('.');
        } else {
            normalized.push(ch);
        }
    }
    let mut value = normalized
        .parse::<f64>()
        .map_err(|_| "#VALUE!".to_string())?;
    if !value.is_finite() {
        return Err("#NUM!".to_string());
    }
    for _ in 0..percent_count {
        value /= 100.0;
    }
    if value.is_finite() {
        Ok(value)
    } else {
        Err("#NUM!".to_string())
    }
}

pub fn parse_formula_decimal(input: &str, radix: f64) -> Result<f64, String> {
    if !radix.is_finite() || radix < 2.0 || radix >= 37.0 {
        return Err("#NUM!".to_string());
    }
    let radix = radix.trunc() as u32;
    let input = input.trim();
    if input.is_empty() || input.len() > 255 {
        return Err("#VALUE!".to_string());
    }
    let mut out = 0u64;
    for ch in input.chars() {
        let Some(digit) = ch.to_digit(36) else {
            return Err("#VALUE!".to_string());
        };
        if digit >= radix {
            return Err("#VALUE!".to_string());
        }
        out = out
            .checked_mul(radix as u64)
            .and_then(|value| value.checked_add(digit as u64))
            .ok_or_else(|| "#NUM!".to_string())?;
    }
    Ok(out as f64)
}

pub fn parse_formula_datevalue(input: &str) -> Result<f64, String> {
    let mut parts = input.trim().split('-');
    let year = parts
        .next()
        .filter(|value| value.len() == 4 && value.chars().all(|ch| ch.is_ascii_digit()))
        .and_then(|value| value.parse::<i32>().ok())
        .ok_or_else(|| "#VALUE!".to_string())?;
    let month = parts
        .next()
        .filter(|value| value.len() == 2 && value.chars().all(|ch| ch.is_ascii_digit()))
        .and_then(|value| value.parse::<i32>().ok())
        .ok_or_else(|| "#VALUE!".to_string())?;
    let day = parts
        .next()
        .filter(|value| value.len() == 2 && value.chars().all(|ch| ch.is_ascii_digit()))
        .and_then(|value| value.parse::<i32>().ok())
        .ok_or_else(|| "#VALUE!".to_string())?;
    if parts.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return Err("#VALUE!".to_string());
    }
    let serial = date_value(vec![year as f64, month as f64, day as f64])?;
    let (roundtrip_year, roundtrip_month, roundtrip_day) =
        civil_from_days(serial.trunc() as i64 + days_from_civil(1899, 12, 30));
    if roundtrip_year != year || roundtrip_month != month as u32 || roundtrip_day != day as u32 {
        return Err("#VALUE!".to_string());
    }
    Ok(serial)
}

pub fn parse_formula_timevalue(input: &str) -> Result<f64, String> {
    let mut parts = input.trim().split(':');
    let hour = parts
        .next()
        .filter(|value| value.len() == 2 && value.chars().all(|ch| ch.is_ascii_digit()))
        .and_then(|value| value.parse::<f64>().ok())
        .ok_or_else(|| "#VALUE!".to_string())?;
    let minute = parts
        .next()
        .filter(|value| value.len() == 2 && value.chars().all(|ch| ch.is_ascii_digit()))
        .and_then(|value| value.parse::<f64>().ok())
        .ok_or_else(|| "#VALUE!".to_string())?;
    let second = parts
        .next()
        .map(|value| {
            if value.len() == 2 && value.chars().all(|ch| ch.is_ascii_digit()) {
                value.parse::<f64>().map_err(|_| "#VALUE!".to_string())
            } else {
                Err("#VALUE!".to_string())
            }
        })
        .transpose()?
        .unwrap_or(0.0);
    if parts.next().is_some()
        || !(0.0..=23.0).contains(&hour)
        || !(0.0..=59.0).contains(&minute)
        || !(0.0..=59.0).contains(&second)
    {
        return Err("#VALUE!".to_string());
    }
    time_value(vec![hour, minute, second])
}

pub fn encode_formula_base_digits(mut value: u64, radix: u32) -> Result<String, String> {
    if !(2..=36).contains(&radix) {
        return Err("#NUM!".to_string());
    }
    if value == 0 {
        return Ok("0".to_string());
    }
    let mut digits = Vec::new();
    while value > 0 {
        let digit = (value % radix as u64) as u8;
        let ch = if digit < 10 {
            (b'0' + digit) as char
        } else {
            (b'A' + digit - 10) as char
        };
        digits.push(ch);
        value /= radix as u64;
    }
    digits.reverse();
    Ok(digits.into_iter().collect())
}

pub fn wildcard_text_matches(value: &[u8], pattern: &[u8]) -> bool {
    let mut value_index = 0usize;
    let mut pattern_index = 0usize;
    let mut star_pattern = None;
    let mut star_value = 0usize;
    while value_index < value.len() {
        if pattern_index < pattern.len() && pattern[pattern_index] == b'~' {
            pattern_index += 1;
            if pattern_index < pattern.len() && pattern[pattern_index] == value[value_index] {
                pattern_index += 1;
                value_index += 1;
                continue;
            }
        } else if pattern_index < pattern.len()
            && (pattern[pattern_index] == b'?' || pattern[pattern_index] == value[value_index])
        {
            pattern_index += 1;
            value_index += 1;
            continue;
        } else if pattern_index < pattern.len() && pattern[pattern_index] == b'*' {
            star_pattern = Some(pattern_index);
            pattern_index += 1;
            star_value = value_index;
            continue;
        }
        let Some(star) = star_pattern else {
            return false;
        };
        pattern_index = star + 1;
        star_value += 1;
        value_index = star_value;
    }
    while pattern_index < pattern.len() && pattern[pattern_index] == b'*' {
        pattern_index += 1;
    }
    pattern_index == pattern.len()
}

pub fn evaluate_function(name: &str, values: Vec<f64>) -> Result<f64, String> {
    match name {
        "DATE" => date_value(values),
        "YEAR" => date_part_value(values, DatePart::Year),
        "MONTH" => date_part_value(values, DatePart::Month),
        "DAY" => date_part_value(values, DatePart::Day),
        "TIME" => time_value(values),
        "HOUR" => time_part_value(values, TimePart::Hour),
        "MINUTE" => time_part_value(values, TimePart::Minute),
        "SECOND" => time_part_value(values, TimePart::Second),
        "DAYS" => days_value(values),
        "DAYS360" => days360_value(values),
        "NETWORKDAYS" => networkdays_value(values),
        "WORKDAY" => workday_value(values),
        "ISOWEEKNUM" => iso_weeknum_value(values),
        "WEEKNUM" => weeknum_value(values),
        "WEEKDAY" => weekday_value(values),
        "EDATE" => edate_value(values, false),
        "EOMONTH" => edate_value(values, true),
        "SUM" => Ok(values.into_iter().sum()),
        "AVERAGE" => {
            if values.is_empty() {
                Err("#DIV/0!".to_string())
            } else {
                Ok(values.iter().sum::<f64>() / values.len() as f64)
            }
        }
        "MIN" => values
            .into_iter()
            .reduce(f64::min)
            .ok_or_else(|| "#N/A".to_string()),
        "MAX" => values
            .into_iter()
            .reduce(f64::max)
            .ok_or_else(|| "#N/A".to_string()),
        "COUNT" => Ok(values.len() as f64),
        "PRODUCT" => {
            if values.is_empty() {
                Err("#VALUE!".to_string())
            } else {
                Ok(values.into_iter().product())
            }
        }
        "MEDIAN" => {
            if values.is_empty() {
                return Err("#VALUE!".to_string());
            }
            if values.iter().any(|value| !value.is_finite()) {
                return Err("#NUM!".to_string());
            }
            let mut values = values;
            values.sort_by(f64::total_cmp);
            let mid = values.len() / 2;
            if values.len() % 2 == 1 {
                Ok(values[mid])
            } else {
                let out = (values[mid - 1] + values[mid]) / 2.0;
                if out.is_finite() {
                    Ok(out)
                } else {
                    Err("#NUM!".to_string())
                }
            }
        }
        "MODE" => mode_value(values),
        "SUMSQ" => {
            if values.is_empty() {
                return Err("#VALUE!".to_string());
            }
            let out = values.into_iter().try_fold(0.0, |sum, value| {
                let squared = value * value;
                let next = sum + squared;
                if squared.is_finite() && next.is_finite() {
                    Some(next)
                } else {
                    None
                }
            });
            out.ok_or_else(|| "#NUM!".to_string())
        }
        "STDEV" | "STDEV.S" => standard_deviation(values, true),
        "STDEVP" | "STDEV.P" => standard_deviation(values, false),
        "VAR" | "VAR.S" => variance(values, true),
        "VARP" | "VAR.P" => variance(values, false),
        "AVEDEV" => average_deviation(values),
        "DEVSQ" => deviation_sum_squares(values),
        "GEOMEAN" => geometric_mean(values),
        "HARMEAN" => harmonic_mean(values),
        "LARGE" => ranked_value(values, true),
        "SMALL" => ranked_value(values, false),
        "PERCENTILE" | "PERCENTILE.INC" => percentile_value(values),
        "PERCENTILE.EXC" => percentile_exc_value(values),
        "QUARTILE" | "QUARTILE.INC" => quartile_value(values),
        "QUARTILE.EXC" => quartile_exc_value(values),
        "COMBIN" => combin_value(values),
        "COMBINA" => combina_value(values),
        "PERMUT" => permut_value(values),
        "PERMUTATIONA" => permutationa_value(values),
        "FACT" => factorial_value(values),
        "FACTDOUBLE" => double_factorial_value(values),
        "GCD" => gcd_value(values),
        "LCM" => lcm_value(values),
        "TRUE" => {
            if values.is_empty() {
                Ok(1.0)
            } else {
                Err("#VALUE!".to_string())
            }
        }
        "FALSE" => {
            if values.is_empty() {
                Ok(0.0)
            } else {
                Err("#VALUE!".to_string())
            }
        }
        "NA" => {
            if values.is_empty() {
                Err("#N/A".to_string())
            } else {
                Err("#VALUE!".to_string())
            }
        }
        "IF" => if_value(values),
        "ISEVEN" => parity_predicate(values, 0),
        "ISODD" => parity_predicate(values, 1),
        "AND" => {
            if values.is_empty() {
                return Err("#VALUE!".to_string());
            }
            Ok(if values.iter().all(|value| *value != 0.0) {
                1.0
            } else {
                0.0
            })
        }

        "OR" => {
            if values.is_empty() {
                return Err("#VALUE!".to_string());
            }
            Ok(if values.iter().any(|value| *value != 0.0) {
                1.0
            } else {
                0.0
            })
        }
        "XOR" => {
            if values.is_empty() {
                return Err("#VALUE!".to_string());
            }
            Ok(
                if values.iter().filter(|value| **value != 0.0).count() % 2 == 1 {
                    1.0
                } else {
                    0.0
                },
            )
        }
        "NOT" => {
            let mut values = values.into_iter();
            let Some(value) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            if values.next().is_some() {
                return Err("#VALUE!".to_string());
            }
            Ok(if value == 0.0 { 1.0 } else { 0.0 })
        }
        "EQ" => compare_function(values, |left, right| left == right),
        "NE" => compare_function(values, |left, right| left != right),
        "GT" => compare_function(values, |left, right| left > right),
        "GTE" => compare_function(values, |left, right| left >= right),
        "LT" => compare_function(values, |left, right| left < right),
        "LTE" => compare_function(values, |left, right| left <= right),
        "ISBETWEEN" => isbetween_value(values),
        "ADD" => binary_operator_function(values, |left, right| left + right),
        "MINUS" => binary_operator_function(values, |left, right| left - right),
        "MULTIPLY" => binary_operator_function(values, |left, right| left * right),
        "DIVIDE" => {
            let mut values = values.into_iter();
            let Some(left) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            let Some(right) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            if values.next().is_some() {
                return Err("#VALUE!".to_string());
            }
            if right == 0.0 {
                return Err("#DIV/0!".to_string());
            }
            let out = left / right;
            if out.is_finite() {
                Ok(out)
            } else {
                Err("#NUM!".to_string())
            }
        }
        "POW" => binary_operator_function(values, |left, right| left.powf(right)),
        "UMINUS" => {
            let mut values = values.into_iter();
            let Some(value) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            if values.next().is_some() {
                return Err("#VALUE!".to_string());
            }
            Ok(-value)
        }
        "UNARY_PERCENT" => {
            let mut values = values.into_iter();
            let Some(value) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            if values.next().is_some() {
                return Err("#VALUE!".to_string());
            }
            Ok(value / 100.0)
        }
        "STANDARDIZE" => {
            let mut values = values.into_iter();
            let Some(value) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            let Some(mean) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            let Some(standard_deviation) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            if values.next().is_some() {
                return Err("#VALUE!".to_string());
            }
            if standard_deviation == 0.0 {
                return Err("#DIV/0!".to_string());
            }
            if standard_deviation < 0.0 {
                return Err("#NUM!".to_string());
            }
            let out = (value - mean) / standard_deviation;
            if out.is_finite() {
                Ok(out)
            } else {
                Err("#NUM!".to_string())
            }
        }
        "FISHER" => {
            let mut values = values.into_iter();
            let Some(value) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            if values.next().is_some() {
                return Err("#VALUE!".to_string());
            }
            if value <= -1.0 || value >= 1.0 {
                return Err("#NUM!".to_string());
            }
            let out = 0.5 * ((1.0 + value) / (1.0 - value)).ln();
            if out.is_finite() {
                Ok(out)
            } else {
                Err("#NUM!".to_string())
            }
        }
        "FISHERINV" => {
            let mut values = values.into_iter();
            let Some(value) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            if values.next().is_some() {
                return Err("#VALUE!".to_string());
            }
            let out = value.tanh();
            if out.is_finite() {
                Ok(out)
            } else {
                Err("#NUM!".to_string())
            }
        }
        "DELTA" => {
            let mut values = values.into_iter();
            let Some(first) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            let second = values.next().unwrap_or(0.0);
            if values.next().is_some() {
                return Err("#VALUE!".to_string());
            }
            Ok(if first == second { 1.0 } else { 0.0 })
        }
        "GESTEP" => {
            let mut values = values.into_iter();
            let Some(value) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            let step = values.next().unwrap_or(0.0);
            if values.next().is_some() {
                return Err("#VALUE!".to_string());
            }
            Ok(if value >= step { 1.0 } else { 0.0 })
        }
        "ERF" | "ERF.PRECISE" => erf_value(values),
        "ERFC" | "ERFC.PRECISE" => {
            let mut values = values.into_iter();
            let Some(value) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            if values.next().is_some() {
                return Err("#VALUE!".to_string());
            }
            if !value.is_finite() {
                return Err("#NUM!".to_string());
            }
            Ok(1.0 - erf_approx(value))
        }
        "ROUND" => {
            let mut values = values.into_iter();
            let Some(value) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            let Some(places) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            if values.next().is_some() {
                return Err("#VALUE!".to_string());
            }
            let factor = 10f64.powi(places as i32);
            Ok((value * factor).round() / factor)
        }
        "TRUNC" => {
            let mut values = values.into_iter();
            let Some(value) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            let places = values.next().unwrap_or(0.0) as i32;
            if values.next().is_some() {
                return Err("#VALUE!".to_string());
            }
            let factor = 10f64.powi(places);
            if !factor.is_finite() || factor == 0.0 {
                return Err("#NUM!".to_string());
            }
            let out = (value * factor).trunc() / factor;
            if out.is_finite() {
                Ok(out)
            } else {
                Err("#NUM!".to_string())
            }
        }
        "ROUNDUP" => {
            let mut values = values.into_iter();
            let Some(value) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            let places = values.next().unwrap_or(0.0) as i32;
            if values.next().is_some() {
                return Err("#VALUE!".to_string());
            }
            let factor = 10f64.powi(places);
            if !factor.is_finite() || factor == 0.0 {
                return Err("#NUM!".to_string());
            }
            let scaled = value * factor;
            let out = if scaled > 0.0 {
                scaled.ceil() / factor
            } else {
                scaled.floor() / factor
            };
            if out.is_finite() {
                Ok(out)
            } else {
                Err("#NUM!".to_string())
            }
        }
        "ROUNDDOWN" => {
            let mut values = values.into_iter();
            let Some(value) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            let places = values.next().unwrap_or(0.0) as i32;
            if values.next().is_some() {
                return Err("#VALUE!".to_string());
            }
            let factor = 10f64.powi(places);
            if !factor.is_finite() || factor == 0.0 {
                return Err("#NUM!".to_string());
            }
            let out = (value * factor).trunc() / factor;
            if out.is_finite() {
                Ok(out)
            } else {
                Err("#NUM!".to_string())
            }
        }
        "FLOOR.MATH" => floor_math_value(values),
        "FLOOR.PRECISE" => floor_precise_value(values),
        "CEILING.MATH" => ceiling_math_value(values),
        "CEILING.PRECISE" | "ISO.CEILING" => ceiling_precise_value(values),
        "FLOOR" => {
            let mut values = values.into_iter();
            let Some(value) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            let factor = values.next().unwrap_or(1.0);
            if values.next().is_some() {
                return Err("#VALUE!".to_string());
            }
            if factor == 0.0 {
                return Err("#DIV/0!".to_string());
            }
            if value != 0.0 && value.signum() != factor.signum() {
                return Err("#NUM!".to_string());
            }
            let out = (value / factor).floor() * factor;
            if out.is_finite() {
                Ok(out)
            } else {
                Err("#NUM!".to_string())
            }
        }
        "CEILING" => {
            let mut values = values.into_iter();
            let Some(value) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            let factor = values.next().unwrap_or(1.0);
            if values.next().is_some() {
                return Err("#VALUE!".to_string());
            }
            if factor == 0.0 {
                return Err("#DIV/0!".to_string());
            }
            if value > 0.0 && factor < 0.0 {
                return Err("#NUM!".to_string());
            }
            let out = (value / factor).ceil() * factor;
            if out.is_finite() {
                Ok(out)
            } else {
                Err("#NUM!".to_string())
            }
        }
        "MROUND" => {
            let mut values = values.into_iter();
            let Some(value) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            let Some(factor) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            if values.next().is_some() {
                return Err("#VALUE!".to_string());
            }
            if value == 0.0 || factor == 0.0 {
                return Ok(0.0);
            }
            if value.signum() != factor.signum() {
                return Err("#NUM!".to_string());
            }
            let quotient = value / factor;
            let rounded = quotient.signum() * (quotient.abs() + 0.5).floor();
            let out = rounded * factor;
            if out.is_finite() {
                Ok(out)
            } else {
                Err("#NUM!".to_string())
            }
        }
        "POWER" => {
            let mut values = values.into_iter();
            let Some(base) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            let Some(exponent) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            if values.next().is_some() {
                return Err("#VALUE!".to_string());
            }
            let value = base.powf(exponent);
            if value.is_finite() {
                Ok(value)
            } else {
                Err("#NUM!".to_string())
            }
        }
        "MOD" => {
            let mut values = values.into_iter();
            let Some(dividend) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            let Some(divisor) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            if values.next().is_some() {
                return Err("#VALUE!".to_string());
            }
            if divisor == 0.0 {
                return Err("#DIV/0!".to_string());
            }
            Ok(dividend - divisor * (dividend / divisor).floor())
        }
        "QUOTIENT" => {
            let mut values = values.into_iter();
            let Some(dividend) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            let Some(divisor) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            if values.next().is_some() {
                return Err("#VALUE!".to_string());
            }
            if divisor == 0.0 {
                return Err("#DIV/0!".to_string());
            }
            let out = (dividend / divisor).trunc();
            if out.is_finite() {
                Ok(out)
            } else {
                Err("#NUM!".to_string())
            }
        }
        "EVEN" => {
            let mut values = values.into_iter();
            let Some(value) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            if values.next().is_some() {
                return Err("#VALUE!".to_string());
            }
            let out = round_away_to_parity(value, 0);
            if out.is_finite() {
                Ok(out)
            } else {
                Err("#NUM!".to_string())
            }
        }
        "ODD" => {
            let mut values = values.into_iter();
            let Some(value) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            if values.next().is_some() {
                return Err("#VALUE!".to_string());
            }
            let out = round_away_to_parity(value, 1);
            if out.is_finite() {
                Ok(out)
            } else {
                Err("#NUM!".to_string())
            }
        }
        "INT" => {
            let mut values = values.into_iter();
            let Some(value) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            if values.next().is_some() {
                return Err("#VALUE!".to_string());
            }
            Ok(value.floor())
        }
        "SIGN" => {
            let mut values = values.into_iter();
            let Some(value) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            if values.next().is_some() {
                return Err("#VALUE!".to_string());
            }
            Ok(if value > 0.0 {
                1.0
            } else if value < 0.0 {
                -1.0
            } else {
                0.0
            })
        }
        "LN" => {
            let mut values = values.into_iter();
            let Some(value) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            if values.next().is_some() {
                return Err("#VALUE!".to_string());
            }
            if value <= 0.0 {
                return Err("#NUM!".to_string());
            }
            let out = value.ln();
            if out.is_finite() {
                Ok(out)
            } else {
                Err("#NUM!".to_string())
            }
        }
        "LOG" => {
            let mut values = values.into_iter();
            let Some(value) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            let base = values.next().unwrap_or(10.0);
            if values.next().is_some() {
                return Err("#VALUE!".to_string());
            }
            if value <= 0.0 || base <= 0.0 || base == 1.0 {
                return Err("#NUM!".to_string());
            }
            let out = value.log(base);
            if out.is_finite() {
                Ok(out)
            } else {
                Err("#NUM!".to_string())
            }
        }
        "LOG10" => {
            let mut values = values.into_iter();
            let Some(value) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            if values.next().is_some() {
                return Err("#VALUE!".to_string());
            }
            if value <= 0.0 {
                return Err("#NUM!".to_string());
            }
            let out = value.log10();
            if out.is_finite() {
                Ok(out)
            } else {
                Err("#NUM!".to_string())
            }
        }
        "EXP" => {
            let mut values = values.into_iter();
            let Some(value) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            if values.next().is_some() {
                return Err("#VALUE!".to_string());
            }
            let out = value.exp();
            if out.is_finite() {
                Ok(out)
            } else {
                Err("#NUM!".to_string())
            }
        }
        "SIN" => {
            let mut values = values.into_iter();
            let Some(value) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            if values.next().is_some() {
                return Err("#VALUE!".to_string());
            }
            let out = value.sin();
            if out.is_finite() {
                Ok(out)
            } else {
                Err("#NUM!".to_string())
            }
        }
        "COS" => {
            let mut values = values.into_iter();
            let Some(value) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            if values.next().is_some() {
                return Err("#VALUE!".to_string());
            }
            let out = value.cos();
            if out.is_finite() {
                Ok(out)
            } else {
                Err("#NUM!".to_string())
            }
        }
        "TAN" => {
            let mut values = values.into_iter();
            let Some(value) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            if values.next().is_some() {
                return Err("#VALUE!".to_string());
            }
            let out = value.tan();
            if out.is_finite() {
                Ok(out)
            } else {
                Err("#NUM!".to_string())
            }
        }
        "SEC" => {
            let mut values = values.into_iter();
            let Some(value) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            if values.next().is_some() {
                return Err("#VALUE!".to_string());
            }
            let divisor = value.cos();
            if divisor == 0.0 {
                return Err("#DIV/0!".to_string());
            }
            let out = 1.0 / divisor;
            if out.is_finite() {
                Ok(out)
            } else {
                Err("#NUM!".to_string())
            }
        }
        "CSC" => {
            let mut values = values.into_iter();
            let Some(value) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            if values.next().is_some() {
                return Err("#VALUE!".to_string());
            }
            let divisor = value.sin();
            if divisor == 0.0 {
                return Err("#DIV/0!".to_string());
            }
            let out = 1.0 / divisor;
            if out.is_finite() {
                Ok(out)
            } else {
                Err("#NUM!".to_string())
            }
        }
        "COT" => {
            let mut values = values.into_iter();
            let Some(value) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            if values.next().is_some() {
                return Err("#VALUE!".to_string());
            }
            let divisor = value.tan();
            if divisor == 0.0 {
                return Err("#DIV/0!".to_string());
            }
            let out = 1.0 / divisor;
            if out.is_finite() {
                Ok(out)
            } else {
                Err("#NUM!".to_string())
            }
        }
        "SINH" => {
            let mut values = values.into_iter();
            let Some(value) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            if values.next().is_some() {
                return Err("#VALUE!".to_string());
            }
            let out = value.sinh();
            if out.is_finite() {
                Ok(out)
            } else {
                Err("#NUM!".to_string())
            }
        }
        "COSH" => {
            let mut values = values.into_iter();
            let Some(value) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            if values.next().is_some() {
                return Err("#VALUE!".to_string());
            }
            let out = value.cosh();
            if out.is_finite() {
                Ok(out)
            } else {
                Err("#NUM!".to_string())
            }
        }
        "TANH" => {
            let mut values = values.into_iter();
            let Some(value) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            if values.next().is_some() {
                return Err("#VALUE!".to_string());
            }
            let out = value.tanh();
            if out.is_finite() {
                Ok(out)
            } else {
                Err("#NUM!".to_string())
            }
        }
        "SECH" => {
            let mut values = values.into_iter();
            let Some(value) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            if values.next().is_some() {
                return Err("#VALUE!".to_string());
            }
            let divisor = value.cosh();
            if divisor == 0.0 {
                return Err("#DIV/0!".to_string());
            }
            let out = 1.0 / divisor;
            if out.is_finite() {
                Ok(out)
            } else {
                Err("#NUM!".to_string())
            }
        }
        "CSCH" => {
            let mut values = values.into_iter();
            let Some(value) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            if values.next().is_some() {
                return Err("#VALUE!".to_string());
            }
            let divisor = value.sinh();
            if divisor == 0.0 {
                return Err("#DIV/0!".to_string());
            }
            let out = 1.0 / divisor;
            if out.is_finite() {
                Ok(out)
            } else {
                Err("#NUM!".to_string())
            }
        }
        "COTH" => {
            let mut values = values.into_iter();
            let Some(value) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            if values.next().is_some() {
                return Err("#VALUE!".to_string());
            }
            let divisor = value.tanh();
            if divisor == 0.0 {
                return Err("#DIV/0!".to_string());
            }
            let out = 1.0 / divisor;
            if out.is_finite() {
                Ok(out)
            } else {
                Err("#NUM!".to_string())
            }
        }
        "ASINH" => {
            let mut values = values.into_iter();
            let Some(value) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            if values.next().is_some() {
                return Err("#VALUE!".to_string());
            }
            let out = value.asinh();
            if out.is_finite() {
                Ok(out)
            } else {
                Err("#NUM!".to_string())
            }
        }
        "ACOSH" => {
            let mut values = values.into_iter();
            let Some(value) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            if values.next().is_some() {
                return Err("#VALUE!".to_string());
            }
            if value < 1.0 {
                return Err("#NUM!".to_string());
            }
            let out = value.acosh();
            if out.is_finite() {
                Ok(out)
            } else {
                Err("#NUM!".to_string())
            }
        }
        "ATANH" => {
            let mut values = values.into_iter();
            let Some(value) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            if values.next().is_some() {
                return Err("#VALUE!".to_string());
            }
            if value <= -1.0 || value >= 1.0 {
                return Err("#NUM!".to_string());
            }
            let out = value.atanh();
            if out.is_finite() {
                Ok(out)
            } else {
                Err("#NUM!".to_string())
            }
        }
        "ASIN" => {
            let mut values = values.into_iter();
            let Some(value) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            if values.next().is_some() {
                return Err("#VALUE!".to_string());
            }
            if !(-1.0..=1.0).contains(&value) {
                return Err("#NUM!".to_string());
            }
            let out = value.asin();
            if out.is_finite() {
                Ok(out)
            } else {
                Err("#NUM!".to_string())
            }
        }
        "ACOS" => {
            let mut values = values.into_iter();
            let Some(value) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            if values.next().is_some() {
                return Err("#VALUE!".to_string());
            }
            if !(-1.0..=1.0).contains(&value) {
                return Err("#NUM!".to_string());
            }
            let out = value.acos();
            if out.is_finite() {
                Ok(out)
            } else {
                Err("#NUM!".to_string())
            }
        }
        "ATAN" => {
            let mut values = values.into_iter();
            let Some(value) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            if values.next().is_some() {
                return Err("#VALUE!".to_string());
            }
            let out = value.atan();
            if out.is_finite() {
                Ok(out)
            } else {
                Err("#NUM!".to_string())
            }
        }
        "ACOT" => {
            let mut values = values.into_iter();
            let Some(value) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            if values.next().is_some() {
                return Err("#VALUE!".to_string());
            }
            let out = if value == 0.0 {
                std::f64::consts::FRAC_PI_2
            } else {
                (1.0 / value).atan()
            };
            if out.is_finite() {
                Ok(out)
            } else {
                Err("#NUM!".to_string())
            }
        }
        "ACOTH" => {
            let mut values = values.into_iter();
            let Some(value) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            if values.next().is_some() {
                return Err("#VALUE!".to_string());
            }
            if value.abs() <= 1.0 {
                return Err("#NUM!".to_string());
            }
            let out = 0.5 * ((value + 1.0) / (value - 1.0)).ln();
            if out.is_finite() {
                Ok(out)
            } else {
                Err("#NUM!".to_string())
            }
        }
        "ATAN2" => {
            let mut values = values.into_iter();
            let Some(x) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            let Some(y) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            if values.next().is_some() {
                return Err("#VALUE!".to_string());
            }
            let out = y.atan2(x);
            if out.is_finite() {
                Ok(out)
            } else {
                Err("#NUM!".to_string())
            }
        }
        "RADIANS" => {
            let mut values = values.into_iter();
            let Some(value) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            if values.next().is_some() {
                return Err("#VALUE!".to_string());
            }
            let out = value.to_radians();
            if out.is_finite() {
                Ok(out)
            } else {
                Err("#NUM!".to_string())
            }
        }
        "DEGREES" => {
            let mut values = values.into_iter();
            let Some(value) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            if values.next().is_some() {
                return Err("#VALUE!".to_string());
            }
            let out = value.to_degrees();
            if out.is_finite() {
                Ok(out)
            } else {
                Err("#NUM!".to_string())
            }
        }
        "PI" => {
            if values.is_empty() {
                Ok(std::f64::consts::PI)
            } else {
                Err("#VALUE!".to_string())
            }
        }
        "E" => {
            if values.is_empty() {
                Ok(std::f64::consts::E)
            } else {
                Err("#VALUE!".to_string())
            }
        }
        "ABS" => {
            let mut values = values.into_iter();
            let Some(value) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            if values.next().is_some() {
                return Err("#VALUE!".to_string());
            }
            Ok(value.abs())
        }
        "SQRT" => {
            let mut values = values.into_iter();
            let Some(value) = values.next() else {
                return Err("#VALUE!".to_string());
            };
            if values.next().is_some() {
                return Err("#VALUE!".to_string());
            }
            if value < 0.0 {
                return Err("#NUM!".to_string());
            }
            Ok(value.sqrt())
        }
        _ => Err("#NAME?".to_string()),
    }
}

enum DatePart {
    Year,
    Month,
    Day,
}

enum TimePart {
    Hour,
    Minute,
    Second,
}

pub fn date_value(values: Vec<f64>) -> Result<f64, String> {
    if values.len() != 3 {
        return Err("#VALUE!".to_string());
    }
    if values.iter().any(|value| !value.is_finite()) {
        return Err("#VALUE!".to_string());
    }
    let mut year = values[0].trunc() as i32;
    if (0..=1899).contains(&year) {
        year += 1900;
    }
    let month = values[1].trunc() as i32;
    let day = values[2].trunc() as i32;
    let month_zero = month - 1;
    year += month_zero.div_euclid(12);
    let month = month_zero.rem_euclid(12) + 1;
    let serial = days_from_civil(year, month as u32, 1)
        .checked_sub(days_from_civil(1899, 12, 30))
        .and_then(|value| value.checked_add((day - 1) as i64))
        .ok_or_else(|| "#NUM!".to_string())?;
    Ok(serial as f64)
}

fn date_part_value(values: Vec<f64>, part: DatePart) -> Result<f64, String> {
    if values.len() != 1 {
        return Err("#VALUE!".to_string());
    }
    let serial = values[0];
    if !serial.is_finite() {
        return Err("#VALUE!".to_string());
    }
    let days = serial.trunc() as i64 + days_from_civil(1899, 12, 30);
    let (year, month, day) = civil_from_days(days);
    Ok(match part {
        DatePart::Year => year as f64,
        DatePart::Month => month as f64,
        DatePart::Day => day as f64,
    })
}

pub fn edate_value(values: Vec<f64>, end_of_month: bool) -> Result<f64, String> {
    if values.len() != 2 {
        return Err("#VALUE!".to_string());
    }
    if values.iter().any(|value| !value.is_finite()) {
        return Err("#VALUE!".to_string());
    }
    let start_serial = values[0].trunc() as i64;
    let month_offset = values[1].trunc() as i32;
    let (year, month, day) = civil_from_days(start_serial + days_from_civil(1899, 12, 30));
    let month_zero = year
        .checked_mul(12)
        .and_then(|value| value.checked_add(month as i32 - 1))
        .and_then(|value| value.checked_add(month_offset))
        .ok_or_else(|| "#NUM!".to_string())?;
    let target_year = month_zero.div_euclid(12);
    let target_month = month_zero.rem_euclid(12) as u32 + 1;
    let target_day = if end_of_month {
        days_in_month(target_year, target_month)
    } else {
        day.min(days_in_month(target_year, target_month))
    };
    let serial = days_from_civil(target_year, target_month, target_day)
        .checked_sub(days_from_civil(1899, 12, 30))
        .ok_or_else(|| "#NUM!".to_string())?;
    Ok(serial as f64)
}

pub fn datedif_value(start: f64, end: f64, unit: &str) -> Result<f64, String> {
    if !start.is_finite() || !end.is_finite() {
        return Err("#VALUE!".to_string());
    }
    let start = start.trunc() as i64;
    let end = end.trunc() as i64;
    if end < start {
        return Err("#NUM!".to_string());
    }
    let (start_year, start_month, start_day) =
        civil_from_days(start + days_from_civil(1899, 12, 30));
    let (end_year, end_month, end_day) = civil_from_days(end + days_from_civil(1899, 12, 30));
    match unit.to_ascii_uppercase().as_str() {
        "D" => Ok((end - start) as f64),
        "M" => Ok(full_months_between(
            start_year,
            start_month,
            start_day,
            end_year,
            end_month,
            end_day,
        ) as f64),
        "Y" => {
            let mut years = end_year - start_year;
            if (end_month, end_day) < (start_month, start_day) {
                years -= 1;
            }
            Ok(years as f64)
        }
        "YM" => Ok((full_months_between(
            start_year,
            start_month,
            start_day,
            end_year,
            end_month,
            end_day,
        ) % 12) as f64),
        "YD" => {
            let mut anniversary = days_from_civil(end_year, start_month, start_day);
            if anniversary > end + days_from_civil(1899, 12, 30) {
                anniversary = days_from_civil(end_year - 1, start_month, start_day);
            }
            Ok((end + days_from_civil(1899, 12, 30) - anniversary) as f64)
        }
        "MD" => {
            let mut days = end_day as i32 - start_day as i32;
            if days < 0 {
                let previous_month = if end_month == 1 { 12 } else { end_month - 1 };
                let previous_year = if end_month == 1 {
                    end_year - 1
                } else {
                    end_year
                };
                days += days_in_month(previous_year, previous_month) as i32;
            }
            Ok(days as f64)
        }
        _ => Err("#VALUE!".to_string()),
    }
}

pub fn networkdays_value(values: Vec<f64>) -> Result<f64, String> {
    if values.len() != 2 {
        return Err("#VALUE!".to_string());
    }
    if values.iter().any(|value| !value.is_finite()) {
        return Err("#VALUE!".to_string());
    }
    networkdays_value_with_holidays(values[0], values[1], &BTreeSet::new())
}

pub fn networkdays_value_with_holidays(
    start: f64,
    end: f64,
    holidays: &BTreeSet<i64>,
) -> Result<f64, String> {
    let start = start.trunc() as i64;
    let end = end.trunc() as i64;
    let step = if start <= end { 1 } else { -1 };
    let mut current = start;
    let mut count = 0i64;
    loop {
        if is_serial_weekday(current) && !holidays.contains(&current) {
            count += step;
        }
        if current == end {
            break;
        }
        current += step;
    }
    Ok(count as f64)
}

pub fn workday_value(values: Vec<f64>) -> Result<f64, String> {
    if values.len() != 2 {
        return Err("#VALUE!".to_string());
    }
    if values.iter().any(|value| !value.is_finite()) {
        return Err("#VALUE!".to_string());
    }
    workday_value_with_holidays(values[0], values[1], &BTreeSet::new())
}

pub fn workday_value_with_holidays(
    start: f64,
    days: f64,
    holidays: &BTreeSet<i64>,
) -> Result<f64, String> {
    let mut current = start.trunc() as i64;
    let remaining = days.trunc() as i64;
    if remaining == 0 {
        return Ok(current as f64);
    }
    let step = if remaining > 0 { 1 } else { -1 };
    let mut seen = 0i64;
    while seen.abs() < remaining.abs() {
        current += step;
        if is_serial_weekday(current) && !holidays.contains(&current) {
            seen += step;
        }
    }
    Ok(current as f64)
}

pub fn iso_weeknum_value(values: Vec<f64>) -> Result<f64, String> {
    if values.len() != 1 {
        return Err("#VALUE!".to_string());
    }
    let serial = values[0];
    if !serial.is_finite() {
        return Err("#VALUE!".to_string());
    }
    let absolute_days = serial.trunc() as i64 + days_from_civil(1899, 12, 30);
    let weekday = iso_weekday_from_absolute_days(absolute_days);
    let thursday_days = absolute_days + (4 - weekday) as i64;
    let (iso_year, _iso_month, _iso_day) = civil_from_days(thursday_days);
    let week_one = iso_week_one_monday(iso_year);
    Ok(((absolute_days - week_one).div_euclid(7) + 1) as f64)
}

pub fn weeknum_value(values: Vec<f64>) -> Result<f64, String> {
    if values.is_empty() || values.len() > 2 {
        return Err("#VALUE!".to_string());
    }
    if values.iter().any(|value| !value.is_finite()) {
        return Err("#VALUE!".to_string());
    }
    let serial = values[0].trunc() as i64;
    let mode = values.get(1).copied().unwrap_or(1.0).trunc() as i32;
    if mode == 21 {
        return iso_weeknum_value(vec![serial as f64]);
    }
    let first_weekday = match mode {
        1 => 7,
        2 => 1,
        11..=17 => mode - 10,
        _ => return Err("#NUM!".to_string()),
    };
    let absolute_days = serial + days_from_civil(1899, 12, 30);
    let (year, _month, _day) = civil_from_days(absolute_days);
    let jan1 = days_from_civil(year, 1, 1);
    let day_of_year_zero = absolute_days - jan1;
    let jan1_offset = (iso_weekday_from_absolute_days(jan1) - first_weekday).rem_euclid(7);
    Ok((day_of_year_zero + i64::from(jan1_offset)).div_euclid(7) as f64 + 1.0)
}

pub fn iso_weekday_from_absolute_days(days: i64) -> i32 {
    (days - days_from_civil(1970, 1, 5)).rem_euclid(7) as i32 + 1
}

pub fn iso_week_one_monday(year: i32) -> i64 {
    let jan4 = days_from_civil(year, 1, 4);
    jan4 - (iso_weekday_from_absolute_days(jan4) - 1) as i64
}

pub fn is_serial_weekday(serial: i64) -> bool {
    let days = serial + days_from_civil(1899, 12, 30);
    let monday_zero = (days - days_from_civil(1970, 1, 5)).rem_euclid(7);
    monday_zero < 5
}

pub fn full_months_between(
    start_year: i32,
    start_month: u32,
    start_day: u32,
    end_year: i32,
    end_month: u32,
    end_day: u32,
) -> i32 {
    let mut months = (end_year - start_year) * 12 + end_month as i32 - start_month as i32;
    if end_day < start_day {
        months -= 1;
    }
    months
}

pub fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 31,
    }
}

pub fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

pub fn time_value(values: Vec<f64>) -> Result<f64, String> {
    if values.len() != 3 {
        return Err("#VALUE!".to_string());
    }
    if values.iter().any(|value| !value.is_finite()) {
        return Err("#VALUE!".to_string());
    }
    let hours = values[0].trunc();
    let minutes = values[1].trunc();
    let seconds = values[2].trunc();
    if hours < 0.0 || minutes < 0.0 || seconds < 0.0 {
        return Err("#NUM!".to_string());
    }
    let total_seconds = hours * 3600.0 + minutes * 60.0 + seconds;
    if !total_seconds.is_finite() {
        return Err("#NUM!".to_string());
    }
    Ok((total_seconds % 86_400.0) / 86_400.0)
}

fn time_part_value(values: Vec<f64>, part: TimePart) -> Result<f64, String> {
    if values.len() != 1 {
        return Err("#VALUE!".to_string());
    }
    let serial = values[0];
    if !serial.is_finite() {
        return Err("#VALUE!".to_string());
    }
    let fraction = serial - serial.floor();
    let seconds = ((fraction * 86_400.0).round() as i64).rem_euclid(86_400);
    Ok(match part {
        TimePart::Hour => (seconds / 3600) as f64,
        TimePart::Minute => ((seconds % 3600) / 60) as f64,
        TimePart::Second => (seconds % 60) as f64,
    })
}

pub fn days_value(values: Vec<f64>) -> Result<f64, String> {
    if values.len() != 2 {
        return Err("#VALUE!".to_string());
    }
    if values.iter().any(|value| !value.is_finite()) {
        return Err("#VALUE!".to_string());
    }
    Ok(values[0].trunc() - values[1].trunc())
}

pub fn days360_value(values: Vec<f64>) -> Result<f64, String> {
    if values.len() != 2 && values.len() != 3 {
        return Err("#VALUE!".to_string());
    }
    if values.iter().any(|value| !value.is_finite()) {
        return Err("#VALUE!".to_string());
    }
    let (start_year, start_month, start_day) = serial_to_civil(values[0]);
    let (mut end_year, mut end_month, mut end_day) = serial_to_civil(values[1]);
    let european = values.get(2).copied().unwrap_or(0.0).trunc() != 0.0;
    let mut start_day = start_day;
    if european {
        start_day = start_day.min(30);
        end_day = end_day.min(30);
    } else {
        let start_is_feb_end = start_month == 2 && start_day == days_in_month(start_year, 2);
        let end_is_feb_end = end_month == 2 && end_day == days_in_month(end_year, 2);
        if start_is_feb_end {
            start_day = 30;
        }
        if end_is_feb_end && start_day >= 30 {
            end_day = 30;
        }
        if start_day == 31 {
            start_day = 30;
        }
        if end_day == 31 {
            if start_day >= 30 {
                end_day = 30;
            } else {
                end_day = 1;
                if end_month == 12 {
                    end_month = 1;
                    end_year += 1;
                } else {
                    end_month += 1;
                }
            }
        }
    }
    Ok(((end_year - start_year) * 360
        + (end_month as i32 - start_month as i32) * 30
        + end_day as i32
        - start_day as i32) as f64)
}

pub fn weekday_value(values: Vec<f64>) -> Result<f64, String> {
    if values.is_empty() || values.len() > 2 {
        return Err("#VALUE!".to_string());
    }
    if values.iter().any(|value| !value.is_finite()) {
        return Err("#VALUE!".to_string());
    }
    let serial = values[0].trunc() as i64;
    let mode = values.get(1).copied().unwrap_or(1.0).trunc() as i32;
    let monday_zero = (serial + 5).rem_euclid(7);
    let value = match mode {
        1 => ((monday_zero + 1).rem_euclid(7) + 1) as f64,
        2 => (monday_zero + 1) as f64,
        3 => monday_zero as f64,
        11..=17 => ((monday_zero - i64::from(mode - 11)).rem_euclid(7) + 1) as f64,
        _ => return Err("#NUM!".to_string()),
    };
    Ok(value)
}

pub fn serial_to_civil(serial: f64) -> (i32, u32, u32) {
    civil_from_days(serial.trunc() as i64 + days_from_civil(1899, 12, 30))
}

pub fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let year = year - if month <= 2 { 1 } else { 0 };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month = month as i32;
    let day = day as i32;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    (era * 146097 + day_of_era - 719468) as i64
}

pub fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let days = days + 719468;
    let era = if days >= 0 { days } else { days - 146096 } / 146097;
    let day_of_era = days - era * 146097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36524 - day_of_era / 146096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += if month <= 2 { 1 } else { 0 };
    (year as i32, month as u32, day as u32)
}

pub fn if_value(values: Vec<f64>) -> Result<f64, String> {
    let mut values = values.into_iter();
    let Some(condition) = values.next() else {
        return Err("#VALUE!".to_string());
    };
    let Some(when_true) = values.next() else {
        return Err("#VALUE!".to_string());
    };
    let when_false = values.next().unwrap_or(0.0);
    if values.next().is_some() {
        return Err("#VALUE!".to_string());
    }
    Ok(if condition != 0.0 {
        when_true
    } else {
        when_false
    })
}
