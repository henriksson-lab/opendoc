use crate::{now_ms, rich_document_operation_kind, AppOperationEnvelope, AppOperationRecord};
use opendoc_core::Document;
use opendoc_merge::{
    merge_operations, ActorId, CausalContext, Operation, OperationId, OperationKind,
};

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
        let op = Operation::in_context(self.operation_id(seq), kind, self.causal_context(0));
        *self.next_seq += 1;
        if let Ok(result) = merge_operations(self.document, &[vec![op.clone()]]) {
            *self.document = result.document;
            self.push_document_operation(summary, op);
        }
    }

    /// The `&str` kind in each tuple is ignored; see [`Self::apply`].
    pub(crate) fn apply_batch(&mut self, operations: Vec<(&str, &str, OperationKind)>) {
        let mut merge_ops = Vec::new();
        let mut records = Vec::new();
        for (offset, (_operation_kind, summary, kind)) in operations.into_iter().enumerate() {
            let seq = *self.next_seq + offset as u64;
            let op = Operation::in_context(
                self.operation_id(seq),
                kind,
                self.causal_context(offset as u64),
            );
            merge_ops.push(op.clone());
            records.push((summary.to_string(), op));
        }
        if let Ok(result) = merge_operations(self.document, &[merge_ops]) {
            *self.document = result.document;
            *self.next_seq += records.len() as u64;
            for (summary, op) in records {
                self.push_document_operation(&summary, op);
            }
        }
    }

    /// The causal context a locally generated operation is written in: every
    /// operation this replica has already applied, local or merged in from
    /// another actor. Without it the merge would read this replica's edits as
    /// concurrent with work it demonstrably already had, and text offsets
    /// would be re-anchored against a document it never saw. See
    /// `docs/adr/0007-causal-ordering-and-text-convergence.md`.
    ///
    /// `lamport_offset` advances the timestamp for later operations in a
    /// batch, which all observe the same set but must still order among
    /// themselves.
    fn causal_context(&self, lamport_offset: u64) -> CausalContext {
        let mut context = CausalContext::observing(
            self.operation_envelopes
                .iter()
                .filter_map(|envelope| envelope.operation.as_ref()),
        );
        context.lamport += lamport_offset;
        context
    }

    fn push_document_operation(&mut self, summary: &str, op: Operation) {
        let record = AppOperationRecord {
            actor: self.actor_id.to_string(),
            seq: op.id.seq,
            kind: rich_document_operation_kind(&op.kind).to_string(),
            summary: summary.to_string(),
            created_at_ms: now_ms(),
        };
        self.operation_journal.push(record.clone());
        self.operation_envelopes.push(AppOperationEnvelope {
            record,
            operation: Some(op),
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
