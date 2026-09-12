use crate::*;
use opendoc_core::{Anchor, Inline, MarkKind, SuggestionKind, SuggestionState};
use serde_json::json;

#[test]
fn imports_google_docs_comment_and_suggestion_extensions_as_source_state() {
    let input = json!({
        "body": { "content": [{
            "paragraph": { "elements": [
                { "textRun": { "content": "Reviewed text", "textStyle": {} } }
            ] }
        }] },
        "opendocComments": [{
            "id": " thread-one ",
            "anchor": { "type": "textRange", "start": " text-start ", "end": " text-end " },
            "comments": [{
                "id": " comment-one ",
                "author": "Ada",
                "body": [{ "textRun": { "content": "Needs citation", "textStyle": { "bold": true } } }],
                "createdAtMs": 17,
                "deleted": false
            }],
            "deleted": false
        }],
        "opendocSuggestions": [{
            "id": " suggest-one ",
            "author": "Grace",
            "kind": {
                "type": "format",
                "range": { "start": " text-start ", "end": " text-end " },
                "textStyle": { "italic": true }
            },
            "state": "proposed",
            "provenance": ["imported fixture"]
        }]
    });
    let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.code == "opendoc-google-comments-extension"));
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.code == "opendoc-google-suggestions-extension"));
    let thread = &report.document.comments[0];
    assert_eq!(thread.id.as_str(), "thread-one");
    assert!(matches!(thread.anchor, Anchor::TextRange(_)));
    assert_eq!(thread.comments[0].id.as_str(), "comment-one");
    assert_eq!(thread.comments[0].author, "Ada");
    assert_eq!(thread.comments[0].created_at_ms, 17);
    match &thread.comments[0].body[0] {
        Inline::Text { text, marks, .. } => {
            assert_eq!(text, "Needs citation");
            assert!(marks.iter().any(|mark| mark.kind == MarkKind::Bold));
        }
        other => panic!("expected comment text body, got {other:?}"),
    }
    let suggestion = &report.document.suggestions[0];
    assert_eq!(suggestion.id.as_str(), "suggest-one");
    assert_eq!(suggestion.author, "Grace");
    assert_eq!(suggestion.state, SuggestionState::Proposed);
    assert_eq!(suggestion.provenance, vec!["imported fixture"]);
    match &suggestion.kind {
        SuggestionKind::Format { range, marks } => {
            assert_eq!(range.start.as_str(), "text-start");
            assert_eq!(range.end.as_str(), "text-end");
            assert!(marks.iter().any(|mark| mark.kind == MarkKind::Italic));
        }
        other => panic!("expected format suggestion, got {other:?}"),
    }
}

#[test]
fn malformed_review_extension_source_metadata_aborts() {
    let base_body = json!({
        "body": { "content": [{
            "paragraph": { "elements": [
                { "textRun": { "content": "Reviewed text", "textStyle": {} } }
            ] }
        }] }
    });

    let mut input = base_body.clone();
    input["opendocComments"] = json!([{
        "id": "thread-one",
        "comments": [{
            "id": "comment-one",
            "author": " Ada ",
            "body": [{ "textRun": { "content": "Needs citation", "textStyle": {} } }]
        }]
    }]);
    assert!(matches!(
        import_google_docs_json("Google", input.to_string().as_bytes()),
        Err(ImportError::InvalidInput(message))
            if message == "comment author has surrounding whitespace"
    ));

    let mut input = base_body.clone();
    input["opendocSuggestions"] = json!([{
        "id": "suggest-one",
        "author": " Grace ",
        "kind": {
            "type": "delete",
            "range": { "start": "text-start", "end": "text-end" }
        }
    }]);
    assert!(matches!(
        import_google_docs_json("Google", input.to_string().as_bytes()),
        Err(ImportError::InvalidInput(message))
            if message == "suggestion author has surrounding whitespace"
    ));

    let mut input = base_body;
    input["opendocSuggestions"] = json!([{
        "id": "suggest-one",
        "author": "Grace",
        "kind": {
            "type": "delete",
            "range": { "start": "text-start", "end": "text-end" }
        },
        "provenance": [" imported "]
    }]);
    assert!(matches!(
        import_google_docs_json("Google", input.to_string().as_bytes()),
        Err(ImportError::InvalidInput(message))
            if message == "suggestion provenance entry has surrounding whitespace"
    ));
}

#[test]
fn google_docs_extension_lists_must_be_arrays() {
    let input = json!({
        "body": { "content": [{
            "paragraph": { "elements": [
                { "textRun": { "content": "Reviewed text", "textStyle": {} } }
            ] }
        }] },
        "opendocComments": {}
    });
    let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
    assert!(
        err.to_string().contains("opendocComments must be an array"),
        "{err}"
    );

    let input = json!({
        "body": { "content": [{
            "paragraph": { "elements": [
                { "textRun": { "content": "Reviewed text", "textStyle": {} } }
            ] }
        }] },
        "opendocSuggestions": {}
    });
    let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
    assert!(
        err.to_string()
            .contains("opendocSuggestions must be an array"),
        "{err}"
    );
}

#[test]
fn google_docs_comment_and_suggestion_extensions_reject_malformed_source_metadata() {
    let body = json!({
        "body": { "content": [{
            "paragraph": { "elements": [
                { "textRun": { "content": "Reviewed text", "textStyle": {} } }
            ] }
        }] }
    });

    let mut input = body.clone();
    input["opendocComments"] = json!(["bad"]);
    let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
    assert!(
        err.to_string().contains("comment thread must be an object"),
        "{err}"
    );

    let mut input = body.clone();
    input["opendocComments"] = json!([{
        "id": "thread-one",
        "comments": ["bad"]
    }]);
    let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
    assert!(
        err.to_string().contains("comment must be an object"),
        "{err}"
    );

    let mut input = body.clone();
    input["opendocComments"] = json!([{
        "id": "thread-one",
        "comments": [{
            "id": "comment-one",
            "author": "Ada",
            "body": [{ "textRun": { "content": "Body", "textStyle": {} } }],
            "createdAtMs": "17"
        }]
    }]);
    let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
    assert!(
        err.to_string()
            .contains("createdAtMs must be a non-negative integer"),
        "{err}"
    );

    let mut input = body.clone();
    input["opendocComments"] = json!([{
        "id": "thread-one",
        "deleted": "false",
        "comments": [{
            "id": "comment-one",
            "author": "Ada",
            "body": [{ "textRun": { "content": "Body", "textStyle": {} } }]
        }]
    }]);
    let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
    assert!(
        err.to_string().contains("deleted must be a boolean"),
        "{err}"
    );

    let mut input = body.clone();
    input["opendocComments"] = json!([{
        "id": "thread-one",
        "comments": [{
            "id": "comment-one",
            "body": [{ "textRun": { "content": "Body", "textStyle": {} } }]
        }]
    }]);
    let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
    assert!(
        err.to_string()
            .contains("comment missing required string field author"),
        "{err}"
    );

    let mut input = body.clone();
    input["opendocComments"] = json!([{
        "id": "thread-one",
        "comments": [{
            "id": "comment-one",
            "author": 7,
            "body": [{ "textRun": { "content": "Body", "textStyle": {} } }]
        }]
    }]);
    let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
    assert!(
        err.to_string()
            .contains("comment missing required string field author"),
        "{err}"
    );

    let mut input = body.clone();
    input["opendocComments"] = json!([{
        "id": "thread-one",
        "anchor": "bad",
        "comments": [{
            "id": "comment-one",
            "author": "Ada",
            "body": [{ "textRun": { "content": "Body", "textStyle": {} } }]
        }]
    }]);
    let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
    assert!(
        err.to_string().contains("anchor must be an object"),
        "{err}"
    );

    let mut input = body.clone();
    input["opendocComments"] = json!([{
        "id": "thread-one",
        "anchor": { "type": 7 },
        "comments": [{
            "id": "comment-one",
            "author": "Ada",
            "body": [{ "textRun": { "content": "Body", "textStyle": {} } }]
        }]
    }]);
    let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
    assert!(err.to_string().contains("type must be a string"), "{err}");

    let mut input = body.clone();
    input["opendocComments"] = json!([{
        "id": "thread-one",
        "anchor": { "type": "cellRange" },
        "comments": [{
            "id": "comment-one",
            "author": "Ada",
            "body": [{ "textRun": { "content": "Body", "textStyle": {} } }]
        }]
    }]);
    let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
    assert!(
        err.to_string()
            .contains("unsupported anchor type cellRange"),
        "{err}"
    );

    let mut input = body.clone();
    input["opendocComments"] = json!([{
        "id": "thread-one",
        "anchor": { "type": "nearestBlock", "blockId": "block-one", "warning": 7 },
        "comments": [{
            "id": "comment-one",
            "author": "Ada",
            "body": [{ "textRun": { "content": "Body", "textStyle": {} } }]
        }]
    }]);
    let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
    assert!(
        err.to_string().contains("warning must be a string"),
        "{err}"
    );

    let mut input = body.clone();
    input["opendocComments"] = json!([{
        "id": "thread-one",
        "anchor": {
            "type": "nearestBlock",
            "blockId": "block-one",
            "warning": " imported degraded anchor "
        },
        "comments": [{
            "id": "comment-one",
            "author": "Ada",
            "body": [{ "textRun": { "content": "Body", "textStyle": {} } }]
        }]
    }]);
    let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
    assert!(
        err.to_string()
            .contains("nearest block anchor warning has surrounding whitespace"),
        "{err}"
    );

    let mut input = body.clone();
    input["opendocSuggestions"] = json!(["bad"]);
    let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
    assert!(
        err.to_string().contains("suggestion must be an object"),
        "{err}"
    );

    let mut input = body.clone();
    input["opendocSuggestions"] = json!([{
        "id": "suggest-one",
        "author": "Grace",
        "kind": { "type": "delete", "range": { "start": "text-start", "end": "text-end" } },
        "provenance": ["ok", 7]
    }]);
    let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
    assert!(
        err.to_string()
            .contains("suggestion provenance entries must be strings"),
        "{err}"
    );

    let mut input = body.clone();
    input["opendocSuggestions"] = json!([{
        "id": "suggest-one",
        "kind": { "type": "delete", "range": { "start": "text-start", "end": "text-end" } }
    }]);
    let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
    assert!(
        err.to_string()
            .contains("suggestion missing required string field author"),
        "{err}"
    );

    let mut input = body.clone();
    input["opendocSuggestions"] = json!([{
        "id": "suggest-one",
        "author": 7,
        "kind": { "type": "delete", "range": { "start": "text-start", "end": "text-end" } }
    }]);
    let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
    assert!(
        err.to_string()
            .contains("suggestion missing required string field author"),
        "{err}"
    );

    let mut input = body.clone();
    input["opendocSuggestions"] = json!([{
        "id": "suggest-one",
        "author": "Grace",
        "kind": "delete"
    }]);
    let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
    assert!(
        err.to_string()
            .contains("suggestion kind must be an object"),
        "{err}"
    );

    let mut input = body.clone();
    input["opendocSuggestions"] = json!([{
        "id": "suggest-one",
        "author": "Grace",
        "kind": {}
    }]);
    let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
    assert!(
        err.to_string()
            .contains("suggestion kind missing required string field type"),
        "{err}"
    );

    let mut input = body.clone();
    input["opendocSuggestions"] = json!([{
        "id": "suggest-one",
        "author": "Grace",
        "kind": { "type": 7 }
    }]);
    let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
    assert!(
        err.to_string()
            .contains("suggestion kind missing required string field type"),
        "{err}"
    );

    let mut input = body.clone();
    input["opendocSuggestions"] = json!([{
        "id": "suggest-one",
        "author": "Grace",
        "kind": { "type": "delete", "range": { "start": "text-start", "end": "text-end" } },
        "state": 7
    }]);
    let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
    assert!(err.to_string().contains("state must be a string"), "{err}");

    let mut input = body;
    input["opendocSuggestions"] = json!([{
        "id": "suggest-one",
        "author": "Grace",
        "kind": { "type": "delete", "range": { "start": "text-start", "end": "text-end" } },
        "state": "resolved"
    }]);
    let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
    assert!(
        err.to_string()
            .contains("unsupported suggestion state resolved"),
        "{err}"
    );
}
