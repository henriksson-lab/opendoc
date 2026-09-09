use super::*;

pub(crate) fn merge_operations_from_envelopes(
    envelopes: &[AppOperationEnvelope],
) -> Vec<Operation> {
    envelopes
        .iter()
        .filter_map(|envelope| envelope.operation.clone())
        .collect()
}

pub(crate) fn merge_blob_envelopes(
    base_blobs: Vec<AppBlobRef>,
    current_blobs: Vec<AppBlobRef>,
    candidate_blobs: Vec<AppBlobRef>,
    streams: &[&[AppOperationEnvelope]],
) -> Result<Vec<AppBlobRef>, AppApiError> {
    let mut rich_blob_state = BTreeMap::new();
    for blob in base_blobs
        .iter()
        .chain(current_blobs.iter())
        .chain(candidate_blobs.iter())
    {
        rich_blob_state
            .entry(blob.hash.clone())
            .and_modify(|existing| merge_blob_sidecar_state(existing, blob))
            .or_insert_with(|| blob.clone());
    }

    let mut merged = base_blobs
        .into_iter()
        .map(|blob| (blob.hash.clone(), blob))
        .collect::<BTreeMap<_, _>>();
    let mut operations = streams
        .iter()
        .flat_map(|stream| stream.iter())
        .filter(|envelope| envelope.blob.is_some())
        .collect::<Vec<_>>();
    operations.sort_by(|left, right| {
        (
            left.record.actor.as_str(),
            left.record.seq,
            left.record.kind.as_str(),
        )
            .cmp(&(
                right.record.actor.as_str(),
                right.record.seq,
                right.record.kind.as_str(),
            ))
    });
    let has_blob_operations = !operations.is_empty();

    let mut deleted_blobs = BTreeSet::new();
    for envelope in operations {
        match envelope.blob.as_ref().expect("filtered blob operation") {
            AppBlobOperation::Add {
                id,
                name,
                media_type,
                hash,
                size,
            } => {
                opendoc_core::HashRef::parse(hash)
                    .map_err(|err| AppApiError::Model(err.to_string()))?;
                if deleted_blobs.contains(hash) {
                    continue;
                }
                let template = rich_blob_state.get(hash);
                merged.entry(hash.clone()).or_insert_with(|| AppBlobRef {
                    id: if id.trim().is_empty() {
                        StableId::new("blob").to_string()
                    } else {
                        id.clone()
                    },
                    name: clean_blob_name(name),
                    media_type: clean_blob_media_type(media_type),
                    hash: hash.clone(),
                    size: template.map(|blob| blob.size).unwrap_or(*size),
                    available: template.map(|blob| blob.available).unwrap_or(false),
                    signature_state: template
                        .map(|blob| blob.signature_state.clone())
                        .unwrap_or_else(|| "unsigned".to_string()),
                    signatures: template
                        .map(|blob| blob.signatures.clone())
                        .unwrap_or_default(),
                    typed_signatures: template
                        .map(|blob| blob.typed_signatures.clone())
                        .unwrap_or_default(),
                    archive_tombstone: template.and_then(|blob| blob.archive_tombstone.clone()),
                });
            }
            AppBlobOperation::UpdateMetadata {
                hash,
                name,
                media_type,
            } => {
                opendoc_core::HashRef::parse(hash)
                    .map_err(|err| AppApiError::Model(err.to_string()))?;
                if deleted_blobs.contains(hash) {
                    continue;
                }
                let blob = merged.entry(hash.clone()).or_insert_with(|| {
                    rich_blob_state
                        .get(hash)
                        .cloned()
                        .unwrap_or_else(|| missing_blob_ref(hash.clone(), 0))
                });
                blob.name = clean_blob_name(name);
                blob.media_type = clean_blob_media_type(media_type);
            }
            AppBlobOperation::Delete { hash, .. } => {
                opendoc_core::HashRef::parse(hash)
                    .map_err(|err| AppApiError::Model(err.to_string()))?;
                deleted_blobs.insert(hash.clone());
                merged.remove(hash);
            }
            AppBlobOperation::ArchiveTombstone {
                hash,
                archive_tombstone,
            } => {
                opendoc_core::HashRef::parse(hash)
                    .map_err(|err| AppApiError::Model(err.to_string()))?;
                if let Some(blob) = merged.get_mut(hash) {
                    blob.archive_tombstone = Some(archive_tombstone.clone());
                }
                rich_blob_state
                    .entry(hash.clone())
                    .and_modify(|blob| {
                        if blob.archive_tombstone.is_none() {
                            blob.archive_tombstone = Some(archive_tombstone.clone());
                        }
                    })
                    .or_insert_with(|| {
                        let mut blob = missing_blob_ref(hash.clone(), 0);
                        blob.archive_tombstone = Some(archive_tombstone.clone());
                        blob
                    });
            }
            AppBlobOperation::Restore {
                id,
                name,
                media_type,
                hash,
                size,
                typed_signatures,
            } => {
                opendoc_core::HashRef::parse(hash)
                    .map_err(|err| AppApiError::Model(err.to_string()))?;
                deleted_blobs.remove(hash);
                let template = rich_blob_state.get(hash);
                merged.insert(
                    hash.clone(),
                    AppBlobRef {
                        id: if id.trim().is_empty() {
                            StableId::new("blob").to_string()
                        } else {
                            id.clone()
                        },
                        name: clean_blob_name(name),
                        media_type: clean_blob_media_type(media_type),
                        hash: hash.clone(),
                        size: template.map(|blob| blob.size).unwrap_or(*size),
                        available: template.map(|blob| blob.available).unwrap_or(false),
                        signature_state: template
                            .map(|blob| blob.signature_state.clone())
                            .unwrap_or_else(|| "unsigned".to_string()),
                        signatures: template
                            .map(|blob| blob.signatures.clone())
                            .unwrap_or_default(),
                        typed_signatures: if typed_signatures.is_empty() {
                            template
                                .map(|blob| blob.typed_signatures.clone())
                                .unwrap_or_default()
                        } else {
                            typed_signatures.clone()
                        },
                        archive_tombstone: template.and_then(|blob| blob.archive_tombstone.clone()),
                    },
                );
            }
        }
    }

    if merged.is_empty() && !has_blob_operations {
        for blob in current_blobs.into_iter().chain(candidate_blobs) {
            merged.insert(blob.hash.clone(), blob);
        }
    }

    for (hash, blob) in &mut merged {
        if let Some(rich) = rich_blob_state.get(hash) {
            merge_blob_sidecar_state(blob, rich);
        }
    }

    Ok(merged.into_values().collect())
}

pub(crate) fn restore_referenced_image_blobs(
    blobs: &mut Vec<AppBlobRef>,
    blocks: &[AppBlock],
    templates: &[Vec<AppBlobRef>],
    warnings: &mut Vec<AppWarning>,
) -> Result<(), AppApiError> {
    let mut referenced_hashes = BTreeSet::new();
    collect_app_image_blob_hashes(blocks, &mut referenced_hashes)?;
    let mut present_hashes = blobs
        .iter()
        .map(|blob| blob.hash.clone())
        .collect::<BTreeSet<_>>();
    for hash in referenced_hashes {
        if present_hashes.contains(&hash) {
            continue;
        }
        let restored = templates
            .iter()
            .flat_map(|template| template.iter())
            .find(|blob| blob.hash == hash)
            .cloned()
            .unwrap_or_else(|| missing_blob_ref(hash.clone(), 0));
        blobs.push(restored);
        present_hashes.insert(hash.clone());
        push_unique_warning(
            warnings,
            "image-blob-reference-restored",
            format!("image block references blob {hash}; restored blob metadata to current state"),
        );
    }
    Ok(())
}

fn collect_app_image_blob_hashes(
    blocks: &[AppBlock],
    hashes: &mut BTreeSet<String>,
) -> Result<(), AppApiError> {
    for block in blocks {
        if block.kind == "image" {
            let hash = block
                .blob_hash
                .clone()
                .ok_or_else(|| AppApiError::Format("image blob hash missing".to_string()))?;
            hashes.insert(
                opendoc_core::HashRef::parse(&hash)
                    .map_err(|err| AppApiError::Format(err.to_string()))?
                    .to_string(),
            );
        }
        for row in &block.rows {
            for cell in row {
                collect_app_image_blob_hashes(cell, hashes)?;
            }
        }
    }
    Ok(())
}

fn merge_blob_sidecar_state(existing: &mut AppBlobRef, next: &AppBlobRef) {
    existing.available |= next.available;
    existing.size = existing.size.max(next.size);
    for signature in &next.signatures {
        if !existing.signatures.contains(signature) {
            existing.signatures.push(signature.clone());
        }
    }
    for signature in &next.typed_signatures {
        if !existing.typed_signatures.contains(signature) {
            existing.typed_signatures.push(signature.clone());
        }
    }
    if existing.archive_tombstone.is_none() {
        existing.archive_tombstone = next.archive_tombstone.clone();
    }
    if existing.signature_state == "unsigned" && next.signature_state != "unsigned" {
        existing.signature_state = next.signature_state.clone();
    }
}

pub(crate) fn missing_blob_ref(hash: String, size: u64) -> AppBlobRef {
    AppBlobRef {
        id: StableId::new("blob").to_string(),
        name: "missing blob".to_string(),
        media_type: "application/octet-stream".to_string(),
        hash,
        size,
        available: false,
        signature_state: "untrusted".to_string(),
        signatures: Vec::new(),
        typed_signatures: Vec::new(),
        archive_tombstone: None,
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ImportedOpenDocBlobRef {
    pub(crate) blob: AppBlobRef,
    pub(crate) tombstone_record: Option<opendoc_format::TombstoneRecord>,
}

pub(crate) fn import_opendoc_blob_refs_from_google_docs_json(
    json_text: &str,
) -> Result<Vec<ImportedOpenDocBlobRef>, AppApiError> {
    let value: Value =
        serde_json::from_str(json_text).map_err(|err| AppApiError::Import(err.to_string()))?;
    let Some(blobs) = value.get("opendocBlobs") else {
        return Ok(Vec::new());
    };
    let blobs = blobs
        .as_array()
        .ok_or_else(|| AppApiError::Import("opendocBlobs must be an array".to_string()))?;
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for blob_value in blobs {
        let mut blob: AppBlobRef = serde_json::from_value(blob_value.clone())
            .map_err(|err| AppApiError::Import(format!("invalid opendocBlobs entry: {err}")))?;
        StableId::parse(&blob.id).map_err(|err| AppApiError::Import(err.to_string()))?;
        let hash = opendoc_core::HashRef::parse(blob.hash.trim())
            .map_err(|err| AppApiError::Import(format!("OpenDoc blob hash is invalid: {err}")))?
            .to_string();
        if !seen.insert(hash.clone()) {
            return Err(AppApiError::Import(format!(
                "duplicate OpenDoc blob reference {hash}"
            )));
        }
        blob.hash = hash;
        blob.name = clean_blob_name(&blob.name);
        blob.media_type = clean_blob_media_type(&blob.media_type);
        for signature in &mut blob.signatures {
            signature.target = signature.target.trim().to_string();
        }
        for typed in &mut blob.typed_signatures {
            typed.source_blob = typed.source_blob.trim().to_string();
            typed.semantic_digest = typed.semantic_digest.trim().to_string();
            typed.signature.target = typed.signature.target.trim().to_string();
            if typed.source_blob != blob.hash {
                return Err(AppApiError::Import(format!(
                    "typed signature source {} does not match blob {}",
                    typed.source_blob, blob.hash
                )));
            }
            typed.signature_state = "untrusted".to_string();
        }
        blob.validate_source()
            .map_err(|err| AppApiError::Import(format!("invalid opendocBlobs entry: {err}")))?;
        let tombstone_record = imported_blob_tombstone_record(blob_value, &blob)?;
        out.push(ImportedOpenDocBlobRef {
            blob,
            tombstone_record,
        });
    }
    Ok(out)
}

fn imported_blob_tombstone_record(
    blob_value: &Value,
    blob: &AppBlobRef,
) -> Result<Option<opendoc_format::TombstoneRecord>, AppApiError> {
    let Some(tombstone) = blob.archive_tombstone.as_ref() else {
        return Ok(None);
    };
    let Some(signature_value) = blob_value
        .get("archive_tombstone")
        .and_then(Value::as_object)
        .and_then(|object| object.get("signature"))
    else {
        return Ok(None);
    };
    let signature = import_byte_array(signature_value, "archive tombstone signature")?;
    let record = opendoc_format::TombstoneRecord {
        object: opendoc_core::HashRef::parse(&blob.hash)
            .map_err(|err| AppApiError::Import(err.to_string()))?,
        archive_locator: tombstone.archive_locator.clone(),
        restore_hint: tombstone.restore_hint.clone(),
        created_at_ms: tombstone.created_at_ms,
        signer: tombstone.signer.clone(),
        signature,
    };
    record
        .validate()
        .map_err(|err| AppApiError::Import(err.to_string()))?;
    Ok(Some(record))
}

fn import_byte_array(value: &Value, label: &str) -> Result<Vec<u8>, AppApiError> {
    let bytes = value
        .as_array()
        .ok_or_else(|| AppApiError::Import(format!("{label} must be an array")))?;
    let mut out = Vec::with_capacity(bytes.len());
    for (index, byte) in bytes.iter().enumerate() {
        let Some(byte) = byte.as_u64() else {
            return Err(AppApiError::Import(format!(
                "{label} {index} must be a byte"
            )));
        };
        let byte = u8::try_from(byte)
            .map_err(|_| AppApiError::Import(format!("{label} {index} must be a byte")))?;
        out.push(byte);
    }
    Ok(out)
}

pub(crate) fn export_google_docs_json_with_opendoc_blobs(
    bytes: Vec<u8>,
    blobs: Vec<AppBlobRef>,
) -> Result<String, AppApiError> {
    let mut value: Value =
        serde_json::from_slice(&bytes).map_err(|err| AppApiError::Import(err.to_string()))?;
    if !blobs.is_empty() {
        value["opendocBlobs"] = serde_json::to_value(blobs)
            .map_err(|err| AppApiError::Import(format!("blob export failed: {err}")))?;
    }
    serde_json::to_string_pretty(&value).map_err(|err| AppApiError::Import(err.to_string()))
}

pub(crate) fn clean_blob_name(name: &str) -> String {
    if name.trim().is_empty() {
        "unnamed blob".to_string()
    } else {
        name.trim().to_string()
    }
}

pub(crate) fn clean_blob_media_type(media_type: &str) -> String {
    if media_type.trim().is_empty() {
        "application/octet-stream".to_string()
    } else {
        media_type.trim().to_string()
    }
}
