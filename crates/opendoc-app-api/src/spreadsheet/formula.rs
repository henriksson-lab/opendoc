//! Formula parser (A1 grammar with Google Sheets operators) and the
//! expression evaluator core. Function implementations live in
//! [`super::functions`].

use std::collections::BTreeSet;

use super::functions;
use super::value::{compare_values, FormulaArray, FormulaError, FormulaValue};
use crate::{column_to_number, formula_sheet_title_prefix, number_to_column, AppCell};

/// One end of a reference. `col`/`row` are `None` for whole-row/whole-column
/// references (`A:A`, `1:1`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CellCoord {
    pub col: Option<u32>,
    pub row: Option<u32>,
    pub col_abs: bool,
    pub row_abs: bool,
}

impl CellCoord {
    pub fn cell(col: u32, row: u32) -> Self {
        Self {
            col: Some(col),
            row: Some(row),
            col_abs: false,
            row_abs: false,
        }
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        if let Some(col) = self.col {
            if self.col_abs {
                out.push('$');
            }
            out.push_str(&number_to_column(col).unwrap_or_default());
        }
        if let Some(row) = self.row {
            if self.row_abs {
                out.push('$');
            }
            out.push_str(&row.to_string());
        }
        out
    }
}

/// A cell or range reference with an optional sheet prefix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefExpr {
    /// Normalized (unquoted) sheet prefix as written in the formula.
    pub sheet: Option<String>,
    pub start: CellCoord,
    pub end: Option<CellCoord>,
}

impl RefExpr {
    pub fn is_range(&self) -> bool {
        self.end.is_some() || self.start.col.is_none() || self.start.row.is_none()
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        if let Some(sheet) = &self.sheet {
            out.push_str(&formula_sheet_title_prefix(sheet));
            out.push('!');
        }
        out.push_str(&self.start.to_text());
        if let Some(end) = &self.end {
            out.push(':');
            out.push_str(&end.to_text());
        }
        out
    }

    /// Column/row bounds (inclusive, 1-based) with `None` for unbounded axes.
    pub fn bounds(&self) -> (Option<u32>, Option<u32>, Option<u32>, Option<u32>) {
        let end = self.end.as_ref().unwrap_or(&self.start);
        let (c1, c2) = match (self.start.col, end.col) {
            (Some(a), Some(b)) => (Some(a.min(b)), Some(a.max(b))),
            _ => (None, None),
        };
        let (r1, r2) = match (self.start.row, end.row) {
            (Some(a), Some(b)) => (Some(a.min(b)), Some(a.max(b))),
            _ => (None, None),
        };
        (c1, c2, r1, r2)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Plus,
    Percent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
    Concat,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Blank,
    Number(f64),
    Text(String),
    Bool(bool),
    Error(FormulaError),
    Ref(RefExpr),
    Name(String),
    ArrayLiteral(Vec<Vec<Expr>>),
    Call {
        name: String,
        args: Vec<Expr>,
    },
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
    },
    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
}

impl Expr {
    /// Visits every reference and name in the expression tree.
    pub fn visit_refs<'a>(&'a self, visit: &mut dyn FnMut(&'a Expr)) {
        match self {
            Expr::Ref(_) | Expr::Name(_) => visit(self),
            Expr::Call { args, .. } => {
                visit(self);
                for arg in args {
                    arg.visit_refs(visit);
                }
            }
            Expr::Unary { expr, .. } => expr.visit_refs(visit),
            Expr::Binary { left, right, .. } => {
                left.visit_refs(visit);
                right.visit_refs(visit);
            }
            Expr::ArrayLiteral(rows) => {
                for row in rows {
                    for item in row {
                        item.visit_refs(visit);
                    }
                }
            }
            _ => {}
        }
    }

    /// True when the formula contains a function whose result may change
    /// without any referenced cell changing.
    pub fn is_volatile(&self) -> bool {
        let mut volatile = false;
        self.visit_refs(&mut |expr| {
            if let Expr::Call { name, .. } = expr {
                if matches!(
                    name.as_str(),
                    "NOW" | "TODAY" | "RAND" | "RANDBETWEEN" | "INDIRECT" | "OFFSET"
                ) {
                    volatile = true;
                }
            }
            if let Expr::Ref(reference) = expr {
                if reference.start.col.is_none() || reference.start.row.is_none() {
                    volatile = true;
                }
            }
        });
        volatile
    }
}

pub fn is_unquoted_sheet_prefix_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.')
}

fn is_name_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.')
}

/// Parses a formula body (without the leading `=`).
pub fn parse_formula(body: &str) -> Result<Expr, FormulaError> {
    let mut parser = Parser {
        chars: body.chars().collect(),
        pos: 0,
    };
    parser.skip_ws();
    if parser.at_end() {
        return Err(FormulaError::Parse);
    }
    let expr = parser.parse_comparison()?;
    parser.skip_ws();
    if !parser.at_end() {
        return Err(FormulaError::Parse);
    }
    Ok(expr)
}

/// Parses a formula cell source (with the leading `=`).
pub fn parse_formula_source(source: &str) -> Result<Expr, FormulaError> {
    let trimmed = source.trim();
    let Some(body) = trimmed.strip_prefix('=') else {
        return Err(FormulaError::Parse);
    };
    parse_formula(body)
}

/// Parses a bare reference such as `A1`, `$B$2:C3`, `Sheet!A:A`.
pub fn parse_reference(text: &str) -> Option<RefExpr> {
    match parse_formula(text.trim()) {
        Ok(Expr::Ref(reference)) => Some(reference),
        _ => None,
    }
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
}

impl Parser {
    fn at_end(&self) -> bool {
        self.pos >= self.chars.len()
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<char> {
        self.chars.get(self.pos + offset).copied()
    }

    fn skip_ws(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.pos += 1;
        }
    }

    fn consume(&mut self, expected: char) -> bool {
        if self.peek() == Some(expected) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn consume_str(&mut self, expected: &str) -> bool {
        let expected: Vec<char> = expected.chars().collect();
        if self.chars[self.pos..].starts_with(&expected) {
            self.pos += expected.len();
            true
        } else {
            false
        }
    }

    fn parse_comparison(&mut self) -> Result<Expr, FormulaError> {
        let mut left = self.parse_concat()?;
        loop {
            self.skip_ws();
            let op = if self.consume_str("<>") {
                BinaryOp::Ne
            } else if self.consume_str(">=") {
                BinaryOp::Ge
            } else if self.consume_str("<=") {
                BinaryOp::Le
            } else if self.consume('=') {
                BinaryOp::Eq
            } else if self.consume('>') {
                BinaryOp::Gt
            } else if self.consume('<') {
                BinaryOp::Lt
            } else {
                return Ok(left);
            };
            let right = self.parse_concat()?;
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
    }

    fn parse_concat(&mut self) -> Result<Expr, FormulaError> {
        let mut left = self.parse_additive()?;
        loop {
            self.skip_ws();
            if self.consume('&') {
                let right = self.parse_additive()?;
                left = Expr::Binary {
                    op: BinaryOp::Concat,
                    left: Box::new(left),
                    right: Box::new(right),
                };
            } else {
                return Ok(left);
            }
        }
    }

    fn parse_additive(&mut self) -> Result<Expr, FormulaError> {
        let mut left = self.parse_multiplicative()?;
        loop {
            self.skip_ws();
            let op = if self.consume('+') {
                BinaryOp::Add
            } else if self.consume('-') {
                BinaryOp::Sub
            } else {
                return Ok(left);
            };
            let right = self.parse_multiplicative()?;
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
    }

    fn parse_multiplicative(&mut self) -> Result<Expr, FormulaError> {
        let mut left = self.parse_power()?;
        loop {
            self.skip_ws();
            let op = if self.consume('*') {
                BinaryOp::Mul
            } else if self.consume('/') {
                BinaryOp::Div
            } else {
                return Ok(left);
            };
            let right = self.parse_power()?;
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
    }

    fn parse_power(&mut self) -> Result<Expr, FormulaError> {
        let mut left = self.parse_unary()?;
        loop {
            self.skip_ws();
            if self.consume('^') {
                let right = self.parse_unary()?;
                left = Expr::Binary {
                    op: BinaryOp::Pow,
                    left: Box::new(left),
                    right: Box::new(right),
                };
            } else {
                return Ok(left);
            }
        }
    }

    fn parse_unary(&mut self) -> Result<Expr, FormulaError> {
        self.skip_ws();
        if self.consume('-') {
            let expr = self.parse_unary()?;
            return Ok(Expr::Unary {
                op: UnaryOp::Neg,
                expr: Box::new(expr),
            });
        }
        if self.consume('+') {
            let expr = self.parse_unary()?;
            return Ok(Expr::Unary {
                op: UnaryOp::Plus,
                expr: Box::new(expr),
            });
        }
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Result<Expr, FormulaError> {
        let mut expr = self.parse_primary()?;
        loop {
            self.skip_ws();
            if self.consume('%') {
                expr = Expr::Unary {
                    op: UnaryOp::Percent,
                    expr: Box::new(expr),
                };
            } else {
                return Ok(expr);
            }
        }
    }

    fn parse_primary(&mut self) -> Result<Expr, FormulaError> {
        self.skip_ws();
        let Some(ch) = self.peek() else {
            return Err(FormulaError::Parse);
        };
        if ch == '(' {
            self.pos += 1;
            let expr = self.parse_comparison()?;
            self.skip_ws();
            if !self.consume(')') {
                return Err(FormulaError::Parse);
            }
            return Ok(expr);
        }
        if ch == '"' {
            return self.parse_string().map(Expr::Text);
        }
        if ch == '{' {
            return self.parse_array_literal();
        }
        if ch == '#' {
            return self.parse_error_literal();
        }
        if ch.is_ascii_digit() || ch == '.' {
            return self.parse_number();
        }
        if ch == '\'' {
            let sheet = self.parse_quoted_sheet_name()?;
            if !self.consume('!') {
                return Err(FormulaError::Parse);
            }
            let reference = self.parse_reference_body().ok_or(FormulaError::Parse)?;
            return Ok(Expr::Ref(RefExpr {
                sheet: Some(sheet),
                ..reference
            }));
        }
        if ch.is_ascii_alphabetic() || ch == '$' || ch == '_' {
            return self.parse_reference_or_name();
        }
        Err(FormulaError::Parse)
    }

    fn parse_string(&mut self) -> Result<String, FormulaError> {
        if !self.consume('"') {
            return Err(FormulaError::Parse);
        }
        let mut out = String::new();
        loop {
            let Some(ch) = self.peek() else {
                return Err(FormulaError::Parse);
            };
            self.pos += 1;
            if ch == '"' {
                if self.peek() == Some('"') {
                    self.pos += 1;
                    out.push('"');
                    continue;
                }
                return Ok(out);
            }
            out.push(ch);
        }
    }

    fn parse_array_literal(&mut self) -> Result<Expr, FormulaError> {
        if !self.consume('{') {
            return Err(FormulaError::Parse);
        }
        let mut rows = vec![Vec::new()];
        loop {
            self.skip_ws();
            if self.consume('}') {
                break;
            }
            let item = self.parse_comparison()?;
            rows.last_mut().expect("array row").push(item);
            self.skip_ws();
            if self.consume(',') {
                continue;
            }
            if self.consume(';') {
                rows.push(Vec::new());
                continue;
            }
            if self.consume('}') {
                break;
            }
            return Err(FormulaError::Parse);
        }
        if rows.iter().any(Vec::is_empty) {
            return Err(FormulaError::Parse);
        }
        let width = rows[0].len();
        if rows.iter().any(|row| row.len() != width) {
            return Err(FormulaError::Value);
        }
        Ok(Expr::ArrayLiteral(rows))
    }

    fn parse_error_literal(&mut self) -> Result<Expr, FormulaError> {
        let start = self.pos;
        self.pos += 1;
        while self
            .peek()
            .is_some_and(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '!' | '?'))
        {
            self.pos += 1;
        }
        let text: String = self.chars[start..self.pos].iter().collect();
        FormulaError::from_code(&text)
            .map(Expr::Error)
            .ok_or(FormulaError::Parse)
    }

    fn parse_number(&mut self) -> Result<Expr, FormulaError> {
        let start = self.pos;
        let mut saw_dot = false;
        let mut saw_exp = false;
        while let Some(ch) = self.peek() {
            if ch.is_ascii_digit() {
                self.pos += 1;
            } else if ch == '.' && !saw_dot && !saw_exp {
                saw_dot = true;
                self.pos += 1;
            } else if (ch == 'e' || ch == 'E')
                && !saw_exp
                && self.pos > start
                && self
                    .peek_at(1)
                    .is_some_and(|next| next.is_ascii_digit() || next == '+' || next == '-')
                && (self.peek_at(1).is_some_and(|next| next.is_ascii_digit())
                    || self.peek_at(2).is_some_and(|next| next.is_ascii_digit()))
            {
                saw_exp = true;
                self.pos += 2;
            } else {
                break;
            }
        }
        let text: String = self.chars[start..self.pos].iter().collect();
        // Whole-row range `1:3`.
        if !saw_dot && !saw_exp && self.peek() == Some(':') {
            let save = self.pos;
            self.pos += 1;
            let row_abs = self.consume('$');
            let row_start = self.pos;
            while self.peek().is_some_and(|ch| ch.is_ascii_digit()) {
                self.pos += 1;
            }
            let end_text: String = self.chars[row_start..self.pos].iter().collect();
            if !end_text.is_empty()
                && !self
                    .peek()
                    .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '$' || ch == '(')
            {
                if let (Ok(start_row), Ok(end_row)) = (text.parse::<u32>(), end_text.parse::<u32>())
                {
                    if start_row > 0 && end_row > 0 {
                        return Ok(Expr::Ref(RefExpr {
                            sheet: None,
                            start: CellCoord {
                                col: None,
                                row: Some(start_row),
                                col_abs: false,
                                row_abs: false,
                            },
                            end: Some(CellCoord {
                                col: None,
                                row: Some(end_row),
                                col_abs: false,
                                row_abs,
                            }),
                        }));
                    }
                }
            }
            self.pos = save;
        }
        let value = text.parse::<f64>().map_err(|_| FormulaError::Parse)?;
        if !value.is_finite() {
            return Err(FormulaError::Num);
        }
        Ok(Expr::Number(value))
    }

    fn parse_quoted_sheet_name(&mut self) -> Result<String, FormulaError> {
        if !self.consume('\'') {
            return Err(FormulaError::Parse);
        }
        let mut out = String::new();
        loop {
            let Some(ch) = self.peek() else {
                return Err(FormulaError::Parse);
            };
            self.pos += 1;
            if ch == '\'' {
                if self.peek() == Some('\'') {
                    self.pos += 1;
                    out.push('\'');
                    continue;
                }
                break;
            }
            out.push(ch);
        }
        if out.trim().is_empty() {
            return Err(FormulaError::Ref);
        }
        Ok(out)
    }

    /// Parses `$A$1`, `A1:B2`, `A:A`, `$A:$B` at the current position.
    /// Returns `None` (without consuming) when the text is not a reference.
    fn parse_reference_body(&mut self) -> Option<RefExpr> {
        let save = self.pos;
        let Some(start) = self.parse_coord() else {
            self.pos = save;
            return None;
        };
        let after_start = self.pos;
        // Whole-column start requires a `:` continuation.
        if start.row.is_none() {
            if self.peek() != Some(':') {
                self.pos = save;
                return None;
            }
            self.pos += 1;
            let Some(end) = self.parse_coord() else {
                self.pos = save;
                return None;
            };
            if end.row.is_some() || self.next_continues_identifier() {
                self.pos = save;
                return None;
            }
            return Some(RefExpr {
                sheet: None,
                start,
                end: Some(end),
            });
        }
        if self.next_continues_identifier() || self.peek() == Some('(') {
            self.pos = save;
            return None;
        }
        if self.peek() == Some(':') {
            self.pos += 1;
            self.skip_ws();
            if let Some(end) = self.parse_coord() {
                if end.row.is_some() && !self.next_continues_identifier() {
                    return Some(RefExpr {
                        sheet: None,
                        start,
                        end: Some(end),
                    });
                }
            }
            self.pos = after_start;
        }
        Some(RefExpr {
            sheet: None,
            start,
            end: None,
        })
    }

    fn next_continues_identifier(&self) -> bool {
        self.peek().is_some_and(is_name_char)
    }

    /// Parses `$?[A-Z]{1,3}($?[0-9]+)?`.
    fn parse_coord(&mut self) -> Option<CellCoord> {
        let save = self.pos;
        let col_abs = self.consume('$');
        let col_start = self.pos;
        while self.peek().is_some_and(|ch| ch.is_ascii_alphabetic()) {
            self.pos += 1;
        }
        let letters: String = self.chars[col_start..self.pos].iter().collect();
        if letters.is_empty() || letters.len() > 3 {
            self.pos = save;
            return None;
        }
        let col = column_to_number(&letters.to_ascii_uppercase())?;
        let row_abs = self.consume('$');
        let row_start = self.pos;
        while self.peek().is_some_and(|ch| ch.is_ascii_digit()) {
            self.pos += 1;
        }
        let digits: String = self.chars[row_start..self.pos].iter().collect();
        if digits.is_empty() {
            if row_abs {
                self.pos = save;
                return None;
            }
            return Some(CellCoord {
                col: Some(col),
                row: None,
                col_abs,
                row_abs: false,
            });
        }
        let row = digits.parse::<u32>().ok().filter(|row| *row > 0)?;
        Some(CellCoord {
            col: Some(col),
            row: Some(row),
            col_abs,
            row_abs,
        })
    }

    fn parse_reference_or_name(&mut self) -> Result<Expr, FormulaError> {
        // Unquoted sheet prefix.
        if self.peek().is_some_and(|ch| ch.is_ascii_alphabetic()) {
            let save = self.pos;
            while self.peek().is_some_and(is_unquoted_sheet_prefix_char) {
                self.pos += 1;
            }
            if self.peek() == Some('!') {
                let prefix: String = self.chars[save..self.pos].iter().collect();
                self.pos += 1;
                let reference = self.parse_reference_body().ok_or(FormulaError::Parse)?;
                return Ok(Expr::Ref(RefExpr {
                    sheet: Some(prefix),
                    ..reference
                }));
            }
            self.pos = save;
        }
        if let Some(reference) = self.parse_reference_body() {
            return Ok(Expr::Ref(reference));
        }
        if self.peek() == Some('$') {
            return Err(FormulaError::Parse);
        }
        let start = self.pos;
        while self.peek().is_some_and(is_name_char) {
            self.pos += 1;
        }
        let name: String = self.chars[start..self.pos].iter().collect();
        if name.is_empty() {
            return Err(FormulaError::Parse);
        }
        let upper = name.to_ascii_uppercase();
        self.skip_ws();
        if self.consume('(') {
            let args = self.parse_call_args()?;
            return Ok(Expr::Call { name: upper, args });
        }
        match upper.as_str() {
            "TRUE" => Ok(Expr::Bool(true)),
            "FALSE" => Ok(Expr::Bool(false)),
            _ => Ok(Expr::Name(upper)),
        }
    }

    fn parse_call_args(&mut self) -> Result<Vec<Expr>, FormulaError> {
        let mut args = Vec::new();
        self.skip_ws();
        if self.consume(')') {
            return Ok(args);
        }
        loop {
            self.skip_ws();
            if self.peek() == Some(',') {
                args.push(Expr::Blank);
                self.pos += 1;
                continue;
            }
            if self.peek() == Some(')') {
                args.push(Expr::Blank);
                self.pos += 1;
                return Ok(args);
            }
            let arg = self.parse_comparison()?;
            args.push(arg);
            self.skip_ws();
            if self.consume(',') {
                self.skip_ws();
                if self.peek() == Some(')') {
                    args.push(Expr::Blank);
                    self.pos += 1;
                    return Ok(args);
                }
                continue;
            }
            if self.consume(')') {
                return Ok(args);
            }
            return Err(FormulaError::Parse);
        }
    }
}

/// A reference resolved against the workbook: sheet index plus inclusive
/// 1-based bounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolvedRange {
    pub sheet: usize,
    pub col_start: u32,
    pub col_end: u32,
    pub row_start: u32,
    pub row_end: u32,
}

impl ResolvedRange {
    pub fn cell(sheet: usize, col: u32, row: u32) -> Self {
        Self {
            sheet,
            col_start: col,
            col_end: col,
            row_start: row,
            row_end: row,
        }
    }

    pub fn width(&self) -> u32 {
        self.col_end - self.col_start + 1
    }

    pub fn height(&self) -> u32 {
        self.row_end - self.row_start + 1
    }

    pub fn is_single(&self) -> bool {
        self.width() == 1 && self.height() == 1
    }

    pub fn cells(&self) -> impl Iterator<Item = (u32, u32)> + '_ {
        (self.row_start..=self.row_end)
            .flat_map(move |row| (self.col_start..=self.col_end).map(move |col| (col, row)))
    }
}

/// Workbook access needed by the evaluator.
pub trait FormulaEnv {
    /// Resolves a sheet prefix (stable id or title) to a sheet index.
    fn resolve_sheet(&self, prefix: &str) -> Option<usize>;
    /// Title used when labelling cross-sheet dependencies of named ranges.
    fn sheet_title(&self, sheet: usize) -> String;
    /// Current evaluated value of a cell (`Blank` when missing).
    fn cell_value(&self, sheet: usize, col: u32, row: u32) -> FormulaValue;
    /// Source metadata of a cell, when it exists.
    fn cell_meta(&self, sheet: usize, col: u32, row: u32) -> Option<&AppCell>;
    /// Largest populated column and row of a sheet (for whole-column/row
    /// references).
    fn sheet_extent(&self, sheet: usize) -> (u32, u32);
    /// Workbook-scoped named range lookup.
    fn named_range(&self, name: &str) -> Option<ResolvedRange>;
    /// Whether a row is hidden (for `SUBTOTAL(101..)`).
    fn row_hidden(&self, sheet: usize, row: u32) -> bool;
    /// Clock for `NOW`/`TODAY`.
    fn now_ms(&self) -> u64;
    /// Seed for `RAND`/`RANDBETWEEN`.
    fn seed(&self) -> u64;
    /// Workbook locale tag (for `TEXT`, `DOLLAR`, `FIXED`).
    fn locale(&self) -> String;
}

/// Evaluates expressions against a [`FormulaEnv`].
pub struct Evaluator<'a> {
    pub env: &'a dyn FormulaEnv,
    pub sheet: usize,
    pub col: u32,
    pub row: u32,
    pub dependencies: BTreeSet<String>,
    pub array_mode: bool,
    pub random_counter: u64,
}

impl<'a> Evaluator<'a> {
    pub fn new(env: &'a dyn FormulaEnv, sheet: usize, col: u32, row: u32) -> Self {
        Self {
            env,
            sheet,
            col,
            row,
            dependencies: BTreeSet::new(),
            array_mode: false,
            random_counter: 0,
        }
    }

    /// Resolves a reference expression to workbook bounds and records its
    /// cells as dependencies.
    pub fn resolve_ref(&mut self, reference: &RefExpr) -> Result<ResolvedRange, FormulaError> {
        let sheet = match &reference.sheet {
            Some(prefix) => self.env.resolve_sheet(prefix).ok_or(FormulaError::Ref)?,
            None => self.sheet,
        };
        let (c1, c2, r1, r2) = reference.bounds();
        let (extent_cols, extent_rows) = self.env.sheet_extent(sheet);
        let (col_start, col_end) = match (c1, c2) {
            (Some(a), Some(b)) => (a, b),
            _ => (1, extent_cols.max(1)),
        };
        let (row_start, row_end) = match (r1, r2) {
            (Some(a), Some(b)) => (a, b),
            _ => (1, extent_rows.max(1)),
        };
        let resolved = ResolvedRange {
            sheet,
            col_start,
            col_end,
            row_start,
            row_end,
        };
        self.record_dependencies(&resolved, reference.sheet.as_deref());
        Ok(resolved)
    }

    fn record_dependencies(&mut self, range: &ResolvedRange, prefix: Option<&str>) {
        let prefix = if range.sheet == self.sheet {
            None
        } else {
            Some(
                prefix
                    .map(str::to_string)
                    .unwrap_or_else(|| self.env.sheet_title(range.sheet)),
            )
        };
        for (col, row) in range.cells() {
            let address = format!("{}{row}", number_to_column(col).unwrap_or_default());
            match &prefix {
                Some(prefix) => self.dependencies.insert(format!("{prefix}!{address}")),
                None => self.dependencies.insert(address),
            };
        }
    }

    /// Resolves a named range and records its dependencies.
    pub fn resolve_name(&mut self, name: &str) -> Result<ResolvedRange, FormulaError> {
        let resolved = self.env.named_range(name).ok_or(FormulaError::Name)?;
        self.record_dependencies(&resolved, None);
        Ok(resolved)
    }

    /// Resolves an expression that must denote a range (reference or name).
    pub fn resolve_range_expr(&mut self, expr: &Expr) -> Result<ResolvedRange, FormulaError> {
        match expr {
            Expr::Ref(reference) => self.resolve_ref(reference),
            Expr::Name(name) => self.resolve_name(name),
            Expr::Call { name, args } if name == "INDIRECT" || name == "OFFSET" => {
                functions::resolve_dynamic_range(self, name, args)
            }
            _ => Err(FormulaError::Value),
        }
    }

    /// Reads a resolved range as a value (scalar for single cells).
    pub fn range_value(&self, range: &ResolvedRange) -> FormulaValue {
        if range.is_single() {
            return self
                .env
                .cell_value(range.sheet, range.col_start, range.row_start);
        }
        let mut values = Vec::with_capacity((range.width() * range.height()) as usize);
        for row in range.row_start..=range.row_end {
            for col in range.col_start..=range.col_end {
                values.push(self.env.cell_value(range.sheet, col, row));
            }
        }
        FormulaValue::Array(FormulaArray::new(
            range.height() as usize,
            range.width() as usize,
            values,
        ))
    }

    /// Reads a resolved range as an array (even for single cells).
    pub fn range_array(&self, range: &ResolvedRange) -> FormulaArray {
        match self.range_value(range) {
            FormulaValue::Array(array) => array,
            value => FormulaArray::new(1, 1, vec![value]),
        }
    }

    pub fn eval(&mut self, expr: &Expr) -> FormulaValue {
        match expr {
            Expr::Blank => FormulaValue::Blank,
            Expr::Number(value) => FormulaValue::Number(*value),
            Expr::Text(text) => FormulaValue::Text(text.clone()),
            Expr::Bool(value) => FormulaValue::Bool(*value),
            Expr::Error(error) => FormulaValue::Error(*error),
            Expr::Ref(reference) => match self.resolve_ref(reference) {
                Ok(range) => self.range_value(&range),
                Err(error) => FormulaValue::Error(error),
            },
            Expr::Name(name) => match self.resolve_name(name) {
                Ok(range) => self.range_value(&range),
                Err(error) => FormulaValue::Error(error),
            },
            Expr::ArrayLiteral(rows) => {
                let evaluated = rows
                    .iter()
                    .map(|row| row.iter().map(|item| self.eval(item).scalar()).collect())
                    .collect();
                FormulaValue::Array(FormulaArray::from_rows(evaluated))
            }
            Expr::Call { name, args } => functions::call(self, name, args),
            Expr::Unary { op, expr } => {
                let value = self.eval(expr);
                self.apply_unary(*op, value)
            }
            Expr::Binary { op, left, right } => {
                let left = self.eval(left);
                let right = self.eval(right);
                self.apply_binary(*op, left, right)
            }
        }
    }

    /// Evaluates an expression and reduces arrays to their top-left value.
    pub fn eval_scalar(&mut self, expr: &Expr) -> FormulaValue {
        self.eval(expr).scalar()
    }

    pub fn eval_number(&mut self, expr: &Expr) -> Result<f64, FormulaError> {
        self.eval_scalar(expr).to_number()
    }

    pub fn eval_text(&mut self, expr: &Expr) -> Result<String, FormulaError> {
        self.eval_scalar(expr).to_text()
    }

    pub fn eval_bool(&mut self, expr: &Expr) -> Result<bool, FormulaError> {
        self.eval_scalar(expr).to_bool()
    }

    fn apply_unary(&mut self, op: UnaryOp, value: FormulaValue) -> FormulaValue {
        if let FormulaValue::Array(array) = value {
            let values = array
                .values
                .into_iter()
                .map(|item| self.apply_unary(op, item))
                .collect();
            return FormulaValue::Array(FormulaArray::new(array.rows, array.cols, values));
        }
        let number = match value.to_number() {
            Ok(number) => number,
            Err(error) => return FormulaValue::Error(error),
        };
        match op {
            UnaryOp::Neg => FormulaValue::number(-number),
            UnaryOp::Plus => FormulaValue::number(number),
            UnaryOp::Percent => FormulaValue::number(number / 100.0),
        }
    }

    fn apply_binary(&mut self, op: BinaryOp, left: FormulaValue, right: FormulaValue) -> FormulaValue {
        match (&left, &right) {
            (FormulaValue::Array(left_array), FormulaValue::Array(right_array)) => {
                let rows = left_array.rows.max(right_array.rows);
                let cols = left_array.cols.max(right_array.cols);
                let mut values = Vec::with_capacity(rows * cols);
                for row in 0..rows {
                    for col in 0..cols {
                        let left_item = broadcast_get(left_array, row, col);
                        let right_item = broadcast_get(right_array, row, col);
                        values.push(match (left_item, right_item) {
                            (Some(left_item), Some(right_item)) => {
                                self.apply_binary(op, left_item.clone(), right_item.clone())
                            }
                            _ => FormulaValue::Error(FormulaError::Na),
                        });
                    }
                }
                FormulaValue::Array(FormulaArray::new(rows, cols, values))
            }
            (FormulaValue::Array(array), _) => {
                let values = array
                    .values
                    .iter()
                    .map(|item| self.apply_binary(op, item.clone(), right.clone()))
                    .collect();
                FormulaValue::Array(FormulaArray::new(array.rows, array.cols, values))
            }
            (_, FormulaValue::Array(array)) => {
                let values = array
                    .values
                    .iter()
                    .map(|item| self.apply_binary(op, left.clone(), item.clone()))
                    .collect();
                FormulaValue::Array(FormulaArray::new(array.rows, array.cols, values))
            }
            _ => apply_scalar_binary(op, &left, &right),
        }
    }
}

fn broadcast_get(array: &FormulaArray, row: usize, col: usize) -> Option<&FormulaValue> {
    let row = if array.rows == 1 { 0 } else { row };
    let col = if array.cols == 1 { 0 } else { col };
    if row < array.rows && col < array.cols {
        Some(array.get(row, col))
    } else {
        None
    }
}

pub fn apply_scalar_binary(op: BinaryOp, left: &FormulaValue, right: &FormulaValue) -> FormulaValue {
    match op {
        BinaryOp::Concat => {
            let left = match left.to_text() {
                Ok(text) => text,
                Err(error) => return FormulaValue::Error(error),
            };
            let right = match right.to_text() {
                Ok(text) => text,
                Err(error) => return FormulaValue::Error(error),
            };
            FormulaValue::Text(format!("{left}{right}"))
        }
        BinaryOp::Eq | BinaryOp::Ne | BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
            match compare_values(left, right) {
                Ok(ordering) => FormulaValue::Bool(match op {
                    BinaryOp::Eq => ordering.is_eq(),
                    BinaryOp::Ne => ordering.is_ne(),
                    BinaryOp::Lt => ordering.is_lt(),
                    BinaryOp::Le => ordering.is_le(),
                    BinaryOp::Gt => ordering.is_gt(),
                    BinaryOp::Ge => ordering.is_ge(),
                    _ => unreachable!(),
                }),
                Err(error) => FormulaValue::Error(error),
            }
        }
        _ => {
            let left = match left.to_number() {
                Ok(number) => number,
                Err(error) => return FormulaValue::Error(error),
            };
            let right = match right.to_number() {
                Ok(number) => number,
                Err(error) => return FormulaValue::Error(error),
            };
            match op {
                BinaryOp::Add => FormulaValue::number(left + right),
                BinaryOp::Sub => FormulaValue::number(left - right),
                BinaryOp::Mul => FormulaValue::number(left * right),
                BinaryOp::Div => {
                    if right == 0.0 {
                        FormulaValue::Error(FormulaError::Div0)
                    } else {
                        FormulaValue::number(left / right)
                    }
                }
                BinaryOp::Pow => {
                    if left == 0.0 && right < 0.0 {
                        FormulaValue::Error(FormulaError::Div0)
                    } else {
                        FormulaValue::number(left.powf(right))
                    }
                }
                _ => unreachable!(),
            }
        }
    }
}

/// Collects the dependency labels of a parsed formula without evaluating it
/// (used to build the workbook dependency graph). Labels use the same shape
/// as evaluation-time dependencies: `A1` for the same sheet and
/// `Prefix!A1` for other sheets.
pub fn static_dependencies(expr: &Expr, env: &dyn FormulaEnv, sheet: usize) -> BTreeSet<String> {
    let mut evaluator = Evaluator::new(env, sheet, 1, 1);
    let mut refs = Vec::new();
    expr.visit_refs(&mut |item| refs.push(item));
    for item in refs {
        match item {
            Expr::Ref(reference) => {
                let _ = evaluator.resolve_ref(reference);
            }
            Expr::Name(name) => {
                let _ = evaluator.resolve_name(name);
            }
            _ => {}
        }
    }
    evaluator.dependencies
}

/// Resolved cells referenced statically by a formula (for the evaluation
/// order graph).
pub fn static_ranges(expr: &Expr, env: &dyn FormulaEnv, sheet: usize) -> Vec<ResolvedRange> {
    let mut evaluator = Evaluator::new(env, sheet, 1, 1);
    let mut refs = Vec::new();
    expr.visit_refs(&mut |item| refs.push(item));
    let mut out = Vec::new();
    for item in refs {
        match item {
            Expr::Ref(reference) => {
                if let Ok(range) = evaluator.resolve_ref(reference) {
                    out.push(range);
                }
            }
            Expr::Name(name) => {
                if let Ok(range) = evaluator.resolve_name(name) {
                    out.push(range);
                }
            }
            _ => {}
        }
    }
    out
}
