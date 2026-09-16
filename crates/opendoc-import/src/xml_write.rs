//! A minimal, correctly escaping XML serializer shared by the package
//! writers.
//!
//! WordprocessingML fixes the *order* of the children of its property
//! elements in the schema and Word refuses a package that reorders them, so
//! `docx_write` is written by position; OpenDocument states its properties as
//! attributes instead, but its element sequences (`office:styles` before
//! `office:automatic-styles` before `office:master-styles`) are equally
//! fixed. Building the text directly keeps both orders visible in the code
//! that writes them, which is why neither writer uses a DOM or a serde
//! mapping.

/// A minimal, correctly escaping XML serializer.
pub(crate) struct Xml {
    out: String,
}

impl Xml {
    pub(crate) fn part() -> Self {
        Self {
            out: String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n"),
        }
    }

    /// An OpenDocument part. ODF parts carry no `standalone` declaration and
    /// no CRLF; the line above is what WordprocessingML parts use.
    pub(crate) fn odf_part() -> Self {
        Self {
            out: String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n"),
        }
    }

    pub(crate) fn fragment() -> Self {
        Self { out: String::new() }
    }

    pub(crate) fn open(&mut self, name: &str, attrs: &[(&str, &str)]) {
        self.out.push('<');
        self.out.push_str(name);
        self.push_attrs(attrs);
        self.out.push('>');
    }

    pub(crate) fn empty(&mut self, name: &str, attrs: &[(&str, &str)]) {
        self.out.push('<');
        self.out.push_str(name);
        self.push_attrs(attrs);
        self.out.push_str("/>");
    }

    pub(crate) fn close(&mut self, name: &str) {
        self.out.push_str("</");
        self.out.push_str(name);
        self.out.push('>');
    }

    pub(crate) fn text(&mut self, value: &str) {
        escape_into(&mut self.out, value, false);
    }

    pub(crate) fn text_element(&mut self, name: &str, attrs: &[(&str, &str)], value: &str) {
        self.open(name, attrs);
        self.text(value);
        self.close(name);
    }

    pub(crate) fn raw(&mut self, fragment: &str) {
        self.out.push_str(fragment);
    }

    fn push_attrs(&mut self, attrs: &[(&str, &str)]) {
        for (name, value) in attrs {
            self.out.push(' ');
            self.out.push_str(name);
            self.out.push_str("=\"");
            escape_into(&mut self.out, value, true);
            self.out.push('"');
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.out.is_empty()
    }

    pub(crate) fn into_bytes(self) -> Vec<u8> {
        self.out.into_bytes()
    }

    pub(crate) fn into_string(self) -> String {
        self.out
    }
}

pub(crate) fn escape_into(out: &mut String, value: &str, attribute: bool) {
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if attribute => out.push_str("&quot;"),
            '\t' if attribute => out.push_str("&#9;"),
            '\n' if attribute => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            _ => out.push(ch),
        }
    }
}

/// XML 1.0 forbids most C0 controls outright — a document holding one cannot
/// be written at all, so the character is removed and named rather than
/// producing a package no reader will open.
pub(crate) fn is_writable_xml_char(ch: char) -> bool {
    match ch {
        '\t' | '\n' | '\r' => true,
        ch if (ch as u32) < 0x20 => false,
        '\u{fffe}' | '\u{ffff}' => false,
        ch => !('\u{fdd0}'..='\u{fdef}').contains(&ch),
    }
}
