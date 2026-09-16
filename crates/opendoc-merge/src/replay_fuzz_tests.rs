//! Replay fuzz tests.

use crate::causal::{ActorId, OperationId};
use crate::inline_ops::inline_id;
use crate::merge::merge_operations;
use crate::operation::{Operation, OperationKind};
use crate::test_support::{cell_columns, table_cell, table_row};
use opendoc_core::{
    Anchor, BibliographyReference, Block, BlockKind, BlockProperties, CitationGroup, CitationItem,
    CitationPlacement, CitationSource, CitationSourceFormat, CitationSummary, Comment,
    CommentThread, Document, Equation, EquationSourceFormat, Inline, InsertPosition, Mark,
    MarkExpand, MarkKind, StableId, Suggestion, SuggestionKind, SuggestionState, TableRow,
    TextRange,
};

#[test]
fn deterministic_fuzz_like_replay_keeps_document_valid() {
    let mut base = Document::new("Doc");
    base.blocks.push(Block::paragraph("seed"));
    let block_id = base.blocks[0].id.clone();
    let mut streams = Vec::new();
    for actor in 0..3 {
        let mut stream = Vec::new();
        for seq in 0..8 {
            stream.push(Operation {
                id: OperationId {
                    actor: ActorId(format!("actor-{actor}")),
                    seq,
                },
                kind: OperationKind::InsertInline {
                    block_id: block_id.clone(),
                    position: InsertPosition::Last,
                    inline: Inline::text(format!("{actor}-{seq};")),
                },
                context: None,
            });
        }
        streams.push(stream);
    }
    let result = merge_operations(&base, &streams).unwrap();
    // The oracle. "It still validates" is true of a merge that drops every
    // operation, and `contains("2-7;")` is true of one that drops all but the
    // last. The answer here is fully determined: no operation carries a causal
    // context, so `causal_order` degenerates to `(actor, seq)`; `seq: 0` is
    // refused outright as a malformed operation id; and `InsertPosition::Last`
    // appends. So the runs must read in actor-then-sequence order, with each
    // actor's first (seq-0) operation missing. PLAN88 §7.
    let mut expected = String::from("seed");
    for actor in 0..3 {
        for seq in 1..8 {
            expected.push_str(&format!("{actor}-{seq};"));
        }
    }
    assert_eq!(result.document.visible_text(), format!("{expected}\n"));
    assert_eq!(
        result
            .warnings
            .iter()
            .map(|warning| warning.code.as_str())
            .collect::<Vec<_>>(),
        vec!["invalid-operation-id"],
        "the seq-0 operation from each actor is refused, once per distinct message"
    );
}

#[test]
fn shuffled_mixed_rich_document_streams_converge_with_warnings() {
    let mut base = Document::new("Doc");
    let mut block = Block::paragraph("");
    block.content.clear();
    let first = Inline::text("alpha ");
    let first_id = inline_id(&first).clone();
    let second = Inline::text("beta ");
    let second_id = inline_id(&second).clone();
    let third = Inline::text("gamma");
    let third_id = inline_id(&third).clone();
    block.content.extend([first, second, third]);
    let block_id = block.id.clone();
    base.blocks.push(block);

    let comment = CommentThread {
        id: StableId::parse("comment-thread-mixed").unwrap(),
        anchor: Anchor::TextRange(TextRange {
            start: first_id.clone(),
            end: first_id.clone(),
        }),
        comments: vec![Comment {
            id: StableId::parse("comment-mixed").unwrap(),
            author: "Alice".to_string(),
            body: vec![Inline::text("note")],
            created_at_ms: 1,
            deleted: false,
        }],
        state: opendoc_core::CommentThreadState::Open,
        resolved_by: None,
        resolved_at_ms: None,
        action_assignee: None,
        action_due_at_ms: None,
        action_completed_by: None,
        action_completed_at_ms: None,
        reactions: Vec::new(),
        deleted: false,
    };
    let suggestion = Suggestion {
        id: StableId::parse("suggestion-mixed").unwrap(),
        author: "Bob".to_string(),
        kind: SuggestionKind::Format {
            range: TextRange {
                start: first_id.clone(),
                end: second_id.clone(),
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
    let equation = Inline::Equation {
        id: StableId::parse("eq-inline-mixed").unwrap(),
        equation: Equation {
            id: StableId::parse("eq-mixed").unwrap(),
            source_format: EquationSourceFormat::LatexLike,
            source: "x+y".to_string(),
        },
    };
    let inserted = Inline::text("inserted ");

    let ops = vec![
        Operation {
            id: OperationId {
                actor: ActorId("actor-a".to_string()),
                seq: 1,
            },
            kind: OperationKind::AddCommentThread { thread: comment },
            context: None,
        },
        Operation {
            id: OperationId {
                actor: ActorId("actor-b".to_string()),
                seq: 1,
            },
            kind: OperationKind::AddSuggestion { suggestion },
            context: None,
        },
        Operation {
            id: OperationId {
                actor: ActorId("actor-c".to_string()),
                seq: 1,
            },
            kind: OperationKind::DeleteInline {
                inline_id: first_id.clone(),
            },
            context: None,
        },
        Operation {
            id: OperationId {
                actor: ActorId("actor-d".to_string()),
                seq: 1,
            },
            kind: OperationKind::AddMarkRange {
                range: TextRange {
                    start: first_id,
                    end: third_id,
                },
                mark: Mark {
                    kind: MarkKind::Bold,
                    value: None,
                    expand: MarkExpand::Both,
                },
            },
            context: None,
        },
        Operation {
            id: OperationId {
                actor: ActorId("actor-e".to_string()),
                seq: 1,
            },
            kind: OperationKind::InsertInline {
                block_id: block_id.clone(),
                position: InsertPosition::After(second_id.clone()),
                inline: inserted,
            },
            context: None,
        },
        Operation {
            id: OperationId {
                actor: ActorId("actor-f".to_string()),
                seq: 1,
            },
            kind: OperationKind::InsertInline {
                block_id,
                position: InsertPosition::After(second_id.clone()),
                inline: equation,
            },
            context: None,
        },
    ];

    let reference = merge_operations(&base, &[ops.clone(), Vec::new()]).unwrap();
    let batched_by_actor = merge_operations(
        &base,
        &[
            vec![ops[0].clone(), ops[3].clone()],
            vec![ops[1].clone(), ops[4].clone()],
            vec![ops[2].clone(), ops[5].clone()],
            Vec::new(),
            Vec::new(),
        ],
    )
    .unwrap();
    let reversed_batches = merge_operations(
        &base,
        &[
            vec![ops[5].clone(), ops[2].clone()],
            vec![ops[4].clone(), ops[1].clone()],
            vec![ops[3].clone(), ops[0].clone()],
        ],
    )
    .unwrap();

    // Grouping agreement is true by construction — the merge folds every
    // stream into one `BTreeMap<OperationId, Operation>` before any semantics
    // run — so it is kept as documentation, not as the check. PLAN88 §7.
    assert_eq!(reference.document, batched_by_actor.document);
    assert_eq!(reference.document, reversed_batches.document);
    assert_eq!(reference.warnings, batched_by_actor.warnings);
    assert_eq!(reference.warnings, reversed_batches.warnings);

    // The oracle. Every one of the six operations has a determined outcome
    // here, so name all of them rather than asking whether three warning
    // codes are somewhere in the list — which they were for a merge that lost
    // the mark, the equation and the insert as well.
    //
    // `actor-c` deleted "alpha ", so: the run is gone; `actor-d`'s mark range
    // started on it and can only reach as far as its surviving end, "gamma";
    // `actor-e`'s and `actor-f`'s inserts both anchored after "beta " and land
    // in operation-id order, the equation (e) before the text (f).
    assert_eq!(
        reference.document.visible_text(),
        "beta x+yinserted gamma\n"
    );
    let described: Vec<(String, Vec<MarkKind>)> = reference.document.blocks[0]
        .content
        .iter()
        .map(|inline| match inline {
            Inline::Text { text, marks, .. } => (
                text.clone(),
                marks.iter().map(|mark| mark.kind.clone()).collect(),
            ),
            Inline::Equation { equation, .. } => {
                (format!("equation:{}", equation.source), Vec::new())
            }
            other => (format!("{other:?}"), Vec::new()),
        })
        .collect();
    let expected: Vec<(String, Vec<MarkKind>)> = vec![
        ("beta ".to_string(), Vec::new()),
        ("equation:x+y".to_string(), Vec::new()),
        ("inserted ".to_string(), Vec::new()),
        ("gamma".to_string(), vec![MarkKind::Bold]),
    ];
    assert_eq!(described, expected);

    assert_eq!(
        reference
            .warnings
            .iter()
            .map(|warning| warning.code.as_str())
            .collect::<Vec<_>>(),
        vec![
            "comment-anchor-orphaned",
            "mark-range-degraded",
            "suggestion-range-degraded"
        ]
    );
    // …and the two anchors degraded to something specific, not merely to
    // "something that warned".
    let thread = &reference.document.comments[0];
    assert!(
        matches!(&thread.anchor, Anchor::Orphaned { quote, context, warning }
            if quote == "alpha " && context == "alpha beta gamma"
                && warning == "comment anchor source was deleted"),
        "{:?}",
        thread.anchor
    );
    let suggestion = &reference.document.suggestions[0];
    match &suggestion.kind {
        SuggestionKind::Format { range, .. } => assert_eq!(
            range.start, range.end,
            "the suggestion range kept an endpoint that no longer exists"
        ),
        other => panic!("expected a format suggestion, got {other:?}"),
    }
    assert_eq!(suggestion.provenance, vec!["auto-degraded:partial-range"]);
}

#[test]
fn shuffled_structured_document_batches_converge_with_citations_and_tables() {
    let mut base = Document::new("Doc");
    let reference_id = StableId::parse("ref-structured").unwrap();
    let citation_id = StableId::parse("citation-structured").unwrap();
    base.citation_database
        .upsert_reference(BibliographyReference {
            id: reference_id.clone(),
            revision: 1,
            source: CitationSource {
                format: CitationSourceFormat::CitumNative,
                bytes: b"title: Structured Merge".to_vec(),
            },
            summary: CitationSummary {
                title: "Structured Merge".to_string(),
                authors: vec!["Doe".to_string()],
                issued: Some("2026".to_string()),
                doi: None,
                url: None,
            },
            deleted: false,
        });
    base.citation_database.upsert_citation(CitationGroup {
        id: citation_id.clone(),
        revision: 1,
        items: vec![CitationItem {
            reference_id: reference_id.clone(),
            locator: None,
            label: None,
            prefix: None,
            suffix: None,
            suppress_author: false,
        }],
        placement: CitationPlacement::Inline,
        rendered_cache: Some("(Doe 2026)".to_string()),
        deleted: false,
    });

    let first = Inline::text("alpha ");
    let first_id = inline_id(&first).clone();
    let citation_inline = Inline::Citation {
        id: StableId::parse("citation-inline-structured").unwrap(),
        citation_id: citation_id.clone(),
        rendered_cache: Some("(Doe 2026)".to_string()),
    };
    let second = Inline::text("omega");
    let second_id = inline_id(&second).clone();
    let paragraph_id = StableId::parse("paragraph-structured").unwrap();
    base.blocks.push(Block {
        id: paragraph_id,
        kind: BlockKind::Paragraph,
        content: vec![first, citation_inline, second],
        properties: BlockProperties::default(),
    });

    let equation_inline_id = StableId::parse("equation-inline-structured").unwrap();
    base.blocks.push(Block {
        id: StableId::parse("equation-paragraph-structured").unwrap(),
        kind: BlockKind::Paragraph,
        content: vec![Inline::Equation {
            id: equation_inline_id.clone(),
            equation: Equation {
                id: StableId::parse("equation-structured").unwrap(),
                source_format: EquationSourceFormat::LatexLike,
                source: "x=1".to_string(),
            },
        }],
        properties: BlockProperties::default(),
    });

    let table_block_id = StableId::parse("table-structured").unwrap();
    let row_id = StableId::parse("row-structured").unwrap();
    let cell_id = StableId::parse("cell-structured").unwrap();
    base.blocks.push(Block {
        id: table_block_id.clone(),
        kind: BlockKind::table(vec![TableRow {
            id: row_id.clone(),
            height: None,
            header: false,
            cells: vec![table_cell(&cell_id, "base cell")],
        }]),
        content: Vec::new(),
        properties: BlockProperties::default(),
    });

    let ops = vec![
        Operation {
            id: OperationId {
                actor: ActorId("actor-a".to_string()),
                seq: 1,
            },
            kind: OperationKind::AddCommentThread {
                thread: CommentThread {
                    id: StableId::parse("comment-thread-structured").unwrap(),
                    anchor: Anchor::TextRange(TextRange {
                        start: first_id.clone(),
                        end: first_id.clone(),
                    }),
                    comments: vec![Comment {
                        id: StableId::parse("comment-structured").unwrap(),
                        author: "Reviewer".to_string(),
                        body: vec![Inline::text("structured note")],
                        created_at_ms: 1,
                        deleted: false,
                    }],
                    state: opendoc_core::CommentThreadState::Open,
                    resolved_by: None,
                    resolved_at_ms: None,
                    action_assignee: None,
                    action_due_at_ms: None,
                    action_completed_by: None,
                    action_completed_at_ms: None,
                    reactions: Vec::new(),
                    deleted: false,
                },
            },
            context: None,
        },
        Operation {
            id: OperationId {
                actor: ActorId("actor-b".to_string()),
                seq: 1,
            },
            kind: OperationKind::AddSuggestion {
                suggestion: Suggestion {
                    id: StableId::parse("suggestion-structured").unwrap(),
                    author: "Editor".to_string(),
                    kind: SuggestionKind::Format {
                        range: TextRange {
                            start: first_id.clone(),
                            end: second_id.clone(),
                        },
                        marks: vec![Mark {
                            kind: MarkKind::Italic,
                            value: None,
                            expand: MarkExpand::Both,
                        }],
                    },
                    state: SuggestionState::Proposed,
                    provenance: Vec::new(),
                },
            },
            context: None,
        },
        Operation {
            id: OperationId {
                actor: ActorId("actor-c".to_string()),
                seq: 1,
            },
            kind: OperationKind::DeleteInline {
                inline_id: first_id.clone(),
            },
            context: None,
        },
        Operation {
            id: OperationId {
                actor: ActorId("actor-d".to_string()),
                seq: 1,
            },
            kind: OperationKind::AddMarkRange {
                range: TextRange {
                    start: first_id,
                    end: second_id,
                },
                mark: Mark {
                    kind: MarkKind::Bold,
                    value: None,
                    expand: MarkExpand::Both,
                },
            },
            context: None,
        },
        Operation {
            id: OperationId {
                actor: ActorId("actor-e".to_string()),
                seq: 1,
            },
            kind: OperationKind::UpdateInlineEquationSource {
                inline_id: equation_inline_id,
                source: "y=2".to_string(),
            },
            context: None,
        },
        Operation {
            id: OperationId {
                actor: ActorId("actor-f".to_string()),
                seq: 1,
            },
            kind: {
                let row = table_row(
                    &StableId::parse("row-inserted-structured").unwrap(),
                    "row inserted",
                );
                OperationKind::InsertTableRow {
                    cell_columns: cell_columns(&base, &table_block_id, &row),
                    table_block_id: table_block_id.clone(),
                    position: InsertPosition::After(row_id.clone()),
                    row,
                }
            },
            context: None,
        },
        Operation {
            id: OperationId {
                actor: ActorId("actor-g".to_string()),
                seq: 1,
            },
            kind: OperationKind::InsertTableCell {
                table_block_id,
                row_id,
                position: InsertPosition::After(cell_id),
                cell: table_cell(
                    &StableId::parse("cell-inserted-structured").unwrap(),
                    "cell inserted",
                ),
            },
            context: None,
        },
        Operation {
            id: OperationId {
                actor: ActorId("actor-h".to_string()),
                seq: 1,
            },
            kind: OperationKind::DeleteBibliographyReference {
                reference_id,
                revision: 2,
            },
            context: None,
        },
    ];

    let reference = merge_operations(&base, &[ops.clone(), Vec::new()]).unwrap();
    let interleaved_batches = merge_operations(
        &base,
        &[
            vec![ops[0].clone(), ops[4].clone(), ops[7].clone()],
            vec![ops[1].clone(), ops[5].clone()],
            vec![ops[2].clone(), ops[6].clone()],
            vec![ops[3].clone()],
            Vec::new(),
        ],
    )
    .unwrap();
    let reversed_batches = merge_operations(
        &base,
        &[
            vec![ops[7].clone(), ops[3].clone()],
            vec![ops[6].clone(), ops[2].clone()],
            vec![ops[5].clone(), ops[1].clone()],
            vec![ops[4].clone(), ops[0].clone()],
        ],
    )
    .unwrap();

    // As above: the three groupings agree by construction, so the assertions
    // that can fail are the ones that name the answer. PLAN88 §7.
    assert_eq!(reference.document, interleaved_batches.document);
    assert_eq!(reference.document, reversed_batches.document);
    assert_eq!(reference.warnings, interleaved_batches.warnings);
    assert_eq!(reference.warnings, reversed_batches.warnings);

    // `contains("y=2")` is satisfied by a merge that applied the equation
    // update and dropped everything else; this is the whole document. The tab
    // is the cell separator and the trailing one is the cell the row insert
    // never knew about, filled in so the grid stays rectangular.
    assert_eq!(
        reference.document.visible_text(),
        "[citation-structured]omega\ny=2\nbase cell\tcell inserted\nrow inserted\t\n"
    );
    assert_eq!(
        reference
            .warnings
            .iter()
            .map(|warning| warning.code.as_str())
            .collect::<Vec<_>>(),
        vec![
            "comment-anchor-orphaned",
            "mark-range-degraded",
            "suggestion-range-degraded",
            "citation-reference-missing"
        ]
    );
    // The deleted reference invalidated the rendered label rather than leaving
    // a cached string that nothing backs any more.
    match &reference.document.blocks[0].content[0] {
        Inline::Citation { rendered_cache, .. } => assert_eq!(
            rendered_cache.as_deref(),
            None,
            "the citation kept a cache for a reference that was deleted"
        ),
        other => panic!("expected the citation inline first, got {other:?}"),
    }
    // The concurrent row insert and cell insert crossed: two rows, two
    // columns, and every row the same width.
    let BlockKind::Table { columns, rows, .. } = &reference.document.blocks[2].kind else {
        panic!(
            "expected a table, got {:?}",
            reference.document.blocks[2].kind
        );
    };
    assert_eq!(columns.len(), 2);
    assert_eq!(rows.len(), 2);
    for row in rows {
        assert_eq!(row.cells.len(), 2, "row {} is ragged", row.id);
    }
}

#[test]
fn deterministic_multi_replica_pseudo_fuzz_converges() {
    let mut base = Document::new("Doc");
    let mut block = Block::paragraph("");
    block.content.clear();
    let mut seed_ids = Vec::new();
    for label in ["alpha ", "beta ", "gamma ", "delta ", "epsilon"] {
        let inline = Inline::text(label);
        seed_ids.push(inline_id(&inline).clone());
        block.content.push(inline);
    }
    let block_id = block.id.clone();
    base.blocks.push(block);

    let mut streams = vec![Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new()];
    let mut seqs = [1_u64, 1, 1];
    let mut rng = 0x5eed_cafe_u64;
    for step in 0..36 {
        rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
        let actor_index = (rng as usize) % 3;
        let actor = format!("actor-{actor_index}");
        let seq = seqs[actor_index];
        seqs[actor_index] += 1;
        let first = seed_ids[(rng as usize >> 8) % seed_ids.len()].clone();
        let second = seed_ids[(rng as usize >> 16) % seed_ids.len()].clone();
        let kind = match step % 6 {
            0 => OperationKind::InsertInline {
                block_id: block_id.clone(),
                position: InsertPosition::After(first),
                inline: Inline::text(format!("i{step} ")),
            },
            1 => OperationKind::UpdateInlineText {
                inline_id: first,
                text: format!("u{step} "),
            },
            2 => OperationKind::AddMarkRange {
                range: TextRange {
                    start: first,
                    end: second,
                },
                mark: Mark {
                    kind: if step % 12 == 2 {
                        MarkKind::Bold
                    } else {
                        MarkKind::Italic
                    },
                    value: None,
                    expand: MarkExpand::Both,
                },
            },
            3 => OperationKind::AddCommentThread {
                thread: CommentThread {
                    id: StableId::parse(format!("comment-thread-{step}")).unwrap(),
                    anchor: Anchor::TextRange(TextRange {
                        start: first,
                        end: second,
                    }),
                    comments: vec![Comment {
                        id: StableId::parse(format!("comment-{step}")).unwrap(),
                        author: format!("author-{actor_index}"),
                        body: vec![Inline::text("note")],
                        created_at_ms: step,
                        deleted: false,
                    }],
                    state: opendoc_core::CommentThreadState::Open,
                    resolved_by: None,
                    resolved_at_ms: None,
                    action_assignee: None,
                    action_due_at_ms: None,
                    action_completed_by: None,
                    action_completed_at_ms: None,
                    reactions: Vec::new(),
                    deleted: false,
                },
            },
            4 => OperationKind::AddSuggestion {
                suggestion: Suggestion {
                    id: StableId::parse(format!("suggestion-{step}")).unwrap(),
                    author: format!("author-{actor_index}"),
                    kind: SuggestionKind::Delete {
                        range: TextRange {
                            start: first,
                            end: second,
                        },
                    },
                    state: SuggestionState::Proposed,
                    provenance: Vec::new(),
                },
            },
            _ => OperationKind::DeleteInline { inline_id: first },
        };
        streams[actor_index].push(Operation {
            id: OperationId {
                actor: ActorId(actor),
                seq,
            },
            kind,
            context: None,
        });
    }

    let single_stream = streams
        .iter()
        .flatten()
        .cloned()
        .collect::<Vec<Operation>>();
    let reversed_streams = streams
        .iter()
        .rev()
        .cloned()
        .collect::<Vec<Vec<Operation>>>();

    let reference = merge_operations(&base, &streams).unwrap();
    let single = merge_operations(&base, &[single_stream]).unwrap();
    let reversed = merge_operations(&base, &reversed_streams).unwrap();

    assert_eq!(reference.document, single.document);
    assert_eq!(reference.document, reversed.document);
    assert_eq!(reference.warnings, single.warnings);
    assert_eq!(reference.warnings, reversed.warnings);
    assert!(reference
        .warnings
        .iter()
        .any(|warning| warning.code == "missing-inline"));

    // The oracle. Everything above compares the merge against itself under
    // three groupings of one operation set, and those agree by construction —
    // `merge_operations` folds every stream into one `BTreeMap<OperationId,
    // Operation>` before a line of semantics runs. A `merge_operations` that
    // returned `base.clone()` passed all four of them. PLAN88 §7.
    //
    // The script is seeded, so its answer is a constant. It is recorded rather
    // than derived — deriving it would mean reimplementing `apply` — so it is
    // a change detector, but one with teeth: it pins the surviving text, the
    // inline count, the annotations that stayed anchored and the exact warning
    // sequence, and no trivial merge satisfies it. A diff here means the
    // merge's answer to this script changed, which has to be explained before
    // the constant is updated.
    assert_eq!(
        reference.document.visible_text(),
        "u19 gamma i18 i12 i24 epsiloni30 i0 i6 \n"
    );
    assert_eq!(reference.document.blocks[0].content.len(), 9);
    assert_eq!(reference.document.comments.len(), 6);
    assert_eq!(reference.document.suggestions.len(), 6);
    assert_eq!(
        reference
            .warnings
            .iter()
            .map(|warning| warning.code.as_str())
            .collect::<Vec<_>>(),
        vec![
            "missing-inline",
            "missing-inline",
            "missing-inline",
            "comment-anchor-degraded",
            "missing-inline",
            "comment-anchor-degraded",
            "missing-inline",
            "inline-anchor-degraded",
            "comment-anchor-degraded",
            "missing-inline",
            "comment-anchor-degraded",
            "mark-range-degraded",
            "mark-range-degraded",
            "mark-range-degraded",
            "mark-range-degraded",
            "suggestion-range-degraded",
            "suggestion-range-degraded",
            "suggestion-range-degraded",
            "suggestion-range-degraded",
            "suggestion-range-missing",
        ]
    );
}
