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
