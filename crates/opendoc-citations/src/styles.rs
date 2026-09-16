//! The CSL styles and locales OpenDoc bundles, and the registry over them.
//!
//! # Why the crate carries its own subset
//!
//! `hayagriva`'s `archive` feature bundles 91 CSL styles and 64 CSL locale
//! files as `include_bytes!` constants reached through a match on
//! `ArchivedStyle`, so the whole 3.13 MB is in every binary that can name any
//! style. Measured in the browser module, that was **2.97 MB of a 5.22 MB
//! data section — 19% of the entire 15.3 MB WASM download**, twenty times the
//! bundled fonts and the largest single item in it, paid before a citation is
//! formatted.
//!
//! What the product actually reaches is eight styles: the ones the citation
//! style dialog offers, which is the only way a style name enters a document
//! other than by hand. So the eight are vendored here, as
//! `assets/styles/*.cbor`, copied byte-for-byte out of
//! `hayagriva-0.10.1/archive/` — the same bytes the archive feature would
//! have handed us, so no citation renders differently than before — and
//! `hayagriva` is built with `default-features = false` so the module is not
//! compiled at all and cannot contribute a byte.
//!
//! The set is deliberately a table, not a policy: adding a style is copying
//! one `.cbor` file and adding one row (`~6-180 KB`), and adding a locale is
//! the same (`~18 KB`). What the bundle does *not* hold degrades — see
//! [`citation_support_warnings`] — it never renders wrongly and never panics.
//!
//! Bundled locales are the ones [`resolve_locale`] privileges by base
//! language plus `en-GB`; every other request falls back to the nearest
//! bundled language and says so.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use hayagriva::citationberg::{
    CitationFormat, IndependentStyle, Locale, LocaleCode, Style, StyleCategory, StyleClass,
};
use opendoc_core::{CitationDatabase, ModelWarning};

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

/// One bundled CSL style: its OpenDoc name, its menu label, the name of the
/// `hayagriva` archive entry the bytes were copied from, and those bytes.
///
/// `static`, not `const`: a `const` is inlined at every use site, which is
/// how the bundled fonts came to be embedded twice in the WASM module.
struct BundledStyle {
    name: &'static str,
    label: &'static str,
    /// Provenance. `hayagriva-0.10.1/archive/styles/<archive_file>.cbor`.
    archive_file: &'static str,
    bytes: &'static [u8],
}

static BUNDLED_STYLES: &[BundledStyle] = &[
    BundledStyle {
        name: "apa",
        label: "APA (7th edition)",
        archive_file: "apa",
        bytes: include_bytes!("../assets/styles/apa.cbor"),
    },
    BundledStyle {
        name: "mla",
        label: "MLA (9th edition)",
        archive_file: "modern-language-association",
        bytes: include_bytes!("../assets/styles/modern-language-association.cbor"),
    },
    BundledStyle {
        name: "chicago-author-date",
        label: "Chicago (author-date, 17th edition)",
        archive_file: "chicago-author-date",
        bytes: include_bytes!("../assets/styles/chicago-author-date.cbor"),
    },
    BundledStyle {
        name: "chicago-notes",
        label: "Chicago (notes and bibliography, 17th edition)",
        archive_file: "chicago-notes-bibliography",
        bytes: include_bytes!("../assets/styles/chicago-notes-bibliography.cbor"),
    },
    BundledStyle {
        name: "ieee",
        label: "IEEE",
        archive_file: "ieee",
        bytes: include_bytes!("../assets/styles/ieee.cbor"),
    },
    BundledStyle {
        name: "vancouver",
        label: "Vancouver",
        archive_file: "nlm-citation-sequence",
        bytes: include_bytes!("../assets/styles/nlm-citation-sequence.cbor"),
    },
    BundledStyle {
        name: "harvard",
        label: "Harvard (Cite Them Right)",
        archive_file: "harvard-cite-them-right",
        bytes: include_bytes!("../assets/styles/harvard-cite-them-right.cbor"),
    },
    BundledStyle {
        name: "nature",
        label: "Nature",
        archive_file: "nature",
        bytes: include_bytes!("../assets/styles/nature.cbor"),
    },
];

/// Aliases that map onto a bundled style name.
///
/// Every alias `hayagriva` itself knew for one of the eight is here, so a
/// document written when the whole archive was bundled still resolves the
/// name it stored.
static STYLE_ALIASES: &[(&str, &str)] = &[
    ("american-psychological-association", "apa"),
    ("modern-language-association", "mla"),
    ("modern-language-association-8", "mla"),
    ("mla-9", "mla"),
    ("mla-8", "mla"),
    ("chicago", "chicago-author-date"),
    ("chicago-fullnotes", "chicago-notes"),
    ("chicago-notes-bibliography", "chicago-notes"),
    ("institute-of-electrical-and-electronics-engineers", "ieee"),
    ("nlm-citation-sequence", "vancouver"),
    ("harvard-cite-them-right", "harvard"),
];

/// The bundled CSL locale files, keyed by their exact locale code.
static BUNDLED_LOCALES: &[(&str, &[u8])] = &[
    ("de-DE", include_bytes!("../assets/locales/de-DE.cbor")),
    ("en-GB", include_bytes!("../assets/locales/en-GB.cbor")),
    ("en-US", include_bytes!("../assets/locales/en-US.cbor")),
    ("es-ES", include_bytes!("../assets/locales/es-ES.cbor")),
    ("fr-FR", include_bytes!("../assets/locales/fr-FR.cbor")),
    ("pt-PT", include_bytes!("../assets/locales/pt-PT.cbor")),
    ("sv-SE", include_bytes!("../assets/locales/sv-SE.cbor")),
    ("zh-CN", include_bytes!("../assets/locales/zh-CN.cbor")),
];

/// Warning code for a citation style OpenDoc does not bundle CSL data for.
pub const UNBUNDLED_STYLE_WARNING: &str = "citation-style-not-bundled";
/// Warning code for a citation locale OpenDoc does not bundle CSL data for.
pub const UNBUNDLED_LOCALE_WARNING: &str = "citation-locale-not-bundled";

fn bundled_style(name: &str) -> Option<&'static BundledStyle> {
    BUNDLED_STYLES.iter().find(|style| style.name == name)
}

/// Resolve a user supplied style name (case-insensitive, aliases allowed) to
/// a canonical bundled name. Returns `None` for every other name — including
/// a CSL style that exists upstream but is not bundled here, and OpenDoc's
/// own `numeric`, both of which keep the lightweight built-in renderer; the
/// first is reported by [`citation_support_warnings`] and the second is not,
/// because a built-in style is not a degradation.
pub fn resolve_style_name(name: &str) -> Option<String> {
    let normalized = name.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return None;
    }
    if let Some(style) = bundled_style(&normalized) {
        return Some(style.name.to_string());
    }
    STYLE_ALIASES
        .iter()
        .find(|(alias, _)| *alias == normalized)
        .map(|(_, canonical)| (*canonical).to_string())
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
    let bundled =
        bundled_style(&canonical).ok_or_else(|| CitationError::UnknownStyle(name.to_string()))?;
    let style = match decode::<Style>(bundled.bytes) {
        // A dependent style only names a parent; none of the eight is one, and
        // bundling a parent for it would be a second entry in the table.
        Some(Style::Independent(style)) => Arc::new(style),
        Some(Style::Dependent(_)) | None => {
            return Err(CitationError::UnknownStyle(name.to_string()));
        }
    };
    if let Ok(mut cache) = style_cache().lock() {
        cache.insert(canonical, style.clone());
    }
    Ok(style)
}

/// The vendored files are CBOR of `citationberg`'s own types, so a decode
/// failure means the pinned `citationberg` no longer matches the bytes rather
/// than that the document asked for something odd. It is not silent: the
/// style falls back and `bundled_csl_data_decodes` fails in `cargo test`.
// citationberg's serialised CSL grammar is deeply nested. On native targets
// decoding an otherwise ordinary APA style while projecting a large app DTO
// can exhaust the small stack that Rust's test workers use. This is only a
// one-time cold path — callers cache the decoded style/locale — so give that
// parser an explicit worker stack rather than weakening CSL rendering or
// silently falling back to the legacy formatter.
#[cfg(not(target_arch = "wasm32"))]
fn decode<T>(bytes: &'static [u8]) -> Option<T>
where
    T: serde::de::DeserializeOwned + Send + 'static,
{
    std::thread::Builder::new()
        .name("opendoc-csl-decode".to_string())
        .stack_size(8 * 1024 * 1024)
        .spawn(move || ciborium::de::from_reader(bytes).ok())
        .ok()?
        .join()
        .ok()?
}

// Browser builds have no native worker threads. Their stack is supplied by
// the WebAssembly runtime, so retain the direct, dependency-free decoder.
#[cfg(target_arch = "wasm32")]
fn decode<T: serde::de::DeserializeOwned>(bytes: &'static [u8]) -> Option<T> {
    ciborium::de::from_reader(bytes).ok()
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
    let label = bundled_style(&canonical)
        .map(|bundled| bundled.label.to_string())
        .unwrap_or_else(|| canonical.clone());
    Ok(CitationStyleInfo {
        name: canonical,
        label,
        csl_id: style.info.id.clone(),
        numeric: style_is_numeric(&style),
        note: style_is_note(&style),
    })
}

/// The styles offered to users, in display order.
pub fn available_styles() -> Vec<CitationStyleInfo> {
    BUNDLED_STYLES
        .iter()
        .filter_map(|style| style_info(style.name).ok())
        .collect()
}

/// Every style name OpenDoc bundles CSL data for, in display order.
pub fn bundled_style_names() -> Vec<&'static str> {
    BUNDLED_STYLES.iter().map(|style| style.name).collect()
}

/// Where each bundled style's bytes came from: `(OpenDoc name, hayagriva
/// archive file stem)`. Provenance, so a refresh can be checked rather than
/// remembered.
pub fn bundled_style_provenance() -> Vec<(&'static str, &'static str)> {
    BUNDLED_STYLES
        .iter()
        .map(|style| (style.name, style.archive_file))
        .collect()
}

/// The bundled CSL locale files, loaded once.
pub(crate) fn locale_files() -> &'static [Locale] {
    static LOCALES: OnceLock<Vec<Locale>> = OnceLock::new();
    LOCALES.get_or_init(|| {
        BUNDLED_LOCALES
            .iter()
            .filter_map(|(_, bytes)| decode::<Locale>(bytes))
            .collect()
    })
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

fn base_language(code: &str) -> String {
    code.replace('_', "-")
        .split('-')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

/// Everything about a citation database's style and locale that the bundled
/// CSL data cannot serve exactly.
///
/// This is the degradation channel for the trimmed bundle (ADR 0003's rule:
/// an unsupported input degrades and says so, it never renders wrongly and
/// never fails). A database with no citations and no references reports
/// nothing — a document that does not cite anything is not affected by which
/// styles exist.
pub fn citation_support_warnings(database: &CitationDatabase) -> Vec<ModelWarning> {
    if database.citations.is_empty() && database.references.is_empty() {
        return Vec::new();
    }
    let mut warnings = Vec::new();
    let style = database.style.trim();
    if resolve_style_name(style).is_none() && !crate::is_legacy_wrapper_style(style) {
        warnings.push(ModelWarning {
            code: UNBUNDLED_STYLE_WARNING.to_string(),
            message: format!(
                "citation style \"{style}\" is not one of the CSL styles OpenDoc bundles ({}), so citations and the bibliography were formatted by the built-in renderer instead",
                bundled_style_names().join(", ")
            ),
        });
    }
    // The line drawn is the *language*, not the exact code: asking for `de`
    // or `de-AT` and getting `de-DE` is the resolver doing its job, but
    // asking for `ja-JP` and getting English is a degradation.
    let requested = database.locale.trim();
    let resolved = resolve_locale(requested);
    let requested_base = base_language(requested);
    if !requested.is_empty() && requested_base != base_language(&resolved.0) {
        warnings.push(ModelWarning {
            code: UNBUNDLED_LOCALE_WARNING.to_string(),
            message: format!(
                "citation locale \"{requested}\" is not one of the CSL locales OpenDoc bundles ({}), so citation terms were rendered in {} instead",
                available_locales().join(", "),
                resolved.0
            ),
        });
    }
    warnings
}
