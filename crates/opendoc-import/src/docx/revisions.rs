//! Tracked changes, comment ranges and the per-paragraph capture state.

use crate::docx::props::RunProps;
use crate::xml::XmlElement;
use opendoc_core::{BlockKind, BlockProperties, ImageLayout, Inline, StableId};

#[derive(Clone, Debug, Default)]
pub(super) struct CommentRange {
    pub(super) start: Option<StableId>,
    pub(super) end: Option<StableId>,
    pub(super) block_id: Option<StableId>,
}

#[derive(Clone, Debug)]
pub(super) struct Revision {
    pub(super) kind: &'static str,
    pub(super) id: String,
    pub(super) author: String,
    pub(super) date: Option<String>,
}

impl Revision {
    pub(super) fn from_element(kind: &'static str, element: &XmlElement) -> Self {
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

    pub(super) fn provenance(&self) -> Vec<String> {
        let mut out = vec![format!("docx:{}:{}", self.kind, self.id)];
        if let Some(date) = &self.date {
            out.push(format!("docx-date:{date}"));
        }
        out
    }
}

pub(super) fn source_author(value: Option<&str>) -> String {
    value
        .map(str::trim)
        .filter(|author| !author.is_empty())
        .unwrap_or("Unknown")
        .to_string()
}

pub(super) struct InsertCapture {
    pub(super) revision: Revision,
    pub(super) anchor: StableId,
    pub(super) content: Vec<Inline>,
}

pub(super) struct DeleteCapture {
    pub(super) revision: Revision,
    pub(super) start: Option<StableId>,
    pub(super) end: Option<StableId>,
}

pub(super) struct FieldState {
    pub(super) instruction: String,
    pub(super) in_result: bool,
    pub(super) pushed_link: bool,
    /// `true` for a field OpenDoc models as a field of its own (a page
    /// number). Word caches the last computed result between `separate` and
    /// `end`; importing that cached text would turn a field into a frozen
    /// number, so the result is swallowed.
    pub(super) suppressed_result: bool,
}

pub(super) enum Segment {
    Inline(Inline),
    /// Equation from a display-math paragraph (`m:oMathPara`).
    MathPara(Inline),
    PageBreak,
    Image {
        rel_id: String,
        alt: Option<String>,
        layout: ImageLayout,
    },
}

pub(super) struct ParagraphState {
    pub(super) kind: BlockKind,
    /// Block-level formatting, already resolved through the style chain.
    pub(super) properties: BlockProperties,
    /// Run properties inherited from the paragraph style.
    pub(super) style_props: RunProps,
    /// `true` for footnote/comment bodies: no blocks, anchors or images.
    pub(super) nested: bool,
    pub(super) segments: Vec<Segment>,
    pub(super) fragment_ids: Vec<StableId>,
    pub(super) fragment: usize,
    pub(super) last_inline_id: Option<StableId>,
    pub(super) fields: Vec<FieldState>,
    pub(super) links: Vec<String>,
    pub(super) inserts: Vec<InsertCapture>,
    pub(super) deletes: Vec<DeleteCapture>,
}

impl ParagraphState {
    pub(super) fn new(
        kind: BlockKind,
        properties: BlockProperties,
        style_props: RunProps,
        nested: bool,
    ) -> Self {
        Self {
            kind,
            properties,
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

    pub(super) fn block_id(&self) -> StableId {
        self.fragment_ids[self.fragment].clone()
    }

    pub(super) fn start_fragment(&mut self) {
        self.fragment_ids.push(StableId::new("block"));
        self.fragment = self.fragment_ids.len() - 1;
        self.last_inline_id = None;
    }

    pub(super) fn in_field_instruction(&self) -> bool {
        self.fields.last().is_some_and(|field| !field.in_result)
    }

    /// Inside the cached result of a field whose value OpenDoc computes.
    pub(super) fn in_suppressed_field_result(&self) -> bool {
        self.fields.iter().any(|field| field.suppressed_result)
    }
}
