//! The result of a one-shot export.
//!
//! Every export target can carry less than the model holds: Google Docs JSON
//! has no checklist, WordprocessingML has no comment suggestion, and neither
//! has a DOI. The exporters have always known what they dropped —
//! `export_google_docs_json_with_warnings` and `export_docx_with_warnings`
//! both return a warning list — and until now the commands threw it away, so
//! a user exporting a checklist was told nothing.
//!
//! The warnings ride the **command result**, not the document. Writing them
//! into `document.warnings` (what `push_model_warning` does) would put the
//! outcome of a read-only projection into signed source state: exporting
//! would dirty the document and change the bytes its signature covers. See
//! `docs/adr/0010-export-results-carry-their-own-warnings.md`.

use serde::{Deserialize, Serialize};

use crate::AppWarning;

/// How [`AppExport::content`] encodes the exported bytes.
///
/// An export is either text (JSON, CSV, Markdown) or a binary package (a
/// `.docx` zip), and the transport is JSON in every runtime, so binary has to
/// be base64. Stating which one it is here means no caller has to keep a
/// table of "this command returns base64" — a table that was previously in
/// `main.ts` and could disagree with Rust.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AppExportEncoding {
    Text,
    Base64,
}

impl AppExportEncoding {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Base64 => "base64",
        }
    }
}

/// The bytes one export produced, how to name the file, and what the target
/// format could not carry.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppExport {
    pub content: String,
    pub encoding: AppExportEncoding,
    pub media_type: String,
    /// File extension without the dot, for the save dialog's filter and
    /// default name. A property of the format, so it is stated once, here.
    pub file_extension: String,
    /// What the target format could not represent. Empty is the normal case.
    pub warnings: Vec<AppWarning>,
}

impl AppExport {
    pub(crate) fn text(
        content: impl Into<String>,
        media_type: &str,
        file_extension: &str,
        warnings: Vec<AppWarning>,
    ) -> Self {
        Self {
            content: content.into(),
            encoding: AppExportEncoding::Text,
            media_type: media_type.to_string(),
            file_extension: file_extension.to_string(),
            warnings,
        }
    }

    pub(crate) fn binary(
        bytes: &[u8],
        media_type: &str,
        file_extension: &str,
        warnings: Vec<AppWarning>,
    ) -> Self {
        Self {
            content: crate::base64_encode(bytes),
            encoding: AppExportEncoding::Base64,
            media_type: media_type.to_string(),
            file_extension: file_extension.to_string(),
            warnings,
        }
    }
}
