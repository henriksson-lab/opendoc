//! Import and export adapters for outside document formats.
//!
//! Google Docs JSON, `.docx` (read and write, natively) and legacy `.doc`
//! (through an external converter, when one exists) all land in the same
//! canonical `opendoc-core` model. Every adapter reports what it could not
//! represent as a `ModelWarning` rather than silently dropping it, and aborts
//! rather than writing a payload it cannot express faithfully.

mod docx;
mod docx_write;
mod error;
mod google_citations;
mod google_color;
mod google_export;
mod google_import;
mod google_style;
mod json;
mod legacy_doc;
mod opendoc_json;
mod xml;

pub use docx_write::DocxImage;
pub use error::ImportError;

pub(crate) use json::{optional_bool, optional_object, optional_str};

#[cfg(test)]
mod test_support;

#[cfg(test)]
mod docx_import_tests;
#[cfg(test)]
mod docx_tests;
#[cfg(test)]
mod docx_write_tests;
#[cfg(test)]
mod google_citation_tests;
#[cfg(test)]
mod google_export_tests;
#[cfg(test)]
mod google_extension_tests;
#[cfg(test)]
mod google_footnote_tests;
#[cfg(test)]
mod google_paragraph_tests;
#[cfg(test)]
mod google_review_tests;
#[cfg(test)]
mod google_style_tests;
#[cfg(test)]
mod google_table_tests;
#[cfg(test)]
mod plaintext_tests;

use crate::google_citations::{
    export_google_citations, import_google_citations, refresh_imported_citation_projection_caches,
    repair_imported_citation_placements, repair_imported_citation_references,
    repair_imported_inline_citation_labels, should_export_citations,
};
use crate::google_export::{
    export_google_block, export_google_comments, export_google_footnotes, export_google_suggestions,
};
use crate::google_import::{
    import_google_comments, import_google_footnotes, import_google_structural_element,
    import_google_suggestions,
};
use crate::google_style::{export_lists, warning, GoogleLists};
use crate::legacy_doc::{convert_legacy_doc_to_plaintext, validate_legacy_doc_container};
use opendoc_core::{Block, BlockKind, Document, Mark, MarkExpand, MarkKind, ModelWarning};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

/// The IANA media type of a `.docx` package.
pub const DOCX_MEDIA_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportedBlob {
    pub name: String,
    pub media_type: String,
    pub hash: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportReport {
    pub document: Document,
    pub warnings: Vec<ModelWarning>,
    pub blobs: Vec<ImportedBlob>,
}

pub fn import_plaintext_projection(
    title: impl Into<String>,
    text: &str,
) -> Result<ImportReport, ImportError> {
    if text.trim().is_empty() {
        return Err(ImportError::EmptyInput);
    }
    let mut document = Document::new(import_document_title(title.into())?);
    for line in text.lines() {
        document.blocks.push(Block::paragraph(line));
    }
    document
        .validate()
        .map_err(|err| ImportError::InvalidDocument(err.to_string()))?;
    Ok(ImportReport {
        document,
        warnings: Vec::new(),
        blobs: Vec::new(),
    })
}

pub fn import_doc_or_docx(path: &Path) -> Result<ImportReport, ImportError> {
    let ext = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if ext != "doc" && ext != "docx" {
        return Err(ImportError::UnsupportedExtension(ext));
    }
    if !path.is_file() {
        return Err(ImportError::InvalidInput(format!(
            "input file {} was not found",
            path.display()
        )));
    }
    let title = path
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("Imported Word Document");
    if ext == "docx" {
        let bytes = fs::read(path).map_err(|err| ImportError::InvalidInput(err.to_string()))?;
        let imported = docx::import_docx_bytes(title, &bytes)?;
        return Ok(ImportReport {
            document: imported.document,
            warnings: imported.warnings,
            blobs: imported.blobs,
        });
    }
    validate_legacy_doc_container(path)?;
    let text = convert_legacy_doc_to_plaintext(path)?;
    import_plaintext_projection(title, &text)
}

/// Import a `.docx` package that is already in memory (browser uploads).
pub fn import_docx_bytes(title: &str, bytes: &[u8]) -> Result<ImportReport, ImportError> {
    let title = if title.trim().is_empty() {
        "Imported Word Document"
    } else {
        title
    };
    let imported = docx::import_docx_bytes(title, bytes)?;
    Ok(ImportReport {
        document: imported.document,
        warnings: imported.warnings,
        blobs: imported.blobs,
    })
}

/// Exports the document as a `.docx` package.
///
/// `images` supplies the bytes behind every [`opendoc_core::BlockKind::Image`]
/// block, keyed by content hash — the model stores only the hash, and the
/// package has to embed the bytes. An image whose blob is missing is written
/// as its alt text and named in the warnings.
///
/// [`export_docx_with_warnings`] is the same call with everything the DOCX
/// format cannot carry reported alongside the bytes.
pub fn export_docx(
    document: &Document,
    images: &BTreeMap<String, DocxImage>,
) -> Result<Vec<u8>, ImportError> {
    export_docx_with_warnings(document, images).map(|(bytes, _)| bytes)
}

/// Exports to DOCX, returning the package bytes and everything
/// WordprocessingML could not represent exactly.
pub fn export_docx_with_warnings(
    document: &Document,
    images: &BTreeMap<String, DocxImage>,
) -> Result<(Vec<u8>, Vec<ModelWarning>), ImportError> {
    let export = docx_write::export_docx_bytes(document, images)?;
    Ok((export.bytes, export.warnings))
}

pub fn import_google_docs_json(
    title: impl Into<String>,
    bytes: &[u8],
) -> Result<ImportReport, ImportError> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|err| ImportError::InvalidInput(err.to_string()))?;
    let mut document = Document::new(import_document_title(title.into())?);
    let mut warnings = Vec::new();
    let content = value
        .pointer("/body/content")
        .and_then(Value::as_array)
        .ok_or_else(|| ImportError::InvalidInput("missing body.content array".to_string()))?;
    let lists = GoogleLists::parse(&value, &mut warnings)?;
    report_dropped_google_document_parts(&value, &mut warnings);
    for element in content {
        document.blocks.extend(import_google_structural_element(
            element,
            &lists,
            &mut warnings,
            true,
        )?);
    }
    document.footnotes = import_google_footnotes(&value, &mut warnings)?;
    document.citation_database = import_google_citations(&value, &mut warnings)?;
    document.comments = import_google_comments(&value, &mut warnings)?;
    document.suggestions = import_google_suggestions(&value, &mut warnings)?;
    repair_imported_citation_placements(&mut document, &mut warnings);
    repair_imported_citation_references(&mut document, &mut warnings);
    repair_imported_inline_citation_labels(&mut document, &mut warnings);
    refresh_imported_citation_projection_caches(&mut document);
    dedupe_warnings(&mut warnings);
    document.warnings.extend(warnings.clone());
    document
        .validate()
        .map_err(|err| ImportError::InvalidDocument(err.to_string()))?;
    Ok(ImportReport {
        document,
        warnings,
        blobs: Vec::new(),
    })
}

pub fn export_google_docs_json(document: &Document) -> Result<Vec<u8>, ImportError> {
    export_google_docs_json_with_warnings(document).map(|(bytes, _)| bytes)
}

/// Exports to Google Docs-shaped JSON, returning everything Google's schema
/// cannot carry alongside the bytes. [`export_google_docs_json`] is the same
/// call with the warnings discarded.
pub fn export_google_docs_json_with_warnings(
    document: &Document,
) -> Result<(Vec<u8>, Vec<ModelWarning>), ImportError> {
    validate_google_docs_exportable_structure(&document.blocks)?;
    document
        .validate()
        .map_err(|err| ImportError::InvalidDocument(err.to_string()))?;
    let mut warnings = Vec::new();
    let content = document
        .blocks
        .iter()
        .map(|block| export_google_block(block, true, &mut warnings))
        .collect::<Result<Vec<_>, _>>()?;
    let mut value = json!({
        "title": document.title,
        "body": { "content": content },
    });
    if let Some(lists) = export_lists(&document.blocks, &mut warnings) {
        value["lists"] = lists;
    }
    let footnotes = export_google_footnotes(&document.footnotes)?;
    if !footnotes.is_empty() {
        value["footnotes"] = Value::Object(footnotes);
    }
    if should_export_citations(&document.citation_database) {
        value["opendocCitations"] = export_google_citations(&document.citation_database)?;
    }
    if !document.comments.is_empty() {
        value["opendocComments"] = export_google_comments(&document.comments)?;
    }
    if !document.suggestions.is_empty() {
        value["opendocSuggestions"] = export_google_suggestions(&document.suggestions)?;
    }
    dedupe_warnings(&mut warnings);
    let bytes = serde_json::to_vec_pretty(&value)
        .map_err(|err| ImportError::InvalidDocument(err.to_string()))?;
    Ok((bytes, warnings))
}

/// Warnings are raised per occurrence, but a reader needs to know *what* was
/// dropped, not how many times; identical ones collapse to the first.
fn dedupe_warnings(warnings: &mut Vec<ModelWarning>) {
    let mut seen = BTreeSet::new();
    warnings.retain(|warning| seen.insert((warning.code.clone(), warning.message.clone())));
}

/// Top-level Google document parts that OpenDoc has no model for (FM-11).
/// The page-geometry model does not exist yet, so these cannot be imported —
/// but they must be named, not vanish.
fn report_dropped_google_document_parts(value: &Value, warnings: &mut Vec<ModelWarning>) {
    const PARTS: [(&str, &str); 6] = [
        (
            "documentStyle",
            "Google Docs page setup (documentStyle: page size, margins, orientation, background) is not representable and was dropped",
        ),
        (
            "headers",
            "Google Docs page headers are not representable and were dropped",
        ),
        (
            "footers",
            "Google Docs page footers are not representable and were dropped",
        ),
        (
            "namedStyles",
            "Google Docs named style definitions (namedStyles) are not representable and were dropped; each paragraph keeps only its own explicit formatting",
        ),
        (
            "inlineObjects",
            "Google Docs inline objects (images and drawings) carry no content in the JSON and were dropped",
        ),
        (
            "positionedObjects",
            "Google Docs positioned objects are not representable and were dropped",
        ),
    ];
    for (key, message) in PARTS {
        if google_part_is_present(value.get(key)) {
            warnings.push(warning(google_style::DROPPED_DOCUMENT_PART, message));
        }
    }
}

fn google_part_is_present(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Object(map)) => !map.is_empty(),
        Some(Value::Array(items)) => !items.is_empty(),
        Some(_) => true,
    }
}

fn validate_google_docs_exportable_structure(blocks: &[Block]) -> Result<(), ImportError> {
    for block in blocks {
        if let BlockKind::Table { rows, .. } = &block.kind {
            if rows.is_empty() {
                return Err(ImportError::UnsupportedStructure(
                    "OpenDoc table has no rows".to_string(),
                ));
            }
            for row in rows {
                if row.cells.is_empty() {
                    return Err(ImportError::UnsupportedStructure(
                        "OpenDoc table row has no cells".to_string(),
                    ));
                }
                for cell in &row.cells {
                    if cell.blocks.is_empty() {
                        return Err(ImportError::UnsupportedStructure(
                            "OpenDoc table cell has no blocks".to_string(),
                        ));
                    }
                    validate_google_docs_exportable_structure(&cell.blocks)?;
                }
            }
        }
    }
    Ok(())
}

fn import_document_title(title: String) -> Result<String, ImportError> {
    let title = title.trim();
    if title.is_empty() {
        return Err(ImportError::InvalidInput(
            "document title is empty".to_string(),
        ));
    }
    Ok(title.to_string())
}

pub(crate) fn mark(kind: MarkKind, value: Option<String>) -> Mark {
    Mark {
        kind,
        value,
        expand: MarkExpand::Both,
    }
}
