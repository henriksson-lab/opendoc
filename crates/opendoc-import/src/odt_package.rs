//! The ODT package's fixed parts, its units and its zip.
//!
//! Split out of [`crate::odt_write`] to keep both under the repository's
//! 2,000-line ceiling. What lives here is everything that does not depend on
//! the document being exported: the namespace declarations, the named styles
//! and metadata parts, the twip-to-point conversion, and the packaging rule
//! that puts `mimetype` first and uncompressed.

use std::io::{Cursor, Write};

use opendoc_core::{BorderStyle, CellBorder, Color, Document, Length};

use crate::xml_write::Xml;
use crate::ImportError;

/// The IANA media type of an `.odt` package.
pub const ODT_MEDIA_TYPE: &str = "application/vnd.oasis.opendocument.text";

// ---------------------------------------------------------------------------
// Namespaces
// ---------------------------------------------------------------------------

pub(crate) const ODF_VERSION: &str = "1.3";

/// The namespace declarations every part shares. Written as attributes in a
/// fixed order so the same document exports to the same bytes.
pub(crate) const NAMESPACES: &[(&str, &str)] = &[
    (
        "xmlns:office",
        "urn:oasis:names:tc:opendocument:xmlns:office:1.0",
    ),
    (
        "xmlns:style",
        "urn:oasis:names:tc:opendocument:xmlns:style:1.0",
    ),
    (
        "xmlns:text",
        "urn:oasis:names:tc:opendocument:xmlns:text:1.0",
    ),
    (
        "xmlns:table",
        "urn:oasis:names:tc:opendocument:xmlns:table:1.0",
    ),
    (
        "xmlns:draw",
        "urn:oasis:names:tc:opendocument:xmlns:drawing:1.0",
    ),
    (
        "xmlns:fo",
        "urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0",
    ),
    ("xmlns:xlink", "http://www.w3.org/1999/xlink"),
    ("xmlns:dc", "http://purl.org/dc/elements/1.1/"),
    (
        "xmlns:meta",
        "urn:oasis:names:tc:opendocument:xmlns:meta:1.0",
    ),
    (
        "xmlns:svg",
        "urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0",
    ),
];

pub(crate) const NS_MANIFEST: &str = "urn:oasis:names:tc:opendocument:xmlns:manifest:1.0";

/// The faces the export asks for. Liberation Sans and Liberation Mono are
/// the fonts whose metrics `opendoc-layout` paginates with (it bundles a
/// subset of them), so an ODT opened in LibreOffice — which ships them —
/// breaks its lines where the editor did. The `svg:font-family` list carries
/// a fallback for readers that have neither.
pub(crate) const SANS: &str = "Liberation Sans";
pub(crate) const MONO: &str = "Liberation Mono";

/// Heading sizes in twips, matching the DOCX writer's half-points
/// (`[40, 32, 28, 24, 22, 20]`) so the two exports of one document agree.
pub(crate) const HEADING_SIZE_TWIPS: [i32; 6] = [400, 320, 280, 240, 220, 200];
/// Body text size, again the DOCX writer's (`w:sz` 22 half-points = 11pt).
pub(crate) const BODY_SIZE_TWIPS: i32 = 220;

// ---------------------------------------------------------------------------
// Units
// ---------------------------------------------------------------------------

/// A [`Length`] as an ODF length in points, exactly.
///
/// 20 twips is one point, so the value is `twips / 20` with a remainder of at
/// most 19 twentieths — five hundredths each. Every length therefore has at
/// most two decimals and none of them is rounded.
pub(crate) fn pt(length: Length) -> String {
    twips_to_pt(length.twips())
}

pub(crate) fn twips_to_pt(twips: i32) -> String {
    let sign = if twips < 0 { "-" } else { "" };
    let magnitude = twips.unsigned_abs();
    let whole = magnitude / 20;
    let hundredths = (magnitude % 20) * 5;
    if hundredths == 0 {
        format!("{sign}{whole}pt")
    } else {
        format!("{sign}{whole}.{hundredths:02}pt")
    }
}

/// A line-height multiple as an ODF percentage, exactly: the model counts
/// thousandths of a line, so a percentage needs one decimal at most.
pub(crate) fn line_height_percent(thousandths: u32) -> String {
    let whole = thousandths / 10;
    let tenths = thousandths % 10;
    if tenths == 0 {
        format!("{whole}%")
    } else {
        format!("{whole}.{tenths}%")
    }
}

// ---------------------------------------------------------------------------
// Static parts
// ---------------------------------------------------------------------------

pub(crate) fn write_font_faces(xml: &mut Xml) {
    xml.open("office:font-face-decls", &[]);
    xml.empty(
        "style:font-face",
        &[
            ("style:name", SANS),
            ("svg:font-family", "'Liberation Sans', Arial, sans-serif"),
            ("style:font-family-generic", "swiss"),
            ("style:font-pitch", "variable"),
        ],
    );
    xml.empty(
        "style:font-face",
        &[
            ("style:name", MONO),
            (
                "svg:font-family",
                "'Liberation Mono', 'Courier New', monospace",
            ),
            ("style:font-family-generic", "modern"),
            ("style:font-pitch", "fixed"),
        ],
    );
    xml.close("office:font-face-decls");
}

/// The named styles the body refers to. ODF encodes a space in a style name
/// as `_20_`, so "Heading 1" is `Heading_20_1` with a display name.
pub(crate) fn write_named_styles(xml: &mut Xml) {
    xml.open("office:styles", &[]);

    xml.open("style:default-style", &[("style:family", "paragraph")]);
    xml.empty(
        "style:text-properties",
        &[
            ("style:font-name", SANS),
            ("fo:font-size", &twips_to_pt(BODY_SIZE_TWIPS)),
        ],
    );
    xml.close("style:default-style");

    xml.open(
        "style:style",
        &[
            ("style:name", "Standard"),
            ("style:family", "paragraph"),
            ("style:class", "text"),
        ],
    );
    xml.close("style:style");

    for level in 1..=6u8 {
        let name = format!("Heading_20_{level}");
        let display = format!("Heading {level}");
        let outline = level.to_string();
        let size = twips_to_pt(HEADING_SIZE_TWIPS[usize::from(level) - 1]);
        xml.open(
            "style:style",
            &[
                ("style:name", &name),
                ("style:display-name", &display),
                ("style:family", "paragraph"),
                ("style:parent-style-name", "Standard"),
                ("style:next-style-name", "Standard"),
                ("style:default-outline-level", &outline),
                ("style:class", "text"),
            ],
        );
        xml.empty(
            "style:paragraph-properties",
            &[("fo:keep-with-next", "always")],
        );
        // A heading is a heading in ODF because of its style, exactly as in
        // WordprocessingML, and a style with no formatting produces a
        // document that does not look like it has headings. These are the two
        // properties the DOCX writer's heading styles set, at the same sizes.
        xml.empty(
            "style:text-properties",
            &[("fo:font-size", &size), ("fo:font-weight", "bold")],
        );
        xml.close("style:style");
    }

    for (name, size, colour) in [("Title", 26u16, None), ("Subtitle", 15u16, Some("#666666"))] {
        xml.open(
            "style:style",
            &[
                ("style:name", name),
                ("style:display-name", name),
                ("style:family", "paragraph"),
                ("style:parent-style-name", "Standard"),
                ("style:next-style-name", "Standard"),
                ("style:class", "text"),
            ],
        );
        let size = format!("{size}pt");
        let mut attrs = vec![("fo:font-size", size.as_str())];
        if let Some(colour) = colour {
            attrs.push(("fo:color", colour));
        }
        xml.empty("style:text-properties", &attrs);
        xml.close("style:style");
    }

    for (name, display) in [
        ("Footnote", "Footnote"),
        ("Header", "Header"),
        ("Footer", "Footer"),
    ] {
        xml.open(
            "style:style",
            &[
                ("style:name", name),
                ("style:display-name", display),
                ("style:family", "paragraph"),
                ("style:parent-style-name", "Standard"),
                ("style:class", "extra"),
            ],
        );
        xml.close("style:style");
    }

    xml.open(
        "style:style",
        &[
            ("style:name", "Bullet_20_Symbol"),
            ("style:display-name", "Bullet Symbol"),
            ("style:family", "text"),
        ],
    );
    xml.close("style:style");
    xml.open(
        "style:style",
        &[
            ("style:name", "Numbering_20_Symbols"),
            ("style:display-name", "Numbering Symbols"),
            ("style:family", "text"),
        ],
    );
    xml.close("style:style");

    xml.close("office:styles");
}

pub(crate) fn meta_part(document: &Document) -> Vec<u8> {
    let mut xml = Xml::odf_part();
    let mut attrs: Vec<(&str, &str)> = NAMESPACES.to_vec();
    attrs.push(("office:version", ODF_VERSION));
    xml.open("office:document-meta", &attrs);
    xml.open("office:meta", &[]);
    // No clock is read: the export of one document is the same bytes every
    // time, the property `docx_write` gets by building `zip` without its
    // `time` feature.
    xml.text_element("meta:generator", &[], "OpenDoc");
    xml.text_element("dc:title", &[], &document.title);
    xml.text_element("dc:language", &[], &document.locale);
    xml.close("office:meta");
    xml.close("office:document-meta");
    xml.into_bytes()
}

// ---------------------------------------------------------------------------
// Values
// ---------------------------------------------------------------------------

pub(crate) fn hex(color: Color) -> String {
    color.as_hex()
}

pub(crate) fn border_value(border: CellBorder) -> String {
    match border.style() {
        BorderStyle::None => "none".to_string(),
        style => format!(
            "{} {} {}",
            pt(border.width()),
            match style {
                BorderStyle::Solid => "solid",
                BorderStyle::Dashed => "dashed",
                BorderStyle::Dotted => "dotted",
                BorderStyle::Double => "double",
                BorderStyle::None => unreachable!("handled above"),
            },
            hex(border.color())
        ),
    }
}

pub(crate) fn hex_value(value: Option<&str>) -> Option<String> {
    let value = value?.trim().trim_start_matches('#');
    if value.len() == 6 && value.chars().all(|ch| ch.is_ascii_hexdigit()) {
        Some(format!("#{}", value.to_ascii_lowercase()))
    } else {
        None
    }
}

/// Scales the unset axis of an image from the picture's own pixel aspect
/// ratio. `from_width` says which axis was given.
pub(crate) fn scale_axis(given: i32, pixels: Option<(u32, u32)>, from_width: bool) -> i32 {
    let Some((pixel_width, pixel_height)) = pixels else {
        // No readable size: a 4:3 frame, reported as unknown by the caller.
        return if from_width {
            given * 3 / 4
        } else {
            given * 4 / 3
        };
    };
    let (numerator, denominator) = if from_width {
        (i64::from(pixel_height), i64::from(pixel_width))
    } else {
        (i64::from(pixel_width), i64::from(pixel_height))
    };
    let scaled = i64::from(given) * numerator / denominator.max(1);
    scaled.clamp(1, i64::from(Length::MAX_TWIPS)) as i32
}

pub(crate) fn image_extension(media_type: &str, bytes: &[u8]) -> Option<&'static str> {
    // MIME tokens are case-insensitive; parameters do not change the native
    // picture kind.  This deliberately mirrors DOCX, raw-image save, and PDF
    // dispatch while leaving the blob's declared metadata untouched.
    let essence = media_type
        .split_once(';')
        .map_or(media_type, |(essence, _)| essence)
        .trim()
        .to_ascii_lowercase();
    let by_media_type = match essence.as_str() {
        "image/png" => Some("png"),
        "image/jpeg" | "image/jpg" => Some("jpg"),
        "image/gif" => Some("gif"),
        "image/bmp" => Some("bmp"),
        "image/tiff" => Some("tif"),
        "image/svg+xml" => Some("svg"),
        "image/webp" => Some("webp"),
        _ => None,
    };
    by_media_type.or_else(|| match bytes {
        bytes if bytes.starts_with(b"\x89PNG\r\n\x1a\n") => Some("png"),
        bytes if bytes.starts_with(b"\xff\xd8\xff") => Some("jpg"),
        bytes if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") => Some("gif"),
        bytes if bytes.starts_with(b"BM") => Some("bmp"),
        _ => None,
    })
}

pub(crate) fn picture_media_type(path: &str) -> String {
    match path.rsplit('.').next().unwrap_or_default() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "tif" | "tiff" => "image/tiff",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        _ => "application/octet-stream",
    }
    .to_string()
}

/// The pixel size in a picture's own bytes, for the axis the model leaves
/// open. Nothing here is written into the document.
pub(crate) fn image_pixels(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") && bytes.len() >= 24 {
        let width = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
        let height = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
        return (width > 0 && height > 0).then_some((width, height));
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        if bytes.len() < 10 {
            return None;
        }
        let width = u32::from(u16::from_le_bytes([bytes[6], bytes[7]]));
        let height = u32::from(u16::from_le_bytes([bytes[8], bytes[9]]));
        return (width > 0 && height > 0).then_some((width, height));
    }
    if bytes.starts_with(b"\xff\xd8\xff") {
        return jpeg_pixels(bytes);
    }
    None
}

pub(crate) fn jpeg_pixels(bytes: &[u8]) -> Option<(u32, u32)> {
    let mut index = 2;
    while index + 9 < bytes.len() {
        if bytes[index] != 0xff {
            index += 1;
            continue;
        }
        let marker = bytes[index + 1];
        if (0xc0..=0xcf).contains(&marker) && !matches!(marker, 0xc4 | 0xc8 | 0xcc) {
            let height = u32::from(u16::from_be_bytes([bytes[index + 5], bytes[index + 6]]));
            let width = u32::from(u16::from_be_bytes([bytes[index + 7], bytes[index + 8]]));
            return (width > 0 && height > 0).then_some((width, height));
        }
        let length = u16::from_be_bytes([bytes[index + 2], bytes[index + 3]]) as usize;
        if length < 2 {
            return None;
        }
        index += 2 + length;
    }
    None
}

// ---------------------------------------------------------------------------
// Packaging
// ---------------------------------------------------------------------------

/// Zips the parts, `mimetype` first and stored.
///
/// That is not a stylistic choice: the ODF package format requires the first
/// entry to be an uncompressed `mimetype` so that the media type sits at a
/// fixed offset and a reader can recognise the file without unzipping it.
/// Everything else is deflated.
pub(crate) fn zip_odf_parts(parts: &[(String, Vec<u8>)]) -> Result<Vec<u8>, ImportError> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    // As in `docx_write`: `zip` is built without its `time` feature, so
    // `SimpleFileOptions::default()` stamps 1980-01-01 rather than reading a
    // clock. The same document exports to the same bytes, and nothing traps
    // on wasm32.
    let stored =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let deflated = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, bytes) in parts {
        let options = if name == "mimetype" { stored } else { deflated };
        writer
            .start_file(name.as_str(), options)
            .map_err(|err| ImportError::InvalidDocument(format!("ODT part {name}: {err}")))?;
        writer
            .write_all(bytes)
            .map_err(|err| ImportError::InvalidDocument(format!("ODT part {name}: {err}")))?;
    }
    let cursor = writer
        .finish()
        .map_err(|err| ImportError::InvalidDocument(format!("ODT package: {err}")))?;
    Ok(cursor.into_inner())
}
