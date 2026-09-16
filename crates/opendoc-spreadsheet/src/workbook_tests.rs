//! Tests for what a cell stores when a user types into it, and for the
//! dependency graph the workbook keeps alongside the values.

use crate::SpreadsheetWorkbook;

fn workbook() -> SpreadsheetWorkbook {
    let mut workbook = SpreadsheetWorkbook::empty("Typed");
    workbook.add_sheet_with_id("sheet-1", "Sheet1");
    workbook
}

fn set(workbook: &mut SpreadsheetWorkbook, address: &str, value: &str) {
    workbook
        .set_cell_in_sheet("sheet-1", address, value.to_string())
        .expect("sheet exists");
}

fn cell(workbook: &SpreadsheetWorkbook, address: &str) -> crate::Cell {
    workbook.sheets[0]
        .cells
        .iter()
        .find(|cell| cell.address == address)
        .cloned()
        .unwrap_or_else(|| panic!("cell {address} exists"))
}

/// A cell's *source* is the value the user named, not the characters they
/// typed to name it. `50%` names the number 0.5 shown as a percentage, and
/// the cell stores those as two separate facts.
#[test]
fn typed_input_is_stored_as_a_value_plus_the_format_it_implied() {
    let mut workbook = workbook();
    set(&mut workbook, "A1", "50%");
    set(&mut workbook, "A2", "$5");
    set(&mut workbook, "A3", "1,000");
    set(&mut workbook, "A4", "2024-01-05");
    set(&mut workbook, "A5", "12");
    set(&mut workbook, "A6", "hello");
    set(&mut workbook, "A7", "TRUE");
    set(&mut workbook, "A8", "=1+1");
    set(&mut workbook, "A9", "'0123");
    set(&mut workbook, "A10", "");

    let percent = cell(&workbook, "A1");
    assert_eq!(
        (percent.user_kind.as_str(), percent.user_value.as_str()),
        ("number", "0.5")
    );
    assert_eq!(percent.format.number_format.as_deref(), Some("0%"));

    let currency = cell(&workbook, "A2");
    assert_eq!(
        (currency.user_kind.as_str(), currency.user_value.as_str()),
        ("number", "5")
    );
    assert_eq!(currency.format.number_format.as_deref(), Some("$#,##0"));

    let grouped = cell(&workbook, "A3");
    assert_eq!(
        (grouped.user_kind.as_str(), grouped.user_value.as_str()),
        ("number", "1000")
    );
    assert_eq!(grouped.format.number_format.as_deref(), Some("#,##0"));

    // A date is its serial number in the 1899-12-30 epoch.
    let date = cell(&workbook, "A4");
    assert_eq!(
        (date.user_kind.as_str(), date.user_value.as_str()),
        ("number", "45296")
    );
    assert!(date.format.number_format.is_some());

    // Plain input keeps its text and gains no format of its own.
    let plain = cell(&workbook, "A5");
    assert_eq!(
        (plain.user_kind.as_str(), plain.user_value.as_str()),
        ("number", "12")
    );
    assert_eq!(plain.format.number_format, None);
    let text = cell(&workbook, "A6");
    assert_eq!(
        (text.user_kind.as_str(), text.user_value.as_str()),
        ("string", "hello")
    );
    let boolean = cell(&workbook, "A7");
    assert_eq!(
        (boolean.user_kind.as_str(), boolean.user_value.as_str()),
        ("bool", "true")
    );
    let formula = cell(&workbook, "A8");
    assert_eq!(
        (formula.user_kind.as_str(), formula.user_value.as_str()),
        ("formula", "=1+1")
    );
    // A leading apostrophe forces text and is not part of the value.
    let forced = cell(&workbook, "A9");
    assert_eq!(
        (forced.user_kind.as_str(), forced.user_value.as_str()),
        ("string", "0123")
    );
    assert_eq!(cell(&workbook, "A10").user_kind, "empty");
}

/// The symptom the missing parse produced: arithmetic on typed input.
#[test]
fn typed_percentages_sum_and_typed_dates_add() {
    let mut workbook = workbook();
    set(&mut workbook, "A1", "50%");
    set(&mut workbook, "A2", "25%");
    set(&mut workbook, "B1", "2024-01-05");
    set(&mut workbook, "C1", "=SUM(A1:A2)");
    set(&mut workbook, "C2", "=B1+1");
    set(&mut workbook, "C3", "=A1*200");
    workbook.evaluate();

    assert_eq!(cell(&workbook, "C1").computed_value, "0.75");
    assert_eq!(cell(&workbook, "C2").computed_value, "45297");
    assert_eq!(cell(&workbook, "C3").computed_value, "100");
}

/// `display_value` is derived from the stored number and the stored format,
/// so the cell shows back what was typed.
#[test]
fn the_display_value_shows_typed_input_back() {
    let mut workbook = workbook();
    set(&mut workbook, "A1", "50%");
    set(&mut workbook, "A2", "1,000");
    workbook.evaluate();

    assert_eq!(cell(&workbook, "A1").display_value, "50%");
    assert_eq!(cell(&workbook, "A2").display_value, "1,000");
}

/// A column deliberately formatted one way is not silently retyped by the
/// next thing entered into it.
#[test]
fn an_inferred_format_never_overwrites_one_the_cell_already_has() {
    let mut workbook = workbook();
    workbook
        .set_cell_format("sheet-1", "A1", "number_format", "0.000".to_string())
        .expect("sheet exists")
        .expect("format is valid");
    set(&mut workbook, "A1", "1,000");

    assert_eq!(cell(&workbook, "A1").user_value, "1000");
    assert_eq!(
        cell(&workbook, "A1").format.number_format.as_deref(),
        Some("0.000")
    );
}

/// A column of running totals: `B1 = A1`, `Bn = B(n-1) + An`.
fn running_totals(rows: usize) -> SpreadsheetWorkbook {
    let mut workbook = workbook();
    for row in 1..=rows {
        set(&mut workbook, &format!("A{row}"), &row.to_string());
        let formula = if row == 1 {
            "=A1".to_string()
        } else {
            format!("=B{}+A{}", row - 1, row)
        };
        set(&mut workbook, &format!("B{row}"), &formula);
    }
    workbook
}

fn graph_entry<'a>(workbook: &'a SpreadsheetWorkbook, address: &str) -> &'a crate::CellDependency {
    workbook
        .dependency_graph
        .iter()
        .find(|entry| entry.address == address)
        .unwrap_or_else(|| panic!("graph entry for {address}"))
}

/// The graph is derived from formula source, so it must track edits — and
/// only edits. An evaluation that changed no source leaves it alone.
#[test]
fn the_dependency_graph_tracks_formula_source_across_edits() {
    let mut workbook = running_totals(4);
    workbook.evaluate();

    assert_eq!(graph_entry(&workbook, "B3").dependencies, vec!["A3", "B2"]);
    assert_eq!(graph_entry(&workbook, "B2").dependents, vec!["B3"]);
    let after_first = workbook.dependency_graph.clone();

    // A no-op evaluation changes nothing.
    workbook.evaluate();
    assert_eq!(workbook.dependency_graph, after_first);

    // Editing a value, not a formula, also leaves the graph alone.
    set(&mut workbook, "A1", "99");
    workbook.evaluate();
    assert_eq!(workbook.dependency_graph, after_first);
    assert_eq!(cell(&workbook, "B4").computed_value, "108");

    // Editing a formula rewrites it.
    set(&mut workbook, "B3", "=A3*2");
    workbook.evaluate();
    assert_eq!(graph_entry(&workbook, "B3").dependencies, vec!["A3"]);
    assert_eq!(cell(&workbook, "B4").computed_value, "10");
}

/// The graph used to carry, per node, the whole transitive set of cells
/// that depend on it — O(n²) to build and O(n²) to serialize, for a field
/// nothing read. A 2,000-row chain serialized to 31 MB, 94% of it that
/// field.
#[test]
fn the_dependency_graph_does_not_grow_quadratically() {
    let mut workbook = running_totals(2_000);
    let started = std::time::Instant::now();
    workbook.evaluate();
    let first = started.elapsed();
    let started = std::time::Instant::now();
    workbook.evaluate();
    let repeat = started.elapsed();

    assert_eq!(workbook.dependency_graph.len(), 4_000);
    let serialized = serde_json::to_string(&workbook).expect("workbook serializes");
    assert!(
        serialized.len() < 4 * 1_048_576,
        "2,000-row workbook serialized to {} bytes",
        serialized.len()
    );
    // Generous ceilings: the measured figures are ~50 ms and ~17 ms, and
    // the quadratic they replace was 2.5 s for both.
    assert!(first.as_millis() < 1_000, "first evaluate took {first:?}");
    assert!(
        repeat.as_millis() < 1_000,
        "repeat evaluate took {repeat:?}"
    );
}

/// A rule marked `strict` is the sheet saying *reject this entry*. Before
/// this, `set_cell` never read `Cell::validation` at all, so a strict rule
/// enforced nothing while the dropdown, the Google Sheets export and the
/// audit view all went on advertising it.
#[test]
fn a_strict_validation_refuses_the_entry_it_names() {
    let mut workbook = workbook();
    workbook
        .set_cell_validation(
            "sheet-1",
            "A1",
            crate::CellValidation::new("number_greater", vec!["10".to_string()], true)
                .expect("a valid rule"),
        )
        .expect("the sheet exists");

    let refused = workbook.set_cell_in_sheet("sheet-1", "A1", "1".to_string());
    assert!(
        refused.is_err(),
        "a strict number_greater 10 accepted the value 1"
    );
    assert_eq!(
        cell(&workbook, "A1").user_value,
        "",
        "the refused value was stored anyway"
    );

    workbook
        .set_cell_in_sheet("sheet-1", "A1", "11".to_string())
        .expect("11 satisfies the rule");
    assert_eq!(cell(&workbook, "A1").user_value, "11");
}

/// A rule that is *not* strict is advisory, and must not block anything.
#[test]
fn a_non_strict_validation_still_accepts_everything() {
    let mut workbook = workbook();
    workbook
        .set_cell_validation(
            "sheet-1",
            "A1",
            crate::CellValidation::new("number_greater", vec!["10".to_string()], false)
                .expect("a valid rule"),
        )
        .expect("the sheet exists");

    workbook
        .set_cell_in_sheet("sheet-1", "A1", "1".to_string())
        .expect("an advisory rule does not block");
    assert_eq!(cell(&workbook, "A1").user_value, "1");
}

/// Only the anchor of a merged block is drawn. A value written into a cell
/// the block covers is stored, signed and exported, and appears nowhere —
/// so the write is refused, and the refusal names the anchor.
#[test]
fn a_cell_a_merge_covers_is_not_writable() {
    let mut workbook = workbook();
    workbook
        .merge_cells("sheet-1", "B2:C3")
        .expect("the sheet exists")
        .expect("the merge is accepted");

    let refused = workbook.set_cell_in_sheet("sheet-1", "C2", "999".to_string());
    assert!(refused.is_err(), "C2 is covered by B2:C3 and never drawn");
    assert!(
        format!("{:?}", refused.unwrap_err()).contains("B2"),
        "the refusal must name the anchor the caller meant"
    );

    workbook
        .set_cell_in_sheet("sheet-1", "B2", "999".to_string())
        .expect("the anchor is writable");
    assert_eq!(cell(&workbook, "B2").user_value, "999");
}

/// Merging over content keeps only the anchor's, as Sheets does. Leaving the
/// rest in place is what made a merge strand invisible data.
#[test]
fn merging_discards_the_content_of_the_cells_it_covers() {
    let mut workbook = workbook();
    workbook
        .set_cell_in_sheet("sheet-1", "B2", "keep".to_string())
        .expect("the sheet exists");
    workbook
        .set_cell_in_sheet("sheet-1", "C2", "stranded".to_string())
        .expect("the sheet exists");

    workbook
        .merge_cells("sheet-1", "B2:C3")
        .expect("the sheet exists")
        .expect("the merge is accepted");

    assert_eq!(cell(&workbook, "B2").user_value, "keep");
    assert_eq!(
        cell(&workbook, "C2").user_value,
        "",
        "the covered cell kept content nothing will ever draw"
    );
}
