//! Style inheritance, heading detection and numbering definitions.

use crate::docx::props::{overlay_para_props, parse_para_props, parse_run_props, RunProps};
use crate::xml::XmlElement;
use opendoc_core::BlockProperties;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum HeadingStyle {
    Title,
    Subtitle,
    Level(u8),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct NumPr {
    pub(super) num_id: String,
    pub(super) level: u8,
}

pub(super) fn parse_num_pr(num_pr: &XmlElement) -> Option<NumPr> {
    let num_id = num_pr.child_val("numId")?.trim().to_string();
    let level = num_pr
        .child_val("ilvl")
        .and_then(|value| value.trim().parse::<u8>().ok())
        .unwrap_or(0)
        .min(8);
    Some(NumPr { num_id, level })
}

#[derive(Clone, Debug, Default)]
pub(super) struct StyleRecord {
    based_on: Option<String>,
    heading: Option<HeadingStyle>,
    num_pr: Option<NumPr>,
    run_props: RunProps,
    para_props: BlockProperties,
    para_dropped: Vec<&'static str>,
}

#[derive(Clone, Debug, Default)]
pub(super) struct Styles {
    by_id: BTreeMap<String, StyleRecord>,
}

#[derive(Clone, Debug, Default)]
pub(super) struct ResolvedStyle {
    pub(super) heading: Option<HeadingStyle>,
    pub(super) num_pr: Option<NumPr>,
    pub(super) run_props: RunProps,
    pub(super) para_props: BlockProperties,
    pub(super) para_dropped: Vec<&'static str>,
}

pub(super) fn detect_heading(
    style_id: &str,
    name: &str,
    outline_level: Option<u8>,
) -> Option<HeadingStyle> {
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
    pub(super) fn parse(root: XmlElement) -> Self {
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
            let para = ppr.map(parse_para_props).unwrap_or_default();
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
                    para_props: para.props,
                    para_dropped: para.dropped,
                },
            );
        }
        Self { by_id }
    }

    pub(super) fn resolve(&self, style_id: &str) -> ResolvedStyle {
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
                para_props: BlockProperties::default(),
                para_dropped: Vec::new(),
            };
        }
        let mut run_props = RunProps::default();
        let mut para_props = BlockProperties::default();
        let mut para_dropped: Vec<&'static str> = Vec::new();
        for record in chain.iter().rev() {
            run_props.overlay(&record.run_props);
            overlay_para_props(&mut para_props, &record.para_props);
            para_dropped.extend(record.para_dropped.iter().copied());
        }
        // One warning per affected paragraph, not one per inherited style that
        // happened to repeat the same unrepresentable property.
        para_dropped.sort_unstable();
        para_dropped.dedup();
        ResolvedStyle {
            heading: chain.iter().find_map(|record| record.heading),
            num_pr: chain.iter().find_map(|record| record.num_pr.clone()),
            run_props,
            para_props,
            para_dropped,
        }
    }
}

// ---------------------------------------------------------------------------
// Numbering
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default)]
pub(super) struct AbstractNum {
    formats: BTreeMap<u8, String>,
    style_link: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub(super) struct NumInstance {
    abstract_id: String,
    overrides: BTreeMap<u8, String>,
}

#[derive(Clone, Debug, Default)]
pub(super) struct Numbering {
    abstracts: BTreeMap<String, AbstractNum>,
    nums: BTreeMap<String, NumInstance>,
}

pub(super) fn level_formats(container: &XmlElement) -> BTreeMap<u8, String> {
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
    pub(super) fn parse(root: XmlElement) -> Self {
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
    pub(super) fn is_ordered(&self, styles: &Styles, num_id: &str, level: u8) -> Option<bool> {
        self.level_format(styles, num_id, level)
            .map(|format| !matches!(format.as_str(), "bullet" | "none" | ""))
    }
}

// ---------------------------------------------------------------------------
// Warning bookkeeping
// ---------------------------------------------------------------------------
