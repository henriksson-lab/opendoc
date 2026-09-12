//! Section properties: page setup and header/footer references.

use crate::xml::XmlElement;
use opendoc_core::{HeaderFooterSlot, Length, PageNumberField, PageSetup};

/// One `w:headerReference` / `w:footerReference`: which slot, which variant,
/// which relationship.
pub(super) struct FurnitureReference<'a> {
    pub(super) slot: HeaderFooterSlot,
    /// `default`, `first` or `even`. OpenDoc models only `default`.
    pub(super) variant: &'a str,
    pub(super) rel_id: &'a str,
}

pub(super) fn furniture_references(sect_pr: &XmlElement) -> Vec<FurnitureReference<'_>> {
    sect_pr
        .elements()
        .filter_map(|element| {
            let slot = match element.local.as_str() {
                "headerReference" => HeaderFooterSlot::Header,
                "footerReference" => HeaderFooterSlot::Footer,
                _ => return None,
            };
            Some(FurnitureReference {
                slot,
                variant: element.attr("type").map(str::trim).unwrap_or("default"),
                rel_id: element.attr("id").map(str::trim).unwrap_or_default(),
            })
        })
        .collect()
}

/// A section-property length. Unlike [`twips_attr`] a missing attribute and
/// an unusable one are the same answer here, because the caller turns either
/// into "keep the default and say the geometry was not representable".
pub(super) fn section_length(element: &XmlElement, name: &str) -> Option<Length> {
    Length::from_twips(element.attr(name)?.trim().parse::<i32>().ok()?).ok()
}

/// Reads `w:pgSz` and `w:pgMar` into a [`PageSetup`].
///
/// The mapping is the identity: WordprocessingML measures a section in twips
/// and so does `Length`, so nothing is converted and nothing can drift. The
/// one asymmetry is direction — `w:left`/`w:right` are physical edges and
/// OpenDoc's margins are logical — and it is resolved the same way the writer
/// resolves it, by treating the leading edge as the left one.
///
/// `w:orient` is deliberately ignored: OpenDoc derives orientation from the
/// dimensions (ADR 0009), and WordprocessingML already writes `w:w`/`w:h`
/// swapped for a landscape section, so honouring both would rotate the page
/// twice.
///
/// Returns `None` when the values do not describe a page OpenDoc can hold, so
/// the caller can warn and keep the default rather than fail the import: a
/// page size is not the kind of corruption that should cost a user the
/// document.
pub(super) fn parse_page_setup(sect_pr: &XmlElement) -> Option<PageSetup> {
    let mut setup = PageSetup::default();
    if let Some(size) = sect_pr.child("pgSz") {
        match (size.attr("w"), size.attr("h")) {
            (None, None) => {}
            _ => {
                setup = setup
                    .with_size(section_length(size, "w")?, section_length(size, "h")?)
                    .ok()?
            }
        }
    }
    if let Some(margins) = sect_pr.child("pgMar") {
        let read = |name: &str, fallback: Length| match margins.attr(name) {
            None => Some(fallback),
            Some(_) => section_length(margins, name),
        };
        setup = setup
            .with_margins(
                read("top", setup.margin_top)?,
                read("bottom", setup.margin_bottom)?,
                read("left", setup.margin_start)?,
                read("right", setup.margin_end)?,
            )
            .ok()?;
        setup = setup
            .with_furniture_margins(
                read("header", setup.margin_header)?,
                read("footer", setup.margin_footer)?,
            )
            .ok()?;
    }
    Some(setup)
}

/// `true` when the section asks for something the single-section page model
/// cannot hold. Multiple text columns are the only one `w:sectPr` itself
/// states; extra *sections* are counted where their `w:sectPr` is found.
pub(super) fn section_has_unrepresentable_properties(sect_pr: &XmlElement) -> bool {
    sect_pr
        .child("cols")
        .and_then(|cols| cols.attr("num"))
        .and_then(|num| num.trim().parse::<u32>().ok())
        .is_some_and(|num| num > 1)
}

/// The page-number field a WordprocessingML field instruction names, if it is
/// one OpenDoc models. `PAGE` and `NUMPAGES` are fields on both sides, so the
/// field stays a field and never freezes into the cached result Word wrote
/// next to it.
pub(super) fn page_number_field(instruction: &str) -> Option<PageNumberField> {
    let mut words = instruction.split_whitespace();
    match words.next()?.to_ascii_uppercase().as_str() {
        "PAGE" => Some(PageNumberField::CurrentPage),
        "NUMPAGES" => Some(PageNumberField::PageCount),
        _ => None,
    }
}
