use crate::*;
use std::collections::BTreeSet;

#[test]
fn lengths_are_typed_and_range_checked() {
    assert_eq!(Length::from_points(12.0).unwrap().twips(), 240);
    assert_eq!(Length::from_inches(1.0).unwrap().twips(), 1440);
    assert_eq!(Length::from_twips(240).unwrap().points(), 12.0);
    // Rounds to the twip grid rather than silently keeping a float.
    assert_eq!(Length::from_points(12.031).unwrap().twips(), 241);
    assert!(Length::from_twips(Length::MAX_TWIPS + 1).is_err());
    assert!(Length::from_twips(Length::MIN_TWIPS - 1).is_err());
    assert!(Length::from_points(f64::NAN).is_err());
    assert!(Length::from_points(f64::INFINITY).is_err());
}

#[test]
fn line_spacing_constructors_reject_nonsense() {
    assert_eq!(
        LineSpacing::single(),
        LineSpacing::Multiple(LineHeightMultiple::SINGLE)
    );
    assert_eq!(
        LineSpacing::multiple(1.5).unwrap(),
        LineSpacing::Multiple(LineHeightMultiple::from_thousandths(1_500).unwrap())
    );
    assert!(LineSpacing::multiple(0.0).is_err());
    assert!(LineSpacing::multiple(11.0).is_err());
    assert!(LineSpacing::exactly(Length::ZERO).is_err());
    assert!(LineSpacing::at_least(Length::from_points(-1.0).unwrap()).is_err());
    assert!(LineSpacing::exactly(Length::from_points(14.0).unwrap()).is_ok());
}

#[test]
fn enumerable_property_values_round_trip_through_canonical_names() {
    for alignment in Alignment::ALL {
        assert_eq!(Alignment::parse(alignment.as_str()).unwrap(), alignment);
    }
    assert_eq!(Alignment::parse("left").unwrap(), Alignment::Start);
    assert_eq!(Alignment::parse("right").unwrap(), Alignment::End);
    assert!(Alignment::parse("centre").is_err());
    for direction in TextDirection::ALL {
        assert_eq!(TextDirection::parse(direction.as_str()).unwrap(), direction);
    }
    assert!(TextDirection::parse("sideways").is_err());
    for key in BlockPropertyKey::ALL {
        assert_eq!(BlockPropertyKey::parse(key.as_str()).unwrap(), key);
    }
    assert!(BlockPropertyKey::parse("colour").is_err());
}

#[test]
fn block_property_keys_are_derived_from_their_values() {
    let properties = [
        BlockProperty::Alignment(Alignment::Center),
        BlockProperty::IndentStart(Length::from_points(36.0).unwrap()),
        BlockProperty::IndentEnd(Length::from_points(18.0).unwrap()),
        BlockProperty::IndentFirstLine(Length::from_points(-18.0).unwrap()),
        BlockProperty::LineSpacing(LineSpacing::multiple(1.5).unwrap()),
        BlockProperty::SpaceBefore(Length::from_points(6.0).unwrap()),
        BlockProperty::SpaceAfter(Length::from_points(6.0).unwrap()),
        BlockProperty::Direction(TextDirection::RightToLeft),
        BlockProperty::KeepWithNext(true),
        BlockProperty::Background(Color::parse("#123456").unwrap()),
        BlockProperty::Border(
            CellBorder::new(
                BorderStyle::Solid,
                Length::from_points(1.0).unwrap(),
                Color::parse("#123456").unwrap(),
            )
            .unwrap(),
        ),
    ];
    // One property per key, and no key without a property.
    let keys = properties
        .iter()
        .map(BlockProperty::key)
        .collect::<BTreeSet<_>>();
    assert_eq!(keys.len(), BlockPropertyKey::ALL.len());
    for key in BlockPropertyKey::ALL {
        assert!(keys.contains(&key));
    }

    let mut bag = BlockProperties::default();
    assert!(bag.is_empty());
    for property in properties {
        assert_eq!(bag.set(property), None);
        assert_eq!(bag.get(property.key()), Some(property));
    }
    assert_eq!(bag.iter().count(), BlockPropertyKey::ALL.len());
    assert_eq!(
        bag.set(BlockProperty::Alignment(Alignment::Justify)),
        Some(BlockProperty::Alignment(Alignment::Center))
    );
    assert_eq!(
        bag.clear(BlockPropertyKey::Alignment),
        Some(BlockProperty::Alignment(Alignment::Justify))
    );
    assert_eq!(bag.get(BlockPropertyKey::Alignment), None);
    assert_eq!(bag.clear(BlockPropertyKey::Alignment), None);
}

#[test]
fn hanging_indent_is_a_negative_first_line_indent() {
    let mut bag = BlockProperties::default();
    assert_eq!(bag.hanging_indent(), None);
    bag.set(BlockProperty::IndentFirstLine(
        Length::from_points(18.0).unwrap(),
    ));
    assert_eq!(bag.hanging_indent(), None);
    bag.set(BlockProperty::IndentFirstLine(
        Length::from_points(-18.0).unwrap(),
    ));
    assert_eq!(bag.hanging_indent(), Length::from_points(18.0).ok());
}

#[test]
fn decoded_block_properties_outside_their_range_are_invalid() {
    let mut doc = Document::new("Properties");
    doc.blocks.push(Block::paragraph("indented"));
    doc.blocks[0].properties.indent_start = Length::from_points(36.0).ok();
    doc.validate().unwrap();

    // Smart constructors cannot build these, but a decoded record can.
    doc.blocks[0].properties.indent_start = Some(Length(Length::MAX_TWIPS + 1));
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument("length is outside \u{b1}22in"))
    ));

    doc.blocks[0].properties.indent_start = None;
    doc.blocks[0].properties.space_before = Length::from_points(-6.0).ok();
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument("block spacing is negative"))
    ));

    doc.blocks[0].properties.space_before = None;
    doc.blocks[0].properties.line_spacing = Some(LineSpacing::Multiple(LineHeightMultiple(0)));
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "line height multiple is outside 0.1..=10"
        ))
    ));
}

#[test]
fn empty_block_properties_serialize_to_nothing() {
    let json = serde_json::to_string(&BlockProperties::default()).unwrap();
    assert_eq!(json, "{}");
    let mut bag = BlockProperties::default();
    bag.set(BlockProperty::Alignment(Alignment::Center));
    bag.set(BlockProperty::IndentFirstLine(
        Length::from_points(-18.0).unwrap(),
    ));
    let json = serde_json::to_string(&bag).unwrap();
    assert_eq!(json, r#"{"alignment":"Center","indent_first_line":-360}"#);
    assert_eq!(serde_json::from_str::<BlockProperties>(&json).unwrap(), bag);
}

#[test]
fn list_kinds_are_exhaustive_and_carry_checkbox_state_only_where_it_exists() {
    assert_eq!(ListKind::Bullet.checked(), None);
    assert_eq!(ListKind::Ordered.checked(), None);
    assert_eq!(ListKind::unchecked().checked(), Some(false));
    assert_eq!(
        ListKind::unchecked().with_checked(true),
        ListKind::Checklist { checked: true }
    );
    // A marker without checkbox state ignores it rather than growing one.
    assert_eq!(ListKind::Bullet.with_checked(true), ListKind::Bullet);
    assert!(ListKind::Ordered.is_ordered());
    assert!(!ListKind::unchecked().is_ordered());
    for (marker, kind) in [
        ("bullet", ListKind::Bullet),
        ("ordered", ListKind::Ordered),
        ("checklist", ListKind::Checklist { checked: true }),
    ] {
        assert_eq!(ListKind::parse(marker, true).unwrap(), kind);
        assert_eq!(kind.as_str(), marker);
    }
    assert!(ListKind::parse("roman", false).is_err());
}

#[test]
fn list_items_carry_a_validated_list_identity() {
    let mut doc = Document::new("Lists");
    let first = new_list_id();
    let second = new_list_id();
    assert_ne!(first, second);
    doc.blocks.push(Block {
        id: StableId::new("block"),
        kind: BlockKind::ListItem {
            list_id: first,
            level: 0,
            kind: ListKind::Ordered,
        },
        content: vec![Inline::text("one")],
        properties: BlockProperties::default(),
    });
    doc.validate().unwrap();
    assert_eq!(doc.blocks[0].list_kind(), Some(ListKind::Ordered));
    assert!(doc.blocks[0].list_id().is_some());

    let BlockKind::ListItem { list_id, .. } = &mut doc.blocks[0].kind else {
        unreachable!()
    };
    *list_id = StableId(" spaced ".to_string());
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "stable id has surrounding whitespace"
        ))
    ));
}

#[test]
fn list_numbering_starts_are_owned_by_the_run_and_are_bounded() {
    let mut properties = ListProperties::default();
    properties.ordered_starts.insert(0, 7);
    assert_eq!(properties.start_for(0), 7);
    assert_eq!(properties.start_for(1), 1);
    assert!(properties.validate().is_ok());

    properties.ordered_starts.insert(9, 1);
    assert!(properties.validate().is_err());
    properties.ordered_starts.remove(&9);
    properties.ordered_starts.insert(0, 0);
    assert!(properties.validate().is_err());
}

#[test]
fn ordered_list_formats_are_run_level_source_state_with_a_depth_default() {
    let mut properties = ListProperties::default();
    assert_eq!(properties.format_for(0), OrderedListFormat::Decimal);
    assert_eq!(properties.format_for(1), OrderedListFormat::LowerAlpha);
    properties
        .ordered_formats
        .insert(0, OrderedListFormat::UpperRoman);
    assert_eq!(properties.format_for(0), OrderedListFormat::UpperRoman);
    assert!(properties.validate().is_ok());
    properties
        .ordered_formats
        .insert(9, OrderedListFormat::Decimal);
    assert!(properties.validate().is_err());
}

#[test]
fn custom_bullet_markers_are_bounded_unicode_source_state() {
    let marker = BulletListMarker::parse("→•").expect("safe visible glyphs");
    assert_eq!(marker.glyph(), "→•");
    assert!(matches!(marker, BulletListMarker::Custom(..)));
    assert!(BulletListMarker::parse("bad\nmarker").is_none());
    assert!(BulletListMarker::parse("\\").is_none());
    assert!(BulletListMarker::parse(&"x".repeat(17)).is_none());

    let mut properties = ListProperties::default();
    properties.bullet_markers.insert(0, marker);
    assert!(properties.validate().is_ok());
    properties
        .bullet_markers
        .insert(1, BulletListMarker::Custom("bad\nmarker".into()));
    assert!(properties.validate().is_err());
}

#[test]
fn named_bullet_markers_keep_their_native_string_wire_form() {
    for (marker, wire) in [
        (BulletListMarker::Disc, "\"disc\""),
        (BulletListMarker::Circle, "\"circle\""),
        (BulletListMarker::Square, "\"square\""),
        (BulletListMarker::Custom("→".to_string()), "\"→\""),
    ] {
        let encoded = serde_json::to_string(&marker).expect("serialise marker");
        assert_eq!(encoded, wire);
        let decoded: BulletListMarker = serde_json::from_str(&encoded).expect("decode marker");
        assert_eq!(decoded, marker);
    }
}

#[test]
fn line_spacing_states_its_own_wire_form_and_label() {
    // The mode/value pair and the label are the model's, so a view never has
    // to join them into a string or take one apart again.
    let one_and_a_half = LineSpacing::multiple(1.5).unwrap();
    assert_eq!(one_and_a_half.mode(), "multiple");
    assert_eq!(one_and_a_half.value(), 1_500);
    assert_eq!(one_and_a_half.label(), "1.5\u{d7}");
    assert_eq!(LineSpacing::single().label(), "Single");
    assert_eq!(LineSpacing::multiple(2.0).unwrap().label(), "Double");

    let exact = LineSpacing::exactly(Length::from_points(24.0).unwrap()).unwrap();
    assert_eq!(exact.mode(), "exact");
    assert_eq!(exact.value(), 480);
    assert_eq!(exact.label(), "Exactly 24 pt");
    let at_least = LineSpacing::at_least(Length::from_points(18.5).unwrap()).unwrap();
    assert_eq!(at_least.mode(), "at-least");
    assert_eq!(at_least.label(), "At least 18.5 pt");

    // Every preset survives the round trip its own accessors describe.
    for preset in LineSpacing::PRESETS {
        assert_eq!(
            LineSpacing::parse(preset.mode(), preset.value()).unwrap(),
            preset
        );
        assert!(!preset.label().is_empty());
    }
    assert_eq!(
        LineSpacing::PRESETS.map(|preset| preset.value()),
        [1_000, 1_150, 1_500, 2_000]
    );
    assert!(LineSpacing::parse("roomy", 2).is_err());
    assert!(LineSpacing::parse("multiple", -1).is_err());
    assert!(LineSpacing::parse("exact", 0).is_err());
}

#[test]
fn image_sizes_are_bounded_by_the_model() {
    let layout = |twips: i32| ImageLayout {
        width: Some(Length::from_twips(twips).unwrap()),
        height: None,
        placement: None,
        ..ImageLayout::default()
    };
    assert_eq!(ImageLayout::MIN_TWIPS, 360);
    assert_eq!(ImageLayout::MAX_TWIPS, Length::MAX_TWIPS);
    layout(ImageLayout::MIN_TWIPS).validate().unwrap();
    layout(ImageLayout::MAX_TWIPS).validate().unwrap();
    // A quarter inch is the floor: smaller is invisible rather than small.
    assert!(layout(ImageLayout::MIN_TWIPS - 1).validate().is_err());
    assert!(layout(0).validate().is_err());
    assert!(layout(-720).validate().is_err());
    // No size at all still means "draw it at its intrinsic size".
    ImageLayout::default().validate().unwrap();
}

#[test]
fn image_visual_effects_are_bounded_by_the_model() {
    let valid = ImageLayout {
        rotation_degrees: Some(-360),
        opacity_percent: Some(0),
        ..ImageLayout::default()
    };
    valid.validate().unwrap();
    assert!(ImageLayout {
        rotation_degrees: Some(361),
        ..ImageLayout::default()
    }
    .validate()
    .is_err());
    assert!(ImageLayout {
        opacity_percent: Some(101),
        ..ImageLayout::default()
    }
    .validate()
    .is_err());
}

#[test]
fn positioned_image_is_a_typed_out_of_flow_layout_not_flow_wrapping() {
    let positioned = PositionedImage {
        anchor: PositionedImageAnchor::Block(StableId::parse("paragraph-1").unwrap()),
        horizontal_offset: Length::from_twips(-240).unwrap(),
        vertical_offset: Length::from_twips(480).unwrap(),
        layer: PositionedImageLayer::InFrontOfText,
    };
    let layout = ImageLayout {
        positioned: Some(positioned.clone()),
        ..ImageLayout::default()
    };
    layout.validate().unwrap();
    assert_eq!(
        serde_json::from_str::<ImageLayout>(&serde_json::to_string(&layout).unwrap()).unwrap(),
        layout
    );

    assert!(ImageLayout {
        placement: Some(ImagePlacement::WrapStart),
        positioned: Some(positioned),
        ..ImageLayout::default()
    }
    .validate()
    .is_err());
}

#[test]
fn insert_position_names_the_place_an_anchor_cannot() {
    let anchor = StableId::parse("row-1").unwrap();
    // `Last` is the old `after: None`: append, and degrade to append when the
    // anchor is gone.
    assert_eq!(InsertPosition::Last.index(3, None), 3);
    assert_eq!(InsertPosition::Before(anchor.clone()).index(3, Some(2)), 2);
    assert_eq!(InsertPosition::Before(anchor.clone()).index(3, None), 3);
    assert_eq!(InsertPosition::After(anchor.clone()).index(3, Some(0)), 1);
    assert_eq!(InsertPosition::After(anchor.clone()).index(3, None), 3);
    // `First` is the position an id cannot name, and it never degrades.
    assert_eq!(InsertPosition::First.index(3, None), 0);
    assert_eq!(InsertPosition::First.index(0, None), 0);

    assert_eq!(InsertPosition::First.anchor(), None);
    assert_eq!(InsertPosition::Last.anchor(), None);
    assert_eq!(
        InsertPosition::Before(anchor.clone()).anchor(),
        Some(&anchor)
    );
    assert_eq!(
        InsertPosition::After(anchor.clone()).anchor(),
        Some(&anchor)
    );

    assert_eq!(InsertPosition::parse(None).unwrap(), InsertPosition::Last);
    assert_eq!(
        InsertPosition::parse(Some("  ")).unwrap(),
        InsertPosition::Last
    );
    assert_eq!(
        InsertPosition::parse(Some(InsertPosition::FIRST_KEYWORD)).unwrap(),
        InsertPosition::First
    );
    assert_eq!(
        InsertPosition::parse(Some("row-1")).unwrap(),
        InsertPosition::After(anchor)
    );
    assert!(InsertPosition::parse(Some(" spaced id ")).is_ok());
    assert!(InsertPosition::After(StableId(" bad ".to_string()))
        .validate()
        .is_err());
}

/// The bridge from the older `after: Option<StableId>` spelling.
///
/// It exists so the convention is written down once instead of at every call
/// site that was ported, and the thing it must never do is turn "no anchor"
/// into `First`: `None` meant *append* and still does.
#[test]
fn after_or_last_keeps_no_anchor_meaning_append() {
    let anchor = StableId::parse("row-1").unwrap();
    assert_eq!(
        InsertPosition::after_or_last(Some(anchor.clone())),
        InsertPosition::After(anchor)
    );
    assert_eq!(InsertPosition::after_or_last(None), InsertPosition::Last);
    assert_ne!(InsertPosition::after_or_last(None), InsertPosition::First);
}
