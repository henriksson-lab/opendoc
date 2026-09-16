use super::*;
use opendoc_core::{Alignment, Length, LineSpacing, TextDirection};
use serde_json::json;

fn app_with_paragraphs(texts: &[&str]) -> OpenDocApp {
    let mut app = OpenDocApp::new_sample();
    app.new_document("Lists");
    app.document.blocks.clear();
    for text in texts {
        app.document.blocks.push(Block::paragraph(*text));
    }
    app
}

fn block_id(app: &OpenDocApp, index: usize) -> String {
    app.document.blocks[index].id.to_string()
}

#[test]
fn date_chip_command_updates_one_validated_atomic_value_and_undoes() {
    let mut app = app_with_paragraphs(&[""]);
    let chip_id = StableId::parse("date-chip-command").expect("valid id");
    app.document.blocks[0].content = vec![Inline::DateChip {
        id: chip_id.clone(),
        date: "2024-02-29".to_string(),
    }];

    app.dispatch_command(
        "update_date_chip",
        json!({ "inlineId": chip_id, "date": "2025-03-01" }),
    )
    .expect("the public command reaches the atomic update");
    assert!(matches!(
        &app.document.blocks[0].content[0],
        Inline::DateChip { date, .. } if date == "2025-03-01"
    ));

    app.undo_current_edit()
        .expect("date-chip update is undoable");
    assert!(matches!(
        &app.document.blocks[0].content[0],
        Inline::DateChip { date, .. } if date == "2024-02-29"
    ));

    let error = app
        .dispatch_command(
            "update_date_chip",
            json!({ "inlineId": chip_id, "date": "2025-02-29" }),
        )
        .expect_err("invalid calendar values remain rejected at the command boundary");
    assert!(
        error.to_string().contains("invalid calendar date"),
        "{error}"
    );
}

#[test]
fn date_chip_insert_uses_the_caret_boundary_validates_and_undoes() {
    let mut app = app_with_paragraphs(&["before"]);
    let block_id = block_id(&app, 0);
    let after_inline_id = match &app.document.blocks[0].content[0] {
        Inline::Text { id, .. } => id.to_string(),
        inline => panic!("expected text inline, got {inline:?}"),
    };

    app.dispatch_command(
        "insert_date_chip_after",
        json!({ "blockId": block_id, "afterInlineId": after_inline_id, "date": "2028-02-29" }),
    )
    .expect("the public command inserts a validated atomic chip at the caret");
    assert!(matches!(
        app.document.blocks[0].content.last(),
        Some(Inline::DateChip { date, .. }) if date == "2028-02-29"
    ));
    app.undo_current_edit()
        .expect("insert is one undoable gesture");
    assert_eq!(app.document.blocks[0].content.len(), 1);

    let error = app
        .dispatch_command(
            "insert_date_chip_after",
            json!({ "blockId": block_id, "afterInlineId": null, "date": "2025-02-29" }),
        )
        .expect_err("invalid calendar values cannot be inserted");
    assert!(
        error.to_string().contains("invalid calendar date"),
        "{error}"
    );
}

#[test]
fn table_of_contents_insert_is_one_undoable_atomic_block() {
    let mut app = app_with_paragraphs(&["before"]);
    let before = block_id(&app, 0);
    app.dispatch_command(
        "insert_table_of_contents_after",
        json!({ "afterBlockId": before }),
    )
    .expect("table of contents inserts");
    assert!(matches!(
        app.document.blocks[1].kind,
        BlockKind::TableOfContents { max_level: 3 }
    ));
    app.undo_current_edit().expect("undo TOC insertion");
    assert_eq!(app.document.blocks.len(), 1);
}

#[test]
fn bibliography_insert_is_one_undoable_atomic_block() {
    let mut app = app_with_paragraphs(&["before"]);
    let before = block_id(&app, 0);
    app.dispatch_command(
        "insert_bibliography_after",
        json!({ "afterBlockId": before }),
    )
    .expect("bibliography inserts");
    assert!(matches!(
        app.document.blocks[1].kind,
        BlockKind::Bibliography
    ));
    app.undo_current_edit()
        .expect("undo bibliography insertion");
    assert_eq!(app.document.blocks.len(), 1);
}

#[test]
fn anchored_endnote_inserts_at_the_requested_caret_and_undoes_as_one_gesture() {
    let mut app = app_with_paragraphs(&["before"]);
    let block_id = block_id(&app, 0);
    let after_inline_id = match &app.document.blocks[0].content[0] {
        Inline::Text { id, .. } => id.to_string(),
        inline => panic!("expected text inline, got {inline:?}"),
    };

    app.dispatch_command(
        "insert_endnote_ref_after",
        json!({ "blockId": block_id, "afterInlineId": after_inline_id }),
    )
    .expect("endnote inserts at caret");

    assert_eq!(app.document.blocks.len(), 1, "no synthetic paragraph");
    let note_id = match app.document.blocks[0].content.last() {
        Some(Inline::FootnoteRef { footnote_id, .. }) => footnote_id.clone(),
        inline => panic!("expected endnote reference, got {inline:?}"),
    };
    assert!(app.document.endnote_ids.contains(&note_id));
    assert!(app.document.footnotes.iter().any(|note| note.id == note_id));

    app.undo_current_edit().expect("undo endnote gesture");
    assert!(app.document.endnote_ids.is_empty());
    assert!(app.document.footnotes.iter().all(|note| note.deleted));
    assert_eq!(app.document.blocks[0].content.len(), 1);
}

fn list_ids(app: &OpenDocApp) -> Vec<Option<String>> {
    app.document
        .blocks
        .iter()
        .map(|block| block.list_id().map(StableId::to_string))
        .collect()
}

#[test]
fn appended_list_items_share_one_run_and_a_paragraph_starts_a_new_one() {
    let mut app = OpenDocApp::new_sample();
    app.new_document("Lists");
    app.document.blocks.clear();

    app.add_list_item("one", 0, "bullet").unwrap();
    app.add_list_item("two", 0, "bullet").unwrap();
    app.add_paragraph("between").expect("paragraph");
    app.add_list_item("three", 0, "bullet").unwrap();
    app.add_list_item("four", 0, "bullet").unwrap();

    let ids = list_ids(&app);
    assert_eq!(ids[0], ids[1], "adjacent items are one list");
    assert_eq!(ids[2], None);
    assert_eq!(ids[3], ids[4]);
    assert_ne!(
        ids[0], ids[3],
        "a paragraph between two lists must not leave them sharing an id"
    );
    assert!(ids[0].as_deref().is_some_and(|id| id != "list-main"));
    app.document.validate().unwrap();
}

#[test]
fn ordered_list_start_is_run_level_source_state_and_undo_restores_default() {
    let mut app = OpenDocApp::new_sample();
    app.new_document("Lists");
    app.document.blocks.clear();
    app.add_list_item("one", 0, "ordered").unwrap();
    app.add_list_item("two", 0, "ordered").unwrap();
    let second = block_id(&app, 1);
    let list_id = app.document.blocks[0].list_id().unwrap().clone();

    app.dispatch_command(
        "set_ordered_list_start",
        json!({ "blockId": second, "start": 7 }),
    )
    .unwrap();
    assert_eq!(app.document.list_properties[&list_id].start_for(0), 7);
    assert_eq!(app.operation_journal.last().unwrap().kind, "set-list-start");

    app.undo_current_edit().unwrap();
    assert_eq!(
        app.document
            .list_properties
            .get(&list_id)
            .map(|p| p.start_for(0)),
        None,
        "undo canonicalises the default by removing the empty run property"
    );
}

#[test]
fn ordered_list_format_is_run_level_source_state_and_undo_restores_inheritance() {
    let mut app = OpenDocApp::new_sample();
    app.new_document("Lists");
    app.document.blocks.clear();
    app.add_list_item("one", 0, "ordered").unwrap();
    let id = block_id(&app, 0);
    let list_id = app.document.blocks[0].list_id().unwrap().clone();

    app.dispatch_command(
        "set_ordered_list_format",
        json!({ "blockId": id, "format": "upper-roman" }),
    )
    .unwrap();
    assert_eq!(
        app.document.list_properties[&list_id].format_for(0),
        opendoc_core::OrderedListFormat::UpperRoman
    );
    assert_eq!(
        app.operation_journal.last().unwrap().kind,
        "set-list-format"
    );

    app.undo_current_edit().unwrap();
    assert!(!app.document.list_properties.contains_key(&list_id));
}

#[test]
fn ordered_list_start_rejects_bullets_and_zero() {
    let mut app = OpenDocApp::new_sample();
    app.new_document("Lists");
    app.document.blocks.clear();
    app.add_list_item("one", 0, "bullet").unwrap();
    let id = block_id(&app, 0);
    assert!(app.set_ordered_list_start(&id, 3).is_err());
    assert!(app.set_ordered_list_start(&id, 0).is_err());
}

#[test]
fn bookmark_command_is_journalled_and_delete_writes_a_tombstone() {
    let mut app = app_with_paragraphs(&["target", "retarget"]);
    let target = block_id(&app, 0);
    app.dispatch_command(
        "set_bookmark",
        json!({ "bookmarkId": null, "name": "intro_target", "blockId": target }),
    )
    .expect("public bookmark command creates a target");
    let bookmark = app.document.bookmarks[0].clone();
    assert_eq!(
        app.operation_journal.last().unwrap().kind,
        "upsert-bookmark"
    );
    let retarget = block_id(&app, 1);
    app.dispatch_command(
        "set_bookmark",
        json!({ "bookmarkId": bookmark.id, "name": "intro_target", "blockId": retarget }),
    )
    .expect("existing bookmark can retarget through the public command");
    assert_eq!(app.document.bookmarks[0].block_id.as_str(), retarget);
    app.dispatch_command("delete_bookmark", json!({ "bookmarkId": bookmark.id }))
        .expect("public delete command tombstones the bookmark");
    assert!(app
        .document
        .bookmarks
        .iter()
        .any(|item| item.id == bookmark.id && item.deleted));
}

#[test]
fn inserting_a_list_item_after_an_item_joins_that_run() {
    let mut app = OpenDocApp::new_sample();
    app.new_document("Lists");
    app.document.blocks.clear();
    app.add_list_item("one", 0, "bullet").unwrap();
    app.add_paragraph("after").expect("paragraph");

    let anchor = block_id(&app, 0);
    app.insert_list_item_after(anchor, "one and a half", 0, "bullet")
        .unwrap();
    let ids = list_ids(&app);
    assert_eq!(ids[0], ids[1]);
    assert_eq!(ids[2], None);
}

#[test]
fn title_and_subtitle_styles_are_durable() {
    let mut app = app_with_paragraphs(&["named style"]);
    let id = block_id(&app, 0);
    app.set_block_text_style(&id, "title", 0, "bullet").unwrap();
    assert!(matches!(app.document.blocks[0].kind, BlockKind::Title));
    app.set_block_text_style(&id, "subtitle", 0, "bullet")
        .unwrap();
    assert!(matches!(app.document.blocks[0].kind, BlockKind::Subtitle));
}

#[test]
fn converting_a_selection_of_paragraphs_makes_one_list_not_three() {
    let mut app = app_with_paragraphs(&["a", "b", "c", "d"]);
    for index in 0..3 {
        app.set_block_text_style(block_id(&app, index), "list-item", 0, "bullet")
            .unwrap();
    }
    // Converted one at a time each block still joins the run in front of it.
    let ids = list_ids(&app);
    assert_eq!(ids[0], ids[1]);
    assert_eq!(ids[1], ids[2]);
    assert_eq!(ids[3], None);

    let mut app = app_with_paragraphs(&["a", "b", "c", "d"]);
    let selection = EditorSelection {
        anchor: EditorPosition {
            block_id: block_id(&app, 0),
            inline_id: None,
            offset: 0,
        },
        focus: EditorPosition {
            block_id: block_id(&app, 2),
            inline_id: None,
            offset: 1,
        },
    };
    app.set_editor_selection_block_style(selection, "list-item", 0, "ordered")
        .unwrap();
    let ids = list_ids(&app);
    assert_eq!(ids[0], ids[1]);
    assert_eq!(ids[1], ids[2]);
    assert_eq!(ids[3], None);
    app.document.validate().unwrap();
}

#[test]
fn leaving_a_list_in_the_middle_splits_the_run_in_two() {
    let mut app = OpenDocApp::new_sample();
    app.new_document("Lists");
    app.document.blocks.clear();
    for text in ["one", "two", "three", "four"] {
        app.add_list_item(text, 0, "ordered").unwrap();
    }
    let before = list_ids(&app);
    assert_eq!(before[0], before[3], "one run to start with");

    app.set_block_text_style(block_id(&app, 1), "paragraph", 0, "bullet")
        .unwrap();

    let after = list_ids(&app);
    assert_eq!(after[0], before[0], "the head keeps the original run");
    assert_eq!(after[1], None);
    assert_eq!(after[2], after[3], "the tail is one run of its own");
    assert_ne!(
        after[0], after[2],
        "numbering must not keep counting across the paragraph"
    );
    app.document.validate().unwrap();
}

#[test]
fn two_cuts_in_one_run_leave_three_distinct_runs() {
    let mut app = OpenDocApp::new_sample();
    app.new_document("Lists");
    app.document.blocks.clear();
    for text in ["one", "two", "three", "four", "five"] {
        app.add_list_item(text, 0, "ordered").unwrap();
    }
    let selection = EditorSelection {
        anchor: EditorPosition {
            block_id: block_id(&app, 1),
            inline_id: None,
            offset: 0,
        },
        focus: EditorPosition {
            block_id: block_id(&app, 3),
            inline_id: None,
            offset: 1,
        },
    };
    // Items 1 and 3 leave; item 2 survives between them.
    app.set_block_text_style(block_id(&app, 1), "paragraph", 0, "bullet")
        .unwrap();
    let _ = selection;
    app.set_block_text_style(block_id(&app, 3), "paragraph", 0, "bullet")
        .unwrap();

    let ids = list_ids(&app);
    assert_eq!(ids[1], None);
    assert_eq!(ids[3], None);
    let runs = [
        ids[0].clone().unwrap(),
        ids[2].clone().unwrap(),
        ids[4].clone().unwrap(),
    ];
    assert_eq!(
        runs.iter().collect::<BTreeSet<_>>().len(),
        3,
        "three separated segments are three lists"
    );
}

#[test]
fn indenting_a_list_item_keeps_its_marker_and_checkbox_state() {
    let mut app = OpenDocApp::new_sample();
    app.new_document("Lists");
    app.document.blocks.clear();
    app.add_list_item("todo", 0, "checklist").unwrap();
    app.set_list_item_checked(block_id(&app, 0), true).unwrap();

    let selection = EditorSelection {
        anchor: EditorPosition {
            block_id: block_id(&app, 0),
            inline_id: None,
            offset: 0,
        },
        focus: EditorPosition {
            block_id: block_id(&app, 0),
            inline_id: None,
            offset: 0,
        },
    };
    app.adjust_editor_selection_list_indent(selection, 1)
        .unwrap();

    assert_eq!(
        app.document.blocks[0].list_kind(),
        Some(ListKind::Checklist { checked: true }),
        "indenting must not retype the marker or lose the checkbox"
    );
    let BlockKind::ListItem { level, .. } = app.document.blocks[0].kind else {
        unreachable!()
    };
    assert_eq!(level, 1);
    // The projection tells a checklist from a bullet even though `ordered`
    // is false for both.
    let block = &app.document().blocks[0];
    assert_eq!(block.ordered, Some(false));
    assert_eq!(block.list_kind.as_deref(), Some("checklist"));
    assert_eq!(block.checked, Some(true));
}

#[test]
fn block_properties_are_set_cleared_journalled_and_projected() {
    let mut app = app_with_paragraphs(&["formatted"]);
    let id = block_id(&app, 0);

    app.set_block_property(&id, BlockProperty::Alignment(Alignment::Center))
        .unwrap();
    app.set_block_property(
        &id,
        BlockProperty::IndentFirstLine(Length::from_points(-18.0).unwrap()),
    )
    .unwrap();
    app.set_block_property(
        &id,
        BlockProperty::LineSpacing(LineSpacing::multiple(1.5).unwrap()),
    )
    .unwrap();

    let properties = &app.document.blocks[0].properties;
    assert_eq!(properties.alignment, Some(Alignment::Center));
    assert_eq!(properties.hanging_indent(), Length::from_points(18.0).ok());

    // The envelope kind is derived from the payload, never hand-written.
    let kinds = app
        .operation_journal
        .iter()
        .map(|record| record.kind.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        kinds,
        vec![
            "set-block-property",
            "set-block-property",
            "set-block-property"
        ]
    );
    app.snapshot_document().validate_source().unwrap();

    let projected = &app.document().blocks[0].properties;
    assert_eq!(projected.alignment.as_deref(), Some("center"));
    assert_eq!(projected.indent_first_line_twips, Some(-360));
    assert_eq!(projected.line_spacing_mode.as_deref(), Some("multiple"));
    assert_eq!(projected.line_spacing_value, Some(1_500));
    // How that spacing reads is a projection rule, so the DTO carries the
    // label and no view has to work out what "multiple:1500" means.
    assert_eq!(projected.line_spacing_label.as_deref(), Some("1.5\u{d7}"));

    app.clear_block_property(&id, BlockPropertyKey::Alignment)
        .unwrap();
    assert_eq!(app.document.blocks[0].properties.alignment, None);
    assert_eq!(
        app.operation_journal
            .last()
            .map(|record| record.kind.clone()),
        Some("clear-block-property".to_string())
    );
}

#[test]
fn block_properties_reject_unknown_blocks_and_impossible_values() {
    let mut app = app_with_paragraphs(&["formatted"]);
    assert!(app
        .set_block_property("block-nope", BlockProperty::Alignment(Alignment::Start))
        .is_err());
    assert!(app
        .clear_block_property("block-nope", BlockPropertyKey::Alignment)
        .is_err());
    assert!(Length::from_points(2_000.0).is_err());
    assert!(LineSpacing::multiple(0.0).is_err());
}

#[test]
fn a_document_carrying_block_properties_round_trips_through_the_projection() {
    let mut app = app_with_paragraphs(&["formatted"]);
    let id = block_id(&app, 0);
    app.set_block_property(&id, BlockProperty::Direction(TextDirection::RightToLeft))
        .unwrap();
    app.set_block_property(
        &id,
        BlockProperty::SpaceAfter(Length::from_points(6.0).unwrap()),
    )
    .unwrap();
    app.set_block_property(
        &id,
        BlockProperty::LineSpacing(
            LineSpacing::exactly(Length::from_points(14.0).unwrap()).unwrap(),
        ),
    )
    .unwrap();

    let projected = app.document();
    let restored = projected.to_core().expect("projection parses back");
    assert_eq!(
        restored.blocks[0].properties,
        app.document.blocks[0].properties
    );
}
fn caret_on(app: &OpenDocApp, index: usize) -> EditorSelection {
    let position = EditorPosition {
        block_id: block_id(app, index),
        inline_id: None,
        offset: 0,
    };
    EditorSelection {
        anchor: position.clone(),
        focus: position,
    }
}

/// B3/B6: the command surface names the marker, so a checklist is
/// reachable without a "was it checked before?" heuristic.
#[test]
fn checklists_are_created_ticked_and_converted_by_name() {
    let mut app = app_with_paragraphs(&["task"]);
    let id = block_id(&app, 0);

    app.set_block_text_style(&id, "list-item", 0, "checklist")
        .unwrap();
    let block = &app.document().blocks[0];
    assert_eq!(block.list_kind.as_deref(), Some("checklist"));
    assert_eq!(block.checked, Some(false), "a new checklist item is open");

    app.set_list_item_checked(&id, true).unwrap();
    assert_eq!(app.document().blocks[0].checked, Some(true));

    // Round-tripping through bullets must not resurrect the tick: the
    // marker is what the command carries, and nothing remembers a
    // checkbox that the block no longer has.
    app.set_block_text_style(&id, "list-item", 0, "bullet")
        .unwrap();
    assert_eq!(app.document().blocks[0].checked, None);
    app.set_block_text_style(&id, "list-item", 0, "checklist")
        .unwrap();
    assert_eq!(app.document().blocks[0].checked, Some(false));

    // A bullet has no checkbox, so ticking it is an error rather than a
    // silent no-op.
    app.set_block_text_style(&id, "list-item", 0, "bullet")
        .unwrap();
    assert!(app.set_list_item_checked(&id, true).is_err());
    assert!(app
        .set_block_text_style(&id, "list-item", 0, "sticker")
        .is_err());
}

/// B3: the named commands go through the model's smart constructors, so
/// an unknown name or an out-of-range length is refused at the boundary.
#[test]
fn named_block_property_commands_parse_validate_and_clear() {
    let mut app = app_with_paragraphs(&["formatted"]);
    let id = block_id(&app, 0);

    app.set_block_alignment(&id, "center").unwrap();
    app.set_block_indent_start(&id, 720).unwrap();
    app.set_block_indent_end(&id, 360).unwrap();
    app.set_block_indent_first_line(&id, -360).unwrap();
    app.set_block_space_before(&id, 120).unwrap();
    app.set_block_space_after(&id, 240).unwrap();
    app.set_block_line_spacing(&id, "multiple", 1_500).unwrap();
    app.set_block_direction(&id, "rtl").unwrap();

    let properties = &app.document.blocks[0].properties;
    assert_eq!(properties.alignment, Some(Alignment::Center));
    assert_eq!(
        properties.indent_start,
        Some(Length::from_twips(720).unwrap())
    );
    assert_eq!(
        properties.indent_end,
        Some(Length::from_twips(360).unwrap())
    );
    assert_eq!(
        properties.hanging_indent(),
        Some(Length::from_twips(360).unwrap()),
        "a negative first-line indent reads back as a hanging indent"
    );
    assert_eq!(
        properties.space_before,
        Some(Length::from_twips(120).unwrap())
    );
    assert_eq!(
        properties.space_after,
        Some(Length::from_twips(240).unwrap())
    );
    assert_eq!(
        properties.line_spacing,
        Some(LineSpacing::multiple(1.5).unwrap())
    );
    assert_eq!(properties.direction, Some(TextDirection::RightToLeft));

    app.set_block_line_spacing(&id, "exact", 480).unwrap();
    assert_eq!(
        app.document.blocks[0].properties.line_spacing,
        Some(LineSpacing::exactly(Length::from_twips(480).unwrap()).unwrap())
    );

    assert!(app.set_block_alignment(&id, "sideways").is_err());
    assert!(app.set_block_direction(&id, "sideways").is_err());
    assert!(app.set_block_line_spacing(&id, "roomy", 2).is_err());
    assert!(app.set_block_line_spacing(&id, "multiple", -1).is_err());
    assert!(
        app.set_block_indent_start(&id, 1_000_000).is_err(),
        "a length past the model's range never reaches the document"
    );

    app.clear_named_block_property(&id, "alignment").unwrap();
    assert_eq!(app.document.blocks[0].properties.alignment, None);
    assert!(app.clear_named_block_property(&id, "colour").is_err());
}

/// B3/B9: one toolbar gesture, two document meanings — the choice is made
/// in Rust from the block's kind, not by the caller.
#[test]
fn indent_moves_list_levels_and_shifts_paragraph_indents() {
    let mut app = app_with_paragraphs(&["prose"]);
    let paragraph = block_id(&app, 0);

    app.adjust_editor_selection_indent(caret_on(&app, 0), 1)
        .unwrap();
    assert_eq!(
        app.document.blocks[0].properties.indent_start,
        Some(Length::from_twips(INDENT_STEP_TWIPS).unwrap())
    );
    app.adjust_editor_selection_indent(caret_on(&app, 0), 1)
        .unwrap();
    assert_eq!(
        app.document.blocks[0].properties.indent_start,
        Some(Length::from_twips(2 * INDENT_STEP_TWIPS).unwrap())
    );

    // Outdenting past zero returns the block to inheriting rather than
    // storing an explicit zero or bleeding into the margin.
    for _ in 0..5 {
        app.adjust_editor_selection_indent(caret_on(&app, 0), -1)
            .unwrap();
    }
    assert_eq!(app.document.blocks[0].properties.indent_start, None);

    app.set_block_text_style(&paragraph, "list-item", 0, "checklist")
        .unwrap();
    app.adjust_editor_selection_indent(caret_on(&app, 0), 1)
        .unwrap();
    let BlockKind::ListItem { level, kind, .. } = app.document.blocks[0].kind else {
        unreachable!()
    };
    assert_eq!(level, 1, "a list item indents by nesting level");
    assert_eq!(kind, ListKind::Checklist { checked: false });
    assert_eq!(
        app.document.blocks[0].properties.indent_start, None,
        "indenting a list item leaves the block indent alone"
    );
}

// ---- Page setup and page furniture (PLAN77 B7) ----------------------

#[test]
fn page_setup_is_written_whole_journalled_and_projected() {
    let mut app = app_with_paragraphs(&["body"]);
    // A header offset that is not the default, so carrying it through is
    // distinguishable from resetting it.
    app.document.page_setup = app
        .document
        .page_setup
        .with_furniture_margins(
            opendoc_core::Length::from_twips(1000).unwrap(),
            opendoc_core::Length::from_twips(1100).unwrap(),
        )
        .unwrap();
    // A4 portrait with 2cm margins.
    let two_cm = opendoc_core::Length::from_centimeters(2.0).unwrap().twips();
    app.set_page_setup(11906, 16838, two_cm, two_cm, two_cm, two_cm)
        .unwrap();

    assert_eq!(app.document.page_setup.width.twips(), 11906);
    assert_eq!(app.document.page_setup.margin_start.twips(), two_cm);
    // The header/footer offsets are not in the page-setup dialog, so they
    // are carried through rather than reset to the default.
    assert_eq!(app.document.page_setup.margin_header.twips(), 1000);
    assert_eq!(app.document.page_setup.margin_footer.twips(), 1100);

    assert_eq!(
        app.operation_journal
            .last()
            .map(|record| record.kind.clone()),
        Some("set-page-setup".to_string())
    );

    let projected = app.document();
    assert_eq!(projected.page_setup.width_twips, 11906);
    // Derived, not stored: the document never said "A4".
    assert_eq!(projected.page_layout.size_name.as_deref(), Some("a4"));
    assert_eq!(projected.page_layout.orientation, "portrait");
    assert!(
        projected
            .page_layout
            .style
            .contains("--page-width: 595.30pt;"),
        "{}",
        projected.page_layout.style
    );
    assert!(projected
        .page_layout
        .size_presets
        .iter()
        .any(|preset| preset.name == "a4" && preset.width_twips == 11906));
    app.snapshot_document().validate_source().unwrap();
}

#[test]
fn the_page_size_preset_table_never_reaches_a_snapshot() {
    let app = app_with_paragraphs(&["body"]);
    // The presets are a UI affordance. They are projection-only, so they
    // must not be signed along with the document.
    assert!(app.document().page_layout.size_presets.len() > 1);
    assert!(app.snapshot_document().page_layout.size_presets.is_empty());
    assert!(app.snapshot_document().page_layout.style.is_empty());
}

#[test]
fn page_orientation_rotates_once_however_often_it_is_asked() {
    let mut app = app_with_paragraphs(&["body"]);
    app.set_page_setup(11906, 16838, 1440, 1440, 1440, 1440)
        .unwrap();
    app.set_page_orientation("landscape").unwrap();
    assert_eq!(app.document.page_setup.width.twips(), 16838);
    app.set_page_orientation("landscape").unwrap();
    assert_eq!(
        app.document.page_setup.width.twips(),
        16838,
        "asking for the orientation the page already has must not rotate it again"
    );
    app.set_page_orientation("portrait").unwrap();
    assert_eq!(app.document.page_setup.width.twips(), 11906);
    assert!(app.set_page_orientation("sideways").is_err());
}

#[test]
fn impossible_page_geometry_is_refused_at_the_command_boundary() {
    let mut app = app_with_paragraphs(&["body"]);
    let before = app.document.page_setup;
    // Side margins wider than the sheet.
    assert!(app
        .set_page_setup(12240, 15840, 1440, 1440, 8000, 8000)
        .is_err());
    // A negative margin.
    assert!(app
        .set_page_setup(12240, 15840, -20, 1440, 1440, 1440)
        .is_err());
    // A page wider than the model's ±22in range.
    assert!(app
        .set_page_setup(99_999, 15840, 1440, 1440, 1440, 1440)
        .is_err());
    assert_eq!(app.document.page_setup, before);
    assert!(app.operation_journal.is_empty());
}

#[test]
fn a_footer_carries_text_and_an_unresolved_page_number_field() {
    let mut app = app_with_paragraphs(&["body"]);
    app.set_page_furniture("footer", "Page", "page-number", "center")
        .unwrap();

    assert_eq!(app.document.footer.len(), 1);
    let block = &app.document.footer[0];
    assert_eq!(block.properties.alignment, Some(Alignment::Center));
    assert!(matches!(
        block.content.as_slice(),
        [
            Inline::Text { text, .. },
            Inline::PageNumber {
                field: opendoc_core::PageNumberField::CurrentPage,
                ..
            }
        ] if text == "Page "
    ));
    // Furniture is not body flow: it must not appear in the body or its
    // text counts.
    assert!(!app.document.visible_text().contains("Page"));
    assert_eq!(
        app.operation_journal
            .last()
            .map(|record| record.kind.clone()),
        Some("set-page-furniture".to_string())
    );

    let projected = app.document();
    assert!(projected.footer_html.contains("data-field=\"page-number\""));
    assert!(projected.body_html().is_empty() || !projected.body_html().contains("doc-page-number"));
    assert_eq!(projected.footer.len(), 1);
    assert!(projected.header_html.is_empty());
    app.snapshot_document().validate_source().unwrap();

    app.clear_page_furniture("footer").unwrap();
    assert!(app.document.footer.is_empty());
    assert!(app.document().footer_html.is_empty());
}

#[test]
fn first_page_furniture_undo_and_inherit_preserve_absent_override() {
    let mut app = app_with_paragraphs(&["body"]);
    app.dispatch_command(
        "set_page_furniture",
        json!({ "slot": "header", "text": "Ordinary", "field": "none", "alignment": "start" }),
    )
    .unwrap();
    app.dispatch_command(
        "set_page_furniture",
        json!({ "slot": "first-page-header", "text": "First", "field": "none", "alignment": "start" }),
    )
    .unwrap();
    assert!(app
        .document
        .has_furniture_override(opendoc_core::HeaderFooterSlot::FirstPageHeader));

    // `None` means inheritance, whereas `Some(vec![])` suppresses the
    // ordinary header. Undo must restore the former exact state.
    app.undo_current_edit().unwrap();
    assert!(!app
        .document
        .has_furniture_override(opendoc_core::HeaderFooterSlot::FirstPageHeader));
    assert_eq!(
        app.document
            .furniture_for_page(opendoc_core::HeaderFooterSlot::Header, 0),
        app.document.header.as_slice()
    );

    app.dispatch_command(
        "set_page_furniture",
        json!({ "slot": "first-page-header", "text": "First", "field": "none", "alignment": "start" }),
    )
    .unwrap();
    app.dispatch_command(
        "clear_page_furniture_override",
        json!({ "slot": "first-page-header" }),
    )
    .unwrap();
    assert!(!app
        .document
        .has_furniture_override(opendoc_core::HeaderFooterSlot::FirstPageHeader));
    app.undo_current_edit().unwrap();
    assert_eq!(
        app.document
            .furniture(opendoc_core::HeaderFooterSlot::FirstPageHeader)
            .len(),
        1,
        "undoing inherit restores the explicit first-page fragment"
    );
    assert!(app
        .dispatch_command("clear_page_furniture_override", json!({ "slot": "header" }))
        .is_err());
}

#[test]
fn page_furniture_lines_become_distinct_paragraph_blocks() {
    let mut app = app_with_paragraphs(&["body"]);
    app.set_page_furniture(
        "header",
        "Running head\n\nConfidential",
        "page-count",
        "end",
    )
    .unwrap();

    assert_eq!(app.document.header.len(), 3);
    assert!(matches!(
        app.document.header[0].content.as_slice(),
        [Inline::Text { text, .. }] if text == "Running head"
    ));
    assert!(
        app.document.header[1].content.is_empty(),
        "blank line is a paragraph"
    );
    assert!(matches!(
        app.document.header[2].content.as_slice(),
        [Inline::Text { text, .. }, Inline::PageNumber { field: opendoc_core::PageNumberField::PageCount, .. }]
        if text == "Confidential "
    ));
    assert!(app
        .document
        .header
        .iter()
        .all(|block| block.properties.alignment == Some(Alignment::End)));
    app.snapshot_document().validate_source().unwrap();
}

#[test]
fn rich_html_furniture_keeps_supported_structure_and_is_one_undoable_edit() {
    let mut app = app_with_paragraphs(&["body"]);
    app.dispatch_command(
        "set_page_furniture_html",
        json!({
            "slot": "header",
            "html": "<h2 style=\"text-align:center;direction:rtl\">Running <em>head</em></h2><ol><li>first</li><li><a href=\"https://example.test\">second</a></li></ol>"
        }),
    )
    .unwrap();

    assert!(matches!(
        app.document.header[0].kind,
        BlockKind::Heading { level: 2 }
    ));
    assert_eq!(
        app.document.header[0].properties.alignment,
        Some(Alignment::Center)
    );
    assert_eq!(
        app.document.header[0].properties.direction,
        Some(opendoc_core::TextDirection::RightToLeft)
    );
    assert!(matches!(
        app.document.header[1].kind,
        BlockKind::ListItem {
            kind: ListKind::Ordered,
            ..
        }
    ));
    assert!(matches!(
        app.document.header[2].kind,
        BlockKind::ListItem {
            kind: ListKind::Ordered,
            ..
        }
    ));
    assert!(matches!(
        app.document.header[2].content.as_slice(),
        [Inline::Link { href, .. }] if href == "https://example.test"
    ));
    assert!(app.document().header_html.contains("<ol"));
    assert!(app
        .document()
        .header_html
        .contains("text-align:center;direction:rtl;"));
    app.snapshot_document().validate_source().unwrap();

    app.undo_current_edit().unwrap();
    assert!(
        app.document.header.is_empty(),
        "rich replacement is one undo step"
    );
}

#[test]
fn rich_html_furniture_imports_a_native_standalone_table() {
    let mut app = app_with_paragraphs(&["body"]);
    app.set_page_furniture_html(
        "footer",
        "<table><tr><th>H</th><td><strong>V</strong></td></tr></table>",
    )
    .unwrap();

    assert!(matches!(
        app.document.footer[0].kind,
        BlockKind::Table { .. }
    ));
    assert!(app.document().footer_html.contains("<table"));
    app.snapshot_document().validate_source().unwrap();
}

#[test]
fn rich_html_furniture_refuses_degrading_objects_without_changing_the_slot() {
    let mut app = app_with_paragraphs(&["body"]);
    app.set_page_furniture("footer", "Keep", "none", "start")
        .unwrap();
    let before = app.document.footer.clone();

    let error = app
        .set_page_furniture_html("footer", "<p>new</p><svg><circle /></svg>")
        .unwrap_err();
    assert!(error.to_string().contains("was not applied"));
    assert_eq!(app.document.footer, before);
}

#[test]
fn page_furniture_refuses_an_empty_request_and_an_unknown_slot() {
    let mut app = app_with_paragraphs(&["body"]);
    assert!(app
        .set_page_furniture("footer", "  ", "none", "start")
        .is_err());
    assert!(app
        .set_page_furniture("watermark", "x", "none", "start")
        .is_err());
    assert!(app
        .set_page_furniture("footer", "x", "section-number", "start")
        .is_err());
    assert!(app.clear_page_furniture("watermark").is_err());
    assert!(app.document.footer.is_empty());
}

#[test]
fn page_setup_and_furniture_survive_the_app_document_round_trip() {
    let mut app = app_with_paragraphs(&["body"]);
    app.set_page_setup(16838, 11906, 720, 720, 1080, 1080)
        .unwrap();
    app.set_page_furniture("header", "Draft", "page-count", "end")
        .unwrap();
    let snapshot = app.snapshot_document();
    let restored = snapshot.to_core().unwrap();
    restored.validate().unwrap();
    assert_eq!(restored.page_setup, app.document.page_setup);
    assert_eq!(restored.header, app.document.header);
    assert_eq!(restored.footer, app.document.footer);
}

#[test]
fn concurrent_page_edits_converge_last_writer_wins_per_slot() {
    // Page geometry and each furniture slot are separate merge keys, so
    // two actors editing different parts of the page both keep their edit;
    // two editing the same part converge on one of them, whichever the
    // causal order puts last, rather than on a blend of both.
    let mut app = app_with_paragraphs(&["body"]);
    app.set_page_setup(12240, 15840, 1440, 1440, 1440, 1440)
        .unwrap();
    let base = app.document.clone();

    let a4 = opendoc_core::PageSetup::from_size_name("a4").unwrap();
    let legal = opendoc_core::PageSetup::from_size_name("legal").unwrap();
    let mut header = Block::paragraph("Running head");
    header.id = StableId::parse("block-header-run").unwrap();
    header.content = vec![Inline::Text {
        id: StableId::parse("text-header-run").unwrap(),
        text: "Running head".to_string(),
        marks: Vec::new(),
    }];

    let op = |actor: &str, seq: u64, kind: OperationKind| opendoc_merge::Operation {
        id: opendoc_merge::OperationId {
            actor: opendoc_merge::ActorId(actor.to_string()),
            seq,
        },
        kind,
        context: None,
    };
    let set_a4 = op("a", 1, OperationKind::SetPageSetup { page_setup: a4 });
    let set_legal = op("b", 1, OperationKind::SetPageSetup { page_setup: legal });
    let set_header = op(
        "b",
        2,
        OperationKind::SetPageFurniture {
            slot: opendoc_core::HeaderFooterSlot::Header,
            blocks: vec![header.clone()],
        },
    );

    let forward = opendoc_merge::merge_operations(
        &base,
        &[
            vec![set_a4.clone()],
            vec![set_legal.clone(), set_header.clone()],
        ],
    )
    .unwrap();
    let reverse =
        opendoc_merge::merge_operations(&base, &[vec![set_legal], vec![set_a4], vec![set_header]])
            .unwrap();
    assert_eq!(forward.document, reverse.document);
    // The geometry is one of the two, never a blend: a page cannot end up
    // with A4's width and Legal's height.
    let merged = forward.document.page_setup;
    assert!(merged == a4 || merged == legal, "{merged:?}");
    // The unrelated slot edit survived whichever geometry won.
    assert_eq!(forward.document.header, vec![header]);
    forward.document.validate().unwrap();
}

// ---- Table structure (PLAN77 E2, ADR 0013) --------------------------

/// An app holding one 2x2 table, with the ids a table command needs.
fn app_with_table() -> (OpenDocApp, String) {
    let mut app = app_with_paragraphs(&["before"]);
    let after = block_id(&app, 0);
    app.insert_table_after(&after).unwrap();
    let table_block_id = app.document.blocks[1].id.to_string();
    (app, table_block_id)
}

fn table_of(app: &OpenDocApp) -> (&[opendoc_core::TableColumn], &[opendoc_core::TableRow]) {
    match &app.document.blocks[1].kind {
        opendoc_core::BlockKind::Table { columns, rows, .. } => (columns, rows),
        other => panic!("expected a table, got {other:?}"),
    }
}

#[test]
fn column_commands_keep_the_grid_rectangular() {
    let (mut app, table) = app_with_table();
    let (columns, rows) = table_of(&app);
    assert_eq!(columns.len(), 2);
    let first_column = columns[0].id.to_string();
    let width_before: Vec<usize> = rows.iter().map(|row| row.cells.len()).collect();
    assert_eq!(width_before, vec![2, 2]);

    app.insert_table_column(&table, Some(first_column.clone()))
        .unwrap();
    let (columns, rows) = table_of(&app);
    assert_eq!(columns.len(), 3);
    assert!(rows.iter().all(|row| row.cells.len() == 3));
    app.document.validate().unwrap();

    app.delete_table_column(&table, &first_column).unwrap();
    let (columns, rows) = table_of(&app);
    assert_eq!(columns.len(), 2);
    assert!(rows.iter().all(|row| row.cells.len() == 2));
    assert!(!app.document.visible_text().contains("A1"));
    app.document.validate().unwrap();
}

/// OB-7: a row added to a two-column table is a two-column row, so no
/// ordinary edit leaves a ragged table for merge to repair and warn about.
#[test]
fn an_added_row_is_as_wide_as_the_table() {
    let (mut app, table) = app_with_table();
    app.add_table_row(&table, None, "new").unwrap();
    let (columns, rows) = table_of(&app);
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().all(|row| row.cells.len() == columns.len()));
    app.document.validate().unwrap();
    assert!(
        !app.document
            .warnings
            .iter()
            .any(|warning| warning.code == "table-geometry-repaired"),
        "adding a row is not a geometry collision: {:?}",
        app.document.warnings
    );
    assert!(app.document.visible_text().contains("new"));
}

/// OB-21: "insert above the first row" and "insert left of the first column".
/// Neither could be expressed while the anchor was `Option<id>`, because
/// `None` there means *append*; the contract's `first` keyword is the one
/// position an id cannot name.
#[test]
fn a_row_and_a_column_can_be_inserted_before_the_first_one() {
    let (mut app, table) = app_with_table();
    let first_column_before = table_of(&app).0[0].id.to_string();
    let first_row_before = table_of(&app).1[0].id.to_string();

    app.dispatch_command(
        "add_table_row",
        serde_json::json!({ "tableBlockId": table, "afterRow": "first", "text": "top" }),
    )
    .unwrap();
    app.dispatch_command(
        "insert_table_column",
        serde_json::json!({ "tableBlockId": table, "afterColumnId": "first" }),
    )
    .unwrap();

    let (columns, rows) = table_of(&app);
    assert_eq!(rows.len(), 3);
    assert_eq!(columns.len(), 3);
    assert_ne!(
        rows[0].id.to_string(),
        first_row_before,
        "the new row is above the one that used to be first"
    );
    assert_eq!(rows[1].id.to_string(), first_row_before);
    assert_ne!(columns[0].id.to_string(), first_column_before);
    assert_eq!(columns[1].id.to_string(), first_column_before);
    assert!(rows.iter().all(|row| row.cells.len() == 3));
    app.document.validate().unwrap();

    // Omitting the anchor still means append, unchanged.
    let before: Vec<String> = table_of(&app)
        .1
        .iter()
        .map(|row| row.id.to_string())
        .collect();
    app.add_table_row(&table, None, "bottom").unwrap();
    let after: Vec<String> = table_of(&app)
        .1
        .iter()
        .map(|row| row.id.to_string())
        .collect();
    assert_eq!(after.len(), before.len() + 1);
    assert_eq!(after[..before.len()], before[..], "the new row went last");
    assert!(app.document.visible_text().contains("bottom"));
    assert!(app.document.visible_text().contains("top"));
}

#[test]
fn column_width_is_twips_and_refuses_a_hairline() {
    let (mut app, table) = app_with_table();
    let column = table_of(&app).0[0].id.to_string();

    app.set_table_column_width(&table, &column, 2880).unwrap();
    assert_eq!(table_of(&app).0[0].width.map(|w| w.twips()), Some(2880));

    assert!(app.set_table_column_width(&table, &column, 10).is_err());
    assert_eq!(
        table_of(&app).0[0].width.map(|w| w.twips()),
        Some(2880),
        "a refused width leaves the stored one alone"
    );

    app.clear_table_column_width(&table, &column).unwrap();
    assert_eq!(table_of(&app).0[0].width, None);
}

#[test]
fn row_height_is_a_minimum_in_twips_and_can_return_to_auto() {
    let (mut app, table) = app_with_table();
    let row = table_of(&app).1[0].id.to_string();

    app.set_table_row_height(&table, &row, 720).unwrap();
    assert_eq!(
        table_of(&app).1[0].height.map(|height| height.twips()),
        Some(720)
    );

    assert!(app.set_table_row_height(&table, &row, 0).is_err());
    assert_eq!(
        table_of(&app).1[0].height.map(|height| height.twips()),
        Some(720),
        "a refused height leaves the stored one alone"
    );

    app.clear_table_row_height(&table, &row).unwrap();
    assert_eq!(table_of(&app).1[0].height, None);
}

#[test]
fn table_row_header_is_typed_state_and_can_be_toggled() {
    let (mut app, table) = app_with_table();
    let row = table_of(&app).1[0].id.to_string();

    app.set_table_row_header(&table, &row, true).unwrap();
    assert!(table_of(&app).1[0].header);

    app.set_table_row_header(&table, &row, false).unwrap();
    assert!(!table_of(&app).1[0].header);
}

#[test]
fn table_sort_orders_body_rows_by_a_column_and_keeps_headers_pinned() {
    let (mut app, table) = app_with_table();
    let first = table_of(&app).1[0].id.to_string();
    let second = table_of(&app).1[1].id.to_string();
    let column = table_of(&app).0[0].id.to_string();

    app.set_table_row_header(&table, &first, true).unwrap();
    app.sort_table_rows(&table, &column, true).unwrap();
    let ids: Vec<String> = table_of(&app)
        .1
        .iter()
        .map(|row| row.id.to_string())
        .collect();
    assert_eq!(ids, vec![first.clone(), second.clone()]);

    app.set_table_row_header(&table, &first, false).unwrap();
    app.sort_table_rows(&table, &column, true).unwrap();
    let ids: Vec<String> = table_of(&app)
        .1
        .iter()
        .map(|row| row.id.to_string())
        .collect();
    assert_eq!(ids, vec![second, first]);
}

#[test]
fn table_border_is_document_state_and_can_be_explicitly_cleared() {
    let (mut app, table) = app_with_table();

    app.set_table_border(&table, "dashed", 20, "#336699")
        .unwrap();
    let block = app
        .document
        .blocks
        .iter()
        .find(|block| block.id.to_string() == table)
        .expect("table block");
    let BlockKind::Table { properties, .. } = &block.kind else {
        panic!("expected table");
    };
    let border = properties.border.expect("stated table border");
    assert_eq!(border.style(), opendoc_core::BorderStyle::Dashed);
    assert_eq!(border.width().twips(), 20);
    assert_eq!(border.color().as_hex(), "#336699");

    app.clear_table_border(&table).unwrap();
    let BlockKind::Table { properties, .. } = &app
        .document
        .blocks
        .iter()
        .find(|block| block.id.to_string() == table)
        .expect("table block")
        .kind
    else {
        panic!("expected table");
    };
    assert_eq!(properties.border, None);
}

#[test]
fn table_alignment_is_table_state_and_can_be_explicitly_cleared() {
    let (mut app, table) = app_with_table();

    app.set_table_alignment(&table, "center").unwrap();
    let BlockKind::Table { properties, .. } = &app
        .document
        .blocks
        .iter()
        .find(|block| block.id.to_string() == table)
        .expect("table block")
        .kind
    else {
        panic!("expected table");
    };
    assert_eq!(
        properties.alignment,
        Some(opendoc_core::TableAlignment::Center)
    );

    app.clear_table_alignment(&table).unwrap();
    let BlockKind::Table { properties, .. } = &app
        .document
        .blocks
        .iter()
        .find(|block| block.id.to_string() == table)
        .expect("table block")
        .kind
    else {
        panic!("expected table");
    };
    assert_eq!(properties.alignment, None);
}

#[test]
fn merging_and_splitting_a_cell_is_reversible() {
    let (mut app, _) = app_with_table();
    let cell = table_of(&app).1[0].cells[0].id.to_string();

    app.merge_table_cells(&cell, 2, 2).unwrap();
    app.document.validate().unwrap();
    let text = app.document.visible_text();
    assert!(text.contains("A1"));
    for hidden in ["B1", "A2", "B2"] {
        assert!(!text.contains(hidden), "{hidden} is covered: {text}");
    }

    app.split_table_cell(&cell).unwrap();
    app.document.validate().unwrap();
    let text = app.document.visible_text();
    for restored in ["A1", "B1", "A2", "B2"] {
        assert!(text.contains(restored), "{restored} is back: {text}");
    }

    // A 1x1 "merge" is a split by another name, and the command says so
    // rather than quietly doing nothing.
    assert!(app.merge_table_cells(&cell, 1, 1).is_err());
    assert!(app.merge_table_cells(&cell, 0, 2).is_err());
}

#[test]
fn cell_styling_is_typed_and_survives_the_source_projection() {
    let (mut app, table) = app_with_table();
    let cell = table_of(&app).1[0].cells[0].id.to_string();
    let column = table_of(&app).0[1].id.to_string();

    app.set_table_column_width(&table, &column, 1800).unwrap();
    app.merge_table_cells(&cell, 2, 1).unwrap();
    app.set_table_cell_background(&cell, "#FFEE00").unwrap();
    app.set_table_cell_border(&cell, "start", "dashed", 30, "#336699")
        .unwrap();
    app.set_table_cell_vertical_alignment(&cell, "bottom")
        .unwrap();
    app.set_table_cell_row_header(&cell, true).unwrap();
    app.set_table_cell_padding(&cell, "end", 120).unwrap();

    // The projection the repository saves and reloads must carry all of
    // it: a column width or a merge that does not round-trip is lost work.
    let projected = app.document();
    let restored = projected.to_core().expect("projection parses back");
    assert_eq!(restored.blocks[1].kind, app.document.blocks[1].kind);
    restored.validate().unwrap();

    // ... and the DTO says, in Rust, which cells the merge hides.
    let shape = projected.blocks[1].table.as_ref().expect("a table block");
    assert_eq!(shape.columns[1].width_twips, Some(1800));
    assert_eq!(shape.cells[0][0].row_span, 2);
    assert!(!shape.cells[0][0].covered);
    assert!(shape.cells[1][0].covered, "the cell under the merge");
    assert!(!shape.cells[0][1].covered);
    assert_eq!(shape.cells[0][0].properties.row_header, Some(true));

    app.clear_table_cell_property(&cell, "background").unwrap();
    assert_eq!(table_of(&app).1[0].cells[0].properties.background, None);
    assert!(table_of(&app).1[0].cells[0]
        .properties
        .vertical_alignment
        .is_some());
    app.clear_table_cell_property(&cell, "row-header").unwrap();
    assert_eq!(table_of(&app).1[0].cells[0].properties.row_header, None);

    // Values the model cannot represent are refused at the command, not
    // smuggled in as strings.
    assert!(app
        .set_table_cell_background(&cell, "rebeccapurple")
        .is_err());
    assert!(app
        .set_table_cell_border(&cell, "top", "wobbly", 20, "#000")
        .is_err());
    assert!(app.set_table_cell_padding(&cell, "middle", 20).is_err());
    assert!(app.set_table_cell_padding(&cell, "top", -20).is_err());
    assert!(app.clear_table_cell_property(&cell, "colour").is_err());
}

#[test]
fn a_merged_header_that_collides_with_a_body_block_is_rejected_not_applied() {
    let app = app_with_paragraphs(&["body"]);
    let colliding = app.document.blocks[0].clone();
    let base = app.document.clone();
    let result = opendoc_merge::merge_operations(
        &base,
        &[vec![opendoc_merge::Operation {
            id: opendoc_merge::OperationId {
                actor: opendoc_merge::ActorId("a".to_string()),
                seq: 1,
            },
            kind: OperationKind::SetPageFurniture {
                slot: opendoc_core::HeaderFooterSlot::Header,
                blocks: vec![colliding],
            },
            context: None,
        }]],
    )
    .unwrap();
    assert!(result.document.header.is_empty());
    assert!(result
        .warnings
        .iter()
        .any(|warning| warning.code == "invalid-page-furniture"));
    result.document.validate().unwrap();
}
