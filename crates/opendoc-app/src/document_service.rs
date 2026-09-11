use crate::{now_ms, rich_document_operation_kind, AppOperationEnvelope, AppOperationRecord};
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

    /// `_operation_kind` is accepted for call-site symmetry but deliberately
    /// ignored: the journal/envelope kind is derived from the payload by
    /// [`rich_document_operation_kind`], so a record can never disagree with
    /// the typed operation it carries. The literals still passed by call
    /// sites are redundant and can be deleted.
    pub(crate) fn apply(&mut self, _operation_kind: &str, summary: &str, kind: OperationKind) {
        let seq = *self.next_seq;
        let op = Operation {
            id: self.operation_id(seq),
            kind: kind.clone(),
        };
        *self.next_seq += 1;
        if let Ok(result) = merge_operations(self.document, &[vec![op]]) {
            *self.document = result.document;
            self.push_document_operation(summary, seq, kind);
        }
    }

    /// The `&str` kind in each tuple is ignored; see [`Self::apply`].
    pub(crate) fn apply_batch(&mut self, operations: Vec<(&str, &str, OperationKind)>) {
        let mut merge_ops = Vec::new();
        let mut records = Vec::new();
        for (offset, (_operation_kind, summary, kind)) in operations.into_iter().enumerate() {
            let seq = *self.next_seq + offset as u64;
            merge_ops.push(Operation {
                id: self.operation_id(seq),
                kind: kind.clone(),
            });
            records.push((summary.to_string(), seq, kind));
        }
        if let Ok(result) = merge_operations(self.document, &[merge_ops]) {
            *self.document = result.document;
            *self.next_seq += records.len() as u64;
            for (summary, seq, kind) in records {
                self.push_document_operation(&summary, seq, kind);
            }
        }
    }

    fn push_document_operation(&mut self, summary: &str, seq: u64, kind: OperationKind) {
        let record = AppOperationRecord {
            actor: self.actor_id.to_string(),
            seq,
            kind: rich_document_operation_kind(&kind).to_string(),
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
