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
