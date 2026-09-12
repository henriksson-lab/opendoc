use crate::*;
use opendoc_core::{Inline, MarkKind};
use serde_json::json;

#[test]
fn imports_google_docs_footnotes_as_document_local_source() {
    let input = json!({
        "body": { "content": [{
            "paragraph": { "elements": [
                { "textRun": { "content": "Text", "textStyle": {} } },
                { "footnoteReference": { "footnoteId": " fn-1 ", "footnoteNumber": "1" } }
            ] }
        }] },
        "footnotes": {
            "fn-1": {
                "footnoteId": " fn-1 ",
                "content": [{
                    "paragraph": { "elements": [{
                        "textRun": { "content": "Footnote body", "textStyle": { "italic": true } }
                    }] }
                }]
            }
        }
    });
    let report = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap();
    assert_eq!(report.document.footnotes.len(), 1);
    assert_eq!(report.document.footnotes[0].id.as_str(), "fn-1");
    match &report.document.blocks[0].content[1] {
        Inline::FootnoteRef { footnote_id, .. } => assert_eq!(footnote_id.as_str(), "fn-1"),
        other => panic!("expected footnote ref, got {other:?}"),
    }
    match &report.document.footnotes[0].body[0] {
        Inline::Text { text, marks, .. } => {
            assert_eq!(text, "Footnote body");
            assert!(marks.iter().any(|mark| mark.kind == MarkKind::Italic));
        }
        other => panic!("expected footnote text body, got {other:?}"),
    }
}

#[test]
fn malformed_google_docs_footnote_source_aborts() {
    let base = json!({
        "body": { "content": [{
            "paragraph": { "elements": [
                { "textRun": { "content": "Text", "textStyle": {} } },
                { "footnoteReference": { "footnoteId": "fn-1", "footnoteNumber": "1" } }
            ] }
        }] },
        "footnotes": {
            "fn-1": {
                "footnoteId": "fn-1",
                "content": [{
                    "paragraph": { "elements": [{
                        "textRun": { "content": "Footnote body", "textStyle": {} }
                    }] }
                }]
            }
        }
    });

    let mut input = base.clone();
    input["footnotes"] = json!("bad");
    let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
    assert!(
        err.to_string().contains("footnotes must be an object"),
        "{err}"
    );

    let mut input = base.clone();
    input["footnotes"]["fn-1"] = json!("bad");
    let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
    assert!(
        err.to_string().contains("footnote must be an object"),
        "{err}"
    );

    let mut input = base.clone();
    input["footnotes"]["fn-1"]["footnoteId"] = json!(7);
    let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
    assert!(
        err.to_string().contains("footnoteId must be a string"),
        "{err}"
    );

    let mut input = base.clone();
    input["footnotes"]["fn-1"]["content"] = json!("bad");
    let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
    assert!(
        err.to_string()
            .contains("footnote missing required array field content"),
        "{err}"
    );

    let mut input = base.clone();
    input["footnotes"]["fn-1"]["content"] = json!(["bad"]);
    let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
    assert!(
        err.to_string()
            .contains("footnote content element must be an object"),
        "{err}"
    );

    let mut input = base;
    input["footnotes"]["fn-1"]["content"] = json!([{ "table": {} }]);
    let err = import_google_docs_json("Google", input.to_string().as_bytes()).unwrap_err();
    assert!(
        err.to_string()
            .contains("only paragraph footnote content is supported"),
        "{err}"
    );
}
