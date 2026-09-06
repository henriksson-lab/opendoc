//! Style and locale registry backed by the CSL styles bundled with `hayagriva`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use hayagriva::archive::{locales, ArchivedStyle};
use hayagriva::citationberg::{
    CitationFormat, IndependentStyle, Locale, LocaleCode, Style, StyleCategory, StyleClass,
};

use crate::CitationError;

/// Metadata about a citation style that can be selected by name.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CitationStyleInfo {
    /// Canonical style name accepted by [`crate::render_citation`] and friends.
    pub name: String,
    /// Human readable label for menus.
    pub label: String,
    /// Identifier of the underlying CSL style (`info/id`).
    pub csl_id: String,
    /// Whether citations are numbered (`[1]`) rather than author-date.
    pub numeric: bool,
    /// Whether the style is a note style whose citations belong in footnotes.
    pub note: bool,
}

/// Curated styles: `(name, label, archive name)`.
const CURATED_STYLES: &[(&str, &str, &str)] = &[
    ("apa", "APA (7th edition)", "apa"),
    ("mla", "MLA (9th edition)", "mla"),
    (
        "chicago-author-date",
        "Chicago (author-date, 17th edition)",
        "chicago-author-date",
    ),
    (
        "chicago-notes",
        "Chicago (notes and bibliography, 17th edition)",
        "chicago-notes",
    ),
    ("ieee", "IEEE", "ieee"),
    ("vancouver", "Vancouver", "vancouver"),
    (
        "harvard",
        "Harvard (Cite Them Right)",
        "harvard-cite-them-right",
    ),
    ("nature", "Nature", "nature"),
];

/// Aliases that map onto a curated style name.
const STYLE_ALIASES: &[(&str, &str)] = &[
    ("american-psychological-association", "apa"),
    ("modern-language-association", "mla"),
    ("mla-9", "mla"),
    ("mla-8", "mla"),
    ("chicago", "chicago-author-date"),
    ("chicago-fullnotes", "chicago-notes"),
    ("chicago-notes-bibliography", "chicago-notes"),
    ("institute-of-electrical-and-electronics-engineers", "ieee"),
    ("nlm-citation-sequence", "vancouver"),
    ("harvard-cite-them-right", "harvard"),
];

/// Resolve a user supplied style name (case-insensitive, aliases allowed) to a
/// canonical name. Curated names are returned as-is; any other style bundled
/// with `hayagriva` resolves to its primary archive name. Returns `None` for
/// names that no CSL style is known for (those keep the legacy renderer).
pub fn resolve_style_name(name: &str) -> Option<String> {
    let normalized = name.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return None;
    }
    if let Some((canonical, _, _)) = CURATED_STYLES
        .iter()
        .find(|(canonical, _, _)| *canonical == normalized)
    {
        return Some((*canonical).to_string());
    }
    if let Some((_, canonical)) = STYLE_ALIASES.iter().find(|(alias, _)| *alias == normalized) {
        return Some((*canonical).to_string());
    }
    let archived = ArchivedStyle::by_name(&normalized)?;
    if let Some((canonical, _, _)) = CURATED_STYLES
        .iter()
        .find(|(_, _, archive_name)| ArchivedStyle::by_name(archive_name) == Some(archived))
    {
        return Some((*canonical).to_string());
    }
    archived.names().first().map(|name| (*name).to_string())
}

fn archived_style(name: &str) -> Option<ArchivedStyle> {
    let canonical = resolve_style_name(name)?;
    let archive_name = CURATED_STYLES
        .iter()
        .find(|(candidate, _, _)| *candidate == canonical)
        .map(|(_, _, archive_name)| *archive_name)
        .unwrap_or(canonical.as_str());
    ArchivedStyle::by_name(archive_name)
}

fn style_cache() -> &'static Mutex<HashMap<String, Arc<IndependentStyle>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Arc<IndependentStyle>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Load (and cache) the independent CSL style for a style name.
pub fn load_style(name: &str) -> Result<Arc<IndependentStyle>, CitationError> {
    let canonical =
        resolve_style_name(name).ok_or_else(|| CitationError::UnknownStyle(name.to_string()))?;
    if let Ok(cache) = style_cache().lock() {
        if let Some(style) = cache.get(&canonical) {
            return Ok(style.clone());
        }
    }
    let archived =
        archived_style(&canonical).ok_or_else(|| CitationError::UnknownStyle(name.to_string()))?;
    let style = match archived.get() {
        Style::Independent(style) => Arc::new(style),
        Style::Dependent(dependent) => {
            let parent = ArchivedStyle::by_id(&dependent.parent_link.href)
                .ok_or_else(|| CitationError::UnknownStyle(name.to_string()))?;
            match parent.get() {
                Style::Independent(style) => Arc::new(style),
                Style::Dependent(_) => {
                    return Err(CitationError::UnknownStyle(name.to_string()));
                }
            }
        }
    };
    if let Ok(mut cache) = style_cache().lock() {
        cache.insert(canonical, style.clone());
    }
    Ok(style)
}

/// Whether a style formats citations as numbers.
pub(crate) fn style_is_numeric(style: &IndependentStyle) -> bool {
    style.info.category.iter().any(|category| {
        matches!(
            category,
            StyleCategory::CitationFormat {
                format: CitationFormat::Numeric | CitationFormat::Label
            }
        )
    })
}

/// Whether a style places citations in notes.
pub(crate) fn style_is_note(style: &IndependentStyle) -> bool {
    style.settings.class == StyleClass::Note
}

/// Describe a style by name.
pub fn style_info(name: &str) -> Result<CitationStyleInfo, CitationError> {
    let canonical =
        resolve_style_name(name).ok_or_else(|| CitationError::UnknownStyle(name.to_string()))?;
    let style = load_style(&canonical)?;
    let label = CURATED_STYLES
        .iter()
        .find(|(candidate, _, _)| *candidate == canonical)
        .map(|(_, label, _)| (*label).to_string())
        .or_else(|| archived_style(&canonical).map(|archived| archived.display_name().to_string()))
        .unwrap_or_else(|| canonical.clone());
    Ok(CitationStyleInfo {
        name: canonical,
        label,
        csl_id: style.info.id.clone(),
        numeric: style_is_numeric(&style),
        note: style_is_note(&style),
    })
}

/// The curated list of styles offered to users, in display order.
pub fn available_styles() -> Vec<CitationStyleInfo> {
    CURATED_STYLES
        .iter()
        .filter_map(|(name, _, _)| style_info(name).ok())
        .collect()
}

/// Every style name bundled with the CSL archive (curated and otherwise).
pub fn all_style_names() -> Vec<String> {
    let mut names: Vec<String> = CURATED_STYLES
        .iter()
        .map(|(name, _, _)| (*name).to_string())
        .collect();
    for archived in ArchivedStyle::all() {
        if let Some(name) = archived.names().first() {
            if resolve_style_name(name).is_some_and(|canonical| !names.contains(&canonical)) {
                names.push((*name).to_string());
            }
        }
    }
    names
}

/// All CSL locale files bundled with `hayagriva`, loaded once.
pub(crate) fn locale_files() -> &'static [Locale] {
    static LOCALES: OnceLock<Vec<Locale>> = OnceLock::new();
    LOCALES.get_or_init(locales)
}

/// Locale codes (for example `en-US`, `de-DE`) with bundled CSL locale data.
pub fn available_locales() -> Vec<String> {
    let mut codes: Vec<String> = locale_files()
        .iter()
        .filter_map(|locale| locale.lang.as_ref().map(|code| code.0.clone()))
        .collect();
    codes.sort();
    codes.dedup();
    codes
}

/// Resolve a locale string to a bundled locale code. Exact matches win, then
/// the first locale sharing the base language, then `en-US`.
pub fn resolve_locale(code: &str) -> LocaleCode {
    let requested = code.trim().replace('_', "-");
    let available = available_locales();
    if let Some(exact) = available
        .iter()
        .find(|candidate| candidate.eq_ignore_ascii_case(&requested))
    {
        return LocaleCode(exact.clone());
    }
    let base = requested
        .split('-')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !base.is_empty() {
        let preferred = match base.as_str() {
            "en" => Some("en-US"),
            "de" => Some("de-DE"),
            "fr" => Some("fr-FR"),
            "sv" => Some("sv-SE"),
            "es" => Some("es-ES"),
            "pt" => Some("pt-PT"),
            "zh" => Some("zh-CN"),
            _ => None,
        };
        if let Some(preferred) = preferred {
            if available.iter().any(|candidate| candidate == preferred) {
                return LocaleCode(preferred.to_string());
            }
        }
        if let Some(candidate) = available.iter().find(|candidate| {
            candidate
                .split('-')
                .next()
                .is_some_and(|candidate_base| candidate_base.eq_ignore_ascii_case(&base))
        }) {
            return LocaleCode(candidate.clone());
        }
    }
    LocaleCode::en_us()
}
