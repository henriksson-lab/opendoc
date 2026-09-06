//! DOCX (WordprocessingML) reader.
//!
//! The package is opened with the `zip` crate and every part is parsed into a
//! small DOM (see [`crate::xml`]). The main document body is converted into the
//! OpenDoc model together with numbering definitions, style inheritance,
//! footnotes/endnotes, comment threads, tracked changes, images, tables,
//! hyperlinks, page breaks and Office Math equations. Properties that the core
//! model cannot represent are counted and reported as warnings (one per
//! property kind) instead of being silently dropped.

use crate::xml::{parse_xml_bytes, XmlElement};
use crate::{mark, ImportError, ImportedBlob};
use opendoc_core::{
    Anchor, Block, BlockKind, Comment, CommentThread, Document, Equation, EquationSourceFormat,
    Footnote, Inline, Mark, MarkKind, ModelWarning, StableId, Suggestion, SuggestionKind,
    SuggestionState, TableCell, TableRow, TextRange,
};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read};

pub(crate) struct DocxImport {
    pub(crate) document: Document,
    pub(crate) warnings: Vec<ModelWarning>,
    pub(crate) blobs: Vec<ImportedBlob>,
}

/// Imports either a zipped DOCX package or a raw `word/document.xml` payload.
pub(crate) fn import_docx_bytes(title: &str, bytes: &[u8]) -> Result<DocxImport, ImportError> {
    let parts = if bytes.starts_with(b"PK") {
        DocxParts::from_package(bytes)?
    } else {
        let xml = parse_xml_bytes(bytes).map_err(|err| {
            ImportError::InvalidInput(format!(
                "DOCX input is neither a ZIP package nor WordprocessingML XML: {err}"
            ))
        })?;
        if !xml.is("document") {
            return Err(ImportError::InvalidInput(
                "DOCX XML root element is not w:document".to_string(),
            ));
        }
        DocxParts::from_raw_document(xml)
    };
    convert_parts(title, &parts)
}

// ---------------------------------------------------------------------------
// Package / relationships
// ---------------------------------------------------------------------------

struct Package {
    archive: zip::ZipArchive<Cursor<Vec<u8>>>,
    names: Vec<String>,
}

impl Package {
    fn open(bytes: &[u8]) -> Result<Self, ImportError> {
        let archive = zip::ZipArchive::new(Cursor::new(bytes.to_vec())).map_err(|err| {
            ImportError::InvalidInput(format!("DOCX package could not be opened: {err}"))
        })?;
        let names = archive.file_names().map(str::to_string).collect();
        Ok(Self { archive, names })
    }

    fn part(&mut self, name: &str) -> Option<Vec<u8>> {
        let clean = name.trim_start_matches('/');
        let actual = self
            .names
            .iter()
            .find(|candidate| candidate.as_str() == clean)
            .or_else(|| {
                self.names
                    .iter()
                    .find(|candidate| candidate.eq_ignore_ascii_case(clean))
            })?
            .clone();
        let mut file = self.archive.by_name(&actual).ok()?;
        let mut out = Vec::new();
        file.read_to_end(&mut out).ok()?;
        Some(out)
    }

    fn xml_part(&mut self, name: &str) -> Option<XmlElement> {
        let bytes = self.part(name)?;
        parse_xml_bytes(&bytes).ok()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Relationship {
    id: String,
    rel_type: String,
    target: String,
    external: bool,
}

fn parse_relationships(bytes: &[u8]) -> Vec<Relationship> {
    let Ok(root) = parse_xml_bytes(bytes) else {
        return Vec::new();
    };
    root.children_named("Relationship")
        .filter_map(|rel| {
            let id = rel.attr("Id")?.to_string();
            let target = rel.attr("Target")?.trim().to_string();
            if target.is_empty() {
                return None;
            }
            Some(Relationship {
                id,
                rel_type: rel.attr("Type").unwrap_or_default().to_string(),
                target,
                external: rel
                    .attr("TargetMode")
                    .is_some_and(|mode| mode.eq_ignore_ascii_case("External")),
            })
        })
        .collect()
}

/// Resolves a relationship target against the directory of the source part.
fn resolve_part_path(base_dir: &str, target: &str) -> Option<String> {
    let target = target.trim();
    if target.is_empty() || target.contains("://") {
        return None;
    }
    let mut segments: Vec<&str> = if target.starts_with('/') {
        Vec::new()
    } else {
        base_dir.split('/').filter(|s| !s.is_empty()).collect()
    };
    for segment in target.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop()?;
            }
            other => segments.push(other),
        }
    }
    if segments.is_empty() {
        None
    } else {
        Some(segments.join("/"))
    }
}

fn media_name(path: &str) -> String {
    path.rsplit('/')
        .next()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("docx-image")
        .to_string()
}

fn media_type(path: &str) -> &'static str {
    match path
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "bmp" => "image/bmp",
        "tif" | "tiff" => "image/tiff",
        "emf" => "image/emf",
        "wmf" => "image/wmf",
        "xml" | "rels" => "application/xml",
        _ => "application/octet-stream",
    }
}

fn relationship_is_image(rel: &Relationship) -> bool {
    rel.rel_type.ends_with("/image")
        || !matches!(
            media_type(&rel.target),
            "application/octet-stream" | "application/xml"
        )
}

// ---------------------------------------------------------------------------
// Loaded parts
// ---------------------------------------------------------------------------

struct DocxParts {
    document: XmlElement,
    relationships: BTreeMap<String, Relationship>,
    styles: Styles,
    numbering: Numbering,
    footnotes: Option<XmlElement>,
    endnotes: Option<XmlElement>,
    comments: Option<XmlElement>,
    comments_extended: Option<XmlElement>,
    media: BTreeMap<String, ImportedBlob>,
    header_footer_parts: usize,
}

impl DocxParts {
    fn from_raw_document(document: XmlElement) -> Self {
        Self {
            document,
            relationships: BTreeMap::new(),
            styles: Styles::default(),
            numbering: Numbering::default(),
            footnotes: None,
            endnotes: None,
            comments: None,
            comments_extended: None,
            media: BTreeMap::new(),
            header_footer_parts: 0,
        }
    }

    fn from_package(bytes: &[u8]) -> Result<Self, ImportError> {
        let mut package = Package::open(bytes)?;
        let root_rels = package
            .part("_rels/.rels")
            .map(|bytes| parse_relationships(&bytes))
            .unwrap_or_default();
        let document_path = root_rels
            .iter()
            .find(|rel| rel.rel_type.ends_with("/officeDocument") && !rel.external)
            .and_then(|rel| resolve_part_path("", &rel.target))
            .unwrap_or_else(|| "word/document.xml".to_string());
        let document_bytes = package.part(&document_path).ok_or_else(|| {
            ImportError::InvalidInput(format!(
                "DOCX package has no main document part {document_path}"
            ))
        })?;
        let document = parse_xml_bytes(&document_bytes).map_err(|err| {
            ImportError::InvalidInput(format!("DOCX main document part is malformed: {err}"))
        })?;
        let (dir, file) = match document_path.rsplit_once('/') {
            Some((dir, file)) => (dir.to_string(), file.to_string()),
            None => (String::new(), document_path.clone()),
        };
        let rels_path = if dir.is_empty() {
            format!("_rels/{file}.rels")
        } else {
            format!("{dir}/_rels/{file}.rels")
        };
        let document_rels = package
            .part(&rels_path)
            .map(|bytes| parse_relationships(&bytes))
            .unwrap_or_default();

        let mut parts = Self::from_raw_document(document);
        let mut styles_part = None;
        let mut numbering_part = None;
        for rel in document_rels {
            let part_path = if rel.external {
                None
            } else {
                resolve_part_path(&dir, &rel.target)
            };
            let rel_type = rel.rel_type.as_str();
            if let Some(path) = &part_path {
                if rel_type.ends_with("/styles") {
                    styles_part = package.xml_part(path);
                } else if rel_type.ends_with("/numbering") {
                    numbering_part = package.xml_part(path);
                } else if rel_type.ends_with("/footnotes") {
                    parts.footnotes = package.xml_part(path);
                } else if rel_type.ends_with("/endnotes") {
                    parts.endnotes = package.xml_part(path);
                } else if rel_type.ends_with("/comments") {
                    parts.comments = package.xml_part(path);
                } else if rel_type.ends_with("/commentsExtended") {
                    parts.comments_extended = package.xml_part(path);
                } else if rel_type.ends_with("/header") || rel_type.ends_with("/footer") {
                    parts.header_footer_parts += 1;
                } else if relationship_is_image(&rel) {
                    if let Some(bytes) = package.part(path).filter(|bytes| !bytes.is_empty()) {
                        let hash = opendoc_core::digest_bytes("sha256", &bytes)
                            .map_err(|err| ImportError::InvalidInput(err.to_string()))?
                            .to_string();
                        parts.media.insert(
                            rel.id.clone(),
                            ImportedBlob {
                                name: media_name(path),
                                media_type: media_type(path).to_string(),
                                hash,
                                bytes,
                            },
                        );
                    }
                }
            }
            parts.relationships.insert(rel.id.clone(), rel);
        }
        let conventional = |name: &str| {
            if dir.is_empty() {
                name.to_string()
            } else {
                format!("{dir}/{name}")
            }
        };
        let styles_part = styles_part.or_else(|| package.xml_part(&conventional("styles.xml")));
        let numbering_part =
            numbering_part.or_else(|| package.xml_part(&conventional("numbering.xml")));
        parts.styles = styles_part.map(Styles::parse).unwrap_or_default();
        parts.numbering = numbering_part.map(Numbering::parse).unwrap_or_default();
        Ok(parts)
    }
}

// ---------------------------------------------------------------------------
// Run properties
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct RunProps {
    bold: Option<bool>,
    italic: Option<bool>,
    underline: Option<bool>,
    strike: Option<bool>,
    vert_align: Option<String>,
    color: Option<String>,
    background: Option<String>,
    size: Option<u32>,
    font: Option<String>,
}

impl RunProps {
    fn overlay(&mut self, over: &RunProps) {
        if over.bold.is_some() {
            self.bold = over.bold;
        }
        if over.italic.is_some() {
            self.italic = over.italic;
        }
        if over.underline.is_some() {
            self.underline = over.underline;
        }
        if over.strike.is_some() {
            self.strike = over.strike;
        }
        if over.vert_align.is_some() {
            self.vert_align.clone_from(&over.vert_align);
        }
        if over.color.is_some() {
            self.color.clone_from(&over.color);
        }
        if over.background.is_some() {
            self.background.clone_from(&over.background);
        }
        if over.size.is_some() {
            self.size = over.size;
        }
        if over.font.is_some() {
            self.font.clone_from(&over.font);
        }
    }

    fn marks(&self) -> Vec<Mark> {
        let mut marks = Vec::new();
        if self.bold == Some(true) {
            marks.push(mark(MarkKind::Bold, None));
        }
        if self.italic == Some(true) {
            marks.push(mark(MarkKind::Italic, None));
        }
        if self.underline == Some(true) {
            marks.push(mark(MarkKind::Underline, None));
        }
        if self.strike == Some(true) {
            marks.push(mark(MarkKind::Strike, None));
        }
        match self.vert_align.as_deref() {
            Some("superscript") => marks.push(mark(MarkKind::Superscript, None)),
            Some("subscript") => marks.push(mark(MarkKind::Subscript, None)),
            _ => {}
        }
        if let Some(color) = &self.color {
            marks.push(mark(MarkKind::Color, Some(color.clone())));
        }
        if let Some(background) = &self.background {
            marks.push(mark(MarkKind::Background, Some(background.clone())));
        }
        if let Some(font) = &self.font {
            marks.push(mark(MarkKind::Font, Some(font.clone())));
        }
        if let Some(size) = self.size {
            marks.push(mark(MarkKind::Size, Some(size.to_string())));
        }
        marks
    }
}

#[derive(Clone, Debug, Default)]
struct ParsedRunProps {
    props: RunProps,
    style: Option<String>,
    /// Properties before a tracked formatting change (`w:rPrChange`).
    previous: Option<RunProps>,
    dropped: Vec<&'static str>,
}

fn toggle_value(element: &XmlElement) -> bool {
    match element.attr("val") {
        None => true,
        Some(value) => !matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "off"
        ),
    }
}

fn hex_color(value: &str) -> Option<String> {
    let value = value.trim();
    if value.len() == 6 && value.chars().all(|ch| ch.is_ascii_hexdigit()) {
        Some(format!("#{}", value.to_ascii_lowercase()))
    } else {
        None
    }
}

fn highlight_color(value: &str) -> Option<String> {
    let hex = match value.trim() {
        "black" => "000000",
        "blue" => "0000ff",
        "cyan" => "00ffff",
        "green" => "00ff00",
        "magenta" => "ff00ff",
        "red" => "ff0000",
        "yellow" => "ffff00",
        "white" => "ffffff",
        "darkBlue" => "000080",
        "darkCyan" => "008080",
        "darkGreen" => "008000",
        "darkMagenta" => "800080",
        "darkRed" => "800000",
        "darkYellow" => "808000",
        "darkGray" => "808080",
        "lightGray" => "c0c0c0",
        _ => return None,
    };
    Some(format!("#{hex}"))
}

fn parse_run_props(rpr: &XmlElement) -> ParsedRunProps {
    let mut parsed = ParsedRunProps::default();
    for element in rpr.elements() {
        match element.local.as_str() {
            "b" => parsed.props.bold = Some(toggle_value(element)),
            "i" => parsed.props.italic = Some(toggle_value(element)),
            "u" => {
                parsed.props.underline = Some(
                    !element
                        .attr("val")
                        .is_some_and(|value| value.trim().eq_ignore_ascii_case("none")),
                )
            }
            "strike" | "dstrike" => parsed.props.strike = Some(toggle_value(element)),
            "vertAlign" => {
                parsed.props.vert_align = element.attr("val").map(|value| value.trim().to_string())
            }
            "color" => {
                if let Some(color) = element.attr("val").and_then(hex_color) {
                    parsed.props.color = Some(color);
                }
            }
            "highlight" => {
                if let Some(color) = element.attr("val").and_then(highlight_color) {
                    parsed.props.background = Some(color);
                }
            }
            "shd" => {
                if parsed.props.background.is_none() {
                    if let Some(color) = element.attr("fill").and_then(hex_color) {
                        parsed.props.background = Some(color);
                    }
                }
            }
            "sz" => {
                if let Some(size) = element
                    .attr("val")
                    .and_then(|value| value.trim().parse::<u32>().ok())
                    .filter(|size| *size > 0)
                {
                    parsed.props.size = Some(size / 2);
                }
            }
            "rFonts" => {
                if let Some(font) = ["ascii", "hAnsi", "cs", "eastAsia"]
                    .iter()
                    .find_map(|attr| element.attr(attr))
                    .map(str::trim)
                    .filter(|font| !font.is_empty())
                {
                    parsed.props.font = Some(font.to_string());
                }
            }
            "rStyle" => parsed.style = element.attr("val").map(|value| value.to_string()),
            "rPrChange" => {
                parsed.previous = Some(
                    element
                        .child("rPr")
                        .map(|old| parse_run_props(old).props)
                        .unwrap_or_default(),
                );
            }
            "caps" | "smallCaps" | "vanish" | "emboss" | "imprint" | "outline" | "shadow"
            | "webHidden" => {
                if toggle_value(element) {
                    parsed.dropped.push(leak_name(&element.local));
                }
            }
            "spacing" | "w" | "kern" | "position" | "effect" | "em" | "fitText" => {
                parsed.dropped.push(leak_name(&element.local));
            }
            _ => {}
        }
    }
    parsed
}

/// Maps a known run-property name to a `'static` label for warning messages.
fn leak_name(name: &str) -> &'static str {
    match name {
        "caps" => "caps",
        "smallCaps" => "smallCaps",
        "vanish" => "vanish",
        "emboss" => "emboss",
        "imprint" => "imprint",
        "outline" => "outline",
        "shadow" => "shadow",
        "webHidden" => "webHidden",
        "spacing" => "spacing",
        "w" => "w",
        "kern" => "kern",
        "position" => "position",
        "effect" => "effect",
        "em" => "em",
        "fitText" => "fitText",
        _ => "other",
    }
}

// ---------------------------------------------------------------------------
// Styles
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HeadingStyle {
    Title,
    Subtitle,
    Level(u8),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct NumPr {
    num_id: String,
    level: u8,
}

fn parse_num_pr(num_pr: &XmlElement) -> Option<NumPr> {
    let num_id = num_pr.child_val("numId")?.trim().to_string();
    let level = num_pr
        .child_val("ilvl")
        .and_then(|value| value.trim().parse::<u8>().ok())
        .unwrap_or(0)
        .min(8);
    Some(NumPr { num_id, level })
}

#[derive(Clone, Debug, Default)]
struct StyleRecord {
    based_on: Option<String>,
    heading: Option<HeadingStyle>,
    num_pr: Option<NumPr>,
    run_props: RunProps,
}

#[derive(Clone, Debug, Default)]
struct Styles {
    by_id: BTreeMap<String, StyleRecord>,
}

#[derive(Clone, Debug, Default)]
struct ResolvedStyle {
    heading: Option<HeadingStyle>,
    num_pr: Option<NumPr>,
    run_props: RunProps,
}

fn detect_heading(style_id: &str, name: &str, outline_level: Option<u8>) -> Option<HeadingStyle> {
    let id = style_id.trim();
    let name = name.trim().to_ascii_lowercase();
    if id.eq_ignore_ascii_case("Title") || name == "title" {
        return Some(HeadingStyle::Title);
    }
    if id.eq_ignore_ascii_case("Subtitle") || name == "subtitle" {
        return Some(HeadingStyle::Subtitle);
    }
    let level = id
        .strip_prefix("Heading")
        .or_else(|| id.strip_prefix("heading"))
        .and_then(|rest| rest.trim().parse::<u8>().ok())
        .or_else(|| {
            name.strip_prefix("heading")
                .and_then(|rest| rest.trim().parse::<u8>().ok())
        });
    if let Some(level) = level.filter(|level| *level >= 1) {
        return Some(HeadingStyle::Level(level.min(6)));
    }
    outline_level
        .filter(|level| *level <= 8)
        .map(|level| HeadingStyle::Level((level + 1).min(6)))
}

impl Styles {
    fn parse(root: XmlElement) -> Self {
        let mut by_id = BTreeMap::new();
        for style in root.children_named("style") {
            let Some(id) = style
                .attr("styleId")
                .map(str::trim)
                .filter(|id| !id.is_empty())
            else {
                continue;
            };
            let kind = style.attr("type").unwrap_or("paragraph");
            let name = style.child_val("name").unwrap_or_default();
            let ppr = style.child("pPr");
            let outline_level = ppr
                .and_then(|ppr| ppr.child_val("outlineLvl"))
                .and_then(|value| value.trim().parse::<u8>().ok());
            let heading = if kind == "paragraph" {
                detect_heading(id, name, outline_level)
            } else {
                None
            };
            by_id.insert(
                id.to_string(),
                StyleRecord {
                    based_on: style
                        .child_val("basedOn")
                        .map(|value| value.trim().to_string()),
                    heading,
                    num_pr: ppr
                        .and_then(|ppr| ppr.child("numPr"))
                        .and_then(parse_num_pr),
                    run_props: style
                        .child("rPr")
                        .map(|rpr| parse_run_props(rpr).props)
                        .unwrap_or_default(),
                },
            );
        }
        Self { by_id }
    }

    fn resolve(&self, style_id: &str) -> ResolvedStyle {
        let mut chain: Vec<&StyleRecord> = Vec::new();
        let mut seen = BTreeSet::new();
        let mut current = Some(style_id.trim().to_string());
        while let Some(id) = current {
            if !seen.insert(id.clone()) || chain.len() > 32 {
                break;
            }
            let Some(record) = self.by_id.get(&id) else {
                break;
            };
            chain.push(record);
            current = record.based_on.clone();
        }
        if chain.is_empty() {
            // Unknown style id (no styles part): fall back to the conventional names.
            return ResolvedStyle {
                heading: detect_heading(style_id, "", None),
                num_pr: None,
                run_props: RunProps::default(),
            };
        }
        let mut run_props = RunProps::default();
        for record in chain.iter().rev() {
            run_props.overlay(&record.run_props);
        }
        ResolvedStyle {
            heading: chain.iter().find_map(|record| record.heading),
            num_pr: chain.iter().find_map(|record| record.num_pr.clone()),
            run_props,
        }
    }
}

// ---------------------------------------------------------------------------
// Numbering
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default)]
struct AbstractNum {
    formats: BTreeMap<u8, String>,
    style_link: Option<String>,
}

#[derive(Clone, Debug, Default)]
struct NumInstance {
    abstract_id: String,
    overrides: BTreeMap<u8, String>,
}

#[derive(Clone, Debug, Default)]
struct Numbering {
    abstracts: BTreeMap<String, AbstractNum>,
    nums: BTreeMap<String, NumInstance>,
}

fn level_formats(container: &XmlElement) -> BTreeMap<u8, String> {
    container
        .children_named("lvl")
        .filter_map(|lvl| {
            let level = lvl.attr("ilvl")?.trim().parse::<u8>().ok()?;
            let format = lvl.child_val("numFmt")?.trim().to_string();
            Some((level, format))
        })
        .collect()
}

impl Numbering {
    fn parse(root: XmlElement) -> Self {
        let mut numbering = Self::default();
        for abstract_num in root.children_named("abstractNum") {
            let Some(id) = abstract_num.attr("abstractNumId") else {
                continue;
            };
            numbering.abstracts.insert(
                id.trim().to_string(),
                AbstractNum {
                    formats: level_formats(abstract_num),
                    style_link: abstract_num
                        .child_val("numStyleLink")
                        .map(|value| value.trim().to_string()),
                },
            );
        }
        for num in root.children_named("num") {
            let Some(id) = num.attr("numId") else {
                continue;
            };
            let mut overrides = BTreeMap::new();
            for lvl_override in num.children_named("lvlOverride") {
                if let Some(lvl) = lvl_override.child("lvl") {
                    if let (Some(level), Some(format)) = (
                        lvl_override
                            .attr("ilvl")
                            .and_then(|value| value.trim().parse::<u8>().ok()),
                        lvl.child_val("numFmt"),
                    ) {
                        overrides.insert(level, format.trim().to_string());
                    }
                }
            }
            numbering.nums.insert(
                id.trim().to_string(),
                NumInstance {
                    abstract_id: num
                        .child_val("abstractNumId")
                        .unwrap_or_default()
                        .trim()
                        .to_string(),
                    overrides,
                },
            );
        }
        numbering
    }

    fn level_format(&self, styles: &Styles, num_id: &str, level: u8) -> Option<String> {
        self.level_format_inner(styles, num_id, level, 0)
    }

    fn level_format_inner(
        &self,
        styles: &Styles,
        num_id: &str,
        level: u8,
        depth: usize,
    ) -> Option<String> {
        if depth > 4 {
            return None;
        }
        let num = self.nums.get(num_id)?;
        if let Some(format) = num.overrides.get(&level) {
            return Some(format.clone());
        }
        let abstract_num = self.abstracts.get(&num.abstract_id)?;
        if let Some(format) = abstract_num.formats.get(&level) {
            return Some(format.clone());
        }
        if let Some(style_link) = &abstract_num.style_link {
            if let Some(num_pr) = styles.resolve(style_link).num_pr {
                if num_pr.num_id != num_id {
                    return self.level_format_inner(styles, &num_pr.num_id, level, depth + 1);
                }
            }
        }
        // Fall back to the closest defined lower level.
        abstract_num
            .formats
            .range(..level)
            .next_back()
            .map(|(_, format)| format.clone())
    }

    /// `Some(ordered)` when the list level is defined, `None` when unknown.
    fn is_ordered(&self, styles: &Styles, num_id: &str, level: u8) -> Option<bool> {
        self.level_format(styles, num_id, level)
            .map(|format| !matches!(format.as_str(), "bullet" | "none" | ""))
    }
}

// ---------------------------------------------------------------------------
// Warning bookkeeping
// ---------------------------------------------------------------------------

const DROPPED_ALIGNMENT: &str = "docx-dropped-alignment";
const DROPPED_INDENT: &str = "docx-dropped-indent";
const DROPPED_SPACING: &str = "docx-dropped-spacing";
const DROPPED_PARAGRAPH_BORDER: &str = "docx-dropped-paragraph-border";
const DROPPED_PARAGRAPH_SHADING: &str = "docx-dropped-paragraph-shading";
const DROPPED_TABS: &str = "docx-dropped-tabs";
const DROPPED_HEADER_FOOTER: &str = "docx-dropped-header-footer";
const DROPPED_SECTION_PROPERTIES: &str = "docx-dropped-section-properties";
const DROPPED_RUN_PROPERTY: &str = "docx-dropped-run-property";
const DROPPED_PARAGRAPH_CHANGE: &str = "docx-dropped-paragraph-change";
const DROPPED_FORMAT_CHANGE: &str = "docx-dropped-format-change";
const DROPPED_DRAWING: &str = "docx-dropped-drawing";
const DROPPED_TEXT_BOX: &str = "docx-dropped-text-box";
const DROPPED_NESTED_IMAGE: &str = "docx-dropped-nested-image";
const DROPPED_NESTED_REVISION: &str = "docx-dropped-nested-revision";
const DROPPED_CELL_SPAN: &str = "docx-dropped-cell-span";
const DROPPED_ALT_CHUNK: &str = "docx-dropped-alt-chunk";
const NESTED_TABLE: &str = "docx-nested-table";
const SPLIT_INLINE_IMAGE: &str = "docx-split-inline-image";
const SPLIT_PAGE_BREAK: &str = "docx-split-page-break";
const TITLE_STYLE_AS_HEADING: &str = "docx-title-style-as-heading";
const UNKNOWN_LIST_DEFINITION: &str = "docx-unknown-list-definition";
const MISSING_FOOTNOTE: &str = "docx-missing-footnote";
const EMPTY_FOOTNOTE: &str = "docx-empty-footnote";
const ENDNOTES_AS_FOOTNOTES: &str = "docx-endnotes-as-footnotes";
const EMPTY_COMMENT: &str = "docx-empty-comment";
const COMMENT_ANCHOR_DEGRADED: &str = "docx-comment-anchor-degraded";
const MISSING_IMAGE_BLOB: &str = "missing-docx-image-blob";

fn dropped_message(code: &str) -> &'static str {
    match code {
        DROPPED_ALIGNMENT => "DOCX paragraph alignment (w:jc) is not representable and was dropped",
        DROPPED_INDENT => "DOCX paragraph indentation (w:ind) is not representable and was dropped",
        DROPPED_SPACING => "DOCX paragraph spacing (w:spacing) is not representable and was dropped",
        DROPPED_PARAGRAPH_BORDER => {
            "DOCX paragraph borders (w:pBdr) are not representable and were dropped"
        }
        DROPPED_PARAGRAPH_SHADING => {
            "DOCX paragraph shading (w:shd) is not representable and was dropped"
        }
        DROPPED_TABS => "DOCX custom tab stops (w:tabs) are not representable and were dropped",
        DROPPED_HEADER_FOOTER => "DOCX headers and footers are not representable and were dropped",
        DROPPED_SECTION_PROPERTIES => {
            "DOCX section properties (w:sectPr: page size, margins, columns) were dropped"
        }
        DROPPED_RUN_PROPERTY => "DOCX run properties without an OpenDoc mark were dropped",
        DROPPED_PARAGRAPH_CHANGE => {
            "DOCX tracked paragraph property changes (w:pPrChange) were dropped"
        }
        DROPPED_FORMAT_CHANGE => {
            "DOCX tracked formatting changes (w:rPrChange) without representable marks were dropped"
        }
        DROPPED_DRAWING => "DOCX drawings or shapes without image data were dropped",
        DROPPED_TEXT_BOX => "DOCX text boxes are not representable and were dropped",
        DROPPED_NESTED_IMAGE => {
            "DOCX images inside footnotes, comments or tracked insertions were dropped"
        }
        DROPPED_NESTED_REVISION => {
            "DOCX tracked changes inside footnotes or comments were flattened into plain text"
        }
        DROPPED_CELL_SPAN => "DOCX merged table cells (gridSpan/vMerge) were imported unmerged",
        DROPPED_ALT_CHUNK => "DOCX embedded alternate content chunks (w:altChunk) were dropped",
        NESTED_TABLE => {
            "DOCX nested tables were imported as tables inside table cells; Google Docs export cannot represent them"
        }
        SPLIT_INLINE_IMAGE => {
            "DOCX inline images mixed with text were imported as standalone image blocks, splitting the paragraph"
        }
        SPLIT_PAGE_BREAK => {
            "DOCX page breaks inside paragraphs were imported as standalone page break blocks, splitting the paragraph"
        }
        TITLE_STYLE_AS_HEADING => {
            "DOCX Title and Subtitle styled paragraphs were imported as level 1 and level 2 headings"
        }
        UNKNOWN_LIST_DEFINITION => {
            "DOCX list paragraphs referenced numbering definitions that could not be resolved; imported as bullet items"
        }
        MISSING_FOOTNOTE => "DOCX footnote or endnote references without a definition were dropped",
        EMPTY_FOOTNOTE => "DOCX footnotes or endnotes with an empty body were dropped",
        ENDNOTES_AS_FOOTNOTES => "DOCX endnotes were imported as footnotes",
        EMPTY_COMMENT => "DOCX comments with an empty body were dropped",
        COMMENT_ANCHOR_DEGRADED => {
            "DOCX comment ranges that did not cover inline text were anchored to the nearest block or the document"
        }
        _ => "DOCX content was dropped",
    }
}

#[derive(Default)]
struct DroppedCounter {
    counts: BTreeMap<&'static str, usize>,
    run_property_names: BTreeSet<&'static str>,
}

impl DroppedCounter {
    fn count(&mut self, code: &'static str) {
        self.count_n(code, 1);
    }

    fn count_n(&mut self, code: &'static str, n: usize) {
        if n == 0 {
            return;
        }
        *self.counts.entry(code).or_insert(0) += n;
    }

    fn into_warnings(self) -> Vec<ModelWarning> {
        self.counts
            .into_iter()
            .map(|(code, count)| {
                let mut message = dropped_message(code).to_string();
                if code == DROPPED_RUN_PROPERTY && !self.run_property_names.is_empty() {
                    message.push_str(&format!(
                        " ({})",
                        self.run_property_names
                            .iter()
                            .copied()
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
                message.push_str(&format!(" ({count} occurrence{})", plural(count)));
                ModelWarning {
                    code: code.to_string(),
                    message,
                }
            })
            .collect()
    }
}

fn plural(count: usize) -> &'static str {
    if count == 1 {
        ""
    } else {
        "s"
    }
}

// ---------------------------------------------------------------------------
// Conversion state
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default)]
struct CommentRange {
    start: Option<StableId>,
    end: Option<StableId>,
    block_id: Option<StableId>,
}

#[derive(Clone, Debug)]
struct Revision {
    kind: &'static str,
    id: String,
    author: String,
    date: Option<String>,
}

impl Revision {
    fn from_element(kind: &'static str, element: &XmlElement) -> Self {
        Self {
            kind,
            id: element.attr("id").unwrap_or("?").trim().to_string(),
            author: source_author(element.attr("author")),
            date: element
                .attr("date")
                .map(str::trim)
                .filter(|date| !date.is_empty())
                .map(str::to_string),
        }
    }

    fn provenance(&self) -> Vec<String> {
        let mut out = vec![format!("docx:{}:{}", self.kind, self.id)];
        if let Some(date) = &self.date {
            out.push(format!("docx-date:{date}"));
        }
        out
    }
}

fn source_author(value: Option<&str>) -> String {
    value
        .map(str::trim)
        .filter(|author| !author.is_empty())
        .unwrap_or("Unknown")
        .to_string()
}

struct InsertCapture {
    revision: Revision,
    anchor: StableId,
    content: Vec<Inline>,
}

struct DeleteCapture {
    revision: Revision,
    start: Option<StableId>,
    end: Option<StableId>,
}

struct FieldState {
    instruction: String,
    in_result: bool,
    pushed_link: bool,
}

enum Segment {
    Inline(Inline),
    /// Equation from a display-math paragraph (`m:oMathPara`).
    MathPara(Inline),
    PageBreak,
    Image {
        rel_id: String,
        alt: Option<String>,
    },
}

struct ParagraphState {
    kind: BlockKind,
    /// Run properties inherited from the paragraph style.
    style_props: RunProps,
    /// `true` for footnote/comment bodies: no blocks, anchors or images.
    nested: bool,
    segments: Vec<Segment>,
    fragment_ids: Vec<StableId>,
    fragment: usize,
    last_inline_id: Option<StableId>,
    fields: Vec<FieldState>,
    links: Vec<String>,
    inserts: Vec<InsertCapture>,
    deletes: Vec<DeleteCapture>,
}

impl ParagraphState {
    fn new(kind: BlockKind, style_props: RunProps, nested: bool) -> Self {
        Self {
            kind,
            style_props,
            nested,
            segments: Vec::new(),
            fragment_ids: vec![StableId::new("block")],
            fragment: 0,
            last_inline_id: None,
            fields: Vec::new(),
            links: Vec::new(),
            inserts: Vec::new(),
            deletes: Vec::new(),
        }
    }

    fn block_id(&self) -> StableId {
        self.fragment_ids[self.fragment].clone()
    }

    fn start_fragment(&mut self) {
        self.fragment_ids.push(StableId::new("block"));
        self.fragment = self.fragment_ids.len() - 1;
        self.last_inline_id = None;
    }

    fn in_field_instruction(&self) -> bool {
        self.fields.last().is_some_and(|field| !field.in_result)
    }
}

struct Converter<'a> {
    parts: &'a DocxParts,
    warnings: Vec<ModelWarning>,
    dropped: DroppedCounter,
    list_ids: BTreeMap<String, StableId>,
    note_ids: BTreeMap<(bool, String), StableId>,
    footnotes: Vec<Footnote>,
    comment_ranges: BTreeMap<String, CommentRange>,
    open_comment_ranges: Vec<String>,
    suggestions: Vec<Suggestion>,
    block_ids: BTreeSet<StableId>,
}

fn convert_parts(title: &str, parts: &DocxParts) -> Result<DocxImport, ImportError> {
    let mut converter = Converter {
        parts,
        warnings: Vec::new(),
        dropped: DroppedCounter::default(),
        list_ids: BTreeMap::new(),
        note_ids: BTreeMap::new(),
        footnotes: Vec::new(),
        comment_ranges: BTreeMap::new(),
        open_comment_ranges: Vec::new(),
        suggestions: Vec::new(),
        block_ids: BTreeSet::new(),
    };
    if let Some(footnotes) = &parts.footnotes {
        converter.import_notes(footnotes, false);
    }
    if let Some(endnotes) = &parts.endnotes {
        converter.import_notes(endnotes, true);
    }
    converter
        .dropped
        .count_n(DROPPED_HEADER_FOOTER, parts.header_footer_parts);

    let mut document = Document::new(title);
    let body = parts.document.child("body").unwrap_or(&parts.document);
    converter.walk_blocks(body, &mut document.blocks, 0);
    if document.blocks.is_empty() {
        return Err(ImportError::EmptyInput);
    }
    document.comments = converter.import_comments();
    document.footnotes = std::mem::take(&mut converter.footnotes);
    document.suggestions = std::mem::take(&mut converter.suggestions);
    let mut warnings = std::mem::take(&mut converter.warnings);
    warnings.extend(converter.dropped.into_warnings());
    document.warnings = warnings.clone();
    document
        .validate()
        .map_err(|err| ImportError::InvalidDocument(err.to_string()))?;

    let mut seen_hashes = BTreeSet::new();
    let blobs = parts
        .media
        .values()
        .filter(|blob| seen_hashes.insert(blob.hash.clone()))
        .cloned()
        .collect();
    Ok(DocxImport {
        document,
        warnings,
        blobs,
    })
}

fn inline_id(inline: &Inline) -> &StableId {
    match inline {
        Inline::Text { id, .. }
        | Inline::Link { id, .. }
        | Inline::Citation { id, .. }
        | Inline::FootnoteRef { id, .. }
        | Inline::Mention { id, .. }
        | Inline::Equation { id, .. } => id,
    }
}

fn inlines_have_source(inlines: &[Inline]) -> bool {
    inlines.iter().any(|inline| match inline {
        Inline::Text { text, .. } | Inline::Link { text, .. } => !text.trim().is_empty(),
        _ => true,
    })
}

fn math_source(element: &XmlElement) -> Option<String> {
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

fn equation_inline(source: String) -> Inline {
    Inline::Equation {
        id: StableId::new("equation"),
        equation: Equation {
            id: StableId::new("eq"),
            source_format: EquationSourceFormat::LatexLike,
            source,
        },
    }
}

fn hyperlink_field_target(instruction: &str) -> Option<String> {
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
    // -- warnings ---------------------------------------------------------

    fn count(&mut self, code: &'static str) {
        self.dropped.count(code);
    }

    // -- notes ------------------------------------------------------------

    fn import_notes(&mut self, part: &XmlElement, endnote: bool) {
        let element_name = if endnote { "endnote" } else { "footnote" };
        let mut imported = 0;
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
                id: stable_id,
                revision: 1,
                body,
                deleted: false,
            });
            imported += 1;
        }
        if endnote {
            self.dropped.count_n(ENDNOTES_AS_FOOTNOTES, imported);
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
            let mut state = ParagraphState::new(BlockKind::Paragraph, RunProps::default(), true);
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

    fn walk_blocks(&mut self, container: &XmlElement, out: &mut Vec<Block>, table_depth: usize) {
        for element in container.elements() {
            match element.local.as_str() {
                "p" => self.convert_paragraph(element, out),
                "tbl" => {
                    if table_depth > 0 {
                        self.count(NESTED_TABLE);
                    }
                    let block = self.convert_table(element, table_depth);
                    self.block_ids.insert(block.id.clone());
                    out.push(block);
                }
                "sectPr" => self.count(DROPPED_SECTION_PROPERTIES),
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

    fn convert_table(&mut self, table: &XmlElement, table_depth: usize) -> Block {
        let mut rows = Vec::new();
        let mut row_elements = Vec::new();
        collect_wrapped(table, "tr", &mut row_elements);
        for row in row_elements {
            let mut cells = Vec::new();
            let mut cell_elements = Vec::new();
            collect_wrapped(row, "tc", &mut cell_elements);
            for cell in cell_elements {
                if let Some(tc_pr) = cell.child("tcPr") {
                    if tc_pr.child("gridSpan").is_some() || tc_pr.child("vMerge").is_some() {
                        self.count(DROPPED_CELL_SPAN);
                    }
                }
                let mut blocks = Vec::new();
                self.walk_blocks(cell, &mut blocks, table_depth + 1);
                if blocks.is_empty() {
                    let block = Block::paragraph("");
                    self.block_ids.insert(block.id.clone());
                    blocks.push(block);
                }
                cells.push(TableCell {
                    id: StableId::new("cell"),
                    blocks,
                    properties: Vec::new(),
                });
            }
            if cells.is_empty() {
                cells.push(self.empty_cell());
            }
            rows.push(TableRow {
                id: StableId::new("row"),
                cells,
            });
        }
        if rows.is_empty() {
            rows.push(TableRow {
                id: StableId::new("row"),
                cells: vec![self.empty_cell()],
            });
        }
        Block {
            id: StableId::new("block"),
            kind: BlockKind::Table { rows },
            content: Vec::new(),
            properties: Vec::new(),
        }
    }

    fn empty_cell(&mut self) -> TableCell {
        let block = Block::paragraph("");
        self.block_ids.insert(block.id.clone());
        TableCell {
            id: StableId::new("cell"),
            blocks: vec![block],
            properties: Vec::new(),
        }
    }

    // -- paragraphs -------------------------------------------------------

    fn convert_paragraph(&mut self, paragraph: &XmlElement, out: &mut Vec<Block>) {
        let ppr = paragraph.child("pPr");
        let style = ppr
            .and_then(|ppr| ppr.child_val("pStyle"))
            .map(|id| self.parts.styles.resolve(id));
        let mut page_break_before = false;
        if let Some(ppr) = ppr {
            for property in ppr.elements() {
                match property.local.as_str() {
                    "jc" => self.count(DROPPED_ALIGNMENT),
                    "ind" => self.count(DROPPED_INDENT),
                    "spacing" => self.count(DROPPED_SPACING),
                    "pBdr" => self.count(DROPPED_PARAGRAPH_BORDER),
                    "shd" => self.count(DROPPED_PARAGRAPH_SHADING),
                    "tabs" => self.count(DROPPED_TABS),
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
            Some(HeadingStyle::Title) => {
                self.count(TITLE_STYLE_AS_HEADING);
                BlockKind::Heading { level: 1 }
            }
            Some(HeadingStyle::Subtitle) => {
                self.count(TITLE_STYLE_AS_HEADING);
                BlockKind::Heading { level: 2 }
            }
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
                    BlockKind::ListItem {
                        list_id,
                        level: num_pr.level.min(8),
                        ordered,
                    }
                }
                None => BlockKind::Paragraph,
            },
        };
        let style_props = style.map(|style| style.run_props).unwrap_or_default();

        if page_break_before {
            out.push(self.page_break_block());
        }
        let mut state = ParagraphState::new(kind, style_props, false);
        self.walk_paragraph_content(paragraph, &mut state);
        self.finish_paragraph(state, out);
    }

    fn page_break_block(&mut self) -> Block {
        let block = Block {
            id: StableId::new("block"),
            kind: BlockKind::PageBreak,
            content: Vec::new(),
            properties: Vec::new(),
        };
        self.block_ids.insert(block.id.clone());
        block
    }

    fn finish_paragraph(&mut self, state: ParagraphState, out: &mut Vec<Block>) {
        let ParagraphState {
            kind,
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
                    properties: Vec::new(),
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
                    self.flush_fragment(&kind, &fragment_ids, &mut fragment, &mut current, out);
                    let block = self.page_break_block();
                    out.push(block);
                }
                Segment::Image { rel_id, alt } => {
                    if text_count > 0 {
                        self.count(SPLIT_INLINE_IMAGE);
                    }
                    self.flush_fragment(&kind, &fragment_ids, &mut fragment, &mut current, out);
                    let block = self.image_block(&rel_id, alt);
                    out.push(block);
                }
            }
        }
        self.flush_fragment(&kind, &fragment_ids, &mut fragment, &mut current, out);
    }

    fn flush_fragment(
        &mut self,
        kind: &BlockKind,
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
            properties: Vec::new(),
        };
        self.block_ids.insert(block.id.clone());
        out.push(block);
    }

    fn image_block(&mut self, rel_id: &str, alt: Option<String>) -> Block {
        let block = match self.parts.media.get(rel_id) {
            Some(blob) => Block {
                id: StableId::new("block"),
                kind: BlockKind::Image {
                    blob_hash: blob.hash.clone(),
                    alt_text: alt.unwrap_or_else(|| blob.name.clone()),
                },
                content: Vec::new(),
                properties: Vec::new(),
            },
            None => {
                let target = self
                    .parts
                    .relationships
                    .get(rel_id)
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
            .and_then(|id| self.parts.relationships.get(id.trim()))
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
                    if !state.in_field_instruction() {
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
            }),
            Some("separate") => {
                if let Some(field) = state.fields.last_mut() {
                    field.in_result = true;
                    if let Some(href) = hyperlink_field_target(&field.instruction) {
                        field.pushed_link = true;
                        state.links.push(href);
                    }
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
        let alt = element.find_descendant("docPr").and_then(|doc_pr| {
            ["descr", "title", "name"]
                .iter()
                .find_map(|attr| doc_pr.attr(attr))
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        });
        self.emit_segment(state, Segment::Image { rel_id, alt });
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

/// Collects `local`-named descendants, looking through content wrappers such as
/// `w:sdt`/`w:sdtContent`/`w:customXml` but not into nested tables.
fn collect_wrapped<'a>(container: &'a XmlElement, local: &str, out: &mut Vec<&'a XmlElement>) {
    for element in container.elements() {
        if element.is(local) {
            out.push(element);
        } else if matches!(
            element.local.as_str(),
            "sdt" | "sdtContent" | "customXml" | "ins" | "del" | "moveTo" | "moveFrom"
        ) {
            collect_wrapped(element, local, out);
        }
    }
}

// ---------------------------------------------------------------------------
// Date parsing (ISO-8601 without external crates)
// ---------------------------------------------------------------------------

fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = (year - era * 400) as u64;
    let month_index = (month + 9) % 12;
    let day_of_year = (153 * month_index as u64 + 2) / 5 + day as u64 - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era as i64 - 719_468
}

fn parse_iso_datetime_ms(value: &str) -> Option<u64> {
    let value = value.trim();
    let (date, time) = value.split_once('T')?;
    let mut date_parts = date.split('-');
    let year: i64 = date_parts.next()?.parse().ok()?;
    let month: u32 = date_parts.next()?.parse().ok()?;
    let day: u32 = date_parts.next()?.parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let clock_end = time.find(['Z', '+', '-']).unwrap_or(time.len());
    let (clock, zone) = time.split_at(clock_end);
    let (clock, fraction) = match clock.split_once('.') {
        Some((clock, fraction)) => (clock, Some(fraction)),
        None => (clock, None),
    };
    let mut clock_parts = clock.split(':');
    let hour: i64 = clock_parts.next()?.parse().ok()?;
    let minute: i64 = clock_parts.next()?.parse().ok()?;
    let second: i64 = clock_parts.next().unwrap_or("0").parse().ok()?;
    let offset_seconds: i64 = match zone {
        "" | "Z" => 0,
        signed => {
            let sign = if signed.starts_with('-') { -1 } else { 1 };
            let mut parts = signed[1..].split(':');
            let hours: i64 = parts.next()?.parse().ok()?;
            let minutes: i64 = parts.next().unwrap_or("0").parse().ok()?;
            sign * (hours * 3600 + minutes * 60)
        }
    };
    let seconds = days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second
        - offset_seconds;
    if seconds < 0 {
        return None;
    }
    let millis = fraction
        .map(|fraction| {
            let digits: String = fraction.chars().take(3).collect();
            let padded = format!("{digits:0<3}");
            padded.parse::<u64>().unwrap_or(0)
        })
        .unwrap_or(0);
    Some(seconds as u64 * 1000 + millis)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_relationship_targets_relative_to_the_source_part() {
        assert_eq!(
            resolve_part_path("word", "media/image1.png").as_deref(),
            Some("word/media/image1.png")
        );
        assert_eq!(
            resolve_part_path("word", "/word/media/image1.png").as_deref(),
            Some("word/media/image1.png")
        );
        assert_eq!(
            resolve_part_path("word", "../customXml/item1.xml").as_deref(),
            Some("customXml/item1.xml")
        );
        assert_eq!(resolve_part_path("word", "../../etc/passwd"), None);
        assert_eq!(resolve_part_path("word", "https://example.invalid/x"), None);
    }

    #[test]
    fn parses_iso_dates_into_unix_milliseconds() {
        assert_eq!(parse_iso_datetime_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(
            parse_iso_datetime_ms("2024-01-15T10:30:00Z"),
            Some(1_705_314_600_000)
        );
        assert_eq!(
            parse_iso_datetime_ms("2024-01-15T11:30:00.250+01:00"),
            Some(1_705_314_600_250)
        );
        assert_eq!(parse_iso_datetime_ms("not a date"), None);
    }

    #[test]
    fn parses_hyperlink_field_instructions() {
        assert_eq!(
            hyperlink_field_target(r#" HYPERLINK "https://example.invalid/a" \o "tip" "#)
                .as_deref(),
            Some("https://example.invalid/a")
        );
        assert_eq!(
            hyperlink_field_target(r#" HYPERLINK \l "Bookmark1" "#).as_deref(),
            Some("#Bookmark1")
        );
        assert_eq!(hyperlink_field_target(" PAGE "), None);
    }

    #[test]
    fn detects_heading_styles_from_ids_names_and_outline_levels() {
        assert_eq!(
            detect_heading("Heading3", "heading 3", Some(2)),
            Some(HeadingStyle::Level(3))
        );
        assert_eq!(
            detect_heading("berschrift1", "heading 1", None),
            Some(HeadingStyle::Level(1))
        );
        assert_eq!(
            detect_heading("Title", "Title", Some(0)),
            Some(HeadingStyle::Title)
        );
        assert_eq!(
            detect_heading("Custom", "My Style", Some(1)),
            Some(HeadingStyle::Level(2))
        );
        assert_eq!(detect_heading("Normal", "Normal", None), None);
        assert_eq!(detect_heading("Normal", "Normal", Some(9)), None);
    }
}
