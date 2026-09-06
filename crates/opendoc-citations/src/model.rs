//! Rich reference model shared by the parsers and the renderer.

use hayagriva::types::{
    Date, EntryType, FormatString, MaybeTyped, Numeric, PageRanges, Person, Publisher, QualifiedUrl,
};
use hayagriva::Entry;
use opendoc_core::{BibliographyReference, CitationSourceFormat, CitationSummary};

use crate::parse::parse_reference_source;

/// A person associated with a reference, split into name parts.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ReferenceAuthor {
    /// Family name, or the full literal name for institutional authors.
    pub family: String,
    /// Given name(s), when known.
    pub given: Option<String>,
    /// Name particle such as `van` or `de`.
    pub particle: Option<String>,
    /// Generational suffix such as `Jr.`.
    pub suffix: Option<String>,
}

impl ReferenceAuthor {
    /// `Given Particle Family Suffix` form suitable for prose.
    pub fn display_name(&self) -> String {
        let mut parts = Vec::new();
        if let Some(given) = &self.given {
            parts.push(given.clone());
        }
        if let Some(particle) = &self.particle {
            parts.push(particle.clone());
        }
        parts.push(self.family.clone());
        if let Some(suffix) = &self.suffix {
            parts.push(suffix.clone());
        }
        parts.join(" ")
    }

    /// `Family, Given` form used for sorting and reference lists.
    pub fn sort_name(&self) -> String {
        let mut family = String::new();
        if let Some(particle) = &self.particle {
            family.push_str(particle);
            family.push(' ');
        }
        family.push_str(&self.family);
        match &self.given {
            Some(given) => format!("{family}, {given}"),
            None => family,
        }
    }

    pub(crate) fn from_person(person: &Person) -> Self {
        Self {
            family: person.name.clone(),
            given: person.given_name.clone(),
            particle: person.prefix.clone(),
            suffix: person.suffix.clone(),
        }
    }

    pub(crate) fn to_person(&self) -> Person {
        Person {
            name: self.family.clone(),
            given_name: self.given.clone(),
            prefix: self.particle.clone(),
            suffix: self.suffix.clone(),
            comma_suffix: false,
            alias: None,
        }
    }
}

/// A detailed projection of a bibliography reference.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ReferenceDetails {
    /// Citation key or reference id.
    pub id: String,
    /// Kind of work (`article`, `book`, `chapter`, `thesis`, `web`, ...).
    pub kind: String,
    /// Title of the work.
    pub title: String,
    /// Authors in order.
    pub authors: Vec<ReferenceAuthor>,
    /// Editors in order.
    pub editors: Vec<ReferenceAuthor>,
    /// Journal, book, or proceedings the work appeared in.
    pub container_title: Option<String>,
    /// Volume number.
    pub volume: Option<String>,
    /// Issue number.
    pub issue: Option<String>,
    /// Page range such as `12-34`.
    pub pages: Option<String>,
    /// Publisher name.
    pub publisher: Option<String>,
    /// Publisher location.
    pub place: Option<String>,
    /// Edition.
    pub edition: Option<String>,
    /// Publication year.
    pub year: Option<i32>,
    /// Full publication date in ISO form (`2020-05-01`, `2020-05`, `2020`).
    pub date: Option<String>,
    /// Digital object identifier.
    pub doi: Option<String>,
    /// URL.
    pub url: Option<String>,
    /// ISBN.
    pub isbn: Option<String>,
    /// ISSN.
    pub issn: Option<String>,
}

impl ReferenceDetails {
    /// Project the details onto the compact summary stored in the core model.
    pub fn to_summary(&self) -> CitationSummary {
        CitationSummary {
            title: self.title.clone(),
            authors: self
                .authors
                .iter()
                .map(ReferenceAuthor::display_name)
                .collect(),
            issued: self
                .year
                .map(|year| year.to_string())
                .or_else(|| self.date.clone()),
            doi: self.doi.clone(),
            url: self.url.clone(),
        }
    }
}

/// Project the details onto the compact summary stored in the core model.
pub fn summary_from_details(details: &ReferenceDetails) -> CitationSummary {
    details.to_summary()
}

/// Build the detailed projection for a stored reference, parsing its source
/// bytes when the format is known and falling back to the stored summary.
pub fn reference_details(reference: &BibliographyReference) -> ReferenceDetails {
    let entry = reference_entry(reference);
    details_from_entry(&entry)
}

/// Build details from a `hayagriva` entry.
pub fn details_from_entry(entry: &Entry) -> ReferenceDetails {
    let date = entry.date_any();
    let first_parent = entry.parents().first();
    ReferenceDetails {
        id: entry.key().to_string(),
        kind: entry_kind(entry.entry_type()),
        title: entry
            .title()
            .map(|title| title.to_string())
            .unwrap_or_default(),
        authors: entry
            .authors()
            .map(|authors| authors.iter().map(ReferenceAuthor::from_person).collect())
            .unwrap_or_default(),
        editors: entry
            .editors()
            .or_else(|| first_parent.and_then(|parent| parent.editors()))
            .map(|editors| editors.iter().map(ReferenceAuthor::from_person).collect())
            .unwrap_or_default(),
        container_title: first_parent
            .and_then(|parent| parent.title())
            .map(|title| title.to_string()),
        volume: entry
            .map(|entry| entry.volume())
            .map(|volume| volume.to_string()),
        issue: entry
            .map(|entry| entry.issue())
            .map(|issue| issue.to_string()),
        pages: entry.page_range().map(|pages| pages.to_string()),
        publisher: entry
            .map(|entry| entry.publisher().and_then(|publisher| publisher.name()))
            .map(|name| name.to_string()),
        place: entry
            .map(|entry| {
                entry
                    .publisher()
                    .and_then(|publisher| publisher.location())
                    .or_else(|| entry.location())
            })
            .map(|place| place.to_string()),
        edition: entry
            .map(|entry| entry.edition())
            .map(|edition| edition.to_string()),
        year: date.map(|date| date.year),
        date: date.map(format_date),
        doi: entry.map(|entry| entry.doi()).map(str::to_string),
        url: entry.url_any().map(|url| url.value.to_string()),
        isbn: entry.map(|entry| entry.isbn()).map(str::to_string),
        issn: entry.map(|entry| entry.issn()).map(str::to_string),
    }
}

fn entry_kind(entry_type: &EntryType) -> String {
    serde_json::to_value(entry_type)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_else(|| format!("{entry_type:?}").to_ascii_lowercase())
}

fn format_date(date: &Date) -> String {
    let mut out = date.year.to_string();
    if let Some(month) = date.month {
        out.push_str(&format!("-{:02}", month + 1));
        if let Some(day) = date.day {
            out.push_str(&format!("-{:02}", day + 1));
        }
    }
    out
}

/// Resolve the `hayagriva` entry for a stored reference. The entry key always
/// equals the reference id so rendered output can be mapped back.
pub fn reference_entry(reference: &BibliographyReference) -> Entry {
    let key = reference.id.as_str();
    let parsed = match &reference.source.format {
        CitationSourceFormat::CitumNative | CitationSourceFormat::Unknown(_) => None,
        format => parse_reference_source(format, &reference.source.bytes)
            .ok()
            .and_then(|entries| {
                let index = entries
                    .iter()
                    .position(|entry| entry.key() == key)
                    .unwrap_or(0);
                entries.into_iter().nth(index)
            }),
    };
    if let Some(entry) = parsed {
        let mut entry = normalize_entry(entry, key);
        fill_from_summary(&mut entry, &reference.summary);
        return entry;
    }

    let summary = if matches!(reference.source.format, CitationSourceFormat::CitumNative) {
        crate::merge_summary_with_citum_native_source(
            reference.summary.clone(),
            &reference.source.bytes,
        )
    } else {
        reference.summary.clone()
    };
    entry_from_summary(key, &summary)
}

/// Fill fields the source lacked from the stored summary.
fn fill_from_summary(entry: &mut Entry, summary: &CitationSummary) {
    if entry.title().is_none() && !summary.title.trim().is_empty() {
        entry.set_title(FormatString::with_value(summary.title.clone()));
    }
    if entry.authors().is_none_or(|authors| authors.is_empty()) && !summary.authors.is_empty() {
        entry.set_authors(
            summary
                .authors
                .iter()
                .map(|name| person_from_name(name))
                .collect(),
        );
    }
    if entry.date_any().is_none() {
        if let Some(date) = summary.issued.as_deref().and_then(parse_date) {
            entry.set_date(date);
        }
    }
    if entry.map(|entry| entry.doi()).is_none() {
        if let Some(doi) = &summary.doi {
            entry.set_doi(doi.clone());
        }
    }
    if entry.url_any().is_none() {
        if let Some(url) = summary.url.as_deref().and_then(parse_url) {
            entry.set_url(url);
        }
    }
}

/// Build an entry from the compact summary alone. The entry is typed as an
/// article without a container, which every bundled style lists sensibly.
pub fn entry_from_summary(key: &str, summary: &CitationSummary) -> Entry {
    let mut entry = Entry::new(key, EntryType::Article);
    if !summary.title.trim().is_empty() {
        entry.set_title(FormatString::with_value(summary.title.clone()));
    }
    if !summary.authors.is_empty() {
        entry.set_authors(
            summary
                .authors
                .iter()
                .map(|name| person_from_name(name))
                .collect(),
        );
    }
    if let Some(date) = summary.issued.as_deref().and_then(parse_date) {
        entry.set_date(date);
    }
    if let Some(doi) = &summary.doi {
        entry.set_doi(doi.clone());
    }
    if let Some(url) = summary.url.as_deref().and_then(parse_url) {
        entry.set_url(url);
    }
    entry
}

/// Build an entry from details (the inverse of [`details_from_entry`]).
pub fn entry_from_details(details: &ReferenceDetails) -> Entry {
    let kind = details.kind.parse::<EntryType>().unwrap_or(EntryType::Misc);
    let mut draft = EntryDraft::new(kind);
    draft.title = Some(details.title.clone()).filter(|title| !title.trim().is_empty());
    draft.authors = details
        .authors
        .iter()
        .map(ReferenceAuthor::to_person)
        .collect();
    draft.editors = details
        .editors
        .iter()
        .map(ReferenceAuthor::to_person)
        .collect();
    draft.container_title = details.container_title.clone();
    draft.volume = details.volume.clone();
    draft.issue = details.issue.clone();
    draft.pages = details.pages.clone();
    draft.publisher = details.publisher.clone();
    draft.place = details.place.clone();
    draft.edition = details.edition.clone();
    draft.date = details
        .date
        .clone()
        .or_else(|| details.year.map(|year| year.to_string()));
    draft.doi = details.doi.clone();
    draft.url = details.url.clone();
    draft.isbn = details.isbn.clone();
    draft.issn = details.issn.clone();
    draft.build(&details.id)
}

/// Parse a free-form author string into a person.
///
/// `Family, Given` is used when a comma is present; otherwise the last word is
/// the family name and the leading words are given names, matching BibTeX.
pub(crate) fn person_from_name(name: &str) -> Person {
    let name = name.trim();
    if let Some((family, given)) = name.split_once(',') {
        let family = family.trim();
        let given = given.trim();
        return Person {
            name: family.to_string(),
            given_name: (!given.is_empty()).then(|| given.to_string()),
            prefix: None,
            suffix: None,
            comma_suffix: false,
            alias: None,
        };
    }
    let mut words: Vec<&str> = name.split_whitespace().collect();
    if words.len() <= 1 {
        return Person {
            name: name.to_string(),
            given_name: None,
            prefix: None,
            suffix: None,
            comma_suffix: false,
            alias: None,
        };
    }
    let family = words.pop().unwrap_or_default().to_string();
    let mut particles = Vec::new();
    while words.len() > 1
        && words
            .last()
            .is_some_and(|word| word.chars().next().is_some_and(char::is_lowercase))
    {
        particles.insert(0, words.pop().unwrap_or_default());
    }
    Person {
        name: family,
        given_name: Some(words.join(" ")),
        prefix: (!particles.is_empty()).then(|| particles.join(" ")),
        suffix: None,
        comma_suffix: false,
        alias: None,
    }
}

pub(crate) fn parse_date(value: &str) -> Option<Date> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if let Ok(date) = value.parse::<Date>() {
        return Some(date);
    }
    // Fall back to the first four digit run that looks like a year.
    let digits: String = value
        .chars()
        .skip_while(|ch| !ch.is_ascii_digit())
        .take_while(|ch| ch.is_ascii_digit())
        .collect();
    digits.parse::<i32>().ok().map(Date::from_year)
}

pub(crate) fn parse_url(value: &str) -> Option<QualifiedUrl> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    value
        .parse::<QualifiedUrl>()
        .ok()
        .or_else(|| format!("https://{value}").parse::<QualifiedUrl>().ok())
}

/// Give an entry a new key and normalise types for rendering.
///
/// `hayagriva` keys are immutable, so the entry is round-tripped through its
/// serialised form. BibLaTeX `@incollection` entries arrive as `anthos` inside
/// an `anthology`, which the bundled APA and Chicago styles render with the
/// collection title twice; they are remapped to `chapter` inside a `book`.
pub(crate) fn normalize_entry(entry: Entry, key: &str) -> Entry {
    let needs_type_remap = uses_anthology_types(&entry);
    if entry.key() == key && !needs_type_remap {
        return entry;
    }
    let Ok(mut value) = serde_json::to_value(&entry) else {
        return entry;
    };
    remap_anthology_types(&mut value);
    let mut library = serde_json::Map::new();
    library.insert(key.to_string(), value);
    let Ok(text) = serde_json::to_string(&serde_json::Value::Object(library)) else {
        return entry;
    };
    hayagriva::io::from_yaml_str(&text)
        .ok()
        .and_then(|library| library.into_iter().next())
        .unwrap_or(entry)
}

fn uses_anthology_types(entry: &Entry) -> bool {
    matches!(entry.entry_type(), EntryType::Anthos | EntryType::Anthology)
        || entry.parents().iter().any(uses_anthology_types)
}

fn remap_anthology_types(value: &mut serde_json::Value) {
    let serde_json::Value::Object(map) = value else {
        return;
    };
    if let Some(serde_json::Value::String(kind)) = map.get_mut("type") {
        match kind.as_str() {
            "anthos" => *kind = "chapter".to_string(),
            "anthology" => *kind = "book".to_string(),
            _ => {}
        }
    }
    match map.get_mut("parent") {
        Some(serde_json::Value::Array(parents)) => {
            for parent in parents {
                remap_anthology_types(parent);
            }
        }
        Some(parent) => remap_anthology_types(parent),
        None => {}
    }
}

/// Intermediate representation used by the RIS and CSL-JSON parsers.
#[derive(Clone, Debug, Default)]
pub(crate) struct EntryDraft {
    pub kind: Option<EntryType>,
    pub title: Option<String>,
    pub authors: Vec<Person>,
    pub editors: Vec<Person>,
    pub container_title: Option<String>,
    pub volume: Option<String>,
    pub issue: Option<String>,
    pub pages: Option<String>,
    pub publisher: Option<String>,
    pub place: Option<String>,
    pub edition: Option<String>,
    pub date: Option<String>,
    pub doi: Option<String>,
    pub url: Option<String>,
    pub isbn: Option<String>,
    pub issn: Option<String>,
    pub note: Option<String>,
}

impl EntryDraft {
    pub(crate) fn new(kind: EntryType) -> Self {
        Self {
            kind: Some(kind),
            ..Self::default()
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.title.is_none() && self.authors.is_empty() && self.date.is_none()
    }

    /// Materialise the draft, placing container fields on a parent entry the
    /// way `hayagriva` expects (journal for articles, book for chapters, ...).
    pub(crate) fn build(self, key: &str) -> Entry {
        let kind = self.kind.unwrap_or(EntryType::Misc);
        let parent_kind = match kind {
            EntryType::Article => Some(EntryType::Periodical),
            EntryType::Chapter | EntryType::Entry | EntryType::Anthos => Some(EntryType::Book),
            EntryType::Conference => Some(EntryType::Proceedings),
            _ => None,
        };
        let container_kind = match (parent_kind, self.container_title.is_some()) {
            (Some(kind), _) => Some(kind),
            (None, true) => Some(match kind {
                EntryType::Web | EntryType::Blog | EntryType::Post => EntryType::Web,
                EntryType::Newspaper => EntryType::Periodical,
                _ => EntryType::Misc,
            }),
            (None, false) => None,
        };

        let mut entry = Entry::new(key, kind);
        if let Some(title) = self.title {
            entry.set_title(FormatString::with_value(title));
        }
        if !self.authors.is_empty() {
            entry.set_authors(self.authors);
        }
        if let Some(date) = self.date.as_deref().and_then(parse_date) {
            entry.set_date(date);
        }
        if let Some(pages) = self
            .pages
            .as_deref()
            .map(str::trim)
            .filter(|p| !p.is_empty())
        {
            let Ok(pages) = pages.parse::<MaybeTyped<PageRanges>>();
            entry.set_page_range(pages);
        }
        if let Some(doi) = self.doi {
            entry.set_doi(doi);
        }
        if let Some(url) = self.url.as_deref().and_then(parse_url) {
            entry.set_url(url);
        }
        if let Some(note) = self.note {
            entry.set_note(FormatString::with_value(note));
        }
        if let Some(edition) = self.edition.as_deref().and_then(parse_numeric) {
            entry.set_edition(edition);
        }

        let volume = self.volume.as_deref().and_then(parse_numeric);
        let issue = self.issue.as_deref().and_then(parse_numeric);
        let publisher = (self.publisher.is_some() || self.place.is_some()).then(|| {
            Publisher::new(
                self.publisher.clone().map(FormatString::with_value),
                self.place.clone().map(FormatString::with_value),
            )
        });
        let isbn = self.isbn;
        let issn = self.issn;
        let editors = self.editors;

        match container_kind {
            Some(container_kind) => {
                let mut parent = Entry::new(&format!("{key}-parent"), container_kind);
                if let Some(title) = self.container_title {
                    parent.set_title(FormatString::with_value(title));
                }
                let container_holds_numbers = matches!(
                    container_kind,
                    EntryType::Periodical | EntryType::Proceedings
                );
                if let Some(volume) = volume {
                    if container_holds_numbers {
                        parent.set_volume(volume);
                    } else {
                        entry.set_volume(volume);
                    }
                }
                if let Some(issue) = issue {
                    if container_holds_numbers {
                        parent.set_issue(issue);
                    } else {
                        entry.set_issue(issue);
                    }
                }
                if let Some(publisher) = publisher {
                    parent.set_publisher(publisher);
                }
                if !editors.is_empty() {
                    parent.set_editors(editors);
                }
                if let Some(isbn) = isbn {
                    parent.set_isbn(isbn);
                }
                if let Some(issn) = issn {
                    parent.set_issn(issn);
                }
                entry.set_parents(vec![parent]);
            }
            None => {
                if let Some(volume) = volume {
                    entry.set_volume(volume);
                }
                if let Some(issue) = issue {
                    entry.set_issue(issue);
                }
                if let Some(publisher) = publisher {
                    entry.set_publisher(publisher);
                }
                if !editors.is_empty() {
                    entry.set_editors(editors);
                }
                if let Some(isbn) = isbn {
                    entry.set_isbn(isbn);
                }
                if let Some(issn) = issn {
                    entry.set_issn(issn);
                }
            }
        }
        entry
    }
}

fn parse_numeric(value: &str) -> Option<MaybeTyped<Numeric>> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    value.parse::<MaybeTyped<Numeric>>().ok()
}
