use crate::test_support::{block_plain_text, text_marks};
use crate::*;
use opendoc_core::{BlockKind, EquationSourceFormat, Inline, MarkKind};

#[test]
fn imports_docx_xml_source_with_structure_and_marks_without_external_converter() {
    let path = std::env::temp_dir().join(format!(
        "opendoc-import-raw-docx-{}.docx",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    std::fs::write(
        &path,
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
<w:p>
  <w:pPr><w:pStyle w:val="Heading2"/></w:pPr>
  <w:r><w:rPr><w:b/></w:rPr><w:t>Structured heading</w:t></w:r>
</w:p>
  <w:p>
    <w:pPr><w:numPr><w:ilvl w:val="1"/><w:numId w:val="7"/></w:numPr></w:pPr>
    <w:r><w:rPr><w:i/><w:u w:val="single"/><w:color w:val="336699"/><w:sz w:val="28"/></w:rPr><w:t>Marked item</w:t></w:r>
    <w:r><w:rPr><w:vertAlign w:val="superscript"/></w:rPr><w:t>sup</w:t></w:r>
    <w:r><w:rPr><w:vertAlign w:val="subscript"/></w:rPr><w:t>sub</w:t></w:r>
    <w:r><w:tab/><w:t>after tab</w:t><w:br/><w:t>after break</w:t></w:r>
  <w:hyperlink w:anchor="LocalBookmark"><w:r><w:t>bookmark</w:t></w:r></w:hyperlink>
</w:p>
  </w:body>
</w:document>"#,
    )
    .unwrap();

    let report = import_doc_or_docx(&path).unwrap();
    assert_eq!(report.document.blocks.len(), 2);
    assert!(matches!(
        report.document.blocks[0].kind,
        BlockKind::Heading { level: 2 }
    ));
    let heading_marks = text_marks(&report.document.blocks[0]);
    assert!(heading_marks.iter().any(|mark| mark.kind == MarkKind::Bold));
    match &report.document.blocks[1].kind {
        BlockKind::ListItem { level, .. } => assert_eq!(*level, 1),
        other => panic!("expected DOCX list item, got {other:?}"),
    }
    let item_marks = text_marks(&report.document.blocks[1]);
    assert!(item_marks.iter().any(|mark| mark.kind == MarkKind::Italic));
    assert!(item_marks
        .iter()
        .any(|mark| mark.kind == MarkKind::Underline));
    assert!(item_marks
        .iter()
        .any(|mark| mark.kind == MarkKind::Color && mark.value.as_deref() == Some("#336699")));
    assert!(item_marks
        .iter()
        .any(|mark| mark.kind == MarkKind::Size && mark.value.as_deref() == Some("14")));
    assert!(report.document.blocks[1]
        .content
        .iter()
        .any(|inline| matches!(
            inline,
            Inline::Text { text, marks, .. }
                if text == "sup" && marks.iter().any(|mark| mark.kind == MarkKind::Superscript)
        )));
    assert!(report.document.blocks[1]
        .content
        .iter()
        .any(|inline| matches!(
            inline,
            Inline::Text { text, marks, .. }
                if text == "sub" && marks.iter().any(|mark| mark.kind == MarkKind::Subscript)
        )));
    assert_eq!(
        report.document.visible_text(),
        "Structured heading\nMarked itemsupsub\tafter tab\nafter breakbookmark\n"
    );
    assert!(report
        .document
        .blocks
        .iter()
        .any(|block| block.content.iter().any(|inline| matches!(
            inline,
            Inline::Link { text, href, .. }
                if text == "bookmark" && href == "#LocalBookmark"
        ))));
    assert!(report.document.validate().is_ok());

    let _ = std::fs::remove_file(path);
}

#[test]
fn imports_docx_office_math_as_equation_source_without_external_converter() {
    let path = std::env::temp_dir().join(format!(
        "opendoc-import-docx-math-{}.docx",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    std::fs::write(
        &path,
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math">
  <w:body>
<w:p>
  <w:r><w:t>Before </w:t></w:r>
  <m:oMath><m:r><m:t>x+1</m:t></m:r></m:oMath>
  <w:r><w:t> after</w:t></w:r>
</w:p>
<w:p>
  <m:oMathPara>
    <m:oMath><m:r><m:t>\sum_i x_i</m:t></m:r></m:oMath>
  </m:oMathPara>
</w:p>
  </w:body>
</w:document>"#,
    )
    .unwrap();

    let report = import_doc_or_docx(&path).unwrap();
    assert_eq!(report.document.blocks.len(), 2);
    let first = &report.document.blocks[0].content;
    assert!(matches!(
        (&first[0], &first[1], &first[2]),
        (
            Inline::Text { text: before, .. },
            Inline::Equation { equation, .. },
            Inline::Text { text: after, .. },
        ) if before == "Before "
            && equation.source == "x+1"
            && equation.source_format == EquationSourceFormat::LatexLike
            && after == " after"
    ));
    match &report.document.blocks[1].kind {
        BlockKind::EquationBlock { equation } => {
            assert_eq!(equation.source, "\\sum_i x_i");
            assert_eq!(equation.source_format, EquationSourceFormat::LatexLike);
        }
        other => panic!("expected DOCX Office Math equation block, got {other:?}"),
    }
    assert!(report.document.blocks[1].content.is_empty());
    assert!(report.document.validate().is_ok());

    let _ = std::fs::remove_file(path);
}

#[test]
fn imports_raw_docx_xml_equation_only_source_without_text_runs() {
    let path = std::env::temp_dir().join(format!(
        "opendoc-import-docx-math-only-{}.docx",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    std::fs::write(
        &path,
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math">
  <w:body>
<w:p>
  <m:oMathPara>
    <m:oMath><m:r><m:t>E=mc^2</m:t></m:r></m:oMath>
  </m:oMathPara>
</w:p>
  </w:body>
</w:document>"#,
    )
    .unwrap();

    let report = import_doc_or_docx(&path).unwrap();
    assert_eq!(report.document.blocks.len(), 1);
    match &report.document.blocks[0].kind {
        BlockKind::EquationBlock { equation } => {
            assert_eq!(equation.source, "E=mc^2");
            assert_eq!(equation.source_format, EquationSourceFormat::LatexLike);
        }
        other => {
            panic!("expected equation-only DOCX to import as equation block, got {other:?}")
        }
    }
    assert!(report.document.blocks[0].content.is_empty());
    assert!(report.document.validate().is_ok());

    let _ = std::fs::remove_file(path);
}

#[test]
fn imports_raw_docx_xml_page_break_only_source_without_text_runs() {
    let path = std::env::temp_dir().join(format!(
        "opendoc-import-docx-page-break-only-{}.docx",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    std::fs::write(
        &path,
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
<w:p><w:r><w:br w:type="page"/></w:r></w:p>
  </w:body>
</w:document>"#,
    )
    .unwrap();

    let report = import_doc_or_docx(&path).unwrap();
    assert_eq!(report.document.blocks.len(), 1);
    assert!(matches!(
        report.document.blocks[0].kind,
        BlockKind::PageBreak
    ));
    assert!(report.document.blocks[0].content.is_empty());
    assert_eq!(report.document.visible_text(), "\n");
    assert!(report.document.validate().is_ok());

    let _ = std::fs::remove_file(path);
}

#[test]
fn imports_docx_tables_as_source_rows_cells_and_nested_blocks() {
    let path = std::env::temp_dir().join(format!(
        "opendoc-import-docx-table-{}.docx",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    std::fs::write(
        &path,
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math">
  <w:body>
<w:p><w:r><w:t>Before table</w:t></w:r></w:p>
<w:tbl>
  <w:tr>
    <w:tc><w:p><w:r><w:t>A1</w:t></w:r></w:p></w:tc>
    <w:tc><w:p><w:r><w:rPr><w:b/></w:rPr><w:t>B1</w:t></w:r></w:p></w:tc>
  </w:tr>
  <w:tr>
    <w:tc><w:p><w:r><w:t>A2 </w:t></w:r><m:oMath><m:r><m:t>x+2</m:t></m:r></m:oMath></w:p></w:tc>
    <w:tc></w:tc>
  </w:tr>
</w:tbl>
<w:p><w:r><w:t>After table</w:t></w:r></w:p>
  </w:body>
</w:document>"#,
    )
    .unwrap();

    let report = import_doc_or_docx(&path).unwrap();
    assert_eq!(report.document.blocks.len(), 3);
    assert_eq!(
        report.document.visible_text(),
        "Before table\nA1\tB1\nA2 x+2\t\nAfter table\n"
    );
    let BlockKind::Table { rows, .. } = &report.document.blocks[1].kind else {
        panic!("expected DOCX table block");
    };
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].cells.len(), 2);
    assert_eq!(rows[1].cells.len(), 2);
    assert_eq!(block_plain_text(&rows[0].cells[0].blocks[0]), "A1");
    assert!(text_marks(&rows[0].cells[1].blocks[0])
        .iter()
        .any(|mark| mark.kind == MarkKind::Bold));
    assert!(rows[1].cells[0].blocks[0].content.iter().any(|inline| {
        matches!(
            inline,
            Inline::Equation { equation, .. } if equation.source == "x+2"
        )
    }));
    assert_eq!(block_plain_text(&rows[1].cells[1].blocks[0]), "");
    assert!(report.document.validate().is_ok());

    let _ = std::fs::remove_file(path);
}

#[test]
fn imports_raw_docx_xml_table_only_source_without_text_runs() {
    let path = std::env::temp_dir().join(format!(
        "opendoc-import-docx-empty-table-only-{}.docx",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    std::fs::write(
        &path,
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
<w:tbl><w:tr><w:tc></w:tc></w:tr></w:tbl>
  </w:body>
</w:document>"#,
    )
    .unwrap();

    let report = import_doc_or_docx(&path).unwrap();
    assert_eq!(report.document.blocks.len(), 1);
    let BlockKind::Table { rows, .. } = &report.document.blocks[0].kind else {
        panic!("expected raw DOCX XML table-only fixture to import as table block");
    };
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].cells.len(), 1);
    assert_eq!(rows[0].cells[0].blocks.len(), 1);
    assert_eq!(block_plain_text(&rows[0].cells[0].blocks[0]), "");
    assert_eq!(report.document.visible_text(), "\n");
    assert!(report.document.validate().is_ok());

    let _ = std::fs::remove_file(path);
}

#[test]
fn imports_raw_docx_xml_drawing_only_as_missing_image_placeholder() {
    let path = std::env::temp_dir().join(format!(
        "opendoc-import-docx-raw-missing-image-{}.docx",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    std::fs::write(
        &path,
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
  xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"
  xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"
  xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"
  xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture">
  <w:body>
<w:p><w:r><w:drawing><wp:inline><wp:docPr id="1" name="Raw Image" descr="Raw missing figure"/><a:graphic><a:graphicData><pic:pic><pic:blipFill><a:blip r:embed="rIdMissing"/></pic:blipFill></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>
  </w:body>
</w:document>"#,
    )
    .unwrap();

    let report = import_doc_or_docx(&path).unwrap();
    assert!(report.blobs.is_empty());
    assert_eq!(report.warnings.len(), 1);
    assert_eq!(report.warnings[0].code, "missing-docx-image-blob");
    assert_eq!(report.document.warnings, report.warnings);
    assert_eq!(report.document.blocks.len(), 1);
    assert_eq!(
        report.document.visible_text(),
        "[missing DOCX image: rIdMissing]\n"
    );
    assert!(matches!(
        report.document.blocks[0].kind,
        BlockKind::Paragraph
    ));
    assert!(report.document.validate().is_ok());

    let _ = std::fs::remove_file(path);
}

#[test]
fn imports_docx_standalone_page_break_as_page_break_block() {
    let path = std::env::temp_dir().join(format!(
        "opendoc-import-docx-page-break-{}.docx",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    std::fs::write(
        &path,
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
<w:p><w:r><w:t>Before page</w:t></w:r></w:p>
<w:p><w:r><w:br w:type="page"/></w:r></w:p>
<w:p><w:r><w:t>Line</w:t><w:br/><w:t>break</w:t></w:r></w:p>
<w:p><w:r><w:t>After page</w:t></w:r></w:p>
  </w:body>
</w:document>"#,
    )
    .unwrap();

    let report = import_doc_or_docx(&path).unwrap();
    assert_eq!(report.document.blocks.len(), 4);
    assert!(matches!(
        report.document.blocks[1].kind,
        BlockKind::PageBreak
    ));
    assert_eq!(
        report.document.visible_text(),
        "Before page\nLine\nbreak\nAfter page\n"
    );
    assert_eq!(block_plain_text(&report.document.blocks[2]), "Line\nbreak");
    assert!(report.document.validate().is_ok());

    let _ = std::fs::remove_file(path);
}
