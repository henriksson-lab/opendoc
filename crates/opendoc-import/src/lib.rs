use opendoc_core::{Block, Document, ModelWarning};
use std::fmt;
use std::path::Path;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportReport {
    pub document: Document,
    pub warnings: Vec<ModelWarning>,
}

pub fn import_plaintext_projection(
    title: impl Into<String>,
    text: &str,
) -> Result<ImportReport, ImportError> {
    if text.trim().is_empty() {
        return Err(ImportError::EmptyInput);
    }
    let mut document = Document::new(title);
    for line in text.lines() {
        document.blocks.push(Block::paragraph(line));
    }
    document
        .validate()
        .map_err(|err| ImportError::InvalidDocument(err.to_string()))?;
    Ok(ImportReport {
        document,
        warnings: Vec::new(),
    })
}

pub fn import_doc_or_docx(path: &Path) -> Result<ImportReport, ImportError> {
    let ext = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if ext != "doc" && ext != "docx" {
        return Err(ImportError::UnsupportedExtension(ext.to_string()));
    }
    Err(ImportError::ConverterUnavailable)
}

#[derive(Debug, Eq, PartialEq)]
pub enum ImportError {
    EmptyInput,
    UnsupportedExtension(String),
    ConverterUnavailable,
    InvalidDocument(String),
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for ImportError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plaintext_projection_imports_lines_as_paragraphs() {
        let report = import_plaintext_projection("Doc", "a\nb").unwrap();
        assert_eq!(report.document.blocks.len(), 2);
        assert_eq!(report.document.visible_text(), "a\nb\n");
    }

    #[test]
    fn unsupported_import_aborts() {
        assert_eq!(
            import_doc_or_docx(Path::new("x.pdf")).unwrap_err(),
            ImportError::UnsupportedExtension("pdf".to_string())
        );
    }
}
