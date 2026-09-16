//! Inline content: text runs, links, equations, marks and text ranges.

use crate::annotation::validate_text_range;
use crate::block::TextScope;
use crate::citation::CitationDatabase;
use crate::document::{inline_stable_id, validate_inline, validate_marks};
use crate::ids::validate_stable_id;
use crate::ids::StableId;
use crate::page::PageNumberField;
use crate::warning::ModelError;
use serde::{Deserialize, Serialize};

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
    /// A Google Docs person smart chip. Its visible label is ordinary document
    /// text; the identity is opaque imported metadata and never authorizes a
    /// profile lookup when a document is opened.
    GooglePersonChip {
        id: StableId,
        label: String,
        email: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        person_id: Option<String>,
    },
    /// A Google Docs rich-link smart chip. `href` remains the offline fallback;
    /// provider/resource metadata makes a later Google export unambiguous.
    GoogleRichLinkChip {
        id: StableId,
        label: String,
        href: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rich_link_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mime_type: Option<String>,
    },
    /// A finite, document-owned choice.  The selected value is stored as an
    /// option identifier rather than its display label, so renaming an option
    /// cannot silently change the user's choice.
    Dropdown {
        id: StableId,
        options: Vec<DropdownOption>,
        selected_option_id: String,
    },
    /// An atomic calendar date. The durable value is canonical ISO calendar
    /// notation (`YYYY-MM-DD`), never a locale-formatted display string.
    DateChip {
        id: StableId,
        date: String,
    },
    Equation {
        id: StableId,
        equation: Equation,
    },
    /// A page-number field. It carries *which* number to print, never the
    /// number itself: the value depends on where the layout engine broke the
    /// pages, which is not a property of the document. See ADR 0009.
    PageNumber {
        id: StableId,
        field: PageNumberField,
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

    pub(crate) fn push_visible_text(
        &self,
        citations: &CitationDatabase,
        scope: TextScope,
        out: &mut String,
    ) {
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
            // A footnote reference contributes nothing in **either** scope,
            // which is why it takes no `scope` test. It used to push
            // `[{footnote_id}]`: a `StableId` that appears nowhere on the
            // page, means nothing to a reader, and is not a stand-in a text
            // file wants either — 20-odd characters of internal identifier,
            // counted as a word. The marker a reader sees is a number the
            // renderer computes from the reference's position, and the
            // footnote's body is not in the text flow at all.
            //
            // The `Citation` arm above looks similar and is not: a citation's
            // rendered form *is* on the page, and falling back to its id is a
            // last resort for a reference the database has lost.
            Inline::FootnoteRef { .. } => {}
            Inline::Mention { label, .. }
            | Inline::GooglePersonChip { label, .. }
            | Inline::GoogleRichLinkChip { label, .. } => out.push_str(label),
            Inline::Dropdown {
                options,
                selected_option_id,
                ..
            } => {
                if let Some(option) = options
                    .iter()
                    .find(|option| option.id == *selected_option_id)
                {
                    out.push_str(&option.label);
                }
            }
            Inline::DateChip { date, .. } => out.push_str(date),
            // The source, not the rendering: an inline equation is typeset
            // mathematics on the page, and its LaTeX is what that was written
            // from (ADR 0003). A text file gets the source, because that is
            // the best plain text can do for a formula.
            Inline::Equation { equation, .. } => {
                if scope == TextScope::PlainText {
                    out.push_str(&equation.source);
                }
            }
            // A field contributes no source text. Its value is produced by
            // pagination, so counting it would make the word count depend on
            // the page size.
            Inline::PageNumber { .. } => {}
        }
    }
}

/// One stable choice in an inline dropdown.
///
/// The id is local to its dropdown, deliberately not a document-wide
/// [`StableId`].  It is nevertheless canonical and immutable once selected;
/// selection operations name this id so concurrent label edits cannot retarget
/// a choice.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DropdownOption {
    pub id: String,
    pub label: String,
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
