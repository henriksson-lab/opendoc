//! The inline projection DTO.

use super::*;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppInline {
    pub id: String,
    pub kind: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub href: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub marks: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mark_kinds: Vec<String>,
    pub mark_values: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dropdown_options: Vec<AppDropdownOption>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_option_id: Option<String>,
    /// Canonical ISO calendar date for a typed date chip. The display text is
    /// deliberately the same value; adapters must not smuggle locale labels
    /// into durable document data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    /// Read-only imported Google person identity. It is opaque document data;
    /// the desktop must not use it to fetch a profile.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub google_person_email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub google_person_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub google_rich_link_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub google_rich_link_mime_type: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppDropdownOption {
    pub id: String,
    pub label: String,
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
                dropdown_options: Vec::new(),
                selected_option_id: None,
                date: None,
                google_person_email: None,
                google_person_id: None,
                google_rich_link_id: None,
                google_rich_link_mime_type: None,
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
                dropdown_options: Vec::new(),
                selected_option_id: None,
                date: None,
                google_person_email: None,
                google_person_id: None,
                google_rich_link_id: None,
                google_rich_link_mime_type: None,
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
                dropdown_options: Vec::new(),
                selected_option_id: None,
                date: None,
                google_person_email: None,
                google_person_id: None,
                google_rich_link_id: None,
                google_rich_link_mime_type: None,
            },
            Inline::GooglePersonChip {
                id,
                label,
                email,
                person_id,
            } => Self {
                id: id.to_string(),
                kind: "google-person-chip".to_string(),
                text: label.clone(),
                href: None,
                target_id: None,
                marks: Vec::new(),
                mark_kinds: Vec::new(),
                mark_values: BTreeMap::new(),
                dropdown_options: Vec::new(),
                selected_option_id: None,
                date: None,
                google_person_email: Some(email.clone()),
                google_person_id: person_id.clone(),
                google_rich_link_id: None,
                google_rich_link_mime_type: None,
            },
            Inline::GoogleRichLinkChip {
                id,
                label,
                href,
                rich_link_id,
                mime_type,
            } => Self {
                id: id.to_string(),
                kind: "google-rich-link-chip".to_string(),
                text: label.clone(),
                href: Some(href.clone()),
                target_id: None,
                marks: Vec::new(),
                mark_kinds: Vec::new(),
                mark_values: BTreeMap::new(),
                dropdown_options: Vec::new(),
                selected_option_id: None,
                date: None,
                google_person_email: None,
                google_person_id: None,
                google_rich_link_id: rich_link_id.clone(),
                google_rich_link_mime_type: mime_type.clone(),
            },
            Inline::Dropdown {
                id,
                options,
                selected_option_id,
            } => Self {
                id: id.to_string(),
                kind: "dropdown".to_string(),
                text: options
                    .iter()
                    .find(|option| option.id == *selected_option_id)
                    .map(|option| option.label.clone())
                    .unwrap_or_default(),
                href: None,
                target_id: None,
                marks: Vec::new(),
                mark_kinds: Vec::new(),
                mark_values: BTreeMap::new(),
                dropdown_options: options
                    .iter()
                    .map(|option| AppDropdownOption {
                        id: option.id.clone(),
                        label: option.label.clone(),
                    })
                    .collect(),
                selected_option_id: Some(selected_option_id.clone()),
                date: None,
                google_person_email: None,
                google_person_id: None,
                google_rich_link_id: None,
                google_rich_link_mime_type: None,
            },
            Inline::DateChip { id, date } => Self {
                id: id.to_string(),
                kind: "date-chip".to_string(),
                text: date.clone(),
                href: None,
                target_id: None,
                marks: Vec::new(),
                mark_kinds: Vec::new(),
                mark_values: BTreeMap::new(),
                dropdown_options: Vec::new(),
                selected_option_id: None,
                date: Some(date.clone()),
                google_person_email: None,
                google_person_id: None,
                google_rich_link_id: None,
                google_rich_link_mime_type: None,
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
                dropdown_options: Vec::new(),
                selected_option_id: None,
                date: None,
                google_person_email: None,
                google_person_id: None,
                google_rich_link_id: None,
                google_rich_link_mime_type: None,
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
                dropdown_options: Vec::new(),
                selected_option_id: None,
                date: None,
                google_person_email: None,
                google_person_id: None,
                google_rich_link_id: None,
                google_rich_link_mime_type: None,
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
            dropdown_options: Vec::new(),
            selected_option_id: None,
            date: None,
            google_person_email: None,
            google_person_id: None,
            google_rich_link_id: None,
            google_rich_link_mime_type: None,
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
            "dropdown" => Inline::Dropdown {
                id: parse_id(&self.id)?,
                options: self
                    .dropdown_options
                    .iter()
                    .map(|option| opendoc_core::DropdownOption {
                        id: option.id.clone(),
                        label: option.label.clone(),
                    })
                    .collect(),
                selected_option_id: self.selected_option_id.clone().ok_or_else(|| {
                    AppApiError::Format("dropdown selected option missing".to_string())
                })?,
            },
            "date-chip" => Inline::DateChip {
                id: parse_id(&self.id)?,
                date: self.date.clone().unwrap_or_else(|| self.text.clone()),
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
