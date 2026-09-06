//! Formula value model shared by the parser, evaluator, and projections.

use crate::trim_number;

/// Standard Google Sheets error codes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FormulaError {
    Value,
    Div0,
    Ref,
    Name,
    Na,
    Num,
    Parse,
    Cycle,
}

impl FormulaError {
    pub fn code(self) -> &'static str {
        match self {
            FormulaError::Value => "#VALUE!",
            FormulaError::Div0 => "#DIV/0!",
            FormulaError::Ref | FormulaError::Cycle => "#REF!",
            FormulaError::Name => "#NAME?",
            FormulaError::Na => "#N/A",
            FormulaError::Num => "#NUM!",
            FormulaError::Parse => "#ERROR!",
        }
    }

    /// Parses a standard error code (also accepting legacy codes without
    /// the trailing punctuation).
    pub fn from_code(code: &str) -> Option<Self> {
        match code.trim().to_ascii_uppercase().as_str() {
            "#VALUE!" | "#VALUE" => Some(FormulaError::Value),
            "#DIV/0!" | "#DIV/0" => Some(FormulaError::Div0),
            "#REF!" | "#REF" | "#RANGE" => Some(FormulaError::Ref),
            "#NAME?" | "#NAME" => Some(FormulaError::Name),
            "#N/A" => Some(FormulaError::Na),
            "#NUM!" | "#NUM" => Some(FormulaError::Num),
            "#ERROR!" | "#PARSE" => Some(FormulaError::Parse),
            "#CYCLE" => Some(FormulaError::Cycle),
            _ => None,
        }
    }

    /// `ERROR.TYPE` code.
    pub fn type_code(self) -> f64 {
        match self {
            FormulaError::Div0 => 2.0,
            FormulaError::Value => 3.0,
            FormulaError::Ref | FormulaError::Cycle => 4.0,
            FormulaError::Name => 5.0,
            FormulaError::Num => 6.0,
            FormulaError::Na => 7.0,
            FormulaError::Parse => 8.0,
        }
    }

    /// Maps a legacy `Result<_, String>` error string to a formula error.
    pub fn from_legacy(message: &str) -> Self {
        Self::from_code(message).unwrap_or(FormulaError::Value)
    }
}

/// A rectangular block of values in row-major order.
#[derive(Clone, Debug, PartialEq)]
pub struct FormulaArray {
    pub rows: usize,
    pub cols: usize,
    pub values: Vec<FormulaValue>,
}

impl FormulaArray {
    pub fn new(rows: usize, cols: usize, values: Vec<FormulaValue>) -> Self {
        debug_assert_eq!(rows * cols, values.len());
        Self { rows, cols, values }
    }

    pub fn from_rows(rows: Vec<Vec<FormulaValue>>) -> Self {
        let row_count = rows.len();
        let col_count = rows.first().map(Vec::len).unwrap_or(0);
        let mut values = Vec::with_capacity(row_count * col_count);
        for mut row in rows {
            row.resize(col_count, FormulaValue::Blank);
            values.extend(row);
        }
        Self {
            rows: row_count,
            cols: col_count,
            values,
        }
    }

    pub fn column(values: Vec<FormulaValue>) -> Self {
        Self {
            rows: values.len(),
            cols: 1,
            values,
        }
    }

    pub fn row(values: Vec<FormulaValue>) -> Self {
        Self {
            rows: 1,
            cols: values.len(),
            values,
        }
    }

    pub fn get(&self, row: usize, col: usize) -> &FormulaValue {
        &self.values[row * self.cols + col]
    }

    pub fn row_values(&self, row: usize) -> &[FormulaValue] {
        &self.values[row * self.cols..(row + 1) * self.cols]
    }

    pub fn column_values(&self, col: usize) -> Vec<FormulaValue> {
        (0..self.rows).map(|row| self.get(row, col).clone()).collect()
    }

    pub fn transpose(&self) -> Self {
        let mut values = Vec::with_capacity(self.values.len());
        for col in 0..self.cols {
            for row in 0..self.rows {
                values.push(self.get(row, col).clone());
            }
        }
        Self {
            rows: self.cols,
            cols: self.rows,
            values,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

/// A formula evaluation result.
#[derive(Clone, Debug, PartialEq)]
pub enum FormulaValue {
    Blank,
    Number(f64),
    Text(String),
    Bool(bool),
    Error(FormulaError),
    Array(FormulaArray),
}

impl From<FormulaError> for FormulaValue {
    fn from(error: FormulaError) -> Self {
        FormulaValue::Error(error)
    }
}

impl From<f64> for FormulaValue {
    fn from(value: f64) -> Self {
        FormulaValue::Number(value)
    }
}

impl From<bool> for FormulaValue {
    fn from(value: bool) -> Self {
        FormulaValue::Bool(value)
    }
}

impl From<String> for FormulaValue {
    fn from(value: String) -> Self {
        FormulaValue::Text(value)
    }
}

impl FormulaValue {
    pub fn text(value: impl Into<String>) -> Self {
        FormulaValue::Text(value.into())
    }

    pub fn number(value: f64) -> Self {
        if value.is_finite() {
            FormulaValue::Number(value)
        } else {
            FormulaValue::Error(FormulaError::Num)
        }
    }

    pub fn is_blank(&self) -> bool {
        matches!(self, FormulaValue::Blank)
    }

    pub fn is_error(&self) -> bool {
        matches!(self, FormulaValue::Error(_))
    }

    pub fn error(&self) -> Option<FormulaError> {
        match self {
            FormulaValue::Error(error) => Some(*error),
            _ => None,
        }
    }

    /// Reduces an array to its top-left element for scalar contexts.
    pub fn scalar(&self) -> FormulaValue {
        match self {
            FormulaValue::Array(array) => array
                .values
                .first()
                .cloned()
                .unwrap_or(FormulaValue::Error(FormulaError::Ref)),
            other => other.clone(),
        }
    }

    /// Numeric coercion following Google Sheets arithmetic rules.
    pub fn to_number(&self) -> Result<f64, FormulaError> {
        match self {
            FormulaValue::Blank => Ok(0.0),
            FormulaValue::Number(value) => Ok(*value),
            FormulaValue::Bool(value) => Ok(if *value { 1.0 } else { 0.0 }),
            FormulaValue::Text(text) => parse_number_text(text).ok_or(FormulaError::Value),
            FormulaValue::Error(error) => Err(*error),
            FormulaValue::Array(_) => self.scalar().to_number(),
        }
    }

    /// Text coercion following Google Sheets rules.
    pub fn to_text(&self) -> Result<String, FormulaError> {
        match self {
            FormulaValue::Blank => Ok(String::new()),
            FormulaValue::Number(value) => Ok(trim_number(*value)),
            FormulaValue::Bool(value) => Ok(if *value { "TRUE" } else { "FALSE" }.to_string()),
            FormulaValue::Text(text) => Ok(text.clone()),
            FormulaValue::Error(error) => Err(*error),
            FormulaValue::Array(_) => self.scalar().to_text(),
        }
    }

    /// Boolean coercion following Google Sheets rules.
    pub fn to_bool(&self) -> Result<bool, FormulaError> {
        match self {
            FormulaValue::Blank => Ok(false),
            FormulaValue::Number(value) => Ok(*value != 0.0),
            FormulaValue::Bool(value) => Ok(*value),
            FormulaValue::Text(text) => match text.trim().to_ascii_uppercase().as_str() {
                "TRUE" => Ok(true),
                "FALSE" => Ok(false),
                _ => parse_number_text(text)
                    .map(|value| value != 0.0)
                    .ok_or(FormulaError::Value),
            },
            FormulaValue::Error(error) => Err(*error),
            FormulaValue::Array(_) => self.scalar().to_bool(),
        }
    }

    /// Projection kind used by `AppCell::computed_kind`.
    pub fn kind_label(&self) -> &'static str {
        match self {
            FormulaValue::Blank => "empty",
            FormulaValue::Number(_) => "number",
            FormulaValue::Text(_) => "string",
            FormulaValue::Bool(_) => "bool",
            FormulaValue::Error(_) => "error",
            FormulaValue::Array(_) => "error",
        }
    }

    /// Canonical projection value used by `AppCell::computed_value`.
    pub fn canonical_text(&self) -> String {
        match self {
            FormulaValue::Blank => String::new(),
            FormulaValue::Number(value) => trim_number(*value),
            FormulaValue::Text(text) => text.clone(),
            FormulaValue::Bool(value) => if *value { "true" } else { "false" }.to_string(),
            FormulaValue::Error(error) => error.code().to_string(),
            FormulaValue::Array(_) => FormulaError::Value.code().to_string(),
        }
    }

    /// Builds a value from a projected `(computed_kind, computed_value)` pair.
    pub fn from_projection(kind: &str, value: &str) -> FormulaValue {
        match kind {
            "number" => value
                .parse::<f64>()
                .ok()
                .filter(|value| value.is_finite())
                .map(FormulaValue::Number)
                .unwrap_or(FormulaValue::Error(FormulaError::Value)),
            "bool" => FormulaValue::Bool(value.eq_ignore_ascii_case("true")),
            "string" => FormulaValue::Text(value.to_string()),
            "error" => FormulaValue::Error(FormulaError::from_legacy(value)),
            _ => FormulaValue::Blank,
        }
    }

    /// `TYPE()` code.
    pub fn type_code(&self) -> f64 {
        match self {
            FormulaValue::Number(_) => 1.0,
            FormulaValue::Text(_) => 2.0,
            FormulaValue::Bool(_) => 4.0,
            FormulaValue::Error(_) => 16.0,
            FormulaValue::Array(_) => 64.0,
            FormulaValue::Blank => 128.0,
        }
    }
}

/// Parses numeric text as used by formula coercion (`"1,000"`, `"50%"`,
/// `" 3 "`).
pub fn parse_number_text(text: &str) -> Option<f64> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let (body, percent) = match trimmed.strip_suffix('%') {
        Some(rest) => (rest.trim_end(), true),
        None => (trimmed, false),
    };
    let cleaned: String = body.chars().filter(|ch| *ch != ',').collect();
    let value = cleaned.parse::<f64>().ok()?;
    if !value.is_finite() {
        return None;
    }
    Some(if percent { value / 100.0 } else { value })
}

/// Type rank used by comparison operators: numbers < text < booleans.
fn compare_rank(value: &FormulaValue) -> u8 {
    match value {
        FormulaValue::Number(_) | FormulaValue::Blank => 0,
        FormulaValue::Text(_) => 1,
        FormulaValue::Bool(_) => 2,
        _ => 3,
    }
}

/// Compares two scalar values with Google Sheets comparison semantics:
/// numbers compare numerically, text compares case-insensitively, booleans
/// compare with FALSE < TRUE, mixed types compare by type rank, and blanks
/// coerce to the other operand's type.
pub fn compare_values(
    left: &FormulaValue,
    right: &FormulaValue,
) -> Result<std::cmp::Ordering, FormulaError> {
    use std::cmp::Ordering;
    let left = left.scalar();
    let right = right.scalar();
    if let FormulaValue::Error(error) = left {
        return Err(error);
    }
    if let FormulaValue::Error(error) = right {
        return Err(error);
    }
    let (left, right) = match (&left, &right) {
        (FormulaValue::Blank, FormulaValue::Text(_)) => {
            (FormulaValue::Text(String::new()), right.clone())
        }
        (FormulaValue::Text(_), FormulaValue::Blank) => {
            (left.clone(), FormulaValue::Text(String::new()))
        }
        (FormulaValue::Blank, FormulaValue::Bool(_)) => (FormulaValue::Bool(false), right.clone()),
        (FormulaValue::Bool(_), FormulaValue::Blank) => (left.clone(), FormulaValue::Bool(false)),
        (FormulaValue::Blank, _) => (FormulaValue::Number(0.0), right.clone()),
        (_, FormulaValue::Blank) => (left.clone(), FormulaValue::Number(0.0)),
        _ => (left.clone(), right.clone()),
    };
    let left_rank = compare_rank(&left);
    let right_rank = compare_rank(&right);
    if left_rank != right_rank {
        return Ok(left_rank.cmp(&right_rank));
    }
    Ok(match (&left, &right) {
        (FormulaValue::Number(left), FormulaValue::Number(right)) => {
            left.partial_cmp(right).unwrap_or(Ordering::Equal)
        }
        (FormulaValue::Text(left), FormulaValue::Text(right)) => {
            left.to_lowercase().cmp(&right.to_lowercase())
        }
        (FormulaValue::Bool(left), FormulaValue::Bool(right)) => left.cmp(right),
        _ => Ordering::Equal,
    })
}

/// Equality as used by lookups and `SWITCH`: same type and equal value
/// (text compares case-insensitively).
pub fn values_match(left: &FormulaValue, right: &FormulaValue) -> bool {
    match (left, right) {
        (FormulaValue::Number(left), FormulaValue::Number(right)) => left == right,
        (FormulaValue::Text(left), FormulaValue::Text(right)) => left.eq_ignore_ascii_case(right),
        (FormulaValue::Bool(left), FormulaValue::Bool(right)) => left == right,
        (FormulaValue::Blank, FormulaValue::Blank) => true,
        (FormulaValue::Blank, FormulaValue::Text(text))
        | (FormulaValue::Text(text), FormulaValue::Blank) => text.is_empty(),
        _ => false,
    }
}
