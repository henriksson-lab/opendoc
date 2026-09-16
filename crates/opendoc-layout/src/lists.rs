//! List structure, numbering and marker selection — the one implementation.
//!
//! A list item's number is drawn twice: once by the browser, from the
//! `<ol>`/`<ul>` structure `opendoc-render` writes, and once on paper, by
//! [`crate::paint`], which has no browser to count for it. Those two used to
//! be separate pieces of code that were kept in step by hand, and they had
//! drifted: the renderer counted per `(list_id, level)` and never restarted a
//! reopened `<ol>`, so a bulleted run followed by an ordered one at the same
//! level was numbered 3, 4 rather than 1, 2. The layout reproduced that so
//! the PDF would match the screen, which made the disagreement invisible
//! rather than absent.
//!
//! So the rule lives here, once, and both callers drive it:
//! [`ListNumbering`] owns the wrapper stack and the ordinal inside each open
//! wrapper, and reports what it did through [`ListEdge`] so a caller that
//! emits markup can emit it.
//!
//! ## The rule, and where it comes from
//!
//! **An ordinal belongs to a wrapper, not to a list or a level.** Opening a
//! wrapper starts its ordinal at zero; every item directly inside it takes
//! the next one; closing it discards the count. That is not a choice — it is
//! what `<ol>` means, measured rather than reasoned about. Chrome 147 drawing
//! the markup `opendoc-render` emits, read out of Blink's accessibility tree
//! (a list item's marker box is an `AXListMarker` whose name is the string
//! Chrome painted):
//!
//! ```text
//! <ol class="doc-list depth-0"><li>one<li>two<li>three</ol>
//!     -> "1." "2." "3."
//! <ul class="doc-list depth-0"><li>b1<li>b2</ul>
//! <ol class="doc-list depth-0"><li>o1<li>o2</ol>
//!     -> "•" "•" "1." "2."          <- a reopened <ol> restarts
//! <ol class="doc-list depth-0"><li>one
//!   <ol class="doc-list depth-1"><li>inner one<li>inner two</ol></li>
//!   <li>two</ol>
//!     -> "1." "a." "b." "2."        <- nested restarts, outer carries on
//! ```
//!
//! Two consequences fall out of that and need no separate rule. A bulleted
//! item cannot consume an ordinal, because a wrapper holds one marker only —
//! a marker change *is* a new wrapper. And a list's identity does not number
//! anything: it only decides, exactly as the marker does, whether an item
//! joins the wrapper already open or starts the next one.
//!
//! ## Marker selection
//!
//! The glyph is CSS on screen (`list-style-type` per `depth-N`) and a drawn
//! string on paper. [`list_style_type`] is the single statement of the cycle:
//! the exported stylesheet's rules are generated from it by
//! [`list_style_type_rules`], and [`ordered_marker`] and [`bullet_glyph`]
//! produce the same sequence for the painted page. Measured the same way:
//!
//! ```text
//! depth 0,1,2,3 ordered -> "1." "a." "b." "i." "ii." "1." "2."
//! depth 0,1,2   bullet  -> "•"  "◦"  (circle) and square below that
//! depth 9 (past the last rule the stylesheet states) -> "1." "2."
//! ```

use opendoc_core::{ListKind, OrderedListFormat, StableId};

/// Which wrapper a list item wants. Bullets and checklists are both `<ul>`,
/// but a checklist may not be folded into a plain bulleted list: they are
/// different markers, and a run that changes marker is a new list.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ListMarker {
    Bullet,
    Ordered,
    Checklist,
}

impl ListMarker {
    pub fn of(kind: ListKind) -> Self {
        match kind {
            ListKind::Bullet => ListMarker::Bullet,
            ListKind::Ordered => ListMarker::Ordered,
            ListKind::Checklist { .. } => ListMarker::Checklist,
        }
    }

    /// The element name that wraps items with this marker.
    pub fn tag(self) -> &'static str {
        match self {
            ListMarker::Ordered => "ol",
            ListMarker::Bullet | ListMarker::Checklist => "ul",
        }
    }

    /// The class the wrapper carries, which is what the stylesheet's indent
    /// and marker rules select on.
    pub fn class(self) -> &'static str {
        match self {
            ListMarker::Bullet | ListMarker::Ordered => "doc-list",
            ListMarker::Checklist => "doc-list doc-checklist",
        }
    }

    /// True when an item under this marker is numbered rather than bulleted.
    pub fn is_ordered(self) -> bool {
        matches!(self, ListMarker::Ordered)
    }
}

/// How many nesting depths the stylesheet states a `list-style-type` for.
///
/// CSS cannot say "cycle for ever": the exported stylesheet and the app's own
/// write one rule per `depth-N`, and past the last one the browser falls back
/// to the initial `disc`/`decimal` — measured: a `depth-9` `<ol>` draws "1."
/// "2.". So this crate stops cycling at exactly the same depth — a marker
/// drawn on paper that the screen does not draw is a disagreement like any
/// other, and it is the kind nobody notices until a deeply nested list prints
/// wrong.
pub const STYLED_LIST_DEPTHS: usize = 9;

/// Where in the disc/circle/square — decimal/alpha/roman cycle this depth
/// sits, or the cycle's first entry once the stylesheet has stopped stating
/// one.
pub fn marker_cycle(depth: usize) -> usize {
    if depth < STYLED_LIST_DEPTHS {
        depth % 3
    } else {
        0
    }
}

/// The `list-style-type` keyword for a wrapper of this marker at this depth.
///
/// The single statement of the cycle: the stylesheet rules and the painted
/// glyphs are both derived from it, so the paper and the screen cannot pick
/// different markers.
pub fn list_style_type(marker: ListMarker, depth: usize) -> &'static str {
    match (marker, marker_cycle(depth)) {
        (ListMarker::Checklist, _) => "none",
        (ListMarker::Bullet, 0) => "disc",
        (ListMarker::Bullet, 1) => "circle",
        (ListMarker::Bullet, _) => "square",
        (ListMarker::Ordered, 0) => "decimal",
        (ListMarker::Ordered, 1) => "lower-alpha",
        (ListMarker::Ordered, _) => "lower-roman",
    }
}

/// The stylesheet rules that state the cycle, written from
/// [`list_style_type`] rather than beside it.
///
/// `prefix` is what the rules are scoped to (`".doc-body "`). Depth 0 is
/// omitted on purpose: `disc` and `decimal` are the initial values, so a rule
/// for it would state what the browser already does.
pub fn list_style_type_rules(prefix: &str) -> String {
    let mut css = String::new();
    for marker in [ListMarker::Bullet, ListMarker::Ordered] {
        for cycle in 1..3 {
            let depths: Vec<usize> = (0..STYLED_LIST_DEPTHS)
                .filter(|depth| marker_cycle(*depth) == cycle)
                .collect();
            if depths.is_empty() {
                continue;
            }
            let selectors: Vec<String> = depths
                .iter()
                .map(|depth| format!("{prefix}{}.depth-{depth}", marker.tag()))
                .collect();
            css.push_str(&format!(
                "{} {{ list-style-type: {}; }}\n",
                selectors.join(", "),
                list_style_type(marker, depths[0])
            ));
        }
    }
    css
}

/// The marker text an ordered item at this depth carries, without its
/// trailing separator: decimal, then lower-alpha, then lower-roman, as
/// [`list_style_type`] says.
pub fn ordered_marker(depth: usize, ordinal: u32) -> String {
    ordered_marker_format(OrderedListFormat::inherited_at(depth as u8), ordinal)
}

/// The marker text for an explicit list-run format. This is also the source
/// of truth for PDF painting; HTML receives the matching CSS keyword.
pub fn ordered_marker_format(format: OrderedListFormat, ordinal: u32) -> String {
    match format {
        OrderedListFormat::Decimal => ordinal.to_string(),
        OrderedListFormat::LowerAlpha => lower_alpha(ordinal),
        OrderedListFormat::UpperAlpha => lower_alpha(ordinal).to_ascii_uppercase(),
        OrderedListFormat::LowerRoman => lower_roman(ordinal),
        OrderedListFormat::UpperRoman => lower_roman(ordinal).to_ascii_uppercase(),
    }
}

/// The character CSS draws for a bulleted item at this depth, and the
/// character this crate paints there.
///
/// It is one function because it has to be one glyph: the paper drawing a
/// disc where the screen draws a hollow circle is a disagreement of exactly
/// the kind this crate exists to prevent. The engine painted the disc for
/// `circle` until the bundled subset carried U+25E6 —
/// `the_subset_carries_every_glyph_the_bullet_cycle_asks_for` in `font` is
/// what keeps it carrying all three.
pub fn bullet_glyph(depth: usize) -> char {
    match list_style_type(ListMarker::Bullet, depth) {
        "disc" => '\u{2022}',
        "circle" => '\u{25E6}',
        _ => '\u{25A0}',
    }
}

/// `a`, `b`, … `z`, `aa`, `ab`: the bijective base-26 sequence CSS uses.
pub(crate) fn lower_alpha(ordinal: u32) -> String {
    let mut value = ordinal.max(1);
    let mut out = Vec::new();
    while value > 0 {
        let digit = (value - 1) % 26;
        out.push(b'a' + digit as u8);
        value = (value - 1) / 26;
    }
    out.reverse();
    String::from_utf8(out).expect("ascii")
}

pub(crate) fn lower_roman(ordinal: u32) -> String {
    // CSS falls back to decimal outside the representable range rather than
    // inventing notation, and so does this.
    if !(1..4_000).contains(&ordinal) {
        return ordinal.to_string();
    }
    const DIGITS: [(u32, &str); 13] = [
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ];
    let mut value = ordinal;
    let mut out = String::new();
    for (amount, numeral) in DIGITS {
        while value >= amount {
            out.push_str(numeral);
            value -= amount;
        }
    }
    out
}

/// One open wrapper: an `<ol>` or `<ul>` element on screen, and a level of
/// indent on paper.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenList {
    /// The `depth-N` the wrapper carries. Not the position in the stack: a
    /// run that starts at level 2 opens one wrapper and the stylesheet styles
    /// it as `depth-2`.
    pub level: u8,
    pub marker: ListMarker,
    /// The list this wrapper belongs to. Part of the wrapper's identity, so
    /// two adjacent lists are two elements rather than one element whose
    /// numbering restarts halfway through it.
    pub list_id: StableId,
    ordinal: u32,
}

impl OpenList {
    /// The ordinal the wrapper was opened to display.  It is queried only
    /// while processing [`ListEdge::Opened`], before an item increments it.
    pub fn start(&self) -> u32 {
        self.ordinal.saturating_add(1)
    }
    /// How many items this wrapper holds so far.
    pub fn ordinal(&self) -> u32 {
        self.ordinal
    }
}

/// What placing an item did to the wrapper stack, reported as it happens so a
/// caller writing markup can write the tags in order.
#[derive(Debug)]
pub enum ListEdge<'a> {
    /// A wrapper ended. `root` is true when it was the outermost one, which
    /// is the only case that costs vertical space: nested `.doc-list`s carry
    /// no bottom margin and the outer one does.
    Closed { list: &'a OpenList, root: bool },
    /// A wrapper began, with its ordinal at zero.
    Opened { list: &'a OpenList, root: bool },
}

/// Where one item landed: which wrapper it is in, and its number in it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ListItemNumber {
    /// The `depth-N` of the wrapper holding the item.
    pub level: u8,
    pub marker: ListMarker,
    /// The item's position in its wrapper, counting from 1.
    pub ordinal: u32,
    /// True when the outermost wrapper was closed and another opened in its
    /// place — a marker or list change at the top level.
    pub closed_root: bool,
}

impl ListItemNumber {
    /// The number to show, or `None` for a marker that has no number.
    pub fn value(self) -> Option<u32> {
        self.marker.is_ordered().then_some(self.ordinal)
    }
}

/// The wrapper stack and the ordinals inside it.
///
/// Drive it with one [`ListNumbering::open_item`] per list item in document
/// order, and [`ListNumbering::close_all`] when a non-list block ends the run.
#[derive(Debug, Default)]
pub struct ListNumbering {
    stack: Vec<OpenList>,
}

impl ListNumbering {
    /// True while no list is open.
    pub fn is_empty(&self) -> bool {
        self.stack.is_empty()
    }

    /// The wrappers currently open, outermost first. An item's indent is a
    /// sum over these, because each one contributes its own padding.
    pub fn open_levels(&self) -> &[OpenList] {
        &self.stack
    }

    /// Places one list item: closes the wrappers it ends, opens the ones it
    /// begins, and takes the next ordinal in the wrapper it lands in.
    pub fn open_item(
        &mut self,
        list_id: &StableId,
        level: u8,
        marker: ListMarker,
        edge: &mut dyn FnMut(ListEdge<'_>),
    ) -> ListItemNumber {
        self.open_item_with_start(list_id, level, marker, 1, edge)
    }

    /// As [`Self::open_item`], with the first ordinal of a newly opened
    /// wrapper.  The start is list-level source state; callers that have no
    /// such source continue to get ordinary HTML's `1`.
    pub fn open_item_with_start(
        &mut self,
        list_id: &StableId,
        level: u8,
        marker: ListMarker,
        start: u32,
        edge: &mut dyn FnMut(ListEdge<'_>),
    ) -> ListItemNumber {
        let had_root = !self.stack.is_empty();
        // Close deeper levels, and any level whose wrapper this item cannot
        // join: a different marker or a different list is a different element.
        while let Some(ends) = self.stack.last().map(|top| {
            top.level > level
                || (top.level == level && (top.marker != marker || &top.list_id != list_id))
        }) {
            if !ends {
                break;
            }
            let closed = self.stack.pop().expect("a level was there to end");
            let root = self.stack.is_empty();
            edge(ListEdge::Closed {
                list: &closed,
                root,
            });
        }
        let closed_root = had_root && self.stack.is_empty();
        // Open wrappers up to the requested level. A run that starts deep
        // opens one wrapper at its own level; a run that descends opens one
        // per level it passes through.
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
            let root = self.stack.is_empty();
            self.stack.push(OpenList {
                level: next_level,
                marker,
                list_id: list_id.clone(),
                // `start` is validated by the model, but keeping this total
                // means a hand-built caller cannot underflow the counter.
                ordinal: start.saturating_sub(1),
            });
            edge(ListEdge::Opened {
                list: self.stack.last().expect("just pushed"),
                root,
            });
        }
        let top = self.stack.last_mut().expect("a wrapper is open");
        top.ordinal += 1;
        ListItemNumber {
            level: top.level,
            marker: top.marker,
            ordinal: top.ordinal,
            closed_root,
        }
    }

    /// Ends every open wrapper, innermost first.
    pub fn close_all(&mut self, edge: &mut dyn FnMut(ListEdge<'_>)) {
        while let Some(closed) = self.stack.pop() {
            let root = self.stack.is_empty();
            edge(ListEdge::Closed {
                list: &closed,
                root,
            });
        }
    }
}

#[cfg(test)]
#[path = "lists_tests.rs"]
mod lists_tests;
