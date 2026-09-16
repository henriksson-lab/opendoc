//! DOCX reader tests built from synthetic in-memory packages.

use crate::{import_doc_or_docx, import_docx_bytes, ImportError, ImportReport};
use opendoc_core::{
    Alignment, Anchor, Block, BlockKind, BorderStyle, CellBorder, Color, Inline, Length,
    LineSpacing, Mark, MarkKind, StableId, SuggestionKind, SuggestionState, TextDirection,
};
use std::io::{Cursor, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

static TEMP_DOCX_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Default Extension="png" ContentType="image/png"/>
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"#;

const ROOT_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#;

const NAMESPACES: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture" xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml" xmlns:w15="http://schemas.microsoft.com/office/word/2012/wordml""#;

fn document_xml(body: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document {NAMESPACES}>
  <w:body>
{body}
  </w:body>
</w:document>"#
    )
}

fn document_rels(relationships: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
{relationships}
</Relationships>"#
    )
}

fn build_docx(parts: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, bytes) in parts {
        writer.start_file(*name, options).unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn temp_docx_path(label: &str) -> PathBuf {
    let sequence = TEMP_DOCX_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "opendoc-import-docx-pkg-{label}-{}-{sequence}.docx",
        std::process::id(),
    ));
    let _ = std::fs::remove_file(&path);
    path
}

fn import_package(label: &str, parts: &[(&str, &[u8])]) -> Result<ImportReport, ImportError> {
    let path = temp_docx_path(label);
    std::fs::write(&path, build_docx(parts)).unwrap();
    let result = import_doc_or_docx(&path);
    let _ = std::fs::remove_file(path);
    result
}

/// Builds a package with the standard content types, root relationships and
/// the given body, plus any extra parts.
fn import_body(label: &str, body: &str, extra: &[(&str, &[u8])]) -> ImportReport {
    let document = document_xml(body);
    let mut parts: Vec<(&str, &[u8])> = vec![
        ("[Content_Types].xml", CONTENT_TYPES.as_bytes()),
        ("_rels/.rels", ROOT_RELS.as_bytes()),
        ("word/document.xml", document.as_bytes()),
    ];
    parts.extend(extra.iter().copied());
    import_package(label, &parts).unwrap()
}

fn text_marks(block: &Block) -> Vec<&Mark> {
    block
        .content
        .iter()
        .flat_map(|inline| match inline {
            Inline::Text { marks, .. } | Inline::Link { marks, .. } => marks.iter().collect(),
            _ => Vec::new(),
        })
        .collect()
}

fn inline_marks(block: &Block, text: &str) -> Vec<MarkKind> {
    block
        .content
        .iter()
        .find_map(|inline| match inline {
            Inline::Text {
                text: value, marks, ..
            }
            | Inline::Link {
                text: value, marks, ..
            } if value == text => Some(marks.iter().map(|mark| mark.kind.clone()).collect()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("inline {text:?} not found in {block:?}"))
}

fn inline_id_with_text(report: &ImportReport, text: &str) -> StableId {
    report
        .document
        .blocks
        .iter()
        .flat_map(|block| block.content.iter())
        .find_map(|inline| match inline {
            Inline::Text {
                id, text: value, ..
            }
            | Inline::Link {
                id, text: value, ..
            } if value == text => Some(id.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("inline {text:?} not found"))
}

fn warning_message(report: &ImportReport, code: &str) -> String {
    report
        .warnings
        .iter()
        .find(|warning| warning.code == code)
        .map(|warning| warning.message.clone())
        .unwrap_or_else(|| panic!("warning {code} missing from {:?}", report.warnings))
}

fn has_warning(report: &ImportReport, code: &str) -> bool {
    report.warnings.iter().any(|warning| warning.code == code)
}

#[test]
fn imports_zipped_docx_package_with_hyperlink_relationship() {
    let rels = document_rels(
        r#"<Relationship Id="rId5" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.invalid/docx" TargetMode="External"/>"#,
    );
    let report = import_body(
        "hyperlink",
        r#"<w:p><w:r><w:t>Docx heading</w:t></w:r></w:p>
<w:p><w:r><w:t>Docx body </w:t></w:r><w:hyperlink r:id="rId5"><w:r><w:rPr><w:u w:val="single"/></w:rPr><w:t>linked</w:t></w:r></w:hyperlink></w:p>"#,
        &[("word/_rels/document.xml.rels", rels.as_bytes())],
    );
    assert!(report
        .document
        .title
        .starts_with("opendoc-import-docx-pkg-hyperlink-"));
    assert_eq!(
        report.document.visible_text(),
        "Docx heading\nDocx body linked\n"
    );
    assert!(report.document.blocks[1]
        .content
        .iter()
        .any(|inline| matches!(
            inline,
            Inline::Link { text, href, marks, .. }
                if text == "linked"
                    && href == "https://example.invalid/docx"
                    && marks.iter().any(|mark| mark.kind == MarkKind::Underline)
        )));
    assert!(report.warnings.is_empty());
    assert!(report.document.validate().is_ok());
}

#[test]
fn imports_paperpile_body_citations_without_using_bibliography_links() {
    let rels = document_rels(
        r#"
<Relationship Id="rIdCitation" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://paperpile.com/c/Doc123/RefA+RefB" TargetMode="External"/>
<Relationship Id="rIdBibliography" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://paperpile.com/b/Doc123/UnusedReference" TargetMode="External"/>"#,
    );
    let report = import_body(
        "paperpile-citations",
        r#"<w:p><w:r><w:t>Claim</w:t></w:r><w:hyperlink r:id="rIdCitation"><w:r><w:rPr><w:vertAlign w:val="superscript"/></w:rPr><w:t>1,2</w:t></w:r></w:hyperlink></w:p>
<w:p><w:hyperlink r:id="rIdBibliography"><w:r><w:t>Unused bibliography entry</w:t></w:r></w:hyperlink></w:p>"#,
        &[("word/_rels/document.xml.rels", rels.as_bytes())],
    );
    assert!(matches!(
        &report.document.blocks[0].content[1],
        Inline::Citation { rendered_cache, .. } if rendered_cache.as_deref() == Some("1,2")
    ));
    assert_eq!(report.document.citation_database.citations.len(), 1);
    assert_eq!(report.document.citation_database.references.len(), 2);
    assert!(report
        .document
        .citation_database
        .references
        .iter()
        .all(|reference| reference.source.bytes == b"https://paperpile.com/c/Doc123/RefA+RefB"));
    assert!(report
        .document
        .citation_database
        .references
        .iter()
        .any(|reference| reference.summary.title == "Paperpile reference Doc123/RefA"));
    assert!(matches!(
        &report.document.blocks[1].content[0],
        Inline::Link { href, .. } if href.contains("/b/Doc123/UnusedReference")
    ));
    assert!(has_warning(&report, "paperpile-docx-citations"));
    assert!(report.document.validate().is_ok());
}

#[test]
fn paperpile_citation_identity_is_scoped_to_the_direct_link_document_key() {
    let rels = document_rels(
        r#"
<Relationship Id="rIdFirst" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://paperpile.com/c/DocOne/SharedKey" TargetMode="External"/>
<Relationship Id="rIdSecond" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://paperpile.com/c/DocTwo/SharedKey" TargetMode="External"/>"#,
    );
    let report = import_body(
        "paperpile-document-scope",
        r#"<w:p><w:hyperlink r:id="rIdFirst"><w:r><w:t>1</w:t></w:r></w:hyperlink></w:p>
<w:p><w:hyperlink r:id="rIdSecond"><w:r><w:t>2</w:t></w:r></w:hyperlink></w:p>"#,
        &[("word/_rels/document.xml.rels", rels.as_bytes())],
    );
    assert_eq!(report.document.citation_database.references.len(), 2);
    let sources = report
        .document
        .citation_database
        .references
        .iter()
        .map(|reference| String::from_utf8(reference.source.bytes.clone()).unwrap())
        .collect::<Vec<_>>();
    assert!(sources.contains(&"https://paperpile.com/c/DocOne/SharedKey".to_string()));
    assert!(sources.contains(&"https://paperpile.com/c/DocTwo/SharedKey".to_string()));
    assert!(report.document.validate().is_ok());
}

#[test]
fn locates_main_document_part_through_package_relationships() {
    let root_rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="/word/main.xml"/>
</Relationships>"#;
    let document = document_xml(r#"<w:p><w:r><w:t>Renamed part</w:t></w:r></w:p>"#);
    let report = import_package(
        "renamed-part",
        &[
            ("[Content_Types].xml", CONTENT_TYPES.as_bytes()),
            ("_rels/.rels", root_rels.as_bytes()),
            ("word/main.xml", document.as_bytes()),
        ],
    )
    .unwrap();
    assert_eq!(report.document.visible_text(), "Renamed part\n");
}

#[test]
fn run_property_detection_ignores_breaks_disabled_toggles_and_complex_script_variants() {
    let report = import_body(
        "run-props",
        r#"<w:p>
  <w:r><w:t>plain</w:t><w:br/><w:t>after break</w:t></w:r>
  <w:r><w:rPr><w:b w:val="0"/><w:i w:val="false"/><w:u w:val="none"/><w:strike w:val="off"/></w:rPr><w:t>disabled</w:t></w:r>
  <w:r><w:rPr><w:bCs/><w:iCs/><w:szCs w:val="40"/></w:rPr><w:t>complex script</w:t></w:r>
  <w:r><w:rPr><w:b/><w:i w:val="1"/><w:u w:val="single"/><w:dstrike/><w:highlight w:val="yellow"/><w:rFonts w:ascii="Arial" w:hAnsi="Arial"/><w:color w:val="AABBCC"/><w:sz w:val="24"/></w:rPr><w:t>enabled</w:t></w:r>
  <w:r><w:rPr><w:b w:val="true"/><w:caps/></w:rPr><w:t>explicit</w:t></w:r>
</w:p>"#,
        &[],
    );
    let block = &report.document.blocks[0];
    assert_eq!(block.content.len(), 5);
    assert!(inline_marks(block, "plain\nafter break").is_empty());
    assert!(inline_marks(block, "disabled").is_empty());
    assert!(inline_marks(block, "complex script").is_empty());
    let enabled = inline_marks(block, "enabled");
    for kind in [
        MarkKind::Bold,
        MarkKind::Italic,
        MarkKind::Underline,
        MarkKind::Strike,
        MarkKind::Background,
        MarkKind::Font,
        MarkKind::Color,
        MarkKind::Size,
    ] {
        assert!(enabled.contains(&kind), "missing {kind:?} in {enabled:?}");
    }
    let enabled_marks = text_marks(block);
    assert!(enabled_marks
        .iter()
        .any(|mark| mark.kind == MarkKind::Background && mark.value.as_deref() == Some("#ffff00")));
    assert!(enabled_marks
        .iter()
        .any(|mark| mark.kind == MarkKind::Font && mark.value.as_deref() == Some("Arial")));
    assert!(enabled_marks
        .iter()
        .any(|mark| mark.kind == MarkKind::Color && mark.value.as_deref() == Some("#aabbcc")));
    assert!(enabled_marks
        .iter()
        .any(|mark| mark.kind == MarkKind::Size && mark.value.as_deref() == Some("12")));
    assert_eq!(inline_marks(block, "explicit"), vec![MarkKind::Bold]);
    assert!(warning_message(&report, "docx-dropped-run-property").contains("caps"));
    assert!(report.document.validate().is_ok());
}

const NUMBERING_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:abstractNum w:abstractNumId="0">
    <w:lvl w:ilvl="0"><w:numFmt w:val="decimal"/></w:lvl>
    <w:lvl w:ilvl="1"><w:numFmt w:val="lowerLetter"/></w:lvl>
  </w:abstractNum>
  <w:abstractNum w:abstractNumId="1">
    <w:lvl w:ilvl="0"><w:numFmt w:val="bullet"/></w:lvl>
  </w:abstractNum>
  <w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>
  <w:num w:numId="2"><w:abstractNumId w:val="1"/></w:num>
  <w:num w:numId="3"><w:abstractNumId w:val="1"/><w:lvlOverride w:ilvl="0"><w:lvl w:ilvl="0"><w:numFmt w:val="upperRoman"/></w:lvl></w:lvlOverride></w:num>
</w:numbering>"#;

const NUMBERING_RELS: &str = r#"<Relationship Id="rIdNum" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering" Target="numbering.xml"/>
<Relationship Id="rIdStyles" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>"#;

#[test]
fn imports_numbering_start_from_abstract_and_instance_definitions() {
    let numbering = NUMBERING_XML
        .replacen(
            "<w:lvl w:ilvl=\"0\"><w:numFmt w:val=\"decimal\"/></w:lvl>",
            "<w:lvl w:ilvl=\"0\"><w:start w:val=\"7\"/><w:numFmt w:val=\"decimal\"/></w:lvl>",
            1,
        )
        .replace(
            "<w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/></w:num>",
            "<w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/><w:lvlOverride w:ilvl=\"1\"><w:startOverride w:val=\"11\"/></w:lvlOverride></w:num>",
        );
    let report = import_body(
        "numbering starts",
        r#"<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>Seven</w:t></w:r></w:p>
<w:p><w:pPr><w:numPr><w:ilvl w:val="1"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>Eleven</w:t></w:r></w:p>"#,
        &[
            ("word/_rels/document.xml.rels", NUMBERING_RELS.as_bytes()),
            ("word/numbering.xml", numbering.as_bytes()),
        ],
    );
    let list_id = report.document.blocks[0].list_id().expect("ordered item");
    let properties = report
        .document
        .list_properties
        .get(list_id)
        .expect("numbering starts are source state");
    assert_eq!(properties.start_for(0), 7);
    assert_eq!(properties.start_for(1), 11);
}

#[test]
fn numbered_and_bulleted_lists_share_one_list_id_per_numbering_instance() {
    let rels = document_rels(NUMBERING_RELS);
    let report = import_body(
        "lists",
        r#"<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>One</w:t></w:r></w:p>
<w:p><w:pPr><w:numPr><w:ilvl w:val="1"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>One a</w:t></w:r></w:p>
<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>Two</w:t></w:r></w:p>
<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="2"/></w:numPr></w:pPr><w:r><w:t>Bullet</w:t></w:r></w:p>
<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="3"/></w:numPr></w:pPr><w:r><w:t>Override</w:t></w:r></w:p>
<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="99"/></w:numPr></w:pPr><w:r><w:t>Unknown</w:t></w:r></w:p>"#,
        &[
            ("word/_rels/document.xml.rels", rels.as_bytes()),
            ("word/numbering.xml", NUMBERING_XML.as_bytes()),
        ],
    );
    let kinds: Vec<(StableId, u8, bool)> = report
        .document
        .blocks
        .iter()
        .map(|block| match &block.kind {
            BlockKind::ListItem {
                list_id,
                level,
                kind,
            } => (list_id.clone(), *level, kind.is_ordered()),
            other => panic!("expected list item, got {other:?}"),
        })
        .collect();
    assert_eq!(kinds.len(), 6);
    assert_eq!(kinds[0].0, kinds[1].0);
    assert_eq!(kinds[0].0, kinds[2].0);
    assert_ne!(kinds[0].0, kinds[3].0);
    assert_eq!((kinds[0].1, kinds[0].2), (0, true));
    assert_eq!((kinds[1].1, kinds[1].2), (1, true));
    assert_eq!((kinds[2].1, kinds[2].2), (0, true));
    assert_eq!((kinds[3].1, kinds[3].2), (0, false));
    assert_eq!((kinds[4].1, kinds[4].2), (0, true));
    assert_eq!((kinds[5].1, kinds[5].2), (0, false));
    assert!(warning_message(&report, "docx-unknown-list-definition").contains("1 occurrence"));
    assert!(report.document.validate().is_ok());
}

const STYLES_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:style w:type="paragraph" w:styleId="Normal"><w:name w:val="Normal"/></w:style>
  <w:style w:type="paragraph" w:styleId="Title"><w:name w:val="Title"/><w:pPr><w:outlineLvl w:val="0"/></w:pPr></w:style>
  <w:style w:type="paragraph" w:styleId="Subtitle"><w:name w:val="Subtitle"/></w:style>
  <w:style w:type="paragraph" w:styleId="Rubrik2"><w:name w:val="heading 2"/><w:basedOn w:val="Normal"/><w:rPr><w:b/></w:rPr></w:style>
  <w:style w:type="paragraph" w:styleId="MyHead"><w:name w:val="My Head"/><w:basedOn w:val="Rubrik2"/><w:rPr><w:i/></w:rPr></w:style>
  <w:style w:type="paragraph" w:styleId="ListNumber"><w:name w:val="List Number"/><w:pPr><w:numPr><w:numId w:val="1"/></w:numPr></w:pPr></w:style>
  <w:style w:type="character" w:styleId="Strong"><w:name w:val="Strong"/><w:rPr><w:b/></w:rPr></w:style>
</w:styles>"#;

#[test]
fn resolves_title_subtitle_heading_and_run_styles_through_based_on_chains() {
    let rels = document_rels(NUMBERING_RELS);
    let report = import_body(
        "styles",
        r#"<w:p><w:pPr><w:pStyle w:val="Title"/></w:pPr><w:r><w:t>Document title</w:t></w:r></w:p>
<w:p><w:pPr><w:pStyle w:val="Subtitle"/></w:pPr><w:r><w:t>Document subtitle</w:t></w:r></w:p>
<w:p><w:pPr><w:pStyle w:val="MyHead"/></w:pPr><w:r><w:t>Custom heading</w:t></w:r><w:r><w:rPr><w:b w:val="0"/></w:rPr><w:t>unbold</w:t></w:r></w:p>
<w:p><w:pPr><w:pStyle w:val="ListNumber"/></w:pPr><w:r><w:t>Styled list</w:t></w:r></w:p>
<w:p><w:r><w:rPr><w:rStyle w:val="Strong"/></w:rPr><w:t>strong run</w:t></w:r></w:p>"#,
        &[
            ("word/_rels/document.xml.rels", rels.as_bytes()),
            ("word/numbering.xml", NUMBERING_XML.as_bytes()),
            ("word/styles.xml", STYLES_XML.as_bytes()),
        ],
    );
    let blocks = &report.document.blocks;
    assert!(matches!(blocks[0].kind, BlockKind::Title));
    assert!(matches!(blocks[1].kind, BlockKind::Subtitle));
    assert!(matches!(blocks[2].kind, BlockKind::Heading { level: 2 }));
    let custom = inline_marks(&blocks[2], "Custom heading");
    assert!(custom.contains(&MarkKind::Bold) && custom.contains(&MarkKind::Italic));
    assert_eq!(inline_marks(&blocks[2], "unbold"), vec![MarkKind::Italic]);
    assert!(matches!(
        blocks[3].kind,
        BlockKind::ListItem {
            kind: opendoc_core::ListKind::Ordered,
            level: 0,
            ..
        }
    ));
    assert_eq!(inline_marks(&blocks[4], "strong run"), vec![MarkKind::Bold]);
    assert!(report.document.validate().is_ok());
}

#[test]
fn imports_footnotes_and_endnotes_with_distinct_document_placement() {
    let rels = document_rels(
        r#"<Relationship Id="rIdFn" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes" Target="footnotes.xml"/>
<Relationship Id="rIdEn" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/endnotes" Target="endnotes.xml"/>"#,
    );
    let footnotes = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:footnote w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:footnote>
  <w:footnote w:type="continuationSeparator" w:id="0"><w:p><w:r><w:continuationSeparator/></w:r></w:p></w:footnote>
  <w:footnote w:id="1"><w:p><w:r><w:rPr><w:vertAlign w:val="superscript"/></w:rPr><w:footnoteRef/></w:r><w:r><w:t xml:space="preserve"> Note </w:t></w:r><w:r><w:rPr><w:i/></w:rPr><w:t>body</w:t></w:r></w:p><w:p><w:r><w:t>second paragraph</w:t></w:r></w:p></w:footnote>
  <w:footnote w:id="2"><w:p><w:r><w:t xml:space="preserve">   </w:t></w:r></w:p></w:footnote>
</w:footnotes>"#;
    let endnotes = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:endnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:endnote w:id="1"><w:p><w:r><w:t>End note body</w:t></w:r></w:p></w:endnote>
</w:endnotes>"#;
    let report = import_body(
        "footnotes",
        r#"<w:p><w:r><w:t>Text</w:t></w:r><w:r><w:rPr><w:vertAlign w:val="superscript"/></w:rPr><w:footnoteReference w:id="1"/></w:r><w:r><w:t> more</w:t></w:r><w:r><w:endnoteReference w:id="1"/></w:r><w:r><w:footnoteReference w:id="2"/></w:r><w:r><w:footnoteReference w:id="42"/></w:r></w:p>"#,
        &[
            ("word/_rels/document.xml.rels", rels.as_bytes()),
            ("word/footnotes.xml", footnotes.as_bytes()),
            ("word/endnotes.xml", endnotes.as_bytes()),
        ],
    );
    let block = &report.document.blocks[0];
    let refs: Vec<&StableId> = block
        .content
        .iter()
        .filter_map(|inline| match inline {
            Inline::FootnoteRef { footnote_id, .. } => Some(footnote_id),
            _ => None,
        })
        .collect();
    assert_eq!(refs.len(), 2, "{:?}", block.content);
    assert_eq!(report.document.footnotes.len(), 2);
    let footnote = report
        .document
        .footnotes
        .iter()
        .find(|footnote| &footnote.id == refs[0])
        .unwrap();
    let footnote_text: String = footnote
        .body
        .iter()
        .map(|inline| match inline {
            Inline::Text { text, .. } => text.clone(),
            _ => String::new(),
        })
        .collect();
    assert_eq!(footnote_text, " Note body\nsecond paragraph");
    assert!(footnote.body.iter().any(|inline| matches!(
        inline,
        Inline::Text { text, marks, .. } if text == "body" && marks.iter().any(|m| m.kind == MarkKind::Italic)
    )));
    let endnote = report
        .document
        .footnotes
        .iter()
        .find(|footnote| &footnote.id == refs[1])
        .unwrap();
    assert!(matches!(&endnote.body[0], Inline::Text { text, .. } if text == "End note body"));
    assert!(warning_message(&report, "docx-missing-footnote").contains("2 occurrences"));
    assert!(warning_message(&report, "docx-empty-footnote").contains("1 occurrence"));
    assert!(report.document.endnote_ids.contains(&endnote.id));
    assert!(!report.document.endnote_ids.contains(&footnote.id));
    assert!(report.document.validate().is_ok());
}

#[test]
fn imports_comment_threads_anchored_to_commented_ranges_with_replies() {
    let rels = document_rels(
        r#"<Relationship Id="rIdC" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments" Target="comments.xml"/>
<Relationship Id="rIdCx" Type="http://schemas.microsoft.com/office/2011/relationships/commentsExtended" Target="commentsExtended.xml"/>"#,
    );
    let comments = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:comments xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml">
  <w:comment w:id="0" w:author="Ann Author" w:date="2024-01-15T10:30:00Z" w:initials="AA">
    <w:p w14:paraId="1111AAAA"><w:r><w:rPr><w:rStyle w:val="CommentReference"/></w:rPr><w:annotationRef/></w:r><w:r><w:t>Please &amp; check</w:t></w:r></w:p>
  </w:comment>
  <w:comment w:id="1" w:author="Bob" w:date="2024-01-15T11:00:00Z">
    <w:p w14:paraId="2222BBBB"><w:r><w:t>Reply text</w:t></w:r></w:p>
  </w:comment>
  <w:comment w:id="2" w:author=" " w:date="bad date">
    <w:p w14:paraId="3333CCCC"><w:r><w:t>Unanchored</w:t></w:r></w:p>
  </w:comment>
  <w:comment w:id="3" w:author="Empty"><w:p><w:r><w:t> </w:t></w:r></w:p></w:comment>
</w:comments>"#;
    let comments_extended = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w15:commentsEx xmlns:w15="http://schemas.microsoft.com/office/word/2012/wordml">
  <w15:commentEx w15:paraId="1111AAAA" w15:done="0"/>
  <w15:commentEx w15:paraId="2222BBBB" w15:paraIdParent="1111AAAA" w15:done="0"/>
</w15:commentsEx>"#;
    let report = import_body(
        "comments",
        r#"<w:p><w:r><w:t>Before </w:t></w:r><w:commentRangeStart w:id="0"/><w:r><w:t>target one</w:t></w:r><w:r><w:rPr><w:b/></w:rPr><w:t>target two</w:t></w:r><w:commentRangeEnd w:id="0"/><w:r><w:commentReference w:id="0"/></w:r><w:r><w:t> after</w:t></w:r></w:p>
<w:p><w:r><w:t>Second</w:t></w:r><w:r><w:commentReference w:id="2"/></w:r></w:p>
<w:p><w:commentRangeStart w:id="3"/><w:r><w:t>Third</w:t></w:r><w:commentRangeEnd w:id="3"/></w:p>"#,
        &[
            ("word/_rels/document.xml.rels", rels.as_bytes()),
            ("word/comments.xml", comments.as_bytes()),
            ("word/commentsExtended.xml", comments_extended.as_bytes()),
        ],
    );
    assert_eq!(
        report.document.visible_text(),
        "Before target onetarget two after\nSecond\nThird\n"
    );
    assert_eq!(report.document.comments.len(), 2);
    let thread = &report.document.comments[0];
    assert_eq!(thread.comments.len(), 2);
    assert_eq!(thread.comments[0].author, "Ann Author");
    assert_eq!(thread.comments[0].created_at_ms, 1_705_314_600_000);
    assert!(matches!(
        &thread.comments[0].body[0],
        Inline::Text { text, .. } if text == "Please & check"
    ));
    assert_eq!(thread.comments[1].author, "Bob");
    assert!(matches!(
        &thread.comments[1].body[0],
        Inline::Text { text, .. } if text == "Reply text"
    ));
    let start = inline_id_with_text(&report, "target one");
    let end = inline_id_with_text(&report, "target two");
    assert!(matches!(
        &thread.anchor,
        Anchor::TextRange(range) if range.start == start && range.end == end
    ));
    let degraded = &report.document.comments[1];
    assert_eq!(degraded.comments.len(), 1);
    assert_eq!(degraded.comments[0].author, "Unknown");
    assert_eq!(degraded.comments[0].created_at_ms, 0);
    assert!(matches!(
        &degraded.anchor,
        Anchor::NearestBlock { block_id, .. } if block_id == &report.document.blocks[1].id
    ));
    assert!(has_warning(&report, "docx-comment-anchor-degraded"));
    assert!(warning_message(&report, "docx-empty-comment").contains("1 occurrence"));
    assert!(report.document.validate().is_ok());
}

#[test]
fn imports_tracked_changes_as_insert_delete_and_format_suggestions() {
    let report = import_body(
        "tracked-changes",
        r#"<w:p>
  <w:r><w:t>Keep </w:t></w:r>
  <w:ins w:id="1" w:author="Ann" w:date="2024-01-15T10:30:00Z"><w:r><w:rPr><w:b/></w:rPr><w:t>added</w:t></w:r></w:ins>
  <w:del w:id="2" w:author="Bob" w:date="2024-01-16T10:30:00Z"><w:r><w:delText>gone</w:delText></w:r><w:r><w:delText> too</w:delText></w:r></w:del>
  <w:r><w:rPr><w:b/><w:rPrChange w:id="3" w:author="Cy" w:date="2024-01-17T10:30:00Z"><w:rPr/></w:rPrChange></w:rPr><w:t>bolded</w:t></w:r>
  <w:r><w:rPr><w:rPrChange w:id="4" w:author="Di"><w:rPr><w:i/></w:rPr></w:rPrChange></w:rPr><w:t>unitalic</w:t></w:r>
</w:p>
<w:p><w:ins w:id="5" w:author="Eve"><w:r><w:t>Whole paragraph</w:t></w:r></w:ins></w:p>
<w:p><w:ins w:id="6" w:author="Eve"><w:r><w:t xml:space="preserve"> </w:t></w:r></w:ins><w:r><w:t>Tail</w:t></w:r></w:p>"#,
        &[],
    );
    assert_eq!(
        report.document.visible_text(),
        "Keep gone tooboldedunitalic\nTail\n"
    );
    let suggestions = &report.document.suggestions;
    assert_eq!(suggestions.len(), 4, "{suggestions:?}");
    assert!(suggestions
        .iter()
        .all(|suggestion| suggestion.state == SuggestionState::Proposed));

    let keep = inline_id_with_text(&report, "Keep ");
    let insert = &suggestions[0];
    assert_eq!(insert.author, "Ann");
    assert!(insert.provenance.contains(&"docx:ins:1".to_string()));
    assert!(insert
        .provenance
        .contains(&"docx-date:2024-01-15T10:30:00Z".to_string()));
    match &insert.kind {
        SuggestionKind::Insert { anchor, content } => {
            assert!(matches!(
                anchor,
                Anchor::TextRange(range) if range.start == keep && range.end == keep
            ));
            assert!(matches!(
                &content[0],
                Inline::Text { text, marks, .. }
                    if text == "added" && marks.iter().any(|mark| mark.kind == MarkKind::Bold)
            ));
        }
        other => panic!("expected insert suggestion, got {other:?}"),
    }

    let delete = &suggestions[1];
    assert_eq!(delete.author, "Bob");
    let gone = inline_id_with_text(&report, "gone");
    let too = inline_id_with_text(&report, " too");
    assert!(matches!(
        &delete.kind,
        SuggestionKind::Delete { range } if range.start == gone && range.end == too
    ));

    let format = &suggestions[2];
    assert_eq!(format.author, "Cy");
    let bolded = inline_id_with_text(&report, "bolded");
    assert!(inline_marks(&report.document.blocks[0], "bolded").is_empty());
    assert!(matches!(
        &format.kind,
        SuggestionKind::Format { range, marks }
            if range.start == bolded && range.end == bolded
                && marks.len() == 1 && marks[0].kind == MarkKind::Bold
    ));
    assert_eq!(
        inline_marks(&report.document.blocks[0], "unitalic"),
        vec![MarkKind::Italic]
    );
    assert!(warning_message(&report, "docx-dropped-format-change").contains("1 occurrence"));

    let whole = &suggestions[3];
    assert_eq!(whole.author, "Eve");
    let placeholder = inline_id_with_text(&report, "");
    assert!(matches!(
        &whole.kind,
        SuggestionKind::Insert { anchor: Anchor::TextRange(range), .. }
            if range.start == placeholder
    ));
    assert_eq!(report.document.blocks[1].content.len(), 1);
    assert!(report.document.validate().is_ok());
}

const IMAGE_RELS: &str = r#"<Relationship Id="rIdImage1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/image1.png"/>"#;

fn drawing_xml(rel_id: &str, descr: &str) -> String {
    format!(
        r#"<w:drawing><wp:inline><wp:docPr id="1" name="Picture 1" descr="{descr}"/><a:graphic><a:graphicData><pic:pic><pic:blipFill><a:blip r:embed="{rel_id}"/></pic:blipFill></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing>"#
    )
}

#[test]
fn imports_docx_standalone_image_as_content_addressed_blob() {
    let rels = document_rels(IMAGE_RELS);
    let image_bytes = b"fake-png-bytes";
    let body = format!(
        r#"<w:p><w:r><w:t>Before image</w:t></w:r></w:p>
<w:p><w:r>{}</w:r></w:p>
<w:p><w:r><w:t>After image</w:t></w:r></w:p>"#,
        drawing_xml("rIdImage1", "Imported &amp; described figure")
    );
    let report = import_body(
        "image",
        &body,
        &[
            ("word/_rels/document.xml.rels", rels.as_bytes()),
            ("word/media/image1.png", image_bytes),
        ],
    );
    let expected_hash = opendoc_core::digest_bytes("sha256", image_bytes)
        .unwrap()
        .to_string();
    assert_eq!(report.blobs.len(), 1);
    assert_eq!(report.blobs[0].name, "image1.png");
    assert_eq!(report.blobs[0].media_type, "image/png");
    assert_eq!(report.blobs[0].hash, expected_hash);
    assert_eq!(report.blobs[0].bytes, image_bytes);
    assert_eq!(report.document.blocks.len(), 3);
    assert!(matches!(
        &report.document.blocks[1].kind,
        BlockKind::Image { blob_hash, alt_text, .. }
            if blob_hash == &expected_hash && alt_text == "Imported & described figure"
    ));
    assert_eq!(
        report.document.visible_text(),
        "Before image\nImported & described figure\nAfter image\n"
    );
    assert!(report.warnings.is_empty());
    assert!(report.document.validate().is_ok());
}

#[test]
fn imports_both_drawingml_image_title_and_description_as_ordered_alt_text() {
    let rels = document_rels(IMAGE_RELS);
    let body = r#"<w:p><w:r><w:drawing><wp:inline>
        <wp:docPr id="1" name="Picture 1" title="Quarterly results" descr="Bar chart comparing Q1 and Q2"/>
        <a:graphic><a:graphicData><pic:pic><pic:nvPicPr>
          <pic:cNvPr id="0" name="Picture 1" descr="fallback description"/>
        </pic:nvPicPr><pic:blipFill><a:blip r:embed="rIdImage1"/></pic:blipFill></pic:pic></a:graphicData></a:graphic>
    </wp:inline></w:drawing></w:r></w:p>"#;
    let report = import_body(
        "image-accessibility",
        body,
        &[
            ("word/_rels/document.xml.rels", rels.as_bytes()),
            ("word/media/image1.png", b"fake-png-bytes"),
        ],
    );

    assert!(matches!(
        &report.document.blocks[0].kind,
        BlockKind::Image { alt_text, .. }
            if alt_text == "Quarterly results\nBar chart comparing Q1 and Q2"
    ));
    assert_eq!(
        report.document.visible_text(),
        "Quarterly results\nBar chart comparing Q1 and Q2\n"
    );
}

#[test]
fn drawingml_picture_metadata_is_not_spliced_into_an_authoritative_docpr_record() {
    let rels = document_rels(IMAGE_RELS);
    let body = r#"<w:p><w:r><w:drawing><wp:inline>
        <wp:docPr id="1" name="Picture 1" descr="Authoritative description"/>
        <a:graphic><a:graphicData><pic:pic><pic:nvPicPr>
          <pic:cNvPr id="0" name="Picture 1" title="Conflicting fallback title" descr="Conflicting fallback description"/>
        </pic:nvPicPr><pic:blipFill><a:blip r:embed="rIdImage1"/></pic:blipFill></pic:pic></a:graphicData></a:graphic>
    </wp:inline></w:drawing></w:r></w:p>"#;
    let report = import_body(
        "image-accessibility-authority",
        body,
        &[
            ("word/_rels/document.xml.rels", rels.as_bytes()),
            ("word/media/image1.png", b"fake-png-bytes"),
        ],
    );

    assert!(matches!(
        &report.document.blocks[0].kind,
        BlockKind::Image { alt_text, .. } if alt_text == "Authoritative description"
    ));
    assert!(report.document.validate().is_ok());
}

#[test]
fn drawingml_picture_metadata_is_a_whole_record_fallback_when_docpr_has_no_accessible_text() {
    let rels = document_rels(IMAGE_RELS);
    let body = r#"<w:p><w:r><w:drawing><wp:inline>
        <wp:docPr id="1" name="Picture 1"/>
        <a:graphic><a:graphicData><pic:pic><pic:nvPicPr>
          <pic:cNvPr id="0" name="Picture 1" title="Fallback title" descr="Fallback description"/>
        </pic:nvPicPr><pic:blipFill><a:blip r:embed="rIdImage1"/></pic:blipFill></pic:pic></a:graphicData></a:graphic>
    </wp:inline></w:drawing></w:r></w:p>"#;
    let report = import_body(
        "image-accessibility-fallback",
        body,
        &[
            ("word/_rels/document.xml.rels", rels.as_bytes()),
            ("word/media/image1.png", b"fake-png-bytes"),
        ],
    );

    assert!(matches!(
        &report.document.blocks[0].kind,
        BlockKind::Image { alt_text, .. } if alt_text == "Fallback title\nFallback description"
    ));
    assert!(report.document.validate().is_ok());
}

#[test]
fn imports_docx_image_extent_as_its_document_size() {
    let rels = document_rels(IMAGE_RELS);
    let body = r#"<w:p><w:r><w:drawing><wp:inline>
        <wp:extent cx="914400" cy="457200"/>
        <wp:docPr id="1" name="Picture 1"/>
        <a:graphic><a:graphicData><pic:pic><pic:blipFill>
          <a:blip r:embed="rIdImage1"/>
          <a:srcRect l="5000" t="10000" r="20000" b="30000"/>
        </pic:blipFill><pic:spPr><a:xfrm rot="5400000"/><a:ln w="12700"><a:solidFill><a:srgbClr val="123456"/></a:solidFill><a:prstDash val="dash"/></a:ln></pic:spPr>
        </pic:pic></a:graphicData></a:graphic>
    </wp:inline></w:drawing></w:r></w:p>"#;
    let report = import_body(
        "image-size",
        body,
        &[
            ("word/_rels/document.xml.rels", rels.as_bytes()),
            ("word/media/image1.png", b"fake-png-bytes"),
        ],
    );
    let BlockKind::Image { layout, .. } = &report.document.blocks[0].kind else {
        panic!("expected image block");
    };
    assert_eq!(layout.width.map(|length| length.twips()), Some(1440));
    assert_eq!(layout.height.map(|length| length.twips()), Some(720));
    assert_eq!(layout.rotation_degrees, Some(90));
    assert_eq!(
        layout.border,
        Some(
            opendoc_core::CellBorder::new(
                opendoc_core::BorderStyle::Dashed,
                opendoc_core::Length::from_twips(20).unwrap(),
                opendoc_core::Color::parse("#123456").unwrap(),
            )
            .unwrap()
        )
    );
    assert_eq!(
        layout.crop,
        Some(opendoc_core::ImageCrop {
            top_percent: 10,
            right_percent: 20,
            bottom_percent: 30,
            left_percent: 5,
        })
    );
    assert!(report.document.validate().is_ok());
}

#[test]
fn names_a_visible_drawingml_image_border_outside_the_supported_subset() {
    let rels = document_rels(IMAGE_RELS);
    let body = r#"<w:p><w:r><w:drawing><wp:inline>
        <wp:docPr id="1" name="Picture 1"/>
        <a:graphic><a:graphicData><pic:pic><pic:blipFill><a:blip r:embed="rIdImage1"/></pic:blipFill>
        <pic:spPr><a:ln w="12700"><a:solidFill><a:schemeClr val="accent1"/></a:solidFill><a:prstDash val="lgDash"/></a:ln></pic:spPr>
        </pic:pic></a:graphicData></a:graphic>
    </wp:inline></w:drawing></w:r></w:p>"#;
    let report = import_body(
        "image-unsupported-border",
        body,
        &[
            ("word/_rels/document.xml.rels", rels.as_bytes()),
            ("word/media/image1.png", b"fake-png-bytes"),
        ],
    );
    let BlockKind::Image { layout, .. } = &report.document.blocks[0].kind else {
        panic!("expected image block");
    };
    assert_eq!(layout.border, None);
    assert!(warning_message(&report, "docx-dropped-image-border").contains("solid/dashed/dotted"));
    assert!(report.document.validate().is_ok());
}

#[test]
fn imports_drawingml_image_rotation_at_nearest_model_degree_and_canonicalises_full_turns() {
    let rels = document_rels(IMAGE_RELS);
    let body = r#"
      <w:p><w:r><w:drawing><wp:inline><wp:docPr id="1" name="Positive"/>
        <a:graphic><a:graphicData><pic:pic><pic:blipFill><a:blip r:embed="rIdImage1"/></pic:blipFill>
        <pic:spPr><a:xfrm rot="59999"/></pic:spPr></pic:pic></a:graphicData></a:graphic>
      </wp:inline></w:drawing></w:r></w:p>
      <w:p><w:r><w:drawing><wp:inline><wp:docPr id="2" name="Negative"/>
        <a:graphic><a:graphicData><pic:pic><pic:blipFill><a:blip r:embed="rIdImage1"/></pic:blipFill>
        <pic:spPr><a:xfrm rot="-59999"/></pic:spPr></pic:pic></a:graphicData></a:graphic>
      </wp:inline></w:drawing></w:r></w:p>
      <w:p><w:r><w:drawing><wp:inline><wp:docPr id="3" name="Full turn"/>
        <a:graphic><a:graphicData><pic:pic><pic:blipFill><a:blip r:embed="rIdImage1"/></pic:blipFill>
        <pic:spPr><a:xfrm rot="-21600000"/></pic:spPr></pic:pic></a:graphicData></a:graphic>
      </wp:inline></w:drawing></w:r></w:p>"#;
    let report = import_body(
        "image-rotation-rounding",
        body,
        &[
            ("word/_rels/document.xml.rels", rels.as_bytes()),
            ("word/media/image1.png", b"fake-png-bytes"),
        ],
    );
    let rotations = report
        .document
        .blocks
        .iter()
        .map(|block| match &block.kind {
            BlockKind::Image { layout, .. } => layout.rotation_degrees,
            other => panic!("expected image, got {other:?}"),
        })
        .collect::<Vec<_>>();
    assert_eq!(rotations, vec![Some(1), Some(-1), None]);
    assert!(report.document.validate().is_ok());
}

#[test]
fn imports_drawingml_image_opacity_at_model_percent_precision() {
    let rels = document_rels(IMAGE_RELS);
    let body = r#"<w:p><w:r><w:drawing><wp:inline>
        <wp:docPr id="1" name="Picture 1" descr="Transparent"/>
        <a:graphic><a:graphicData><pic:pic><pic:blipFill>
          <a:blip r:embed="rIdImage1"><a:alphaModFix amt="60500"/></a:blip>
        </pic:blipFill></pic:pic></a:graphicData></a:graphic>
    </wp:inline></w:drawing></w:r></w:p>"#;
    let report = import_body(
        "opacity",
        body,
        &[
            ("word/_rels/document.xml.rels", rels.as_bytes()),
            ("word/media/image1.png", b"fake-png-bytes"),
        ],
    );
    let BlockKind::Image { layout, .. } = &report.document.blocks[0].kind else {
        panic!("expected image");
    };
    assert_eq!(layout.opacity_percent, Some(61));
}

#[test]
fn malformed_drawingml_image_opacity_is_named_instead_of_looking_absent() {
    let rels = document_rels(IMAGE_RELS);
    let body = r#"<w:p><w:r><w:drawing><wp:inline>
        <wp:docPr id="1" name="Picture 1"/>
        <a:graphic><a:graphicData><pic:pic><pic:blipFill>
          <a:blip r:embed="rIdImage1"><a:alphaModFix amt="100001"/></a:blip>
        </pic:blipFill></pic:pic></a:graphicData></a:graphic>
    </wp:inline></w:drawing></w:r></w:p>"#;
    let report = import_body(
        "malformed-opacity",
        body,
        &[
            ("word/_rels/document.xml.rels", rels.as_bytes()),
            ("word/media/image1.png", b"fake-png-bytes"),
        ],
    );
    let BlockKind::Image { layout, .. } = &report.document.blocks[0].kind else {
        panic!("expected image");
    };
    assert!(layout.opacity_percent.is_none());
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.code == "docx-dropped-image-opacity"));
}

#[test]
fn malformed_or_empty_drawingml_crop_is_named_instead_of_looking_uncropped() {
    let rels = document_rels(IMAGE_RELS);
    let body = r#"
      <w:p><w:r><w:drawing><wp:inline><wp:docPr id="1" name="Malformed crop"/>
        <a:graphic><a:graphicData><pic:pic><pic:blipFill>
          <a:blip r:embed="rIdImage1"/><a:srcRect l="not-a-number"/>
        </pic:blipFill></pic:pic></a:graphicData></a:graphic>
      </wp:inline></w:drawing></w:r></w:p>
      <w:p><w:r><w:drawing><wp:inline><wp:docPr id="2" name="Empty crop"/>
        <a:graphic><a:graphicData><pic:pic><pic:blipFill>
          <a:blip r:embed="rIdImage1"/><a:srcRect l="99000" r="99000"/>
        </pic:blipFill></pic:pic></a:graphicData></a:graphic>
      </wp:inline></w:drawing></w:r></w:p>
      <w:p><w:r><w:drawing><wp:inline><wp:docPr id="3" name="Identity crop"/>
        <a:graphic><a:graphicData><pic:pic><pic:blipFill>
          <a:blip r:embed="rIdImage1"/><a:srcRect/>
        </pic:blipFill></pic:pic></a:graphicData></a:graphic>
      </wp:inline></w:drawing></w:r></w:p>"#;
    let report = import_body(
        "malformed-crop",
        body,
        &[
            ("word/_rels/document.xml.rels", rels.as_bytes()),
            ("word/media/image1.png", b"fake-png-bytes"),
        ],
    );
    assert!(report.document.blocks.iter().all(|block| matches!(
        &block.kind,
        BlockKind::Image { layout, .. } if layout.crop.is_none()
    )));
    let crop_warning = report
        .warnings
        .iter()
        .find(|warning| warning.code == "docx-dropped-image-crop")
        .expect("a source identity crop is harmless; malformed and no-visible-pixel crops are not");
    assert!(crop_warning.message.contains("2 occurrences"));
}

#[test]
fn imports_unmapped_positioned_docx_image_with_explicit_warning() {
    let rels = document_rels(IMAGE_RELS);
    let body = r#"<w:p><w:r><w:drawing><wp:anchor behindDoc="1">
        <wp:positionH relativeFrom="page"><wp:posOffset>-152400</wp:posOffset></wp:positionH>
        <wp:positionV relativeFrom="page"><wp:posOffset>304800</wp:posOffset></wp:positionV>
        <wp:wrapNone/><wp:docPr id="1" name="Picture 1"/>
        <a:graphic><a:graphicData><pic:pic><pic:blipFill><a:blip r:embed="rIdImage1"/></pic:blipFill></pic:pic></a:graphicData></a:graphic>
    </wp:anchor></w:drawing></w:r></w:p>"#;
    let report = import_body(
        "positioned-image",
        body,
        &[
            ("word/_rels/document.xml.rels", rels.as_bytes()),
            ("word/media/image1.png", b"fake-png-bytes"),
        ],
    );
    assert!(has_warning(&report, "docx-dropped-positioned-image"));
    let BlockKind::Image { layout, .. } = &report.document.blocks[0].kind else {
        panic!("expected image block");
    };
    assert!(layout.positioned.is_none());
    assert!(layout.placement.is_none());
}

#[test]
fn non_default_square_wrap_semantics_are_named_before_the_side_is_approximated() {
    let rels = document_rels(IMAGE_RELS);
    let body = r#"<w:p><w:r><w:drawing><wp:anchor behindDoc="1" relativeHeight="9">
        <wp:positionH relativeFrom="column"><wp:align>left</wp:align></wp:positionH>
        <wp:positionV relativeFrom="paragraph"><wp:posOffset>0</wp:posOffset></wp:positionV>
        <wp:wrapSquare wrapText="left"/><wp:docPr id="1" name="Picture 1"/>
        <a:graphic><a:graphicData><pic:pic><pic:blipFill><a:blip r:embed="rIdImage1"/></pic:blipFill></pic:pic></a:graphicData></a:graphic>
    </wp:anchor></w:drawing></w:r></w:p>"#;
    let report = import_body(
        "non-default-square-wrap",
        body,
        &[
            ("word/_rels/document.xml.rels", rels.as_bytes()),
            ("word/media/image1.png", b"fake-png-bytes"),
        ],
    );
    assert!(has_warning(&report, "docx-dropped-positioned-image"));
    let BlockKind::Image { layout, .. } = &report.document.blocks[0].kind else {
        panic!("expected image block");
    };
    assert_eq!(
        layout.placement,
        Some(opendoc_core::ImagePlacement::WrapStart)
    );
    assert!(layout.positioned.is_none());
}

#[test]
fn imports_margin_anchored_no_wrap_docx_image_as_positioned_page_content() {
    let rels = document_rels(IMAGE_RELS);
    let body = r#"<w:p><w:r><w:drawing><wp:anchor behindDoc="1">
        <wp:positionH relativeFrom="margin"><wp:posOffset>-152400</wp:posOffset></wp:positionH>
        <wp:positionV relativeFrom="margin"><wp:posOffset>304800</wp:posOffset></wp:positionV>
        <wp:wrapNone/><wp:docPr id="1" name="Picture 1"/>
        <a:graphic><a:graphicData><pic:pic><pic:blipFill><a:blip r:embed="rIdImage1"/></pic:blipFill></pic:pic></a:graphicData></a:graphic>
    </wp:anchor></w:drawing></w:r></w:p>"#;
    let report = import_body(
        "positioned-image",
        body,
        &[
            ("word/_rels/document.xml.rels", rels.as_bytes()),
            ("word/media/image1.png", b"fake-png-bytes"),
        ],
    );
    assert!(!has_warning(&report, "docx-dropped-positioned-image"));
    let BlockKind::Image { layout, .. } = &report.document.blocks[0].kind else {
        panic!("expected image block");
    };
    assert_eq!(
        layout.positioned,
        Some(opendoc_core::PositionedImage {
            anchor: opendoc_core::PositionedImageAnchor::PageContent,
            horizontal_offset: opendoc_core::Length::from_twips(-240).unwrap(),
            vertical_offset: opendoc_core::Length::from_twips(480).unwrap(),
            layer: opendoc_core::PositionedImageLayer::BehindText,
        })
    );
    assert!(report.document.validate().is_ok());
}

#[test]
fn margin_anchored_no_wrap_image_with_explicit_z_order_is_named_before_fallback() {
    let rels = document_rels(IMAGE_RELS);
    let body = r#"<w:p><w:r><w:drawing><wp:anchor behindDoc="1" relativeHeight="42">
        <wp:positionH relativeFrom="margin"><wp:posOffset>-152400</wp:posOffset></wp:positionH>
        <wp:positionV relativeFrom="margin"><wp:posOffset>304800</wp:posOffset></wp:positionV>
        <wp:wrapNone/><wp:docPr id="1" name="Picture 1"/>
        <a:graphic><a:graphicData><pic:pic><pic:blipFill><a:blip r:embed="rIdImage1"/></pic:blipFill></pic:pic></a:graphicData></a:graphic>
    </wp:anchor></w:drawing></w:r></w:p>"#;
    let report = import_body(
        "positioned-z-order",
        body,
        &[
            ("word/_rels/document.xml.rels", rels.as_bytes()),
            ("word/media/image1.png", b"fake-png-bytes"),
        ],
    );
    assert!(has_warning(&report, "docx-dropped-positioned-image"));
    let BlockKind::Image { layout, .. } = &report.document.blocks[0].kind else {
        panic!("expected image block");
    };
    assert!(layout.positioned.is_none());
    assert!(layout.placement.is_none());
}

#[test]
fn missing_docx_image_media_imports_placeholder_with_warning() {
    let rels = document_rels(
        r#"<Relationship Id="rIdImage1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/missing.png"/>"#,
    );
    let body = format!(
        r#"<w:p><w:r><w:t>Before image</w:t></w:r></w:p>
<w:p><w:r>{}</w:r></w:p>
<w:p><w:r><w:t>After image</w:t></w:r></w:p>"#,
        drawing_xml("rIdImage1", "")
    );
    let report = import_body(
        "missing-image",
        &body,
        &[("word/_rels/document.xml.rels", rels.as_bytes())],
    );
    assert!(report.blobs.is_empty());
    assert_eq!(report.warnings.len(), 1);
    assert_eq!(report.warnings[0].code, "missing-docx-image-blob");
    assert_eq!(report.document.warnings, report.warnings);
    assert_eq!(report.document.blocks.len(), 3);
    assert_eq!(
        report.document.visible_text(),
        "Before image\n[missing DOCX image: missing.png]\nAfter image\n"
    );
    assert!(matches!(
        report.document.blocks[1].kind,
        BlockKind::Paragraph
    ));
    assert!(report.document.validate().is_ok());
}

#[test]
fn inline_images_mixed_with_text_become_standalone_image_blocks_at_the_run_position() {
    let rels = document_rels(IMAGE_RELS);
    let image_bytes = b"inline-png";
    let body = format!(
        r#"<w:p><w:pPr><w:pStyle w:val="Heading3"/></w:pPr><w:r><w:t>Before </w:t></w:r><w:r>{}<w:t> after</w:t></w:r></w:p>"#,
        drawing_xml("rIdImage1", "Figure")
    );
    let report = import_body(
        "inline-image",
        &body,
        &[
            ("word/_rels/document.xml.rels", rels.as_bytes()),
            ("word/media/image1.png", image_bytes),
        ],
    );
    let blocks = &report.document.blocks;
    assert_eq!(blocks.len(), 3, "{blocks:?}");
    assert!(matches!(blocks[0].kind, BlockKind::Heading { level: 3 }));
    assert!(matches!(&blocks[1].kind, BlockKind::Image { alt_text, .. } if alt_text == "Figure"));
    assert!(matches!(blocks[2].kind, BlockKind::Heading { level: 3 }));
    assert_eq!(report.document.visible_text(), "Before \nFigure\n after\n");
    assert!(warning_message(&report, "docx-split-inline-image").contains("1 occurrence"));
    assert!(report.document.validate().is_ok());
}

#[test]
fn page_break_inside_a_paragraph_splits_into_a_page_break_block() {
    let report = import_body(
        "page-break-split",
        r#"<w:p><w:r><w:t>A</w:t><w:br w:type="page"/><w:t>B</w:t></w:r></w:p>
<w:p><w:pPr><w:pageBreakBefore/></w:pPr><w:r><w:t>C</w:t></w:r></w:p>"#,
        &[],
    );
    let kinds: Vec<&BlockKind> = report
        .document
        .blocks
        .iter()
        .map(|block| &block.kind)
        .collect();
    assert!(matches!(
        kinds.as_slice(),
        [
            BlockKind::Paragraph,
            BlockKind::PageBreak,
            BlockKind::Paragraph,
            BlockKind::PageBreak,
            BlockKind::Paragraph
        ]
    ));
    assert_eq!(report.document.visible_text(), "A\nB\nC\n");
    assert!(has_warning(&report, "docx-split-page-break"));
}

#[test]
fn interior_next_page_section_break_is_preserved_as_a_page_break() {
    let report = import_body(
        "interior-section-break",
        r#"<w:p><w:r><w:t>First section</w:t></w:r><w:pPr><w:sectPr><w:pgSz w:w="11906" w:h="16838"/></w:sectPr></w:pPr></w:p>
<w:p><w:r><w:t>Second section</w:t></w:r></w:p>
<w:p><w:r><w:t>Continuous first</w:t></w:r><w:pPr><w:sectPr><w:type w:val="continuous"/></w:sectPr></w:pPr></w:p>
<w:p><w:r><w:t>Continuous second</w:t></w:r></w:p>
<w:p><w:r><w:t>Column first</w:t></w:r><w:pPr><w:sectPr><w:type w:val="nextColumn"/></w:sectPr></w:pPr></w:p>
<w:p><w:r><w:t>Column second</w:t></w:r></w:p>"#,
        &[],
    );
    let kinds: Vec<&BlockKind> = report
        .document
        .blocks
        .iter()
        .map(|block| &block.kind)
        .collect();
    assert!(matches!(
        kinds.as_slice(),
        [
            BlockKind::Paragraph,
            BlockKind::PageBreak,
            BlockKind::Paragraph,
            BlockKind::Paragraph,
            BlockKind::Paragraph,
            BlockKind::Paragraph,
            BlockKind::Paragraph
        ]
    ));
    assert_eq!(
        report.document.visible_text(),
        "First section\nSecond section\nContinuous first\nContinuous second\nColumn first\nColumn second\n"
    );
    assert!(has_warning(&report, "docx-dropped-section-properties"));
    assert!(report.document.validate().is_ok());
}

/// The `w:tbl` LibreOffice 7.3 produced from a three-column HTML table with
/// one horizontal merge, one vertical merge, a shaded cell and per-cell
/// borders, copied verbatim out of `word/document.xml`.
///
/// It is the fixture for the whole table mapping because it is the one thing
/// a synthetic body cannot be: what a real producer actually writes.
const LIBREOFFICE_MERGED_TABLE: &str = r#"<w:tbl>
  <w:tblPr>
    <w:tblW w:w="3345" w:type="dxa"/>
    <w:tblLayout w:type="fixed"/>
    <w:tblCellMar><w:top w:w="60" w:type="dxa"/><w:left w:w="60" w:type="dxa"/></w:tblCellMar>
  </w:tblPr>
  <w:tblGrid>
    <w:gridCol w:w="1670"/>
    <w:gridCol w:w="455"/>
    <w:gridCol w:w="1220"/>
  </w:tblGrid>
  <w:tr>
    <w:tc>
      <w:tcPr>
        <w:tcW w:w="2125" w:type="dxa"/>
        <w:gridSpan w:val="2"/>
        <w:tcBorders>
          <w:top w:val="double" w:sz="2" w:space="0" w:color="808080"/>
          <w:left w:val="double" w:sz="2" w:space="0" w:color="808080"/>
          <w:bottom w:val="double" w:sz="2" w:space="0" w:color="808080"/>
          <w:right w:val="double" w:sz="2" w:space="0" w:color="808080"/>
        </w:tcBorders>
        <w:shd w:fill="FFCC00" w:val="clear"/>
        <w:vAlign w:val="center"/>
      </w:tcPr>
      <w:p><w:r><w:t>A1+B1 merged across</w:t></w:r></w:p>
    </w:tc>
    <w:tc>
      <w:tcPr><w:tcW w:w="1220" w:type="dxa"/><w:vAlign w:val="bottom"/></w:tcPr>
      <w:p><w:r><w:t>C1</w:t></w:r></w:p>
    </w:tc>
  </w:tr>
  <w:tr>
    <w:tc>
      <w:tcPr>
        <w:tcW w:w="1670" w:type="dxa"/>
        <w:vMerge w:val="restart"/>
        <w:shd w:fill="00CCFF" w:val="clear"/>
        <w:tcMar><w:top w:w="80" w:type="dxa"/><w:left w:w="120" w:type="dxa"/></w:tcMar>
      </w:tcPr>
      <w:p><w:r><w:t>A2 spans down</w:t></w:r></w:p>
    </w:tc>
    <w:tc><w:tcPr><w:tcW w:w="455" w:type="dxa"/></w:tcPr><w:p><w:r><w:t>B2</w:t></w:r></w:p></w:tc>
    <w:tc><w:tcPr><w:tcW w:w="1220" w:type="dxa"/></w:tcPr><w:p><w:r><w:t>C2</w:t></w:r></w:p></w:tc>
  </w:tr>
  <w:tr>
    <w:tc><w:tcPr><w:tcW w:w="1670" w:type="dxa"/><w:vMerge w:val="continue"/></w:tcPr><w:p/></w:tc>
    <w:tc><w:tcPr><w:tcW w:w="455" w:type="dxa"/></w:tcPr><w:p><w:r><w:t>B3</w:t></w:r></w:p></w:tc>
    <w:tc><w:tcPr><w:tcW w:w="1220" w:type="dxa"/></w:tcPr><w:p><w:r><w:t>C3</w:t></w:r></w:p></w:tc>
  </w:tr>
</w:tbl>"#;

fn block_text(block: &Block) -> String {
    block
        .content
        .iter()
        .map(|inline| match inline {
            Inline::Text { text, .. } | Inline::Link { text, .. } => text.as_str(),
            _ => "",
        })
        .collect()
}

/// `(rows, columns)` of a cell's span, in the order [`CellSpan`] states them.
fn spans(cell: &opendoc_core::TableCell) -> (u32, u32) {
    (cell.span.rows(), cell.span.columns())
}

fn cell_text(cell: &opendoc_core::TableCell) -> String {
    cell.blocks
        .iter()
        .flat_map(|block| block.content.iter())
        .map(|inline| match inline {
            Inline::Text { text, .. } => text.as_str(),
            _ => "",
        })
        .collect()
}

/// P1-4's first half. A `w:gridSpan` makes one `w:tc` occupy two grid
/// columns, so a reader that pushes one model cell per element puts every
/// later cell in the row one column too far left: "C1" belongs in column 2
/// and used to land in column 1. The covered position is materialised, so it
/// lands where the file puts it.
#[test]
fn a_grid_span_puts_the_next_cell_in_the_column_the_file_gives_it() {
    let report = import_body("gridspan-alignment", LIBREOFFICE_MERGED_TABLE, &[]);
    let BlockKind::Table { columns, rows, .. } = &report.document.blocks[0].kind else {
        panic!("expected a table");
    };
    assert_eq!(3, columns.len());
    for row in rows {
        assert_eq!(3, row.cells.len(), "the grid is not rectangular");
    }
    assert_eq!("A1+B1 merged across", cell_text(&rows[0].cells[0]));
    // The position the span covers: still in the grid, still addressable,
    // holding nothing of its own.
    assert_eq!("", cell_text(&rows[0].cells[1]));
    assert_eq!("C1", cell_text(&rows[0].cells[2]));
    assert_eq!(
        vec!["A2 spans down", "B2", "C2"],
        rows[1].cells.iter().map(cell_text).collect::<Vec<_>>()
    );
    assert_eq!(
        vec!["", "B3", "C3"],
        rows[2].cells.iter().map(cell_text).collect::<Vec<_>>()
    );
    report.document.validate().expect("a valid grid");
}

/// `w:gridSpan` is a column span and `w:vMerge` restart/continue is a row
/// span on the restart cell — ADR 0013's mapping, with the continuation cell
/// staying in the grid as the model's covered cell.
#[test]
fn grid_spans_and_vertical_merges_import_as_cell_spans() {
    let report = import_body("merges", LIBREOFFICE_MERGED_TABLE, &[]);
    let BlockKind::Table { rows, .. } = &report.document.blocks[0].kind else {
        panic!("expected a table");
    };
    assert_eq!(
        (1, 2),
        (
            rows[0].cells[0].span.rows(),
            rows[0].cells[0].span.columns()
        )
    );
    assert_eq!(
        (2, 1),
        (
            rows[1].cells[0].span.rows(),
            rows[1].cells[0].span.columns()
        )
    );
    // Everything a span covers stays single: the rectangle belongs to the
    // cell that starts it, and nothing else claims a position.
    for (row_index, column_index) in [(0, 1), (0, 2), (1, 1), (1, 2), (2, 0), (2, 1), (2, 2)] {
        assert!(
            rows[row_index].cells[column_index].span.is_single(),
            "cell ({row_index},{column_index}) claims a rectangle"
        );
    }
    // The positions the two rectangles hide, derived from the spans exactly
    // as every reader derives them: the cell to the right of the horizontal
    // merge, and the cell below the vertical one.
    assert_eq!(
        vec![(0usize, 1usize), (2, 0)],
        opendoc_core::table_covered_positions(rows)
            .into_iter()
            .collect::<Vec<_>>()
    );
}

/// `w:gridCol` is already in twips and so is the model, so the widths cross
/// exactly — they used to be dropped with no warning at all.
#[test]
fn grid_column_widths_import_exactly() {
    let report = import_body("grid-widths", LIBREOFFICE_MERGED_TABLE, &[]);
    let BlockKind::Table { columns, .. } = &report.document.blocks[0].kind else {
        panic!("expected a table");
    };
    assert_eq!(
        vec![Some(1670), Some(455), Some(1220)],
        columns
            .iter()
            .map(|column| column.width.map(|width| width.twips()))
            .collect::<Vec<_>>()
    );
    assert!(!has_warning(&report, "docx-clamped-table-column-width"));
}

/// A table that declares itself autofit has `w:gridCol` values that are the
/// producer's cached layout rather than widths anybody chose, and the model
/// has a word for that: `None`, auto.
#[test]
fn an_autofit_tables_grid_is_read_as_auto_rather_than_as_chosen_widths() {
    let report = import_body(
        "autofit",
        r#"<w:tbl>
  <w:tblPr><w:tblW w:w="0" w:type="auto"/><w:tblLayout w:type="autofit"/></w:tblPr>
  <w:tblGrid><w:gridCol w:w="4680"/><w:gridCol w:w="4680"/></w:tblGrid>
  <w:tr><w:tc><w:p><w:r><w:t>a</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>b</w:t></w:r></w:p></w:tc></w:tr>
</w:tbl>"#,
        &[],
    );
    let BlockKind::Table { columns, .. } = &report.document.blocks[0].kind else {
        panic!("expected a table");
    };
    assert!(columns.iter().all(|column| column.width.is_none()));
}

/// `w:tcPr` shading, borders, vertical alignment and margins all have typed
/// homes in `TableCellProperties` (ADR 0013) and used to reach none of them.
#[test]
fn cell_shading_borders_alignment_and_margins_import_into_typed_properties() {
    let report = import_body("cell-style", LIBREOFFICE_MERGED_TABLE, &[]);
    let BlockKind::Table { rows, .. } = &report.document.blocks[0].kind else {
        panic!("expected a table");
    };
    let merged = &rows[0].cells[0].properties;
    assert_eq!(
        Some("#ffcc00".to_string()),
        merged.background.map(|c| c.as_hex())
    );
    assert_eq!(
        Some(opendoc_core::VerticalAlignment::Middle),
        merged.vertical_alignment
    );
    let top = merged.border_top.expect("a top border");
    assert_eq!(opendoc_core::BorderStyle::Double, top.style());
    // `w:sz` counts eighths of a point, so 2 eighths is exactly 5 twips.
    assert_eq!(5, top.width().twips());
    assert_eq!("#808080", top.color().as_hex());
    assert_eq!(
        Some(opendoc_core::VerticalAlignment::Bottom),
        rows[0].cells[2].properties.vertical_alignment
    );
    let restart = &rows[1].cells[0].properties;
    assert_eq!(
        Some("#00ccff".to_string()),
        restart.background.map(|c| c.as_hex())
    );
    assert_eq!(Some(80), restart.padding_top.map(|p| p.twips()));
    assert_eq!(Some(120), restart.padding_start.map(|p| p.twips()));
}

/// A `w:tcPr` value with no model home is counted rather than ignored — the
/// warning that used to say "imported unmerged" now covers only what really
/// cannot cross.
#[test]
fn cell_properties_with_no_model_home_are_counted() {
    let report = import_body(
        "cell-dropped",
        r#"<w:tbl><w:tr><w:tc>
  <w:tcPr>
    <w:textDirection w:val="btLr"/>
    <w:noWrap/>
    <w:tcBorders><w:tl2br w:val="single" w:sz="4"/></w:tcBorders>
    <w:vAlign w:val="both"/>
  </w:tcPr>
  <w:p><w:r><w:t>sideways</w:t></w:r></w:p>
</w:tc></w:tr></w:tbl>"#,
        &[],
    );
    assert!(warning_message(&report, "docx-dropped-cell-property").contains("4 occurrences"));
    assert_eq!("sideways\n", report.document.visible_text());
}

/// A `w:vMerge="continue"` with nothing above it to continue is a file that
/// lost its restart. Reading it as a covered cell would hide its content, so
/// it becomes an ordinary cell and the repair is reported.
#[test]
fn a_vertical_merge_continuation_with_no_restart_keeps_its_content() {
    let report = import_body(
        "orphan-vmerge",
        r#"<w:tbl>
  <w:tr><w:tc><w:tcPr><w:vMerge w:val="continue"/></w:tcPr><w:p><w:r><w:t>orphan</w:t></w:r></w:p></w:tc></w:tr>
</w:tbl>"#,
        &[],
    );
    let BlockKind::Table { rows, .. } = &report.document.blocks[0].kind else {
        panic!("expected a table");
    };
    assert!(rows[0].cells[0].span.is_single());
    assert_eq!("orphan\n", report.document.visible_text());
    assert!(has_warning(&report, "docx-table-merge-repaired"));
}

/// The package LibreOffice 7.3 wrote, byte for byte: zip, `[Content_Types]`,
/// relationships, `word/styles.xml` and all.
///
/// Every other table test in this file builds its own package around a body
/// this repository typed out, so all of them share one idea of what a
/// producer emits. This fixture shares none of it. It was produced by
/// `soffice --headless --convert-to docx` from an HTML table with one
/// `colspan`, one `rowspan`, two shaded cells, two cell borders and cell
/// padding, and the assertions below are **that table** — the grid the source
/// described — not the grid the reader happened to build.
const LIBREOFFICE_PACKAGE: &[u8] = include_bytes!("../fixtures/libreoffice-73-merged-table.docx");

/// P1-4's first half, against bytes this repository did not write.
///
/// The `colspan="2"` reaches Word as `w:gridSpan`, so a reader that pushes one
/// model cell per `w:tc` puts "C1" in column 1 when the file puts it in column
/// 2 — silent corruption rather than a lost merge. The covered position is
/// materialised, so the row is three cells wide and every one of them is where
/// the file says.
#[test]
fn a_real_libreoffice_package_lands_every_cell_in_the_column_it_was_written_into() {
    let report =
        import_docx_bytes("libreoffice", LIBREOFFICE_PACKAGE).expect("the package is readable");
    // The paragraphs around the table prove the whole package parsed, not
    // just the fragment under test.
    assert_eq!(
        Some("Before the table"),
        report.document.blocks.first().map(block_text).as_deref()
    );
    assert_eq!(
        Some("After the table"),
        report.document.blocks.last().map(block_text).as_deref()
    );
    let BlockKind::Table { columns, rows, .. } = &report.document.blocks[1].kind else {
        panic!("expected a table, got {:?}", report.document.blocks[1].kind);
    };
    assert_eq!(3, columns.len());
    assert_eq!(
        vec![
            vec!["A1+B1 merged across", "", "C1"],
            vec!["A2 spans down", "B2", "C2"],
            vec!["", "B3", "C3"],
        ],
        rows.iter()
            .map(|row| row.cells.iter().map(cell_text).collect::<Vec<_>>())
            .collect::<Vec<_>>()
    );
    report.document.validate().expect("a rectangular grid");
}

/// The same real package's `w:gridCol`, `w:gridSpan`, `w:vMerge` and `w:tcPr`.
///
/// The widths are LibreOffice's own twips, so they are asserted as the
/// literals `unzip -p ... word/document.xml` shows and not as anything this
/// crate computed.
#[test]
fn a_real_libreoffice_packages_widths_merges_and_cell_styling_all_cross() {
    let report =
        import_docx_bytes("libreoffice", LIBREOFFICE_PACKAGE).expect("the package is readable");
    let BlockKind::Table { columns, rows, .. } = &report.document.blocks[1].kind else {
        panic!("expected a table");
    };
    assert_eq!(
        vec![Some(1819), Some(495), Some(500)],
        columns
            .iter()
            .map(|column| column.width.map(|width| width.twips()))
            .collect::<Vec<_>>()
    );
    assert!(!has_warning(&report, "docx-clamped-table-column-width"));
    // `colspan="2"` and `rowspan="2"`, each a rectangle on the cell that
    // starts it, and nothing else claiming a position (ADR 0013).
    assert_eq!((1, 2), spans(&rows[0].cells[0]));
    assert_eq!((2, 1), spans(&rows[1].cells[0]));
    assert_eq!(
        vec![(0usize, 1usize), (2, 0)],
        opendoc_core::table_covered_positions(rows)
            .into_iter()
            .collect::<Vec<_>>()
    );
    let merged = &rows[0].cells[0].properties;
    assert_eq!(
        Some("#ffcc00".to_string()),
        merged.background.map(|color| color.as_hex())
    );
    assert_eq!(
        Some(opendoc_core::VerticalAlignment::Middle),
        merged.vertical_alignment
    );
    // `w:sz` counts eighths of a point: 18 eighths is exactly 45 twips and 2
    // eighths exactly 5, so neither border needs approximating.
    let top = merged.border_top.expect("the red top border");
    assert_eq!(opendoc_core::BorderStyle::Solid, top.style());
    assert_eq!(45, top.width().twips());
    assert_eq!("#ff0000", top.color().as_hex());
    let bottom = merged.border_bottom.expect("the blue bottom border");
    assert_eq!(opendoc_core::BorderStyle::Dashed, bottom.style());
    assert_eq!(5, bottom.width().twips());
    assert_eq!("#0000ff", bottom.color().as_hex());
    assert!(!has_warning(&report, "docx-approximated-cell-border"));
    // `w:tblCellMar` is the padding of every cell that states no `w:tcMar` of
    // its own, and LibreOffice writes one on every table it produces. The
    // merged cell has no `w:tcMar`, so the table's 28/0 is its padding — not
    // *no* padding, which is what OpenDoc draws at its own 3pt/6pt default.
    assert_eq!(
        (Some(28), Some(0), Some(28), Some(0)),
        (
            merged.padding_top.map(|p| p.twips()),
            merged.padding_start.map(|p| p.twips()),
            merged.padding_bottom.map(|p| p.twips()),
            merged.padding_end.map(|p| p.twips()),
        )
    );
    let spanning = &rows[1].cells[0].properties;
    assert_eq!(
        Some("#00ccff".to_string()),
        spanning.background.map(|color| color.as_hex())
    );
    // And a cell that *does* state a `w:tcMar` keeps its own, rather than the
    // table's.
    assert_eq!(
        (Some(60), Some(60), Some(60), Some(60)),
        (
            spanning.padding_top.map(|p| p.twips()),
            spanning.padding_start.map(|p| p.twips()),
            spanning.padding_bottom.map(|p| p.twips()),
            spanning.padding_end.map(|p| p.twips()),
        )
    );
}

/// A cell can be both: `w:gridSpan="2"` *and* a `w:vMerge` continuation.
///
/// The rectangle belongs to the cell that starts it, so the continuation must
/// not claim one of its own — two cells claiming grid column 1 is a table
/// `Document::validate` refuses, and the repair that follows would wipe every
/// merge in the table rather than just this one.
#[test]
fn a_continuation_cell_that_also_spans_columns_claims_no_rectangle_of_its_own() {
    let report = import_body(
        "vmerge-with-gridspan",
        r#"<w:tbl>
  <w:tblGrid><w:gridCol w:w="1000"/><w:gridCol w:w="1000"/><w:gridCol w:w="1000"/></w:tblGrid>
  <w:tr>
    <w:tc><w:tcPr><w:gridSpan w:val="2"/><w:vMerge w:val="restart"/></w:tcPr><w:p><w:r><w:t>wide and tall</w:t></w:r></w:p></w:tc>
    <w:tc><w:p><w:r><w:t>C1</w:t></w:r></w:p></w:tc>
  </w:tr>
  <w:tr>
    <w:tc><w:tcPr><w:gridSpan w:val="2"/><w:vMerge w:val="continue"/></w:tcPr><w:p/></w:tc>
    <w:tc><w:p><w:r><w:t>C2</w:t></w:r></w:p></w:tc>
  </w:tr>
</w:tbl>"#,
        &[],
    );
    let BlockKind::Table { columns, rows, .. } = &report.document.blocks[0].kind else {
        panic!("expected a table");
    };
    assert_eq!(3, columns.len());
    assert_eq!((2, 2), spans(&rows[0].cells[0]));
    // The continuation carries the same `w:gridSpan`, and claims nothing.
    assert!(
        rows[1].cells[0].span.is_single(),
        "the continuation claimed a rectangle of its own: {:?}",
        rows[1].cells[0].span
    );
    assert_eq!(
        vec![(0usize, 1usize), (1, 0), (1, 1)],
        opendoc_core::table_covered_positions(rows)
            .into_iter()
            .collect::<Vec<_>>()
    );
    // The merges survived: the repair path that flattens every span in a
    // table it cannot validate did not run.
    assert!(!has_warning(&report, "docx-table-merge-repaired"));
    report.document.validate().expect("a valid grid");
    assert_eq!(
        vec![vec!["wide and tall", "", "C1"], vec!["", "", "C2"],],
        rows.iter()
            .map(|row| row.cells.iter().map(cell_text).collect::<Vec<_>>())
            .collect::<Vec<_>>()
    );
}

/// A `w:gridCol` outside what [`Length`] and [`opendoc_core::TableColumn`]
/// accept is clamped, and clamping is a change to the document, so it is
/// named rather than done quietly.
#[test]
fn a_grid_column_width_outside_the_models_range_is_clamped_and_named() {
    let report = import_body(
        "clamped-widths",
        r#"<w:tbl>
  <w:tblGrid><w:gridCol w:w="10"/><w:gridCol w:w="50000"/><w:gridCol w:w="1000"/></w:tblGrid>
  <w:tr><w:tc><w:p/></w:tc><w:tc><w:p/></w:tc><w:tc><w:p/></w:tc></w:tr>
</w:tbl>"#,
        &[],
    );
    let BlockKind::Table { columns, .. } = &report.document.blocks[0].kind else {
        panic!("expected a table");
    };
    assert_eq!(
        vec![Some(144), Some(31_680), Some(1000)],
        columns
            .iter()
            .map(|column| column.width.map(|width| width.twips()))
            .collect::<Vec<_>>()
    );
    assert!(
        warning_message(&report, "docx-clamped-table-column-width").contains("2 occurrences"),
        "{:?}",
        report.warnings
    );
    report
        .document
        .validate()
        .expect("the clamped widths are legal");
}

/// The repair for a lost restart has to be complete: an orphan is an
/// *ordinary* cell, so it keeps its `w:gridSpan` as well as its content.
///
/// Reading it as covered instead would leave the span on the floor, and the
/// position it should have reached would be filled with an empty cell — the
/// row would still be rectangular and still be wrong.
#[test]
fn an_orphaned_continuation_keeps_the_columns_it_spans_as_well_as_its_content() {
    let report = import_body(
        "orphan-vmerge-gridspan",
        r#"<w:tbl>
  <w:tblGrid><w:gridCol w:w="1000"/><w:gridCol w:w="1000"/><w:gridCol w:w="1000"/></w:tblGrid>
  <w:tr>
    <w:tc><w:tcPr><w:gridSpan w:val="2"/><w:vMerge w:val="continue"/></w:tcPr><w:p><w:r><w:t>orphan</w:t></w:r></w:p></w:tc>
    <w:tc><w:p><w:r><w:t>C1</w:t></w:r></w:p></w:tc>
  </w:tr>
</w:tbl>"#,
        &[],
    );
    let BlockKind::Table { rows, .. } = &report.document.blocks[0].kind else {
        panic!("expected a table");
    };
    assert_eq!(
        (1, 2),
        spans(&rows[0].cells[0]),
        "the orphan lost the columns it spans"
    );
    assert_eq!(
        vec!["orphan", "", "C1"],
        rows[0].cells.iter().map(cell_text).collect::<Vec<_>>()
    );
    assert!(has_warning(&report, "docx-table-merge-repaired"));
    report.document.validate().expect("a valid grid");
}

#[test]
fn nested_tables_import_as_table_blocks_inside_cells_with_warning() {
    let report = import_body(
        "nested-table",
        r#"<w:tbl>
  <w:tr>
    <w:tc><w:tcPr><w:gridSpan w:val="2"/></w:tcPr><w:p><w:r><w:t>Outer</w:t></w:r></w:p>
      <w:tbl><w:tr><w:tc><w:p><w:r><w:t>Inner</w:t></w:r></w:p></w:tc></w:tr></w:tbl>
      <w:p><w:r><w:t>After inner</w:t></w:r></w:p>
    </w:tc>
  </w:tr>
</w:tbl>"#,
        &[],
    );
    let BlockKind::Table { columns, rows, .. } = &report.document.blocks[0].kind else {
        panic!("expected table");
    };
    // The outer cell carries `w:gridSpan="2"`, so the grid is two columns
    // wide and the cell spans both of them.
    assert_eq!(2, columns.len());
    assert_eq!(2, rows[0].cells.len());
    let cell = &rows[0].cells[0];
    assert_eq!(2, cell.span.columns());
    assert_eq!(cell.blocks.len(), 3);
    let BlockKind::Table {
        rows: inner_rows, ..
    } = &cell.blocks[1].kind
    else {
        panic!("expected nested table, got {:?}", cell.blocks[1].kind);
    };
    assert_eq!(inner_rows.len(), 1);
    assert_eq!(
        report.document.visible_text(),
        "Outer\nInner\nAfter inner\n"
    );
    assert!(warning_message(&report, "docx-nested-table").contains("1 occurrence"));
    assert!(report.document.validate().is_ok());
}

#[test]
fn paragraph_properties_import_and_unrepresentable_ones_warn_once_per_kind() {
    let rels = document_rels(
        r#"<Relationship Id="rIdH" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/>
<Relationship Id="rIdF" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer" Target="footer1.xml"/>"#,
    );
    let header = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:t>Header</w:t></w:r></w:p></w:hdr>"#;
    let report = import_body(
        "dropped-props",
        r#"<w:p><w:pPr><w:jc w:val="center"/><w:ind w:left="720"/></w:pPr><w:r><w:t>Centered</w:t></w:r></w:p>
<w:p><w:pPr><w:jc w:val="right"/><w:spacing w:before="120"/><w:pBdr><w:top w:val="single"/></w:pBdr></w:pPr><w:r><w:t>Right</w:t></w:r></w:p>
<w:sectPr><w:headerReference w:type="default" r:id="rIdH"/><w:footerReference w:type="default" r:id="rIdF"/></w:sectPr>"#,
        &[
            ("word/_rels/document.xml.rels", rels.as_bytes()),
            ("word/header1.xml", header.as_bytes()),
            ("word/footer1.xml", header.as_bytes()),
        ],
    );
    assert_eq!(report.document.visible_text(), "Centered\nRight\n");
    // Alignment, indents and spacing are representable now, so they arrive as
    // values rather than as warnings.
    assert_eq!(
        report.document.blocks[0].properties.alignment,
        Some(Alignment::Center)
    );
    assert_eq!(
        report.document.blocks[0].properties.indent_start,
        Some(Length::from_twips(720).unwrap())
    );
    assert_eq!(
        report.document.blocks[1].properties.alignment,
        Some(Alignment::End)
    );
    assert_eq!(
        report.document.blocks[1].properties.space_before,
        Some(Length::from_twips(120).unwrap())
    );
    assert!(!has_warning(&report, "docx-dropped-alignment"));
    assert!(!has_warning(&report, "docx-dropped-indent"));
    assert!(!has_warning(&report, "docx-dropped-spacing"));
    // What OpenDoc still cannot hold keeps warning, once per kind with a count.
    assert!(has_warning(&report, "docx-dropped-paragraph-border"));
    // The header, the footer and the section's page geometry are read now
    // (ADR 0009's model, this reader's half of the round trip), so they are
    // no longer counted as dropped.
    for slot in [&report.document.header, &report.document.footer] {
        assert!(
            matches!(&slot[0].content[0], Inline::Text { text, .. } if text == "Header"),
            "{:?}",
            slot[0].content
        );
    }
    assert!(!has_warning(&report, "docx-dropped-header-footer"));
    assert!(!has_warning(&report, "docx-dropped-section-properties"));
    assert_eq!(
        report
            .warnings
            .iter()
            .filter(|warning| warning.code == "docx-dropped-paragraph-border")
            .count(),
        1
    );
    assert_eq!(report.document.warnings, report.warnings);
    assert!(report.document.validate().is_ok());
}

#[test]
fn header_images_use_the_header_relationship_scope_not_the_document_scope() {
    // Google Docs' DOCX export assigns `rId1` to the document theme and also
    // assigns `rId1` to the header logo.  IDs are scoped to each `.rels`
    // part, so resolving the header drawing through document.xml.rels turns a
    // real logo into a spurious missing-image placeholder.
    let rels = document_rels(
        r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="theme/theme1.xml"/>
<Relationship Id="rIdH" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/>"#,
    );
    let header_rels = document_rels(
        r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/logo.png"/>"#,
    );
    let header = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><w:hdr {NAMESPACES}><w:p><w:r><w:drawing><wp:inline><wp:extent cx="63500" cy="63500"/><a:graphic><a:graphicData><pic:pic><pic:blipFill><a:blip r:embed="rId1"/></pic:blipFill></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p></w:hdr>"#,
    );
    let report = import_body(
        "header-image-relationship-scope",
        r#"<w:p><w:r><w:t>Body</w:t></w:r></w:p><w:sectPr><w:headerReference w:type="default" r:id="rIdH"/></w:sectPr>"#,
        &[
            ("word/_rels/document.xml.rels", rels.as_bytes()),
            ("word/header1.xml", header.as_bytes()),
            ("word/_rels/header1.xml.rels", header_rels.as_bytes()),
            ("word/media/logo.png", b"header-logo-bytes"),
            ("word/theme/theme1.xml", b"<theme/>"),
        ],
    );
    assert!(matches!(
        report.document.header[0].kind,
        BlockKind::Image { .. }
    ));
    assert_eq!(report.blobs.len(), 1);
    assert!(!has_warning(&report, "missing-docx-image-blob"));
    assert!(report.document.validate().is_ok());
}

#[test]
fn uniform_docx_paragraph_border_imports_as_the_typed_frame() {
    let report = import_body(
        "paragraph-frame",
        r#"<w:p><w:pPr><w:pBdr><w:top w:val="dashed" w:sz="8" w:space="0" w:color="336699"/><w:left w:val="dashed" w:sz="8" w:space="0" w:color="336699"/><w:bottom w:val="dashed" w:sz="8" w:space="0" w:color="336699"/><w:right w:val="dashed" w:sz="8" w:space="0" w:color="336699"/></w:pBdr></w:pPr><w:r><w:t>Framed</w:t></w:r></w:p>"#,
        &[],
    );
    assert_eq!(
        report.document.blocks[0].properties.border,
        Some(
            CellBorder::new(
                BorderStyle::Dashed,
                Length::from_twips(20).unwrap(),
                Color::parse("#336699").unwrap(),
            )
            .unwrap()
        )
    );
    assert!(!has_warning(&report, "docx-dropped-paragraph-border"));
}

#[test]
fn hyperlink_fields_and_anchor_links_import_as_link_inlines() {
    let report = import_body(
        "field-links",
        r#"<w:p>
  <w:r><w:fldChar w:fldCharType="begin"/></w:r>
  <w:r><w:instrText xml:space="preserve"> HYPERLINK "https://example.invalid/field" </w:instrText></w:r>
  <w:r><w:fldChar w:fldCharType="separate"/></w:r>
  <w:r><w:t>field link</w:t></w:r>
  <w:r><w:fldChar w:fldCharType="end"/></w:r>
  <w:r><w:t> and </w:t></w:r>
  <w:fldSimple w:instr=" HYPERLINK \l &quot;Top&quot; "><w:r><w:t>simple</w:t></w:r></w:fldSimple>
  <w:hyperlink w:anchor="Section2"><w:r><w:t>anchor</w:t></w:r></w:hyperlink>
</w:p>"#,
        &[],
    );
    let block = &report.document.blocks[0];
    let links: Vec<(&str, &str)> = block
        .content
        .iter()
        .filter_map(|inline| match inline {
            Inline::Link { text, href, .. } => Some((text.as_str(), href.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(
        links,
        vec![
            ("field link", "https://example.invalid/field"),
            ("simple", "#Top"),
            ("anchor", "#Section2"),
        ]
    );
    assert_eq!(
        report.document.visible_text(),
        "field link and simpleanchor\n"
    );
}

#[test]
fn equations_import_from_zipped_packages_like_raw_xml() {
    let report = import_body(
        "math",
        r#"<w:p><w:r><w:t>Before </w:t></w:r><m:oMath><m:r><m:t>x+1</m:t></m:r></m:oMath></w:p>
<w:p><m:oMathPara><m:oMath><m:r><m:t>E=mc^2</m:t></m:r></m:oMath></m:oMathPara></w:p>"#,
        &[],
    );
    assert!(matches!(
        &report.document.blocks[0].content[1],
        Inline::Equation { equation, .. } if equation.source == "x+1"
    ));
    assert!(matches!(
        &report.document.blocks[1].kind,
        BlockKind::EquationBlock { equation } if equation.source == "E=mc^2"
    ));
}

#[test]
fn empty_docx_body_aborts_with_empty_input() {
    let err = import_package(
        "empty",
        &[
            ("[Content_Types].xml", CONTENT_TYPES.as_bytes()),
            ("_rels/.rels", ROOT_RELS.as_bytes()),
            (
                "word/document.xml",
                document_xml("<w:p/><w:p><w:r><w:t></w:t></w:r></w:p>").as_bytes(),
            ),
        ],
    )
    .unwrap_err();
    assert_eq!(err, ImportError::EmptyInput);
}

#[test]
fn packages_without_a_main_document_part_abort() {
    let err = import_package(
        "no-document",
        &[("[Content_Types].xml", CONTENT_TYPES.as_bytes())],
    )
    .unwrap_err();
    assert!(matches!(
        err,
        ImportError::InvalidInput(message) if message.contains("no main document part")
    ));
}

#[test]
fn non_docx_payloads_with_docx_extension_abort_without_external_converters() {
    let path = temp_docx_path("garbage");
    std::fs::write(&path, b"this is neither a zip nor xml").unwrap();
    let err = import_doc_or_docx(&path).unwrap_err();
    let _ = std::fs::remove_file(path);
    assert!(matches!(
        err,
        ImportError::InvalidInput(message)
            if message.contains("neither a ZIP package nor WordprocessingML XML")
    ));
}

#[test]
fn attribute_parsing_accepts_single_quotes_and_alternate_prefixes() {
    let document = r#"<?xml version='1.0' encoding='UTF-8'?>
<x:document xmlns:x='http://schemas.openxmlformats.org/wordprocessingml/2006/main'>
  <x:body>
    <x:p><x:pPr><x:pStyle x:val='Heading4'/></x:pPr><x:r><x:rPr><x:b x:val='1'/></x:rPr><x:t xml:space='preserve'>Prefixed </x:t></x:r></x:p>
  </x:body>
</x:document>"#;
    let report = import_package(
        "prefixes",
        &[
            ("[Content_Types].xml", CONTENT_TYPES.as_bytes()),
            ("_rels/.rels", ROOT_RELS.as_bytes()),
            ("word/document.xml", document.as_bytes()),
        ],
    )
    .unwrap();
    assert!(matches!(
        report.document.blocks[0].kind,
        BlockKind::Heading { level: 4 }
    ));
    assert_eq!(
        inline_marks(&report.document.blocks[0], "Prefixed "),
        vec![MarkKind::Bold]
    );
}

#[test]
fn docx_paragraph_properties_map_to_twips_without_rounding_drift() {
    let report = import_body(
        "para-props",
        r#"<w:p><w:pPr>
    <w:jc w:val="both"/>
    <w:ind w:start="1440" w:end="720" w:firstLine="360"/>
    <w:spacing w:before="240" w:after="120" w:line="360" w:lineRule="auto"/>
    <w:bidi/>
  </w:pPr><w:r><w:t>Justified</w:t></w:r></w:p>"#,
        &[],
    );
    let properties = &report.document.blocks[0].properties;
    assert_eq!(properties.alignment, Some(Alignment::Justify));
    // DOCX already speaks twips, so these are equalities, not approximations.
    assert_eq!(
        properties.indent_start,
        Some(Length::from_twips(1440).unwrap())
    );
    assert_eq!(
        properties.indent_end,
        Some(Length::from_twips(720).unwrap())
    );
    assert_eq!(
        properties.indent_first_line,
        Some(Length::from_twips(360).unwrap())
    );
    assert_eq!(
        properties.space_before,
        Some(Length::from_twips(240).unwrap())
    );
    assert_eq!(
        properties.space_after,
        Some(Length::from_twips(120).unwrap())
    );
    // w:line is 240ths of a line under the default "auto" rule.
    assert_eq!(
        properties.line_spacing,
        Some(LineSpacing::multiple(1.5).unwrap())
    );
    assert_eq!(properties.direction, Some(TextDirection::RightToLeft));
    assert!(!has_warning(&report, "docx-dropped-alignment"));
    assert!(!has_warning(&report, "docx-dropped-indent"));
    assert!(!has_warning(&report, "docx-dropped-spacing"));
    assert!(report.document.validate().is_ok());
}

#[test]
fn docx_hanging_indent_is_a_negative_first_line_indent() {
    let report = import_body(
        "hanging",
        r#"<w:p><w:pPr><w:ind w:left="720" w:hanging="360" w:firstLine="180"/></w:pPr><w:r><w:t>Hang</w:t></w:r></w:p>"#,
        &[],
    );
    let properties = &report.document.blocks[0].properties;
    assert_eq!(
        properties.indent_start,
        Some(Length::from_twips(720).unwrap())
    );
    // `w:hanging` wins over `w:firstLine` when both are present.
    assert_eq!(
        properties.indent_first_line,
        Some(Length::from_twips(-360).unwrap())
    );
}

#[test]
fn docx_exact_and_at_least_line_rules_keep_their_rule() {
    let report = import_body(
        "line-rules",
        r#"<w:p><w:pPr><w:spacing w:line="280" w:lineRule="exact"/></w:pPr><w:r><w:t>Exact</w:t></w:r></w:p>
<w:p><w:pPr><w:spacing w:line="280" w:lineRule="atLeast"/></w:pPr><w:r><w:t>AtLeast</w:t></w:r></w:p>"#,
        &[],
    );
    assert_eq!(
        report.document.blocks[0].properties.line_spacing,
        Some(LineSpacing::Exact(Length::from_twips(280).unwrap()))
    );
    assert_eq!(
        report.document.blocks[1].properties.line_spacing,
        Some(LineSpacing::AtLeast(Length::from_twips(280).unwrap()))
    );
}

#[test]
fn docx_paragraph_properties_inherit_through_the_style_chain_and_direct_values_win() {
    let styles = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:style w:type="paragraph" w:styleId="Base">
    <w:name w:val="Base"/>
    <w:pPr><w:jc w:val="center"/><w:spacing w:after="200"/></w:pPr>
  </w:style>
  <w:style w:type="paragraph" w:styleId="Quote">
    <w:name w:val="Quote"/>
    <w:basedOn w:val="Base"/>
    <w:pPr><w:ind w:left="720"/></w:pPr>
  </w:style>
</w:styles>"#;
    let report = import_body(
        "style-chain-props",
        r#"<w:p><w:pPr><w:pStyle w:val="Quote"/></w:pPr><w:r><w:t>Inherited</w:t></w:r></w:p>
<w:p><w:pPr><w:pStyle w:val="Quote"/><w:jc w:val="right"/></w:pPr><w:r><w:t>Overridden</w:t></w:r></w:p>"#,
        &[("word/styles.xml", styles.as_bytes())],
    );
    let inherited = &report.document.blocks[0].properties;
    assert_eq!(inherited.alignment, Some(Alignment::Center));
    assert_eq!(
        inherited.indent_start,
        Some(Length::from_twips(720).unwrap())
    );
    assert_eq!(
        inherited.space_after,
        Some(Length::from_twips(200).unwrap())
    );
    let overridden = &report.document.blocks[1].properties;
    assert_eq!(overridden.alignment, Some(Alignment::End));
    assert_eq!(
        overridden.indent_start,
        Some(Length::from_twips(720).unwrap())
    );
}

#[test]
fn docx_paragraph_properties_that_cannot_be_represented_still_warn() {
    let report = import_body(
        "unrepresentable-props",
        r#"<w:p><w:pPr>
    <w:jc w:val="highKashida"/>
    <w:ind w:leftChars="200"/>
    <w:spacing w:before="-120" w:beforeAutospacing="1"/>
  </w:pPr><w:r><w:t>Odd</w:t></w:r></w:p>
<w:p><w:pPr><w:jc w:val="distribute"/></w:pPr><w:r><w:t>Distributed</w:t></w:r></w:p>"#,
        &[],
    );
    assert_eq!(report.document.blocks[0].properties.alignment, None);
    assert_eq!(report.document.blocks[0].properties.indent_start, None);
    assert_eq!(report.document.blocks[0].properties.space_before, None);
    assert!(has_warning(&report, "docx-dropped-alignment"));
    assert!(has_warning(&report, "docx-dropped-indent"));
    assert!(has_warning(&report, "docx-dropped-spacing"));
    assert_eq!(
        report.document.blocks[1].properties.alignment,
        Some(Alignment::Justify)
    );
    assert!(warning_message(&report, "docx-approximated-alignment").contains("justified"));
}

#[test]
fn docx_paragraph_properties_survive_export_to_google_docs_json_and_back() {
    let report = import_body(
        "docx-to-google",
        r#"<w:p><w:pPr>
    <w:jc w:val="center"/>
    <w:ind w:start="720" w:end="360" w:hanging="360"/>
    <w:spacing w:before="240" w:after="120" w:line="480" w:lineRule="auto"/>
  </w:pPr><w:r><w:t>Round trip</w:t></w:r></w:p>"#,
        &[],
    );
    let expected = report.document.blocks[0].properties.clone();
    assert_eq!(expected.alignment, Some(Alignment::Center));
    assert_eq!(
        expected.indent_first_line,
        Some(Length::from_twips(-360).unwrap())
    );
    let (bytes, warnings) = crate::export_google_docs_json_with_warnings(&report.document).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let reimported = crate::import_google_docs_json("Round trip", &bytes).unwrap();
    assert_eq!(reimported.document.blocks[0].properties, expected);
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
}

#[test]
fn docx_paragraph_properties_reach_every_fragment_of_a_split_paragraph() {
    let report = import_body(
        "split-props",
        r#"<w:p><w:pPr><w:jc w:val="center"/></w:pPr>
  <w:r><w:t>Before</w:t></w:r>
  <w:r><w:br w:type="page"/></w:r>
  <w:r><w:t>After</w:t></w:r>
</w:p>"#,
        &[],
    );
    assert_eq!(report.document.blocks.len(), 3);
    assert_eq!(
        report.document.blocks[0].properties.alignment,
        Some(Alignment::Center)
    );
    assert!(report.document.blocks[1].properties.is_empty());
    assert_eq!(
        report.document.blocks[2].properties.alignment,
        Some(Alignment::Center)
    );
}

// ---------------------------------------------------------------------------
// Hostile packages
//
// Every case here reproduced against this reader before the caps in
// `crate::xml` and `crate::docx::package` existed. They are cheap because
// nothing is ever inflated or walked: a refusal costs milliseconds.
// ---------------------------------------------------------------------------

/// A 1.6 KB `.docx` used to abort the process outright.
///
/// `parse_xml` is iterative, so the tree is *built*; it is the walk over it
/// (`walk_blocks`'s catch-all arm, and the derived recursive `Drop` on
/// `XmlElement`) that overflowed the stack. `fatal runtime error: stack
/// overflow` is a `SIGABRT`: no `catch_unwind` sees it, and in the desktop
/// shell it takes the whole host process down with the document.
#[test]
fn deeply_nested_xml_is_refused_instead_of_overflowing_the_stack() {
    let depth = 40_000;
    let body = format!(
        "{}<w:p><w:r><w:t>x</w:t></w:r></w:p>{}",
        "<w:sdt>".repeat(depth),
        "</w:sdt>".repeat(depth)
    );
    let document = document_xml(&body);
    let err = import_package(
        "deep-nesting",
        &[
            ("[Content_Types].xml", CONTENT_TYPES.as_bytes()),
            ("_rels/.rels", ROOT_RELS.as_bytes()),
            ("word/document.xml", document.as_bytes()),
        ],
    )
    .unwrap_err();
    assert!(
        matches!(&err, ImportError::InvalidInput(message) if message.contains("nests deeper")),
        "{err:?}"
    );
}

/// Nesting the reader *does* understand is refused by the same cap.
#[test]
fn deeply_nested_tables_are_refused_by_the_same_depth_cap() {
    let depth = 2_000;
    let body = format!(
        "{}<w:p><w:r><w:t>x</w:t></w:r></w:p>{}",
        "<w:tbl><w:tr><w:tc>".repeat(depth),
        "</w:tc></w:tr></w:tbl>".repeat(depth)
    );
    let document = document_xml(&body);
    let err = import_package(
        "deep-tables",
        &[
            ("[Content_Types].xml", CONTENT_TYPES.as_bytes()),
            ("_rels/.rels", ROOT_RELS.as_bytes()),
            ("word/document.xml", document.as_bytes()),
        ],
    )
    .unwrap_err();
    assert!(
        matches!(&err, ImportError::InvalidInput(message) if message.contains("nests deeper")),
        "{err:?}"
    );
}

/// Below the XML depth cap, tables past the model's own nesting limit are
/// flattened with a warning rather than refused — the cell text is the content
/// and it is kept.
#[test]
fn tables_nested_past_the_model_limit_are_flattened_with_a_warning() {
    let depth = 40;
    let body = format!(
        "{}<w:p><w:r><w:t>deep cell</w:t></w:r></w:p>{}",
        "<w:tbl><w:tr><w:tc>".repeat(depth),
        "</w:tc></w:tr></w:tbl>".repeat(depth)
    );
    let report = import_body("flattened-tables", &body, &[]);
    assert!(
        has_warning(&report, "docx-table-nesting-limit"),
        "{:?}",
        report.warnings
    );
    // The cell text survived the flattening, somewhere in the tree.
    fn holds_text(blocks: &[Block], needle: &str) -> bool {
        blocks.iter().any(|block| {
            block
                .content
                .iter()
                .any(|inline| matches!(inline, Inline::Text { text, .. } if text == needle))
                || match &block.kind {
                    BlockKind::Table { rows, .. } => rows.iter().any(|row| {
                        row.cells
                            .iter()
                            .any(|cell| holds_text(&cell.blocks, needle))
                    }),
                    _ => false,
                }
        })
    }
    assert!(
        holds_text(&report.document.blocks, "deep cell"),
        "{:?}",
        report.document.blocks
    );
}

/// A 1 MB package whose `word/document.xml` declares 1 GiB used to be inflated
/// in full: 3.1 GB of resident memory from a file that fits in an email.
#[test]
fn a_part_that_declares_more_than_the_cap_is_refused_before_it_is_inflated() {
    let mut document = Vec::new();
    document.extend_from_slice(
        br#"<?xml version="1.0"?><w:document xmlns:w="urn:w"><w:body><w:p><w:r><w:t>"#,
    );
    document.resize(128 * 1024 * 1024, b'A');
    document.extend_from_slice(b"</w:t></w:r></w:p></w:body></w:document>");
    let packaged = build_docx(&[
        ("[Content_Types].xml", CONTENT_TYPES.as_bytes()),
        ("_rels/.rels", ROOT_RELS.as_bytes()),
        ("word/document.xml", &document),
    ]);
    // The attack's whole shape: a small file that says it is enormous.
    assert!(packaged.len() < 1024 * 1024, "{}", packaged.len());

    let path = temp_docx_path("zip-bomb");
    std::fs::write(&path, packaged).unwrap();
    let err = import_doc_or_docx(&path).unwrap_err();
    let _ = std::fs::remove_file(path);
    assert!(
        matches!(&err, ImportError::InvalidInput(message) if message.contains("over the")),
        "{err:?}"
    );
}

/// Many parts, each under the per-part cap, still cannot add up past the
/// budget the packaged size earns.
#[test]
fn parts_that_add_up_past_the_budget_are_refused() {
    let part = vec![b'A'; 8 * 1024 * 1024];
    let mut parts: Vec<(&str, &[u8])> = vec![
        ("[Content_Types].xml", CONTENT_TYPES.as_bytes()),
        ("_rels/.rels", ROOT_RELS.as_bytes()),
    ];
    let names: Vec<String> = (0..40)
        .map(|index| format!("word/media/f{index}.bin"))
        .collect();
    for name in &names {
        parts.push((name.as_str(), &part));
    }
    let packaged = build_docx(&parts);
    let path = temp_docx_path("zip-budget");
    std::fs::write(&path, packaged).unwrap();
    let err = import_doc_or_docx(&path).unwrap_err();
    let _ = std::fs::remove_file(path);
    assert!(
        matches!(&err, ImportError::InvalidInput(message) if message.contains("declares more than")),
        "{err:?}"
    );
}

// ---------------------------------------------------------------------------
// w:tblBorders
// ---------------------------------------------------------------------------

/// A border grid with a *different* value on all six edges, so no assertion
/// below can be satisfied by the wrong one. Widths are chosen to be exact:
/// `w:sz` counts eighths of a point and a twip is a twentieth, so `sz` times
/// 2.5 is the twips, and every one of these is a whole number.
const SIX_DISTINCT_EDGES: &str = r#"<w:tblBorders>
      <w:top w:val="single" w:sz="8" w:space="0" w:color="FF0000"/>
      <w:bottom w:val="double" w:sz="12" w:space="0" w:color="00FF00"/>
      <w:left w:val="dashed" w:sz="16" w:space="0" w:color="0000FF"/>
      <w:right w:val="dotted" w:sz="4" w:space="0" w:color="123456"/>
      <w:insideH w:val="single" w:sz="24" w:space="0" w:color="ABCDEF"/>
      <w:insideV w:val="dashed" w:sz="20" w:space="0" w:color="FEDCBA"/>
    </w:tblBorders>"#;

fn three_by_three(tbl_pr_extra: &str, cells: &[[&str; 3]; 3]) -> String {
    let rows: String = cells
        .iter()
        .map(|row| {
            let cells: String = row
                .iter()
                .map(|cell| format!("<w:tc><w:tcPr>{cell}</w:tcPr><w:p/></w:tc>"))
                .collect();
            format!("<w:tr>{cells}</w:tr>")
        })
        .collect();
    format!(
        r#"<w:tbl>
    <w:tblPr>{tbl_pr_extra}</w:tblPr>
    <w:tblGrid><w:gridCol w:w="1000"/><w:gridCol w:w="1000"/><w:gridCol w:w="1000"/></w:tblGrid>
    {rows}
  </w:tbl>"#
    )
}

fn table_of(report: &ImportReport) -> &[opendoc_core::TableRow] {
    match &report.document.blocks[0].kind {
        BlockKind::Table { rows, .. } => rows,
        other => panic!("expected a table, got {other:?}"),
    }
}

fn table_border_of(report: &ImportReport) -> Option<(opendoc_core::BorderStyle, i32, String)> {
    match &report.document.blocks[0].kind {
        BlockKind::Table { properties, .. } => properties.border.map(|border| {
            (
                border.style(),
                border.width().twips(),
                border.color().as_hex(),
            )
        }),
        other => panic!("expected a table, got {other:?}"),
    }
}

/// A border as `(style, twips, hex)`, or `None` for an edge nothing set.
fn edges(cell: &opendoc_core::TableCell) -> [Option<(opendoc_core::BorderStyle, i32, String)>; 4] {
    let describe = |border: Option<opendoc_core::CellBorder>| {
        border.map(|border| {
            (
                border.style(),
                border.width().twips(),
                border.color().as_hex(),
            )
        })
    };
    [
        describe(cell.properties.border_top),
        describe(cell.properties.border_bottom),
        describe(cell.properties.border_start),
        describe(cell.properties.border_end),
    ]
}

fn border(
    style: opendoc_core::BorderStyle,
    twips: i32,
    hex: &str,
) -> Option<(opendoc_core::BorderStyle, i32, String)> {
    Some((style, twips, hex.to_string()))
}

/// `w:tblBorders` used to be read by nobody, so a table whose borders live
/// there — which is *every* table Word styles, because `TableGrid` is a
/// `w:tblBorders` and nothing else — imported with no borders at all and said
/// nothing about it.
///
/// The model has no table-level border, so the grid is resolved onto the
/// cells: which of the six edges a cell inherits is decided by where it sits.
/// The six values here are all different, so an assertion cannot be satisfied
/// by the reader picking the wrong edge.
#[test]
fn a_tables_own_border_grid_reaches_every_cell_by_where_the_cell_sits() {
    use opendoc_core::BorderStyle::{Dashed, Dotted, Double, Solid};
    let body = three_by_three(SIX_DISTINCT_EDGES, &[[""; 3]; 3]);
    let report = import_body("tbl-borders", &body, &[]);
    let rows = table_of(&report);

    let top = border(Solid, 20, "#ff0000");
    let bottom = border(Double, 30, "#00ff00");
    let left = border(Dashed, 40, "#0000ff");
    let right = border(Dotted, 10, "#123456");
    let inside_h = border(Solid, 60, "#abcdef");
    let inside_v = border(Dashed, 50, "#fedcba");

    // Top-left: two outer edges, two interior ones.
    assert_eq!(
        [
            top.clone(),
            inside_h.clone(),
            left.clone(),
            inside_v.clone()
        ],
        edges(&rows[0].cells[0]),
        "the top-left cell"
    );
    // Dead centre: every edge is interior.
    assert_eq!(
        [
            inside_h.clone(),
            inside_h.clone(),
            inside_v.clone(),
            inside_v.clone()
        ],
        edges(&rows[1].cells[1]),
        "the middle cell"
    );
    // Bottom-right: the other two outer edges.
    assert_eq!(
        [inside_h.clone(), bottom.clone(), inside_v.clone(), right],
        edges(&rows[2].cells[2]),
        "the bottom-right cell"
    );
    // Top-middle: the table's top, the table's neither-left-nor-right.
    assert_eq!(
        [top, inside_h, inside_v.clone(), inside_v],
        edges(&rows[0].cells[1]),
        "the top-middle cell"
    );
    assert!(!has_warning(&report, "docx-dropped-table-border"));
}

/// A `w:tcBorders` edge overrides the one the cell would have inherited, and
/// leaves the other three standing — the same rule `w:tcMar` follows against
/// `w:tblCellMar`.
#[test]
fn a_cells_own_border_overrides_the_one_the_table_would_have_given_it() {
    use opendoc_core::BorderStyle::{Dashed, None as NoBorder, Solid};
    let mut cells = [[""; 3]; 3];
    cells[1][1] = r#"<w:tcBorders><w:top w:val="nil"/></w:tcBorders>"#;
    let body = three_by_three(SIX_DISTINCT_EDGES, &cells);
    let report = import_body("tbl-borders-override", &body, &[]);
    let rows = table_of(&report);

    assert_eq!(
        [
            border(NoBorder, 0, "#000000"),
            border(Solid, 60, "#abcdef"),
            border(Dashed, 50, "#fedcba"),
            border(Dashed, 50, "#fedcba"),
        ],
        edges(&rows[1].cells[1]),
        "the cell that turned its own top edge off"
    );
    // Its neighbour, which said nothing, still has the table's interior edge.
    assert_eq!(
        Some((Solid, 60, "#abcdef".to_string())),
        edges(&rows[1].cells[0])[0],
        "a cell that stated nothing lost the edge it inherits"
    );
}

/// Which edge a *merged* cell inherits is decided by where its rectangle
/// ends, not by where it starts: a cell spanning to the last column takes the
/// table's right edge, and one spanning down to the last row takes its
/// bottom. Getting this wrong draws an interior line across the outside of
/// the table, or an outer line through the middle of it.
///
/// Both directions need a span that *reaches* an outer edge and one that does
/// not, or the two halves of the condition cannot be told apart: a reader
/// that ignored the span entirely and asked only where the cell *starts*
/// would agree about a cell spanning columns 0 and 1, and disagree about one
/// spanning columns 1 and 2.
#[test]
fn a_merged_cell_inherits_the_edges_its_rectangle_reaches() {
    use opendoc_core::BorderStyle::{Dashed, Dotted, Double, Solid};
    let body = format!(
        r#"<w:tbl>
    <w:tblPr>{SIX_DISTINCT_EDGES}</w:tblPr>
    <w:tblGrid><w:gridCol w:w="1000"/><w:gridCol w:w="1000"/><w:gridCol w:w="1000"/></w:tblGrid>
    <w:tr>
      <w:tc><w:tcPr><w:gridSpan w:val="2"/></w:tcPr><w:p/></w:tc>
      <w:tc><w:tcPr/><w:p/></w:tc>
    </w:tr>
    <w:tr>
      <w:tc><w:tcPr/><w:p/></w:tc>
      <w:tc><w:tcPr><w:gridSpan w:val="2"/></w:tcPr><w:p/></w:tc>
    </w:tr>
    <w:tr>
      <w:tc><w:tcPr><w:vMerge w:val="restart"/></w:tcPr><w:p/></w:tc>
      <w:tc><w:tcPr/><w:p/></w:tc>
      <w:tc><w:tcPr/><w:p/></w:tc>
    </w:tr>
    <w:tr>
      <w:tc><w:tcPr><w:vMerge w:val="continue"/></w:tcPr><w:p/></w:tc>
      <w:tc><w:tcPr/><w:p/></w:tc>
      <w:tc><w:tcPr/><w:p/></w:tc>
    </w:tr>
  </w:tbl>"#
    );
    let report = import_body("tbl-borders-merged", &body, &[]);
    let rows = table_of(&report);

    // Columns 0 and 1 of row 0, so its end is still *interior*: the table's
    // right edge belongs to the cell in column 2.
    assert_eq!((1, 2), spans(&rows[0].cells[0]));
    assert_eq!(
        [
            border(Solid, 20, "#ff0000"),
            border(Solid, 60, "#abcdef"),
            border(Dashed, 40, "#0000ff"),
            border(Dashed, 50, "#fedcba"),
        ],
        edges(&rows[0].cells[0]),
        "the cell spanning columns 0 and 1"
    );
    assert_eq!(
        Some((Dotted, 10, "#123456".to_string())),
        edges(&rows[0].cells[2])[3],
        "the cell in the last column lost the table's right edge"
    );
    // Columns 1 and 2 of row 1: this one *does* reach the last column, and it
    // starts at a column that is neither the first nor the last.
    assert_eq!((1, 2), spans(&rows[1].cells[1]));
    assert_eq!(
        [
            border(Solid, 60, "#abcdef"),
            border(Solid, 60, "#abcdef"),
            border(Dashed, 50, "#fedcba"),
            border(Dotted, 10, "#123456"),
        ],
        edges(&rows[1].cells[1]),
        "the cell spanning columns 1 and 2 did not reach the table's right edge"
    );
    // Rows 2 and 3, so it reaches the bottom of the table.
    assert_eq!((2, 1), spans(&rows[2].cells[0]));
    assert_eq!(
        Some((Double, 30, "#00ff00".to_string())),
        edges(&rows[2].cells[0])[1],
        "the vertically merged cell did not reach the table's bottom edge"
    );
    // Its unmerged neighbour in the same row does *not*: row 2 is not the
    // last row.
    assert_eq!(
        Some((Solid, 60, "#abcdef".to_string())),
        edges(&rows[2].cells[1])[1],
        "an unmerged cell above the last row took the table's bottom edge"
    );
}

/// The diagonals are the one thing `w:tblBorders` can say that no per-cell
/// edge can hold, so they are named rather than dropped in silence (ADR 0010).
#[test]
fn a_table_border_diagonal_is_named_rather_than_dropped_in_silence() {
    let body = three_by_three(
        r#"<w:tblBorders>
      <w:top w:val="single" w:sz="8" w:space="0" w:color="FF0000"/>
      <w:tl2br w:val="single" w:sz="8" w:space="0" w:color="FF0000"/>
      <w:tr2bl w:val="single" w:sz="8" w:space="0" w:color="FF0000"/>
    </w:tblBorders>"#,
        &[[""; 3]; 3],
    );
    let report = import_body("tbl-borders-diagonal", &body, &[]);
    assert!(
        warning_message(&report, "docx-dropped-table-border").contains("2 occurrences"),
        "{:?}",
        report.warnings
    );
    // And the edge that *is* representable still crossed.
    assert_eq!(
        border(opendoc_core::BorderStyle::Solid, 20, "#ff0000"),
        edges(&table_of(&report)[0].cells[0])[0]
    );
}

/// Word's own table styles, trimmed to what this reader looks at: `Normal
/// Table` carries the 108-twip cell margins every Word table inherits, and
/// `Table Grid` is `basedOn` it and adds nothing but a border grid.
const TABLE_STYLES_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:style w:type="table" w:styleId="TableNormal">
    <w:name w:val="Normal Table"/>
    <w:tblPr>
      <!-- A top edge `TableGrid` overrides. It is here so the `w:basedOn`
           chain has to be applied base-first: applied the other way round,
           this green dotted line wins and the table is not `TableGrid` at
           all. -->
      <w:tblBorders>
        <w:top w:val="dotted" w:sz="2" w:space="0" w:color="00FF00"/>
      </w:tblBorders>
      <w:tblCellMar>
        <w:top w:w="0" w:type="dxa"/>
        <w:left w:w="108" w:type="dxa"/>
        <w:bottom w:w="0" w:type="dxa"/>
        <w:right w:w="108" w:type="dxa"/>
      </w:tblCellMar>
    </w:tblPr>
  </w:style>
  <w:style w:type="table" w:styleId="TableGrid">
    <w:name w:val="Table Grid"/>
    <w:basedOn w:val="TableNormal"/>
    <w:tblPr>
      <w:tblBorders>
        <w:top w:val="single" w:sz="4" w:space="0" w:color="auto"/>
        <w:left w:val="single" w:sz="4" w:space="0" w:color="auto"/>
        <w:bottom w:val="single" w:sz="4" w:space="0" w:color="auto"/>
        <w:right w:val="single" w:sz="4" w:space="0" w:color="auto"/>
        <w:insideH w:val="single" w:sz="4" w:space="0" w:color="auto"/>
        <w:insideV w:val="single" w:sz="4" w:space="0" w:color="auto"/>
      </w:tblBorders>
    </w:tblPr>
  </w:style>
  <w:style w:type="table" w:styleId="BandedRows">
    <w:name w:val="Banded Rows"/>
    <w:basedOn w:val="TableGrid"/>
    <w:tblStylePr w:type="firstRow">
      <w:tcPr><w:shd w:val="clear" w:color="auto" w:fill="4472C4"/></w:tcPr>
    </w:tblStylePr>
  </w:style>
</w:styles>"#;

/// The table a person gets by pressing *Insert Table* in Word states no
/// border anywhere in `word/document.xml`: it names `TableGrid`, and the
/// border grid is in `styles.xml`. Reading only the table's own `w:tblPr`
/// therefore imports the commonest bordered table in the world borderless.
///
/// The `w:basedOn` half matters too, in both directions: the 108-twip cell
/// margin every Word table has comes from `Normal Table` two links up the
/// chain, and `Normal Table`'s own top edge is *overridden* by `Table Grid`'s
/// — so a chain resolved in the wrong order gives this table a green dotted
/// top.
#[test]
fn a_word_table_takes_its_borders_from_the_style_it_names() {
    let body = three_by_three(r#"<w:tblStyle w:val="TableGrid"/>"#, &[[""; 3]; 3]);
    let report = import_body(
        "tbl-style-borders",
        &body,
        &[("word/styles.xml", TABLE_STYLES_XML.as_bytes())],
    );
    let rows = table_of(&report);
    // `w:sz="4"` is four eighths of a point: exactly 10 twips. `auto` is the
    // colour Word resolves against the text colour, which is black here.
    let hairline = border(opendoc_core::BorderStyle::Solid, 10, "#000000");
    assert_eq!(hairline, table_border_of(&report));
    for (row_index, row) in rows.iter().enumerate() {
        for (column_index, cell) in row.cells.iter().enumerate() {
            assert_eq!(
                [None, None, None, None],
                edges(cell),
                "cell ({row_index}, {column_index})"
            );
        }
    }
    // And the margins from the style two links up the `w:basedOn` chain.
    assert_eq!(
        (Some(0), Some(108), Some(0), Some(108)),
        (
            rows[0].cells[0].properties.padding_top.map(|p| p.twips()),
            rows[0].cells[0].properties.padding_start.map(|p| p.twips()),
            rows[0].cells[0]
                .properties
                .padding_bottom
                .map(|p| p.twips()),
            rows[0].cells[0].properties.padding_end.map(|p| p.twips()),
        )
    );
    assert!(!has_warning(&report, "docx-dropped-table-style-banding"));
}

/// Direct formatting beats the style, edge by edge: turning the top off on
/// the table leaves the style's other five standing.
#[test]
fn a_tables_own_grid_overrides_the_styles_edge_by_edge() {
    let body = three_by_three(
        r#"<w:tblStyle w:val="TableGrid"/><w:tblBorders><w:top w:val="nil"/></w:tblBorders>"#,
        &[[""; 3]; 3],
    );
    let report = import_body(
        "tbl-style-override",
        &body,
        &[("word/styles.xml", TABLE_STYLES_XML.as_bytes())],
    );
    let rows = table_of(&report);
    assert_eq!(
        [
            border(opendoc_core::BorderStyle::None, 0, "#000000"),
            border(opendoc_core::BorderStyle::Solid, 10, "#000000"),
            border(opendoc_core::BorderStyle::Solid, 10, "#000000"),
            border(opendoc_core::BorderStyle::Solid, 10, "#000000"),
        ],
        edges(&rows[0].cells[0]),
        "the table turned only its top edge off"
    );
    // A cell below the first row never had the table's top edge to lose.
    assert_eq!(
        Some((opendoc_core::BorderStyle::Solid, 10, "#000000".to_string())),
        edges(&rows[1].cells[0])[0]
    );
}

/// A table style's conditional formatting — a shaded header row, banded rows
/// — is not resolved, so a table that uses such a style says so (ADR 0010)
/// instead of quietly losing its header shading.
#[test]
fn a_table_styles_conditional_formatting_is_named_rather_than_dropped_in_silence() {
    let body = three_by_three(r#"<w:tblStyle w:val="BandedRows"/>"#, &[[""; 3]; 3]);
    let report = import_body(
        "tbl-style-banding",
        &body,
        &[("word/styles.xml", TABLE_STYLES_XML.as_bytes())],
    );
    assert!(
        warning_message(&report, "docx-dropped-table-style-banding").contains("1 occurrence"),
        "{:?}",
        report.warnings
    );
    // What the style *can* say still crossed: `BandedRows` is `basedOn`
    // `TableGrid`, so the plain border grid is there.
    assert_eq!(
        border(opendoc_core::BorderStyle::Solid, 10, "#000000"),
        table_border_of(&report)
    );
    // And a table using a style with no conditional formatting is silent, so
    // the warning above is about `w:tblStylePr` and not about table styles.
    let plain = three_by_three(r#"<w:tblStyle w:val="TableGrid"/>"#, &[[""; 3]; 3]);
    let plain = import_body(
        "tbl-style-banding-control",
        &plain,
        &[("word/styles.xml", TABLE_STYLES_XML.as_bytes())],
    );
    assert!(!has_warning(&plain, "docx-dropped-table-style-banding"));
}

/// The real LibreOffice package states no `w:tblBorders` and no `w:tblStyle`,
/// and its cells carry an *empty* `w:tcBorders`. WordprocessingML reads all
/// of that as no border, and so does this: the two cells that state an edge
/// have it and no other cell has any.
///
/// This is the half of the round trip that used to be undone by the writer's
/// invented grid, which turned this table's `fo:border="none"` into `0.5pt
/// solid #000000` on the way back through LibreOffice.
#[test]
fn a_real_libreoffice_package_that_states_no_borders_imports_with_none() {
    let report =
        import_docx_bytes("libreoffice", LIBREOFFICE_PACKAGE).expect("the package is readable");
    let BlockKind::Table { rows, .. } = &report.document.blocks[1].kind else {
        panic!("expected a table");
    };
    assert_eq!(
        [
            border(opendoc_core::BorderStyle::Solid, 45, "#ff0000"),
            border(opendoc_core::BorderStyle::Dashed, 5, "#0000ff"),
            None,
            None,
        ],
        edges(&rows[0].cells[0]),
        "the only cell in the package that states a border"
    );
    for (row_index, row) in rows.iter().enumerate() {
        for (column_index, cell) in row.cells.iter().enumerate() {
            if (row_index, column_index) == (0, 0) {
                continue;
            }
            assert_eq!(
                [None, None, None, None],
                edges(cell),
                "cell ({row_index}, {column_index}) was given a border the package never states"
            );
        }
    }
}
