//! Suggestion tests.

use crate::causal::{ActorId, OperationId};
use crate::inline_ops::inline_id;
use crate::merge::merge_operations;
use crate::operation::{BlockTextStyle, Operation, OperationKind};
use crate::{preview_suggestion_resolution, SuggestionPreviewResolution};
use opendoc_core::{
    Anchor, Block, BlockKind, BlockProperties, Document, Equation, EquationSourceFormat, Inline,
    InsertPosition, Mark, MarkExpand, MarkKind, ParagraphStyle, StableId, Suggestion,
    SuggestionKind, SuggestionState, TableRow, TextRange,
};

#[test]
fn block_delete_suggestion_rejects_the_sole_block_of_a_table_cell() {
    let mut base = Document::new("Table review");
    let mut row = TableRow::empty(1);
    row.cells[0].blocks[0] = Block::paragraph("cell text");
    let cell_block_id = row.cells[0].blocks[0].id.clone();
    base.blocks.push(Block {
        id: StableId::parse("table-review").unwrap(),
        kind: BlockKind::table(vec![row]),
        content: Vec::new(),
        properties: BlockProperties::default(),
    });
    let suggestion_id = StableId::parse("delete-cell-block").unwrap();
    let suggestion = Suggestion {
        id: suggestion_id.clone(),
        author: "Ada".to_string(),
        kind: SuggestionKind::BlockDelete {
            block_id: cell_block_id.clone(),
        },
        state: SuggestionState::Proposed,
        provenance: Vec::new(),
    };

    let result = merge_operations(
        &base,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("Ada".to_string()),
                seq: 1,
            },
            kind: OperationKind::AddSuggestion { suggestion },
            context: None,
        }]],
    )
    .expect("unfulfillable structural review proposal stays valid");

    assert_eq!(
        result.document.suggestions[0].state,
        SuggestionState::Rejected
    );
    assert!(result.document.suggestions[0]
        .provenance
        .iter()
        .any(|entry| entry == "auto-rejected:table-cell-requires-block"));
    assert!(result
        .warnings
        .iter()
        .any(|warning| warning.code == "table-cell-requires-block"));
    assert!(matches!(
        &result.document.blocks[0].kind,
        BlockKind::Table { rows, .. } if rows[0].cells[0].blocks[0].id == cell_block_id
    ));
    result
        .document
        .validate()
        .expect("table cell remains valid");
}

#[test]
fn stale_paragraph_style_suggestion_auto_rejects_after_a_source_style_change() {
    let mut base = Document::new("Styles");
    let paragraph = Block::paragraph("source text");
    let block_id = paragraph.id.clone();
    base.blocks.push(paragraph);
    let suggestion_id = StableId::parse("paragraph-style-suggestion").unwrap();
    base.suggestions.push(Suggestion {
        id: suggestion_id.clone(),
        author: "Ada".to_string(),
        kind: SuggestionKind::ParagraphStyleChange {
            block_id: block_id.clone(),
            expected: ParagraphStyle::Paragraph,
            proposed: ParagraphStyle::Heading { level: 2 },
        },
        state: SuggestionState::Proposed,
        provenance: Vec::new(),
    });

    let result = merge_operations(
        &base,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("editor".to_string()),
                seq: 1,
            },
            kind: OperationKind::SetBlockTextStyle {
                block_id,
                style: BlockTextStyle::Title,
            },
            context: None,
        }]],
    )
    .expect("style update merges");

    let suggestion = &result.document.suggestions[0];
    assert_eq!(suggestion.state, SuggestionState::Rejected);
    assert!(suggestion
        .provenance
        .iter()
        .any(|entry| entry == "auto-rejected:source-style-mismatch"));
    assert!(result
        .warnings
        .iter()
        .any(|warning| warning.code == "suggestion-paragraph-style-mismatch"));
    assert!(matches!(
        result.document.blocks[0].kind,
        opendoc_core::BlockKind::Title
    ));
}

#[test]
fn preview_resolution_uses_accept_semantics_without_mutating_source() {
    let mut source = Document::new("Preview");
    let paragraph = Block::paragraph("before");
    let anchor = match &paragraph.content[0] {
        Inline::Text { id, .. } => id.clone(),
        other => panic!("paragraph constructor made unexpected inline {other:?}"),
    };
    source.blocks.push(paragraph);
    let suggestion_id = StableId::new("preview-insert");
    source.suggestions.push(Suggestion {
        id: suggestion_id.clone(),
        author: "Alice".to_string(),
        kind: SuggestionKind::Insert {
            anchor: Anchor::TextRange(TextRange {
                start: anchor.clone(),
                end: anchor,
            }),
            content: vec![Inline::text(" after")],
        },
        state: SuggestionState::Proposed,
        provenance: Vec::new(),
    });

    let (accepted, warnings) =
        preview_suggestion_resolution(&source, &suggestion_id, SuggestionPreviewResolution::Accept);
    assert!(warnings.is_empty());
    assert_eq!(accepted.visible_text(), "before after\n");
    assert_eq!(accepted.suggestions[0].state, SuggestionState::Accepted);
    assert_eq!(source.visible_text(), "before\n");
    assert_eq!(source.suggestions[0].state, SuggestionState::Proposed);

    let (rejected, warnings) =
        preview_suggestion_resolution(&source, &suggestion_id, SuggestionPreviewResolution::Reject);
    assert!(warnings.is_empty());
    assert_eq!(rejected.visible_text(), "before\n");
    assert_eq!(rejected.suggestions[0].state, SuggestionState::Rejected);
}

#[test]
fn structural_preview_keeps_identity_bound_acceptance_and_source_is_unchanged() {
    let mut source = Document::new("Preview");
    let paragraph = Block::paragraph("survives in source");
    let block_id = paragraph.id.clone();
    source.blocks.push(paragraph);
    let suggestion_id = StableId::new("preview-block-delete");
    source.suggestions.push(Suggestion {
        id: suggestion_id.clone(),
        author: "Alice".to_string(),
        kind: SuggestionKind::BlockDelete {
            block_id: block_id.clone(),
        },
        state: SuggestionState::Proposed,
        provenance: Vec::new(),
    });

    let (preview, warnings) =
        preview_suggestion_resolution(&source, &suggestion_id, SuggestionPreviewResolution::Accept);
    assert!(warnings.is_empty());
    assert!(preview.blocks.is_empty());
    assert_eq!(preview.suggestions[0].state, SuggestionState::Accepted);
    assert_eq!(source.blocks[0].id, block_id);
    assert_eq!(source.suggestions[0].state, SuggestionState::Proposed);
}

#[test]
fn suggestions_and_atomic_equations_converge() {
    let mut base = Document::new("Doc");
    let block = Block::paragraph("hello");
    let block_id = block.id.clone();
    base.blocks.push(block);
    let equation = Inline::Equation {
        id: StableId::new("eq-inline"),
        equation: Equation {
            id: StableId::new("eq"),
            source_format: EquationSourceFormat::LatexLike,
            source: "E=mc^2".to_string(),
        },
    };
    let suggestion_id = StableId::new("suggestion");
    let suggestion = Suggestion {
        id: suggestion_id.clone(),
        author: "Bob".to_string(),
        kind: SuggestionKind::Format {
            range: TextRange {
                start: StableId::parse("x").unwrap(),
                end: StableId::parse("y").unwrap(),
            },
            marks: vec![Mark {
                kind: MarkKind::Italic,
                value: None,
                expand: MarkExpand::Both,
            }],
        },
        state: SuggestionState::Proposed,
        provenance: Vec::new(),
    };
    let ops = vec![
        Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::InsertInline {
                block_id,
                position: InsertPosition::Last,
                inline: equation,
            },
            context: None,
        },
        Operation {
            id: OperationId {
                actor: ActorId("b".to_string()),
                seq: 1,
            },
            kind: OperationKind::AddSuggestion { suggestion },
            context: None,
        },
        Operation {
            id: OperationId {
                actor: ActorId("c".to_string()),
                seq: 1,
            },
            kind: OperationKind::AcceptSuggestion {
                suggestion_id,
                accepted_by: "Carol".to_string(),
            },
            context: None,
        },
    ];
    let result = merge_operations(&base, &[ops]).unwrap();
    assert!(result.document.visible_text().contains("E=mc^2"));
    assert_eq!(
        result.document.suggestions[0].state,
        SuggestionState::Accepted
    );
    assert_eq!(
        result.document.suggestions[0].provenance,
        vec!["accepted-by:Carol"]
    );
}

#[test]
fn duplicate_suggestion_add_degrades_to_warning() {
    let mut base = Document::new("Doc");
    base.blocks.push(Block::paragraph("hello"));
    let suggestion_id = StableId::parse("suggestion-duplicate-merge").unwrap();
    let suggestion = |author: &str, text: &str| Suggestion {
        id: suggestion_id.clone(),
        author: author.to_string(),
        kind: SuggestionKind::Insert {
            anchor: Anchor::Document,
            content: vec![Inline::text(text)],
        },
        state: SuggestionState::Proposed,
        provenance: Vec::new(),
    };
    let first = Operation {
        id: OperationId {
            actor: ActorId("a".to_string()),
            seq: 1,
        },
        kind: OperationKind::AddSuggestion {
            suggestion: suggestion("Alice", "first"),
        },
        context: None,
    };
    let second = Operation {
        id: OperationId {
            actor: ActorId("b".to_string()),
            seq: 1,
        },
        kind: OperationKind::AddSuggestion {
            suggestion: suggestion("Bob", "second"),
        },
        context: None,
    };

    let result = merge_operations(&base, &[vec![first], vec![second]]).unwrap();
    assert_eq!(result.document.suggestions.len(), 1);
    assert!(result
        .warnings
        .iter()
        .any(|warning| warning.code == "duplicate-suggestion"));
}

#[test]
fn accepting_insert_suggestion_applies_content_after_anchor() {
    let mut base = Document::new("Doc");
    let block = Block::paragraph("hello");
    let text_id = match &block.content[0] {
        Inline::Text { id, .. } => id.clone(),
        _ => unreachable!(),
    };
    base.blocks.push(block);
    let suggestion_id = StableId::parse("suggestion-insert-accept").unwrap();
    let inserted = Inline::Text {
        id: StableId::parse("text-accepted-insert").unwrap(),
        text: " accepted".to_string(),
        marks: Vec::new(),
    };

    let result = merge_operations(
        &base,
        &[vec![
            Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::AddSuggestion {
                    suggestion: Suggestion {
                        id: suggestion_id.clone(),
                        author: "Bob".to_string(),
                        kind: SuggestionKind::Insert {
                            anchor: Anchor::TextRange(TextRange {
                                start: text_id.clone(),
                                end: text_id,
                            }),
                            content: vec![inserted],
                        },
                        state: SuggestionState::Proposed,
                        provenance: Vec::new(),
                    },
                },
                context: None,
            },
            Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 2,
                },
                kind: OperationKind::AcceptSuggestion {
                    suggestion_id: suggestion_id.clone(),
                    accepted_by: "Alice".to_string(),
                },
                context: None,
            },
        ]],
    )
    .unwrap();

    assert_eq!(result.document.visible_text(), "hello accepted\n");
    assert_eq!(
        result.document.suggestions[0].state,
        SuggestionState::Accepted
    );
    assert_eq!(
        result.document.suggestions[0].provenance,
        vec!["accepted-by:Alice"]
    );
}

#[test]
fn concurrent_suggestion_add_and_accept_converge_when_accept_sorts_first() {
    let mut base = Document::new("Doc");
    base.blocks.push(Block::paragraph("hello"));
    let suggestion_id = StableId::parse("suggestion-add-accept-race").unwrap();
    let suggestion = Suggestion {
        id: suggestion_id.clone(),
        author: "Bob".to_string(),
        kind: SuggestionKind::Insert {
            anchor: Anchor::Document,
            content: vec![Inline::text(" accepted proposal")],
        },
        state: SuggestionState::Proposed,
        provenance: Vec::new(),
    };
    let accept = Operation {
        id: OperationId {
            actor: ActorId("actor-a".to_string()),
            seq: 1,
        },
        kind: OperationKind::AcceptSuggestion {
            suggestion_id: suggestion_id.clone(),
            accepted_by: "Alice".to_string(),
        },
        context: None,
    };
    let add = Operation {
        id: OperationId {
            actor: ActorId("actor-z".to_string()),
            seq: 1,
        },
        kind: OperationKind::AddSuggestion { suggestion },
        context: None,
    };

    let accept_first = merge_operations(&base, &[vec![accept.clone()], vec![add.clone()]]).unwrap();
    let add_first = merge_operations(&base, &[vec![add], vec![accept]]).unwrap();

    assert_eq!(accept_first.document, add_first.document);
    assert_eq!(accept_first.warnings, add_first.warnings);
    assert_eq!(
        accept_first.document.suggestions[0].state,
        SuggestionState::Accepted
    );
    assert_eq!(
        accept_first.document.suggestions[0].provenance,
        vec!["accepted-by:Alice"]
    );
    assert!(accept_first
        .document
        .visible_text()
        .contains("accepted proposal"));
    assert!(accept_first.warnings.is_empty());
}

#[test]
fn accepting_insert_suggestion_with_deleted_range_end_degrades_to_start() {
    let mut base = Document::new("Doc");
    let mut block = Block::paragraph("");
    block.content.clear();
    let start = Inline::text("start");
    let start_id = inline_id(&start).clone();
    let end = Inline::text(" end");
    let end_id = inline_id(&end).clone();
    block.content.push(start);
    block.content.push(end);
    base.blocks.push(block);

    let suggestion_id = StableId::parse("suggestion-insert-partial-anchor").unwrap();
    base.suggestions.push(Suggestion {
        id: suggestion_id.clone(),
        author: "Reviewer".to_string(),
        kind: SuggestionKind::Insert {
            anchor: Anchor::TextRange(TextRange {
                start: start_id,
                end: end_id.clone(),
            }),
            content: vec![Inline::text(" inserted")],
        },
        state: SuggestionState::Proposed,
        provenance: Vec::new(),
    });

    let delete_end = Operation {
        id: OperationId {
            actor: ActorId("a".to_string()),
            seq: 1,
        },
        kind: OperationKind::DeleteInline { inline_id: end_id },
        context: None,
    };
    let accept = Operation {
        id: OperationId {
            actor: ActorId("b".to_string()),
            seq: 1,
        },
        kind: OperationKind::AcceptSuggestion {
            suggestion_id,
            accepted_by: "Alice".to_string(),
        },
        context: None,
    };

    let actor_streams =
        merge_operations(&base, &[vec![delete_end.clone()], vec![accept.clone()]]).unwrap();
    let storage_batch =
        merge_operations(&base, &[vec![delete_end.clone(), accept.clone()], vec![]]).unwrap();
    let reversed_batches = merge_operations(&base, &[vec![accept], vec![delete_end]]).unwrap();

    assert_eq!(actor_streams.document, storage_batch.document);
    assert_eq!(actor_streams.document, reversed_batches.document);
    assert_eq!(actor_streams.document.visible_text(), "start inserted\n");
    assert_eq!(
        actor_streams.document.suggestions[0].state,
        SuggestionState::Accepted
    );
    assert!(actor_streams
        .warnings
        .iter()
        .any(|warning| warning.code == "suggestion-anchor-degraded"));
}

#[test]
fn accepting_invalid_insert_suggestion_degrades_without_mutating_source() {
    let mut base = Document::new("Doc");
    base.blocks.push(Block::paragraph("hello"));
    let suggestion_id = StableId::parse("suggestion-insert-invalid-accept").unwrap();
    base.suggestions.push(Suggestion {
        id: suggestion_id.clone(),
        author: "Bob".to_string(),
        kind: SuggestionKind::Insert {
            anchor: Anchor::Document,
            content: vec![Inline::Link {
                id: StableId::parse("invalid-accepted-link").unwrap(),
                text: "bad link".to_string(),
                href: String::new(),
                marks: Vec::new(),
            }],
        },
        state: SuggestionState::Proposed,
        provenance: Vec::new(),
    });

    let result = merge_operations(
        &base,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::AcceptSuggestion {
                suggestion_id: suggestion_id.clone(),
                accepted_by: "Alice".to_string(),
            },
            context: None,
        }]],
    )
    .unwrap();

    assert_eq!(result.document.visible_text(), "hello\n");
    assert_eq!(result.warnings[0].code, "invalid-link-href");
    assert_eq!(
        result.document.suggestions[0].state,
        SuggestionState::Rejected
    );
    assert_eq!(
        result.document.suggestions[0].provenance,
        vec!["auto-rejected:invalid-accept-payload"]
    );
}

#[test]
fn accepting_delete_suggestion_removes_source_range() {
    let mut base = Document::new("Doc");
    base.blocks.push(Block::paragraph("alpha "));
    let second = Inline::Text {
        id: StableId::parse("text-delete-target").unwrap(),
        text: "beta".to_string(),
        marks: Vec::new(),
    };
    let second_id = inline_id(&second).clone();
    base.blocks[0].content.push(second);
    let suggestion_id = StableId::parse("suggestion-delete-accept").unwrap();
    base.suggestions.push(Suggestion {
        id: suggestion_id.clone(),
        author: "Bob".to_string(),
        kind: SuggestionKind::Delete {
            range: TextRange {
                start: second_id.clone(),
                end: second_id,
            },
        },
        state: SuggestionState::Proposed,
        provenance: Vec::new(),
    });

    let result = merge_operations(
        &base,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::AcceptSuggestion {
                suggestion_id,
                accepted_by: "Alice".to_string(),
            },
            context: None,
        }]],
    )
    .unwrap();

    assert_eq!(result.document.visible_text(), "alpha \n");
    assert_eq!(
        result.document.suggestions[0].state,
        SuggestionState::Accepted
    );
}

#[test]
fn accepting_link_change_suggestion_is_atomic_and_rejects_a_changed_source() {
    let mut base = Document::new("Doc");
    let inline = Inline::Text {
        id: StableId::parse("link-target").unwrap(),
        text: "OpenDoc".to_string(),
        marks: Vec::new(),
    };
    let inline_id = inline_id(&inline).clone();
    base.blocks.push(Block {
        id: StableId::new("paragraph"),
        kind: opendoc_core::BlockKind::Paragraph,
        properties: Default::default(),
        content: vec![inline],
    });
    let suggestion_id = StableId::parse("suggestion-link-accept").unwrap();
    base.suggestions.push(Suggestion {
        id: suggestion_id.clone(),
        author: "Bob".to_string(),
        kind: SuggestionKind::LinkChange {
            inline_id: inline_id.clone(),
            expected_href: None,
            href: Some("https://opendoc.example/".to_string()),
        },
        state: SuggestionState::Proposed,
        provenance: Vec::new(),
    });
    let result = merge_operations(
        &base,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::AcceptSuggestion {
                suggestion_id: suggestion_id.clone(),
                accepted_by: "Alice".to_string(),
            },
            context: None,
        }]],
    )
    .unwrap();
    assert!(
        matches!(&result.document.blocks[0].content[0], Inline::Link { id, href, .. } if id == &inline_id && href == "https://opendoc.example/")
    );
    assert_eq!(
        result.document.suggestions[0].state,
        SuggestionState::Accepted
    );

    // The exact stable id still exists, but a concurrent source change means
    // accepting this review proposal must not overwrite it.
    let mut changed = base;
    changed.blocks[0].content[0] = Inline::Link {
        id: inline_id,
        text: "OpenDoc".to_string(),
        href: "https://other.example/".to_string(),
        marks: Vec::new(),
    };
    let stale = merge_operations(
        &changed,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::AcceptSuggestion {
                suggestion_id,
                accepted_by: "Alice".to_string(),
            },
            context: None,
        }]],
    )
    .unwrap();
    assert!(
        matches!(&stale.document.blocks[0].content[0], Inline::Link { href, .. } if href == "https://other.example/")
    );
    assert_eq!(
        stale.document.suggestions[0].state,
        SuggestionState::Rejected
    );
    assert!(stale
        .warnings
        .iter()
        .any(|warning| warning.code == "suggestion-link-target-changed"));
}

#[test]
fn accepting_format_suggestion_applies_range_marks() {
    let mut base = Document::new("Doc");
    base.blocks.push(Block::paragraph("alpha"));
    let text_id = match &base.blocks[0].content[0] {
        Inline::Text { id, .. } => id.clone(),
        _ => unreachable!(),
    };
    let suggestion_id = StableId::parse("suggestion-format-accept").unwrap();
    base.suggestions.push(Suggestion {
        id: suggestion_id.clone(),
        author: "Bob".to_string(),
        kind: SuggestionKind::Format {
            range: TextRange {
                start: text_id.clone(),
                end: text_id,
            },
            marks: vec![Mark {
                kind: MarkKind::Bold,
                value: None,
                expand: MarkExpand::Both,
            }],
        },
        state: SuggestionState::Proposed,
        provenance: Vec::new(),
    });

    let result = merge_operations(
        &base,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::AcceptSuggestion {
                suggestion_id,
                accepted_by: "Alice".to_string(),
            },
            context: None,
        }]],
    )
    .unwrap();

    match &result.document.blocks[0].content[0] {
        Inline::Text { marks, .. } => {
            assert!(marks.iter().any(|mark| mark.kind == MarkKind::Bold));
        }
        _ => unreachable!(),
    }
    assert_eq!(
        result.document.suggestions[0].state,
        SuggestionState::Accepted
    );
}

#[test]
fn format_replacement_requires_the_reviewed_value_and_never_partially_applies() {
    let mut base = Document::new("Doc");
    let mut block = Block::paragraph("alpha");
    let text_id = inline_id(&block.content[0]).clone();
    if let Inline::Text { marks, .. } = &mut block.content[0] {
        marks.push(Mark {
            kind: MarkKind::Color,
            value: Some("#112233".to_string()),
            expand: MarkExpand::Both,
        });
    }
    base.blocks.push(block);
    let suggestion_id = StableId::parse("suggestion-format-replace").unwrap();
    base.suggestions.push(Suggestion {
        id: suggestion_id.clone(),
        author: "Bob".to_string(),
        kind: SuggestionKind::FormatReplace {
            range: TextRange {
                start: text_id.clone(),
                end: text_id,
            },
            kind: MarkKind::Color,
            expected_value: "#112233".to_string(),
            value: "#445566".to_string(),
        },
        state: SuggestionState::Proposed,
        provenance: Vec::new(),
    });

    let accepted = merge_operations(
        &base,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::AcceptSuggestion {
                suggestion_id: suggestion_id.clone(),
                accepted_by: "Alice".to_string(),
            },
            context: None,
        }]],
    )
    .unwrap();
    assert!(matches!(
        &accepted.document.blocks[0].content[0],
        Inline::Text { marks, .. }
            if marks.iter().any(|mark| mark.kind == MarkKind::Color && mark.value.as_deref() == Some("#445566"))
    ));
    assert_eq!(
        accepted.document.suggestions[0].state,
        SuggestionState::Accepted
    );

    let mut changed = base.clone();
    if let Inline::Text { marks, .. } = &mut changed.blocks[0].content[0] {
        marks[0].value = Some("#abcdef".to_string());
    }
    let rejected = merge_operations(
        &changed,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::AcceptSuggestion {
                suggestion_id,
                accepted_by: "Alice".to_string(),
            },
            context: None,
        }]],
    )
    .unwrap();
    assert!(matches!(
        &rejected.document.blocks[0].content[0],
        Inline::Text { marks, .. }
            if marks.iter().any(|mark| mark.kind == MarkKind::Color && mark.value.as_deref() == Some("#abcdef"))
    ));
    assert_eq!(
        rejected.document.suggestions[0].state,
        SuggestionState::Rejected
    );
    assert!(rejected
        .warnings
        .iter()
        .any(|item| item.code == "suggestion-format-precondition-failed"));
}

#[test]
fn format_removal_suggestion_is_inert_until_accepted_and_then_removes_the_mark() {
    let mut base = Document::new("Doc");
    let mut block = Block::paragraph("alpha");
    let text_id = inline_id(&block.content[0]).clone();
    if let Inline::Text { marks, .. } = &mut block.content[0] {
        marks.push(Mark {
            kind: MarkKind::Bold,
            value: None,
            expand: MarkExpand::Both,
        });
    }
    base.blocks.push(block);
    let suggestion_id = StableId::parse("suggestion-format-remove").unwrap();
    base.suggestions.push(Suggestion {
        id: suggestion_id.clone(),
        author: "Bob".to_string(),
        kind: SuggestionKind::FormatRemove {
            range: TextRange {
                start: text_id.clone(),
                end: text_id,
            },
            kind: MarkKind::Bold,
            value: None,
        },
        state: SuggestionState::Proposed,
        provenance: Vec::new(),
    });

    // Merely proposing the removal never mutates the reviewed content.
    assert!(matches!(
        &base.blocks[0].content[0],
        Inline::Text { marks, .. } if marks.iter().any(|mark| mark.kind == MarkKind::Bold)
    ));

    let result = merge_operations(
        &base,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::AcceptSuggestion {
                suggestion_id,
                accepted_by: "Alice".to_string(),
            },
            context: None,
        }]],
    )
    .unwrap();

    assert!(matches!(
        &result.document.blocks[0].content[0],
        Inline::Text { marks, .. } if marks.iter().all(|mark| mark.kind != MarkKind::Bold)
    ));
    assert_eq!(
        result.document.suggestions[0].state,
        SuggestionState::Accepted
    );
}

#[test]
fn rejecting_format_removal_suggestion_preserves_the_mark() {
    let mut base = Document::new("Doc");
    let mut block = Block::paragraph("alpha");
    let text_id = inline_id(&block.content[0]).clone();
    if let Inline::Text { marks, .. } = &mut block.content[0] {
        marks.push(Mark {
            kind: MarkKind::Italic,
            value: None,
            expand: MarkExpand::Both,
        });
    }
    base.blocks.push(block);
    let suggestion_id = StableId::parse("suggestion-format-remove-reject").unwrap();
    base.suggestions.push(Suggestion {
        id: suggestion_id.clone(),
        author: "Bob".to_string(),
        kind: SuggestionKind::FormatRemove {
            range: TextRange {
                start: text_id.clone(),
                end: text_id,
            },
            kind: MarkKind::Italic,
            value: None,
        },
        state: SuggestionState::Proposed,
        provenance: Vec::new(),
    });
    let result = merge_operations(
        &base,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::RejectSuggestion {
                suggestion_id,
                rejected_by: "Alice".to_string(),
            },
            context: None,
        }]],
    )
    .unwrap();
    assert!(matches!(
        &result.document.blocks[0].content[0],
        Inline::Text { marks, .. } if marks.iter().any(|mark| mark.kind == MarkKind::Italic)
    ));
    assert_eq!(
        result.document.suggestions[0].state,
        SuggestionState::Rejected
    );
}

#[test]
fn accepting_invalid_format_suggestion_degrades_without_mutating_marks() {
    let mut base = Document::new("Doc");
    base.blocks.push(Block::paragraph("alpha"));
    let text_id = inline_id(&base.blocks[0].content[0]).clone();
    let suggestion_id = StableId::parse("suggestion-format-invalid-accept").unwrap();
    base.suggestions.push(Suggestion {
        id: suggestion_id.clone(),
        author: "Bob".to_string(),
        kind: SuggestionKind::Format {
            range: TextRange {
                start: text_id.clone(),
                end: text_id,
            },
            marks: vec![Mark {
                kind: MarkKind::Color,
                value: None,
                expand: MarkExpand::Both,
            }],
        },
        state: SuggestionState::Proposed,
        provenance: Vec::new(),
    });

    let result = merge_operations(
        &base,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::AcceptSuggestion {
                suggestion_id: suggestion_id.clone(),
                accepted_by: "Alice".to_string(),
            },
            context: None,
        }]],
    )
    .unwrap();

    match &result.document.blocks[0].content[0] {
        Inline::Text { marks, .. } => assert!(marks.is_empty()),
        _ => unreachable!(),
    }
    assert_eq!(result.warnings[0].code, "invalid-mark-value");
    assert_eq!(
        result.document.suggestions[0].state,
        SuggestionState::Rejected
    );
    assert_eq!(
        result.document.suggestions[0].provenance,
        vec!["auto-rejected:invalid-accept-payload"]
    );
}

#[test]
fn suggestion_rejection_is_recorded_as_provenance() {
    let mut base = Document::new("Doc");
    base.blocks.push(Block::paragraph("hello"));
    let suggestion_id = StableId::new("suggestion");
    let suggestion = Suggestion {
        id: suggestion_id.clone(),
        author: "Bob".to_string(),
        kind: SuggestionKind::Insert {
            anchor: Anchor::Document,
            content: vec![Inline::text("nope")],
        },
        state: SuggestionState::Proposed,
        provenance: Vec::new(),
    };

    let result = merge_operations(
        &base,
        &[vec![
            Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::AddSuggestion { suggestion },
                context: None,
            },
            Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 2,
                },
                kind: OperationKind::RejectSuggestion {
                    suggestion_id,
                    rejected_by: "Carol".to_string(),
                },
                context: None,
            },
        ]],
    )
    .unwrap();

    assert_eq!(
        result.document.suggestions[0].state,
        SuggestionState::Rejected
    );
    assert_eq!(
        result.document.suggestions[0].provenance,
        vec!["rejected-by:Carol"]
    );
}

#[test]
fn malformed_suggestion_resolution_reviewer_degrades_to_valid_provenance() {
    let mut accept_base = Document::new("Doc");
    accept_base.blocks.push(Block::paragraph("hello"));
    let accept_id = StableId::parse("suggestion-accept-reviewer").unwrap();
    accept_base.suggestions.push(Suggestion {
        id: accept_id.clone(),
        author: "Bob".to_string(),
        kind: SuggestionKind::Insert {
            anchor: Anchor::Document,
            content: vec![Inline::text(" accepted")],
        },
        state: SuggestionState::Proposed,
        provenance: Vec::new(),
    });
    let accepted = merge_operations(
        &accept_base,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::AcceptSuggestion {
                suggestion_id: accept_id,
                accepted_by: " Alice ".to_string(),
            },
            context: None,
        }]],
    )
    .unwrap();
    assert_eq!(
        accepted.document.suggestions[0].state,
        SuggestionState::Accepted
    );
    assert_eq!(
        accepted.document.suggestions[0].provenance,
        vec!["accepted-by:unknown"]
    );
    assert!(accepted
        .warnings
        .iter()
        .any(|warning| warning.code == "invalid-suggestion-reviewer"));

    let mut reject_base = Document::new("Doc");
    reject_base.blocks.push(Block::paragraph("hello"));
    let reject_id = StableId::parse("suggestion-reject-reviewer").unwrap();
    reject_base.suggestions.push(Suggestion {
        id: reject_id.clone(),
        author: "Bob".to_string(),
        kind: SuggestionKind::Insert {
            anchor: Anchor::Document,
            content: vec![Inline::text(" rejected")],
        },
        state: SuggestionState::Proposed,
        provenance: Vec::new(),
    });
    let rejected = merge_operations(
        &reject_base,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("b".to_string()),
                seq: 1,
            },
            kind: OperationKind::RejectSuggestion {
                suggestion_id: reject_id,
                rejected_by: " ".to_string(),
            },
            context: None,
        }]],
    )
    .unwrap();
    assert_eq!(
        rejected.document.suggestions[0].state,
        SuggestionState::Rejected
    );
    assert_eq!(
        rejected.document.suggestions[0].provenance,
        vec!["rejected-by:unknown"]
    );
    assert!(rejected
        .warnings
        .iter()
        .any(|warning| warning.code == "invalid-suggestion-reviewer"));
}

#[test]
fn rejecting_resolved_suggestion_degrades_to_warning_without_state_change() {
    let mut base = Document::new("Doc");
    base.blocks.push(Block::paragraph("hello"));
    let suggestion_id = StableId::parse("suggestion-already-accepted").unwrap();
    base.suggestions.push(Suggestion {
        id: suggestion_id.clone(),
        author: "Bob".to_string(),
        kind: SuggestionKind::Insert {
            anchor: Anchor::Document,
            content: vec![Inline::text("accepted text")],
        },
        state: SuggestionState::Accepted,
        provenance: vec!["accepted-by:Alice".to_string()],
    });

    let result = merge_operations(
        &base,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::RejectSuggestion {
                suggestion_id,
                rejected_by: "Carol".to_string(),
            },
            context: None,
        }]],
    )
    .unwrap();

    assert_eq!(
        result.document.suggestions[0].state,
        SuggestionState::Accepted
    );
    assert_eq!(
        result.document.suggestions[0].provenance,
        vec!["accepted-by:Alice"]
    );
    assert!(result
        .warnings
        .iter()
        .any(|warning| warning.code == "resolved-suggestion"));
}

#[test]
fn concurrent_accept_and_reject_suggestion_converges_without_manual_conflict() {
    let mut base = Document::new("Doc");
    base.blocks.push(Block::paragraph("hello"));
    let suggestion_id = StableId::parse("suggestion-review-race").unwrap();
    base.suggestions.push(Suggestion {
        id: suggestion_id.clone(),
        author: "Bob".to_string(),
        kind: SuggestionKind::Insert {
            anchor: Anchor::Document,
            content: vec![Inline::text(" suggestion")],
        },
        state: SuggestionState::Proposed,
        provenance: Vec::new(),
    });
    let reject = Operation {
        id: OperationId {
            actor: ActorId("actor-a".to_string()),
            seq: 1,
        },
        kind: OperationKind::RejectSuggestion {
            suggestion_id: suggestion_id.clone(),
            rejected_by: "Carol".to_string(),
        },
        context: None,
    };
    let accept = Operation {
        id: OperationId {
            actor: ActorId("actor-b".to_string()),
            seq: 1,
        },
        kind: OperationKind::AcceptSuggestion {
            suggestion_id: suggestion_id.clone(),
            accepted_by: "Alice".to_string(),
        },
        context: None,
    };

    let actor_streams =
        merge_operations(&base, &[vec![reject.clone()], vec![accept.clone()]]).unwrap();
    let storage_batch =
        merge_operations(&base, &[vec![reject.clone(), accept.clone()], vec![]]).unwrap();
    let reversed_batches = merge_operations(&base, &[vec![accept], vec![reject]]).unwrap();

    assert_eq!(actor_streams.document, storage_batch.document);
    assert_eq!(actor_streams.document, reversed_batches.document);
    assert_eq!(actor_streams.warnings, storage_batch.warnings);
    assert_eq!(actor_streams.warnings, reversed_batches.warnings);
    assert_eq!(
        actor_streams.document.suggestions[0].state,
        SuggestionState::Rejected
    );
    assert_eq!(
        actor_streams.document.suggestions[0].provenance,
        vec!["rejected-by:Carol"]
    );
    assert!(!actor_streams.document.visible_text().contains("suggestion"));
    assert!(actor_streams
        .warnings
        .iter()
        .any(|warning| warning.code == "resolved-suggestion"));
}

#[test]
fn concurrent_suggestion_resolution_beats_stale_content_update() {
    let mut base = Document::new("Doc");
    base.blocks.push(Block::paragraph("hello"));
    let suggestion_id = StableId::parse("suggestion-stale-update-race").unwrap();
    base.suggestions.push(Suggestion {
        id: suggestion_id.clone(),
        author: "Bob".to_string(),
        kind: SuggestionKind::Insert {
            anchor: Anchor::Document,
            content: vec![Inline::text(" original proposal")],
        },
        state: SuggestionState::Proposed,
        provenance: Vec::new(),
    });
    let stale_update = Operation {
        id: OperationId {
            actor: ActorId("actor-a".to_string()),
            seq: 1,
        },
        kind: OperationKind::UpdateSuggestionInsertContent {
            suggestion_id: suggestion_id.clone(),
            content: vec![Inline::text(" stale rewrite")],
        },
        context: None,
    };
    let accept = Operation {
        id: OperationId {
            actor: ActorId("actor-b".to_string()),
            seq: 1,
        },
        kind: OperationKind::AcceptSuggestion {
            suggestion_id: suggestion_id.clone(),
            accepted_by: "Alice".to_string(),
        },
        context: None,
    };

    let actor_streams =
        merge_operations(&base, &[vec![stale_update.clone()], vec![accept.clone()]]).unwrap();
    let reversed_batches =
        merge_operations(&base, &[vec![accept.clone()], vec![stale_update.clone()]]).unwrap();
    let storage_batch = merge_operations(&base, &[vec![stale_update, accept]]).unwrap();

    assert_eq!(actor_streams.document, reversed_batches.document);
    assert_eq!(actor_streams.document, storage_batch.document);
    assert_eq!(actor_streams.warnings, reversed_batches.warnings);
    assert_eq!(actor_streams.warnings, storage_batch.warnings);
    assert_eq!(
        actor_streams.document.suggestions[0].state,
        SuggestionState::Accepted
    );
    match &actor_streams.document.suggestions[0].kind {
        SuggestionKind::Insert { content, .. } => match &content[0] {
            Inline::Text { text, .. } => assert_eq!(text, " original proposal"),
            _ => unreachable!(),
        },
        _ => unreachable!(),
    }
    assert!(actor_streams
        .document
        .visible_text()
        .contains("original proposal"));
    assert!(!actor_streams
        .document
        .visible_text()
        .contains("stale rewrite"));
    assert!(actor_streams
        .warnings
        .iter()
        .any(|warning| warning.code == "stale-suggestion-update"));
}

#[test]
fn insert_suggestion_content_updates_by_stable_id() {
    let mut base = Document::new("Doc");
    base.blocks.push(Block::paragraph("hello"));
    let suggestion_id = StableId::new("suggestion");
    base.suggestions.push(Suggestion {
        id: suggestion_id.clone(),
        author: "Alice".to_string(),
        kind: SuggestionKind::Insert {
            anchor: Anchor::Document,
            content: vec![Inline::text("old proposal")],
        },
        state: SuggestionState::Proposed,
        provenance: Vec::new(),
    });

    let result = merge_operations(
        &base,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::UpdateSuggestionInsertContent {
                suggestion_id,
                content: vec![Inline::text("new proposal")],
            },
            context: None,
        }]],
    )
    .unwrap();

    match &result.document.suggestions[0].kind {
        SuggestionKind::Insert { content, .. } => match &content[0] {
            Inline::Text { text, .. } => assert_eq!(text, "new proposal"),
            _ => unreachable!(),
        },
        _ => unreachable!(),
    }
}

#[test]
fn invalid_insert_suggestion_content_update_degrades_to_warning() {
    let mut base = Document::new("Doc");
    base.blocks.push(Block::paragraph("hello"));
    let suggestion_id = StableId::new("suggestion");
    base.suggestions.push(Suggestion {
        id: suggestion_id.clone(),
        author: "Alice".to_string(),
        kind: SuggestionKind::Insert {
            anchor: Anchor::Document,
            content: vec![Inline::text("old proposal")],
        },
        state: SuggestionState::Proposed,
        provenance: Vec::new(),
    });

    let result = merge_operations(
        &base,
        &[vec![
            Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::UpdateSuggestionInsertContent {
                    suggestion_id: suggestion_id.clone(),
                    content: Vec::new(),
                },
                context: None,
            },
            Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 2,
                },
                kind: OperationKind::UpdateSuggestionInsertContent {
                    suggestion_id: suggestion_id.clone(),
                    content: vec![Inline::text(" ")],
                },
                context: None,
            },
            Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 3,
                },
                kind: OperationKind::UpdateSuggestionInsertContent {
                    suggestion_id,
                    content: vec![Inline::Equation {
                        id: StableId::parse("suggestion-equation-empty").unwrap(),
                        equation: Equation {
                            id: StableId::parse("suggestion-equation").unwrap(),
                            source_format: EquationSourceFormat::LatexLike,
                            source: String::new(),
                        },
                    }],
                },
                context: None,
            },
        ]],
    )
    .unwrap();

    match &result.document.suggestions[0].kind {
        SuggestionKind::Insert { content, .. } => match &content[0] {
            Inline::Text { text, .. } => assert_eq!(text, "old proposal"),
            _ => unreachable!(),
        },
        _ => unreachable!(),
    }
    assert_eq!(
        result
            .warnings
            .iter()
            .map(|warning| warning.code.as_str())
            .collect::<Vec<_>>(),
        vec![
            "invalid-suggestion",
            "invalid-suggestion",
            "invalid-inline-equation-source"
        ]
    );
}

#[test]
fn invalid_suggestion_operations_degrade_to_warning() {
    let base = Document::new("Doc");
    let result = merge_operations(
        &base,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::AddSuggestion {
                suggestion: Suggestion {
                    id: StableId::parse("suggestion-empty").unwrap(),
                    author: "Reviewer".to_string(),
                    kind: SuggestionKind::Insert {
                        anchor: Anchor::Document,
                        content: Vec::new(),
                    },
                    state: SuggestionState::Proposed,
                    provenance: Vec::new(),
                },
            },
            context: None,
        }]],
    )
    .unwrap();

    assert!(result.document.suggestions.is_empty());
    assert_eq!(result.warnings[0].code, "invalid-suggestion");
}

#[test]
fn whitespace_insert_suggestion_add_degrades_to_warning() {
    let base = Document::new("Doc");
    let result = merge_operations(
        &base,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::AddSuggestion {
                suggestion: Suggestion {
                    id: StableId::parse("suggestion-whitespace").unwrap(),
                    author: "Reviewer".to_string(),
                    kind: SuggestionKind::Insert {
                        anchor: Anchor::Document,
                        content: vec![Inline::text(" ")],
                    },
                    state: SuggestionState::Proposed,
                    provenance: Vec::new(),
                },
            },
            context: None,
        }]],
    )
    .unwrap();

    assert!(result.document.suggestions.is_empty());
    assert_eq!(result.warnings[0].code, "invalid-suggestion");
}

#[test]
fn duplicate_imported_suggestion_provenance_is_not_replayed_as_duplicate_history() {
    let base = Document::new("Doc");
    let result = merge_operations(
        &base,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("importer".to_string()),
                seq: 1,
            },
            kind: OperationKind::AddSuggestion {
                suggestion: Suggestion {
                    id: StableId::parse("suggestion-duplicate-provenance").unwrap(),
                    author: "Reviewer".to_string(),
                    kind: SuggestionKind::Insert {
                        anchor: Anchor::Document,
                        content: vec![Inline::text("proposal")],
                    },
                    state: SuggestionState::Rejected,
                    provenance: vec![
                        "rejected-by:Reviewer".to_string(),
                        "rejected-by:Reviewer".to_string(),
                    ],
                },
            },
            context: None,
        }]],
    )
    .expect("invalid imported provenance is isolated to its operation");

    assert!(result.document.suggestions.is_empty());
    assert_eq!(result.warnings[0].code, "invalid-suggestion");
    result
        .document
        .validate()
        .expect("duplicate imported history cannot poison the document");
}

#[test]
fn insert_suggestion_anchor_repairs_after_target_text_delete() {
    let mut base = Document::new("Doc");
    let block = Block::paragraph("hello");
    let text_id = match &block.content[0] {
        Inline::Text { id, .. } => id.clone(),
        _ => unreachable!(),
    };
    base.blocks.push(block);
    let suggestion_id = StableId::new("suggestion");
    base.suggestions.push(Suggestion {
        id: suggestion_id.clone(),
        author: "Bob".to_string(),
        kind: SuggestionKind::Insert {
            anchor: Anchor::TextRange(TextRange {
                start: text_id.clone(),
                end: text_id.clone(),
            }),
            content: vec![Inline::text("insert")],
        },
        state: SuggestionState::Proposed,
        provenance: Vec::new(),
    });

    let result = merge_operations(
        &base,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::DeleteInline { inline_id: text_id },
            context: None,
        }]],
    )
    .unwrap();

    assert_eq!(result.warnings[0].code, "suggestion-anchor-degraded");
    assert!(result.warnings[0].message.contains(suggestion_id.as_str()));
    match &result.document.suggestions[0].kind {
        SuggestionKind::Insert { anchor, .. } => {
            assert!(matches!(anchor, Anchor::NearestBlock { .. }))
        }
        _ => unreachable!(),
    }
    assert_eq!(
        result.document.suggestions[0].provenance,
        vec!["auto-degraded:missing-anchor"]
    );
}

#[test]
fn format_suggestion_range_collapses_to_surviving_endpoint() {
    let mut base = Document::new("Doc");
    let mut block = Block::paragraph("");
    block.content.clear();
    let first = Inline::text("alpha ");
    let first_id = inline_id(&first).clone();
    let second = Inline::text("beta");
    let second_id = inline_id(&second).clone();
    block.content.extend([first, second]);
    base.blocks.push(block);
    let suggestion_id = StableId::new("suggestion");
    base.suggestions.push(Suggestion {
        id: suggestion_id.clone(),
        author: "Bob".to_string(),
        kind: SuggestionKind::Format {
            range: TextRange {
                start: first_id.clone(),
                end: second_id.clone(),
            },
            marks: vec![Mark {
                kind: MarkKind::Bold,
                value: None,
                expand: MarkExpand::Both,
            }],
        },
        state: SuggestionState::Proposed,
        provenance: Vec::new(),
    });

    let result = merge_operations(
        &base,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::DeleteInline {
                inline_id: first_id,
            },
            context: None,
        }]],
    )
    .unwrap();

    assert_eq!(result.warnings[0].code, "suggestion-range-degraded");
    assert!(result.warnings[0].message.contains(suggestion_id.as_str()));
    match &result.document.suggestions[0].kind {
        SuggestionKind::Format { range, .. } => {
            assert_eq!(range.start, second_id);
            assert_eq!(range.end, second_id);
        }
        _ => unreachable!(),
    }
    assert_eq!(
        result.document.suggestions[0].state,
        SuggestionState::Proposed
    );
    assert_eq!(
        result.document.suggestions[0].provenance,
        vec!["auto-degraded:partial-range"]
    );
}

#[test]
fn delete_suggestion_auto_rejects_when_entire_range_was_deleted() {
    let mut base = Document::new("Doc");
    let mut block = Block::paragraph("");
    block.content.clear();
    let first = Inline::text("alpha ");
    let first_id = inline_id(&first).clone();
    let second = Inline::text("beta");
    let second_id = inline_id(&second).clone();
    block.content.extend([first, second]);
    base.blocks.push(block);
    let suggestion_id = StableId::new("suggestion");
    base.suggestions.push(Suggestion {
        id: suggestion_id.clone(),
        author: "Bob".to_string(),
        kind: SuggestionKind::Delete {
            range: TextRange {
                start: first_id.clone(),
                end: second_id.clone(),
            },
        },
        state: SuggestionState::Proposed,
        provenance: Vec::new(),
    });

    let result = merge_operations(
        &base,
        &[vec![
            Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 1,
                },
                kind: OperationKind::DeleteInline {
                    inline_id: first_id,
                },
                context: None,
            },
            Operation {
                id: OperationId {
                    actor: ActorId("a".to_string()),
                    seq: 2,
                },
                kind: OperationKind::DeleteInline {
                    inline_id: second_id,
                },
                context: None,
            },
        ]],
    )
    .unwrap();

    assert_eq!(result.warnings[0].code, "suggestion-range-missing");
    assert!(result.warnings[0].message.contains(suggestion_id.as_str()));
    assert_eq!(
        result.document.suggestions[0].state,
        SuggestionState::Rejected
    );
    assert_eq!(
        result.document.suggestions[0].provenance,
        vec!["auto-rejected:missing-range"]
    );
}

#[test]
fn block_delete_suggestion_is_identity_targeted_and_convergent() {
    let mut base = Document::new("Doc");
    let first = Block::paragraph("keep");
    let target = Block::paragraph("remove");
    let target_id = target.id.clone();
    let later = Block::paragraph("also keep");
    base.blocks.extend([first, target, later]);
    let suggestion_id = StableId::new("block-delete-suggestion");

    let add = Operation {
        id: OperationId {
            actor: ActorId("author".to_string()),
            seq: 1,
        },
        kind: OperationKind::AddSuggestion {
            suggestion: Suggestion {
                id: suggestion_id.clone(),
                author: "Ada".to_string(),
                kind: SuggestionKind::BlockDelete {
                    block_id: target_id.clone(),
                },
                state: SuggestionState::Proposed,
                provenance: Vec::new(),
            },
        },
        context: None,
    };
    let accept = Operation {
        id: OperationId {
            actor: ActorId("reviewer".to_string()),
            seq: 1,
        },
        kind: OperationKind::AcceptSuggestion {
            suggestion_id: suggestion_id.clone(),
            accepted_by: "Grace".to_string(),
        },
        context: None,
    };

    let left = merge_operations(&base, &[vec![add.clone()], vec![accept.clone()]]).unwrap();
    let right = merge_operations(&base, &[vec![accept], vec![add]]).unwrap();
    assert_eq!(left.document, right.document);
    assert!(!left
        .document
        .blocks
        .iter()
        .any(|block| block.id == target_id));
    assert_eq!(left.document.blocks.len(), 2);
    assert_eq!(
        left.document.suggestions[0].state,
        SuggestionState::Accepted
    );
}

#[test]
fn block_delete_suggestion_never_retargets_after_its_block_is_deleted() {
    let mut base = Document::new("Doc");
    let target = Block::paragraph("target");
    let target_id = target.id.clone();
    base.blocks.extend([target, Block::paragraph("neighbour")]);
    base.suggestions.push(Suggestion {
        id: StableId::new("block-delete-missing"),
        author: "Ada".to_string(),
        kind: SuggestionKind::BlockDelete {
            block_id: target_id.clone(),
        },
        state: SuggestionState::Proposed,
        provenance: Vec::new(),
    });
    let result = merge_operations(
        &base,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("other".to_string()),
                seq: 1,
            },
            kind: OperationKind::DeleteBlock {
                block_id: target_id,
            },
            context: None,
        }]],
    )
    .unwrap();
    assert_eq!(result.document.blocks.len(), 1);
    assert_eq!(
        result.document.suggestions[0].state,
        SuggestionState::Rejected
    );
    assert_eq!(
        result.document.suggestions[0].provenance,
        vec!["auto-rejected:missing-block"]
    );
    assert!(result
        .warnings
        .iter()
        .any(|warning| warning.code == "suggestion-block-missing"));
}

#[test]
fn structural_insert_and_replace_are_identity_bound_and_convergent() {
    let mut base = Document::new("Doc");
    let mut anchor = Block::paragraph("anchor");
    anchor.id = StableId::new("structural-anchor");
    let anchor_id = anchor.id.clone();
    let mut target = Block::paragraph("old");
    target.id = StableId::new("structural-target");
    let target_id = target.id.clone();
    let expected = target.clone();
    base.blocks.extend([anchor, target]);
    let mut inserted = Block::paragraph("proposed after anchor");
    inserted.id = StableId::new("structural-inserted");
    let inserted_id = inserted.id.clone();
    let mut replacement = Block::paragraph("new");
    replacement.id = target_id.clone();
    let replace_id = StableId::new("replace-suggestion");
    let insert_id = StableId::new("insert-suggestion");
    let add_insert = Operation {
        id: OperationId {
            actor: ActorId("author".to_string()),
            seq: 1,
        },
        kind: OperationKind::AddSuggestion {
            suggestion: Suggestion {
                id: insert_id.clone(),
                author: "Ada".to_string(),
                kind: SuggestionKind::BlockInsert {
                    position: InsertPosition::After(anchor_id),
                    block: inserted,
                },
                state: SuggestionState::Proposed,
                provenance: Vec::new(),
            },
        },
        context: None,
    };
    let add_replace = Operation {
        id: OperationId {
            actor: ActorId("author".to_string()),
            seq: 2,
        },
        kind: OperationKind::AddSuggestion {
            suggestion: Suggestion {
                id: replace_id.clone(),
                author: "Ada".to_string(),
                kind: SuggestionKind::BlockReplace {
                    block_id: target_id,
                    expected: Box::new(expected),
                    replacement: Box::new(replacement),
                },
                state: SuggestionState::Proposed,
                provenance: Vec::new(),
            },
        },
        context: None,
    };
    let accept_insert = Operation {
        id: OperationId {
            actor: ActorId("reviewer".to_string()),
            seq: 1,
        },
        kind: OperationKind::AcceptSuggestion {
            suggestion_id: insert_id,
            accepted_by: "Grace".to_string(),
        },
        context: None,
    };
    let accept_replace = Operation {
        id: OperationId {
            actor: ActorId("reviewer".to_string()),
            seq: 2,
        },
        kind: OperationKind::AcceptSuggestion {
            suggestion_id: replace_id,
            accepted_by: "Grace".to_string(),
        },
        context: None,
    };
    let left = merge_operations(
        &base,
        &[
            vec![add_insert.clone(), add_replace.clone()],
            vec![accept_insert.clone(), accept_replace.clone()],
        ],
    )
    .unwrap();
    let right = merge_operations(
        &base,
        &[
            vec![accept_insert, accept_replace],
            vec![add_insert, add_replace],
        ],
    )
    .unwrap();
    assert_eq!(left.document, right.document);
    assert!(left
        .document
        .blocks
        .iter()
        .any(|block| block.id == inserted_id));
    assert!(left.document.blocks.iter().any(
        |block| matches!(block.content.as_slice(), [Inline::Text { text, .. }] if text == "new")
    ));
}

#[test]
fn block_replace_suggestion_rejects_a_concurrent_source_edit() {
    let mut base = Document::new("Doc");
    let mut target = Block::paragraph("old");
    target.id = StableId::new("replace-cas-target");
    let target_id = target.id.clone();
    let inline_id = match &target.content[0] {
        Inline::Text { id, .. } => id.clone(),
        other => panic!("paragraph constructor made unexpected inline {other:?}"),
    };
    let expected = target.clone();
    let mut replacement = Block::paragraph("proposed");
    replacement.id = target_id.clone();
    base.blocks.push(target);
    base.suggestions.push(Suggestion {
        id: StableId::new("replace-cas-suggestion"),
        author: "Ada".to_string(),
        kind: SuggestionKind::BlockReplace {
            block_id: target_id,
            expected: Box::new(expected),
            replacement: Box::new(replacement),
        },
        state: SuggestionState::Proposed,
        provenance: Vec::new(),
    });

    let result = merge_operations(
        &base,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("other".to_string()),
                seq: 1,
            },
            kind: OperationKind::UpdateInlineText {
                inline_id,
                text: "concurrent".to_string(),
            },
            context: None,
        }]],
    )
    .expect("concurrent source edit merges");

    assert!(matches!(
        &result.document.blocks[0].content[0],
        Inline::Text { text, .. } if text == "concurrent"
    ));
    assert_eq!(
        result.document.suggestions[0].state,
        SuggestionState::Rejected
    );
    assert!(result.document.suggestions[0]
        .provenance
        .iter()
        .any(|entry| entry == "auto-rejected:source-block-mismatch"));
    assert!(result
        .warnings
        .iter()
        .any(|warning| warning.code == "suggestion-block-source-changed"));
}

#[test]
fn structural_insert_rejects_a_deleted_anchor_instead_of_appending() {
    let mut base = Document::new("Doc");
    let mut anchor = Block::paragraph("anchor");
    anchor.id = StableId::new("missing-structural-anchor-target");
    let anchor_id = anchor.id.clone();
    base.blocks.push(anchor);
    base.suggestions.push(Suggestion {
        id: StableId::new("missing-structural-anchor"),
        author: "Ada".to_string(),
        kind: SuggestionKind::BlockInsert {
            position: InsertPosition::After(anchor_id.clone()),
            block: Block::paragraph("must not append"),
        },
        state: SuggestionState::Proposed,
        provenance: Vec::new(),
    });
    let result = merge_operations(
        &base,
        &[vec![Operation {
            id: OperationId {
                actor: ActorId("other".to_string()),
                seq: 1,
            },
            kind: OperationKind::DeleteBlock {
                block_id: anchor_id,
            },
            context: None,
        }]],
    )
    .unwrap();
    assert!(result.document.blocks.is_empty());
    assert_eq!(
        result.document.suggestions[0].state,
        SuggestionState::Rejected
    );
    assert!(result.document.suggestions[0]
        .provenance
        .iter()
        .any(|item| item == "auto-rejected:missing-block-anchor"));
}
