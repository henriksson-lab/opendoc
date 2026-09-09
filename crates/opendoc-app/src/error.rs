use opendoc_spreadsheet::SpreadsheetError;

#[derive(Debug)]
pub enum AppApiError {
    Conflict(String),
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
