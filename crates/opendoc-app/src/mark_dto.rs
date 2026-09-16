//! Mark labels, projection and parsing, plus inline offset helpers.

use super::*;

pub(crate) fn inline_id(inline: &Inline) -> &StableId {
    match inline {
        Inline::Text { id, .. }
        | Inline::Link { id, .. }
        | Inline::Citation { id, .. }
        | Inline::FootnoteRef { id, .. }
        | Inline::Mention { id, .. }
        | Inline::GooglePersonChip { id, .. }
        | Inline::GoogleRichLinkChip { id, .. }
        | Inline::Dropdown { id, .. }
        | Inline::DateChip { id, .. }
        | Inline::Equation { id, .. }
        | Inline::PageNumber { id, .. } => id,
    }
}

pub(crate) fn byte_offset_for_char_offset(text: &str, offset: usize) -> Result<usize, AppApiError> {
    if offset == text.chars().count() {
        return Ok(text.len());
    }
    text.char_indices()
        .map(|(index, _)| index)
        .nth(offset)
        .ok_or_else(|| {
            AppApiError::Format(format!(
                "text split offset {offset} is outside text length {}",
                text.chars().count()
            ))
        })
}

pub(crate) fn mark_label(mark: &Mark) -> String {
    let expand = match mark.expand {
        MarkExpand::None => "none",
        MarkExpand::Start => "start",
        MarkExpand::End => "end",
        MarkExpand::Both => "both",
    };
    let kind = mark_kind_label(&mark.kind);
    match &mark.value {
        Some(value) => format!("{kind}:{value}:{expand}"),
        None => format!("{kind}:{expand}"),
    }
}

/// The paragraph-style picker's value for a block. Lists name their marker
/// rather than saying "ordered or not", so a checklist is a selectable style
/// instead of being indistinguishable from a bullet.
pub(crate) fn block_style_value(kind: &BlockKind) -> String {
    match kind {
        BlockKind::Title => "title".to_string(),
        BlockKind::Subtitle => "subtitle".to_string(),
        BlockKind::Heading { level } => format!("heading:{level}"),
        BlockKind::ListItem { kind, .. } => format!("list:{}", kind.as_str()),
        BlockKind::HorizontalRule => "horizontal-rule".to_string(),
        BlockKind::TableOfContents { .. } => "table-of-contents".to_string(),
        BlockKind::Bibliography => "bibliography".to_string(),
        _ => "paragraph".to_string(),
    }
}

pub(crate) fn mark_projection(marks: &[Mark]) -> (Vec<String>, BTreeMap<String, String>) {
    let mut kinds = Vec::new();
    let mut values = BTreeMap::new();
    for mark in marks {
        let kind = mark_kind_label(&mark.kind).to_string();
        if !kinds.contains(&kind) {
            kinds.push(kind.clone());
        }
        if let Some(value) = &mark.value {
            values.insert(kind, value.clone());
        }
    }
    (kinds, values)
}

fn mark_kind_label(kind: &MarkKind) -> &'static str {
    match kind {
        MarkKind::Bold => "bold",
        MarkKind::Italic => "italic",
        MarkKind::Underline => "underline",
        MarkKind::Strike => "strike",
        MarkKind::Code => "code",
        MarkKind::Superscript => "superscript",
        MarkKind::Subscript => "subscript",
        MarkKind::Color => "color",
        MarkKind::Background => "background",
        MarkKind::Font => "font",
        MarkKind::Size => "size",
        MarkKind::Link => "link",
        MarkKind::Citation => "citation",
    }
}

pub(crate) fn parse_marks(labels: &[String]) -> Result<Vec<Mark>, AppApiError> {
    let mut marks = Vec::new();
    for label in labels {
        if let Some(mark) = parse_mark_label(label)? {
            marks.push(mark);
        }
    }
    Ok(marks)
}

fn parse_mark_label(label: &str) -> Result<Option<Mark>, AppApiError> {
    let mut parts = label.split(':');
    let kind = parts.next().unwrap_or_default();
    let second = parts.next();
    let third = parts.next();
    if parts.next().is_some() {
        return Ok(None);
    }
    let (value, expand) = match (second, third) {
        (Some(expand), None) => (None, expand),
        (Some(value), Some(expand)) => (Some(value.to_string()), expand),
        _ => return Ok(None),
    };
    let kind = parse_mark_kind(kind)?;
    validate_mark_payload(&kind, value.as_deref())?;
    Ok(Some(Mark {
        kind,
        value,
        expand: match expand {
            "none" => MarkExpand::None,
            "start" => MarkExpand::Start,
            "end" => MarkExpand::End,
            "both" => MarkExpand::Both,
            _ => return Ok(None),
        },
    }))
}

pub(crate) fn validate_mark_payload(
    kind: &MarkKind,
    value: Option<&str>,
) -> Result<(), AppApiError> {
    let needs_value = matches!(
        kind,
        MarkKind::Color | MarkKind::Background | MarkKind::Font | MarkKind::Size
    );
    match (value, needs_value) {
        (Some(mark_value), true) if mark_value.trim().is_empty() => {
            Err(AppApiError::Format("mark value is empty".to_string()))
        }
        (None, true) => Err(AppApiError::Format("mark value is missing".to_string())),
        (Some(_), false) => Err(AppApiError::Format("boolean mark has value".to_string())),
        _ => Ok(()),
    }
}

pub(crate) fn validate_mark_removal_payload(
    kind: &MarkKind,
    value: Option<&str>,
) -> Result<(), AppApiError> {
    let supports_value = matches!(
        kind,
        MarkKind::Color | MarkKind::Background | MarkKind::Font | MarkKind::Size
    );
    match (value, supports_value) {
        (Some(mark_value), true) if mark_value.trim().is_empty() => {
            Err(AppApiError::Format("mark value is empty".to_string()))
        }
        (Some(_), false) => Err(AppApiError::Format("boolean mark has value".to_string())),
        _ => Ok(()),
    }
}

pub(crate) fn validate_format_replacement_payload(
    kind: &MarkKind,
    expected_value: &str,
    value: &str,
) -> Result<(), AppApiError> {
    if !matches!(
        kind,
        MarkKind::Color | MarkKind::Background | MarkKind::Font | MarkKind::Size
    ) {
        return Err(AppApiError::Format(
            "format replacement kind is not value-bearing".to_string(),
        ));
    }
    for (label, candidate) in [
        ("expected format value", expected_value),
        ("format value", value),
    ] {
        if candidate.trim().is_empty() || candidate.trim() != candidate {
            return Err(AppApiError::Format(format!(
                "{label} is empty or has surrounding whitespace"
            )));
        }
    }
    if expected_value == value {
        return Err(AppApiError::Format(
            "format replacement value equals expected value".to_string(),
        ));
    }
    Ok(())
}

pub(crate) fn parse_mark_kind(kind: &str) -> Result<MarkKind, AppApiError> {
    match kind {
        "bold" => Ok(MarkKind::Bold),
        "italic" => Ok(MarkKind::Italic),
        "underline" => Ok(MarkKind::Underline),
        "strike" => Ok(MarkKind::Strike),
        "code" => Ok(MarkKind::Code),
        "superscript" => Ok(MarkKind::Superscript),
        "subscript" => Ok(MarkKind::Subscript),
        "color" => Ok(MarkKind::Color),
        "background" => Ok(MarkKind::Background),
        "font" => Ok(MarkKind::Font),
        "size" => Ok(MarkKind::Size),
        "link" => Ok(MarkKind::Link),
        "citation" => Ok(MarkKind::Citation),
        _ => Err(AppApiError::Format(format!("unsupported mark kind {kind}"))),
    }
}
