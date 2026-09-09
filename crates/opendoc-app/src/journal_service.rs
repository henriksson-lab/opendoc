use crate::{
    now_ms, AppBlobOperation, AppOperationEnvelope, AppOperationRecord, AppSpreadsheetOperation,
    OpenDocApp,
};

pub(crate) struct OperationJournalService<'a> {
    actor_id: &'a str,
    operation_journal: &'a mut Vec<AppOperationRecord>,
    operation_envelopes: &'a mut Vec<AppOperationEnvelope>,
    next_seq: &'a mut u64,
}

impl<'a> OperationJournalService<'a> {
    pub(crate) fn new(
        actor_id: &'a str,
        operation_journal: &'a mut Vec<AppOperationRecord>,
        operation_envelopes: &'a mut Vec<AppOperationEnvelope>,
        next_seq: &'a mut u64,
    ) -> Self {
        Self {
            actor_id,
            operation_journal,
            operation_envelopes,
            next_seq,
        }
    }

    pub(crate) fn push_app_operation(&mut self, operation_kind: &str, summary: &str) {
        self.push_operation_envelope(operation_kind, summary, None, None);
    }

    pub(crate) fn push_blob_operation(
        &mut self,
        operation_kind: &str,
        summary: &str,
        blob: AppBlobOperation,
    ) {
        self.push_operation_envelope(operation_kind, summary, None, Some(blob));
    }

    pub(crate) fn push_spreadsheet_operation(
        &mut self,
        operation_kind: &str,
        summary: &str,
        spreadsheet: AppSpreadsheetOperation,
    ) {
        self.push_operation_envelope(operation_kind, summary, Some(spreadsheet), None);
    }

    fn push_operation_envelope(
        &mut self,
        operation_kind: &str,
        summary: &str,
        spreadsheet: Option<AppSpreadsheetOperation>,
        blob: Option<AppBlobOperation>,
    ) {
        let seq = *self.next_seq;
        *self.next_seq += 1;
        let record = AppOperationRecord {
            actor: self.actor_id.to_string(),
            seq,
            kind: operation_kind.to_string(),
            summary: summary.to_string(),
            created_at_ms: now_ms(),
        };
        self.operation_journal.push(record.clone());
        self.operation_envelopes.push(AppOperationEnvelope {
            record,
            operation: None,
            spreadsheet,
            blob,
        });
    }
}

impl OpenDocApp {
    pub(crate) fn push_app_operation(&mut self, operation_kind: &str, summary: &str) {
        self.operation_journal_service()
            .push_app_operation(operation_kind, summary);
    }

    pub(crate) fn push_blob_operation(
        &mut self,
        operation_kind: &str,
        summary: &str,
        blob: AppBlobOperation,
    ) {
        self.operation_journal_service()
            .push_blob_operation(operation_kind, summary, blob);
    }

    pub(crate) fn push_spreadsheet_operation(
        &mut self,
        operation_kind: &str,
        summary: &str,
        spreadsheet: AppSpreadsheetOperation,
    ) {
        self.operation_journal_service().push_spreadsheet_operation(
            operation_kind,
            summary,
            spreadsheet,
        );
    }

    fn operation_journal_service(&mut self) -> OperationJournalService<'_> {
        OperationJournalService::new(
            &self.actor_id,
            &mut self.operation_journal,
            &mut self.operation_envelopes,
            &mut self.next_seq,
        )
    }
}
