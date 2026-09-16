use super::*;
use opendoc_core::MarkKind;

fn app_with(text: &str) -> OpenDocApp {
    let mut app = OpenDocApp::new_sample();
    app.new_document("Marks");
    app.document.blocks.clear();
    app.document.blocks.push(Block::paragraph(text));
    app
}

fn selection(app: &OpenDocApp, from: usize, to: usize) -> EditorSelection {
    let block = &app.document.blocks[0];
    let inline_id = inline_stable_id(&block.content[0]).to_string();
    EditorSelection {
        anchor: EditorPosition {
            block_id: block.id.to_string(),
            inline_id: Some(inline_id.clone()),
            offset: from,
        },
        focus: EditorPosition {
            block_id: block.id.to_string(),
            inline_id: Some(inline_id),
            offset: to,
        },
    }
}

#[test]
fn bold_toggles_on_a_partial_run_and_splits_it() {
    let mut app = app_with("hello world");
    let result = app
        .apply_editor_mark(EditorMarkInput {
            selection: selection(&app, 6, 11),
            mark_kind: "bold".to_string(),
            value: None,
            action: None,
        })
        .unwrap();
    assert!(result.handled);
    let block = &app.document.blocks[0];
    assert_eq!(block.content.len(), 2);
    match &block.content[1] {
        Inline::Text { text, marks, .. } => {
            assert_eq!(text, "world");
            assert!(marks.iter().any(|mark| mark.kind == MarkKind::Bold));
        }
        other => panic!("unexpected {other:?}"),
    }
    // The anchor sits at the boundary: end of the first run or start
    // of the bold run are both valid.
    assert!(matches!(result.selection.anchor.offset, 0 | 6));
    assert_eq!(result.selection.focus.offset, 5);
    // Toggle again removes it.
    let bold_id = inline_stable_id(&block.content[1]).to_string();
    let block_id = block.id.to_string();
    let again = app
        .apply_editor_mark(EditorMarkInput {
            selection: EditorSelection {
                anchor: EditorPosition {
                    block_id: block_id.clone(),
                    inline_id: Some(bold_id.clone()),
                    offset: 0,
                },
                focus: EditorPosition {
                    block_id,
                    inline_id: Some(bold_id),
                    offset: 5,
                },
            },
            mark_kind: "bold".to_string(),
            value: None,
            action: None,
        })
        .unwrap();
    assert!(again.handled);
    match &app.document.blocks[0].content[1] {
        Inline::Text { marks, .. } => assert!(marks.is_empty()),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn middle_split_link_and_clear_all() {
    let mut app = app_with("abcdef");
    app.apply_editor_mark(EditorMarkInput {
        selection: selection(&app, 2, 4),
        mark_kind: "color".to_string(),
        value: Some("#ff0000".to_string()),
        action: Some("set".to_string()),
    })
    .unwrap();
    let texts: Vec<String> = app.document.blocks[0]
        .content
        .iter()
        .map(|inline| inline_text(inline).unwrap_or_default().to_string())
        .collect();
    assert_eq!(texts, vec!["ab", "cd", "ef"]);

    let mut app = app_with("visit site");
    app.apply_editor_mark(EditorMarkInput {
        selection: selection(&app, 6, 10),
        mark_kind: "link".to_string(),
        value: Some("https://example.org".to_string()),
        action: Some("set".to_string()),
    })
    .unwrap();
    assert!(matches!(
        app.document.blocks[0].content[1],
        Inline::Link { .. }
    ));

    let mut app = app_with("plain");
    app.apply_editor_mark(EditorMarkInput {
        selection: selection(&app, 0, 5),
        mark_kind: "italic".to_string(),
        value: None,
        action: None,
    })
    .unwrap();
    app.apply_editor_mark(EditorMarkInput {
        selection: selection(&app, 0, 5),
        mark_kind: "all".to_string(),
        value: None,
        action: Some("remove".to_string()),
    })
    .unwrap();
    match &app.document.blocks[0].content[0] {
        Inline::Text { marks, .. } => assert!(marks.is_empty()),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn sized_table_insertion() {
    let mut app = app_with("before");
    let block_id = app.document.blocks[0].id.to_string();
    let doc = app.insert_table_after_sized(&block_id, 3, 4).unwrap();
    let table = doc
        .blocks
        .iter()
        .find(|block| block.kind == "table")
        .unwrap();
    assert_eq!(table.rows.len(), 3);
    assert_eq!(table.rows[0].len(), 4);
}

/// A selection that crosses a block holding no text used to lose every text
/// block after it.
///
/// Pass 1 records one boundary pair per *text* block; pass 2 used to read them
/// back with `enumerate()` over every block in the range, non-text blocks
/// included. With a page break between the first and second paragraph, the
/// second paragraph was handed the first's boundaries and the third got
/// nothing at all, so Bold over all three bolded two of them — and pass 1 had
/// already split runs at boundaries the mark then never reached, so the
/// document also gained split runs carrying no formatting.
#[test]
fn a_mark_reaches_every_text_block_across_a_page_break() {
    let mut app = OpenDocApp::new_sample();
    app.new_document("Marks");
    app.document.blocks.clear();
    app.document.blocks.push(Block::paragraph("first"));
    app.document.blocks.push(Block {
        id: StableId::new("block"),
        kind: BlockKind::PageBreak,
        content: Vec::new(),
        properties: BlockProperties::default(),
    });
    app.document.blocks.push(Block::paragraph("second"));
    app.document.blocks.push(Block::paragraph("third"));

    let first = &app.document.blocks[0];
    let last = &app.document.blocks[3];
    let selection = EditorSelection {
        anchor: EditorPosition {
            block_id: first.id.to_string(),
            inline_id: Some(inline_stable_id(&first.content[0]).to_string()),
            offset: 0,
        },
        focus: EditorPosition {
            block_id: last.id.to_string(),
            inline_id: Some(inline_stable_id(&last.content[0]).to_string()),
            offset: 5,
        },
    };
    let result = app
        .apply_editor_mark(EditorMarkInput {
            selection,
            mark_kind: "bold".to_string(),
            value: None,
            action: None,
        })
        .unwrap();
    assert!(result.handled);

    let bold_of = |index: usize| -> Vec<(String, bool)> {
        app.document.blocks[index]
            .content
            .iter()
            .map(|inline| match inline {
                Inline::Text { text, marks, .. } => (
                    text.clone(),
                    marks.iter().any(|mark| mark.kind == MarkKind::Bold),
                ),
                other => panic!("unexpected {other:?}"),
            })
            .collect()
    };
    assert_eq!(bold_of(0), vec![("first".to_string(), true)]);
    assert_eq!(bold_of(2), vec![("second".to_string(), true)]);
    assert_eq!(
        bold_of(3),
        vec![("third".to_string(), true)],
        "the block after the page break lost its formatting"
    );
    // Every run was wholly inside the selection, so nothing needed splitting.
    for index in [0, 2, 3] {
        assert_eq!(
            app.document.blocks[index].content.len(),
            1,
            "block {index} was split into runs for no reason"
        );
    }
}

/// The same bug with a partial selection, which is what makes the split runs
/// visible: the mark has to land on exactly the characters selected in *both*
/// paragraphs, not on the first paragraph's range applied to the second.
#[test]
fn a_partial_mark_across_an_image_uses_each_blocks_own_boundaries() {
    let mut app = OpenDocApp::new_sample();
    app.new_document("Marks");
    app.document.blocks.clear();
    app.document.blocks.push(Block::paragraph("aaaabbbb"));
    app.document.blocks.push(Block {
        id: StableId::new("block"),
        kind: BlockKind::Image {
            blob_hash: "sha256:deadbeef".to_string(),
            alt_text: "a picture".to_string(),
            layout: Default::default(),
        },
        content: Vec::new(),
        properties: BlockProperties::default(),
    });
    app.document.blocks.push(Block::paragraph("ccccdddd"));

    let first = &app.document.blocks[0];
    let last = &app.document.blocks[2];
    let selection = EditorSelection {
        anchor: EditorPosition {
            block_id: first.id.to_string(),
            inline_id: Some(inline_stable_id(&first.content[0]).to_string()),
            offset: 4,
        },
        focus: EditorPosition {
            block_id: last.id.to_string(),
            inline_id: Some(inline_stable_id(&last.content[0]).to_string()),
            offset: 4,
        },
    };
    app.apply_editor_mark(EditorMarkInput {
        selection,
        mark_kind: "italic".to_string(),
        value: None,
        action: None,
    })
    .unwrap();

    let italics = |index: usize| -> Vec<(String, bool)> {
        app.document.blocks[index]
            .content
            .iter()
            .map(|inline| match inline {
                Inline::Text { text, marks, .. } => (
                    text.clone(),
                    marks.iter().any(|mark| mark.kind == MarkKind::Italic),
                ),
                other => panic!("unexpected {other:?}"),
            })
            .collect()
    };
    assert_eq!(
        italics(0),
        vec![("aaaa".to_string(), false), ("bbbb".to_string(), true)]
    );
    assert_eq!(
        italics(2),
        vec![("cccc".to_string(), true), ("dddd".to_string(), false)],
        "the paragraph after the image took the first paragraph's boundaries"
    );
}

/// A document of `count` top-level paragraphs, and a selection covering all
/// of them.
fn document_of_paragraphs(count: usize) -> (OpenDocApp, EditorSelection) {
    let mut app = OpenDocApp::new_sample();
    app.new_document("Marks");
    app.document.blocks.clear();
    for index in 0..count {
        app.document
            .blocks
            .push(Block::paragraph(format!("paragraph number {index}")));
    }
    let first = &app.document.blocks[0];
    let last = &app.document.blocks[count - 1];
    let selection = EditorSelection {
        anchor: EditorPosition {
            block_id: first.id.to_string(),
            inline_id: Some(inline_stable_id(&first.content[0]).to_string()),
            offset: 0,
        },
        focus: EditorPosition {
            block_id: last.id.to_string(),
            inline_id: Some(inline_stable_id(&last.content[0]).to_string()),
            offset: char_len(inline_text(&last.content[0]).expect("text run")),
        },
    };
    (app, selection)
}

fn bold_everything(count: usize) -> usize {
    let (mut app, selection) = document_of_paragraphs(count);
    let (result, visits) = crate::document_tree::measure_block_lookup_visits(|| {
        app.apply_editor_mark(EditorMarkInput {
            selection,
            mark_kind: "bold".to_string(),
            value: None,
            action: None,
        })
    });
    result.expect("bold is applied");
    // The mark really landed, so the measurement is of the work, not of an
    // early return.
    for block in &app.document.blocks {
        match &block.content[0] {
            Inline::Text { marks, .. } => {
                assert!(
                    marks.iter().any(|mark| mark.kind == MarkKind::Bold),
                    "every selected run should be bold"
                );
            }
            other => panic!("unexpected {other:?}"),
        }
    }
    visits
}

/// Applying one mark across a whole document must cost a number of block-tree
/// lookups proportional to the document, not to the document squared.
///
/// Both passes of `apply_editor_mark` need the `Block` behind each indexed
/// entry. They used to search the tree for it by id, once per selected block,
/// so quadrupling the document multiplied the search cost by sixteen. The
/// bound here is stated as a *growth ratio* between two document sizes rather
/// than as a constant, so it does not restate anything the implementation
/// chooses: linear work grows by four when the document does, quadratic work
/// by sixteen, and no reasonable constant factor closes that gap.
#[test]
fn one_mark_over_the_whole_document_costs_block_lookups_linear_in_its_size() {
    const SMALL: usize = 200;
    const LARGE: usize = 800;
    assert_eq!(LARGE, SMALL * 4, "the ratios below assume a factor of four");

    let small = bold_everything(SMALL);
    let large = bold_everything(LARGE);

    // The counter is wired to something real: marking N blocks cannot be done
    // without looking at least N blocks up.
    assert!(
        small >= SMALL,
        "bolding {SMALL} blocks visited only {small} block nodes, \
         so the lookup counter is not measuring the edit"
    );
    // Four times the document, at most six times the lookups. Quadratic is
    // sixteen.
    assert!(
        large <= small * 6,
        "bolding {SMALL} blocks visited {small} block nodes and bolding \
         {LARGE} visited {large}: growth of {:.1}x for 4x the document, \
         which is the per-selected-block document rescan coming back",
        large as f64 / small as f64
    );
}

/// The path a `DocumentIndex` records for a block has to address that exact
/// block, at every nesting level — a path is arithmetic over positions, and
/// getting it wrong hands a caller the wrong block's content silently.
#[test]
fn every_indexed_block_path_resolves_to_the_block_it_was_built_from() {
    let mut app = OpenDocApp::new_sample();
    app.new_document("Paths");
    app.document.blocks.clear();
    app.document.blocks.push(Block::paragraph("before"));
    app.document
        .blocks
        .push(crate::document_tree::default_table_block());
    app.document.blocks.push(Block::paragraph("between"));
    app.document
        .blocks
        .push(crate::document_tree::default_table_block());
    app.document.blocks.push(Block::paragraph("after"));

    let index = DocumentIndex::build(&app.document.blocks);
    let mut nested = 0;
    for entry in &index.blocks {
        let by_path = entry
            .path
            .resolve(&app.document.blocks)
            .unwrap_or_else(|| panic!("path for block {} resolves", entry.id));
        // Independent oracle: the same block found by searching for its id.
        let by_id = crate::document_tree::find_block_in_blocks(&app.document.blocks, &entry.id)
            .expect("indexed block is in the document");
        assert_eq!(
            by_path, by_id,
            "the path recorded for block {} addresses a different block",
            entry.id
        );
        if !entry.path.ancestors.is_empty() {
            nested += 1;
        }
    }
    // Two 2x2 tables of one paragraph per cell: eight blocks live inside
    // table cells, and a flat document would prove nothing about the
    // ancestor arithmetic.
    assert_eq!(nested, 8, "the fixture must exercise nested blocks");
}
