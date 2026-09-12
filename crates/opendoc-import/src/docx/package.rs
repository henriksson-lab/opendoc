//! Opening the DOCX zip package and resolving its parts and relationships.

use crate::docx::styles::{Numbering, Styles};
use crate::xml::{parse_xml_bytes, XmlElement};
use crate::{ImportError, ImportedBlob};
use std::collections::BTreeMap;
use std::io::Cursor;
use std::io::Read;

pub(super) struct Package {
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
pub(super) struct Relationship {
    id: String,
    rel_type: String,
    pub(super) target: String,
    external: bool,
}

pub(super) fn parse_relationships(bytes: &[u8]) -> Vec<Relationship> {
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
pub(super) fn resolve_part_path(base_dir: &str, target: &str) -> Option<String> {
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

pub(super) fn media_name(path: &str) -> String {
    path.rsplit('/')
        .next()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("docx-image")
        .to_string()
}

pub(super) fn media_type(path: &str) -> &'static str {
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

pub(super) fn relationship_is_image(rel: &Relationship) -> bool {
    rel.rel_type.ends_with("/image")
        || !matches!(
            media_type(&rel.target),
            "application/octet-stream" | "application/xml"
        )
}

// ---------------------------------------------------------------------------
// Loaded parts
// ---------------------------------------------------------------------------

pub(super) struct DocxParts {
    pub(super) document: XmlElement,
    pub(super) relationships: BTreeMap<String, Relationship>,
    pub(super) styles: Styles,
    pub(super) numbering: Numbering,
    pub(super) footnotes: Option<XmlElement>,
    pub(super) endnotes: Option<XmlElement>,
    pub(super) comments: Option<XmlElement>,
    pub(super) comments_extended: Option<XmlElement>,
    pub(super) media: BTreeMap<String, ImportedBlob>,
    /// Header and footer parts, keyed by the relationship id the section
    /// properties refer to them by. Loaded eagerly because which one is
    /// *used* is decided by `w:sectPr`, which is read later.
    pub(super) furniture_parts: BTreeMap<String, XmlElement>,
}

impl DocxParts {
    pub(super) fn from_raw_document(document: XmlElement) -> Self {
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
            furniture_parts: BTreeMap::new(),
        }
    }

    pub(super) fn from_package(bytes: &[u8]) -> Result<Self, ImportError> {
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
                    if let Some(part) = package.xml_part(path) {
                        parts.furniture_parts.insert(rel.id.clone(), part);
                    }
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
