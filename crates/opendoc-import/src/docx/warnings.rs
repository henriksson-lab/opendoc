//! The fidelity warning vocabulary and the counter that summarises it.

use opendoc_core::ModelWarning;
use std::collections::{BTreeMap, BTreeSet};

pub(super) const DROPPED_ALIGNMENT: &str = "docx-dropped-alignment";
pub(super) const DROPPED_INDENT: &str = "docx-dropped-indent";
pub(super) const DROPPED_SPACING: &str = "docx-dropped-spacing";
pub(super) const DROPPED_PARAGRAPH_BORDER: &str = "docx-dropped-paragraph-border";
pub(super) const DROPPED_PARAGRAPH_SHADING: &str = "docx-dropped-paragraph-shading";
pub(super) const DROPPED_TABS: &str = "docx-dropped-tabs";
pub(super) const APPROXIMATED_ALIGNMENT: &str = "docx-approximated-alignment";
pub(super) const DROPPED_HEADER_FOOTER: &str = "docx-dropped-header-footer";
pub(super) const DROPPED_SECTION_PROPERTIES: &str = "docx-dropped-section-properties";
pub(super) const DROPPED_FURNITURE_CONTENT: &str = "docx-dropped-header-footer-content";
pub(super) const INVALID_PAGE_SETUP: &str = "docx-invalid-page-setup";
pub(super) const DROPPED_RUN_PROPERTY: &str = "docx-dropped-run-property";
pub(super) const DROPPED_PARAGRAPH_CHANGE: &str = "docx-dropped-paragraph-change";
pub(super) const DROPPED_FORMAT_CHANGE: &str = "docx-dropped-format-change";
pub(super) const DROPPED_DRAWING: &str = "docx-dropped-drawing";
pub(super) const DROPPED_TEXT_BOX: &str = "docx-dropped-text-box";
pub(super) const DROPPED_NESTED_IMAGE: &str = "docx-dropped-nested-image";
pub(super) const DROPPED_NESTED_REVISION: &str = "docx-dropped-nested-revision";
pub(super) const DROPPED_CELL_PROPERTY: &str = "docx-dropped-cell-property";
pub(super) const APPROXIMATED_CELL_BORDER: &str = "docx-approximated-cell-border";
pub(super) const DROPPED_TABLE_BORDER: &str = "docx-dropped-table-border";
pub(super) const DROPPED_TABLE_STYLE_BANDING: &str = "docx-dropped-table-style-banding";
pub(super) const CLAMPED_COLUMN_WIDTH: &str = "docx-clamped-table-column-width";
pub(super) const TABLE_MERGE_REPAIRED: &str = "docx-table-merge-repaired";
pub(super) const DROPPED_ALT_CHUNK: &str = "docx-dropped-alt-chunk";
pub(super) const NESTED_TABLE: &str = "docx-nested-table";
pub(super) const TABLE_NESTING_LIMIT: &str = "docx-table-nesting-limit";
pub(super) const SPLIT_INLINE_IMAGE: &str = "docx-split-inline-image";
pub(super) const SPLIT_PAGE_BREAK: &str = "docx-split-page-break";
pub(super) const UNKNOWN_LIST_DEFINITION: &str = "docx-unknown-list-definition";
pub(super) const MISSING_FOOTNOTE: &str = "docx-missing-footnote";
pub(super) const EMPTY_FOOTNOTE: &str = "docx-empty-footnote";
pub(super) const EMPTY_COMMENT: &str = "docx-empty-comment";
pub(super) const COMMENT_ANCHOR_DEGRADED: &str = "docx-comment-anchor-degraded";
pub(super) const MISSING_IMAGE_BLOB: &str = "missing-docx-image-blob";
pub(super) const DROPPED_IMAGE_OPACITY: &str = "docx-dropped-image-opacity";
pub(super) const DROPPED_IMAGE_CROP: &str = "docx-dropped-image-crop";
pub(super) const DROPPED_IMAGE_BORDER: &str = "docx-dropped-image-border";
pub(super) const DROPPED_POSITIONED_IMAGE: &str = "docx-dropped-positioned-image";
pub(super) const DROPPED_BOOKMARK_RANGE: &str = "docx-bookmark-range-unrepresentable";
pub(super) const DROPPED_BOOKMARK_NAME: &str = "docx-bookmark-name-unrepresentable";
pub(super) const DROPPED_BOOKMARK_DUPLICATE: &str = "docx-bookmark-duplicate-name";

pub(super) fn dropped_message(code: &str) -> &'static str {
    match code {
        TABLE_NESTING_LIMIT => {
            "DOCX tables nested past the depth OpenDoc represents were flattened; their cell contents were kept as blocks"
        }
        DROPPED_ALIGNMENT => {
            "DOCX paragraph alignment values (w:jc) that OpenDoc cannot represent were dropped"
        }
        DROPPED_INDENT => {
            "DOCX paragraph indentation values (w:ind) that OpenDoc cannot represent were dropped"
        }
        DROPPED_SPACING => {
            "DOCX paragraph spacing values (w:spacing) that OpenDoc cannot represent were dropped"
        }
        APPROXIMATED_ALIGNMENT => {
            "DOCX distributed alignment (w:jc distribute) was imported as justified"
        }
        DROPPED_PARAGRAPH_BORDER => {
            "DOCX paragraph borders (w:pBdr) are not representable and were dropped"
        }
        DROPPED_PARAGRAPH_SHADING => {
            "DOCX paragraph shading (w:shd) is not representable and was dropped"
        }
        DROPPED_TABS => "DOCX custom tab stops (w:tabs) are not representable and were dropped",
        DROPPED_HEADER_FOOTER => {
            "DOCX first-page and even/odd header or footer variants were dropped; OpenDoc has one header and one footer for the whole document"
        }
        DROPPED_SECTION_PROPERTIES => {
            "DOCX section properties OpenDoc's single-section page model cannot represent (extra sections, multiple columns) were dropped"
        }
        DROPPED_FURNITURE_CONTENT => {
            "DOCX header or footer content that cannot sit outside the body flow (page breaks, footnote references) was dropped"
        }
        INVALID_PAGE_SETUP => {
            "DOCX page geometry (w:pgSz/w:pgMar) was outside what OpenDoc can represent; the default page setup was used"
        }
        DROPPED_RUN_PROPERTY => "DOCX run properties without an OpenDoc mark were dropped",
        DROPPED_PARAGRAPH_CHANGE => {
            "DOCX tracked paragraph property changes (w:pPrChange) were dropped"
        }
        DROPPED_FORMAT_CHANGE => {
            "DOCX tracked formatting changes (w:rPrChange) without representable marks were dropped"
        }
        DROPPED_DRAWING => "DOCX drawings or shapes without image data were dropped",
        DROPPED_TEXT_BOX => "DOCX text boxes are not representable and were dropped",
        DROPPED_NESTED_IMAGE => {
            "DOCX images inside footnotes, comments or tracked insertions were dropped"
        }
        DROPPED_NESTED_REVISION => {
            "DOCX tracked changes inside footnotes or comments were flattened into plain text"
        }
        DROPPED_CELL_PROPERTY => {
            "DOCX table cell properties (w:tcPr) that OpenDoc cannot represent were dropped"
        }
        APPROXIMATED_CELL_BORDER => {
            "DOCX table cell borders were approximated: an artistic border style became a plain line, or a thickness was rounded to the nearest twip"
        }
        DROPPED_TABLE_BORDER => {
            "DOCX table borders (w:tblBorders) with no per-cell equivalent (the diagonals w:tl2br and w:tr2bl) were dropped"
        }
        DROPPED_TABLE_STYLE_BANDING => {
            "DOCX table styles' conditional formatting (w:tblStylePr: banded rows and columns, first and last row, first and last column) was not applied; only the style's plain table and cell formatting was read"
        }
        CLAMPED_COLUMN_WIDTH => {
            "DOCX table column widths (w:gridCol) outside what OpenDoc can represent were clamped into range"
        }
        TABLE_MERGE_REPAIRED => {
            "DOCX vertically merged table cells (w:vMerge) whose continuation had no restart above it were imported as ordinary cells, keeping their content"
        }
        DROPPED_ALT_CHUNK => "DOCX embedded alternate content chunks (w:altChunk) were dropped",
        NESTED_TABLE => {
            "DOCX nested tables were imported as tables inside table cells"
        }
        SPLIT_INLINE_IMAGE => {
            "DOCX inline images mixed with text were imported as standalone image blocks, splitting the paragraph"
        }
        SPLIT_PAGE_BREAK => {
            "DOCX page breaks inside paragraphs were imported as standalone page break blocks, splitting the paragraph"
        }
        UNKNOWN_LIST_DEFINITION => {
            "DOCX list paragraphs referenced numbering definitions that could not be resolved; imported as bullet items"
        }
        MISSING_FOOTNOTE => "DOCX footnote or endnote references without a definition were dropped",
        DROPPED_IMAGE_OPACITY => {
            "DOCX image opacity values outside DrawingML's 0..=100000 range, or without an amount, were dropped"
        }
        DROPPED_IMAGE_CROP => {
            "DOCX image crop values that were malformed or left no visible source pixels were dropped"
        }
        DROPPED_IMAGE_BORDER => {
            "DOCX image borders outside OpenDoc's solid/dashed/dotted sRGB subset were dropped"
        }
        EMPTY_FOOTNOTE => "DOCX footnotes or endnotes with an empty body were dropped",
        EMPTY_COMMENT => "DOCX comments with an empty body were dropped",
        COMMENT_ANCHOR_DEGRADED => {
            "DOCX comment ranges that did not cover inline text were anchored to the nearest block or the document"
        }
        DROPPED_POSITIONED_IMAGE => {
            "DOCX positioned-image anchor geometry or layer is not representable and was imported as an in-flow image"
        }
        DROPPED_BOOKMARK_RANGE => {
            "DOCX bookmark ranges or positions that could not map exactly to one imported text block were not imported"
        }
        DROPPED_BOOKMARK_NAME => {
            "DOCX bookmark names outside OpenDoc's portable bookmark-name syntax were not imported"
        }
        DROPPED_BOOKMARK_DUPLICATE => {
            "DOCX bookmarks whose names collide after import were not imported"
        }
        _ => "DOCX content was dropped",
    }
}

#[derive(Default)]
pub(super) struct DroppedCounter {
    counts: BTreeMap<&'static str, usize>,
    pub(super) run_property_names: BTreeSet<&'static str>,
}

impl DroppedCounter {
    pub(super) fn count(&mut self, code: &'static str) {
        self.count_n(code, 1);
    }

    pub(super) fn count_n(&mut self, code: &'static str, n: usize) {
        if n == 0 {
            return;
        }
        *self.counts.entry(code).or_insert(0) += n;
    }

    pub(super) fn into_warnings(self) -> Vec<ModelWarning> {
        self.counts
            .into_iter()
            .map(|(code, count)| {
                let mut message = dropped_message(code).to_string();
                if code == DROPPED_RUN_PROPERTY && !self.run_property_names.is_empty() {
                    message.push_str(&format!(
                        " ({})",
                        self.run_property_names
                            .iter()
                            .copied()
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
                message.push_str(&format!(" ({count} occurrence{})", plural(count)));
                ModelWarning {
                    code: code.to_string(),
                    message,
                }
            })
            .collect()
    }
}

pub(super) fn plural(count: usize) -> &'static str {
    if count == 1 {
        ""
    } else {
        "s"
    }
}

// ---------------------------------------------------------------------------
// Conversion state
// ---------------------------------------------------------------------------
