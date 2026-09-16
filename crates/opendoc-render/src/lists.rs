//! The nesting writer that turns list structure into `<ol>`/`<ul>` markup.
//!
//! The *rule* — which wrapper an item joins, which ones it ends, and what
//! number it takes inside the one it lands in — is not here. It is
//! [`opendoc_layout::lists`], because the painted page has to number the same
//! items the same way and a second copy of the rule is how the two came to
//! disagree. This file is the half that is genuinely about HTML: which tag,
//! which class, and where the `</li>` goes.

use crate::*;
use opendoc_core::{BulletListMarker, OrderedListFormat};
use opendoc_layout::lists::{ListEdge, ListNumbering};

pub(crate) use opendoc_layout::lists::ListMarker;

/// Emits nested `<ol>`/`<ul>` structure for consecutive list items, numbering
/// each ordered item by its position in the wrapper that holds it.
#[derive(Default)]
pub(crate) struct ListWriter {
    numbering: ListNumbering,
    /// One flag per open wrapper: whether the item it is holding still needs
    /// its `</li>`. Kept here rather than in the numbering because an
    /// unclosed tag is a fact about markup and nothing else has one.
    item_open: Vec<bool>,
}

/// What [`ListWriter::open_item`] did, for a caller that needs more than the
/// numbering.
pub(crate) struct OpenedItem {
    /// The number to put on an ordered item, or `None` for a marker that has
    /// no number.
    pub(crate) value: Option<u32>,
    /// Where in `out` a new *outermost* list began, when this item started
    /// one. A caller recording the body's top-level elements needs the offset
    /// rather than a flag: a run whose marker changes closes its outermost
    /// list and opens the next one inside a single call, so the boundary
    /// between the two elements is a position in the middle of what this call
    /// wrote and cannot be observed from outside.
    pub(crate) root_start: Option<usize>,
}

/// Resolves the document-level style settings for a particular list level.
/// Grouping these related lookups keeps the nesting writer concerned only
/// with markup rather than with how a document stores its list properties.
pub(crate) struct ListStyleResolver<F, G> {
    pub(crate) ordered_format_for: F,
    pub(crate) bullet_marker_for: G,
}

impl ListWriter {
    pub(crate) fn open_item<F, G>(
        &mut self,
        out: &mut String,
        list_id: &StableId,
        level: u8,
        marker: ListMarker,
        start: u32,
        styles: ListStyleResolver<F, G>,
    ) -> OpenedItem
    where
        F: Fn(u8) -> OrderedListFormat,
        G: Fn(u8) -> BulletListMarker,
    {
        let mut root_start = None;
        let item_open = &mut self.item_open;
        let number =
            self.numbering
                .open_item_with_start(list_id, level, marker, start, &mut |edge| match edge {
                    ListEdge::Closed { list, .. } => {
                        if item_open.pop() == Some(true) {
                            out.push_str("</li>");
                        }
                        let _ = write!(out, "</{}>", list.marker.tag());
                    }
                    ListEdge::Opened { list, root } => {
                        // The offset is taken before the tag is written, so a
                        // recorded span starts at the `<`.
                        if root {
                            root_start = Some(out.len());
                        }
                        let start = if list.marker.is_ordered() && list.start() != 1 {
                            format!(" start=\"{}\"", list.start())
                        } else {
                            String::new()
                        };
                        let format = (styles.ordered_format_for)(list.level);
                        let style = match list.marker {
                            ListMarker::Ordered
                                if format != OrderedListFormat::inherited_at(list.level) =>
                            {
                                format!(" style=\"list-style-type: {}\"", format.css_name())
                            }
                            ListMarker::Bullet
                                if (styles.bullet_marker_for)(list.level)
                                    != BulletListMarker::inherited_at(list.level) =>
                            {
                                bullet_css_style(&(styles.bullet_marker_for)(list.level))
                            }
                            _ => String::new(),
                        };
                        let _ = write!(
                            out,
                            "<{} class=\"{} depth-{}\" data-level=\"{}\"{}{}>",
                            list.marker.tag(),
                            list.marker.class(),
                            list.level,
                            list.level,
                            start,
                            style,
                        );
                        item_open.push(false);
                    }
                });
        // The previous sibling in this wrapper, if any, ends where this item
        // begins. A wrapper that was just opened has no sibling to close.
        if let Some(open) = item_open.last_mut() {
            if *open {
                out.push_str("</li>");
                *open = false;
            }
        }
        // The li tag itself is written by the caller through
        // render_block_attributes with the numbering value.
        OpenedItem {
            value: number.value(),
            root_start,
        }
    }

    /// True while no list is open.
    pub(crate) fn is_empty(&self) -> bool {
        self.numbering.is_empty()
    }

    pub(crate) fn item_content_written(&mut self) {
        if let Some(open) = self.item_open.last_mut() {
            *open = true;
        }
    }

    pub(crate) fn close_all(&mut self, out: &mut String) {
        let item_open = &mut self.item_open;
        self.numbering.close_all(&mut |edge| {
            if let ListEdge::Closed { list, .. } = edge {
                if item_open.pop() == Some(true) {
                    out.push_str("</li>");
                }
                let _ = write!(out, "</{}>", list.marker.tag());
            }
        });
    }
}

/// Escape every custom-marker code point for CSS, so a document marker can
/// never become markup or another declaration in this style attribute.
fn bullet_css_style(marker: &BulletListMarker) -> String {
    match marker.css_name() {
        Some(name) => format!(" style=\"list-style-type: {name}\""),
        None => {
            let literal = marker
                .glyph()
                .chars()
                .map(|ch| format!("\\{:x} ", ch as u32))
                .collect::<String>();
            format!(" style=\"list-style-type: &quot;{literal}&quot;\"")
        }
    }
}

#[cfg(test)]
#[path = "lists_tests.rs"]
mod lists_tests;
