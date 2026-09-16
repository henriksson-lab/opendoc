//! Sanitiser for the MathML that [`math_core`] produces.
//!
//! # Why this exists
//!
//! Everywhere else in this crate the renderer builds markup itself and escapes
//! every value it did not write ([`crate::escape_html`]). Equations are the one
//! place where a *third-party* library hands back a string of markup that is
//! then concatenated into the document body verbatim. That makes `math_core` a
//! trusted producer by accident, and it is not one: version 0.8.2 does not
//! escape the argument of `\operatorname`, so the LaTeX source
//!
//! ```text
//! \operatorname{</math><img/src=x/onerror=alert(1)>}
//! ```
//!
//! comes back as markup that closes the `<math>` element and opens an `<img>`
//! with an event handler. The LaTeX source is authored content — it arrives in
//! imported `.json` files, over the collaboration transport and from any
//! repository a user is handed — so that is a stored cross-site scripting hole
//! in every surface that renders a document.
//!
//! # What it does
//!
//! The converter's output is re-parsed as XML and *re-serialised from scratch*
//! against an allowlist. Nothing from the input is ever copied through as
//! markup:
//!
//! * element and attribute names are written from the `const` tables below, so
//!   a name can never carry a delimiter;
//! * text, and every attribute value, is written through [`escape_html`];
//! * `style`, `class`, `id` and `href` get value rules of their own, because
//!   escaping alone would still leave `url(...)` exfiltration and DOM id
//!   collisions;
//! * anything not on the allowlist — an unknown element, an unknown attribute,
//!   a comment, a processing instruction, unbalanced tags — rejects the whole
//!   equation.
//!
//! Rejection is deliberate rather than best-effort stripping: the caller
//! degrades to the escaped LaTeX source and emits a [`ModelWarning`], which is
//! the same trade the rest of the renderer makes, and it means a future
//! `math_core` that starts emitting something new is *reported* rather than
//! silently dropped or silently trusted.

use quick_xml::events::attributes::Attribute;
use quick_xml::events::Event;
use quick_xml::Reader;

use crate::escape_html;

/// Prefix put in front of every `id` (and every fragment `href`) that comes out
/// of the converter.
///
/// `\label{foo}` makes `math_core` emit a bare `id="foo"`, which would collide
/// with the frontend's own element ids and is reachable from document content.
/// Namespacing keeps intra-equation references working without letting an
/// equation name an element the application uses.
const ID_NAMESPACE: &str = "opendoc-math-";

/// Longest attribute value accepted. Nothing the converter emits comes close;
/// the cap exists so a pathological input cannot turn into an enormous
/// attribute.
const MAX_ATTRIBUTE_VALUE: usize = 256;

/// Deepest element nesting accepted, so a pathological equation cannot drive
/// the re-serialiser's stack (the stack here is a `Vec`, but the cap also
/// bounds what the browser is asked to lay out).
const MAX_DEPTH: usize = 128;

/// Elements the converter is allowed to produce.
///
/// MathML Core presentation elements, `semantics`/`annotation` (the crate is
/// configured to embed the LaTeX source as an annotation), and the four HTML
/// elements `math_core` emits inside `<mtext>` for text-mode fonts.
const ALLOWED_ELEMENTS: &[&str] = &[
    // MathML
    "a",
    "annotation",
    "maction",
    "math",
    "menclose",
    "merror",
    "mfrac",
    "mi",
    "mlabeledtr",
    "mmultiscripts",
    "mn",
    "mo",
    "mover",
    "mpadded",
    "mphantom",
    "mprescripts",
    "mroot",
    "mrow",
    "ms",
    "mspace",
    "msqrt",
    "mstyle",
    "msub",
    "msubsup",
    "msup",
    "mtable",
    "mtd",
    "mtext",
    "mtr",
    "munder",
    "munderover",
    "none",
    "semantics",
    // HTML, emitted inside <mtext> for \textsc, \textsf, \textrm, \texttt and
    // the bold/italic text-mode commands.
    "b",
    "code",
    "i",
    "span",
];

/// Attributes the converter is allowed to produce.
///
/// Presentational only. Nothing here takes a URL except `href`, which has a
/// rule of its own in [`sanitize_attribute`]; `definitionURL`, `src`, `xlink:*`
/// and every `on*` handler are absent on purpose.
const ALLOWED_ATTRIBUTES: &[&str] = &[
    "accent",
    "accentunder",
    "align",
    "class",
    "columnalign",
    "columnlines",
    "columnspacing",
    "columnspan",
    "depth",
    "dir",
    "display",
    "displaystyle",
    "encoding",
    "fence",
    "form",
    "frame",
    "framespacing",
    "height",
    "href",
    "id",
    "largeop",
    "linethickness",
    "lspace",
    "mathbackground",
    "mathcolor",
    "mathsize",
    "mathvariant",
    "maxsize",
    "minsize",
    "movablelimits",
    "notation",
    "rowalign",
    "rowlines",
    "rowspacing",
    "rowspan",
    "rspace",
    "scriptlevel",
    "separator",
    "stretchy",
    "style",
    "symmetric",
    "voffset",
    "width",
];

/// CSS properties the converter is allowed to set through `style`.
///
/// Taken from what `math_core` 0.8.2 actually writes (table rules, alignment,
/// padding, `\color`, text-mode font sizing) plus the symmetric partners of
/// those properties. A declaration outside this set rejects the equation; a
/// value outside [`is_style_value`] does too. Both matter: `style` is the one
/// allowlisted attribute whose *value* is a language, and `background:url(…)`
/// inside it is a working exfiltration channel in an export that carries no
/// content-security policy.
const ALLOWED_STYLE_PROPERTIES: &[&str] = &[
    "background-color",
    "border-bottom",
    "border-left",
    "border-right",
    "border-top",
    "color",
    "font-family",
    "font-size",
    "font-style",
    "font-variant-caps",
    "font-weight",
    "height",
    "justify-items",
    "line-height",
    "margin-bottom",
    "margin-left",
    "margin-right",
    "margin-top",
    "padding-bottom",
    "padding-left",
    "padding-right",
    "padding-top",
    "text-align",
    "vertical-align",
    "width",
];

/// Why a converter result was refused. Becomes the equation's
/// `data-equation-error` and the text of a [`opendoc_core::ModelWarning`].
pub(crate) struct MathMlRejected(pub String);

fn reject(reason: impl Into<String>) -> MathMlRejected {
    MathMlRejected(reason.into())
}

/// Re-serialise `markup` from the allowlist above, or say why it could not be.
///
/// Pure: the same input always produces the same output, and nothing outside
/// the allowlist tables can appear in the result.
pub(crate) fn sanitize_mathml(markup: &str) -> Result<String, MathMlRejected> {
    let mut reader = Reader::from_str(markup);
    reader.config_mut().trim_text(false);
    // On by default, and load-bearing: this is what turns an injected
    // `</math>` into a rejection rather than a silently truncated tree.
    reader.config_mut().check_end_names = true;
    reader.config_mut().expand_empty_elements = false;

    let mut out = String::with_capacity(markup.len() + 32);
    let mut open: Vec<&'static str> = Vec::new();
    let mut root_seen = false;

    loop {
        let event = reader
            .read_event()
            .map_err(|err| reject(format!("converter output is not well-formed XML: {err}")))?;
        match event {
            Event::Start(start) => {
                let name = allowed_element(start.name().as_ref())?;
                if open.is_empty() {
                    if root_seen {
                        return Err(reject("converter output has more than one root element"));
                    }
                    if name != "math" {
                        return Err(reject(format!(
                            "converter output is rooted at <{name}> rather than <math>"
                        )));
                    }
                    root_seen = true;
                }
                if open.len() >= MAX_DEPTH {
                    return Err(reject(format!(
                        "converter output nests deeper than {MAX_DEPTH} elements"
                    )));
                }
                write_open_tag(&mut out, name, start.attributes())?;
                out.push('>');
                open.push(name);
            }
            Event::Empty(start) => {
                let name = allowed_element(start.name().as_ref())?;
                if open.is_empty() {
                    if root_seen {
                        return Err(reject("converter output has more than one root element"));
                    }
                    if name != "math" {
                        return Err(reject(format!(
                            "converter output is rooted at <{name}> rather than <math>"
                        )));
                    }
                    root_seen = true;
                }
                write_open_tag(&mut out, name, start.attributes())?;
                // Written as an open/close pair rather than `<name/>`: the same
                // string is parsed as HTML in the app and as XHTML in an
                // export, and only this form means the same thing in both.
                out.push_str("></");
                out.push_str(name);
                out.push('>');
            }
            Event::End(end) => {
                let name = open
                    .pop()
                    .ok_or_else(|| reject("converter output closes an element it never opened"))?;
                // `check_end_names` already proved the names match; this is the
                // second half of that proof, against the allowlisted name we
                // actually wrote.
                if allowed_element(end.name().as_ref())? != name {
                    return Err(reject("converter output closes elements out of order"));
                }
                out.push_str("</");
                out.push_str(name);
                out.push('>');
            }
            Event::Text(text) => {
                let decoded = text.decode().map_err(|err| {
                    reject(format!("converter output has undecodable text: {err}"))
                })?;
                out.push_str(&escape_html(decoded.as_ref()));
            }
            Event::CData(cdata) => {
                let decoded = cdata.decode().map_err(|err| {
                    reject(format!("converter output has undecodable CDATA: {err}"))
                })?;
                out.push_str(&escape_html(decoded.as_ref()));
            }
            Event::GeneralRef(reference) => {
                let resolved = match reference.resolve_char_ref() {
                    Ok(Some(ch)) => ch.to_string(),
                    _ => {
                        let name = reference.decode().map_err(|err| {
                            reject(format!("converter output has an undecodable entity: {err}"))
                        })?;
                        match name.as_ref() {
                            "amp" => "&".to_string(),
                            "lt" => "<".to_string(),
                            "gt" => ">".to_string(),
                            "quot" => "\"".to_string(),
                            "apos" => "'".to_string(),
                            other => {
                                return Err(reject(format!(
                                    "converter output references the unknown entity &{other};"
                                )))
                            }
                        }
                    }
                };
                out.push_str(&escape_html(&resolved));
            }
            // None of these appear in `math_core`'s output, and each of them is
            // a way to smuggle markup past a reader that skips it.
            Event::Comment(_) => return Err(reject("converter output contains a comment")),
            Event::PI(_) => {
                return Err(reject("converter output contains a processing instruction"))
            }
            Event::Decl(_) => return Err(reject("converter output contains an XML declaration")),
            Event::DocType(_) => {
                return Err(reject("converter output contains a doctype declaration"))
            }
            Event::Eof => break,
        }
    }

    if !open.is_empty() {
        return Err(reject("converter output leaves an element unclosed"));
    }
    if !root_seen {
        return Err(reject("converter output has no <math> element"));
    }
    Ok(out)
}

/// The allowlisted name for `raw`, as a `'static` string.
///
/// Returning the table's own `&'static str` rather than the parsed bytes is the
/// point: every name written into the output comes from [`ALLOWED_ELEMENTS`],
/// so no parsed byte can reach the markup as a name.
fn allowed_element(raw: &[u8]) -> Result<&'static str, MathMlRejected> {
    let name = std::str::from_utf8(raw)
        .map_err(|_| reject("converter output has a non-UTF-8 element name"))?;
    ALLOWED_ELEMENTS
        .iter()
        .copied()
        .find(|candidate| *candidate == name)
        .ok_or_else(|| reject(format!("converter output contains the element <{name}>")))
}

fn allowed_attribute(raw: &[u8]) -> Result<&'static str, MathMlRejected> {
    let name = std::str::from_utf8(raw)
        .map_err(|_| reject("converter output has a non-UTF-8 attribute name"))?;
    ALLOWED_ATTRIBUTES
        .iter()
        .copied()
        .find(|candidate| *candidate == name)
        .ok_or_else(|| reject(format!("converter output sets the attribute {name}")))
}

fn write_open_tag(
    out: &mut String,
    name: &'static str,
    attributes: quick_xml::events::attributes::Attributes<'_>,
) -> Result<(), MathMlRejected> {
    out.push('<');
    out.push_str(name);
    for attribute in attributes {
        let attribute = attribute
            .map_err(|err| reject(format!("converter output has a malformed attribute: {err}")))?;
        let Some((key, value)) = sanitize_attribute(name, &attribute)? else {
            continue;
        };
        out.push(' ');
        out.push_str(key);
        out.push_str("=\"");
        out.push_str(&escape_html(&value));
        out.push('"');
    }
    Ok(())
}

/// One attribute, checked and normalised, or `None` for one that is dropped
/// without prejudice (namespace declarations, which mean nothing once the
/// markup is inlined into an HTML document).
fn sanitize_attribute(
    element: &str,
    attribute: &Attribute<'_>,
) -> Result<Option<(&'static str, String)>, MathMlRejected> {
    let raw_key = attribute.key.as_ref();
    if raw_key == b"xmlns" || raw_key.starts_with(b"xmlns:") {
        return Ok(None);
    }
    let key = allowed_attribute(raw_key)?;
    let value = attribute
        .normalized_value(quick_xml::XmlVersion::Implicit1_0)
        .map_err(|err| reject(format!("converter output has an undecodable {key}: {err}")))?
        .into_owned();
    if value.len() > MAX_ATTRIBUTE_VALUE {
        return Err(reject(format!(
            "converter output sets a {key} longer than {MAX_ATTRIBUTE_VALUE} characters"
        )));
    }
    if value.chars().any(|ch| ch.is_control()) {
        return Err(reject(format!(
            "converter output sets a {key} containing a control character"
        )));
    }
    match key {
        "style" => Ok(Some((key, sanitize_style(&value)?))),
        "class" => {
            if value.is_empty()
                || !value
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | ' '))
            {
                return Err(reject(format!(
                    "converter output sets the class {value:?}, which is not a plain class list"
                )));
            }
            Ok(Some((key, value)))
        }
        "id" => Ok(Some((key, format!("{ID_NAMESPACE}{}", id_body(&value)?)))),
        "href" => {
            if element != "a" {
                return Err(reject(format!("converter output sets href on <{element}>")));
            }
            // Only a reference to a target inside this same equation. An
            // absolute URL here would be a link the document's author chose
            // and the reader did not, which is not something a formula gets to
            // introduce.
            let Some(fragment) = value.strip_prefix('#') else {
                return Err(reject(format!(
                    "converter output links to {value:?}, which is not a fragment reference"
                )));
            };
            Ok(Some((
                key,
                format!("#{ID_NAMESPACE}{}", id_body(fragment)?),
            )))
        }
        _ => Ok(Some((key, value))),
    }
}

/// The part of an `id` (or of a fragment reference) that may be kept.
fn id_body(value: &str) -> Result<&str, MathMlRejected> {
    if value.is_empty()
        || !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | ':'))
    {
        return Err(reject(format!(
            "converter output uses {value:?} as an element id"
        )));
    }
    Ok(value)
}

/// Rebuild a `style` value from allowlisted declarations.
///
/// Rebuilt rather than filtered: the output is assembled from the matched
/// property name and a value that passed [`is_style_value`], so no part of the
/// input survives as CSS syntax.
fn sanitize_style(value: &str) -> Result<String, MathMlRejected> {
    let mut out = String::with_capacity(value.len());
    for declaration in value.split(';') {
        let declaration = declaration.trim();
        if declaration.is_empty() {
            continue;
        }
        let Some((property, raw)) = declaration.split_once(':') else {
            return Err(reject(format!(
                "converter output has the style declaration {declaration:?}"
            )));
        };
        let property = property.trim();
        let Some(property) = ALLOWED_STYLE_PROPERTIES
            .iter()
            .copied()
            .find(|candidate| *candidate == property)
        else {
            return Err(reject(format!(
                "converter output sets the CSS property {property:?}"
            )));
        };
        let raw = raw.trim();
        if !is_style_value(raw) {
            return Err(reject(format!(
                "converter output sets {property} to {raw:?}"
            )));
        }
        out.push_str(property);
        out.push(':');
        out.push_str(raw);
        out.push(';');
    }
    if out.is_empty() {
        return Err(reject("converter output has an empty style attribute"));
    }
    Ok(out)
}

/// Characters a CSS value may consist of.
///
/// Deliberately without `(`, `)`, `\`, `/`, `*`, `@`, `"` and `'`: those are
/// what `url(...)`, CSS escapes, comments and at-rules are made of, and nothing
/// `math_core` emits needs any of them.
fn is_style_value(value: &str) -> bool {
    !value.is_empty()
        && value.chars().all(|ch| {
            ch.is_ascii_alphanumeric() || matches!(ch, ' ' | '.' | ',' | '%' | '#' | '+' | '-')
        })
}
