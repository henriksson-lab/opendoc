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
mod odt;
mod odt_package;
mod odt_write;
mod opendoc_json;
mod xml;
mod xml_write;

pub use error::ImportError;

/// The bytes behind a [`opendoc_core::BlockKind::Image`] block, supplied by
/// the caller because the model stores only the content hash and a package
/// has to embed the bytes.
///
/// Shared by every package writer: DOCX and ODT face the same problem and
/// there is no reason for two shapes of the same answer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExportImage {
    pub media_type: String,
    pub bytes: Vec<u8>,
}

/// The name [`ExportImage`] was introduced under, when only the DOCX writer
/// needed it.
pub type DocxImage = ExportImage;

pub(crate) use json::{optional_array, optional_bool, optional_object, optional_str};

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
mod odt_import_tests;
#[cfg(test)]
mod odt_write_tests;
#[cfg(test)]
mod plaintext_tests;

use crate::google_citations::{
    export_google_citations, import_google_citations, refresh_imported_citation_projection_caches,
    repair_imported_citation_placements, repair_imported_citation_references,
    repair_imported_inline_citation_labels, should_export_citations,
};
use crate::google_export::{
    can_export_page_break_before, export_google_block, export_google_comment_history,
    export_google_comments, export_google_footnotes, export_google_suggestions,
    set_google_page_break_before,
};
use crate::google_import::{
    import_google_bookmarks, import_google_comment_history, import_google_comments,
    import_google_footnotes, import_google_structural_element, import_google_suggestions,
    import_native_google_bookmarks, GoogleBlockRange,
};
use crate::google_style::{export_lists, warning, GoogleLists};
use crate::legacy_doc::{convert_legacy_doc_to_plaintext, validate_legacy_doc_container};
use opendoc_core::{
    Block, BlockKind, Document, HeaderFooterSlot, ImageCrop, Length, Mark, MarkExpand, MarkKind,
    ModelWarning, StableId,
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

/// The IANA media type of a `.docx` package.
pub const DOCX_MEDIA_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document";

pub use odt_package::ODT_MEDIA_TYPE;

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

/// Import an OpenDocument Text package (or a raw `content.xml` payload).
///
/// The reader deliberately starts with the portable body-text subset.  In
/// particular, it projects only ODF bookmark ranges which exactly wrap one
/// imported paragraph or heading onto OpenDoc's stable-block bookmarks; it
/// never guesses a character-range target.
pub fn import_odt_bytes(title: &str, bytes: &[u8]) -> Result<ImportReport, ImportError> {
    let title = if title.trim().is_empty() {
        "Imported OpenDocument Text"
    } else {
        title
    };
    let imported = odt::import_odt_bytes(title, bytes)?;
    Ok(ImportReport {
        document: imported.document,
        warnings: imported.warnings,
        blobs: Vec::new(),
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
    images: &BTreeMap<String, ExportImage>,
) -> Result<Vec<u8>, ImportError> {
    export_docx_with_warnings(document, images).map(|(bytes, _)| bytes)
}

/// Exports to DOCX, returning the package bytes and everything
/// WordprocessingML could not represent exactly.
pub fn export_docx_with_warnings(
    document: &Document,
    images: &BTreeMap<String, ExportImage>,
) -> Result<(Vec<u8>, Vec<ModelWarning>), ImportError> {
    let export = docx_write::export_docx_bytes(document, images)?;
    Ok((export.bytes, export.warnings))
}

/// Exports the document as an `.odt` (OpenDocument Text) package.
///
/// `images` supplies the bytes behind every
/// [`opendoc_core::BlockKind::Image`] block, keyed by content hash, exactly
/// as [`export_docx`] needs them.
///
/// [`export_odt_with_warnings`] is the same call with everything ODF cannot
/// carry exactly reported alongside the bytes.
pub fn export_odt(
    document: &Document,
    images: &BTreeMap<String, ExportImage>,
) -> Result<Vec<u8>, ImportError> {
    export_odt_with_warnings(document, images).map(|(bytes, _)| bytes)
}

/// Exports to ODT, returning the package bytes and everything OpenDocument
/// could not represent exactly.
pub fn export_odt_with_warnings(
    document: &Document,
    images: &BTreeMap<String, ExportImage>,
) -> Result<(Vec<u8>, Vec<ModelWarning>), ImportError> {
    let export = odt_write::export_odt_bytes(document, images)?;
    Ok((export.bytes, export.warnings))
}

pub fn import_google_docs_json(
    title: impl Into<String>,
    bytes: &[u8],
) -> Result<ImportReport, ImportError> {
    import_google_docs_json_with_image_resources(title, bytes, &BTreeMap::new())
}

/// Imports Google Docs API JSON with the image bytes the API deliberately
/// leaves out of its JSON response. Keys are Google `inlineObjectId` values;
/// callers obtain the bytes through an already-authorised transport and this
/// crate never follows a `contentUri` itself.
pub fn import_google_docs_json_with_image_resources(
    title: impl Into<String>,
    bytes: &[u8],
    image_resources: &BTreeMap<String, ExportImage>,
) -> Result<ImportReport, ImportError> {
    let mut value: Value =
        serde_json::from_slice(bytes).map_err(|err| ImportError::InvalidInput(err.to_string()))?;
    let mut document = Document::new(import_document_title(title.into())?);
    let mut warnings = Vec::new();
    normalize_google_tabbed_document(&mut value, &mut warnings)?;
    let materialized =
        materialize_google_inline_images(&mut value, image_resources, &mut warnings)?;
    let content = value
        .pointer("/body/content")
        .and_then(Value::as_array)
        .ok_or_else(|| ImportError::InvalidInput("missing body.content array".to_string()))?;
    let lists = GoogleLists::parse(&value, &mut warnings)?;
    import_google_page_setup(&value, &mut document, &mut warnings)?;
    import_google_document_furniture(&value, &lists, &mut document, &mut warnings)?;
    report_dropped_google_document_parts(&value, &mut warnings);
    let mut google_block_ranges = Vec::new();
    for element in content {
        let start = crate::json::optional_u64(element, "startIndex")?;
        let end = crate::json::optional_u64(element, "endIndex")?;
        // A Google body *always* starts with a SectionBreak.  It describes
        // the first section; it does not put a physical break before that
        // section.  Similarly, a continuous interior section has a real
        // source boundary but must not be projected to our only visible
        // boundary, PageBreak.  The full section style remains intentionally
        // unrepresentable until the document model gains section ownership.
        let initial_section_break =
            document.blocks.is_empty() && element.get("sectionBreak").is_some();
        let section_type = element
            .pointer("/sectionBreak/sectionStyle/sectionType")
            .and_then(|value| value.as_str());
        let continuous_section_break = section_type == Some("CONTINUOUS");
        // A section boundary with a newly introduced (or malformed) policy
        // must not be guessed to be a physical page break.  `NEXT_PAGE` is
        // the one known policy that has the same visible meaning as our
        // structural PageBreak; `CONTINUOUS` deliberately has none.  The
        // absent field is Google's ordinary default and retains the historic
        // NEXT_PAGE projection.
        let unknown_section_type = element.get("sectionBreak").is_some()
            && element
                .pointer("/sectionBreak/sectionStyle/sectionType")
                .is_some_and(|value| {
                    !matches!(value.as_str(), Some("NEXT_PAGE") | Some("CONTINUOUS"))
                });
        let mut imported = import_google_structural_element(element, &lists, &mut warnings, 0)?;
        if initial_section_break || continuous_section_break || unknown_section_type {
            imported.retain(|block| !matches!(block.kind, BlockKind::PageBreak));
            if continuous_section_break && !initial_section_break {
                warnings.push(google_style::warning(
                    google_style::DROPPED_DOCUMENT_PART,
                    "a continuous Google Docs section boundary has no visible PageBreak projection and was dropped; its section styling is not representable",
                ));
            }
            if unknown_section_type {
                let source_type = section_type.unwrap_or("non-string sectionType");
                warnings.push(google_style::warning(
                    google_style::DROPPED_DOCUMENT_PART,
                    &format!(
                        "a Google Docs section boundary with unknown sectionType {source_type:?} was dropped rather than guessed as an OpenDoc PageBreak"
                    ),
                ));
            }
        }
        if let (Some(start), Some(end), Some(block)) = (start, end, imported.first()) {
            if start < end {
                google_block_ranges.push(GoogleBlockRange {
                    start,
                    end,
                    block_id: block.id.clone(),
                });
            }
        }
        document.blocks.extend(imported);
    }
    // Google owns start numbers in the list definition rather than on a
    // paragraph.  Install only definitions used by a real imported item;
    // otherwise arbitrary unused JSON would become signed source state.
    let mut imported_list_ids = BTreeSet::new();
    collect_imported_list_ids(&document.blocks, &mut imported_list_ids);
    collect_imported_list_ids(&document.header, &mut imported_list_ids);
    collect_imported_list_ids(&document.footer, &mut imported_list_ids);
    for list_id in imported_list_ids {
        if let Some(properties) = lists.properties_for(list_id.as_str()) {
            document
                .list_properties
                .entry(list_id)
                .or_insert_with(|| properties.clone());
        }
    }
    document.footnotes = import_google_footnotes(&value, &lists, &mut warnings)?;
    let mut bookmarks = import_google_bookmarks(&value, &mut warnings)?;
    let bookmark_names = bookmarks
        .iter()
        .filter(|bookmark| !bookmark.deleted)
        .map(|bookmark| bookmark.name.clone())
        .collect();
    let bookmark_ids = bookmarks
        .iter()
        .map(|bookmark| bookmark.id.clone())
        .collect();
    bookmarks.extend(import_native_google_bookmarks(
        &value,
        &google_block_ranges,
        &materialized.inline_image_split_ranges,
        &bookmark_names,
        &bookmark_ids,
        &mut warnings,
    )?);
    document.bookmarks = bookmarks;
    document.citation_database = import_google_citations(&value, &mut warnings)?;
    document.comments = import_google_comments(&value, &mut warnings)?;
    document.comment_history = import_google_comment_history(&value, &mut warnings)?;
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
        blobs: materialized.blobs,
    })
}

fn collect_imported_list_ids(blocks: &[Block], output: &mut BTreeSet<StableId>) {
    for block in blocks {
        match &block.kind {
            BlockKind::ListItem { list_id, .. } => {
                output.insert(list_id.clone());
            }
            BlockKind::Table { rows, .. } => {
                for row in rows {
                    for cell in &row.cells {
                        collect_imported_list_ids(&cell.blocks, output);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Google keeps document-wide page geometry in `documentStyle`.  This is one
/// of the few document-style fields whose meaning is exactly the same as the
/// mandatory OpenDoc [`PageSetup`]: a finite sheet plus body and furniture
/// margins.  Import it before reporting the remaining document-style fields
/// so a normal Google page setup no longer disappears behind a broad warning.
///
/// A partially specified page size is deliberately not guessed.  Google uses
/// field masks, and importing one source dimension together with an unrelated
/// default dimension would claim a sheet the source did not name.
fn import_google_page_setup(
    value: &Value,
    document: &mut Document,
    warnings: &mut Vec<ModelWarning>,
) -> Result<(), ImportError> {
    let Some(style) = optional_object(value, "documentStyle")? else {
        return Ok(());
    };

    let mut setup = document.page_setup;
    if let Some(size) = optional_object(style, "pageSize")? {
        let width = google_style::import_dimension(size, "width", warnings)?;
        let height = google_style::import_dimension(size, "height", warnings)?;
        match (width, height) {
            (Some(width), Some(height)) => match setup.with_size(width, height) {
                Ok(next) => setup = next,
                Err(_) => warnings.push(google_style::warning(
                    google_style::DROPPED_DOCUMENT_PART,
                    "Google Docs documentStyle.pageSize leaves no valid OpenDoc page and was dropped",
                )),
            },
            (None, None) => {}
            _ => warnings.push(google_style::warning(
                google_style::DROPPED_DOCUMENT_PART,
                "Google Docs documentStyle.pageSize named only one dimension and was dropped",
            )),
        }
    }

    // `flipPageOrientation` does not name a second kind of page: Google
    // defines it as swapping `pageSize`'s two dimensions. Keep that
    // *effective* finite sheet in PageSetup instead of retaining a transport
    // flag whose only meaning is already represented by width and height.
    // This is deliberately done after pageSize so a portrait source size plus
    // the flag becomes the same landscape sheet seen by Google users. It also
    // correctly applies to Google's implicit default pageSize when a response
    // supplies the flag without an explicit size.
    if crate::json::optional_bool(style, "flipPageOrientation")?.unwrap_or(false) {
        // Swapping positive, individually valid dimensions cannot invalidate
        // the content rectangle, but use the model constructor so this bridge
        // continues to share PageSetup's validation contract.
        match setup.with_size(setup.height, setup.width) {
            Ok(next) => setup = next,
            Err(_) => warnings.push(google_style::warning(
                google_style::DROPPED_DOCUMENT_PART,
                "Google Docs documentStyle.flipPageOrientation leaves no valid OpenDoc page and was dropped",
            )),
        }
    }

    let top = google_style::import_dimension(style, "marginTop", warnings)?;
    let bottom = google_style::import_dimension(style, "marginBottom", warnings)?;
    let start = google_style::import_dimension(style, "marginLeft", warnings)?;
    let end = google_style::import_dimension(style, "marginRight", warnings)?;
    if top.is_some() || bottom.is_some() || start.is_some() || end.is_some() {
        match setup.with_margins(
            top.unwrap_or(setup.margin_top),
            bottom.unwrap_or(setup.margin_bottom),
            start.unwrap_or(setup.margin_start),
            end.unwrap_or(setup.margin_end),
        ) {
            Ok(next) => setup = next,
            Err(_) => warnings.push(google_style::warning(
                google_style::DROPPED_DOCUMENT_PART,
                "Google Docs documentStyle body margins leave no valid OpenDoc content area and were dropped",
            )),
        }
    }

    let header = google_style::import_dimension(style, "marginHeader", warnings)?;
    let footer = google_style::import_dimension(style, "marginFooter", warnings)?;
    if header.is_some() || footer.is_some() {
        match setup.with_furniture_margins(
            header.unwrap_or(setup.margin_header),
            footer.unwrap_or(setup.margin_footer),
        ) {
            Ok(next) => setup = next,
            Err(_) => warnings.push(google_style::warning(
                google_style::DROPPED_DOCUMENT_PART,
                "Google Docs documentStyle header/footer margins leave no valid OpenDoc page and were dropped",
            )),
        }
    }
    if let Some(start) = crate::json::optional_u64(style, "pageNumberStart")? {
        match u32::try_from(start) {
            Ok(start) => setup.page_number_start = start,
            Err(_) => warnings.push(google_style::warning(
                google_style::DROPPED_DOCUMENT_PART,
                "Google Docs documentStyle.pageNumberStart is too large for OpenDoc and was dropped",
            )),
        }
    }
    document.page_setup = setup;
    Ok(())
}

/// Import the document-style furniture that has the same scope as OpenDoc's
/// document-wide slots.  A Google first/even variant is safe only when no
/// section boundary can give it another meaning: Google applies a first-page
/// variant to the first page *of every section*, whereas OpenDoc has exactly
/// one first page.  Section-local ownership stays a disclosed loss.
fn import_google_document_furniture(
    value: &Value,
    lists: &GoogleLists,
    document: &mut Document,
    warnings: &mut Vec<ModelWarning>,
) -> Result<(), ImportError> {
    let Some(style) = optional_object(value, "documentStyle")? else {
        return Ok(());
    };
    for (style_id, map_name, slot, label) in [
        (
            "defaultHeaderId",
            "headers",
            HeaderFooterSlot::Header,
            "header",
        ),
        (
            "defaultFooterId",
            "footers",
            HeaderFooterSlot::Footer,
            "footer",
        ),
    ] {
        let Some(id) = optional_str(style, style_id)? else {
            continue;
        };
        if id.is_empty() {
            warnings.push(warning(
                google_style::DROPPED_DOCUMENT_PART,
                &format!("Google Docs {style_id} is empty and its default {label} was dropped"),
            ));
            continue;
        }
        if let Some(blocks) =
            import_google_furniture_fragment(value, lists, id, map_name, style_id, label, warnings)?
        {
            *document.furniture_mut(slot) = blocks;
        }
    }

    if !google_document_furniture_variants_are_global(value) {
        return Ok(());
    }
    for (enabled_name, style_id, map_name, slot, label) in [
        (
            "useFirstPageHeaderFooter",
            "firstPageHeaderId",
            "headers",
            HeaderFooterSlot::FirstPageHeader,
            "first-page header",
        ),
        (
            "useFirstPageHeaderFooter",
            "firstPageFooterId",
            "footers",
            HeaderFooterSlot::FirstPageFooter,
            "first-page footer",
        ),
        (
            "useEvenPageHeaderFooter",
            "evenPageHeaderId",
            "headers",
            HeaderFooterSlot::EvenPageHeader,
            "even-page header",
        ),
        (
            "useEvenPageHeaderFooter",
            "evenPageFooterId",
            "footers",
            HeaderFooterSlot::EvenPageFooter,
            "even-page footer",
        ),
    ] {
        // Google IDs alone do not select a variant.  The policy is the source
        // fact that distinguishes an unused stored fragment from an override.
        if !crate::json::optional_bool(style, enabled_name)?.unwrap_or(false) {
            continue;
        }
        let Some(id) = optional_str(style, style_id)? else {
            // With the policy enabled but no ID Google displays no furniture;
            // retain that intentional suppression instead of inheriting ours.
            *document.furniture_mut(slot) = Vec::new();
            continue;
        };
        if id.is_empty() {
            warnings.push(warning(
                google_style::DROPPED_DOCUMENT_PART,
                &format!("Google Docs {style_id} is empty and its {label} was dropped"),
            ));
            continue;
        }
        if let Some(blocks) =
            import_google_furniture_fragment(value, lists, id, map_name, style_id, label, warnings)?
        {
            *document.furniture_mut(slot) = blocks;
        }
    }
    Ok(())
}

fn import_google_furniture_fragment(
    value: &Value,
    lists: &GoogleLists,
    id: &str,
    map_name: &str,
    style_id: &str,
    label: &str,
    warnings: &mut Vec<ModelWarning>,
) -> Result<Option<Vec<Block>>, ImportError> {
    let Some(map) = optional_object(value, map_name)? else {
        warnings.push(warning(
            google_style::DROPPED_DOCUMENT_PART,
            &format!("Google Docs {style_id} references {id:?}, but the {map_name} map is absent; its {label} was dropped"),
        ));
        return Ok(None);
    };
    let Some(fragment) = map.get(id) else {
        warnings.push(warning(
            google_style::DROPPED_DOCUMENT_PART,
            &format!("Google Docs {style_id} references missing {map_name}[{id:?}]; its {label} was dropped"),
        ));
        return Ok(None);
    };
    let content = optional_array(fragment, "content")?.ok_or_else(|| {
        ImportError::InvalidInput(format!(
            "Google Docs {map_name}[{id:?}] is missing content array"
        ))
    })?;
    let mut blocks = Vec::new();
    for element in content {
        blocks.extend(import_google_structural_element(
            element, lists, warnings, 0,
        )?);
    }
    Ok(Some(blocks))
}

/// No interior Google section may be projected as one OpenDoc section.  A
/// lone initial section is harmless only if it does not override furniture or
/// its selection policy; then it simply inherits `documentStyle`.
fn google_document_furniture_variants_are_global(value: &Value) -> bool {
    const SECTION_FURNITURE_FIELDS: [&str; 7] = [
        "defaultHeaderId",
        "defaultFooterId",
        "firstPageHeaderId",
        "firstPageFooterId",
        "evenPageHeaderId",
        "evenPageFooterId",
        "useFirstPageHeaderFooter",
    ];
    let Some(content) = value.pointer("/body/content").and_then(Value::as_array) else {
        return true;
    };
    let mut section_count = 0usize;
    for element in content {
        if element.get("sectionBreak").is_none() {
            continue;
        }
        section_count += 1;
        let Some(style) = element
            .pointer("/sectionBreak/sectionStyle")
            .and_then(Value::as_object)
        else {
            continue;
        };
        if SECTION_FURNITURE_FIELDS
            .iter()
            .any(|field| style.contains_key(*field))
        {
            return false;
        }
    }
    section_count <= 1
}

/// The Docs API moves document content below `tabs[].documentTab` when the
/// caller requests `includeTabsContent=true`. OpenDoc has one document body,
/// not a tab tree, so admit the first tab through the ordinary importer while
/// making every unrepresented tab explicit. This is deliberately a source
/// projection, not a claim that tab ids, titles, hierarchy, or tab-local
/// anchors round-trip.
fn normalize_google_tabbed_document(
    value: &mut Value,
    warnings: &mut Vec<ModelWarning>,
) -> Result<(), ImportError> {
    let Some(tabs) = value.get("tabs") else {
        return Ok(());
    };
    let tabs = tabs.as_array().ok_or_else(|| {
        ImportError::InvalidInput("Google Docs tabs must be an array".to_string())
    })?;
    if tabs.is_empty() {
        return Ok(());
    }
    let count = google_tab_count(tabs);
    let first = tabs.first().expect("non-empty tabs has a first element");
    // Preserve enough source identity in the degradation message for a caller
    // to tell which tab was actually admitted.  This is intentionally
    // diagnostic-only: a tab title is neither a durable OpenDoc document
    // title nor a substitute for a tab-tree model.
    let selected_tab = first
        .pointer("/tabProperties/title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .or_else(|| {
            first
                .pointer("/tabProperties/tabId")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|id| !id.is_empty())
        })
        .unwrap_or("unnamed first tab")
        .to_string();
    let document_tab = first
        .get("documentTab")
        .and_then(Value::as_object)
        .cloned()
        .ok_or_else(|| {
            ImportError::InvalidInput(
                "first Google Docs tab has no documentTab content".to_string(),
            )
        })?;

    // `includeTabsContent` leaves the legacy root fields empty. Copy the
    // first tab's existing native fields before the ordinary import pipeline
    // resolves lists, notes and inline-image resources from those fields.
    let root = value.as_object_mut().ok_or_else(|| {
        ImportError::InvalidInput("Google Docs document must be an object".to_string())
    })?;
    for field in [
        "body",
        "headers",
        "footers",
        "footnotes",
        "lists",
        "namedRanges",
        "inlineObjects",
        "positionedObjects",
        "documentStyle",
        "namedStyles",
    ] {
        if let Some(tab_value) = document_tab.get(field) {
            root.insert(field.to_string(), tab_value.clone());
        }
    }
    warnings.push(ModelWarning {
        code: "google-tabs-first-tab-only".to_string(),
        message: format!(
            "Google Docs tab hierarchy has {count} tab(s); only the first tab ({selected_tab}) was imported because OpenDoc has one document body"
        ),
    });
    Ok(())
}

/// Count top-level and child tabs iteratively: Google tab hierarchies are
/// source input and must not gain an unbounded recursive traversal merely to
/// produce a degradation warning.
fn google_tab_count(tabs: &[Value]) -> usize {
    let mut count = 0usize;
    let mut pending = tabs.iter().collect::<Vec<_>>();
    while let Some(tab) = pending.pop() {
        count = count.saturating_add(1);
        if let Some(children) = tab.get("childTabs").and_then(Value::as_array) {
            pending.extend(children);
        }
    }
    count
}

/// Turns the API's ID-only inline object reference into the existing typed
/// image extension before regular structural import. OpenDoc image blocks are
/// block-level, so an inline object surrounded by text is split out after its
/// paragraph and named rather than pretending it remained in a text run.
struct MaterializedGoogleInlineImages {
    blobs: Vec<ImportedBlob>,
    inline_image_split_ranges: Vec<(u64, u64)>,
}

fn materialize_google_inline_images(
    value: &mut Value,
    resources: &BTreeMap<String, ExportImage>,
    warnings: &mut Vec<ModelWarning>,
) -> Result<MaterializedGoogleInlineImages, ImportError> {
    let inline_objects = value.get("inlineObjects").cloned().unwrap_or(Value::Null);
    let content = value
        .pointer_mut("/body/content")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| ImportError::InvalidInput("missing body.content array".to_string()))?;
    let mut blobs = Vec::new();
    let mut seen = BTreeSet::new();
    let mut handled_any = false;
    let mut unresolved = false;
    let mut rewritten = Vec::with_capacity(content.len());
    // A native bookmark points into the source paragraph, but materialising
    // an inline image turns one such paragraph into several blocks. Its API
    // JSON does not give element offsets with which to select one of them, so
    // retain the source interval and reject a guessed bookmark projection.
    let mut inline_image_split_ranges = Vec::new();
    for mut element in content.drain(..) {
        let Some(paragraph_elements) = element
            .pointer("/paragraph/elements")
            .and_then(Value::as_array)
            .cloned()
        else {
            rewritten.push(element);
            continue;
        };
        // OpenDoc images are block objects. Preserve their source position by
        // splitting a Google paragraph into same-style text fragments around
        // each authorised image, rather than retaining all prose and appending
        // every image after the paragraph.
        let mut segment = Vec::new();
        let mut split = false;
        for paragraph_element in paragraph_elements {
            let Some(id) = paragraph_element
                .pointer("/inlineObjectElement/inlineObjectId")
                .and_then(Value::as_str)
            else {
                segment.push(paragraph_element);
                continue;
            };
            let Some(resource) = resources.get(id) else {
                unresolved = true;
                warn_google_inline_image_metadata_without_resource(&inline_objects, id, warnings);
                segment.push(paragraph_element);
                continue;
            };
            if !google_inline_image_resource_is_usable(resource) {
                unresolved = true;
                warnings.push(ModelWarning {
                    code: "google-inline-image-resource-invalid".to_string(),
                    message: format!(
                        "Google inline image {id} was not materialised because its supplied resource must have non-empty bytes and an image media type"
                    ),
                });
                warn_google_inline_image_metadata_without_resource(&inline_objects, id, warnings);
                segment.push(paragraph_element);
                continue;
            }
            if !segment.is_empty() {
                let mut fragment = element.clone();
                fragment["paragraph"]["elements"] = Value::Array(std::mem::take(&mut segment));
                rewritten.push(fragment);
            }
            handled_any = true;
            let hash = opendoc_core::digest_bytes("sha256", &resource.bytes)
                .map_err(|error| {
                    ImportError::UnsupportedStructure(format!(
                        "Google image resource {id} cannot be hashed: {error}"
                    ))
                })?
                .to_string();
            if seen.insert(hash.clone()) {
                blobs.push(ImportedBlob {
                    name: format!("google-{id}"),
                    media_type: resource.media_type.clone(),
                    hash: hash.clone(),
                    bytes: resource.bytes.clone(),
                });
            }
            let object = inline_objects.get(id).unwrap_or(&Value::Null);
            let properties = object.get("inlineObjectProperties").unwrap_or(object);
            let alt = google_inline_image_alt(properties);
            if alt.is_none() {
                warnings.push(ModelWarning {
                    code: "google-inline-image-accessibility-metadata-missing".to_string(),
                    message: format!(
                        "Google inline image {id} has no title or description, so it was imported without author-provided accessible text"
                    ),
                });
            }
            // Native Google inline objects carry their displayed dimensions
            // in points. They map exactly to OpenDoc twips, but a zero or
            // sub-minimum source box cannot be a visible ImageLayout, so it
            // stays unspecified rather than invalidating an otherwise useful
            // authorised asset import.
            let size = properties
                .pointer("/embeddedObject/size")
                .unwrap_or(&Value::Null);
            let mut image_dimension =
                |key: &str| -> Result<Option<opendoc_core::Length>, ImportError> {
                    let value = google_style::import_dimension(size, key, warnings)?;
                    match value {
                        Some(value) if value.twips() >= opendoc_core::ImageLayout::MIN_TWIPS => {
                            Ok(Some(value))
                        }
                        Some(_) => {
                            warnings.push(ModelWarning {
                            code: "google-inline-image-size-unrepresentable".to_string(),
                            message: format!(
                                "Google inline image {id} {key} is below OpenDoc's minimum visible image size and was left unspecified"
                            ),
                        });
                            Ok(None)
                        }
                        None => Ok(None),
                    }
                };
            let layout = opendoc_core::ImageLayout {
                width: image_dimension("width")?,
                height: image_dimension("height")?,
                crop: google_inline_image_crop(properties, id, warnings),
                ..opendoc_core::ImageLayout::default()
            };
            warnings.push(ModelWarning { code: "google-inline-image-split".to_string(), message: format!("Google inline image {id} was imported as a following image block because OpenDoc has no inline image run") });
            let mut image = json!({ "blockId": StableId::new("block").to_string(), "blobHash": hash, "altText": alt.unwrap_or_default() });
            if !layout.is_empty() {
                image["layout"] = serde_json::to_value(layout).map_err(|error| {
                    ImportError::UnsupportedStructure(format!(
                        "Google inline image {id} layout could not be encoded: {error}"
                    ))
                })?;
            }
            rewritten.push(json!({ "opendocImage": image }));
            split = true;
        }
        if split {
            if let (Some(start), Some(end)) = (
                crate::json::optional_u64(&element, "startIndex")?,
                crate::json::optional_u64(&element, "endIndex")?,
            ) {
                if start < end {
                    inline_image_split_ranges.push((start, end));
                }
            }
            if !segment.is_empty() {
                element["paragraph"]["elements"] = Value::Array(segment);
                rewritten.push(element);
            }
        } else {
            rewritten.push(element);
        }
    }
    *content = rewritten;
    // Avoid a second, contradictory top-level "dropped" warning only when
    // every referenced native object was materialised. An object map with no
    // supplied bytes (or only a partial resource bundle) remains visible to
    // the ordinary loss reporter.
    if handled_any && !unresolved {
        value["inlineObjects"] = Value::Null;
    }
    Ok(MaterializedGoogleInlineImages {
        blobs,
        inline_image_split_ranges,
    })
}

/// Accessibility metadata names an image, not an arbitrary paragraph. When
/// the caller did not supply usable image bytes, retaining it as visible text
/// would falsely turn alternative text into a caption; dropping the object
/// without naming this loss would be silent.
fn warn_google_inline_image_metadata_without_resource(
    inline_objects: &Value,
    id: &str,
    warnings: &mut Vec<ModelWarning>,
) {
    let object = inline_objects.get(id).unwrap_or(&Value::Null);
    let properties = object.get("inlineObjectProperties").unwrap_or(object);
    if google_inline_image_alt(properties).is_some() {
        warnings.push(ModelWarning {
            code: "google-inline-image-accessibility-metadata-unrepresentable".to_string(),
            message: format!(
                "Google inline image {id} had title or description metadata, but no usable supplied image resource existed to carry it"
            ),
        });
    }
}

/// A caller-authorised resource still needs to describe an image before it
/// may replace an API inline-object reference. The bytes intentionally remain
/// opaque: this boundary may carry a browser-decodable format that OpenDoc
/// itself does not parse. It only rejects an empty payload or a non-image
/// declared media type, both of which would otherwise create a misleading
/// image block and hide the original import warning.
fn google_inline_image_resource_is_usable(resource: &ExportImage) -> bool {
    let essence = resource
        .media_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim();
    !resource.bytes.is_empty()
        && essence.split_once('/').is_some_and(|(kind, subtype)| {
            // `image/*` is an Accept-range, not the media type of retained
            // bytes; a slash or whitespace in the subtype is equally unable
            // to name a faithful raw-image download.  This boundary need not
            // decode the bytes, but it must preserve a concrete, syntactically
            // valid declared media-type essence for downstream exporters.
            kind.eq_ignore_ascii_case("image") && subtype != "*" && is_media_type_token(subtype)
        })
}

/// RFC 9110's `token` alphabet, sufficient for a media-type subtype after
/// parameters have been stripped.  Keep this local rather than normalising a
/// declared type: import must preserve the caller-supplied resource exactly.
fn is_media_type_token(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                )
        })
}

/// Google exposes an image title and description separately, but says both
/// participate in the displayed alt text. OpenDoc deliberately has one
/// accessible-text field, so preserve both textual values in source order on
/// separate lines rather than dropping the title whenever a description is
/// present. The separator is a projection boundary, not visible image text.
/// An absent value remains absent instead of fabricating generic alt text that
/// a later export could mistake for source-authored accessibility metadata.
fn google_inline_image_alt(properties: &Value) -> Option<String> {
    let text = |field: &str| {
        properties
            .pointer(field)
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
    };
    match (
        text("/embeddedObject/title"),
        text("/embeddedObject/description"),
    ) {
        (Some(title), Some(description)) => Some(format!("{title}\n{description}")),
        (Some(title), None) => Some(title.to_string()),
        (None, Some(description)) => Some(description.to_string()),
        (None, None) => None,
    }
}

/// Google represents image crops as fractional source-edge offsets.  The
/// model deliberately has whole percentages, so retain the nearest value only
/// for an ordinary inward crop.  Google also permits a crop rectangle outside
/// the source image; that needs a larger canvas model and must not be mistaken
/// for OpenDoc's subtractive crop.
fn google_inline_image_crop(
    properties: &Value,
    id: &str,
    warnings: &mut Vec<ModelWarning>,
) -> Option<ImageCrop> {
    let crop_properties = properties.pointer("/embeddedObject/imageProperties/cropProperties")?;
    if !crop_properties.is_object() {
        warnings.push(ModelWarning {
            code: "google-inline-image-crop-unrepresentable".to_string(),
            message: format!(
                "Google inline image {id} has malformed crop properties that OpenDoc cannot represent"
            ),
        });
        return None;
    }
    let edge = |key: &str| {
        crop_properties.get(key).map_or(Ok(0), |value| {
            let value = value.as_f64().filter(|value| value.is_finite());
            let Some(value) = value else {
                return Err(());
            };
            if !(0.0..=1.0).contains(&value) {
                return Err(());
            }
            u8::try_from((value * 100.0).round() as u16).map_err(|_| ())
        })
    };
    let crop = match (
        edge("offsetTop"),
        edge("offsetRight"),
        edge("offsetBottom"),
        edge("offsetLeft"),
    ) {
        (Ok(top_percent), Ok(right_percent), Ok(bottom_percent), Ok(left_percent)) => ImageCrop {
            top_percent,
            right_percent,
            bottom_percent,
            left_percent,
        },
        _ => {
            warnings.push(ModelWarning {
                code: "google-inline-image-crop-unrepresentable".to_string(),
                message: format!(
                    "Google inline image {id} has a malformed or out-of-source crop that OpenDoc cannot represent"
                ),
            });
            return None;
        }
    };
    if crop.is_empty() {
        return None;
    }
    if crop.validate().is_ok() {
        Some(crop)
    } else {
        warnings.push(ModelWarning {
            code: "google-inline-image-crop-unrepresentable".to_string(),
            message: format!(
                "Google inline image {id} crop leaves no visible source pixels after whole-percent conversion"
            ),
        });
        None
    }
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
    // Prefer Google's native paragraph-style spelling when OpenDoc's
    // structural PageBreak immediately precedes a paragraph. The boundary is
    // still emitted explicitly before tables/extensions and at document end,
    // where there is no following paragraph style to own it.
    let mut content = Vec::new();
    let mut index = 0;
    while index < document.blocks.len() {
        let block = &document.blocks[index];
        if matches!(&block.kind, BlockKind::PageBreak)
            && document
                .blocks
                .get(index + 1)
                .is_some_and(can_export_page_break_before)
        {
            let mut following =
                export_google_block(&document.blocks[index + 1], true, &mut warnings)?;
            set_google_page_break_before(&mut following);
            content.push(following);
            index += 2;
            continue;
        }
        content.push(export_google_block(block, true, &mut warnings)?);
        index += 1;
    }
    let mut value = json!({
        "title": document.title,
        "body": { "content": content },
    });
    if document.page_setup != Default::default()
        || !document.header.is_empty()
        || !document.footer.is_empty()
        || document.first_page_header.is_some()
        || document.first_page_footer.is_some()
        || document.even_page_header.is_some()
        || document.even_page_footer.is_some()
    {
        let setup = &document.page_setup;
        let dimension = |length: Length| json!({ "magnitude": length.points(), "unit": "PT" });
        value["documentStyle"] = json!({
            "pageSize": {
                "width": dimension(setup.width),
                "height": dimension(setup.height),
            },
            "marginTop": dimension(setup.margin_top),
            "marginBottom": dimension(setup.margin_bottom),
            "marginLeft": dimension(setup.margin_start),
            "marginRight": dimension(setup.margin_end),
            "marginHeader": dimension(setup.margin_header),
            "marginFooter": dimension(setup.margin_footer),
            "pageNumberStart": setup.page_number_start,
        });
    }
    // Google's map keys are transport-local identifiers rather than document
    // content. Emit deterministic keys for our document-level slots. The
    // policy fields are essential: an alternate ID with a disabled policy is
    // not an override in Google's rendering model.
    for (slot, map_name, style_id, policy_name, key) in [
        (
            HeaderFooterSlot::Header,
            "headers",
            "defaultHeaderId",
            None,
            "opendoc.default-header",
        ),
        (
            HeaderFooterSlot::Footer,
            "footers",
            "defaultFooterId",
            None,
            "opendoc.default-footer",
        ),
        (
            HeaderFooterSlot::FirstPageHeader,
            "headers",
            "firstPageHeaderId",
            Some("useFirstPageHeaderFooter"),
            "opendoc.first-page-header",
        ),
        (
            HeaderFooterSlot::FirstPageFooter,
            "footers",
            "firstPageFooterId",
            Some("useFirstPageHeaderFooter"),
            "opendoc.first-page-footer",
        ),
        (
            HeaderFooterSlot::EvenPageHeader,
            "headers",
            "evenPageHeaderId",
            Some("useEvenPageHeaderFooter"),
            "opendoc.even-page-header",
        ),
        (
            HeaderFooterSlot::EvenPageFooter,
            "footers",
            "evenPageFooterId",
            Some("useEvenPageHeaderFooter"),
            "opendoc.even-page-footer",
        ),
    ] {
        if slot.is_override() && !document.has_furniture_override(slot) {
            continue;
        }
        let blocks = document.furniture(slot);
        if !slot.is_override() && blocks.is_empty() {
            continue;
        }
        let content = blocks
            .iter()
            .map(|block| export_google_block(block, true, &mut warnings))
            .collect::<Result<Vec<_>, _>>()?;
        if value.get(map_name).is_none() {
            value[map_name] = json!({});
        }
        value[map_name][key] = json!({ "content": content });
        value["documentStyle"][style_id] = Value::String(key.to_string());
        if let Some(policy_name) = policy_name {
            value["documentStyle"][policy_name] = Value::Bool(true);
        }
    }
    let mut list_blocks: Vec<&[Block]> = vec![
        document.blocks.as_slice(),
        document.header.as_slice(),
        document.footer.as_slice(),
    ];
    if let Some(blocks) = &document.first_page_header {
        list_blocks.push(blocks.as_slice());
    }
    if let Some(blocks) = &document.first_page_footer {
        list_blocks.push(blocks.as_slice());
    }
    if let Some(blocks) = &document.even_page_header {
        list_blocks.push(blocks.as_slice());
    }
    if let Some(blocks) = &document.even_page_footer {
        list_blocks.push(blocks.as_slice());
    }
    if let Some(lists) = export_lists(&list_blocks, &document.list_properties, &mut warnings) {
        value["lists"] = lists;
    }
    let footnotes = export_google_footnotes(&document.footnotes)?;
    if !footnotes.is_empty() {
        value["footnotes"] = Value::Object(footnotes);
    }
    let live_endnotes = document
        .footnotes
        .iter()
        .filter(|note| !note.deleted && document.endnote_ids.contains(&note.id))
        .count();
    if live_endnotes != 0 {
        warnings.push(ModelWarning {
            code: "google-export-endnotes-as-footnotes".to_string(),
            message: format!(
                "{live_endnotes} OpenDoc endnote(s) were exported as Google Docs footnotes; Google Docs has no distinct endnote placement"
            ),
        });
    }
    if should_export_citations(&document.citation_database) {
        value["opendocCitations"] = export_google_citations(&document.citation_database)?;
    }
    // A style or locale OpenDoc does not bundle CSL data for renders through
    // the fallback formatter; the export is one of the surfaces that can say
    // so rather than leaving it silent.
    warnings.extend(opendoc_citations::citation_support_warnings(
        &document.citation_database,
    ));
    if !document.comments.is_empty() {
        value["opendocComments"] = export_google_comments(&document.comments)?;
    }
    if !document.comment_history.is_empty() {
        value["opendocCommentHistory"] = export_google_comment_history(&document.comment_history)?;
    }
    if !document.bookmarks.is_empty() {
        value["opendocBookmarks"] = Value::Array(
            document
                .bookmarks
                .iter()
                .map(|bookmark| {
                    json!({
                        "id": bookmark.id.to_string(),
                        "name": bookmark.name,
                        "blockId": bookmark.block_id.to_string(),
                        "revision": bookmark.revision,
                        "deleted": bookmark.deleted,
                    })
                })
                .collect(),
        );
        warnings.push(ModelWarning {
            code: "google-export-bookmarks-opendoc-extension".to_string(),
            message: "bookmarks were written in OpenDoc's extension; native Google Docs bookmark semantics are unavailable".to_string(),
        });
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
/// Page geometry in `documentStyle` is imported separately; the rest must be
/// named rather than vanish.
fn report_dropped_google_document_parts(value: &Value, warnings: &mut Vec<ModelWarning>) {
    const PARTS: [(&str, &str); 4] = [
        (
            "namedStyles",
            "Google Docs named style definitions (namedStyles) are not representable and were dropped; each paragraph keeps only its own explicit formatting",
        ),
        (
            "namedRanges",
            "Google Docs named ranges were not imported because OpenDoc has no character-range anchor model; visible document content was preserved",
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
    // A documentStyle first/even ID is selected only when its policy is on
    // and no section can give it a different scope.  All other map members
    // remain explicit rather than being mistaken for ordinary furniture.
    report_unselected_google_furniture(value, "headers", "header", warnings);
    report_unselected_google_furniture(value, "footers", "footer", warnings);

    // `pageSize`, its orientation-flip spelling, all six margins, and
    // page-number start are native PageSetup projections. The remaining
    // documentStyle fields are page paint and the section-scoped part of the
    // header/footer policy.  Keep those losses distinct: a client receiving a
    // background must not be told that its furniture was lost too, and an
    // empty/default background must not create a phantom loss warning merely
    // because a partial-response field was present.
    if let Some(style) = value.get("documentStyle").and_then(Value::as_object) {
        if google_part_is_present(style.get("background")) {
            warnings.push(ModelWarning {
                code: "google-dropped-document-part".to_string(),
                message: "Google Docs documentStyle.background is page paint and is not representable; page geometry and document furniture were imported".to_string(),
            });
        }
        let has_variant_fields = [
            "evenPageHeaderId",
            "evenPageFooterId",
            "firstPageHeaderId",
            "firstPageFooterId",
        ]
        .iter()
        .any(|field| style.contains_key(*field));
        if has_variant_fields && !google_document_furniture_variants_are_global(value) {
            warnings.push(ModelWarning {
                code: "google-dropped-document-part".to_string(),
                message: "Google Docs documentStyle first/even header/footer IDs are section-scoped and were not flattened into OpenDoc's document-wide furniture; page geometry was imported".to_string(),
            });
        }
    }
}

fn report_unselected_google_furniture(
    value: &Value,
    map_name: &str,
    label: &str,
    warnings: &mut Vec<ModelWarning>,
) {
    let Some(map) = value.get(map_name).and_then(Value::as_object) else {
        return;
    };
    if map.is_empty() {
        return;
    }
    let selected = google_selected_document_furniture_ids(value, map_name);
    let unselected = map
        .keys()
        .any(|id| !selected.iter().any(|selected| *selected == id));
    if unselected {
        warnings.push(warning(
            google_style::DROPPED_DOCUMENT_PART,
            &format!("Google Docs {label} map contains non-default or section-local fragments that were not flattened into OpenDoc's document-wide {label}"),
        ));
    }
}

fn google_selected_document_furniture_ids<'a>(value: &'a Value, map_name: &str) -> Vec<&'a str> {
    let Some(style) = value.get("documentStyle").and_then(Value::as_object) else {
        return Vec::new();
    };
    let mut fields = match map_name {
        "headers" => vec!["defaultHeaderId"],
        "footers" => vec!["defaultFooterId"],
        _ => return Vec::new(),
    };
    if google_document_furniture_variants_are_global(value) {
        let first_enabled = style
            .get("useFirstPageHeaderFooter")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let even_enabled = style
            .get("useEvenPageHeaderFooter")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if first_enabled {
            fields.push(match map_name {
                "headers" => "firstPageHeaderId",
                "footers" => "firstPageFooterId",
                _ => unreachable!(),
            });
        }
        if even_enabled {
            fields.push(match map_name {
                "headers" => "evenPageHeaderId",
                "footers" => "evenPageFooterId",
                _ => unreachable!(),
            });
        }
    }
    fields
        .into_iter()
        .filter_map(|field| style.get(field).and_then(Value::as_str))
        .collect()
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
