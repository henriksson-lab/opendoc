//! Spreadsheet engine: formula language, formatting, recalculation,
//! structural edits, and interchange. The public workbook/sheet/cell types
//! stay in the crate root; these modules implement their behaviour.

pub mod format;
pub mod formula;
pub mod functions;
pub mod io;
pub mod recalc;
pub mod structure;
pub mod value;
