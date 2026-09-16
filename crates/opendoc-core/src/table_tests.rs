use crate::*;
use std::collections::BTreeSet;

#[test]
fn table_structure_rejects_duplicate_row_and_cell_ids() {
    let row_id = StableId::parse("row-duplicate").unwrap();
    let cell_id = StableId::parse("cell-duplicate").unwrap();
    let mut doc = Document::new("Duplicate table IDs");
    doc.blocks.push(Block {
        id: StableId::parse("table-1").unwrap(),
        kind: BlockKind::Table {
            columns: vec![TableColumn::auto()],
            properties: Default::default(),
            rows: vec![
                TableRow {
                    id: row_id.clone(),
                    height: None,
                    header: false,
                    cells: vec![TableCell {
                        id: StableId::parse("cell-1").unwrap(),
                        span: CellSpan::SINGLE,
                        properties: TableCellProperties::default(),
                        blocks: vec![Block::paragraph("a")],
                    }],
                },
                TableRow {
                    id: row_id,
                    height: None,
                    header: false,
                    cells: vec![TableCell {
                        id: StableId::parse("cell-2").unwrap(),
                        span: CellSpan::SINGLE,
                        properties: TableCellProperties::default(),
                        blocks: vec![Block::paragraph("b")],
                    }],
                },
            ],
        },
        content: Vec::new(),
        properties: BlockProperties::default(),
    });
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument("duplicate table row id"))
    ));

    if let BlockKind::Table { rows, .. } = &mut doc.blocks[0].kind {
        rows[1].id = StableId::parse("row-2").unwrap();
        rows[0].cells[0].id = cell_id.clone();
        rows[0].cells.push(TableCell {
            id: cell_id,
            span: CellSpan::SINGLE,
            properties: TableCellProperties::default(),
            blocks: vec![Block::paragraph("c")],
        });
    }
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument("duplicate table cell id"))
    ));

    if let BlockKind::Table { rows, .. } = &mut doc.blocks[0].kind {
        rows[0].cells.pop();
        rows[1].cells[0].id = rows[0].cells[0].id.clone();
    }
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument("duplicate table cell id"))
    ));
}

/// Builds an `rows`x`columns` grid of single cells whose text is its
/// position, so a test can assert on what survived where.
fn grid(rows: usize, columns: usize) -> Block {
    Block {
        id: StableId::new("block"),
        kind: BlockKind::table(
            (0..rows)
                .map(|row| TableRow {
                    id: StableId::new("row"),
                    height: None,
                    header: false,
                    cells: (0..columns)
                        .map(|column| {
                            TableCell::new(vec![Block::paragraph(format!("r{row}c{column}"))])
                        })
                        .collect(),
                })
                .collect(),
        ),
        content: Vec::new(),
        properties: BlockProperties::default(),
    }
}

fn table_parts(block: &mut Block) -> (&mut Vec<TableColumn>, &mut Vec<TableRow>) {
    match &mut block.kind {
        BlockKind::Table { columns, rows, .. } => (columns, rows),
        _ => panic!("not a table"),
    }
}

#[test]
fn decoded_table_geometry_must_be_a_rectangle() {
    let mut doc = Document::new("Ragged table");
    doc.blocks.push(grid(2, 3));
    doc.validate().expect("a rectangular grid is valid");

    // A row short of a cell is not a grid anyone can edit.
    let (_, rows) = table_parts(&mut doc.blocks[0]);
    rows[1].cells.pop();
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "table row does not have one cell per column"
        ))
    ));

    // ... and neither is a row with a cell too many.
    let (_, rows) = table_parts(&mut doc.blocks[0]);
    rows[1].cells.push(TableCell::empty());
    rows[1].cells.push(TableCell::empty());
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "table row does not have one cell per column"
        ))
    ));

    let (columns, rows) = table_parts(&mut doc.blocks[0]);
    rows[1].cells.pop();
    let duplicate = columns[0].id.clone();
    columns[1].id = duplicate;
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument("duplicate table column id"))
    ));
}

#[test]
fn decoded_table_spans_must_stay_inside_the_grid_and_not_overlap() {
    let mut doc = Document::new("Merged table");
    doc.blocks.push(grid(2, 3));

    let (_, rows) = table_parts(&mut doc.blocks[0]);
    rows[0].cells[0].span = CellSpan::new(2, 2).expect("legal span");
    doc.validate().expect("a 2x2 merge in a 2x3 grid is valid");

    // One column too far.
    let (_, rows) = table_parts(&mut doc.blocks[0]);
    rows[0].cells[2].span = CellSpan::new(1, 2).expect("legal span");
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "table cell span reaches outside the grid"
        ))
    ));

    // Two merges claiming the same position.
    let (_, rows) = table_parts(&mut doc.blocks[0]);
    rows[0].cells[2].span = CellSpan::SINGLE;
    rows[1].cells[1].span = CellSpan::new(1, 2).expect("legal span");
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument("table cell spans overlap"))
    ));

    // A merge starting on a position another merge already covers.
    let (_, rows) = table_parts(&mut doc.blocks[0]);
    rows[1].cells[1].span = CellSpan::SINGLE;
    rows[0].cells[1].span = CellSpan::new(1, 2).expect("legal span");
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument("table cell spans overlap"))
    ));

    assert!(matches!(
        CellSpan::new(0, 1),
        Err(ModelError::InvalidDocument(
            "cell span is outside 1..=4096 in one direction"
        ))
    ));
}

#[test]
fn a_covered_cell_keeps_its_content_but_leaves_the_visible_text() {
    let mut doc = Document::new("Merged table");
    doc.blocks.push(grid(2, 2));
    let (_, rows) = table_parts(&mut doc.blocks[0]);
    rows[0].cells[0].span = CellSpan::new(1, 2).expect("legal span");
    doc.validate().expect("valid");

    assert_eq!(
        table_covered_positions(match &doc.blocks[0].kind {
            BlockKind::Table { rows, .. } => rows,
            _ => panic!("not a table"),
        }),
        BTreeSet::from([(0, 1)])
    );

    let text = doc.visible_text();
    assert!(text.contains("r0c0"), "the merged cell is still shown");
    assert!(
        !text.contains("r0c1"),
        "the covered cell is not on the page: {text}"
    );
    // It is not gone, though: splitting hands it straight back.
    let (_, rows) = table_parts(&mut doc.blocks[0]);
    rows[0].cells[0].span = CellSpan::SINGLE;
    assert!(doc.visible_text().contains("r0c1"));
}

#[test]
fn table_cell_properties_round_trip_every_key() {
    let border = CellBorder::new(
        BorderStyle::Dashed,
        Length::from_points(1.5).expect("legal width"),
        Color::parse("#3366CC").expect("legal colour"),
    )
    .expect("legal border");
    let padding = Length::from_points(4.0).expect("legal padding");
    let properties = [
        TableCellProperty::Background(Color::parse("#fff").expect("legal colour")),
        TableCellProperty::BorderTop(border),
        TableCellProperty::BorderBottom(border),
        TableCellProperty::BorderStart(border),
        TableCellProperty::BorderEnd(border),
        TableCellProperty::VerticalAlignment(VerticalAlignment::Middle),
        TableCellProperty::RowHeader(true),
        TableCellProperty::PaddingTop(padding),
        TableCellProperty::PaddingBottom(padding),
        TableCellProperty::PaddingStart(padding),
        TableCellProperty::PaddingEnd(padding),
    ];
    let keys: Vec<_> = properties.iter().map(|property| property.key()).collect();
    assert_eq!(keys, TableCellPropertyKey::ALL.to_vec());

    let mut set = TableCellProperties::default();
    assert!(set.is_empty());
    for property in properties {
        assert_eq!(set.set(property), None);
        assert_eq!(set.get(property.key()), Some(property));
    }
    assert_eq!(set.iter().count(), TableCellPropertyKey::ALL.len());
    set.validate().expect("smart constructors keep it in range");

    for key in TableCellPropertyKey::ALL {
        assert_eq!(TableCellPropertyKey::parse(key.as_str()), Ok(key));
        assert!(set.clear(key).is_some());
        assert_eq!(set.get(key), None);
    }
    assert!(set.is_empty());
}

#[test]
fn table_style_values_reject_what_they_cannot_represent() {
    assert_eq!(Color::parse("#3366cc").expect("legal").as_hex(), "#3366cc");
    assert_eq!(
        Color::parse("#3af").expect("legal").rgb(),
        (0x33, 0xaa, 0xff)
    );
    for bad in ["3366cc", "#12", "#gggggg", "rebeccapurple", ""] {
        assert!(Color::parse(bad).is_err(), "{bad} should not parse");
    }

    assert!(CellBorder::new(
        BorderStyle::Solid,
        Length::from_points(7.0).expect("legal length"),
        Color::BLACK
    )
    .is_err());
    assert!(CellBorder::new(
        BorderStyle::Solid,
        Length::from_points(-1.0).expect("legal length"),
        Color::BLACK
    )
    .is_err());

    // A decoded value that never met a smart constructor is still caught.
    let mut doc = Document::new("Bad cell style");
    doc.blocks.push(grid(1, 1));
    let (_, rows) = table_parts(&mut doc.blocks[0]);
    rows[0].cells[0].properties.padding_top = Some(Length(-1));
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument("cell padding is negative"))
    ));

    let (columns, rows) = table_parts(&mut doc.blocks[0]);
    rows[0].cells[0].properties.padding_top = None;
    columns[0].width = Some(Length(TableColumn::MIN_WIDTH_TWIPS - 1));
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument(
            "table column width is below 0.1in"
        ))
    ));
}

#[test]
fn an_unstyled_unmerged_cell_serializes_to_nothing_extra() {
    let cell = TableCell::empty();
    let json = serde_json::to_value(&cell).expect("serializable");
    let object = json.as_object().expect("object");
    assert!(!object.contains_key("span"), "{json}");
    assert!(!object.contains_key("properties"), "{json}");
    let decoded: TableCell = serde_json::from_value(json).expect("decodable");
    assert_eq!(decoded, cell);
}

#[test]
fn table_structure_rejects_empty_table_shapes() {
    let mut doc = Document::new("Empty table");
    doc.blocks.push(Block {
        id: StableId::parse("table-empty").unwrap(),
        kind: BlockKind::Table {
            columns: Vec::new(),
            properties: Default::default(),
            rows: Vec::new(),
        },
        content: Vec::new(),
        properties: BlockProperties::default(),
    });
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument("table has no rows"))
    ));

    doc.blocks[0].kind = BlockKind::Table {
        columns: vec![TableColumn::auto()],
        properties: Default::default(),
        rows: vec![TableRow {
            id: StableId::parse("row-empty").unwrap(),
            height: None,
            header: false,
            cells: Vec::new(),
        }],
    };
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument("table row has no cells"))
    ));

    doc.blocks[0].kind = BlockKind::Table {
        columns: vec![TableColumn::auto()],
        properties: Default::default(),
        rows: vec![TableRow {
            id: StableId::parse("row-with-empty-cell").unwrap(),
            height: None,
            header: false,
            cells: vec![TableCell {
                id: StableId::parse("cell-empty").unwrap(),
                span: CellSpan::SINGLE,
                properties: TableCellProperties::default(),
                blocks: Vec::new(),
            }],
        }],
    };
    assert!(matches!(
        doc.validate(),
        Err(ModelError::InvalidDocument("table cell has no blocks"))
    ));
}
