use crate::{now_ms, AppOperationEnvelope, AppOperationRecord};
use opendoc_core::Document;
use opendoc_merge::{merge_operations, ActorId, Operation, OperationId, OperationKind};

pub(crate) struct DocumentOperationService<'a> {
    actor_id: &'a str,
    document: &'a mut Document,
    operation_journal: &'a mut Vec<AppOperationRecord>,
    operation_envelopes: &'a mut Vec<AppOperationEnvelope>,
    next_seq: &'a mut u64,
}

impl<'a> DocumentOperationService<'a> {
    pub(crate) fn new(
        actor_id: &'a str,
        document: &'a mut Document,
        operation_journal: &'a mut Vec<AppOperationRecord>,
        operation_envelopes: &'a mut Vec<AppOperationEnvelope>,
        next_seq: &'a mut u64,
    ) -> Self {
        Self {
            actor_id,
            document,
            operation_journal,
            operation_envelopes,
            next_seq,
        }
    }

    pub(crate) fn apply(&mut self, operation_kind: &str, summary: &str, kind: OperationKind) {
        let seq = *self.next_seq;
        let op = Operation {
            id: self.operation_id(seq),
            kind: kind.clone(),
        };
        *self.next_seq += 1;
        if let Ok(result) = merge_operations(self.document, &[vec![op]]) {
            *self.document = result.document;
            self.push_document_operation(operation_kind, summary, seq, kind);
        }
    }

    pub(crate) fn apply_batch(&mut self, operations: Vec<(&str, &str, OperationKind)>) {
        let mut merge_ops = Vec::new();
        let mut records = Vec::new();
        for (offset, (operation_kind, summary, kind)) in operations.into_iter().enumerate() {
            let seq = *self.next_seq + offset as u64;
            merge_ops.push(Operation {
                id: self.operation_id(seq),
                kind: kind.clone(),
            });
            records.push((operation_kind.to_string(), summary.to_string(), seq, kind));
        }
        if let Ok(result) = merge_operations(self.document, &[merge_ops]) {
            *self.document = result.document;
            *self.next_seq += records.len() as u64;
            for (operation_kind, summary, seq, kind) in records {
                self.push_document_operation(&operation_kind, &summary, seq, kind);
            }
        }
    }

    fn push_document_operation(
        &mut self,
        operation_kind: &str,
        summary: &str,
        seq: u64,
        kind: OperationKind,
    ) {
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
            operation: Some(Operation {
                id: self.operation_id(seq),
                kind,
            }),
            spreadsheet: None,
            blob: None,
        });
    }

    fn operation_id(&self, seq: u64) -> OperationId {
        OperationId {
            actor: ActorId(self.actor_id.to_string()),
            seq,
        }
    }
}
