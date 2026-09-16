use crate::test_support::block_plain_text;
use crate::*;
use opendoc_core::{
    Block, BlockKind, BlockProperties, Document, EquationSourceFormat, Inline, StableId,
};
use serde_json::{json, Value};

#[test]
fn native_google_bookmarks_project_to_the_containing_stable_block() {
    let input = json!({
        "body": { "content": [
            {
                "startIndex": 1,
                "endIndex": 7,
                "paragraph": { "elements": [{ "textRun": { "content": "First\n" } }] }
            },
            {
                "startIndex": 7,
                "endIndex": 14,
                "paragraph": { "elements": [{ "textRun": { "content": "Second\n" } }] }
            }
        ] },
        "bookmarks": {
            "kix.bookmark": {
                "bookmarkId": "kix.bookmark",
                "position": { "index": 7 },
                "endIndex": 7
            }
        }
    });
    let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();
    assert_eq!(report.document.bookmarks.len(), 1);
    assert_eq!(report.document.bookmarks[0].id.as_str(), "kix.bookmark");
    assert_eq!(report.document.bookmarks[0].name, "google-bookmark-1");
    assert_eq!(
        report.document.bookmarks[0].block_id,
        report.document.blocks[1].id
    );
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.code == "google-bookmark-projected-to-block"));
}

#[test]
fn native_google_bookmarks_with_character_offsets_or_ranges_are_not_moved_to_blocks() {
    let input = json!({
        "body": { "content": [
            {
                "startIndex": 1,
                "endIndex": 7,
                "paragraph": { "elements": [{ "textRun": { "content": "First\\n" } }] }
            },
            {
                "startIndex": 7,
                "endIndex": 14,
                "paragraph": { "elements": [{ "textRun": { "content": "Second\\n" } }] }
            }
        ] },
        "bookmarks": {
            "interior": {
                "bookmarkId": "interior",
                "position": { "index": 8 }
            },
            "range": {
                "bookmarkId": "range",
                "position": { "index": 7 },
                "endIndex": 12
            }
        }
    });
    let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();
    assert!(report.document.bookmarks.is_empty());
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.code == "google-bookmark-offset-unrepresentable"));
    assert!(report
        .warnings
        .iter()
        .any(|warning| { warning.code == "google-bookmark-character-range-unrepresentable" }));
}

#[test]
fn native_google_named_ranges_are_reported_without_discarding_document_content() {
    let input = json!({
        "body": { "content": [{
            "startIndex": 1,
            "endIndex": 7,
            "paragraph": { "elements": [{ "textRun": { "content": "First\n" } }] }
        }] },
        "namedRanges": {
            "named-range-id": {
                "namedRanges": [{
                    "name": "selection",
                    "ranges": [{ "startIndex": 1, "endIndex": 6 }]
                }]
            }
        }
    });
    let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();
    assert_eq!(report.document.blocks.len(), 1);
    assert_eq!(block_plain_text(&report.document.blocks[0]), "First");
    assert!(report.warnings.iter().any(|warning| {
        warning.code == "google-dropped-document-part" && warning.message.contains("named ranges")
    }));
    assert!(report.document.bookmarks.is_empty());
}

#[test]
fn opendoc_table_of_contents_extension_round_trips_its_scope() {
    let input = json!({
        "body": { "content": [{
            "opendocTableOfContents": { "blockId": "contents", "maxLevel": 4 }
        }] }
    });
    let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();
    assert!(matches!(
        report.document.blocks[0].kind,
        BlockKind::TableOfContents { max_level: 4 }
    ));
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.code == "opendoc-google-toc-extension"));
}

#[test]
fn opendoc_bibliography_extension_round_trips_as_a_generated_block() {
    let input = json!({
        "body": { "content": [{
            "opendocBibliography": { "blockId": "references" }
        }] }
    });
    let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();
    assert!(matches!(
        report.document.blocks[0].kind,
        BlockKind::Bibliography
    ));
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.code == "opendoc-google-bibliography-extension"));
}

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
                "altText": "Imported figure",
                "layout": {
                    "width": 1440,
                    "positioned": {
                        "anchor": "PageContent",
                        "horizontal_offset": -240,
                        "vertical_offset": 480,
                        "layer": "BehindText"
                    }
                }
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
            layout,
        } => {
            assert_eq!(blob_hash, "sha256:imported");
            assert_eq!(alt_text, "Imported figure");
            assert_eq!(layout.width.unwrap().twips(), 1_440);
            assert!(
                matches!(layout.positioned, Some(opendoc_core::PositionedImage {
                anchor: opendoc_core::PositionedImageAnchor::PageContent,
                horizontal_offset,
                vertical_offset,
                layer: opendoc_core::PositionedImageLayer::BehindText,
            }) if horizontal_offset.twips() == -240 && vertical_offset.twips() == 480)
            );
        }
        other => panic!("expected image block, got {other:?}"),
    }
}

#[test]
fn imports_native_google_inline_image_when_authorised_bytes_are_supplied() {
    let input = json!({
        "inlineObjects": {
            "kix.image-1": { "inlineObjectProperties": { "embeddedObject": {
                "title": "Diagram", "description": "A native Google image",
                "size": {
                    "width": { "magnitude": 144, "unit": "PT" },
                    "height": { "magnitude": 72, "unit": "PT" }
                },
                "imageProperties": { "cropProperties": {
                    "offsetTop": 0.124,
                    "offsetRight": 0.25,
                    "offsetBottom": 0.376,
                    "offsetLeft": 0.5
                } }
            } } }
        },
        "body": { "content": [{
            "paragraph": { "elements": [
                { "textRun": { "content": "Before image" } },
                { "inlineObjectElement": { "inlineObjectId": "kix.image-1" } },
                { "textRun": { "content": "After image" } }
            ] }
        }] }
    });
    let bytes = b"authorised-image-bytes".to_vec();
    let resources = std::collections::BTreeMap::from([(
        "kix.image-1".to_string(),
        ExportImage {
            media_type: "image/png".to_string(),
            bytes: bytes.clone(),
        },
    )]);
    let report = import_google_docs_json_with_image_resources(
        "Google",
        input.to_string().as_bytes(),
        &resources,
    )
    .unwrap();
    assert_eq!(
        report.document.visible_text(),
        "Before image\nDiagram\nA native Google image\nAfter image\n"
    );
    assert_eq!(report.blobs.len(), 1);
    assert_eq!(report.blobs[0].bytes, bytes);
    assert!(matches!(
        &report.document.blocks[1].kind,
        BlockKind::Image { alt_text, blob_hash, layout }
            if alt_text == "Diagram\nA native Google image" && blob_hash == &report.blobs[0].hash
                && layout.width.map(|length| length.twips()) == Some(2880)
                && layout.height.map(|length| length.twips()) == Some(1440)
                && layout.crop == Some(opendoc_core::ImageCrop {
                    top_percent: 12,
                    right_percent: 25,
                    bottom_percent: 38,
                    left_percent: 50,
                })
    ));
    assert!(
        matches!(&report.document.blocks[2].kind, BlockKind::Paragraph)
            && matches!(
                report.document.blocks[2].content.as_slice(),
                [Inline::Text { text, .. }] if text == "After image"
            ),
        "the text after a native inline image must remain after its block fallback: {:?}",
        report.document.blocks
    );
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.code == "google-inline-image-split"));
    assert!(!report.warnings.iter().any(|warning| warning
        .message
        .contains("inline objects (images and drawings) carry no content")));
}

#[test]
fn bookmark_in_a_paragraph_split_by_a_native_google_image_is_not_misattached() {
    let input = json!({
        "inlineObjects": {
            "kix.image-1": { "inlineObjectProperties": { "embeddedObject": {} } }
        },
        "body": { "content": [{
            "startIndex": 1,
            "endIndex": 14,
            "paragraph": { "elements": [
                { "textRun": { "content": "Before" } },
                { "inlineObjectElement": { "inlineObjectId": "kix.image-1" } },
                { "textRun": { "content": "After\n" } }
            ] }
        }] },
        "bookmarks": {
            "caption-anchor": {
                "bookmarkId": "caption-anchor",
                "position": { "index": 9 }
            }
        }
    });
    let resources = std::collections::BTreeMap::from([(
        "kix.image-1".to_string(),
        ExportImage {
            media_type: "image/png".to_string(),
            bytes: b"authorised-image-bytes".to_vec(),
        },
    )]);

    let report = import_google_docs_json_with_image_resources(
        "Google",
        input.to_string().as_bytes(),
        &resources,
    )
    .unwrap();
    assert!(report.document.bookmarks.is_empty());
    assert!(report.warnings.iter().any(|warning| {
        warning.code == "google-bookmark-inline-image-split-unrepresentable"
            && warning.message.contains("cannot be mapped safely")
    }));
    assert_eq!(report.document.visible_text(), "Before\nAfter\n");
}

#[test]
fn native_google_inline_image_without_accessibility_metadata_does_not_fabricate_alt_text() {
    let input = json!({
        "inlineObjects": {
            "kix.image-1": { "inlineObjectProperties": { "embeddedObject": {} } }
        },
        "body": { "content": [{
            "paragraph": { "elements": [
                { "inlineObjectElement": { "inlineObjectId": "kix.image-1" } }
            ] }
        }] }
    });
    let resources = std::collections::BTreeMap::from([(
        "kix.image-1".to_string(),
        ExportImage {
            media_type: "image/png".to_string(),
            bytes: b"authorised-image-bytes".to_vec(),
        },
    )]);

    let report = import_google_docs_json_with_image_resources(
        "Google",
        input.to_string().as_bytes(),
        &resources,
    )
    .unwrap();

    assert!(matches!(
        &report.document.blocks[0].kind,
        BlockKind::Image { alt_text, .. } if alt_text.is_empty()
    ));
    assert!(report.warnings.iter().any(|warning| {
        warning.code == "google-inline-image-accessibility-metadata-missing"
            && warning.message.contains("kix.image-1")
    }));
    assert!(report.document.validate().is_ok());
}

#[test]
fn native_google_inline_image_does_not_materialise_a_non_image_resource() {
    let input = json!({
        "inlineObjects": {
            "kix.image-1": { "inlineObjectProperties": { "embeddedObject": {
                "title": "Diagram"
            } } }
        },
        "body": { "content": [{
            "paragraph": { "elements": [
                { "inlineObjectElement": { "inlineObjectId": "kix.image-1" } }
            ] }
        }] }
    });
    let resources = std::collections::BTreeMap::from([(
        "kix.image-1".to_string(),
        ExportImage {
            media_type: "text/plain".to_string(),
            bytes: b"not an image".to_vec(),
        },
    )]);

    let report = import_google_docs_json_with_image_resources(
        "Google",
        input.to_string().as_bytes(),
        &resources,
    )
    .unwrap();

    assert!(report.blobs.is_empty());
    assert!(!report
        .document
        .blocks
        .iter()
        .any(|block| matches!(block.kind, BlockKind::Image { .. })));
    assert!(report.warnings.iter().any(|warning| {
        warning.code == "google-inline-image-resource-invalid"
            && warning.message.contains("kix.image-1")
    }));
    assert!(report.warnings.iter().any(|warning| {
        warning
            .message
            .contains("inline objects (images and drawings) carry no content")
    }));
}

#[test]
fn missing_google_image_resource_names_lost_accessibility_metadata() {
    let input = json!({
        "inlineObjects": {
            "kix.image-1": { "inlineObjectProperties": { "embeddedObject": {
                "title": "Diagram", "description": "Image-only accessible text"
            } } }
        },
        "body": { "content": [{
            "paragraph": { "elements": [
                { "inlineObjectElement": { "inlineObjectId": "kix.image-1" } }
            ] }
        }] }
    });
    let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();
    assert!(!report
        .document
        .blocks
        .iter()
        .any(|block| matches!(block.kind, BlockKind::Image { .. })));
    assert!(report.warnings.iter().any(|warning| {
        warning.code == "google-inline-image-accessibility-metadata-unrepresentable"
            && warning.message.contains("kix.image-1")
    }));
    assert_eq!(report.document.visible_text(), "\n");
}

#[test]
fn native_google_inline_image_requires_a_concrete_well_formed_image_media_type() {
    let input = json!({
        "inlineObjects": {
            "wildcard": { "inlineObjectProperties": { "embeddedObject": {} } },
            "malformed": { "inlineObjectProperties": { "embeddedObject": {} } }
        },
        "body": { "content": [{
            "paragraph": { "elements": [
                { "inlineObjectElement": { "inlineObjectId": "wildcard" } },
                { "inlineObjectElement": { "inlineObjectId": "malformed" } }
            ] }
        }] }
    });
    let resources = std::collections::BTreeMap::from([
        (
            "wildcard".to_string(),
            ExportImage {
                // An Accept range cannot identify raw bytes for a later
                // download/export, so it must not silently become an image.
                media_type: "image/*".to_string(),
                bytes: b"authorised-image-bytes".to_vec(),
            },
        ),
        (
            "malformed".to_string(),
            ExportImage {
                media_type: "image/png/not-a-media-type".to_string(),
                bytes: b"authorised-image-bytes".to_vec(),
            },
        ),
    ]);

    let report = import_google_docs_json_with_image_resources(
        "Google",
        input.to_string().as_bytes(),
        &resources,
    )
    .unwrap();

    assert!(report.blobs.is_empty());
    assert!(!report
        .document
        .blocks
        .iter()
        .any(|block| matches!(block.kind, BlockKind::Image { .. })));
    for id in ["wildcard", "malformed"] {
        assert!(report.warnings.iter().any(|warning| {
            warning.code == "google-inline-image-resource-invalid" && warning.message.contains(id)
        }));
    }
}

#[test]
fn native_google_image_crop_outside_source_is_warned_not_coerced() {
    let input = json!({
        "inlineObjects": {
            "kix.image-1": { "inlineObjectProperties": { "embeddedObject": {
                "imageProperties": { "cropProperties": { "offsetLeft": -0.2 } }
            } } }
        },
        "body": { "content": [{
            "paragraph": { "elements": [
                { "inlineObjectElement": { "inlineObjectId": "kix.image-1" } }
            ] }
        }] }
    });
    let resources = std::collections::BTreeMap::from([(
        "kix.image-1".to_string(),
        ExportImage {
            media_type: "image/png".to_string(),
            bytes: b"authorised-image-bytes".to_vec(),
        },
    )]);
    let report = import_google_docs_json_with_image_resources(
        "Google",
        input.to_string().as_bytes(),
        &resources,
    )
    .unwrap();
    let image = report
        .document
        .blocks
        .iter()
        .find_map(|block| match &block.kind {
            BlockKind::Image { layout, .. } => Some(layout),
            _ => None,
        })
        .expect("materialised image block");
    assert!(image.crop.is_none());
    assert!(report.warnings.iter().any(|warning| warning.code
        == "google-inline-image-crop-unrepresentable"
        && warning.message.contains("out-of-source crop")));
}

#[test]
fn malformed_google_image_crop_object_is_named_instead_of_becoming_no_crop() {
    let input = json!({
        "inlineObjects": { "kix.image-1": { "inlineObjectProperties": { "embeddedObject": {
            "imageProperties": { "cropProperties": "not an object" }
        } } } },
        "body": { "content": [{ "paragraph": { "elements": [
            { "inlineObjectElement": { "inlineObjectId": "kix.image-1" } }
        ] } }] }
    });
    let resources = std::collections::BTreeMap::from([(
        "kix.image-1".to_string(),
        ExportImage {
            media_type: "image/png".to_string(),
            bytes: b"authorised-image-bytes".to_vec(),
        },
    )]);
    let report = import_google_docs_json_with_image_resources(
        "Google",
        input.to_string().as_bytes(),
        &resources,
    )
    .unwrap();
    assert!(report.warnings.iter().any(|warning| warning.code
        == "google-inline-image-crop-unrepresentable"
        && warning.message.contains("malformed crop properties")));
}

#[test]
fn native_google_image_crop_that_rounds_away_the_last_source_pixel_is_warned() {
    let input = json!({
        "inlineObjects": {
            "kix.image-1": { "inlineObjectProperties": { "embeddedObject": {
                "imageProperties": { "cropProperties": {
                    // 0.8% of the source remains before conversion, but the
                    // model's whole-percent edges would sum to 100%.
                    "offsetTop": 0.496,
                    "offsetBottom": 0.496
                } }
            } } }
        },
        "body": { "content": [{
            "paragraph": { "elements": [
                { "inlineObjectElement": { "inlineObjectId": "kix.image-1" } }
            ] }
        }] }
    });
    let resources = std::collections::BTreeMap::from([(
        "kix.image-1".to_string(),
        ExportImage {
            media_type: "image/png".to_string(),
            bytes: b"authorised-image-bytes".to_vec(),
        },
    )]);

    let report = import_google_docs_json_with_image_resources(
        "Google",
        input.to_string().as_bytes(),
        &resources,
    )
    .unwrap();
    let image = report
        .document
        .blocks
        .iter()
        .find_map(|block| match &block.kind {
            BlockKind::Image { layout, .. } => Some(layout),
            _ => None,
        })
        .expect("authorised image remains importable when its crop is not");
    assert!(image.crop.is_none());
    assert!(report.warnings.iter().any(|warning| {
        warning.code == "google-inline-image-crop-unrepresentable"
            && warning.message.contains("after whole-percent conversion")
    }));
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
        .any(|warning| warning.code == "google-unrepresentable-small-caps"));
}
