//! Editor input model: the frontend reports a document selection plus a
//! browser `beforeinput` type and the Rust core decides which operations to
//! apply. Keeping this logic here means every shell (Tauri, WASM browser,
//! tests) gets identical editing semantics and the TypeScript layer only
//! maps DOM selections to [`opendoc_api::EditorPosition`]s.

use super::{
    list_run_split_operations, AppApiError, AppDocument, AppEditorSelection,
    EditorSelectionService, OpenDocApp,
};
use opendoc_api::{EditorInput, EditorMarkInput, EditorPosition, EditorSelection};
use opendoc_core::{
    digest_bytes, Block, BlockKind, BlockProperties, BlockProperty, ImageLayout, Inline,
    InsertPosition, StableId, TableCell, TableRow,
};
use opendoc_merge::{BlockTextStyle, OperationKind};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::ops::{Deref, DerefMut};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EditorResult {
    pub document: AppDocument,
    pub selection: EditorSelection,
    /// `false` when the gesture is not something the core edits (for
    /// example Backspace before an image); the frontend may then fall back
    /// to selecting the neighbouring block.
    pub handled: bool,
}

/// Linear character index space of one editable block. Text and link runs
/// contribute their scalar-value length; every other inline counts as one
/// atomic character.
#[derive(Clone, Debug)]
pub(crate) struct InlineSpan {
    pub(crate) id: StableId,
    pub(crate) start: usize,
    pub(crate) len: usize,
    pub(crate) editable: bool,
}

/// Where a block sits in the document tree, as the chain of indices that
/// reaches it from the top-level slice.
///
/// [`DocumentIndex`] flattens the tree, and every consumer of an entry
/// eventually needs the `Block` itself — its `content`, which the index only
/// summarises. Searching for it by id costs a walk of the whole tree, and the
/// callers that do it are inside loops over a *selection*, so the search ran
/// once per selected block: applying one mark across a 1,500-block document
/// scanned on the order of a million block nodes. The path makes the same
/// lookup cost the block's nesting depth, which is one step for a top-level
/// block and three for a block inside a table cell, independent of how large
/// the document or the selection is.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BlockPath {
    /// `(block index, row, cell)` for each enclosing table, outermost first.
    ancestors: Vec<(usize, usize, usize)>,
    /// Index of the block itself within the slice those steps reach.
    index: usize,
}

impl BlockPath {
    /// A path that deliberately addresses no block, for an index entry that
    /// describes a block the plan is going to *create*: it is not in the
    /// pre-edit snapshot paths resolve against. There is no `Default` for
    /// this type on purpose — an all-zero path would resolve to the first
    /// block of the document instead of failing.
    fn unresolved() -> Self {
        Self {
            ancestors: Vec::new(),
            index: usize::MAX,
        }
    }

    /// Number of block nodes a lookup through this path touches.
    fn steps(&self) -> usize {
        self.ancestors.len() + 1
    }

    /// The ancestor chain that addresses blocks inside one of this block's
    /// table cells.
    fn cell_ancestors(&self, row: usize, cell: usize) -> Vec<(usize, usize, usize)> {
        let mut ancestors = self.ancestors.clone();
        ancestors.push((self.index, row, cell));
        ancestors
    }

    /// Resolve against a block slice of the same shape the path was built
    /// from. `None` if the tree no longer has that shape.
    pub(crate) fn resolve<'a>(&self, blocks: &'a [Block]) -> Option<&'a Block> {
        crate::document_tree::record_block_lookup_visits(self.steps());
        let mut slice = blocks;
        for &(block, row, cell) in &self.ancestors {
            let BlockKind::Table { rows, .. } = &slice.get(block)?.kind else {
                return None;
            };
            slice = &rows.get(row)?.cells.get(cell)?.blocks;
        }
        slice.get(self.index)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct BlockEntry {
    pub(crate) id: StableId,
    pub(crate) path: BlockPath,
    kind: BlockKind,
    /// Editable text block (paragraph, heading, list item).
    pub(crate) text_block: bool,
    /// `true` for document-body blocks. Nested cell blocks use the same
    /// structural operations, but their container keeps joins local.
    top_level: bool,
    /// Identity of the container (top-level, or a table cell id) so ranges
    /// never cross containers.
    pub(crate) container: String,
    pub(crate) spans: Vec<InlineSpan>,
    pub(crate) len: usize,
}

pub(crate) struct DocumentIndex {
    pub(crate) blocks: Vec<BlockEntry>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Resolved {
    pub(crate) block: usize,
    pub(crate) abs: usize,
}

fn char_len(value: &str) -> usize {
    value.chars().count()
}

fn inline_text(inline: &Inline) -> Option<&str> {
    match inline {
        Inline::Text { text, .. } | Inline::Link { text, .. } => Some(text),
        _ => None,
    }
}

/// Selection gives each atomic inline one object position. Paste caret
/// arithmetic must use the same projection, not only visible text.
fn inline_width(inline: &Inline) -> usize {
    inline_text(inline).map(char_len).unwrap_or(1)
}

/// An atomic block that is *one object*: a caret can stand before or after it
/// and nowhere inside, so the only edit either delete key can make to it is to
/// remove it whole.
///
/// A table is deliberately not one of these even though it is not a text
/// block. The caret goes *into* a table — its cells hold ordinary blocks — so
/// there is a real place for Backspace to act, and treating the whole grid as
/// one object would let a keystroke at the start of the paragraph after it
/// throw away every cell. The match is exhaustive with no catch-all so a block
/// kind added later has to be classified rather than inheriting an answer.
fn is_atomic_object_block(kind: &BlockKind) -> bool {
    match kind {
        BlockKind::PageBreak
        | BlockKind::HorizontalRule
        | BlockKind::TableOfContents { .. }
        | BlockKind::Bibliography
        | BlockKind::Image { .. }
        | BlockKind::EquationBlock { .. } => true,
        BlockKind::Paragraph
        | BlockKind::Title
        | BlockKind::Subtitle
        | BlockKind::Heading { .. }
        | BlockKind::ListItem { .. }
        | BlockKind::Table { .. } => false,
    }
}

pub(crate) fn inline_stable_id(inline: &Inline) -> &StableId {
    match inline {
        Inline::Text { id, .. }
        | Inline::Link { id, .. }
        | Inline::Citation { id, .. }
        | Inline::FootnoteRef { id, .. }
        | Inline::Mention { id, .. }
        | Inline::GooglePersonChip { id, .. }
        | Inline::GoogleRichLinkChip { id, .. }
        | Inline::Dropdown { id, .. }
        | Inline::DateChip { id, .. }
        | Inline::Equation { id, .. }
        | Inline::PageNumber { id, .. } => id,
    }
}

impl DocumentIndex {
    pub(crate) fn build(blocks: &[Block]) -> Self {
        let mut index = Self { blocks: Vec::new() };
        index.push_blocks(blocks, true, "document");
        index
    }

    fn push_blocks(&mut self, blocks: &[Block], top_level: bool, container: &str) {
        self.push_blocks_at(blocks, top_level, container, &[]);
    }

    fn push_blocks_at(
        &mut self,
        blocks: &[Block],
        top_level: bool,
        container: &str,
        ancestors: &[(usize, usize, usize)],
    ) {
        for (block_index, block) in blocks.iter().enumerate() {
            let text_block = matches!(
                block.kind,
                BlockKind::Paragraph
                    | BlockKind::Title
                    | BlockKind::Subtitle
                    | BlockKind::Heading { .. }
                    | BlockKind::ListItem { .. }
            );
            let mut spans = Vec::new();
            let mut cursor = 0;
            for inline in &block.content {
                let (len, editable) = match inline_text(inline) {
                    Some(text) => (char_len(text), true),
                    None => (1, false),
                };
                spans.push(InlineSpan {
                    id: inline_stable_id(inline).clone(),
                    start: cursor,
                    len,
                    editable,
                });
                cursor += len;
            }
            let len = if text_block { cursor } else { 1 };
            let path = BlockPath {
                ancestors: ancestors.to_vec(),
                index: block_index,
            };
            self.blocks.push(BlockEntry {
                id: block.id.clone(),
                path: path.clone(),
                kind: block.kind.clone(),
                text_block,
                top_level,
                container: container.to_string(),
                spans,
                len,
            });
            if let BlockKind::Table { rows, .. } = &block.kind {
                for (row_index, row) in rows.iter().enumerate() {
                    for (cell_index, cell) in row.cells.iter().enumerate() {
                        let cell_ancestors = path.cell_ancestors(row_index, cell_index);
                        self.push_blocks_at(&cell.blocks, false, cell.id.as_str(), &cell_ancestors);
                    }
                }
            }
        }
    }

    pub(crate) fn block_index(&self, block_id: &str) -> Option<usize> {
        self.blocks
            .iter()
            .position(|entry| entry.id.as_str() == block_id)
    }

    pub(crate) fn resolve(&self, position: &EditorPosition) -> Result<Resolved, AppApiError> {
        let block = self.block_index(&position.block_id).ok_or_else(|| {
            AppApiError::NotFound(format!("block {} was not found", position.block_id))
        })?;
        let entry = &self.blocks[block];
        let abs = match &position.inline_id {
            Some(inline_id) => {
                let span = entry
                    .spans
                    .iter()
                    .find(|span| span.id.as_str() == inline_id)
                    .ok_or_else(|| {
                        AppApiError::NotFound(format!(
                            "inline {inline_id} was not found in block {}",
                            position.block_id
                        ))
                    })?;
                span.start + position.offset.min(span.len)
            }
            None => position.offset.min(entry.len),
        };
        Ok(Resolved { block, abs })
    }

    /// Convert a linear position back into an inline-relative position,
    /// preferring the end of the preceding editable run at boundaries so
    /// typed text inherits that run's marks (as Google Docs does).
    pub(crate) fn position(&self, resolved: Resolved) -> EditorPosition {
        let entry = &self.blocks[resolved.block];
        let abs = resolved.abs.min(entry.len);
        if !entry.text_block || entry.spans.is_empty() {
            return EditorPosition {
                block_id: entry.id.to_string(),
                inline_id: None,
                offset: abs.min(1),
            };
        }
        // First editable run whose range includes `abs` (so a boundary maps
        // to the end of the preceding run), else the atomic inline the
        // caret touches.
        let span = entry
            .spans
            .iter()
            .find(|span| span.editable && abs >= span.start && abs <= span.start + span.len)
            .or_else(|| {
                entry
                    .spans
                    .iter()
                    .find(|span| abs >= span.start && abs <= span.start + span.len)
            })
            .unwrap_or_else(|| entry.spans.last().expect("spans are non-empty"));
        EditorPosition {
            block_id: entry.id.to_string(),
            inline_id: Some(span.id.to_string()),
            offset: abs.saturating_sub(span.start).min(span.len),
        }
    }

    pub(crate) fn text_of(&self, blocks: &[Block], block: usize) -> String {
        let entry = &self.blocks[block];
        let mut out = String::new();
        if let Some(core) = entry.path.resolve(blocks) {
            for inline in &core.content {
                match inline_text(inline) {
                    Some(text) => out.push_str(text),
                    None => out.push('\u{FFFC}'),
                }
            }
        }
        out
    }
}

pub(crate) type PlannedOp = (&'static str, &'static str, OperationKind);

/// Grapheme-aware length of the cluster that ends at character `abs` of
/// `text` (used for Backspace), or that starts there (Delete).
fn grapheme_len_before(text: &str, abs: usize) -> usize {
    let mut count = 0;
    for grapheme in text.graphemes(true) {
        let len = char_len(grapheme);
        if count + len >= abs {
            return abs - count;
        }
        count += len;
    }
    0
}

fn grapheme_len_after(text: &str, abs: usize) -> usize {
    let mut count = 0;
    for grapheme in text.graphemes(true) {
        let len = char_len(grapheme);
        if count >= abs {
            return len;
        }
        count += len;
    }
    0
}

fn word_start_before(text: &str, abs: usize) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let mut index = abs.min(chars.len());
    while index > 0 && chars[index - 1].is_whitespace() {
        index -= 1;
    }
    let boundary_class = |ch: char| ch.is_alphanumeric() || ch == '_';
    if index > 0 {
        let class = boundary_class(chars[index - 1]);
        while index > 0
            && !chars[index - 1].is_whitespace()
            && boundary_class(chars[index - 1]) == class
        {
            index -= 1;
        }
    }
    index
}

fn word_end_after(text: &str, abs: usize) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let mut index = abs.min(chars.len());
    while index < chars.len() && chars[index].is_whitespace() {
        index += 1;
    }
    let boundary_class = |ch: char| ch.is_alphanumeric() || ch == '_';
    if index < chars.len() {
        let class = boundary_class(chars[index]);
        while index < chars.len()
            && !chars[index].is_whitespace()
            && boundary_class(chars[index]) == class
        {
            index += 1;
        }
    }
    index
}

impl OpenDocApp {
    pub fn describe_editor_selection(
        &self,
        selection: EditorSelection,
    ) -> Result<AppEditorSelection, AppApiError> {
        EditorSelectionService::new(self).describe(selection)
    }

    pub fn select_all_editor_content(&self) -> Result<EditorResult, AppApiError> {
        EditorSelectionService::new(self).select_all()
    }

    pub fn apply_editor_input(&mut self, input: EditorInput) -> Result<EditorResult, AppApiError> {
        EditorCommandService::new(self).apply_editor_input(input)
    }

    pub fn apply_editor_mark(
        &mut self,
        input: EditorMarkInput,
    ) -> Result<EditorResult, AppApiError> {
        EditorCommandService::new(self).apply_editor_mark(input)
    }

    pub fn insert_table_after_sized(
        &mut self,
        after_block_id: impl AsRef<str>,
        rows: usize,
        columns: usize,
    ) -> Result<AppDocument, AppApiError> {
        EditorCommandService::new(self).insert_table_after_sized(after_block_id, rows, columns)
    }
}

struct EditorCommandService<'a> {
    app: &'a mut OpenDocApp,
}

impl<'a> EditorCommandService<'a> {
    fn new(app: &'a mut OpenDocApp) -> Self {
        Self { app }
    }

    /// Apply one editing gesture. See [`EditorInput`].
    fn apply_editor_input(&mut self, input: EditorInput) -> Result<EditorResult, AppApiError> {
        let mut parsed_paste = if matches!(
            input.input_type.as_str(),
            "insertFromPaste" | "insertFromYank"
        ) {
            input.html.as_deref().map(clipboard_html::parse)
        } else {
            None
        };
        if let Some(paste) = &mut parsed_paste {
            self.materialize_pasted_images(&mut paste.blocks)?;
        }
        let index = DocumentIndex::build(&self.document.blocks);
        let anchor = index.resolve(&input.selection.anchor)?;
        let focus = index.resolve(&input.selection.focus)?;
        let (start, end) = if anchor <= focus {
            (anchor, focus)
        } else {
            (focus, anchor)
        };
        let collapsed = start == end;
        let mut plan = EditPlan::new(self, index);
        // `apply_batch` replaces the source document with its merged result,
        // so projection-only paste warnings must be appended afterwards or
        // they are silently discarded with the pre-batch document.
        let mut pending_paste_warning = None;
        let outcome = match input.input_type.as_str() {
            "insertText" | "insertReplacementText" | "insertCompositionText" | "insertFromDrop" => {
                let text = input.data.clone().unwrap_or_default();
                if text.is_empty() && collapsed {
                    return Ok(self.unhandled(input.selection));
                }
                plan.delete_range(start, end);
                let caret = plan.insert_text(start, &text);
                Some(caret)
            }
            "insertFromPaste" | "insertFromYank" => {
                // The clipboard's `text/html` flavour, when there is one and
                // it says more than the plain text does. It has been on the
                // wire since the frontend was written and nothing read it,
                // so every paste arrived as unformatted text.
                let parsed = parsed_paste;
                // An image-only or MathML-only hostile fragment may have no
                // prose blocks at all. It still needs its named degradation
                // warning even though the plain-text clipboard flavour is
                // what supplies the text we insert below.
                pending_paste_warning = parsed.as_ref().and_then(|paste| paste.warning);
                let pasted = parsed.filter(|paste| !paste.blocks.is_empty());
                let text = input.data.clone().unwrap_or_default();
                if text.is_empty() && pasted.is_none() && collapsed {
                    return Ok(self.unhandled(input.selection));
                }
                plan.delete_range(start, end);
                match pasted {
                    Some(paste) => Some(plan.insert_pasted_blocks(start, paste.blocks)),
                    None => Some(plan.insert_multiline(start, &text)),
                }
            }
            "insertLineBreak" => {
                plan.delete_range(start, end);
                Some(plan.insert_text(start, "\n"))
            }
            "insertParagraph" => {
                plan.delete_range(start, end);
                plan.split_block(start, false).map(|split| split.caret)
            }
            "deleteContentBackward" | "deleteByCut" | "deleteContent" | "deleteByDrag" => {
                if collapsed {
                    plan.delete_backward(start)
                } else {
                    plan.delete_range(start, end);
                    Some(start)
                }
            }
            "deleteContentForward" => {
                if collapsed {
                    plan.delete_forward(start)
                } else {
                    plan.delete_range(start, end);
                    Some(start)
                }
            }
            "deleteWordBackward" | "deleteSoftLineBackward" | "deleteHardLineBackward" => {
                if collapsed {
                    let entry = &plan.index.blocks[start.block];
                    if !entry.text_block || start.abs == 0 {
                        plan.delete_backward(start)
                    } else {
                        let text = plan.index.text_of(&plan.blocks, start.block);
                        let from = if input.input_type == "deleteWordBackward" {
                            word_start_before(&text, start.abs)
                        } else {
                            0
                        };
                        let from = Resolved {
                            block: start.block,
                            abs: from,
                        };
                        plan.delete_range(from, start);
                        Some(from)
                    }
                } else {
                    plan.delete_range(start, end);
                    Some(start)
                }
            }
            "deleteWordForward" | "deleteSoftLineForward" | "deleteHardLineForward" => {
                if collapsed {
                    let entry = &plan.index.blocks[start.block];
                    if !entry.text_block || start.abs >= entry.len {
                        plan.delete_forward(start)
                    } else {
                        let text = plan.index.text_of(&plan.blocks, start.block);
                        let to = if input.input_type == "deleteWordForward" {
                            word_end_after(&text, start.abs)
                        } else {
                            entry.len
                        };
                        let to = Resolved {
                            block: start.block,
                            abs: to,
                        };
                        plan.delete_range(start, to);
                        Some(start)
                    }
                } else {
                    plan.delete_range(start, end);
                    Some(start)
                }
            }
            _ => None,
        };
        let Some(caret) = outcome else {
            return Ok(self.unhandled(input.selection));
        };
        let (caret_block_id, caret_abs) = match plan.pending_caret.take() {
            Some((block_id, abs)) => (block_id, abs),
            None => (plan.index.blocks[caret.block].id.clone(), caret.abs),
        };
        let ops = plan.ops;
        if ops.is_empty() {
            return Ok(self.unhandled(input.selection));
        }
        self.apply_batch(ops)?;
        if let Some(warning) = pending_paste_warning {
            self.push_model_warning(warning.code, warning.message);
        }
        let document = self.document();
        let after = DocumentIndex::build(&self.document.blocks);
        let selection = match after.block_index(caret_block_id.as_str()) {
            Some(block) => EditorSelection::collapsed(after.position(Resolved {
                block,
                abs: caret_abs,
            })),
            None => input.selection,
        };
        Ok(EditorResult {
            document,
            selection,
            handled: true,
        })
    }

    fn unhandled(&self, selection: EditorSelection) -> EditorResult {
        EditorResult {
            document: self.document(),
            selection,
            handled: false,
        }
    }

    fn materialize_pasted_images(&mut self, blocks: &mut [PastedBlock]) -> Result<(), AppApiError> {
        for block in blocks {
            let PastedBlockKind::Image(image) = &mut block.kind else {
                continue;
            };
            if image.blob_hash.is_some() {
                continue;
            }
            let hash = digest_bytes("sha256", &image.bytes)
                .map_err(|error| AppApiError::Model(error.to_string()))?
                .to_string();
            self.app.add_binary_blob(
                image.name.clone(),
                image.media_type.clone(),
                image.bytes.clone(),
            )?;
            image.blob_hash = Some(hash);
        }
        Ok(())
    }
}

/// Accumulates operations against a snapshot of the pre-edit document. All
/// offsets are expressed against that snapshot; the operations are applied
/// together in one batch so intermediate states never need re-indexing.
pub(crate) struct EditPlan {
    blocks: Vec<Block>,
    pub(crate) index: DocumentIndex,
    pub(crate) ops: Vec<PlannedOp>,
    /// Caret expressed as (block id, linear offset) in post-edit
    /// coordinates when the target block did not exist before the edit.
    pending_caret: Option<(StableId, usize)>,
}

impl EditPlan {
    pub(crate) fn new(app: &OpenDocApp, index: DocumentIndex) -> Self {
        Self {
            blocks: app.document.blocks.clone(),
            index,
            ops: Vec::new(),
            pending_caret: None,
        }
    }

    fn block(&self, block: usize) -> &Block {
        self.index.blocks[block]
            .path
            .resolve(&self.blocks)
            .expect("indexed block exists")
    }

    /// Delete the characters in `[from, to)`.
    ///
    /// # Three cases, and why the third is not the second
    ///
    /// *Within one block* the characters go and nothing structural happens.
    ///
    /// *Across blocks of one container* — two paragraphs of the body, or two
    /// paragraphs **of one table cell** — the blocks between the ends are
    /// removed and the tail of the end block is joined into the start block.
    /// A cell is a container like any other, which is why this no longer asks
    /// whether the blocks are top level: that question refused a delete
    /// entirely inside one cell, silently, and a cell's blocks are ordinary
    /// blocks addressed by ordinary operations.
    ///
    /// *Across containers* — a selection running from the body into a cell,
    /// or out of one — deletes the text it covers and **changes no
    /// structure**: no block, row, cell or table is removed. That is a
    /// decision, not a shortcut. A table's geometry is identity-managed
    /// (ADR 0013, ADR 0019) and a selection that merely *ends* inside a table
    /// says nothing about which rows the user meant to lose; Word answers a
    /// selection like this the same way, by clearing the cells it crosses and
    /// leaving the grid standing. The alternative — inferring rows to delete
    /// from where a drag happened to stop — is how a table is lost by
    /// accident. What is no longer an option is the old answer, which was to
    /// do nothing at all and report success.
    pub(crate) fn delete_range(&mut self, from: Resolved, to: Resolved) {
        if to <= from {
            return;
        }
        if from.block == to.block {
            let entry = &self.index.blocks[from.block];
            if !entry.text_block {
                // A range over an atomic block is a selection *of* it — there
                // is nothing inside to delete part of, and `entry.len` is 1
                // precisely so "before it" and "after it" are the only two
                // positions. This is how a clicked image is deleted: the
                // editor selects the whole figure, so both delete keys arrive
                // here rather than at `delete_backward`/`delete_forward`.
                if is_atomic_object_block(&entry.kind) {
                    self.delete_block(from.block);
                }
                return;
            }
            self.delete_within_block(from.block, from.abs, to.abs, true);
            return;
        }
        let start_entry = &self.index.blocks[from.block];
        let end_entry = &self.index.blocks[to.block];
        if start_entry.container != end_entry.container {
            self.delete_across_containers(from, to);
            return;
        }
        let container = start_entry.container.clone();
        let start_len = start_entry.len;
        let start_is_text = start_entry.text_block;
        let end_is_text = end_entry.text_block;
        if start_is_text {
            self.delete_within_block(from.block, from.abs, start_len, true);
        }
        for middle in from.block + 1..to.block {
            // Siblings of the two ends, and only those: a block nested in a
            // table between them belongs to the table's own container and
            // goes with the table, not separately.
            if self.index.blocks[middle].container == container {
                self.delete_block(middle);
            }
        }
        if end_is_text {
            self.delete_within_block(to.block, 0, to.abs, false);
            if start_is_text {
                self.join_into(to.block, from.block, to.abs);
            }
        } else if to.abs > 0 {
            self.delete_block(to.block);
        }
        if !start_is_text {
            self.delete_block(from.block);
        }
    }

    /// Delete the text a range covers when its ends are in different
    /// containers, leaving every block, cell, row and table in place.
    ///
    /// Each block the range touches loses exactly the part of itself the
    /// range covers: the start block its tail, the end block its head, and
    /// every block between them all of its text. An atomic object block
    /// (a page break, an image, an equation block) wholly inside the range
    /// and at the top level is deleted, because there is no "part" of one to
    /// clear and leaving it behind would look like the delete had missed it.
    ///
    /// See [`EditPlan::delete_range`] for why nothing structural happens.
    fn delete_across_containers(&mut self, from: Resolved, to: Resolved) {
        let start_entry = self.index.blocks[from.block].clone();
        if start_entry.text_block {
            self.delete_within_block(from.block, from.abs, start_entry.len, true);
        }
        for middle in from.block + 1..to.block {
            let entry = self.index.blocks[middle].clone();
            if entry.text_block {
                self.delete_within_block(middle, 0, entry.len, true);
            } else if entry.top_level && is_atomic_object_block(&entry.kind) {
                self.delete_block(middle);
            }
        }
        let end_entry = self.index.blocks[to.block].clone();
        if end_entry.text_block {
            self.delete_within_block(to.block, 0, to.abs, false);
        }
    }

    /// Delete `[from, to)` inside one block. When `keep_first` is set the
    /// inline containing `from` is never removed outright so text can still
    /// be inserted into it afterwards.
    fn delete_within_block(&mut self, block: usize, from: usize, to: usize, keep_first: bool) {
        if to <= from {
            return;
        }
        let entry = self.index.blocks[block].clone();
        if !entry.text_block {
            return;
        }
        let mut kept_one = false;
        for span in &entry.spans {
            let span_end = span.start + span.len;
            if span_end <= from || span.start >= to {
                continue;
            }
            let local_from = from.saturating_sub(span.start);
            let local_to = to.min(span_end) - span.start;
            let covers_all = local_from == 0 && local_to == span.len;
            if !span.editable {
                self.ops.push((
                    "delete-inline",
                    "delete inline",
                    OperationKind::DeleteInline {
                        inline_id: span.id.clone(),
                    },
                ));
                continue;
            }
            let must_keep = (keep_first && !kept_one) || entry.spans.len() == 1;
            kept_one = true;
            if covers_all && !must_keep {
                self.ops.push((
                    "delete-inline",
                    "delete inline",
                    OperationKind::DeleteInline {
                        inline_id: span.id.clone(),
                    },
                ));
            } else if local_to > local_from {
                self.ops.push((
                    "delete-text",
                    "delete text",
                    OperationKind::DeleteText {
                        inline_id: span.id.clone(),
                        start: local_from,
                        end: local_to,
                    },
                ));
            }
        }
    }

    fn delete_block(&mut self, block: usize) {
        let id = self.index.blocks[block].id.clone();
        self.ops.push((
            "delete-block",
            "delete block",
            OperationKind::DeleteBlock { block_id: id },
        ));
    }

    /// Move every inline of `source` that starts at or after `from_abs`
    /// (in pre-edit coordinates) to the end of `target`, then delete
    /// `source`.
    fn join_into(&mut self, source: usize, target: usize, from_abs: usize) {
        let source_entry = self.index.blocks[source].clone();
        let target_entry = self.index.blocks[target].clone();
        let mut after = target_entry.spans.last().map(|span| span.id.clone());
        for span in &source_entry.spans {
            // Spans that ended before the cut were deleted (or emptied) by
            // delete_within_block; only spans with a remaining tail move.
            if span.start + span.len <= from_abs {
                continue;
            }
            self.ops.push((
                "move-inline",
                "join block into previous",
                OperationKind::MoveInlineToBlock {
                    inline_id: span.id.clone(),
                    target_block_id: target_entry.id.clone(),
                    position: InsertPosition::after_or_last(after.clone()),
                },
            ));
            after = Some(span.id.clone());
        }
        self.delete_block(source);
    }

    /// Insert `text` at `at` and return the caret position after it.
    pub(crate) fn insert_text(&mut self, at: Resolved, text: &str) -> Resolved {
        if text.is_empty() {
            return at;
        }
        let entry = self.index.blocks[at.block].clone();
        let inserted = char_len(text);
        if !entry.text_block {
            return at;
        }
        // Prefer the editable span that ends at `abs` (so the new text takes
        // that run's marks), else the one containing it.
        let mut target: Option<&InlineSpan> = None;
        for span in &entry.spans {
            let end = span.start + span.len;
            if span.editable && at.abs >= span.start && at.abs <= end {
                target = Some(span);
                if at.abs < end {
                    break;
                }
            }
        }
        match target {
            Some(span) => {
                self.ops.push((
                    "insert-text",
                    "insert text",
                    OperationKind::InsertText {
                        inline_id: span.id.clone(),
                        offset: at.abs - span.start,
                        text: text.to_string(),
                    },
                ));
            }
            None => {
                // No editable run touches the caret: create one after the
                // atomic inline that precedes the caret (or at the start).
                let after = entry
                    .spans
                    .iter()
                    .rfind(|span| span.start + span.len <= at.abs)
                    .map(|span| span.id.clone());
                self.ops.push((
                    "insert-inline",
                    "insert text run",
                    OperationKind::InsertInline {
                        block_id: entry.id.clone(),
                        position: InsertPosition::after_or_last(after),
                        inline: Inline::text(text),
                    },
                ));
            }
        }
        Resolved {
            block: at.block,
            abs: at.abs + inserted,
        }
    }

    /// Paste: the first line goes at the caret and each further line becomes
    /// a new sibling block, whether that sibling lives in the body or a cell.
    fn insert_multiline(&mut self, at: Resolved, text: &str) -> Resolved {
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        let entry = self.index.blocks[at.block].clone();
        if !entry.text_block || !normalized.contains('\n') {
            return self.insert_text(at, &normalized);
        }
        let mut lines = normalized.split('\n');
        let first = lines.next().unwrap_or_default();
        // Split first so the original tail ends up after every pasted line,
        // then fill the original block, chain middle lines as new blocks
        // after it, and prepend the last line to the tail block.
        let split = self.split_block(at, true);
        let caret = self.insert_text(at, first);
        let Some(split) = split else {
            let rest: Vec<&str> = lines.collect();
            if rest.is_empty() {
                return caret;
            }
            return self.insert_text(caret, &format!("\n{}", rest.join("\n")));
        };
        let rest: Vec<&str> = lines.collect();
        let (last, middle) = rest.split_last().expect("at least one more line");
        let mut previous_block_id = entry.id.clone();
        for line in middle {
            let block_id = StableId::new("block");
            let mut block = Block::paragraph(*line);
            block.id = block_id.clone();
            if let BlockKind::ListItem { .. } = entry.kind {
                block.kind = entry.kind.clone();
            }
            self.ops.push((
                "insert-block",
                "paste paragraph",
                OperationKind::InsertBlock {
                    position: InsertPosition::After(previous_block_id.clone()),
                    block,
                },
            ));
            previous_block_id = block_id;
        }
        if !last.is_empty() {
            self.ops.push((
                "insert-text",
                "paste last line",
                OperationKind::InsertText {
                    inline_id: split.first_inline_id.clone(),
                    offset: 0,
                    text: last.to_string(),
                },
            ));
        }
        self.pending_caret = Some((split.block_id.clone(), char_len(last)));
        caret
    }

    /// Paste with formatting: the first pasted paragraph goes at the caret,
    /// each further one becomes a new block after it, and every run keeps the
    /// marks the clipboard gave it.
    ///
    /// This is [`EditPlan::insert_multiline`] with runs instead of lines, and
    /// it is deliberately built the same way — split first, fill the original
    /// block, chain the middle paragraphs, prepend the last into the tail —
    /// so a rich paste and a plain one leave the same structure behind.
    fn insert_pasted_blocks(&mut self, at: Resolved, blocks: Vec<PastedBlock>) -> Resolved {
        if let [PastedBlock {
            kind: PastedBlockKind::Table { rows },
            ..
        }] = blocks.as_slice()
        {
            return self.insert_pasted_table(at, rows);
        }
        let entry = self.index.blocks[at.block].clone();
        if !entry.text_block {
            // Non-text blocks have no structural caret position, so preserve
            // the paste as text in their only editable representation.
            let mut caret = at;
            for (index, block) in blocks.iter().enumerate() {
                if index > 0 {
                    caret = self.insert_text(caret, "\n");
                }
                caret = self.insert_runs(caret, &block.runs);
            }
            return caret;
        }
        let mut blocks = blocks;
        // Images are blocks in the durable model.  A leading or trailing
        // image needs an empty prose block on that side so the ordinary rich
        // paste split can put it between its surrounding source text rather
        // than losing it as the special first/final run insertion.
        if is_atomic_pasted_block(blocks.first()) {
            blocks.insert(0, PastedBlock::default());
        }
        if is_atomic_pasted_block(blocks.last()) {
            blocks.push(PastedBlock::default());
        }
        // `<ol start>` and `<ol type>` are list-run settings, not item
        // styling.  Keep this clipboard-local map separate from the item
        // blocks so one operation is emitted for each pasted wrapper.
        let pasted_list_settings = pasted_list_settings(&blocks);
        let mut pasted_lists = BTreeMap::new();
        let first = if blocks.is_empty() {
            PastedBlock::default()
        } else {
            blocks.remove(0)
        };
        if entry.len == 0 {
            if let Some(style) = pasted_block_style(&first, &mut pasted_lists) {
                self.ops.push((
                    "set-block-text-style",
                    "paste block style",
                    OperationKind::SetBlockTextStyle {
                        block_id: entry.id.clone(),
                        style,
                    },
                ));
            }
            append_pasted_block_properties(&mut self.ops, &entry.id, &first.properties);
        }
        if blocks.is_empty() {
            let caret = self.insert_runs(at, &first.runs);
            append_pasted_list_settings(&mut self.ops, &pasted_lists, &pasted_list_settings);
            return caret;
        }
        let split = self.split_block(at, true);
        let caret = self.insert_runs(at, &first.runs);
        let Some(split) = split else {
            let mut caret = caret;
            for block in &blocks {
                caret = self.insert_text(caret, "\n");
                caret = self.insert_runs(caret, &block.runs);
            }
            append_pasted_list_settings(&mut self.ops, &pasted_lists, &pasted_list_settings);
            return caret;
        };
        let last = blocks.pop().expect("at least one block past the first");
        let mut previous_block_id = entry.id.clone();
        for block in blocks {
            let mut new_block = if is_atomic_pasted_block(Some(&block)) {
                pasted_block_to_block(&block)
            } else {
                Block::paragraph("")
            };
            let block_id = new_block.id.clone();
            if let Some(style) = pasted_block_style(&block, &mut pasted_lists) {
                new_block.kind = block_kind_from_style(style);
            } else if !is_atomic_pasted_block(Some(&block))
                && matches!(entry.kind, BlockKind::ListItem { .. })
            {
                new_block.kind = entry.kind.clone();
            }
            if !is_atomic_pasted_block(Some(&block)) {
                new_block.content = block.runs;
            }
            new_block.properties = block.properties.clone();
            self.ops.push((
                "insert-block",
                "paste paragraph",
                OperationKind::InsertBlock {
                    position: InsertPosition::After(previous_block_id.clone()),
                    block: new_block,
                },
            ));
            previous_block_id = block_id;
        }
        // At the end of an otherwise empty tail, the split block is wholly
        // owned by the final pasted block, so retaining its structural style
        // does not restyle source prose that merely followed the caret.
        if at.abs == entry.len {
            if let Some(style) = pasted_block_style(&last, &mut pasted_lists) {
                self.ops.push((
                    "set-block-text-style",
                    "paste final block style",
                    OperationKind::SetBlockTextStyle {
                        block_id: split.block_id.clone(),
                        style,
                    },
                ));
            }
            append_pasted_block_properties(&mut self.ops, &split.block_id, &last.properties);
        }
        // The tail block already holds what was after the caret; the last
        // pasted paragraph goes in front of it.
        let mut after: Option<StableId> = None;
        let mut trailing = 0usize;
        for inline in last.runs {
            trailing += inline_width(&inline);
            let inline_id = inline_stable_id(&inline).clone();
            self.ops.push((
                "insert-inline",
                "paste last paragraph",
                OperationKind::InsertInline {
                    block_id: split.block_id.clone(),
                    position: match &after {
                        Some(previous) => InsertPosition::After(previous.clone()),
                        None => InsertPosition::First,
                    },
                    inline,
                },
            ));
            after = Some(inline_id);
        }
        self.pending_caret = Some((split.block_id.clone(), trailing));
        append_pasted_list_settings(&mut self.ops, &pasted_lists, &pasted_list_settings);
        caret
    }

    /// Put a standalone clipboard table immediately after the block holding
    /// the paste caret.  Table cells are real document subtrees, not HTML
    /// retained for later rendering.  Keeping the caret in the first cell
    /// makes the newly pasted grid immediately editable.
    fn insert_pasted_table(
        &mut self,
        at: Resolved,
        rows: &[clipboard_html::PastedTableRow],
    ) -> Resolved {
        let entry = self.index.blocks[at.block].clone();
        let table_id = StableId::new("block");
        let mut first_cell = None;
        let rows = rows
            .iter()
            .map(|row| {
                let cells = row
                    .cells
                    .iter()
                    .map(|blocks| {
                        let blocks = blocks.iter().map(pasted_block_to_block).collect::<Vec<_>>();
                        let cell = TableCell::new(if blocks.is_empty() {
                            vec![Block::paragraph("")]
                        } else {
                            blocks
                        });
                        if first_cell.is_none() {
                            first_cell = Some(cell.blocks[0].id.clone());
                        }
                        cell
                    })
                    .collect();
                TableRow {
                    id: StableId::new("row"),
                    height: None,
                    header: row.header,
                    cells,
                }
            })
            .collect();
        self.ops.push((
            "insert-block",
            "paste table",
            OperationKind::InsertBlock {
                position: InsertPosition::After(entry.id),
                block: Block {
                    id: table_id,
                    kind: BlockKind::table(rows),
                    content: Vec::new(),
                    properties: BlockProperties::default(),
                },
            },
        ));
        if let Some(cell) = first_cell {
            self.pending_caret = Some((cell, 0));
        }
        at
    }

    /// Inserts pasted runs at `at`, each as its own inline so its marks are
    /// its own, and returns the caret after the last of them.
    fn insert_runs(&mut self, at: Resolved, runs: &[Inline]) -> Resolved {
        let entry = self.index.blocks[at.block].clone();
        if !entry.text_block {
            return at;
        }
        // An unmarked single run is ordinary typed text, and goes through the
        // ordinary path so it joins the run the caret is in rather than
        // splitting it in two for nothing.
        if let [Inline::Text { text, marks, .. }] = runs {
            if marks.is_empty() {
                return self.insert_text(at, text);
            }
        }
        let mut after = entry
            .spans
            .iter()
            .rfind(|span| span.start + span.len <= at.abs)
            .map(|span| span.id.clone());
        let mut inserted = 0usize;
        for run in runs {
            inserted += inline_width(run);
            let run = run.clone();
            let inline_id = inline_stable_id(&run).clone();
            self.ops.push((
                "insert-inline",
                "paste run",
                OperationKind::InsertInline {
                    block_id: entry.id.clone(),
                    // A missing predecessor normally means append, but at
                    // the true start boundary it means the pasted atomic
                    // object belongs before the source text.
                    position: match &after {
                        Some(previous) => InsertPosition::After(previous.clone()),
                        None if at.abs == 0 => InsertPosition::First,
                        None => InsertPosition::Last,
                    },
                    inline: run,
                },
            ));
            after = Some(inline_id);
        }
        Resolved {
            block: at.block,
            abs: at.abs + inserted,
        }
    }

    /// Enter: split the block at `at`. With `keep_placeholder` the new
    /// block always starts with an editable run (possibly empty) so callers
    /// can insert text at its start.
    /// A list item that just became a paragraph cuts its run in two. The
    /// items after it must move to a fresh run, or numbering keeps counting
    /// across the paragraph now separating the halves.
    fn split_list_run_after_leaving(&mut self, block_id: &StableId) {
        let leaving = BTreeSet::from([block_id.clone()]);
        let operations = list_run_split_operations(&self.blocks, &leaving);
        self.ops.extend(operations);
    }

    fn split_block(&mut self, at: Resolved, keep_placeholder: bool) -> Option<SplitOutcome> {
        let entry = self.index.blocks[at.block].clone();
        if !entry.text_block {
            return None;
        }
        let block = self.block(at.block).clone();
        let at_end = at.abs >= entry.len;
        let list_empty = matches!(block.kind, BlockKind::ListItem { .. }) && entry.len == 0;
        if list_empty {
            self.ops.push((
                "set-block-text-style",
                "leave list",
                OperationKind::SetBlockTextStyle {
                    block_id: block.id.clone(),
                    style: BlockTextStyle::Paragraph,
                },
            ));
            self.split_list_run_after_leaving(&block.id);
            let first_inline_id = entry
                .spans
                .first()
                .map(|span| span.id.clone())
                .unwrap_or_else(|| StableId::new("text"));
            return Some(SplitOutcome {
                caret: at,
                block_id: block.id.clone(),
                first_inline_id,
            });
        }
        let new_kind = match &block.kind {
            BlockKind::Title | BlockKind::Subtitle | BlockKind::Heading { .. } if at_end => {
                BlockKind::Paragraph
            }
            other => other.clone(),
        };
        let new_block_id = StableId::new("block");
        // Split point inside a run: trim the run and carry the tail over.
        let mut tail_inline: Option<Inline> = None;
        let mut moved: Vec<StableId> = Vec::new();
        for (span, inline) in entry.spans.iter().zip(block.content.iter()) {
            let end = span.start + span.len;
            if end <= at.abs && !(span.editable && span.start == at.abs) {
                continue;
            }
            if span.editable && span.start <= at.abs && at.abs < end {
                // The run containing the cut stays in place (trimmed) so the
                // caller can still insert into it; its tail is copied over.
                let text = inline_text(inline).unwrap_or_default();
                let split = opendoc_merge::byte_index_for_char_offset(text, at.abs - span.start);
                let tail = text[split..].to_string();
                if span.len > at.abs - span.start {
                    self.ops.push((
                        "delete-text",
                        "split run",
                        OperationKind::DeleteText {
                            inline_id: span.id.clone(),
                            start: at.abs - span.start,
                            end: span.len,
                        },
                    ));
                }
                tail_inline = Some(match inline {
                    Inline::Link { href, marks, .. } => Inline::Link {
                        id: StableId::new("link"),
                        text: tail,
                        href: href.clone(),
                        marks: marks.clone(),
                    },
                    Inline::Text { marks, .. } => Inline::Text {
                        id: StableId::new("text"),
                        text: tail,
                        marks: marks.clone(),
                    },
                    _ => Inline::text(tail),
                });
                continue;
            }
            if span.start >= at.abs {
                moved.push(span.id.clone());
            }
        }
        let first_inline = tail_inline.unwrap_or_else(|| Inline::text(""));
        let first_inline_id = inline_stable_id(&first_inline).clone();
        let new_block = Block {
            id: new_block_id.clone(),
            kind: new_kind,
            content: vec![first_inline],
            properties: BlockProperties::default(),
        };
        self.ops.push((
            "insert-block",
            "split paragraph",
            OperationKind::InsertBlock {
                position: InsertPosition::After(block.id.clone()),
                block: new_block,
            },
        ));
        let mut after = Some(first_inline_id.clone());
        let move_count = moved.len();
        for inline_id in moved {
            self.ops.push((
                "move-inline",
                "carry inline to split block",
                OperationKind::MoveInlineToBlock {
                    inline_id: inline_id.clone(),
                    target_block_id: new_block_id.clone(),
                    position: InsertPosition::after_or_last(after.clone()),
                },
            ));
            after = Some(inline_id);
        }
        if !keep_placeholder && move_count > 0 && first_inline_is_empty(&self.ops, &first_inline_id)
        {
            self.ops.push((
                "delete-inline",
                "drop empty split run",
                OperationKind::DeleteInline {
                    inline_id: first_inline_id.clone(),
                },
            ));
        }
        // The new block is not in the pre-edit index; register a synthetic
        // entry so the caller can address it.
        self.index.blocks.push(BlockEntry {
            id: new_block_id.clone(),
            path: BlockPath::unresolved(),
            kind: entry.kind.clone(),
            text_block: true,
            top_level: entry.top_level,
            container: entry.container.clone(),
            spans: Vec::new(),
            len: 0,
        });
        Some(SplitOutcome {
            caret: Resolved {
                block: self.index.blocks.len() - 1,
                abs: 0,
            },
            block_id: new_block_id,
            first_inline_id,
        })
    }

    /// Backspace with a collapsed caret.
    fn delete_backward(&mut self, at: Resolved) -> Option<Resolved> {
        let entry = self.index.blocks[at.block].clone();
        if !entry.text_block {
            // Caret on an atomic block: delete it when the caret sits after it.
            if at.abs > 0 && is_atomic_object_block(&entry.kind) {
                self.delete_block(at.block);
                return Some(self.caret_before_block(at.block));
            }
            return None;
        }
        if at.abs > 0 {
            let text = self.index.text_of(&self.blocks, at.block);
            let mut len = grapheme_len_before(&text, at.abs).max(1);
            // Atomic inline: delete it whole.
            if let Some(span) = entry
                .spans
                .iter()
                .find(|span| !span.editable && span.start + span.len == at.abs)
            {
                len = span.len;
            }
            let from = Resolved {
                block: at.block,
                abs: at.abs - len,
            };
            self.delete_within_block(at.block, from.abs, at.abs, true);
            return Some(from);
        }
        // Caret at block start.
        if let BlockKind::ListItem { .. } = entry.kind {
            self.ops.push((
                "set-block-text-style",
                "remove list bullet",
                OperationKind::SetBlockTextStyle {
                    block_id: entry.id.clone(),
                    style: BlockTextStyle::Paragraph,
                },
            ));
            self.split_list_run_after_leaving(&entry.id);
            return Some(at);
        }
        let previous = self.previous_sibling(at.block)?;
        let previous_entry = self.index.blocks[previous].clone();
        if previous_entry.text_block {
            let caret = Resolved {
                block: previous,
                abs: previous_entry.len,
            };
            if entry.len == 0 && entry.spans.iter().all(|span| span.editable) {
                self.delete_block(at.block);
            } else {
                self.join_into(at.block, previous, 0);
            }
            return Some(caret);
        }
        // The previous block holds no text the caret can join: it is one
        // object. Backspace removes it, which is the only gesture that can —
        // there is nowhere inside it for the caret to stand in some browsers,
        // and a document whose picture cannot be deleted is worse than one
        // that loses a picture to a keystroke it can also undo.
        if is_atomic_object_block(&previous_entry.kind) {
            self.delete_block(previous);
            return Some(at);
        }
        None
    }

    /// Delete with a collapsed caret.
    fn delete_forward(&mut self, at: Resolved) -> Option<Resolved> {
        let entry = self.index.blocks[at.block].clone();
        if !entry.text_block {
            if at.abs == 0 && is_atomic_object_block(&entry.kind) {
                self.delete_block(at.block);
                return Some(self.caret_before_block(at.block));
            }
            return None;
        }
        if at.abs < entry.len {
            let text = self.index.text_of(&self.blocks, at.block);
            let mut len = grapheme_len_after(&text, at.abs).max(1);
            if let Some(span) = entry
                .spans
                .iter()
                .find(|span| !span.editable && span.start == at.abs)
            {
                len = span.len;
            }
            self.delete_within_block(at.block, at.abs, at.abs + len, true);
            return Some(at);
        }
        let next = self.next_sibling(at.block)?;
        let next_entry = self.index.blocks[next].clone();
        if next_entry.text_block {
            if next_entry.len == 0 {
                self.delete_block(next);
            } else {
                self.join_into(next, at.block, 0);
            }
            return Some(at);
        }
        // The mirror of Backspace's last case: the next block is one object
        // with no text to join, so Delete removes it.
        if is_atomic_object_block(&next_entry.kind) {
            self.delete_block(next);
            return Some(at);
        }
        None
    }

    fn previous_sibling(&self, block: usize) -> Option<usize> {
        let container = &self.index.blocks[block].container;
        (0..block)
            .rev()
            .find(|&candidate| &self.index.blocks[candidate].container == container)
    }

    fn next_sibling(&self, block: usize) -> Option<usize> {
        let container = &self.index.blocks[block].container;
        (block + 1..self.index.blocks.len())
            .find(|&candidate| &self.index.blocks[candidate].container == container)
    }

    fn caret_before_block(&self, block: usize) -> Resolved {
        match self.previous_sibling(block) {
            Some(previous) => Resolved {
                block: previous,
                abs: self.index.blocks[previous].len,
            },
            None => match self.next_sibling(block) {
                Some(next) => Resolved {
                    block: next,
                    abs: 0,
                },
                None => Resolved { block, abs: 0 },
            },
        }
    }
}

struct SplitOutcome {
    caret: Resolved,
    block_id: StableId,
    first_inline_id: StableId,
}

fn first_inline_is_empty(ops: &[PlannedOp], inline_id: &StableId) -> bool {
    ops.iter().any(|(_, _, kind)| match kind {
        OperationKind::InsertBlock { block, .. } => block
            .content
            .first()
            .map(|inline| {
                inline_stable_id(inline) == inline_id
                    && inline_text(inline).map(str::is_empty).unwrap_or(false)
            })
            .unwrap_or(false),
        _ => false,
    })
}

// ---- Marks and structural helpers ------------------------------------------

fn mark_kind_from_label(label: &str) -> Option<opendoc_core::MarkKind> {
    use opendoc_core::MarkKind;
    Some(match label {
        "bold" => MarkKind::Bold,
        "italic" => MarkKind::Italic,
        "underline" => MarkKind::Underline,
        "strike" | "strikethrough" => MarkKind::Strike,
        "code" => MarkKind::Code,
        "superscript" => MarkKind::Superscript,
        "subscript" => MarkKind::Subscript,
        "color" => MarkKind::Color,
        "background" | "highlight" => MarkKind::Background,
        "font" => MarkKind::Font,
        "size" => MarkKind::Size,
        "link" => MarkKind::Link,
        _ => return None,
    })
}

impl EditorCommandService<'_> {
    pub fn apply_editor_mark(
        &mut self,
        input: EditorMarkInput,
    ) -> Result<EditorResult, AppApiError> {
        use opendoc_core::{Mark, MarkExpand, MarkKind};
        let index = DocumentIndex::build(&self.document.blocks);
        let anchor = index.resolve(&input.selection.anchor)?;
        let focus = index.resolve(&input.selection.focus)?;
        let (start, end) = if anchor <= focus {
            (anchor, focus)
        } else {
            (focus, anchor)
        };
        if start == end {
            return Ok(self.unhandled(input.selection));
        }
        let action = input.action.as_deref().unwrap_or("toggle");
        let remove_all = input.mark_kind == "all";
        let kind = if remove_all {
            None
        } else {
            Some(mark_kind_from_label(&input.mark_kind).ok_or_else(|| {
                AppApiError::Format(format!("unknown mark kind {}", input.mark_kind))
            })?)
        };

        // Pass 1: split runs at the selection boundaries so every affected
        // run lies entirely inside the selection.
        //
        // Each boundary pair carries **the index of the block it belongs to**.
        // A selection routinely spans blocks that hold no text — a page break,
        // an image, an equation block, a table — and those contribute no pair,
        // so reading the pairs back positionally in pass 2 would hand every
        // text block after the first non-text one somebody else's boundaries
        // and drop the tail of the selection entirely. Splitting still
        // happened, so the document also gained split runs carrying no mark.
        let mut split_ops: Vec<PlannedOp> = Vec::new();
        let mut boundaries: Vec<(usize, usize, usize)> = Vec::new();
        for block in start.block..=end.block {
            let entry = &index.blocks[block];
            if !entry.text_block {
                continue;
            }
            let from = if block == start.block { start.abs } else { 0 };
            let to = if block == end.block {
                end.abs
            } else {
                entry.len
            };
            boundaries.push((block, from, to));
            let core_block = entry
                .path
                .resolve(&self.document.blocks)
                .expect("indexed block");
            for (span, inline) in entry.spans.iter().zip(core_block.content.iter()) {
                if !span.editable {
                    continue;
                }
                let span_end = span.start + span.len;
                if span_end <= from || span.start >= to {
                    continue;
                }
                let text = inline_text(inline).unwrap_or_default();
                let (id, marks, href) = match inline {
                    Inline::Text { id, marks, .. } => (id.clone(), marks.clone(), None),
                    Inline::Link {
                        id, marks, href, ..
                    } => (id.clone(), marks.clone(), Some(href.clone())),
                    _ => continue,
                };
                let local_from = from.saturating_sub(span.start).min(span.len);
                let local_to = to.min(span_end) - span.start;
                if local_from == 0 && local_to == span.len {
                    continue;
                }
                // Keep the head in place, insert the middle and tail as new runs.
                let head_end = opendoc_merge::byte_index_for_char_offset(text, local_from);
                let mid_end = opendoc_merge::byte_index_for_char_offset(text, local_to);
                let make = |content: &str| match &href {
                    Some(href) => Inline::Link {
                        id: StableId::new("link"),
                        text: content.to_string(),
                        href: href.clone(),
                        marks: marks.clone(),
                    },
                    None => Inline::Text {
                        id: StableId::new("text"),
                        text: content.to_string(),
                        marks: marks.clone(),
                    },
                };
                let mut after = id.clone();
                let mut pieces: Vec<Inline> = Vec::new();
                if local_from > 0 {
                    pieces.push(make(&text[head_end..mid_end]));
                    if local_to < span.len {
                        pieces.push(make(&text[mid_end..]));
                    }
                    split_ops.push((
                        "delete-text",
                        "split run for formatting",
                        OperationKind::DeleteText {
                            inline_id: id.clone(),
                            start: local_from,
                            end: span.len,
                        },
                    ));
                } else {
                    // local_from == 0, local_to < len: head is the selected part.
                    pieces.push(make(&text[mid_end..]));
                    split_ops.push((
                        "delete-text",
                        "split run for formatting",
                        OperationKind::DeleteText {
                            inline_id: id.clone(),
                            start: local_to,
                            end: span.len,
                        },
                    ));
                }
                for piece in pieces {
                    let piece_id = inline_stable_id(&piece).clone();
                    split_ops.push((
                        "insert-inline",
                        "split run for formatting",
                        OperationKind::InsertInline {
                            block_id: entry.id.clone(),
                            position: InsertPosition::After(after.clone()),
                            inline: piece,
                        },
                    ));
                    after = piece_id;
                }
            }
        }
        if !split_ops.is_empty() {
            self.apply_batch(split_ops)?;
        }

        // Pass 2: the runs now inside the selection.
        //
        // Splitting a run changes no block's identity, position or character
        // count, so the block indices pass 1 recorded still address the same
        // blocks in the rebuilt index.
        let index = DocumentIndex::build(&self.document.blocks);
        let mut targets: Vec<(StableId, Vec<Mark>, Option<String>, String)> = Vec::new();
        for &(block, from, to) in &boundaries {
            let entry = &index.blocks[block];
            let core_block = entry
                .path
                .resolve(&self.document.blocks)
                .expect("indexed block");
            for (span, inline) in entry.spans.iter().zip(core_block.content.iter()) {
                if !span.editable || span.len == 0 {
                    continue;
                }
                let span_end = span.start + span.len;
                if span.start >= from && span_end <= to {
                    match inline {
                        Inline::Text { id, marks, text } => {
                            targets.push((id.clone(), marks.clone(), None, text.clone()))
                        }
                        Inline::Link {
                            id,
                            marks,
                            href,
                            text,
                        } => targets.push((
                            id.clone(),
                            marks.clone(),
                            Some(href.clone()),
                            text.clone(),
                        )),
                        _ => {}
                    }
                }
            }
        }
        if targets.is_empty() {
            return Ok(self.unhandled(input.selection));
        }
        let mut ops: Vec<PlannedOp> = Vec::new();
        match (kind, action) {
            (None, _) => {
                for (id, marks, _, _) in &targets {
                    for mark in marks {
                        ops.push((
                            "remove-mark",
                            "clear formatting",
                            OperationKind::RemoveMark {
                                text_id: id.clone(),
                                kind: mark.kind.clone(),
                                value: mark.value.clone(),
                            },
                        ));
                    }
                }
            }
            (Some(MarkKind::Link), action) => {
                // Links are a different inline kind: swap runs in place.
                for (id, marks, _, text) in &targets {
                    let block_id = index
                        .blocks
                        .iter()
                        .find(|entry| entry.spans.iter().any(|span| &span.id == id))
                        .map(|entry| entry.id.clone())
                        .expect("target run belongs to a block");
                    let replacement = if action == "remove" {
                        Inline::Text {
                            id: StableId::new("text"),
                            text: text.clone(),
                            marks: marks.clone(),
                        }
                    } else {
                        Inline::Link {
                            id: StableId::new("link"),
                            text: text.clone(),
                            href: input.value.clone().unwrap_or_default(),
                            marks: marks.clone(),
                        }
                    };
                    ops.push((
                        "insert-inline",
                        "convert run to link",
                        OperationKind::InsertInline {
                            block_id,
                            position: InsertPosition::After(id.clone()),
                            inline: replacement,
                        },
                    ));
                    ops.push((
                        "delete-inline",
                        "convert run to link",
                        OperationKind::DeleteInline {
                            inline_id: id.clone(),
                        },
                    ));
                }
            }
            (Some(kind), action) => {
                let every_has = targets
                    .iter()
                    .all(|(_, marks, _, _)| marks.iter().any(|mark| mark.kind == kind));
                let removing = action == "remove" || (action == "toggle" && every_has);
                for (id, marks, _, _) in &targets {
                    let existing: Vec<&Mark> =
                        marks.iter().filter(|mark| mark.kind == kind).collect();
                    if removing {
                        for mark in existing {
                            ops.push((
                                "remove-mark",
                                "remove formatting",
                                OperationKind::RemoveMark {
                                    text_id: id.clone(),
                                    kind: kind.clone(),
                                    value: mark.value.clone(),
                                },
                            ));
                        }
                        continue;
                    }
                    let value = input.value.clone();
                    if existing.iter().any(|mark| mark.value == value) {
                        continue;
                    }
                    for mark in existing {
                        ops.push((
                            "remove-mark",
                            "replace formatting value",
                            OperationKind::RemoveMark {
                                text_id: id.clone(),
                                kind: kind.clone(),
                                value: mark.value.clone(),
                            },
                        ));
                    }
                    ops.push((
                        "add-mark",
                        "apply formatting",
                        OperationKind::AddMark {
                            text_id: id.clone(),
                            mark: Mark {
                                kind: kind.clone(),
                                value,
                                expand: MarkExpand::None,
                            },
                        },
                    ));
                }
            }
        }
        if ops.is_empty() {
            return Ok(EditorResult {
                document: self.document(),
                selection: self.reselect(&index, start, end),
                handled: true,
            });
        }
        let document = self.apply_batch(ops)?;
        let after = DocumentIndex::build(&self.document.blocks);
        let selection = self.reselect(&after, start, end);
        Ok(EditorResult {
            document,
            selection,
            handled: true,
        })
    }

    fn reselect(&self, index: &DocumentIndex, start: Resolved, end: Resolved) -> EditorSelection {
        let clamp = |resolved: Resolved| Resolved {
            block: resolved.block.min(index.blocks.len().saturating_sub(1)),
            abs: resolved.abs,
        };
        EditorSelection {
            anchor: index.position(clamp(start)),
            focus: index.position(clamp(end)),
        }
    }

    /// Insert an empty `rows` x `columns` table after any document block.
    pub fn insert_table_after_sized(
        &mut self,
        after_block_id: impl AsRef<str>,
        rows: usize,
        columns: usize,
    ) -> Result<AppDocument, AppApiError> {
        let after = StableId::parse(after_block_id.as_ref())
            .map_err(|err| AppApiError::Model(err.to_string()))?;
        if crate::document_tree::find_block_in_blocks(&self.document.blocks, &after).is_none() {
            return Err(AppApiError::NotFound(format!(
                "block {after} was not found"
            )));
        }
        let rows = rows.clamp(1, 200);
        let columns = columns.clamp(1, 50);
        let table_rows = (0..rows)
            .map(|_| opendoc_core::TableRow::empty(columns))
            .collect();
        self.apply_batch(vec![(
            "insert-block",
            "table after block",
            OperationKind::InsertBlock {
                position: InsertPosition::After(after),
                block: Block {
                    id: StableId::new("block"),
                    kind: BlockKind::table(table_rows),
                    content: Vec::new(),
                    properties: BlockProperties::default(),
                },
            },
        )])
    }
}

impl Deref for EditorCommandService<'_> {
    type Target = OpenDocApp;

    fn deref(&self) -> &Self::Target {
        self.app
    }
}

impl DerefMut for EditorCommandService<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.app
    }
}

pub(crate) mod clipboard_html;

use clipboard_html::{PastedBlock, PastedBlockKind};

/// Explicit, safe ordered-list settings that a rich clipboard fragment gave
/// one parse-local list wrapper. Missing fields deliberately remain inherited.
fn pasted_list_settings(
    blocks: &[PastedBlock],
) -> BTreeMap<u64, (u8, Option<u32>, Option<opendoc_core::OrderedListFormat>)> {
    blocks
        .iter()
        .filter_map(|block| match &block.kind {
            PastedBlockKind::ListItem {
                kind,
                level,
                list_key,
                ordered_start,
                ordered_format,
            } if kind.is_ordered() && (ordered_start.is_some() || ordered_format.is_some()) => {
                Some((*list_key, (*level, *ordered_start, *ordered_format)))
            }
            _ => None,
        })
        .collect()
}

/// Append list-run properties only after the item operations that make their
/// targets exist. `apply` validates the target as an ordered list level, so
/// reversing this order would turn a valid rich paste into a warning.
fn append_pasted_list_settings(
    ops: &mut Vec<(&'static str, &'static str, OperationKind)>,
    lists: &BTreeMap<u64, StableId>,
    settings: &BTreeMap<u64, (u8, Option<u32>, Option<opendoc_core::OrderedListFormat>)>,
) {
    for (key, (level, start, format)) in settings {
        let Some(list_id) = lists.get(key) else {
            continue;
        };
        if let Some(start) = start {
            ops.push((
                "set-list-start",
                "paste ordered-list start",
                OperationKind::SetListStart {
                    list_id: list_id.clone(),
                    level: *level,
                    start: *start,
                },
            ));
        }
        if let Some(format) = format {
            ops.push((
                "set-list-format",
                "paste ordered-list format",
                OperationKind::SetListFormat {
                    list_id: list_id.clone(),
                    level: *level,
                    format: *format,
                },
            ));
        }
    }
}

/// Emit only properties the safe clipboard parser can actually populate. The
/// target block already exists (unlike the middle pasted blocks), so these
/// must be normal mergeable operations in the same undoable paste batch.
fn append_pasted_block_properties(
    ops: &mut Vec<(&'static str, &'static str, OperationKind)>,
    block_id: &StableId,
    properties: &BlockProperties,
) {
    if let Some(alignment) = properties.alignment {
        ops.push((
            "set-block-property",
            "paste paragraph alignment",
            OperationKind::SetBlockProperty {
                block_id: block_id.clone(),
                property: BlockProperty::Alignment(alignment),
            },
        ));
    }
    if let Some(direction) = properties.direction {
        ops.push((
            "set-block-property",
            "paste paragraph direction",
            OperationKind::SetBlockProperty {
                block_id: block_id.clone(),
                property: BlockProperty::Direction(direction),
            },
        ));
    }
}

/// Materialise a clipboard-local list identity once per paste.  The parser
/// cannot mint document identities, and reusing its counter across pastes
/// would incorrectly join unrelated lists.
pub(crate) fn pasted_block_style(
    block: &PastedBlock,
    lists: &mut BTreeMap<u64, StableId>,
) -> Option<BlockTextStyle> {
    match &block.kind {
        PastedBlockKind::Paragraph => None,
        PastedBlockKind::Heading { level } => Some(BlockTextStyle::Heading { level: *level }),
        PastedBlockKind::ListItem {
            kind,
            level,
            list_key,
            ..
        } => Some(BlockTextStyle::ListItem {
            list_id: lists
                .entry(*list_key)
                .or_insert_with(|| StableId::new("list"))
                .clone(),
            level: *level,
            kind: *kind,
        }),
        PastedBlockKind::Table { .. } => None,
        PastedBlockKind::Image(_) => None,
        PastedBlockKind::HorizontalRule => None,
    }
}

pub(crate) fn block_kind_from_style(style: BlockTextStyle) -> BlockKind {
    match style {
        BlockTextStyle::Paragraph => BlockKind::Paragraph,
        BlockTextStyle::Title => BlockKind::Title,
        BlockTextStyle::Subtitle => BlockKind::Subtitle,
        BlockTextStyle::Heading { level } => BlockKind::Heading { level },
        BlockTextStyle::ListItem {
            list_id,
            level,
            kind,
        } => BlockKind::ListItem {
            list_id,
            level,
            kind,
        },
    }
}

pub(crate) fn pasted_block_to_block(block: &PastedBlock) -> Block {
    let kind = match &block.kind {
        PastedBlockKind::Heading { level } => BlockKind::Heading { level: *level },
        // A clipboard-local list identity cannot escape into an independently
        // constructed cell.  Retain its words and marks as paragraphs rather
        // than accidentally joining it to an unrelated document list.
        PastedBlockKind::Image(image) => BlockKind::Image {
            blob_hash: image
                .blob_hash
                .clone()
                .expect("clipboard images are materialised before insertion"),
            alt_text: image.alt_text.clone(),
            layout: ImageLayout::default(),
        },
        PastedBlockKind::HorizontalRule => BlockKind::HorizontalRule,
        PastedBlockKind::Paragraph
        | PastedBlockKind::ListItem { .. }
        | PastedBlockKind::Table { .. } => BlockKind::Paragraph,
    };
    Block {
        id: StableId::new("block"),
        kind,
        content: if block.runs.is_empty() && !is_atomic_pasted_block(Some(block)) {
            Block::paragraph("").content
        } else {
            block.runs.clone()
        },
        properties: block.properties.clone(),
    }
}

fn is_atomic_pasted_block(block: Option<&PastedBlock>) -> bool {
    matches!(
        block.map(|block| &block.kind),
        Some(PastedBlockKind::Image(_) | PastedBlockKind::HorizontalRule)
    )
}

#[cfg(test)]
mod mark_tests;
#[cfg(test)]
mod tests;
