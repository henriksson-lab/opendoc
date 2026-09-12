//! HTML escaping, attribute quoting and href/CSS value sanitisation.

use crate::*;

pub(crate) fn base64_encode(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

pub(crate) fn attr(value: &str) -> String {
    escape_html(value)
}

/// Keep only characters that are safe inside a CSS declaration value.
pub(crate) fn css_value(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.len() > 64 {
        return None;
    }
    if trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || " #,.()%-_'".contains(ch))
    {
        Some(trimmed.to_string())
    } else {
        None
    }
}

pub(crate) fn safe_href(href: &str) -> String {
    let trimmed = href.trim();
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("mailto:")
        || lower.starts_with("doi:")
        || lower.starts_with('#')
    {
        trimmed.to_string()
    } else if !trimmed.is_empty() && !lower.contains(':') {
        format!("https://{trimmed}")
    } else {
        "#".to_string()
    }
}
