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

    pub(crate) fn mutate<T>(
        &mut self,
        policy: SpreadsheetEvaluationPolicy,
        apply: impl FnOnce(&mut AppSpreadsheetWorkbook) -> Result<T, AppApiError>,
    ) -> Result<T, AppApiError> {
        self.signatures.clear();
        *self.saved_projection = None;
        let result = apply(self.workbook)?;
        if policy == SpreadsheetEvaluationPolicy::Force || !self.defer_evaluation {
            *self.workbook = self.workbook.evaluated();
        }
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
