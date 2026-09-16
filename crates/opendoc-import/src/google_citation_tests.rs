use crate::*;
use opendoc_core::{BlockKind, CitationPlacement, CitationSourceFormat, Inline, StableId};
use serde_json::json;

#[test]
fn drops_google_docs_equation_without_fabricating_equation_source() {
    let input = json!({
        "body": { "content": [{
            "paragraph": { "elements": [
                { "textRun": { "content": "Equation: ", "textStyle": {} } },
                { "equation": {} }
            ] }
        }] }
    });
    let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.code == "google-equation-source-unavailable"));
    assert_eq!(report.document.blocks[0].content.len(), 1);
    assert!(matches!(
        &report.document.blocks[0].content[0],
        Inline::Text { text, .. } if text == "Equation: "
    ));
}

#[test]
fn native_google_equation_metadata_is_not_treated_as_latex_or_visible_text() {
    // Native Docs equation objects can carry review metadata. Neither that
    // metadata nor arbitrary, undocumented JSON is a source/accessible-text
    // contract, so accepting it would create a false editable equation.
    let input = json!({
        "body": { "content": [{
            "paragraph": { "elements": [
                { "textRun": { "content": "Before ", "textStyle": {} } },
                { "equation": {
                    "suggestedInsertionIds": ["suggestion-1"],
                    "source": "x^2",
                    "content": "x squared"
                } },
                { "textRun": { "content": " after\n", "textStyle": {} } }
            ] }
        }] }
    });

    let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();
    let warning = report
        .warnings
        .iter()
        .find(|warning| warning.code == "google-equation-source-unavailable")
        .expect("native equation must disclose its unavailable source");
    assert!(warning.message.contains("neither equation source"));
    assert!(report.document.blocks[0]
        .content
        .iter()
        .all(|inline| !matches!(inline, Inline::Equation { .. })));
    let visible_text: String = report.document.blocks[0]
        .content
        .iter()
        .filter_map(|inline| match inline {
            Inline::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(visible_text, "Before  after");
}

#[test]
fn imports_google_docs_citation_extension_as_document_local_source() {
    let input = json!({
        "body": { "content": [{
            "paragraph": { "elements": [
                { "textRun": { "content": "Prior work ", "textStyle": {} } },
                { "opendocCitation": {
                    "citationId": " cite-doe-2020 ",
                    "renderedCache": "(Doe 2020, p. 42)"
                } }
            ] }
        }] },
        "footnotes": {
            "fn-cite": {
                "footnoteId": " fn-cite ",
                "content": [{
                    "paragraph": { "elements": [
                        { "textRun": { "content": "Citation footnote", "textStyle": {} } }
                    ] }
                }]
            }
        },
        "opendocCitations": {
            "style": "apa-7th",
            "locale": "en-US",
            "references": [{
                "id": " ref-doe-2020 ",
                "revision": 3,
                "format": "citum-native",
                "bytesUtf8": "doe citation payload",
                "summary": {
                    "title": "Example Article",
                    "authors": ["Doe", "Roe"],
                    "issued": "2020",
                    "doi": "10.1000/example",
                    "url": "https://example.invalid/article"
                },
                "deleted": false
            }],
            "groups": [
                {
                    "id": " cite-doe-2020 ",
                    "revision": 4,
                    "items": [{
                        "referenceId": " ref-doe-2020 ",
                        "locator": "42",
                        "label": "page",
                        "prefix": "see",
                        "suffix": "for details",
                        "suppressAuthor": true
                    }],
                    "placement": "inline",
                    "renderedCache": "(Doe 2020, p. 42)",
                    "deleted": false
                },
                {
                    "id": " cite-footnote ",
                    "revision": 5,
                    "items": [{
                        "referenceId": " ref-doe-2020 ",
                        "locator": "9",
                        "label": "page",
                        "prefix": null,
                        "suffix": null,
                        "suppressAuthor": false
                    }],
                    "placement": "footnote",
                    "footnoteId": " fn-cite ",
                    "renderedCache": "(Doe 2020, p. 9)",
                    "deleted": false
                }
            ]
        }
    });
    let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.code == "opendoc-google-citations-extension"));
    match &report.document.blocks[0].content[1] {
        Inline::Citation {
            citation_id,
            rendered_cache,
            ..
        } => {
            assert_eq!(citation_id.as_str(), "cite-doe-2020");
            assert_eq!(rendered_cache.as_deref(), Some("(Doe 2020, p. 42)"));
        }
        other => panic!("expected citation inline, got {other:?}"),
    }
    let reference = &report.document.citation_database.references[0];
    assert_eq!(reference.id.as_str(), "ref-doe-2020");
    assert_eq!(reference.revision, 3);
    assert_eq!(reference.summary.title, "Example Article");
    assert_eq!(reference.summary.authors, vec!["Doe", "Roe"]);
    assert_eq!(reference.source.format, CitationSourceFormat::CitumNative);
    assert_eq!(reference.source.bytes, b"doe citation payload");
    let citation = &report.document.citation_database.citations[0];
    assert_eq!(citation.id.as_str(), "cite-doe-2020");
    assert_eq!(citation.items[0].reference_id.as_str(), "ref-doe-2020");
    assert_eq!(citation.items[0].locator.as_deref(), Some("42"));
    assert_eq!(citation.items[0].prefix.as_deref(), Some("see"));
    assert!(citation.items[0].suppress_author);
    let footnote_citation = report
        .document
        .citation_database
        .citations
        .iter()
        .find(|citation| citation.id.as_str() == "cite-footnote")
        .unwrap();
    assert_eq!(
        footnote_citation.placement,
        CitationPlacement::Footnote {
            footnote_id: StableId::parse("fn-cite").unwrap()
        }
    );
}

#[test]
fn google_docs_citation_import_clears_stale_labels_for_missing_references() {
    let input = json!({
        "body": { "content": [{
            "paragraph": { "elements": [
                { "opendocCitation": {
                    "citationId": "cite-missing-reference",
                    "renderedCache": "(Misleading 2020)"
                } }
            ] }
        }] },
        "opendocCitations": {
            "style": "apa-7th",
            "locale": "en-US",
            "references": [],
            "groups": [{
                "id": "cite-missing-reference",
                "revision": 1,
                "items": [{
                    "referenceId": "ref-missing",
                    "locator": "42",
                    "label": "page",
                    "suppressAuthor": false
                }],
                "placement": "inline",
                "renderedCache": "(Misleading 2020)",
                "deleted": false
            }]
        }
    });

    let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();

    assert_eq!(report.document.visible_text(), "[cite-missing-reference]\n");
    assert_eq!(
        report.document.citation_database.citations[0].rendered_cache,
        None
    );
    match &report.document.blocks[0].content[0] {
        Inline::Citation { rendered_cache, .. } => assert_eq!(rendered_cache, &None),
        other => panic!("expected citation inline, got {other:?}"),
    }
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.code == "citation-reference-missing"));
    assert!(report.document.validate().is_ok());
}

#[test]
fn google_docs_citation_import_moves_missing_footnote_placement_inline() {
    let input = json!({
        "body": { "content": [{
            "paragraph": { "elements": [
                { "opendocCitation": {
                    "citationId": "cite-missing-footnote",
                    "renderedCache": "(Misleading footnote)"
                } }
            ] }
        }] },
        "opendocCitations": {
            "style": "apa-7th",
            "locale": "en-US",
            "references": [{
                "id": "ref-footnote",
                "revision": 1,
                "format": "citum-native",
                "bytesUtf8": "title: Footnote",
                "summary": { "title": "Footnote" },
                "deleted": false
            }],
            "groups": [{
                "id": "cite-missing-footnote",
                "revision": 1,
                "items": [{
                    "referenceId": "ref-footnote",
                    "suppressAuthor": false
                }],
                "placement": "footnote",
                "footnoteId": "fn-missing",
                "renderedCache": "(Misleading footnote)",
                "deleted": false
            }]
        }
    });

    let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();

    assert_eq!(report.document.visible_text(), "[cite-missing-footnote]\n");
    assert_eq!(
        report.document.citation_database.citations[0].placement,
        CitationPlacement::Inline
    );
    assert_eq!(
        report.document.citation_database.citations[0].rendered_cache,
        None
    );
    match &report.document.blocks[0].content[0] {
        Inline::Citation { rendered_cache, .. } => assert_eq!(rendered_cache, &None),
        other => panic!("expected citation inline, got {other:?}"),
    }
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.code == "citation-footnote-target-missing"));
    assert!(report.document.validate().is_ok());
}

#[test]
fn google_docs_citation_import_clears_stale_labels_for_missing_groups() {
    let input = json!({
        "body": { "content": [{
            "paragraph": { "elements": [
                { "opendocCitation": {
                    "citationId": "cite-missing-group",
                    "renderedCache": "(Stale Citation)"
                } }
            ] }
        }] },
        "opendocCitations": {
            "style": "apa-7th",
            "locale": "en-US",
            "references": [],
            "groups": []
        }
    });

    let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();

    assert_eq!(report.document.visible_text(), "[cite-missing-group]\n");
    match &report.document.blocks[0].content[0] {
        Inline::Citation { rendered_cache, .. } => assert_eq!(rendered_cache, &None),
        other => panic!("expected citation inline, got {other:?}"),
    }
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.code == "citation-group-missing"));
    assert!(report.document.validate().is_ok());
}

#[test]
fn google_docs_citation_import_clears_stale_footnote_label_for_missing_group() {
    // Footnote bodies are a separate inline container in the Docs resource.
    // A body-only repair used to leave this attacker-controlled cache visible
    // even after reporting that the group did not exist.
    let input = json!({
        "body": { "content": [{
            "paragraph": { "elements": [{
                "footnoteReference": { "footnoteId": "fn-stale-label" }
            }] }
        }] },
        "footnotes": {
            "fn-stale-label": {
                "footnoteId": "fn-stale-label",
                "content": [{ "paragraph": { "elements": [{
                    "opendocCitation": {
                        "citationId": "cite-no-group",
                        "renderedCache": "(stale label)"
                    }
                }] } }]
            }
        }
    });

    let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();
    match &report.document.footnotes[0].body[0] {
        Inline::Citation { rendered_cache, .. } => assert_eq!(rendered_cache, &None),
        other => panic!("expected citation inline, got {other:?}"),
    }
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.code == "citation-group-missing"));
    assert!(report.document.validate().is_ok());
}

#[test]
fn google_docs_citation_import_clears_stale_footnote_label_for_missing_reference() {
    let input = json!({
        "body": { "content": [{
            "paragraph": { "elements": [{
                "footnoteReference": { "footnoteId": "fn-stale-reference" }
            }] }
        }] },
        "footnotes": {
            "fn-stale-reference": {
                "footnoteId": "fn-stale-reference",
                "content": [{ "paragraph": { "elements": [{
                    "opendocCitation": {
                        "citationId": "cite-no-reference",
                        "renderedCache": "(stale reference)"
                    }
                }] } }]
            }
        },
        "opendocCitations": {
            "references": [],
            "groups": [{
                "id": "cite-no-reference",
                "revision": 1,
                "items": [{ "referenceId": "ref-missing", "suppressAuthor": false }],
                "placement": "inline",
                "renderedCache": "(stale reference)",
                "deleted": false
            }]
        }
    });

    let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();
    match &report.document.footnotes[0].body[0] {
        Inline::Citation { rendered_cache, .. } => assert_eq!(rendered_cache, &None),
        other => panic!("expected citation inline, got {other:?}"),
    }
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.code == "citation-reference-missing"));
    assert!(report.document.validate().is_ok());
}

#[test]
fn google_docs_citation_import_clears_nested_labels_for_deleted_groups() {
    let input = json!({
        "body": { "content": [{
            "table": {
                "tableRows": [{
                    "tableCells": [{
                        "content": [{
                            "paragraph": { "elements": [{
                                "opendocCitation": {
                                    "citationId": "cite-deleted-group",
                                    "renderedCache": "(Stale Citation)"
                                }
                            }] }
                        }]
                    }]
                }]
            }
        }] },
        "opendocCitations": {
            "style": "apa-7th",
            "locale": "en-US",
            "references": [{
                "id": "ref-deleted-group",
                "revision": 1,
                "format": "citum-native",
                "bytesUtf8": "title: Deleted Group",
                "summary": { "title": "Deleted Group" },
                "deleted": false
            }],
            "groups": [{
                "id": "cite-deleted-group",
                "revision": 2,
                "items": [{
                    "referenceId": "ref-deleted-group",
                    "suppressAuthor": false
                }],
                "placement": "inline",
                "renderedCache": "(Stale Citation)",
                "deleted": true
            }]
        }
    });

    let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();

    assert_eq!(report.document.visible_text(), "[cite-deleted-group]\n");
    match &report.document.blocks[0].kind {
        BlockKind::Table { rows, .. } => match &rows[0].cells[0].blocks[0].content[0] {
            Inline::Citation { rendered_cache, .. } => assert_eq!(rendered_cache, &None),
            other => panic!("expected citation inline, got {other:?}"),
        },
        other => panic!("expected table, got {other:?}"),
    }
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.code == "citation-group-missing"));
    assert!(report.document.validate().is_ok());
}

#[test]
fn malformed_citation_extension_import_aborts() {
    let mut input = json!({
        "body": { "content": [{
            "paragraph": { "elements": [
                { "opendocCitation": { "citationId": "cite-bad" } }
            ] }
        }] },
        "opendocCitations": {
            "style": "apa-7th",
            "locale": "en-US",
            "references": [{
                "id": "ref-bad",
                "revision": 1,
                "format": "citum-native",
                "bytesUtf8": "title: Bad",
                "summary": {
                    "title": "Bad",
                    "authors": [" "]
                },
                "deleted": false
            }],
            "groups": [{
                "id": "cite-bad",
                "revision": 1,
                "items": [{
                    "referenceId": "ref-bad",
                    "locator": null,
                    "label": null,
                    "prefix": null,
                    "suffix": null,
                    "suppressAuthor": false
                }],
                "placement": "inline",
                "deleted": false
            }]
        }
    });
    assert!(matches!(
        import_google_docs_json("Google", input.to_string().as_bytes()),
        Err(ImportError::InvalidDocument(message))
            if message == "bibliography summary field is empty"
    ));

    input["opendocCitations"]["references"][0]["summary"]["authors"] = json!(["Doe"]);
    input["opendocCitations"]["groups"][0]["items"][0]["prefix"] = json!(" ");
    assert!(matches!(
        import_google_docs_json("Google", input.to_string().as_bytes()),
        Err(ImportError::InvalidInput(message))
            if message == "citation item field is empty"
    ));

    input["opendocCitations"]["groups"][0]["items"][0]["prefix"] = json!(" see ");
    assert!(matches!(
        import_google_docs_json("Google", input.to_string().as_bytes()),
        Err(ImportError::InvalidInput(message))
            if message == "citation item field has surrounding whitespace"
    ));

    input["opendocCitations"]["groups"][0]["items"][0]["prefix"] = json!(null);
    input["opendocCitations"]["references"][0]["summary"]["doi"] = json!(" 10.123/example ");
    assert!(matches!(
        import_google_docs_json("Google", input.to_string().as_bytes()),
        Err(ImportError::InvalidInput(message))
            if message == "bibliography summary field has surrounding whitespace"
    ));

    let base = json!({
        "body": { "content": [{
            "paragraph": { "elements": [
                { "opendocCitation": { "citationId": "cite-bad" } }
            ] }
        }] },
        "opendocCitations": {
            "style": "apa-7th",
            "locale": "en-US",
            "references": [{
                "id": "ref-bad",
                "revision": 1,
                "format": "citum-native",
                "bytesUtf8": "title: Bad",
                "summary": {
                    "title": "Bad",
                    "authors": ["Doe"]
                },
                "deleted": false
            }],
            "groups": [{
                "id": "cite-bad",
                "revision": 1,
                "items": [{
                    "referenceId": "ref-bad",
                    "locator": null,
                    "label": null,
                    "prefix": null,
                    "suffix": null,
                    "suppressAuthor": false
                }],
                "placement": "inline",
                "deleted": false
            }]
        }
    });

    let mut input = base.clone();
    input["opendocCitations"] = json!("bad");
    let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
    assert!(
        err.to_string()
            .contains("opendocCitations must be an object"),
        "{err}"
    );

    let mut input = base.clone();
    input["opendocCitations"]["references"] = json!({});
    let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
    assert!(
        err.to_string().contains("references must be an array"),
        "{err}"
    );

    let mut input = base.clone();
    input["opendocCitations"]["groups"] = json!({});
    let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
    assert!(err.to_string().contains("groups must be an array"), "{err}");

    let mut input = base.clone();
    input["opendocCitations"]["references"] = json!(["bad"]);
    let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
    assert!(
        err.to_string()
            .contains("citation reference must be an object"),
        "{err}"
    );

    let mut input = base.clone();
    input["opendocCitations"]["groups"] = json!(["bad"]);
    let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
    assert!(
        err.to_string().contains("citation group must be an object"),
        "{err}"
    );

    let mut input = base.clone();
    input["opendocCitations"]["references"][0]["revision"] = json!("1");
    let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
    assert!(
        err.to_string()
            .contains("revision must be a non-negative integer"),
        "{err}"
    );

    let mut input = base.clone();
    input["opendocCitations"]["references"][0]["summary"] = json!("bad");
    let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
    assert!(
        err.to_string()
            .contains("citation summary must be an object"),
        "{err}"
    );

    let mut input = base.clone();
    input["opendocCitations"]["references"][0]["summary"]["authors"] = json!(["Doe", 7]);
    let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
    assert!(
        err.to_string()
            .contains("citation summary authors entries must be strings"),
        "{err}"
    );

    let mut input = base.clone();
    input["opendocCitations"]["references"][0]["deleted"] = json!("false");
    let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
    assert!(
        err.to_string().contains("deleted must be a boolean"),
        "{err}"
    );

    let mut input = base.clone();
    input["opendocCitations"]["groups"][0]["items"] = json!("bad");
    let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
    assert!(
        err.to_string()
            .contains("citation group missing required array field items"),
        "{err}"
    );

    let mut input = base.clone();
    input["opendocCitations"]["groups"][0]["items"] = json!(["bad"]);
    let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
    assert!(
        err.to_string().contains("citation item must be an object"),
        "{err}"
    );

    let mut input = base.clone();
    input["opendocCitations"]["groups"][0]["placement"] = json!(7);
    let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
    assert!(
        err.to_string().contains("placement must be a string"),
        "{err}"
    );

    let mut input = base.clone();
    input["opendocCitations"]["groups"][0]["items"][0]["suppressAuthor"] = json!("false");
    let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
    assert!(
        err.to_string().contains("suppressAuthor must be a boolean"),
        "{err}"
    );
}

#[test]
fn citation_import_fills_missing_summary_from_citum_native_source() {
    let input = json!({
        "body": { "content": [{
            "paragraph": { "elements": [
                { "textRun": { "content": "Prior work ", "textStyle": {} } },
                { "opendocCitation": { "citationId": "cite-doe-2020" } }
            ] }
        }] },
        "opendocCitations": {
            "style": "apa-7th",
            "locale": "en-US",
            "references": [{
                "id": "ref-doe-2020",
                "revision": 3,
                "format": "citum-native",
                "bytesUtf8": "title: Example Article\nauthor: Doe; Roe\nyear: 2020\ndoi: 10.1000/example\nurl: https://example.invalid/article\n",
                "summary": {
                    "title": "",
                    "authors": []
                },
                "deleted": false
            }],
            "groups": [{
                "id": "cite-doe-2020",
                "revision": 4,
                "items": [{
                    "referenceId": "ref-doe-2020",
                    "locator": "42",
                    "label": "page",
                    "prefix": "see",
                    "suffix": null,
                    "suppressAuthor": false
                }],
                "placement": "inline",
                "deleted": false
            }]
        }
    });

    let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();
    let reference = &report.document.citation_database.references[0];
    assert_eq!(reference.summary.title, "Example Article");
    assert_eq!(reference.summary.authors, vec!["Doe", "Roe"]);
    assert_eq!(reference.summary.issued.as_deref(), Some("2020"));
    assert_eq!(reference.summary.doi.as_deref(), Some("10.1000/example"));
    assert_eq!(
        reference.summary.url.as_deref(),
        Some("https://example.invalid/article")
    );
    assert!(report.document.validate().is_ok());
}

#[test]
fn citation_import_unescapes_citum_native_source_fields() {
    let input = json!({
        "body": { "content": [{
            "paragraph": { "elements": [
                { "textRun": { "content": "Escaped source ", "textStyle": {} } },
                { "opendocCitation": { "citationId": "cite-escaped" } }
            ] }
        }] },
        "opendocCitations": {
            "style": "apa-7th",
            "locale": "en-US",
            "references": [{
                "id": "ref-escaped",
                "revision": 1,
                "format": "citum-native",
                "bytesUtf8": "title: Line one\\nLine two\\; source\nauthor: Curie\\; Lab; Roe\\\\Unit\nyear: 1911\nurl: https://example.invalid/a\\;b\n",
                "summary": {
                    "title": "",
                    "authors": []
                },
                "deleted": false
            }],
            "groups": [{
                "id": "cite-escaped",
                "revision": 1,
                "items": [{
                    "referenceId": "ref-escaped",
                    "suppressAuthor": false
                }],
                "placement": "inline",
                "deleted": false
            }]
        }
    });

    let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();
    let reference = &report.document.citation_database.references[0];
    assert_eq!(reference.summary.title, "Line one\nLine two; source");
    assert_eq!(reference.summary.authors, vec!["Curie; Lab", "Roe\\Unit"]);
    assert_eq!(reference.summary.issued.as_deref(), Some("1911"));
    assert_eq!(
        reference.summary.url.as_deref(),
        Some("https://example.invalid/a;b")
    );
    assert_eq!(
        report.document.visible_text(),
        "Escaped source (Curie; Lab 1911)\n"
    );
    assert!(report.document.validate().is_ok());
}

#[test]
fn footnote_citation_import_without_footnote_id_aborts() {
    let input = json!({
        "body": { "content": [{
            "paragraph": { "elements": [
                { "textRun": { "content": "Prior work ", "textStyle": {} } }
            ] }
        }] },
        "opendocCitations": {
            "style": "apa-7th",
            "locale": "en-US",
            "references": [{
                "id": "ref-doe-2020",
                "revision": 1,
                "format": "citum-native",
                "bytesUtf8": "doe citation payload",
                "summary": {
                    "title": "Example Article",
                    "authors": ["Doe"],
                    "issued": "2020"
                },
                "deleted": false
            }],
            "groups": [{
                "id": "cite-footnote",
                "revision": 1,
                "items": [{
                    "referenceId": "ref-doe-2020",
                    "locator": "9",
                    "label": "page",
                    "prefix": null,
                    "suffix": null,
                    "suppressAuthor": false
                }],
                "placement": "footnote",
                "renderedCache": "(Doe 2020, p. 9)",
                "deleted": false
            }]
        }
    });

    let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
    assert!(
        err.to_string()
            .contains("footnote citation missing required string field footnoteId"),
        "{err}"
    );
}

/// A brand-new document exports without a citation warning.
///
/// `CitationDatabase::default().style` used to be `apa-7th`, a name
/// `opendoc_citations::resolve_style_name` has never resolved. While
/// `apa-7th` was exempt from `citation_support_warnings` that was invisible;
/// the moment the exemption was removed — correctly, because it meant the
/// commonest database in the product was the one nobody was ever told about —
/// **every** new document that cited anything started reporting
/// `citation-style-not-bundled` on export, telling the user their citations
/// had been formatted by the built-in renderer rather than by CSL.
///
/// The default is now `apa`, which is one of the eight bundled CSL styles.
/// The `apa-7th` half of this test is the control: without it, the assertion
/// would also hold for a `citation_support_warnings` that had gone back to
/// exempting everything, or for an export that stopped calling it.
#[test]
fn a_new_documents_default_citation_style_is_one_the_bundle_renders() {
    let cited = |style: Option<&str>| {
        let mut document = opendoc_core::Document::new("Cited");
        if let Some(style) = style {
            document.citation_database.style = style.to_string();
        }
        document
            .citation_database
            .references
            .push(opendoc_core::BibliographyReference {
                id: StableId::parse("ref-doe-2020").unwrap(),
                revision: 1,
                source: opendoc_core::CitationSource {
                    format: CitationSourceFormat::CitumNative,
                    bytes: b"doe".to_vec(),
                },
                summary: opendoc_core::CitationSummary {
                    title: "Example".to_string(),
                    authors: vec!["Doe".to_string()],
                    issued: Some("2020".to_string()),
                    doi: None,
                    url: None,
                },
                deleted: false,
            });
        document.validate().expect("a valid cited document");
        let (_, warnings) =
            export_google_docs_json_with_warnings(&document).expect("the export succeeds");
        warnings
            .into_iter()
            .map(|warning| warning.code)
            .collect::<Vec<_>>()
    };

    assert!(
        !cited(None).contains(&"citation-style-not-bundled".to_string()),
        "a document nobody chose a citation style for warns about the style it was given: {:?}",
        cited(None)
    );
    // The control: the style the default used to be does warn, so the
    // assertion above is about the default and not about the warning having
    // gone quiet.
    assert!(
        cited(Some("apa-7th")).contains(&"citation-style-not-bundled".to_string()),
        "the historical default no longer warns, so the assertion above proves nothing: {:?}",
        cited(Some("apa-7th"))
    );
}
