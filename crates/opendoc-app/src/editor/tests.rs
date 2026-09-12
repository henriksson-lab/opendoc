use super::*;
use opendoc_core::ListKind;

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
    app.add_table();
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
    // Enter inside a cell becomes a soft line break rather than a split.
    let result = app
        .apply_editor_input(input(
            EditorSelection::collapsed(position),
            "insertParagraph",
            None,
        ))
        .unwrap();
    assert!(result.handled);
    assert!(app.document.visible_text().contains("\ncell A1"));
}
