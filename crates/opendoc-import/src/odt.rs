//! Deliberately bounded OpenDocument Text reader.
//!
//! ODF bookmarks are ranges, while OpenDoc bookmarks identify one stable
//! block.  This reader therefore admits only the one shape with an exact
//! projection: a paired, direct `text:bookmark-start` and
//! `text:bookmark-end` wrapping one `text:p` or `text:h`.  Every other range
//! is reported instead of being silently attached to an arbitrary block.

use crate::xml::{parse_xml_bytes, XmlElement, XmlNode, MAX_XML_BYTES};
use crate::ImportError;
use opendoc_core::{Block, BlockKind, Bookmark, Document, ModelWarning, StableId};
use std::collections::BTreeSet;
use std::io::{Cursor, Read};

const MAX_ENTRIES: usize = 8192;
const BOOKMARK_RANGE: &str = "odt-bookmark-range-unrepresentable";
const BOOKMARK_NAME: &str = "odt-bookmark-name-unrepresentable";
const BOOKMARK_DUPLICATE: &str = "odt-bookmark-duplicate-name";
const TABLE_OF_CONTENTS: &str = "odt-table-of-contents-unrepresentable";
const BODY_CONTENT: &str = "odt-body-content-unrepresentable";
const IMAGE_CONTENT: &str = "odt-image-unrepresentable";

pub(super) struct OdtImport {
    pub(super) document: Document,
    pub(super) warnings: Vec<ModelWarning>,
}

/// Imports an ODT package, or the `office:document-content` XML that sits in
/// its `content.xml`.  A package is screened before the sole body part is
/// inflated; this is intentionally not a generic zip extractor.
pub(super) fn import_odt_bytes(title: &str, bytes: &[u8]) -> Result<OdtImport, ImportError> {
    let root = if bytes.starts_with(b"PK") {
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes.to_vec())).map_err(|error| {
            ImportError::InvalidInput(format!("ODT package could not be opened: {error}"))
        })?;
        if archive.len() > MAX_ENTRIES {
            return Err(ImportError::InvalidInput(format!(
                "ODT package holds {} entries, over the {MAX_ENTRIES} limit",
                archive.len()
            )));
        }
        let file = archive
            .by_name("content.xml")
            .map_err(|_| ImportError::InvalidInput("ODT package has no content.xml".to_string()))?;
        if file.size() > MAX_XML_BYTES as u64 {
            return Err(ImportError::InvalidInput(format!(
                "ODT content.xml declares {} bytes, over the {MAX_XML_BYTES}-byte limit",
                file.size()
            )));
        }
        let mut content = Vec::new();
        file.take(MAX_XML_BYTES as u64 + 1)
            .read_to_end(&mut content)
            .map_err(|error| {
                ImportError::InvalidInput(format!("ODT content.xml could not be read: {error}"))
            })?;
        parse_xml_bytes(&content).map_err(|error| {
            ImportError::InvalidInput(format!("ODT content.xml is malformed: {error}"))
        })?
    } else {
        parse_xml_bytes(bytes).map_err(|error| {
            ImportError::InvalidInput(format!(
                "ODT input is neither a ZIP package nor content.xml: {error}"
            ))
        })?
    };
    if !root.is("document-content") {
        return Err(ImportError::InvalidInput(
            "ODT XML root element is not office:document-content".to_string(),
        ));
    }
    let text = root.find_descendant("text").ok_or_else(|| {
        ImportError::InvalidInput("ODT content.xml has no office:text body".to_string())
    })?;
    let mut reader = Reader {
        document: Document::new(title),
        warnings: Vec::new(),
        bookmark_names: BTreeSet::new(),
        rejected_ranges: 0,
        rejected_names: 0,
        duplicate_names: 0,
        dropped_table_of_contents: 0,
        dropped_body_content: 0,
        dropped_images: 0,
    };
    reader.walk_body(text);
    reader.finish()
}

struct Reader {
    document: Document,
    warnings: Vec<ModelWarning>,
    bookmark_names: BTreeSet<String>,
    rejected_ranges: usize,
    rejected_names: usize,
    duplicate_names: usize,
    dropped_table_of_contents: usize,
    /// Top-level ODF containers and inline objects which this deliberately
    /// text-only reader cannot turn into an OpenDoc block.  Keep this count
    /// separate from bookmark ranges: a document must not look faithfully
    /// imported merely because its omitted content held no bookmark.
    dropped_body_content: usize,
    /// `draw:image` has a distinct asset and geometry loss from the frame or
    /// container that happened to own it, so surface it independently.
    dropped_images: usize,
}

impl Reader {
    fn walk_body(&mut self, container: &XmlElement) {
        for child in container.elements() {
            match child.local.as_str() {
                "p" | "h" => self.import_text_block(child),
                // A native ODF index is a generated result with templates,
                // cached entries and update behaviour. This bounded reader
                // does not yet interpret that vocabulary as OpenDoc's
                // derived TOC block, so do not silently discard it.
                "table-of-content" => {
                    self.dropped_table_of_contents += 1;
                    self.dropped_images += odt_image_count(child);
                }
                // Sections are transparent containers for body blocks.  A
                // bookmark crossing a section boundary necessarily fails the
                // per-block exact-range rule below.
                "section" => self.walk_body(child),
                // Do not descend into lists, notes, frames, tables or
                // annotations: their paragraphs do not have a top-level
                // stable block in this bounded reader.  Count the loss so a
                // successful import never silently pretends that the source
                // contained only the surrounding paragraphs.
                _ => {
                    self.dropped_body_content += 1;
                    self.dropped_images += odt_image_count(child);
                }
            }
        }
    }

    fn import_text_block(&mut self, source: &XmlElement) {
        self.dropped_body_content += unsupported_inline_body_content(source);
        self.dropped_images += odt_image_count(source);
        let candidates = self.whole_block_bookmarks(source);
        let mut block = Block::paragraph(odf_text(source));
        if source.is("h") {
            let level = source
                .attr("outline-level")
                .and_then(|value| value.trim().parse::<u8>().ok())
                .filter(|level| (1..=6).contains(level))
                .unwrap_or(1);
            block.kind = BlockKind::Heading { level };
        }
        let target = block.id.clone();
        // Empty paragraphs have no text-bearing target.  Keep the block, but
        // do not turn an ODF zero-width range into a fabricated anchor.
        if block.content.is_empty() || odf_text(source).is_empty() {
            self.rejected_ranges += candidates.len();
        } else {
            for name in candidates {
                let bookmark = Bookmark {
                    id: StableId::new("odt-bookmark"),
                    name,
                    block_id: target.clone(),
                    revision: 1,
                    deleted: false,
                };
                if bookmark.validate().is_err() {
                    self.rejected_names += 1;
                } else if !self.bookmark_names.insert(bookmark.name.clone()) {
                    self.duplicate_names += 1;
                } else {
                    self.document.bookmarks.push(bookmark);
                }
            }
        }
        self.document.blocks.push(block);
    }

    /// Collect only direct pairs at the outside of one paragraph/heading.
    /// Nested markers and incomplete/cross-block pairs remain a range rather
    /// than a stable-block bookmark, so they are named and rejected.
    fn whole_block_bookmarks(&mut self, block: &XmlElement) -> Vec<String> {
        let mut starts = Vec::<(String, usize)>::new();
        let mut ends = Vec::<(String, usize)>::new();
        let mut position = 0usize;
        let mut nested = 0usize;
        for child in &block.children {
            match child {
                XmlNode::Element(element) if element.is("bookmark-start") => {
                    let Some(name) = element
                        .attr("name")
                        .map(str::trim)
                        .filter(|name| !name.is_empty())
                    else {
                        self.rejected_ranges += 1;
                        continue;
                    };
                    starts.push((name.to_string(), position));
                }
                XmlNode::Element(element) if element.is("bookmark-end") => {
                    let Some(name) = element
                        .attr("name")
                        .map(str::trim)
                        .filter(|name| !name.is_empty())
                    else {
                        self.rejected_ranges += 1;
                        continue;
                    };
                    ends.push((name.to_string(), position));
                }
                // `text:bookmark` is ODF's point form, not a range.  Its
                // character position has no stable-block equivalent, even
                // when it happens to be adjacent to an endpoint.
                XmlNode::Element(element) if element.is("bookmark") => {
                    self.rejected_ranges += 1;
                    position += 1;
                }
                XmlNode::Element(element) => {
                    nested += element
                        .descendants()
                        .into_iter()
                        .filter(|descendant| {
                            descendant.is("bookmark-start") || descendant.is("bookmark-end")
                        })
                        .count();
                    position += 1;
                }
                XmlNode::Text(text) if !text.is_empty() => position += 1,
                XmlNode::Text(_) => {}
            }
        }
        self.rejected_ranges += nested;
        let mut imported = Vec::new();
        for (name, start) in starts {
            let matching: Vec<_> = ends
                .iter()
                .filter(|(end_name, _)| end_name == &name)
                .map(|(_, end)| *end)
                .collect();
            if start == 0 && matching.as_slice() == [position] {
                imported.push(name);
            } else {
                self.rejected_ranges += 1;
            }
        }
        // Ends with no qualifying paired start (including a cross-paragraph
        // close) are independently unrepresentable source ranges.
        for (name, end) in ends {
            if !imported
                .iter()
                .any(|candidate| candidate == &name && end == position)
            {
                self.rejected_ranges += 1;
            }
        }
        imported
    }

    fn finish(mut self) -> Result<OdtImport, ImportError> {
        if self.rejected_ranges > 0 {
            self.warnings.push(count_warning(
                BOOKMARK_RANGE,
                self.rejected_ranges,
                "ODT bookmark ranges or positions that could not map exactly to one imported text block were not imported",
            ));
        }
        if self.rejected_names > 0 {
            self.warnings.push(count_warning(
                BOOKMARK_NAME,
                self.rejected_names,
                "ODT bookmark names outside OpenDoc's portable bookmark-name syntax were not imported",
            ));
        }
        if self.duplicate_names > 0 {
            self.warnings.push(count_warning(
                BOOKMARK_DUPLICATE,
                self.duplicate_names,
                "ODT bookmarks whose names collide after import were not imported",
            ));
        }
        if self.dropped_table_of_contents > 0 {
            self.warnings.push(count_warning(
                TABLE_OF_CONTENTS,
                self.dropped_table_of_contents,
                "native ODT table-of-content element(s) were not imported because generated index semantics are not represented by this bounded reader",
            ));
        }
        if self.dropped_body_content > 0 {
            self.warnings.push(count_warning(
                BODY_CONTENT,
                self.dropped_body_content,
                "native ODT body container(s) or inline object(s), such as tables, lists, frames, notes, or annotations, were not imported by the bounded text reader",
            ));
        }
        if self.dropped_images > 0 {
            self.warnings.push(count_warning(
                IMAGE_CONTENT,
                self.dropped_images,
                "native ODT draw:image element(s) were not imported because the bounded ODT reader does not yet extract picture bytes or frame geometry",
            ));
        }
        self.document
            .validate()
            .map_err(|error| ImportError::InvalidDocument(error.to_string()))?;
        Ok(OdtImport {
            document: self.document,
            warnings: self.warnings,
        })
    }
}

/// Paragraphs themselves have a stable OpenDoc home, but some ODF inline
/// children carry a second object tree.  `odf_text` deliberately retains any
/// fallback text in those trees; it cannot retain the object, so report each
/// outermost object once.  Descending only through ordinary formatting
/// containers avoids double-counting a frame inside a table or note.
fn unsupported_inline_body_content(element: &XmlElement) -> usize {
    element
        .children
        .iter()
        .map(|child| match child {
            XmlNode::Element(child)
                if matches!(
                    child.local.as_str(),
                    "frame" | "table" | "list" | "note" | "annotation"
                ) =>
            {
                1
            }
            XmlNode::Element(child) => unsupported_inline_body_content(child),
            XmlNode::Text(_) => 0,
        })
        .sum()
}

/// Count image elements separately from their surrounding ODF containers.
/// A frame may have already contributed one `odt-body-content-unrepresentable`
/// count, but an image is a materially different loss: it carries bytes and
/// geometry that a caller could otherwise expect in the import report's blob
/// store.  Count every native `draw:image` element once, including images in
/// skipped tables and generated indexes, without attempting to resolve its
/// package path or turn an absent `xlink:href` into a fabricated asset.
fn odt_image_count(element: &XmlElement) -> usize {
    usize::from(element.is("image"))
        + element
            .descendants()
            .into_iter()
            .filter(|descendant| descendant.is("image"))
            .count()
}

fn count_warning(code: &str, count: usize, message: &str) -> ModelWarning {
    ModelWarning {
        code: code.to_string(),
        message: format!("{count} {message}"),
    }
}

/// The bounded body reader intentionally flattens ODF inline formatting, but
/// preserves the ODF whitespace elements so the bookmark target's text is not
/// changed while assessing its range shape.
fn odf_text(element: &XmlElement) -> String {
    let mut out = String::new();
    append_odf_text(element, &mut out);
    out
}

fn append_odf_text(element: &XmlElement, out: &mut String) {
    for child in &element.children {
        match child {
            XmlNode::Text(text) => out.push_str(text),
            XmlNode::Element(element)
                if element.is("bookmark-start") || element.is("bookmark-end") => {}
            XmlNode::Element(element) if element.is("s") => {
                let count = element
                    .attr("c")
                    .and_then(|value| value.parse::<usize>().ok())
                    .filter(|count| *count > 0)
                    .unwrap_or(1);
                out.extend(std::iter::repeat_n(' ', count));
            }
            XmlNode::Element(element) if element.is("tab") => out.push('\t'),
            XmlNode::Element(element) if element.is("line-break") => out.push('\n'),
            XmlNode::Element(element) => append_odf_text(element, out),
        }
    }
}
