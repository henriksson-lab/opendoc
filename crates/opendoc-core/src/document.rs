//! The document root and the validation walk over its block tree.

use crate::annotation::{CommentThread, Suggestion};
use crate::block::{Block, BlockKind, Footnote};
use crate::citation::CitationDatabase;
use crate::ids::validate_stable_id;
use crate::ids::{DocumentUuid, HashRef, StableId};
use crate::inline::{Equation, Inline, Mark, MarkKind};
use crate::page::{validate_furniture_payload, HeaderFooterSlot, PageSetup};
use crate::table::validate_table_geometry;
use crate::warning::{ModelError, ModelWarning};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub uuid: DocumentUuid,
    pub title: String,
    pub locale: String,
    pub doi: Option<String>,
    /// The sheet every page of this document is laid out on. Document-level,
    /// not per-section: OpenDoc has no section model, so a document has one
    /// page geometry. See [`PageSetup`].
    #[serde(default)]
    pub page_setup: PageSetup,
    /// Blocks repeated at the top of every page. Page furniture, not body
    /// flow: it never appears in [`Document::blocks`] and never contributes
    /// to [`Document::visible_text`], but its block and inline ids share the
    /// document's id space so every block id stays globally addressable.
    #[serde(default)]
    pub header: Vec<Block>,
    /// Blocks repeated at the bottom of every page. See [`Document::header`].
    #[serde(default)]
    pub footer: Vec<Block>,
    pub blocks: Vec<Block>,
    pub footnotes: Vec<Footnote>,
    pub comments: Vec<CommentThread>,
    pub suggestions: Vec<Suggestion>,
    pub citation_database: CitationDatabase,
    pub warnings: Vec<ModelWarning>,
}

impl Document {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            uuid: DocumentUuid::new(),
            title: title.into(),
            locale: "en-US".to_string(),
            doi: None,
            page_setup: PageSetup::default(),
            header: Vec::new(),
            footer: Vec::new(),
            blocks: Vec::new(),
            footnotes: Vec::new(),
            comments: Vec::new(),
            suggestions: Vec::new(),
            citation_database: CitationDatabase::default(),
            warnings: Vec::new(),
        }
    }

    pub fn visible_text(&self) -> String {
        let mut out = String::new();
        for block in &self.blocks {
            block.push_visible_text(&self.citation_database, &mut out);
            if !out.ends_with('\n') {
                out.push('\n');
            }
        }
        out
    }

    /// The blocks occupying one page-furniture slot.
    pub fn furniture(&self, slot: HeaderFooterSlot) -> &[Block] {
        match slot {
            HeaderFooterSlot::Header => &self.header,
            HeaderFooterSlot::Footer => &self.footer,
        }
    }

    /// The blocks occupying one page-furniture slot, for replacement.
    pub fn furniture_mut(&mut self, slot: HeaderFooterSlot) -> &mut Vec<Block> {
        match slot {
            HeaderFooterSlot::Header => &mut self.header,
            HeaderFooterSlot::Footer => &mut self.footer,
        }
    }

    pub fn validate(&self) -> Result<(), ModelError> {
        self.uuid.validate()?;
        if self.title.trim().is_empty() {
            return Err(ModelError::InvalidDocument("title is empty"));
        }
        if self.title.trim() != self.title {
            return Err(ModelError::InvalidDocument(
                "title has surrounding whitespace",
            ));
        }
        if self.locale.trim().is_empty() {
            return Err(ModelError::InvalidDocument("document locale is empty"));
        }
        if self.locale.trim() != self.locale {
            return Err(ModelError::InvalidDocument(
                "document locale has surrounding whitespace",
            ));
        }
        if let Some(doi) = &self.doi {
            if doi.trim().is_empty() {
                return Err(ModelError::InvalidDocument("document DOI is empty"));
            }
            if doi.trim() != doi {
                return Err(ModelError::InvalidDocument(
                    "document DOI has surrounding whitespace",
                ));
            }
        }
        self.page_setup.validate()?;
        // Body, header and footer share one block/inline id space: a block id
        // is the document's addressing unit, so a header block that reused a
        // body block's id would make every id-keyed operation ambiguous.
        let mut block_ids = BTreeSet::new();
        let mut inline_ids = BTreeSet::new();
        validate_blocks(&self.blocks, &mut block_ids, &mut inline_ids)?;
        for slot in HeaderFooterSlot::ALL {
            let furniture = self.furniture(slot);
            validate_blocks(furniture, &mut block_ids, &mut inline_ids)?;
            validate_furniture_payload(furniture)?;
        }
        let mut comment_thread_ids = BTreeSet::new();
        for comment in &self.comments {
            if !comment_thread_ids.insert(comment.id.clone()) {
                return Err(ModelError::InvalidDocument("duplicate comment thread id"));
            }
            comment.validate()?;
        }
        let mut suggestion_ids = BTreeSet::new();
        for suggestion in &self.suggestions {
            if !suggestion_ids.insert(suggestion.id.clone()) {
                return Err(ModelError::InvalidDocument("duplicate suggestion id"));
            }
            suggestion.validate()?;
        }
        let mut footnote_ids = BTreeSet::new();
        for footnote in &self.footnotes {
            if !footnote_ids.insert(footnote.id.clone()) {
                return Err(ModelError::InvalidDocument("duplicate footnote id"));
            }
            footnote.validate()?;
        }
        let live_footnotes = self
            .footnotes
            .iter()
            .filter(|footnote| !footnote.deleted)
            .map(|footnote| footnote.id.clone())
            .collect::<BTreeSet<_>>();
        for footnote_id in footnote_reference_ids(&self.blocks) {
            if !live_footnotes.contains(&footnote_id) {
                return Err(ModelError::InvalidDocument(
                    "footnote reference target is missing",
                ));
            }
        }
        self.citation_database.validate(&live_footnotes)?;
        for warning in &self.warnings {
            warning.validate()?;
        }
        Ok(())
    }
}

pub(crate) fn validate_block_tree(blocks: &[Block]) -> Result<(), ModelError> {
    let mut block_ids = BTreeSet::new();
    let mut inline_ids = BTreeSet::new();
    validate_blocks(blocks, &mut block_ids, &mut inline_ids)
}

pub(crate) fn validate_blocks(
    blocks: &[Block],
    block_ids: &mut BTreeSet<StableId>,
    inline_ids: &mut BTreeSet<StableId>,
) -> Result<(), ModelError> {
    for block in blocks {
        validate_stable_id("block id", &block.id)?;
        if !block_ids.insert(block.id.clone()) {
            return Err(ModelError::InvalidDocument("duplicate block id"));
        }
        for inline in &block.content {
            let id = inline_stable_id(inline);
            validate_stable_id("inline id", id)?;
            if !inline_ids.insert(id.clone()) {
                return Err(ModelError::InvalidDocument("duplicate inline id"));
            }
            validate_inline(inline)?;
        }
        if let BlockKind::Table { columns, rows } = &block.kind {
            if rows.is_empty() {
                return Err(ModelError::InvalidDocument("table has no rows"));
            }
            if columns.is_empty() {
                return Err(ModelError::InvalidDocument("table has no columns"));
            }
            let mut row_ids = BTreeSet::new();
            for row in rows {
                validate_stable_id("table row id", &row.id)?;
                if !row_ids.insert(row.id.clone()) {
                    return Err(ModelError::InvalidDocument("duplicate table row id"));
                }
                if row.cells.is_empty() {
                    return Err(ModelError::InvalidDocument("table row has no cells"));
                }
                let mut cell_ids = BTreeSet::new();
                for cell in &row.cells {
                    validate_stable_id("table cell id", &cell.id)?;
                    if !cell_ids.insert(cell.id.clone()) {
                        return Err(ModelError::InvalidDocument("duplicate table cell id"));
                    }
                    if cell.blocks.is_empty() {
                        return Err(ModelError::InvalidDocument("table cell has no blocks"));
                    }
                    cell.properties.validate()?;
                    validate_blocks(&cell.blocks, block_ids, inline_ids)?;
                }
            }
            validate_table_geometry(columns, rows)?;
        }
        validate_block_payload(block)?;
    }
    Ok(())
}

pub(crate) fn validate_block_payload(block: &Block) -> Result<(), ModelError> {
    block.properties.validate()?;
    match &block.kind {
        BlockKind::Heading { level } if !(1..=6).contains(level) => Err(
            ModelError::InvalidDocument("heading level is outside 1..=6"),
        ),
        BlockKind::ListItem { level, .. } if *level > 8 => Err(ModelError::InvalidDocument(
            "list item level is outside 0..=8",
        )),
        BlockKind::ListItem { list_id, .. } => validate_stable_id("list id", list_id),
        BlockKind::EquationBlock { equation } => validate_equation(equation),
        BlockKind::Image {
            blob_hash, layout, ..
        } => {
            HashRef::parse(blob_hash)
                .map(|_| ())
                .map_err(|_| ModelError::InvalidDocument("image blob hash is invalid"))?;
            layout.validate()
        }
        _ => Ok(()),
    }
}

pub(crate) fn validate_inline(inline: &Inline) -> Result<(), ModelError> {
    match inline {
        Inline::Text { marks, .. } => validate_marks(marks),
        Inline::Link { href, marks, .. } => {
            validate_marks(marks)?;
            if href.trim().is_empty() {
                return Err(ModelError::InvalidDocument("link href is empty"));
            }
            Ok(())
        }
        Inline::Mention { label, .. } if label.trim().is_empty() => {
            Err(ModelError::InvalidDocument("mention label is empty"))
        }
        Inline::Equation { equation, .. } => validate_equation(equation),
        Inline::Citation { citation_id, .. } => validate_stable_id("citation id", citation_id),
        Inline::FootnoteRef { footnote_id, .. } => {
            validate_stable_id("footnote reference id", footnote_id)
        }
        _ => Ok(()),
    }
}

pub(crate) fn validate_inline_sequence(inlines: &[Inline]) -> Result<(), ModelError> {
    for inline in inlines {
        validate_inline(inline)?;
    }
    Ok(())
}

pub(crate) fn inline_sequence_is_empty_source_text(inlines: &[Inline]) -> bool {
    inlines.iter().all(|inline| match inline {
        Inline::Text { text, .. } | Inline::Link { text, .. } => text.trim().is_empty(),
        Inline::Mention { .. }
        | Inline::Equation { .. }
        | Inline::Citation { .. }
        | Inline::FootnoteRef { .. }
        | Inline::PageNumber { .. } => false,
    })
}

pub(crate) fn validate_equation(equation: &Equation) -> Result<(), ModelError> {
    validate_stable_id("equation id", &equation.id)?;
    if equation.source.trim().is_empty() {
        return Err(ModelError::InvalidDocument("equation source is empty"));
    }
    if equation.source.trim() != equation.source {
        return Err(ModelError::InvalidDocument(
            "equation source has surrounding whitespace",
        ));
    }
    Ok(())
}

pub(crate) fn validate_marks(marks: &[Mark]) -> Result<(), ModelError> {
    for mark in marks {
        let needs_value = matches!(
            mark.kind,
            MarkKind::Color | MarkKind::Background | MarkKind::Font | MarkKind::Size
        );
        match (&mark.value, needs_value) {
            (Some(value), true) if value.trim().is_empty() => {
                return Err(ModelError::InvalidDocument("mark value is empty"));
            }
            (None, true) => {
                return Err(ModelError::InvalidDocument("mark value is missing"));
            }
            (Some(_), false) => {
                return Err(ModelError::InvalidDocument("boolean mark has value"));
            }
            _ => {}
        }
    }
    Ok(())
}

pub(crate) fn inline_stable_id(inline: &Inline) -> &StableId {
    match inline {
        Inline::Text { id, .. }
        | Inline::Link { id, .. }
        | Inline::Citation { id, .. }
        | Inline::FootnoteRef { id, .. }
        | Inline::Mention { id, .. }
        | Inline::Equation { id, .. }
        | Inline::PageNumber { id, .. } => id,
    }
}

pub(crate) fn footnote_reference_ids(blocks: &[Block]) -> BTreeSet<StableId> {
    let mut ids = BTreeSet::new();
    collect_footnote_reference_ids(blocks, &mut ids);
    ids
}

pub(crate) fn collect_footnote_reference_ids(blocks: &[Block], ids: &mut BTreeSet<StableId>) {
    for block in blocks {
        for inline in &block.content {
            if let Inline::FootnoteRef { footnote_id, .. } = inline {
                ids.insert(footnote_id.clone());
            }
        }
        if let BlockKind::Table { rows, .. } = &block.kind {
            for row in rows {
                for cell in &row.cells {
                    collect_footnote_reference_ids(&cell.blocks, ids);
                }
            }
        }
    }
}
