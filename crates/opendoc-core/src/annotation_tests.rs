use crate::*;

#[test]
fn footnote_references_require_live_targets() {
    let footnote_id = StableId::parse("footnote-1").unwrap();
    let mut doc = Document::new("Footnotes");
    doc.footnotes.push(Footnote {
        id: footnote_id.clone(),
        revision: 1,
        body: vec![Inline::text("footnote body")],
        deleted: false,
    });
    doc.blocks.push(Block {
        id: StableId::new("table"),
        kind: BlockKind::Table {
            columns: vec![TableColumn::auto()],
            rows: vec![TableRow {
                id: StableId::new("row"),
                cells: vec![TableCell {
                    id: StableId::new("cell"),
                    span: CellSpan::SINGLE,
                    properties: TableCellProperties::default(),
                    blocks: vec![Block {
                        id: StableId::new("cell-block"),
                        kind: BlockKind::Paragraph,
                        content: vec![Inline::FootnoteRef {
                            id: StableId::new("footnote-ref"),
                            footnote_id: footnote_id.clone(),
                        }],
                        properties: BlockProperties::default(),
                    }],
                }],
            }],
        },
        content: Vec::new(),
        properties: BlockProperties::default(),
    });
    doc.validate().unwrap();

    doc.footnotes[0].deleted = true;
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "footnote reference target is missing"
        ))
    ));
}

#[test]
fn footnotes_require_non_empty_source_body() {
    let mut doc = Document::new("Footnotes");
    doc.footnotes.push(Footnote {
        id: StableId::parse("footnote-empty").unwrap(),
        revision: 1,
        body: vec![Inline::text(" ")],
        deleted: false,
    });

    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument("footnote body is empty"))
    ));
}

#[test]
fn missing_footnote_reference_target_is_invalid() {
    let mut doc = Document::new("Footnotes");
    doc.blocks.push(Block {
        id: StableId::new("block"),
        kind: BlockKind::Paragraph,
        content: vec![Inline::FootnoteRef {
            id: StableId::new("footnote-ref"),
            footnote_id: StableId::parse("missing-footnote").unwrap(),
        }],
        properties: BlockProperties::default(),
    });

    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "footnote reference target is missing"
        ))
    ));
}

#[test]
fn comment_threads_require_auditable_comments() {
    let mut doc = Document::new("Comments");
    doc.blocks.push(Block::paragraph("body"));
    doc.comments.push(CommentThread {
        id: StableId::parse("comment-thread").unwrap(),
        anchor: Anchor::Document,
        comments: vec![Comment {
            id: StableId::parse("comment-1").unwrap(),
            author: "Reviewer".to_string(),
            body: vec![Inline::text("Review note")],
            created_at_ms: 1,
            deleted: false,
        }],
        deleted: false,
    });
    doc.validate().unwrap();

    doc.comments[0].comments[0].author = " ".to_string();
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument("comment author is empty"))
    ));
    doc.comments[0].comments[0].author = "Reviewer".to_string();
    doc.comments[0].comments[0].body.clear();
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument("comment body is empty"))
    ));
    doc.comments[0].comments[0].body = vec![Inline::text(" ")];
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument("comment body is empty"))
    ));
}

#[test]
fn comment_threads_reject_duplicate_comment_ids() {
    let mut doc = Document::new("Comments");
    let comment_id = StableId::parse("comment-1").unwrap();
    doc.comments.push(CommentThread {
        id: StableId::parse("comment-thread").unwrap(),
        anchor: Anchor::Document,
        comments: vec![
            Comment {
                id: comment_id.clone(),
                author: "Reviewer".to_string(),
                body: vec![Inline::text("First")],
                created_at_ms: 1,
                deleted: false,
            },
            Comment {
                id: comment_id,
                author: "Reviewer".to_string(),
                body: vec![Inline::text("Second")],
                created_at_ms: 2,
                deleted: true,
            },
        ],
        deleted: false,
    });

    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument("duplicate comment id"))
    ));
}

#[test]
fn comment_and_suggestion_anchors_require_auditable_payloads() {
    let mut doc = Document::new("Anchors");
    doc.blocks.push(Block::paragraph("body"));
    doc.comments.push(CommentThread {
        id: StableId::parse("comment-thread").unwrap(),
        anchor: Anchor::NearestBlock {
            block_id: StableId::parse("block-retained").unwrap(),
            warning: "anchor moved after deletion".to_string(),
        },
        comments: vec![Comment {
            id: StableId::parse("comment-1").unwrap(),
            author: "Reviewer".to_string(),
            body: vec![Inline::text("Review note")],
            created_at_ms: 1,
            deleted: false,
        }],
        deleted: false,
    });
    doc.validate().unwrap();

    doc.comments[0].comments[0].author = " Reviewer ".to_string();
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "comment author has surrounding whitespace"
        ))
    ));
    doc.comments[0].comments[0].author = "Reviewer".to_string();

    if let Anchor::NearestBlock { warning, .. } = &mut doc.comments[0].anchor {
        warning.clear();
    }
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "nearest block anchor warning is empty"
        ))
    ));

    if let Anchor::NearestBlock { warning, .. } = &mut doc.comments[0].anchor {
        *warning = " moved after delete ".to_string();
    }
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "nearest block anchor warning has surrounding whitespace"
        ))
    ));

    doc.comments.clear();
    doc.suggestions.push(Suggestion {
        id: StableId::parse("suggestion-1").unwrap(),
        author: "Reviewer".to_string(),
        kind: SuggestionKind::Delete {
            range: TextRange {
                start: StableId(String::new()),
                end: StableId::parse("text-end").unwrap(),
            },
        },
        state: SuggestionState::Proposed,
        provenance: Vec::new(),
    });
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument("text range start is empty"))
    ));

    doc.suggestions[0].kind = SuggestionKind::Insert {
        anchor: Anchor::NearestBlock {
            block_id: StableId(String::new()),
            warning: "anchor moved after deletion".to_string(),
        },
        content: vec![Inline::text("inserted")],
    };
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "nearest block anchor block id is empty"
        ))
    ));
}

#[test]
fn suggestions_require_auditable_payloads() {
    let mut doc = Document::new("Suggestions");
    doc.suggestions.push(Suggestion {
        id: StableId::parse("suggestion-1").unwrap(),
        author: "Reviewer".to_string(),
        kind: SuggestionKind::Insert {
            anchor: Anchor::Document,
            content: Vec::new(),
        },
        state: SuggestionState::Proposed,
        provenance: Vec::new(),
    });
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "insert suggestion content is empty"
        ))
    ));
    doc.suggestions[0].kind = SuggestionKind::Insert {
        anchor: Anchor::Document,
        content: vec![Inline::text(" ")],
    };
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "insert suggestion content is empty"
        ))
    ));

    doc.suggestions[0].kind = SuggestionKind::Format {
        range: TextRange {
            start: StableId::parse("text-1").unwrap(),
            end: StableId::parse("text-1").unwrap(),
        },
        marks: Vec::new(),
    };
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "format suggestion marks are empty"
        ))
    ));

    doc.suggestions[0].kind = SuggestionKind::Insert {
        anchor: Anchor::Document,
        content: vec![Inline::text("suggested text")],
    };
    doc.suggestions[0].provenance = vec![" ".to_string()];
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "suggestion provenance entry is empty"
        ))
    ));

    doc.suggestions[0].provenance = vec![" imported ".to_string()];
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "suggestion provenance entry has surrounding whitespace"
        ))
    ));

    doc.suggestions[0].provenance = Vec::new();
    doc.suggestions[0].author = " Reviewer ".to_string();
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "suggestion author has surrounding whitespace"
        ))
    ));
}
