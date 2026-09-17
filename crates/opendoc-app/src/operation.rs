use super::*;

/// The journal line for one envelope.
///
/// `seq` is the **envelope's** sequence number, not the operation's. The two
/// were one number until the counters were split, and that is what made a
/// blob, spreadsheet or undo envelope punch a hole in an actor's *operation*
/// numbering — a hole a collaboration service refuses, correctly, because
/// `VectorClock::observed` reads `seq >= n` as "everything up to n". An
/// envelope that carries a typed operation gets its operation's id from
/// `AppOperationEnvelope::operation`, and the two numbers are free to differ.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppOperationRecord {
    pub actor: String,
    pub seq: u64,
    pub kind: String,
    pub summary: String,
    pub created_at_ms: u64,
}

/// One journalled operation, and the payload it carries.
///
/// This is the entry an operation segment stores, so it is also the record a
/// service would have to write for a repository it produced to be openable by
/// a local save (see `docs/adr/0015`, "What remains"). Exactly one of
/// `operation`, `spreadsheet` and `blob` may be set; an envelope with none of
/// them is a marker for something the journal records but no typed operation
/// describes, such as an undo.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppOperationEnvelope {
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
    /// The envelope a bare typed operation becomes when it arrives without
    /// one — from a collaboration service's fanout, or out of an operation
    /// segment a service wrote.
    ///
    /// The record is *derived*, never taken on trust: the actor and kind come
    /// from the operation itself, so a record can never disagree with the
    /// payload it describes. `created_at_ms` is zero because this replica does
    /// not know when the operation was authored and a local clock reading
    /// would be a fabrication; the summary is the operation kind, which is the
    /// only description available.
    ///
    /// `envelope_seq` is the number this replica's journal gives the envelope.
    /// It is *not* the operation's sequence number: a remote actor numbers its
    /// own operations, and this replica numbers its own envelopes, and neither
    /// may renumber the other.
    pub fn from_operation(operation: Operation, envelope_seq: u64) -> Self {
        let kind = rich_document_operation_kind(&operation.kind);
        Self {
            record: AppOperationRecord {
                actor: operation.id.actor.0.clone(),
                seq: envelope_seq,
                kind: kind.to_string(),
                summary: kind.replace('-', " "),
                created_at_ms: 0,
            },
            operation: Some(operation),
            spreadsheet: None,
            blob: None,
        }
    }

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
            // The actor must match, because the record describes the payload.
            // The *sequence* deliberately need not: `record.seq` is the
            // envelope's identity and `operation.id.seq` is the operation's,
            // and requiring them to be equal is what forced every envelope
            // carrying no operation to burn an operation id.
            if operation.id.actor.0 != self.record.actor {
                return Err(AppApiError::Format(
                    "operation envelope record does not match rich-document operation actor"
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

/// Two uniqueness rules, because there are two identities.
///
/// `(record.actor, record.seq)` is the **envelope's**: a candidate merge and
/// the recovery segment both deduplicate on it. `operation.id` is the
/// **operation's**: it is what the causal order and last-writer-wins are
/// computed from, and two different payloads under one id is history being
/// rewritten. Checking only the first would let a journal carry one operation
/// id twice as long as the envelopes around it were numbered differently.
pub(crate) fn validate_operation_envelopes(
    envelopes: &[AppOperationEnvelope],
) -> Result<(), AppApiError> {
    let mut seen_envelopes = BTreeSet::new();
    let mut seen_operations = BTreeSet::new();
    for envelope in envelopes {
        envelope.validate_source()?;
        if !seen_envelopes.insert((envelope.record.actor.clone(), envelope.record.seq)) {
            return Err(AppApiError::Format(format!(
                "duplicate operation envelope {}#{}",
                envelope.record.actor, envelope.record.seq
            )));
        }
        if let Some(operation) = &envelope.operation {
            if !seen_operations.insert(operation.id.clone()) {
                return Err(AppApiError::Format(format!(
                    "duplicate rich-document operation {}#{}",
                    operation.id.actor.0, operation.id.seq
                )));
            }
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
        OperationKind::UpsertBookmark { bookmark } => bookmark
            .validate()
            .map_err(|err| AppApiError::Format(err.to_string())),
        OperationKind::SetPageSetup { page_setup } => page_setup
            .validate()
            .map_err(|err| AppApiError::Format(err.to_string())),
        OperationKind::SetPageFurniture { blocks, .. } => {
            for block in blocks {
                block
                    .validate_isolated()
                    .map_err(|err| AppApiError::Format(err.to_string()))?;
            }
            Ok(())
        }
        OperationKind::ClearPageFurnitureOverride { slot } => {
            if slot.is_override() {
                Ok(())
            } else {
                Err(AppApiError::Format(format!(
                    "{} is ordinary furniture and cannot inherit from itself",
                    slot.as_str()
                )))
            }
        }
        OperationKind::InsertBlock { position, block } => {
            validate_insert_position(position)?;
            block
                .validate_isolated()
                .map_err(|err| AppApiError::Format(err.to_string()))
        }
        OperationKind::InsertSection {
            before_block_id,
            boundary_id,
            section,
        } => {
            validate_stable_operation_id(before_block_id)?;
            validate_stable_operation_id(boundary_id)?;
            validate_stable_operation_id(&section.id)?;
            section
                .page_setup
                .validate()
                .map_err(|err| AppApiError::Format(err.to_string()))?;
            for slot in opendoc_core::HeaderFooterSlot::ALL {
                for block in section.furniture(slot) {
                    block
                        .validate_isolated()
                        .map_err(|err| AppApiError::Format(err.to_string()))?;
                }
            }
            Ok(())
        }
        OperationKind::DeleteSection { section_id } => validate_stable_operation_id(section_id),
        OperationKind::SetSectionPageSetup {
            section_id,
            page_setup,
        } => {
            validate_stable_operation_id(section_id)?;
            page_setup
                .validate()
                .map_err(|err| AppApiError::Format(err.to_string()))
        }
        OperationKind::SetSectionFurniture {
            section_id, blocks, ..
        } => {
            validate_stable_operation_id(section_id)?;
            for block in blocks {
                block
                    .validate_isolated()
                    .map_err(|err| AppApiError::Format(err.to_string()))?;
            }
            Ok(())
        }
        OperationKind::ClearSectionFurnitureOverride { section_id, slot } => {
            validate_stable_operation_id(section_id)?;
            if slot.is_override() {
                Ok(())
            } else {
                Err(AppApiError::Format(format!(
                    "{} is ordinary furniture and cannot inherit from itself",
                    slot.as_str()
                )))
            }
        }
        OperationKind::DeleteBlock { block_id } => validate_stable_operation_id(block_id),
        OperationKind::MoveBlock { block_id, position } => {
            validate_stable_operation_id(block_id)?;
            validate_insert_position(position)
        }
        OperationKind::SetBlockTextStyle { block_id, style } => {
            validate_stable_operation_id(block_id)?;
            match style {
                BlockTextStyle::Paragraph => Ok(()),
                BlockTextStyle::Title | BlockTextStyle::Subtitle => Ok(()),
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
            position,
            inline,
        } => {
            validate_stable_operation_id(block_id)?;
            validate_insert_position(position)?;
            inline
                .validate()
                .map_err(|err| AppApiError::Format(err.to_string()))
        }
        OperationKind::MoveInlineToBlock {
            inline_id,
            target_block_id,
            position,
        } => {
            validate_stable_operation_id(inline_id)?;
            validate_stable_operation_id(target_block_id)?;
            validate_insert_position(position)
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
        OperationKind::SetEndnotePlacement { footnote_id, .. } => {
            validate_stable_operation_id(footnote_id)
        }
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
        | OperationKind::RestoreCommentThread { thread_id }
        | OperationKind::ReopenCommentThread { thread_id } => {
            validate_stable_operation_id(thread_id)
        }
        OperationKind::SetCommentThreadAction {
            thread_id,
            assignee,
            due_at_ms,
            completed_by,
            completed_at_ms,
        } => {
            validate_stable_operation_id(thread_id)?;
            for value in [assignee.as_deref(), completed_by.as_deref()]
                .into_iter()
                .flatten()
            {
                validate_canonical_operation_field("comment action identity", value)?;
            }
            if completed_by.is_some() != completed_at_ms.is_some() {
                return Err(AppApiError::Format(
                    "comment action completion metadata is incomplete".to_string(),
                ));
            }
            if due_at_ms.is_some() && assignee.is_none() {
                return Err(AppApiError::Format(
                    "comment action due date requires an assignee".to_string(),
                ));
            }
            if completed_by.is_some() && assignee.is_none() {
                return Err(AppApiError::Format(
                    "comment action completion requires an assignee".to_string(),
                ));
            }
            Ok(())
        }
        OperationKind::SetCommentThreadReaction {
            thread_id,
            emoji,
            actor,
            ..
        } => {
            validate_stable_operation_id(thread_id)?;
            opendoc_core::validate_comment_reaction_emoji(emoji)
                .map_err(|err| AppApiError::Format(err.to_string()))?;
            validate_canonical_operation_field("comment reaction actor", actor)
        }
        OperationKind::ResolveCommentThread {
            thread_id,
            resolved_by,
            ..
        } => {
            validate_stable_operation_id(thread_id)?;
            validate_canonical_operation_field("comment resolver", resolved_by)
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
        OperationKind::SelectDropdownOption {
            inline_id,
            option_id,
        } => {
            validate_stable_operation_id(inline_id)?;
            validate_canonical_operation_field("dropdown option id", option_id)
        }
        OperationKind::UpdateDateChip { inline_id, date } => {
            validate_stable_operation_id(inline_id)?;
            Inline::DateChip {
                id: inline_id.clone(),
                date: date.clone(),
            }
            .validate()
            .map_err(|err| AppApiError::Format(err.to_string()))
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
        OperationKind::UpdateImageLayout { block_id, layout } => {
            validate_stable_operation_id(block_id)?;
            layout
                .validate()
                .map_err(|err| AppApiError::Format(err.to_string()))
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
        OperationKind::SetListStart {
            list_id,
            level,
            start,
        } => {
            validate_stable_operation_id(list_id)?;
            if *level > 8 {
                return Err(AppApiError::Format(
                    "list numbering level is outside 0..=8".to_string(),
                ));
            }
            if *start == 0 {
                return Err(AppApiError::Format(
                    "list numbering start must be positive".to_string(),
                ));
            }
            Ok(())
        }
        OperationKind::SetListFormat { list_id, level, .. } => {
            validate_stable_operation_id(list_id)?;
            if *level > 8 {
                return Err(AppApiError::Format(
                    "list numbering level is outside 0..=8".to_string(),
                ));
            }
            Ok(())
        }
        OperationKind::SetListBulletMarker { list_id, level, .. } => {
            validate_stable_operation_id(list_id)?;
            if *level > 8 {
                return Err(AppApiError::Format(
                    "list bullet marker level is outside 0..=8".to_string(),
                ));
            }
            Ok(())
        }
        OperationKind::SetBlockProperty { block_id, property } => {
            validate_stable_operation_id(block_id)?;
            property
                .validate()
                .map_err(|err| AppApiError::Format(err.to_string()))
        }
        OperationKind::ClearBlockProperty { block_id, .. } => {
            validate_stable_operation_id(block_id)
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
            position,
            row,
            cell_columns,
        } => {
            validate_stable_operation_id(table_block_id)?;
            validate_insert_position(position)?;
            // A binding that names a cell the row does not carry, or two
            // cells for one column, is a payload no replica writes. Refusing
            // it here means the merge never has to guess which of the two
            // representations to believe.
            for (cell_id, column_id) in cell_columns {
                validate_stable_operation_id(cell_id)?;
                validate_stable_operation_id(column_id)?;
                if !row.cells.iter().any(|cell| &cell.id == cell_id) {
                    return Err(AppApiError::Format(format!(
                        "table row cell binding names cell {cell_id}, which the row does not carry"
                    )));
                }
            }
            let bound_columns: std::collections::BTreeSet<&StableId> =
                cell_columns.values().collect();
            if bound_columns.len() != cell_columns.len() {
                return Err(AppApiError::Format(
                    "table row cell bindings name one column twice".to_string(),
                ));
            }
            if !cell_columns.is_empty() && cell_columns.len() != row.cells.len() {
                return Err(AppApiError::Format(
                    "table row cell bindings do not cover every cell of the row".to_string(),
                ));
            }
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
            position,
            cell,
        } => {
            validate_stable_operation_id(table_block_id)?;
            validate_stable_operation_id(row_id)?;
            validate_insert_position(position)?;
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
        OperationKind::InsertTableColumn {
            table_block_id,
            position,
            column,
        } => {
            validate_stable_operation_id(table_block_id)?;
            validate_insert_position(position)?;
            validate_stable_operation_id(&column.id)?;
            column
                .validate()
                .map_err(|err| AppApiError::Format(err.to_string()))
        }
        OperationKind::DeleteTableColumn {
            table_block_id,
            column_id,
        } => {
            validate_stable_operation_id(table_block_id)?;
            validate_stable_operation_id(column_id)
        }
        OperationKind::SetTableColumnWidth {
            table_block_id,
            column_id,
            width,
        } => {
            validate_stable_operation_id(table_block_id)?;
            validate_stable_operation_id(column_id)?;
            match width {
                Some(width) if width.twips() < opendoc_core::TableColumn::MIN_WIDTH_TWIPS => Err(
                    AppApiError::Format("table column width is below 0.1in".to_string()),
                ),
                _ => Ok(()),
            }
        }
        OperationKind::SetTableRowHeight {
            table_block_id,
            row_id,
            height,
        } => {
            validate_stable_operation_id(table_block_id)?;
            validate_stable_operation_id(row_id)?;
            match height {
                Some(height) if height.is_negative() || height.twips() == 0 => Err(
                    AppApiError::Format("table row height must be positive".to_string()),
                ),
                _ => Ok(()),
            }
        }
        OperationKind::SetTableRowHeader {
            table_block_id,
            row_id,
            ..
        } => {
            validate_stable_operation_id(table_block_id)?;
            validate_stable_operation_id(row_id)
        }
        OperationKind::ReorderTableRows {
            table_block_id,
            row_ids,
        } => {
            validate_stable_operation_id(table_block_id)?;
            if row_ids.is_empty() {
                return Err(AppApiError::Format("table row order is empty".to_string()));
            }
            for row_id in row_ids {
                validate_stable_operation_id(row_id)?;
            }
            Ok(())
        }
        OperationKind::SetTableBorder {
            table_block_id,
            border,
        } => {
            validate_stable_operation_id(table_block_id)?;
            border
                .map(opendoc_core::CellBorder::validate)
                .transpose()
                .map(|_| ())
                .map_err(|err| AppApiError::Format(err.to_string()))
        }
        OperationKind::SetTableAlignment {
            table_block_id,
            alignment: _,
        } => {
            validate_stable_operation_id(table_block_id)?;
            Ok(())
        }
        OperationKind::SetTableCellSpan { cell_id, span } => {
            validate_stable_operation_id(cell_id)?;
            span.validate()
                .map_err(|err| AppApiError::Format(err.to_string()))
        }
        OperationKind::SetTableCellProperty { cell_id, property } => {
            validate_stable_operation_id(cell_id)?;
            property
                .validate()
                .map_err(|err| AppApiError::Format(err.to_string()))
        }
        OperationKind::ClearTableCellProperty { cell_id, .. } => {
            validate_stable_operation_id(cell_id)
        }
    }
}

fn validate_stable_operation_id(id: &StableId) -> Result<(), AppApiError> {
    StableId::parse(id.as_str())
        .map(|_| ())
        .map_err(|err| AppApiError::Format(err.to_string()))
}

/// An anchored position has to name a well-formed sibling id; `First` and
/// `Last` name none, so there is nothing to check.
fn validate_insert_position(position: &InsertPosition) -> Result<(), AppApiError> {
    match position.anchor() {
        Some(id) => validate_stable_operation_id(id),
        None => Ok(()),
    }
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
        | Inline::GooglePersonChip { .. }
        | Inline::GoogleRichLinkChip { .. }
        | Inline::Dropdown { .. }
        | Inline::DateChip { .. }
        | Inline::Equation { .. }
        | Inline::Citation { .. }
        | Inline::FootnoteRef { .. }
        | Inline::PageNumber { .. } => false,
    }
}

fn validate_table_row_payload(row: &opendoc_core::TableRow) -> Result<(), AppApiError> {
    let block = Block {
        id: StableId::new("table-validator"),
        kind: BlockKind::table(vec![row.clone()]),
        content: Vec::new(),
        properties: BlockProperties::default(),
    };
    block
        .validate_isolated()
        .map_err(|err| AppApiError::Format(err.to_string()))
}

fn validate_table_cell_payload(cell: &opendoc_core::TableCell) -> Result<(), AppApiError> {
    let row = opendoc_core::TableRow {
        id: StableId::new("row-validator"),
        height: None,
        header: false,
        cells: vec![cell.clone()],
    };
    validate_table_row_payload(&row)
}

/// The single source of truth for the journal/envelope `kind` string of a
/// rich-document operation.
///
/// Envelope validation compares `AppOperationRecord::kind` against this, and
/// `DocumentOperationService` derives the record kind from the payload with
/// this same function, so the two can never drift apart.
pub(crate) fn rich_document_operation_kind(kind: &OperationKind) -> &'static str {
    match kind {
        OperationKind::SetDocumentTitle { .. } => "set-document-title",
        OperationKind::SetDocumentDoi { .. } => "set-document-doi",
        OperationKind::SetDocumentLocale { .. } => "set-document-locale",
        OperationKind::UpsertBookmark { .. } => "upsert-bookmark",
        OperationKind::SetPageSetup { .. } => "set-page-setup",
        OperationKind::SetPageFurniture { .. } => "set-page-furniture",
        OperationKind::ClearPageFurnitureOverride { .. } => "clear-page-furniture-override",
        OperationKind::InsertBlock { .. } => "insert-block",
        OperationKind::DeleteBlock { .. } => "delete-block",
        OperationKind::MoveBlock { .. } => "move-block",
        OperationKind::InsertSection { .. } => "insert-section",
        OperationKind::DeleteSection { .. } => "delete-section",
        OperationKind::SetSectionPageSetup { .. } => "set-section-page-setup",
        OperationKind::SetSectionFurniture { .. } => "set-section-furniture",
        OperationKind::ClearSectionFurnitureOverride { .. } => "clear-section-furniture-override",
        OperationKind::InsertInline { .. } => "insert-inline",
        OperationKind::MoveInlineToBlock { .. } => "move-inline-to-block",
        OperationKind::AddMark { .. } => "add-mark",
        OperationKind::RemoveMark { .. } => "remove-mark",
        OperationKind::AddMarkRange { .. } => "add-mark-range",
        OperationKind::AddSuggestion { .. } => "add-suggestion",
        OperationKind::UpdateSuggestionInsertContent { .. } => "update-suggestion",
        OperationKind::AddCommentThread { .. } => "add-comment-thread",
        OperationKind::AddCommentReply { .. } => "add-comment-reply",
        OperationKind::ResolveCommentThread { .. } => "resolve-comment-thread",
        OperationKind::ReopenCommentThread { .. } => "reopen-comment-thread",
        OperationKind::SetCommentThreadAction { .. } => "set-comment-thread-action",
        OperationKind::SetCommentThreadReaction { .. } => "set-comment-thread-reaction",
        OperationKind::UpsertFootnote { .. } => "upsert-footnote",
        OperationKind::SetEndnotePlacement { .. } => "set-endnote-placement",
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
        OperationKind::SelectDropdownOption { .. } => "select-dropdown-option",
        OperationKind::UpdateDateChip { .. } => "update-date-chip",
        OperationKind::UpdateLinkHref { .. } => "update-link-href",
        OperationKind::UpdateBlockEquationSource { .. } => "update-block-equation-source",
        OperationKind::UpdateImageAltText { .. } => "update-image-alt-text",
        OperationKind::UpdateImageLayout { .. } => "update-image-layout",
        OperationKind::UpdateImageBlobHash { .. } => "update-image-blob-hash",
        OperationKind::SetBlockTextStyle { .. } => "set-block-text-style",
        OperationKind::UpdateHeadingLevel { .. } => "update-heading-level",
        OperationKind::UpdateListItem { .. } => "update-list-item",
        OperationKind::SetListStart { .. } => "set-list-start",
        OperationKind::SetListFormat { .. } => "set-list-format",
        OperationKind::SetListBulletMarker { .. } => "set-list-bullet-marker",
        OperationKind::SetBlockProperty { .. } => "set-block-property",
        OperationKind::ClearBlockProperty { .. } => "clear-block-property",
        OperationKind::AcceptSuggestion { .. } => "accept-suggestion",
        OperationKind::RejectSuggestion { .. } => "reject-suggestion",
        OperationKind::DeleteInline { .. } => "delete-inline",
        OperationKind::InsertTableRow { .. } => "insert-table-row",
        OperationKind::DeleteTableRow { .. } => "delete-table-row",
        OperationKind::InsertTableCell { .. } => "insert-table-cell",
        OperationKind::DeleteTableCell { .. } => "delete-table-cell",
        OperationKind::InsertTableColumn { .. } => "insert-table-column",
        OperationKind::DeleteTableColumn { .. } => "delete-table-column",
        OperationKind::SetTableColumnWidth { .. } => "set-table-column-width",
        OperationKind::SetTableRowHeight { .. } => "set-table-row-height",
        OperationKind::SetTableRowHeader { .. } => "set-table-row-header",
        OperationKind::ReorderTableRows { .. } => "reorder-table-rows",
        OperationKind::SetTableBorder { .. } => "set-table-border",
        OperationKind::SetTableAlignment { .. } => "set-table-alignment",
        OperationKind::SetTableCellSpan { .. } => "set-table-cell-span",
        OperationKind::SetTableCellProperty { .. } => "set-table-cell-property",
        OperationKind::ClearTableCellProperty { .. } => "clear-table-cell-property",
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum AppBlobOperation {
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
pub enum AppSpreadsheetOperation {
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
    /// The whole workbook was replaced, wholesale.
    ///
    /// An import is the one command that does that, and before this it
    /// journalled a bare app envelope carrying no spreadsheet payload at all
    /// — which `merge_spreadsheet_envelope_streams` skips. Crash recovery
    /// replays from the recovery segment's base workbook and repository merge
    /// from the merge base, so both reproduced the workbook as it was
    /// *before* the import and silently resurrected the edits it replaced.
    ///
    /// The payload is the imported workbook itself rather than the file it
    /// came from: replay then reproduces exactly what the user saw, and does
    /// not depend on the importer answering the same way it did on the day.
    /// Boxed because it is an order of magnitude larger than every other
    /// variant.
    ReplaceWorkbook {
        workbook: Box<AppSpreadsheetWorkbook>,
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
    /// A last-writer-wins print-area assignment. The range is canonical A1.
    SetPrintArea {
        sheet_id: String,
        range: String,
    },
    /// Clear only the exact print area observed by the author.  Carrying the
    /// target prevents a delayed clear from erasing another actor's area.
    ClearPrintArea {
        sheet_id: String,
        range: String,
    },
    /// A last-writer-wins paper-orientation assignment.  The value is a typed
    /// enum so journal replay cannot invent a PDF geometry.
    SetPrintOrientation {
        sheet_id: String,
        orientation: AppSheetPrintOrientation,
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
    SetRowHeight {
        sheet_id: String,
        row: String,
        /// Height in pixels; `0` clears the explicit height.
        height: u32,
    },
    SetColumnWidth {
        sheet_id: String,
        column: String,
        /// Width in pixels; `0` clears the explicit width.
        width: u32,
    },
    /// Hides or reveals one row. Separate from [`SetRowHeight`] because
    /// hiding is not a size: a hidden row keeps the height it was given.
    ///
    /// [`SetRowHeight`]: AppSpreadsheetOperation::SetRowHeight
    SetRowHidden {
        sheet_id: String,
        row: String,
        hidden: bool,
    },
    SetColumnHidden {
        sheet_id: String,
        column: String,
        hidden: bool,
    },
    CopyRange {
        sheet_id: String,
        source_range: String,
        target_address: String,
    },
    SortRange {
        sheet_id: String,
        range: String,
        column: String,
        descending: bool,
        has_header: bool,
    },
    FillRange {
        sheet_id: String,
        source_range: String,
        target_range: String,
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
    /// The envelope kind this payload belongs in. Journalling derives the
    /// kind from here so the two can never drift apart.
    pub(crate) fn operation_kind(&self) -> &'static str {
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
            Self::ReplaceWorkbook { .. } => "replace-spreadsheet-workbook",
            Self::MergeCells { .. } => "merge-spreadsheet-cells",
            Self::UnmergeCells { .. } => "unmerge-spreadsheet-cells",
            Self::RestoreMerge { .. } => "restore-spreadsheet-merge",
            Self::SetBasicFilter { .. } => "set-spreadsheet-basic-filter",
            Self::SetPrintArea { .. } => "set-spreadsheet-print-area",
            Self::ClearPrintArea { .. } => "clear-spreadsheet-print-area",
            Self::SetPrintOrientation { .. } => "set-spreadsheet-print-orientation",
            Self::SetBasicFilterOptions { .. } => "set-spreadsheet-basic-filter-options",
            Self::ClearBasicFilter { .. } => "clear-spreadsheet-basic-filter",
            Self::RestoreBasicFilter { .. } => "restore-spreadsheet-basic-filter",
            Self::AddProtectedRange { .. } => "add-spreadsheet-protected-range",
            Self::UpdateProtectedRange { .. } => "update-spreadsheet-protected-range",
            Self::DeleteProtectedRange { .. } => "delete-spreadsheet-protected-range",
            Self::RestoreProtectedRange { .. } => "restore-spreadsheet-protected-range",
            Self::SetCell { .. } => "set-spreadsheet-cell",
            Self::SetCellFormat { .. } => "set-spreadsheet-cell-format",
            Self::SetRowHeight { .. } => "set-spreadsheet-row-height",
            Self::SetColumnWidth { .. } => "set-spreadsheet-column-width",
            Self::SetRowHidden { .. } => "set-spreadsheet-row-hidden",
            Self::SetColumnHidden { .. } => "set-spreadsheet-column-hidden",
            Self::CopyRange { .. } => "copy-spreadsheet-range",
            Self::SortRange { .. } => "sort-spreadsheet-range",
            Self::FillRange { .. } => "fill-spreadsheet-range",
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
            Self::ReplaceWorkbook { workbook } => {
                workbook.validate_source()?;
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
            Self::SetPrintArea { sheet_id, range } | Self::ClearPrintArea { sheet_id, range } => {
                validate_canonical_sheet_id(sheet_id)?;
                validate_canonical_cell_range("spreadsheet print area operation range", range)?;
            }
            Self::SetPrintOrientation { sheet_id, .. } => {
                validate_canonical_sheet_id(sheet_id)?;
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
            Self::SetRowHeight {
                sheet_id,
                row,
                height,
            } => {
                validate_canonical_sheet_id(sheet_id)?;
                validate_canonical_row_label(row)?;
                validate_axis_size_operation_px("spreadsheet row height operation", *height)?;
            }
            Self::SetColumnWidth {
                sheet_id,
                column,
                width,
            } => {
                validate_canonical_sheet_id(sheet_id)?;
                validate_canonical_column_label(column)?;
                validate_axis_size_operation_px("spreadsheet column width operation", *width)?;
            }
            Self::SetRowHidden { sheet_id, row, .. } => {
                validate_canonical_sheet_id(sheet_id)?;
                validate_canonical_row_label(row)?;
            }
            Self::SetColumnHidden {
                sheet_id, column, ..
            } => {
                validate_canonical_sheet_id(sheet_id)?;
                validate_canonical_column_label(column)?;
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
            Self::SortRange {
                sheet_id,
                range,
                column,
                ..
            } => {
                validate_canonical_sheet_id(sheet_id)?;
                validate_canonical_cell_range("spreadsheet sort operation range", range)?;
                validate_canonical_column_label(column)?;
            }
            Self::FillRange {
                sheet_id,
                source_range,
                target_range,
            } => {
                validate_canonical_sheet_id(sheet_id)?;
                validate_canonical_cell_range(
                    "spreadsheet fill operation source range",
                    source_range,
                )?;
                validate_canonical_cell_range(
                    "spreadsheet fill operation target range",
                    target_range,
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

/// A stored axis size in pixels; `0` means "clear the explicit size".
fn validate_axis_size_operation_px(label: &str, size: u32) -> Result<(), AppApiError> {
    if size > MAX_AXIS_SIZE_PX {
        return Err(AppApiError::Format(format!(
            "{label} size {size} exceeds {MAX_AXIS_SIZE_PX} pixels"
        )));
    }
    Ok(())
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
        "wrap_strategy" => {
            if !matches!(value.trim(), "wrap" | "") {
                return Err(AppApiError::Format(format!(
                    "unsupported text wrap strategy {}",
                    value.trim()
                )));
            }
        }
        "vertical_align" => {
            if !matches!(value.trim(), "top" | "middle" | "bottom" | "") {
                return Err(AppApiError::Format(format!(
                    "unsupported vertical alignment {}",
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

#[cfg(test)]
mod tests {
    use super::*;

    fn caret(app: &OpenDocApp, block: usize, offset: usize) -> EditorSelection {
        EditorSelection::collapsed(EditorPosition {
            block_id: app.document.blocks[block].id.to_string(),
            inline_id: None,
            offset,
        })
    }

    fn editor_input(selection: EditorSelection, input_type: &str) -> EditorInput {
        EditorInput {
            selection,
            input_type: input_type.to_string(),
            data: None,
            html: None,
        }
    }

    fn move_inline_envelope(app: &OpenDocApp) -> &AppOperationEnvelope {
        app.operation_envelopes
            .iter()
            .find(|envelope| {
                matches!(
                    envelope.operation.as_ref().map(|operation| &operation.kind),
                    Some(OperationKind::MoveInlineToBlock { .. })
                )
            })
            .expect("editor join emits a MoveInlineToBlock operation")
    }

    /// Replay the journalled operations onto `base` exactly as the journal
    /// records them, after a canonical CBOR encode/decode cycle.
    fn round_trip_and_replay(app: &OpenDocApp, base: &Document) -> Document {
        validate_operation_envelopes(&app.operation_envelopes)
            .expect("live operation envelopes validate");

        let decoded = app
            .operation_envelopes
            .iter()
            .map(|envelope| {
                let bytes = encode_canonical_cbor(envelope).expect("envelope encodes");
                decode_cbor::<AppOperationEnvelope>(&bytes).expect("envelope decodes")
            })
            .collect::<Vec<_>>();
        assert_eq!(decoded, app.operation_envelopes);
        validate_operation_envelopes(&decoded).expect("decoded operation envelopes validate");

        let mut stream = Vec::new();
        for envelope in &decoded {
            let Some(operation) = envelope.operation.clone() else {
                continue;
            };
            assert_eq!(
                envelope.record.kind,
                rich_document_operation_kind(&operation.kind),
                "journal kind drifted from its payload discriminant"
            );
            stream.push(operation);
        }
        merge_operations(base, &[stream])
            .expect("journal replays")
            .document
    }

    #[test]
    fn move_inline_to_block_envelope_kind_matches_its_payload_and_round_trips() {
        let mut app = OpenDocApp::new_sample();
        app.new_document("Envelope kinds");
        let base = app.document.clone();
        app.add_paragraph("Hello").expect("paragraph");
        app.add_paragraph("world").expect("paragraph");

        // Backspace at the start of the last paragraph joins it into the
        // previous one, which is the editor path that emits
        // `OperationKind::MoveInlineToBlock`.
        let last = app.document.blocks.len() - 1;
        let result = app
            .apply_editor_input(editor_input(caret(&app, last, 0), "deleteContentBackward"))
            .expect("backspace join is handled");
        assert!(result.handled);

        let envelope = move_inline_envelope(&app);
        assert_eq!(envelope.record.kind, "move-inline-to-block");
        assert_eq!(
            envelope.record.kind,
            rich_document_operation_kind(&envelope.operation.as_ref().unwrap().kind)
        );

        let replayed = round_trip_and_replay(&app, &base);
        assert_eq!(replayed, app.document);
    }

    #[test]
    fn split_paragraph_move_inline_envelope_kind_matches_its_payload_and_round_trips() {
        let mut app = OpenDocApp::new_sample();
        app.new_document("Envelope kinds");
        let base = app.document.clone();
        app.add_paragraph("Hello world").expect("paragraph");
        let last = app.document.blocks.len() - 1;

        // Bolding the first word splits the paragraph into two runs, so the
        // split below has a trailing inline to carry over.
        app.apply_editor_mark(EditorMarkInput {
            selection: EditorSelection {
                anchor: caret(&app, last, 0).anchor,
                focus: caret(&app, last, 5).focus,
            },
            mark_kind: "bold".to_string(),
            value: None,
            action: None,
        })
        .expect("bold is applied");

        // Enter inside the first run carries the whole trailing run into the
        // freshly inserted block, the second `MoveInlineToBlock` producer in
        // the editor.
        let result = app
            .apply_editor_input(editor_input(caret(&app, last, 2), "insertParagraph"))
            .expect("split is handled");
        assert!(result.handled);

        let envelope = move_inline_envelope(&app);
        assert_eq!(envelope.record.kind, "move-inline-to-block");

        let replayed = round_trip_and_replay(&app, &base);
        assert_eq!(replayed, app.document);
    }

    #[test]
    fn journalled_document_operations_carry_their_payload_discriminant() {
        let mut app = OpenDocApp::new_sample();
        app.new_document("Envelope kinds");
        let base = app.document.clone();
        app.add_paragraph("alpha").expect("paragraph");
        app.add_heading("beta", 2).expect("heading is added");
        app.add_table().expect("a table");
        app.set_document_title("Renamed").expect("title is set");
        let last = app.document.blocks.len() - 1;
        let _ = app.apply_editor_input(editor_input(caret(&app, last, 0), "insertParagraph"));

        assert!(app
            .operation_envelopes
            .iter()
            .any(|envelope| envelope.operation.is_some()));
        let replayed = round_trip_and_replay(&app, &base);
        assert_eq!(replayed, app.document);
    }

    /// The four structural inserts anchor with an `InsertPosition` rather than
    /// an `after: Option<StableId>`, so their source validation goes through
    /// `validate_insert_position`. An `After` still has to name a well-formed
    /// id; `First` and `Last` name none and so have nothing to check.
    ///
    /// Decoding, not authoring, is what this guards: no command can build a
    /// malformed anchor, but an operation segment read off disk or off a socket
    /// can carry one.
    #[test]
    fn a_malformed_insert_anchor_is_refused_on_every_structural_insert() {
        // A `StableId` cannot be *constructed* malformed outside its own
        // crate; it can only be decoded that way, which is exactly the route
        // this validation exists to cover.
        let bad = || {
            InsertPosition::After(
                serde_json::from_str::<StableId>("\" bad \"").expect("decodes as written"),
            )
        };
        let block = || Block::paragraph("body");
        let inline = || Inline::text("run");
        let table_cell = || opendoc_core::TableCell::new(vec![Block::paragraph("cell")]);
        let ok_id = || StableId::parse("blk-one").unwrap();

        let refused = [
            OperationKind::InsertBlock {
                position: bad(),
                block: block(),
            },
            OperationKind::InsertInline {
                block_id: ok_id(),
                position: bad(),
                inline: inline(),
            },
            OperationKind::MoveInlineToBlock {
                inline_id: ok_id(),
                target_block_id: ok_id(),
                position: bad(),
            },
            OperationKind::InsertTableCell {
                table_block_id: ok_id(),
                row_id: ok_id(),
                position: bad(),
                cell: table_cell(),
            },
        ];
        for kind in refused {
            let error = validate_rich_document_operation_source(&kind)
                .expect_err("a malformed insert anchor has to be refused");
            assert!(
                matches!(error, AppApiError::Format(_)),
                "{kind:?} was refused with {error:?}"
            );
        }

        // And the two positions that name no anchor are accepted, so the
        // check above is about the anchor rather than about the operation.
        for position in [InsertPosition::First, InsertPosition::Last] {
            validate_rich_document_operation_source(&OperationKind::InsertBlock {
                position,
                block: block(),
            })
            .expect("First and Last name no anchor, so there is nothing to reject");
        }
    }
}
