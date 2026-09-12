use opendoc_spreadsheet::SpreadsheetError;

#[derive(Debug)]
pub enum AppApiError {
    Conflict(String),
    /// A command that would replace the open document was refused because the
    /// document holds changes the repository does not have. The caller decides
    /// what to do and, if the user accepts the loss, repeats the dispatch with
    /// `opendoc_api::DISCARD_UNSAVED_CHANGES_ARG` set to `true`.
    ///
    /// This is a distinct variant rather than a `Conflict` so that a transport
    /// can recognise it without matching on message text.
    UnsavedChanges(String),
    Format(String),
    Import(String),
    Model(String),
    NotFound(String),
    Sign(String),
    Store(String),
}

impl std::fmt::Display for AppApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for AppApiError {}

impl From<opendoc_api::CommandParseError> for AppApiError {
    fn from(error: opendoc_api::CommandParseError) -> Self {
        match error {
            opendoc_api::CommandParseError::Format(message) => AppApiError::Format(message),
        }
    }
}

impl From<SpreadsheetError> for AppApiError {
    fn from(error: SpreadsheetError) -> Self {
        match error {
            SpreadsheetError::Conflict(message) => AppApiError::Conflict(message),
            SpreadsheetError::Format(message) => AppApiError::Format(message),
            SpreadsheetError::Import(message) => AppApiError::Import(message),
            SpreadsheetError::Model(message) => AppApiError::Model(message),
            SpreadsheetError::NotFound(message) => AppApiError::NotFound(message),
        }
    }
}
