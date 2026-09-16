//! The converter: WordprocessingML body into the canonical document model.

use crate::docx::package::{media_name, DocxParts, Relationship};
use crate::docx::props::{
    overlay_para_props, parse_para_props, parse_run_props, toggle_value, RunProps,
};
use crate::docx::revisions::{
    source_author, CommentRange, DeleteCapture, FieldState, InsertCapture, ParagraphState,
    Revision, Segment,
};
use crate::docx::section::{
    furniture_references, page_number_field, parse_page_setup,
    section_has_unrepresentable_properties, section_starts_new_page,
};
use crate::docx::styles::{parse_num_pr, HeadingStyle};
use crate::docx::table::plan_table;
use crate::docx::util::parse_iso_datetime_ms;
use crate::docx::warnings::{
    DroppedCounter, COMMENT_ANCHOR_DEGRADED, DROPPED_ALT_CHUNK, DROPPED_BOOKMARK_DUPLICATE,
    DROPPED_BOOKMARK_NAME, DROPPED_BOOKMARK_RANGE, DROPPED_DRAWING, DROPPED_FORMAT_CHANGE,
    DROPPED_FURNITURE_CONTENT, DROPPED_HEADER_FOOTER, DROPPED_IMAGE_BORDER, DROPPED_IMAGE_CROP,
    DROPPED_IMAGE_OPACITY, DROPPED_NESTED_IMAGE, DROPPED_NESTED_REVISION, DROPPED_PARAGRAPH_CHANGE,
    DROPPED_POSITIONED_IMAGE, DROPPED_RUN_PROPERTY, DROPPED_SECTION_PROPERTIES, DROPPED_TEXT_BOX,
    EMPTY_COMMENT, EMPTY_FOOTNOTE, INVALID_PAGE_SETUP, MISSING_FOOTNOTE, MISSING_IMAGE_BLOB,
    NESTED_TABLE, SPLIT_INLINE_IMAGE, SPLIT_PAGE_BREAK, TABLE_MERGE_REPAIRED, TABLE_NESTING_LIMIT,
    UNKNOWN_LIST_DEFINITION,
};
use crate::docx::DocxImport;
use crate::xml::XmlElement;
use crate::ImportError;
use opendoc_core::{
    validate_table_geometry, Anchor, BibliographyReference, Block, BlockKind, BlockProperties,
    Bookmark, BorderStyle, CellBorder, CellSpan, CitationGroup, CitationItem, CitationPlacement,
    CitationSource, CitationSourceFormat, CitationSummary, Color, Comment, CommentThread, Document,
    Equation, EquationSourceFormat, Footnote, HeaderFooterSlot, ImageCrop, ImageLayout,
    ImagePlacement, Inline, Length, ListKind, ListProperties, Mark, ModelWarning,
    OrderedListFormat, PositionedImage, PositionedImageAnchor, PositionedImageLayer, StableId,
    Suggestion, SuggestionKind, SuggestionState, TableCell, TableColumn, TableRow, TextRange,
};
use std::collections::{BTreeMap, BTreeSet};

/// How deeply tables may nest before the structure is flattened.
///
/// `crate::xml::MAX_XML_DEPTH` already bounds the recursion here — a deeper
/// tree does not exist — so this is a *model* limit, not a safety one: a table
/// nested sixteen deep is not a table anyone reads, and every level costs a
/// `TableCell` walk in layout, render, merge and export.
const MAX_TABLE_DEPTH: usize = 16;

/// A DOCX bookmark that has already been proven to be a whole-paragraph
/// range.  OpenDoc deliberately does not have character-position bookmarks;
/// this is the one Word shape whose target has an unambiguous stable-block
/// projection (and is the shape our DOCX writer produces).
#[derive(Clone, Debug)]
struct ParagraphBookmark {
    name: String,
}

pub(super) struct Converter<'a> {
    parts: &'a DocxParts,
    warnings: Vec<ModelWarning>,
    dropped: DroppedCounter,
    list_ids: BTreeMap<String, StableId>,
    /// Concrete levels that actually occurred in the body for each Word
    /// numbering instance. Abstract definitions commonly declare all nine
    /// levels; importing those unused defaults would create source state the
    /// document never expressed.
    list_levels: BTreeMap<String, BTreeSet<u8>>,
    note_ids: BTreeMap<(bool, String), StableId>,
    footnotes: Vec<Footnote>,
    endnote_ids: BTreeSet<StableId>,
    comment_ranges: BTreeMap<String, CommentRange>,
    open_comment_ranges: Vec<String>,
    suggestions: Vec<Suggestion>,
    block_ids: BTreeSet<StableId>,
    bookmarks: Vec<Bookmark>,
    bookmark_names: BTreeSet<String>,
    /// Relationship identifiers are local to a Word part.  While walking a
    /// header or footer this names its owning main-document relationship so
    /// drawings and hyperlinks resolve in that part rather than in the body.
    active_furniture: Option<String>,
}

/// Read Word's `wp:extent` in EMUs into the canonical twip geometry. A public
/// Google Doc is downloaded as DOCX, so preserving this size here also keeps
/// its imported images from unexpectedly expanding to the column width.
///
/// DOCX permits non-integral twips; round to the nearest representable twip.
/// A malformed or impractically small/large extent is treated as unspecified,
/// just as an absent extent is, rather than making the entire document fail
/// validation because of one drawing.
fn docx_image_layout(element: &XmlElement) -> (ImageLayout, bool, bool, bool) {
    const EMUS_PER_TWIP: i64 = 635;
    let dimension = |name: &str| {
        let extent = element.find_descendant("extent")?;
        let emus = extent.attr(name)?.trim().parse::<i64>().ok()?;
        let twips = emus.checked_add(EMUS_PER_TWIP / 2)? / EMUS_PER_TWIP;
        let twips = i32::try_from(twips).ok()?;
        let length = Length::from_twips(twips).ok()?;
        (ImageLayout::MIN_TWIPS..=ImageLayout::MAX_TWIPS)
            .contains(&length.twips())
            .then_some(length)
    };
    let mut layout = ImageLayout {
        width: dimension("cx"),
        height: dimension("cy"),
        ..ImageLayout::default()
    };
    // DrawingML stores clockwise rotation in 1/60,000ths of a degree. A
    // complete turn is visual identity, so keep the canonical source absent
    // rather than writing a needless 360 into a document that stated none.
    layout.rotation_degrees = element
        .find_descendant("xfrm")
        .and_then(|transform| transform.attr("rot"))
        .and_then(|raw| raw.trim().parse::<i32>().ok())
        .and_then(drawingml_rotation_degrees);
    // DrawingML `srcRect` uses thousandths of a percent. The model's whole
    // percentages are intentional: they are what the desktop UI can edit
    // precisely, so round imported fractions to its nearest honest value.
    let (crop, malformed_crop) = match element.find_descendant("srcRect") {
        None => (None, false),
        Some(source_rect) => {
            let edge = |name: &str| {
                source_rect
                    .attr(name)
                    .unwrap_or("0")
                    .trim()
                    .parse::<u32>()
                    .ok()
                    .and_then(|value| u8::try_from((value + 500) / 1_000).ok())
            };
            match (edge("l"), edge("t"), edge("r"), edge("b")) {
                (
                    Some(left_percent),
                    Some(top_percent),
                    Some(right_percent),
                    Some(bottom_percent),
                ) => {
                    let crop = ImageCrop {
                        left_percent,
                        top_percent,
                        right_percent,
                        bottom_percent,
                    };
                    if crop.is_empty() {
                        // An empty `srcRect` is the source's explicit identity
                        // crop. Canonicalising it to absence loses no appearance.
                        (None, false)
                    } else if crop.validate().is_ok() {
                        (Some(crop), false)
                    } else {
                        (None, true)
                    }
                }
                _ => (None, true),
            }
        }
    };
    layout.crop = crop;
    // `a:alphaModFix@amt` is opacity in thousandths of a percent. The model
    // intentionally exposes whole percentages, so retain the nearest model
    // value rather than dropping an explicit DrawingML transparency effect.
    // Values outside the schema's 0..=100000 range are malformed and remain
    // absent, just like an invalid extent or crop above.
    let (opacity, malformed_opacity) = match element.find_descendant("alphaModFix") {
        Some(alpha) => match alpha
            .attr("amt")
            .and_then(|raw| raw.trim().parse::<u32>().ok())
            .filter(|amount| *amount <= 100_000)
        {
            Some(amount) => (u8::try_from((amount + 500) / 1_000).ok(), false),
            None => (None, true),
        },
        None => (None, false),
    };
    layout.opacity_percent = opacity;
    // `a:ln` is the DrawingML picture outline.  Preserve precisely the
    // solid/dash/dot + sRGB subset our writer produces; gradient/pattern
    // fills and other dash presets have no truthful CellBorder equivalent.
    let (border, malformed_border) = docx_image_border(element);
    layout.border = border;
    // A square-wrapped anchor aligned to a column edge is exactly the
    // block-level wrap model OpenDoc has. Other anchor geometry remains
    // intentionally unmodelled rather than being mistaken for this subset.
    layout.placement = element.find_descendant("anchor").and_then(|anchor| {
        anchor.child("wrapSquare")?;
        let horizontal = anchor.child("positionH")?;
        (horizontal.attr("relativeFrom") == Some("column"))
            .then(|| horizontal.child("align"))
            .flatten()
            .and_then(|align| match align.text().trim() {
                "left" => Some(ImagePlacement::WrapStart),
                "right" => Some(ImagePlacement::WrapEnd),
                _ => None,
            })
    });
    layout.positioned = docx_page_content_position(element);
    (layout, malformed_opacity, malformed_crop, malformed_border)
}

/// Converts DrawingML's 1/60,000-degree rotation to the model's whole degree
/// with signed nearest-value rounding. Integer division truncates toward zero,
/// which used to turn a 0.999-degree source rotation into an absent effect.
/// A complete turn is visual identity in either sign and is deliberately
/// canonicalised to absence.
fn drawingml_rotation_degrees(units: i32) -> Option<i16> {
    const UNITS_PER_DEGREE: i64 = 60_000;
    let units = i64::from(units);
    let rounded = if units >= 0 {
        (units + UNITS_PER_DEGREE / 2) / UNITS_PER_DEGREE
    } else {
        (units - UNITS_PER_DEGREE / 2) / UNITS_PER_DEGREE
    };
    let degrees = i16::try_from(rounded).ok()?;
    (degrees != 0 && degrees.unsigned_abs() != 360 && (-360..=360).contains(&degrees))
        .then_some(degrees)
}

/// Read the exact DrawingML picture-outline subset the writer emits.  A
/// visible outline that falls outside it must be named: silently treating a
/// gradient, themed, or unsupported dash line as no border loses authored
/// presentation just as surely as malformed crop does.
fn docx_image_border(element: &XmlElement) -> (Option<CellBorder>, bool) {
    const EMUS_PER_TWIP: i64 = 635;
    let Some(line) = element
        .find_descendant("spPr")
        .and_then(|properties| properties.child("ln"))
    else {
        return (None, false);
    };
    // An explicit no-fill line is visually identical to an absent outline.
    if line.child("noFill").is_some() {
        return (None, false);
    }
    let Some(emus) = line
        .attr("w")
        .and_then(|value| value.trim().parse::<i64>().ok())
    else {
        return (None, true);
    };
    let rounded = if emus >= 0 {
        match emus.checked_add(EMUS_PER_TWIP / 2) {
            Some(value) => value,
            None => return (None, true),
        }
    } else {
        match emus.checked_sub(EMUS_PER_TWIP / 2) {
            Some(value) => value,
            None => return (None, true),
        }
    } / EMUS_PER_TWIP;
    let Some(width) = i32::try_from(rounded)
        .ok()
        .and_then(|twips| Length::from_twips(twips).ok())
    else {
        return (None, true);
    };
    let Some(color) = line
        .find_descendant("srgbClr")
        .and_then(|source| source.attr("val"))
        .map(str::trim)
        .filter(|value| value.len() == 6)
        .and_then(|value| Color::parse(&format!("#{value}")).ok())
    else {
        return (None, true);
    };
    let style = match line
        .child("prstDash")
        .and_then(|dash| dash.attr("val"))
        .unwrap_or("solid")
    {
        "solid" => BorderStyle::Solid,
        "dash" => BorderStyle::Dashed,
        "dot" => BorderStyle::Dotted,
        _ => return (None, true),
    };
    match CellBorder::new(style, width, color) {
        Ok(border) => (Some(border), false),
        Err(_) => (None, true),
    }
}

/// The useful, lossless WordprocessingML subset for our page-content image
/// tuple. `margin` is Word's page content rectangle (rather than its physical
/// page edge); unlike a paragraph-relative anchor, it has no implicit source
/// paragraph whose identity we would have to invent on import.
fn docx_page_content_position(element: &XmlElement) -> Option<PositionedImage> {
    const EMUS_PER_TWIP: i64 = 635;
    let anchor = element.find_descendant("anchor")?;
    anchor.child("wrapNone")?;
    // `behindDoc` only tells us on which side of text the picture paints.
    // A nonzero `relativeHeight` additionally orders it among other floating
    // objects.  OpenDoc's two-layer tuple deliberately has no such z-order,
    // so accepting it here would make a source ordering silently disappear.
    if !matches!(anchor.attr("relativeHeight"), None | Some("0")) {
        return None;
    }
    let offset = |axis: &str| {
        let position = anchor.child(axis)?;
        if position.attr("relativeFrom") != Some("margin") {
            return None;
        }
        let emus = position
            .child("posOffset")?
            .text()
            .trim()
            .parse::<i64>()
            .ok()?;
        let rounded = if emus >= 0 {
            emus.checked_add(EMUS_PER_TWIP / 2)?
        } else {
            emus.checked_sub(EMUS_PER_TWIP / 2)?
        } / EMUS_PER_TWIP;
        Length::from_twips(i32::try_from(rounded).ok()?).ok()
    };
    let horizontal_offset = offset("positionH")?;
    let vertical_offset = offset("positionV")?;
    let layer = match anchor.attr("behindDoc").unwrap_or("0") {
        "1" | "true" | "on" => PositionedImageLayer::BehindText,
        "0" | "false" | "off" => PositionedImageLayer::InFrontOfText,
        _ => return None,
    };
    Some(PositionedImage {
        anchor: PositionedImageAnchor::PageContent,
        horizontal_offset,
        vertical_offset,
        layer,
    })
}

/// The wrapped, column-edge subset below maps to the in-flow placement model.
/// Every other `wp:anchor` carries object positioning that ADR 0022 says must
/// not be guessed from a partial tuple.
fn has_unmapped_positioned_image(element: &XmlElement) -> bool {
    let Some(anchor) = element.find_descendant("anchor") else {
        return false;
    };
    if docx_page_content_position(element).is_some() {
        return false;
    }
    let Some(horizontal) = anchor.child("positionH") else {
        return true;
    };
    // The in-flow model captures only the writer's column-edge square-wrap
    // subset.  `wrapText=left|right`, a behind-text layer, or an explicit
    // z-order changes what Word paints; projecting any of those merely as
    // `WrapStart`/`WrapEnd` would claim a faithful positioned-object mapping
    // where none exists. Keep the useful horizontal side as an approximation,
    // but make the source loss visible through the existing warning.
    let supported_alignment = horizontal.attr("relativeFrom") == Some("column")
        && horizontal
            .child("align")
            .is_some_and(|align| matches!(align.text().trim(), "left" | "right"))
        && anchor
            .child("wrapSquare")
            .is_some_and(|wrap| matches!(wrap.attr("wrapText"), None | Some("bothSides")))
        && matches!(anchor.attr("behindDoc"), None | Some("0" | "false" | "off"))
        && matches!(anchor.attr("relativeHeight"), None | Some("0"));
    !supported_alignment
}

pub(super) fn convert_parts(title: &str, parts: &DocxParts) -> Result<DocxImport, ImportError> {
    let mut converter = Converter {
        parts,
        warnings: Vec::new(),
        dropped: DroppedCounter::default(),
        list_ids: BTreeMap::new(),
        list_levels: BTreeMap::new(),
        note_ids: BTreeMap::new(),
        footnotes: Vec::new(),
        endnote_ids: BTreeSet::new(),
        comment_ranges: BTreeMap::new(),
        open_comment_ranges: Vec::new(),
        suggestions: Vec::new(),
        block_ids: BTreeSet::new(),
        bookmarks: Vec::new(),
        bookmark_names: BTreeSet::new(),
        active_furniture: None,
    };
    if let Some(footnotes) = &parts.footnotes {
        converter.import_notes(footnotes, false);
    }
    if let Some(endnotes) = &parts.endnotes {
        converter.import_notes(endnotes, true);
    }

    let mut document = Document::new(title);
    let body = parts.document.child("body").unwrap_or(&parts.document);
    converter.walk_blocks(body, &mut document.blocks, 0);
    if document.blocks.is_empty() {
        return Err(ImportError::EmptyInput);
    }
    converter.install_list_starts(&mut document);
    // Section properties are read after the body, not during the walk: the
    // body-level `w:sectPr` is the *document's* page, while a `w:sectPr`
    // inside a paragraph's `w:pPr` marks an extra section OpenDoc has no
    // model for, and only the position tells them apart.
    converter.import_section(body.child("sectPr"), &mut document);
    document.comments = converter.import_comments();
    document.footnotes = std::mem::take(&mut converter.footnotes);
    document.endnote_ids = std::mem::take(&mut converter.endnote_ids);
    document.suggestions = std::mem::take(&mut converter.suggestions);
    document.bookmarks = std::mem::take(&mut converter.bookmarks);
    let mut warnings = std::mem::take(&mut converter.warnings);
    import_paperpile_docx_citations(&mut document, &mut warnings);
    warnings.extend(converter.dropped.into_warnings());
    document.warnings = warnings.clone();
    document
        .validate()
        .map_err(|err| ImportError::InvalidDocument(err.to_string()))?;

    let mut seen_hashes = BTreeSet::new();
    let blobs = parts
        .media
        .values()
        .chain(
            parts
                .furniture_media
                .values()
                .flat_map(|media| media.values()),
        )
        .filter(|blob| seen_hashes.insert(blob.hash.clone()))
        .cloned()
        .collect();
    Ok(DocxImport {
        document,
        warnings,
        blobs,
    })
}

/// Reifies the citation identity Paperpile writes into Google Docs' DOCX
/// export. Its body citations are hyperlinks of the form
/// `paperpile.com/c/<document-key>/<item-key+…>`; its bibliography entries
/// use `/b/` instead. We deliberately only look at `/c/` links, because a
/// Paperpile bibliography can be stale or include unused references.
///
/// The link names the cited Paperpile item keys and retains the rendered
/// label, but it does not contain bibliographic metadata. The public Paperpile
/// endpoint requires the add-on's authenticated document access, so each
/// reference is stored as an opaque source instead of inventing metadata from
/// the reference list or the network.
fn import_paperpile_docx_citations(document: &mut Document, warnings: &mut Vec<ModelWarning>) {
    // Paperpile item keys are scoped to the Paperpile document named by the
    // citation hyperlink.  Keeping that scope means that a combined import
    // cannot accidentally conflate two otherwise equal keys, and retaining
    // the original occurrence URL gives a future authenticated adapter the
    // exact direct source it would need.  In particular, do not consult the
    // `/b/` links in the rendered reference list for either identity or
    // metadata: that list is not the citation occurrence source.
    let mut references = BTreeMap::<(String, String), PaperpileReference>::new();
    let mut groups = Vec::new();
    for block in &mut document.blocks {
        replace_paperpile_links_in_block(block, &mut references, &mut groups);
    }
    for footnote in &mut document.footnotes {
        replace_paperpile_links_inlines(&mut footnote.body, &mut references, &mut groups);
    }
    if groups.is_empty() {
        return;
    }
    document.citation_database.references = references
        .into_iter()
        .map(|((document_key, key), reference)| BibliographyReference {
            id: reference.id,
            revision: 0,
            source: CitationSource {
                format: CitationSourceFormat::Unknown("paperpile-docx-link".to_string()),
                // This is intentionally the direct citation link, rather
                // than a synthesized record or a URL from the bibliography.
                // It contains the document and the ordered group of item keys
                // that appeared at the occurrence.
                bytes: reference.source_href.into_bytes(),
            },
            summary: CitationSummary {
                title: format!("Paperpile reference {document_key}/{key}"),
                authors: Vec::new(),
                issued: None,
                doi: None,
                url: None,
            },
            deleted: false,
        })
        .collect();
    document.citation_database.citations = groups;
    warnings.push(ModelWarning {
        code: "paperpile-docx-citations".to_string(),
        message: "imported Paperpile body citation links, scoped item keys, and rendered labels; bibliography entries were not used because Paperpile metadata is not present in the public export".to_string(),
    });
}

fn replace_paperpile_links_in_block(
    block: &mut Block,
    references: &mut BTreeMap<(String, String), PaperpileReference>,
    groups: &mut Vec<CitationGroup>,
) {
    replace_paperpile_links_inlines(&mut block.content, references, groups);
    if let BlockKind::Table { rows, .. } = &mut block.kind {
        for row in rows {
            for cell in &mut row.cells {
                for nested in &mut cell.blocks {
                    replace_paperpile_links_in_block(nested, references, groups);
                }
            }
        }
    }
}

fn replace_paperpile_links_inlines(
    inlines: &mut [Inline],
    references: &mut BTreeMap<(String, String), PaperpileReference>,
    groups: &mut Vec<CitationGroup>,
) {
    for inline in inlines {
        let Inline::Link { text, href, .. } = inline else {
            continue;
        };
        let Some(citation_link) = paperpile_citation_link(href) else {
            continue;
        };
        if text.trim().is_empty() {
            continue;
        }
        let items = citation_link
            .keys
            .into_iter()
            .map(|key| CitationItem {
                reference_id: references
                    .entry((citation_link.document_key.clone(), key))
                    .or_insert_with(|| PaperpileReference {
                        id: StableId::new("paperpile-reference"),
                        source_href: citation_link.href.clone(),
                    })
                    .id
                    .clone(),
                locator: None,
                label: None,
                prefix: None,
                suffix: None,
                suppress_author: false,
            })
            .collect();
        let citation_id = StableId::new("paperpile-citation");
        groups.push(CitationGroup {
            id: citation_id.clone(),
            revision: 0,
            items,
            placement: CitationPlacement::Inline,
            rendered_cache: Some(text.clone()),
            deleted: false,
        });
        *inline = Inline::Citation {
            id: StableId::new("citation-label"),
            citation_id,
            rendered_cache: Some(text.clone()),
        };
    }
}

#[derive(Clone, Debug)]
struct PaperpileReference {
    id: StableId,
    source_href: String,
}

#[derive(Clone, Debug)]
struct PaperpileCitationLink {
    document_key: String,
    keys: Vec<String>,
    href: String,
}

fn paperpile_citation_link(href: &str) -> Option<PaperpileCitationLink> {
    let (_, remainder) = href.trim().split_once("://")?;
    let (host, path) = remainder.split_once('/')?;
    if !host.eq_ignore_ascii_case("paperpile.com") {
        return None;
    }
    let mut parts = path.split('/');
    if parts.next()? != "c" {
        return None;
    }
    let document_key = parts.next()?;
    let keys = parts.next()?;
    if parts.next().is_some() || !paperpile_key_is_valid(document_key) || keys.is_empty() {
        return None;
    }
    let keys = keys.split('+').map(str::to_string).collect::<Vec<_>>();
    keys.iter()
        .all(|key| paperpile_key_is_valid(key))
        .then_some(PaperpileCitationLink {
            document_key: document_key.to_string(),
            keys,
            href: href.to_string(),
        })
}

fn paperpile_key_is_valid(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

// ---------------------------------------------------------------------------
// Section properties (w:sectPr)
// ---------------------------------------------------------------------------

pub(super) fn inline_id(inline: &Inline) -> &StableId {
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

pub(super) fn inlines_have_source(inlines: &[Inline]) -> bool {
    inlines.iter().any(|inline| match inline {
        Inline::Text { text, .. } | Inline::Link { text, .. } => !text.trim().is_empty(),
        _ => true,
    })
}

pub(super) fn math_source(element: &XmlElement) -> Option<String> {
    let mut out = String::new();
    for descendant in element.descendants() {
        if descendant.is("t") && descendant.prefix.as_deref() != Some("w") {
            out.push_str(&descendant.text());
        }
    }
    let trimmed = out.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

pub(super) fn equation_inline(source: String) -> Inline {
    Inline::Equation {
        id: StableId::new("equation"),
        equation: Equation {
            id: StableId::new("eq"),
            source_format: EquationSourceFormat::LatexLike,
            source,
        },
    }
}

pub(super) fn hyperlink_field_target(instruction: &str) -> Option<String> {
    let trimmed = instruction.trim();
    let rest = trimmed.strip_prefix("HYPERLINK")?;
    let quoted: Vec<&str> = rest
        .split('"')
        .enumerate()
        .filter(|(index, _)| index % 2 == 1)
        .map(|(_, value)| value)
        .collect();
    let mut is_local = false;
    let mut target: Option<&str> = None;
    let mut expect_local_value = false;
    for token in rest.split_whitespace() {
        if expect_local_value {
            expect_local_value = false;
            if let Some(value) = token.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
                target = Some(value);
                is_local = true;
            }
        } else if token == "\\l" {
            expect_local_value = true;
        }
    }
    let target = target
        .or_else(|| quoted.first().copied())
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    Some(if is_local {
        format!("#{target}")
    } else {
        target.to_string()
    })
}

impl<'a> Converter<'a> {
    /// Numbering starts belong to Word's numbering definition, while the
    /// OpenDoc block walk has already assigned each concrete `numId` a stable
    /// list id.  Transfer only positive, non-default starts; default starts
    /// stay absent so imported source is canonical.
    fn install_list_starts(&self, document: &mut Document) {
        for (num_id, list_id) in &self.list_ids {
            let mut properties = ListProperties::default();
            for level in 0..=8 {
                if self
                    .parts
                    .numbering
                    .is_ordered(&self.parts.styles, num_id, level)
                    != Some(true)
                {
                    continue;
                }
                if let Some(start) = self
                    .parts
                    .numbering
                    .start(num_id, level)
                    .filter(|start| *start != 1)
                {
                    properties.ordered_starts.insert(level, start);
                }
                // Word defines starts for all levels of one numbering
                // instance, including levels that have no current paragraph;
                // those non-default starts are source state and must survive.
                // Counter *formats* are different: a writer-generated
                // abstract definition has defaults for unused levels, and
                // importing them would materialise meaningless overrides.
                let format = self
                    .list_levels
                    .get(num_id)
                    .is_some_and(|levels| levels.contains(&level))
                    .then(|| {
                        self.parts
                            .numbering
                            .format(&self.parts.styles, num_id, level)
                    })
                    .flatten()
                    .and_then(|format| match format.as_str() {
                        "decimal" => Some(OrderedListFormat::Decimal),
                        "lowerLetter" | "lowerAlpha" => Some(OrderedListFormat::LowerAlpha),
                        "upperLetter" | "upperAlpha" => Some(OrderedListFormat::UpperAlpha),
                        "lowerRoman" => Some(OrderedListFormat::LowerRoman),
                        "upperRoman" => Some(OrderedListFormat::UpperRoman),
                        _ => None,
                    });
                if let Some(format) =
                    format.filter(|format| *format != OrderedListFormat::inherited_at(level))
                {
                    properties.ordered_formats.insert(level, format);
                }
                if self
                    .parts
                    .numbering
                    .is_ordered(&self.parts.styles, num_id, level)
                    == Some(false)
                {
                    let marker = self
                        .parts
                        .numbering
                        .level_text(num_id, level)
                        .and_then(|text| match text {
                            "\u{2022}" | "\u{25cf}" => Some(opendoc_core::BulletListMarker::Disc),
                            "\u{25e6}" | "\u{25cb}" => Some(opendoc_core::BulletListMarker::Circle),
                            "\u{25a0}" => Some(opendoc_core::BulletListMarker::Square),
                            glyph => opendoc_core::BulletListMarker::parse(glyph),
                        });
                    if let Some(marker) = marker.filter(|marker| {
                        *marker != opendoc_core::BulletListMarker::inherited_at(level)
                    }) {
                        properties.bullet_markers.insert(level, marker);
                    }
                }
            }
            if !properties.is_empty() {
                document.list_properties.insert(list_id.clone(), properties);
            }
        }
    }

    // -- warnings ---------------------------------------------------------

    fn count(&mut self, code: &'static str) {
        self.dropped.count(code);
    }

    // -- notes ------------------------------------------------------------

    fn import_notes(&mut self, part: &XmlElement, endnote: bool) {
        let element_name = if endnote { "endnote" } else { "footnote" };
        for note in part.children_named(element_name) {
            let Some(id) = note.attr("id").map(str::trim) else {
                continue;
            };
            if note.attr("type").is_some_and(|kind| {
                matches!(
                    kind.trim(),
                    "separator" | "continuationSeparator" | "continuationNotice"
                )
            }) {
                continue;
            }
            let body = self.nested_inline_body(note);
            if !inlines_have_source(&body) {
                self.count(EMPTY_FOOTNOTE);
                continue;
            }
            let stable_id = StableId::new(element_name);
            self.note_ids
                .insert((endnote, id.to_string()), stable_id.clone());
            self.footnotes.push(Footnote {
                id: stable_id.clone(),
                revision: 1,
                body,
                deleted: false,
            });
            if endnote {
                self.endnote_ids.insert(stable_id);
            }
        }
    }

    /// Converts every paragraph below `container` into one inline sequence,
    /// separating paragraphs with newline text (footnote and comment bodies).
    fn nested_inline_body(&mut self, container: &XmlElement) -> Vec<Inline> {
        let mut body = Vec::new();
        for paragraph in container
            .descendants()
            .into_iter()
            .filter(|element| element.is("p"))
        {
            let mut state = ParagraphState::new(
                BlockKind::Paragraph,
                BlockProperties::default(),
                RunProps::default(),
                true,
            );
            self.walk_paragraph_content(paragraph, &mut state);
            let mut inlines = Vec::new();
            for segment in state.segments {
                match segment {
                    Segment::Inline(inline) | Segment::MathPara(inline) => inlines.push(inline),
                    Segment::PageBreak => inlines.push(Inline::text("\n")),
                    Segment::Image { .. } => {}
                }
            }
            if inlines.is_empty() {
                continue;
            }
            if !body.is_empty() {
                body.push(Inline::text("\n"));
            }
            body.extend(inlines);
        }
        body
    }

    // -- comments ---------------------------------------------------------

    fn import_comments(&mut self) -> Vec<CommentThread> {
        let Some(part) = &self.parts.comments else {
            return Vec::new();
        };
        let parent_of: BTreeMap<String, String> = self
            .parts
            .comments_extended
            .as_ref()
            .map(|extended| {
                extended
                    .children_named("commentEx")
                    .filter_map(|entry| {
                        Some((
                            entry.attr("paraId")?.trim().to_string(),
                            entry.attr("paraIdParent")?.trim().to_string(),
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default();

        struct Record {
            docx_id: String,
            para_id: Option<String>,
            comment: Comment,
        }
        let mut records = Vec::new();
        for element in part.children_named("comment") {
            let Some(docx_id) = element.attr("id").map(str::trim) else {
                continue;
            };
            let body = self.nested_inline_body(element);
            if !inlines_have_source(&body) {
                self.count(EMPTY_COMMENT);
                continue;
            }
            let para_id = element
                .find_descendant("p")
                .and_then(|paragraph| paragraph.attr("paraId"))
                .map(|value| value.trim().to_string());
            records.push(Record {
                docx_id: docx_id.to_string(),
                para_id,
                comment: Comment {
                    id: StableId::new("comment"),
                    author: source_author(element.attr("author")),
                    body,
                    created_at_ms: element
                        .attr("date")
                        .and_then(parse_iso_datetime_ms)
                        .unwrap_or(0),
                    deleted: false,
                },
            });
        }

        // Group replies (commentsExtended parent links) under their root comment.
        let mut thread_of_para: BTreeMap<String, usize> = BTreeMap::new();
        let mut threads: Vec<(String, Vec<Comment>)> = Vec::new();
        for record in records {
            let parent_thread = record
                .para_id
                .as_ref()
                .and_then(|para_id| parent_of.get(para_id))
                .and_then(|parent| thread_of_para.get(parent))
                .copied();
            let index = match parent_thread {
                Some(index) => {
                    threads[index].1.push(record.comment);
                    index
                }
                None => {
                    threads.push((record.docx_id.clone(), vec![record.comment]));
                    threads.len() - 1
                }
            };
            if let Some(para_id) = record.para_id {
                thread_of_para.insert(para_id, index);
            }
        }

        threads
            .into_iter()
            .map(|(docx_id, comments)| CommentThread {
                id: StableId::new("comment-thread"),
                anchor: self.comment_anchor(&docx_id),
                comments,
                state: opendoc_core::CommentThreadState::Open,
                resolved_by: None,
                resolved_at_ms: None,
                action_assignee: None,
                action_due_at_ms: None,
                action_completed_by: None,
                action_completed_at_ms: None,
                reactions: Vec::new(),
                deleted: false,
            })
            .collect()
    }

    fn comment_anchor(&mut self, docx_id: &str) -> Anchor {
        let range = self
            .comment_ranges
            .get(docx_id)
            .cloned()
            .unwrap_or_default();
        if let (Some(start), Some(end)) = (range.start, range.end) {
            return Anchor::TextRange(TextRange { start, end });
        }
        self.count(COMMENT_ANCHOR_DEGRADED);
        match range.block_id.filter(|id| self.block_ids.contains(id)) {
            Some(block_id) => Anchor::NearestBlock {
                block_id,
                warning:
                    "DOCX comment range did not cover inline text; anchored to the nearest block"
                        .to_string(),
            },
            None => Anchor::Document,
        }
    }

    fn open_comment_range(&mut self, docx_id: &str) {
        self.comment_ranges.entry(docx_id.to_string()).or_default();
        if !self.open_comment_ranges.iter().any(|id| id == docx_id) {
            self.open_comment_ranges.push(docx_id.to_string());
        }
    }

    fn close_comment_range(&mut self, docx_id: &str) {
        self.open_comment_ranges.retain(|id| id != docx_id);
    }

    // -- block-level walk -------------------------------------------------

    // -- section properties ------------------------------------------------

    /// Reads the body-level `w:sectPr` into the document's page setup and its
    /// header and footer.
    ///
    /// A missing `w:sectPr` is not a warning: every dimension keeps the
    /// default the model already holds, which is what Word itself assumes.
    fn import_section(&mut self, sect_pr: Option<&XmlElement>, document: &mut Document) {
        let Some(sect_pr) = sect_pr else {
            return;
        };
        match parse_page_setup(sect_pr) {
            Some(setup) => document.page_setup = setup,
            None => self.count(INVALID_PAGE_SETUP),
        }
        if section_has_unrepresentable_properties(sect_pr) {
            self.count(DROPPED_SECTION_PROPERTIES);
        }
        for reference in furniture_references(sect_pr) {
            let slot = match (reference.slot, reference.variant) {
                (HeaderFooterSlot::Header, "default") => HeaderFooterSlot::Header,
                (HeaderFooterSlot::Footer, "default") => HeaderFooterSlot::Footer,
                (HeaderFooterSlot::Header, "first") => HeaderFooterSlot::FirstPageHeader,
                (HeaderFooterSlot::Footer, "first") => HeaderFooterSlot::FirstPageFooter,
                (HeaderFooterSlot::Header, "even") => HeaderFooterSlot::EvenPageHeader,
                (HeaderFooterSlot::Footer, "even") => HeaderFooterSlot::EvenPageFooter,
                _ => {
                    self.count(DROPPED_HEADER_FOOTER);
                    continue;
                }
            };
            let Some(part) = self.parts.furniture_parts.get(reference.rel_id) else {
                self.count(DROPPED_HEADER_FOOTER);
                continue;
            };
            let mut blocks = Vec::new();
            self.active_furniture = Some(reference.rel_id.to_string());
            self.walk_blocks(part, &mut blocks, 0);
            self.active_furniture = None;
            self.strip_unfurnishable(&mut blocks);
            if blocks.is_empty() {
                // An explicit empty first/even part is Word's native
                // suppression form. Ordinary empty furniture has no useful
                // distinction, but variants must retain `Some(empty)` rather
                // than accidentally inheriting the default header/footer.
                if matches!(
                    slot,
                    HeaderFooterSlot::FirstPageHeader
                        | HeaderFooterSlot::FirstPageFooter
                        | HeaderFooterSlot::EvenPageHeader
                        | HeaderFooterSlot::EvenPageFooter
                ) {
                    *document.furniture_mut(slot) = blocks;
                }
                continue;
            }
            *document.furniture_mut(slot) = blocks;
        }
    }

    /// Removes what page furniture may not hold. `Document::validate()` would
    /// refuse the whole import over a page break in a header, which would cost
    /// the user a document for a decoration; dropping it with a warning is the
    /// same trade the rest of this reader makes.
    fn strip_unfurnishable(&mut self, blocks: &mut Vec<Block>) {
        let before = blocks.len();
        blocks.retain(|block| !matches!(block.kind, BlockKind::PageBreak));
        self.dropped
            .count_n(DROPPED_FURNITURE_CONTENT, before - blocks.len());
        for block in blocks.iter_mut() {
            let before = block.content.len();
            block
                .content
                .retain(|inline| !matches!(inline, Inline::FootnoteRef { .. }));
            self.dropped
                .count_n(DROPPED_FURNITURE_CONTENT, before - block.content.len());
            if let BlockKind::Table { rows, .. } = &mut block.kind {
                for row in rows.iter_mut() {
                    for cell in row.cells.iter_mut() {
                        self.strip_unfurnishable(&mut cell.blocks);
                    }
                }
            }
        }
        // A paragraph whose only content was a footnote reference is now
        // empty, and an empty text block is not something this reader ever
        // produces; an image or a table legitimately has no inline content.
        blocks.retain(|block| {
            !block.content.is_empty()
                || !matches!(
                    block.kind,
                    BlockKind::Paragraph
                        | BlockKind::Title
                        | BlockKind::Subtitle
                        | BlockKind::Heading { .. }
                        | BlockKind::ListItem { .. }
                )
        });
    }

    fn walk_blocks(&mut self, container: &XmlElement, out: &mut Vec<Block>, table_depth: usize) {
        for element in container.elements() {
            match element.local.as_str() {
                "p" => self.convert_paragraph(element, out),
                "tbl" => {
                    if table_depth > 0 {
                        self.count(NESTED_TABLE);
                    }
                    if table_depth >= MAX_TABLE_DEPTH {
                        // Flattened rather than refused: the paragraphs inside
                        // the cells are the content, and the structure past
                        // this depth is not something the model — or a reader —
                        // can make sense of. `walk_blocks` on the table element
                        // itself descends through `tr`/`tc` on the catch-all
                        // arm and picks the paragraphs up.
                        self.count(TABLE_NESTING_LIMIT);
                        self.walk_blocks(element, out, table_depth);
                        continue;
                    }
                    let block = self.convert_table(element, table_depth);
                    self.block_ids.insert(block.id.clone());
                    out.push(block);
                }
                // Read by `import_section` once the body walk is done, so
                // counting it here would report the page as dropped.
                "sectPr" => {}
                "altChunk" => self.count(DROPPED_ALT_CHUNK),
                "commentRangeStart" => {
                    if let Some(id) = element.attr("id") {
                        self.open_comment_range(id.trim());
                    }
                }
                "commentRangeEnd" => {
                    if let Some(id) = element.attr("id") {
                        self.close_comment_range(id.trim());
                    }
                }
                "pPr" | "tblPr" | "tblGrid" | "trPr" | "tcPr" | "sdtPr" | "sdtEndPr"
                | "bookmarkStart" | "bookmarkEnd" | "proofErr" | "customXmlPr" => {}
                _ => self.walk_blocks(element, out, table_depth),
            }
        }
    }

    /// A `w:tbl` as a rectangular grid.
    ///
    /// The grid is *planned* before any cell content is converted, because a
    /// `w:gridSpan` makes one `w:tc` occupy several columns: the covered
    /// positions have to be materialised, or every later cell in the row lands
    /// one column too far left. See [`crate::docx::table`].
    fn convert_table(&mut self, table: &XmlElement, table_depth: usize) -> Block {
        // A table's borders and cell margins can come from the style it
        // names as well as from its own `w:tblPr`; Word's built-in table
        // styles are where it puts them. Resolved first so the table's own
        // `w:tblPr` lies over the top.
        let inherited = table
            .child("tblPr")
            .and_then(|tbl_pr| tbl_pr.child("tblStyle"))
            .and_then(|style| style.attr("val"))
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(|id| self.parts.styles.resolve_table(id))
            .unwrap_or_default();
        let plan = plan_table(table, &inherited);
        for code in &plan.dropped {
            self.count(code);
        }
        let column_count = plan.column_count();
        let mut rows = Vec::new();
        for (row_index, planned_row) in plan.rows.iter().enumerate() {
            let mut cells: Vec<TableCell> = Vec::new();
            for (cell_index, planned) in planned_row.iter().enumerate() {
                // The grid positions a `w:gridSpan` swallowed exist in the
                // model even though no `w:tc` describes them, so they are
                // filled before this cell is placed. Without them every cell
                // after a span in the same row lands one column too far left
                // — P1-4's misalignment, and the reason this is materialised
                // rather than counted.
                while cells.len() < planned.column {
                    cells.push(self.empty_cell());
                }
                let mut blocks = Vec::new();
                self.walk_blocks(planned.element, &mut blocks, table_depth + 1);
                if blocks.is_empty() {
                    let block = Block::paragraph("");
                    self.block_ids.insert(block.id.clone());
                    blocks.push(block);
                }
                let mut cell = TableCell::new(blocks);
                cell.properties = planned.properties.clone();
                // A covered cell keeps its identity and its content but never
                // its own span: the rectangle belongs to the cell that starts
                // it (ADR 0013).
                if !plan.is_covered(row_index, cell_index) {
                    cell.span = planned.span();
                }
                cells.push(cell);
            }
            // And the positions past the last `w:tc`: the columns a trailing
            // span swallowed, and the tail of a row shorter than the grid.
            while cells.len() < column_count {
                cells.push(self.empty_cell());
            }
            cells.truncate(column_count);
            rows.push(TableRow {
                id: StableId::new("row"),
                height: plan.row_heights.get(row_index).copied().flatten(),
                header: plan.row_headers.get(row_index).copied().unwrap_or(false),
                cells,
            });
        }
        if rows.is_empty() {
            rows.push(TableRow {
                id: StableId::new("row"),
                height: None,
                header: false,
                cells: (0..column_count).map(|_| self.empty_cell()).collect(),
            });
        }
        let mut columns: Vec<TableColumn> = (0..column_count)
            .map(|index| TableColumn {
                id: StableId::new("column"),
                width: plan.column_widths.get(index).copied().flatten(),
            })
            .collect();
        // Spans are a rectangle over a grid the file described; a file that
        // described an impossible one is read without merges rather than as a
        // document `validate` would refuse.
        if validate_table_geometry(&columns, &rows).is_err() {
            self.count(TABLE_MERGE_REPAIRED);
            for row in rows.iter_mut() {
                for cell in row.cells.iter_mut() {
                    cell.span = CellSpan::SINGLE;
                }
            }
            if validate_table_geometry(&columns, &rows).is_err() {
                columns = (0..column_count).map(|_| TableColumn::auto()).collect();
            }
        }
        Block {
            id: StableId::new("block"),
            kind: BlockKind::Table {
                columns,
                properties: opendoc_core::TableProperties {
                    border: plan.border,
                    alignment: plan.alignment,
                },
                rows,
            },
            content: Vec::new(),
            properties: BlockProperties::default(),
        }
    }

    fn empty_cell(&mut self) -> TableCell {
        let block = Block::paragraph("");
        self.block_ids.insert(block.id.clone());
        TableCell::new(vec![block])
    }

    // -- paragraphs -------------------------------------------------------

    fn convert_paragraph(&mut self, paragraph: &XmlElement, out: &mut Vec<Block>) {
        // The writer's native TOC is one standalone `fldSimple`, whose field
        // result is only a reader cache. Import the generating instruction,
        // not that cached title/result text. A TOC mixed with real paragraph
        // content or expressed through Word's multi-run field sequence has
        // no unambiguous block boundary here and remains ordinary content.
        if let Some(max_level) = docx_simple_toc_level(paragraph) {
            let block = Block {
                id: StableId::new("block"),
                kind: BlockKind::TableOfContents { max_level },
                content: Vec::new(),
                properties: BlockProperties::default(),
            };
            self.block_ids.insert(block.id.clone());
            out.push(block);
            return;
        }
        let bookmarks = self.whole_paragraph_bookmarks(paragraph);
        let ppr = paragraph.child("pPr");
        // An interior `w:sectPr` terminates the *preceding* section.  The
        // model has no section record yet, but a next-page boundary itself is
        // faithfully representable as a PageBreak.  Keep it after this
        // paragraph rather than treating it like `pageBreakBefore` on the
        // following paragraph; a following table or an empty final paragraph
        // must still begin in the new physical section.  Its setup/furniture
        // stay warned as unrepresentable below.
        let section_break_after = ppr
            .and_then(|properties| properties.child("sectPr"))
            .is_some_and(section_starts_new_page);
        // A Word caption is not an attribute of a drawing: it is a following
        // paragraph carrying the built-in `Caption` style.  We only recover
        // the narrow source shape this writer emits (one plain paragraph
        // immediately after one image), rather than guessing that any styled
        // paragraph elsewhere in a document belongs to an image.
        let is_simple_caption = ppr
            .and_then(|properties| properties.child_val("pStyle"))
            .is_some_and(|style| style.trim() == "Caption");
        let style = ppr
            .and_then(|ppr| ppr.child_val("pStyle"))
            .map(|id| self.parts.styles.resolve(id));
        let mut page_break_before = false;
        let mut properties = BlockProperties::default();
        if let Some(style) = style.as_ref() {
            overlay_para_props(&mut properties, &style.para_props);
            for code in style.para_dropped.clone() {
                self.count(code);
            }
        }
        if let Some(ppr) = ppr {
            let direct = parse_para_props(ppr);
            overlay_para_props(&mut properties, &direct.props);
            for code in direct.dropped {
                self.count(code);
            }
            for property in ppr.elements() {
                match property.local.as_str() {
                    "pPrChange" => self.count(DROPPED_PARAGRAPH_CHANGE),
                    "sectPr" => self.count(DROPPED_SECTION_PROPERTIES),
                    "pageBreakBefore" => page_break_before = toggle_value(property),
                    _ => {}
                }
            }
        }
        let direct_num_pr = ppr
            .and_then(|ppr| ppr.child("numPr"))
            .map(|num_pr| parse_num_pr(num_pr).filter(|num_pr| num_pr.num_id != "0"));
        let num_pr = match direct_num_pr {
            Some(direct) => direct,
            None => style.as_ref().and_then(|style| style.num_pr.clone()),
        };
        let kind = match style.as_ref().and_then(|style| style.heading) {
            Some(HeadingStyle::Title) => BlockKind::Title,
            Some(HeadingStyle::Subtitle) => BlockKind::Subtitle,
            Some(HeadingStyle::Level(level)) => BlockKind::Heading {
                level: level.clamp(1, 6),
            },
            None => match num_pr {
                Some(num_pr) => {
                    let ordered = match self.parts.numbering.is_ordered(
                        &self.parts.styles,
                        &num_pr.num_id,
                        num_pr.level,
                    ) {
                        Some(ordered) => ordered,
                        None => {
                            self.count(UNKNOWN_LIST_DEFINITION);
                            false
                        }
                    };
                    let list_id = self
                        .list_ids
                        .entry(num_pr.num_id.clone())
                        .or_insert_with(|| StableId::new("docx-list"))
                        .clone();
                    self.list_levels
                        .entry(num_pr.num_id.clone())
                        .or_default()
                        .insert(num_pr.level.min(8));
                    BlockKind::ListItem {
                        list_id,
                        level: num_pr.level.min(8),
                        kind: if ordered {
                            ListKind::Ordered
                        } else {
                            ListKind::Bullet
                        },
                    }
                }
                None => BlockKind::Paragraph,
            },
        };
        let style_props = style.map(|style| style.run_props).unwrap_or_default();

        if page_break_before {
            out.push(self.page_break_block());
        }
        let output_start = out.len();
        let mut state = ParagraphState::new(kind, properties, style_props, false);
        self.walk_paragraph_content(paragraph, &mut state);
        self.finish_paragraph(state, out);
        // A whole-paragraph bookmark has a stable owner only while this
        // paragraph remains a block. Do not hide that owner in image layout
        // metadata merely because the visible Caption shape is otherwise
        // simple and adjacent.
        if is_simple_caption && bookmarks.is_empty() {
            self.attach_simple_image_caption(out, output_start);
        }
        self.install_paragraph_bookmarks(bookmarks, &out[output_start..]);
        if section_break_after {
            out.push(self.page_break_block());
        }
    }

    /// Recover an adjacent DOCX Caption-style paragraph only where it has an
    /// unambiguous OpenDoc owner.  Rich captions, field-generated numbering,
    /// intervening blocks, and captions after non-images remain ordinary
    /// paragraphs: the model has no place to retain their independent Word
    /// semantics without inventing an association.
    fn attach_simple_image_caption(&mut self, out: &mut Vec<Block>, caption_start: usize) {
        if caption_start == 0 || out.len() != caption_start + 1 {
            return;
        }
        let caption = match &out[caption_start] {
            Block {
                kind: BlockKind::Paragraph,
                content,
                properties,
                ..
            } if properties == &BlockProperties::default() => content
                .iter()
                .map(|inline| match inline {
                    Inline::Text { text, marks, .. } if marks.is_empty() => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Option<Vec<_>>>()
                .map(|parts| parts.concat())
                .filter(|caption| !caption.trim().is_empty()),
            _ => None,
        };
        let Some(caption) = caption else {
            return;
        };
        let Some(Block {
            kind: BlockKind::Image { layout, .. },
            ..
        }) = out.get_mut(caption_start - 1)
        else {
            return;
        };
        // No overwrite: an image may already have an explicit extension
        // caption. Leaving the styled paragraph visible is less lossy.
        if layout.caption.is_some() {
            return;
        }
        layout.caption = Some(caption);
        out.remove(caption_start);
    }

    /// Collect only bookmarks that wrap the exact paragraph content.  The
    /// element is intentionally inspected at this level: a bookmark inside a
    /// run, hyperlink, tracked revision, or a range spanning paragraphs has a
    /// character/range meaning that the stable-block model must not guess.
    fn whole_paragraph_bookmarks(&mut self, paragraph: &XmlElement) -> Vec<ParagraphBookmark> {
        let mut starts = BTreeMap::<String, (String, usize)>::new();
        let mut ends = BTreeMap::<String, Vec<usize>>::new();
        let mut direct_starts = 0usize;
        let mut rejected = 0usize;
        let mut position = 0usize;

        for element in paragraph.elements() {
            match element.local.as_str() {
                "bookmarkStart" => {
                    direct_starts += 1;
                    let Some(id) = element
                        .attr("id")
                        .map(str::trim)
                        .filter(|id| !id.is_empty())
                    else {
                        rejected += 1;
                        continue;
                    };
                    let Some(name) = element
                        .attr("name")
                        .map(str::trim)
                        .filter(|name| !name.is_empty())
                    else {
                        rejected += 1;
                        continue;
                    };
                    if starts
                        .insert(id.to_string(), (name.to_string(), position))
                        .is_some()
                    {
                        rejected += 1;
                    }
                }
                "bookmarkEnd" => {
                    let Some(id) = element
                        .attr("id")
                        .map(str::trim)
                        .filter(|id| !id.is_empty())
                    else {
                        rejected += 1;
                        continue;
                    };
                    ends.entry(id.to_string()).or_default().push(position);
                }
                // These are positional/formatting annotations, not body
                // content.  They do not make an otherwise whole-paragraph
                // bookmark into a character-range bookmark.
                "pPr" | "proofErr" | "customXmlPr" | "sdtPr" | "sdtEndPr" | "commentRangeStart"
                | "commentRangeEnd" | "permStart" | "permEnd" => {}
                _ => position += 1,
            }
        }

        let nested_starts = paragraph
            .descendants()
            .into_iter()
            .filter(|element| element.is("bookmarkStart"))
            .count()
            .saturating_sub(direct_starts);
        rejected += nested_starts;

        let mut imported = Vec::new();
        for (id, (name, start)) in starts {
            let Some(end_positions) = ends.remove(&id) else {
                rejected += 1;
                continue;
            };
            // A whole paragraph begins at the first body child and ends just
            // after the last.  This includes the exact range emitted by our
            // DOCX writer.  Anything else has a character/range distinction
            // OpenDoc cannot retain.
            if start != 0 || end_positions.as_slice() != [position] {
                rejected += 1;
                continue;
            }
            imported.push(ParagraphBookmark { name });
        }
        rejected += ends.values().map(Vec::len).sum::<usize>();
        self.dropped.count_n(DROPPED_BOOKMARK_RANGE, rejected);
        imported
    }

    fn install_paragraph_bookmarks(&mut self, bookmarks: Vec<ParagraphBookmark>, blocks: &[Block]) {
        if bookmarks.is_empty() {
            return;
        }
        let Some(block) = (blocks.len() == 1).then(|| &blocks[0]).filter(|block| {
            !block.content.is_empty()
                && matches!(
                    block.kind,
                    BlockKind::Paragraph
                        | BlockKind::Title
                        | BlockKind::Subtitle
                        | BlockKind::Heading { .. }
                        | BlockKind::ListItem { .. }
                )
        }) else {
            self.dropped
                .count_n(DROPPED_BOOKMARK_RANGE, bookmarks.len());
            return;
        };
        for candidate in bookmarks {
            let bookmark = Bookmark {
                id: StableId::new("docx-bookmark"),
                name: candidate.name,
                block_id: block.id.clone(),
                revision: 1,
                deleted: false,
            };
            if bookmark.validate().is_err() {
                self.count(DROPPED_BOOKMARK_NAME);
            } else if !self.bookmark_names.insert(bookmark.name.clone()) {
                self.count(DROPPED_BOOKMARK_DUPLICATE);
            } else {
                self.bookmarks.push(bookmark);
            }
        }
    }

    fn page_break_block(&mut self) -> Block {
        let block = Block {
            id: StableId::new("block"),
            kind: BlockKind::PageBreak,
            content: Vec::new(),
            properties: BlockProperties::default(),
        };
        self.block_ids.insert(block.id.clone());
        block
    }

    fn finish_paragraph(&mut self, state: ParagraphState, out: &mut Vec<Block>) {
        let ParagraphState {
            kind,
            properties,
            segments,
            fragment_ids,
            ..
        } = state;
        if segments.len() == 1 {
            if let Some(Segment::MathPara(Inline::Equation { equation, .. })) = segments.first() {
                let block = Block {
                    id: fragment_ids[0].clone(),
                    kind: BlockKind::EquationBlock {
                        equation: equation.clone(),
                    },
                    content: Vec::new(),
                    properties,
                };
                self.block_ids.insert(block.id.clone());
                out.push(block);
                return;
            }
        }
        let is_text =
            |segment: &Segment| matches!(segment, Segment::Inline(_) | Segment::MathPara(_));
        let text_count = segments.iter().filter(|segment| is_text(segment)).count();
        let mut fragment = 0;
        let mut current: Vec<Inline> = Vec::new();
        for segment in segments {
            match segment {
                Segment::Inline(inline) | Segment::MathPara(inline) => current.push(inline),
                Segment::PageBreak => {
                    if text_count > 0 {
                        self.count(SPLIT_PAGE_BREAK);
                    }
                    self.flush_fragment(
                        &kind,
                        &properties,
                        &fragment_ids,
                        &mut fragment,
                        &mut current,
                        out,
                    );
                    let block = self.page_break_block();
                    out.push(block);
                }
                Segment::Image {
                    rel_id,
                    alt,
                    layout,
                } => {
                    if text_count > 0 {
                        self.count(SPLIT_INLINE_IMAGE);
                    }
                    self.flush_fragment(
                        &kind,
                        &properties,
                        &fragment_ids,
                        &mut fragment,
                        &mut current,
                        out,
                    );
                    let block = self.image_block(&rel_id, alt, layout);
                    out.push(block);
                }
            }
        }
        self.flush_fragment(
            &kind,
            &properties,
            &fragment_ids,
            &mut fragment,
            &mut current,
            out,
        );
    }

    fn flush_fragment(
        &mut self,
        kind: &BlockKind,
        properties: &BlockProperties,
        fragment_ids: &[StableId],
        fragment: &mut usize,
        current: &mut Vec<Inline>,
        out: &mut Vec<Block>,
    ) {
        let id = fragment_ids
            .get(*fragment)
            .cloned()
            .unwrap_or_else(|| StableId::new("block"));
        *fragment += 1;
        if current.is_empty() {
            return;
        }
        let block = Block {
            id,
            kind: kind.clone(),
            content: std::mem::take(current),
            properties: properties.clone(),
        };
        self.block_ids.insert(block.id.clone());
        out.push(block);
    }

    /// Resolve a relationship in the part currently being walked.  Word only
    /// guarantees IDs are unique inside one `.rels` part; falling back from a
    /// header/footer ID to the main document would silently bind the wrong
    /// target when both happen to use (for example) `rId1`.
    fn current_relationship(&self, rel_id: &str) -> Option<&Relationship> {
        match &self.active_furniture {
            Some(furniture_id) => self
                .parts
                .furniture_relationships
                .get(furniture_id)
                .and_then(|relationships| relationships.get(rel_id)),
            None => self.parts.relationships.get(rel_id),
        }
    }

    fn current_media(&self, rel_id: &str) -> Option<&crate::ImportedBlob> {
        match &self.active_furniture {
            Some(furniture_id) => self
                .parts
                .furniture_media
                .get(furniture_id)
                .and_then(|media| media.get(rel_id)),
            None => self.parts.media.get(rel_id),
        }
    }

    fn image_block(&mut self, rel_id: &str, alt: Option<String>, layout: ImageLayout) -> Block {
        let block = match self.current_media(rel_id) {
            Some(blob) => Block {
                id: StableId::new("block"),
                kind: BlockKind::Image {
                    blob_hash: blob.hash.clone(),
                    // A package filename is storage bookkeeping, not author
                    // supplied alternative text. In particular, our own
                    // writer emits a required DrawingML name while omitting
                    // `descr` for an empty source alt string; importing that
                    // name here would manufacture accessibility content.
                    alt_text: alt.unwrap_or_default(),
                    layout,
                },
                content: Vec::new(),
                properties: BlockProperties::default(),
            },
            None => {
                let target = self
                    .current_relationship(rel_id)
                    .map(|rel| rel.target.clone())
                    .unwrap_or_else(|| rel_id.to_string());
                let name = media_name(&target);
                self.warnings.push(ModelWarning {
                    code: MISSING_IMAGE_BLOB.to_string(),
                    message: format!(
                        "DOCX image relationship {rel_id} target {target} could not be read"
                    ),
                });
                Block::paragraph(format!("[missing DOCX image: {name}]"))
            }
        };
        self.block_ids.insert(block.id.clone());
        block
    }

    // -- inline-level walk ------------------------------------------------

    fn walk_paragraph_content(&mut self, container: &XmlElement, state: &mut ParagraphState) {
        for element in container.elements() {
            match element.local.as_str() {
                "pPr" | "rPr" | "bookmarkStart" | "bookmarkEnd" | "proofErr" | "sdtPr"
                | "sdtEndPr" | "customXmlPr" => {}
                "r" => self.walk_run(element, state),
                "hyperlink" => self.walk_hyperlink(element, state),
                "fldSimple" => {
                    // A page number stated as `w:fldSimple` is the same field
                    // as the three-run `w:fldChar` form; the cached result
                    // inside it is dropped for the same reason.
                    if let Some(field) = element.attr("instr").and_then(page_number_field) {
                        self.emit(
                            state,
                            Inline::PageNumber {
                                id: StableId::new("page-number"),
                                field,
                            },
                        );
                        continue;
                    }
                    let href = element.attr("instr").and_then(hyperlink_field_target);
                    if let Some(href) = href {
                        state.links.push(href);
                        self.walk_paragraph_content(element, state);
                        state.links.pop();
                    } else {
                        self.walk_paragraph_content(element, state);
                    }
                }
                "ins" | "moveTo" => self.walk_insertion(element, state),
                "del" | "moveFrom" => self.walk_deletion(element, state),
                "oMathPara" => {
                    if let Some(source) = math_source(element) {
                        let inline = equation_inline(source);
                        self.emit_segment(state, Segment::MathPara(inline));
                    }
                }
                "oMath" => {
                    if let Some(source) = math_source(element) {
                        self.emit(state, equation_inline(source));
                    }
                }
                "commentRangeStart" => {
                    if !state.nested {
                        if let Some(id) = element.attr("id") {
                            self.open_comment_range(id.trim());
                        }
                    }
                }
                "commentRangeEnd" => {
                    if !state.nested {
                        if let Some(id) = element.attr("id") {
                            self.close_comment_range(id.trim());
                        }
                    }
                }
                "tbl" | "p" => {
                    // Paragraph content never nests block content directly; text boxes
                    // and similar wrappers are handled by the run walker.
                }
                _ => self.walk_paragraph_content(element, state),
            }
        }
    }

    fn walk_hyperlink(&mut self, element: &XmlElement, state: &mut ParagraphState) {
        let href = element
            .attr_prefixed("r", "id")
            .and_then(|id| self.current_relationship(id.trim()))
            .map(|rel| rel.target.clone())
            .filter(|target| !target.trim().is_empty())
            .or_else(|| {
                element
                    .attr("anchor")
                    .map(str::trim)
                    .filter(|anchor| !anchor.is_empty())
                    .map(|anchor| format!("#{anchor}"))
            })
            .or_else(|| {
                element
                    .attr("docLocation")
                    .map(str::trim)
                    .filter(|location| !location.is_empty())
                    .map(|location| format!("#{location}"))
            });
        match href {
            Some(href) => {
                state.links.push(href);
                self.walk_paragraph_content(element, state);
                state.links.pop();
            }
            None => self.walk_paragraph_content(element, state),
        }
    }

    fn walk_insertion(&mut self, element: &XmlElement, state: &mut ParagraphState) {
        if state.nested {
            self.count(DROPPED_NESTED_REVISION);
            self.walk_paragraph_content(element, state);
            return;
        }
        let kind = if element.is("moveTo") {
            "moveTo"
        } else {
            "ins"
        };
        let revision = Revision::from_element(kind, element);
        let anchor = match state.last_inline_id.clone() {
            Some(id) => id,
            None => {
                let placeholder = Inline::text("");
                let id = inline_id(&placeholder).clone();
                self.emit(state, placeholder);
                id
            }
        };
        state.inserts.push(InsertCapture {
            revision,
            anchor,
            content: Vec::new(),
        });
        self.walk_paragraph_content(element, state);
        let Some(capture) = state.inserts.pop() else {
            return;
        };
        if !inlines_have_source(&capture.content) {
            return;
        }
        self.suggestions.push(Suggestion {
            id: StableId::new("suggestion"),
            author: capture.revision.author.clone(),
            kind: SuggestionKind::Insert {
                anchor: Anchor::TextRange(TextRange {
                    start: capture.anchor.clone(),
                    end: capture.anchor,
                }),
                content: capture.content,
            },
            state: SuggestionState::Proposed,
            provenance: capture.revision.provenance(),
        });
    }

    fn walk_deletion(&mut self, element: &XmlElement, state: &mut ParagraphState) {
        if state.nested || !state.inserts.is_empty() {
            self.count(DROPPED_NESTED_REVISION);
            self.walk_paragraph_content(element, state);
            return;
        }
        let kind = if element.is("moveFrom") {
            "moveFrom"
        } else {
            "del"
        };
        state.deletes.push(DeleteCapture {
            revision: Revision::from_element(kind, element),
            start: None,
            end: None,
        });
        self.walk_paragraph_content(element, state);
        let Some(capture) = state.deletes.pop() else {
            return;
        };
        if let (Some(start), Some(end)) = (capture.start, capture.end) {
            self.suggestions.push(Suggestion {
                id: StableId::new("suggestion"),
                author: capture.revision.author.clone(),
                kind: SuggestionKind::Delete {
                    range: TextRange { start, end },
                },
                state: SuggestionState::Proposed,
                provenance: capture.revision.provenance(),
            });
        }
    }

    fn walk_run(&mut self, run: &XmlElement, state: &mut ParagraphState) {
        let parsed = run.child("rPr").map(parse_run_props).unwrap_or_default();
        let mut base = state.style_props.clone();
        if let Some(style) = &parsed.style {
            base.overlay(&self.parts.styles.resolve(style).run_props);
        }
        let (props, format_change) = match &parsed.previous {
            Some(previous) => {
                let mut old = base.clone();
                old.overlay(previous);
                let mut new = base;
                new.overlay(&parsed.props);
                let old_marks = old.marks();
                let added: Vec<Mark> = new
                    .marks()
                    .into_iter()
                    .filter(|mark| !old_marks.contains(mark))
                    .collect();
                (old, Some(added))
            }
            None => {
                base.overlay(&parsed.props);
                (base, None)
            }
        };
        if !parsed.dropped.is_empty() {
            self.count(DROPPED_RUN_PROPERTY);
            self.dropped
                .run_property_names
                .extend(parsed.dropped.iter().copied());
        }
        let marks = props.marks();
        let mut text = String::new();
        let mut run_inline_ids: Vec<StableId> = Vec::new();

        for child in run.elements() {
            match child.local.as_str() {
                "rPr" => {}
                "t" | "delText" => {
                    if !state.in_field_instruction() && !state.in_suppressed_field_result() {
                        text.push_str(&child.text());
                    }
                }
                "tab" | "ptab" => text.push('\t'),
                "br" => {
                    let is_page = child
                        .attr("type")
                        .is_some_and(|kind| kind.trim().eq_ignore_ascii_case("page"));
                    if is_page && !state.nested && state.inserts.is_empty() {
                        self.flush_run_text(state, &mut text, &marks, &mut run_inline_ids);
                        self.emit_segment(state, Segment::PageBreak);
                    } else {
                        text.push('\n');
                    }
                }
                "cr" => text.push('\n'),
                "noBreakHyphen" => text.push('\u{2011}'),
                "sym" => {
                    if let Some(ch) = child
                        .attr("char")
                        .and_then(|value| u32::from_str_radix(value.trim(), 16).ok())
                        .and_then(char::from_u32)
                    {
                        text.push(ch);
                    }
                }
                "footnoteReference" | "endnoteReference" => {
                    self.flush_run_text(state, &mut text, &marks, &mut run_inline_ids);
                    let endnote = child.is("endnoteReference");
                    if let Some(id) = child.attr("id") {
                        self.emit_note_reference(state, endnote, id.trim());
                    }
                }
                "commentReference" => {
                    if !state.nested {
                        if let Some(id) = child.attr("id") {
                            let block_id = state.block_id();
                            self.comment_ranges
                                .entry(id.trim().to_string())
                                .or_default()
                                .block_id
                                .get_or_insert(block_id);
                        }
                    }
                }
                "fldChar" => {
                    self.flush_run_text(state, &mut text, &marks, &mut run_inline_ids);
                    self.handle_field_char(child, state);
                }
                "instrText" => {
                    if let Some(field) = state.fields.last_mut() {
                        if !field.in_result {
                            field.instruction.push_str(&child.text());
                        }
                    }
                }
                "drawing" | "pict" | "object" | "AlternateContent" => {
                    self.flush_run_text(state, &mut text, &marks, &mut run_inline_ids);
                    self.handle_image(child, state);
                }
                _ => {}
            }
        }
        self.flush_run_text(state, &mut text, &marks, &mut run_inline_ids);

        if let Some(added_marks) = format_change {
            match (run_inline_ids.first(), run_inline_ids.last()) {
                (Some(start), Some(end)) if !added_marks.is_empty() && !state.nested => {
                    let revision = run
                        .child("rPr")
                        .and_then(|rpr| rpr.child("rPrChange"))
                        .map(|change| Revision::from_element("rPrChange", change));
                    let revision = revision.unwrap_or_else(|| Revision {
                        kind: "rPrChange",
                        id: "?".to_string(),
                        author: "Unknown".to_string(),
                        date: None,
                    });
                    self.suggestions.push(Suggestion {
                        id: StableId::new("suggestion"),
                        author: revision.author.clone(),
                        kind: SuggestionKind::Format {
                            range: TextRange {
                                start: start.clone(),
                                end: end.clone(),
                            },
                            marks: added_marks,
                        },
                        state: SuggestionState::Proposed,
                        provenance: revision.provenance(),
                    });
                }
                _ => self.count(DROPPED_FORMAT_CHANGE),
            }
        }
    }

    fn flush_run_text(
        &mut self,
        state: &mut ParagraphState,
        text: &mut String,
        marks: &[Mark],
        run_inline_ids: &mut Vec<StableId>,
    ) {
        if text.is_empty() {
            return;
        }
        let content = std::mem::take(text);
        let inline = match state.links.last() {
            Some(href) => Inline::Link {
                id: StableId::new("link"),
                text: content,
                href: href.clone(),
                marks: marks.to_vec(),
            },
            None => Inline::Text {
                id: StableId::new("text"),
                text: content,
                marks: marks.to_vec(),
            },
        };
        run_inline_ids.push(inline_id(&inline).clone());
        self.emit(state, inline);
    }

    fn handle_field_char(&mut self, field_char: &XmlElement, state: &mut ParagraphState) {
        match field_char.attr("fldCharType").map(str::trim) {
            Some("begin") => state.fields.push(FieldState {
                instruction: String::new(),
                in_result: false,
                pushed_link: false,
                suppressed_result: false,
            }),
            Some("separate") => {
                let mut page_number = None;
                if let Some(field) = state.fields.last_mut() {
                    field.in_result = true;
                    if let Some(href) = hyperlink_field_target(&field.instruction) {
                        field.pushed_link = true;
                        state.links.push(href);
                    }
                    if let Some(which) = page_number_field(&field.instruction) {
                        field.suppressed_result = true;
                        page_number = Some(which);
                    }
                }
                if let Some(field) = page_number {
                    self.emit(
                        state,
                        Inline::PageNumber {
                            id: StableId::new("page-number"),
                            field,
                        },
                    );
                }
            }
            Some("end") => {
                if let Some(field) = state.fields.pop() {
                    if field.pushed_link {
                        state.links.pop();
                    }
                }
            }
            _ => {}
        }
    }

    fn emit_note_reference(&mut self, state: &mut ParagraphState, endnote: bool, id: &str) {
        if state.nested {
            return;
        }
        match self.note_ids.get(&(endnote, id.to_string())).cloned() {
            Some(footnote_id) => {
                let inline = Inline::FootnoteRef {
                    id: StableId::new("footnote-ref"),
                    footnote_id,
                };
                self.emit(state, inline);
            }
            None => self.count(MISSING_FOOTNOTE),
        }
    }

    fn handle_image(&mut self, element: &XmlElement, state: &mut ParagraphState) {
        if element.has_descendant("txbxContent") {
            self.count(DROPPED_TEXT_BOX);
            return;
        }
        let rel_id = element.descendants().into_iter().find_map(|node| {
            if node.is("blip") {
                node.attr_prefixed("r", "embed")
                    .or_else(|| node.attr_prefixed("r", "link"))
            } else if node.is("imagedata") {
                node.attr_prefixed("r", "id")
            } else {
                None
            }
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
        });
        let Some(rel_id) = rel_id else {
            self.count(DROPPED_DRAWING);
            return;
        };
        if state.nested || !state.inserts.is_empty() {
            self.count(DROPPED_NESTED_IMAGE);
            return;
        }
        if has_unmapped_positioned_image(element) {
            self.count(DROPPED_POSITIONED_IMAGE);
        }
        let alt = docx_image_accessible_text(element);
        let (layout, malformed_opacity, malformed_crop, malformed_border) =
            docx_image_layout(element);
        if malformed_opacity {
            self.count(DROPPED_IMAGE_OPACITY);
        }
        if malformed_crop {
            self.count(DROPPED_IMAGE_CROP);
        }
        if malformed_border {
            self.count(DROPPED_IMAGE_BORDER);
        }
        self.emit_segment(
            state,
            Segment::Image {
                rel_id,
                alt,
                layout,
            },
        );
    }

    fn emit(&mut self, state: &mut ParagraphState, inline: Inline) {
        if let Some(capture) = state.inserts.last_mut() {
            capture.content.push(inline);
            return;
        }
        let id = inline_id(&inline).clone();
        if !state.nested {
            let block_id = state.block_id();
            for docx_id in &self.open_comment_ranges {
                let range = self.comment_ranges.entry(docx_id.clone()).or_default();
                if range.start.is_none() {
                    range.start = Some(id.clone());
                }
                range.end = Some(id.clone());
                if range.block_id.is_none() {
                    range.block_id = Some(block_id.clone());
                }
            }
            for capture in state.deletes.iter_mut() {
                if capture.start.is_none() {
                    capture.start = Some(id.clone());
                }
                capture.end = Some(id.clone());
            }
        }
        state.last_inline_id = Some(id);
        state.segments.push(Segment::Inline(inline));
    }

    fn emit_segment(&mut self, state: &mut ParagraphState, segment: Segment) {
        match segment {
            Segment::Inline(inline) => self.emit(state, inline),
            Segment::MathPara(inline) => {
                if state.inserts.last().is_some() {
                    self.emit(state, inline);
                    return;
                }
                // Register the equation like any inline so ranges can cover it,
                // but keep the display-math flag for standalone detection.
                self.emit(state, inline);
                if let Some(Segment::Inline(inline)) = state.segments.pop() {
                    state.segments.push(Segment::MathPara(inline));
                }
            }
            Segment::PageBreak | Segment::Image { .. } => {
                state.segments.push(segment);
                state.start_fragment();
            }
        }
    }
}

/// DrawingML holds an image title and description separately. OpenDoc has one
/// accessible-text field, so retain both source values in their documented
/// order rather than treating the first non-empty attribute as an excuse to
/// discard the other. `wp:docPr` is authoritative when present, but some
/// producers put equivalent metadata only on `pic:cNvPr`. DrawingML's
/// required generic object name is bookkeeping, not a replacement for either
/// accessible field.
fn docx_image_accessible_text(element: &XmlElement) -> Option<String> {
    let accessible_text = |properties: &XmlElement| {
        let attribute = |name: &str| {
            properties
                .attr(name)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        };
        match (attribute("title"), attribute("descr")) {
            (Some(title), Some(description)) if title == description => Some(title),
            (Some(title), Some(description)) => Some(format!("{title}\n{description}")),
            (Some(title), None) => Some(title),
            (None, Some(description)) => Some(description),
            (None, None) => None,
        }
    };
    let doc_pr = element.find_descendant("docPr");
    let picture_properties = element.find_descendant("cNvPr");
    // `wp:docPr` is the drawing's authoritative nonvisual record. Some
    // producers leave its accessible fields empty and put them only on
    // `pic:cNvPr`, so fall back by whole record — never splice a title from
    // one record with a description from the other, which fabricates an
    // accessible label when producer metadata conflicts.
    doc_pr
        .and_then(accessible_text)
        .or_else(|| picture_properties.and_then(accessible_text))
}

/// Recognise the bounded field shape written by [`docx_write`]: a paragraph
/// with no properties and one `w:fldSimple` instruction beginning `TOC` and
/// carrying its heading scope as `\o "1-N"`. A result cached by Word is
/// intentionally irrelevant—OpenDoc derives entries again from its headings.
fn docx_simple_toc_level(paragraph: &XmlElement) -> Option<u8> {
    if paragraph.child("pPr").is_some() {
        return None;
    }
    let fields: Vec<&XmlElement> = paragraph.elements().collect();
    let [field] = fields.as_slice() else {
        return None;
    };
    if !field.is("fldSimple") {
        return None;
    }
    let instruction = field.attr("instr")?.trim();
    let mut tokens = instruction.split_whitespace();
    if !tokens.next()?.eq_ignore_ascii_case("TOC") {
        return None;
    }
    while let Some(token) = tokens.next() {
        if token.eq_ignore_ascii_case("\\o") {
            let range = tokens.next()?.trim_matches('"');
            let (first, last) = range.split_once('-')?;
            if first != "1" {
                return None;
            }
            let max_level = last.parse::<u8>().ok()?;
            return (1..=6).contains(&max_level).then_some(max_level);
        }
    }
    None
}
