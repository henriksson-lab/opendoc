//! DOCX (WordprocessingML) reader.
//!
//! The package is opened with the `zip` crate and every part is parsed into a
//! small DOM (see [`crate::xml`]). The main document body is converted into the
//! OpenDoc model together with numbering definitions, style inheritance,
//! footnotes/endnotes, comment threads, tracked changes, images, tables,
//! hyperlinks, page breaks and Office Math equations. Properties that the core
//! model cannot represent are counted and reported as warnings (one per
//! property kind) instead of being silently dropped.

use crate::xml::parse_xml_bytes;
use crate::{ImportError, ImportedBlob};
use opendoc_core::{Document, ModelWarning};

mod convert;
mod package;
mod props;
mod revisions;
mod section;
mod styles;
mod util;
mod warnings;

#[cfg(test)]
mod unit_tests;

use convert::convert_parts;
use package::DocxParts;

pub(crate) struct DocxImport {
    pub(crate) document: Document,
    pub(crate) warnings: Vec<ModelWarning>,
    pub(crate) blobs: Vec<ImportedBlob>,
}

/// Imports either a zipped DOCX package or a raw `word/document.xml` payload.
pub(crate) fn import_docx_bytes(title: &str, bytes: &[u8]) -> Result<DocxImport, ImportError> {
    let parts = if bytes.starts_with(b"PK") {
        DocxParts::from_package(bytes)?
    } else {
        let xml = parse_xml_bytes(bytes).map_err(|err| {
            ImportError::InvalidInput(format!(
                "DOCX input is neither a ZIP package nor WordprocessingML XML: {err}"
            ))
        })?;
        if !xml.is("document") {
            return Err(ImportError::InvalidInput(
                "DOCX XML root element is not w:document".to_string(),
            ));
        }
        DocxParts::from_raw_document(xml)
    };
    convert_parts(title, &parts)
}

// ---------------------------------------------------------------------------
// Package / relationships
// ---------------------------------------------------------------------------
