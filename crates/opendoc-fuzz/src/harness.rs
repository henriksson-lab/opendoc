//! The in-gate driver: run every target over the generated corpus on stable.
//!
//! `cargo fuzz` is better at finding new inputs, and it needs nightly. This
//! harness is what makes the targets part of `cargo test --release --workspace`
//! so a regression in a cap, or a new panic on a truncated archive, fails the
//! same gate as everything else. The corpus is a pure function of the seeds,
//! so a failure names a seed and is replayable.

use crate::corpus;
use crate::rng::Rng;
use crate::targets;

/// Mutations per reader. Each one is a fresh parse of a whole package, so this
/// is a balance between coverage and the time the gate is allowed to take.
const MUTATIONS: u64 = 400;

fn drive(label: &str, seed: &[u8], target: fn(&[u8])) {
    assert!(
        !seed.is_empty(),
        "{label}: the seed corpus is empty, so every mutation below is a mutation of nothing"
    );
    // The seed itself must parse, or the mutations are not mutations of a
    // valid file and the whole run is exercising the reject path.
    target(seed);
    for index in 0..MUTATIONS {
        let mut rng = Rng::new(index ^ 0x5eed_0000_0000_0000);
        let input = corpus::mutate(seed, &mut rng);
        target(&input);
    }
    // …and a few inputs that are not mutations of anything.
    for index in 0..MUTATIONS / 4 {
        let mut rng = Rng::new(index ^ 0x0bad_0000_0000_0000);
        let len = rng.below(4096);
        let bytes: Vec<u8> = (0..len).map(|_| rng.byte()).collect();
        target(&bytes);
    }
    target(&[]);
    target(b"PK\x03\x04");
    target(b"{");
}

#[test]
fn the_docx_reader_survives_mutations_of_a_real_package() {
    drive("docx", &corpus::docx_seed(), targets::docx);
}

#[test]
fn the_xlsx_reader_survives_mutations_of_a_real_package() {
    drive("xlsx", &corpus::xlsx_seed(), targets::xlsx);
}

// ---------------------------------------------------------------------------
// the shapes the proofs of concept used
//
// Random mutation never builds these. They are the inputs the caps exist for,
// and each one asserts the *refusal*, not merely the survival: a reader that
// accepted them would satisfy "it did not crash".
// ---------------------------------------------------------------------------

#[test]
fn a_docx_nested_past_the_xml_depth_cap_is_refused_rather_than_walked() {
    // 40,000 nested elements in about 1.6 KB used to abort the process with a
    // stack overflow — a SIGABRT, so not even a catchable panic.
    for depth in [512, 4_096, 40_000] {
        let bytes = corpus::docx_nested_elements(depth);
        assert!(
            bytes.len() < 200_000,
            "the {depth}-deep package is {} bytes, which is not the hazard",
            bytes.len()
        );
        targets::docx(&bytes);
        assert!(
            opendoc_import::import_docx_bytes("fuzz", &bytes).is_err(),
            "a package nested {depth} deep was accepted"
        );
    }
    // …and something inside the cap still reads, so the refusal is the depth
    // and not the shape.
    let shallow = corpus::docx_nested_elements(8);
    targets::docx(&shallow);
}

#[test]
fn a_docx_with_tables_nested_past_the_converter_cap_is_refused() {
    let bytes = corpus::docx_nested_tables(64);
    targets::docx(&bytes);
    if let Ok(report) = opendoc_import::import_docx_bytes("fuzz", &bytes) {
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.code.contains("depth") || warning.code.contains("nest")),
            "64 nested tables were imported with no warning about depth: {:?}",
            report.warnings
        );
    }
}

#[test]
fn a_docx_part_that_inflates_past_the_budget_is_refused() {
    // Sixteen megabytes of one byte compresses to a few kilobytes. The reader
    // has to decide on the *declared* size, before inflating.
    let bytes = corpus::docx_inflation_bomb(16 * 1024 * 1024);
    assert!(
        bytes.len() < 200_000,
        "the bomb is {} bytes, which is not the hazard",
        bytes.len()
    );
    targets::docx(&bytes);
}

#[test]
fn a_docx_with_more_entries_than_the_cap_is_refused() {
    let bytes = corpus::docx_many_entries(20_000);
    targets::docx(&bytes);
    assert!(
        opendoc_import::import_docx_bytes("fuzz", &bytes).is_err(),
        "a package with 20,000 entries was accepted"
    );
}

#[test]
fn an_xlsx_declaring_an_absurd_extent_is_refused_rather_than_allocated() {
    // A few hundred bytes describing a sheet a million rows tall. Nothing here
    // is large except the numbers.
    for (column, row) in [("A", 1_000_000u64), ("ZZ", 200_000), ("A", 10_001)] {
        let bytes = corpus::xlsx_far_cell(column, row);
        assert!(
            bytes.len() < 200_000,
            "the {column}{row} package is {} bytes, which is not the hazard",
            bytes.len()
        );
        targets::xlsx(&bytes);
    }
}

#[test]
fn an_xlsx_nested_past_the_xml_depth_cap_is_refused() {
    for depth in [512, 40_000] {
        let bytes = corpus::xlsx_nested_elements(depth);
        targets::xlsx(&bytes);
    }
}

#[test]
fn an_xlsx_part_that_inflates_past_the_budget_is_refused() {
    let bytes = corpus::xlsx_inflation_bomb(16 * 1024 * 1024);
    assert!(bytes.len() < 200_000);
    targets::xlsx(&bytes);
}

#[test]
fn google_json_nested_past_the_recursion_limit_is_refused_rather_than_walked() {
    // The importer walks structural elements recursively, so the nesting depth
    // of the JSON is the depth of the recursion. `serde_json` refuses past its
    // own recursion limit, which is the first line of defence; this asserts
    // that the answer is a refusal rather than a stack overflow.
    for depth in [64, 1_024, 40_000] {
        let bytes = corpus::google_json_nested_tables(depth);
        targets::google_json(&bytes);
        if depth >= 1_024 {
            assert!(
                opendoc_import::import_google_docs_json("fuzz", &bytes).is_err(),
                "JSON nested {depth} deep was accepted"
            );
        }
    }
}

#[test]
fn a_very_wide_google_json_document_is_bounded_by_its_own_size() {
    let bytes = corpus::google_json_wide(20_000);
    targets::google_json(&bytes);
}

#[test]
fn google_json_carrying_absurd_numbers_does_not_overflow_or_panic() {
    targets::google_json(&corpus::google_json_absurd_numbers());
}

// ---------------------------------------------------------------------------
// a defect this target found, now fixed and kept from coming back
// ---------------------------------------------------------------------------

/// **A crafted `.xlsx` is refused, not aborted.**
///
/// `opendoc-spreadsheet/src/io.rs` used to open a workbook with
/// `let _ = reader.load_merged_regions();`, discarding the error. `calamine`
/// 0.31 keeps the merged-region table as an `Option` and `expect()`s it on
/// the next `worksheet_range` call, so any workbook that made
/// `load_merged_regions` fail panicked rather than returning
/// `SpreadsheetError::Import`. A package whose relationships name
/// `xl/sharedStrings.xml` while the part is absent is one — and a single
/// flipped byte in a zip entry name produces it, which is how this was found.
///
/// Why it mattered beyond tidiness: a panic in the browser core is a module
/// trap the page cannot recover from, and in the native shell it unwinds out
/// of a Tauri command. Opening a file someone sent you is the reachability.
///
/// The fix propagates the error. This test is the guard: both crafted
/// packages must come back as a clean refusal, and neither may panic.
#[test]
fn a_workbook_whose_shared_strings_part_is_gone_is_refused_not_aborted() {
    for (label, bytes) in [
        (
            "the part renamed",
            corpus::xlsx_with_misnamed_shared_strings(),
        ),
        ("the part removed", corpus::xlsx_without_shared_strings()),
    ] {
        let encoded = corpus::base64_of(&bytes);
        let outcome = targets::catch(|| {
            opendoc_spreadsheet::SpreadsheetWorkbook::from_xlsx_base64(&encoded, "fuzz").is_err()
        });
        match outcome {
            Ok(refused) => assert!(
                refused,
                "{label}: a workbook naming a part that is not there was accepted"
            ),
            Err(message) => panic!("{label}: the reader panicked instead of refusing: {message}"),
        }
    }
}
