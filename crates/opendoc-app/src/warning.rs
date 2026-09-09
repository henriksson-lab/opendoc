use crate::AppApiError;
use opendoc_core::ModelWarning;
use opendoc_spreadsheet::SpreadsheetWarning;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppWarning {
    pub code: String,
    pub message: String,
}

impl AppWarning {
    pub(crate) fn from_core(warning: &ModelWarning) -> Self {
        Self {
            code: warning.code.clone(),
            message: warning.message.clone(),
        }
    }

    pub(crate) fn to_core(&self) -> ModelWarning {
        ModelWarning {
            code: self.code.clone(),
            message: self.message.clone(),
        }
    }

    pub(crate) fn validate_source(&self) -> Result<(), AppApiError> {
        self.to_core()
            .validate()
            .map_err(|err| AppApiError::Model(err.to_string()))
    }
}

impl From<SpreadsheetWarning> for AppWarning {
    fn from(warning: SpreadsheetWarning) -> Self {
        Self {
            code: warning.code,
            message: warning.message,
        }
    }
}

pub(crate) fn push_unique_warning(warnings: &mut Vec<AppWarning>, code: &str, message: String) {
    if !warnings
        .iter()
        .any(|warning| warning.code == code && warning.message == message)
    {
        warnings.push(AppWarning {
            code: code.to_string(),
            message,
        });
    }
}

pub(crate) fn push_spreadsheet_warning(
    warnings: &mut Vec<AppWarning>,
    code: &str,
    message: String,
) {
    warnings.push(AppWarning {
        code: code.to_string(),
        message,
    });
}
