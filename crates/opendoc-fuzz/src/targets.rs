//! The targets: one `fn(&[u8])` per reader, each asserting what that reader
//! promises for *every* input.
//!
//! These are shared by the in-gate harness and by the `cargo fuzz` entry
//! points in `fuzz/`, so the two cannot drift apart.

use std::time::{Duration, Instant};

use opendoc_core::{Document, ModelWarning};
use opendoc_spreadsheet::SpreadsheetWorkbook;

/// The XLSX import caps, read from `opendoc-spreadsheet` itself.
///
/// They used to be copied here, because they lived behind a private module —
/// and the copy had already fallen behind (1,024 columns against the real
/// 1,000). They are re-exported now, so the assertions below are against the
/// numbers the importer actually enforces.
const XLSX_IMPORT_MAX_ROWS: usize = opendoc_spreadsheet::XLSX_IMPORT_MAX_ROWS as usize;
const XLSX_IMPORT_MAX_COLUMNS: usize = opendoc_spreadsheet::XLSX_IMPORT_MAX_COLUMNS as usize;

/// How long any one input may take.
///
/// Generous on purpose: this is a denial-of-service tripwire, not a benchmark.
/// The XLSX proof of concept took minutes from a few hundred bytes and the
/// DOCX one aborted the process; either would blow through five seconds.
pub const BUDGET: Duration = Duration::from_secs(5);

/// The largest document any reader may hand back from a fuzz input.
///
/// A parser that answers a small file with a huge document is a denial of
/// service on everything downstream, whether or not it "crashed".
const MAX_BLOCKS: usize = 200_000;

/// What a reader must answer identically for identical bytes.
///
/// Not the whole `Document`: every import mints fresh identities — a new
/// `DocumentUuid` and a fresh `StableId` per block and inline — so two imports
/// of one file are never `==`. That is deliberate (an import is a new
/// document, not a restored one) and it is also the reason a determinism
/// assertion has to be about *content*. Everything below is content.
fn shape_of(document: &Document, warnings: &[ModelWarning]) -> (String, usize, Vec<String>) {
    (
        document.visible_text(),
        document.blocks.len(),
        warnings
            .iter()
            .map(|warning| warning.code.clone())
            .collect(),
    )
}

#[track_caller]
fn within_budget<T>(label: &str, bytes: &[u8], call: impl FnOnce() -> T) -> T {
    let started = Instant::now();
    let value = call();
    let elapsed = started.elapsed();
    assert!(
        elapsed <= BUDGET,
        "{label}: {} bytes took {elapsed:?}, over the {BUDGET:?} budget",
        bytes.len()
    );
    value
}

/// `opendoc_import::import_docx_bytes`.
pub fn docx(bytes: &[u8]) {
    let first = within_budget("docx", bytes, || {
        opendoc_import::import_docx_bytes("fuzz", bytes)
    });
    if let Ok(report) = &first {
        report
            .document
            .validate()
            .expect("a DOCX import that succeeded produced an invalid document");
        assert!(
            report.document.blocks.len() <= MAX_BLOCKS,
            "a {}-byte DOCX produced {} blocks",
            bytes.len(),
            report.document.blocks.len()
        );
    }
    // Same bytes, same answer. A reader whose result depends on allocator
    // addresses, hashing order or a clock cannot be content addressed.
    let second = opendoc_import::import_docx_bytes("fuzz", bytes);
    assert_eq!(
        first.is_ok(),
        second.is_ok(),
        "DOCX import was not deterministic"
    );
    if let (Ok(left), Ok(right)) = (&first, &second) {
        assert_eq!(
            shape_of(&left.document, &left.warnings),
            shape_of(&right.document, &right.warnings),
            "DOCX import was not deterministic"
        );
    }
}

/// Run `call`, returning `Err(message)` if it panicked.
pub fn catch(call: impl FnOnce() -> bool + std::panic::UnwindSafe) -> Result<bool, String> {
    std::panic::catch_unwind(call).map_err(|payload| {
        payload
            .downcast_ref::<&str>()
            .map(|text| (*text).to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "<non-string panic payload>".to_string())
    })
}

/// `SpreadsheetWorkbook::from_xlsx_base64`, which is the whole public XLSX
/// reading surface — the browser shell uploads base64 and the native shell
/// reads a file and encodes it, so both arrive here.
pub fn xlsx(bytes: &[u8]) {
    let encoded = crate::corpus::base64_of(bytes);
    // No panic is tolerated any more. This target used to allow exactly two
    // messages from `calamine`, because `io.rs` discarded the error from
    // `load_merged_regions` and `calamine` turned that into an `expect()` two
    // calls later. That is fixed — the reader returns `SpreadsheetError`
    // instead — so a panic here is a defect, whatever its message.
    if let Err(message) = catch(|| SpreadsheetWorkbook::from_xlsx_base64(&encoded, "fuzz").is_ok())
    {
        panic!(
            "a {} byte XLSX panicked instead of being refused: {message}",
            bytes.len()
        );
    }
    let first = within_budget("xlsx", bytes, || {
        SpreadsheetWorkbook::from_xlsx_base64(&encoded, "fuzz")
    });
    if let Ok(workbook) = &first {
        for sheet in &workbook.sheets {
            // The caps are the fix for the denial of service. This is the
            // assertion that keeps them: accepting the workbook is only
            // correct if it is inside them.
            assert!(
                sheet.rows.len() <= XLSX_IMPORT_MAX_ROWS,
                "sheet {} was accepted with {} rows, over the {XLSX_IMPORT_MAX_ROWS} cap",
                sheet.title,
                sheet.rows.len()
            );
            assert!(
                sheet.columns.len() <= XLSX_IMPORT_MAX_COLUMNS,
                "sheet {} was accepted with {} columns, over the {XLSX_IMPORT_MAX_COLUMNS} cap",
                sheet.title,
                sheet.columns.len()
            );
            assert!(
                sheet.cells.len() <= XLSX_IMPORT_MAX_ROWS * 16,
                "sheet {} was accepted with {} cells",
                sheet.title,
                sheet.cells.len()
            );
        }
    }
    let second = SpreadsheetWorkbook::from_xlsx_base64(&encoded, "fuzz");
    assert_eq!(
        first.is_ok(),
        second.is_ok(),
        "XLSX import was not deterministic"
    );
    if let (Ok(left), Ok(right)) = (&first, &second) {
        assert_eq!(
            left.sheets.len(),
            right.sheets.len(),
            "XLSX import was not deterministic"
        );
        for (left_sheet, right_sheet) in left.sheets.iter().zip(right.sheets.iter()) {
            assert_eq!(
                left_sheet.cells, right_sheet.cells,
                "XLSX import was not deterministic"
            );
        }
    }
}

/// `opendoc_import::import_google_docs_json`.
pub fn google_json(bytes: &[u8]) {
    let first = within_budget("google-json", bytes, || {
        opendoc_import::import_google_docs_json("fuzz", bytes)
    });
    if let Ok(report) = &first {
        report
            .document
            .validate()
            .expect("a Google JSON import that succeeded produced an invalid document");
        assert!(
            report.document.blocks.len() <= MAX_BLOCKS,
            "a {}-byte Google JSON document produced {} blocks",
            bytes.len(),
            report.document.blocks.len()
        );
    }
    let second = opendoc_import::import_google_docs_json("fuzz", bytes);
    assert_eq!(
        first.is_ok(),
        second.is_ok(),
        "Google JSON import was not deterministic"
    );
    if let (Ok(left), Ok(right)) = (&first, &second) {
        assert_eq!(
            shape_of(&left.document, &left.warnings),
            shape_of(&right.document, &right.warnings),
            "Google JSON import was not deterministic"
        );
    }
}
