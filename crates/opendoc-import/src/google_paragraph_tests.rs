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
fn imports_google_rendered_weight_700_as_the_existing_bold_mark() {
    // Docs represents CSS weight separately from `bold`.  Its documented
    // default weight is 400, so an explicit 700 is exactly the rendering
    // produced by the OpenDoc Bold mark on Google export.
    let input = json!({
        "body": { "content": [{
            "paragraph": { "elements": [{ "textRun": {
                "content": "Weighted\\n",
                "textStyle": {
                    "bold": false,
                    "weightedFontFamily": { "fontFamily": "Roboto", "weight": 700 }
                }
            } }] }
        }] }
    });

    let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let marks = text_marks(&report.document.blocks[0]);
    assert!(marks.iter().any(|mark| mark.kind == MarkKind::Bold));
    assert!(marks
        .iter()
        .any(|mark| { mark.kind == MarkKind::Font && mark.value.as_deref() == Some("Roboto") }));

    let bytes = export_google_docs_json(&report.document).unwrap();
    let exported: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let style = &exported["body"]["content"][0]["paragraph"]["elements"][0]["textRun"]["textStyle"];
    assert_eq!(style["bold"], true);
    assert_eq!(style["weightedFontFamily"]["fontFamily"], "Roboto");
    // Weight is intentionally omitted: Google defaults it to 400 then applies
    // `bold`, yielding exactly the imported rendered weight of 700.
    assert!(style["weightedFontFamily"].get("weight").is_none());
}

#[test]
fn discloses_google_font_weights_that_have_no_opendoc_equivalent() {
    let input = json!({
        "body": { "content": [{
            "paragraph": { "elements": [{ "textRun": {
                "content": "Thin\\n",
                "textStyle": {
                    "weightedFontFamily": { "fontFamily": "Roboto", "weight": 300 }
                }
            } }] }
        }] }
    });

    let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();
    assert!(report
        .warnings
        .iter()
        .any(|warning| { warning.code == "google-text-style-font-weight-degraded" }));
    let marks = text_marks(&report.document.blocks[0]);
    assert!(!marks.iter().any(|mark| mark.kind == MarkKind::Bold));
    assert!(marks
        .iter()
        .any(|mark| { mark.kind == MarkKind::Font && mark.value.as_deref() == Some("Roboto") }));
}

#[test]
fn google_text_font_size_round_trips_only_the_native_point_dimension() {
    let input = json!({
        "body": { "content": [{
            "paragraph": { "elements": [{ "textRun": {
                "content": "Sized\\n",
                "textStyle": { "fontSize": { "magnitude": 13.5, "unit": "PT" } }
            } }] }
        }] }
    });

    let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let marks = text_marks(&report.document.blocks[0]);
    assert!(marks
        .iter()
        .any(|mark| { mark.kind == MarkKind::Size && mark.value.as_deref() == Some("13.5") }));

    let bytes = export_google_docs_json(&report.document).unwrap();
    let exported: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let font_size = &exported["body"]["content"][0]["paragraph"]["elements"][0]["textRun"]
        ["textStyle"]["fontSize"];
    assert_eq!(font_size["magnitude"], 13.5);
    assert_eq!(font_size["unit"], "PT");
}

#[test]
fn google_text_font_size_that_opendoc_cannot_render_is_precisely_disclosed() {
    let input = json!({
        "body": { "content": [{
            "paragraph": { "elements": [{ "textRun": {
                "content": "Sized\\n",
                "textStyle": { "bold": true, "fontSize": { "magnitude": 12, "unit": "PX" } }
            } }] }
        }] }
    });

    let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();
    assert!(report.warnings.iter().any(|warning| {
        warning.code == "google-dropped-text-font-size"
            && warning.message.contains("unsupported unit PX")
    }));
    let marks = text_marks(&report.document.blocks[0]);
    assert!(marks.iter().any(|mark| mark.kind == MarkKind::Bold));
    assert!(!marks.iter().any(|mark| mark.kind == MarkKind::Size));
}

#[test]
fn explicit_google_small_caps_false_is_not_a_phantom_loss() {
    let input = json!({
        "body": { "content": [{
            "paragraph": { "elements": [{ "textRun": {
                "content": "Ordinary\\n",
                "textStyle": { "smallCaps": false }
            } }] }
        }] }
    });

    let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
}

#[test]
fn imports_first_google_tab_through_the_normal_body_pipeline_and_names_the_rest() {
    let input = json!({
        "tabs": [{
            "tabProperties": { "tabId": "first", "title": "First tab" },
            "documentTab": {
                "body": { "content": [{ "paragraph": {
                    "elements": [{ "textRun": { "content": "First tab text\n" } }]
                }}] },
                "lists": {}
            },
            "childTabs": [{
                "tabProperties": { "tabId": "child", "title": "Child tab" },
                "documentTab": { "body": { "content": [{ "paragraph": {
                    "elements": [{ "textRun": { "content": "Not silently merged\n" } }]
                }}] } }
            }]
        }]
    });

    let report = import_google_docs_json("Google tabs", input.to_string().as_bytes()).unwrap();
    assert_eq!(report.document.visible_text(), "First tab text\n");
    assert!(report.warnings.iter().any(|warning| {
        warning.code == "google-tabs-first-tab-only"
            && warning.message.contains("2 tab(s)")
            && warning.message.contains("First tab")
    }));
}

#[test]
fn imports_only_the_final_google_paragraph_terminator() {
    let input = json!({
        "body": { "content": [{
            "paragraph": { "elements": [
                { "textRun": { "content": "first line\n", "textStyle": {} } },
                { "textRun": { "content": "second line\n\n", "textStyle": {} } }
            ] }
        }] }
    });

    let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();
    assert_eq!(report.document.visible_text(), "first line\nsecond line\n");
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
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
fn imports_native_google_internal_links_as_plain_text_with_a_warning() {
    for (link, destination) in [
        (json!({ "tabId": "tab-1" }), "tab"),
        (json!({ "bookmarkId": "bookmark-1" }), "bookmark"),
        (json!({ "headingId": "heading-1" }), "heading"),
        (
            json!({ "bookmark": { "id": "bookmark-1", "tabId": "tab-1" } }),
            "bookmark",
        ),
        (
            json!({ "heading": { "id": "heading-1", "tabId": "tab-1" } }),
            "heading",
        ),
    ] {
        let input = json!({
            "body": { "content": [{
                "paragraph": { "elements": [{
                    "textRun": {
                        "content": "Jump",
                        "textStyle": { "italic": true, "link": link }
                    }
                }] }
            }] }
        });
        let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();
        match &report.document.blocks[0].content[0] {
            opendoc_core::Inline::Text { text, marks, .. } => {
                assert_eq!(text, "Jump");
                assert!(marks
                    .iter()
                    .any(|mark| matches!(mark.kind, MarkKind::Italic)));
            }
            other => panic!("expected plain text inline, got {other:?}"),
        }
        assert!(report.warnings.iter().any(|warning| {
            warning.code == "google-internal-link-dropped" && warning.message.contains(destination)
        }));
    }
}

#[test]
fn imports_unsafe_google_docs_external_links_as_plain_text_with_a_warning() {
    for href in [
        "javascript:alert(1)",
        "data:text/html,unsafe",
        "https://example.test/\u{7f}",
    ] {
        let input = json!({
            "body": { "content": [{
                "paragraph": { "elements": [{
                    "textRun": {
                        "content": "Do not navigate",
                        "textStyle": { "italic": true, "link": { "url": href } }
                    }
                }] }
            }] }
        });
        let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();
        match &report.document.blocks[0].content[0] {
            opendoc_core::Inline::Text { text, marks, .. } => {
                assert_eq!(text, "Do not navigate");
                assert!(marks
                    .iter()
                    .any(|mark| matches!(mark.kind, MarkKind::Italic)));
            }
            other => panic!("expected plain text inline, got {other:?}"),
        }
        assert!(report.warnings.iter().any(|warning| {
            warning.code == "google-unsafe-external-link-dropped"
                && warning.message.contains("unsafe navigation scheme")
        }));
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
        (
            json!({
                "body": { "content": [{
                    "paragraph": { "elements": [{
                        "textRun": {
                            "content": "OpenDoc",
                            "textStyle": { "link": { "bookmarkId": 7 } }
                        }
                    }] }
                }] }
            }),
            "Google Docs internal link bookmarkId must be a string",
        ),
        (
            json!({
                "body": { "content": [{
                    "paragraph": { "elements": [{
                        "textRun": {
                            "content": "OpenDoc",
                            "textStyle": { "link": { "bookmark": [] } }
                        }
                    }] }
                }] }
            }),
            "bookmark must be an object",
        ),
        (
            json!({
                "body": { "content": [{
                    "paragraph": { "elements": [{
                        "textRun": {
                            "content": "OpenDoc",
                            "textStyle": { "link": { "bookmarkId": "bookmark-1", "headingId": "heading-1" } }
                        }
                    }] }
                }] }
            }),
            "Google Docs link has multiple destinations",
        ),
        (
            json!({
                "body": { "content": [{
                    "paragraph": { "elements": [{
                        "textRun": {
                            "content": "OpenDoc",
                            "textStyle": { "link": { "url": "https://example.test", "bookmarkId": "bookmark-1" } }
                        }
                    }] }
                }] }
            }),
            "Google Docs link has multiple destinations",
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
fn imports_google_list_start_from_the_list_definition_not_the_item() {
    let input = json!({
        "lists": { "continued": { "listProperties": { "nestingLevels": [
            { "glyphType": "DECIMAL", "startNumber": 7 }
        ] } } },
        "body": { "content": [{
            "paragraph": {
                "bullet": { "listId": "continued", "nestingLevel": 0 },
                "elements": [{ "textRun": { "content": "Seven", "textStyle": {} } }]
            }
        }] }
    });
    let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();
    let list_id = report.document.blocks[0].list_id().expect("list item");
    assert_eq!(
        report
            .document
            .list_properties
            .get(list_id)
            .expect("used list has settings")
            .start_for(0),
        7
    );
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
