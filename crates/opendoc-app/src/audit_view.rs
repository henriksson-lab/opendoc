use super::*;
use std::ops::Deref;

impl OpenDocApp {
    pub fn audit_view(&self) -> AppAuditView {
        AuditProjectionService::new(self).audit_view()
    }

    pub(crate) fn deleted_sheet_restore_payload(
        &self,
        sheet_id: &str,
    ) -> Option<(AppSheet, Vec<AppNamedRange>)> {
        AuditProjectionService::new(self).deleted_sheet_restore_payload(sheet_id)
    }

    pub(crate) fn deleted_named_range_restore_payload(&self, name: &str) -> Option<AppNamedRange> {
        AuditProjectionService::new(self).deleted_named_range_restore_payload(name)
    }

    pub(crate) fn deleted_protected_range_restore_payload(
        &self,
        sheet_id: &str,
        range: &str,
    ) -> Option<AppSheetProtectedRange> {
        AuditProjectionService::new(self).deleted_protected_range_restore_payload(sheet_id, range)
    }

    pub(crate) fn deleted_basic_filter_restore_payload(
        &self,
        sheet_id: &str,
    ) -> Option<AppSheetFilter> {
        AuditProjectionService::new(self).deleted_basic_filter_restore_payload(sheet_id)
    }

    pub(crate) fn deleted_merge_restore_payload(
        &self,
        sheet_id: &str,
        range: &str,
    ) -> Option<AppSheetMerge> {
        AuditProjectionService::new(self).deleted_merge_restore_payload(sheet_id, range)
    }

    pub(crate) fn deleted_cell_validation_restore_payload(
        &self,
        sheet_id: &str,
        address: &str,
    ) -> Option<AppCellValidation> {
        AuditProjectionService::new(self).deleted_cell_validation_restore_payload(sheet_id, address)
    }

    pub(crate) fn deleted_row_restore_payload(
        &self,
        sheet_id: &str,
        row: &str,
    ) -> Option<AppDeletedRowPayload> {
        AuditProjectionService::new(self).deleted_row_restore_payload(sheet_id, row)
    }

    pub(crate) fn deleted_column_restore_payload(
        &self,
        sheet_id: &str,
        column: &str,
    ) -> Option<AppDeletedColumnPayload> {
        AuditProjectionService::new(self).deleted_column_restore_payload(sheet_id, column)
    }

    pub(crate) fn retained_deleted_blob_refs(&self) -> BTreeMap<String, AppBlobRef> {
        AuditProjectionService::new(self).retained_deleted_blob_refs()
    }
}

pub(crate) struct AuditProjectionService<'a> {
    app: &'a OpenDocApp,
}

impl<'a> AuditProjectionService<'a> {
    fn new(app: &'a OpenDocApp) -> Self {
        Self { app }
    }

    fn audit_view(&self) -> AppAuditView {
        let document = self.document();
        let blob_signatures = document
            .blobs
            .iter()
            .map(|blob| AppAuditBlobSignature {
                blob_hash: blob.hash.clone(),
                blob_name: blob.name.clone(),
                deleted: false,
                signature_state: blob.signature_state.clone(),
                archive_tombstone: blob.archive_tombstone.clone(),
                signatures: blob.signatures.clone(),
                typed_signatures: blob.typed_signatures.clone(),
            })
            .chain(
                self.retained_deleted_blob_refs()
                    .into_values()
                    .map(|blob| self.deleted_blob_audit_signature(blob)),
            )
            .collect::<Vec<_>>();
        let attached_tombstone_hashes = blob_signatures
            .iter()
            .filter(|blob| blob.archive_tombstone.is_some())
            .map(|blob| blob.blob_hash.clone())
            .collect::<BTreeSet<_>>();
        let (repository_tombstones, repository_tombstone_problems) =
            self.audit_repository_tombstones(&attached_tombstone_hashes);
        AppAuditView {
            uuid: document.uuid.clone(),
            title: document.title.clone(),
            repository_root: document.repository_root.clone(),
            repository_backend: document.repository_backend.clone(),
            repository_namespace: document.repository_namespace.clone(),
            last_manifest: document.last_manifest.clone(),
            warnings: document.warnings.clone(),
            signatures: document.signatures.clone(),
            blob_signatures,
            repository_tombstones,
            repository_tombstone_problems,
            deleted_comments: document
                .comments
                .iter()
                .filter(|thread| {
                    thread.deleted || thread.comments.iter().any(|comment| comment.deleted)
                })
                .cloned()
                .collect(),
            deleted_sheets: self.deleted_sheet_audit_entries(),
            deleted_named_ranges: self.deleted_named_range_audit_entries(),
            deleted_protected_ranges: self.deleted_protected_range_audit_entries(),
            deleted_basic_filters: self.deleted_basic_filter_audit_entries(),
            deleted_merges: self.deleted_merge_audit_entries(),
            deleted_cell_validations: self.deleted_cell_validation_audit_entries(),
            deleted_rows: self.deleted_row_audit_entries(),
            deleted_columns: self.deleted_column_audit_entries(),
            deleted_cell_comments: document.workbook.deleted_cell_comments(),
            resolved_suggestions: document
                .suggestions
                .iter()
                .filter(|suggestion| suggestion.state != "proposed")
                .cloned()
                .collect(),
            deleted_references: document
                .citations
                .references
                .iter()
                .filter(|reference| reference.deleted)
                .cloned()
                .collect(),
            deleted_citations: document
                .citations
                .citations
                .iter()
                .filter(|citation| citation.deleted)
                .cloned()
                .collect(),
            candidate_head_problems: self.audit_candidate_head_problems(),
            operations: document.operations.clone(),
        }
    }

    fn deleted_sheet_audit_entries(&self) -> Vec<AppDeletedSheet> {
        self.operation_envelopes
            .iter()
            .filter_map(|envelope| {
                let Some(AppSpreadsheetOperation::DeleteSheet { sheet_id, .. }) =
                    &envelope.spreadsheet
                else {
                    return None;
                };
                if self
                    .workbook
                    .sheets
                    .iter()
                    .any(|sheet| sheet.id == *sheet_id)
                    || self.deleted_sheet_restore_payload(sheet_id).is_none()
                {
                    return None;
                }
                Some(AppDeletedSheet {
                    sheet_id: sheet_id.clone(),
                    operation: envelope.record.clone(),
                })
            })
            .collect()
    }

    fn deleted_sheet_restore_payload(
        &self,
        sheet_id: &str,
    ) -> Option<(AppSheet, Vec<AppNamedRange>)> {
        let mut payload = None;
        for envelope in &self.operation_envelopes {
            match &envelope.spreadsheet {
                Some(AppSpreadsheetOperation::DeleteSheet {
                    sheet_id: deleted_id,
                    sheet,
                    named_ranges,
                }) if deleted_id == sheet_id => {
                    payload = Some((sheet.clone(), named_ranges.clone()));
                }
                Some(AppSpreadsheetOperation::RestoreSheet {
                    sheet_id: restored_id,
                    ..
                }) if restored_id == sheet_id => {
                    payload = None;
                }
                _ => {}
            }
        }
        payload
    }

    fn deleted_named_range_audit_entries(&self) -> Vec<AppDeletedNamedRange> {
        self.operation_envelopes
            .iter()
            .filter_map(|envelope| {
                let Some(AppSpreadsheetOperation::DeleteNamedRange { name, range }) =
                    &envelope.spreadsheet
                else {
                    return None;
                };
                if self
                    .workbook
                    .named_ranges
                    .iter()
                    .any(|current| current.name == *name)
                    || self.deleted_named_range_restore_payload(name).is_none()
                {
                    return None;
                }
                Some(AppDeletedNamedRange {
                    name: name.clone(),
                    range: range.clone(),
                    operation: envelope.record.clone(),
                })
            })
            .collect()
    }

    fn deleted_named_range_restore_payload(&self, name: &str) -> Option<AppNamedRange> {
        let mut payload = None;
        for envelope in &self.operation_envelopes {
            match &envelope.spreadsheet {
                Some(AppSpreadsheetOperation::DeleteNamedRange {
                    name: deleted_name,
                    range,
                }) if deleted_name == name => {
                    payload = Some(range.clone());
                }
                Some(AppSpreadsheetOperation::RestoreNamedRange {
                    name: restored_name,
                    ..
                }) if restored_name == name => {
                    payload = None;
                }
                _ => {}
            }
        }
        payload
    }

    fn deleted_protected_range_audit_entries(&self) -> Vec<AppDeletedProtectedRange> {
        self.operation_envelopes
            .iter()
            .filter_map(|envelope| {
                let Some(AppSpreadsheetOperation::DeleteProtectedRange {
                    sheet_id,
                    range,
                    protected_range,
                }) = &envelope.spreadsheet
                else {
                    return None;
                };
                if self
                    .workbook
                    .sheets
                    .iter()
                    .find(|sheet| sheet.id == *sheet_id)
                    .and_then(|sheet| {
                        sheet
                            .protected_ranges
                            .iter()
                            .find(|current| current.range == *range)
                    })
                    .is_some()
                    || self
                        .deleted_protected_range_restore_payload(sheet_id, range)
                        .is_none()
                {
                    return None;
                }
                Some(AppDeletedProtectedRange {
                    sheet_id: sheet_id.clone(),
                    protected_range: protected_range.clone(),
                    operation: envelope.record.clone(),
                })
            })
            .collect()
    }

    fn deleted_protected_range_restore_payload(
        &self,
        sheet_id: &str,
        range: &str,
    ) -> Option<AppSheetProtectedRange> {
        let mut payload = None;
        for envelope in &self.operation_envelopes {
            match &envelope.spreadsheet {
                Some(AppSpreadsheetOperation::DeleteProtectedRange {
                    sheet_id: deleted_sheet_id,
                    range: deleted_range,
                    protected_range,
                }) if deleted_sheet_id == sheet_id && deleted_range == range => {
                    payload = Some(protected_range.clone());
                }
                Some(AppSpreadsheetOperation::RestoreProtectedRange {
                    sheet_id: restored_sheet_id,
                    range: restored_range,
                    ..
                }) if restored_sheet_id == sheet_id && restored_range == range => {
                    payload = None;
                }
                _ => {}
            }
        }
        payload
    }

    fn deleted_basic_filter_audit_entries(&self) -> Vec<AppDeletedBasicFilter> {
        self.operation_envelopes
            .iter()
            .filter_map(|envelope| {
                let Some(AppSpreadsheetOperation::ClearBasicFilter { sheet_id, filter }) =
                    &envelope.spreadsheet
                else {
                    return None;
                };
                if self
                    .workbook
                    .sheets
                    .iter()
                    .find(|sheet| sheet.id == *sheet_id)
                    .is_some_and(|sheet| !sheet.filters.is_empty())
                    || self
                        .deleted_basic_filter_restore_payload(sheet_id)
                        .is_none()
                {
                    return None;
                }
                Some(AppDeletedBasicFilter {
                    sheet_id: sheet_id.clone(),
                    filter: filter.clone(),
                    operation: envelope.record.clone(),
                })
            })
            .collect()
    }

    fn deleted_basic_filter_restore_payload(&self, sheet_id: &str) -> Option<AppSheetFilter> {
        let mut payload = None;
        for envelope in &self.operation_envelopes {
            match &envelope.spreadsheet {
                Some(AppSpreadsheetOperation::ClearBasicFilter {
                    sheet_id: cleared_sheet_id,
                    filter,
                }) if cleared_sheet_id == sheet_id => {
                    payload = Some(filter.clone());
                }
                Some(AppSpreadsheetOperation::RestoreBasicFilter {
                    sheet_id: restored_sheet_id,
                    ..
                }) if restored_sheet_id == sheet_id => {
                    payload = None;
                }
                _ => {}
            }
        }
        payload
    }

    fn deleted_merge_audit_entries(&self) -> Vec<AppDeletedMerge> {
        self.operation_envelopes
            .iter()
            .filter_map(|envelope| {
                let Some(AppSpreadsheetOperation::UnmergeCells {
                    sheet_id,
                    range,
                    merge,
                }) = &envelope.spreadsheet
                else {
                    return None;
                };
                if self
                    .workbook
                    .sheets
                    .iter()
                    .find(|sheet| sheet.id == *sheet_id)
                    .and_then(|sheet| sheet.merges.iter().find(|current| current.range == *range))
                    .is_some()
                    || self
                        .deleted_merge_restore_payload(sheet_id, range)
                        .is_none()
                {
                    return None;
                }
                Some(AppDeletedMerge {
                    sheet_id: sheet_id.clone(),
                    merge: merge.clone(),
                    operation: envelope.record.clone(),
                })
            })
            .collect()
    }

    fn deleted_merge_restore_payload(&self, sheet_id: &str, range: &str) -> Option<AppSheetMerge> {
        let mut payload = None;
        for envelope in &self.operation_envelopes {
            match &envelope.spreadsheet {
                Some(AppSpreadsheetOperation::UnmergeCells {
                    sheet_id: unmerged_sheet_id,
                    range: unmerged_range,
                    merge,
                }) if unmerged_sheet_id == sheet_id && unmerged_range == range => {
                    payload = Some(merge.clone());
                }
                Some(AppSpreadsheetOperation::RestoreMerge {
                    sheet_id: restored_sheet_id,
                    range: restored_range,
                    ..
                }) if restored_sheet_id == sheet_id && restored_range == range => {
                    payload = None;
                }
                _ => {}
            }
        }
        payload
    }

    fn deleted_cell_validation_audit_entries(&self) -> Vec<AppDeletedCellValidation> {
        self.operation_envelopes
            .iter()
            .filter_map(|envelope| {
                let Some(AppSpreadsheetOperation::ClearCellValidation {
                    sheet_id,
                    address,
                    validation,
                }) = &envelope.spreadsheet
                else {
                    return None;
                };
                if self.workbook.cell_validation(sheet_id, address).is_some()
                    || self
                        .deleted_cell_validation_restore_payload(sheet_id, address)
                        .is_none()
                {
                    return None;
                }
                Some(AppDeletedCellValidation {
                    sheet_id: sheet_id.clone(),
                    address: address.clone(),
                    validation: validation.clone(),
                    operation: envelope.record.clone(),
                })
            })
            .collect()
    }

    fn deleted_cell_validation_restore_payload(
        &self,
        sheet_id: &str,
        address: &str,
    ) -> Option<AppCellValidation> {
        let mut payload = None;
        for envelope in &self.operation_envelopes {
            match &envelope.spreadsheet {
                Some(AppSpreadsheetOperation::ClearCellValidation {
                    sheet_id: cleared_sheet_id,
                    address: cleared_address,
                    validation,
                }) if cleared_sheet_id == sheet_id && cleared_address == address => {
                    payload = Some(validation.clone());
                }
                Some(AppSpreadsheetOperation::RestoreCellValidation {
                    sheet_id: restored_sheet_id,
                    address: restored_address,
                    ..
                }) if restored_sheet_id == sheet_id && restored_address == address => {
                    payload = None;
                }
                _ => {}
            }
        }
        payload
    }

    fn deleted_row_audit_entries(&self) -> Vec<AppDeletedRow> {
        self.operation_envelopes
            .iter()
            .filter_map(|envelope| {
                let Some(AppSpreadsheetOperation::DeleteRow {
                    sheet_id,
                    row,
                    payload,
                }) = &envelope.spreadsheet
                else {
                    return None;
                };
                if self
                    .workbook
                    .sheets
                    .iter()
                    .find(|sheet| sheet.id == *sheet_id)
                    .is_some_and(|sheet| sheet.rows.iter().any(|current| current == row))
                    || self.deleted_row_restore_payload(sheet_id, row).is_none()
                {
                    return None;
                }
                Some(AppDeletedRow {
                    sheet_id: sheet_id.clone(),
                    row: row.clone(),
                    payload: payload.clone(),
                    operation: envelope.record.clone(),
                })
            })
            .collect()
    }

    fn deleted_row_restore_payload(
        &self,
        sheet_id: &str,
        row: &str,
    ) -> Option<AppDeletedRowPayload> {
        let mut payload = None;
        for envelope in &self.operation_envelopes {
            match &envelope.spreadsheet {
                Some(AppSpreadsheetOperation::DeleteRow {
                    sheet_id: deleted_sheet_id,
                    row: deleted_row,
                    payload: deleted_payload,
                }) if deleted_sheet_id == sheet_id && deleted_row == row => {
                    payload = Some(deleted_payload.clone());
                }
                Some(AppSpreadsheetOperation::RestoreRow {
                    sheet_id: restored_sheet_id,
                    row: restored_row,
                    ..
                }) if restored_sheet_id == sheet_id && restored_row == row => {
                    payload = None;
                }
                _ => {}
            }
        }
        payload
    }

    fn deleted_column_audit_entries(&self) -> Vec<AppDeletedColumn> {
        self.operation_envelopes
            .iter()
            .filter_map(|envelope| {
                let Some(AppSpreadsheetOperation::DeleteColumn {
                    sheet_id,
                    column,
                    payload,
                }) = &envelope.spreadsheet
                else {
                    return None;
                };
                if self
                    .workbook
                    .sheets
                    .iter()
                    .find(|sheet| sheet.id == *sheet_id)
                    .is_some_and(|sheet| sheet.columns.iter().any(|current| current == column))
                    || self
                        .deleted_column_restore_payload(sheet_id, column)
                        .is_none()
                {
                    return None;
                }
                Some(AppDeletedColumn {
                    sheet_id: sheet_id.clone(),
                    column: column.clone(),
                    payload: payload.clone(),
                    operation: envelope.record.clone(),
                })
            })
            .collect()
    }

    fn deleted_column_restore_payload(
        &self,
        sheet_id: &str,
        column: &str,
    ) -> Option<AppDeletedColumnPayload> {
        let mut payload = None;
        for envelope in &self.operation_envelopes {
            match &envelope.spreadsheet {
                Some(AppSpreadsheetOperation::DeleteColumn {
                    sheet_id: deleted_sheet_id,
                    column: deleted_column,
                    payload: deleted_payload,
                }) if deleted_sheet_id == sheet_id && deleted_column == column => {
                    payload = Some(deleted_payload.clone());
                }
                Some(AppSpreadsheetOperation::RestoreColumn {
                    sheet_id: restored_sheet_id,
                    column: restored_column,
                    ..
                }) if restored_sheet_id == sheet_id && restored_column == column => {
                    payload = None;
                }
                _ => {}
            }
        }
        payload
    }

    fn audit_repository_tombstones(
        &self,
        attached_hashes: &BTreeSet<String>,
    ) -> (Vec<AppRepositoryTombstone>, Vec<String>) {
        let Some(root) = self.repository_root.as_ref() else {
            return (Vec::new(), Vec::new());
        };
        let result = match self.repository_backend.as_deref() {
            Some("local") => {
                let repo = Repository::new(LocalObjectStore::new(root));
                scan_repository_tombstones(&repo, attached_hashes)
            }
            Some("flat") => {
                let Some(namespace) = self.repository_namespace.as_ref() else {
                    return (
                        Vec::new(),
                        vec!["flat repository tombstone audit needs a namespace".to_string()],
                    );
                };
                match FlatObjectStore::new(root, namespace.clone()) {
                    Ok(store) => {
                        let repo = Repository::new(store);
                        scan_repository_tombstones(&repo, attached_hashes)
                    }
                    Err(err) => Err(err),
                }
            }
            #[cfg(feature = "opendal-store")]
            Some("opendal-fs") => {
                let Some(namespace) = self.repository_namespace.as_ref() else {
                    return (
                        Vec::new(),
                        vec!["OpenDAL FS repository tombstone audit needs a namespace".to_string()],
                    );
                };
                match OpenDalObjectStore::from_fs_root(root, namespace.clone()) {
                    Ok(store) => {
                        let repo = Repository::new(store);
                        scan_repository_tombstones(&repo, attached_hashes)
                    }
                    Err(err) => Err(err),
                }
            }
            Some(other) => {
                return (
                    Vec::new(),
                    vec![format!(
                        "repository tombstone audit does not support repository backend {other}"
                    )],
                );
            }
            None => return (Vec::new(), Vec::new()),
        };
        match result {
            Ok((tombstones, problems)) => (tombstones, problems),
            Err(err) => (
                Vec::new(),
                vec![format!("repository tombstone scan failed: {err}")],
            ),
        }
    }

    fn audit_candidate_head_problems(&self) -> Vec<AppCandidateHeadProblem> {
        let Some(root) = self.repository_root.as_ref() else {
            return Vec::new();
        };
        let result = match self.repository_backend.as_deref() {
            Some("local") => {
                let repo = Repository::new(LocalObjectStore::new(root));
                repo.resolve_candidate_heads(self.document.uuid.as_str(), SNAPSHOT_BRANCH)
                    .map(|resolution| resolution.invalid_candidates)
            }
            Some("flat") => {
                let Some(namespace) = self.repository_namespace.as_ref() else {
                    return vec![AppCandidateHeadProblem {
                        path: String::new(),
                        reason: "flat repository audit needs a namespace".to_string(),
                    }];
                };
                match FlatObjectStore::new(root, namespace.clone()) {
                    Ok(store) => {
                        let repo = Repository::new(store);
                        repo.resolve_candidate_heads(self.document.uuid.as_str(), SNAPSHOT_BRANCH)
                            .map(|resolution| resolution.invalid_candidates)
                    }
                    Err(err) => Err(err),
                }
            }
            #[cfg(feature = "opendal-store")]
            Some("opendal-fs") => {
                let Some(namespace) = self.repository_namespace.as_ref() else {
                    return vec![AppCandidateHeadProblem {
                        path: String::new(),
                        reason: "OpenDAL FS repository audit needs a namespace".to_string(),
                    }];
                };
                match OpenDalObjectStore::from_fs_root(root, namespace.clone()) {
                    Ok(store) => {
                        let repo = Repository::new(store);
                        repo.resolve_candidate_heads(self.document.uuid.as_str(), SNAPSHOT_BRANCH)
                            .map(|resolution| resolution.invalid_candidates)
                    }
                    Err(err) => Err(err),
                }
            }
            Some(other) => {
                return vec![AppCandidateHeadProblem {
                    path: String::new(),
                    reason: format!(
                        "candidate head audit does not support repository backend {other}"
                    ),
                }];
            }
            None => return Vec::new(),
        };
        match result {
            Ok(problems) => problems
                .into_iter()
                .map(AppCandidateHeadProblem::from_store)
                .collect(),
            Err(err) => vec![AppCandidateHeadProblem {
                path: String::new(),
                reason: format!("candidate head scan failed: {err}"),
            }],
        }
    }

    fn deleted_blob_audit_signature(&self, blob: AppBlobRef) -> AppAuditBlobSignature {
        let mut typed_signatures = blob.typed_signatures.clone();
        for typed in &mut typed_signatures {
            typed.signature_state = if let Some(bytes) = self.blob_bytes.get(&blob.hash) {
                verify_app_typed_signature(typed, &blob.hash, bytes)
            } else {
                "untrusted".to_string()
            };
        }
        AppAuditBlobSignature {
            blob_hash: blob.hash.clone(),
            blob_name: blob.name.clone(),
            deleted: true,
            signature_state: if self
                .blob_signatures
                .get(&blob.hash)
                .is_some_and(|signatures| !signatures.is_empty())
            {
                "signed".to_string()
            } else {
                "unsigned".to_string()
            },
            archive_tombstone: self.blob_tombstones.get(&blob.hash).cloned(),
            signatures: self
                .blob_signatures
                .get(&blob.hash)
                .into_iter()
                .flatten()
                .map(AppSignature::from_record)
                .collect(),
            typed_signatures,
        }
    }

    fn retained_deleted_blob_refs(&self) -> BTreeMap<String, AppBlobRef> {
        let current = self
            .blobs
            .iter()
            .map(|blob| blob.hash.as_str())
            .collect::<BTreeSet<_>>();
        let mut retained = BTreeMap::new();
        let mut deleted = BTreeSet::new();
        for envelope in &self.operation_envelopes {
            let Some(operation) = &envelope.blob else {
                continue;
            };
            match operation {
                AppBlobOperation::Add {
                    id,
                    name,
                    media_type,
                    hash,
                    size,
                } => {
                    if current.contains(hash.as_str()) {
                        continue;
                    }
                    retained.entry(hash.clone()).or_insert_with(|| AppBlobRef {
                        id: if id.trim().is_empty() {
                            StableId::new("blob").to_string()
                        } else {
                            id.clone()
                        },
                        name: clean_blob_name(name),
                        media_type: clean_blob_media_type(media_type),
                        hash: hash.clone(),
                        size: *size,
                        available: self.blob_bytes.contains_key(hash),
                        signature_state: "unsigned".to_string(),
                        signatures: Vec::new(),
                        typed_signatures: Vec::new(),
                        archive_tombstone: self.blob_tombstones.get(hash).cloned(),
                    });
                }
                AppBlobOperation::UpdateMetadata {
                    hash,
                    name,
                    media_type,
                } => {
                    if let Some(blob) = retained.get_mut(hash) {
                        blob.name = clean_blob_name(name);
                        blob.media_type = clean_blob_media_type(media_type);
                    }
                }
                AppBlobOperation::Delete {
                    hash,
                    typed_signatures,
                } => {
                    if !current.contains(hash.as_str()) {
                        deleted.insert(hash.clone());
                        let blob = retained
                            .entry(hash.clone())
                            .or_insert_with(|| missing_blob_ref(hash.clone(), 0));
                        blob.typed_signatures = typed_signatures.clone();
                    }
                }
                AppBlobOperation::Restore { hash, .. } => {
                    deleted.remove(hash);
                    retained.remove(hash);
                }
                AppBlobOperation::ArchiveTombstone {
                    hash,
                    archive_tombstone,
                } => {
                    if let Some(blob) = retained.get_mut(hash) {
                        blob.archive_tombstone = Some(archive_tombstone.clone());
                    }
                }
            }
        }
        retained.retain(|hash, _| deleted.contains(hash));
        retained
    }
}

impl Deref for AuditProjectionService<'_> {
    type Target = OpenDocApp;

    fn deref(&self) -> &Self::Target {
        self.app
    }
}

fn scan_repository_tombstones<S: ObjectStore>(
    repo: &Repository<S>,
    attached_hashes: &BTreeSet<String>,
) -> Result<(Vec<AppRepositoryTombstone>, Vec<String>), StoreError> {
    let scan = repo.scan_tombstone_entries()?;
    let tombstones = scan
        .records
        .into_iter()
        .map(|record| {
            let blob_hash = record.object.to_string();
            AppRepositoryTombstone {
                attached_to_blob_ref: attached_hashes.contains(&blob_hash),
                blob_hash,
                archive_tombstone: AppArchiveTombstone::from_record(&record),
            }
        })
        .collect();
    let problems = scan
        .invalid
        .into_iter()
        .map(|problem| {
            format!(
                "repository tombstone {} failed validation: {}",
                problem.path, problem.reason
            )
        })
        .collect();
    Ok((tombstones, problems))
}
