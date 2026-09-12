//! The inline projection DTO.

use super::*;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppInline {
    pub id: String,
    pub kind: String,
    pub text: String,
    pub href: Option<String>,
    pub target_id: Option<String>,
    pub marks: Vec<String>,
    pub mark_kinds: Vec<String>,
    pub mark_values: BTreeMap<String, String>,
}

impl AppInline {
    pub(crate) fn from_core(inline: &Inline, citations: &opendoc_core::CitationDatabase) -> Self {
        match inline {
            Inline::Text { id, text, marks } => Self::textual(id, "text", text, None, None, marks),
            Inline::Link {
                id,
                text,
                href,
                marks,
            } => Self::textual(id, "link", text, Some(href.clone()), None, marks),
            Inline::Citation {
                id,
                citation_id,
                rendered_cache,
            } => Self {
                id: id.to_string(),
                kind: "citation".to_string(),
                text: rendered_cache
                    .clone()
                    .or_else(|| citations.rendered_citation(citation_id).cloned())
                    .unwrap_or_else(|| format!("[{citation_id}]")),
                href: None,
                target_id: Some(citation_id.to_string()),
                marks: Vec::new(),
                mark_kinds: Vec::new(),
                mark_values: BTreeMap::new(),
            },
            Inline::FootnoteRef { id, footnote_id } => Self {
                id: id.to_string(),
                kind: "footnote-ref".to_string(),
                text: format!("[{footnote_id}]"),
                href: None,
                target_id: Some(footnote_id.to_string()),
                marks: Vec::new(),
                mark_kinds: Vec::new(),
                mark_values: BTreeMap::new(),
            },
            Inline::Mention { id, label } => Self {
                id: id.to_string(),
                kind: "mention".to_string(),
                text: label.clone(),
                href: None,
                target_id: None,
                marks: Vec::new(),
                mark_kinds: Vec::new(),
                mark_values: BTreeMap::new(),
            },
            Inline::Equation { id, equation } => Self {
                id: id.to_string(),
                kind: "equation".to_string(),
                text: equation.source.clone(),
                href: None,
                target_id: Some(equation.id.to_string()),
                marks: Vec::new(),
                mark_kinds: Vec::new(),
                mark_values: BTreeMap::new(),
            },
            // A field projects with an empty `text`: the value is produced by
            // pagination, so there is nothing here to report. `target_id`
            // carries which field it is, the only part that is document data.
            Inline::PageNumber { id, field } => Self {
                id: id.to_string(),
                kind: "page-number".to_string(),
                text: String::new(),
                href: None,
                target_id: Some(field.as_str().to_string()),
                marks: Vec::new(),
                mark_kinds: Vec::new(),
                mark_values: BTreeMap::new(),
            },
        }
    }

    fn textual(
        id: &StableId,
        kind: &str,
        text: &str,
        href: Option<String>,
        target_id: Option<String>,
        marks: &[Mark],
    ) -> Self {
        let (mark_kinds, mark_values) = mark_projection(marks);
        Self {
            id: id.to_string(),
            kind: kind.to_string(),
            text: text.to_string(),
            href,
            target_id,
            marks: marks.iter().map(mark_label).collect(),
            mark_kinds,
            mark_values,
        }
    }

    pub(crate) fn to_core(&self) -> Result<Inline, AppApiError> {
        Ok(match self.kind.as_str() {
            "link" => Inline::Link {
                id: parse_id(&self.id)?,
                text: self.text.clone(),
                href: match self.href.as_ref() {
                    Some(href) if href.trim().is_empty() => {
                        return Err(AppApiError::Format("link href missing".to_string()));
                    }
                    Some(href) if href.trim() != href => {
                        return Err(AppApiError::Format(
                            "link href has surrounding whitespace".to_string(),
                        ));
                    }
                    Some(href) => href.clone(),
                    None => return Err(AppApiError::Format("link href missing".to_string())),
                },
                marks: parse_marks(&self.marks)?,
            },
            "citation" => {
                Inline::Citation {
                    id: parse_id(&self.id)?,
                    citation_id: parse_id(self.target_id.as_deref().ok_or_else(|| {
                        AppApiError::Format("citation target missing".to_string())
                    })?)?,
                    rendered_cache: Some(self.text.clone()),
                }
            }
            "footnote-ref" => {
                Inline::FootnoteRef {
                    id: parse_id(&self.id)?,
                    footnote_id: parse_id(self.target_id.as_deref().ok_or_else(|| {
                        AppApiError::Format("footnote target missing".to_string())
                    })?)?,
                }
            }
            "mention" => Inline::Mention {
                id: parse_id(&self.id)?,
                label: if self.text.trim().is_empty() {
                    return Err(AppApiError::Format("mention label is empty".to_string()));
                } else if self.text.trim() != self.text {
                    return Err(AppApiError::Format(
                        "mention label has surrounding whitespace".to_string(),
                    ));
                } else {
                    self.text.clone()
                },
            },
            "page-number" => Inline::PageNumber {
                id: parse_id(&self.id)?,
                field: opendoc_core::PageNumberField::parse(
                    self.target_id.as_deref().unwrap_or_default(),
                )
                .map_err(|err| AppApiError::Format(err.to_string()))?,
            },
            "equation" => {
                if self.text.trim().is_empty() {
                    return Err(AppApiError::Format(
                        "inline equation source is empty".to_string(),
                    ));
                }
                if self.text.trim() != self.text {
                    return Err(AppApiError::Format(
                        "inline equation source has surrounding whitespace".to_string(),
                    ));
                }
                Inline::Equation {
                    id: parse_id(&self.id)?,
                    equation: Equation {
                        id: self
                            .target_id
                            .as_deref()
                            .map(parse_id)
                            .transpose()?
                            .unwrap_or_else(|| StableId::new("eq")),
                        source_format: EquationSourceFormat::LatexLike,
                        source: self.text.clone(),
                    },
                }
            }
            _ => Inline::Text {
                id: parse_id(&self.id)?,
                text: self.text.clone(),
                marks: parse_marks(&self.marks)?,
            },
        })
    }
}
