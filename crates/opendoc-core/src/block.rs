//! Blocks: the document's structural units and their footnote bodies.

use crate::block_properties::{BlockProperties, ListKind};
use crate::citation::CitationDatabase;
use crate::document::{
    inline_sequence_is_empty_source_text, validate_block_tree, validate_inline_sequence,
};
use crate::ids::validate_stable_id;
use crate::ids::StableId;
use crate::image::ImageLayout;
use crate::inline::{Equation, Inline};
use crate::table::{table_covered_positions, TableCell, TableColumn, TableProperties, TableRow};
use crate::warning::ModelError;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Footnote {
    pub id: StableId,
    pub revision: u64,
    pub body: Vec<Inline>,
    pub deleted: bool,
}

impl Footnote {
    pub fn validate(&self) -> Result<(), ModelError> {
        validate_stable_id("footnote id", &self.id)?;
        if self.body.is_empty() || inline_sequence_is_empty_source_text(&self.body) {
            return Err(ModelError::InvalidDocument("footnote body is empty"));
        }
        validate_inline_sequence(&self.body)?;
        Ok(())
    }
}

/// Which question the document's text walk is answering.
///
/// There are two, they are not the same, and they used to be one function —
/// which is how a formula's LaTeX, an image's alt text and a footnote's
/// `StableId` ended up inside the document panel's word and character counts.
///
/// The split is the same source-versus-projection line ADR 0003 draws
/// everywhere else: one scope asks what the page *shows*, the other asks what
/// a text file can *carry*.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextScope {
    /// The text a reader reads off the page. Content that is not text puts
    /// nothing here, because nothing it would put here is text anyone sees:
    /// an equation is typeset mathematics, an image is a picture, a footnote
    /// reference is a marker the renderer numbers. This is what
    /// [`Document::counted_text`](crate::Document::counted_text) returns and
    /// what the word and character counts count.
    Page,
    /// The document degraded to plain text, for a `.txt` export. Content that
    /// is not text becomes the conventional textual stand-in — an image its
    /// alt text, a formula the LaTeX it is written in — because a file with a
    /// hole in it is worse than a file that says what was there. This is what
    /// [`Document::visible_text`](crate::Document::visible_text) returns.
    PlainText,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Block {
    pub id: StableId,
    pub kind: BlockKind,
    pub content: Vec<Inline>,
    pub properties: BlockProperties,
}

impl Block {
    pub fn paragraph(text: impl Into<String>) -> Self {
        Self {
            id: StableId::new("block"),
            kind: BlockKind::Paragraph,
            content: vec![Inline::text(text)],
            properties: BlockProperties::default(),
        }
    }

    /// The list marker of this block, when it is a list item.
    pub fn list_kind(&self) -> Option<ListKind> {
        self.kind.list_kind()
    }

    /// The list run this block belongs to, when it is a list item.
    pub fn list_id(&self) -> Option<&StableId> {
        self.kind.list_id()
    }

    pub fn validate_isolated(&self) -> Result<(), ModelError> {
        validate_block_tree(std::slice::from_ref(self))
    }

    pub(crate) fn push_visible_text(
        &self,
        citations: &CitationDatabase,
        scope: TextScope,
        out: &mut String,
    ) {
        for inline in &self.content {
            inline.push_visible_text(citations, scope, out);
        }
        match &self.kind {
            BlockKind::Table { rows, .. } => {
                // A cell hidden under a merged neighbour is not visible text.
                // It keeps its content so a split can hand it back, but it is
                // not on the page, so it is not in the page's text either.
                let covered = table_covered_positions(rows);
                for (row_index, row) in rows.iter().enumerate() {
                    if row_index > 0 && !out.ends_with('\n') {
                        out.push('\n');
                    }
                    for (cell_index, cell) in row.cells.iter().enumerate() {
                        if covered.contains(&(row_index, cell_index)) {
                            continue;
                        }
                        if cell_index > 0 {
                            out.push('\t');
                        }
                        for (block_index, block) in cell.blocks.iter().enumerate() {
                            if block_index > 0 && !out.ends_with('\n') {
                                out.push('\n');
                            }
                            block.push_visible_text(citations, scope, out);
                        }
                    }
                }
            }
            // An equation block and an image put no text on the page. What a
            // reader sees of an equation is typeset mathematics — ADR 0003
            // makes the LaTeX the canonical *source* and the rendering a
            // projection — and an image's `alt_text` is an accessibility
            // attribute the page never draws at all. So under
            // [`TextScope::Page`] they contribute nothing, which is the same
            // rule `Inline::PageNumber` and a cell hidden under a merged
            // neighbour are already held to.
            //
            // A `.txt` export is the other question, and its answer is the
            // stand-in: a file with a hole where the figure was is worse than
            // one that says what was there.
            BlockKind::EquationBlock { equation } if scope == TextScope::PlainText => {
                out.push_str(&equation.source)
            }
            BlockKind::Image { alt_text, .. } if scope == TextScope::PlainText => {
                out.push_str(alt_text)
            }
            // Generated bibliography text is deliberately omitted here. The
            // plain-text writer does not own CSL rendering, and emitting a
            // stale or differently-formatted reference list would violate
            // the same source/projection boundary as a TOC.
            BlockKind::Bibliography => {}
            _ => {}
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum BlockKind {
    Paragraph,
    /// A document title.  Unlike a level-one heading this is a display style,
    /// not an outline entry.
    Title,
    /// A document subtitle.  Like [`Self::Title`], this is not an outline
    /// entry and deliberately has no numeric heading level.
    Subtitle,
    Heading {
        level: u8,
    },
    ListItem {
        list_id: StableId,
        level: u8,
        kind: ListKind,
    },
    /// A table is a **rectangular grid**: `columns` names the columns left to
    /// right, and every row carries exactly one [`TableCell`] per column, in
    /// that order. A cell's column is therefore its position, and there is no
    /// second representation of it to keep in sync. Merged cells are spans on
    /// the cell they start at (see [`CellSpan`]), never missing cells, so the
    /// grid stays rectangular however it is merged.
    Table {
        columns: Vec<TableColumn>,
        #[serde(default, skip_serializing_if = "TableProperties::is_empty")]
        properties: TableProperties,
        rows: Vec<TableRow>,
    },
    EquationBlock {
        equation: Equation,
    },
    Image {
        blob_hash: String,
        alt_text: String,
        /// Display geometry. Skipped when empty, so an image that was never
        /// resized serializes exactly as it did before image layout existed.
        #[serde(default, skip_serializing_if = "ImageLayout::is_empty")]
        layout: ImageLayout,
    },
    /// A semantic divider in the document flow.  Unlike a paragraph border it
    /// has no text host, so ordinary paragraph editing cannot erase it.
    HorizontalRule,
    /// A generated navigation block. Its entries are derived from the current
    /// top-level headings at render/layout time; they are deliberately not
    /// copied into inline source state, where they would become stale after a
    /// concurrent heading edit.
    TableOfContents {
        /// The deepest heading level included by this table of contents.
        /// Kept on the atomic block so collaborators agree on its scope.
        max_level: u8,
    },
    /// A generated reference list. Its entries are derived from live citation
    /// groups and their referenced records at render/layout time; formatted
    /// entries are deliberately not copied into inline source state, where a
    /// style or metadata edit would leave them stale.
    Bibliography,
    PageBreak,
}

impl BlockKind {
    /// A table around `rows`, given one auto-width column per cell of the
    /// widest row and short rows padded out with empty cells, so the grid is
    /// rectangular by construction.
    ///
    /// This is the constructor for code that reads a table row by row and
    /// does not know the column count until the end — importers, mostly.
    pub fn table(mut rows: Vec<TableRow>) -> Self {
        let column_count = rows.iter().map(|row| row.cells.len()).max().unwrap_or(0);
        for row in &mut rows {
            while row.cells.len() < column_count {
                row.cells.push(TableCell::empty());
            }
        }
        BlockKind::Table {
            columns: (0..column_count).map(|_| TableColumn::auto()).collect(),
            properties: TableProperties::default(),
            rows,
        }
    }

    /// The list marker, when this is a list item.
    pub fn list_kind(&self) -> Option<ListKind> {
        match self {
            BlockKind::ListItem { kind, .. } => Some(*kind),
            _ => None,
        }
    }

    /// The list run this kind belongs to, when this is a list item.
    pub fn list_id(&self) -> Option<&StableId> {
        match self {
            BlockKind::ListItem { list_id, .. } => Some(list_id),
            _ => None,
        }
    }
}
