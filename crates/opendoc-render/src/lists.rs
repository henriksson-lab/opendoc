//! List markers and the nesting writer that opens and closes list levels.

use crate::*;

/// Which wrapper a list item wants. Bullets and checklists are both `<ul>`,
/// but a checklist may not be folded into a plain bulleted list: they are
/// different markers, and a run that changes marker is a new list.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ListMarker {
    Bullet,
    Ordered,
    Checklist,
}

impl ListMarker {
    pub(crate) fn of(kind: ListKind) -> Self {
        match kind {
            ListKind::Bullet => ListMarker::Bullet,
            ListKind::Ordered => ListMarker::Ordered,
            ListKind::Checklist { .. } => ListMarker::Checklist,
        }
    }

    fn tag(self) -> &'static str {
        match self {
            ListMarker::Ordered => "ol",
            ListMarker::Bullet | ListMarker::Checklist => "ul",
        }
    }

    fn class(self) -> &'static str {
        match self {
            ListMarker::Bullet | ListMarker::Ordered => "doc-list",
            ListMarker::Checklist => "doc-list doc-checklist",
        }
    }

    fn is_ordered(self) -> bool {
        matches!(self, ListMarker::Ordered)
    }
}

/// Emits nested `<ol>`/`<ul>` structure for consecutive list items and
/// numbers ordered items per list and level (restarting deeper levels).
#[derive(Default)]
pub(crate) struct ListWriter {
    stack: Vec<ListLevel>,
    counters: BTreeMap<(String, u8), usize>,
}

pub(crate) struct ListLevel {
    level: u8,
    marker: ListMarker,
    item_open: bool,
}

impl ListWriter {
    pub(crate) fn open_item(
        &mut self,
        out: &mut String,
        list_id: &StableId,
        level: u8,
        marker: ListMarker,
    ) -> Option<usize> {
        // Close deeper levels and mismatched lists at this level.
        while let Some(top) = self.stack.last() {
            if top.level > level || (top.level == level && top.marker != marker) {
                self.close_top(out);
            } else {
                break;
            }
        }
        // Open lists up to the requested level.
        while self
            .stack
            .last()
            .map(|top| top.level < level)
            .unwrap_or(true)
        {
            let next_level = match self.stack.last() {
                Some(top) => top.level + 1,
                None => level,
            };
            let _ = write!(
                out,
                "<{} class=\"{} depth-{next_level}\" data-level=\"{next_level}\">",
                marker.tag(),
                marker.class()
            );
            self.stack.push(ListLevel {
                level: next_level,
                marker,
                item_open: false,
            });
        }
        if let Some(top) = self.stack.last_mut() {
            if top.item_open {
                out.push_str("</li>");
                top.item_open = false;
            }
        }
        // Reset counters for deeper levels of this list.
        let deeper: Vec<(String, u8)> = self
            .counters
            .keys()
            .filter(|(id, item_level)| id == list_id.as_str() && *item_level > level)
            .cloned()
            .collect();
        for key in deeper {
            self.counters.remove(&key);
        }
        let counter = self
            .counters
            .entry((list_id.to_string(), level))
            .or_insert(0);
        *counter += 1;
        let value = *counter;
        // The li tag itself is written by the caller through
        // render_block_attributes with the numbering value.
        marker.is_ordered().then_some(value)
    }

    pub(crate) fn item_content_written(&mut self) {
        if let Some(top) = self.stack.last_mut() {
            top.item_open = true;
        }
    }

    fn close_top(&mut self, out: &mut String) {
        if let Some(top) = self.stack.pop() {
            if top.item_open {
                out.push_str("</li>");
            }
            let _ = write!(out, "</{}>", top.marker.tag());
        }
    }

    pub(crate) fn close_all(&mut self, out: &mut String) {
        while !self.stack.is_empty() {
            self.close_top(out);
        }
        self.counters.clear();
    }
}
