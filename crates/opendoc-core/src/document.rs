//! The document root and the validation walk over its block tree.

use crate::annotation::{
    Anchor, CommentActivityEntry, CommentHistoryEntry, CommentThread, Suggestion, SuggestionKind,
    MAX_COMMENT_ACTIVITY_ENTRIES,
};
use crate::block::{Block, BlockKind, Footnote, TextScope};
use crate::bookmark::Bookmark;
use crate::citation::CitationDatabase;
use crate::ids::validate_stable_id;
use crate::ids::{derived_stable_id, DocumentUuid, HashRef, StableId};
use crate::inline::{Equation, Inline, Mark, MarkKind};
use crate::list::ListProperties;
use crate::page::{validate_furniture_payload, HeaderFooterSlot, PageSetup, Section};
use crate::table::validate_table_geometry;
use crate::text_sequence::{TextSequence, TextTokenId};
use crate::warning::{ModelError, ModelWarning};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
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
    /// Optional first-page header override. `None` inherits [`Self::header`],
    /// while `Some(vec![])` explicitly suppresses it on page one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_page_header: Option<Vec<Block>>,
    /// Optional first-page footer override. See [`Self::first_page_header`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_page_footer: Option<Vec<Block>>,
    /// Optional even-page header override. `None` inherits [`Self::header`],
    /// while `Some(vec![])` explicitly suppresses it on even pages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub even_page_header: Option<Vec<Block>>,
    /// Optional even-page footer override. See [`Self::even_page_header`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub even_page_footer: Option<Vec<Block>>,
    /// Per-section source state. An empty map is a legacy document whose
    /// document-wide page fields are its implicit root section. New section
    /// authoring first materializes that deterministic root through
    /// [`Self::materialize_legacy_sections`].
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub sections: BTreeMap<StableId, Section>,
    /// Numbering settings keyed by list-run identity.  This is deliberately
    /// document-level rather than an item field: a restart applies to the
    /// wrapper, even when a later edit changes which item is first.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub list_properties: BTreeMap<StableId, ListProperties>,
    /// Durable character-token source, keyed by its editable text or link
    /// inline id. An absent map denotes a legacy snapshot; a present map must
    /// cover every editable run exactly once.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub text_sequences: BTreeMap<StableId, TextSequence>,
    /// Durable named block targets used by links and generated navigation.
    /// Tombstones are retained so an old replica cannot resurrect a deleted
    /// bookmark during collaboration.
    #[serde(default)]
    pub bookmarks: Vec<Bookmark>,
    pub blocks: Vec<Block>,
    pub footnotes: Vec<Footnote>,
    /// The note records whose references are endnotes rather than footnotes.
    ///
    /// Notes deliberately stay in the one stable-id namespace: citations can
    /// live in either sort of note, and a reference has the same atomic shape.
    /// Placement belongs here, however, because it is document layout state,
    /// not an accidental spelling convention on the note id.  An absent id is
    /// a footnote, which also keeps old saved documents source-compatible.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub endnote_ids: BTreeSet<StableId>,
    pub comments: Vec<CommentThread>,
    /// Append-only evidence for edits and per-comment tombstones.  Keeping it
    /// at the document root avoids turning every live comment into a growing
    /// rendering payload while remaining part of saved, signed source state.
    #[serde(default)]
    pub comment_history: Vec<CommentHistoryEntry>,
    /// Bounded, append-only collaboration-session activity. Unlike
    /// `comment_history`, this covers thread and action transitions and keeps
    /// the causal operation id needed to deduplicate replay.
    #[serde(default)]
    pub comment_activity: Vec<CommentActivityEntry>,
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
            first_page_header: None,
            first_page_footer: None,
            even_page_header: None,
            even_page_footer: None,
            sections: BTreeMap::new(),
            list_properties: BTreeMap::new(),
            text_sequences: BTreeMap::new(),
            bookmarks: Vec::new(),
            blocks: Vec::new(),
            footnotes: Vec::new(),
            endnote_ids: BTreeSet::new(),
            comments: Vec::new(),
            comment_history: Vec::new(),
            comment_activity: Vec::new(),
            suggestions: Vec::new(),
            citation_database: CitationDatabase::default(),
            warnings: Vec::new(),
        }
    }

    /// The stable identity of this document's implicit root section.
    pub fn root_section_id(&self) -> StableId {
        derived_stable_id("section-root", &[self.uuid.as_str()])
    }

    /// Materialize legacy document-wide page context as the deterministic root
    /// section before creating a section boundary or a section-targeted
    /// operation. This mirrors token-source materialization: a read of an old
    /// document does not silently claim a new durable representation.
    pub fn materialize_legacy_sections(&mut self) -> Result<bool, ModelError> {
        if !self.sections.is_empty() {
            self.validate_sections()?;
            return Ok(false);
        }
        let root = Section {
            id: self.root_section_id(),
            page_setup: self.page_setup,
            header: self.header.clone(),
            footer: self.footer.clone(),
            first_page_header: self.first_page_header.clone(),
            first_page_footer: self.first_page_footer.clone(),
            even_page_header: self.even_page_header.clone(),
            even_page_footer: self.even_page_footer.clone(),
        };
        self.sections.insert(root.id.clone(), root);
        if let Err(error) = self.validate_sections() {
            // Migration is a source transition, not best-effort repair.  A
            // malformed legacy body must therefore remain byte-for-byte in
            // its legacy representation when materialization is refused.
            self.sections.clear();
            return Err(error);
        }
        Ok(true)
    }

    /// The document degraded to plain text: what a `.txt` export carries.
    ///
    /// Content that is not text becomes its conventional textual stand-in —
    /// an image its alt text, an equation the LaTeX it is written in. See
    /// [`TextScope::PlainText`].
    ///
    /// **This is not what to count words with.** A stand-in is text a reader
    /// never sees, so counting it reports a document whose prose is `one two`
    /// as nine words. [`Self::counted_text`] is the projection for that.
    pub fn visible_text(&self) -> String {
        self.text(TextScope::PlainText)
    }

    /// The text a reader reads off the page: what the word and character
    /// counts count. See [`TextScope::Page`].
    ///
    /// One walk serves both scopes, so the two projections cannot drift into
    /// disagreeing about what a table cell, a covered cell or a page-number
    /// field contributes — only about the handful of things they are
    /// deliberately answering differently.
    pub fn counted_text(&self) -> String {
        self.text(TextScope::Page)
    }

    fn text(&self, scope: TextScope) -> String {
        let mut out = String::new();
        for block in &self.blocks {
            block.push_visible_text(&self.citation_database, scope, &mut out);
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
            HeaderFooterSlot::FirstPageHeader => self.first_page_header.as_deref().unwrap_or(&[]),
            HeaderFooterSlot::FirstPageFooter => self.first_page_footer.as_deref().unwrap_or(&[]),
            HeaderFooterSlot::EvenPageHeader => self.even_page_header.as_deref().unwrap_or(&[]),
            HeaderFooterSlot::EvenPageFooter => self.even_page_footer.as_deref().unwrap_or(&[]),
        }
    }

    /// The blocks occupying one page-furniture slot, for replacement.
    pub fn furniture_mut(&mut self, slot: HeaderFooterSlot) -> &mut Vec<Block> {
        match slot {
            HeaderFooterSlot::Header => &mut self.header,
            HeaderFooterSlot::Footer => &mut self.footer,
            HeaderFooterSlot::FirstPageHeader => self.first_page_header.get_or_insert_default(),
            HeaderFooterSlot::FirstPageFooter => self.first_page_footer.get_or_insert_default(),
            HeaderFooterSlot::EvenPageHeader => self.even_page_header.get_or_insert_default(),
            HeaderFooterSlot::EvenPageFooter => self.even_page_footer.get_or_insert_default(),
        }
    }

    /// Furniture that should appear on `page_index` (zero based). A missing
    /// first- or even-page override inherits its ordinary slot; an explicit
    /// empty override intentionally renders nothing.
    pub fn furniture_for_page(&self, slot: HeaderFooterSlot, page_index: usize) -> &[Block] {
        match (slot.base_slot(), page_index == 0, page_index % 2 == 1) {
            (HeaderFooterSlot::Header, true, _) => {
                self.first_page_header.as_deref().unwrap_or(&self.header)
            }
            (HeaderFooterSlot::Footer, true, _) => {
                self.first_page_footer.as_deref().unwrap_or(&self.footer)
            }
            (HeaderFooterSlot::Header, false, true) => {
                self.even_page_header.as_deref().unwrap_or(&self.header)
            }
            (HeaderFooterSlot::Footer, false, true) => {
                self.even_page_footer.as_deref().unwrap_or(&self.footer)
            }
            (HeaderFooterSlot::Header, false, false) => &self.header,
            (HeaderFooterSlot::Footer, false, false) => &self.footer,
            _ => unreachable!("base_slot only returns an ordinary furniture slot"),
        }
    }

    /// Whether a first-page slot is explicitly present, including an empty
    /// override that suppresses inherited furniture. Ordinary slots are
    /// always present.
    pub const fn has_furniture_override(&self, slot: HeaderFooterSlot) -> bool {
        match slot {
            HeaderFooterSlot::FirstPageHeader => self.first_page_header.is_some(),
            HeaderFooterSlot::FirstPageFooter => self.first_page_footer.is_some(),
            HeaderFooterSlot::EvenPageHeader => self.even_page_header.is_some(),
            HeaderFooterSlot::EvenPageFooter => self.even_page_footer.is_some(),
            HeaderFooterSlot::Header | HeaderFooterSlot::Footer => true,
        }
    }

    /// Removes a first/even-page override so that slot inherits ordinary
    /// furniture again. Returns `false` for ordinary slots, which cannot
    /// inherit from another document-level slot.
    pub fn clear_furniture_override(&mut self, slot: HeaderFooterSlot) -> bool {
        match slot {
            HeaderFooterSlot::FirstPageHeader => self.first_page_header = None,
            HeaderFooterSlot::FirstPageFooter => self.first_page_footer = None,
            HeaderFooterSlot::EvenPageHeader => self.even_page_header = None,
            HeaderFooterSlot::EvenPageFooter => self.even_page_footer = None,
            HeaderFooterSlot::Header | HeaderFooterSlot::Footer => return false,
        }
        true
    }

    /// Persist deterministic baseline token identities for every editable
    /// text/link run in a legacy document. This is deliberately an explicit
    /// migration: merely reading an old snapshot continues to leave the map
    /// absent, so it cannot claim durable character-anchor support until the
    /// migrated source is saved.
    ///
    /// Returns `true` when legacy source was materialized. A partially
    /// populated map is invalid source and is never silently completed.
    pub fn materialize_legacy_text_sequences(&mut self) -> Result<bool, ModelError> {
        if !self.text_sequences.is_empty() {
            self.validate_text_sequences()?;
            return Ok(false);
        }
        let runs = editable_text_runs(self);
        if runs.is_empty() {
            return Ok(false);
        }
        self.text_sequences = runs
            .into_iter()
            .map(|(inline_id, text)| {
                let sequence = TextSequence::materialize_legacy(&self.uuid, &inline_id, &text);
                (inline_id, sequence)
            })
            .collect();
        self.validate_text_sequences()?;
        Ok(true)
    }

    /// Bring an already-materialized token map back into coverage after a
    /// whole-run or structural write.  Unchanged runs keep their durable
    /// tokens (including tombstones); a run whose visible projection was
    /// replaced wholesale starts a new baseline for its new source text.
    ///
    /// This is intentionally not a migration: an empty map remains the
    /// legacy representation.  Merge uses it only after it has explicitly
    /// materialized a map for a character edit, or when the input snapshot
    /// already carried one.
    pub fn synchronize_text_sequences(&mut self) -> Result<(), ModelError> {
        if self.text_sequences.is_empty() {
            return Ok(());
        }
        let runs = editable_text_runs(self);
        self.text_sequences = runs
            .into_iter()
            .map(|(inline_id, text)| {
                let sequence = self
                    .text_sequences
                    .remove(&inline_id)
                    .filter(|sequence| sequence.visible_text() == text)
                    .unwrap_or_else(|| {
                        TextSequence::materialize_legacy(&self.uuid, &inline_id, &text)
                    });
                (inline_id, sequence)
            })
            .collect();
        self.validate_text_sequences()
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
        if self.sections.is_empty() {
            self.page_setup.validate()?;
        }
        self.validate_sections()?;
        for (list_id, properties) in &self.list_properties {
            validate_stable_id("list properties id", list_id)?;
            properties.validate()?;
        }
        self.validate_text_sequences()?;
        let mut bookmark_ids = BTreeSet::new();
        let mut live_bookmark_names = BTreeSet::new();
        for bookmark in &self.bookmarks {
            bookmark.validate()?;
            if !bookmark_ids.insert(bookmark.id.clone()) {
                return Err(ModelError::InvalidDocument("duplicate bookmark id"));
            }
            if !bookmark.deleted && !live_bookmark_names.insert(bookmark.name.clone()) {
                return Err(ModelError::InvalidDocument("duplicate live bookmark name"));
            }
        }
        // Body, header and footer share one block/inline id space: a block id
        // is the document's addressing unit, so a header block that reused a
        // body block's id would make every id-keyed operation ambiguous.
        let mut block_ids = BTreeSet::new();
        let mut inline_ids = BTreeSet::new();
        let mut cell_ids = BTreeSet::new();
        validate_blocks(
            &self.blocks,
            &mut block_ids,
            &mut inline_ids,
            &mut cell_ids,
            true,
        )?;
        if self.sections.is_empty() {
            for slot in HeaderFooterSlot::ALL {
                let furniture = self.furniture(slot);
                validate_blocks(
                    furniture,
                    &mut block_ids,
                    &mut inline_ids,
                    &mut cell_ids,
                    false,
                )?;
                validate_furniture_payload(furniture)?;
            }
        } else {
            for section in self.sections.values() {
                for slot in HeaderFooterSlot::ALL {
                    let furniture = section.furniture(slot);
                    validate_blocks(
                        furniture,
                        &mut block_ids,
                        &mut inline_ids,
                        &mut cell_ids,
                        false,
                    )?;
                    validate_furniture_payload(furniture)?;
                }
            }
        }
        let mut comment_thread_ids = BTreeSet::new();
        for comment in &self.comments {
            if !comment_thread_ids.insert(comment.id.clone()) {
                return Err(ModelError::InvalidDocument("duplicate comment thread id"));
            }
            comment.validate()?;
            self.validate_token_anchor(&comment.anchor)?;
        }
        let mut comment_history_operation_ids = BTreeSet::new();
        for entry in &self.comment_history {
            entry.validate()?;
            // `at_ms` is the merge operation sequence number, and `actor`
            // is its actor.  Together they are the lossless operation id for
            // this older, compact provenance format.  Duplicating one would
            // make the append-only review record lie about a single action
            // occurring more than once.
            if !comment_history_operation_ids.insert((entry.actor.clone(), entry.at_ms)) {
                return Err(ModelError::InvalidDocument(
                    "duplicate comment history operation id",
                ));
            }
            let Some(thread) = self
                .comments
                .iter()
                .find(|thread| thread.id == entry.thread_id)
            else {
                return Err(ModelError::InvalidDocument(
                    "comment history references missing thread",
                ));
            };
            if thread
                .comments
                .iter()
                .all(|comment| comment.id != entry.comment_id)
            {
                return Err(ModelError::InvalidDocument(
                    "comment history references missing comment",
                ));
            }
        }
        if self.comment_activity.len() > MAX_COMMENT_ACTIVITY_ENTRIES {
            return Err(ModelError::InvalidDocument(
                "comment activity exceeds its deterministic capacity",
            ));
        }
        let mut comment_activity_operation_ids = BTreeSet::new();
        let mut prior_activity_key = None;
        for entry in &self.comment_activity {
            entry.validate()?;
            if !comment_activity_operation_ids
                .insert((entry.operation_actor.clone(), entry.operation_seq))
            {
                return Err(ModelError::InvalidDocument(
                    "duplicate comment activity operation id",
                ));
            }
            let key = (
                entry.at_ms,
                entry.operation_actor.clone(),
                entry.operation_seq,
            );
            if prior_activity_key
                .as_ref()
                .is_some_and(|prior| prior > &key)
            {
                return Err(ModelError::InvalidDocument(
                    "comment activity is not in canonical order",
                ));
            }
            prior_activity_key = Some(key);
            let Some(thread) = self
                .comments
                .iter()
                .find(|thread| thread.id == entry.thread_id)
            else {
                return Err(ModelError::InvalidDocument(
                    "comment activity references missing thread",
                ));
            };
            if let Some(comment_id) = &entry.comment_id {
                if thread
                    .comments
                    .iter()
                    .all(|comment| comment.id != *comment_id)
                {
                    return Err(ModelError::InvalidDocument(
                        "comment activity references missing comment",
                    ));
                }
            }
        }
        let mut suggestion_ids = BTreeSet::new();
        for suggestion in &self.suggestions {
            if !suggestion_ids.insert(suggestion.id.clone()) {
                return Err(ModelError::InvalidDocument("duplicate suggestion id"));
            }
            suggestion.validate()?;
            if let SuggestionKind::Insert { anchor, .. } = &suggestion.kind {
                self.validate_token_anchor(anchor)?;
            }
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
        if !self.endnote_ids.is_subset(&footnote_ids) {
            return Err(ModelError::InvalidDocument(
                "endnote placement references a missing note",
            ));
        }
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

    fn validate_sections(&self) -> Result<(), ModelError> {
        if self.sections.is_empty() {
            if self
                .blocks
                .iter()
                .any(|block| matches!(block.kind, BlockKind::SectionBreak { .. }))
            {
                return Err(ModelError::InvalidDocument(
                    "section break requires materialized section source",
                ));
            }
            return Ok(());
        }

        let root_id = self.root_section_id();
        let Some(root) = self.sections.get(&root_id) else {
            return Err(ModelError::InvalidDocument(
                "section source has no root section",
            ));
        };
        if root.id != root_id {
            return Err(ModelError::InvalidDocument(
                "root section id does not match document",
            ));
        }
        for (section_id, section) in &self.sections {
            validate_stable_id("section id", section_id)?;
            if &section.id != section_id {
                return Err(ModelError::InvalidDocument(
                    "section map key does not match section id",
                ));
            }
            section.page_setup.validate()?;
        }

        let mut boundary_sections = BTreeSet::new();
        for (index, block) in self.blocks.iter().enumerate() {
            let BlockKind::SectionBreak { section_id } = &block.kind else {
                continue;
            };
            if index == 0 || index + 1 == self.blocks.len() {
                return Err(ModelError::InvalidDocument(
                    "section break cannot be first or last body block",
                ));
            }
            if matches!(self.blocks[index - 1].kind, BlockKind::SectionBreak { .. })
                || matches!(self.blocks[index + 1].kind, BlockKind::SectionBreak { .. })
            {
                return Err(ModelError::InvalidDocument(
                    "section breaks cannot be adjacent",
                ));
            }
            if section_id == &root_id || !self.sections.contains_key(section_id) {
                return Err(ModelError::InvalidDocument(
                    "section break refers to an unknown or root section",
                ));
            }
            if !boundary_sections.insert(section_id.clone()) {
                return Err(ModelError::InvalidDocument(
                    "more than one section break refers to a section",
                ));
            }
        }
        if self.sections.len() != boundary_sections.len() + 1 {
            return Err(ModelError::InvalidDocument(
                "every non-root section requires one section break",
            ));
        }
        Ok(())
    }

    fn validate_text_sequences(&self) -> Result<(), ModelError> {
        // Empty is the unambiguous legacy representation. It is intentionally
        // valid until an explicit signed migration materializes the map.
        if self.text_sequences.is_empty() {
            return Ok(());
        }
        let runs = editable_text_runs(self);
        if self.text_sequences.len() != runs.len()
            || self.text_sequences.keys().any(|id| !runs.contains_key(id))
        {
            return Err(ModelError::InvalidDocument(
                "text sequence map does not cover editable runs exactly",
            ));
        }
        for (inline_id, text) in runs {
            let sequence =
                self.text_sequences
                    .get(&inline_id)
                    .ok_or(ModelError::InvalidDocument(
                        "text sequence map does not cover editable runs exactly",
                    ))?;
            sequence.validate()?;
            if sequence.visible_text() != text {
                return Err(ModelError::InvalidDocument(
                    "text sequence visible text differs from inline text",
                ));
            }
            let mut baseline_ordinals = BTreeSet::new();
            let mut previous_baseline_ordinal = None;
            for token in &sequence.tokens {
                if let TextTokenId::Baseline {
                    document_uuid,
                    inline_id: token_inline_id,
                    ordinal: token_ordinal,
                } = &token.id
                {
                    if document_uuid != &self.uuid || token_inline_id != &inline_id {
                        return Err(ModelError::InvalidDocument(
                            "baseline text token belongs to another editable run",
                        ));
                    }
                    if !baseline_ordinals.insert(*token_ordinal)
                        || previous_baseline_ordinal
                            .is_some_and(|previous| *token_ordinal <= previous)
                    {
                        return Err(ModelError::InvalidDocument(
                            "baseline text token ordinals are not strictly ordered",
                        ));
                    }
                    previous_baseline_ordinal = Some(*token_ordinal);
                }
            }
        }
        Ok(())
    }

    /// Token anchors require an explicitly persisted sequence map.  This is
    /// intentionally stricter than legacy whole-inline anchors: accepting a
    /// character range without its source atoms would fabricate durability on
    /// the next concurrent edit.
    fn validate_token_anchor(&self, anchor: &Anchor) -> Result<(), ModelError> {
        let Anchor::TokenRange(range) = anchor else {
            return Ok(());
        };
        let sequence =
            self.text_sequences
                .get(&range.inline_id)
                .ok_or(ModelError::InvalidDocument(
                    "token anchor requires a persisted text sequence",
                ))?;
        range.validate_against(sequence)
    }
}

fn editable_text_runs(document: &Document) -> BTreeMap<StableId, String> {
    let mut runs = BTreeMap::new();
    collect_editable_text_runs(&document.blocks, &mut runs);
    if document.sections.is_empty() {
        collect_editable_text_runs(&document.header, &mut runs);
        collect_editable_text_runs(&document.footer, &mut runs);
        if let Some(blocks) = &document.first_page_header {
            collect_editable_text_runs(blocks, &mut runs);
        }
        if let Some(blocks) = &document.first_page_footer {
            collect_editable_text_runs(blocks, &mut runs);
        }
        if let Some(blocks) = &document.even_page_header {
            collect_editable_text_runs(blocks, &mut runs);
        }
        if let Some(blocks) = &document.even_page_footer {
            collect_editable_text_runs(blocks, &mut runs);
        }
    } else {
        for section in document.sections.values() {
            for slot in HeaderFooterSlot::ALL {
                collect_editable_text_runs(section.furniture(slot), &mut runs);
            }
        }
    }
    runs
}

fn collect_editable_text_runs(blocks: &[Block], runs: &mut BTreeMap<StableId, String>) {
    for block in blocks {
        for inline in &block.content {
            match inline {
                Inline::Text { id, text, .. } | Inline::Link { id, text, .. } => {
                    runs.insert(id.clone(), text.clone());
                }
                _ => {}
            }
        }
        if let BlockKind::Table { rows, .. } = &block.kind {
            for row in rows {
                for cell in &row.cells {
                    collect_editable_text_runs(&cell.blocks, runs);
                }
            }
        }
    }
}

pub(crate) fn validate_block_tree(blocks: &[Block]) -> Result<(), ModelError> {
    let mut block_ids = BTreeSet::new();
    let mut inline_ids = BTreeSet::new();
    let mut cell_ids = BTreeSet::new();
    validate_blocks(
        blocks,
        &mut block_ids,
        &mut inline_ids,
        &mut cell_ids,
        false,
    )
}

pub(crate) fn validate_blocks(
    blocks: &[Block],
    block_ids: &mut BTreeSet<StableId>,
    inline_ids: &mut BTreeSet<StableId>,
    cell_ids: &mut BTreeSet<StableId>,
    section_breaks_allowed: bool,
) -> Result<(), ModelError> {
    for block in blocks {
        validate_stable_id("block id", &block.id)?;
        if !block_ids.insert(block.id.clone()) {
            return Err(ModelError::InvalidDocument("duplicate block id"));
        }
        if matches!(block.kind, BlockKind::SectionBreak { .. }) && !section_breaks_allowed {
            return Err(ModelError::InvalidDocument(
                "section break is only allowed in document body flow",
            ));
        }
        for inline in &block.content {
            let id = inline_stable_id(inline);
            validate_stable_id("inline id", id)?;
            if !inline_ids.insert(id.clone()) {
                return Err(ModelError::InvalidDocument("duplicate inline id"));
            }
            validate_inline(inline)?;
        }
        if let BlockKind::Table {
            columns,
            properties,
            rows,
        } = &block.kind
        {
            properties.validate()?;
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
                for cell in &row.cells {
                    validate_stable_id("table cell id", &cell.id)?;
                    if !cell_ids.insert(cell.id.clone()) {
                        return Err(ModelError::InvalidDocument("duplicate table cell id"));
                    }
                    if cell.blocks.is_empty() {
                        return Err(ModelError::InvalidDocument("table cell has no blocks"));
                    }
                    cell.properties.validate()?;
                    validate_blocks(&cell.blocks, block_ids, inline_ids, cell_ids, false)?;
                }
            }
            validate_table_geometry(columns, rows)?;
        }
        validate_block_payload(block)?;
        if let BlockKind::Image {
            layout:
                crate::image::ImageLayout {
                    positioned:
                        Some(crate::image::PositionedImage {
                            anchor: crate::image::PositionedImageAnchor::Block(anchor),
                            ..
                        }),
                    ..
                },
            ..
        } = &block.kind
        {
            if anchor == &block.id {
                return Err(ModelError::InvalidDocument(
                    "positioned image cannot anchor to itself",
                ));
            }
        }
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
        BlockKind::TableOfContents { max_level } if !(1..=6).contains(max_level) => Err(
            ModelError::InvalidDocument("table of contents max level is outside 1..=6"),
        ),
        BlockKind::TableOfContents { .. } if !block.content.is_empty() => Err(
            ModelError::InvalidDocument("table of contents cannot contain inline content"),
        ),
        BlockKind::TableOfContents { .. } if !block.properties.is_empty() => Err(
            ModelError::InvalidDocument("table of contents cannot contain block properties"),
        ),
        BlockKind::Bibliography if !block.content.is_empty() => Err(ModelError::InvalidDocument(
            "bibliography cannot contain inline content",
        )),
        BlockKind::Bibliography if !block.properties.is_empty() => Err(
            ModelError::InvalidDocument("bibliography cannot contain block properties"),
        ),
        BlockKind::SectionBreak { .. } if !block.content.is_empty() => Err(
            ModelError::InvalidDocument("section break cannot contain inline content"),
        ),
        BlockKind::SectionBreak { .. } if !block.properties.is_empty() => Err(
            ModelError::InvalidDocument("section break cannot contain block properties"),
        ),
        BlockKind::SectionBreak { section_id } => {
            validate_stable_id("section break id", section_id)
        }
        BlockKind::HorizontalRule if !block.content.is_empty() => Err(ModelError::InvalidDocument(
            "horizontal rule cannot contain inline content",
        )),
        BlockKind::HorizontalRule if !block.properties.is_empty() => Err(
            ModelError::InvalidDocument("horizontal rule cannot contain block properties"),
        ),
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
        Inline::GooglePersonChip {
            label,
            email,
            person_id,
            ..
        } => {
            if label.trim().is_empty() || email.trim().is_empty() {
                return Err(ModelError::InvalidDocument(
                    "Google person chip label or email is empty",
                ));
            }
            if person_id.as_deref().is_some_and(|id| id.trim().is_empty()) {
                return Err(ModelError::InvalidDocument(
                    "Google person chip id is empty",
                ));
            }
            Ok(())
        }
        Inline::GoogleRichLinkChip {
            label,
            href,
            rich_link_id,
            mime_type,
            ..
        } => {
            if label.trim().is_empty() || href.trim().is_empty() {
                return Err(ModelError::InvalidDocument(
                    "Google rich link chip label or href is empty",
                ));
            }
            if rich_link_id
                .as_deref()
                .is_some_and(|id| id.trim().is_empty())
                || mime_type
                    .as_deref()
                    .is_some_and(|mime| mime.trim().is_empty())
            {
                return Err(ModelError::InvalidDocument(
                    "Google rich link chip metadata is empty",
                ));
            }
            Ok(())
        }
        Inline::Dropdown {
            options,
            selected_option_id,
            ..
        } => {
            if options.is_empty() {
                return Err(ModelError::InvalidDocument("dropdown has no options"));
            }
            let mut ids = BTreeSet::new();
            for option in options {
                if option.id.trim().is_empty() || option.id.trim() != option.id {
                    return Err(ModelError::InvalidDocument(
                        "dropdown option id is empty or non-canonical",
                    ));
                }
                if option.label.trim().is_empty() || option.label.trim() != option.label {
                    return Err(ModelError::InvalidDocument(
                        "dropdown option label is empty or non-canonical",
                    ));
                }
                if !ids.insert(&option.id) {
                    return Err(ModelError::InvalidDocument("duplicate dropdown option id"));
                }
            }
            if !ids.contains(selected_option_id) {
                return Err(ModelError::InvalidDocument(
                    "dropdown selected option is absent",
                ));
            }
            Ok(())
        }
        Inline::DateChip { date, .. } => validate_date_chip(date),
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
        | Inline::GooglePersonChip { .. }
        | Inline::GoogleRichLinkChip { .. }
        | Inline::Dropdown { .. }
        | Inline::DateChip { .. }
        | Inline::Equation { .. }
        | Inline::Citation { .. }
        | Inline::FootnoteRef { .. }
        | Inline::PageNumber { .. } => false,
    })
}

/// Validate the deliberately small calendar-date wire type used by date
/// chips. Locale strings and timestamps have no portable calendar-day
/// semantics, so they are intentionally not accepted here.
fn validate_date_chip(date: &str) -> Result<(), ModelError> {
    let bytes = date.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || !bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
    {
        return Err(ModelError::InvalidDocument("date chip must be YYYY-MM-DD"));
    }
    let number = |range: std::ops::Range<usize>| {
        std::str::from_utf8(&bytes[range])
            .ok()
            .and_then(|part| part.parse::<u32>().ok())
    };
    let (Some(year), Some(month), Some(day)) = (number(0..4), number(5..7), number(8..10)) else {
        return Err(ModelError::InvalidDocument("date chip must be YYYY-MM-DD"));
    };
    if year == 0 || !(1..=12).contains(&month) {
        return Err(ModelError::InvalidDocument(
            "date chip has an invalid calendar date",
        ));
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => unreachable!(),
    };
    if !(1..=days).contains(&day) {
        return Err(ModelError::InvalidDocument(
            "date chip has an invalid calendar date",
        ));
    }
    Ok(())
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

pub(crate) fn validate_mark_removal(
    kind: &MarkKind,
    value: Option<&str>,
) -> Result<(), ModelError> {
    let value_bearing = matches!(
        kind,
        MarkKind::Color | MarkKind::Background | MarkKind::Font | MarkKind::Size
    );
    match (value, value_bearing) {
        (Some(value), true) if value.trim().is_empty() => {
            Err(ModelError::InvalidDocument("mark removal value is empty"))
        }
        (Some(_), false) => Err(ModelError::InvalidDocument(
            "boolean mark removal has value",
        )),
        // No value means remove every value of a value-bearing mark, exactly
        // like the ordinary RemoveMark operation.
        _ => Ok(()),
    }
}

/// Validates the compare-and-set payload of a tracked value-mark replacement.
/// Replacements deliberately do not apply to boolean marks: their absence is
/// not an old *value* and they already have unambiguous add/remove proposals.
pub(crate) fn validate_mark_replacement(
    kind: &MarkKind,
    expected_value: &str,
    value: &str,
) -> Result<(), ModelError> {
    if !matches!(
        kind,
        MarkKind::Color | MarkKind::Background | MarkKind::Font | MarkKind::Size
    ) {
        return Err(ModelError::InvalidDocument(
            "format replacement kind is not value-bearing",
        ));
    }
    if expected_value.trim().is_empty() || expected_value.trim() != expected_value {
        return Err(ModelError::InvalidDocument(
            "format replacement expected value is empty or has surrounding whitespace",
        ));
    }
    if value.trim().is_empty() || value.trim() != value {
        return Err(ModelError::InvalidDocument(
            "format replacement value is empty or has surrounding whitespace",
        ));
    }
    if expected_value == value {
        return Err(ModelError::InvalidDocument(
            "format replacement value equals expected value",
        ));
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
        | Inline::GooglePersonChip { id, .. }
        | Inline::GoogleRichLinkChip { id, .. }
        | Inline::Dropdown { id, .. }
        | Inline::DateChip { id, .. }
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
