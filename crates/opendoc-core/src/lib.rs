use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(not(target_arch = "wasm32"))]
use std::time::{SystemTime, UNIX_EPOCH};

static ID_COUNTER: AtomicU64 = AtomicU64::new(1);
static PROCESS_NONCE: std::sync::OnceLock<u64> = std::sync::OnceLock::new();

/// A 64-bit value that is unique per process (and, with overwhelming
/// probability, across machines). Stable ids combine it with a per-process
/// counter so that two replicas never mint the same block, inline, comment,
/// or actor id.
fn process_nonce() -> u64 {
    *PROCESS_NONCE.get_or_init(|| {
        use std::hash::{BuildHasher, Hasher};
        let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
        hasher.write_u128(now_nanos());
        #[cfg(not(target_arch = "wasm32"))]
        hasher.write_u32(std::process::id());
        #[cfg(target_arch = "wasm32")]
        {
            // No process ids (and no OS entropy for RandomState) in browsers.
            hasher.write_u64((js_sys::Math::random() * u64::MAX as f64) as u64);
            hasher.write_u64((js_sys::Math::random() * u64::MAX as f64) as u64);
        }
        let value = hasher.finish();
        if value == 0 {
            1
        } else {
            value
        }
    })
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
pub struct DocumentUuid(String);

impl DocumentUuid {
    pub fn new() -> Self {
        Self(format!("doc-{:016x}-{:016x}", now_nanos(), next_counter()))
    }

    pub fn parse(value: impl Into<String>) -> Result<Self, ModelError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(ModelError::InvalidId("document uuid is empty"));
        }
        if value.trim() != value {
            return Err(ModelError::InvalidId(
                "document uuid has surrounding whitespace",
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for DocumentUuid {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for DocumentUuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
pub struct StableId(String);

impl StableId {
    pub fn new(prefix: &str) -> Self {
        Self(format!(
            "{prefix}-{:016x}{:08x}",
            process_nonce(),
            next_counter()
        ))
    }

    pub fn parse(value: impl Into<String>) -> Result<Self, ModelError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(ModelError::InvalidId("stable id is empty"));
        }
        if value.trim() != value {
            return Err(ModelError::InvalidId(
                "stable id has surrounding whitespace",
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for StableId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct HashRef {
    algorithm: String,
    digest: String,
}

impl HashRef {
    pub fn new(
        algorithm: impl Into<String>,
        digest: impl Into<String>,
    ) -> Result<Self, ModelError> {
        let algorithm = algorithm.into();
        let digest = digest.into();
        if algorithm.trim().is_empty()
            || digest.trim().is_empty()
            || algorithm.trim() != algorithm
            || digest.trim() != digest
            || !is_hash_ref_component(&algorithm)
            || !is_hash_ref_component(&digest)
        {
            return Err(ModelError::InvalidHash);
        }
        Ok(Self { algorithm, digest })
    }

    pub fn parse(value: &str) -> Result<Self, ModelError> {
        let (algorithm, digest) = value.split_once(':').ok_or(ModelError::InvalidHash)?;
        Self::new(algorithm, digest)
    }

    pub fn algorithm(&self) -> &str {
        &self.algorithm
    }

    pub fn digest(&self) -> &str {
        &self.digest
    }
}

pub fn digest_bytes(algorithm: &str, bytes: &[u8]) -> Result<HashRef, ModelError> {
    match algorithm {
        "sha256" => HashRef::new("sha256", lowercase_hex(&Sha256::digest(bytes))),
        _ => Err(ModelError::UnsupportedHashAlgorithm),
    }
}

fn lowercase_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn is_hash_ref_component(value: &str) -> bool {
    value != "."
        && value != ".."
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.')
}

impl fmt::Display for HashRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.algorithm, self.digest)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub uuid: DocumentUuid,
    pub title: String,
    pub locale: String,
    pub doi: Option<String>,
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
        validate_block_tree(&self.blocks)?;
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

impl DocumentUuid {
    fn validate(&self) -> Result<(), ModelError> {
        if self.0.trim().is_empty() {
            Err(ModelError::InvalidDocument("document uuid is empty"))
        } else if self.0.trim() != self.0 {
            Err(ModelError::InvalidDocument(
                "document uuid has surrounding whitespace",
            ))
        } else {
            Ok(())
        }
    }
}

fn validate_block_tree(blocks: &[Block]) -> Result<(), ModelError> {
    let mut block_ids = BTreeSet::new();
    let mut inline_ids = BTreeSet::new();
    validate_blocks(blocks, &mut block_ids, &mut inline_ids)
}

fn validate_blocks(
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
        if let BlockKind::Table { rows } = &block.kind {
            if rows.is_empty() {
                return Err(ModelError::InvalidDocument("table has no rows"));
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
                    validate_blocks(&cell.blocks, block_ids, inline_ids)?;
                }
            }
        }
        validate_block_payload(block)?;
    }
    Ok(())
}

fn validate_block_payload(block: &Block) -> Result<(), ModelError> {
    match &block.kind {
        BlockKind::Heading { level } if !(1..=6).contains(level) => Err(
            ModelError::InvalidDocument("heading level is outside 1..=6"),
        ),
        BlockKind::ListItem { level, .. } if *level > 8 => Err(ModelError::InvalidDocument(
            "list item level is outside 0..=8",
        )),
        BlockKind::EquationBlock { equation } => validate_equation(equation),
        BlockKind::Image { blob_hash, .. } => HashRef::parse(blob_hash)
            .map(|_| ())
            .map_err(|_| ModelError::InvalidDocument("image blob hash is invalid")),
        _ => Ok(()),
    }
}

fn validate_inline(inline: &Inline) -> Result<(), ModelError> {
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

fn validate_inline_sequence(inlines: &[Inline]) -> Result<(), ModelError> {
    for inline in inlines {
        validate_inline(inline)?;
    }
    Ok(())
}

fn inline_sequence_is_empty_source_text(inlines: &[Inline]) -> bool {
    inlines.iter().all(|inline| match inline {
        Inline::Text { text, .. } | Inline::Link { text, .. } => text.trim().is_empty(),
        Inline::Mention { .. }
        | Inline::Equation { .. }
        | Inline::Citation { .. }
        | Inline::FootnoteRef { .. } => false,
    })
}

fn validate_equation(equation: &Equation) -> Result<(), ModelError> {
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

fn validate_marks(marks: &[Mark]) -> Result<(), ModelError> {
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

fn inline_stable_id(inline: &Inline) -> &StableId {
    match inline {
        Inline::Text { id, .. }
        | Inline::Link { id, .. }
        | Inline::Citation { id, .. }
        | Inline::FootnoteRef { id, .. }
        | Inline::Mention { id, .. }
        | Inline::Equation { id, .. } => id,
    }
}

fn footnote_reference_ids(blocks: &[Block]) -> BTreeSet<StableId> {
    let mut ids = BTreeSet::new();
    collect_footnote_reference_ids(blocks, &mut ids);
    ids
}

fn collect_footnote_reference_ids(blocks: &[Block], ids: &mut BTreeSet<StableId>) {
    for block in blocks {
        for inline in &block.content {
            if let Inline::FootnoteRef { footnote_id, .. } = inline {
                ids.insert(footnote_id.clone());
            }
        }
        if let BlockKind::Table { rows } = &block.kind {
            for row in rows {
                for cell in &row.cells {
                    collect_footnote_reference_ids(&cell.blocks, ids);
                }
            }
        }
    }
}

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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Block {
    pub id: StableId,
    pub kind: BlockKind,
    pub content: Vec<Inline>,
    pub properties: Vec<Property>,
}

impl Block {
    pub fn paragraph(text: impl Into<String>) -> Self {
        Self {
            id: StableId::new("block"),
            kind: BlockKind::Paragraph,
            content: vec![Inline::text(text)],
            properties: Vec::new(),
        }
    }

    pub fn validate_isolated(&self) -> Result<(), ModelError> {
        validate_block_tree(std::slice::from_ref(self))
    }

    fn push_visible_text(&self, citations: &CitationDatabase, out: &mut String) {
        for inline in &self.content {
            inline.push_visible_text(citations, out);
        }
        match &self.kind {
            BlockKind::Table { rows } => {
                for (row_index, row) in rows.iter().enumerate() {
                    if row_index > 0 && !out.ends_with('\n') {
                        out.push('\n');
                    }
                    for (cell_index, cell) in row.cells.iter().enumerate() {
                        if cell_index > 0 {
                            out.push('\t');
                        }
                        for (block_index, block) in cell.blocks.iter().enumerate() {
                            if block_index > 0 && !out.ends_with('\n') {
                                out.push('\n');
                            }
                            block.push_visible_text(citations, out);
                        }
                    }
                }
            }
            BlockKind::EquationBlock { equation } => out.push_str(&equation.source),
            BlockKind::Image { alt_text, .. } => out.push_str(alt_text),
            _ => {}
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum BlockKind {
    Paragraph,
    Heading {
        level: u8,
    },
    ListItem {
        list_id: StableId,
        level: u8,
        ordered: bool,
    },
    Table {
        rows: Vec<TableRow>,
    },
    EquationBlock {
        equation: Equation,
    },
    Image {
        blob_hash: String,
        alt_text: String,
    },
    PageBreak,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TableRow {
    pub id: StableId,
    pub cells: Vec<TableCell>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TableCell {
    pub id: StableId,
    pub blocks: Vec<Block>,
    pub properties: Vec<Property>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Inline {
    Text {
        id: StableId,
        text: String,
        marks: Vec<Mark>,
    },
    Link {
        id: StableId,
        text: String,
        href: String,
        marks: Vec<Mark>,
    },
    Citation {
        id: StableId,
        citation_id: StableId,
        rendered_cache: Option<String>,
    },
    FootnoteRef {
        id: StableId,
        footnote_id: StableId,
    },
    Mention {
        id: StableId,
        label: String,
    },
    Equation {
        id: StableId,
        equation: Equation,
    },
}

impl Inline {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text {
            id: StableId::new("text"),
            text: text.into(),
            marks: Vec::new(),
        }
    }

    pub fn validate(&self) -> Result<(), ModelError> {
        validate_stable_id("inline id", inline_stable_id(self))?;
        validate_inline(self)
    }

    fn push_visible_text(&self, citations: &CitationDatabase, out: &mut String) {
        match self {
            Inline::Text { text, .. } | Inline::Link { text, .. } => out.push_str(text),
            Inline::Citation {
                citation_id,
                rendered_cache,
                ..
            } => {
                if let Some(rendered) = rendered_cache
                    .as_ref()
                    .or_else(|| citations.rendered_citation(citation_id))
                {
                    out.push_str(rendered);
                } else {
                    out.push('[');
                    out.push_str(citation_id.as_str());
                    out.push(']');
                }
            }
            Inline::FootnoteRef { footnote_id, .. } => {
                out.push('[');
                out.push_str(footnote_id.as_str());
                out.push(']');
            }
            Inline::Mention { label, .. } => out.push_str(label),
            Inline::Equation { equation, .. } => out.push_str(&equation.source),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Equation {
    pub id: StableId,
    pub source_format: EquationSourceFormat,
    pub source: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum EquationSourceFormat {
    LatexLike,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Mark {
    pub kind: MarkKind,
    pub value: Option<String>,
    pub expand: MarkExpand,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum MarkKind {
    Bold,
    Italic,
    Underline,
    Strike,
    Code,
    Superscript,
    Subscript,
    Color,
    Background,
    Font,
    Size,
    Link,
    Citation,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum MarkExpand {
    None,
    Start,
    End,
    Both,
}

impl Mark {
    pub fn validate(&self) -> Result<(), ModelError> {
        validate_marks(std::slice::from_ref(self))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TextRange {
    pub start: StableId,
    pub end: StableId,
}

impl TextRange {
    pub fn validate(&self) -> Result<(), ModelError> {
        validate_text_range(self)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CommentThread {
    pub id: StableId,
    pub anchor: Anchor,
    pub comments: Vec<Comment>,
    pub deleted: bool,
}

impl CommentThread {
    pub fn validate(&self) -> Result<(), ModelError> {
        validate_stable_id("comment thread id", &self.id)?;
        validate_anchor(&self.anchor)?;
        if self.comments.is_empty() {
            return Err(ModelError::InvalidDocument(
                "comment thread has no comments",
            ));
        }
        let mut comment_ids = BTreeSet::new();
        for comment in &self.comments {
            comment.validate()?;
            if !comment_ids.insert(comment.id.clone()) {
                return Err(ModelError::InvalidDocument("duplicate comment id"));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Comment {
    pub id: StableId,
    pub author: String,
    pub body: Vec<Inline>,
    pub created_at_ms: u64,
    pub deleted: bool,
}

impl Comment {
    pub fn validate(&self) -> Result<(), ModelError> {
        validate_stable_id("comment id", &self.id)?;
        if self.author.trim().is_empty() {
            return Err(ModelError::InvalidDocument("comment author is empty"));
        }
        if self.author.trim() != self.author {
            return Err(ModelError::InvalidDocument(
                "comment author has surrounding whitespace",
            ));
        }
        if self.body.is_empty() || inline_sequence_is_empty_source_text(&self.body) {
            return Err(ModelError::InvalidDocument("comment body is empty"));
        }
        validate_inline_sequence(&self.body)?;
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Anchor {
    TextRange(TextRange),
    NearestBlock { block_id: StableId, warning: String },
    Document,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Suggestion {
    pub id: StableId,
    pub author: String,
    pub kind: SuggestionKind,
    pub state: SuggestionState,
    pub provenance: Vec<String>,
}

impl Suggestion {
    pub fn validate(&self) -> Result<(), ModelError> {
        validate_stable_id("suggestion id", &self.id)?;
        if self.author.trim().is_empty() {
            return Err(ModelError::InvalidDocument("suggestion author is empty"));
        }
        if self.author.trim() != self.author {
            return Err(ModelError::InvalidDocument(
                "suggestion author has surrounding whitespace",
            ));
        }
        for item in &self.provenance {
            if item.trim().is_empty() {
                return Err(ModelError::InvalidDocument(
                    "suggestion provenance entry is empty",
                ));
            }
            if item.trim() != item {
                return Err(ModelError::InvalidDocument(
                    "suggestion provenance entry has surrounding whitespace",
                ));
            }
        }
        match &self.kind {
            SuggestionKind::Insert { anchor, content } => {
                validate_anchor(anchor)?;
                if content.is_empty() || inline_sequence_is_empty_source_text(content) {
                    return Err(ModelError::InvalidDocument(
                        "insert suggestion content is empty",
                    ));
                }
                validate_inline_sequence(content)?;
            }
            SuggestionKind::Delete { range } => validate_text_range(range)?,
            SuggestionKind::Format { range, marks } => {
                validate_text_range(range)?;
                if marks.is_empty() {
                    return Err(ModelError::InvalidDocument(
                        "format suggestion marks are empty",
                    ));
                }
                validate_marks(marks)?;
            }
        }
        Ok(())
    }
}

fn validate_anchor(anchor: &Anchor) -> Result<(), ModelError> {
    match anchor {
        Anchor::TextRange(range) => validate_text_range(range),
        Anchor::NearestBlock { block_id, warning } => {
            validate_stable_id("nearest block anchor block id", block_id)?;
            if warning.trim().is_empty() {
                return Err(ModelError::InvalidDocument(
                    "nearest block anchor warning is empty",
                ));
            }
            if warning.trim() != warning {
                return Err(ModelError::InvalidDocument(
                    "nearest block anchor warning has surrounding whitespace",
                ));
            }
            Ok(())
        }
        Anchor::Document => Ok(()),
    }
}

fn validate_text_range(range: &TextRange) -> Result<(), ModelError> {
    validate_stable_id("text range start", &range.start)?;
    validate_stable_id("text range end", &range.end)
}

fn validate_stable_id(label: &'static str, id: &StableId) -> Result<(), ModelError> {
    if id.0.trim().is_empty() {
        Err(ModelError::InvalidDocument(match label {
            "block id" => "block id is empty",
            "inline id" => "inline id is empty",
            "table row id" => "table row id is empty",
            "table cell id" => "table cell id is empty",
            "equation id" => "equation id is empty",
            "footnote id" => "footnote id is empty",
            "footnote reference id" => "footnote reference id is empty",
            "comment thread id" => "comment thread id is empty",
            "comment id" => "comment id is empty",
            "suggestion id" => "suggestion id is empty",
            "citation id" => "citation id is empty",
            "bibliography reference id" => "bibliography reference id is empty",
            "citation group id" => "citation group id is empty",
            "citation item reference id" => "citation item reference id is empty",
            "footnote citation id" => "footnote citation id is empty",
            "nearest block anchor block id" => "nearest block anchor block id is empty",
            "text range start" => "text range start is empty",
            "text range end" => "text range end is empty",
            _ => "stable id is empty",
        }))
    } else if id.0.trim() != id.0 {
        Err(ModelError::InvalidDocument(
            "stable id has surrounding whitespace",
        ))
    } else {
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum SuggestionKind {
    Insert {
        anchor: Anchor,
        content: Vec<Inline>,
    },
    Delete {
        range: TextRange,
    },
    Format {
        range: TextRange,
        marks: Vec<Mark>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum SuggestionState {
    Proposed,
    Accepted,
    Rejected,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CitationDatabase {
    pub style: String,
    pub locale: String,
    pub references: Vec<BibliographyReference>,
    pub citations: Vec<CitationGroup>,
}

impl Default for CitationDatabase {
    fn default() -> Self {
        Self {
            style: "apa-7th".to_string(),
            locale: "en-US".to_string(),
            references: Vec::new(),
            citations: Vec::new(),
        }
    }
}

impl CitationDatabase {
    pub fn validate(&self, live_footnotes: &BTreeSet<StableId>) -> Result<(), ModelError> {
        if self.style.trim().is_empty() {
            return Err(ModelError::InvalidDocument("citation style is empty"));
        }
        if self.style.trim() != self.style {
            return Err(ModelError::InvalidDocument(
                "citation style has surrounding whitespace",
            ));
        }
        if self.locale.trim().is_empty() {
            return Err(ModelError::InvalidDocument("citation locale is empty"));
        }
        if self.locale.trim() != self.locale {
            return Err(ModelError::InvalidDocument(
                "citation locale has surrounding whitespace",
            ));
        }

        let mut reference_ids = BTreeSet::new();
        for reference in &self.references {
            reference.validate()?;
            if !reference_ids.insert(reference.id.clone()) {
                return Err(ModelError::InvalidDocument(
                    "duplicate bibliography reference id",
                ));
            }
        }

        let mut citation_ids = BTreeSet::new();
        for citation in &self.citations {
            citation.validate(live_footnotes)?;
            if !citation_ids.insert(citation.id.clone()) {
                return Err(ModelError::InvalidDocument("duplicate citation group id"));
            }
        }

        Ok(())
    }

    pub fn upsert_reference(&mut self, reference: BibliographyReference) {
        if let Some(existing) = self
            .references
            .iter_mut()
            .find(|item| item.id == reference.id)
        {
            if reference.revision >= existing.revision {
                *existing = reference;
            }
        } else {
            self.references.push(reference);
            self.references
                .sort_by(|left, right| left.id.cmp(&right.id));
        }
    }

    pub fn upsert_citation(&mut self, citation: CitationGroup) {
        if let Some(existing) = self
            .citations
            .iter_mut()
            .find(|item| item.id == citation.id)
        {
            if citation.revision >= existing.revision {
                *existing = citation;
            }
        } else {
            self.citations.push(citation);
            self.citations.sort_by(|left, right| left.id.cmp(&right.id));
        }
    }

    pub fn delete_reference(&mut self, reference_id: &StableId, revision: u64) -> bool {
        if let Some(reference) = self
            .references
            .iter_mut()
            .find(|item| &item.id == reference_id)
        {
            if revision >= reference.revision {
                reference.revision = revision;
                reference.deleted = true;
            }
            true
        } else {
            false
        }
    }

    pub fn delete_citation(&mut self, citation_id: &StableId, revision: u64) -> bool {
        if let Some(citation) = self
            .citations
            .iter_mut()
            .find(|item| &item.id == citation_id)
        {
            if revision >= citation.revision {
                citation.revision = revision;
                citation.deleted = true;
                citation.rendered_cache = None;
            }
            true
        } else {
            false
        }
    }

    pub fn rendered_citation(&self, citation_id: &StableId) -> Option<&String> {
        self.citations
            .iter()
            .find(|item| &item.id == citation_id && !item.deleted)
            .and_then(|item| item.rendered_cache.as_ref())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BibliographyReference {
    pub id: StableId,
    pub revision: u64,
    pub source: CitationSource,
    pub summary: CitationSummary,
    pub deleted: bool,
}

impl BibliographyReference {
    pub fn validate(&self) -> Result<(), ModelError> {
        validate_stable_id("bibliography reference id", &self.id)?;
        if self
            .source
            .bytes
            .iter()
            .all(|byte| byte.is_ascii_whitespace())
        {
            return Err(ModelError::InvalidDocument("bibliography source is empty"));
        }
        if let CitationSourceFormat::Unknown(value) = &self.source.format {
            if value.trim().is_empty() {
                return Err(ModelError::InvalidDocument(
                    "bibliography source format is empty",
                ));
            }
            if value.trim() != value {
                return Err(ModelError::InvalidDocument(
                    "bibliography source format has surrounding whitespace",
                ));
            }
        }
        if self.summary.title.trim() != self.summary.title {
            return Err(ModelError::InvalidDocument(
                "bibliography summary field has surrounding whitespace",
            ));
        }
        for author in &self.summary.authors {
            if author.trim().is_empty() {
                return Err(ModelError::InvalidDocument(
                    "bibliography summary field is empty",
                ));
            }
            if author.trim() != author {
                return Err(ModelError::InvalidDocument(
                    "bibliography summary field has surrounding whitespace",
                ));
            }
        }
        for value in [
            self.summary.issued.as_deref(),
            self.summary.doi.as_deref(),
            self.summary.url.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            if value.trim().is_empty() {
                return Err(ModelError::InvalidDocument(
                    "bibliography summary field is empty",
                ));
            }
            if value.trim() != value {
                return Err(ModelError::InvalidDocument(
                    "bibliography summary field has surrounding whitespace",
                ));
            }
        }
        if self.summary.title.trim().is_empty()
            && self.summary.authors.is_empty()
            && self.summary.issued.is_none()
            && self.summary.doi.is_none()
            && self.summary.url.is_none()
        {
            return Err(ModelError::InvalidDocument("bibliography summary is empty"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CitationSource {
    pub format: CitationSourceFormat,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum CitationSourceFormat {
    CitumNative,
    CslJson,
    Bibtex,
    Ris,
    Unknown(String),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CitationSummary {
    pub title: String,
    pub authors: Vec<String>,
    pub issued: Option<String>,
    pub doi: Option<String>,
    pub url: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CitationGroup {
    pub id: StableId,
    pub revision: u64,
    pub items: Vec<CitationItem>,
    pub placement: CitationPlacement,
    pub rendered_cache: Option<String>,
    pub deleted: bool,
}

impl CitationGroup {
    pub fn validate_payload(&self) -> Result<(), ModelError> {
        validate_stable_id("citation group id", &self.id)?;
        if self.items.is_empty() {
            return Err(ModelError::InvalidDocument("citation group has no items"));
        }
        for item in &self.items {
            item.validate()?;
        }
        Ok(())
    }

    fn validate(&self, live_footnotes: &BTreeSet<StableId>) -> Result<(), ModelError> {
        self.validate_payload()?;
        if let CitationPlacement::Footnote { footnote_id } = &self.placement {
            validate_stable_id("footnote citation id", footnote_id)?;
            if !self.deleted && !live_footnotes.contains(footnote_id) {
                return Err(ModelError::InvalidDocument(
                    "footnote citation target is missing",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CitationItem {
    pub reference_id: StableId,
    pub locator: Option<String>,
    pub label: Option<String>,
    pub prefix: Option<String>,
    pub suffix: Option<String>,
    pub suppress_author: bool,
}

impl CitationItem {
    pub fn validate(&self) -> Result<(), ModelError> {
        validate_stable_id("citation item reference id", &self.reference_id)?;
        for value in [
            self.locator.as_deref(),
            self.label.as_deref(),
            self.prefix.as_deref(),
            self.suffix.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            if value.trim().is_empty() {
                return Err(ModelError::InvalidDocument("citation item field is empty"));
            }
            if value.trim() != value {
                return Err(ModelError::InvalidDocument(
                    "citation item field has surrounding whitespace",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum CitationPlacement {
    Inline,
    Footnote { footnote_id: StableId },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Property {
    pub key: String,
    pub value: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ModelWarning {
    pub code: String,
    pub message: String,
}

impl ModelWarning {
    pub fn validate(&self) -> Result<(), ModelError> {
        if self.code.trim().is_empty() {
            return Err(ModelError::InvalidDocument("warning code is empty"));
        }
        if self.code.trim() != self.code {
            return Err(ModelError::InvalidDocument(
                "warning code has surrounding whitespace",
            ));
        }
        if self.message.trim().is_empty() {
            return Err(ModelError::InvalidDocument("warning message is empty"));
        }
        if self.message.trim() != self.message {
            return Err(ModelError::InvalidDocument(
                "warning message has surrounding whitespace",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ModelError {
    InvalidId(&'static str),
    InvalidHash,
    UnsupportedHashAlgorithm,
    InvalidDocument(&'static str),
}

impl fmt::Display for ModelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModelError::InvalidId(message) => f.write_str(message),
            ModelError::InvalidHash => f.write_str("invalid hash reference"),
            ModelError::UnsupportedHashAlgorithm => f.write_str("unsupported hash algorithm"),
            ModelError::InvalidDocument(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for ModelError {}

#[cfg(not(target_arch = "wasm32"))]
fn now_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0)
}

#[cfg(target_arch = "wasm32")]
fn now_nanos() -> u128 {
    (js_sys::Date::now() * 1_000_000.0) as u128
}

fn next_counter() -> u64 {
    ID_COUNTER.fetch_add(1, Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_ref_requires_algorithm() {
        let hash = HashRef::parse("sha256:abc").unwrap();
        assert_eq!(hash.algorithm(), "sha256");
        assert_eq!(hash.digest(), "abc");
        assert_eq!(hash.to_string(), "sha256:abc");
        assert!(HashRef::parse("abc").is_err());
    }

    #[test]
    fn hash_ref_rejects_path_hostile_components() {
        for value in [
            "sha256:",
            ":abc",
            " sha256:abc",
            "sha256:abc ",
            "sha/256:abc",
            "sha256:../abc",
            "sha256:abc/def",
            "sha256:abc\\def",
            "sha256:abc:def",
            ".:abc",
            "..:abc",
            "sha256:.",
            "sha256:..",
            "sha256:abc\ndef",
        ] {
            assert!(HashRef::parse(value).is_err(), "{value:?} parsed");
        }

        let hash = HashRef::parse("blake3-v1:abc_DEF-123.xyz").unwrap();
        assert_eq!(hash.to_string(), "blake3-v1:abc_DEF-123.xyz");
    }

    #[test]
    fn stable_id_parse_rejects_surrounding_whitespace() {
        assert_eq!(StableId::parse("block-1").unwrap().as_str(), "block-1");
        assert!(matches!(
            StableId::parse(" block-1 "),
            Err(ModelError::InvalidId(
                "stable id has surrounding whitespace"
            ))
        ));
    }

    #[test]
    fn document_uuid_parse_rejects_surrounding_whitespace() {
        assert_eq!(DocumentUuid::parse("doc-1").unwrap().as_str(), "doc-1");
        assert!(matches!(
            DocumentUuid::parse(" doc-1 "),
            Err(ModelError::InvalidId(
                "document uuid has surrounding whitespace"
            ))
        ));
    }

    #[test]
    fn digest_bytes_is_algorithm_explicit() {
        let hash = digest_bytes("sha256", b"hello").unwrap();
        assert_eq!(hash.algorithm(), "sha256");
        assert_eq!(
            hash.digest(),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
        assert!(digest_bytes("unknown", b"hello").is_err());
    }

    #[test]
    fn minimal_document_is_valid() {
        let mut doc = Document::new("Example");
        doc.blocks.push(Block::paragraph("hello"));
        doc.validate().unwrap();
        assert_eq!(doc.visible_text(), "hello\n");
        doc.title = " Example ".to_string();
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument(
                "title has surrounding whitespace"
            ))
        ));
        doc.title = "Example".to_string();
        doc.locale = " ".to_string();
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument("document locale is empty"))
        ));
        doc.locale = " en-US ".to_string();
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument(
                "document locale has surrounding whitespace"
            ))
        ));
        doc.locale = "en-US".to_string();
        doc.doi = Some(" ".to_string());
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument("document DOI is empty"))
        ));
        doc.doi = Some(" 10.123/example ".to_string());
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument(
                "document DOI has surrounding whitespace"
            ))
        ));
    }

    #[test]
    fn warning_records_require_auditable_payloads() {
        let mut doc = Document::new("Warnings");
        doc.blocks.push(Block::paragraph("body"));
        doc.warnings.push(ModelWarning {
            code: "degraded-import".to_string(),
            message: "unsupported imported field was ignored".to_string(),
        });
        doc.validate().unwrap();

        doc.warnings[0].code = " ".to_string();
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument("warning code is empty"))
        ));

        doc.warnings[0].code = " degraded-import ".to_string();
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument(
                "warning code has surrounding whitespace"
            ))
        ));

        doc.warnings[0].code = "degraded-import".to_string();
        doc.warnings[0].message.clear();
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument("warning message is empty"))
        ));

        doc.warnings[0].message = " unsupported imported field was ignored ".to_string();
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument(
                "warning message has surrounding whitespace"
            ))
        ));
    }

    #[test]
    fn document_structure_rejects_duplicate_operation_ids() {
        let duplicate_block_id = StableId::parse("block-duplicate").unwrap();
        let duplicate_inline_id = StableId::parse("text-duplicate").unwrap();
        let mut doc = Document::new("Duplicate IDs");
        doc.blocks.push(Block {
            id: duplicate_block_id.clone(),
            kind: BlockKind::Paragraph,
            content: vec![Inline::Text {
                id: StableId::parse("text-1").unwrap(),
                text: "first".to_string(),
                marks: Vec::new(),
            }],
            properties: Vec::new(),
        });
        doc.blocks.push(Block {
            id: duplicate_block_id,
            kind: BlockKind::Paragraph,
            content: vec![Inline::Text {
                id: StableId::parse("text-2").unwrap(),
                text: "second".to_string(),
                marks: Vec::new(),
            }],
            properties: Vec::new(),
        });
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument("duplicate block id"))
        ));

        doc.blocks[1].id = StableId::parse("block-2").unwrap();
        doc.blocks[0].content = vec![Inline::Text {
            id: duplicate_inline_id.clone(),
            text: "first".to_string(),
            marks: Vec::new(),
        }];
        doc.blocks[1].content = vec![Inline::Text {
            id: duplicate_inline_id,
            text: "second".to_string(),
            marks: Vec::new(),
        }];
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument("duplicate inline id"))
        ));
    }

    #[test]
    fn source_model_rejects_empty_stable_ids_after_decode() {
        let mut doc = Document::new("Stable IDs");
        doc.uuid = DocumentUuid(String::new());
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument("document uuid is empty"))
        ));

        doc.uuid = DocumentUuid(" doc-stable ".to_string());
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument(
                "document uuid has surrounding whitespace"
            ))
        ));

        doc.uuid = DocumentUuid::parse("doc-stable").unwrap();
        doc.blocks.push(Block {
            id: StableId(String::new()),
            kind: BlockKind::Paragraph,
            content: vec![Inline::text("body")],
            properties: Vec::new(),
        });
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument("block id is empty"))
        ));

        doc.blocks[0].id = StableId::parse("block-1").unwrap();
        doc.blocks[0].content = vec![Inline::Text {
            id: StableId(String::new()),
            text: "body".to_string(),
            marks: Vec::new(),
        }];
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument("inline id is empty"))
        ));

        doc.blocks[0].content = vec![Inline::Text {
            id: StableId(" text-1 ".to_string()),
            text: "body".to_string(),
            marks: Vec::new(),
        }];
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument(
                "stable id has surrounding whitespace"
            ))
        ));

        doc.blocks[0].content = vec![Inline::Citation {
            id: StableId::parse("citation-label").unwrap(),
            citation_id: StableId(String::new()),
            rendered_cache: None,
        }];
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument("citation id is empty"))
        ));

        doc.blocks[0].content = vec![Inline::Equation {
            id: StableId::parse("inline-equation").unwrap(),
            equation: Equation {
                id: StableId(String::new()),
                source_format: EquationSourceFormat::LatexLike,
                source: "x^2".to_string(),
            },
        }];
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument("equation id is empty"))
        ));

        doc.blocks[0] = Block {
            id: StableId::parse("table-1").unwrap(),
            kind: BlockKind::Table {
                rows: vec![TableRow {
                    id: StableId(String::new()),
                    cells: vec![TableCell {
                        id: StableId::parse("cell-1").unwrap(),
                        blocks: vec![Block::paragraph("cell")],
                        properties: Vec::new(),
                    }],
                }],
            },
            content: Vec::new(),
            properties: Vec::new(),
        };
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument("table row id is empty"))
        ));

        doc.blocks.clear();
        doc.blocks.push(Block::paragraph("body"));
        doc.citation_database.style = " apa-7th ".to_string();
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument(
                "citation style has surrounding whitespace"
            ))
        ));

        doc.citation_database.style = "apa-7th".to_string();
        doc.citation_database.locale = " en-US ".to_string();
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument(
                "citation locale has surrounding whitespace"
            ))
        ));

        doc.citation_database.locale = "en-US".to_string();
        doc.citation_database
            .upsert_reference(BibliographyReference {
                id: StableId(String::new()),
                revision: 1,
                source: CitationSource {
                    format: CitationSourceFormat::CitumNative,
                    bytes: b"title: Source".to_vec(),
                },
                summary: CitationSummary {
                    title: "Source".to_string(),
                    authors: Vec::new(),
                    issued: None,
                    doi: None,
                    url: None,
                },
                deleted: false,
            });
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument(
                "bibliography reference id is empty"
            ))
        ));

        doc.citation_database.references.clear();
        doc.citation_database.upsert_citation(CitationGroup {
            id: StableId::parse("citation-group").unwrap(),
            revision: 1,
            items: vec![CitationItem {
                reference_id: StableId(String::new()),
                locator: None,
                label: None,
                prefix: None,
                suffix: None,
                suppress_author: false,
            }],
            placement: CitationPlacement::Inline,
            rendered_cache: None,
            deleted: false,
        });
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument(
                "citation item reference id is empty"
            ))
        ));

        doc.citation_database.citations[0].items[0].reference_id =
            StableId::parse("ref-missing").unwrap();
        doc.citation_database.citations[0].placement = CitationPlacement::Footnote {
            footnote_id: StableId(String::new()),
        };
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument("footnote citation id is empty"))
        ));
    }

    #[test]
    fn table_structure_rejects_duplicate_row_and_cell_ids() {
        let row_id = StableId::parse("row-duplicate").unwrap();
        let cell_id = StableId::parse("cell-duplicate").unwrap();
        let mut doc = Document::new("Duplicate table IDs");
        doc.blocks.push(Block {
            id: StableId::parse("table-1").unwrap(),
            kind: BlockKind::Table {
                rows: vec![
                    TableRow {
                        id: row_id.clone(),
                        cells: vec![TableCell {
                            id: StableId::parse("cell-1").unwrap(),
                            blocks: vec![Block::paragraph("a")],
                            properties: Vec::new(),
                        }],
                    },
                    TableRow {
                        id: row_id,
                        cells: vec![TableCell {
                            id: StableId::parse("cell-2").unwrap(),
                            blocks: vec![Block::paragraph("b")],
                            properties: Vec::new(),
                        }],
                    },
                ],
            },
            content: Vec::new(),
            properties: Vec::new(),
        });
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument("duplicate table row id"))
        ));

        if let BlockKind::Table { rows } = &mut doc.blocks[0].kind {
            rows[1].id = StableId::parse("row-2").unwrap();
            rows[0].cells[0].id = cell_id.clone();
            rows[0].cells.push(TableCell {
                id: cell_id,
                blocks: vec![Block::paragraph("c")],
                properties: Vec::new(),
            });
        }
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument("duplicate table cell id"))
        ));
    }

    #[test]
    fn table_structure_rejects_empty_table_shapes() {
        let mut doc = Document::new("Empty table");
        doc.blocks.push(Block {
            id: StableId::parse("table-empty").unwrap(),
            kind: BlockKind::Table { rows: Vec::new() },
            content: Vec::new(),
            properties: Vec::new(),
        });
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument("table has no rows"))
        ));

        doc.blocks[0].kind = BlockKind::Table {
            rows: vec![TableRow {
                id: StableId::parse("row-empty").unwrap(),
                cells: Vec::new(),
            }],
        };
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument("table row has no cells"))
        ));

        doc.blocks[0].kind = BlockKind::Table {
            rows: vec![TableRow {
                id: StableId::parse("row-with-empty-cell").unwrap(),
                cells: vec![TableCell {
                    id: StableId::parse("cell-empty").unwrap(),
                    blocks: Vec::new(),
                    properties: Vec::new(),
                }],
            }],
        };
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument("table cell has no blocks"))
        ));
    }

    #[test]
    fn structured_nodes_reject_empty_or_invalid_payloads() {
        let mut doc = Document::new("Structured payloads");
        doc.blocks.push(Block {
            id: StableId::parse("link-block").unwrap(),
            kind: BlockKind::Paragraph,
            content: vec![Inline::Link {
                id: StableId::parse("link-empty").unwrap(),
                text: "link".to_string(),
                href: String::new(),
                marks: Vec::new(),
            }],
            properties: Vec::new(),
        });
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument("link href is empty"))
        ));

        doc.blocks[0].content = vec![Inline::Mention {
            id: StableId::parse("mention-empty").unwrap(),
            label: " ".to_string(),
        }];
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument("mention label is empty"))
        ));

        doc.blocks[0].content = vec![Inline::Equation {
            id: StableId::parse("inline-equation-empty").unwrap(),
            equation: Equation {
                id: StableId::parse("equation-empty").unwrap(),
                source_format: EquationSourceFormat::LatexLike,
                source: String::new(),
            },
        }];
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument("equation source is empty"))
        ));

        doc.blocks[0].content = vec![Inline::Equation {
            id: StableId::parse("inline-equation-padded").unwrap(),
            equation: Equation {
                id: StableId::parse("equation-padded").unwrap(),
                source_format: EquationSourceFormat::LatexLike,
                source: " x=1 ".to_string(),
            },
        }];
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument(
                "equation source has surrounding whitespace"
            ))
        ));

        doc.blocks[0] = Block {
            id: StableId::parse("block-equation-empty").unwrap(),
            kind: BlockKind::EquationBlock {
                equation: Equation {
                    id: StableId::parse("block-equation").unwrap(),
                    source_format: EquationSourceFormat::LatexLike,
                    source: " ".to_string(),
                },
            },
            content: Vec::new(),
            properties: Vec::new(),
        };
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument("equation source is empty"))
        ));

        doc.blocks[0] = Block {
            id: StableId::parse("block-equation-padded").unwrap(),
            kind: BlockKind::EquationBlock {
                equation: Equation {
                    id: StableId::parse("block-equation-padded-source").unwrap(),
                    source_format: EquationSourceFormat::LatexLike,
                    source: " y=1 ".to_string(),
                },
            },
            content: Vec::new(),
            properties: Vec::new(),
        };
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument(
                "equation source has surrounding whitespace"
            ))
        ));

        doc.blocks[0] = Block {
            id: StableId::parse("image-bad-hash").unwrap(),
            kind: BlockKind::Image {
                blob_hash: "not-a-hash".to_string(),
                alt_text: "image".to_string(),
            },
            content: Vec::new(),
            properties: Vec::new(),
        };
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument("image blob hash is invalid"))
        ));

        doc.blocks[0] = Block {
            id: StableId::parse("image-padded-hash").unwrap(),
            kind: BlockKind::Image {
                blob_hash: " sha256:abc ".to_string(),
                alt_text: "image".to_string(),
            },
            content: Vec::new(),
            properties: Vec::new(),
        };
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument("image blob hash is invalid"))
        ));

        doc.blocks[0] = Block {
            id: StableId::parse("heading-bad-level").unwrap(),
            kind: BlockKind::Heading { level: 0 },
            content: vec![Inline::text("bad heading")],
            properties: Vec::new(),
        };
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument(
                "heading level is outside 1..=6"
            ))
        ));

        doc.blocks[0] = Block {
            id: StableId::parse("list-bad-level").unwrap(),
            kind: BlockKind::ListItem {
                list_id: StableId::parse("list-main").unwrap(),
                level: 9,
                ordered: false,
            },
            content: vec![Inline::text("bad list item")],
            properties: Vec::new(),
        };
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument(
                "list item level is outside 0..=8"
            ))
        ));
    }

    #[test]
    fn mark_payloads_require_consistent_values() {
        let mut doc = Document::new("Marks");
        doc.blocks.push(Block::paragraph("marked"));
        let Inline::Text { marks, .. } = &mut doc.blocks[0].content[0] else {
            unreachable!();
        };
        marks.push(Mark {
            kind: MarkKind::Color,
            value: None,
            expand: MarkExpand::Both,
        });
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument("mark value is missing"))
        ));

        let Inline::Text { marks, .. } = &mut doc.blocks[0].content[0] else {
            unreachable!();
        };
        marks[0].value = Some(" ".to_string());
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument("mark value is empty"))
        ));

        let Inline::Text { marks, .. } = &mut doc.blocks[0].content[0] else {
            unreachable!();
        };
        marks[0] = Mark {
            kind: MarkKind::Bold,
            value: Some("true".to_string()),
            expand: MarkExpand::Both,
        };
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument("boolean mark has value"))
        ));

        let Inline::Text { marks, .. } = &mut doc.blocks[0].content[0] else {
            unreachable!();
        };
        marks[0] = Mark {
            kind: MarkKind::Font,
            value: Some("Inter".to_string()),
            expand: MarkExpand::Both,
        };
        doc.validate().unwrap();
    }

    #[test]
    fn retained_inline_bodies_use_same_payload_validation() {
        let mut doc = Document::new("Retained bodies");
        doc.comments.push(CommentThread {
            id: StableId::parse("thread-1").unwrap(),
            anchor: Anchor::Document,
            comments: vec![Comment {
                id: StableId::parse("comment-1").unwrap(),
                author: "Reviewer".to_string(),
                body: vec![Inline::Mention {
                    id: StableId::parse("mention-empty").unwrap(),
                    label: String::new(),
                }],
                created_at_ms: 1,
                deleted: false,
            }],
            deleted: false,
        });
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument("mention label is empty"))
        ));

        doc.comments.clear();
        doc.suggestions.push(Suggestion {
            id: StableId::parse("suggestion-1").unwrap(),
            author: "Reviewer".to_string(),
            kind: SuggestionKind::Insert {
                anchor: Anchor::Document,
                content: vec![Inline::Equation {
                    id: StableId::parse("inline-equation-empty").unwrap(),
                    equation: Equation {
                        id: StableId::parse("equation-empty").unwrap(),
                        source_format: EquationSourceFormat::LatexLike,
                        source: String::new(),
                    },
                }],
            },
            state: SuggestionState::Proposed,
            provenance: Vec::new(),
        });
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument("equation source is empty"))
        ));

        doc.suggestions[0].kind = SuggestionKind::Format {
            range: TextRange {
                start: StableId::parse("text-a").unwrap(),
                end: StableId::parse("text-b").unwrap(),
            },
            marks: vec![Mark {
                kind: MarkKind::Bold,
                value: Some("true".to_string()),
                expand: MarkExpand::Both,
            }],
        };
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument("boolean mark has value"))
        ));
    }

    #[test]
    fn retained_record_collections_reject_duplicate_ids() {
        let mut doc = Document::new("Duplicate retained IDs");
        let footnote_id = StableId::parse("footnote-duplicate").unwrap();
        doc.footnotes.push(Footnote {
            id: footnote_id.clone(),
            revision: 1,
            body: vec![Inline::text("first")],
            deleted: true,
        });
        doc.footnotes.push(Footnote {
            id: footnote_id,
            revision: 2,
            body: vec![Inline::text("second")],
            deleted: true,
        });
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument("duplicate footnote id"))
        ));

        doc.footnotes.clear();
        let thread_id = StableId::parse("comment-thread-duplicate").unwrap();
        for body in ["first", "second"] {
            doc.comments.push(CommentThread {
                id: thread_id.clone(),
                anchor: Anchor::Document,
                comments: vec![Comment {
                    id: StableId::new("comment"),
                    author: "Reviewer".to_string(),
                    body: vec![Inline::text(body)],
                    created_at_ms: 1,
                    deleted: false,
                }],
                deleted: false,
            });
        }
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument("duplicate comment thread id"))
        ));

        doc.comments.clear();
        let suggestion_id = StableId::parse("suggestion-duplicate").unwrap();
        for text in ["first", "second"] {
            doc.suggestions.push(Suggestion {
                id: suggestion_id.clone(),
                author: "Reviewer".to_string(),
                kind: SuggestionKind::Insert {
                    anchor: Anchor::Document,
                    content: vec![Inline::text(text)],
                },
                state: SuggestionState::Proposed,
                provenance: Vec::new(),
            });
        }
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument("duplicate suggestion id"))
        ));
    }

    #[test]
    fn footnote_references_require_live_targets() {
        let footnote_id = StableId::parse("footnote-1").unwrap();
        let mut doc = Document::new("Footnotes");
        doc.footnotes.push(Footnote {
            id: footnote_id.clone(),
            revision: 1,
            body: vec![Inline::text("footnote body")],
            deleted: false,
        });
        doc.blocks.push(Block {
            id: StableId::new("table"),
            kind: BlockKind::Table {
                rows: vec![TableRow {
                    id: StableId::new("row"),
                    cells: vec![TableCell {
                        id: StableId::new("cell"),
                        blocks: vec![Block {
                            id: StableId::new("cell-block"),
                            kind: BlockKind::Paragraph,
                            content: vec![Inline::FootnoteRef {
                                id: StableId::new("footnote-ref"),
                                footnote_id: footnote_id.clone(),
                            }],
                            properties: Vec::new(),
                        }],
                        properties: Vec::new(),
                    }],
                }],
            },
            content: Vec::new(),
            properties: Vec::new(),
        });
        doc.validate().unwrap();

        doc.footnotes[0].deleted = true;
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument(
                "footnote reference target is missing"
            ))
        ));
    }

    #[test]
    fn footnotes_require_non_empty_source_body() {
        let mut doc = Document::new("Footnotes");
        doc.footnotes.push(Footnote {
            id: StableId::parse("footnote-empty").unwrap(),
            revision: 1,
            body: vec![Inline::text(" ")],
            deleted: false,
        });

        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument("footnote body is empty"))
        ));
    }

    #[test]
    fn missing_footnote_reference_target_is_invalid() {
        let mut doc = Document::new("Footnotes");
        doc.blocks.push(Block {
            id: StableId::new("block"),
            kind: BlockKind::Paragraph,
            content: vec![Inline::FootnoteRef {
                id: StableId::new("footnote-ref"),
                footnote_id: StableId::parse("missing-footnote").unwrap(),
            }],
            properties: Vec::new(),
        });

        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument(
                "footnote reference target is missing"
            ))
        ));
    }

    #[test]
    fn comment_threads_require_auditable_comments() {
        let mut doc = Document::new("Comments");
        doc.blocks.push(Block::paragraph("body"));
        doc.comments.push(CommentThread {
            id: StableId::parse("comment-thread").unwrap(),
            anchor: Anchor::Document,
            comments: vec![Comment {
                id: StableId::parse("comment-1").unwrap(),
                author: "Reviewer".to_string(),
                body: vec![Inline::text("Review note")],
                created_at_ms: 1,
                deleted: false,
            }],
            deleted: false,
        });
        doc.validate().unwrap();

        doc.comments[0].comments[0].author = " ".to_string();
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument("comment author is empty"))
        ));
        doc.comments[0].comments[0].author = "Reviewer".to_string();
        doc.comments[0].comments[0].body.clear();
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument("comment body is empty"))
        ));
        doc.comments[0].comments[0].body = vec![Inline::text(" ")];
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument("comment body is empty"))
        ));
    }

    #[test]
    fn comment_threads_reject_duplicate_comment_ids() {
        let mut doc = Document::new("Comments");
        let comment_id = StableId::parse("comment-1").unwrap();
        doc.comments.push(CommentThread {
            id: StableId::parse("comment-thread").unwrap(),
            anchor: Anchor::Document,
            comments: vec![
                Comment {
                    id: comment_id.clone(),
                    author: "Reviewer".to_string(),
                    body: vec![Inline::text("First")],
                    created_at_ms: 1,
                    deleted: false,
                },
                Comment {
                    id: comment_id,
                    author: "Reviewer".to_string(),
                    body: vec![Inline::text("Second")],
                    created_at_ms: 2,
                    deleted: true,
                },
            ],
            deleted: false,
        });

        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument("duplicate comment id"))
        ));
    }

    #[test]
    fn comment_and_suggestion_anchors_require_auditable_payloads() {
        let mut doc = Document::new("Anchors");
        doc.blocks.push(Block::paragraph("body"));
        doc.comments.push(CommentThread {
            id: StableId::parse("comment-thread").unwrap(),
            anchor: Anchor::NearestBlock {
                block_id: StableId::parse("block-retained").unwrap(),
                warning: "anchor moved after deletion".to_string(),
            },
            comments: vec![Comment {
                id: StableId::parse("comment-1").unwrap(),
                author: "Reviewer".to_string(),
                body: vec![Inline::text("Review note")],
                created_at_ms: 1,
                deleted: false,
            }],
            deleted: false,
        });
        doc.validate().unwrap();

        doc.comments[0].comments[0].author = " Reviewer ".to_string();
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument(
                "comment author has surrounding whitespace"
            ))
        ));
        doc.comments[0].comments[0].author = "Reviewer".to_string();

        if let Anchor::NearestBlock { warning, .. } = &mut doc.comments[0].anchor {
            warning.clear();
        }
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument(
                "nearest block anchor warning is empty"
            ))
        ));

        if let Anchor::NearestBlock { warning, .. } = &mut doc.comments[0].anchor {
            *warning = " moved after delete ".to_string();
        }
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument(
                "nearest block anchor warning has surrounding whitespace"
            ))
        ));

        doc.comments.clear();
        doc.suggestions.push(Suggestion {
            id: StableId::parse("suggestion-1").unwrap(),
            author: "Reviewer".to_string(),
            kind: SuggestionKind::Delete {
                range: TextRange {
                    start: StableId(String::new()),
                    end: StableId::parse("text-end").unwrap(),
                },
            },
            state: SuggestionState::Proposed,
            provenance: Vec::new(),
        });
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument("text range start is empty"))
        ));

        doc.suggestions[0].kind = SuggestionKind::Insert {
            anchor: Anchor::NearestBlock {
                block_id: StableId(String::new()),
                warning: "anchor moved after deletion".to_string(),
            },
            content: vec![Inline::text("inserted")],
        };
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument(
                "nearest block anchor block id is empty"
            ))
        ));
    }

    #[test]
    fn suggestions_require_auditable_payloads() {
        let mut doc = Document::new("Suggestions");
        doc.suggestions.push(Suggestion {
            id: StableId::parse("suggestion-1").unwrap(),
            author: "Reviewer".to_string(),
            kind: SuggestionKind::Insert {
                anchor: Anchor::Document,
                content: Vec::new(),
            },
            state: SuggestionState::Proposed,
            provenance: Vec::new(),
        });
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument(
                "insert suggestion content is empty"
            ))
        ));
        doc.suggestions[0].kind = SuggestionKind::Insert {
            anchor: Anchor::Document,
            content: vec![Inline::text(" ")],
        };
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument(
                "insert suggestion content is empty"
            ))
        ));

        doc.suggestions[0].kind = SuggestionKind::Format {
            range: TextRange {
                start: StableId::parse("text-1").unwrap(),
                end: StableId::parse("text-1").unwrap(),
            },
            marks: Vec::new(),
        };
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument(
                "format suggestion marks are empty"
            ))
        ));

        doc.suggestions[0].kind = SuggestionKind::Insert {
            anchor: Anchor::Document,
            content: vec![Inline::text("suggested text")],
        };
        doc.suggestions[0].provenance = vec![" ".to_string()];
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument(
                "suggestion provenance entry is empty"
            ))
        ));

        doc.suggestions[0].provenance = vec![" imported ".to_string()];
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument(
                "suggestion provenance entry has surrounding whitespace"
            ))
        ));

        doc.suggestions[0].provenance = Vec::new();
        doc.suggestions[0].author = " Reviewer ".to_string();
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument(
                "suggestion author has surrounding whitespace"
            ))
        ));
    }

    #[test]
    fn citation_labels_render_from_document_local_database() {
        let mut doc = Document::new("Example");
        let reference_id = StableId::parse("ref-doe-2020").unwrap();
        let citation_id = StableId::parse("cite-intro").unwrap();
        doc.citation_database
            .upsert_reference(BibliographyReference {
                id: reference_id.clone(),
                revision: 1,
                source: CitationSource {
                    format: CitationSourceFormat::CitumNative,
                    bytes: b"id: doe-2020\ntitle: Example".to_vec(),
                },
                summary: CitationSummary {
                    title: "Example".to_string(),
                    authors: vec!["Doe".to_string()],
                    issued: Some("2020".to_string()),
                    doi: None,
                    url: None,
                },
                deleted: false,
            });
        doc.citation_database.upsert_citation(CitationGroup {
            id: citation_id.clone(),
            revision: 1,
            items: vec![CitationItem {
                reference_id,
                locator: Some("42".to_string()),
                label: Some("page".to_string()),
                prefix: Some("see".to_string()),
                suffix: None,
                suppress_author: false,
            }],
            placement: CitationPlacement::Inline,
            rendered_cache: Some("(see Doe 2020, 42)".to_string()),
            deleted: false,
        });
        doc.blocks.push(Block {
            id: StableId::new("block"),
            kind: BlockKind::Paragraph,
            content: vec![Inline::Citation {
                id: StableId::new("citation-label"),
                citation_id,
                rendered_cache: None,
            }],
            properties: Vec::new(),
        });

        assert_eq!(doc.visible_text(), "(see Doe 2020, 42)\n");
    }

    #[test]
    fn citation_groups_require_items_but_not_live_bibliography_targets() {
        let mut doc = Document::new("Citations");
        let reference_id = StableId::parse("deleted-ref").unwrap();
        let citation_id = StableId::parse("cite-deleted-ref").unwrap();
        doc.citation_database
            .upsert_reference(BibliographyReference {
                id: reference_id.clone(),
                revision: 1,
                source: CitationSource {
                    format: CitationSourceFormat::CitumNative,
                    bytes: b"title: Retired".to_vec(),
                },
                summary: CitationSummary {
                    title: "Retired".to_string(),
                    authors: Vec::new(),
                    issued: None,
                    doi: None,
                    url: None,
                },
                deleted: true,
            });
        doc.citation_database.upsert_citation(CitationGroup {
            id: citation_id.clone(),
            revision: 1,
            items: vec![CitationItem {
                reference_id,
                locator: None,
                label: None,
                prefix: None,
                suffix: None,
                suppress_author: false,
            }],
            placement: CitationPlacement::Inline,
            rendered_cache: None,
            deleted: false,
        });
        doc.blocks.push(Block {
            id: StableId::new("block"),
            kind: BlockKind::Paragraph,
            content: vec![Inline::Citation {
                id: StableId::new("citation-label"),
                citation_id,
                rendered_cache: None,
            }],
            properties: Vec::new(),
        });
        doc.validate().unwrap();
        assert_eq!(doc.visible_text(), "[cite-deleted-ref]\n");

        doc.citation_database.citations[0].items.clear();
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument("citation group has no items"))
        ));
    }

    #[test]
    fn citation_payloads_reject_empty_source_fields() {
        let reference_id = StableId::parse("ref-bad").unwrap();
        let mut doc = Document::new("Citation payloads");
        doc.citation_database
            .upsert_reference(BibliographyReference {
                id: reference_id.clone(),
                revision: 1,
                source: CitationSource {
                    format: CitationSourceFormat::CitumNative,
                    bytes: b"title: Bad source".to_vec(),
                },
                summary: CitationSummary {
                    title: "Bad source".to_string(),
                    authors: vec![" ".to_string()],
                    issued: None,
                    doi: None,
                    url: None,
                },
                deleted: false,
            });
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument(
                "bibliography summary field is empty"
            ))
        ));

        doc.citation_database.references[0].summary.authors = Vec::new();
        doc.citation_database.references[0].summary.doi = Some(" ".to_string());
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument(
                "bibliography summary field is empty"
            ))
        ));

        doc.citation_database.references[0].summary.doi = Some(" 10.123/example ".to_string());
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument(
                "bibliography summary field has surrounding whitespace"
            ))
        ));

        doc.citation_database.references[0].summary.doi = None;
        doc.citation_database.references[0].summary.title = " Bad source ".to_string();
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument(
                "bibliography summary field has surrounding whitespace"
            ))
        ));

        doc.citation_database.references[0].summary.title = "Bad source".to_string();
        doc.citation_database.references[0].source.format =
            CitationSourceFormat::Unknown(String::new());
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument(
                "bibliography source format is empty"
            ))
        ));

        doc.citation_database.references[0].source.format =
            CitationSourceFormat::Unknown(" custom-format ".to_string());
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument(
                "bibliography source format has surrounding whitespace"
            ))
        ));

        doc.citation_database.references[0].source.format = CitationSourceFormat::CitumNative;
        doc.citation_database.references[0].source.bytes = b" \n\t ".to_vec();
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument("bibliography source is empty"))
        ));

        doc.citation_database.references[0].source.bytes = b"title: Bad source".to_vec();
        doc.citation_database.upsert_citation(CitationGroup {
            id: StableId::parse("cite-bad").unwrap(),
            revision: 1,
            items: vec![CitationItem {
                reference_id,
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
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument("citation item field is empty"))
        ));

        doc.citation_database.references[0].summary.title = "Bad source".to_string();
        doc.citation_database.references[0].summary.authors = Vec::new();
        doc.citation_database.references[0].summary.issued = None;
        doc.citation_database.references[0].summary.doi = None;
        doc.citation_database.references[0].summary.url = None;
        doc.citation_database.citations[0].items[0].prefix = Some(" see ".to_string());
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument(
                "citation item field has surrounding whitespace"
            ))
        ));
    }

    #[test]
    fn live_footnote_citations_require_live_footnote_targets() {
        let footnote_id = StableId::parse("fn-citation").unwrap();
        let reference_id = StableId::parse("ref-doe-2020").unwrap();
        let citation_id = StableId::parse("cite-footnote").unwrap();
        let mut doc = Document::new("Footnote citation");
        doc.citation_database
            .upsert_reference(BibliographyReference {
                id: reference_id.clone(),
                revision: 1,
                source: CitationSource {
                    format: CitationSourceFormat::CitumNative,
                    bytes: b"title: Footnote source".to_vec(),
                },
                summary: CitationSummary {
                    title: "Footnote source".to_string(),
                    authors: Vec::new(),
                    issued: None,
                    doi: None,
                    url: None,
                },
                deleted: false,
            });
        doc.citation_database.upsert_citation(CitationGroup {
            id: citation_id,
            revision: 1,
            items: vec![CitationItem {
                reference_id,
                locator: None,
                label: None,
                prefix: None,
                suffix: None,
                suppress_author: false,
            }],
            placement: CitationPlacement::Footnote {
                footnote_id: footnote_id.clone(),
            },
            rendered_cache: None,
            deleted: false,
        });
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidDocument(
                "footnote citation target is missing"
            ))
        ));

        doc.footnotes.push(Footnote {
            id: footnote_id,
            revision: 1,
            body: vec![Inline::text("citation footnote")],
            deleted: false,
        });
        doc.validate().unwrap();
    }
}
