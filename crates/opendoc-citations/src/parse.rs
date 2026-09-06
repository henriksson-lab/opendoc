//! Parsers for BibTeX/BibLaTeX, RIS, and CSL-JSON reference sources.

use std::fmt;

use hayagriva::types::{Date, EntryType, Person};
use hayagriva::Entry;
use opendoc_core::CitationSourceFormat;
use serde_json::Value;

use crate::model::{details_from_entry, person_from_name, EntryDraft, ReferenceDetails};

/// Errors produced while parsing reference sources.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReferenceParseError {
    /// The source was not valid UTF-8.
    InvalidUtf8,
    /// BibTeX/BibLaTeX syntax or type errors.
    Bibtex(String),
    /// RIS structure errors.
    Ris(String),
    /// CSL-JSON structure errors.
    CslJson(String),
    /// The format has no parser.
    Unsupported(String),
    /// The source parsed but contained no references.
    Empty,
}

impl fmt::Display for ReferenceParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUtf8 => write!(f, "reference source is not valid UTF-8"),
            Self::Bibtex(message) => write!(f, "invalid BibTeX: {message}"),
            Self::Ris(message) => write!(f, "invalid RIS: {message}"),
            Self::CslJson(message) => write!(f, "invalid CSL-JSON: {message}"),
            Self::Unsupported(format) => write!(f, "unsupported reference format {format}"),
            Self::Empty => write!(f, "reference source contains no references"),
        }
    }
}

impl std::error::Error for ReferenceParseError {}

/// Parse a reference source in the given format into `hayagriva` entries.
pub fn parse_reference_source(
    format: &CitationSourceFormat,
    bytes: &[u8],
) -> Result<Vec<Entry>, ReferenceParseError> {
    let text = std::str::from_utf8(bytes).map_err(|_| ReferenceParseError::InvalidUtf8)?;
    let text = text.trim_start_matches('\u{feff}');
    match format {
        CitationSourceFormat::Bibtex => parse_bibtex(text),
        CitationSourceFormat::Ris => parse_ris(text),
        CitationSourceFormat::CslJson => parse_csl_json(text),
        CitationSourceFormat::CitumNative => crate::parse_citum_native_summary(bytes)
            .map(|summary| vec![crate::model::entry_from_summary("citum", &summary)])
            .ok_or(ReferenceParseError::Empty),
        CitationSourceFormat::Unknown(name) => Err(ReferenceParseError::Unsupported(name.clone())),
    }
}

/// Parse a reference source into detailed projections.
pub fn parse_reference_details(
    format: &CitationSourceFormat,
    bytes: &[u8],
) -> Result<Vec<ReferenceDetails>, ReferenceParseError> {
    let entries = parse_reference_source(format, bytes)?;
    Ok(entries.iter().map(details_from_entry).collect())
}

/// Guess the source format of raw reference bytes.
pub fn detect_source_format(bytes: &[u8]) -> CitationSourceFormat {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return CitationSourceFormat::Unknown("binary".to_string());
    };
    let trimmed = text.trim_start_matches('\u{feff}').trim_start();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return CitationSourceFormat::CslJson;
    }
    if trimmed.starts_with('@') {
        return CitationSourceFormat::Bibtex;
    }
    if trimmed
        .lines()
        .any(|line| line.starts_with("TY  - ") || line.starts_with("ER  -"))
    {
        return CitationSourceFormat::Ris;
    }
    if trimmed.contains('@') && trimmed.contains('{') {
        return CitationSourceFormat::Bibtex;
    }
    CitationSourceFormat::CitumNative
}

/// Parse BibTeX/BibLaTeX text.
pub fn parse_bibtex(text: &str) -> Result<Vec<Entry>, ReferenceParseError> {
    let library = hayagriva::io::from_biblatex_str(text).map_err(|errors| {
        ReferenceParseError::Bibtex(
            errors
                .iter()
                .map(|error| error.to_string())
                .collect::<Vec<_>>()
                .join("; "),
        )
    })?;
    let entries: Vec<Entry> = library.into_iter().collect();
    if entries.is_empty() {
        return Err(ReferenceParseError::Empty);
    }
    Ok(entries)
}

/// Parse RIS text (one or more records terminated by `ER  -`).
pub fn parse_ris(text: &str) -> Result<Vec<Entry>, ReferenceParseError> {
    let mut entries = Vec::new();
    let mut record: Option<RisRecord> = None;
    let mut saw_tag = false;

    for raw_line in text.lines() {
        let line = raw_line.trim_end();
        if line.trim().is_empty() {
            continue;
        }
        let Some((tag, value)) = split_ris_line(line) else {
            // Continuation line: append to the last value.
            if let Some(record) = record.as_mut() {
                record.append_continuation(line.trim());
            }
            continue;
        };
        saw_tag = true;
        match tag {
            "TY" => {
                if let Some(previous) = record.take() {
                    if !previous.draft.is_empty() {
                        entries.push(previous.finish(entries.len()));
                    }
                }
                record = Some(RisRecord::new(value));
            }
            "ER" => {
                if let Some(previous) = record.take() {
                    entries.push(previous.finish(entries.len()));
                }
            }
            _ => {
                let record = record.get_or_insert_with(|| RisRecord::new("GEN"));
                record.apply(tag, value);
            }
        }
    }
    if let Some(previous) = record.take() {
        if !previous.draft.is_empty() {
            entries.push(previous.finish(entries.len()));
        }
    }

    if !saw_tag {
        return Err(ReferenceParseError::Ris("no RIS tags found".to_string()));
    }
    if entries.is_empty() {
        return Err(ReferenceParseError::Empty);
    }
    Ok(entries)
}

fn split_ris_line(line: &str) -> Option<(&str, &str)> {
    let bytes = line.as_bytes();
    if bytes.len() < 4 || !bytes[0].is_ascii_uppercase() && !bytes[0].is_ascii_digit() {
        return None;
    }
    if !(bytes[1].is_ascii_uppercase() || bytes[1].is_ascii_digit()) {
        return None;
    }
    let rest = &line[2..];
    let rest = rest
        .strip_prefix("  - ")
        .or_else(|| rest.strip_prefix(" - "))?;
    Some((&line[..2], rest.trim()))
}

struct RisRecord {
    draft: EntryDraft,
    id: Option<String>,
    start_page: Option<String>,
    end_page: Option<String>,
    last_field: Option<&'static str>,
}

impl RisRecord {
    fn new(kind: &str) -> Self {
        Self {
            draft: EntryDraft::new(ris_entry_type(kind.trim())),
            id: None,
            start_page: None,
            end_page: None,
            last_field: None,
        }
    }

    fn apply(&mut self, tag: &str, value: &str) {
        if value.is_empty() {
            return;
        }
        self.last_field = None;
        match tag {
            "ID" => self.id = Some(value.to_string()),
            "AU" | "A1" => self.draft.authors.push(ris_person(value)),
            "A2" | "ED" => self.draft.editors.push(ris_person(value)),
            "A3" | "A4" => {
                if self.draft.authors.is_empty() {
                    self.draft.authors.push(ris_person(value));
                }
            }
            "TI" | "T1" => {
                self.draft.title = Some(value.to_string());
                self.last_field = Some("title");
            }
            "CT" | "BT" => {
                if self.draft.title.is_none() {
                    self.draft.title = Some(value.to_string());
                } else if self.draft.container_title.is_none() {
                    self.draft.container_title = Some(value.to_string());
                }
            }
            "T2" | "JO" | "JF" | "JA" | "J1" | "J2" => {
                if self.draft.container_title.is_none() || tag == "T2" {
                    self.draft.container_title = Some(value.to_string());
                }
            }
            "PY" | "Y1" | "DA" => {
                let date = ris_date(value);
                if self.draft.date.is_none() || tag == "DA" {
                    self.draft.date = Some(date);
                }
            }
            "VL" => self.draft.volume = Some(value.to_string()),
            "IS" | "CP" => self.draft.issue = Some(value.to_string()),
            "SP" => {
                if value.contains('-') && self.end_page.is_none() {
                    self.draft.pages = Some(value.to_string());
                } else {
                    self.start_page = Some(value.to_string());
                }
            }
            "EP" => self.end_page = Some(value.to_string()),
            "PB" => self.draft.publisher = Some(value.to_string()),
            "CY" | "PP" => self.draft.place = Some(value.to_string()),
            "ET" => self.draft.edition = Some(value.to_string()),
            "DO" => self.draft.doi = Some(normalize_doi(value)),
            "UR" | "L1" | "L2" | "LK" => {
                if self.draft.url.is_none() {
                    self.draft.url = Some(value.to_string());
                }
            }
            "SN" => {
                let cleaned = value.replace(['-', ' '], "");
                if cleaned.len() >= 10 {
                    self.draft.isbn = Some(value.to_string());
                } else {
                    self.draft.issn = Some(value.to_string());
                }
            }
            "N1" if self.draft.note.is_none() => {
                self.draft.note = Some(value.to_string());
                self.last_field = Some("note");
            }
            _ => {}
        }
    }

    fn append_continuation(&mut self, value: &str) {
        match self.last_field {
            Some("title") => {
                if let Some(title) = self.draft.title.as_mut() {
                    title.push(' ');
                    title.push_str(value);
                }
            }
            Some("note") => {
                if let Some(note) = self.draft.note.as_mut() {
                    note.push(' ');
                    note.push_str(value);
                }
            }
            _ => {}
        }
    }

    fn finish(mut self, index: usize) -> Entry {
        if self.draft.pages.is_none() {
            self.draft.pages = match (self.start_page, self.end_page) {
                (Some(start), Some(end)) if start != end => Some(format!("{start}-{end}")),
                (Some(start), _) => Some(start),
                (None, Some(end)) => Some(end),
                (None, None) => None,
            };
        }
        let key = self
            .id
            .clone()
            .unwrap_or_else(|| ris_default_key(&self.draft, index));
        self.draft.build(&key)
    }
}

fn ris_default_key(draft: &EntryDraft, index: usize) -> String {
    let family = draft
        .authors
        .first()
        .map(|person| person.name.to_ascii_lowercase())
        .unwrap_or_else(|| "ref".to_string());
    let family: String = family.chars().filter(|ch| ch.is_alphanumeric()).collect();
    let year = draft
        .date
        .as_deref()
        .and_then(|date| date.get(..4))
        .unwrap_or("nd");
    format!("{family}{year}-{}", index + 1)
}

fn ris_entry_type(kind: &str) -> EntryType {
    match kind.to_ascii_uppercase().as_str() {
        "JOUR" | "JFULL" | "EJOUR" | "MGZN" => EntryType::Article,
        "NEWS" => EntryType::Newspaper,
        "BOOK" | "EBOOK" => EntryType::Book,
        "CHAP" | "ECHAP" => EntryType::Chapter,
        "CONF" | "CPAPER" => EntryType::Conference,
        "THES" => EntryType::Thesis,
        "RPRT" => EntryType::Report,
        "ELEC" | "WEB" => EntryType::Web,
        "BLOG" => EntryType::Blog,
        "PAT" => EntryType::Patent,
        "CASE" => EntryType::Case,
        "STAT" | "BILL" => EntryType::Legislation,
        "MANSCPT" | "UNPB" | "UNPD" => EntryType::Manuscript,
        "VIDEO" | "MPCT" => EntryType::Video,
        "SOUND" | "MUSIC" => EntryType::Audio,
        "ART" => EntryType::Artwork,
        "COMP" => EntryType::Repository,
        "EDBOOK" => EntryType::Anthology,
        "ENCYC" | "DICT" => EntryType::Reference,
        "PCOMM" | "ICOMM" => EntryType::Misc,
        // Generic and unknown records render most sensibly as standalone
        // articles: `misc` is dropped from Chicago bibliographies.
        _ => EntryType::Article,
    }
}

fn ris_person(value: &str) -> Person {
    // RIS names are `Family, Given, Suffix`; keep only the first two parts for
    // the name parser and attach any suffix explicitly.
    let mut parts = value.split(',').map(str::trim);
    let family = parts.next().unwrap_or_default();
    let given = parts.next().filter(|given| !given.is_empty());
    let suffix = parts.next().filter(|suffix| !suffix.is_empty());
    let mut person = match given {
        Some(given) => person_from_name(&format!("{family}, {given}")),
        None => person_from_name(family),
    };
    person.suffix = suffix.map(str::to_string);
    person
}

fn ris_date(value: &str) -> String {
    // RIS dates are `YYYY/MM/DD/other`; keep the ISO-like prefix.
    let parts: Vec<&str> = value.split('/').collect();
    let mut out = String::new();
    for (index, part) in parts.iter().take(3).enumerate() {
        let part = part.trim();
        if part.is_empty() || !part.chars().all(|ch| ch.is_ascii_digit()) {
            break;
        }
        if index > 0 {
            out.push('-');
        }
        if index > 0 && part.len() == 1 {
            out.push('0');
        }
        out.push_str(part);
    }
    if out.is_empty() {
        value.trim().to_string()
    } else {
        out
    }
}

/// Parse CSL-JSON: a single item object or an array of items.
pub fn parse_csl_json(text: &str) -> Result<Vec<Entry>, ReferenceParseError> {
    let value: Value = serde_json::from_str(text)
        .map_err(|error| ReferenceParseError::CslJson(error.to_string()))?;
    let items = match value {
        Value::Array(items) => items,
        Value::Object(_) => vec![value],
        _ => {
            return Err(ReferenceParseError::CslJson(
                "expected an object or an array of objects".to_string(),
            ))
        }
    };
    let mut entries = Vec::new();
    for (index, item) in items.iter().enumerate() {
        let Value::Object(object) = item else {
            return Err(ReferenceParseError::CslJson(format!(
                "item {} is not an object",
                index + 1
            )));
        };
        entries.push(csl_json_entry(object, index));
    }
    if entries.is_empty() {
        return Err(ReferenceParseError::Empty);
    }
    Ok(entries)
}

fn csl_json_entry(object: &serde_json::Map<String, Value>, index: usize) -> Entry {
    let kind = object
        .get("type")
        .and_then(Value::as_str)
        .map(csl_entry_type)
        .unwrap_or(EntryType::Article);
    let mut draft = EntryDraft::new(kind);
    let string = |key: &str| -> Option<String> {
        object.get(key).and_then(|value| match value {
            Value::String(text) => Some(text.clone()),
            Value::Number(number) => Some(number.to_string()),
            _ => None,
        })
    };
    draft.title = string("title");
    draft.authors = csl_names(object.get("author"));
    draft.editors = csl_names(object.get("editor"));
    draft.container_title = string("container-title")
        .or_else(|| string("collection-title"))
        .or_else(|| string("event-title"))
        .or_else(|| string("event"));
    draft.volume = string("volume");
    draft.issue = string("issue").or_else(|| string("number"));
    draft.pages = string("page");
    draft.publisher = string("publisher");
    draft.place = string("publisher-place");
    draft.edition = string("edition");
    draft.date = csl_date(object.get("issued"))
        .or_else(|| csl_date(object.get("event-date")))
        .or_else(|| csl_date(object.get("original-date")));
    draft.doi = string("DOI").map(|doi| normalize_doi(&doi));
    draft.url = string("URL");
    draft.isbn = string("ISBN");
    draft.issn = string("ISSN");
    draft.note = string("note");
    let key = object
        .get("id")
        .and_then(|id| match id {
            Value::String(text) => Some(text.clone()),
            Value::Number(number) => Some(number.to_string()),
            _ => None,
        })
        .filter(|id| !id.trim().is_empty())
        .unwrap_or_else(|| format!("csl-item-{}", index + 1));
    draft.build(&key)
}

fn csl_names(value: Option<&Value>) -> Vec<Person> {
    let Some(Value::Array(names)) = value else {
        return Vec::new();
    };
    names
        .iter()
        .filter_map(|name| match name {
            Value::Object(name) => {
                let field = |key: &str| name.get(key).and_then(Value::as_str).map(str::trim);
                if let Some(literal) = field("literal").filter(|literal| !literal.is_empty()) {
                    return Some(Person {
                        name: literal.to_string(),
                        given_name: None,
                        prefix: None,
                        suffix: None,
                        comma_suffix: false,
                        alias: None,
                    });
                }
                let family = field("family").unwrap_or_default();
                let given = field("given").filter(|given| !given.is_empty());
                if family.is_empty() {
                    return given.map(person_from_name);
                }
                let particle = [field("non-dropping-particle"), field("dropping-particle")]
                    .into_iter()
                    .flatten()
                    .filter(|particle| !particle.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ");
                Some(Person {
                    name: family.to_string(),
                    given_name: given.map(str::to_string),
                    prefix: (!particle.is_empty()).then_some(particle),
                    suffix: field("suffix")
                        .filter(|suffix| !suffix.is_empty())
                        .map(str::to_string),
                    comma_suffix: false,
                    alias: None,
                })
            }
            Value::String(text) => Some(person_from_name(text)),
            _ => None,
        })
        .collect()
}

fn csl_date(value: Option<&Value>) -> Option<String> {
    let value = value?;
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Object(object) => {
            if let Some(Value::Array(parts)) = object.get("date-parts") {
                if let Some(Value::Array(first)) = parts.first() {
                    let numbers: Vec<i64> = first
                        .iter()
                        .filter_map(|part| match part {
                            Value::Number(number) => number.as_i64(),
                            Value::String(text) => text.trim().parse().ok(),
                            _ => None,
                        })
                        .collect();
                    if let Some(year) = numbers.first() {
                        let mut out = year.to_string();
                        if let Some(month) = numbers.get(1) {
                            out.push_str(&format!("-{month:02}"));
                            if let Some(day) = numbers.get(2) {
                                out.push_str(&format!("-{day:02}"));
                            }
                        }
                        return Some(out);
                    }
                }
            }
            object
                .get("raw")
                .or_else(|| object.get("literal"))
                .and_then(Value::as_str)
                .map(str::to_string)
        }
        _ => None,
    }
}

fn csl_entry_type(kind: &str) -> EntryType {
    match kind.trim().to_ascii_lowercase().as_str() {
        "article-journal" | "article" | "article-magazine" => EntryType::Article,
        "article-newspaper" => EntryType::Newspaper,
        "book" | "classic" => EntryType::Book,
        "chapter" => EntryType::Chapter,
        "entry" | "entry-dictionary" | "entry-encyclopedia" => EntryType::Entry,
        "paper-conference" => EntryType::Conference,
        "thesis" => EntryType::Thesis,
        "report" => EntryType::Report,
        "webpage" | "post" | "post-weblog" => EntryType::Web,
        "patent" => EntryType::Patent,
        "legal_case" => EntryType::Case,
        "legislation" | "bill" | "regulation" => EntryType::Legislation,
        "manuscript" => EntryType::Manuscript,
        "motion_picture" | "broadcast" => EntryType::Video,
        "song" | "musical_score" => EntryType::Audio,
        "graphic" | "figure" => EntryType::Artwork,
        "software" | "dataset" => EntryType::Repository,
        "periodical" => EntryType::Periodical,
        "performance" | "speech" | "event" => EntryType::Performance,
        "personal_communication" | "interview" => EntryType::Misc,
        // Generic and unknown types render most sensibly as standalone
        // articles: `misc` is dropped from Chicago bibliographies.
        _ => EntryType::Article,
    }
}

fn normalize_doi(value: &str) -> String {
    let trimmed = value.trim();
    for prefix in [
        "https://doi.org/",
        "http://doi.org/",
        "https://dx.doi.org/",
        "http://dx.doi.org/",
        "doi:",
        "DOI:",
    ] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            return rest.trim().to_string();
        }
    }
    trimmed.to_string()
}

/// Parse a loose date string into a `hayagriva` date (used by callers that
/// want the typed value for a details field).
pub fn parse_reference_date(value: &str) -> Option<Date> {
    crate::model::parse_date(value)
}
