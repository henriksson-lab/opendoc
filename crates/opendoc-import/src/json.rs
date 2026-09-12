//! Typed accessors over untrusted Google Docs JSON.

use crate::error::ImportError;
use opendoc_core::{CitationSourceFormat, StableId};
use serde_json::Value;

pub(crate) fn required_str<'a>(
    value: &'a Value,
    key: &str,
    label: &str,
) -> Result<&'a str, ImportError> {
    value.get(key).and_then(Value::as_str).ok_or_else(|| {
        ImportError::InvalidInput(format!("{label} missing required string field {key}"))
    })
}

pub(crate) fn parse_imported_stable_id(value: &str) -> Result<StableId, ImportError> {
    StableId::parse(value.trim()).map_err(|err| ImportError::InvalidInput(err.to_string()))
}

pub(crate) fn required_array<'a>(
    value: &'a Value,
    key: &str,
    label: &str,
) -> Result<&'a Vec<Value>, ImportError> {
    value.get(key).and_then(Value::as_array).ok_or_else(|| {
        ImportError::InvalidInput(format!("{label} missing required array field {key}"))
    })
}

pub(crate) fn optional_object<'a>(
    value: &'a Value,
    key: &str,
) -> Result<Option<&'a Value>, ImportError> {
    match value.get(key) {
        Some(Value::Null) | None => Ok(None),
        Some(field) => {
            expect_object(field, key)?;
            Ok(Some(field))
        }
    }
}

pub(crate) fn optional_array<'a>(
    value: &'a Value,
    key: &str,
) -> Result<Option<&'a Vec<Value>>, ImportError> {
    match value.get(key) {
        Some(Value::Null) | None => Ok(None),
        Some(field) => field
            .as_array()
            .map(Some)
            .ok_or_else(|| ImportError::InvalidInput(format!("{key} must be an array"))),
    }
}

pub(crate) fn expect_object(value: &Value, label: &str) -> Result<(), ImportError> {
    if value.is_object() {
        Ok(())
    } else {
        Err(ImportError::InvalidInput(format!(
            "{label} must be an object"
        )))
    }
}

pub(crate) fn optional_bool(value: &Value, key: &str) -> Result<Option<bool>, ImportError> {
    match value.get(key) {
        Some(Value::Null) | None => Ok(None),
        Some(field) => field
            .as_bool()
            .map(Some)
            .ok_or_else(|| ImportError::InvalidInput(format!("{key} must be a boolean"))),
    }
}

pub(crate) fn optional_u64(value: &Value, key: &str) -> Result<Option<u64>, ImportError> {
    match value.get(key) {
        Some(Value::Null) | None => Ok(None),
        Some(field) => field.as_u64().map(Some).ok_or_else(|| {
            ImportError::InvalidInput(format!("{key} must be a non-negative integer"))
        }),
    }
}

pub(crate) fn optional_u8(value: &Value, key: &str) -> Result<Option<u8>, ImportError> {
    let Some(raw) = optional_u64(value, key)? else {
        return Ok(None);
    };
    u8::try_from(raw)
        .map(Some)
        .map_err(|_| ImportError::InvalidInput(format!("{key} is too large")))
}

pub(crate) fn optional_str<'a>(
    value: &'a Value,
    key: &str,
) -> Result<Option<&'a str>, ImportError> {
    match value.get(key) {
        Some(Value::Null) | None => Ok(None),
        Some(field) => field
            .as_str()
            .map(Some)
            .ok_or_else(|| ImportError::InvalidInput(format!("{key} must be a string"))),
    }
}

pub(crate) fn optional_checked_string(
    value: &Value,
    key: &str,
) -> Result<Option<String>, ImportError> {
    Ok(optional_str(value, key)?
        .filter(|value| !value.is_empty())
        .map(ToString::to_string))
}

pub(crate) fn optional_source_string(
    value: &Value,
    key: &str,
    label: &str,
) -> Result<Option<String>, ImportError> {
    let Some(raw) = optional_str(value, key)? else {
        return Ok(None);
    };
    source_string(raw, label).map(Some)
}

pub(crate) fn required_source_string(
    value: &Value,
    key: &str,
    required_label: &str,
    source_label: &str,
) -> Result<String, ImportError> {
    let raw = required_str(value, key, required_label)?;
    source_string(raw, source_label)
}

pub(crate) fn source_string(raw: &str, label: &str) -> Result<String, ImportError> {
    if raw.trim().is_empty() {
        return Err(ImportError::InvalidInput(format!("{label} is empty")));
    }
    if raw.trim() != raw {
        return Err(ImportError::InvalidInput(format!(
            "{label} has surrounding whitespace"
        )));
    }
    Ok(raw.to_string())
}

pub(crate) fn citation_source_format_from_label(label: &str) -> CitationSourceFormat {
    match label {
        "citum-native" => CitationSourceFormat::CitumNative,
        "csl-json" => CitationSourceFormat::CslJson,
        "bibtex" => CitationSourceFormat::Bibtex,
        "ris" => CitationSourceFormat::Ris,
        other => CitationSourceFormat::Unknown(other.to_string()),
    }
}
