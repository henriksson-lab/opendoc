# `cargo fuzz` entry points

These are the same targets `opendoc-fuzz` runs in the gate, wired to
libFuzzer. They are here **in addition to** the in-gate harness, not instead of
it: `cargo fuzz` needs a nightly toolchain (`-Zsanitizer`, and `libfuzzer-sys`
builds with `-Cpasses=sancov-module`), and the gate — `npm run verify` and CI —
runs stable. A fuzz step the gate cannot run is a step that does not run.

So this directory is **excluded from the workspace**. Nothing in
`cargo test --release --workspace` or `cargo clippy --workspace` touches it, and
it does not need to compile for the gate to be green.

To use it:

```sh
rustup toolchain install nightly
cargo install cargo-fuzz
cd crates/opendoc-fuzz
cargo +nightly fuzz run docx
cargo +nightly fuzz run xlsx
cargo +nightly fuzz run google_json
```

Seed the corpus from this repository's own writers rather than from nothing —
`opendoc_fuzz::corpus::docx_seed()`, `xlsx_seed()` and `google_json_seed()`
produce valid packages, and a mutation of a valid package reaches the parser
while a mutation of noise bounces off the zip header.

A crash found here belongs in `crates/opendoc-fuzz/src/harness.rs` as a named
test, so that the stable gate keeps it fixed.
