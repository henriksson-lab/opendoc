//! The import/export error type.

use std::fmt;

#[derive(Debug, Eq, PartialEq)]
pub enum ImportError {
    EmptyInput,
    UnsupportedExtension(String),
    ConverterUnavailable,
    InvalidInput(String),
    InvalidDocument(String),
    UnsupportedStructure(String),
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for ImportError {}
