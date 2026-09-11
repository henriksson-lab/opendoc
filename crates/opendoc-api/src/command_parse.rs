use crate::command_args::*;
use crate::{
    AppCitationItem, EditorSelection, OpenDocCommand, OpenDocPermissionGrant, OpenDocPresencePeer,
    OpenDocRelayOperation, OpenDocRuntimeLookupEntry, OpenDocRuntimeMode, OpenDocRuntimeProfile,
    OpenDocStorageBackend,
};
use opendoc_spreadsheet::{SheetFilterCriterion, SheetFilterSortSpec};
use serde_json::Value;

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum CommandParseError {
    Format(String),
}

impl std::fmt::Display for CommandParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Format(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for CommandParseError {}

pub(crate) fn arg_string(args: &Value, name: &str) -> Result<String, CommandParseError> {
    args.get(name)
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .ok_or_else(|| CommandParseError::Format(format!("missing string argument {name}")))
}

fn parse_runtime_mode(mode: &str) -> Result<OpenDocRuntimeMode, CommandParseError> {
    match mode {
        "tauri-local" => Ok(OpenDocRuntimeMode::TauriLocal),
        "browser-local" => Ok(OpenDocRuntimeMode::BrowserLocal),
        "hpc-single-user" => Ok(OpenDocRuntimeMode::HpcSingleUser),
        "multi-user-service" => Ok(OpenDocRuntimeMode::MultiUserService),
        other => Err(CommandParseError::Format(format!(
            "unsupported runtime mode {other}"
        ))),
    }
}

pub fn runtime_profile_from_args(args: &Value) -> Result<OpenDocRuntimeProfile, CommandParseError> {
    Ok(OpenDocRuntimeProfile::for_mode_with_capabilities(
        parse_runtime_mode(&arg_string(args, "mode")?)?,
        arg_runtime_storage_backends(args, "storageBackends")?,
        arg_optional_bool(args, "signingEnabled")?,
    ))
}

/// Gestures that extend the previous undo step when repeated quickly on the
/// same block: plain typing and single-character deletes.
pub fn undo_coalesce_key(command: &str, args: &Value) -> Option<String> {
    if command != "apply_editor_input" {
        return None;
    }
    let input_type = args.get("input_type")?.as_str()?;
    if !matches!(
        input_type,
        "insertText" | "insertCompositionText" | "deleteContentBackward" | "deleteContentForward"
    ) {
        return None;
    }
    let block_id = args
        .get("selection")?
        .get("focus")?
        .get("block_id")?
        .as_str()?;
    Some(format!("{input_type}:{block_id}"))
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

pub(crate) fn arg_i8(args: &Value, name: &str) -> Result<i8, CommandParseError> {
    let Some(value) = args.get(name).and_then(Value::as_i64) else {
        return Err(CommandParseError::Format(format!(
            "missing integer argument {name}"
        )));
    };
    i8::try_from(value)
        .map_err(|_| CommandParseError::Format(format!("integer argument {name} is out of range")))
}

pub(crate) fn arg_bool(args: &Value, name: &str) -> Result<bool, CommandParseError> {
    args.get(name)
        .and_then(Value::as_bool)
        .ok_or_else(|| CommandParseError::Format(format!("missing boolean argument {name}")))
}

fn editor_selection_arg(args: &Value) -> Result<EditorSelection, CommandParseError> {
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

fn arg_runtime_storage_backends(
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

fn normalized_value_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn nonnegative_integer_millis(value: Option<&Value>) -> u64 {
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

pub fn parse_json_command(
    command: &str,
    args: &Value,
) -> Result<Option<OpenDocCommand>, CommandParseError> {
    match command {
        "create_document" => Ok(Some(OpenDocCommand::CreateDocument(CreateDocumentArgs {
            title: arg_string(args, "title")?,
        }))),
        "close_document" => Ok(Some(OpenDocCommand::CloseDocument)),
        "undo_current_edit" => Ok(Some(OpenDocCommand::UndoCurrentEdit)),
        "redo_current_edit" => Ok(Some(OpenDocCommand::RedoCurrentEdit)),
        "get_document" => Ok(Some(OpenDocCommand::GetDocument)),
        "get_audit_view" => Ok(Some(OpenDocCommand::GetAuditView)),
        "set_document_title" => Ok(Some(OpenDocCommand::SetDocumentTitle(
            SetDocumentTitleArgs {
                title: arg_string(args, "title")?,
            },
        ))),
        "set_document_locale" => Ok(Some(OpenDocCommand::SetDocumentLocale(
            SetDocumentLocaleArgs {
                locale: arg_string(args, "locale")?,
            },
        ))),
        "add_paragraph" => Ok(Some(OpenDocCommand::AddParagraph(AddParagraphArgs {
            text: arg_string(args, "text")?,
        }))),
        "render_document_html" => Ok(Some(OpenDocCommand::RenderDocumentHtml)),
        "get_runtime_profile" => Ok(Some(OpenDocCommand::GetRuntimeProfile(
            runtime_profile_from_args(args)?,
        ))),
        "get_runtime_session" => Ok(Some(OpenDocCommand::GetRuntimeSession(
            GetRuntimeSessionArgs {
                profile: runtime_profile_from_args(args)?,
                subject: arg_optional_string(args, "subject")?,
                document_uuid: arg_optional_string(args, "documentUuid")?,
                presence: arg_presence_peers(args, "presence")?,
                permissions: arg_permission_grants(args, "permissions")?,
            },
        ))),
        "authorize_runtime_command" => Ok(Some(OpenDocCommand::AuthorizeRuntimeCommand(
            AuthorizeRuntimeCommandArgs {
                profile: runtime_profile_from_args(args)?,
                subject: arg_optional_string(args, "subject")?,
                document_uuid: arg_optional_string(args, "documentUuid")?,
                command_name: arg_string(args, "commandName")?,
                permissions: arg_permission_grants(args, "permissions")?,
            },
        ))),
        "create_runtime_share_invite" => Ok(Some(OpenDocCommand::CreateRuntimeShareInvite(
            CreateRuntimeShareInviteArgs {
                profile: runtime_profile_from_args(args)?,
                subject: arg_optional_string(args, "subject")?,
                document_uuid: arg_optional_string(args, "documentUuid")?,
                target_subject: arg_optional_string(args, "targetSubject")?,
                actions: arg_runtime_share_actions(args, "actions"),
                permissions: arg_permission_grants(args, "permissions")?,
            },
        ))),
        "relay_runtime_sync" => Ok(Some(OpenDocCommand::RelayRuntimeSync(
            RelayRuntimeSyncArgs {
                profile: runtime_profile_from_args(args)?,
                subject: arg_optional_string(args, "subject")?,
                document_uuid: arg_optional_string(args, "documentUuid")?,
                base_manifest: arg_optional_string(args, "baseManifest")?,
                operations: arg_relay_operations(args, "operations")?,
                permissions: arg_permission_grants(args, "permissions")?,
                presence: arg_presence_peers(args, "presence")?,
            },
        ))),
        "resolve_runtime_document_lookup" => Ok(Some(
            OpenDocCommand::ResolveRuntimeDocumentLookup(ResolveRuntimeDocumentLookupArgs {
                profile: runtime_profile_from_args(args)?,
                subject: arg_optional_string(args, "subject")?,
                document_uuid: arg_optional_string(args, "documentUuid")?,
                doi: arg_optional_string(args, "doi")?,
                permissions: arg_permission_grants(args, "permissions")?,
                service_index: arg_runtime_lookup_entries(args, "serviceIndex")?,
                scanned_documents: arg_runtime_lookup_entries(args, "scannedDocuments")?,
            }),
        )),
        "import_google_docs_json" => Ok(Some(OpenDocCommand::ImportGoogleDocsJson(
            ImportGoogleDocsJsonArgs {
                title: arg_string(args, "title")?,
                json_text: arg_string(args, "jsonText")?,
            },
        ))),
        "import_doc_or_docx_path" => Ok(Some(OpenDocCommand::ImportDocOrDocxPath(
            ImportDocOrDocxPathArgs {
                path: arg_string(args, "path")?,
            },
        ))),
        "export_google_docs_json" => Ok(Some(OpenDocCommand::ExportGoogleDocsJson)),
        "import_google_sheets_json" => Ok(Some(OpenDocCommand::ImportGoogleSheetsJson(
            ImportGoogleSheetsJsonArgs {
                json_text: arg_string(args, "jsonText")?,
            },
        ))),
        "export_google_sheets_json" => Ok(Some(OpenDocCommand::ExportGoogleSheetsJson)),
        "render_workbook_html" => Ok(Some(OpenDocCommand::RenderWorkbookHtml(
            RenderWorkbookHtmlArgs {
                sheet_id: arg_string(args, "sheetId")?,
            },
        ))),
        "import_docx_base64" => Ok(Some(OpenDocCommand::ImportDocxBase64(
            ImportDocxBase64Args {
                name: arg_string(args, "name")?,
                base64: arg_string(args, "base64")?,
            },
        ))),
        "add_binary_blob" => Ok(Some(OpenDocCommand::AddBinaryBlob(AddBinaryBlobArgs {
            name: arg_string(args, "name")?,
            media_type: arg_string(args, "mediaType")?,
            bytes: arg_u8_vec(args, "bytes")?,
        }))),
        "simulate_shallow_clone" => Ok(Some(OpenDocCommand::SimulateShallowClone)),
        "update_binary_blob_metadata" => Ok(Some(OpenDocCommand::UpdateBinaryBlobMetadata(
            UpdateBinaryBlobMetadataArgs {
                blob_hash: arg_string(args, "blobHash")?,
                name: arg_string(args, "name")?,
                media_type: arg_string(args, "mediaType")?,
            },
        ))),
        "delete_binary_blob" => Ok(Some(OpenDocCommand::DeleteBinaryBlob(
            DeleteBinaryBlobArgs {
                blob_hash: arg_string(args, "blobHash")?,
            },
        ))),
        "restore_binary_blob" => Ok(Some(OpenDocCommand::RestoreBinaryBlob(
            RestoreBinaryBlobArgs {
                blob_hash: arg_string(args, "blobHash")?,
            },
        ))),
        "record_blob_archive_tombstone" => Ok(Some(OpenDocCommand::RecordBlobArchiveTombstone(
            RecordBlobArchiveTombstoneArgs {
                blob_hash: arg_string(args, "blobHash")?,
                archive_locator: arg_string(args, "archiveLocator")?,
                restore_hint: arg_string(args, "restoreHint")?,
                signer: arg_string(args, "signer")?,
                signature: arg_u8_vec(args, "signature")?,
            },
        ))),
        "add_image_block" => Ok(Some(OpenDocCommand::AddImageBlock(AddImageBlockArgs {
            blob_hash: arg_string(args, "blobHash")?,
            alt_text: arg_string(args, "altText")?,
        }))),
        "insert_image_block_after" => Ok(Some(OpenDocCommand::InsertImageBlockAfter(
            InsertImageBlockAfterArgs {
                after_block_id: arg_string(args, "afterBlockId")?,
                blob_hash: arg_string(args, "blobHash")?,
                alt_text: arg_string(args, "altText")?,
            },
        ))),
        "sign_blob_with_openssh_private_key" => Ok(Some(
            OpenDocCommand::SignBlobWithOpenSshPrivateKey(SignBlobWithOpenSshPrivateKeyArgs {
                blob_hash: arg_string(args, "blobHash")?,
                private_key_pem: arg_string(args, "privateKeyPem")?,
                signer_display: arg_string(args, "signerDisplay")?,
            }),
        )),
        "sign_fastq_blob_with_openssh_private_key" => {
            Ok(Some(OpenDocCommand::SignFastqBlobWithOpenSshPrivateKey(
                SignFastqBlobWithOpenSshPrivateKeyArgs {
                    blob_hash: arg_string(args, "blobHash")?,
                    profile: arg_string(args, "profile")?,
                    private_key_pem: arg_string(args, "privateKeyPem")?,
                    signer_display: arg_string(args, "signerDisplay")?,
                },
            )))
        }
        "sign_image_pixels_blob_with_openssh_private_key" => Ok(Some(
            OpenDocCommand::SignImagePixelsBlobWithOpenSshPrivateKey(
                SignImagePixelsBlobWithOpenSshPrivateKeyArgs {
                    blob_hash: arg_string(args, "blobHash")?,
                    width: arg_u32(args, "width")?,
                    height: arg_u32(args, "height")?,
                    pixels: arg_u8_vec(args, "pixels")?,
                    private_key_pem: arg_string(args, "privateKeyPem")?,
                    signer_display: arg_string(args, "signerDisplay")?,
                },
            ),
        )),
        "sign_with_openssh_private_key" => Ok(Some(OpenDocCommand::SignWithOpenSshPrivateKey(
            SignWithOpenSshPrivateKeyArgs {
                private_key_pem: arg_string(args, "privateKeyPem")?,
                signer_display: arg_string(args, "signerDisplay")?,
            },
        ))),
        "verify_current_signature" => Ok(Some(OpenDocCommand::VerifyCurrentSignature(
            VerifyCurrentSignatureArgs {
                private_key_pem: arg_string(args, "privateKeyPem")?,
            },
        ))),
        "verify_current_signatures" => Ok(Some(OpenDocCommand::VerifyCurrentSignatures)),
        "save_local_repository" => Ok(Some(OpenDocCommand::SaveLocalRepository(repository_path(
            args,
        )?))),
        "save_local_repository_or_candidate" => Ok(Some(
            OpenDocCommand::SaveLocalRepositoryOrCandidate(repository_path(args)?),
        )),
        "save_flat_repository" => Ok(Some(OpenDocCommand::SaveFlatRepository(
            repository_namespace(args)?,
        ))),
        "save_flat_repository_or_candidate" => Ok(Some(
            OpenDocCommand::SaveFlatRepositoryOrCandidate(repository_namespace(args)?),
        )),
        "save_opendal_fs_repository" => Ok(Some(OpenDocCommand::SaveOpenDalFsRepository(
            repository_namespace(args)?,
        ))),
        "save_opendal_fs_repository_or_candidate" => Ok(Some(
            OpenDocCommand::SaveOpenDalFsRepositoryOrCandidate(repository_namespace(args)?),
        )),
        "autosave_current_repository" => Ok(Some(OpenDocCommand::AutosaveCurrentRepository)),
        "compact_local_repository" => Ok(Some(OpenDocCommand::CompactLocalRepository(
            CompactLocalRepositoryArgs {
                path: arg_string(args, "path")?,
                pack_name: arg_string(args, "packName")?,
            },
        ))),
        "open_local_repository" => Ok(Some(OpenDocCommand::OpenLocalRepository(
            repository_document(args)?,
        ))),
        "scan_local_repository" => Ok(Some(OpenDocCommand::ScanLocalRepository(repository_path(
            args,
        )?))),
        "open_flat_repository" => Ok(Some(OpenDocCommand::OpenFlatRepository(
            repository_namespace_document(args)?,
        ))),
        "open_opendal_fs_repository" => Ok(Some(OpenDocCommand::OpenOpenDalFsRepository(
            repository_namespace_document(args)?,
        ))),
        "merge_local_repository_candidates" => Ok(Some(
            OpenDocCommand::MergeLocalRepositoryCandidates(repository_document(args)?),
        )),
        "merge_flat_repository_candidates" => Ok(Some(
            OpenDocCommand::MergeFlatRepositoryCandidates(repository_namespace_document(args)?),
        )),
        "merge_opendal_fs_repository_candidates" => {
            Ok(Some(OpenDocCommand::MergeOpenDalFsRepositoryCandidates(
                repository_namespace_document(args)?,
            )))
        }
        "open_local_repository_by_doi" => Ok(Some(OpenDocCommand::OpenLocalRepositoryByDoi(
            repository_doi(args)?,
        ))),
        "open_flat_repository_by_doi" => Ok(Some(OpenDocCommand::OpenFlatRepositoryByDoi(
            repository_namespace_doi(args)?,
        ))),
        "open_opendal_fs_repository_by_doi" => Ok(Some(
            OpenDocCommand::OpenOpenDalFsRepositoryByDoi(repository_namespace_doi(args)?),
        )),
        "set_document_doi" => Ok(Some(OpenDocCommand::SetDocumentDoi(SetDocumentDoiArgs {
            doi: arg_string(args, "doi")?,
        }))),
        "insert_paragraph_after" => Ok(Some(OpenDocCommand::InsertParagraphAfter(
            InsertParagraphAfterArgs {
                after_block_id: arg_optional_string(args, "afterBlockId")?,
                text: arg_string(args, "text")?,
            },
        ))),
        "split_paragraph_at_inline" => Ok(Some(OpenDocCommand::SplitParagraphAtInline(
            SplitParagraphAtInlineArgs {
                inline_id: arg_string(args, "inlineId")?,
            },
        ))),
        "split_paragraph_at_text_offset" => Ok(Some(OpenDocCommand::SplitParagraphAtTextOffset(
            SplitParagraphAtTextOffsetArgs {
                block_id: arg_string(args, "blockId")?,
                inline_id: arg_string(args, "inlineId")?,
                offset: arg_u32(args, "offset")? as usize,
            },
        ))),
        "join_paragraph_with_previous" => Ok(Some(OpenDocCommand::JoinParagraphWithPrevious(
            block_id_args(args)?,
        ))),
        "delete_block" => Ok(Some(OpenDocCommand::DeleteBlock(block_id_args(args)?))),
        "set_block_text_style" => Ok(Some(OpenDocCommand::SetBlockTextStyle(
            SetBlockTextStyleArgs {
                block_id: arg_string(args, "blockId")?,
                style: arg_string(args, "style")?,
                level: arg_u8(args, "level")?,
                ordered: arg_bool(args, "ordered")?,
            },
        ))),
        "set_editor_selection_block_style" => Ok(Some(
            OpenDocCommand::SetEditorSelectionBlockStyle(SetEditorSelectionBlockStyleArgs {
                selection: editor_selection_arg(args)?,
                style: arg_string(args, "style")?,
                level: arg_u8(args, "level")?,
                ordered: arg_bool(args, "ordered")?,
            }),
        )),
        "add_heading" => Ok(Some(OpenDocCommand::AddHeading(AddHeadingArgs {
            text: arg_string(args, "text")?,
            level: arg_u8(args, "level")?,
        }))),
        "update_heading_level" => Ok(Some(OpenDocCommand::UpdateHeadingLevel(
            UpdateHeadingLevelArgs {
                block_id: arg_string(args, "blockId")?,
                level: arg_u8(args, "level")?,
            },
        ))),
        "add_link" => Ok(Some(OpenDocCommand::AddLink(AddLinkArgs {
            text: arg_string(args, "text")?,
            href: arg_string(args, "href")?,
        }))),
        "insert_link_after" => Ok(Some(OpenDocCommand::InsertLinkAfter(InsertLinkAfterArgs {
            block_id: arg_string(args, "blockId")?,
            after_inline_id: arg_optional_string(args, "afterInlineId")?,
            text: arg_string(args, "text")?,
            href: arg_string(args, "href")?,
        }))),
        "add_mention" => Ok(Some(OpenDocCommand::AddMention(AddMentionArgs {
            label: arg_string(args, "label")?,
        }))),
        "insert_mention_after" => Ok(Some(OpenDocCommand::InsertMentionAfter(
            InsertMentionAfterArgs {
                block_id: arg_string(args, "blockId")?,
                after_inline_id: arg_optional_string(args, "afterInlineId")?,
                label: arg_string(args, "label")?,
            },
        ))),
        "add_footnote_ref" => Ok(Some(OpenDocCommand::AddFootnoteRef)),
        "insert_footnote_ref_after" => Ok(Some(OpenDocCommand::InsertFootnoteRefAfter(
            InsertFootnoteRefAfterArgs {
                block_id: arg_string(args, "blockId")?,
                after_inline_id: arg_optional_string(args, "afterInlineId")?,
            },
        ))),
        "update_footnote_body" => Ok(Some(OpenDocCommand::UpdateFootnoteBody(
            UpdateFootnoteBodyArgs {
                footnote_id: arg_string(args, "footnoteId")?,
                body: arg_string(args, "body")?,
            },
        ))),
        "add_equation" => Ok(Some(OpenDocCommand::AddEquation(AddEquationArgs {
            source: arg_string(args, "source")?,
        }))),
        "insert_equation_after" => Ok(Some(OpenDocCommand::InsertEquationAfter(
            InsertEquationAfterArgs {
                block_id: arg_string(args, "blockId")?,
                after_inline_id: arg_optional_string(args, "afterInlineId")?,
                source: arg_string(args, "source")?,
            },
        ))),
        "add_equation_block" => Ok(Some(OpenDocCommand::AddEquationBlock(AddEquationArgs {
            source: arg_string(args, "source")?,
        }))),
        "insert_equation_block_after" => Ok(Some(OpenDocCommand::InsertEquationBlockAfter(
            InsertEquationBlockAfterArgs {
                after_block_id: arg_string(args, "afterBlockId")?,
                source: arg_string(args, "source")?,
            },
        ))),
        "add_list_item" => Ok(Some(OpenDocCommand::AddListItem(AddListItemArgs {
            text: arg_string(args, "text")?,
            level: arg_u8(args, "level")?,
            ordered: arg_bool(args, "ordered")?,
        }))),
        "insert_list_item_after" => Ok(Some(OpenDocCommand::InsertListItemAfter(
            InsertListItemAfterArgs {
                after_block_id: arg_string(args, "afterBlockId")?,
                text: arg_string(args, "text")?,
                level: arg_u8(args, "level")?,
                ordered: arg_bool(args, "ordered")?,
            },
        ))),
        "update_list_item" => Ok(Some(OpenDocCommand::UpdateListItem(UpdateListItemArgs {
            block_id: arg_string(args, "blockId")?,
            level: arg_u8(args, "level")?,
            ordered: arg_bool(args, "ordered")?,
        }))),
        "adjust_editor_selection_list_indent" => Ok(Some(
            OpenDocCommand::AdjustEditorSelectionListIndent(AdjustEditorSelectionListIndentArgs {
                selection: editor_selection_arg(args)?,
                delta: arg_i8(args, "delta")?,
            }),
        )),
        "insert_page_break_after" => Ok(Some(OpenDocCommand::InsertPageBreakAfter(
            after_block_args(args)?,
        ))),
        "add_page_break" => Ok(Some(OpenDocCommand::AddPageBreak)),
        "insert_table_after" => Ok(Some(OpenDocCommand::InsertTableAfter(
            insert_table_after_args(args)?,
        ))),
        "add_table" => Ok(Some(OpenDocCommand::AddTable)),
        "add_table_row" => Ok(Some(OpenDocCommand::AddTableRow(AddTableRowArgs {
            table_block_id: arg_string(args, "tableBlockId")?,
            after_row: arg_optional_string(args, "afterRow")?,
            text: arg_string(args, "text")?,
        }))),
        "delete_table_row" => Ok(Some(OpenDocCommand::DeleteTableRow(DeleteTableRowArgs {
            table_block_id: arg_string(args, "tableBlockId")?,
            row_id: arg_string(args, "rowId")?,
        }))),
        "add_table_cell" => Ok(Some(OpenDocCommand::AddTableCell(AddTableCellArgs {
            table_block_id: arg_string(args, "tableBlockId")?,
            row_id: arg_string(args, "rowId")?,
            after_cell: arg_optional_string(args, "afterCell")?,
            text: arg_string(args, "text")?,
        }))),
        "delete_table_cell" => Ok(Some(OpenDocCommand::DeleteTableCell(DeleteTableCellArgs {
            table_block_id: arg_string(args, "tableBlockId")?,
            row_id: arg_string(args, "rowId")?,
            cell_id: arg_string(args, "cellId")?,
        }))),
        "add_citation" => Ok(Some(OpenDocCommand::AddCitation)),
        "insert_citation" => Ok(Some(OpenDocCommand::InsertCitation(InsertCitationArgs {
            reference_id: arg_string(args, "referenceId")?,
            after_inline_id: arg_optional_string(args, "afterInlineId")?,
            locator: arg_optional_string(args, "locator")?,
            label: arg_optional_string(args, "label")?,
            prefix: arg_optional_string(args, "prefix")?,
            suffix: arg_optional_string(args, "suffix")?,
            suppress_author: arg_bool(args, "suppressAuthor")?,
        }))),
        "insert_citation_group" => Ok(Some(OpenDocCommand::InsertCitationGroup(
            InsertCitationGroupArgs {
                items: arg_citation_items(args, "items")?,
                after_inline_id: arg_optional_string(args, "afterInlineId")?,
            },
        ))),
        "insert_footnote_citation_group" => Ok(Some(OpenDocCommand::InsertFootnoteCitationGroup(
            InsertFootnoteCitationGroupArgs {
                footnote_id: arg_string(args, "footnoteId")?,
                items: arg_citation_items(args, "items")?,
            },
        ))),
        "insert_footnote_citation_after" => Ok(Some(OpenDocCommand::InsertFootnoteCitationAfter(
            InsertFootnoteCitationAfterArgs {
                block_id: arg_string(args, "blockId")?,
                after_inline_id: arg_optional_string(args, "afterInlineId")?,
                items: arg_citation_items(args, "items")?,
            },
        ))),
        "update_citation_group_items" => Ok(Some(OpenDocCommand::UpdateCitationGroupItems(
            UpdateCitationGroupItemsArgs {
                citation_id: arg_string(args, "citationId")?,
                items: arg_citation_items(args, "items")?,
            },
        ))),
        "set_citation_style" => Ok(Some(OpenDocCommand::SetCitationStyle(
            SetCitationStyleArgs {
                style: arg_string(args, "style")?,
                locale: arg_string(args, "locale")?,
            },
        ))),
        "add_comment" => Ok(Some(OpenDocCommand::AddComment(author_body_args(args)?))),
        "add_text_range_comment" => Ok(Some(OpenDocCommand::AddTextRangeComment(
            text_range_author_body_args(args)?,
        ))),
        "add_block_comment" => Ok(Some(OpenDocCommand::AddBlockComment(
            block_author_body_args(args)?,
        ))),
        "add_comment_reply" => Ok(Some(OpenDocCommand::AddCommentReply(
            thread_author_body_args(args)?,
        ))),
        "delete_comment_thread" => Ok(Some(OpenDocCommand::DeleteCommentThread(thread_id_args(
            args,
        )?))),
        "restore_comment_thread" => Ok(Some(OpenDocCommand::RestoreCommentThread(thread_id_args(
            args,
        )?))),
        "delete_comment" => Ok(Some(OpenDocCommand::DeleteComment(thread_comment_id_args(
            args,
        )?))),
        "restore_comment" => Ok(Some(OpenDocCommand::RestoreComment(
            thread_comment_id_args(args)?,
        ))),
        "update_comment" => Ok(Some(OpenDocCommand::UpdateComment(UpdateCommentArgs {
            thread_id: arg_string(args, "threadId")?,
            comment_id: arg_string(args, "commentId")?,
            body: arg_string(args, "body")?,
        }))),
        "add_suggestion" => Ok(Some(OpenDocCommand::AddSuggestion(author_text_args(args)?))),
        "add_text_range_suggestion" => Ok(Some(OpenDocCommand::AddTextRangeSuggestion(
            text_range_author_text_args(args)?,
        ))),
        "add_block_suggestion" => Ok(Some(OpenDocCommand::AddBlockSuggestion(
            block_author_text_args(args)?,
        ))),
        "add_delete_suggestion" => Ok(Some(OpenDocCommand::AddDeleteSuggestion(
            DeleteSuggestionArgs {
                author: arg_string(args, "author")?,
                inline_id: arg_string(args, "inlineId")?,
            },
        ))),
        "add_text_range_delete_suggestion" => Ok(Some(
            OpenDocCommand::AddTextRangeDeleteSuggestion(text_range_author_args(args)?),
        )),
        "add_format_suggestion" => Ok(Some(OpenDocCommand::AddFormatSuggestion(
            format_suggestion_args(args)?,
        ))),
        "add_text_range_format_suggestion" => Ok(Some(
            OpenDocCommand::AddTextRangeFormatSuggestion(TextRangeFormatSuggestionArgs {
                start_inline_id: arg_string(args, "startInlineId")?,
                end_inline_id: arg_string(args, "endInlineId")?,
                author: arg_string(args, "author")?,
                mark_kind: arg_string(args, "markKind")?,
                value: arg_optional_string(args, "value")?,
            }),
        )),
        "update_suggestion" => Ok(Some(OpenDocCommand::UpdateSuggestion(
            UpdateSuggestionArgs {
                suggestion_id: arg_string(args, "suggestionId")?,
                text: arg_string(args, "text")?,
            },
        ))),
        "accept_suggestion" => Ok(Some(OpenDocCommand::AcceptSuggestion(
            AcceptSuggestionArgs {
                suggestion_id: arg_string(args, "suggestionId")?,
                accepted_by: arg_string(args, "acceptedBy")?,
            },
        ))),
        "accept_all_suggestions" => Ok(Some(OpenDocCommand::AcceptAllSuggestions(
            AcceptAllSuggestionsArgs {
                accepted_by: arg_string(args, "acceptedBy")?,
            },
        ))),
        "reject_suggestion" => Ok(Some(OpenDocCommand::RejectSuggestion(
            RejectSuggestionArgs {
                suggestion_id: arg_string(args, "suggestionId")?,
                rejected_by: arg_string(args, "rejectedBy")?,
            },
        ))),
        "reject_all_suggestions" => Ok(Some(OpenDocCommand::RejectAllSuggestions(
            RejectAllSuggestionsArgs {
                rejected_by: arg_string(args, "rejectedBy")?,
            },
        ))),
        "describe_editor_selection" => Ok(Some(OpenDocCommand::DescribeEditorSelection(
            editor_selection_arg(args)?,
        ))),
        "select_all_editor_content" => Ok(Some(OpenDocCommand::SelectAllEditorContent)),
        "apply_editor_input" => Ok(Some(OpenDocCommand::ApplyEditorInput(
            serde_json::from_value(args.clone())
                .map_err(|err| CommandParseError::Format(format!("invalid editor input: {err}")))?,
        ))),
        "apply_editor_mark" => Ok(Some(OpenDocCommand::ApplyEditorMark(
            serde_json::from_value(args.clone()).map_err(|err| {
                CommandParseError::Format(format!("invalid editor mark input: {err}"))
            })?,
        ))),
        "add_bibliography_reference" => Ok(Some(OpenDocCommand::AddBibliographyReference(
            bibliography_reference_metadata_args(args)?,
        ))),
        "update_bibliography_reference" => Ok(Some(OpenDocCommand::UpdateBibliographyReference(
            UpdateBibliographyReferenceArgs {
                reference_id: arg_string(args, "referenceId")?,
                title: arg_string(args, "title")?,
                issued: arg_optional_string(args, "issued")?,
            },
        ))),
        "update_bibliography_reference_metadata" => {
            Ok(Some(OpenDocCommand::UpdateBibliographyReferenceMetadata(
                UpdateBibliographyReferenceMetadataArgs {
                    reference_id: arg_string(args, "referenceId")?,
                    metadata: bibliography_reference_metadata_args(args)?,
                },
            )))
        }
        "delete_bibliography_reference" => Ok(Some(OpenDocCommand::DeleteBibliographyReference(
            reference_id_args(args)?,
        ))),
        "restore_bibliography_reference" => Ok(Some(OpenDocCommand::RestoreBibliographyReference(
            reference_id_args(args)?,
        ))),
        "delete_citation_group" => Ok(Some(OpenDocCommand::DeleteCitationGroup(citation_id_args(
            args,
        )?))),
        "restore_citation_group" => Ok(Some(OpenDocCommand::RestoreCitationGroup(
            citation_id_args(args)?,
        ))),
        "update_inline_text" => Ok(Some(OpenDocCommand::UpdateInlineText(InlineTextArgs {
            inline_id: arg_string(args, "inlineId")?,
            text: arg_string(args, "text")?,
        }))),
        "update_inline_equation_source" => Ok(Some(OpenDocCommand::UpdateInlineEquationSource(
            InlineSourceArgs {
                inline_id: arg_string(args, "inlineId")?,
                source: arg_string(args, "source")?,
            },
        ))),
        "update_mention_label" => Ok(Some(OpenDocCommand::UpdateMentionLabel(InlineLabelArgs {
            inline_id: arg_string(args, "inlineId")?,
            label: arg_string(args, "label")?,
        }))),
        "update_link_href" => Ok(Some(OpenDocCommand::UpdateLinkHref(InlineHrefArgs {
            inline_id: arg_string(args, "inlineId")?,
            href: arg_string(args, "href")?,
        }))),
        "insert_inline_text" => Ok(Some(OpenDocCommand::InsertInlineText(
            InsertInlineTextArgs {
                block_id: arg_string(args, "blockId")?,
                after_inline_id: arg_optional_string(args, "afterInlineId")?,
                text: arg_string(args, "text")?,
            },
        ))),
        "delete_inline" => Ok(Some(OpenDocCommand::DeleteInline(inline_id_args(args)?))),
        "add_text_mark" => Ok(Some(OpenDocCommand::AddTextMark(text_mark_args(args)?))),
        "add_text_mark_range" => Ok(Some(OpenDocCommand::AddTextMarkRange(
            text_mark_range_args(args)?,
        ))),
        "remove_text_mark" => Ok(Some(OpenDocCommand::RemoveTextMark(text_mark_args(args)?))),
        "remove_text_mark_range" => Ok(Some(OpenDocCommand::RemoveTextMarkRange(
            text_mark_range_args(args)?,
        ))),
        "update_block_equation_source" => Ok(Some(OpenDocCommand::UpdateBlockEquationSource(
            BlockSourceArgs {
                block_id: arg_string(args, "blockId")?,
                source: arg_string(args, "source")?,
            },
        ))),
        "update_image_alt_text" => Ok(Some(OpenDocCommand::UpdateImageAltText(BlockAltTextArgs {
            block_id: arg_string(args, "blockId")?,
            alt_text: arg_string(args, "altText")?,
        }))),
        "update_image_blob_hash" => Ok(Some(OpenDocCommand::UpdateImageBlobHash(
            BlockBlobHashArgs {
                block_id: arg_string(args, "blockId")?,
                blob_hash: arg_string(args, "blobHash")?,
            },
        ))),
        "describe_spreadsheet_selection" => Ok(Some(OpenDocCommand::DescribeSpreadsheetSelection(
            spreadsheet_selection_args(args)?,
        ))),
        "reduce_spreadsheet_selection" => Ok(Some(OpenDocCommand::ReduceSpreadsheetSelection(
            SpreadsheetSelectionActionArgs {
                sheet_id: arg_string(args, "sheetId")?,
                anchor: arg_string(args, "anchor")?,
                focus: arg_string(args, "focus")?,
                action: arg_string(args, "action")?,
                value: arg_string(args, "value")?,
                extend: arg_bool(args, "extend")?,
            },
        ))),
        "copy_spreadsheet_selection_tsv" => Ok(Some(OpenDocCommand::CopySpreadsheetSelectionTsv(
            spreadsheet_selection_args(args)?,
        ))),
        "paste_spreadsheet_tsv" => Ok(Some(OpenDocCommand::PasteSpreadsheetTsv(
            SpreadsheetPasteTsvArgs {
                sheet_id: arg_string(args, "sheetId")?,
                origin: arg_string(args, "origin")?,
                text: arg_string(args, "text")?,
            },
        ))),
        "clear_spreadsheet_selection" => Ok(Some(OpenDocCommand::ClearSpreadsheetSelection(
            spreadsheet_selection_args(args)?,
        ))),
        "set_spreadsheet_selection_format" => Ok(Some(
            OpenDocCommand::SetSpreadsheetSelectionFormat(SpreadsheetSelectionFormatArgs {
                sheet_id: arg_string(args, "sheetId")?,
                anchor: arg_string(args, "anchor")?,
                focus: arg_string(args, "focus")?,
                property: arg_string(args, "property")?,
                value: arg_string(args, "value")?,
            }),
        )),
        "add_spreadsheet_row_after_selection" => Ok(Some(
            OpenDocCommand::AddSpreadsheetRowAfterSelection(spreadsheet_selection_args(args)?),
        )),
        "add_spreadsheet_column_after_selection" => Ok(Some(
            OpenDocCommand::AddSpreadsheetColumnAfterSelection(spreadsheet_selection_args(args)?),
        )),
        "delete_spreadsheet_selection_row" => Ok(Some(
            OpenDocCommand::DeleteSpreadsheetSelectionRow(spreadsheet_selection_args(args)?),
        )),
        "delete_spreadsheet_selection_column" => Ok(Some(
            OpenDocCommand::DeleteSpreadsheetSelectionColumn(spreadsheet_selection_args(args)?),
        )),
        "merge_spreadsheet_selection" => Ok(Some(OpenDocCommand::MergeSpreadsheetSelection(
            spreadsheet_selection_args(args)?,
        ))),
        "freeze_spreadsheet_selection" => Ok(Some(OpenDocCommand::FreezeSpreadsheetSelection(
            spreadsheet_selection_args(args)?,
        ))),
        "set_spreadsheet_selection_filter" => Ok(Some(
            OpenDocCommand::SetSpreadsheetSelectionFilter(spreadsheet_selection_args(args)?),
        )),
        "set_spreadsheet_workbook_metadata" => Ok(Some(
            OpenDocCommand::SetSpreadsheetWorkbookMetadata(SpreadsheetWorkbookMetadataArgs {
                title: arg_string(args, "title")?,
                locale: arg_string(args, "locale")?,
                timezone: arg_string(args, "timezone")?,
            }),
        )),
        "set_spreadsheet_cell" => Ok(Some(OpenDocCommand::SetSpreadsheetCell(
            SpreadsheetCellArgs {
                address: arg_string(args, "address")?,
                value: arg_string(args, "value")?,
            },
        ))),
        "set_spreadsheet_cells" => Ok(Some(OpenDocCommand::SetSpreadsheetCells(
            SpreadsheetCellEditsArgs {
                cells: arg_spreadsheet_cell_edits(args, "cells")?,
            },
        ))),
        "add_spreadsheet_sheet" => Ok(Some(OpenDocCommand::AddSpreadsheetSheet(SheetTitleArgs {
            title: arg_string(args, "title")?,
        }))),
        "rename_spreadsheet_sheet" => Ok(Some(OpenDocCommand::RenameSpreadsheetSheet(
            sheet_rename_args(args)?,
        ))),
        "delete_spreadsheet_sheet" => Ok(Some(OpenDocCommand::DeleteSpreadsheetSheet(
            sheet_id_args(args)?,
        ))),
        "restore_spreadsheet_sheet" => Ok(Some(OpenDocCommand::RestoreSpreadsheetSheet(
            sheet_id_args(args)?,
        ))),
        "add_spreadsheet_row" => Ok(Some(OpenDocCommand::AddSpreadsheetRow(sheet_row_args(
            args,
        )?))),
        "delete_spreadsheet_row" => Ok(Some(OpenDocCommand::DeleteSpreadsheetRow(sheet_row_args(
            args,
        )?))),
        "restore_spreadsheet_row" => Ok(Some(OpenDocCommand::RestoreSpreadsheetRow(
            sheet_row_args(args)?,
        ))),
        "add_spreadsheet_column" => Ok(Some(OpenDocCommand::AddSpreadsheetColumn(
            sheet_column_args(args)?,
        ))),
        "delete_spreadsheet_column" => Ok(Some(OpenDocCommand::DeleteSpreadsheetColumn(
            sheet_column_args(args)?,
        ))),
        "restore_spreadsheet_column" => Ok(Some(OpenDocCommand::RestoreSpreadsheetColumn(
            sheet_column_args(args)?,
        ))),
        "add_spreadsheet_cell_comment" => Ok(Some(OpenDocCommand::AddSpreadsheetCellComment(
            CellCommentCreateArgs {
                sheet_id: arg_string(args, "sheetId")?,
                address: arg_string(args, "address")?,
                author: arg_string(args, "author")?,
                body: arg_string(args, "body")?,
            },
        ))),
        "update_spreadsheet_cell_comment" => Ok(Some(
            OpenDocCommand::UpdateSpreadsheetCellComment(CellCommentUpdateArgs {
                comment_id: arg_string(args, "commentId")?,
                body: arg_string(args, "body")?,
            }),
        )),
        "delete_spreadsheet_cell_comment" => Ok(Some(
            OpenDocCommand::DeleteSpreadsheetCellComment(cell_comment_id_args(args)?),
        )),
        "restore_spreadsheet_cell_comment" => Ok(Some(
            OpenDocCommand::RestoreSpreadsheetCellComment(cell_comment_id_args(args)?),
        )),
        "set_spreadsheet_frozen_axes" => Ok(Some(OpenDocCommand::SetSpreadsheetFrozenAxes(
            FrozenAxesArgs {
                sheet_id: arg_string(args, "sheetId")?,
                frozen_rows: arg_u32(args, "frozenRows")?,
                frozen_columns: arg_u32(args, "frozenColumns")?,
            },
        ))),
        "set_spreadsheet_cell_validation" => Ok(Some(
            OpenDocCommand::SetSpreadsheetCellValidation(CellValidationArgs {
                sheet_id: arg_string(args, "sheetId")?,
                address: arg_string(args, "address")?,
                kind: arg_string(args, "kind")?,
                values: arg_string_array(args, "values")?,
                strict: arg_bool(args, "strict")?,
            }),
        )),
        "clear_spreadsheet_cell_validation" => Ok(Some(
            OpenDocCommand::ClearSpreadsheetCellValidation(sheet_address_args(args)?),
        )),
        "restore_spreadsheet_cell_validation" => Ok(Some(
            OpenDocCommand::RestoreSpreadsheetCellValidation(sheet_address_args(args)?),
        )),
        "merge_spreadsheet_cells" => Ok(Some(OpenDocCommand::MergeSpreadsheetCells(
            sheet_range_args(args)?,
        ))),
        "unmerge_spreadsheet_cells" => Ok(Some(OpenDocCommand::UnmergeSpreadsheetCells(
            sheet_range_args(args)?,
        ))),
        "restore_spreadsheet_merge" => Ok(Some(OpenDocCommand::RestoreSpreadsheetMerge(
            sheet_range_args(args)?,
        ))),
        "set_spreadsheet_basic_filter" => Ok(Some(OpenDocCommand::SetSpreadsheetBasicFilter(
            sheet_range_args(args)?,
        ))),
        "set_spreadsheet_basic_filter_options" => Ok(Some(
            OpenDocCommand::SetSpreadsheetBasicFilterOptions(FilterOptionsArgs {
                sheet_id: arg_string(args, "sheetId")?,
                criteria: arg_filter_criteria(args, "criteria")?,
                sort_specs: arg_filter_sort_specs(args, "sortSpecs")?,
            }),
        )),
        "clear_spreadsheet_basic_filter" => Ok(Some(OpenDocCommand::ClearSpreadsheetBasicFilter(
            sheet_id_args(args)?,
        ))),
        "restore_spreadsheet_basic_filter" => Ok(Some(
            OpenDocCommand::RestoreSpreadsheetBasicFilter(sheet_id_args(args)?),
        )),
        "add_spreadsheet_protected_range" => Ok(Some(
            OpenDocCommand::AddSpreadsheetProtectedRange(protected_range_args(args)?),
        )),
        "update_spreadsheet_protected_range" => Ok(Some(
            OpenDocCommand::UpdateSpreadsheetProtectedRange(protected_range_args(args)?),
        )),
        "delete_spreadsheet_protected_range" => Ok(Some(
            OpenDocCommand::DeleteSpreadsheetProtectedRange(sheet_range_args(args)?),
        )),
        "restore_spreadsheet_protected_range" => Ok(Some(
            OpenDocCommand::RestoreSpreadsheetProtectedRange(sheet_range_args(args)?),
        )),
        "set_spreadsheet_cell_in_sheet" => Ok(Some(OpenDocCommand::SetSpreadsheetCellInSheet(
            SheetCellArgs {
                sheet_id: arg_string(args, "sheetId")?,
                address: arg_string(args, "address")?,
                value: arg_string(args, "value")?,
            },
        ))),
        "set_spreadsheet_cells_in_sheet" => Ok(Some(OpenDocCommand::SetSpreadsheetCellsInSheet(
            SheetCellEditsArgs {
                sheet_id: arg_string(args, "sheetId")?,
                cells: arg_spreadsheet_cell_edits(args, "cells")?,
            },
        ))),
        "set_spreadsheet_cell_format" => Ok(Some(OpenDocCommand::SetSpreadsheetCellFormat(
            CellFormatArgs {
                sheet_id: arg_string(args, "sheetId")?,
                address: arg_string(args, "address")?,
                property: arg_string(args, "property")?,
                value: arg_string(args, "value")?,
            },
        ))),
        "set_spreadsheet_row_height" => Ok(Some(OpenDocCommand::SetSpreadsheetRowHeight(
            SheetRowHeightArgs {
                sheet_id: arg_string(args, "sheetId")?,
                row: arg_string(args, "row")?,
                height: arg_u32(args, "height")?,
            },
        ))),
        "set_spreadsheet_column_width" => Ok(Some(OpenDocCommand::SetSpreadsheetColumnWidth(
            SheetColumnWidthArgs {
                sheet_id: arg_string(args, "sheetId")?,
                column: arg_string(args, "column")?,
                width: arg_u32(args, "width")?,
            },
        ))),
        "copy_spreadsheet_range" => Ok(Some(OpenDocCommand::CopySpreadsheetRange(CopyRangeArgs {
            sheet_id: arg_string(args, "sheetId")?,
            source_range: arg_string(args, "sourceRange")?,
            target_address: arg_string(args, "targetAddress")?,
        }))),
        "add_spreadsheet_named_range" => Ok(Some(OpenDocCommand::AddSpreadsheetNamedRange(
            named_range_args(args)?,
        ))),
        "update_spreadsheet_named_range" => Ok(Some(OpenDocCommand::UpdateSpreadsheetNamedRange(
            named_range_args(args)?,
        ))),
        "delete_spreadsheet_named_range" => Ok(Some(OpenDocCommand::DeleteSpreadsheetNamedRange(
            named_range_name_args(args)?,
        ))),
        "restore_spreadsheet_named_range" => Ok(Some(
            OpenDocCommand::RestoreSpreadsheetNamedRange(named_range_name_args(args)?),
        )),
        _ => Ok(None),
    }
}

fn repository_path(args: &Value) -> Result<RepositoryPathArgs, CommandParseError> {
    Ok(RepositoryPathArgs {
        path: arg_string(args, "path")?,
    })
}

fn repository_namespace(args: &Value) -> Result<RepositoryNamespaceArgs, CommandParseError> {
    Ok(RepositoryNamespaceArgs {
        path: arg_string(args, "path")?,
        namespace: arg_string(args, "namespace")?,
    })
}

fn repository_document(args: &Value) -> Result<RepositoryDocumentArgs, CommandParseError> {
    Ok(RepositoryDocumentArgs {
        path: arg_string(args, "path")?,
        document_uuid: arg_string(args, "documentUuid")?,
    })
}

fn repository_namespace_document(
    args: &Value,
) -> Result<RepositoryNamespaceDocumentArgs, CommandParseError> {
    Ok(RepositoryNamespaceDocumentArgs {
        path: arg_string(args, "path")?,
        namespace: arg_string(args, "namespace")?,
        document_uuid: arg_string(args, "documentUuid")?,
    })
}

fn repository_doi(args: &Value) -> Result<RepositoryDoiArgs, CommandParseError> {
    Ok(RepositoryDoiArgs {
        path: arg_string(args, "path")?,
        doi: arg_string(args, "doi")?,
    })
}

fn repository_namespace_doi(args: &Value) -> Result<RepositoryNamespaceDoiArgs, CommandParseError> {
    Ok(RepositoryNamespaceDoiArgs {
        path: arg_string(args, "path")?,
        namespace: arg_string(args, "namespace")?,
        doi: arg_string(args, "doi")?,
    })
}

fn block_id_args(args: &Value) -> Result<BlockIdArgs, CommandParseError> {
    Ok(BlockIdArgs {
        block_id: arg_string(args, "blockId")?,
    })
}

fn after_block_args(args: &Value) -> Result<AfterBlockArgs, CommandParseError> {
    Ok(AfterBlockArgs {
        after_block_id: arg_string(args, "afterBlockId")?,
    })
}

fn insert_table_after_args(args: &Value) -> Result<InsertTableAfterArgs, CommandParseError> {
    let after_block_id = arg_string(args, "afterBlockId")?;
    match (
        args.get("rows").and_then(Value::as_u64),
        args.get("columns").and_then(Value::as_u64),
    ) {
        (Some(rows), Some(columns)) => Ok(InsertTableAfterArgs::Sized {
            after_block_id,
            rows: rows as usize,
            columns: columns as usize,
        }),
        _ => Ok(InsertTableAfterArgs::Default { after_block_id }),
    }
}

fn author_body_args(args: &Value) -> Result<AuthorBodyArgs, CommandParseError> {
    Ok(AuthorBodyArgs {
        author: arg_string(args, "author")?,
        body: arg_string(args, "body")?,
    })
}

fn text_range_author_body_args(args: &Value) -> Result<TextRangeAuthorBodyArgs, CommandParseError> {
    Ok(TextRangeAuthorBodyArgs {
        start_inline_id: arg_string(args, "startInlineId")?,
        end_inline_id: arg_string(args, "endInlineId")?,
        author: arg_string(args, "author")?,
        body: arg_string(args, "body")?,
    })
}

fn block_author_body_args(args: &Value) -> Result<BlockAuthorBodyArgs, CommandParseError> {
    Ok(BlockAuthorBodyArgs {
        block_id: arg_string(args, "blockId")?,
        author: arg_string(args, "author")?,
        body: arg_string(args, "body")?,
    })
}

fn thread_author_body_args(args: &Value) -> Result<ThreadAuthorBodyArgs, CommandParseError> {
    Ok(ThreadAuthorBodyArgs {
        thread_id: arg_string(args, "threadId")?,
        author: arg_string(args, "author")?,
        body: arg_string(args, "body")?,
    })
}

fn thread_id_args(args: &Value) -> Result<ThreadIdArgs, CommandParseError> {
    Ok(ThreadIdArgs {
        thread_id: arg_string(args, "threadId")?,
    })
}

fn thread_comment_id_args(args: &Value) -> Result<ThreadCommentIdArgs, CommandParseError> {
    Ok(ThreadCommentIdArgs {
        thread_id: arg_string(args, "threadId")?,
        comment_id: arg_string(args, "commentId")?,
    })
}

fn author_text_args(args: &Value) -> Result<AuthorTextArgs, CommandParseError> {
    Ok(AuthorTextArgs {
        author: arg_string(args, "author")?,
        text: arg_string(args, "text")?,
    })
}

fn text_range_author_text_args(args: &Value) -> Result<TextRangeAuthorTextArgs, CommandParseError> {
    Ok(TextRangeAuthorTextArgs {
        start_inline_id: arg_string(args, "startInlineId")?,
        end_inline_id: arg_string(args, "endInlineId")?,
        author: arg_string(args, "author")?,
        text: arg_string(args, "text")?,
    })
}

fn block_author_text_args(args: &Value) -> Result<BlockAuthorTextArgs, CommandParseError> {
    Ok(BlockAuthorTextArgs {
        block_id: arg_string(args, "blockId")?,
        author: arg_string(args, "author")?,
        text: arg_string(args, "text")?,
    })
}

fn text_range_author_args(args: &Value) -> Result<TextRangeAuthorArgs, CommandParseError> {
    Ok(TextRangeAuthorArgs {
        start_inline_id: arg_string(args, "startInlineId")?,
        end_inline_id: arg_string(args, "endInlineId")?,
        author: arg_string(args, "author")?,
    })
}

fn format_suggestion_args(args: &Value) -> Result<FormatSuggestionArgs, CommandParseError> {
    Ok(FormatSuggestionArgs {
        author: arg_string(args, "author")?,
        inline_id: arg_string(args, "inlineId")?,
        mark_kind: arg_string(args, "markKind")?,
        value: arg_optional_string(args, "value")?,
    })
}

fn bibliography_reference_metadata_args(
    args: &Value,
) -> Result<BibliographyReferenceMetadataArgs, CommandParseError> {
    Ok(BibliographyReferenceMetadataArgs {
        title: arg_string(args, "title")?,
        authors: arg_string_vec(args, "authors")?,
        issued: arg_optional_string(args, "issued")?,
        doi: arg_optional_string(args, "doi")?,
        url: arg_optional_string(args, "url")?,
    })
}

fn reference_id_args(args: &Value) -> Result<ReferenceIdArgs, CommandParseError> {
    Ok(ReferenceIdArgs {
        reference_id: arg_string(args, "referenceId")?,
    })
}

fn citation_id_args(args: &Value) -> Result<CitationIdArgs, CommandParseError> {
    Ok(CitationIdArgs {
        citation_id: arg_string(args, "citationId")?,
    })
}

fn inline_id_args(args: &Value) -> Result<InlineIdArgs, CommandParseError> {
    Ok(InlineIdArgs {
        inline_id: arg_string(args, "inlineId")?,
    })
}

fn text_mark_args(args: &Value) -> Result<TextMarkArgs, CommandParseError> {
    Ok(TextMarkArgs {
        inline_id: arg_string(args, "inlineId")?,
        mark_kind: arg_string(args, "markKind")?,
        value: arg_optional_string(args, "value")?,
    })
}

fn text_mark_range_args(args: &Value) -> Result<TextMarkRangeArgs, CommandParseError> {
    Ok(TextMarkRangeArgs {
        start_inline_id: arg_string(args, "startInlineId")?,
        end_inline_id: arg_string(args, "endInlineId")?,
        mark_kind: arg_string(args, "markKind")?,
        value: arg_optional_string(args, "value")?,
    })
}

fn sheet_id_args(args: &Value) -> Result<SheetIdArgs, CommandParseError> {
    Ok(SheetIdArgs {
        sheet_id: arg_string(args, "sheetId")?,
    })
}

fn sheet_rename_args(args: &Value) -> Result<SheetRenameArgs, CommandParseError> {
    Ok(SheetRenameArgs {
        sheet_id: arg_string(args, "sheetId")?,
        title: arg_string(args, "title")?,
    })
}

fn sheet_row_args(args: &Value) -> Result<SheetRowArgs, CommandParseError> {
    Ok(SheetRowArgs {
        sheet_id: arg_string(args, "sheetId")?,
        row: arg_string(args, "row")?,
    })
}

fn sheet_column_args(args: &Value) -> Result<SheetColumnArgs, CommandParseError> {
    Ok(SheetColumnArgs {
        sheet_id: arg_string(args, "sheetId")?,
        column: arg_string(args, "column")?,
    })
}

fn spreadsheet_selection_args(args: &Value) -> Result<SpreadsheetSelectionArgs, CommandParseError> {
    Ok(SpreadsheetSelectionArgs {
        sheet_id: arg_string(args, "sheetId")?,
        anchor: arg_string(args, "anchor")?,
        focus: arg_string(args, "focus")?,
    })
}

fn cell_comment_id_args(args: &Value) -> Result<CellCommentIdArgs, CommandParseError> {
    Ok(CellCommentIdArgs {
        comment_id: arg_string(args, "commentId")?,
    })
}

fn sheet_address_args(args: &Value) -> Result<SheetAddressArgs, CommandParseError> {
    Ok(SheetAddressArgs {
        sheet_id: arg_string(args, "sheetId")?,
        address: arg_string(args, "address")?,
    })
}

fn sheet_range_args(args: &Value) -> Result<SheetRangeArgs, CommandParseError> {
    Ok(SheetRangeArgs {
        sheet_id: arg_string(args, "sheetId")?,
        range: arg_string(args, "range")?,
    })
}

fn protected_range_args(args: &Value) -> Result<ProtectedRangeArgs, CommandParseError> {
    Ok(ProtectedRangeArgs {
        sheet_id: arg_string(args, "sheetId")?,
        range: arg_string(args, "range")?,
        description: arg_string(args, "description")?,
        warning_only: arg_bool(args, "warningOnly")?,
    })
}

fn named_range_args(args: &Value) -> Result<NamedRangeArgs, CommandParseError> {
    Ok(NamedRangeArgs {
        sheet_id: arg_string(args, "sheetId")?,
        name: arg_string(args, "name")?,
        range: arg_string(args, "range")?,
    })
}

fn named_range_name_args(args: &Value) -> Result<NamedRangeNameArgs, CommandParseError> {
    Ok(NamedRangeNameArgs {
        name: arg_string(args, "name")?,
    })
}
