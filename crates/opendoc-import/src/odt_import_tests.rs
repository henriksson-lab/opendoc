//! ODT reader bookmark projections.

use crate::test_support::block_plain_text;
use crate::{export_odt, import_odt_bytes, ExportImage};
use opendoc_core::{Block, Bookmark, Document, StableId};
use std::collections::BTreeMap;

const PREFIX: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0"><office:body><office:text>"#;
const SUFFIX: &str = r#"</office:text></office:body></office:document-content>"#;

fn content(body: &str) -> Vec<u8> {
    format!("{PREFIX}{body}{SUFFIX}").into_bytes()
}

#[test]
fn imports_only_whole_odt_paragraph_bookmarks_as_stable_block_targets() {
    let report = import_odt_bytes(
        "Bookmarks",
        &content(
            r#"<text:p><text:bookmark-start text:name="Intro"/>whole paragraph<text:bookmark-end text:name="Intro"/></text:p>
<text:p>before <text:bookmark-start text:name="CharacterRange"/>selection<text:bookmark-end text:name="CharacterRange"/> after</text:p>
<text:p><text:bookmark text:name="Point"/>point bookmark</text:p>
<text:p><text:bookmark-start text:name="CrossBlock"/>starts here</text:p><text:p>ends there<text:bookmark-end text:name="CrossBlock"/></text:p>"#,
        ),
    )
    .unwrap();
    assert_eq!(report.document.bookmarks.len(), 1);
    let bookmark = &report.document.bookmarks[0];
    assert_eq!(bookmark.name, "Intro");
    assert_eq!(bookmark.block_id, report.document.blocks[0].id);
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.code == "odt-bookmark-range-unrepresentable"));
    assert!(!report.document.bookmarks.iter().any(|bookmark| {
        bookmark.name == "CharacterRange"
            || bookmark.name == "Point"
            || bookmark.name == "CrossBlock"
    }));
}

#[test]
fn native_odt_table_of_contents_is_named_when_the_bounded_reader_skips_it() {
    let report = import_odt_bytes(
        "TOC",
        &content(
            r#"<text:p>Before</text:p><text:table-of-content text:name="Contents"><text:table-of-content-source text:outline-level="3"/><text:index-body><text:p>Cached entry</text:p></text:index-body></text:table-of-content><text:p>After</text:p>"#,
        ),
    )
    .unwrap();
    assert_eq!(report.document.visible_text(), "Before\nAfter\n");
    assert!(report.warnings.iter().any(|warning| {
        warning.code == "odt-table-of-contents-unrepresentable"
            && warning.message.contains("1 native ODT table-of-content")
    }));
}

#[test]
fn bounded_reader_names_dropped_odt_body_objects_instead_of_silently_flattening_them() {
    let report = import_odt_bytes(
        "Objects",
        &content(
            r#"<text:p>Before <draw:frame xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0"><draw:image/></draw:frame> after</text:p><table:table xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0"><table:table-row><table:table-cell><text:p>cell</text:p></table:table-cell></table:table-row></table:table><text:list xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0"><text:list-item><text:p>item</text:p></text:list-item></text:list>"#,
        ),
    )
    .unwrap();

    assert_eq!(report.document.visible_text(), "Before  after\n");
    let warning = report
        .warnings
        .iter()
        .find(|warning| warning.code == "odt-body-content-unrepresentable")
        .expect("dropped ODT objects must be disclosed");
    assert!(
        warning.message.starts_with("3 native ODT body"),
        "{warning:?}"
    );
    let image_warning = report
        .warnings
        .iter()
        .find(|warning| warning.code == "odt-image-unrepresentable")
        .expect("dropped ODT image must be disclosed independently of its frame");
    assert!(
        image_warning
            .message
            .starts_with("1 native ODT draw:image element"),
        "{image_warning:?}"
    );
}

#[test]
fn bounded_reader_counts_each_odt_image_once_across_inline_and_skipped_containers() {
    let report = import_odt_bytes(
        "Images",
        &content(
            r#"<text:p>Before <draw:frame xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0"><draw:image/><draw:image/></draw:frame> after</text:p><table:table xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0"><table:table-row><table:table-cell><draw:frame xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0"><draw:image/></draw:frame></table:table-cell></table:table-row></table:table>"#,
        ),
    )
    .unwrap();
    assert_eq!(report.document.visible_text(), "Before  after\n");
    let warning = report
        .warnings
        .iter()
        .find(|warning| warning.code == "odt-image-unrepresentable")
        .expect("all skipped ODT images must be named");
    assert!(warning
        .message
        .starts_with("3 native ODT draw:image element"));
}

#[test]
fn rejects_nested_invalid_and_duplicate_odt_bookmark_names_without_guessing() {
    let report = import_odt_bytes(
        "Bookmarks",
        &content(
            r#"<text:p><text:bookmark-start text:name="Nested"/><text:span>text<text:bookmark-end text:name="Nested"/></text:span></text:p>
<text:p><text:bookmark-start text:name="bad name"/>bad<text:bookmark-end text:name="bad name"/></text:p>
<text:p><text:bookmark-start text:name="Same"/>first<text:bookmark-end text:name="Same"/></text:p>
<text:p><text:bookmark-start text:name="Same"/>second<text:bookmark-end text:name="Same"/></text:p>"#,
        ),
    )
    .unwrap();
    assert_eq!(
        report
            .document
            .bookmarks
            .iter()
            .map(|bookmark| bookmark.name.as_str())
            .collect::<Vec<_>>(),
        vec!["Same"]
    );
    for code in [
        "odt-bookmark-range-unrepresentable",
        "odt-bookmark-name-unrepresentable",
        "odt-bookmark-duplicate-name",
    ] {
        assert!(report.warnings.iter().any(|warning| warning.code == code));
    }
}

#[test]
fn own_odt_zero_width_bookmark_shape_round_trips_to_a_stable_block() {
    let mut source = Document::new("Source");
    source.blocks.push(Block::paragraph("here"));
    source.blocks[0].id = StableId::parse("target").unwrap();
    source.bookmarks.push(Bookmark {
        id: StableId::parse("bookmark-intro").unwrap(),
        name: "Intro".to_string(),
        block_id: StableId::parse("target").unwrap(),
        revision: 1,
        deleted: false,
    });
    let bytes = export_odt(&source, &BTreeMap::<String, ExportImage>::new()).unwrap();
    let report = import_odt_bytes("Round trip", &bytes).unwrap();
    assert_eq!(block_plain_text(&report.document.blocks[0]), "here");
    assert_eq!(report.document.bookmarks.len(), 1);
    assert_eq!(report.document.bookmarks[0].name, "Intro");
    assert_eq!(
        report.document.bookmarks[0].block_id,
        report.document.blocks[0].id
    );
    assert!(report.warnings.is_empty());
}
