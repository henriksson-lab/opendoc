//! The store error type.

use std::fmt;

#[derive(Debug)]
pub enum StoreError {
    Io(String),
    CorruptHead,
    Format(String),
    HashMismatch,
    LookupMismatch,
    InvalidPath,
    UnsupportedHash,
}

impl From<std::io::Error> for StoreError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value.to_string())
    }
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for StoreError {}
