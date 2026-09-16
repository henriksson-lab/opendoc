//! Parsing the scalar values commands carry: colours, lengths, enums.

use super::*;

pub(crate) fn parse_color(value: &str) -> Result<opendoc_core::Color, AppApiError> {
    opendoc_core::Color::parse(value.trim()).map_err(|err| AppApiError::Format(err.to_string()))
}

/// One indent step: 0.5in in twips, the tab stop Word and Google Docs both
/// use for the toolbar indent buttons.
pub(crate) const INDENT_STEP_TWIPS: i32 = 720;

pub(crate) fn parse_alignment(name: &str) -> Result<opendoc_core::Alignment, AppApiError> {
    opendoc_core::Alignment::parse(name.trim()).map_err(|err| AppApiError::Format(err.to_string()))
}

pub(crate) fn parse_direction(name: &str) -> Result<opendoc_core::TextDirection, AppApiError> {
    opendoc_core::TextDirection::parse(name.trim())
        .map_err(|err| AppApiError::Format(err.to_string()))
}

pub(crate) fn parse_header_footer_slot(
    name: &str,
) -> Result<opendoc_core::HeaderFooterSlot, AppApiError> {
    opendoc_core::HeaderFooterSlot::parse(name.trim())
        .map_err(|err| AppApiError::Format(err.to_string()))
}

/// `"none"` is the absence of a field, spelled rather than encoded as an
/// empty string, so a caller cannot ask for a field by forgetting one.
pub(crate) fn parse_optional_page_number_field(
    name: &str,
) -> Result<Option<opendoc_core::PageNumberField>, AppApiError> {
    match name.trim() {
        "none" => Ok(None),
        other => opendoc_core::PageNumberField::parse(other)
            .map(Some)
            .map_err(|err| AppApiError::Format(err.to_string())),
    }
}

/// Reads an insert anchor off the command surface.
///
/// `None` or an empty string appends, the literal
/// [`opendoc_core::InsertPosition::FIRST_KEYWORD`] goes before every sibling,
/// and anything else is the id of the sibling to follow. The keyword exists
/// because `None` is already spoken for: it means *append*, and that meaning
/// is what makes a replayed insert whose anchor was deleted degrade rather
/// than vanish.
pub(crate) fn parse_insert_position(
    anchor: Option<&str>,
) -> Result<opendoc_core::InsertPosition, AppApiError> {
    opendoc_core::InsertPosition::parse(anchor).map_err(|err| AppApiError::Format(err.to_string()))
}

pub(crate) fn parse_length(twips: i32) -> Result<opendoc_core::Length, AppApiError> {
    opendoc_core::Length::from_twips(twips).map_err(|err| AppApiError::Format(err.to_string()))
}

pub(crate) fn parse_block_property_key(name: &str) -> Result<BlockPropertyKey, AppApiError> {
    BlockPropertyKey::parse(name.trim()).map_err(|err| AppApiError::Format(err.to_string()))
}

/// `"multiple"` counts thousandths of a line, the other two rules count twips.
/// The unit travels with the rule so a caller cannot send a line count where a
/// height was meant — and the pairing itself is
/// [`opendoc_core::LineSpacing::parse`]'s, not a second reading of it here.
pub(crate) fn parse_line_spacing(
    mode: &str,
    value: i32,
) -> Result<opendoc_core::LineSpacing, AppApiError> {
    opendoc_core::LineSpacing::parse(mode, value)
        .map_err(|err| AppApiError::Format(err.to_string()))
}
