use super::*;
use opendoc_core::{ListKind, MarkKind};
use serde_json::json;

fn app_with(paragraphs: &[&str]) -> OpenDocApp {
    let mut app = OpenDocApp::new_sample();
    app.new_document("Editor");
    app.document.blocks.clear();
    for text in paragraphs {
        app.document.blocks.push(Block::paragraph(*text));
    }
    app
}

fn pos(app: &OpenDocApp, block: usize, offset: usize) -> EditorPosition {
    let block = &app.document.blocks[block];
    EditorPosition {
        block_id: block.id.to_string(),
        inline_id: block
            .content
            .first()
            .map(|inline| inline_stable_id(inline).to_string()),
        offset,
    }
}

fn input(selection: EditorSelection, input_type: &str, data: Option<&str>) -> EditorInput {
    EditorInput {
        selection,
        input_type: input_type.to_string(),
        data: data.map(str::to_string),
        html: None,
    }
}

fn texts(app: &OpenDocApp) -> Vec<String> {
    app.document
        .blocks
        .iter()
        .map(|block| {
            block
                .content
                .iter()
                .map(|inline| inline_text(inline).unwrap_or("\u{FFFC}"))
                .collect::<String>()
        })
        .collect()
}

fn list_levels(app: &OpenDocApp) -> Vec<(u8, bool)> {
    app.document
        .blocks
        .iter()
        .filter_map(|block| match block.kind {
            BlockKind::ListItem { level, kind, .. } => Some((level, kind.is_ordered())),
            _ => None,
        })
        .collect()
}

#[test]
fn describes_editor_selection_in_document_order() {
    let app = app_with(&["first", "middle", "last"]);
    let context = app
        .describe_editor_selection(EditorSelection {
            anchor: pos(&app, 2, 1),
            focus: pos(&app, 0, 2),
        })
        .unwrap();
    let expected_blocks = app
        .document
        .blocks
        .iter()
        .map(|block| block.id.to_string())
        .collect::<Vec<_>>();
    assert_eq!(context.selected_block_ids, expected_blocks);
    assert_eq!(
        context.focus_block_id,
        Some(app.document.blocks[0].id.to_string())
    );
    assert_eq!(
        context.inline_range.unwrap().start,
        inline_stable_id(&app.document.blocks[0].content[0]).to_string()
    );
}

#[test]
fn select_all_editor_content_is_rust_owned() {
    let app = app_with(&["first", "last"]);
    let result = app.select_all_editor_content().unwrap();

    assert!(result.handled);
    assert_eq!(result.selection.anchor, pos(&app, 0, 0));
    assert_eq!(result.selection.focus, pos(&app, 1, 4));
}

#[test]
fn selection_block_style_and_indent_commands_are_rust_owned() {
    let mut app = app_with(&["first", "middle"]);
    let selection = EditorSelection {
        anchor: pos(&app, 0, 0),
        focus: pos(&app, 1, 1),
    };
    app.set_editor_selection_block_style(selection.clone(), "list-item", 0, "bullet")
        .unwrap();
    assert_eq!(list_levels(&app), vec![(0, false), (0, false)]);
    app.adjust_editor_selection_list_indent(selection.clone(), 1)
        .unwrap();
    assert_eq!(list_levels(&app), vec![(1, false), (1, false)]);
    app.adjust_editor_selection_list_indent(selection, -8)
        .unwrap();
    assert_eq!(list_levels(&app), vec![(0, false), (0, false)]);
}

#[test]
fn typing_inserts_at_caret_and_moves_it() {
    let mut app = app_with(&["Hello world"]);
    let result = app
        .apply_editor_input(input(
            EditorSelection::collapsed(pos(&app, 0, 5)),
            "insertText",
            Some(","),
        ))
        .unwrap();
    assert!(result.handled);
    assert_eq!(texts(&app), vec!["Hello, world"]);
    assert_eq!(result.selection.focus.offset, 6);
    assert_eq!(
        result.selection.focus.block_id,
        app.document.blocks[0].id.to_string()
    );
}

#[test]
fn typing_replaces_a_selection_across_blocks() {
    let mut app = app_with(&["first line", "middle", "last line"]);
    let result = app
        .apply_editor_input(input(
            EditorSelection {
                anchor: pos(&app, 2, 5),
                focus: pos(&app, 0, 5),
            },
            "insertText",
            Some("-"),
        ))
        .unwrap();
    assert!(result.handled);
    assert_eq!(texts(&app), vec!["first-line"]);
    assert_eq!(result.selection.focus.offset, 6);
}

#[test]
fn enter_splits_and_backspace_joins() {
    let mut app = app_with(&["Hello world"]);
    let result = app
        .apply_editor_input(input(
            EditorSelection::collapsed(pos(&app, 0, 5)),
            "insertParagraph",
            None,
        ))
        .unwrap();
    assert_eq!(texts(&app), vec!["Hello", " world"]);
    assert_eq!(
        result.selection.focus.block_id,
        app.document.blocks[1].id.to_string()
    );
    assert_eq!(result.selection.focus.offset, 0);

    let result = app
        .apply_editor_input(input(
            EditorSelection::collapsed(pos(&app, 1, 0)),
            "deleteContentBackward",
            None,
        ))
        .unwrap();
    assert!(result.handled);
    assert_eq!(texts(&app), vec!["Hello world"]);
    assert_eq!(result.selection.focus.offset, 5);
}

#[test]
fn enter_at_end_of_heading_creates_paragraph_and_empty_list_item_leaves_list() {
    let mut app = app_with(&["Title"]);
    app.document.blocks[0].kind = BlockKind::Heading { level: 1 };
    app.apply_editor_input(input(
        EditorSelection::collapsed(pos(&app, 0, 5)),
        "insertParagraph",
        None,
    ))
    .unwrap();
    assert!(matches!(app.document.blocks[1].kind, BlockKind::Paragraph));
    assert!(matches!(
        app.document.blocks[0].kind,
        BlockKind::Heading { level: 1 }
    ));

    let mut app = app_with(&[""]);
    app.document.blocks[0].kind = BlockKind::ListItem {
        list_id: StableId::new("list"),
        level: 0,
        kind: ListKind::Bullet,
    };
    app.apply_editor_input(input(
        EditorSelection::collapsed(pos(&app, 0, 0)),
        "insertParagraph",
        None,
    ))
    .unwrap();
    assert_eq!(app.document.blocks.len(), 1);
    assert!(matches!(app.document.blocks[0].kind, BlockKind::Paragraph));
}

#[test]
fn enter_in_a_list_item_continues_the_list() {
    for list_kind in [
        ListKind::Bullet,
        ListKind::Ordered,
        ListKind::Checklist { checked: true },
    ] {
        let mut app = app_with(&["first item", "after"]);
        let list_id = StableId::new("list");
        app.document.blocks[0].kind = BlockKind::ListItem {
            list_id: list_id.clone(),
            level: 1,
            kind: list_kind,
        };
        let result = app
            .apply_editor_input(input(
                EditorSelection::collapsed(pos(&app, 0, char_len("first item"))),
                "insertParagraph",
                None,
            ))
            .unwrap();
        assert!(result.handled);
        assert_eq!(texts(&app), vec!["first item", "", "after"]);
        // The continuation stays in the same list, at the same level and
        // with the same numbering, so ordered lists keep counting.
        match &app.document.blocks[1].kind {
            BlockKind::ListItem {
                list_id: next_list,
                level,
                kind: next_kind,
            } => {
                assert_eq!(next_list, &list_id);
                assert_eq!(*level, 1);
                assert_eq!(*next_kind, list_kind);
            }
            other => panic!("expected a list item, got {other:?}"),
        }
        assert_eq!(
            result.selection.focus.block_id,
            app.document.blocks[1].id.to_string()
        );
        assert_eq!(result.selection.focus.offset, 0);

        // Enter on the now-empty continuation leaves the list instead of
        // adding another empty bullet.
        let result = app
            .apply_editor_input(input(
                EditorSelection::collapsed(pos(&app, 1, 0)),
                "insertParagraph",
                None,
            ))
            .unwrap();
        assert!(result.handled);
        assert_eq!(texts(&app), vec!["first item", "", "after"]);
        assert!(matches!(app.document.blocks[1].kind, BlockKind::Paragraph));
        assert_eq!(list_levels(&app), vec![(1, list_kind.is_ordered())]);
    }
}

#[test]
fn backspace_deletes_whole_grapheme_and_atomic_inlines() {
    let mut app = app_with(&["ok 👨‍👩‍👧"]);
    let len = char_len("ok 👨‍👩‍👧");
    let result = app
        .apply_editor_input(input(
            EditorSelection::collapsed(pos(&app, 0, len)),
            "deleteContentBackward",
            None,
        ))
        .unwrap();
    assert_eq!(texts(&app), vec!["ok "]);
    assert_eq!(result.selection.focus.offset, 3);

    let mut app = app_with(&["see "]);
    app.document.blocks[0].content.push(Inline::Mention {
        id: StableId::new("mention"),
        label: "@bob".to_string(),
    });
    let block_id = app.document.blocks[0].id.to_string();
    let result = app
        .apply_editor_input(input(
            EditorSelection::collapsed(EditorPosition {
                block_id,
                inline_id: Some(inline_stable_id(&app.document.blocks[0].content[1]).to_string()),
                offset: 1,
            }),
            "deleteContentBackward",
            None,
        ))
        .unwrap();
    assert!(result.handled);
    assert_eq!(app.document.blocks[0].content.len(), 1);
    assert_eq!(texts(&app), vec!["see "]);
}

#[test]
fn delete_word_backward_and_paste_multiline() {
    let mut app = app_with(&["alpha beta gamma"]);
    let result = app
        .apply_editor_input(input(
            EditorSelection::collapsed(pos(&app, 0, 10)),
            "deleteWordBackward",
            None,
        ))
        .unwrap();
    assert_eq!(texts(&app), vec!["alpha  gamma"]);
    assert_eq!(result.selection.focus.offset, 6);

    let mut app = app_with(&["ab"]);
    let result = app
        .apply_editor_input(input(
            EditorSelection::collapsed(pos(&app, 0, 1)),
            "insertFromPaste",
            Some("X\nY\nZ"),
        ))
        .unwrap();
    assert!(result.handled);
    assert_eq!(texts(&app), vec!["aX", "Y", "Zb"]);
}

/// A cell is a block container, not a string with a special newline
/// convention.  Plain-text clipboard lines must therefore have the same
/// structure as Enter, and the whole multi-operation edit must remain one
/// undoable command.
#[test]
fn multiline_plain_paste_in_a_table_cell_creates_paragraphs_and_undoes_as_one_edit() {
    let mut app = OpenDocApp::new_sample();
    app.new_document("Table paste");
    app.add_table().expect("table");
    let cell_id = first_cell_id(&app);
    let cell = all_cell_blocks(&app, &cell_id)
        .into_iter()
        .next()
        .expect("first cell paragraph");
    let position = block_position(&cell, 2);

    app.dispatch_command(
        "apply_editor_input",
        json!({
            "selection": { "anchor": position, "focus": position },
            "input_type": "insertFromPaste",
            "data": "one\ntwo\nthree",
        }),
    )
    .expect("paste into a table cell");

    let pasted = all_cell_blocks(&app, &cell_id);
    assert_eq!(
        pasted.iter().map(inline_string).collect::<Vec<_>>(),
        vec!["A1one", "two", "three"],
        "clipboard newlines become cell-local paragraph siblings"
    );
    assert!(
        pasted
            .iter()
            .all(|block| !inline_string(block).contains('\n')),
        "no cell paragraph smuggles the paste back into a soft break"
    );
    app.document
        .validate()
        .expect("the table tree remains valid");

    app.dispatch_command("undo_current_edit", json!({}))
        .expect("one undo reverses the entire paste");
    assert_eq!(
        all_cell_blocks(&app, &cell_id)
            .iter()
            .map(inline_string)
            .collect::<Vec<_>>(),
        vec!["A1"],
        "undo restores the pre-paste cell subtree"
    );
}

#[test]
fn multiline_rich_paste_in_a_table_cell_creates_paragraphs_with_marks() {
    let mut app = OpenDocApp::new_sample();
    app.new_document("Rich table paste");
    app.add_table().expect("table");
    let cell_id = first_cell_id(&app);
    let cell = all_cell_blocks(&app, &cell_id)
        .into_iter()
        .next()
        .expect("first cell paragraph");

    app.apply_editor_input(paste(
        EditorSelection::collapsed(block_position(&cell, 2)),
        "one\ntwo\nthree",
        "<p>one</p><p><b>two</b></p><p><i>three</i></p>",
    ))
    .expect("rich paste into a table cell");

    let pasted = all_cell_blocks(&app, &cell_id);
    assert_eq!(
        pasted.iter().map(inline_string).collect::<Vec<_>>(),
        vec!["A1one", "two", "three"]
    );
    assert_eq!(marks_of_cell(&pasted[1]), vec![MarkKind::Bold]);
    assert_eq!(marks_of_cell(&pasted[2]), vec![MarkKind::Italic]);
    assert!(pasted
        .iter()
        .all(|block| !inline_string(block).contains('\n')));
    app.document
        .validate()
        .expect("the table tree remains valid");
}

#[test]
fn rich_html_br_stays_an_inline_soft_break_in_one_paragraph() {
    let mut app = app_with(&["ab"]);
    let position = pos(&app, 0, 1);
    app.apply_editor_input(paste(
        EditorSelection::collapsed(position),
        "one\ntwo",
        "one<br><strong>two</strong>",
    ))
    .expect("rich soft-break paste");
    assert_eq!(texts(&app), vec!["abone\ntwo"]);
    assert_eq!(
        app.document.blocks.len(),
        1,
        "br must not create a sibling paragraph"
    );
    assert_eq!(
        marks_of(&app, 0),
        vec![
            ("ab".to_string(), vec![]),
            ("one\n".to_string(), vec![]),
            ("two".to_string(), vec![MarkKind::Bold]),
        ]
    );
}

#[test]
fn rich_html_canonical_time_inserts_a_date_chip_not_plain_text() {
    let mut app = app_with(&["before "]);
    let position = pos(&app, 0, 7);
    app.apply_editor_input(paste(
        EditorSelection::collapsed(position),
        "2024-02-29",
        r#"<time datetime="2024-02-29">2024-02-29</time>"#,
    ))
    .expect("canonical time paste");
    assert!(
        app.document.blocks[0]
            .content
            .iter()
            .any(|inline| matches!(inline, Inline::DateChip { date, .. } if date == "2024-02-29")),
        "the date must remain an atomic calendar value"
    );
    assert_eq!(app.document.visible_text(), "before 2024-02-29\n");
    app.document.validate().expect("pasted date chip is valid");
}

#[test]
fn rich_html_page_field_round_trips_and_leaves_the_caret_after_its_atomic_position() {
    let mut app = app_with(&["before after"]);
    let position = pos(&app, 0, 7);
    let result = app
        .apply_editor_input(paste(
            EditorSelection::collapsed(position),
            "",
            r#"<span class="doc-field" data-field="page-number"></span>"#,
        ))
        .expect("page-field paste");
    assert!(
        app.document.blocks[0].content.iter().any(|inline| matches!(
            inline,
            Inline::PageNumber {
                field: opendoc_core::PageNumberField::CurrentPage,
                ..
            }
        )),
        "the renderer's source token must remain a derived page field"
    );
    let after = DocumentIndex::build(&app.document.blocks);
    assert_eq!(
        after
            .resolve(&result.selection.anchor)
            .expect("returned paste selection")
            .abs,
        8,
        "the caret must be after the one-position atomic field, not before it"
    );
    app.document.validate().expect("pasted page field is valid");
}

#[test]
fn unsupported_gestures_are_reported_unhandled() {
    let mut app = app_with(&["only"]);
    let result = app
        .apply_editor_input(input(
            EditorSelection::collapsed(pos(&app, 0, 0)),
            "deleteContentBackward",
            None,
        ))
        .unwrap();
    assert!(!result.handled);
    assert_eq!(texts(&app), vec!["only"]);
    let result = app
        .apply_editor_input(input(
            EditorSelection::collapsed(pos(&app, 0, 0)),
            "historyUndo",
            None,
        ))
        .unwrap();
    assert!(!result.handled);
}

#[test]
fn editing_inside_table_cells_stays_within_the_cell() {
    let mut app = OpenDocApp::new_sample();
    app.new_document("Table");
    app.add_table().expect("a table");
    let table = app
        .document
        .blocks
        .iter()
        .find(|block| matches!(block.kind, BlockKind::Table { .. }))
        .unwrap()
        .clone();
    let BlockKind::Table { rows, .. } = &table.kind else {
        unreachable!()
    };
    let cell_block = &rows[0].cells[0].blocks[0];
    let position = EditorPosition {
        block_id: cell_block.id.to_string(),
        inline_id: Some(inline_stable_id(&cell_block.content[0]).to_string()),
        offset: 0,
    };
    let result = app
        .apply_editor_input(input(
            EditorSelection::collapsed(position.clone()),
            "insertText",
            Some("cell "),
        ))
        .unwrap();
    assert!(result.handled);
    assert!(app.document.visible_text().contains("cell A1"));
    // Enter inside a cell creates an ordinary second paragraph in that same
    // cell, rather than hiding a soft break in the first paragraph.
    let result = app
        .apply_editor_input(input(
            EditorSelection::collapsed(position),
            "insertParagraph",
            None,
        ))
        .unwrap();
    assert!(result.handled);
    let BlockKind::Table { rows, .. } = &app
        .document
        .blocks
        .iter()
        .find(|block| matches!(block.kind, BlockKind::Table { .. }))
        .expect("table still lives in the body")
        .kind
    else {
        panic!("table still lives in the body");
    };
    let blocks = &rows[0].cells[0].blocks;
    assert_eq!(blocks.len(), 2, "Enter split the cell paragraph");
    assert_eq!(
        blocks[0]
            .content
            .iter()
            .filter_map(inline_text)
            .collect::<String>(),
        ""
    );
    assert_eq!(
        blocks[1]
            .content
            .iter()
            .filter_map(inline_text)
            .collect::<String>(),
        "cell A1"
    );
}

#[test]
fn document_insertions_can_target_a_table_cell_block() {
    let mut app = OpenDocApp::new_empty_document();
    let body = app.document.blocks[0].id.to_string();
    app.insert_table_after(&body).expect("table");
    let cell_block = match &app.document.blocks[1].kind {
        BlockKind::Table { rows, .. } => rows[0].cells[0].blocks[0].id.to_string(),
        _ => panic!("table"),
    };

    app.insert_paragraph_after(Some(cell_block), "second paragraph")
        .expect("paragraph in a cell");
    let BlockKind::Table { rows, .. } = &app.document.blocks[1].kind else {
        panic!("table remains in the body");
    };
    let blocks = &rows[0].cells[0].blocks;
    assert_eq!(blocks.len(), 2);
    assert_eq!(
        blocks[1]
            .content
            .iter()
            .filter_map(inline_text)
            .collect::<String>(),
        "second paragraph"
    );
    let paragraph = blocks[1].id.to_string();
    let _ = blocks;
    app.insert_equation_block_after(&paragraph, "x^2")
        .expect("equation in a cell");
    app.document
        .validate()
        .expect("nested cell structure stays valid");
    let BlockKind::Table { rows, .. } = &app.document.blocks[1].kind else {
        panic!("table remains in the body");
    };
    assert!(matches!(
        rows[0].cells[0].blocks[2].kind,
        BlockKind::EquationBlock { .. }
    ));
    let equation = rows[0].cells[0].blocks[2].id.to_string();
    app.insert_table_after(&equation)
        .expect("nested table in a cell");
    app.document
        .validate()
        .expect("nested table keeps its cell fragment valid");
    let BlockKind::Table { rows, .. } = &app.document.blocks[1].kind else {
        panic!("outer table remains in the body");
    };
    assert!(matches!(
        rows[0].cells[0].blocks[3].kind,
        BlockKind::Table { .. }
    ));
}

#[test]
fn cell_blocks_can_be_reordered_by_stable_sibling_identity() {
    let mut app = OpenDocApp::new_empty_document();
    let body = app.document.blocks[0].id.to_string();
    app.insert_table_after(&body).expect("table");
    let first = match &app.document.blocks[1].kind {
        BlockKind::Table { rows, .. } => rows[0].cells[0].blocks[0].id.to_string(),
        _ => panic!("table"),
    };
    app.insert_paragraph_after(Some(first.clone()), "second")
        .expect("second cell paragraph");
    let second = match &app.document.blocks[1].kind {
        BlockKind::Table { rows, .. } => rows[0].cells[0].blocks[1].id.to_string(),
        _ => panic!("table"),
    };

    app.dispatch_command(
        "move_block",
        json!({
            "blockId": second,
            "anchorBlockId": first,
            "placement": "before",
        }),
    )
    .expect("move into the first cell-local sibling position");
    let BlockKind::Table { rows, .. } = &app.document.blocks[1].kind else {
        panic!("table");
    };
    let blocks = &rows[0].cells[0].blocks;
    assert_eq!(blocks[0].id.to_string(), second);
    assert_eq!(blocks[1].id.to_string(), first);
    app.document
        .validate()
        .expect("reordered cell remains valid");
}

#[test]
fn direct_delete_refuses_the_sole_block_of_a_table_cell() {
    let mut app = OpenDocApp::new_empty_document();
    let body = app.document.blocks[0].id.to_string();
    app.insert_table_after(&body).expect("table");
    let cell_id = first_cell_id(&app);
    let only_block = all_cell_blocks(&app, &cell_id)[0].id.to_string();

    app.delete_block(only_block)
        .expect("rejected delete is a valid command result");
    assert_eq!(all_cell_blocks(&app, &cell_id).len(), 1);
    assert!(app
        .document
        .warnings
        .iter()
        .any(|warning| warning.code == "table-cell-requires-block"));
    app.document
        .validate()
        .expect("sole-cell delete cannot corrupt the document");
}

#[test]
fn list_and_image_insertions_keep_their_cell_local_sibling_order() {
    let mut app = OpenDocApp::new_empty_document();
    let body = app.document.blocks[0].id.to_string();
    app.insert_table_after(&body).expect("table");
    let cell_block = match &app.document.blocks[1].kind {
        BlockKind::Table { rows, .. } => rows[0].cells[0].blocks[0].id.to_string(),
        _ => panic!("table"),
    };

    app.insert_list_item_after(&cell_block, "first", 0, "bullet")
        .expect("first cell-local list item");
    let first_list = match &app.document.blocks[1].kind {
        BlockKind::Table { rows, .. } => rows[0].cells[0].blocks[1].clone(),
        _ => panic!("table"),
    };
    app.insert_list_item_after(first_list.id.to_string(), "second", 0, "bullet")
        .expect("second cell-local list item");

    let (list_id, second_id) = match &app.document.blocks[1].kind {
        BlockKind::Table { rows, .. } => {
            let blocks = &rows[0].cells[0].blocks;
            assert_eq!(blocks.len(), 3);
            let BlockKind::ListItem { list_id, .. } = &blocks[1].kind else {
                panic!("first inserted block is a list item");
            };
            assert!(matches!(
                &blocks[2].kind,
                BlockKind::ListItem { list_id: second, .. } if second == list_id
            ));
            (list_id.clone(), blocks[2].id.to_string())
        }
        _ => panic!("table"),
    };
    assert!(!list_id.as_str().is_empty());

    app.add_binary_blob("cell.png", "image/png", vec![1, 2, 3])
        .expect("blob");
    let hash = app.blobs[0].hash.clone();
    app.dispatch_command(
        "insert_image_block_after",
        json!({
            "afterBlockId": second_id,
            "blobHash": hash,
            "altText": "cell picture",
        }),
    )
    .expect("cell-local image");
    app.document.validate().expect("cell remains valid");
    let BlockKind::Table { rows, .. } = &app.document.blocks[1].kind else {
        panic!("table remains in body");
    };
    let blocks = &rows[0].cells[0].blocks;
    assert!(matches!(blocks[3].kind, BlockKind::Image { .. }));

    app.undo_current_edit().expect("undo cell image insertion");
    let BlockKind::Table { rows, .. } = &app.document.blocks[1].kind else {
        panic!("table remains in body");
    };
    assert_eq!(rows[0].cells[0].blocks.len(), 3);
}

/// An atomic block in a cell is still an object, not an exception to object
/// selection. The desktop maps `Range.selectNode(figure)` through the same
/// two before/after positions as a body image; accepting that selection here
/// is what makes Delete and Backspace genuinely usable after a cell-local
/// image insertion.
#[test]
fn a_selected_image_inside_a_table_cell_is_deleted_as_one_object() {
    let mut app = OpenDocApp::new_empty_document();
    let body = app.document.blocks[0].id.to_string();
    app.insert_table_after(&body).expect("table");
    let cell_id = first_cell_id(&app);
    let after = all_cell_blocks(&app, &cell_id)[0].id.to_string();
    app.add_binary_blob("cell.png", "image/png", vec![1, 2, 3])
        .expect("blob");
    let hash = app.blobs[0].hash.clone();
    app.insert_image_block_after(&after, &hash, "cell picture")
        .expect("image in cell");
    let image = all_cell_blocks(&app, &cell_id)[1].clone();
    let selection = EditorSelection {
        anchor: EditorPosition {
            block_id: image.id.to_string(),
            inline_id: None,
            offset: 0,
        },
        focus: EditorPosition {
            block_id: image.id.to_string(),
            inline_id: None,
            offset: 1,
        },
    };

    // Go through the public command dispatcher: that is the UI path and it
    // owns the undo checkpoint around an editor gesture.
    app.dispatch_command(
        "apply_editor_input",
        json!({
            "selection": selection,
            "input_type": "deleteContentForward",
            "data": null,
        }),
    )
    .expect("delete selected image");
    assert_eq!(cell_block_count(&app, &cell_id), 1);
    app.document
        .validate()
        .expect("cell remains valid after deletion");
    app.undo_current_edit().expect("undo image deletion");
    assert!(matches!(
        all_cell_blocks(&app, &cell_id)[1].kind,
        BlockKind::Image { .. }
    ));
}

#[test]
fn joining_paragraphs_uses_the_table_cells_sibling_container() {
    let mut app = OpenDocApp::new_empty_document();
    let body = app.document.blocks[0].id.to_string();
    app.insert_table_after(&body).expect("table");
    let first = match &app.document.blocks[1].kind {
        BlockKind::Table { rows, .. } => rows[0].cells[0].blocks[0].id.clone(),
        _ => panic!("table"),
    };
    app.insert_paragraph_after(Some(first.to_string()), "second paragraph")
        .expect("paragraph in a cell");
    let second = match &app.document.blocks[1].kind {
        BlockKind::Table { rows, .. } => rows[0].cells[0].blocks[1].id.to_string(),
        _ => panic!("table"),
    };

    app.join_paragraph_with_previous(&second)
        .expect("join paragraphs in the same cell");
    app.document
        .validate()
        .expect("cell remains a valid document fragment");
    let BlockKind::Table { rows, .. } = &app.document.blocks[1].kind else {
        panic!("table");
    };
    let blocks = &rows[0].cells[0].blocks;
    assert_eq!(blocks.len(), 1);
    assert_eq!(
        blocks[0]
            .content
            .iter()
            .filter_map(inline_text)
            .collect::<String>(),
        "A1second paragraph"
    );
}

// ---- Deleting an atomic block ---------------------------------------------
//
// An image, an equation block and a page break are *one object*: the caret can
// stand before or after one and nowhere inside, and the only edit either
// delete key can make is to remove it whole. Three gestures have to reach it,
// and before this none of them did for an image:
//
// * the object is selected (the editor does that on a click, because Chrome
//   will not — see `DocumentEditor.selectAtomicBlock`);
// * Backspace at the start of the paragraph after it;
// * Delete at the end of the paragraph before it.
//
// A table is deliberately not one of these. Its cells hold ordinary blocks, so
// the caret goes into it and there is a real place for a delete key to act.

fn image_block() -> Block {
    Block {
        id: StableId::new("block"),
        kind: BlockKind::Image {
            blob_hash: "sha256:deadbeef".to_string(),
            alt_text: "a picture".to_string(),
            layout: Default::default(),
        },
        content: Vec::new(),
        properties: BlockProperties::default(),
    }
}

fn atomic_position(app: &OpenDocApp, block: usize, offset: usize) -> EditorPosition {
    EditorPosition {
        block_id: app.document.blocks[block].id.to_string(),
        inline_id: None,
        offset,
    }
}

fn kinds(app: &OpenDocApp) -> Vec<&'static str> {
    app.document
        .blocks
        .iter()
        .map(|block| match block.kind {
            BlockKind::Paragraph => "paragraph",
            BlockKind::Image { .. } => "image",
            BlockKind::EquationBlock { .. } => "equation",
            BlockKind::PageBreak => "page-break",
            BlockKind::Table { .. } => "table",
            _ => "other",
        })
        .collect()
}

#[test]
fn a_selected_image_is_deleted_by_either_delete_key() {
    for input_type in ["deleteContentBackward", "deleteContentForward"] {
        let mut app = app_with(&["before", "after"]);
        app.document.blocks.insert(1, image_block());
        // What the editor produces when an image is clicked: a range over the
        // whole object, anchor before it and focus after it.
        let result = app
            .apply_editor_input(input(
                EditorSelection {
                    anchor: atomic_position(&app, 1, 0),
                    focus: atomic_position(&app, 1, 1),
                },
                input_type,
                None,
            ))
            .unwrap();
        assert!(result.handled, "{input_type} was not handled");
        assert_eq!(
            kinds(&app),
            vec!["paragraph", "paragraph"],
            "{input_type} did not delete the selected image"
        );
    }
}

#[test]
fn backspace_after_an_image_deletes_it_and_delete_before_it_does_too() {
    let mut app = app_with(&["before", "after"]);
    app.document.blocks.insert(1, image_block());
    app.apply_editor_input(input(
        EditorSelection::collapsed(atomic_position(&app, 1, 1)),
        "deleteContentBackward",
        None,
    ))
    .unwrap();
    assert_eq!(kinds(&app), vec!["paragraph", "paragraph"]);

    let mut app = app_with(&["before", "after"]);
    app.document.blocks.insert(1, image_block());
    app.apply_editor_input(input(
        EditorSelection::collapsed(atomic_position(&app, 1, 0)),
        "deleteContentForward",
        None,
    ))
    .unwrap();
    assert_eq!(kinds(&app), vec!["paragraph", "paragraph"]);
}

#[test]
fn backspace_at_the_start_of_the_paragraph_after_an_image_deletes_the_image() {
    let mut app = app_with(&["before", "after"]);
    app.document.blocks.insert(1, image_block());
    let result = app
        .apply_editor_input(input(
            EditorSelection::collapsed(pos(&app, 2, 0)),
            "deleteContentBackward",
            None,
        ))
        .unwrap();
    assert!(result.handled, "Backspace after an image was unhandled");
    assert_eq!(kinds(&app), vec!["paragraph", "paragraph"]);
    assert_eq!(texts(&app), vec!["before", "after"]);
}

#[test]
fn delete_at_the_end_of_the_paragraph_before_an_image_deletes_the_image() {
    let mut app = app_with(&["before", "after"]);
    app.document.blocks.insert(1, image_block());
    let result = app
        .apply_editor_input(input(
            EditorSelection::collapsed(pos(&app, 0, 6)),
            "deleteContentForward",
            None,
        ))
        .unwrap();
    assert!(result.handled, "Delete before an image was unhandled");
    assert_eq!(kinds(&app), vec!["paragraph", "paragraph"]);
    assert_eq!(texts(&app), vec!["before", "after"]);
}

#[test]
fn an_equation_block_is_atomic_the_same_way_an_image_is() {
    let mut app = app_with(&["before", "after"]);
    app.document.blocks.insert(
        1,
        Block {
            id: StableId::new("block"),
            kind: BlockKind::EquationBlock {
                equation: opendoc_core::Equation {
                    id: StableId::new("equation"),
                    source_format: opendoc_core::EquationSourceFormat::LatexLike,
                    source: "x^2".to_string(),
                },
            },
            content: Vec::new(),
            properties: BlockProperties::default(),
        },
    );
    app.apply_editor_input(input(
        EditorSelection::collapsed(pos(&app, 2, 0)),
        "deleteContentBackward",
        None,
    ))
    .unwrap();
    assert_eq!(kinds(&app), vec!["paragraph", "paragraph"]);
}

/// The line the atomic rule is drawn at. A table is not one object: its cells
/// hold ordinary blocks, the caret goes inside it, and a keystroke at the
/// start of the paragraph after it must not throw away every cell.
#[test]
fn backspace_after_a_table_does_not_delete_the_table() {
    let mut app = app_with(&["after"]);
    app.document.blocks.insert(
        0,
        Block {
            id: StableId::new("block"),
            kind: BlockKind::table(vec![opendoc_core::TableRow::empty(2)]),
            content: Vec::new(),
            properties: BlockProperties::default(),
        },
    );
    let paragraph = app.document.blocks[1].id.to_string();
    let inline_id = app.document.blocks[1]
        .content
        .first()
        .map(|inline| inline_stable_id(inline).to_string());
    let result = app
        .apply_editor_input(input(
            EditorSelection::collapsed(EditorPosition {
                block_id: paragraph,
                inline_id,
                offset: 0,
            }),
            "deleteContentBackward",
            None,
        ))
        .unwrap();
    assert!(!result.handled, "Backspace after a table deleted something");
    assert_eq!(kinds(&app), vec!["table", "paragraph"]);
}

// ---- Selections that cross a block or a table boundary ---------------------
//
// Both of these used to return `handled: false` with no operations: the
// gesture did nothing and reported success, which is the worst of the three
// possible answers.

/// A cell holds ordinary blocks, so a selection across two of them is an
/// ordinary cross-block delete. `delete_range` refused it because the blocks
/// were not `top_level` — a condition about the *body*, applied to a cell.
#[test]
fn deleting_across_two_paragraphs_of_one_table_cell_works() {
    let mut app = OpenDocApp::new_sample();
    app.new_document("Table");
    app.add_table().expect("a table");
    let cell_id = first_cell_id(&app);

    // A second paragraph in the same cell, by splitting is not available
    // inside a cell yet — so it is placed directly, which is what the
    // document would hold either way.
    push_cell_block(&mut app, &cell_id, Block::paragraph("second"));

    let (first, second) = cell_blocks(&app, &cell_id);
    let selection = EditorSelection {
        anchor: block_position(&first, 1),
        focus: block_position(&second, 3),
    };
    let result = app
        .apply_editor_input(input(selection, "deleteContentBackward", None))
        .unwrap();

    assert!(
        result.handled,
        "the delete did nothing and said it was fine"
    );
    let (remaining, _) = cell_blocks(&app, &cell_id);
    assert_eq!(
        inline_string(&remaining),
        "Aond",
        "expected the tail of the first block joined to the tail of the second"
    );
    assert_eq!(
        cell_block_count(&app, &cell_id),
        1,
        "the emptied second block should have been joined away"
    );
}

/// A selection running from the body into a cell deletes the text it covers
/// and leaves the table standing. See `EditPlan::delete_range` for why the
/// structure is deliberately untouched.
#[test]
fn deleting_from_a_paragraph_into_a_table_cell_clears_the_text_it_covers() {
    let mut app = OpenDocApp::new_sample();
    app.new_document("Table");
    app.document.blocks.clear();
    app.document.blocks.push(Block::paragraph("before"));
    app.add_table().expect("a table");
    let cell_id = first_cell_id(&app);
    let (cell_first, _) = cell_blocks(&app, &cell_id);
    let rows_before = table_row_count(&app);

    let selection = EditorSelection {
        anchor: block_position(&app.document.blocks[0].clone(), 3),
        focus: block_position(&cell_first, 1),
    };
    let result = app
        .apply_editor_input(input(selection, "deleteContentBackward", None))
        .unwrap();

    assert!(
        result.handled,
        "the delete did nothing and said it was fine"
    );
    assert_eq!(
        inline_string(&app.document.blocks[0]),
        "bef",
        "the start paragraph kept text the selection covered"
    );
    let (cell_first, _) = cell_blocks(&app, &cell_id);
    assert_eq!(
        inline_string(&cell_first),
        "1",
        "the cell kept the head the selection covered"
    );
    assert_eq!(
        table_row_count(&app),
        rows_before,
        "a text selection removed table rows"
    );
    app.document.validate().expect("the document stays valid");
}

fn first_cell_id(app: &OpenDocApp) -> StableId {
    let table = app
        .document
        .blocks
        .iter()
        .find(|block| matches!(block.kind, BlockKind::Table { .. }))
        .expect("a table");
    let BlockKind::Table { rows, .. } = &table.kind else {
        unreachable!()
    };
    rows[0].cells[0].id.clone()
}

fn table_row_count(app: &OpenDocApp) -> usize {
    app.document
        .blocks
        .iter()
        .find_map(|block| match &block.kind {
            BlockKind::Table { rows, .. } => Some(rows.len()),
            _ => None,
        })
        .expect("a table")
}

fn push_cell_block(app: &mut OpenDocApp, cell_id: &StableId, block: Block) {
    for table in &mut app.document.blocks {
        if let BlockKind::Table { rows, .. } = &mut table.kind {
            for row in rows {
                for cell in &mut row.cells {
                    if &cell.id == cell_id {
                        cell.blocks.push(block);
                        return;
                    }
                }
            }
        }
    }
    panic!("cell {cell_id} was not found");
}

fn cell_block_count(app: &OpenDocApp, cell_id: &StableId) -> usize {
    all_cell_blocks(app, cell_id).len()
}

fn cell_blocks(app: &OpenDocApp, cell_id: &StableId) -> (Block, Block) {
    let blocks = all_cell_blocks(app, cell_id);
    let first = blocks.first().expect("the cell has a block").clone();
    let second = blocks.get(1).cloned().unwrap_or_else(|| first.clone());
    (first, second)
}

fn all_cell_blocks(app: &OpenDocApp, cell_id: &StableId) -> Vec<Block> {
    for table in &app.document.blocks {
        if let BlockKind::Table { rows, .. } = &table.kind {
            for row in rows {
                for cell in &row.cells {
                    if &cell.id == cell_id {
                        return cell.blocks.clone();
                    }
                }
            }
        }
    }
    panic!("cell {cell_id} was not found");
}

fn block_position(block: &Block, offset: usize) -> EditorPosition {
    EditorPosition {
        block_id: block.id.to_string(),
        inline_id: block
            .content
            .first()
            .map(|inline| inline_stable_id(inline).to_string()),
        offset,
    }
}

fn inline_string(block: &Block) -> String {
    block
        .content
        .iter()
        .map(|inline| inline_text(inline).unwrap_or("\u{FFFC}"))
        .collect()
}

// ---- Pasting with formatting ----------------------------------------------
//
// `EditorInput::html` was populated by the frontend on every paste and read
// by nothing: the contract advertised a feature that did not exist, and every
// paste arrived as unformatted text.

fn paste(selection: EditorSelection, text: &str, html: &str) -> EditorInput {
    EditorInput {
        selection,
        input_type: "insertFromPaste".to_string(),
        data: Some(text.to_string()),
        html: Some(html.to_string()),
    }
}

fn marks_of(app: &OpenDocApp, block: usize) -> Vec<(String, Vec<MarkKind>)> {
    app.document.blocks[block]
        .content
        .iter()
        .filter_map(|inline| match inline {
            Inline::Text { text, marks, .. } => {
                Some((text.clone(), marks.iter().map(|m| m.kind.clone()).collect()))
            }
            Inline::Link { text, marks, .. } => {
                Some((text.clone(), marks.iter().map(|m| m.kind.clone()).collect()))
            }
            _ => None,
        })
        // The empty placeholder run a split leaves behind is not content;
        // the plain-text paste path leaves one too.
        .filter(|(text, _)| !text.is_empty())
        .collect()
}

#[test]
fn pasting_html_keeps_its_marks() {
    let mut app = app_with(&["start"]);
    let result = app
        .apply_editor_input(paste(
            EditorSelection::collapsed(pos(&app, 0, 5)),
            "plain bold",
            "plain <b>bold</b>",
        ))
        .unwrap();

    assert!(result.handled);
    assert_eq!(
        marks_of(&app, 0),
        vec![
            ("start".to_string(), vec![]),
            ("plain ".to_string(), vec![]),
            ("bold".to_string(), vec![MarkKind::Bold]),
        ]
    );
    app.document.validate().expect("the document stays valid");
}

#[test]
fn rich_paste_keeps_direct_paragraph_alignment_and_direction_in_one_undo_step() {
    let mut app = app_with(&[""]);
    app.dispatch_command(
        "apply_editor_input",
        serde_json::to_value(paste(
            EditorSelection::collapsed(pos(&app, 0, 0)),
            "centered right-to-left\nleft-to-right",
            r#"<p style="text-align:center;direction:rtl">centered right-to-left</p><p dir="ltr">left-to-right</p>"#,
        ))
        .expect("serializable rich paragraph paste"),
    )
    .expect("rich paragraph paste");

    assert_eq!(app.document.blocks.len(), 2);
    assert_eq!(
        app.document.blocks[0].properties.alignment,
        Some(opendoc_core::Alignment::Center)
    );
    assert_eq!(
        app.document.blocks[0].properties.direction,
        Some(opendoc_core::TextDirection::RightToLeft)
    );
    assert_eq!(
        app.document.blocks[1].properties.direction,
        Some(opendoc_core::TextDirection::LeftToRight)
    );
    app.document
        .validate()
        .expect("typed pasted properties are valid");

    app.undo_current_edit()
        .expect("one undo reverses the full rich paste");
    assert_eq!(app.document.blocks.len(), 1);
    assert!(app.document.blocks[0].properties.is_empty());
    assert_eq!(inline_string(&app.document.blocks[0]), "");
}

#[test]
fn pasting_google_html_font_family_keeps_the_document_font_mark() {
    let mut app = app_with(&["start"]);
    app.apply_editor_input(paste(
        EditorSelection::collapsed(pos(&app, 0, 5)),
        "body",
        r#"<span style="font-family:Arial,sans-serif">body</span>"#,
    ))
    .unwrap();

    let pasted = app.document.blocks[0]
        .content
        .iter()
        .find(|inline| matches!(inline, Inline::Text { text, .. } if text == "body"))
        .expect("pasted text is present");
    let Inline::Text { marks, .. } = pasted else {
        unreachable!()
    };
    assert!(marks
        .iter()
        .any(|mark| { mark.kind == MarkKind::Font && mark.value.as_deref() == Some("Arial") }));
    app.document.validate().expect("pasted font mark is valid");
}

#[test]
fn standalone_html_table_pastes_as_an_editable_document_table() {
    let mut app = app_with(&["before"]);
    let result = app
        .apply_editor_input(paste(
            EditorSelection::collapsed(pos(&app, 0, 6)),
            "name score Ada 42",
            "<table><tr><th>name</th><th>score</th></tr><tr><td><b>Ada</b></td><td>42</td></tr></table>",
        ))
        .unwrap();

    assert!(result.handled);
    let table = app
        .document
        .blocks
        .iter()
        .find(|block| matches!(block.kind, BlockKind::Table { .. }))
        .expect("HTML table became a table block");
    let BlockKind::Table { rows, .. } = &table.kind else {
        unreachable!()
    };
    assert_eq!(rows.len(), 2);
    assert!(rows[0].header);
    assert_eq!(inline_string(&rows[1].cells[0].blocks[0]), "Ada");
    assert_eq!(
        marks_of_cell(&rows[1].cells[0].blocks[0]),
        vec![MarkKind::Bold]
    );
    app.document.validate().expect("pasted table is valid");
}

#[test]
fn semantic_html_thead_with_td_cells_pastes_as_a_native_header_row() {
    let mut app = app_with(&["before"]);
    app.apply_editor_input(paste(
        EditorSelection::collapsed(pos(&app, 0, 6)),
        "name score Ada 42",
        "<table><thead><tr><td>name</td><td>score</td></tr></thead><tbody><tr><td>Ada</td><td>42</td></tr></tbody></table>",
    ))
    .unwrap();

    let table = app
        .document
        .blocks
        .iter()
        .find(|block| matches!(block.kind, BlockKind::Table { .. }))
        .expect("HTML table became a table block");
    let BlockKind::Table { rows, .. } = &table.kind else {
        unreachable!()
    };
    assert!(rows[0].header, "thead has a durable row-header home");
    assert!(!rows[1].header, "tbody must remain ordinary data");
    assert_eq!(inline_string(&rows[0].cells[0].blocks[0]), "name");
    assert_eq!(inline_string(&rows[1].cells[0].blocks[0]), "Ada");
    app.document
        .validate()
        .expect("semantic clipboard header row is valid");
}

#[test]
fn html_horizontal_rule_pastes_as_a_native_rule_between_prose() {
    let mut app = app_with(&["before"]);
    app.apply_editor_input(paste(
        EditorSelection::collapsed(pos(&app, 0, 6)),
        "first second",
        "<p>first</p><hr style=\"border:99px solid red\"><p>second</p>",
    ))
    .unwrap();

    assert_eq!(texts(&app), vec!["beforefirst", "", "second"]);
    assert!(matches!(
        app.document.blocks[1].kind,
        BlockKind::HorizontalRule
    ));
    assert!(app.document.blocks[1].content.is_empty());
    app.document
        .validate()
        .expect("pasted horizontal rule is valid");
}

#[test]
fn html_checkbox_list_pastes_as_native_checklist_items() {
    let mut app = app_with(&["prefix"]);
    app.apply_editor_input(paste(
        EditorSelection::collapsed(pos(&app, 0, 6)),
        "intro done todo",
        "<p>intro</p><ul><li><input type=checkbox checked>done</li><li><input type=checkbox>todo</li></ul>",
    ))
    .unwrap();

    assert_eq!(texts(&app), vec!["prefixintro", "done", "todo"]);
    assert!(matches!(
        app.document.blocks[1].kind,
        BlockKind::ListItem {
            kind: ListKind::Checklist { checked: true },
            ..
        }
    ));
    assert!(matches!(
        app.document.blocks[2].kind,
        BlockKind::ListItem {
            kind: ListKind::Checklist { checked: false },
            ..
        }
    ));
    app.document.validate().expect("pasted checklist is valid");
}

#[test]
fn html_ordered_list_explicit_start_and_type_become_list_run_properties() {
    let mut app = app_with(&[""]);
    app.apply_editor_input(paste(
        EditorSelection::collapsed(pos(&app, 0, 0)),
        "C D",
        "<ol start=3 type=A><li>C</li><li>D</li></ol>",
    ))
    .unwrap();

    let list_id = match &app.document.blocks[0].kind {
        BlockKind::ListItem { list_id, kind, .. } => {
            assert!(kind.is_ordered());
            list_id
        }
        other => panic!("first pasted block should be ordered list item, got {other:?}"),
    };
    let properties = &app.document.list_properties[list_id];
    assert_eq!(properties.start_for(0), 3);
    assert_eq!(
        properties.format_for(0),
        opendoc_core::OrderedListFormat::UpperAlpha
    );
    assert!(matches!(
        app.document.blocks[1].kind,
        BlockKind::ListItem { .. }
    ));
    app.document
        .validate()
        .expect("pasted ordered list is valid");
}

#[test]
fn html_semantic_mark_pastes_as_a_durable_background_mark() {
    let mut app = app_with(&[""]);
    app.apply_editor_input(paste(
        EditorSelection::collapsed(pos(&app, 0, 0)),
        "relevant",
        "<mark>relevant</mark>",
    ))
    .unwrap();
    assert_eq!(
        marks_of(&app, 0),
        vec![("relevant".to_string(), vec![MarkKind::Background])]
    );
    app.document
        .validate()
        .expect("semantic highlight paste is valid durable source state");
}

#[test]
fn table_span_degradation_is_visible_once() {
    let mut app = app_with(&[""]);
    for _ in 0..2 {
        let selection = EditorSelection::collapsed(pos(&app, 0, 0));
        app.apply_editor_input(paste(
            selection,
            "merged",
            "<table><tr><td colspan=\"2\">merged</td></tr></table>",
        ))
        .unwrap();
    }
    assert_eq!(
        app.document
            .warnings
            .iter()
            .filter(|warning| warning.code == "clipboard-table-degraded")
            .count(),
        1,
        "the warning panel names the degradation without flooding"
    );
}

#[test]
fn unsupported_embedded_clipboard_objects_are_named_once_not_silently_dropped() {
    let mut app = app_with(&[""]);
    for _ in 0..2 {
        let selection = EditorSelection::collapsed(pos(&app, 0, 0));
        app.apply_editor_input(paste(
            selection,
            "caption",
            r#"<img src="https://example.invalid/picture.png" alt="caption">"#,
        ))
        .unwrap();
    }
    assert_eq!(texts(&app), vec!["captioncaption".to_string()]);
    assert_eq!(
        app.document
            .warnings
            .iter()
            .filter(|warning| warning.code == "clipboard-object-degraded")
            .count(),
        1,
        "unsupported object loss is explicit but repeated pastes do not flood warnings"
    );
}

#[test]
fn pasted_review_markup_warns_once_while_retaining_the_visible_text() {
    let mut app = app_with(&[""]);
    for _ in 0..2 {
        app.apply_editor_input(paste(
            EditorSelection::collapsed(pos(&app, 0, 0)),
            "before after",
            "<p><del>before</del><ins>after</ins></p>",
        ))
        .unwrap();
    }
    assert_eq!(texts(&app), vec!["beforeafterbeforeafter".to_string()]);
    assert_eq!(
        app.document
            .warnings
            .iter()
            .filter(|warning| warning.code == "clipboard-review-degraded")
            .count(),
        1,
        "revision history loss is visible without flooding the warning panel"
    );
}

#[test]
fn safe_mathml_paste_is_an_atomic_native_equation_and_one_undo_step() {
    let mut app = app_with(&["tail"]);
    let selection = EditorSelection::collapsed(pos(&app, 0, 0));
    let checkpoints = app.undo_stack.len();
    app.dispatch_command(
        "apply_editor_input",
        serde_json::to_value(paste(
            selection,
            "x squared tail",
            "<math><msup><mi>x</mi><mn>2</mn></msup></math> tail",
        ))
        .unwrap(),
    )
    .expect("safe MathML paste");

    assert_eq!(app.undo_stack.len(), checkpoints + 1);
    assert!(matches!(
        &app.document.blocks[0].content[0],
        Inline::Equation { equation, .. } if equation.source == "x^{2}"
    ));
    assert_eq!(texts(&app), vec!["\u{FFFC} tailtail".to_string()]);
    assert!(app
        .document
        .warnings
        .iter()
        .all(|warning| warning.code != "clipboard-object-degraded"));
    app.undo_current_edit()
        .expect("one undo restores the whole paste");
    assert_eq!(texts(&app), vec!["tail".to_string()]);
}

#[test]
fn mixed_raster_data_image_paste_owns_bytes_preserves_order_and_undoes_once() {
    let mut app = app_with(&["tail"]);
    let selection = EditorSelection::collapsed(pos(&app, 0, 0));
    let input = paste(
        selection,
        "before after",
        r#"<p>before<img src="data:image/png;base64,AQID">after</p>"#,
    );
    let checkpoints_before = app.undo_stack.len();
    app.dispatch_command("apply_editor_input", serde_json::to_value(input).unwrap())
        .expect("mixed paste");

    assert_eq!(app.undo_stack.len(), checkpoints_before + 1);
    assert_eq!(texts(&app), vec!["before", "", "aftertail"]);
    assert!(matches!(
        app.document.blocks[1].kind,
        BlockKind::Image { .. }
    ));
    assert_eq!(
        app.blobs.len(),
        1,
        "the image is content-addressed, not URL-backed"
    );
    assert_eq!(app.blob_bytes.values().next().unwrap(), &vec![1, 2, 3]);
    assert!(app
        .document
        .warnings
        .iter()
        .all(|warning| warning.code != "clipboard-object-degraded"));

    app.undo_current_edit()
        .expect("one undo restores the whole paste");
    assert_eq!(texts(&app), vec!["tail"]);
    assert!(app.blobs.is_empty());
}

#[test]
fn byte_owned_image_cap_is_visible_once_through_the_paste_warning_panel() {
    let mut app = app_with(&[""]);
    let html = r#"<img src="data:image/png;base64,AQID">"#.repeat(21);
    for _ in 0..2 {
        app.apply_editor_input(paste(
            EditorSelection::collapsed(pos(&app, 0, 0)),
            "",
            &html,
        ))
        .expect("bounded image paste");
    }
    assert_eq!(
        app.document
            .blocks
            .iter()
            .filter(|block| matches!(block.kind, BlockKind::Image { .. }))
            .count(),
        40,
        "each paste retains its first twenty byte-owned images"
    );
    assert_eq!(
        app.document
            .warnings
            .iter()
            .filter(|warning| warning.code == "clipboard-object-degraded")
            .count(),
        1,
        "the image-limit degradation is explicit without flooding the warning panel"
    );
}

#[test]
fn html_data_image_paste_preserves_its_alt_text_not_its_generated_blob_name() {
    let mut app = app_with(&[""]);
    app.apply_editor_input(paste(
        EditorSelection::collapsed(pos(&app, 0, 0)),
        "flow diagram",
        r#"<img src="data:image/png;base64,AQID" alt="Flow diagram">"#,
    ))
    .expect("safe data image paste");

    let BlockKind::Image { alt_text, .. } = &app
        .document
        .blocks
        .iter()
        .find(|block| matches!(block.kind, BlockKind::Image { .. }))
        .expect("data image becomes a native image block")
        .kind
    else {
        panic!("data image becomes a native image block");
    };
    assert_eq!(alt_text, "Flow diagram");
    assert_ne!(alt_text, "pasted-image.png");
    app.document
        .validate()
        .expect("image alt text is valid document state");
}

fn marks_of_cell(block: &Block) -> Vec<MarkKind> {
    block
        .content
        .iter()
        .flat_map(|inline| match inline {
            Inline::Text { marks, .. } | Inline::Link { marks, .. } => marks
                .iter()
                .map(|mark| mark.kind.clone())
                .collect::<Vec<_>>(),
            _ => Vec::new(),
        })
        .collect()
}

#[test]
fn pasting_several_html_paragraphs_makes_several_blocks() {
    let mut app = app_with(&["start"]);
    let result = app
        .apply_editor_input(paste(
            EditorSelection::collapsed(pos(&app, 0, 5)),
            "one\ntwo",
            "<p>one</p><p><i>two</i></p>",
        ))
        .unwrap();

    assert!(result.handled);
    assert_eq!(texts(&app), vec!["startone".to_string(), "two".to_string()]);
    assert_eq!(
        marks_of(&app, 1),
        vec![("two".to_string(), vec![MarkKind::Italic])]
    );
    app.document.validate().expect("the document stays valid");
}

#[test]
fn pasting_html_preserves_headings_and_nested_list_identity() {
    let mut app = app_with(&["prefix"]);
    app.apply_editor_input(paste(
        EditorSelection::collapsed(pos(&app, 0, 6)),
        "intro\nTitle\none\ntwo\nnested",
        "<p>intro</p><h2>Title</h2><ol><li>one</li><li>two<ul><li>nested</li></ul></li></ol>",
    ))
    .unwrap();

    assert_eq!(
        texts(&app),
        vec!["prefixintro", "Title", "one", "two", "nested"]
    );
    assert!(matches!(
        app.document.blocks[1].kind,
        BlockKind::Heading { level: 2 }
    ));
    let lists = app.document.blocks[2..]
        .iter()
        .map(|block| match &block.kind {
            BlockKind::ListItem {
                list_id,
                level,
                kind,
            } => (list_id.clone(), *level, *kind),
            other => panic!("expected list item, got {other:?}"),
        })
        .collect::<Vec<_>>();
    assert_eq!(lists[0].1, 0);
    assert_eq!(lists[1].1, 0);
    assert_eq!(lists[2].1, 1);
    assert_eq!(lists[0].2, ListKind::Ordered);
    assert_eq!(lists[1].2, ListKind::Ordered);
    assert_eq!(lists[2].2, ListKind::Bullet);
    assert_eq!(
        lists[0].0, lists[1].0,
        "sibling list items must share a list"
    );
    assert_ne!(lists[1].0, lists[2].0, "nested list needs its own identity");
    app.document.validate().expect("the document stays valid");
}

/// The HTML flavour is untrusted input from an arbitrary application. It
/// never becomes markup — it becomes runs — so a payload pastes as the words
/// it showed, and a script's *contents* do not arrive at all.
#[test]
fn a_hostile_paste_arrives_as_text_and_nothing_else() {
    let mut app = app_with(&[""]);
    app.apply_editor_input(paste(
        EditorSelection::collapsed(pos(&app, 0, 0)),
        "click",
        r#"<script>fetch('/steal')</script><a href="javascript:alert(1)">click</a>"#,
    ))
    .unwrap();

    assert_eq!(texts(&app), vec!["click".to_string()]);
    assert!(
        !app.document.blocks[0]
            .content
            .iter()
            .any(|inline| matches!(inline, Inline::Link { .. })),
        "a javascript: href kept its link"
    );
    let html = app.render_document_html();
    assert!(
        !html.contains("fetch('/steal')"),
        "the script's contents reached the rendered document: {html}"
    );
    app.document.validate().expect("the document stays valid");
}

/// With no HTML flavour, or an HTML flavour that says nothing, the plain-text
/// path is unchanged.
#[test]
fn a_paste_with_no_usable_html_still_pastes_its_text() {
    let mut app = app_with(&["start"]);
    app.apply_editor_input(EditorInput {
        selection: EditorSelection::collapsed(pos(&app, 0, 5)),
        input_type: "insertFromPaste".to_string(),
        data: Some("one\ntwo".to_string()),
        html: Some("<style>p{}</style>".to_string()),
    })
    .unwrap();
    assert_eq!(texts(&app), vec!["startone".to_string(), "two".to_string()]);
}
