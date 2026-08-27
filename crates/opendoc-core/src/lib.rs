use sha2::{Digest, Sha256};
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static ID_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
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

#[derive(Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct StableId(String);

impl StableId {
    pub fn new(prefix: &str) -> Self {
        Self(format!("{prefix}-{:016x}", next_counter()))
    }

    pub fn parse(value: impl Into<String>) -> Result<Self, ModelError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(ModelError::InvalidId("stable id is empty"));
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

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
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
        if algorithm.is_empty() || digest.is_empty() {
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

impl fmt::Display for HashRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.algorithm, self.digest)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Document {
    pub uuid: DocumentUuid,
    pub title: String,
    pub locale: String,
    pub blocks: Vec<Block>,
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
            blocks: Vec::new(),
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
        if self.title.trim().is_empty() {
            return Err(ModelError::InvalidDocument("title is empty"));
        }
        for comment in &self.comments {
            comment.validate()?;
        }
        for suggestion in &self.suggestions {
            suggestion.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
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
            _ => {}
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
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
    PageBreak,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TableRow {
    pub id: StableId,
    pub cells: Vec<TableCell>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TableCell {
    pub id: StableId,
    pub blocks: Vec<Block>,
    pub properties: Vec<Property>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Equation {
    pub id: StableId,
    pub source_format: EquationSourceFormat,
    pub source: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EquationSourceFormat {
    LatexLike,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Mark {
    pub kind: MarkKind,
    pub value: Option<String>,
    pub expand: MarkExpand,
}

#[derive(Clone, Debug, Eq, PartialEq)]
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MarkExpand {
    None,
    Start,
    End,
    Both,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextRange {
    pub start: StableId,
    pub end: StableId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommentThread {
    pub id: StableId,
    pub anchor: Anchor,
    pub comments: Vec<Comment>,
    pub deleted: bool,
}

impl CommentThread {
    fn validate(&self) -> Result<(), ModelError> {
        if self.comments.is_empty() {
            return Err(ModelError::InvalidDocument(
                "comment thread has no comments",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Comment {
    pub id: StableId,
    pub author: String,
    pub body: Vec<Inline>,
    pub created_at_ms: u64,
    pub deleted: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Anchor {
    TextRange(TextRange),
    NearestBlock { block_id: StableId, warning: String },
    Document,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Suggestion {
    pub id: StableId,
    pub author: String,
    pub kind: SuggestionKind,
    pub state: SuggestionState,
    pub provenance: Vec<String>,
}

impl Suggestion {
    fn validate(&self) -> Result<(), ModelError> {
        if self.author.trim().is_empty() {
            return Err(ModelError::InvalidDocument("suggestion author is empty"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SuggestionState {
    Proposed,
    Accepted,
    Rejected,
}

#[derive(Clone, Debug, Eq, PartialEq)]
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BibliographyReference {
    pub id: StableId,
    pub revision: u64,
    pub source: CitationSource,
    pub summary: CitationSummary,
    pub deleted: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CitationSource {
    pub format: CitationSourceFormat,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CitationSourceFormat {
    CitumNative,
    CslJson,
    Bibtex,
    Ris,
    Unknown(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CitationSummary {
    pub title: String,
    pub authors: Vec<String>,
    pub issued: Option<String>,
    pub doi: Option<String>,
    pub url: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CitationGroup {
    pub id: StableId,
    pub revision: u64,
    pub items: Vec<CitationItem>,
    pub placement: CitationPlacement,
    pub rendered_cache: Option<String>,
    pub deleted: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CitationItem {
    pub reference_id: StableId,
    pub locator: Option<String>,
    pub label: Option<String>,
    pub prefix: Option<String>,
    pub suffix: Option<String>,
    pub suppress_author: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CitationPlacement {
    Inline,
    Footnote { footnote_id: StableId },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Property {
    pub key: String,
    pub value: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelWarning {
    pub code: String,
    pub message: String,
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

fn now_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0)
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
}
