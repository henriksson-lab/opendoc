//! Google Docs JSON paragraph-formatting and list fidelity.
//!
//! Every property that can survive is asserted through import → export →
//! import, because a mapping that only works in one direction is not fidelity.
//! Every property that cannot survive is asserted to say so in a warning.

use crate::{export_google_docs_json_with_warnings, import_google_docs_json, ImportError};
use opendoc_core::{
    Alignment, Block, BlockKind, BlockProperties, Document, Length, LineSpacing, ListKind,
    ModelWarning, StableId, TextDirection,
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
            "spaceBelow": pt(6.0)
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
    };
    assert_eq!(report.document.blocks[0].properties, expected);
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);

    let (round_tripped, warnings) = round_trip(&report.document);
    assert_eq!(round_tripped.blocks[0].properties, expected);
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
        "borderTop",
        "shading",
        "tabStops",
        "keepWithNext",
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
    for glyph in [
        "DECIMAL",
        "ZERO_DECIMAL",
        "ALPHA",
        "UPPER_ALPHA",
        "ROMAN",
        "UPPER_ROMAN",
    ] {
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
fn document_style_headers_footers_and_named_styles_are_named_in_warnings() {
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
        "page setup",
        "page headers",
        "page footers",
        "named style definitions",
        "inline objects",
        "positioned objects",
    ] {
        assert!(
            messages.iter().any(|message| message.contains(expected)),
            "{expected} not reported in {messages:?}"
        );
    }
    assert_eq!(report.document.visible_text(), "Body\n");
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
fn section_styling_is_reported_while_the_break_itself_is_kept() {
    let report = import(&json!({
        "body": { "content": [
            { "sectionBreak": { "sectionStyle": { "columnProperties": [{}, {}] } } }
        ]}
    }));
    assert!(matches!(
        report.document.blocks[0].kind,
        BlockKind::PageBreak
    ));
    assert!(message(&report.warnings, "google-dropped-document-part").contains("section styling"));
}

// ---------------------------------------------------------------------------
// FS-10: real-world documents must not abort
// ---------------------------------------------------------------------------

#[test]
fn understood_but_unrepresentable_elements_degrade_instead_of_aborting() {
    for (element, fragment) in [
        (json!({ "horizontalRule": {} }), "horizontal rule"),
        (json!({ "columnBreak": {} }), "column break"),
        (
            json!({ "autoText": { "type": "PAGE_NUMBER" } }),
            "auto text",
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
fn person_chips_and_rich_links_keep_their_text() {
    let report = import(&document_with(vec![json!({ "paragraph": { "elements": [
        { "person": { "personProperties": { "name": "Ada Lovelace", "email": "ada@example.invalid" } } },
        { "richLink": { "richLinkProperties": { "title": "Spec", "uri": "https://example.invalid/spec" } } }
    ]}})]));
    let content = &report.document.blocks[0].content;
    assert!(matches!(
        &content[0],
        opendoc_core::Inline::Mention { label, .. } if label == "Ada Lovelace"
    ));
    assert!(matches!(
        &content[1],
        opendoc_core::Inline::Link { text, href, .. }
            if text == "Spec" && href == "https://example.invalid/spec"
    ));
    assert!(has(&report.warnings, "google-degraded-paragraph-element"));
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
fn a_table_of_contents_is_flattened_rather_than_aborting() {
    let report = import(&document_with(vec![
        json!({ "tableOfContents": { "content": [
            { "paragraph": { "elements": [{ "textRun": { "content": "1. Introduction" } }] } }
        ]}}),
    ]));
    assert_eq!(report.document.visible_text(), "1. Introduction\n");
    assert!(message(&report.warnings, "google-dropped-structural-element").contains("flattened"));
}

#[test]
fn a_named_style_with_no_opendoc_equivalent_is_reported() {
    let report = import(&document_with(vec![paragraph(
        json!({ "namedStyleType": "SUBTITLE" }),
        "Subtitle",
    )]));
    assert!(matches!(
        report.document.blocks[0].kind,
        BlockKind::Paragraph
    ));
    assert!(message(&report.warnings, "google-dropped-named-style").contains("SUBTITLE"));
}

#[test]
fn identical_warnings_collapse_instead_of_repeating_once_per_paragraph() {
    let content = (0..5)
        .map(|_| paragraph(json!({ "keepWithNext": true }), "Row"))
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
