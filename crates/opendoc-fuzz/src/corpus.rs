//! Inputs for the targets.
//!
//! Two sources, because random bytes alone never reach a parser's interesting
//! states and hand-written shapes alone never surprise anyone:
//!
//! * **Seeds**, produced by this repository's own exporters, then mutated by a
//!   seeded PRNG. Starting from a valid package is what gets a mutation past
//!   the zip header and into the XML.
//! * **Adversarial shapes**, built explicitly: nesting past the cap, a
//!   declared size past the cap, a compression ratio past the cap, a sheet
//!   dimension past the cap. These are the inputs the denial-of-service proofs
//!   of concept used, and no random mutation would ever construct one.

use std::collections::BTreeMap;
use std::io::Write;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;

use opendoc_core::{Block, Document};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::rng::Rng;

// ---------------------------------------------------------------------------
// valid seeds, from this repository's own writers
// ---------------------------------------------------------------------------

fn seed_document() -> Document {
    let mut document = Document::new("Fuzz seed");
    document.blocks.push(Block::paragraph(
        "the quick brown fox jumps over the lazy dog",
    ));
    document.blocks.push(Block::paragraph("a second paragraph"));
    document
}

/// A real `.docx` package, so a mutation starts inside the format rather than
/// bouncing off the zip header.
pub fn docx_seed() -> Vec<u8> {
    opendoc_import::export_docx(&seed_document(), &BTreeMap::new())
        .expect("the seed document exports")
}

/// A real `.xlsx` package.
pub fn xlsx_seed() -> Vec<u8> {
    let encoded = opendoc_spreadsheet::SpreadsheetWorkbook::sample()
        .to_xlsx_base64()
        .expect("the sample workbook exports");
    STANDARD
        .decode(encoded)
        .expect("the exporter emits valid base64")
}

/// The public XLSX surface takes base64, so the targets have to hand it base64
/// even when the corpus is bytes.
pub fn base64_of(bytes: &[u8]) -> String {
    STANDARD.encode(bytes)
}

/// Real Google Docs-shaped JSON.
pub fn google_json_seed() -> Vec<u8> {
    opendoc_import::export_google_docs_json(&seed_document()).expect("the seed document exports")
}

// ---------------------------------------------------------------------------
// mutation
// ---------------------------------------------------------------------------

/// Derive one input from `seed` by a handful of byte-level edits.
///
/// Flips, splices, truncations and repeats — enough to corrupt a length
/// prefix, a CRC, an element name or a UTF-8 sequence, which is where a parser
/// that trusts its input falls over.
pub fn mutate(seed: &[u8], rng: &mut Rng) -> Vec<u8> {
    let mut bytes = seed.to_vec();
    let edits = 1 + rng.below(6);
    for _ in 0..edits {
        if bytes.is_empty() {
            bytes.push(rng.byte());
            continue;
        }
        match rng.below(6) {
            // Flip a byte.
            0 | 1 => {
                let at = rng.below(bytes.len());
                bytes[at] ^= 1 << rng.below(8);
            }
            // Overwrite a byte with something arbitrary.
            2 => {
                let at = rng.below(bytes.len());
                bytes[at] = rng.byte();
            }
            // Cut the tail off — a truncated archive or a truncated element.
            3 => {
                let keep = rng.below(bytes.len());
                bytes.truncate(keep);
            }
            // Splice a run back in somewhere else, which is how a length
            // prefix ends up describing the wrong bytes.
            4 => {
                let len = 1 + rng.below(bytes.len().min(64));
                let from = rng.below(bytes.len() - len + 1);
                let chunk = bytes[from..from + len].to_vec();
                let at = rng.below(bytes.len() + 1);
                bytes.splice(at..at, chunk);
            }
            // Insert a run of one byte value.
            _ => {
                let len = 1 + rng.below(64);
                let value = rng.byte();
                let at = rng.below(bytes.len() + 1);
                bytes.splice(at..at, std::iter::repeat_n(value, len));
            }
        }
    }
    bytes
}

// ---------------------------------------------------------------------------
// zip construction
// ---------------------------------------------------------------------------

fn zip_of(entries: &[(&str, Vec<u8>)], method: CompressionMethod) -> Vec<u8> {
    let mut buffer = Vec::new();
    {
        let mut writer = ZipWriter::new(std::io::Cursor::new(&mut buffer));
        let options = SimpleFileOptions::default().compression_method(method);
        for (name, bytes) in entries {
            writer.start_file(*name, options).expect("start entry");
            writer.write_all(bytes).expect("write entry");
        }
        writer.finish().expect("finish archive");
    }
    buffer
}

fn nested_xml(tag: &str, depth: usize, innermost: &str) -> Vec<u8> {
    let mut out = String::from(r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    for _ in 0..depth {
        out.push('<');
        out.push_str(tag);
        out.push('>');
    }
    out.push_str(innermost);
    for _ in 0..depth {
        out.push_str("</");
        out.push_str(tag);
        out.push('>');
    }
    out.into_bytes()
}

// ---------------------------------------------------------------------------
// adversarial DOCX packages
// ---------------------------------------------------------------------------

const DOCX_CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"#;

const DOCX_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#;

fn docx_package(document_xml: Vec<u8>) -> Vec<u8> {
    zip_of(
        &[
            (
                "[Content_Types].xml",
                DOCX_CONTENT_TYPES.as_bytes().to_vec(),
            ),
            ("_rels/.rels", DOCX_RELS.as_bytes().to_vec()),
            ("word/document.xml", document_xml),
        ],
        CompressionMethod::Deflated,
    )
}

/// `word/document.xml` nested `depth` elements deep.
///
/// The shape that used to abort the process with `fatal runtime error: stack
/// overflow` — a `SIGABRT` no caller can catch — from a 1.6 KB file.
pub fn docx_nested_elements(depth: usize) -> Vec<u8> {
    docx_package(nested_xml("a", depth, ""))
}

/// Tables nested `depth` deep, which is the same hazard reached through the
/// converter rather than through the XML reader.
pub fn docx_nested_tables(depth: usize) -> Vec<u8> {
    let mut inner = String::from("<w:p><w:r><w:t>cell</w:t></w:r></w:p>");
    for _ in 0..depth {
        inner = format!("<w:tbl><w:tr><w:tc>{inner}</w:tc></w:tr></w:tbl>");
    }
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{inner}</w:body></w:document>"#
    );
    docx_package(xml.into_bytes())
}

/// A package whose one part inflates to `bytes` from almost nothing.
pub fn docx_inflation_bomb(bytes: usize) -> Vec<u8> {
    docx_package(vec![b'A'; bytes])
}

/// A package holding `count` entries.
pub fn docx_many_entries(count: usize) -> Vec<u8> {
    let names: Vec<String> = (0..count)
        .map(|index| format!("word/p{index}.xml"))
        .collect();
    let entries: Vec<(&str, Vec<u8>)> = names
        .iter()
        .map(|name| (name.as_str(), b"<a/>".to_vec()))
        .collect();
    zip_of(&entries, CompressionMethod::Deflated)
}

// ---------------------------------------------------------------------------
// adversarial XLSX packages
// ---------------------------------------------------------------------------

const XLSX_CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
<Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>
</Types>"#;

const XLSX_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>
</Relationships>"#;

const XLSX_WORKBOOK: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<sheets><sheet name="Sheet1" sheetId="1" r:id="rId1"/></sheets>
</workbook>"#;

const XLSX_WORKBOOK_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>
</Relationships>"#;

fn xlsx_package(sheet_xml: Vec<u8>) -> Vec<u8> {
    zip_of(
        &[
            (
                "[Content_Types].xml",
                XLSX_CONTENT_TYPES.as_bytes().to_vec(),
            ),
            ("_rels/.rels", XLSX_RELS.as_bytes().to_vec()),
            ("xl/workbook.xml", XLSX_WORKBOOK.as_bytes().to_vec()),
            (
                "xl/_rels/workbook.xml.rels",
                XLSX_WORKBOOK_RELS.as_bytes().to_vec(),
            ),
            ("xl/worksheets/sheet1.xml", sheet_xml),
        ],
        CompressionMethod::Deflated,
    )
}

/// A sheet whose only cell sits at row `row`, column `column`.
///
/// The whole file is a few hundred bytes; the *dimension* is whatever the cell
/// reference says. A reader that allocates a grid from the declared extent
/// dies here, which is the XLSX denial of service in one input.
pub fn xlsx_far_cell(column: &str, row: u64) -> Vec<u8> {
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><dimension ref="A1:{column}{row}"/><sheetData><row r="{row}"><c r="{column}{row}" t="str"><v>far</v></c></row></sheetData></worksheet>"#
    );
    xlsx_package(xml.into_bytes())
}

/// A sheet part nested `depth` elements deep.
pub fn xlsx_nested_elements(depth: usize) -> Vec<u8> {
    xlsx_package(nested_xml("a", depth, ""))
}

/// A sheet part that inflates to `bytes`.
pub fn xlsx_inflation_bomb(bytes: usize) -> Vec<u8> {
    xlsx_package(vec![b'A'; bytes])
}

// ---------------------------------------------------------------------------
// adversarial Google Docs JSON
// ---------------------------------------------------------------------------

/// `body.content` holding a table nested `depth` deep.
///
/// The importer walks structural elements recursively, so the depth of the
/// JSON is the depth of the recursion.
pub fn google_json_nested_tables(depth: usize) -> Vec<u8> {
    let mut inner = String::from(
        r#"{"paragraph":{"elements":[{"textRun":{"content":"cell","textStyle":{}}}]}}"#,
    );
    for _ in 0..depth {
        inner =
            format!(r#"{{"table":{{"tableRows":[{{"tableCells":[{{"content":[{inner}]}}]}}]}}}}"#);
    }
    format!(r#"{{"title":"deep","body":{{"content":[{inner}]}}}}"#).into_bytes()
}

/// A `body.content` array holding `count` paragraphs.
pub fn google_json_wide(count: usize) -> Vec<u8> {
    let one = r#"{"paragraph":{"elements":[{"textRun":{"content":"x","textStyle":{}}}]}}"#;
    let mut out = String::from(r#"{"title":"wide","body":{"content":["#);
    for index in 0..count {
        if index > 0 {
            out.push(',');
        }
        out.push_str(one);
    }
    out.push_str("]}}");
    out.into_bytes()
}

/// A text run declaring an enormous repetition, so the *declared* size and the
/// file size are unrelated.
pub fn google_json_absurd_numbers() -> Vec<u8> {
    br#"{"title":"numbers","body":{"content":[
        {"paragraph":{"elements":[
            {"startIndex":-9223372036854775808,"endIndex":9223372036854775807,
             "textRun":{"content":"x","textStyle":{"fontSize":{"magnitude":1e308,"unit":"PT"}}}}
        ],"paragraphStyle":{"indentStart":{"magnitude":-1e308,"unit":"PT"}}}}
    ]}}"#
        .to_vec()
}

/// The seed workbook with `xl/sharedStrings.xml` removed and every
/// relationship left pointing at it.
///
/// The smallest input that reproduces the panic `targets::KNOWN_XLSX_PANIC`
/// names. A single flipped byte in that entry's *name* does the same thing,
/// which is how the mutation run found it.
pub fn xlsx_without_shared_strings() -> Vec<u8> {
    xlsx_shared_strings_renamed_to(None)
}

/// The seed workbook with `xl/sharedStrings.xml` present under a *different*
/// name, which is what a single flipped byte in a zip entry name produces.
pub fn xlsx_with_misnamed_shared_strings() -> Vec<u8> {
    xlsx_shared_strings_renamed_to(Some("xl/sharedStr)ngs.xml"))
}

fn xlsx_shared_strings_renamed_to(new_name: Option<&str>) -> Vec<u8> {
    use std::io::Read;
    let seed = xlsx_seed();
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(seed)).expect("the seed is a zip archive");
    let names: Vec<String> = archive.file_names().map(ToString::to_string).collect();
    let mut out = Vec::new();
    {
        let mut writer = ZipWriter::new(std::io::Cursor::new(&mut out));
        let options = SimpleFileOptions::default();
        for name in &names {
            let written = if name == "xl/sharedStrings.xml" {
                match new_name {
                    Some(renamed) => renamed.to_string(),
                    None => continue,
                }
            } else {
                name.clone()
            };
            let mut bytes = Vec::new();
            archive
                .by_name(name)
                .expect("the entry was listed")
                .read_to_end(&mut bytes)
                .expect("the entry inflates");
            writer.start_file(written, options).expect("start");
            writer.write_all(&bytes).expect("write");
        }
        writer.finish().expect("finish");
    }
    out
}
