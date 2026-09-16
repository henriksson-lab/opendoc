use crate::*;

#[test]
fn legacy_text_sequence_has_deterministic_token_gaps_and_retains_tombstones() {
    let uuid = DocumentUuid::parse("doc-sequence").unwrap();
    let inline = StableId::parse("inline-sequence").unwrap();
    let mut sequence = TextSequence::materialize_legacy(&uuid, &inline, "a😀b");
    sequence.validate().unwrap();
    assert_eq!(sequence.visible_text(), "a😀b");

    let gap = sequence.gap_at_visible_offset(2, TextGapBias::After);
    assert_eq!(sequence.visible_offset_of_gap(&gap), Some(2));
    sequence.tokens[1].tombstoned = true;
    assert_eq!(sequence.visible_text(), "ab");
    // The old gap remains at a truthful boundary rather than being rewritten
    // to a neighbour as an offset anchor would be.
    assert_eq!(sequence.visible_offset_of_gap(&gap), Some(1));

    let same = TextSequence::materialize_legacy(&uuid, &inline, "a😀b");
    assert_eq!(same.tokens[1].id, sequence.tokens[1].id);
}

#[test]
fn text_sequence_rejects_duplicate_and_forward_predecessor_tokens() {
    let uuid = DocumentUuid::parse("doc-sequence-invalid").unwrap();
    let inline = StableId::parse("inline-sequence-invalid").unwrap();
    let mut sequence = TextSequence::materialize_legacy(&uuid, &inline, "ab");
    sequence.tokens[1].id = sequence.tokens[0].id.clone();
    assert!(matches!(
        sequence.validate(),
        Err(ModelError::InvalidDocument("duplicate text token id"))
    ));

    let mut sequence = TextSequence::materialize_legacy(&uuid, &inline, "ab");
    sequence.tokens[0].predecessor = Some(sequence.tokens[1].id.clone());
    assert!(matches!(
        sequence.validate(),
        Err(ModelError::InvalidDocument(
            "text token predecessor is not earlier in sequence"
        ))
    ));
}

#[test]
fn structured_nodes_reject_empty_or_invalid_payloads() {
    let mut doc = Document::new("Structured payloads");
    doc.blocks.push(Block {
        id: StableId::parse("link-block").unwrap(),
        kind: BlockKind::Paragraph,
        content: vec![Inline::Link {
            id: StableId::parse("link-empty").unwrap(),
            text: "link".to_string(),
            href: String::new(),
            marks: Vec::new(),
        }],
        properties: BlockProperties::default(),
    });
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument("link href is empty"))
    ));

    doc.blocks[0].content = vec![Inline::Mention {
        id: StableId::parse("mention-empty").unwrap(),
        label: " ".to_string(),
    }];
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument("mention label is empty"))
    ));

    doc.blocks[0].content = vec![Inline::Equation {
        id: StableId::parse("inline-equation-empty").unwrap(),
        equation: Equation {
            id: StableId::parse("equation-empty").unwrap(),
            source_format: EquationSourceFormat::LatexLike,
            source: String::new(),
        },
    }];
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument("equation source is empty"))
    ));

    doc.blocks[0].content = vec![Inline::Equation {
        id: StableId::parse("inline-equation-padded").unwrap(),
        equation: Equation {
            id: StableId::parse("equation-padded").unwrap(),
            source_format: EquationSourceFormat::LatexLike,
            source: " x=1 ".to_string(),
        },
    }];
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "equation source has surrounding whitespace"
        ))
    ));

    doc.blocks[0] = Block {
        id: StableId::parse("block-equation-empty").unwrap(),
        kind: BlockKind::EquationBlock {
            equation: Equation {
                id: StableId::parse("block-equation").unwrap(),
                source_format: EquationSourceFormat::LatexLike,
                source: " ".to_string(),
            },
        },
        content: Vec::new(),
        properties: BlockProperties::default(),
    };
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument("equation source is empty"))
    ));

    doc.blocks[0] = Block {
        id: StableId::parse("block-equation-padded").unwrap(),
        kind: BlockKind::EquationBlock {
            equation: Equation {
                id: StableId::parse("block-equation-padded-source").unwrap(),
                source_format: EquationSourceFormat::LatexLike,
                source: " y=1 ".to_string(),
            },
        },
        content: Vec::new(),
        properties: BlockProperties::default(),
    };
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "equation source has surrounding whitespace"
        ))
    ));

    doc.blocks[0] = Block {
        id: StableId::parse("image-bad-hash").unwrap(),
        kind: BlockKind::Image {
            blob_hash: "not-a-hash".to_string(),
            alt_text: "image".to_string(),
            layout: ImageLayout::default(),
        },
        content: Vec::new(),
        properties: BlockProperties::default(),
    };
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument("image blob hash is invalid"))
    ));

    doc.blocks[0] = Block {
        id: StableId::parse("image-padded-hash").unwrap(),
        kind: BlockKind::Image {
            blob_hash: " sha256:abc ".to_string(),
            alt_text: "image".to_string(),
            layout: ImageLayout::default(),
        },
        content: Vec::new(),
        properties: BlockProperties::default(),
    };
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument("image blob hash is invalid"))
    ));

    doc.blocks[0] = Block {
        id: StableId::parse("heading-bad-level").unwrap(),
        kind: BlockKind::Heading { level: 0 },
        content: vec![Inline::text("bad heading")],
        properties: BlockProperties::default(),
    };
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "heading level is outside 1..=6"
        ))
    ));

    doc.blocks[0] = Block {
        id: StableId::parse("list-bad-level").unwrap(),
        kind: BlockKind::ListItem {
            list_id: new_list_id(),
            level: 9,
            kind: ListKind::Bullet,
        },
        content: vec![Inline::text("bad list item")],
        properties: BlockProperties::default(),
    };
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "list item level is outside 0..=8"
        ))
    ));
}

#[test]
fn dropdown_requires_a_canonical_selected_option() {
    let dropdown = Inline::Dropdown {
        id: StableId::parse("status").unwrap(),
        options: vec![DropdownOption {
            id: "draft".to_string(),
            label: "Draft".to_string(),
        }],
        selected_option_id: "published".to_string(),
    };
    assert!(matches!(
        dropdown.validate(),
        Err(ModelError::InvalidDocument(
            "dropdown selected option is absent"
        ))
    ));
}

#[test]
fn mark_payloads_require_consistent_values() {
    let mut doc = Document::new("Marks");
    doc.blocks.push(Block::paragraph("marked"));
    let Inline::Text { marks, .. } = &mut doc.blocks[0].content[0] else {
        unreachable!();
    };
    marks.push(Mark {
        kind: MarkKind::Color,
        value: None,
        expand: MarkExpand::Both,
    });
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument("mark value is missing"))
    ));

    let Inline::Text { marks, .. } = &mut doc.blocks[0].content[0] else {
        unreachable!();
    };
    marks[0].value = Some(" ".to_string());
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument("mark value is empty"))
    ));

    let Inline::Text { marks, .. } = &mut doc.blocks[0].content[0] else {
        unreachable!();
    };
    marks[0] = Mark {
        kind: MarkKind::Bold,
        value: Some("true".to_string()),
        expand: MarkExpand::Both,
    };
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument("boolean mark has value"))
    ));

    let Inline::Text { marks, .. } = &mut doc.blocks[0].content[0] else {
        unreachable!();
    };
    marks[0] = Mark {
        kind: MarkKind::Font,
        value: Some("Inter".to_string()),
        expand: MarkExpand::Both,
    };
    doc.validate().unwrap();
}

#[test]
fn retained_inline_bodies_use_same_payload_validation() {
    let mut doc = Document::new("Retained bodies");
    doc.comments.push(CommentThread {
        id: StableId::parse("thread-1").unwrap(),
        anchor: Anchor::Document,
        comments: vec![Comment {
            id: StableId::parse("comment-1").unwrap(),
            author: "Reviewer".to_string(),
            body: vec![Inline::Mention {
                id: StableId::parse("mention-empty").unwrap(),
                label: String::new(),
            }],
            created_at_ms: 1,
            deleted: false,
        }],
        state: CommentThreadState::Open,
        resolved_by: None,
        resolved_at_ms: None,
        action_assignee: None,
        action_due_at_ms: None,
        action_completed_by: None,
        action_completed_at_ms: None,
        reactions: Vec::new(),
        deleted: false,
    });
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument("mention label is empty"))
    ));

    doc.comments.clear();
    doc.suggestions.push(Suggestion {
        id: StableId::parse("suggestion-1").unwrap(),
        author: "Reviewer".to_string(),
        kind: SuggestionKind::Insert {
            anchor: Anchor::Document,
            content: vec![Inline::Equation {
                id: StableId::parse("inline-equation-empty").unwrap(),
                equation: Equation {
                    id: StableId::parse("equation-empty").unwrap(),
                    source_format: EquationSourceFormat::LatexLike,
                    source: String::new(),
                },
            }],
        },
        state: SuggestionState::Proposed,
        provenance: Vec::new(),
    });
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument("equation source is empty"))
    ));

    doc.suggestions[0].kind = SuggestionKind::Format {
        range: TextRange {
            start: StableId::parse("text-a").unwrap(),
            end: StableId::parse("text-b").unwrap(),
        },
        marks: vec![Mark {
            kind: MarkKind::Bold,
            value: Some("true".to_string()),
            expand: MarkExpand::Both,
        }],
    };
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument("boolean mark has value"))
    ));
}

#[test]
fn retained_record_collections_reject_duplicate_ids() {
    let mut doc = Document::new("Duplicate retained IDs");
    let footnote_id = StableId::parse("footnote-duplicate").unwrap();
    doc.footnotes.push(Footnote {
        id: footnote_id.clone(),
        revision: 1,
        body: vec![Inline::text("first")],
        deleted: true,
    });
    doc.footnotes.push(Footnote {
        id: footnote_id,
        revision: 2,
        body: vec![Inline::text("second")],
        deleted: true,
    });
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument("duplicate footnote id"))
    ));

    doc.footnotes.clear();
    let thread_id = StableId::parse("comment-thread-duplicate").unwrap();
    for body in ["first", "second"] {
        doc.comments.push(CommentThread {
            id: thread_id.clone(),
            anchor: Anchor::Document,
            comments: vec![Comment {
                id: StableId::new("comment"),
                author: "Reviewer".to_string(),
                body: vec![Inline::text(body)],
                created_at_ms: 1,
                deleted: false,
            }],
            state: CommentThreadState::Open,
            resolved_by: None,
            resolved_at_ms: None,
            action_assignee: None,
            action_due_at_ms: None,
            action_completed_by: None,
            action_completed_at_ms: None,
            reactions: Vec::new(),
            deleted: false,
        });
    }
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument("duplicate comment thread id"))
    ));

    doc.comments.clear();
    let suggestion_id = StableId::parse("suggestion-duplicate").unwrap();
    for text in ["first", "second"] {
        doc.suggestions.push(Suggestion {
            id: suggestion_id.clone(),
            author: "Reviewer".to_string(),
            kind: SuggestionKind::Insert {
                anchor: Anchor::Document,
                content: vec![Inline::text(text)],
            },
            state: SuggestionState::Proposed,
            provenance: Vec::new(),
        });
    }
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument("duplicate suggestion id"))
    ));
}
