//! DOCX reader tests built from synthetic in-memory packages.

use crate::{import_doc_or_docx, ImportError, ImportReport};
use opendoc_core::{
    Anchor, Block, BlockKind, Inline, Mark, MarkKind, StableId, SuggestionKind, SuggestionState,
};
use std::io::{Cursor, Write};
use std::path::PathBuf;

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
    let path = std::env::temp_dir().join(format!(
        "opendoc-import-docx-pkg-{label}-{}.docx",
        std::process::id()
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
                ordered,
            } => (list_id.clone(), *level, *ordered),
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
    assert!(matches!(blocks[0].kind, BlockKind::Heading { level: 1 }));
    assert!(matches!(blocks[1].kind, BlockKind::Heading { level: 2 }));
    assert!(matches!(blocks[2].kind, BlockKind::Heading { level: 2 }));
    let custom = inline_marks(&blocks[2], "Custom heading");
    assert!(custom.contains(&MarkKind::Bold) && custom.contains(&MarkKind::Italic));
    assert_eq!(inline_marks(&blocks[2], "unbold"), vec![MarkKind::Italic]);
    assert!(matches!(
        blocks[3].kind,
        BlockKind::ListItem {
            ordered: true,
            level: 0,
            ..
        }
    ));
    assert_eq!(inline_marks(&blocks[4], "strong run"), vec![MarkKind::Bold]);
    assert!(warning_message(&report, "docx-title-style-as-heading").contains("2 occurrences"));
    assert!(report.document.validate().is_ok());
}

#[test]
fn imports_footnotes_and_endnotes_as_footnote_records_with_references() {
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
    assert!(has_warning(&report, "docx-endnotes-as-footnotes"));
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
        BlockKind::Image { blob_hash, alt_text }
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
    let BlockKind::Table { rows } = &report.document.blocks[0].kind else {
        panic!("expected table");
    };
    let cell = &rows[0].cells[0];
    assert_eq!(cell.blocks.len(), 3);
    let BlockKind::Table { rows: inner_rows } = &cell.blocks[1].kind else {
        panic!("expected nested table, got {:?}", cell.blocks[1].kind);
    };
    assert_eq!(inner_rows.len(), 1);
    assert_eq!(
        report.document.visible_text(),
        "Outer\nInner\nAfter inner\n"
    );
    assert!(warning_message(&report, "docx-nested-table").contains("1 occurrence"));
    assert!(has_warning(&report, "docx-dropped-cell-span"));
    assert!(report.document.validate().is_ok());
}

#[test]
fn dropped_paragraph_properties_and_headers_emit_one_warning_per_kind_with_counts() {
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
    assert!(warning_message(&report, "docx-dropped-alignment").contains("(2 occurrences)"));
    assert!(warning_message(&report, "docx-dropped-indent").contains("(1 occurrence)"));
    assert!(warning_message(&report, "docx-dropped-spacing").contains("(1 occurrence)"));
    assert!(has_warning(&report, "docx-dropped-paragraph-border"));
    assert!(warning_message(&report, "docx-dropped-header-footer").contains("(2 occurrences)"));
    assert!(has_warning(&report, "docx-dropped-section-properties"));
    assert_eq!(
        report
            .warnings
            .iter()
            .filter(|warning| warning.code == "docx-dropped-alignment")
            .count(),
        1
    );
    assert_eq!(report.document.warnings, report.warnings);
    assert!(report.document.validate().is_ok());
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
