//! CSL rendering of citations, footnotes, and bibliographies via `hayagriva`.

use std::collections::HashMap;

use hayagriva::citationberg::taxonomy::Locator;
use hayagriva::citationberg::{FontStyle, FontWeight};
use hayagriva::{
    BibliographyDriver, BibliographyRequest, CitationItem as HayagrivaItem, CitationRequest, Elem,
    ElemChild, ElemChildren, ElemMeta, Entry, Formatted, Formatting, LocatorPayload,
    SpecificLocator,
};
use opendoc_core::{
    BibliographyReference, CitationDatabase, CitationGroup, CitationPlacement, StableId,
};

use crate::model::reference_entry;
use crate::styles::{load_style, locale_files, resolve_locale, style_is_note, style_is_numeric};
use crate::CitationError;

/// A run of text with lightweight formatting.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RichSegment {
    /// The text of the run.
    pub text: String,
    /// Whether the run is italic (titles of books and journals).
    pub italic: bool,
    /// Whether the run is bold.
    pub bold: bool,
    /// Link target, for DOIs and URLs.
    pub link: Option<String>,
}

impl RichSegment {
    /// Plain text segment.
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ..Self::default()
        }
    }
}

/// Concatenate segments into plain text.
pub fn segments_text(segments: &[RichSegment]) -> String {
    segments
        .iter()
        .map(|segment| segment.text.as_str())
        .collect()
}

/// A rendered citation group.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderedCitation {
    /// Id of the citation group.
    pub citation_id: String,
    /// Plain text (for example `(Doe, 2020, p. 12)` or `[1]`).
    pub text: String,
    /// Formatted runs.
    pub rich: Vec<RichSegment>,
    /// Footnote number assigned by a note style (1-based), if any.
    pub note_number: Option<usize>,
    /// Whether every item referenced a live, citable reference. When false the
    /// text is the `[citation-id]` placeholder.
    pub resolved: bool,
}

/// A rendered bibliography entry with formatting.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RichBibliographyEntry {
    /// Id of the bibliography reference.
    pub reference_id: String,
    /// Citation number for numeric styles (1-based).
    pub number: Option<usize>,
    /// Plain text.
    pub text: String,
    /// Formatted runs.
    pub rich: Vec<RichSegment>,
}

/// Everything rendered for a citation database.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderedDatabase {
    /// Canonical style name that was used.
    pub style: String,
    /// Locale that was used.
    pub locale: String,
    /// Whether the style is numeric.
    pub numeric: bool,
    /// Whether the style is a note style.
    pub note: bool,
    /// One entry per live citation group, in database order.
    pub citations: Vec<RenderedCitation>,
    /// Bibliography entries in the order dictated by the style.
    pub bibliography: Vec<RichBibliographyEntry>,
}

/// Render all live citation groups and the bibliography of a database.
pub fn render_database(database: &CitationDatabase) -> Result<RenderedDatabase, CitationError> {
    let groups: Vec<&CitationGroup> = database
        .citations
        .iter()
        .filter(|citation| !citation.deleted)
        .collect();
    render_groups(database, &groups, None)
}

/// Render all live citation groups of a database.
pub fn render_citations(
    database: &CitationDatabase,
) -> Result<Vec<RenderedCitation>, CitationError> {
    render_database(database).map(|rendered| rendered.citations)
}

/// Render a single citation group in the context of the whole database so
/// numbering and disambiguation stay consistent. The group need not be stored
/// in the database yet; if a stored group shares its id, the given group
/// replaces it.
pub fn render_citation(
    database: &CitationDatabase,
    citation: &CitationGroup,
) -> Result<RenderedCitation, CitationError> {
    render_citation_inner(database, citation, false)
}

/// Render a citation group as footnote content. Note styles number the
/// footnote and may shorten repeated citations; in-text styles produce the
/// same text as an inline citation.
pub fn render_footnote_citation(
    database: &CitationDatabase,
    citation: &CitationGroup,
) -> Result<RenderedCitation, CitationError> {
    render_citation_inner(database, citation, true)
}

fn render_citation_inner(
    database: &CitationDatabase,
    citation: &CitationGroup,
    force_footnote: bool,
) -> Result<RenderedCitation, CitationError> {
    let mut groups: Vec<&CitationGroup> = Vec::new();
    let mut target_index = None;
    for stored in database.citations.iter().filter(|stored| !stored.deleted) {
        if stored.id == citation.id {
            if target_index.is_none() {
                target_index = Some(groups.len());
                groups.push(citation);
            }
        } else {
            groups.push(stored);
        }
    }
    let target_index = target_index.unwrap_or_else(|| {
        groups.push(citation);
        groups.len() - 1
    });
    let forced = force_footnote.then_some(target_index);
    let rendered = render_groups(database, &groups, forced)?;
    rendered
        .citations
        .into_iter()
        .nth(target_index)
        .ok_or_else(|| CitationError::Render("citation group was not rendered".to_string()))
}

/// Render the bibliography with formatting, sorted per the style.
pub fn render_bibliography_rich(
    database: &CitationDatabase,
) -> Result<Vec<RichBibliographyEntry>, CitationError> {
    // No bibliography entries means no CSL state needs rendering. Apart from
    // being the exact result for a new (or all-deleted) database, this keeps
    // an ordinary empty-document projection from recursively decoding a
    // bundled style it cannot possibly use.
    if !database
        .references
        .iter()
        .any(|reference| !reference.deleted)
    {
        return Ok(Vec::new());
    }
    render_database(database).map(|rendered| rendered.bibliography)
}

struct ItemPlan {
    entry_index: usize,
    locator: Option<(Locator, String)>,
    suppress_author: bool,
    prefix: Option<String>,
    suffix: Option<String>,
}

struct GroupPlan {
    id: StableId,
    items: Vec<ItemPlan>,
    note_number: Option<usize>,
    resolved: bool,
}

fn render_groups(
    database: &CitationDatabase,
    groups: &[&CitationGroup],
    force_footnote: Option<usize>,
) -> Result<RenderedDatabase, CitationError> {
    let style = load_style(&database.style)?;
    let numeric = style_is_numeric(&style);
    let note = style_is_note(&style);
    let locale = resolve_locale(&database.locale);
    let locale_files = locale_files();

    // Entries for every live reference, keyed by reference id.
    let mut entries: Vec<Entry> = Vec::new();
    let mut entry_index: HashMap<&str, usize> = HashMap::new();
    let mut references: HashMap<&str, &BibliographyReference> = HashMap::new();
    for reference in database.references.iter().filter(|r| !r.deleted) {
        if entry_index.contains_key(reference.id.as_str()) {
            continue;
        }
        entry_index.insert(reference.id.as_str(), entries.len());
        references.insert(reference.id.as_str(), reference);
        entries.push(reference_entry(reference));
    }
    let citable: Vec<bool> = entries
        .iter()
        .map(|entry| {
            numeric
                || entry.authors().is_some_and(|authors| !authors.is_empty())
                || entry.editors().is_some_and(|editors| !editors.is_empty())
                || entry.date_any().is_some()
        })
        .collect();

    // Plan every group.
    let mut plans: Vec<GroupPlan> = Vec::with_capacity(groups.len());
    let mut footnote_counter = 0usize;
    for (index, group) in groups.iter().enumerate() {
        let mut resolved = true;
        let mut items = Vec::with_capacity(group.items.len());
        for item in &group.items {
            let Some(&entry_index) = entry_index.get(item.reference_id.as_str()) else {
                resolved = false;
                continue;
            };
            if !citable[entry_index] {
                resolved = false;
                continue;
            }
            items.push(ItemPlan {
                entry_index,
                locator: item
                    .locator
                    .as_deref()
                    .map(|locator| plan_locator(item.label.as_deref(), locator)),
                suppress_author: item.suppress_author,
                prefix: item.prefix.clone(),
                suffix: item.suffix.clone(),
            });
        }
        let is_footnote = force_footnote == Some(index)
            || matches!(group.placement, CitationPlacement::Footnote { .. });
        let note_number = if is_footnote {
            footnote_counter += 1;
            Some(footnote_counter)
        } else {
            None
        };
        if items.is_empty() {
            resolved = false;
        }
        plans.push(GroupPlan {
            id: group.id.clone(),
            items,
            note_number,
            resolved,
        });
    }

    // Feed the driver.
    let mut driver: BibliographyDriver<'_, Entry> = BibliographyDriver::new();
    let mut driven: Vec<Option<usize>> = Vec::with_capacity(plans.len());
    let mut cited = vec![false; entries.len()];
    let mut driven_count = 0usize;
    for plan in &plans {
        if !plan.resolved {
            driven.push(None);
            continue;
        }
        let items: Vec<HayagrivaItem<'_, Entry>> = plan
            .items
            .iter()
            .map(|item| {
                cited[item.entry_index] = true;
                HayagrivaItem::with_locator(
                    &entries[item.entry_index],
                    item.locator
                        .as_ref()
                        .map(|(kind, value)| SpecificLocator(*kind, LocatorPayload::Str(value))),
                )
            })
            .collect();
        driver.citation(CitationRequest::new(
            items,
            &style,
            Some(locale.clone()),
            locale_files,
            plan.note_number,
        ));
        driven.push(Some(driven_count));
        driven_count += 1;
    }
    // Uncited references still belong in the bibliography.
    for (index, entry) in entries.iter().enumerate() {
        if cited[index] {
            continue;
        }
        driver.citation(CitationRequest::new(
            vec![HayagrivaItem::new(entry, None, None, true, None)],
            &style,
            Some(locale.clone()),
            locale_files,
            None,
        ));
    }

    let rendered = driver.finish(BibliographyRequest {
        style: &style,
        locale: Some(locale.clone()),
        locale_files,
    });

    // Citations.
    let mut citations = Vec::with_capacity(plans.len());
    for (plan, driven_index) in plans.iter().zip(driven.iter()) {
        let Some(driven_index) = driven_index else {
            citations.push(RenderedCitation {
                citation_id: plan.id.to_string(),
                text: format!("[{}]", plan.id),
                rich: vec![RichSegment::plain(format!("[{}]", plan.id))],
                note_number: plan.note_number,
                resolved: false,
            });
            continue;
        };
        let Some(rendered_citation) = rendered.citations.get(*driven_index) else {
            return Err(CitationError::Render(format!(
                "citation group {} was not rendered",
                plan.id
            )));
        };
        let mut children = rendered_citation.citation.clone();
        for (item_index, item) in plan.items.iter().enumerate() {
            if item.suppress_author {
                if let Some(elem) = find_entry_elem(&mut children, item_index) {
                    if remove_names(&mut elem.children.0) {
                        trim_leading_delimiters(&mut elem.children.0);
                    }
                }
            }
            if let Some(prefix) = item
                .prefix
                .as_deref()
                .map(str::trim)
                .filter(|p| !p.is_empty())
            {
                if let Some(elem) = find_entry_elem(&mut children, item_index) {
                    elem.children.0.insert(0, plain_child(format!("{prefix} ")));
                }
            }
            if let Some(suffix) = item
                .suffix
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                if let Some(elem) = find_entry_elem(&mut children, item_index) {
                    elem.children.0.push(plain_child(format!(" {suffix}")));
                }
            }
        }
        let rich = segments_from_children(&children);
        let text = segments_text(&rich);
        citations.push(RenderedCitation {
            citation_id: plan.id.to_string(),
            text,
            rich,
            note_number: rendered_citation.note_number.or(plan.note_number),
            resolved: true,
        });
    }

    // Bibliography.
    let mut bibliography = Vec::new();
    if let Some(rendered_bibliography) = rendered.bibliography {
        for (position, item) in rendered_bibliography.items.iter().enumerate() {
            let mut rich = Vec::new();
            if let Some(first_field) = &item.first_field {
                push_child(first_field, &mut rich);
                if rich
                    .last()
                    .is_some_and(|segment| !segment.text.ends_with(' '))
                {
                    rich.push(RichSegment::plain(" "));
                }
            }
            push_children(&item.content, &mut rich);
            let mut rich = merge_segments(rich);
            let mut text = segments_text(&rich).trim().to_string();
            if text.is_empty() {
                // Some styles list nothing for certain kinds of work; fall
                // back to the plain legacy entry rather than a blank line.
                if let Some(reference) = references.get(item.key.as_str()) {
                    text = crate::render_bibliography_reference(reference, position + 1, numeric);
                    rich = vec![RichSegment::plain(text.clone())];
                }
            }
            bibliography.push(RichBibliographyEntry {
                reference_id: item.key.clone(),
                number: numeric.then_some(position + 1),
                text,
                rich,
            });
        }
    }

    Ok(RenderedDatabase {
        style: crate::styles::resolve_style_name(&database.style)
            .unwrap_or_else(|| database.style.clone()),
        locale: locale.0,
        numeric,
        note,
        citations,
        bibliography,
    })
}

/// Map a core label plus locator string onto a CSL locator.
fn plan_locator(label: Option<&str>, locator: &str) -> (Locator, String) {
    let normalized = label
        .map(|label| label.trim().trim_end_matches('.').to_ascii_lowercase())
        .unwrap_or_default();
    let kind = match normalized.as_str() {
        "" | "page" | "pages" | "p" | "pp" | "pg" => Some(Locator::Page),
        "chapter" | "chap" | "ch" => Some(Locator::Chapter),
        "section" | "sec" | "§" => Some(Locator::Section),
        "paragraph" | "para" | "paragraphs" | "¶" => Some(Locator::Paragraph),
        "figure" | "fig" => Some(Locator::Figure),
        "table" | "tab" => Some(Locator::Table),
        "line" | "lines" | "l" | "ll" => Some(Locator::Line),
        "note" | "n" | "footnote" => Some(Locator::Note),
        "volume" | "vol" => Some(Locator::Volume),
        "verse" | "v" => Some(Locator::Verse),
        "part" | "pt" => Some(Locator::Part),
        "column" | "col" => Some(Locator::Column),
        "book" | "bk" => Some(Locator::Book),
        "issue" | "no" | "number" => Some(Locator::Issue),
        "folio" | "fol" => Some(Locator::Folio),
        "opus" | "op" => Some(Locator::Opus),
        "sub verbo" | "sub-verbo" | "s.v" | "sv" => Some(Locator::SubVerbo),
        "supplement" | "supp" => Some(Locator::Supplement),
        "appendix" | "app" => Some(Locator::Appendix),
        "act" => Some(Locator::Act),
        "scene" | "sc" => Some(Locator::Scene),
        "canon" => Some(Locator::Canon),
        "equation" | "eq" => Some(Locator::Equation),
        "rule" => Some(Locator::Rule),
        "title" => Some(Locator::Title),
        "timestamp" | "time" => Some(Locator::Timestamp),
        "article" | "art" => Some(Locator::ArticleLocator),
        "elocation" | "elocation-id" => Some(Locator::Elocation),
        _ => None,
    };
    match kind {
        Some(kind) => (kind, locator.trim().to_string()),
        None => (
            Locator::Custom,
            format!("{} {}", label.unwrap_or_default().trim(), locator.trim()),
        ),
    }
}

fn plain_child(text: String) -> ElemChild {
    ElemChild::Text(Formatted {
        text,
        formatting: Formatting::default(),
    })
}

/// Path (child indices) to the element rendered for citation item `index`.
fn entry_path(children: &[ElemChild], index: usize, path: &mut Vec<usize>) -> bool {
    for (position, child) in children.iter().enumerate() {
        if let ElemChild::Elem(elem) = child {
            path.push(position);
            if matches!(elem.meta, Some(ElemMeta::Entry(entry)) if entry == index) {
                return true;
            }
            if entry_path(&elem.children.0, index, path) {
                return true;
            }
            path.pop();
        }
    }
    false
}

fn find_entry_elem(children: &mut ElemChildren, index: usize) -> Option<&mut Elem> {
    let mut path = Vec::new();
    if !entry_path(&children.0, index, &mut path) {
        return None;
    }
    let mut current: &mut Vec<ElemChild> = &mut children.0;
    let (last, rest) = path.split_last()?;
    for &position in rest {
        match current.get_mut(position)? {
            ElemChild::Elem(elem) => current = &mut elem.children.0,
            _ => return None,
        }
    }
    match current.get_mut(*last)? {
        ElemChild::Elem(elem) => Some(elem),
        _ => None,
    }
}

/// Remove the first `cs:names` element (depth first).
fn remove_names(children: &mut Vec<ElemChild>) -> bool {
    for position in 0..children.len() {
        let is_names = matches!(
            &children[position],
            ElemChild::Elem(elem) if matches!(elem.meta, Some(ElemMeta::Names(_)))
        );
        if is_names {
            children.remove(position);
            return true;
        }
        if let ElemChild::Elem(elem) = &mut children[position] {
            if remove_names(&mut elem.children.0) {
                return true;
            }
        }
    }
    false
}

/// Drop delimiters and whitespace left dangling at the start of an item after
/// the author names were removed.
fn trim_leading_delimiters(children: &mut Vec<ElemChild>) {
    loop {
        let Some(first) = children.first_mut() else {
            return;
        };
        match first {
            ElemChild::Text(formatted) => {
                let trimmed = formatted
                    .text
                    .trim_start_matches(|ch: char| {
                        ch == ',' || ch == ';' || ch == ':' || ch.is_whitespace()
                    })
                    .to_string();
                if trimmed.is_empty() {
                    children.remove(0);
                    continue;
                }
                formatted.text = trimmed;
                return;
            }
            ElemChild::Elem(elem) => {
                trim_leading_delimiters(&mut elem.children.0);
                if elem.children.0.is_empty() {
                    children.remove(0);
                    continue;
                }
                return;
            }
            _ => return,
        }
    }
}

/// Flatten rendered children into formatted runs.
pub(crate) fn segments_from_children(children: &ElemChildren) -> Vec<RichSegment> {
    let mut out = Vec::new();
    push_children(children, &mut out);
    merge_segments(out)
}

fn push_children(children: &ElemChildren, out: &mut Vec<RichSegment>) {
    for child in &children.0 {
        push_child(child, out);
    }
}

fn push_child(child: &ElemChild, out: &mut Vec<RichSegment>) {
    match child {
        ElemChild::Text(formatted) => out.push(segment(formatted, None)),
        ElemChild::Elem(elem) => push_children(&elem.children, out),
        ElemChild::Markup(markup) => out.push(RichSegment::plain(markup.clone())),
        ElemChild::Link { text, url } => out.push(segment(text, Some(url.clone()))),
        ElemChild::Transparent { .. } => {}
    }
}

fn segment(formatted: &Formatted, link: Option<String>) -> RichSegment {
    RichSegment {
        text: formatted.text.clone(),
        italic: formatted.formatting.font_style == FontStyle::Italic,
        bold: formatted.formatting.font_weight == FontWeight::Bold,
        link,
    }
}

fn merge_segments(segments: Vec<RichSegment>) -> Vec<RichSegment> {
    let mut merged: Vec<RichSegment> = Vec::with_capacity(segments.len());
    for segment in segments {
        if segment.text.is_empty() {
            continue;
        }
        if let Some(last) = merged.last_mut() {
            if last.italic == segment.italic
                && last.bold == segment.bold
                && last.link == segment.link
            {
                last.text.push_str(&segment.text);
                continue;
            }
        }
        merged.push(segment);
    }
    merged
}
