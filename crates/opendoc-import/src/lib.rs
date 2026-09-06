use opendoc_citations::{merge_summary_with_citum_native_source, render_citation_group};
use opendoc_core::{
    Anchor, BibliographyReference, Block, BlockKind, CitationDatabase, CitationGroup, CitationItem,
    CitationPlacement, CitationSource, CitationSourceFormat, CitationSummary, Comment,
    CommentThread, Document, Equation, EquationSourceFormat, Footnote, Inline, Mark, MarkExpand,
    MarkKind, ModelWarning, StableId, Suggestion, SuggestionKind, SuggestionState, TableCell,
    TableRow, TextRange,
};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::path::Path;
use std::process::Command;

mod docx;
mod xml;

#[cfg(test)]
mod docx_tests;

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
    for element in content {
        if let Some(block) = import_google_structural_element(element, &mut warnings, true)? {
            document.blocks.push(block);
        }
    }
    document.footnotes = import_google_footnotes(&value, &mut warnings)?;
    document.citation_database = import_google_citations(&value, &mut warnings)?;
    document.comments = import_google_comments(&value, &mut warnings)?;
    document.suggestions = import_google_suggestions(&value, &mut warnings)?;
    repair_imported_citation_placements(&mut document, &mut warnings);
    repair_imported_citation_references(&mut document, &mut warnings);
    repair_imported_inline_citation_labels(&mut document, &mut warnings);
    refresh_imported_citation_projection_caches(&mut document);
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
    validate_google_docs_exportable_structure(&document.blocks)?;
    document
        .validate()
        .map_err(|err| ImportError::InvalidDocument(err.to_string()))?;
    let content = document
        .blocks
        .iter()
        .map(|block| export_google_block(block, true))
        .collect::<Result<Vec<_>, _>>()?;
    let mut value = json!({
        "title": document.title,
        "body": { "content": content },
    });
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
    serde_json::to_vec_pretty(&value).map_err(|err| ImportError::InvalidDocument(err.to_string()))
}

fn validate_google_docs_exportable_structure(blocks: &[Block]) -> Result<(), ImportError> {
    for block in blocks {
        if let BlockKind::Table { rows } = &block.kind {
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

#[derive(Debug, Eq, PartialEq)]
pub enum ImportError {
    EmptyInput,
    UnsupportedExtension(String),
    ConverterUnavailable,
    InvalidInput(String),
    InvalidDocument(String),
    UnsupportedStructure(String),
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for ImportError {}

fn import_document_title(title: String) -> Result<String, ImportError> {
    let title = title.trim();
    if title.is_empty() {
        return Err(ImportError::InvalidInput(
            "document title is empty".to_string(),
        ));
    }
    Ok(title.to_string())
}

/// Legacy binary `.doc` files are converted through an external converter
/// (pandoc or LibreOffice) into a plain-text projection. This is the only
/// import path that shells out; `.docx` is parsed natively.
fn convert_legacy_doc_to_plaintext(path: &Path) -> Result<String, ImportError> {
    if let Some(text) = try_pandoc_plaintext(path)? {
        return Ok(text);
    }
    if let Some(text) = try_libreoffice_plaintext(path)? {
        return Ok(text);
    }
    Err(ImportError::ConverterUnavailable)
}

fn validate_legacy_doc_container(path: &Path) -> Result<(), ImportError> {
    const OLE_COMPOUND_DOCUMENT_MAGIC: &[u8] = &[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1];
    let bytes = fs::read(path).map_err(|err| ImportError::InvalidInput(err.to_string()))?;
    if bytes.starts_with(OLE_COMPOUND_DOCUMENT_MAGIC) || bytes.starts_with(b"{\\rtf") {
        Ok(())
    } else {
        Err(ImportError::UnsupportedStructure(
            "legacy .doc import requires an OLE compound document or RTF payload".to_string(),
        ))
    }
}

fn try_pandoc_plaintext(path: &Path) -> Result<Option<String>, ImportError> {
    let Ok(output) = Command::new("pandoc")
        .arg(path)
        .args(["-t", "plain", "--wrap=none"])
        .output()
    else {
        return Ok(None);
    };
    if !output.status.success() {
        return Ok(None);
    }
    let text = String::from_utf8(output.stdout)
        .map_err(|err| ImportError::InvalidInput(err.to_string()))?;
    Ok(non_empty_text(text))
}

fn try_libreoffice_plaintext(path: &Path) -> Result<Option<String>, ImportError> {
    let out_dir =
        std::env::temp_dir().join(format!("opendoc-import-convert-{}", std::process::id()));
    let _ = fs::remove_dir_all(&out_dir);
    fs::create_dir_all(&out_dir).map_err(|err| ImportError::InvalidInput(err.to_string()))?;
    let output = Command::new("soffice")
        .args([
            "--headless",
            "--convert-to",
            "txt:Text",
            "--outdir",
            out_dir
                .to_str()
                .ok_or_else(|| ImportError::InvalidInput("invalid output path".to_string()))?,
        ])
        .arg(path)
        .output();
    let Ok(output) = output else {
        let _ = fs::remove_dir_all(out_dir);
        return Ok(None);
    };
    if !output.status.success() {
        let _ = fs::remove_dir_all(out_dir);
        return Ok(None);
    }
    let converted = out_dir.join(format!(
        "{}.txt",
        path.file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("document")
    ));
    let text = match fs::read_to_string(&converted) {
        Ok(text) => text,
        Err(_) => {
            let _ = fs::remove_dir_all(out_dir);
            return Ok(None);
        }
    };
    let _ = fs::remove_dir_all(out_dir);
    Ok(non_empty_text(text))
}

fn non_empty_text(text: String) -> Option<String> {
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}

fn import_google_paragraph(
    paragraph: &Value,
    warnings: &mut Vec<ModelWarning>,
) -> Result<Block, ImportError> {
    if google_paragraph_is_page_break(paragraph)? {
        return Ok(Block {
            id: StableId::new("block"),
            kind: BlockKind::PageBreak,
            content: Vec::new(),
            properties: Vec::new(),
        });
    }
    let style = optional_object(paragraph, "paragraphStyle")?.unwrap_or(&Value::Null);
    let named_style = optional_str(style, "namedStyleType")?.unwrap_or_default();
    let bullet = optional_object(paragraph, "bullet")?;
    let kind = if let Some(level) = heading_level(named_style) {
        BlockKind::Heading { level }
    } else if let Some(bullet) = bullet {
        let level = optional_u8(bullet, "nestingLevel")?.unwrap_or(0);
        if level > 8 {
            return Err(ImportError::InvalidInput(
                "nestingLevel is outside 0..=8".to_string(),
            ));
        }
        BlockKind::ListItem {
            list_id: parse_imported_stable_id(
                optional_str(bullet, "listId")?.unwrap_or("google-list"),
            )?,
            level,
            ordered: optional_bool(bullet, "ordered")?.unwrap_or(false),
        }
    } else {
        BlockKind::Paragraph
    };
    if style
        .as_object()
        .is_some_and(|object| object.keys().any(|key| key == "unsupportedStyle"))
    {
        warnings.push(ModelWarning {
            code: "unsupported-google-paragraph-style".to_string(),
            message: "ignored unsupported Google Docs paragraph style field".to_string(),
        });
    }
    Ok(Block {
        id: StableId::new("block"),
        kind,
        content: import_google_paragraph_elements(paragraph, warnings)?,
        properties: Vec::new(),
    })
}

fn google_paragraph_is_page_break(paragraph: &Value) -> Result<bool, ImportError> {
    let elements = paragraph
        .get("elements")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            ImportError::InvalidInput("paragraph.elements must be an array".to_string())
        })?;
    let mut has_page_break = false;
    let mut has_nonempty_text = false;
    for element in elements {
        if element.get("pageBreak").is_some() {
            has_page_break = true;
            if has_nonempty_text {
                return Err(ImportError::UnsupportedStructure(
                    "mixed Google Docs page break paragraph is unsupported".to_string(),
                ));
            }
        } else if let Some(run) = element.get("textRun") {
            let text = run
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim_end_matches('\n')
                .to_string();
            if !text.is_empty() {
                has_nonempty_text = true;
            }
            if has_page_break && has_nonempty_text {
                return Err(ImportError::UnsupportedStructure(
                    "mixed Google Docs page break paragraph is unsupported".to_string(),
                ));
            }
        } else if has_page_break {
            return Err(ImportError::UnsupportedStructure(
                "mixed Google Docs page break paragraph is unsupported".to_string(),
            ));
        }
    }
    Ok(has_page_break)
}

fn import_google_paragraph_elements(
    paragraph: &Value,
    warnings: &mut Vec<ModelWarning>,
) -> Result<Vec<opendoc_core::Inline>, ImportError> {
    let elements = paragraph
        .get("elements")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            ImportError::InvalidInput("paragraph.elements must be an array".to_string())
        })?;
    let mut inlines = Vec::new();
    for element in elements {
        if let Some(run) = element.get("textRun") {
            let text = run
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim_end_matches('\n')
                .to_string();
            if text.is_empty() {
                continue;
            }
            let style = optional_object(run, "textStyle")?.unwrap_or(&Value::Null);
            let marks = import_google_text_marks(style, warnings);
            if let Some(href) = import_google_text_link_href(style)? {
                inlines.push(opendoc_core::Inline::Link {
                    id: StableId::new("link"),
                    text,
                    href,
                    marks,
                });
            } else {
                inlines.push(opendoc_core::Inline::Text {
                    id: StableId::new("text"),
                    text,
                    marks,
                });
            }
        } else if element.get("inlineObjectElement").is_some() {
            return Err(ImportError::UnsupportedStructure(
                "inline objects require an explicit importer".to_string(),
            ));
        } else if element.get("footnoteReference").is_some() {
            let footnote_id = element
                .pointer("/footnoteReference/footnoteId")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    ImportError::UnsupportedStructure(
                        "Google Docs footnote reference missing footnoteId".to_string(),
                    )
                })?;
            inlines.push(Inline::FootnoteRef {
                id: StableId::new("footnote-ref"),
                footnote_id: parse_imported_stable_id(footnote_id)?,
            });
        } else if let Some(equation) = element.get("opendocEquation") {
            warnings.push(ModelWarning {
                code: "opendoc-google-equation-extension".to_string(),
                message: "imported OpenDoc inline equation extension from Google Docs-shaped JSON"
                    .to_string(),
            });
            inlines.push(import_opendoc_inline_equation(equation)?);
        } else if element.get("equation").is_some() {
            warnings.push(ModelWarning {
                code: "google-equation-source-unavailable".to_string(),
                message: "Google Docs API equation elements do not expose equation source"
                    .to_string(),
            });
            inlines.push(Inline::Equation {
                id: StableId::new("equation"),
                equation: Equation {
                    id: StableId::new("eq"),
                    source_format: EquationSourceFormat::LatexLike,
                    source: "\\placeholder{}".to_string(),
                },
            });
        } else if let Some(citation) = element.get("opendocCitation") {
            warnings.push(ModelWarning {
                code: "opendoc-google-citation-extension".to_string(),
                message: "imported OpenDoc citation extension from Google Docs-shaped JSON"
                    .to_string(),
            });
            let citation_id = citation
                .get("citationId")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    ImportError::UnsupportedStructure(
                        "OpenDoc citation element missing citationId".to_string(),
                    )
                })?;
            inlines.push(Inline::Citation {
                id: StableId::new("citation-label"),
                citation_id: parse_imported_stable_id(citation_id)?,
                rendered_cache: citation
                    .get("renderedCache")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
            });
        } else if let Some(mention) = element.get("opendocMention") {
            warnings.push(ModelWarning {
                code: "opendoc-google-mention-extension".to_string(),
                message: "imported OpenDoc mention extension from Google Docs-shaped JSON"
                    .to_string(),
            });
            let label = mention
                .get("label")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| {
                    ImportError::UnsupportedStructure(
                        "OpenDoc mention element missing label".to_string(),
                    )
                })?;
            let id = mention
                .get("inlineId")
                .and_then(Value::as_str)
                .map(parse_imported_stable_id)
                .transpose()?
                .unwrap_or_else(|| StableId::new("mention"));
            inlines.push(Inline::Mention {
                id,
                label: label.to_string(),
            });
        } else {
            return Err(ImportError::UnsupportedStructure(
                "unsupported high-risk Google Docs paragraph element".to_string(),
            ));
        }
    }
    Ok(inlines)
}

fn import_google_text_link_href(style: &Value) -> Result<Option<String>, ImportError> {
    let Some(link) = optional_object(style, "link")? else {
        return Ok(None);
    };
    let href = optional_str(link, "url")?.ok_or_else(|| {
        ImportError::UnsupportedStructure("Google Docs link missing url".to_string())
    })?;
    if href.trim().is_empty() {
        return Err(ImportError::UnsupportedStructure(
            "Google Docs link url is empty".to_string(),
        ));
    }
    if href.trim() != href {
        return Err(ImportError::UnsupportedStructure(
            "Google Docs link url has surrounding whitespace".to_string(),
        ));
    }
    Ok(Some(href.to_string()))
}

fn import_google_footnotes(
    value: &Value,
    warnings: &mut Vec<ModelWarning>,
) -> Result<Vec<Footnote>, ImportError> {
    let Some(footnotes_value) = value.get("footnotes") else {
        return Ok(Vec::new());
    };
    let footnotes = footnotes_value
        .as_object()
        .ok_or_else(|| ImportError::InvalidInput("footnotes must be an object".to_string()))?;
    let mut out = Vec::new();
    for (fallback_id, footnote) in footnotes {
        expect_object(footnote, "footnote")?;
        let id = optional_str(footnote, "footnoteId")?.unwrap_or(fallback_id);
        let content = required_array(footnote, "content", "footnote")?;
        let mut body = Vec::new();
        for element in content {
            expect_object(element, "footnote content element")?;
            if let Some(paragraph) = element.get("paragraph") {
                let mut inlines = import_google_paragraph_elements(paragraph, warnings)?;
                if !body.is_empty() && !body_ends_with_newline(&body) {
                    body.push(Inline::text("\n"));
                }
                body.append(&mut inlines);
            } else if element.get("startIndex").is_some() || element.get("endIndex").is_some() {
                warnings.push(ModelWarning {
                    code: "unsupported-google-footnote-metadata".to_string(),
                    message: "ignored Google Docs footnote metadata-only element".to_string(),
                });
            } else {
                return Err(ImportError::UnsupportedStructure(
                    "only paragraph footnote content is supported".to_string(),
                ));
            }
        }
        if body.is_empty() {
            body.push(Inline::text(" "));
        }
        out.push(Footnote {
            id: parse_imported_stable_id(id)?,
            revision: 1,
            body,
            deleted: false,
        });
    }
    out.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(out)
}

fn body_ends_with_newline(inlines: &[Inline]) -> bool {
    matches!(
        inlines.last(),
        Some(Inline::Text { text, .. }) if text.ends_with('\n')
    )
}

fn import_google_citations(
    value: &Value,
    warnings: &mut Vec<ModelWarning>,
) -> Result<CitationDatabase, ImportError> {
    let Some(citations) = value.get("opendocCitations") else {
        return Ok(CitationDatabase::default());
    };
    expect_object(citations, "opendocCitations")?;
    warnings.push(ModelWarning {
        code: "opendoc-google-citations-extension".to_string(),
        message: "imported document-local citation database from OpenDoc Google-shaped extension"
            .to_string(),
    });
    let mut database = CitationDatabase {
        style: optional_str(citations, "style")?
            .unwrap_or("apa-7th")
            .to_string(),
        locale: optional_str(citations, "locale")?
            .unwrap_or("en-US")
            .to_string(),
        references: Vec::new(),
        citations: Vec::new(),
    };
    if let Some(references) = optional_array(citations, "references")? {
        for reference in references {
            expect_object(reference, "citation reference")?;
            database
                .references
                .push(import_google_reference(reference)?);
        }
    }
    if let Some(groups) = optional_array(citations, "groups")? {
        for group in groups {
            expect_object(group, "citation group")?;
            database
                .citations
                .push(import_google_citation_group(group)?);
        }
    }
    database
        .references
        .sort_by(|left, right| left.id.cmp(&right.id));
    database
        .citations
        .sort_by(|left, right| left.id.cmp(&right.id));
    Ok(database)
}

fn import_google_reference(value: &Value) -> Result<BibliographyReference, ImportError> {
    let id = required_str(value, "id", "citation reference")?;
    let summary_value = value.get("summary").unwrap_or(&Value::Null);
    if !summary_value.is_null() {
        expect_object(summary_value, "citation summary")?;
    }
    let format =
        citation_source_format_from_label(optional_str(value, "format")?.unwrap_or("citum-native"));
    let source_bytes = optional_str(value, "bytesUtf8")?
        .unwrap_or_default()
        .as_bytes()
        .to_vec();
    let authors = optional_array(summary_value, "authors")?
        .map(|authors| {
            authors
                .iter()
                .map(|author| {
                    author.as_str().map(ToString::to_string).ok_or_else(|| {
                        ImportError::InvalidInput(
                            "citation summary authors entries must be strings".to_string(),
                        )
                    })
                })
                .collect()
        })
        .transpose()?
        .unwrap_or_default();
    let mut summary = CitationSummary {
        title: optional_str(summary_value, "title")?
            .unwrap_or_default()
            .to_string(),
        authors,
        issued: optional_source_string(summary_value, "issued", "bibliography summary field")?,
        doi: optional_source_string(summary_value, "doi", "bibliography summary field")?,
        url: optional_source_string(summary_value, "url", "bibliography summary field")?,
    };
    if format == CitationSourceFormat::CitumNative {
        summary = merge_summary_with_citum_native_source(summary, &source_bytes);
    }
    Ok(BibliographyReference {
        id: parse_imported_stable_id(id)?,
        revision: optional_u64(value, "revision")?.unwrap_or(1),
        source: CitationSource {
            format,
            bytes: source_bytes,
        },
        summary,
        deleted: optional_bool(value, "deleted")?.unwrap_or(false),
    })
}

fn import_google_citation_group(value: &Value) -> Result<CitationGroup, ImportError> {
    let id = required_str(value, "id", "citation group")?;
    let items = required_array(value, "items", "citation group")?
        .iter()
        .map(|item| {
            expect_object(item, "citation item")?;
            import_google_citation_item(item)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let placement = match optional_str(value, "placement")?.unwrap_or("inline") {
        "inline" => CitationPlacement::Inline,
        "footnote" => CitationPlacement::Footnote {
            footnote_id: parse_imported_stable_id(required_str(
                value,
                "footnoteId",
                "footnote citation",
            )?)?,
        },
        other => {
            return Err(ImportError::InvalidInput(format!(
                "unsupported citation placement {other}"
            )));
        }
    };
    Ok(CitationGroup {
        id: parse_imported_stable_id(id)?,
        revision: optional_u64(value, "revision")?.unwrap_or(1),
        items,
        placement,
        rendered_cache: optional_checked_string(value, "renderedCache")?,
        deleted: optional_bool(value, "deleted")?.unwrap_or(false),
    })
}

fn import_google_citation_item(value: &Value) -> Result<CitationItem, ImportError> {
    let reference_id = required_str(value, "referenceId", "citation item")?;
    Ok(CitationItem {
        reference_id: parse_imported_stable_id(reference_id)?,
        locator: optional_source_string(value, "locator", "citation item field")?,
        label: optional_source_string(value, "label", "citation item field")?,
        prefix: optional_source_string(value, "prefix", "citation item field")?,
        suffix: optional_source_string(value, "suffix", "citation item field")?,
        suppress_author: optional_bool(value, "suppressAuthor")?.unwrap_or(false),
    })
}

fn repair_imported_citation_placements(document: &mut Document, warnings: &mut Vec<ModelWarning>) {
    let live_footnotes = document
        .footnotes
        .iter()
        .filter(|footnote| !footnote.deleted)
        .map(|footnote| footnote.id.clone())
        .collect::<BTreeSet<_>>();
    for citation in &mut document.citation_database.citations {
        if citation.deleted {
            continue;
        }
        let CitationPlacement::Footnote { footnote_id } = &citation.placement else {
            continue;
        };
        if live_footnotes.contains(footnote_id) {
            continue;
        }
        let missing_footnote_id = footnote_id.clone();
        citation.placement = CitationPlacement::Inline;
        citation.rendered_cache = None;
        clear_imported_inline_citation_cache(&mut document.blocks, &citation.id);
        warnings.push(ModelWarning {
            code: "citation-footnote-target-missing".to_string(),
            message: format!(
                "citation group {} moved inline because footnote {missing_footnote_id} was missing",
                citation.id
            ),
        });
    }
}

fn repair_imported_citation_references(document: &mut Document, warnings: &mut Vec<ModelWarning>) {
    let live_references = document
        .citation_database
        .references
        .iter()
        .filter(|reference| !reference.deleted)
        .map(|reference| reference.id.clone())
        .collect::<BTreeSet<_>>();
    let mut affected_citations = Vec::new();
    for citation in &mut document.citation_database.citations {
        if citation.deleted {
            continue;
        }
        if citation
            .items
            .iter()
            .any(|item| !live_references.contains(&item.reference_id))
        {
            citation.rendered_cache = None;
            affected_citations.push(citation.id.clone());
        }
    }
    affected_citations.sort();
    affected_citations.dedup();
    for citation_id in affected_citations {
        clear_imported_inline_citation_cache(&mut document.blocks, &citation_id);
        warnings.push(ModelWarning {
            code: "citation-reference-missing".to_string(),
            message: format!(
                "citation group {citation_id} references a missing bibliography record"
            ),
        });
    }
}

fn clear_imported_inline_citation_cache(blocks: &mut [Block], citation_id: &StableId) {
    for block in blocks {
        for inline in &mut block.content {
            if let Inline::Citation {
                citation_id: inline_citation_id,
                rendered_cache,
                ..
            } = inline
            {
                if inline_citation_id == citation_id {
                    *rendered_cache = None;
                }
            }
        }
        if let BlockKind::Table { rows } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    clear_imported_inline_citation_cache(&mut cell.blocks, citation_id);
                }
            }
        }
    }
}

fn repair_imported_inline_citation_labels(
    document: &mut Document,
    warnings: &mut Vec<ModelWarning>,
) {
    let live_citations = document
        .citation_database
        .citations
        .iter()
        .filter(|citation| !citation.deleted)
        .map(|citation| citation.id.clone())
        .collect::<BTreeSet<_>>();
    let mut affected = BTreeSet::new();
    clear_imported_missing_inline_citation_caches(
        &mut document.blocks,
        &live_citations,
        &mut affected,
    );
    for citation_id in affected {
        warnings.push(ModelWarning {
            code: "citation-group-missing".to_string(),
            message: format!(
                "inline citation label {citation_id} references a missing citation group"
            ),
        });
    }
}

fn refresh_imported_citation_projection_caches(document: &mut Document) {
    let live_references = document
        .citation_database
        .references
        .iter()
        .filter(|reference| !reference.deleted)
        .map(|reference| reference.id.clone())
        .collect::<BTreeSet<_>>();
    let database = document.citation_database.clone();
    for citation in &mut document.citation_database.citations {
        if citation.deleted
            || citation
                .items
                .iter()
                .any(|item| !live_references.contains(&item.reference_id))
        {
            citation.rendered_cache = None;
        } else {
            let rendered = render_citation_group(&database, citation);
            citation.rendered_cache = if rendered == format!("[{}]", citation.id) {
                None
            } else {
                Some(rendered)
            };
        }
    }
}

fn clear_imported_missing_inline_citation_caches(
    blocks: &mut [Block],
    live_citations: &BTreeSet<StableId>,
    affected: &mut BTreeSet<StableId>,
) {
    for block in blocks {
        for inline in &mut block.content {
            if let Inline::Citation {
                citation_id,
                rendered_cache,
                ..
            } = inline
            {
                if !live_citations.contains(citation_id) {
                    *rendered_cache = None;
                    affected.insert(citation_id.clone());
                }
            }
        }
        if let BlockKind::Table { rows } = &mut block.kind {
            for row in rows {
                for cell in &mut row.cells {
                    clear_imported_missing_inline_citation_caches(
                        &mut cell.blocks,
                        live_citations,
                        affected,
                    );
                }
            }
        }
    }
}

fn required_str<'a>(value: &'a Value, key: &str, label: &str) -> Result<&'a str, ImportError> {
    value.get(key).and_then(Value::as_str).ok_or_else(|| {
        ImportError::InvalidInput(format!("{label} missing required string field {key}"))
    })
}

fn parse_imported_stable_id(value: &str) -> Result<StableId, ImportError> {
    StableId::parse(value.trim()).map_err(|err| ImportError::InvalidInput(err.to_string()))
}

fn required_array<'a>(
    value: &'a Value,
    key: &str,
    label: &str,
) -> Result<&'a Vec<Value>, ImportError> {
    value.get(key).and_then(Value::as_array).ok_or_else(|| {
        ImportError::InvalidInput(format!("{label} missing required array field {key}"))
    })
}

fn optional_object<'a>(value: &'a Value, key: &str) -> Result<Option<&'a Value>, ImportError> {
    match value.get(key) {
        Some(Value::Null) | None => Ok(None),
        Some(field) => {
            expect_object(field, key)?;
            Ok(Some(field))
        }
    }
}

fn optional_array<'a>(value: &'a Value, key: &str) -> Result<Option<&'a Vec<Value>>, ImportError> {
    match value.get(key) {
        Some(Value::Null) | None => Ok(None),
        Some(field) => field
            .as_array()
            .map(Some)
            .ok_or_else(|| ImportError::InvalidInput(format!("{key} must be an array"))),
    }
}

fn expect_object(value: &Value, label: &str) -> Result<(), ImportError> {
    if value.is_object() {
        Ok(())
    } else {
        Err(ImportError::InvalidInput(format!(
            "{label} must be an object"
        )))
    }
}

fn optional_bool(value: &Value, key: &str) -> Result<Option<bool>, ImportError> {
    match value.get(key) {
        Some(Value::Null) | None => Ok(None),
        Some(field) => field
            .as_bool()
            .map(Some)
            .ok_or_else(|| ImportError::InvalidInput(format!("{key} must be a boolean"))),
    }
}

fn optional_u64(value: &Value, key: &str) -> Result<Option<u64>, ImportError> {
    match value.get(key) {
        Some(Value::Null) | None => Ok(None),
        Some(field) => field.as_u64().map(Some).ok_or_else(|| {
            ImportError::InvalidInput(format!("{key} must be a non-negative integer"))
        }),
    }
}

fn optional_u8(value: &Value, key: &str) -> Result<Option<u8>, ImportError> {
    let Some(raw) = optional_u64(value, key)? else {
        return Ok(None);
    };
    u8::try_from(raw)
        .map(Some)
        .map_err(|_| ImportError::InvalidInput(format!("{key} is too large")))
}

fn optional_str<'a>(value: &'a Value, key: &str) -> Result<Option<&'a str>, ImportError> {
    match value.get(key) {
        Some(Value::Null) | None => Ok(None),
        Some(field) => field
            .as_str()
            .map(Some)
            .ok_or_else(|| ImportError::InvalidInput(format!("{key} must be a string"))),
    }
}

fn optional_checked_string(value: &Value, key: &str) -> Result<Option<String>, ImportError> {
    Ok(optional_str(value, key)?
        .filter(|value| !value.is_empty())
        .map(ToString::to_string))
}

fn optional_source_string(
    value: &Value,
    key: &str,
    label: &str,
) -> Result<Option<String>, ImportError> {
    let Some(raw) = optional_str(value, key)? else {
        return Ok(None);
    };
    source_string(raw, label).map(Some)
}

fn required_source_string(
    value: &Value,
    key: &str,
    required_label: &str,
    source_label: &str,
) -> Result<String, ImportError> {
    let raw = required_str(value, key, required_label)?;
    source_string(raw, source_label)
}

fn source_string(raw: &str, label: &str) -> Result<String, ImportError> {
    if raw.trim().is_empty() {
        return Err(ImportError::InvalidInput(format!("{label} is empty")));
    }
    if raw.trim() != raw {
        return Err(ImportError::InvalidInput(format!(
            "{label} has surrounding whitespace"
        )));
    }
    Ok(raw.to_string())
}

fn citation_source_format_from_label(label: &str) -> CitationSourceFormat {
    match label {
        "citum-native" => CitationSourceFormat::CitumNative,
        "csl-json" => CitationSourceFormat::CslJson,
        "bibtex" => CitationSourceFormat::Bibtex,
        "ris" => CitationSourceFormat::Ris,
        other => CitationSourceFormat::Unknown(other.to_string()),
    }
}

fn import_google_comments(
    value: &Value,
    warnings: &mut Vec<ModelWarning>,
) -> Result<Vec<CommentThread>, ImportError> {
    let Some(threads) = optional_array(value, "opendocComments")? else {
        return Ok(Vec::new());
    };
    warnings.push(ModelWarning {
        code: "opendoc-google-comments-extension".to_string(),
        message: "imported OpenDoc comments extension from Google Docs-shaped JSON".to_string(),
    });
    threads
        .iter()
        .map(|thread| {
            expect_object(thread, "comment thread")?;
            import_google_comment_thread(thread)
        })
        .collect()
}

fn import_google_comment_thread(value: &Value) -> Result<CommentThread, ImportError> {
    let id = required_str(value, "id", "comment thread")?;
    let comments = value
        .get("comments")
        .and_then(Value::as_array)
        .ok_or_else(|| ImportError::InvalidInput("comment thread comments missing".to_string()))?
        .iter()
        .map(|comment| {
            expect_object(comment, "comment")?;
            import_google_comment(comment)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(CommentThread {
        id: parse_imported_stable_id(id)?,
        anchor: import_google_anchor(value.get("anchor").unwrap_or(&Value::Null))?,
        comments,
        deleted: optional_bool(value, "deleted")?.unwrap_or(false),
    })
}

fn import_google_comment(value: &Value) -> Result<Comment, ImportError> {
    let id = required_str(value, "id", "comment")?;
    Ok(Comment {
        id: parse_imported_stable_id(id)?,
        author: required_source_string(value, "author", "comment", "comment author")?,
        body: import_google_inline_body(value.get("body").unwrap_or(&Value::Null))?,
        created_at_ms: optional_u64(value, "createdAtMs")?.unwrap_or(0),
        deleted: optional_bool(value, "deleted")?.unwrap_or(false),
    })
}

fn import_google_suggestions(
    value: &Value,
    warnings: &mut Vec<ModelWarning>,
) -> Result<Vec<Suggestion>, ImportError> {
    let Some(suggestions) = optional_array(value, "opendocSuggestions")? else {
        return Ok(Vec::new());
    };
    warnings.push(ModelWarning {
        code: "opendoc-google-suggestions-extension".to_string(),
        message: "imported OpenDoc suggestions extension from Google Docs-shaped JSON".to_string(),
    });
    suggestions
        .iter()
        .map(|suggestion| {
            expect_object(suggestion, "suggestion")?;
            import_google_suggestion(suggestion)
        })
        .collect()
}

fn import_google_suggestion(value: &Value) -> Result<Suggestion, ImportError> {
    let id = required_str(value, "id", "suggestion")?;
    let kind = value
        .get("kind")
        .ok_or_else(|| ImportError::InvalidInput("suggestion kind missing".to_string()))?;
    expect_object(kind, "suggestion kind")?;
    Ok(Suggestion {
        id: parse_imported_stable_id(id)?,
        author: required_source_string(value, "author", "suggestion", "suggestion author")?,
        kind: import_google_suggestion_kind(kind)?,
        state: import_google_suggestion_state(optional_str(value, "state")?.unwrap_or("proposed"))?,
        provenance: optional_array(value, "provenance")?
            .map(|items| {
                items
                    .iter()
                    .map(|item| {
                        let Some(raw) = item.as_str() else {
                            return Err(ImportError::InvalidInput(
                                "suggestion provenance entries must be strings".to_string(),
                            ));
                        };
                        source_string(raw, "suggestion provenance entry")
                    })
                    .collect()
            })
            .transpose()?
            .unwrap_or_default(),
    })
}

fn import_google_suggestion_kind(value: &Value) -> Result<SuggestionKind, ImportError> {
    match required_str(value, "type", "suggestion kind")? {
        "insert" => Ok(SuggestionKind::Insert {
            anchor: import_google_anchor(value.get("anchor").unwrap_or(&Value::Null))?,
            content: import_google_inline_body(value.get("content").unwrap_or(&Value::Null))?,
        }),
        "delete" => Ok(SuggestionKind::Delete {
            range: import_google_range(value.get("range").unwrap_or(&Value::Null))?,
        }),
        "format" => Ok(SuggestionKind::Format {
            range: import_google_range(value.get("range").unwrap_or(&Value::Null))?,
            marks: import_google_text_marks(
                value.get("textStyle").unwrap_or(&Value::Null),
                &mut Vec::new(),
            ),
        }),
        other => Err(ImportError::InvalidInput(format!(
            "unsupported suggestion kind {other}"
        ))),
    }
}

fn import_google_suggestion_state(value: &str) -> Result<SuggestionState, ImportError> {
    match value {
        "accepted" => Ok(SuggestionState::Accepted),
        "rejected" => Ok(SuggestionState::Rejected),
        "proposed" => Ok(SuggestionState::Proposed),
        other => Err(ImportError::InvalidInput(format!(
            "unsupported suggestion state {other}"
        ))),
    }
}

fn import_google_anchor(value: &Value) -> Result<Anchor, ImportError> {
    if value.is_null() {
        return Ok(Anchor::Document);
    }
    expect_object(value, "anchor")?;
    match optional_str(value, "type")?.unwrap_or("document") {
        "textRange" => Ok(Anchor::TextRange(import_google_range(value)?)),
        "nearestBlock" => {
            let block_id = required_str(value, "blockId", "nearest block anchor")?;
            Ok(Anchor::NearestBlock {
                block_id: parse_imported_stable_id(block_id)?,
                warning: optional_source_string(value, "warning", "nearest block anchor warning")?
                    .unwrap_or_else(|| "imported degraded anchor".to_string()),
            })
        }
        "document" => Ok(Anchor::Document),
        other => Err(ImportError::InvalidInput(format!(
            "unsupported anchor type {other}"
        ))),
    }
}

fn import_google_range(value: &Value) -> Result<TextRange, ImportError> {
    let start = required_str(value, "start", "text range")?;
    let end = required_str(value, "end", "text range")?;
    Ok(TextRange {
        start: parse_imported_stable_id(start)?,
        end: parse_imported_stable_id(end)?,
    })
}

fn import_google_inline_body(value: &Value) -> Result<Vec<Inline>, ImportError> {
    let elements = value.as_array().ok_or_else(|| {
        ImportError::InvalidInput("inline body must be an array of paragraph elements".to_string())
    })?;
    let paragraph = json!({ "elements": elements });
    let mut warnings = Vec::new();
    let mut body = import_google_paragraph_elements(&paragraph, &mut warnings)?;
    if body.is_empty() {
        body.push(Inline::text(" "));
    }
    Ok(body)
}

fn import_google_text_marks(style: &Value, warnings: &mut Vec<ModelWarning>) -> Vec<Mark> {
    let mut marks = Vec::new();
    let simple = [
        ("bold", MarkKind::Bold),
        ("italic", MarkKind::Italic),
        ("underline", MarkKind::Underline),
        ("strikethrough", MarkKind::Strike),
        ("baselineOffset", MarkKind::Superscript),
    ];
    for (key, kind) in simple {
        if key == "baselineOffset" {
            if style.get(key).and_then(Value::as_str) == Some("SUPERSCRIPT") {
                marks.push(mark(kind, None));
            } else if style.get(key).and_then(Value::as_str) == Some("SUBSCRIPT") {
                marks.push(mark(MarkKind::Subscript, None));
            }
        } else if style.get(key).and_then(Value::as_bool) == Some(true) {
            marks.push(mark(kind, None));
        }
    }
    if let Some(font) = style
        .pointer("/weightedFontFamily/fontFamily")
        .and_then(Value::as_str)
    {
        marks.push(mark(MarkKind::Font, Some(font.to_string())));
    }
    if let Some(size) = style.pointer("/fontSize/magnitude").and_then(Value::as_f64) {
        marks.push(mark(MarkKind::Size, Some(trim_float(size))));
    }
    if let Some(color) = style
        .pointer("/foregroundColor/color/rgbColor")
        .and_then(import_google_rgb)
    {
        marks.push(mark(MarkKind::Color, Some(color)));
    }
    if let Some(color) = style
        .pointer("/backgroundColor/color/rgbColor")
        .and_then(import_google_rgb)
    {
        marks.push(mark(MarkKind::Background, Some(color)));
    }
    if style
        .as_object()
        .is_some_and(|object| object.contains_key("smallCaps"))
    {
        warnings.push(ModelWarning {
            code: "unsupported-google-text-style".to_string(),
            message: "ignored unsupported Google Docs text style field".to_string(),
        });
    }
    marks
}

fn import_google_table(
    table: &Value,
    warnings: &mut Vec<ModelWarning>,
) -> Result<Block, ImportError> {
    let rows = table
        .get("tableRows")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            ImportError::UnsupportedStructure("malformed Google Docs table".to_string())
        })?
        .iter()
        .map(|row| {
            let cells = row
                .get("tableCells")
                .and_then(Value::as_array)
                .ok_or_else(|| {
                    ImportError::UnsupportedStructure("malformed Google Docs table row".to_string())
                })?
                .iter()
                .map(|cell| {
                    let mut content = cell
                        .get("content")
                        .and_then(Value::as_array)
                        .ok_or_else(|| {
                            ImportError::UnsupportedStructure(
                                "malformed Google Docs table cell".to_string(),
                            )
                        })?
                        .iter()
                        .map(|element| import_google_structural_element(element, warnings, false))
                        .filter_map(|block| block.transpose())
                        .collect::<Result<Vec<_>, _>>()?;
                    if content.is_empty() {
                        warnings.push(ModelWarning {
                            code: "google-empty-table-cell-normalized".to_string(),
                            message: "empty Google Docs table cell imported as a blank paragraph"
                                .to_string(),
                        });
                        content.push(Block::paragraph(""));
                    }
                    Ok(TableCell {
                        id: StableId::new("cell"),
                        blocks: content,
                        properties: Vec::new(),
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            let cells = if cells.is_empty() {
                warnings.push(ModelWarning {
                    code: "google-empty-table-row-normalized".to_string(),
                    message: "empty Google Docs table row imported with a blank cell".to_string(),
                });
                vec![TableCell {
                    id: StableId::new("cell"),
                    blocks: vec![Block::paragraph("")],
                    properties: Vec::new(),
                }]
            } else {
                cells
            };
            Ok(TableRow {
                id: StableId::new("row"),
                cells,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let rows = if rows.is_empty() {
        warnings.push(ModelWarning {
            code: "google-empty-table-normalized".to_string(),
            message: "empty Google Docs table imported with a blank row and cell".to_string(),
        });
        vec![TableRow {
            id: StableId::new("row"),
            cells: vec![TableCell {
                id: StableId::new("cell"),
                blocks: vec![Block::paragraph("")],
                properties: Vec::new(),
            }],
        }]
    } else {
        rows
    };
    Ok(Block {
        id: StableId::new("block"),
        kind: BlockKind::Table { rows },
        content: Vec::new(),
        properties: Vec::new(),
    })
}

fn import_google_structural_element(
    element: &Value,
    warnings: &mut Vec<ModelWarning>,
    allow_tables: bool,
) -> Result<Option<Block>, ImportError> {
    if let Some(paragraph) = element.get("paragraph") {
        Ok(Some(import_google_paragraph(paragraph, warnings)?))
    } else if let Some(table) = element.get("table") {
        if allow_tables {
            Ok(Some(import_google_table(table, warnings)?))
        } else {
            Err(ImportError::UnsupportedStructure(
                "nested Google Docs tables are unsupported".to_string(),
            ))
        }
    } else if let Some(equation) = element.get("opendocEquationBlock") {
        Ok(Some(import_opendoc_equation_block(equation)?))
    } else if let Some(image) = element.get("opendocImage") {
        Ok(Some(import_opendoc_image(image)?))
    } else if element.get("sectionBreak").is_some() {
        Ok(Some(Block {
            id: StableId::new("block"),
            kind: BlockKind::PageBreak,
            content: Vec::new(),
            properties: Vec::new(),
        }))
    } else if element.get("startIndex").is_some() || element.get("endIndex").is_some() {
        warnings.push(ModelWarning {
            code: "unsupported-google-structural-element".to_string(),
            message: "ignored Google Docs structural metadata-only element".to_string(),
        });
        Ok(None)
    } else {
        Err(ImportError::UnsupportedStructure(
            "unsupported high-risk Google Docs structural element".to_string(),
        ))
    }
}

fn export_google_block(block: &Block, allow_tables: bool) -> Result<Value, ImportError> {
    match &block.kind {
        BlockKind::Paragraph => Ok(json!({
            "paragraph": { "elements": export_google_inlines(&block.content)? }
        })),
        BlockKind::Heading { level } => Ok(json!({
            "paragraph": {
                "paragraphStyle": { "namedStyleType": format!("HEADING_{level}") },
                "elements": export_google_inlines(&block.content)?
            }
        })),
        BlockKind::ListItem {
            list_id,
            level,
            ordered,
        } => Ok(json!({
            "paragraph": {
                "bullet": {
                    "listId": list_id.to_string(),
                    "nestingLevel": level,
                    "ordered": ordered,
                },
                "elements": export_google_inlines(&block.content)?
            }
        })),
        BlockKind::Table { rows } => {
            if !allow_tables {
                return Err(ImportError::UnsupportedStructure(
                    "nested OpenDoc tables cannot be exported to Google Docs-shaped JSON v0"
                        .to_string(),
                ));
            }
            if rows.is_empty() {
                return Err(ImportError::UnsupportedStructure(
                    "OpenDoc table has no rows".to_string(),
                ));
            }
            Ok(json!({
                "table": {
                    "tableRows": rows
                        .iter()
                        .map(export_google_table_row)
                        .collect::<Result<Vec<_>, _>>()?
                }
            }))
        }
        BlockKind::PageBreak => Ok(json!({
            "paragraph": {
                "elements": [{ "pageBreak": {} }]
            }
        })),
        BlockKind::EquationBlock { equation } => {
            if equation.source.trim().is_empty() {
                return Err(ImportError::UnsupportedStructure(
                    "OpenDoc block equation source is empty".to_string(),
                ));
            }
            Ok(json!({
                "opendocEquationBlock": {
                    "blockId": block.id.to_string(),
                    "equationId": equation.id.to_string(),
                    "sourceFormat": export_equation_source_format(&equation.source_format),
                    "source": equation.source
                }
            }))
        }
        BlockKind::Image {
            blob_hash,
            alt_text,
        } => {
            opendoc_core::HashRef::parse(blob_hash).map_err(|err| {
                ImportError::UnsupportedStructure(format!(
                    "OpenDoc image blobHash is invalid: {err}"
                ))
            })?;
            Ok(json!({
                "opendocImage": {
                    "blockId": block.id.to_string(),
                    "blobHash": blob_hash,
                    "altText": alt_text
                }
            }))
        }
    }
}

fn import_opendoc_equation_block(equation: &Value) -> Result<Block, ImportError> {
    let source = equation
        .get("source")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            ImportError::UnsupportedStructure("OpenDoc block equation missing source".to_string())
        })?
        .to_string();
    let source_format = equation
        .get("sourceFormat")
        .and_then(Value::as_str)
        .map(import_equation_source_format)
        .transpose()?
        .unwrap_or(EquationSourceFormat::LatexLike);
    let block_id = equation
        .get("blockId")
        .and_then(Value::as_str)
        .map(parse_imported_stable_id)
        .transpose()?
        .unwrap_or_else(|| StableId::new("block"));
    let equation_id = equation
        .get("equationId")
        .and_then(Value::as_str)
        .map(parse_imported_stable_id)
        .transpose()?
        .unwrap_or_else(|| StableId::new("eq"));
    Ok(Block {
        id: block_id,
        kind: BlockKind::EquationBlock {
            equation: Equation {
                id: equation_id,
                source_format,
                source,
            },
        },
        content: Vec::new(),
        properties: Vec::new(),
    })
}

fn import_opendoc_inline_equation(equation: &Value) -> Result<Inline, ImportError> {
    let source = equation
        .get("source")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            ImportError::UnsupportedStructure("OpenDoc inline equation missing source".to_string())
        })?
        .to_string();
    let source_format = equation
        .get("sourceFormat")
        .and_then(Value::as_str)
        .map(import_equation_source_format)
        .transpose()?
        .unwrap_or(EquationSourceFormat::LatexLike);
    let inline_id = equation
        .get("inlineId")
        .and_then(Value::as_str)
        .map(parse_imported_stable_id)
        .transpose()?
        .unwrap_or_else(|| StableId::new("equation"));
    let equation_id = equation
        .get("equationId")
        .and_then(Value::as_str)
        .map(parse_imported_stable_id)
        .transpose()?
        .unwrap_or_else(|| StableId::new("eq"));
    Ok(Inline::Equation {
        id: inline_id,
        equation: Equation {
            id: equation_id,
            source_format,
            source,
        },
    })
}

fn import_equation_source_format(value: &str) -> Result<EquationSourceFormat, ImportError> {
    match value {
        "latex-like" => Ok(EquationSourceFormat::LatexLike),
        other => Err(ImportError::UnsupportedStructure(format!(
            "unsupported OpenDoc equation source format {other}"
        ))),
    }
}

fn export_equation_source_format(source_format: &EquationSourceFormat) -> &'static str {
    match source_format {
        EquationSourceFormat::LatexLike => "latex-like",
    }
}

fn import_opendoc_image(image: &Value) -> Result<Block, ImportError> {
    let blob_hash = image
        .get("blobHash")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            ImportError::UnsupportedStructure("OpenDoc image missing blobHash".to_string())
        })?
        .trim()
        .to_string();
    opendoc_core::HashRef::parse(&blob_hash).map_err(|err| {
        ImportError::UnsupportedStructure(format!("OpenDoc image blobHash is invalid: {err}"))
    })?;
    let alt_text = image
        .get("altText")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let id = image
        .get("blockId")
        .and_then(Value::as_str)
        .map(parse_imported_stable_id)
        .transpose()?
        .unwrap_or_else(|| StableId::new("block"));
    Ok(Block {
        id,
        kind: BlockKind::Image {
            blob_hash,
            alt_text,
        },
        content: Vec::new(),
        properties: Vec::new(),
    })
}

fn export_google_table_row(row: &TableRow) -> Result<Value, ImportError> {
    if row.cells.is_empty() {
        return Err(ImportError::UnsupportedStructure(
            "OpenDoc table row has no cells".to_string(),
        ));
    }
    Ok(json!({
        "tableCells": row
            .cells
            .iter()
            .map(|cell| {
                if cell.blocks.is_empty() {
                    return Err(ImportError::UnsupportedStructure(
                        "OpenDoc table cell has no blocks".to_string(),
                    ));
                }
                Ok(json!({
                    "content": cell
                        .blocks
                        .iter()
                        .map(|block| export_google_block(block, false))
                        .collect::<Result<Vec<_>, _>>()?
                }))
            })
            .collect::<Result<Vec<_>, ImportError>>()?
    }))
}

fn export_google_inlines(inlines: &[opendoc_core::Inline]) -> Result<Vec<Value>, ImportError> {
    inlines
        .iter()
        .map(|inline| {
            Ok(match inline {
                opendoc_core::Inline::Text { text, marks, .. } => json!({
                    "textRun": {
                        "content": text,
                        "textStyle": export_google_text_style(marks)?
                    }
                }),
                opendoc_core::Inline::Link {
                    text, href, marks, ..
                } => {
                    if href.trim().is_empty() {
                        return Err(ImportError::UnsupportedStructure(
                            "OpenDoc link href is empty".to_string(),
                        ));
                    }
                    let mut style = export_google_text_style(marks)?;
                    style["link"] = json!({ "url": href });
                    json!({ "textRun": { "content": text, "textStyle": style } })
                }
                opendoc_core::Inline::FootnoteRef { footnote_id, .. } => json!({
                    "footnoteReference": {
                        "footnoteId": footnote_id.to_string()
                    }
                }),
                opendoc_core::Inline::Equation { id, equation } => {
                    if equation.source.trim().is_empty() {
                        return Err(ImportError::UnsupportedStructure(
                            "OpenDoc inline equation source is empty".to_string(),
                        ));
                    }
                    json!({
                        "opendocEquation": {
                            "inlineId": id.to_string(),
                            "equationId": equation.id.to_string(),
                            "sourceFormat": export_equation_source_format(&equation.source_format),
                            "source": equation.source
                        }
                    })
                }
                opendoc_core::Inline::Citation {
                    citation_id,
                    rendered_cache,
                    ..
                } => json!({
                    "opendocCitation": {
                        "citationId": citation_id.to_string(),
                        "renderedCache": rendered_cache
                    }
                }),
                opendoc_core::Inline::Mention { id, label } => {
                    if label.trim().is_empty() {
                        return Err(ImportError::UnsupportedStructure(
                            "OpenDoc mention label is empty".to_string(),
                        ));
                    }
                    json!({
                        "opendocMention": {
                            "inlineId": id.to_string(),
                            "label": label
                        }
                    })
                }
            })
        })
        .collect()
}

fn export_google_footnotes(
    footnotes: &[Footnote],
) -> Result<serde_json::Map<String, Value>, ImportError> {
    let mut map = serde_json::Map::new();
    for footnote in footnotes.iter().filter(|footnote| !footnote.deleted) {
        map.insert(
            footnote.id.to_string(),
            json!({
                "footnoteId": footnote.id.to_string(),
                "content": [{
                    "paragraph": {
                        "elements": export_google_inlines(&footnote.body)?
                    }
                }]
            }),
        );
    }
    Ok(map)
}

fn should_export_citations(database: &CitationDatabase) -> bool {
    database.style != "apa-7th"
        || database.locale != "en-US"
        || !database.references.is_empty()
        || !database.citations.is_empty()
}

fn export_google_citations(database: &CitationDatabase) -> Result<Value, ImportError> {
    if database.style.trim().is_empty() || database.locale.trim().is_empty() {
        return Err(ImportError::UnsupportedStructure(
            "OpenDoc citation database metadata is empty".to_string(),
        ));
    }
    let references = database
        .references
        .iter()
        .map(export_google_reference)
        .collect::<Result<Vec<_>, _>>()?;
    let groups = database
        .citations
        .iter()
        .map(export_google_citation_group)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(json!({
        "style": database.style,
        "locale": database.locale,
        "references": references,
        "groups": groups
    }))
}

fn export_google_reference(reference: &BibliographyReference) -> Result<Value, ImportError> {
    reference
        .validate()
        .map_err(|err| ImportError::UnsupportedStructure(format!("OpenDoc {err}")))?;
    Ok(json!({
        "id": reference.id.to_string(),
        "revision": reference.revision,
        "format": citation_source_format_label(&reference.source.format),
        "bytesUtf8": String::from_utf8_lossy(&reference.source.bytes),
        "summary": {
            "title": reference.summary.title,
            "authors": reference.summary.authors,
            "issued": reference.summary.issued,
            "doi": reference.summary.doi,
            "url": reference.summary.url,
        },
        "deleted": reference.deleted
    }))
}

fn export_google_citation_group(citation: &CitationGroup) -> Result<Value, ImportError> {
    citation
        .validate_payload()
        .map_err(|err| ImportError::UnsupportedStructure(format!("OpenDoc {err}")))?;
    let mut value = json!({
        "id": citation.id.to_string(),
        "revision": citation.revision,
        "items": citation.items.iter().map(export_google_citation_item).collect::<Vec<_>>(),
        "placement": match &citation.placement {
            CitationPlacement::Inline => "inline".to_string(),
            CitationPlacement::Footnote { .. } => "footnote".to_string(),
        },
        "renderedCache": citation.rendered_cache,
        "deleted": citation.deleted
    });
    if let CitationPlacement::Footnote { footnote_id } = &citation.placement {
        value["footnoteId"] = json!(footnote_id.to_string());
    }
    Ok(value)
}

fn export_google_citation_item(item: &CitationItem) -> Value {
    json!({
        "referenceId": item.reference_id.to_string(),
        "locator": item.locator,
        "label": item.label,
        "prefix": item.prefix,
        "suffix": item.suffix,
        "suppressAuthor": item.suppress_author
    })
}

fn citation_source_format_label(format: &CitationSourceFormat) -> String {
    match format {
        CitationSourceFormat::CitumNative => "citum-native".to_string(),
        CitationSourceFormat::CslJson => "csl-json".to_string(),
        CitationSourceFormat::Bibtex => "bibtex".to_string(),
        CitationSourceFormat::Ris => "ris".to_string(),
        CitationSourceFormat::Unknown(value) => value.clone(),
    }
}

fn export_google_comments(comments: &[CommentThread]) -> Result<Value, ImportError> {
    Ok(Value::Array(
        comments
            .iter()
            .map(|thread| {
                Ok(json!({
                    "id": thread.id.to_string(),
                    "anchor": export_google_anchor(&thread.anchor),
                    "comments": thread.comments.iter().map(export_google_comment).collect::<Result<Vec<_>, _>>()?,
                    "deleted": thread.deleted
                }))
            })
            .collect::<Result<Vec<_>, ImportError>>()?,
    ))
}

fn export_google_comment(comment: &Comment) -> Result<Value, ImportError> {
    Ok(json!({
        "id": comment.id.to_string(),
        "author": comment.author,
        "body": export_google_inlines(&comment.body)?,
        "createdAtMs": comment.created_at_ms,
        "deleted": comment.deleted
    }))
}

fn export_google_suggestions(suggestions: &[Suggestion]) -> Result<Value, ImportError> {
    Ok(Value::Array(
        suggestions
            .iter()
            .map(export_google_suggestion)
            .collect::<Result<Vec<_>, _>>()?,
    ))
}

fn export_google_suggestion(suggestion: &Suggestion) -> Result<Value, ImportError> {
    Ok(json!({
        "id": suggestion.id.to_string(),
        "author": suggestion.author,
        "kind": export_google_suggestion_kind(&suggestion.kind)?,
        "state": export_google_suggestion_state(&suggestion.state),
        "provenance": suggestion.provenance
    }))
}

fn export_google_suggestion_kind(kind: &SuggestionKind) -> Result<Value, ImportError> {
    Ok(match kind {
        SuggestionKind::Insert { anchor, content } => json!({
            "type": "insert",
            "anchor": export_google_anchor(anchor),
            "content": export_google_inlines(content)?
        }),
        SuggestionKind::Delete { range } => json!({
            "type": "delete",
            "range": export_google_range(range)
        }),
        SuggestionKind::Format { range, marks } => json!({
            "type": "format",
            "range": export_google_range(range),
            "textStyle": export_google_text_style(marks)?
        }),
    })
}

fn export_google_suggestion_state(state: &SuggestionState) -> &'static str {
    match state {
        SuggestionState::Proposed => "proposed",
        SuggestionState::Accepted => "accepted",
        SuggestionState::Rejected => "rejected",
    }
}

fn export_google_anchor(anchor: &Anchor) -> Value {
    match anchor {
        Anchor::TextRange(range) => {
            let mut value = export_google_range(range);
            value["type"] = json!("textRange");
            value
        }
        Anchor::NearestBlock { block_id, warning } => json!({
            "type": "nearestBlock",
            "blockId": block_id.to_string(),
            "warning": warning
        }),
        Anchor::Document => json!({ "type": "document" }),
    }
}

fn export_google_range(range: &TextRange) -> Value {
    json!({
        "start": range.start.to_string(),
        "end": range.end.to_string()
    })
}

fn export_google_text_style(marks: &[Mark]) -> Result<Value, ImportError> {
    let mut style = json!({});
    for mark in marks {
        match mark.kind {
            MarkKind::Bold => {
                reject_boolean_mark_value(mark)?;
                style["bold"] = json!(true);
            }
            MarkKind::Italic => {
                reject_boolean_mark_value(mark)?;
                style["italic"] = json!(true);
            }
            MarkKind::Underline => {
                reject_boolean_mark_value(mark)?;
                style["underline"] = json!(true);
            }
            MarkKind::Strike => {
                reject_boolean_mark_value(mark)?;
                style["strikethrough"] = json!(true);
            }
            MarkKind::Superscript => {
                reject_boolean_mark_value(mark)?;
                style["baselineOffset"] = json!("SUPERSCRIPT");
            }
            MarkKind::Subscript => {
                reject_boolean_mark_value(mark)?;
                style["baselineOffset"] = json!("SUBSCRIPT");
            }
            MarkKind::Color => {
                let value = required_mark_value(mark)?;
                style["foregroundColor"] = export_google_color(value);
            }
            MarkKind::Background => {
                let value = required_mark_value(mark)?;
                style["backgroundColor"] = export_google_color(value);
            }
            MarkKind::Font => {
                let value = required_mark_value(mark)?;
                style["weightedFontFamily"] = json!({ "fontFamily": value });
            }
            MarkKind::Size => {
                let value = required_mark_value(mark)?;
                let magnitude = value.parse::<f64>().map_err(|_| {
                    ImportError::UnsupportedStructure(
                        "OpenDoc size mark value is invalid".to_string(),
                    )
                })?;
                style["fontSize"] = json!({ "magnitude": magnitude, "unit": "PT" });
            }
            MarkKind::Code | MarkKind::Link | MarkKind::Citation => {
                reject_boolean_mark_value(mark)?;
            }
        }
    }
    Ok(style)
}

fn required_mark_value(mark: &Mark) -> Result<&str, ImportError> {
    mark.value
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            ImportError::UnsupportedStructure("OpenDoc mark value is missing".to_string())
        })
}

fn reject_boolean_mark_value(mark: &Mark) -> Result<(), ImportError> {
    if mark.value.is_some() {
        return Err(ImportError::UnsupportedStructure(
            "OpenDoc boolean mark has value".to_string(),
        ));
    }
    Ok(())
}

fn heading_level(named_style: &str) -> Option<u8> {
    named_style
        .strip_prefix("HEADING_")
        .and_then(|level| level.parse::<u8>().ok())
        .filter(|level| (1..=6).contains(level))
}

pub(crate) fn mark(kind: MarkKind, value: Option<String>) -> Mark {
    Mark {
        kind,
        value,
        expand: MarkExpand::Both,
    }
}

fn import_google_rgb(value: &Value) -> Option<String> {
    let red = value.get("red").and_then(Value::as_f64).unwrap_or(0.0);
    let green = value.get("green").and_then(Value::as_f64).unwrap_or(0.0);
    let blue = value.get("blue").and_then(Value::as_f64).unwrap_or(0.0);
    Some(format!(
        "#{:02x}{:02x}{:02x}",
        float_color(red),
        float_color(green),
        float_color(blue)
    ))
}

fn export_google_color(value: &str) -> Value {
    let value = value.trim_start_matches('#');
    let red = u8::from_str_radix(value.get(0..2).unwrap_or("00"), 16).unwrap_or(0);
    let green = u8::from_str_radix(value.get(2..4).unwrap_or("00"), 16).unwrap_or(0);
    let blue = u8::from_str_radix(value.get(4..6).unwrap_or("00"), 16).unwrap_or(0);
    json!({
        "color": {
            "rgbColor": {
                "red": red as f64 / 255.0,
                "green": green as f64 / 255.0,
                "blue": blue as f64 / 255.0,
            }
        }
    })
}

fn float_color(value: f64) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn trim_float(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{}", value as i64)
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_export_error_contains(document: &Document, expected: &str) {
        let err = export_google_docs_json(document).unwrap_err();
        assert!(
            err.to_string().contains(expected),
            "expected export error containing {expected:?}, got {err:?}"
        );
    }

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

    #[test]
    fn mislabeled_legacy_doc_payload_aborts_before_converter_fallback() {
        let path = std::env::temp_dir().join(format!(
            "opendoc-import-mislabeled-legacy-doc-{}.doc",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        std::fs::write(&path, b"not a supported legacy Word binary").unwrap();

        assert!(matches!(
            import_doc_or_docx(&path),
            Err(ImportError::UnsupportedStructure(message))
                if message == "legacy .doc import requires an OLE compound document or RTF payload"
        ));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn imports_docx_xml_source_with_structure_and_marks_without_external_converter() {
        let path = std::env::temp_dir().join(format!(
            "opendoc-import-raw-docx-{}.docx",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        std::fs::write(
            &path,
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    <w:p>
      <w:pPr><w:pStyle w:val="Heading2"/></w:pPr>
      <w:r><w:rPr><w:b/></w:rPr><w:t>Structured heading</w:t></w:r>
    </w:p>
      <w:p>
        <w:pPr><w:numPr><w:ilvl w:val="1"/><w:numId w:val="7"/></w:numPr></w:pPr>
        <w:r><w:rPr><w:i/><w:u w:val="single"/><w:color w:val="336699"/><w:sz w:val="28"/></w:rPr><w:t>Marked item</w:t></w:r>
        <w:r><w:rPr><w:vertAlign w:val="superscript"/></w:rPr><w:t>sup</w:t></w:r>
        <w:r><w:rPr><w:vertAlign w:val="subscript"/></w:rPr><w:t>sub</w:t></w:r>
        <w:r><w:tab/><w:t>after tab</w:t><w:br/><w:t>after break</w:t></w:r>
      <w:hyperlink w:anchor="LocalBookmark"><w:r><w:t>bookmark</w:t></w:r></w:hyperlink>
    </w:p>
  </w:body>
</w:document>"#,
        )
        .unwrap();

        let report = import_doc_or_docx(&path).unwrap();
        assert_eq!(report.document.blocks.len(), 2);
        assert!(matches!(
            report.document.blocks[0].kind,
            BlockKind::Heading { level: 2 }
        ));
        let heading_marks = text_marks(&report.document.blocks[0]);
        assert!(heading_marks.iter().any(|mark| mark.kind == MarkKind::Bold));
        match &report.document.blocks[1].kind {
            BlockKind::ListItem { level, .. } => assert_eq!(*level, 1),
            other => panic!("expected DOCX list item, got {other:?}"),
        }
        let item_marks = text_marks(&report.document.blocks[1]);
        assert!(item_marks.iter().any(|mark| mark.kind == MarkKind::Italic));
        assert!(item_marks
            .iter()
            .any(|mark| mark.kind == MarkKind::Underline));
        assert!(item_marks
            .iter()
            .any(|mark| mark.kind == MarkKind::Color && mark.value.as_deref() == Some("#336699")));
        assert!(item_marks
            .iter()
            .any(|mark| mark.kind == MarkKind::Size && mark.value.as_deref() == Some("14")));
        assert!(report.document.blocks[1]
            .content
            .iter()
            .any(|inline| matches!(
                inline,
                Inline::Text { text, marks, .. }
                    if text == "sup" && marks.iter().any(|mark| mark.kind == MarkKind::Superscript)
            )));
        assert!(report.document.blocks[1]
            .content
            .iter()
            .any(|inline| matches!(
                inline,
                Inline::Text { text, marks, .. }
                    if text == "sub" && marks.iter().any(|mark| mark.kind == MarkKind::Subscript)
            )));
        assert_eq!(
            report.document.visible_text(),
            "Structured heading\nMarked itemsupsub\tafter tab\nafter breakbookmark\n"
        );
        assert!(report
            .document
            .blocks
            .iter()
            .any(|block| block.content.iter().any(|inline| matches!(
                inline,
                Inline::Link { text, href, .. }
                    if text == "bookmark" && href == "#LocalBookmark"
            ))));
        assert!(report.document.validate().is_ok());

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn imports_docx_office_math_as_equation_source_without_external_converter() {
        let path = std::env::temp_dir().join(format!(
            "opendoc-import-docx-math-{}.docx",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        std::fs::write(
            &path,
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math">
  <w:body>
    <w:p>
      <w:r><w:t>Before </w:t></w:r>
      <m:oMath><m:r><m:t>x+1</m:t></m:r></m:oMath>
      <w:r><w:t> after</w:t></w:r>
    </w:p>
    <w:p>
      <m:oMathPara>
        <m:oMath><m:r><m:t>\sum_i x_i</m:t></m:r></m:oMath>
      </m:oMathPara>
    </w:p>
  </w:body>
</w:document>"#,
        )
        .unwrap();

        let report = import_doc_or_docx(&path).unwrap();
        assert_eq!(report.document.blocks.len(), 2);
        let first = &report.document.blocks[0].content;
        assert!(matches!(
            (&first[0], &first[1], &first[2]),
            (
                Inline::Text { text: before, .. },
                Inline::Equation { equation, .. },
                Inline::Text { text: after, .. },
            ) if before == "Before "
                && equation.source == "x+1"
                && equation.source_format == EquationSourceFormat::LatexLike
                && after == " after"
        ));
        match &report.document.blocks[1].kind {
            BlockKind::EquationBlock { equation } => {
                assert_eq!(equation.source, "\\sum_i x_i");
                assert_eq!(equation.source_format, EquationSourceFormat::LatexLike);
            }
            other => panic!("expected DOCX Office Math equation block, got {other:?}"),
        }
        assert!(report.document.blocks[1].content.is_empty());
        assert!(report.document.validate().is_ok());

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn imports_raw_docx_xml_equation_only_source_without_text_runs() {
        let path = std::env::temp_dir().join(format!(
            "opendoc-import-docx-math-only-{}.docx",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        std::fs::write(
            &path,
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math">
  <w:body>
    <w:p>
      <m:oMathPara>
        <m:oMath><m:r><m:t>E=mc^2</m:t></m:r></m:oMath>
      </m:oMathPara>
    </w:p>
  </w:body>
</w:document>"#,
        )
        .unwrap();

        let report = import_doc_or_docx(&path).unwrap();
        assert_eq!(report.document.blocks.len(), 1);
        match &report.document.blocks[0].kind {
            BlockKind::EquationBlock { equation } => {
                assert_eq!(equation.source, "E=mc^2");
                assert_eq!(equation.source_format, EquationSourceFormat::LatexLike);
            }
            other => {
                panic!("expected equation-only DOCX to import as equation block, got {other:?}")
            }
        }
        assert!(report.document.blocks[0].content.is_empty());
        assert!(report.document.validate().is_ok());

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn imports_raw_docx_xml_page_break_only_source_without_text_runs() {
        let path = std::env::temp_dir().join(format!(
            "opendoc-import-docx-page-break-only-{}.docx",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        std::fs::write(
            &path,
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    <w:p><w:r><w:br w:type="page"/></w:r></w:p>
  </w:body>
</w:document>"#,
        )
        .unwrap();

        let report = import_doc_or_docx(&path).unwrap();
        assert_eq!(report.document.blocks.len(), 1);
        assert!(matches!(
            report.document.blocks[0].kind,
            BlockKind::PageBreak
        ));
        assert!(report.document.blocks[0].content.is_empty());
        assert_eq!(report.document.visible_text(), "\n");
        assert!(report.document.validate().is_ok());

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn imports_docx_tables_as_source_rows_cells_and_nested_blocks() {
        let path = std::env::temp_dir().join(format!(
            "opendoc-import-docx-table-{}.docx",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        std::fs::write(
            &path,
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math">
  <w:body>
    <w:p><w:r><w:t>Before table</w:t></w:r></w:p>
    <w:tbl>
      <w:tr>
        <w:tc><w:p><w:r><w:t>A1</w:t></w:r></w:p></w:tc>
        <w:tc><w:p><w:r><w:rPr><w:b/></w:rPr><w:t>B1</w:t></w:r></w:p></w:tc>
      </w:tr>
      <w:tr>
        <w:tc><w:p><w:r><w:t>A2 </w:t></w:r><m:oMath><m:r><m:t>x+2</m:t></m:r></m:oMath></w:p></w:tc>
        <w:tc></w:tc>
      </w:tr>
    </w:tbl>
    <w:p><w:r><w:t>After table</w:t></w:r></w:p>
  </w:body>
</w:document>"#,
        )
        .unwrap();

        let report = import_doc_or_docx(&path).unwrap();
        assert_eq!(report.document.blocks.len(), 3);
        assert_eq!(
            report.document.visible_text(),
            "Before table\nA1\tB1\nA2 x+2\t\nAfter table\n"
        );
        let BlockKind::Table { rows } = &report.document.blocks[1].kind else {
            panic!("expected DOCX table block");
        };
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].cells.len(), 2);
        assert_eq!(rows[1].cells.len(), 2);
        assert_eq!(block_plain_text(&rows[0].cells[0].blocks[0]), "A1");
        assert!(text_marks(&rows[0].cells[1].blocks[0])
            .iter()
            .any(|mark| mark.kind == MarkKind::Bold));
        assert!(rows[1].cells[0].blocks[0].content.iter().any(|inline| {
            matches!(
                inline,
                Inline::Equation { equation, .. } if equation.source == "x+2"
            )
        }));
        assert_eq!(block_plain_text(&rows[1].cells[1].blocks[0]), "");
        assert!(report.document.validate().is_ok());

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn imports_raw_docx_xml_table_only_source_without_text_runs() {
        let path = std::env::temp_dir().join(format!(
            "opendoc-import-docx-empty-table-only-{}.docx",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        std::fs::write(
            &path,
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    <w:tbl><w:tr><w:tc></w:tc></w:tr></w:tbl>
  </w:body>
</w:document>"#,
        )
        .unwrap();

        let report = import_doc_or_docx(&path).unwrap();
        assert_eq!(report.document.blocks.len(), 1);
        let BlockKind::Table { rows } = &report.document.blocks[0].kind else {
            panic!("expected raw DOCX XML table-only fixture to import as table block");
        };
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].cells.len(), 1);
        assert_eq!(rows[0].cells[0].blocks.len(), 1);
        assert_eq!(block_plain_text(&rows[0].cells[0].blocks[0]), "");
        assert_eq!(report.document.visible_text(), "\n");
        assert!(report.document.validate().is_ok());

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn imports_raw_docx_xml_drawing_only_as_missing_image_placeholder() {
        let path = std::env::temp_dir().join(format!(
            "opendoc-import-docx-raw-missing-image-{}.docx",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        std::fs::write(
            &path,
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
  xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"
  xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"
  xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"
  xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture">
  <w:body>
    <w:p><w:r><w:drawing><wp:inline><wp:docPr id="1" name="Raw Image" descr="Raw missing figure"/><a:graphic><a:graphicData><pic:pic><pic:blipFill><a:blip r:embed="rIdMissing"/></pic:blipFill></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>
  </w:body>
</w:document>"#,
        )
        .unwrap();

        let report = import_doc_or_docx(&path).unwrap();
        assert!(report.blobs.is_empty());
        assert_eq!(report.warnings.len(), 1);
        assert_eq!(report.warnings[0].code, "missing-docx-image-blob");
        assert_eq!(report.document.warnings, report.warnings);
        assert_eq!(report.document.blocks.len(), 1);
        assert_eq!(
            report.document.visible_text(),
            "[missing DOCX image: rIdMissing]\n"
        );
        assert!(matches!(
            report.document.blocks[0].kind,
            BlockKind::Paragraph
        ));
        assert!(report.document.validate().is_ok());

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn imports_docx_standalone_page_break_as_page_break_block() {
        let path = std::env::temp_dir().join(format!(
            "opendoc-import-docx-page-break-{}.docx",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        std::fs::write(
            &path,
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    <w:p><w:r><w:t>Before page</w:t></w:r></w:p>
    <w:p><w:r><w:br w:type="page"/></w:r></w:p>
    <w:p><w:r><w:t>Line</w:t><w:br/><w:t>break</w:t></w:r></w:p>
    <w:p><w:r><w:t>After page</w:t></w:r></w:p>
  </w:body>
</w:document>"#,
        )
        .unwrap();

        let report = import_doc_or_docx(&path).unwrap();
        assert_eq!(report.document.blocks.len(), 4);
        assert!(matches!(
            report.document.blocks[1].kind,
            BlockKind::PageBreak
        ));
        assert_eq!(
            report.document.visible_text(),
            "Before page\nLine\nbreak\nAfter page\n"
        );
        assert_eq!(block_plain_text(&report.document.blocks[2]), "Line\nbreak");
        assert!(report.document.validate().is_ok());

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn imports_google_docs_paragraph_heading_and_marks() {
        let input = json!({
            "body": {
                "content": [
                    {
                        "paragraph": {
                            "paragraphStyle": { "namedStyleType": "HEADING_2" },
                            "elements": [
                                { "textRun": {
                                    "content": "Heading\n",
                                    "textStyle": { "bold": true }
                                } }
                            ]
                        }
                    },
                    {
                        "paragraph": {
                            "elements": [
                                { "textRun": {
                                    "content": "Body",
                                    "textStyle": {
                                        "italic": true,
                                        "underline": true,
                                        "foregroundColor": {
                                            "color": { "rgbColor": { "red": 1.0, "green": 0.0, "blue": 0.0 } }
                                        }
                                    }
                                } }
                            ]
                        }
                    }
                ]
            }
        });
        let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();
        assert!(report.warnings.is_empty());
        assert_eq!(report.document.visible_text(), "Heading\nBody\n");
        assert!(matches!(
            report.document.blocks[0].kind,
            BlockKind::Heading { level: 2 }
        ));
        let marks = text_marks(&report.document.blocks[1]);
        assert!(marks.iter().any(|mark| mark.kind == MarkKind::Italic));
        assert!(marks.iter().any(|mark| mark.kind == MarkKind::Underline));
        assert!(marks
            .iter()
            .any(|mark| mark.kind == MarkKind::Color && mark.value.as_deref() == Some("#ff0000")));
    }

    #[test]
    fn google_docs_import_canonicalizes_wrapper_title() {
        let input = json!({
            "body": {
                "content": [
                    { "paragraph": { "elements": [{ "textRun": { "content": "Body" } }] } }
                ]
            }
        });
        let report =
            import_google_docs_json(" Imported Title ", input.to_string().as_bytes()).unwrap();
        assert_eq!(report.document.title, "Imported Title");
        assert!(matches!(
            import_google_docs_json(" ", input.to_string().as_bytes()),
            Err(ImportError::InvalidInput(message)) if message == "document title is empty"
        ));
    }

    #[test]
    fn imports_google_docs_links_as_link_inline() {
        let input = json!({
            "body": { "content": [{
                "paragraph": { "elements": [{
                    "textRun": {
                        "content": "OpenDoc",
                        "textStyle": { "link": { "url": "https://example.invalid/opendoc" } }
                    }
                }] }
            }] }
        });
        let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();
        match &report.document.blocks[0].content[0] {
            opendoc_core::Inline::Link { text, href, .. } => {
                assert_eq!(text, "OpenDoc");
                assert_eq!(href, "https://example.invalid/opendoc");
            }
            other => panic!("expected link inline, got {other:?}"),
        }
    }

    #[test]
    fn rejects_malformed_google_docs_links_without_plain_text_downgrade() {
        for (input, expected) in [
            (
                json!({
                    "body": { "content": [{
                        "paragraph": { "elements": [{
                            "textRun": {
                                "content": "OpenDoc",
                                "textStyle": "linked"
                            }
                        }] }
                    }] }
                }),
                "textStyle must be an object",
            ),
            (
                json!({
                    "body": { "content": [{
                        "paragraph": { "elements": [{
                            "textRun": {
                                "content": "OpenDoc",
                                "textStyle": { "link": [] }
                            }
                        }] }
                    }] }
                }),
                "link must be an object",
            ),
            (
                json!({
                    "body": { "content": [{
                        "paragraph": { "elements": [{
                            "textRun": {
                                "content": "OpenDoc",
                                "textStyle": { "link": { "url": 7 } }
                            }
                        }] }
                    }] }
                }),
                "url must be a string",
            ),
            (
                json!({
                    "body": { "content": [{
                        "paragraph": { "elements": [{
                            "textRun": {
                                "content": "OpenDoc",
                                "textStyle": { "link": { "url": " " } }
                            }
                        }] }
                    }] }
                }),
                "Google Docs link url is empty",
            ),
        ] {
            let err = import_google_docs_json("Google", input.to_string().as_bytes())
                .unwrap_err()
                .to_string();
            assert!(err.contains(expected), "{err}");
        }
    }

    #[test]
    fn imports_google_docs_list_items_with_level_and_ordering() {
        let input = json!({
            "body": { "content": [{
                "paragraph": {
                    "bullet": { "listId": "list-1", "nestingLevel": 2, "ordered": true },
                    "elements": [{ "textRun": { "content": "Item", "textStyle": {} } }]
                }
            }] }
        });
        let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();
        match &report.document.blocks[0].kind {
            BlockKind::ListItem {
                list_id,
                level,
                ordered,
            } => {
                assert_eq!(list_id.as_str(), "list-1");
                assert_eq!(*level, 2);
                assert!(*ordered);
            }
            other => panic!("expected list item, got {other:?}"),
        }
    }

    #[test]
    fn malformed_google_docs_paragraph_structure_metadata_aborts() {
        let base = json!({
            "body": { "content": [{
                "paragraph": {
                    "elements": [{ "textRun": { "content": "Item", "textStyle": {} } }]
                }
            }] }
        });

        let mut input = base.clone();
        input["body"]["content"][0]["paragraph"]["paragraphStyle"] = json!("bad");
        let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
        assert!(
            err.to_string().contains("paragraphStyle must be an object"),
            "{err}"
        );

        let mut input = base.clone();
        input["body"]["content"][0]["paragraph"]["paragraphStyle"] = json!({ "namedStyleType": 7 });
        let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
        assert!(
            err.to_string().contains("namedStyleType must be a string"),
            "{err}"
        );

        let mut input = base.clone();
        input["body"]["content"][0]["paragraph"]["bullet"] = json!("bad");
        let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
        assert!(
            err.to_string().contains("bullet must be an object"),
            "{err}"
        );

        let mut input = base.clone();
        input["body"]["content"][0]["paragraph"]["bullet"] = json!({ "listId": 7 });
        let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
        assert!(err.to_string().contains("listId must be a string"), "{err}");

        let mut input = base.clone();
        input["body"]["content"][0]["paragraph"]["bullet"] = json!({ "nestingLevel": "2" });
        let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
        assert!(
            err.to_string()
                .contains("nestingLevel must be a non-negative integer"),
            "{err}"
        );

        let mut input = base.clone();
        input["body"]["content"][0]["paragraph"]["bullet"] = json!({ "nestingLevel": 256 });
        let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
        assert!(
            err.to_string().contains("nestingLevel is too large"),
            "{err}"
        );

        let mut input = base.clone();
        input["body"]["content"][0]["paragraph"]["bullet"] = json!({ "nestingLevel": 9 });
        let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
        assert!(
            err.to_string().contains("nestingLevel is outside 0..=8"),
            "{err}"
        );

        let mut input = base;
        input["body"]["content"][0]["paragraph"]["bullet"] = json!({ "ordered": "true" });
        let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
        assert!(
            err.to_string().contains("ordered must be a boolean"),
            "{err}"
        );
    }

    #[test]
    fn imports_google_docs_table_cells_as_nested_blocks() {
        let input = json!({
            "body": { "content": [{
                "table": {
                    "tableRows": [{
                        "tableCells": [
                            { "content": [{ "paragraph": { "elements": [{ "textRun": { "content": "A1", "textStyle": {} } }] } }] },
                            { "content": [{ "paragraph": { "elements": [{ "textRun": { "content": "B1", "textStyle": {} } }] } }] }
                        ]
                    }]
                }
            }] }
        });
        let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();
        match &report.document.blocks[0].kind {
            BlockKind::Table { rows } => {
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0].cells.len(), 2);
                assert_eq!(report.document.visible_text(), "A1\tB1\n");
            }
            other => panic!("expected table, got {other:?}"),
        }
    }

    #[test]
    fn imports_empty_google_docs_table_shapes_as_editable_placeholders() {
        let input = json!({
            "body": { "content": [
                { "table": { "tableRows": [] } },
                { "table": { "tableRows": [{ "tableCells": [] }] } },
                { "table": { "tableRows": [{ "tableCells": [{ "content": [] }] }] } }
            ] }
        });
        let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();

        assert_eq!(report.document.blocks.len(), 3);
        for block in &report.document.blocks {
            let BlockKind::Table { rows } = &block.kind else {
                panic!("expected table block");
            };
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].cells.len(), 1);
            assert_eq!(rows[0].cells[0].blocks.len(), 1);
        }
        assert!(report.document.validate().is_ok());
        let warning_codes = report
            .warnings
            .iter()
            .map(|warning| warning.code.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            warning_codes,
            vec![
                "google-empty-table-normalized",
                "google-empty-table-row-normalized",
                "google-empty-table-cell-normalized"
            ]
        );
    }

    #[test]
    fn imports_google_docs_table_cells_with_opendoc_block_extensions() {
        let input = json!({
            "body": { "content": [{
                "table": {
                    "tableRows": [{
                        "tableCells": [{
                            "content": [
                                { "paragraph": { "elements": [
                                    { "textRun": { "content": "Cell text", "textStyle": {} } }
                                ] } },
                                { "opendocEquationBlock": {
                                    "blockId": "cell-equation-block",
                                    "equationId": "cell-equation",
                                    "sourceFormat": "latex-like",
                                    "source": "x+y"
                                } },
                                { "opendocImage": {
                                    "blockId": "cell-image",
                                    "blobHash": "sha256:cellimage",
                                    "altText": "Cell image"
                                } },
                                { "paragraph": { "elements": [
                                    { "pageBreak": {} },
                                    { "textRun": { "content": "\n" } }
                                ] } }
                            ]
                        }]
                    }]
                }
            }] }
        });
        let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();
        match &report.document.blocks[0].kind {
            BlockKind::Table { rows } => {
                let blocks = &rows[0].cells[0].blocks;
                assert!(matches!(blocks[0].kind, BlockKind::Paragraph));
                assert!(matches!(blocks[1].kind, BlockKind::EquationBlock { .. }));
                assert!(matches!(blocks[2].kind, BlockKind::Image { .. }));
                assert!(matches!(blocks[3].kind, BlockKind::PageBreak));
            }
            other => panic!("expected table, got {other:?}"),
        }
    }

    #[test]
    fn nested_google_docs_table_in_table_cell_aborts() {
        let input = json!({
            "body": { "content": [{
                "table": {
                    "tableRows": [{
                        "tableCells": [{
                            "content": [{
                                "table": { "tableRows": [] }
                            }]
                        }]
                    }]
                }
            }] }
        });
        assert!(matches!(
            import_google_docs_json("Google", input.to_string().as_bytes()),
            Err(ImportError::UnsupportedStructure(_))
        ));
    }

    #[test]
    fn exports_google_docs_table_cells_with_opendoc_block_extensions() {
        let mut document = Document::new("Nested Export");
        document.blocks.push(Block {
            id: StableId::new("block"),
            kind: BlockKind::Table {
                rows: vec![TableRow {
                    id: StableId::parse("row-export").unwrap(),
                    cells: vec![TableCell {
                        id: StableId::parse("cell-export").unwrap(),
                        blocks: vec![
                            Block::paragraph("Cell text"),
                            Block {
                                id: StableId::parse("cell-equation-block").unwrap(),
                                kind: BlockKind::EquationBlock {
                                    equation: Equation {
                                        id: StableId::parse("cell-equation").unwrap(),
                                        source_format: EquationSourceFormat::LatexLike,
                                        source: "x+y".to_string(),
                                    },
                                },
                                content: Vec::new(),
                                properties: Vec::new(),
                            },
                            Block {
                                id: StableId::parse("cell-image").unwrap(),
                                kind: BlockKind::Image {
                                    blob_hash: "sha256:cellimage".to_string(),
                                    alt_text: "Cell image".to_string(),
                                },
                                content: Vec::new(),
                                properties: Vec::new(),
                            },
                        ],
                        properties: Vec::new(),
                    }],
                }],
            },
            content: Vec::new(),
            properties: Vec::new(),
        });
        let bytes = export_google_docs_json(&document).unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        let content =
            &value["body"]["content"][0]["table"]["tableRows"][0]["tableCells"][0]["content"];
        assert_eq!(
            content[0]["paragraph"]["elements"][0]["textRun"]["content"],
            "Cell text"
        );
        assert_eq!(
            content[1]["opendocEquationBlock"]["blockId"],
            "cell-equation-block"
        );
        assert_eq!(content[2]["opendocImage"]["blockId"], "cell-image");
    }

    #[test]
    fn nested_table_cell_export_aborts_instead_of_dropping_or_misrepresenting() {
        let mut document = Document::new("Nested Table Export");
        document.blocks.push(Block {
            id: StableId::new("block"),
            kind: BlockKind::Table {
                rows: vec![TableRow {
                    id: StableId::parse("outer-row").unwrap(),
                    cells: vec![TableCell {
                        id: StableId::parse("outer-cell").unwrap(),
                        blocks: vec![Block {
                            id: StableId::parse("nested-table").unwrap(),
                            kind: BlockKind::Table {
                                rows: vec![TableRow {
                                    id: StableId::parse("inner-row").unwrap(),
                                    cells: vec![TableCell {
                                        id: StableId::parse("inner-cell").unwrap(),
                                        blocks: vec![Block::paragraph("Nested")],
                                        properties: Vec::new(),
                                    }],
                                }],
                            },
                            content: Vec::new(),
                            properties: Vec::new(),
                        }],
                        properties: Vec::new(),
                    }],
                }],
            },
            content: Vec::new(),
            properties: Vec::new(),
        });

        assert!(matches!(
            export_google_docs_json(&document),
            Err(ImportError::UnsupportedStructure(_))
        ));
    }

    #[test]
    fn malformed_empty_table_export_aborts_instead_of_emitting_invalid_google_shape() {
        let mut document = Document::new("Empty Table Export");
        document.blocks.push(Block {
            id: StableId::new("table"),
            kind: BlockKind::Table { rows: Vec::new() },
            content: Vec::new(),
            properties: Vec::new(),
        });
        assert!(matches!(
            export_google_docs_json(&document),
            Err(ImportError::UnsupportedStructure(message))
                if message == "OpenDoc table has no rows"
        ));

        document.blocks[0].kind = BlockKind::Table {
            rows: vec![TableRow {
                id: StableId::new("row"),
                cells: Vec::new(),
            }],
        };
        assert!(matches!(
            export_google_docs_json(&document),
            Err(ImportError::UnsupportedStructure(message))
                if message == "OpenDoc table row has no cells"
        ));

        document.blocks[0].kind = BlockKind::Table {
            rows: vec![TableRow {
                id: StableId::new("row"),
                cells: vec![TableCell {
                    id: StableId::new("cell"),
                    blocks: Vec::new(),
                    properties: Vec::new(),
                }],
            }],
        };
        assert!(matches!(
            export_google_docs_json(&document),
            Err(ImportError::UnsupportedStructure(message))
                if message == "OpenDoc table cell has no blocks"
        ));
    }

    #[test]
    fn malformed_structured_payload_export_aborts_instead_of_emitting_invalid_extensions() {
        let mut document = Document::new("Bad Structured Export");
        document.blocks.push(Block {
            id: StableId::new("link-block"),
            kind: BlockKind::Paragraph,
            content: vec![Inline::Link {
                id: StableId::new("link"),
                text: "link".to_string(),
                href: String::new(),
                marks: Vec::new(),
            }],
            properties: Vec::new(),
        });
        assert_export_error_contains(&document, "link href is empty");

        document.blocks[0] = Block {
            id: StableId::new("equation-block"),
            kind: BlockKind::EquationBlock {
                equation: Equation {
                    id: StableId::new("eq"),
                    source_format: EquationSourceFormat::LatexLike,
                    source: " ".to_string(),
                },
            },
            content: Vec::new(),
            properties: Vec::new(),
        };
        assert_export_error_contains(&document, "equation source");

        document.blocks[0] = Block {
            id: StableId::new("image"),
            kind: BlockKind::Image {
                blob_hash: "not-a-hash".to_string(),
                alt_text: "image".to_string(),
            },
            content: Vec::new(),
            properties: Vec::new(),
        };
        assert_export_error_contains(&document, "image blob");

        document.blocks.clear();
        document.comments.push(CommentThread {
            id: StableId::new("thread"),
            anchor: Anchor::Document,
            comments: vec![Comment {
                id: StableId::new("comment"),
                author: "Reviewer".to_string(),
                body: vec![Inline::Mention {
                    id: StableId::new("mention"),
                    label: " ".to_string(),
                }],
                created_at_ms: 1,
                deleted: false,
            }],
            deleted: false,
        });
        assert_export_error_contains(&document, "mention label is empty");

        document.comments.clear();
        document.suggestions.push(Suggestion {
            id: StableId::new("suggestion"),
            author: "Reviewer".to_string(),
            kind: SuggestionKind::Insert {
                anchor: Anchor::Document,
                content: vec![Inline::Equation {
                    id: StableId::new("equation"),
                    equation: Equation {
                        id: StableId::new("eq"),
                        source_format: EquationSourceFormat::LatexLike,
                        source: String::new(),
                    },
                }],
            },
            state: SuggestionState::Proposed,
            provenance: Vec::new(),
        });
        assert_export_error_contains(&document, "equation source is empty");

        document.suggestions.clear();
        document.blocks.push(Block {
            id: StableId::new("mark-block"),
            kind: BlockKind::Paragraph,
            content: vec![Inline::Text {
                id: StableId::new("text"),
                text: "marked".to_string(),
                marks: vec![mark(MarkKind::Color, None)],
            }],
            properties: Vec::new(),
        });
        assert_export_error_contains(&document, "mark value is missing");

        document.blocks[0].content = vec![Inline::Text {
            id: StableId::new("text"),
            text: "marked".to_string(),
            marks: vec![mark(MarkKind::Bold, Some("true".to_string()))],
        }];
        assert_export_error_contains(&document, "boolean mark has value");

        document.blocks[0].content = vec![Inline::Text {
            id: StableId::new("text"),
            text: "marked".to_string(),
            marks: vec![mark(MarkKind::Size, Some("large".to_string()))],
        }];
        assert_export_error_contains(&document, "size mark value is invalid");

        document.blocks[0].content.clear();
        document
            .citation_database
            .upsert_reference(BibliographyReference {
                id: StableId::new("bad-ref"),
                revision: 1,
                source: CitationSource {
                    format: CitationSourceFormat::CitumNative,
                    bytes: b"title: bad".to_vec(),
                },
                summary: CitationSummary {
                    title: "Bad".to_string(),
                    authors: vec![" ".to_string()],
                    issued: None,
                    doi: None,
                    url: None,
                },
                deleted: false,
            });
        assert_export_error_contains(&document, "bibliography summary field is empty");

        document.citation_database.references[0].summary.authors = vec!["Doe".to_string()];
        document.citation_database.upsert_citation(CitationGroup {
            id: StableId::new("bad-citation"),
            revision: 1,
            items: vec![CitationItem {
                reference_id: StableId::new("bad-ref"),
                locator: None,
                label: None,
                prefix: Some(" ".to_string()),
                suffix: None,
                suppress_author: false,
            }],
            placement: CitationPlacement::Inline,
            rendered_cache: None,
            deleted: false,
        });
        assert_export_error_contains(&document, "citation item field is empty");
    }

    #[test]
    fn google_docs_export_validates_source_before_serializing() {
        let mut document = Document::new("Export Source Guard");
        document.title = " Export Source Guard ".to_string();
        document.blocks.push(Block::paragraph("Body"));

        assert_export_error_contains(&document, "title has surrounding whitespace");
    }

    #[test]
    fn imports_google_docs_footnotes_as_document_local_source() {
        let input = json!({
            "body": { "content": [{
                "paragraph": { "elements": [
                    { "textRun": { "content": "Text", "textStyle": {} } },
                    { "footnoteReference": { "footnoteId": " fn-1 ", "footnoteNumber": "1" } }
                ] }
            }] },
            "footnotes": {
                "fn-1": {
                    "footnoteId": " fn-1 ",
                    "content": [{
                        "paragraph": { "elements": [{
                            "textRun": { "content": "Footnote body", "textStyle": { "italic": true } }
                        }] }
                    }]
                }
            }
        });
        let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();
        assert_eq!(report.document.footnotes.len(), 1);
        assert_eq!(report.document.footnotes[0].id.as_str(), "fn-1");
        match &report.document.blocks[0].content[1] {
            Inline::FootnoteRef { footnote_id, .. } => assert_eq!(footnote_id.as_str(), "fn-1"),
            other => panic!("expected footnote ref, got {other:?}"),
        }
        match &report.document.footnotes[0].body[0] {
            Inline::Text { text, marks, .. } => {
                assert_eq!(text, "Footnote body");
                assert!(marks.iter().any(|mark| mark.kind == MarkKind::Italic));
            }
            other => panic!("expected footnote text body, got {other:?}"),
        }
    }

    #[test]
    fn malformed_google_docs_footnote_source_aborts() {
        let base = json!({
            "body": { "content": [{
                "paragraph": { "elements": [
                    { "textRun": { "content": "Text", "textStyle": {} } },
                    { "footnoteReference": { "footnoteId": "fn-1", "footnoteNumber": "1" } }
                ] }
            }] },
            "footnotes": {
                "fn-1": {
                    "footnoteId": "fn-1",
                    "content": [{
                        "paragraph": { "elements": [{
                            "textRun": { "content": "Footnote body", "textStyle": {} }
                        }] }
                    }]
                }
            }
        });

        let mut input = base.clone();
        input["footnotes"] = json!("bad");
        let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
        assert!(
            err.to_string().contains("footnotes must be an object"),
            "{err}"
        );

        let mut input = base.clone();
        input["footnotes"]["fn-1"] = json!("bad");
        let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
        assert!(
            err.to_string().contains("footnote must be an object"),
            "{err}"
        );

        let mut input = base.clone();
        input["footnotes"]["fn-1"]["footnoteId"] = json!(7);
        let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
        assert!(
            err.to_string().contains("footnoteId must be a string"),
            "{err}"
        );

        let mut input = base.clone();
        input["footnotes"]["fn-1"]["content"] = json!("bad");
        let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
        assert!(
            err.to_string()
                .contains("footnote missing required array field content"),
            "{err}"
        );

        let mut input = base.clone();
        input["footnotes"]["fn-1"]["content"] = json!(["bad"]);
        let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
        assert!(
            err.to_string()
                .contains("footnote content element must be an object"),
            "{err}"
        );

        let mut input = base;
        input["footnotes"]["fn-1"]["content"] = json!([{ "table": {} }]);
        let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
        assert!(
            err.to_string()
                .contains("only paragraph footnote content is supported"),
            "{err}"
        );
    }

    #[test]
    fn imports_google_docs_equation_as_placeholder_with_warning() {
        let input = json!({
            "body": { "content": [{
                "paragraph": { "elements": [
                    { "textRun": { "content": "Equation: ", "textStyle": {} } },
                    { "equation": {} }
                ] }
            }] }
        });
        let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();
        assert!(report
            .warnings
            .iter()
            .any(|warning| warning.code == "google-equation-source-unavailable"));
        match &report.document.blocks[0].content[1] {
            Inline::Equation { equation, .. } => {
                assert_eq!(equation.source, "\\placeholder{}");
                assert_eq!(equation.source_format, EquationSourceFormat::LatexLike);
            }
            other => panic!("expected equation inline, got {other:?}"),
        }
    }

    #[test]
    fn imports_google_docs_citation_extension_as_document_local_source() {
        let input = json!({
            "body": { "content": [{
                "paragraph": { "elements": [
                    { "textRun": { "content": "Prior work ", "textStyle": {} } },
                    { "opendocCitation": {
                        "citationId": " cite-doe-2020 ",
                        "renderedCache": "(Doe 2020, p. 42)"
                    } }
                ] }
            }] },
            "footnotes": {
                "fn-cite": {
                    "footnoteId": " fn-cite ",
                    "content": [{
                        "paragraph": { "elements": [
                            { "textRun": { "content": "Citation footnote", "textStyle": {} } }
                        ] }
                    }]
                }
            },
            "opendocCitations": {
                "style": "apa-7th",
                "locale": "en-US",
                "references": [{
                    "id": " ref-doe-2020 ",
                    "revision": 3,
                    "format": "citum-native",
                    "bytesUtf8": "doe citation payload",
                    "summary": {
                        "title": "Example Article",
                        "authors": ["Doe", "Roe"],
                        "issued": "2020",
                        "doi": "10.1000/example",
                        "url": "https://example.invalid/article"
                    },
                    "deleted": false
                }],
                "groups": [
                    {
                        "id": " cite-doe-2020 ",
                        "revision": 4,
                        "items": [{
                            "referenceId": " ref-doe-2020 ",
                            "locator": "42",
                            "label": "page",
                            "prefix": "see",
                            "suffix": "for details",
                            "suppressAuthor": true
                        }],
                        "placement": "inline",
                        "renderedCache": "(Doe 2020, p. 42)",
                        "deleted": false
                    },
                    {
                        "id": " cite-footnote ",
                        "revision": 5,
                        "items": [{
                            "referenceId": " ref-doe-2020 ",
                            "locator": "9",
                            "label": "page",
                            "prefix": null,
                            "suffix": null,
                            "suppressAuthor": false
                        }],
                        "placement": "footnote",
                        "footnoteId": " fn-cite ",
                        "renderedCache": "(Doe 2020, p. 9)",
                        "deleted": false
                    }
                ]
            }
        });
        let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();
        assert!(report
            .warnings
            .iter()
            .any(|warning| warning.code == "opendoc-google-citations-extension"));
        match &report.document.blocks[0].content[1] {
            Inline::Citation {
                citation_id,
                rendered_cache,
                ..
            } => {
                assert_eq!(citation_id.as_str(), "cite-doe-2020");
                assert_eq!(rendered_cache.as_deref(), Some("(Doe 2020, p. 42)"));
            }
            other => panic!("expected citation inline, got {other:?}"),
        }
        let reference = &report.document.citation_database.references[0];
        assert_eq!(reference.id.as_str(), "ref-doe-2020");
        assert_eq!(reference.revision, 3);
        assert_eq!(reference.summary.title, "Example Article");
        assert_eq!(reference.summary.authors, vec!["Doe", "Roe"]);
        assert_eq!(reference.source.format, CitationSourceFormat::CitumNative);
        assert_eq!(reference.source.bytes, b"doe citation payload");
        let citation = &report.document.citation_database.citations[0];
        assert_eq!(citation.id.as_str(), "cite-doe-2020");
        assert_eq!(citation.items[0].reference_id.as_str(), "ref-doe-2020");
        assert_eq!(citation.items[0].locator.as_deref(), Some("42"));
        assert_eq!(citation.items[0].prefix.as_deref(), Some("see"));
        assert!(citation.items[0].suppress_author);
        let footnote_citation = report
            .document
            .citation_database
            .citations
            .iter()
            .find(|citation| citation.id.as_str() == "cite-footnote")
            .unwrap();
        assert_eq!(
            footnote_citation.placement,
            CitationPlacement::Footnote {
                footnote_id: StableId::parse("fn-cite").unwrap()
            }
        );
    }

    #[test]
    fn google_docs_citation_import_clears_stale_labels_for_missing_references() {
        let input = json!({
            "body": { "content": [{
                "paragraph": { "elements": [
                    { "opendocCitation": {
                        "citationId": "cite-missing-reference",
                        "renderedCache": "(Misleading 2020)"
                    } }
                ] }
            }] },
            "opendocCitations": {
                "style": "apa-7th",
                "locale": "en-US",
                "references": [],
                "groups": [{
                    "id": "cite-missing-reference",
                    "revision": 1,
                    "items": [{
                        "referenceId": "ref-missing",
                        "locator": "42",
                        "label": "page",
                        "suppressAuthor": false
                    }],
                    "placement": "inline",
                    "renderedCache": "(Misleading 2020)",
                    "deleted": false
                }]
            }
        });

        let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();

        assert_eq!(report.document.visible_text(), "[cite-missing-reference]\n");
        assert_eq!(
            report.document.citation_database.citations[0].rendered_cache,
            None
        );
        match &report.document.blocks[0].content[0] {
            Inline::Citation { rendered_cache, .. } => assert_eq!(rendered_cache, &None),
            other => panic!("expected citation inline, got {other:?}"),
        }
        assert!(report
            .warnings
            .iter()
            .any(|warning| warning.code == "citation-reference-missing"));
        assert!(report.document.validate().is_ok());
    }

    #[test]
    fn google_docs_citation_import_moves_missing_footnote_placement_inline() {
        let input = json!({
            "body": { "content": [{
                "paragraph": { "elements": [
                    { "opendocCitation": {
                        "citationId": "cite-missing-footnote",
                        "renderedCache": "(Misleading footnote)"
                    } }
                ] }
            }] },
            "opendocCitations": {
                "style": "apa-7th",
                "locale": "en-US",
                "references": [{
                    "id": "ref-footnote",
                    "revision": 1,
                    "format": "citum-native",
                    "bytesUtf8": "title: Footnote",
                    "summary": { "title": "Footnote" },
                    "deleted": false
                }],
                "groups": [{
                    "id": "cite-missing-footnote",
                    "revision": 1,
                    "items": [{
                        "referenceId": "ref-footnote",
                        "suppressAuthor": false
                    }],
                    "placement": "footnote",
                    "footnoteId": "fn-missing",
                    "renderedCache": "(Misleading footnote)",
                    "deleted": false
                }]
            }
        });

        let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();

        assert_eq!(report.document.visible_text(), "[cite-missing-footnote]\n");
        assert_eq!(
            report.document.citation_database.citations[0].placement,
            CitationPlacement::Inline
        );
        assert_eq!(
            report.document.citation_database.citations[0].rendered_cache,
            None
        );
        match &report.document.blocks[0].content[0] {
            Inline::Citation { rendered_cache, .. } => assert_eq!(rendered_cache, &None),
            other => panic!("expected citation inline, got {other:?}"),
        }
        assert!(report
            .warnings
            .iter()
            .any(|warning| warning.code == "citation-footnote-target-missing"));
        assert!(report.document.validate().is_ok());
    }

    #[test]
    fn google_docs_citation_import_clears_stale_labels_for_missing_groups() {
        let input = json!({
            "body": { "content": [{
                "paragraph": { "elements": [
                    { "opendocCitation": {
                        "citationId": "cite-missing-group",
                        "renderedCache": "(Stale Citation)"
                    } }
                ] }
            }] },
            "opendocCitations": {
                "style": "apa-7th",
                "locale": "en-US",
                "references": [],
                "groups": []
            }
        });

        let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();

        assert_eq!(report.document.visible_text(), "[cite-missing-group]\n");
        match &report.document.blocks[0].content[0] {
            Inline::Citation { rendered_cache, .. } => assert_eq!(rendered_cache, &None),
            other => panic!("expected citation inline, got {other:?}"),
        }
        assert!(report
            .warnings
            .iter()
            .any(|warning| warning.code == "citation-group-missing"));
        assert!(report.document.validate().is_ok());
    }

    #[test]
    fn google_docs_citation_import_clears_nested_labels_for_deleted_groups() {
        let input = json!({
            "body": { "content": [{
                "table": {
                    "tableRows": [{
                        "tableCells": [{
                            "content": [{
                                "paragraph": { "elements": [{
                                    "opendocCitation": {
                                        "citationId": "cite-deleted-group",
                                        "renderedCache": "(Stale Citation)"
                                    }
                                }] }
                            }]
                        }]
                    }]
                }
            }] },
            "opendocCitations": {
                "style": "apa-7th",
                "locale": "en-US",
                "references": [{
                    "id": "ref-deleted-group",
                    "revision": 1,
                    "format": "citum-native",
                    "bytesUtf8": "title: Deleted Group",
                    "summary": { "title": "Deleted Group" },
                    "deleted": false
                }],
                "groups": [{
                    "id": "cite-deleted-group",
                    "revision": 2,
                    "items": [{
                        "referenceId": "ref-deleted-group",
                        "suppressAuthor": false
                    }],
                    "placement": "inline",
                    "renderedCache": "(Stale Citation)",
                    "deleted": true
                }]
            }
        });

        let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();

        assert_eq!(report.document.visible_text(), "[cite-deleted-group]\n");
        match &report.document.blocks[0].kind {
            BlockKind::Table { rows } => match &rows[0].cells[0].blocks[0].content[0] {
                Inline::Citation { rendered_cache, .. } => assert_eq!(rendered_cache, &None),
                other => panic!("expected citation inline, got {other:?}"),
            },
            other => panic!("expected table, got {other:?}"),
        }
        assert!(report
            .warnings
            .iter()
            .any(|warning| warning.code == "citation-group-missing"));
        assert!(report.document.validate().is_ok());
    }

    #[test]
    fn malformed_citation_extension_import_aborts() {
        let mut input = json!({
            "body": { "content": [{
                "paragraph": { "elements": [
                    { "opendocCitation": { "citationId": "cite-bad" } }
                ] }
            }] },
            "opendocCitations": {
                "style": "apa-7th",
                "locale": "en-US",
                "references": [{
                    "id": "ref-bad",
                    "revision": 1,
                    "format": "citum-native",
                    "bytesUtf8": "title: Bad",
                    "summary": {
                        "title": "Bad",
                        "authors": [" "]
                    },
                    "deleted": false
                }],
                "groups": [{
                    "id": "cite-bad",
                    "revision": 1,
                    "items": [{
                        "referenceId": "ref-bad",
                        "locator": null,
                        "label": null,
                        "prefix": null,
                        "suffix": null,
                        "suppressAuthor": false
                    }],
                    "placement": "inline",
                    "deleted": false
                }]
            }
        });
        assert!(matches!(
            import_google_docs_json("Google", input.to_string().as_bytes()),
            Err(ImportError::InvalidDocument(message))
                if message == "bibliography summary field is empty"
        ));

        input["opendocCitations"]["references"][0]["summary"]["authors"] = json!(["Doe"]);
        input["opendocCitations"]["groups"][0]["items"][0]["prefix"] = json!(" ");
        assert!(matches!(
            import_google_docs_json("Google", input.to_string().as_bytes()),
            Err(ImportError::InvalidInput(message))
                if message == "citation item field is empty"
        ));

        input["opendocCitations"]["groups"][0]["items"][0]["prefix"] = json!(" see ");
        assert!(matches!(
            import_google_docs_json("Google", input.to_string().as_bytes()),
            Err(ImportError::InvalidInput(message))
                if message == "citation item field has surrounding whitespace"
        ));

        input["opendocCitations"]["groups"][0]["items"][0]["prefix"] = json!(null);
        input["opendocCitations"]["references"][0]["summary"]["doi"] = json!(" 10.123/example ");
        assert!(matches!(
            import_google_docs_json("Google", input.to_string().as_bytes()),
            Err(ImportError::InvalidInput(message))
                if message == "bibliography summary field has surrounding whitespace"
        ));

        let base = json!({
            "body": { "content": [{
                "paragraph": { "elements": [
                    { "opendocCitation": { "citationId": "cite-bad" } }
                ] }
            }] },
            "opendocCitations": {
                "style": "apa-7th",
                "locale": "en-US",
                "references": [{
                    "id": "ref-bad",
                    "revision": 1,
                    "format": "citum-native",
                    "bytesUtf8": "title: Bad",
                    "summary": {
                        "title": "Bad",
                        "authors": ["Doe"]
                    },
                    "deleted": false
                }],
                "groups": [{
                    "id": "cite-bad",
                    "revision": 1,
                    "items": [{
                        "referenceId": "ref-bad",
                        "locator": null,
                        "label": null,
                        "prefix": null,
                        "suffix": null,
                        "suppressAuthor": false
                    }],
                    "placement": "inline",
                    "deleted": false
                }]
            }
        });

        let mut input = base.clone();
        input["opendocCitations"] = json!("bad");
        let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
        assert!(
            err.to_string()
                .contains("opendocCitations must be an object"),
            "{err}"
        );

        let mut input = base.clone();
        input["opendocCitations"]["references"] = json!({});
        let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
        assert!(
            err.to_string().contains("references must be an array"),
            "{err}"
        );

        let mut input = base.clone();
        input["opendocCitations"]["groups"] = json!({});
        let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
        assert!(err.to_string().contains("groups must be an array"), "{err}");

        let mut input = base.clone();
        input["opendocCitations"]["references"] = json!(["bad"]);
        let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
        assert!(
            err.to_string()
                .contains("citation reference must be an object"),
            "{err}"
        );

        let mut input = base.clone();
        input["opendocCitations"]["groups"] = json!(["bad"]);
        let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
        assert!(
            err.to_string().contains("citation group must be an object"),
            "{err}"
        );

        let mut input = base.clone();
        input["opendocCitations"]["references"][0]["revision"] = json!("1");
        let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
        assert!(
            err.to_string()
                .contains("revision must be a non-negative integer"),
            "{err}"
        );

        let mut input = base.clone();
        input["opendocCitations"]["references"][0]["summary"] = json!("bad");
        let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
        assert!(
            err.to_string()
                .contains("citation summary must be an object"),
            "{err}"
        );

        let mut input = base.clone();
        input["opendocCitations"]["references"][0]["summary"]["authors"] = json!(["Doe", 7]);
        let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
        assert!(
            err.to_string()
                .contains("citation summary authors entries must be strings"),
            "{err}"
        );

        let mut input = base.clone();
        input["opendocCitations"]["references"][0]["deleted"] = json!("false");
        let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
        assert!(
            err.to_string().contains("deleted must be a boolean"),
            "{err}"
        );

        let mut input = base.clone();
        input["opendocCitations"]["groups"][0]["items"] = json!("bad");
        let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
        assert!(
            err.to_string()
                .contains("citation group missing required array field items"),
            "{err}"
        );

        let mut input = base.clone();
        input["opendocCitations"]["groups"][0]["items"] = json!(["bad"]);
        let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
        assert!(
            err.to_string().contains("citation item must be an object"),
            "{err}"
        );

        let mut input = base.clone();
        input["opendocCitations"]["groups"][0]["placement"] = json!(7);
        let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
        assert!(
            err.to_string().contains("placement must be a string"),
            "{err}"
        );

        let mut input = base.clone();
        input["opendocCitations"]["groups"][0]["items"][0]["suppressAuthor"] = json!("false");
        let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
        assert!(
            err.to_string().contains("suppressAuthor must be a boolean"),
            "{err}"
        );
    }

    #[test]
    fn citation_import_fills_missing_summary_from_citum_native_source() {
        let input = json!({
            "body": { "content": [{
                "paragraph": { "elements": [
                    { "textRun": { "content": "Prior work ", "textStyle": {} } },
                    { "opendocCitation": { "citationId": "cite-doe-2020" } }
                ] }
            }] },
            "opendocCitations": {
                "style": "apa-7th",
                "locale": "en-US",
                "references": [{
                    "id": "ref-doe-2020",
                    "revision": 3,
                    "format": "citum-native",
                    "bytesUtf8": "title: Example Article\nauthor: Doe; Roe\nyear: 2020\ndoi: 10.1000/example\nurl: https://example.invalid/article\n",
                    "summary": {
                        "title": "",
                        "authors": []
                    },
                    "deleted": false
                }],
                "groups": [{
                    "id": "cite-doe-2020",
                    "revision": 4,
                    "items": [{
                        "referenceId": "ref-doe-2020",
                        "locator": "42",
                        "label": "page",
                        "prefix": "see",
                        "suffix": null,
                        "suppressAuthor": false
                    }],
                    "placement": "inline",
                    "deleted": false
                }]
            }
        });

        let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();
        let reference = &report.document.citation_database.references[0];
        assert_eq!(reference.summary.title, "Example Article");
        assert_eq!(reference.summary.authors, vec!["Doe", "Roe"]);
        assert_eq!(reference.summary.issued.as_deref(), Some("2020"));
        assert_eq!(reference.summary.doi.as_deref(), Some("10.1000/example"));
        assert_eq!(
            reference.summary.url.as_deref(),
            Some("https://example.invalid/article")
        );
        assert!(report.document.validate().is_ok());
    }

    #[test]
    fn citation_import_unescapes_citum_native_source_fields() {
        let input = json!({
            "body": { "content": [{
                "paragraph": { "elements": [
                    { "textRun": { "content": "Escaped source ", "textStyle": {} } },
                    { "opendocCitation": { "citationId": "cite-escaped" } }
                ] }
            }] },
            "opendocCitations": {
                "style": "apa-7th",
                "locale": "en-US",
                "references": [{
                    "id": "ref-escaped",
                    "revision": 1,
                    "format": "citum-native",
                    "bytesUtf8": "title: Line one\\nLine two\\; source\nauthor: Curie\\; Lab; Roe\\\\Unit\nyear: 1911\nurl: https://example.invalid/a\\;b\n",
                    "summary": {
                        "title": "",
                        "authors": []
                    },
                    "deleted": false
                }],
                "groups": [{
                    "id": "cite-escaped",
                    "revision": 1,
                    "items": [{
                        "referenceId": "ref-escaped",
                        "suppressAuthor": false
                    }],
                    "placement": "inline",
                    "deleted": false
                }]
            }
        });

        let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();
        let reference = &report.document.citation_database.references[0];
        assert_eq!(reference.summary.title, "Line one\nLine two; source");
        assert_eq!(reference.summary.authors, vec!["Curie; Lab", "Roe\\Unit"]);
        assert_eq!(reference.summary.issued.as_deref(), Some("1911"));
        assert_eq!(
            reference.summary.url.as_deref(),
            Some("https://example.invalid/a;b")
        );
        assert_eq!(
            report.document.visible_text(),
            "Escaped source (Curie; Lab 1911)\n"
        );
        assert!(report.document.validate().is_ok());
    }

    #[test]
    fn footnote_citation_import_without_footnote_id_aborts() {
        let input = json!({
            "body": { "content": [{
                "paragraph": { "elements": [
                    { "textRun": { "content": "Prior work ", "textStyle": {} } }
                ] }
            }] },
            "opendocCitations": {
                "style": "apa-7th",
                "locale": "en-US",
                "references": [{
                    "id": "ref-doe-2020",
                    "revision": 1,
                    "format": "citum-native",
                    "bytesUtf8": "doe citation payload",
                    "summary": {
                        "title": "Example Article",
                        "authors": ["Doe"],
                        "issued": "2020"
                    },
                    "deleted": false
                }],
                "groups": [{
                    "id": "cite-footnote",
                    "revision": 1,
                    "items": [{
                        "referenceId": "ref-doe-2020",
                        "locator": "9",
                        "label": "page",
                        "prefix": null,
                        "suffix": null,
                        "suppressAuthor": false
                    }],
                    "placement": "footnote",
                    "renderedCache": "(Doe 2020, p. 9)",
                    "deleted": false
                }]
            }
        });

        let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
        assert!(
            err.to_string()
                .contains("footnote citation missing required string field footnoteId"),
            "{err}"
        );
    }

    #[test]
    fn imports_google_docs_comment_and_suggestion_extensions_as_source_state() {
        let input = json!({
            "body": { "content": [{
                "paragraph": { "elements": [
                    { "textRun": { "content": "Reviewed text", "textStyle": {} } }
                ] }
            }] },
            "opendocComments": [{
                "id": " thread-one ",
                "anchor": { "type": "textRange", "start": " text-start ", "end": " text-end " },
                "comments": [{
                    "id": " comment-one ",
                    "author": "Ada",
                    "body": [{ "textRun": { "content": "Needs citation", "textStyle": { "bold": true } } }],
                    "createdAtMs": 17,
                    "deleted": false
                }],
                "deleted": false
            }],
            "opendocSuggestions": [{
                "id": " suggest-one ",
                "author": "Grace",
                "kind": {
                    "type": "format",
                    "range": { "start": " text-start ", "end": " text-end " },
                    "textStyle": { "italic": true }
                },
                "state": "proposed",
                "provenance": ["imported fixture"]
            }]
        });
        let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();
        assert!(report
            .warnings
            .iter()
            .any(|warning| warning.code == "opendoc-google-comments-extension"));
        assert!(report
            .warnings
            .iter()
            .any(|warning| warning.code == "opendoc-google-suggestions-extension"));
        let thread = &report.document.comments[0];
        assert_eq!(thread.id.as_str(), "thread-one");
        assert!(matches!(thread.anchor, Anchor::TextRange(_)));
        assert_eq!(thread.comments[0].id.as_str(), "comment-one");
        assert_eq!(thread.comments[0].author, "Ada");
        assert_eq!(thread.comments[0].created_at_ms, 17);
        match &thread.comments[0].body[0] {
            Inline::Text { text, marks, .. } => {
                assert_eq!(text, "Needs citation");
                assert!(marks.iter().any(|mark| mark.kind == MarkKind::Bold));
            }
            other => panic!("expected comment text body, got {other:?}"),
        }
        let suggestion = &report.document.suggestions[0];
        assert_eq!(suggestion.id.as_str(), "suggest-one");
        assert_eq!(suggestion.author, "Grace");
        assert_eq!(suggestion.state, SuggestionState::Proposed);
        assert_eq!(suggestion.provenance, vec!["imported fixture"]);
        match &suggestion.kind {
            SuggestionKind::Format { range, marks } => {
                assert_eq!(range.start.as_str(), "text-start");
                assert_eq!(range.end.as_str(), "text-end");
                assert!(marks.iter().any(|mark| mark.kind == MarkKind::Italic));
            }
            other => panic!("expected format suggestion, got {other:?}"),
        }
    }

    #[test]
    fn malformed_review_extension_source_metadata_aborts() {
        let base_body = json!({
            "body": { "content": [{
                "paragraph": { "elements": [
                    { "textRun": { "content": "Reviewed text", "textStyle": {} } }
                ] }
            }] }
        });

        let mut input = base_body.clone();
        input["opendocComments"] = json!([{
            "id": "thread-one",
            "comments": [{
                "id": "comment-one",
                "author": " Ada ",
                "body": [{ "textRun": { "content": "Needs citation", "textStyle": {} } }]
            }]
        }]);
        assert!(matches!(
            import_google_docs_json("Google", input.to_string().as_bytes()),
            Err(ImportError::InvalidInput(message))
                if message == "comment author has surrounding whitespace"
        ));

        let mut input = base_body.clone();
        input["opendocSuggestions"] = json!([{
            "id": "suggest-one",
            "author": " Grace ",
            "kind": {
                "type": "delete",
                "range": { "start": "text-start", "end": "text-end" }
            }
        }]);
        assert!(matches!(
            import_google_docs_json("Google", input.to_string().as_bytes()),
            Err(ImportError::InvalidInput(message))
                if message == "suggestion author has surrounding whitespace"
        ));

        let mut input = base_body;
        input["opendocSuggestions"] = json!([{
            "id": "suggest-one",
            "author": "Grace",
            "kind": {
                "type": "delete",
                "range": { "start": "text-start", "end": "text-end" }
            },
            "provenance": [" imported "]
        }]);
        assert!(matches!(
            import_google_docs_json("Google", input.to_string().as_bytes()),
            Err(ImportError::InvalidInput(message))
                if message == "suggestion provenance entry has surrounding whitespace"
        ));
    }

    #[test]
    fn google_docs_extension_lists_must_be_arrays() {
        let input = json!({
            "body": { "content": [{
                "paragraph": { "elements": [
                    { "textRun": { "content": "Reviewed text", "textStyle": {} } }
                ] }
            }] },
            "opendocComments": {}
        });
        let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
        assert!(
            err.to_string().contains("opendocComments must be an array"),
            "{err}"
        );

        let input = json!({
            "body": { "content": [{
                "paragraph": { "elements": [
                    { "textRun": { "content": "Reviewed text", "textStyle": {} } }
                ] }
            }] },
            "opendocSuggestions": {}
        });
        let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
        assert!(
            err.to_string()
                .contains("opendocSuggestions must be an array"),
            "{err}"
        );
    }

    #[test]
    fn google_docs_comment_and_suggestion_extensions_reject_malformed_source_metadata() {
        let body = json!({
            "body": { "content": [{
                "paragraph": { "elements": [
                    { "textRun": { "content": "Reviewed text", "textStyle": {} } }
                ] }
            }] }
        });

        let mut input = body.clone();
        input["opendocComments"] = json!(["bad"]);
        let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
        assert!(
            err.to_string().contains("comment thread must be an object"),
            "{err}"
        );

        let mut input = body.clone();
        input["opendocComments"] = json!([{
            "id": "thread-one",
            "comments": ["bad"]
        }]);
        let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
        assert!(
            err.to_string().contains("comment must be an object"),
            "{err}"
        );

        let mut input = body.clone();
        input["opendocComments"] = json!([{
            "id": "thread-one",
            "comments": [{
                "id": "comment-one",
                "author": "Ada",
                "body": [{ "textRun": { "content": "Body", "textStyle": {} } }],
                "createdAtMs": "17"
            }]
        }]);
        let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
        assert!(
            err.to_string()
                .contains("createdAtMs must be a non-negative integer"),
            "{err}"
        );

        let mut input = body.clone();
        input["opendocComments"] = json!([{
            "id": "thread-one",
            "deleted": "false",
            "comments": [{
                "id": "comment-one",
                "author": "Ada",
                "body": [{ "textRun": { "content": "Body", "textStyle": {} } }]
            }]
        }]);
        let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
        assert!(
            err.to_string().contains("deleted must be a boolean"),
            "{err}"
        );

        let mut input = body.clone();
        input["opendocComments"] = json!([{
            "id": "thread-one",
            "comments": [{
                "id": "comment-one",
                "body": [{ "textRun": { "content": "Body", "textStyle": {} } }]
            }]
        }]);
        let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
        assert!(
            err.to_string()
                .contains("comment missing required string field author"),
            "{err}"
        );

        let mut input = body.clone();
        input["opendocComments"] = json!([{
            "id": "thread-one",
            "comments": [{
                "id": "comment-one",
                "author": 7,
                "body": [{ "textRun": { "content": "Body", "textStyle": {} } }]
            }]
        }]);
        let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
        assert!(
            err.to_string()
                .contains("comment missing required string field author"),
            "{err}"
        );

        let mut input = body.clone();
        input["opendocComments"] = json!([{
            "id": "thread-one",
            "anchor": "bad",
            "comments": [{
                "id": "comment-one",
                "author": "Ada",
                "body": [{ "textRun": { "content": "Body", "textStyle": {} } }]
            }]
        }]);
        let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
        assert!(
            err.to_string().contains("anchor must be an object"),
            "{err}"
        );

        let mut input = body.clone();
        input["opendocComments"] = json!([{
            "id": "thread-one",
            "anchor": { "type": 7 },
            "comments": [{
                "id": "comment-one",
                "author": "Ada",
                "body": [{ "textRun": { "content": "Body", "textStyle": {} } }]
            }]
        }]);
        let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
        assert!(err.to_string().contains("type must be a string"), "{err}");

        let mut input = body.clone();
        input["opendocComments"] = json!([{
            "id": "thread-one",
            "anchor": { "type": "cellRange" },
            "comments": [{
                "id": "comment-one",
                "author": "Ada",
                "body": [{ "textRun": { "content": "Body", "textStyle": {} } }]
            }]
        }]);
        let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
        assert!(
            err.to_string()
                .contains("unsupported anchor type cellRange"),
            "{err}"
        );

        let mut input = body.clone();
        input["opendocComments"] = json!([{
            "id": "thread-one",
            "anchor": { "type": "nearestBlock", "blockId": "block-one", "warning": 7 },
            "comments": [{
                "id": "comment-one",
                "author": "Ada",
                "body": [{ "textRun": { "content": "Body", "textStyle": {} } }]
            }]
        }]);
        let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
        assert!(
            err.to_string().contains("warning must be a string"),
            "{err}"
        );

        let mut input = body.clone();
        input["opendocComments"] = json!([{
            "id": "thread-one",
            "anchor": {
                "type": "nearestBlock",
                "blockId": "block-one",
                "warning": " imported degraded anchor "
            },
            "comments": [{
                "id": "comment-one",
                "author": "Ada",
                "body": [{ "textRun": { "content": "Body", "textStyle": {} } }]
            }]
        }]);
        let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
        assert!(
            err.to_string()
                .contains("nearest block anchor warning has surrounding whitespace"),
            "{err}"
        );

        let mut input = body.clone();
        input["opendocSuggestions"] = json!(["bad"]);
        let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
        assert!(
            err.to_string().contains("suggestion must be an object"),
            "{err}"
        );

        let mut input = body.clone();
        input["opendocSuggestions"] = json!([{
            "id": "suggest-one",
            "author": "Grace",
            "kind": { "type": "delete", "range": { "start": "text-start", "end": "text-end" } },
            "provenance": ["ok", 7]
        }]);
        let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
        assert!(
            err.to_string()
                .contains("suggestion provenance entries must be strings"),
            "{err}"
        );

        let mut input = body.clone();
        input["opendocSuggestions"] = json!([{
            "id": "suggest-one",
            "kind": { "type": "delete", "range": { "start": "text-start", "end": "text-end" } }
        }]);
        let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
        assert!(
            err.to_string()
                .contains("suggestion missing required string field author"),
            "{err}"
        );

        let mut input = body.clone();
        input["opendocSuggestions"] = json!([{
            "id": "suggest-one",
            "author": 7,
            "kind": { "type": "delete", "range": { "start": "text-start", "end": "text-end" } }
        }]);
        let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
        assert!(
            err.to_string()
                .contains("suggestion missing required string field author"),
            "{err}"
        );

        let mut input = body.clone();
        input["opendocSuggestions"] = json!([{
            "id": "suggest-one",
            "author": "Grace",
            "kind": "delete"
        }]);
        let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
        assert!(
            err.to_string()
                .contains("suggestion kind must be an object"),
            "{err}"
        );

        let mut input = body.clone();
        input["opendocSuggestions"] = json!([{
            "id": "suggest-one",
            "author": "Grace",
            "kind": {}
        }]);
        let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
        assert!(
            err.to_string()
                .contains("suggestion kind missing required string field type"),
            "{err}"
        );

        let mut input = body.clone();
        input["opendocSuggestions"] = json!([{
            "id": "suggest-one",
            "author": "Grace",
            "kind": { "type": 7 }
        }]);
        let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
        assert!(
            err.to_string()
                .contains("suggestion kind missing required string field type"),
            "{err}"
        );

        let mut input = body.clone();
        input["opendocSuggestions"] = json!([{
            "id": "suggest-one",
            "author": "Grace",
            "kind": { "type": "delete", "range": { "start": "text-start", "end": "text-end" } },
            "state": 7
        }]);
        let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
        assert!(err.to_string().contains("state must be a string"), "{err}");

        let mut input = body;
        input["opendocSuggestions"] = json!([{
            "id": "suggest-one",
            "author": "Grace",
            "kind": { "type": "delete", "range": { "start": "text-start", "end": "text-end" } },
            "state": "resolved"
        }]);
        let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
        assert!(
            err.to_string()
                .contains("unsupported suggestion state resolved"),
            "{err}"
        );
    }

    #[test]
    fn export_google_docs_represents_v0_subset() {
        let mut document = Document::new("Exported");
        document.blocks.push(Block {
            id: StableId::new("block"),
            kind: BlockKind::Heading { level: 1 },
            content: vec![opendoc_core::Inline::Text {
                id: StableId::new("text"),
                text: "Title".to_string(),
                marks: vec![mark(MarkKind::Bold, None)],
            }],
            properties: Vec::new(),
        });
        document.blocks.push(Block {
            id: StableId::new("block"),
            kind: BlockKind::ListItem {
                list_id: StableId::parse("list-export").unwrap(),
                level: 1,
                ordered: false,
            },
            content: vec![opendoc_core::Inline::Link {
                id: StableId::new("link"),
                text: "Link".to_string(),
                href: "https://example.invalid".to_string(),
                marks: Vec::new(),
            }],
            properties: Vec::new(),
        });
        document.blocks.push(Block {
            id: StableId::new("block"),
            kind: BlockKind::Paragraph,
            content: vec![
                Inline::Text {
                    id: StableId::new("text"),
                    text: "With footnote".to_string(),
                    marks: Vec::new(),
                },
                Inline::FootnoteRef {
                    id: StableId::new("footnote-ref"),
                    footnote_id: StableId::parse("fn-export").unwrap(),
                },
            ],
            properties: Vec::new(),
        });
        document.blocks.push(Block {
            id: StableId::parse("equation-block-export").unwrap(),
            kind: BlockKind::EquationBlock {
                equation: Equation {
                    id: StableId::parse("eq-block-export").unwrap(),
                    source_format: EquationSourceFormat::LatexLike,
                    source: "\\int_0^1 x^2 dx".to_string(),
                },
            },
            content: Vec::new(),
            properties: Vec::new(),
        });
        document.blocks.push(Block {
            id: StableId::new("block"),
            kind: BlockKind::Paragraph,
            content: vec![
                Inline::Text {
                    id: StableId::new("text"),
                    text: "Equation".to_string(),
                    marks: Vec::new(),
                },
                Inline::Equation {
                    id: StableId::parse("inline-eq-export").unwrap(),
                    equation: Equation {
                        id: StableId::parse("eq-inline-export").unwrap(),
                        source_format: EquationSourceFormat::LatexLike,
                        source: "E=mc^2".to_string(),
                    },
                },
            ],
            properties: Vec::new(),
        });
        document.blocks.push(Block {
            id: StableId::new("block"),
            kind: BlockKind::Paragraph,
            content: vec![
                Inline::Text {
                    id: StableId::new("text"),
                    text: "Mention ".to_string(),
                    marks: Vec::new(),
                },
                Inline::Mention {
                    id: StableId::parse("mention-export").unwrap(),
                    label: "@Ada".to_string(),
                },
            ],
            properties: Vec::new(),
        });
        document.blocks.push(Block {
            id: StableId::new("block"),
            kind: BlockKind::Paragraph,
            content: vec![
                Inline::Text {
                    id: StableId::new("text"),
                    text: "Cited ".to_string(),
                    marks: Vec::new(),
                },
                Inline::Citation {
                    id: StableId::new("citation-label"),
                    citation_id: StableId::parse("cite-export").unwrap(),
                    rendered_cache: Some("(Doe 2020)".to_string()),
                },
            ],
            properties: Vec::new(),
        });
        document.blocks.push(Block {
            id: StableId::parse("image-export").unwrap(),
            kind: BlockKind::Image {
                blob_hash: "sha256:abc123".to_string(),
                alt_text: "Exported figure".to_string(),
            },
            content: Vec::new(),
            properties: Vec::new(),
        });
        document.footnotes.push(Footnote {
            id: StableId::parse("fn-export").unwrap(),
            revision: 1,
            body: vec![Inline::Text {
                id: StableId::new("text"),
                text: "Exported footnote".to_string(),
                marks: Vec::new(),
            }],
            deleted: false,
        });
        document
            .citation_database
            .upsert_reference(BibliographyReference {
                id: StableId::parse("ref-export").unwrap(),
                revision: 2,
                source: CitationSource {
                    format: CitationSourceFormat::CitumNative,
                    bytes: b"exported citation payload".to_vec(),
                },
                summary: CitationSummary {
                    title: "Exported Article".to_string(),
                    authors: vec!["Doe".to_string()],
                    issued: Some("2020".to_string()),
                    doi: None,
                    url: None,
                },
                deleted: false,
            });
        document.citation_database.upsert_citation(CitationGroup {
            id: StableId::parse("cite-export").unwrap(),
            revision: 3,
            items: vec![CitationItem {
                reference_id: StableId::parse("ref-export").unwrap(),
                locator: Some("12".to_string()),
                label: Some("page".to_string()),
                prefix: None,
                suffix: None,
                suppress_author: false,
            }],
            placement: CitationPlacement::Inline,
            rendered_cache: Some("(Doe 2020)".to_string()),
            deleted: false,
        });
        document.citation_database.upsert_citation(CitationGroup {
            id: StableId::parse("cite-footnote-export").unwrap(),
            revision: 4,
            items: vec![CitationItem {
                reference_id: StableId::parse("ref-export").unwrap(),
                locator: Some("44".to_string()),
                label: Some("page".to_string()),
                prefix: None,
                suffix: None,
                suppress_author: false,
            }],
            placement: CitationPlacement::Footnote {
                footnote_id: StableId::parse("fn-export").unwrap(),
            },
            rendered_cache: Some("(Doe 2020, 44)".to_string()),
            deleted: false,
        });
        document.comments.push(CommentThread {
            id: StableId::parse("thread-export").unwrap(),
            anchor: Anchor::TextRange(TextRange {
                start: StableId::parse("text-start").unwrap(),
                end: StableId::parse("text-end").unwrap(),
            }),
            comments: vec![Comment {
                id: StableId::parse("comment-export").unwrap(),
                author: "Ada".to_string(),
                body: vec![Inline::Text {
                    id: StableId::new("text"),
                    text: "Exported comment".to_string(),
                    marks: vec![mark(MarkKind::Italic, None)],
                }],
                created_at_ms: 22,
                deleted: false,
            }],
            deleted: false,
        });
        document.suggestions.push(Suggestion {
            id: StableId::parse("suggest-export").unwrap(),
            author: "Grace".to_string(),
            kind: SuggestionKind::Delete {
                range: TextRange {
                    start: StableId::parse("text-start").unwrap(),
                    end: StableId::parse("text-end").unwrap(),
                },
            },
            state: SuggestionState::Accepted,
            provenance: vec!["accepted during review".to_string()],
        });
        let bytes = export_google_docs_json(&document).unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["title"], "Exported");
        assert_eq!(
            value["body"]["content"][0]["paragraph"]["paragraphStyle"]["namedStyleType"],
            "HEADING_1"
        );
        assert_eq!(
            value["body"]["content"][1]["paragraph"]["bullet"]["listId"],
            "list-export"
        );
        assert_eq!(
            value["body"]["content"][1]["paragraph"]["elements"][0]["textRun"]["textStyle"]["link"]
                ["url"],
            "https://example.invalid"
        );
        assert_eq!(
            value["body"]["content"][2]["paragraph"]["elements"][1]["footnoteReference"]
                ["footnoteId"],
            "fn-export"
        );
        assert_eq!(
            value["footnotes"]["fn-export"]["content"][0]["paragraph"]["elements"][0]["textRun"]
                ["content"],
            "Exported footnote"
        );
        assert_eq!(
            value["body"]["content"][3]["opendocEquationBlock"]["blockId"],
            "equation-block-export"
        );
        assert_eq!(
            value["body"]["content"][3]["opendocEquationBlock"]["equationId"],
            "eq-block-export"
        );
        assert_eq!(
            value["body"]["content"][3]["opendocEquationBlock"]["sourceFormat"],
            "latex-like"
        );
        assert_eq!(
            value["body"]["content"][3]["opendocEquationBlock"]["source"],
            "\\int_0^1 x^2 dx"
        );
        assert_eq!(
            value["body"]["content"][4]["paragraph"]["elements"][1]["opendocEquation"]["inlineId"],
            "inline-eq-export"
        );
        assert_eq!(
            value["body"]["content"][4]["paragraph"]["elements"][1]["opendocEquation"]
                ["equationId"],
            "eq-inline-export"
        );
        assert_eq!(
            value["body"]["content"][4]["paragraph"]["elements"][1]["opendocEquation"]
                ["sourceFormat"],
            "latex-like"
        );
        assert_eq!(
            value["body"]["content"][4]["paragraph"]["elements"][1]["opendocEquation"]["source"],
            "E=mc^2"
        );
        assert_eq!(
            value["body"]["content"][5]["paragraph"]["elements"][1]["opendocMention"]["inlineId"],
            "mention-export"
        );
        assert_eq!(
            value["body"]["content"][5]["paragraph"]["elements"][1]["opendocMention"]["label"],
            "@Ada"
        );
        assert_eq!(
            value["body"]["content"][6]["paragraph"]["elements"][1]["opendocCitation"]
                ["citationId"],
            "cite-export"
        );
        assert_eq!(
            value["body"]["content"][7]["opendocImage"]["blockId"],
            "image-export"
        );
        assert_eq!(
            value["body"]["content"][7]["opendocImage"]["blobHash"],
            "sha256:abc123"
        );
        assert_eq!(
            value["body"]["content"][7]["opendocImage"]["altText"],
            "Exported figure"
        );
        assert_eq!(
            value["opendocCitations"]["references"][0]["id"],
            "ref-export"
        );
        assert_eq!(
            value["opendocCitations"]["references"][0]["summary"]["title"],
            "Exported Article"
        );
        assert_eq!(
            value["opendocCitations"]["groups"][0]["items"][0]["referenceId"],
            "ref-export"
        );
        assert_eq!(
            value["opendocCitations"]["groups"][0]["items"][0]["suppressAuthor"],
            false
        );
        let groups = value["opendocCitations"]["groups"].as_array().unwrap();
        let footnote_group = groups
            .iter()
            .find(|group| group["id"] == "cite-footnote-export")
            .unwrap();
        assert_eq!(footnote_group["placement"], "footnote");
        assert_eq!(footnote_group["footnoteId"], "fn-export");
        assert_eq!(value["opendocComments"][0]["id"], "thread-export");
        assert_eq!(
            value["opendocComments"][0]["comments"][0]["body"][0]["textRun"]["content"],
            "Exported comment"
        );
        assert_eq!(value["opendocSuggestions"][0]["id"], "suggest-export");
        assert_eq!(value["opendocSuggestions"][0]["state"], "accepted");
        assert_eq!(value["opendocSuggestions"][0]["kind"]["type"], "delete");
    }

    #[test]
    fn imports_google_docs_opendoc_inline_equation_extension() {
        let input = json!({
            "body": { "content": [{
                "paragraph": { "elements": [
                    { "textRun": { "content": "Equation " } },
                    { "opendocEquation": {
                        "inlineId": " inline-eq-import ",
                        "equationId": " eq-import ",
                        "sourceFormat": "latex-like",
                        "source": "a^2 + b^2 = c^2"
                    } }
                ] }
            }] }
        });
        let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();
        assert!(report
            .warnings
            .iter()
            .any(|warning| warning.code == "opendoc-google-equation-extension"));
        let inline = &report.document.blocks[0].content[1];
        match inline {
            Inline::Equation { id, equation } => {
                assert_eq!(id.as_str(), "inline-eq-import");
                assert_eq!(equation.id.as_str(), "eq-import");
                assert_eq!(equation.source_format, EquationSourceFormat::LatexLike);
                assert_eq!(equation.source, "a^2 + b^2 = c^2");
            }
            other => panic!("expected inline equation, got {other:?}"),
        }
    }

    #[test]
    fn opendoc_inline_equation_extension_with_empty_source_aborts_import() {
        let input = json!({
            "body": { "content": [{
                "paragraph": { "elements": [
                    { "opendocEquation": {
                        "inlineId": "inline-eq-empty",
                        "equationId": "eq-empty",
                        "sourceFormat": "latex-like",
                        "source": " "
                    } }
                ] }
            }] }
        });
        assert!(matches!(
            import_google_docs_json("Google", input.to_string().as_bytes()),
            Err(ImportError::UnsupportedStructure(message))
                if message == "OpenDoc inline equation missing source"
        ));
    }

    #[test]
    fn imports_google_docs_opendoc_mention_extension() {
        let input = json!({
            "body": { "content": [{
                "paragraph": { "elements": [
                    { "textRun": { "content": "Reviewed by " } },
                    { "opendocMention": {
                        "inlineId": " mention-import ",
                        "label": "@Grace"
                    } }
                ] }
            }] }
        });
        let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();
        assert!(report
            .warnings
            .iter()
            .any(|warning| warning.code == "opendoc-google-mention-extension"));
        let inline = &report.document.blocks[0].content[1];
        match inline {
            Inline::Mention { id, label } => {
                assert_eq!(id.as_str(), "mention-import");
                assert_eq!(label, "@Grace");
            }
            other => panic!("expected mention, got {other:?}"),
        }
    }

    #[test]
    fn imports_google_docs_page_break_paragraph_as_block() {
        let input = json!({
            "body": { "content": [{
                "paragraph": { "elements": [
                    { "pageBreak": {} },
                    { "textRun": { "content": "\n" } }
                ] }
            }] }
        });
        let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();
        assert_eq!(report.document.blocks.len(), 1);
        assert!(matches!(
            report.document.blocks[0].kind,
            BlockKind::PageBreak
        ));
    }

    #[test]
    fn mixed_google_docs_page_break_paragraph_aborts() {
        let input = json!({
            "body": { "content": [{
                "paragraph": { "elements": [
                    { "textRun": { "content": "Before" } },
                    { "pageBreak": {} }
                ] }
            }] }
        });
        assert!(matches!(
            import_google_docs_json("Google", input.to_string().as_bytes()),
            Err(ImportError::UnsupportedStructure(_))
        ));
    }

    #[test]
    fn exports_page_break_as_google_docs_page_break_paragraph() {
        let mut document = Document::new("Page Break Export");
        document.blocks.push(Block {
            id: StableId::new("block"),
            kind: BlockKind::PageBreak,
            content: Vec::new(),
            properties: Vec::new(),
        });
        let bytes = export_google_docs_json(&document).unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            value["body"]["content"][0]["paragraph"]["elements"][0]["pageBreak"],
            json!({})
        );
    }

    #[test]
    fn imports_google_docs_opendoc_equation_block_extension() {
        let input = json!({
            "body": { "content": [{
                "opendocEquationBlock": {
                    "blockId": " equation-block-import ",
                    "equationId": " eq-block-import ",
                    "sourceFormat": "latex-like",
                    "source": "\\sum_i x_i"
                }
            }] }
        });
        let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();
        assert_eq!(report.document.blocks.len(), 1);
        let block = &report.document.blocks[0];
        assert_eq!(block.id.as_str(), "equation-block-import");
        match &block.kind {
            BlockKind::EquationBlock { equation } => {
                assert_eq!(equation.id.as_str(), "eq-block-import");
                assert_eq!(equation.source_format, EquationSourceFormat::LatexLike);
                assert_eq!(equation.source, "\\sum_i x_i");
            }
            other => panic!("expected equation block, got {other:?}"),
        }
    }

    #[test]
    fn opendoc_equation_block_extension_with_empty_source_aborts_import() {
        let input = json!({
            "body": { "content": [{
                "opendocEquationBlock": {
                    "blockId": "block-eq-empty",
                    "equationId": "eq-empty",
                    "sourceFormat": "latex-like",
                    "source": ""
                }
            }] }
        });
        assert!(matches!(
            import_google_docs_json("Google", input.to_string().as_bytes()),
            Err(ImportError::UnsupportedStructure(message))
                if message == "OpenDoc block equation missing source"
        ));
    }

    #[test]
    fn imports_google_docs_opendoc_image_extension() {
        let input = json!({
            "body": { "content": [{
                "opendocImage": {
                    "blockId": " image-import ",
                    "blobHash": " sha256:imported ",
                    "altText": "Imported figure"
                }
            }] }
        });
        let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();
        assert_eq!(report.document.blocks.len(), 1);
        let block = &report.document.blocks[0];
        assert_eq!(block.id.as_str(), "image-import");
        match &block.kind {
            BlockKind::Image {
                blob_hash,
                alt_text,
            } => {
                assert_eq!(blob_hash, "sha256:imported");
                assert_eq!(alt_text, "Imported figure");
            }
            other => panic!("expected image block, got {other:?}"),
        }
    }

    #[test]
    fn opendoc_image_extension_with_invalid_blob_hash_aborts_import() {
        let input = json!({
            "body": { "content": [{
                "opendocImage": {
                    "blockId": "image-import",
                    "blobHash": "not-a-hash",
                    "altText": "Imported figure"
                }
            }] }
        });
        assert!(matches!(
            import_google_docs_json("Google", input.to_string().as_bytes()),
            Err(ImportError::UnsupportedStructure(message))
                if message.contains("OpenDoc image blobHash is invalid")
        ));
    }

    #[test]
    fn unsupported_high_risk_google_docs_element_aborts() {
        let input = json!({
            "body": { "content": [{
                "paragraph": { "elements": [{ "inlineObjectElement": { "inlineObjectId": "img1" } }] }
            }] }
        });
        assert!(matches!(
            import_google_docs_json("Google", input.to_string().as_bytes()),
            Err(ImportError::UnsupportedStructure(_))
        ));
    }

    #[test]
    fn partial_fidelity_emits_warning() {
        let input = json!({
            "body": { "content": [{
                "paragraph": {
                    "paragraphStyle": { "unsupportedStyle": true },
                    "elements": [{ "textRun": {
                        "content": "Warn",
                        "textStyle": { "smallCaps": true }
                    } }]
                }
            }] }
        });
        let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();
        assert_eq!(report.document.visible_text(), "Warn\n");
        assert!(report
            .warnings
            .iter()
            .any(|warning| warning.code == "unsupported-google-paragraph-style"));
        assert!(report
            .warnings
            .iter()
            .any(|warning| warning.code == "unsupported-google-text-style"));
    }

    fn text_marks(block: &Block) -> &[Mark] {
        match &block.content[0] {
            opendoc_core::Inline::Text { marks, .. } => marks,
            other => panic!("expected text inline, got {other:?}"),
        }
    }

    fn block_plain_text(block: &Block) -> String {
        block
            .content
            .iter()
            .filter_map(|inline| match inline {
                Inline::Text { text, .. } | Inline::Link { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }
}
