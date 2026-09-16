//! Google Docs JSON paragraph-formatting and list fidelity.
//!
//! Every property that can survive is asserted through import → export →
//! import, because a mapping that only works in one direction is not fidelity.
//! Every property that cannot survive is asserted to say so in a warning.

use crate::{export_google_docs_json_with_warnings, import_google_docs_json, ImportError};
use opendoc_core::{
    Alignment, Block, BlockKind, BlockProperties, Color, Document, HeaderFooterSlot, Inline,
    Length, LineSpacing, ListKind, ModelWarning, OrderedListFormat, PageNumberField, StableId,
    TextDirection,
};
use serde_json::{json, Value};

fn import(input: &Value) -> crate::ImportReport {
    import_google_docs_json("Google", input.to_string().as_bytes())
        .unwrap_or_else(|err| panic!("import failed: {err}"))
}

fn import_err(input: &Value) -> ImportError {
    import_google_docs_json("Google", input.to_string().as_bytes())
        .expect_err("import should have failed")
}

/// Exports and re-imports, returning the document that came back together with
/// every warning either direction raised.
fn round_trip(document: &Document) -> (Document, Vec<ModelWarning>) {
    let (bytes, mut warnings) =
        export_google_docs_json_with_warnings(document).expect("export failed");
    let report = import_google_docs_json(&document.title, &bytes).expect("re-import failed");
    warnings.extend(report.warnings);
    (report.document, warnings)
}

fn has(warnings: &[ModelWarning], code: &str) -> bool {
    warnings.iter().any(|warning| warning.code == code)
}

fn message(warnings: &[ModelWarning], code: &str) -> String {
    warnings
        .iter()
        .find(|warning| warning.code == code)
        .unwrap_or_else(|| panic!("warning {code} missing from {warnings:?}"))
        .message
        .clone()
}

fn paragraph(style: Value, text: &str) -> Value {
    json!({ "paragraph": {
        "paragraphStyle": style,
        "elements": [{ "textRun": { "content": text } }]
    }})
}

fn document_with(content: Vec<Value>) -> Value {
    json!({ "body": { "content": content } })
}

fn pt(points: f64) -> Value {
    json!({ "magnitude": points, "unit": "PT" })
}

fn single_text(block: &Block) -> &str {
    match block.content.as_slice() {
        [Inline::Text { text, .. }] => text,
        other => panic!("expected one plain text run, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// FM-9: paragraph formatting
// ---------------------------------------------------------------------------

#[test]
fn google_paragraph_formatting_imports_and_survives_a_round_trip() {
    let input = document_with(vec![paragraph(
        json!({
            "alignment": "JUSTIFIED",
            "direction": "RIGHT_TO_LEFT",
            "indentStart": pt(36.0),
            "indentEnd": pt(18.0),
            "indentFirstLine": pt(54.0),
            "lineSpacing": 150.0,
            "spaceAbove": pt(12.0),
            "spaceBelow": pt(6.0),
            "keepWithNext": true,
            "shading": { "backgroundColor": { "color": { "rgbColor": { "red": 0.2, "green": 0.4, "blue": 0.6 } } } }
        }),
        "Formatted",
    )]);
    let report = import(&input);
    let expected = BlockProperties {
        alignment: Some(Alignment::Justify),
        direction: Some(TextDirection::RightToLeft),
        indent_start: Some(Length::from_points(36.0).unwrap()),
        indent_end: Some(Length::from_points(18.0).unwrap()),
        // 54pt absolute minus a 36pt start indent is an 18pt first-line offset.
        indent_first_line: Some(Length::from_points(18.0).unwrap()),
        line_spacing: Some(LineSpacing::multiple(1.5).unwrap()),
        space_before: Some(Length::from_points(12.0).unwrap()),
        space_after: Some(Length::from_points(6.0).unwrap()),
        keep_with_next: Some(true),
        background: Some(Color::parse("#336699").unwrap()),
        border: None,
    };
    assert_eq!(report.document.blocks[0].properties, expected);
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);

    let (round_tripped, warnings) = round_trip(&report.document);
    assert_eq!(round_tripped.blocks[0].properties, expected);
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn uniform_google_paragraph_border_round_trips() {
    let border = json!({
        "color": { "color": { "rgbColor": { "red": 0.2, "green": 0.4, "blue": 0.6 } } },
        "width": pt(1.0),
        "padding": pt(0.0),
        "dashStyle": "DASH",
    });
    let input = document_with(vec![paragraph(
        json!({
            "borderTop": border,
            "borderBottom": border,
            "borderLeft": border,
            "borderRight": border,
        }),
        "framed",
    )]);
    let report = import(&input);
    assert_eq!(
        report.document.blocks[0].properties.border,
        Some(
            opendoc_core::CellBorder::new(
                opendoc_core::BorderStyle::Dashed,
                Length::from_points(1.0).unwrap(),
                Color::parse("#336699").unwrap(),
            )
            .unwrap()
        ),
    );
    let (round_tripped, warnings) = round_trip(&report.document);
    assert_eq!(
        round_tripped.blocks[0].properties.border,
        report.document.blocks[0].properties.border
    );
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn every_google_alignment_and_direction_value_round_trips() {
    for (google, expected) in [
        ("START", Alignment::Start),
        ("CENTER", Alignment::Center),
        ("END", Alignment::End),
        ("JUSTIFIED", Alignment::Justify),
    ] {
        let report = import(&document_with(vec![paragraph(
            json!({ "alignment": google }),
            "A",
        )]));
        assert_eq!(
            report.document.blocks[0].properties.alignment,
            Some(expected),
            "{google}"
        );
        let (round_tripped, _) = round_trip(&report.document);
        assert_eq!(
            round_tripped.blocks[0].properties.alignment,
            Some(expected),
            "{google} did not survive the round trip"
        );
    }
    for (google, expected) in [
        ("LEFT_TO_RIGHT", TextDirection::LeftToRight),
        ("RIGHT_TO_LEFT", TextDirection::RightToLeft),
    ] {
        let report = import(&document_with(vec![paragraph(
            json!({ "direction": google }),
            "A",
        )]));
        assert_eq!(
            report.document.blocks[0].properties.direction,
            Some(expected),
            "{google}"
        );
    }
}

#[test]
fn unspecified_alignment_and_direction_stay_inherited_rather_than_defaulted() {
    let report = import(&document_with(vec![paragraph(
        json!({
            "alignment": "ALIGNMENT_UNSPECIFIED",
            "direction": "CONTENT_DIRECTION_UNSPECIFIED"
        }),
        "Inherit",
    )]));
    assert_eq!(report.document.blocks[0].properties.alignment, None);
    assert_eq!(report.document.blocks[0].properties.direction, None);
    assert!(report.document.blocks[0].properties.is_empty());
}

#[test]
fn google_hanging_indent_round_trips_as_a_negative_first_line_indent() {
    // Google's indentFirstLine is absolute; a hanging indent is a first line
    // that starts left of the body indent.
    let input = document_with(vec![paragraph(
        json!({ "indentStart": pt(36.0), "indentFirstLine": pt(0.0) }),
        "Hanging",
    )]);
    let report = import(&input);
    let properties = &report.document.blocks[0].properties;
    assert_eq!(
        properties.indent_start,
        Some(Length::from_points(36.0).unwrap())
    );
    assert_eq!(
        properties.indent_first_line,
        Some(Length::from_points(-36.0).unwrap())
    );
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);

    let (bytes, _) = export_google_docs_json_with_warnings(&report.document).unwrap();
    let exported: Value = serde_json::from_slice(&bytes).unwrap();
    let style = &exported["body"]["content"][0]["paragraph"]["paragraphStyle"];
    assert_eq!(style["indentStart"]["magnitude"], json!(36.0));
    // Back to Google's absolute basis, not the relative one OpenDoc stores.
    assert_eq!(style["indentFirstLine"]["magnitude"], json!(0.0));

    let (round_tripped, warnings) = round_trip(&report.document);
    assert_eq!(round_tripped.blocks[0].properties, *properties);
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn google_page_break_before_round_trips_through_the_structural_page_break() {
    let input = document_with(vec![paragraph(
        json!({ "pageBreakBefore": true, "alignment": "CENTER" }),
        "New page",
    )]);
    let report = import(&input);
    assert_eq!(report.document.blocks.len(), 2);
    assert!(matches!(
        report.document.blocks[0].kind,
        BlockKind::PageBreak
    ));
    assert_eq!(single_text(&report.document.blocks[1]), "New page");
    assert_eq!(
        report.document.blocks[1].properties.alignment,
        Some(Alignment::Center)
    );
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);

    let (bytes, warnings) = export_google_docs_json_with_warnings(&report.document).unwrap();
    let exported: Value = serde_json::from_slice(&bytes).unwrap();
    // A page boundary which belongs to a following paragraph returns to the
    // native ParagraphStyle spelling instead of being degraded to an inline
    // `pageBreak` paragraph.
    assert_eq!(exported["body"]["content"].as_array().unwrap().len(), 1);
    assert_eq!(
        exported["body"]["content"][0]["paragraph"]["paragraphStyle"]["pageBreakBefore"],
        json!(true)
    );
    let reimported = import(&exported);
    assert_eq!(reimported.document.blocks.len(), 2);
    assert!(matches!(
        reimported.document.blocks[0].kind,
        BlockKind::PageBreak
    ));
    assert_eq!(single_text(&reimported.document.blocks[1]), "New page");
    assert_eq!(
        reimported.document.blocks[1].properties,
        report.document.blocks[1].properties
    );
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
}

#[test]
fn explicit_google_page_break_before_false_is_not_silently_inherited() {
    let report = import(&document_with(vec![paragraph(
        json!({ "pageBreakBefore": false }),
        "Stay here",
    )]));
    assert_eq!(report.document.blocks.len(), 1);
    assert!(message(&report.warnings, "google-dropped-paragraph-style")
        .contains("explicit pageBreakBefore=false"));
}

#[test]
fn first_line_indent_without_a_start_indent_says_which_base_it_assumed() {
    let report = import(&document_with(vec![paragraph(
        json!({ "indentFirstLine": pt(18.0) }),
        "Indented",
    )]));
    assert_eq!(
        report.document.blocks[0].properties.indent_first_line,
        Some(Length::from_points(18.0).unwrap())
    );
    assert_eq!(report.document.blocks[0].properties.indent_start, None);
    assert!(
        message(&report.warnings, "google-unresolved-first-line-indent")
            .contains("inherited indentStart of 0")
    );
}

#[test]
fn unrepresentable_paragraph_style_fields_are_named_in_a_warning() {
    let report = import(&document_with(vec![paragraph(
        json!({
            "borderTop": { "width": pt(1.0) },
            "shading": { "backgroundColor": {} },
            "tabStops": [{ "offset": pt(36.0) }],
            "keepWithNext": true,
            "somethingGoogleAddedLater": true
        }),
        "Bordered",
    )]));
    let messages = report
        .warnings
        .iter()
        .filter(|warning| warning.code == "google-dropped-paragraph-style")
        .map(|warning| warning.message.clone())
        .collect::<Vec<_>>();
    for expected in [
        "per-edge or between-paragraph borders",
        "shading",
        "tabStops",
        "somethingGoogleAddedLater",
    ] {
        assert!(
            messages.iter().any(|message| message.contains(expected)),
            "{expected} not reported in {messages:?}"
        );
    }
    // Unknown fields are reported, not fatal.
    assert_eq!(report.document.visible_text(), "Bordered\n");
}

#[test]
fn out_of_range_and_wrongly_united_dimensions_warn_rather_than_being_guessed() {
    let report = import(&document_with(vec![paragraph(
        json!({ "indentStart": { "magnitude": 12.0, "unit": "EMU" } }),
        "Odd",
    )]));
    assert_eq!(report.document.blocks[0].properties.indent_start, None);
    assert!(message(&report.warnings, "google-dropped-dimension-unit").contains("EMU"));

    let report = import(&document_with(vec![paragraph(
        json!({ "indentStart": pt(100_000.0) }),
        "Huge",
    )]));
    assert_eq!(report.document.blocks[0].properties.indent_start, None);
    assert!(message(&report.warnings, "google-dropped-paragraph-style").contains("out of range"));

    let report = import(&document_with(vec![paragraph(
        json!({ "spaceAbove": pt(-6.0) }),
        "Negative",
    )]));
    assert_eq!(report.document.blocks[0].properties.space_before, None);
    assert!(message(&report.warnings, "google-dropped-paragraph-style").contains("negative"));
}

#[test]
fn malformed_paragraph_style_values_still_abort() {
    let err = import_err(&document_with(vec![paragraph(
        json!({ "lineSpacing": "double" }),
        "Bad",
    )]));
    assert!(err.to_string().contains("lineSpacing must be a number"));

    let err = import_err(&document_with(vec![paragraph(
        json!({ "indentStart": { "magnitude": "36" } }),
        "Bad",
    )]));
    assert!(err.to_string().contains("indentStart.magnitude"));
}

#[test]
fn exact_line_spacing_cannot_reach_google_and_says_so() {
    let mut document = Document::new("Exact Spacing");
    let properties = BlockProperties {
        line_spacing: Some(LineSpacing::exactly(Length::from_points(14.0).unwrap()).unwrap()),
        ..BlockProperties::default()
    };
    document.blocks.push(Block {
        id: StableId::new("block"),
        kind: BlockKind::Paragraph,
        content: vec![opendoc_core::Inline::text("Exact")],
        properties,
    });
    let (round_tripped, warnings) = round_trip(&document);
    assert_eq!(round_tripped.blocks[0].properties.line_spacing, None);
    assert!(message(&warnings, "google-dropped-line-spacing-rule")
        .contains("percentage of the natural line height"));
}

#[test]
fn block_formatting_on_an_image_block_cannot_reach_google_and_says_so() {
    let mut document = Document::new("Formatted Image");
    let properties = BlockProperties {
        alignment: Some(Alignment::Center),
        ..BlockProperties::default()
    };
    document.blocks.push(Block {
        id: StableId::new("block"),
        kind: BlockKind::Image {
            blob_hash: "sha256:".to_string() + "aa".repeat(32).as_str(),
            alt_text: "Alt".to_string(),
            layout: Default::default(),
        },
        content: Vec::new(),
        properties,
    });
    let (_, warnings) = round_trip(&document);
    assert!(message(&warnings, "google-dropped-block-properties").contains("equation or image"));
}

// ---------------------------------------------------------------------------
// FM-10: list types
// ---------------------------------------------------------------------------

fn list_document(nesting_levels: Vec<Value>, bullet: Value, text: &str) -> Value {
    json!({
        "lists": { "kix.list1": { "listProperties": { "nestingLevels": nesting_levels } } },
        "body": { "content": [{ "paragraph": {
            "bullet": bullet,
            "elements": [{ "textRun": { "content": text } }]
        }}]}
    })
}

#[test]
fn ordered_lists_come_from_the_glyph_type_not_a_made_up_flag() {
    for glyph in ["DECIMAL", "ALPHA", "UPPER_ALPHA", "ROMAN", "UPPER_ROMAN"] {
        let report = import(&list_document(
            vec![json!({ "glyphType": glyph, "glyphFormat": "%0." })],
            json!({ "listId": "kix.list1", "nestingLevel": 0 }),
            "Numbered",
        ));
        match &report.document.blocks[0].kind {
            BlockKind::ListItem { kind, .. } => assert_eq!(*kind, ListKind::Ordered, "{glyph}"),
            other => panic!("expected list item, got {other:?}"),
        }
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    }
}

#[test]
fn ordered_list_counter_formats_round_trip_through_google_list_definitions() {
    let formats = [
        ("decimal", OrderedListFormat::Decimal, "DECIMAL"),
        ("lower-alpha", OrderedListFormat::LowerAlpha, "ALPHA"),
        ("upper-alpha", OrderedListFormat::UpperAlpha, "UPPER_ALPHA"),
        ("lower-roman", OrderedListFormat::LowerRoman, "ROMAN"),
        ("upper-roman", OrderedListFormat::UpperRoman, "UPPER_ROMAN"),
    ];
    let mut document = Document::new("Formats");
    for (name, format, _) in formats {
        let list_id = StableId::parse(format!("list-{name}")).unwrap();
        document.blocks.push(Block {
            id: StableId::new("block"),
            kind: BlockKind::ListItem {
                list_id: list_id.clone(),
                level: 0,
                kind: ListKind::Ordered,
            },
            content: vec![opendoc_core::Inline::text(name)],
            properties: BlockProperties::default(),
        });
        // Every format is explicit at level zero, except Decimal where the
        // same result is also the inherited default. Keeping it here makes
        // the exporter mapping assertion uniform.
        document
            .list_properties
            .entry(list_id)
            .or_default()
            .ordered_formats
            .insert(0, format);
    }

    let (bytes, warnings) = export_google_docs_json_with_warnings(&document).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let exported: Value = serde_json::from_slice(&bytes).unwrap();
    for (name, _, glyph_type) in formats {
        assert_eq!(
            exported["lists"][format!("list-{name}")]["listProperties"]["nestingLevels"][0]
                ["glyphType"],
            glyph_type,
            "{name} should use Google's native glyph type"
        );
    }

    let imported = import(&exported);
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    for ((_, expected, _), block) in formats.iter().zip(&imported.document.blocks) {
        let BlockKind::ListItem { list_id, .. } = &block.kind else {
            panic!("expected list item");
        };
        assert_eq!(
            imported
                .document
                .list_properties
                .get(list_id)
                .map(|properties| properties.format_for(0))
                .unwrap_or_else(|| OrderedListFormat::inherited_at(0)),
            *expected
        );
    }
}

#[test]
fn zero_decimal_list_warns_instead_of_claiming_lossless_decimal_formatting() {
    let report = import(&list_document(
        vec![json!({ "glyphType": "ZERO_DECIMAL", "glyphFormat": "%0." })],
        json!({ "listId": "kix.list1", "nestingLevel": 0 }),
        "Numbered",
    ));
    assert!(matches!(
        report.document.blocks[0].kind,
        BlockKind::ListItem {
            kind: ListKind::Ordered,
            ..
        }
    ));
    assert!(
        message(&report.warnings, "google-unrepresentable-list-format").contains("ZERO_DECIMAL")
    );
}

#[test]
fn noncanonical_google_list_glyph_format_is_named_before_using_the_model_template() {
    let report = import(&list_document(
        vec![json!({ "glyphType": "DECIMAL", "glyphFormat": "%0)" })],
        json!({ "listId": "kix.list1", "nestingLevel": 0 }),
        "Numbered",
    ));
    assert!(matches!(
        report.document.blocks[0].kind,
        BlockKind::ListItem {
            kind: ListKind::Ordered,
            ..
        }
    ));
    assert!(message(&report.warnings, "google-unrepresentable-list-glyph-format").contains("%0)"));

    let (bytes, warnings) = export_google_docs_json_with_warnings(&report.document).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let exported: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        exported["lists"]["kix.list1"]["listProperties"]["nestingLevels"][0]["glyphFormat"],
        "%0."
    );
}

#[test]
fn bullet_glyphs_stay_bulleted_and_nesting_levels_are_read_per_level() {
    let report = import(&list_document(
        vec![
            json!({ "glyphSymbol": "\u{25cf}" }),
            json!({ "glyphType": "DECIMAL" }),
        ],
        json!({ "listId": "kix.list1", "nestingLevel": 1 }),
        "Nested",
    ));
    match &report.document.blocks[0].kind {
        BlockKind::ListItem { kind, level, .. } => {
            assert_eq!(*level, 1);
            assert_eq!(*kind, ListKind::Ordered);
        }
        other => panic!("expected list item, got {other:?}"),
    }

    let report = import(&list_document(
        vec![json!({ "glyphSymbol": "\u{25cf}" })],
        json!({ "listId": "kix.list1", "nestingLevel": 0 }),
        "Bulleted",
    ));
    match &report.document.blocks[0].kind {
        BlockKind::ListItem { kind, .. } => assert_eq!(*kind, ListKind::Bullet),
        other => panic!("expected list item, got {other:?}"),
    }
}

#[test]
fn invalid_google_custom_bullet_marker_is_named_before_falling_back() {
    let report = import(&list_document(
        vec![json!({ "glyphSymbol": "xxxxxxxxxxxxxxxxx" })],
        json!({ "listId": "kix.list1", "nestingLevel": 0 }),
        "Bulleted",
    ));
    assert!(matches!(
        report.document.blocks[0].kind,
        BlockKind::ListItem {
            kind: ListKind::Bullet,
            ..
        }
    ));
    assert!(
        message(&report.warnings, "google-unrepresentable-bullet-marker").contains("kix.list1")
    );
    let list_id = report.document.blocks[0].list_id().expect("list item");
    assert!(report
        .document
        .list_properties
        .get(list_id)
        .is_none_or(|properties| properties.bullet_markers.is_empty()));
}

#[test]
fn a_list_with_no_definition_falls_back_to_a_bullet_and_says_so() {
    let report = import(&json!({
        "body": { "content": [{ "paragraph": {
            "bullet": { "listId": "kix.missing", "nestingLevel": 0 },
            "elements": [{ "textRun": { "content": "Orphan" } }]
        }}]}
    }));
    match &report.document.blocks[0].kind {
        BlockKind::ListItem { kind, .. } => assert_eq!(*kind, ListKind::Bullet),
        other => panic!("expected list item, got {other:?}"),
    }
    assert!(message(&report.warnings, "google-unknown-list-definition").contains("kix.missing"));
}

#[test]
fn start_number_on_a_nonordered_google_level_is_not_stored_silently() {
    let report = import(&list_document(
        vec![json!({ "glyphSymbol": "\u{2022}", "startNumber": 7 })],
        json!({ "listId": "kix.bulleted", "nestingLevel": 0 }),
        "Bulleted",
    ));
    assert!(matches!(
        report.document.blocks[0].kind,
        BlockKind::ListItem {
            kind: ListKind::Bullet,
            ..
        }
    ));
    assert!(
        message(&report.warnings, "google-unrepresentable-list-start")
            .contains("non-ordered marker")
    );
    let list_id = report.document.blocks[0].list_id().expect("list item");
    assert!(report
        .document
        .list_properties
        .get(list_id)
        .is_none_or(|properties| properties.ordered_starts.is_empty()));
    let (bytes, export_warnings) =
        export_google_docs_json_with_warnings(&report.document).expect("export must succeed");
    assert!(export_warnings.is_empty(), "{export_warnings:?}");
    let exported: Value = serde_json::from_slice(&bytes).expect("export is JSON");
    assert!(
        exported["lists"]["kix.bulleted"]["listProperties"]["nestingLevels"][0]["startNumber"]
            .is_null()
    );
}

#[test]
fn bulleted_and_ordered_lists_round_trip_through_a_real_lists_section() {
    let report = import(&json!({
        "lists": {
            "kix.b": { "listProperties": { "nestingLevels": [{ "glyphSymbol": "\u{25cf}" }] } },
            "kix.o": { "listProperties": { "nestingLevels": [{ "glyphType": "DECIMAL" }] } }
        },
        "body": { "content": [
            { "paragraph": { "bullet": { "listId": "kix.b", "nestingLevel": 0 },
                "elements": [{ "textRun": { "content": "Bullet" } }] } },
            { "paragraph": { "bullet": { "listId": "kix.o", "nestingLevel": 0 },
                "elements": [{ "textRun": { "content": "Ordered" } }] } }
        ]}
    }));
    let kinds = |document: &Document| {
        document
            .blocks
            .iter()
            .map(|block| match &block.kind {
                BlockKind::ListItem { kind, .. } => *kind,
                other => panic!("expected list item, got {other:?}"),
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        kinds(&report.document),
        vec![ListKind::Bullet, ListKind::Ordered]
    );

    let (bytes, warnings) = export_google_docs_json_with_warnings(&report.document).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let exported: Value = serde_json::from_slice(&bytes).unwrap();
    // The exported document carries Google's own list definitions, not the
    // non-standard `bullet.ordered` flag it used to invent.
    assert_eq!(
        exported["lists"]["kix.o"]["listProperties"]["nestingLevels"][0]["glyphType"],
        json!("DECIMAL")
    );
    assert!(exported["body"]["content"][1]["paragraph"]["bullet"]["ordered"].is_null());

    let (round_tripped, _) = round_trip(&report.document);
    assert_eq!(
        kinds(&round_tripped),
        vec![ListKind::Bullet, ListKind::Ordered]
    );
}

#[test]
fn checklists_keep_their_checked_state_across_a_google_round_trip_and_warn() {
    let mut document = Document::new("Checklist");
    for (index, checked) in [false, true].into_iter().enumerate() {
        document.blocks.push(Block {
            id: StableId::new("block"),
            kind: BlockKind::ListItem {
                list_id: StableId::parse("check-list").unwrap(),
                level: 0,
                kind: ListKind::Checklist { checked },
            },
            content: vec![opendoc_core::Inline::text(if index == 0 {
                "Todo"
            } else {
                "Done"
            })],
            properties: BlockProperties::default(),
        });
    }
    let (round_tripped, warnings) = round_trip(&document);
    let kinds = round_tripped
        .blocks
        .iter()
        .map(|block| match &block.kind {
            BlockKind::ListItem { kind, .. } => *kind,
            other => panic!("expected list item, got {other:?}"),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        kinds,
        vec![
            ListKind::Checklist { checked: false },
            ListKind::Checklist { checked: true }
        ]
    );
    // Google itself has no checklist, so the export says what a Google reader
    // will see even though OpenDoc reads the state back exactly.
    assert!(message(&warnings, "google-checklist-exported-as-bullet").contains("ballot-box"));
}

/// The round trip above carries the tick state in `opendocChecked`, so it
/// passes whatever glyphs the export draws — swap the two constants and every
/// done item still comes back done while a Google reader sees it unticked.
/// What a Google reader sees is the `glyphSymbol`, so that is asserted
/// against the state that produced it, in both directions.
#[test]
fn the_ballot_box_a_google_reader_sees_matches_the_tick_state() {
    let mut document = Document::new("Checklist");
    for (level, checked) in [(0u8, false), (1, true)] {
        document.blocks.push(Block {
            id: StableId::new("block"),
            kind: BlockKind::ListItem {
                list_id: StableId::parse("check-list").unwrap(),
                level,
                kind: ListKind::Checklist { checked },
            },
            content: vec![opendoc_core::Inline::text("item")],
            properties: BlockProperties::default(),
        });
    }
    let (bytes, _) = export_google_docs_json_with_warnings(&document).expect("export failed");
    let exported: Value = serde_json::from_slice(&bytes).expect("export is not JSON");
    let levels = exported["lists"]["check-list"]["listProperties"]["nestingLevels"]
        .as_array()
        .expect("no nesting levels");
    assert_eq!(
        json!("\u{2610}"),
        levels[0]["glyphSymbol"],
        "the unticked level did not draw an empty ballot box"
    );
    assert_eq!(
        json!("\u{2611}"),
        levels[1]["glyphSymbol"],
        "the ticked level did not draw a ticked ballot box"
    );

    // And the reading back: an empty box is unticked, a ticked box is ticked.
    for (glyph, checked) in [("\u{2610}", false), ("\u{2611}", true)] {
        let report = import(&list_document(
            vec![json!({ "glyphSymbol": glyph })],
            json!({ "listId": "kix.list1", "nestingLevel": 0 }),
            "Item",
        ));
        assert_eq!(
            ListKind::Checklist { checked },
            match &report.document.blocks[0].kind {
                BlockKind::ListItem { kind, .. } => *kind,
                other => panic!("expected a list item, got {other:?}"),
            },
            "{glyph:?} did not import as checked={checked}"
        );
    }
}

#[test]
fn a_checklist_authored_with_ballot_box_glyphs_imports_as_a_checklist() {
    let report = import(&list_document(
        vec![json!({ "glyphSymbol": "\u{2611}" })],
        json!({ "listId": "kix.list1", "nestingLevel": 0 }),
        "Done",
    ));
    match &report.document.blocks[0].kind {
        BlockKind::ListItem { kind, .. } => {
            assert_eq!(*kind, ListKind::Checklist { checked: true })
        }
        other => panic!("expected list item, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// FM-11: document-level parts with no model yet
// ---------------------------------------------------------------------------

#[test]
fn document_style_page_geometry_imports_while_other_document_parts_are_named() {
    let report = import(&json!({
        "documentStyle": { "pageSize": { "width": pt(612.0), "height": pt(792.0) } },
        "headers": { "kix.h1": { "content": [] } },
        "footers": { "kix.f1": { "content": [] } },
        "namedStyles": { "styles": [{ "namedStyleType": "NORMAL_TEXT" }] },
        "inlineObjects": { "kix.i1": {} },
        "positionedObjects": { "kix.p1": {} },
        "body": { "content": [{ "paragraph": {
            "elements": [{ "textRun": { "content": "Body" } }]
        }}]}
    }));
    let messages = report
        .warnings
        .iter()
        .filter(|warning| warning.code == "google-dropped-document-part")
        .map(|warning| warning.message.clone())
        .collect::<Vec<_>>();
    for expected in [
        "header map contains non-default",
        "footer map contains non-default",
        "named style definitions",
        "inline objects",
        "positioned objects",
    ] {
        assert!(
            messages.iter().any(|message| message.contains(expected)),
            "{expected} not reported in {messages:?}"
        );
    }
    assert_eq!(report.document.page_setup.width.twips(), 12_240);
    assert_eq!(report.document.page_setup.height.twips(), 15_840);
    assert_eq!(report.document.visible_text(), "Body\n");
}

#[test]
fn google_default_header_and_footer_import_and_export_through_document_furniture() {
    let report = import(&json!({
        "documentStyle": {
            "defaultHeaderId": "kix.header-default",
            "defaultFooterId": "kix.footer-default"
        },
        "headers": {
            "kix.header-default": { "content": [paragraph(json!({ "alignment": "CENTER" }), "Running head")] }
        },
        "footers": {
            "kix.footer-default": { "content": [paragraph(json!({}), "Page footer")] }
        },
        "body": { "content": [paragraph(json!({}), "Body")] }
    }));
    assert_eq!(report.document.header.len(), 1);
    assert_eq!(report.document.footer.len(), 1);
    assert_eq!(single_text(&report.document.header[0]), "Running head");
    assert_eq!(single_text(&report.document.footer[0]), "Page footer");
    assert_eq!(
        report.document.header[0].properties.alignment,
        Some(Alignment::Center)
    );
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);

    let (bytes, warnings) = export_google_docs_json_with_warnings(&report.document).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let exported: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        exported["documentStyle"]["defaultHeaderId"],
        json!("opendoc.default-header")
    );
    assert_eq!(
        exported["headers"]["opendoc.default-header"]["content"][0]["paragraph"]["elements"][0]
            ["textRun"]["content"],
        json!("Running head")
    );
    let reparsed = import(&exported);
    assert_eq!(single_text(&reparsed.document.header[0]), "Running head");
    assert_eq!(
        reparsed.document.header[0].properties,
        report.document.header[0].properties
    );
    assert_eq!(single_text(&reparsed.document.footer[0]), "Page footer");
}

#[test]
fn google_disabled_variant_and_unselected_furniture_remain_explicit() {
    let report = import(&json!({
        "documentStyle": {
            "defaultHeaderId": "default-header",
            "firstPageHeaderId": "first-header"
        },
        "headers": {
            "default-header": { "content": [paragraph(json!({}), "Ordinary")] },
            "first-header": { "content": [paragraph(json!({}), "First only")] }
        },
        "body": { "content": [paragraph(json!({}), "Body")] }
    }));
    assert_eq!(single_text(&report.document.header[0]), "Ordinary");
    assert!(report.document.first_page_header.is_none());
    assert!(
        report
            .warnings
            .iter()
            .any(|warning| warning.message.contains("non-default or section-local")),
        "{:?}",
        report.warnings
    );
}

#[test]
fn google_document_style_first_and_even_furniture_round_trip_with_their_policies() {
    let report = import(&json!({
        "documentStyle": {
            "defaultHeaderId": "ordinary-header",
            "defaultFooterId": "ordinary-footer",
            "useFirstPageHeaderFooter": true,
            "firstPageHeaderId": "first-header",
            "firstPageFooterId": "first-footer",
            "useEvenPageHeaderFooter": true,
            "evenPageHeaderId": "even-header",
            "evenPageFooterId": "even-footer"
        },
        "headers": {
            "ordinary-header": { "content": [paragraph(json!({}), "Ordinary header")] },
            "first-header": { "content": [paragraph(json!({}), "First header")] },
            "even-header": { "content": [paragraph(json!({}), "Even header")] }
        },
        "footers": {
            "ordinary-footer": { "content": [paragraph(json!({}), "Ordinary footer")] },
            "first-footer": { "content": [paragraph(json!({}), "First footer")] },
            "even-footer": { "content": [paragraph(json!({}), "Even footer")] }
        },
        "body": { "content": [paragraph(json!({}), "Body")] }
    }));
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert_eq!(single_text(&report.document.header[0]), "Ordinary header");
    assert_eq!(
        single_text(
            report
                .document
                .furniture(HeaderFooterSlot::FirstPageHeader)
                .first()
                .unwrap()
        ),
        "First header"
    );
    assert_eq!(
        single_text(
            report
                .document
                .furniture(HeaderFooterSlot::EvenPageFooter)
                .first()
                .unwrap()
        ),
        "Even footer"
    );

    let (bytes, warnings) = export_google_docs_json_with_warnings(&report.document).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let exported: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        exported["documentStyle"]["useFirstPageHeaderFooter"],
        json!(true)
    );
    assert_eq!(
        exported["documentStyle"]["useEvenPageHeaderFooter"],
        json!(true)
    );
    assert_eq!(
        exported["headers"]["opendoc.first-page-header"]["content"][0]["paragraph"]["elements"][0]
            ["textRun"]["content"],
        json!("First header")
    );
    let reparsed = import(&exported);
    assert!(reparsed.warnings.is_empty(), "{:?}", reparsed.warnings);
    for (slot, expected) in [
        (HeaderFooterSlot::FirstPageHeader, "First header"),
        (HeaderFooterSlot::FirstPageFooter, "First footer"),
        (HeaderFooterSlot::EvenPageHeader, "Even header"),
        (HeaderFooterSlot::EvenPageFooter, "Even footer"),
    ] {
        assert!(reparsed.document.has_furniture_override(slot));
        assert_eq!(single_text(&reparsed.document.furniture(slot)[0]), expected);
    }
}

#[test]
fn google_enabled_variant_without_an_id_is_an_explicit_empty_override() {
    let report = import(&json!({
        "documentStyle": { "useFirstPageHeaderFooter": true },
        "body": { "content": [paragraph(json!({}), "Body")] }
    }));
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert_eq!(report.document.first_page_header, Some(Vec::new()));
    assert_eq!(report.document.first_page_footer, Some(Vec::new()));
    let (reparsed, warnings) = round_trip(&report.document);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(reparsed.first_page_header, Some(Vec::new()));
    assert_eq!(reparsed.first_page_footer, Some(Vec::new()));
}

#[test]
fn google_section_local_variant_is_not_misrepresented_as_document_wide() {
    let report = import(&json!({
        "documentStyle": {
            "useFirstPageHeaderFooter": true,
            "firstPageHeaderId": "first-header"
        },
        "headers": { "first-header": { "content": [paragraph(json!({}), "First header")] } },
        "body": { "content": [
            { "sectionBreak": { "sectionStyle": { "firstPageHeaderId": "section-header" } } },
            paragraph(json!({}), "Body")
        ] }
    }));
    assert!(report.document.first_page_header.is_none());
    assert!(
        report.warnings.iter().any(|warning| warning
            .message
            .contains("first/even header/footer IDs")
            && warning.message.contains("section-scoped")),
        "{:?}",
        report.warnings
    );
    assert!(
        report
            .warnings
            .iter()
            .any(|warning| warning.message.contains("non-default or section-local")),
        "{:?}",
        report.warnings
    );
}

#[test]
fn google_document_style_background_and_section_local_furniture_are_precisely_disclosed() {
    let report = import(&json!({
        "documentStyle": {
            "background": { "color": { "rgbColor": { "red": 0.2, "green": 0.3, "blue": 0.4 } } },
            "useFirstPageHeaderFooter": true,
            "firstPageHeaderId": "first-header"
        },
        "headers": { "first-header": { "content": [paragraph(json!({}), "First header")] } },
        "body": { "content": [
            { "sectionBreak": { "sectionStyle": { "firstPageHeaderId": "section-header" } } },
            paragraph(json!({}), "Body")
        ] }
    }));
    let messages = report
        .warnings
        .iter()
        .filter(|warning| warning.code == "google-dropped-document-part")
        .map(|warning| warning.message.as_str())
        .collect::<Vec<_>>();
    assert!(
        messages
            .iter()
            .any(|message| message.contains("documentStyle.background")
                && message.contains("page paint")),
        "{messages:?}"
    );
    assert!(
        messages.iter().any(|message| {
            message.contains("first/even header/footer IDs") && message.contains("section-scoped")
        }),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .all(|message| !message.contains("page paint or section-scoped")),
        "losses must name their actual source: {messages:?}"
    );
}

#[test]
fn google_document_style_empty_background_and_policy_bits_are_not_phantom_losses() {
    let report = import(&json!({
        "documentStyle": {
            "background": {},
            "useFirstPageHeaderFooter": false,
            "useEvenPageHeaderFooter": false
        },
        "body": { "content": [paragraph(json!({}), "Body")] }
    }));
    assert!(
        !has(&report.warnings, "google-dropped-document-part"),
        "an empty page-paint record and disabled policies contain no durable source state: {:?}",
        report.warnings
    );
}

#[test]
fn google_interior_section_boundary_without_style_still_prevents_global_variant_import() {
    let report = import(&json!({
        "documentStyle": {
            "useFirstPageHeaderFooter": true,
            "firstPageHeaderId": "first-header"
        },
        "headers": { "first-header": { "content": [paragraph(json!({}), "First header")] } },
        "body": { "content": [
            { "sectionBreak": {} },
            paragraph(json!({}), "First section"),
            { "sectionBreak": {} },
            paragraph(json!({}), "Second section")
        ] }
    }));
    assert!(report.document.first_page_header.is_none());
    assert!(
        report.warnings.iter().any(|warning| warning
            .message
            .contains("first/even header/footer IDs")
            && warning.message.contains("section-scoped")),
        "{:?}",
        report.warnings
    );
}

#[test]
fn google_default_furniture_lists_keep_their_shared_native_definition() {
    let report = import(&json!({
        "documentStyle": { "defaultHeaderId": "header-list" },
        "headers": { "header-list": { "content": [{ "paragraph": {
            "bullet": { "listId": "kix.header-list", "nestingLevel": 0 },
            "elements": [{ "textRun": { "content": "One" } }]
        }}] } },
        "lists": { "kix.header-list": { "listProperties": { "nestingLevels": [{
            "glyphType": "UPPER_ROMAN", "glyphFormat": "%0.", "startNumber": 3
        }] } } },
        "body": { "content": [paragraph(json!({}), "Body")] }
    }));
    let BlockKind::ListItem { list_id, kind, .. } = &report.document.header[0].kind else {
        panic!("expected imported header list item");
    };
    assert_eq!(*kind, ListKind::Ordered);
    let properties = report.document.list_properties.get(list_id).unwrap();
    assert_eq!(properties.format_for(0), OrderedListFormat::UpperRoman);
    assert_eq!(properties.start_for(0), 3);

    let (bytes, warnings) = export_google_docs_json_with_warnings(&report.document).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let exported: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        exported["lists"]["kix.header-list"]["listProperties"]["nestingLevels"][0]["glyphType"],
        json!("UPPER_ROMAN")
    );
    assert_eq!(
        exported["lists"]["kix.header-list"]["listProperties"]["nestingLevels"][0]["startNumber"],
        json!(3)
    );
}

#[test]
fn google_document_style_page_geometry_round_trips_without_a_drop_warning() {
    let report = import(&json!({
        "documentStyle": {
            "pageSize": { "width": pt(841.9), "height": pt(595.3) },
            "marginTop": pt(36.0), "marginBottom": pt(54.0),
            "marginLeft": pt(72.0), "marginRight": pt(90.0),
            "marginHeader": pt(18.0), "marginFooter": pt(24.0),
            "pageNumberStart": 12
        },
        "body": { "content": [{ "paragraph": {
            "elements": [{ "textRun": { "content": "Body" } }]
        }}]}
    }));
    assert_eq!(report.document.page_setup.width.twips(), 16_838);
    assert_eq!(report.document.page_setup.height.twips(), 11_906);
    assert_eq!(report.document.page_setup.margin_top.twips(), 720);
    assert_eq!(report.document.page_setup.margin_bottom.twips(), 1_080);
    assert_eq!(report.document.page_setup.margin_start.twips(), 1_440);
    assert_eq!(report.document.page_setup.margin_end.twips(), 1_800);
    assert_eq!(report.document.page_setup.margin_header.twips(), 360);
    assert_eq!(report.document.page_setup.margin_footer.twips(), 480);
    assert_eq!(report.document.page_setup.page_number_start, 12);
    assert!(
        !has(&report.warnings, "google-dropped-document-part"),
        "{:?}",
        report.warnings
    );

    let (bytes, warnings) = export_google_docs_json_with_warnings(&report.document).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let exported: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        exported["documentStyle"]["pageSize"]["width"]["magnitude"],
        json!(841.9)
    );
    assert_eq!(
        exported["documentStyle"]["marginRight"]["magnitude"],
        json!(90.0)
    );
    assert_eq!(exported["documentStyle"]["pageNumberStart"], json!(12));
    let reparsed = import(&exported);
    assert_eq!(reparsed.document.page_setup, report.document.page_setup);
}

#[test]
fn google_document_style_orientation_flip_is_canonicalized_to_effective_page_geometry() {
    // Google spells this as a portrait page size plus a separate boolean.
    // PageSetup owns physical geometry, so its canonical spelling is the
    // actual landscape sheet. Exporting that sheet without a redundant
    // transport flag preserves what users see and what every other OpenDoc
    // exporter consumes.
    let report = import(&json!({
        "documentStyle": {
            "pageSize": { "width": pt(612.0), "height": pt(792.0) },
            "flipPageOrientation": true
        },
        "body": { "content": [paragraph(json!({}), "Body")] }
    }));
    assert_eq!(report.document.page_setup.width.twips(), 15_840);
    assert_eq!(report.document.page_setup.height.twips(), 12_240);
    assert!(
        !has(&report.warnings, "google-dropped-document-part"),
        "{:?}",
        report.warnings
    );

    let (bytes, warnings) = export_google_docs_json_with_warnings(&report.document).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let exported: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        exported["documentStyle"]["pageSize"]["width"]["magnitude"],
        json!(792.0)
    );
    assert!(
        exported["documentStyle"]["flipPageOrientation"].is_null(),
        "effective dimensions must not be flipped twice: {exported}"
    );
    let reparsed = import(&exported);
    assert_eq!(reparsed.document.page_setup, report.document.page_setup);
    assert!(reparsed.warnings.is_empty(), "{:?}", reparsed.warnings);
}

#[test]
fn absent_document_parts_produce_no_warning() {
    let report = import(&json!({
        "headers": {},
        "body": { "content": [{ "paragraph": {
            "elements": [{ "textRun": { "content": "Body" } }]
        }}]}
    }));
    assert!(
        !has(&report.warnings, "google-dropped-document-part"),
        "{:?}",
        report.warnings
    );
}

#[test]
fn the_initial_google_section_break_does_not_create_a_blank_first_page() {
    let report = import(&json!({
        "body": { "content": [
            { "startIndex": 0, "endIndex": 1,
              "sectionBreak": { "sectionStyle": { "columnProperties": [{}, {}] } } },
            { "startIndex": 1, "endIndex": 6, "paragraph": {
                "elements": [{ "textRun": { "content": "Body\n" } }]
            }}
        ]}
    }));
    assert_eq!(report.document.visible_text(), "Body\n");
    assert!(report
        .document
        .blocks
        .iter()
        .all(|block| !matches!(block.kind, BlockKind::PageBreak)));
    assert!(message(&report.warnings, "google-dropped-document-part").contains("section styling"));
}

#[test]
fn continuous_google_sections_do_not_become_page_breaks() {
    let report = import(&json!({
        "body": { "content": [
            { "startIndex": 0, "endIndex": 1, "sectionBreak": {} },
            { "startIndex": 1, "endIndex": 7, "paragraph": {
                "elements": [{ "textRun": { "content": "First\n" } }]
            }},
            { "startIndex": 7, "endIndex": 8, "sectionBreak": {
                "sectionStyle": { "sectionType": "CONTINUOUS" }
            }},
            { "startIndex": 8, "endIndex": 15, "paragraph": {
                "elements": [{ "textRun": { "content": "Second\n" } }]
            }}
        ]}
    }));
    assert_eq!(report.document.visible_text(), "First\nSecond\n");
    assert!(report
        .document
        .blocks
        .iter()
        .all(|block| !matches!(block.kind, BlockKind::PageBreak)));
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.code == "google-dropped-document-part"
            && warning.message.contains("continuous")));
}

#[test]
fn next_page_google_sections_still_preserve_the_physical_boundary() {
    let report = import(&json!({
        "body": { "content": [
            { "startIndex": 0, "endIndex": 1, "sectionBreak": {} },
            { "startIndex": 1, "endIndex": 7, "paragraph": {
                "elements": [{ "textRun": { "content": "First\n" } }]
            }},
            { "startIndex": 7, "endIndex": 8, "sectionBreak": {
                "sectionStyle": { "sectionType": "NEXT_PAGE" }
            }},
            { "startIndex": 8, "endIndex": 15, "paragraph": {
                "elements": [{ "textRun": { "content": "Second\n" } }]
            }}
        ]}
    }));
    assert!(matches!(
        report.document.blocks[1].kind,
        BlockKind::PageBreak
    ));
}

#[test]
fn unknown_google_section_type_is_not_guessed_as_a_page_break() {
    let report = import(&json!({
        "body": { "content": [
            { "startIndex": 0, "endIndex": 1, "sectionBreak": {} },
            { "startIndex": 1, "endIndex": 7, "paragraph": {
                "elements": [{ "textRun": { "content": "First\n" } }]
            }},
            { "startIndex": 7, "endIndex": 8, "sectionBreak": {
                "sectionStyle": { "sectionType": "FUTURE_BOUNDARY" }
            }},
            { "startIndex": 8, "endIndex": 15, "paragraph": {
                "elements": [{ "textRun": { "content": "Second\n" } }]
            }}
        ]}
    }));
    assert_eq!(report.document.visible_text(), "First\nSecond\n");
    assert!(report
        .document
        .blocks
        .iter()
        .all(|block| !matches!(block.kind, BlockKind::PageBreak)));
    assert!(report.warnings.iter().any(|warning| {
        warning.code == "google-dropped-document-part"
            && warning.message.contains("FUTURE_BOUNDARY")
            && warning.message.contains("rather than guessed")
    }));
}

// ---------------------------------------------------------------------------
// FS-10: real-world documents must not abort
// ---------------------------------------------------------------------------

#[test]
fn understood_but_unrepresentable_elements_degrade_instead_of_aborting() {
    for (element, fragment) in [
        (json!({ "columnBreak": {} }), "column break"),
        (
            json!({ "autoText": { "type": "SECTION_NUMBER" } }),
            "unsupported type",
        ),
        (
            json!({ "inlineObjectElement": { "inlineObjectId": "kix.i1" } }),
            "inline object",
        ),
    ] {
        let report = import(&document_with(vec![json!({ "paragraph": {
            "elements": [{ "textRun": { "content": "Keep" } }, element]
        }})]));
        assert_eq!(report.document.visible_text(), "Keep\n");
        assert!(
            message(&report.warnings, "google-dropped-paragraph-element").contains(fragment),
            "{fragment}"
        );
    }
}

#[test]
fn native_google_auto_text_page_fields_round_trip_without_an_extension() {
    let report = import(&document_with(vec![json!({ "paragraph": {
        "elements": [
            { "textRun": { "content": "Page " } },
            { "autoText": { "type": "PAGE_NUMBER" } },
            { "textRun": { "content": " of " } },
            { "autoText": { "type": "PAGE_COUNT" } },
            { "textRun": { "content": "\n" } }
        ]
    }})]));
    assert!(matches!(
        report.document.blocks[0].content.as_slice(),
        [
            Inline::Text { text, .. },
            Inline::PageNumber { field: PageNumberField::CurrentPage, .. },
            Inline::Text { text: middle, .. },
            Inline::PageNumber { field: PageNumberField::PageCount, .. },
        ] if text == "Page " && middle == " of "
    ));
    assert!(!has(&report.warnings, "google-dropped-paragraph-element"));

    let (exported, warnings) = export_google_docs_json_with_warnings(&report.document).unwrap();
    assert!(warnings.is_empty());
    let value: Value = serde_json::from_slice(&exported).unwrap();
    let elements = value["body"]["content"][0]["paragraph"]["elements"]
        .as_array()
        .unwrap();
    assert_eq!(elements[1]["autoText"]["type"], "PAGE_NUMBER");
    assert_eq!(elements[3]["autoText"]["type"], "PAGE_COUNT");
    let reread = import_google_docs_json("Reread", &exported).unwrap();
    assert!(matches!(
        reread.document.blocks[0].content.as_slice(),
        [
            Inline::Text { .. },
            Inline::PageNumber {
                field: PageNumberField::CurrentPage,
                ..
            },
            Inline::Text { .. },
            Inline::PageNumber {
                field: PageNumberField::PageCount,
                ..
            },
        ]
    ));
}

#[test]
fn google_horizontal_rule_is_a_durable_rule_block_and_round_trips() {
    let report = import(&document_with(vec![json!({ "paragraph": {
        "elements": [{ "horizontalRule": {} }]
    }})]));
    assert!(matches!(
        report.document.blocks.as_slice(),
        [Block { kind: BlockKind::HorizontalRule, content, .. }] if content.is_empty()
    ));
    assert!(!has(&report.warnings, "google-dropped-paragraph-element"));
    let (round_tripped, warnings) = round_trip(&report.document);
    assert!(matches!(
        round_tripped.blocks[0].kind,
        BlockKind::HorizontalRule
    ));
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn person_chips_and_rich_links_keep_typed_offline_identity() {
    let report = import(&document_with(vec![json!({ "paragraph": { "elements": [
        { "person": { "personProperties": { "name": "Ada Lovelace", "email": "ada@example.invalid", "personId": "people/ada" } } },
        { "richLink": { "richLinkId": "chip-42", "richLinkProperties": { "title": "Spec", "uri": "https://example.invalid/spec", "mimeType": "application/vnd.google-apps.document" } } }
    ]}})]));
    let content = &report.document.blocks[0].content;
    assert!(matches!(
        &content[0],
        opendoc_core::Inline::GooglePersonChip { label, email, person_id, .. }
            if label == "Ada Lovelace" && email == "ada@example.invalid" && person_id.as_deref() == Some("people/ada")
    ));
    assert!(matches!(
        &content[1],
        opendoc_core::Inline::GoogleRichLinkChip { label, href, rich_link_id, mime_type, .. }
            if label == "Spec" && href == "https://example.invalid/spec" && rich_link_id.as_deref() == Some("chip-42") && mime_type.as_deref() == Some("application/vnd.google-apps.document")
    ));
    assert!(!has(&report.warnings, "google-degraded-paragraph-element"));
    let (round_tripped, warnings) = round_trip(&report.document);
    assert!(matches!(
        &round_tripped.blocks[0].content[..],
        [opendoc_core::Inline::GooglePersonChip { email, person_id, .. },
         opendoc_core::Inline::GoogleRichLinkChip { rich_link_id, mime_type, .. }]
            if email == "ada@example.invalid"
                && person_id.as_deref() == Some("people/ada")
                && rich_link_id.as_deref() == Some("chip-42")
                && mime_type.as_deref() == Some("application/vnd.google-apps.document")
    ));
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn unsafe_google_rich_link_uri_degrades_to_its_visible_title() {
    let report = import(&document_with(vec![json!({ "paragraph": { "elements": [{
        "richLink": {
            "richLinkId": "unsafe-chip",
            "richLinkProperties": {
                "title": "Untrusted document",
                "uri": "javascript:alert(1)",
                "mimeType": "application/vnd.google-apps.document"
            }
        }
    }]}})]));

    assert!(matches!(
        report.document.blocks[0].content.as_slice(),
        [Inline::Text { text, marks, .. }] if text == "Untrusted document" && marks.is_empty()
    ));
    assert!(has(&report.warnings, "google-unsafe-external-link-dropped"));
}

#[test]
fn native_iso_utc_date_element_round_trips_as_the_existing_date_chip() {
    let report = import(&document_with(vec![json!({ "paragraph": { "elements": [{
        "dateElement": {
            "dateId": "google-date-42",
            "dateElementProperties": {
                "timestamp": "2028-02-29T00:00:00Z",
                "timeZoneId": "Etc/UTC",
                "dateFormat": "DATE_FORMAT_ISO8601",
                "timeFormat": "TIME_FORMAT_DISABLED",
                "displayText": "2028-02-29"
            }
        }
    }] }} )]));
    assert!(matches!(
        &report.document.blocks[0].content[..],
        [Inline::DateChip { id, date }] if id.as_str() == "google-date-42" && date == "2028-02-29"
    ));
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let (round_tripped, warnings) = round_trip(&report.document);
    assert!(matches!(
        &round_tripped.blocks[0].content[..],
        [Inline::DateChip { date, .. }] if date == "2028-02-29"
    ));
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn formatted_native_date_element_keeps_display_text_instead_of_claiming_date_chip_fidelity() {
    let report = import(&document_with(vec![json!({ "paragraph": { "elements": [{
        "dateElement": {
            "dateId": "google-date-local",
            "textStyle": { "bold": true },
            "dateElementProperties": {
                "timestamp": "2028-02-29T05:30:00Z",
                "timeZoneId": "Asia/Kolkata",
                "dateFormat": "DATE_FORMAT_MONTH_DAY_YEAR_ABBREVIATED",
                "timeFormat": "TIME_FORMAT_HOUR_MINUTE",
                "displayText": "Feb 29, 2028, 11:00 AM"
            }
        }
    }] }} )]));
    assert!(matches!(
        &report.document.blocks[0].content[..],
        [Inline::Text { text, marks, .. }]
            if text == "Feb 29, 2028, 11:00 AM"
                && marks.iter().any(|mark| mark.kind == opendoc_core::MarkKind::Bold)
    ));
    assert!(has(&report.warnings, "google-date-element-degraded"));
}

#[test]
fn unknown_elements_and_structures_are_reported_by_name_and_survive() {
    let report = import(&document_with(vec![
        json!({ "paragraph": { "elements": [
            { "textRun": { "content": "Before" } },
            { "somethingGoogleAddedLater": { "id": "x" } }
        ]}}),
        json!({ "aBrandNewStructuralElement": { "id": "y" } }),
        json!({ "paragraph": { "elements": [{ "textRun": { "content": "After" } }] } }),
    ]));
    assert_eq!(report.document.visible_text(), "Before\nAfter\n");
    assert!(
        message(&report.warnings, "google-dropped-paragraph-element")
            .contains("somethingGoogleAddedLater")
    );
    assert!(
        message(&report.warnings, "google-dropped-structural-element")
            .contains("aBrandNewStructuralElement")
    );
}

#[test]
fn a_native_table_of_contents_becomes_a_live_generated_block_not_stale_entry_text() {
    let report = import(&document_with(vec![
        paragraph(json!({ "namedStyleType": "HEADING_1" }), "Introduction"),
        json!({ "tableOfContents": { "content": [
            { "paragraph": { "elements": [{ "textRun": { "content": "1. Introduction" } }] } }
        ]}}),
    ]));
    assert_eq!(report.document.visible_text(), "Introduction\n");
    assert!(matches!(
        report.document.blocks[1].kind,
        BlockKind::TableOfContents { max_level: 3 }
    ));
    assert!(message(&report.warnings, "google-toc-scope-projected").contains("Heading 1--3"));
}

#[test]
fn malformed_native_table_of_contents_content_is_not_silently_accepted() {
    let err = import_err(&document_with(vec![json!({
        "tableOfContents": { "content": { "not": "an array" } }
    })]));
    assert!(matches!(err, ImportError::InvalidInput(message) if message.contains("content")));
}

#[test]
fn title_and_subtitle_named_styles_are_durable_non_outline_blocks() {
    let report = import(&document_with(vec![
        paragraph(json!({ "namedStyleType": "TITLE" }), "Title"),
        paragraph(json!({ "namedStyleType": "SUBTITLE" }), "Subtitle"),
    ]));
    assert!(matches!(report.document.blocks[0].kind, BlockKind::Title));
    assert!(matches!(
        report.document.blocks[1].kind,
        BlockKind::Subtitle
    ));
    assert!(report.warnings.is_empty());
}

#[test]
fn identical_warnings_collapse_instead_of_repeating_once_per_paragraph() {
    let content = (0..5)
        .map(|_| paragraph(json!({ "tabStops": [{ "offset": pt(36.0) }] }), "Row"))
        .collect::<Vec<_>>();
    let report = import(&document_with(content));
    assert_eq!(
        report
            .warnings
            .iter()
            .filter(|warning| warning.code == "google-dropped-paragraph-style")
            .count(),
        1,
        "{:?}",
        report.warnings
    );
}
