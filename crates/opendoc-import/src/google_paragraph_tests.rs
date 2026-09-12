use crate::test_support::text_marks;
use crate::*;
use opendoc_core::ListKind;
use opendoc_core::{BlockKind, MarkKind};
use serde_json::json;

#[test]
fn imports_google_docs_paragraph_heading_and_marks() {
    let input = json!({
        "body": {
            "content": [
                {
                    "paragraph": {
                        "paragraphStyle": { "namedStyleType": "HEADING_2" },
                        "elements": [
                            { "textRun": {
                                "content": "Heading\n",
                                "textStyle": { "bold": true }
                            } }
                        ]
                    }
                },
                {
                    "paragraph": {
                        "elements": [
                            { "textRun": {
                                "content": "Body",
                                "textStyle": {
                                    "italic": true,
                                    "underline": true,
                                    "foregroundColor": {
                                        "color": { "rgbColor": { "red": 1.0, "green": 0.0, "blue": 0.0 } }
                                    }
                                }
                            } }
                        ]
                    }
                }
            ]
        }
    });
    let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();
    assert!(report.warnings.is_empty());
    assert_eq!(report.document.visible_text(), "Heading\nBody\n");
    assert!(matches!(
        report.document.blocks[0].kind,
        BlockKind::Heading { level: 2 }
    ));
    let marks = text_marks(&report.document.blocks[1]);
    assert!(marks.iter().any(|mark| mark.kind == MarkKind::Italic));
    assert!(marks.iter().any(|mark| mark.kind == MarkKind::Underline));
    assert!(marks
        .iter()
        .any(|mark| mark.kind == MarkKind::Color && mark.value.as_deref() == Some("#ff0000")));
}

#[test]
fn google_docs_import_canonicalizes_wrapper_title() {
    let input = json!({
        "body": {
            "content": [
                { "paragraph": { "elements": [{ "textRun": { "content": "Body" } }] } }
            ]
        }
    });
    let report = import_google_docs_json(" Imported Title ", input.to_string().as_bytes()).unwrap();
    assert_eq!(report.document.title, "Imported Title");
    assert!(matches!(
        import_google_docs_json(" ", input.to_string().as_bytes()),
        Err(ImportError::InvalidInput(message)) if message == "document title is empty"
    ));
}

#[test]
fn imports_google_docs_links_as_link_inline() {
    let input = json!({
        "body": { "content": [{
            "paragraph": { "elements": [{
                "textRun": {
                    "content": "OpenDoc",
                    "textStyle": { "link": { "url": "https://example.invalid/opendoc" } }
                }
            }] }
        }] }
    });
    let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();
    match &report.document.blocks[0].content[0] {
        opendoc_core::Inline::Link { text, href, .. } => {
            assert_eq!(text, "OpenDoc");
            assert_eq!(href, "https://example.invalid/opendoc");
        }
        other => panic!("expected link inline, got {other:?}"),
    }
}

#[test]
fn rejects_malformed_google_docs_links_without_plain_text_downgrade() {
    for (input, expected) in [
        (
            json!({
                "body": { "content": [{
                    "paragraph": { "elements": [{
                        "textRun": {
                            "content": "OpenDoc",
                            "textStyle": "linked"
                        }
                    }] }
                }] }
            }),
            "textStyle must be an object",
        ),
        (
            json!({
                "body": { "content": [{
                    "paragraph": { "elements": [{
                        "textRun": {
                            "content": "OpenDoc",
                            "textStyle": { "link": [] }
                        }
                    }] }
                }] }
            }),
            "link must be an object",
        ),
        (
            json!({
                "body": { "content": [{
                    "paragraph": { "elements": [{
                        "textRun": {
                            "content": "OpenDoc",
                            "textStyle": { "link": { "url": 7 } }
                        }
                    }] }
                }] }
            }),
            "url must be a string",
        ),
        (
            json!({
                "body": { "content": [{
                    "paragraph": { "elements": [{
                        "textRun": {
                            "content": "OpenDoc",
                            "textStyle": { "link": { "url": " " } }
                        }
                    }] }
                }] }
            }),
            "Google Docs link url is empty",
        ),
    ] {
        let err = import_google_docs_json("Google", input.to_string().as_bytes())
            .unwrap_err()
            .to_string();
        assert!(err.contains(expected), "{err}");
    }
}

#[test]
fn imports_google_docs_list_items_with_level_and_ordering() {
    let input = json!({
        "lists": { "list-1": { "listProperties": { "nestingLevels": [
            { "glyphSymbol": "\u{25cf}" },
            { "glyphSymbol": "\u{25cb}" },
            { "glyphType": "DECIMAL" }
        ] } } },
        "body": { "content": [{
            "paragraph": {
                "bullet": { "listId": "list-1", "nestingLevel": 2 },
                "elements": [{ "textRun": { "content": "Item", "textStyle": {} } }]
            }
        }] }
    });
    let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();
    match &report.document.blocks[0].kind {
        BlockKind::ListItem {
            list_id,
            level,
            kind,
        } => {
            assert_eq!(list_id.as_str(), "list-1");
            assert_eq!(*level, 2);
            assert_eq!(*kind, ListKind::Ordered);
        }
        other => panic!("expected list item, got {other:?}"),
    }
}

#[test]
fn malformed_google_docs_paragraph_structure_metadata_aborts() {
    let base = json!({
        "body": { "content": [{
            "paragraph": {
                "elements": [{ "textRun": { "content": "Item", "textStyle": {} } }]
            }
        }] }
    });

    let mut input = base.clone();
    input["body"]["content"][0]["paragraph"]["paragraphStyle"] = json!("bad");
    let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
    assert!(
        err.to_string().contains("paragraphStyle must be an object"),
        "{err}"
    );

    let mut input = base.clone();
    input["body"]["content"][0]["paragraph"]["paragraphStyle"] = json!({ "namedStyleType": 7 });
    let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
    assert!(
        err.to_string().contains("namedStyleType must be a string"),
        "{err}"
    );

    let mut input = base.clone();
    input["body"]["content"][0]["paragraph"]["bullet"] = json!("bad");
    let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
    assert!(
        err.to_string().contains("bullet must be an object"),
        "{err}"
    );

    let mut input = base.clone();
    input["body"]["content"][0]["paragraph"]["bullet"] = json!({ "listId": 7 });
    let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
    assert!(err.to_string().contains("listId must be a string"), "{err}");

    let mut input = base.clone();
    input["body"]["content"][0]["paragraph"]["bullet"] = json!({ "nestingLevel": "2" });
    let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
    assert!(
        err.to_string()
            .contains("nestingLevel must be a non-negative integer"),
        "{err}"
    );

    let mut input = base.clone();
    input["body"]["content"][0]["paragraph"]["bullet"] = json!({ "nestingLevel": 256 });
    let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
    assert!(
        err.to_string().contains("nestingLevel is too large"),
        "{err}"
    );

    let mut input = base.clone();
    input["body"]["content"][0]["paragraph"]["bullet"] = json!({ "nestingLevel": 9 });
    let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
    assert!(
        err.to_string().contains("nestingLevel is outside 0..=8"),
        "{err}"
    );

    let mut input = base.clone();
    input["body"]["content"][0]["paragraph"]["bullet"] = json!({ "opendocListKind": 7 });
    let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
    assert!(
        err.to_string().contains("opendocListKind must be a string"),
        "{err}"
    );

    let mut input = base;
    input["body"]["content"][0]["paragraph"]["bullet"] = json!({ "opendocListKind": "todo" });
    let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
    assert!(
        err.to_string().contains("unknown opendocListKind todo"),
        "{err}"
    );
}
