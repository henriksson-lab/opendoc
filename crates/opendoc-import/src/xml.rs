//! Minimal DOM built on top of `quick-xml`.
//!
//! WordprocessingML parts are traversed structurally, so a small in-memory tree
//! keyed by local element names is far simpler to work with than a streaming
//! event loop. Names are matched by their local part so that documents using
//! non-standard namespace prefixes (or none at all) still parse.

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct XmlElement {
    pub(crate) prefix: Option<String>,
    pub(crate) local: String,
    pub(crate) attrs: Vec<XmlAttr>,
    pub(crate) children: Vec<XmlNode>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct XmlAttr {
    pub(crate) prefix: Option<String>,
    pub(crate) local: String,
    pub(crate) value: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum XmlNode {
    Element(XmlElement),
    Text(String),
}

#[derive(Debug)]
pub(crate) struct XmlError(pub(crate) String);

impl std::fmt::Display for XmlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Deepest element nesting this reader will build a tree for.
///
/// The parse loop is iterative, but the tree it builds is not: every walk over
/// it — `collect_text`, `collect_descendants`, `strip_unfurnishable`,
/// `docx::convert::walk_blocks` — recurses, and so does the derived `Drop` on
/// [`XmlElement`]. A 1.6 KB `.docx` holding 40,000 nested elements used to
/// abort the process with `fatal runtime error: stack overflow`, which is a
/// `SIGABRT` no caller can catch and which takes the whole Tauri host with it.
///
/// Capping the *depth of the tree* fixes every one of those walks at once,
/// which is why the limit lives here rather than in each of them. 256 is far
/// past anything real: a WordprocessingML body nests three levels per nested
/// table plus a dozen or so for a drawing, so this allows tables nested some
/// seventy deep.
pub(crate) const MAX_XML_DEPTH: usize = 256;

/// Largest XML part this reader will accept.
///
/// The zip reader screens declared sizes before it inflates anything
/// ([`crate::docx::package`]); this is the same ceiling for input that did not
/// come through a zip at all.
pub(crate) const MAX_XML_BYTES: usize = 64 * 1024 * 1024;

pub(crate) fn parse_xml_bytes(bytes: &[u8]) -> Result<XmlElement, XmlError> {
    if bytes.len() > MAX_XML_BYTES {
        return Err(XmlError(format!(
            "XML part is {} bytes, over the {MAX_XML_BYTES}-byte limit",
            bytes.len()
        )));
    }
    let text = String::from_utf8_lossy(bytes);
    parse_xml(&text)
}

pub(crate) fn parse_xml(text: &str) -> Result<XmlElement, XmlError> {
    let mut reader = Reader::from_str(text);
    reader.config_mut().trim_text(false);
    let mut stack: Vec<XmlElement> = Vec::new();
    let mut root: Option<XmlElement> = None;
    loop {
        let event = reader
            .read_event()
            .map_err(|err| XmlError(format!("malformed XML: {err}")))?;
        match event {
            Event::Start(start) => {
                if stack.len() >= MAX_XML_DEPTH {
                    return Err(XmlError(format!(
                        "XML nests deeper than {MAX_XML_DEPTH} elements"
                    )));
                }
                stack.push(element_from_start(&start)?)
            }
            Event::Empty(start) => {
                let element = element_from_start(&start)?;
                attach(&mut stack, &mut root, element)?;
            }
            Event::End(_) => {
                let element = stack
                    .pop()
                    .ok_or_else(|| XmlError("unexpected closing tag".to_string()))?;
                attach(&mut stack, &mut root, element)?;
            }
            Event::Text(text) => {
                let decoded = text
                    .decode()
                    .map_err(|err| XmlError(format!("undecodable XML text: {err}")))?;
                push_text(&mut stack, &decoded);
            }
            Event::CData(cdata) => {
                let decoded = cdata
                    .decode()
                    .map_err(|err| XmlError(format!("undecodable XML CDATA: {err}")))?;
                push_text(&mut stack, &decoded);
            }
            Event::GeneralRef(reference) => {
                let resolved = match reference.resolve_char_ref() {
                    Ok(Some(ch)) => ch.to_string(),
                    _ => {
                        let name = reference
                            .decode()
                            .map_err(|err| XmlError(format!("undecodable XML entity: {err}")))?;
                        match name.as_ref() {
                            "amp" => "&".to_string(),
                            "lt" => "<".to_string(),
                            "gt" => ">".to_string(),
                            "quot" => "\"".to_string(),
                            "apos" => "'".to_string(),
                            other => format!("&{other};"),
                        }
                    }
                };
                push_text(&mut stack, &resolved);
            }
            Event::Eof => break,
            Event::Comment(_) | Event::Decl(_) | Event::PI(_) | Event::DocType(_) => {}
        }
    }
    if !stack.is_empty() {
        return Err(XmlError("unclosed XML element".to_string()));
    }
    root.ok_or_else(|| XmlError("XML document has no root element".to_string()))
}

fn attach(
    stack: &mut [XmlElement],
    root: &mut Option<XmlElement>,
    element: XmlElement,
) -> Result<(), XmlError> {
    if let Some(parent) = stack.last_mut() {
        parent.children.push(XmlNode::Element(element));
        Ok(())
    } else if root.is_none() {
        *root = Some(element);
        Ok(())
    } else {
        Err(XmlError(
            "XML document has more than one root element".to_string(),
        ))
    }
}

fn push_text(stack: &mut [XmlElement], text: &str) {
    if text.is_empty() {
        return;
    }
    let Some(parent) = stack.last_mut() else {
        return;
    };
    if let Some(XmlNode::Text(existing)) = parent.children.last_mut() {
        existing.push_str(text);
    } else {
        parent.children.push(XmlNode::Text(text.to_string()));
    }
}

fn element_from_start(start: &BytesStart<'_>) -> Result<XmlElement, XmlError> {
    let name = start.name();
    let (prefix, local) = split_qname(name.as_ref());
    let mut attrs = Vec::new();
    for attr in start.attributes() {
        let attr = attr.map_err(|err| XmlError(format!("malformed XML attribute: {err}")))?;
        let (attr_prefix, attr_local) = split_qname(attr.key.as_ref());
        if attr_local == "xmlns" || attr_prefix.as_deref() == Some("xmlns") {
            continue;
        }
        let value = attr
            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
            .map_err(|err| XmlError(format!("malformed XML attribute value: {err}")))?
            .into_owned();
        attrs.push(XmlAttr {
            prefix: attr_prefix,
            local: attr_local,
            value,
        });
    }
    Ok(XmlElement {
        prefix,
        local,
        attrs,
        children: Vec::new(),
    })
}

fn split_qname(raw: &[u8]) -> (Option<String>, String) {
    let raw = String::from_utf8_lossy(raw);
    match raw.split_once(':') {
        Some((prefix, local)) => (Some(prefix.to_string()), local.to_string()),
        None => (None, raw.into_owned()),
    }
}

impl XmlElement {
    pub(crate) fn is(&self, local: &str) -> bool {
        self.local == local
    }

    pub(crate) fn attr(&self, local: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|attr| attr.local == local)
            .map(|attr| attr.value.as_str())
    }

    /// Attribute lookup preferring the given prefix, falling back to any prefix.
    pub(crate) fn attr_prefixed(&self, prefix: &str, local: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|attr| attr.local == local && attr.prefix.as_deref() == Some(prefix))
            .or_else(|| self.attrs.iter().find(|attr| attr.local == local))
            .map(|attr| attr.value.as_str())
    }

    pub(crate) fn elements(&self) -> impl Iterator<Item = &XmlElement> {
        self.children.iter().filter_map(|child| match child {
            XmlNode::Element(element) => Some(element),
            XmlNode::Text(_) => None,
        })
    }

    pub(crate) fn child(&self, local: &str) -> Option<&XmlElement> {
        self.elements().find(|element| element.is(local))
    }

    pub(crate) fn children_named<'a>(
        &'a self,
        local: &'a str,
    ) -> impl Iterator<Item = &'a XmlElement> + 'a {
        self.elements().filter(move |element| element.is(local))
    }

    /// `w:val`-style attribute of a direct child element.
    pub(crate) fn child_val(&self, local: &str) -> Option<&str> {
        self.child(local).and_then(|child| child.attr("val"))
    }

    /// Concatenated text of this element and all descendants.
    pub(crate) fn text(&self) -> String {
        let mut out = String::new();
        self.collect_text(&mut out);
        out
    }

    fn collect_text(&self, out: &mut String) {
        for child in &self.children {
            match child {
                XmlNode::Text(text) => out.push_str(text),
                XmlNode::Element(element) => element.collect_text(out),
            }
        }
    }

    /// Depth-first iterator over all descendant elements.
    pub(crate) fn descendants(&self) -> Vec<&XmlElement> {
        let mut out = Vec::new();
        self.collect_descendants(&mut out);
        out
    }

    fn collect_descendants<'a>(&'a self, out: &mut Vec<&'a XmlElement>) {
        for element in self.elements() {
            out.push(element);
            element.collect_descendants(out);
        }
    }

    pub(crate) fn find_descendant(&self, local: &str) -> Option<&XmlElement> {
        for element in self.elements() {
            if element.is(local) {
                return Some(element);
            }
            if let Some(found) = element.find_descendant(local) {
                return Some(found);
            }
        }
        None
    }

    pub(crate) fn has_descendant(&self, local: &str) -> bool {
        self.find_descendant(local).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_prefixed_elements_attributes_and_entities() {
        let root = parse_xml(
            r#"<?xml version="1.0"?><w:document xmlns:w="urn:w"><w:p><w:t xml:space="preserve">a &amp; b &#x41;</w:t><w:b w:val='0'/></w:p></w:document>"#,
        )
        .unwrap();
        assert_eq!(root.local, "document");
        assert_eq!(root.prefix.as_deref(), Some("w"));
        let paragraph = root.child("p").unwrap();
        assert_eq!(paragraph.child("t").unwrap().text(), "a & b A");
        assert_eq!(paragraph.child_val("b"), Some("0"));
        assert!(root.has_descendant("t"));
    }

    #[test]
    fn rejects_malformed_xml() {
        assert!(parse_xml("<w:document><w:p></w:document>").is_err());
        assert!(parse_xml("plain text").is_err());
    }
}
