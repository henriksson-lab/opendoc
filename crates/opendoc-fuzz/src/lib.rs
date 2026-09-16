//! Fuzz targets for the file parsers.
//!
//! The repository had none. The files named `*fuzz*` elsewhere in the tree are
//! hand-written property tests over *typed operations*; nothing drove a reader
//! with bytes it did not construct itself, and nothing exercised deep nesting,
//! decompression ratio or extreme declared dimensions. That is why the DOCX and
//! XLSX denial-of-service proofs of concept survived a green gate. PLAN88 §7.
//!
//! ## Shape
//!
//! A *target* here is `fn(&[u8])`: it hands the bytes to one reader and asserts
//! the invariants that reader promises. A target never decides whether the
//! bytes are "valid"; it asserts things that must hold for **every** input:
//!
//! 1. the call returns rather than panicking or aborting;
//! 2. it returns within a bounded time, so an input cannot be a denial of
//!    service;
//! 3. an `Ok` result is *within the reader's own caps* — the caps are the fix
//!    for the PoCs, and a fuzz target is what keeps them honest;
//! 4. an `Ok` result is a **valid** document or workbook, so no reader can hand
//!    the rest of the system something the model rejects;
//! 5. the answer is the same for the same bytes twice.
//!
//! Point 3 is the one worth stating plainly: "it did not crash" is not an
//! invariant worth a test on its own, because a reader that accepted a
//! ten-million-row sheet would satisfy it.
//!
//! ## Why not only `cargo fuzz`
//!
//! `cargo fuzz` needs a nightly toolchain (`-Zsanitizer=address`, and
//! `libfuzzer-sys` builds with `-Cpasses=sancov-module`). The gate — `npm run
//! verify` and CI — runs stable, so a nightly-only fuzz step would be a step
//! the gate cannot run, which is exactly the failure mode §7 is about. So the
//! targets live here, driven by [`harness`] on stable inside
//! `cargo test --release --workspace`, over a corpus that is *generated*
//! deterministically rather than checked in: valid packages produced by this
//! repository's own exporters, mutated by a seeded PRNG, plus hand-built
//! adversarial shapes that no random mutation would ever reach (a zip
//! declaring a gigabyte, XML nested past the cap, a sheet declaring a million
//! rows).
//!
//! `fuzz/` holds the `cargo fuzz` entry points for the same targets, for
//! anyone who has nightly. It is excluded from the workspace so the stable gate
//! never tries to build it.

pub mod corpus;
pub mod rng;
pub mod targets;

#[cfg(test)]
mod harness;
