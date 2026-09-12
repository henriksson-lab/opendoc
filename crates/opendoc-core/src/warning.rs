//! Model warnings and errors.

use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ModelWarning {
    pub code: String,
    pub message: String,
}

impl ModelWarning {
    pub fn validate(&self) -> Result<(), ModelError> {
        if self.code.trim().is_empty() {
            return Err(ModelError::InvalidDocument("warning code is empty"));
        }
        if self.code.trim() != self.code {
            return Err(ModelError::InvalidDocument(
                "warning code has surrounding whitespace",
            ));
        }
        if self.message.trim().is_empty() {
            return Err(ModelError::InvalidDocument("warning message is empty"));
        }
        if self.message.trim() != self.message {
            return Err(ModelError::InvalidDocument(
                "warning message has surrounding whitespace",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ModelError {
    InvalidId(&'static str),
    InvalidHash,
    UnsupportedHashAlgorithm,
    InvalidDocument(&'static str),
}

impl fmt::Display for ModelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModelError::InvalidId(message) => f.write_str(message),
            ModelError::InvalidHash => f.write_str("invalid hash reference"),
            ModelError::UnsupportedHashAlgorithm => f.write_str("unsupported hash algorithm"),
            ModelError::InvalidDocument(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for ModelError {}
