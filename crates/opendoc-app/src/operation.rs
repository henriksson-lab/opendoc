use super::*;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppOperationRecord {
    pub actor: String,
    pub seq: u64,
    pub kind: String,
    pub summary: String,
    pub created_at_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct AppOperationEnvelope {
    pub record: AppOperationRecord,
    pub operation: Option<Operation>,
    #[serde(default)]
    pub spreadsheet: Option<AppSpreadsheetOperation>,
    #[serde(default)]
    pub blob: Option<AppBlobOperation>,
}

impl AppOperationRecord {
    fn validate_source(&self) -> Result<(), AppApiError> {
        if self.actor.trim().is_empty() {
            return Err(AppApiError::Format(
                "operation record actor is empty".to_string(),
            ));
        }
        if self.actor.trim() != self.actor {
            return Err(AppApiError::Format(
                "operation record actor has surrounding whitespace".to_string(),
            ));
        }
        if self.seq == 0 {
            return Err(AppApiError::Format(
                "operation record sequence is zero".to_string(),
            ));
        }
        if self.kind.trim().is_empty() {
            return Err(AppApiError::Format(
                "operation record kind is empty".to_string(),
            ));
        }
        if self.kind.trim() != self.kind {
            return Err(AppApiError::Format(
                "operation record kind has surrounding whitespace".to_string(),
            ));
        }
        if self.summary.trim().is_empty() {
            return Err(AppApiError::Format(
                "operation record summary is empty".to_string(),
            ));
        }
        if self.summary.trim() != self.summary {
            return Err(AppApiError::Format(
                "operation record summary has surrounding whitespace".to_string(),
            ));
        }
        Ok(())
    }
}

impl AppOperationEnvelope {
    fn validate_source(&self) -> Result<(), AppApiError> {
        self.record.validate_source()?;
        let payload_count = usize::from(self.operation.is_some())
            + usize::from(self.spreadsheet.is_some())
            + usize::from(self.blob.is_some());
        if payload_count > 1 {
            return Err(AppApiError::Format(
                "operation envelope has multiple payloads".to_string(),
            ));
        }
        if let Some(operation) = &self.operation {
            if operation.id.actor.0.trim().is_empty() {
                return Err(AppApiError::Format(
                    "operation payload actor is empty".to_string(),
                ));
            }
            if operation.id.actor.0.trim() != operation.id.actor.0 {
                return Err(AppApiError::Format(
                    "operation payload actor has surrounding whitespace".to_string(),
                ));
            }
            if operation.id.seq == 0 {
                return Err(AppApiError::Format(
                    "operation payload sequence is zero".to_string(),
                ));
            }
            if operation.id.actor.0 != self.record.actor || operation.id.seq != self.record.seq {
                return Err(AppApiError::Format(
                    "operation envelope record does not match rich-document operation id"
                        .to_string(),
                ));
            }
            let expected_kind = rich_document_operation_kind(&operation.kind);
            if self.record.kind != expected_kind {
                return Err(AppApiError::Format(format!(
                    "operation envelope kind {} does not match rich-document payload {}",
                    self.record.kind, expected_kind
                )));
            }
            validate_rich_document_operation_source(&operation.kind)?;
        }
        if let Some(blob) = &self.blob {
            blob.validate_source()?;
            if self.record.kind != blob.operation_kind() {
                return Err(AppApiError::Format(format!(
                    "operation envelope kind {} does not match blob payload {}",
                    self.record.kind,
                    blob.operation_kind()
                )));
            }
        }
        if let Some(spreadsheet) = &self.spreadsheet {
            spreadsheet.validate_source()?;
            if self.record.kind != spreadsheet.operation_kind() {
                return Err(AppApiError::Format(format!(
                    "operation envelope kind {} does not match spreadsheet payload {}",
                    self.record.kind,
                    spreadsheet.operation_kind()
                )));
            }
        }
        Ok(())
    }
}

pub(crate) fn validate_operation_envelopes(
    envelopes: &[AppOperationEnvelope],
) -> Result<(), AppApiError> {
    let mut seen = BTreeSet::new();
    for envelope in envelopes {
        envelope.validate_source()?;
        if !seen.insert((envelope.record.actor.clone(), envelope.record.seq)) {
            return Err(AppApiError::Format(format!(
                "duplicate operation envelope {}#{}",
                envelope.record.actor, envelope.record.seq
            )));
        }
    }
    Ok(())
}

pub(crate) fn validate_operation_segment_envelopes(
    envelopes: &[AppOperationEnvelope],
) -> Result<(), AppApiError> {
    if envelopes.is_empty() {
        return Err(AppApiError::Format(
            "operation segment operations are empty".to_string(),
        ));
    }
    validate_operation_envelopes(envelopes)
}

fn validate_rich_document_operation_source(kind: &OperationKind) -> Result<(), AppApiError> {
    match kind {
        OperationKind::SetDocumentTitle { title } => {
            validate_canonical_operation_field("document title", title)
        }
        OperationKind::SetDocumentDoi { doi } => {
            if let Some(doi) = doi {
                validate_canonical_operation_field("document DOI", doi)?;
            }
            Ok(())
        }
        OperationKind::SetDocumentLocale { locale } => {
            validate_canonical_operation_field("document locale", locale)
        }
        OperationKind::InsertBlock { after, block } => {
            validate_optional_stable_id(after)?;
            block
                .validate_isolated()
                .map_err(|err| AppApiError::Format(err.to_string()))
        }
        OperationKind::DeleteBlock { block_id } => validate_stable_operation_id(block_id),
        OperationKind::SetBlockTextStyle { block_id, style } => {
            validate_stable_operation_id(block_id)?;
            match style {
                BlockTextStyle::Paragraph => Ok(()),
                BlockTextStyle::Heading { level } if (1..=6).contains(level) => Ok(()),
                BlockTextStyle::Heading { .. } => Err(AppApiError::Format(
                    "heading level is outside 1..=6".to_string(),
                )),
                BlockTextStyle::ListItem { list_id, level, .. } if *level <= 8 => {
                    validate_stable_operation_id(list_id)?;
                    Ok(())
                }
                BlockTextStyle::ListItem { .. } => Err(AppApiError::Format(
                    "list item level is outside 0..=8".to_string(),
                )),
            }
        }
        OperationKind::InsertInline {
            block_id,
            after,
            inline,
        } => {
            validate_stable_operation_id(block_id)?;
            validate_optional_stable_id(after)?;
            inline
                .validate()
                .map_err(|err| AppApiError::Format(err.to_string()))
        }
        OperationKind::MoveInlineToBlock {
            inline_id,
            target_block_id,
            after,
        } => {
            validate_stable_operation_id(inline_id)?;
            validate_stable_operation_id(target_block_id)?;
            validate_optional_stable_id(after)
        }
        OperationKind::AddMark { text_id, mark } => {
            validate_stable_operation_id(text_id)?;
            mark.validate()
                .map_err(|err| AppApiError::Format(err.to_string()))
        }
        OperationKind::RemoveMark {
            text_id,
            kind,
            value,
        } => {
            validate_stable_operation_id(text_id)?;
            validate_mark_removal_payload(kind, value.as_deref())
        }
        OperationKind::AddMarkRange { range, mark } => {
            range
                .validate()
                .map_err(|err| AppApiError::Format(err.to_string()))?;
            mark.validate()
                .map_err(|err| AppApiError::Format(err.to_string()))
        }
        OperationKind::AddSuggestion { suggestion } => suggestion
            .validate()
            .map_err(|err| AppApiError::Format(err.to_string())),
        OperationKind::UpdateSuggestionInsertContent {
            suggestion_id,
            content,
        } => {
            validate_stable_operation_id(suggestion_id)?;
            validate_inline_operation_body(content, "suggestion content")
        }
        OperationKind::AddCommentThread { thread } => thread
            .validate()
            .map_err(|err| AppApiError::Format(err.to_string())),
        OperationKind::AddCommentReply { thread_id, comment } => {
            validate_stable_operation_id(thread_id)?;
            comment
                .validate()
                .map_err(|err| AppApiError::Format(err.to_string()))
        }
        OperationKind::UpsertFootnote { footnote } => footnote
            .validate()
            .map_err(|err| AppApiError::Format(err.to_string())),
        OperationKind::UpsertBibliographyReference { reference } => reference
            .validate()
            .map_err(|err| AppApiError::Format(err.to_string())),
        OperationKind::DeleteBibliographyReference {
            reference_id,
            revision,
        } => {
            validate_stable_operation_id(reference_id)?;
            validate_nonzero_operation_revision(*revision, "bibliography reference revision")
        }
        OperationKind::UpsertCitationGroup { citation } => citation
            .validate_payload()
            .map_err(|err| AppApiError::Format(err.to_string())),
        OperationKind::DeleteCitationGroup {
            citation_id,
            revision,
        } => {
            validate_stable_operation_id(citation_id)?;
            validate_nonzero_operation_revision(*revision, "citation group revision")
        }
        OperationKind::UpdateCitationStyle { style, locale } => {
            validate_canonical_operation_field("citation style", style)?;
            validate_canonical_operation_field("citation locale", locale)
        }
        OperationKind::DeleteCommentThread { thread_id }
        | OperationKind::RestoreCommentThread { thread_id } => {
            validate_stable_operation_id(thread_id)
        }
        OperationKind::DeleteComment {
            thread_id,
            comment_id,
        }
        | OperationKind::RestoreComment {
            thread_id,
            comment_id,
        } => {
            validate_stable_operation_id(thread_id)?;
            validate_stable_operation_id(comment_id)
        }
        OperationKind::UpdateCommentBody {
            thread_id,
            comment_id,
            body,
        } => {
            validate_stable_operation_id(thread_id)?;
            validate_stable_operation_id(comment_id)?;
            let comment = Comment {
                id: comment_id.clone(),
                author: "operation-validator".to_string(),
                body: body.clone(),
                created_at_ms: 1,
                deleted: false,
            };
            comment
                .validate()
                .map_err(|err| AppApiError::Format(err.to_string()))
        }
        OperationKind::UpdateInlineText { inline_id, .. } => {
            validate_stable_operation_id(inline_id)
        }
        OperationKind::InsertText {
            inline_id, text, ..
        } => {
            validate_stable_operation_id(inline_id)?;
            if text.is_empty() {
                return Err(AppApiError::Format(
                    "insert-text operation has empty text".to_string(),
                ));
            }
            Ok(())
        }
        OperationKind::DeleteText {
            inline_id,
            start,
            end,
        } => {
            validate_stable_operation_id(inline_id)?;
            if end <= start {
                return Err(AppApiError::Format(
                    "delete-text operation has an empty range".to_string(),
                ));
            }
            Ok(())
        }
        OperationKind::UpdateInlineEquationSource { inline_id, source } => {
            validate_stable_operation_id(inline_id)?;
            validate_canonical_operation_field("inline equation source", source)
        }
        OperationKind::UpdateMentionLabel { inline_id, label } => {
            validate_stable_operation_id(inline_id)?;
            validate_canonical_operation_field("mention label", label)
        }
        OperationKind::UpdateLinkHref { inline_id, href } => {
            validate_stable_operation_id(inline_id)?;
            validate_canonical_operation_field("link href", href)
        }
        OperationKind::UpdateBlockEquationSource { block_id, source } => {
            validate_stable_operation_id(block_id)?;
            validate_canonical_operation_field("block equation source", source)
        }
        OperationKind::UpdateImageAltText { block_id, .. } => {
            validate_stable_operation_id(block_id)
        }
        OperationKind::UpdateImageBlobHash {
            block_id,
            blob_hash,
        } => {
            validate_stable_operation_id(block_id)?;
            opendoc_core::HashRef::parse(blob_hash)
                .map(|_| ())
                .map_err(|err| AppApiError::Format(err.to_string()))
        }
        OperationKind::UpdateHeadingLevel { block_id, level } => {
            validate_stable_operation_id(block_id)?;
            if !(1..=6).contains(level) {
                return Err(AppApiError::Format(
                    "heading level is outside 1..=6".to_string(),
                ));
            }
            Ok(())
        }
        OperationKind::UpdateListItem {
            block_id, level, ..
        } => {
            validate_stable_operation_id(block_id)?;
            if *level > 8 {
                return Err(AppApiError::Format(
                    "list item level is outside 0..=8".to_string(),
                ));
            }
            Ok(())
        }
        OperationKind::AcceptSuggestion {
            suggestion_id,
            accepted_by,
        } => {
            validate_stable_operation_id(suggestion_id)?;
            validate_canonical_operation_field("accepted by", accepted_by)
        }
        OperationKind::RejectSuggestion {
            suggestion_id,
            rejected_by,
        } => {
            validate_stable_operation_id(suggestion_id)?;
            validate_canonical_operation_field("rejected by", rejected_by)
        }
        OperationKind::DeleteInline { inline_id } => validate_stable_operation_id(inline_id),
        OperationKind::InsertTableRow {
            table_block_id,
            after_row,
            row,
        } => {
            validate_stable_operation_id(table_block_id)?;
            validate_optional_stable_id(after_row)?;
            validate_table_row_payload(row)
        }
        OperationKind::DeleteTableRow {
            table_block_id,
            row_id,
        } => {
            validate_stable_operation_id(table_block_id)?;
            validate_stable_operation_id(row_id)
        }
        OperationKind::InsertTableCell {
            table_block_id,
            row_id,
            after_cell,
            cell,
        } => {
            validate_stable_operation_id(table_block_id)?;
            validate_stable_operation_id(row_id)?;
            validate_optional_stable_id(after_cell)?;
            validate_table_cell_payload(cell)
        }
        OperationKind::DeleteTableCell {
            table_block_id,
            row_id,
            cell_id,
        } => {
            validate_stable_operation_id(table_block_id)?;
            validate_stable_operation_id(row_id)?;
            validate_stable_operation_id(cell_id)
        }
    }
}

fn validate_stable_operation_id(id: &StableId) -> Result<(), AppApiError> {
    StableId::parse(id.as_str())
        .map(|_| ())
        .map_err(|err| AppApiError::Format(err.to_string()))
}

fn validate_optional_stable_id(id: &Option<StableId>) -> Result<(), AppApiError> {
    if let Some(id) = id {
        validate_stable_operation_id(id)?;
    }
    Ok(())
}

fn validate_nonzero_operation_revision(
    revision: u64,
    label: &'static str,
) -> Result<(), AppApiError> {
    if revision == 0 {
        return Err(AppApiError::Format(format!("{label} is zero")));
    }
    Ok(())
}

fn validate_inline_operation_body(inlines: &[Inline], label: &str) -> Result<(), AppApiError> {
    if inlines.is_empty() || inlines.iter().all(inline_is_empty_source_text) {
        return Err(AppApiError::Format(format!("{label} is empty")));
    }
    for inline in inlines {
        inline
            .validate()
            .map_err(|err| AppApiError::Format(err.to_string()))?;
    }
    Ok(())
}

fn inline_is_empty_source_text(inline: &Inline) -> bool {
    match inline {
        Inline::Text { text, .. } | Inline::Link { text, .. } => text.trim().is_empty(),
        Inline::Mention { .. }
        | Inline::Equation { .. }
        | Inline::Citation { .. }
        | Inline::FootnoteRef { .. } => false,
    }
}

fn validate_table_row_payload(row: &opendoc_core::TableRow) -> Result<(), AppApiError> {
    let block = Block {
        id: StableId::new("table-validator"),
        kind: BlockKind::Table {
            rows: vec![row.clone()],
        },
        content: Vec::new(),
        properties: Vec::new(),
    };
    block
        .validate_isolated()
        .map_err(|err| AppApiError::Format(err.to_string()))
}

fn validate_table_cell_payload(cell: &opendoc_core::TableCell) -> Result<(), AppApiError> {
    let row = opendoc_core::TableRow {
        id: StableId::new("row-validator"),
        cells: vec![cell.clone()],
    };
    validate_table_row_payload(&row)
}

fn rich_document_operation_kind(kind: &OperationKind) -> &'static str {
    match kind {
        OperationKind::SetDocumentTitle { .. } => "set-document-title",
        OperationKind::SetDocumentDoi { .. } => "set-document-doi",
        OperationKind::SetDocumentLocale { .. } => "set-document-locale",
        OperationKind::InsertBlock { .. } => "insert-block",
        OperationKind::DeleteBlock { .. } => "delete-block",
        OperationKind::InsertInline { .. } => "insert-inline",
        OperationKind::MoveInlineToBlock { .. } => "move-inline-to-block",
        OperationKind::AddMark { .. } => "add-mark",
        OperationKind::RemoveMark { .. } => "remove-mark",
        OperationKind::AddMarkRange { .. } => "add-mark-range",
        OperationKind::AddSuggestion { .. } => "add-suggestion",
        OperationKind::UpdateSuggestionInsertContent { .. } => "update-suggestion",
        OperationKind::AddCommentThread { .. } => "add-comment-thread",
        OperationKind::AddCommentReply { .. } => "add-comment-reply",
        OperationKind::UpsertFootnote { .. } => "upsert-footnote",
        OperationKind::UpsertBibliographyReference { .. } => "upsert-bibliography-reference",
        OperationKind::DeleteBibliographyReference { .. } => "delete-bibliography-reference",
        OperationKind::UpsertCitationGroup { .. } => "upsert-citation-group",
        OperationKind::DeleteCitationGroup { .. } => "delete-citation-group",
        OperationKind::UpdateCitationStyle { .. } => "set-citation-style",
        OperationKind::DeleteCommentThread { .. } => "delete-comment-thread",
        OperationKind::RestoreCommentThread { .. } => "restore-comment-thread",
        OperationKind::DeleteComment { .. } => "delete-comment",
        OperationKind::RestoreComment { .. } => "restore-comment",
        OperationKind::UpdateCommentBody { .. } => "update-comment",
        OperationKind::UpdateInlineText { .. } => "update-inline-text",
        OperationKind::InsertText { .. } => "insert-text",
        OperationKind::DeleteText { .. } => "delete-text",
        OperationKind::UpdateInlineEquationSource { .. } => "update-inline-equation-source",
        OperationKind::UpdateMentionLabel { .. } => "update-mention-label",
        OperationKind::UpdateLinkHref { .. } => "update-link-href",
        OperationKind::UpdateBlockEquationSource { .. } => "update-block-equation-source",
        OperationKind::UpdateImageAltText { .. } => "update-image-alt-text",
        OperationKind::UpdateImageBlobHash { .. } => "update-image-blob-hash",
        OperationKind::SetBlockTextStyle { .. } => "set-block-text-style",
        OperationKind::UpdateHeadingLevel { .. } => "update-heading-level",
        OperationKind::UpdateListItem { .. } => "update-list-item",
        OperationKind::AcceptSuggestion { .. } => "accept-suggestion",
        OperationKind::RejectSuggestion { .. } => "reject-suggestion",
        OperationKind::DeleteInline { .. } => "delete-inline",
        OperationKind::InsertTableRow { .. } => "insert-table-row",
        OperationKind::DeleteTableRow { .. } => "delete-table-row",
        OperationKind::InsertTableCell { .. } => "insert-table-cell",
        OperationKind::DeleteTableCell { .. } => "delete-table-cell",
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub(crate) enum AppBlobOperation {
    Add {
        id: String,
        name: String,
        media_type: String,
        hash: String,
        size: u64,
    },
    UpdateMetadata {
        hash: String,
        name: String,
        media_type: String,
    },
    Delete {
        hash: String,
        #[serde(default)]
        typed_signatures: Vec<AppTypedContentSignature>,
    },
    Restore {
        id: String,
        name: String,
        media_type: String,
        hash: String,
        size: u64,
        #[serde(default)]
        typed_signatures: Vec<AppTypedContentSignature>,
    },
    ArchiveTombstone {
        hash: String,
        archive_tombstone: AppArchiveTombstone,
    },
}

impl AppBlobOperation {
    fn operation_kind(&self) -> &'static str {
        match self {
            Self::Add { .. } => "add-binary-blob",
            Self::UpdateMetadata { .. } => "update-binary-blob-metadata",
            Self::Delete { .. } => "delete-binary-blob",
            Self::Restore { .. } => "restore-binary-blob",
            Self::ArchiveTombstone { .. } => "record-blob-archive-tombstone",
        }
    }

    fn validate_source(&self) -> Result<(), AppApiError> {
        match self {
            Self::Add {
                id,
                name,
                media_type,
                hash,
                ..
            }
            | Self::Restore {
                id,
                name,
                media_type,
                hash,
                ..
            } => {
                parse_id(id)?;
                validate_blob_operation_name(name)?;
                validate_blob_operation_media_type(media_type)?;
                opendoc_core::HashRef::parse(hash)
                    .map_err(|err| AppApiError::Format(err.to_string()))?;
            }
            Self::UpdateMetadata {
                hash,
                name,
                media_type,
            } => {
                opendoc_core::HashRef::parse(hash)
                    .map_err(|err| AppApiError::Format(err.to_string()))?;
                validate_blob_operation_name(name)?;
                validate_blob_operation_media_type(media_type)?;
            }
            Self::Delete { hash, .. } => {
                opendoc_core::HashRef::parse(hash)
                    .map_err(|err| AppApiError::Format(err.to_string()))?;
            }
            Self::ArchiveTombstone {
                hash,
                archive_tombstone,
            } => {
                opendoc_core::HashRef::parse(hash)
                    .map_err(|err| AppApiError::Format(err.to_string()))?;
                archive_tombstone.validate_source()?;
            }
        }

        match self {
            Self::Delete {
                hash,
                typed_signatures,
            }
            | Self::Restore {
                hash,
                typed_signatures,
                ..
            } => {
                for signature in typed_signatures {
                    signature.validate_source(hash)?;
                }
            }
            Self::Add { .. } | Self::UpdateMetadata { .. } => {}
            Self::ArchiveTombstone { .. } => {}
        }
        Ok(())
    }
}

fn validate_blob_operation_name(name: &str) -> Result<(), AppApiError> {
    if name.trim().is_empty() {
        return Err(AppApiError::Format(
            "blob operation name is empty".to_string(),
        ));
    }
    Ok(())
}

fn validate_blob_operation_media_type(media_type: &str) -> Result<(), AppApiError> {
    if media_type.trim().is_empty() {
        return Err(AppApiError::Format(
            "blob operation media type is empty".to_string(),
        ));
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub(crate) enum AppSpreadsheetOperation {
    SetWorkbookMetadata {
        title: String,
        locale: String,
        timezone: String,
    },
    AddSheet {
        sheet_id: String,
        title: String,
    },
    RenameSheet {
        sheet_id: String,
        title: String,
    },
    DeleteSheet {
        sheet_id: String,
        sheet: AppSheet,
        #[serde(default)]
        named_ranges: Vec<AppNamedRange>,
    },
    RestoreSheet {
        sheet_id: String,
        sheet: AppSheet,
        #[serde(default)]
        named_ranges: Vec<AppNamedRange>,
    },
    AddRow {
        sheet_id: String,
        row: String,
    },
    DeleteRow {
        sheet_id: String,
        row: String,
        payload: AppDeletedRowPayload,
    },
    RestoreRow {
        sheet_id: String,
        row: String,
        payload: AppDeletedRowPayload,
    },
    AddColumn {
        sheet_id: String,
        column: String,
    },
    DeleteColumn {
        sheet_id: String,
        column: String,
        payload: AppDeletedColumnPayload,
    },
    RestoreColumn {
        sheet_id: String,
        column: String,
        payload: AppDeletedColumnPayload,
    },
    AddCellComment {
        sheet_id: String,
        address: String,
        comment_id: String,
        author: String,
        body: String,
    },
    UpdateCellComment {
        comment_id: String,
        body: String,
    },
    DeleteCellComment {
        comment_id: String,
    },
    RestoreCellComment {
        comment_id: String,
    },
    SetFrozenAxes {
        sheet_id: String,
        frozen_rows: u32,
        frozen_columns: u32,
    },
    SetCellValidation {
        sheet_id: String,
        address: String,
        validation: AppCellValidation,
    },
    ClearCellValidation {
        sheet_id: String,
        address: String,
        validation: AppCellValidation,
    },
    RestoreCellValidation {
        sheet_id: String,
        address: String,
        validation: AppCellValidation,
    },
    MergeCells {
        sheet_id: String,
        range: String,
    },
    UnmergeCells {
        sheet_id: String,
        range: String,
        merge: AppSheetMerge,
    },
    RestoreMerge {
        sheet_id: String,
        range: String,
        merge: AppSheetMerge,
    },
    SetBasicFilter {
        sheet_id: String,
        range: String,
    },
    SetBasicFilterOptions {
        sheet_id: String,
        criteria: Vec<AppSheetFilterCriterion>,
        sort_specs: Vec<AppSheetFilterSortSpec>,
    },
    ClearBasicFilter {
        sheet_id: String,
        filter: AppSheetFilter,
    },
    RestoreBasicFilter {
        sheet_id: String,
        filter: AppSheetFilter,
    },
    AddProtectedRange {
        sheet_id: String,
        range: String,
        description: String,
        warning_only: bool,
    },
    UpdateProtectedRange {
        sheet_id: String,
        range: String,
        description: String,
        warning_only: bool,
    },
    DeleteProtectedRange {
        sheet_id: String,
        range: String,
        protected_range: AppSheetProtectedRange,
    },
    RestoreProtectedRange {
        sheet_id: String,
        range: String,
        protected_range: AppSheetProtectedRange,
    },
    SetCell {
        sheet_id: String,
        address: String,
        value: String,
    },
    SetCellFormat {
        sheet_id: String,
        address: String,
        property: String,
        value: String,
    },
    CopyRange {
        sheet_id: String,
        source_range: String,
        target_address: String,
    },
    AddNamedRange {
        sheet_id: String,
        name: String,
        range: String,
    },
    UpdateNamedRange {
        sheet_id: String,
        name: String,
        range: String,
    },
    DeleteNamedRange {
        name: String,
        range: AppNamedRange,
    },
    RestoreNamedRange {
        name: String,
        range: AppNamedRange,
    },
}

impl AppSpreadsheetOperation {
    fn operation_kind(&self) -> &'static str {
        match self {
            Self::SetWorkbookMetadata { .. } => "set-spreadsheet-workbook-metadata",
            Self::AddSheet { .. } => "add-spreadsheet-sheet",
            Self::RenameSheet { .. } => "rename-spreadsheet-sheet",
            Self::DeleteSheet { .. } => "delete-spreadsheet-sheet",
            Self::RestoreSheet { .. } => "restore-spreadsheet-sheet",
            Self::AddRow { .. } => "add-spreadsheet-row",
            Self::DeleteRow { .. } => "delete-spreadsheet-row",
            Self::RestoreRow { .. } => "restore-spreadsheet-row",
            Self::AddColumn { .. } => "add-spreadsheet-column",
            Self::DeleteColumn { .. } => "delete-spreadsheet-column",
            Self::RestoreColumn { .. } => "restore-spreadsheet-column",
            Self::AddCellComment { .. } => "add-spreadsheet-cell-comment",
            Self::UpdateCellComment { .. } => "update-spreadsheet-cell-comment",
            Self::DeleteCellComment { .. } => "delete-spreadsheet-cell-comment",
            Self::RestoreCellComment { .. } => "restore-spreadsheet-cell-comment",
            Self::SetFrozenAxes { .. } => "set-spreadsheet-frozen-axes",
            Self::SetCellValidation { .. } => "set-spreadsheet-cell-validation",
            Self::ClearCellValidation { .. } => "clear-spreadsheet-cell-validation",
            Self::RestoreCellValidation { .. } => "restore-spreadsheet-cell-validation",
            Self::MergeCells { .. } => "merge-spreadsheet-cells",
            Self::UnmergeCells { .. } => "unmerge-spreadsheet-cells",
            Self::RestoreMerge { .. } => "restore-spreadsheet-merge",
            Self::SetBasicFilter { .. } => "set-spreadsheet-basic-filter",
            Self::SetBasicFilterOptions { .. } => "set-spreadsheet-basic-filter-options",
            Self::ClearBasicFilter { .. } => "clear-spreadsheet-basic-filter",
            Self::RestoreBasicFilter { .. } => "restore-spreadsheet-basic-filter",
            Self::AddProtectedRange { .. } => "add-spreadsheet-protected-range",
            Self::UpdateProtectedRange { .. } => "update-spreadsheet-protected-range",
            Self::DeleteProtectedRange { .. } => "delete-spreadsheet-protected-range",
            Self::RestoreProtectedRange { .. } => "restore-spreadsheet-protected-range",
            Self::SetCell { .. } => "set-spreadsheet-cell",
            Self::SetCellFormat { .. } => "set-spreadsheet-cell-format",
            Self::CopyRange { .. } => "copy-spreadsheet-range",
            Self::AddNamedRange { .. } => "add-spreadsheet-named-range",
            Self::UpdateNamedRange { .. } => "update-spreadsheet-named-range",
            Self::DeleteNamedRange { .. } => "delete-spreadsheet-named-range",
            Self::RestoreNamedRange { .. } => "restore-spreadsheet-named-range",
        }
    }

    fn validate_source(&self) -> Result<(), AppApiError> {
        match self {
            Self::SetWorkbookMetadata {
                title,
                locale,
                timezone,
            } => {
                validate_canonical_operation_field("spreadsheet workbook title", title)?;
                validate_canonical_operation_field("spreadsheet workbook locale", locale)?;
                validate_canonical_operation_field("spreadsheet workbook timezone", timezone)?;
            }
            Self::AddSheet { sheet_id, title } | Self::RenameSheet { sheet_id, title } => {
                validate_canonical_sheet_id(sheet_id)?;
                validate_canonical_operation_field("spreadsheet sheet title", title)?;
            }
            Self::DeleteSheet {
                sheet_id,
                sheet,
                named_ranges,
            }
            | Self::RestoreSheet {
                sheet_id,
                sheet,
                named_ranges,
            } => {
                validate_canonical_sheet_id(sheet_id)?;
                if sheet.id != *sheet_id {
                    return Err(AppApiError::Format(format!(
                        "spreadsheet sheet operation id {} does not match payload sheet {}",
                        sheet_id, sheet.id
                    )));
                }
                sheet.validate_source()?;
                for range in named_ranges {
                    range.validate_source()?;
                    if range.sheet_id != *sheet_id {
                        return Err(AppApiError::Format(format!(
                            "spreadsheet sheet operation named range {} references {} instead of {}",
                            range.name, range.sheet_id, sheet_id
                        )));
                    }
                }
            }
            Self::ClearBasicFilter { sheet_id, filter }
            | Self::RestoreBasicFilter { sheet_id, filter } => {
                validate_canonical_sheet_id(sheet_id)?;
                filter.validate_source()?;
            }
            Self::AddRow { sheet_id, row } => {
                validate_canonical_sheet_id(sheet_id)?;
                validate_canonical_row_label(row)?;
            }
            Self::DeleteRow {
                sheet_id,
                row,
                payload,
            }
            | Self::RestoreRow {
                sheet_id,
                row,
                payload,
            } => {
                validate_canonical_sheet_id(sheet_id)?;
                validate_canonical_row_label(row)?;
                payload.validate_source(row)?;
            }
            Self::AddColumn { sheet_id, column } => {
                validate_canonical_sheet_id(sheet_id)?;
                validate_canonical_column_label(column)?;
            }
            Self::DeleteColumn {
                sheet_id,
                column,
                payload,
            }
            | Self::RestoreColumn {
                sheet_id,
                column,
                payload,
            } => {
                validate_canonical_sheet_id(sheet_id)?;
                validate_canonical_column_label(column)?;
                payload.validate_source(column)?;
            }
            Self::AddCellComment {
                sheet_id,
                address,
                comment_id,
                author,
                body,
            } => {
                validate_canonical_sheet_id(sheet_id)?;
                validate_canonical_cell_address("spreadsheet comment operation address", address)?;
                parse_id(comment_id)?;
                validate_canonical_operation_field("spreadsheet comment author", author)?;
                validate_non_empty_operation_field("spreadsheet comment body", body)?;
            }
            Self::UpdateCellComment { comment_id, body } => {
                parse_id(comment_id)?;
                validate_non_empty_operation_field("spreadsheet comment body", body)?;
            }
            Self::DeleteCellComment { comment_id } | Self::RestoreCellComment { comment_id } => {
                parse_id(comment_id)?;
            }
            Self::SetFrozenAxes { sheet_id, .. } => {
                validate_canonical_sheet_id(sheet_id)?;
            }
            Self::SetCellValidation {
                sheet_id,
                address,
                validation,
            } => {
                validate_canonical_sheet_id(sheet_id)?;
                validate_canonical_cell_address(
                    "spreadsheet validation operation address",
                    address,
                )?;
                validation.validate_source()?;
            }
            Self::ClearCellValidation {
                sheet_id,
                address,
                validation,
            }
            | Self::RestoreCellValidation {
                sheet_id,
                address,
                validation,
            } => {
                validate_canonical_sheet_id(sheet_id)?;
                validate_canonical_cell_address(
                    "spreadsheet validation operation address",
                    address,
                )?;
                validation.validate_source()?;
            }
            Self::SetCell {
                sheet_id, address, ..
            } => {
                validate_canonical_sheet_id(sheet_id)?;
                validate_canonical_cell_address("spreadsheet cell operation address", address)?;
            }
            Self::MergeCells { sheet_id, range } => {
                validate_canonical_sheet_id(sheet_id)?;
                validate_canonical_merge_range("spreadsheet merge operation range", range)?;
            }
            Self::UnmergeCells {
                sheet_id,
                range,
                merge,
            }
            | Self::RestoreMerge {
                sheet_id,
                range,
                merge,
            } => {
                validate_canonical_sheet_id(sheet_id)?;
                validate_canonical_merge_range("spreadsheet merge operation range", range)?;
                merge.validate_source()?;
                if merge.range != *range {
                    return Err(AppApiError::Format(format!(
                        "spreadsheet merge operation range {} does not match payload {}",
                        range, merge.range
                    )));
                }
            }
            Self::SetBasicFilter { sheet_id, range } => {
                validate_canonical_sheet_id(sheet_id)?;
                validate_canonical_cell_range("spreadsheet filter operation range", range)?;
            }
            Self::SetBasicFilterOptions {
                sheet_id,
                criteria,
                sort_specs,
            } => {
                validate_canonical_sheet_id(sheet_id)?;
                validate_filter_option_payload(criteria, sort_specs)?;
            }
            Self::AddProtectedRange {
                sheet_id,
                range,
                description,
                warning_only,
            }
            | Self::UpdateProtectedRange {
                sheet_id,
                range,
                description,
                warning_only,
            } => {
                validate_canonical_sheet_id(sheet_id)?;
                validate_canonical_cell_range(
                    "spreadsheet protected range operation range",
                    range,
                )?;
                validate_protected_range_description(description)?;
                if !warning_only {
                    return Err(AppApiError::Format(
                        "spreadsheet protected range operation is not warning-only".to_string(),
                    ));
                }
            }
            Self::DeleteProtectedRange {
                sheet_id,
                range,
                protected_range,
            }
            | Self::RestoreProtectedRange {
                sheet_id,
                range,
                protected_range,
            } => {
                validate_canonical_sheet_id(sheet_id)?;
                validate_canonical_cell_range(
                    "spreadsheet protected range operation range",
                    range,
                )?;
                protected_range.validate_source()?;
                if protected_range.range != *range {
                    return Err(AppApiError::Format(format!(
                        "spreadsheet protected range operation range {} does not match payload {}",
                        range, protected_range.range
                    )));
                }
            }
            Self::SetCellFormat {
                sheet_id,
                address,
                property,
                value,
            } => {
                validate_canonical_sheet_id(sheet_id)?;
                validate_canonical_cell_address("spreadsheet format operation address", address)?;
                validate_cell_format_operation_property(property, value)?;
            }
            Self::CopyRange {
                sheet_id,
                source_range,
                target_address,
            } => {
                validate_canonical_sheet_id(sheet_id)?;
                validate_canonical_cell_range(
                    "spreadsheet copy operation source range",
                    source_range,
                )?;
                validate_canonical_cell_address(
                    "spreadsheet copy operation target address",
                    target_address,
                )?;
            }
            Self::AddNamedRange {
                sheet_id,
                name,
                range,
            }
            | Self::UpdateNamedRange {
                sheet_id,
                name,
                range,
            } => {
                validate_canonical_sheet_id(sheet_id)?;
                let normalized_name = normalize_named_range_name(name)?;
                if *name != normalized_name {
                    return Err(AppApiError::Format(format!(
                        "spreadsheet named range operation name {} is not canonical",
                        name
                    )));
                }
                validate_canonical_cell_range("spreadsheet named range operation range", range)?;
            }
            Self::DeleteNamedRange { name, range } | Self::RestoreNamedRange { name, range } => {
                let normalized_name = normalize_named_range_name(name)?;
                if *name != normalized_name {
                    return Err(AppApiError::Format(format!(
                        "spreadsheet named range operation name {} is not canonical",
                        name
                    )));
                }
                range.validate_source()?;
                if range.name != *name {
                    return Err(AppApiError::Format(format!(
                        "spreadsheet named range operation name {} does not match payload {}",
                        name, range.name
                    )));
                }
            }
        }
        Ok(())
    }
}

fn validate_non_empty_operation_field(label: &str, value: &str) -> Result<(), AppApiError> {
    if value.trim().is_empty() {
        return Err(AppApiError::Format(format!("{label} is empty")));
    }
    Ok(())
}

fn validate_canonical_operation_field(label: &str, value: &str) -> Result<(), AppApiError> {
    validate_non_empty_operation_field(label, value)?;
    if value.trim() != value {
        return Err(AppApiError::Format(format!(
            "{label} has surrounding whitespace"
        )));
    }
    Ok(())
}

fn validate_optional_canonical_operation_field(
    label: &str,
    value: &str,
) -> Result<(), AppApiError> {
    if value.trim() != value {
        return Err(AppApiError::Format(format!(
            "{label} has surrounding whitespace"
        )));
    }
    Ok(())
}

pub(crate) fn validate_filter_option_payload(
    criteria: &[AppSheetFilterCriterion],
    sort_specs: &[AppSheetFilterSortSpec],
) -> Result<(), AppApiError> {
    let mut criterion_columns = BTreeSet::new();
    for criterion in criteria {
        let column = normalize_column_label(&criterion.column)?;
        if criterion.column != column {
            return Err(AppApiError::Format(format!(
                "filter criterion column {} is not canonical",
                criterion.column
            )));
        }
        if !criterion_columns.insert(column.clone()) {
            return Err(AppApiError::Format(format!(
                "duplicate filter criterion column {column}"
            )));
        }
        if criterion.condition.trim() != criterion.condition {
            return Err(AppApiError::Format(
                "filter criterion condition has surrounding whitespace".to_string(),
            ));
        }
        validate_filter_condition(&criterion.condition)?;
        if criterion.value.trim().is_empty() {
            return Err(AppApiError::Format(
                "filter criterion value is empty".to_string(),
            ));
        }
    }
    let mut sort_columns = BTreeSet::new();
    for sort_spec in sort_specs {
        let column = normalize_column_label(&sort_spec.column)?;
        if sort_spec.column != column {
            return Err(AppApiError::Format(format!(
                "filter sort column {} is not canonical",
                sort_spec.column
            )));
        }
        if !sort_columns.insert(column.clone()) {
            return Err(AppApiError::Format(format!(
                "duplicate filter sort column {column}"
            )));
        }
    }
    Ok(())
}

pub(crate) fn normalize_filter_criteria(
    criteria: Vec<AppSheetFilterCriterion>,
) -> Result<Vec<AppSheetFilterCriterion>, AppApiError> {
    criteria
        .into_iter()
        .map(|criterion| {
            Ok(AppSheetFilterCriterion {
                column: normalize_column_label(&criterion.column)?,
                condition: criterion.condition.trim().to_string(),
                value: criterion.value,
            })
        })
        .collect()
}

pub(crate) fn normalize_filter_sort_specs(
    sort_specs: Vec<AppSheetFilterSortSpec>,
) -> Result<Vec<AppSheetFilterSortSpec>, AppApiError> {
    sort_specs
        .into_iter()
        .map(|sort_spec| {
            Ok(AppSheetFilterSortSpec {
                column: normalize_column_label(&sort_spec.column)?,
                descending: sort_spec.descending,
            })
        })
        .collect()
}

fn validate_cell_format_operation_property(property: &str, value: &str) -> Result<(), AppApiError> {
    if property.trim() != property {
        return Err(AppApiError::Format(format!(
            "spreadsheet format property {property} is not canonical; expected {}",
            property.trim()
        )));
    }
    match property {
        "bold" | "italic" => {
            parse_format_bool(value)?;
        }
        "text_color" | "background_color" => {
            if !value.trim().is_empty() {
                validate_sheet_color(value)?;
            }
        }
        "horizontal_align" => {
            if !matches!(value.trim(), "left" | "center" | "right" | "") {
                return Err(AppApiError::Format(format!(
                    "unsupported horizontal alignment {}",
                    value.trim()
                )));
            }
        }
        "number_format" => {
            validate_optional_canonical_operation_field("number format", value)?;
        }
        other => {
            return Err(AppApiError::Format(format!(
                "unsupported cell format property {other}"
            )));
        }
    }
    Ok(())
}
