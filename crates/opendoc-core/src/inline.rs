//! Inline content: text runs, links, equations, marks and text ranges.

use crate::annotation::validate_text_range;
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

    pub(crate) fn push_visible_text(&self, citations: &CitationDatabase, out: &mut String) {
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
            // A field contributes no source text. Its value is produced by
            // pagination, so counting it would make the word count depend on
            // the page size.
            Inline::PageNumber { .. } => {}
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
