use crate::{
    AppApiError, AppDocument, AppSpreadsheetOperation, AppSpreadsheetWorkbook, OpenDocApp,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SpreadsheetEvaluationPolicy {
    RespectDeferred,
    Force,
}

pub(crate) struct SpreadsheetMutationService<'a> {
    workbook: &'a mut AppSpreadsheetWorkbook,
    signatures: &'a mut Vec<opendoc_format::SignatureRecord>,
    saved_projection: &'a mut Option<AppDocument>,
    defer_evaluation: bool,
}

impl<'a> SpreadsheetMutationService<'a> {
    pub(crate) fn new(
        workbook: &'a mut AppSpreadsheetWorkbook,
        signatures: &'a mut Vec<opendoc_format::SignatureRecord>,
        saved_projection: &'a mut Option<AppDocument>,
        defer_evaluation: bool,
    ) -> Self {
        Self {
            workbook,
            signatures,
            saved_projection,
            defer_evaluation,
        }
    }

    /// Applies one mutation to the workbook, all of it or none of it.
    ///
    /// `apply` never touches the live workbook: it is handed a staged copy,
    /// which replaces the workbook only once it has returned `Ok`. A command
    /// that walks a range and fails halfway — a selection running past the
    /// last row, a format that no cell in the range accepts — therefore leaves
    /// the workbook exactly as it found it, instead of leaving the cells it
    /// did reach changed and reporting an error about the ones it did not.
    /// The invalidations happen on the same terms: a mutation that failed has
    /// not invalidated the signatures or the saved projection, because it has
    /// not changed anything they describe.
    ///
    /// **Cost: one clone of the workbook per mutation, and that clone is not
    /// new.** Evaluation already produced one — `evaluated()` is
    /// `clone` + `evaluate` — so the staged copy *is* the copy that was going
    /// to be made anyway, evaluated in place and moved into the workbook. On
    /// the evaluating path (every command but the deferred ones) the
    /// transaction is free. Only a mutation that skips evaluation pays a clone
    /// it did not pay before, which is the price of the guarantee; cloning
    /// lazily instead would mean knowing in advance which mutations can fail,
    /// and "this one cannot fail" is precisely the assumption that put the
    /// half-applied ranges here.
    pub(crate) fn mutate<T>(
        &mut self,
        policy: SpreadsheetEvaluationPolicy,
        apply: impl FnOnce(&mut AppSpreadsheetWorkbook) -> Result<T, AppApiError>,
    ) -> Result<T, AppApiError> {
        let mut staged = self.workbook.clone();
        let result = apply(&mut staged)?;
        if policy == SpreadsheetEvaluationPolicy::Force || !self.defer_evaluation {
            staged.evaluate();
        }
        *self.workbook = staged;
        self.signatures.clear();
        *self.saved_projection = None;
        Ok(result)
    }
}

impl OpenDocApp {
    /// Journals a spreadsheet mutation, deriving the operation envelope kind
    /// from the payload.
    ///
    /// Hand-written kind strings at the call site are what produced the
    /// `move-inline` drift bug in the rich-document path; every spreadsheet
    /// command goes through here so the kind and the payload are one fact.
    pub(crate) fn journal_spreadsheet_operation(
        &mut self,
        summary: &str,
        operation: AppSpreadsheetOperation,
    ) {
        let kind = operation.operation_kind();
        self.push_spreadsheet_operation(kind, summary, operation);
    }

    pub(crate) fn mutate_spreadsheet<T>(
        &mut self,
        policy: SpreadsheetEvaluationPolicy,
        apply: impl FnOnce(&mut AppSpreadsheetWorkbook) -> Result<T, AppApiError>,
    ) -> Result<T, AppApiError> {
        self.spreadsheet_mutation_service().mutate(policy, apply)
    }

    fn spreadsheet_mutation_service(&mut self) -> SpreadsheetMutationService<'_> {
        SpreadsheetMutationService::new(
            &mut self.workbook,
            &mut self.signatures,
            &mut self.saved_projection,
            self.defer_spreadsheet_evaluation,
        )
    }
}
