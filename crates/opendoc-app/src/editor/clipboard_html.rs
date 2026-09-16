//! Reading the `text/html` flavour of a paste.
//!
//! # The input is hostile
//!
//! `EditorInput::html` is whatever the source application wrote onto the
//! clipboard. That is a web page, another editor, a mail client — or a page
//! that arranged to be copied from. It is never trusted, and the shape of
//! this module follows from that:
//!
//! * **Nothing that arrives is ever emitted as markup.** The parser produces
//!   `Inline` values — text and a closed set of marks — and the document
//!   renderer escapes those on the way out, as it does for every other run.
//!   That is a stronger guarantee than sanitising markup and re-emitting it,
//!   which is the pattern `opendoc-render::mathml` had to build because it
//!   *has* to emit markup; here there is no such obligation, so the safe
//!   answer is to keep none of it.
//! * **Allowlists, never denylists.** An element decides only which marks are
//!   open and whether a line ends; an unknown element contributes nothing but
//!   its text. A style property is read only from a fixed table, and its
//!   value only from a fixed vocabulary — a colour is matched against a
//!   pattern rather than passed through.
//! * **`<script>`, `<style>` and friends lose their contents entirely**, not
//!   just their tags, because their text is code rather than prose.
//! * **Every dimension is bounded**: input length, nesting depth, blocks,
//!   runs, and the length of one run. A clipboard is an easy way to hand a
//!   document 100 MB of nested `<b>`, and the parser is iterative so depth
//!   cannot overflow the stack either.
//! * **A link's `href` is checked against a scheme allowlist.** `javascript:`
//!   and `data:` are dropped and the text kept, so a booby-trapped link
//!   pastes as the words it showed.
//!
//! # Why a parser at all, rather than a dependency
//!
//! The repository has no HTML parser (`quick-xml` is the nearest, and
//! clipboard HTML is not XML — unclosed `<br>`, bare `&`, unquoted
//! attributes). What is needed here is not a DOM: it is a linear walk that
//! tracks which marks are open. That is this file, and it stays this file
//! precisely because every byte of its input is untrusted.

use base64::Engine;
use opendoc_core::{
    Alignment, BlockProperties, Equation, EquationSourceFormat, Inline, ListKind, Mark, MarkExpand,
    MarkKind, OrderedListFormat, PageNumberField, StableId, TextDirection,
};
use quick_xml::events::Event;
use quick_xml::Reader;

/// The block semantics carried by a safe HTML clipboard fragment.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) enum PastedBlockKind {
    #[default]
    Paragraph,
    Heading {
        level: u8,
    },
    ListItem {
        kind: ListKind,
        level: u8,
        list_key: u64,
        /// Explicit HTML `<ol start>` state.  A missing attribute intentionally
        /// remains inherited: HTML's default is presentation, while an
        /// explicit restart is source state the document can carry.
        ordered_start: Option<u32>,
        /// The five standard HTML `<ol type>` values have exact homes in the
        /// model.  Unknown values do not become CSS or an arbitrary counter.
        ordered_format: Option<OrderedListFormat>,
    },
    /// A rectangular HTML table.  This deliberately carries only the parts
    /// that have a lossless home in the document model: rows, cells, header
    /// rows, and the ordinary rich blocks inside a cell.
    Table {
        rows: Vec<PastedTableRow>,
    },
    /// A raster `data:` image whose bytes were supplied by the clipboard.
    /// It is deliberately a block: the document model has no URL-backed or
    /// inline-image primitive.  The editor turns it into a byte-owned blob
    /// before applying the one paste batch.
    Image(PastedImage),
    /// A semantic HTML rule. Its presentation belongs to the destination
    /// document; the model deliberately has no arbitrary-rule CSS payload.
    HorizontalRule,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PastedImage {
    /// A local blob label, never used as the image's accessible text.
    pub(crate) name: String,
    /// The source image's alternative text. An explicitly empty `alt` remains
    /// empty (decorative); absent alt falls back to the generated blob label.
    pub(crate) alt_text: String,
    pub(crate) media_type: String,
    pub(crate) bytes: Vec<u8>,
    pub(crate) blob_hash: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct PastedTableRow {
    pub(crate) header: bool,
    pub(crate) cells: Vec<Vec<PastedBlock>>,
}

/// One block's worth of pasted content.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct PastedBlock {
    pub(crate) runs: Vec<Inline>,
    pub(crate) kind: PastedBlockKind,
    /// Paragraph properties with an exact, existing model home. This is not
    /// a CSS bag: the HTML reader admits only direct alignment/direction
    /// declarations and semantic attributes below.
    pub(crate) properties: BlockProperties,
}

/// A parsed clipboard fragment plus the one user-visible degradation it
/// discovered.  A document warning is deduplicated by the caller, so a large
/// table with many spans does not turn the warnings panel into a log flood.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ParsedPaste {
    pub(crate) blocks: Vec<PastedBlock>,
    pub(crate) warning: Option<PasteWarning>,
}

/// One deliberately named, document-visible paste degradation.  Warnings are
/// deduplicated by [`OpenDocApp::push_model_warning`](crate::OpenDocApp::push_model_warning),
/// not by the hostile fragment parser: repeated pastes should still have the
/// same answer, while the warnings panel must not become a log flood.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PasteWarning {
    pub(crate) code: &'static str,
    pub(crate) message: &'static str,
}

/// At most this many bytes of HTML are read; the rest is ignored.
const MAX_INPUT_BYTES: usize = 4 * 1024 * 1024;
/// Deeper nesting than this stops opening marks. Nothing is rejected — the
/// text still arrives — because a paste that silently lost its words would be
/// worse than one that lost its italics.
const MAX_DEPTH: usize = 64;
/// Caps on what one paste may produce.
const MAX_BLOCKS: usize = 2_000;
const MAX_RUNS: usize = 20_000;
const MAX_RUN_CHARS: usize = 200_000;
/// Attribute values longer than this are ignored rather than parsed.
const MAX_ATTRIBUTE_BYTES: usize = 4_096;

/// Elements whose *content* is not prose, and is dropped with them.
const RAW_TEXT_ELEMENTS: &[&str] = &[
    "script", "style", "head", "title", "noscript", "template", "iframe", "object", "embed", "svg",
    "canvas",
];

/// Elements that end the current paragraph.
const BLOCK_ELEMENTS: &[&str] = &[
    "address",
    "article",
    "aside",
    "blockquote",
    "div",
    "dd",
    "dl",
    "dt",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hr",
    "li",
    "main",
    "nav",
    "ol",
    "p",
    "pre",
    "section",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "tr",
    "ul",
];

/// Elements that are one mark each. Presentational synonyms are folded here
/// rather than at the call site, so `<b>` and `<strong>` cannot drift apart.
fn element_mark(name: &str) -> Option<MarkKind> {
    Some(match name {
        "b" | "strong" => MarkKind::Bold,
        "i" | "em" | "cite" | "var" | "dfn" => MarkKind::Italic,
        "u" | "ins" => MarkKind::Underline,
        "s" | "strike" | "del" => MarkKind::Strike,
        "code" | "kbd" | "samp" | "tt" => MarkKind::Code,
        "sup" => MarkKind::Superscript,
        "sub" => MarkKind::Subscript,
        _ => return None,
    })
}

/// Schemes a pasted link may keep. Everything else — `javascript:`, `data:`,
/// `vbscript:`, and anything a future browser invents — loses the link and
/// keeps the text.
fn href_allowed(href: &str) -> bool {
    let lowered = href.trim().to_ascii_lowercase();
    if lowered.starts_with('#') || lowered.starts_with('/') || lowered.starts_with('.') {
        return true;
    }
    match lowered.split_once(':') {
        Some((scheme, _)) => matches!(scheme, "http" | "https" | "mailto" | "tel" | "ftp"),
        // No scheme at all is a relative reference.
        None => true,
    }
}

/// One open element on the parser's stack.
#[derive(Clone, Debug)]
struct Open {
    name: String,
    /// Marks this element opened, to be closed with it.
    marks: Vec<Mark>,
    /// The link this element opened, if it was an allowed `<a href>`.
    href: Option<String>,
    /// Stable only within this clipboard parse: the editor replaces it with a
    /// document identity, while adjacent items retain their shared list.
    list_key: Option<u64>,
    /// Explicit ordered-list attributes from this element, if it is an `<ol>`.
    ordered_start: Option<u32>,
    ordered_format: Option<OrderedListFormat>,
}

/// Parses `html` into the paragraphs and marked runs it names.
///
/// Never fails: a paste that cannot be understood degrades to the text it
/// contained, which is what the plain-text flavour would have given anyway.
pub(crate) fn parse(html: &str) -> ParsedPaste {
    // The standalone-table shortcut is a parser too. Normalize its source
    // before even looking for a table so it has the same resource boundary as
    // the ordinary fragment builder below; otherwise a huge closed table
    // could make `parse_table` scan past the advertised input cap.
    let html = bounded_input(html);
    let object_warning = unsupported_object_warning(html);
    let review_warning = review_markup_warning(html);
    let structural_warning = structural_markup_warning(html);
    let math_warning = mathml_degradation_warning(html);
    if let Some((start, end, table, warning)) = parse_table(html) {
        // Do not silently throw prose around a table away.  A standalone
        // copied table (including its harmless html/body wrappers) gets a
        // native table; mixed prose follows the established rich-text path.
        if parse_blocks(&html[..start]).is_empty() && parse_blocks(&html[end..]).is_empty() {
            return ParsedPaste {
                blocks: vec![PastedBlock {
                    runs: Vec::new(),
                    kind: PastedBlockKind::Table { rows: table },
                    properties: BlockProperties::default(),
                }],
                warning: warning
                    .map(|message| PasteWarning {
                        code: "clipboard-table-degraded",
                        message,
                    })
                    .or(object_warning)
                    .or(review_warning)
                    .or(structural_warning)
                    .or(math_warning),
            };
        }
    }
    let (blocks, degraded_math, dropped_data_images) = parse_blocks_with_warning(html);
    ParsedPaste {
        blocks,
        warning: object_warning
            .or(review_warning)
            .or(structural_warning)
            .or(dropped_data_images.then_some(PasteWarning {
                code: "clipboard-object-degraded",
                message: "Only the first 20 byte-owned clipboard images were imported; later images were skipped.",
            }))
            .or(degraded_math.then_some(PasteWarning {
            code: "clipboard-object-degraded",
            message: "MathML could not be represented faithfully as an equation; its visible text was pasted instead.",
            }))
            .or(math_warning),
    }
}

/// A quote and a preformatted run are not ordinary paragraphs.  Their visible
/// words can still be kept safely, but the model deliberately has neither a
/// quotation block nor a paragraph-level whitespace-preservation mode.  Do
/// not silently claim that such a paste retained those source semantics.
fn structural_markup_warning(html: &str) -> Option<PasteWarning> {
    let mut at = 0;
    while at < html.len() {
        let offset = html[at..].find('<').map(|offset| at + offset)?;
        let end = tag_end(html, offset)?;
        let body = &html[offset + 1..end];
        let name = body
            .split(|character: char| character.is_ascii_whitespace() || character == '/')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if !body.starts_with('/') && matches!(name.as_str(), "blockquote" | "pre") {
            return Some(PasteWarning {
                code: "clipboard-structure-degraded",
                message: "Preformatted whitespace and block quotations were pasted as ordinary paragraphs; their source block semantics were not imported.",
            });
        }
        at = end + 1;
    }
    None
}

/// HTML's `<ins>` and `<del>` carry a review decision, rather than merely
/// presentational underline or strike. The clipboard reader preserves their
/// visible text using those marks, but has no trustworthy author, timestamp,
/// or accept/reject history to create a native suggestion. Name that loss
/// once instead of making a pasted revision look like a live review thread.
fn review_markup_warning(html: &str) -> Option<PasteWarning> {
    let mut at = 0;
    while at < html.len() {
        let offset = html[at..].find('<').map(|offset| at + offset)?;
        let end = tag_end(html, offset)?;
        let body = &html[offset + 1..end];
        let name = body
            .split(|character: char| character.is_ascii_whitespace() || character == '/')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if !body.starts_with('/') && matches!(name.as_str(), "ins" | "del") {
            return Some(PasteWarning {
                code: "clipboard-review-degraded",
                message: "Tracked insertions and deletions were pasted as ordinary underlined or struck text; review history was not imported.",
            });
        }
        at = end + 1;
    }
    None
}

fn mathml_degradation_warning(html: &str) -> Option<PasteWarning> {
    let mut at = 0;
    while at < html.len() {
        let offset = html[at..].find('<').map(|offset| at + offset)?;
        let end = tag_end(html, offset)?;
        let body = &html[offset + 1..end];
        let name = body
            .split(|c: char| c.is_ascii_whitespace() || c == '/')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        if name == "math"
            && !body.starts_with('/')
            && matching_math_end(html, offset, end)
                .and_then(|math_end| mathml_to_latex(&html[offset..math_end]))
                .is_none()
        {
            return Some(PasteWarning {
                code: "clipboard-object-degraded",
                message: "MathML could not be represented faithfully as an equation; its visible text was pasted instead.",
            });
        }
        at = end + 1;
    }
    None
}

/// Objects do not have a lossless home in the HTML-fragment model yet.  In
/// particular, a remote `<img src=https://…>` must never become a document
/// reference merely because it was pasted. Image-only fragments may take the
/// desktop's fast file path; mixed fragments are decoded here only when they
/// are bounded raster `data:` bytes, then become owned blobs in the editor.
/// Everything else is an honest degradation rather than a URL-backed image.
fn unsupported_object_warning(html: &str) -> Option<PasteWarning> {
    const EMBEDDED_OBJECTS: &[&str] = &[
        "img", "math", "svg", "canvas", "object", "embed", "iframe", "video", "audio",
    ];
    let mut at = 0;
    while at < html.len() {
        let Some(offset) = html[at..].find('<').map(|value| at + value) else {
            break;
        };
        let Some(end) = tag_end(html, offset) else {
            break;
        };
        let inner = &html[offset + 1..end];
        let body = inner.strip_prefix('/').unwrap_or(inner);
        let name = body
            .split(|character: char| character.is_ascii_whitespace() || character == '/')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if EMBEDDED_OBJECTS.contains(&name.as_str())
            && name != "math"
            && !(name == "img" && data_image_from_tag(body).is_some())
        {
            return Some(PasteWarning {
                code: "clipboard-object-degraded",
                message: "An embedded clipboard object was not imported; only byte-owned image files are supported.",
            });
        }
        at = end + 1;
    }
    None
}

fn parse_blocks(html: &str) -> Vec<PastedBlock> {
    parse_blocks_with_warning(html).0
}

fn parse_blocks_with_warning(html: &str) -> (Vec<PastedBlock>, bool, bool) {
    let html = bounded_input(html);

    let mut builder = Builder::default();
    let mut open: Vec<Open> = Vec::new();
    // Depth of raw-text elements we are inside; their text is dropped.
    let mut suppressed = 0usize;
    let bytes = html.as_bytes();
    let mut at = 0usize;

    while at < bytes.len() {
        let Some(tag_start) = html[at..].find('<').map(|offset| at + offset) else {
            if suppressed == 0 {
                builder.push_text(&html[at..], &open);
            }
            break;
        };
        if tag_start > at && suppressed == 0 {
            builder.push_text(&html[at..tag_start], &open);
        }
        // `<!-- -->`, `<!doctype>`, `<?...?>`: markup that carries no content
        // and is a classic way to smuggle a tag past a naive reader, so each
        // is skipped as a unit rather than treated as text.
        if html[tag_start..].starts_with("<!--") {
            at = match html[tag_start + 4..].find("-->") {
                Some(offset) => tag_start + 4 + offset + 3,
                None => bytes.len(),
            };
            continue;
        }
        if html[tag_start..].starts_with("<!") || html[tag_start..].starts_with("<?") {
            at = match html[tag_start..].find('>') {
                Some(offset) => tag_start + offset + 1,
                None => bytes.len(),
            };
            continue;
        }
        let Some(tag_end) = tag_end(html, tag_start) else {
            // An unterminated `<` is text, not a tag.
            if suppressed == 0 {
                builder.push_text(&html[tag_start..], &open);
            }
            break;
        };
        let inner = &html[tag_start + 1..tag_end];
        at = tag_end + 1;

        let closing = inner.starts_with('/');
        let body = if closing { &inner[1..] } else { inner };
        let name = body
            .split(|character: char| character.is_ascii_whitespace() || character == '/')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            continue;
        }

        if closing {
            if RAW_TEXT_ELEMENTS.contains(&name.as_str()) {
                suppressed = suppressed.saturating_sub(1);
            }
            // Close back to the matching element. Unbalanced markup — which
            // is the common case, not the attack case — closes what it can
            // and leaves the rest open.
            if let Some(position) = open.iter().rposition(|entry| entry.name == name) {
                open.truncate(position);
            }
            if BLOCK_ELEMENTS.contains(&name.as_str()) {
                builder.end_block();
            }
            continue;
        }

        if RAW_TEXT_ELEMENTS.contains(&name.as_str()) {
            suppressed += 1;
            continue;
        }
        if name == "math" {
            // MathML is foreign, hostile XML, not ordinary HTML prose.  Take
            // it through the deliberately tiny converter below.  It accepts
            // only presentation constructs for which our canonical
            // LatexLike source has the same meaning; all other MathML leaves
            // its visible token text and gets the normal one warning.
            let Some(math_end) = matching_math_end(html, tag_start, tag_end) else {
                builder.push_text(&mathml_visible_text(&html[tag_start..]), &open);
                builder.degraded_math = true;
                break;
            };
            let math = &html[tag_start..math_end];
            match mathml_to_latex(math) {
                Some(source) => builder.push_equation(source),
                None => {
                    builder.push_text(&mathml_visible_text(math), &open);
                    builder.degraded_math = true;
                }
            }
            at = math_end;
            continue;
        }
        if name == "br" {
            // `<br>` is an explicit soft line break, not a paragraph
            // boundary. The text model already carries that distinction as a
            // newline inside one inline run; making a new PastedBlock here
            // used to turn a copied address or poetry line into independent
            // editable paragraphs.
            builder.push_line_break(&open);
            continue;
        }
        if name == "hr" {
            builder.push_horizontal_rule();
            continue;
        }
        if name == "img" {
            if let Some(image) = data_image_from_tag(body) {
                builder.push_image(image);
                // An OpenDoc image is a block, whereas HTML permits it
                // inline inside a list item. The image emission closes the
                // leading list fragment; retain the same list identity for
                // text that follows it in the still-open `<li>` rather than
                // silently turning that trailing source text into prose.
                builder.resume_open_list_item(&open);
            }
            continue;
        }
        if name == "time" {
            if let Some((after, date)) = simple_date_time(html, tag_end, body, &open) {
                builder.push_date_chip(date);
                at = after;
                continue;
            }
        }
        // The renderer's page fields are deliberately empty: their value is
        // derived by pagination, not stored as source text. Preserve only
        // that exact closed empty projection, never a guessed displayed page
        // number or a foreign element that happens to use `data-field`.
        if name == "span" {
            if let Some((after, field)) = simple_page_number_field(html, tag_end, body, &open) {
                builder.push_page_number_field(field);
                at = after;
                continue;
            }
        }
        // Task-list clipboard HTML commonly represents the state as a real
        // checkbox inside a list item. Read only that explicit semantic form:
        // classes, CSS, Unicode checkbox glyphs, and non-list form controls
        // are presentation or interaction detail with no safe list meaning.
        if name == "input" {
            if attribute(body, "type").is_some_and(|value| value.eq_ignore_ascii_case("checkbox")) {
                builder.set_current_list_item_checked(has_attribute(body, "checked"));
            }
            continue;
        }
        let block_element = BLOCK_ELEMENTS.contains(&name.as_str());
        if block_element {
            builder.end_block();
            builder.properties = pasted_block_properties(body);
        }
        let self_closing = body.trim_end().ends_with('/') || VOID_ELEMENTS.contains(&name.as_str());
        if self_closing {
            continue;
        }
        if open.len() >= MAX_DEPTH {
            // Past the cap the element is still tracked as open, so closing
            // tags stay balanced, but it opens nothing.
            open.push(Open {
                name,
                marks: Vec::new(),
                href: None,
                list_key: None,
                ordered_start: None,
                ordered_format: None,
            });
            continue;
        }
        let mut marks = Vec::new();
        if let Some(kind) = element_mark(&name) {
            marks.push(Mark {
                kind,
                value: None,
                expand: MarkExpand::None,
            });
        }
        // `<mark>` is the HTML semantic highlighting element. Its canonical
        // browser presentation is yellow, which has an exact existing home
        // in the document's typed Background mark. This deliberately does
        // not read author CSS: a nested direct background declaration still
        // wins through `marks_and_href`, just like all other value marks.
        if name == "mark" {
            replace_value_mark(&mut marks, MarkKind::Background, "#ffff00".to_string());
        }
        marks.extend(style_marks(
            attribute(body, "style").as_deref().unwrap_or(""),
        ));
        let href = (name == "a")
            .then(|| attribute(body, "href"))
            .flatten()
            .filter(|href| href_allowed(href));
        let list_key = if matches!(name.as_str(), "ol" | "ul") {
            Some(builder.next_list_key())
        } else {
            None
        };
        // Only semantic HTML attributes are read.  In particular, this does
        // not interpret `style`: clipboard CSS needs a full cascade to be
        // meaningful, whereas these values have direct, bounded model homes.
        let ordered_start = (name == "ol")
            .then(|| attribute(body, "start"))
            .flatten()
            .and_then(|value| value.trim().parse::<u32>().ok())
            .filter(|start| *start > 0);
        let ordered_format = (name == "ol")
            .then(|| attribute(body, "type"))
            .flatten()
            .and_then(|value| match value.trim() {
                "1" => Some(OrderedListFormat::Decimal),
                "a" => Some(OrderedListFormat::LowerAlpha),
                "A" => Some(OrderedListFormat::UpperAlpha),
                "i" => Some(OrderedListFormat::LowerRoman),
                "I" => Some(OrderedListFormat::UpperRoman),
                _ => None,
            });
        if name == "h1"
            || name == "h2"
            || name == "h3"
            || name == "h4"
            || name == "h5"
            || name == "h6"
        {
            builder.kind = PastedBlockKind::Heading {
                level: name[1..].parse().expect("HTML heading suffix is numeric"),
            };
        } else if name == "li" {
            let lists = open
                .iter()
                .filter(|entry| matches!(entry.name.as_str(), "ol" | "ul"))
                .collect::<Vec<_>>();
            if let Some(list) = lists.last() {
                builder.kind = PastedBlockKind::ListItem {
                    kind: if list.name == "ol" {
                        ListKind::Ordered
                    } else {
                        ListKind::Bullet
                    },
                    level: (lists.len() - 1).min(8) as u8,
                    list_key: list
                        .list_key
                        .expect("list elements have a parse-local identity"),
                    ordered_start: list.ordered_start,
                    ordered_format: list.ordered_format,
                };
            }
        }
        open.push(Open {
            name,
            marks,
            href,
            list_key,
            ordered_start,
            ordered_format,
        });
    }

    let degraded_math = builder.degraded_math;
    let dropped_data_images = builder.dropped_data_images;
    (builder.finish(), degraded_math, dropped_data_images)
}

/// Apply the parser's source-byte budget at every parser entry point. The cut
/// is deliberately on a UTF-8 boundary; an incomplete tag is harmless text
/// to the closed-set reader.
fn bounded_input(html: &str) -> &str {
    if html.len() <= MAX_INPUT_BYTES {
        return html;
    }
    let mut cut = MAX_INPUT_BYTES;
    while cut > 0 && !html.is_char_boundary(cut) {
        cut -= 1;
    }
    &html[..cut]
}

/// Extract one HTML table without building or re-emitting a DOM.  The cell
/// payload is sent back through `parse_blocks`, so scripts, links and marks
/// receive exactly the same sanitisation as ordinary rich paste.
fn parse_table(html: &str) -> Option<(usize, usize, Vec<PastedTableRow>, Option<&'static str>)> {
    let mut at = 0;
    let mut table_start = None;
    let mut body_start = 0;
    let mut depth = 0usize;
    while at < html.len() {
        let offset = html[at..].find('<')? + at;
        let end = tag_end(html, offset)?;
        let inner = &html[offset + 1..end];
        let closing = inner.starts_with('/');
        let body = if closing { &inner[1..] } else { inner };
        let name = body
            .split(|c: char| c.is_ascii_whitespace() || c == '/')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        if name == "table" {
            if closing {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    let start = table_start?;
                    let (rows, warning) = parse_table_rows(&html[body_start..offset]);
                    return (!rows.is_empty()).then_some((start, end + 1, rows, warning));
                }
            } else {
                if depth == 0 {
                    table_start = Some(offset);
                    body_start = end + 1;
                }
                depth += 1;
            }
        }
        at = end + 1;
    }
    None
}

fn parse_table_rows(source: &str) -> (Vec<PastedTableRow>, Option<&'static str>) {
    const MAX_TABLE_CELLS: usize = 100;
    let mut rows = Vec::new();
    let mut row: Option<Vec<(bool, usize)>> = None;
    let mut cell_start: Option<usize> = None;
    // `<thead>` is semantic row-header intent even when an office clipboard
    // serializes its cells as `<td>`. The document model already represents
    // that intent on the row, so retain it rather than inferring solely from
    // the individual cell tag names.
    let mut header_section = false;
    let mut warning = None;
    let mut at = 0;
    while at < source.len() {
        let Some(offset) = source[at..].find('<').map(|value| at + value) else {
            break;
        };
        let Some(end) = tag_end(source, offset) else {
            break;
        };
        let inner = &source[offset + 1..end];
        let closing = inner.starts_with('/');
        let body = if closing { &inner[1..] } else { inner };
        let name = body
            .split(|c: char| c.is_ascii_whitespace() || c == '/')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        match (closing, name.as_str()) {
            (false, "tr") => {
                // A following `<tr>` implies the end of the preceding row.
                // Clipboard HTML from browsers and office suites commonly
                // omits `</tr>` here, so replacing `row` directly would
                // silently lose that complete earlier source row.
                finish_html_table_row(
                    source,
                    &mut rows,
                    &mut row,
                    &mut cell_start,
                    offset,
                    header_section,
                    &mut warning,
                );
                row = Some(Vec::new());
            }
            // Starting a new table section also closes a preceding omitted
            // row. Without this, the section markup would become literal
            // content of that row's final cell until a later `<tr>` happened
            // to recover it.
            (false, "thead" | "tbody" | "tfoot") => {
                finish_html_table_row(
                    source,
                    &mut rows,
                    &mut row,
                    &mut cell_start,
                    offset,
                    header_section,
                    &mut warning,
                );
                header_section = name == "thead";
            }
            (false, "td" | "th") => {
                // HTML permits an omitted `</td>`/`</th>` before the next
                // cell. Browsers close that cell at this tag; without doing
                // the same, the clipboard reader discarded the first cell of
                // a very common hand-authored/table-export shape.
                close_html_table_cell(&mut row, &mut cell_start, offset);
                if attribute(body, "colspan").is_some() || attribute(body, "rowspan").is_some() {
                    warning = Some("Table cell spans were flattened while pasting HTML.");
                }
                // Each completed source cell occupies two temporary entries
                // (its header/start record and its end marker). Count cells,
                // not that implementation detail, so this is truly a 100
                // cell cap rather than an accidental 50-cell cap.
                if row
                    .as_ref()
                    .is_some_and(|cells| cells.len() / 2 < MAX_TABLE_CELLS)
                {
                    cell_start = Some(end + 1);
                    row.as_mut()
                        .expect("row was checked")
                        .push((name == "th", end + 1));
                } else if row.is_some() {
                    warning = Some(
                        "Only the first 100 HTML table cells in a row were imported; later cells were skipped.",
                    );
                }
            }
            (true, "td" | "th") => {
                close_html_table_cell(&mut row, &mut cell_start, offset);
            }
            // `</tr>` also implies the close of a final omitted table cell.
            // The same is true at the end of a table section: HTML's parser
            // closes an omitted row before `</thead>`, `</tbody>`, or
            // `</tfoot>`.
            (true, "tr") => finish_html_table_row(
                source,
                &mut rows,
                &mut row,
                &mut cell_start,
                offset,
                header_section,
                &mut warning,
            ),
            (true, "thead" | "tbody" | "tfoot") => {
                finish_html_table_row(
                    source,
                    &mut rows,
                    &mut row,
                    &mut cell_start,
                    offset,
                    header_section,
                    &mut warning,
                );
                header_section = false;
            }
            _ => {}
        }
        at = end + 1;
    }
    // `parse_table` deliberately gives this helper the content *inside* the
    // outer `<table>`, so an omitted final `</tr>`/`</td>` reaches EOF rather
    // than a visible `</table>` token. HTML closes both at that boundary.
    finish_html_table_row(
        source,
        &mut rows,
        &mut row,
        &mut cell_start,
        source.len(),
        header_section,
        &mut warning,
    );
    (rows, warning)
}

/// Finish one HTML row, including the cell and row end tags browsers infer.
///
/// This is deliberately shared by explicit `</tr>`, the next `<tr>`, and a
/// table-section close so all browser-valid optional-end forms have identical
/// capacity accounting and source slicing.
fn finish_html_table_row(
    source: &str,
    rows: &mut Vec<PastedTableRow>,
    row: &mut Option<Vec<(bool, usize)>>,
    cell_start: &mut Option<usize>,
    end: usize,
    header_section: bool,
    warning: &mut Option<&'static str>,
) {
    close_html_table_cell(row, cell_start, end);
    let Some(cells) = row.take() else {
        return;
    };
    if rows.len() >= 200 {
        *warning =
            Some("Only the first 200 HTML table rows were imported; later rows were skipped.");
        return;
    }
    let mut parsed = Vec::new();
    let mut index = 0;
    while index + 1 < cells.len() {
        let (is_header, start) = cells[index];
        let (_, cell_end) = cells[index + 1];
        // The indices were taken from one UTF-8 source string at tag
        // boundaries, so every slice is a valid source fragment.
        parsed.push((is_header, (start, cell_end)));
        index += 2;
    }
    if !parsed.is_empty() {
        let header = header_section || parsed.iter().all(|(is_header, _)| *is_header);
        rows.push(PastedTableRow {
            header,
            cells: parsed
                .into_iter()
                .map(|(_, (start, cell_end))| parse_blocks(&source[start..cell_end]))
                .collect(),
        });
    }
}

/// Finish one table cell, including HTML's optional-end-tag recovery.
///
/// `row` keeps alternating `(header, start)` and `(ignored, end)` pairs until
/// the row is accepted. Keeping that compact scratch representation lets the
/// source cap count actual cells, while this helper makes every explicit or
/// implied close take the exact same path.
fn close_html_table_cell(
    row: &mut Option<Vec<(bool, usize)>>,
    cell_start: &mut Option<usize>,
    end: usize,
) {
    if let (Some(start), Some(cells)) = (cell_start.take(), row.as_mut()) {
        if let Some((_, cell_offset)) = cells.last_mut() {
            *cell_offset = start;
        }
        // Store the end temporarily by appending a marker pair; it is
        // converted below before the row is accepted.
        cells.push((false, end));
    }
}

/// The `>` that ends the start tag beginning at `tag_start`, skipping any
/// that sits inside a quoted attribute value.
///
/// A browser does not end a tag on the `>` in
/// `<a href="data:text/html,<script>">`, and a reader that does disagrees
/// with the browser about where the markup ends. Disagreements of exactly
/// that shape are the mutation-XSS family, so the cheap naive scan is the
/// wrong one even though nothing here re-emits markup.
fn tag_end(html: &str, tag_start: usize) -> Option<usize> {
    let mut quote: Option<char> = None;
    for (offset, character) in html[tag_start + 1..].char_indices() {
        match (quote, character) {
            (Some(open), _) if character == open => quote = None,
            (Some(_), _) => {}
            (None, '"') | (None, '\'') => quote = Some(character),
            (None, '>') => return Some(tag_start + 1 + offset),
            (None, _) => {}
        }
    }
    None
}

const VOID_ELEMENTS: &[&str] = &[
    "area", "base", "br", "col", "hr", "img", "input", "link", "meta", "param", "source", "track",
    "wbr",
];

/// Accumulates blocks and runs, merging adjacent runs that carry the same
/// marks so a paste does not arrive pre-split by the source's own spans.
#[derive(Default)]
struct Builder {
    blocks: Vec<PastedBlock>,
    current: Vec<Inline>,
    kind: PastedBlockKind,
    properties: BlockProperties,
    list_keys: u64,
    images: usize,
    degraded_math: bool,
    dropped_data_images: bool,
}

impl Builder {
    fn push_text(&mut self, raw: &str, open: &[Open]) {
        let text = decode_entities(raw);
        let text = collapse_whitespace(&text);
        if text.is_empty() || (text == " " && self.current.is_empty()) {
            // A space at the very start of a paragraph is the source's
            // indentation, not a word gap.
            return;
        }
        let (marks, href) = marks_and_href(open);
        self.push_run(text, marks, href);
    }

    /// Adds an explicit HTML `<br>` without applying HTML whitespace
    /// collapsing. A leading/trailing break has no representable source text
    /// by itself, so it stays absent rather than manufacturing an empty block.
    fn push_line_break(&mut self, open: &[Open]) {
        if self.current.is_empty() {
            return;
        }
        let (marks, href) = marks_and_href(open);
        self.push_run("\n".to_string(), marks, href);
    }

    fn push_run(&mut self, text: String, marks: Vec<Mark>, href: Option<String>) {
        if text.is_empty() {
            return;
        }
        match self.current.last_mut() {
            Some(Inline::Text {
                text: existing,
                marks: existing_marks,
                ..
            }) if href.is_none()
                && *existing_marks == marks
                && existing.chars().count() < MAX_RUN_CHARS =>
            {
                existing.push_str(&text)
            }
            Some(Inline::Link {
                text: existing,
                marks: existing_marks,
                href: existing_href,
                ..
            }) if href.as_deref() == Some(existing_href)
                && *existing_marks == marks
                && existing.chars().count() < MAX_RUN_CHARS =>
            {
                existing.push_str(&text)
            }
            _ => {
                if self.run_count() < MAX_RUNS {
                    self.current.push(build_inline(text, marks, href));
                }
            }
        }
    }

    fn push_equation(&mut self, source: String) {
        self.current.push(Inline::Equation {
            id: StableId::new("equation"),
            equation: Equation {
                id: StableId::new("equation"),
                source_format: EquationSourceFormat::LatexLike,
                source,
            },
        });
    }

    fn push_date_chip(&mut self, date: String) {
        if self.run_count() < MAX_RUNS {
            self.current.push(Inline::DateChip {
                id: StableId::new("date-chip"),
                date,
            });
        }
    }

    fn push_page_number_field(&mut self, field: PageNumberField) {
        if self.run_count() < MAX_RUNS {
            self.current.push(Inline::PageNumber {
                id: StableId::new("page-number"),
                field,
            });
        }
    }

    fn run_count(&self) -> usize {
        self.blocks
            .iter()
            .map(|block| block.runs.len())
            .sum::<usize>()
            + self.current.len()
    }

    /// Closes the paragraph being built, if it has anything in it.
    ///
    /// An empty one is never emitted: `</p><p>` is two block boundaries in a
    /// row and means one paragraph break, not a blank line, and markup that
    /// wraps everything in three nested `<div>`s would otherwise paste as a
    /// document of empty paragraphs.
    fn end_block(&mut self) {
        if self.current.is_empty() {
            return;
        }
        if self.blocks.len() >= MAX_BLOCKS {
            self.current.clear();
            return;
        }
        let runs = std::mem::take(&mut self.current);
        self.blocks.push(PastedBlock {
            runs: trimmed(runs),
            kind: std::mem::take(&mut self.kind),
            properties: std::mem::take(&mut self.properties),
        });
    }

    fn push_image(&mut self, image: PastedImage) {
        self.end_block();
        if self.blocks.len() >= MAX_BLOCKS {
            return;
        }
        if self.images >= MAX_DATA_IMAGES {
            self.dropped_data_images = true;
            return;
        }
        self.images += 1;
        self.blocks.push(PastedBlock {
            runs: Vec::new(),
            kind: PastedBlockKind::Image(image),
            properties: BlockProperties::default(),
        });
    }

    /// Re-establish the active list item's typed context after a block object
    /// split it out of an HTML `<li>`. The parser only calls this while the
    /// original element stack is live, so a stray image between list items
    /// cannot manufacture a list block.
    fn resume_open_list_item(&mut self, open: &[Open]) {
        let Some(list_item) = open.iter().rposition(|entry| entry.name == "li") else {
            return;
        };
        let lists = open[..=list_item]
            .iter()
            .filter(|entry| matches!(entry.name.as_str(), "ol" | "ul"))
            .collect::<Vec<_>>();
        let Some(list) = lists.last() else {
            return;
        };
        self.kind = PastedBlockKind::ListItem {
            kind: if list.name == "ol" {
                ListKind::Ordered
            } else {
                ListKind::Bullet
            },
            level: (lists.len() - 1).min(8) as u8,
            list_key: list
                .list_key
                .expect("list elements have a parse-local identity"),
            ordered_start: list.ordered_start,
            ordered_format: list.ordered_format,
        };
    }

    fn push_horizontal_rule(&mut self) {
        self.end_block();
        if self.blocks.len() >= MAX_BLOCKS {
            return;
        }
        self.blocks.push(PastedBlock {
            runs: Vec::new(),
            kind: PastedBlockKind::HorizontalRule,
            properties: BlockProperties::default(),
        });
    }

    fn set_current_list_item_checked(&mut self, checked: bool) {
        if let PastedBlockKind::ListItem { kind, .. } = &mut self.kind {
            *kind = ListKind::Checklist { checked };
        }
    }

    fn next_list_key(&mut self) -> u64 {
        self.list_keys += 1;
        self.list_keys
    }

    fn finish(mut self) -> Vec<PastedBlock> {
        self.end_block();
        self.blocks.retain(|block| {
            !block.runs.is_empty()
                || matches!(
                    block.kind,
                    PastedBlockKind::Image(_) | PastedBlockKind::HorizontalRule
                )
        });
        self.blocks
    }
}

fn marks_and_href(open: &[Open]) -> (Vec<Mark>, Option<String>) {
    let mut marks: Vec<Mark> = Vec::new();
    for entry in open {
        for mark in &entry.marks {
            // The model has one value for each of these direct properties.
            // A nested clipboard span is more specific than its parent, so
            // its concrete value replaces the inherited one. Retaining both
            // made a valid `color:red` / `color:blue` fragment ambiguous to
            // renderers and exporters even though no CSS cascade or class
            // lookup is needed to resolve this direct-element case.
            if value_mark_kind(&mark.kind) {
                marks.retain(|existing| existing.kind != mark.kind);
                marks.push(mark.clone());
            } else if !marks.contains(mark) {
                marks.push(mark.clone());
            }
        }
    }
    let href = open.iter().rev().find_map(|entry| entry.href.clone());
    (marks, href)
}

/// Value-bearing marks have one effective declaration at an inline point;
/// boolean marks can coexist. Keep this local rather than accepting a CSS
/// property bag into the document model.
fn value_mark_kind(kind: &MarkKind) -> bool {
    matches!(
        kind,
        MarkKind::Color | MarkKind::Background | MarkKind::Font | MarkKind::Size
    )
}

/// Accept the one HTML time form with an exact DateChip home.  `datetime`
/// also permits instants, durations and localised display text, none of which
/// is a calendar-day chip.  We therefore require a canonical date as both
/// attribute and visible text, no nested markup, and no surrounding mark/link
/// context that an atomic chip cannot preserve.
fn simple_date_time(
    html: &str,
    opening_end: usize,
    tag: &str,
    open: &[Open],
) -> Option<(usize, String)> {
    if open
        .iter()
        .any(|entry| !entry.marks.is_empty() || entry.href.is_some())
    {
        return None;
    }
    let date = attribute(tag, "datetime")?;
    if !is_calendar_date(&date) {
        return None;
    }
    let close_start = html[opening_end + 1..]
        .find("</time")
        .map(|offset| opening_end + 1 + offset)?;
    let close_end = tag_end(html, close_start)?;
    let closing = html[close_start + 1..close_end].trim();
    if !closing
        .strip_prefix('/')
        .is_some_and(|name| name.trim().eq_ignore_ascii_case("time"))
    {
        return None;
    }
    let visible = &html[opening_end + 1..close_start];
    if visible.contains('<') || collapse_whitespace(&decode_entities(visible)).trim() != date {
        return None;
    }
    Some((close_end + 1, date))
}

/// Accept only the empty `<span data-field=...></span>` token emitted by the
/// renderer for a page field. Its page value is intentionally absent before
/// pagination, so exact structural shape rather than visible text proves the
/// round trip. Keeping the tag empty prevents an arbitrary foreign span from
/// consuming its text under the guise of a field.
fn simple_page_number_field(
    html: &str,
    opening_end: usize,
    tag: &str,
    open: &[Open],
) -> Option<(usize, PageNumberField)> {
    if open
        .iter()
        .any(|entry| !entry.marks.is_empty() || entry.href.is_some())
    {
        return None;
    }
    let field = PageNumberField::parse(attribute(tag, "data-field")?.trim()).ok()?;
    let close_start = html[opening_end + 1..]
        .find("</span")
        .map(|offset| opening_end + 1 + offset)?;
    let close_end = tag_end(html, close_start)?;
    let closing = html[close_start + 1..close_end].trim();
    if !closing
        .strip_prefix('/')
        .is_some_and(|name| name.trim().eq_ignore_ascii_case("span"))
        || !html[opening_end + 1..close_start].trim().is_empty()
    {
        return None;
    }
    Some((close_end + 1, field))
}

fn is_calendar_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || !bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
    {
        return false;
    }
    let number = |range: std::ops::Range<usize>| {
        std::str::from_utf8(&bytes[range]).ok()?.parse::<u32>().ok()
    };
    let (Some(year), Some(month), Some(day)) = (number(0..4), number(5..7), number(8..10)) else {
        return false;
    };
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    year != 0 && (1..=days).contains(&day)
}

const MAX_DATA_IMAGE_BYTES: usize = 4 * 1024 * 1024;
const MAX_DATA_IMAGES: usize = 20;

/// Decode exactly the bounded, byte-ownable raster shape accepted from HTML.
/// All other image sources, including remote URLs and SVG, remain visible
/// degradations rather than references the document cannot archive.
fn data_image_from_tag(body: &str) -> Option<PastedImage> {
    let source = attribute(body, "src")?;
    let (media_type, encoded) = source.strip_prefix("data:")?.split_once(";base64,")?;
    let media_type = media_type.to_ascii_lowercase();
    let extension = match media_type.as_str() {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/bmp" => "bmp",
        "image/tiff" => "tiff",
        _ => return None,
    };
    if encoded.len() > MAX_DATA_IMAGE_BYTES.saturating_mul(4).div_ceil(3)
        || !encoded
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'='))
    {
        return None;
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()?;
    if bytes.len() > MAX_DATA_IMAGE_BYTES {
        return None;
    }
    Some(PastedImage {
        name: format!("pasted-image.{extension}"),
        alt_text: attribute(body, "alt").unwrap_or_else(|| format!("pasted-image.{extension}")),
        media_type,
        bytes,
        blob_hash: None,
    })
}

const MAX_MATHML_BYTES: usize = 64 * 1024;
const MAX_MATHML_DEPTH: usize = 32;
const MAX_MATHML_NODES: usize = 512;

/// Locate the close tag for the `<math>` opening at `start`.  This only
/// delimits a candidate; [`mathml_to_latex`] still uses an XML reader and
/// rejects malformed markup.  Keeping this scan quote-aware means an `>` in
/// an attacker-controlled attribute cannot desynchronise the HTML walk.
fn matching_math_end(html: &str, start: usize, first_tag_end: usize) -> Option<usize> {
    let mut depth = 1usize;
    let mut at = first_tag_end + 1;
    while at < html.len() && at.saturating_sub(start) <= MAX_MATHML_BYTES {
        let offset = html[at..].find('<')? + at;
        let end = tag_end(html, offset)?;
        let inner = &html[offset + 1..end];
        let closing = inner.starts_with('/');
        let body = inner.strip_prefix('/').unwrap_or(inner);
        let name = body
            .split(|c: char| c.is_ascii_whitespace() || c == '/')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        if name == "math" {
            if closing {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(end + 1);
                }
            } else if !body.trim_end().ends_with('/') {
                depth += 1;
            }
        }
        at = end + 1;
    }
    None
}

#[derive(Debug)]
enum MathNode {
    Token {
        name: String,
        text: String,
    },
    Group {
        name: String,
        children: Vec<MathNode>,
    },
}

/// Convert only the small presentation-MathML subset whose tree maps exactly
/// to our canonical LatexLike source.  Attributes are intentionally refused:
/// they can change MathML semantics (or carry URLs), while this importer has
/// no attribute model to preserve them in.
fn mathml_to_latex(markup: &str) -> Option<String> {
    if markup.len() > MAX_MATHML_BYTES {
        return None;
    }
    let mut reader = Reader::from_str(markup);
    reader.config_mut().trim_text(true);
    reader.config_mut().check_end_names = true;
    let mut stack: Vec<MathNode> = Vec::new();
    let mut root = None;
    let mut nodes = 0usize;
    loop {
        match reader.read_event().ok()? {
            Event::Start(event) => {
                let name = std::str::from_utf8(event.name().as_ref())
                    .ok()?
                    .to_ascii_lowercase();
                if !math_element_allowed(&name) || event.attributes().next().is_some() {
                    return None;
                }
                nodes += 1;
                if nodes > MAX_MATHML_NODES || stack.len() >= MAX_MATHML_DEPTH {
                    return None;
                }
                stack.push(if matches!(name.as_str(), "mi" | "mn" | "mo") {
                    MathNode::Token {
                        name,
                        text: String::new(),
                    }
                } else {
                    MathNode::Group {
                        name,
                        children: Vec::new(),
                    }
                });
            }
            Event::Empty(event) => {
                let name = std::str::from_utf8(event.name().as_ref())
                    .ok()?
                    .to_ascii_lowercase();
                // Only an empty grouping row has a source-equivalent meaning.
                if name != "mrow" || event.attributes().next().is_some() {
                    return None;
                }
                push_math_node(
                    &mut stack,
                    &mut root,
                    MathNode::Group {
                        name,
                        children: Vec::new(),
                    },
                )?;
            }
            Event::Text(text) => {
                let decoded = text.decode().ok()?;
                let Some(MathNode::Token { text: token, .. }) = stack.last_mut() else {
                    if !decoded.trim().is_empty() {
                        return None;
                    }
                    continue;
                };
                token.push_str(&decoded);
                if token.len() > 256 {
                    return None;
                }
            }
            Event::End(event) => {
                let name = std::str::from_utf8(event.name().as_ref())
                    .ok()?
                    .to_ascii_lowercase();
                let node = stack.pop()?;
                if math_node_name(&node) != name {
                    return None;
                }
                push_math_node(&mut stack, &mut root, node)?;
            }
            Event::Eof => break,
            Event::Comment(_)
            | Event::CData(_)
            | Event::PI(_)
            | Event::Decl(_)
            | Event::DocType(_) => return None,
            _ => return None,
        }
    }
    if !stack.is_empty() {
        return None;
    }
    let MathNode::Group { name, children } = root? else {
        return None;
    };
    if name != "math" {
        return None;
    }
    let source = children
        .iter()
        .map(math_node_to_latex)
        .collect::<Option<String>>()?;
    (!source.is_empty() && source.len() <= 8 * 1024).then_some(source)
}

fn math_element_allowed(name: &str) -> bool {
    matches!(
        name,
        "math" | "mrow" | "mi" | "mn" | "mo" | "msup" | "msub" | "msubsup" | "mfrac" | "msqrt"
    )
}

fn math_node_name(node: &MathNode) -> &str {
    match node {
        MathNode::Token { name, .. } | MathNode::Group { name, .. } => name,
    }
}

fn push_math_node(
    stack: &mut [MathNode],
    root: &mut Option<MathNode>,
    node: MathNode,
) -> Option<()> {
    if let Some(MathNode::Group { children, .. }) = stack.last_mut() {
        children.push(node);
        Some(())
    } else if stack.is_empty() && root.is_none() {
        *root = Some(node);
        Some(())
    } else {
        None
    }
}

fn math_node_to_latex(node: &MathNode) -> Option<String> {
    match node {
        MathNode::Token { name, text } => math_token_to_latex(name, text),
        MathNode::Group { name, children } => match name.as_str() {
            "math" | "mrow" => children.iter().map(math_node_to_latex).collect(),
            "msup" if children.len() == 2 => Some(format!(
                "{}^{{{}}}",
                math_node_to_latex(&children[0])?,
                math_node_to_latex(&children[1])?
            )),
            "msub" if children.len() == 2 => Some(format!(
                "{}_{{{}}}",
                math_node_to_latex(&children[0])?,
                math_node_to_latex(&children[1])?
            )),
            "msubsup" if children.len() == 3 => Some(format!(
                "{}_{{{}}}^{{{}}}",
                math_node_to_latex(&children[0])?,
                math_node_to_latex(&children[1])?,
                math_node_to_latex(&children[2])?
            )),
            "mfrac" if children.len() == 2 => Some(format!(
                "\\frac{{{}}}{{{}}}",
                math_node_to_latex(&children[0])?,
                math_node_to_latex(&children[1])?
            )),
            "msqrt" if children.len() == 1 => {
                Some(format!("\\sqrt{{{}}}", math_node_to_latex(&children[0])?))
            }
            _ => None,
        },
    }
}

fn math_token_to_latex(name: &str, token: &str) -> Option<String> {
    let token = token.trim();
    if token.is_empty() || token.len() > 128 {
        return None;
    }
    let allowed = match name {
        "mi" => token.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
        "mn" => token.chars().all(|c| c.is_ascii_digit() || c == '.'),
        "mo" => token.chars().all(|c| {
            matches!(
                c,
                '+' | '-' | '=' | '(' | ')' | '[' | ']' | ',' | '/' | '*' | '<' | '>' | '|'
            )
        }),
        _ => false,
    };
    allowed.then_some(token.to_string())
}

/// Safe visible fallback: only token contents are retained; unknown elements
/// (including script-like payloads) never get to contribute text.
fn mathml_visible_text(markup: &str) -> String {
    let mut reader = Reader::from_str(markup);
    reader.config_mut().trim_text(true);
    let mut token_depth = 0usize;
    let mut suppressed = 0usize;
    let mut out = String::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) => {
                if matches!(
                    event.name().as_ref(),
                    b"script" | b"style" | b"iframe" | b"object" | b"embed"
                ) {
                    suppressed += 1;
                }
                if matches!(event.name().as_ref(), b"mi" | b"mn" | b"mo") {
                    token_depth += 1;
                }
            }
            Ok(Event::End(event)) => {
                if matches!(event.name().as_ref(), b"mi" | b"mn" | b"mo") {
                    token_depth = token_depth.saturating_sub(1);
                }
                if matches!(
                    event.name().as_ref(),
                    b"script" | b"style" | b"iframe" | b"object" | b"embed"
                ) {
                    suppressed = suppressed.saturating_sub(1);
                }
            }
            Ok(Event::Text(text)) if token_depth > 0 && suppressed == 0 => {
                if let Ok(text) = text.decode() {
                    out.push_str(text.trim());
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        if out.len() >= 8 * 1024 {
            break;
        }
    }
    out
}

/// A paragraph's runs with the layout whitespace at its two ends removed, and
/// any run that is left empty dropped.
fn trimmed(runs: Vec<Inline>) -> Vec<Inline> {
    let mut runs = runs;
    if let Some(Inline::Text { text, .. } | Inline::Link { text, .. }) = runs.first_mut() {
        *text = text.trim_start_matches(' ').to_string();
    }
    if let Some(Inline::Text { text, .. } | Inline::Link { text, .. }) = runs.last_mut() {
        *text = text.trim_end_matches(' ').to_string();
    }
    runs.retain(|run| match run {
        Inline::Text { text, .. } | Inline::Link { text, .. } => !text.is_empty(),
        _ => true,
    });
    runs
}

fn build_inline(text: String, marks: Vec<Mark>, href: Option<String>) -> Inline {
    match href {
        Some(href) => Inline::Link {
            id: opendoc_core::StableId::new("link"),
            text,
            href,
            marks,
        },
        None => Inline::Text {
            id: opendoc_core::StableId::new("text"),
            text,
            marks,
        },
    }
}

/// The marks an inline `style` attribute names.
///
/// Only these properties are read, and each only against a fixed
/// vocabulary — a paste from Google Docs or Word carries its formatting here
/// rather than in `<b>`/`<i>`, so ignoring `style` would lose most real
/// formatting; reading it loosely would be a second parser over attacker
/// text.  Foreground and highlight colours have exact homes in `MarkKind`;
/// only canonical six-digit RGB values are admitted so DOCX/ODT export does
/// not silently discard them. Point font sizes are likewise document-owned
/// and canonicalized to the model's bare point value. A `font-family` keeps
/// one bounded, concrete family name — the model's `Font` mark already has
/// DOCX/ODT/Google homes — while fallback lists, generic families, relative
/// sizes, and arbitrary CSS remain out of scope.
fn style_marks(style: &str) -> Vec<Mark> {
    if style.len() > MAX_ATTRIBUTE_BYTES {
        return Vec::new();
    }
    let mut marks = Vec::new();
    for declaration in style.split(';') {
        let Some((property, value)) = declaration.split_once(':') else {
            continue;
        };
        let property = property.trim().to_ascii_lowercase();
        let value = value.trim();
        let lowered_value = value.to_ascii_lowercase();
        match property.as_str() {
            "font-weight" => {
                let bold = lowered_value == "bold"
                    || lowered_value == "bolder"
                    || lowered_value
                        .parse::<u32>()
                        .is_ok_and(|weight| weight >= 600);
                if bold {
                    marks.push(boolean_mark(MarkKind::Bold));
                }
            }
            "font-style" => {
                if lowered_value == "italic" || lowered_value == "oblique" {
                    marks.push(boolean_mark(MarkKind::Italic));
                }
            }
            "text-decoration" | "text-decoration-line" => {
                if lowered_value.contains("underline") {
                    marks.push(boolean_mark(MarkKind::Underline));
                }
                if lowered_value.contains("line-through") {
                    marks.push(boolean_mark(MarkKind::Strike));
                }
            }
            "vertical-align" => match lowered_value.as_str() {
                "super" => marks.push(boolean_mark(MarkKind::Superscript)),
                "sub" => marks.push(boolean_mark(MarkKind::Subscript)),
                _ => {}
            },
            "color" => {
                if let Some(color) = clipboard_rgb(&lowered_value) {
                    replace_value_mark(&mut marks, MarkKind::Color, color);
                }
            }
            "background-color" | "background" => {
                if let Some(color) = clipboard_rgb(&lowered_value) {
                    replace_value_mark(&mut marks, MarkKind::Background, color);
                }
            }
            "font-size" => {
                if let Some(points) = clipboard_point_size(&lowered_value) {
                    replace_value_mark(&mut marks, MarkKind::Size, points);
                }
            }
            "font-family" => {
                if let Some(font) = clipboard_font_family(value) {
                    replace_value_mark(&mut marks, MarkKind::Font, font);
                }
            }
            _ => {}
        }
    }
    marks
}

/// The small block-property subset safe to read from a clipboard style. These
/// are direct declarations on a paragraph-like HTML element, with exact typed
/// model homes. Inherited CSS, classes, `auto`, and physical layout context
/// remain out of scope rather than becoming a clipboard-only approximation.
fn pasted_block_properties(tag: &str) -> BlockProperties {
    let mut properties = BlockProperties::default();
    if let Some(direction) = attribute(tag, "dir")
        .as_deref()
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .and_then(|value| TextDirection::parse(&value).ok())
    {
        properties.direction = Some(direction);
    }
    if let Some(alignment) = attribute(tag, "align")
        .as_deref()
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .and_then(|value| Alignment::parse(&value).ok())
    {
        properties.alignment = Some(alignment);
    }
    let Some(style) = attribute(tag, "style") else {
        return properties;
    };
    if style.len() > MAX_ATTRIBUTE_BYTES {
        return properties;
    }
    for declaration in style.split(';') {
        let Some((property, value)) = declaration.split_once(':') else {
            continue;
        };
        let property = property.trim().to_ascii_lowercase();
        let value = value.trim().to_ascii_lowercase();
        match property.as_str() {
            // An inline declaration is source state of this exact element;
            // unlike a class or stylesheet it needs no CSS cascade to read.
            "text-align" => {
                if let Ok(alignment) = Alignment::parse(&value) {
                    properties.alignment = Some(alignment);
                }
            }
            "direction" => {
                if let Ok(direction) = TextDirection::parse(&value) {
                    properties.direction = Some(direction);
                }
            }
            _ => {}
        }
    }
    properties
}

/// Read one concrete family name from the CSS form Google Docs and Word put
/// on the clipboard. The model records one chosen typeface, not a browser
/// fallback algorithm, so only the first list entry is meaningful here. Its
/// deliberately small grammar is a subset of the renderer's CSS-value
/// grammar: hostile input cannot alter the later `font-family` declaration.
fn clipboard_font_family(value: &str) -> Option<String> {
    const GENERIC_FAMILIES: &[&str] = &[
        "serif",
        "sans-serif",
        "monospace",
        "cursive",
        "fantasy",
        "system-ui",
    ];
    let mut first = value.split(',').next()?.trim();
    if first.len() > 64 {
        return None;
    }
    if let Some(quote) = first
        .chars()
        .next()
        .filter(|quote| matches!(quote, '\'' | '"'))
    {
        first = first.strip_prefix(quote)?.strip_suffix(quote)?.trim();
    }
    if first.is_empty()
        || !first.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, ' ' | '-' | '_' | '\'')
        })
        || GENERIC_FAMILIES.contains(&first.to_ascii_lowercase().as_str())
    {
        return None;
    }
    Some(first.to_string())
}

fn boolean_mark(kind: MarkKind) -> Mark {
    Mark {
        kind,
        value: None,
        expand: MarkExpand::None,
    }
}

/// Canonicalise the absolute point sizes Google Docs writes into HTML
/// clipboard fragments. Pixels and relative units deliberately stay out: a
/// pixel depends on a display/CSS context and `em`/`%` depend on the
/// surrounding document, neither of which is part of the paste payload.
/// Keeping a practical upper bound also prevents a hostile clipboard from
/// producing a pathological layout merely by naming an enormous font.
fn clipboard_point_size(value: &str) -> Option<String> {
    const MAX_POINTS: f64 = 1_000.0;
    let points = value
        .trim()
        .strip_suffix("pt")?
        .trim()
        .parse::<f64>()
        .ok()?;
    if !points.is_finite() || !(0.0..=MAX_POINTS).contains(&points) || points == 0.0 {
        return None;
    }
    Some(points.to_string())
}

/// Canonicalise the two colour spellings Google Docs commonly places in its
/// clipboard HTML.  CSS names, `rgba()`, variables, gradients and functions
/// are intentionally excluded: they either have no portable document value or
/// would turn this bounded reader into a CSS evaluator.
fn clipboard_rgb(value: &str) -> Option<String> {
    let value = value.trim();
    if let Some(hex) = value.strip_prefix('#') {
        if hex.len() == 6 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Some(format!("#{}", hex.to_ascii_lowercase()));
        }
        if hex.len() == 3 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            let mut expanded = String::from("#");
            for byte in hex.bytes() {
                let character = (byte as char).to_ascii_lowercase();
                expanded.push(character);
                expanded.push(character);
            }
            return Some(expanded);
        }
    }
    let body = value.strip_prefix("rgb(")?.strip_suffix(')')?;
    let channels = body
        .split(',')
        .map(|channel| channel.trim().parse::<u8>().ok())
        .collect::<Option<Vec<_>>>()?;
    if channels.len() != 3 {
        return None;
    }
    Some(format!(
        "#{:02x}{:02x}{:02x}",
        channels[0], channels[1], channels[2]
    ))
}

fn replace_value_mark(marks: &mut Vec<Mark>, kind: MarkKind, value: String) {
    marks.retain(|mark| mark.kind != kind);
    marks.push(Mark {
        kind,
        value: Some(value),
        expand: MarkExpand::None,
    });
}

/// One attribute's value out of a start tag's body.
///
/// Quoted and unquoted forms both, because clipboard HTML uses both. The
/// value is returned as written; the caller decides what it is allowed to
/// mean.
fn attribute(body: &str, name: &str) -> Option<String> {
    let lowered = body.to_ascii_lowercase();
    let mut search = 0usize;
    while let Some(offset) = lowered[search..].find(name) {
        let start = search + offset;
        search = start + name.len();
        // The name has to stand alone: `href` must not match `data-href`.
        let before_ok = start == 0
            || lowered[..start]
                .chars()
                .next_back()
                .is_some_and(|character| character.is_ascii_whitespace());
        if !before_ok {
            continue;
        }
        let rest = lowered[search..].trim_start();
        if !rest.starts_with('=') {
            continue;
        }
        let value_start = search + (lowered[search..].len() - rest.len()) + 1;
        let value = body[value_start..].trim_start();
        if value.len() > MAX_ATTRIBUTE_BYTES {
            return None;
        }
        let raw = match value.chars().next() {
            Some('"') => value[1..].split('"').next().unwrap_or(""),
            Some('\'') => value[1..].split('\'').next().unwrap_or(""),
            _ => value
                .split(|character: char| character.is_ascii_whitespace())
                .next()
                .unwrap_or(""),
        };
        return Some(decode_entities(raw));
    }
    None
}

/// Whether a boolean HTML attribute occurs as a complete attribute name.
/// `checked` is intentionally not parsed as a value: HTML defines its mere
/// presence as true, including `checked`, `checked=""`, and
/// `checked="checked"`.
fn has_attribute(body: &str, name: &str) -> bool {
    let lowered = body.to_ascii_lowercase();
    let mut search = 0usize;
    while let Some(offset) = lowered[search..].find(name) {
        let start = search + offset;
        search = start + name.len();
        let before_ok = start == 0
            || lowered[..start]
                .chars()
                .next_back()
                .is_some_and(|character| character.is_ascii_whitespace());
        let after_ok = lowered[search..].chars().next().is_none_or(|character| {
            character.is_ascii_whitespace() || character == '=' || character == '/'
        });
        if before_ok && after_ok {
            return true;
        }
    }
    false
}

/// Decodes the entity forms a clipboard actually carries.
///
/// An unrecognised entity is kept as the literal characters it was written
/// with. That is safe here in a way it would not be in a sanitiser that
/// re-emits markup: this text becomes an `Inline`, and the renderer escapes
/// it on the way out.
fn decode_entities(raw: &str) -> String {
    if !raw.contains('&') {
        return raw.to_string();
    }
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        let Some(end) = tail[..tail.len().min(12)].find(';') else {
            out.push('&');
            rest = &tail[1..];
            continue;
        };
        let name = &tail[1..end];
        let decoded = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" | "#39" => Some('\''),
            "nbsp" => Some(' '),
            _ => numeric_entity(name),
        };
        match decoded {
            Some(character) => {
                out.push(character);
                rest = &tail[end + 1..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn numeric_entity(name: &str) -> Option<char> {
    let digits = name.strip_prefix('#')?;
    let code = match digits.strip_prefix(['x', 'X']) {
        Some(hex) => u32::from_str_radix(hex, 16).ok()?,
        None => digits.parse::<u32>().ok()?,
    };
    let character = char::from_u32(code)?;
    // A control character in pasted prose is never meant; it is how a payload
    // hides from a reader looking at the text.
    (!character.is_control() || character == '\n' || character == '\t').then_some(character)
}

/// HTML whitespace collapses, so pasted markup does not bring the source's
/// indentation into the document as runs of spaces. A newline inside markup
/// is layout, not a line break: `<br>` and the block elements are what end a
/// line here.
fn collapse_whitespace(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pending_space = false;
    for character in text.chars() {
        // U+00A0 is a non-breaking space and is content, not layout.
        if character.is_whitespace() && character != '\u{00A0}' {
            pending_space = true;
            continue;
        }
        if pending_space {
            out.push(' ');
            pending_space = false;
        }
        out.push(character);
    }
    if pending_space {
        out.push(' ');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runs(html: &str) -> Vec<(String, Vec<MarkKind>, Option<String>)> {
        parse(html)
            .blocks
            .into_iter()
            .flat_map(|block| block.runs)
            .map(|inline| match inline {
                Inline::Text { text, marks, .. } => {
                    (text, marks.into_iter().map(|m| m.kind).collect(), None)
                }
                Inline::Link {
                    text, href, marks, ..
                } => (
                    text,
                    marks.into_iter().map(|m| m.kind).collect(),
                    Some(href),
                ),
                other => panic!("unexpected inline {other:?}"),
            })
            .collect()
    }

    fn text_of(html: &str) -> String {
        parse(html)
            .blocks
            .into_iter()
            .map(|block| {
                block
                    .runs
                    .iter()
                    .map(|inline| match inline {
                        Inline::Text { text, .. } | Inline::Link { text, .. } => text.as_str(),
                        _ => "",
                    })
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
    #[test]
    fn semantic_tags_become_marks() {
        assert_eq!(
            runs("<p>plain <b>bold</b> <i>italic</i> <code>code</code></p>"),
            vec![
                ("plain ".to_string(), vec![], None),
                ("bold".to_string(), vec![MarkKind::Bold], None),
                (" ".to_string(), vec![], None),
                ("italic".to_string(), vec![MarkKind::Italic], None),
                (" ".to_string(), vec![], None),
                ("code".to_string(), vec![MarkKind::Code], None),
            ]
        );
    }

    #[test]
    fn review_markup_keeps_visible_formatting_and_names_lost_history() {
        let parsed = parse("<p><del>old</del><ins>new</ins></p>");
        let runs = match &parsed.blocks[0].runs[..] {
            [Inline::Text {
                text: old,
                marks: old_marks,
                ..
            }, Inline::Text {
                text: new,
                marks: new_marks,
                ..
            }] => (old, old_marks, new, new_marks),
            other => panic!("review paste must retain its two visible runs, got {other:?}"),
        };
        assert_eq!(runs.0, "old");
        assert_eq!(runs.1, &vec![boolean_mark(MarkKind::Strike)]);
        assert_eq!(runs.2, "new");
        assert_eq!(runs.3, &vec![boolean_mark(MarkKind::Underline)]);
        assert_eq!(
            parsed.warning,
            Some(PasteWarning {
                code: "clipboard-review-degraded",
                message: "Tracked insertions and deletions were pasted as ordinary underlined or struck text; review history was not imported.",
            })
        );
    }

    #[test]
    fn preformatted_and_quoted_html_names_the_unmodelled_block_semantics() {
        for html in [
            "<pre>  source\n  indentation</pre>",
            "<blockquote>quoted source</blockquote>",
            "<blockquote><pre>quoted code</pre></blockquote>",
        ] {
            let parsed = parse(html);
            assert!(
                !parsed.blocks.is_empty(),
                "the visible source words must still arrive: {html}"
            );
            assert_eq!(
                parsed.warning,
                Some(PasteWarning {
                    code: "clipboard-structure-degraded",
                    message: "Preformatted whitespace and block quotations were pasted as ordinary paragraphs; their source block semantics were not imported.",
                }),
                "{html}"
            );
        }
        assert!(parse("<p>ordinary paragraph</p>").warning.is_none());
    }

    /// Google Docs and Word write formatting as inline CSS, not as `<b>`, so
    /// a reader that only knew the tags would lose most real-world pastes.
    #[test]
    fn inline_styles_become_marks() {
        assert_eq!(
            runs(r#"<span style="font-weight:700;font-style:italic">both</span>"#),
            vec![(
                "both".to_string(),
                vec![MarkKind::Bold, MarkKind::Italic],
                None
            )]
        );
        assert_eq!(
            runs(r#"<span style="text-decoration:underline line-through">x</span>"#),
            vec![(
                "x".to_string(),
                vec![MarkKind::Underline, MarkKind::Strike],
                None
            )]
        );
        // A weight below the bold threshold is not bold.
        assert_eq!(
            runs(r#"<span style="font-weight:400">x</span>"#),
            vec![("x".to_string(), vec![], None)]
        );
    }

    #[test]
    fn direct_paragraph_alignment_and_direction_have_typed_model_homes() {
        let parsed = parse(
            r#"<p align="end" style="text-align:center;direction:rtl">right to left</p><p dir="ltr">left to right</p>"#,
        );
        assert_eq!(parsed.blocks.len(), 2);
        // The direct style overrides the legacy semantic alignment attribute,
        // matching normal HTML precedence without attempting a stylesheet
        // cascade. The parser does not invent properties from a class.
        assert_eq!(
            parsed.blocks[0].properties.alignment,
            Some(Alignment::Center)
        );
        assert_eq!(
            parsed.blocks[0].properties.direction,
            Some(TextDirection::RightToLeft)
        );
        assert_eq!(
            parsed.blocks[1].properties.direction,
            Some(TextDirection::LeftToRight)
        );
        assert_eq!(parsed.blocks[1].properties.alignment, None);
        assert!(parse(r#"<p class="rtl center">words</p>"#).blocks[0]
            .properties
            .is_empty());
    }

    #[test]
    fn google_clipboard_rgb_and_highlight_colours_become_canonical_marks() {
        let parsed =
            parse(r#"<span style="color: rgb(17, 34, 51); background-color:#f80">coloured</span>"#);
        let Inline::Text { marks, .. } = &parsed.blocks[0].runs[0] else {
            panic!("clipboard prose must remain a text run");
        };
        assert_eq!(
            marks,
            &[
                Mark {
                    kind: MarkKind::Color,
                    value: Some("#112233".to_string()),
                    expand: MarkExpand::None,
                },
                Mark {
                    kind: MarkKind::Background,
                    value: Some("#ff8800".to_string()),
                    expand: MarkExpand::None,
                },
            ]
        );
        // CSS expressions cannot be made portable to DOCX/ODT; keep the
        // words, but never retain source CSS as a model mark.
        assert!(matches!(
            &parse(r#"<span style="color:var(--unsafe);background:linear-gradient(red,blue)">x</span>"#).blocks[0].runs[0],
            Inline::Text { marks, .. } if marks.is_empty()
        ));
    }

    #[test]
    fn semantic_html_mark_becomes_the_canonical_typed_highlight() {
        let parsed = parse("before <mark>relevant</mark> after");
        assert!(matches!(
            parsed.blocks[0].runs.as_slice(),
            [
                Inline::Text { text: before, marks: first, .. },
                Inline::Text { text, marks, .. },
                Inline::Text { text: after, marks: last, .. },
            ] if before == "before " && first.is_empty()
                && text == "relevant"
                && marks == &vec![Mark {
                    kind: MarkKind::Background,
                    value: Some("#ffff00".to_string()),
                    expand: MarkExpand::None,
                }]
                && after == " after" && last.is_empty()
        ));

        // A direct inner declaration has normal source-order precedence over
        // the semantic default, without admitting a stylesheet cascade.
        let Inline::Text { marks, .. } =
            &parse(r#"<mark><span style="background-color:#123">specific</span></mark>"#).blocks[0]
                .runs[0]
        else {
            panic!("clipboard prose must remain a text run");
        };
        assert_eq!(
            marks,
            &[Mark {
                kind: MarkKind::Background,
                value: Some("#112233".to_string()),
                expand: MarkExpand::None,
            }]
        );
    }
    #[test]
    fn nested_direct_value_styles_replace_their_parent_value() {
        let parsed = parse(
            r#"<span style="color:#f00;font-size:10pt;font-family:Arial"><span style="color:rgb(0, 0, 255);font-size:12pt;font-family:'Times New Roman'">inner</span></span>"#,
        );
        let Inline::Text { marks, .. } = &parsed.blocks[0].runs[0] else {
            panic!("clipboard prose must remain a text run");
        };
        assert_eq!(
            marks,
            &[
                Mark {
                    kind: MarkKind::Color,
                    value: Some("#0000ff".to_string()),
                    expand: MarkExpand::None,
                },
                Mark {
                    kind: MarkKind::Size,
                    value: Some("12".to_string()),
                    expand: MarkExpand::None,
                },
                Mark {
                    kind: MarkKind::Font,
                    value: Some("Times New Roman".to_string()),
                    expand: MarkExpand::None,
                },
            ]
        );
    }

    #[test]
    fn google_clipboard_point_font_size_becomes_a_canonical_size_mark() {
        let parsed = parse(r#"<span style="font-size: 11.0pt">sized</span>"#);
        let Inline::Text { marks, .. } = &parsed.blocks[0].runs[0] else {
            panic!("clipboard prose must remain a text run");
        };
        assert_eq!(
            marks,
            &[Mark {
                kind: MarkKind::Size,
                value: Some("11".to_string()),
                expand: MarkExpand::None,
            }]
        );

        // Relative/display units cannot be resolved without source CSS or a
        // display context, and an absurd absolute size is not a useful model
        // value either. Their words still paste safely.
        for value in ["12px", "1.2em", "120%", "1001pt", "0pt"] {
            assert!(matches!(
                &parse(&format!(r#"<span style="font-size:{value}">x</span>"#)).blocks[0].runs[0],
                Inline::Text { marks, .. } if marks.is_empty()
            ));
        }
    }

    #[test]
    fn google_clipboard_font_family_becomes_a_safe_font_mark() {
        let parsed = parse(r#"<span style="font-family: 'Times New Roman', serif">typed</span>"#);
        let Inline::Text { marks, .. } = &parsed.blocks[0].runs[0] else {
            panic!("clipboard prose must remain a text run");
        };
        assert_eq!(
            marks,
            &[Mark {
                kind: MarkKind::Font,
                value: Some("Times New Roman".to_string()),
                expand: MarkExpand::None,
            }]
        );

        // A generic fallback is display policy rather than a concrete source
        // font, and CSS punctuation must never reach the renderer's style.
        for value in ["sans-serif", "Arial !important", "url(evil)"] {
            assert!(matches!(
                &parse(&format!(r#"<span style="font-family:{value}">x</span>"#)).blocks[0].runs[0],
                Inline::Text { marks, .. } if marks.is_empty()
            ));
        }
    }

    #[test]
    fn raster_data_image_keeps_alt_text_separate_from_its_blob_name() {
        let parsed =
            parse(r#"<img src="data:image/png;base64,AQID" alt="Diagram of sample flow">"#);
        let PastedBlockKind::Image(image) = &parsed.blocks[0].kind else {
            panic!("bounded raster data image must be parsed");
        };
        assert_eq!(image.name, "pasted-image.png");
        assert_eq!(image.alt_text, "Diagram of sample flow");

        let parsed = parse(r#"<img src="data:image/png;base64,AQID" alt="">"#);
        let PastedBlockKind::Image(image) = &parsed.blocks[0].kind else {
            panic!("bounded raster data image must be parsed");
        };
        assert!(
            image.alt_text.is_empty(),
            "explicit decorative alt stays empty"
        );
    }

    #[test]
    fn data_image_inside_a_list_item_preserves_the_trailing_list_fragment() {
        let parsed = parse(
            r#"<ul><li>before<img src="data:image/png;base64,AQID" alt="diagram">after</li></ul>"#,
        );
        assert!(matches!(
            parsed.blocks.as_slice(),
            [
                PastedBlock {
                    runs: before,
                    kind: PastedBlockKind::ListItem { kind: ListKind::Bullet, list_key: first_key, .. },
                    ..
                },
                PastedBlock { kind: PastedBlockKind::Image(_), .. },
                PastedBlock {
                    runs: after,
                    kind: PastedBlockKind::ListItem { kind: ListKind::Bullet, list_key: last_key, .. },
                    ..
                },
            ] if before.len() == 1
                && after.len() == 1
                && matches!(&before[0], Inline::Text { text, .. } if text == "before")
                && matches!(&after[0], Inline::Text { text, .. } if text == "after")
                && first_key == last_key
        ));
    }

    #[test]
    fn data_images_beyond_the_owned_byte_cap_are_named_once() {
        let html = r#"<img src="data:image/png;base64,AQID">"#.repeat(MAX_DATA_IMAGES + 1);
        let parsed = parse(&html);
        assert_eq!(
            parsed
                .blocks
                .iter()
                .filter(|block| matches!(block.kind, PastedBlockKind::Image(_)))
                .count(),
            MAX_DATA_IMAGES
        );
        assert_eq!(
            parsed.warning,
            Some(PasteWarning {
                code: "clipboard-object-degraded",
                message: "Only the first 20 byte-owned clipboard images were imported; later images were skipped.",
            })
        );
    }

    #[test]
    fn canonical_html_time_becomes_an_atomic_date_chip() {
        let parsed = parse(r#"before <time datetime="2024-02-29">2024-02-29</time> after"#);
        assert!(matches!(
            parsed.blocks[0].runs.as_slice(),
            [
                Inline::Text { text: before, .. },
                Inline::DateChip { date, .. },
                Inline::Text { text: after, .. },
            ] if before == "before " && date == "2024-02-29" && after == " after"
        ));
        for html in [
            r#"<time datetime="2023-02-29">2023-02-29</time>"#,
            r#"<time datetime="2024-02-29">29 February 2024</time>"#,
            r#"<time datetime="2024-02-29T12:00:00Z">2024-02-29</time>"#,
            r#"<strong><time datetime="2024-02-29">2024-02-29</time></strong>"#,
        ] {
            assert!(
                !parse(html).blocks[0]
                    .runs
                    .iter()
                    .any(|inline| matches!(inline, Inline::DateChip { .. })),
                "{html} must retain ordinary visible text"
            );
        }
    }

    #[test]
    fn renderer_page_field_tokens_round_trip_without_inventing_a_page_value() {
        let parsed = parse(
            r#"before <span class="doc-field" data-field="page-number"></span> and <span data-field="page-count"></span> after"#,
        );
        assert!(matches!(
            parsed.blocks[0].runs.as_slice(),
            [
                Inline::Text { text: before, .. },
                Inline::PageNumber { field: PageNumberField::CurrentPage, .. },
                Inline::Text { text: middle, .. },
                Inline::PageNumber { field: PageNumberField::PageCount, .. },
                Inline::Text { text: after, .. },
            ] if before == "before " && middle == " and " && after == " after"
        ));

        // A closed renderer token is intentionally stricter than merely
        // seeing a familiar data attribute: source text may never disappear.
        for html in [
            r#"<span data-field="page-number">7</span>"#,
            r#"<span data-field="section-number"></span>"#,
            r#"<strong><span data-field="page-count"></span></strong>"#,
        ] {
            assert!(
                !parse(html)
                    .blocks
                    .iter()
                    .flat_map(|block| &block.runs)
                    .any(|inline| matches!(inline, Inline::PageNumber { .. })),
                "{html} must not manufacture a page field"
            );
        }
        assert_eq!(text_of(r#"<span data-field="page-number">7</span>"#), "7");
    }

    #[test]
    fn semantic_horizontal_rule_becomes_a_content_free_block() {
        let parsed = parse("<p>before</p><hr style=\"border:99px solid red\"><p>after</p>");
        assert!(matches!(
            parsed.blocks.as_slice(),
            [
                PastedBlock { kind: PastedBlockKind::Paragraph, .. },
                PastedBlock { runs, kind: PastedBlockKind::HorizontalRule, .. },
                PastedBlock { kind: PastedBlockKind::Paragraph, .. },
            ] if runs.is_empty()
        ));
        assert_eq!(
            parsed
                .blocks
                .iter()
                .map(|block| {
                    block
                        .runs
                        .iter()
                        .map(|inline| match inline {
                            Inline::Text { text, .. } | Inline::Link { text, .. } => text.as_str(),
                            _ => "",
                        })
                        .collect::<String>()
                })
                .collect::<Vec<_>>(),
            ["before", "", "after"]
        );
    }

    #[test]
    fn explicit_checkbox_inputs_make_checked_and_unchecked_checklist_items() {
        let parsed = parse(
            "<ul><li><input type=checkbox checked>done</li><li><input type=checkbox>todo</li></ul>",
        );
        assert!(matches!(
            parsed.blocks.as_slice(),
            [
                PastedBlock {
                    kind: PastedBlockKind::ListItem {
                        kind: ListKind::Checklist { checked: true },
                        level: 0,
                        ..
                    },
                    ..
                },
                PastedBlock {
                    kind: PastedBlockKind::ListItem {
                        kind: ListKind::Checklist { checked: false },
                        level: 0,
                        ..
                    },
                    ..
                },
            ]
        ));
        assert_eq!(text_of("<ul><li><input type=checkbox checked>done</li><li><input type=checkbox>todo</li></ul>"), "done\ntodo");
        assert!(matches!(
            parse("<p><input type=checkbox checked>not a task</p>").blocks[0].kind,
            PastedBlockKind::Paragraph
        ));
    }

    #[test]
    fn nested_marks_accumulate_and_close_with_their_element() {
        assert_eq!(
            runs("<b>one<i>two</i>three</b>four"),
            vec![
                ("one".to_string(), vec![MarkKind::Bold], None),
                (
                    "two".to_string(),
                    vec![MarkKind::Bold, MarkKind::Italic],
                    None
                ),
                ("three".to_string(), vec![MarkKind::Bold], None),
                ("four".to_string(), vec![], None),
            ]
        );
    }

    #[test]
    fn block_elements_end_paragraphs_but_br_is_a_soft_line_break() {
        assert_eq!(text_of("<p>one</p><p>two</p>"), "one\ntwo");
        assert_eq!(text_of("one<br>two"), "one\ntwo");
        assert_eq!(text_of("<div>one</div><div>two</div>"), "one\ntwo");
        let parsed = parse("one<br><strong>two</strong>");
        assert_eq!(parsed.blocks.len(), 1, "a break must not split a paragraph");
        assert!(matches!(
            parsed.blocks[0].runs.as_slice(),
            [
                Inline::Text { text, marks, .. },
                Inline::Text { text: bold, marks: bold_marks, .. },
            ] if text == "one\n" && marks.is_empty() && bold == "two" && bold_marks.iter().any(|mark| mark.kind == MarkKind::Bold)
        ));
    }

    // ---- The input is hostile -------------------------------------------

    /// Nothing from a `<script>` reaches the document, including its text.
    #[test]
    fn script_and_style_contents_are_dropped_entirely() {
        assert_eq!(text_of("<script>alert(1)</script>keep"), "keep");
        assert_eq!(text_of("<style>body{}</style>keep"), "keep");
        assert_eq!(
            text_of("<div><script>var x = '</script>';</script></div>after"),
            "';\nafter"
        );
    }

    /// Markup that arrives as text stays text, and is stored as the
    /// characters it is — the renderer escapes it on the way out, so there
    /// is nothing here for it to become.
    #[test]
    fn markup_in_text_position_stays_text() {
        assert_eq!(
            text_of("&lt;img src=x onerror=alert(1)&gt;"),
            "<img src=x onerror=alert(1)>"
        );
    }

    #[test]
    fn a_javascript_href_loses_its_link_and_keeps_its_text() {
        assert_eq!(
            runs(r#"<a href="javascript:alert(1)">click</a>"#),
            vec![("click".to_string(), vec![], None)]
        );
        assert_eq!(
            runs(r#"<a href="data:text/html,<script>">click</a>"#),
            vec![("click".to_string(), vec![], None)]
        );
        assert_eq!(
            runs(r#"<a href="https://example.com/x">click</a>"#),
            vec![(
                "click".to_string(),
                vec![],
                Some("https://example.com/x".to_string())
            )]
        );
    }

    /// `data-href` must not be read as `href`.
    #[test]
    fn an_attribute_name_has_to_stand_alone() {
        assert_eq!(
            runs(r#"<a data-href="https://example.com">x</a>"#),
            vec![("x".to_string(), vec![], None)]
        );
    }

    /// Deep nesting is bounded, and the text still arrives.
    #[test]
    fn deep_nesting_is_bounded_without_losing_the_text() {
        let html = format!("{}deep{}", "<b>".repeat(5_000), "</b>".repeat(5_000));
        let parsed = parse(&html);
        assert_eq!(text_of(&html), "deep");
        assert!(!parsed.blocks.is_empty());
    }

    #[test]
    fn comments_and_doctypes_carry_no_content() {
        assert_eq!(text_of("<!doctype html><!-- <b>x</b> -->keep"), "keep");
        assert_eq!(text_of("<!-- unterminated"), "");
    }

    #[test]
    fn whitespace_between_tags_collapses() {
        assert_eq!(
            text_of("<p>\n    one   two\n</p>"),
            "one two",
            "source indentation must not arrive as runs of spaces"
        );
    }

    #[test]
    fn a_control_character_entity_is_not_decoded() {
        assert_eq!(text_of("a&#0;b"), "a&#0;b");
    }

    #[test]
    fn unbalanced_markup_does_not_lose_text() {
        assert_eq!(text_of("<b>one</i>two"), "onetwo");
        assert_eq!(text_of("a < b"), "a < b");
    }

    #[test]
    fn html_with_no_text_produces_no_blocks() {
        assert!(parse("").blocks.is_empty());
        assert!(parse("<p></p><div></div>").blocks.is_empty());
        assert!(parse("<script>x</script>").blocks.is_empty());
    }

    #[test]
    fn html_table_row_cap_is_named_instead_of_silently_truncating() {
        let html = format!("<table>{}</table>", "<tr><td>x</td></tr>".repeat(201));
        let parsed = parse(&html);
        assert!(matches!(
            parsed.blocks.as_slice(),
            [PastedBlock {
                kind: PastedBlockKind::Table { rows },
                ..
            }] if rows.len() == 200
        ));
        assert_eq!(
            parsed.warning,
            Some(PasteWarning {
                code: "clipboard-table-degraded",
                message:
                    "Only the first 200 HTML table rows were imported; later rows were skipped.",
            })
        );
    }

    #[test]
    fn over_cap_standalone_table_never_reaches_the_table_fast_path() {
        // The closing tag sits past the source cap. The table scanner must
        // only see the bounded prefix, so it cannot accept a standalone table
        // by scanning the hostile suffix.
        let row = "<tr><td>x</td></tr>";
        let count = (MAX_INPUT_BYTES / row.len()) + 2;
        let html = format!("<table>{}</table>", row.repeat(count));
        let parsed = parse(&html);
        assert!(parsed.blocks.len() <= MAX_BLOCKS);
        assert!(parsed
            .blocks
            .iter()
            .all(|block| !matches!(block.kind, PastedBlockKind::Table { .. })));
    }

    #[test]
    fn html_table_cell_cap_is_exact_and_names_truncation() {
        for (count, expected_warning) in [
            (50, None),
            (100, None),
            (
                101,
                Some("Only the first 100 HTML table cells in a row were imported; later cells were skipped."),
            ),
        ] {
            let html = format!("<table><tr>{}</tr></table>", "<td>x</td>".repeat(count));
            let parsed = parse(&html);
            assert!(matches!(
                parsed.blocks.as_slice(),
                [PastedBlock { kind: PastedBlockKind::Table { rows }, .. }]
                    if rows.len() == 1 && rows[0].cells.len() == count.min(100)
            ));
            assert_eq!(parsed.warning.map(|warning| warning.message), expected_warning);
        }
    }

    #[test]
    fn html_table_optional_cell_end_tags_preserve_each_cell() {
        let parsed = parse("<table><tr><th>Head<td>First<td>Second</tr></table>");
        let [PastedBlock {
            kind: PastedBlockKind::Table { rows },
            ..
        }] = parsed.blocks.as_slice()
        else {
            panic!("a standalone table must stay a table");
        };
        assert_eq!(rows.len(), 1);
        assert!(!rows[0].header, "mixed th/td rows are not header rows");
        assert_eq!(rows[0].cells.len(), 3);
        let texts = rows[0]
            .cells
            .iter()
            .map(|cell| {
                cell.iter()
                    .flat_map(|block| block.runs.iter())
                    .filter_map(|inline| match inline {
                        Inline::Text { text, .. } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        assert_eq!(texts, ["Head", "First", "Second"]);
        assert!(parsed.warning.is_none());
    }

    #[test]
    fn html_table_optional_row_end_tags_preserve_every_row() {
        // Every omitted end tag below is browser-valid: the next cell, row,
        // table section, or outer-table EOF supplies its implied close.
        let parsed = parse(
            "<table><thead><tr><th>Head A<th>Head B<tbody><tr><td>one<td>two<tr><td>three<td>four</table>",
        );
        let [PastedBlock {
            kind: PastedBlockKind::Table { rows },
            ..
        }] = parsed.blocks.as_slice()
        else {
            panic!("a browser-valid compact table must stay a table");
        };
        assert_eq!(rows.len(), 3);
        assert!(rows[0].header);
        assert!(rows[1..].iter().all(|row| !row.header));
        let cell_text = |row: &PastedTableRow| {
            row.cells
                .iter()
                .map(|cell| {
                    cell.iter()
                        .flat_map(|block| block.runs.iter())
                        .filter_map(|inline| match inline {
                            Inline::Text { text, .. } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(cell_text(&rows[0]), ["Head A", "Head B"]);
        assert_eq!(cell_text(&rows[1]), ["one", "two"]);
        assert_eq!(cell_text(&rows[2]), ["three", "four"]);
        assert!(parsed.warning.is_none());
    }

    #[test]
    fn html_thead_marks_td_rows_as_headers_without_leaking_to_body_rows() {
        // Office clipboards commonly use `<td>` throughout, relying on the
        // enclosing semantic section rather than emitting `<th>` per cell.
        // The header bit has a direct model home, so do not discard it.
        let parsed = parse(
            "<table><thead><tr><td>Name</td><td>Score</td></tr></thead><tr><td>Ada</td><td>42</td></tr></table>",
        );
        let [PastedBlock {
            kind: PastedBlockKind::Table { rows },
            ..
        }] = parsed.blocks.as_slice()
        else {
            panic!("a standalone table must stay a table");
        };
        assert_eq!(rows.len(), 2);
        assert!(rows[0].header, "thead is semantic header-row state");
        assert!(
            !rows[1].header,
            "a following row outside thead must not inherit header state"
        );
        let cell_text = |cell: &Vec<PastedBlock>| {
            cell.iter()
                .flat_map(|block| block.runs.iter())
                .filter_map(|inline| match inline {
                    Inline::Text { text, .. } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<String>()
        };
        assert_eq!(cell_text(&rows[0].cells[0]), "Name");
        assert_eq!(cell_text(&rows[0].cells[1]), "Score");
        assert_eq!(cell_text(&rows[1].cells[0]), "Ada");
        assert_eq!(cell_text(&rows[1].cells[1]), "42");
        assert!(parsed.warning.is_none());
    }

    #[test]
    fn bounded_presentation_mathml_becomes_a_native_inline_equation() {
        let parsed = parse(
            "before <math><mfrac><mi>x</mi><mrow><mn>1</mn><mo>+</mo><mi>y</mi></mrow></mfrac></math> after tail",
        );
        assert!(parsed.warning.is_none());
        assert_eq!(parsed.blocks.len(), 1);
        assert!(matches!(
            &parsed.blocks[0].runs[1],
            Inline::Equation { equation, .. }
                if equation.source_format == EquationSourceFormat::LatexLike
                    && equation.source == r"\frac{x}{1+y}"
        ));
    }

    #[test]
    fn mathml_with_semantic_or_active_content_degrades_to_tokens_and_warns() {
        let parsed = parse(
            "<math><semantics><mi>x</mi><annotation encoding=\"application/x-tex\">evil</annotation></semantics><script>alert(1)</script></math>",
        );
        assert_eq!(text_of("<math><semantics><mi>x</mi><annotation encoding=\"application/x-tex\">evil</annotation></semantics><script>alert(1)</script></math>"), "x");
        assert_eq!(
            parsed.warning,
            Some(PasteWarning {
                code: "clipboard-object-degraded",
                message: "MathML could not be represented faithfully as an equation; its visible text was pasted instead.",
            })
        );
        assert!(parsed.blocks[0]
            .runs
            .iter()
            .all(|run| !matches!(run, Inline::Equation { .. })));
    }

    #[test]
    fn mathml_attributes_are_not_silently_accepted() {
        let parsed = parse("<math><mi href=\"https://example.invalid\">x</mi></math>");
        assert!(parsed.warning.is_some());
        assert_eq!(
            text_of("<math><mi href=\"https://example.invalid\">x</mi></math>"),
            "x"
        );
    }
}
