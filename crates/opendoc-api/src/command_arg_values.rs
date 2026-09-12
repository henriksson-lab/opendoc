//! Typed accessors over a command's JSON argument object.

use crate::command_parse::CommandParseError;
use crate::{
    AppCitationItem, EditorSelection, FindOptions, OpenDocPermissionGrant, OpenDocPresencePeer,
    OpenDocRelayOperation, OpenDocRuntimeLookupEntry, OpenDocStorageBackend,
};
use opendoc_spreadsheet::{SheetFilterCriterion, SheetFilterSortSpec};
use serde_json::Value;

pub(crate) fn arg_string(args: &Value, name: &str) -> Result<String, CommandParseError> {
    args.get(name)
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .ok_or_else(|| CommandParseError::Format(format!("missing string argument {name}")))
}

pub(crate) fn arg_optional_string(
    args: &Value,
    name: &str,
) -> Result<Option<String>, CommandParseError> {
    match args.get(name) {
        Some(Value::Null) | None => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        _ => Err(CommandParseError::Format(format!(
            "argument {name} must be a string or null"
        ))),
    }
}

pub(crate) fn arg_string_vec(args: &Value, name: &str) -> Result<Vec<String>, CommandParseError> {
    let Some(values) = args.get(name).and_then(Value::as_array) else {
        return Err(CommandParseError::Format(format!(
            "missing string-array argument {name}"
        )));
    };
    values
        .iter()
        .map(|value| {
            value.as_str().map(str::to_string).ok_or_else(|| {
                CommandParseError::Format(format!(
                    "string-array argument {name} contains a non-string"
                ))
            })
        })
        .collect()
}

pub(crate) fn arg_u8(args: &Value, name: &str) -> Result<u8, CommandParseError> {
    let Some(value) = args.get(name).and_then(Value::as_u64) else {
        return Err(CommandParseError::Format(format!(
            "missing integer argument {name}"
        )));
    };
    u8::try_from(value)
        .map_err(|_| CommandParseError::Format(format!("integer argument {name} is out of range")))
}

pub(crate) fn arg_u32(args: &Value, name: &str) -> Result<u32, CommandParseError> {
    let Some(value) = args.get(name).and_then(Value::as_u64) else {
        return Err(CommandParseError::Format(format!(
            "missing integer argument {name}"
        )));
    };
    u32::try_from(value)
        .map_err(|_| CommandParseError::Format(format!("integer argument {name} is out of range")))
}

pub(crate) fn arg_optional_u32(args: &Value, name: &str) -> Result<Option<u32>, CommandParseError> {
    match args.get(name) {
        Some(Value::Null) | None => Ok(None),
        Some(value) => {
            let Some(value) = value.as_u64() else {
                return Err(CommandParseError::Format(format!(
                    "argument {name} must be a non-negative integer or null"
                )));
            };
            u32::try_from(value).map(Some).map_err(|_| {
                CommandParseError::Format(format!("integer argument {name} is out of range"))
            })
        }
    }
}

pub(crate) fn arg_i32(args: &Value, name: &str) -> Result<i32, CommandParseError> {
    let Some(value) = args.get(name).and_then(Value::as_i64) else {
        return Err(CommandParseError::Format(format!(
            "missing integer argument {name}"
        )));
    };
    i32::try_from(value)
        .map_err(|_| CommandParseError::Format(format!("integer argument {name} is out of range")))
}

pub(crate) fn arg_i8(args: &Value, name: &str) -> Result<i8, CommandParseError> {
    let Some(value) = args.get(name).and_then(Value::as_i64) else {
        return Err(CommandParseError::Format(format!(
            "missing integer argument {name}"
        )));
    };
    i8::try_from(value)
        .map_err(|_| CommandParseError::Format(format!("integer argument {name} is out of range")))
}

/// The find query and its three toggles, parsed as one payload for every
/// command that searches. Keeping it in one helper is what stops
/// `replace_all_in_document` from acquiring, say, a different default for
/// `wholeWord` than `find_in_document` has.
pub(crate) fn find_options_arg(args: &Value) -> Result<FindOptions, CommandParseError> {
    Ok(FindOptions {
        query: arg_string(args, "query")?,
        match_case: arg_bool(args, "matchCase")?,
        whole_word: arg_bool(args, "wholeWord")?,
        regex: arg_bool(args, "regex")?,
    })
}

pub(crate) fn arg_bool(args: &Value, name: &str) -> Result<bool, CommandParseError> {
    args.get(name)
        .and_then(Value::as_bool)
        .ok_or_else(|| CommandParseError::Format(format!("missing boolean argument {name}")))
}

pub(crate) fn editor_selection_arg(args: &Value) -> Result<EditorSelection, CommandParseError> {
    serde_json::from_value(
        args.get("selection")
            .cloned()
            .ok_or_else(|| CommandParseError::Format("missing selection argument".to_string()))?,
    )
    .map_err(|err| CommandParseError::Format(format!("invalid editor selection: {err}")))
}

pub(crate) fn arg_optional_bool(
    args: &Value,
    name: &str,
) -> Result<Option<bool>, CommandParseError> {
    match args.get(name) {
        Some(Value::Null) | None => Ok(None),
        Some(Value::Bool(value)) => Ok(Some(*value)),
        _ => Err(CommandParseError::Format(format!(
            "argument {name} must be a boolean or null"
        ))),
    }
}

pub(crate) fn arg_runtime_storage_backends(
    args: &Value,
    name: &str,
) -> Result<Option<Vec<OpenDocStorageBackend>>, CommandParseError> {
    let Some(values) = args.get(name) else {
        return Ok(None);
    };
    let Some(values) = values.as_array() else {
        return Err(CommandParseError::Format(format!(
            "argument {name} must be a storage-backend array"
        )));
    };
    let mut backends = Vec::new();
    for value in values {
        let Some(raw) = value.as_str() else {
            continue;
        };
        let backend = match raw.trim() {
            "local" => Some(OpenDocStorageBackend::Local),
            "flat" => Some(OpenDocStorageBackend::Flat),
            "opendal-fs" => Some(OpenDocStorageBackend::OpenDalFs),
            _ => None,
        };
        if let Some(backend) = backend {
            if !backends.contains(&backend) {
                backends.push(backend);
            }
        }
    }
    Ok(Some(backends))
}

pub(crate) fn arg_string_array(args: &Value, name: &str) -> Result<Vec<String>, CommandParseError> {
    let Some(values) = args.get(name).and_then(Value::as_array) else {
        return Err(CommandParseError::Format(format!(
            "missing string-array argument {name}"
        )));
    };
    values
        .iter()
        .map(|value| {
            value.as_str().map(ToString::to_string).ok_or_else(|| {
                CommandParseError::Format(format!(
                    "string-array argument {name} contains a non-string"
                ))
            })
        })
        .collect()
}

pub(crate) fn arg_runtime_share_actions(args: &Value, name: &str) -> Vec<String> {
    let Some(values) = args.get(name).and_then(Value::as_array) else {
        return Vec::new();
    };
    values
        .iter()
        .map(|value| match value {
            Value::String(value) => value.clone(),
            Value::Null => "null".to_string(),
            Value::Bool(value) => value.to_string(),
            Value::Number(value) => value.to_string(),
            _ => String::new(),
        })
        .collect()
}

pub(crate) fn arg_spreadsheet_cell_edits(
    args: &Value,
    name: &str,
) -> Result<Vec<(String, String)>, CommandParseError> {
    let Some(values) = args.get(name).and_then(Value::as_array) else {
        return Err(CommandParseError::Format(format!(
            "missing spreadsheet-cell-edit-array argument {name}"
        )));
    };
    values
        .iter()
        .map(|value| {
            let Some(entry) = value.as_object() else {
                return Err(CommandParseError::Format(format!(
                    "spreadsheet-cell-edit-array argument {name} contains a non-object"
                )));
            };
            let address = entry
                .get("address")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    CommandParseError::Format(format!(
                        "spreadsheet-cell-edit-array argument {name} contains an edit without a string address"
                    ))
                })?
                .to_string();
            let value = entry
                .get("value")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    CommandParseError::Format(format!(
                        "spreadsheet-cell-edit-array argument {name} contains an edit without a string value"
                    ))
                })?
                .to_string();
            Ok((address, value))
        })
        .collect()
}

pub(crate) fn arg_citation_items(
    args: &Value,
    name: &str,
) -> Result<Vec<AppCitationItem>, CommandParseError> {
    let Some(values) = args.get(name).and_then(Value::as_array) else {
        return Err(CommandParseError::Format(format!(
            "missing citation-item-array argument {name}"
        )));
    };
    values
        .iter()
        .map(|value| {
            serde_json::from_value(value.clone()).map_err(|err| {
                CommandParseError::Format(format!(
                    "invalid citation item in argument {name}: {err}"
                ))
            })
        })
        .collect()
}

pub(crate) fn arg_filter_criteria(
    args: &Value,
    name: &str,
) -> Result<Vec<SheetFilterCriterion>, CommandParseError> {
    let Some(values) = args.get(name).and_then(Value::as_array) else {
        return Err(CommandParseError::Format(format!(
            "missing filter-criteria-array argument {name}"
        )));
    };
    values
        .iter()
        .map(|value| {
            serde_json::from_value(value.clone()).map_err(|err| {
                CommandParseError::Format(format!(
                    "invalid filter criterion in argument {name}: {err}"
                ))
            })
        })
        .collect()
}

pub(crate) fn arg_filter_sort_specs(
    args: &Value,
    name: &str,
) -> Result<Vec<SheetFilterSortSpec>, CommandParseError> {
    let Some(values) = args.get(name).and_then(Value::as_array) else {
        return Err(CommandParseError::Format(format!(
            "missing filter-sort-array argument {name}"
        )));
    };
    values
        .iter()
        .map(|value| {
            serde_json::from_value(value.clone()).map_err(|err| {
                CommandParseError::Format(format!("invalid filter sort in argument {name}: {err}"))
            })
        })
        .collect()
}

pub(crate) fn arg_presence_peers(
    args: &Value,
    name: &str,
) -> Result<Vec<OpenDocPresencePeer>, CommandParseError> {
    let Some(values) = args.get(name) else {
        return Ok(Vec::new());
    };
    let Some(values) = values.as_array() else {
        return Err(CommandParseError::Format(format!(
            "argument {name} must be a presence-peer array"
        )));
    };
    Ok(values
        .iter()
        .filter_map(|value| {
            let entry = value.as_object()?;
            let subject = normalized_value_string(entry.get("subject"))?;
            Some(OpenDocPresencePeer {
                subject,
                display_name: normalized_value_string(entry.get("display_name"))
                    .unwrap_or_default(),
                role: normalized_value_string(entry.get("role")).unwrap_or_default(),
                cursor_anchor: normalized_value_string(entry.get("cursor_anchor")),
                last_seen_ms: nonnegative_integer_millis(entry.get("last_seen_ms")),
            })
        })
        .collect())
}

pub(crate) fn normalized_value_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub(crate) fn nonnegative_integer_millis(value: Option<&Value>) -> u64 {
    let Some(value) = value else {
        return 0;
    };
    if let Some(number) = value.as_u64() {
        return number;
    }
    if let Some(number) = value.as_i64() {
        return u64::try_from(number).unwrap_or(0);
    }
    value
        .as_f64()
        .filter(|number| number.is_finite() && *number > 0.0)
        .map(|number| number.trunc().min(u64::MAX as f64) as u64)
        .unwrap_or(0)
}

pub(crate) fn arg_permission_grants(
    args: &Value,
    name: &str,
) -> Result<Vec<OpenDocPermissionGrant>, CommandParseError> {
    let Some(values) = args.get(name) else {
        return Ok(Vec::new());
    };
    let Some(values) = values.as_array() else {
        return Err(CommandParseError::Format(format!(
            "argument {name} must be a permission-grant array"
        )));
    };
    Ok(values
        .iter()
        .filter_map(|value| {
            let entry = value.as_object()?;
            let subject = normalized_value_string(entry.get("subject"))?;
            let action = normalized_value_string(entry.get("action"))?;
            let scope = normalized_value_string(entry.get("scope"))?;
            Some(OpenDocPermissionGrant {
                subject,
                action,
                scope,
                document_uuid: normalized_value_string(entry.get("document_uuid")),
            })
        })
        .collect())
}

pub(crate) fn arg_relay_operations(
    args: &Value,
    name: &str,
) -> Result<Vec<OpenDocRelayOperation>, CommandParseError> {
    let Some(values) = args.get(name) else {
        return Ok(Vec::new());
    };
    let Some(values) = values.as_array() else {
        return Err(CommandParseError::Format(format!(
            "argument {name} must be a relay-operation array"
        )));
    };
    Ok(values
        .iter()
        .map(|value| {
            let Some(entry) = value.as_object() else {
                return OpenDocRelayOperation {
                    id: String::new(),
                    actor: String::new(),
                    seq: 0,
                    kind: String::new(),
                    base_manifest: None,
                };
            };
            OpenDocRelayOperation {
                id: normalized_value_string(entry.get("id")).unwrap_or_default(),
                actor: normalized_value_string(entry.get("actor")).unwrap_or_default(),
                seq: nonnegative_integer_millis(entry.get("seq")),
                kind: normalized_value_string(entry.get("kind")).unwrap_or_default(),
                base_manifest: normalized_value_string(entry.get("base_manifest")),
            }
        })
        .collect())
}

pub(crate) fn arg_runtime_lookup_entries(
    args: &Value,
    name: &str,
) -> Result<Vec<OpenDocRuntimeLookupEntry>, CommandParseError> {
    let Some(values) = args.get(name) else {
        return Ok(Vec::new());
    };
    let Some(values) = values.as_array() else {
        return Err(CommandParseError::Format(format!(
            "argument {name} must be a runtime-lookup-entry array"
        )));
    };
    Ok(values
        .iter()
        .map(|value| {
            let Some(entry) = value.as_object() else {
                return OpenDocRuntimeLookupEntry {
                    document_uuid: String::new(),
                    doi: None,
                    manifest: None,
                };
            };
            OpenDocRuntimeLookupEntry {
                document_uuid: normalized_value_string(entry.get("document_uuid"))
                    .unwrap_or_default(),
                doi: normalized_value_string(entry.get("doi")),
                manifest: normalized_value_string(entry.get("manifest")),
            }
        })
        .collect())
}

pub(crate) fn arg_u8_vec(args: &Value, name: &str) -> Result<Vec<u8>, CommandParseError> {
    let Some(values) = args.get(name).and_then(Value::as_array) else {
        return Err(CommandParseError::Format(format!(
            "missing byte-array argument {name}"
        )));
    };
    values
        .iter()
        .map(|value| {
            let Some(byte) = value.as_u64() else {
                return Err(CommandParseError::Format(format!(
                    "byte-array argument {name} contains a non-integer"
                )));
            };
            u8::try_from(byte).map_err(|_| {
                CommandParseError::Format(format!("byte-array argument {name} contains {byte}"))
            })
        })
        .collect()
}
