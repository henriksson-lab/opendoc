use crate::test_support::block_plain_text;
use crate::*;
use opendoc_core::{
    Block, BlockKind, BlockProperties, Document, EquationSourceFormat, Inline, StableId,
};
use serde_json::{json, Value};

#[test]
fn imports_google_docs_opendoc_inline_equation_extension() {
    let input = json!({
        "body": { "content": [{
            "paragraph": { "elements": [
                { "textRun": { "content": "Equation " } },
                { "opendocEquation": {
                    "inlineId": " inline-eq-import ",
                    "equationId": " eq-import ",
                    "sourceFormat": "latex-like",
                    "source": "a^2 + b^2 = c^2"
                } }
            ] }
        }] }
    });
    let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.code == "opendoc-google-equation-extension"));
    let inline = &report.document.blocks[0].content[1];
    match inline {
        Inline::Equation { id, equation } => {
            assert_eq!(id.as_str(), "inline-eq-import");
            assert_eq!(equation.id.as_str(), "eq-import");
            assert_eq!(equation.source_format, EquationSourceFormat::LatexLike);
            assert_eq!(equation.source, "a^2 + b^2 = c^2");
        }
        other => panic!("expected inline equation, got {other:?}"),
    }
}

#[test]
fn opendoc_inline_equation_extension_with_empty_source_aborts_import() {
    let input = json!({
        "body": { "content": [{
            "paragraph": { "elements": [
                { "opendocEquation": {
                    "inlineId": "inline-eq-empty",
                    "equationId": "eq-empty",
                    "sourceFormat": "latex-like",
                    "source": " "
                } }
            ] }
        }] }
    });
    assert!(matches!(
        import_google_docs_json("Google", input.to_string().as_bytes()),
        Err(ImportError::UnsupportedStructure(message))
            if message == "OpenDoc inline equation missing source"
    ));
}

#[test]
fn imports_google_docs_opendoc_mention_extension() {
    let input = json!({
        "body": { "content": [{
            "paragraph": { "elements": [
                { "textRun": { "content": "Reviewed by " } },
                { "opendocMention": {
                    "inlineId": " mention-import ",
                    "label": "@Grace"
                } }
            ] }
        }] }
    });
    let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.code == "opendoc-google-mention-extension"));
    let inline = &report.document.blocks[0].content[1];
    match inline {
        Inline::Mention { id, label } => {
            assert_eq!(id.as_str(), "mention-import");
            assert_eq!(label, "@Grace");
        }
        other => panic!("expected mention, got {other:?}"),
    }
}

#[test]
fn imports_google_docs_page_break_paragraph_as_block() {
    let input = json!({
        "body": { "content": [{
            "paragraph": { "elements": [
                { "pageBreak": {} },
                { "textRun": { "content": "\n" } }
            ] }
        }] }
    });
    let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();
    assert_eq!(report.document.blocks.len(), 1);
    assert!(matches!(
        report.document.blocks[0].kind,
        BlockKind::PageBreak
    ));
}

#[test]
fn mixed_google_docs_page_break_paragraph_splits_around_the_break() {
    let input = json!({
        "body": { "content": [{
            "paragraph": {
                "paragraphStyle": { "alignment": "CENTER" },
                "elements": [
                    { "textRun": { "content": "Before" } },
                    { "pageBreak": {} },
                    { "textRun": { "content": "After" } }
                ]
            }
        }] }
    });
    let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();
    assert_eq!(report.document.blocks.len(), 3);
    assert!(matches!(
        report.document.blocks[1].kind,
        BlockKind::PageBreak
    ));
    assert_eq!(block_plain_text(&report.document.blocks[0]), "Before");
    assert_eq!(block_plain_text(&report.document.blocks[2]), "After");
    // Both halves keep the paragraph's formatting; the break itself has none.
    assert_eq!(
        report.document.blocks[0].properties.alignment,
        Some(opendoc_core::Alignment::Center)
    );
    assert_eq!(
        report.document.blocks[2].properties.alignment,
        Some(opendoc_core::Alignment::Center)
    );
    assert!(report.document.blocks[1].properties.is_empty());
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.code == "google-split-page-break"));
}

#[test]
fn exports_page_break_as_google_docs_page_break_paragraph() {
    let mut document = Document::new("Page Break Export");
    document.blocks.push(Block {
        id: StableId::new("block"),
        kind: BlockKind::PageBreak,
        content: Vec::new(),
        properties: BlockProperties::default(),
    });
    let bytes = export_google_docs_json(&document).unwrap();
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        value["body"]["content"][0]["paragraph"]["elements"][0]["pageBreak"],
        json!({})
    );
}

#[test]
fn imports_google_docs_opendoc_equation_block_extension() {
    let input = json!({
        "body": { "content": [{
            "opendocEquationBlock": {
                "blockId": " equation-block-import ",
                "equationId": " eq-block-import ",
                "sourceFormat": "latex-like",
                "source": "\\sum_i x_i"
            }
        }] }
    });
    let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();
    assert_eq!(report.document.blocks.len(), 1);
    let block = &report.document.blocks[0];
    assert_eq!(block.id.as_str(), "equation-block-import");
    match &block.kind {
        BlockKind::EquationBlock { equation } => {
            assert_eq!(equation.id.as_str(), "eq-block-import");
            assert_eq!(equation.source_format, EquationSourceFormat::LatexLike);
            assert_eq!(equation.source, "\\sum_i x_i");
        }
        other => panic!("expected equation block, got {other:?}"),
    }
}

#[test]
fn opendoc_equation_block_extension_with_empty_source_aborts_import() {
    let input = json!({
        "body": { "content": [{
            "opendocEquationBlock": {
                "blockId": "block-eq-empty",
                "equationId": "eq-empty",
                "sourceFormat": "latex-like",
                "source": ""
            }
        }] }
    });
    assert!(matches!(
        import_google_docs_json("Google", input.to_string().as_bytes()),
        Err(ImportError::UnsupportedStructure(message))
            if message == "OpenDoc block equation missing source"
    ));
}

#[test]
fn imports_google_docs_opendoc_image_extension() {
    let input = json!({
        "body": { "content": [{
            "opendocImage": {
                "blockId": " image-import ",
                "blobHash": " sha256:imported ",
                "altText": "Imported figure"
            }
        }] }
    });
    let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();
    assert_eq!(report.document.blocks.len(), 1);
    let block = &report.document.blocks[0];
    assert_eq!(block.id.as_str(), "image-import");
    match &block.kind {
        BlockKind::Image {
            blob_hash,
            alt_text,
            ..
        } => {
            assert_eq!(blob_hash, "sha256:imported");
            assert_eq!(alt_text, "Imported figure");
        }
        other => panic!("expected image block, got {other:?}"),
    }
}

#[test]
fn opendoc_image_extension_with_invalid_blob_hash_aborts_import() {
    let input = json!({
        "body": { "content": [{
            "opendocImage": {
                "blockId": "image-import",
                "blobHash": "not-a-hash",
                "altText": "Imported figure"
            }
        }] }
    });
    assert!(matches!(
        import_google_docs_json("Google", input.to_string().as_bytes()),
        Err(ImportError::UnsupportedStructure(message))
            if message.contains("OpenDoc image blobHash is invalid")
    ));
}

#[test]
fn inline_object_element_is_dropped_with_a_warning_instead_of_aborting() {
    let input = json!({
        "body": { "content": [{
            "paragraph": { "elements": [
                { "textRun": { "content": "Figure" } },
                { "inlineObjectElement": { "inlineObjectId": "img1" } }
            ] }
        }] }
    });
    let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();
    assert_eq!(report.document.visible_text(), "Figure\n");
    let warning = report
        .warnings
        .iter()
        .find(|warning| warning.code == "google-dropped-paragraph-element")
        .expect("inline object warning");
    assert!(warning.message.contains("inline object"), "{warning:?}");
}

#[test]
fn partial_fidelity_emits_warning() {
    let input = json!({
        "body": { "content": [{
            "paragraph": {
                "paragraphStyle": { "unsupportedStyle": true },
                "elements": [{ "textRun": {
                    "content": "Warn",
                    "textStyle": { "smallCaps": true }
                } }]
            }
        }] }
    });
    let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();
    assert_eq!(report.document.visible_text(), "Warn\n");
    let paragraph_warning = report
        .warnings
        .iter()
        .find(|warning| warning.code == "google-dropped-paragraph-style")
        .expect("paragraph style warning");
    assert!(
        paragraph_warning.message.contains("unsupportedStyle"),
        "{paragraph_warning:?}"
    );
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.code == "unsupported-google-text-style"));
}
