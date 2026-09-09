pub fn compare_function(
    values: Vec<f64>,
    compare: impl FnOnce(f64, f64) -> bool,
) -> Result<f64, String> {
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
    Ok(if compare(left, right) { 1.0 } else { 0.0 })
}

pub fn isbetween_value(values: Vec<f64>) -> Result<f64, String> {
    if values.len() < 3 || values.len() > 5 {
        return Err("#VALUE!".to_string());
    }
    if values.iter().any(|value| !value.is_finite()) {
        return Err("#VALUE!".to_string());
    }
    let value = values[0];
    let lower = values[1];
    let upper = values[2];
    let lower_inclusive = values.get(3).copied().unwrap_or(1.0) != 0.0;
    let upper_inclusive = values.get(4).copied().unwrap_or(1.0) != 0.0;
    let lower_ok = if lower_inclusive {
        value >= lower
    } else {
        value > lower
    };
    let upper_ok = if upper_inclusive {
        value <= upper
    } else {
        value < upper
    };
    Ok(if lower_ok && upper_ok { 1.0 } else { 0.0 })
}

pub fn parity_predicate(values: Vec<f64>, parity: i64) -> Result<f64, String> {
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
    let value = value.trunc() as i64;
    Ok(if value.rem_euclid(2) == parity {
        1.0
    } else {
        0.0
    })
}

pub fn binary_operator_function(
    values: Vec<f64>,
    apply: impl FnOnce(f64, f64) -> f64,
) -> Result<f64, String> {
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
    let out = apply(left, right);
    if out.is_finite() {
        Ok(out)
    } else {
        Err("#NUM!".to_string())
    }
}

pub fn standard_deviation(values: Vec<f64>, sample: bool) -> Result<f64, String> {
    let out = variance(values, sample)?.sqrt();
    if out.is_finite() {
        Ok(out)
    } else {
        Err("#NUM!".to_string())
    }
}

pub fn variance(values: Vec<f64>, sample: bool) -> Result<f64, String> {
    if values.iter().any(|value| !value.is_finite()) {
        return Err("#NUM!".to_string());
    }
    let len = values.len();
    if len == 0 || (sample && len < 2) {
        return Err("#DIV/0!".to_string());
    }
    let mean = values.iter().sum::<f64>() / len as f64;
    let sum_sq = values
        .iter()
        .map(|value| {
            let delta = value - mean;
            delta * delta
        })
        .sum::<f64>();
    let divisor = if sample { len - 1 } else { len } as f64;
    let out = sum_sq / divisor;
    if out.is_finite() {
        Ok(out)
    } else {
        Err("#NUM!".to_string())
    }
}

pub fn correlation_value(left: &[f64], right: &[f64]) -> Result<f64, String> {
    if left.len() != right.len() || left.len() < 2 {
        return Err("#DIV/0!".to_string());
    }
    if left
        .iter()
        .chain(right.iter())
        .any(|value| !value.is_finite())
    {
        return Err("#NUM!".to_string());
    }
    let left_mean = left.iter().sum::<f64>() / left.len() as f64;
    let right_mean = right.iter().sum::<f64>() / right.len() as f64;
    let mut numerator = 0.0;
    let mut left_sum_squares = 0.0;
    let mut right_sum_squares = 0.0;
    for (left_value, right_value) in left.iter().zip(right.iter()) {
        let left_delta = *left_value - left_mean;
        let right_delta = *right_value - right_mean;
        numerator += left_delta * right_delta;
        left_sum_squares += left_delta * left_delta;
        right_sum_squares += right_delta * right_delta;
    }
    if left_sum_squares == 0.0 || right_sum_squares == 0.0 {
        return Err("#DIV/0!".to_string());
    }
    let out = clamp_near_unit(numerator / (left_sum_squares.sqrt() * right_sum_squares.sqrt()));
    if out.is_finite() {
        Ok(out)
    } else {
        Err("#NUM!".to_string())
    }
}

pub fn covariance_value(left: &[f64], right: &[f64], sample: bool) -> Result<f64, String> {
    if left.len() != right.len() || left.len() < if sample { 2 } else { 1 } {
        return Err("#DIV/0!".to_string());
    }
    if left
        .iter()
        .chain(right.iter())
        .any(|value| !value.is_finite())
    {
        return Err("#NUM!".to_string());
    }
    let left_mean = left.iter().sum::<f64>() / left.len() as f64;
    let right_mean = right.iter().sum::<f64>() / right.len() as f64;
    let sum_products = left
        .iter()
        .zip(right.iter())
        .map(|(left_value, right_value)| (*left_value - left_mean) * (*right_value - right_mean))
        .sum::<f64>();
    let divisor = if sample { left.len() - 1 } else { left.len() } as f64;
    let out = sum_products / divisor;
    if out.is_finite() {
        Ok(out)
    } else {
        Err("#NUM!".to_string())
    }
}

pub fn regression_value(known_y: &[f64], known_x: &[f64], function: &str) -> Result<f64, String> {
    let stats = regression_stats(known_y, known_x, function == "RSQ")?;
    let out = match function {
        "SLOPE" => stats.slope,
        "INTERCEPT" => stats.intercept,
        "RSQ" => stats.rsq.ok_or_else(|| "#DIV/0!".to_string())?,
        _ => return Err("#NAME?".to_string()),
    };
    if out.is_finite() {
        Ok(out)
    } else {
        Err("#NUM!".to_string())
    }
}

pub fn forecast_value(x: f64, known_y: &[f64], known_x: &[f64]) -> Result<f64, String> {
    let stats = regression_stats(known_y, known_x, false)?;
    let out = stats.intercept + stats.slope * x;
    if out.is_finite() {
        Ok(out)
    } else {
        Err("#NUM!".to_string())
    }
}

struct RegressionStats {
    slope: f64,
    intercept: f64,
    rsq: Option<f64>,
}

fn regression_stats(
    known_y: &[f64],
    known_x: &[f64],
    require_response_variance: bool,
) -> Result<RegressionStats, String> {
    if known_y.len() != known_x.len() || known_y.len() < 2 {
        return Err("#DIV/0!".to_string());
    }
    if known_y
        .iter()
        .chain(known_x.iter())
        .any(|value| !value.is_finite())
    {
        return Err("#NUM!".to_string());
    }
    let y_mean = known_y.iter().sum::<f64>() / known_y.len() as f64;
    let x_mean = known_x.iter().sum::<f64>() / known_x.len() as f64;
    let mut sum_xy = 0.0;
    let mut sum_xx = 0.0;
    let mut sum_yy = 0.0;
    for (y, x) in known_y.iter().zip(known_x.iter()) {
        let x_delta = *x - x_mean;
        let y_delta = *y - y_mean;
        sum_xy += x_delta * y_delta;
        sum_xx += x_delta * x_delta;
        sum_yy += y_delta * y_delta;
    }
    if sum_xx == 0.0 {
        return Err("#DIV/0!".to_string());
    }
    let slope = sum_xy / sum_xx;
    let intercept = y_mean - slope * x_mean;
    let rsq = if require_response_variance {
        if sum_yy == 0.0 {
            return Err("#DIV/0!".to_string());
        }
        let r = clamp_near_unit(sum_xy / (sum_xx.sqrt() * sum_yy.sqrt()));
        Some(r * r)
    } else {
        None
    };
    Ok(RegressionStats {
        slope,
        intercept,
        rsq,
    })
}

pub fn clamp_near_unit(value: f64) -> f64 {
    const EPSILON: f64 = 1e-12;
    if (value - 1.0).abs() <= EPSILON {
        1.0
    } else if (value + 1.0).abs() <= EPSILON {
        -1.0
    } else if value.abs() <= EPSILON {
        0.0
    } else {
        value
    }
}

pub fn mode_value(values: Vec<f64>) -> Result<f64, String> {
    if values.is_empty() {
        return Err("#VALUE!".to_string());
    }
    if values.iter().any(|value| !value.is_finite()) {
        return Err("#NUM!".to_string());
    }
    let mut sorted = values;
    sorted.sort_by(f64::total_cmp);
    let mut best_value = sorted[0];
    let mut best_count = 1usize;
    let mut current_value = sorted[0];
    let mut current_count = 1usize;
    for value in sorted.into_iter().skip(1) {
        if value == current_value {
            current_count += 1;
            continue;
        }
        if current_count > best_count {
            best_value = current_value;
            best_count = current_count;
        }
        current_value = value;
        current_count = 1;
    }
    if current_count > best_count {
        best_value = current_value;
        best_count = current_count;
    }
    if best_count < 2 {
        Err("#N/A".to_string())
    } else {
        Ok(best_value)
    }
}

pub fn average_deviation(values: Vec<f64>) -> Result<f64, String> {
    let len = values.len();
    if len < 2 {
        return Err("#NUM!".to_string());
    }
    let sum_sq = deviation_sum(values, |delta| delta.abs())?;
    Ok(sum_sq / len as f64)
}

pub fn deviation_sum_squares(values: Vec<f64>) -> Result<f64, String> {
    if values.len() < 2 {
        return Ok(0.0);
    }
    deviation_sum(values, |delta| delta * delta)
}

pub fn erf_value(values: Vec<f64>) -> Result<f64, String> {
    let mut values = values.into_iter();
    let Some(lower) = values.next() else {
        return Err("#VALUE!".to_string());
    };
    let upper = values.next();
    if values.next().is_some() {
        return Err("#VALUE!".to_string());
    }
    if !lower.is_finite() || upper.is_some_and(|value| !value.is_finite()) {
        return Err("#NUM!".to_string());
    }
    Ok(match upper {
        Some(upper) => erf_approx(upper) - erf_approx(lower),
        None => erf_approx(lower),
    })
}

pub fn erf_approx(value: f64) -> f64 {
    let sign = if value < 0.0 { -1.0 } else { 1.0 };
    let x = value.abs();
    let t = 1.0 / (1.0 + 0.3275911 * x);
    let polynomial = (((((1.061405429 * t - 1.453152027) * t) + 1.421413741) * t - 0.284496736)
        * t
        + 0.254829592)
        * t;
    sign * (1.0 - polynomial * (-x * x).exp())
}

pub fn geometric_mean(values: Vec<f64>) -> Result<f64, String> {
    if values.is_empty() {
        return Err("#VALUE!".to_string());
    }
    if values
        .iter()
        .any(|value| !value.is_finite() || *value <= 0.0)
    {
        return Err("#NUM!".to_string());
    }
    let log_sum = values.iter().map(|value| value.ln()).sum::<f64>();
    let out = (log_sum / values.len() as f64).exp();
    if out.is_finite() {
        Ok(out)
    } else {
        Err("#NUM!".to_string())
    }
}

pub fn harmonic_mean(values: Vec<f64>) -> Result<f64, String> {
    if values.is_empty() {
        return Err("#VALUE!".to_string());
    }
    if values
        .iter()
        .any(|value| !value.is_finite() || *value <= 0.0)
    {
        return Err("#NUM!".to_string());
    }
    let reciprocal_sum = values.iter().map(|value| 1.0 / value).sum::<f64>();
    let out = values.len() as f64 / reciprocal_sum;
    if out.is_finite() {
        Ok(out)
    } else {
        Err("#NUM!".to_string())
    }
}

pub fn percentile_value(values: Vec<f64>) -> Result<f64, String> {
    if values.len() < 2 {
        return Err("#VALUE!".to_string());
    }
    let mut values = values;
    let Some(percentile) = values.pop() else {
        return Err("#VALUE!".to_string());
    };
    if values.is_empty() {
        return Err("#NUM!".to_string());
    }
    percentile_from_sorted(values, percentile)
}

pub fn quartile_value(values: Vec<f64>) -> Result<f64, String> {
    if values.len() < 2 {
        return Err("#VALUE!".to_string());
    }
    let mut values = values;
    let Some(quartile) = values.pop() else {
        return Err("#VALUE!".to_string());
    };
    if values.is_empty() {
        return Err("#NUM!".to_string());
    }
    if !quartile.is_finite() {
        return Err("#NUM!".to_string());
    }
    let quartile = quartile.trunc();
    if !(0.0..=4.0).contains(&quartile) {
        return Err("#NUM!".to_string());
    }
    percentile_from_sorted(values, quartile / 4.0)
}

pub fn percentile_exc_value(values: Vec<f64>) -> Result<f64, String> {
    if values.len() < 2 {
        return Err("#VALUE!".to_string());
    }
    let mut values = values;
    let Some(percentile) = values.pop() else {
        return Err("#VALUE!".to_string());
    };
    if values.is_empty() {
        return Err("#NUM!".to_string());
    }
    percentile_exc_from_sorted(values, percentile)
}

pub fn quartile_exc_value(values: Vec<f64>) -> Result<f64, String> {
    if values.len() < 2 {
        return Err("#VALUE!".to_string());
    }
    let mut values = values;
    let Some(quartile) = values.pop() else {
        return Err("#VALUE!".to_string());
    };
    if values.is_empty() || !quartile.is_finite() {
        return Err("#NUM!".to_string());
    }
    let quartile = quartile.trunc();
    if !(1.0..=3.0).contains(&quartile) {
        return Err("#NUM!".to_string());
    }
    percentile_exc_from_sorted(values, quartile / 4.0)
}

pub fn percentile_from_sorted(mut values: Vec<f64>, percentile: f64) -> Result<f64, String> {
    if !percentile.is_finite()
        || !(0.0..=1.0).contains(&percentile)
        || values.iter().any(|value| !value.is_finite())
    {
        return Err("#NUM!".to_string());
    }
    values.sort_by(f64::total_cmp);
    if values.len() == 1 {
        return Ok(values[0]);
    }
    let position = percentile * (values.len() - 1) as f64;
    let lower = position.floor() as usize;
    let upper = position.ceil() as usize;
    let fraction = position - lower as f64;
    let out = values[lower] + (values[upper] - values[lower]) * fraction;
    if out.is_finite() {
        Ok(out)
    } else {
        Err("#NUM!".to_string())
    }
}

pub fn percentile_exc_from_sorted(mut values: Vec<f64>, percentile: f64) -> Result<f64, String> {
    if !percentile.is_finite()
        || !(0.0..1.0).contains(&percentile)
        || values.iter().any(|value| !value.is_finite())
    {
        return Err("#NUM!".to_string());
    }
    values.sort_by(f64::total_cmp);
    let position = percentile * (values.len() + 1) as f64;
    if position < 1.0 || position > values.len() as f64 {
        return Err("#NUM!".to_string());
    }
    let lower_position = position.floor();
    let upper_position = position.ceil();
    let lower = (lower_position as usize).saturating_sub(1);
    let upper = (upper_position as usize).saturating_sub(1);
    let fraction = position - lower_position;
    let out = values[lower] + (values[upper] - values[lower]) * fraction;
    if out.is_finite() {
        Ok(out)
    } else {
        Err("#NUM!".to_string())
    }
}

pub fn percentrank_value(
    mut values: Vec<f64>,
    target: f64,
    significant_digits: Option<i32>,
    exclusive: bool,
) -> Result<f64, String> {
    if values.is_empty() || values.iter().any(|value| !value.is_finite()) {
        return Err("#NUM!".to_string());
    }
    values.sort_by(f64::total_cmp);
    let min = values[0];
    let max = values[values.len() - 1];
    if target < min || target > max {
        return Err("#N/A".to_string());
    }
    let divisor = if exclusive {
        (values.len() + 1) as f64
    } else {
        (values.len() - 1).max(1) as f64
    };
    let mut rank = None;
    for (index, value) in values.iter().enumerate() {
        if *value == target {
            let position = if exclusive {
                (index + 1) as f64
            } else {
                index as f64
            };
            rank = Some(position / divisor);
            break;
        }
        if *value > target {
            let lower_index = index - 1;
            let upper_index = index;
            let lower_value = values[lower_index];
            let upper_value = values[upper_index];
            if upper_value == lower_value {
                let position = if exclusive {
                    (lower_index + 1) as f64
                } else {
                    lower_index as f64
                };
                rank = Some(position / divisor);
            } else {
                let fraction = (target - lower_value) / (upper_value - lower_value);
                let position = if exclusive {
                    (lower_index + 1) as f64 + fraction
                } else {
                    lower_index as f64 + fraction
                };
                rank = Some(position / divisor);
            }
            break;
        }
    }
    let mut out = rank.unwrap_or(1.0);
    if let Some(digits) = significant_digits {
        let factor = 10_f64.powi(digits);
        out = (out * factor).trunc() / factor;
    }
    if out.is_finite() {
        Ok(out)
    } else {
        Err("#NUM!".to_string())
    }
}

pub fn combin_value(values: Vec<f64>) -> Result<f64, String> {
    let (n, k) = integer_pair(values)?;
    if k > n {
        return Err("#NUM!".to_string());
    }
    combination_from_integers(n, k)
}

pub fn combina_value(values: Vec<f64>) -> Result<f64, String> {
    let (n, k) = integer_pair(values)?;
    if n == 0 && k > 0 {
        return Err("#NUM!".to_string());
    }
    if n == 0 && k == 0 {
        return Ok(1.0);
    }
    let Some(sum) = n.checked_add(k) else {
        return Err("#NUM!".to_string());
    };
    let Some(pool) = sum.checked_sub(1) else {
        return Err("#NUM!".to_string());
    };
    if sum >= 1031 {
        return Err("#NUM!".to_string());
    }
    combination_from_integers(pool, k)
}

pub fn combination_from_integers(n: u64, k: u64) -> Result<f64, String> {
    let k = k.min(n.saturating_sub(k));
    let mut out = 1.0;
    for i in 1..=k {
        out *= (n - k + i) as f64 / i as f64;
        if !out.is_finite() {
            return Err("#NUM!".to_string());
        }
    }
    Ok(out.round())
}

pub fn permut_value(values: Vec<f64>) -> Result<f64, String> {
    let (n, k) = integer_pair(values)?;
    if k > n {
        return Err("#NUM!".to_string());
    }
    let mut out = 1.0;
    for value in (n - k + 1)..=n {
        out *= value as f64;
        if !out.is_finite() {
            return Err("#NUM!".to_string());
        }
    }
    Ok(out)
}

pub fn permutationa_value(values: Vec<f64>) -> Result<f64, String> {
    let (n, k) = integer_pair(values)?;
    if n == 0 || k > n {
        return Err("#NUM!".to_string());
    }
    let out = (n as f64).powf(k as f64);
    if out.is_finite() {
        Ok(out)
    } else {
        Err("#NUM!".to_string())
    }
}

pub fn factorial_value(values: Vec<f64>) -> Result<f64, String> {
    let value = unary_integer(values)?;
    let mut out = 1.0;
    for item in 2..=value {
        out *= item as f64;
        if !out.is_finite() {
            return Err("#NUM!".to_string());
        }
    }
    Ok(out)
}

pub fn double_factorial_value(values: Vec<f64>) -> Result<f64, String> {
    let value = unary_integer(values)?;
    let mut out = 1.0;
    let mut item = value;
    while item > 1 {
        out *= item as f64;
        if !out.is_finite() {
            return Err("#NUM!".to_string());
        }
        item -= 2;
    }
    Ok(out)
}

pub fn gcd_value(values: Vec<f64>) -> Result<f64, String> {
    let values = integer_values(values)?;
    let out = values.into_iter().fold(0, gcd_u64);
    Ok(out as f64)
}

pub fn lcm_value(values: Vec<f64>) -> Result<f64, String> {
    const MAX_EXACT_INTEGER: u128 = 9_007_199_254_740_992;
    let values = integer_values(values)?;
    let mut out = 1u128;
    for value in values {
        if value == 0 {
            return Ok(0.0);
        }
        let divisor = gcd_u64(out as u64, value) as u128;
        out = (out / divisor) * value as u128;
        if out >= MAX_EXACT_INTEGER {
            return Err("#NUM!".to_string());
        }
    }
    Ok(out as f64)
}

pub fn gcd_u64(mut left: u64, mut right: u64) -> u64 {
    while right != 0 {
        let next = left % right;
        left = right;
        right = next;
    }
    left
}

pub fn integer_values(values: Vec<f64>) -> Result<Vec<u64>, String> {
    if values.is_empty() {
        return Err("#VALUE!".to_string());
    }
    values
        .into_iter()
        .map(|value| {
            if !value.is_finite() || value < 0.0 || value > u64::MAX as f64 {
                Err("#NUM!".to_string())
            } else {
                Ok(value.trunc() as u64)
            }
        })
        .collect()
}

pub fn unary_integer(values: Vec<f64>) -> Result<u64, String> {
    let mut values = values.into_iter();
    let Some(value) = values.next() else {
        return Err("#VALUE!".to_string());
    };
    if values.next().is_some() {
        return Err("#VALUE!".to_string());
    }
    if !value.is_finite() || value < 0.0 {
        return Err("#NUM!".to_string());
    }
    Ok(value.trunc() as u64)
}

pub fn integer_pair(values: Vec<f64>) -> Result<(u64, u64), String> {
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
    if !left.is_finite()
        || !right.is_finite()
        || left < 0.0
        || right < 0.0
        || left > u64::MAX as f64
        || right > u64::MAX as f64
    {
        return Err("#NUM!".to_string());
    }
    Ok((left.trunc() as u64, right.trunc() as u64))
}

pub fn deviation_sum<F>(values: Vec<f64>, map: F) -> Result<f64, String>
where
    F: Fn(f64) -> f64,
{
    if values.iter().any(|value| !value.is_finite()) {
        return Err("#NUM!".to_string());
    }
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    let out = values
        .into_iter()
        .map(|value| map(value - mean))
        .sum::<f64>();
    if out.is_finite() {
        Ok(out)
    } else {
        Err("#NUM!".to_string())
    }
}

pub fn ranked_value(mut values: Vec<f64>, largest: bool) -> Result<f64, String> {
    let Some(rank) = values.pop() else {
        return Err("#VALUE!".to_string());
    };
    if values.is_empty() || !rank.is_finite() || rank.fract() != 0.0 {
        return Err("#NUM!".to_string());
    }
    if values.iter().any(|value| !value.is_finite()) {
        return Err("#NUM!".to_string());
    }
    let rank = rank as usize;
    if rank == 0 || rank > values.len() {
        return Err("#NUM!".to_string());
    }
    values.sort_by(f64::total_cmp);
    let index = if largest {
        values.len() - rank
    } else {
        rank - 1
    };
    Ok(values[index])
}

pub fn round_away_to_parity(value: f64, parity: i64) -> f64 {
    if value == 0.0 && parity == 0 {
        return 0.0;
    }
    let sign = if value < 0.0 { -1.0 } else { 1.0 };
    let mut rounded = value.abs().ceil() as i64;
    if rounded % 2 != parity {
        rounded += 1;
    }
    sign * rounded as f64
}

pub fn floor_math_value(values: Vec<f64>) -> Result<f64, String> {
    let mut values = values.into_iter();
    let Some(number) = values.next() else {
        return Err("#VALUE!".to_string());
    };
    let significance = values.next().unwrap_or(1.0).abs();
    let mode = values.next().unwrap_or(0.0);
    if values.next().is_some() {
        return Err("#VALUE!".to_string());
    }
    if significance == 0.0 {
        return Err("#DIV/0!".to_string());
    }
    let rounded = if number < 0.0 && mode != 0.0 {
        (number / significance).ceil() * significance
    } else {
        (number / significance).floor() * significance
    };
    if rounded.is_finite() {
        Ok(rounded)
    } else {
        Err("#NUM!".to_string())
    }
}

pub fn floor_precise_value(values: Vec<f64>) -> Result<f64, String> {
    let mut values = values.into_iter();
    let Some(number) = values.next() else {
        return Err("#VALUE!".to_string());
    };
    let significance = values.next().unwrap_or(1.0).abs();
    if values.next().is_some() {
        return Err("#VALUE!".to_string());
    }
    if significance == 0.0 {
        return Err("#DIV/0!".to_string());
    }
    let rounded = (number / significance).floor() * significance;
    if rounded.is_finite() {
        Ok(rounded)
    } else {
        Err("#NUM!".to_string())
    }
}

pub fn ceiling_math_value(values: Vec<f64>) -> Result<f64, String> {
    let mut values = values.into_iter();
    let Some(number) = values.next() else {
        return Err("#VALUE!".to_string());
    };
    let significance = values.next().unwrap_or(1.0).abs();
    let mode = values.next().unwrap_or(0.0);
    if values.next().is_some() {
        return Err("#VALUE!".to_string());
    }
    if significance == 0.0 {
        return Err("#DIV/0!".to_string());
    }
    let rounded = if number < 0.0 && mode != 0.0 {
        (number / significance).floor() * significance
    } else {
        (number / significance).ceil() * significance
    };
    if rounded.is_finite() {
        Ok(rounded)
    } else {
        Err("#NUM!".to_string())
    }
}

pub fn ceiling_precise_value(values: Vec<f64>) -> Result<f64, String> {
    let mut values = values.into_iter();
    let Some(number) = values.next() else {
        return Err("#VALUE!".to_string());
    };
    let significance = values.next().unwrap_or(1.0).abs();
    if values.next().is_some() {
        return Err("#VALUE!".to_string());
    }
    if significance == 0.0 {
        return Err("#DIV/0!".to_string());
    }
    let rounded = (number / significance).ceil() * significance;
    if rounded.is_finite() {
        Ok(rounded)
    } else {
        Err("#NUM!".to_string())
    }
}
